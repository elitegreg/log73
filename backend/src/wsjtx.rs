use crate::{
    adif,
    bands::BandCatalog,
    contest_rules::ContestRulesStore,
    db::{Database, RadioConfig},
    log_cache::LogCache,
    radio::{RadioState, ServerMessage},
    scoring::IncrementalScoreTracker,
    validation,
};
use ham_radio_digital_interfacing::wsjtx::{
    Event, Message, MulticastGroup, ServerConfig, WsjtXServer,
};
use std::{collections::HashMap, net::Ipv4Addr, sync::Arc};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

#[derive(Clone)]
pub struct WsjtXManager {
    inner: Arc<WsjtXManagerInner>,
}

struct WsjtXManagerInner {
    listeners: Mutex<HashMap<i64, ManagedListener>>,
    ingestor: WsjtXIngestor,
}

struct ManagedListener {
    log_id: i64,
    refcount: usize,
    commands: mpsc::Sender<ControllerCommand>,
    task: JoinHandle<()>,
}

enum ControllerCommand {
    Reload(Box<RadioConfig>),
    Shutdown(oneshot::Sender<()>),
}

struct RunningListener {
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

#[derive(Clone)]
struct WsjtXIngestor {
    db: Database,
    rules: ContestRulesStore,
    bands: BandCatalog,
    log_cache: LogCache,
    scoring: IncrementalScoreTracker,
    events: broadcast::Sender<ServerMessage>,
}

impl WsjtXManager {
    pub fn new(
        db: Database,
        rules: ContestRulesStore,
        bands: BandCatalog,
        log_cache: LogCache,
        scoring: IncrementalScoreTracker,
        events: broadcast::Sender<ServerMessage>,
    ) -> Self {
        Self {
            inner: Arc::new(WsjtXManagerInner {
                listeners: Mutex::new(HashMap::new()),
                ingestor: WsjtXIngestor {
                    db,
                    rules,
                    bands,
                    log_cache,
                    scoring,
                    events,
                },
            }),
        }
    }

    pub async fn acquire(
        &self,
        radio_id: i64,
        log_id: i64,
        config: RadioConfig,
        initial_state: Option<RadioState>,
        updates: broadcast::Receiver<RadioState>,
    ) -> Result<(), String> {
        let mut listeners = self.inner.listeners.lock().await;
        if let Some(listener) = listeners.get_mut(&radio_id) {
            if listener.log_id != log_id {
                return Err(format!(
                    "radio {radio_id} is already assigned to log {}",
                    listener.log_id
                ));
            }
            listener.refcount += 1;
            return Ok(());
        }

        let (commands, command_rx) = mpsc::channel(8);
        let ingestor = self.inner.ingestor.clone();
        let task = tokio::spawn(run_controller(
            radio_id,
            log_id,
            config,
            initial_state,
            updates,
            command_rx,
            ingestor,
        ));
        listeners.insert(
            radio_id,
            ManagedListener {
                log_id,
                refcount: 1,
                commands,
                task,
            },
        );
        Ok(())
    }

    pub async fn release(&self, radio_id: i64) {
        let listener = {
            let mut listeners = self.inner.listeners.lock().await;
            let Some(listener) = listeners.get_mut(&radio_id) else {
                return;
            };
            listener.refcount = listener.refcount.saturating_sub(1);
            if listener.refcount > 0 {
                return;
            }
            listeners.remove(&radio_id)
        };
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

    pub async fn reload_config(&self, radio_id: i64, config: RadioConfig) {
        let commands = self
            .inner
            .listeners
            .lock()
            .await
            .get(&radio_id)
            .map(|listener| listener.commands.clone());
        if let Some(commands) = commands {
            let _ = commands
                .send(ControllerCommand::Reload(Box::new(config)))
                .await;
        }
    }
}

async fn run_controller(
    radio_id: i64,
    log_id: i64,
    mut config: RadioConfig,
    initial_state: Option<RadioState>,
    mut updates: broadcast::Receiver<RadioState>,
    mut commands: mpsc::Receiver<ControllerCommand>,
    ingestor: WsjtXIngestor,
) {
    let mut mode = initial_state.map(|state| state.mode).unwrap_or_default();
    let mut running = reconcile_listener(None, radio_id, log_id, &config, &mode, &ingestor).await;

    loop {
        tokio::select! {
            update = updates.recv() => match update {
                Ok(update) => {
                    let next_mode = update.mode;
                    if next_mode != mode {
                        mode = next_mode;
                        running = reconcile_listener(running, radio_id, log_id, &config, &mode, &ingestor).await;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(radio_id, log_id, skipped, "WSJT-X radio-state subscription lagged");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            command = commands.recv() => match command {
                Some(ControllerCommand::Reload(next_config)) => {
                    let needs_restart = listener_settings_changed(&config, &next_config);
                    config = *next_config;
                    if needs_restart {
                        running = stop_listener(running, radio_id, log_id).await;
                    }
                    running = reconcile_listener(running, radio_id, log_id, &config, &mode, &ingestor).await;
                }
                Some(ControllerCommand::Shutdown(completed)) => {
                    let _ = stop_listener(running, radio_id, log_id).await;
                    let _ = completed.send(());
                    return;
                }
                None => break,
            }
        }
    }
    let _ = stop_listener(running, radio_id, log_id).await;
}

async fn reconcile_listener(
    running: Option<RunningListener>,
    radio_id: i64,
    log_id: i64,
    config: &RadioConfig,
    mode: &str,
    ingestor: &WsjtXIngestor,
) -> Option<RunningListener> {
    let should_run = config.wsjtx_enabled && mode.eq_ignore_ascii_case("DATA");
    match (running, should_run) {
        (Some(running), true) => Some(running),
        (Some(running), false) => stop_listener(Some(running), radio_id, log_id).await,
        (None, false) => None,
        (None, true) => {
            let (shutdown, shutdown_rx) = oneshot::channel();
            let config = config.clone();
            let ingestor = ingestor.clone();
            let task = tokio::spawn(run_listener(
                radio_id,
                log_id,
                config,
                shutdown_rx,
                ingestor,
            ));
            Some(RunningListener { shutdown, task })
        }
    }
}

async fn stop_listener(
    running: Option<RunningListener>,
    radio_id: i64,
    log_id: i64,
) -> Option<RunningListener> {
    if let Some(running) = running {
        let _ = running.shutdown.send(());
        let _ = running.task.await;
        info!(radio_id, log_id, "WSJT-X UDP listener stopped");
    }
    None
}

async fn run_listener(
    radio_id: i64,
    log_id: i64,
    config: RadioConfig,
    mut shutdown: oneshot::Receiver<()>,
    ingestor: WsjtXIngestor,
) {
    let server_config = match server_config(&config) {
        Ok(config) => config,
        Err(message) => {
            ingestor.emit_error(radio_id, log_id, message);
            return;
        }
    };
    let server = match WsjtXServer::spawn(server_config) {
        Ok(server) => server,
        Err(error) => {
            ingestor.emit_error(
                radio_id,
                log_id,
                format!("Unable to start WSJT-X UDP listener: {error}"),
            );
            return;
        }
    };
    let local_address = server.local_address();
    let mut events = server.subscribe();
    info!(radio_id, log_id, %local_address, "WSJT-X UDP listener started");

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            event = events.recv() => match event {
                Ok(Event::Datagram { datagram, .. }) => {
                    if let Message::LoggedAdif(logged) = &datagram.message {
                        match logged.text.as_deref() {
                            Some(text) => {
                                if let Err(message) = ingestor.ingest(radio_id, log_id, text).await {
                                    ingestor.emit_error(radio_id, log_id, message);
                                }
                            }
                            None => ingestor.emit_error(
                                radio_id,
                                log_id,
                                "WSJT-X sent an empty Logged ADIF message".to_string(),
                            ),
                        }
                    }
                }
                Ok(Event::ProtocolError { source, error }) => ingestor.emit_error(
                    radio_id,
                    log_id,
                    format!("Invalid WSJT-X datagram from {source}: {error}"),
                ),
                Ok(Event::SocketError { operation, peer, message, .. }) => ingestor.emit_error(
                    radio_id,
                    log_id,
                    format!("WSJT-X UDP {operation} error{}: {message}", peer.map_or_else(String::new, |peer| format!(" for {peer}"))),
                ),
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(skipped)) => ingestor.emit_error(
                    radio_id,
                    log_id,
                    format!("WSJT-X event receiver lagged; skipped {skipped} events"),
                ),
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    if let Err(error) = server.shutdown().await {
        ingestor.emit_error(
            radio_id,
            log_id,
            format!("WSJT-X UDP listener stopped with an error: {error}"),
        );
    }
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

fn listener_settings_changed(previous: &RadioConfig, next: &RadioConfig) -> bool {
    previous.wsjtx_enabled != next.wsjtx_enabled
        || previous.wsjtx_bind_address != next.wsjtx_bind_address
        || previous.wsjtx_port != next.wsjtx_port
        || previous.wsjtx_multicast_group != next.wsjtx_multicast_group
}

impl WsjtXIngestor {
    async fn ingest(&self, radio_id: i64, log_id: i64, text: &str) -> Result<(), String> {
        let log = self
            .db
            .log(log_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("WSJT-X target log {log_id} was not found"))?;
        let rules = self
            .rules
            .get(&log.contest_id)
            .ok_or_else(|| format!("Unknown contest: {}", log.contest_id))?;
        let contact = adif::import_wsjtx_contact(rules, text)
            .map_err(|error| format!("Unable to import WSJT-X ADIF: {}", error.error))?;
        validation::validate_contacts(
            &self.db,
            &self.rules,
            self.bands.snapshot().as_ref(),
            log_id,
            std::slice::from_ref(&contact),
        )
        .await
        .map_err(|error| format!("Unable to validate WSJT-X contact: {error}"))?;
        let result = self
            .log_cache
            .upsert_contacts(log_id, vec![contact])
            .await
            .map_err(|error| format!("Unable to save WSJT-X contact: {error}"))?;
        for contact in result.contacts.into_iter().chain(result.changed_contacts) {
            let _ = self.events.send(ServerMessage::LogEntry { contact });
        }
        let totals = self.scoring.totals(log_id).unwrap_or_default();
        let _ = self.events.send(ServerMessage::ScoreUpdate {
            log_id,
            qso_count: totals.qso_count,
            multipliers: totals.multipliers,
            bonus_points: totals.bonus_points,
            total_score: totals.score,
        });
        debug!(radio_id, log_id, "saved WSJT-X Logged ADIF contact");
        Ok(())
    }

    fn emit_error(&self, radio_id: i64, log_id: i64, message: String) {
        error!(radio_id, log_id, %message, "WSJT-X error");
        let _ = self.events.send(ServerMessage::WsjtXError {
            radio_id,
            log_id,
            message,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> RadioConfig {
        RadioConfig {
            id: 1,
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
        }
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
    fn listener_restart_detection_uses_only_wsjtx_settings() {
        let config = test_config();
        let mut changed = config.clone();
        changed.wsjtx_port = 2238;
        assert!(listener_settings_changed(&config, &changed));

        changed = config.clone();
        changed.name = "Renamed".to_string();
        assert!(!listener_settings_changed(&config, &changed));
    }
}
