//! Golden tests over real datagrams captured from the live shack decoders
//! (MSHV, JTDX, WSJT-X — 2026-08-26, see `tests/vectors/summary.json`).
//!
//! Two guarantees, per M1's exit criterion (docs/PLAN.md §10):
//!  - every captured Status/Decode/other datagram parses;
//!  - Decodes re-encode **byte-identically** from their parsed fields, and
//!    a Status re-encodes to a prefix of the original (real emitters append
//!    schema-dependent trailing fields past the point the permissive parser
//!    — like the Swift one it ports — reads), byte-for-byte except that a
//!    null QString on the wire (`FFFFFFFF`, used by WSJT-X for unset
//!    fields like dxCall) may correspond to the empty string we re-encode
//!    as length 0 — the parser deliberately collapses null to "" (Swift
//!    parity), so the distinction is not representable after a parse.

use dxca_core::wsjtx::{self, Message};
use std::fs;
use std::path::PathBuf;

fn vectors(decoder: &str) -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors")
        .join(decoder);
    let mut out: Vec<(String, Vec<u8>)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("vector dir {} missing: {e}", dir.display()))
        .map(|entry| {
            let p = entry.unwrap().path();
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read(&p).unwrap(),
            )
        })
        .filter(|(name, _)| name.ends_with(".bin"))
        .collect();
    out.sort();
    assert!(!out.is_empty(), "no vectors in {}", dir.display());
    out
}

/// Walk the Status wire layout in both buffers in lockstep and require
/// byte equality, except that where the re-encode has an empty string
/// (`00000000`) the original may instead carry a null one (`FFFFFFFF`).
/// The layout here mirrors `encode_status` exactly: header, then
/// str, u64, str×4, bool×3, u32×2, str×3.
fn prefix_matches_modulo_null_strings(original: &[u8], reencoded: &[u8]) -> bool {
    const STR: u8 = 0;
    const FIXED_HEADER: usize = 12; // magic + schema + type
    // Field plan after the header: strings interleaved with fixed-width runs.
    let plan: &[(u8, usize)] = &[
        (STR, 0),
        (1, 8), // dial frequency
        (STR, 0),
        (STR, 0),
        (STR, 0),
        (STR, 0),
        (1, 3 + 4 + 4), // three bools + rxDF + txDF
        (STR, 0),
        (STR, 0),
        (STR, 0),
    ];
    let (mut o, mut r) = (FIXED_HEADER, FIXED_HEADER);
    if original.get(..o) != reencoded.get(..r) {
        return false;
    }
    for &(kind, width) in plan {
        if kind == STR {
            let (Some(olen), Some(rlen)) = (read_u32(original, o), read_u32(reencoded, r)) else {
                return false;
            };
            o += 4;
            r += 4;
            if olen == 0xFFFF_FFFF && rlen == 0 {
                continue; // null on the wire, parsed and re-encoded as ""
            }
            if olen != rlen {
                return false;
            }
            let n = olen as usize;
            if original.get(o..o + n) != reencoded.get(r..r + n) {
                return false;
            }
            o += n;
            r += n;
        } else {
            if original.get(o..o + width) != reencoded.get(r..r + width) {
                return false;
            }
            o += width;
            r += width;
        }
    }
    r == reencoded.len() // the plan must account for every re-encoded byte
}

fn read_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().unwrap()))
}

fn check_decoder(decoder: &str) {
    let mut statuses = 0;
    let mut decodes = 0;
    for (name, bytes) in vectors(decoder) {
        let parsed =
            wsjtx::parse(&bytes).unwrap_or_else(|| panic!("{decoder}/{name}: failed to parse"));
        match &parsed.message {
            Message::Decode(d) => {
                decodes += 1;
                let re = wsjtx::encode_decode(parsed.schema, d);
                assert_eq!(
                    re, bytes,
                    "{decoder}/{name}: Decode re-encode not byte-identical"
                );
            }
            Message::Status(s) => {
                statuses += 1;
                let re = wsjtx::encode_status(parsed.schema, s);
                assert!(
                    prefix_matches_modulo_null_strings(&bytes, &re),
                    "{decoder}/{name}: Status re-encode diverges from the original beyond \
                     null-vs-empty strings (re-encoded {} bytes, original {})",
                    re.len(),
                    bytes.len()
                );
            }
            Message::Other(_) => {}
        }
    }
    // The capture must actually exercise the two message types the
    // aggregator lives on — a vector set without them proves nothing.
    assert!(decodes > 0, "{decoder}: no Decode vectors captured");
    assert!(statuses > 0, "{decoder}: no Status vectors captured");
}

#[test]
fn mshv_vectors() {
    check_decoder("mshv");
}

#[test]
fn jtdx_vectors() {
    check_decoder("jtdx");
}

#[test]
fn wsjtx_vectors() {
    check_decoder("wsjtx");
}
