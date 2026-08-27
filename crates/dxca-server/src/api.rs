//! The HTTP API (plan §7). Session-cookie auth; admin role gates user
//! management; per-user config and classification for the authenticated
//! account. The embedded web UI is served by the fallback.

use crate::auth;
use crate::config::{BroadcastDestination, ClusterNode, Config, UdpSource};
use crate::db::{ClubLogUserConfig, NotifyUserConfig, User};
use crate::nodes::NodeManager;
use crate::pipeline::{PipelineInput, PipelineState};
use crate::users::UserService;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct AppState {
    pub pipeline: Arc<PipelineState>,
    pub nodes: Arc<NodeManager>,
    pub users: Arc<UserService>,
    /// The live global config (M5 web editing) + where it persists.
    pub config: Arc<Mutex<Config>>,
    pub config_path: PathBuf,
    /// Pipeline input — hot-applied sources/nodes feed into it.
    pub input_tx: mpsc::Sender<PipelineInput>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/spots", get(spots))
        .route("/api/stream", get(stream))
        .route("/api/setup", post(setup))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/me/station", get(station))
        .route("/api/reference", get(reference))
        .route("/api/config/me/clublog", get(get_clublog).put(put_clublog))
        .route(
            "/api/config/me/notifications",
            get(get_notify).put(put_notify),
        )
        .route("/api/clublog/refresh", post(refresh))
        .route("/api/config/global", get(get_global).put(put_global))
        .route("/api/telegram/test", post(telegram_test))
        .route("/api/lotw/refresh", post(lotw_refresh))
        .route("/api/users", get(list_users).post(create_user))
        .with_state(state)
        .fallback(crate::assets::serve)
}

fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(serde_json::json!({ "error": msg.into() }))).into_response()
}

fn unauthorized() -> Response {
    err(StatusCode::UNAUTHORIZED, "not logged in")
}

// A Response-typed Err is the ergonomic axum idiom; the size is irrelevant
// on these cold auth-failure paths.
#[allow(clippy::result_large_err)]
fn require_user(app: &AppState, headers: &HeaderMap) -> Result<User, Response> {
    auth::user_from_headers(&app.users.db, headers).ok_or_else(unauthorized)
}

#[allow(clippy::result_large_err)]
fn require_admin(app: &AppState, headers: &HeaderMap) -> Result<User, Response> {
    let user = require_user(app, headers)?;
    if !user.is_admin() {
        return Err(err(StatusCode::FORBIDDEN, "admin only"));
    }
    Ok(user)
}

// --- status + spots ------------------------------------------------------

fn status_json(app: &AppState) -> serde_json::Value {
    let counters = app.pipeline.broadcaster().counters();
    let user_count = app.users.db.user_count().unwrap_or(0);
    serde_json::json!({
        "name": "dxca",
        "version": env!("CARGO_PKG_VERSION"),
        "milestone": "M5 web parity",
        "setup_required": user_count == 0,
        "users": user_count,
        "cty_loaded": app.users.resolver_loaded(),
        "cty_entities": app.users.entity_count(),
        "lotw_users": app.users.lotw_count(),
        "telnet_clients": app.pipeline.telnet.client_count(),
        "spots_per_source": *app.pipeline.source_counts.lock().unwrap(),
        "cluster_nodes": app.nodes.statuses(),
        "udp_sent": counters.total_sent(),
        "udp_failed": counters.total_failed(),
    })
}

async fn status(State(app): State<AppState>) -> Json<serde_json::Value> {
    Json(status_json(&app))
}

#[derive(Deserialize)]
struct SpotsQuery {
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    200
}

/// One spot as the UI sees it: the raw fields plus the extracted DX call,
/// the LoTW marker, and — when a session is present — that user's
/// classification (alert level, DXCC name, band): plan §5's per-user
/// highlighting.
fn annotate_spot(app: &AppState, user: Option<&User>, s: &dxca_core::Spot) -> serde_json::Value {
    let mut v = serde_json::to_value(s).expect("spot serializes");
    let dx_call = s.dx_callsign();
    v["dx_call"] = serde_json::to_value(&dx_call).unwrap();
    v["is_lotw"] = serde_json::Value::Bool(dx_call.is_some_and(|c| app.users.is_lotw_user(&c)));
    if let Some(u) = user
        && let Some(c) = app.users.classify(u.id, s)
    {
        v["alert"] = serde_json::to_value(c.level).unwrap();
        v["dxcc_name"] = serde_json::to_value(&c.dxcc_name).unwrap();
        v["band"] = serde_json::to_value(c.band).unwrap();
        v["is_beacon"] = serde_json::Value::Bool(c.is_beacon);
    }
    v
}

async fn spots(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SpotsQuery>,
) -> Json<serde_json::Value> {
    let user = auth::user_from_headers(&app.users.db, &headers);
    let spots = app.pipeline.recent_spots(q.limit.min(2000));
    let annotated: Vec<serde_json::Value> = spots
        .iter()
        .map(|s| annotate_spot(&app, user.as_ref(), s))
        .collect();
    Json(serde_json::json!({ "spots": annotated }))
}

// --- live stream ---------------------------------------------------------

/// WebSocket: every processed spot as an annotated frame for THIS session's
/// user, plus a status frame every 5 s.
async fn stream(
    State(app): State<AppState>,
    headers: HeaderMap,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> Response {
    let user = auth::user_from_headers(&app.users.db, &headers);
    ws.on_upgrade(move |socket| stream_socket(socket, app, user))
}

async fn stream_socket(
    mut socket: axum::extract::ws::WebSocket,
    app: AppState,
    user: Option<User>,
) {
    use axum::extract::ws::Message as WsMessage;
    let mut spot_rx = app.pipeline.spot_events.subscribe();
    let mut status_tick = tokio::time::interval(std::time::Duration::from_secs(5));
    loop {
        tokio::select! {
            spot = spot_rx.recv() => match spot {
                Ok(spot) => {
                    let frame = serde_json::json!({
                        "type": "spot",
                        "spot": annotate_spot(&app, user.as_ref(), &spot),
                    });
                    if socket.send(WsMessage::Text(frame.to_string().into())).await.is_err() {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            },
            _ = status_tick.tick() => {
                let frame = serde_json::json!({ "type": "status", "status": status_json(&app) });
                if socket.send(WsMessage::Text(frame.to_string().into())).await.is_err() {
                    return;
                }
            }
            msg = socket.recv() => match msg {
                None | Some(Err(_)) => return,
                Some(Ok(WsMessage::Close(_))) => return,
                Some(Ok(_)) => continue, // client input ignored
            },
        }
    }
}

// --- auth ----------------------------------------------------------------

#[derive(Deserialize)]
struct Credentials {
    callsign: String,
    password: String,
    #[serde(default)]
    display_name: String,
}

/// First-run bootstrap: creates the admin account. Refused once any user
/// exists (no default credentials, ever).
async fn setup(State(app): State<AppState>, Json(req): Json<Credentials>) -> Response {
    match app.users.db.user_count() {
        Ok(0) => {}
        Ok(_) => return err(StatusCode::FORBIDDEN, "setup already done"),
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
    create_account(&app, &req, "admin", true).await
}

/// Admin creates further accounts (role "user" unless "admin" requested).
async fn create_user(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateUserReq>,
) -> Response {
    if let Err(resp) = require_admin(&app, &headers) {
        return resp;
    }
    let role = if req.role == "admin" { "admin" } else { "user" };
    // No session cookie here — creating an account for someone else must
    // not switch the admin's own session.
    create_account(
        &app,
        &Credentials {
            callsign: req.callsign,
            password: req.password,
            display_name: req.display_name,
        },
        role,
        false,
    )
    .await
}

#[derive(Deserialize)]
struct CreateUserReq {
    callsign: String,
    password: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    role: String,
}

async fn create_account(
    app: &AppState,
    req: &Credentials,
    role: &str,
    with_session: bool,
) -> Response {
    if req.callsign.trim().is_empty() || req.password.len() < 6 {
        return err(
            StatusCode::BAD_REQUEST,
            "callsign required, password ≥ 6 chars",
        );
    }
    let hash = match auth::hash_password(&req.password) {
        Ok(h) => h,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let id = match app
        .users
        .db
        .create_user(req.callsign.trim(), &req.display_name, &hash, role)
    {
        Ok(id) => id,
        Err(e) => return err(StatusCode::CONFLICT, e),
    };
    let body = Json(serde_json::json!({
        "id": id, "callsign": req.callsign.trim().to_uppercase(), "role": role,
    }));
    if !with_session {
        return body.into_response();
    }
    match auth::start_session(&app.users.db, id) {
        Ok(cookie) => ([(header::SET_COOKIE, cookie)], body).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn login(State(app): State<AppState>, Json(req): Json<Credentials>) -> Response {
    let found = match app.users.db.user_by_callsign(&req.callsign) {
        Ok(f) => f,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let Some((user, stored_hash)) = found else {
        return err(StatusCode::UNAUTHORIZED, "bad callsign or password");
    };
    if !auth::verify_password(&req.password, &stored_hash) {
        return err(StatusCode::UNAUTHORIZED, "bad callsign or password");
    }
    match auth::start_session(&app.users.db, user.id) {
        Ok(cookie) => (
            [(header::SET_COOKIE, cookie)],
            Json(serde_json::json!(user)),
        )
            .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn logout(State(app): State<AppState>, headers: HeaderMap) -> Response {
    auth::end_session(&app.users.db, &headers);
    (
        [(header::SET_COOKIE, auth::clear_cookie())],
        Json(serde_json::json!({"ok": true})),
    )
        .into_response()
}

async fn me(State(app): State<AppState>, headers: HeaderMap) -> Response {
    match require_user(&app, &headers) {
        Ok(user) => Json(serde_json::json!(user)).into_response(),
        Err(resp) => resp,
    }
}

/// The Spots screen's station card: who is logged in, the callsign their log
/// is for, and the award totals. `stats` is null until they refresh ClubLog
/// — the card then says "no log loaded" instead of showing four zeroes,
/// which would read as a station that has worked nothing.
async fn station(State(app): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_user(&app, &headers) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let cl = app.users.db.clublog_config(user.id).ok();
    let meta = app.users.db.matrix_meta(user.id).ok().flatten();
    Json(serde_json::json!({
        "callsign": user.callsign,
        "display_name": user.display_name,
        // The log's own callsign may differ from the login (a /P or club
        // log), so the card names the one the matrix was built from.
        "log_callsign": cl.as_ref().map(|c| c.callsign.clone()).filter(|c| !c.is_empty()),
        "stats": app.users.stats(user.id),
        "qso_count": meta.map(|m| m.0),
        "last_refresh_unix": meta.map(|m| m.1),
    }))
    .into_response()
}

/// The vocabularies the UI builds its filter controls from — served rather
/// than hardcoded in Svelte so the band list, the mode buckets and the level
/// ladder cannot drift from what the classifier actually emits.
async fn reference() -> Response {
    let levels: Vec<serde_json::Value> = dxca_core::classify::AlertLevel::FLAGGABLE
        .iter()
        .map(|l| serde_json::json!({ "key": l.key(), "label": l.label() }))
        .collect();
    Json(serde_json::json!({
        "bands": dxca_core::bands::SELECTABLE_BANDS,
        "modes": dxca_core::modes::CLASSES,
        "levels": levels,
    }))
    .into_response()
}

async fn list_users(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(resp) = require_admin(&app, &headers) {
        return resp;
    }
    match app.users.db.users() {
        Ok(users) => Json(serde_json::json!({ "users": users })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

// --- per-user config -----------------------------------------------------

async fn get_clublog(State(app): State<AppState>, headers: HeaderMap) -> Response {
    with_user_config(&app, &headers, |db, uid| db.clublog_config(uid))
}

async fn put_clublog(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(cfg): Json<ClubLogUserConfig>,
) -> Response {
    match require_user(&app, &headers) {
        Ok(user) => match app.users.db.set_clublog_config(user.id, &cfg) {
            Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
        },
        Err(resp) => resp,
    }
}

async fn get_notify(State(app): State<AppState>, headers: HeaderMap) -> Response {
    with_user_config(&app, &headers, |db, uid| db.notify_config(uid))
}

async fn put_notify(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(cfg): Json<NotifyUserConfig>,
) -> Response {
    match require_user(&app, &headers) {
        Ok(user) => match app.users.db.set_notify_config(user.id, &cfg) {
            Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
        },
        Err(resp) => resp,
    }
}

fn with_user_config<T: serde::Serialize>(
    app: &AppState,
    headers: &HeaderMap,
    read: impl Fn(&crate::db::Db, i64) -> Result<T, String>,
) -> Response {
    match require_user(app, headers) {
        Ok(user) => match read(&app.users.db, user.id) {
            Ok(cfg) => Json(serde_json::json!(cfg)).into_response(),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
        },
        Err(resp) => resp,
    }
}

// --- global config (M5 web editing, admin) -------------------------------

/// The editable arrays plus the file-only scalars (shown read-only).
async fn get_global(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(resp) = require_admin(&app, &headers) {
        return resp;
    }
    let cfg = app.config.lock().unwrap().clone();
    Json(serde_json::json!({
        "udp_sources": cfg.udp_sources,
        "cluster_nodes": cfg.cluster_nodes,
        "broadcast_destinations": cfg.broadcast_destinations,
        "read_only": {
            "web_bind": cfg.web_bind,
            "telnet_port": cfg.telnet_port,
            "dedupe_window_secs": cfg.dedupe_window_secs,
            "spot_ring_capacity": cfg.spot_ring_capacity,
            "data_dir": cfg.data_dir,
            "lotw_refresh_days": cfg.lotw_refresh_days,
        },
        // When the shared LoTW list was last actually downloaded — 0 = never
        // recorded, which is what a list seeded from a file cache looks like.
        "lotw_last_refresh_unix": app.users.db.meta_unix(crate::refresh::LOTW_OK_KEY),
    }))
    .into_response()
}

#[derive(Deserialize)]
struct GlobalConfigReq {
    udp_sources: Vec<UdpSource>,
    cluster_nodes: Vec<ClusterNode>,
    broadcast_destinations: Vec<BroadcastDestination>,
}

/// Hot-apply + persist the three arrays. Bind failures (port clash)
/// reject the whole request before anything is torn down; persistence
/// failure is reported but the running state is already applied.
async fn put_global(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<GlobalConfigReq>,
) -> Response {
    if let Err(resp) = require_admin(&app, &headers) {
        return resp;
    }

    // Names must be unique — they key counters, status maps, and the
    // spotter field on cluster lines.
    for (label, names) in [
        (
            "source",
            req.udp_sources
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>(),
        ),
        (
            "node",
            req.cluster_nodes.iter().map(|n| n.name.clone()).collect(),
        ),
        (
            "destination",
            req.broadcast_destinations
                .iter()
                .map(|d| d.name.clone())
                .collect(),
        ),
    ] {
        let mut seen = std::collections::HashSet::new();
        for name in &names {
            if name.trim().is_empty() {
                return err(
                    StatusCode::BAD_REQUEST,
                    format!("a {label} has an empty name"),
                );
            }
            if !seen.insert(name.to_uppercase()) {
                return err(
                    StatusCode::BAD_REQUEST,
                    format!("duplicate {label} name: {name}"),
                );
            }
        }
    }

    // Apply: sources first (binds can fail → reject), then destinations
    // and nodes (infallible diffs).
    if let Err(e) = app
        .pipeline
        .apply_sources(&req.udp_sources, &app.input_tx)
        .await
    {
        return err(StatusCode::BAD_REQUEST, format!("source listener: {e}"));
    }
    let new_cfg = {
        let mut cfg = app.config.lock().unwrap();
        cfg.udp_sources = req.udp_sources;
        cfg.cluster_nodes = req.cluster_nodes;
        cfg.broadcast_destinations = req.broadcast_destinations;
        cfg.clone()
    };
    if let Err(e) = app
        .pipeline
        .apply_destinations(new_cfg.broadcast_destinations())
    {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("destinations: {e}"),
        );
    }
    app.nodes.apply(&new_cfg.cluster_nodes, &app.input_tx);

    match new_cfg.save(&app.config_path) {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("applied, but saving the config file failed: {e}"),
        ),
    }
}

/// Send a test message through the caller's Telegram config (M5 button).
async fn telegram_test(State(app): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_user(&app, &headers) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let service = app.users.clone();
    match tokio::task::spawn_blocking(move || service.telegram_test(user.id)).await {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})).into_response(),
        Ok(Err(e)) => err(StatusCode::BAD_GATEWAY, e),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("join: {e}")),
    }
}

/// Refresh the global LoTW users list (admin; the list is server-wide).
async fn lotw_refresh(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(resp) = require_admin(&app, &headers) {
        return resp;
    }
    let service = app.users.clone();
    let result =
        tokio::task::spawn_blocking(move || service.refresh_lotw(dxca_connect::lotw::DEFAULT_URL))
            .await;
    match result {
        Ok(Ok(count)) => Json(serde_json::json!({"lotw_users": count})).into_response(),
        Ok(Err(e)) => err(StatusCode::BAD_GATEWAY, e),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("join: {e}")),
    }
}

// --- ClubLog refresh -----------------------------------------------------

/// Synchronous refresh (download + parse + matrix build) on a blocking
/// task; the response reports the resulting counts, 1.x-status style.
async fn refresh(State(app): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_user(&app, &headers) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let service = app.users.clone();
    let result = tokio::task::spawn_blocking(move || service.refresh_user(user.id)).await;
    match result {
        Ok(Ok((qso_count, dxcc_count))) => Json(serde_json::json!({
            "qso_count": qso_count,
            "dxcc_count": dxcc_count,
        }))
        .into_response(),
        Ok(Err(e)) => err(StatusCode::BAD_GATEWAY, e),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("join: {e}")),
    }
}
