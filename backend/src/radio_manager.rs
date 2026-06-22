use crate::cat_keyer::CatKeyer;
use crate::cw;
use crate::db::{Database, RadioConfig};
use crate::radio::{
    RadioCommand, RadioState, RadioStatus, ServerMessage, mode_candidates_for_request,
    mode_is_phone, normalize_mode,
};
use crate::voice_keyer::{VoiceKeyer, VoicePlaybackThread};
use crate::voice_messages;
use backon::{BackoffBuilder, ExponentialBuilder};
use cw_serial_keyer::{Config as CwSerialConfig, ControlLine, SerialKeyer as CwSerialDevice};
use futures_util::future::{BoxFuture, FutureExt};
use radio_cat_rs::{
    AsyncIoTransport, ChangeFlags, ConnectionState, Frequency, Radio,
    RadioConfig as CatRadioConfig, RadioError, RadioTask, RitXitOffsetHz, StateField, StateUpdate,
    TransportConfig,
};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Notify, RwLock, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, trace, warn};

const CAT_RECONNECT_MIN_DELAY: Duration = Duration::from_secs(1);
const CAT_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(10);
const MIN_RIT_OFFSET_HZ: i32 = -9_999;
const MAX_RIT_OFFSET_HZ: i32 = 9_999;

#[derive(Clone)]
pub struct RadioManager {
    db: Database,
    voice_keyer: VoiceKeyer,
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
    pub fn new(db: Database, voice_keyer: VoiceKeyer) -> Self {
        Self {
            db,
            voice_keyer,
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

            let mut config = self
                .db
                .radio(radio_id)
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("radio not found: {radio_id}"))?;
            self.voice_keyer.sanitize_radio_config(&mut config);

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
                    debug_radio_config(&config, "starting managed radio");
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
                    let task_voice_keyer = self.voice_keyer.clone();
                    let task = tokio::spawn(async move {
                        run_managed_radio(
                            config,
                            ManagedRadioRuntime {
                                current_status: task_current_status,
                                current: task_current,
                                status_updates: task_status_updates,
                                updates: task_updates,
                            },
                            command_rx,
                            shutdown_rx,
                            task_voice_keyer,
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

        debug_radio_config(&config, "requesting active radio config reload");
        command_sender
            .send(RadioCommand::ReloadConfig(Box::new(config)))
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

struct ManagedRadioRuntime {
    current_status: Arc<RwLock<RadioStatus>>,
    current: Arc<RwLock<Option<RadioState>>>,
    status_updates: broadcast::Sender<RadioStatus>,
    updates: broadcast::Sender<RadioState>,
}

async fn run_managed_radio(
    mut config: RadioConfig,
    runtime: ManagedRadioRuntime,
    mut commands: mpsc::Receiver<RadioCommand>,
    mut shutdown: oneshot::Receiver<()>,
    voice_keyer: VoiceKeyer,
) {
    let ManagedRadioRuntime {
        current_status,
        current,
        status_updates,
        updates,
    } = runtime;
    let mut reconnect_backoff = cat_reconnect_backoff().build();
    let mut reconnect_deadline = None;

    loop {
        if let Some(deadline) = reconnect_deadline {
            loop {
                tokio::select! {
                    _ = &mut shutdown => return,
                    command = commands.recv() => {
                        match command {
                            Some(RadioCommand::ReloadConfig(new_config)) => {
                                info!(radio_id = new_config.id, "reloading radio config while waiting to reconnect CAT");
                                config = *new_config;
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

        debug_radio_config(&config, "attempting CAT radio connection");
        let connect_config = config.clone();
        let connected = tokio::select! {
            _ = &mut shutdown => return,
            command = commands.recv() => {
                match command {
                    Some(RadioCommand::ReloadConfig(new_config)) => {
                        info!(radio_id = new_config.id, "reloading radio config before CAT connect");
                        config = *new_config;
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
            result = connect_cat_radio(connect_config) => {
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

        reconnect_backoff = cat_reconnect_backoff().build();
        info!(
            radio_id = config.id,
            radio_kind = %config.radio_kind,
            transport_kind = %config.transport_kind,
            "connected to CAT radio"
        );

        let ConnectedCatRadio {
            radio,
            task,
            shared_cw_serial_keyer,
        } = connected;
        let mut radio_updates = radio.subscribe_updates();
        let mut radio_task = tokio::spawn(async move { task.run().await });
        publish_cat_snapshot(
            config.id,
            radio.latest_state().as_ref(),
            &current_status,
            &status_updates,
            &current,
            &updates,
        )
        .await;

        let (cw_tx, cw_rx) = mpsc::channel(32);
        let cw_config = config.clone();
        let cw_radio = radio.clone();
        let cw_voice_keyer = voice_keyer.clone();
        let cw_task = tokio::spawn(async move {
            run_cw_task(
                cw_config,
                cw_radio,
                shared_cw_serial_keyer,
                cw_voice_keyer,
                cw_rx,
            )
            .await
        });

        let mut last_rit_offset_hz = current
            .read()
            .await
            .as_ref()
            .map(|state| state.rit_offset_hz)
            .unwrap_or(0);

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    shutdown_cw_task(cw_tx, cw_task).await;
                    abort_radio_task(radio_task).await;
                    return;
                }
                result = &mut radio_task => {
                    match result {
                        Ok(Ok(())) => warn!(radio_id = config.id, "CAT radio task exited"),
                        Ok(Err(error)) => warn!(radio_id = config.id, %error, "CAT radio task failed"),
                        Err(error) => warn!(radio_id = config.id, %error, "CAT radio task join failed"),
                    }
                    set_radio_status(&current_status, &status_updates, false).await;
                    reconnect_deadline = Some(next_cat_reconnect_deadline(&mut reconnect_backoff));
                    shutdown_cw_task(cw_tx, cw_task).await;
                    break;
                }
                update = radio_updates.recv() => {
                    match update {
                        Ok(update) => {
                            trace!(
                                radio_id = config.id,
                                source = ?update.source,
                                changes = ?update.changes,
                                fields = ?update.fields,
                                "received radio-cat state update"
                            );
                            publish_cat_snapshot(
                                config.id,
                                update.state.as_ref(),
                                &current_status,
                                &status_updates,
                                &current,
                                &updates,
                            ).await;
                            if let Some(offset) = update.state.rit_xit.offset_hz {
                                last_rit_offset_hz = i32::from(offset.as_hz());
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!(radio_id = config.id, skipped, "radio-cat update receiver lagged");
                            publish_cat_snapshot(
                                config.id,
                                radio.latest_state().as_ref(),
                                &current_status,
                                &status_updates,
                                &current,
                                &updates,
                            ).await;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            warn!(radio_id = config.id, "radio-cat update channel closed");
                            set_radio_status(&current_status, &status_updates, false).await;
                            reconnect_deadline = Some(next_cat_reconnect_deadline(&mut reconnect_backoff));
                            shutdown_cw_task(cw_tx, cw_task).await;
                            abort_radio_task(radio_task).await;
                            break;
                        }
                    }
                }
                command = commands.recv() => {
                    let Some(command) = command else {
                        shutdown_cw_task(cw_tx, cw_task).await;
                        abort_radio_task(radio_task).await;
                        return;
                    };
                    debug!(radio_id = config.id, ?command, "received radio command");
                    match command {
                        RadioCommand::SendMessage { mode, keys, fields, completed } => {
                            debug!(radio_id = config.id, mode, ?keys, "forwarding message send command");
                            if let Err(error) = cw_tx.send(CwTaskCommand::SendMessage { mode, keys, fields, completed }).await {
                                let CwTaskCommand::SendMessage { completed, .. } = error.0 else { unreachable!() };
                                let _ = completed.send(Err("cw task unavailable".to_string()));
                            }
                        }
                        RadioCommand::SendCwText { text, wait_for_completion, completed } => {
                            debug!(radio_id = config.id, text, wait_for_completion, "forwarding cw text send command");
                            if let Err(error) = cw_tx.send(CwTaskCommand::SendText { text, wait_for_completion, completed }).await {
                                let CwTaskCommand::SendText { completed, .. } = error.0 else { unreachable!() };
                                let _ = completed.send(Err("cw task unavailable".to_string()));
                            }
                        }
                        RadioCommand::StopKeying => {
                            debug!(radio_id = config.id, "forwarding keying stop command");
                            let _ = cw_tx.send(CwTaskCommand::Stop).await;
                        }
                        RadioCommand::SetWpm(wpm) => {
                            debug!(radio_id = config.id, wpm, "forwarding cw set_wpm command");
                            let _ = cw_tx.send(CwTaskCommand::SetWpm(wpm)).await;
                        }
                        RadioCommand::ReloadConfig(new_config) => {
                            debug_radio_config(&new_config, "reloading active radio config");
                            set_radio_status(&current_status, &status_updates, false).await;
                            shutdown_cw_task(cw_tx, cw_task).await;
                            abort_radio_task(radio_task).await;
                            config = *new_config;
                            break;
                        }
                        command => {
                            let is_rit_command = matches!(
                                command,
                                RadioCommand::RitClear
                                    | RadioCommand::RitIncrement(_)
                                    | RadioCommand::RitDecrement(_)
                            );
                            debug!(
                                radio_id = config.id,
                                ?command,
                                last_rit_offset_hz,
                                "applying CAT radio command"
                            );
                            match apply_command(&radio, &current, command, &mut last_rit_offset_hz).await {
                                Ok(()) => {}
                                Err(error) if is_unsupported_capability(&error) => {
                                    warn!(radio_id = config.id, %error, "CAT radio command unsupported");
                                }
                                Err(error) => {
                                    set_radio_status(&current_status, &status_updates, false).await;
                                    reconnect_deadline = Some(next_cat_reconnect_deadline(&mut reconnect_backoff));
                                    error!(radio_id = config.id, %error, "failed to apply radio command");
                                    shutdown_cw_task(cw_tx, cw_task).await;
                                    abort_radio_task(radio_task).await;
                                    break;
                                }
                            }
                            if is_rit_command {
                                debug!(
                                    radio_id = config.id,
                                    last_rit_offset_hz,
                                    "RIT command applied or ignored; tracked offset updated when supported"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn shutdown_cw_task(cw_tx: mpsc::Sender<CwTaskCommand>, cw_task: JoinHandle<()>) {
    let _ = cw_tx.send(CwTaskCommand::Shutdown).await;
    let _ = cw_task.await;
}

async fn abort_radio_task(radio_task: JoinHandle<radio_cat_rs::Result<()>>) {
    radio_task.abort();
    let _ = radio_task.await;
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

async fn publish_cat_snapshot(
    radio_id: i64,
    cat_state: &radio_cat_rs::RadioState,
    current_status: &Arc<RwLock<RadioStatus>>,
    status_updates: &broadcast::Sender<RadioStatus>,
    current: &Arc<RwLock<Option<RadioState>>>,
    updates: &broadcast::Sender<RadioState>,
) {
    set_radio_status(
        current_status,
        status_updates,
        matches!(cat_state.connection, ConnectionState::Ready),
    )
    .await;

    let previous = current.read().await.clone();
    let Some(state) = logger_state_from_cat_state(cat_state, previous.as_ref()) else {
        trace!(
            radio_id,
            "radio-cat state does not yet include logger-visible radio state"
        );
        return;
    };

    if previous.as_ref() == Some(&state) {
        trace!(
            radio_id,
            frequency_hz = state.frequency_hz,
            mode = %state.mode,
            rit_offset_hz = state.rit_offset_hz,
            "radio state unchanged; not broadcasting"
        );
        return;
    }

    trace!(
        radio_id,
        frequency_hz = state.frequency_hz,
        mode = %state.mode,
        rit_offset_hz = state.rit_offset_hz,
        "broadcasting radio state update"
    );
    *current.write().await = Some(state.clone());
    let _ = updates.send(state);
}

fn logger_state_from_cat_state(
    cat_state: &radio_cat_rs::RadioState,
    previous: Option<&RadioState>,
) -> Option<RadioState> {
    let frequency_hz = cat_state
        .main_rx
        .frequency
        .map(|frequency| frequency.hz())
        .or_else(|| previous.map(|state| state.frequency_hz))?;
    let mode = cat_state
        .main_rx
        .mode
        .map(|mode| normalize_mode(&mode))
        .or_else(|| previous.map(|state| state.mode.clone()))?;
    let rit_offset_hz = cat_state
        .rit_xit
        .offset_hz
        .map(|offset| i32::from(offset.as_hz()))
        .or_else(|| previous.map(|state| state.rit_offset_hz))
        .unwrap_or(0);

    Some(RadioState {
        frequency_hz,
        mode,
        rit_offset_hz,
    })
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
        RadioCommand::StopKeying
        | RadioCommand::SetWpm(_)
        | RadioCommand::SetFrequency(_)
        | RadioCommand::SetMode(_)
        | RadioCommand::RitClear
        | RadioCommand::RitIncrement(_)
        | RadioCommand::RitDecrement(_)
        | RadioCommand::ReloadConfig(_) => {}
    }
}

struct ConnectedCatRadio {
    radio: Radio,
    task: RadioTask,
    shared_cw_serial_keyer: Option<CwSerialDevice>,
}

async fn connect_cat_radio(config: RadioConfig) -> Result<ConnectedCatRadio, String> {
    let cat_config = cat_radio_config_for(&config)?;

    if uses_shared_cw_serial_port(&config) {
        let mut shared_cw_serial_keyer = open_serial_keyer(
            &config.cw_serial_port,
            config.cw_serial_baud_rate,
            &config.cw_serial_line,
        )
        .await
        .map_err(|error| error.to_string())?;
        let radio_id = config.id;
        let serial_port = config.serial_port.clone();
        let io = shared_cw_serial_keyer.serial_stream();
        info!(
            radio_id,
            serial_port = %serial_port,
            baud_rate = config.serial_baud_rate,
            line = %config.cw_serial_line,
            "sharing serial port for CAT and CW keying"
        );

        let transport = AsyncIoTransport::new(io);
        let (radio, task) = match Radio::build_with_transport(cat_config, transport).await {
            Ok(result) => result,
            Err(error) => {
                if let Err(close_error) = shared_cw_serial_keyer.close().await {
                    warn!(radio_id, %close_error, "failed to close shared serial CW keyer");
                }
                return Err(error.to_string());
            }
        };

        return Ok(ConnectedCatRadio {
            radio,
            task,
            shared_cw_serial_keyer: Some(shared_cw_serial_keyer),
        });
    }

    let (radio, task) = Radio::build(cat_config)
        .await
        .map_err(|error| error.to_string())?;

    Ok(ConnectedCatRadio {
        radio,
        task,
        shared_cw_serial_keyer: None,
    })
}

fn uses_shared_cw_serial_port(config: &RadioConfig) -> bool {
    config.transport_kind.trim().eq_ignore_ascii_case("serial")
        && config.cw_keyer_type.trim().eq_ignore_ascii_case("serial")
        && !config.serial_port.trim().is_empty()
        && config.serial_port.trim() == config.cw_serial_port.trim()
}

fn cat_radio_config_for(config: &RadioConfig) -> Result<CatRadioConfig, String> {
    let mut cat_config = CatRadioConfig::new(config.radio_kind.trim())
        .with_transport(transport_config_for(config)?)
        .with_options(config.options.clone());

    if config.radio_kind.trim().eq_ignore_ascii_case("dummy") {
        cat_config = cat_config.with_transport(TransportConfig::None);
    }

    Ok(cat_config)
}

fn transport_config_for(config: &RadioConfig) -> Result<TransportConfig, String> {
    trace!(
        radio_id = config.id,
        transport_kind = %config.transport_kind,
        "building CAT transport config"
    );

    match config.transport_kind.trim().to_ascii_lowercase().as_str() {
        "none" => Ok(TransportConfig::None),
        "tcp" => {
            let host = config.tcp_host.trim();
            if host.is_empty() {
                return Err("TCP host is required".to_string());
            }
            if config.tcp_port == 0 {
                return Err("TCP port must be between 1 and 65535".to_string());
            }
            Ok(TransportConfig::tcp_socket(host, config.tcp_port))
        }
        "serial" => {
            let serial_port = config.serial_port.trim();
            if serial_port.is_empty() {
                return Err("serial port is required".to_string());
            }
            if config.serial_baud_rate == 0 {
                return Err("serial baud rate must be greater than 0".to_string());
            }
            Ok(TransportConfig::serial(
                serial_port,
                config.serial_baud_rate,
            ))
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

fn debug_radio_config(config: &RadioConfig, message: &'static str) {
    debug!(
        radio_id = config.id,
        radio_kind = %config.radio_kind,
        transport_kind = %config.transport_kind,
        tcp_host = %config.tcp_host,
        tcp_port = config.tcp_port,
        serial_port = %config.serial_port,
        serial_baud_rate = config.serial_baud_rate,
        cw_keyer_type = %config.cw_keyer_type,
        winkeyer_serial_port = %config.winkeyer_serial_port,
        cw_serial_port = %config.cw_serial_port,
        cw_serial_baud_rate = config.cw_serial_baud_rate,
        cw_serial_line = %config.cw_serial_line,
        shared_cw_serial_port = uses_shared_cw_serial_port(config),
        "{message}"
    );
}

enum CwTaskCommand {
    SendMessage {
        mode: String,
        keys: Vec<String>,
        fields: serde_json::Map<String, serde_json::Value>,
        completed: oneshot::Sender<Result<(), String>>,
    },
    SendText {
        text: String,
        wait_for_completion: bool,
        completed: oneshot::Sender<Result<(), String>>,
    },
    Stop,
    SetWpm(u8),
    Shutdown,
}

enum PendingCwPayload {
    Message {
        mode: String,
        keys: Vec<String>,
        fields: serde_json::Map<String, serde_json::Value>,
    },
    Text {
        text: String,
        wait_for_completion: bool,
    },
}

struct PendingCwSend {
    payload: PendingCwPayload,
    completed: oneshot::Sender<Result<(), String>>,
}

async fn run_cw_task(
    config: RadioConfig,
    radio: Radio,
    shared_cw_serial_keyer: Option<CwSerialDevice>,
    voice_keyer: VoiceKeyer,
    mut commands: mpsc::Receiver<CwTaskCommand>,
) {
    let mut controller =
        CwController::new(&config, radio, shared_cw_serial_keyer, voice_keyer).await;
    let mut pending = VecDeque::new();

    loop {
        let (next_send, prepend_space) = if let Some(send) = pending.pop_front() {
            (Some(send), true)
        } else {
            match commands.recv().await {
                Some(CwTaskCommand::SendMessage {
                    mode,
                    keys,
                    fields,
                    completed,
                }) => (
                    Some(PendingCwSend {
                        payload: PendingCwPayload::Message { mode, keys, fields },
                        completed,
                    }),
                    false,
                ),
                Some(CwTaskCommand::SendText {
                    text,
                    wait_for_completion,
                    completed,
                }) => (
                    Some(PendingCwSend {
                        payload: PendingCwPayload::Text {
                            text,
                            wait_for_completion,
                        },
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
            PendingCwPayload::Message { mode, keys, fields } => {
                controller
                    .send_message(
                        mode,
                        keys,
                        fields,
                        prepend_space,
                        &mut commands,
                        &mut pending,
                    )
                    .await
            }
            PendingCwPayload::Text {
                text,
                wait_for_completion,
            } => {
                controller
                    .send_text(text, *wait_for_completion, &mut commands, &mut pending)
                    .await
            }
        };
        debug!(
            radio_id = config.id,
            ?result,
            remaining_pending = pending.len(),
            "cw task send command finished"
        );
        let should_shutdown = matches!(
            result.as_ref().err().map(String::as_str),
            Some("cw shutdown" | "keying shutdown")
        );
        let _ = send.completed.send(result);
        if should_shutdown {
            break;
        }
    }

    controller.close().await;
}

struct CwController {
    radio_id: i64,
    radio: Radio,
    messages: String,
    voice_messages: String,
    voice_playback: Option<VoicePlaybackThread>,
    voice_data_ptt_supported: bool,
    keyer: Option<Box<dyn CwKeyer>>,
}

struct VoiceDataPttGuard {
    radio_id: i64,
    radio: Radio,
    active: bool,
}

impl VoiceDataPttGuard {
    async fn acquire(radio_id: i64, radio: Radio, supported: bool) -> Self {
        let mut guard = Self {
            radio_id,
            radio,
            active: false,
        };

        if !supported {
            return guard;
        }

        match guard.radio.set_data_ptt(true).await {
            Ok(()) => {
                guard.active = true;
                debug!(radio_id = guard.radio_id, "enabled data ptt for voice playback");
            }
            Err(error) => {
                warn!(radio_id = guard.radio_id, %error, "failed to enable data ptt for voice playback");
            }
        }

        guard
    }

    async fn release(&mut self) {
        if !self.active {
            return;
        }

        match self.radio.set_data_ptt(false).await {
            Ok(()) => {
                self.active = false;
                debug!(radio_id = self.radio_id, "disabled data ptt after voice playback");
            }
            Err(error) => {
                warn!(radio_id = self.radio_id, %error, "failed to disable data ptt after voice playback");
            }
        }
    }
}

impl Drop for VoiceDataPttGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }

        let radio = self.radio.clone();
        let radio_id = self.radio_id;
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    if let Err(error) = radio.set_data_ptt(false).await {
                        warn!(radio_id, %error, "failed to disable data ptt after voice playback");
                    } else {
                        debug!(radio_id, "disabled data ptt after voice playback");
                    }
                });
            }
            Err(_) => {
                warn!(radio_id, "voice data ptt guard dropped without a tokio runtime; unable to schedule cleanup");
            }
        }
    }
}

impl CwController {
    async fn new(
        config: &RadioConfig,
        radio: Radio,
        shared_cw_serial_keyer: Option<CwSerialDevice>,
        voice_keyer: VoiceKeyer,
    ) -> Self {
        if let Err(error) = voice_keyer.sync_radio_messages(config) {
            warn!(radio_id = config.id, %error, "failed to sync voice keyer registrations for radio");
        }
        let voice_data_ptt_supported = radio
            .capabilities()
            .tx
            .map(|tx| tx.ptt.can_write())
            .unwrap_or(false);
        let mut controller = Self {
            radio_id: config.id,
            radio: radio.clone(),
            messages: config.cw_messages.clone(),
            voice_messages: config.voice_messages.clone(),
            voice_playback: match VoicePlaybackThread::spawn(
                config.id,
                voice_keyer,
                config.voice_output_device_id.clone(),
            ) {
                Ok(worker) => Some(worker),
                Err(error) => {
                    warn!(radio_id = config.id, %error, "failed to start voice keyer thread");
                    None
                }
            },
            voice_data_ptt_supported,
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
        keys: &[String],
        fields: &serde_json::Map<String, serde_json::Value>,
        prepend_space: bool,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        let Some(logger_mode) = self.radio_logger_mode() else {
            debug!(
                radio_id = self.radio_id,
                mode,
                ?keys,
                "ignoring message send; unable to determine radio mode"
            );
            return Ok(());
        };
        if mode_is_phone(&logger_mode) {
            return self.send_voice_messages(mode, keys, commands, pending).await;
        }
        if logger_mode != "CW" && logger_mode != "CW-R" {
            debug!(
                radio_id = self.radio_id,
                mode,
                ?keys,
                radio_mode = %logger_mode,
                "ignoring message send; radio mode is not CW or phone"
            );
            return Ok(());
        }

        let mut rendered_parts = Vec::new();
        for key in keys {
            let Some(rendered_text) = cw::render(&self.messages, mode, key, fields) else {
                warn!(
                    radio_id = self.radio_id,
                    mode,
                    ?keys,
                    key,
                    "unknown cw message"
                );
                return Err("unknown cw message".to_string());
            };
            if rendered_text.is_empty() {
                debug!(
                    radio_id = self.radio_id,
                    mode, key, "ignoring empty cw message"
                );
                continue;
            }
            rendered_parts.push(rendered_text);
        }
        if rendered_parts.is_empty() {
            debug!(
                radio_id = self.radio_id,
                mode,
                ?keys,
                "ignoring empty cw message sequence"
            );
            return Ok(());
        }
        let text = cw_send_text(rendered_parts.join(" "), prepend_space);
        debug!(
            radio_id = self.radio_id,
            mode,
            ?keys,
            text,
            "sending cw text"
        );
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
                ?keys,
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
                        mode,
                        ?keys,
                        "waiting for cw keyer busy"
                    );
                    self.wait_until_busy_or_stopped(commands, pending).await?;
                }
                debug!(
                    radio_id = self.radio_id,
                    mode,
                    ?keys,
                    "waiting for cw keyer idle"
                );
                let result = self.wait_until_idle_or_stopped(commands, pending).await;
                debug!(
                    radio_id = self.radio_id,
                    mode,
                    ?keys,
                    ?result,
                    "finished waiting for cw keyer idle"
                );
                result
            }
            CwSendCompletion::RadioCatUpdates(updates) => {
                debug!(
                    radio_id = self.radio_id,
                    mode,
                    ?keys,
                    "waiting for radio-cat cw completion updates"
                );
                self.wait_until_radio_cat_cw_complete_or_stopped(updates, commands, pending)
                    .await
            }
        }
    }

    async fn send_voice_messages(
        &mut self,
        mode: &str,
        keys: &[String],
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        for key in keys {
            if voice_messages::file_path_for(&self.voice_messages, mode, key).is_none() {
                debug!(
                    radio_id = self.radio_id,
                    mode,
                    key,
                    "ignoring voice message without a file"
                );
                continue;
            }
            if self.voice_playback.is_none() {
                return Err("voice keyer thread unavailable".to_string());
            }

            let mut data_ptt = VoiceDataPttGuard::acquire(
                self.radio_id,
                self.radio.clone(),
                self.voice_data_ptt_supported,
            )
            .await;
            let completed = match self.voice_playback.as_ref().unwrap().play_message(mode, key) {
                Ok(completed) => completed,
                Err(error) => {
                    data_ptt.release().await;
                    return Err(error);
                }
            };
            debug!(
                radio_id = self.radio_id,
                mode,
                key,
                "queued voice keyer playback"
            );
            let result = self
                .wait_until_voice_playback_done_or_stopped(key, completed, commands, pending)
                .await;
            data_ptt.release().await;
            result?;
        }

        Ok(())
    }

    async fn wait_until_voice_playback_done_or_stopped(
        &mut self,
        key: &str,
        mut completed: tokio::sync::oneshot::Receiver<Result<Duration, String>>,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        loop {
            tokio::select! {
                result = &mut completed => {
                    return match result {
                        Ok(Ok(duration)) => {
                            debug!(radio_id = self.radio_id, key, duration_ms = duration.as_millis(), "voice keyer playback completed");
                            Ok(())
                        }
                        Ok(Err(error)) => Err(error),
                        Err(_) => Err("voice keyer thread unavailable".to_string()),
                    };
                }
                command = commands.recv() => {
                    match command {
                        Some(CwTaskCommand::Stop) => {
                            debug!(radio_id = self.radio_id, key, "stop command interrupting voice keyer playback");
                            fail_pending_cw_sends(pending, "keying stopped");
                            self.stop().await;
                            return Err("keying stopped".to_string());
                        }
                        Some(CwTaskCommand::SetWpm(wpm)) => {
                            debug!(radio_id = self.radio_id, wpm, "set_wpm command received during voice keyer playback");
                            self.set_wpm(wpm).await;
                        }
                        Some(CwTaskCommand::Shutdown) | None => {
                            debug!(radio_id = self.radio_id, key, "shutdown interrupting voice keyer playback");
                            fail_pending_cw_sends(pending, "keying shutdown");
                            self.stop().await;
                            return Err("keying shutdown".to_string());
                        }
                        Some(CwTaskCommand::SendMessage {
                            mode,
                            keys,
                            fields,
                            completed,
                        }) => {
                            debug!(radio_id = self.radio_id, mode, ?keys, pending_count = pending.len(), "queueing message send command while voice keyer is playing");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Message { mode, keys, fields },
                                completed,
                            });
                        }
                        Some(CwTaskCommand::SendText { text, wait_for_completion, completed }) => {
                            debug!(radio_id = self.radio_id, text, pending_count = pending.len(), "queueing cw text send command while voice keyer is playing");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Text { text, wait_for_completion },
                                completed,
                            });
                        }
                    }
                }
            }
        }
    }

    fn radio_logger_mode(&self) -> Option<String> {
        match self.radio.latest_state().main_rx.mode {
            Some(mode) => Some(normalize_mode(&mode)),
            None => {
                warn!(
                    radio_id = self.radio_id,
                    "radio mode is unknown before message send"
                );
                None
            }
        }
    }

    async fn send_text(
        &mut self,
        text: &str,
        wait_for_completion: bool,
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

        if !wait_for_completion {
            debug!(
                radio_id = self.radio_id,
                text, "cw text queued without waiting for completion"
            );
            return Ok(());
        }

        match completion {
            CwSendCompletion::PollStatus { wait_for_busy } => {
                if wait_for_busy {
                    self.wait_until_busy_or_stopped(commands, pending).await?;
                }
                self.wait_until_idle_or_stopped(commands, pending).await
            }
            CwSendCompletion::RadioCatUpdates(updates) => {
                self.wait_until_radio_cat_cw_complete_or_stopped(updates, commands, pending)
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
                            keys,
                            fields,
                            completed,
                        }) => {
                            debug!(radio_id = self.radio_id, mode, ?keys, pending_count = pending.len(), "queueing message send command while busy");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Message { mode, keys, fields },
                                completed,
                            });
                        }
                        Some(CwTaskCommand::SendText {
                            text,
                            wait_for_completion,
                            completed,
                        }) => {
                            debug!(radio_id = self.radio_id, text, wait_for_completion, pending_count = pending.len(), "queueing cw text send command while busy");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Text {
                                    text,
                                    wait_for_completion,
                                },
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
                            keys,
                            fields,
                            completed,
                        }) => {
                            debug!(radio_id = self.radio_id, mode, ?keys, pending_count = pending.len(), "queueing message send command while busy");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Message { mode, keys, fields },
                                completed,
                            });
                        }
                        Some(CwTaskCommand::SendText {
                            text,
                            wait_for_completion,
                            completed,
                        }) => {
                            debug!(radio_id = self.radio_id, text, wait_for_completion, pending_count = pending.len(), "queueing cw text send command while busy");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Text {
                                    text,
                                    wait_for_completion,
                                },
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

    async fn wait_until_radio_cat_cw_complete_or_stopped(
        &mut self,
        mut updates: broadcast::Receiver<StateUpdate>,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        let mut saw_busy = self
            .radio
            .latest_state()
            .keyer
            .as_ref()
            .and_then(|keyer| keyer.sending)
            == Some(true);
        if saw_busy {
            debug!(
                radio_id = self.radio_id,
                "radio-cat keyer already reports sending"
            );
        }

        loop {
            tokio::select! {
                command = commands.recv() => {
                    match command {
                        Some(CwTaskCommand::Stop) => {
                            debug!(radio_id = self.radio_id, "stop command interrupting radio-cat cw wait");
                            fail_pending_cw_sends(pending, "cw stopped");
                            self.stop().await;
                            return Err("cw stopped".to_string());
                        }
                        Some(CwTaskCommand::SetWpm(wpm)) => {
                            debug!(radio_id = self.radio_id, wpm, "set_wpm command received during radio-cat cw wait");
                            self.set_wpm(wpm).await;
                        }
                        Some(CwTaskCommand::Shutdown) | None => {
                            debug!(radio_id = self.radio_id, "shutdown interrupting radio-cat cw wait");
                            fail_pending_cw_sends(pending, "cw shutdown");
                            self.stop().await;
                            return Err("cw shutdown".to_string());
                        }
                        Some(CwTaskCommand::SendMessage {
                            mode,
                            keys,
                            fields,
                            completed,
                        }) => {
                            debug!(radio_id = self.radio_id, mode, ?keys, pending_count = pending.len(), "queueing message send command while busy");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Message { mode, keys, fields },
                                completed,
                            });
                        }
                        Some(CwTaskCommand::SendText {
                            text,
                            wait_for_completion,
                            completed,
                        }) => {
                            debug!(radio_id = self.radio_id, text, wait_for_completion, pending_count = pending.len(), "queueing cw text send command while busy");
                            pending.push_back(PendingCwSend {
                                payload: PendingCwPayload::Text {
                                    text,
                                    wait_for_completion,
                                },
                                completed,
                            });
                        }
                    }
                }
                update = updates.recv() => {
                    match update {
                        Ok(update) => {
                            if !update.changes.contains(ChangeFlags::KEYER)
                                || !update.fields.contains(&StateField::KeyerSending) {
                                continue;
                            }
                            match update.state.keyer.as_ref().and_then(|keyer| keyer.sending) {
                                Some(true) => {
                                    saw_busy = true;
                                    debug!(radio_id = self.radio_id, source = ?update.source, "radio-cat keyer reports sending");
                                }
                                Some(false) => {
                                    debug!(radio_id = self.radio_id, source = ?update.source, saw_busy, "radio-cat keyer reports idle");
                                    return Ok(());
                                }
                                None => {
                                    debug!(radio_id = self.radio_id, source = ?update.source, "radio-cat keyer sending state unavailable");
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!(radio_id = self.radio_id, skipped, "lagged while waiting for radio-cat cw completion updates");
                            match self.radio.latest_state().keyer.as_ref().and_then(|keyer| keyer.sending) {
                                Some(true) => {
                                    saw_busy = true;
                                    debug!(radio_id = self.radio_id, "radio-cat keyer still reports sending after lag");
                                }
                                Some(false) => {
                                    debug!(radio_id = self.radio_id, saw_busy, "radio-cat keyer reports idle after lag");
                                    return Ok(());
                                }
                                None => {}
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            warn!(radio_id = self.radio_id, "radio-cat cw completion update channel closed");
                            return Err("radio-cat cw completion updates unavailable".to_string());
                        }
                    }
                }
            }
        }
    }

    async fn stop(&mut self) {
        if let Some(voice_playback) = self.voice_playback.as_ref() {
            voice_playback.stop_keying();
        }

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
        if let Some(voice_playback) = self.voice_playback.as_mut() {
            voice_playback.shutdown();
        }

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
    radio: Radio,
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
    RadioCatUpdates(broadcast::Receiver<StateUpdate>),
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
            let updates = self.subscribe_updates();
            CatKeyer::send_text(self, text).await?;
            Ok(CwSendCompletion::RadioCatUpdates(updates))
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

async fn apply_command(
    radio: &Radio,
    current: &Arc<RwLock<Option<RadioState>>>,
    command: RadioCommand,
    last_rit_offset_hz: &mut i32,
) -> Result<(), RadioError> {
    match command {
        RadioCommand::SetFrequency(frequency_hz) => {
            debug!(frequency_hz, "setting CAT radio frequency");
            radio
                .set_main_frequency(Frequency::from_hz(frequency_hz))
                .await
        }
        RadioCommand::SetMode(mode) => {
            let frequency_hz = current
                .read()
                .await
                .as_ref()
                .map(|state| state.frequency_hz)
                .or_else(|| {
                    radio
                        .latest_state()
                        .main_rx
                        .frequency
                        .map(|frequency| frequency.hz())
                })
                .unwrap_or(14_000_000);
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
                match radio.set_main_mode(radio_mode).await {
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
        RadioCommand::RitClear => {
            debug!(
                tracked_rit_offset_hz = *last_rit_offset_hz,
                "clearing CAT radio RIT"
            );
            let applied = set_rit_offset_hz(radio, 0, Some(false)).await?;
            if applied {
                *last_rit_offset_hz = 0;
            }
            debug!(
                tracked_rit_offset_hz = *last_rit_offset_hz,
                applied, "RIT clear command completed"
            );
            Ok(())
        }
        RadioCommand::RitIncrement(hz) => {
            let current_offset_hz = *last_rit_offset_hz;
            let next_offset_hz = next_rit_offset_hz(current_offset_hz, hz);
            debug!(
                hz,
                current_offset_hz, next_offset_hz, "incrementing CAT radio RIT"
            );
            if next_offset_hz == current_offset_hz {
                debug!(
                    current_offset_hz,
                    "RIT increment did not change offset after clamping"
                );
                return Ok(());
            }
            let applied = set_rit_offset_hz(radio, next_offset_hz, Some(true)).await?;
            if applied {
                *last_rit_offset_hz = next_offset_hz;
            }
            debug!(
                hz,
                current_offset_hz,
                tracked_rit_offset_hz = *last_rit_offset_hz,
                next_offset_hz,
                applied,
                "RIT increment command completed"
            );
            Ok(())
        }
        RadioCommand::RitDecrement(hz) => {
            let delta_hz = hz.saturating_neg();
            let current_offset_hz = *last_rit_offset_hz;
            let next_offset_hz = next_rit_offset_hz(current_offset_hz, delta_hz);
            debug!(
                hz,
                delta_hz, current_offset_hz, next_offset_hz, "decrementing CAT radio RIT"
            );
            if next_offset_hz == current_offset_hz {
                debug!(
                    current_offset_hz,
                    "RIT decrement did not change offset after clamping"
                );
                return Ok(());
            }
            let applied = set_rit_offset_hz(radio, next_offset_hz, Some(true)).await?;
            if applied {
                *last_rit_offset_hz = next_offset_hz;
            }
            debug!(
                hz,
                delta_hz,
                current_offset_hz,
                tracked_rit_offset_hz = *last_rit_offset_hz,
                next_offset_hz,
                applied,
                "RIT decrement command completed"
            );
            Ok(())
        }
        RadioCommand::SendMessage { .. }
        | RadioCommand::SendCwText { .. }
        | RadioCommand::StopKeying
        | RadioCommand::SetWpm(_)
        | RadioCommand::ReloadConfig(_) => Ok(()),
    }
}

fn next_rit_offset_hz(current_offset_hz: i32, delta_hz: i32) -> i32 {
    current_offset_hz
        .saturating_add(delta_hz)
        .clamp(MIN_RIT_OFFSET_HZ, MAX_RIT_OFFSET_HZ)
}

async fn set_rit_offset_hz(
    radio: &Radio,
    target_offset_hz: i32,
    enabled: Option<bool>,
) -> Result<bool, RadioError> {
    debug!(target_offset_hz, enabled, "applying CAT RIT offset");
    if let Some(enabled) = enabled {
        match radio.set_main_rit_enabled(enabled).await {
            Ok(()) => {}
            Err(error) if is_unsupported_capability(&error) => {
                debug!(
                    target_offset_hz,
                    enabled, "CAT RIT enable unsupported by radio"
                );
            }
            Err(error) => return Err(error),
        }
    }

    let offset =
        RitXitOffsetHz::new(target_offset_hz as i16).map_err(|error| RadioError::InvalidValue {
            field: "rit_offset_hz",
            message: error.to_string(),
        })?;

    match radio.set_main_rit_offset(offset).await {
        Ok(()) => {
            debug!(target_offset_hz, "CAT RIT offset applied");
            Ok(true)
        }
        Err(error) if is_unsupported_capability(&error) => {
            debug!(target_offset_hz, "CAT RIT offset unsupported by radio");
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

fn is_unsupported_capability(error: &RadioError) -> bool {
    matches!(error, RadioError::UnsupportedCapability { .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn test_config() -> RadioConfig {
        RadioConfig {
            id: 1,
            name: "Test".to_string(),
            radio_kind: "dummy".to_string(),
            transport_kind: "none".to_string(),
            tcp_host: "127.0.0.1".to_string(),
            tcp_port: 5002,
            serial_port: String::new(),
            serial_baud_rate: 115_200,
            options: String::new(),
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
            voice_messages: crate::voice_messages::DEFAULT_VOICE_MESSAGES.to_string(),
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
    fn builds_none_transport_config_for_dummy() {
        let config = test_config();
        let cat_config = cat_radio_config_for(&config).expect("config should build");
        assert!(matches!(cat_config.transport, TransportConfig::None));
    }

    #[test]
    fn builds_tcp_transport_config() {
        let mut config = test_config();
        config.radio_kind = "elecraft-k4".to_string();
        config.transport_kind = "tcp".to_string();

        let transport = transport_config_for(&config).expect("transport should build");

        assert_eq!(
            transport,
            TransportConfig::Tcp {
                address: "127.0.0.1:5002".to_string()
            }
        );
    }

    #[test]
    fn builds_serial_transport_config() {
        let mut config = test_config();
        config.transport_kind = "serial".to_string();
        config.tcp_host = String::new();
        config.tcp_port = 0;
        config.serial_port = "/dev/ttyUSB0".to_string();
        config.serial_baud_rate = 57_600;

        let transport = transport_config_for(&config).expect("transport should build");

        assert_eq!(
            transport,
            TransportConfig::Serial {
                path: "/dev/ttyUSB0".to_string(),
                baud_rate: 57_600,
            }
        );
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
                    keys: vec!["F1".to_string()],
                    fields: Map::new(),
                },
                completed: first_tx,
            },
            PendingCwSend {
                payload: PendingCwPayload::Message {
                    mode: "run".to_string(),
                    keys: vec!["F2".to_string()],
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
                keys: vec!["F1".to_string()],
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
    fn logger_state_uses_previous_values_for_unknown_fields() {
        let previous = RadioState {
            frequency_hz: 14_000_000,
            mode: "CW".to_string(),
            rit_offset_hz: 20,
        };
        let cat_state = radio_cat_rs::RadioState::default();

        assert_eq!(
            logger_state_from_cat_state(&cat_state, Some(&previous)),
            Some(previous)
        );
    }

    #[test]
    fn next_rit_offset_hz_applies_delta_from_tracked_offset() {
        assert_eq!(next_rit_offset_hz(0, 20), 20);
        assert_eq!(next_rit_offset_hz(35, 15), 50);
        assert_eq!(next_rit_offset_hz(35, -15), 20);
    }

    #[test]
    fn next_rit_offset_hz_clamps_to_supported_range() {
        assert_eq!(next_rit_offset_hz(MAX_RIT_OFFSET_HZ, 1), MAX_RIT_OFFSET_HZ);
        assert_eq!(next_rit_offset_hz(MIN_RIT_OFFSET_HZ, -1), MIN_RIT_OFFSET_HZ);
    }

    #[test]
    fn queued_cw_send_text_prepends_single_space() {
        assert_eq!(cw_send_text("CQ TEST".to_string(), false), "CQ TEST");
        assert_eq!(cw_send_text("CQ TEST".to_string(), true), " CQ TEST");
    }
}
