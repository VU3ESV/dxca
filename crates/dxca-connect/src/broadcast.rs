//! UDP broadcast destinations — port of the Swift `UDPBroadcaster`, with
//! the v1.8.2 passthrough format and the v1.8.3 fail-counter semantics
//! preserved exactly:
//!  - passthrough destinations are fed **only** by [`UdpBroadcaster::send_raw`]
//!    (one send per inbound datagram, verbatim); the per-spot path skips
//!    them before any bookkeeping, so they never book phantom failures;
//!  - `unfiltered` destinations ignore the caller's `passes_filters` flag
//!    (upstream aggregators like RBN do their own dedupe);
//!  - SO_BROADCAST is on unconditionally so LAN-broadcast addresses work.

use dxca_core::wsjtx;
use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Plain-text DX cluster line.
    Cluster,
    /// Synthesized WSJT-X Status+Decode pair per spot.
    Wsjtx,
    /// Raw relay of every inbound source datagram.
    Passthrough,
}

impl Format {
    /// Swift `init(rawString:)`: unknown strings fall back to cluster.
    pub fn from_str_lossy(s: &str) -> Format {
        match s {
            "wsjtx" => Format::Wsjtx,
            "passthrough" => Format::Passthrough,
            _ => Format::Cluster,
        }
    }
}

pub struct DestinationConfig {
    /// Stable identifier for the counters (destination name in config).
    pub id: String,
    pub ip: Ipv4Addr,
    pub port: u16,
    pub format: Format,
    /// Source-name allowlist; empty = all sources.
    pub allowed_sources: HashSet<String>,
    pub unfiltered: bool,
}

struct Destination {
    cfg: DestinationConfig,
    socket: UdpSocket,
    addr: SocketAddrV4,
}

/// One spot's payload fields for the per-spot formats.
pub struct SpotPayload<'a> {
    pub cluster_line: &'a str,
    pub source_name: &'a str,
    pub callsign: Option<&'a str>,
    pub frequency_hz: u64,
    pub snr_db: i32,
    pub mode: &'a str,
    pub time_ms: u32,
}

#[derive(Debug, Default, Clone)]
pub struct Counters {
    pub sent: HashMap<String, u64>,
    pub failed: HashMap<String, u64>,
}

impl Counters {
    pub fn total_sent(&self) -> u64 {
        self.sent.values().sum()
    }

    pub fn total_failed(&self) -> u64 {
        self.failed.values().sum()
    }
}

pub struct UdpBroadcaster {
    destinations: Vec<Destination>,
    counters: Mutex<Counters>,
}

impl UdpBroadcaster {
    /// Build from enabled destinations. Sockets are unconnected
    /// fire-and-forget senders with SO_BROADCAST set.
    pub fn new(configs: Vec<DestinationConfig>) -> std::io::Result<Self> {
        let mut destinations = Vec::with_capacity(configs.len());
        for cfg in configs {
            let socket = UdpSocket::bind(("0.0.0.0", 0))?;
            socket.set_broadcast(true)?;
            let addr = SocketAddrV4::new(cfg.ip, cfg.port);
            destinations.push(Destination { cfg, socket, addr });
        }
        Ok(UdpBroadcaster {
            destinations,
            counters: Mutex::new(Counters::default()),
        })
    }

    pub fn counters(&self) -> Counters {
        self.counters.lock().unwrap().clone()
    }

    /// Per-spot broadcast to cluster/wsjtx destinations. `passes_filters`
    /// is the caller's verdict (dedupe window + display filters); filtered
    /// destinations require it, `unfiltered` ones ignore it.
    pub fn broadcast_spot(&self, spot: &SpotPayload<'_>, passes_filters: bool) {
        let mut results: Vec<(&str, bool)> = Vec::new();
        for dest in &self.destinations {
            // Skip passthrough BEFORE any bookkeeping (v1.8.3 fix).
            if dest.cfg.format == Format::Passthrough {
                continue;
            }
            if !source_allowed(&dest.cfg, spot.source_name) {
                continue;
            }
            if !dest.cfg.unfiltered && !passes_filters {
                continue;
            }
            let ok = match dest.cfg.format {
                Format::Cluster => {
                    let payload = format!("{}\r\n", spot.cluster_line);
                    send(dest, payload.as_bytes())
                }
                Format::Wsjtx => match spot.callsign {
                    Some(call) if !call.is_empty() => {
                        let (status, decode) = wsjtx::encode_spot(
                            call,
                            spot.frequency_hz,
                            spot.snr_db,
                            spot.mode,
                            spot.time_ms,
                        );
                        let s1 = send(dest, &status);
                        let s2 = send(dest, &decode);
                        s1 && s2
                    }
                    _ => false,
                },
                Format::Passthrough => unreachable!("skipped above"),
            };
            results.push((&dest.cfg.id, ok));
        }
        self.book(&results);
    }

    /// Relay one raw inbound datagram verbatim to every passthrough
    /// destination whose allowlist admits the source.
    pub fn send_raw(&self, data: &[u8], source_name: &str) {
        if data.is_empty() {
            return;
        }
        let mut results: Vec<(&str, bool)> = Vec::new();
        for dest in &self.destinations {
            if dest.cfg.format != Format::Passthrough {
                continue;
            }
            if !source_allowed(&dest.cfg, source_name) {
                continue;
            }
            results.push((&dest.cfg.id, send(dest, data)));
        }
        self.book(&results);
    }

    fn book(&self, results: &[(&str, bool)]) {
        if results.is_empty() {
            return;
        }
        let mut c = self.counters.lock().unwrap();
        for (id, ok) in results {
            let map = if *ok { &mut c.sent } else { &mut c.failed };
            *map.entry(id.to_string()).or_insert(0) += 1;
        }
    }
}

fn source_allowed(cfg: &DestinationConfig, source_name: &str) -> bool {
    cfg.allowed_sources.is_empty() || cfg.allowed_sources.contains(source_name)
}

fn send(dest: &Destination, data: &[u8]) -> bool {
    dest.socket.send_to(data, dest.addr).is_ok()
}
