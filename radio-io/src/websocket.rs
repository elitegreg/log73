use crate::{
    DigitalIoEvent, DigitalIoManager, DigitalIoTargetState, RadioClientMessage, RadioCommand,
    RadioConfig, RadioIoConfig, RadioManager, RadioServerMessage, WsjtXEvent, WsjtXManager,
    WsjtXTargetState, cw, is_valid_message_mode, modes,
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
use tracing::{debug, info, warn};

const MAX_RADIO_FREQUENCY_HZ: u64 = 500_000_000;
const MAX_RIT_OFFSET_HZ: i32 = 9_999;
const MIN_CW_WPM: u8 = 5;
const MAX_CW_WPM: u8 = 60;
const MAX_REQUEST_ID_LEN: usize = 64;
const MAX_LOGGER_ID_LEN: usize = 128;
const MAX_TEXT_LEN: usize = 256;
const MAX_DIGITAL_IO_TEXT_LEN: usize = 16_384;
const MAX_MESSAGE_FIELDS: usize = 100;
const MAX_FIELD_NAME_LEN: usize = 64;
const MAX_FIELD_STRING_LEN: usize = 1024;
const MAX_FIELD_ARRAY_ITEMS: usize = 100;
const MAX_FIELD_OBJECT_ENTRIES: usize = 100;
const MAX_FIELD_JSON_DEPTH: usize = 4;

#[derive(Clone)]
pub struct RadioWebSocketState {
    radio_manager: RadioManager,
    wsjtx_manager: WsjtXManager,
    digital_io_manager: DigitalIoManager,
    registry: Arc<Mutex<RadioRegistry>>,
    shutdown_sessions: broadcast::Sender<()>,
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
        let (shutdown_sessions, _) = broadcast::channel(1);
        Self {
            radio_manager,
            wsjtx_manager: WsjtXManager::new(),
            digital_io_manager: DigitalIoManager::new(),
            registry: Arc::new(Mutex::new(RadioRegistry { radios })),
            shutdown_sessions,
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

    fn close_sessions(&self) {
        let _ = self.shutdown_sessions.send(());
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

#[derive(Clone)]
pub struct SingleRadioWebSocketState {
    state: RadioWebSocketState,
    radio_id: i64,
}

impl SingleRadioWebSocketState {
    pub fn new(radio_manager: RadioManager, config: RadioConfig) -> Self {
        let radio_id = config.id;
        Self {
            state: RadioWebSocketState::new(radio_manager, RadioIoConfig::new(vec![config])),
            radio_id,
        }
    }

    /// Deterministically stops all shared radio resources for this local host.
    pub async fn shutdown(&self) {
        self.state.close_sessions();
        self.state.wsjtx_manager.shutdown_all().await;
        self.state.digital_io_manager.shutdown_all().await;
        self.state.radio_manager.shutdown_all().await;
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
    radio_id: Option<i64>,
    logger_id: Option<String>,
    log_id: Option<i64>,
}

#[derive(Clone)]
struct LoggerIdentity {
    logger_id: String,
    log_id: i64,
}

pub async fn radio_ws_handler(
    Extension(state): Extension<RadioWebSocketState>,
    Query(query): Query<RadioWsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(radio_id) = query.radio_id else {
        return (StatusCode::BAD_REQUEST, "radio_id is required").into_response();
    };
    let identity = match logger_identity(&query) {
        Ok(identity) => identity,
        Err(error) => return (StatusCode::BAD_REQUEST, error).into_response(),
    };
    if !state.contains_radio(radio_id) {
        return (StatusCode::NOT_FOUND, format!("radio {radio_id} not found")).into_response();
    }

    ws.on_upgrade(move |socket| handle_radio_socket(socket, state, radio_id, identity))
        .into_response()
}

pub async fn single_radio_ws_handler(
    Extension(single): Extension<SingleRadioWebSocketState>,
    Query(query): Query<RadioWsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if query
        .radio_id
        .is_some_and(|radio_id| radio_id != single.radio_id)
    {
        return (StatusCode::NOT_FOUND, "radio not found").into_response();
    }
    let identity = match logger_identity(&query) {
        Ok(identity) => identity,
        Err(error) => return (StatusCode::BAD_REQUEST, error).into_response(),
    };
    ws.on_upgrade(move |socket| {
        handle_radio_socket(socket, single.state, single.radio_id, identity)
    })
    .into_response()
}

fn logger_identity(query: &RadioWsQuery) -> Result<Option<LoggerIdentity>, String> {
    match (&query.logger_id, query.log_id) {
        (None, None) => Ok(None),
        (Some(logger_id), Some(log_id)) => {
            validate_required_text("Logger id", logger_id, MAX_LOGGER_ID_LEN)?;
            if log_id <= 0 {
                return Err("log_id must be positive".to_string());
            }
            Ok(Some(LoggerIdentity {
                logger_id: logger_id.trim().to_string(),
                log_id,
            }))
        }
        _ => Err("logger_id and log_id must be provided together".to_string()),
    }
}

async fn handle_radio_socket(
    socket: WebSocket,
    state: RadioWebSocketState,
    radio_id: i64,
    identity: Option<LoggerIdentity>,
) {
    let acquired = match state.acquire(radio_id).await {
        Ok(acquired) => acquired,
        Err(error) => {
            warn!(radio_id, %error, "radio websocket could not acquire radio");
            return;
        }
    };
    let config = acquired.config;
    let radio_handle = acquired.handle;
    let initial_state = radio_handle.current_state().await;
    let mut wsjtx_subscriptions = identity.as_ref().map(|_| {
        (
            state.wsjtx_manager.subscribe_targets(),
            state.wsjtx_manager.subscribe_events(),
        )
    });
    let mut digital_io_subscriptions = identity.as_ref().map(|_| {
        (
            state.digital_io_manager.subscribe_targets(),
            state.digital_io_manager.subscribe_events(),
        )
    });
    let initial_wsjtx_target = if let Some(identity) = &identity {
        match state
            .wsjtx_manager
            .acquire(
                radio_id,
                &identity.logger_id,
                identity.log_id,
                config.clone(),
                initial_state.clone(),
                radio_handle.subscribe(),
            )
            .await
        {
            Ok(target) => Some(target),
            Err(error) => {
                warn!(radio_id, logger_id = %identity.logger_id, %error, "radio websocket could not register WSJT-X logger");
                state.release(radio_id).await;
                return;
            }
        }
    } else {
        None
    };
    let initial_digital_io_target = if let Some(identity) = &identity {
        match state
            .digital_io_manager
            .acquire(
                radio_id,
                &identity.logger_id,
                identity.log_id,
                config.clone(),
                initial_state.clone(),
                radio_handle.subscribe(),
            )
            .await
        {
            Ok(target) => Some(target),
            Err(error) => {
                warn!(radio_id, logger_id = %identity.logger_id, %error, "radio websocket could not register digital I/O logger");
                state
                    .wsjtx_manager
                    .release(radio_id, &identity.logger_id)
                    .await;
                state.release(radio_id).await;
                return;
            }
        }
    } else {
        None
    };

    info!(radio_id, "radio websocket connected");
    let (mut sender, mut receiver) = socket.split();
    let current_status = RadioServerMessage::RadioStatus(radio_handle.current_status().await);
    if send_radio_ws_message(&mut sender, &current_status)
        .await
        .is_err()
    {
        release_socket_registration(&state, radio_id, identity.as_ref()).await;
        return;
    }
    if let Some(current) = initial_state {
        let current = RadioServerMessage::RadioState(current);
        if send_radio_ws_message(&mut sender, &current).await.is_err() {
            release_socket_registration(&state, radio_id, identity.as_ref()).await;
            return;
        }
    }
    if let Some(target) = &initial_wsjtx_target
        && send_radio_ws_message(&mut sender, &wsjtx_target_message(target))
            .await
            .is_err()
    {
        release_socket_registration(&state, radio_id, identity.as_ref()).await;
        return;
    }
    if let Some(target) = &initial_digital_io_target
        && send_radio_ws_message(&mut sender, &digital_io_target_message(target))
            .await
            .is_err()
    {
        release_socket_registration(&state, radio_id, identity.as_ref()).await;
        return;
    }

    let mut status_updates = radio_handle.subscribe_status();
    let mut state_updates = radio_handle.subscribe();
    let (direct_tx, mut direct_rx) = mpsc::channel::<RadioServerMessage>(32);
    let wsjtx_bridge = identity.as_ref().and_then(|identity| {
        wsjtx_subscriptions.take().map(|subscriptions| {
            spawn_wsjtx_socket_bridge(
                subscriptions,
                radio_id,
                identity.logger_id.clone(),
                direct_tx.clone(),
            )
        })
    });
    let digital_io_bridge = identity.as_ref().and_then(|identity| {
        digital_io_subscriptions.take().map(|subscriptions| {
            spawn_digital_io_socket_bridge(
                subscriptions,
                radio_id,
                identity.logger_id.clone(),
                direct_tx.clone(),
            )
        })
    });
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

    let mut shutdown = state.shutdown_sessions.subscribe();
    loop {
        let message = tokio::select! {
            _ = shutdown.recv() => break,
            message = receiver.next() => message,
        };
        let Some(Ok(message)) = message else {
            break;
        };
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
                let current_mode = radio_handle
                    .current_state()
                    .await
                    .map(|radio_state| radio_state.mode);
                if current_mode
                    .as_deref()
                    .is_some_and(|mode| fldigi_enabled_for_mode(&config, mode))
                {
                    let result =
                        render_digital_messages(&config.digital_messages, &mode, &keys, &fields);
                    match (identity.as_ref(), result) {
                        (Some(identity), Ok(text)) => {
                            send_fldigi_text(
                                &state.digital_io_manager,
                                radio_id,
                                identity,
                                text,
                                Some(request_id),
                                &direct_tx,
                            )
                            .await;
                        }
                        (Some(identity), Err(error)) => {
                            send_digital_io_error(&direct_tx, identity.log_id, error).await;
                        }
                        (None, _) => {
                            warn!(
                                radio_id,
                                "unidentified radio websocket cannot send FLDigi messages"
                            );
                        }
                    }
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
            Ok(RadioClientMessage::SendText {
                request_id,
                text,
                wait_for_completion,
            }) => {
                if let Err(error) = validate_text_request(&request_id, &text) {
                    warn!(radio_id, request_id, %error, "invalid radio websocket send_text command");
                    continue;
                }
                let current_mode = radio_handle
                    .current_state()
                    .await
                    .map(|radio_state| radio_state.mode);
                if current_mode
                    .as_deref()
                    .is_some_and(|mode| fldigi_enabled_for_mode(&config, mode))
                {
                    if let Some(identity) = &identity {
                        send_fldigi_text(
                            &state.digital_io_manager,
                            radio_id,
                            identity,
                            text,
                            Some(request_id),
                            &direct_tx,
                        )
                        .await;
                    } else {
                        warn!(
                            radio_id,
                            "unidentified radio websocket cannot send FLDigi text"
                        );
                    }
                    continue;
                }
                if !current_mode.as_deref().is_some_and(mode_is_cw) {
                    let error = "text sending requires CW or active FLDigi DATA/RTTY";
                    warn!(radio_id, radio_mode = ?current_mode, error);
                    if let Some(identity) = &identity {
                        send_digital_io_error(&direct_tx, identity.log_id, error.to_string()).await;
                    }
                    continue;
                }
                let (completed, result) = oneshot::channel();
                if radio_handle
                    .send_command(RadioCommand::SendText {
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
                let current_mode = radio_handle
                    .current_state()
                    .await
                    .map(|radio_state| radio_state.mode);
                if current_mode
                    .as_deref()
                    .is_some_and(|mode| fldigi_enabled_for_mode(&config, mode))
                    && let Some(identity) = &identity
                    && let Err(error) = state
                        .digital_io_manager
                        .stop_transmitting(radio_id, &identity.logger_id)
                        .await
                {
                    send_digital_io_error(&direct_tx, identity.log_id, error).await;
                }
            }
            Ok(RadioClientMessage::SetWpm { wpm }) => {
                if let Err(error) = validate_cw_wpm(wpm) {
                    warn!(radio_id, wpm, %error, "invalid radio websocket set_wpm command");
                    continue;
                }
                let _ = radio_handle.send_command(RadioCommand::SetWpm(wpm)).await;
            }
            Ok(RadioClientMessage::SetWsjtXTarget { enabled }) => {
                let Some(identity) = &identity else {
                    warn!(
                        radio_id,
                        "unidentified radio websocket cannot set WSJT-X target"
                    );
                    continue;
                };
                if let Err(error) = state
                    .wsjtx_manager
                    .set_target(radio_id, &identity.logger_id, enabled)
                    .await
                {
                    warn!(radio_id, logger_id = %identity.logger_id, enabled, %error, "unable to update WSJT-X target");
                }
            }
            Ok(RadioClientMessage::SetDigitalIoTarget { enabled }) => {
                let Some(identity) = &identity else {
                    warn!(
                        radio_id,
                        "unidentified radio websocket cannot set digital I/O target"
                    );
                    continue;
                };
                if let Err(error) = state
                    .digital_io_manager
                    .set_target(radio_id, &identity.logger_id, enabled)
                    .await
                {
                    warn!(radio_id, logger_id = %identity.logger_id, enabled, %error, "unable to update digital I/O target");
                }
            }
            Ok(RadioClientMessage::DigitalIoSend { text }) => {
                let Some(identity) = &identity else {
                    warn!(
                        radio_id,
                        "unidentified radio websocket cannot send digital I/O text"
                    );
                    continue;
                };
                if let Err(error) = validate_digital_io_text(&text) {
                    warn!(radio_id, %error, "invalid digital I/O send command");
                    continue;
                }
                if let Err(error) = state
                    .digital_io_manager
                    .transmit(radio_id, &identity.logger_id, text)
                    .await
                {
                    warn!(radio_id, logger_id = %identity.logger_id, %error, "unable to send digital I/O text");
                }
            }
            Ok(RadioClientMessage::DigitalIoClear) => {
                let Some(identity) = &identity else {
                    warn!(
                        radio_id,
                        "unidentified radio websocket cannot clear digital I/O text"
                    );
                    continue;
                };
                if let Err(error) = state
                    .digital_io_manager
                    .clear_receive_buffer(radio_id, &identity.logger_id)
                    .await
                {
                    warn!(radio_id, logger_id = %identity.logger_id, %error, "unable to clear digital I/O text");
                }
            }
            Ok(RadioClientMessage::WsjtXEventReceived { event_id }) => {
                if let Err(error) = validate_wsjtx_event_id(&event_id) {
                    warn!(radio_id, %error, "invalid WSJT-X receipt acknowledgment");
                    continue;
                }
                if let Some(identity) = &identity {
                    debug!(radio_id, logger_id = %identity.logger_id, event_id, "browser acknowledged WSJT-X event receipt");
                }
            }
            Err(error) => warn!(radio_id, %error, "invalid radio websocket message"),
        }
    }

    outbound.abort();
    if let Some(bridge) = wsjtx_bridge {
        bridge.abort();
    }
    if let Some(bridge) = digital_io_bridge {
        bridge.abort();
    }
    release_socket_registration(&state, radio_id, identity.as_ref()).await;
    info!(radio_id, "radio websocket disconnected");
}

async fn release_socket_registration(
    state: &RadioWebSocketState,
    radio_id: i64,
    identity: Option<&LoggerIdentity>,
) {
    if let Some(identity) = identity {
        state
            .wsjtx_manager
            .release(radio_id, &identity.logger_id)
            .await;
        state
            .digital_io_manager
            .release(radio_id, &identity.logger_id)
            .await;
    }
    state.release(radio_id).await;
}

fn wsjtx_target_message(target: &WsjtXTargetState) -> RadioServerMessage {
    RadioServerMessage::WsjtXTarget {
        logger_id: target.logger_id.clone(),
        log_id: target.log_id,
    }
}

fn digital_io_target_message(target: &DigitalIoTargetState) -> RadioServerMessage {
    RadioServerMessage::DigitalIoTarget {
        logger_id: target.logger_id.clone(),
        log_id: target.log_id,
    }
}

fn spawn_wsjtx_socket_bridge(
    (mut targets, mut events): (
        broadcast::Receiver<WsjtXTargetState>,
        broadcast::Receiver<WsjtXEvent>,
    ),
    radio_id: i64,
    logger_id: String,
    direct_tx: mpsc::Sender<RadioServerMessage>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let message = tokio::select! {
                target = targets.recv() => match target {
                    Ok(target) if target.radio_id == radio_id => wsjtx_target_message(&target),
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                event = events.recv() => match event {
                    Ok(event) => match wsjtx_event_message(event, radio_id, &logger_id) {
                        Some(message) => message,
                        None => continue,
                    },
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
            };
            if direct_tx.send(message).await.is_err() {
                break;
            }
        }
    })
}

fn spawn_digital_io_socket_bridge(
    (mut targets, mut events): (
        broadcast::Receiver<DigitalIoTargetState>,
        broadcast::Receiver<DigitalIoEvent>,
    ),
    radio_id: i64,
    logger_id: String,
    direct_tx: mpsc::Sender<RadioServerMessage>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let message = tokio::select! {
                target = targets.recv() => match target {
                    Ok(target) if target.radio_id == radio_id => digital_io_target_message(&target),
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                event = events.recv() => match event {
                    Ok(event) => match digital_io_event_message(event, radio_id, &logger_id) {
                        Some(message) => message,
                        None => continue,
                    },
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
            };
            if direct_tx.send(message).await.is_err() {
                break;
            }
        }
    })
}

fn wsjtx_event_message(
    event: WsjtXEvent,
    radio_id: i64,
    logger_id: &str,
) -> Option<RadioServerMessage> {
    match event {
        WsjtXEvent::LoggedAdif {
            radio_id: event_radio_id,
            logger_id: target_logger_id,
            log_id,
            event_id,
            text,
        } if event_radio_id == radio_id && target_logger_id == logger_id => {
            Some(RadioServerMessage::WsjtXLoggedAdif {
                event_id,
                log_id,
                text,
            })
        }
        WsjtXEvent::Error {
            radio_id: event_radio_id,
            logger_id: target_logger_id,
            log_id,
            message,
        } if event_radio_id == radio_id && target_logger_id == logger_id => {
            Some(RadioServerMessage::WsjtXError { log_id, message })
        }
        _ => None,
    }
}

fn digital_io_event_message(
    event: DigitalIoEvent,
    radio_id: i64,
    logger_id: &str,
) -> Option<RadioServerMessage> {
    match event {
        DigitalIoEvent::Received {
            radio_id: event_radio_id,
            logger_id: event_logger_id,
            log_id,
            text,
        } if event_radio_id == radio_id && event_logger_id == logger_id => {
            Some(RadioServerMessage::DigitalIoReceived { log_id, text })
        }
        DigitalIoEvent::Error {
            radio_id: event_radio_id,
            logger_id: event_logger_id,
            log_id,
            message,
        } if event_radio_id == radio_id && event_logger_id == logger_id => {
            Some(RadioServerMessage::DigitalIoError { log_id, message })
        }
        _ => None,
    }
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

fn fldigi_enabled_for_mode(config: &RadioConfig, mode: &str) -> bool {
    if mode.eq_ignore_ascii_case("DATA") {
        config.fldigi_data_enabled
    } else if mode.eq_ignore_ascii_case("RTTY") {
        config.fldigi_rtty_enabled
    } else {
        false
    }
}

fn mode_is_cw(mode: &str) -> bool {
    mode.eq_ignore_ascii_case("CW") || mode.eq_ignore_ascii_case("CW-R")
}

fn render_digital_messages(
    config: &str,
    mode: &str,
    keys: &[String],
    fields: &serde_json::Map<String, Value>,
) -> Result<String, String> {
    let mut rendered = Vec::new();
    for key in keys {
        let text = cw::render(config, mode, key, fields)
            .ok_or_else(|| format!("unknown digital message {key}"))?;
        if !text.is_empty() {
            rendered.push(text);
        }
    }
    Ok(rendered.join(" "))
}

async fn send_fldigi_text(
    manager: &DigitalIoManager,
    radio_id: i64,
    identity: &LoggerIdentity,
    text: String,
    request_id: Option<String>,
    direct_tx: &mpsc::Sender<RadioServerMessage>,
) {
    if text.is_empty() {
        if let Some(request_id) = request_id {
            let _ = direct_tx
                .send(RadioServerMessage::MessageSent { request_id })
                .await;
        }
        return;
    }
    match manager.transmit(radio_id, &identity.logger_id, text).await {
        Ok(completed) => {
            if let Some(request_id) = request_id {
                spawn_radio_message_completion(direct_tx.clone(), request_id, completed);
            }
        }
        Err(error) => send_digital_io_error(direct_tx, identity.log_id, error).await,
    }
}

async fn send_digital_io_error(
    direct_tx: &mpsc::Sender<RadioServerMessage>,
    log_id: i64,
    message: String,
) {
    let _ = direct_tx
        .send(RadioServerMessage::DigitalIoError { log_id, message })
        .await;
}

fn validate_radio_frequency_hz(frequency_hz: u64) -> Result<(), String> {
    if frequency_hz == 0 || frequency_hz > MAX_RADIO_FREQUENCY_HZ {
        return Err(format!(
            "frequency must be between 1 and {MAX_RADIO_FREQUENCY_HZ} Hz"
        ));
    }
    Ok(())
}

fn validate_digital_io_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("digital I/O text cannot be empty".to_string());
    }
    if text.chars().count() > MAX_DIGITAL_IO_TEXT_LEN {
        return Err(format!(
            "digital I/O text must be at most {MAX_DIGITAL_IO_TEXT_LEN} characters"
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

fn validate_text_request(request_id: &str, text: &str) -> Result<(), String> {
    validate_required_text("Text request id", request_id, MAX_REQUEST_ID_LEN)?;
    validate_required_text("Text", text, MAX_TEXT_LEN)
}

fn validate_cw_wpm(wpm: u8) -> Result<(), String> {
    if !(MIN_CW_WPM..=MAX_CW_WPM).contains(&wpm) {
        return Err(format!(
            "CW WPM must be between {MIN_CW_WPM} and {MAX_CW_WPM}"
        ));
    }
    Ok(())
}

fn validate_wsjtx_event_id(event_id: &str) -> Result<(), String> {
    validate_required_text("WSJT-X event id", event_id, MAX_REQUEST_ID_LEN)?;
    uuid::Uuid::parse_str(event_id.trim())
        .map(|_| ())
        .map_err(|_| "WSJT-X event id must be a UUID".to_string())
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
                digital_program: "none".to_string(),
                wsjtx_enabled: false,
                wsjtx_bind_address: "127.0.0.1".to_string(),
                wsjtx_port: 2237,
                wsjtx_multicast_group: String::new(),
                fldigi_data_enabled: false,
                fldigi_rtty_enabled: false,
                fldigi_host: crate::DEFAULT_FLDIGI_HOST.to_string(),
                fldigi_port: crate::DEFAULT_FLDIGI_PORT,
                flrig_enabled: false,
                flrig_port: crate::DEFAULT_FLRIG_PORT,
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
                digital_messages: crate::digital_messages::DEFAULT_DIGITAL_MESSAGES.to_string(),
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
        assert!(validate_wsjtx_event_id("8bf420c0-46f2-44aa-bfac-71a2ed41ef1a").is_ok());
        assert!(validate_wsjtx_event_id("event-1").is_err());
        assert!(validate_digital_io_text("CQ TEST").is_ok());
        assert!(validate_digital_io_text("").is_err());
        assert!(validate_digital_io_text(&"x".repeat(MAX_DIGITAL_IO_TEXT_LEN + 1)).is_err());
    }

    #[test]
    fn validates_keying_message_shape() {
        let fields = serde_json::Map::from_iter([("CALL".to_string(), Value::from("K1ABC"))]);
        assert!(validate_message_request("request-1", "run", &["F1".to_string()], &fields).is_ok());
        assert!(
            validate_message_request("request-1", "run", &["F13".to_string()], &fields).is_err()
        );
        assert!(validate_text_request("request-1", "CQ TEST").is_ok());
        assert!(validate_text_request("request-1", "").is_err());
    }

    #[test]
    fn identifies_modes_with_configured_fldigi() {
        let mut radio = test_radio(1, "Digital");
        radio.fldigi_data_enabled = true;
        radio.fldigi_rtty_enabled = false;

        assert!(fldigi_enabled_for_mode(&radio, "DATA"));
        assert!(!fldigi_enabled_for_mode(&radio, "RTTY"));
        assert!(!fldigi_enabled_for_mode(&radio, "CW"));

        radio.fldigi_data_enabled = false;
        radio.fldigi_rtty_enabled = true;
        assert!(!fldigi_enabled_for_mode(&radio, "DATA"));
        assert!(fldigi_enabled_for_mode(&radio, "rtty"));
        assert!(mode_is_cw("CW"));
        assert!(mode_is_cw("cw-r"));
        assert!(!mode_is_cw("DATA"));
    }

    #[test]
    fn renders_digital_message_sequences() {
        let fields = serde_json::Map::from_iter([
            ("CALL".to_string(), Value::from("K1ABC")),
            ("EXCH".to_string(), Value::from("599 001")),
        ]);
        let rendered = render_digital_messages(
            crate::digital_messages::DEFAULT_DIGITAL_MESSAGES,
            "run",
            &["F5".to_string(), "F2".to_string()],
            &fields,
        )
        .expect("digital messages render");

        assert_eq!(rendered, "K1ABC 599 001");
        assert!(
            render_digital_messages(
                crate::digital_messages::DEFAULT_DIGITAL_MESSAGES,
                "run",
                &["F13".to_string()],
                &fields,
            )
            .is_err()
        );
    }

    #[test]
    fn validates_optional_logger_identity_as_an_atomic_pair() {
        let control_only = RadioWsQuery {
            radio_id: Some(1),
            logger_id: None,
            log_id: None,
        };
        assert!(logger_identity(&control_only).unwrap().is_none());

        let identified = RadioWsQuery {
            radio_id: Some(1),
            logger_id: Some(" logger-1 ".to_string()),
            log_id: Some(42),
        };
        let identity = logger_identity(&identified)
            .expect("identity validates")
            .expect("identity exists");
        assert_eq!(identity.logger_id, "logger-1");
        assert_eq!(identity.log_id, 42);

        let partial = RadioWsQuery {
            radio_id: Some(1),
            logger_id: Some("logger-1".to_string()),
            log_id: None,
        };
        assert!(logger_identity(&partial).is_err());
    }

    #[test]
    fn single_radio_state_owns_exactly_its_configured_runtime_radio() {
        let radio = test_radio(7, "Client radio");
        let single = SingleRadioWebSocketState::new(
            RadioManager::new(VoiceKeyer::new(), BandCatalog::new(Vec::new())),
            radio,
        );
        assert_eq!(single.radio_id, 7);
        assert!(single.state.contains_radio(7));
        assert!(!single.state.contains_radio(8));
    }

    #[test]
    fn wsjtx_events_route_only_to_the_selected_radio_and_logger() {
        let event = WsjtXEvent::LoggedAdif {
            radio_id: 1,
            logger_id: "logger-1".to_string(),
            log_id: 42,
            event_id: "event-1".to_string(),
            text: "<EOR>".to_string(),
        };
        assert!(matches!(
            wsjtx_event_message(event.clone(), 1, "logger-1"),
            Some(RadioServerMessage::WsjtXLoggedAdif { .. })
        ));
        assert!(wsjtx_event_message(event.clone(), 1, "logger-2").is_none());
        assert!(wsjtx_event_message(event, 2, "logger-1").is_none());
    }

    #[test]
    fn digital_io_events_route_only_to_the_selected_radio_and_logger() {
        let event = DigitalIoEvent::Received {
            radio_id: 1,
            logger_id: "logger-1".to_string(),
            log_id: 42,
            text: "CQ TEST".to_string(),
        };
        assert!(matches!(
            digital_io_event_message(event.clone(), 1, "logger-1"),
            Some(RadioServerMessage::DigitalIoReceived { .. })
        ));
        assert!(digital_io_event_message(event.clone(), 1, "logger-2").is_none());
        assert!(digital_io_event_message(event, 2, "logger-1").is_none());
    }

    #[tokio::test]
    async fn message_sent_waits_for_successful_transmission_completion() {
        let (direct_tx, mut direct_rx) = mpsc::channel(2);
        let (completed, result) = oneshot::channel();
        spawn_radio_message_completion(direct_tx, "request-1".to_string(), result);

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), direct_rx.recv())
                .await
                .is_err()
        );
        completed.send(Ok(())).expect("transmission completes");

        let message = tokio::time::timeout(std::time::Duration::from_secs(1), direct_rx.recv())
            .await
            .expect("completion message arrives")
            .expect("completion channel remains open");
        assert_eq!(
            message,
            RadioServerMessage::MessageSent {
                request_id: "request-1".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn failed_or_cancelled_transmission_does_not_report_message_sent() {
        let (direct_tx, mut direct_rx) = mpsc::channel(2);
        let (completed, result) = oneshot::channel();
        spawn_radio_message_completion(direct_tx, "request-1".to_string(), result);
        completed
            .send(Err("FLDigi transmission cancelled".to_string()))
            .expect("transmission is cancelled");

        let message = tokio::time::timeout(std::time::Duration::from_secs(1), direct_rx.recv())
            .await
            .expect("completion is processed");
        assert!(message.is_none());
    }

    #[tokio::test]
    async fn wsjtx_candidate_is_untargeted_until_checkbox_enable() {
        let state = test_state(vec![test_radio(1, "Initial")]);
        let acquired = state.acquire(1).await.expect("radio acquires");
        let initial = state
            .wsjtx_manager
            .acquire(
                1,
                "logger-1",
                42,
                acquired.config.clone(),
                acquired.handle.current_state().await,
                acquired.handle.subscribe(),
            )
            .await
            .expect("candidate registers");
        assert_eq!(initial.logger_id, None);
        assert_eq!(initial.log_id, None);

        let enabled = state
            .wsjtx_manager
            .set_target(1, "logger-1", true)
            .await
            .expect("checkbox enables target");
        assert_eq!(enabled.logger_id.as_deref(), Some("logger-1"));
        assert_eq!(enabled.log_id, Some(42));

        state.wsjtx_manager.release(1, "logger-1").await;
        drop(acquired);
        state.release(1).await;
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
