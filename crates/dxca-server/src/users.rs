//! Per-user state over the shared spot stream (plan §5): the global DXCC
//! resolver (one cty.xml for the server), per-user matrices in memory
//! (backed by SQLite), per-user classification, the ClubLog refresh flow,
//! and Telegram alert fan-out with per-user, per-callsign cooldown.

use crate::db::{Db, NotifyUserConfig};
use dxca_connect::clublog::{self, Endpoints};
use dxca_connect::flex;
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
    /// Live SmartSDR sessions, keyed by address so several accounts aimed at
    /// one radio share a connection. Made on demand and kept for the life of
    /// the process — a radio that goes away is handled inside the client by
    /// reconnecting, not by tearing this down.
    flex: Mutex<HashMap<(String, u16), Arc<flex::FlexClient>>>,
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
            resolver.load(data, now_unix());
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
            flex: Mutex::new(HashMap::new()),
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

    /// The shared Telegram sender, for the operational alerts in
    /// [`crate::health`]. Those are not spot alerts and do not belong in the
    /// fan-out, but they go to the same per-account chat.
    pub fn telegram(&self) -> Telegram {
        self.telegram.clone()
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
        resolver.load(data, now_unix());
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
        let (matrix, qso_count, uncredited) =
            LogMatrix::build_from_adif_reporting(&content, &resolver);
        log_uncredited(user_id, &uncredited);
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

    /// The same totals counting **current entities only** — the ARRL
    /// deleted list left out, so they line up with the published standings.
    ///
    /// `None` also when no cty.xml is loaded: without the resolver there is
    /// no way to know which entities are deleted, and quietly returning the
    /// unfiltered totals under a "current only" label would be a lie.
    pub fn stats_current(&self, user_id: i64) -> Option<dxca_core::matrix::MatrixStats> {
        let resolver = self.resolver.read().unwrap().clone();
        if !resolver.is_loaded() {
            return None;
        }
        Some(
            self.matrices
                .read()
                .unwrap()
                .get(&user_id)?
                .stats_excluding(&resolver.deleted_adifs()),
        )
    }

    /// Per-band / per-mode counts, current entities only. `None` on the same
    /// terms as [`stats_current`](Self::stats_current).
    pub fn band_mode_stats_current(
        &self,
        user_id: i64,
    ) -> Option<dxca_core::matrix::BandModeStats> {
        let resolver = self.resolver.read().unwrap().clone();
        if !resolver.is_loaded() {
            return None;
        }
        Some(
            self.matrices
                .read()
                .unwrap()
                .get(&user_id)?
                .by_band_and_mode_excluding(&resolver.deleted_adifs()),
        )
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

    /// The sun's elevation at this user's QTH, right now.
    ///
    /// `None` when they have set no locator, or one that will not parse —
    /// which is what keeps the phase-rotation mask opt-in. Computed **once
    /// per request** and handed to `annotate_spot`, never per spot: the sun
    /// does not move across a spot list, and a database read per row would
    /// be absurd.
    pub fn sun_phase(&self, user_id: i64) -> Option<dxca_core::solar::SunPhase> {
        let cfg = self.db.station_config(user_id).ok()?;
        let pos = dxca_core::grid::parse(&cfg.locator)?;
        Some(dxca_core::solar::phase(
            pos,
            now_unix(),
            cfg.greyline_window_min,
        ))
    }

    /// The phase plus the sunrise/sunset it was derived from, for the
    /// screen. The UI shows the two times beside the phase badge so the
    /// operator can see what the mask is reasoning from rather than having
    /// to trust it — the same disclosure the `N dimmed` count provides.
    pub fn sun_state(&self, user_id: i64) -> Option<serde_json::Value> {
        let cfg = self.db.station_config(user_id).ok()?;
        let pos = dxca_core::grid::parse(&cfg.locator)?;
        let now = now_unix();
        let t = dxca_core::solar::sun_times(pos, now);
        Some(serde_json::json!({
            "phase": dxca_core::solar::phase(pos, now, cfg.greyline_window_min).key(),
            "sunrise_unix": t.sunrise_unix,
            "sunset_unix": t.sunset_unix,
            "greyline_window_min": cfg.greyline_window_min,
            "locator": cfg.locator,
        }))
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
            // Telegram and the panadapter are two sinks for the same
            // alerts, and either alone is a reasonable way to run — so the
            // gate asks whether ANY of them wants this account's alerts,
            // not whether Telegram does.
            let wants_telegram = notify.telegram_enabled;
            let wants_flex = notify.flex_enabled && !notify.flex_host.is_empty();
            if !wants_telegram && !wants_flex {
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
            // Machines spot relentlessly — on this station roughly three
            // quarters of the feed — so an operator who only wants to be
            // interrupted by a spot a person bothered to send can say so.
            // Independent of the Spots screen's own Manual-only, like the
            // band/mode narrowing above it.
            if !notify.passes_spotter(spot.is_skimmer) {
                continue;
            }
            // The band mask, if this account asked for it on Telegram
            // (milestone 4). Computed per SPOT here rather than per request
            // as the API does, because the fan-out runs continuously and a
            // session can outlive a sunset — a phase cached at startup would
            // narrow the wrong bands for the rest of the evening.
            //
            // New DXCC is exempt, exactly as it is on screen — and the
            // reason is stronger here. A dimmed row is still on the page and
            // one hover from being read; a held Telegram is a spot the
            // operator never learns about at all. If the model is ever
            // wrong, being wrong about the rarest catch of the year is the
            // one failure that would end this feature's welcome.
            if notify.notify_respect_band_mask && c.level != AlertLevel::NewDxcc {
                let open = self
                    .sun_phase(user_id)
                    .zip(c.band)
                    .map(|(p, b)| dxca_core::bands::plausible_in(b, p));
                if !notify.passes_band_mask(open) {
                    continue;
                }
            }
            let Some(call) = spot.dx_callsign() else {
                continue;
            };
            if !self.cooldown_ok(user_id, &call, &notify) {
                continue;
            }
            // The panadapter first: it is a queue push, never a network
            // round trip, so it costs nothing to do inline and lands while
            // the Telegram is still in flight.
            if wants_flex {
                self.push_flex(&notify, &c, &call, spot);
            }
            if !wants_telegram {
                continue;
            }
            let text = alert_html(&c, &call, spot, self.is_lotw_user(&call));
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
                spotter: spot.spotter.clone().unwrap_or_default(),
                snr_db: Some(spot.snr_db as i64),
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

    /// Panadapter colour for each alert level, taken from the **dashboard's
    /// own dark palette** so a red dot on the radio means what a red row
    /// means on screen. `0xAARRGGBB`, opaque.
    ///
    /// The four `?` levels are the same hues at 58% mixed toward the muted
    /// grey — the `color-mix` the stylesheet performs, precomputed here
    /// because the radio wants a literal.
    fn flex_color(level: AlertLevel) -> &'static str {
        match level {
            AlertLevel::NewDxcc => "0xFFF5636B",    // --err
            AlertLevel::NewBand => "0xFF2F81F7",    // --accent
            AlertLevel::NewMode => "0xFFFAB219",    // --warn
            AlertLevel::NewSlot => "0xFFF0883E",    // --alert-slot
            AlertLevel::UnconfDxcc => "0xFFC97479", // the four above, dimmed
            AlertLevel::UnconfBand => "0xFF5686CA",
            AlertLevel::UnconfMode => "0xFFCCA249",
            AlertLevel::UnconfSlot => "0xFFC68A5F",
            _ => "0xFF8C8C8C",
        }
    }

    /// How long each level stays on the panadapter.
    ///
    /// The ladder is the whole point. A **New DXCC** is worth leaving up for
    /// an hour — you may be mid-QSO when it appears and still want to find
    /// it afterwards. A **New Band or Mode** is worth a quarter hour, about
    /// as long as you would stay on a band looking for it. Everything
    /// below — New Slot and the four worked-but-unconfirmed levels — is
    /// worth knowing about only while the station is still calling, so it
    /// gets a minute.
    ///
    /// That floor is what keeps the display usable. Those lower levels are
    /// most of the alert traffic, and at nine nodes a twenty-minute life
    /// would paint the whole band inside an hour, burying the one red mark
    /// this feature exists to show.
    /// Adjustable per account; 0 on any field means the default beside it.
    fn flex_lifetime_secs(cfg: &NotifyUserConfig, level: AlertLevel) -> u64 {
        let or = |set: u64, default: u64| if set == 0 { default } else { set };
        let minutes = match level {
            AlertLevel::NewDxcc => or(cfg.flex_life_dxcc_minutes, 60),
            AlertLevel::NewBand | AlertLevel::NewMode => or(cfg.flex_life_band_mode_minutes, 15),
            _ => or(cfg.flex_life_other_minutes, 1),
        };
        minutes.saturating_mul(60)
    }

    /// Queue one alert onto the operator's panadapter.
    ///
    /// Never blocks: [`FlexClient::spot`] is a channel push, and the TCP
    /// session lives on its own thread. Clients are made on demand and kept,
    /// keyed by address, so several accounts pointing at one radio share a
    /// single connection rather than opening one each.
    fn push_flex(&self, notify: &NotifyUserConfig, c: &Classification, call: &str, spot: &Spot) {
        let port = if notify.flex_port == 0 {
            4992
        } else {
            notify.flex_port
        };
        let key = (notify.flex_host.clone(), port);
        let client = {
            let mut map = self.flex.lock().unwrap();
            map.entry(key)
                .or_insert_with(|| Arc::new(flex::FlexClient::connect(&notify.flex_host, port)))
                .clone()
        };
        // Level plus entity when they fit in the radio's 20 characters, the
        // entity alone when they do not — the colour already says which
        // level it is, so the entity is the half worth keeping.
        let comment = flex::comment_for(c.level.label(), c.dxcc_name.as_deref());
        client.spot(&flex::FlexSpot {
            callsign: call.to_string(),
            freq_mhz: spot.frequency_mhz(),
            mode: spot.mode.clone(),
            comment,
            // The station that heard it, falling back to the feed that
            // carried it — the panadapter has one field for this and an
            // empty one reads as a defect.
            spotter: match &spot.spotter {
                Some(s) if !s.is_empty() => s.clone(),
                _ => spot.source_name.clone(),
            },
            timestamp_unix: spot.time_unix,
            color: Self::flex_color(c.level).to_string(),
            lifetime_secs: Self::flex_lifetime_secs(notify, c.level),
        });
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

/// Most logs have none of these; a big one might have a handful. A cap keeps
/// a pathological log from filling the journal, and the summary line says
/// what was held back so the cap can never read as "that was all of them".
const UNCREDITED_LOG_CAP: usize = 50;

/// Print the contacts ClubLog gives no credit for, after a refresh.
///
/// These are otherwise invisible: the QSO is simply absent from the totals,
/// and the only symptom is a number that disagrees with ClubLog's by one.
/// Tracing VU24DX's 314-against-313 back to a single `ZL8AC` QSO in 65,908
/// records took a whole session — this turns that into one line at refresh
/// time, carrying the date needed to find the QSO in the log and delete it.
fn log_uncredited(user_id: i64, items: &[dxca_core::matrix::UncreditedContact]) {
    if items.is_empty() {
        return;
    }
    println!(
        "dxca: user {user_id}: {} contact(s) in this log earn no DXCC credit:",
        items.len()
    );
    for c in items.iter().take(UNCREDITED_LOG_CAP) {
        println!("dxca: user {user_id}:   {c}");
    }
    if let Some(held) = items
        .len()
        .checked_sub(UNCREDITED_LOG_CAP)
        .filter(|n| *n > 0)
    {
        println!("dxca: user {user_id}:   ... and {held} more not listed");
    }
}

/// The LoTW marker in a Telegram alert: the station uploads to Logbook of
/// the World, so a QSO with it is likely to confirm without a card chase.
///
/// The Spots table marks this with a green `●`, and matching that colour is
/// the constraint. Telegram's HTML supports `<b>`, `<i>` and `<a>` but **no
/// colour attribute**, so a plain `*` or `●` arrives in the body text colour
/// whatever we do. The only green Telegram will render is an emoji that is
/// green in the font itself — and of those, `❇️` is the one shaped like an
/// asterisk rather than a dot, a tick or a heart.
///
/// It is therefore emoji-sized rather than punctuation-sized. That is the
/// trade for the colour; there is no third option.
const LOTW_MARK: &str = "❇️";

/// The 1.x Telegram message: emoji level label, HTML-escaped, source line.
///
/// `is_lotw` appends [`LOTW_MARK`] to the callsign.
fn alert_html(c: &Classification, call: &str, spot: &Spot, is_lotw: bool) -> String {
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
    // The mark rides on the callsign, not the label, so it stays put whatever
    // the alert level is — and it goes through `escape_html` with the call
    // rather than being concatenated onto escaped output.
    let title = format!("{label}: {call}{}", if is_lotw { LOTW_MARK } else { "" });
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
    // Who actually heard it, and which of our feeds carried it. Labelled
    // rather than joined with "via": on a phone, `Spotter:` and `Node:` are
    // scannable, and a relay chain written as prose is not.
    //
    // A node that spots under its own callsign shows both lines the same,
    // which is the honest answer — it means the node made the spot itself.
    let origin = match &spot.spotter {
        Some(sp) if !sp.is_empty() => {
            format!("Spotter: {sp}   Node: {}", spot.source_name)
        }
        // Decoded here: there is no spotting station to name.
        _ => format!("Node: {}", spot.source_name),
    };
    format!(
        "<b>{}</b>\n{}\n{}\n{}Z",
        escape_html(&title),
        escape_html(&body),
        escape_html(&origin),
        escape_html(&spot.hhmm()),
    )
}

#[cfg(test)]
mod alert_message_tests {
    use super::*;
    use dxca_core::Spot;

    fn spot(source: &str, spotter: Option<&str>) -> Spot {
        Spot {
            // 14:28 UTC on some day — hhmm() is derived from this.
            time_unix: 14 * 3600 + 28 * 60,
            snr_db: -10,
            delta_time_s: 0.0,
            delta_frequency_hz: 0,
            mode: "FT8".into(),
            mode_inferred: false,
            message: "CQ K1JT".into(),
            is_cq: true,
            comment: String::new(),
            low_confidence: false,
            off_air: false,
            dial_frequency_hz: 14_074_000,
            source_name: source.into(),
            spotter: spotter.map(str::to_string),
            is_skimmer: false,
        }
    }

    fn classification() -> Classification {
        Classification {
            level: AlertLevel::NewDxcc,
            dxcc_id: Some(24),
            dxcc_name: Some("Bouvet".into()),
            band: Some("20M"),
            is_beacon: false,
        }
    }

    /// A relaying node is not the station that heard the DX. An alert that
    /// may send the operator to the radio should say which is which.
    #[test]
    fn a_relayed_alert_labels_the_spotter_and_the_node() {
        let html = alert_html(
            &classification(),
            "3Y0J",
            &spot("N2WQ-2", Some("VU2XYZ")),
            false,
        );
        assert!(html.contains("Spotter: VU2XYZ"), "got {html}");
        assert!(html.contains("Node: N2WQ-2"), "got {html}");
        assert!(!html.contains(" via "), "labelled, not prose: {html}");
    }

    /// Locally decoded: the source already names the receiver, so "via"
    /// would just repeat it.
    /// Decoded here: there is no spotting station, so no Spotter line —
    /// an empty label would read as missing data rather than as "us".
    #[test]
    fn a_local_alert_names_only_the_node() {
        let html = alert_html(&classification(), "3Y0J", &spot("MSHV", None), false);
        assert!(html.contains("Node: MSHV"), "got {html}");
        assert!(!html.contains("Spotter:"), "no empty label: {html}");
    }

    /// The spot's own time, in UTC, not the delivery time — a queued or
    /// retried alert must still say when the station was heard.
    #[test]
    fn the_alert_carries_the_spot_time_in_utc() {
        let html = alert_html(
            &classification(),
            "3Y0J",
            &spot("N2WQ-2", Some("VU2XYZ")),
            false,
        );
        assert!(html.contains("1428Z"), "got {html}");
    }

    /// A LoTW station is marked right after its callsign — the same fact the
    /// Spots table shows as a green dot, in the one form that survives
    /// Telegram's colourless HTML.
    #[test]
    fn a_lotw_station_is_marked_after_the_callsign() {
        let s = spot("N2WQ-2", Some("VU2XYZ"));
        let plain = alert_html(&classification(), "3Y0J", &s, false);
        let lotw = alert_html(&classification(), "3Y0J", &s, true);

        assert!(lotw.contains("3Y0J❇️"), "marked after the call: {lotw}");
        assert!(!plain.contains("3Y0J❇️"), "unmarked otherwise: {plain}");
        // The mark is the ONLY difference — it must not disturb the level
        // label, the body line, the origin lines or the time.
        assert_eq!(lotw.replace("3Y0J❇️", "3Y0J"), plain);
    }

    /// The mark belongs to the callsign, not to the alert level, so it is
    /// there on every level rather than only on the loudest one.
    #[test]
    fn the_lotw_mark_is_independent_of_the_alert_level() {
        for level in [
            AlertLevel::NewDxcc,
            AlertLevel::NewSlot,
            AlertLevel::UnconfBand,
            AlertLevel::UnconfMode,
        ] {
            let c = Classification {
                level,
                ..classification()
            };
            let html = alert_html(&c, "3Y0J", &spot("MSHV", None), true);
            assert!(html.contains("3Y0J❇️"), "{level:?}: {html}");
        }
    }

    /// A node that spots under its own callsign shows both labels reading
    /// the same. That is the honest answer — it means the node made the
    /// spot itself, rather than relaying somebody else's.
    #[test]
    fn a_node_spotting_under_its_own_name_shows_both_labels() {
        let html = alert_html(
            &classification(),
            "3Y0J",
            &spot("W3LPL", Some("W3LPL")),
            false,
        );
        assert!(html.contains("Spotter: W3LPL"), "got {html}");
        assert!(html.contains("Node: W3LPL"), "got {html}");
    }
}
