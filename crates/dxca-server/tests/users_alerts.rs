//! M4 exit criterion (docs/PLAN.md §10): two accounts with different logs
//! see different highlights on the same spot stream, and each gets only
//! their own Telegram pings — exercised through the REAL flows: HTTP API
//! with session cookies, ClubLog refresh against a fake ClubLog server
//! (gzipped cty.xml + per-user ADIF), classification over an injected
//! spot, and Telegram fan-out against a fake bot API with cooldown.

use dxca_connect::clublog::Endpoints;
use dxca_connect::telegram::Telegram;
use dxca_core::Spot;
use dxca_server::api::{self, AppState};
use dxca_server::config::Config;
use dxca_server::db::Db;
use dxca_server::nodes::NodeManager;
use dxca_server::pipeline::{self, PipelineInput};
use dxca_server::users::UserService;
use std::io::Write as _;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const CTY_XML: &str = r#"<?xml version="1.0"?>
<clublog>
 <entities>
  <entity><adif>324</adif><name>INDIA</name><prefix>VU</prefix><deleted>false</deleted><cqz>22</cqz><cont>AS</cont></entity>
  <entity><adif>291</adif><name>UNITED STATES</name><prefix>K</prefix><deleted>false</deleted><cqz>5</cqz><cont>NA</cont></entity>
 </entities>
 <exceptions/>
 <prefixes/>
</clublog>"#;

/// User A (VU2CPL) has worked India on 20M FT8; user B (K1ABC) has only
/// worked the US — India is a brand-new DXCC for B.
const ADIF_A: &str =
    "<eoh><CALL:6>VU2AAA<BAND:3>20M<MODE:3>FT8<eor><CALL:4>K9XX<BAND:3>20M<MODE:3>FT8<eor>";
const ADIF_B: &str = "<eoh><CALL:4>K9XX<BAND:3>20M<MODE:3>FT8<eor>";

/// Minimal HTTP/1.1 server: reads one request (headers + Content-Length
/// body), hands it to the handler, writes the response, closes.
async fn spawn_http<F>(handler: F) -> u16
where
    F: Fn(&str, &str) -> (u16, Vec<u8>) + Send + Sync + 'static,
{
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handler = Arc::new(handler);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let handler = handler.clone();
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                // Read until the full head + declared body is in.
                loop {
                    let Ok(n) = stream.read(&mut buf).await else {
                        return;
                    };
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(head_end) = find_head_end(&raw) {
                        let head = String::from_utf8_lossy(&raw[..head_end]).into_owned();
                        let content_len = head
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if raw.len() >= head_end + 4 + content_len {
                            let body = String::from_utf8_lossy(
                                &raw[head_end + 4..head_end + 4 + content_len],
                            )
                            .into_owned();
                            let request_line = head.lines().next().unwrap_or("").to_string();
                            let (status, payload) = handler(&request_line, &body);
                            let resp = format!(
                                "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                payload.len()
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.write_all(&payload).await;
                            return;
                        }
                    }
                }
            });
        }
    });
    port
}

fn find_head_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

/// Blocking HTTP helper (ureq) run off the async runtime.
async fn http(
    method: &'static str,
    url: String,
    cookie: Option<String>,
    body: Option<serde_json::Value>,
) -> (u16, Option<String>, serde_json::Value) {
    tokio::task::spawn_blocking(move || {
        let mut req = match method {
            "GET" => ureq::get(&url),
            "PUT" => ureq::put(&url),
            _ => ureq::post(&url),
        };
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
        let json = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        (status, set_cookie, json)
    })
    .await
    .unwrap()
}

async fn wait_until<F: Fn() -> bool>(what: &str, deadline_s: u64, f: F) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(deadline_s);
    while !f() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

fn test_spot() -> Spot {
    Spot {
        time_unix: 1_787_800_000,
        snr_db: -11,
        delta_time_s: 0.1,
        delta_frequency_hz: 1500,
        mode: "FT8".into(),
        mode_inferred: false,
        message: "CQ VU2ZZZ MK83".into(),
        is_cq: true,
        comment: String::new(),
        low_confidence: false,
        off_air: false,
        dial_frequency_hz: 14_074_000,
        source_name: "JTDX".into(),
        spotter: None,
        is_skimmer: false,
        grid: None,
        iota: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_users_same_stream_different_highlights_and_pings() {
    // Fake ClubLog: gzipped cty.xml; per-user ADIF keyed off the call field.
    let clublog_port = spawn_http(|request_line, body| {
        if request_line.starts_with("GET /cty.php") {
            (200, gzip(CTY_XML.as_bytes()))
        } else if body.contains("call=VU2CPL") {
            (200, ADIF_A.as_bytes().to_vec())
        } else {
            (200, ADIF_B.as_bytes().to_vec())
        }
    })
    .await;

    // Fake Telegram: record every sendMessage (path carries the bot token).
    let pings: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let pings_srv = pings.clone();
    let telegram_port = spawn_http(move |request_line, body| {
        pings_srv
            .lock()
            .unwrap()
            .push((request_line.to_string(), body.to_string()));
        (200, b"{\"ok\":true}".to_vec())
    })
    .await;

    // Compose the app the way main() does, with a scratch data dir.
    let data_dir = std::env::temp_dir().join(format!("dxca-m4-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    let cfg = Config {
        telnet_port: 0,
        udp_sources: Vec::new(),
        ..Config::default()
    };
    let (pipeline_state, input_tx) = pipeline::start(&cfg, None).await.unwrap();
    let db = Arc::new(Db::open(&data_dir.join("dxca.db")).unwrap());
    let users = Arc::new(UserService::new(
        db,
        data_dir.to_str().unwrap(),
        Telegram::with_base(&format!("http://127.0.0.1:{telegram_port}")),
        Endpoints::single_base(&format!("http://127.0.0.1:{clublog_port}")),
    ));
    let mut spot_rx = pipeline_state.spot_events.subscribe();
    let fan_out_users = users.clone();
    tokio::spawn(async move {
        while let Ok(spot) = spot_rx.recv().await {
            fan_out_users.fan_out(&spot);
        }
    });
    let app = api::build_router(AppState {
        pipeline: pipeline_state,
        nodes: Arc::new(NodeManager::new()),
        users,
        config: Arc::new(Mutex::new(cfg.clone())),
        config_path: data_dir.join("dxca.toml"),
        input_tx: input_tx.clone(),
    });
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let api_port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = format!("http://127.0.0.1:{api_port}");

    // First-run setup creates the admin (user A) and logs them in.
    let (status, cookie_a, _) = http(
        "POST",
        format!("{base}/api/setup"),
        None,
        Some(serde_json::json!({"callsign": "VU2CPL", "password": "secret1"})),
    )
    .await;
    assert_eq!(status, 200);
    let cookie_a = cookie_a.expect("setup returns a session cookie");
    // Second setup attempt is refused.
    let (status, _, _) = http(
        "POST",
        format!("{base}/api/setup"),
        None,
        Some(serde_json::json!({"callsign": "EVIL", "password": "xxxxxx"})),
    )
    .await;
    assert_eq!(status, 403);

    // Admin creates user B (no session switch), B logs in.
    let (status, cookie, _) = http(
        "POST",
        format!("{base}/api/users"),
        Some(cookie_a.clone()),
        Some(serde_json::json!({"callsign": "K1ABC", "password": "secret2"})),
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        cookie.is_none(),
        "admin-created accounts must not switch the session"
    );
    let (status, cookie_b, _) = http(
        "POST",
        format!("{base}/api/login"),
        None,
        Some(serde_json::json!({"callsign": "k1abc", "password": "secret2"})),
    )
    .await;
    assert_eq!(status, 200);
    let cookie_b = cookie_b.expect("login returns a session cookie");

    // cty.xml is SERVER-wide now: one API key, set by an admin, fetched
    // once — not something either user carries. A is the admin.
    let (status, _, body) = http(
        "PUT",
        format!("{base}/api/config/global"),
        Some(cookie_a.clone()),
        Some(serde_json::json!({
            "udp_sources": [], "cluster_nodes": [], "broadcast_destinations": [],
            "clublog_api_key": "test-key",
        })),
    )
    .await;
    assert_eq!(status, 200, "set server API key: {body}");
    let (status, _, body) = http(
        "POST",
        format!("{base}/api/cty/refresh"),
        Some(cookie_a.clone()),
        None,
    )
    .await;
    assert_eq!(status, 200, "cty refresh: {body}");

    // A non-admin must not be able to touch a server-wide resource.
    let (status, _, _) = http(
        "POST",
        format!("{base}/api/cty/refresh"),
        Some(cookie_b.clone()),
        None,
    )
    .await;
    assert_eq!(status, 403, "cty refresh is admin-only");

    // Per-user ClubLog + Telegram config: credentials for THEIR OWN log.
    for (cookie, call, token) in [(&cookie_a, "VU2CPL", "tokA"), (&cookie_b, "K1ABC", "tokB")] {
        let (status, _, _) = http(
            "PUT",
            format!("{base}/api/config/me/clublog"),
            Some(cookie.clone()),
            Some(serde_json::json!({
                "callsign": call, "email": "x@y.z", "app_password": "pw",
            })),
        )
        .await;
        assert_eq!(status, 200);
        let (status, _, _) = http(
            "PUT",
            format!("{base}/api/config/me/notifications"),
            Some(cookie.clone()),
            Some(serde_json::json!({
                "telegram_enabled": true, "telegram_bot_token": token,
                "telegram_chat_id": "42", "cooldown_minutes": 5,
            })),
        )
        .await;
        assert_eq!(status, 200);
    }

    // Refresh both users through the fake ClubLog.
    let (status, _, body) = http(
        "POST",
        format!("{base}/api/clublog/refresh"),
        Some(cookie_a.clone()),
        None,
    )
    .await;
    assert_eq!(status, 200, "refresh A: {body}");
    assert_eq!(body["qso_count"], 2);
    assert_eq!(body["dxcc_count"], 2);
    let (status, _, body) = http(
        "POST",
        format!("{base}/api/clublog/refresh"),
        Some(cookie_b.clone()),
        None,
    )
    .await;
    assert_eq!(status, 200, "refresh B: {body}");
    assert_eq!(body["dxcc_count"], 1);

    // One spot, same stream: VU2ZZZ on 20M FT8.
    input_tx
        .send(PipelineInput::Cluster(test_spot()))
        .await
        .unwrap();

    // Different highlights per account (the M4 exit criterion). Poll until
    // the pipeline task has pushed the spot into the ring.
    let mut body_a = serde_json::Value::Null;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while body_a["spots"][0].is_null() {
        assert!(
            std::time::Instant::now() < deadline,
            "spot never reached the ring"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        (_, _, body_a) = http(
            "GET",
            format!("{base}/api/spots?limit=1"),
            Some(cookie_a.clone()),
            None,
        )
        .await;
    }
    let (_, _, body_b) = http(
        "GET",
        format!("{base}/api/spots?limit=1"),
        Some(cookie_b.clone()),
        None,
    )
    .await;
    assert_eq!(
        body_a["spots"][0]["alert"], "worked",
        "A worked India 20M DATA: {body_a}"
    );
    assert_eq!(body_a["spots"][0]["dxcc_name"], "INDIA");
    assert_eq!(
        body_b["spots"][0]["alert"], "newDXCC",
        "India is new for B: {body_b}"
    );
    // Anonymous view carries no classification.
    let (_, _, body_anon) = http("GET", format!("{base}/api/spots?limit=1"), None, None).await;
    assert!(body_anon["spots"][0].get("alert").is_none());

    // Telegram: only B (new DXCC) gets pinged, on B's own bot token.
    let p = pings.clone();
    wait_until("B's telegram ping", 10, || !p.lock().unwrap().is_empty()).await;
    {
        let recorded = pings.lock().unwrap();
        assert_eq!(recorded.len(), 1, "exactly one ping: {recorded:?}");
        assert!(
            recorded[0].0.contains("/bottokB/"),
            "ping went to B: {recorded:?}"
        );
        assert!(recorded[0].1.contains("NEW DXCC"));
        assert!(recorded[0].1.contains("VU2ZZZ"));
    }

    // Cooldown: the same call again within the window pings nobody twice.
    input_tx
        .send(PipelineInput::Cluster(test_spot()))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_eq!(
        pings.lock().unwrap().len(),
        1,
        "cooldown must suppress the repeat"
    );

    // --- the My Alerts history -------------------------------------------
    // The fan-out is fire-and-forget on a background thread, so before this
    // existed there was no way to tell a delivered alert from a suppressed
    // one. It records per user, so A — who was pinged for nothing — must see
    // an empty list even though B was alerted on the same spot.
    let (status, _, body) = http(
        "GET",
        format!("{base}/api/me/alerts"),
        Some(cookie_b.clone()),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let alerts = body["alerts"].as_array().unwrap();
    assert_eq!(alerts.len(), 1, "one alert recorded for B: {body}");
    assert_eq!(alerts[0]["callsign"], "VU2ZZZ");
    assert_eq!(alerts[0]["level"], "newDXCC");
    assert_eq!(alerts[0]["delivered"], true);
    assert_eq!(alerts[0]["error"], "");
    assert_eq!(alerts[0]["band"], "20M");
    assert!(
        alerts[0]["dxcc_name"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "the entity is carried too: {}",
        alerts[0]
    );

    let (status, _, body) = http(
        "GET",
        format!("{base}/api/me/alerts"),
        Some(cookie_a.clone()),
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        body["alerts"].as_array().unwrap().is_empty(),
        "A was never pinged, so A's history is empty: {body}"
    );

    let (status, _, _) = http("GET", format!("{base}/api/me/alerts"), None, None).await;
    assert_eq!(status, 401, "the history needs a session");

    let _ = std::fs::remove_dir_all(&data_dir);
}
