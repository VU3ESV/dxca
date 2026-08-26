//! Built-in telnet cluster server — the Rust counterpart of the Swift
//! `ClusterTCPServer`, whose behaviour is the M2 parity spec: no login,
//! a welcome banner on connect, every broadcast line fanned out to all
//! clients with CRLF, client input read and discarded (it only serves to
//! detect disconnects). The login-capable server lifted from Meridian
//! arrives with per-user telnet feeds (plan §5 phase 2).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

pub const WELCOME: &str = "DX Cluster Server - DXCA\r\n";

pub struct ClusterServer {
    tx: broadcast::Sender<String>,
    clients: Arc<AtomicUsize>,
    local_port: u16,
}

impl ClusterServer {
    /// Bind `port` (0 = ephemeral, see [`Self::local_port`]) and start
    /// accepting. A bind failure surfaces immediately (port-clash honesty).
    pub async fn start(port: u16) -> std::io::Result<ClusterServer> {
        let listener = TcpListener::bind(("0.0.0.0", port)).await?;
        let local_port = listener.local_addr()?.port();
        let (tx, _) = broadcast::channel::<String>(256);
        let clients = Arc::new(AtomicUsize::new(0));

        let accept_tx = tx.clone();
        let accept_clients = clients.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _peer)) = listener.accept().await else {
                    return; // listener died — server shut down
                };
                let rx = accept_tx.subscribe();
                let counter = accept_clients.clone();
                counter.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(async move {
                    let _ = serve_client(stream, rx).await;
                    counter.fetch_sub(1, Ordering::Relaxed);
                });
            }
        });

        Ok(ClusterServer {
            tx,
            clients,
            local_port,
        })
    }

    /// The actually bound port (useful when started with port 0).
    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    /// Fan one cluster line out to every connected client (CRLF appended).
    pub fn broadcast_line(&self, line: &str) {
        // No receivers is fine — send() errs only when nobody listens.
        let _ = self.tx.send(format!("{line}\r\n"));
    }

    pub fn client_count(&self) -> usize {
        self.clients.load(Ordering::Relaxed)
    }
}

async fn serve_client(
    mut stream: TcpStream,
    mut rx: broadcast::Receiver<String>,
) -> std::io::Result<()> {
    stream.write_all(WELCOME.as_bytes()).await?;
    let mut discard = [0u8; 1024];
    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Ok(line) => stream.write_all(line.as_bytes()).await?,
                // Lagged: skip what we missed, keep the client connected.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
            read = stream.read(&mut discard) => match read {
                Ok(0) | Err(_) => return Ok(()), // client went away
                Ok(_) => continue,               // input discarded (1.x parity)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    async fn read_until(stream: &mut TcpStream, needle: &str) -> String {
        let mut got = String::new();
        let mut buf = [0u8; 512];
        while !got.contains(needle) {
            let n = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buf))
                .await
                .expect("timed out")
                .expect("read");
            assert!(n > 0, "server closed early; got {got:?}");
            got.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        got
    }

    #[tokio::test]
    async fn banner_and_fanout_to_all_clients() {
        let server = ClusterServer::start(0).await.expect("bind");
        let port = server.local_port();

        let mut a = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut b = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        read_until(&mut a, "DXCA").await;
        read_until(&mut b, "DXCA").await;

        // Give the server a beat to register both subscriptions.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(server.client_count(), 2);

        server.broadcast_line("DX de TEST:      14074.0   K1JT          FT8 -10 dB 1428Z");
        let got_a = read_until(&mut a, "1428Z").await;
        let got_b = read_until(&mut b, "1428Z").await;
        assert!(got_a.contains("DX de TEST:"));
        assert!(got_b.ends_with("1428Z\r\n"));

        drop(a);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(server.client_count(), 1);
    }
}
