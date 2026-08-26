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
    fn boundaries_are_inclusive_and_gaps_are_none() {
        assert_eq!(band_from_mhz(14.0), Some("20M"));
        assert_eq!(band_from_mhz(14.35), Some("20M"));
        assert_eq!(band_from_mhz(13.999), None);
        assert_eq!(band_from_mhz(2.5), None); // between 160M and 80M
    }
}
