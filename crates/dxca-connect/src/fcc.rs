//! FCC amateur database → call→state table (`docs/AWARDS.md` phase 4).
//!
//! The weekly complete dump (`l_amat.zip`, ~200 MB) is the one public,
//! credential-free source for which US state a callsign is licensed in —
//! nothing on the wire carries a state, so WAS spotting has no other leg
//! to stand on. Two members matter: `HD.dat` (license status) and `EN.dat`
//! (call + address). The distilled table is ~8 MB of sorted `CALL ST`
//! lines that `dxca_core::awards::StateTable` binary-searches.
//!
//! Cost honesty: the download is ~200 MB and the distillation holds the
//! active-license set (~30 MB) plus the output map (~90 MB) transiently.
//! That is why the refresh defaults to **manual-only** in config — an
//! admin decides when a Pi does this, it never sneaks into a tick.
//!
//! The obvious limitation is inherited from the source: the FCC knows the
//! *license* address. A W6 living in Ohio spots as California. Loggers
//! that offer WAS spotting all share this, and the alert wording stays
//! honest by naming the state as a lookup, not a fact.

use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

pub const DEFAULT_URL: &str = "https://data.fcc.gov/download/pub/uls/complete/l_amat.zip";

/// Download the dump to `tmp_path`, distill, and clean up. Returns the
/// sorted table text and its entry count. Blocking for minutes — callers
/// run it on a blocking task.
pub fn download_and_distill(url: &str, tmp_path: &Path) -> Result<(String, usize), String> {
    let resp = ureq::get(url)
        // **data.fcc.gov answers 403 to `Accept-Encoding: gzip`** — the
        // header ureq adds on its own, because the gzip feature is on for
        // ClubLog's gzipped endpoints. Verified 2026-09-01 against the live
        // host: `gzip` and `identity` are both refused, every other value
        // (and no header at all) returns 200/206. It is a WAF rule, not
        // content negotiation, and it is why the first Pi download failed
        // with a bare 403.
        //
        // This value is both accepted and honest about what we want: a zip
        // is already compressed, so re-encoding it would cost CPU at both
        // ends for nothing.
        .set("Accept-Encoding", "identity;q=1, *;q=0")
        .timeout(std::time::Duration::from_secs(1800))
        .call()
        .map_err(|e| format!("FCC download: {e}"))?;
    let mut file = std::fs::File::create(tmp_path).map_err(|e| format!("FCC temp file: {e}"))?;
    std::io::copy(&mut resp.into_reader().take(1024 * 1024 * 1024), &mut file)
        .map_err(|e| format!("FCC download write: {e}"))?;
    drop(file);
    let result = distill_zip(tmp_path);
    let _ = std::fs::remove_file(tmp_path);
    result
}

/// Distill an on-disk `l_amat.zip`.
pub fn distill_zip(path: &Path) -> Result<(String, usize), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("FCC open: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("FCC unzip: {e}"))?;
    let active = {
        let hd = archive
            .by_name("HD.dat")
            .map_err(|e| format!("FCC HD.dat: {e}"))?;
        active_licenses(BufReader::new(hd))
    };
    if active.is_empty() {
        return Err("FCC HD.dat: no active licenses found — wrong file?".into());
    }
    let en = archive
        .by_name("EN.dat")
        .map_err(|e| format!("FCC EN.dat: {e}"))?;
    let (table, count) = distill(BufReader::new(en), &active);
    if count < 100_000 {
        return Err(format!(
            "FCC distill: only {count} calls — refusing a suspiciously small table"
        ));
    }
    Ok((table, count))
}

/// `HD.dat`: `HD|usi|…|call|license_status|…` — the unique system
/// identifiers whose status is `A` (active). Everything else — expired,
/// cancelled, terminated — is left out, so a reassigned call can never
/// answer with its previous holder's state.
pub fn active_licenses(hd: impl BufRead) -> HashSet<u64> {
    let mut active = HashSet::new();
    for line in raw_lines(hd) {
        let mut f = line.split('|');
        let usi = f.nth(1).and_then(|s| s.parse::<u64>().ok());
        if let (Some(usi), Some("A")) = (usi, f.nth(3)) {
            active.insert(usi);
        }
    }
    active
}

/// `EN.dat`: `EN|usi|…|call(4)|…|state(17)|…` — sorted `CALL ST` lines for
/// active licensees whose state survives `normalize_state` (the fifty, DC
/// folded to MD; territories out). Later rows for a call overwrite earlier
/// ones, matching the dump's own ordering.
pub fn distill(en: impl BufRead, active: &HashSet<u64>) -> (String, usize) {
    let mut table: BTreeMap<String, &'static str> = BTreeMap::new();
    for line in raw_lines(en) {
        let f: Vec<&str> = line.split('|').collect();
        if f.len() < 18 || f[0] != "EN" {
            continue;
        }
        if !f[1].parse::<u64>().is_ok_and(|usi| active.contains(&usi)) {
            continue;
        }
        let call = f[4].trim();
        if call.is_empty() {
            continue;
        }
        let Some(state) = dxca_core::awards::normalize_state(f[17]) else {
            continue;
        };
        table.insert(call.to_ascii_uppercase(), state);
    }
    let count = table.len();
    let mut out = String::with_capacity(count * 10);
    for (call, st) in table {
        out.push_str(&call);
        out.push(' ');
        out.push_str(st);
        out.push('\n');
    }
    (out, count)
}

/// Lines as lossy UTF-8 — FCC records occasionally carry bytes that are
/// not, and a licensee's odd name must not abort the whole table.
fn raw_lines(mut r: impl BufRead) -> impl Iterator<Item = String> {
    std::iter::from_fn(move || {
        let mut buf = Vec::new();
        match r.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(String::from_utf8_lossy(&buf).trim_end().to_string()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hd_filters_to_active_and_en_distills_sorted() {
        let hd = "HD|100|||W1AA|A|rest\nHD|101|||W2BB|E|rest\nHD|102|||K3CC|A|rest\n";
        let active = active_licenses(hd.as_bytes());
        assert_eq!(active, HashSet::from([100, 102]));

        // 30-field EN rows, state at index 17.
        let en_row = |usi: u64, call: &str, state: &str| {
            let mut f = vec![String::new(); 30];
            f[0] = "EN".into();
            f[1] = usi.to_string();
            f[4] = call.into();
            f[17] = state.into();
            f.join("|")
        };
        let en = [
            en_row(102, "K3CC", "PA"),
            en_row(100, "W1AA", "DC"),  // folds to MD
            en_row(101, "W2BB", "NY"),  // not active → out
            en_row(100, "KP4XX", "PR"), // territory → out
        ]
        .join("\n");
        let (table, count) = distill(en.as_bytes(), &active);
        assert_eq!(count, 2);
        assert_eq!(table, "K3CC PA\nW1AA MD\n", "sorted, DC folded");
        // Round-trips into the core lookup.
        let t = dxca_core::awards::StateTable::parse(table);
        assert_eq!(t.lookup("w1aa"), Some("MD"));
    }
}
