//! Callsign → DXCC entity resolution — port of the Swift `DXCCResolver`.
//! Exact-call exceptions win, then longest-prefix match; rules inactive at
//! load time (historical entities, expired exceptions) are dropped so they
//! can't contaminate today's lookups.

use crate::cty::{CtyData, DxccEntity, InvalidOperation, PrefixRule};
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
    /// For each **whitelisted** entity, every exact-call rule ClubLog lists
    /// for it — the accepted operations, with their date windows.
    ///
    /// Kept separately from `exact` because that map is filtered to rules
    /// active *now*, and a whitelist is a historical record: ZL8X counted in
    /// November 2010 and counts still, even though the rule expired in 2010.
    whitelist: HashMap<i32, Vec<PrefixRule>>,
    /// Exact-call and prefix CQ-zone overrides, mirroring `exact`/`prefix`.
    /// Only rules that actually carry a `<cqz>` land here, so a miss falls
    /// through to the entity's own zone rather than to a wrong number.
    exact_zone: HashMap<String, i32>,
    prefix_zone: HashMap<String, i32>,
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
        self.exact_zone.clear();
        self.prefix_zone.clear();
        self.invalid.clear();
        self.whitelist.clear();
        for op in data.invalid_operations {
            self.invalid.entry(op.call.clone()).or_default().push(op);
        }
        // Built from the UNFILTERED rules, and only for entities that are
        // actually whitelisted — every other entity accepts a prefix match,
        // so carrying its exceptions here would be dead weight.
        for rule in data.prefix_rules.iter().filter(|r| r.is_exact) {
            if self.entities.get(&rule.adif).is_some_and(|e| e.whitelist) {
                self.whitelist
                    .entry(rule.adif)
                    .or_default()
                    .push(rule.clone());
            }
        }
        for rule in data.prefix_rules.iter().filter(|r| r.is_active(now_unix)) {
            if rule.is_exact {
                self.exact.insert(rule.call.clone(), rule.adif);
                if let Some(z) = rule.cq_zone {
                    self.exact_zone.insert(rule.call.clone(), z);
                }
            } else {
                // First seen wins on duplicates, like the Swift load.
                self.prefix.entry(rule.call.clone()).or_insert(rule.adif);
                if let Some(z) = rule.cq_zone {
                    self.prefix_zone.entry(rule.call.clone()).or_insert(z);
                }
            }
        }
        self.sorted_prefixes = self.prefix.keys().cloned().collect();
        self.sorted_prefixes
            .sort_by_key(|p| std::cmp::Reverse(p.len()));
    }

    /// The **CQ zone** for a callsign — what WAZ and the DX Marathon count.
    ///
    /// Walks the same specificity ladder `resolve` does, because the answer
    /// has to change with it: an exact-call exception first, then the
    /// longest matching prefix, then the entity's own zone as the fallback.
    /// That order is the whole feature — `W6` and `W1` are one entity and
    /// two zones, and only the prefix rule knows which.
    ///
    /// A US `KG4` 2×3 call ([`is_us_kg4`]) skips the prefix step: the
    /// longest prefix it matches is Guantanamo's `KG4`, zone 8. It falls
    /// through to the zone of whatever `resolve` returns instead — the USA's
    /// own zone, or the entity of an exception that names the call.
    ///
    /// `None` when nothing gives a zone, or when the value is outside 1–40:
    /// a zone we cannot place is not a zone to award.
    pub fn zone(&self, callsign: &str) -> Option<i32> {
        let clean = normalize_call(&callsign.to_uppercase());
        let z = self
            .exact_zone
            .get(&clean)
            .copied()
            .or_else(|| {
                if is_us_kg4(&clean) {
                    return None;
                }
                self.sorted_prefixes
                    .iter()
                    .find(|p| clean.starts_with(p.as_str()))
                    .and_then(|p| self.prefix_zone.get(p).copied())
            })
            .or_else(|| {
                self.resolve(callsign)
                    .and_then(|adif| self.entity(adif))
                    .map(|e| e.cq_zone)
            })?;
        (1..=40).contains(&z).then_some(z)
    }

    /// Resolve to a DXCC entity id; None when unloaded, unmatched, or the
    /// call is a ClubLog non-DX operation (exact rule with adif 0).
    ///
    /// Exact-call exceptions first, then the one rule cty.xml does not carry
    /// as data — a `KG4` 2×3 call is the USA, see [`is_us_kg4`] — then the
    /// longest matching prefix.
    pub fn resolve(&self, callsign: &str) -> Option<i32> {
        let clean = normalize_call(&callsign.to_uppercase());
        if let Some(&adif) = self.exact.get(&clean) {
            return (adif > 0).then_some(adif);
        }
        if is_us_kg4(&clean) && self.entities.contains_key(&USA) {
            return Some(USA);
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

    /// How many entities are whitelisted — 59 in the live cty.xml.
    pub fn whitelisted_entity_count(&self) -> usize {
        self.whitelist.len()
    }

    /// True when `adif` is a [whitelisted](DxccEntity::whitelist) entity and
    /// `call` is **not** one of the operations ClubLog accepts for it at
    /// `at_unix` — so the contact earns no credit for that entity.
    ///
    /// The date matters twice over: the whitelist itself may only start at a
    /// given date (Iran from 2019, the Pacific islands from the 1978 rule
    /// change), and each accepted operation has its own window, because the
    /// same callsign is often reissued to a later DXpedition.
    ///
    /// **An unknown QSO time is never rejected.** Neither is a call in an
    /// entity that is not whitelisted. Both follow the rule the
    /// invalid-operation check uses: do not discard a contact that cannot be
    /// placed.
    ///
    /// **Caveat worth knowing:** cty.xml lags real operations. A DXpedition
    /// ClubLog has not yet listed will be rejected here — correctly, in that
    /// ClubLog will not credit it either, but it means a brand-new entity can
    /// read as un-worked until the next cty.xml refresh catches up.
    pub fn is_whitelist_rejected(&self, adif: i32, call: &str, at_unix: Option<i64>) -> bool {
        let Some(accepted) = self.whitelist.get(&adif) else {
            return false; // not a whitelisted entity
        };
        let entity = self.entities.get(&adif);
        let starts = entity.and_then(|e| e.whitelist_start_unix);
        let Some(at) = at_unix else { return false };
        if starts.is_some_and(|s| at < s) {
            return false; // before the whitelist applied, ordinary calls count
        }
        let call = call.to_uppercase();
        // `K9HEI/KH9` in the log is `KH9/K9HEI` in cty.xml — the same
        // operation with the location prefix on the other side of the slash,
        // and ClubLog treats them as one. Matching only the literal string
        // rejected a *confirmed* Wake Island QSO from VU2CPL's log, which is
        // the tell for a false rejection: a QSL match means the operation was
        // real. Both orders are therefore tried.
        let alt = swap_slash(&call);
        !accepted
            .iter()
            .any(|r| (r.call == call || Some(&r.call) == alt.as_ref()) && r.is_active(at))
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

/// ADIF id of the United States.
const USA: i32 = 291;

/// True for `KG4` plus a **three-letter** suffix — an ordinary US station
/// in call area 4, not Guantanamo Bay.
///
/// Only `KG4` with a two-letter suffix (`KG4AB`) is Guantanamo; the FCC
/// issues the 2×3 form (`KG4OJT`) sequentially to stateside amateurs. cty.xml
/// carries a bare `KG4` prefix rule for adif 105, so a plain prefix walk
/// sends every one of them to Guantanamo — a false *New One* alert. ClubLog
/// applies the suffix-length rule in its own code rather than in the data:
/// its exceptions list only the 2×3 `KG4` calls that are somewhere *else*
/// (`KG4TJS` Alaska, `KG4FJB` Hawaii), never the ones that are simply USA.
/// Those exceptions still win — both callers check them first.
///
/// Takes a [`normalize_call`] result, so `KG4OJT/P` qualifies while
/// `KG4/KG4OJT` — a US station operating from Guantanamo — normalises to
/// `KG4` and does not. `KG44WW`, a special-event call, has a digit in its
/// suffix and does not either.
fn is_us_kg4(clean: &str) -> bool {
    clean.len() == 6
        && clean.starts_with("KG4")
        && clean.bytes().skip(3).all(|b| b.is_ascii_uppercase())
}

/// `A/B` → `B/A`, for a call with exactly two parts; `None` otherwise.
///
/// Only used by the whitelist lookup. It is not normalisation — nothing is
/// dropped — just the other way round of writing the same operation.
fn swap_slash(call: &str) -> Option<String> {
    let parts: Vec<&str> = call.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [a, b] => Some(format!("{b}/{a}")),
        _ => None,
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
            cq_zone: None,
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
                    ..Default::default()
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

    fn entity(adif: i32, whitelist: bool, start: Option<i64>) -> DxccEntity {
        DxccEntity {
            adif,
            name: "TEST".into(),
            whitelist,
            whitelist_start_unix: start,
            ..Default::default()
        }
    }

    /// `adif` 133 is whitelisted, 324 is not; `accepted` are its listed
    /// operations as (call, start, end).
    fn whitelisted(
        start: Option<i64>,
        accepted: &[(&str, Option<i64>, Option<i64>)],
    ) -> DxccResolver {
        let mut r = DxccResolver::default();
        r.load(
            CtyData {
                entities: HashMap::from([
                    (133, entity(133, true, start)),
                    (324, entity(324, false, None)),
                ]),
                prefix_rules: accepted
                    .iter()
                    .map(|(c, s, e)| PrefixRule {
                        call: (*c).into(),
                        adif: 133,
                        is_exact: true,
                        start_unix: *s,
                        end_unix: *e,
                        cq_zone: None,
                    })
                    .collect(),
                ..Default::default()
            },
            0,
        );
        r
    }

    /// The VU24DX case: `ZL8` resolves to Kermadec, but Kermadec accepts only
    /// the calls ClubLog lists, and `ZL8AC` is not one of them.
    #[test]
    fn a_whitelisted_entity_rejects_an_unlisted_call() {
        let r = whitelisted(None, &[("ZL8X", Some(100), Some(200))]);
        assert_eq!(r.whitelisted_entity_count(), 1);
        assert!(r.is_whitelist_rejected(133, "ZL8AC", Some(150)));
        assert!(!r.is_whitelist_rejected(133, "ZL8X", Some(150)), "listed");
        // Not a whitelisted entity — any call is fine.
        assert!(!r.is_whitelist_rejected(324, "VU2CPL", Some(150)));
        // Unknown entity: nothing to reject against.
        assert!(!r.is_whitelist_rejected(999, "ZL8AC", Some(150)));
    }

    /// A listed call still has to be worked inside *its own* window — the
    /// same callsign is reissued to later DXpeditions.
    #[test]
    fn a_listed_call_is_still_bound_by_its_window() {
        let r = whitelisted(None, &[("ZL8X", Some(100), Some(200))]);
        assert!(!r.is_whitelist_rejected(133, "ZL8X", Some(150)));
        assert!(r.is_whitelist_rejected(133, "ZL8X", Some(99)));
        assert!(r.is_whitelist_rejected(133, "ZL8X", Some(201)));
    }

    /// Expired rules are dropped from `exact` at load, but a whitelist is a
    /// historical record: a 2010 DXpedition still counts for a 2010 QSO.
    #[test]
    fn the_whitelist_keeps_rules_that_have_since_expired() {
        // now = 10_000, well past the window.
        let mut r = DxccResolver::default();
        r.load(
            CtyData {
                entities: HashMap::from([(133, entity(133, true, None))]),
                prefix_rules: vec![PrefixRule {
                    call: "ZL8X".into(),
                    adif: 133,
                    is_exact: true,
                    start_unix: Some(100),
                    end_unix: Some(200),
                    cq_zone: None,
                }],
                ..Default::default()
            },
            10_000,
        );
        assert_eq!(r.resolve("ZL8X"), None, "dropped from live resolution");
        assert!(
            !r.is_whitelist_rejected(133, "ZL8X", Some(150)),
            "but still accepted for a QSO inside its window"
        );
    }

    /// `K9HEI/KH9` in a log is `KH9/K9HEI` in cty.xml. Matching the literal
    /// string only would reject a confirmed Wake Island QSO.
    #[test]
    fn either_side_of_the_slash_matches() {
        let r = whitelisted(None, &[("KH9/K9HEI", None, None)]);
        assert!(!r.is_whitelist_rejected(133, "KH9/K9HEI", Some(150)));
        assert!(!r.is_whitelist_rejected(133, "K9HEI/KH9", Some(150)));
        // Not a licence to match anything with a slash in it.
        assert!(r.is_whitelist_rejected(133, "KH9/W1ABC", Some(150)));
        assert!(r.is_whitelist_rejected(133, "K9HEI", Some(150)));
    }

    /// Ten entities were only whitelisted from a date — the 1978 Pacific
    /// rule change, Turkmenistan in 2007, Iran in 2019. Before it, ordinary
    /// calls counted and must keep counting.
    #[test]
    fn a_dated_whitelist_does_not_reach_back_before_its_start() {
        let r = whitelisted(Some(1_000), &[("ZL8X", None, None)]);
        assert!(!r.is_whitelist_rejected(133, "ZL8AC", Some(999)), "before");
        assert!(r.is_whitelist_rejected(133, "ZL8AC", Some(1_000)), "on");
        assert!(r.is_whitelist_rejected(133, "ZL8AC", Some(5_000)), "after");
    }

    /// Same rule as the invalid list: never discard a contact we cannot place.
    #[test]
    fn an_undated_qso_is_never_whitelist_rejected() {
        let r = whitelisted(None, &[("ZL8X", Some(100), Some(200))]);
        assert!(!r.is_whitelist_rejected(133, "ZL8AC", None));
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

    /// The shape of the live cty.xml around `KG4`: a bare `KG4` prefix for
    /// Guantanamo, `K`/`W` for the USA, and exceptions only for the 2×3
    /// calls that are elsewhere. `KG4TJS` → Alaska is a real one;
    /// `KG4XYZ` → Guantanamo without a `<cqz>` is invented, to pin that an
    /// exception decides the zone even when it carries none.
    const KG4_CTY: &str = r#"<?xml version="1.0"?>
<clublog date="2026-09-21">
 <entities>
  <entity><adif>291</adif><name>UNITED STATES OF AMERICA</name><prefix>K</prefix><deleted>false</deleted><cqz>5</cqz><cont>NA</cont></entity>
  <entity><adif>105</adif><name>GUANTANAMO BAY</name><prefix>KG4</prefix><deleted>false</deleted><cqz>8</cqz><cont>NA</cont></entity>
  <entity><adif>6</adif><name>ALASKA</name><prefix>KL</prefix><deleted>false</deleted><cqz>1</cqz><cont>NA</cont></entity>
 </entities>
 <exceptions>
  <exception record="1"><call>KG4TJS</call><entity>ALASKA</entity><adif>6</adif><cqz>1</cqz><start>2014-05-16T00:00:00+00:00</start></exception>
  <exception record="2"><call>KG4XYZ</call><entity>GUANTANAMO BAY</entity><adif>105</adif></exception>
 </exceptions>
 <prefixes>
  <prefix record="1"><call>K</call><entity>UNITED STATES OF AMERICA</entity><adif>291</adif><cqz>5</cqz></prefix>
  <prefix record="2"><call>W</call><entity>UNITED STATES OF AMERICA</entity><adif>291</adif><cqz>5</cqz></prefix>
  <prefix record="3"><call>KG4</call><entity>GUANTANAMO BAY</entity><adif>105</adif><cqz>8</cqz><start>1949-01-01T00:00:00+00:00</start></prefix>
 </prefixes>
</clublog>"#;

    fn kg4_resolver() -> DxccResolver {
        let mut r = DxccResolver::default();
        let now = crate::cty::parse_iso8601("2026-09-21T22:19:00+00:00").unwrap();
        r.load(crate::cty::parse(KG4_CTY).expect("fixture parses"), now);
        r
    }

    /// KG4OJT, spotted by VU2OY's RBN on 20 m FT8, 2026-09-21 — alerted
    /// as a New Mode for Guantanamo Bay. It is a stateside call.
    #[test]
    fn a_kg4_call_with_a_three_letter_suffix_is_the_usa() {
        let r = kg4_resolver();
        assert_eq!(r.resolve("KG4OJT"), Some(291));
        assert_eq!(r.resolve("kg4ojt"), Some(291), "case-insensitive");
        assert_eq!(r.entity_name("KG4OJT"), Some("UNITED STATES OF AMERICA"));
    }

    #[test]
    fn a_kg4_call_with_a_two_letter_suffix_is_guantanamo() {
        let r = kg4_resolver();
        assert_eq!(r.resolve("KG4AB"), Some(105));
        // A digit in the suffix is not a 2×3 call — KG44WW was a
        // Guantanamo special-event station.
        assert_eq!(r.resolve("KG44WW"), Some(105));
    }

    #[test]
    fn an_exception_still_beats_the_kg4_rule() {
        let r = kg4_resolver();
        assert_eq!(
            r.resolve("KG4TJS"),
            Some(6),
            "ClubLog puts KG4TJS in Alaska"
        );
        assert_eq!(r.zone("KG4TJS"), Some(1));
        assert_eq!(r.resolve("KG4XYZ"), Some(105));
        assert_eq!(
            r.zone("KG4XYZ"),
            Some(8),
            "no <cqz> on the exception: the zone follows its entity, not the USA's"
        );
    }

    #[test]
    fn a_us_kg4_call_takes_a_us_zone() {
        let r = kg4_resolver();
        assert_ne!(r.zone("KG4OJT"), Some(8), "zone 8 is Guantanamo's");
        assert_eq!(r.zone("KG4OJT"), Some(5), "the USA's own zone");
        assert_eq!(r.zone("KG4AB"), Some(8));
    }

    #[test]
    fn kg4_portable_forms_normalise_before_the_rule() {
        let r = kg4_resolver();
        assert_eq!(r.resolve("KG4OJT/P"), Some(291));
        assert_eq!(r.resolve("KG4OJT/4"), Some(291));
        assert_eq!(r.resolve("KG4OJT/QRP"), Some(291));
        // Location prefix in front: the shorter side is where they are.
        assert_eq!(r.resolve("W4/KG4OJT"), Some(291));
        assert_eq!(
            r.resolve("KG4/KG4OJT"),
            Some(105),
            "operating from Guantanamo"
        );
        assert_eq!(r.resolve("KG4AB/P"), Some(105));
        assert_eq!(r.zone("KG4OJT/P"), Some(5));
    }

    /// The rule must not answer for a resolver with no data behind it.
    #[test]
    fn an_unloaded_resolver_places_no_kg4_call() {
        let r = DxccResolver::default();
        assert_eq!(r.resolve("KG4OJT"), None);
        assert_eq!(r.zone("KG4OJT"), None);
    }
}
