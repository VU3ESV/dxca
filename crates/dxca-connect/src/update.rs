//! "Is there a newer DXCA?" — one read of GitHub's latest-release record.
//!
//! The network half of the release check; scheduling, storage and the web UI
//! notice live in `dxca-server`'s `update.rs`. Blocking (ureq) like every
//! other client in this crate; callers run it on a blocking task.
//!
//! What it deliberately is not:
//!
//! * **Not an updater.** It reads one JSON record and reports. Nothing is
//!   downloaded or installed — DXCA runs as a service on Pis, Docker,
//!   Windows and macOS, each with its own install route (README
//!   *Updating*), and an unattended binary swap on someone else's Pi is
//!   exactly the kind of decision this program does not take for them.
//! * **Not authenticated.** No token: there is nothing to keep one in that
//!   would not ship to every install, and the public endpoint's 60 requests
//!   an hour per address is sixty times what a once-a-day check needs.
//! * **Not a second server.** No Sparkle-style appcast to host and keep in
//!   step with the tags — the GitHub release *is* the announcement, so the
//!   record this reads cannot disagree with what was actually published.

use std::io::Read;
use std::time::Duration;

/// GitHub's latest-release record for this repository. `/releases/latest`
/// never returns a draft or a pre-release, so an `-rc` tag cannot reach an
/// install through here however the version comparison would rank it.
pub const LATEST_URL: &str = "https://api.github.com/repos/vu2cpl/dxca/releases/latest";

/// Where the link points when the record's own `html_url` is missing or is
/// not a github.com page (see [`parse_release`]).
const RELEASES_PAGE: &str = "https://github.com/vu2cpl/dxca/releases/latest";

/// Release notes are shown in the web UI, not read here, so they only need
/// to be bounded. GitHub allows 125,000 characters; a body that long is not
/// release notes anyone reads in a settings card.
const BODY_MAX_BYTES: usize = 32 * 1024;

/// The four fields DXCA reads from a release; everything else in GitHub's
/// record (assets, author, reactions, …) is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// `tag_name`, as tagged: `v2.22.1`.
    pub tag: String,
    /// The release title; the tag when the release has none.
    pub name: String,
    /// The release page — notes and the Windows zip.
    pub url: String,
    /// The release notes (Markdown), capped at [`BODY_MAX_BYTES`].
    pub body: String,
}

impl Release {
    /// The tag without its leading `v`, for "DXCA 2.23.0 is available".
    pub fn version(&self) -> &str {
        strip_v(&self.tag)
    }

    /// Back to GitHub's own field names. The server stores the last good
    /// answer in this shape so that [`parse_release`] reads both — one parser,
    /// so a stored record and a fresh one cannot be read two different ways.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "tag_name": self.tag,
            "name": self.name,
            "html_url": self.url,
            "body": self.body,
        })
    }
}

/// How long one check may take, connect to last byte. The server passes
/// this; its tests pass a fraction of a second, so the timeout case is
/// exercised without a ten-second test.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// Fetch the latest release. `version` goes into the User-Agent, which
/// GitHub requires and which names the caller honestly in their logs.
///
/// `Ok` means exactly one thing: HTTP 200 and a JSON record with a
/// `tag_name` — whether or not that release is newer. Everything else
/// (offline, timeout, any other status, a body that is not a release) is an
/// `Err`, and the server records nothing durable for it (Manoj, 2026-10-09).
///
/// Exactly two headers and no credentials: the request carries nothing about
/// the install it comes from beyond the version it is running.
//
// clippy::result_large_err: the `ureq::Error` is turned into a String in the
// same expression, as in telegram.rs — it never leaves this function.
#[allow(clippy::result_large_err)]
pub fn fetch_latest(url: &str, version: &str, timeout: Duration) -> Result<Release, String> {
    let resp = ureq::get(url)
        .timeout(timeout)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", &format!("DXCA/{version}"))
        .call()
        .map_err(|e| match e {
            // The unauthenticated limit is 60 an hour per *public address*,
            // shared by every machine behind the same router and every tool
            // on them that asks GitHub anything without a token — so it can
            // be spent before DXCA's one request a day arrives. It was, on
            // the day this was written. A bare "HTTP 403" reads like a
            // block; this says it is a queue that empties within the hour.
            ureq::Error::Status(code @ (403 | 429), resp)
                if resp.header("x-ratelimit-remaining") == Some("0") =>
            {
                format!(
                    "GitHub's hourly limit for this address is used up (HTTP {code}) — \
                     it resets within the hour"
                )
            }
            // Any other 403 (a block), or 404 (no published release yet):
            // "no answer today", which the caller treats like any error.
            ureq::Error::Status(code, _) => format!("GitHub answered HTTP {code}"),
            other => format!("GitHub: {other}"),
        })?;
    // ureq hands back every 2xx (and follows redirects — a renamed repo
    // answers 301 — before getting here). GitHub's answer to this request is
    // 200; a 203 from a caching proxy or a 204 is not GitHub's record, and
    // counting it as a success would hold the next check off for a day.
    if resp.status() != 200 {
        return Err(format!("GitHub answered HTTP {}", resp.status()));
    }
    // The record is a few KB; a megabyte is far past any real one and keeps
    // a misbehaving proxy from feeding the parser without end.
    let mut text = String::new();
    resp.into_reader()
        .take(1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|e| format!("GitHub read: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("GitHub JSON: {e}"))?;
    parse_release(&v)
}

/// Read the four fields out of a release record (GitHub's, or the copy the
/// server stored).
///
/// `tag_name` is the only required one — without it there is nothing to
/// compare. The link is accepted only as an `https://github.com/` page:
/// it becomes an `href` in the web UI, and a `javascript:` URL there would
/// run in an admin's session. Anything else falls back to the releases page,
/// which is where the link would have led anyway.
pub fn parse_release(v: &serde_json::Value) -> Result<Release, String> {
    let text = |key: &str| v.get(key).and_then(|x| x.as_str()).unwrap_or("").trim();
    let tag = text("tag_name");
    if tag.is_empty() {
        return Err("release record has no tag_name".into());
    }
    let name = match text("name") {
        "" => tag,
        n => n,
    };
    let url = match text("html_url") {
        u if u.starts_with("https://github.com/") => u,
        _ => RELEASES_PAGE,
    };
    Ok(Release {
        tag: tag.to_string(),
        name: name.to_string(),
        url: url.to_string(),
        body: truncate(
            v.get("body").and_then(|x| x.as_str()).unwrap_or(""),
            BODY_MAX_BYTES,
        ),
    })
}

fn strip_v(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix(['v', 'V']).unwrap_or(s)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// A version as a tuple of integers: leading `v` stripped, split on every
/// non-digit, empty pieces dropped. `v2.22.1` → `[2, 22, 1]`.
///
/// Deliberately not semver. The tags are `vMAJOR.MINOR.PATCH` and nothing
/// else, and a parser that rejects what it does not expect would turn a
/// slightly odd tag into "no update, ever" in silence; this one always
/// produces *something* comparable. A tag with no digits at all reads as
/// `[]`, which compares as 0.0.0 and so can never be "newer".
pub fn version_key(v: &str) -> Vec<u64> {
    strip_v(v)
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        // Twenty-odd digits in one component is no real version; saturating
        // keeps it orderable instead of dropping it and shifting the rest.
        .map(|p| p.parse().unwrap_or(u64::MAX))
        .collect()
}

/// True when `candidate` is a strictly higher version than `current`,
/// comparing [`version_key`]s with missing components read as 0 — so
/// `2.23` equals `2.23.0`, and an equal version is never an update.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (a, b) = (version_key(candidate), version_key(current));
    let n = a.len().max(b.len());
    let at = |k: &[u64], i: usize| k.get(i).copied().unwrap_or(0);
    for i in 0..n {
        match at(&a, i).cmp(&at(&b, i)) {
            std::cmp::Ordering::Equal => continue,
            other => return other.is_gt(),
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The comparison is numeric per component. A string compare ranks
    /// "2.9.0" above "2.10.0", which would offer every install on 2.10+ a
    /// "newer" 2.9 — and the next minor after this one is exactly that shape.
    #[test]
    fn components_compare_as_numbers_not_strings() {
        assert!(is_newer("v2.10.0", "2.9.0"));
        assert!(!is_newer("v2.9.0", "2.10.0"));
        assert!(is_newer("v2.22.10", "2.22.9"));
        assert!(is_newer("v3.0.0", "2.99.99"));
    }

    /// The running version is `CARGO_PKG_VERSION` (no `v`) and the tag has
    /// one: the same release must compare equal, or every install would be
    /// told about the version it is already running.
    #[test]
    fn the_running_release_is_not_an_update_to_itself() {
        assert!(!is_newer("v2.22.1", "2.22.1"));
        assert!(!is_newer("V2.22.1", "v2.22.1"));
        assert!(!is_newer("2.22.1", "2.22.1"));
        assert!(is_newer("v2.22.2", "2.22.1"));
        assert!(!is_newer("v2.22.0", "2.22.1"), "older is never newer");
    }

    /// Missing components are zeros, so a two-part tag neither outranks nor
    /// undercuts its three-part spelling.
    #[test]
    fn missing_components_read_as_zero() {
        assert!(!is_newer("v2.23", "2.23.0"));
        assert!(!is_newer("v2.23.0", "2.23"));
        assert!(is_newer("v2.23.0.1", "2.23"));
        assert!(is_newer("v2.23", "2.22.9"));
    }

    #[test]
    fn version_key_splits_on_every_non_digit() {
        assert_eq!(version_key("v2.22.1"), vec![2, 22, 1]);
        assert_eq!(version_key(" 2.22.1 "), vec![2, 22, 1]);
        assert_eq!(version_key("v2.23.0-rc1"), vec![2, 23, 0, 1]);
        assert_eq!(version_key("release"), Vec::<u64>::new());
        assert_eq!(version_key(""), Vec::<u64>::new());
    }

    /// A tag with no number in it must never read as an update — it compares
    /// as 0.0.0, which nothing real is below.
    #[test]
    fn a_tag_without_digits_is_never_newer() {
        assert!(!is_newer("latest", "2.22.1"));
        assert!(!is_newer("", "0.0.1"));
    }

    /// The shape GitHub actually returns (trimmed), including fields this
    /// module ignores.
    #[test]
    fn parses_a_github_record() {
        let v = serde_json::json!({
            "tag_name": "v2.22.1",
            "name": "v2.22.1 — a KG4 call with a three-letter suffix is the USA",
            "html_url": "https://github.com/vu2cpl/dxca/releases/tag/v2.22.1",
            "body": "A bug fix.",
            "draft": false,
            "prerelease": false,
            "assets": [{"name": "dxca-2.22.1-windows-x64.zip"}],
        });
        let r = parse_release(&v).unwrap();
        assert_eq!(r.tag, "v2.22.1");
        assert_eq!(r.version(), "2.22.1");
        assert!(r.name.starts_with("v2.22.1 — "));
        assert_eq!(r.url, "https://github.com/vu2cpl/dxca/releases/tag/v2.22.1");
        assert_eq!(r.body, "A bug fix.");
        // What the server stores reads back as the same release.
        assert_eq!(parse_release(&r.to_json()).unwrap(), r);
    }

    #[test]
    fn a_record_without_a_tag_is_an_error() {
        assert!(parse_release(&serde_json::json!({"name": "x"})).is_err());
        assert!(parse_release(&serde_json::json!({"tag_name": "  "})).is_err());
        assert!(parse_release(&serde_json::json!([])).is_err());
        assert!(parse_release(&serde_json::json!({"message": "API rate limit exceeded"})).is_err());
    }

    /// The link becomes an `href` in an admin's browser. Anything that is not
    /// a github.com page — `javascript:`, plain http, another host — is
    /// replaced, never passed through.
    #[test]
    fn only_a_github_page_is_accepted_as_the_link() {
        for bad in [
            "javascript:alert(1)",
            "http://github.com/vu2cpl/dxca/releases/tag/v9",
            "https://example.com/dxca",
            "https://github.com.evil.example/x",
            "",
        ] {
            let v = serde_json::json!({"tag_name": "v9.0.0", "html_url": bad});
            assert_eq!(parse_release(&v).unwrap().url, RELEASES_PAGE, "{bad}");
        }
    }

    /// The real endpoint, by hand only:
    /// `cargo test -p dxca-connect -- --ignored --nocapture live_`.
    /// Kept out of the gate — a gate that needs GitHub up and the address's
    /// rate limit unspent would go red for reasons that are not this code.
    #[test]
    #[ignore = "network: reads the real GitHub API"]
    fn live_latest_release_is_read() {
        let r = fetch_latest(LATEST_URL, env!("CARGO_PKG_VERSION"), TIMEOUT).unwrap();
        println!("latest: {} | {} | {}", r.tag, r.name, r.url);
        assert!(!version_key(&r.tag).is_empty(), "{}", r.tag);
        assert!(
            r.url
                .starts_with("https://github.com/vu2cpl/dxca/releases/tag/")
        );
        assert!(!r.body.is_empty());
    }

    #[test]
    fn a_missing_title_falls_back_to_the_tag_and_long_notes_are_capped() {
        let long = "é".repeat(BODY_MAX_BYTES); // two bytes each: cut mid-char
        let r = parse_release(&serde_json::json!({"tag_name": "v3.0.0", "body": long})).unwrap();
        assert_eq!(r.name, "v3.0.0");
        assert!(r.body.len() <= BODY_MAX_BYTES + '…'.len_utf8());
        assert!(r.body.ends_with('…'));
    }
}
