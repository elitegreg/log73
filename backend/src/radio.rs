use crate::bands::band_for_frequency;
use crate::frequency::Frequency;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, broadcast, mpsc};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    RadioState(RadioState),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadioState {
    pub frequency_hz: u64,
    pub mode: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    SetFrequency { frequency_hz: u64 },
    SetMode { mode: String },
}

#[derive(Debug, Clone)]
pub enum RadioCommand {
    SetFrequency(u64),
    SetMode(String),
}

#[derive(Clone)]
pub struct RadioSharedState {
    current: Arc<RwLock<Option<RadioState>>>,
    updates: broadcast::Sender<RadioState>,
    commands: mpsc::Sender<RadioCommand>,
}

impl RadioSharedState {
    pub fn new(commands: mpsc::Sender<RadioCommand>) -> Self {
        let (updates, _) = broadcast::channel(32);

        Self {
            current: Arc::new(RwLock::new(None)),
            updates,
            commands,
        }
    }

    pub async fn current(&self) -> Option<RadioState> {
        self.current.read().await.clone()
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

    async fn update(&self, state: RadioState) {
        *self.current.write().await = Some(state.clone());
        let _ = self.updates.send(state);
    }
}

pub fn normalize_mode(mode: &rigctld::Mode) -> String {
    match mode {
        rigctld::Mode::USB | rigctld::Mode::LSB => "SSB".to_string(),
        other => other.to_string(),
    }
}

pub fn mode_for_request(requested: &str, frequency_hz: u64) -> Option<rigctld::Mode> {
    match requested.to_uppercase().as_str() {
        "CW" => Some(rigctld::Mode::CW),
        "FM" => Some(rigctld::Mode::FM),
        "AM" => Some(rigctld::Mode::AM),
        "SSB" => Some(ssb_mode_for_frequency(frequency_hz)),
        "USB" => Some(rigctld::Mode::USB),
        "LSB" => Some(rigctld::Mode::LSB),
        _ => None,
    }
}

fn ssb_mode_for_frequency(frequency_hz: u64) -> rigctld::Mode {
    let frequency = Frequency::from_hz(frequency_hz);

    match band_for_frequency(frequency).map(|band| band.meters) {
        Some(meters) if meters >= 40 => rigctld::Mode::LSB,
        _ => rigctld::Mode::USB,
    }
}

pub async fn run_radio_task(
    host: String,
    port: u16,
    poll_interval: Duration,
    shared: RadioSharedState,
    mut commands: mpsc::Receiver<RadioCommand>,
) {
    loop {
        let mut rig = rigctld::Rig::new(&host, port);
        rig.set_communication_timeout(poll_interval.min(Duration::from_secs(2)));

        if let Err(error) = rig.connect().await {
            eprintln!("failed to connect to rigctld at {host}:{port}: {error}");
            tokio::time::sleep(poll_interval).await;
            continue;
        }

        println!("connected to rigctld at {host}:{port}");
        let mut interval = tokio::time::interval(poll_interval);
        let mut last_frequency_hz = shared
            .current()
            .await
            .map(|state| state.frequency_hz)
            .unwrap_or(0);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match poll_radio(&mut rig).await {
                        Ok(state) => {
                            last_frequency_hz = state.frequency_hz;
                            shared.update(state).await;
                        }
                        Err(error) => {
                            eprintln!("failed to poll rigctld: {error}");
                            rig.disconnect();
                            break;
                        }
                    }
                }
                command = commands.recv() => {
                    let Some(command) = command else {
                        return;
                    };

                    if let Err(error) = apply_command(&mut rig, command, last_frequency_hz).await {
                        eprintln!("failed to apply radio command: {error}");
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
                    eprintln!("ignoring unsupported radio mode: {mode}");
                    Ok(())
                }
            }
        }
    }
}
