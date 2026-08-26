//! Wire formats and parsers for the cluster client — **lifted from
//! meridian-core/src/dxcluster/wire.rs** (plan §6). Client-side pieces
//! only: the server-dialect grammar, banner/format_spot emitters, and
//! SETT machinery stayed upstream. Divergences carry `// DXCA:` markers;
//! everything else is verbatim so improvements can flow both ways.

use super::{ClusterSpot, SpotKind};

/// Literal mode tokens recognized in spot comments.
const MODE_TOKENS: [&str; 10] = [
    "CW", "RTTY", "FT8", "FT4", "WSPR", "PSK31", "PSK63", "JT65", "JT9", "MSK144",
];

/// Maidenhead shape: two field letters `A–R`, two digits, optionally two
/// subsquare letters `A–X`.
pub fn looks_like_grid(t: &str) -> bool {
    let b = t.as_bytes();
    if b.len() != 4 && b.len() != 6 {
        return false;
    }
    let in_range = |c: u8, hi: u8| {
        let c = c.to_ascii_uppercase();
        c.is_ascii_uppercase() && c <= hi
    };
    in_range(b[0], b'R')
        && in_range(b[1], b'R')
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
        && (b.len() == 4 || (in_range(b[4], b'X') && in_range(b[5], b'X')))
}

/// An inbound `DX de` line, parsed tolerantly. Skimmer fields (`snr_db`,
/// `wpm`, `mode`) are extracted opportunistically when the comment has them.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedSpot {
    /// Spotting station, `-#` skimmer suffix stripped.
    pub spotter: String,
    pub spotter_is_skimmer: bool,
    /// Frequency in kHz.
    pub freq_khz: f64,
    /// The spotted DX callsign.
    pub call: String,
    /// Free-text comment (whitespace-normalized).
    pub comment: String,
    /// Spot time of day, UTC (the wire carries no date).
    pub hour: u8,
    pub minute: u8,
    pub snr_db: Option<i32>,
    /// WPM for CW; baud for RTTY (`BPS` token).
    pub wpm: Option<u32>,
    /// Mode token when identifiable.
    pub mode: Option<String>,
    pub kind: SpotKind,
    /// The **spotted station's** locator (comment's last token when
    /// grid-shaped).
    pub grid: Option<String>,
    /// The **spotting station's** own grid (after-time slot, `set/dxgrid`).
    pub spotter_grid: Option<String>,
}

/// Parse one `DX de …` line. Returns `None` when the line isn't a
/// well-formed spot (callers fall back to raw passthrough).
pub fn parse_spot_line(line: &str) -> Option<ParsedSpot> {
    let rest = line.trim().strip_prefix("DX de ")?;
    let mut toks = rest.split_whitespace();

    let spotter_tok = toks.next()?.trim_end_matches(':');
    let (spotter, spotter_is_skimmer) = match spotter_tok.strip_suffix("-#") {
        Some(s) => (s.to_string(), true),
        None => (spotter_tok.to_string(), false),
    };

    let freq_khz: f64 = toks.next()?.parse().ok()?;
    let call = toks.next()?.to_string();
    let remaining: Vec<&str> = toks.collect();

    // The rightmost `HHMMZ` token is the time; anything after it (a grid) is
    // a node extra, anything before it is the comment.
    let tpos = remaining.iter().rposition(|t| {
        t.len() == 5
            && t.ends_with('Z')
            && t[..4].chars().all(|c| c.is_ascii_digit())
            && t[..2].parse::<u8>().map(|h| h < 24).unwrap_or(false)
            && t[2..4].parse::<u8>().map(|m| m < 60).unwrap_or(false)
    })?;
    let hour: u8 = remaining[tpos][..2].parse().ok()?;
    let minute: u8 = remaining[tpos][2..4].parse().ok()?;
    let spotter_grid = remaining
        .get(tpos + 1)
        .filter(|t| looks_like_grid(t))
        .map(|t| t.to_string());

    let comment_toks = &remaining[..tpos];
    let grid = comment_toks
        .last()
        .filter(|t| looks_like_grid(t))
        .map(|t| t.to_string());
    let mut snr_db = None;
    let mut wpm = None;
    let mut mode: Option<String> = None;
    for (i, tok) in comment_toks.iter().enumerate() {
        match *tok {
            "dB" if i > 0 => snr_db = snr_db.or_else(|| comment_toks[i - 1].parse().ok()),
            "WPM" if i > 0 => {
                wpm = wpm.or_else(|| comment_toks[i - 1].parse().ok());
                mode.get_or_insert_with(|| "CW".to_string());
            }
            "BPS" if i > 0 => {
                wpm = wpm.or_else(|| comment_toks[i - 1].parse().ok());
                mode.get_or_insert_with(|| "RTTY".to_string());
            }
            t if MODE_TOKENS.contains(&t) => {
                mode.get_or_insert_with(|| t.to_string());
            }
            _ => {}
        }
    }
    Some(ParsedSpot {
        spotter,
        spotter_is_skimmer,
        freq_khz,
        call,
        comment: comment_toks.join(" "),
        hour,
        minute,
        snr_db,
        wpm,
        mode,
        kind: comment_kind(comment_toks),
        grid,
        spotter_grid,
    })
}

/// The transmission kind a spot comment reports (type token, last-before-grid
/// first, then unambiguous markers anywhere; `DX` last-token-only).
fn comment_kind(toks: &[&str]) -> SpotKind {
    fn of(tok: &str) -> Option<SpotKind> {
        match tok {
            "CQ" => Some(SpotKind::Cq),
            "DX" => Some(SpotKind::Dx),
            "BCN" | "BEACON" => Some(SpotKind::Bcn),
            "NCDXF" => Some(SpotKind::Ncdxf),
            _ => None,
        }
    }
    let typed = match toks.last() {
        Some(t) if looks_like_grid(t) => &toks[..toks.len() - 1],
        _ => toks,
    };
    if let Some(kind) = typed.last().copied().and_then(of) {
        return kind;
    }
    typed
        .iter()
        .filter(|t| **t != "DX")
        .find_map(|t| of(t))
        .unwrap_or(SpotKind::Unknown)
}

/// What kind of line an inbound cluster stream just produced.
#[derive(Clone, Debug, PartialEq)]
pub enum LineClass {
    Spot(ParsedSpot),
    /// WWV / WCY solar-propagation report.
    Wwv,
    /// `To ALL de …` announcement.
    Announce,
    /// A node prompt (`… de NODE … >`) — command-completion marker.
    Prompt,
    Other,
}

/// Classify one inbound line (client side).
pub fn classify_line(line: &str) -> LineClass {
    let t = line.trim();
    if let Some(p) = parse_spot_line(t) {
        return LineClass::Spot(p);
    }
    let upper = t.to_uppercase();
    if upper.starts_with("WWV DE ") || upper.starts_with("WCY DE ") {
        return LineClass::Wwv;
    }
    if upper.starts_with("TO ALL DE ") || upper.starts_with("TO LOCAL DE ") {
        return LineClass::Announce;
    }
    if t.ends_with('>') && upper.contains(" DE ") {
        return LineClass::Prompt;
    }
    LineClass::Other
}

/// The user-level spot submission our client sends to a node:
/// `dx <freq-kHz> <call> <remarks>` (documented DXSpider grammar).
pub fn dx_command(spot: &ClusterSpot) -> String {
    let kind = spot
        .kind
        .token()
        .map(|t| format!(" {t}"))
        .unwrap_or_default();
    let grid = spot
        .grid
        .as_deref()
        .filter(|g| looks_like_grid(g))
        .map(|g| format!(" {g}"))
        .unwrap_or_default();
    let remarks = match spot.mode.as_str() {
        "CW" => format!("{} dB {} WPM{kind}{grid}", spot.snr_db, spot.wpm),
        "RTTY" => format!("{} dB {} BPS{kind}{grid}", spot.snr_db, spot.wpm),
        _ => format!("{} dB {}{kind}{grid}", spot.snr_db, spot.mode),
    };
    format!("dx {:.1} {} {}\r\n", spot.freq_khz, spot.call, remarks)
}

// DXCA: Telnet IAC stripping, ported from the Swift client (1.x). AR-Cluster
// forks (N2WQ) prefix their banner with IAC option negotiation; the bytes are
// not UTF-8 and would poison prompt matching. We never respond to the
// negotiation — clusters accept silent partners — we just drop the bytes.
// Trailing partial IAC sequences at a chunk boundary are dropped (1.x caveat:
// clusters emit the whole preamble in one segment; revisit if one doesn't).
pub fn strip_telnet_iac(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b == 0xFF {
            let Some(&next) = data.get(i + 1) else { break };
            match next {
                0xFF => {
                    out.push(0xFF);
                    i += 2;
                }
                0xFB..=0xFE => i += 3, // WILL/WONT/DO/DONT + option byte
                0xFA => {
                    // SB ... IAC SE (variable subnegotiation)
                    let mut j = i + 2;
                    while j + 1 < data.len() {
                        if data[j] == 0xFF && data[j + 1] == 0xF0 {
                            j += 2;
                            break;
                        }
                        j += 1;
                    }
                    i = j;
                }
                _ => i += 2, // other 2-byte IAC commands (NOP, GA, …)
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_spider_line() {
        let p = parse_spot_line(
            "DX de W3LPL:     14074.0  K1JT           FT8 -10 dB                  1428Z",
        )
        .unwrap();
        assert_eq!(p.spotter, "W3LPL");
        assert_eq!(p.freq_khz, 14074.0);
        assert_eq!(p.call, "K1JT");
        assert_eq!(p.snr_db, Some(-10));
        assert_eq!(p.mode.as_deref(), Some("FT8"));
        assert_eq!((p.hour, p.minute), (14, 28));
    }

    #[test]
    fn skimmer_suffix_and_grids() {
        let p = parse_spot_line(
            "DX de K1ABC-#:   14025.3  W9XYZ          12 dB  22 WPM  CQ  FN20    1423Z EM12",
        )
        .unwrap();
        assert!(p.spotter_is_skimmer);
        assert_eq!(p.spotter, "K1ABC");
        assert_eq!(p.mode.as_deref(), Some("CW"));
        assert_eq!(p.grid.as_deref(), Some("FN20"));
        assert_eq!(p.spotter_grid.as_deref(), Some("EM12"));
        assert_eq!(p.kind, SpotKind::Cq);
    }

    #[test]
    fn classification() {
        assert!(matches!(
            classify_line("WWV de VE7CC <18>: SFI=150"),
            LineClass::Wwv
        ));
        assert!(matches!(
            classify_line("To ALL de W6BT: hi"),
            LineClass::Announce
        ));
        assert!(matches!(
            classify_line("W6BT de GB7DJK 11-Jul-2026 0923Z dxspider >"),
            LineClass::Prompt
        ));
        assert!(matches!(
            classify_line("random banner text"),
            LineClass::Other
        ));
    }

    #[test]
    fn iac_stripping() {
        // IAC WILL SUPPRESS-GA, IAC WILL ECHO, then "login: "
        let mut data = vec![0xFF, 0xFB, 0x03, 0xFF, 0xFB, 0x01];
        data.extend_from_slice(b"login: ");
        assert_eq!(strip_telnet_iac(&data), b"login: ");
        // IAC IAC escapes a literal 0xFF; trailing lone IAC drops.
        assert_eq!(
            strip_telnet_iac(&[0x41, 0xFF, 0xFF, 0x42, 0xFF]),
            &[0x41, 0xFF, 0x42]
        );
    }
}
