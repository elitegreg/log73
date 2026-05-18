use crate::cw;
use crate::db::{Database, RadioConfig};
use crate::radio::{RadioCommand, RadioState, ServerMessage, mode_for_request, normalize_mode};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, trace, warn};

#[derive(Clone)]
pub struct RadioManager {
    db: Database,
    radios: Arc<Mutex<HashMap<i64, ManagedRadio>>>,
}

#[derive(Clone)]
pub struct RadioHandle {
    current: Arc<RwLock<Option<RadioState>>>,
    updates: broadcast::Sender<RadioState>,
    commands: mpsc::Sender<RadioCommand>,
}

struct ManagedRadio {
    current: Arc<RwLock<Option<RadioState>>>,
    updates: broadcast::Sender<RadioState>,
    commands: mpsc::Sender<RadioCommand>,
    shutdown: Option<oneshot::Sender<()>>,
    _task: JoinHandle<()>,
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
        let mut radios = self.radios.lock().await;

        if let Some(radio) = radios.get_mut(&radio_id) {
            radio.refcount += 1;
            debug!(
                radio_id,
                refcount = radio.refcount,
                "acquired existing managed radio"
            );
            return Ok(RadioHandle {
                current: radio.current.clone(),
                updates: radio.updates.clone(),
                commands: radio.commands.clone(),
            });
        }

        let config = self
            .db
            .radio(radio_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("radio not found: {radio_id}"))?;
        debug!(
            radio_id,
            host = %config.rigctld_host,
            port = config.rigctld_port,
            poll_frequency = config.poll_frequency,
            rigctld_timeout = config.rigctld_timeout,
            winkeyer_enabled = config.winkeyer_enabled,
            winkeyer_serial_port = %config.winkeyer_serial_port,
            "starting managed radio"
        );
        let current = Arc::new(RwLock::new(None));
        let (updates, _) = broadcast::channel(32);
        let (commands, command_rx) = mpsc::channel(32);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task_current = current.clone();
        let task_updates = updates.clone();
        let task = tokio::spawn(async move {
            run_managed_radio(config, task_current, task_updates, command_rx, shutdown_rx).await;
        });

        radios.insert(
            radio_id,
            ManagedRadio {
                current: current.clone(),
                updates: updates.clone(),
                commands: commands.clone(),
                shutdown: Some(shutdown_tx),
                _task: task,
                refcount: 1,
            },
        );

        Ok(RadioHandle {
            current,
            updates,
            commands,
        })
    }

    pub async fn release(&self, radio_id: i64) {
        let mut removed = None;
        {
            let mut radios = self.radios.lock().await;
            if let Some(radio) = radios.get_mut(&radio_id) {
                radio.refcount = radio.refcount.saturating_sub(1);
                debug!(
                    radio_id,
                    refcount = radio.refcount,
                    "released managed radio reference"
                );
                if radio.refcount == 0 {
                    removed = radios.remove(&radio_id);
                }
            }
        }

        if let Some(mut radio) = removed {
            debug!(
                radio_id,
                "releasing final radio reference; shutting down managed radio"
            );
            if let Some(shutdown) = radio.shutdown.take() {
                let _ = shutdown.send(());
            }
        }
    }

    pub async fn is_active(&self, radio_id: i64) -> bool {
        self.radios.lock().await.contains_key(&radio_id)
    }

    pub async fn reload_config(&self, radio_id: i64, config: RadioConfig) -> Result<(), String> {
        let command_sender = {
            let radios = self.radios.lock().await;
            radios.get(&radio_id).map(|radio| radio.commands.clone())
        };

        let Some(command_sender) = command_sender else {
            return Ok(());
        };

        debug!(
            radio_id,
            host = %config.rigctld_host,
            port = config.rigctld_port,
            poll_frequency = config.poll_frequency,
            rigctld_timeout = config.rigctld_timeout,
            winkeyer_enabled = config.winkeyer_enabled,
            winkeyer_serial_port = %config.winkeyer_serial_port,
            "requesting active radio config reload"
        );
        command_sender
            .send(RadioCommand::ReloadConfig(config))
            .await
            .map_err(|_| "radio task unavailable".to_string())
    }
}

impl RadioHandle {
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

    pub async fn send_command(
        &self,
        command: RadioCommand,
    ) -> Result<(), mpsc::error::SendError<RadioCommand>> {
        self.commands.send(command).await
    }
}

async fn run_managed_radio(
    mut config: RadioConfig,
    current: Arc<RwLock<Option<RadioState>>>,
    updates: broadcast::Sender<RadioState>,
    mut commands: mpsc::Receiver<RadioCommand>,
    mut shutdown: oneshot::Receiver<()>,
) {
    loop {
        let poll_interval = Duration::from_secs_f64(config.poll_frequency);
        let rigctld_timeout = Duration::from_secs_f64(config.rigctld_timeout);
        debug!(
            radio_id = config.id,
            host = %config.rigctld_host,
            port = config.rigctld_port,
            poll_frequency = config.poll_frequency,
            rigctld_timeout = config.rigctld_timeout,
            "attempting rigctld connection"
        );
        let mut rig = rigctld::Rig::new(&config.rigctld_host, config.rigctld_port);
        rig.set_communication_timeout(rigctld_timeout);

        tokio::select! {
            _ = &mut shutdown => return,
            command = commands.recv() => {
                match command {
                    Some(RadioCommand::ReloadConfig(new_config)) => {
                        info!(radio_id = new_config.id, "reloading radio config before rigctld connect");
                        config = new_config;
                        continue;
                    }
                    Some(command) => {
                        warn!(radio_id = config.id, ?command, "dropping radio command while rigctld is disconnected");
                        continue;
                    }
                    None => return,
                }
            }
            result = rig.connect() => {
                if let Err(error) = result {
                    warn!(
                        radio_id = config.id,
                        host = %config.rigctld_host,
                        port = config.rigctld_port,
                        %error,
                        "failed to connect to rigctld"
                    );
                    tokio::time::sleep(poll_interval).await;
                    continue;
                }
            }
        }

        info!(
            radio_id = config.id,
            host = %config.rigctld_host,
            port = config.rigctld_port,
            "connected to rigctld"
        );
        let (cw_tx, cw_rx) = mpsc::channel(32);
        let cw_config = config.clone();
        let cw_task = tokio::spawn(async move { run_cw_task(cw_config, cw_rx).await });
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
                    rig.disconnect();
                    return;
                }
                _ = interval.tick() => {
                    trace!(radio_id = config.id, "polling radio state");
                    match poll_radio(&mut rig).await {
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
                            warn!(radio_id = config.id, %error, "failed to poll rigctld");
                            let _ = cw_tx.send(CwTaskCommand::Shutdown).await;
                            let _ = cw_task.await;
                            rig.disconnect();
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
                        RadioCommand::SendCw { mode, key, fields, completed } => {
                            debug!(radio_id = config.id, mode, key, "forwarding cw send command");
                            if let Err(error) = cw_tx.send(CwTaskCommand::Send { mode, key, fields, completed }).await {
                                let CwTaskCommand::Send { completed, .. } = error.0 else { unreachable!() };
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
                                host = %new_config.rigctld_host,
                                port = new_config.rigctld_port,
                                poll_frequency = new_config.poll_frequency,
                                rigctld_timeout = new_config.rigctld_timeout,
                                winkeyer_enabled = new_config.winkeyer_enabled,
                                winkeyer_serial_port = %new_config.winkeyer_serial_port,
                                "reloading active radio config"
                            );
                            debug!(radio_id = config.id, "shutting down cw task for radio config reload");
                            let _ = cw_tx.send(CwTaskCommand::Shutdown).await;
                            let _ = cw_task.await;
                            debug!(radio_id = config.id, "disconnecting rigctld for radio config reload");
                            rig.disconnect();
                            config = new_config;
                            break;
                        }
                        command => {
                            debug!(radio_id = config.id, ?command, last_frequency_hz, "applying rigctld command");
                            if let Err(error) = apply_command(&mut rig, command, last_frequency_hz).await {
                                error!(radio_id = config.id, %error, "failed to apply radio command");
                                let _ = cw_tx.send(CwTaskCommand::Shutdown).await;
                                let _ = cw_task.await;
                                rig.disconnect();
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

enum CwTaskCommand {
    Send {
        mode: String,
        key: String,
        fields: serde_json::Map<String, serde_json::Value>,
        completed: oneshot::Sender<Result<(), String>>,
    },
    Stop,
    SetWpm(u8),
    Shutdown,
}

async fn run_cw_task(config: RadioConfig, mut commands: mpsc::Receiver<CwTaskCommand>) {
    let mut controller = CwController::new(&config).await;

    while let Some(command) = commands.recv().await {
        match command {
            CwTaskCommand::Send {
                mode,
                key,
                fields,
                completed,
            } => {
                debug!(
                    radio_id = config.id,
                    mode, key, "cw task received send command"
                );
                let result = controller.send(&mode, &key, &fields, &mut commands).await;
                debug!(
                    radio_id = config.id,
                    ?result,
                    "cw task send command finished"
                );
                let _ = completed.send(result);
            }
            CwTaskCommand::Stop => {
                debug!(radio_id = config.id, "cw task received stop command");
                controller.stop().await;
            }
            CwTaskCommand::SetWpm(wpm) => {
                debug!(
                    radio_id = config.id,
                    wpm, "cw task received set_wpm command"
                );
                controller.set_wpm(wpm).await;
            }
            CwTaskCommand::Shutdown => {
                debug!(radio_id = config.id, "cw task received shutdown command");
                break;
            }
        }
    }

    controller.close().await;
}

struct CwController {
    radio_id: i64,
    enabled: bool,
    serial_port: String,
    messages: String,
    winkeyer: Option<winkeyer::WinKeyer>,
}

impl CwController {
    async fn new(config: &RadioConfig) -> Self {
        let mut controller = Self {
            radio_id: config.id,
            enabled: config.winkeyer_enabled,
            serial_port: config.winkeyer_serial_port.clone(),
            messages: config.cw_messages.clone(),
            winkeyer: None,
        };
        if controller.enabled {
            controller.connect().await;
        }
        controller
    }

    async fn connect(&mut self) {
        if !self.enabled || self.serial_port.trim().is_empty() {
            return;
        }

        match winkeyer::WinKeyer::open(&self.serial_port).await {
            Ok((mut winkeyer, revision)) => {
                winkeyer.set_timeout(Duration::from_millis(500));
                info!(
                    radio_id = self.radio_id,
                    serial_port = %self.serial_port,
                    revision,
                    "connected to winkeyer"
                );
                self.winkeyer = Some(winkeyer);
            }
            Err(error) => {
                warn!(
                    radio_id = self.radio_id,
                    serial_port = %self.serial_port,
                    %error,
                    "failed to connect to winkeyer"
                );
                self.winkeyer = None;
            }
        }
    }

    async fn ensure_connected(&mut self) -> Option<&mut winkeyer::WinKeyer> {
        if !self.enabled {
            debug!(
                radio_id = self.radio_id,
                "ignoring cw command; winkeyer disabled"
            );
            return None;
        }
        if self.winkeyer.is_none() {
            self.connect().await;
        }
        self.winkeyer.as_mut()
    }

    async fn send(
        &mut self,
        mode: &str,
        key: &str,
        fields: &serde_json::Map<String, serde_json::Value>,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
    ) -> Result<(), String> {
        let Some(text) = cw::render(&self.messages, mode, key, fields) else {
            warn!(radio_id = self.radio_id, mode, key, "unknown cw message");
            return Err("unknown cw message".to_string());
        };
        if text.is_empty() {
            debug!(
                radio_id = self.radio_id,
                mode, key, "ignoring empty cw message"
            );
            return Ok(());
        }
        debug!(radio_id = self.radio_id, mode, key, text, "sending cw text");
        let radio_id = self.radio_id;
        let Some(winkeyer) = self.ensure_connected().await else {
            return Err("winkeyer unavailable".to_string());
        };
        if let Err(error) = winkeyer.send_text(&text).await {
            warn!(radio_id, %error, "failed to send cw text");
            self.winkeyer = None;
            return Err(error.to_string());
        }
        debug!(radio_id, mode, key, "cw text queued to winkeyer");
        debug!(radio_id, mode, key, "waiting for winkeyer to become busy");
        self.wait_until_busy_or_stopped(commands).await?;
        debug!(radio_id, mode, key, "waiting for winkeyer idle");
        let result = self.wait_until_idle_or_stopped(commands).await;
        debug!(
            radio_id,
            mode,
            key,
            ?result,
            "finished waiting for winkeyer idle"
        );
        result
    }

    async fn wait_until_busy_or_stopped(
        &mut self,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
    ) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            tokio::select! {
                command = commands.recv() => {
                    match command {
                        Some(CwTaskCommand::Stop) => {
                            debug!(radio_id = self.radio_id, "stop command interrupting cw busy wait");
                            self.stop().await;
                            return Ok(());
                        }
                        Some(CwTaskCommand::SetWpm(wpm)) => {
                            debug!(radio_id = self.radio_id, wpm, "set_wpm command received during cw busy wait");
                            self.set_wpm(wpm).await;
                        }
                        Some(CwTaskCommand::Shutdown) | None => {
                            debug!(radio_id = self.radio_id, "shutdown interrupting cw busy wait");
                            self.stop().await;
                            return Err("cw shutdown".to_string());
                        }
                        Some(CwTaskCommand::Send { completed, .. }) => {
                            debug!(radio_id = self.radio_id, "rejecting cw send command while busy");
                            let _ = completed.send(Err("cw busy".to_string()));
                        }
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    warn!(radio_id = self.radio_id, "timed out waiting for winkeyer to become busy");
                    return Err("winkeyer did not become busy".to_string());
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    let radio_id = self.radio_id;
                    let Some(winkeyer) = self.ensure_connected().await else {
                        return Err("winkeyer unavailable".to_string());
                    };
                    match winkeyer.status().await {
                        Ok(status) if status.busy || status.wait || status.key_down => {
                            debug!(radio_id = self.radio_id, "winkeyer is busy");
                            return Ok(());
                        }
                        Ok(_) => {}
                        Err(error) => {
                            warn!(radio_id, %error, "failed waiting for winkeyer busy");
                            self.winkeyer = None;
                            return Err(error.to_string());
                        }
                    }
                }
            }
        }
    }

    async fn wait_until_idle_or_stopped(
        &mut self,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
    ) -> Result<(), String> {
        loop {
            tokio::select! {
                command = commands.recv() => {
                    match command {
                        Some(CwTaskCommand::Stop) => {
                            debug!(radio_id = self.radio_id, "stop command interrupting cw idle wait");
                            self.stop().await;
                            return Ok(());
                        }
                        Some(CwTaskCommand::SetWpm(wpm)) => {
                            debug!(radio_id = self.radio_id, wpm, "set_wpm command received during cw idle wait");
                            self.set_wpm(wpm).await;
                        }
                        Some(CwTaskCommand::Shutdown) | None => {
                            debug!(radio_id = self.radio_id, "shutdown interrupting cw idle wait");
                            self.stop().await;
                            return Err("cw shutdown".to_string());
                        }
                        Some(CwTaskCommand::Send { completed, .. }) => {
                            debug!(radio_id = self.radio_id, "rejecting cw send command while busy");
                            let _ = completed.send(Err("cw busy".to_string()));
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    let radio_id = self.radio_id;
                    let Some(winkeyer) = self.ensure_connected().await else {
                        return Err("winkeyer unavailable".to_string());
                    };
                    match winkeyer.status().await {
                        Ok(status) if !status.busy && !status.wait && !status.key_down => {
                            debug!(radio_id = self.radio_id, "winkeyer is idle");
                            return Ok(());
                        }
                        Ok(_) => {}
                        Err(error) => {
                            warn!(radio_id, %error, "failed waiting for winkeyer idle");
                            self.winkeyer = None;
                            return Err(error.to_string());
                        }
                    }
                }
            }
        }
    }

    async fn stop(&mut self) {
        let radio_id = self.radio_id;
        let Some(winkeyer) = self.ensure_connected().await else {
            return;
        };
        debug!(radio_id, "clearing winkeyer buffer");
        if let Err(error) = winkeyer.clear_buffer().await {
            warn!(radio_id, %error, "failed to clear winkeyer buffer");
            self.winkeyer = None;
        } else {
            debug!(radio_id, "winkeyer buffer cleared");
        }
    }

    async fn set_wpm(&mut self, wpm: u8) {
        let radio_id = self.radio_id;
        let Some(winkeyer) = self.ensure_connected().await else {
            return;
        };
        debug!(radio_id, wpm, "setting winkeyer wpm");
        if let Err(error) = winkeyer.set_wpm(wpm).await {
            warn!(radio_id, wpm, %error, "failed to set winkeyer wpm");
            self.winkeyer = None;
        } else {
            debug!(radio_id, wpm, "winkeyer wpm set");
        }
    }

    async fn close(&mut self) {
        if let Some(mut winkeyer) = self.winkeyer.take() {
            if let Err(error) = winkeyer.close().await {
                warn!(radio_id = self.radio_id, %error, "failed to close winkeyer");
            }
        }
    }
}

async fn poll_radio(rig: &mut rigctld::Rig) -> Result<RadioState, rigctld::RigError> {
    let frequency_hz = rig.get_frequency().await?;
    let (mode, _) = rig.get_mode().await?;

    Ok(RadioState {
        frequency_hz,
        mode: normalize_mode(&mode),
    })
}

async fn apply_command(
    rig: &mut rigctld::Rig,
    command: RadioCommand,
    last_frequency_hz: u64,
) -> Result<(), rigctld::RigError> {
    match command {
        RadioCommand::SetFrequency(frequency_hz) => rig.set_frequency(frequency_hz).await,
        RadioCommand::SetMode(mode) => {
            let frequency_hz = if last_frequency_hz == 0 {
                rig.get_frequency().await?
            } else {
                last_frequency_hz
            };

            match mode_for_request(&mode, frequency_hz) {
                Some(rig_mode) => rig.set_mode(rig_mode, 0).await,
                None => {
                    warn!(mode, "ignoring unsupported radio mode");
                    Ok(())
                }
            }
        }
        RadioCommand::SendCw { .. }
        | RadioCommand::StopCw
        | RadioCommand::SetWpm(_)
        | RadioCommand::ReloadConfig(_) => Ok(()),
    }
}
