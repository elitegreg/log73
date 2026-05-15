mod bands;
mod frequency;
mod radio;
mod scqso_in_state;

use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use radio::{ClientMessage, RadioCommand, RadioSharedState, ServerMessage};
use scqso_in_state::ContestRules;
use std::{env, time::Duration};
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;

type Contact = serde_json::Map<String, serde_json::Value>;

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

    tokio::spawn(radio::run_radio_task(
        config.rigctld_host.clone(),
        config.rigctld_port,
        config.poll_interval,
        radio_state.clone(),
        command_rx,
    ));

    let app = Router::new()
        .route("/contest-settings/get", get(contest_settings))
        .route("/contacts", get(contacts).post(commit_contact))
        .route("/ws", get(ws_handler))
        .with_state(radio_state)
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
    State(radio_state): State<RadioSharedState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, radio_state))
}

async fn handle_socket(socket: WebSocket, radio_state: RadioSharedState) {
    let (mut sender, mut receiver) = socket.split();

    if let Some(current) = radio_state.current().await {
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

    let mut updates = radio_state.subscribe();
    let outbound = tokio::spawn(async move {
        while let Ok(update) = updates.recv().await {
            let message = serde_json::to_string(&ServerMessage::RadioState(update))
                .expect("radio state should serialize");

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
                let _ = radio_state
                    .send_command(RadioCommand::SetFrequency(frequency_hz))
                    .await;
            }
            Ok(ClientMessage::SetMode { mode }) => {
                let _ = radio_state.send_command(RadioCommand::SetMode(mode)).await;
            }
            Err(error) => eprintln!("invalid websocket message: {error}"),
        }
    }

    outbound.abort();
}

async fn contest_settings() -> Json<ContestRules> {
    Json(ContestRules::new())
}

async fn contacts() -> Json<Vec<Contact>> {
    Json(Vec::new())
}

async fn commit_contact(Json(contact): Json<Contact>) -> Json<serde_json::Value> {
    println!(
        "received contact: {}",
        serde_json::to_string_pretty(&contact).expect("contact should serialize")
    );
    Json(serde_json::json!({ "ok": true }))
}
