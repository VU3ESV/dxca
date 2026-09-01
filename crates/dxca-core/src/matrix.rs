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

/// Worked/confirmed state for one non-DXCC award key — a VUCC grid square,
/// a WAS state, an IOTA reference. Bands only: mode endorsements are a
/// deliberate deferral (`docs/AWARDS.md` §2.4), and adding sets later is a
/// serde default away, not a migration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AwardStatus {
    pub bands: HashSet<String>,
    #[serde(rename = "confirmedBands")]
    pub confirmed_bands: HashSet<String>,
    // Mode-class axis (CW / PHONE / DATA), added 2.17.8 for WAS mode
    // endorsements and Triple Play. Serde-defaulted, so a matrix stored
    // before it existed still loads — it simply has no modes until the
    // next log refresh rebuilds it.
    #[serde(default)]
    pub modes: HashSet<String>,
    #[serde(default, rename = "confirmedModes")]
    pub confirmed_modes: HashSet<String>,
}

impl AwardStatus {
    fn record(&mut self, band: &str, mode: Option<&str>, confirmed: bool) {
        self.bands.insert(band.to_string());
        if confirmed {
            self.confirmed_bands.insert(band.to_string());
        }
        if let Some(m) = mode {
            self.modes.insert(m.to_string());
            if confirmed {
                self.confirmed_modes.insert(m.to_string());
            }
        }
    }

    /// Confirmed at the key level — on any band. What WAS and IOTA count.
    pub fn is_confirmed(&self) -> bool {
        !self.confirmed_bands.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LogMatrix {
    #[serde(rename = "byDXCC")]
    pub by_dxcc: HashMap<i32, DxccStatus>,
    /// Lowercased calls already worked — fast path for exact-call checks.
    #[serde(rename = "workedCalls")]
    pub worked_calls: HashSet<String>,
    // The three non-DXCC award axes (docs/AWARDS.md phases 2–4). All three
    // default empty under serde, so every matrix_json stored before they
    // existed — and every 1.x matrix.json — still deserializes untouched.
    /// VUCC: 4-char grid square → per-band state, **50 MHz+ bands only**
    /// (`bands::is_vucc_band` gates what `record_grid` accepts).
    #[serde(default, rename = "byGrid")]
    pub by_grid: HashMap<String, AwardStatus>,
    /// WAS: two-letter state → per-band state. Keys are validated through
    /// `awards::normalize_state`, so territories never take a slot.
    #[serde(default, rename = "byState")]
    pub by_state: HashMap<String, AwardStatus>,
    /// IOTA: normalized reference (`AS-003`) → per-band state.
    #[serde(default, rename = "byIota")]
    pub by_iota: HashMap<String, AwardStatus>,
    /// WAZ: CQ zone 1–40 → per-band/mode state. Fed from the ADIF `CQZ`
    /// field, which ClubLog's export does carry — so unlike WAS and IOTA
    /// this axis works without a LoTW report.
    #[serde(default, rename = "byZone")]
    pub by_zone: HashMap<i32, AwardStatus>,
    /// **DX Marathon**: calendar year → the entities and zones worked in
    /// it. The one axis with a time dimension, because the award resets
    /// every January — a score, not a lifetime total.
    #[serde(default, rename = "byYear")]
    pub by_year: HashMap<i32, MarathonYear>,
}

/// One calendar year of DX Marathon: entities and zones, no bands, no
/// modes, and no confirmation — the award scores what you worked.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MarathonYear {
    pub entities: HashSet<i32>,
    pub zones: HashSet<i32>,
}

impl MarathonYear {
    /// The Marathon score: one point per entity plus one per zone.
    pub fn score(&self) -> usize {
        self.entities.len() + self.zones.len()
    }
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
        let (m, n, _) = Self::build_from_adif_reporting(content, resolver);
        (m, n)
    }

    /// [`build_from_adif`](Self::build_from_adif), also returning **which
    /// contacts earned no credit and why**.
    ///
    /// Without this the two rules above are silent: a QSO is simply absent
    /// from the totals, and the only symptom is a number disagreeing with
    /// ClubLog's by one. Working back from that to a single QSO in a
    /// 65,908-record log took a session; the answer is one line of output.
    ///
    /// Reports only contacts dropped by the *ClubLog credit rules*. Records
    /// with no callsign, band or mode, and callsigns that resolve to no
    /// entity at all, are skipped as they always were — those are missing
    /// data, not a ruling.
    pub fn build_from_adif_reporting(
        content: &str,
        resolver: &crate::dxcc::DxccResolver,
    ) -> (LogMatrix, usize, Vec<UncreditedContact>) {
        let records = crate::adif::parse(content);
        let count = records.len();
        let mut matrix = LogMatrix::default();
        let mut uncredited = Vec::new();
        let note = |r: &crate::adif::Record,
                    call: &str,
                    band: &str,
                    mode: &str,
                    dxcc: Option<i32>,
                    reason| UncreditedContact {
            call: call.to_string(),
            qso_date: r.qso_date().unwrap_or_default().to_string(),
            time_on: r.time_on().unwrap_or_default().to_string(),
            band: band.to_string(),
            mode: mode.to_string(),
            dxcc,
            reason,
        };

        for r in &records {
            let (Some(call), Some(band), Some(mode)) = (r.call(), r.band(), r.mode()) else {
                continue;
            };
            // Before resolution, and before `worked_calls`: an invalidated
            // contact is not a worked call either, so the same station
            // spotted again should still alert as new.
            let at = r.qso_datetime_unix();
            if resolver.is_invalid_operation(&call, at) {
                let d = r.dxcc().or_else(|| resolver.resolve(&call));
                uncredited.push(note(
                    r,
                    &call,
                    &band,
                    &mode,
                    d,
                    UncreditedReason::InvalidOperation,
                ));
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
                uncredited.push(note(
                    r,
                    &call,
                    &band,
                    &mode,
                    Some(d),
                    UncreditedReason::NotWhitelisted,
                ));
                continue;
            }
            let confirmed = r.is_confirmed();
            matrix.record(d, &band, crate::modes::canonical(&mode), &call, confirmed);
            // The award axes ride the same credit-gated loop on purpose: a
            // contact ClubLog would not credit for DXCC (invalid operation,
            // whitelist reject) earns no grid, state or island either.
            // ClubLog's export carries GRIDSQUARE (verified 2026-09-01,
            // 98% of records) but never STATE or IOTA — those two record
            // nothing here and are fed by `merge_lotw_confirmed`.
            let mode_class = crate::modes::canonical(&mode);
            if let Some(g) = r.grid_square() {
                matrix.record_grid(&g, &band, Some(mode_class), confirmed);
            }
            // The DXCC gate is the LOG-side twin of the one `best_award`
            // applies to spots: LoTW's STATE is a subdivision code for the
            // whole world, and several collide with US postal codes —
            // China's Shandong is `SD`, Brazil's Santa Catarina `SC`,
            // Russia's Moscow oblast `MO`. Without this a Shandong QSL
            // credited South Dakota, which is exactly what it did (VU2CPL,
            // 2026-09-01: 49 states, one of them from China).
            if let Some(s) = r.state()
                && r.dxcc().is_some_and(crate::awards::counts_for_was)
            {
                matrix.record_state(&s, &band, Some(mode_class), confirmed);
            }
            if let Some(i) = r.iota() {
                matrix.record_iota(&i, &band, Some(mode_class), confirmed);
            }
            // WAZ and the Marathon both ride the ADIF `CQZ`, falling back
            // to what the resolver can say about the call. The Marathon
            // also needs the YEAR, which is the only axis in this matrix
            // that has ever cared what date a contact was made.
            let zone = r.cqz().or_else(|| resolver.zone(&call));
            if let Some(z) = zone {
                matrix.record_zone(z, &band, Some(mode_class), confirmed);
            }
            if let Some(year) = r.qso_year() {
                matrix.record_marathon(year, Some(d), zone);
            }
        }
        (matrix, count, uncredited)
    }

    /// Merge a **LoTW QSL report** (`lotwreport.adi`, confirmations only)
    /// into the award axes — the source for what ClubLog's export cannot
    /// carry: `STATE`, `IOTA`, and grid confirmations from the other
    /// station's own TQSL location. Strictly additive, and deliberately
    /// blind to `by_dxcc`: DXCC stays ClubLog's. Every record in a
    /// `qso_qsl=yes` report is a confirmation by definition.
    ///
    /// Returns how many records landed at least one award fact.
    pub fn merge_lotw_confirmed(&mut self, content: &str) -> usize {
        let mut merged = 0;
        for r in crate::adif::parse(content) {
            let Some(band) = r.band() else { continue };
            // LoTW reports the mode as `MODE`, with `APP_LoTW_MODEGROUP`
            // carrying its own CW/PHONE/DATA bucketing; ours agrees, so the
            // standard field is enough and we keep one bucketing rule.
            let mode = r.mode();
            let mode_class = mode.as_deref().and_then(crate::modes::canonical_opt);
            let mut any = false;
            if let Some(g) = r.grid_square() {
                any |= self.record_grid(&g, &band, mode_class, true);
            }
            // See the note in `build_from_adif_reporting`: STATE is a
            // worldwide subdivision code and several collide with US ones.
            if let Some(s) = r.state()
                && r.dxcc().is_some_and(crate::awards::counts_for_was)
            {
                any |= self.record_state(&s, &band, mode_class, true);
            }
            if let Some(i) = r.iota() {
                any |= self.record_iota(&i, &band, mode_class, true);
            }
            merged += usize::from(any);
        }
        merged
    }

    /// Record a grid credit — VUCC bands only, 4-char square. Returns
    /// whether anything was recorded (the input was a real locator on a
    /// scoring band).
    pub fn record_grid(
        &mut self,
        locator: &str,
        band: &str,
        mode: Option<&str>,
        confirmed: bool,
    ) -> bool {
        let Some(g4) = crate::grid::grid4(locator) else {
            return false;
        };
        if !crate::bands::is_vucc_band(band) {
            return false;
        }
        self.by_grid
            .entry(g4)
            .or_default()
            .record(band, mode, confirmed);
        true
    }

    /// Record a WAS credit; territories and noise fall out in
    /// `normalize_state` (DC folds into MD there too).
    pub fn record_state(
        &mut self,
        raw: &str,
        band: &str,
        mode: Option<&str>,
        confirmed: bool,
    ) -> bool {
        let Some(st) = crate::awards::normalize_state(raw) else {
            return false;
        };
        self.by_state
            .entry(st.to_string())
            .or_default()
            .record(band, mode, confirmed);
        true
    }

    /// Record a CQ-zone credit (WAZ). Zones outside 1–40 are refused.
    pub fn record_zone(
        &mut self,
        zone: i32,
        band: &str,
        mode: Option<&str>,
        confirmed: bool,
    ) -> bool {
        if !(1..=40).contains(&zone) {
            return false;
        }
        self.by_zone
            .entry(zone)
            .or_default()
            .record(band, mode, confirmed);
        true
    }

    /// Record a DX Marathon credit for the year a contact was made.
    pub fn record_marathon(&mut self, year: i32, dxcc: Option<i32>, zone: Option<i32>) {
        if dxcc.is_none() && zone.is_none() {
            return;
        }
        let y = self.by_year.entry(year).or_default();
        if let Some(d) = dxcc {
            y.entities.insert(d);
        }
        if let Some(z) = zone.filter(|z| (1..=40).contains(z)) {
            y.zones.insert(z);
        }
    }

    /// Record an IOTA credit under the normalized reference.
    pub fn record_iota(
        &mut self,
        raw: &str,
        band: &str,
        mode: Option<&str>,
        confirmed: bool,
    ) -> bool {
        let Some(r) = crate::awards::normalize_iota(raw) else {
            return false;
        };
        self.by_iota
            .entry(r)
            .or_default()
            .record(band, mode, confirmed);
        true
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

    /// Is this call already in the log? Same slash handling as
    /// `lotw::is_user` — exact, bare-before-slash, and after-slash (prefix
    /// overrides like VP8/K1JT) — against the lowercased calls [`record`]
    /// stores. The first real consumer of `worked_calls`: 1.x carried the
    /// field but never read it.
    ///
    /// [`record`]: Self::record
    pub fn has_worked_call(&self, callsign: &str) -> bool {
        let lower = callsign.to_lowercase();
        if self.worked_calls.contains(&lower) {
            return true;
        }
        match lower.split_once('/') {
            Some((bare, suffix)) => {
                self.worked_calls.contains(bare) || self.worked_calls.contains(suffix)
            }
            None => false,
        }
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

    /// Totals for the three non-DXCC awards (`docs/AWARDS.md` phases 2–4).
    /// The ARRL deleted-entities toggle does not apply here — a grid, a
    /// state or an island cannot be deleted out from under a QSO — so there
    /// is no `_excluding` twin.
    pub fn award_stats(&self) -> AwardStats {
        // VUCC scores per band; a band nothing was worked on is left out
        // rather than sent as a zero row (most stations have no 33cm).
        let vucc = crate::bands::VUCC_BANDS
            .iter()
            .map(|b| VuccBandCount {
                band: (*b).to_string(),
                worked: self
                    .by_grid
                    .values()
                    .filter(|s| s.bands.contains(*b))
                    .count(),
                confirmed: self
                    .by_grid
                    .values()
                    .filter(|s| s.confirmed_bands.contains(*b))
                    .count(),
            })
            .filter(|c| c.worked > 0)
            .collect();
        // The missing list is the useful shape for WAS — fifty minus worked
        // is short for anyone chasing it, and "which ones" is the question.
        let mut was_missing: Vec<String> = crate::awards::US_STATES
            .iter()
            .filter(|s| !self.by_state.contains_key(**s))
            .map(|s| (*s).to_string())
            .collect();
        was_missing.sort();
        // WAS endorsements: the same fifty states counted per band and per
        // mode class. Rows with nothing on them are dropped — a WAS chaser
        // wants the bands they are actually working, not fifteen zeroes.
        let was_by_band = crate::bands::SELECTABLE_BANDS
            .iter()
            .map(|b| AwardBreakdown {
                key: (*b).to_string(),
                worked: self
                    .by_state
                    .values()
                    .filter(|s| s.bands.contains(*b))
                    .count(),
                confirmed: self
                    .by_state
                    .values()
                    .filter(|s| s.confirmed_bands.contains(*b))
                    .count(),
            })
            .filter(|r| r.worked > 0)
            .collect();
        let was_by_mode = crate::modes::CLASSES
            .iter()
            .map(|m| AwardBreakdown {
                key: (*m).to_string(),
                worked: self
                    .by_state
                    .values()
                    .filter(|s| s.modes.contains(*m))
                    .count(),
                confirmed: self
                    .by_state
                    .values()
                    .filter(|s| s.confirmed_modes.contains(*m))
                    .count(),
            })
            .collect();
        // WAZ: forty zones, and which are missing — the same shape WAS
        // gets, because it is the same kind of chase.
        // Missing means **not confirmed**: an award is claimed on
        // confirmations, so a worked-but-unconfirmed zone is still wanted.
        let confirmed_zone = |z: &i32| self.by_zone.get(z).is_some_and(|s| s.is_confirmed());
        let mut waz_missing: Vec<i32> = (1..=40).filter(|z| !confirmed_zone(z)).collect();
        waz_missing.sort_unstable();
        // Per mode class, and which zones each still wants — the WAZ twin
        // of the Triple Play worklist.
        let waz_by_mode: Vec<AwardBreakdown> = crate::modes::CLASSES
            .iter()
            .map(|m| AwardBreakdown {
                key: (*m).to_string(),
                worked: self
                    .by_zone
                    .values()
                    .filter(|s| s.modes.contains(*m))
                    .count(),
                confirmed: self
                    .by_zone
                    .values()
                    .filter(|s| s.confirmed_modes.contains(*m))
                    .count(),
            })
            .collect();
        let waz_needed_by_mode: Vec<ZoneNeed> = crate::modes::CLASSES
            .iter()
            .map(|m| ZoneNeed {
                mode: (*m).to_string(),
                zones: (1..=40)
                    .filter(|z| {
                        !self
                            .by_zone
                            .get(z)
                            .is_some_and(|s| s.confirmed_modes.contains(*m))
                    })
                    .collect(),
            })
            .filter(|n| !n.zones.is_empty())
            .collect();
        let waz_by_band = crate::bands::SELECTABLE_BANDS
            .iter()
            .map(|b| AwardBreakdown {
                key: (*b).to_string(),
                worked: self
                    .by_zone
                    .values()
                    .filter(|s| s.bands.contains(*b))
                    .count(),
                confirmed: self
                    .by_zone
                    .values()
                    .filter(|s| s.confirmed_bands.contains(*b))
                    .count(),
            })
            .filter(|r| r.worked > 0)
            .collect();
        AwardStats {
            vucc,
            waz_worked: self.by_zone.len(),
            waz_confirmed: self.by_zone.values().filter(|s| s.is_confirmed()).count(),
            waz_missing,
            waz_by_mode,
            waz_needed_by_mode,
            waz_by_band,
            marathon: self.marathon_years(),
            was_worked: self.by_state.len(),
            was_confirmed: self.by_state.values().filter(|s| s.is_confirmed()).count(),
            was_missing,
            was_by_band,
            was_by_mode,
            triple_play: self.triple_play_count(),
            triple_play_missing: self.triple_play_missing(),
            iota_worked: self.by_iota.len(),
            iota_confirmed: self.by_iota.values().filter(|s| s.is_confirmed()).count(),
        }
    }

    /// Every year the log has Marathon points in, newest first. The award
    /// runs on the calendar year, so the current one is the live score and
    /// the rest are history worth keeping in view.
    pub fn marathon_years(&self) -> Vec<MarathonScore> {
        let mut out: Vec<MarathonScore> = self
            .by_year
            .iter()
            .map(|(y, m)| MarathonScore {
                year: *y,
                entities: m.entities.len(),
                zones: m.zones.len(),
                score: m.score(),
            })
            .collect();
        out.sort_by_key(|m| std::cmp::Reverse(m.year));
        out
    }

    /// **ARRL Triple Play**: all fifty states confirmed in each of CW,
    /// Phone and Digital — 150 confirmations, any band.
    ///
    /// The award requires the confirmations be **through LoTW**, which this
    /// satisfies by construction rather than by checking: the only thing
    /// that ever writes a confirmed state is `merge_lotw_confirmed`, since
    /// ClubLog's export carries no `STATE` at all.
    pub fn triple_play_count(&self) -> usize {
        self.by_state
            .values()
            .filter(|s| {
                crate::modes::CLASSES
                    .iter()
                    .all(|m| s.confirmed_modes.contains(*m))
            })
            .count()
    }

    /// What Triple Play still needs, as `(state, modes still missing)` —
    /// the actual worklist, which a bare "39 of 50" cannot give.
    pub fn triple_play_missing(&self) -> Vec<TriplePlayGap> {
        let mut gaps: Vec<TriplePlayGap> = crate::awards::US_STATES
            .iter()
            .map(|st| {
                let have = self.by_state.get(*st);
                TriplePlayGap {
                    state: (*st).to_string(),
                    needed: crate::modes::CLASSES
                        .iter()
                        .filter(|m| !have.is_some_and(|s| s.confirmed_modes.contains(**m)))
                        .map(|m| (*m).to_string())
                        .collect(),
                }
            })
            .filter(|g| !g.needed.is_empty())
            .collect();
        gaps.sort_by(|a, b| {
            a.needed
                .len()
                .cmp(&b.needed.len())
                .then(a.state.cmp(&b.state))
        });
        gaps
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

/// Why ClubLog gives a contact no DXCC credit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UncreditedReason {
    /// On ClubLog's [invalid-operations](crate::cty::InvalidOperation) list
    /// for the QSO's date — a pirate, an unlicensed operation, or a
    /// DXpedition whose paperwork was rejected.
    InvalidOperation,
    /// A [whitelisted](crate::cty::DxccEntity::whitelist) entity, and this
    /// callsign is not one of the operations ClubLog accepts for it.
    NotWhitelisted,
}

impl UncreditedReason {
    pub fn as_str(self) -> &'static str {
        match self {
            UncreditedReason::InvalidOperation => "invalid operation",
            UncreditedReason::NotWhitelisted => "not on the entity's whitelist",
        }
    }
}

/// One contact that is in the log and scores nothing — the detail behind a
/// total that disagrees with ClubLog's.
///
/// Fields are the raw ADIF values, so the line printed can be searched for
/// verbatim in the operator's own log or in the ADIF they downloaded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UncreditedContact {
    pub call: String,
    /// `YYYYMMDD` as ADIF wrote it; empty when the record carried no date.
    pub qso_date: String,
    /// `HHMMSS` or `HHMM` as ADIF wrote it; empty when absent.
    pub time_on: String,
    pub band: String,
    pub mode: String,
    /// The entity it *would* have counted for. `None` only when an invalid
    /// operation could not be resolved to one at all.
    pub dxcc: Option<i32>,
    pub reason: UncreditedReason,
}

impl std::fmt::Display for UncreditedContact {
    /// One greppable line: call, when, where, and the ruling.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.call)?;
        if !self.qso_date.is_empty() {
            write!(f, " {}", self.qso_date)?;
            if !self.time_on.is_empty() {
                write!(f, " {}Z", self.time_on)?;
            }
        } else {
            write!(f, " (no date)")?;
        }
        write!(f, " {} {}", self.band, self.mode)?;
        if let Some(d) = self.dxcc {
            write!(f, " DXCC {d}")?;
        }
        write!(f, " — {}, no credit", self.reason.as_str())
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

/// A per-band or per-mode count of states — a WAS endorsement row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AwardBreakdown {
    pub key: String,
    pub worked: usize,
    pub confirmed: usize,
}

/// The zones one mode class still wants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneNeed {
    pub mode: String,
    pub zones: Vec<i32>,
}

/// One state that Triple Play still wants, and in which modes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriplePlayGap {
    pub state: String,
    pub needed: Vec<String>,
}

/// One VUCC band's grid counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VuccBandCount {
    pub band: String,
    pub worked: usize,
    pub confirmed: usize,
}

/// Totals for the non-DXCC awards, shaped for the Stats screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AwardStats {
    /// Per scoring band, bands with nothing worked omitted.
    pub vucc: Vec<VuccBandCount>,
    pub was_worked: usize,
    pub was_confirmed: usize,
    /// States never worked, alphabetical — the chase list.
    pub was_missing: Vec<String>,
    /// WAS counted per band, and per mode class — the endorsements.
    pub was_by_band: Vec<AwardBreakdown>,
    pub was_by_mode: Vec<AwardBreakdown>,
    /// States confirmed in all three modes (ARRL Triple Play), and the
    /// worklist for the rest.
    pub triple_play: usize,
    pub triple_play_missing: Vec<TriplePlayGap>,
    pub iota_worked: usize,
    pub iota_confirmed: usize,
    /// WAZ: forty CQ zones, the missing list, and the per-band breakdown.
    pub waz_worked: usize,
    pub waz_confirmed: usize,
    pub waz_missing: Vec<i32>,
    pub waz_by_mode: Vec<AwardBreakdown>,
    /// Which zones each mode class still wants, confirmed-wise.
    pub waz_needed_by_mode: Vec<ZoneNeed>,
    pub waz_by_band: Vec<AwardBreakdown>,
    /// DX Marathon, newest year first.
    pub marathon: Vec<MarathonScore>,
}

/// One year's DX Marathon score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarathonScore {
    pub year: i32,
    pub entities: usize,
    pub zones: usize,
    pub score: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn award_axes_record_and_count() {
        let mut m = LogMatrix::default();
        // VUCC: 6M scores, 20M never does, RR73 is not a grid.
        assert!(m.record_grid("MK83va", "6M", Some("DATA"), false));
        assert!(
            m.record_grid("MK83", "6M", Some("DATA"), true),
            "same square, now confirmed"
        );
        assert!(m.record_grid("FN20", "2M", Some("DATA"), false));
        assert!(
            !m.record_grid("MK83", "20M", Some("DATA"), false),
            "HF never scores VUCC"
        );
        assert!(!m.record_grid("RR73", "6M", Some("DATA"), false));
        // WAS: DC folds to MD, PR is not a state.
        assert!(m.record_state("OH", "20M", Some("DATA"), true));
        assert!(m.record_state("dc", "40M", Some("DATA"), false));
        assert!(!m.record_state("PR", "20M", Some("DATA"), true));
        // IOTA normalizes.
        assert!(m.record_iota("as-3", "15M", Some("DATA"), false));
        assert!(!m.record_iota("XX-1", "15M", Some("DATA"), false));

        let a = m.award_stats();
        assert_eq!(
            a.vucc,
            vec![
                VuccBandCount {
                    band: "6M".into(),
                    worked: 1,
                    confirmed: 1
                },
                VuccBandCount {
                    band: "2M".into(),
                    worked: 1,
                    confirmed: 0
                },
            ],
            "bands in VUCC order, empty bands omitted"
        );
        assert_eq!((a.was_worked, a.was_confirmed), (2, 1));
        assert_eq!(a.was_missing.len(), 48);
        assert!(!a.was_missing.contains(&"OH".to_string()));
        assert!(
            !a.was_missing.contains(&"MD".to_string()),
            "DC counted as MD"
        );
        assert_eq!((a.iota_worked, a.iota_confirmed), (1, 0));
        assert!(m.by_iota.contains_key("AS-003"));
    }

    #[test]
    fn lotw_merge_is_additive_and_confirmed() {
        let mut m = LogMatrix::default();
        m.record_state("OH", "20M", Some("DATA"), false); // worked via ClubLog, unconfirmed
        let report = "<CALL:4>W8AA<BAND:3>20M<MODE:2>CW<STATE:2>OH<DXCC:3>291<eor>\
                      <CALL:4>K6BB<BAND:2>6M<MODE:3>FT8<GRIDSQUARE:6>DM04QD<DXCC:3>291<eor>\
                      <CALL:5>VK9CC<BAND:3>15M<MODE:3>SSB<IOTA:6>OC-003<eor>\
                      <CALL:4>BY1X<BAND:3>20M<MODE:2>CW<STATE:2>SD<DXCC:3>318<eor>\
                      <CALL:4>NOAW<BAND:3>40M<eor>"; // no award fact
        assert_eq!(m.merge_lotw_confirmed(report), 3);
        assert!(m.by_state["OH"].is_confirmed(), "LoTW confirmed the state");
        assert!(m.by_grid["DM04"].confirmed_bands.contains("6M"));
        assert!(m.by_iota["OC-003"].is_confirmed());
        assert!(m.by_dxcc.is_empty(), "DXCC stays ClubLog's");
        assert!(m.worked_calls.is_empty(), "worked calls stay ClubLog's");

        // The collision that reached production: LoTW's STATE is a
        // worldwide subdivision code, and China's Shandong is `SD`. Only
        // the DXCC entity can tell it from South Dakota, and one such QSL
        // put a 49th state in a real log.
        assert!(
            !m.by_state.contains_key("SD"),
            "Shandong is not South Dakota"
        );

        // The mode rides along, which is what WAS endorsements and Triple
        // Play are counted from.
        assert!(m.by_state["OH"].confirmed_modes.contains("CW"));
        assert!(m.by_iota["OC-003"].confirmed_modes.contains("PHONE"));
    }

    #[test]
    fn adif_build_records_award_axes_from_clublog_fields() {
        // A GRIDSQUARE on a VUCC band lands in by_grid straight from the
        // ClubLog export; the same field on HF records nothing for VUCC.
        let adif = "<CALL:5>SV2AB<BAND:2>6M<MODE:3>FT8<DXCC:3>236\
                    <GRIDSQUARE:6>KN10LO<QSL_RCVD:1>Y<eor>\
                    <CALL:5>SV2CD<BAND:3>20M<MODE:3>FT8<DXCC:3>236\
                    <GRIDSQUARE:4>KN20<STATE:2>OH<IOTA:6>EU-001<eor>";
        let (m, count) = LogMatrix::build_from_adif(adif, &resolver_with(vec![]));
        assert_eq!(count, 2);
        assert!(m.by_grid["KN10"].confirmed_bands.contains("6M"));
        assert!(!m.by_grid.contains_key("KN20"), "HF grid scores nothing");
        // STATE/IOTA never actually appear in ClubLog exports, but the build
        // path must take them when present — an uploaded ADIF might. The
        // state here is attached to a GREEK contact, so it must be refused:
        // a subdivision code only means a US state on a US entity.
        assert!(!m.by_state.contains_key("OH"), "Greece cannot carry Ohio");
        assert!(m.by_iota.contains_key("EU-001"));

        // The same field on a US contact is taken.
        let us = "<CALL:4>W8AA<BAND:3>20M<MODE:2>CW<DXCC:3>291<STATE:2>OH<eor>";
        let (m2, _) = LogMatrix::build_from_adif(us, &resolver_with(vec![]));
        assert!(m2.by_state.contains_key("OH"));
    }

    #[test]
    fn worked_call_lookup_handles_slashes() {
        let mut m = LogMatrix::default();
        m.record(324, "20M", "DATA", "VU2ABC", false);
        assert!(m.has_worked_call("vu2abc"), "case-insensitive exact");
        assert!(m.has_worked_call("VU2ABC/P"), "suffix stripped");
        assert!(m.has_worked_call("VP8/VU2ABC"), "prefix override stripped");
        assert!(!m.has_worked_call("VU2XYZ"));
        assert!(!LogMatrix::default().has_worked_call("VU2ABC"), "empty log");
    }

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
                    // Needed by the WAS tests: a state only counts on a
                    // WAS-countable entity, so one has to exist here.
                    (291, entity(291, "UNITED STATES", "K")),
                ]),
                prefix_rules: vec![crate::cty::PrefixRule {
                    call: "SV".into(),
                    adif: 236,
                    is_exact: false,
                    start_unix: None,
                    end_unix: None,
                    cq_zone: None,
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
            cq_zone: None,
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
        let (m, count, uncredited) = LogMatrix::build_from_adif_reporting(adif, &r);
        assert_eq!(count, 1, "still a QSO");
        assert_eq!(m.stats().dxcc_worked, 0, "ClubLog credits nothing here");
        assert!(m.status(133).is_none());
        assert!(m.worked_calls.is_empty());

        // The line the operator actually gets, with the date that makes the
        // QSO findable in a 65,908-record log.
        assert_eq!(uncredited.len(), 1);
        assert_eq!(
            uncredited[0].to_string(),
            "ZL8AC 20250712 101500Z 40M FT8 DXCC 133 \
             — not on the entity's whitelist, no credit"
        );

        // The listed operation, same entity, is credited normally.
        let listed = "<CALL:4>ZL8X<QSO_DATE:8>20250712<TIME_ON:6>101500\
                      <BAND:3>40M<MODE:3>FT8<DXCC:3>133<eor>";
        let (m, _) = LogMatrix::build_from_adif(listed, &r);
        assert_eq!(m.stats().dxcc_worked, 1);
        assert!(m.status(133).is_some());
    }

    /// The report is the whole point of the feature: a dropped QSO is
    /// otherwise invisible, and the date is what makes it findable in a log.
    #[test]
    fn uncredited_contacts_are_reported_with_enough_to_find_them() {
        let ops = vec![invalid_op(
            "SV2RSG/A",
            "2024-09-05T00:00:00+00:00",
            "2024-09-09T23:59:59+00:00",
        )];
        let adif = "<CALL:8>SV2RSG/A<QSO_DATE:8>20240906<TIME_ON:6>101500\
                    <BAND:3>20M<MODE:3>FT8<DXCC:3>180<eor>\
                    <CALL:6>SV2CPL<QSO_DATE:8>20240906<TIME_ON:6>101500\
                    <BAND:3>20M<MODE:2>CW<DXCC:3>236<eor>";
        let (_, count, uncredited) =
            LogMatrix::build_from_adif_reporting(adif, &resolver_with(ops));

        assert_eq!(count, 2, "both records are still QSOs");
        assert_eq!(uncredited.len(), 1, "only the rejected one is reported");
        let c = &uncredited[0];
        assert_eq!(c.call, "SV2RSG/A");
        assert_eq!(c.qso_date, "20240906");
        assert_eq!(c.time_on, "101500");
        assert_eq!(c.band, "20M");
        assert_eq!(c.mode, "FT8", "the log's own mode, not the DATA bucket");
        assert_eq!(c.dxcc, Some(180));
        assert_eq!(c.reason, UncreditedReason::InvalidOperation);

        // The printed line has to carry the date, or it cannot be looked up.
        let line = c.to_string();
        assert!(line.contains("SV2RSG/A"), "{line}");
        assert!(line.contains("20240906"), "{line}");
        assert!(line.contains("invalid operation"), "{line}");
    }

    /// A clean log must produce an empty report — otherwise the line becomes
    /// noise every refresh and stops being read.
    #[test]
    fn a_log_with_nothing_uncredited_reports_nothing() {
        let adif = "<CALL:6>SV2CPL<QSO_DATE:8>20240906<TIME_ON:6>101500\
                    <BAND:3>20M<MODE:2>CW<DXCC:3>236<eor>";
        let (_, _, uncredited) = LogMatrix::build_from_adif_reporting(adif, &resolver_with(vec![]));
        assert!(uncredited.is_empty());
    }

    /// `build_from_adif` must stay exactly the reporting build minus the
    /// report — the two would drift apart silently otherwise.
    #[test]
    fn the_reporting_build_matches_the_plain_one() {
        let ops = vec![invalid_op(
            "SV2RSG/A",
            "2024-09-05T00:00:00+00:00",
            "2024-09-09T23:59:59+00:00",
        )];
        let adif = "<CALL:8>SV2RSG/A<QSO_DATE:8>20240906<TIME_ON:6>101500\
                    <BAND:3>20M<MODE:3>FT8<DXCC:3>180<eor>\
                    <CALL:6>SV2CPL<QSO_DATE:8>20240906<TIME_ON:6>101500\
                    <BAND:3>20M<MODE:2>CW<DXCC:3>236<eor>";
        let plain = LogMatrix::build_from_adif(adif, &resolver_with(ops.clone()));
        let (m, n, _) = LogMatrix::build_from_adif_reporting(adif, &resolver_with(ops));
        assert_eq!(plain.0, m);
        assert_eq!(plain.1, n);
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
            cq_zone: None,
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
