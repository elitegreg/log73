use crate::bands::{Band, band_for_frequency};
use crate::db::{Contact, contact_adif, contact_adif_value};
use crate::dxcluster::{DxClusterRbnSpot, DxClusterSpot};
use crate::log_cache::LogCacheProcessor;
use crate::scoring::ContestScoringModule;
use radio_cat_rs::Frequency;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

const BANDMAP_EVENT_BUFFER: usize = 512;
const BANDMAP_PRUNE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BandMapSpotType {
    Dx,
    Rbn,
    Local,
    InUse,
    Cq,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BandMapSpot {
    pub id: u64,
    pub received_at: u64,
    pub spot_type: BandMapSpotType,
    pub source: String,
    pub call_de: String,
    pub call_dx: String,
    pub frequency_hz: u64,
    pub utc: u16,
    pub loc: Option<String>,
    pub comment: Option<String>,
    pub rbn: Option<DxClusterRbnSpot>,
    pub band_name: Option<String>,
    pub radio_id: Option<i64>,
    pub radio_name: Option<String>,
    pub log_id: Option<i64>,
    pub exchange_fields: Option<Map<String, Value>>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum BandMapEvent {
    SpotUpserted(Box<BandMapSpot>),
    SpotDeleted { id: u64 },
}

#[derive(Debug, Clone)]
pub struct LocalSpotInput {
    pub frequency_hz: u64,
    pub call_dx: String,
    pub comment: Option<String>,
    pub radio_id: Option<i64>,
    pub radio_name: Option<String>,
    pub log_id: Option<i64>,
    pub exchange_fields: Option<Map<String, Value>>,
    pub received_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct CqSpotInput {
    pub frequency_hz: u64,
    pub radio_id: i64,
    pub radio_name: String,
}

#[derive(Debug, Clone)]
pub struct InUseSpotInput {
    pub frequency_hz: u64,
}

#[derive(Clone)]
pub struct BandMapManager {
    inner: Arc<BandMapManagerInner>,
}

struct BandMapManagerInner {
    bands: Arc<Vec<Band>>,
    max_age: Mutex<Duration>,
    store: Mutex<BandMapSpotStore>,
    events: broadcast::Sender<BandMapEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum BandMapDedupeKey {
    Cluster {
        source_id: u64,
    },
    Local {
        log_id: Option<i64>,
        band_name: String,
        call_dx: String,
    },
    LocalUnknownBand {
        log_id: Option<i64>,
        frequency_tenth_khz: u64,
        call_dx: String,
    },
    InUse {
        frequency_tenth_khz: u64,
    },
    Cq {
        radio_id: i64,
        band_name: String,
    },
}

#[derive(Default)]
struct BandMapSpotStore {
    next_id: u64,
    ids_by_time: VecDeque<u64>,
    spots_by_id: HashMap<u64, BandMapSpot>,
    id_by_key: HashMap<BandMapDedupeKey, u64>,
    key_by_id: HashMap<u64, BandMapDedupeKey>,
}

#[derive(Debug, Clone)]
struct BandMapSpotCandidate {
    received_at: u64,
    spot_type: BandMapSpotType,
    source: String,
    call_de: String,
    call_dx: String,
    frequency_hz: u64,
    utc: u16,
    loc: Option<String>,
    comment: Option<String>,
    rbn: Option<DxClusterRbnSpot>,
    band_name: Option<String>,
    radio_id: Option<i64>,
    radio_name: Option<String>,
    log_id: Option<i64>,
    exchange_fields: Option<Map<String, Value>>,
}

enum UpsertOutcome {
    Inserted(BandMapSpot),
    Updated(BandMapSpot),
    Refreshed(BandMapSpot),
}

impl BandMapManager {
    pub fn new(bands: Arc<Vec<Band>>, max_age: Duration) -> Self {
        let (events, _) = broadcast::channel(BANDMAP_EVENT_BUFFER);
        let manager = Self {
            inner: Arc::new(BandMapManagerInner {
                bands,
                max_age: Mutex::new(max_age),
                store: Mutex::new(BandMapSpotStore::default()),
                events,
            }),
        };
        manager.spawn_prune_task();
        manager
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BandMapEvent> {
        self.inner.events.subscribe()
    }

    pub fn set_max_age(&self, max_age: Duration) {
        *self
            .inner
            .max_age
            .lock()
            .expect("band map max age mutex poisoned") = max_age;
        self.prune_old_spots();
    }

    pub fn spots(&self, log_id: Option<i64>) -> Vec<BandMapSpot> {
        self.inner
            .store
            .lock()
            .expect("band map store mutex poisoned")
            .spots(log_id)
    }

    pub fn delete_spot(&self, id: u64) -> bool {
        let deleted = self
            .inner
            .store
            .lock()
            .expect("band map store mutex poisoned")
            .remove(id);
        if deleted {
            let _ = self.inner.events.send(BandMapEvent::SpotDeleted { id });
        }
        deleted
    }

    pub fn upsert_dxcluster_spot(&self, spot: DxClusterSpot) -> BandMapSpot {
        let key = BandMapDedupeKey::Cluster { source_id: spot.id };
        let candidate = BandMapSpotCandidate {
            received_at: spot.received_at,
            spot_type: if spot.rbn.is_some() {
                BandMapSpotType::Rbn
            } else {
                BandMapSpotType::Dx
            },
            source: spot.source,
            call_de: spot.call_de,
            call_dx: spot.call_dx,
            frequency_hz: spot.frequency_hz,
            utc: spot.utc,
            loc: spot.loc,
            comment: spot.comment,
            rbn: spot.rbn,
            band_name: self.band_name_for_frequency(spot.frequency_hz),
            radio_id: None,
            radio_name: None,
            log_id: None,
            exchange_fields: None,
        };
        self.upsert_candidate(key, candidate)
    }

    pub fn upsert_local_spot(&self, input: LocalSpotInput) -> BandMapSpot {
        let frequency_tenth_khz = frequency_tenth_khz(input.frequency_hz);
        let band_name = self.band_name_for_frequency(input.frequency_hz);
        let call_dx = input.call_dx.trim().to_uppercase();
        let key = match band_name.clone() {
            Some(band_name) => BandMapDedupeKey::Local {
                log_id: input.log_id,
                band_name,
                call_dx: call_dx.clone(),
            },
            None => BandMapDedupeKey::LocalUnknownBand {
                log_id: input.log_id,
                frequency_tenth_khz,
                call_dx: call_dx.clone(),
            },
        };
        let candidate = BandMapSpotCandidate {
            received_at: input.received_at.unwrap_or_else(unix_timestamp_secs),
            spot_type: BandMapSpotType::Local,
            source: "local".to_string(),
            call_de: "LOCAL".to_string(),
            call_dx,
            frequency_hz: input.frequency_hz,
            utc: current_utc_hhmm(),
            loc: None,
            comment: input.comment.filter(|comment| !comment.trim().is_empty()),
            rbn: None,
            band_name,
            radio_id: input.radio_id,
            radio_name: input.radio_name,
            log_id: input.log_id,
            exchange_fields: input.exchange_fields,
        };
        self.upsert_candidate(key, candidate)
    }

    pub fn upsert_cq(&self, input: CqSpotInput) -> Option<BandMapSpot> {
        let band_name = self.band_name_for_frequency(input.frequency_hz)?;
        let key = BandMapDedupeKey::Cq {
            radio_id: input.radio_id,
            band_name: band_name.clone(),
        };
        let candidate = BandMapSpotCandidate {
            received_at: unix_timestamp_secs(),
            spot_type: BandMapSpotType::Cq,
            source: "cq".to_string(),
            call_de: String::new(),
            call_dx: String::new(),
            frequency_hz: input.frequency_hz,
            utc: current_utc_hhmm(),
            loc: None,
            comment: None,
            rbn: None,
            band_name: Some(band_name),
            radio_id: Some(input.radio_id),
            radio_name: Some(input.radio_name),
            log_id: None,
            exchange_fields: None,
        };
        Some(self.upsert_candidate(key, candidate))
    }

    pub fn upsert_in_use(&self, input: InUseSpotInput) -> BandMapSpot {
        let key = BandMapDedupeKey::InUse {
            frequency_tenth_khz: frequency_tenth_khz(input.frequency_hz),
        };
        let candidate = BandMapSpotCandidate {
            received_at: unix_timestamp_secs(),
            spot_type: BandMapSpotType::InUse,
            source: "in_use".to_string(),
            call_de: String::new(),
            call_dx: String::new(),
            frequency_hz: input.frequency_hz,
            utc: current_utc_hhmm(),
            loc: None,
            comment: None,
            rbn: None,
            band_name: self.band_name_for_frequency(input.frequency_hz),
            radio_id: None,
            radio_name: None,
            log_id: None,
            exchange_fields: None,
        };
        self.upsert_candidate(key, candidate)
    }

    fn upsert_candidate(
        &self,
        key: BandMapDedupeKey,
        candidate: BandMapSpotCandidate,
    ) -> BandMapSpot {
        let max_age = self.max_age();
        let outcome = {
            let mut store = self
                .inner
                .store
                .lock()
                .expect("band map store mutex poisoned");
            let _ = store.prune(max_age);
            store.upsert(key, candidate)
        };

        match outcome {
            UpsertOutcome::Inserted(spot) | UpsertOutcome::Updated(spot) => {
                let _ = self
                    .inner
                    .events
                    .send(BandMapEvent::SpotUpserted(Box::new(spot.clone())));
                spot
            }
            UpsertOutcome::Refreshed(spot) => spot,
        }
    }

    fn sync_local_spots_for_log(&self, log_id: i64, contacts: &[Contact]) {
        let max_age = self.max_age();
        let cutoff = cutoff_timestamp(max_age);
        let mut desired = HashMap::<BandMapDedupeKey, BandMapSpotCandidate>::new();

        for contact in contacts {
            let Some(candidate) = local_spot_candidate(&self.inner.bands, log_id, contact) else {
                continue;
            };
            if candidate.received_at < cutoff {
                continue;
            }

            let key = match candidate.band_name.clone() {
                Some(band_name) => BandMapDedupeKey::Local {
                    log_id: Some(log_id),
                    band_name,
                    call_dx: candidate.call_dx.clone(),
                },
                None => BandMapDedupeKey::LocalUnknownBand {
                    log_id: Some(log_id),
                    frequency_tenth_khz: frequency_tenth_khz(candidate.frequency_hz),
                    call_dx: candidate.call_dx.clone(),
                },
            };

            match desired.get(&key) {
                Some(existing) if existing.received_at >= candidate.received_at => {}
                _ => {
                    desired.insert(key, candidate);
                }
            }
        }

        let (deleted_ids, upserted_spots) = {
            let mut store = self
                .inner
                .store
                .lock()
                .expect("band map store mutex poisoned");
            let mut deleted_ids = store.prune(max_age);
            let existing_keys = store.local_keys_for_log(log_id);
            let desired_keys = desired.keys().cloned().collect::<HashSet<_>>();
            let mut upserted_spots = Vec::new();

            for (key, candidate) in desired {
                match store.upsert(key, candidate) {
                    UpsertOutcome::Inserted(spot) | UpsertOutcome::Updated(spot) => {
                        upserted_spots.push(spot)
                    }
                    UpsertOutcome::Refreshed(_) => {}
                }
            }

            for key in existing_keys.difference(&desired_keys) {
                if let Some(id) = store.id_by_key.get(key).copied() {
                    store.remove_by_id(id, &mut deleted_ids);
                }
            }

            (deleted_ids, upserted_spots)
        };

        for id in deleted_ids {
            let _ = self.inner.events.send(BandMapEvent::SpotDeleted { id });
        }
        for spot in upserted_spots {
            let _ = self
                .inner
                .events
                .send(BandMapEvent::SpotUpserted(Box::new(spot)));
        }
    }

    fn prune_old_spots(&self) {
        let deleted_ids = self
            .inner
            .store
            .lock()
            .expect("band map store mutex poisoned")
            .prune(self.max_age());
        for id in deleted_ids {
            let _ = self.inner.events.send(BandMapEvent::SpotDeleted { id });
        }
    }

    fn max_age(&self) -> Duration {
        *self
            .inner
            .max_age
            .lock()
            .expect("band map max age mutex poisoned")
    }

    fn band_name_for_frequency(&self, frequency_hz: u64) -> Option<String> {
        band_for_frequency(&self.inner.bands, Frequency::from_hz(frequency_hz))
            .map(|band| band.name.clone())
    }

    fn spawn_prune_task(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }

        let manager = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(BANDMAP_PRUNE_INTERVAL);
            loop {
                interval.tick().await;
                manager.prune_old_spots();
            }
        });
    }
}

impl Default for BandMapManager {
    fn default() -> Self {
        Self::new(Arc::new(Vec::new()), Duration::from_secs(60 * 60))
    }
}

impl LogCacheProcessor for BandMapManager {
    fn on_log_loaded(
        &self,
        log_id: i64,
        _module: Arc<ContestScoringModule>,
        contacts: &mut [Contact],
    ) {
        self.sync_local_spots_for_log(log_id, contacts);
    }

    fn on_contacts_upserted(
        &self,
        log_id: i64,
        _module: Arc<ContestScoringModule>,
        contacts: &mut [Contact],
        _committed_contacts: &[Contact],
        _previous_contacts: &[Option<Contact>],
    ) -> Vec<Contact> {
        self.sync_local_spots_for_log(log_id, contacts);
        Vec::new()
    }

    fn on_contact_deleted(
        &self,
        log_id: i64,
        _module: Arc<ContestScoringModule>,
        contacts: &mut [Contact],
        _deleted_contact: &Contact,
    ) -> Vec<Contact> {
        self.sync_local_spots_for_log(log_id, contacts);
        Vec::new()
    }
}

impl BandMapSpotStore {
    fn upsert(&mut self, key: BandMapDedupeKey, candidate: BandMapSpotCandidate) -> UpsertOutcome {
        if let Some(id) = self.id_by_key.get(&key).copied()
            && let Some(spot) = self.spots_by_id.get_mut(&id)
        {
            let changed = spot_visible_fields_changed(spot, &candidate);
            spot.received_at = candidate.received_at;
            spot.spot_type = candidate.spot_type;
            spot.source = candidate.source;
            spot.call_de = candidate.call_de;
            spot.call_dx = candidate.call_dx;
            spot.frequency_hz = candidate.frequency_hz;
            spot.utc = candidate.utc;
            spot.loc = candidate.loc;
            spot.comment = candidate.comment;
            spot.rbn = candidate.rbn;
            spot.band_name = candidate.band_name;
            spot.radio_id = candidate.radio_id;
            spot.radio_name = candidate.radio_name;
            spot.log_id = candidate.log_id;
            spot.exchange_fields = candidate.exchange_fields;
            self.ids_by_time.retain(|current_id| *current_id != id);
            self.ids_by_time.push_back(id);
            return if changed {
                UpsertOutcome::Updated(spot.clone())
            } else {
                UpsertOutcome::Refreshed(spot.clone())
            };
        }

        self.next_id = self.next_id.saturating_add(1).max(1);
        let id = self.next_id;
        let spot = BandMapSpot {
            id,
            received_at: candidate.received_at,
            spot_type: candidate.spot_type,
            source: candidate.source,
            call_de: candidate.call_de,
            call_dx: candidate.call_dx,
            frequency_hz: candidate.frequency_hz,
            utc: candidate.utc,
            loc: candidate.loc,
            comment: candidate.comment,
            rbn: candidate.rbn,
            band_name: candidate.band_name,
            radio_id: candidate.radio_id,
            radio_name: candidate.radio_name,
            log_id: candidate.log_id,
            exchange_fields: candidate.exchange_fields,
        };
        self.ids_by_time.push_back(id);
        self.id_by_key.insert(key.clone(), id);
        self.key_by_id.insert(id, key);
        self.spots_by_id.insert(id, spot.clone());
        UpsertOutcome::Inserted(spot)
    }

    fn spots(&self, log_id: Option<i64>) -> Vec<BandMapSpot> {
        self.ids_by_time
            .iter()
            .filter_map(|id| self.spots_by_id.get(id))
            .filter(|spot| spot_visible_for_log(spot, log_id))
            .cloned()
            .collect()
    }

    fn prune(&mut self, max_age: Duration) -> Vec<u64> {
        let cutoff = cutoff_timestamp(max_age);
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

    fn remove(&mut self, id: u64) -> bool {
        let mut deleted_ids = Vec::new();
        self.ids_by_time.retain(|current_id| *current_id != id);
        self.remove_by_id(id, &mut deleted_ids);
        !deleted_ids.is_empty()
    }

    fn remove_by_id(&mut self, id: u64, deleted_ids: &mut Vec<u64>) {
        if self.spots_by_id.remove(&id).is_none() {
            return;
        }
        if let Some(key) = self.key_by_id.remove(&id) {
            self.id_by_key.remove(&key);
        }
        deleted_ids.push(id);
    }

    fn local_keys_for_log(&self, log_id: i64) -> HashSet<BandMapDedupeKey> {
        self.id_by_key
            .keys()
            .filter_map(|key| match key {
                BandMapDedupeKey::Local {
                    log_id: Some(current_log_id),
                    ..
                }
                | BandMapDedupeKey::LocalUnknownBand {
                    log_id: Some(current_log_id),
                    ..
                } if *current_log_id == log_id => Some(key.clone()),
                _ => None,
            })
            .collect()
    }
}

fn spot_visible_fields_changed(spot: &BandMapSpot, candidate: &BandMapSpotCandidate) -> bool {
    spot.spot_type != candidate.spot_type
        || spot.source != candidate.source
        || spot.call_de != candidate.call_de
        || spot.call_dx != candidate.call_dx
        || spot.frequency_hz != candidate.frequency_hz
        || spot.utc != candidate.utc
        || spot.loc != candidate.loc
        || spot.comment != candidate.comment
        || spot.rbn != candidate.rbn
        || spot.band_name != candidate.band_name
        || spot.radio_id != candidate.radio_id
        || spot.radio_name != candidate.radio_name
        || spot.log_id != candidate.log_id
        || spot.exchange_fields != candidate.exchange_fields
}

fn spot_visible_for_log(spot: &BandMapSpot, log_id: Option<i64>) -> bool {
    if !matches!(spot.spot_type, BandMapSpotType::Local) {
        return true;
    }
    match (spot.log_id, log_id) {
        (Some(spot_log_id), Some(requested_log_id)) => spot_log_id == requested_log_id,
        (Some(_), None) => false,
        _ => true,
    }
}

fn local_spot_candidate(
    bands: &[Band],
    log_id: i64,
    contact: &Contact,
) -> Option<BandMapSpotCandidate> {
    let adif = contact_adif(contact)?;
    let call_dx = json_string(adif.get("CALL")?)?.trim().to_uppercase();
    if call_dx.is_empty() {
        return None;
    }
    let frequency_hz = value_u64(adif.get("FREQ"))?;
    if frequency_hz == 0 {
        return None;
    }
    let received_at = value_u64(adif.get("QSO_DATE_TIME_ON"))?;
    let band_name = adif
        .get("BAND")
        .and_then(json_string)
        .filter(|band_name| !band_name.trim().is_empty())
        .or_else(|| {
            band_for_frequency(bands, Frequency::from_hz(frequency_hz))
                .map(|band| band.name.clone())
        });
    Some(BandMapSpotCandidate {
        received_at,
        spot_type: BandMapSpotType::Local,
        source: "local".to_string(),
        call_de: contact_adif_value(contact, "STATION_CALLSIGN")
            .and_then(json_string)
            .unwrap_or_default(),
        call_dx,
        frequency_hz,
        utc: utc_hhmm_from_timestamp(received_at),
        loc: None,
        comment: None,
        rbn: None,
        band_name,
        radio_id: None,
        radio_name: None,
        log_id: Some(log_id),
        exchange_fields: Some(adif.clone()),
    })
}

fn frequency_tenth_khz(frequency_hz: u64) -> u64 {
    (frequency_hz + 50) / 100
}

fn value_u64(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_i64().and_then(|value| u64::try_from(value).ok())),
        Value::String(string) => string.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn json_string(value: &Value) -> Option<String> {
    match value {
        Value::String(string) => Some(string.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn cutoff_timestamp(max_age: Duration) -> u64 {
    unix_timestamp_secs().saturating_sub(max_age.as_secs())
}

fn current_utc_hhmm() -> u16 {
    utc_hhmm_from_timestamp(unix_timestamp_secs())
}

fn utc_hhmm_from_timestamp(timestamp: u64) -> u16 {
    let seconds_since_midnight = timestamp % 86_400;
    let hours = seconds_since_midnight / 3_600;
    let minutes = (seconds_since_midnight % 3_600) / 60;
    (hours * 100 + minutes) as u16
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
    use crate::db::{build_contact, set_contact_adif};
    use serde_json::json;

    fn test_bands() -> Arc<Vec<Band>> {
        Arc::new(vec![Band {
            iaru_region: 2,
            name: "20m".to_string(),
            lower_hz: 14_000_000,
            upper_hz: 14_350_000,
            default_ssb_mode: "USB".to_string(),
            sort_order: 1,
        }])
    }

    fn contact(log_id: i64, call: &str, freq: u64, timestamp: u64, sect: &str) -> Contact {
        let mut contact = build_contact(Map::new(), Map::new());
        set_contact_adif(&mut contact, "LOG_ID", json!(log_id));
        set_contact_adif(&mut contact, "CALL", json!(call));
        set_contact_adif(&mut contact, "FREQ", json!(freq));
        set_contact_adif(&mut contact, "BAND", json!("20m"));
        set_contact_adif(&mut contact, "QSO_DATE_TIME_ON", json!(timestamp));
        set_contact_adif(&mut contact, "ARRL_SECT", json!(sect));
        contact
    }

    fn dx_spot(id: u64, call_dx: &str, frequency_hz: u64) -> DxClusterSpot {
        DxClusterSpot {
            id,
            received_at: unix_timestamp_secs(),
            source: "dx".to_string(),
            call_de: "N0CALL".to_string(),
            call_dx: call_dx.to_string(),
            frequency_hz,
            utc: 1234,
            loc: None,
            comment: Some("test".to_string()),
            rbn: None,
        }
    }

    #[test]
    fn upserts_cq_once_per_band_per_radio() {
        let manager = BandMapManager::new(test_bands(), Duration::from_secs(60));
        manager.upsert_cq(CqSpotInput {
            frequency_hz: 14_074_000,
            radio_id: 7,
            radio_name: "K4".to_string(),
        });
        manager.upsert_cq(CqSpotInput {
            frequency_hz: 14_075_000,
            radio_id: 7,
            radio_name: "K4".to_string(),
        });

        let spots = manager.spots(None);
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].spot_type, BandMapSpotType::Cq);
        assert_eq!(spots[0].frequency_hz, 14_075_000);
        assert_eq!(spots[0].radio_name.as_deref(), Some("K4"));
    }

    #[test]
    fn upserts_in_use_once_per_tenth_khz() {
        let manager = BandMapManager::new(test_bands(), Duration::from_secs(60));
        manager.upsert_in_use(InUseSpotInput {
            frequency_hz: 14_074_210,
        });
        manager.upsert_in_use(InUseSpotInput {
            frequency_hz: 14_074_249,
        });

        let spots = manager.spots(None);
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].spot_type, BandMapSpotType::InUse);
        assert_eq!(spots[0].frequency_hz, 14_074_249);
    }

    #[test]
    fn local_spots_include_exchange_fields() {
        let manager = BandMapManager::new(test_bands(), Duration::from_secs(60));
        let mut exchange_fields = Map::new();
        exchange_fields.insert("ARRL_SECT".to_string(), json!("SC"));
        let spot = manager.upsert_local_spot(LocalSpotInput {
            frequency_hz: 14_074_000,
            call_dx: "K1ABC".to_string(),
            comment: Some("worked".to_string()),
            radio_id: Some(1),
            radio_name: Some("K4".to_string()),
            log_id: Some(9),
            exchange_fields: Some(exchange_fields),
            received_at: Some(unix_timestamp_secs()),
        });

        assert_eq!(spot.spot_type, BandMapSpotType::Local);
        assert_eq!(spot.log_id, Some(9));
        assert_eq!(
            spot.exchange_fields
                .as_ref()
                .and_then(|fields| fields.get("ARRL_SECT")),
            Some(&json!("SC"))
        );
    }

    #[test]
    fn dxcluster_spots_are_imported_into_band_map() {
        let manager = BandMapManager::new(test_bands(), Duration::from_secs(60));
        let spot = manager.upsert_dxcluster_spot(dx_spot(22, "K1ABC", 14_074_000));

        assert_eq!(spot.spot_type, BandMapSpotType::Dx);
        assert_eq!(spot.band_name.as_deref(), Some("20m"));
        assert_eq!(manager.spots(None).len(), 1);
    }

    #[test]
    fn loading_and_updating_contacts_syncs_local_spots_by_log() {
        let manager = BandMapManager::new(test_bands(), Duration::from_secs(60 * 60));
        let now = unix_timestamp_secs();
        let mut contacts = vec![contact(9, "K1ABC", 14_074_000, now, "SC")];
        manager.on_log_loaded(9, Arc::new(ContestScoringModule::default()), &mut contacts);

        let initial_spots = manager.spots(Some(9));
        assert_eq!(initial_spots.len(), 1);
        assert_eq!(
            initial_spots[0]
                .exchange_fields
                .as_ref()
                .and_then(|fields| fields.get("ARRL_SECT")),
            Some(&json!("SC"))
        );

        let mut updated_contacts = vec![contact(9, "K1ABC", 14_074_500, now + 10, "NC")];
        let previous_contacts = vec![None];
        manager.on_contacts_upserted(
            9,
            Arc::new(ContestScoringModule::default()),
            &mut updated_contacts,
            &[],
            &previous_contacts,
        );

        let updated_spots = manager.spots(Some(9));
        assert_eq!(updated_spots.len(), 1);
        assert_eq!(updated_spots[0].frequency_hz, 14_074_500);
        assert_eq!(
            updated_spots[0]
                .exchange_fields
                .as_ref()
                .and_then(|fields| fields.get("ARRL_SECT")),
            Some(&json!("NC"))
        );
    }

    #[test]
    fn contact_delete_removes_local_spot() {
        let manager = BandMapManager::new(test_bands(), Duration::from_secs(60 * 60));
        let now = unix_timestamp_secs();
        let mut contacts = vec![contact(9, "K1ABC", 14_074_000, now, "SC")];
        manager.on_log_loaded(9, Arc::new(ContestScoringModule::default()), &mut contacts);
        let deleted_contact = contacts[0].clone();
        let mut remaining = Vec::<Contact>::new();
        manager.on_contact_deleted(
            9,
            Arc::new(ContestScoringModule::default()),
            &mut remaining,
            &deleted_contact,
        );

        assert!(manager.spots(Some(9)).is_empty());
    }

    #[test]
    fn filters_local_spots_by_requested_log() {
        let manager = BandMapManager::new(test_bands(), Duration::from_secs(60));
        let now = unix_timestamp_secs();
        manager.upsert_local_spot(LocalSpotInput {
            frequency_hz: 14_074_000,
            call_dx: "K1ABC".to_string(),
            comment: None,
            radio_id: None,
            radio_name: None,
            log_id: Some(9),
            exchange_fields: None,
            received_at: Some(now),
        });
        manager.upsert_local_spot(LocalSpotInput {
            frequency_hz: 14_075_000,
            call_dx: "N1XYZ".to_string(),
            comment: None,
            radio_id: None,
            radio_name: None,
            log_id: Some(10),
            exchange_fields: None,
            received_at: Some(now),
        });

        assert_eq!(manager.spots(Some(9)).len(), 1);
        assert_eq!(manager.spots(Some(10)).len(), 1);
        assert!(manager.spots(None).is_empty());
    }

    #[test]
    fn manual_delete_emits_event() {
        let manager = BandMapManager::new(test_bands(), Duration::from_secs(60));
        let spot = manager.upsert_local_spot(LocalSpotInput {
            frequency_hz: 14_074_000,
            call_dx: "K1ABC".to_string(),
            comment: None,
            radio_id: None,
            radio_name: None,
            log_id: Some(9),
            exchange_fields: None,
            received_at: Some(unix_timestamp_secs()),
        });
        let mut events = manager.subscribe();

        assert!(manager.delete_spot(spot.id));
        assert!(matches!(
            events.try_recv(),
            Ok(BandMapEvent::SpotDeleted { id }) if id == spot.id
        ));
    }

    #[test]
    fn expired_spots_are_not_loaded_from_contacts() {
        let manager = BandMapManager::new(test_bands(), Duration::from_secs(60));
        let old = unix_timestamp_secs().saturating_sub(600);
        let mut contacts = vec![contact(9, "K1ABC", 14_074_000, old, "SC")];
        manager.on_log_loaded(9, Arc::new(ContestScoringModule::default()), &mut contacts);

        assert!(manager.spots(Some(9)).is_empty());
    }
}
