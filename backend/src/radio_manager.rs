use crate::db::{Database, RadioConfig};
use crate::radio::{RadioCommand, RadioState, ServerMessage, mode_for_request, normalize_mode};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

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
    config: RadioConfig,
    current: Arc<RwLock<Option<RadioState>>>,
    updates: broadcast::Sender<RadioState>,
    mut commands: mpsc::Receiver<RadioCommand>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let poll_interval = Duration::from_secs_f64(config.poll_frequency);
    let rigctld_timeout = Duration::from_secs_f64(config.rigctld_timeout);

    loop {
        let mut rig = rigctld::Rig::new(&config.rigctld_host, config.rigctld_port);
        rig.set_communication_timeout(rigctld_timeout);

        tokio::select! {
            _ = &mut shutdown => return,
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
                    rig.disconnect();
                    return;
                }
                _ = interval.tick() => {
                    match poll_radio(&mut rig).await {
                        Ok(state) => {
                            last_frequency_hz = state.frequency_hz;
                            *current.write().await = Some(state.clone());
                            let _ = updates.send(state);
                        }
                        Err(error) => {
                            warn!(radio_id = config.id, %error, "failed to poll rigctld");
                            rig.disconnect();
                            break;
                        }
                    }
                }
                command = commands.recv() => {
                    let Some(command) = command else { return; };
                    debug!(radio_id = config.id, ?command, "applying radio command");
                    if let Err(error) = apply_command(&mut rig, command, last_frequency_hz).await {
                        error!(radio_id = config.id, %error, "failed to apply radio command");
                        rig.disconnect();
                        break;
                    }
                }
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
    }
}
