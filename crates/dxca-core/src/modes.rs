//! ADIF mode string → award bucket ("CW" / "PHONE" / "DATA") — port of the
//! Swift `ModeNormalizer`. FT8, FT4, RTTY, JT65 etc. all collapse to DATA
//! so digital modes share one DXCC slot, matching DXCC/LoTW/ClubLog rules.

/// The three award buckets, in the order the UI shows them. Every value
/// `canonical` can return appears here — the test below is what keeps that
/// true if a fourth bucket is ever added.
pub const CLASSES: &[&str] = &["CW", "PHONE", "DATA"];

/// Settle a spot's mode: what the decoder or the spot comment reported wins;
/// otherwise fall back to inferring it from the frequency. Returns the mode
/// and whether it was inferred.
///
/// Exists because cluster nodes relaying human spots (DB0SUE, N2WQ) send
/// comments with no mode field at all. Before this, such a spot reached
/// `canonical("")` and was bucketed as **DATA** — an unmarked guess that
/// credited phone spots to digital award slots. An empty return means the
/// frequency was in no segment worth guessing about, and callers must treat
/// that as *unknown*, not as DATA.
pub fn resolve(reported: &str, freq_mhz: f64) -> (String, bool) {
    let reported = reported.trim();
    if !reported.is_empty() {
        return (reported.to_string(), false);
    }
    match crate::bands::mode_from_mhz(freq_mhz) {
        Some(m) => (m.to_string(), true),
        None => (String::new(), false),
    }
}

/// Award bucket for an ADIF mode string.
///
/// **Empty input returns DATA**, which is right for the ADIF path it was
/// written for (a logged QSO always carries a mode; a blank one is a
/// malformed record) but wrong for a spot whose mode is simply unknown.
/// Spot callers use [`canonical_opt`] so unknown stays unknown.
pub fn canonical(raw: &str) -> &'static str {
    let mode = raw.trim().to_ascii_uppercase();
    if mode.is_empty() {
        return "DATA";
    }
    match mode.as_str() {
        "CW" => "CW",
        "SSB" | "USB" | "LSB" | "AM" | "FM" | "PHONE" | "VOICE" | "DIGITALVOICE" | "C4FM"
        | "DMR" | "DSTAR" => "PHONE",
        _ => "DATA",
    }
}

/// Award bucket, or `None` when the mode is unknown. The spot path uses this
/// so an unknown mode is never silently scored as DATA.
pub fn canonical_opt(raw: &str) -> Option<&'static str> {
    if raw.trim().is_empty() {
        return None;
    }
    Some(canonical(raw))
}

#[cfg(test)]
mod tests {
    use super::canonical;

    #[test]
    fn buckets() {
        assert_eq!(canonical("CW"), "CW");
        assert_eq!(canonical("ssb"), "PHONE");
        assert_eq!(canonical("FT8"), "DATA");
        assert_eq!(canonical("FT4"), "DATA");
        assert_eq!(canonical("RTTY"), "DATA");
        assert_eq!(canonical(" ft8 "), "DATA");
        assert_eq!(canonical(""), "DATA");
    }

    #[test]
    fn classes_cover_everything_canonical_can_return() {
        use super::CLASSES;
        for raw in [
            "CW",
            "SSB",
            "USB",
            "LSB",
            "AM",
            "FM",
            "PHONE",
            "VOICE",
            "DIGITALVOICE",
            "C4FM",
            "DMR",
            "DSTAR",
            "FT8",
            "FT4",
            "RTTY",
            "JT65",
            "MSK144",
            "",
            "nonsense",
        ] {
            let c = canonical(raw);
            assert!(CLASSES.contains(&c), "{raw} → {c}, which CLASSES omits");
        }
    }
}
