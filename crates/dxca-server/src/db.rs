//! SQLite persistence (plan §4/§5): users, sessions, per-user config, and
//! per-user matrix cache. One bundled-SQLite connection behind a mutex —
//! shack-scale traffic, no pool needed. Secrets (ClubLog app password,
//! Telegram token) live here in plain text by design; the file is created
//! 0600 and the trade-off is documented in the README (plan §5).

use dxca_core::classify::{AlertConfig, AlertLevel};
use dxca_core::matrix::LogMatrix;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

/// Per-user ClubLog settings — the 1.x `ClubLogConfig` fields that matter
/// server-side, with the alert toggles flattened in.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClubLogUserConfig {
    pub callsign: String,
    pub email: String,
    pub app_password: String,
    pub api_key: String,
    /// Automatic re-download interval in hours; **0 = manual only**.
    /// Per-user because each account pulls its own log with its own
    /// credentials — unlike the LoTW list, which is one shared file.
    #[serde(default = "default_refresh_hours")]
    pub refresh_hours: i64,
    #[serde(flatten)]
    pub alerts: AlertConfigOpt,
}

/// Daily. A log that only moves when someone remembers the button means
/// today's QSOs keep alerting as New DXCC tomorrow; ClubLog's own ADIF
/// export is not something to pull much harder than this.
fn default_refresh_hours() -> i64 {
    24
}

// Hand-written rather than derived: `Default` is what a brand-new account
// gets, and serde's per-field default is what an OLD stored row gets for a
// key it predates. Deriving would have made those disagree — 0 (manual) for
// the new user, 24 for the existing one.
impl Default for ClubLogUserConfig {
    fn default() -> Self {
        ClubLogUserConfig {
            callsign: String::new(),
            email: String::new(),
            app_password: String::new(),
            api_key: String::new(),
            refresh_hours: default_refresh_hours(),
            alerts: AlertConfigOpt::default(),
        }
    }
}

/// AlertConfig with serde defaults matching 1.x for the `New*` half; the
/// `Unconf*` half defaults off, so an existing account behaves exactly as it
/// did until the operator ticks something.
///
/// The 1.x `alert_unconfirmed` switch is **gone**. Stored rows may still
/// carry the key — serde ignores unknown fields, so they deserialize fine —
/// and it needs no migration: it swapped the whole comparison to the
/// confirmed sets, which the four `alert_unconf_*` levels now express
/// directly and, unlike the switch, alongside the `New*` half.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertConfigOpt {
    pub alert_new_dxcc: bool,
    pub alert_new_slot: bool,
    pub alert_new_band: bool,
    pub alert_new_mode: bool,
    pub alert_unconf_dxcc: bool,
    pub alert_unconf_slot: bool,
    pub alert_unconf_band: bool,
    pub alert_unconf_mode: bool,
}

impl Default for AlertConfigOpt {
    fn default() -> Self {
        let d = AlertConfig::default();
        AlertConfigOpt {
            alert_new_dxcc: d.alert_new_dxcc,
            alert_new_slot: d.alert_new_slot,
            alert_new_band: d.alert_new_band,
            alert_new_mode: d.alert_new_mode,
            alert_unconf_dxcc: d.alert_unconf_dxcc,
            alert_unconf_slot: d.alert_unconf_slot,
            alert_unconf_band: d.alert_unconf_band,
            alert_unconf_mode: d.alert_unconf_mode,
        }
    }
}

impl From<&AlertConfigOpt> for AlertConfig {
    fn from(o: &AlertConfigOpt) -> AlertConfig {
        AlertConfig {
            alert_new_dxcc: o.alert_new_dxcc,
            alert_new_slot: o.alert_new_slot,
            alert_new_band: o.alert_new_band,
            alert_new_mode: o.alert_new_mode,
            alert_unconf_dxcc: o.alert_unconf_dxcc,
            alert_unconf_slot: o.alert_unconf_slot,
            alert_unconf_band: o.alert_unconf_band,
            alert_unconf_mode: o.alert_unconf_mode,
        }
    }
}

/// Per-user notification settings — the 1.x `NotificationConfig` minus the
/// macOS system notifications (headless server).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifyUserConfig {
    pub telegram_enabled: bool,
    pub telegram_bot_token: String,
    pub telegram_chat_id: String,
    pub cooldown_minutes: i64,
    pub notify_new_dxcc: bool,
    pub notify_new_slot: bool,
    pub notify_new_band: bool,
    pub notify_new_mode: bool,
    // DXCA 2.1: the confirmation-hunting half, off by default like the
    // classifier's. A level ticked here still only fires if the classifier is
    // allowed to flag it at all (My ClubLog) — notify narrows, never widens.
    pub notify_unconf_dxcc: bool,
    pub notify_unconf_slot: bool,
    pub notify_unconf_band: bool,
    pub notify_unconf_mode: bool,
    // DXCA 2.1: band / mode-class narrowing for Telegram only. **Empty means
    // ALL** — the same convention `broadcast_destinations.sources` uses, and
    // the reason a fresh account is not silent. Bands are resolver names
    // ("20M"), modes are award buckets ("CW"/"PHONE"/"DATA").
    pub notify_bands: Vec<String>,
    pub notify_modes: Vec<String>,
}

impl Default for NotifyUserConfig {
    fn default() -> Self {
        NotifyUserConfig {
            telegram_enabled: false,
            telegram_bot_token: String::new(),
            telegram_chat_id: String::new(),
            cooldown_minutes: 15,
            notify_new_dxcc: true,
            notify_new_slot: true,
            notify_new_band: true,
            notify_new_mode: true,
            notify_unconf_dxcc: false,
            notify_unconf_slot: false,
            notify_unconf_band: false,
            notify_unconf_mode: false,
            notify_bands: Vec::new(),
            notify_modes: Vec::new(),
        }
    }
}

impl NotifyUserConfig {
    /// Does this spot's band/mode survive the Telegram narrowing? Empty list
    /// = no narrowing on that axis.
    pub fn passes_band_mode(&self, band: Option<&str>, mode_class: &str) -> bool {
        let band_ok = self.notify_bands.is_empty()
            || band.is_some_and(|b| self.notify_bands.iter().any(|x| x == b));
        let mode_ok =
            self.notify_modes.is_empty() || self.notify_modes.iter().any(|x| x == mode_class);
        band_ok && mode_ok
    }

    /// Whether this level is wanted, over all eight flaggable levels.
    pub fn wants_level(&self, level: AlertLevel) -> bool {
        match level {
            AlertLevel::NewDxcc => self.notify_new_dxcc,
            AlertLevel::NewSlot => self.notify_new_slot,
            AlertLevel::NewBand => self.notify_new_band,
            AlertLevel::NewMode => self.notify_new_mode,
            AlertLevel::UnconfDxcc => self.notify_unconf_dxcc,
            AlertLevel::UnconfSlot => self.notify_unconf_slot,
            AlertLevel::UnconfBand => self.notify_unconf_band,
            AlertLevel::UnconfMode => self.notify_unconf_mode,
            AlertLevel::Worked | AlertLevel::None => false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub callsign: String,
    pub display_name: String,
    pub role: String,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    callsign TEXT UNIQUE NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    pass_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'user',
    created_unix INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_unix INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS user_configs (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    clublog_json TEXT NOT NULL DEFAULT '{}',
    notify_json TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS matrices (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    matrix_json TEXT NOT NULL,
    qso_count INTEGER NOT NULL,
    last_refresh_unix INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before 1970")
        .as_secs() as i64
}

type DbResult<T> = Result<T, String>;

fn db_err<E: std::fmt::Display>(e: E) -> String {
    format!("db: {e}")
}

impl Db {
    pub fn open(path: &Path) -> DbResult<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(db_err)?;
        }
        let conn = Connection::open(path).map_err(db_err)?;
        conn.execute_batch(SCHEMA).map_err(db_err)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(db_err)?;
        // Secrets at rest: owner-only, plan §5.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    pub fn user_count(&self) -> DbResult<i64> {
        self.conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
            .map_err(db_err)
    }

    pub fn create_user(
        &self,
        callsign: &str,
        display_name: &str,
        pass_hash: &str,
        role: &str,
    ) -> DbResult<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (callsign, display_name, pass_hash, role, created_unix)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                callsign.to_uppercase(),
                display_name,
                pass_hash,
                role,
                now_unix()
            ],
        )
        .map_err(|e| format!("create user: {e}"))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn users(&self) -> DbResult<Vec<User>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, callsign, display_name, role FROM users ORDER BY id")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(User {
                    id: r.get(0)?,
                    callsign: r.get(1)?,
                    display_name: r.get(2)?,
                    role: r.get(3)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<_, _>>().map_err(db_err)
    }

    /// (user, stored password hash) by callsign, case-insensitive.
    pub fn user_by_callsign(&self, callsign: &str) -> DbResult<Option<(User, String)>> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT id, callsign, display_name, role, pass_hash FROM users WHERE callsign = ?1",
                params![callsign.to_uppercase()],
                |r| {
                    Ok((
                        User {
                            id: r.get(0)?,
                            callsign: r.get(1)?,
                            display_name: r.get(2)?,
                            role: r.get(3)?,
                        },
                        r.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(db_err)
    }

    // --- sessions ---------------------------------------------------------

    pub fn create_session(&self, token_hash: &str, user_id: i64, ttl_secs: i64) -> DbResult<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO sessions (token_hash, user_id, expires_unix) VALUES (?1, ?2, ?3)",
                params![token_hash, user_id, now_unix() + ttl_secs],
            )
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn session_user(&self, token_hash: &str) -> DbResult<Option<User>> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT u.id, u.callsign, u.display_name, u.role
                 FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ?1 AND s.expires_unix > ?2",
                params![token_hash, now_unix()],
                |r| {
                    Ok(User {
                        id: r.get(0)?,
                        callsign: r.get(1)?,
                        display_name: r.get(2)?,
                        role: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(db_err)
    }

    pub fn delete_session(&self, token_hash: &str) -> DbResult<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM sessions WHERE token_hash = ?1",
                params![token_hash],
            )
            .map(|_| ())
            .map_err(db_err)
    }

    // --- per-user config --------------------------------------------------

    pub fn clublog_config(&self, user_id: i64) -> DbResult<ClubLogUserConfig> {
        self.config_json(user_id, "clublog_json")
    }

    pub fn set_clublog_config(&self, user_id: i64, cfg: &ClubLogUserConfig) -> DbResult<()> {
        self.set_config_json(user_id, "clublog_json", cfg)
    }

    pub fn notify_config(&self, user_id: i64) -> DbResult<NotifyUserConfig> {
        self.config_json(user_id, "notify_json")
    }

    pub fn set_notify_config(&self, user_id: i64, cfg: &NotifyUserConfig) -> DbResult<()> {
        self.set_config_json(user_id, "notify_json", cfg)
    }

    fn config_json<T: serde::de::DeserializeOwned + Default>(
        &self,
        user_id: i64,
        column: &str,
    ) -> DbResult<T> {
        let sql = format!("SELECT {column} FROM user_configs WHERE user_id = ?1");
        let json: Option<String> = self
            .conn
            .lock()
            .unwrap()
            .query_row(&sql, params![user_id], |r| r.get(0))
            .optional()
            .map_err(db_err)?;
        match json {
            Some(j) => serde_json::from_str(&j).map_err(db_err),
            None => Ok(T::default()),
        }
    }

    fn set_config_json<T: Serialize>(&self, user_id: i64, column: &str, cfg: &T) -> DbResult<()> {
        let json = serde_json::to_string(cfg).map_err(db_err)?;
        let sql = format!(
            "INSERT INTO user_configs (user_id, {column}) VALUES (?1, ?2)
             ON CONFLICT(user_id) DO UPDATE SET {column} = ?2"
        );
        self.conn
            .lock()
            .unwrap()
            .execute(&sql, params![user_id, json])
            .map(|_| ())
            .map_err(db_err)
    }

    // --- per-user matrix cache -------------------------------------------

    pub fn set_matrix(&self, user_id: i64, matrix: &LogMatrix, qso_count: usize) -> DbResult<()> {
        let json = serde_json::to_string(matrix).map_err(db_err)?;
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO matrices (user_id, matrix_json, qso_count, last_refresh_unix)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(user_id) DO UPDATE SET
                   matrix_json = ?2, qso_count = ?3, last_refresh_unix = ?4",
                params![user_id, json, qso_count as i64, now_unix()],
            )
            .map(|_| ())
            .map_err(db_err)
    }

    // --- meta: small server-wide bookkeeping ------------------------------
    // Refresh timestamps live here rather than on a file's mtime, because
    // `install -m 600` rewrites mtimes on every deploy and would silently
    // reset the LoTW clock each time.

    pub fn meta_get(&self, key: &str) -> DbResult<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
            r.get(0)
        })
        .optional()
        .map_err(db_err)
    }

    pub fn meta_set(&self, key: &str, value: &str) -> DbResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
               ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )
        .map(|_| ())
        .map_err(db_err)
    }

    /// A unix stamp in `meta`, or 0 when never recorded.
    pub fn meta_unix(&self, key: &str) -> i64 {
        self.meta_get(key)
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }

    pub fn meta_set_now(&self, key: &str) -> DbResult<()> {
        self.meta_set(key, &now_unix().to_string())
    }

    /// One user's log provenance: (qso_count, last_refresh_unix). The matrix
    /// itself is already in memory, so the station card only needs these two.
    pub fn matrix_meta(&self, user_id: i64) -> DbResult<Option<(i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT qso_count, last_refresh_unix FROM matrices WHERE user_id = ?1",
            params![user_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_err)
    }

    /// Every stored matrix: (user_id, matrix, qso_count, last_refresh).
    pub fn matrices(&self) -> DbResult<Vec<(i64, LogMatrix, i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT user_id, matrix_json, qso_count, last_refresh_unix FROM matrices")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get(2)?,
                    r.get(3)?,
                ))
            })
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, json, count, refresh) = row.map_err(db_err)?;
            let matrix = serde_json::from_str(&json).map_err(db_err)?;
            out.push((id, matrix, count, refresh));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_band_mode_lists_mean_all() {
        let n = NotifyUserConfig::default();
        assert!(n.passes_band_mode(Some("20M"), "CW"));
        assert!(n.passes_band_mode(Some("70CM"), "DATA"));
        // A spot whose frequency fell in no band still passes an unset filter
        // — silence there would be a filter nobody asked for.
        assert!(n.passes_band_mode(None, "PHONE"));
    }

    #[test]
    fn band_and_mode_narrowing_are_anded() {
        let n = NotifyUserConfig {
            notify_bands: vec!["20M".into(), "15M".into()],
            notify_modes: vec!["CW".into()],
            ..Default::default()
        };
        assert!(n.passes_band_mode(Some("20M"), "CW"));
        assert!(!n.passes_band_mode(Some("20M"), "DATA"), "mode must gate");
        assert!(!n.passes_band_mode(Some("40M"), "CW"), "band must gate");
        // Band narrowing is on, and this spot has no band at all → excluded.
        assert!(!n.passes_band_mode(None, "CW"));
    }

    #[test]
    fn wants_level_covers_all_eight_and_never_worked() {
        let all_on = NotifyUserConfig {
            notify_unconf_dxcc: true,
            notify_unconf_slot: true,
            notify_unconf_band: true,
            notify_unconf_mode: true,
            ..Default::default()
        };
        for level in AlertLevel::FLAGGABLE {
            assert!(all_on.wants_level(level), "{level:?} should be wanted");
        }
        // Worked / None are outcomes, not alerts — never notifiable.
        assert!(!all_on.wants_level(AlertLevel::Worked));
        assert!(!all_on.wants_level(AlertLevel::None));
        // Default keeps the ? half quiet.
        let d = NotifyUserConfig::default();
        assert!(d.wants_level(AlertLevel::NewDxcc));
        assert!(!d.wants_level(AlertLevel::UnconfDxcc));
    }

    fn temp_db() -> (Db, std::path::PathBuf) {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "dxca-test-{}-{}.db",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&path);
        (Db::open(&path).unwrap(), path)
    }

    #[test]
    fn users_sessions_configs_roundtrip() {
        let (db, path) = temp_db();
        assert_eq!(db.user_count().unwrap(), 0);
        let id = db.create_user("vu2cpl", "Manoj", "hash", "admin").unwrap();
        assert_eq!(db.user_count().unwrap(), 1);
        let (user, hash) = db.user_by_callsign("VU2CPL").unwrap().unwrap();
        assert_eq!(user.id, id);
        assert!(user.is_admin());
        assert_eq!(hash, "hash");
        // Duplicate callsign refused.
        assert!(db.create_user("VU2CPL", "", "h", "user").is_err());

        db.create_session("tok", id, 3600).unwrap();
        assert_eq!(db.session_user("tok").unwrap().unwrap().callsign, "VU2CPL");
        assert!(db.session_user("other").unwrap().is_none());
        db.delete_session("tok").unwrap();
        assert!(db.session_user("tok").unwrap().is_none());

        // Configs default when unset, round-trip when set.
        assert!(!db.notify_config(id).unwrap().telegram_enabled);
        let mut cl = db.clublog_config(id).unwrap();
        assert!(cl.alerts.alert_new_dxcc);
        cl.callsign = "VU2CPL".into();
        cl.api_key = "k".into();
        db.set_clublog_config(id, &cl).unwrap();
        assert_eq!(db.clublog_config(id).unwrap().api_key, "k");

        let mut m = LogMatrix::default();
        m.record(324, "20M", "DATA", "VU2AAA", true);
        db.set_matrix(id, &m, 1).unwrap();
        let all = db.matrices().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, id);
        assert!(all[0].1.status(324).is_some());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn expired_sessions_do_not_authenticate() {
        let (db, path) = temp_db();
        let id = db.create_user("K1ABC", "", "h", "user").unwrap();
        db.create_session("old", id, -10).unwrap();
        assert!(db.session_user("old").unwrap().is_none());
        let _ = std::fs::remove_file(path);
    }
}
