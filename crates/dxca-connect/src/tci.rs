//! TCI spot injection for ExpertSDR3 (SunSDR and friends), over WebSocket.
//!
//! The [`crate::flex`] feature for a different make of radio: DXCA's
//! **alerts** on the panorama, colour-coded by level, so a rare one is
//! visible where you are already looking.
//!
//! Written against the published spec — *TCI Protocol*, Expert Group LLC,
//! rev 1.0.7 / TCI 2.0, 12 Jan 2024 — §3.1 and §4.3.
//!
//! ## The command
//!
//! ```text
//! SPOT:RN6LHF,CW,7100000,16711680,ANY_TEXT;
//!      \____/ \/ \_____/ \______/ \______/
//!      call  mode  Hz     ARGB     text
//! ```
//!
//! Three differences from SmartSDR's `spot add` that shape this module:
//!
//! 1. **The transport is a WebSocket**, not a raw socket. TCI "uses a full
//!    duplex web socket protocol that runs on top of a TCP connection"
//!    (§1.4), so the bytes on 40001 are HTTP upgrade then RFC 6455 frames.
//!    A plain `TcpStream` write of `SPOT:...;` is discarded by the server
//!    without a word, which is exactly the silent-success failure the Flex
//!    module's header warns about, one layer down.
//!
//! 2. **The delimiters are `:` `,` `;`**, and §3.1 names them reserved:
//!    they "cannot be included in the command name and command arguments".
//!    Spaces, however, are fine — the opposite of Flex, where the space is
//!    the delimiter. So the text field here is allowed to read like prose,
//!    and [`sanitize`] strips the three that would truncate the command
//!    instead.
//!
//! 3. **There is no lifetime field, and no timestamp or spotter field.**
//!    `SPOT` has five arguments and that is all of them. A spot placed on
//!    the panorama stays until something removes it, so the ladder that
//!    Flex gets for free from `lifetime_seconds` has to be run here: the
//!    worker keeps each call's deadline and sends `SPOT_DELETE:<call>;`
//!    when it passes. Without that the panorama silts up over an evening
//!    and buries the one red mark the feature exists to show.
//!
//! `SPOT_CLEAR;` — "delete all spots from panorama" — is deliberately never
//! sent, not even on connect to tidy up. The server synchronises state
//! across every connected client (§3.1), so it would also wipe the spots
//! some other logger put there.
//!
//! ## The connection has to be drained
//!
//! Same hazard as Flex, for the same reason plus one. From `READY;` onward
//! the server pushes sensor notifications (§4.4) whether anyone asked or
//! not, so a writer that never reads stalls the server on us. On top of
//! that, WebSocket has Ping frames that must be Ponged or the peer hangs
//! up, and tungstenite only emits those Pongs from inside a read. So this
//! worker reads on every pass and throws the result away.
//!
//! Unlike Flex it cannot do that from a second thread: the framing state
//! lives in the `WebSocket` value, and two threads cannot share it without
//! a lock that would serialise reads against writes anyway. One thread does
//! both, against a socket with a short read timeout.

use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use tungstenite::{Message, WebSocket, client::client, stream::MaybeTlsStream};

/// Wait between reconnect attempts, as in [`crate::flex`]: connections are
/// only attempted when a spot needs sending, so this only matters when
/// alerts arrive in a burst while the radio is off.
#[cfg(not(test))]
const RECONNECT_AFTER: Duration = Duration::from_secs(30);
/// Shortened under test so the reconnect path can be exercised at all — a
/// 30-second wait would price its regression test out of the gate, and the
/// reconnect is exactly where this module's one real defect lived.
#[cfg(test)]
const RECONNECT_AFTER: Duration = Duration::from_millis(200);

/// How long past its deadline a deletion we could not send is still worth
/// sending.
///
/// `pending` is deliberately KEPT across a reconnect (see `worker`), because
/// a TCI spot is the *server's* state and outlives our link — dropping the
/// deletions would silt the panorama up permanently, which is the one thing
/// this module exists to prevent. But keeping them forever has its own cost:
/// a non-empty `pending` is what makes the worker keep dialling, so a radio
/// switched off for a week would be dialled every 30s for a week. After this
/// long the operator has almost certainly restarted ExpertSDR3 (which clears
/// the panorama anyway) and the mark is moot, so we let it go and fall back
/// to sleeping until the next alert.
const PENDING_GRACE: Duration = Duration::from_secs(30 * 60);

/// Queue depth. Alerts are rare; anything approaching this means the radio
/// has gone away and the backlog is worthless anyway.
const QUEUE: usize = 256;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a read may block before the worker goes back to look at its
/// queue and its expiry deadlines. Short enough that a spot is never held
/// up noticeably, long enough that an idle link is not a spin loop.
const READ_TIMEOUT: Duration = Duration::from_millis(250);

/// How long to wait for `READY;` before sending anyway. See [`drain_until_ready`].
const READY_TIMEOUT: Duration = Duration::from_secs(3);

/// ExpertSDR3's default TCI port. The spec does not name one — it describes
/// the protocol, not the product — so this is the application default, and
/// it is configurable for a reason.
pub const DEFAULT_PORT: u16 = 40001;

/// The text field, in characters. Not a protocol limit — the spec sets
/// none — but a panorama label is drawn beside a callsign in a crowded
/// band, and past about this width it overlaps its neighbours.
pub const TEXT_MAX: usize = 40;

/// One spot to place on the panorama.
#[derive(Debug, Clone, PartialEq)]
pub struct TciSpot {
    pub callsign: String,
    pub freq_hz: u64,
    pub mode: String,
    /// Free text; `:` `,` `;` are stripped and it is clipped to [`TEXT_MAX`].
    pub text: String,
    /// `0xAARRGGBB`. Sent as the decimal integer the protocol wants.
    pub color_argb: u32,
    /// How long before this call is removed with `SPOT_DELETE`. `0` never
    /// expires it, which is the caller's choice to make and not this
    /// module's to override.
    pub lifetime_secs: u64,
}

/// Reserved characters out, and short enough to sit on a panorama.
///
/// §3.1: `:` `,` and `;` "cannot be included in the command name and
/// command arguments". One of them in a value truncates the command exactly
/// as a space does in SmartSDR's, so this is load-bearing, not cosmetic.
///
/// They become spaces rather than vanishing: `AF-045,IOTA` should read
/// `AF-045 IOTA`, not `AF-045IOTA`. Runs collapse so the width is spent on
/// words. Clipped by **characters** — byte-slicing would panic mid-boundary
/// on an entity name like `Åland`.
fn sanitize(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut kept = 0usize;
    let mut last_was_space = false;
    for ch in s.chars() {
        if kept >= max {
            break;
        }
        let blank = ch.is_whitespace() || matches!(ch, ':' | ',' | ';');
        if blank {
            // Never lead with a space, and never repeat one.
            if !last_was_space && kept > 0 {
                out.push(' ');
                last_was_space = true;
                kept += 1;
            }
        } else {
            out.push(ch);
            last_was_space = false;
            kept += 1;
        }
    }
    // A value that ended on a stripped delimiter would carry a trailing
    // space into the command; harmless, but it renders as a gap.
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// The panorama text for one alert: `"<level> <entity>"`.
///
/// TCI is roomier than SmartSDR's twenty characters, so unlike
/// [`crate::flex::comment_for`] this does not have to choose between the
/// level and the entity — both fit, and the pair reads as a sentence.
pub fn text_for(level_label: &str, entity: Option<&str>) -> String {
    match entity.filter(|e| !e.is_empty()) {
        Some(entity) => format!("{level_label} {entity}"),
        None => level_label.to_string(),
    }
}

/// Build one `SPOT` command.
///
/// Pure, so the wire format is testable without a radio.
pub fn spot_command(s: &TciSpot) -> String {
    format!(
        "SPOT:{},{},{},{},{};",
        sanitize(&s.callsign, 32),
        sanitize(&s.mode, 16),
        s.freq_hz,
        // Decimal, not `0x…`: the spec's own example is `16711680`, and
        // that is red — so the field is a plain integer even though it is
        // documented as ARGB.
        s.color_argb,
        sanitize(&s.text, TEXT_MAX),
    )
}

/// Build one `SPOT_DELETE` command.
pub fn delete_command(callsign: &str) -> String {
    format!("SPOT_DELETE:{};", sanitize(callsign, 32))
}

#[derive(Default)]
pub struct Counters {
    pub sent: AtomicU64,
    pub failed: AtomicU64,
    /// `SPOT_DELETE`s that went out — expiries, not errors.
    pub expired: AtomicU64,
}

/// A live (or reconnecting) link to one ExpertSDR3.
///
/// Cheap to hold and safe to share: sending queues onto a channel and never
/// blocks the caller, which matters because the alert fan-out runs on the
/// spot pipeline's runtime.
pub struct TciClient {
    tx: SyncSender<TciSpot>,
    pub counters: Arc<Counters>,
    pub target: String,
}

impl TciClient {
    /// Start the worker. Does **not** connect — the first spot does that, so
    /// a configured-but-switched-off radio costs nothing until it is needed.
    pub fn connect(host: &str, port: u16) -> TciClient {
        let (tx, rx) = std::sync::mpsc::sync_channel::<TciSpot>(QUEUE);
        let counters = Arc::new(Counters::default());
        let target = format!("{host}:{port}");
        {
            let target = target.clone();
            let counters = counters.clone();
            std::thread::spawn(move || worker(target, rx, counters));
        }
        TciClient {
            tx,
            counters,
            target,
        }
    }

    /// Queue one spot. `false` means it was dropped — the queue is full,
    /// which only happens when the radio has gone away.
    pub fn spot(&self, s: &TciSpot) -> bool {
        match self.tx.try_send(s.clone()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.counters.failed.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }
}

/// A spot we placed and owe a `SPOT_DELETE`.
struct Pending {
    callsign: String,
    due: Instant,
}

fn worker(target: String, rx: Receiver<TciSpot>, counters: Arc<Counters>) {
    let mut conn: Option<WebSocket<MaybeTlsStream<TcpStream>>> = None;
    let mut last_attempt: Option<Instant> = None;
    // Deadlines for spots we have placed, oldest first. Insertion is
    // append-only and lifetimes per level are fixed, so this is *nearly*
    // sorted; `expire_due` scans it rather than assuming order, because
    // "nearly" is not a thing to rely on when the levels have different
    // ladders.
    let mut pending: Vec<Pending> = Vec::new();

    loop {
        // With no link to drain and nothing owed a deletion, there is
        // genuinely nothing to do until a spot arrives — so block until one
        // does. This matters on the always-on host: a poll here would be
        // four wakeups a second for the life of the process, on a radio that
        // may be switched off all week.
        let next = if conn.is_none() && pending.is_empty() {
            match rx.recv() {
                Ok(spot) => Some(spot),
                // Every sender is gone: the client was dropped.
                Err(_) => break,
            }
        } else {
            // Otherwise wake for whichever comes first: a spot, the next
            // deletion falling due, or the drain interval.
            //
            // While DISCONNECTED, wait for the next dial instead: nothing
            // can be sent until then, and the deletions we are holding are
            // by now overdue, so asking for the soonest deadline would give
            // a zero-length timeout and spin this thread at full tilt.
            let wait = if conn.is_none() {
                last_attempt.map_or(Duration::ZERO, |t| {
                    RECONNECT_AFTER.saturating_sub(t.elapsed())
                })
            } else {
                pending
                    .iter()
                    .map(|p| p.due.saturating_duration_since(Instant::now()))
                    .min()
                    .unwrap_or(READ_TIMEOUT)
                    .min(READ_TIMEOUT)
            };
            match rx.recv_timeout(wait) {
                Ok(spot) => Some(spot),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        };

        // Deletions we could not send stay owed across a reconnect, but not
        // past `PENDING_GRACE` — that is what stops a dark radio being
        // dialled forever for marks nobody can see any more.
        let now = Instant::now();
        pending.retain(|p| now < p.due + PENDING_GRACE);

        if conn.is_none() && (next.is_some() || !pending.is_empty()) {
            // Only one attempt per RECONNECT_AFTER, so a burst of alerts at
            // a dark radio does not become a burst of connect syscalls.
            let due = last_attempt.is_none_or(|t| t.elapsed() >= RECONNECT_AFTER);
            if due {
                last_attempt = Some(Instant::now());
                conn = dial(&target);
                // `pending` is deliberately KEPT here. The premise it used
                // to be cleared on — "a reconnect means the server lost our
                // spots" — is wrong for TCI: a spot is server-side state on
                // the panorama and outlives the client that placed it, so
                // dropping the deletions strands our marks there for good.
                // `expire_due` sends the overdue ones on the next pass.
                //
                // The counter-risk is real but much the smaller one: if some
                // other client re-spotted the same call during our outage,
                // our delete takes their mark down too. That is one spot,
                // recoverable by re-spotting; the alternative was a panorama
                // that silts up permanently and can only be cleared by hand.
            }
        }

        let Some(ws) = conn.as_mut() else {
            if next.is_some() {
                counters.failed.fetch_add(1, Ordering::Relaxed);
            }
            continue;
        };

        let mut failed = false;

        if let Some(spot) = &next {
            match send(ws, &spot_command(spot)) {
                Ok(()) => {
                    counters.sent.fetch_add(1, Ordering::Relaxed);
                    if spot.lifetime_secs > 0 {
                        // One deadline per call, not per spot: the same
                        // station re-spotted should sit on the panorama for
                        // a full lifetime from the LAST sighting, and two
                        // deletes for one call would take out a re-spot.
                        let due = Instant::now() + Duration::from_secs(spot.lifetime_secs);
                        match pending.iter_mut().find(|p| p.callsign == spot.callsign) {
                            Some(p) => p.due = due,
                            None => pending.push(Pending {
                                callsign: spot.callsign.clone(),
                                due,
                            }),
                        }
                    }
                }
                Err(e) => {
                    eprintln!("dxca: tci {target}: write failed, will reconnect: {e}");
                    counters.failed.fetch_add(1, Ordering::Relaxed);
                    failed = true;
                }
            }
        }

        if !failed {
            failed = !expire_due(ws, &target, &mut pending, &counters);
        }

        // The drain. Without it the server's notification stream fills our
        // receive buffer and blocks its sends, and its Pings go unanswered —
        // see the module docs.
        if !failed {
            failed = !drain(ws, &target);
        }

        if failed {
            // The link is gone; the spots on the panorama are NOT — they are
            // the server's, so what we owe stays owed (see the dial above).
            conn = None;
        }
    }
}

/// Send every deletion that has fallen due. `false` means the link broke.
fn expire_due(
    ws: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    target: &str,
    pending: &mut Vec<Pending>,
    counters: &Counters,
) -> bool {
    let now = Instant::now();
    let mut ok = true;
    pending.retain(|p| {
        if !ok || p.due > now {
            return true;
        }
        match send(ws, &delete_command(&p.callsign)) {
            Ok(()) => {
                counters.expired.fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(e) => {
                eprintln!("dxca: tci {target}: expiry write failed: {e}");
                ok = false;
                true
            }
        }
    });
    ok
}

/// Read and discard whatever is waiting. `false` means the link is gone.
///
/// A read timeout is the normal case, not an error: it is how the worker
/// gets back to its queue.
fn drain(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>, target: &str) -> bool {
    loop {
        match ws.read() {
            Ok(Message::Close(_)) => {
                println!("dxca: tci {target}: server closed the session");
                return false;
            }
            // Text is the command stream, binary is an audio/IQ block we
            // never asked for. Both go in the bin.
            Ok(_) => continue,
            Err(e) if transient(&e) => return true,
            Err(e) => {
                eprintln!("dxca: tci {target}: read failed, will reconnect: {e}");
                return false;
            }
        }
    }
}

/// Is this error just "nothing to read right now"?
///
/// A socket read timeout surfaces as `WouldBlock` on Unix and `TimedOut` on
/// Windows — both, since DXCA ships to both. `Interrupted` is a signal, not
/// a failure. tungstenite keeps its partial frame buffered across all
/// three, so resuming the read later is safe.
fn transient(e: &tungstenite::Error) -> bool {
    match e {
        tungstenite::Error::Io(io) => matches!(
            io.kind(),
            std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::Interrupted
        ),
        _ => false,
    }
}

/// One command out. TCI commands are WebSocket **text** frames (§3.4:
/// "commands are transmitted as strings and audio streams are transmitted
/// as byte streams") — a binary frame is ignored.
fn send(
    ws: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    cmd: &str,
) -> Result<(), tungstenite::Error> {
    ws.send(Message::Text(cmd.into()))
}

fn dial(target: &str) -> Option<WebSocket<MaybeTlsStream<TcpStream>>> {
    let addr: std::net::SocketAddr = target.parse().ok().or_else(|| {
        use std::net::ToSocketAddrs;
        target.to_socket_addrs().ok()?.next()
    })?;
    let stream = match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("dxca: tci {target}: connect failed: {e}");
            return None;
        }
    };
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let _ = stream.set_nodelay(true);

    // The handshake needs a blocking read or it can return WouldBlock
    // before the server's response has arrived; the short timeout is put on
    // afterwards, for the drain loop.
    let _ = stream.set_read_timeout(Some(CONNECT_TIMEOUT));

    // `ws://host:port/` — plain, never `wss://`. TCI has no TLS mode and
    // the radio is a box on the LAN. `MaybeTlsStream` in the type is what
    // tungstenite's own handshake returns; nothing here ever populates the
    // Tls arm.
    let url = format!("ws://{target}/");
    let mut ws = match client(&url, MaybeTlsStream::Plain(stream)) {
        Ok((ws, _resp)) => ws,
        Err(e) => {
            eprintln!("dxca: tci {target}: websocket handshake failed: {e}");
            return None;
        }
    };

    // §4.1: on connect the server sends its initialization block and then
    // `READY;`. Spots sent before it are accepted by ExpertSDR3 in
    // practice, but waiting is what the protocol describes and it costs one
    // idle moment on a connection that is only made when a spot is already
    // waiting.
    drain_until_ready(&mut ws, target);

    if let MaybeTlsStream::Plain(s) = ws.get_ref() {
        let _ = s.set_read_timeout(Some(READ_TIMEOUT));
    }
    println!("dxca: tci {target}: connected");
    Some(ws)
}

/// Read the initialization block, stopping at `READY;`.
///
/// Bounded by [`READY_TIMEOUT`], and never fatal. A server that never says
/// `READY;` — an older ExpertSDR3, or something else answering on the
/// port — should still get its spots rather than have DXCA sulk.
fn drain_until_ready(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>, target: &str) {
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        match ws.read() {
            Ok(Message::Text(t)) => {
                // Case-insensitive: §3.1, "the case of letters does not
                // matter".
                if t.trim().eq_ignore_ascii_case("READY;") {
                    return;
                }
            }
            Ok(Message::Close(_)) => return,
            Ok(_) => continue,
            Err(e) if transient(&e) => continue,
            Err(e) => {
                eprintln!("dxca: tci {target}: read during handshake failed: {e}");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot() -> TciSpot {
        TciSpot {
            callsign: "3Y0J".into(),
            freq_hz: 14_074_000,
            mode: "FT8".into(),
            text: "NEW DXCC Bouvet Island".into(),
            color_argb: 0xFFF5_636B,
            lifetime_secs: 3600,
        }
    }

    /// The wire format, pinned against the spec's own example:
    /// `SPOT:RN6LHF,CW,7100000,16711680,ANY_TEXT;`
    #[test]
    fn the_command_matches_the_spec_example() {
        let c = spot_command(&TciSpot {
            callsign: "RN6LHF".into(),
            freq_hz: 7_100_000,
            mode: "CW".into(),
            text: "ANY_TEXT".into(),
            color_argb: 16_711_680,
            lifetime_secs: 0,
        });
        assert_eq!(c, "SPOT:RN6LHF,CW,7100000,16711680,ANY_TEXT;");
    }

    /// Five arguments, one terminator. A value that leaked a delimiter
    /// would change the count and the radio would parse nonsense — this is
    /// the assertion that keeps that from regressing.
    #[test]
    fn no_value_may_contain_a_delimiter() {
        let c = spot_command(&TciSpot {
            callsign: "3Y0J".into(),
            // Every reserved character, in the field most likely to see one.
            text: "AF-045; IOTA: rare, DX".into(),
            ..spot()
        });
        let body = c
            .strip_prefix("SPOT:")
            .and_then(|b| b.strip_suffix(';'))
            .expect("prefix and terminator");
        assert_eq!(body.split(',').count(), 5, "{c}");
        assert!(!body.contains(';'), "{c}");
        assert!(!body.contains(':'), "{c}");
        assert!(c.ends_with("AF-045 IOTA rare DX;"), "{c}");
    }

    /// Frequency is Hz and an integer — a decimal point here is a spot that
    /// lands nowhere.
    #[test]
    fn frequency_is_whole_hertz() {
        let c = spot_command(&spot());
        assert!(c.contains(",14074000,"), "{c}");
    }

    /// Colour goes out as decimal ARGB, not `0x…`. The dashboard's red is
    /// `0xFFF5636B`; on the wire that is 4294271851.
    #[test]
    fn colour_is_decimal_argb() {
        let c = spot_command(&spot());
        assert!(c.contains(",4294271851,"), "{c}");
    }

    /// Unlike Flex, spaces survive — the delimiter here is the comma, so the
    /// text may read like prose.
    #[test]
    fn spaces_are_kept() {
        assert_eq!(sanitize("NEW DXCC Bouvet", TEXT_MAX), "NEW DXCC Bouvet");
        assert_eq!(
            text_for("NEW DXCC", Some("Bouvet Island")),
            "NEW DXCC Bouvet Island"
        );
        assert_eq!(text_for("NEW DXCC", None), "NEW DXCC");
        assert_eq!(text_for("NEW DXCC", Some("")), "NEW DXCC");
    }

    /// Clipped by characters, not bytes: an entity name like `Åland` would
    /// panic a byte slice mid-boundary.
    #[test]
    fn the_text_is_clipped_by_characters() {
        let long = "NEW DXCC Åland Islands and then some more words still";
        let s = sanitize(long, TEXT_MAX);
        assert_eq!(s.chars().count(), TEXT_MAX);
        assert!(s.starts_with("NEW DXCC Åland"), "{s}");
    }

    /// Runs of delimiters and whitespace collapse to one space, and neither
    /// end carries a stray one.
    #[test]
    fn runs_collapse_and_edges_are_clean() {
        assert_eq!(sanitize("a ,; \t b", 40), "a b");
        assert_eq!(sanitize(",,leading", 40), "leading");
        assert_eq!(sanitize("trailing;;", 40), "trailing");
    }

    /// The whole path against a real WebSocket server: handshake, `READY;`,
    /// the spot, and the session kept for the next one. The string tests
    /// above cannot show any of that.
    #[test]
    fn it_connects_and_sends_over_websocket() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let (got_tx, got_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut server = tungstenite::accept(stream).unwrap();
            // A real server talks first: the initialization block, then
            // READY. This stands in for that, and proves the client waits
            // for it rather than wedging.
            for line in ["PROTOCOL:ExpertSDR3,2.0;", "DEVICE:SunSDR2DX;", "READY;"] {
                server.send(Message::Text(line.into())).unwrap();
            }
            for _ in 0..2 {
                match server.read() {
                    Ok(Message::Text(t)) => {
                        let _ = got_tx.send(t.to_string());
                    }
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        });

        let client = TciClient::connect(&addr.ip().to_string(), addr.port());
        assert!(client.spot(&spot()));
        assert!(client.spot(&TciSpot {
            callsign: "VP8PJ".into(),
            ..spot()
        }));

        let first = got_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("first command should arrive");
        let second = got_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("second should reuse the same session");

        // The FIRST thing on the wire is a spot: TCI has no login and DXCA
        // sends no state-changing command, so nothing precedes it.
        assert!(first.starts_with("SPOT:3Y0J,"), "{first}");
        assert!(first.contains(",14074000,"), "{first}");
        assert!(second.starts_with("SPOT:VP8PJ,"), "{second}");
        assert_eq!(client.counters.sent.load(Ordering::Relaxed), 2);
        assert_eq!(client.counters.failed.load(Ordering::Relaxed), 0);
    }

    /// The panorama would silt up without this: TCI has no `lifetime`
    /// field, so the deletion is DXCA's job and this is the proof it
    /// happens.
    #[test]
    fn a_spot_is_deleted_when_its_lifetime_runs_out() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let (got_tx, got_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut server = tungstenite::accept(stream).unwrap();
            server.send(Message::Text("READY;".into())).unwrap();
            while let Ok(msg) = server.read() {
                if let Message::Text(t) = msg
                    && got_tx.send(t.to_string()).is_err()
                {
                    break;
                }
            }
        });

        let client = TciClient::connect(&addr.ip().to_string(), addr.port());
        assert!(client.spot(&TciSpot {
            lifetime_secs: 1,
            ..spot()
        }));

        let placed = got_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(placed.starts_with("SPOT:3Y0J,"), "{placed}");

        let deleted = got_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the spot should be deleted when its lifetime passes");
        assert_eq!(deleted, "SPOT_DELETE:3Y0J;");
        assert_eq!(client.counters.expired.load(Ordering::Relaxed), 1);
    }

    /// The defect deferred to the release pass: a reconnect used to call
    /// `pending.clear()`, on the premise that the server had lost our spots.
    /// It has not — a TCI spot is the panorama's state, not the link's — so
    /// a transient drop stranded every mark DXCA had placed, permanently,
    /// which is the exact silting-up the lifetimes exist to prevent.
    #[test]
    fn a_reconnect_still_owes_the_deletions_it_could_not_send() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let (got_tx, got_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // Two sessions: the first is dropped mid-life on purpose, the
            // second is where the owed deletion has to turn up.
            for session in 0..2 {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let Ok(mut server) = tungstenite::accept(stream) else {
                    return;
                };
                let _ = server.send(Message::Text("READY;".into()));
                while let Ok(msg) = server.read() {
                    if let Message::Text(t) = msg {
                        let t = t.to_string();
                        let is_spot = t.starts_with("SPOT:");
                        if got_tx.send(t).is_err() {
                            return;
                        }
                        // Kill the first link the moment the spot is placed.
                        if session == 0 && is_spot {
                            break;
                        }
                    }
                }
            }
        });

        let client = TciClient::connect(&addr.ip().to_string(), addr.port());
        assert!(client.spot(&TciSpot {
            lifetime_secs: 1,
            ..spot()
        }));

        let placed = got_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(placed.starts_with("SPOT:3Y0J,"), "{placed}");

        // The link dies here. The deletion must survive it and arrive on the
        // NEW session — before the fix, nothing ever came.
        let deleted = got_rx
            .recv_timeout(Duration::from_secs(20))
            .expect("the deletion must survive the reconnect");
        assert_eq!(deleted, "SPOT_DELETE:3Y0J;");
    }

    /// A radio that is not there must not panic, block, or lose the client —
    /// the spot is queued, the connect fails, and the count says so.
    #[test]
    fn a_dead_radio_is_counted_not_fatal() {
        // Port 1 on loopback: nothing listens, connect fails fast.
        let client = TciClient::connect("127.0.0.1", 1);
        assert!(client.spot(&spot()), "queued regardless");
        for _ in 0..100 {
            if client.counters.failed.load(Ordering::Relaxed) > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(client.counters.sent.load(Ordering::Relaxed), 0);
        assert!(client.counters.failed.load(Ordering::Relaxed) >= 1);
    }
}
