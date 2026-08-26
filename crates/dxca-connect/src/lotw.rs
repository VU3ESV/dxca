//! LoTW user-activity list — port of the Swift `LoTWDatabase` (M5 display
//! marker): download the ARRL CSV, keep a callsign set, and answer "is
//! this call a known LoTW uploader?" with the 1.x slash-tolerant lookup.

use std::collections::HashSet;
use std::io::Read;

pub const DEFAULT_URL: &str = "https://lotw.arrl.org/lotw-user-activity.csv";

/// Download the users list (any http(s) URL; the 1.x default is ARRL's).
pub fn download(url: &str) -> Result<String, String> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .map_err(|e| format!("LoTW download: {e}"))?;
    let mut out = String::new();
    resp.into_reader()
        .take(64 * 1024 * 1024)
        .read_to_string(&mut out)
        .map_err(|e| format!("LoTW read: {e}"))?;
    Ok(out)
}

/// 1.x `parseUsers`: first comma/tab/space token per line, uppercased,
/// callsign-shaped (≥3 chars, a digit and a letter).
pub fn parse_users(text: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    for raw in text.split(['\n', '\r']) {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(token) = line.split([',', '\t', ' ']).find(|t| !t.is_empty()) else {
            continue;
        };
        let call = token.to_uppercase();
        if call.chars().count() >= 3
            && call.chars().any(|c| c.is_numeric())
            && call.chars().any(|c| c.is_alphabetic())
        {
            set.insert(call);
        }
    }
    set
}

/// 1.x `isUser`: exact, bare-before-slash, and after-slash (prefix
/// overrides like VP8/K1JT) lookups.
pub fn is_user(users: &HashSet<String>, callsign: &str) -> bool {
    if users.is_empty() {
        return false;
    }
    let upper = callsign.to_uppercase();
    if users.contains(&upper) {
        return true;
    }
    if let Some((bare, suffix)) = upper.split_once('/') {
        if users.contains(bare) {
            return true;
        }
        if users.contains(suffix) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_lookup() {
        let users =
            parse_users("# header\nK1JT,2026-08-01,12:00:00\nVU2CPL\t2026-08-02\nbad\n73,x\n");
        assert_eq!(users.len(), 2);
        assert!(is_user(&users, "k1jt"));
        assert!(is_user(&users, "K1JT/4"));
        assert!(is_user(&users, "VP8/VU2CPL"));
        assert!(!is_user(&users, "N0CALL"));
    }
}
