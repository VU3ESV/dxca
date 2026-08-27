//! The cluster **client**: a sans-I/O session ([`ClientSession`]) driven by
//! a dial-out supervisor thread ([`ClusterClient`]) with reconnect/backoff.
//! Lifted from meridian-core; `// DXCA:` marks the grafts (password auth,
//! honest-status proof tracking, 1.x backoff schedule, watchdog, IAC strip).

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use super::{ClientConfig, ClientEvent, ClusterSpot, unix_now};
use crate::dxcluster::wire::{self, LineClass};

/// Give up waiting for a login prompt / post-login prompt after this long
/// and treat the stream as live (some feeds never prompt).
const LOGIN_TIMEOUT_S: u64 = 30;

/// Pacing fallback between init commands when the node never shows a
/// prompt.
const INIT_PACING_S: u64 = 2;

/// Cap on re-sending the callsign when the node re-prompts.
const MAX_LOGIN_ATTEMPTS: u8 = 3;

/// Line-assembly buffer cap.
const MAX_BUFFER: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Waiting for a login-prompt substring (or timeout).
    AwaitLoginPrompt,
    /// Callsign sent; waiting for a prompt or first data line.
    AwaitPrompt,
    /// Logged in; streaming.
    Ready,
}

/// Sans-I/O client protocol state.
pub(crate) struct ClientSession {
    cfg: ClientConfig,
    state: State,
    state_since: u64,
    buf: String,
    out: Vec<String>,
    init_idx: usize,
    last_send: u64,
    login_attempts: u8,
    // DXCA: password prompt answered (at most once per connection).
    sent_password: bool,
    // DXCA: real evidence the session works (see ClientEvent::Proven).
    proven: bool,
}

impl ClientSession {
    pub fn new(cfg: &ClientConfig, now: u64) -> Self {
        ClientSession {
            cfg: cfg.clone(),
            state: State::AwaitLoginPrompt,
            state_since: now,
            buf: String::new(),
            out: Vec::new(),
            init_idx: 0,
            last_send: now,
            login_attempts: 0,
            sent_password: false,
            proven: false,
        }
    }

    pub fn ready(&self) -> bool {
        self.state == State::Ready
    }

    // DXCA: whether real evidence arrived this connection.
    pub fn proven(&self) -> bool {
        self.proven
    }

    /// Outbound lines queued since the last call (shell writes them).
    pub fn take_output(&mut self) -> Vec<String> {
        std::mem::take(&mut self.out)
    }

    fn queue(&mut self, line: String, now: u64) {
        self.out.push(line);
        self.last_send = now;
    }

    fn matches_login_prompt(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        self.cfg
            .login_prompts
            .iter()
            .any(|p| lower.contains(p.as_str()))
    }

    // DXCA: password prompt matching, same shape as login.
    fn matches_password_prompt(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        self.cfg
            .password_prompts
            .iter()
            .any(|p| lower.contains(p.as_str()))
    }

    fn send_login(&mut self, now: u64) {
        self.login_attempts += 1;
        let call = self.cfg.login_call.clone();
        self.queue(format!("{call}\r\n"), now);
        self.enter(State::AwaitPrompt, now);
    }

    fn enter(&mut self, state: State, now: u64) {
        self.state = state;
        self.state_since = now;
    }

    // DXCA: mark the session proven-live, at most once per connection.
    fn mark_proven(&mut self, events: &mut Vec<ClientEvent>) {
        if !self.proven {
            self.proven = true;
            events.push(ClientEvent::Proven);
        }
    }

    /// Login complete: emit the event and start the init script.
    fn go_ready(&mut self, now: u64, events: &mut Vec<ClientEvent>) {
        self.enter(State::Ready, now);
        events.push(ClientEvent::LoggedIn);
        self.advance_init(now);
    }

    fn advance_init(&mut self, now: u64) {
        if self.init_idx < self.cfg.init_commands.len() {
            let cmd = self.cfg.init_commands[self.init_idx].clone();
            self.init_idx += 1;
            self.queue(format!("{cmd}\r\n"), now);
        }
    }

    /// Feed one TCP read's worth of text.
    pub fn on_bytes(&mut self, data: &str, now: u64) -> Vec<ClientEvent> {
        let mut events = Vec::new();
        self.buf.push_str(data);
        if self.buf.len() > MAX_BUFFER {
            let cut = self.buf.len() - MAX_BUFFER;
            self.buf.drain(..cut);
        }

        while let Some(pos) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=pos).collect();
            // DXCA: bells and other C0 decoration come off here, before
            // anything parses or forwards the line — see wire::strip_c0_controls.
            let line = wire::strip_c0_controls(line.trim_end_matches(['\r', '\n']));
            if !line.trim().is_empty() {
                self.on_line(&line, now, &mut events);
            }
        }

        // Login prompts usually arrive WITHOUT a newline ("login: ",
        // "Please enter your callsign:") — scan the unterminated remainder.
        if self.state == State::AwaitLoginPrompt && self.matches_login_prompt(&self.buf.clone()) {
            self.buf.clear();
            self.send_login(now);
        }
        // DXCA: password prompts hang without a newline too ("password: ").
        if self.state == State::AwaitPrompt
            && !self.sent_password
            && self.matches_password_prompt(&self.buf.clone())
        {
            self.buf.clear();
            self.send_password(now);
        }
        events
    }

    // DXCA: answer a password prompt. Sends only when a password is
    // configured, but always latches sent_password (1.x behaviour).
    fn send_password(&mut self, now: u64) {
        if !self.cfg.password.is_empty() {
            let pw = self.cfg.password.clone();
            self.queue(format!("{pw}\r\n"), now);
        }
        self.sent_password = true;
    }

    fn on_line(&mut self, line: &str, now: u64, events: &mut Vec<ClientEvent>) {
        match self.state {
            State::AwaitLoginPrompt => {
                if self.matches_login_prompt(line) {
                    self.send_login(now);
                    return;
                }
                // Some feeds skip login entirely and stream immediately.
                match wire::classify_line(line) {
                    LineClass::Spot(p) => {
                        self.go_ready(now, events);
                        self.mark_proven(events); // DXCA
                        events.push(ClientEvent::Spot {
                            spot: p,
                            raw: line.to_string(),
                        });
                    }
                    LineClass::Wwv => {
                        self.go_ready(now, events);
                        self.mark_proven(events); // DXCA
                        events.push(ClientEvent::Wwv(line.to_string()));
                    }
                    _ => {} // banner noise
                }
            }
            State::AwaitPrompt => {
                // DXCA: a password prompt comes before any re-prompt check —
                // "password:" must never be mistaken for a login re-prompt.
                if !self.sent_password && self.matches_password_prompt(line) {
                    self.send_password(now);
                    return;
                }
                if self.matches_login_prompt(line) {
                    // Node re-prompted — our call didn't take.
                    if self.login_attempts < MAX_LOGIN_ATTEMPTS {
                        self.send_login(now);
                    }
                    return;
                }
                match wire::classify_line(line) {
                    LineClass::Prompt => {
                        self.go_ready(now, events);
                        self.mark_proven(events); // DXCA
                    }
                    LineClass::Spot(p) => {
                        self.go_ready(now, events);
                        self.mark_proven(events); // DXCA
                        events.push(ClientEvent::Spot {
                            spot: p,
                            raw: line.to_string(),
                        });
                    }
                    LineClass::Wwv => {
                        self.go_ready(now, events);
                        self.mark_proven(events); // DXCA
                        events.push(ClientEvent::Wwv(line.to_string()));
                    }
                    LineClass::Announce => {
                        self.go_ready(now, events);
                        self.mark_proven(events); // DXCA
                        events.push(ClientEvent::Announce(line.to_string()));
                    }
                    LineClass::Other => {
                        // DXCA: 1.x welcome-line detection — hello/welcome/
                        // connected/cluster after the call was sent is a
                        // login ack even on nodes that never show a prompt.
                        let lower = line.to_lowercase();
                        if ["hello", "welcome", "connected", "cluster"]
                            .iter()
                            .any(|w| lower.contains(w))
                        {
                            self.go_ready(now, events);
                            self.mark_proven(events);
                        }
                    }
                }
            }
            State::Ready => match wire::classify_line(line) {
                LineClass::Spot(p) => {
                    self.mark_proven(events); // DXCA: timeout-ready path
                    events.push(ClientEvent::Spot {
                        spot: p,
                        raw: line.to_string(),
                    });
                }
                LineClass::Wwv => {
                    self.mark_proven(events); // DXCA
                    events.push(ClientEvent::Wwv(line.to_string()));
                }
                LineClass::Announce => {
                    self.mark_proven(events); // DXCA
                    events.push(ClientEvent::Announce(line.to_string()));
                }
                // Prompts are internal: they pace the init script.
                LineClass::Prompt => {
                    self.mark_proven(events); // DXCA
                    self.advance_init(now);
                }
                LineClass::Other => events.push(ClientEvent::Line(line.to_string())),
            },
        }
    }

    /// Drive timeouts: login fallback, init pacing, keepalive.
    pub fn on_tick(&mut self, now: u64) -> Vec<ClientEvent> {
        let mut events = Vec::new();
        match self.state {
            State::AwaitLoginPrompt | State::AwaitPrompt => {
                if now.saturating_sub(self.state_since) >= LOGIN_TIMEOUT_S {
                    // DXCA: the timeout unlocks the session (init commands,
                    // submissions) but is NOT proof — the status pill stays
                    // yellow and the auth watchdog keeps running.
                    self.go_ready(now, &mut events);
                }
            }
            State::Ready => {
                if self.init_idx < self.cfg.init_commands.len()
                    && now.saturating_sub(self.last_send) >= INIT_PACING_S
                {
                    self.advance_init(now);
                } else if self.init_idx >= self.cfg.init_commands.len()
                    && self.cfg.keepalive_secs > 0
                    && now.saturating_sub(self.last_send) >= self.cfg.keepalive_secs
                {
                    self.queue("\r\n".to_string(), now);
                }
            }
        }
        events
    }

    /// Queue a `dx` submission. `false` (dropped) unless logged in.
    pub fn submit_spot(&mut self, spot: &ClusterSpot, now: u64) -> bool {
        if !self.ready() {
            return false;
        }
        self.queue(wire::dx_command(spot), now);
        true
    }

    /// Queue a raw command line (filters, sh/dx, …).
    pub fn send_line(&mut self, line: &str, now: u64) -> bool {
        if !self.ready() {
            return false;
        }
        self.queue(format!("{}\r\n", line.trim_end()), now);
        true
    }
}

// --- supervisor shell ----------------------------------------------------

enum Cmd {
    Spot(ClusterSpot),
    Line(String),
    Stop,
}

/// A supervised outbound cluster connection. Owns one OS thread that
/// connects, logs in, forwards [`ClientEvent`]s, and reconnects until
/// [`stop`](Self::stop) (or drop).
pub struct ClusterClient {
    cmds: Sender<Cmd>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ClusterClient {
    /// Start the supervisor. Returns the handle plus the event stream.
    pub fn start(cfg: ClientConfig) -> (Self, Receiver<ClientEvent>) {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (event_tx, event_rx) = mpsc::channel::<ClientEvent>();
        let thread = thread::Builder::new()
            .name(format!("dxcluster-client-{}", cfg.host))
            .spawn(move || supervisor(cfg, cmd_rx, event_tx))
            .expect("spawn cluster client thread");
        (
            ClusterClient {
                cmds: cmd_tx,
                thread: Some(thread),
            },
            event_rx,
        )
    }

    /// Submit a spot for `dx`-command upload.
    pub fn submit_spot(&self, spot: ClusterSpot) {
        let _ = self.cmds.send(Cmd::Spot(spot));
    }

    /// Send a raw command line once logged in.
    pub fn send_line(&self, line: &str) {
        let _ = self.cmds.send(Cmd::Line(line.to_string()));
    }

    /// Stop the supervisor and close the connection.
    pub fn stop(&mut self) {
        let _ = self.cmds.send(Cmd::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ClusterClient {
    fn drop(&mut self) {
        self.stop();
    }
}

fn supervisor(cfg: ClientConfig, cmds: Receiver<Cmd>, events: Sender<ClientEvent>) {
    // DXCA: 1.x backoff — fixed schedule, last entry repeats, and the
    // attempt index resets ONLY on proven-live. A node that accepts TCP but
    // ignores logins keeps escalating instead of being hammered every 10 s.
    let schedule = if cfg.reconnect_schedule_s.is_empty() {
        vec![10]
    } else {
        cfg.reconnect_schedule_s.clone()
    };
    let mut attempt: usize = 0;
    let mut running = true;
    while running {
        match connect(&cfg) {
            Ok(stream) => {
                let _ = events.send(ClientEvent::Connected);
                let mut proven = false;
                let reason =
                    serve_connection(&cfg, &stream, &cmds, &events, &mut running, &mut proven);
                let _ = stream.shutdown(Shutdown::Both);
                if proven {
                    attempt = 0; // DXCA: only a proven session earns the fast retry
                }
                let _ = events.send(ClientEvent::Disconnected {
                    reason: reason.clone(),
                });
            }
            Err(e) => {
                let _ = events.send(ClientEvent::Disconnected {
                    reason: e.to_string(),
                });
            }
        }
        if running {
            let delay = schedule[attempt.min(schedule.len() - 1)];
            attempt += 1;
            sleep_interruptible(&cmds, delay, &mut running);
        }
    }
}

fn connect(cfg: &ClientConfig) -> std::io::Result<TcpStream> {
    let no_addr =
        || std::io::Error::new(std::io::ErrorKind::NotFound, "host resolved to no address");
    // Try each resolved address in turn (dual-stack hosts).
    let mut last_err = None;
    for addr in (cfg.host.as_str(), cfg.port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(10)) {
            Ok(stream) => {
                stream.set_nodelay(true).ok();
                stream
                    .set_read_timeout(Some(Duration::from_millis(250)))
                    .ok();
                return Ok(stream);
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(no_addr))
}

/// Run one live connection until it drops. Returns the disconnect reason.
fn serve_connection(
    cfg: &ClientConfig,
    stream: &TcpStream,
    cmds: &Receiver<Cmd>,
    events: &Sender<ClientEvent>,
    running: &mut bool,
    proven_out: &mut bool, // DXCA
) -> String {
    let mut session = ClientSession::new(cfg, unix_now());
    let mut read_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => return e.to_string(),
    };
    let mut write_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => return e.to_string(),
    };
    write_stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .ok();
    let mut buf = [0u8; 4096];
    // DXCA: watchdog clocks (1.x semantics — recycle a connection that never
    // proves itself, or a proven one that goes silent).
    let connected_at = Instant::now();
    let mut last_rx = Instant::now();

    loop {
        // Consumer commands.
        loop {
            match cmds.try_recv() {
                Ok(Cmd::Stop) => {
                    *running = false;
                    *proven_out = session.proven();
                    return "stopped".into();
                }
                Ok(Cmd::Spot(s)) => {
                    session.submit_spot(&s, unix_now());
                }
                Ok(Cmd::Line(l)) => {
                    session.send_line(&l, unix_now());
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    *running = false;
                    *proven_out = session.proven();
                    return "consumer dropped".into();
                }
            }
        }

        // Flush session output.
        for line in session.take_output() {
            if let Err(e) = write_stream.write_all(line.as_bytes()) {
                *proven_out = session.proven();
                return format!("write failed: {e}");
            }
        }

        // Read (250 ms timeout doubles as the tick cadence).
        match read_stream.read(&mut buf) {
            Ok(0) => {
                *proven_out = session.proven();
                return "closed by peer".into();
            }
            Ok(n) => {
                last_rx = Instant::now(); // DXCA
                // DXCA: strip Telnet IAC negotiation before text handling.
                let cleaned = wire::strip_telnet_iac(&buf[..n]);
                let chunk = String::from_utf8_lossy(&cleaned).into_owned();
                for ev in session.on_bytes(&chunk, unix_now()) {
                    let _ = events.send(ev);
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                *proven_out = session.proven();
                return format!("read failed: {e}");
            }
        }

        for ev in session.on_tick(unix_now()) {
            let _ = events.send(ev);
        }

        // DXCA: watchdog. Both paths return through the normal disconnect
        // flow, so the supervisor's backoff escalation applies (attempt only
        // resets on proven).
        if !session.proven() && connected_at.elapsed().as_secs() >= cfg.auth_timeout_s {
            *proven_out = false;
            return format!("no login ack within {}s", cfg.auth_timeout_s);
        }
        if session.proven() && last_rx.elapsed().as_secs() >= cfg.silence_timeout_s {
            *proven_out = session.proven();
            return format!("silent for {}s", cfg.silence_timeout_s);
        }
    }
}

/// Back off, but stay responsive to Stop (and drop perishable spots).
fn sleep_interruptible(cmds: &Receiver<Cmd>, secs: u64, running: &mut bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        match cmds.recv_timeout(Duration::from_millis(250)) {
            Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => {
                *running = false;
                return;
            }
            Ok(_) => {} // perishable while disconnected
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ClientConfig {
        ClientConfig::new("127.0.0.1", 7300, "VU2CPL")
    }

    const SPOT_LINE: &str =
        "DX de W3LPL:   14074.0  K1JT          FT8 -10 dB                  1428Z\r\n";

    fn proven_events(ev: &[ClientEvent]) -> usize {
        ev.iter().filter(|e| **e == ClientEvent::Proven).count()
    }

    #[test]
    fn login_prompt_then_node_prompt_proves() {
        let mut s = ClientSession::new(&cfg(), 100);
        assert!(s.on_bytes("login: ", 100).is_empty());
        assert_eq!(s.take_output(), vec!["VU2CPL\r\n".to_string()]);
        assert!(!s.proven());

        let ev = s.on_bytes("VU2CPL de GB7DJK 27-Aug-2026 0923Z dxspider >\r\n", 101);
        assert!(ev.contains(&ClientEvent::LoggedIn));
        assert_eq!(proven_events(&ev), 1);
        assert!(s.ready() && s.proven());
    }

    #[test]
    fn password_prompt_gets_the_password() {
        let mut c = cfg();
        c.password = "secret".into();
        let mut s = ClientSession::new(&c, 100);
        s.on_bytes("login: ", 100);
        s.take_output();
        // Hanging password prompt (no newline).
        s.on_bytes("password: ", 101);
        assert_eq!(s.take_output(), vec!["secret\r\n".to_string()]);
        // A second password prompt is never answered again.
        s.on_bytes("password: ", 102);
        assert!(s.take_output().is_empty());
    }

    #[test]
    fn welcome_line_is_a_login_ack() {
        let mut s = ClientSession::new(&cfg(), 100);
        s.on_bytes("login: ", 100);
        s.take_output();
        let ev = s.on_bytes("Hello VU2CPL, welcome to the AR-Cluster node\r\n", 101);
        assert!(ev.contains(&ClientEvent::LoggedIn));
        assert_eq!(proven_events(&ev), 1);
        assert!(s.proven());
    }

    #[test]
    fn timeout_readies_but_does_not_prove() {
        let mut s = ClientSession::new(&cfg(), 100);
        let ev = s.on_tick(131);
        assert!(ev.contains(&ClientEvent::LoggedIn));
        assert_eq!(proven_events(&ev), 0, "timeout must not prove");
        assert!(s.ready() && !s.proven());

        // A later spot on the timeout-readied stream proves it.
        let ev = s.on_bytes(SPOT_LINE, 200);
        assert_eq!(proven_events(&ev), 1);
        assert!(s.proven());
    }

    #[test]
    fn data_before_any_prompt_means_a_no_login_feed() {
        let mut s = ClientSession::new(&cfg(), 100);
        let ev = s.on_bytes(SPOT_LINE, 100);
        assert!(ev.contains(&ClientEvent::LoggedIn));
        assert_eq!(proven_events(&ev), 1);
        assert!(matches!(
            ev.last(),
            Some(ClientEvent::Spot { spot, .. }) if spot.call == "K1JT"
        ));
    }

    // DXCA: DO5SSB-2 (db0sue.de:8000, DXSpider 1.57) — captured off the wire
    // 2026-08-27. Two things here bite a line-oriented parser: the welcome
    // banner arrives in the SAME read as the unterminated "login: ", and the
    // node's `\x07` bells ride on the end of every spot line.
    #[test]
    fn db0sue_dxspider_logs_in_and_proves() {
        let mut s = ClientSession::new(&cfg(), 100);

        // Read 1: banner line + the hanging prompt, one packet.
        assert!(s.on_bytes("* Welcome, DXer! *\r\nlogin: ", 100).is_empty());
        assert_eq!(
            s.take_output(),
            vec!["VU2CPL\r\n".to_string()],
            "the callsign must go out even though `login: ` has no newline \
             and shares its read with the banner"
        );
        assert!(!s.proven(), "a prompt alone is not evidence");

        // Read 2: the login reply, then the boxed banner, then the node
        // prompt — the first real evidence the session works.
        let ev = s.on_bytes(
            "Hello Manoj Ramawarrier, this is DO5SSB-2 in Frankfurt, Germany\r\n\
             running DXSpider V1.57 build 633\r\n\
             Capabilities: ve7cc rbn\r\n\
             ***********************************************************\r\n\
             **  **    You are connected to 'spider.dxtron.com'    **  **\r\n\
             ***********************************************************\r\n\
             Nodes: 15/406 Users [Loc/Clr]: 101/3995 Max: 233/7822\r\n\
             VU2CPL de DO5SSB-2 27-Aug-2026 0508Z dxspider >\r\n",
            101,
        );
        assert!(ev.contains(&ClientEvent::LoggedIn));
        assert_eq!(proven_events(&ev), 1);
        assert!(s.ready() && s.proven());

        // Read 3: a real spot, bells and all.
        let ev = s.on_bytes(
            "DX de SV1OML:    24915.0  JR3UIC       FT8 KM18vc -> PM75 73 t u      0508Z\x07\x07\r\n",
            102,
        );
        assert!(
            matches!(
                ev.last(),
                Some(ClientEvent::Spot { spot, .. }) if spot.call == "JR3UIC"
            ),
            "expected a JR3UIC spot, got {ev:?}"
        );
    }

    #[test]
    fn iac_prefixed_login_prompt_still_matches() {
        // The supervisor strips IAC before on_bytes; simulate that here.
        let mut data = vec![0xFF, 0xFB, 0x03, 0xFF, 0xFB, 0x01];
        data.extend_from_slice(b"login: ");
        let cleaned = wire::strip_telnet_iac(&data);
        let mut s = ClientSession::new(&cfg(), 100);
        s.on_bytes(&String::from_utf8_lossy(&cleaned), 100);
        assert_eq!(s.take_output(), vec!["VU2CPL\r\n".to_string()]);
    }
}
