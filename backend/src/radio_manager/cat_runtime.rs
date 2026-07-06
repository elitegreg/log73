use super::commands::{apply_command, fail_unavailable_radio_command, logger_state_from_cat_state};
use super::cw_task::{CwTaskCommand, run_cw_task};
use super::keyers::{CwSerialDevice, open_serial_keyer};
use crate::db::RadioConfig;
use crate::radio::{RadioCommand, RadioState, RadioStatus};
use crate::voice_keyer::VoiceKeyer;
use backon::{BackoffBuilder, ExponentialBuilder};
use radio_cat_rs::{AsyncIoTransport, ConnectionState, Radio, RadioTask, TransportConfig};
use radio_cat_rs::{RadioConfig as CatRadioConfig, RadioError};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, trace, warn};

const CAT_RECONNECT_MIN_DELAY: Duration = Duration::from_secs(1);
const CAT_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(10);

pub(super) struct ManagedRadioRuntime {
    pub(super) current_status: Arc<RwLock<RadioStatus>>,
    pub(super) current: Arc<RwLock<Option<RadioState>>>,
    pub(super) status_updates: broadcast::Sender<RadioStatus>,
    pub(super) updates: broadcast::Sender<RadioState>,
}

pub(super) async fn run_managed_radio(
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

pub(super) fn uses_shared_cw_serial_port(config: &RadioConfig) -> bool {
    config.transport_kind.trim().eq_ignore_ascii_case("serial")
        && config.cw_keyer_type.trim().eq_ignore_ascii_case("serial")
        && !config.serial_port.trim().is_empty()
        && config.serial_port.trim() == config.cw_serial_port.trim()
}

pub(super) fn cat_radio_config_for(config: &RadioConfig) -> Result<CatRadioConfig, String> {
    let mut cat_config = CatRadioConfig::new(config.radio_kind.trim())
        .with_transport(transport_config_for(config)?)
        .with_options(config.options.clone());

    if config.radio_kind.trim().eq_ignore_ascii_case("dummy") {
        cat_config = cat_config.with_transport(TransportConfig::None);
    }

    Ok(cat_config)
}

pub(super) fn transport_config_for(config: &RadioConfig) -> Result<TransportConfig, String> {
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

pub(super) fn cat_reconnect_backoff() -> ExponentialBuilder {
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

pub(super) fn debug_radio_config(config: &RadioConfig, message: &'static str) {
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

fn is_unsupported_capability(error: &RadioError) -> bool {
    matches!(error, RadioError::UnsupportedCapability { .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::RadioConfig;

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
}
