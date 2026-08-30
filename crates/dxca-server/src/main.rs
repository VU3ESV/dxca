//! DXCA server — composition root.
//!
//! M4: config → UDP source listeners + DX-cluster node clients → spot
//! pipeline → telnet server + UDP broadcast; SQLite-backed users with
//! session auth; per-user ClubLog matrices classified over the shared
//! stream; Telegram alert fan-out. The embedded web UI is still the stub
//! shell until M5 (docs/PLAN.md).

use dxca_connect::clublog::Endpoints;
use dxca_connect::telegram::Telegram;
use dxca_connect::telnet::InteractiveConfig;
use dxca_server::api::{self, AppState};
use dxca_server::db::Db;
use dxca_server::nodes::NodeManager;
use dxca_server::telnetcmd::TelnetCommands;
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

    // Opened before the pipeline: the telnet server's optional login gate
    // authenticates against these accounts, so the database has to exist
    // before the listener binds.
    let db = match Db::open(&Path::new(&cfg.data_dir).join("dxca.db")) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            eprintln!("dxca: {e}");
            std::process::exit(1);
        }
    };

    // DX-cluster node clients: honest-status supervised connections. Built
    // before the pipeline because the telnet server's command passthrough
    // writes to these nodes, and the listener binds inside pipeline::start.
    let manager = Arc::new(NodeManager::new());

    let interactive = cfg.telnet_interactive.then(|| {
        let commands = TelnetCommands::start(manager.clone());
        // Closes the cycle: the sink writes to nodes, and the nodes hand
        // every event to the sink first. Must happen before any node
        // starts, or a `SHOW/DX` reply could reach the spot pipeline.
        manager.set_event_filter(commands.clone());
        InteractiveConfig {
            auth: Arc::new(dxca_server::auth::DbAuthenticator::new(db.clone())),
            commands: Some(commands),
        }
    });
    let (pipeline_state, input_tx) = match pipeline::start(&cfg, interactive).await {
        Ok(started) => started,
        Err(e) => {
            eprintln!("dxca: pipeline start failed (port clash?): {e}");
            std::process::exit(1);
        }
    };

    manager.apply(&cfg.cluster_nodes, &input_tx);
    let telegram = match &cfg.telegram_base_override {
        Some(base) => Telegram::with_base(base),
        None => Telegram::default(),
    };
    let endpoints = match &cfg.clublog_base_override {
        Some(base) => Endpoints::single_base(base),
        None => Endpoints::default(),
    };
    let users = Arc::new(UserService::new(db, &cfg.data_dir, telegram, endpoints));

    // Step one of docs/MULTI-STATION.md: move a single-operator install's
    // TOML feeds into its account.
    //
    // BEHAVIOUR-NEUTRAL. Nothing reads `feeds_json` to build a pipeline yet —
    // `config/dxca.toml` is still what runs the server, and the TOML sections
    // are left in place so the previous binary remains a working rollback.
    // This only fills in the column the next step will read.
    migrate_feeds(&users, &cfg);

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
    // Feed-health alerts. Silent unless an account sets a threshold, and
    // powerless if this host is what failed — see the module docs.
    dxca_server::health::spawn(users.clone(), pipeline_state.clone(), manager.clone());

    let app_state = AppState {
        pipeline: pipeline_state,
        nodes: manager.clone(),
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

/// Populate each account's `feeds_json` from the TOML, once.
///
/// Idempotent: it does nothing the moment any account owns feeds, so it is
/// safe on every boot. Refuses on a multi-account install rather than
/// guessing whose cluster logins are whose.
fn migrate_feeds(users: &dxca_server::users::UserService, cfg: &config::Config) {
    use dxca_server::feeds::{Migration, migrate_from_toml};

    let Ok(all) = users.db.users() else { return };
    let accounts: Vec<(i64, String, dxca_server::feeds::FeedsUserConfig)> = all
        .into_iter()
        .filter_map(|u| {
            let feeds = users.db.feeds_config(u.id).ok()?;
            Some((u.id, u.callsign, feeds))
        })
        .collect();

    let (what, store) = migrate_from_toml(
        &accounts,
        &cfg.udp_sources,
        &cfg.cluster_nodes,
        &cfg.broadcast_destinations,
    );
    if let Some((user_id, feeds)) = store
        && let Err(e) = users.db.set_feeds_config(user_id, &feeds)
    {
        eprintln!("dxca: feeds migration: could not store: {e}");
        return;
    }
    match what {
        Migration::Moved {
            callsign,
            sources,
            nodes,
            destinations,
        } => println!(
            "dxca: feeds migration: {sources} source(s), {nodes} node(s),              {destinations} destination(s) now owned by {callsign}              (config/dxca.toml still drives the pipeline)"
        ),
        Migration::Ambiguous { accounts } => eprintln!(
            "dxca: feeds migration: {accounts} accounts and no way to tell whose              the config's sources and nodes are — refusing to guess. An admin              must assign them."
        ),
        // Silent: the overwhelmingly common case on every boot after the first.
        Migration::AlreadyMoved | Migration::NothingToMove => {}
    }
}
