//! MQTT publishing, against a real broker socket.
//!
//! The other option was to assert on the config round-trip and trust
//! rumqttc, which would have proved nothing about topics, credentials or
//! payloads — the three things a consumer actually depends on. So this test
//! stands up a minimal MQTT 3.1.1 listener, points a destination at it
//! through the HTTP API, pushes a spot through the real pipeline, and reads
//! what genuinely arrived on the socket.
//!
//! The broker understands exactly what rumqttc sends it: CONNECT (answered
//! with CONNACK), PUBLISH at QoS 0, and PINGREQ. Nothing else is needed and
//! nothing else is implemented.

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
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

#[derive(Debug, Default, Clone)]
struct Seen {
    /// (client_id, username, password) from CONNECT.
    credentials: Option<(String, String, String)>,
    /// (topic, payload) per PUBLISH, in arrival order.
    published: Vec<(String, String)>,
}

/// MQTT's variable-length integer: 7 bits per byte, top bit = continue.
fn read_varint(buf: &[u8], at: &mut usize) -> Option<usize> {
    let mut value = 0usize;
    let mut shift = 0;
    loop {
        let byte = *buf.get(*at)?;
        *at += 1;
        value |= ((byte & 0x7F) as usize) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift > 21 {
            return None;
        }
    }
}

/// MQTT string: two-byte big-endian length, then UTF-8.
fn read_string(buf: &[u8], at: &mut usize) -> Option<String> {
    let len = u16::from_be_bytes([*buf.get(*at)?, *buf.get(*at + 1)?]) as usize;
    *at += 2;
    let s = String::from_utf8_lossy(buf.get(*at..*at + len)?).into_owned();
    *at += len;
    Some(s)
}

async fn spawn_broker(seen: Arc<Mutex<Seen>>) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let Ok(n) = sock.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);

                    // Drain whole packets; leave a partial one buffered.
                    loop {
                        if buf.is_empty() {
                            break;
                        }
                        let packet_type = buf[0] >> 4;
                        let mut at = 1;
                        let Some(remaining) = read_varint(&buf, &mut at) else {
                            break;
                        };
                        if buf.len() < at + remaining {
                            break; // not all here yet
                        }
                        let body = buf[at..at + remaining].to_vec();
                        buf.drain(..at + remaining);

                        match packet_type {
                            1 => {
                                // CONNECT: protocol name, level, flags,
                                // keepalive, then client id [, user, pass].
                                let mut p = 0;
                                let _proto = read_string(&body, &mut p);
                                p += 1; // level
                                let flags = body.get(p).copied().unwrap_or(0);
                                p += 1;
                                p += 2; // keepalive
                                let client_id = read_string(&body, &mut p).unwrap_or_default();
                                let user = if flags & 0x80 != 0 {
                                    read_string(&body, &mut p).unwrap_or_default()
                                } else {
                                    String::new()
                                };
                                let pass = if flags & 0x40 != 0 {
                                    read_string(&body, &mut p).unwrap_or_default()
                                } else {
                                    String::new()
                                };
                                seen.lock().unwrap().credentials = Some((client_id, user, pass));
                                // CONNACK, session-present 0, accepted 0.
                                let _ = sock.write_all(&[0x20, 0x02, 0x00, 0x00]).await;
                            }
                            3 => {
                                // PUBLISH at QoS 0: topic then payload, no
                                // packet identifier.
                                let mut p = 0;
                                let topic = read_string(&body, &mut p).unwrap_or_default();
                                let payload = String::from_utf8_lossy(&body[p..]).into_owned();
                                seen.lock().unwrap().published.push((topic, payload));
                            }
                            12 => {
                                let _ = sock.write_all(&[0xD0, 0x00]).await; // PINGRESP
                            }
                            14 => return, // DISCONNECT
                            _ => {}
                        }
                    }
                }
            });
        }
    });
    port
}

fn spot(call: &str) -> Spot {
    Spot {
        time_unix: 1_787_745_000,
        snr_db: -10,
        delta_time_s: 0.0,
        delta_frequency_hz: 0,
        mode: "FT8".into(),
        mode_inferred: false,
        message: format!("CQ {call}"),
        is_cq: true,
        comment: "FT8 -10 dB".into(),
        low_confidence: false,
        off_air: false,
        dial_frequency_hz: 14_074_000,
        source_name: "VU2OY".into(),
        spotter: None,
        is_skimmer: false,
        grid: None,
        iota: None,
    }
}

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

/// Poll until the broker has seen `n` publishes, or give up.
async fn wait_for_publishes(seen: &Arc<Mutex<Seen>>, n: usize) -> Vec<(String, String)> {
    for _ in 0..100 {
        {
            let s = seen.lock().unwrap();
            if s.published.len() >= n {
                return s.published.clone();
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    seen.lock().unwrap().published.clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn spots_are_published_to_both_topics_with_credentials() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let broker_port = spawn_broker(seen.clone()).await;

    let data_dir = std::env::temp_dir().join(format!("dxca-mqtt-{}", std::process::id()));
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
    let api_port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = format!("http://127.0.0.1:{api_port}");

    let (status, cookie, _) = http(
        "POST",
        format!("{base}/api/setup"),
        None,
        Some(serde_json::json!({"callsign": "VU2CPL", "password": "secret1"})),
    )
    .await;
    assert_eq!(status, 200);
    let admin = cookie.unwrap();

    // --- admin-gated, and validated before anything is stored -------------
    let (status, _, _) = http("GET", format!("{base}/api/mqtt"), None, None).await;
    assert_eq!(status, 401);

    let (status, _, body) = http(
        "PUT",
        format!("{base}/api/mqtt"),
        Some(admin.clone()),
        Some(serde_json::json!({"destinations": [
            {"name": "", "host": "x", "topic": "t"}
        ]})),
    )
    .await;
    assert_eq!(status, 400, "a nameless destination is refused");
    assert!(body["error"].as_str().unwrap().contains("name"));

    let (status, _, body) = http(
        "PUT",
        format!("{base}/api/mqtt"),
        Some(admin.clone()),
        Some(serde_json::json!({"destinations": [
            {"name": "a", "host": "h", "topic": "t"},
            {"name": "A", "host": "h", "topic": "t"}
        ]})),
    )
    .await;
    assert_eq!(
        status, 400,
        "duplicate names are refused case-insensitively"
    );
    assert!(body["error"].as_str().unwrap().contains("duplicate"));

    // --- the real destination ---------------------------------------------
    let (status, _, body) = http(
        "PUT",
        format!("{base}/api/mqtt"),
        Some(admin.clone()),
        Some(serde_json::json!({"destinations": [{
            "name": "panadapter",
            "host": "127.0.0.1",
            "port": broker_port,
            "username": "svc",
            "password": "brokerpw",
            // Trailing slash on purpose: the publisher must not emit
            // "shack/dxca/spots//json".
            "topic": "shack/dxca/spots/",
            "client_id": "dxca-test",
            "enabled": true
        }]})),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["connected"], 1);

    // --- push a spot through the real pipeline ----------------------------
    input_tx
        .send(PipelineInput::Cluster(spot("K1JT")))
        .await
        .unwrap();

    let published = wait_for_publishes(&seen, 2).await;
    assert!(
        published.len() >= 2,
        "expected a json and a cluster publish, got {published:?}"
    );

    let creds = seen.lock().unwrap().credentials.clone();
    let (client_id, user, pass) = creds.expect("broker saw a CONNECT");
    assert_eq!(client_id, "dxca-test");
    assert_eq!(user, "svc", "username reached the broker");
    assert_eq!(pass, "brokerpw", "password reached the broker");

    let json = published
        .iter()
        .find(|(t, _)| t == "shack/dxca/spots/json")
        .unwrap_or_else(|| panic!("no json topic in {published:?}"));
    let v: serde_json::Value = serde_json::from_str(&json.1).unwrap();
    assert_eq!(v["callsign"], "K1JT");
    assert_eq!(v["band"], "20M");
    assert_eq!(v["mode"], "FT8");
    assert_eq!(v["frequency_hz"], 14_074_000u64);
    assert_eq!(v["comment"], "FT8 -10 dB");
    assert_eq!(v["is_cq"], true);

    let line = published
        .iter()
        .find(|(t, _)| t == "shack/dxca/spots/cluster")
        .unwrap_or_else(|| panic!("no cluster topic in {published:?}"));
    assert!(
        line.1.starts_with("DX de ") && line.1.contains("K1JT"),
        "cluster payload is the DX-Spider line: {:?}",
        line.1
    );

    // --- disabling stops the publishing ------------------------------------
    let (status, _, body) = http(
        "PUT",
        format!("{base}/api/mqtt"),
        Some(admin),
        Some(serde_json::json!({"destinations": [{
            "name": "panadapter",
            "host": "127.0.0.1",
            "port": broker_port,
            "topic": "shack/dxca/spots",
            "enabled": false
        }]})),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["connected"], 0, "a disabled row connects nothing");

    let before = seen.lock().unwrap().published.len();
    input_tx
        .send(PipelineInput::Cluster(spot("W1AW")))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert_eq!(
        seen.lock().unwrap().published.len(),
        before,
        "nothing more should be published once disabled"
    );

    let _ = std::fs::remove_dir_all(&data_dir);
}
