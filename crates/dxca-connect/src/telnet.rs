//! Built-in telnet cluster server — the Rust counterpart of the Swift
//! `ClusterTCPServer`: a welcome banner on connect, every broadcast line
//! fanned out to all clients with CRLF.
//!
//! **Optional login** (`docs/TELNET-INTERACTIVE.md` milestone 2). A session
//! may authenticate against DXCA's accounts with `LOGIN <callsign>`. Until
//! it does — and always, when the feature is disabled — the session behaves
//! exactly as it always has: banner, spots, input read only to detect a
//! disconnect.
//!
//! **Why `LOGIN` is a command and not a prompt on connect.** Real cluster
//! nodes prompt for a callsign, and it is tempting to do the same. But the
//! loggers already pointed at port 7575 (RUMlog, Logger32, N1MM+) were
//! configured against a server that never prompted, and what they transmit
//! on connect is unknown — a 45-second packet capture on the production Pi
//! showed an established RUMlog session sending *nothing at all*, but
//! connect-time behaviour could not be observed without disconnecting a
//! live logger. Prompting would therefore be a guess with a working setup
//! as the stake. An opt-in verb cannot break a client that never sends it,
//! so connect-time behaviour stops mattering. Revisit only with a capture
//! of an actual reconnect.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

pub const WELCOME: &str = "DX Cluster Server - DXCA\r\n";

/// Second banner line, sent only when the login gate is enabled.
///
/// Without it nothing on the wire says `LOGIN` exists — an operator
/// connects, sees a wall of spots, and reasonably concludes the server
/// never asked them for anything. Loggers are unaffected: this is banner
/// text, which they discard along with the first line.
pub const LOGIN_HINT: &str =
    "Type  LOGIN <callsign>  for cluster commands (SH/DX, SH/WWV, ...).\r\n";

/// Longest input line accepted; longer ones are discarded rather than
/// buffered, so a client cannot grow the server's memory by never sending
/// a newline.
const MAX_LINE: usize = 512;

/// Failed logins tolerated before the connection is dropped. The LAN is
/// fast enough to brute-force a weak password if we let it run forever.
const MAX_LOGIN_FAILURES: u32 = 3;

/// Quiet period after a command's last reply line before the spot feed
/// resumes. Deliberately longer than the router's own 2 s response window,
/// so the feed does not restart in the middle of a reply that is still
/// trickling in.
const HOLD_GRACE_MS: u64 = 2_500;

/// Hard cap on holding the feed, however badly a command misbehaves.
/// Longer than the router's 15 s command timeout, so a stuck command frees
/// the feed shortly after the router gives up on it rather than muting the
/// session indefinitely.
const HOLD_MAX_MS: u64 = 20_000;

/// Spot lines kept while the feed is held. A held feed is a few seconds, so
/// this is generous; past it the oldest are dropped rather than letting one
/// distracted operator grow the server's memory.
const HOLD_BUFFER_MAX: usize = 500;

/// Who a telnet session turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelnetIdentity {
    pub user_id: i64,
    pub callsign: String,
    pub role: String,
}

/// Verifies telnet credentials.
///
/// A trait so this crate stays free of the account database — `dxca-connect`
/// is I/O engines, and SQLite lives a layer up. Implementations are called
/// off the async runtime (argon2 is deliberately slow), so `authenticate`
/// may block.
pub trait Authenticator: Send + Sync + 'static {
    fn authenticate(&self, callsign: &str, password: &str) -> Option<TelnetIdentity>;
}

/// Identifies one telnet session for the lifetime of its connection.
pub type SessionId = u64;

/// Where an authenticated session's commands go.
///
/// A trait for the same reason [`Authenticator`] is one: command policy and
/// the cluster nodes live a layer up, and this crate must not reach for
/// them. Replies are asynchronous — a node takes its time — so they arrive
/// on the channel handed back by [`open`](CommandSink::open) rather than as
/// a return value.
pub trait CommandSink: Send + Sync + 'static {
    /// Register a freshly authenticated session.
    fn open(
        &self,
        session: SessionId,
        identity: &TelnetIdentity,
    ) -> tokio::sync::mpsc::UnboundedReceiver<String>;
    /// One command line from that session.
    fn submit(&self, session: SessionId, line: &str);
    /// The session logged out or the socket closed.
    fn close(&self, session: SessionId);
}

/// Enables the login verb. Absent = today's behaviour exactly, which is the
/// default: an upgrade must never silently give a port new capabilities.
#[derive(Clone)]
pub struct InteractiveConfig {
    pub auth: Arc<dyn Authenticator>,
    /// Absent = a session can log in but issue no commands (milestone 2's
    /// state). Present = command passthrough.
    pub commands: Option<Arc<dyn CommandSink>>,
}

pub struct ClusterServer {
    tx: broadcast::Sender<String>,
    clients: Arc<AtomicUsize>,
    local_port: u16,
}

impl ClusterServer {
    /// Bind `port` (0 = ephemeral, see [`Self::local_port`]) and start
    /// accepting, with no login offered — the historical behaviour.
    pub async fn start(port: u16) -> std::io::Result<ClusterServer> {
        Self::start_with(port, None).await
    }

    /// As [`start`](Self::start), with an optional login gate.
    ///
    /// A bind failure surfaces immediately (port-clash honesty).
    pub async fn start_with(
        port: u16,
        interactive: Option<InteractiveConfig>,
    ) -> std::io::Result<ClusterServer> {
        let listener = TcpListener::bind(("0.0.0.0", port)).await?;
        let local_port = listener.local_addr()?.port();
        let (tx, _) = broadcast::channel::<String>(256);
        let clients = Arc::new(AtomicUsize::new(0));

        let accept_tx = tx.clone();
        let accept_clients = clients.clone();
        let next_session = Arc::new(std::sync::atomic::AtomicU64::new(1));
        tokio::spawn(async move {
            loop {
                let Ok((stream, _peer)) = listener.accept().await else {
                    return; // listener died — server shut down
                };
                let rx = accept_tx.subscribe();
                let counter = accept_clients.clone();
                let interactive = interactive.clone();
                let session = next_session.fetch_add(1, Ordering::Relaxed);
                counter.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(async move {
                    let _ = serve_client(stream, rx, interactive, session).await;
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

/// Where a session is in the login exchange.
enum SessionState {
    /// The default, and the only state a plain logger ever occupies.
    Anonymous,
    /// `LOGIN <call>` seen; the next line is the password.
    AwaitingPassword { callsign: String },
    /// Authenticated. Milestone 3 gives this state command passthrough.
    Authed(TelnetIdentity),
}

async fn serve_client(
    mut stream: TcpStream,
    mut rx: broadcast::Receiver<String>,
    interactive: Option<InteractiveConfig>,
    session: SessionId,
) -> std::io::Result<()> {
    stream.write_all(WELCOME.as_bytes()).await?;
    if interactive.is_some() {
        stream.write_all(LOGIN_HINT.as_bytes()).await?;
    }
    let mut buf = [0u8; 1024];
    let mut pending = Vec::<u8>::new();
    let mut state = SessionState::Anonymous;
    let mut failures = 0u32;
    // Replies to this session's commands, once it has any. `None` until
    // login, which is why an anonymous session cannot be sent anything.
    let mut replies: Option<tokio::sync::mpsc::UnboundedReceiver<String>> = None;
    let sink = interactive.as_ref().and_then(|c| c.commands.clone());

    // The spot feed is HELD, not dropped, while the operator is mid-exchange
    // — typing a password, or waiting on a command's reply. Field-tested
    // reason: a `SH/DX` table with live spots landing between its rows is
    // unreadable, and the reply is what the operator asked for. Spots are
    // buffered and flushed when the hold lifts, so nothing is lost.
    //
    // This cannot fix input being shredded as it is TYPED: in line mode the
    // client echoes locally and sends nothing until Enter, so the server
    // does not know a line is in progress. That needs `IAC WILL ECHO`.
    let mut held: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    // Extended by each reply line; capped by `hold_deadline`.
    let mut hold_until: Option<tokio::time::Instant> = None;
    let mut hold_deadline: Option<tokio::time::Instant> = None;

    // Closing over `sink` in the loop below would move it; keep a handle
    // for the disconnect path.
    let closer = sink.clone();
    let result = 'session: loop {
        tokio::select! {
            line = rx.recv() => match line {
                Ok(line) => {
                    let holding = hold_until.is_some()
                        || matches!(state, SessionState::AwaitingPassword { .. });
                    if holding {
                        if held.len() >= HOLD_BUFFER_MAX {
                            held.pop_front();
                        }
                        held.push_back(line);
                    } else {
                        stream.write_all(line.as_bytes()).await?;
                    }
                }
                // Lagged: skip what we missed, keep the client connected.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break 'session Ok(()),
            },
            // Command replies. `recv()` on a None receiver would be a
            // never-ready future, so the branch is disabled instead.
            Some(reply) = async {
                match replies.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                stream.write_all(reply.as_bytes()).await?;
                stream.write_all(b"\r\n").await?;
                // More of the reply may still be coming; keep the feed back
                // until it goes quiet, but never past the hard deadline.
                if let Some(deadline) = hold_deadline {
                    let next = tokio::time::Instant::now()
                        + std::time::Duration::from_millis(HOLD_GRACE_MS);
                    hold_until = Some(next.min(deadline));
                }
            },
            // The hold expiring is an event in its own right: flush what
            // the feed produced while the operator was reading.
            _ = async {
                match hold_until {
                    Some(t) => tokio::time::sleep_until(t).await,
                    None => std::future::pending().await,
                }
            } => {
                hold_until = None;
                hold_deadline = None;
                for line in held.drain(..) {
                    stream.write_all(line.as_bytes()).await?;
                }
            },
            read = stream.read(&mut buf) => {
                let n = match read {
                    Ok(0) | Err(_) => break 'session Ok(()), // client went away
                    Ok(n) => n,
                };
                // With no login configured the bytes are still read — the
                // read is how a disconnect is noticed — but nothing is
                // parsed. Byte-for-byte the old behaviour.
                let Some(cfg) = interactive.as_ref() else { continue };

                // Telnet clients open with IAC negotiation; it is not UTF-8
                // and would otherwise land in the first "command".
                pending.extend_from_slice(&crate::dxcluster::wire::strip_telnet_iac(&buf[..n]));
                while let Some(pos) = pending.iter().position(|b| *b == b'\n' || *b == b'\r') {
                    let raw: Vec<u8> = pending.drain(..=pos).collect();
                    let line = String::from_utf8_lossy(&raw[..raw.len() - 1])
                        .trim()
                        .to_string();
                    if line.is_empty() {
                        continue;
                    }
                    let was_authed = matches!(state, SessionState::Authed(_));
                    let mut disconnect = false;
                    if let Some(reply) =
                        handle_line(&line, &mut state, &mut failures, cfg, &mut disconnect).await
                    {
                        stream.write_all(reply.as_bytes()).await?;
                    }
                    if disconnect {
                        if let Some(sink) = sink.as_ref() {
                            sink.close(session);
                        }
                        break 'session Ok(());
                    }
                    // The password exchange ends the moment the password
                    // line arrives, so its hold is released here rather than
                    // by the timer.
                    if !matches!(state, SessionState::AwaitingPassword { .. })
                        && hold_until.is_none()
                    {
                        for line in held.drain(..) {
                            stream.write_all(line.as_bytes()).await?;
                        }
                    }
                    match (&state, was_authed) {
                        // Just logged in — open the command channel.
                        (SessionState::Authed(id), false) => {
                            if let Some(sink) = sink.as_ref() {
                                replies = Some(sink.open(session, id));
                            }
                        }
                        // Just logged out.
                        (SessionState::Anonymous, true) => {
                            if let Some(sink) = sink.as_ref() {
                                sink.close(session);
                            }
                            replies = None;
                        }
                        // An ordinary command from an authenticated session.
                        (SessionState::Authed(_), true) => {
                            if let Some(sink) = sink.as_ref() {
                                sink.submit(session, &line);
                                let now = tokio::time::Instant::now();
                                hold_deadline =
                                    Some(now + std::time::Duration::from_millis(HOLD_MAX_MS));
                                hold_until = Some(
                                    now + std::time::Duration::from_millis(HOLD_GRACE_MS),
                                );
                            }
                        }
                        _ => {}
                    }
                    if failures >= MAX_LOGIN_FAILURES {
                        let _ = stream.write_all(b"Too many failures.\r\n").await;
                        break 'session Ok(());
                    }
                }
                // An over-long line is dropped, not buffered forever.
                if pending.len() > MAX_LINE {
                    pending.clear();
                }
            },
        }
    };
    if let Some(sink) = closer {
        sink.close(session);
    }
    result
}

/// Process one input line. Returns what to send back, if anything.
///
/// Silence is the default: an unrecognized line from an anonymous session
/// is ignored exactly as every line was before this existed, so a logger
/// that transmits something unexpected sees no change whatsoever.
async fn handle_line(
    line: &str,
    state: &mut SessionState,
    failures: &mut u32,
    cfg: &InteractiveConfig,
    // Set when the operator asked to leave; the caller closes the socket
    // after the reply is written.
    disconnect: &mut bool,
) -> Option<String> {
    match state {
        SessionState::AwaitingPassword { callsign } => {
            let callsign = callsign.clone();
            let password = line.to_string();
            let auth = cfg.auth.clone();
            // argon2 is deliberately expensive; verifying it on the async
            // runtime would stall every other session's spot delivery.
            let verdict = tokio::task::spawn_blocking(move || {
                auth.authenticate(&callsign, &password)
            })
            .await
            .ok()
            .flatten();
            match verdict {
                Some(id) => {
                    let greeting = format!(
                        "\r\nWelcome {}{}. Type HELP for what you can do here.\r\n",
                        id.callsign,
                        if id.role == "admin" { " (admin)" } else { "" }
                    );
                    *state = SessionState::Authed(id);
                    *failures = 0;
                    Some(greeting)
                }
                None => {
                    *state = SessionState::Anonymous;
                    *failures += 1;
                    // Deliberately does not say which half was wrong.
                    Some("\r\nLogin failed.\r\n".into())
                }
            }
        }
        _ => {
            let mut parts = line.split_whitespace();
            let verb = parts.next().unwrap_or_default().to_uppercase();
            match verb.as_str() {
                "LOGIN" => match parts.next() {
                    Some(call) => {
                        *state = SessionState::AwaitingPassword {
                            callsign: call.to_uppercase(),
                        };
                        // Leading CRLF so the prompt starts on a clean line
                        // rather than after whatever spot arrived last, and
                        // an explicit warning because this server does not
                        // negotiate telnet ECHO — the password will appear
                        // on screen as it is typed.
                        Some(
                            "\r\nThe spot feed pauses while you type. \
                             Your password WILL be visible.\r\nPassword: "
                                .into(),
                        )
                    }
                    None => Some("Usage: LOGIN <callsign>\r\n".into()),
                },
                // Only honoured once authenticated: a logger that happens
                // to transmit "BYE" must not be hung up on, which is why an
                // anonymous session's BYE is still ignored entirely.
                //
                // For a human it **disconnects**, as it does on every real
                // cluster. It used to log out and leave the socket open with
                // the feed still streaming, which left the operator with no
                // obvious way out — the telnet escape is `Ctrl-]`, a control
                // character, and typing a literal `]` (easily done) just
                // sends more text to a server that is now ignoring it.
                "BYE" | "QUIT" => match state {
                    SessionState::Authed(id) => {
                        let msg = format!("73 {}.\r\n", id.callsign);
                        *state = SessionState::Anonymous;
                        *disconnect = true;
                        Some(msg)
                    }
                    _ => None,
                },
                _ => None, // silence, as before
            }
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

#[cfg(test)]
mod login_tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    /// Accepts VU2CPL/secret and nothing else.
    struct StubAuth;
    impl Authenticator for StubAuth {
        fn authenticate(&self, callsign: &str, password: &str) -> Option<TelnetIdentity> {
            (callsign == "VU2CPL" && password == "secret").then(|| TelnetIdentity {
                user_id: 1,
                callsign: "VU2CPL".into(),
                role: "admin".into(),
            })
        }
    }

    fn interactive() -> Option<InteractiveConfig> {
        Some(InteractiveConfig {
            auth: Arc::new(StubAuth),
            // No sink: milestone 2's behaviour, and these tests assert it
            // still holds — logging in must work with no passthrough.
            commands: None,
        })
    }

    async fn read_until(stream: &mut TcpStream, needle: &str) -> String {
        let mut got = String::new();
        let mut buf = [0u8; 512];
        while !got.contains(needle) {
            let n = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buf))
                .await
                .unwrap_or_else(|_| panic!("timed out waiting for {needle:?}; got {got:?}"))
                .expect("read");
            assert!(n > 0, "server closed early; got {got:?}");
            got.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        got
    }

    /// Nothing arrives within a short window. Used to assert *silence*,
    /// which is the contract for anonymous sessions.
    async fn expect_quiet(stream: &mut TcpStream) {
        let mut buf = [0u8; 512];
        match tokio::time::timeout(std::time::Duration::from_millis(300), stream.read(&mut buf))
            .await
        {
            Err(_) => {}
            Ok(Ok(0)) => panic!("server closed the connection"),
            Ok(Ok(n)) => panic!(
                "expected silence, got {:?}",
                String::from_utf8_lossy(&buf[..n])
            ),
            Ok(Err(e)) => panic!("read error: {e}"),
        }
    }

    /// **The regression guard for every existing logger.** RUMlog,
    /// Logger32 and N1MM+ are pointed at this port today. Whatever they
    /// transmit, an unauthenticated session must get the banner, the spots,
    /// and not one byte of anything else — even with the login gate on.
    #[tokio::test]
    async fn an_anonymous_session_is_answered_with_silence_and_spots() {
        let server = ClusterServer::start_with(0, interactive()).await.unwrap();
        let port = server.local_port();
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        // Two banner lines when the gate is on: the historical one, plus the
        // hint that tells a human LOGIN exists. Both must be consumed before
        // asserting silence — a banner is not a reply to input.
        let banner = read_until(&mut c, "LOGIN <callsign>").await;
        assert!(banner.contains("DX Cluster Server"));

        // The kind of thing a logger might blurt out on connect.
        for junk in [
            "VU2CPL\r\n",                      // a bare callsign
            "set/name Manoj\r\n",              // a cluster command
            "\r\n",                            // a stray newline
            "sh/dx\r\n",                       // a query
            "BYE\r\n",                         // must NOT hang us up
        ] {
            c.write_all(junk.as_bytes()).await.unwrap();
            expect_quiet(&mut c).await;
        }

        // And the feed still works.
        server.broadcast_line("DX de TEST:      14074.0   K1JT   FT8  1428Z");
        assert!(read_until(&mut c, "K1JT").await.contains("DX de TEST:"));
    }

    /// With the feature off, `LOGIN` is not a word this server knows.
    #[tokio::test]
    async fn login_is_not_offered_when_the_feature_is_disabled() {
        let server = ClusterServer::start_with(0, None).await.unwrap();
        let port = server.local_port();
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        read_until(&mut c, "DXCA").await;
        c.write_all(b"LOGIN VU2CPL\r\n").await.unwrap();
        expect_quiet(&mut c).await;
        server.broadcast_line("DX de TEST:      14074.0   K1JT   FT8  1428Z");
        assert!(read_until(&mut c, "K1JT").await.contains("K1JT"));
    }

    #[tokio::test]
    async fn a_good_login_is_accepted_and_the_feed_keeps_running() {
        let server = ClusterServer::start_with(0, interactive()).await.unwrap();
        let port = server.local_port();
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        read_until(&mut c, "DXCA").await;

        c.write_all(b"login vu2cpl\r\n").await.unwrap(); // case-insensitive
        assert!(read_until(&mut c, "Password").await.contains("Password: "));
        c.write_all(b"secret\r\n").await.unwrap();
        let welcome = read_until(&mut c, "Welcome").await;
        assert!(welcome.contains("VU2CPL"), "got {welcome:?}");
        assert!(welcome.contains("admin"), "role is shown: {welcome:?}");

        server.broadcast_line("DX de TEST:      14074.0   K1JT   FT8  1428Z");
        assert!(read_until(&mut c, "K1JT").await.contains("K1JT"));

        // BYE says 73 and hangs up, as it does on every real cluster.
        // It used to log out and leave the socket open with the feed still
        // streaming, which left the operator with no obvious way out.
        c.write_all(b"bye\r\n").await.unwrap();
        assert!(read_until(&mut c, "73").await.contains("VU2CPL"));
        let mut buf = [0u8; 256];
        let n = tokio::time::timeout(std::time::Duration::from_secs(5), c.read(&mut buf))
            .await
            .expect("the server should close after BYE")
            .expect("read");
        assert_eq!(n, 0, "BYE must close the connection");
    }

    /// The logger protection is unchanged: an anonymous session that
    /// happens to transmit BYE is ignored, not hung up on.
    #[tokio::test]
    async fn an_anonymous_bye_does_not_disconnect() {
        let server = ClusterServer::start_with(0, interactive()).await.unwrap();
        let port = server.local_port();
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        read_until(&mut c, "LOGIN <callsign>").await;
        c.write_all(b"BYE\r\n").await.unwrap();
        expect_quiet(&mut c).await;
        server.broadcast_line("DX de TEST:      21074.0   W1AW   FT8  1429Z");
        assert!(
            read_until(&mut c, "W1AW").await.contains("W1AW"),
            "the feed must keep running for a logger"
        );
    }

    #[tokio::test]
    async fn a_bad_password_fails_without_saying_which_half_was_wrong() {
        let server = ClusterServer::start_with(0, interactive()).await.unwrap();
        let port = server.local_port();
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        read_until(&mut c, "DXCA").await;

        c.write_all(b"LOGIN VU2CPL\r\n").await.unwrap();
        read_until(&mut c, "Password").await;
        c.write_all(b"wrong\r\n").await.unwrap();
        let reply = read_until(&mut c, "failed").await;
        assert!(reply.contains("Login failed."), "got {reply:?}");
        let lower = reply.to_lowercase();
        assert!(
            !lower.contains("password") && !lower.contains("callsign") && !lower.contains("unknown"),
            "must not reveal which half was wrong: {reply:?}"
        );

        // An unknown callsign fails identically.
        c.write_all(b"LOGIN NOSUCH\r\n").await.unwrap();
        read_until(&mut c, "Password").await;
        c.write_all(b"whatever\r\n").await.unwrap();
        assert!(read_until(&mut c, "failed").await.contains("Login failed."));
    }

    #[tokio::test]
    async fn repeated_failures_drop_the_connection() {
        let server = ClusterServer::start_with(0, interactive()).await.unwrap();
        let port = server.local_port();
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        read_until(&mut c, "DXCA").await;

        for _ in 0..MAX_LOGIN_FAILURES {
            c.write_all(b"LOGIN VU2CPL\r\n").await.unwrap();
            read_until(&mut c, "Password").await;
            c.write_all(b"nope\r\n").await.unwrap();
        }
        // The server says so and hangs up; read to EOF.
        let mut tail = String::new();
        let mut buf = [0u8; 512];
        loop {
            let n = tokio::time::timeout(std::time::Duration::from_secs(5), c.read(&mut buf))
                .await
                .expect("server should have closed the socket")
                .expect("read");
            if n == 0 {
                break;
            }
            tail.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        assert!(tail.contains("Too many failures."), "got {tail:?}");
    }

    /// The bug that made this feature look broken in production: the
    /// `Password: ` prompt carries no newline (the cursor has to stay put
    /// for the answer), so on a live feed the next spot glued itself to the
    /// prompt and scrolled it away. From the operator's chair that is
    /// indistinguishable from never being asked. The feed is now held while
    /// a password is outstanding.
    #[tokio::test]
    async fn the_password_prompt_is_not_buried_by_the_spot_feed() {
        let server = ClusterServer::start_with(0, interactive()).await.unwrap();
        let port = server.local_port();
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        read_until(&mut c, "LOGIN <callsign>").await;

        c.write_all(b"LOGIN VU2CPL\r\n").await.unwrap();
        let prompt = read_until(&mut c, "Password: ").await;
        // A busy node, mid-login.
        for i in 0..20 {
            server.broadcast_line(&format!(
                "DX de FLOOD:      14074.0   TEST{i}         FT8 -10 dB  1428Z"
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        expect_quiet(&mut c).await;
        assert!(
            prompt.trim_end().ends_with("Password:"),
            "the prompt must be the last thing on screen, got {prompt:?}"
        );

        // Once the password is in, the feed resumes.
        c.write_all(b"secret\r\n").await.unwrap();
        read_until(&mut c, "Welcome").await;
        server.broadcast_line("DX de TEST:      21074.0   W1AW   FT8  1429Z");
        assert!(read_until(&mut c, "W1AW").await.contains("W1AW"));
    }

    /// Nothing on the wire used to say `LOGIN` existed.
    #[tokio::test]
    async fn the_banner_tells_the_operator_how_to_log_in() {
        let server = ClusterServer::start_with(0, interactive()).await.unwrap();
        let mut c = TcpStream::connect(("127.0.0.1", server.local_port()))
            .await
            .unwrap();
        let banner = read_until(&mut c, "LOGIN").await;
        assert!(banner.contains("LOGIN <callsign>"), "got {banner:?}");

        // ...but only when the gate is on. A plain install's banner is
        // unchanged, which is what every existing logger sees.
        let plain = ClusterServer::start_with(0, None).await.unwrap();
        let mut p = TcpStream::connect(("127.0.0.1", plain.local_port()))
            .await
            .unwrap();
        read_until(&mut p, "DX Cluster Server").await;
        expect_quiet(&mut p).await;
    }

    /// A stub sink that answers one command with several lines, slowly
    /// enough that the feed would interleave if it were not held.
    struct SlowSink {
        tx: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>,
    }
    impl CommandSink for SlowSink {
        fn open(
            &self,
            _session: SessionId,
            _identity: &TelnetIdentity,
        ) -> tokio::sync::mpsc::UnboundedReceiver<String> {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            *self.tx.lock().unwrap() = Some(tx);
            rx
        }
        fn submit(&self, _session: SessionId, _line: &str) {
            let tx = self.tx.lock().unwrap().clone().expect("opened");
            tokio::spawn(async move {
                for i in 0..3 {
                    let _ = tx.send(format!("reply row {i}"));
                    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
                }
            });
        }
        fn close(&self, _session: SessionId) {}
    }

    /// The field bug: a `SH/DX` table with live spots landing between its
    /// rows is unreadable. The feed is held while a reply is arriving —
    /// and **flushed afterwards**, so the spots are delayed, never dropped.
    #[tokio::test]
    async fn the_spot_feed_is_held_during_a_reply_then_flushed() {
        let server = ClusterServer::start_with(
            0,
            Some(InteractiveConfig {
                auth: Arc::new(StubAuth),
                commands: Some(Arc::new(SlowSink {
                    tx: std::sync::Mutex::new(None),
                })),
            }),
        )
        .await
        .unwrap();
        let port = server.local_port();
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        read_until(&mut c, "LOGIN <callsign>").await;
        c.write_all(b"LOGIN VU2CPL\r\n").await.unwrap();
        read_until(&mut c, "Password").await;
        c.write_all(b"secret\r\n").await.unwrap();
        read_until(&mut c, "Welcome").await;

        c.write_all(b"sh/dx\r\n").await.unwrap();
        // A spot arrives while the reply is still coming.
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        server.broadcast_line("DX de TEST:      14074.0   MIDREPLY   FT8  1428Z");

        // Every reply row lands before the spot does.
        let got = read_until(&mut c, "reply row 2").await;
        assert!(
            !got.contains("MIDREPLY"),
            "a spot interrupted the reply: {got}"
        );

        // ...and the spot is not lost — it arrives once the hold lifts.
        let after = read_until(&mut c, "MIDREPLY").await;
        assert!(after.contains("MIDREPLY"), "the held spot must be flushed");
    }

    /// An anonymous session has no commands, so nothing is ever held: the
    /// regression guard for every logger on the port.
    #[tokio::test]
    async fn an_anonymous_session_is_never_held() {
        let server = ClusterServer::start_with(0, interactive()).await.unwrap();
        let port = server.local_port();
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        read_until(&mut c, "LOGIN <callsign>").await;
        server.broadcast_line("DX de TEST:      14074.0   NOW   FT8  1428Z");
        assert!(read_until(&mut c, "NOW").await.contains("NOW"));
    }

    /// A real telnet client opens with IAC negotiation. Those bytes are not
    /// UTF-8 and must not be mistaken for the first command.
    #[tokio::test]
    async fn telnet_negotiation_bytes_do_not_break_the_login() {
        let server = ClusterServer::start_with(0, interactive()).await.unwrap();
        let port = server.local_port();
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        read_until(&mut c, "DXCA").await;

        // IAC WILL NAWS, IAC DO ECHO, then the command in the same segment.
        let mut opening = vec![0xFF, 0xFB, 0x1F, 0xFF, 0xFD, 0x01];
        opening.extend_from_slice(b"LOGIN VU2CPL\r\n");
        c.write_all(&opening).await.unwrap();
        assert!(read_until(&mut c, "Password").await.contains("Password: "));
    }
}
