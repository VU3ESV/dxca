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
///
/// **No API key here.** It was only ever used to fetch cty.xml, which is one
/// shared file backing one shared resolver, so it moved to a server-wide
/// setting (`Db::clublog_api_key`). What remains is genuinely personal: the
/// credentials that download *this operator's* log. Stored rows may still
/// carry the old `api_key`; serde ignores it, and `adopt_legacy_api_key`
/// lifts it to the server setting once at startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClubLogUserConfig {
    pub callsign: String,
    pub email: String,
    pub app_password: String,
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

/// One MQTT destination: where to publish spots for a panadapter overlay.
///
/// Server-wide, admin-edited, and stored in the database rather than
/// `config/dxca.toml` — see `Db::mqtt_destinations` for why.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MqttDestination {
    pub name: String,
    pub host: String,
    pub port: u16,
    /// Empty = connect anonymously. The shack broker has required
    /// credentials since 2026-08-21.
    pub username: String,
    pub password: String,
    /// Base topic; `/json` and `/cluster` are appended by the publisher.
    pub topic: String,
    pub client_id: String,
    /// Source-name allowlist; empty = every source.
    pub sources: Vec<String>,
    /// Publish every spot, ignoring the dedupe verdict.
    pub unfiltered: bool,
    pub enabled: bool,
}

impl Default for MqttDestination {
    fn default() -> Self {
        MqttDestination {
            name: String::new(),
            host: String::new(),
            // The shack broker's plain port. TLS (8883) would need rumqttc's
            // rustls feature turning back on — see the workspace manifest.
            port: 1883,
            username: String::new(),
            password: String::new(),
            topic: "shack/dxca/spots".into(),
            client_id: "dxca".into(),
            sources: Vec::new(),
            unfiltered: false,
            enabled: true,
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
    /// Ping only for spots a **human** typed, never a skimmer's.
    ///
    /// Off by default, so an existing account keeps behaving exactly as it
    /// did. Stored in the notify JSON blob, so an old row without the key
    /// simply deserializes to `false` — no migration.
    ///
    /// This is the Telegram half of the Spots screen's "Manual only", and
    /// independent of it on purpose: the whole point of the split is to be
    /// able to watch everything on screen while only being pinged for the
    /// spots a person bothered to send.
    pub notify_manual_only: bool,
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
            notify_manual_only: false,
        }
    }
}

impl NotifyUserConfig {
    /// Does this spot's band/mode survive the Telegram narrowing? Empty list
    /// = no narrowing on that axis.
    /// Should a spot from a **skimmer** ping this account?
    ///
    /// Narrows like `passes_band_mode`: `false` only when the operator has
    /// asked for human spots and this one is a machine's.
    pub fn passes_skimmer(&self, is_skimmer: bool) -> bool {
        !(self.notify_manual_only && is_skimmer)
    }

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

/// How many sent alerts to keep per user. A shack roster alerts a few dozen
/// times a day, so this is weeks of history and still trivial to query.
const ALERT_HISTORY_MAX: i64 = 500;

/// One Telegram alert as it was sent — or as it failed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentAlert {
    pub time_unix: i64,
    pub callsign: String,
    pub frequency_hz: i64,
    pub mode: String,
    pub band: String,
    pub dxcc_name: String,
    /// The serialized `AlertLevel`, so the UI reuses the same label and
    /// colour table the spots feed uses.
    pub level: String,
    pub source: String,
    /// The station that spotted it, when a relaying node named one. Empty
    /// for locally decoded spots, where `source` already names the receiver.
    #[serde(default)]
    pub spotter: String,
    pub delivered: bool,
    /// Telegram's complaint when `delivered` is false; empty otherwise.
    pub error: String,
}

/// `meta` key holding the MQTT destination list as a JSON array.
const MQTT_DESTINATIONS: &str = "mqtt_destinations";

/// `meta` key holding the server-wide ClubLog API key (cty.xml downloads).
const CLUBLOG_API_KEY: &str = "clublog_api_key";
/// Marks the one-time lift of a pre-2.1 per-user key. Separate from the key
/// itself so that clearing the key is not mistaken for "never migrated".
const CLUBLOG_KEY_ADOPTED: &str = "clublog_api_key_adopted";

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
CREATE TABLE IF NOT EXISTS blacklist (
    callsign TEXT PRIMARY KEY,
    added_unix INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS alerts_sent (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    time_unix INTEGER NOT NULL,
    callsign TEXT NOT NULL,
    frequency_hz INTEGER NOT NULL,
    mode TEXT NOT NULL,
    band TEXT NOT NULL,
    dxcc_name TEXT NOT NULL,
    level TEXT NOT NULL,
    source TEXT NOT NULL,
    delivered INTEGER NOT NULL,
    error TEXT NOT NULL DEFAULT '',
    spotter TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS alerts_sent_user_time
    ON alerts_sent (user_id, time_unix DESC);
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

/// Columns added to tables that already exist in the field.
///
/// `CREATE TABLE IF NOT EXISTS` is a no-op on a database that already has
/// the table, so a new column in [`SCHEMA`] reaches fresh installs only —
/// every existing install silently keeps the old shape and then fails at
/// the first query naming the column. This closes that gap.
///
/// Additive only, and deliberately so: `ADD COLUMN` is the one schema change
/// SQLite performs without rewriting the table, and a column with a default
/// cannot invalidate a row that is already there. Anything that needs to
/// drop, rename or retype belongs in a real versioned migration, not here.
const ADDED_COLUMNS: &[(&str, &str, &str)] = &[
    // table, column, full DDL for ALTER TABLE ... ADD COLUMN
    (
        "alerts_sent",
        "spotter",
        "spotter TEXT NOT NULL DEFAULT ''",
    ),
];

/// Bring an existing database up to the current shape. Runs on every open;
/// each step is skipped when its column is already present, so it is cheap
/// and safe to repeat.
fn migrate(conn: &Connection) -> DbResult<()> {
    for (table, column, ddl) in ADDED_COLUMNS {
        let present = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(db_err)?
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(db_err)?
            .filter_map(Result::ok)
            .any(|name| name == *column);
        if !present {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {ddl};"))
                .map_err(db_err)?;
        }
    }
    Ok(())
}

impl Db {
    pub fn open(path: &Path) -> DbResult<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(db_err)?;
        }
        let conn = Connection::open(path).map_err(db_err)?;
        conn.execute_batch(SCHEMA).map_err(db_err)?;
        migrate(&conn)?;
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

    pub fn user_by_id(&self, id: i64) -> DbResult<Option<User>> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT id, callsign, display_name, role FROM users WHERE id = ?1",
                params![id],
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

    /// How many accounts hold the admin role. The API uses this to refuse
    /// the two edits that cannot be undone from the web UI: removing or
    /// demoting the last admin while other accounts still exist.
    pub fn admin_count(&self) -> DbResult<i64> {
        self.conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM users WHERE role = 'admin'", [], |r| {
                r.get(0)
            })
            .map_err(db_err)
    }

    /// Delete an account. Its sessions, per-user config and worked matrix go
    /// with it through `ON DELETE CASCADE` — which is live because `open`
    /// turns `PRAGMA foreign_keys` on. (Hand-editing the file with the
    /// sqlite3 CLI does NOT: that defaults to off and orphans the children.)
    /// Returns false when no such id.
    pub fn delete_user(&self, id: i64) -> DbResult<bool> {
        let n = self
            .conn
            .lock()
            .unwrap()
            .execute("DELETE FROM users WHERE id = ?1", params![id])
            .map_err(db_err)?;
        Ok(n > 0)
    }

    /// Patch the mutable identity fields; `None` leaves one alone. Callsign
    /// is uppercased like `create_user` does, so a rename cannot produce a
    /// row that `user_by_callsign` (which uppercases its argument) can never
    /// match — that would be an account nobody could log into.
    ///
    /// Renaming is safe for the rest of the schema: user_configs, matrices
    /// and sessions all key on user_id, and ClubLogUserConfig carries its
    /// own callsign for the ADIF download, independent of the login name.
    pub fn update_user(
        &self,
        id: i64,
        callsign: Option<&str>,
        display_name: Option<&str>,
        role: Option<&str>,
    ) -> DbResult<bool> {
        let conn = self.conn.lock().unwrap();
        let mut changed = false;
        if let Some(c) = callsign {
            conn.execute(
                "UPDATE users SET callsign = ?2 WHERE id = ?1",
                params![id, c.trim().to_uppercase()],
            )
            .map_err(|e| format!("rename user: {e}"))?;
            changed = true;
        }
        if let Some(d) = display_name {
            conn.execute(
                "UPDATE users SET display_name = ?2 WHERE id = ?1",
                params![id, d],
            )
            .map_err(db_err)?;
            changed = true;
        }
        if let Some(r) = role {
            conn.execute("UPDATE users SET role = ?2 WHERE id = ?1", params![id, r])
                .map_err(db_err)?;
            changed = true;
        }
        Ok(changed)
    }

    pub fn set_pass_hash(&self, id: i64, hash: &str) -> DbResult<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE users SET pass_hash = ?2 WHERE id = ?1",
                params![id, hash],
            )
            .map_err(db_err)?;
        Ok(())
    }

    // --- sent alerts ------------------------------------------------------
    //
    // A log of what actually went to Telegram, per user. Kept because
    // "did it alert me?" was otherwise unanswerable: the fan-out is
    // fire-and-forget on a background thread, so a spot that was flagged,
    // narrowed out, held by the cooldown or rejected by Telegram all looked
    // identical from the UI — silence.
    //
    // Failures are recorded too, with the error. A Telegram send that was
    // refused is the single most useful row on that screen and the one a
    // "sent" log that only stored successes would hide.

    /// Record one alert and prune the user's history to `ALERT_HISTORY_MAX`.
    pub fn record_sent_alert(&self, user_id: i64, a: &SentAlert) -> DbResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO alerts_sent
               (user_id, time_unix, callsign, frequency_hz, mode, band,
                dxcc_name, level, source, spotter, delivered, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                user_id,
                a.time_unix,
                a.callsign,
                a.frequency_hz,
                a.mode,
                a.band,
                a.dxcc_name,
                a.level,
                a.source,
                a.spotter,
                a.delivered as i64,
                a.error,
            ],
        )
        .map_err(|e| format!("record alert: {e}"))?;
        // Bounded per user, not globally: one busy operator must not evict
        // another's history.
        conn.execute(
            "DELETE FROM alerts_sent WHERE user_id = ?1 AND id NOT IN
               (SELECT id FROM alerts_sent WHERE user_id = ?1
                ORDER BY id DESC LIMIT ?2)",
            params![user_id, ALERT_HISTORY_MAX],
        )
        .map_err(db_err)?;
        Ok(())
    }

    pub fn sent_alerts(&self, user_id: i64, limit: usize) -> DbResult<Vec<SentAlert>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT time_unix, callsign, frequency_hz, mode, band,
                        dxcc_name, level, source, spotter, delivered, error
                 FROM alerts_sent WHERE user_id = ?1
                 ORDER BY time_unix DESC, id DESC LIMIT ?2",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![user_id, limit as i64], |r| {
                Ok(SentAlert {
                    time_unix: r.get(0)?,
                    callsign: r.get(1)?,
                    frequency_hz: r.get(2)?,
                    mode: r.get(3)?,
                    band: r.get(4)?,
                    dxcc_name: r.get(5)?,
                    level: r.get(6)?,
                    source: r.get(7)?,
                    spotter: r.get(8)?,
                    delivered: r.get::<_, i64>(9)? != 0,
                    error: r.get(10)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<_, _>>().map_err(db_err)
    }

    // --- MQTT destinations ------------------------------------------------
    //
    // Stored here, NOT in config/dxca.toml, because a broker password is a
    // secret and that file is installed 0644 while this database is 0600 —
    // exactly the reasoning that moved the ClubLog API key. Kept as one JSON
    // blob in `meta`: it is a short list edited as a whole, like the alert
    // configs, and a table would buy nothing.

    pub fn mqtt_destinations(&self) -> DbResult<Vec<MqttDestination>> {
        let raw = self.meta_get(MQTT_DESTINATIONS)?.unwrap_or_default();
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&raw).map_err(|e| format!("parse mqtt destinations: {e}"))
    }

    pub fn set_mqtt_destinations(&self, dests: &[MqttDestination]) -> DbResult<()> {
        let json = serde_json::to_string(dests)
            .map_err(|e| format!("serialize mqtt destinations: {e}"))?;
        self.meta_set(MQTT_DESTINATIONS, &json)
    }

    // --- blacklist --------------------------------------------------------
    //
    // Server-wide and admin-managed by design: a matching spot is dropped in
    // the pipeline, before the ring, so it is gone from the Spots table, the
    // telnet cluster server, the UDP fan-out and Telegram for every account
    // at once. That is only coherent as one shared list — a per-user drop
    // cannot exist, because the ring is shared.
    //
    // Callsigns are stored uppercase and matched exactly.

    pub fn blacklist(&self) -> DbResult<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT callsign FROM blacklist ORDER BY callsign")
            .map_err(db_err)?;
        let rows = stmt.query_map([], |r| r.get(0)).map_err(db_err)?;
        rows.collect::<Result<_, _>>().map_err(db_err)
    }

    /// Returns false when the call was already listed.
    pub fn blacklist_add(&self, callsign: &str) -> DbResult<bool> {
        let n = self
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT OR IGNORE INTO blacklist (callsign, added_unix) VALUES (?1, ?2)",
                params![callsign.trim().to_uppercase(), now_unix()],
            )
            .map_err(|e| format!("blacklist add: {e}"))?;
        Ok(n > 0)
    }

    /// Returns false when the call was not listed.
    pub fn blacklist_remove(&self, callsign: &str) -> DbResult<bool> {
        let n = self
            .conn
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM blacklist WHERE callsign = ?1",
                params![callsign.trim().to_uppercase()],
            )
            .map_err(db_err)?;
        Ok(n > 0)
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

    /// The server-wide ClubLog API key (for cty.xml). Stored in the database
    /// rather than `config/dxca.toml` because that file is installed 0644
    /// while the database is 0600 — a secret belongs with the other secrets.
    pub fn clublog_api_key(&self) -> String {
        self.meta_get(CLUBLOG_API_KEY)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    pub fn set_clublog_api_key(&self, key: &str) -> DbResult<()> {
        self.meta_set(CLUBLOG_API_KEY, key)
    }

    /// One-time adoption of a per-user key from before the setting moved, so
    /// an operator who had one in their ClubLog tab keeps working with no
    /// manual step. Returns the callsign it took the key from, for the log.
    ///
    /// Guarded by its own "already ran" flag rather than by "is the server
    /// key empty?". Those look equivalent and are not: an admin who
    /// deliberately CLEARS the key leaves it empty, and an emptiness check
    /// would re-adopt the stale key from the user row on the next restart —
    /// silently undoing them, forever. The flag is set even when no legacy
    /// key is found, so the scan happens exactly once per database.
    pub fn adopt_legacy_api_key(&self) -> DbResult<Option<String>> {
        if self.meta_get(CLUBLOG_KEY_ADOPTED)?.is_some() {
            return Ok(None);
        }
        self.meta_set(CLUBLOG_KEY_ADOPTED, "1")?;
        if !self.clublog_api_key().is_empty() {
            return Ok(None);
        }
        for user in self.users()? {
            // The field is gone from ClubLogUserConfig, so read the raw JSON.
            let raw: Option<String> = {
                let conn = self.conn.lock().unwrap();
                conn.query_row(
                    "SELECT clublog_json FROM user_configs WHERE user_id = ?1",
                    params![user.id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_err)?
            };
            let Some(raw) = raw else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            let key = v.get("api_key").and_then(|k| k.as_str()).unwrap_or("");
            if !key.is_empty() {
                self.set_clublog_api_key(key)?;
                return Ok(Some(user.callsign));
            }
        }
        Ok(None)
    }

    /// Write a raw clublog_json blob — tests only, to forge a row in the
    /// shape an older build would have stored.
    #[cfg(test)]
    pub fn set_clublog_json_raw(&self, user_id: i64, json: &str) -> DbResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO user_configs (user_id, clublog_json) VALUES (?1, ?2)
               ON CONFLICT(user_id) DO UPDATE SET clublog_json = ?2",
            params![user_id, json],
        )
        .map(|_| ())
        .map_err(db_err)
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
    /// An account stored before this key existed must deserialize to
    /// "off", or an upgrade would silently start suppressing alerts that
    /// used to arrive.
    #[test]
    fn manual_only_defaults_off_for_a_stored_row_that_predates_it() {
        let old_row = r#"{"telegram_enabled":true,"telegram_bot_token":"t",
            "telegram_chat_id":"c","cooldown_minutes":15,
            "notify_new_dxcc":true,"notify_bands":[],"notify_modes":[]}"#;
        let cfg: NotifyUserConfig = serde_json::from_str(old_row).expect("old row parses");
        assert!(!cfg.notify_manual_only, "must default off");
        assert!(cfg.telegram_enabled, "the rest of the row still loads");
        assert_eq!(cfg.cooldown_minutes, 15);
    }

    #[test]
    fn manual_only_narrows_skimmers_and_nothing_else() {
        let mut n = NotifyUserConfig::default();
        // Off by default: every spot passes, machine or not.
        assert!(n.passes_skimmer(true));
        assert!(n.passes_skimmer(false));

        n.notify_manual_only = true;
        assert!(!n.passes_skimmer(true), "a skimmer spot is held back");
        assert!(n.passes_skimmer(false), "a human's still pings");
    }

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

    #[test]
    /// The migration is the risky half of adding a column: production
    /// databases already exist, `CREATE TABLE IF NOT EXISTS` will not touch
    /// them, and the first query naming the new column would fail at
    /// runtime rather than at compile time. This builds a database with the
    /// OLD alerts_sent shape, opens it through `Db::open`, and checks the
    /// column arrives with existing rows intact.
    #[test]
    fn opening_an_old_database_adds_the_spotter_column_without_losing_rows() {
        // Unique per run: a previous failure leaves the directory behind
        // (the panic skips the cleanup at the end), and reusing the path
        // would then fail with "table users already exists" — masking the
        // real assertion with a setup error.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("dxca-migrate-{}-{nanos}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dxca.db");

        // A pre-migration database, written by hand: no `spotter` column.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE users (
                     id INTEGER PRIMARY KEY, callsign TEXT UNIQUE NOT NULL,
                     display_name TEXT NOT NULL DEFAULT '', pass_hash TEXT NOT NULL,
                     role TEXT NOT NULL DEFAULT 'user', created_unix INTEGER NOT NULL);
                 CREATE TABLE alerts_sent (
                     id INTEGER PRIMARY KEY,
                     user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                     time_unix INTEGER NOT NULL, callsign TEXT NOT NULL,
                     frequency_hz INTEGER NOT NULL, mode TEXT NOT NULL,
                     band TEXT NOT NULL, dxcc_name TEXT NOT NULL,
                     level TEXT NOT NULL, source TEXT NOT NULL,
                     delivered INTEGER NOT NULL, error TEXT NOT NULL DEFAULT '');
                 INSERT INTO users (id, callsign, pass_hash, created_unix)
                     VALUES (1, 'VU2CPL', 'h', 0);
                 INSERT INTO alerts_sent
                     (user_id, time_unix, callsign, frequency_hz, mode, band,
                      dxcc_name, level, source, delivered, error)
                     VALUES (1, 100, 'OLDCALL', 14074000, 'FT8', '20M',
                             'Bouvet', 'newDXCC', 'DB0SUE', 1, '');",
            )
            .unwrap();
        }

        // Opening it must migrate, not explode.
        let db = Db::open(&path).expect("an old database must still open");
        let rows = db.sent_alerts(1, 10).expect("the new column must be queryable");
        assert_eq!(rows.len(), 1, "the existing row survives");
        assert_eq!(rows[0].callsign, "OLDCALL");
        assert_eq!(rows[0].spotter, "", "back-filled with the default");

        // And a new row round-trips the spotter.
        db.record_sent_alert(
            1,
            &SentAlert {
                time_unix: 200,
                callsign: "3Y0J".into(),
                frequency_hz: 14_074_000,
                mode: "FT8".into(),
                band: "20M".into(),
                dxcc_name: "Bouvet".into(),
                level: "newDXCC".into(),
                source: "N2WQ-2".into(),
                spotter: "VU2XYZ".into(),
                delivered: true,
                error: String::new(),
            },
        )
        .unwrap();
        let rows = db.sent_alerts(1, 10).unwrap();
        assert_eq!(rows[0].spotter, "VU2XYZ", "newest first");

        // Idempotent: opening again must not try to add it twice.
        drop(db);
        let db = Db::open(&path).expect("re-open must be a no-op");
        assert_eq!(db.sent_alerts(1, 10).unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sent_alerts_keep_failures_and_stay_bounded_per_user() {
        let (db, _p) = temp_db();
        let a = db.create_user("VU2CPL", "h", "", "admin").unwrap();
        let b = db.create_user("K1ABC", "h", "", "user").unwrap();

        let alert = |call: &str, delivered: bool, error: &str| SentAlert {
            time_unix: 1_787_745_000,
            callsign: call.into(),
            frequency_hz: 14_074_000,
            mode: "FT8".into(),
            band: "20M".into(),
            dxcc_name: "INDIA".into(),
            level: "newDXCC".into(),
            source: "VU2OY".into(),
            spotter: String::new(),
            delivered,
            error: error.into(),
        };

        db.record_sent_alert(a, &alert("VU2ZZZ", true, "")).unwrap();
        // A refused send is the row most worth keeping — it is why the
        // history exists at all.
        db.record_sent_alert(a, &alert("P5DX", false, "chat not found"))
            .unwrap();
        db.record_sent_alert(b, &alert("W1AW", true, "")).unwrap();

        let rows = db.sent_alerts(a, 100).unwrap();
        assert_eq!(rows.len(), 2, "B's alert is not in A's history");
        let failed = rows.iter().find(|r| !r.delivered).unwrap();
        assert_eq!(failed.callsign, "P5DX");
        assert_eq!(failed.error, "chat not found", "the reason is kept");
        assert_eq!(db.sent_alerts(b, 100).unwrap().len(), 1);

        // The cap is per user, so a busy operator cannot evict another's
        // history. Push A past it and B must be untouched.
        for i in 0..(ALERT_HISTORY_MAX + 20) {
            db.record_sent_alert(a, &alert(&format!("T{i}"), true, ""))
                .unwrap();
        }
        assert_eq!(
            db.sent_alerts(a, 10_000).unwrap().len() as i64,
            ALERT_HISTORY_MAX,
            "A is pruned to the cap"
        );
        assert_eq!(
            db.sent_alerts(b, 100).unwrap().len(),
            1,
            "B's single alert survives A's flood"
        );
    }

    #[test]
    fn legacy_per_user_api_key_is_adopted_once() {
        let (db, _p) = temp_db();
        let id = db.create_user("VU2CPL", "hash", "Manoj", "admin").unwrap();

        // A row as written BEFORE the key moved: api_key is not a field of
        // ClubLogUserConfig any more, so write the raw JSON the old build
        // would have stored.
        db.set_clublog_json_raw(
            id,
            r#"{"callsign":"VU2CPL","email":"a@b.c","app_password":"p","api_key":"LEGACY123"}"#,
        )
        .unwrap();
        assert_eq!(db.clublog_api_key(), "", "server has none to begin with");

        assert_eq!(
            db.adopt_legacy_api_key().unwrap().as_deref(),
            Some("VU2CPL")
        );
        assert_eq!(db.clublog_api_key(), "LEGACY123");

        // Idempotent: a second run finds the server key set and does nothing.
        assert_eq!(db.adopt_legacy_api_key().unwrap(), None);

        // A deliberate clear must survive every later startup, even though
        // the legacy key is still sitting in the user's row. Guarding on
        // "is the server key empty?" instead of the ran-once flag would
        // silently re-adopt here and undo the admin, forever.
        db.set_clublog_api_key("").unwrap();
        assert_eq!(db.adopt_legacy_api_key().unwrap(), None);
        assert_eq!(db.clublog_api_key(), "", "the clear stands");
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
        cl.email = "op@example.com".into();
        db.set_clublog_config(id, &cl).unwrap();
        let back = db.clublog_config(id).unwrap();
        assert_eq!(back.callsign, "VU2CPL");
        assert_eq!(back.email, "op@example.com");
        assert_eq!(back.refresh_hours, 24, "default survives a round trip");

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
