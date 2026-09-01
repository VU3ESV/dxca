//! ClubLog `cty.xml` parser — port of the Swift `CTYParser`.
//!
//! The file has three sections the 1.x app consumes: `<entities>` (DXCC
//! entity list), `<exceptions>` (exact-call overrides), and `<prefixes>`
//! (prefix → DXCC rules), plus a fourth the 1.x app ignored and this port
//! now reads: `<invalid_operations>` (see [`InvalidOperation`]). The Swift
//! implementation is an event-driven
//! `XMLParserDelegate`; this port drives the same record-assembly logic
//! from a minimal built-in XML scanner (start/end/text events, entity
//! decoding, attributes skipped) — cty.xml is machine-generated and needs
//! nothing more. Swift-parity details preserved:
//!  - the text buffer clears on **both** element start and end, so only
//!    leaf text survives (parent text around children is dropped);
//!  - `<entity>`/`<prefix>` appear both as records and as nested labels —
//!    disambiguated by the parent element, exactly as in Swift;
//!  - an entity's canonical prefix registers as a rule (exact-match when it
//!    looks like a full callsign: has a digit and ≥3 letters — 4U1UN,
//!    1A0KM), skipped for deleted entities;
//!  - dates are ISO-8601 internet format; a rule is active when
//!    `start ≤ at ≤ end` with missing bounds open.

use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DxccEntity {
    pub adif: i32,
    pub name: String,
    pub prefix: String,
    pub cq_zone: i32,
    pub continent: String,
    /// A **whitelisted** entity: ClubLog credits a QSO here *only* when the
    /// callsign is one it has explicitly listed as an exception. 59 entities
    /// carry this — the rare ones a pirate would pick, from Bouvet and
    /// Navassa to Mount Athos, Kermadec and North Korea.
    ///
    /// Matching the bare prefix is not enough for these. VU24DX's log holds
    /// one `ZL8AC` QSO; `ZL8` resolves to Kermadec, but ZL8AC appears nowhere
    /// in cty.xml, so ClubLog gives no credit and DXCA — which read the
    /// prefix and stopped there — did. That was the whole of its 314 against
    /// ClubLog's 313.
    pub whitelist: bool,
    /// When the whitelist began to apply. `None` means always. Ten entities
    /// carry a date: Turkmenistan from 2007, Iran from 2019, the Pacific
    /// islands from the 1978 rule change — before it, ordinary calls counted.
    pub whitelist_start_unix: Option<i64>,
    /// An ARRL **deleted** entity — Abu Ail, Blenheim Reef, British North
    /// Borneo and 59 others. A QSO with one still counts as worked and is
    /// still a real contact, but it scores nothing toward *current* DXCC or
    /// the Challenge, so an operator comparing their totals against the
    /// ARRL standings needs to exclude them.
    ///
    /// cty.xml has always carried this; it was read only to skip building a
    /// prefix rule (a deleted entity has no live prefix to resolve) and was
    /// then discarded, so nothing downstream could tell the difference.
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrefixRule {
    pub call: String,
    pub adif: i32,
    pub is_exact: bool,
    pub start_unix: Option<i64>,
    pub end_unix: Option<i64>,
    /// The CQ zone this rule overrides the entity's with, when cty.xml
    /// gives one.
    ///
    /// **This is what makes WAZ possible at all.** An entity-level zone is
    /// a single number, and the big entities span several: the USA covers
    /// zones 3, 4 and 5, Russia nine. ClubLog puts a `<cqz>` on the prefix
    /// and exception records precisely so `W6` and `W1` resolve
    /// differently, and it was being parsed and dropped until 2.19.
    pub cq_zone: Option<i32>,
}

impl PrefixRule {
    pub fn is_active(&self, at_unix: i64) -> bool {
        if let Some(start) = self.start_unix
            && at_unix < start
        {
            return false;
        }
        if let Some(end) = self.end_unix
            && at_unix > end
        {
            return false;
        }
        true
    }
}

/// A ClubLog **invalid operation**: an exact callsign, optionally bounded by
/// a date window, whose QSOs do not count for DXCC — pirates, unlicensed
/// operations, and DXpeditions whose paperwork was later rejected.
///
/// ClubLog resolves such a QSO to an entity (its dashboard still labels the
/// contact) but refuses to credit it. 1.x never read this section, so a
/// station whose only contact with an entity was an invalidated operation
/// counted as worked here and not on ClubLog — VU24DX's Mount Athos, worked
/// solely as `SV2RSG/A`, was exactly one such entity.
///
/// Matched on the **raw** callsign, never the portable-normalised one: the
/// list is full calls (`SV2RSG/A`, `3D2/N1GXE`, `1A0KM/IBYO`) and
/// [`crate::dxcc::normalize_call`] would strip `SV2RSG/A` down to `SV2RSG`,
/// which is a different — valid — station.
#[derive(Debug, Clone, PartialEq)]
pub struct InvalidOperation {
    pub call: String,
    /// Open bounds mean "always invalid on that side". Both open — which is
    /// the common case — means the call never counts, at any date.
    pub start_unix: Option<i64>,
    pub end_unix: Option<i64>,
}

impl InvalidOperation {
    /// Whether this entry covers `at_unix`.
    ///
    /// `None` is an unknown QSO time. An entry with no window at all still
    /// matches — the call never counts whenever it was worked — but a
    /// *windowed* entry does not, because discarding a contact we cannot
    /// place inside the window would be a guess against the operator.
    pub fn covers(&self, at_unix: Option<i64>) -> bool {
        if self.start_unix.is_none() && self.end_unix.is_none() {
            return true;
        }
        let Some(at) = at_unix else { return false };
        self.start_unix.is_none_or(|s| at >= s) && self.end_unix.is_none_or(|e| at <= e)
    }
}

#[derive(Debug, Default)]
pub struct CtyData {
    pub entities: HashMap<i32, DxccEntity>,
    pub prefix_rules: Vec<PrefixRule>,
    /// Several entries can share one callsign — a call flagged over more
    /// than one period appears once per period.
    pub invalid_operations: Vec<InvalidOperation>,
}

#[derive(Default)]
struct RecordTmp {
    adif: Option<i32>,
    name: Option<String>,
    prefix: Option<String>,
    cqz: Option<i32>,
    continent: Option<String>,
    call: Option<String>,
    deleted: bool,
    start: Option<i64>,
    end: Option<i64>,
    whitelist: bool,
    whitelist_start: Option<i64>,
}

/// Parse cty.xml content. Returns None only when the XML is unscannable;
/// individual malformed records are skipped like in Swift.
pub fn parse(content: &str) -> Option<CtyData> {
    let mut out = CtyData::default();
    let mut path: Vec<String> = Vec::new();
    let mut buffer = String::new();
    let mut tmp = RecordTmp::default();

    for event in Scanner::new(content) {
        match event {
            Event::Start(name) => {
                let lower = name.to_lowercase();
                path.push(lower.clone());
                buffer.clear();
                let parent = parent_of(&path);
                let is_top_record = (lower == "entity" && parent == "entities")
                    || (lower == "exception" && parent == "exceptions")
                    || (lower == "prefix" && parent == "prefixes")
                    || (lower == "invalid" && parent == "invalid_operations");
                if is_top_record {
                    tmp = RecordTmp::default();
                }
            }
            Event::Text(t) => buffer.push_str(&t),
            Event::End(name) => {
                let lower = name.to_lowercase();
                let value = buffer.trim().to_string();
                let parent = parent_of(&path);
                match lower.as_str() {
                    "adif" => tmp.adif = value.parse().ok(),
                    "name" => tmp.name = Some(value),
                    "prefix" => {
                        if parent == "entity" {
                            tmp.prefix = Some(value);
                        } else if parent == "prefixes"
                            && let (Some(adif), Some(call)) = (tmp.adif, tmp.call.as_ref())
                        {
                            out.prefix_rules.push(PrefixRule {
                                call: call.to_uppercase(),
                                adif,
                                is_exact: false,
                                start_unix: tmp.start,
                                end_unix: tmp.end,
                                cq_zone: tmp.cqz,
                            });
                        }
                    }
                    "call" => tmp.call = Some(value),
                    "cqz" => tmp.cqz = value.parse().ok(),
                    "cont" => tmp.continent = Some(value),
                    "deleted" => tmp.deleted = value.eq_ignore_ascii_case("true"),
                    "whitelist" => tmp.whitelist = value.eq_ignore_ascii_case("true"),
                    "whitelist_start" => tmp.whitelist_start = parse_iso8601(&value),
                    "start" => tmp.start = parse_iso8601(&value),
                    "end" => tmp.end = parse_iso8601(&value),
                    "entity" => {
                        if parent == "entities"
                            && let (Some(adif), Some(name)) = (tmp.adif, tmp.name.clone())
                        {
                            let prefix = tmp.prefix.clone().unwrap_or_default();
                            out.entities.insert(
                                adif,
                                DxccEntity {
                                    adif,
                                    name,
                                    prefix: prefix.clone(),
                                    cq_zone: tmp.cqz.unwrap_or(0),
                                    continent: tmp.continent.clone().unwrap_or_default(),
                                    whitelist: tmp.whitelist,
                                    whitelist_start_unix: tmp.whitelist_start,
                                    deleted: tmp.deleted,
                                },
                            );
                            if !tmp.deleted && !prefix.is_empty() {
                                let upper = prefix.to_uppercase();
                                let has_digit = upper.chars().any(|c| c.is_numeric());
                                let letters = upper.chars().filter(|c| c.is_alphabetic()).count();
                                out.prefix_rules.push(PrefixRule {
                                    call: upper,
                                    adif,
                                    is_exact: has_digit && letters >= 3,
                                    start_unix: tmp.start,
                                    end_unix: tmp.end,
                                    cq_zone: tmp.cqz,
                                });
                            }
                        }
                    }
                    // `<invalid>` records carry no `<adif>` — the call is
                    // enough, since the entry says "this contact scores
                    // nothing", not "it belongs elsewhere".
                    "invalid" => {
                        if parent == "invalid_operations"
                            && let Some(call) = tmp.call.as_ref()
                        {
                            out.invalid_operations.push(InvalidOperation {
                                call: call.to_uppercase(),
                                start_unix: tmp.start,
                                end_unix: tmp.end,
                            });
                        }
                    }
                    "exception" => {
                        if let (Some(adif), Some(call)) = (tmp.adif, tmp.call.as_ref()) {
                            out.prefix_rules.push(PrefixRule {
                                call: call.to_uppercase(),
                                adif,
                                is_exact: true,
                                start_unix: tmp.start,
                                end_unix: tmp.end,
                                cq_zone: tmp.cqz,
                            });
                        }
                    }
                    _ => {}
                }
                path.pop();
                buffer.clear();
            }
        }
    }
    Some(out)
}

fn parent_of(path: &[String]) -> &str {
    if path.len() >= 2 {
        &path[path.len() - 2]
    } else {
        ""
    }
}

// ---------------------------------------------------------------------------
// Minimal XML scanner — start/end/text events, entities decoded, attributes
// and prolog/comments/CDATA-free (cty.xml has none of the latter).
// ---------------------------------------------------------------------------

enum Event {
    Start(String),
    End(String),
    Text(String),
}

struct Scanner<'a> {
    rest: &'a str,
    /// Queued end event for a self-closing `<tag/>`.
    pending_end: Option<String>,
}

impl<'a> Scanner<'a> {
    fn new(content: &'a str) -> Self {
        Scanner {
            rest: content,
            pending_end: None,
        }
    }
}

impl Iterator for Scanner<'_> {
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        if let Some(name) = self.pending_end.take() {
            return Some(Event::End(name));
        }
        loop {
            if self.rest.is_empty() {
                return None;
            }
            if let Some(after) = self.rest.strip_prefix('<') {
                // Comment / prolog / doctype: skip to their terminator.
                if after.starts_with("!--") {
                    let end = self.rest.find("-->")?;
                    self.rest = &self.rest[end + 3..];
                    continue;
                }
                if after.starts_with('?') || after.starts_with('!') {
                    let end = self.rest.find('>')?;
                    self.rest = &self.rest[end + 1..];
                    continue;
                }
                let end = self.rest.find('>')?;
                let inner = &self.rest[1..end];
                self.rest = &self.rest[end + 1..];
                if let Some(name) = inner.strip_prefix('/') {
                    return Some(Event::End(name.trim().to_string()));
                }
                let self_closing = inner.ends_with('/');
                let inner = inner.strip_suffix('/').unwrap_or(inner);
                let name = inner
                    .split(|c: char| c.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .to_string();
                if self_closing {
                    self.pending_end = Some(name.clone());
                }
                return Some(Event::Start(name));
            }
            let next_lt = self.rest.find('<').unwrap_or(self.rest.len());
            let text = &self.rest[..next_lt];
            self.rest = &self.rest[next_lt..];
            if !text.trim().is_empty() {
                return Some(Event::Text(decode_entities(text)));
            }
        }
    }
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..semi];
        match ent {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let code = ent
                    .strip_prefix("#x")
                    .or_else(|| ent.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()));
                match code.and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => {
                        out.push('&');
                        rest = &rest[1..];
                        continue;
                    }
                }
            }
        }
        rest = &rest[semi + 1..];
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// ISO-8601 internet date-time → unix seconds ("2019-01-31T23:59:59+00:00"
// or trailing "Z"). Hand-rolled: fixed machine-generated format, no clock,
// no chrono/time dependency.
// ---------------------------------------------------------------------------

pub fn parse_iso8601(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> { s.get(r)?.parse().ok() };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let offset_secs = match &s[19..] {
        "" | "Z" | "z" => 0,
        tz => {
            let tzb = tz.as_bytes();
            if tzb.len() != 6 || (tzb[0] != b'+' && tzb[0] != b'-') || tzb[3] != b':' {
                return None;
            }
            let sign = if tzb[0] == b'-' { -1 } else { 1 };
            let th: i64 = tz.get(1..3)?.parse().ok()?;
            let tm: i64 = tz.get(4..6)?.parse().ok()?;
            sign * (th * 3600 + tm * 60)
        }
    };
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - offset_secs)
}

/// Days since 1970-01-01 (Howard Hinnant's civil-days algorithm).
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<clublog date="2026-08-24">
 <entities>
  <entity><adif>324</adif><name>INDIA</name><prefix>VU</prefix><deleted>false</deleted><cqz>22</cqz><cont>AS</cont></entity>
  <entity><adif>289</adif><name>UNITED NATIONS HQ</name><prefix>4U1UN</prefix><deleted>false</deleted><cqz>5</cqz><cont>NA</cont></entity>
  <entity><adif>222</adif><name>BOSNIA &amp; OLD</name><prefix>YO</prefix><deleted>true</deleted><cqz>28</cqz><cont>EU</cont></entity>
 </entities>
 <exceptions>
  <exception record="1"><call>VU2XX/OLD</call><entity>INDIA</entity><adif>324</adif><cqz>22</cqz><start>1990-01-01T00:00:00+00:00</start><end>1999-12-31T23:59:59+00:00</end></exception>
  <exception record="2"><call>4X6TU</call><entity>BEACON</entity><adif>0</adif></exception>
 </exceptions>
 <prefixes>
  <prefix record="1"><call>VU</call><entity>INDIA</entity><adif>324</adif><cqz>22</cqz><cont>AS</cont></prefix>
  <prefix record="2"><call>K</call><entity>UNITED STATES</entity><adif>291</adif></prefix>
 </prefixes>
 <invalid_operations>
  <invalid record="1"><call>SV2RSG/A</call><start>2024-09-05T00:00:00+00:00</start><end>2024-09-09T23:59:59+00:00</end></invalid>
  <invalid record="2"><call>SV2RSG/A</call><start>2021-12-09T00:00:00+00:00</start><end>2021-12-09T23:59:59+00:00</end></invalid>
  <invalid record="3"><call>HM0DX</call></invalid>
  <invalid record="4"><call>ST2NH</call><start>2023-04-16T00:00:00+00:00</start></invalid>
 </invalid_operations>
</clublog>"#;

    /// The section 1.x never read. Without it a QSO ClubLog refuses to
    /// credit is scored anyway, and an entity worked only through such an
    /// operation inflates the DXCC total by one.
    #[test]
    fn invalid_operations_are_parsed_with_their_windows() {
        let cty = parse(SAMPLE).unwrap();
        assert_eq!(cty.invalid_operations.len(), 4);

        // One call, two rejected periods — both are kept.
        let athos: Vec<_> = cty
            .invalid_operations
            .iter()
            .filter(|o| o.call == "SV2RSG/A")
            .collect();
        assert_eq!(athos.len(), 2);
        let sept = athos
            .iter()
            .find(|o| o.start_unix == parse_iso8601("2024-09-05T00:00:00+00:00"))
            .expect("the September 2024 window");
        assert_eq!(sept.end_unix, parse_iso8601("2024-09-09T23:59:59+00:00"));

        let unbounded = cty
            .invalid_operations
            .iter()
            .find(|o| o.call == "HM0DX")
            .unwrap();
        assert_eq!((unbounded.start_unix, unbounded.end_unix), (None, None));

        // Open-ended: invalid from a date onwards.
        let open = cty
            .invalid_operations
            .iter()
            .find(|o| o.call == "ST2NH")
            .unwrap();
        assert!(open.start_unix.is_some() && open.end_unix.is_none());

        // An `<invalid>` record carries no <adif>, so it must not leak a
        // prefix rule or an entity.
        assert!(!cty.prefix_rules.iter().any(|r| r.call == "HM0DX"));
    }

    #[test]
    fn invalid_operation_windows() {
        let day = |s: &str| parse_iso8601(s);
        let windowed = InvalidOperation {
            call: "SV2RSG/A".into(),
            start_unix: day("2024-09-05T00:00:00+00:00"),
            end_unix: day("2024-09-09T23:59:59+00:00"),
        };
        assert!(windowed.covers(day("2024-09-06T10:15:00+00:00")));
        assert!(!windowed.covers(day("2024-09-04T23:59:59+00:00")));
        assert!(!windowed.covers(day("2024-09-10T00:00:00+00:00")));
        assert!(!windowed.covers(None), "a windowed entry needs a date");

        let always = InvalidOperation {
            call: "HM0DX".into(),
            start_unix: None,
            end_unix: None,
        };
        assert!(always.covers(None), "no window, no date — still invalid");
        assert!(always.covers(day("1999-01-01T00:00:00+00:00")));

        let from = InvalidOperation {
            call: "ST2NH".into(),
            start_unix: day("2023-04-16T00:00:00+00:00"),
            end_unix: None,
        };
        assert!(from.covers(day("2026-01-01T00:00:00+00:00")));
        assert!(!from.covers(day("2020-01-01T00:00:00+00:00")));
    }

    /// cty.xml has always carried `<deleted>`; the parser read it only to
    /// decide whether to build a prefix rule, and then dropped it. Without
    /// it on the entity, nothing downstream can separate current DXCC from
    /// the ARRL deleted list.
    #[test]
    fn the_deleted_flag_survives_onto_the_entity() {
        let data = parse(SAMPLE).expect("sample parses");
        assert!(!data.entities[&324].deleted, "India is current");
        assert!(data.entities[&222].deleted, "the deleted one is marked");

        // Still true that a deleted entity contributes no prefix rule — a
        // dead entity has no live prefix to resolve a callsign against.
        assert!(
            !data.prefix_rules.iter().any(|r| r.adif == 222),
            "no prefix rule for a deleted entity"
        );
    }

    #[test]
    fn parses_entities_exceptions_prefixes() {
        let cty = parse(SAMPLE).unwrap();
        assert_eq!(cty.entities.len(), 3);
        assert_eq!(cty.entities[&324].name, "INDIA");
        assert_eq!(cty.entities[&222].name, "BOSNIA & OLD");

        // Rules: entity prefixes VU (prefix), 4U1UN (exact — digit + 3
        // letters), none for the deleted entity; 2 exceptions; 2 prefixes.
        let exacts: Vec<_> = cty.prefix_rules.iter().filter(|r| r.is_exact).collect();
        assert!(exacts.iter().any(|r| r.call == "4U1UN" && r.adif == 289));
        assert!(exacts.iter().any(|r| r.call == "4X6TU" && r.adif == 0));
        assert!(exacts.iter().any(|r| r.call == "VU2XX/OLD"));
        assert!(
            !cty.prefix_rules.iter().any(|r| r.call == "YO"),
            "deleted entity leaked a rule"
        );
        assert!(
            cty.prefix_rules
                .iter()
                .any(|r| r.call == "K" && !r.is_exact && r.adif == 291)
        );
    }

    #[test]
    fn dated_rules_respect_activity_window() {
        let cty = parse(SAMPLE).unwrap();
        let dated = cty
            .prefix_rules
            .iter()
            .find(|r| r.call == "VU2XX/OLD")
            .unwrap();
        let mid_1995 = parse_iso8601("1995-06-01T00:00:00+00:00").unwrap();
        let in_2020 = parse_iso8601("2020-01-01T00:00:00+00:00").unwrap();
        assert!(dated.is_active(mid_1995));
        assert!(!dated.is_active(in_2020));
    }

    #[test]
    fn iso8601_reference_values() {
        assert_eq!(parse_iso8601("1970-01-01T00:00:00+00:00"), Some(0));
        assert_eq!(parse_iso8601("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(
            parse_iso8601("2026-08-26T05:30:00+05:30"),
            parse_iso8601("2026-08-26T00:00:00+00:00")
        );
        assert_eq!(parse_iso8601("not a date"), None);
    }
}
