//! Frequency → ADIF band name ("20M", "40M", …) — port of the Swift
//! `BandResolver`, table values copied verbatim.

use crate::solar::SunPhase;

struct BandRange {
    name: &'static str,
    low_mhz: f64,
    high_mhz: f64,
}

#[rustfmt::skip]
static BANDS: &[BandRange] = &[
    BandRange { name: "2190M", low_mhz: 0.135,  high_mhz: 0.138 },
    BandRange { name: "630M",  low_mhz: 0.472,  high_mhz: 0.479 },
    BandRange { name: "160M",  low_mhz: 1.8,    high_mhz: 2.0 },
    BandRange { name: "80M",   low_mhz: 3.5,    high_mhz: 4.0 },
    BandRange { name: "60M",   low_mhz: 5.25,   high_mhz: 5.45 },
    BandRange { name: "40M",   low_mhz: 7.0,    high_mhz: 7.3 },
    BandRange { name: "30M",   low_mhz: 10.1,   high_mhz: 10.15 },
    BandRange { name: "20M",   low_mhz: 14.0,   high_mhz: 14.35 },
    BandRange { name: "17M",   low_mhz: 18.068, high_mhz: 18.168 },
    BandRange { name: "15M",   low_mhz: 21.0,   high_mhz: 21.45 },
    BandRange { name: "12M",   low_mhz: 24.89,  high_mhz: 24.99 },
    BandRange { name: "10M",   low_mhz: 28.0,   high_mhz: 29.7 },
    BandRange { name: "6M",    low_mhz: 50.0,   high_mhz: 54.0 },
    BandRange { name: "4M",    low_mhz: 70.0,   high_mhz: 70.5 },
    BandRange { name: "2M",    low_mhz: 144.0,  high_mhz: 148.0 },
    BandRange { name: "1.25M", low_mhz: 222.0,  high_mhz: 225.0 },
    BandRange { name: "70CM",  low_mhz: 420.0,  high_mhz: 450.0 },
    BandRange { name: "33CM",  low_mhz: 902.0,  high_mhz: 928.0 },
    BandRange { name: "23CM",  low_mhz: 1240.0, high_mhz: 1300.0 },
];

/// The bands offered as filter/alert choices, 160M → 70CM, in the operator's
/// customary order (longest wavelength first). Deliberately narrower than
/// `BANDS`: the LF/MF pair below 160M and the microwave bands above 70CM are
/// still *resolved* from frequency — a spot there keeps its band name — they
/// just aren't worth a checkbox in this shack. Serving it from here keeps the
/// UI, the Telegram gate and the resolver reading one list.
#[rustfmt::skip]
pub const SELECTABLE_BANDS: &[&str] = &[
    "160M", "80M", "60M", "40M", "30M", "20M", "17M", "15M", "12M", "10M",
    "6M", "4M", "2M", "1.25M", "70CM",
];

/// The ten bands that score for the **ARRL DXCC Challenge**: one point per
/// entity per band, confirmed contacts only, 1945 onward.
///
/// The trap is **60M, which does not count** — it is in `SELECTABLE_BANDS`
/// and in the resolver, and an operator filtering the spots screen by band
/// sees it there, but a 60m QSL adds nothing to the Challenge total. The
/// three WARC bands (30/17/12) DO count, which is the other half of the same
/// confusion. Nothing above 6M scores either.
///
/// Mode is irrelevant here — that is what separates a Challenge point from
/// this crate's "slot", which is band × mode.
#[rustfmt::skip]
pub const CHALLENGE_BANDS: &[&str] = &[
    "160M", "80M", "40M", "30M", "20M", "17M", "15M", "12M", "10M", "6M",
];

/// Which phases of the station's day a band is plausibly workable in — the
/// model behind the phase-rotation spot mask
/// (`docs/PHASE-ROTATION-MASK.md`).
///
/// A band with **no entry here is never masked**, which is the deliberate
/// default for everything this model does not claim to understand.
///
/// **Why phases rather than sun-elevation windows,** which this table
/// replaced: the interesting propagation on the low bands happens on the
/// *grey line*, the narrow window either side of the terminator where the D
/// layer has collapsed but the F layer is still lit. An elevation threshold
/// cannot express it — "45 minutes either side of sunset" is about 11° of
/// elevation at the equator and barely 3° in northern Europe, so one
/// threshold is wrong at one end or the other. Phases are resolved against
/// the real sunrise and sunset for that place and day, and the window is
/// the operator's to set.
///
/// These assignments are a starting point to be tuned against a real feed,
/// not settled physics. They describe only the *operator's* end: a genuine
/// 160m opening needs darkness at both ends of the path, and this knows
/// about one. It is a plausibility filter, not a prediction.
#[rustfmt::skip]
const BAND_PHASES: &[(&str, &[SunPhase])] = &[
    // Darkness bands. The grey line is the whole point on these two — dawn
    // and dusk are when the DX is worked, not merely when it is possible.
    ("160M", &[SunPhase::Night, SunPhase::Dawn, SunPhase::Dusk]),
    ("80M",  &[SunPhase::Night, SunPhase::Dawn, SunPhase::Dusk]),
    // Night bands that stay usable well into the grey line and a little past.
    ("60M",  &[SunPhase::Night, SunPhase::Dawn, SunPhase::Dusk]),
    ("40M",  &[SunPhase::Night, SunPhase::Dawn, SunPhase::Dusk]),
    // 30M works day and night — no entry, never masked.
    // 20M is open in daylight and stays open well past sunset on long paths,
    // which is why Night is included here and nowhere above it.
    ("20M",  &[SunPhase::Day, SunPhase::Dawn, SunPhase::Dusk, SunPhase::Night]),
    ("17M",  &[SunPhase::Day, SunPhase::Dawn, SunPhase::Dusk]),
    ("15M",  &[SunPhase::Day, SunPhase::Dawn, SunPhase::Dusk]),
    // The high bands want real daylight and a MUF to match.
    ("12M",  &[SunPhase::Day]),
    ("10M",  &[SunPhase::Day]),
    // 6M and up obey sporadic-E and tropo, not the sun. Never masked.
];

/// Is `band` plausibly workable during `phase`?
///
/// **Fails open**: an unknown band, or one the model says nothing about
/// (30M, 6M and up), is always plausible. The asymmetry is deliberate and
/// runs through this whole feature — hiding a workable rare one costs far
/// more than showing an unworkable one.
pub fn plausible_in(band: &str, phase: SunPhase) -> bool {
    let band = band.trim().to_ascii_uppercase();
    match BAND_PHASES.iter().find(|(b, _)| *b == band) {
        Some((_, phases)) => phases.contains(&phase),
        None => true,
    }
}

/// Does a band score for the DXCC Challenge?
pub fn is_challenge_band(band: &str) -> bool {
    CHALLENGE_BANDS.contains(&band)
}

// --- mode inference from frequency ---------------------------------------
//
// Cluster nodes like DB0SUE and N2WQ relay human spots whose comment is free
// text with no mode field at all, so `scrape_mode` finds nothing. An unknown
// mode used to fall through `modes::canonical("")` to **DATA**, silently
// crediting a 14.200 phone spot to the operator's digital slots. Guessing
// from frequency is strictly better than guessing DATA, provided the guess
// is labelled as one — which is why `Spot::mode_inferred` exists.
//
// **IARU Region 3** (this shack's own), by explicit choice. The honest
// limitation: a spot's mode follows the *transmitting* station's band plan,
// so a Region 1 station calling phone low in 40m can be inferred wrongly.
// The segments below therefore stay coarse, and anything not covered returns
// None rather than being forced into a bucket.

/// Digital calling frequencies (dial, MHz). Checked BEFORE the broad
/// segments because several sit inside a phone segment — 50.313 FT8 is in
/// the middle of the 6m SSB range, and would otherwise infer as SSB.
#[rustfmt::skip]
static WATERING_HOLES: &[(f64, &str)] = &[
    // FT8
    (1.840, "FT8"),   (3.573, "FT8"),   (7.074, "FT8"),   (10.136, "FT8"),
    (14.074, "FT8"),  (18.100, "FT8"),  (21.074, "FT8"),  (24.915, "FT8"),
    (28.074, "FT8"),  (50.313, "FT8"),  (50.323, "FT8"),  (144.174, "FT8"),
    // FT4
    (3.575, "FT4"),   (7.0475, "FT4"),  (10.140, "FT4"),  (14.080, "FT4"),
    (18.104, "FT4"),  (21.140, "FT4"),  (24.919, "FT4"),  (28.180, "FT4"),
    // JS8
    (1.842, "JS8"),   (3.578, "JS8"),   (7.078, "JS8"),   (14.078, "JS8"),
    (21.078, "JS8"),  (24.922, "JS8"),  (28.078, "JS8"),
    // WSPR
    (1.8366, "WSPR"), (3.5686, "WSPR"), (7.0386, "WSPR"), (10.1387, "WSPR"),
    (14.0956, "WSPR"), (18.1046, "WSPR"), (21.0946, "WSPR"), (28.1246, "WSPR"),
];

/// ±500 Hz. Spotters round, and a dial a few hundred Hz off is still that
/// mode's watering hole; a full kHz would start swallowing its neighbours.
///
/// In **Hz, compared as integers**. As MHz f64 the arithmetic misses its own
/// boundary: `(14.0745 - 14.074).abs()` is 0.0005000000000000004, so a dial
/// exactly 500 Hz up fell outside a 0.0005 tolerance.
const HOLE_TOLERANCE_HZ: i64 = 500;

struct ModeRange {
    low_mhz: f64,
    high_mhz: f64,
    /// "CW" / "SSB" / "DATA" — what `modes::canonical` needs to bucket it.
    mode: &'static str,
}

/// Coarse IARU Region 3 segments. Deliberately leaves gaps (beacon windows,
/// the 10m 28.190–28.300 beacon band, everything above 2m) so an uncertain
/// frequency infers nothing rather than something wrong.
#[rustfmt::skip]
static MODE_SEGMENTS: &[ModeRange] = &[
    ModeRange { low_mhz: 1.800,  high_mhz: 1.838,  mode: "CW" },
    ModeRange { low_mhz: 1.838,  high_mhz: 1.843,  mode: "DATA" },
    ModeRange { low_mhz: 1.843,  high_mhz: 2.000,  mode: "SSB" },

    ModeRange { low_mhz: 3.500,  high_mhz: 3.535,  mode: "CW" },
    ModeRange { low_mhz: 3.560,  high_mhz: 3.600,  mode: "DATA" },
    ModeRange { low_mhz: 3.600,  high_mhz: 4.000,  mode: "SSB" },

    ModeRange { low_mhz: 7.000,  high_mhz: 7.035,  mode: "CW" },
    ModeRange { low_mhz: 7.035,  high_mhz: 7.050,  mode: "DATA" },
    ModeRange { low_mhz: 7.080,  high_mhz: 7.300,  mode: "SSB" },

    ModeRange { low_mhz: 10.100, high_mhz: 10.130, mode: "CW" },
    ModeRange { low_mhz: 10.130, high_mhz: 10.150, mode: "DATA" },
    // No phone on 30m anywhere.

    ModeRange { low_mhz: 14.000, high_mhz: 14.070, mode: "CW" },
    ModeRange { low_mhz: 14.070, high_mhz: 14.099, mode: "DATA" },
    // 14.099-14.101 is the beacon window: no inference.
    ModeRange { low_mhz: 14.101, high_mhz: 14.112, mode: "DATA" },
    ModeRange { low_mhz: 14.112, high_mhz: 14.350, mode: "SSB" },

    ModeRange { low_mhz: 18.068, high_mhz: 18.095, mode: "CW" },
    ModeRange { low_mhz: 18.095, high_mhz: 18.109, mode: "DATA" },
    ModeRange { low_mhz: 18.111, high_mhz: 18.168, mode: "SSB" },

    ModeRange { low_mhz: 21.000, high_mhz: 21.070, mode: "CW" },
    ModeRange { low_mhz: 21.070, high_mhz: 21.149, mode: "DATA" },
    ModeRange { low_mhz: 21.151, high_mhz: 21.450, mode: "SSB" },

    ModeRange { low_mhz: 24.890, high_mhz: 24.915, mode: "CW" },
    ModeRange { low_mhz: 24.915, high_mhz: 24.929, mode: "DATA" },
    ModeRange { low_mhz: 24.931, high_mhz: 24.990, mode: "SSB" },

    ModeRange { low_mhz: 28.000, high_mhz: 28.070, mode: "CW" },
    ModeRange { low_mhz: 28.070, high_mhz: 28.190, mode: "DATA" },
    // 28.190-28.300 is the IBP beacon band: no inference.
    ModeRange { low_mhz: 28.300, high_mhz: 29.700, mode: "SSB" },

    ModeRange { low_mhz: 50.000, high_mhz: 50.100, mode: "CW" },
    ModeRange { low_mhz: 50.100, high_mhz: 50.500, mode: "SSB" },

    ModeRange { low_mhz: 144.000, high_mhz: 144.150, mode: "CW" },
    ModeRange { low_mhz: 144.150, high_mhz: 144.500, mode: "SSB" },
];

/// Best guess at the mode of a transmission on `freq`, or `None` when the
/// frequency is in no segment this table is confident about.
///
/// Always a guess. Callers must record that it was inferred rather than
/// reported — see `Spot::mode_inferred`.
pub fn mode_from_mhz(freq: f64) -> Option<&'static str> {
    let hz = (freq * 1_000_000.0).round() as i64;
    if let Some((_, m)) = WATERING_HOLES
        .iter()
        .find(|(f, _)| ((f * 1_000_000.0).round() as i64 - hz).abs() <= HOLE_TOLERANCE_HZ)
    {
        return Some(m);
    }
    MODE_SEGMENTS
        .iter()
        .find(|r| freq >= r.low_mhz && freq < r.high_mhz)
        .map(|r| r.mode)
}

pub fn mode_from_hz(freq: u64) -> Option<&'static str> {
    mode_from_mhz(freq as f64 / 1_000_000.0)
}

pub fn band_from_mhz(freq: f64) -> Option<&'static str> {
    BANDS
        .iter()
        .find(|b| freq >= b.low_mhz && freq <= b.high_mhz)
        .map(|b| b.name)
}

pub fn band_from_hz(freq: u64) -> Option<&'static str> {
    band_from_mhz(freq as f64 / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_bands() {
        assert_eq!(band_from_hz(14_074_000), Some("20M"));
        assert_eq!(band_from_hz(7_074_000), Some("40M"));
        assert_eq!(band_from_hz(24_915_000), Some("12M"));
        assert_eq!(band_from_mhz(50.313), Some("6M"));
    }

    #[test]
    fn every_selectable_band_is_a_real_band() {
        for name in SELECTABLE_BANDS {
            assert!(
                BANDS.iter().any(|b| b.name == *name),
                "{name} is offered as a filter but the resolver never emits it"
            );
        }
        assert_eq!(SELECTABLE_BANDS.first(), Some(&"160M"));
        assert_eq!(SELECTABLE_BANDS.last(), Some(&"70CM"));
    }

    /// Day and night, the two cases the mask exists to separate.
    #[test]
    fn the_phase_decides_which_bands_are_plausible() {
        assert!(
            !plausible_in("160M", SunPhase::Day),
            "160m at midday is the whole point"
        );
        assert!(!plausible_in("80M", SunPhase::Day));
        assert!(plausible_in("20M", SunPhase::Day));
        assert!(plausible_in("15M", SunPhase::Day));
        assert!(plausible_in("10M", SunPhase::Day));

        assert!(plausible_in("160M", SunPhase::Night));
        assert!(plausible_in("80M", SunPhase::Night));
        assert!(plausible_in("40M", SunPhase::Night));
        assert!(!plausible_in("15M", SunPhase::Night));
        assert!(!plausible_in("10M", SunPhase::Night));
    }

    /// The grey line is why phases replaced elevation windows. Dawn and dusk
    /// must be workable on the low bands AND on the high bands — that
    /// overlap is the point, and an elevation threshold that put 160m and
    /// 15m on opposite sides of one number could not produce it.
    #[test]
    fn the_greyline_is_open_at_both_ends_of_the_spectrum() {
        for phase in [SunPhase::Dawn, SunPhase::Dusk] {
            assert!(plausible_in("160M", phase), "160m on the grey line");
            assert!(plausible_in("80M", phase));
            assert!(plausible_in("40M", phase));
            assert!(plausible_in("20M", phase));
            assert!(plausible_in("15M", phase));
        }
        // The high bands are the exception: they want real daylight.
        assert!(!plausible_in("10M", SunPhase::Dawn));
        assert!(!plausible_in("12M", SunPhase::Dusk));
    }

    /// 30M works day and night, and the VHF bands answer to sporadic-E
    /// rather than the sun. Neither is ever masked.
    #[test]
    fn bands_the_model_does_not_claim_to_understand_are_never_masked() {
        for phase in [
            SunPhase::Dawn,
            SunPhase::Day,
            SunPhase::Dusk,
            SunPhase::Night,
        ] {
            assert!(plausible_in("30M", phase), "30M in {phase:?}");
            assert!(plausible_in("6M", phase), "6M in {phase:?}");
            assert!(plausible_in("2M", phase), "2M in {phase:?}");
            assert!(plausible_in("70CM", phase), "70CM in {phase:?}");
        }
    }

    /// Fail open: anything unrecognised is plausible. The cost of hiding a
    /// workable rare one far exceeds the cost of showing an unworkable one,
    /// so ignorance must never mask.
    #[test]
    fn an_unknown_band_is_never_masked() {
        for phase in [SunPhase::Day, SunPhase::Night] {
            assert!(plausible_in("", phase));
            assert!(plausible_in("2200M", phase), "a band the table omits");
            assert!(plausible_in("nonsense", phase));
        }
    }

    #[test]
    fn band_names_are_matched_case_insensitively() {
        assert_eq!(
            plausible_in("160m", SunPhase::Night),
            plausible_in("160M", SunPhase::Night)
        );
        assert!(!plausible_in("160m", SunPhase::Day));
    }

    /// Every band in the phase table must be a band the rest of DXCA knows,
    /// or the mask would silently never fire for it.
    #[test]
    fn every_phased_band_is_a_real_band() {
        for (band, _) in BAND_PHASES {
            assert!(
                SELECTABLE_BANDS.contains(band),
                "{band} is not in SELECTABLE_BANDS"
            );
        }
    }

    /// No band may be masked in every phase. An entry listing no phases —
    /// or a typo producing one — would hide that band around the clock,
    /// which is the one outcome this feature must never produce.
    #[test]
    fn no_band_is_masked_in_every_phase() {
        for (band, _) in BAND_PHASES {
            let open = [
                SunPhase::Dawn,
                SunPhase::Day,
                SunPhase::Dusk,
                SunPhase::Night,
            ]
            .into_iter()
            .filter(|p| plausible_in(band, *p))
            .count();
            assert!(open > 0, "{band} would be masked at every hour");
        }
    }

    #[test]
    fn challenge_bands_are_the_ten_that_score() {
        assert_eq!(CHALLENGE_BANDS.len(), 10);
        for name in CHALLENGE_BANDS {
            assert!(
                BANDS.iter().any(|b| b.name == *name),
                "{name} scores for the Challenge but the resolver never emits it"
            );
        }
        // WARC counts...
        for b in ["30M", "17M", "12M"] {
            assert!(is_challenge_band(b), "{b} (WARC) must count");
        }
        // ...60M does not, despite being a filterable band, and neither does
        // anything above 6M or below 160M.
        for b in ["60M", "4M", "2M", "1.25M", "70CM", "630M", "2190M"] {
            assert!(!is_challenge_band(b), "{b} must NOT count");
        }
    }

    #[test]
    fn boundaries_are_inclusive_and_gaps_are_none() {
        assert_eq!(band_from_mhz(14.0), Some("20M"));
        assert_eq!(band_from_mhz(14.35), Some("20M"));
        assert_eq!(band_from_mhz(13.999), None);
        assert_eq!(band_from_mhz(2.5), None); // between 160M and 80M
    }

    #[test]
    fn watering_holes_beat_the_segment_they_sit_in() {
        // The whole reason holes are checked first: 50.313 is inside the 6m
        // SSB segment, and 14.074 inside the 20m digital one.
        assert_eq!(mode_from_mhz(50.313), Some("FT8"));
        assert_eq!(mode_from_mhz(50.200), Some("SSB"), "6m phone either side");
        assert_eq!(mode_from_mhz(14.074), Some("FT8"));
        assert_eq!(mode_from_mhz(7.0475), Some("FT4"));
        assert_eq!(mode_from_mhz(14.0956), Some("WSPR"));
    }

    #[test]
    fn hole_tolerance_is_half_a_kilohertz() {
        assert_eq!(mode_from_mhz(14.0745), Some("FT8"), "+500 Hz still FT8");
        assert_eq!(mode_from_mhz(14.0735), Some("FT8"), "-500 Hz still FT8");
        // Beyond it, the surrounding digital segment answers — still DATA,
        // just not claimed to be FT8 specifically.
        assert_eq!(mode_from_mhz(14.0760), Some("DATA"));
    }

    #[test]
    fn segments_cover_the_common_cases() {
        assert_eq!(mode_from_mhz(14.020), Some("CW"));
        assert_eq!(mode_from_mhz(14.200), Some("SSB"), "the N2WQ-style case");
        assert_eq!(mode_from_mhz(7.020), Some("CW"));
        assert_eq!(mode_from_mhz(7.150), Some("SSB"));
        assert_eq!(mode_from_mhz(3.700), Some("SSB"));
        assert_eq!(mode_from_mhz(10.120), Some("CW"));
        assert_eq!(mode_from_mhz(21.300), Some("SSB"));
        assert_eq!(mode_from_mhz(28.500), Some("SSB"));
    }

    #[test]
    fn uncertain_frequencies_infer_nothing() {
        // Better a blank mode than a wrong award slot.
        assert_eq!(mode_from_mhz(14.100), None, "20m beacon window");
        assert_eq!(mode_from_mhz(28.250), None, "10m IBP beacon band");
        assert_eq!(mode_from_mhz(5.300), None, "60m: no segment plan here");
        assert_eq!(mode_from_mhz(7.060), None, "40m gap between the segments");
        assert_eq!(mode_from_mhz(432.100), None, "70cm: not modelled");
        assert_eq!(mode_from_mhz(13.999), None, "not a band at all");
    }

    #[test]
    fn no_phone_is_ever_inferred_on_30m() {
        // 30m is CW/digital only everywhere; a phone inference there would
        // be a plain bug rather than a regional disagreement.
        for f in [10.100, 10.110, 10.130, 10.140, 10.149] {
            assert_ne!(mode_from_mhz(f), Some("SSB"), "{f} MHz");
        }
    }
}
