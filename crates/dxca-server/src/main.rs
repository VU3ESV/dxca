//! DXCA server — composition root.
//!
//! M0: loads config, serves the embedded web UI and a status endpoint,
//! shuts down cleanly on SIGINT/SIGTERM. The spot pipeline, telnet server,
//! and auth arrive in M2–M4 (docs/PLAN.md).

mod assets;
mod config;

use axum::{Json, Router, routing::get};
use std::path::Path;

async fn status() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "name": "dxca",
        "version": env!("CARGO_PKG_VERSION"),
        "milestone": "M0 scaffold",
    }))
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

    let app = Router::new()
        .route("/api/status", get(status))
        .fallback(assets::serve);

    let listener = match tokio::net::TcpListener::bind(&cfg.web_bind).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("dxca: cannot bind web {}: {e}", cfg.web_bind);
            std::process::exit(1);
        }
    };
    println!(
        "dxca {} — web http://{}/ (telnet {} and UDP {} arrive in M2)",
        env!("CARGO_PKG_VERSION"),
        cfg.web_bind,
        cfg.telnet_port,
        cfg.udp_listen_port
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
