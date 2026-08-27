//! Per-user state over the shared spot stream (plan §5): the global DXCC
//! resolver (one cty.xml for the server), per-user matrices in memory
//! (backed by SQLite), per-user classification, the ClubLog refresh flow,
//! and Telegram alert fan-out with per-user, per-callsign cooldown.

use crate::db::{Db, NotifyUserConfig};
use dxca_connect::clublog::{self, Endpoints};
use dxca_connect::lotw;
use dxca_connect::telegram::{Telegram, escape_html};
use dxca_core::classify::{AlertClassifier, AlertConfig, AlertLevel, Classification};
use dxca_core::dxcc::DxccResolver;
use dxca_core::matrix::LogMatrix;
use dxca_core::{Spot, cty};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

pub struct UserService {
    pub db: Arc<Db>,
    resolver: RwLock<Arc<DxccResolver>>,
    matrices: RwLock<HashMap<i64, Arc<LogMatrix>>>,
    /// (user_id, DX call) → last-notified unix.
    cooldowns: Mutex<HashMap<(i64, String), i64>>,
    telegram: Telegram,
    endpoints: Endpoints,
    cty_path: PathBuf,
    /// Known LoTW uploaders (global, M5 display marker).
    lotw_users: RwLock<Arc<HashSet<String>>>,
    lotw_path: PathBuf,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before 1970")
        .as_secs() as i64
}

impl UserService {
    /// Load the cached cty.xml (if present) and every stored matrix.
    pub fn new(
        db: Arc<Db>,
        data_dir: &str,
        telegram: Telegram,
        endpoints: Endpoints,
    ) -> UserService {
        let cty_path = PathBuf::from(data_dir).join("cty.xml");
        let mut resolver = DxccResolver::default();
        if let Ok(xml) = std::fs::read_to_string(&cty_path)
            && let Some(data) = cty::parse(&xml)
        {
            resolver.load(data.entities, &data.prefix_rules, now_unix());
        }
        let matrices = db
            .matrices()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, m, _, _)| (id, Arc::new(m)))
            .collect();
        let lotw_path = PathBuf::from(data_dir).join("lotw-users.txt");
        let lotw_users = std::fs::read_to_string(&lotw_path)
            .map(|text| lotw::parse_users(&text))
            .unwrap_or_default();
        UserService {
            db,
            resolver: RwLock::new(Arc::new(resolver)),
            matrices: RwLock::new(matrices),
            cooldowns: Mutex::new(HashMap::new()),
            telegram,
            endpoints,
            cty_path,
            lotw_users: RwLock::new(Arc::new(lotw_users)),
            lotw_path,
        }
    }

    pub fn is_lotw_user(&self, callsign: &str) -> bool {
        lotw::is_user(&self.lotw_users.read().unwrap(), callsign)
    }

    pub fn lotw_count(&self) -> usize {
        self.lotw_users.read().unwrap().len()
    }

    /// Download and reload the LoTW users list (blocking). Returns the
    /// user count.
    pub fn refresh_lotw(&self, url: &str) -> Result<usize, String> {
        let text = lotw::download(url)?;
        let users = lotw::parse_users(&text);
        if users.is_empty() {
            return Err("LoTW list parsed to zero users — not saving".into());
        }
        if let Some(dir) = self.lotw_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(&self.lotw_path, &text).map_err(|e| format!("save LoTW list: {e}"))?;
        let count = users.len();
        *self.lotw_users.write().unwrap() = Arc::new(users);
        // Stamped HERE, not in the scheduler, so a manual "Refresh LoTW
        // users list" resets the automatic clock too — otherwise pressing
        // the button would be followed by the scheduler downloading the same
        // 6 MB again on its next tick.
        let _ = self.db.meta_set_now(crate::refresh::LOTW_OK_KEY);
        Ok(count)
    }

    /// Send a test message through the user's configured Telegram
    /// (blocking) — the M5 "Test" button.
    pub fn telegram_test(&self, user_id: i64) -> Result<(), String> {
        let cfg = self.db.notify_config(user_id)?;
        self.telegram.send(
            &cfg.telegram_bot_token,
            &cfg.telegram_chat_id,
            "<b>DXCA test</b>\nTelegram alerts are wired up.",
        )
    }

    pub fn resolver_loaded(&self) -> bool {
        self.resolver.read().unwrap().is_loaded()
    }

    pub fn entity_count(&self) -> usize {
        self.resolver.read().unwrap().entity_count()
    }

    /// The 1.x refresh flow for one user: cty.xml (when an API key is set),
    /// then the ADIF log, then the matrix build. Blocking — run it on a
    /// blocking task. Returns (qso_count, dxcc_count).
    /// Download and reload **cty.xml** (blocking). Server-wide: one file, one
    /// resolver, every account classified against it — which is why the key
    /// is a server setting and this is admin-only, matching `refresh_lotw`.
    ///
    /// It used to ride along inside `refresh_user`, keyed off whichever
    /// account happened to have an `api_key`. That meant any non-admin could
    /// swap a server-wide resource, and with automatic refresh every keyed
    /// user re-downloaded the same ~10 MB daily.
    pub fn refresh_cty(&self, api_key: &str) -> Result<usize, String> {
        if api_key.is_empty() {
            return Err("no ClubLog API key configured (System → ClubLog API key)".into());
        }
        let xml = clublog::download_cty(&self.endpoints, api_key)?;
        let data = cty::parse(&xml).ok_or("cty.xml parse failed")?;
        let count = data.entities.len();
        if let Some(dir) = self.cty_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(&self.cty_path, &xml).map_err(|e| format!("save cty.xml: {e}"))?;
        let mut resolver = DxccResolver::default();
        resolver.load(data.entities, &data.prefix_rules, now_unix());
        *self.resolver.write().unwrap() = Arc::new(resolver);
        // Stamped here so the manual button and the scheduler share one
        // clock, exactly as refresh_lotw does.
        let _ = self.db.meta_set_now(crate::refresh::CTY_OK_KEY);
        Ok(count)
    }

    /// Download one user's ClubLog log and rebuild their matrix (blocking).
    /// Uses their email + app password only — the API key plays no part
    /// here, it was only ever for cty.xml.
    pub fn refresh_user(&self, user_id: i64) -> Result<(usize, usize), String> {
        let cfg = self.db.clublog_config(user_id)?;

        if cfg.callsign.is_empty() || cfg.email.is_empty() || cfg.app_password.is_empty() {
            return Err("need callsign, email and app password".into());
        }
        let adif = clublog::download_adif(
            &self.endpoints,
            &cfg.callsign,
            &cfg.email,
            &cfg.app_password,
        )?;
        let content = match String::from_utf8(adif) {
            Ok(s) => s,
            Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(), // Latin-1 fallback
        };

        let resolver = self.resolver.read().unwrap().clone();
        if !resolver.is_loaded() {
            // The key is an admin/server setting now, so a plain user cannot
            // fix this themselves — say who can.
            return Err(
                "no cty.xml loaded — an admin must set the ClubLog API key in System and refresh"
                    .into(),
            );
        }
        let (matrix, qso_count) = LogMatrix::build_from_adif(&content, &resolver);
        let dxcc_count = matrix.total_dxcc_count();
        self.db.set_matrix(user_id, &matrix, qso_count)?;
        self.matrices
            .write()
            .unwrap()
            .insert(user_id, Arc::new(matrix));
        Ok((qso_count, dxcc_count))
    }

    /// Award totals for one user's station card. `None` until they have a
    /// matrix — a user who has never refreshed ClubLog has nothing to count,
    /// which the card shows as "no log" rather than as four zeroes.
    pub fn stats(&self, user_id: i64) -> Option<dxca_core::matrix::MatrixStats> {
        Some(self.matrices.read().unwrap().get(&user_id)?.stats())
    }

    /// Per-band and per-mode entity counts for the My ClubLog statistics
    /// card. Same in-memory matrix as `stats`, just sliced.
    pub fn band_mode_stats(&self, user_id: i64) -> Option<dxca_core::matrix::BandModeStats> {
        Some(
            self.matrices
                .read()
                .unwrap()
                .get(&user_id)?
                .by_band_and_mode(),
        )
    }

    /// Classify one spot for one user (their matrix + alert toggles).
    /// None when the user has no matrix yet.
    pub fn classify(&self, user_id: i64, spot: &Spot) -> Option<Classification> {
        let matrix = self.matrices.read().unwrap().get(&user_id)?.clone();
        let resolver = self.resolver.read().unwrap().clone();
        let config: AlertConfig = (&self.db.clublog_config(user_id).ok()?.alerts).into();
        let call = spot.dx_callsign()?;
        Some(
            AlertClassifier {
                matrix: &matrix,
                resolver: &resolver,
                config: &config,
            }
            .classify(&call, spot.frequency_mhz(), &spot.mode),
        )
    }

    /// Alert fan-out for one spot: every user with a matrix classifies it;
    /// matching levels go to their Telegram with per-callsign cooldown
    /// (1.x `maybeNotify`, minus macOS notifications and display filters).
    pub fn fan_out(self: &Arc<Self>, spot: &Spot) {
        let user_ids: Vec<i64> = self.matrices.read().unwrap().keys().copied().collect();
        for user_id in user_ids {
            let Ok(notify) = self.db.notify_config(user_id) else {
                continue;
            };
            if !notify.telegram_enabled {
                continue;
            }
            let Some(c) = self.classify(user_id, spot) else {
                continue;
            };
            if !notify.wants_level(c.level) {
                continue;
            }
            // Band / mode-class narrowing is Telegram's alone — the Spots
            // screen keeps its own, so the operator can watch everything and
            // still be pinged for only CW on 20M.
            if !notify.passes_band_mode(c.band, dxca_core::modes::canonical(&spot.mode)) {
                continue;
            }
            let Some(call) = spot.dx_callsign() else {
                continue;
            };
            if !self.cooldown_ok(user_id, &call, &notify) {
                continue;
            }
            let text = alert_html(&c, &call, spot);
            let telegram = self.telegram.clone();
            let (token, chat) = (notify.telegram_bot_token, notify.telegram_chat_id);
            // Recorded for the My Alerts history — including failures, which
            // are the rows worth having. Built here where the classification
            // is still to hand; written after the send, with its verdict.
            let mut record = crate::db::SentAlert {
                time_unix: spot.time_unix,
                callsign: call.clone(),
                frequency_hz: spot.frequency_hz() as i64,
                mode: spot.mode.clone(),
                band: c.band.unwrap_or_default().to_string(),
                dxcc_name: c.dxcc_name.clone().unwrap_or_default(),
                level: serde_json::to_value(c.level)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default(),
                source: spot.source_name.clone(),
                delivered: true,
                error: String::new(),
            };
            let this = self.clone();
            // Fire-and-forget off the pipeline: a slow Telegram round trip
            // must never stall spot processing.
            tokio::task::spawn_blocking(move || {
                if let Err(e) = telegram.send(&token, &chat, &text) {
                    eprintln!("dxca: telegram: {e}");
                    record.delivered = false;
                    record.error = e;
                }
                if let Err(e) = this.db.record_sent_alert(user_id, &record) {
                    eprintln!("dxca: alert history: {e}");
                }
            });
        }
    }

    /// 1.x cooldown: per callsign, clamped 5–60 minutes, with the same
    /// opportunistic 2000-entry sweep.
    fn cooldown_ok(&self, user_id: i64, call: &str, cfg: &NotifyUserConfig) -> bool {
        let key = (user_id, call.to_uppercase());
        let now = now_unix();
        let cooldown_secs = cfg.cooldown_minutes.clamp(5, 60) * 60;
        let mut map = self.cooldowns.lock().unwrap();
        if let Some(&last) = map.get(&key)
            && now - last < cooldown_secs
        {
            return false;
        }
        if map.len() > 2000 {
            map.retain(|_, t| now - *t < 3600);
        }
        map.insert(key, now);
        true
    }
}

/// The 1.x Telegram message: emoji level label, HTML-escaped, source line.
fn alert_html(c: &Classification, call: &str, spot: &Spot) -> String {
    // The `?` half reuses its New counterpart's hue as a hollow circle: same
    // axis (DXCC/band/mode/slot), lesser catch — worked already, still not
    // confirmed. Colour says WHICH axis, filled-vs-hollow says how badly you
    // need it.
    let label = match c.level {
        AlertLevel::NewDxcc => "🔴 NEW DXCC",
        AlertLevel::NewSlot => "🟠 New Slot",
        AlertLevel::NewBand => "🔵 New Band",
        AlertLevel::NewMode => "🟡 New Mode",
        AlertLevel::UnconfDxcc => "⭕ ? DXCC (unconfirmed)",
        AlertLevel::UnconfSlot => "🟧 ? Slot (unconfirmed)",
        AlertLevel::UnconfBand => "🔷 ? Band (unconfirmed)",
        AlertLevel::UnconfMode => "🟨 ? Mode (unconfirmed)",
        _ => "Alert",
    };
    let dxcc = c.dxcc_name.clone().unwrap_or_default();
    let freq = format!("{:.3} MHz", spot.frequency_mhz());
    let band = c.band.unwrap_or("");
    let title = format!("{label}: {call}");
    let body = format!(
        "{}{freq}  {band}  {}  {} dB",
        if dxcc.is_empty() {
            String::new()
        } else {
            format!("{dxcc}  ")
        },
        spot.mode,
        spot.snr_db
    );
    format!(
        "<b>{}</b>\n{}\nSource: {}",
        escape_html(&title),
        escape_html(&body),
        escape_html(&spot.source_name)
    )
}
