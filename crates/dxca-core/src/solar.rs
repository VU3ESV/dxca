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
    let eps0 =
        23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.000_59 - t * 0.001_813))) / 60.0) / 60.0;
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

// ---------------------------------------------------------------------------
//  Sunrise, sunset, and the four phases of the station's day
// ---------------------------------------------------------------------------
//
// Elevation alone answers "how high is the sun", which is enough to say
// whether a band is a day band or a night band and NOT enough to find the
// grey line. The grey line is the narrow window either side of the terminator
// where the D layer has collapsed but the F layer is still lit — the best
// hour of the day on 160m and 80m, and an elevation threshold cannot express
// it, because "45 minutes either side of sunset" is 11° of elevation at the
// equator and barely 3° in northern Europe.
//
// So the window is expressed in MINUTES and resolved against the actual
// sunrise and sunset for that place and day. The operator sets the minutes,
// because how long the grey line is useful for genuinely varies — with the
// band, the season, the path and the station.
//
// This model — Dawn / Day / Dusk / Night around a configurable window —
// follows Meridian's `meridian-core::geo`, which has the same shack's
// greyline scheduler behind it. Same idea, same default of 45 minutes, so
// the two programs cannot disagree about what phase it is.

/// The four propagation phases of the station's day.
///
/// There is one sun, so this is a property of the station, not of a band —
/// which band is plausible in which phase is [`crate::bands`]'s business.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SunPhase {
    Dawn,
    Day,
    Dusk,
    Night,
}

impl SunPhase {
    /// Stable machine key — the same string serde emits.
    pub fn key(self) -> &'static str {
        match self {
            SunPhase::Dawn => "dawn",
            SunPhase::Day => "day",
            SunPhase::Dusk => "dusk",
            SunPhase::Night => "night",
        }
    }

    /// Is this one of the two grey-line phases?
    pub fn is_greyline(self) -> bool {
        matches!(self, SunPhase::Dawn | SunPhase::Dusk)
    }
}

/// Today's sunrise and sunset as Unix seconds (UTC).
///
/// `None` for an event that does not happen: polar day has no sunrise or
/// sunset, and neither does polar night.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SunTimes {
    pub sunrise_unix: Option<i64>,
    pub sunset_unix: Option<i64>,
}

pub fn sun_times(pos: LatLon, at_unix: i64) -> SunTimes {
    let midnight = at_unix.div_euclid(86_400) * 86_400;
    let doy = day_of_year_utc(at_unix);
    let event = |rising: bool| match event_ut_hours(doy, pos, rising) {
        SunCalc::Event(ut) => Some(midnight + (ut * 3600.0).round() as i64),
        SunCalc::NeverRises | SunCalc::NeverSets => None,
    };
    SunTimes {
        sunrise_unix: event(true),
        sunset_unix: event(false),
    }
}

/// The phase at `at_unix`, counting `± window_min` minutes around sunrise as
/// `Dawn` and around sunset as `Dusk`.
///
/// Pure and clock-free. Polar day and polar night return `Day`/`Night`; when
/// a short day or night makes the two windows overlap they meet at solar
/// midday/midnight and the vanishing `Day`/`Night` is never returned — which
/// is the correct answer at high latitude in June, where the whole night IS
/// grey line.
pub fn phase(pos: LatLon, at_unix: i64, window_min: u32) -> SunPhase {
    let w = window_min as i64 * 60;
    let events = events_near(pos, at_unix);
    let prev = events.iter().rev().find(|&&(t, _)| t <= at_unix).copied();
    let next = events.iter().find(|&&(t, _)| t > at_unix).copied();

    match (prev, next) {
        // Sun is up: [sunrise pt] .. now .. [sunset nt].
        (Some((pt, Event::Sunrise)), Some((nt, Event::Sunset))) => {
            if pt + w >= nt - w {
                if at_unix < (pt + nt) / 2 {
                    SunPhase::Dawn
                } else {
                    SunPhase::Dusk
                }
            } else if at_unix < pt + w {
                SunPhase::Dawn
            } else if at_unix >= nt - w {
                SunPhase::Dusk
            } else {
                SunPhase::Day
            }
        }
        // Sun is down: [sunset pt] .. now .. [sunrise nt].
        (Some((pt, Event::Sunset)), Some((nt, Event::Sunrise))) => {
            if pt + w >= nt - w {
                if at_unix < (pt + nt) / 2 {
                    SunPhase::Dusk
                } else {
                    SunPhase::Dawn
                }
            } else if at_unix < pt + w {
                SunPhase::Dusk
            } else if at_unix >= nt - w {
                SunPhase::Dawn
            } else {
                SunPhase::Night
            }
        }
        // Polar day/night, or a transition day with a lone event.
        _ => polar_state(pos, at_unix).unwrap_or_else(|| match prev.or(next) {
            Some((_, Event::Sunrise)) => SunPhase::Day,
            _ => SunPhase::Night,
        }),
    }
}

/// `Some` phase when the day has no horizon crossing at all.
fn polar_state(pos: LatLon, at_unix: i64) -> Option<SunPhase> {
    match event_ut_hours(day_of_year_utc(at_unix), pos, true) {
        SunCalc::NeverRises => Some(SunPhase::Night),
        SunCalc::NeverSets => Some(SunPhase::Day),
        SunCalc::Event(_) => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Event {
    Sunrise,
    Sunset,
}

/// Events for the UTC days bracketing `at_unix` — yesterday through two days
/// ahead, sorted. Wide enough to bracket any instant between an adjacent
/// pair and to reach tomorrow's sunrise for the night→dawn boundary.
fn events_near(pos: LatLon, at_unix: i64) -> Vec<(i64, Event)> {
    let mut events = Vec::new();
    for k in -1..=2 {
        let t = sun_times(pos, at_unix + k * 86_400);
        if let Some(sr) = t.sunrise_unix {
            events.push((sr, Event::Sunrise));
        }
        if let Some(ss) = t.sunset_unix {
            events.push((ss, Event::Sunset));
        }
    }
    events.sort_by_key(|&(t, _)| t);
    events
}

enum SunCalc {
    /// UT hours in [0, 24) of the event.
    Event(f64),
    NeverRises,
    NeverSets,
}

/// "Almanac for Computers" sunrise/sunset solve.
///
/// A different algorithm from `elevation` above, and deliberately so: this
/// one solves directly for the horizon crossing instead of searching the
/// elevation curve for a zero, which would need iteration and would behave
/// badly on the polar days where the curve never crosses. Its 90.833° zenith
/// includes the standard refraction and solar-radius allowance, so these
/// times match a published almanac rather than the geometric sunrise
/// `elevation` describes.
fn event_ut_hours(day_of_year: f64, pos: LatLon, rising: bool) -> SunCalc {
    const DEG: f64 = std::f64::consts::PI / 180.0;
    let zenith = 90.833_f64;
    let lng_hour = pos.lon / 15.0;
    let t = day_of_year + ((if rising { 6.0 } else { 18.0 } - lng_hour) / 24.0);

    let m = 0.9856 * t - 3.289;
    let l =
        (m + 1.916 * (m * DEG).sin() + 0.020 * (2.0 * m * DEG).sin() + 282.634).rem_euclid(360.0);

    let mut ra = (0.91764 * (l * DEG).tan()).atan() / DEG;
    ra = ra.rem_euclid(360.0);
    ra += (l / 90.0).floor() * 90.0 - (ra / 90.0).floor() * 90.0;
    ra /= 15.0;

    let sin_dec = 0.39782 * (l * DEG).sin();
    let cos_dec = sin_dec.asin().cos();

    let cos_h = ((zenith * DEG).cos() - sin_dec * (pos.lat * DEG).sin())
        / (cos_dec * (pos.lat * DEG).cos());
    if cos_h > 1.0 {
        return SunCalc::NeverRises;
    }
    if cos_h < -1.0 {
        return SunCalc::NeverSets;
    }
    let h = if rising {
        360.0 - cos_h.acos() / DEG
    } else {
        cos_h.acos() / DEG
    } / 15.0;

    SunCalc::Event((h + ra - 0.06571 * t - 6.622 - lng_hour).rem_euclid(24.0))
}

/// Day of year (1-based) for the UTC calendar day containing `at_unix`.
/// Howard Hinnant's civil calendar algorithms, proleptic Gregorian.
fn day_of_year_utc(at_unix: i64) -> f64 {
    let days = at_unix.div_euclid(86_400);
    let (y, _, _) = civil_from_days(days);
    (days - days_from_civil(y, 1, 1) + 1) as f64
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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
        let pos = LatLon {
            lat: 69.65,
            lon: 18.96,
        };
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
        let pos = LatLon {
            lat: 69.65,
            lon: 18.96,
        };
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
        // 2026-06-21T00:00:00Z — already an exact midnight, so the
        // round-down this used to spell out (`t - t % 86_400`) was a no-op.
        let midnight_utc = 1_782_000_000;
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
        assert!(
            best > 60.0,
            "a tropical midsummer noon should be high: {best}"
        );
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

    // --- phases and the grey line -------------------------------------

    /// The whole reason phases exist. `phase` must call the window in
    /// MINUTES either side of the real sunset, and 45 minutes is a very
    /// different number of degrees at 13°N than at 48°N — which is exactly
    /// what an elevation threshold could not express.
    #[test]
    fn the_greyline_window_is_minutes_not_degrees() {
        // 2026-06-21, a solstice, so the two latitudes are as far apart in
        // day length as they ever get.
        let day = 1_782_000_000; // 2026-06-21 08:00 UTC
        for (name, grid) in [("Bengaluru", "MK83TE"), ("Munich", "JN58TD")] {
            let pos = grid::parse(grid).expect("valid grid");
            let sunset = super::sun_times(pos, day)
                .sunset_unix
                .expect("neither is polar");

            // 44 minutes before sunset is Dusk; 46 minutes before is not.
            assert_eq!(
                super::phase(pos, sunset - 44 * 60, 45),
                SunPhase::Dusk,
                "{name}: inside the window"
            );
            assert_ne!(
                super::phase(pos, sunset - 46 * 60, 45),
                SunPhase::Dusk,
                "{name}: outside the window"
            );
            // And the window scales with the setting, not with the sun's
            // angle: at 90 minutes the same instant IS dusk.
            assert_eq!(
                super::phase(pos, sunset - 46 * 60, 90),
                SunPhase::Dusk,
                "{name}: a wider window reaches further"
            );
        }
    }

    /// Elevation at a fixed number of minutes before sunset differs sharply
    /// with latitude. This is the evidence for the test above, pinned so a
    /// future reader can see WHY minutes and degrees are not interchangeable.
    #[test]
    fn the_same_minutes_are_very_different_elevations() {
        let day = 1_782_000_000;
        let mut elevations = vec![];
        for grid in ["MK83TE", "JN58TD"] {
            let pos = grid::parse(grid).unwrap();
            let sunset = super::sun_times(pos, day).sunset_unix.unwrap();
            elevations.push(elevation(pos, sunset - 45 * 60));
        }
        // Bengaluru's sun is far higher 45 minutes before sunset than
        // Munich's, because it sets almost vertically near the equator.
        assert!(
            elevations[0] > elevations[1] + 3.0,
            "expected a large gap, got {elevations:?}"
        );
    }

    /// Above the Arctic circle in June the sun never sets, and the phase
    /// model must say Day for the whole 24 hours rather than dividing a
    /// nonexistent sunset into windows.
    #[test]
    fn polar_day_is_day_all_day() {
        let pos = grid::parse("JP99").expect("Tromsø-ish"); // ~69.5N
        let june = 1_782_000_000;
        for h in 0..24 {
            assert_eq!(
                super::phase(pos, june + h * 3600, 45),
                SunPhase::Day,
                "hour {h} of the midnight sun"
            );
        }
        let t = super::sun_times(pos, june);
        assert_eq!(t.sunrise_unix, None, "polar day has no sunrise");
        assert_eq!(t.sunset_unix, None, "polar day has no sunset");
    }

    #[test]
    fn polar_night_is_night_all_day() {
        let pos = grid::parse("JP99").unwrap();
        let december = 1_797_000_000; // 2026-12-16
        for h in 0..24 {
            assert_eq!(
                super::phase(pos, december + h * 3600, 45),
                SunPhase::Night,
                "hour {h} of the polar night"
            );
        }
    }

    /// Every instant of a day must land in exactly one phase, and a normal
    /// day must contain all four. A gap or an overlap would show up here as
    /// a missing phase.
    #[test]
    fn a_normal_day_passes_through_all_four_phases() {
        let pos = grid::parse("MK83TE").unwrap();
        let start = 1_782_000_000;
        let mut seen = std::collections::BTreeSet::new();
        for m in 0..(24 * 60) {
            seen.insert(super::phase(pos, start + m * 60, 45).key());
        }
        assert_eq!(
            seen,
            ["dawn", "day", "dusk", "night"].into_iter().collect(),
            "a normal day has all four phases"
        );
    }

    /// A window wide enough to swallow the day must never report Day — the
    /// short-day branch. Without it, a 12-hour window at a high latitude
    /// would leave instants in no phase at all.
    #[test]
    fn an_absurdly_wide_window_still_answers() {
        let pos = grid::parse("MK83TE").unwrap();
        let start = 1_782_000_000;
        for m in 0..(24 * 60) {
            let p = super::phase(pos, start + m * 60, 720);
            assert!(
                p.is_greyline(),
                "a 12-hour window is grey line throughout, got {p:?}"
            );
        }
    }

    /// Sunrise and sunset here come from the almanac solve at 90.833°, which
    /// includes refraction — so unlike `elevation`, these should match
    /// published times closely rather than with a known bias.
    #[test]
    fn sun_times_match_published_values() {
        // Munich, 2026-06-21: published sunrise 05:14 local (03:14 UTC),
        // sunset 21:18 local (19:18 UTC).
        let pos = grid::parse("JN58TD").unwrap();
        let t = super::sun_times(pos, 1_782_000_000);
        let hhmm = |u: i64| (u.rem_euclid(86_400)) / 60; // minutes into the UTC day
        assert!(
            (hhmm(t.sunrise_unix.unwrap()) - (3 * 60 + 14)).abs() <= 5,
            "sunrise {} min into the day",
            hhmm(t.sunrise_unix.unwrap())
        );
        assert!(
            (hhmm(t.sunset_unix.unwrap()) - (19 * 60 + 18)).abs() <= 5,
            "sunset {} min into the day",
            hhmm(t.sunset_unix.unwrap())
        );
    }
}
