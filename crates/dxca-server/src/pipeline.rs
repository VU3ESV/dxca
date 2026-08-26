//! The M2 spot path (docs/PLAN.md §10): UDP sources → raw passthrough →
//! parse → per-source dial tracking → Spot → 60 s dedupe window → ring
//! buffer + telnet fan-out + per-spot UDP broadcast. Mirrors the 1.x
//! `ContentView.handleDecode` orchestration, minus display filters (those
//! return with the web UI in M5) — `passes_filters` here is the
//! rebroadcast-dedupe verdict alone.

use crate::config::Config;
use dxca_connect::broadcast::{SpotPayload, UdpBroadcaster};
use dxca_connect::telnet::ClusterServer;
use dxca_connect::wsjtx_udp::{self, SourceDatagram};
use dxca_core::wsjtx::{self, Message};
use dxca_core::{Spot, format, time_from_decode_ms};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// One unit of pipeline work: a raw UDP datagram from a decoder source, or
/// an already-synthesized spot from a DX-cluster node (M3).
pub enum PipelineInput {
    Datagram(SourceDatagram),
    Cluster(Spot),
}

/// Shared state the web API reads.
pub struct PipelineState {
    pub spots: Mutex<VecDeque<Spot>>,
    pub broadcaster: Arc<UdpBroadcaster>,
    pub telnet: ClusterServer,
    /// Spots received per source name (proven-live counters).
    pub source_counts: Mutex<HashMap<String, u64>>,
    /// Every processed spot, for subscribers (alert fan-out — M4; the
    /// dashboard WebSocket — M5). Lagging subscribers skip, never stall.
    pub spot_events: tokio::sync::broadcast::Sender<Spot>,
}

impl PipelineState {
    pub fn recent_spots(&self, limit: usize) -> Vec<Spot> {
        let spots = self.spots.lock().unwrap();
        spots.iter().rev().take(limit).cloned().collect()
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before 1970")
        .as_secs() as i64
}

/// Start listeners + telnet server + pipeline task. Returns the shared
/// state handle and a sender for additional inputs (cluster-node spots);
/// runs until the process exits.
pub async fn start(
    cfg: &Config,
) -> std::io::Result<(Arc<PipelineState>, mpsc::Sender<PipelineInput>)> {
    let telnet = ClusterServer::start(cfg.telnet_port).await?;
    let broadcaster = Arc::new(UdpBroadcaster::new(cfg.broadcast_destinations())?);

    let (spot_events, _) = tokio::sync::broadcast::channel(1024);
    let state = Arc::new(PipelineState {
        spots: Mutex::new(VecDeque::new()),
        broadcaster: broadcaster.clone(),
        telnet,
        source_counts: Mutex::new(HashMap::new()),
        spot_events,
    });

    let (tx, rx) = mpsc::channel::<PipelineInput>(1024);
    for source in cfg.udp_sources.iter().filter(|s| s.enabled) {
        let (name, port) = (source.name.clone(), source.port);
        let (dg_tx, mut dg_rx) = mpsc::channel::<SourceDatagram>(256);
        let tx = tx.clone();
        tokio::spawn(async move {
            if let Err(e) = wsjtx_udp::run_listener(name.clone(), port, dg_tx).await {
                eprintln!("dxca: UDP source {name} on port {port} failed: {e}");
            }
        });
        let tx2 = tx.clone();
        tokio::spawn(async move {
            while let Some(dg) = dg_rx.recv().await {
                if tx2.send(PipelineInput::Datagram(dg)).await.is_err() {
                    return;
                }
            }
        });
    }

    let pipeline_state = state.clone();
    let dedupe_window = cfg.dedupe_window_secs as i64;
    let ring_capacity = cfg.spot_ring_capacity;
    tokio::spawn(async move {
        run_pipeline(rx, pipeline_state, dedupe_window, ring_capacity).await;
    });

    Ok((state, tx))
}

async fn run_pipeline(
    mut rx: mpsc::Receiver<PipelineInput>,
    state: Arc<PipelineState>,
    dedupe_window_secs: i64,
    ring_capacity: usize,
) {
    // Per-source dial frequency from the latest Status (1.x keeps it on
    // each listener); dedupe cache mirrors `rebroadcastCache`.
    let mut dial_by_source: HashMap<String, u64> = HashMap::new();
    let mut dedupe: HashMap<String, i64> = HashMap::new();

    while let Some(input) = rx.recv().await {
        let datagram = match input {
            PipelineInput::Cluster(spot) => {
                // Already a synthesized spot (nodes.rs) — shared tail only.
                process_spot(&state, &mut dedupe, spot, dedupe_window_secs, ring_capacity);
                continue;
            }
            PipelineInput::Datagram(dg) => dg,
        };

        // Passthrough first — raw, before parsing, exactly like 1.x's
        // onRawDatagram wiring.
        state
            .broadcaster
            .send_raw(&datagram.data, &datagram.source_name);

        let Some(parsed) = wsjtx::parse(&datagram.data) else {
            continue;
        };
        match parsed.message {
            Message::Status(status) => {
                dial_by_source.insert(datagram.source_name.clone(), status.dial_frequency_hz);
            }
            Message::Decode(decode) => {
                let now = now_unix();
                let dial = dial_by_source
                    .get(&datagram.source_name)
                    .copied()
                    .unwrap_or(0);
                let spot = Spot {
                    time_unix: time_from_decode_ms(now, decode.time_ms),
                    snr_db: decode.snr_db,
                    delta_time_s: decode.delta_time_s,
                    delta_frequency_hz: decode.delta_frequency_hz,
                    mode: decode.mode,
                    message: decode.message,
                    low_confidence: decode.low_confidence,
                    off_air: decode.off_air,
                    dial_frequency_hz: dial,
                    source_name: datagram.source_name.clone(),
                };
                process_spot(&state, &mut dedupe, spot, dedupe_window_secs, ring_capacity);
            }
            Message::Other(_) => {} // 1.x parity: types 0/3/5/12/… dropped
        }
    }
}

/// The shared pipeline tail for a spot from either ingest path: counters →
/// rebroadcast dedupe → telnet fan-out → UDP broadcast → ring.
fn process_spot(
    state: &PipelineState,
    dedupe: &mut HashMap<String, i64>,
    spot: Spot,
    dedupe_window_secs: i64,
    ring_capacity: usize,
) {
    let now = now_unix();
    *state
        .source_counts
        .lock()
        .unwrap()
        .entry(spot.source_name.clone())
        .or_insert(0) += 1;

    // Rebroadcast dedupe: first spot per CALL-BAND-MODE per window reaches
    // the telnet feed + filtered destinations.
    let passes = match spot.duplicate_key() {
        Some(key) => {
            // Opportunistic cleanup, like 1.x's 2000-entry sweep.
            if dedupe.len() > 2000 {
                dedupe.retain(|_, t| now - *t < dedupe_window_secs);
            }
            match dedupe.get(&key) {
                Some(last) if now - last < dedupe_window_secs => false,
                _ => {
                    dedupe.insert(key, now);
                    true
                }
            }
        }
        // 1.x parity: no dedupe key (no callsign) → never counts as a
        // duplicate → still broadcast (as UNKNOWN).
        None => true,
    };

    let line = format::format(&spot);
    if passes {
        state.telnet.broadcast_line(&line);
    }
    let dx_call = spot.dx_callsign();
    state.broadcaster.broadcast_spot(
        &SpotPayload {
            cluster_line: &line,
            source_name: &spot.source_name,
            callsign: dx_call.as_deref(),
            frequency_hz: spot.frequency_hz(),
            snr_db: spot.snr_db,
            mode: &spot.mode,
            time_ms: (spot.time_unix.rem_euclid(86_400) * 1000) as u32,
        },
        passes,
    );

    let _ = state.spot_events.send(spot.clone()); // no subscribers is fine
    let mut ring = state.spots.lock().unwrap();
    ring.push_back(spot);
    while ring.len() > ring_capacity {
        ring.pop_front();
    }
}
