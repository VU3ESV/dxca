//! DX-cluster node manager — starts one lifted [`ClusterClient`] per
//! configured node, tracks per-node honest status (the 1.x three-state
//! badge: connected-unproven / proven-live / down), and converts inbound
//! cluster spots into synthetic decodes exactly the way 1.x
//! `handleClusterSpot` did.

use crate::pipeline::PipelineInput;
use dxca_connect::dxcluster::{ClientConfig, ClientEvent, ClusterClient, ParsedSpot, SpotKind};
use dxca_core::Spot;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};

#[derive(Debug, Clone, Default, Serialize)]
pub struct NodeStatus {
    /// Human status text, 1.x pill-style ("Connecting…", "Connected",
    /// "Live", "Reconnecting: <reason>").
    pub state: String,
    pub connected: bool,
    /// Proven live: login acked or data actually flowing (never mere TCP).
    pub proven: bool,
    pub spot_count: u64,
    pub last_spot_unix: Option<i64>,
    /// Consecutive unproven connection attempts (resets on proof).
    pub attempt: u32,
}

/// A non-spot line from one node's session, published for consumers that
/// need the node's own words rather than its spots — the command router
/// (`docs/TELNET-INTERACTIVE.md`) being the first. Spots keep their existing
/// path into the pipeline and are deliberately absent here.
#[derive(Clone, Debug)]
pub struct NodeLine {
    pub node: String,
    pub event: ClientEvent,
}

/// Given first refusal on every node event, before it reaches the spot
/// pipeline.
///
/// This exists for one rule: a `SHOW/DX` reply arrives as `DX de …` lines
/// that parse as perfectly good spots but are **historical**, often hours
/// old. Letting them through would re-announce them to every logger and
/// fire Telegram alerts for last week's QSOs. The command router claims
/// them while one of its response windows is open, and returning `true`
/// here is what keeps them out.
///
/// Called on the node's event thread, so implementations must not block.
pub trait NodeEventFilter: Send + Sync {
    /// `true` = consumed; the event must go no further.
    fn intercept(&self, node: &str, event: &ClientEvent) -> bool;
}

pub struct NodeManager {
    statuses: Arc<Mutex<HashMap<String, NodeStatus>>>,
    /// Set once, before nodes start. A `OnceLock` rather than a constructor
    /// argument because the filter needs the manager (to write commands to
    /// nodes) and the manager needs the filter — the cycle has to be closed
    /// after both exist.
    filter: Arc<std::sync::OnceLock<Arc<dyn NodeEventFilter>>>,
    /// name → (config fingerprint, running client). Interior mutability so
    /// the M5 hot-apply works through the shared Arc.
    clients: Mutex<HashMap<String, (String, ClusterClient)>>,
    /// Fan-out of non-spot node events. Lagging subscribers lose the oldest
    /// lines rather than stalling the node thread — a slow telnet client
    /// must never back-pressure spot ingestion.
    lines: broadcast::Sender<NodeLine>,
}

impl Default for NodeManager {
    fn default() -> Self {
        Self::new()
    }
}

fn fingerprint(cfg: &ClientConfig) -> String {
    format!(
        "{}:{}:{}:{}",
        cfg.host, cfg.port, cfg.login_call, cfg.password
    )
}

impl NodeManager {
    pub fn new() -> Self {
        let (lines, _) = broadcast::channel(256);
        NodeManager {
            statuses: Arc::new(Mutex::new(HashMap::new())),
            clients: Mutex::new(HashMap::new()),
            lines,
            filter: Arc::new(std::sync::OnceLock::new()),
        }
    }

    /// Install the event filter. Call before starting nodes; a second call
    /// is ignored, which keeps the "set once" contract honest rather than
    /// silently swapping the filter out from under a running node thread.
    pub fn set_event_filter(&self, filter: Arc<dyn NodeEventFilter>) {
        let _ = self.filter.set(filter);
    }

    pub fn statuses(&self) -> HashMap<String, NodeStatus> {
        self.statuses.lock().unwrap().clone()
    }

    /// Subscribe to non-spot node lines (prompts, announcements, WWV, and
    /// anything else the node says). Nothing consumes this in production
    /// yet — it is the feed the command router will read.
    pub fn subscribe_lines(&self) -> broadcast::Receiver<NodeLine> {
        self.lines.subscribe()
    }

    /// Write a raw command line to one node's session.
    ///
    /// `false` means the node is not configured. A `true` only means the
    /// line was handed to the client: it is dropped there if the session is
    /// not logged in yet (`ClusterClient::send_line` gates on `ready()`),
    /// which is why callers must check the node is live first rather than
    /// treating this as delivery confirmation.
    pub fn send_line(&self, node: &str, line: &str) -> bool {
        match self.clients.lock().unwrap().get(node) {
            Some((_, client)) => {
                client.send_line(line);
                true
            }
            None => false,
        }
    }

    /// Hot-apply a node list: diff by name + config fingerprint. Removed or
    /// changed nodes stop (off the async runtime — a supervisor join can
    /// block up to its connect timeout); new/changed ones start.
    pub fn apply(&self, nodes: &[crate::config::ClusterNode], tx: &mpsc::Sender<PipelineInput>) {
        let wanted: HashMap<String, ClientConfig> = nodes
            .iter()
            .filter(|n| n.enabled)
            .map(|n| {
                let mut cfg = ClientConfig::new(&n.host, n.port, &n.login_call);
                cfg.password = n.password.clone();
                (n.name.clone(), cfg)
            })
            .collect();

        let mut to_start: Vec<(String, ClientConfig)> = Vec::new();
        let mut retired: Vec<ClusterClient> = Vec::new();
        {
            let mut clients = self.clients.lock().unwrap();
            let names: Vec<String> = clients.keys().cloned().collect();
            for name in names {
                let keep = wanted
                    .get(&name)
                    .is_some_and(|cfg| clients[&name].0 == fingerprint(cfg));
                if !keep && let Some((_, client)) = clients.remove(&name) {
                    retired.push(client);
                    self.statuses.lock().unwrap().remove(&name);
                }
            }
            for (name, cfg) in &wanted {
                if !clients.contains_key(name) {
                    to_start.push((name.clone(), cfg.clone()));
                }
            }
        }
        if !retired.is_empty() {
            // Stopping joins the supervisor thread (up to its connect
            // timeout) — never on the async runtime.
            tokio::task::spawn_blocking(move || drop(retired));
        }
        for (name, cfg) in to_start {
            self.start_node(name, cfg, tx.clone());
        }
    }

    /// Start one node client and its event-consumer thread. Spots flow into
    /// the pipeline channel; status updates land in the shared map.
    pub fn start_node(&self, name: String, cfg: ClientConfig, tx: mpsc::Sender<PipelineInput>) {
        let fp = fingerprint(&cfg);
        let (client, events) = ClusterClient::start(cfg);
        if let Some((_, old)) = self
            .clients
            .lock()
            .unwrap()
            .insert(name.clone(), (fp, client))
        {
            // Replaced in place — retire the old client off-thread.
            tokio::task::spawn_blocking(move || drop(old));
        }
        self.statuses.lock().unwrap().insert(
            name.clone(),
            NodeStatus {
                state: "Connecting…".into(),
                ..NodeStatus::default()
            },
        );

        let statuses = self.statuses.clone();
        let lines = self.lines.clone();
        let filter = self.filter.clone();
        std::thread::Builder::new()
            .name(format!("dxca-node-{name}"))
            .spawn(move || {
                // The client's event channel closes when the supervisor
                // stops; the thread ends with it.
                while let Ok(event) = events.recv() {
                    // First refusal to the command router. A claimed event
                    // belongs to somebody's `SHOW/DX` reply and must not be
                    // treated as live traffic — status counters included,
                    // or a history query would inflate the node's spot
                    // count and its "last spot" clock.
                    if let Some(f) = filter.get()
                        && f.intercept(&name, &event)
                    {
                        continue;
                    }
                    let mut spot_to_send = None;
                    {
                        let mut map = statuses.lock().unwrap();
                        let st = map.entry(name.clone()).or_default();
                        match &event {
                            ClientEvent::Connected => {
                                st.connected = true;
                                st.proven = false;
                                st.state = "Connected".into();
                            }
                            ClientEvent::LoggedIn => {
                                // Session usable; the pill only turns green
                                // on Proven (1.x honest-status rule).
                                if !st.proven {
                                    st.state = "Connected".into();
                                }
                            }
                            ClientEvent::Proven => {
                                st.proven = true;
                                st.attempt = 0;
                                st.state = "Live".into();
                            }
                            ClientEvent::Disconnected { reason } => {
                                st.connected = false;
                                st.proven = false;
                                st.attempt += 1;
                                st.state = format!("Reconnecting: {reason}");
                            }
                            ClientEvent::Spot { spot, .. } => {
                                st.spot_count += 1;
                                st.last_spot_unix = Some(now_unix());
                                spot_to_send = Some(synthetic_spot(&name, spot));
                            }
                            // Not status-bearing, but not worthless either:
                            // these are the node's own words, and a command
                            // reply is made of them. Published below.
                            ClientEvent::Wwv(_)
                            | ClientEvent::Announce(_)
                            | ClientEvent::Line(_)
                            | ClientEvent::Prompt(_) => {}
                        }
                    }
                    // Publish the node's own words. `send` errs only when
                    // nobody is subscribed, which is the normal case today.
                    if matches!(
                        event,
                        ClientEvent::Wwv(_)
                            | ClientEvent::Announce(_)
                            | ClientEvent::Line(_)
                            | ClientEvent::Prompt(_)
                    ) {
                        let _ = lines.send(NodeLine {
                            node: name.clone(),
                            event: event.clone(),
                        });
                    }
                    if let Some(spot) = spot_to_send
                        && tx.blocking_send(PipelineInput::Cluster(spot)).is_err()
                    {
                        return; // pipeline gone
                    }
                }
            })
            .expect("spawn node event thread");
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before 1970")
        .as_secs() as i64
}

/// 1.x `handleClusterSpot` parity: a cluster spot becomes a synthetic
/// decode — message `CQ <call>`, receive-time stamp, SNR and mode scraped
/// from the comment, dial = spot frequency with zero offset.
/// `node_name` is the CONFIGURED name, which may carry an owner prefix once
/// feeds are per-account. It is split here: `source_name` keeps the bare
/// name because it becomes the spotter callsign on the cluster line, and the
/// owner travels in its own field. See `Spot::owner`.
fn synthetic_spot(node_name: &str, p: &ParsedSpot) -> Spot {
    let (node_owner, node_display) = crate::feeds::split(node_name);
    // Three sources, best first. `p.mode` is the parser's own token-based
    // read, which also infers CW from a `WPM` token and RTTY from `BPS` —
    // strictly better than re-scanning the comment, and it used to be thrown
    // away here. Then the widened comment scrape. Then, only if the spot
    // genuinely says nothing (DB0SUE and N2WQ relay human spots with free-
    // text comments), the band plan.
    let reported = match p.mode.as_deref() {
        Some(m) if !m.is_empty() => m.to_string(),
        _ => scrape_mode(&p.comment),
    };
    let (mode, mode_inferred) = dxca_core::modes::resolve(&reported, p.freq_khz / 1000.0);
    // The parser already worked out what the comment reported; this used to
    // discard it and stamp "CQ" on every cluster spot, which is why the
    // CQ-only filter matched 100% of the feed. A skimmer spot with no marker
    // still counts — a skimmer only reports stations calling CQ.
    let is_cq = matches!(p.kind, SpotKind::Cq | SpotKind::Dx) || p.spotter_is_skimmer;
    Spot {
        time_unix: now_unix(),
        snr_db: p.snr_db.unwrap_or(0),
        delta_time_s: 0.0,
        delta_frequency_hz: 0,
        mode,
        mode_inferred,
        // Kept as "CQ <call>" whatever the kind: it is what `dx_callsign`
        // parses the callsign out of, and the outbound cluster line and the
        // dedupe key both ride on that. What the spotter actually typed is
        // carried in `comment` and is what the UI shows.
        message: format!("CQ {}", p.call),
        is_cq,
        comment: p.comment.clone(),
        low_confidence: false,
        off_air: false,
        dial_frequency_hz: (p.freq_khz * 1000.0) as u64,
        source_name: node_display.to_string(),
        // The parser had this all along; it used to be discarded here, which
        // is why a relayed spot showed only the node that carried it.
        spotter: (!p.spotter.is_empty()).then(|| p.spotter.clone()),
        // Used for `is_cq` above and then discarded until 2026-08-28, which
        // left the UI unable to tell a skimmer catch from a hand-typed spot
        // — especially once the `-#` marker had been stripped off the call.
        is_skimmer: p.spotter_is_skimmer,
        owner: node_owner.to_string(),
    }
}

/// Modes recognised in a spot comment. Wider than the 1.x list of ten, which
/// knew no `USB`/`LSB` — so an ordinary phone spot commented "USB" got no
/// mode at all while the same spot commented "SSB" got one.
///
/// `canonical` maps these into the three award buckets, so `USB`/`LSB` need
/// no translation here; they land in PHONE on their own.
#[rustfmt::skip]
const KNOWN_MODES: &[&str] = &[
    // Digital
    "FT8", "FT4", "JS8", "Q65", "FST4", "FST4W", "WSPR", "MSK144",
    "JT65", "JT9", "JT6M", "RTTY", "PSK", "PSK31", "PSK63", "PSK125",
    "OLIVIA", "CONTESTIA", "MFSK", "DOMINO", "THOR", "HELL", "SSTV", "ROS",
    // CW
    "CW",
    // Phone
    "SSB", "USB", "LSB", "PHONE", "AM", "FM", "C4FM", "DMR", "DSTAR",
];

/// First recognised mode **token** in the comment.
///
/// Matches whole tokens, not substrings. The 1.x version used
/// `comment.contains("CW")`, which read a mode out of "QSL via N1CW", turned
/// "tnx OM DO5SSB relay" into SSB, and scored "CWops number 123" as CW. A
/// wrong mode is worse than none: it silently lands the spot in the wrong
/// DXCC award slot, where nothing ever flags it.
fn scrape_mode(comment: &str) -> String {
    comment
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_uppercase())
        .find(|t| KNOWN_MODES.contains(&t.as_str()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dxca_connect::dxcluster::wire::parse_spot_line;

    /// A configured name carrying an owner splits: the bare name is what
    /// reaches the cluster line, the owner rides in its own field.
    ///
    /// Getting this wrong is silent — `format::format` strips the colon and
    /// sends `DX de VU2CPLDB0SUE:` to every logger, which reads as a real
    /// callsign. Hence a test on each producer.
    #[test]
    fn a_namespaced_node_name_splits_into_source_and_owner() {
        let line = "DX de W3LPL-#:    14074.0  K1JT         FT8            1428Z";
        let parsed = parse_spot_line(line).expect("parses");

        let plain = synthetic_spot("DB0SUE", &parsed);
        assert_eq!(plain.source_name, "DB0SUE");
        assert_eq!(plain.owner, "", "an unqualified name has no owner");

        let owned = synthetic_spot("VU2CPL:DB0SUE", &parsed);
        assert_eq!(
            owned.source_name, "DB0SUE",
            "the wire must see the bare name"
        );
        assert_eq!(owned.owner, "VU2CPL");
        // Everything else is untouched by the split.
        assert_eq!(owned.spotter, plain.spotter);
        assert_eq!(owned.is_skimmer, plain.is_skimmer);
    }

    /// The whole point of the `spotter` field: a relaying node is not the
    /// station that heard the DX. HamAlert, DB0SUE and N2WQ all carry other
    /// people's spots, and attributing them to the node hides who was
    /// actually on the air.
    #[test]
    fn the_spotting_station_survives_the_relay() {
        let p = parse_spot_line(
            "DX de VU2XYZ:    14074.0  K1JT           FT8 -10 dB                  1428Z",
        )
        .unwrap();
        let s = synthetic_spot("N2WQ-2", &p);
        assert_eq!(s.source_name, "N2WQ-2", "the feed that carried it");
        assert_eq!(
            s.spotter.as_deref(),
            Some("VU2XYZ"),
            "the station that heard it"
        );
    }

    /// Stripping the `-#` makes the spotter readable but destroys the one
    /// thing that said "machine". The flag is what an operator hunting real
    /// contacts filters on, so it has to survive alongside the clean call.
    #[test]
    fn a_skimmer_stays_identifiable_after_its_marker_is_stripped() {
        let p = parse_spot_line(
            "DX de W3LPL-#:   14025.3  W9XYZ          12 dB  22 WPM  CQ         1423Z",
        )
        .unwrap();
        let s = synthetic_spot("N2WQ-2", &p);
        assert_eq!(s.spotter.as_deref(), Some("W3LPL"), "readable call");
        assert!(s.is_skimmer, "but still known to be a machine");

        // The same operator spotting by hand is NOT a skimmer spot.
        let p = parse_spot_line(
            "DX de W3LPL:     14025.3  W9XYZ          CQ                        1423Z",
        )
        .unwrap();
        let s = synthetic_spot("N2WQ-2", &p);
        assert_eq!(s.spotter.as_deref(), Some("W3LPL"), "same callsign");
        assert!(!s.is_skimmer, "different kind of spot");
    }

    /// A skimmer's `-#` suffix is stripped by the parser, so the spotter is
    /// the operator's callsign rather than a machine name with punctuation.
    #[test]
    fn a_skimmer_spotter_is_recorded_without_its_marker() {
        let p = parse_spot_line(
            "DX de K1ABC-#:   14025.3  W9XYZ          12 dB  22 WPM  CQ         1423Z",
        )
        .unwrap();
        let s = synthetic_spot("HamAlert", &p);
        assert_eq!(s.spotter.as_deref(), Some("K1ABC"));
        assert_eq!(s.source_name, "HamAlert");
    }

    #[test]
    fn cluster_spot_becomes_synthetic_decode() {
        let p = parse_spot_line(
            "DX de W3LPL:     14074.0  K1JT           FT8 -10 dB                  1428Z",
        )
        .unwrap();
        let s = synthetic_spot("VE7CC", &p);
        assert_eq!(s.message, "CQ K1JT");
        assert_eq!(s.dial_frequency_hz, 14_074_000);
        assert_eq!(s.snr_db, -10);
        assert_eq!(s.mode, "FT8");
        assert_eq!(s.source_name, "VE7CC");
        assert_eq!(s.dx_callsign().as_deref(), Some("K1JT"));
    }

    #[test]
    fn mode_scrape_order_matches_1x() {
        assert_eq!(scrape_mode("FT8 -15 dB"), "FT8");
        assert_eq!(scrape_mode("22 WPM CW CQ"), "CW");
        assert_eq!(scrape_mode("loud ssb signal"), "SSB");
        assert_eq!(scrape_mode("nothing known"), "");
    }

    #[test]
    fn mode_scrape_matches_tokens_not_substrings() {
        // Each of these read as a mode under the 1.x `contains` scan, and a
        // wrong mode is worse than none — it lands the spot in an award slot
        // it does not belong to, where nothing ever flags it.
        assert_eq!(scrape_mode("QSL via N1CW"), "");
        assert_eq!(scrape_mode("tnx OM DO5SSB relay"), "");
        assert_eq!(scrape_mode("CWops number 123"), "");
        assert_eq!(scrape_mode("worked him on FT8W"), "");
        // Punctuation is a boundary, so a real mode still reads through it.
        assert_eq!(scrape_mode("CQ,FT8,-12dB"), "FT8");
        assert_eq!(scrape_mode("up 1 (USB)"), "USB");
    }

    #[test]
    fn mode_scrape_knows_more_than_the_1x_ten() {
        // USB/LSB especially: an ordinary phone spot commented "USB" got no
        // mode at all from the 1.x list while "SSB" got one.
        for (comment, want) in [
            ("14200 USB loud", "USB"),
            ("LSB 59", "LSB"),
            ("JS8 -12 dB", "JS8"),
            ("Q65 -18", "Q65"),
            ("PSK63 cq", "PSK63"),
            ("olivia 8/250", "OLIVIA"),
            ("FM simplex", "FM"),
            ("sstv now", "SSTV"),
        ] {
            assert_eq!(scrape_mode(comment), want, "comment: {comment:?}");
        }
    }

    #[test]
    fn comment_without_a_mode_infers_from_frequency() {
        // The DB0SUE / N2WQ case: a human spot relayed as free text. Before
        // this the mode was "" and the classifier scored it as DATA.
        let p =
            parse_spot_line("DX de DB0SUE:    14200.0  N2WQ           up 2                1428Z")
                .unwrap();
        let s = synthetic_spot("DB0SUE", &p);
        assert_eq!(s.mode, "SSB", "20m phone segment");
        assert!(s.mode_inferred, "and it must say so");

        // A comment that names the mode is never overridden by the guess.
        let p =
            parse_spot_line("DX de DB0SUE:    14200.0  N2WQ           CW nr 5             1428Z")
                .unwrap();
        let s = synthetic_spot("DB0SUE", &p);
        assert_eq!(s.mode, "CW");
        assert!(!s.mode_inferred, "reported beats inferred");
    }

    #[test]
    fn cq_is_taken_from_the_spot_not_from_the_synthetic_message() {
        // Every cluster spot's message is "CQ <call>", so a message-text
        // test said yes to all of them and the CQ-only filter did nothing.
        let go = |line: &str| synthetic_spot("NODE", &parse_spot_line(line).unwrap());

        // Human spot, free-text comment, no marker → not a CQ.
        let s = go("DX de DB0SUE:    14200.0  N2WQ           up 2                1428Z");
        assert!(!s.is_cq, "an unmarked human spot is not a CQ");
        assert_eq!(s.comment, "up 2", "and the real comment is carried");
        assert!(
            s.message.starts_with("CQ "),
            "message stays the callsign carrier for dx_callsign/format/dedupe"
        );

        // The comment says CQ → a CQ.
        assert!(go("DX de DB0SUE:    14200.0  N2WQ           CQ DX               1428Z").is_cq);

        // Skimmer with no marker → still a CQ: skimmers only report CQ calls.
        let s = go("DX de W3LPL-#:   14025.0  K1ABC          22 WPM -15 dB       1428Z");
        assert!(s.is_cq, "unmarked skimmer spot counts as CQ");

        // Beacon → not a CQ.
        assert!(!go("DX de DB0SUE:    14100.0  4U1UN          NCDXF BCN           1428Z").is_cq);
    }

    #[test]
    fn skimmer_wpm_mode_is_no_longer_discarded() {
        // wire.rs infers CW from the WPM token; synthetic_spot used to throw
        // that away and re-scan the comment, which found nothing.
        let p =
            parse_spot_line("DX de W3LPL-#:   7020.0  K1ABC          CQ 22 WPM -15 dB    1428Z")
                .unwrap();
        let s = synthetic_spot("W3LPL", &p);
        assert_eq!(s.mode, "CW");
        assert!(
            !s.mode_inferred,
            "the WPM token reported it, we did not guess"
        );
    }
}
