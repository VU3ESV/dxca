//! The spot model shared by every pipeline stage.
//!
//! M0 skeleton: fields cover what both ingest paths (WSJT-X UDP decode,
//! DX-cluster line) can supply. M1 ports the exact semantics from the Swift
//! `SpotMessage` (dedupe window, callsign heuristics) with golden tests;
//! nothing here is final until then. A documented `From` mapping to
//! `meridian_proto::Spot` is planned to live in this file (plan §6) once
//! integration becomes concrete.

use serde::{Deserialize, Serialize};

/// Where a spot entered the aggregator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpotSource {
    /// A WSJT-X/JTDX instance, identified by the operator-assigned source name.
    Udp { name: String },
    /// A DX-cluster node, identified by the configured node name.
    Cluster { node: String },
}

/// One aggregated spot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spot {
    /// Unix time (seconds) the spot was received by DXCA.
    pub received_unix: u64,
    pub source: SpotSource,
    /// The station being spotted.
    pub dx_call: String,
    /// The spotting station (the decoder's own call for UDP sources).
    pub de_call: String,
    /// RF frequency in Hz (dial + audio offset for digital modes).
    pub freq_hz: u64,
    /// Normalized mode (FT8, FT4, CW, ...). Digital modes group as DATA for
    /// award slots — that mapping lives in the classifier, not here.
    pub mode: String,
    /// SNR in dB where the source supplies one (UDP decodes do, cluster
    /// lines usually via comment only).
    pub snr_db: Option<i32>,
    /// Free-text tail: the decode message text or the cluster comment.
    pub comment: String,
}

impl Spot {
    /// Key used by the duplicate-suppression window. M1 must port the exact
    /// v1.8.x semantics (60 s window, per dx_call + band + mode); until the
    /// band resolver lands this keys on raw kHz, which is strictly tighter
    /// (never merges spots v1.8.x would keep apart).
    pub fn dedupe_key(&self) -> String {
        format!("{}|{}|{}", self.dx_call, self.freq_hz / 1000, self.mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(freq_hz: u64) -> Spot {
        Spot {
            received_unix: 1_756_200_000,
            source: SpotSource::Udp {
                name: "JTDX".into(),
            },
            dx_call: "P5DX".into(),
            de_call: "VU2CPL".into(),
            freq_hz,
            mode: "FT8".into(),
            snr_db: Some(-12),
            comment: "CQ P5DX PM95".into(),
        }
    }

    #[test]
    fn dedupe_key_ignores_sub_khz_drift() {
        assert_eq!(spot(14_074_250).dedupe_key(), spot(14_074_900).dedupe_key());
    }

    #[test]
    fn dedupe_key_separates_bands() {
        assert_ne!(spot(14_074_250).dedupe_key(), spot(7_074_250).dedupe_key());
    }
}
