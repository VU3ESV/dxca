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
        self.stats_excluding(&HashSet::new())
    }

    /// [`stats`](Self::stats) with `skip` entities left out — the ARRL
    /// deleted list, in practice, so the totals match the standings an
    /// operator is comparing against. Worked-but-deleted QSOs are real
    /// contacts and stay in the matrix; they simply stop scoring here.
    pub fn stats_excluding(&self, skip: &HashSet<i32>) -> MatrixStats {
        let kept = || self.by_dxcc.iter().filter(|(adif, _)| !skip.contains(adif));
        let challenge = |bands: &HashSet<String>| {
            bands
                .iter()
                .filter(|b| crate::bands::is_challenge_band(b))
                .count()
        };
        MatrixStats {
            dxcc_worked: kept().count(),
            dxcc_confirmed: kept()
                .filter(|(_, s)| !s.confirmed_slots.is_empty())
                .count(),
            slots_worked: kept().map(|(_, s)| s.slots.len()).sum(),
            slots_confirmed: kept().map(|(_, s)| s.confirmed_slots.len()).sum(),
            challenge_worked: kept().map(|(_, s)| challenge(&s.bands)).sum(),
            challenge_confirmed: kept().map(|(_, s)| challenge(&s.confirmed_bands)).sum(),
        }
    }

    /// Entities worked and confirmed **per band** and **per mode class** —
    /// the DXCC-by-band table an operator actually plans from, which the
    /// six totals in [`MatrixStats`] cannot show.
    ///
    /// Counts entities, not QSOs: a band's number is how many DXCC entities
    /// have at least one contact there, which is what the award tracks.
    /// Empty rows are kept so the gaps are visible — a band with nothing on
    /// it is the most interesting row on the page.
    pub fn by_band_and_mode(&self) -> BandModeStats {
        self.by_band_and_mode_excluding(&HashSet::new())
    }

    /// [`by_band_and_mode`](Self::by_band_and_mode), minus `skip`.
    pub fn by_band_and_mode_excluding(&self, skip: &HashSet<i32>) -> BandModeStats {
        let count = |has: &dyn Fn(&DxccStatus) -> bool| {
            self.by_dxcc
                .iter()
                .filter(|(adif, s)| !skip.contains(adif) && has(s))
                .count()
        };

        let bands = crate::bands::SELECTABLE_BANDS
            .iter()
            .map(|b| SliceCount {
                key: (*b).to_string(),
                worked: count(&|s: &DxccStatus| s.bands.contains(*b)),
                confirmed: count(&|s: &DxccStatus| s.confirmed_bands.contains(*b)),
            })
            .collect();

        let modes = crate::modes::CLASSES
            .iter()
            .map(|m| SliceCount {
                key: (*m).to_string(),
                worked: count(&|s: &DxccStatus| s.modes.contains(*m)),
                confirmed: count(&|s: &DxccStatus| s.confirmed_modes.contains(*m)),
            })
            .collect();

        BandModeStats { bands, modes }
    }
}

/// Entities worked/confirmed for one band or one mode class.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceCount {
    pub key: String,
    pub worked: usize,
    pub confirmed: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BandModeStats {
    /// In `bands::SELECTABLE_BANDS` order — 160M first, the operator's
    /// customary ordering, not whatever a hash map iterated.
    pub bands: Vec<SliceCount>,
    /// In `modes::CLASSES` order: CW, PHONE, DATA.
    pub modes: Vec<SliceCount>,
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

    /// The ARRL counts current entities; a deleted one is a real QSO that
    /// scores nothing. Excluding must drop it from every total at once —
    /// entities, slots and Challenge points — or the card shows a mixture
    /// that matches no published standing.
    #[test]
    fn deleted_entities_drop_out_of_every_total_together() {
        let mut m = LogMatrix::default();
        // India (324, current) on two Challenge bands, both confirmed.
        m.record(324, "20M", "CW", "VU2ABC", true);
        m.record(324, "40M", "CW", "VU2XYZ", true);
        // Abu Ail (002, DELETED) on one Challenge band, confirmed.
        m.record(2, "20M", "CW", "OLD1", true);

        let all = m.stats();
        assert_eq!(all.dxcc_worked, 2);
        assert_eq!(all.dxcc_confirmed, 2);
        assert_eq!(all.challenge_worked, 3, "2 bands + 1 band");
        assert_eq!(all.challenge_confirmed, 3);

        let current = m.stats_excluding(&HashSet::from([2]));
        assert_eq!(current.dxcc_worked, 1, "Abu Ail no longer counts");
        assert_eq!(current.dxcc_confirmed, 1);
        assert_eq!(current.slots_worked, 2, "only India's two slots");
        assert_eq!(current.slots_confirmed, 2);
        assert_eq!(current.challenge_worked, 2);
        assert_eq!(current.challenge_confirmed, 2);

        // The QSO itself is untouched — it was still a contact.
        assert!(m.status(2).is_some(), "the deleted entity stays in the log");
    }

    /// The per-band table has to agree with the totals, or the two cards on
    /// screen contradict each other.
    #[test]
    fn the_band_table_excludes_deleted_entities_too() {
        let mut m = LogMatrix::default();
        m.record(324, "20M", "CW", "VU2ABC", true);
        m.record(2, "20M", "CW", "OLD1", true);

        let band = |st: &BandModeStats, k: &str| {
            st.bands.iter().find(|b| b.key == k).unwrap().clone()
        };
        assert_eq!(band(&m.by_band_and_mode(), "20M").worked, 2);
        let current = m.by_band_and_mode_excluding(&HashSet::from([2]));
        assert_eq!(band(&current, "20M").worked, 1);
        assert_eq!(band(&current, "20M").confirmed, 1);
    }

    /// Excluding nothing must be identical to not excluding — `stats()`
    /// delegates to the filtered path, so a bug there would silently change
    /// every existing total.
    #[test]
    fn excluding_an_empty_set_changes_nothing() {
        let mut m = LogMatrix::default();
        m.record(324, "20M", "CW", "VU2ABC", true);
        m.record(291, "40M", "DATA", "K1ABC", false);
        assert_eq!(m.stats(), m.stats_excluding(&HashSet::new()));
        assert_eq!(
            m.by_band_and_mode(),
            m.by_band_and_mode_excluding(&HashSet::new())
        );
    }

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
    fn band_and_mode_slices_count_entities_not_qsos() {
        let mut m = LogMatrix::default();
        // India on 20M twice, two modes and two calls — one ENTITY on 20M.
        m.record(324, "20M", "DATA", "VU2ABC", true);
        m.record(324, "20M", "CW", "VU2XYZ", false);
        // USA on 20M (confirmed) and on 40M (not).
        m.record(291, "20M", "CW", "K1ABC", true);
        m.record(291, "40M", "CW", "K9XYZ", false);

        let s = m.by_band_and_mode();
        let band = |k: &str| s.bands.iter().find(|b| b.key == k).unwrap().clone();
        let mode = |k: &str| s.modes.iter().find(|b| b.key == k).unwrap().clone();

        assert_eq!(band("20M").worked, 2, "two entities on 20M, not four QSOs");
        assert_eq!(band("20M").confirmed, 2);
        assert_eq!(band("40M").worked, 1);
        assert_eq!(band("40M").confirmed, 0, "worked but unconfirmed");
        assert_eq!(band("15M").worked, 0, "an empty band still gets a row");

        assert_eq!(mode("CW").worked, 2);
        assert_eq!(mode("CW").confirmed, 1, "only the US CW slot is confirmed");
        assert_eq!(mode("DATA").worked, 1);
        assert_eq!(mode("PHONE").worked, 0);

        // Order is the operator's, not a hash map's.
        assert_eq!(s.bands.first().unwrap().key, "160M");
        assert_eq!(
            s.modes.iter().map(|m| m.key.as_str()).collect::<Vec<_>>(),
            vec!["CW", "PHONE", "DATA"]
        );
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
