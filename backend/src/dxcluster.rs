use crate::db::DxClusterConfig;
use dxcllistener::Listener;
use dxclparser::{DX, ParseError, RBN, Spot};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

const DXCLUSTER_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DXCLUSTER_PRUNE_INTERVAL: Duration = Duration::from_secs(30);
const DXCLUSTER_EVENT_BUFFER: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DxClusterRbnSpot {
    pub mode: String,
    pub db: i16,
    pub speed: Option<u16>,
    pub speed_unit: Option<String>,
    pub info: String,
    pub loc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DxClusterSpot {
    pub id: u64,
    pub received_at: u64,
    pub source: String,
    pub call_de: String,
    pub call_dx: String,
    pub frequency_hz: u64,
    pub utc: u16,
    pub loc: Option<String>,
    pub comment: Option<String>,
    pub rbn: Option<DxClusterRbnSpot>,
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
    outbound: Mutex<Option<mpsc::Sender<DxClusterOutboundCommand>>>,
}

struct DxClusterOutboundCommand {
    text: String,
    completed: oneshot::Sender<Result<(), String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DxClusterDedupeKey {
    frequency_tenth_khz: u64,
    call_dx: String,
}

#[derive(Default)]
struct DxClusterSpotStore {
    next_id: u64,
    ids_by_time: VecDeque<u64>,
    ids_by_callsign: HashMap<String, BTreeSet<u64>>,
    ids_by_dedupe_key: HashMap<DxClusterDedupeKey, u64>,
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
                outbound: Mutex::new(None),
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
        let (outbound_tx, outbound_rx) = mpsc::channel(64);
        *self.inner.outbound.lock().await = Some(outbound_tx);
        let handle = tokio::spawn(async move {
            run_dxcluster_task(config, manager, outbound_rx).await;
        });
        *self.inner.task.lock().await = Some(handle);
    }

    pub async fn send_text(&self, text: impl Into<String>) -> Result<(), String> {
        let (completed_tx, completed_rx) = oneshot::channel();
        let command = DxClusterOutboundCommand {
            text: text.into(),
            completed: completed_tx,
        };
        let sender = self
            .inner
            .outbound
            .lock()
            .await
            .clone()
            .ok_or_else(|| "DX cluster listener is not running".to_string())?;
        sender
            .send(command)
            .await
            .map_err(|_| "DX cluster listener is not running".to_string())?;
        completed_rx
            .await
            .map_err(|_| "DX cluster listener stopped before sending text".to_string())?
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
        *self.inner.outbound.lock().await = None;
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
        if let Some(spot) = spot {
            let _ = self.inner.events.send(DxClusterEvent::SpotAdded(spot));
        }
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
    fn add(&mut self, dx: DX) -> Option<DxClusterSpot> {
        let now = unix_timestamp_secs();
        let call_dx = dx.call_dx.to_uppercase();
        let rbn = parsed_rbn(&dx);
        let source = if rbn.is_some() { "rbn" } else { "dx" }.to_string();
        let new_dedupe_key = dedupe_key(dx.freq, &call_dx);

        if let Some(id) = self.find_duplicate_id(&new_dedupe_key) {
            if let Some(spot) = self.spots_by_id.get_mut(&id) {
                let old_dedupe_key = dedupe_key(spot.frequency_hz, &spot.call_dx);
                spot.received_at = now;
                spot.source = source;
                spot.call_de = dx.call_de;
                spot.call_dx = call_dx;
                spot.frequency_hz = dx.freq;
                spot.utc = dx.utc;
                spot.loc = dx.loc;
                spot.comment = dx.comment;
                spot.rbn = rbn;
                self.ids_by_time.retain(|current_id| *current_id != id);
                self.ids_by_time.push_back(id);
                self.ids_by_dedupe_key.remove(&old_dedupe_key);
                self.ids_by_dedupe_key.insert(new_dedupe_key, id);
                return None;
            }
        }

        self.next_id = self.next_id.saturating_add(1).max(1);
        let id = self.next_id;
        let spot = DxClusterSpot {
            id,
            received_at: now,
            source,
            call_de: dx.call_de,
            call_dx,
            frequency_hz: dx.freq,
            utc: dx.utc,
            loc: dx.loc,
            comment: dx.comment,
            rbn,
        };

        self.ids_by_time.push_back(id);
        self.ids_by_callsign
            .entry(spot.call_dx.clone())
            .or_default()
            .insert(id);
        self.ids_by_dedupe_key.insert(new_dedupe_key, id);
        self.spots_by_id.insert(id, spot.clone());
        Some(spot)
    }

    fn find_duplicate_id(&mut self, key: &DxClusterDedupeKey) -> Option<u64> {
        let mut candidate_keys = Vec::with_capacity(3);
        candidate_keys.push(key.clone());
        if let Some(previous) = key.frequency_tenth_khz.checked_sub(1) {
            candidate_keys.push(DxClusterDedupeKey {
                frequency_tenth_khz: previous,
                call_dx: key.call_dx.clone(),
            });
        }
        candidate_keys.push(DxClusterDedupeKey {
            frequency_tenth_khz: key.frequency_tenth_khz.saturating_add(1),
            call_dx: key.call_dx.clone(),
        });

        for candidate_key in candidate_keys {
            if let Some(id) = self.ids_by_dedupe_key.get(&candidate_key).copied() {
                if self.spots_by_id.contains_key(&id) {
                    return Some(id);
                }
                self.ids_by_dedupe_key.remove(&candidate_key);
            }
        }

        None
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
        self.ids_by_dedupe_key.clear();
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
        self.ids_by_dedupe_key
            .remove(&dedupe_key(spot.frequency_hz, &spot.call_dx));

        deleted_ids.push(id);
    }
}

fn dedupe_key(frequency_hz: u64, call_dx: &str) -> DxClusterDedupeKey {
    DxClusterDedupeKey {
        frequency_tenth_khz: (frequency_hz + 50) / 100,
        call_dx: call_dx.to_uppercase(),
    }
}

fn parsed_rbn(dx: &DX) -> Option<DxClusterRbnSpot> {
    dx.comment
        .as_deref()
        .and_then(|comment| dxclparser::parse_rbn(comment).ok())
        .map(rbn_spot)
}

fn rbn_spot(rbn: RBN) -> DxClusterRbnSpot {
    DxClusterRbnSpot {
        mode: rbn.mode,
        db: rbn.db,
        speed: rbn.speed,
        speed_unit: rbn.speed_unit,
        info: rbn.info,
        loc: rbn.loc,
    }
}

async fn run_dxcluster_task(
    config: DxClusterConfig,
    manager: DxClusterManager,
    mut outbound_rx: mpsc::Receiver<DxClusterOutboundCommand>,
) {
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
            command = outbound_rx.recv() => {
                let Some(command) = command else {
                    break;
                };
                let result = listener
                    .send_text(command.text)
                    .map_err(|error| error.to_string());
                let _ = command.completed.send(result);
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

pub fn format_dxcluster_frequency_khz(frequency_hz: u64) -> String {
    let whole_khz = frequency_hz / 1000;
    let fractional_hz = frequency_hz % 1000;
    if fractional_hz == 0 {
        return whole_khz.to_string();
    }

    let mut fraction = format!("{fractional_hz:03}");
    while fraction.ends_with('0') {
        fraction.pop();
    }
    format!("{whole_khz}.{fraction}")
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
        dx_with_freq(call_dx, 14_074_000)
    }

    fn dx_with_freq(call_dx: &str, freq: u64) -> DX {
        DX {
            call_de: "N0CALL".to_string(),
            call_dx: call_dx.to_string(),
            freq,
            utc: 1234,
            loc: Some("EM73".to_string()),
            comment: Some("test".to_string()),
        }
    }

    #[test]
    fn formats_dxcluster_frequency_in_khz() {
        assert_eq!(format_dxcluster_frequency_khz(14_074_000), "14074");
        assert_eq!(format_dxcluster_frequency_khz(14_074_100), "14074.1");
        assert_eq!(format_dxcluster_frequency_khz(14_074_125), "14074.125");
    }

    #[tokio::test]
    async fn deduplicates_same_callsign_and_tenth_khz_without_broadcast() {
        let manager = DxClusterManager::new();
        manager
            .add_dx_spot(dx_with_freq("k1abc", 14_074_241), Duration::from_secs(60))
            .await;
        let mut events = manager.subscribe();
        let mut duplicate = dx_with_freq("K1ABC", 14_074_249);
        duplicate.call_de = "W9XYZ".to_string();
        duplicate.comment = Some("updated".to_string());

        manager
            .add_dx_spot(duplicate, Duration::from_secs(60))
            .await;

        let spots = manager.spots().await;
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].call_de, "W9XYZ");
        assert_eq!(spots[0].comment.as_deref(), Some("updated"));
        assert!(matches!(
            events.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn deduplicates_adjacent_tenth_khz_for_same_callsign() {
        let manager = DxClusterManager::new();
        manager
            .add_dx_spot(dx_with_freq("K1ABC", 14_074_000), Duration::from_secs(60))
            .await;
        let mut adjacent = dx_with_freq("K1ABC", 14_074_100);
        adjacent.comment = Some("adjacent".to_string());

        manager.add_dx_spot(adjacent, Duration::from_secs(60)).await;

        let spots = manager.spots().await;
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].frequency_hz, 14_074_100);
        assert_eq!(spots[0].comment.as_deref(), Some("adjacent"));
    }

    #[tokio::test]
    async fn keeps_spots_two_tenths_khz_apart() {
        let manager = DxClusterManager::new();
        manager
            .add_dx_spot(dx_with_freq("K1ABC", 14_074_000), Duration::from_secs(60))
            .await;
        manager
            .add_dx_spot(dx_with_freq("K1ABC", 14_074_200), Duration::from_secs(60))
            .await;

        assert_eq!(manager.spots().await.len(), 2);
    }

    #[tokio::test]
    async fn detects_rbn_comment_metadata() {
        let manager = DxClusterManager::new();
        let mut rbn = dx_with_freq("K1ABC", 14_074_241);
        rbn.comment = Some("CW 12 dB 24 WPM CQ".to_string());

        manager.add_dx_spot(rbn, Duration::from_secs(60)).await;

        let spots = manager.spots().await;
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].source, "rbn");
        let rbn = spots[0].rbn.as_ref().expect("rbn metadata should exist");
        assert_eq!(rbn.mode, "CW");
        assert_eq!(rbn.db, 12);
        assert_eq!(rbn.speed, Some(24));
        assert_eq!(rbn.speed_unit.as_deref(), Some("WPM"));
        assert_eq!(rbn.info, "CQ");
    }

    #[tokio::test]
    async fn rbn_and_dx_same_callsign_and_tenth_khz_dedupe_to_latest() {
        let manager = DxClusterManager::new();
        let mut rbn = dx_with_freq("K1ABC", 14_074_241);
        rbn.comment = Some("CW 12 dB 24 WPM CQ".to_string());
        manager.add_dx_spot(rbn, Duration::from_secs(60)).await;
        let mut events = manager.subscribe();
        let mut dx = dx_with_freq("K1ABC", 14_074_249);
        dx.comment = Some("normal spot".to_string());

        manager.add_dx_spot(dx, Duration::from_secs(60)).await;

        let spots = manager.spots().await;
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].source, "dx");
        assert!(spots[0].rbn.is_none());
        assert_eq!(spots[0].comment.as_deref(), Some("normal spot"));
        assert!(matches!(
            events.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn dx_and_rbn_same_callsign_and_tenth_khz_dedupe_to_latest_rbn() {
        let manager = DxClusterManager::new();
        manager
            .add_dx_spot(dx_with_freq("K1ABC", 14_074_241), Duration::from_secs(60))
            .await;
        let mut rbn = dx_with_freq("K1ABC", 14_074_249);
        rbn.comment = Some("FT8 -7 dB EM73 CQ".to_string());

        manager.add_dx_spot(rbn, Duration::from_secs(60)).await;

        let spots = manager.spots().await;
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].source, "rbn");
        assert_eq!(
            spots[0].rbn.as_ref().map(|rbn| rbn.mode.as_str()),
            Some("FT8")
        );
    }

    #[tokio::test]
    async fn permits_same_tenth_khz_for_different_callsigns() {
        let manager = DxClusterManager::new();
        manager
            .add_dx_spot(dx_with_freq("K1ABC", 14_074_241), Duration::from_secs(60))
            .await;
        manager
            .add_dx_spot(dx_with_freq("N5DEF", 14_074_249), Duration::from_secs(60))
            .await;

        assert_eq!(manager.spots().await.len(), 2);
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
