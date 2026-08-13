use crate::{RadioConfig, RadioState};
use ham_radio_digital_interfacing::wsjtx::{
    Event, Message, MulticastGroup, ServerConfig, WsjtXServer,
};
use std::{collections::HashMap, net::Ipv4Addr, sync::Arc};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

#[derive(Clone)]
pub struct WsjtXManager {
    inner: Arc<WsjtXManagerInner>,
}

struct WsjtXManagerInner {
    listeners: Mutex<HashMap<i64, ManagedListener>>,
    events: broadcast::Sender<WsjtXEvent>,
    target_events: broadcast::Sender<WsjtXTargetState>,
}

struct ManagedListener {
    registrations: HashMap<String, LoggerRegistration>,
    target: Option<WsjtXTarget>,
    target_updates: watch::Sender<Option<WsjtXTarget>>,
    commands: mpsc::Sender<ControllerCommand>,
    task: JoinHandle<()>,
}

struct LoggerRegistration {
    log_id: i64,
    refcount: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WsjtXTarget {
    logger_id: String,
    log_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WsjtXTargetState {
    pub radio_id: i64,
    pub logger_id: Option<String>,
    pub log_id: Option<i64>,
}

enum ControllerCommand {
    Shutdown(oneshot::Sender<()>),
}

struct RunningListener {
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WsjtXEvent {
    LoggedAdif {
        radio_id: i64,
        log_id: i64,
        text: String,
    },
    Error {
        radio_id: i64,
        log_id: i64,
        message: String,
    },
}

impl WsjtXManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(WsjtXManagerInner {
                listeners: Mutex::new(HashMap::new()),
                events: broadcast::channel(32).0,
                target_events: broadcast::channel(32).0,
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
    ) -> Result<WsjtXTargetState, String> {
        let mut listeners = self.inner.listeners.lock().await;
        if let Some(listener) = listeners.get_mut(&radio_id) {
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
            return Ok(target_state(radio_id, listener.target.as_ref()));
        }

        let (commands, command_rx) = mpsc::channel(8);
        let target = WsjtXTarget {
            logger_id: logger_id.to_string(),
            log_id,
        };
        let (target_updates, target_rx) = watch::channel(Some(target.clone()));
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
        let registrations = HashMap::from([(
            logger_id.to_string(),
            LoggerRegistration {
                log_id,
                refcount: 1,
            },
        )]);
        listeners.insert(
            radio_id,
            ManagedListener {
                registrations,
                target: Some(target.clone()),
                target_updates,
                commands,
                task,
            },
        );
        let state = target_state(radio_id, Some(&target));
        let _ = self.inner.target_events.send(state.clone());
        Ok(state)
    }

    pub async fn release(&self, radio_id: i64, logger_id: &str) {
        let (listener, target_changed) = {
            let mut listeners = self.inner.listeners.lock().await;
            let Some(listener) = listeners.get_mut(&radio_id) else {
                return;
            };

            let remove_registration = match listener.registrations.get_mut(logger_id) {
                Some(registration) => {
                    registration.refcount = registration.refcount.saturating_sub(1);
                    registration.refcount == 0
                }
                None => false,
            };
            if remove_registration {
                listener.registrations.remove(logger_id);
            }

            let target_changed = remove_registration
                && listener
                    .target
                    .as_ref()
                    .is_some_and(|target| target.logger_id == logger_id);
            if target_changed {
                listener.target = None;
                listener.target_updates.send_replace(None);
            }

            let listener = if listener.registrations.is_empty() {
                listeners.remove(&radio_id)
            } else {
                None
            };
            (listener, target_changed)
        };
        if target_changed {
            let _ = self.inner.target_events.send(no_target_state(radio_id));
        }
        if let Some(listener) = listener {
            let (completed, result) = oneshot::channel();
            let _ = listener
                .commands
                .send(ControllerCommand::Shutdown(completed))
                .await;
            let _ = result.await;
            let _ = listener.task.await;
        }
    }

    pub async fn set_target(
        &self,
        radio_id: i64,
        logger_id: &str,
        enabled: bool,
    ) -> Result<WsjtXTargetState, String> {
        let state = {
            let mut listeners = self.inner.listeners.lock().await;
            let listener = listeners
                .get_mut(&radio_id)
                .ok_or_else(|| format!("radio {radio_id} has no open logger"))?;
            let registration = listener.registrations.get(logger_id).ok_or_else(|| {
                format!("logger {logger_id} is not registered for radio {radio_id}")
            })?;

            let next_target = if enabled {
                Some(WsjtXTarget {
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

            if listener.target != next_target {
                listener.target = next_target.clone();
                listener.target_updates.send_replace(next_target);
            }
            target_state(radio_id, listener.target.as_ref())
        };
        let _ = self.inner.target_events.send(state.clone());
        Ok(state)
    }

    pub fn subscribe_targets(&self) -> broadcast::Receiver<WsjtXTargetState> {
        self.inner.target_events.subscribe()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<WsjtXEvent> {
        self.inner.events.subscribe()
    }

    #[cfg(test)]
    async fn current_target(&self, radio_id: i64) -> WsjtXTargetState {
        let listeners = self.inner.listeners.lock().await;
        target_state(
            radio_id,
            listeners
                .get(&radio_id)
                .and_then(|listener| listener.target.as_ref()),
        )
    }
}

impl Default for WsjtXManager {
    fn default() -> Self {
        Self::new()
    }
}

fn target_state(radio_id: i64, target: Option<&WsjtXTarget>) -> WsjtXTargetState {
    WsjtXTargetState {
        radio_id,
        logger_id: target.map(|target| target.logger_id.clone()),
        log_id: target.map(|target| target.log_id),
    }
}

fn no_target_state(radio_id: i64) -> WsjtXTargetState {
    target_state(radio_id, None)
}

async fn run_controller(
    radio_id: i64,
    config: RadioConfig,
    initial_state: Option<RadioState>,
    mut updates: broadcast::Receiver<RadioState>,
    mut target_updates: watch::Receiver<Option<WsjtXTarget>>,
    mut commands: mpsc::Receiver<ControllerCommand>,
    events: broadcast::Sender<WsjtXEvent>,
) {
    let mut mode = initial_state.map(|state| state.mode).unwrap_or_default();
    let mut running =
        reconcile_listener(None, radio_id, &config, &mode, &target_updates, &events).await;

    loop {
        tokio::select! {
            update = updates.recv() => match update {
                Ok(update) => {
                    let next_mode = update.mode;
                    if next_mode != mode {
                        mode = next_mode;
                        running = reconcile_listener(running, radio_id, &config, &mode, &target_updates, &events).await;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(radio_id, skipped, "WSJT-X radio-state subscription lagged");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            changed = target_updates.changed() => {
                if changed.is_err() {
                    break;
                }
                running = reconcile_listener(running, radio_id, &config, &mode, &target_updates, &events).await;
            },
            command = commands.recv() => match command {
                Some(ControllerCommand::Shutdown(completed)) => {
                    let _ = stop_listener(running, radio_id).await;
                    let _ = completed.send(());
                    return;
                }
                None => break,
            }
        }
    }
    let _ = stop_listener(running, radio_id).await;
}

async fn reconcile_listener(
    running: Option<RunningListener>,
    radio_id: i64,
    config: &RadioConfig,
    mode: &str,
    target_updates: &watch::Receiver<Option<WsjtXTarget>>,
    events: &broadcast::Sender<WsjtXEvent>,
) -> Option<RunningListener> {
    let should_run = listener_should_run(config, mode, target_updates.borrow().is_some());
    match (running, should_run) {
        (Some(running), true) => Some(running),
        (Some(running), false) => stop_listener(Some(running), radio_id).await,
        (None, false) => None,
        (None, true) => {
            let (shutdown, shutdown_rx) = oneshot::channel();
            let config = config.clone();
            let events = events.clone();
            let target_updates = target_updates.clone();
            let task = tokio::spawn(run_listener(
                radio_id,
                config,
                target_updates,
                shutdown_rx,
                events,
            ));
            Some(RunningListener { shutdown, task })
        }
    }
}

fn listener_should_run(config: &RadioConfig, mode: &str, has_target: bool) -> bool {
    config.wsjtx_enabled && mode.eq_ignore_ascii_case("DATA") && has_target
}

async fn stop_listener(running: Option<RunningListener>, radio_id: i64) -> Option<RunningListener> {
    if let Some(running) = running {
        let _ = running.shutdown.send(());
        let _ = running.task.await;
        info!(radio_id, "WSJT-X UDP listener stopped");
    }
    None
}

async fn run_listener(
    radio_id: i64,
    config: RadioConfig,
    target_updates: watch::Receiver<Option<WsjtXTarget>>,
    mut shutdown: oneshot::Receiver<()>,
    events: broadcast::Sender<WsjtXEvent>,
) {
    let server_config = match server_config(&config) {
        Ok(config) => config,
        Err(message) => {
            emit_target_error(&events, radio_id, &target_updates, message);
            return;
        }
    };
    let server = match WsjtXServer::spawn(server_config) {
        Ok(server) => server,
        Err(error) => {
            emit_target_error(
                &events,
                radio_id,
                &target_updates,
                format!("Unable to start WSJT-X UDP listener: {error}"),
            );
            return;
        }
    };
    let local_address = server.local_address();
    let mut wsjtx_events = server.subscribe();
    info!(radio_id, %local_address, "WSJT-X UDP listener started");

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            event = wsjtx_events.recv() => match event {
                Ok(Event::Datagram { datagram, .. }) => {
                    if let Message::LoggedAdif(logged) = &datagram.message {
                        let Some(target) = target_updates.borrow().clone() else {
                            continue;
                        };
                        match logged.text.as_deref() {
                            Some(text) => {
                                debug!(radio_id, log_id = target.log_id, wsjtx_instance_id = %datagram.id, adif = %text, "received WSJT-X Logged ADIF contact");
                                let _ = events.send(WsjtXEvent::LoggedAdif {
                                    radio_id,
                                    log_id: target.log_id,
                                    text: text.to_string(),
                                });
                            }
                            None => emit_error(
                                &events,
                                radio_id,
                                target.log_id,
                                "WSJT-X sent an empty Logged ADIF message".to_string(),
                            ),
                        }
                    }
                }
                Ok(Event::ProtocolError { source, error }) => emit_target_error(
                    &events,
                    radio_id,
                    &target_updates,
                    format!("Invalid WSJT-X datagram from {source}: {error}"),
                ),
                Ok(Event::SocketError { operation, peer, message, .. }) => emit_target_error(
                    &events,
                    radio_id,
                    &target_updates,
                    format!("WSJT-X UDP {operation} error{}: {message}", peer.map_or_else(String::new, |peer| format!(" for {peer}"))),
                ),
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(skipped)) => emit_target_error(
                    &events,
                    radio_id,
                    &target_updates,
                    format!("WSJT-X event receiver lagged; skipped {skipped} events"),
                ),
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    if let Err(error) = server.shutdown().await {
        emit_target_error(
            &events,
            radio_id,
            &target_updates,
            format!("WSJT-X UDP listener stopped with an error: {error}"),
        );
    }
}

fn emit_target_error(
    events: &broadcast::Sender<WsjtXEvent>,
    radio_id: i64,
    target_updates: &watch::Receiver<Option<WsjtXTarget>>,
    message: String,
) {
    if let Some(target) = target_updates.borrow().as_ref() {
        emit_error(events, radio_id, target.log_id, message);
    } else {
        error!(radio_id, %message, "WSJT-X error without an active target");
    }
}

fn emit_error(events: &broadcast::Sender<WsjtXEvent>, radio_id: i64, log_id: i64, message: String) {
    error!(radio_id, log_id, %message, "WSJT-X error");
    let _ = events.send(WsjtXEvent::Error {
        radio_id,
        log_id,
        message,
    });
}

fn server_config(config: &RadioConfig) -> Result<ServerConfig, String> {
    let bind_address = format!("{}:{}", config.wsjtx_bind_address, config.wsjtx_port)
        .parse()
        .map_err(|error| format!("Invalid WSJT-X bind address: {error}"))?;
    let mut multicast_groups = Vec::new();
    if !config.wsjtx_multicast_group.trim().is_empty() {
        let group = config
            .wsjtx_multicast_group
            .trim()
            .parse::<Ipv4Addr>()
            .map_err(|error| format!("Invalid WSJT-X multicast group: {error}"))?;
        let interface = config
            .wsjtx_bind_address
            .parse::<Ipv4Addr>()
            .map_err(|error| format!("Invalid WSJT-X multicast interface: {error}"))?;
        multicast_groups.push(MulticastGroup::V4 { group, interface });
    }
    Ok(ServerConfig {
        bind_address,
        multicast_groups,
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        server_revision: String::new(),
        ..ServerConfig::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manager() -> WsjtXManager {
        WsjtXManager::new()
    }

    fn test_config() -> RadioConfig {
        RadioConfig::new(
            1,
            crate::RadioSettings {
                name: "Dummy".to_string(),
                radio_kind: "dummy".to_string(),
                transport_kind: "none".to_string(),
                tcp_host: String::new(),
                tcp_port: 0,
                serial_port: String::new(),
                serial_baud_rate: 115_200,
                options: String::new(),
                data_mode: "DATA-USB".to_string(),
                rtty_mode: "RTTY".to_string(),
                wsjtx_enabled: true,
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

    #[test]
    fn builds_unicast_and_multicast_server_config() {
        let config = server_config(&test_config()).expect("unicast config is valid");
        assert_eq!(config.bind_address.to_string(), "127.0.0.1:2237");
        assert!(config.multicast_groups.is_empty());

        let mut radio = test_config();
        radio.wsjtx_bind_address = "0.0.0.0".to_string();
        radio.wsjtx_multicast_group = "239.255.0.1".to_string();
        let config = server_config(&radio).expect("multicast config is valid");
        assert_eq!(
            config.multicast_groups,
            vec![MulticastGroup::V4 {
                group: "239.255.0.1".parse().unwrap(),
                interface: Ipv4Addr::UNSPECIFIED,
            }]
        );
    }

    #[test]
    fn listener_requires_enabled_data_mode_and_a_target() {
        let config = test_config();
        assert!(listener_should_run(&config, "DATA", true));
        assert!(!listener_should_run(&config, "CW", true));
        assert!(!listener_should_run(&config, "DATA", false));

        let mut disabled = config;
        disabled.wsjtx_enabled = false;
        assert!(!listener_should_run(&disabled, "DATA", true));
    }

    #[tokio::test]
    async fn first_logger_is_target_and_later_logger_does_not_steal_it() {
        let manager = test_manager();
        let (_, updates) = broadcast::channel(4);
        let initial_state = Some(RadioState {
            frequency_hz: 14_074_000,
            mode: "CW".to_string(),
            rit_offset_hz: 0,
        });

        let first = manager
            .acquire(
                1,
                "logger-a",
                10,
                test_config(),
                initial_state.clone(),
                updates.resubscribe(),
            )
            .await
            .expect("first logger registers");
        assert_eq!(first.logger_id.as_deref(), Some("logger-a"));
        assert_eq!(first.log_id, Some(10));

        let second = manager
            .acquire(1, "logger-b", 11, test_config(), initial_state, updates)
            .await
            .expect("second logger registers");
        assert_eq!(second, first);

        manager.release(1, "logger-b").await;
        manager.release(1, "logger-a").await;
    }

    #[tokio::test]
    async fn target_switch_is_exclusive_and_target_close_does_not_reassign() {
        let manager = test_manager();
        let (_, updates) = broadcast::channel(4);
        let initial_state = Some(RadioState {
            frequency_hz: 14_074_000,
            mode: "CW".to_string(),
            rit_offset_hz: 0,
        });
        manager
            .acquire(
                1,
                "logger-a",
                10,
                test_config(),
                initial_state.clone(),
                updates.resubscribe(),
            )
            .await
            .expect("first logger registers");
        manager
            .acquire(1, "logger-b", 11, test_config(), initial_state, updates)
            .await
            .expect("second logger registers");

        let switched = manager
            .set_target(1, "logger-b", true)
            .await
            .expect("target switches");
        assert_eq!(switched.logger_id.as_deref(), Some("logger-b"));
        assert_eq!(switched.log_id, Some(11));

        manager.release(1, "logger-b").await;
        assert_eq!(manager.current_target(1).await, no_target_state(1));

        manager.release(1, "logger-a").await;
    }

    #[tokio::test]
    async fn target_can_be_explicitly_cleared() {
        let manager = test_manager();
        let (_, updates) = broadcast::channel(4);
        manager
            .acquire(
                1,
                "logger-a",
                10,
                test_config(),
                Some(RadioState {
                    frequency_hz: 14_074_000,
                    mode: "CW".to_string(),
                    rit_offset_hz: 0,
                }),
                updates,
            )
            .await
            .expect("logger registers");

        let cleared = manager
            .set_target(1, "logger-a", false)
            .await
            .expect("target clears");
        assert_eq!(cleared, no_target_state(1));

        manager.release(1, "logger-a").await;
    }
}
