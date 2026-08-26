//! Global (admin-owned) configuration — plan §4.
//!
//! Loaded from `config/dxca.toml`; every field has a default so a missing
//! file yields a runnable server. Per-user state (ClubLog credentials,
//! alert preferences) never lives here — that is SQLite territory (M4).
//! Defaults keep DXCA 1.x's ports (plan §3): telnet 7575, UDP listen 2237,
//! so the shack pipeline docs stay valid with only an IP change.

use serde::Deserialize;
use std::path::Path;

pub const DEFAULT_PATH: &str = "config/dxca.toml";

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Web GUI + API bind address.
    pub web_bind: String,
    /// Telnet cluster server port (served from M2).
    pub telnet_port: u16,
    /// WSJT-X/JTDX UDP listen port (served from M2).
    pub udp_listen_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            web_bind: "0.0.0.0:7580".into(),
            telnet_port: 7575,
            udp_listen_port: 2237,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let cfg = Config::load(Path::new("does/not/exist.toml")).unwrap();
        assert_eq!(cfg.telnet_port, 7575);
        assert_eq!(cfg.udp_listen_port, 2237);
    }

    #[test]
    fn unknown_key_is_an_error() {
        let err = toml::from_str::<Config>("telnet_prot = 7575").unwrap_err();
        assert!(err.to_string().contains("telnet_prot"));
    }
}
