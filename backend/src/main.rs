mod adif;
mod auth;
mod bands;
mod cabrillo;
mod cat_keyer;
mod contest_rules;
mod cw;
mod db;
mod dxcc;
mod log_cache;
mod radio;
mod radio_manager;
mod scoring;
mod static_assets;
mod supercheckpartial;
mod validation;

use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    middleware,
    response::IntoResponse,
    routing::{delete, get, post},
};
use clap::Parser;
use contest_rules::{ContestRules, ContestRulesStore};
use db::{Contact, Database, NewLog, NewRadio, UpdateLog};
use futures_util::{SinkExt, StreamExt};
use log_cache::LogCache;
use radio::{ClientMessage, RadioCommand, ServerMessage};
use radio_cat_rs::supported_radio_kinds;
use radio_manager::RadioManager;
use scoring::{IncrementalScoreTracker, ScoreTotals, ScoringModules, score_contacts};
use std::{collections::HashMap, fs::OpenOptions, path::PathBuf, time::Duration};
use supercheckpartial::SuperCheckPartial;
use tokio::sync::{broadcast, mpsc, oneshot};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{Span, debug, error, info, warn};
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Clone)]
struct AppState {
    radio_manager: RadioManager,
    log_events: broadcast::Sender<ServerMessage>,
    db: Database,
    contest_rules: ContestRulesStore,
    log_cache: LogCache,
    incremental_scoring: IncrementalScoreTracker,
    supercheckpartial: SuperCheckPartial,
    dxcc: std::sync::Arc<dxcc::DxccDatabase>,
}

const MAX_CLIENT_ERROR_TEXT_LENGTH: usize = 4096;
const MAX_CLIENT_ERROR_JSON_LENGTH: usize = 8192;

fn init_tracing(cli: &Cli) -> std::io::Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    let filter = EnvFilter::try_new(&cli.log_level).unwrap_or_else(|_| EnvFilter::new("info"));

    if let Some(path) = &cli.log_file {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let (writer, guard) = tracing_appender::non_blocking(file);
        fmt().with_env_filter(filter).with_writer(writer).init();
        return Ok(Some(guard));
    }

    fmt().with_env_filter(filter).init();
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

    #[arg(long, default_value = "../contest-rules")]
    contest_rules_dir: PathBuf,

    #[arg(long, default_value = "../data")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let _log_guard = init_tracing(&cli).expect("failed to initialize logging");

    let (log_events, _) = broadcast::channel(128);
    let contest_rules = ContestRulesStore::load_dir(&cli.contest_rules_dir)
        .unwrap_or_else(|error| panic!("failed to load contest rules: {error}"));
    let supercheckpartial = SuperCheckPartial::load_dir(&cli.data_dir).unwrap_or_else(|error| {
        warn!(
            data_dir = %cli.data_dir.display(),
            %error,
            "failed to load MASTER.SCP; supercheckpartial matches will be unavailable"
        );
        SuperCheckPartial::default()
    });
    info!(
        callsigns = supercheckpartial.len(),
        data_dir = %cli.data_dir.display(),
        "loaded supercheckpartial callsigns"
    );
    let dxcc = dxcc::DxccDatabase::load_dir(&cli.data_dir).unwrap_or_else(|error| {
        warn!(
            data_dir = %cli.data_dir.display(),
            %error,
            "failed to load cty.dat; DXCC lookup will be unavailable"
        );
        dxcc::DxccDatabase::default()
    });
    info!(
        entities = dxcc.entity_count(),
        rules = dxcc.rule_count(),
        data_dir = %cli.data_dir.display(),
        "loaded DXCC country data"
    );
    let db = Database::open("log73.db").expect("failed to open log73.db");
    let radio_manager = RadioManager::new(db.clone());
    let scoring_modules = ScoringModules::new();
    let incremental_scoring = IncrementalScoreTracker::new();
    let log_cache = LogCache::new(db.clone(), contest_rules.clone(), scoring_modules.clone());
    log_cache.register_processor(std::sync::Arc::new(incremental_scoring.clone()));

    let app_state = AppState {
        radio_manager,
        log_events,
        db,
        contest_rules,
        log_cache,
        incremental_scoring,
        supercheckpartial,
        dxcc: std::sync::Arc::new(dxcc),
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
        .route("/logs", get(logs).post(create_log))
        .route("/logs/{id}", get(log).put(update_log).delete(delete_log))
        .route("/logs/{id}/qso-count", get(log_qso_count))
        .route("/logs/{id}/adif", post(export_adif))
        .route("/logs/{id}/cabrillo", post(export_cabrillo))
        .route(
            "/logs/{log_id}/contacts",
            get(contacts).post(commit_contact),
        )
        .route("/contacts/{id}", delete(delete_contact))
        .route("/radio-kinds", get(radio_kinds))
        .route("/radios", get(radios).post(create_radio))
        .route(
            "/radios/{id}",
            get(radio).put(update_radio).delete(delete_radio),
        )
        .route("/radios/{id}/cw-labels", get(cw_labels));

    let app = Router::new()
        .nest("/api", api)
        .route("/ws", get(ws_handler))
        .fallback(static_assets::static_handler)
        .with_state(app_state.clone())
        .layer(middleware::from_fn_with_state(app_state, auth::basic_auth))
        .layer(request_trace_layer)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(&cli.bind)
        .await
        .unwrap_or_else(|error| panic!("failed to bind backend to {}: {error}", cli.bind));

    info!(
        address = %cli.bind,
        "log73 backend listening; radio connections are lazy"
    );
    axum::serve(listener, app).await.expect("server failed");
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
    let outbound_session_id = session_id.clone();
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
                    Ok(ServerMessage::LogEntry { contact }) => {
                        let contact_session_id = contact.get("_session_id").and_then(serde_json::Value::as_str);
                        if contact_session_id == Some(outbound_session_id.as_str()) { continue; }
                        serde_json::to_string(&ServerMessage::LogEntry { contact }).expect("log entry should serialize")
                    }
                    Ok(event) => serde_json::to_string(&event).expect("log event should serialize"),
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
            Ok(ClientMessage::SendCw {
                request_id,
                mode,
                key,
                fields,
            }) => {
                debug!(
                    session_id,
                    radio_id, request_id, mode, key, "websocket send_cw command received"
                );
                if let Err(error) =
                    validation::validate_cw_request(&request_id, &mode, &key, &fields)
                {
                    warn!(session_id, radio_id, request_id, mode, key, %error, "invalid websocket send_cw command");
                    continue;
                }
                let (completed_tx, completed_rx) = oneshot::channel();
                let command_result = radio_handle
                    .send_command(RadioCommand::SendCw {
                        mode,
                        key,
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
                            "waiting for cw send completion"
                        );
                        match completed_rx.await {
                            Ok(Ok(())) => {
                                debug!(
                                    session_id = %completion_session_id,
                                    request_id,
                                    "cw send complete; sending cw_sent websocket message"
                                );
                                if direct_tx
                                    .send(ServerMessage::CwSent { request_id })
                                    .await
                                    .is_err()
                                {
                                    debug!(
                                        session_id = %completion_session_id,
                                        "unable to send cw_sent websocket message; session closed"
                                    );
                                }
                            }
                            Ok(Err(error)) => {
                                debug!(
                                    session_id = %completion_session_id,
                                    request_id,
                                    %error,
                                    "cw send did not complete; not sending cw_sent websocket message"
                                );
                            }
                            Err(error) => {
                                debug!(
                                    session_id = %completion_session_id,
                                    request_id,
                                    %error,
                                    "cw completion channel closed; not sending cw_sent websocket message"
                                );
                            }
                        }
                    });
                } else {
                    debug!(
                        session_id,
                        radio_id, request_id, "failed to queue cw command"
                    );
                }
            }
            Ok(ClientMessage::SendCwText { request_id, text }) => {
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
                                    "cw text send complete; sending cw_sent websocket message"
                                );
                                if direct_tx
                                    .send(ServerMessage::CwSent { request_id })
                                    .await
                                    .is_err()
                                {
                                    debug!(
                                        session_id = %completion_session_id,
                                        "unable to send cw_sent websocket message; session closed"
                                    );
                                }
                            }
                            Ok(Err(error)) => {
                                debug!(
                                    session_id = %completion_session_id,
                                    request_id,
                                    %error,
                                    "cw text send did not complete; not sending cw_sent websocket message"
                                );
                            }
                            Err(error) => {
                                debug!(
                                    session_id = %completion_session_id,
                                    request_id,
                                    %error,
                                    "cw text completion channel closed; not sending cw_sent websocket message"
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
            Ok(ClientMessage::StopCw) => {
                debug!(session_id, radio_id, "websocket stop_cw command received");
                let _ = radio_handle.send_command(RadioCommand::StopCw).await;
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

    outbound.abort();
    app_state.radio_manager.release(radio_id).await;
    info!(session_id, radio_id, "backend websocket disconnected");
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

async fn supercheckpartial_matches(State(app_state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "callsigns": app_state.supercheckpartial.callsigns() }))
}

async fn dxcc_data(State(app_state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "dxcc": app_state.dxcc.as_ref() }))
}

async fn config(State(app_state): State<AppState>) -> Json<serde_json::Value> {
    match app_state.db.auth_config_view().await {
        Ok(config) => Json(serde_json::json!({ "ok": true, "config": config })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct UpdateConfigPayload {
    login_user: String,
    login_password: String,
    login_password_confirm: String,
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
    if let Err(error) = validation::validate_auth_config(
        &payload.login_user,
        &payload.login_password,
        &payload.login_password_confirm,
    ) {
        return Json(serde_json::json!({ "ok": false, "error": error }));
    }

    let login_password = match auth::hash_password(&payload.login_password) {
        Ok(login_password) => login_password,
        Err(error) => return Json(serde_json::json!({ "ok": false, "error": error })),
    };

    match app_state
        .db
        .update_auth_config(db::UpdateAuthConfig {
            login_user: payload.login_user,
            login_password,
        })
        .await
    {
        Ok(()) => match app_state.db.auth_config_view().await {
            Ok(config) => Json(serde_json::json!({ "ok": true, "config": config })),
            Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
        },
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
        Ok(Some(log)) => Json(serde_json::json!({ "ok": true, "log": log })),
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

async fn radios(State(app_state): State<AppState>) -> Json<Vec<db::RadioConfig>> {
    match app_state.db.radios().await {
        Ok(radios) => Json(radios),
        Err(error) => {
            error!(%error, "failed to load radios");
            Json(Vec::new())
        }
    }
}

async fn radio_kinds() -> Json<Vec<String>> {
    Json(
        supported_radio_kinds()
            .iter()
            .map(|kind| kind.display_name())
            .collect(),
    )
}

async fn radio(State(app_state): State<AppState>, Path(id): Path<i64>) -> Json<serde_json::Value> {
    match app_state.db.radio(id).await {
        Ok(Some(radio)) => Json(serde_json::json!({ "ok": true, "radio": radio })),
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

async fn create_radio(
    State(app_state): State<AppState>,
    Json(payload): Json<NewRadio>,
) -> Json<serde_json::Value> {
    debug!(payload = %pretty_json(&payload), "create radio POST body");
    if let Err(error) = validation::validate_radio(&payload) {
        return Json(serde_json::json!({ "ok": false, "error": error }));
    }
    match app_state.db.create_radio(payload).await {
        Ok(radio) => Json(serde_json::json!({ "ok": true, "radio": radio })),
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
    match app_state.db.update_radio(id, payload).await {
        Ok(Some(radio)) => {
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
        Ok(deleted) => Json(serde_json::json!({ "ok": true, "deleted": deleted })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

const DEFAULT_CONTACTS_PAGE_LIMIT: usize = 200;
const MAX_CONTACTS_PAGE_LIMIT: usize = 1000;

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
                    contact.insert(
                        "_session_id".to_string(),
                        serde_json::Value::String(session_id),
                    );
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
    contact
        .get("_session_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
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
    contact
        .get("QSO_DATE_TIME_ON")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0)
}

fn contact_id(contact: &Contact) -> Option<i64> {
    contact
        .get("_id")
        .or_else(|| contact.get("ID"))
        .and_then(serde_json::Value::as_i64)
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
