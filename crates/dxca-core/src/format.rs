//! DX-cluster announcement line — port of the Swift `ClusterFormatter`.
//! Mirrors the de-facto DX-Spider layout every cluster client tokenises:
//!
//! ```text
//! DX de MSHV:      14074.0  K1JT          FT8 -10 dB             1428Z
//! ```
//!
//! Parsers tokenise by whitespace runs, but the uppercase `Z`, the
//! unpadded frequency, and the padded cells are all deliberate — RUMlog
//! regex-matches `\d{4}Z$` and mis-parsed drifting columns in early 1.x.

use crate::spot::Spot;

pub fn format(spot: &Spot) -> String {
    let dx_call = spot
        .dx_callsign()
        .unwrap_or_else(|| "UNKNOWN".into())
        .to_uppercase();

    // The spotter must be a SINGLE token: source names like "MSHV 2237"
    // would tokenise as two fields and shove the freq into the call slot.
    let cleaned: String = spot
        .source_name
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '/' || *c == '-')
        .collect();
    let raw = if cleaned.is_empty() {
        "NOCALL".to_string()
    } else {
        cleaned
    };
    let spotter: String = raw.chars().take(13).collect(); // DX-Spider spotter limit

    let freq_str = format!("{:.1}", spot.frequency_khz());
    let comment = format!("{} {} dB", spot.mode, spot.snr_db);

    format!(
        "DX de {} {} {}{} {}Z",
        pad_or_trunc(&format!("{spotter}:"), 14),
        pad_or_trunc(&freq_str, 9),
        pad_or_trunc(&dx_call, 14),
        pad_or_trunc(&comment, 28),
        spot.hhmm()
    )
}

/// Swift `padding(toLength:)`: pad with spaces to `len` — or truncate.
fn pad_or_trunc(s: &str, len: usize) -> String {
    let mut out: String = s.chars().take(len).collect();
    while out.chars().count() < len {
        out.push(' ');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spot::Spot;

    #[test]
    fn spider_layout() {
        let s = Spot {
            time_unix: 14 * 3600 + 28 * 60, // 1428Z
            snr_db: -10,
            delta_time_s: 0.0,
            delta_frequency_hz: 0,
            mode: "FT8".into(),
            mode_inferred: false,
            message: "CQ K1JT FN20".into(),
            is_cq: true,
            comment: String::new(),
            low_confidence: false,
            off_air: false,
            dial_frequency_hz: 14_074_000,
            source_name: "MSHV 2333".into(),
            spotter: None,
            is_skimmer: false, // space must be stripped
            grid: None,
            iota: None,
        };
        let line = format(&s);
        assert_eq!(
            line,
            "DX de MSHV2333:      14074.0   K1JT          FT8 -10 dB                   1428Z"
        );
        assert!(line.ends_with("Z"));
        // The spotter is one token.
        assert_eq!(line.split_whitespace().nth(2), Some("MSHV2333:"));
    }

    #[test]
    fn no_callsign_becomes_unknown() {
        let s = Spot {
            time_unix: 0,
            snr_db: 3,
            delta_time_s: 0.0,
            delta_frequency_hz: 0,
            mode: "FT4".into(),
            mode_inferred: false,
            message: "73".into(),
            is_cq: true,
            comment: String::new(),
            low_confidence: false,
            off_air: false,
            dial_frequency_hz: 7_047_500,
            source_name: "JTDX".into(),
            spotter: None,
            is_skimmer: false,
            grid: None,
            iota: None,
        };
        assert!(format(&s).contains("UNKNOWN"));
    }
}

#[cfg(test)]
mod source_name_reaches_the_wire {
    use super::*;
    use crate::Spot;

    /// `source_name` IS the spotter callsign on the cluster line, so whatever
    /// is put in it goes out to every connected logger.
    ///
    /// Worth a test because the failure is silent. This function filters the
    /// name to `[A-Z0-9/-]`, so a value carrying punctuation is not rejected
    /// and not truncated — the punctuation is simply dropped, and the result
    /// still looks like a plausible callsign. A namespaced `VU2CPL:MSHV`
    /// becomes `DX de VU2CPLMSHV:`, which nobody would read as a bug.
    #[test]
    fn punctuation_in_a_source_name_is_silently_welded_not_rejected() {
        let mut s = Spot {
            time_unix: 0,
            snr_db: -10,
            delta_time_s: 0.0,
            delta_frequency_hz: 0,
            mode: "FT8".into(),
            mode_inferred: false,
            message: "CQ K1JT".into(),
            is_cq: true,
            comment: String::new(),
            low_confidence: false,
            off_air: false,
            dial_frequency_hz: 14_074_000,
            source_name: "MSHV".into(),
            spotter: None,
            is_skimmer: false,
            grid: None,
            iota: None,
        };
        assert!(format(&s).contains("MSHV:"), "the ordinary case");

        s.source_name = "VU2CPL:MSHV".into();
        let welded = format(&s);
        assert!(welded.contains("VU2CPLMSHV:"), "{welded}");
        assert!(
            !welded.contains("VU2CPL:MSHV"),
            "and nothing here would tell you it happened"
        );
    }
}
