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
use dxca_server::nodes::NodeManager;
use dxca_server::pipeline;
use dxca_server::config::Config;
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
    let (_state, input_tx) = pipeline::start(&cfg).await.expect("pipeline");
    let nodes = Arc::new(NodeManager::new());
    let mut lines = nodes.subscribe_lines();
    nodes.start_node("DB0SUE".into(), test_client_cfg(node_port), input_tx);

    wait_until("node proven live", 15, || {
        nodes
            .statuses()
            .get("DB0SUE")
            .is_some_and(|s| s.proven)
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
        assert!(std::time::Instant::now() < deadline, "reply never completed");
        let Ok(Ok(node_line)) = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            lines.recv(),
        )
        .await
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
    let (_state, input_tx) = pipeline::start(&cfg).await.expect("pipeline");
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
