//! Full-chain parity against the 1.x macOS app's own artifacts — the
//! strongest golden test we have: parse the real ClubLog cty.xml and
//! log.adi from the app's cache, rebuild the matrix exactly the way
//! `ClubLogClient` does, and compare against the matrix.json the Swift
//! app computed from the same inputs.
//!
//! `#[ignore]`d because it needs the 1.x app's cache on this machine
//! (personal log data — not committed to the repo). Run with:
//!     cargo test -p dxca-core --test local_parity -- --ignored

use dxca_core::{cty, dxcc::DxccResolver, matrix::LogMatrix};
use std::path::PathBuf;

fn cache_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap())
        .join("Library/Application Support/DXClusterAggregator")
}

#[test]
#[ignore = "needs the 1.x app's local cache (cty.xml, log.adi, matrix.json)"]
fn matrix_matches_swift_apps_own_build() {
    let dir = cache_dir();
    let cty_xml = std::fs::read_to_string(dir.join("cty.xml")).expect("cty.xml");
    let log_bytes = std::fs::read(dir.join("log.adi")).expect("log.adi");
    let swift_matrix: LogMatrix =
        serde_json::from_slice(&std::fs::read(dir.join("matrix.json")).expect("matrix.json"))
            .expect("matrix.json deserializes into the ported LogMatrix");

    // content decode: UTF-8 first, Latin-1 fallback (ClubLogClient order).
    let content = match String::from_utf8(log_bytes.clone()) {
        Ok(s) => s,
        Err(_) => log_bytes.iter().map(|&b| b as char).collect(),
    };

    let data = cty::parse(&cty_xml).expect("cty.xml parses");
    println!(
        "cty.xml: {} entities, {} rules",
        data.entities.len(),
        data.prefix_rules.len()
    );

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut resolver = DxccResolver::default();
    resolver.load(data.entities, &data.prefix_rules, now_unix);

    // Rebuild the matrix through the production builder (the exact
    // ClubLogClient loop, empty band filter).
    let (ours, qso_count) = LogMatrix::build_from_adif(&content, &resolver);
    println!("log.adi: {qso_count} records");

    // Compare entity sets first for a readable failure.
    let mut ours_ids: Vec<_> = ours.by_dxcc.keys().copied().collect();
    let mut swift_ids: Vec<_> = swift_matrix.by_dxcc.keys().copied().collect();
    ours_ids.sort_unstable();
    swift_ids.sort_unstable();
    assert_eq!(ours_ids, swift_ids, "DXCC entity sets differ");

    for id in &ours_ids {
        assert_eq!(
            ours.by_dxcc[id], swift_matrix.by_dxcc[id],
            "status differs for DXCC {id}"
        );
    }
    assert_eq!(
        ours.worked_calls, swift_matrix.worked_calls,
        "workedCalls sets differ"
    );
    println!(
        "parity OK: {} DXCC entities, {} worked calls",
        ours.total_dxcc_count(),
        ours.worked_calls.len()
    );
}
