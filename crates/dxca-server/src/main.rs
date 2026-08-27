//! DXCA server — composition root.
//!
//! M4: config → UDP source listeners + DX-cluster node clients → spot
//! pipeline → telnet server + UDP broadcast; SQLite-backed users with
//! session auth; per-user ClubLog matrices classified over the shared
//! stream; Telegram alert fan-out. The embedded web UI is still the stub
//! shell until M5 (docs/PLAN.md).

use dxca_connect::clublog::Endpoints;
use dxca_connect::telegram::Telegram;
use dxca_server::api::{self, AppState};
use dxca_server::db::Db;
use dxca_server::nodes::NodeManager;
use dxca_server::users::UserService;
use dxca_server::{config, pipeline};
use std::path::Path;
use std::sync::Arc;

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
    let manager = NodeManager::new();
    manager.apply(&cfg.cluster_nodes, &input_tx);

    // Users + alerts (M4).
    let db = match Db::open(&Path::new(&cfg.data_dir).join("dxca.db")) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            eprintln!("dxca: {e}");
            std::process::exit(1);
        }
    };
    let telegram = match &cfg.telegram_base_override {
        Some(base) => Telegram::with_base(base),
        None => Telegram::default(),
    };
    let endpoints = match &cfg.clublog_base_override {
        Some(base) => Endpoints::single_base(base),
        None => Endpoints::default(),
    };
    let users = Arc::new(UserService::new(db, &cfg.data_dir, telegram, endpoints));

    // Alert fan-out: every processed spot classifies per user.
    let mut spot_rx = pipeline_state.spot_events.subscribe();
    let fan_out_users = users.clone();
    tokio::spawn(async move {
        loop {
            match spot_rx.recv().await {
                Ok(spot) => fan_out_users.fan_out(&spot),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });

    // The ClubLog API key used to live in each user's ClubLog config even
    // though it only ever fetched the SHARED cty.xml. Lift a pre-existing one
    // to the server setting, once, so an upgrade needs no manual step.
    match users.db.adopt_legacy_api_key() {
        Ok(Some(from)) => println!("dxca: adopted ClubLog API key from {from}'s settings"),
        Ok(None) => {}
        Err(e) => eprintln!("dxca: could not check for a legacy ClubLog API key: {e}"),
    }

    // Automatic cty / ClubLog / LoTW re-download (PLAN §5's "refresh schedule").
    dxca_server::refresh::spawn(users.clone(), cfg.cty_refresh_days, cfg.lotw_refresh_days);

    let app_state = AppState {
        pipeline: pipeline_state,
        nodes: Arc::new(manager),
        users,
        config: Arc::new(std::sync::Mutex::new(cfg.clone())),
        config_path: Path::new(config::DEFAULT_PATH).to_path_buf(),
        input_tx: input_tx.clone(),
    };
    // MQTT destinations live in the database (they carry a broker password),
    // so they connect here rather than from the TOML config.
    match api::load_mqtt(&app_state) {
        Ok(0) => {}
        Ok(n) => println!("dxca: MQTT destinations connected ({n})"),
        Err(e) => eprintln!("dxca: MQTT load failed, nothing is published: {e}"),
    }

    // The pipeline drops blacklisted calls from its own live set, so the
    // stored list has to reach it before the first spot does.
    match api::load_blacklist(&app_state) {
        Ok(0) => {}
        Ok(n) => println!("dxca: blacklist loaded ({n} calls)"),
        Err(e) => eprintln!("dxca: blacklist load failed, nothing is blocked: {e}"),
    }

    let app = api::build_router(app_state.clone());

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
        "dxca {} — web http://{}/ · telnet {} · sources [{}] · nodes [{}] · users {} · cty {}",
        env!("CARGO_PKG_VERSION"),
        cfg.web_bind,
        cfg.telnet_port,
        sources.join(", "),
        node_names.join(", "),
        app_state.users.db.user_count().unwrap_or(0),
        if app_state.users.resolver_loaded() {
            format!("{} entities", app_state.users.entity_count())
        } else {
            "not loaded".to_string()
        }
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
