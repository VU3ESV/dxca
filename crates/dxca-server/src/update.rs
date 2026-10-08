//! The release check, server side: when to ask GitHub, what to remember, and
//! what the web UI is told. The request itself is `dxca_connect::update`.
//!
//! An operator finds out about a new DXCA today by reading GitHub or being
//! told — and the installs that most need the fix (other people's Pis, the
//! Windows box) are the ones nobody looks at. So the server asks, and says so
//! where its admin already looks: a banner across the web UI and one line in
//! the service log. It never downloads or installs anything.
//!
//! Design notes, in the order they bit:
//!
//! * **Once a day, stamped before the request.** The stamp is written before
//!   the outcome is known and persisted, so a failing check — offline, rate
//!   limited, a proxy eating TLS — is still once a day, and a crash-looping
//!   service cannot turn into a request per restart. Same arrangement as
//!   `refresh.rs`.
//!
//! * **Silent when it fails.** An automatic check that cannot reach GitHub is
//!   not news: no banner, no log line. The error is kept for the Server card
//!   (Settings › Server › Reference data), which is where someone asking
//!   "why does it never say anything" will look.
//!
//! * **The last good answer survives a failure and a restart.** Stored in
//!   `meta`, so the banner is there from the first page load after a restart
//!   rather than a day later, and a check that fails tomorrow does not
//!   withdraw a notice that is still true.
//!
//! * **"Newer" is decided at read time**, against the running binary. After
//!   the upgrade the stored record is the version now running, so the notice
//!   goes away by itself with nothing to clear.

use crate::db::Db;
use dxca_connect::update::{self as gh, Release};
use std::sync::Arc;
use std::time::Duration;

/// When the last check was *attempted* (unix), success or not.
const LAST_CHECK_KEY: &str = "update_last_check_unix";
/// The last good answer, in GitHub's field names minus the notes — read on
/// every status frame, so it is kept small.
const LATEST_KEY: &str = "update_latest_release";
/// That release's notes, read only by the Server card.
const NOTES_KEY: &str = "update_latest_notes";
/// Why the last check failed; empty after a success.
const ERROR_KEY: &str = "update_last_error";
/// The tag an admin chose to skip. The banner stays away for that tag only —
/// the next release brings it back.
const SKIPPED_KEY: &str = "update_skipped_tag";

/// Delay before the first look after start, so a check never competes with
/// cluster logins and listener binds for the first seconds of a boot.
const FIRST_LOOK_AFTER: Duration = Duration::from_secs(30);
/// How often the loop looks at the stamp. The stamp decides; this is cheap.
const TICK: Duration = Duration::from_secs(60 * 60);
const INTERVAL_SECS: i64 = 24 * 60 * 60;

/// Test hook, inert unless set: pretend the running version is this one, so
/// the banner can be seen against the real latest release without cutting
/// one. `DXCA_UPDATE_TEST_VERSION=0.0.1 ./dxca`. Only the comparison and what
/// the UI is told change — the User-Agent always carries the real version.
pub const TEST_VERSION_ENV: &str = "DXCA_UPDATE_TEST_VERSION";

/// The version releases are compared against.
pub fn current_version() -> String {
    match std::env::var(TEST_VERSION_ENV) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => env!("CARGO_PKG_VERSION").to_string(),
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before 1970")
        .as_secs() as i64
}

/// Whether a day has passed since the last attempt. A stamp a day or more in
/// the *future* can only have been written by a clock that was wrong at the
/// time — a Pi has no RTC — and without the `abs` it would hold the check off
/// until the real clock caught up with it.
fn is_due(now: i64, last_attempt: i64) -> bool {
    (now - last_attempt).abs() >= INTERVAL_SECS
}

/// Start the daily check. With `check_for_updates = false` nothing is
/// spawned and nothing is ever requested; an admin can still press
/// *Check now* on the Server card.
pub fn spawn(db: Arc<Db>, enabled: bool) {
    if !enabled {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(FIRST_LOOK_AFTER).await;
        // The tag already written to the log by this process: one line per
        // new release per run, not one per hourly look.
        let mut announced: Option<String> = None;
        loop {
            if is_due(now_unix(), db.meta_unix(LAST_CHECK_KEY)) {
                let db = db.clone();
                // The result is deliberately dropped: an automatic check that
                // fails says nothing (the module docs say why), and a success
                // is reported below from what it stored.
                let _ = tokio::task::spawn_blocking(move || check(&db)).await;
            }
            if let Some(r) = available(&db)
                && announced.as_deref() != Some(r.tag.as_str())
            {
                println!(
                    "dxca: DXCA {} is available (you have {}) — {}",
                    r.version(),
                    current_version(),
                    r.url
                );
                announced = Some(r.tag);
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

/// Ask GitHub now and remember the answer. Blocking. Used by the daily loop
/// and by the Server card's *Check now*, which is why it returns the error
/// rather than swallowing it: a person who pressed a button is owed a reason.
pub fn check(db: &Db) -> Result<Release, String> {
    check_from(db, gh::LATEST_URL)
}

fn check_from(db: &Db, url: &str) -> Result<Release, String> {
    let _ = db.meta_set_now(LAST_CHECK_KEY);
    match gh::fetch_latest(url, env!("CARGO_PKG_VERSION")) {
        Ok(r) => {
            let mut head = r.to_json();
            head["body"] = serde_json::Value::String(String::new());
            let _ = db.meta_set(LATEST_KEY, &head.to_string());
            let _ = db.meta_set(NOTES_KEY, &r.body);
            let _ = db.meta_set(ERROR_KEY, "");
            Ok(r)
        }
        Err(e) => {
            // The previous answer is left alone: it is still the best thing
            // known, and a flaky night must not withdraw a true notice.
            let _ = db.meta_set(ERROR_KEY, &e);
            Err(e)
        }
    }
}

/// The last release GitHub reported, without its notes.
fn stored(db: &Db) -> Option<Release> {
    let text = db.meta_get(LATEST_KEY).ok().flatten()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    gh::parse_release(&v).ok()
}

fn skipped(db: &Db) -> String {
    db.meta_get(SKIPPED_KEY).ok().flatten().unwrap_or_default()
}

/// The stored release, if it is newer than `current`.
fn available_for(db: &Db, current: &str) -> Option<Release> {
    stored(db).filter(|r| gh::is_newer(&r.tag, current))
}

fn available(db: &Db) -> Option<Release> {
    available_for(db, &current_version())
}

/// Remember (or, with an empty tag, forget) the release an admin skipped.
pub fn set_skipped(db: &Db, tag: &str) -> Result<(), String> {
    db.meta_set(SKIPPED_KEY, tag.trim())
}

/// What `/api/status` carries: `null`, or the release the banner offers.
/// `null` too when the check is switched off — an operator who turned it
/// off asked not to be told, including by an answer stored before that.
pub fn status_json(db: &Db, enabled: bool) -> serde_json::Value {
    status_for(db, enabled, &current_version())
}

fn status_for(db: &Db, enabled: bool, current: &str) -> serde_json::Value {
    if !enabled {
        return serde_json::Value::Null;
    }
    match available_for(db, current) {
        None => serde_json::Value::Null,
        Some(r) => serde_json::json!({
            "tag": r.tag,
            "version": r.version(),
            "name": r.name,
            "url": r.url,
            "current": current,
            "skipped": skipped(db) == r.tag,
        }),
    }
}

/// Everything the Server card shows (admin-only `GET /api/update`).
pub fn detail_json(db: &Db, enabled: bool) -> serde_json::Value {
    let current = current_version();
    let latest = stored(db);
    serde_json::json!({
        "enabled": enabled,
        "current": current,
        "newer": latest.as_ref().is_some_and(|r| gh::is_newer(&r.tag, &current)),
        "latest": latest.map(|r| serde_json::json!({
            "tag": r.tag,
            "version": r.version(),
            "name": r.name,
            "url": r.url,
            "notes": db.meta_get(NOTES_KEY).ok().flatten().unwrap_or_default(),
        })),
        "skipped": skipped(db),
        "last_check_unix": db.meta_unix(LAST_CHECK_KEY),
        "last_error": db.meta_get(ERROR_KEY).ok().flatten().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    const DAY: i64 = 86_400;
    const NOW: i64 = 1_800_000_000;

    fn temp_db() -> (Db, std::path::PathBuf) {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "dxca-update-test-{}-{}.db",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&path);
        (Db::open(&path).unwrap(), path)
    }

    /// A one-shot HTTP server answering a single request with `status` and
    /// `body`; returns the base URL and a handle yielding the request bytes.
    fn fake_github(status: u16, body: &str) -> (String, std::thread::JoinHandle<String>) {
        fake_github_with(status, "", body)
    }

    /// As [`fake_github`], with extra response header lines (`"Name: v\r\n"`).
    fn fake_github_with(
        status: u16,
        headers: &str,
        body: &str,
    ) -> (String, std::thread::JoinHandle<String>) {
        let headers = headers.to_string();
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        let body = body.to_string();
        let handle = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            // Read to the end of the headers — one read may not be all of them.
            let mut req = Vec::new();
            let mut buf = [0u8; 1024];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = s.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
            }
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{headers}\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            s.write_all(reply.as_bytes()).unwrap();
            String::from_utf8_lossy(&req).into_owned()
        });
        (url, handle)
    }

    const RELEASE: &str = r#"{"tag_name":"v2.23.0","name":"v2.23.0 — something",
        "html_url":"https://github.com/vu2cpl/dxca/releases/tag/v2.23.0",
        "body":"Notes.","assets":[]}"#;

    #[test]
    fn checks_once_a_day() {
        assert!(is_due(NOW, 0), "never checked");
        assert!(!is_due(NOW, NOW - DAY + 60), "23h59m ago");
        assert!(is_due(NOW, NOW - DAY), "exactly a day");
    }

    /// A Pi boots on a guessed clock. A stamp written while it was a day or
    /// more fast would otherwise silence the check until real time caught up;
    /// a stamp a little in the future (fake-hwclock) is still "just checked".
    #[test]
    fn a_stamp_from_a_wrong_clock_does_not_silence_the_check() {
        assert!(is_due(NOW, NOW + 30 * DAY));
        assert!(!is_due(NOW, NOW + 600));
    }

    /// The request is GitHub's JSON media type, a `DXCA/<version>` agent and
    /// nothing else — no token, no cookie, nothing about the install.
    #[test]
    fn a_successful_check_is_stored_and_offered() {
        let (db, path) = temp_db();
        let (url, server) = fake_github(200, RELEASE);
        let r = check_from(&db, &url).unwrap();
        let req = server.join().unwrap().to_lowercase();
        assert!(req.starts_with("get /latest "), "{req}");
        assert!(req.contains("accept: application/vnd.github+json"), "{req}");
        assert!(
            req.contains(&format!("user-agent: dxca/{}", env!("CARGO_PKG_VERSION"))),
            "{req}"
        );
        assert!(!req.contains("authorization"), "{req}");
        assert!(!req.contains("cookie"), "{req}");

        assert_eq!(r.tag, "v2.23.0");
        assert!(db.meta_unix(LAST_CHECK_KEY) > 0);
        let s = status_for(&db, true, "2.22.1");
        assert_eq!(s["version"], "2.23.0");
        assert_eq!(s["current"], "2.22.1");
        assert_eq!(s["skipped"], false);
        let d = detail_json(&db, true);
        assert_eq!(d["latest"]["notes"], "Notes.");
        assert_eq!(d["last_error"], "");
        let _ = std::fs::remove_file(path);
    }

    /// Rate limited, offline, garbage: the stamp still advances (so it is not
    /// retried until tomorrow), the reason is kept for the Server card, and
    /// the answer from the last good check stays on offer.
    #[test]
    fn a_failed_check_keeps_the_last_good_answer() {
        let (db, path) = temp_db();
        let (url, server) = fake_github(200, RELEASE);
        check_from(&db, &url).unwrap();
        server.join().unwrap();
        db.meta_set(LAST_CHECK_KEY, "0").unwrap();

        let (url, server) = fake_github(403, r#"{"message":"API rate limit exceeded"}"#);
        let e = check_from(&db, &url).unwrap_err();
        server.join().unwrap();
        assert!(e.contains("403"), "{e}");
        assert!(
            db.meta_unix(LAST_CHECK_KEY) > 0,
            "a failure is still an attempt"
        );
        assert_eq!(detail_json(&db, true)["last_error"], e);
        assert_eq!(status_for(&db, true, "2.22.1")["tag"], "v2.23.0");

        // GitHub's rate-limit 403 says so in a header; the reason shown on
        // the Server card must say "wait", not read like a block.
        let (url, server) = fake_github_with(
            403,
            "X-RateLimit-Remaining: 0\r\n",
            r#"{"message":"API rate limit exceeded"}"#,
        );
        let e = check_from(&db, &url).unwrap_err();
        server.join().unwrap();
        assert!(e.contains("hourly limit"), "{e}");

        let (url, server) = fake_github(200, "not json");
        assert!(check_from(&db, &url).is_err());
        server.join().unwrap();
        assert_eq!(status_for(&db, true, "2.22.1")["tag"], "v2.23.0");
        let _ = std::fs::remove_file(path);
    }

    /// After the upgrade the stored release is the one running: the notice
    /// must go away with nothing to clear by hand.
    #[test]
    fn the_notice_ends_when_the_install_catches_up() {
        let (db, path) = temp_db();
        assert!(
            status_for(&db, true, "2.22.1").is_null(),
            "nothing stored yet"
        );
        let (url, server) = fake_github(200, RELEASE);
        check_from(&db, &url).unwrap();
        server.join().unwrap();
        assert!(!status_for(&db, true, "2.22.1").is_null());
        assert!(status_for(&db, true, "2.23.0").is_null());
        assert!(status_for(&db, true, "2.24.0").is_null());
        assert_eq!(detail_json(&db, true)["latest"]["tag"], "v2.23.0");
        let _ = std::fs::remove_file(path);
    }

    /// Skip holds for that tag only, and switching the check off hides even
    /// an answer stored before it was switched off.
    #[test]
    fn skip_is_per_release_and_off_means_off() {
        let (db, path) = temp_db();
        let (url, server) = fake_github(200, RELEASE);
        check_from(&db, &url).unwrap();
        server.join().unwrap();

        set_skipped(&db, "v2.23.0").unwrap();
        assert_eq!(status_for(&db, true, "2.22.1")["skipped"], true);
        set_skipped(&db, "v2.22.9").unwrap();
        assert_eq!(status_for(&db, true, "2.22.1")["skipped"], false);
        set_skipped(&db, "").unwrap();
        assert_eq!(detail_json(&db, true)["skipped"], "");

        assert!(status_for(&db, false, "2.22.1").is_null());
        let _ = std::fs::remove_file(path);
    }
}
