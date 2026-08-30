//! Spot classification against a user's log matrix — port of the Swift
//! `AlertClassifier`. Decides New DXCC / New Band / New Mode / New Slot /
//! Worked for one spot and one user; in DXCA 2.0 this runs once per user
//! over the shared spot stream (plan §5).

use crate::beacons;
use crate::dxcc::DxccResolver;
use crate::matrix::LogMatrix;
use crate::{bands, modes};
use serde::{Deserialize, Serialize};

/// Serde names for the first six match the Swift `AlertLevel` raw values, so
/// 1.x-era JSON and the 2.0 API still speak the same strings.
///
/// The `Unconf*` half is DXCA 2.1 and has no 1.x counterpart. It answers a
/// different question from the `New*` half: those mean *never worked*, these
/// mean *worked and still not confirmed* — the QSL/LoTW gap you close by
/// working the same thing again. They coexist deliberately; the old
/// `alert_unconfirmed` switch could only show one or the other, because it
/// swapped the entire comparison over to the confirmed sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertLevel {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "worked")]
    Worked,
    #[serde(rename = "newMode")]
    NewMode,
    #[serde(rename = "newBand")]
    NewBand,
    #[serde(rename = "newSlot")]
    NewSlot,
    #[serde(rename = "newDXCC")]
    NewDxcc,
    #[serde(rename = "unconfMode")]
    UnconfMode,
    #[serde(rename = "unconfBand")]
    UnconfBand,
    #[serde(rename = "unconfSlot")]
    UnconfSlot,
    #[serde(rename = "unconfDXCC")]
    UnconfDxcc,
}

impl AlertLevel {
    /// Every level a spot can be flagged as, rarest first — the one order
    /// the UI, the Telegram labels and the config screens all read from, so
    /// a level cannot rank differently in two places.
    pub const FLAGGABLE: [AlertLevel; 8] = [
        AlertLevel::NewDxcc,
        AlertLevel::NewBand,
        AlertLevel::NewMode,
        AlertLevel::NewSlot,
        AlertLevel::UnconfDxcc,
        AlertLevel::UnconfBand,
        AlertLevel::UnconfMode,
        AlertLevel::UnconfSlot,
    ];

    /// Stable machine key — the same string serde emits.
    pub fn key(self) -> &'static str {
        match self {
            AlertLevel::None => "none",
            AlertLevel::Worked => "worked",
            AlertLevel::NewMode => "newMode",
            AlertLevel::NewBand => "newBand",
            AlertLevel::NewSlot => "newSlot",
            AlertLevel::NewDxcc => "newDXCC",
            AlertLevel::UnconfMode => "unconfMode",
            AlertLevel::UnconfBand => "unconfBand",
            AlertLevel::UnconfSlot => "unconfSlot",
            AlertLevel::UnconfDxcc => "unconfDXCC",
        }
    }

    /// Short human label. `?` is the logger convention for worked-but-unconfirmed.
    pub fn label(self) -> &'static str {
        match self {
            AlertLevel::None => "",
            AlertLevel::Worked => "",
            AlertLevel::NewMode => "New Mode",
            AlertLevel::NewBand => "New Band",
            AlertLevel::NewSlot => "New Slot",
            AlertLevel::NewDxcc => "NEW DXCC",
            AlertLevel::UnconfMode => "? Mode",
            AlertLevel::UnconfBand => "? Band",
            AlertLevel::UnconfSlot => "? Slot",
            AlertLevel::UnconfDxcc => "? DXCC",
        }
    }
}

/// The per-user alert toggles the classifier consults. A level switched off
/// here is never flagged at all — it collapses to `Worked`, exactly as the
/// four 1.x toggles always did.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    pub alert_new_dxcc: bool,
    pub alert_new_slot: bool,
    pub alert_new_band: bool,
    pub alert_new_mode: bool,
    // DXCA 2.1: the confirmation-hunting half. Off by default — a fresh
    // install behaves exactly like 1.x until the operator asks for these.
    pub alert_unconf_dxcc: bool,
    pub alert_unconf_slot: bool,
    pub alert_unconf_band: bool,
    pub alert_unconf_mode: bool,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            alert_new_dxcc: true,
            alert_new_slot: true,
            alert_new_band: true,
            alert_new_mode: true,
            alert_unconf_dxcc: false,
            alert_unconf_slot: false,
            alert_unconf_band: false,
            alert_unconf_mode: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Classification {
    pub level: AlertLevel,
    pub dxcc_id: Option<i32>,
    pub dxcc_name: Option<String>,
    pub band: Option<&'static str>,
    pub is_beacon: bool,
}

impl Classification {
    fn none() -> Self {
        Classification {
            level: AlertLevel::None,
            dxcc_id: None,
            dxcc_name: None,
            band: None,
            is_beacon: false,
        }
    }
}

pub struct AlertClassifier<'a> {
    pub matrix: &'a LogMatrix,
    pub resolver: &'a DxccResolver,
    pub config: &'a AlertConfig,
}

impl AlertClassifier<'_> {
    /// Classify a spot. `None`-level when data is missing (no matrix entry
    /// needed — that's New DXCC — but no resolver, band, or DXCC id).
    pub fn classify(&self, callsign: &str, frequency_mhz: f64, mode: &str) -> Classification {
        if callsign.is_empty() || !self.resolver.is_loaded() {
            return Classification::none();
        }

        let band = bands::band_from_mhz(frequency_mhz);

        // Beacons / satellites / gateways (ClubLog adif=0, or our own
        // beacon database): label them, never alert.
        let known_beacon = beacons::display_name(callsign);
        if self.resolver.is_non_dx_operation(callsign) || known_beacon.is_some() {
            return Classification {
                level: AlertLevel::None,
                dxcc_id: None,
                dxcc_name: Some(known_beacon.unwrap_or_else(|| "Beacon".to_string())),
                band,
                is_beacon: true,
            };
        }

        let dxcc_id = self.resolver.resolve(callsign);
        let dxcc_name = dxcc_id
            .and_then(|id| self.resolver.entity(id))
            .map(|e| e.name.clone());
        // FT8/FT4/JT*/RTTY/… → DATA: digital modes share one award slot.
        // `None` when the mode is genuinely unknown — `canonical` would call
        // that DATA, which is how phone spots from comment-only cluster
        // nodes used to be credited to digital slots.
        let normalized_mode = modes::canonical_opt(mode);

        let (Some(dxcc), Some(bnd)) = (dxcc_id, band) else {
            return Classification {
                level: AlertLevel::None,
                dxcc_id,
                dxcc_name,
                band,
                is_beacon: false,
            };
        };

        let raw = self.raw_level(dxcc, bnd, normalized_mode);
        Classification {
            level: self.apply_filter(raw),
            dxcc_id: Some(dxcc),
            dxcc_name,
            band: Some(bnd),
            is_beacon: false,
        }
    }

    /// The ladder, rarest gap first. Two passes over the same entity: the
    /// WORKED sets decide the `New*` half, and only once a spot survives all
    /// of those (i.e. the slot really is in the log) do the CONFIRMED sets
    /// decide the `Unconf*` half. That ordering is the whole point — a band
    /// you have never worked is a better catch than one you have worked and
    /// not confirmed, so it must win even though both are "missing".
    fn raw_level(&self, dxcc: i32, band: &str, mode: Option<&str>) -> AlertLevel {
        let Some(status) = self.matrix.status(dxcc) else {
            return AlertLevel::NewDxcc;
        };

        // No mode means no slot and no mode gap that can honestly be
        // answered, so only the band half of the ladder runs. Inventing a
        // slot would put the spot in an award bucket it may not belong to;
        // returning nothing would hide a genuinely new band. Band gaps are
        // mode-independent, so they stay.
        let Some(mode) = mode else {
            if !status.bands.contains(band) {
                return AlertLevel::NewBand;
            }
            if status.confirmed_slots.is_empty() {
                return AlertLevel::UnconfDxcc;
            }
            if !status.confirmed_bands.contains(band) {
                return AlertLevel::UnconfBand;
            }
            return AlertLevel::Worked;
        };

        let slot = format!("{band}-{mode}");

        // --- never worked -------------------------------------------------
        // Priority: never-worked band > never-worked mode > new combination
        // of both (the genuine 5BDXCC-style slot).
        if !status.slots.contains(&slot) {
            if !status.bands.contains(band) {
                return AlertLevel::NewBand;
            }
            if !status.modes.contains(mode) {
                return AlertLevel::NewMode;
            }
            return AlertLevel::NewSlot;
        }

        // --- worked, but is it confirmed? ---------------------------------
        // Same shape one level down. The entity-wide check comes first: with
        // nothing at all confirmed the band/mode/slot gaps are all true too,
        // and "this entity is entirely unconfirmed" is the bigger fact.
        if status.confirmed_slots.is_empty() {
            return AlertLevel::UnconfDxcc;
        }
        if !status.confirmed_slots.contains(&slot) {
            if !status.confirmed_bands.contains(band) {
                return AlertLevel::UnconfBand;
            }
            if !status.confirmed_modes.contains(mode) {
                return AlertLevel::UnconfMode;
            }
            return AlertLevel::UnconfSlot;
        }
        AlertLevel::Worked
    }

    /// A level the operator switched off is not flagged at all. Note this
    /// does NOT fall through to the next-best level: switching off New Band
    /// means "don't tell me about bands", not "tell me it's a slot instead".
    fn apply_filter(&self, level: AlertLevel) -> AlertLevel {
        let keep = match level {
            AlertLevel::NewDxcc => self.config.alert_new_dxcc,
            AlertLevel::NewSlot => self.config.alert_new_slot,
            AlertLevel::NewBand => self.config.alert_new_band,
            AlertLevel::NewMode => self.config.alert_new_mode,
            AlertLevel::UnconfDxcc => self.config.alert_unconf_dxcc,
            AlertLevel::UnconfSlot => self.config.alert_unconf_slot,
            AlertLevel::UnconfBand => self.config.alert_unconf_band,
            AlertLevel::UnconfMode => self.config.alert_unconf_mode,
            AlertLevel::Worked | AlertLevel::None => return level,
        };
        if keep { level } else { AlertLevel::Worked }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cty::{DxccEntity, PrefixRule};
    use std::collections::HashMap;

    fn resolver() -> DxccResolver {
        let mut entities = HashMap::new();
        entities.insert(
            324,
            DxccEntity {
                adif: 324,
                name: "INDIA".into(),
                prefix: "VU".into(),
                cq_zone: 22,
                continent: "AS".into(),
                deleted: false,
            },
        );
        entities.insert(
            291,
            DxccEntity {
                adif: 291,
                name: "UNITED STATES".into(),
                prefix: "K".into(),
                cq_zone: 5,
                continent: "NA".into(),
                deleted: false,
            },
        );
        let rules = vec![
            PrefixRule {
                call: "VU".into(),
                adif: 324,
                is_exact: false,
                start_unix: None,
                end_unix: None,
            },
            PrefixRule {
                call: "K".into(),
                adif: 291,
                is_exact: false,
                start_unix: None,
                end_unix: None,
            },
        ];
        let mut r = DxccResolver::default();
        r.load(
            crate::cty::CtyData {
                entities,
                prefix_rules: rules,
                ..Default::default()
            },
            0,
        );
        r
    }

    fn matrix() -> LogMatrix {
        let mut m = LogMatrix::default();
        // India worked: 20M DATA (confirmed), 40M CW (unconfirmed).
        m.record(324, "20M", "DATA", "VU2AAA", true);
        m.record(324, "40M", "CW", "VU2BBB", false);
        m
    }

    fn classify(call: &str, mhz: f64, mode: &str, config: &AlertConfig) -> Classification {
        let m = matrix();
        let r = resolver();
        AlertClassifier {
            matrix: &m,
            resolver: &r,
            config,
        }
        .classify(call, mhz, mode)
    }

    #[test]
    fn unknown_mode_answers_only_the_band_half() {
        let cfg = AlertConfig::default();
        // The matrix has India on 20M-DATA (confirmed) and 40M-CW.
        //
        // An empty mode used to reach `canonical("")` and come back DATA, so
        // this spot was scored as the worked 20M-DATA slot — a phone spot
        // silently credited to a digital one. Now the mode half is simply
        // not answered: 20M is worked and confirmed, so nothing is flagged.
        assert_eq!(
            classify("VU2ZZZ", 14.200, "", &cfg).level,
            AlertLevel::Worked,
            "no mode: cannot claim a new mode or slot on a worked band"
        );
        // A band gap is mode-independent, so it still reports.
        assert_eq!(
            classify("VU2ZZZ", 21.200, "", &cfg).level,
            AlertLevel::NewBand,
            "no mode: a never-worked band is still a never-worked band"
        );
        // So is a whole-entity gap.
        assert_eq!(
            classify("K1JT", 14.200, "", &cfg).level,
            AlertLevel::NewDxcc
        );
        // 40M-CW is worked but unconfirmed, so the band is unconfirmed too —
        // visible only with the Unconf half switched on, since it is off by
        // default and `apply_filter` downgrades a disabled level to Worked.
        let unconf = AlertConfig {
            alert_unconf_band: true,
            ..AlertConfig::default()
        };
        assert_eq!(
            classify("VU2ZZZ", 7.150, "", &unconf).level,
            AlertLevel::UnconfBand
        );
        assert_eq!(
            classify("VU2ZZZ", 7.150, "", &cfg).level,
            AlertLevel::Worked,
            "same spot, default config: the disabled level reads as Worked"
        );
        // Contrast: the SAME frequency WITH a mode does reach the mode half.
        assert_eq!(
            classify("VU2ZZZ", 14.200, "SSB", &cfg).level,
            AlertLevel::NewMode,
            "a known mode still gets the full ladder"
        );
    }

    #[test]
    fn level_priorities() {
        let cfg = AlertConfig::default();
        // Unknown entity → new DXCC.
        assert_eq!(
            classify("K1JT", 14.074, "FT8", &cfg).level,
            AlertLevel::NewDxcc
        );
        // Worked slot (FT8 → DATA on 20M).
        assert_eq!(
            classify("VU2ZZZ", 14.074, "FT8", &cfg).level,
            AlertLevel::Worked
        );
        // Never-worked band beats everything.
        assert_eq!(
            classify("VU2ZZZ", 21.074, "FT8", &cfg).level,
            AlertLevel::NewBand
        );
        // Known band + known mode, new combination → new slot.
        assert_eq!(
            classify("VU2ZZZ", 7.074, "FT8", &cfg).level,
            AlertLevel::NewSlot
        );
        // Never-worked mode on a worked band → new mode.
        assert_eq!(
            classify("VU2ZZZ", 14.2, "SSB", &cfg).level,
            AlertLevel::NewMode
        );
    }

    /// All eight on: the `New*` half must still win wherever it applies, and
    /// the `Unconf*` half only speaks for slots that ARE in the log.
    fn all_on() -> AlertConfig {
        AlertConfig {
            alert_unconf_dxcc: true,
            alert_unconf_slot: true,
            alert_unconf_band: true,
            alert_unconf_mode: true,
            ..AlertConfig::default()
        }
    }

    #[test]
    fn unconfirmed_levels_sit_under_the_new_ones() {
        let cfg = all_on();
        // 40M CW is worked but unconfirmed. India HAS a confirmation (20M
        // DATA), so this is not the entity-wide gap — it is the band gap.
        assert_eq!(
            classify("VU2ZZZ", 7.028, "CW", &cfg).level,
            AlertLevel::UnconfBand
        );
        // 20M DATA is confirmed → nothing to chase.
        assert_eq!(
            classify("VU2ZZZ", 14.074, "FT8", &cfg).level,
            AlertLevel::Worked
        );
        // A band never worked at all still outranks any confirmation gap.
        assert_eq!(
            classify("VU2ZZZ", 21.074, "FT8", &cfg).level,
            AlertLevel::NewBand
        );
    }

    #[test]
    fn entity_with_nothing_confirmed_reads_as_unconf_dxcc() {
        let mut m = LogMatrix::default();
        m.record(324, "20M", "DATA", "VU2AAA", false); // worked, never confirmed
        let r = resolver();
        let cfg = all_on();
        let c = AlertClassifier {
            matrix: &m,
            resolver: &r,
            config: &cfg,
        }
        .classify("VU2ZZZ", 14.074, "FT8");
        assert_eq!(c.level, AlertLevel::UnconfDxcc);
    }

    #[test]
    fn unconf_mode_and_slot_are_distinguishable() {
        let mut m = LogMatrix::default();
        // 20M DATA confirmed; 20M CW and 40M DATA worked but not confirmed.
        m.record(324, "20M", "DATA", "VU2AAA", true);
        m.record(324, "20M", "CW", "VU2BBB", false);
        m.record(324, "40M", "DATA", "VU2CCC", false);
        let r = resolver();
        let cfg = all_on();
        let go = |mhz, mode| {
            AlertClassifier {
                matrix: &m,
                resolver: &r,
                config: &cfg,
            }
            .classify("VU2ZZZ", mhz, mode)
            .level
        };
        // 20M is a confirmed band and CW is an unconfirmed mode → mode gap.
        assert_eq!(go(14.030, "CW"), AlertLevel::UnconfMode);
        // 40M band unconfirmed → band gap outranks the mode gap.
        assert_eq!(go(7.074, "FT8"), AlertLevel::UnconfBand);
    }

    #[test]
    fn unconfirmed_levels_are_off_by_default() {
        // A fresh install behaves exactly like 1.x.
        let cfg = AlertConfig::default();
        assert_eq!(
            classify("VU2ZZZ", 7.028, "CW", &cfg).level,
            AlertLevel::Worked,
            "the ? levels must stay silent until switched on"
        );
    }

    #[test]
    fn filters_downgrade_to_worked() {
        let cfg = AlertConfig {
            alert_new_band: false,
            ..AlertConfig::default()
        };
        assert_eq!(
            classify("VU2ZZZ", 21.074, "FT8", &cfg).level,
            AlertLevel::Worked
        );
    }

    #[test]
    fn beacons_and_gaps_never_alert() {
        let cfg = AlertConfig::default();
        let c = classify("4X6TU", 14.1, "CW", &cfg);
        assert_eq!(c.level, AlertLevel::None);
        assert!(c.is_beacon);
        assert_eq!(
            c.dxcc_name.as_deref(),
            Some("NCDXF Beacon — Tel Aviv, Israel")
        );
        // Frequency outside any band → None level, but DXCC still resolved.
        let c = classify("VU2ZZZ", 2.5, "FT8", &cfg);
        assert_eq!(c.level, AlertLevel::None);
        assert_eq!(c.dxcc_id, Some(324));
        // Empty call → nothing.
        assert_eq!(classify("", 14.074, "FT8", &cfg).level, AlertLevel::None);
    }
}
