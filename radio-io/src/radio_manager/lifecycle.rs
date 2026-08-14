use super::cat_runtime::{ManagedRadioRuntime, debug_radio_config, run_managed_radio};
use crate::bands::BandCatalog;
use crate::config::RadioConfig;
use crate::radio::{RadioCommand, RadioState, RadioStatus};
use crate::voice_keyer::VoiceKeyer;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify, RwLock, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::debug;

#[derive(Clone)]
pub struct RadioManager {
    voice_keyer: VoiceKeyer,
    bands: BandCatalog,
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
    pub fn new(voice_keyer: VoiceKeyer, bands: BandCatalog) -> Self {
        Self {
            voice_keyer,
            bands,
            radios: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn acquire(&self, mut config: RadioConfig) -> Result<RadioHandle, String> {
        let radio_id = config.id;
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
                    let task_bands = self.bands.clone();
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
                            task_bands,
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

    /// Stops every managed radio, including connections still held by WebSocket
    /// sessions. Hosts use this during process-level shutdown.
    pub async fn shutdown_all(&self) {
        let radio_ids = self.radios.lock().await.keys().copied().collect::<Vec<_>>();
        for radio_id in radio_ids {
            loop {
                let active = self.is_active(radio_id).await;
                if !active {
                    break;
                }
                self.release(radio_id).await;
            }
        }
    }
}

impl RadioHandle {
    pub async fn current_state(&self) -> Option<RadioState> {
        self.current.read().await.clone()
    }

    pub async fn current_status(&self) -> RadioStatus {
        self.current_status.read().await.clone()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cw::DEFAULT_CW_MESSAGES, voice_messages::DEFAULT_VOICE_MESSAGES};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::{Duration, Instant, sleep};

    fn test_radio() -> RadioConfig {
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
                wsjtx_enabled: false,
                wsjtx_bind_address: "127.0.0.1".to_string(),
                wsjtx_port: 2237,
                wsjtx_multicast_group: String::new(),
                flrig_enabled: false,
                flrig_port: crate::DEFAULT_FLRIG_PORT,
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
                cw_messages: DEFAULT_CW_MESSAGES.to_string(),
                voice_messages: DEFAULT_VOICE_MESSAGES.to_string(),
            },
        )
    }

    #[tokio::test]
    async fn active_radio_is_shared_by_all_logger_sessions() {
        let radio = test_radio();
        let manager = RadioManager::new(VoiceKeyer::new(), BandCatalog::new(Vec::new()));

        manager
            .acquire(radio.clone())
            .await
            .expect("first logger acquires radio");
        manager
            .acquire(radio.clone())
            .await
            .expect("logger for a different log shares radio");

        manager.release(radio.id).await;
        assert!(manager.is_active(radio.id).await);
        manager.release(radio.id).await;
        assert!(!manager.is_active(radio.id).await);
    }

    #[tokio::test]
    async fn flrig_listener_follows_managed_cat_lifecycle() {
        let port = available_tcp_port();
        let mut radio = test_radio();
        radio.flrig_enabled = true;
        radio.flrig_port = port;
        let manager = RadioManager::new(VoiceKeyer::new(), BandCatalog::new(Vec::new()));

        manager
            .acquire(radio.clone())
            .await
            .expect("acquires radio");
        wait_for_listener(port).await;

        manager.release(radio.id).await;
        assert!(TcpStream::connect(local_address(port)).await.is_err());
    }

    #[tokio::test]
    async fn flrig_bind_conflict_keeps_cat_online_and_retries() {
        let conflict = TcpListener::bind(local_address(0))
            .await
            .expect("bind conflicting listener");
        let port = conflict.local_addr().expect("conflict address").port();
        let mut radio = test_radio();
        radio.flrig_enabled = true;
        radio.flrig_port = port;
        let manager = RadioManager::new(VoiceKeyer::new(), BandCatalog::new(Vec::new()));

        let handle = manager
            .acquire(radio.clone())
            .await
            .expect("acquires radio");
        wait_for_online(&handle).await;
        drop(conflict);
        wait_for_listener(port).await;

        manager.release(radio.id).await;
    }

    fn available_tcp_port() -> u16 {
        std::net::TcpListener::bind(local_address(0))
            .expect("bind temporary listener")
            .local_addr()
            .expect("temporary listener address")
            .port()
    }

    fn local_address(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    async fn wait_for_listener(port: u16) {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if TcpStream::connect(local_address(port)).await.is_ok() {
                return;
            }
            assert!(Instant::now() < deadline, "FLRig listener did not start");
            sleep(Duration::from_millis(25)).await;
        }
    }

    async fn wait_for_online(handle: &RadioHandle) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if handle.current_status().await.online {
                return;
            }
            assert!(Instant::now() < deadline, "CAT radio did not become online");
            sleep(Duration::from_millis(25)).await;
        }
    }
}
