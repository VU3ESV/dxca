//! The server-wide call blacklist, through the real HTTP API and the real
//! pipeline.
//!
//! The point of the feature is that a blocked call is dropped *before the
//! ring*, not hidden by a display filter — so the test asserts on
//! `/api/spots`, which is what every screen and the WebSocket both read
//! from, and checks that an edit takes effect on the next spot with no
//! restart.

use dxca_connect::clublog::Endpoints;
use dxca_connect::telegram::Telegram;
use dxca_core::Spot;
use dxca_server::api::{self, AppState};
use dxca_server::config::Config;
use dxca_server::db::Db;
use dxca_server::nodes::NodeManager;
use dxca_server::pipeline::{self, PipelineInput};
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
        let cookie = resp
            .header("set-cookie")
            .and_then(|c| c.split(';').next())
            .map(str::to_string);
        let text = resp.into_string().unwrap_or_default();
        (
            status,
            cookie,
            serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
        )
    })
    .await
    .unwrap()
}

fn spot(call: &str) -> Spot {
    Spot {
        time_unix: 1_787_745_000,
        snr_db: -12,
        delta_time_s: 0.2,
        delta_frequency_hz: 0,
        mode: "FT8".into(),
        mode_inferred: false,
        message: format!("CQ {call}"),
        is_cq: true,
        comment: String::new(),
        low_confidence: false,
        off_air: false,
        dial_frequency_hz: 14_074_000,
        source_name: "TEST".into(),
        spotter: None,
        is_skimmer: false,
    }
}

/// Callsigns currently in the ring, via the API every screen reads.
async fn ring_calls(base: &str) -> Vec<String> {
    let (status, _, body) = http("GET", format!("{base}/api/spots?limit=100"), None, None).await;
    assert_eq!(status, 200);
    body["spots"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["dx_call"].as_str().map(str::to_string))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn blocked_calls_never_reach_the_ring() {
    let data_dir = std::env::temp_dir().join(format!("dxca-blacklist-{}", std::process::id()));
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
    let state = AppState {
        pipeline: pipeline_state,
        nodes: Arc::new(NodeManager::new()),
        users,
        config: Arc::new(Mutex::new(cfg.clone())),
        config_path: data_dir.join("dxca.toml"),
        input_tx: input_tx.clone(),
    };
    let app = api::build_router(state.clone());
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = format!("http://127.0.0.1:{port}");

    let (status, cookie, _) = http(
        "POST",
        format!("{base}/api/setup"),
        None,
        Some(serde_json::json!({"callsign": "VU2CPL", "password": "secret1"})),
    )
    .await;
    assert_eq!(status, 200);
    let admin = cookie.unwrap();

    let send = |call: &str| {
        let tx = input_tx.clone();
        let s = spot(call);
        async move {
            tx.send(PipelineInput::Cluster(s)).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
    };

    // --- baseline: nothing blocked, everything lands ----------------------
    send("K1ABC").await;
    send("R1BAD").await;
    let calls = ring_calls(&base).await;
    assert!(calls.contains(&"K1ABC".to_string()));
    assert!(calls.contains(&"R1BAD".to_string()), "baseline: {calls:?}");

    // --- admin-gated -------------------------------------------------------
    let (status, _, _) = http("GET", format!("{base}/api/blacklist"), None, None).await;
    assert_eq!(status, 401, "anonymous cannot read the list");
    let (status, cookie, _) = http(
        "POST",
        format!("{base}/api/login"),
        None,
        Some(serde_json::json!({"callsign": "VU2CPL", "password": "secret1"})),
    )
    .await;
    assert_eq!(status, 200);
    let _ = cookie;

    // --- block, and it takes effect on the NEXT spot, no restart ----------
    let (status, _, body) = http(
        "POST",
        format!("{base}/api/blacklist"),
        Some(admin.clone()),
        Some(serde_json::json!({"callsign": "r1bad"})),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["callsign"], "R1BAD", "stored uppercase");
    assert_eq!(body["added"], true);

    send("R1BAD").await;
    send("K1ABC").await;
    let calls = ring_calls(&base).await;
    let blocked = calls.iter().filter(|c| *c == "R1BAD").count();
    assert_eq!(
        blocked, 1,
        "only the pre-block spot survives; the new one never entered: {calls:?}"
    );
    assert_eq!(
        calls.iter().filter(|c| *c == "K1ABC").count(),
        2,
        "an unblocked call is untouched"
    );

    // Re-adding is idempotent and says so.
    let (status, _, body) = http(
        "POST",
        format!("{base}/api/blacklist"),
        Some(admin.clone()),
        Some(serde_json::json!({"callsign": "R1BAD"})),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["added"], false, "already listed");

    // --- unblock, and it resumes on the next spot -------------------------
    let (status, _, body) = http(
        "DELETE",
        format!("{base}/api/blacklist/R1BAD"),
        Some(admin.clone()),
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["calls"].as_array().unwrap().len(), 0);

    send("R1BAD").await;
    assert_eq!(
        ring_calls(&base)
            .await
            .iter()
            .filter(|c| *c == "R1BAD")
            .count(),
        2,
        "unblocking lets it through again"
    );

    let (status, _, _) = http(
        "DELETE",
        format!("{base}/api/blacklist/NEVERLISTED"),
        Some(admin),
        None,
    )
    .await;
    assert_eq!(status, 404, "removing something not listed is a 404");

    let _ = std::fs::remove_dir_all(&data_dir);
}
