//! DXCA server — composition root.
//!
//! M3: config → UDP source listeners + DX-cluster node clients → spot
//! pipeline (dedupe, ring) → telnet cluster server + UDP broadcast (incl.
//! passthrough), plus the embedded web UI, `/api/status`, and
//! `/api/spots`. Auth and per-user state arrive in M4 (docs/PLAN.md).

use axum::extract::{Query, State};
use axum::{Json, Router, routing::get};
use dxca_connect::dxcluster::ClientConfig;
use dxca_server::nodes::NodeManager;
use dxca_server::pipeline::PipelineState;
use dxca_server::{assets, config, pipeline};
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
struct AppState {
    pipeline: Arc<PipelineState>,
    nodes: Arc<NodeManager>,
}

async fn status(State(app): State<AppState>) -> Json<serde_json::Value> {
    let counters = app.pipeline.broadcaster.counters();
    Json(serde_json::json!({
        "name": "dxca",
        "version": env!("CARGO_PKG_VERSION"),
        "milestone": "M3 cluster ingest",
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

async fn spots(
    State(app): State<AppState>,
    Query(q): Query<SpotsQuery>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "spots": app.pipeline.recent_spots(q.limit.min(2000)) }))
}

#[tokio::main]
async fn main() {
    let cfg = match config::Config::load(Path::new(config::DEFAULT_PATH)) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("dxca: config error: {e}");
            std::process::exit(1);
        }
    };

    let (pipeline_state, input_tx) = match pipeline::start(&cfg).await {
        Ok(started) => started,
        Err(e) => {
            eprintln!("dxca: pipeline start failed (port clash?): {e}");
            std::process::exit(1);
        }
    };

    // DX-cluster node clients (M3): honest-status supervised connections.
    let mut manager = NodeManager::new();
    for node in cfg.cluster_nodes.iter().filter(|n| n.enabled) {
        let mut client_cfg = ClientConfig::new(&node.host, node.port, &node.login_call);
        client_cfg.password = node.password.clone();
        manager.start_node(node.name.clone(), client_cfg, input_tx.clone());
    }
    let app_state = AppState {
        pipeline: pipeline_state,
        nodes: Arc::new(manager),
    };

    let app = Router::new()
        .route("/api/status", get(status))
        .route("/api/spots", get(spots))
        .with_state(app_state)
        .fallback(assets::serve);

    let listener = match tokio::net::TcpListener::bind(&cfg.web_bind).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("dxca: cannot bind web {}: {e}", cfg.web_bind);
            std::process::exit(1);
        }
    };
    let sources: Vec<String> = cfg
        .udp_sources
        .iter()
        .filter(|s| s.enabled)
        .map(|s| format!("{} {}", s.name, s.port))
        .collect();
    let node_names: Vec<String> = cfg
        .cluster_nodes
        .iter()
        .filter(|n| n.enabled)
        .map(|n| n.name.clone())
        .collect();
    println!(
        "dxca {} — web http://{}/ · telnet {} · sources [{}] · nodes [{}]",
        env!("CARGO_PKG_VERSION"),
        cfg.web_bind,
        cfg.telnet_port,
        sources.join(", "),
        node_names.join(", ")
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
    println!("dxca: shut down cleanly");
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    ctrl_c.await.expect("install ctrl-c handler");
}
