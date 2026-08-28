//! The aggregated spot — port of the Swift `SpotMessage` (minus the SwiftUI
//! display fields). Carries one decode (or a cluster spot converted to a
//! synthetic decode, 1.x-style) through dedupe, the telnet feed, UDP
//! broadcast, and classification.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spot {
    /// Unix seconds: today's UTC date carrying the decode's time-of-day
    /// (see [`time_from_decode_ms`]), or the receive time for cluster spots.
    pub time_unix: i64,
    pub snr_db: i32,
    pub delta_time_s: f64,
    pub delta_frequency_hz: u32,
    /// Raw mode exactly as the decoder sent it — 1.x passes Decode.mode
    /// through unmapped (WSJT-X uses mode characters like "~" for FT8),
    /// and the DATA bucket in the classifier absorbs whatever it is.
    pub mode: String,
    /// True when `mode` was **guessed from the frequency** rather than
    /// reported by the decoder or the spot comment.
    ///
    /// Cluster nodes that relay human spots (DB0SUE, N2WQ) send free-text
    /// comments with no mode field, and an empty mode used to be bucketed as
    /// DATA by `modes::canonical` — a silent, and often wrong, guess. It is
    /// still a guess now, but a labelled one: the UI marks it and the API
    /// exposes it, so an operator can see which award slots rest on an
    /// assumption. A decoder-reported mode always wins over an inferred one.
    pub mode_inferred: bool,
    pub message: String,
    /// Does this spot report a station **calling CQ**?
    ///
    /// Stored rather than sniffed from `message`, because for a cluster spot
    /// the message is synthesised and cannot carry the answer. Every cluster
    /// spot used to be built as `CQ <call>`, so the CQ-only filter matched
    /// 100% of the feed and appeared to do nothing.
    ///
    /// Cluster spots take it from the parsed `SpotKind`, widened to count
    /// skimmer spots: a skimmer only reports stations calling CQ, so an
    /// unmarked skimmer spot is one even though its comment never says so.
    /// A human spot with no marker is somebody logging a station they heard
    /// or worked, which is not.
    pub is_cq: bool,
    /// The spotter's free-text comment, for cluster spots — what a human
    /// actually typed. Empty for decoder spots, whose `message` already IS
    /// the decoded text.
    pub comment: String,
    pub low_confidence: bool,
    pub off_air: bool,
    pub dial_frequency_hz: u64,
    /// Where DXCA got this spot: a decoder source ("MSHV") or the configured
    /// name of the cluster node that relayed it ("DB0SUE", "HamAlert").
    ///
    /// **Not the same as who spotted it** — see [`Spot::spotter`]. A node
    /// name answers "which of my feeds carried this"; on a relaying node
    /// like HamAlert or DB0SUE that says nothing about the station whose
    /// receiver actually heard the DX.
    pub source_name: String,
    /// The **spotting station** — the call after `DX de` on the cluster
    /// line, skimmer `-#` suffix already stripped by the parser.
    ///
    /// `None` for spots decoded here, where the local receiver is the
    /// spotter and `source_name` already names it. The parser has always
    /// extracted this; until 2026-08-28 `synthetic_spot` dropped it on the
    /// floor, so every relayed spot was attributed to the relaying node and
    /// the operator could not tell a W3LPL skimmer catch from a hand-typed
    /// spot two hops away.
    #[serde(default)]
    pub spotter: Option<String>,
}

impl Spot {
    pub fn frequency_hz(&self) -> u64 {
        self.dial_frequency_hz + u64::from(self.delta_frequency_hz)
    }

    pub fn frequency_khz(&self) -> f64 {
        self.frequency_hz() as f64 / 1_000.0
    }

    pub fn frequency_mhz(&self) -> f64 {
        self.frequency_hz() as f64 / 1_000_000.0
    }

    // `is_cq` used to be derived here from the message text. It is a stored
    // field now — see the doc comment on it — because a synthesised cluster
    // message can only ever answer "yes".

    /// "HHmm" in UTC, for the cluster line.
    pub fn hhmm(&self) -> String {
        let secs = self.time_unix.rem_euclid(86_400);
        format!("{:02}{:02}", secs / 3600, (secs % 3600) / 60)
    }

    /// The spotted (DX) callsign extracted from the FT8/FT4 message text —
    /// Swift `SpotMessage.dxCallsign` verbatim. Handles "CQ CALL GRID",
    /// directed "CQ NA CALL GRID", two-station exchanges (prefer the
    /// transmitting station in slot 1), and `<hashed>` calls.
    pub fn dx_callsign(&self) -> Option<String> {
        let parts: Vec<&str> = self.message.split(' ').filter(|p| !p.is_empty()).collect();
        if parts.len() < 2 {
            return None;
        }

        if parts[0].eq_ignore_ascii_case("CQ") {
            if parts.len() >= 3 && !looks_like_callsign(parts[1]) {
                return looks_like_callsign(parts[2]).then(|| strip_call_decoration(parts[2]));
            }
            return looks_like_callsign(parts[1]).then(|| strip_call_decoration(parts[1]));
        }

        if looks_like_callsign(parts[1]) {
            return Some(strip_call_decoration(parts[1]));
        }
        if looks_like_callsign(parts[0]) {
            return Some(strip_call_decoration(parts[0]));
        }
        None
    }

    /// Dedupe key: CALL-BAND-MODE (the 60-second-window key used both for
    /// display collapse and rebroadcast dedupe in 1.x). None when no
    /// callsign can be extracted — such spots always pass.
    pub fn duplicate_key(&self) -> Option<String> {
        let call = self.dx_callsign()?.to_uppercase();
        let band = crate::bands::band_from_hz(self.frequency_hz()).unwrap_or("");
        Some(format!("{call}-{band}-{}", self.mode.to_uppercase()))
    }
}

/// Does a decoded message announce a CQ? Only meaningful for real decoder
/// text — a synthesised cluster message can only ever say yes.
pub fn message_is_cq(message: &str) -> bool {
    message.to_uppercase().starts_with("CQ ")
}

/// Map a WSJT-X decode time (ms since midnight UTC) onto today's UTC date —
/// the Swift `timeFromMilliseconds` without a hidden clock: `now_unix`
/// supplies "today".
pub fn time_from_decode_ms(now_unix: i64, decode_ms: u32) -> i64 {
    let midnight = now_unix - now_unix.rem_euclid(86_400);
    midnight + i64::from(decode_ms / 1000)
}

/// Strip the `<>` brackets WSJT-X puts around hashed/known callsigns.
fn strip_call_decoration(s: &str) -> String {
    s.trim_start_matches('<').trim_end_matches('>').to_string()
}

/// Reject FT8 tokens that aren't callsigns — RR73/73/TU…, signal reports
/// (R+05, -12), 4-char Maidenhead grids (LL85), placeholders. Swift
/// `looksLikeCallsign` verbatim.
fn looks_like_callsign(s: &str) -> bool {
    let upper = s.to_uppercase();
    let core = upper.trim_start_matches('<').trim_end_matches('>');
    if core.is_empty() || core == "..." {
        return false;
    }
    let len = core.chars().count();
    if !(3..=11).contains(&len) {
        return false;
    }

    const BLACKLIST: [&str; 9] = ["RR73", "RRR", "73", "TU", "TNX", "QSL", "DE", "TEST", "CQ"];
    if BLACKLIST.contains(&core) {
        return false;
    }

    // Signal reports: R+05, R-12, +05, -12.
    if core.starts_with("R+") || core.starts_with("R-") {
        return false;
    }
    if (core.starts_with('+') || core.starts_with('-'))
        && core.chars().skip(1).all(|c| c.is_numeric())
    {
        return false;
    }

    // 4-char Maidenhead grid: 2 letters + 2 digits.
    if len == 4 {
        let c: Vec<char> = core.chars().collect();
        if c[0].is_alphabetic() && c[1].is_alphabetic() && c[2].is_numeric() && c[3].is_numeric() {
            return false;
        }
    }

    let has_digit = core.chars().any(|c| c.is_numeric());
    let has_letter = core.chars().any(|c| c.is_alphabetic());
    if !has_digit || !has_letter {
        return false;
    }

    core.chars()
        .all(|c| c.is_alphabetic() || c.is_numeric() || c == '/')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(message: &str) -> Spot {
        Spot {
            time_unix: 1_787_745_000,
            snr_db: -12,
            delta_time_s: 0.2,
            delta_frequency_hz: 1487,
            mode: "FT8".into(),
            mode_inferred: false,
            message: message.into(),
            is_cq: true,
            comment: String::new(),
            low_confidence: false,
            off_air: false,
            dial_frequency_hz: 14_074_000,
            source_name: "JTDX".into(),
            spotter: None,
        }
    }

    #[test]
    fn dx_call_extraction_table() {
        // (message, expected dx callsign)
        let cases = [
            ("CQ P5DX PM95", Some("P5DX")),
            ("CQ NA K1JT FN20", Some("K1JT")), // directed CQ
            ("CQ DX VU2CPL MK83", Some("VU2CPL")),
            ("K1JT VU2CPL -15", Some("VU2CPL")), // exchange: prefer slot 1
            ("VU2CPL K1JT RR73", Some("K1JT")),
            ("K1JT RR73", Some("K1JT")), // slot 1 is decoration → fall back to slot 0
            ("<K1JT> VU2CPL R-07", Some("VU2CPL")),
            ("VU2CPL <K1JT> +03", Some("K1JT")), // hashed call unwraps
            ("K1JT LL85", Some("K1JT")),         // slot 1 is a grid → fall back to slot 0
            ("CQ TEST", None),
            ("73", None),
        ];
        for (msg, want) in cases {
            assert_eq!(spot(msg).dx_callsign().as_deref(), want, "message: {msg:?}");
        }
    }

    #[test]
    fn cq_and_keys() {
        let s = spot("CQ P5DX PM95");
        assert!(s.is_cq);
        assert_eq!(s.duplicate_key().as_deref(), Some("P5DX-20M-FT8"));
        assert_eq!(spot("73").duplicate_key(), None);
        assert_eq!(s.frequency_hz(), 14_075_487);
    }

    #[test]
    fn decode_time_maps_onto_today() {
        // 2026-08-27 ~13:10 UTC; decode said 05:31:30 (19,890,000 ms).
        let t = time_from_decode_ms(1_787_836_200, 19_890_000);
        assert_eq!(t.rem_euclid(86_400), 5 * 3600 + 31 * 60 + 30);
        let s = Spot {
            time_unix: t,
            ..spot("CQ P5DX PM95")
        };
        assert_eq!(s.hhmm(), "0531");
    }
}
