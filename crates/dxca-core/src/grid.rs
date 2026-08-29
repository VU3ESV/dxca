//! Maidenhead locator → coordinates.
//!
//! Milestone 1 of [`docs/PHASE-ROTATION-MASK.md`]: the mask needs to know
//! where the operator is before it can work out what the sun is doing there.
//! `dxcluster::wire::looks_like_grid` has always *validated* a locator;
//! nothing converted one.
//!
//! Returns the **centre** of the square rather than its corner. A 4-character
//! square is 2° of longitude by 1° of latitude — about 150 km by 110 km at
//! the equator — and the corner is up to half that away from wherever the
//! operator actually is. For sun elevation the difference is a few minutes of
//! sunrise time either way, which is well inside what this feature cares
//! about, but the centre is free and the corner is a needless bias.

/// Decoded position: degrees, north and east positive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

/// Parse a 4- or 6-character Maidenhead locator to the centre of its square.
///
/// `None` for anything malformed — the caller treats an unparseable locator
/// as "no locator", which disables the mask rather than guessing a position.
///
/// 8-character (extended-square) locators are rejected rather than truncated:
/// they are rare, and silently dropping precision an operator deliberately
/// typed is worse than saying no.
pub fn parse(locator: &str) -> Option<LatLon> {
    let s = locator.trim().as_bytes();
    if s.len() != 4 && s.len() != 6 {
        return None;
    }

    // Field: A–R, 20° of longitude by 10° of latitude.
    let field_lon = letter(s[0], b'R')?;
    let field_lat = letter(s[1], b'R')?;
    // Square: 0–9, 2° by 1°.
    let sq_lon = digit(s[2])?;
    let sq_lat = digit(s[3])?;

    let mut lon = -180.0 + field_lon * 20.0 + sq_lon * 2.0;
    let mut lat = -90.0 + field_lat * 10.0 + sq_lat;

    if s.len() == 6 {
        // Subsquare: a–x, 5 minutes of longitude by 2.5 of latitude.
        let sub_lon = letter(s[4], b'X')?;
        let sub_lat = letter(s[5], b'X')?;
        lon += sub_lon * (2.0 / 24.0);
        lat += sub_lat * (1.0 / 24.0);
        // Centre of the subsquare.
        lon += 1.0 / 24.0;
        lat += 1.0 / 48.0;
    } else {
        // Centre of the square.
        lon += 1.0;
        lat += 0.5;
    }

    Some(LatLon { lat, lon })
}

/// A–`max` as 0-based, case-insensitive.
fn letter(b: u8, max: u8) -> Option<f64> {
    let c = b.to_ascii_uppercase();
    (c.is_ascii_uppercase() && c <= max).then(|| f64::from(c - b'A'))
}

fn digit(b: u8) -> Option<f64> {
    b.is_ascii_digit().then(|| f64::from(b - b'0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64, tol: f64, what: &str) {
        assert!((a - b).abs() < tol, "{what}: {a} vs {b} (tol {tol})");
    }

    /// Published reference squares. JN58TD is the canonical Maidenhead
    /// example (Munich); the others pin the corners of the scheme.
    #[test]
    fn known_locators_decode_to_their_published_positions() {
        let munich = parse("JN58TD").expect("JN58TD parses");
        near(munich.lat, 48.1458, 0.001, "JN58TD latitude");
        near(munich.lon, 11.625, 0.001, "JN58TD longitude");

        // MK82 — Bengaluru. (MK68 is a different square entirely, at
        // 18.5N 73.0E: worth pinning both, because transposing the square
        // digits is the easiest mistake to make by hand and lands you
        // several hundred kilometres away.)
        let blr = parse("MK82").expect("MK82 parses");
        near(blr.lat, 12.5, 0.001, "MK82 latitude");
        near(blr.lon, 77.0, 0.001, "MK82 longitude");
        let other = parse("MK68").expect("MK68 parses");
        near(other.lat, 18.5, 0.001, "MK68 latitude");
        near(other.lon, 73.0, 0.001, "MK68 longitude");

        // AA00 is the south-west corner square; its centre sits half a
        // square in from -90/-180.
        let corner = parse("AA00").expect("AA00 parses");
        near(corner.lat, -89.5, 0.001, "AA00 latitude");
        near(corner.lon, -179.0, 0.001, "AA00 longitude");
    }

    /// Case must not matter: locators are written `JN58td`, `jn58TD` and
    /// every combination in logs and on the air.
    #[test]
    fn case_is_irrelevant() {
        assert_eq!(parse("jn58td"), parse("JN58TD"));
        assert_eq!(parse("Jn58Td"), parse("JN58TD"));
        assert_eq!(parse("mk68"), parse("MK68"));
    }

    /// Six characters must land inside the four-character square they
    /// refine — a subsquare that escaped its parent would be a sign the
    /// arithmetic is wrong in a way single-value tests can miss.
    #[test]
    fn a_subsquare_lies_within_its_square() {
        let square = parse("JN58").unwrap();
        for sub in ["JN58AA", "JN58TD", "JN58XX", "JN58MM"] {
            let p = parse(sub).unwrap();
            assert!(
                (p.lat - square.lat).abs() <= 0.5,
                "{sub} latitude escaped JN58"
            );
            assert!(
                (p.lon - square.lon).abs() <= 1.0,
                "{sub} longitude escaped JN58"
            );
        }
    }

    /// Malformed input disables the mask rather than guessing a position,
    /// so every one of these must be `None`.
    #[test]
    fn malformed_locators_are_refused() {
        for bad in [
            "", "J", "JN5", "JN588", "JN58TDX", "JN58TDXX", // wrong length
            "SN58", "JS58", // field letter past R
            "JNX8", "JN5X", // square must be digits
            "JN58YD", "JN58TY", // subsquare letter past X
            "12345678", "----", "JN 58",
        ] {
            assert_eq!(parse(bad), None, "{bad:?} must be refused");
        }
    }

    /// Whitespace around a pasted locator is the operator's, not an error.
    #[test]
    fn surrounding_whitespace_is_tolerated() {
        assert_eq!(parse("  MK68  "), parse("MK68"));
    }
}
