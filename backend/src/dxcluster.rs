use crate::db::DxClusterConfig;
use dxcllistener::Listener;
use dxclparser::{DX, ParseError, Spot};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, broadcast, mpsc};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

const DXCLUSTER_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DXCLUSTER_PRUNE_INTERVAL: Duration = Duration::from_secs(30);
const DXCLUSTER_EVENT_BUFFER: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DxClusterSpot {
    pub id: u64,
    pub received_at: u64,
    pub call_de: String,
    pub call_dx: String,
    pub frequency_hz: u64,
    pub utc: u16,
    pub loc: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone)]
pub enum DxClusterEvent {
    SpotAdded(DxClusterSpot),
    SpotDeleted { id: u64 },
}

#[derive(Clone)]
pub struct DxClusterManager {
    inner: Arc<DxClusterManagerInner>,
}

struct DxClusterManagerInner {
    store: Mutex<DxClusterSpotStore>,
    events: broadcast::Sender<DxClusterEvent>,
    task: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Default)]
struct DxClusterSpotStore {
    next_id: u64,
    ids_by_time: VecDeque<u64>,
    ids_by_callsign: HashMap<String, BTreeSet<u64>>,
    spots_by_id: HashMap<u64, DxClusterSpot>,
}

impl DxClusterManager {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(DXCLUSTER_EVENT_BUFFER);
        Self {
            inner: Arc::new(DxClusterManagerInner {
                store: Mutex::new(DxClusterSpotStore::default()),
                events,
                task: Mutex::new(None),
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DxClusterEvent> {
        self.inner.events.subscribe()
    }

    pub async fn apply_config(&self, config: DxClusterConfig) {
        self.stop_task().await;

        if !config.enabled {
            info!("dxcluster disabled; listener task not started");
            return;
        }

        let manager = self.clone();
        let handle = tokio::spawn(async move {
            run_dxcluster_task(config, manager).await;
        });
        *self.inner.task.lock().await = Some(handle);
    }

    pub async fn spots(&self) -> Vec<DxClusterSpot> {
        self.inner.store.lock().await.spots()
    }

    pub async fn clear_spots(&self) -> Vec<u64> {
        let deleted_ids = self.inner.store.lock().await.clear();
        self.broadcast_deletes(&deleted_ids);
        deleted_ids
    }

    async fn stop_task(&self) {
        if let Some(task) = self.inner.task.lock().await.take() {
            task.abort();
        }
    }

    async fn add_dx_spot(&self, dx: DX, max_age: Duration) {
        let (spot, deleted_ids) = {
            let mut store = self.inner.store.lock().await;
            let deleted_ids = store.prune(max_age);
            let spot = store.add(dx);
            (spot, deleted_ids)
        };

        self.broadcast_deletes(&deleted_ids);
        let _ = self.inner.events.send(DxClusterEvent::SpotAdded(spot));
    }

    async fn prune_old_spots(&self, max_age: Duration) {
        let deleted_ids = self.inner.store.lock().await.prune(max_age);
        self.broadcast_deletes(&deleted_ids);
    }

    fn broadcast_deletes(&self, deleted_ids: &[u64]) {
        for id in deleted_ids {
            let _ = self
                .inner
                .events
                .send(DxClusterEvent::SpotDeleted { id: *id });
        }
    }
}

impl Default for DxClusterManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DxClusterSpotStore {
    fn add(&mut self, dx: DX) -> DxClusterSpot {
        self.next_id = self.next_id.saturating_add(1).max(1);
        let id = self.next_id;
        let spot = DxClusterSpot {
            id,
            received_at: unix_timestamp_secs(),
            call_de: dx.call_de,
            call_dx: dx.call_dx.to_uppercase(),
            frequency_hz: dx.freq,
            utc: dx.utc,
            loc: dx.loc,
            comment: dx.comment,
        };

        self.ids_by_time.push_back(id);
        self.ids_by_callsign
            .entry(spot.call_dx.clone())
            .or_default()
            .insert(id);
        self.spots_by_id.insert(id, spot.clone());
        spot
    }

    fn spots(&self) -> Vec<DxClusterSpot> {
        self.ids_by_time
            .iter()
            .filter_map(|id| self.spots_by_id.get(id).cloned())
            .collect()
    }

    fn prune(&mut self, max_age: Duration) -> Vec<u64> {
        let cutoff = unix_timestamp_secs().saturating_sub(max_age.as_secs());
        let mut deleted_ids = Vec::new();

        while let Some(id) = self.ids_by_time.front().copied() {
            let Some(spot) = self.spots_by_id.get(&id) else {
                self.ids_by_time.pop_front();
                continue;
            };
            if spot.received_at >= cutoff {
                break;
            }
            self.ids_by_time.pop_front();
            self.remove_by_id(id, &mut deleted_ids);
        }

        deleted_ids
    }

    fn clear(&mut self) -> Vec<u64> {
        let deleted_ids: Vec<u64> = self.ids_by_time.drain(..).collect();
        self.spots_by_id.clear();
        self.ids_by_callsign.clear();
        deleted_ids
    }

    fn remove_by_id(&mut self, id: u64, deleted_ids: &mut Vec<u64>) {
        let Some(spot) = self.spots_by_id.remove(&id) else {
            return;
        };

        if let Some(ids) = self.ids_by_callsign.get_mut(&spot.call_dx) {
            ids.remove(&id);
            if ids.is_empty() {
                self.ids_by_callsign.remove(&spot.call_dx);
            }
        }

        deleted_ids.push(id);
    }
}

async fn run_dxcluster_task(config: DxClusterConfig, manager: DxClusterManager) {
    let host = config.host.trim().to_string();
    let callsign = config.callsign.trim().to_uppercase();
    let port = config.port;
    let max_age = Duration::from_secs(u64::from(config.max_age_min) * 60);

    if host.is_empty() {
        error!("dxcluster host is empty; listener task stopped");
        return;
    }
    if port == 0 {
        error!("dxcluster port is 0; listener task stopped");
        return;
    }
    if callsign.is_empty() {
        error!("dxcluster callsign is empty; listener task stopped");
        return;
    }

    manager.prune_old_spots(max_age).await;

    let (line_tx, mut line_rx) = mpsc::unbounded_channel();
    let mut listener = Listener::new(host.clone(), port, callsign.clone());
    info!(host = %host, port, callsign = %callsign, "starting dxcluster listener");

    if let Err(error) = listener.listen(line_tx, DXCLUSTER_CONNECT_TIMEOUT).await {
        error!(host = %host, port, callsign = %callsign, %error, "failed to start dxcluster listener");
        return;
    }

    info!(host = %host, port, callsign = %callsign, "dxcluster listener connected");
    for command in config
        .commands
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Err(error) = listener.send_text(command) {
            warn!(host = %host, port, callsign = %callsign, command, %error, "failed to send dxcluster command");
        }
    }

    let mut prune_interval = tokio::time::interval(DXCLUSTER_PRUNE_INTERVAL);
    loop {
        tokio::select! {
            line = line_rx.recv() => {
                let Some(line) = line else {
                    break;
                };
                handle_cluster_line(&line, &manager, max_age).await;
            }
            _ = prune_interval.tick() => {
                manager.prune_old_spots(max_age).await;
                if !listener.is_running() {
                    break;
                }
            }
        }
    }

    if listener.is_running() {
        let _ = listener.request_stop();
    }

    match listener.join().await {
        Ok(()) => info!(host = %host, port, callsign = %callsign, "dxcluster listener stopped"),
        Err(error) => {
            warn!(host = %host, port, callsign = %callsign, %error, "dxcluster listener stopped with error")
        }
    }
}

async fn handle_cluster_line(line: &str, manager: &DxClusterManager, max_age: Duration) {
    match dxclparser::parse(line) {
        Ok(Spot::DX(dx)) => manager.add_dx_spot(dx, max_age).await,
        Ok(_) => {}
        Err(ParseError::UnknownType) => debug!(line, "ignoring non-spot dxcluster line"),
        Err(error) => debug!(line, %error, "failed to parse dxcluster line"),
    }
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dx(call_dx: &str) -> DX {
        DX {
            call_de: "N0CALL".to_string(),
            call_dx: call_dx.to_string(),
            freq: 14_074_000,
            utc: 1234,
            loc: Some("EM73".to_string()),
            comment: Some("test".to_string()),
        }
    }

    #[tokio::test]
    async fn stores_and_clears_spots() {
        let manager = DxClusterManager::new();
        manager
            .add_dx_spot(dx("k1abc"), Duration::from_secs(60))
            .await;

        let spots = manager.spots().await;
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].call_dx, "K1ABC");

        let deleted_ids = manager.clear_spots().await;
        assert_eq!(deleted_ids.len(), 1);
        assert!(manager.spots().await.is_empty());
    }
}
