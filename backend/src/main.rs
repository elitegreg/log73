mod adif;
mod auth;
mod bands;
mod cabrillo;
mod cat_keyer;
mod contest_rules;
mod cw;
mod db;
mod dxcc;
mod dxcluster;
mod log_cache;
mod radio;
mod radio_manager;
mod scoring;
mod static_assets;
mod stats;
mod supercheckpartial;
mod validation;
mod voice_keyer;
mod voice_messages;

use axum::{
    Json, Router,
    body::Body,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use clap::Parser;
use contest_rules::{ContestRules, ContestRulesStore};
use db::{
    AuthConfig, Contact, Database, NewLog, NewRadio, UpdateLog, contact_adif_value, contact_id,
    contact_log_id, contact_meta_value, set_contact_meta,
};
use dxcluster::{DxClusterEvent, DxClusterManager, format_dxcluster_frequency_khz};
use futures_util::{SinkExt, StreamExt};
use log_cache::LogCache;
use radio::{ClientMessage, RadioCommand, ServerMessage};
use radio_cat_rs::{list_serial_ports, supported_drivers};
use radio_manager::RadioManager;
use scoring::{IncrementalScoreTracker, ScoreTotals, ScoringModules, score_contacts};
use stats::StatsTracker;
use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    fs,
    fs::OpenOptions,
    hash::Hasher,
    path::{Path as FsPath, PathBuf},
    time::Duration,
};
use supercheckpartial::SuperCheckPartial;
use tokio::sync::{RwLock, broadcast, mpsc, oneshot};
use tower_http::trace::TraceLayer;
use tracing::{Span, debug, error, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use voice_keyer::VoiceKeyer;

#[derive(Clone)]
struct AppState {
    radio_manager: RadioManager,
    log_events: broadcast::Sender<ServerMessage>,
    db: Database,
    auth_config: std::sync::Arc<RwLock<AuthConfig>>,
    contest_rules: ContestRulesStore,
    log_cache: LogCache,
    incremental_scoring: IncrementalScoreTracker,
    stats: StatsTracker,
    supercheckpartial: SuperCheckPartial,
    dxcc: std::sync::Arc<dxcc::DxccDatabase>,
    dxcluster: DxClusterManager,
    voice_keyer: VoiceKeyer,
}

const MAX_CLIENT_ERROR_TEXT_LENGTH: usize = 4096;
const MAX_CLIENT_ERROR_JSON_LENGTH: usize = 8192;

fn disabled_auth_config() -> AuthConfig {
    AuthConfig {
        login_user: String::new(),
        login_password: String::new(),
    }
}

fn auth_config_or_disabled(result: rusqlite::Result<AuthConfig>) -> AuthConfig {
    match result {
        Ok(config) => config,
        Err(error) => {
            warn!(%error, "failed to load auth config; basic auth disabled until config reload");
            disabled_auth_config()
        }
    }
}

fn init_tracing(cli: &Cli) -> std::io::Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    let filter = EnvFilter::try_new(&cli.log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    let stdout_layer = fmt::layer();

    if let Some(path) = &cli.log_file {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let (writer, guard) = tracing_appender::non_blocking(file);
        let file_layer = fmt::layer().with_writer(writer).with_ansi(false);

        tracing_subscriber::registry()
            .with(filter)
            .with(stdout_layer)
            .with(file_layer)
            .init();

        return Ok(Some(guard));
    }

    tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .init();

    Ok(None)
}

fn redacted_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            let value = if name == header::AUTHORIZATION || name == header::COOKIE {
                "<redacted>".to_string()
            } else {
                value.to_str().unwrap_or("<non-utf8>").to_string()
            };
            (name.to_string(), value)
        })
        .collect()
}

fn pretty_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|error| format!("<unable to serialize json: {error}>"))
}

#[derive(Debug, Parser)]
#[command(version, about = "Log73 contest logger backend")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:7300")]
    bind: String,

    #[arg(long, default_value = "info")]
    log_level: String,

    #[arg(long)]
    log_file: Option<PathBuf>,

    #[arg(long)]
    config_dir: Option<PathBuf>,

    #[arg(long)]
    data_dir: Option<PathBuf>,

    #[arg(long)]
    app_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct AppPaths {
    config_dir: PathBuf,
    data_dir: PathBuf,
    app_dir: PathBuf,
    installed_data_dir: PathBuf,
    installed_contest_rules_dir: PathBuf,
    user_contest_rules_dir: PathBuf,
    master_scp_path: PathBuf,
    cty_dat_path: PathBuf,
    database_path: PathBuf,
}

fn resolve_paths(cli: &Cli) -> AppPaths {
    let config_dir = cli
        .config_dir
        .clone()
        .unwrap_or_else(log73_paths::config_dir);
    let data_dir = cli.data_dir.clone().unwrap_or_else(log73_paths::data_dir);
    let app_dir = cli.app_dir.clone().unwrap_or_else(log73_paths::app_root);
    let installed_data_dir = log73_paths::installed_data_dir(&app_dir);
    let installed_contest_rules_dir = log73_paths::contest_rules_dir(&installed_data_dir);
    let user_contest_rules_dir = log73_paths::contest_rules_dir(&data_dir);
    let master_scp_path = data_file_path(&data_dir, &installed_data_dir, "MASTER.SCP");
    let cty_dat_path = data_file_path(&data_dir, &installed_data_dir, "cty.dat");
    let database_path = log73_paths::database_path(&data_dir);

    AppPaths {
        config_dir,
        data_dir,
        app_dir,
        installed_data_dir,
        installed_contest_rules_dir,
        user_contest_rules_dir,
        master_scp_path,
        cty_dat_path,
        database_path,
    }
}

fn data_file_path(user_data_dir: &FsPath, installed_data_dir: &FsPath, file_name: &str) -> PathBuf {
    let user_path = user_data_dir.join(file_name);
    match user_path.try_exists() {
        Ok(true) => user_path,
        Ok(false) => installed_data_dir.join(file_name),
        Err(_) => user_path,
    }
}

fn ensure_startup_dirs(paths: &AppPaths, log_file: Option<&PathBuf>) -> std::io::Result<()> {
    fs::create_dir_all(&paths.config_dir)?;
    fs::create_dir_all(&paths.data_dir)?;
    let voicekeyer_dir = paths.data_dir.join("voicekeyer");
    fs::create_dir_all(&voicekeyer_dir)?;

    if let Some(parent) = log_file
        .and_then(|path| path.parent())
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let paths = resolve_paths(&cli);
    ensure_startup_dirs(&paths, cli.log_file.as_ref())
        .expect("failed to initialize path directories");
    let _log_guard = init_tracing(&cli).expect("failed to initialize logging");

    info!(
        config_dir = %paths.config_dir.display(),
        data_dir = %paths.data_dir.display(),
        app_dir = %paths.app_dir.display(),
        installed_data_dir = %paths.installed_data_dir.display(),
        installed_contest_rules_dir = %paths.installed_contest_rules_dir.display(),
        user_contest_rules_dir = %paths.user_contest_rules_dir.display(),
        master_scp_path = %paths.master_scp_path.display(),
        cty_dat_path = %paths.cty_dat_path.display(),
        database_path = %paths.database_path.display(),
        "using log73 paths"
    );

    let (log_events, _) = broadcast::channel(128);
    let contest_rules = ContestRulesStore::load_dirs([
        paths.installed_contest_rules_dir.as_path(),
        paths.user_contest_rules_dir.as_path(),
    ])
    .unwrap_or_else(|error| panic!("failed to load contest rules: {error}"));
    let supercheckpartial =
        SuperCheckPartial::load_file(&paths.master_scp_path).unwrap_or_else(|error| {
            warn!(
                path = %paths.master_scp_path.display(),
                %error,
                "failed to load MASTER.SCP; supercheckpartial matches will be unavailable"
            );
            SuperCheckPartial::default()
        });
    info!(
        callsigns = supercheckpartial.len(),
        path = %paths.master_scp_path.display(),
        "loaded supercheckpartial callsigns"
    );
    let dxcc = dxcc::DxccDatabase::load_file(&paths.cty_dat_path).unwrap_or_else(|error| {
        warn!(
            path = %paths.cty_dat_path.display(),
            %error,
            "failed to load cty.dat; DXCC lookup will be unavailable"
        );
        dxcc::DxccDatabase::default()
    });
    info!(
        entities = dxcc.entity_count(),
        rules = dxcc.rule_count(),
        path = %paths.cty_dat_path.display(),
        "loaded DXCC country data"
    );
    let db = Database::open(&paths.database_path).expect("failed to open log73 database");
    let auth_config =
        std::sync::Arc::new(RwLock::new(auth_config_or_disabled(db.auth_config().await)));
    let dxcluster = DxClusterManager::new();
    let voicekeyer_dir = paths.data_dir.join("voicekeyer");
    let voice_keyer = VoiceKeyer::with_voicekeyer_dir(voicekeyer_dir);
    match db.dxcluster_config().await {
        Ok(config) => dxcluster.apply_config(config).await,
        Err(error) => warn!(%error, "failed to load dxcluster config; listener task not started"),
    }
    let radio_manager = RadioManager::new(db.clone(), voice_keyer.clone());
    let scoring_modules = ScoringModules::new();
    let incremental_scoring = IncrementalScoreTracker::new();
    let stats = StatsTracker::new();
    let log_cache = LogCache::new(db.clone(), contest_rules.clone(), scoring_modules.clone());
    log_cache.register_processor(std::sync::Arc::new(incremental_scoring.clone()));
    log_cache.register_processor(std::sync::Arc::new(stats.clone()));

    let app_state = AppState {
        radio_manager,
        log_events,
        db,
        auth_config,
        contest_rules,
        log_cache,
        incremental_scoring,
        stats,
        supercheckpartial,
        dxcc: std::sync::Arc::new(dxcc),
        dxcluster,
        voice_keyer,
    };

    let request_trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &Request<axum::body::Body>| {
            tracing::debug_span!(
                "http_request",
                method = %request.method(),
                uri = %request.uri(),
                version = ?request.version()
            )
        })
        .on_request(|request: &Request<axum::body::Body>, _span: &Span| {
            debug!(
                method = %request.method(),
                uri = %request.uri(),
                version = ?request.version(),
                headers = ?redacted_headers(request.headers()),
                "incoming request"
            );
        })
        .on_response(
            |response: &axum::response::Response, latency: Duration, _span: &Span| {
                info!(
                    status = response.status().as_u16(),
                    latency_ms = latency.as_secs_f64() * 1000.0,
                    "request completed"
                );
            },
        );

    let api = Router::new()
        .route("/contest-rules", get(list_contest_rules))
        .route("/contest-settings", get(contest_settings))
        .route("/config", get(config).put(update_config))
        .route("/client-errors", post(report_client_error))
        .route("/supercheckpartial", get(supercheckpartial_matches))
        .route("/dxcc", get(dxcc_data))
        .route(
            "/dxcluster/spots",
            get(dxcluster_spots)
                .post(save_dxcluster_spot)
                .delete(clear_dxcluster_spots),
        )
        .route("/logs", get(logs).post(create_log))
        .route("/logs/{id}", get(log).put(update_log).delete(delete_log))
        .route("/logs/{id}/qso-count", get(log_qso_count))
        .route("/logs/{id}/stats", get(log_stats))
        .route("/logs/{id}/adif", post(export_adif))
        .route(
            "/logs/{id}/adif/import",
            post(import_adif).layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route("/logs/{id}/cabrillo", post(export_cabrillo))
        .route("/logs/{id}/serial-allocation", post(allocate_serials))
        .route(
            "/logs/{log_id}/contacts",
            get(contacts).post(commit_contact),
        )
        .route("/contacts/{id}", delete(delete_contact))
        .route("/audio-devices/input", get(input_audio_devices))
        .route("/audio-devices/output", get(output_audio_devices))
        .route("/radio-kinds", get(radio_kinds))
        .route("/serial-ports", get(serial_ports))
        .route("/radios", get(radios).post(create_radio))
        .route("/radios/cw-messages/default", get(default_cw_messages))
        .route("/radios/cw-messages/validate", post(validate_cw_messages))
        .route(
            "/radios/voice-messages/default",
            get(default_voice_messages),
        )
        .route(
            "/radios/voice-messages/validate",
            post(validate_voice_messages),
        )
        .route(
            "/radios/{id}",
            get(radio).put(update_radio).delete(delete_radio),
        )
        .route("/radios/{id}/cw-labels", get(cw_labels))
        .route("/radios/{id}/message-labels", get(message_labels));

    let app = Router::new()
        .nest("/api", api)
        .route("/ws", get(ws_handler))
        .fallback(static_assets::static_handler)
        .with_state(app_state.clone())
        .layer(middleware::from_fn_with_state(app_state, auth::basic_auth))
        .layer(request_trace_layer);

    let listener = tokio::net::TcpListener::bind(&cli.bind)
        .await
        .unwrap_or_else(|error| panic!("failed to bind backend to {}: {error}", cli.bind));

    info!(
        address = %cli.bind,
        "log73 backend listening; radio connections are lazy"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server failed");
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.expect("failed to listen for SIGINT");
                info!("received SIGINT; starting graceful shutdown");
            }
            _ = sigterm.recv() => {
                info!("received SIGTERM; starting graceful shutdown");
            }
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for shutdown signal");
        info!("received shutdown signal; starting graceful shutdown");
    }
}

async fn ws_handler(
    State(app_state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let session_id = params.get("session_id").cloned().unwrap_or_default();
    let radio_id = params
        .get("radio_id")
        .and_then(|value| value.parse::<i64>().ok());
    let log_id = params
        .get("log_id")
        .and_then(|value| value.parse::<i64>().ok());
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, session_id, radio_id, log_id))
}

async fn handle_socket(
    socket: WebSocket,
    app_state: AppState,
    session_id: String,
    radio_id: Option<i64>,
    log_id: Option<i64>,
) {
    let Some(radio_id) = radio_id else {
        warn!(session_id, "backend websocket missing radio_id");
        return;
    };

    let Ok(radio_handle) = app_state.radio_manager.acquire(radio_id).await else {
        warn!(
            session_id,
            radio_id, "backend websocket requested unavailable radio"
        );
        return;
    };

    info!(session_id, radio_id, "backend websocket connected");
    let (mut sender, mut receiver) = socket.split();

    let current_status = radio_handle.current_status_message().await;
    if sender
        .send(Message::Text(
            serde_json::to_string(&current_status)
                .expect("radio status should serialize")
                .into(),
        ))
        .await
        .is_err()
    {
        app_state.radio_manager.release(radio_id).await;
        return;
    }

    if let Some(current) = radio_handle.current_message().await
        && sender
            .send(Message::Text(
                serde_json::to_string(&current)
                    .expect("radio state should serialize")
                    .into(),
            ))
            .await
            .is_err()
    {
        app_state.radio_manager.release(radio_id).await;
        return;
    }

    if let Some(log_id) = log_id
        && let Some(totals) = app_state.incremental_scoring.totals(log_id)
    {
        let score_update = score_update_message(log_id, &totals);
        if sender
            .send(Message::Text(
                serde_json::to_string(&score_update)
                    .expect("score update should serialize")
                    .into(),
            ))
            .await
            .is_err()
        {
            app_state.radio_manager.release(radio_id).await;
            return;
        }
    }

    let mut radio_status_updates = radio_handle.subscribe_status();
    let mut radio_updates = radio_handle.subscribe();
    let mut log_events = app_state.log_events.subscribe();
    let (direct_tx, mut direct_rx) = mpsc::channel::<ServerMessage>(32);
    let mut dxcluster_enabled = false;
    let mut dxcluster_subscription: Option<tokio::task::JoinHandle<()>> = None;
    let outbound_session_id = session_id.clone();
    let outbound_log_id = log_id;
    let outbound = tokio::spawn(async move {
        loop {
            let message = tokio::select! {
                status = radio_status_updates.recv() => match status {
                    Ok(status) => serde_json::to_string(&ServerMessage::RadioStatus(status)).expect("radio status should serialize"),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                update = radio_updates.recv() => match update {
                    Ok(update) => serde_json::to_string(&ServerMessage::RadioState(update)).expect("radio state should serialize"),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                event = log_events.recv() => match event {
                    Ok(event) => match websocket_log_event_for_client(event, outbound_session_id.as_str(), outbound_log_id) {
                        Some(event) => serde_json::to_string(&event).expect("log event should serialize"),
                        None => continue,
                    },
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                direct = direct_rx.recv() => match direct {
                    Some(message) => {
                        if let ServerMessage::Pong { request_id } = &message {
                            debug!(session_id = %outbound_session_id, radio_id, request_id, "sending websocket pong");
                        }
                        serde_json::to_string(&message).expect("direct message should serialize")
                    }
                    None => break,
                }
            };

            if sender.send(Message::Text(message.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = receiver.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        match serde_json::from_str::<ClientMessage>(&text) {
            Ok(ClientMessage::Ping { request_id }) => {
                debug!(session_id, radio_id, request_id, "websocket ping received");
                debug!(session_id, radio_id, request_id, "queueing websocket pong");
                if direct_tx
                    .send(ServerMessage::Pong { request_id })
                    .await
                    .is_err()
                {
                    debug!(
                        session_id,
                        radio_id, "failed to queue websocket pong; session closed"
                    );
                    break;
                }
            }
            Ok(ClientMessage::SetDxClusterEnabled { enabled }) => {
                debug!(
                    session_id,
                    radio_id, enabled, "websocket set_dxcluster_enabled command received"
                );
                if dxcluster_enabled == enabled {
                    continue;
                }
                dxcluster_enabled = enabled;
                if let Some(subscription) = dxcluster_subscription.take() {
                    subscription.abort();
                }
                if enabled {
                    dxcluster_subscription = Some(spawn_dxcluster_websocket_subscription(
                        app_state.dxcluster.clone(),
                        direct_tx.clone(),
                        session_id.clone(),
                        radio_id,
                    ));
                }
            }
            Ok(ClientMessage::SetFrequency { frequency_hz }) => {
                debug!(
                    session_id,
                    radio_id, frequency_hz, "websocket set_frequency command received"
                );
                if let Err(error) = validation::validate_radio_frequency_hz(frequency_hz) {
                    warn!(session_id, radio_id, frequency_hz, %error, "invalid websocket set_frequency command");
                    continue;
                }
                let _ = radio_handle
                    .send_command(RadioCommand::SetFrequency(frequency_hz))
                    .await;
            }
            Ok(ClientMessage::SetMode { mode }) => {
                debug!(
                    session_id,
                    radio_id, mode, "websocket set_mode command received"
                );
                if let Err(error) = validation::validate_radio_mode(&mode) {
                    warn!(session_id, radio_id, mode, %error, "invalid websocket set_mode command");
                    continue;
                }
                let _ = radio_handle.send_command(RadioCommand::SetMode(mode)).await;
            }
            Ok(ClientMessage::RitClear) => {
                debug!(session_id, radio_id, "websocket rit_clear command received");
                let _ = radio_handle.send_command(RadioCommand::RitClear).await;
            }
            Ok(ClientMessage::RitIncrement { hz }) => {
                debug!(
                    session_id,
                    radio_id, hz, "websocket rit_increment command received"
                );
                if let Err(error) = validation::validate_rit_adjustment_hz(hz) {
                    warn!(session_id, radio_id, hz, %error, "invalid websocket rit_increment command");
                    continue;
                }
                let _ = radio_handle
                    .send_command(RadioCommand::RitIncrement(hz))
                    .await;
            }
            Ok(ClientMessage::RitDecrement { hz }) => {
                debug!(
                    session_id,
                    radio_id, hz, "websocket rit_decrement command received"
                );
                if let Err(error) = validation::validate_rit_adjustment_hz(hz) {
                    warn!(session_id, radio_id, hz, %error, "invalid websocket rit_decrement command");
                    continue;
                }
                let _ = radio_handle
                    .send_command(RadioCommand::RitDecrement(hz))
                    .await;
            }
            Ok(ClientMessage::SendMessage {
                request_id,
                mode,
                keys,
                fields,
            }) => {
                debug!(
                    session_id,
                    radio_id,
                    request_id,
                    mode,
                    ?keys,
                    "websocket send_message command received"
                );
                if let Err(error) =
                    validation::validate_message_request(&request_id, &mode, &keys, &fields)
                {
                    warn!(session_id, radio_id, request_id, mode, ?keys, %error, "invalid websocket send_message command");
                    continue;
                }
                let (completed_tx, completed_rx) = oneshot::channel();
                let command_result = radio_handle
                    .send_command(RadioCommand::SendMessage {
                        mode,
                        keys,
                        fields,
                        completed: completed_tx,
                    })
                    .await;
                if command_result.is_ok() {
                    let direct_tx = direct_tx.clone();
                    let completion_session_id = session_id.clone();
                    tokio::spawn(async move {
                        debug!(
                            session_id = %completion_session_id,
                            request_id,
                            "waiting for message send completion"
                        );
                        match completed_rx.await {
                            Ok(Ok(())) => {
                                debug!(
                                    session_id = %completion_session_id,
                                    request_id,
                                    "message send complete; sending message_sent websocket message"
                                );
                                if direct_tx
                                    .send(ServerMessage::MessageSent { request_id })
                                    .await
                                    .is_err()
                                {
                                    debug!(
                                        session_id = %completion_session_id,
                                        "unable to send message_sent websocket message; session closed"
                                    );
                                }
                            }
                            Ok(Err(error)) => {
                                debug!(
                                    session_id = %completion_session_id,
                                    request_id,
                                    %error,
                                    "message send did not complete; not sending message_sent websocket message"
                                );
                            }
                            Err(error) => {
                                debug!(
                                    session_id = %completion_session_id,
                                    request_id,
                                    %error,
                                    "message completion channel closed; not sending message_sent websocket message"
                                );
                            }
                        }
                    });
                } else {
                    debug!(
                        session_id,
                        radio_id, request_id, "failed to queue message command"
                    );
                }
            }
            Ok(ClientMessage::SendCwText {
                request_id,
                text,
                wait_for_completion,
            }) => {
                debug!(
                    session_id,
                    radio_id, request_id, "websocket send_cw_text command received"
                );
                if let Err(error) = validation::validate_cw_text_request(&request_id, &text) {
                    warn!(session_id, radio_id, request_id, %error, "invalid websocket send_cw_text command");
                    continue;
                }
                let (completed_tx, completed_rx) = oneshot::channel();
                let command_result = radio_handle
                    .send_command(RadioCommand::SendCwText {
                        text,
                        wait_for_completion,
                        completed: completed_tx,
                    })
                    .await;
                if command_result.is_ok() {
                    let direct_tx = direct_tx.clone();
                    let completion_session_id = session_id.clone();
                    tokio::spawn(async move {
                        debug!(
                            session_id = %completion_session_id,
                            request_id,
                            "waiting for cw text send completion"
                        );
                        match completed_rx.await {
                            Ok(Ok(())) => {
                                debug!(
                                    session_id = %completion_session_id,
                                    request_id,
                                    "cw text send complete; sending message_sent websocket message"
                                );
                                if direct_tx
                                    .send(ServerMessage::MessageSent { request_id })
                                    .await
                                    .is_err()
                                {
                                    debug!(
                                        session_id = %completion_session_id,
                                        "unable to send message_sent websocket message; session closed"
                                    );
                                }
                            }
                            Ok(Err(error)) => {
                                debug!(
                                    session_id = %completion_session_id,
                                    request_id,
                                    %error,
                                    "cw text send did not complete; not sending message_sent websocket message"
                                );
                            }
                            Err(error) => {
                                debug!(
                                    session_id = %completion_session_id,
                                    request_id,
                                    %error,
                                    "cw text completion channel closed; not sending message_sent websocket message"
                                );
                            }
                        }
                    });
                } else {
                    debug!(
                        session_id,
                        radio_id, request_id, "failed to queue cw text command"
                    );
                }
            }
            Ok(ClientMessage::SendDxClusterSpot {
                frequency_hz,
                call,
                comment,
            }) => {
                debug!(
                    session_id,
                    radio_id, frequency_hz, call, "websocket send_dxcluster_spot command received"
                );
                if let Err(error) =
                    validation::validate_dxcluster_spot_request(frequency_hz, &call, &comment)
                {
                    warn!(session_id, radio_id, frequency_hz, call, %error, "invalid websocket send_dxcluster_spot command");
                    continue;
                }
                let frequency_khz = format_dxcluster_frequency_khz(frequency_hz);
                let normalized_call = call.trim().to_uppercase();
                let text = format!(
                    "DX {} {} {}",
                    frequency_khz,
                    normalized_call,
                    comment.trim()
                );
                if let Err(error) = app_state.dxcluster.send_text(text).await {
                    warn!(session_id, radio_id, frequency_hz, call = %normalized_call, %error, "failed to send DX cluster spot");
                }
            }
            Ok(ClientMessage::StopKeying) => {
                debug!(
                    session_id,
                    radio_id, "websocket stop_keying command received"
                );
                let _ = radio_handle.send_command(RadioCommand::StopKeying).await;
            }
            Ok(ClientMessage::SetWpm { wpm }) => {
                debug!(
                    session_id,
                    radio_id, wpm, "websocket set_wpm command received"
                );
                if let Err(error) = validation::validate_cw_wpm(wpm) {
                    warn!(session_id, radio_id, wpm, %error, "invalid websocket set_wpm command");
                    continue;
                }
                let _ = radio_handle.send_command(RadioCommand::SetWpm(wpm)).await;
            }
            Err(error) => warn!(session_id, radio_id, %error, "invalid websocket message"),
        }
    }

    if let Some(subscription) = dxcluster_subscription {
        subscription.abort();
    }
    outbound.abort();
    app_state.radio_manager.release(radio_id).await;
    info!(session_id, radio_id, "backend websocket disconnected");
}

fn spawn_dxcluster_websocket_subscription(
    dxcluster: DxClusterManager,
    direct_tx: mpsc::Sender<ServerMessage>,
    session_id: String,
    radio_id: i64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut events = dxcluster.subscribe();
        loop {
            let message = match events.recv().await {
                Ok(DxClusterEvent::SpotAdded(spot)) => ServerMessage::DxClusterSpot { spot },
                Ok(DxClusterEvent::SpotDeleted { id }) => {
                    ServerMessage::DxClusterSpotDeleted { id }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(session_id = %session_id, radio_id, skipped, "websocket dxcluster subscription lagged");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            };

            if direct_tx.send(message).await.is_err() {
                debug!(session_id = %session_id, radio_id, "websocket dxcluster subscription closed");
                break;
            }
        }
    })
}

#[derive(Debug, serde::Deserialize)]
struct ContestSettingsQuery {
    contest_id: Option<String>,
}

async fn list_contest_rules(
    State(app_state): State<AppState>,
) -> Json<Vec<contest_rules::ContestSummary>> {
    Json(app_state.contest_rules.summaries())
}

async fn contest_settings(
    State(app_state): State<AppState>,
    Query(query): Query<ContestSettingsQuery>,
) -> Json<ContestRules> {
    let rules = query
        .contest_id
        .as_deref()
        .and_then(|contest_id| app_state.contest_rules.get(contest_id))
        .or_else(|| app_state.contest_rules.default_contest())
        .expect("contest rules store should not be empty")
        .clone();
    Json(rules)
}

async fn supercheckpartial_matches(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    cached_json_response(
        &headers,
        &serde_json::json!({ "ok": true, "callsigns": app_state.supercheckpartial.callsigns() }),
    )
}

async fn dxcc_data(State(app_state): State<AppState>, headers: HeaderMap) -> Response {
    cached_json_response(
        &headers,
        &serde_json::json!({ "ok": true, "dxcc": app_state.dxcc.as_ref() }),
    )
}

async fn dxcluster_spots(State(app_state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "spots": app_state.dxcluster.spots().await }))
}

async fn clear_dxcluster_spots(State(app_state): State<AppState>) -> Json<serde_json::Value> {
    let deleted_ids = app_state.dxcluster.clear_spots().await;
    Json(serde_json::json!({ "ok": true, "deleted_ids": deleted_ids }))
}

#[derive(Debug, serde::Deserialize)]
struct SaveDxClusterSpotPayload {
    frequency_hz: u64,
    call: String,
    #[serde(default)]
    comment: String,
}

async fn save_dxcluster_spot(
    State(app_state): State<AppState>,
    Json(payload): Json<SaveDxClusterSpotPayload>,
) -> Json<serde_json::Value> {
    if let Err(error) = validation::validate_dxcluster_spot_request(
        payload.frequency_hz,
        &payload.call,
        &payload.comment,
    ) {
        return Json(serde_json::json!({ "ok": false, "error": error }));
    }

    let comment = payload.comment.trim();
    let spot = app_state
        .dxcluster
        .add_manual_spot(
            payload.frequency_hz,
            payload.call.trim().to_uppercase(),
            (!comment.is_empty()).then(|| comment.to_string()),
        )
        .await;

    Json(serde_json::json!({ "ok": true, "spot": spot }))
}

async fn config(State(app_state): State<AppState>) -> Json<serde_json::Value> {
    match app_state.db.config_view().await {
        Ok(config) => Json(serde_json::json!({ "ok": true, "config": config })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
struct UpdateConfigPayload {
    #[serde(default)]
    login_user: String,
    #[serde(default)]
    login_password_change: Option<String>,
    #[serde(default)]
    login_password_confirm: Option<String>,
    #[serde(default)]
    disable_login: bool,
    #[serde(default)]
    dxcluster_enabled: bool,
    #[serde(default)]
    dxcluster_host: String,
    #[serde(default = "default_dxcluster_port")]
    dxcluster_port: u16,
    #[serde(default)]
    dxcluster_callsign: String,
    #[serde(default = "default_dxcluster_max_age_min")]
    dxcluster_max_age_min: u16,
    #[serde(default)]
    dxcluster_commands: String,
}

fn default_dxcluster_port() -> u16 {
    db::DEFAULT_DXCLUSTER_PORT
}

fn default_dxcluster_max_age_min() -> u16 {
    db::DEFAULT_DXCLUSTER_MAX_AGE_MIN
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
struct ClientErrorPayload {
    name: Option<String>,
    message: Option<String>,
    stack: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
struct ClientErrorReportPayload {
    source: Option<String>,
    message: Option<String>,
    url: Option<String>,
    user_agent: Option<String>,
    error: Option<ClientErrorPayload>,
    details: Option<serde_json::Value>,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
struct CabrilloExportPayload {
    #[serde(default)]
    export_params: serde_json::Value,
}

#[derive(Debug, Default, serde::Deserialize)]
struct AdifImportPayload {
    #[serde(default)]
    adif: String,
    #[serde(default)]
    mappings: adif::ImportMappings,
}

fn truncate_text(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        return value.to_string();
    }
    value.chars().take(max_len).collect()
}

fn normalized_text(value: Option<&str>, max_len: usize) -> Option<String> {
    let text = value.unwrap_or("").trim();
    if text.is_empty() {
        return None;
    }
    Some(truncate_text(text, max_len))
}

fn normalized_json_text(value: Option<&serde_json::Value>, max_len: usize) -> Option<String> {
    let value = value?;
    let serialized = serde_json::to_string(value)
        .unwrap_or_else(|error| format!("<unable to serialize client error details: {error}>"));
    Some(truncate_text(&serialized, max_len))
}

async fn report_client_error(
    Json(payload): Json<ClientErrorReportPayload>,
) -> Json<serde_json::Value> {
    let source = normalized_text(payload.source.as_deref(), MAX_CLIENT_ERROR_TEXT_LENGTH)
        .unwrap_or_else(|| "frontend".to_string());
    let message = normalized_text(payload.message.as_deref(), MAX_CLIENT_ERROR_TEXT_LENGTH);
    let url = normalized_text(payload.url.as_deref(), MAX_CLIENT_ERROR_TEXT_LENGTH);
    let user_agent = normalized_text(payload.user_agent.as_deref(), MAX_CLIENT_ERROR_TEXT_LENGTH);
    let error_name = normalized_text(
        payload
            .error
            .as_ref()
            .and_then(|error| error.name.as_deref()),
        MAX_CLIENT_ERROR_TEXT_LENGTH,
    );
    let error_message = normalized_text(
        payload
            .error
            .as_ref()
            .and_then(|error| error.message.as_deref()),
        MAX_CLIENT_ERROR_TEXT_LENGTH,
    );
    let error_stack = normalized_text(
        payload
            .error
            .as_ref()
            .and_then(|error| error.stack.as_deref()),
        MAX_CLIENT_ERROR_JSON_LENGTH,
    );
    let details = normalized_json_text(payload.details.as_ref(), MAX_CLIENT_ERROR_JSON_LENGTH);

    error!(
        source = %source,
        client_message = message.as_deref().unwrap_or(""),
        url = url.as_deref().unwrap_or(""),
        user_agent = user_agent.as_deref().unwrap_or(""),
        error_name = error_name.as_deref().unwrap_or(""),
        error_message = error_message.as_deref().unwrap_or(""),
        error_stack = error_stack.as_deref().unwrap_or(""),
        details = details.as_deref().unwrap_or(""),
        "frontend client error reported"
    );

    Json(serde_json::json!({ "ok": true }))
}

async fn update_config(
    State(app_state): State<AppState>,
    Json(payload): Json<UpdateConfigPayload>,
) -> Json<serde_json::Value> {
    debug!("update config PUT request");
    let login_password = match validation::validate_auth_config(
        &payload.login_user,
        payload.login_password_change.as_deref(),
        payload.login_password_confirm.as_deref(),
        payload.disable_login,
    ) {
        Ok(validation::LoginPasswordChange::Preserve) => db::LoginPasswordUpdate::Preserve,
        Ok(validation::LoginPasswordChange::Disable) => db::LoginPasswordUpdate::Disable,
        Ok(validation::LoginPasswordChange::Change(password)) => {
            match auth::hash_password(&password) {
                Ok(login_password) => db::LoginPasswordUpdate::Set(login_password),
                Err(error) => return Json(serde_json::json!({ "ok": false, "error": error })),
            }
        }
        Err(error) => return Json(serde_json::json!({ "ok": false, "error": error })),
    };
    if let Err(error) = validation::validate_dxcluster_config(
        &payload.dxcluster_host,
        payload.dxcluster_port,
        &payload.dxcluster_callsign,
        payload.dxcluster_max_age_min,
        &payload.dxcluster_commands,
    ) {
        return Json(serde_json::json!({ "ok": false, "error": error }));
    }

    match app_state
        .db
        .update_config(db::UpdateConfig {
            login_user: payload.login_user,
            login_password,
            dxcluster_enabled: payload.dxcluster_enabled,
            dxcluster_host: payload.dxcluster_host,
            dxcluster_port: payload.dxcluster_port,
            dxcluster_callsign: payload.dxcluster_callsign,
            dxcluster_max_age_min: payload.dxcluster_max_age_min,
            dxcluster_commands: payload.dxcluster_commands,
        })
        .await
    {
        Ok(()) => {
            match app_state.db.auth_config().await {
                Ok(config) => *app_state.auth_config.write().await = config,
                Err(error) => warn!(%error, "failed to reload auth config after update"),
            }
            match app_state.db.dxcluster_config().await {
                Ok(config) => app_state.dxcluster.apply_config(config).await,
                Err(error) => warn!(%error, "failed to reload DX Cluster config after update"),
            }
            match app_state.db.config_view().await {
                Ok(config) => Json(serde_json::json!({ "ok": true, "config": config })),
                Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
            }
        }
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn logs(State(app_state): State<AppState>) -> Json<Vec<db::Log>> {
    match app_state.db.logs().await {
        Ok(logs) => Json(logs),
        Err(error) => {
            error!(%error, "failed to load logs");
            Json(Vec::new())
        }
    }
}

async fn log(State(app_state): State<AppState>, Path(id): Path<i64>) -> Json<serde_json::Value> {
    match app_state.db.log(id).await {
        Ok(Some(log)) => Json(serde_json::json!({ "ok": true, "log": log })),
        Ok(None) => Json(serde_json::json!({ "ok": false, "error": "not found" })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn create_log(
    State(app_state): State<AppState>,
    Json(payload): Json<NewLog>,
) -> Json<serde_json::Value> {
    debug!(payload = %pretty_json(&payload), "create log POST body");
    if let Err(error) = validation::validate_new_log(&app_state.contest_rules, &payload) {
        return Json(serde_json::json!({ "ok": false, "error": error }));
    }
    match app_state.db.create_log(payload).await {
        Ok(log) => Json(serde_json::json!({ "ok": true, "log": log })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn update_log(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateLog>,
) -> Json<serde_json::Value> {
    debug!(id, payload = %pretty_json(&payload), "update log PUT body");
    let log = match app_state.db.log(id).await {
        Ok(Some(log)) => log,
        Ok(None) => return Json(serde_json::json!({ "ok": false, "error": "not found" })),
        Err(error) => return Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    };
    let Some(rules) = app_state.contest_rules.get(&log.contest_id) else {
        return Json(
            serde_json::json!({ "ok": false, "error": format!("unknown contest: {}", log.contest_id) }),
        );
    };
    if let Err(error) = validation::validate_update_log(rules, &payload) {
        return Json(serde_json::json!({ "ok": false, "error": error }));
    }
    match app_state.db.update_log(id, payload).await {
        Ok(Some(log)) => {
            app_state.log_cache.remove_log(id);
            Json(serde_json::json!({ "ok": true, "log": log }))
        }
        Ok(None) => Json(serde_json::json!({ "ok": false, "error": "not found" })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn export_cabrillo(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<CabrilloExportPayload>,
) -> impl IntoResponse {
    let log = match app_state.db.log(id).await {
        Ok(Some(log)) => log,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "ok": false, "error": "not found" })),
            )
                .into_response();
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
            )
                .into_response();
        }
    };
    let Some(rules) = app_state.contest_rules.get(&log.contest_id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "ok": false, "error": format!("unknown contest: {}", log.contest_id) })),
        )
            .into_response();
    };
    if let Err(error) = validation::validate_cabrillo_export_params(rules, &payload.export_params) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "ok": false, "error": error })),
        )
            .into_response();
    }

    let mut contacts = match app_state.db.contacts(id).await {
        Ok(contacts) => contacts,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
            )
                .into_response();
        }
    };
    contacts.sort_by_key(contact_score_order);
    let mut scored_contacts = contacts.clone();
    let claimed_score =
        score_contacts(rules, log.contest_params.clone(), &mut scored_contacts).score;
    let text = match cabrillo::render_log(
        rules,
        &log,
        &contacts,
        &payload.export_params,
        claimed_score,
    ) {
        Ok(text) => text,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "ok": false, "error": error })),
            )
                .into_response();
        }
    };

    download_response(
        text,
        "text/plain; charset=utf-8",
        &cabrillo::export_filename(&log),
    )
}

async fn export_adif(State(app_state): State<AppState>, Path(id): Path<i64>) -> impl IntoResponse {
    let log = match app_state.db.log(id).await {
        Ok(Some(log)) => log,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "ok": false, "error": "not found" })),
            )
                .into_response();
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
            )
                .into_response();
        }
    };

    let contacts = match app_state.db.contacts(id).await {
        Ok(contacts) => contacts,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
            )
                .into_response();
        }
    };

    let text = match adif::render_log(&log, &contacts) {
        Ok(text) => text,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "ok": false, "error": error })),
            )
                .into_response();
        }
    };

    download_response(
        text,
        "text/plain; charset=utf-8",
        &adif::export_filename(&log),
    )
}

async fn import_adif(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<AdifImportPayload>,
) -> Json<serde_json::Value> {
    let log = match app_state.db.log(id).await {
        Ok(Some(log)) => log,
        Ok(None) => return Json(serde_json::json!({ "ok": false, "error": "not found" })),
        Err(error) => return Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    };
    let Some(rules) = app_state.contest_rules.get(&log.contest_id) else {
        return Json(
            serde_json::json!({ "ok": false, "error": format!("unknown contest: {}", log.contest_id) }),
        );
    };

    let imported = match adif::import_contacts(&log, rules, &payload.adif, &payload.mappings) {
        Ok(imported) => imported,
        Err(error) => {
            return Json(serde_json::json!({
                "ok": false,
                "error": error.error,
                "line": error.line,
                "errors": [error],
            }));
        }
    };
    let contacts = imported
        .iter()
        .map(|imported| imported.contact.clone())
        .collect::<Vec<_>>();

    if let Err((index, error)) =
        validation::validate_import_contacts(&app_state.db, &app_state.contest_rules, id, &contacts)
            .await
    {
        let line = imported
            .get(index)
            .map(|imported| imported.line)
            .unwrap_or(1);
        let import_error = adif::ImportError { line, error };
        return Json(serde_json::json!({
            "ok": false,
            "error": import_error.error,
            "line": import_error.line,
            "errors": [import_error],
        }));
    }

    match app_state.log_cache.upsert_contacts(id, contacts).await {
        Ok(result) => {
            for contact in &result.contacts {
                let _ = app_state.log_events.send(ServerMessage::LogEntry {
                    contact: contact.clone(),
                });
            }
            for contact in result.changed_contacts {
                let _ = app_state
                    .log_events
                    .send(ServerMessage::LogEntry { contact });
            }

            let totals = app_state.incremental_scoring.totals(id).unwrap_or_default();
            send_score_update(&app_state, id, &totals);

            let imported = result.contacts.len();
            Json(serde_json::json!({
                "ok": true,
                "imported": imported,
                "contacts": result.contacts,
            }))
        }
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

fn download_response(body: String, content_type: &str, filename: &str) -> axum::response::Response {
    let mut response = body.into_response();
    if let Ok(content_type) = HeaderValue::from_str(content_type) {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
    }
    if let Ok(disposition) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
    {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, disposition);
    }
    response
}

async fn delete_log(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<serde_json::Value> {
    match app_state.db.delete_log(id).await {
        Ok(deleted) => {
            if deleted {
                app_state.log_cache.remove_log(id);
            }
            Json(serde_json::json!({ "ok": true, "deleted": deleted }))
        }
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn log_qso_count(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<serde_json::Value> {
    match app_state.db.log(id).await {
        Ok(Some(_)) => {}
        Ok(None) => return Json(serde_json::json!({ "ok": false, "error": "not found" })),
        Err(error) => return Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }

    match app_state.db.log_qso_count(id).await {
        Ok(qso_count) => Json(serde_json::json!({ "ok": true, "qso_count": qso_count })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn log_stats(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<serde_json::Value> {
    if let Err(error) = app_state.log_cache.ensure_loaded(id).await {
        return Json(serde_json::json!({ "ok": false, "error": error }));
    }

    match app_state.stats.snapshot(id) {
        Some(stats) => Json(serde_json::json!({ "ok": true, "stats": stats })),
        None => Json(serde_json::json!({ "ok": false, "error": "not found" })),
    }
}

async fn input_audio_devices(State(app_state): State<AppState>) -> Json<serde_json::Value> {
    match app_state.voice_keyer.input_devices() {
        Ok(devices) => Json(serde_json::json!({ "ok": true, "devices": devices })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error })),
    }
}

async fn output_audio_devices(State(app_state): State<AppState>) -> Json<serde_json::Value> {
    match app_state.voice_keyer.output_devices() {
        Ok(devices) => Json(serde_json::json!({ "ok": true, "devices": devices })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error })),
    }
}

async fn radios(State(app_state): State<AppState>) -> Json<Vec<db::RadioConfig>> {
    match app_state.db.radios().await {
        Ok(mut radios) => {
            for radio in &mut radios {
                app_state.voice_keyer.sanitize_radio_config(radio);
            }
            Json(radios)
        }
        Err(error) => {
            error!(%error, "failed to load radios");
            Json(Vec::new())
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct RadioKindOption {
    id: &'static str,
    display_name: &'static str,
    description: &'static str,
}

async fn radio_kinds() -> Json<Vec<RadioKindOption>> {
    Json(
        supported_drivers()
            .iter()
            .map(|driver| RadioKindOption {
                id: driver.id,
                display_name: driver.display_name,
                description: driver.description,
            })
            .collect(),
    )
}

#[derive(Debug, serde::Serialize)]
struct SerialPortOption {
    name: String,
    display_name: String,
}

async fn serial_ports() -> Json<serde_json::Value> {
    match list_serial_ports() {
        Ok(entries) => Json(serde_json::json!({
            "ok": true,
            "serial_ports": entries
                .into_iter()
                .map(|entry| SerialPortOption {
                    display_name: entry.to_string(),
                    name: entry.name,
                })
                .collect::<Vec<_>>()
        })),
        Err(error) => Json(serde_json::json!({
            "ok": false,
            "error": error.to_string()
        })),
    }
}

async fn radio(State(app_state): State<AppState>, Path(id): Path<i64>) -> Json<serde_json::Value> {
    match app_state.db.radio(id).await {
        Ok(Some(mut radio)) => {
            app_state.voice_keyer.sanitize_radio_config(&mut radio);
            Json(serde_json::json!({ "ok": true, "radio": radio }))
        }
        Ok(None) => Json(serde_json::json!({ "ok": false, "error": "not found" })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn cw_labels(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<serde_json::Value> {
    match app_state.db.radio(id).await {
        Ok(Some(radio)) => {
            Json(serde_json::json!({ "ok": true, "labels": cw::labels(&radio.cw_messages) }))
        }
        Ok(None) => Json(serde_json::json!({ "ok": false, "error": "not found" })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn message_labels(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<serde_json::Value> {
    match app_state.db.radio(id).await {
        Ok(Some(radio)) => Json(serde_json::json!({
            "ok": true,
            "labels": {
                "cw": cw::labels(&radio.cw_messages),
                "voice": voice_messages::labels(&radio.voice_messages)
            }
        })),
        Ok(None) => Json(serde_json::json!({ "ok": false, "error": "not found" })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

#[derive(Debug, serde::Deserialize)]
struct CwMessagesPayload {
    cw_messages: String,
}

#[derive(Debug, serde::Deserialize)]
struct VoiceMessagesPayload {
    voice_messages: String,
}

async fn default_cw_messages() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "cw_messages": cw::DEFAULT_CW_MESSAGES }))
}

async fn validate_cw_messages(Json(payload): Json<CwMessagesPayload>) -> Json<serde_json::Value> {
    match validation::validate_cw_messages(&payload.cw_messages) {
        Ok(()) => Json(serde_json::json!({
            "ok": true,
            "labels": cw::labels(&payload.cw_messages)
        })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error })),
    }
}

async fn default_voice_messages() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "ok": true,
        "voice_messages": voice_messages::DEFAULT_VOICE_MESSAGES
    }))
}

async fn validate_voice_messages(
    State(app_state): State<AppState>,
    Json(payload): Json<VoiceMessagesPayload>,
) -> Json<serde_json::Value> {
    match app_state
        .voice_keyer
        .validate_voice_messages(&payload.voice_messages)
    {
        Ok(labels) => Json(serde_json::json!({ "ok": true, "labels": labels })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error })),
    }
}

async fn create_radio(
    State(app_state): State<AppState>,
    Json(payload): Json<NewRadio>,
) -> Json<serde_json::Value> {
    debug!(payload = %pretty_json(&payload), "create radio POST body");
    if let Err(error) = validation::validate_radio(&payload) {
        return Json(serde_json::json!({ "ok": false, "error": error }));
    }
    if let Err(error) = app_state
        .voice_keyer
        .validate_voice_messages(&payload.voice_messages)
    {
        return Json(serde_json::json!({ "ok": false, "error": error }));
    }
    match app_state.db.create_radio(payload).await {
        Ok(mut radio) => {
            if let Err(error) = app_state.voice_keyer.sync_radio_messages(&radio) {
                warn!(radio_id = radio.id, %error, "failed to sync voice keyer registrations after radio create");
            }
            app_state.voice_keyer.sanitize_radio_config(&mut radio);
            Json(serde_json::json!({ "ok": true, "radio": radio }))
        }
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn update_radio(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<NewRadio>,
) -> Json<serde_json::Value> {
    debug!(id, payload = %pretty_json(&payload), "update radio PUT body");
    if let Err(error) = validation::validate_radio(&payload) {
        return Json(serde_json::json!({ "ok": false, "error": error }));
    }
    if let Err(error) = app_state
        .voice_keyer
        .validate_voice_messages(&payload.voice_messages)
    {
        return Json(serde_json::json!({ "ok": false, "error": error }));
    }
    match app_state.db.update_radio(id, payload).await {
        Ok(Some(mut radio)) => {
            if let Err(error) = app_state.voice_keyer.sync_radio_messages(&radio) {
                warn!(radio_id = radio.id, %error, "failed to sync voice keyer registrations after radio update");
            }
            app_state.voice_keyer.sanitize_radio_config(&mut radio);
            let active = app_state.radio_manager.is_active(id).await;
            debug!(
                id,
                active, "radio updated; checking whether reload is needed"
            );
            if active {
                if let Err(error) = app_state
                    .radio_manager
                    .reload_config(id, radio.clone())
                    .await
                {
                    warn!(id, %error, "failed to reload active radio after update");
                    return Json(serde_json::json!({ "ok": false, "error": error }));
                }
                debug!(id, "active radio reload requested after update");
            }
            Json(serde_json::json!({ "ok": true, "radio": radio }))
        }
        Ok(None) => Json(serde_json::json!({ "ok": false, "error": "not found" })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn delete_radio(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<serde_json::Value> {
    if app_state.radio_manager.is_active(id).await {
        return Json(serde_json::json!({ "ok": false, "error": "cannot delete an active radio" }));
    }

    match app_state.db.delete_radio(id).await {
        Ok(deleted) => {
            if deleted && let Err(error) = app_state.voice_keyer.clear_radio_messages(id) {
                warn!(id, %error, "failed to clear voice keyer registrations after radio delete");
            }
            Json(serde_json::json!({ "ok": true, "deleted": deleted }))
        }
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

const DEFAULT_SERIAL_BATCH_SIZE: i64 = 10;
const MAX_SERIAL_BATCH_SIZE: i64 = 1000;
const DEFAULT_CONTACTS_PAGE_LIMIT: usize = 200;
const MAX_CONTACTS_PAGE_LIMIT: usize = 1000;

#[derive(Debug, serde::Deserialize)]
struct SerialAllocationPayload {
    field_adif: String,
    count: Option<i64>,
}

async fn allocate_serials(
    State(app_state): State<AppState>,
    Path(log_id): Path<i64>,
    Json(payload): Json<SerialAllocationPayload>,
) -> Json<serde_json::Value> {
    let field_adif = payload.field_adif.trim();
    if field_adif.is_empty() {
        return Json(
            serde_json::json!({ "ok": false, "error": "serial field ADIF name is required" }),
        );
    }
    let count = payload
        .count
        .unwrap_or(DEFAULT_SERIAL_BATCH_SIZE)
        .clamp(1, MAX_SERIAL_BATCH_SIZE);

    let log = match app_state.db.log(log_id).await {
        Ok(Some(log)) => log,
        Ok(None) => {
            return Json(
                serde_json::json!({ "ok": false, "error": format!("log {log_id} not found") }),
            );
        }
        Err(error) => return Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    };
    let Some(rules) = app_state.contest_rules.get(&log.contest_id) else {
        return Json(
            serde_json::json!({ "ok": false, "error": format!("unknown contest: {}", log.contest_id) }),
        );
    };
    let Some(serial_field) = rules.exchange.iter().find(|field| {
        field.is_sent
            && field.adif.eq_ignore_ascii_case(field_adif)
            && exchange_field_type_kind(&field.field_type) == "SERIAL"
    }) else {
        return Json(serde_json::json!({
            "ok": false,
            "error": format!("{} is not a sent serial field for contest {}", field_adif, rules.contest),
        }));
    };

    match app_state
        .db
        .allocate_serials(log_id, serial_field.adif.clone(), count)
        .await
    {
        Ok(allocation) => Json(serde_json::json!({ "ok": true, "allocation": allocation })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

fn exchange_field_type_kind(field_type: &str) -> String {
    field_type
        .split(':')
        .next()
        .unwrap_or("STRING")
        .trim()
        .to_uppercase()
}

#[derive(Debug, Default, serde::Deserialize)]
struct ContactsQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    callsign_prefix: Option<String>,
}

fn contacts_page(query: &ContactsQuery) -> Option<(usize, usize)> {
    if query.limit.is_none() && query.offset.is_none() {
        return None;
    }

    let limit = query
        .limit
        .unwrap_or(DEFAULT_CONTACTS_PAGE_LIMIT)
        .clamp(1, MAX_CONTACTS_PAGE_LIMIT);
    let offset = query.offset.unwrap_or(0);

    Some((limit, offset))
}

async fn contacts(
    State(app_state): State<AppState>,
    Path(log_id): Path<i64>,
    Query(query): Query<ContactsQuery>,
) -> Json<Vec<Contact>> {
    let (limit, offset) = contacts_page(&query).unwrap_or((usize::MAX, 0));
    let callsign_prefix = query
        .callsign_prefix
        .as_deref()
        .map(str::trim)
        .filter(|callsign_prefix| !callsign_prefix.is_empty())
        .map(str::to_uppercase);

    let contacts = match app_state
        .log_cache
        .contacts_display_page(log_id, offset, limit, callsign_prefix)
        .await
    {
        Ok(contacts) => contacts,
        Err(error) => {
            error!(log_id, %error, "failed to load contacts");
            Vec::new()
        }
    };

    let totals = app_state
        .incremental_scoring
        .totals(log_id)
        .unwrap_or_default();
    send_score_update(&app_state, log_id, &totals);
    Json(contacts)
}

async fn commit_contact(
    State(app_state): State<AppState>,
    Path(log_id): Path<i64>,
    Json(payload): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    debug!(log_id, payload = %pretty_json(&payload), "commit contact POST body");
    let input_contacts = match contacts_from_payload(payload) {
        Ok(contacts) => contacts,
        Err(error) => return Json(serde_json::json!({ "ok": false, "error": error })),
    };
    if let Err(error) = validation::validate_contacts(
        &app_state.db,
        &app_state.contest_rules,
        log_id,
        &input_contacts,
    )
    .await
    {
        return Json(serde_json::json!({ "ok": false, "error": error, "status": "failed" }));
    }

    let session_ids = input_contacts
        .iter()
        .map(contact_session_id)
        .collect::<Vec<_>>();

    match app_state
        .log_cache
        .upsert_contacts(log_id, input_contacts)
        .await
    {
        Ok(mut result) => {
            for (contact, session_id) in result.contacts.iter_mut().zip(session_ids) {
                if let Some(session_id) = session_id {
                    set_contact_meta(contact, "sessionId", serde_json::Value::String(session_id));
                }
                let _ = app_state.log_events.send(ServerMessage::LogEntry {
                    contact: contact.clone(),
                });
            }

            for contact in result.changed_contacts {
                let _ = app_state
                    .log_events
                    .send(ServerMessage::LogEntry { contact });
            }

            let totals = app_state
                .incremental_scoring
                .totals(log_id)
                .unwrap_or_default();
            send_score_update(&app_state, log_id, &totals);

            let contact = result.contacts.first().cloned();
            Json(serde_json::json!({
                "ok": true,
                "contact": contact,
                "contacts": result.contacts
            }))
        }
        Err(error) => {
            error!(log_id, %error, "failed to commit contacts");
            Json(serde_json::json!({ "ok": false, "error": error }))
        }
    }
}

async fn delete_contact(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<serde_json::Value> {
    match app_state.log_cache.delete_contact(id).await {
        Ok(Some(result)) => {
            let _ = app_state.log_events.send(ServerMessage::ContactDeleted {
                id,
                log_id: result.log_id,
            });
            for contact in result.changed_contacts {
                let _ = app_state
                    .log_events
                    .send(ServerMessage::LogEntry { contact });
            }

            let totals = app_state
                .incremental_scoring
                .totals(result.log_id)
                .unwrap_or_default();
            send_score_update(&app_state, result.log_id, &totals);
            Json(serde_json::json!({ "ok": true, "deleted": true }))
        }
        Ok(None) => Json(serde_json::json!({ "ok": true, "deleted": false })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error })),
    }
}

fn contact_session_id(contact: &Contact) -> Option<String> {
    contact_meta_value(contact, "sessionId")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn websocket_log_event_for_client(
    event: ServerMessage,
    outbound_session_id: &str,
    outbound_log_id: Option<i64>,
) -> Option<ServerMessage> {
    let outbound_log_id = outbound_log_id?;
    match event {
        ServerMessage::LogEntry { contact } => {
            if contact_log_id(&contact) != Some(outbound_log_id) {
                return None;
            }
            let contact_session_id =
                contact_meta_value(&contact, "sessionId").and_then(serde_json::Value::as_str);
            if contact_session_id == Some(outbound_session_id) {
                return None;
            }
            Some(ServerMessage::LogEntry { contact })
        }
        ServerMessage::ContactDeleted { id, log_id } if log_id == outbound_log_id => {
            Some(ServerMessage::ContactDeleted { id, log_id })
        }
        ServerMessage::ScoreUpdate {
            log_id,
            qso_count,
            multipliers,
            bonus_points,
            total_score,
        } if log_id == outbound_log_id => Some(ServerMessage::ScoreUpdate {
            log_id,
            qso_count,
            multipliers,
            bonus_points,
            total_score,
        }),
        _ => None,
    }
}

const REFERENCE_DATA_CACHE_CONTROL: &str = "private, max-age=86400";

fn cached_json_response<T: serde::Serialize>(request_headers: &HeaderMap, value: &T) -> Response {
    let body = serde_json::to_vec(value).expect("reference data should serialize");
    let etag = reference_data_etag(&body);
    let response = Response::builder()
        .header(header::CACHE_CONTROL, REFERENCE_DATA_CACHE_CONTROL)
        .header(header::ETAG, &etag);

    if if_none_match_matches(request_headers, &etag) {
        return response
            .status(StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .expect("not modified response should build");
    }

    response
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("json response should build")
}

fn if_none_match_matches(request_headers: &HeaderMap, etag: &str) -> bool {
    request_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .any(|candidate| candidate == "*" || candidate == etag)
        })
        .unwrap_or(false)
}

fn reference_data_etag(body: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    hasher.write(body);
    format!("W/\"{:x}-{}\"", hasher.finish(), body.len())
}

fn send_score_update(app_state: &AppState, log_id: i64, totals: &ScoreTotals) {
    let _ = app_state
        .log_events
        .send(score_update_message(log_id, totals));
}

fn score_update_message(log_id: i64, totals: &ScoreTotals) -> ServerMessage {
    ServerMessage::ScoreUpdate {
        log_id,
        qso_count: totals.qso_count,
        multipliers: totals.multipliers,
        bonus_points: totals.bonus_points,
        total_score: totals.score,
    }
}

fn contact_score_order(contact: &Contact) -> (i64, i64) {
    (contact_epoch(contact), contact_id(contact).unwrap_or(0))
}

fn contact_epoch(contact: &Contact) -> i64 {
    contact_adif_value(contact, "QSO_DATE_TIME_ON")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0)
}

fn contacts_from_payload(payload: serde_json::Value) -> Result<Vec<Contact>, String> {
    match payload {
        serde_json::Value::Array(values) => values
            .into_iter()
            .map(|value| match value {
                serde_json::Value::Object(contact) => Ok(contact),
                _ => Err("contact list must contain objects".to_string()),
            })
            .collect(),
        serde_json::Value::Object(contact) => Ok(vec![contact]),
        _ => Err("contacts payload must be an object or list of objects".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::build_contact;
    use axum::http::HeaderValue;
    use serde_json::{Map, json};

    fn committed_contact(log_id: i64, session_id: &str) -> Contact {
        build_contact(
            Map::from_iter([
                ("id".to_string(), json!(73)),
                ("logId".to_string(), json!(log_id)),
                ("status".to_string(), json!("Committed")),
                ("sessionId".to_string(), json!(session_id)),
            ]),
            Map::from_iter([
                ("CALL".to_string(), json!("K1ABC")),
                ("QSO_DATE_TIME_ON".to_string(), json!(1_700_000_000_i64)),
            ]),
        )
    }

    #[test]
    fn websocket_log_entry_is_only_sent_to_matching_log() {
        let event = ServerMessage::LogEntry {
            contact: committed_contact(1, "origin-session"),
        };

        assert!(websocket_log_event_for_client(event.clone(), "other-session", Some(1)).is_some());
        assert!(websocket_log_event_for_client(event, "other-session", Some(2)).is_none());
    }

    #[test]
    fn websocket_log_entry_skips_same_session_echoes() {
        let event = ServerMessage::LogEntry {
            contact: committed_contact(1, "origin-session"),
        };

        assert!(websocket_log_event_for_client(event, "origin-session", Some(1)).is_none());
    }

    #[test]
    fn websocket_contact_deleted_and_score_updates_are_log_scoped() {
        let deleted = ServerMessage::ContactDeleted { id: 55, log_id: 7 };
        assert!(websocket_log_event_for_client(deleted.clone(), "session", Some(7)).is_some());
        assert!(websocket_log_event_for_client(deleted, "session", Some(8)).is_none());

        let score = ServerMessage::ScoreUpdate {
            log_id: 7,
            qso_count: 10,
            multipliers: 3,
            bonus_points: 5,
            total_score: 35,
        };
        assert!(websocket_log_event_for_client(score.clone(), "session", Some(7)).is_some());
        assert!(websocket_log_event_for_client(score, "session", Some(8)).is_none());
    }

    #[test]
    fn cached_json_response_sets_private_cache_headers_and_etag() {
        let headers = HeaderMap::new();
        let response = cached_json_response(&headers, &json!({ "ok": true }));

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static(REFERENCE_DATA_CACHE_CONTROL))
        );
        assert!(response.headers().contains_key(header::ETAG));
    }

    #[test]
    fn cached_json_response_returns_not_modified_for_matching_etag() {
        let body = serde_json::to_vec(&json!({ "ok": true, "callsigns": ["K1ABC"] }))
            .expect("json should serialize");
        let etag = reference_data_etag(&body);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_str(&etag).expect("etag header should parse"),
        );

        let response =
            cached_json_response(&headers, &json!({ "ok": true, "callsigns": ["K1ABC"] }));

        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            response.headers().get(header::ETAG),
            Some(&HeaderValue::from_str(&etag).expect("etag header should parse"))
        );
    }

    #[test]
    fn auth_config_or_disabled_returns_loaded_auth_config() {
        let config = AuthConfig {
            login_user: "greg".to_string(),
            login_password: "hash".to_string(),
        };

        let loaded = auth_config_or_disabled(Ok(config.clone()));

        assert_eq!(loaded.login_user, config.login_user);
        assert_eq!(loaded.login_password, config.login_password);
    }

    #[test]
    fn auth_config_or_disabled_falls_back_to_disabled_auth() {
        let loaded = auth_config_or_disabled(Err(rusqlite::Error::InvalidQuery));

        assert!(loaded.login_user.is_empty());
        assert!(loaded.login_password.is_empty());
    }
}
