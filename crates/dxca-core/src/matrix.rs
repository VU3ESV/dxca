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
    /// Build a matrix from ADIF text — the `ClubLogClient` loop from 1.x
    /// (band filter empty): explicit DXCC field wins over resolution,
    /// unknown/deleted/invalid entities are skipped, modes collapse to
    /// award buckets. Returns the matrix and the total record count (the
    /// 1.x `qsoCount`).
    ///
    /// **One deliberate divergence from 1.x**: contacts ClubLog lists as
    /// [invalid operations](crate::cty::InvalidOperation) are dropped
    /// entirely. 1.x scored them, which is why its DXCC-worked total could
    /// sit one above ClubLog's own — see the doc comment there. The
    /// *returned count* is still every record in the file, so the "N QSOs"
    /// figure keeps matching ClubLog's; it is only the award matrix that
    /// leaves them out.
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
            // Before resolution, and before `worked_calls`: an invalidated
            // contact is not a worked call either, so the same station
            // spotted again should still alert as new.
            let at = r.qso_datetime_unix();
            if resolver.is_invalid_operation(&call, at) {
                continue;
            }
            let Some(d) = r.dxcc().or_else(|| resolver.resolve(&call)) else {
                continue;
            };
            if d <= 0 || resolver.entity(d).is_none() {
                continue;
            }
            // Whitelisted entity, unlisted call — a `ZL8` prefix is not a
            // Kermadec credit. Checked on the *resolved* entity so it applies
            // whether the id came from ClubLog's DXCC field or from our own
            // prefix match.
            if resolver.is_whitelist_rejected(d, &call, at) {
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

        // The cells the two projections above only ever summarise: entities
        // holding the "20M-CW"-shaped slot itself. `record` builds that key
        // as `{band}-{mode}`, so the lookup is the same string.
        let grid = crate::modes::CLASSES
            .iter()
            .map(|m| ModeRow {
                mode: (*m).to_string(),
                bands: crate::bands::SELECTABLE_BANDS
                    .iter()
                    .map(|b| {
                        let slot = format!("{b}-{m}");
                        SliceCount {
                            key: (*b).to_string(),
                            worked: count(&|s: &DxccStatus| s.slots.contains(&slot)),
                            confirmed: count(&|s: &DxccStatus| s.confirmed_slots.contains(&slot)),
                        }
                    })
                    .collect(),
            })
            .collect();

        BandModeStats { bands, modes, grid }
    }
}

/// Entities worked/confirmed for one band or one mode class.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceCount {
    pub key: String,
    pub worked: usize,
    pub confirmed: usize,
}

/// One mode's row of the band × mode grid — entities worked/confirmed on
/// each band *in that mode*.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeRow {
    /// One of `modes::CLASSES`.
    pub mode: String,
    /// In `bands::SELECTABLE_BANDS` order, one cell per band. Empty cells are
    /// kept for the same reason empty band rows are.
    pub bands: Vec<SliceCount>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BandModeStats {
    /// In `bands::SELECTABLE_BANDS` order — 160M first, the operator's
    /// customary ordering, not whatever a hash map iterated.
    pub bands: Vec<SliceCount>,
    /// In `modes::CLASSES` order: CW, PHONE, DATA.
    pub modes: Vec<SliceCount>,
    /// The full cross product, one row per mode class.
    ///
    /// **`bands` and `modes` are not derivable from this by summing**, and
    /// that is the whole reason all three are carried. An entity worked on
    /// 20M in both CW and DATA is *one* entity on 20M but occupies *two*
    /// cells in the 20M column — adding the column up double-counts it.
    /// `bands` is this grid's mode-agnostic summary (RUMlog's "Mixed" row)
    /// and `modes` its band-agnostic one (RUMlog's "Total" column).
    pub grid: Vec<ModeRow>,
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

        let band =
            |st: &BandModeStats, k: &str| st.bands.iter().find(|b| b.key == k).unwrap().clone();
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

    /// Build a resolver knowing one entity, one prefix rule, and whatever
    /// invalid operations the test needs.
    fn resolver_with(ops: Vec<crate::cty::InvalidOperation>) -> crate::dxcc::DxccResolver {
        let entity = |adif: i32, name: &str, prefix: &str| crate::cty::DxccEntity {
            adif,
            name: name.into(),
            prefix: prefix.into(),
            cq_zone: 0,
            continent: String::new(),
            deleted: false,
            ..Default::default()
        };
        let mut r = crate::dxcc::DxccResolver::default();
        r.load(
            crate::cty::CtyData {
                entities: HashMap::from([
                    (180, entity(180, "MOUNT ATHOS", "SV/A")),
                    (236, entity(236, "GREECE", "SV")),
                    (48, entity(48, "EASTERN KIRIBATI", "T32")),
                ]),
                prefix_rules: vec![crate::cty::PrefixRule {
                    call: "SV".into(),
                    adif: 236,
                    is_exact: false,
                    start_unix: None,
                    end_unix: None,
                }],
                invalid_operations: ops,
            },
            0,
        );
        r
    }

    fn invalid_op(call: &str, start: &str, end: &str) -> crate::cty::InvalidOperation {
        crate::cty::InvalidOperation {
            call: call.into(),
            start_unix: crate::cty::parse_iso8601(start),
            end_unix: crate::cty::parse_iso8601(end),
        }
    }

    /// The VU24DX case, in miniature. Mount Athos is worked only as
    /// `SV2RSG/A`, an operation ClubLog rejects — so the entity must not be
    /// credited at all, which is what brought the app's DXCC-worked total
    /// down from 314 to ClubLog's own 313.
    #[test]
    fn an_entity_worked_only_through_an_invalid_operation_is_not_worked() {
        let adif = "<CALL:8>SV2RSG/A<QSO_DATE:8>20240906<TIME_ON:6>101500\
                    <BAND:3>20M<MODE:3>FT8<DXCC:3>180<eor>";

        // 1.x behaviour — no invalid list — credits the entity.
        let (m, count) = LogMatrix::build_from_adif(adif, &resolver_with(vec![]));
        assert_eq!(count, 1);
        assert_eq!(m.stats().dxcc_worked, 1, "1.x scored it");

        // With the list, the contact scores nothing...
        let ops = vec![invalid_op(
            "SV2RSG/A",
            "2024-09-05T00:00:00+00:00",
            "2024-09-09T23:59:59+00:00",
        )];
        let (m, count) = LogMatrix::build_from_adif(adif, &resolver_with(ops));
        assert_eq!(m.stats().dxcc_worked, 0, "ClubLog does not credit it");
        assert!(m.status(180).is_none());
        // ...and is not a worked call either, so the station spotted again
        // still alerts.
        assert!(m.worked_calls.is_empty());
        // The QSO count is the file's record count regardless — it has to
        // keep matching the "N QSOs" ClubLog reports.
        assert_eq!(count, 1, "still a QSO, just not an award-scoring one");
    }

    /// Only the rejected period is dropped. The same call outside the
    /// window, and the licensed operator's own call, both still count.
    #[test]
    fn invalid_operations_drop_only_the_rejected_contacts() {
        let ops = vec![invalid_op(
            "SV2RSG/A",
            "2024-09-05T00:00:00+00:00",
            "2024-09-09T23:59:59+00:00",
        )];
        let adif = "<CALL:8>SV2RSG/A<QSO_DATE:8>20240906<TIME_ON:6>101500\
                    <BAND:3>20M<MODE:3>FT8<DXCC:3>180<eor>\
                    <CALL:8>SV2RSG/A<QSO_DATE:8>20250101<TIME_ON:6>101500\
                    <BAND:3>40M<MODE:3>FT8<DXCC:3>180<eor>\
                    <CALL:6>SV2CPL<QSO_DATE:8>20240906<TIME_ON:6>101500\
                    <BAND:3>20M<MODE:2>CW<DXCC:3>236<eor>";
        let (m, count) = LogMatrix::build_from_adif(adif, &resolver_with(ops));
        assert_eq!(count, 3);
        assert_eq!(m.stats().dxcc_worked, 2, "Mount Athos survives on 40M");
        assert_eq!(
            m.status(180).unwrap().slots,
            HashSet::from(["40M-DATA".into()]),
            "only the September contact was dropped"
        );
        assert!(m.status(236).is_some(), "Greece is untouched");
    }

    /// The real VU24DX case, end to end: one `ZL8AC` QSO, resolved to
    /// Kermadec by the `ZL8` prefix, on an entity whose credits ClubLog
    /// restricts to a listed set that ZL8AC is not in. This is the QSO that
    /// made the app read 314 DXCC worked against ClubLog's 313.
    #[test]
    fn an_unlisted_call_earns_no_credit_in_a_whitelisted_entity() {
        let entity =
            |adif: i32, name: &str, prefix: &str, whitelist: bool| crate::cty::DxccEntity {
                adif,
                name: name.into(),
                prefix: prefix.into(),
                whitelist,
                ..Default::default()
            };
        let rule = |call: &str, adif: i32, is_exact: bool| crate::cty::PrefixRule {
            call: call.into(),
            adif,
            is_exact,
            start_unix: None,
            end_unix: None,
        };
        let mut r = crate::dxcc::DxccResolver::default();
        r.load(
            crate::cty::CtyData {
                entities: HashMap::from([
                    (133, entity(133, "KERMADEC ISLAND", "ZL8", true)),
                    (170, entity(170, "NEW ZEALAND", "ZL", false)),
                ]),
                prefix_rules: vec![
                    rule("ZL8", 133, false),
                    rule("ZL", 170, false),
                    // The one Kermadec operation ClubLog accepts here.
                    rule("ZL8X", 133, true),
                ],
                ..Default::default()
            },
            0,
        );

        let adif = "<CALL:5>ZL8AC<QSO_DATE:8>20250712<TIME_ON:6>101500\
                    <BAND:3>40M<MODE:3>FT8<DXCC:3>133<eor>";
        let (m, count) = LogMatrix::build_from_adif(adif, &r);
        assert_eq!(count, 1, "still a QSO");
        assert_eq!(m.stats().dxcc_worked, 0, "ClubLog credits nothing here");
        assert!(m.status(133).is_none());
        assert!(m.worked_calls.is_empty());

        // The listed operation, same entity, is credited normally.
        let listed = "<CALL:4>ZL8X<QSO_DATE:8>20250712<TIME_ON:6>101500\
                      <BAND:3>40M<MODE:3>FT8<DXCC:3>133<eor>";
        let (m, _) = LogMatrix::build_from_adif(listed, &r);
        assert_eq!(m.stats().dxcc_worked, 1);
        assert!(m.status(133).is_some());
    }

    /// The rejection must be aimed at the entity, not the prefix: an ordinary
    /// New Zealand call sharing the `ZL` root is untouched.
    #[test]
    fn a_whitelisted_entity_does_not_taint_its_neighbours() {
        let entity = |adif: i32, prefix: &str, whitelist: bool| crate::cty::DxccEntity {
            adif,
            name: "E".into(),
            prefix: prefix.into(),
            whitelist,
            ..Default::default()
        };
        let rule = |call: &str, adif: i32| crate::cty::PrefixRule {
            call: call.into(),
            adif,
            is_exact: false,
            start_unix: None,
            end_unix: None,
        };
        let mut r = crate::dxcc::DxccResolver::default();
        r.load(
            crate::cty::CtyData {
                entities: HashMap::from([
                    (133, entity(133, "ZL8", true)),
                    (170, entity(170, "ZL", false)),
                ]),
                prefix_rules: vec![rule("ZL8", 133), rule("ZL", 170)],
                ..Default::default()
            },
            0,
        );
        let adif = "<CALL:5>ZL1ABC<QSO_DATE:8>20250712<TIME_ON:6>101500\
                    <BAND:3>40M<MODE:3>FT8<DXCC:3>170<eor>";
        let (m, _) = LogMatrix::build_from_adif(adif, &r);
        assert_eq!(m.stats().dxcc_worked, 1, "New Zealand still counts");
        assert!(m.status(170).is_some());
    }

    /// The windows are not day-aligned, so the *time* has to be read too —
    /// a boundary-day QSO on either side of the cut-off must land right.
    #[test]
    fn the_window_is_tested_to_the_minute() {
        let ops = vec![invalid_op(
            "T32C",
            "2011-10-01T20:36:00+00:00",
            "2011-10-02T16:29:59+00:00",
        )];
        let build = |time: &str| {
            let adif = format!(
                "<CALL:4>T32C<QSO_DATE:8>20111001<TIME_ON:6>{time}\
                 <BAND:3>20M<MODE:2>CW<DXCC:2>48<eor>"
            );
            LogMatrix::build_from_adif(&adif, &resolver_with(ops.clone()))
                .0
                .stats()
                .dxcc_worked
        };
        assert_eq!(build("203500"), 1, "a minute before the cut-off, valid");
        assert_eq!(build("203600"), 0, "on the cut-off, invalid");
        assert_eq!(build("210000"), 0, "after it, invalid");
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

    /// The band × mode grid, and the reason its columns must not be summed.
    #[test]
    fn grid_splits_every_band_by_mode() {
        let mut m = LogMatrix::default();
        // India on 20M in two modes, two calls — ONE entity on 20M, but a
        // cell in each of two modes.
        m.record(324, "20M", "DATA", "VU2ABC", true);
        m.record(324, "20M", "CW", "VU2XYZ", false);
        // USA on 20M CW (confirmed) and 40M CW (not).
        m.record(291, "20M", "CW", "K1ABC", true);
        m.record(291, "40M", "CW", "K9XYZ", false);

        let s = m.by_band_and_mode();
        let cell = |mode: &str, band: &str| {
            s.grid
                .iter()
                .find(|r| r.mode == mode)
                .unwrap()
                .bands
                .iter()
                .find(|b| b.key == band)
                .unwrap()
                .clone()
        };

        assert_eq!(cell("CW", "20M").worked, 2, "India and the USA, both in CW");
        assert_eq!(cell("CW", "20M").confirmed, 1, "only the US CW slot");
        assert_eq!(cell("DATA", "20M").worked, 1);
        assert_eq!(cell("DATA", "20M").confirmed, 1);
        assert_eq!(
            cell("PHONE", "20M").worked,
            0,
            "an empty cell is still a cell"
        );
        assert_eq!(cell("CW", "40M").worked, 1);
        assert_eq!(cell("CW", "40M").confirmed, 0, "worked but unconfirmed");
        assert_eq!(
            cell("CW", "15M").worked,
            0,
            "and an empty band keeps its row"
        );

        // The mode-agnostic row is NOT the column sum: India is one entity on
        // 20M yet fills two cells there. This is why `bands` is carried
        // alongside the grid rather than computed from it.
        let column: usize = s.grid.iter().map(|r| cell(&r.mode, "20M").worked).sum();
        assert_eq!(column, 3, "the column double-counts India");
        assert_eq!(
            s.bands.iter().find(|b| b.key == "20M").unwrap().worked,
            2,
            "while the entity count on 20M is two"
        );

        // Same orderings as the projections.
        assert_eq!(
            s.grid.iter().map(|r| r.mode.as_str()).collect::<Vec<_>>(),
            vec!["CW", "PHONE", "DATA"]
        );
        assert_eq!(s.grid[0].bands.first().unwrap().key, "160M");
    }

    /// `Stats.svelte` reads these three by name. A rename here would empty
    /// the table on screen without failing a single Rust assertion, so the
    /// wire names are pinned.
    #[test]
    fn band_mode_stats_keeps_its_wire_names() {
        let mut m = LogMatrix::default();
        m.record(324, "20M", "CW", "VU2ABC", true);
        let v = serde_json::to_value(m.by_band_and_mode()).unwrap();

        assert!(v.get("bands").is_some(), "the Mixed row");
        assert!(v.get("modes").is_some(), "the Total column");
        assert!(v.get("grid").is_some(), "the cells");

        let row = &v["grid"][0];
        assert_eq!(row["mode"], "CW");
        assert!(row["bands"].is_array(), "one cell per band, in band order");
        assert_eq!(row["bands"][0]["key"], "160M");
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
