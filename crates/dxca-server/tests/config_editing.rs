//! M5 remainder: web config editing with hot-apply. An admin PUTs a new
//! source list, destination list, and node list through the API; the
//! running pipeline must follow — old listener port freed, new one live,
//! passthrough re-pointed, node client dialing — and the TOML file must
//! be rewritten so a restart agrees with what's running.

use dxca_connect::clublog::Endpoints;
use dxca_connect::telegram::Telegram;
use dxca_server::api::{self, AppState};
use dxca_server::config::{BroadcastDestination, Config, UdpSource};
use dxca_server::db::Db;
use dxca_server::nodes::NodeManager;
use dxca_server::pipeline;
use dxca_server::users::UserService;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, UdpSocket};

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
        (
            status,
            set_cookie,
            serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
        )
    })
    .await
    .unwrap()
}

fn wsjtx_heartbeat() -> Vec<u8> {
    // Any datagram works for the passthrough check; a real header keeps
    // the pipeline parse path quiet.
    let mut d = Vec::new();
    d.extend_from_slice(&0xadbc_cbdau32.to_be_bytes());
    d.extend_from_slice(&2u32.to_be_bytes());
    d.extend_from_slice(&0u32.to_be_bytes()); // Heartbeat
    d.extend_from_slice(&4u32.to_be_bytes());
    d.extend_from_slice(b"JTDX");
    d
}

async fn recv_with_timeout(socket: &UdpSocket, secs: u64) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; 65_536];
    match tokio::time::timeout(
        std::time::Duration::from_secs(secs),
        socket.recv_from(&mut buf),
    )
    .await
    {
        Ok(Ok((n, _))) => Some(buf[..n].to_vec()),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn admin_edits_hot_apply_and_persist() {
    let data_dir = std::env::temp_dir().join(format!("dxca-m5cfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    std::fs::create_dir_all(&data_dir).unwrap();
    let config_path = data_dir.join("dxca.toml");

    // Recorders standing in for passthrough destinations, before and after.
    let dest1 = UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
    let dest2 = UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
    let (p1, p2) = (
        dest1.local_addr().unwrap().port(),
        dest2.local_addr().unwrap().port(),
    );

    // A fake cluster node that just counts connections.
    let node_listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let node_port = node_listener.local_addr().unwrap().port();
    let accepts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let accepts_srv = accepts.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = node_listener.accept().await else {
                return;
            };
            accepts_srv.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut buf = [0u8; 256];
            use tokio::io::AsyncReadExt;
            while matches!(s.read(&mut buf).await, Ok(n) if n > 0) {}
        }
    });

    const PORT_A: u16 = 48_111;
    const PORT_B: u16 = 48_112;
    let cfg = Config {
        telnet_port: 0,
        udp_sources: vec![UdpSource {
            name: "A".into(),
            port: PORT_A,
            enabled: true,
        }],
        broadcast_destinations: vec![BroadcastDestination {
            name: "logger".into(),
            ip: Ipv4Addr::LOCALHOST,
            port: p1,
            format: "passthrough".into(),
            sources: Vec::new(),
            unfiltered: false,
            enabled: true,
        }],
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
        config_path: config_path.clone(),
        input_tx: input_tx.clone(),
    });
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let api_port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = format!("http://127.0.0.1:{api_port}");

    // Baseline: source A feeds passthrough destination 1.
    let sender = UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
    sender
        .send_to(&wsjtx_heartbeat(), ("127.0.0.1", PORT_A))
        .await
        .unwrap();
    assert!(
        recv_with_timeout(&dest1, 5).await.is_some(),
        "baseline passthrough"
    );

    // Admin session; the endpoint is admin-gated.
    let (status, _, _) = http("GET", format!("{base}/api/config/global"), None, None).await;
    assert_eq!(status, 401);
    let (status, cookie, _) = http(
        "POST",
        format!("{base}/api/setup"),
        None,
        Some(serde_json::json!({"callsign": "VU2CPL", "password": "secret1"})),
    )
    .await;
    assert_eq!(status, 200);
    let cookie = cookie.unwrap();

    // GET shows the current arrays.
    let (status, _, body) = http(
        "GET",
        format!("{base}/api/config/global"),
        Some(cookie.clone()),
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["udp_sources"][0]["name"], "A");

    // Duplicate names are rejected before anything is touched.
    let (status, _, body) = http(
        "PUT",
        format!("{base}/api/config/global"),
        Some(cookie.clone()),
        Some(serde_json::json!({
            "udp_sources": [
                {"name": "B", "port": PORT_B}, {"name": "b", "port": PORT_B + 1}
            ],
            "cluster_nodes": [], "broadcast_destinations": [],
        })),
    )
    .await;
    assert_eq!(status, 400, "{body}");

    // The real edit: source A → B (new port), destination → recorder 2,
    // and one cluster node pointing at the fake listener.
    let (status, _, body) = http(
        "PUT",
        format!("{base}/api/config/global"),
        Some(cookie.clone()),
        Some(serde_json::json!({
            "udp_sources": [{"name": "B", "port": PORT_B}],
            "cluster_nodes": [{
                "name": "FAKE", "host": "127.0.0.1", "port": node_port,
                "login_call": "VU2CPL",
            }],
            "broadcast_destinations": [{
                "name": "logger2", "ip": "127.0.0.1", "port": p2,
                "format": "passthrough",
            }],
        })),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    // New source + new destination live.
    let payload = wsjtx_heartbeat();
    sender
        .send_to(&payload, ("127.0.0.1", PORT_B))
        .await
        .unwrap();
    let got = recv_with_timeout(&dest2, 5)
        .await
        .expect("new passthrough path");
    assert_eq!(got, payload, "passthrough byte-identical after re-point");
    // Old destination no longer fed (grace poll: nothing in 1 s).
    assert!(
        recv_with_timeout(&dest1, 1).await.is_none(),
        "old destination still fed"
    );

    // Old source port is free again — we can bind it ourselves.
    drop(
        UdpSocket::bind(("127.0.0.1", PORT_A))
            .await
            .expect("old port freed"),
    );

    // The node client dialed the fake node, and status reports it.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while accepts.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        assert!(std::time::Instant::now() < deadline, "node never dialed");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let (_, _, status_body) = http("GET", format!("{base}/api/status"), None, None).await;
    assert!(
        status_body["cluster_nodes"]["FAKE"].is_object(),
        "{status_body}"
    );

    // Persisted — a restart would agree. But WHERE changed with
    // docs/MULTI-STATION.md: sources and nodes belong to the account that
    // saved them, and only passthrough is still the machine's, so only
    // passthrough is still in the file.
    let reloaded = Config::load(&config_path).unwrap();
    assert!(
        reloaded.udp_sources.is_empty(),
        "sources moved to the account, and stale copies here would let the \
         aggregate's fallback resurrect them: {:?}",
        reloaded.udp_sources
    );
    assert!(reloaded.cluster_nodes.is_empty(), "nodes moved too");
    assert_eq!(
        reloaded.broadcast_destinations.len(),
        1,
        "the passthrough row stays in the file"
    );
    assert_eq!(reloaded.broadcast_destinations[0].port, p2);
    assert_eq!(reloaded.broadcast_destinations[0].format, "passthrough");

    // And the operator gets back what they saved, with bare names — a
    // qualified one coming back would be re-qualified on the next save.
    let (status, _, body) = http(
        "GET",
        format!("{base}/api/config/global"),
        Some(cookie.clone()),
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["udp_sources"][0]["name"], "B", "{body}");
    assert_eq!(body["cluster_nodes"][0]["name"], "FAKE", "{body}");

    let _ = std::fs::remove_dir_all(&data_dir);
}
