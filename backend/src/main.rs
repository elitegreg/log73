mod auth;
mod bands;
mod contest_rules;
mod cw;
mod db;
mod frequency;
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
    http::{HeaderMap, Request, header},
    middleware,
    response::IntoResponse,
    routing::{delete, get, post},
};
use clap::Parser;
use contest_rules::{ContestRules, ContestRulesStore};
use db::{Contact, Database, NewLog, NewRadio, UpdateLog};
use futures_util::{SinkExt, StreamExt};
use radio::{ClientMessage, RadioCommand, ServerMessage};
use radio_manager::RadioManager;
use scoring::{ContestScoreTracker, ScoreTotals, ScoringModules};
use std::{
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    path::PathBuf,
    time::Duration,
};
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
    scoring_modules: ScoringModules,
    score_tracker: ContestScoreTracker,
    supercheckpartial: SuperCheckPartial,
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
    let db = Database::open("log73.db").expect("failed to open log73.db");
    let radio_manager = RadioManager::new(db.clone());
    let app_state = AppState {
        radio_manager,
        log_events,
        db,
        contest_rules,
        scoring_modules: ScoringModules::new(),
        score_tracker: ContestScoreTracker::new(),
        supercheckpartial,
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
        .route("/logs", get(logs).post(create_log))
        .route("/logs/{id}", get(log).put(update_log).delete(delete_log))
        .route(
            "/logs/{log_id}/contacts",
            get(contacts).post(commit_contact),
        )
        .route("/contacts/{id}", delete(delete_contact))
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
        && let Some(totals) = app_state.score_tracker.totals(log_id)
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

    let mut radio_updates = radio_handle.subscribe();
    let mut log_events = app_state.log_events.subscribe();
    let (direct_tx, mut direct_rx) = mpsc::channel::<ServerMessage>(32);
    let outbound_session_id = session_id.clone();
    let outbound = tokio::spawn(async move {
        loop {
            let message = tokio::select! {
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
                    Some(message) => serde_json::to_string(&message).expect("direct message should serialize"),
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
    if let Err(error) = validation::validate_update_log(&payload) {
        return Json(serde_json::json!({ "ok": false, "error": error }));
    }
    match app_state.db.update_log(id, payload).await {
        Ok(Some(log)) => Json(serde_json::json!({ "ok": true, "log": log })),
        Ok(None) => Json(serde_json::json!({ "ok": false, "error": "not found" })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn delete_log(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<serde_json::Value> {
    match app_state.db.delete_log(id).await {
        Ok(deleted) => Json(serde_json::json!({ "ok": true, "deleted": deleted })),
        Err(_) => {
            Json(serde_json::json!({ "ok": false, "error": "cannot delete a log that has QSOs" }))
        }
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
const TOKIO_YIELD_EVERY_ROWS: usize = 1000;

#[derive(Debug, Default, serde::Deserialize)]
struct ContactsQuery {
    limit: Option<usize>,
    offset: Option<usize>,
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
    if let Err(error) = ensure_score_tracker_for_log(&app_state, log_id).await {
        error!(log_id, %error, "failed to load contacts");
        return Json(Vec::new());
    }

    let contacts = match contacts_page(&query) {
        Some((limit, offset)) => app_state
            .score_tracker
            .contacts_display_page(log_id, offset, limit),
        None => app_state
            .score_tracker
            .contacts_display_page(log_id, 0, usize::MAX),
    };
    let totals = app_state.score_tracker.totals(log_id).unwrap_or_default();
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

    if let Err(error) = ensure_score_tracker_for_log(&app_state, log_id).await {
        warn!(log_id, %error, "unable to initialize score tracker before committing contact");
    }
    let old_contacts = input_contacts
        .iter()
        .map(|contact| {
            contact_id(contact).and_then(|id| app_state.score_tracker.contact(log_id, id))
        })
        .collect::<Vec<_>>();

    match app_state.db.upsert_contacts(log_id, input_contacts).await {
        Ok(mut contacts) => {
            let score_result =
                score_committed_contacts(&app_state, log_id, &mut contacts, &old_contacts).await;
            for (contact, session_id) in contacts.iter_mut().zip(session_ids) {
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
            for contact in score_result.changed_contacts {
                let _ = app_state
                    .log_events
                    .send(ServerMessage::LogEntry { contact });
            }
            send_score_update(&app_state, log_id, &score_result.totals);
            let contact = contacts.first().cloned();
            Json(serde_json::json!({ "ok": true, "contact": contact, "contacts": contacts }))
        }
        Err(error) => {
            error!(log_id, %error, "failed to commit contacts");
            Json(serde_json::json!({ "ok": false, "error": error.to_string() }))
        }
    }
}

async fn delete_contact(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<serde_json::Value> {
    match app_state.db.delete_contact(id).await {
        Ok(Some(log_id)) => {
            let old_contact = app_state.score_tracker.contact(log_id, id);
            let needs_full_rescore = old_contact
                .as_ref()
                .map(|contact| {
                    scored_contact_claimed_mult_or_bonus(contact)
                        || app_state
                            .score_tracker
                            .removing_contact_affects_dupes(log_id, contact)
                })
                .unwrap_or(true);
            let score_result = if needs_full_rescore {
                rescore_log_after_change(&app_state, log_id, &HashSet::new()).await
            } else {
                match app_state.score_tracker.delete_incremental(log_id, id) {
                    Some(totals) => ContactScoreResult {
                        totals,
                        changed_contacts: Vec::new(),
                    },
                    None => rescore_log_after_change(&app_state, log_id, &HashSet::new()).await,
                }
            };

            let _ = app_state
                .log_events
                .send(ServerMessage::ContactDeleted { id, log_id });
            for contact in score_result.changed_contacts {
                let _ = app_state
                    .log_events
                    .send(ServerMessage::LogEntry { contact });
            }
            send_score_update(&app_state, log_id, &score_result.totals);
            Json(serde_json::json!({ "ok": true, "deleted": true }))
        }
        Ok(None) => Json(serde_json::json!({ "ok": true, "deleted": false })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

fn contact_session_id(contact: &Contact) -> Option<String> {
    contact
        .get("_session_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

#[derive(Default)]
struct ScoredLogSnapshot {
    contacts: Vec<Contact>,
    totals: ScoreTotals,
}

#[derive(Default)]
struct ContactScoreResult {
    totals: ScoreTotals,
    changed_contacts: Vec<Contact>,
}

async fn scored_contacts_for_log(
    app_state: &AppState,
    log_id: i64,
) -> Result<ScoredLogSnapshot, String> {
    let log = app_state
        .db
        .log(log_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("log {log_id} not found"))?;
    let rules = app_state
        .contest_rules
        .get(&log.contest_id)
        .ok_or_else(|| format!("unknown contest: {}", log.contest_id))?;
    let mut contacts = app_state
        .db
        .contacts(log_id)
        .await
        .map_err(|error| error.to_string())?;

    yield_now_for_row_count(contacts.len()).await;

    contacts.sort_by_key(contact_score_order);
    let module = app_state
        .scoring_modules
        .get(rules, log.contest_params.clone());
    let totals = app_state
        .score_tracker
        .reset_log(log_id, module, &mut contacts);
    tokio::task::yield_now().await;
    contacts.sort_by_key(|contact| std::cmp::Reverse(contact_display_order(contact)));
    Ok(ScoredLogSnapshot { contacts, totals })
}

async fn ensure_score_tracker_for_log(app_state: &AppState, log_id: i64) -> Result<(), String> {
    if app_state.score_tracker.totals(log_id).is_some() {
        return Ok(());
    }

    scored_contacts_for_log(app_state, log_id).await.map(|_| ())
}

async fn score_committed_contacts(
    app_state: &AppState,
    log_id: i64,
    contacts: &mut [Contact],
    old_contacts: &[Option<Contact>],
) -> ContactScoreResult {
    if contacts.len() == 1 && old_contacts.len() == 1 {
        let contact = contacts[0].clone();
        let score_result = if let Some(old_contact) = &old_contacts[0] {
            let needs_full_rescore = scored_contact_claimed_mult_or_bonus(old_contact)
                || contact_score_order(old_contact) != contact_score_order(&contact)
                || !app_state.score_tracker.is_last_contact(log_id, old_contact)
                || app_state
                    .score_tracker
                    .removing_contact_affects_dupes(log_id, old_contact);

            if needs_full_rescore {
                None
            } else {
                app_state.score_tracker.replace_incremental(log_id, contact)
            }
        } else if app_state.score_tracker.can_append(log_id, &contact) {
            app_state.score_tracker.add_incremental(log_id, contact)
        } else {
            None
        };

        if let Some((scored_contact, totals)) = score_result {
            contacts[0] = scored_contact;
            return ContactScoreResult {
                totals,
                changed_contacts: Vec::new(),
            };
        }
    }

    let old_scored_contacts = app_state.score_tracker.contacts(log_id);
    let committed_contact_ids = contacts
        .iter()
        .filter_map(contact_id)
        .collect::<HashSet<_>>();
    let scored_log = match scored_contacts_for_log(app_state, log_id).await {
        Ok(scored_log) => scored_log,
        Err(error) => {
            error!(log_id, %error, "failed to rescore contacts after commit");
            return ContactScoreResult {
                totals: app_state.score_tracker.totals(log_id).unwrap_or_default(),
                changed_contacts: Vec::new(),
            };
        }
    };

    for (index, contact) in contacts.iter_mut().enumerate() {
        if let Some(scored_contact) = scored_contact_by_id(&scored_log.contacts, contact) {
            *contact = scored_contact;
        }
        yield_now_every_rows(index + 1).await;
    }

    ContactScoreResult {
        changed_contacts: scoring_changed_contacts(
            &old_scored_contacts,
            &scored_log.contacts,
            &committed_contact_ids,
        )
        .await,
        totals: scored_log.totals,
    }
}

async fn rescore_log_after_change(
    app_state: &AppState,
    log_id: i64,
    excluded_contact_ids: &HashSet<i64>,
) -> ContactScoreResult {
    let old_scored_contacts = app_state.score_tracker.contacts(log_id);
    match scored_contacts_for_log(app_state, log_id).await {
        Ok(scored_log) => ContactScoreResult {
            changed_contacts: scoring_changed_contacts(
                &old_scored_contacts,
                &scored_log.contacts,
                excluded_contact_ids,
            )
            .await,
            totals: scored_log.totals,
        },
        Err(error) => {
            error!(log_id, %error, "failed to rescore contacts after change");
            ContactScoreResult {
                totals: app_state.score_tracker.totals(log_id).unwrap_or_default(),
                changed_contacts: Vec::new(),
            }
        }
    }
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

fn scored_contact_by_id(scored_contacts: &[Contact], contact: &Contact) -> Option<Contact> {
    let id = contact_id(contact)?;
    scored_contacts
        .iter()
        .find(|scored_contact| contact_id(scored_contact) == Some(id))
        .cloned()
}

async fn scoring_changed_contacts(
    old_contacts: &[Contact],
    new_contacts: &[Contact],
    excluded_contact_ids: &HashSet<i64>,
) -> Vec<Contact> {
    let old_contacts_by_id = old_contacts
        .iter()
        .filter_map(|contact| contact_id(contact).map(|id| (id, contact)))
        .collect::<HashMap<_, _>>();

    let mut changed = Vec::new();
    for (index, contact) in new_contacts.iter().enumerate() {
        if let Some(id) = contact_id(contact)
            && !excluded_contact_ids.contains(&id)
            && old_contacts_by_id
                .get(&id)
                .map(|old_contact| scored_contact_values_changed(old_contact, contact))
                .unwrap_or(false)
        {
            changed.push(contact.clone());
        }
        yield_now_every_rows(index + 1).await;
    }
    changed
}

async fn yield_now_for_row_count(row_count: usize) {
    let mut processed = TOKIO_YIELD_EVERY_ROWS;
    while processed <= row_count {
        tokio::task::yield_now().await;
        processed += TOKIO_YIELD_EVERY_ROWS;
    }
}

async fn yield_now_every_rows(processed_rows: usize) {
    if processed_rows % TOKIO_YIELD_EVERY_ROWS == 0 {
        tokio::task::yield_now().await;
    }
}

fn scored_contact_values_changed(old_contact: &Contact, new_contact: &Contact) -> bool {
    ["_pts", "_mult", "_bonus", "_dupe"]
        .iter()
        .any(|field| old_contact.get(*field) != new_contact.get(*field))
}

fn scored_contact_claimed_mult_or_bonus(contact: &Contact) -> bool {
    scored_contact_value(contact, "_mult") > 0 || scored_contact_value(contact, "_bonus") > 0
}

fn scored_contact_value(contact: &Contact, field: &str) -> i64 {
    contact
        .get(field)
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0)
}

fn contact_score_order(contact: &Contact) -> (i64, i64) {
    (contact_epoch(contact), contact_id(contact).unwrap_or(0))
}

fn contact_display_order(contact: &Contact) -> (i64, i64) {
    contact_score_order(contact)
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
