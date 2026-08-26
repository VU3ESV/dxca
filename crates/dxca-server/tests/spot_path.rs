//! M2 end-to-end: feed real captured decoder datagrams (dxca-core's test
//! vectors) into a running pipeline over actual UDP sockets, then assert
//! the two output paths:
//!  - the passthrough destination receives every datagram byte-identical
//!    (the 1094/1094 invariant from the live capture, in miniature);
//!  - a telnet client gets the banner and a DX-Spider line carrying the
//!    callsign extracted from the decode.

use dxca_server::config::{BroadcastDestination, Config, UdpSource};
use dxca_server::pipeline;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpStream, UdpSocket};

fn vector(decoder: &str, name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dxca-core/tests/vectors")
        .join(decoder)
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("vector {}: {e}", p.display()))
}

#[tokio::test]
async fn vectors_flow_to_passthrough_and_telnet() {
    // The passthrough "logger" (stand-in RUMlog) binds first so we know
    // its port for the destination config.
    let logger = UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
    let logger_port = logger.local_addr().unwrap().port();

    let source_port = 48_334; // test-only fixed port for the JTDX source
    let cfg = Config {
        telnet_port: 0, // ephemeral — read back from the state
        udp_sources: vec![UdpSource {
            name: "JTDX".into(),
            port: source_port,
            enabled: true,
        }],
        broadcast_destinations: vec![BroadcastDestination {
            name: "logger".into(),
            ip: Ipv4Addr::LOCALHOST,
            port: logger_port,
            format: "passthrough".into(),
            sources: Vec::new(),
            unfiltered: false,
            enabled: true,
        }],
        ..Config::default()
    };

    let state = pipeline::start(&cfg).await.expect("pipeline start");

    // Telnet client connects and sees the banner.
    let mut telnet = TcpStream::connect(("127.0.0.1", state.telnet.local_port()))
        .await
        .unwrap();
    let banner = read_some(&mut telnet, "DXCA").await;
    assert!(banner.contains("DX Cluster Server"));

    // Feed a real Status (sets the dial) then a real Decode.
    let status = vector("jtdx", "type01-1.bin");
    let decode = vector("jtdx", "type02-1.bin");
    let sender = UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
    sender
        .send_to(&status, ("127.0.0.1", source_port))
        .await
        .unwrap();
    sender
        .send_to(&decode, ("127.0.0.1", source_port))
        .await
        .unwrap();

    // Passthrough must deliver both datagrams byte-identical, in order.
    for expected in [&status, &decode] {
        let mut buf = vec![0u8; 65_536];
        let (n, _) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            logger.recv_from(&mut buf),
        )
        .await
        .expect("passthrough datagram timed out")
        .unwrap();
        assert_eq!(
            &buf[..n],
            expected.as_slice(),
            "passthrough not byte-identical"
        );
    }

    // The telnet client gets a DX line for the decode, with the callsign
    // the extractor pulled from the real message text.
    let line = read_some(&mut telnet, "Z").await;
    assert!(line.starts_with("DX de JTDX:"), "line: {line:?}");
    let spot = &state.recent_spots(1)[0];
    let call = spot.dx_callsign().expect("captured decode has a callsign");
    assert!(line.contains(&call), "line {line:?} missing call {call}");
    assert!(line.trim_end().ends_with('Z'));

    // And the ring recorded the spot with the Status-supplied dial.
    assert!(
        spot.dial_frequency_hz > 0,
        "dial should come from the Status"
    );
    assert_eq!(spot.source_name, "JTDX");
}

async fn read_some(stream: &mut TcpStream, needle: &str) -> String {
    let mut got = String::new();
    let mut buf = [0u8; 1024];
    while !got.contains(needle) {
        let n = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buf))
            .await
            .expect("telnet read timed out")
            .expect("telnet read");
        assert!(n > 0, "server closed; got {got:?}");
        got.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    got
}
