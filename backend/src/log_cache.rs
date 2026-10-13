use crate::contest_rules::ContestRulesStore;
use crate::db::{Contact, Database, contact_adif_value, contact_id, set_contact_adif};
use crate::dxcc::{DxccDatabase, DxccInfo};
use crate::scoring::{ContestScoringModule, ScoringModules};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

pub trait LogCacheProcessor: Send + Sync {
    fn on_log_loaded(
        &self,
        log_id: i64,
        module: Arc<ContestScoringModule>,
        contacts: &mut [Contact],
    );

    fn on_contacts_upserted(
        &self,
        log_id: i64,
        module: Arc<ContestScoringModule>,
        contacts: &mut [Contact],
        committed_contacts: &[Contact],
        previous_contacts: &[Option<Contact>],
    ) -> Vec<Contact>;

    fn on_contact_deleted(
        &self,
        log_id: i64,
        module: Arc<ContestScoringModule>,
        contacts: &mut [Contact],
        deleted_contact: &Contact,
    ) -> Vec<Contact>;

    fn on_log_removed(&self, _log_id: i64) {}
}

#[derive(Debug, Default)]
pub struct LogCacheUpsertResult {
    pub contacts: Vec<Contact>,
    pub changed_contacts: Vec<Contact>,
}

#[derive(Debug)]
pub struct LogCacheDeleteResult {
    pub log_id: i64,
    pub changed_contacts: Vec<Contact>,
}

#[derive(Clone)]
pub struct LogCache {
    inner: Arc<LogCacheInner>,
}

struct LogCacheInner {
    db: Database,
    contest_rules: ContestRulesStore,
    scoring_modules: ScoringModules,
    dxcc: Arc<DxccDatabase>,
    logs: Mutex<HashMap<i64, CachedLog>>,
    processors: Mutex<Vec<Arc<dyn LogCacheProcessor>>>,
    coordination: Mutex<HashMap<i64, Weak<AsyncMutex<()>>>>,
}

struct CachedLog {
    revision: u64,
    module: Arc<ContestScoringModule>,
    contacts: Vec<Contact>,
    callsign_index: BTreeMap<String, Vec<i64>>,
    contact_positions: HashMap<i64, usize>,
}

impl LogCache {
    /// Fill missing DXCC-derived ADIF fields on in-memory contacts without
    /// persisting. Used to diagnose warnings on the same enriched values
    /// scoring sees; the later upsert enrich is idempotent fill-missing.
    pub(crate) fn enrich_contacts(&self, contacts: &mut [Contact]) {
        enrich_missing_dxcc(&self.inner.dxcc, contacts);
    }

    pub fn new(
        db: Database,
        contest_rules: ContestRulesStore,
        scoring_modules: ScoringModules,
        dxcc: Arc<DxccDatabase>,
    ) -> Self {
        Self {
            inner: Arc::new(LogCacheInner {
                db,
                contest_rules,
                scoring_modules,
                dxcc,
                logs: Mutex::new(HashMap::new()),
                processors: Mutex::new(Vec::new()),
                coordination: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn register_processor(&self, processor: Arc<dyn LogCacheProcessor>) {
        let mut processors = self
            .inner
            .processors
            .lock()
            .expect("log cache processors mutex poisoned");
        processors.push(processor);
    }

    pub async fn ensure_loaded(&self, log_id: i64) -> Result<(), String> {
        let _guard = self.coordinate(log_id).await;
        self.ensure_loaded_locked(log_id).await
    }

    pub async fn refresh_loaded(&self) {
        let ids = self
            .inner
            .logs
            .lock()
            .expect("log cache mutex poisoned")
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for log_id in ids {
            if let Err(error) = self.ensure_loaded(log_id).await {
                tracing::debug!(log_id, %error, "failed to refresh derived log cache");
            }
        }
    }

    async fn coordinate(&self, log_id: i64) -> OwnedMutexGuard<()> {
        let gate = {
            let mut gates = self
                .inner
                .coordination
                .lock()
                .expect("log cache coordination poisoned");
            gates.retain(|_, gate| gate.strong_count() != 0);
            if let Some(gate) = gates.get(&log_id).and_then(Weak::upgrade) {
                gate
            } else {
                let gate = Arc::new(AsyncMutex::new(()));
                gates.insert(log_id, Arc::downgrade(&gate));
                gate
            }
        };
        gate.lock_owned().await
    }

    async fn ensure_loaded_locked(&self, log_id: i64) -> Result<(), String> {
        if self.is_loaded(log_id) {
            return Ok(());
        }
        loop {
            let Some(snapshot) = self
                .inner
                .db
                .log_snapshot(log_id)
                .await
                .map_err(|error| error.to_string())?
            else {
                self.clear_log(log_id);
                return Err(format!("log {log_id} not found"));
            };
            let (module, mut contacts) = self.load_log(&snapshot)?;
            let processors = self.processors();
            let mut logs = self.inner.logs.lock().expect("log cache mutex poisoned");
            if self.inner.db.log_revision(log_id) != Some(snapshot.revision) {
                continue;
            }
            for processor in &processors {
                processor.on_log_loaded(log_id, Arc::clone(&module), &mut contacts);
            }
            if self.inner.db.log_revision(log_id) != Some(snapshot.revision) {
                logs.remove(&log_id);
                for processor in &processors {
                    processor.on_log_removed(log_id);
                }
                continue;
            }
            sort_contacts_desc(&mut contacts);
            let mut cached = CachedLog::new(module, contacts);
            cached.revision = snapshot.revision;
            logs.insert(log_id, cached);
            return Ok(());
        }
    }

    pub async fn contacts_display_page(
        &self,
        log_id: i64,
        offset: usize,
        limit: usize,
        callsign_prefix: Option<String>,
    ) -> Result<Vec<Contact>, String> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let _guard = self.coordinate(log_id).await;
        self.ensure_loaded_locked(log_id).await?;
        let logs = self.inner.logs.lock().expect("log cache mutex poisoned");
        let Some(cached_log) = logs.get(&log_id) else {
            return Ok(Vec::new());
        };

        let callsign_prefix = normalized_callsign_prefix(callsign_prefix.as_deref());
        if let Some(callsign_prefix) = callsign_prefix.as_deref() {
            let matching_ids = cached_log.matching_contact_ids_for_prefix(callsign_prefix);
            if matching_ids.is_empty() {
                return Ok(Vec::new());
            }

            let mut matching_positions = matching_ids
                .into_iter()
                .filter_map(|contact_id| cached_log.contact_positions.get(&contact_id).copied())
                .collect::<Vec<_>>();
            matching_positions.sort_unstable();

            return Ok(matching_positions
                .into_iter()
                .skip(offset)
                .take(limit)
                .filter_map(|index| cached_log.contacts.get(index).cloned())
                .collect());
        }

        if offset >= cached_log.contacts.len() {
            return Ok(Vec::new());
        }

        let end = offset.saturating_add(limit).min(cached_log.contacts.len());
        Ok(cached_log.contacts[offset..end].to_vec())
    }

    pub async fn upsert_contacts(
        &self,
        log_id: i64,
        mut contacts: Vec<Contact>,
    ) -> Result<LogCacheUpsertResult, String> {
        let _guard = self.coordinate(log_id).await;
        self.ensure_loaded_locked(log_id).await?;
        enrich_missing_dxcc(&self.inner.dxcc, &mut contacts);

        let mutation = self
            .inner
            .db
            .upsert_contacts_versioned(log_id, contacts)
            .await
            .map_err(|error| error.to_string())?;

        let committed_contacts = mutation.contacts;
        let current_revision = self
            .inner
            .logs
            .lock()
            .expect("log cache mutex poisoned")
            .get(&log_id)
            .map(|log| log.revision);
        if current_revision != Some(mutation.previous_revision)
            || self.inner.db.log_revision(log_id) != Some(mutation.revision)
        {
            self.ensure_loaded_locked(log_id).await?;
            let logs = self.inner.logs.lock().expect("log cache mutex poisoned");
            let cached = logs
                .get(&log_id)
                .ok_or_else(|| format!("log {log_id} is not loaded"))?;
            let contacts = committed_contacts
                .iter()
                .filter_map(|contact| contact_id(contact).and_then(|id| cached.contact_by_id(id)))
                .collect();
            let ids = committed_contacts.iter().filter_map(contact_id).collect();
            return Ok(LogCacheUpsertResult {
                contacts,
                changed_contacts: dedupe_contacts(cached.contacts.clone(), &ids),
            });
        }
        // Duplicate IDs within a batch represent one final change to derived state.
        let mut seen = HashSet::new();
        let mut final_contacts = committed_contacts
            .iter()
            .rev()
            .filter(|contact| contact_id(contact).is_some_and(|id| seen.insert(id)))
            .cloned()
            .collect::<Vec<_>>();
        final_contacts.reverse();
        let processors = self.processors();
        let mut logs = self.inner.logs.lock().expect("log cache mutex poisoned");
        let Some(cached_log) = logs.get_mut(&log_id) else {
            return Err(format!("log {log_id} is not loaded"));
        };

        let previous_contacts = final_contacts
            .iter()
            .map(|committed_contact| {
                contact_id(committed_contact)
                    .and_then(|contact_id| cached_log.contact_by_id(contact_id))
            })
            .collect::<Vec<_>>();

        let indexes_dirty = cached_log.apply_upserted_contacts(&final_contacts, &previous_contacts);

        let committed_contact_ids = committed_contacts
            .iter()
            .filter_map(contact_id)
            .collect::<HashSet<_>>();

        let mut changed_contacts = Vec::new();
        for processor in &processors {
            changed_contacts.extend(processor.on_contacts_upserted(
                log_id,
                Arc::clone(&cached_log.module),
                &mut cached_log.contacts,
                &final_contacts,
                &previous_contacts,
            ));
        }

        if indexes_dirty {
            cached_log.rebuild_indexes();
        }
        cached_log.revision = mutation.revision;
        let changed_contacts = dedupe_contacts(changed_contacts, &committed_contact_ids);

        let contacts = committed_contacts
            .iter()
            .filter_map(|committed_contact| {
                let contact_id = contact_id(committed_contact)?;
                cached_log.contact_by_id(contact_id)
            })
            .collect::<Vec<_>>();

        Ok(LogCacheUpsertResult {
            contacts,
            changed_contacts,
        })
    }

    pub async fn delete_contact(&self, id: i64) -> Result<Option<LogCacheDeleteResult>, String> {
        let Some(log_id) = self
            .inner
            .db
            .contact_log_id(id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        let _guard = self.coordinate(log_id).await;
        self.ensure_loaded_locked(log_id).await?;
        let Some(deletion) = self
            .inner
            .db
            .delete_contact_versioned(id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        if deletion.log_id != log_id {
            // An ID can be reused after deletion; the other log's revision will
            // force its derived cache to refresh on its next read.
            return Ok(Some(LogCacheDeleteResult {
                log_id: deletion.log_id,
                changed_contacts: Vec::new(),
            }));
        }
        let current_revision = self
            .inner
            .logs
            .lock()
            .expect("log cache mutex poisoned")
            .get(&log_id)
            .map(|log| log.revision);
        if current_revision != Some(deletion.previous_revision)
            || self.inner.db.log_revision(log_id) != Some(deletion.revision)
        {
            self.ensure_loaded_locked(log_id).await?;
            let logs = self.inner.logs.lock().expect("log cache mutex poisoned");
            let changed_contacts = logs
                .get(&log_id)
                .map(|log| log.contacts.clone())
                .unwrap_or_default();
            return Ok(Some(LogCacheDeleteResult {
                log_id,
                changed_contacts,
            }));
        }
        let processors = self.processors();
        let mut logs = self.inner.logs.lock().expect("log cache mutex poisoned");
        let Some(cached_log) = logs.get_mut(&log_id) else {
            return Ok(Some(LogCacheDeleteResult {
                log_id,
                changed_contacts: Vec::new(),
            }));
        };

        let Some(index) = cached_log
            .contacts
            .iter()
            .position(|contact| contact_id(contact) == Some(id))
        else {
            return Ok(Some(LogCacheDeleteResult {
                log_id,
                changed_contacts: Vec::new(),
            }));
        };

        let deleted_contact = cached_log.contacts.remove(index);
        let mut changed_contacts = Vec::new();
        for processor in &processors {
            changed_contacts.extend(processor.on_contact_deleted(
                log_id,
                Arc::clone(&cached_log.module),
                &mut cached_log.contacts,
                &deleted_contact,
            ));
        }

        cached_log.rebuild_indexes();
        cached_log.revision = deletion.revision;

        Ok(Some(LogCacheDeleteResult {
            log_id,
            changed_contacts: dedupe_contacts(changed_contacts, &HashSet::new()),
        }))
    }

    pub async fn remove_log(&self, log_id: i64) {
        let _guard = self.coordinate(log_id).await;
        self.clear_log(log_id);
    }

    fn clear_log(&self, log_id: i64) {
        {
            let mut logs = self.inner.logs.lock().expect("log cache mutex poisoned");
            logs.remove(&log_id);
        }

        for processor in self.processors() {
            processor.on_log_removed(log_id);
        }
    }

    fn processors(&self) -> Vec<Arc<dyn LogCacheProcessor>> {
        self.inner
            .processors
            .lock()
            .expect("log cache processors mutex poisoned")
            .clone()
    }

    fn is_loaded(&self, log_id: i64) -> bool {
        self.inner
            .logs
            .lock()
            .expect("log cache mutex poisoned")
            .get(&log_id)
            .is_some_and(|log| self.inner.db.log_revision(log_id) == Some(log.revision))
    }

    fn load_log(
        &self,
        snapshot: &crate::db::LogSnapshot,
    ) -> Result<(Arc<ContestScoringModule>, Vec<Contact>), String> {
        let log = &snapshot.log;

        let rules = self
            .inner
            .contest_rules
            .get(&log.contest_id)
            .ok_or_else(|| format!("unknown contest: {}", log.contest_id))?;

        let module = self
            .inner
            .scoring_modules
            .get(rules, log.contest_params.clone());

        let mut contacts = snapshot.contacts.to_vec();
        enrich_missing_dxcc(&self.inner.dxcc, &mut contacts);
        sort_contacts_desc(&mut contacts);

        Ok((module, contacts))
    }
}

fn enrich_missing_dxcc(database: &DxccDatabase, contacts: &mut [Contact]) {
    let mut station_info_by_callsign = HashMap::<String, Option<DxccInfo>>::new();
    for contact in contacts {
        let station_callsign = contact_adif_value(contact, "STATION_CALLSIGN")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|callsign| !callsign.is_empty())
            .map(str::to_uppercase);
        let station_info = station_callsign.as_deref().and_then(|callsign| {
            station_info_by_callsign
                .entry(callsign.to_string())
                .or_insert_with(|| database.lookup(callsign))
                .clone()
        });
        match station_info {
            Some(info) => {
                set_contact_adif(
                    contact,
                    "MY_DXCC",
                    serde_json::Value::Number(info.adif.into()),
                );
                set_contact_adif(
                    contact,
                    "MY_CONT",
                    serde_json::Value::String(info.continent),
                );
                set_contact_adif(
                    contact,
                    "APP_LOG73_MY_DXCC_PFX",
                    serde_json::Value::String(info.primary_prefix),
                );
            }
            None => {
                // Keep values supplied by the client when the server's CTY
                // database cannot resolve the station callsign.
            }
        }

        let callsign = contact_adif_value(contact, "CALL")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|callsign| !callsign.is_empty());
        let Some(callsign) = callsign else {
            continue;
        };

        let dxcc = contact_adif_value(contact, "DXCC");
        let dxcc_missing = dxcc.is_none_or(serde_json::Value::is_null);
        let prefix_missing = contact_adif_value(contact, "APP_LOG73_DXCC_PFX")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|prefix| prefix.trim().is_empty());
        let continent_missing = contact_adif_value(contact, "CONT")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|continent| continent.trim().is_empty());
        if !dxcc_missing && !prefix_missing && !continent_missing {
            continue;
        }

        let Some(info) = database.lookup(callsign) else {
            continue;
        };
        if dxcc_missing {
            set_contact_adif(contact, "DXCC", serde_json::Value::Number(info.adif.into()));
        }
        if prefix_missing && !info.primary_prefix.trim().is_empty() {
            set_contact_adif(
                contact,
                "APP_LOG73_DXCC_PFX",
                serde_json::Value::String(info.primary_prefix),
            );
        }
        if continent_missing && !info.continent.trim().is_empty() {
            set_contact_adif(contact, "CONT", serde_json::Value::String(info.continent));
        }
    }
}

impl CachedLog {
    fn new(module: Arc<ContestScoringModule>, contacts: Vec<Contact>) -> Self {
        let mut cached_log = Self {
            revision: 0,
            module,
            contacts,
            callsign_index: BTreeMap::new(),
            contact_positions: HashMap::new(),
        };
        cached_log.rebuild_indexes();
        cached_log
    }

    fn rebuild_indexes(&mut self) {
        self.callsign_index.clear();
        self.contact_positions.clear();

        for (index, contact) in self.contacts.iter().enumerate() {
            let Some(contact_id) = contact_id(contact) else {
                continue;
            };
            self.contact_positions.insert(contact_id, index);

            if let Some(callsign) = contact_callsign(contact) {
                self.callsign_index
                    .entry(callsign)
                    .or_default()
                    .push(contact_id);
            }
        }
    }

    fn apply_upserted_contacts(
        &mut self,
        committed_contacts: &[Contact],
        previous_contacts: &[Option<Contact>],
    ) -> bool {
        // Current processors operate on the contact slice and do not need the
        // cache indexes while they run. Inserts keep indexes current
        // incrementally; updates take the slower path and rebuild once after
        // processors finish.
        if previous_contacts.iter().any(Option::is_some) {
            for committed_contact in committed_contacts {
                merge_contact(&mut self.contacts, committed_contact);
            }
            sort_contacts_desc(&mut self.contacts);
            return true;
        }

        let mut indexes_dirty = false;
        for committed_contact in committed_contacts {
            if indexes_dirty || self.has_contact_id(committed_contact) {
                merge_contact(&mut self.contacts, committed_contact);
                indexes_dirty = true;
            } else {
                self.insert_new_contact_sorted(committed_contact);
            }
        }

        if indexes_dirty {
            sort_contacts_desc(&mut self.contacts);
        }
        indexes_dirty
    }

    fn has_contact_id(&self, contact: &Contact) -> bool {
        contact_id(contact)
            .is_some_and(|contact_id| self.contact_positions.contains_key(&contact_id))
    }

    fn insert_new_contact_sorted(&mut self, contact: &Contact) {
        let index = self.sorted_insert_position(contact);
        if index == self.contacts.len() {
            self.contacts.push(contact.clone());
        } else {
            self.contacts.insert(index, contact.clone());
        }

        self.refresh_contact_positions_from(index);
        self.add_contact_to_callsign_index(contact);
    }

    fn sorted_insert_position(&self, contact: &Contact) -> usize {
        let contact_key = contact_order_key(contact);
        let Some(first_contact) = self.contacts.first() else {
            return 0;
        };

        if contact_key > contact_order_key(first_contact) {
            return 0;
        }

        if let Some(last_contact) = self.contacts.last()
            && contact_order_key(last_contact) >= contact_key
        {
            return self.contacts.len();
        }

        self.contacts
            .partition_point(|existing| contact_order_key(existing) >= contact_key)
    }

    fn refresh_contact_positions_from(&mut self, start: usize) {
        for (index, contact) in self.contacts.iter().enumerate().skip(start) {
            if let Some(contact_id) = contact_id(contact) {
                self.contact_positions.insert(contact_id, index);
            }
        }
    }

    fn add_contact_to_callsign_index(&mut self, contact: &Contact) {
        let Some(contact_id) = contact_id(contact) else {
            return;
        };
        let Some(callsign) = contact_callsign(contact) else {
            return;
        };

        self.callsign_index
            .entry(callsign)
            .or_default()
            .push(contact_id);
    }

    fn contact_by_id(&self, contact_id: i64) -> Option<Contact> {
        let index = *self.contact_positions.get(&contact_id)?;
        self.contacts.get(index).cloned()
    }

    fn matching_contact_ids_for_prefix(&self, callsign_prefix: &str) -> HashSet<i64> {
        let mut matching_ids = HashSet::new();
        for (callsign, contact_ids) in self.callsign_index.range(callsign_prefix.to_string()..) {
            if !callsign.starts_with(callsign_prefix) {
                break;
            }
            matching_ids.extend(contact_ids.iter().copied());
        }
        matching_ids
    }
}

fn merge_contact(contacts: &mut Vec<Contact>, contact: &Contact) {
    if let Some(id) = contact_id(contact)
        && let Some(index) = contacts
            .iter()
            .position(|existing| contact_id(existing) == Some(id))
    {
        contacts[index] = contact.clone();
        return;
    }

    contacts.push(contact.clone());
}

fn dedupe_contacts(contacts: Vec<Contact>, excluded_ids: &HashSet<i64>) -> Vec<Contact> {
    let mut seen_ids = HashSet::new();
    let mut deduped = Vec::new();

    for contact in contacts {
        let Some(id) = contact_id(&contact) else {
            continue;
        };
        if excluded_ids.contains(&id) || !seen_ids.insert(id) {
            continue;
        }
        deduped.push(contact);
    }

    deduped
}

fn normalized_callsign_prefix(callsign_prefix: Option<&str>) -> Option<String> {
    let callsign_prefix = callsign_prefix.unwrap_or_default().trim().to_uppercase();
    (!callsign_prefix.is_empty()).then_some(callsign_prefix)
}

fn contact_callsign(contact: &Contact) -> Option<String> {
    let callsign = contact_adif_value(contact, "CALL")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_uppercase();
    (!callsign.is_empty()).then_some(callsign)
}

fn sort_contacts_desc(contacts: &mut [Contact]) {
    contacts.sort_by_key(|contact| Reverse(contact_order_key(contact)));
}

fn contact_order_key(contact: &Contact) -> (i64, i64) {
    (
        contact_adif_value(contact, "QSO_DATE_TIME_ON")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0),
        contact_id(contact).unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest_rules::ContestRules;
    use crate::db::build_contact;
    use serde_json::{Map, Value, json};

    fn cached_log(contacts: Vec<Contact>) -> CachedLog {
        CachedLog::new(test_module(), contacts)
    }

    async fn fixture() -> (crate::AppState, i64) {
        let state = crate::tests::cache_test_state();
        let log = state
            .db
            .create_log(crate::db::NewLog {
                name: "Derived cache".into(),
                contest_id: "CQ-WW-CW".into(),
                station_callsign: "N0CALL".into(),
                contest_params: json!({ "CQ_ZONE": "4" }),
            })
            .await
            .unwrap();
        (state, log.id)
    }

    #[tokio::test]
    async fn concurrent_mutations_and_duplicate_batch_ids_keep_derived_state_consistent() {
        let (state, log_id) = fixture().await;
        let mut tasks = Vec::new();
        for index in 0..16 {
            let cache = state.log_cache.clone();
            tasks.push(tokio::spawn(async move {
                cache
                    .upsert_contacts(
                        log_id,
                        vec![crate::tests::cache_test_contact(&format!("K{index}ABC"))],
                    )
                    .await
                    .unwrap()
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(state.db.log_qso_count(log_id).await.unwrap(), 16);
        assert_eq!(
            state.incremental_scoring.totals(log_id).unwrap().qso_count,
            16
        );
        let mut saved = state.db.contacts(log_id).await.unwrap();
        let first = saved[0].clone();
        set_contact_adif(&mut saved[0], "CALL", json!("W9XYZ"));
        state
            .log_cache
            .upsert_contacts(log_id, vec![first, saved[0].clone()])
            .await
            .unwrap();
        assert_eq!(
            state.incremental_scoring.totals(log_id).unwrap().qso_count,
            16
        );
        let current = state
            .log_cache
            .contacts_display_page(log_id, 0, 100, None)
            .await
            .unwrap();
        assert_eq!(current.len(), 16);
        assert!(
            current
                .iter()
                .any(|contact| contact_adif_value(contact, "CALL") == Some(&json!("W9XYZ")))
        );
    }

    #[tokio::test]
    async fn raw_mutation_metadata_change_and_deletion_refresh_derived_cache_without_sql() {
        let (state, log_id) = fixture().await;
        state
            .log_cache
            .upsert_contacts(log_id, vec![crate::tests::cache_test_contact("W1AW")])
            .await
            .unwrap();
        let before = state.db.test_counters();
        state
            .db
            .upsert_contacts(log_id, vec![crate::tests::cache_test_contact("K1ABC")])
            .await
            .unwrap();
        state.log_cache.ensure_loaded(log_id).await.unwrap();
        assert_eq!(
            state.incremental_scoring.totals(log_id).unwrap().qso_count,
            2
        );
        state
            .db
            .update_log(
                log_id,
                crate::db::UpdateLog {
                    name: "Changed".into(),
                    station_callsign: "N0CALL".into(),
                    contest_params: json!({ "CQ_ZONE": "5" }),
                },
            )
            .await
            .unwrap();
        state.log_cache.ensure_loaded(log_id).await.unwrap();
        assert_eq!(
            state.incremental_scoring.totals(log_id).unwrap().qso_count,
            2
        );
        let cached = state
            .log_cache
            .contacts_display_page(log_id, 0, 100, None)
            .await
            .unwrap();
        state
            .db
            .delete_contact(contact_id(&cached[0]).unwrap())
            .await
            .unwrap();
        state.log_cache.ensure_loaded(log_id).await.unwrap();
        assert_eq!(
            state.incremental_scoring.totals(log_id).unwrap().qso_count,
            1
        );
        state.db.delete_log(log_id).await.unwrap();
        assert!(state.log_cache.ensure_loaded(log_id).await.is_err());
        assert!(state.incremental_scoring.totals(log_id).is_none());
        assert!(state.stats.snapshot(log_id).is_none());
        assert_eq!(state.db.test_counters().2, before.2);
        assert_eq!(state.db.test_counters().1, before.1);
    }

    #[tokio::test]
    async fn failed_write_does_not_modify_derived_state() {
        let (state, log_id) = fixture().await;
        state
            .log_cache
            .upsert_contacts(log_id, vec![crate::tests::cache_test_contact("W1AW")])
            .await
            .unwrap();
        let before = state
            .log_cache
            .contacts_display_page(log_id, 0, 100, None)
            .await
            .unwrap();
        let mut invalid = crate::tests::cache_test_contact("K1ABC");
        invalid
            .get_mut("adif")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("CALL");
        assert!(
            state
                .log_cache
                .upsert_contacts(
                    log_id,
                    vec![crate::tests::cache_test_contact("W9XYZ"), invalid]
                )
                .await
                .is_err()
        );
        assert_eq!(
            state
                .log_cache
                .contacts_display_page(log_id, 0, 100, None)
                .await
                .unwrap(),
            before
        );
        assert_eq!(
            state.incremental_scoring.totals(log_id).unwrap().qso_count,
            1
        );
    }

    struct PausingProcessor {
        started: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl LogCacheProcessor for PausingProcessor {
        fn on_log_loaded(&self, _: i64, _: Arc<ContestScoringModule>, _: &mut [Contact]) {
            if let Some(started) = self.started.lock().unwrap().take() {
                let _ = started.send(());
                let _ = self.release.lock().unwrap().recv();
            }
        }
        fn on_contacts_upserted(
            &self,
            _: i64,
            _: Arc<ContestScoringModule>,
            _: &mut [Contact],
            _: &[Contact],
            _: &[Option<Contact>],
        ) -> Vec<Contact> {
            Vec::new()
        }
        fn on_contact_deleted(
            &self,
            _: i64,
            _: Arc<ContestScoringModule>,
            _: &mut [Contact],
            _: &Contact,
        ) -> Vec<Contact> {
            Vec::new()
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn loads_racing_metadata_updates_or_deletion_cannot_publish_stale_state() {
        for delete in [false, true] {
            let (state, log_id) = fixture().await;
            state
                .db
                .upsert_contacts(log_id, vec![crate::tests::cache_test_contact("W1AW")])
                .await
                .unwrap();
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            state
                .log_cache
                .register_processor(Arc::new(PausingProcessor {
                    started: Mutex::new(Some(started_tx)),
                    release: Mutex::new(release_rx),
                }));
            let cache = state.log_cache.clone();
            let load = tokio::spawn(async move { cache.ensure_loaded(log_id).await });
            started_rx.await.unwrap();
            if delete {
                state.db.delete_log(log_id).await.unwrap();
            } else {
                state
                    .db
                    .update_log(
                        log_id,
                        crate::db::UpdateLog {
                            name: "New settings".into(),
                            station_callsign: "N0CALL".into(),
                            contest_params: json!({ "CQ_ZONE": "6" }),
                        },
                    )
                    .await
                    .unwrap();
            }
            release_tx.send(()).unwrap();
            let loaded = load.await.unwrap();
            if delete {
                assert!(loaded.is_err());
                assert!(
                    !state
                        .log_cache
                        .inner
                        .logs
                        .lock()
                        .unwrap()
                        .contains_key(&log_id)
                );
                assert!(state.incremental_scoring.totals(log_id).is_none());
                assert!(state.stats.snapshot(log_id).is_none());
            } else {
                loaded.unwrap();
                assert_eq!(
                    state
                        .log_cache
                        .inner
                        .logs
                        .lock()
                        .unwrap()
                        .get(&log_id)
                        .unwrap()
                        .revision,
                    state.db.log_revision(log_id).unwrap()
                );
                assert_eq!(
                    state.incremental_scoring.totals(log_id).unwrap().qso_count,
                    1
                );
            }
            assert_eq!(state.db.test_counters().2, 0);
        }
    }

    #[tokio::test]
    async fn cancelled_derived_write_recovers_from_worker_publication() {
        let (state, log_id) = fixture().await;
        state.log_cache.ensure_loaded(log_id).await.unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let database = state.db.clone();
        let pause = tokio::spawn(async move {
            database
                .test_control(move |_| {
                    let _ = started_tx.send(());
                    let _ = release_rx.recv();
                    Ok(())
                })
                .await
                .unwrap();
        });
        started_rx.await.unwrap();
        let cache = state.log_cache.clone();
        let writer = tokio::spawn(async move {
            cache
                .upsert_contacts(log_id, vec![crate::tests::cache_test_contact("W1AW")])
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while state.db.test_pending_commands() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        writer.abort();
        assert!(writer.await.unwrap_err().is_cancelled());
        release_tx.send(()).unwrap();
        pause.await.unwrap();
        state.db.test_control(|_| Ok(())).await.unwrap();
        state.log_cache.ensure_loaded(log_id).await.unwrap();
        assert_eq!(
            state.incremental_scoring.totals(log_id).unwrap().qso_count,
            1
        );
        assert_eq!(
            state
                .log_cache
                .contacts_display_page(log_id, 0, 100, None)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(state.db.test_counters().2, 0);
    }

    fn test_module() -> Arc<ContestScoringModule> {
        let rules = ContestRules {
            id: "test".to_string(),
            name: "Test".to_string(),
            ..ContestRules::default()
        };
        ScoringModules::new().get(&rules, Value::Null)
    }

    fn contact(id: i64, epoch: i64, callsign: &str) -> Contact {
        build_contact(
            Map::from_iter([("id".to_string(), json!(id))]),
            Map::from_iter([
                ("QSO_DATE_TIME_ON".to_string(), json!(epoch)),
                ("CALL".to_string(), json!(callsign)),
            ]),
        )
    }

    #[test]
    fn dxcc_enrichment_stamps_station_geography_and_missing_worked_continent() {
        let database = DxccDatabase::from_str(
            "K,United States,291,NA,5,8,37.0,95.0,5.0,N W;\n\
             F,France,227,EU,14,27,46.0,-2.0,-1.0,F;\n",
        )
        .expect("test CTY data should parse");
        let mut contacts = vec![build_contact(
            Map::new(),
            Map::from_iter([
                ("STATION_CALLSIGN".to_string(), json!("N0CALL")),
                ("CALL".to_string(), json!("F1ABC")),
            ]),
        )];

        enrich_missing_dxcc(&database, &mut contacts);

        assert_eq!(
            contact_adif_value(&contacts[0], "MY_DXCC"),
            Some(&json!(291))
        );
        assert_eq!(
            contact_adif_value(&contacts[0], "MY_CONT"),
            Some(&json!("NA"))
        );
        assert_eq!(contact_adif_value(&contacts[0], "CONT"), Some(&json!("EU")));
        assert_eq!(contact_adif_value(&contacts[0], "DXCC"), Some(&json!(227)));
        assert_eq!(
            contact_adif_value(&contacts[0], "APP_LOG73_MY_DXCC_PFX"),
            Some(&json!("K"))
        );
        assert_eq!(
            contact_adif_value(&contacts[0], "APP_LOG73_DXCC_PFX"),
            Some(&json!("F"))
        );
    }

    fn contact_ids(cached_log: &CachedLog) -> Vec<i64> {
        cached_log.contacts.iter().filter_map(contact_id).collect()
    }

    fn assert_indexes_are_consistent(cached_log: &CachedLog) {
        let mut expected_callsigns = BTreeMap::<String, Vec<i64>>::new();
        for (index, contact) in cached_log.contacts.iter().enumerate() {
            let Some(contact_id) = contact_id(contact) else {
                continue;
            };
            assert_eq!(
                cached_log.contact_positions.get(&contact_id),
                Some(&index),
                "contact_positions should point to the contact vector index"
            );

            if let Some(callsign) = contact_callsign(contact) {
                expected_callsigns
                    .entry(callsign)
                    .or_default()
                    .push(contact_id);
            }
        }

        for (indexed_contact_id, index) in &cached_log.contact_positions {
            assert_eq!(
                cached_log.contacts.get(*index).and_then(contact_id),
                Some(*indexed_contact_id),
                "contact_positions should not contain stale entries"
            );
        }

        let mut actual_callsigns = cached_log.callsign_index.clone();
        for contact_ids in actual_callsigns.values_mut() {
            contact_ids.sort_unstable();
        }
        for contact_ids in expected_callsigns.values_mut() {
            contact_ids.sort_unstable();
        }
        assert_eq!(actual_callsigns, expected_callsigns);
    }

    #[test]
    fn insert_upsert_appends_when_sorted_order_allows_it() {
        let mut cached_log = cached_log(vec![contact(3, 300, "K3AAA"), contact(2, 200, "K2AAA")]);

        let indexes_dirty =
            cached_log.apply_upserted_contacts(&[contact(1, 100, "K1AAA")], &[None]);

        assert!(!indexes_dirty);
        assert_eq!(contact_ids(&cached_log), vec![3, 2, 1]);
        assert_indexes_are_consistent(&cached_log);
    }

    #[test]
    fn insert_upsert_binary_inserts_near_end_and_refreshes_shifted_positions() {
        let mut cached_log = cached_log(vec![
            contact(4, 400, "K4AAA"),
            contact(2, 200, "K2AAA"),
            contact(1, 100, "K1AAA"),
        ]);

        let indexes_dirty =
            cached_log.apply_upserted_contacts(&[contact(3, 150, "K3AAA")], &[None]);

        assert!(!indexes_dirty);
        assert_eq!(contact_ids(&cached_log), vec![4, 2, 3, 1]);
        assert_eq!(cached_log.contact_positions.get(&3), Some(&2));
        assert_eq!(cached_log.contact_positions.get(&1), Some(&3));
        assert_indexes_are_consistent(&cached_log);
    }

    #[test]
    fn insert_upsert_handles_beginning_boundary_for_descending_order() {
        let mut cached_log = cached_log(vec![contact(2, 200, "K2AAA"), contact(1, 100, "K1AAA")]);

        let indexes_dirty =
            cached_log.apply_upserted_contacts(&[contact(3, 300, "K3AAA")], &[None]);

        assert!(!indexes_dirty);
        assert_eq!(contact_ids(&cached_log), vec![3, 2, 1]);
        assert_indexes_are_consistent(&cached_log);
    }

    #[test]
    fn update_upsert_uses_slow_path_and_rebuilds_indexes() {
        let original = contact(1, 100, "K1AAA");
        let mut cached_log = cached_log(vec![contact(3, 300, "K3AAA"), original.clone()]);
        let updated = contact(1, 400, "W1NEW");

        let indexes_dirty = cached_log.apply_upserted_contacts(&[updated], &[Some(original)]);
        assert!(indexes_dirty);
        cached_log.rebuild_indexes();

        assert_eq!(contact_ids(&cached_log), vec![1, 3]);
        assert_eq!(
            cached_log.matching_contact_ids_for_prefix("W1"),
            HashSet::from([1])
        );
        assert!(cached_log.matching_contact_ids_for_prefix("K1").is_empty());
        assert_indexes_are_consistent(&cached_log);
    }
}
