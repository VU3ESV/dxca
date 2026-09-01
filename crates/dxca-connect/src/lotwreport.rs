//! LoTW QSL report download — the operator's **own confirmations**, which
//! is a different thing from `lotw.rs`'s public users list. `docs/AWARDS.md`
//! phase 3: the QSL detail records carry `STATE`, `GRIDSQUARE` and `IOTA`,
//! the three fields ClubLog's export never does (verified 2026-09-01), so
//! this is the confirmed side of WAS, VUCC and IOTA.
//!
//! Always a **full** report (`qso_qsl=yes`, no since-date): the matrix is
//! rebuilt from scratch on every ClubLog refresh, so an incremental pull
//! would lose every older LoTW-only confirmation on the next rebuild. The
//! server caches the report on disk and re-downloads on its own cadence
//! precisely so this fullness does not turn into a daily hammering of ARRL.

use std::io::Read;

pub const DEFAULT_BASE: &str = "https://lotw.arrl.org/lotwuser/lotwreport.adi";

/// Download the full QSL report for one LoTW account (blocking; minutes on
/// a large log — callers run it on a blocking task).
pub fn download(base: &str, login: &str, password: &str) -> Result<String, String> {
    let resp = ureq::get(base)
        .query("login", login)
        .query("password", password)
        .query("qso_query", "1")
        .query("qso_qsl", "yes")
        .query("qso_qsldetail", "yes")
        .timeout(std::time::Duration::from_secs(600))
        .call()
        .map_err(|e| format!("LoTW report download: {e}"))?;
    let mut out = String::new();
    resp.into_reader()
        .take(256 * 1024 * 1024)
        .read_to_string(&mut out)
        .map_err(|e| format!("LoTW report read: {e}"))?;
    validate(&out)?;
    Ok(out)
}

/// LoTW answers a bad login with an HTML page and HTTP 200, so the status
/// code proves nothing — the body has to look like ADIF.
fn validate(body: &str) -> Result<(), String> {
    let head: String = body.chars().take(2048).collect::<String>().to_lowercase();
    if head.contains("<html") || head.contains("<!doctype") {
        return Err("LoTW rejected the login — check the LoTW username and password".into());
    }
    if !head.contains("<eoh>") && !head.contains("<app_lotw") {
        return Err("LoTW report: response is not ADIF".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_and_junk_are_refused_adif_passes() {
        assert!(validate("<HTML><body>password incorrect").is_err());
        assert!(validate("<!DOCTYPE html><p>login</p>").is_err());
        assert!(validate("plain text that is not a report").is_err());
        assert!(validate("<APP_LoTW_LASTQSL:19>2026-08-30 00:00:00\n<eoh>\n").is_ok());
        assert!(validate("header\n<eoh>\n<CALL:4>W8AA<eor>").is_ok());
    }
}
