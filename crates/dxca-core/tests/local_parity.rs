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

    let mut data = cty::parse(&cty_xml).expect("cty.xml parses");
    println!(
        "cty.xml: {} entities, {} rules, {} invalid operations",
        data.entities.len(),
        data.prefix_rules.len(),
        data.invalid_operations.len()
    );

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // The invalid-operation list and the entity whitelist are **deliberate
    // divergences** from 1.x, which read neither. Parity is therefore
    // asserted against a resolver loaded without them; the two tests below
    // cover what each actually does to this same log.
    let invalid_ops = std::mem::take(&mut data.invalid_operations);
    let whitelisted = data.entities.values().filter(|e| e.whitelist).count();
    for e in data.entities.values_mut() {
        e.whitelist = false;
    }
    let mut resolver = DxccResolver::default();
    resolver.load(data, now_unix);
    assert_eq!(resolver.invalid_operation_count(), 0);
    assert_eq!(resolver.whitelisted_entity_count(), 0);

    // Rebuild the matrix through the production builder (the exact
    // ClubLogClient loop, empty band filter).
    let (ours, qso_count) = LogMatrix::build_from_adif(&content, &resolver);
    println!("log.adi: {qso_count} records");
    println!(
        "(cty.xml carried {} invalid operations, {whitelisted} whitelisted entities)",
        invalid_ops.len()
    );

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

/// The other half of the parity story: what ClubLog's invalid-operation list
/// does to this same real log.
///
/// It cannot assert a fixed number — the answer depends on whose log is
/// cached here and on the day's cty.xml — but it can assert the *shape* of
/// the change, which is what would have caught the original bug: dropping
/// invalid contacts may only ever remove worked slots, never add any, and it
/// must leave the QSO count alone.
#[test]
#[ignore = "needs the 1.x app's local cache (cty.xml, log.adi)"]
fn invalid_operations_change_the_matrix_in_one_direction_only() {
    let dir = cache_dir();
    let cty_xml = std::fs::read_to_string(dir.join("cty.xml")).expect("cty.xml");
    let log_bytes = std::fs::read(dir.join("log.adi")).expect("log.adi");
    let content = match String::from_utf8(log_bytes.clone()) {
        Ok(s) => s,
        Err(_) => log_bytes.iter().map(|&b| b as char).collect(),
    };
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let build = |keep_invalid: bool| {
        let mut data = cty::parse(&cty_xml).expect("cty.xml parses");
        if !keep_invalid {
            data.invalid_operations.clear();
        }
        let mut r = DxccResolver::default();
        r.load(data, now_unix);
        let (m, n) = LogMatrix::build_from_adif(&content, &r);
        (m, n, r.invalid_operation_count())
    };

    let (before, qsos_before, _) = build(false);
    let (after, qsos_after, listed) = build(true);
    assert!(listed > 0, "cty.xml should carry an invalid-operation list");

    assert_eq!(
        qsos_before, qsos_after,
        "the QSO count is every record in the file — it must not move"
    );

    // Entities may only be lost.
    let gained: Vec<_> = after
        .by_dxcc
        .keys()
        .filter(|k| !before.by_dxcc.contains_key(k))
        .collect();
    assert!(gained.is_empty(), "invalid ops must never add entities");

    for (adif, status) in &after.by_dxcc {
        let was = &before.by_dxcc[adif];
        assert!(
            status.slots.is_subset(&was.slots),
            "DXCC {adif} gained slots from the invalid list"
        );
    }
    assert!(after.worked_calls.is_subset(&before.worked_calls));

    let dropped: Vec<_> = before
        .by_dxcc
        .keys()
        .filter(|k| !after.by_dxcc.contains_key(k))
        .collect();
    // Slots are the sensitive measure: a log can keep every entity and
    // every callsign (both worked again on a valid date) while still losing
    // the band-mode slots the rejected contacts were the only source of.
    // If nothing at all moved here, the skip never fired and the whole fix
    // is inert against real data.
    println!(
        "{listed} invalid operations listed; entities {} -> {} (dropped: {dropped:?}), \
         worked calls {} -> {}, slots {} -> {}",
        before.total_dxcc_count(),
        after.total_dxcc_count(),
        before.worked_calls.len(),
        after.worked_calls.len(),
        before.stats().slots_worked,
        after.stats().slots_worked,
    );
    assert!(
        after.stats().slots_worked <= before.stats().slots_worked,
        "dropping contacts cannot add slots"
    );

    // And the operator-facing report for this same real log — the exact
    // lines `refresh_user` prints. Empty is the expected and healthy answer.
    let data = cty::parse(&cty_xml).expect("cty.xml parses");
    let mut r = DxccResolver::default();
    r.load(data, now_unix);
    let (_, _, uncredited) = LogMatrix::build_from_adif_reporting(&content, &r);
    println!("uncredited contacts in this log: {}", uncredited.len());
    for c in &uncredited {
        println!("   {c}");
    }
}

/// Does the **real** cty.xml actually flag a known invalid operation, end to
/// end, through the production loader? Isolates a code fault from a data one
/// when a log's totals fail to move.
#[test]
#[ignore = "needs the 1.x app's local cache (cty.xml)"]
fn the_real_cty_flags_a_known_invalid_operation() {
    let cty_xml = std::fs::read_to_string(cache_dir().join("cty.xml")).expect("cty.xml");
    let data = cty::parse(&cty_xml).expect("cty.xml parses");
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    for op in data
        .invalid_operations
        .iter()
        .filter(|o| o.call.starts_with("SV2RSG"))
    {
        println!(
            "listed: {} {:?} -> {:?}",
            op.call, op.start_unix, op.end_unix
        );
    }

    let mut r = DxccResolver::default();
    r.load(data, now_unix);
    println!("{} invalid operations loaded", r.invalid_operation_count());

    let at = |s: &str| cty::parse_iso8601(s);
    // Inside the September 2024 window.
    assert!(r.is_invalid_operation("SV2RSG/A", at("2024-09-06T10:15:00+00:00")));
    // Outside every window — the same call, a year later.
    assert!(!r.is_invalid_operation("SV2RSG/A", at("2025-09-06T10:15:00+00:00")));
}

/// The whitelist, against the real cty.xml and the real callsigns that
/// exposed it. VU24DX's `ZL8AC` is the one contact that made DXCA read 314
/// where ClubLog read 313.
#[test]
#[ignore = "needs the 1.x app's local cache (cty.xml)"]
fn the_real_cty_rejects_an_unlisted_call_in_a_whitelisted_entity() {
    let cty_xml = std::fs::read_to_string(cache_dir().join("cty.xml")).expect("cty.xml");
    let data = cty::parse(&cty_xml).expect("cty.xml parses");
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut r = DxccResolver::default();
    r.load(data, now_unix);
    println!("{} whitelisted entities", r.whitelisted_entity_count());

    let at = |s: &str| cty::parse_iso8601(s);
    let recent = at("2024-06-01T12:00:00+00:00");

    // 133 Kermadec: ZL8AC is nowhere in cty.xml.
    assert!(r.is_whitelist_rejected(133, "ZL8AC", recent), "ZL8AC");
    // ...but ZL8X inside its 2010 window is accepted, expired rule or not.
    assert!(!r.is_whitelist_rejected(133, "ZL8X", at("2010-11-25T00:00:00+00:00")));
    // and outside it, rejected — the call is reissued.
    assert!(r.is_whitelist_rejected(133, "ZL8X", recent));

    // The three that check out, and must keep checking out.
    assert!(
        !r.is_whitelist_rejected(180, "SV2RSG/A", recent),
        "Mount Athos"
    );
    // T33T inside the 2022 DXpedition window (05–25 Nov), not the 1990 one.
    assert!(!r.is_whitelist_rejected(490, "T33T", at("2022-11-10T00:00:00+00:00")));
    assert!(r.is_whitelist_rejected(490, "T33T", at("2022-11-01T00:00:00+00:00")));
    assert!(!r.is_whitelist_rejected(489, "3D2CCC", at("2024-05-01T00:00:00+00:00")));

    // Not a whitelisted entity: any call goes.
    assert!(!r.is_whitelist_rejected(324, "VU2CPL", recent), "India");
    // Unknown QSO time is never rejected.
    assert!(!r.is_whitelist_rejected(133, "ZL8AC", None));
}
