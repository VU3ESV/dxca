//! Milestone 3 end-to-end: a real telnet session, logged in, issuing a
//! cluster command that reaches a real (fake) DXSpider node, with the reply
//! coming back to that session and **nowhere else**.
//!
//! The assertion that matters most is the negative one. A `SHOW/DX` reply is
//! made of `DX de …` lines that parse as perfectly good spots but are
//! historical — often hours old. If they leak into the pipeline they are
//! re-announced to every logger on the feed and fire Telegram alerts for
//! QSOs from last week. `sh_dx_history_reaches_the_asker_and_nothing_else`
//! is the test that would catch that regression.

use dxca_connect::dxcluster::ClientConfig;
use dxca_connect::telnet::{Authenticator, InteractiveConfig, TelnetIdentity};
use dxca_server::config::Config;
use dxca_server::nodes::NodeManager;
use dxca_server::pipeline;
use dxca_server::telnetcmd::TelnetCommands;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

struct StubAuth;
impl Authenticator for StubAuth {
    fn authenticate(&self, callsign: &str, password: &str) -> Option<TelnetIdentity> {
        (callsign == "VU2CPL" && password == "secret").then(|| TelnetIdentity {
            user_id: 1,
            callsign: "VU2CPL".into(),
            role: "admin".into(),
        })
    }
}

fn test_client_cfg(port: u16) -> ClientConfig {
    let mut cfg = ClientConfig::new("127.0.0.1", port, "VU2CPL");
    cfg.reconnect_schedule_s = vec![1, 1];
    cfg.auth_timeout_s = 1;
    cfg.silence_timeout_s = 600;
    cfg.init_commands = Vec::new();
    cfg
}

async fn read_until(stream: &mut TcpStream, needle: &str) -> String {
    let mut got = String::new();
    let mut buf = [0u8; 1024];
    while !got.contains(needle) {
        let n = tokio::time::timeout(std::time::Duration::from_secs(10), stream.read(&mut buf))
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {needle:?}; got:\n{got}"))
            .expect("read");
        assert!(n > 0, "server closed early; got {got:?}");
        got.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    got
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

/// Everything at once, because the point is the interaction: the command
/// leaves, the history comes back to one session, the pipeline stays clean,
/// and the other client on the feed sees none of it.
#[tokio::test]
async fn sh_dx_history_reaches_the_asker_and_nothing_else() {
    // --- a fake DXSpider node that answers SHOW/DX with old spots --------
    let node = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let node_port = node.local_addr().unwrap().port();
    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel::<String>();
    tokio::spawn(async move {
        let (mut s, _) = node.accept().await.unwrap();
        s.write_all(b"login: ").await.unwrap();
        let mut buf = [0u8; 1024];
        // Drain the login callsign; this double never inspects it, so the
        // byte count is deliberately discarded.
        let _ = s.read(&mut buf).await.unwrap();
        s.write_all(b"Hello VU2CPL, welcome to the test node\r\n")
            .await
            .unwrap();
        // One genuinely live spot first, to prove the ordinary path still
        // works while the passthrough exists.
        s.write_all(
            b"DX de W3LPL:     14074.0  K1JT           FT8 -10 dB                  1428Z\r\n",
        )
        .await
        .unwrap();

        let n = s.read(&mut buf).await.unwrap();
        let got = String::from_utf8_lossy(&buf[..n]).trim().to_string();
        // The history burst: same wire shape, but these are old.
        s.write_all(
            b"DX de OLD1:      21074.0  ZS6ABC         FT8 old spot                 0801Z\r\n",
        )
        .await
        .unwrap();
        s.write_all(
            b"DX de OLD2:      28074.0  VK3XYZ         FT8 older spot               0755Z\r\n",
        )
        .await
        .unwrap();
        s.write_all(b"DB0SUE de VU2CPL >\r\n").await.unwrap();
        let _ = cmd_tx.send(got);
        let _ = s.read(&mut buf).await; // hold open
    });

    // --- DXCA, with the passthrough enabled ------------------------------
    let cfg = Config {
        telnet_port: 0,
        udp_sources: Vec::new(),
        ..Config::default()
    };
    let manager = Arc::new(NodeManager::new());
    let commands = TelnetCommands::start(manager.clone());
    manager.set_event_filter(commands.clone());
    let (state, input_tx) = pipeline::start(
        &cfg,
        Some(InteractiveConfig {
            auth: Arc::new(StubAuth),
            commands: Some(commands),
        }),
    )
    .await
    .expect("pipeline");
    manager.start_node("DB0SUE".into(), test_client_cfg(node_port), input_tx);
    let telnet_port = state.telnet.local_port();

    wait_until("node live", 15, || {
        manager.statuses().get("DB0SUE").is_some_and(|s| s.proven)
    })
    .await;
    // The genuinely live spot went through the normal path.
    wait_until("live spot in the ring", 10, || {
        !state.recent_spots(10).is_empty()
    })
    .await;

    // --- an anonymous client, watching the shared feed -------------------
    let mut bystander = TcpStream::connect(("127.0.0.1", telnet_port))
        .await
        .unwrap();
    read_until(&mut bystander, "DXCA").await;

    // --- the operator logs in and asks for history -----------------------
    let mut op = TcpStream::connect(("127.0.0.1", telnet_port))
        .await
        .unwrap();
    read_until(&mut op, "DXCA").await;
    op.write_all(b"LOGIN VU2CPL\r\n").await.unwrap();
    read_until(&mut op, "Password").await;
    op.write_all(b"secret\r\n").await.unwrap();
    read_until(&mut op, "Welcome").await;

    op.write_all(b"sh/dx 5\r\n").await.unwrap();

    // The node received the CANONICAL form, not the abbreviation: what the
    // allowlist judged and what the node runs must be the same string.
    let seen = tokio::time::timeout(std::time::Duration::from_secs(10), cmd_rx)
        .await
        .expect("node never got the command")
        .expect("sender dropped");
    assert_eq!(seen, "SHOW/DX 5", "node runs the canonicalized command");

    // The operator gets the history.
    let got = read_until(&mut op, "VK3XYZ").await;
    assert!(got.contains("ZS6ABC"), "both history lines: {got}");

    // --- and now the assertions that matter ------------------------------
    // Give any leak time to arrive before declaring it absent.
    tokio::time::sleep(std::time::Duration::from_millis(750)).await;

    let ring = state.recent_spots(100);
    let calls: Vec<String> = ring.iter().filter_map(|s| s.dx_callsign()).collect();
    assert!(
        calls.iter().any(|c| c == "K1JT"),
        "the genuinely live spot must still be there: {calls:?}"
    );
    assert!(
        !calls.iter().any(|c| c == "ZS6ABC" || c == "VK3XYZ"),
        "SH/DX history LEAKED into the spot pipeline: {calls:?}"
    );

    // The bystander saw the live spot and none of the history.
    let mut seen_by_bystander = String::new();
    let mut buf = [0u8; 4096];
    while let Ok(Ok(n)) = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        bystander.read(&mut buf),
    )
    .await
    {
        if n == 0 {
            break;
        }
        seen_by_bystander.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    assert!(
        !seen_by_bystander.contains("ZS6ABC") && !seen_by_bystander.contains("VK3XYZ"),
        "another client saw one operator's history query: {seen_by_bystander}"
    );

    // The node's spot counter must not have been inflated by the history
    // either — a query should not look like traffic.
    let count = manager.statuses()["DB0SUE"].spot_count;
    assert_eq!(count, 1, "only the live spot counts as a spot");
}

/// A dangerous command is refused before it can reach the node, whatever
/// spelling is used. The fake node here answers nothing: if anything were
/// forwarded the test would hang on the assertion instead of passing.
#[tokio::test]
async fn dangerous_commands_never_reach_the_node() {
    let node = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let node_port = node.local_addr().unwrap().port();
    let forwarded = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let forwarded_srv = forwarded.clone();
    tokio::spawn(async move {
        let (mut s, _) = node.accept().await.unwrap();
        s.write_all(b"login: ").await.unwrap();
        let mut buf = [0u8; 1024];
        // Drain the login callsign; the count is deliberately discarded.
        let _ = s.read(&mut buf).await.unwrap();
        s.write_all(b"Hello VU2CPL, welcome\r\n").await.unwrap();
        // Anything further is a command DXCA should never have sent.
        while let Ok(n) = s.read(&mut buf).await {
            if n == 0 {
                break;
            }
            forwarded_srv.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });

    let cfg = Config {
        telnet_port: 0,
        udp_sources: Vec::new(),
        ..Config::default()
    };
    let manager = Arc::new(NodeManager::new());
    let commands = TelnetCommands::start(manager.clone());
    manager.set_event_filter(commands.clone());
    let (state, input_tx) = pipeline::start(
        &cfg,
        Some(InteractiveConfig {
            auth: Arc::new(StubAuth),
            commands: Some(commands),
        }),
    )
    .await
    .expect("pipeline");
    manager.start_node("DB0SUE".into(), test_client_cfg(node_port), input_tx);
    let telnet_port = state.telnet.local_port();
    wait_until("node live", 15, || {
        manager.statuses().get("DB0SUE").is_some_and(|s| s.proven)
    })
    .await;

    let mut op = TcpStream::connect(("127.0.0.1", telnet_port))
        .await
        .unwrap();
    read_until(&mut op, "DXCA").await;
    op.write_all(b"LOGIN VU2CPL\r\n").await.unwrap();
    read_until(&mut op, "Password").await;
    op.write_all(b"secret\r\n").await.unwrap();
    read_until(&mut op, "Welcome").await;

    // Abbreviations of the commands that would do real damage.
    for (line, expect) in [
        ("s/pass hunter2", "refused"),
        ("set/homenode W1AW", "refused"),
        ("uns/dx", "refused"),
        ("dx 14074.0 K1JT hi", "refused"),
        ("acc/spots dx", "refused"),
        ("sysop", "refused"),
        ("frobnicate", "not a command"),
    ] {
        op.write_all(format!("{line}\r\n").as_bytes())
            .await
            .unwrap();
        let reply = read_until(&mut op, expect).await;
        assert!(reply.contains(expect), "for {line:?} got {reply}");
    }

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(
        forwarded.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a refused command was written to the node"
    );
}
