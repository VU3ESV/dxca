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

    /// The state for a call — exact, then bare-before-slash, then
    /// after-slash, the same ladder `lotw::is_user` and
    /// `LogMatrix::has_worked_call` walk.
    pub fn lookup(&self, callsign: &str) -> Option<&str> {
        let upper = callsign.trim().to_ascii_uppercase();
        if let Some(st) = self.lookup_exact(&upper) {
            return Some(st);
        }
        let (bare, suffix) = upper.split_once('/')?;
        self.lookup_exact(bare)
            .or_else(|| self.lookup_exact(suffix))
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

    #[test]
    fn state_table_lookup_walks_the_slash_ladder() {
        // Deliberately unsorted input — parse must sort its own index.
        let t = StateTable::parse("W1AW CT\nK6XYZ CA\nAA0A SD\n".into());
        assert_eq!(t.len(), 3);
        assert_eq!(t.lookup("w1aw"), Some("CT"));
        assert_eq!(t.lookup("W1AW/M"), Some("CT"), "suffix stripped");
        assert_eq!(t.lookup("KH6/K6XYZ"), Some("CA"), "prefix override");
        assert_eq!(t.lookup("G4ABC"), None);
        assert!(StateTable::parse(String::new()).is_empty());
    }
}
