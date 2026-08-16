use crate::{
    DEFAULT_FLDIGI_POLL_INTERVAL, DEFAULT_FLDIGI_REQUEST_TIMEOUT, DEFAULT_FLDIGI_TX_POLL_INTERVAL,
    FldigiConfig, FldigiInterface, RadioConfig, RadioState,
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
    Transmit {
        logger_id: String,
        accepted: oneshot::Sender<Result<oneshot::Receiver<Result<(), String>>, String>>,
        text: String,
    },
    StopTransmit {
        logger_id: String,
        completed: oneshot::Sender<Result<(), String>>,
    },
    ClearReceiveBuffer {
        logger_id: String,
        completed: oneshot::Sender<Result<(), String>>,
    },
    Shutdown(oneshot::Sender<()>),
}

struct RunningDigitalIo {
    target: DigitalIoTarget,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
    commands: mpsc::Sender<RunningCommand>,
}

enum RunningCommand {
    Transmit {
        text: String,
        completed: oneshot::Sender<Result<(), String>>,
    },
    StopTransmit {
        completed: oneshot::Sender<Result<(), String>>,
    },
    ClearReceiveBuffer {
        completed: oneshot::Sender<Result<(), String>>,
    },
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

    pub async fn transmit(
        &self,
        radio_id: i64,
        logger_id: &str,
        text: String,
    ) -> Result<oneshot::Receiver<Result<(), String>>, String> {
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
        let (accepted, result) = oneshot::channel();
        commands
            .send(ControllerCommand::Transmit {
                logger_id: logger_id.to_string(),
                accepted,
                text,
            })
            .await
            .map_err(|_| "digital I/O controller is unavailable".to_string())?;
        result.await.map_err(|_| {
            "digital I/O controller stopped before accepting the transmission".to_string()
        })?
    }

    pub async fn stop_transmitting(&self, radio_id: i64, logger_id: &str) -> Result<(), String> {
        self.control(radio_id, logger_id, |logger_id, completed| {
            ControllerCommand::StopTransmit {
                logger_id,
                completed,
            }
        })
        .await
    }

    pub async fn clear_receive_buffer(&self, radio_id: i64, logger_id: &str) -> Result<(), String> {
        self.control(radio_id, logger_id, |logger_id, completed| {
            ControllerCommand::ClearReceiveBuffer {
                logger_id,
                completed,
            }
        })
        .await
    }

    pub fn subscribe_targets(&self) -> broadcast::Receiver<DigitalIoTargetState> {
        self.inner.targets.subscribe()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<DigitalIoEvent> {
        self.inner.events.subscribe()
    }

    async fn control(
        &self,
        radio_id: i64,
        logger_id: &str,
        command: impl FnOnce(String, oneshot::Sender<Result<(), String>>) -> ControllerCommand,
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
        let (completed, result) = oneshot::channel();
        commands
            .send(command(logger_id.to_string(), completed))
            .await
            .map_err(|_| "digital I/O controller is unavailable".to_string())?;
        result.await.map_err(|_| {
            "digital I/O controller stopped before completing the command".to_string()
        })?
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
                Some(ControllerCommand::Transmit { logger_id, text, accepted }) => {
                    if running.is_none() {
                        let target = target_updates.borrow().clone();
                        running = reconcile(
                            running,
                            radio_id,
                            &config,
                            &mode,
                            target,
                            &events,
                        )
                        .await;
                    }
                    let result = if let Some(running) = &running {
                        let (completed, result) = oneshot::channel();
                        running.commands.send(RunningCommand::Transmit { text, completed }).await.map(|()| result).map_err(|_| {
                            warn!(radio_id, logger_id, "FLDigi command channel closed");
                            "FLDigi command channel is unavailable".to_string()
                        })
                    } else {
                        Err("FLDigi is not active for this logger and mode".to_string())
                    };
                    if let Err(error) = &result {
                        warn!(radio_id, logger_id, %error, "unable to queue FLDigi transmission");
                    }
                    let _ = accepted.send(result);
                }
                Some(ControllerCommand::StopTransmit { logger_id, completed }) => {
                    let result = if let Some(running) = &running {
                        running.commands.send(RunningCommand::StopTransmit { completed }).await.map_err(|error| {
                            warn!(radio_id, logger_id, "FLDigi command channel closed");
                            let RunningCommand::StopTransmit { completed } = error.0 else { unreachable!() };
                            let _ = completed.send(Err("FLDigi command channel is unavailable".to_string()));
                            "FLDigi command channel is unavailable".to_string()
                        })
                    } else {
                        let _ = completed.send(Ok(()));
                        Ok(())
                    };
                    if let Err(error) = result {
                        warn!(radio_id, logger_id, %error, "unable to queue FLDigi stop command");
                    }
                }
                Some(ControllerCommand::ClearReceiveBuffer { logger_id, completed }) => {
                    if running.is_none() {
                        let target = target_updates.borrow().clone();
                        running = reconcile(
                            running,
                            radio_id,
                            &config,
                            &mode,
                            target,
                            &events,
                        )
                        .await;
                    }
                    let result = if let Some(running) = &running {
                        running.commands.send(RunningCommand::ClearReceiveBuffer { completed }).await.map_err(|error| {
                            warn!(radio_id, logger_id, "FLDigi command channel closed");
                            let RunningCommand::ClearReceiveBuffer { completed } = error.0 else { unreachable!() };
                            let _ = completed.send(Err("FLDigi command channel is unavailable".to_string()));
                            "FLDigi command channel is unavailable".to_string()
                        })
                    } else {
                        let error = "FLDigi is not active for this logger and mode".to_string();
                        let _ = completed.send(Err(error.clone()));
                        Err(error)
                    };
                    if let Err(error) = result {
                        warn!(radio_id, logger_id, %error, "unable to queue FLDigi clear command");
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
        tx_poll_interval: DEFAULT_FLDIGI_TX_POLL_INTERVAL,
        request_timeout: DEFAULT_FLDIGI_REQUEST_TIMEOUT,
    })?;
    let (commands, command_rx) = mpsc::channel(32);
    let (shutdown, shutdown_rx) = oneshot::channel();
    let events = events.clone();
    let task = tokio::spawn(run_fldigi_interface(
        interface,
        command_rx,
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
    mut commands: mpsc::Receiver<RunningCommand>,
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
            command = commands.recv() => match command {
                Some(RunningCommand::Transmit { text, completed }) => {
                    match interface.transmit(text).await {
                        Ok(transmission) => {
                            tokio::spawn(async move {
                                let result = transmission.await.map_err(|error| error.to_string());
                                let _ = completed.send(result);
                            });
                        }
                        Err(error) => {
                            let _ = completed.send(Err(error.to_string()));
                        }
                    }
                }
                Some(RunningCommand::StopTransmit { completed }) => {
                    let result = interface
                        .clear_transmit_buffer()
                        .await
                        .map_err(|error| error.to_string());
                    let _ = completed.send(result);
                }
                Some(RunningCommand::ClearReceiveBuffer { completed }) => {
                    let result = interface
                        .clear_receive_buffer()
                        .await
                        .map_err(|error| error.to_string());
                    let _ = completed.send(result);
                }
                None => {
                    interface.shutdown().await;
                    return;
                }
            },
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

    #[tokio::test]
    async fn sending_requires_the_selected_logger_and_an_active_mode() {
        let mut config = RadioConfig::new(1, crate::RadioSettings::default());
        config.fldigi_data_enabled = true;
        let (updates, update_rx) = broadcast::channel(4);
        let manager = DigitalIoManager::new();
        manager
            .acquire(
                1,
                "logger-1",
                42,
                config,
                Some(RadioState {
                    frequency_hz: 14_074_000,
                    mode: "CW".to_string(),
                    rit_offset_hz: 0,
                }),
                update_rx,
            )
            .await
            .expect("logger registers");

        let not_targeted = manager.transmit(1, "logger-1", "CQ".to_string()).await;
        assert!(matches!(
            not_targeted,
            Err(error) if error == "digital I/O is not enabled for this logger"
        ));

        manager
            .set_target(1, "logger-1", true)
            .await
            .expect("logger becomes target");
        manager
            .stop_transmitting(1, "logger-1")
            .await
            .expect("stopping is idempotent when FLDigi is inactive");
        let inactive_mode = manager.transmit(1, "logger-1", "CQ".to_string()).await;
        assert!(matches!(
            inactive_mode,
            Err(error) if error == "FLDigi is not active for this logger and mode"
        ));

        manager.shutdown_all().await;
        drop(updates);
    }
}
