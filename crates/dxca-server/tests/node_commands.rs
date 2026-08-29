//! Milestone 1 of `docs/TELNET-INTERACTIVE.md`: the command round trip
//! against a real (fake) DXSpider node, through the real NodeManager.
//!
//! The router's queueing and terminator logic is unit-tested in
//! `cmdrouter.rs`; what those tests cannot prove is that the plumbing either
//! side of it exists — that a command actually reaches a node's socket, and
//! that the node's reply actually comes back out of NodeManager instead of
//! being dropped in a match arm, which is exactly where it used to go.
//!
//! Nothing here is user-facing yet: no telnet session, no auth. This is the
//! wiring, proven end to end before anything is built on it.

use dxca_connect::dxcluster::{ClientConfig, ClientEvent};
use dxca_server::cmdrouter::{CommandRouter, RouterAction, SessionId};
use dxca_server::config::Config;
use dxca_server::nodes::NodeManager;
use dxca_server::pipeline;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SESSION: SessionId = 42;

fn test_client_cfg(port: u16) -> ClientConfig {
    let mut cfg = ClientConfig::new("127.0.0.1", port, "VU2CPL");
    cfg.reconnect_schedule_s = vec![1, 1];
    cfg.auth_timeout_s = 1;
    cfg.silence_timeout_s = 600;
    cfg.init_commands = Vec::new();
    cfg
}

async fn wait_until<F: Fn() -> bool>(what: &str, deadline_s: u64, f: F) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(deadline_s);
    while !f() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// A command written by the router reaches the node's socket, and the
/// node's reply comes back to the session that asked — with the closing
/// prompt freeing the slot.
#[tokio::test]
async fn command_reaches_the_node_and_its_reply_comes_back() {
    let node = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let node_port = node.local_addr().unwrap().port();

    // The fake node: log in, then answer `sh/wwv` with two lines and a
    // prompt — the shape a real DXSpider session has.
    let (saw_cmd_tx, saw_cmd_rx) = tokio::sync::oneshot::channel::<String>();
    tokio::spawn(async move {
        let (mut s, _) = node.accept().await.unwrap();
        s.write_all(b"login: ").await.unwrap();
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).contains("VU2CPL"));
        s.write_all(b"Hello VU2CPL, welcome to the test node\r\n")
            .await
            .unwrap();

        // Wait for the command the router sends.
        let n = s.read(&mut buf).await.unwrap();
        let got = String::from_utf8_lossy(&buf[..n]).trim().to_string();
        s.write_all(b"WWV de AR0CKET <18Z> SFI=140 A=7 K=2\r\n")
            .await
            .unwrap();
        s.write_all(b"  no storms observed\r\n").await.unwrap();
        s.write_all(b"DB0SUE de VU2CPL >\r\n").await.unwrap();
        let _ = saw_cmd_tx.send(got);
        let _ = s.read(&mut buf).await; // hold open
    });

    let cfg = Config {
        telnet_port: 0,
        udp_sources: Vec::new(),
        ..Config::default()
    };
    let (_state, input_tx) = pipeline::start(&cfg, None).await.expect("pipeline");
    let nodes = Arc::new(NodeManager::new());
    let mut lines = nodes.subscribe_lines();
    nodes.start_node("DB0SUE".into(), test_client_cfg(node_port), input_tx);

    wait_until("node proven live", 15, || {
        nodes.statuses().get("DB0SUE").is_some_and(|s| s.proven)
    })
    .await;

    // Router says "write this"; the manager is what actually writes it.
    let mut router = CommandRouter::new();
    let actions = router.submit("DB0SUE", SESSION, "sh/wwv".into(), 0);
    for action in actions {
        match action {
            RouterAction::ToNode { node, line } => {
                assert!(nodes.send_line(&node, &line), "node must be known");
            }
            other => panic!("expected a node write, got {other:?}"),
        }
    }

    let seen = tokio::time::timeout(std::time::Duration::from_secs(10), saw_cmd_rx)
        .await
        .expect("node never received the command")
        .expect("sender dropped");
    assert_eq!(seen, "sh/wwv", "the node got exactly what was submitted");

    // Now drain the reply back through the router, as the telnet layer will.
    let mut to_session: Vec<String> = Vec::new();
    let mut closed = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !closed {
        assert!(
            std::time::Instant::now() < deadline,
            "reply never completed"
        );
        let Ok(Ok(node_line)) =
            tokio::time::timeout(std::time::Duration::from_millis(500), lines.recv()).await
        else {
            continue;
        };
        let is_prompt = matches!(node_line.event, ClientEvent::Prompt(_));
        let (actions, consumed) = router.on_event(&node_line.node, &node_line.event, 100);
        for action in actions {
            if let RouterAction::ToSession { session, text } = action {
                assert_eq!(session, SESSION, "reply must go to the asker");
                to_session.push(text);
            }
        }
        if is_prompt {
            assert!(consumed, "the prompt terminates the window");
            closed = true;
        }
    }

    assert!(
        to_session.iter().any(|l| l.contains("SFI=140")),
        "the WWV line should have reached the session; got {to_session:?}"
    );
    assert!(
        !router.is_busy("DB0SUE"),
        "the prompt must free the slot for the next command"
    );
}

/// The prompt is surfaced at all. It used to be swallowed inside the client
/// (it paced the init script and went no further), which left the router
/// with no completion marker — the whole design turns on this event
/// existing, so it gets its own assertion.
#[tokio::test]
async fn node_prompts_are_published_not_swallowed() {
    let node = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let node_port = node.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut s, _) = node.accept().await.unwrap();
        s.write_all(b"login: ").await.unwrap();
        let mut buf = [0u8; 512];
        let _ = s.read(&mut buf).await.unwrap();
        s.write_all(b"Hello VU2CPL, welcome\r\n").await.unwrap();
        s.write_all(b"DB0SUE de VU2CPL >\r\n").await.unwrap();
        let _ = s.read(&mut buf).await;
    });

    let cfg = Config {
        telnet_port: 0,
        udp_sources: Vec::new(),
        ..Config::default()
    };
    let (_state, input_tx) = pipeline::start(&cfg, None).await.expect("pipeline");
    let nodes = Arc::new(NodeManager::new());
    let mut lines = nodes.subscribe_lines();
    nodes.start_node("DB0SUE".into(), test_client_cfg(node_port), input_tx);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "no prompt event was ever published"
        );
        let Ok(Ok(node_line)) =
            tokio::time::timeout(std::time::Duration::from_millis(500), lines.recv()).await
        else {
            continue;
        };
        if let ClientEvent::Prompt(text) = node_line.event {
            assert_eq!(node_line.node, "DB0SUE");
            assert!(text.contains('>'), "prompt line carried through: {text}");
            return;
        }
    }
}

/// `send_line` to a node that was never configured is a clean `false`, not
/// a panic and not a silent success the caller would misread as delivery.
#[tokio::test]
async fn unknown_node_is_refused() {
    let nodes = NodeManager::new();
    assert!(!nodes.send_line("NOSUCH", "sh/dx"));
}

// --- milestone 2: the login gate against the real accounts table --------

/// The stub authenticator in `telnet.rs` proves the protocol; this proves
/// the wiring — that a real account in SQLite, with a real argon2 hash,
/// logs in over a real socket, and that a wrong password does not.
#[tokio::test]
async fn telnet_login_uses_the_real_accounts_table() {
    use dxca_connect::telnet::{ClusterServer, InteractiveConfig};
    use dxca_server::auth::{DbAuthenticator, hash_password};
    use dxca_server::db::Db;
    use tokio::net::TcpStream;

    let dir = std::env::temp_dir().join(format!("dxca-telnet-login-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = Arc::new(Db::open(&dir.join("dxca.db")).unwrap());
    db.create_user(
        "VU2CPL",
        "Manoj",
        &hash_password("shack-secret").unwrap(),
        "admin",
    )
    .unwrap();

    let server = ClusterServer::start_with(
        0,
        Some(InteractiveConfig {
            auth: Arc::new(DbAuthenticator::new(db.clone())),
            commands: None,
        }),
    )
    .await
    .unwrap();
    let port = server.local_port();

    async fn read_until(stream: &mut TcpStream, needle: &str) -> String {
        let mut got = String::new();
        let mut buf = [0u8; 512];
        while !got.contains(needle) {
            let n = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buf))
                .await
                .unwrap_or_else(|_| panic!("timed out waiting for {needle:?}; got {got:?}"))
                .expect("read");
            assert!(n > 0, "server closed early; got {got:?}");
            got.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        got
    }

    // Wrong password first — the account exists, the secret does not match.
    let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    read_until(&mut c, "DXCA").await;
    c.write_all(b"LOGIN VU2CPL\r\n").await.unwrap();
    read_until(&mut c, "Password").await;
    c.write_all(b"guessing\r\n").await.unwrap();
    assert!(read_until(&mut c, "failed").await.contains("Login failed."));

    // The real one, on a fresh connection.
    let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    read_until(&mut c, "DXCA").await;
    c.write_all(b"LOGIN vu2cpl\r\n").await.unwrap();
    read_until(&mut c, "Password").await;
    c.write_all(b"shack-secret\r\n").await.unwrap();
    let welcome = read_until(&mut c, "Welcome").await;
    assert!(welcome.contains("VU2CPL"), "got {welcome:?}");
    assert!(
        welcome.contains("admin"),
        "role carried through: {welcome:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// --- phase-rotation mask milestone 2 -----------------------------------

/// The mask is **opt-in**: an account with no locator gets no `band_open`
/// annotation at all, so nothing anywhere can decide to hide its spots.
/// This is the property Manoj asked for explicitly — default is no
/// filtering, nothing imposed — so it is asserted rather than assumed.
#[tokio::test]
async fn no_locator_means_no_band_annotation() {
    use dxca_connect::clublog::Endpoints;
    use dxca_connect::telegram::Telegram;
    use dxca_server::db::{Db, StationConfig};
    use dxca_server::users::UserService;

    let dir = std::env::temp_dir().join(format!("dxca-mask-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = Arc::new(Db::open(&dir.join("dxca.db")).unwrap());
    let uid = db.create_user("VU2CPL", "Manoj", "hash", "admin").unwrap();
    let users = UserService::new(
        db.clone(),
        dir.to_str().unwrap(),
        Telegram::default(),
        Endpoints::default(),
    );

    assert_eq!(
        users.sun_phase(uid),
        None,
        "no locator set: the mask must be unavailable"
    );

    // A locator that cannot be parsed is the same as none — it disables the
    // mask rather than guessing a position.
    db.set_station_config(
        uid,
        &StationConfig {
            locator: "NONSENSE".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(users.sun_phase(uid), None, "unparseable locator");

    // A real one switches it on.
    db.set_station_config(
        uid,
        &StationConfig {
            locator: "MK83TE".into(),
            ..Default::default()
        },
    )
    .unwrap();
    users.sun_phase(uid).expect("locator set");
    // The window must survive a round trip with its default intact. A
    // missing value deserialising as 0 would abolish the grey line rather
    // than default it, which is why StationConfig hand-writes Default.
    assert_eq!(
        db.station_config(uid).unwrap().greyline_window_min,
        45,
        "the default greyline window is 45 minutes"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The band model, applied through the same call the API makes: at
/// Bengaluru's local midday 160m is implausible and 15m is fine, and at
/// local midnight the reverse. This is the feature in one assertion.
#[tokio::test]
async fn the_mask_follows_the_sun_at_a_real_qth() {
    use dxca_core::{bands, grid, solar};

    let pos = grid::parse("MK83TE").expect("MK83TE");
    // 2026-06-21; MK83 is near 77.6E, so local noon is about 0700 UTC and
    // local midnight about 1900 UTC.
    let midnight_utc = 1_782_000_000;
    let local_noon = midnight_utc + 7 * 3_600;
    let local_midnight = midnight_utc + 19 * 3_600;

    let day = solar::phase(pos, local_noon, 45);
    let night = solar::phase(pos, local_midnight, 45);
    assert_eq!(day, solar::SunPhase::Day, "local noon");
    assert_eq!(night, solar::SunPhase::Night, "local midnight");

    assert!(!bands::plausible_in("160M", day), "160m at local midday");
    assert!(bands::plausible_in("15M", day), "15m at local midday");
    assert!(bands::plausible_in("160M", night), "160m at local midnight");
    assert!(!bands::plausible_in("15M", night), "15m at local midnight");
    // 30M is the band that does not care.
    assert!(bands::plausible_in("30M", day) && bands::plausible_in("30M", night));
}

/// The grey line, which is what the phase model bought over elevation
/// windows: a window either side of sunset where 160m AND 15m are both
/// plausible at once. No single elevation threshold can produce that.
#[tokio::test]
async fn the_greyline_opens_the_low_and_high_bands_together() {
    use dxca_core::{bands, grid, solar};

    let pos = grid::parse("MK83TE").expect("MK83TE");
    let sunset = solar::sun_times(pos, 1_782_000_000)
        .sunset_unix
        .expect("Bengaluru is not polar");

    // Half an hour before sunset, inside the default 45-minute window.
    let dusk = solar::phase(pos, sunset - 30 * 60, 45);
    assert_eq!(dusk, solar::SunPhase::Dusk);
    assert!(bands::plausible_in("160M", dusk), "160m on the grey line");
    assert!(bands::plausible_in("15M", dusk), "15m on the grey line");

    // Narrow the window and the same instant is ordinary daylight again:
    // 160m closes, 15m stays. The setting is what moves the boundary.
    let narrow = solar::phase(pos, sunset - 30 * 60, 10);
    assert_eq!(narrow, solar::SunPhase::Day);
    assert!(!bands::plausible_in("160M", narrow));
    assert!(bands::plausible_in("15M", narrow));
}

/// Fail open, on the path that matters most. A Telegram alert suppressed in
/// error is a spot the operator never learns about, so "no opinion" must
/// never suppress: no locator, or a band the model does not model, sends.
#[test]
fn telegram_band_mask_fails_open() {
    use dxca_server::db::NotifyUserConfig;

    let mut cfg = NotifyUserConfig::default();
    assert!(
        !cfg.notify_respect_band_mask,
        "the Telegram band mask must default off"
    );

    cfg.notify_respect_band_mask = true;
    assert!(cfg.passes_band_mask(None), "no opinion must never suppress");
    assert!(cfg.passes_band_mask(Some(true)), "an open band sends");
    assert!(!cfg.passes_band_mask(Some(false)), "a closed band is held");

    cfg.notify_respect_band_mask = false;
    assert!(
        cfg.passes_band_mask(Some(false)),
        "switched off, a closed band still sends"
    );
}
