//! DX-cluster node manager — starts one lifted [`ClusterClient`] per
//! configured node, tracks per-node honest status (the 1.x three-state
//! badge: connected-unproven / proven-live / down), and converts inbound
//! cluster spots into synthetic decodes exactly the way 1.x
//! `handleClusterSpot` did.

use crate::pipeline::PipelineInput;
use dxca_connect::dxcluster::{ClientConfig, ClientEvent, ClusterClient, ParsedSpot};
use dxca_core::Spot;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

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

pub struct NodeManager {
    statuses: Arc<Mutex<HashMap<String, NodeStatus>>>,
    clients: Vec<ClusterClient>,
}

impl Default for NodeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeManager {
    pub fn new() -> Self {
        NodeManager {
            statuses: Arc::new(Mutex::new(HashMap::new())),
            clients: Vec::new(),
        }
    }

    pub fn statuses(&self) -> HashMap<String, NodeStatus> {
        self.statuses.lock().unwrap().clone()
    }

    /// Start one node client and its event-consumer thread. Spots flow into
    /// the pipeline channel; status updates land in the shared map.
    pub fn start_node(&mut self, name: String, cfg: ClientConfig, tx: mpsc::Sender<PipelineInput>) {
        let (client, events) = ClusterClient::start(cfg);
        self.clients.push(client);
        self.statuses.lock().unwrap().insert(
            name.clone(),
            NodeStatus {
                state: "Connecting…".into(),
                ..NodeStatus::default()
            },
        );

        let statuses = self.statuses.clone();
        std::thread::Builder::new()
            .name(format!("dxca-node-{name}"))
            .spawn(move || {
                // The client's event channel closes when the supervisor
                // stops; the thread ends with it.
                while let Ok(event) = events.recv() {
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
                            ClientEvent::Wwv(_)
                            | ClientEvent::Announce(_)
                            | ClientEvent::Line(_) => {}
                        }
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
fn synthetic_spot(node_name: &str, p: &ParsedSpot) -> Spot {
    Spot {
        time_unix: now_unix(),
        snr_db: p.snr_db.unwrap_or(0),
        delta_time_s: 0.0,
        delta_frequency_hz: 0,
        mode: scrape_mode(&p.comment),
        message: format!("CQ {}", p.call),
        low_confidence: false,
        off_air: false,
        dial_frequency_hz: (p.freq_khz * 1000.0) as u64,
        source_name: node_name.to_string(),
    }
}

/// The 1.x known-mode scan: first list entry found anywhere in the
/// uppercased comment (order matters — FT8 before CW etc.).
fn scrape_mode(comment: &str) -> String {
    const KNOWN: [&str; 10] = [
        "FT8", "FT4", "CW", "SSB", "RTTY", "PSK31", "JT65", "JT9", "MSK144", "WSPR",
    ];
    let upper = comment.to_uppercase();
    KNOWN
        .iter()
        .find(|m| upper.contains(**m))
        .map(|m| m.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dxca_connect::dxcluster::wire::parse_spot_line;

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
}
