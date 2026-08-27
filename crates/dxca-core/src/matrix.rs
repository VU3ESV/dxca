//! Per-user worked/confirmed matrix — port of the Swift `LogMatrix`.
//! Serde field names match the Swift `Codable` JSON exactly, so the 1.x
//! app's `matrix.json` deserializes as-is (sets are JSON arrays, the
//! integer-keyed map is a JSON object with stringified keys).

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DxccStatus {
    /// All worked, regardless of confirmation.
    pub bands: HashSet<String>,
    pub modes: HashSet<String>,
    /// "20M-DATA"-style band-mode slots.
    pub slots: HashSet<String>,
    // Confirmed only (LoTW / QSL / eQSL received).
    #[serde(rename = "confirmedBands")]
    pub confirmed_bands: HashSet<String>,
    #[serde(rename = "confirmedModes")]
    pub confirmed_modes: HashSet<String>,
    #[serde(rename = "confirmedSlots")]
    pub confirmed_slots: HashSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LogMatrix {
    #[serde(rename = "byDXCC")]
    pub by_dxcc: HashMap<i32, DxccStatus>,
    /// Lowercased calls already worked — fast path for exact-call checks.
    #[serde(rename = "workedCalls")]
    pub worked_calls: HashSet<String>,
}

impl LogMatrix {
    /// Build a matrix from ADIF text — the exact `ClubLogClient` loop from
    /// 1.x (band filter empty): explicit DXCC field wins over resolution,
    /// unknown/deleted/invalid entities are skipped, modes collapse to
    /// award buckets. Returns the matrix and the total record count (the
    /// 1.x `qsoCount`).
    pub fn build_from_adif(
        content: &str,
        resolver: &crate::dxcc::DxccResolver,
    ) -> (LogMatrix, usize) {
        let records = crate::adif::parse(content);
        let count = records.len();
        let mut matrix = LogMatrix::default();
        for r in &records {
            let (Some(call), Some(band), Some(mode)) = (r.call(), r.band(), r.mode()) else {
                continue;
            };
            let Some(d) = r.dxcc().or_else(|| resolver.resolve(&call)) else {
                continue;
            };
            if d <= 0 || resolver.entity(d).is_none() {
                continue;
            }
            matrix.record(
                d,
                &band,
                crate::modes::canonical(&mode),
                &call,
                r.is_confirmed(),
            );
        }
        (matrix, count)
    }

    pub fn record(&mut self, dxcc: i32, band: &str, mode: &str, call: &str, confirmed: bool) {
        let s = self.by_dxcc.entry(dxcc).or_default();
        let slot = format!("{band}-{mode}");
        s.bands.insert(band.to_string());
        s.modes.insert(mode.to_string());
        s.slots.insert(slot.clone());
        if confirmed {
            s.confirmed_bands.insert(band.to_string());
            s.confirmed_modes.insert(mode.to_string());
            s.confirmed_slots.insert(slot);
        }
        self.worked_calls.insert(call.to_lowercase());
    }

    pub fn status(&self, dxcc: i32) -> Option<&DxccStatus> {
        self.by_dxcc.get(&dxcc)
    }

    pub fn total_dxcc_count(&self) -> usize {
        self.by_dxcc.len()
    }

    /// Award totals for the station card. Confirmed DXCC counts entities with
    /// **at least one** confirmed slot — the DXCC-award rule — not entities
    /// whose every slot is confirmed.
    pub fn stats(&self) -> MatrixStats {
        let challenge = |bands: &HashSet<String>| {
            bands
                .iter()
                .filter(|b| crate::bands::is_challenge_band(b))
                .count()
        };
        MatrixStats {
            dxcc_worked: self.by_dxcc.len(),
            dxcc_confirmed: self
                .by_dxcc
                .values()
                .filter(|s| !s.confirmed_slots.is_empty())
                .count(),
            slots_worked: self.by_dxcc.values().map(|s| s.slots.len()).sum(),
            slots_confirmed: self.by_dxcc.values().map(|s| s.confirmed_slots.len()).sum(),
            challenge_worked: self.by_dxcc.values().map(|s| challenge(&s.bands)).sum(),
            challenge_confirmed: self
                .by_dxcc
                .values()
                .map(|s| challenge(&s.confirmed_bands))
                .sum(),
        }
    }
}

/// What the Spots screen's station card reports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixStats {
    pub dxcc_worked: usize,
    pub dxcc_confirmed: usize,
    /// Band × MODE combinations — this crate's "slot".
    pub slots_worked: usize,
    pub slots_confirmed: usize,
    /// **DXCC Challenge** points: entity × band over the ten Challenge bands,
    /// mode-agnostic. Distinct from `slots_*` in both respects — a station
    /// worked on 20M in CW and FT8 is two slots but one Challenge point, and
    /// a 60M contact is a slot but never a Challenge point.
    ///
    /// The award counts `challenge_confirmed` (1000 points to claim,
    /// endorsements every 500). `challenge_worked` is carried alongside it
    /// because the gap between the two is the QSL chase.
    pub challenge_worked: usize,
    pub challenge_confirmed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_tracks_worked_and_confirmed() {
        let mut m = LogMatrix::default();
        m.record(324, "20M", "DATA", "VU2ABC", false);
        m.record(324, "20M", "CW", "VU2XYZ", true);
        let s = m.status(324).unwrap();
        assert_eq!(s.slots, HashSet::from(["20M-DATA".into(), "20M-CW".into()]));
        assert_eq!(s.confirmed_slots, HashSet::from(["20M-CW".into()]));
        assert!(m.worked_calls.contains("vu2abc"));
        assert_eq!(m.total_dxcc_count(), 1);
    }

    #[test]
    fn challenge_counts_entity_bands_not_slots() {
        let mut m = LogMatrix::default();
        // India on 20M in two modes: two slots, but ONE Challenge point.
        m.record(324, "20M", "DATA", "VU2AAA", true);
        m.record(324, "20M", "CW", "VU2BBB", true);
        // A second band adds a second point.
        m.record(324, "40M", "CW", "VU2CCC", true);
        // 60M is a real slot and a real band — and worth nothing here.
        m.record(324, "60M", "PHONE", "VU2DDD", true);
        // Worked but unconfirmed: counts as worked, not as a point.
        m.record(291, "15M", "CW", "K1AAA", false);

        let s = m.stats();
        assert_eq!(s.slots_confirmed, 4, "20M×2 + 40M + 60M");
        assert_eq!(s.challenge_confirmed, 2, "20M + 40M; 60M excluded");
        assert_eq!(s.challenge_worked, 3, "the above plus K1AAA on 15M");
        assert_eq!(s.dxcc_worked, 2);
        assert_eq!(s.dxcc_confirmed, 1, "K1AAA has nothing confirmed");
    }

    #[test]
    fn swift_matrix_json_shape_roundtrips() {
        // Shape produced by the Swift app's JSONEncoder for LogMatrix.
        let json = r#"{
            "byDXCC": {"324": {
                "bands": ["20M"], "modes": ["DATA"], "slots": ["20M-DATA"],
                "confirmedBands": [], "confirmedModes": [], "confirmedSlots": []
            }},
            "workedCalls": ["vu2cpl"]
        }"#;
        let m: LogMatrix = serde_json::from_str(json).unwrap();
        assert!(m.status(324).unwrap().bands.contains("20M"));
        let re: LogMatrix = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(m, re);
    }
}
