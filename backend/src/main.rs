mod auth;
mod bands;
mod db;
mod frequency;
mod radio;
mod radio_manager;
mod scqso_in_state;
mod static_assets;

use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, Request, header},
    middleware,
    response::IntoResponse,
    routing::{delete, get},
};
use clap::Parser;
use db::{Contact, Database, NewLog, NewRadio};
use futures_util::{SinkExt, StreamExt};
use radio::{ClientMessage, RadioCommand, ServerMessage};
use radio_manager::RadioManager;
use scqso_in_state::ContestRules;
use std::{collections::HashMap, fs::OpenOptions, path::PathBuf, time::Duration};
use tokio::sync::broadcast;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{Span, debug, error, info, warn};
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Clone)]
struct AppState {
    radio_manager: RadioManager,
    log_entries: broadcast::Sender<Contact>,
    db: Database,
}

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
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let _log_guard = init_tracing(&cli).expect("failed to initialize logging");

    let (log_entries, _) = broadcast::channel(128);
    let db = Database::open("log73.db").expect("failed to open log73.db");
    let radio_manager = RadioManager::new(db.clone());
    let app_state = AppState {
        radio_manager,
        log_entries,
        db,
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
        .route("/contest-settings", get(contest_settings))
        .route("/logs", get(logs).post(create_log))
        .route("/logs/{id}", get(log).delete(delete_log))
        .route(
            "/logs/{log_id}/contacts",
            get(contacts).post(commit_contact),
        )
        .route("/contacts/{id}", delete(delete_contact))
        .route("/radios", get(radios).post(create_radio))
        .route("/radios/{id}", get(radio).delete(delete_radio));

    let app = Router::new()
        .nest("/api", api)
        .route("/ws", get(ws_handler))
        .fallback(static_assets::static_handler)
        .with_state(app_state)
        .layer(middleware::from_fn(auth::basic_auth))
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
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, session_id, radio_id))
}

async fn handle_socket(
    socket: WebSocket,
    app_state: AppState,
    session_id: String,
    radio_id: Option<i64>,
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

    if let Some(current) = radio_handle.current_message().await {
        if sender
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
    }

    let mut radio_updates = radio_handle.subscribe();
    let mut log_entries = app_state.log_entries.subscribe();
    let outbound_session_id = session_id.clone();
    let outbound = tokio::spawn(async move {
        loop {
            let message = tokio::select! {
                update = radio_updates.recv() => match update {
                    Ok(update) => serde_json::to_string(&ServerMessage::RadioState(update)).expect("radio state should serialize"),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                contact = log_entries.recv() => match contact {
                    Ok(contact) => {
                        let contact_session_id = contact.get("_session_id").and_then(serde_json::Value::as_str);
                        if contact_session_id == Some(outbound_session_id.as_str()) { continue; }
                        serde_json::to_string(&ServerMessage::LogEntry { contact }).expect("log entry should serialize")
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
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
                let _ = radio_handle
                    .send_command(RadioCommand::SetFrequency(frequency_hz))
                    .await;
            }
            Ok(ClientMessage::SetMode { mode }) => {
                debug!(
                    session_id,
                    radio_id, mode, "websocket set_mode command received"
                );
                let _ = radio_handle.send_command(RadioCommand::SetMode(mode)).await;
            }
            Err(error) => warn!(session_id, radio_id, %error, "invalid websocket message"),
        }
    }

    outbound.abort();
    app_state.radio_manager.release(radio_id).await;
    info!(session_id, radio_id, "backend websocket disconnected");
}

async fn contest_settings() -> Json<ContestRules> {
    Json(ContestRules::new())
}

async fn logs(State(app_state): State<AppState>) -> Json<Vec<db::Log>> {
    Json(app_state.db.logs().unwrap_or_else(|error| {
        error!(%error, "failed to load logs");
        Vec::new()
    }))
}

async fn log(State(app_state): State<AppState>, Path(id): Path<i64>) -> Json<serde_json::Value> {
    match app_state.db.log(id) {
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
    match app_state.db.create_log(payload) {
        Ok(log) => Json(serde_json::json!({ "ok": true, "log": log })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn delete_log(
    State(app_state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<serde_json::Value> {
    match app_state.db.delete_log(id) {
        Ok(deleted) => Json(serde_json::json!({ "ok": true, "deleted": deleted })),
        Err(_) => {
            Json(serde_json::json!({ "ok": false, "error": "cannot delete a log that has QSOs" }))
        }
    }
}

async fn radios(State(app_state): State<AppState>) -> Json<Vec<db::RadioConfig>> {
    Json(app_state.db.radios().unwrap_or_else(|error| {
        error!(%error, "failed to load radios");
        Vec::new()
    }))
}

async fn radio(State(app_state): State<AppState>, Path(id): Path<i64>) -> Json<serde_json::Value> {
    match app_state.db.radio(id) {
        Ok(Some(radio)) => Json(serde_json::json!({ "ok": true, "radio": radio })),
        Ok(None) => Json(serde_json::json!({ "ok": false, "error": "not found" })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn create_radio(
    State(app_state): State<AppState>,
    Json(payload): Json<NewRadio>,
) -> Json<serde_json::Value> {
    debug!(payload = %pretty_json(&payload), "create radio POST body");
    match app_state.db.create_radio(payload) {
        Ok(radio) => Json(serde_json::json!({ "ok": true, "radio": radio })),
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

    match app_state.db.delete_radio(id) {
        Ok(deleted) => Json(serde_json::json!({ "ok": true, "deleted": deleted })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

async fn contacts(
    State(app_state): State<AppState>,
    Path(log_id): Path<i64>,
) -> Json<Vec<Contact>> {
    match app_state.db.contacts(log_id) {
        Ok(contacts) => Json(contacts),
        Err(error) => {
            error!(log_id, %error, "failed to load contacts");
            Json(Vec::new())
        }
    }
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
    let session_ids = input_contacts
        .iter()
        .map(contact_session_id)
        .collect::<Vec<_>>();

    match app_state.db.upsert_contacts(log_id, input_contacts) {
        Ok(mut contacts) => {
            for (contact, session_id) in contacts.iter_mut().zip(session_ids) {
                if let Some(session_id) = session_id {
                    contact.insert(
                        "_session_id".to_string(),
                        serde_json::Value::String(session_id),
                    );
                }
                let _ = app_state.log_entries.send(contact.clone());
            }
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
    match app_state.db.delete_contact(id) {
        Ok(deleted) => Json(serde_json::json!({ "ok": true, "deleted": deleted })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error.to_string() })),
    }
}

fn contact_session_id(contact: &Contact) -> Option<String> {
    contact
        .get("_session_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
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
