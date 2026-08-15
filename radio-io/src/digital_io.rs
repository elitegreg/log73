use crate::{
    DEFAULT_FLDIGI_POLL_INTERVAL, DEFAULT_FLDIGI_REQUEST_TIMEOUT, FldigiCommand, FldigiConfig,
    FldigiInterface, RadioConfig, RadioState,
};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    sync::{Mutex, broadcast, mpsc, oneshot, watch},
    task::JoinHandle,
};
use tracing::warn;

#[derive(Clone)]
pub struct DigitalIoManager {
    inner: Arc<DigitalIoManagerInner>,
}

struct DigitalIoManagerInner {
    listeners: Mutex<HashMap<i64, ManagedListener>>,
    targets: broadcast::Sender<DigitalIoTargetState>,
    events: broadcast::Sender<DigitalIoEvent>,
}

struct ManagedListener {
    registrations: HashMap<String, LoggerRegistration>,
    target: Option<DigitalIoTarget>,
    target_updates: watch::Sender<Option<DigitalIoTarget>>,
    commands: mpsc::Sender<ControllerCommand>,
    task: JoinHandle<()>,
}

struct LoggerRegistration {
    log_id: i64,
    refcount: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DigitalIoTarget {
    logger_id: String,
    log_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DigitalIoTargetState {
    pub radio_id: i64,
    pub logger_id: Option<String>,
    pub log_id: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DigitalIoEvent {
    Received {
        radio_id: i64,
        logger_id: String,
        log_id: i64,
        text: String,
    },
    Error {
        radio_id: i64,
        logger_id: String,
        log_id: i64,
        message: String,
    },
}

enum ControllerCommand {
    Send {
        logger_id: String,
        command: FldigiCommand,
    },
    Shutdown(oneshot::Sender<()>),
}

struct RunningDigitalIo {
    target: DigitalIoTarget,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
    commands: mpsc::Sender<FldigiCommand>,
}

impl DigitalIoManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DigitalIoManagerInner {
                listeners: Mutex::new(HashMap::new()),
                targets: broadcast::channel(32).0,
                events: broadcast::channel(64).0,
            }),
        }
    }

    pub async fn acquire(
        &self,
        radio_id: i64,
        logger_id: &str,
        log_id: i64,
        config: RadioConfig,
        initial_state: Option<RadioState>,
        updates: broadcast::Receiver<RadioState>,
    ) -> Result<DigitalIoTargetState, String> {
        let mut listeners = self.inner.listeners.lock().await;
        if let Some(listener) = listeners.get_mut(&radio_id) {
            register_listener(listener, logger_id, log_id)?;
            return Ok(target_state(radio_id, listener.target.as_ref()));
        }

        let (commands, command_rx) = mpsc::channel(32);
        let (target_updates, target_rx) = watch::channel(None);
        let events = self.inner.events.clone();
        let task = tokio::spawn(run_controller(
            radio_id,
            config,
            initial_state,
            updates,
            target_rx,
            command_rx,
            events,
        ));
        let mut registrations = HashMap::new();
        registrations.insert(
            logger_id.to_string(),
            LoggerRegistration {
                log_id,
                refcount: 1,
            },
        );
        listeners.insert(
            radio_id,
            ManagedListener {
                registrations,
                target: None,
                target_updates,
                commands,
                task,
            },
        );
        Ok(no_target_state(radio_id))
    }

    pub async fn release(&self, radio_id: i64, logger_id: &str) {
        let (listener, target_changed) = {
            let mut listeners = self.inner.listeners.lock().await;
            let Some(listener) = listeners.get_mut(&radio_id) else {
                return;
            };
            let remove = listener
                .registrations
                .get_mut(logger_id)
                .is_some_and(|registration| {
                    registration.refcount = registration.refcount.saturating_sub(1);
                    registration.refcount == 0
                });
            if remove {
                listener.registrations.remove(logger_id);
            }
            let target_changed = remove
                && listener
                    .target
                    .as_ref()
                    .is_some_and(|target| target.logger_id == logger_id);
            if target_changed {
                listener.target = None;
                listener.target_updates.send_replace(None);
            }
            let listener = listener
                .registrations
                .is_empty()
                .then(|| listeners.remove(&radio_id))
                .flatten();
            (listener, target_changed)
        };
        if target_changed {
            let _ = self.inner.targets.send(no_target_state(radio_id));
        }
        shutdown_listener(listener).await;
    }

    pub async fn shutdown_all(&self) {
        let listeners = {
            let mut listeners = self.inner.listeners.lock().await;
            std::mem::take(&mut *listeners)
        };
        for (radio_id, listener) in listeners {
            shutdown_listener(Some(listener)).await;
            let _ = self.inner.targets.send(no_target_state(radio_id));
        }
    }

    pub async fn set_target(
        &self,
        radio_id: i64,
        logger_id: &str,
        enabled: bool,
    ) -> Result<DigitalIoTargetState, String> {
        let state = {
            let mut listeners = self.inner.listeners.lock().await;
            let listener = listeners
                .get_mut(&radio_id)
                .ok_or_else(|| format!("radio {radio_id} has no open logger"))?;
            let registration = listener.registrations.get(logger_id).ok_or_else(|| {
                format!("logger {logger_id} is not registered for radio {radio_id}")
            })?;
            let target = if enabled {
                Some(DigitalIoTarget {
                    logger_id: logger_id.to_string(),
                    log_id: registration.log_id,
                })
            } else if listener
                .target
                .as_ref()
                .is_some_and(|target| target.logger_id == logger_id)
            {
                None
            } else {
                listener.target.clone()
            };
            if listener.target != target {
                listener.target = target.clone();
                listener.target_updates.send_replace(target);
            }
            target_state(radio_id, listener.target.as_ref())
        };
        let _ = self.inner.targets.send(state.clone());
        Ok(state)
    }

    pub async fn send(
        &self,
        radio_id: i64,
        logger_id: &str,
        command: FldigiCommand,
    ) -> Result<(), String> {
        let commands = {
            let listeners = self.inner.listeners.lock().await;
            let listener = listeners
                .get(&radio_id)
                .ok_or_else(|| format!("radio {radio_id} has no open logger"))?;
            if !listener
                .target
                .as_ref()
                .is_some_and(|target| target.logger_id == logger_id)
            {
                return Err("digital I/O is not enabled for this logger".to_string());
            }
            listener.commands.clone()
        };
        commands
            .send(ControllerCommand::Send {
                logger_id: logger_id.to_string(),
                command,
            })
            .await
            .map_err(|_| "digital I/O controller is unavailable".to_string())
    }

    pub fn subscribe_targets(&self) -> broadcast::Receiver<DigitalIoTargetState> {
        self.inner.targets.subscribe()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<DigitalIoEvent> {
        self.inner.events.subscribe()
    }
}

impl Default for DigitalIoManager {
    fn default() -> Self {
        Self::new()
    }
}

fn register_listener(
    listener: &mut ManagedListener,
    logger_id: &str,
    log_id: i64,
) -> Result<(), String> {
    if let Some(registration) = listener.registrations.get_mut(logger_id) {
        if registration.log_id != log_id {
            return Err(format!(
                "logger {logger_id} is already registered for log {}",
                registration.log_id
            ));
        }
        registration.refcount += 1;
    } else {
        listener.registrations.insert(
            logger_id.to_string(),
            LoggerRegistration {
                log_id,
                refcount: 1,
            },
        );
    }
    Ok(())
}

async fn shutdown_listener(listener: Option<ManagedListener>) {
    let Some(listener) = listener else {
        return;
    };
    let (completed, result) = oneshot::channel();
    let _ = listener
        .commands
        .send(ControllerCommand::Shutdown(completed))
        .await;
    let _ = result.await;
    let _ = listener.task.await;
}

fn target_state(radio_id: i64, target: Option<&DigitalIoTarget>) -> DigitalIoTargetState {
    DigitalIoTargetState {
        radio_id,
        logger_id: target.map(|target| target.logger_id.clone()),
        log_id: target.map(|target| target.log_id),
    }
}

fn no_target_state(radio_id: i64) -> DigitalIoTargetState {
    target_state(radio_id, None)
}

async fn run_controller(
    radio_id: i64,
    config: RadioConfig,
    initial_state: Option<RadioState>,
    mut updates: broadcast::Receiver<RadioState>,
    mut target_updates: watch::Receiver<Option<DigitalIoTarget>>,
    mut commands: mpsc::Receiver<ControllerCommand>,
    events: broadcast::Sender<DigitalIoEvent>,
) {
    let mut mode = initial_state.map(|state| state.mode).unwrap_or_default();
    let initial_target = target_updates.borrow().clone();
    let mut running = reconcile(None, radio_id, &config, &mode, initial_target, &events).await;
    loop {
        tokio::select! {
            update = updates.recv() => match update {
                Ok(update) => {
                    mode = update.mode;
                    let target = target_updates.borrow().clone();
                    running = reconcile(running, radio_id, &config, &mode, target, &events).await;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            changed = target_updates.changed() => {
                if changed.is_err() { break; }
                let target = target_updates.borrow().clone();
                running = reconcile(running, radio_id, &config, &mode, target, &events).await;
            }
            command = commands.recv() => match command {
                Some(ControllerCommand::Send { logger_id, command }) => {
                    if let Some(running) = &running
                        && running.commands.send(command).await.is_err()
                    {
                        warn!(radio_id, logger_id, "FLDigi command channel closed");
                    }
                }
                Some(ControllerCommand::Shutdown(completed)) => {
                    stop_running(running).await;
                    let _ = completed.send(());
                    return;
                }
                None => break,
            }
        }
    }
    stop_running(running).await;
}

async fn reconcile(
    running: Option<RunningDigitalIo>,
    radio_id: i64,
    config: &RadioConfig,
    mode: &str,
    target: Option<DigitalIoTarget>,
    events: &broadcast::Sender<DigitalIoEvent>,
) -> Option<RunningDigitalIo> {
    let should_run = target
        .as_ref()
        .is_some_and(|_| digital_io_enabled_for_mode(config, mode));
    if should_run
        && running.as_ref().is_some_and(|running| {
            &running.target != target.as_ref().expect("target exists when enabled")
        })
    {
        stop_running(running).await;
        return start_running_for_target(
            radio_id,
            config,
            target.expect("target exists when enabled"),
            events,
        );
    }
    if should_run && running.is_none() {
        let target = target.expect("target exists when digital I/O should run");
        return start_running_for_target(radio_id, config, target, events);
    }
    if !should_run && running.is_some() {
        stop_running(running).await;
        return None;
    }
    running
}

fn start_running_for_target(
    radio_id: i64,
    config: &RadioConfig,
    target: DigitalIoTarget,
    events: &broadcast::Sender<DigitalIoEvent>,
) -> Option<RunningDigitalIo> {
    match start_running(radio_id, config, target.clone(), events) {
        Ok(running) => Some(running),
        Err(message) => {
            let _ = events.send(DigitalIoEvent::Error {
                radio_id,
                logger_id: target.logger_id,
                log_id: target.log_id,
                message,
            });
            None
        }
    }
}

fn digital_io_enabled_for_mode(config: &RadioConfig, mode: &str) -> bool {
    if mode.eq_ignore_ascii_case("DATA") {
        config.fldigi_data_enabled
    } else if mode.eq_ignore_ascii_case("RTTY") {
        config.fldigi_rtty_enabled
    } else {
        false
    }
}

fn start_running(
    radio_id: i64,
    config: &RadioConfig,
    target: DigitalIoTarget,
    events: &broadcast::Sender<DigitalIoEvent>,
) -> Result<RunningDigitalIo, String> {
    let interface = FldigiInterface::start(FldigiConfig {
        endpoint: format!("http://{}:{}/RPC2", config.fldigi_host, config.fldigi_port),
        poll_interval: DEFAULT_FLDIGI_POLL_INTERVAL,
        request_timeout: DEFAULT_FLDIGI_REQUEST_TIMEOUT,
    })?;
    let commands = interface.commands.clone();
    let (shutdown, shutdown_rx) = oneshot::channel();
    let events = events.clone();
    let task = tokio::spawn(run_fldigi_interface(
        interface,
        shutdown_rx,
        radio_id,
        target.clone(),
        events,
    ));
    Ok(RunningDigitalIo {
        target,
        shutdown,
        task,
        commands,
    })
}

async fn stop_running(running: Option<RunningDigitalIo>) {
    if let Some(running) = running {
        let _ = running.shutdown.send(());
        let _ = running.task.await;
    }
}

async fn run_fldigi_interface(
    mut interface: FldigiInterface,
    mut shutdown: oneshot::Receiver<()>,
    radio_id: i64,
    target: DigitalIoTarget,
    events: broadcast::Sender<DigitalIoEvent>,
) {
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                interface.shutdown().await;
                return;
            }
            text = interface.received.recv() => match text {
                Some(text) => {
                    let _ = events.send(DigitalIoEvent::Received {
                        radio_id,
                        logger_id: target.logger_id.clone(),
                        log_id: target.log_id,
                        text,
                    });
                }
                None => return,
            },
            error = interface.errors.recv() => match error {
                Some(error) => {
                    let _ = events.send(DigitalIoEvent::Error {
                        radio_id,
                        logger_id: target.logger_id.clone(),
                        log_id: target.log_id,
                        message: error.to_string(),
                    });
                }
                None => return,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_only_for_enabled_digital_mode() {
        let mut config = RadioConfig::new(1, crate::RadioSettings::default());
        config.fldigi_data_enabled = true;
        config.fldigi_rtty_enabled = true;

        assert!(digital_io_enabled_for_mode(&config, "DATA"));
        assert!(digital_io_enabled_for_mode(&config, "rtty"));
        assert!(!digital_io_enabled_for_mode(&config, "CW"));

        config.fldigi_data_enabled = false;
        config.fldigi_rtty_enabled = false;
        assert!(!digital_io_enabled_for_mode(&config, "DATA"));
        assert!(!digital_io_enabled_for_mode(&config, "RTTY"));
    }
}
