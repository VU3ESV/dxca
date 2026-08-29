//! Where the sun is, for a place and an instant.
//!
//! Milestone 1 of [`docs/PHASE-ROTATION-MASK.md`]. The phase-rotation mask
//! asks one question of this module — *how high is the sun at the operator's
//! QTH right now* — and decides which bands are plausible from the answer.
//!
//! **Why elevation rather than clock time.** A table of local-time windows
//! ("160m from sunset to sunrise") is the obvious implementation and is
//! wrong exactly where it matters. Sunset moves about an hour across the
//! year in Bengaluru and about six in northern Europe; above the Arctic
//! circle the concept stops existing. Elevation encodes latitude, longitude,
//! date and time in one number, with no tables to go stale.
//!
//! This is the NOAA solar-position algorithm, which is accurate to well
//! under a degree — far finer than a band-openness heuristic can use. It
//! needs no data files and no network: only the timestamp and the position.
//!
//! Atmospheric refraction is **not** corrected for. It lifts the apparent
//! sun by about 0.6° near the horizon, which matters for publishing a
//! sunrise table and does not matter for deciding whether 160m is plausible.
//! Said here so nobody later mistakes the omission for an oversight.

use crate::grid::LatLon;

/// Sun elevation in degrees above the horizon: negative when below it.
///
/// `at_unix` is ordinary Unix seconds (UTC).
pub fn elevation(pos: LatLon, at_unix: i64) -> f64 {
    let jd = at_unix as f64 / 86_400.0 + 2_440_587.5;
    let t = (jd - 2_451_545.0) / 36_525.0; // Julian centuries since J2000.0

    // Geometric mean longitude and anomaly of the sun.
    let l0 = (280.466_46 + t * (36_000.769_83 + t * 0.000_303_2)).rem_euclid(360.0);
    let m = 357.529_11 + t * (35_999.050_29 - 0.000_153_7 * t);
    let e = 0.016_708_634 - t * (0.000_042_037 + 0.000_000_126_7 * t);

    // Equation of centre → true → apparent longitude.
    let m_rad = m.to_radians();
    let c = m_rad.sin() * (1.914_602 - t * (0.004_817 + 0.000_014 * t))
        + (2.0 * m_rad).sin() * (0.019_993 - 0.000_101 * t)
        + (3.0 * m_rad).sin() * 0.000_289;
    let true_long = l0 + c;
    let omega = 125.04 - 1_934.136 * t;
    let lambda = true_long - 0.005_69 - 0.004_78 * omega.to_radians().sin();

    // Obliquity of the ecliptic, corrected.
    let eps0 = 23.0
        + (26.0 + (21.448 - t * (46.815 + t * (0.000_59 - t * 0.001_813))) / 60.0) / 60.0;
    let eps = (eps0 + 0.002_56 * omega.to_radians().cos()).to_radians();

    let declination = (eps.sin() * lambda.to_radians().sin()).asin();

    // Equation of time, in minutes.
    let y = (eps / 2.0).tan().powi(2);
    let l0_rad = l0.to_radians();
    let eq_time = 4.0
        * (y * (2.0 * l0_rad).sin() - 2.0 * e * m_rad.sin()
            + 4.0 * e * y * m_rad.sin() * (2.0 * l0_rad).cos()
            - 0.5 * y * y * (4.0 * l0_rad).sin()
            - 1.25 * e * e * (2.0 * m_rad).sin())
        .to_degrees();

    // True solar time → hour angle.
    let minutes_utc = (at_unix.rem_euclid(86_400)) as f64 / 60.0;
    let true_solar = minutes_utc + eq_time + 4.0 * pos.lon;
    let hour_angle = (true_solar / 4.0 - 180.0).to_radians();

    let lat = pos.lat.to_radians();
    let cos_zenith =
        lat.sin() * declination.sin() + lat.cos() * declination.cos() * hour_angle.cos();
    90.0 - cos_zenith.clamp(-1.0, 1.0).acos().to_degrees()
}

/// Is the sun below the horizon at this place and time?
pub fn is_dark(pos: LatLon, at_unix: i64) -> bool {
    elevation(pos, at_unix) < 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid;

    /// 2026-03-20 was an equinox; on any equinox the sun stands overhead at
    /// the equator at local solar noon, which is the sharpest check
    /// available without a published table.
    #[test]
    fn overhead_at_the_equator_at_solar_noon_on_an_equinox() {
        // Equator, longitude 0 — solar noon is close to 12:00 UTC.
        let pos = LatLon { lat: 0.0, lon: 0.0 };
        // 2026-03-20 12:00 UTC.
        let noon = 1_774_008_000;
        let el = elevation(pos, noon);
        assert!(
            el > 88.0,
            "the sun should be all but overhead at an equinox noon, got {el}"
        );
    }

    /// The sun is below the horizon at local midnight and above it at local
    /// noon, everywhere that has a normal day. Checked across latitudes and
    /// both hemispheres, because a sign error in the declination term
    /// survives a single-location test.
    #[test]
    fn night_is_below_the_horizon_and_noon_is_above_it() {
        // 2026-06-21 00:00 UTC, northern summer solstice.
        let midnight_utc = 1_782_000_000;
        for (name, lat, lon) in [
            ("Bengaluru", 12.97, 77.59),
            ("Munich", 48.14, 11.58),
            ("Sydney", -33.87, 151.21),
            ("Cape Town", -33.92, 18.42),
        ] {
            let pos = LatLon { lat, lon };
            // Local solar noon in UTC: an EASTERN longitude reaches noon
            // earlier in the UTC day, so the offset is subtracted from
            // 12:00 rather than from midnight.
            let noon = midnight_utc + ((12.0 - lon / 15.0) * 3_600.0) as i64;
            let midnight = noon + 12 * 3_600;
            assert!(
                elevation(pos, noon) > 0.0,
                "{name}: sun should be up at local noon"
            );
            assert!(
                elevation(pos, midnight) < 0.0,
                "{name}: sun should be down at local midnight"
            );
        }
    }

    /// Polar day is the case a clock-time rule cannot express at all, and
    /// the reason this module exists. Above the Arctic circle in June the
    /// sun never sets — so a "160m is open at 0200 local" rule would be
    /// wrong for months at a time.
    #[test]
    fn the_midnight_sun_never_sets() {
        // Tromsø, well inside the Arctic circle, at the June solstice.
        let pos = LatLon { lat: 69.65, lon: 18.96 };
        let solstice = 1_782_000_000;
        for hour in 0..24 {
            let t = solstice + hour * 3_600;
            assert!(
                elevation(pos, t) > 0.0,
                "the sun must not set at Tromsø in June (hour {hour})"
            );
        }
    }

    /// ...and the same place in December never sees it rise.
    #[test]
    fn the_polar_night_never_dawns() {
        let pos = LatLon { lat: 69.65, lon: 18.96 };
        // 2026-12-21.
        let solstice = 1_797_811_200;
        for hour in 0..24 {
            let t = solstice + hour * 3_600;
            assert!(
                elevation(pos, t) < 0.0,
                "the sun must not rise at Tromsø in December (hour {hour})"
            );
        }
    }

    /// Elevation must vary smoothly and peak once a day — a check that
    /// catches a wrapped hour angle, which would otherwise look plausible
    /// at any single instant.
    #[test]
    fn elevation_peaks_once_a_day_near_local_noon() {
        let pos = grid::parse("MK82").expect("MK82");
        let midnight_utc = 1_782_000_000 - 1_782_000_000 % 86_400;
        let (mut best_hour, mut best) = (0, f64::MIN);
        for hour in 0..24 {
            let el = elevation(pos, midnight_utc + hour * 3_600);
            if el > best {
                best = el;
                best_hour = hour;
            }
        }
        // MK82 is around 77°E, so solar noon is near 0700 UTC.
        assert!(
            (6..=8).contains(&best_hour),
            "peak elevation at {best_hour}:00 UTC, expected near 07:00"
        );
        assert!(best > 60.0, "a tropical midsummer noon should be high: {best}");
    }

    /// Validation against the outside world, not just against itself.
    ///
    /// Published sunrise times, converted to UTC:
    ///   Munich    2026-06-21  05:14 CEST = 03:14 UTC
    ///   Munich    2026-12-21  08:03 CET  = 07:03 UTC
    ///   Bengaluru 2026-06-21  05:54 IST  = 00:24 UTC
    ///
    /// This module computes 03:20, 07:08 and 00:32 — consistently 5 to 8
    /// minutes late, and consistently in the same direction. That is
    /// **atmospheric refraction**, which lifts the apparent sun by about
    /// 0.6° and makes it appear to rise several minutes before it
    /// geometrically does; almanacs quote the apparent time, this returns
    /// the true one. The bias is left uncorrected on purpose (see the
    /// module docs) because a band-openness threshold cannot use that
    /// precision — but it is pinned here so the day someone sees a
    /// six-minute discrepancy, they find this note instead of a mystery.
    #[test]
    fn sunrise_matches_published_times_within_the_refraction_bias() {
        let cases = [
            ("Munich June", "JN58TD", 1_782_000_000_i64, 3 * 60 + 14),
            ("Munich December", "JN58TD", 1_797_811_200_i64, 7 * 60 + 3),
            ("Bengaluru June", "MK82", 1_782_000_000_i64, 24),
        ];
        for (name, loc, midnight_utc, published_minute) in cases {
            let pos = grid::parse(loc).unwrap();
            let mut prev = elevation(pos, midnight_utc);
            let mut found = None;
            for m in 1..1440 {
                let el = elevation(pos, midnight_utc + m * 60);
                if prev < 0.0 && el >= 0.0 {
                    found = Some(m);
                    break;
                }
                prev = el;
            }
            let got = found.unwrap_or_else(|| panic!("{name}: no sunrise found"));
            let late = got - published_minute;
            assert!(
                (0..=12).contains(&late),
                "{name}: sunrise {}:{:02} UTC is {late} min from the published \
                 {}:{:02} — expected 0-12 late (refraction)",
                got / 60,
                got % 60,
                published_minute / 60,
                published_minute % 60
            );
        }
    }

    /// The convenience wrapper must agree with the number it wraps.
    #[test]
    fn is_dark_agrees_with_elevation() {
        let pos = grid::parse("JN58TD").unwrap();
        for hour in 0..24 {
            let t = 1_782_000_000 + hour * 3_600;
            assert_eq!(is_dark(pos, t), elevation(pos, t) < 0.0);
        }
    }
}
