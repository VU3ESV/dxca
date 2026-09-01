//! IOTA directory download — `groups.json` from iota-world.org
//! (`docs/AWARDS.md` phase 3).
//!
//! The directory's terms are "personal non-commercial home use", so it is
//! **downloaded at runtime and never bundled** — the same arrangement
//! cty.xml and the LoTW users list have always had. `groups.json` (287 KB)
//! carries `refno` + `name` for every group, which is all validation and
//! display need; the 1.3 MB `fulllist.json` adds per-island detail DXCA
//! has no use for.

use std::collections::HashMap;
use std::io::Read;

pub const DEFAULT_URL: &str =
    "https://www.iota-world.org/islands-on-the-air/downloads/download-file.html?path=groups.json";

/// Download the raw groups.json text.
pub fn download(url: &str) -> Result<String, String> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .map_err(|e| format!("IOTA download: {e}"))?;
    let mut out = String::new();
    resp.into_reader()
        .take(16 * 1024 * 1024)
        .read_to_string(&mut out)
        .map_err(|e| format!("IOTA read: {e}"))?;
    Ok(out)
}

/// The parsed directory: reference → group name.
#[derive(Debug, Clone, Default)]
pub struct IotaDirectory {
    names: HashMap<String, String>,
}

impl IotaDirectory {
    /// Parse groups.json — an array of `{ "refno": "AF-001", "name": … }`
    /// objects. Refuses a parse that yields implausibly few groups (the
    /// live directory has ~1,180), so an error page or a format change can
    /// never silently replace a working directory with an empty one — the
    /// same guard `refresh_lotw` applies to an empty users list.
    pub fn parse(text: &str) -> Result<IotaDirectory, String> {
        let val: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("IOTA parse: {e}"))?;
        let arr = val.as_array().ok_or("IOTA parse: not a JSON array")?;
        let mut names = HashMap::new();
        for item in arr {
            let Some(refno) = item.get("refno").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(norm) = dxca_core::awards::normalize_iota(refno) else {
                continue;
            };
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            names.insert(norm, name.to_string());
        }
        if names.len() < 500 {
            return Err(format!(
                "IOTA parse: only {} groups — refusing a suspiciously small directory",
                names.len()
            ));
        }
        Ok(IotaDirectory { names })
    }

    pub fn is_valid(&self, reference: &str) -> bool {
        self.names.contains_key(reference)
    }

    /// The group's name ("Agalega Islands"), for display.
    pub fn name(&self, reference: &str) -> Option<&str> {
        self.names.get(reference).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_refno_and_name_and_refuses_small() {
        // The size guard means a realistic test needs bulk — generate it.
        let mut items: Vec<String> = (1..=600)
            .map(|i| format!(r#"{{"refno":"AS-{i:03}","name":"Group {i}"}}"#))
            .collect();
        items.push(r#"{"refno":"nonsense","name":"skipped"}"#.into());
        let text = format!("[{}]", items.join(","));
        let dir = IotaDirectory::parse(&text).unwrap();
        assert_eq!(dir.len(), 600, "malformed refno skipped");
        assert!(dir.is_valid("AS-003"));
        assert_eq!(dir.name("AS-003"), Some("Group 3"));
        assert!(!dir.is_valid("EU-001"));

        assert!(IotaDirectory::parse("[]").is_err(), "empty refused");
        assert!(IotaDirectory::parse("<html>err</html>").is_err());
    }
}
