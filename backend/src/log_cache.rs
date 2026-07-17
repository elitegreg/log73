use crate::contest_rules::ContestRulesStore;
use crate::db::{
    Contact, Database, contact_adif_value, contact_id, contact_meta_value, set_contact_adif,
    set_contact_meta,
};
use crate::dxcc::{DxccDatabase, DxccInfo};
use crate::scoring::{ContestScoringModule, ScoringModules};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

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
}

struct CachedLog {
    module: Arc<ContestScoringModule>,
    contacts: Vec<Contact>,
    callsign_index: BTreeMap<String, Vec<i64>>,
    contact_positions: HashMap<i64, usize>,
}

impl LogCache {
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
        if self.is_loaded(log_id) {
            return Ok(());
        }

        let (module, mut contacts) = self.load_log(log_id).await?;
        let processors = self.processors();

        let mut logs = self.inner.logs.lock().expect("log cache mutex poisoned");
        if logs.contains_key(&log_id) {
            return Ok(());
        }

        for processor in &processors {
            processor.on_log_loaded(log_id, Arc::clone(&module), &mut contacts);
        }

        sort_contacts_desc(&mut contacts);
        logs.insert(log_id, CachedLog::new(module, contacts));
        Ok(())
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

        self.ensure_loaded(log_id).await?;
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
        self.ensure_loaded(log_id).await?;
        enrich_missing_dxcc(&self.inner.dxcc, &mut contacts);

        let committed_contacts = self
            .inner
            .db
            .upsert_contacts(log_id, contacts)
            .await
            .map_err(|error| error.to_string())?;

        let processors = self.processors();
        let mut logs = self.inner.logs.lock().expect("log cache mutex poisoned");
        let Some(cached_log) = logs.get_mut(&log_id) else {
            return Err(format!("log {log_id} is not loaded"));
        };

        let previous_contacts = committed_contacts
            .iter()
            .map(|committed_contact| {
                contact_id(committed_contact)
                    .and_then(|contact_id| cached_log.contact_by_id(contact_id))
            })
            .collect::<Vec<_>>();

        let indexes_dirty =
            cached_log.apply_upserted_contacts(&committed_contacts, &previous_contacts);

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
                &committed_contacts,
                &previous_contacts,
            ));
        }

        if indexes_dirty {
            cached_log.rebuild_indexes();
        }
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
            .delete_contact(id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };

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

        Ok(Some(LogCacheDeleteResult {
            log_id,
            changed_contacts: dedupe_contacts(changed_contacts, &HashSet::new()),
        }))
    }

    pub fn remove_log(&self, log_id: i64) {
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
            .contains_key(&log_id)
    }

    async fn load_log(
        &self,
        log_id: i64,
    ) -> Result<(Arc<ContestScoringModule>, Vec<Contact>), String> {
        let log = self
            .inner
            .db
            .log(log_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("log {log_id} not found"))?;

        let rules = self
            .inner
            .contest_rules
            .get(&log.contest_id)
            .ok_or_else(|| format!("unknown contest: {}", log.contest_id))?;

        let module = self
            .inner
            .scoring_modules
            .get(rules, log.contest_params.clone());

        let mut contacts = self
            .inner
            .db
            .contacts(log_id)
            .await
            .map_err(|error| error.to_string())?;
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
                set_contact_meta(
                    contact,
                    "MY_DXCC_PREFIX",
                    serde_json::Value::String(info.primary_prefix),
                );
            }
            None => {
                set_contact_adif(contact, "MY_DXCC", serde_json::Value::Null);
                set_contact_adif(contact, "MY_CONT", serde_json::Value::Null);
                set_contact_meta(contact, "MY_DXCC_PREFIX", serde_json::Value::Null);
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
        let prefix_missing = contact_meta_value(contact, "DXCC_PREFIX")
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
            set_contact_meta(
                contact,
                "DXCC_PREFIX",
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

    fn test_module() -> Arc<ContestScoringModule> {
        let rules = ContestRules {
            contest: "test".to_string(),
            display_name: "Test".to_string(),
            allowed_bands: Vec::new(),
            allowed_modes: Vec::new(),
            define: Vec::new(),
            exchange: Vec::new(),
            qso_columns: Vec::new(),
            qso_column_fields: Default::default(),
            log_params: Vec::new(),
            qso_points: None,
            dupe_key: Vec::new(),
            multipliers: Vec::new(),
            bonus_points: Vec::new(),
            power_multiplier: Vec::new(),
            cabrillo: None,
            metadata: None,
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
            contact_meta_value(&contacts[0], "MY_DXCC_PREFIX"),
            Some(&json!("K"))
        );
        assert_eq!(
            contact_meta_value(&contacts[0], "DXCC_PREFIX"),
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
