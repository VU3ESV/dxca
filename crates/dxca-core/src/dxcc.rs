//! Callsign → DXCC entity resolution — port of the Swift `DXCCResolver`.
//! Exact-call exceptions win, then longest-prefix match; rules inactive at
//! load time (historical entities, expired exceptions) are dropped so they
//! can't contaminate today's lookups.

use crate::cty::{DxccEntity, PrefixRule};
use std::collections::HashMap;

#[derive(Default)]
pub struct DxccResolver {
    entities: HashMap<i32, DxccEntity>,
    exact: HashMap<String, i32>,
    prefix: HashMap<String, i32>,
    /// Prefixes sorted longest-first for longest-match resolution.
    sorted_prefixes: Vec<String>,
}

impl DxccResolver {
    pub fn is_loaded(&self) -> bool {
        !self.entities.is_empty()
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// `now_unix` filters rule activity — the Swift resolver used `Date()`
    /// internally; dxca-core has no clock, so the caller supplies it.
    pub fn load(
        &mut self,
        entities: HashMap<i32, DxccEntity>,
        rules: &[PrefixRule],
        now_unix: i64,
    ) {
        self.entities = entities;
        self.exact.clear();
        self.prefix.clear();
        for rule in rules.iter().filter(|r| r.is_active(now_unix)) {
            if rule.is_exact {
                self.exact.insert(rule.call.clone(), rule.adif);
            } else {
                // First seen wins on duplicates, like the Swift load.
                self.prefix.entry(rule.call.clone()).or_insert(rule.adif);
            }
        }
        self.sorted_prefixes = self.prefix.keys().cloned().collect();
        self.sorted_prefixes
            .sort_by_key(|p| std::cmp::Reverse(p.len()));
    }

    /// Resolve to a DXCC entity id; None when unloaded, unmatched, or the
    /// call is a ClubLog non-DX operation (exact rule with adif 0).
    pub fn resolve(&self, callsign: &str) -> Option<i32> {
        let clean = normalize_call(&callsign.to_uppercase());
        if let Some(&adif) = self.exact.get(&clean) {
            return (adif > 0).then_some(adif);
        }
        for prefix in &self.sorted_prefixes {
            if clean.starts_with(prefix.as_str()) {
                let adif = self.prefix[prefix];
                return (adif > 0).then_some(adif);
            }
        }
        None
    }

    /// True when ClubLog flags the call as a non-DX operation (beacons,
    /// satellites, Internet gateways — exact records with adif 0).
    pub fn is_non_dx_operation(&self, callsign: &str) -> bool {
        let clean = normalize_call(&callsign.to_uppercase());
        self.exact.get(&clean) == Some(&0)
    }

    /// ADIF ids of every **deleted** entity this resolver knows.
    ///
    /// Handed to [`crate::matrix::LogMatrix::stats_excluding`] so award
    /// totals can be shown the way the ARRL counts them. The matrix itself
    /// stays resolver-free — it stores what was worked, not what currently
    /// scores — so the caller, which holds both, supplies the set.
    pub fn deleted_adifs(&self) -> std::collections::HashSet<i32> {
        self.entities
            .values()
            .filter(|e| e.deleted)
            .map(|e| e.adif)
            .collect()
    }

    pub fn entity(&self, adif: i32) -> Option<&DxccEntity> {
        self.entities.get(&adif)
    }

    pub fn entity_name(&self, callsign: &str) -> Option<&str> {
        self.entity(self.resolve(callsign)?)
            .map(|e| e.name.as_str())
    }
}

/// Normalize a slash-portable callsign, Swift rules verbatim:
/// portable suffixes drop ("K1JT/P" → K1JT), numeric call-area suffixes
/// drop ("W1AW/4" → W1AW), and for prefix overrides the shorter side wins
/// ("VP8/K1JT" → VP8). Splitting skips empty parts (Swift `split` default).
fn normalize_call(call: &str) -> String {
    if !call.contains('/') {
        return call.to_string();
    }
    let parts: Vec<&str> = call.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() != 2 {
        return parts.first().unwrap_or(&call).to_string();
    }
    let (a, b) = (parts[0], parts[1]);

    const PORTABLE: [&str; 9] = ["P", "M", "MM", "AM", "QRP", "A", "B", "LH", "BCN"];
    if PORTABLE.contains(&b) {
        return a.to_string();
    }
    if PORTABLE.contains(&a) {
        return b.to_string();
    }

    // Numeric call-area suffix like "W1AW/4" (≤2 alphanumeric chars with a
    // digit) → keep the main call.
    if b.len() <= 2 && b.chars().all(|c| c.is_alphanumeric()) && b.chars().any(|c| c.is_numeric()) {
        return a.to_string();
    }

    // Prefix override: the shorter side is usually the location prefix.
    if a.len() <= b.len() {
        a.to_string()
    } else {
        b.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(call: &str, adif: i32, exact: bool) -> PrefixRule {
        PrefixRule {
            call: call.into(),
            adif,
            is_exact: exact,
            start_unix: None,
            end_unix: None,
        }
    }

    fn resolver() -> DxccResolver {
        let mut entities = HashMap::new();
        for (adif, name, prefix) in [
            (324, "INDIA", "VU"),
            (291, "UNITED STATES", "K"),
            (141, "FALKLAND IS.", "VP8"),
        ] {
            entities.insert(
                adif,
                DxccEntity {
                    adif,
                    name: name.into(),
                    prefix: prefix.into(),
                    cq_zone: 0,
                    continent: String::new(),
                    deleted: false,
                },
            );
        }
        let rules = vec![
            rule("VU", 324, false),
            rule("K", 291, false),
            rule("VP8", 141, false),
            rule("VU4", 11, false),   // longer prefix must beat "VU"
            rule("K1ABC", 999, true), // exact overrides prefix
            rule("4X6TU", 0, true),   // non-DX operation
        ];
        let mut r = DxccResolver::default();
        r.load(entities, &rules, 0);
        r
    }

    #[test]
    fn exact_beats_prefix_and_longest_prefix_wins() {
        let r = resolver();
        assert_eq!(r.resolve("VU2CPL"), Some(324));
        assert_eq!(r.resolve("VU4KV"), Some(11));
        assert_eq!(r.resolve("K1ABC"), Some(999));
        assert_eq!(r.resolve("K1JT"), Some(291));
    }

    #[test]
    fn non_dx_operations_resolve_to_none() {
        let r = resolver();
        assert_eq!(r.resolve("4X6TU"), None);
        assert!(r.is_non_dx_operation("4X6TU"));
        assert!(!r.is_non_dx_operation("VU2CPL"));
    }

    #[test]
    fn expired_rules_are_dropped_at_load() {
        let mut r = DxccResolver::default();
        let mut expired = rule("VU", 324, false);
        expired.end_unix = Some(100);
        r.load(
            HashMap::from([(324, resolver().entity(324).unwrap().clone())]),
            &[expired],
            200,
        );
        assert_eq!(r.resolve("VU2CPL"), None);
    }

    #[test]
    fn portable_normalization() {
        let r = resolver();
        assert_eq!(r.resolve("VU2CPL/P"), Some(324)); // suffix drops
        assert_eq!(r.resolve("K1JT/4"), Some(291)); // call-area drops
        assert_eq!(r.resolve("VP8/K1JT"), Some(141)); // shorter side = location
        assert_eq!(r.resolve("P/VU2CPL"), Some(324)); // leading portable marker
    }
}
