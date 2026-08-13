use crate::{
    RadioClientMessage, RadioCommand, RadioConfig, RadioIoConfig, RadioManager, RadioServerMessage,
    is_valid_message_mode, modes,
};
use axum::{
    Extension,
    extract::{
        Query,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{HashMap, hash_map::Entry},
    fmt,
    sync::{Arc, Mutex, MutexGuard},
};
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{info, warn};

const MAX_RADIO_FREQUENCY_HZ: u64 = 500_000_000;
const MAX_RIT_OFFSET_HZ: i32 = 9_999;
const MIN_CW_WPM: u8 = 5;
const MAX_CW_WPM: u8 = 60;
const MAX_REQUEST_ID_LEN: usize = 64;
const MAX_CW_TEXT_LEN: usize = 256;
const MAX_MESSAGE_FIELDS: usize = 100;
const MAX_FIELD_NAME_LEN: usize = 64;
const MAX_FIELD_STRING_LEN: usize = 1024;
const MAX_FIELD_ARRAY_ITEMS: usize = 100;
const MAX_FIELD_OBJECT_ENTRIES: usize = 100;
const MAX_FIELD_JSON_DEPTH: usize = 4;

#[derive(Clone)]
pub struct RadioWebSocketState {
    radio_manager: RadioManager,
    registry: Arc<Mutex<RadioRegistry>>,
}

impl RadioWebSocketState {
    pub fn new(radio_manager: RadioManager, config: RadioIoConfig) -> Self {
        let radios = config
            .radios
            .into_iter()
            .map(|config| {
                (
                    config.id,
                    RegisteredRadio {
                        config,
                        users: 0,
                        mutation_reserved: false,
                    },
                )
            })
            .collect();
        Self {
            radio_manager,
            registry: Arc::new(Mutex::new(RadioRegistry { radios })),
        }
    }

    pub fn add_radio(&self, config: RadioConfig) -> Result<(), String> {
        let mut registry = lock_registry(&self.registry);
        match registry.radios.entry(config.id) {
            Entry::Vacant(entry) => {
                entry.insert(RegisteredRadio {
                    config,
                    users: 0,
                    mutation_reserved: false,
                });
                Ok(())
            }
            Entry::Occupied(_) => Err(format!("radio {} is already configured", config.id)),
        }
    }

    pub fn begin_mutation(&self, radio_id: i64) -> Result<RadioMutationPermit, RadioMutationError> {
        let mut registry = lock_registry(&self.registry);
        let radio = registry
            .radios
            .get_mut(&radio_id)
            .ok_or(RadioMutationError::NotFound { radio_id })?;
        if radio.users > 0 {
            return Err(RadioMutationError::InUse { radio_id });
        }
        if radio.mutation_reserved {
            return Err(RadioMutationError::MutationInProgress { radio_id });
        }
        radio.mutation_reserved = true;
        Ok(RadioMutationPermit {
            registry: self.registry.clone(),
            radio_id,
            finished: false,
        })
    }

    pub async fn acquire(&self, radio_id: i64) -> Result<AcquiredRadio, RadioAcquireError> {
        let config = {
            let mut registry = lock_registry(&self.registry);
            let radio = registry
                .radios
                .get_mut(&radio_id)
                .ok_or(RadioAcquireError::NotFound { radio_id })?;
            if radio.mutation_reserved {
                return Err(RadioAcquireError::MutationInProgress { radio_id });
            }
            radio.users += 1;
            radio.config.clone()
        };

        match self.radio_manager.acquire(config.clone()).await {
            Ok(handle) => Ok(AcquiredRadio { config, handle }),
            Err(message) => {
                self.finish_use(radio_id);
                Err(RadioAcquireError::Unavailable { radio_id, message })
            }
        }
    }

    pub async fn release(&self, radio_id: i64) {
        self.radio_manager.release(radio_id).await;
        self.finish_use(radio_id);
    }

    fn contains_radio(&self, radio_id: i64) -> bool {
        lock_registry(&self.registry).radios.contains_key(&radio_id)
    }

    fn finish_use(&self, radio_id: i64) {
        let mut registry = lock_registry(&self.registry);
        if let Some(radio) = registry.radios.get_mut(&radio_id) {
            radio.users = radio.users.saturating_sub(1);
        }
    }
}

pub struct AcquiredRadio {
    pub config: RadioConfig,
    pub handle: crate::RadioHandle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RadioAcquireError {
    NotFound { radio_id: i64 },
    MutationInProgress { radio_id: i64 },
    Unavailable { radio_id: i64, message: String },
}

impl fmt::Display for RadioAcquireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { radio_id } => write!(formatter, "radio {radio_id} not found"),
            Self::MutationInProgress { radio_id } => {
                write!(formatter, "radio {radio_id} is being changed")
            }
            Self::Unavailable { radio_id, message } => {
                write!(formatter, "radio {radio_id} is unavailable: {message}")
            }
        }
    }
}

impl std::error::Error for RadioAcquireError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RadioMutationError {
    NotFound { radio_id: i64 },
    InUse { radio_id: i64 },
    MutationInProgress { radio_id: i64 },
}

impl fmt::Display for RadioMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { radio_id } => write!(formatter, "radio {radio_id} not found"),
            Self::InUse { radio_id } => write!(formatter, "radio {radio_id} is in use"),
            Self::MutationInProgress { radio_id } => {
                write!(formatter, "radio {radio_id} is already being changed")
            }
        }
    }
}

impl std::error::Error for RadioMutationError {}

pub struct RadioMutationPermit {
    registry: Arc<Mutex<RadioRegistry>>,
    radio_id: i64,
    finished: bool,
}

impl RadioMutationPermit {
    pub fn commit_update(mut self, config: RadioConfig) -> Result<(), String> {
        if config.id != self.radio_id {
            return Err(format!(
                "radio config id {} does not match reserved radio {}",
                config.id, self.radio_id
            ));
        }
        let mut registry = lock_registry(&self.registry);
        let radio = registry
            .radios
            .get_mut(&self.radio_id)
            .ok_or_else(|| format!("radio {} not found", self.radio_id))?;
        radio.config = config;
        radio.mutation_reserved = false;
        self.finished = true;
        Ok(())
    }

    pub fn commit_delete(mut self) -> Result<(), String> {
        let mut registry = lock_registry(&self.registry);
        registry
            .radios
            .remove(&self.radio_id)
            .ok_or_else(|| format!("radio {} not found", self.radio_id))?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for RadioMutationPermit {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Some(radio) = lock_registry(&self.registry).radios.get_mut(&self.radio_id) {
            radio.mutation_reserved = false;
        }
    }
}

#[derive(Default)]
struct RadioRegistry {
    radios: HashMap<i64, RegisteredRadio>,
}

struct RegisteredRadio {
    config: RadioConfig,
    users: usize,
    mutation_reserved: bool,
}

fn lock_registry(registry: &Mutex<RadioRegistry>) -> MutexGuard<'_, RadioRegistry> {
    registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, Deserialize)]
pub struct RadioWsQuery {
    radio_id: i64,
}

pub async fn radio_ws_handler(
    Extension(state): Extension<RadioWebSocketState>,
    Query(query): Query<RadioWsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if !state.contains_radio(query.radio_id) {
        return (
            StatusCode::NOT_FOUND,
            format!("radio {} not found", query.radio_id),
        )
            .into_response();
    }

    ws.on_upgrade(move |socket| handle_radio_socket(socket, state, query.radio_id))
        .into_response()
}

async fn handle_radio_socket(socket: WebSocket, state: RadioWebSocketState, radio_id: i64) {
    let acquired = match state.acquire(radio_id).await {
        Ok(acquired) => acquired,
        Err(error) => {
            warn!(radio_id, %error, "radio websocket could not acquire radio");
            return;
        }
    };
    let radio_handle = acquired.handle;

    info!(radio_id, "radio websocket connected");
    let (mut sender, mut receiver) = socket.split();
    let current_status = RadioServerMessage::RadioStatus(radio_handle.current_status().await);
    if send_radio_ws_message(&mut sender, &current_status)
        .await
        .is_err()
    {
        state.release(radio_id).await;
        return;
    }
    if let Some(current) = radio_handle.current_state().await {
        let current = RadioServerMessage::RadioState(current);
        if send_radio_ws_message(&mut sender, &current).await.is_err() {
            state.release(radio_id).await;
            return;
        }
    }

    let mut status_updates = radio_handle.subscribe_status();
    let mut state_updates = radio_handle.subscribe();
    let (direct_tx, mut direct_rx) = mpsc::channel::<RadioServerMessage>(32);
    let outbound = tokio::spawn(async move {
        loop {
            let message = tokio::select! {
                status = status_updates.recv() => match status {
                    Ok(status) => RadioServerMessage::RadioStatus(status),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                state = state_updates.recv() => match state {
                    Ok(state) => RadioServerMessage::RadioState(state),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                direct = direct_rx.recv() => match direct {
                    Some(message) => message,
                    None => break,
                },
            };
            if send_radio_ws_message(&mut sender, &message).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = receiver.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        match serde_json::from_str::<RadioClientMessage>(&text) {
            Ok(RadioClientMessage::Ping { request_id }) => {
                if direct_tx
                    .send(RadioServerMessage::Pong { request_id })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Ok(RadioClientMessage::SetFrequency { frequency_hz }) => {
                if let Err(error) = validate_radio_frequency_hz(frequency_hz) {
                    warn!(radio_id, frequency_hz, %error, "invalid radio websocket set_frequency command");
                    continue;
                }
                let _ = radio_handle
                    .send_command(RadioCommand::SetFrequency(frequency_hz))
                    .await;
            }
            Ok(RadioClientMessage::SetMode { mode }) => {
                if let Err(error) = validate_radio_mode(&mode) {
                    warn!(radio_id, mode, %error, "invalid radio websocket set_mode command");
                    continue;
                }
                let _ = radio_handle.send_command(RadioCommand::SetMode(mode)).await;
            }
            Ok(RadioClientMessage::RitClear) => {
                let _ = radio_handle.send_command(RadioCommand::RitClear).await;
            }
            Ok(RadioClientMessage::RitIncrement { hz }) => {
                if let Err(error) = validate_rit_adjustment_hz(hz) {
                    warn!(radio_id, hz, %error, "invalid radio websocket rit_increment command");
                    continue;
                }
                let _ = radio_handle
                    .send_command(RadioCommand::RitIncrement(hz))
                    .await;
            }
            Ok(RadioClientMessage::RitDecrement { hz }) => {
                if let Err(error) = validate_rit_adjustment_hz(hz) {
                    warn!(radio_id, hz, %error, "invalid radio websocket rit_decrement command");
                    continue;
                }
                let _ = radio_handle
                    .send_command(RadioCommand::RitDecrement(hz))
                    .await;
            }
            Ok(RadioClientMessage::SendMessage {
                request_id,
                mode,
                keys,
                fields,
            }) => {
                if let Err(error) = validate_message_request(&request_id, &mode, &keys, &fields) {
                    warn!(radio_id, request_id, mode, ?keys, %error, "invalid radio websocket send_message command");
                    continue;
                }
                let (completed, result) = oneshot::channel();
                if radio_handle
                    .send_command(RadioCommand::SendMessage {
                        mode,
                        keys,
                        fields,
                        completed,
                    })
                    .await
                    .is_ok()
                {
                    spawn_radio_message_completion(direct_tx.clone(), request_id, result);
                }
            }
            Ok(RadioClientMessage::SendCwText {
                request_id,
                text,
                wait_for_completion,
            }) => {
                if let Err(error) = validate_cw_text_request(&request_id, &text) {
                    warn!(radio_id, request_id, %error, "invalid radio websocket send_cw_text command");
                    continue;
                }
                let (completed, result) = oneshot::channel();
                if radio_handle
                    .send_command(RadioCommand::SendCwText {
                        text,
                        wait_for_completion,
                        completed,
                    })
                    .await
                    .is_ok()
                {
                    spawn_radio_message_completion(direct_tx.clone(), request_id, result);
                }
            }
            Ok(RadioClientMessage::StopKeying) => {
                let _ = radio_handle.send_command(RadioCommand::StopKeying).await;
            }
            Ok(RadioClientMessage::SetWpm { wpm }) => {
                if let Err(error) = validate_cw_wpm(wpm) {
                    warn!(radio_id, wpm, %error, "invalid radio websocket set_wpm command");
                    continue;
                }
                let _ = radio_handle.send_command(RadioCommand::SetWpm(wpm)).await;
            }
            Err(error) => warn!(radio_id, %error, "invalid radio websocket message"),
        }
    }

    outbound.abort();
    state.release(radio_id).await;
    info!(radio_id, "radio websocket disconnected");
}

async fn send_radio_ws_message(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: &RadioServerMessage,
) -> Result<(), axum::Error> {
    sender
        .send(Message::Text(
            serde_json::to_string(message)
                .expect("radio websocket message should serialize")
                .into(),
        ))
        .await
}

fn spawn_radio_message_completion(
    direct_tx: mpsc::Sender<RadioServerMessage>,
    request_id: String,
    completed: oneshot::Receiver<Result<(), String>>,
) {
    tokio::spawn(async move {
        if matches!(completed.await, Ok(Ok(()))) {
            let _ = direct_tx
                .send(RadioServerMessage::MessageSent { request_id })
                .await;
        }
    });
}

fn validate_radio_frequency_hz(frequency_hz: u64) -> Result<(), String> {
    if frequency_hz == 0 || frequency_hz > MAX_RADIO_FREQUENCY_HZ {
        return Err(format!(
            "frequency must be between 1 and {MAX_RADIO_FREQUENCY_HZ} Hz"
        ));
    }
    Ok(())
}

fn validate_radio_mode(mode: &str) -> Result<(), String> {
    let mode = mode.trim().to_uppercase();
    if modes::LOGGER_MODE_OPTIONS.contains(&mode.as_str()) {
        Ok(())
    } else {
        Err(format!(
            "mode must be one of: {}",
            modes::LOGGER_MODE_OPTIONS.join(", ")
        ))
    }
}

fn validate_rit_adjustment_hz(hz: i32) -> Result<(), String> {
    if hz <= 0 || hz > MAX_RIT_OFFSET_HZ {
        return Err(format!(
            "RIT adjustment must be between 1 and {MAX_RIT_OFFSET_HZ} Hz"
        ));
    }
    Ok(())
}

fn validate_message_request(
    request_id: &str,
    mode: &str,
    keys: &[String],
    fields: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    validate_required_text("Message request id", request_id, MAX_REQUEST_ID_LEN)?;
    if !is_valid_message_mode(mode) {
        return Err("Message mode must be run or S&P".to_string());
    }
    if keys.is_empty() {
        return Err("Message keys must contain at least one key".to_string());
    }
    for key in keys {
        let normalized_key = key.trim().to_uppercase();
        if !matches!(
            normalized_key.as_str(),
            "F1" | "F2" | "F3" | "F4" | "F5" | "F6" | "F7" | "F8" | "F9" | "F10" | "F11" | "F12"
        ) {
            return Err("Message keys must contain only F1 through F12".to_string());
        }
    }
    if fields.len() > MAX_MESSAGE_FIELDS {
        return Err(format!(
            "Message fields cannot contain more than {MAX_MESSAGE_FIELDS} entries"
        ));
    }
    for (key, value) in fields {
        validate_field_name(key)?;
        validate_json_value_size(value, 0).map_err(|error| format!("{key}: {error}"))?;
    }
    Ok(())
}

fn validate_cw_text_request(request_id: &str, text: &str) -> Result<(), String> {
    validate_required_text("CW request id", request_id, MAX_REQUEST_ID_LEN)?;
    validate_required_text("CW text", text, MAX_CW_TEXT_LEN)
}

fn validate_cw_wpm(wpm: u8) -> Result<(), String> {
    if !(MIN_CW_WPM..=MAX_CW_WPM).contains(&wpm) {
        return Err(format!(
            "CW WPM must be between {MIN_CW_WPM} and {MAX_CW_WPM}"
        ));
    }
    Ok(())
}

fn validate_required_text(label: &str, value: &str, max_length: usize) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label} is required"));
    }
    if value.chars().count() > max_length {
        return Err(format!("{label} must be at most {max_length} characters"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} cannot contain control characters"));
    }
    Ok(())
}

fn validate_field_name(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("field names cannot be empty".to_string());
    }
    if key.chars().count() > MAX_FIELD_NAME_LEN {
        return Err(format!(
            "field name {key} must be at most {MAX_FIELD_NAME_LEN} characters"
        ));
    }
    if key.chars().any(char::is_control) {
        return Err(format!(
            "field name {key} cannot contain control characters"
        ));
    }
    Ok(())
}

fn validate_json_value_size(value: &Value, depth: usize) -> Result<(), String> {
    if depth > MAX_FIELD_JSON_DEPTH {
        return Err(format!(
            "nested JSON cannot be deeper than {MAX_FIELD_JSON_DEPTH} levels"
        ));
    }
    match value {
        Value::String(value) if value.chars().count() > MAX_FIELD_STRING_LEN => Err(format!(
            "string value must be at most {MAX_FIELD_STRING_LEN} characters"
        )),
        Value::Array(values) => {
            if values.len() > MAX_FIELD_ARRAY_ITEMS {
                return Err(format!(
                    "array value cannot contain more than {MAX_FIELD_ARRAY_ITEMS} items"
                ));
            }
            for value in values {
                validate_json_value_size(value, depth + 1)?;
            }
            Ok(())
        }
        Value::Object(values) => {
            if values.len() > MAX_FIELD_OBJECT_ENTRIES {
                return Err(format!(
                    "object value cannot contain more than {MAX_FIELD_OBJECT_ENTRIES} fields"
                ));
            }
            for (key, value) in values {
                validate_field_name(key)?;
                validate_json_value_size(value, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BandCatalog, voice_keyer::VoiceKeyer};

    fn test_radio(radio_id: i64, name: &str) -> RadioConfig {
        RadioConfig::new(
            radio_id,
            crate::RadioSettings {
                name: name.to_string(),
                radio_kind: "dummy".to_string(),
                transport_kind: "none".to_string(),
                tcp_host: String::new(),
                tcp_port: 0,
                serial_port: String::new(),
                serial_baud_rate: 115_200,
                options: String::new(),
                data_mode: "DATA-USB".to_string(),
                rtty_mode: "RTTY".to_string(),
                wsjtx_enabled: false,
                wsjtx_bind_address: "127.0.0.1".to_string(),
                wsjtx_port: 2237,
                wsjtx_multicast_group: String::new(),
                cw_tuning_increment_hz: 20,
                ssb_tuning_increment_hz: 100,
                rit_clear_on_log: false,
                voice_input_device_id: None,
                voice_output_device_id: None,
                cw_keyer_type: "none".to_string(),
                winkeyer_serial_port: String::new(),
                cw_serial_port: String::new(),
                cw_serial_baud_rate: 9_600,
                cw_serial_line: "dtr".to_string(),
                cw_messages: String::new(),
                voice_messages: String::new(),
            },
        )
    }

    fn test_state(radios: Vec<RadioConfig>) -> RadioWebSocketState {
        RadioWebSocketState::new(
            RadioManager::new(VoiceKeyer::new(), BandCatalog::new(Vec::new())),
            RadioIoConfig::new(radios),
        )
    }

    #[test]
    fn validates_radio_websocket_commands() {
        assert!(validate_radio_frequency_hz(14_025_000).is_ok());
        assert!(validate_radio_frequency_hz(0).is_err());
        assert!(validate_radio_mode("CW").is_ok());
        assert!(validate_radio_mode("USB").is_err());
        assert!(validate_rit_adjustment_hz(25).is_ok());
        assert!(validate_rit_adjustment_hz(0).is_err());
        assert!(validate_cw_wpm(25).is_ok());
        assert!(validate_cw_wpm(4).is_err());
    }

    #[test]
    fn validates_keying_message_shape() {
        let fields = serde_json::Map::from_iter([("CALL".to_string(), Value::from("K1ABC"))]);
        assert!(validate_message_request("request-1", "run", &["F1".to_string()], &fields).is_ok());
        assert!(
            validate_message_request("request-1", "run", &["F13".to_string()], &fields).is_err()
        );
        assert!(validate_cw_text_request("request-1", "CQ TEST").is_ok());
        assert!(validate_cw_text_request("request-1", "").is_err());
    }

    #[tokio::test]
    async fn active_radio_cannot_be_changed_and_all_acquisitions_count() {
        let state = test_state(vec![test_radio(1, "Initial")]);
        let first = state.acquire(1).await.expect("first user acquires radio");
        let second = state.acquire(1).await.expect("second user shares radio");

        assert_eq!(first.config.name, "Initial");
        assert!(matches!(
            state.begin_mutation(1),
            Err(RadioMutationError::InUse { radio_id: 1 })
        ));

        drop(first);
        state.release(1).await;
        assert!(matches!(
            state.begin_mutation(1),
            Err(RadioMutationError::InUse { radio_id: 1 })
        ));

        drop(second);
        state.release(1).await;
        assert!(state.begin_mutation(1).is_ok());
    }

    #[tokio::test]
    async fn mutation_reservation_blocks_acquisition_and_drop_restores_access() {
        let state = test_state(vec![test_radio(1, "Initial")]);
        let mutation = state.begin_mutation(1).expect("idle radio can be changed");

        assert!(matches!(
            state.acquire(1).await,
            Err(RadioAcquireError::MutationInProgress { radio_id: 1 })
        ));

        drop(mutation);
        let acquired = state
            .acquire(1)
            .await
            .expect("dropped permit restores access");
        drop(acquired);
        state.release(1).await;
    }

    #[tokio::test]
    async fn committed_update_replaces_configuration_and_delete_removes_it() {
        let state = test_state(vec![test_radio(1, "Initial")]);
        let mutation = state.begin_mutation(1).expect("idle radio can be updated");
        mutation
            .commit_update(test_radio(1, "Updated"))
            .expect("update commits");

        let acquired = state.acquire(1).await.expect("updated radio is available");
        assert_eq!(acquired.config.name, "Updated");
        drop(acquired);
        state.release(1).await;

        state
            .begin_mutation(1)
            .expect("idle radio can be deleted")
            .commit_delete()
            .expect("delete commits");
        assert!(matches!(
            state.acquire(1).await,
            Err(RadioAcquireError::NotFound { radio_id: 1 })
        ));
        assert!(!state.contains_radio(1));
    }

    #[tokio::test]
    async fn created_radio_can_be_added_after_initialization() {
        let state = test_state(Vec::new());
        state
            .add_radio(test_radio(2, "Created"))
            .expect("new radio is added");
        assert!(state.add_radio(test_radio(2, "Duplicate")).is_err());

        let acquired = state.acquire(2).await.expect("created radio is available");
        assert_eq!(acquired.config.name, "Created");
        drop(acquired);
        state.release(2).await;
    }
}
