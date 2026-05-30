use crate::cat_keyer::CatKeyer;
use crate::cw;
use crate::db::{Database, RadioConfig};
use crate::radio::{
    RadioCommand, RadioState, RadioStatus, ServerMessage, mode_candidates_for_request,
    normalize_mode,
};
use backon::{BackoffBuilder, ExponentialBuilder};
use cw_serial_keyer::{Config as CwSerialConfig, ControlLine, SerialKeyer as CwSerialDevice};
use futures_util::future::{BoxFuture, FutureExt};
use radio_cat_rs::{ConnectionConfig, ControllableRadio, RadioKind, create_radio};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Notify, RwLock, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, trace, warn};

const CAT_RECONNECT_MIN_DELAY: Duration = Duration::from_secs(1);
const CAT_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct RadioManager {
    db: Database,
    radios: Arc<Mutex<HashMap<i64, ManagedRadioSlot>>>,
}

#[derive(Clone)]
pub struct RadioHandle {
    current_status: Arc<RwLock<RadioStatus>>,
    current: Arc<RwLock<Option<RadioState>>>,
    status_updates: broadcast::Sender<RadioStatus>,
    updates: broadcast::Sender<RadioState>,
    commands: mpsc::Sender<RadioCommand>,
}

enum ManagedRadioSlot {
    Active(ManagedRadio),
    ShuttingDown { done: Arc<Notify> },
}

struct ManagedRadio {
    current_status: Arc<RwLock<RadioStatus>>,
    current: Arc<RwLock<Option<RadioState>>>,
    status_updates: broadcast::Sender<RadioStatus>,
    updates: broadcast::Sender<RadioState>,
    commands: mpsc::Sender<RadioCommand>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
    refcount: usize,
}

impl RadioManager {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            radios: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn acquire(&self, radio_id: i64) -> Result<RadioHandle, String> {
        loop {
            let wait_for_shutdown = {
                let mut radios = self.radios.lock().await;

                if let Some(slot) = radios.get_mut(&radio_id) {
                    match slot {
                        ManagedRadioSlot::Active(radio) => {
                            radio.refcount += 1;
                            debug!(
                                radio_id,
                                refcount = radio.refcount,
                                "acquired existing managed radio"
                            );
                            return Ok(RadioHandle {
                                current_status: radio.current_status.clone(),
                                current: radio.current.clone(),
                                status_updates: radio.status_updates.clone(),
                                updates: radio.updates.clone(),
                                commands: radio.commands.clone(),
                            });
                        }
                        ManagedRadioSlot::ShuttingDown { done } => Some(done.clone()),
                    }
                } else {
                    None
                }
            };

            if let Some(done) = wait_for_shutdown {
                debug!(radio_id, "waiting for managed radio shutdown to complete");
                done.notified().await;
                continue;
            }

            let config = self
                .db
                .radio(radio_id)
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("radio not found: {radio_id}"))?;

            let mut wait_for_shutdown = None;
            {
                let mut radios = self.radios.lock().await;

                if let Some(slot) = radios.get_mut(&radio_id) {
                    match slot {
                        ManagedRadioSlot::Active(radio) => {
                            radio.refcount += 1;
                            debug!(
                                radio_id,
                                refcount = radio.refcount,
                                "acquired existing managed radio"
                            );
                            return Ok(RadioHandle {
                                current_status: radio.current_status.clone(),
                                current: radio.current.clone(),
                                status_updates: radio.status_updates.clone(),
                                updates: radio.updates.clone(),
                                commands: radio.commands.clone(),
                            });
                        }
                        ManagedRadioSlot::ShuttingDown { done } => {
                            wait_for_shutdown = Some(done.clone());
                        }
                    }
                }

                if wait_for_shutdown.is_none() {
                    debug!(
                        radio_id,
                        radio_kind = %config.radio_kind,
                        transport_kind = %config.transport_kind,
                        tcp_host = %config.tcp_host,
                        tcp_port = config.tcp_port,
                        serial_port = %config.serial_port,
                        serial_baud_rate = config.serial_baud_rate,
                        poll_frequency = config.poll_frequency,
                        cat_timeout = config.cat_timeout,
                        cw_keyer_type = %config.cw_keyer_type,
                        winkeyer_serial_port = %config.winkeyer_serial_port,
                        cw_serial_port = %config.cw_serial_port,
                        cw_serial_baud_rate = config.cw_serial_baud_rate,
                        cw_serial_line = %config.cw_serial_line,
                        "starting managed radio"
                    );
                    let current_status = Arc::new(RwLock::new(RadioStatus { online: false }));
                    let current = Arc::new(RwLock::new(None));
                    let (status_updates, _) = broadcast::channel(32);
                    let (updates, _) = broadcast::channel(32);
                    let (commands, command_rx) = mpsc::channel(32);
                    let (shutdown_tx, shutdown_rx) = oneshot::channel();
                    let task_current_status = current_status.clone();
                    let task_current = current.clone();
                    let task_status_updates = status_updates.clone();
                    let task_updates = updates.clone();
                    let task = tokio::spawn(async move {
                        run_managed_radio(
                            config,
                            task_current_status,
                            task_current,
                            task_status_updates,
                            task_updates,
                            command_rx,
                            shutdown_rx,
                        )
                        .await;
                    });

                    radios.insert(
                        radio_id,
                        ManagedRadioSlot::Active(ManagedRadio {
                            current_status: current_status.clone(),
                            current: current.clone(),
                            status_updates: status_updates.clone(),
                            updates: updates.clone(),
                            commands: commands.clone(),
                            shutdown: Some(shutdown_tx),
                            task: Some(task),
                            refcount: 1,
                        }),
                    );

                    return Ok(RadioHandle {
                        current_status,
                        current,
                        status_updates,
                        updates,
                        commands,
                    });
                }
            }

            if let Some(done) = wait_for_shutdown {
                debug!(radio_id, "waiting for managed radio shutdown to complete");
                done.notified().await;
            }
        }
    }

    pub async fn release(&self, radio_id: i64) {
        let mut shutdown = None;
        let mut task = None;
        let mut done = None;

        {
            let mut radios = self.radios.lock().await;
            if let Some(slot) = radios.get_mut(&radio_id) {
                match slot {
                    ManagedRadioSlot::Active(radio) => {
                        radio.refcount = radio.refcount.saturating_sub(1);
                        debug!(
                            radio_id,
                            refcount = radio.refcount,
                            "released managed radio reference"
                        );
                        if radio.refcount == 0 {
                            debug!(
                                radio_id,
                                "releasing final radio reference; shutting down managed radio"
                            );
                            let shutdown_done = Arc::new(Notify::new());
                            done = Some(shutdown_done.clone());
                            shutdown = radio.shutdown.take();
                            task = radio.task.take();
                            *slot = ManagedRadioSlot::ShuttingDown {
                                done: shutdown_done,
                            };
                        }
                    }
                    ManagedRadioSlot::ShuttingDown { .. } => {
                        debug!(
                            radio_id,
                            "release ignored; managed radio already shutting down"
                        );
                    }
                }
            }
        }

        if let Some(shutdown) = shutdown {
            let _ = shutdown.send(());
        }
        if let Some(task) = task {
            let _ = task.await;
        }
        if let Some(done) = done {
            done.notify_waiters();
            let mut radios = self.radios.lock().await;
            if matches!(
                radios.get(&radio_id),
                Some(ManagedRadioSlot::ShuttingDown { .. })
            ) {
                radios.remove(&radio_id);
            }
        }
    }

    pub async fn is_active(&self, radio_id: i64) -> bool {
        matches!(
            self.radios.lock().await.get(&radio_id),
            Some(ManagedRadioSlot::Active(_))
        )
    }

    pub async fn reload_config(&self, radio_id: i64, config: RadioConfig) -> Result<(), String> {
        let command_sender = {
            let radios = self.radios.lock().await;
            match radios.get(&radio_id) {
                Some(ManagedRadioSlot::Active(radio)) => Some(radio.commands.clone()),
                Some(ManagedRadioSlot::ShuttingDown { .. }) | None => None,
            }
        };

        let Some(command_sender) = command_sender else {
            return Ok(());
        };

        debug!(
            radio_id,
            radio_kind = %config.radio_kind,
            transport_kind = %config.transport_kind,
            tcp_host = %config.tcp_host,
            tcp_port = config.tcp_port,
            serial_port = %config.serial_port,
            serial_baud_rate = config.serial_baud_rate,
            poll_frequency = config.poll_frequency,
            cat_timeout = config.cat_timeout,
            cw_keyer_type = %config.cw_keyer_type,
            winkeyer_serial_port = %config.winkeyer_serial_port,
            cw_serial_port = %config.cw_serial_port,
            cw_serial_baud_rate = config.cw_serial_baud_rate,
            cw_serial_line = %config.cw_serial_line,
            "requesting active radio config reload"
        );
        command_sender
            .send(RadioCommand::ReloadConfig(config))
            .await
            .map_err(|_| "radio task unavailable".to_string())
    }
}

impl RadioHandle {
    pub async fn current_status_message(&self) -> ServerMessage {
        ServerMessage::RadioStatus(self.current_status.read().await.clone())
    }

    pub async fn current_message(&self) -> Option<ServerMessage> {
        self.current
            .read()
            .await
            .clone()
            .map(ServerMessage::RadioState)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RadioState> {
        self.updates.subscribe()
    }

    pub fn subscribe_status(&self) -> broadcast::Receiver<RadioStatus> {
        self.status_updates.subscribe()
    }

    pub async fn send_command(
        &self,
        command: RadioCommand,
    ) -> Result<(), mpsc::error::SendError<RadioCommand>> {
        self.commands.send(command).await
    }
}

async fn run_managed_radio(
    mut config: RadioConfig,
    current_status: Arc<RwLock<RadioStatus>>,
    current: Arc<RwLock<Option<RadioState>>>,
    status_updates: broadcast::Sender<RadioStatus>,
    updates: broadcast::Sender<RadioState>,
    mut commands: mpsc::Receiver<RadioCommand>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let mut reconnect_backoff = cat_reconnect_backoff().build();
    let mut reconnect_deadline = None;

    loop {
        let poll_interval = Duration::from_secs_f64(config.poll_frequency);

        if let Some(deadline) = reconnect_deadline {
            loop {
                tokio::select! {
                    _ = &mut shutdown => return,
                    command = commands.recv() => {
                        match command {
                            Some(RadioCommand::ReloadConfig(new_config)) => {
                                info!(radio_id = new_config.id, "reloading radio config while waiting to reconnect CAT");
                                config = new_config;
                                reconnect_backoff = cat_reconnect_backoff().build();
                                reconnect_deadline = None;
                                break;
                            }
                            Some(command) => {
                                warn!(radio_id = config.id, ?command, "dropping radio command while waiting to reconnect CAT");
                                fail_unavailable_radio_command(command, "radio disconnected");
                            }
                            None => return,
                        }
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        reconnect_deadline = None;
                        break;
                    }
                }
            }

            if reconnect_deadline.is_some() {
                continue;
            }
        }

        let radio_kind = match radio_kind_for_config(&config) {
            Ok(radio_kind) => radio_kind,
            Err(error) => {
                set_radio_status(&current_status, &status_updates, false).await;
                reconnect_deadline = Some(next_cat_reconnect_deadline(&mut reconnect_backoff));
                warn!(radio_id = config.id, %error, "radio config has unsupported radio kind");
                continue;
            }
        };
        debug!(
            radio_id = config.id,
            radio_kind = %config.radio_kind,
            transport_kind = %config.transport_kind,
            tcp_host = %config.tcp_host,
            tcp_port = config.tcp_port,
            serial_port = %config.serial_port,
            serial_baud_rate = config.serial_baud_rate,
            poll_frequency = config.poll_frequency,
            cat_timeout = config.cat_timeout,
            shared_cw_serial_port = uses_shared_cw_serial_port(&config),
            "attempting CAT radio connection"
        );

        let connect_config = config.clone();
        let connected = tokio::select! {
            _ = &mut shutdown => return,
            command = commands.recv() => {
                match command {
                    Some(RadioCommand::ReloadConfig(new_config)) => {
                        info!(radio_id = new_config.id, "reloading radio config before CAT connect");
                        config = new_config;
                        continue;
                    }
                    Some(command) => {
                        warn!(radio_id = config.id, ?command, "dropping radio command while CAT is disconnected");
                        fail_unavailable_radio_command(command, "radio disconnected");
                        continue;
                    }
                    None => return,
                }
            }
            result = connect_cat_radio(connect_config, radio_kind) => {
                match result {
                    Ok(connected) => connected,
                    Err(error) => {
                    set_radio_status(&current_status, &status_updates, false).await;
                    reconnect_deadline = Some(next_cat_reconnect_deadline(&mut reconnect_backoff));
                    warn!(
                        radio_id = config.id,
                        radio_kind = %config.radio_kind,
                        transport_kind = %config.transport_kind,
                        %error,
                        "failed to connect to CAT radio"
                    );
                    continue;
                }
            }
            }
        };

        let radio: Arc<dyn ControllableRadio> = connected.radio.into();
        let shared_cw_serial_keyer = connected.shared_cw_serial_keyer;

        reconnect_backoff = cat_reconnect_backoff().build();
        info!(
            radio_id = config.id,
            radio_kind = %config.radio_kind,
            transport_kind = %config.transport_kind,
            "connected to CAT radio"
        );
        set_radio_status(&current_status, &status_updates, true).await;
        let (cw_tx, cw_rx) = mpsc::channel(32);
        let cw_config = config.clone();
        let cw_radio = radio.clone();
        let cw_task = tokio::spawn(async move {
            run_cw_task(cw_config, cw_radio, shared_cw_serial_keyer, cw_rx).await
        });
        let mut interval = tokio::time::interval(poll_interval);
        let mut last_frequency_hz = current
            .read()
            .await
            .as_ref()
            .map(|state| state.frequency_hz)
            .unwrap_or(0);

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    let _ = cw_tx.send(CwTaskCommand::Shutdown).await;
                    let _ = cw_task.await;
                    return;
                }
                _ = interval.tick() => {
                    trace!(radio_id = config.id, "polling radio state");
                    match poll_radio(radio.as_ref()).await {
                        Ok(state) => {
                            last_frequency_hz = state.frequency_hz;
                            *current.write().await = Some(state.clone());
                            trace!(
                                radio_id = config.id,
                                frequency_hz = state.frequency_hz,
                                mode = %state.mode,
                                "polled radio state"
                            );
                            let _ = updates.send(state);
                        }
                        Err(error) => {
                            set_radio_status(&current_status, &status_updates, false).await;
                            reconnect_deadline = Some(next_cat_reconnect_deadline(&mut reconnect_backoff));
                            warn!(radio_id = config.id, %error, "failed to poll CAT radio");
                            let _ = cw_tx.send(CwTaskCommand::Shutdown).await;
                            let _ = cw_task.await;
                            break;
                        }
                    }
                }
                command = commands.recv() => {
                    let Some(command) = command else {
                        let _ = cw_tx.send(CwTaskCommand::Shutdown).await;
                        let _ = cw_task.await;
                        return;
                    };
                    debug!(radio_id = config.id, ?command, "received radio command");
                    match command {
                        RadioCommand::SendMessage { mode, key, fields, completed } => {
                            debug!(radio_id = config.id, mode, key, "forwarding message send command");
                            if let Err(error) = cw_tx.send(CwTaskCommand::SendMessage { mode, key, fields, completed }).await {
                                let CwTaskCommand::SendMessage { completed, .. } = error.0 else { unreachable!() };
                                let _ = completed.send(Err("cw task unavailable".to_string()));
                            }
                        }
                        RadioCommand::SendCwText { text, completed } => {
                            debug!(radio_id = config.id, text, "forwarding cw text send command");
                            if let Err(error) = cw_tx.send(CwTaskCommand::SendText { text, completed }).await {
                                let CwTaskCommand::SendText { completed, .. } = error.0 else { unreachable!() };
                                let _ = completed.send(Err("cw task unavailable".to_string()));
                            }
                        }
                        RadioCommand::StopCw => {
                            debug!(radio_id = config.id, "forwarding cw stop command");
                            let _ = cw_tx.send(CwTaskCommand::Stop).await;
                        }
                        RadioCommand::SetWpm(wpm) => {
                            debug!(radio_id = config.id, wpm, "forwarding cw set_wpm command");
                            let _ = cw_tx.send(CwTaskCommand::SetWpm(wpm)).await;
                        }
                        RadioCommand::ReloadConfig(new_config) => {
                            info!(
                                radio_id = new_config.id,
                                radio_kind = %new_config.radio_kind,
                                transport_kind = %new_config.transport_kind,
                                tcp_host = %new_config.tcp_host,
                                tcp_port = new_config.tcp_port,
                                serial_port = %new_config.serial_port,
                                serial_baud_rate = new_config.serial_baud_rate,
                                poll_frequency = new_config.poll_frequency,
                                cat_timeout = new_config.cat_timeout,
                                cw_keyer_type = %new_config.cw_keyer_type,
                                winkeyer_serial_port = %new_config.winkeyer_serial_port,
                                cw_serial_port = %new_config.cw_serial_port,
                                cw_serial_baud_rate = new_config.cw_serial_baud_rate,
                                cw_serial_line = %new_config.cw_serial_line,
                                "reloading active radio config"
                            );
                            set_radio_status(&current_status, &status_updates, false).await;
                            debug!(radio_id = config.id, "shutting down cw task for radio config reload");
                            let _ = cw_tx.send(CwTaskCommand::Shutdown).await;
                            let _ = cw_task.await;
                            debug!(radio_id = config.id, "dropping CAT radio for config reload");
                            config = new_config;
                            break;
                        }
                        command => {
                            debug!(radio_id = config.id, ?command, last_frequency_hz, "applying CAT radio command");
                            if let Err(error) = apply_command(radio.as_ref(), command, last_frequency_hz).await {
                                set_radio_status(&current_status, &status_updates, false).await;
                                reconnect_deadline = Some(next_cat_reconnect_deadline(&mut reconnect_backoff));
                                error!(radio_id = config.id, %error, "failed to apply radio command");
                                let _ = cw_tx.send(CwTaskCommand::Shutdown).await;
                                let _ = cw_task.await;
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn set_radio_status(
    current_status: &Arc<RwLock<RadioStatus>>,
    status_updates: &broadcast::Sender<RadioStatus>,
    online: bool,
) {
    let mut status = current_status.write().await;
    if status.online == online {
        trace!(online, "radio status unchanged; not broadcasting");
        return;
    }
    trace!(
        previous_online = status.online,
        next_online = online,
        "updating and broadcasting radio status"
    );
    status.online = online;
    let _ = status_updates.send(status.clone());
}

fn fail_pending_cw_sends(pending: &mut VecDeque<PendingCwSend>, reason: &str) {
    while let Some(send) = pending.pop_front() {
        let _ = send.completed.send(Err(reason.to_string()));
    }
}

fn fail_unavailable_radio_command(command: RadioCommand, reason: &str) {
    match command {
        RadioCommand::SendMessage { completed, .. }
        | RadioCommand::SendCwText { completed, .. } => {
            let _ = completed.send(Err(reason.to_string()));
        }
        RadioCommand::StopCw
        | RadioCommand::SetWpm(_)
        | RadioCommand::SetFrequency(_)
        | RadioCommand::SetMode(_)
        | RadioCommand::ReloadConfig(_) => {}
    }
}

struct ConnectedCatRadio {
    radio: Box<dyn ControllableRadio>,
    shared_cw_serial_keyer: Option<CwSerialDevice>,
}

async fn connect_cat_radio(
    config: RadioConfig,
    radio_kind: RadioKind,
) -> Result<ConnectedCatRadio, String> {
    if uses_shared_cw_serial_port(&config) {
        warn!(
            radio_id = config.id,
            "shared CAT/CW serial port is not supported by this build; falling back to independent connections"
        );
    }

    let connection = connection_config_for(&config)?;
    let radio = create_radio(radio_kind, connection)
        .await
        .map_err(|error| error.to_string())?;

    Ok(ConnectedCatRadio {
        radio,
        shared_cw_serial_keyer: None,
    })
}

fn uses_shared_cw_serial_port(config: &RadioConfig) -> bool {
    config.transport_kind.trim().eq_ignore_ascii_case("serial")
        && config.cw_keyer_type.trim().eq_ignore_ascii_case("serial")
        && !config.serial_port.trim().is_empty()
        && config.serial_port.trim() == config.cw_serial_port.trim()
}

fn radio_kind_for_config(config: &RadioConfig) -> Result<RadioKind, String> {
    let parsed = config
        .radio_kind
        .trim()
        .parse::<RadioKind>()
        .map_err(|error| error.to_string())?;
    trace!(
        radio_id = config.id,
        configured_radio_kind = %config.radio_kind,
        parsed_radio_kind = parsed.as_str(),
        "parsed radio kind for CAT factory"
    );
    Ok(parsed)
}

fn connection_config_for(config: &RadioConfig) -> Result<ConnectionConfig, String> {
    let timeout = Duration::from_secs_f64(config.cat_timeout);
    trace!(
        radio_id = config.id,
        transport_kind = %config.transport_kind,
        cat_timeout = config.cat_timeout,
        timeout_ms = timeout.as_millis(),
        "building CAT connection config"
    );

    match config.transport_kind.trim().to_ascii_lowercase().as_str() {
        "tcp" => {
            let host = config.tcp_host.trim();
            if host.is_empty() {
                return Err("TCP host is required".to_string());
            }
            if config.tcp_port == 0 {
                return Err("TCP port must be between 1 and 65535".to_string());
            }
            trace!(
                radio_id = config.id,
                tcp_host = host,
                tcp_port = config.tcp_port,
                timeout_ms = timeout.as_millis(),
                "built TCP CAT connection config"
            );
            Ok(ConnectionConfig::tcp(host, config.tcp_port).with_timeout(timeout))
        }
        "serial" => {
            let serial_port = config.serial_port.trim();
            if serial_port.is_empty() {
                return Err("serial port is required".to_string());
            }
            if config.serial_baud_rate == 0 {
                return Err("serial baud rate must be greater than 0".to_string());
            }
            trace!(
                radio_id = config.id,
                serial_port,
                serial_baud_rate = config.serial_baud_rate,
                timeout_ms = timeout.as_millis(),
                "built serial CAT connection config"
            );
            Ok(
                ConnectionConfig::serial(serial_port, config.serial_baud_rate)
                    .with_timeout(timeout),
            )
        }
        other => Err(format!("unsupported transport kind `{other}`")),
    }
}

fn cat_reconnect_backoff() -> ExponentialBuilder {
    ExponentialBuilder::default()
        .with_min_delay(CAT_RECONNECT_MIN_DELAY)
        .with_max_delay(CAT_RECONNECT_MAX_DELAY)
        .without_max_times()
}

fn next_cat_reconnect_deadline(
    reconnect_backoff: &mut impl Iterator<Item = Duration>,
) -> tokio::time::Instant {
    let delay = reconnect_backoff.next().unwrap_or(CAT_RECONNECT_MAX_DELAY);
    debug!(
        reconnect_delay_ms = delay.as_millis(),
        reconnect_delay_secs = delay.as_secs_f64(),
        "scheduled next CAT reconnect attempt"
    );
    tokio::time::Instant::now() + delay
}

enum CwTaskCommand {
    SendMessage {
        mode: String,
        key: String,
        fields: serde_json::Map<String, serde_json::Value>,
        completed: oneshot::Sender<Result<(), String>>,
    },
    SendText {
        text: String,
        completed: oneshot::Sender<Result<(), String>>,
    },
    Stop,
    SetWpm(u8),
    Shutdown,
}

enum PendingCwPayload {
    Message {
        mode: String,
        key: String,
        fields: serde_json::Map<String, serde_json::Value>,
    },
    Text {
        text: String,
    },
}

struct PendingCwSend {
    payload: PendingCwPayload,
    completed: oneshot::Sender<Result<(), String>>,
}

async fn run_cw_task(
    config: RadioConfig,
    radio: Arc<dyn ControllableRadio>,
    shared_cw_serial_keyer: Option<CwSerialDevice>,
    mut commands: mpsc::Receiver<CwTaskCommand>,
) {
    let mut controller = CwController::new(&config, radio, shared_cw_serial_keyer).await;
    let mut pending = VecDeque::new();

    loop {
        let (next_send, prepend_space) = if let Some(send) = pending.pop_front() {
            (Some(send), true)
        } else {
            match commands.recv().await {
                Some(CwTaskCommand::SendMessage {
                    mode,
                    key,
                    fields,
                    completed,
                }) => (
                    Some(PendingCwSend {
                        payload: PendingCwPayload::Message { mode, key, fields },
                        completed,
                    }),
                    false,
                ),
                Some(CwTaskCommand::SendText { text, completed }) => (
                    Some(PendingCwSend {
                        payload: PendingCwPayload::Text { text },
                        completed,
                    }),
                    false,
                ),
                Some(CwTaskCommand::Stop) => {
                    debug!(radio_id = config.id, "cw task received stop command");
                    controller.stop().await;
                    continue;
                }
                Some(CwTaskCommand::SetWpm(wpm)) => {
                    debug!(
                        radio_id = config.id,
                        wpm, "cw task received set_wpm command"
                    );
                    controller.set_wpm(wpm).await;
                    continue;
                }
                Some(CwTaskCommand::Shutdown) | None => {
                    debug!(radio_id = config.id, "cw task received shutdown command");
                    fail_pending_cw_sends(&mut pending, "cw shutdown");
                    break;
                }
            }
        };

        let Some(send) = next_send else {
            continue;
        };
        debug!(
            radio_id = config.id,
            pending_count = pending.len(),
            "cw task starting queued send"
        );
        let result = match &send.payload {
            PendingCwPayload::Message { mode, key, fields } => {
                controller
                    .send_message(
                        mode,
                        key,
                        fields,
                        prepend_space,
                        &mut commands,
                        &mut pending,
                    )
                    .await
            }
            PendingCwPayload::Text { text } => {
                controller
                    .send_text(text, &mut commands, &mut pending)
                    .await
            }
        };
        debug!(
            radio_id = config.id,
            ?result,
            remaining_pending = pending.len(),
            "cw task send command finished"
        );
        let _ = send.completed.send(result);
    }

    controller.close().await;
}

struct CwController {
    radio_id: i64,
    radio: Arc<dyn ControllableRadio>,
    messages: String,
    keyer: Option<Box<dyn CwKeyer>>,
}

impl CwController {
    async fn new(
        config: &RadioConfig,
        radio: Arc<dyn ControllableRadio>,
        shared_cw_serial_keyer: Option<CwSerialDevice>,
    ) -> Self {
        let mut controller = Self {
            radio_id: config.id,
            radio: radio.clone(),
            messages: config.cw_messages.clone(),
            keyer: cw_keyer_for_config(config, radio, shared_cw_serial_keyer).await,
        };
        controller.connect().await;
        controller
    }

    async fn connect(&mut self) {
        if let Some(keyer) = self.keyer.as_mut() {
            keyer.connect(self.radio_id).await;
        } else {
            debug!(radio_id = self.radio_id, "CW keying disabled for radio");
        }
    }

    async fn send_message(
        &mut self,
        mode: &str,
        key: &str,
        fields: &serde_json::Map<String, serde_json::Value>,
        prepend_space: bool,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        if !self.radio_mode_is_cw().await {
            debug!(
                radio_id = self.radio_id,
                mode, key, "ignoring message send; radio mode is not CW/CW-R"
            );
            return Ok(());
        }

        let Some(rendered_text) = cw::render(&self.messages, mode, key, fields) else {
            warn!(radio_id = self.radio_id, mode, key, "unknown cw message");
            return Err("unknown cw message".to_string());
        };
        if rendered_text.is_empty() {
            debug!(
                radio_id = self.radio_id,
                mode, key, "ignoring empty cw message"
            );
            return Ok(());
        }
        let text = cw_send_text(rendered_text, prepend_space);
        debug!(radio_id = self.radio_id, mode, key, text, "sending cw text");
        let completion = {
            let Some(keyer) = self.keyer.as_mut() else {
                debug!(
                    radio_id = self.radio_id,
                    "ignoring CW send; no CW keyer configured"
                );
                return Err("cw keyer unavailable".to_string());
            };
            let keyer_name = keyer.name();
            let completion = keyer.send_text(self.radio_id, &text).await?;
            debug!(
                radio_id = self.radio_id,
                mode,
                key,
                keyer = keyer_name,
                "cw text queued"
            );
            completion
        };

        match completion {
            CwSendCompletion::PollStatus { wait_for_busy } => {
                if wait_for_busy {
                    debug!(
                        radio_id = self.radio_id,
                        mode, key, "waiting for cw keyer busy"
                    );
                    self.wait_until_busy_or_stopped(commands, pending).await?;
                }
                debug!(
                    radio_id = self.radio_id,
                    mode, key, "waiting for cw keyer idle"
                );
                let result = self.wait_until_idle_or_stopped(commands, pending).await;
                debug!(
                    radio_id = self.radio_id,
                    mode,
                    key,
                    ?result,
                    "finished waiting for cw keyer idle"
                );
                result
            }
            CwSendCompletion::Estimated(estimated_duration) => {
                debug!(
                    radio_id = self.radio_id,
                    mode,
                    key,
                    estimated_duration_ms = estimated_duration.as_millis(),
                    "waiting for estimated CW completion"
                );
                self.wait_until_deadline_or_stopped(
                    tokio::time::Instant::now() + estimated_duration,
                    commands,
                    pending,
                )
                .await
            }
        }
    }

    async fn radio_mode_is_cw(&mut self) -> bool {
        match self.radio.get_mode().await {
            Ok(mode) => {
                let logger_mode = normalize_mode(&mode);
                logger_mode == "CW" || logger_mode == "CW-R"
            }
            Err(error) => {
                warn!(
                    radio_id = self.radio_id,
                    %error,
                    "unable to read radio mode before message send"
                );
                false
            }
        }
    }

    async fn send_text(
        &mut self,
        text: &str,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        if text.trim().is_empty() {
            return Ok(());
        }

        debug!(radio_id = self.radio_id, text, "sending cw text");
        let completion = {
            let Some(keyer) = self.keyer.as_mut() else {
                debug!(
                    radio_id = self.radio_id,
                    "ignoring CW send; no CW keyer configured"
                );
                return Err("cw keyer unavailable".to_string());
            };
            let keyer_name = keyer.name();
            let completion = keyer.send_text(self.radio_id, text).await?;
            debug!(
                radio_id = self.radio_id,
                keyer = keyer_name,
                "cw text queued"
            );
            completion
        };

        match completion {
            CwSendCompletion::PollStatus { wait_for_busy } => {
                if wait_for_busy {
                    self.wait_until_busy_or_stopped(commands, pending).await?;
                }
                self.wait_until_idle_or_stopped(commands, pending).await
            }
            CwSendCompletion::Estimated(estimated_duration) => {
                self.wait_until_deadline_or_stopped(
                    tokio::time::Instant::now() + estimated_duration,
                    commands,
                    pending,
                )
                .await
            }
        }
    }

    async fn wait_until_busy_or_stopped(
        &mut self,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            tokio::select! {
                command = commands.recv() => {
                    match command {
                        Some(CwTaskCommand::Stop) => {
                            debug!(radio_id = self.radio_id, "stop command interrupting cw busy wait");
                            fail_pending_cw_sends(pending, "cw stopped");
                            self.stop().await;
                            return Err("cw stopped".to_string());
                        }
                        Some(CwTaskCommand::SetWpm(wpm)) => {
                            debug!(radio_id = self.radio_id, wpm, "set_wpm command received during cw busy wait");
                            self.set_wpm(wpm).await;
                        }
                        Some(CwTaskCommand::Shutdown) | None => {
                            debug!(radio_id = self.radio_id, "shutdown interrupting cw busy wait");
                            fail_pending_cw_sends(pending, "cw shutdown");
                            self.stop().await;
                            return Err("cw shutdown".to_string());
                        }
                        Some(CwTaskCommand::SendMessage {
                            mode,
                            key,
                            fields,
                            completed,
                        }) => {
                            debug!(radio_id = self.radio_id, mode, key, pending_count = pending.len(), "queueing message send command while busy");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Message { mode, key, fields },
                                completed,
                            });
                        }
                        Some(CwTaskCommand::SendText { text, completed }) => {
                            debug!(radio_id = self.radio_id, text, pending_count = pending.len(), "queueing cw text send command while busy");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Text { text },
                                completed,
                            });
                        }
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    warn!(radio_id = self.radio_id, "timed out waiting for cw keyer to become busy");
                    return Err("cw keyer did not become busy".to_string());
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    match self.keyer_status().await {
                        Ok(status) if status.busy => {
                            debug!(radio_id = self.radio_id, "cw keyer is busy");
                            return Ok(());
                        }
                        Ok(_) => {}
                        Err(error) => {
                            warn!(radio_id = self.radio_id, %error, "failed waiting for cw keyer busy");
                            return Err(error);
                        }
                    }
                }
            }
        }
    }

    async fn wait_until_idle_or_stopped(
        &mut self,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        loop {
            tokio::select! {
                command = commands.recv() => {
                    match command {
                        Some(CwTaskCommand::Stop) => {
                            debug!(radio_id = self.radio_id, "stop command interrupting cw idle wait");
                            fail_pending_cw_sends(pending, "cw stopped");
                            self.stop().await;
                            return Err("cw stopped".to_string());
                        }
                        Some(CwTaskCommand::SetWpm(wpm)) => {
                            debug!(radio_id = self.radio_id, wpm, "set_wpm command received during cw idle wait");
                            self.set_wpm(wpm).await;
                        }
                        Some(CwTaskCommand::Shutdown) | None => {
                            debug!(radio_id = self.radio_id, "shutdown interrupting cw idle wait");
                            fail_pending_cw_sends(pending, "cw shutdown");
                            self.stop().await;
                            return Err("cw shutdown".to_string());
                        }
                        Some(CwTaskCommand::SendMessage {
                            mode,
                            key,
                            fields,
                            completed,
                        }) => {
                            debug!(radio_id = self.radio_id, mode, key, pending_count = pending.len(), "queueing message send command while busy");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Message { mode, key, fields },
                                completed,
                            });
                        }
                        Some(CwTaskCommand::SendText { text, completed }) => {
                            debug!(radio_id = self.radio_id, text, pending_count = pending.len(), "queueing cw text send command while busy");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Text { text },
                                completed,
                            });
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    match self.keyer_status().await {
                        Ok(status) if !status.busy => {
                            debug!(radio_id = self.radio_id, "cw keyer is idle");
                            return Ok(());
                        }
                        Ok(_) => {}
                        Err(error) => {
                            warn!(radio_id = self.radio_id, %error, "failed waiting for cw keyer idle");
                            return Err(error);
                        }
                    }
                }
            }
        }
    }

    async fn wait_until_deadline_or_stopped(
        &mut self,
        deadline: tokio::time::Instant,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        loop {
            tokio::select! {
                command = commands.recv() => {
                    match command {
                        Some(CwTaskCommand::Stop) => {
                            debug!(radio_id = self.radio_id, "stop command interrupting estimated cw wait");
                            fail_pending_cw_sends(pending, "cw stopped");
                            self.stop().await;
                            return Err("cw stopped".to_string());
                        }
                        Some(CwTaskCommand::SetWpm(wpm)) => {
                            debug!(radio_id = self.radio_id, wpm, "set_wpm command received during estimated cw wait");
                            self.set_wpm(wpm).await;
                        }
                        Some(CwTaskCommand::Shutdown) | None => {
                            debug!(radio_id = self.radio_id, "shutdown interrupting estimated cw wait");
                            fail_pending_cw_sends(pending, "cw shutdown");
                            self.stop().await;
                            return Err("cw shutdown".to_string());
                        }
                        Some(CwTaskCommand::SendMessage {
                            mode,
                            key,
                            fields,
                            completed,
                        }) => {
                            debug!(radio_id = self.radio_id, mode, key, pending_count = pending.len(), "queueing message send command while busy");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Message { mode, key, fields },
                                completed,
                            });
                        }
                        Some(CwTaskCommand::SendText { text, completed }) => {
                            debug!(radio_id = self.radio_id, text, pending_count = pending.len(), "queueing cw text send command while busy");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Text { text },
                                completed,
                            });
                        }
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    debug!(radio_id = self.radio_id, "estimated CW send duration elapsed");
                    return Ok(());
                }
            }
        }
    }

    async fn stop(&mut self) {
        if let Some(keyer) = self.keyer.as_mut() {
            let keyer_name = keyer.name();
            debug!(
                radio_id = self.radio_id,
                keyer = keyer_name,
                "clearing cw keyer buffer"
            );
            if let Err(error) = keyer.clear_buffer(self.radio_id).await {
                warn!(radio_id = self.radio_id, keyer = keyer_name, %error, "failed to clear cw keyer buffer");
            } else {
                debug!(
                    radio_id = self.radio_id,
                    keyer = keyer_name,
                    "cw keyer buffer cleared"
                );
            }
        }
    }

    async fn set_wpm(&mut self, wpm: u8) {
        if let Some(keyer) = self.keyer.as_mut() {
            let keyer_name = keyer.name();
            debug!(
                radio_id = self.radio_id,
                keyer = keyer_name,
                wpm,
                "setting cw keyer wpm"
            );
            if let Err(error) = keyer.set_wpm(self.radio_id, wpm).await {
                warn!(radio_id = self.radio_id, keyer = keyer_name, wpm, %error, "failed to set cw keyer wpm");
            } else {
                debug!(
                    radio_id = self.radio_id,
                    keyer = keyer_name,
                    wpm,
                    "cw keyer wpm set"
                );
            }
        }
    }

    async fn close(&mut self) {
        if let Some(keyer) = self.keyer.as_mut() {
            keyer.close(self.radio_id).await;
        }
    }

    async fn keyer_status(&mut self) -> Result<CwKeyerStatus, String> {
        let Some(keyer) = self.keyer.as_mut() else {
            return Err("cw keyer unavailable".to_string());
        };
        let keyer_name = keyer.name();
        keyer
            .status(self.radio_id)
            .await?
            .ok_or_else(|| format!("{keyer_name} keyer does not report status"))
    }
}

async fn cw_keyer_for_config(
    config: &RadioConfig,
    radio: Arc<dyn ControllableRadio>,
    shared_cw_serial_keyer: Option<CwSerialDevice>,
) -> Option<Box<dyn CwKeyer>> {
    match config.cw_keyer_type.trim().to_ascii_lowercase().as_str() {
        "winkeyer" => Some(Box::new(WinkeyerKeyer {
            serial_port: config.winkeyer_serial_port.clone(),
            device: None,
        })),
        "cat" => Some(Box::new(CatKeyer::open(config.id, radio).await)),
        "serial" => Some(Box::new(SerialLineKeyer {
            serial_port: config.cw_serial_port.clone(),
            baud_rate: config.cw_serial_baud_rate,
            line: config.cw_serial_line.clone(),
            device: shared_cw_serial_keyer,
        })),
        _ => None,
    }
}

struct CwKeyerStatus {
    busy: bool,
}

enum CwSendCompletion {
    PollStatus { wait_for_busy: bool },
    Estimated(Duration),
}

trait CwKeyer: Send {
    fn name(&self) -> &'static str;
    fn connect<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()>;
    fn send_text<'a>(
        &'a mut self,
        radio_id: i64,
        text: &'a str,
    ) -> BoxFuture<'a, Result<CwSendCompletion, String>>;
    fn status<'a>(
        &'a mut self,
        radio_id: i64,
    ) -> BoxFuture<'a, Result<Option<CwKeyerStatus>, String>>;
    fn clear_buffer<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, Result<(), String>>;
    fn set_wpm<'a>(&'a mut self, radio_id: i64, wpm: u8) -> BoxFuture<'a, Result<(), String>>;
    fn close<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()>;
}

impl CwKeyer for CatKeyer {
    fn name(&self) -> &'static str {
        "CAT"
    }

    fn connect<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()> {
        async move {
            debug!(radio_id, "CAT CW keyer ready");
        }
        .boxed()
    }

    fn send_text<'a>(
        &'a mut self,
        _radio_id: i64,
        text: &'a str,
    ) -> BoxFuture<'a, Result<CwSendCompletion, String>> {
        async move {
            CatKeyer::send_text(self, text).await?;
            Ok(CwSendCompletion::Estimated(
                self.estimated_send_duration(text),
            ))
        }
        .boxed()
    }

    fn status<'a>(
        &'a mut self,
        _radio_id: i64,
    ) -> BoxFuture<'a, Result<Option<CwKeyerStatus>, String>> {
        async move { Ok(None) }.boxed()
    }

    fn clear_buffer<'a>(&'a mut self, _radio_id: i64) -> BoxFuture<'a, Result<(), String>> {
        async move { CatKeyer::clear_buffer(self).await }.boxed()
    }

    fn set_wpm<'a>(&'a mut self, _radio_id: i64, wpm: u8) -> BoxFuture<'a, Result<(), String>> {
        async move { CatKeyer::set_wpm(self, wpm).await }.boxed()
    }

    fn close<'a>(&'a mut self, _radio_id: i64) -> BoxFuture<'a, ()> {
        async move {
            CatKeyer::close(self).await;
        }
        .boxed()
    }
}

struct WinkeyerKeyer {
    serial_port: String,
    device: Option<winkeyer::WinKeyer>,
}

impl CwKeyer for WinkeyerKeyer {
    fn name(&self) -> &'static str {
        "Winkeyer"
    }

    fn connect<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()> {
        async move {
            connect_winkeyer(radio_id, self).await;
        }
        .boxed()
    }

    fn send_text<'a>(
        &'a mut self,
        radio_id: i64,
        text: &'a str,
    ) -> BoxFuture<'a, Result<CwSendCompletion, String>> {
        async move {
            let result = {
                let Some(winkeyer) = ensure_winkeyer_connected(radio_id, self).await else {
                    return Err("winkeyer unavailable".to_string());
                };
                winkeyer.send_text(text).await
            };
            if let Err(error) = result {
                warn!(radio_id, %error, "failed to send cw text through winkeyer");
                self.device = None;
                return Err(error.to_string());
            }
            Ok(CwSendCompletion::PollStatus {
                wait_for_busy: true,
            })
        }
        .boxed()
    }

    fn status<'a>(
        &'a mut self,
        radio_id: i64,
    ) -> BoxFuture<'a, Result<Option<CwKeyerStatus>, String>> {
        async move {
            let result = {
                let Some(winkeyer) = ensure_winkeyer_connected(radio_id, self).await else {
                    return Err("winkeyer unavailable".to_string());
                };
                winkeyer.status().await
            };
            match result {
                Ok(status) => Ok(Some(CwKeyerStatus {
                    busy: status.busy || status.wait || status.key_down,
                })),
                Err(error) => {
                    warn!(radio_id, %error, "failed to read winkeyer status");
                    self.device = None;
                    Err(error.to_string())
                }
            }
        }
        .boxed()
    }

    fn clear_buffer<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, Result<(), String>> {
        async move {
            let result = {
                let Some(winkeyer) = ensure_winkeyer_connected(radio_id, self).await else {
                    return Err("winkeyer unavailable".to_string());
                };
                winkeyer.clear_buffer().await
            };
            if let Err(error) = result {
                self.device = None;
                return Err(error.to_string());
            }
            Ok(())
        }
        .boxed()
    }

    fn set_wpm<'a>(&'a mut self, radio_id: i64, wpm: u8) -> BoxFuture<'a, Result<(), String>> {
        async move {
            let result = {
                let Some(winkeyer) = ensure_winkeyer_connected(radio_id, self).await else {
                    return Err("winkeyer unavailable".to_string());
                };
                winkeyer.set_wpm(wpm).await
            };
            if let Err(error) = result {
                self.device = None;
                return Err(error.to_string());
            }
            Ok(())
        }
        .boxed()
    }

    fn close<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()> {
        async move {
            if let Some(mut winkeyer) = self.device.take()
                && let Err(error) = winkeyer.close().await
            {
                warn!(radio_id, %error, "failed to close winkeyer");
            }
        }
        .boxed()
    }
}

struct SerialLineKeyer {
    serial_port: String,
    baud_rate: u32,
    line: String,
    device: Option<CwSerialDevice>,
}

impl CwKeyer for SerialLineKeyer {
    fn name(&self) -> &'static str {
        "Serial"
    }

    fn connect<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()> {
        async move {
            connect_serial_keyer(radio_id, self).await;
        }
        .boxed()
    }

    fn send_text<'a>(
        &'a mut self,
        radio_id: i64,
        text: &'a str,
    ) -> BoxFuture<'a, Result<CwSendCompletion, String>> {
        async move {
            let result = {
                let Some(serial_keyer) = ensure_serial_keyer_connected(radio_id, self).await else {
                    return Err("serial CW keyer unavailable".to_string());
                };
                serial_keyer.send_text(text).await
            };
            if let Err(error) = result {
                warn!(radio_id, %error, "failed to send cw text through serial keyer");
                self.device = None;
                return Err(error.to_string());
            }
            Ok(CwSendCompletion::PollStatus {
                wait_for_busy: false,
            })
        }
        .boxed()
    }

    fn status<'a>(
        &'a mut self,
        radio_id: i64,
    ) -> BoxFuture<'a, Result<Option<CwKeyerStatus>, String>> {
        async move {
            let result = {
                let Some(serial_keyer) = ensure_serial_keyer_connected(radio_id, self).await else {
                    return Err("serial CW keyer unavailable".to_string());
                };
                serial_keyer.status().await
            };
            match result {
                Ok(status) => Ok(Some(CwKeyerStatus {
                    busy: status.busy
                        || status.key_down
                        || status.ptt_on
                        || status.queued_messages > 0,
                })),
                Err(error) => {
                    warn!(radio_id, %error, "failed to read serial CW keyer status");
                    self.device = None;
                    Err(error.to_string())
                }
            }
        }
        .boxed()
    }

    fn clear_buffer<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, Result<(), String>> {
        async move {
            let result = {
                let Some(serial_keyer) = ensure_serial_keyer_connected(radio_id, self).await else {
                    return Err("serial CW keyer unavailable".to_string());
                };
                serial_keyer.clear_buffer().await
            };
            if let Err(error) = result {
                self.device = None;
                return Err(error.to_string());
            }
            Ok(())
        }
        .boxed()
    }

    fn set_wpm<'a>(&'a mut self, radio_id: i64, wpm: u8) -> BoxFuture<'a, Result<(), String>> {
        async move {
            let result = {
                let Some(serial_keyer) = ensure_serial_keyer_connected(radio_id, self).await else {
                    return Err("serial CW keyer unavailable".to_string());
                };
                serial_keyer.set_wpm(wpm).await
            };
            if let Err(error) = result {
                self.device = None;
                return Err(error.to_string());
            }
            Ok(())
        }
        .boxed()
    }

    fn close<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()> {
        async move {
            if let Some(mut serial_keyer) = self.device.take()
                && let Err(error) = serial_keyer.close().await
            {
                warn!(radio_id, %error, "failed to close serial CW keyer");
            }
        }
        .boxed()
    }
}

fn cw_send_text(text: String, prepend_space: bool) -> String {
    if prepend_space {
        format!(" {text}")
    } else {
        text
    }
}

fn cw_serial_control_line(line: &str) -> ControlLine {
    match line.trim().to_ascii_lowercase().as_str() {
        "rts" => ControlLine::Rts,
        _ => ControlLine::Dtr,
    }
}

fn cw_serial_config(serial_port: &str, baud_rate: u32, line: &str) -> CwSerialConfig {
    CwSerialConfig::new(serial_port)
        .baud_rate(baud_rate)
        .key_line(cw_serial_control_line(line))
        .ptt_line(None)
}

async fn open_serial_keyer(
    serial_port: &str,
    baud_rate: u32,
    line: &str,
) -> cw_serial_keyer::Result<CwSerialDevice> {
    let config = cw_serial_config(serial_port, baud_rate, line);
    let mut serial_keyer = CwSerialDevice::open_with_config(config).await?;
    serial_keyer.set_timeout(Duration::from_millis(500));
    Ok(serial_keyer)
}

async fn connect_serial_keyer(radio_id: i64, keyer: &mut SerialLineKeyer) {
    if keyer.serial_port.trim().is_empty() {
        warn!(radio_id, "Serial CW keying selected without a serial port");
        return;
    }
    if keyer.device.is_some() {
        return;
    }

    match open_serial_keyer(&keyer.serial_port, keyer.baud_rate, &keyer.line).await {
        Ok(serial_keyer) => {
            info!(
                radio_id,
                serial_port = %keyer.serial_port,
                baud_rate = keyer.baud_rate,
                line = %keyer.line,
                "connected to serial CW keyer"
            );
            keyer.device = Some(serial_keyer);
        }
        Err(error) => {
            warn!(
                radio_id,
                serial_port = %keyer.serial_port,
                baud_rate = keyer.baud_rate,
                line = %keyer.line,
                %error,
                "failed to connect to serial CW keyer"
            );
            keyer.device = None;
        }
    }
}

async fn ensure_serial_keyer_connected(
    radio_id: i64,
    keyer: &mut SerialLineKeyer,
) -> Option<&mut CwSerialDevice> {
    if keyer.device.is_none() {
        connect_serial_keyer(radio_id, keyer).await;
    }
    keyer.device.as_mut()
}

async fn connect_winkeyer(radio_id: i64, keyer: &mut WinkeyerKeyer) {
    if keyer.serial_port.trim().is_empty() {
        warn!(
            radio_id,
            "Winkeyer CW keying selected without a serial port"
        );
        return;
    }
    if keyer.device.is_some() {
        return;
    }

    match winkeyer::WinKeyer::open(&keyer.serial_port).await {
        Ok((mut winkeyer, revision)) => {
            winkeyer.set_timeout(Duration::from_millis(500));
            info!(
                radio_id,
                serial_port = %keyer.serial_port,
                revision,
                "connected to winkeyer"
            );
            keyer.device = Some(winkeyer);
        }
        Err(error) => {
            warn!(
                radio_id,
                serial_port = %keyer.serial_port,
                %error,
                "failed to connect to winkeyer"
            );
            keyer.device = None;
        }
    }
}

async fn ensure_winkeyer_connected(
    radio_id: i64,
    keyer: &mut WinkeyerKeyer,
) -> Option<&mut winkeyer::WinKeyer> {
    if keyer.device.is_none() {
        connect_winkeyer(radio_id, keyer).await;
    }
    keyer.device.as_mut()
}

async fn poll_radio(radio: &dyn ControllableRadio) -> Result<RadioState, radio_cat_rs::RadioError> {
    let frequency_hz = radio.get_frequency().await?.hz();
    let mode = radio.get_mode().await?;
    trace!(
        frequency_hz,
        raw_mode = %mode,
        "polled raw CAT radio state"
    );

    Ok(RadioState {
        frequency_hz,
        mode: normalize_mode(&mode),
    })
}

async fn apply_command(
    radio: &dyn ControllableRadio,
    command: RadioCommand,
    last_frequency_hz: u64,
) -> Result<(), radio_cat_rs::RadioError> {
    match command {
        RadioCommand::SetFrequency(frequency_hz) => {
            debug!(frequency_hz, "setting CAT radio frequency");
            radio
                .set_frequency(radio_cat_rs::Frequency::from_hz(frequency_hz))
                .await
        }
        RadioCommand::SetMode(mode) => {
            let frequency_hz = if last_frequency_hz == 0 {
                radio.get_frequency().await?.hz()
            } else {
                last_frequency_hz
            };
            trace!(
                requested_mode = %mode,
                resolved_frequency_hz = frequency_hz,
                "translating CAT mode request"
            );

            let radio_modes = mode_candidates_for_request(&mode, frequency_hz);
            if radio_modes.is_empty() {
                debug!(mode, frequency_hz, "ignoring unsupported CAT radio mode");
                return Ok(());
            }

            let mut last_error = None;
            for radio_mode in radio_modes {
                match radio.set_mode(radio_mode).await {
                    Ok(()) => {
                        debug!(
                            requested_mode = %mode,
                            applied_mode = %radio_mode,
                            resolved_frequency_hz = frequency_hz,
                            "setting CAT radio mode"
                        );
                        return Ok(());
                    }
                    Err(error) => {
                        warn!(
                            requested_mode = %mode,
                            attempted_mode = %radio_mode,
                            resolved_frequency_hz = frequency_hz,
                            %error,
                            "failed to set CAT radio mode candidate"
                        );
                        last_error = Some(error);
                    }
                }
            }

            if let Some(error) = last_error {
                Err(error)
            } else {
                Ok(())
            }
        }
        RadioCommand::SendMessage { .. }
        | RadioCommand::SendCwText { .. }
        | RadioCommand::StopCw
        | RadioCommand::SetWpm(_)
        | RadioCommand::ReloadConfig(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn test_config() -> RadioConfig {
        RadioConfig {
            id: 1,
            name: "Test".to_string(),
            radio_kind: "k4".to_string(),
            transport_kind: "tcp".to_string(),
            tcp_host: "127.0.0.1".to_string(),
            tcp_port: 5002,
            serial_port: String::new(),
            serial_baud_rate: 115_200,
            poll_frequency: 0.25,
            cat_timeout: 2.0,
            cw_keyer_type: "none".to_string(),
            winkeyer_serial_port: String::new(),
            cw_serial_port: String::new(),
            cw_serial_baud_rate: 9_600,
            cw_serial_line: "dtr".to_string(),
            cw_messages: String::new(),
        }
    }

    #[test]
    fn cat_reconnect_backoff_starts_at_one_second() {
        let mut backoff = cat_reconnect_backoff().build();
        assert_eq!(backoff.next(), Some(Duration::from_secs(1)));
    }

    #[test]
    fn cat_reconnect_backoff_caps_at_max_delay() {
        let mut backoff = cat_reconnect_backoff().build();
        let delays = (0..6)
            .map(|_| backoff.next().expect("backoff should continue"))
            .collect::<Vec<_>>();

        assert_eq!(
            delays,
            vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8),
                Duration::from_secs(10),
                Duration::from_secs(10),
            ]
        );
    }

    #[test]
    fn builds_tcp_connection_config_with_timeout() {
        let config = test_config();

        let connection = connection_config_for(&config).expect("connection should build");

        match connection {
            ConnectionConfig::Tcp {
                host,
                port,
                timeout,
            } => {
                assert_eq!(host, "127.0.0.1");
                assert_eq!(port, 5002);
                assert_eq!(timeout, Duration::from_secs(2));
            }
            ConnectionConfig::Serial { .. } => panic!("expected tcp config"),
        }
    }

    #[test]
    fn builds_serial_connection_config_with_timeout() {
        let mut config = test_config();
        config.transport_kind = "serial".to_string();
        config.tcp_host = String::new();
        config.tcp_port = 0;
        config.serial_port = "/dev/ttyUSB0".to_string();
        config.serial_baud_rate = 57_600;

        let connection = connection_config_for(&config).expect("connection should build");

        match connection {
            ConnectionConfig::Serial {
                path,
                baud_rate,
                timeout,
            } => {
                assert_eq!(path, std::path::PathBuf::from("/dev/ttyUSB0"));
                assert_eq!(baud_rate, 57_600);
                assert_eq!(timeout, Duration::from_secs(2));
            }
            ConnectionConfig::Tcp { .. } => panic!("expected serial config"),
        }
    }

    #[test]
    fn detects_shared_cw_serial_cat_port() {
        let mut config = test_config();
        config.transport_kind = "serial".to_string();
        config.serial_port = "/dev/ttyUSB0".to_string();
        config.cw_keyer_type = "serial".to_string();
        config.cw_serial_port = "/dev/ttyUSB0".to_string();

        assert!(uses_shared_cw_serial_port(&config));
    }

    #[test]
    fn does_not_share_different_cw_serial_cat_ports() {
        let mut config = test_config();
        config.transport_kind = "serial".to_string();
        config.serial_port = "/dev/ttyUSB0".to_string();
        config.cw_keyer_type = "serial".to_string();
        config.cw_serial_port = "/dev/ttyUSB1".to_string();

        assert!(!uses_shared_cw_serial_port(&config));
    }

    #[tokio::test]
    async fn fail_pending_cw_sends_rejects_all_queued_requests() {
        let (first_tx, first_rx) = oneshot::channel();
        let (second_tx, second_rx) = oneshot::channel();
        let mut pending = VecDeque::from([
            PendingCwSend {
                payload: PendingCwPayload::Message {
                    mode: "run".to_string(),
                    key: "F1".to_string(),
                    fields: Map::new(),
                },
                completed: first_tx,
            },
            PendingCwSend {
                payload: PendingCwPayload::Message {
                    mode: "run".to_string(),
                    key: "F2".to_string(),
                    fields: Map::new(),
                },
                completed: second_tx,
            },
        ]);

        fail_pending_cw_sends(&mut pending, "cw stopped");

        assert!(pending.is_empty());
        assert_eq!(first_rx.await.unwrap(), Err("cw stopped".to_string()));
        assert_eq!(second_rx.await.unwrap(), Err("cw stopped".to_string()));
    }

    #[tokio::test]
    async fn fail_unavailable_radio_command_rejects_send_message() {
        let (completed_tx, completed_rx) = oneshot::channel();

        fail_unavailable_radio_command(
            RadioCommand::SendMessage {
                mode: "run".to_string(),
                key: "F1".to_string(),
                fields: Map::new(),
                completed: completed_tx,
            },
            "radio disconnected",
        );

        assert_eq!(
            completed_rx.await.unwrap(),
            Err("radio disconnected".to_string())
        );
    }

    #[test]
    fn queued_cw_send_text_prepends_single_space() {
        assert_eq!(cw_send_text("CQ TEST".to_string(), false), "CQ TEST");
        assert_eq!(cw_send_text("CQ TEST".to_string(), true), " CQ TEST");
    }
}
