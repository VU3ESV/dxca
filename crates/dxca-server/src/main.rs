//! DXCA server — composition root.
//!
//! M2: config → UDP source listeners → spot pipeline (dedupe, ring) →
//! telnet cluster server + UDP broadcast (incl. passthrough), plus the
//! embedded web UI, `/api/status`, and `/api/spots`. Auth and per-user
//! state arrive in M4 (docs/PLAN.md).

use axum::extract::{Query, State};
use axum::{Json, Router, routing::get};
use dxca_server::pipeline::PipelineState;
use dxca_server::{assets, config, pipeline};
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;

async fn status(State(state): State<Arc<PipelineState>>) -> Json<serde_json::Value> {
    let counters = state.broadcaster.counters();
    Json(serde_json::json!({
        "name": "dxca",
        "version": env!("CARGO_PKG_VERSION"),
        "milestone": "M2 spot path",
        "telnet_clients": state.telnet.client_count(),
        "spots_per_source": *state.source_counts.lock().unwrap(),
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
    State(state): State<Arc<PipelineState>>,
    Query(q): Query<SpotsQuery>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "spots": state.recent_spots(q.limit.min(2000)) }))
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

    let state = match pipeline::start(&cfg).await {
        Ok(state) => state,
        Err(e) => {
            eprintln!("dxca: pipeline start failed (port clash?): {e}");
            std::process::exit(1);
        }
    };

    let app = Router::new()
        .route("/api/status", get(status))
        .route("/api/spots", get(spots))
        .with_state(state)
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
    println!(
        "dxca {} — web http://{}/ · telnet {} · sources [{}]",
        env!("CARGO_PKG_VERSION"),
        cfg.web_bind,
        cfg.telnet_port,
        sources.join(", ")
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
