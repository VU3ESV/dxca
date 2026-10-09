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
//! * **Only a success is written down** (Manoj, 2026-10-09). Success is HTTP
//!   200 with a release record carrying a `tag_name`, newer or not; it stores
//!   the record and the time. A failure — offline, timeout, any HTTP error
//!   including GitHub's rate-limit 403, a body that is not a release — writes
//!   nothing to the database, so the next start or the next hourly look
//!   simply tries again. The first version stamped every *attempt* before the
//!   request (the `refresh.rs` arrangement), which turned one bad minute into
//!   a day without an answer; `refresh.rs` moves megabytes per attempt and
//!   needs that floor, this moves one small record.
//!
//! * **An hour between failed automatic attempts, in memory.** Without a
//!   stored stamp something still has to stop an offline install asking
//!   every tick; the back-off is that, and it lives only as long as the
//!   process. It uses the monotonic clock: a Pi's wall clock jumps when NTP
//!   first syncs, and the jump must neither cut the hour short nor stretch it.
//!
//! * **Development builds never ask by themselves.** A version containing
//!   "dev" (any case) gets no automatic check — a build on a bench is not an
//!   install that needs telling, and it would spend the address's shared
//!   GitHub allowance. *Check now* still works, and `DXCA_UPDATE_TEST_VERSION`
//!   replaces the version this is decided on, as it does the comparison.
//!
//! * **Silent when it fails.** An automatic check that cannot reach GitHub is
//!   not news: no banner, no log line. The reason is kept in memory for the
//!   Server card (Settings › Server › Reference data), which is where someone
//!   asking "why does it never say anything" will look.
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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// When the last check *succeeded* (unix). Nothing else moves it — see the
/// module docs. Named for what it holds: the first version kept the time of
/// every attempt under `update_last_check_unix`, which this replaces (that
/// version never ran on any install).
const LAST_SUCCESS_KEY: &str = "update_last_success_unix";
/// The last good answer, in GitHub's field names minus the notes — read on
/// every status frame, so it is kept small.
const LATEST_KEY: &str = "update_latest_release";
/// That release's notes, read only by the Server card.
const NOTES_KEY: &str = "update_latest_notes";
/// The tag an admin chose to skip. The banner stays away for that tag only —
/// the next release brings it back.
const SKIPPED_KEY: &str = "update_skipped_tag";

/// Delay before the first look after start, so a check never competes with
/// cluster logins and listener binds for the first seconds of a boot.
const FIRST_LOOK_AFTER: Duration = Duration::from_secs(30);
/// How often the loop runs [`auto_check_due`]. The rule decides; a look that
/// is not due costs one `meta` read.
const TICK: Duration = Duration::from_secs(60 * 60);
/// A day from the last *successful* check to the next automatic one.
const INTERVAL_SECS: i64 = 24 * 60 * 60;
/// After a failed automatic attempt, the least wait before the next.
const BACKOFF: Duration = Duration::from_secs(60 * 60);

/// Test hook, inert unless set: pretend the running version is this one, so
/// the banner can be seen against the real latest release without cutting
/// one. `DXCA_UPDATE_TEST_VERSION=0.0.1 ./dxca`. Only the comparison, the
/// development-build rule and what the UI is told change — the User-Agent
/// always carries the real version.
pub const TEST_VERSION_ENV: &str = "DXCA_UPDATE_TEST_VERSION";

/// The version releases are compared against.
pub fn current_version() -> String {
    match std::env::var(TEST_VERSION_ENV) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// A development build: the version says "dev" anywhere, in any case
/// (`2.23.0-dev`, `2.23.0-DEV.3`). Manoj's rule, 2026-10-09.
fn is_dev_build(version: &str) -> bool {
    version.to_ascii_lowercase().contains("dev")
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before 1970")
        .as_secs() as i64
}

/// Whether the loop should ask GitHub now — the whole automatic schedule in
/// one pure function (Manoj, 2026-10-09):
///
/// * the setting is on, and the running version is not a development build;
/// * a day has passed since the last **successful** check (`0` = never);
/// * no automatic attempt has failed in the last hour (`since_failure` is
///   `None` when none has, in this process).
///
/// The day is measured by the wall clock because it is stored; a stamp a day
/// or more in the *future* can only have been written by a clock that was
/// wrong at the time — a Pi has no RTC — and without the `abs` it would hold
/// the check off until the real clock caught up with it.
fn auto_check_due(
    enabled: bool,
    version: &str,
    now_unix: i64,
    last_success_unix: i64,
    since_failure: Option<Duration>,
) -> bool {
    enabled
        && !is_dev_build(version)
        && (now_unix - last_success_unix).abs() >= INTERVAL_SECS
        && since_failure.is_none_or(|d| d >= BACKOFF)
}

/// The last attempt that failed, automatic or *Check now*.
struct Failure {
    at_unix: i64,
    error: String,
}

/// The release check: the database for what is durable, and memory for the
/// one thing that must not be — why the last attempt failed. Shared by the
/// loop and the API (`AppState::update`).
pub struct Checker {
    db: Arc<Db>,
    /// For the Server card only. In memory because a failure writes nothing
    /// durable (module docs); cleared by the next success.
    failure: Mutex<Option<Failure>>,
}

impl Checker {
    pub fn new(db: Arc<Db>) -> Self {
        Self {
            db,
            failure: Mutex::new(None),
        }
    }

    /// Ask GitHub now. Blocking. Used by the loop and by the Server card's
    /// *Check now*, which is why it returns the error rather than swallowing
    /// it: a person who pressed a button is owed a reason. Either way a
    /// failure moves no stamp, so *Check now* failing does not delay the
    /// next automatic check either.
    pub fn check(&self) -> Result<Release, String> {
        self.check_from(gh::LATEST_URL, gh::TIMEOUT)
    }

    fn check_from(&self, url: &str, timeout: Duration) -> Result<Release, String> {
        match gh::fetch_latest(url, env!("CARGO_PKG_VERSION"), timeout) {
            Ok(r) => {
                let mut head = r.to_json();
                head["body"] = serde_json::Value::String(String::new());
                let _ = self.db.meta_set(LATEST_KEY, &head.to_string());
                let _ = self.db.meta_set(NOTES_KEY, &r.body);
                // The time last: a crash between the writes leaves the old
                // stamp, which only means one more check, never a day lost.
                let _ = self.db.meta_set_now(LAST_SUCCESS_KEY);
                *self.failure.lock().unwrap() = None;
                Ok(r)
            }
            Err(e) => {
                // Nothing durable: the previous answer is still the best
                // thing known, and the next attempt must not be held off.
                *self.failure.lock().unwrap() = Some(Failure {
                    at_unix: now_unix(),
                    error: e.clone(),
                });
                Err(e)
            }
        }
    }

    fn last_success(&self) -> i64 {
        self.db.meta_unix(LAST_SUCCESS_KEY)
    }

    /// One look by the loop: check if [`auto_check_due`] says so, and keep
    /// the back-off — set when this automatic attempt fails, cleared when one
    /// succeeds. `failed_at` belongs to the loop: a failed *Check now* does
    /// not start the hour. Returns whether GitHub was asked. Blocking.
    fn auto_look(
        &self,
        url: &str,
        timeout: Duration,
        enabled: bool,
        version: &str,
        now: Instant,
        failed_at: &mut Option<Instant>,
    ) -> bool {
        let since_failure = failed_at.map(|t| now.saturating_duration_since(t));
        if !auto_check_due(
            enabled,
            version,
            now_unix(),
            self.last_success(),
            since_failure,
        ) {
            return false;
        }
        // Stamped when the failure is known, not when the attempt began, so
        // the hour runs from the end of a slow one.
        *failed_at = match self.check_from(url, timeout) {
            Ok(_) => None,
            Err(_) => Some(Instant::now()),
        };
        true
    }

    /// Everything the Server card shows (admin-only `GET /api/update`).
    pub fn detail_json(&self, enabled: bool) -> serde_json::Value {
        let current = current_version();
        let latest = stored(&self.db);
        let (last_error, last_error_unix) = match &*self.failure.lock().unwrap() {
            Some(f) => (f.error.clone(), f.at_unix),
            None => (String::new(), 0),
        };
        serde_json::json!({
            "enabled": enabled,
            // Whether the loop asks by itself at all: off, or a development
            // build, and only *Check now* ever does.
            "automatic": enabled && !is_dev_build(&current),
            "current": current,
            "newer": latest.as_ref().is_some_and(|r| gh::is_newer(&r.tag, &current)),
            "latest": latest.map(|r| serde_json::json!({
                "tag": r.tag,
                "version": r.version(),
                "name": r.name,
                "url": r.url,
                "notes": self.db.meta_get(NOTES_KEY).ok().flatten().unwrap_or_default(),
            })),
            "skipped": skipped(&self.db),
            "last_success_unix": self.last_success(),
            "last_error": last_error,
            "last_error_unix": last_error_unix,
        })
    }
}

/// Start the automatic check. With `check_for_updates = false`, or on a
/// development build, nothing is spawned and nothing is ever requested; an
/// admin can still press *Check now* on the Server card.
pub fn spawn(checker: Arc<Checker>, enabled: bool) {
    let version = current_version();
    // The same two conditions `auto_check_due` starts with; checked here as
    // well so that a loop which could never ask is not left running.
    if !enabled || is_dev_build(&version) {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(FIRST_LOOK_AFTER).await;
        // The tag already written to the log by this process: one line per
        // new release per run, not one per hourly look.
        let mut announced: Option<String> = None;
        // When this run's last automatic attempt failed — the back-off.
        let mut failed_at: Option<Instant> = None;
        loop {
            let (c, v, prev) = (checker.clone(), version.clone(), failed_at);
            // A failed attempt says nothing (the module docs say why); a
            // success is reported below from what it stored. A panic in the
            // blocking task counts as a failure, so it is not retried hourly
            // at full speed either.
            failed_at = tokio::task::spawn_blocking(move || {
                let mut f = prev;
                c.auto_look(
                    gh::LATEST_URL,
                    gh::TIMEOUT,
                    enabled,
                    &v,
                    Instant::now(),
                    &mut f,
                );
                f
            })
            .await
            .unwrap_or_else(|_| Some(Instant::now()));
            if let Some(r) = available(&checker.db)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    const DAY: i64 = 86_400;
    const NOW: i64 = 1_800_000_000;
    const HOUR: Duration = Duration::from_secs(3600);
    /// Long enough for a loopback answer, short enough that the timeout case
    /// does not cost the gate ten seconds.
    const QUICK: Duration = Duration::from_millis(400);

    fn temp_db() -> (Arc<Db>, std::path::PathBuf) {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "dxca-update-test-{}-{}.db",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&path);
        (Arc::new(Db::open(&path).unwrap()), path)
    }

    /// Every `meta` key this module has ever written, including the two the
    /// first version used (`update_last_check_unix`, `update_last_error`) —
    /// a failure must bring none of them back.
    fn persisted(db: &Db) -> Vec<(&'static str, Option<String>)> {
        [
            LAST_SUCCESS_KEY,
            LATEST_KEY,
            NOTES_KEY,
            SKIPPED_KEY,
            "update_last_check_unix",
            "update_last_error",
        ]
        .into_iter()
        .map(|k| (k, db.meta_get(k).unwrap()))
        .collect()
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

    /// "Offline": refused at once. Port 1 needs root to listen on, so — unlike
    /// a port bound and released — no parallel test's one-shot server can be
    /// handed it and have its single request stolen.
    const DEAD_URL: &str = "http://127.0.0.1:1/latest";

    /// A server that accepts and never answers, until the handle is joined.
    fn silent_server() -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            std::thread::sleep(QUICK * 3);
            drop(s);
        });
        (url, handle)
    }

    const RELEASE: &str = r#"{"tag_name":"v2.23.0","name":"v2.23.0 — something",
        "html_url":"https://github.com/vu2cpl/dxca/releases/tag/v2.23.0",
        "body":"Notes.","assets":[]}"#;

    /// One answer from `fake_github`, through the checker, server joined.
    fn answer(c: &Checker, status: u16, body: &str) -> Result<Release, String> {
        let (url, server) = fake_github(status, body);
        let r = c.check_from(&url, QUICK);
        server.join().unwrap();
        r
    }

    // --- the schedule --------------------------------------------------------

    #[test]
    fn checks_a_day_after_the_last_success() {
        assert!(auto_check_due(true, "2.22.1", NOW, 0, None), "never");
        assert!(
            !auto_check_due(true, "2.22.1", NOW, NOW - DAY + 60, None),
            "23h59m ago"
        );
        assert!(
            auto_check_due(true, "2.22.1", NOW, NOW - DAY, None),
            "a day"
        );
    }

    /// A Pi boots on a guessed clock. A stamp written while it was a day or
    /// more fast would otherwise silence the check until real time caught up;
    /// a stamp a little in the future (fake-hwclock) is still "just checked".
    #[test]
    fn a_stamp_from_a_wrong_clock_does_not_silence_the_check() {
        assert!(auto_check_due(true, "2.22.1", NOW, NOW + 30 * DAY, None));
        assert!(!auto_check_due(true, "2.22.1", NOW, NOW + 600, None));
    }

    /// After a failed automatic attempt the next waits an hour — and the hour
    /// never shortens the day after a success.
    #[test]
    fn a_failed_attempt_holds_the_next_off_for_an_hour() {
        let due = |since| auto_check_due(true, "2.22.1", NOW, 0, since);
        assert!(due(None), "no failure this run");
        assert!(!due(Some(Duration::ZERO)), "just failed");
        assert!(!due(Some(HOUR - Duration::from_secs(1))), "59m59s");
        assert!(due(Some(HOUR)), "an hour");
        assert!(due(Some(5 * HOUR)));
        assert!(
            !auto_check_due(true, "2.22.1", NOW, NOW - 2 * 3600, Some(2 * HOUR)),
            "succeeded two hours ago: the day rules"
        );
    }

    #[test]
    fn switched_off_never_checks_by_itself() {
        assert!(!auto_check_due(false, "2.22.1", NOW, 0, None));
    }

    /// Manoj's rule: "dev" anywhere in the version, in any case, and the loop
    /// never asks. Other suffixes (an `-rc`) are not development builds.
    #[test]
    fn a_development_build_never_checks_by_itself() {
        for v in [
            "2.23.0-dev",
            "2.23.0-DEV",
            "2.23.0-Dev.3",
            "dev",
            "2.23.0+devel",
        ] {
            assert!(is_dev_build(v), "{v}");
            assert!(!auto_check_due(true, v, NOW, 0, None), "{v}");
        }
        for v in ["2.22.1", "2.23.0-rc1", "0.0.1"] {
            assert!(!is_dev_build(v), "{v}");
            assert!(auto_check_due(true, v, NOW, 0, None), "{v}");
        }
    }

    /// The loop's own step: a failure arms the hour for this process only, a
    /// restart (a new `Checker`, nothing in memory) tries again at once, and
    /// a success clears the hour and starts the day.
    #[test]
    fn the_loop_backs_off_after_a_failure_and_a_restart_tries_again() {
        let (db, path) = temp_db();
        let c = Checker::new(db.clone());
        let mut failed_at = None;
        let t0 = Instant::now();
        assert!(c.auto_look(DEAD_URL, QUICK, true, "2.22.1", t0, &mut failed_at));
        assert!(failed_at.is_some(), "a failure starts the back-off");
        assert!(
            !c.auto_look(DEAD_URL, QUICK, true, "2.22.1", t0, &mut failed_at),
            "within the hour: not asked"
        );

        // The process restarts: the back-off was memory, the database holds
        // nothing about the failure, so the first look asks.
        let c = Checker::new(db.clone());
        let mut failed_at = None;
        let (url, server) = fake_github(200, RELEASE);
        assert!(c.auto_look(&url, QUICK, true, "2.22.1", t0, &mut failed_at));
        server.join().unwrap();
        assert!(failed_at.is_none(), "a success clears the back-off");
        assert!(
            !c.auto_look(
                DEAD_URL,
                QUICK,
                true,
                "2.22.1",
                t0 + 2 * HOUR,
                &mut failed_at
            ),
            "a success holds the next one off for a day"
        );

        // A failure an hour old no longer holds anything off.
        let (db2, path2) = temp_db();
        let c = Checker::new(db2);
        let mut failed_at = Some(Instant::now());
        let later = Instant::now() + HOUR;
        let (url, server) = fake_github(200, RELEASE);
        assert!(c.auto_look(&url, QUICK, true, "2.22.1", later, &mut failed_at));
        server.join().unwrap();

        // Never checked, nothing failed — and still, off or a development
        // build, the loop's step does not ask.
        let (db3, path3) = temp_db();
        let c = Checker::new(db3);
        let mut none = None;
        assert!(!c.auto_look(DEAD_URL, QUICK, false, "2.22.1", t0, &mut none));
        assert!(!c.auto_look(DEAD_URL, QUICK, true, "2.23.0-dev", t0, &mut none));
        assert!(none.is_none());
        assert!(c.detail_json(true)["last_error"] == "", "nothing was tried");
        for p in [path, path2, path3] {
            let _ = std::fs::remove_file(p);
        }
    }

    // --- what a check stores -------------------------------------------------

    /// The request is GitHub's JSON media type, a `DXCA/<version>` agent and
    /// nothing else — no token, no cookie, nothing about the install. A
    /// success stores the record and the time.
    #[test]
    fn a_successful_check_is_stored_and_offered() {
        let (db, path) = temp_db();
        let c = Checker::new(db.clone());
        let (url, server) = fake_github(200, RELEASE);
        let r = c.check_from(&url, QUICK).unwrap();
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
        assert!((db.meta_unix(LAST_SUCCESS_KEY) - now_unix()).abs() < 5);
        let s = status_for(&db, true, "2.22.1");
        assert_eq!(s["version"], "2.23.0");
        assert_eq!(s["current"], "2.22.1");
        assert_eq!(s["skipped"], false);
        let d = c.detail_json(true);
        assert_eq!(d["latest"]["notes"], "Notes.");
        assert_eq!(d["last_success_unix"], db.meta_unix(LAST_SUCCESS_KEY));
        assert_eq!(d["last_error"], "");
        let _ = std::fs::remove_file(path);
    }

    /// "Newer or not": GitHub naming the version already running is still a
    /// successful check, and still starts the day.
    #[test]
    fn an_answer_that_is_not_newer_is_still_a_success() {
        let (db, path) = temp_db();
        let c = Checker::new(db.clone());
        let same = r#"{"tag_name":"v2.22.1"}"#;
        answer(&c, 200, same).unwrap();
        assert!(db.meta_unix(LAST_SUCCESS_KEY) > 0);
        assert!(status_for(&db, true, "2.22.1").is_null());
        let _ = std::fs::remove_file(path);
    }

    /// Offline, timed out, any HTTP error (the rate-limit 403 included), a
    /// 2xx that is not 200, a body that is not a release: the database is
    /// left exactly as it was — no stamp, so the next start or the next look
    /// asks again; the last good answer stays on offer; the reason is kept
    /// for the Server card. Same for *Check now*, which is this path.
    #[test]
    fn a_failed_check_writes_nothing_durable() {
        let (db, path) = temp_db();
        let c = Checker::new(db.clone());
        let empty = persisted(&db);

        let (silent, held) = silent_server();
        // (what failed, a piece of the reason that proves it was that).
        let failures: Vec<(&str, Result<Release, String>)> = vec![
            ("Connection refused", c.check_from(DEAD_URL, QUICK)),
            ("timed out", c.check_from(&silent, QUICK)),
            ("HTTP 403", answer(&c, 403, r#"{"message":"forbidden"}"#)),
            ("HTTP 404", answer(&c, 404, r#"{"message":"Not Found"}"#)),
            ("HTTP 500", answer(&c, 500, "")),
            ("HTTP 203", answer(&c, 203, RELEASE)),
            ("JSON", answer(&c, 200, "not json")),
            ("no tag_name", answer(&c, 200, r#"{"name":"x"}"#)),
        ];
        held.join().unwrap();
        for (what, r) in &failures {
            let e = r.as_ref().expect_err(what);
            assert!(e.contains(what), "{what}: {e}");
            assert_eq!(persisted(&db), empty, "{what} wrote to the database");
        }
        assert_eq!(c.last_success(), 0);
        assert!(
            auto_check_due(true, "2.22.1", now_unix(), c.last_success(), None),
            "still due"
        );

        // The same with a good answer already stored: nothing moves.
        answer(&c, 200, RELEASE).unwrap();
        let before = persisted(&db);
        let e = answer(&c, 403, r#"{"message":"forbidden"}"#).unwrap_err();
        assert!(e.contains("403"), "{e}");
        assert_eq!(persisted(&db), before);
        assert_eq!(status_for(&db, true, "2.22.1")["tag"], "v2.23.0");
        let d = c.detail_json(true);
        assert_eq!(d["last_error"], e);
        assert!((d["last_error_unix"].as_i64().unwrap() - now_unix()).abs() < 5);
        assert_eq!(d["latest"]["tag"], "v2.23.0");

        // GitHub's rate-limit 403 says so in a header; the reason shown on
        // the Server card must say "wait", not read like a block.
        let (url, server) = fake_github_with(
            403,
            "X-RateLimit-Remaining: 0\r\n",
            r#"{"message":"API rate limit exceeded"}"#,
        );
        let e = c.check_from(&url, QUICK).unwrap_err();
        server.join().unwrap();
        assert!(e.contains("hourly limit"), "{e}");
        assert_eq!(persisted(&db), before);

        // The next success clears the reason.
        answer(&c, 200, RELEASE).unwrap();
        assert_eq!(c.detail_json(true)["last_error"], "");
        assert_eq!(c.detail_json(true)["last_error_unix"], 0);
        let _ = std::fs::remove_file(path);
    }

    /// After the upgrade the stored release is the one running: the notice
    /// must go away with nothing to clear by hand.
    #[test]
    fn the_notice_ends_when_the_install_catches_up() {
        let (db, path) = temp_db();
        let c = Checker::new(db.clone());
        assert!(
            status_for(&db, true, "2.22.1").is_null(),
            "nothing stored yet"
        );
        answer(&c, 200, RELEASE).unwrap();
        assert!(!status_for(&db, true, "2.22.1").is_null());
        assert!(status_for(&db, true, "2.23.0").is_null());
        assert!(status_for(&db, true, "2.24.0").is_null());
        assert_eq!(c.detail_json(true)["latest"]["tag"], "v2.23.0");
        let _ = std::fs::remove_file(path);
    }

    /// Skip holds for that tag only, and switching the check off hides even
    /// an answer stored before it was switched off.
    #[test]
    fn skip_is_per_release_and_off_means_off() {
        let (db, path) = temp_db();
        let c = Checker::new(db.clone());
        answer(&c, 200, RELEASE).unwrap();

        set_skipped(&db, "v2.23.0").unwrap();
        assert_eq!(status_for(&db, true, "2.22.1")["skipped"], true);
        set_skipped(&db, "v2.22.9").unwrap();
        assert_eq!(status_for(&db, true, "2.22.1")["skipped"], false);
        set_skipped(&db, "").unwrap();
        assert_eq!(c.detail_json(true)["skipped"], "");

        assert!(status_for(&db, false, "2.22.1").is_null());
        assert_eq!(c.detail_json(false)["automatic"], false);
        let _ = std::fs::remove_file(path);
    }
}
