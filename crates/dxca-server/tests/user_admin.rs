//! Editing and deleting accounts through the real HTTP API, including the
//! two states the guards exist to prevent and the delete-to-zero path that
//! deliberately is allowed.
//!
//! The interesting asymmetry: deleting the last account is FINE (it re-arms
//! `/api/setup`, which is how you start a server over), while demoting or
//! deleting the last admin with other accounts still present is not — that
//! leaves users nobody can administer and a setup endpoint that stays shut
//! because the account count is no longer zero.

use dxca_connect::clublog::Endpoints;
use dxca_connect::telegram::Telegram;
use dxca_server::api::{self, AppState};
use dxca_server::config::Config;
use dxca_server::db::Db;
use dxca_server::nodes::NodeManager;
use dxca_server::pipeline;
use dxca_server::users::UserService;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

async fn http(
    method: &'static str,
    url: String,
    cookie: Option<String>,
    body: Option<serde_json::Value>,
) -> (u16, Option<String>, serde_json::Value) {
    tokio::task::spawn_blocking(move || {
        // `ureq::request` covers PATCH and DELETE too, which the named
        // helpers in the other test files do not.
        let mut req = ureq::request(method, &url);
        if let Some(c) = &cookie {
            req = req.set("Cookie", c);
        }
        let result = match body {
            Some(json) => req.send_json(json),
            None => req.call(),
        };
        let resp = match result {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("http {method} {url}: {e}"),
        };
        let status = resp.status();
        let set_cookie = resp
            .header("set-cookie")
            .and_then(|c| c.split(';').next())
            .map(str::to_string);
        let text = resp.into_string().unwrap_or_default();
        (
            status,
            set_cookie,
            serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
        )
    })
    .await
    .unwrap()
}

/// id of `callsign` in the roster, so the tests never hardcode rowids.
async fn id_of(base: &str, cookie: &str, callsign: &str) -> i64 {
    let (status, _, body) = http(
        "GET",
        format!("{base}/api/users"),
        Some(cookie.into()),
        None,
    )
    .await;
    assert_eq!(status, 200, "list users");
    body["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["callsign"] == callsign)
        .unwrap_or_else(|| panic!("{callsign} not in roster: {body}"))["id"]
        .as_i64()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_and_delete_accounts_down_to_zero() {
    let data_dir = std::env::temp_dir().join(format!("dxca-useradmin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    std::fs::create_dir_all(&data_dir).unwrap();

    let cfg = Config {
        telnet_port: 0,
        udp_sources: Vec::new(),
        broadcast_destinations: Vec::new(),
        cluster_nodes: Vec::new(),
        ..Config::default()
    };
    let (pipeline_state, input_tx) = pipeline::start(&cfg, None).await.unwrap();
    let db = Arc::new(Db::open(&data_dir.join("dxca.db")).unwrap());
    let users = Arc::new(UserService::new(
        db,
        data_dir.to_str().unwrap(),
        Telegram::default(),
        Endpoints::default(),
    ));
    let app = api::build_router(AppState {
        pipeline: pipeline_state,
        nodes: Arc::new(NodeManager::new()),
        users,
        config: Arc::new(Mutex::new(cfg.clone())),
        config_path: data_dir.join("dxca.toml"),
        input_tx,
    });
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = format!("http://127.0.0.1:{port}");

    // --- roster: one admin, two users -------------------------------------
    let (status, cookie, _) = http(
        "POST",
        format!("{base}/api/setup"),
        None,
        Some(serde_json::json!({"callsign": "VU2CPL", "password": "secret1"})),
    )
    .await;
    assert_eq!(status, 200);
    let admin = cookie.unwrap();

    for (call, pw) in [("K1ABC", "secret2"), ("W1XYZ", "secret3")] {
        let (status, _, _) = http(
            "POST",
            format!("{base}/api/users"),
            Some(admin.clone()),
            Some(serde_json::json!({"callsign": call, "password": pw, "role": "user"})),
        )
        .await;
        assert_eq!(status, 200, "create {call}");
    }
    let vu2cpl = id_of(&base, &admin, "VU2CPL").await;
    let k1abc = id_of(&base, &admin, "K1ABC").await;
    let w1xyz = id_of(&base, &admin, "W1XYZ").await;

    // --- both endpoints are admin-gated -----------------------------------
    let (status, cookie, _) = http(
        "POST",
        format!("{base}/api/login"),
        None,
        Some(serde_json::json!({"callsign": "K1ABC", "password": "secret2"})),
    )
    .await;
    assert_eq!(status, 200);
    let plain_user = cookie.unwrap();

    let (status, _, _) = http(
        "PATCH",
        format!("{base}/api/users/{w1xyz}"),
        Some(plain_user.clone()),
        Some(serde_json::json!({"display_name": "hijacked"})),
    )
    .await;
    assert_eq!(status, 403, "non-admin cannot edit");
    let (status, _, _) = http(
        "DELETE",
        format!("{base}/api/users/{w1xyz}"),
        Some(plain_user),
        None,
    )
    .await;
    assert_eq!(status, 403, "non-admin cannot delete");

    // --- ordinary edits ----------------------------------------------------
    let (status, _, body) = http(
        "PATCH",
        format!("{base}/api/users/{w1xyz}"),
        Some(admin.clone()),
        Some(serde_json::json!({"display_name": "Willy"})),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["user"]["display_name"], "Willy");
    assert_eq!(
        body["user"]["callsign"], "W1XYZ",
        "untouched field survives"
    );

    // Rename onto a taken callsign is refused before anything is written.
    let (status, _, body) = http(
        "PATCH",
        format!("{base}/api/users/{w1xyz}"),
        Some(admin.clone()),
        Some(serde_json::json!({"callsign": "K1ABC"})),
    )
    .await;
    assert_eq!(status, 409);
    assert!(
        body["error"].as_str().unwrap().contains("K1ABC"),
        "names the clash: {body}"
    );
    assert_eq!(
        id_of(&base, &admin, "W1XYZ").await,
        w1xyz,
        "the refused rename left the row alone"
    );

    // Rename succeeds and uppercases, so login (which uppercases too) matches.
    let (status, _, body) = http(
        "PATCH",
        format!("{base}/api/users/{w1xyz}"),
        Some(admin.clone()),
        Some(serde_json::json!({"callsign": "w1new"})),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["user"]["callsign"], "W1NEW");

    // Validation matches account creation: one rule, not two.
    let (status, _, _) = http(
        "PATCH",
        format!("{base}/api/users/{w1xyz}"),
        Some(admin.clone()),
        Some(serde_json::json!({"password": "short"})),
    )
    .await;
    assert_eq!(status, 400, "password floor");
    let (status, _, _) = http(
        "PATCH",
        format!("{base}/api/users/{w1xyz}"),
        Some(admin.clone()),
        Some(serde_json::json!({"role": "Admin"})),
    )
    .await;
    assert_eq!(status, 400, "role is exactly 'user' or 'admin'");

    // Password change takes effect: the old one stops working.
    let (status, _, _) = http(
        "PATCH",
        format!("{base}/api/users/{w1xyz}"),
        Some(admin.clone()),
        Some(serde_json::json!({"password": "newpass1"})),
    )
    .await;
    assert_eq!(status, 200);
    let (status, _, _) = http(
        "POST",
        format!("{base}/api/login"),
        None,
        Some(serde_json::json!({"callsign": "W1NEW", "password": "secret3"})),
    )
    .await;
    assert_eq!(status, 401, "old password rejected");
    let (status, _, _) = http(
        "POST",
        format!("{base}/api/login"),
        None,
        Some(serde_json::json!({"callsign": "W1NEW", "password": "newpass1"})),
    )
    .await;
    assert_eq!(status, 200, "new password works");

    // --- the two guarded states -------------------------------------------
    let (status, _, body) = http(
        "PATCH",
        format!("{base}/api/users/{vu2cpl}"),
        Some(admin.clone()),
        Some(serde_json::json!({"role": "user"})),
    )
    .await;
    assert_eq!(status, 409, "cannot demote the only admin");
    assert!(body["error"].as_str().unwrap().contains("only admin"));

    let (status, _, body) = http(
        "DELETE",
        format!("{base}/api/users/{vu2cpl}"),
        Some(admin.clone()),
        None,
    )
    .await;
    assert_eq!(
        status, 409,
        "cannot delete the only admin while users remain"
    );
    assert!(body["error"].as_str().unwrap().contains("only admin"));

    // With a second admin in place, both become legal.
    let (status, _, _) = http(
        "PATCH",
        format!("{base}/api/users/{k1abc}"),
        Some(admin.clone()),
        Some(serde_json::json!({"role": "admin"})),
    )
    .await;
    assert_eq!(status, 200);
    let (status, _, body) = http(
        "DELETE",
        format!("{base}/api/users/{vu2cpl}"),
        Some(admin.clone()),
        None,
    )
    .await;
    assert_eq!(status, 200, "deleting an admin is fine once another exists");
    assert_eq!(body["deleted"], "VU2CPL");
    assert_eq!(body["remaining"], 2);

    // That cookie belonged to the deleted account — sessions cascade, so it
    // is dead now, which is exactly why the UI reloads after a self-delete.
    let (status, _, _) = http("GET", format!("{base}/api/users"), Some(admin), None).await;
    assert_eq!(status, 401, "deleted admin's session died with the row");

    // --- delete all the way down to zero ----------------------------------
    let (status, cookie, _) = http(
        "POST",
        format!("{base}/api/login"),
        None,
        Some(serde_json::json!({"callsign": "K1ABC", "password": "secret2"})),
    )
    .await;
    assert_eq!(status, 200);
    let admin2 = cookie.unwrap();

    let (status, _, body) = http(
        "DELETE",
        format!("{base}/api/users/{w1xyz}"),
        Some(admin2.clone()),
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["remaining"], 1);

    // The last account, and it is the last admin — allowed, because zero
    // accounts is a recoverable state and one-user-no-admin is not.
    let (status, _, body) = http(
        "DELETE",
        format!("{base}/api/users/{k1abc}"),
        Some(admin2),
        None,
    )
    .await;
    assert_eq!(status, 200, "the last account can be deleted");
    assert_eq!(body["remaining"], 0);

    // And first-run setup is armed again.
    let (status, _, _) = http(
        "POST",
        format!("{base}/api/setup"),
        None,
        Some(serde_json::json!({"callsign": "VU2NEW", "password": "secret9"})),
    )
    .await;
    assert_eq!(status, 200, "empty roster re-arms /api/setup");

    let _ = std::fs::remove_dir_all(&data_dir);
}
