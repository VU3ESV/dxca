//! The HTTP API (plan §7). Session-cookie auth; admin role gates user
//! management; per-user config and classification for the authenticated
//! account. The embedded web UI is served by the fallback.

use crate::auth;
use crate::db::{ClubLogUserConfig, NotifyUserConfig, User};
use crate::nodes::NodeManager;
use crate::pipeline::PipelineState;
use crate::users::UserService;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pipeline: Arc<PipelineState>,
    pub nodes: Arc<NodeManager>,
    pub users: Arc<UserService>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/spots", get(spots))
        .route("/api/setup", post(setup))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/config/me/clublog", get(get_clublog).put(put_clublog))
        .route(
            "/api/config/me/notifications",
            get(get_notify).put(put_notify),
        )
        .route("/api/clublog/refresh", post(refresh))
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

async fn status(State(app): State<AppState>) -> Json<serde_json::Value> {
    let counters = app.pipeline.broadcaster.counters();
    let user_count = app.users.db.user_count().unwrap_or(0);
    Json(serde_json::json!({
        "name": "dxca",
        "version": env!("CARGO_PKG_VERSION"),
        "milestone": "M4 users + alerts",
        "setup_required": user_count == 0,
        "users": user_count,
        "cty_loaded": app.users.resolver_loaded(),
        "cty_entities": app.users.entity_count(),
        "telnet_clients": app.pipeline.telnet.client_count(),
        "spots_per_source": *app.pipeline.source_counts.lock().unwrap(),
        "cluster_nodes": app.nodes.statuses(),
        "udp_sent": counters.total_sent(),
        "udp_failed": counters.total_failed(),
    }))
}

#[derive(Deserialize)]
struct SpotsQuery {
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    200
}

/// Recent spots; when a session cookie is present each spot carries that
/// user's classification (alert level, DXCC name, band) — plan §5's
/// per-user highlighting, JSON edition.
async fn spots(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SpotsQuery>,
) -> Json<serde_json::Value> {
    let user = auth::user_from_headers(&app.users.db, &headers);
    let spots = app.pipeline.recent_spots(q.limit.min(2000));
    let annotated: Vec<serde_json::Value> = spots
        .iter()
        .map(|s| {
            let mut v = serde_json::to_value(s).expect("spot serializes");
            if let Some(u) = &user
                && let Some(c) = app.users.classify(u.id, s)
            {
                v["alert"] = serde_json::to_value(c.level).unwrap();
                v["dxcc_name"] = serde_json::to_value(&c.dxcc_name).unwrap();
                v["band"] = serde_json::to_value(c.band).unwrap();
                v["is_beacon"] = serde_json::Value::Bool(c.is_beacon);
            }
            v
        })
        .collect();
    Json(serde_json::json!({ "spots": annotated }))
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
