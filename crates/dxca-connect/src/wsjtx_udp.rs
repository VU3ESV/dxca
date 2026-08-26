//! WSJT-X/JTDX UDP source listener — the Rust counterpart of the Swift
//! `WSJTXUDPListener`. One listener per configured source port; every
//! received datagram is forwarded raw (for passthrough) and parsed
//! (Status/Decode) into the pipeline channel.

use tokio::net::UdpSocket;
use tokio::sync::mpsc;

/// One inbound datagram, attributed to its source by name.
#[derive(Debug)]
pub struct SourceDatagram {
    pub source_name: String,
    pub data: Vec<u8>,
}

/// Bind 0.0.0.0:`port` and pump datagrams into `tx` until the socket
/// errors or the receiver closes. Returns the bound socket errors early so
/// the caller can report a port clash honestly (the 1.x "bind race" story).
pub async fn run_listener(
    source_name: String,
    port: u16,
    tx: mpsc::Sender<SourceDatagram>,
) -> std::io::Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", port)).await?;
    let mut buf = vec![0u8; 65_536];
    loop {
        let (n, _peer) = socket.recv_from(&mut buf).await?;
        if n == 0 {
            continue;
        }
        let datagram = SourceDatagram {
            source_name: source_name.clone(),
            data: buf[..n].to_vec(),
        };
        if tx.send(datagram).await.is_err() {
            return Ok(()); // pipeline gone — shut down quietly
        }
    }
}
