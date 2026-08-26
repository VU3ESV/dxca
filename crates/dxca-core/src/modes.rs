//! ADIF mode string → award bucket ("CW" / "PHONE" / "DATA") — port of the
//! Swift `ModeNormalizer`. FT8, FT4, RTTY, JT65 etc. all collapse to DATA
//! so digital modes share one DXCC slot, matching DXCC/LoTW/ClubLog rules.

pub fn canonical(raw: &str) -> &'static str {
    let mode = raw.trim().to_ascii_uppercase();
    if mode.is_empty() {
        return "DATA";
    }
    match mode.as_str() {
        "CW" => "CW",
        "SSB" | "USB" | "LSB" | "AM" | "FM" | "PHONE" | "VOICE" | "DIGITALVOICE" | "C4FM"
        | "DMR" | "DSTAR" => "PHONE",
        _ => "DATA",
    }
}

#[cfg(test)]
mod tests {
    use super::canonical;

    #[test]
    fn buckets() {
        assert_eq!(canonical("CW"), "CW");
        assert_eq!(canonical("ssb"), "PHONE");
        assert_eq!(canonical("FT8"), "DATA");
        assert_eq!(canonical("FT4"), "DATA");
        assert_eq!(canonical("RTTY"), "DATA");
        assert_eq!(canonical(" ft8 "), "DATA");
        assert_eq!(canonical(""), "DATA");
    }
}
