//! ADIF v1/v2 text parser — port of the Swift `ADIFParser`.
//!
//! Format: `<TAG:length[:type]>value`, records terminated by `<eor>`, an
//! optional header ended by `<eoh>`. Two Swift behaviours are preserved
//! deliberately (golden parity with the 1.x app's matrix build):
//!  - lengths count **characters**, not bytes (the Swift parser slices an
//!    `Array(content)` of `Character`s) — matters for non-ASCII names;
//!  - header fields before `<eoh>` are stored and end up in the first
//!    record (the Swift parser never cleared them on `<eoh>`); harmless in
//!    practice because headers carry no CALL/BAND/MODE.

use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Record {
    pub fields: HashMap<String, String>,
}

impl Record {
    fn upper(&self, key: &str) -> Option<String> {
        self.fields.get(key).map(|v| v.to_uppercase())
    }

    pub fn call(&self) -> Option<String> {
        self.upper("CALL")
    }

    pub fn band(&self) -> Option<String> {
        self.upper("BAND")
    }

    pub fn mode(&self) -> Option<String> {
        self.upper("MODE")
    }

    pub fn dxcc(&self) -> Option<i32> {
        self.fields.get("DXCC")?.parse().ok()
    }

    pub fn grid_square(&self) -> Option<String> {
        self.upper("GRIDSQUARE")
    }

    pub fn qso_date(&self) -> Option<&str> {
        self.fields.get("QSO_DATE").map(String::as_str)
    }

    /// Confirmed if LoTW/QSL/eQSL received is Y (or V = verified), or
    /// ClubLog's own matched flag is set.
    pub fn is_confirmed(&self) -> bool {
        for key in ["LOTW_QSL_RCVD", "QSL_RCVD", "EQSL_QSL_RCVD"] {
            if let Some(v) = self.upper(key)
                && (v == "Y" || v == "V")
            {
                return true;
            }
        }
        self.upper("APP_CLUBLOG_QSO_QSL").as_deref() == Some("Y")
    }
}

pub fn parse(content: &str) -> Vec<Record> {
    let chars: Vec<char> = content.chars().collect();
    let n = chars.len();
    let mut records = Vec::new();
    let mut current = Record::default();
    let mut i = 0;

    while i < n {
        let Some(lt) = find_next(&chars, i, '<') else {
            break;
        };
        let Some(gt) = find_next(&chars, lt + 1, '>') else {
            break;
        };
        let tag: String = chars[lt + 1..gt].iter().collect();
        let lower = tag.to_lowercase();

        if lower == "eoh" {
            i = gt + 1;
            continue;
        }
        if lower == "eor" {
            if !current.fields.is_empty() {
                records.push(std::mem::take(&mut current));
            }
            i = gt + 1;
            continue;
        }

        // TAG:length or TAG:length:type
        let mut parts = tag.splitn(3, ':');
        let name = parts.next().unwrap_or("").to_uppercase();
        let Some(length) = parts.next().and_then(|s| s.parse::<usize>().ok()) else {
            i = gt + 1;
            continue;
        };

        let value_start = gt + 1;
        let value_end = (value_start + length).min(n);
        let value: String = chars[value_start..value_end].iter().collect();
        current.fields.insert(name, value);
        i = value_end;
    }

    if !current.fields.is_empty() {
        records.push(current);
    }
    records
}

fn find_next(chars: &[char], from: usize, c: char) -> Option<usize> {
    chars[from..].iter().position(|&x| x == c).map(|p| from + p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_records_and_flushes_trailing() {
        let adif = "<ADIF_VER:5>3.1.0<eoh>\
                    <CALL:6>VU2CPL<BAND:3>20M<MODE:3>FT8<LOTW_QSL_RCVD:1>Y<eor>\n\
                    <CALL:4>P5DX<BAND:3>40M<MODE:2>CW";
        let recs = parse(adif);
        assert_eq!(recs.len(), 2);
        // Header field leaks into the first record — Swift-parity behaviour.
        assert_eq!(
            recs[0].fields.get("ADIF_VER").map(String::as_str),
            Some("3.1.0")
        );
        assert_eq!(recs[0].call().as_deref(), Some("VU2CPL"));
        assert!(recs[0].is_confirmed());
        // Trailing record without <eor> is flushed.
        assert_eq!(recs[1].call().as_deref(), Some("P5DX"));
        assert!(!recs[1].is_confirmed());
    }

    #[test]
    fn lengths_count_characters_not_bytes() {
        // "Tolløse" is 7 characters but 8 UTF-8 bytes.
        let adif = "<NAME:7>Tolløse<CALL:6>OZ7IGY<eor>";
        let recs = parse(adif);
        assert_eq!(
            recs[0].fields.get("NAME").map(String::as_str),
            Some("Tolløse")
        );
        assert_eq!(recs[0].call().as_deref(), Some("OZ7IGY"));
    }

    #[test]
    fn confirmed_variants() {
        for (field, val, expect) in [
            ("QSL_RCVD", "V", true),
            ("EQSL_QSL_RCVD", "y", true),
            ("APP_CLUBLOG_QSO_QSL", "Y", true),
            ("QSL_RCVD", "N", false),
            ("QSL_SENT", "Y", false),
        ] {
            let adif = format!("<CALL:4>TEST<{field}:{}>{val}<eor>", val.len());
            assert_eq!(parse(&adif)[0].is_confirmed(), expect, "{field}={val}");
        }
    }

    #[test]
    fn malformed_tags_are_skipped() {
        let recs = parse("<NOLEN><CALL:6>VU2CPL<BAD:x>zz<eor>");
        assert_eq!(recs[0].call().as_deref(), Some("VU2CPL"));
        assert!(!recs[0].fields.contains_key("NOLEN"));
    }
}
