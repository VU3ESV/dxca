//! Callsign → DXCC entity resolution — port of the Swift `DXCCResolver`.
//! Exact-call exceptions win, then longest-prefix match; rules inactive at
//! load time (historical entities, expired exceptions) are dropped so they
//! can't contaminate today's lookups.

use crate::cty::{CtyData, DxccEntity, InvalidOperation};
use std::collections::HashMap;

#[derive(Default)]
pub struct DxccResolver {
    entities: HashMap<i32, DxccEntity>,
    exact: HashMap<String, i32>,
    prefix: HashMap<String, i32>,
    /// Prefixes sorted longest-first for longest-match resolution.
    sorted_prefixes: Vec<String>,
    /// ClubLog's invalid-operation list, keyed by raw callsign. Unlike the
    /// prefix rules these are **not** filtered by load-time activity: they
    /// are tested against each QSO's own date, so a window that closed in
    /// 2013 still has to invalidate a 2013 contact.
    invalid: HashMap<String, Vec<InvalidOperation>>,
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
    ///
    /// Takes the whole [`CtyData`] rather than its pieces so that a caller
    /// cannot load the entities and prefixes while silently dropping the
    /// invalid-operation list — forgetting it is not a compile error but a
    /// wrong DXCC total, which is exactly the bug this list fixes.
    pub fn load(&mut self, data: CtyData, now_unix: i64) {
        self.entities = data.entities;
        self.exact.clear();
        self.prefix.clear();
        self.invalid.clear();
        for op in data.invalid_operations {
            self.invalid.entry(op.call.clone()).or_default().push(op);
        }
        for rule in data.prefix_rules.iter().filter(|r| r.is_active(now_unix)) {
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

    /// True when ClubLog lists this contact as an
    /// [`InvalidOperation`] — the call is on the invalid list and
    /// `at_unix` (the QSO's own time, from
    /// [`Record::qso_datetime_unix`](crate::adif::Record::qso_datetime_unix))
    /// falls inside one of its windows.
    ///
    /// Matches the **raw** call, deliberately un-normalised: the list names
    /// full callsigns, and `SV2RSG/A` normalises to `SV2RSG`, a different
    /// and perfectly valid station.
    pub fn is_invalid_operation(&self, callsign: &str, at_unix: Option<i64>) -> bool {
        self.invalid
            .get(&callsign.to_uppercase())
            .is_some_and(|ops| ops.iter().any(|op| op.covers(at_unix)))
    }

    /// How many invalid-operation entries are loaded — the live cty.xml
    /// carries a few thousand. Zero after loading a file that has none.
    pub fn invalid_operation_count(&self) -> usize {
        self.invalid.values().map(Vec::len).sum()
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
    use crate::cty::PrefixRule;

    fn rule(call: &str, adif: i32, exact: bool) -> PrefixRule {
        PrefixRule {
            call: call.into(),
            adif,
            is_exact: exact,
            start_unix: None,
            end_unix: None,
        }
    }

    fn data(entities: HashMap<i32, DxccEntity>, rules: Vec<PrefixRule>) -> CtyData {
        CtyData {
            entities,
            prefix_rules: rules,
            ..Default::default()
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
        r.load(data(entities, rules), 0);
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
            data(
                HashMap::from([(324, resolver().entity(324).unwrap().clone())]),
                vec![expired],
            ),
            200,
        );
        assert_eq!(r.resolve("VU2CPL"), None);
    }

    fn invalid(call: &str, start: Option<i64>, end: Option<i64>) -> InvalidOperation {
        InvalidOperation {
            call: call.into(),
            start_unix: start,
            end_unix: end,
        }
    }

    fn with_invalid(ops: Vec<InvalidOperation>) -> DxccResolver {
        let mut r = DxccResolver::default();
        r.load(
            CtyData {
                invalid_operations: ops,
                ..Default::default()
            },
            0,
        );
        r
    }

    /// The windowed case — a call flagged only for the period ClubLog
    /// rejected. A contact either side of it is a real one.
    #[test]
    fn invalid_operations_respect_their_window() {
        let r = with_invalid(vec![invalid("SV2RSG/A", Some(100), Some(200))]);
        assert!(r.is_invalid_operation("SV2RSG/A", Some(150)));
        assert!(r.is_invalid_operation("SV2RSG/A", Some(100)), "inclusive");
        assert!(r.is_invalid_operation("SV2RSG/A", Some(200)), "inclusive");
        assert!(!r.is_invalid_operation("SV2RSG/A", Some(99)));
        assert!(!r.is_invalid_operation("SV2RSG/A", Some(201)));
        // Undated QSO: a windowed entry cannot place it, so it stands.
        assert!(!r.is_invalid_operation("SV2RSG/A", None));
    }

    /// Most entries have no window at all — the call never counted.
    #[test]
    fn unbounded_invalid_operations_always_match() {
        let r = with_invalid(vec![invalid("HM0DX", None, None)]);
        assert!(r.is_invalid_operation("HM0DX", Some(12_345)));
        assert!(
            r.is_invalid_operation("HM0DX", None),
            "no date still counts"
        );
        assert!(r.is_invalid_operation("hm0dx", Some(1)), "case-insensitive");
        assert!(
            !r.is_invalid_operation("HM0DXX", Some(1)),
            "exact, not prefix"
        );
    }

    /// One call, several rejected periods — ClubLog lists SV2RSG/A three
    /// times. Any one of them has to match.
    #[test]
    fn several_windows_for_one_call() {
        let r = with_invalid(vec![
            invalid("SV2RSG/A", Some(100), Some(200)),
            invalid("SV2RSG/A", Some(500), Some(600)),
        ]);
        assert_eq!(r.invalid_operation_count(), 2);
        assert!(r.is_invalid_operation("SV2RSG/A", Some(150)));
        assert!(r.is_invalid_operation("SV2RSG/A", Some(550)));
        assert!(!r.is_invalid_operation("SV2RSG/A", Some(350)), "the gap");
    }

    /// The list names full callsigns. Normalising the lookup would strip
    /// `SV2RSG/A` to `SV2RSG` — a different, valid station — and would also
    /// stop the flagged call from matching itself.
    #[test]
    fn invalid_lookup_does_not_normalize_the_call() {
        let r = with_invalid(vec![invalid("SV2RSG/A", None, None)]);
        assert!(r.is_invalid_operation("SV2RSG/A", None));
        assert!(
            !r.is_invalid_operation("SV2RSG", None),
            "the un-suffixed call is a different station"
        );
    }

    /// A resolver loaded from cty.xml without the section — or from an old
    /// cached file — must simply flag nothing.
    #[test]
    fn no_invalid_list_flags_nothing() {
        let r = resolver();
        assert_eq!(r.invalid_operation_count(), 0);
        assert!(!r.is_invalid_operation("SV2RSG/A", Some(150)));
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
