//! Global (admin-owned) configuration — plan §4.
//!
//! Loaded from `config/dxca.toml`; every field has a default so a missing
//! file yields a runnable server. Per-user state (ClubLog credentials,
//! alert preferences) never lives here — that is SQLite territory (M4).
//! Defaults keep DXCA 1.x's shack wiring (UDP-PIPELINE.md): telnet 7575,
//! sources MSHV 2333 / JTDX 2334 / WSJTX 2335, passthrough → RUMlog 2237.

use dxca_connect::broadcast::{DestinationConfig, Format};
use serde::Deserialize;
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::path::Path;

pub const DEFAULT_PATH: &str = "config/dxca.toml";

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UdpSource {
    pub name: String,
    pub port: u16,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Web GUI + API bind address.
    pub web_bind: String,
    /// Telnet cluster server port.
    pub telnet_port: u16,
    /// WSJT-X/JTDX source listeners.
    pub udp_sources: Vec<UdpSource>,
    /// UDP broadcast destinations.
    pub broadcast_destinations: Vec<BroadcastDestination>,
    /// Rebroadcast dedupe window (CALL-BAND-MODE), 1.x default 60 s.
    pub dedupe_window_secs: u64,
    /// In-memory spot ring size served to the web UI.
    pub spot_ring_capacity: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            web_bind: "0.0.0.0:7580".into(),
            telnet_port: 7575,
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

    /// Enabled destinations in the broadcaster's terms.
    pub fn broadcast_destinations(&self) -> Vec<DestinationConfig> {
        self.broadcast_destinations
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
