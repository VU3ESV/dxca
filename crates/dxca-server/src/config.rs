//! Global (admin-owned) configuration — plan §4.
//!
//! Loaded from `config/dxca.toml`; every field has a default so a missing
//! file yields a runnable server. Per-user state (ClubLog credentials,
//! alert preferences) never lives here — that is SQLite territory (M4).
//! Defaults keep DXCA 1.x's shack wiring (UDP-PIPELINE.md): telnet 7575,
//! sources MSHV 2333 / JTDX 2334 / WSJTX 2335, passthrough → RUMlog 2237.

use dxca_connect::broadcast::{DestinationConfig, Format};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::path::Path;

pub const DEFAULT_PATH: &str = "config/dxca.toml";

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UdpSource {
    pub name: String,
    pub port: u16,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterNode {
    pub name: String,
    pub host: String,
    pub port: u16,
    /// Callsign sent at the login prompt.
    pub login_call: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BroadcastDestination {
    pub name: String,
    pub ip: Ipv4Addr,
    pub port: u16,
    /// "cluster" | "wsjtx" | "passthrough" (unknown → cluster, 1.x parity).
    #[serde(default)]
    pub format: String,
    /// Source-name allowlist; empty = all sources.
    #[serde(default)]
    pub sources: Vec<String>,
    /// Ignore the dedupe/filter verdict — send every spot (RBN-style).
    #[serde(default)]
    pub unfiltered: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// NOTE: scalar fields are declared before the array-of-tables fields —
/// TOML serialization (Config::save) requires that ordering.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Web GUI + API bind address.
    pub web_bind: String,
    /// Telnet cluster server port.
    pub telnet_port: u16,
    /// Offer `LOGIN` on the telnet server, letting an account authenticate
    /// (`docs/TELNET-INTERACTIVE.md`). **Default false, deliberately:** an
    /// upgrade must never silently give port 7575 capabilities the operator
    /// did not ask for — this ships to other people's Pis, whose node
    /// sessions carry *their* callsign. With it off the server behaves
    /// exactly as it always has.
    pub telnet_interactive: bool,
    /// Rebroadcast dedupe window (CALL-BAND-MODE), 1.x default 60 s.
    pub dedupe_window_secs: u64,
    /// In-memory spot ring size served to the web UI.
    pub spot_ring_capacity: usize,
    /// Directory for runtime state: dxca.db (users, sessions, matrices)
    /// and the cached cty.xml.
    pub data_dir: String,
    /// How often to re-download ClubLog's cty.xml, in days; 0 disables it.
    /// Server-wide for the same reason the LoTW list is: one file, one
    /// resolver, shared by every account.
    ///
    /// The API key it needs is deliberately **not** here — this file is
    /// installed 0644 while `data/dxca.db` is 0600, so the key lives in the
    /// database with the other secrets (README §Secrets). Only the cadence,
    /// which is not a secret, is a file setting.
    pub cty_refresh_days: u64,
    /// How often to re-download the LoTW users list, in days; 0 disables it.
    /// Server-wide rather than per-user because the list itself is — one
    /// file, one download, shared by every account. A week is well inside
    /// the rate at which the list actually moves, and the file is ~6 MB.
    pub lotw_refresh_days: u64,
    /// Test/debug override: point ClubLog downloads at this base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clublog_base_override: Option<String>,
    /// Test/debug override: point Telegram sends at this base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telegram_base_override: Option<String>,
    /// WSJT-X/JTDX source listeners.
    pub udp_sources: Vec<UdpSource>,
    /// DX-cluster telnet nodes to ingest (M3). Default: none — node
    /// choice and credentials are operator-specific.
    pub cluster_nodes: Vec<ClusterNode>,
    /// UDP broadcast destinations.
    pub broadcast_destinations: Vec<BroadcastDestination>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            web_bind: "0.0.0.0:7580".into(),
            telnet_port: 7575,
            telnet_interactive: false,
            udp_sources: vec![
                UdpSource {
                    name: "MSHV".into(),
                    port: 2333,
                    enabled: true,
                },
                UdpSource {
                    name: "JTDX".into(),
                    port: 2334,
                    enabled: true,
                },
                UdpSource {
                    name: "WSJTX".into(),
                    port: 2335,
                    enabled: true,
                },
            ],
            cluster_nodes: Vec::new(),
            broadcast_destinations: vec![BroadcastDestination {
                name: "RUMlog".into(),
                ip: Ipv4Addr::LOCALHOST,
                port: 2237,
                format: "passthrough".into(),
                sources: Vec::new(),
                unfiltered: false,
                enabled: true,
            }],
            dedupe_window_secs: 60,
            spot_ring_capacity: 5000,
            data_dir: "data".into(),
            cty_refresh_days: 7,
            lotw_refresh_days: 7,
            clublog_base_override: None,
            telegram_base_override: None,
        }
    }
}

impl Config {
    /// Load from `path`, falling back to defaults when the file is absent.
    /// A present-but-invalid file is an error — silently ignoring a typo'd
    /// config and running on defaults would be the dishonest failure mode.
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
    }

    /// Persist to `path` (the M5 web-editing flow). The file is fully
    /// rewritten — hand-written comments live in dxca.example.toml, not
    /// the live config.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let body = toml::to_string_pretty(self).map_err(|e| format!("serialize config: {e}"))?;
        let text = format!(
            "# DXCA configuration — rewritten by the web UI (System page) on\n\
             # every save. Keep annotated notes in config/dxca.example.toml.\n\n{body}"
        );
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        }
        std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
    }

    /// Enabled destinations in the broadcaster's terms.
    pub fn broadcast_destinations(&self) -> Vec<DestinationConfig> {
        destinations_of(&self.broadcast_destinations)
    }
}

/// The same conversion over any list, so an aggregate assembled from several
/// accounts can be applied without first pretending to be a `Config`.
pub fn destinations_of(dests: &[BroadcastDestination]) -> Vec<DestinationConfig> {
    {
        dests
            .iter()
            .filter(|d| d.enabled)
            .map(|d| DestinationConfig {
                id: d.name.clone(),
                ip: d.ip,
                port: d.port,
                format: Format::from_str_lossy(&d.format),
                allowed_sources: d.sources.iter().cloned().collect::<HashSet<_>>(),
                unfiltered: d.unfiltered,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_shack_defaults() {
        let cfg = Config::load(Path::new("does/not/exist.toml")).unwrap();
        assert_eq!(cfg.telnet_port, 7575);
        assert_eq!(cfg.udp_sources.len(), 3);
        assert_eq!(cfg.udp_sources[1].name, "JTDX");
        assert_eq!(cfg.broadcast_destinations[0].port, 2237);
        assert_eq!(cfg.dedupe_window_secs, 60);
    }

    #[test]
    fn unknown_key_is_an_error() {
        let err = toml::from_str::<Config>("telnet_prot = 7575").unwrap_err();
        assert!(err.to_string().contains("telnet_prot"));
    }

    #[test]
    fn save_load_roundtrip() {
        let mut cfg = Config::default();
        cfg.cluster_nodes.push(ClusterNode {
            name: "VE7CC".into(),
            host: "ve7cc.net".into(),
            port: 23,
            login_call: "VU2CPL-2".into(),
            password: String::new(),
            enabled: true,
        });
        let path = std::env::temp_dir().join(format!("dxca-cfg-{}.toml", std::process::id()));
        cfg.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.cluster_nodes.len(), 1);
        assert_eq!(loaded.cluster_nodes[0].host, "ve7cc.net");
        assert_eq!(loaded.udp_sources.len(), 3);
        assert_eq!(loaded.telnet_port, 7575);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn toml_arrays_parse() {
        let cfg: Config = toml::from_str(
            r#"
            telnet_port = 7576
            [[udp_sources]]
            name = "JTDX"
            port = 2334
            [[broadcast_destinations]]
            name = "RBN"
            ip = "192.168.1.255"
            port = 2250
            format = "wsjtx"
            sources = ["JTDX"]
            unfiltered = true
            "#,
        )
        .unwrap();
        assert_eq!(cfg.udp_sources.len(), 1);
        let dests = cfg.broadcast_destinations();
        assert_eq!(dests.len(), 1);
        assert!(dests[0].unfiltered);
        assert!(dests[0].allowed_sources.contains("JTDX"));
    }
}
