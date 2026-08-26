//! Spot classification against a user's log matrix — port of the Swift
//! `AlertClassifier`. Decides New DXCC / New Band / New Mode / New Slot /
//! Worked for one spot and one user; in DXCA 2.0 this runs once per user
//! over the shared spot stream (plan §5).

use crate::beacons;
use crate::dxcc::DxccResolver;
use crate::matrix::LogMatrix;
use crate::{bands, modes};
use serde::{Deserialize, Serialize};

/// Serde names match the Swift `AlertLevel` raw values, so 1.x-era JSON
/// and the 2.0 API speak the same strings.
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
}

/// The per-user alert toggles the classifier consults — the subset of the
/// Swift `ClubLogConfig` that affects classification (same defaults).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    pub alert_new_dxcc: bool,
    pub alert_new_slot: bool,
    pub alert_new_band: bool,
    pub alert_new_mode: bool,
    /// Treat unconfirmed QSOs as not worked (confirmation hunting).
    pub alert_unconfirmed: bool,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            alert_new_dxcc: true,
            alert_new_slot: true,
            alert_new_band: true,
            alert_new_mode: true,
            alert_unconfirmed: false,
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
        let normalized_mode = modes::canonical(mode);

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

    fn raw_level(&self, dxcc: i32, band: &str, mode: &str) -> AlertLevel {
        let Some(status) = self.matrix.status(dxcc) else {
            return AlertLevel::NewDxcc;
        };

        // In unconfirmed-hunting mode only confirmed contacts count.
        let (bands, modes, slots) = if self.config.alert_unconfirmed {
            (
                &status.confirmed_bands,
                &status.confirmed_modes,
                &status.confirmed_slots,
            )
        } else {
            (&status.bands, &status.modes, &status.slots)
        };

        if self.config.alert_unconfirmed && bands.is_empty() && modes.is_empty() && slots.is_empty()
        {
            return AlertLevel::NewDxcc;
        }

        let slot = format!("{band}-{mode}");
        if !slots.contains(&slot) {
            // Priority: never-worked band > never-worked mode > new
            // combination of both (the genuine 5BDXCC-style slot).
            if !bands.contains(band) {
                return AlertLevel::NewBand;
            }
            if !modes.contains(mode) {
                return AlertLevel::NewMode;
            }
            return AlertLevel::NewSlot;
        }
        AlertLevel::Worked
    }

    fn apply_filter(&self, level: AlertLevel) -> AlertLevel {
        let keep = match level {
            AlertLevel::NewDxcc => self.config.alert_new_dxcc,
            AlertLevel::NewSlot => self.config.alert_new_slot,
            AlertLevel::NewBand => self.config.alert_new_band,
            AlertLevel::NewMode => self.config.alert_new_mode,
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
        r.load(entities, &rules, 0);
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

    #[test]
    fn unconfirmed_mode_counts_confirmed_only() {
        let cfg = AlertConfig {
            alert_unconfirmed: true,
            ..AlertConfig::default()
        };
        // 40M CW is worked but unconfirmed → still alerts (new band).
        assert_eq!(
            classify("VU2ZZZ", 7.028, "CW", &cfg).level,
            AlertLevel::NewBand
        );
        // 20M DATA is confirmed → worked.
        assert_eq!(
            classify("VU2ZZZ", 14.074, "FT8", &cfg).level,
            AlertLevel::Worked
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
