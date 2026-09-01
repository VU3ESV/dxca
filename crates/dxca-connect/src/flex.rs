//! FlexRadio spot injection over the SmartSDR API (TCP 4992).
//!
//! Puts DXCA's **alerts** on the panadapter, colour-coded by level, so a rare
//! one is visible where you are already looking instead of only in a phone
//! notification.
//!
//! ## Why this is not a broadcast destination
//!
//! Every format in [`crate::broadcast`] is a UDP datagram to an address.
//! 4992 is a **TCP session**: connect, then sequenced `C<n>|…` commands the
//! radio answers with `R<n>|…`. A `Format::Flex` would have produced a
//! configuration row that looks right and silently does nothing. Same
//! reasoning that made MQTT a sibling module rather than a fourth format.
//!
//! ## The command, and where it came from
//!
//! Ported from Manoj's working Node-RED flow rather than derived from the
//! API docs, so the field set is one already proven against a real radio:
//!
//! ```text
//! C7001|spot add rx_freq=14.074 callsign=3Y0J mode=FT8 comment=NEW_DXCC-Bouvet \
//!   spotter_callsign=VU2CPL timestamp=1788077349 color=0xFFF5636B priority=2 \
//!   lifetime_seconds=1200 source=DXCA
//! ```
//!
//! Every value is space-delimited `key=value`, so **no value may contain a
//! space** — a comment with one silently truncates the command at that point
//! and the radio parses the remainder as garbage. [`sanitize`] is what stops
//! that, and it is not cosmetic.
//!
//! ## The connection has to be drained
//!
//! 4992 is bidirectional: from the moment a client connects, the radio
//! streams `S<handle>|…` status messages continuously, whether anyone asked
//! or not. A writer that never reads fills its receive buffer, the TCP window
//! closes, and the radio's sends block on us. So every connection carries a
//! reader thread whose whole job is to throw those bytes away.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant};

/// Wait between reconnect attempts. Connections are only attempted when a
/// spot needs sending, so this only matters when alerts arrive in a burst
/// while the radio is off.
/// SmartSDR's command port.
///
/// The `tci::DEFAULT_PORT` counterpart, named for the same reason: with more
/// than one radio per account the default is applied per device, and a bare
/// 4992 repeated at each of those sites is the kind of constant that gets
/// changed in three places and missed in a fourth.
pub const DEFAULT_PORT: u16 = 4992;

const RECONNECT_AFTER: Duration = Duration::from_secs(30);

/// Queue depth. Alerts are rare; anything approaching this means the radio
/// has gone away and the backlog is worthless anyway.
const QUEUE: usize = 256;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// One spot to place on the panadapter.
#[derive(Debug, Clone, PartialEq)]
pub struct FlexSpot {
    pub callsign: String,
    pub freq_mhz: f64,
    pub mode: String,
    /// Free text; spaces become underscores and it is clipped to 20.
    pub comment: String,
    pub spotter: String,
    pub timestamp_unix: i64,
    /// `0xAARRGGBB`, as SmartSDR expects.
    pub color: String,
    pub lifetime_secs: u64,
}

/// The radio's comment field, in characters.
pub const COMMENT_MAX: usize = 20;

/// Space-free, and short enough for the radio's comment field.
///
/// Whitespace to `_` and clipped to `max` **characters** — the Node-RED
/// flow's `.replace(/\s+/g,'_').substring(0,20)`. Byte-slicing would panic
/// on a multi-byte boundary, which a callsign will not produce but an entity
/// name might.
fn sanitize(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut kept = 0usize;
    let mut last_was_us = false;
    for ch in s.chars() {
        if kept >= max {
            break;
        }
        if ch.is_whitespace() {
            if !last_was_us {
                out.push('_');
                last_was_us = true;
                kept += 1;
            }
        } else {
            out.push(ch);
            last_was_us = false;
            kept += 1;
        }
    }
    out
}

/// The comment for one alert: `"<level> <entity>"` when it fits, otherwise
/// the **entity alone**.
///
/// Twenty characters is not many, and "NEW DXCC DPRK (NORTH KOREA)" clipped
/// to fit reads `NEW_DXCC_DPRK_(NORTH` — the level twice over (the colour
/// already says it) and the entity cut mid-word. When both will not fit, the
/// entity is the half worth keeping.
pub fn comment_for(level_label: &str, entity: Option<&str>) -> String {
    let Some(entity) = entity.filter(|e| !e.is_empty()) else {
        return level_label.to_string();
    };
    let both = format!("{level_label} {entity}");
    if sanitize(&both, usize::MAX).chars().count() <= COMMENT_MAX {
        both
    } else {
        entity.to_string()
    }
}

/// Build one `spot add` command, newline-terminated.
///
/// Pure, so the wire format is testable without a radio.
pub fn spot_command(seq: u64, s: &FlexSpot) -> String {
    format!(
        "C{seq}|spot add rx_freq={:.4} callsign={} mode={} comment={} \
         spotter_callsign={} timestamp={} color={} priority=2 \
         lifetime_seconds={} source=DXCA\n",
        s.freq_mhz,
        sanitize(&s.callsign, 32),
        sanitize(&s.mode, 16),
        sanitize(&s.comment, COMMENT_MAX),
        sanitize(&s.spotter, 32),
        s.timestamp_unix,
        sanitize(&s.color, 16),
        s.lifetime_secs,
    )
}

#[derive(Default)]
pub struct Counters {
    pub sent: AtomicU64,
    pub failed: AtomicU64,
}

/// A live (or reconnecting) link to one radio.
///
/// Cheap to hold and safe to share: sending queues onto a channel and never
/// blocks the caller, which matters because the alert fan-out runs on the
/// spot pipeline's runtime.
pub struct FlexClient {
    tx: SyncSender<String>,
    seq: AtomicU64,
    pub counters: Arc<Counters>,
    pub target: String,
}

impl FlexClient {
    /// Start the worker. Does **not** connect — the first spot does that, so
    /// a configured-but-switched-off radio costs nothing until it is needed.
    pub fn connect(host: &str, port: u16) -> FlexClient {
        let (tx, rx) = std::sync::mpsc::sync_channel::<String>(QUEUE);
        let counters = Arc::new(Counters::default());
        let target = format!("{host}:{port}");
        {
            let target = target.clone();
            let counters = counters.clone();
            std::thread::spawn(move || worker(target, rx, counters));
        }
        FlexClient {
            tx,
            // Matches the Node-RED flow's base. Nothing depends on the
            // value; a distinctive start just makes the commands easy to
            // pick out in a packet capture.
            seq: AtomicU64::new(7000),
            counters,
            target,
        }
    }

    /// Queue one spot. `false` means it was dropped — the queue is full,
    /// which only happens when the radio has gone away.
    pub fn spot(&self, s: &FlexSpot) -> bool {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        match self.tx.try_send(spot_command(seq, s)) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.counters.failed.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }
}

fn worker(target: String, rx: std::sync::mpsc::Receiver<String>, counters: Arc<Counters>) {
    let mut conn: Option<TcpStream> = None;
    let mut last_attempt: Option<Instant> = None;

    for cmd in rx {
        if conn.is_none() {
            // Only one attempt per RECONNECT_AFTER, so a burst of alerts at
            // a dark radio does not become a burst of connect syscalls.
            let due = last_attempt.is_none_or(|t| t.elapsed() >= RECONNECT_AFTER);
            if due {
                last_attempt = Some(Instant::now());
                conn = dial(&target);
            }
        }
        let Some(stream) = conn.as_mut() else {
            counters.failed.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        match stream
            .write_all(cmd.as_bytes())
            .and_then(|()| stream.flush())
        {
            Ok(()) => {
                counters.sent.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                eprintln!("dxca: flex {target}: write failed, will reconnect: {e}");
                counters.failed.fetch_add(1, Ordering::Relaxed);
                conn = None;
            }
        }
    }
}

fn dial(target: &str) -> Option<TcpStream> {
    let addr: std::net::SocketAddr = target.parse().ok().or_else(|| {
        use std::net::ToSocketAddrs;
        target.to_socket_addrs().ok()?.next()
    })?;
    let stream = match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("dxca: flex {target}: connect failed: {e}");
            return None;
        }
    };
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let _ = stream.set_nodelay(true);
    // NOTHING is sent on connect. `C1|client program DXCA` was tried against
    // the real radio and refused — `R1|10000002|unknown client program`, so
    // SmartSDR validates the name against a list it knows rather than taking
    // any string. A rejected command every reconnect buys nothing, so the
    // handshake stays empty, which is also what the Node-RED flow this was
    // ported from always did.
    //
    // `client gui` is the command that would claim a station and its slices.
    // DXCA never sends it, never parses the handle the radio offers, and
    // issues nothing but `spot add`. A passive connect was checked against
    // the radio and creates nothing: `slices=3` in its greeting was Aether's,
    // already there.
    // The drain. Without it the radio's continuous status stream fills our
    // receive buffer and blocks its sends — see the module docs.
    if let Ok(reader) = stream.try_clone() {
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break; // radio closed the session
                }
            }
        });
    }
    println!("dxca: flex {target}: connected");
    Some(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot() -> FlexSpot {
        FlexSpot {
            callsign: "3Y0J".into(),
            freq_mhz: 14.074,
            mode: "FT8".into(),
            comment: "NEW DXCC Bouvet Island".into(),
            spotter: "VU2CPL".into(),
            timestamp_unix: 1_788_077_349,
            color: "0xFFF5636B".into(),
            lifetime_secs: 1200,
        }
    }

    /// The wire format, pinned against the Node-RED flow it was ported from.
    #[test]
    fn the_command_matches_the_working_flow() {
        let c = spot_command(7001, &spot());
        assert!(c.starts_with("C7001|spot add "), "{c}");
        assert!(c.ends_with('\n'), "the radio needs the newline");
        for part in [
            "rx_freq=14.0740",
            "callsign=3Y0J",
            "mode=FT8",
            "spotter_callsign=VU2CPL",
            "timestamp=1788077349",
            "color=0xFFF5636B",
            "priority=2",
            "lifetime_seconds=1200",
            "source=DXCA",
        ] {
            assert!(c.contains(part), "missing {part} in {c}");
        }
    }

    /// Every value is space-delimited `key=value`, so one space inside a
    /// value truncates the command and the radio parses the rest as
    /// nonsense. This is the assertion that keeps that from regressing.
    #[test]
    fn no_value_may_contain_a_space() {
        let c = spot_command(1, &spot());
        let body = c.trim_end().strip_prefix("C1|spot add ").unwrap();
        for field in body.split(' ') {
            assert!(
                field.contains('='),
                "field {field:?} has no '=' — a value leaked a space: {c}"
            );
        }
        assert!(c.contains("comment=NEW_DXCC_Bouvet_Isla"), "{c}");
    }

    /// 20 characters, and counted as characters: byte-slicing a multi-byte
    /// entity name would panic mid-boundary.
    #[test]
    fn the_comment_is_clipped_to_twenty_characters() {
        assert_eq!(sanitize("NEW DXCC Bouvet Island", 20).chars().count(), 20);
        let s = sanitize("NEW DXCC Åland Islands", 20);
        assert_eq!(s.chars().count(), 20);
        assert!(s.starts_with("NEW_DXCC_Åland"), "{s}");
    }

    /// A run of whitespace collapses to one underscore rather than several,
    /// so the 20 characters are spent on the name and not on padding.
    #[test]
    fn runs_of_whitespace_collapse() {
        assert_eq!(sanitize("a   b\tc", 20), "a_b_c");
    }

    /// Sequence numbers advance per spot — the radio pairs its `R<n>|`
    /// replies to them, and a repeat would be ambiguous.
    #[test]
    fn sequence_numbers_advance() {
        let c = FlexClient::connect("127.0.0.1", 1); // nothing listening: fine
        let first = c.seq.load(Ordering::Relaxed);
        c.spot(&spot());
        c.spot(&spot());
        assert_eq!(c.seq.load(Ordering::Relaxed), first + 2);
    }

    /// The whole path against a real socket: connect on first spot, write
    /// the command, and keep the session for the next one. The string tests
    /// above cannot show any of that.
    #[test]
    fn it_connects_and_writes_to_a_real_socket() {
        use std::io::BufRead;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let (got_tx, got_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            // A real radio talks first and never stops; this stands in for
            // that, and proves the client does not wedge when it does.
            let mut w = stream.try_clone().unwrap();
            let _ = w.write_all(b"V1.4.0.0\nH12345678\n");
            let mut r = std::io::BufReader::new(stream);
            for _ in 0..2 {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let _ = got_tx.send(line);
            }
        });

        let client = FlexClient::connect(&addr.ip().to_string(), addr.port());
        assert!(client.spot(&spot()));
        assert!(client.spot(&spot()));

        let first = got_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("first command should arrive");
        let second = got_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("second should reuse the same session");

        // The FIRST thing on the wire is a spot — nothing is sent on
        // connect, and `client gui` in particular is never sent, because
        // that is the command that would claim a station.
        assert!(first.starts_with("C7001|spot add "), "{first}");
        assert!(!first.contains("client"), "no handshake: {first}");
        assert!(first.contains("callsign=3Y0J"), "{first}");
        assert!(second.starts_with("C7002|"), "sequence advances: {second}");
        assert_eq!(client.counters.sent.load(Ordering::Relaxed), 2);
        assert_eq!(client.counters.failed.load(Ordering::Relaxed), 0);
    }

    /// Twenty characters is not many. When level and entity both fit, both
    /// go; when they do not, the entity wins, because the spot's colour has
    /// already said which level it is.
    #[test]
    fn the_comment_prefers_the_entity_over_the_level() {
        // Fits: 8 + 1 + 6 = 15.
        assert_eq!(comment_for("NEW DXCC", Some("Bouvet")), "NEW DXCC Bouvet");

        // Does not: the real case that prompted this — the old behaviour
        // rendered `NEW_DXCC_DPRK_(NORTH`, saying the level twice and cutting
        // the entity mid-word.
        assert_eq!(
            comment_for("NEW DXCC", Some("DPRK (NORTH KOREA)")),
            "DPRK (NORTH KOREA)"
        );
        // ...and on the wire that is the whole entity, not a fragment.
        let s = sanitize(
            &comment_for("NEW DXCC", Some("DPRK (NORTH KOREA)")),
            COMMENT_MAX,
        );
        assert_eq!(s, "DPRK_(NORTH_KOREA)");

        // No entity at all: the level is all there is to say.
        assert_eq!(comment_for("NEW DXCC", None), "NEW DXCC");
        assert_eq!(comment_for("NEW DXCC", Some("")), "NEW DXCC");

        // An entity too long even alone is still clipped by `spot_command`,
        // and clipped entity beats clipped level-plus-entity.
        let long = comment_for("? Slot", Some("SOUTH SANDWICH ISLANDS"));
        assert_eq!(long, "SOUTH SANDWICH ISLANDS");
        assert_eq!(sanitize(&long, COMMENT_MAX), "SOUTH_SANDWICH_ISLAN");
    }

    /// A radio that is not there must not panic, block, or lose the client —
    /// the spot is queued, the write fails, and the count says so.
    #[test]
    fn a_dead_radio_is_counted_not_fatal() {
        // Port 1 on loopback: nothing listens, connect fails fast.
        let client = FlexClient::connect("127.0.0.1", 1);
        assert!(client.spot(&spot()), "queued regardless");
        for _ in 0..50 {
            if client.counters.failed.load(Ordering::Relaxed) > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(client.counters.sent.load(Ordering::Relaxed), 0);
        assert!(client.counters.failed.load(Ordering::Relaxed) >= 1);
    }

    /// Frequency to four decimals: 14.074 must not render as `14.074` on one
    /// spot and `14.0740000001` on another.
    #[test]
    fn frequency_formats_consistently() {
        let mut s = spot();
        s.freq_mhz = 7.0;
        assert!(spot_command(1, &s).contains("rx_freq=7.0000"));
        s.freq_mhz = 14.074_123_9;
        assert!(spot_command(1, &s).contains("rx_freq=14.0741"));
    }
}
