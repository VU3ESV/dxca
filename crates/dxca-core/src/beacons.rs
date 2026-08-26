//! Known-beacon lookup — port of the Swift `BeaconDatabase`. Labels beacon
//! spots so they never masquerade as DX opportunities (the classifier
//! returns level None + is_beacon for these).

pub struct BeaconInfo {
    pub call: &'static str,
    pub location: &'static str,
    /// "NCDXF" for the rotating IBP network, None otherwise.
    pub network: Option<&'static str>,
}

/// The 18 NCDXF/IBP rotating beacons (14.100/18.110/21.150/24.930/28.200)
/// plus commonly spotted national beacons — copied verbatim from Swift.
#[rustfmt::skip]
static BEACONS: &[BeaconInfo] = &[
    BeaconInfo { call: "4U1UN",  location: "United Nations, New York", network: Some("NCDXF") },
    BeaconInfo { call: "VE8AT",  location: "Eureka, Canada",           network: Some("NCDXF") },
    BeaconInfo { call: "W6WX",   location: "Mt Umunhum, California",   network: Some("NCDXF") },
    BeaconInfo { call: "KH6RS",  location: "Maui, Hawaii",             network: Some("NCDXF") },
    BeaconInfo { call: "KH6WO",  location: "Maui, Hawaii",             network: Some("NCDXF") },
    BeaconInfo { call: "ZL6B",   location: "Masterton, New Zealand",   network: Some("NCDXF") },
    BeaconInfo { call: "VK6RBP", location: "Rolystone, Australia",     network: Some("NCDXF") },
    BeaconInfo { call: "JA2IGY", location: "Mt Asama, Japan",          network: Some("NCDXF") },
    BeaconInfo { call: "RR9O",   location: "Novosibirsk, Russia",      network: Some("NCDXF") },
    BeaconInfo { call: "VR2B",   location: "Hong Kong",                network: Some("NCDXF") },
    BeaconInfo { call: "4S7B",   location: "Colombo, Sri Lanka",       network: Some("NCDXF") },
    BeaconInfo { call: "ZS6DN",  location: "Pretoria, South Africa",   network: Some("NCDXF") },
    BeaconInfo { call: "5Z4B",   location: "Nairobi, Kenya",           network: Some("NCDXF") },
    BeaconInfo { call: "4X6TU",  location: "Tel Aviv, Israel",         network: Some("NCDXF") },
    BeaconInfo { call: "OH2B",   location: "Espoo, Finland",           network: Some("NCDXF") },
    BeaconInfo { call: "CS3B",   location: "Madeira, Portugal",        network: Some("NCDXF") },
    BeaconInfo { call: "LU4AA",  location: "Buenos Aires, Argentina",  network: Some("NCDXF") },
    BeaconInfo { call: "OA4B",   location: "Lima, Peru",               network: Some("NCDXF") },
    BeaconInfo { call: "YV5B",   location: "Caracas, Venezuela",       network: Some("NCDXF") },
    BeaconInfo { call: "DK0WCY",  location: "Scheggerott, Germany (HF/aurora)",   network: None },
    BeaconInfo { call: "ZL2VHM",  location: "Mt Climie, New Zealand (50/144MHz)", network: None },
    BeaconInfo { call: "LX0HF",   location: "Luxembourg HF beacon",               network: None },
    BeaconInfo { call: "LX0FOUR", location: "Luxembourg 4M beacon",               network: None },
    BeaconInfo { call: "GB3RAL",  location: "Didcot, England (LF beacon)",        network: None },
    BeaconInfo { call: "GB3VHF",  location: "Wrotham, England (144 MHz)",         network: None },
    BeaconInfo { call: "GB3MCB",  location: "St Austell, England (50/70MHz)",     network: None },
    BeaconInfo { call: "GB3SCS",  location: "Sandwich, England (28MHz)",          network: None },
    BeaconInfo { call: "OZ7IGY",  location: "Tolløse, Denmark (multi-band)",      network: None },
    BeaconInfo { call: "OK0EG",   location: "Praděd, Czech Republic (50MHz)",     network: None },
    BeaconInfo { call: "F5ZCB",   location: "Saint-Loubès, France (50MHz)",       network: None },
    BeaconInfo { call: "DB0ANN",  location: "Nuremberg, Germany (HF beacon)",     network: None },
];

/// Look up a callsign (any /suffix stripped). None if not a known beacon.
pub fn lookup(call: &str) -> Option<&'static BeaconInfo> {
    let upper = call.to_ascii_uppercase();
    let bare = upper.split('/').next().unwrap_or(&upper);
    BEACONS.iter().find(|b| b.call == bare)
}

/// True for the /B and /BCN suffix convention (e.g. IT9ATQ/B), a strong
/// beacon hint even when the call isn't in the explicit list.
pub fn has_beacon_suffix(call: &str) -> bool {
    let upper = call.to_ascii_uppercase();
    match upper.split_once('/') {
        Some((_, suffix)) => suffix == "B" || suffix == "BCN",
        None => false,
    }
}

/// Human label for a beacon spot, or None if the call isn't beacon-like.
pub fn display_name(call: &str) -> Option<String> {
    if let Some(info) = lookup(call) {
        return Some(match info.network {
            Some(net) => format!("{net} Beacon — {}", info.location),
            None => format!("Beacon — {}", info.location),
        });
    }
    if has_beacon_suffix(call) {
        return Some("Beacon (/B suffix)".to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ncdxf_and_suffix_matching() {
        assert_eq!(
            display_name("4X6TU").as_deref(),
            Some("NCDXF Beacon — Tel Aviv, Israel")
        );
        assert_eq!(
            display_name("oh2b/b").as_deref(),
            Some("NCDXF Beacon — Espoo, Finland")
        );
        assert_eq!(
            display_name("IT9ATQ/B").as_deref(),
            Some("Beacon (/B suffix)")
        );
        assert_eq!(
            display_name("IT9ATQ/BCN").as_deref(),
            Some("Beacon (/B suffix)")
        );
        assert_eq!(display_name("VU2CPL"), None);
        assert!(!has_beacon_suffix("VU2CPL"));
    }
}
