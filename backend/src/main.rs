mod bands;
mod frequency;
mod radio;
mod scqso_in_state;

use axum::{
    Json, Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use radio::{ClientMessage, RadioCommand, RadioSharedState, ServerMessage};
use scqso_in_state::ContestRules;
use std::{collections::HashMap, env, time::Duration};
use tokio::sync::{broadcast, mpsc};
use tower_http::cors::CorsLayer;

type Contact = serde_json::Map<String, serde_json::Value>;

#[derive(Clone)]
struct AppState {
    radio: RadioSharedState,
    log_entries: broadcast::Sender<Contact>,
}

#[derive(Debug)]
struct Config {
    rigctld_host: String,
    rigctld_port: u16,
    poll_interval: Duration,
}

#[tokio::main]
async fn main() {
    let config = Config::from_args();
    let (command_tx, command_rx) = mpsc::channel(32);
    let radio_state = RadioSharedState::new(command_tx);
    let (log_entries, _) = broadcast::channel(128);
    let app_state = AppState {
        radio: radio_state.clone(),
        log_entries,
    };

    tokio::spawn(radio::run_radio_task(
        config.rigctld_host.clone(),
        config.rigctld_port,
        config.poll_interval,
        radio_state,
        command_rx,
    ));

    let app = Router::new()
        .route("/contest-settings/get", get(contest_settings))
        .route("/contacts", get(contacts).post(commit_contact))
        .route("/ws", get(ws_handler))
        .with_state(app_state)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind backend to 0.0.0.0:8080");

    println!(
        "log73 backend listening on http://0.0.0.0:8080; rigctld at {}:{}; poll interval {:?}",
        config.rigctld_host, config.rigctld_port, config.poll_interval
    );
    axum::serve(listener, app).await.expect("server failed");
}

impl Config {
    fn from_args() -> Self {
        let mut rigctld_host = "127.0.0.1".to_string();
        let mut rigctld_port = 4532;
        let mut poll_interval = Duration::from_millis(250);
        let mut args = env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--rigctld-host" => {
                    rigctld_host = args.next().expect("--rigctld-host requires a host value");
                }
                "--rigctld-port" => {
                    rigctld_port = args
                        .next()
                        .expect("--rigctld-port requires a port value")
                        .parse()
                        .expect("--rigctld-port must be a number");
                }
                "--poll-frequency" | "--poll-interval" => {
                    let seconds: f64 = args
                        .next()
                        .expect("--poll-frequency requires a seconds value")
                        .parse()
                        .expect("--poll-frequency must be a number of seconds");
                    poll_interval = Duration::from_secs_f64(seconds);
                }
                "--help" | "-h" => {
                    println!(
                        "Usage: log73-backend [--rigctld-host HOST] [--rigctld-port PORT] [--poll-frequency SECONDS]"
                    );
                    std::process::exit(0);
                }
                _ => panic!("unknown argument: {arg}"),
            }
        }

        Self {
            rigctld_host,
            rigctld_port,
            poll_interval,
        }
    }
}

async fn ws_handler(
    State(app_state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let session_id = params.get("session_id").cloned().unwrap_or_default();
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, session_id))
}

async fn handle_socket(socket: WebSocket, app_state: AppState, session_id: String) {
    println!("backend websocket connected: session_id={session_id}");
    let (mut sender, mut receiver) = socket.split();

    if let Some(current) = app_state.radio.current().await {
        if sender
            .send(Message::Text(
                serde_json::to_string(&ServerMessage::RadioState(current))
                    .expect("radio state should serialize")
                    .into(),
            ))
            .await
            .is_err()
        {
            return;
        }
    }

    let mut radio_updates = app_state.radio.subscribe();
    let mut log_entries = app_state.log_entries.subscribe();
    let outbound_session_id = session_id.clone();
    let outbound = tokio::spawn(async move {
        loop {
            let message = tokio::select! {
                update = radio_updates.recv() => {
                    match update {
                        Ok(update) => serde_json::to_string(&ServerMessage::RadioState(update))
                            .expect("radio state should serialize"),
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                contact = log_entries.recv() => {
                    match contact {
                        Ok(contact) => {
                            let contact_session_id = contact
                                .get("_session_id")
                                .and_then(serde_json::Value::as_str);

                            if contact_session_id == Some(outbound_session_id.as_str()) {
                                continue;
                            }

                            serde_json::to_string(&ServerMessage::LogEntry { contact })
                                .expect("log entry should serialize")
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
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
                let _ = app_state
                    .radio
                    .send_command(RadioCommand::SetFrequency(frequency_hz))
                    .await;
            }
            Ok(ClientMessage::SetMode { mode }) => {
                let _ = app_state
                    .radio
                    .send_command(RadioCommand::SetMode(mode))
                    .await;
            }
            Err(error) => eprintln!("invalid websocket message: {error}"),
        }
    }

    outbound.abort();
    println!("backend websocket disconnected: session_id={session_id}");
}

async fn contest_settings() -> Json<ContestRules> {
    Json(ContestRules::new())
}

async fn contacts() -> Json<Vec<Contact>> {
    Json(Vec::new())
}

async fn commit_contact(
    State(app_state): State<AppState>,
    Json(mut contact): Json<Contact>,
) -> Json<serde_json::Value> {
    contact.insert(
        "_status".to_string(),
        serde_json::Value::String("Committed".to_string()),
    );

    println!(
        "received contact: {}",
        serde_json::to_string_pretty(&contact).expect("contact should serialize")
    );

    let _ = app_state.log_entries.send(contact.clone());
    Json(serde_json::json!({ "ok": true, "contact": contact }))
}
