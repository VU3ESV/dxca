//! Frequency → ADIF band name ("20M", "40M", …) — port of the Swift
//! `BandResolver`, table values copied verbatim.

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

/// Does a band score for the DXCC Challenge?
pub fn is_challenge_band(band: &str) -> bool {
    CHALLENGE_BANDS.contains(&band)
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
}
