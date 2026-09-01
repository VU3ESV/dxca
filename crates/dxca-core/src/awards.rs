//! Award reference data beyond DXCC — the pure halves of `docs/AWARDS.md`
//! phases 2–4: IOTA reference extraction, the WAS state vocabulary, and the
//! call→state lookup table distilled from the FCC amateur database.
//!
//! Downloading and distilling live in `dxca-connect` (this crate does no
//! I/O); what belongs here is everything a classifier or a matrix build
//! needs at spot time.

/// The fifty WAS states, postal codes, alphabetical. **DC is not here** —
/// ARRL WAS rule 6 counts District of Columbia contacts for Maryland, which
/// [`normalize_state`] applies before this list is consulted.
#[rustfmt::skip]
pub const US_STATES: [&str; 50] = [
    "AK", "AL", "AR", "AZ", "CA", "CO", "CT", "DE", "FL", "GA",
    "HI", "IA", "ID", "IL", "IN", "KS", "KY", "LA", "MA", "MD",
    "ME", "MI", "MN", "MO", "MS", "MT", "NC", "ND", "NE", "NH",
    "NJ", "NM", "NV", "NY", "OH", "OK", "OR", "PA", "RI", "SC",
    "SD", "TN", "TX", "UT", "VA", "VT", "WA", "WI", "WV", "WY",
];

/// The DXCC entities whose contacts can carry a WAS state: the lower 48
/// (291), **Alaska** (6) and **Hawaii** (110). All three are separate DXCC
/// entities but all three are WAS states, so a `KL7`/`KH6` contact counts
/// here even though it is not "the USA" to DXCC.
///
/// This list is what stops a US licensee operating abroad being credited
/// with their home state: `DV2/K7AZQ` resolves to the Philippines, so no
/// state is looked up at all — see [`counts_for_was`].
pub const WAS_DXCC: [i32; 3] = [291, 6, 110];

/// Can a contact with this DXCC entity carry a WAS state?
pub fn counts_for_was(dxcc: i32) -> bool {
    WAS_DXCC.contains(&dxcc)
}

/// Suffixes that say "same licensee, still somewhere in their own country"
/// — the only ones [`StateTable::lookup`] will look past.
///
/// A **numeric** suffix is deliberately excluded: `W1AW/7` is a Connecticut
/// licensee announcing they are in call area 7, which is the one thing that
/// makes their licence address the wrong answer.
const PLAIN_MODIFIERS: [&str; 3] = ["P", "M", "QRP"];

/// The CQ zone a US state sits in — 3 in the west, 4 through the middle,
/// 5 on the eastern seaboard, with Alaska 1 and Hawaii 31.
///
/// **This exists because cty.xml cannot answer it.** ClubLog's prefix
/// records carry a `<cqz>` for Canada (VE7 is 3, VE3 is 4) and for Russia,
/// but there are **no US call-area records at all**, so the resolver has
/// only the entity's own zone and answers 5 for the entire country. For a
/// zone award that is not a rounding error: zones 3 and 4 would never be
/// credited, and a third of the map would be unreachable.
///
/// The FCC table DXCA already loads gives the state, and the state gives
/// the zone exactly. Same caveat as WAS: it is the *licence* address.
pub fn us_zone(state: &str) -> Option<i32> {
    const Z3: [&str; 9] = ["AZ", "CA", "ID", "MT", "NV", "OR", "UT", "WA", "WY"];
    const Z4: [&str; 22] = [
        "AL", "AR", "CO", "IA", "IL", "IN", "KS", "KY", "LA", "MI", "MN", "MO", "MS", "ND", "NE",
        "NM", "OH", "OK", "SD", "TN", "TX", "WI",
    ];
    let st = normalize_state(state)?;
    match st {
        "AK" => Some(1),
        "HI" => Some(31),
        s if Z3.contains(&s) => Some(3),
        s if Z4.contains(&s) => Some(4),
        // The remaining seventeen are the eastern seaboard.
        _ => Some(5),
    }
}

/// A WAS-countable state code from a raw two-letter value: uppercased,
/// DC folded into MD (WAS rule 6), everything else validated against
/// [`US_STATES`]. `None` for territories (PR, GU, VI…) and noise.
pub fn normalize_state(raw: &str) -> Option<&'static str> {
    let up = raw.trim().to_ascii_uppercase();
    let code = if up == "DC" { "MD" } else { up.as_str() };
    US_STATES.iter().find(|s| **s == code).copied()
}

/// An IOTA reference from one token — `AS-003`, case-insensitive, digits
/// zero-padded to the directory's three (`as-3` → `AS-003`). `None` when the
/// token is not continent-dash-number shaped.
pub fn normalize_iota(token: &str) -> Option<String> {
    let (cont, num) = token.split_once('-')?;
    let cont = cont.to_ascii_uppercase();
    if !matches!(
        cont.as_str(),
        "AF" | "AN" | "AS" | "EU" | "NA" | "OC" | "SA"
    ) {
        return None;
    }
    if num.is_empty() || num.len() > 3 || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(format!("{cont}-{:03}", num.parse::<u32>().ok()?))
}

/// The first IOTA reference in free text — how a cluster spot's comment
/// ("CQ IOTA as-153" / "EU-005 SOTA…") yields the island group being
/// activated. Token-based rather than regex so `NA-1234` (too long) and
/// `DATA-15` (not a continent) stay out.
pub fn find_iota_ref(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '(' || c == ')')
        .find_map(normalize_iota)
}

/// Call → US state, over the file `dxca-connect::fcc` distills from the FCC
/// amateur database: one `CALL ST` per line, sorted by call, ~8 MB for the
/// full 800k-license table.
///
/// Held as the raw text plus a line index and binary-searched, deliberately:
/// a `HashMap<String, String>` of the same table costs ~90 MB of small
/// allocations on the Pi, this costs the file. Lookups are per *alert
/// candidate*, not per spot, so a search beats a hash by nothing anyone
/// can measure.
pub struct StateTable {
    data: String,
    /// Byte offset of each line, ordered by the call it starts with.
    lines: Vec<u32>,
}

impl StateTable {
    /// Build from distilled text. Lines that don't parse are skipped; the
    /// index is re-sorted rather than trusted, so a hand-edited file still
    /// looks up correctly.
    pub fn parse(data: String) -> StateTable {
        let mut lines: Vec<u32> = Vec::new();
        let bytes = data.as_bytes();
        let mut start = 0usize;
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'\n' {
                if i > start {
                    lines.push(start as u32);
                }
                start = i + 1;
            }
        }
        if start < bytes.len() {
            lines.push(start as u32);
        }
        let mut t = StateTable { data, lines };
        let d = std::mem::take(&mut t.data);
        t.lines
            .sort_by(|a, b| line_call(&d, *a).cmp(line_call(&d, *b)));
        t.data = d;
        t
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The state for a call.
    ///
    /// **Deliberately NOT the slash ladder `lotw::is_user` and
    /// `LogMatrix::has_worked_call` walk**, and the difference is the whole
    /// point: those answer *who* a call belongs to, where finding `K7AZQ`
    /// inside `DV2/K7AZQ` is exactly right. This answers *where the
    /// operator is*, where it is exactly wrong — `DV2/K7AZQ` is an Arizonan
    /// transmitting from the Philippines, and crediting Arizona put a false
    /// New State on the screen (reported 2026-09-01).
    ///
    /// So: an exact match, or a base call carrying nothing but a plain
    /// operating modifier ([`PLAIN_MODIFIERS`]). Anything else with a slash
    /// — a prefix override, a suffix override, a call-area digit — means the
    /// licence address is not where they are, and the honest answer is none.
    pub fn lookup(&self, callsign: &str) -> Option<&str> {
        let upper = callsign.trim().to_ascii_uppercase();
        if let Some(st) = self.lookup_exact(&upper) {
            return Some(st);
        }
        let (base, suffix) = upper.split_once('/')?;
        if !PLAIN_MODIFIERS.contains(&suffix) {
            return None;
        }
        self.lookup_exact(base)
    }

    fn lookup_exact(&self, call: &str) -> Option<&str> {
        let i = self
            .lines
            .binary_search_by(|off| line_call(&self.data, *off).cmp(call))
            .ok()?;
        let off = self.lines[i] as usize;
        let line = self.data[off..].split('\n').next()?;
        let st = line.split_once(' ')?.1.trim();
        (!st.is_empty()).then_some(st)
    }
}

fn line_call(data: &str, off: u32) -> &str {
    let rest = &data[off as usize..];
    let end = rest.find([' ', '\n']).unwrap_or(rest.len());
    &rest[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_lands_in_a_real_cq_zone() {
        // The three continental zones plus the two that are their own.
        assert_eq!(us_zone("CA"), Some(3));
        assert_eq!(us_zone("WA"), Some(3));
        assert_eq!(us_zone("WY"), Some(3), "Wyoming is west, not middle");
        assert_eq!(us_zone("TX"), Some(4));
        assert_eq!(us_zone("OH"), Some(4), "Ohio is 4, not the seaboard");
        assert_eq!(us_zone("MI"), Some(4));
        assert_eq!(us_zone("NY"), Some(5));
        assert_eq!(us_zone("FL"), Some(5));
        assert_eq!(us_zone("GA"), Some(5), "Georgia is 5, unlike Alabama");
        assert_eq!(us_zone("AL"), Some(4));
        assert_eq!(us_zone("AK"), Some(1));
        assert_eq!(us_zone("HI"), Some(31));
        assert_eq!(us_zone("DC"), Some(5), "folds to MD first");
        assert_eq!(us_zone("PR"), None, "not a WAS state, not a US zone here");

        // Every one of the fifty must land somewhere, or a state would be
        // silently unreachable for WAZ.
        for st in US_STATES {
            assert!(us_zone(st).is_some(), "{st} has no zone");
        }
    }

    #[test]
    fn state_normalization() {
        assert_eq!(normalize_state("oh"), Some("OH"));
        assert_eq!(normalize_state(" TX "), Some("TX"));
        assert_eq!(normalize_state("DC"), Some("MD"), "WAS rule 6");
        assert_eq!(normalize_state("PR"), None, "territories don't count");
        assert_eq!(normalize_state("GU"), None);
        assert_eq!(normalize_state(""), None);
        assert_eq!(normalize_state("Ohio"), None);
    }

    #[test]
    fn iota_refs_normalize() {
        assert_eq!(normalize_iota("AS-003").as_deref(), Some("AS-003"));
        assert_eq!(normalize_iota("as-3").as_deref(), Some("AS-003"));
        assert_eq!(normalize_iota("eu-005").as_deref(), Some("EU-005"));
        assert_eq!(normalize_iota("XX-003"), None, "not a continent");
        assert_eq!(normalize_iota("NA-1234"), None, "too many digits");
        assert_eq!(normalize_iota("DATA-15"), None);
        assert_eq!(normalize_iota("AS-"), None);
        assert_eq!(normalize_iota("AS003"), None);
    }

    #[test]
    fn iota_ref_found_in_comment_text() {
        assert_eq!(
            find_iota_ref("up 2 IOTA as-153 tnx").as_deref(),
            Some("AS-153")
        );
        assert_eq!(
            find_iota_ref("EU-005, SOTA G/LD-001").as_deref(),
            Some("EU-005")
        );
        assert_eq!(find_iota_ref("FT8 -12 dB"), None);
        // A CW speed token must not read as a reference.
        assert_eq!(find_iota_ref("25 WPM CQ"), None);
    }

    /// A licence address answers "where is this station" only while the
    /// station is at it. Every case below is one where it is not — and the
    /// `KH6/` line is the one this test used to assert BACKWARDS, which is
    /// how `DV2/K7AZQ` reached the screen as a New State.
    #[test]
    fn a_call_operating_away_from_its_licence_has_no_state() {
        // Deliberately unsorted input — parse must sort its own index.
        let t = StateTable::parse("W1AW CT\nK6XYZ CA\nK7AZQ AZ\nAA0A SD\n".into());
        assert_eq!(t.len(), 4);
        assert_eq!(t.lookup("w1aw"), Some("CT"));
        assert_eq!(t.lookup("K7AZQ"), Some("AZ"));

        // Plain operating modifiers: same licensee, still home.
        assert_eq!(t.lookup("W1AW/M"), Some("CT"));
        assert_eq!(t.lookup("K7AZQ/P"), Some("AZ"));
        assert_eq!(t.lookup("K7AZQ/QRP"), Some("AZ"));

        // The reported case: an Arizonan transmitting from the Philippines.
        assert_eq!(t.lookup("DV2/K7AZQ"), None, "prefix override is a PLACE");
        // Same shape, US territory in front — still not the home state.
        assert_eq!(t.lookup("KH6/K6XYZ"), None);
        // Suffix override, and a call-area digit: both say "not at home".
        assert_eq!(t.lookup("K6XYZ/DU2"), None);
        assert_eq!(t.lookup("W1AW/7"), None, "call area 7 is not Connecticut");

        assert_eq!(t.lookup("G4ABC"), None);
        assert!(StateTable::parse(String::new()).is_empty());
    }

    #[test]
    fn was_counts_the_fifty_states_across_three_dxcc_entities() {
        assert!(counts_for_was(291), "lower 48");
        assert!(counts_for_was(6), "Alaska is a WAS state");
        assert!(counts_for_was(110), "Hawaii is a WAS state");
        assert!(!counts_for_was(375), "Philippines");
        assert!(!counts_for_was(103), "Guam is not a WAS state");
    }
}
