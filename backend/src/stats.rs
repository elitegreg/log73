use crate::db::{Contact, contact_adif_value, contact_id};
use crate::log_cache::LogCacheProcessor;
use crate::scoring::ContestScoringModule;
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const LAST_10_CONTACTS: usize = 10;
const LAST_100_CONTACTS: usize = 100;
const LAST_30_MINUTES_SECONDS: i64 = 30 * 60;
const LAST_60_MINUTES_SECONDS: i64 = 60 * 60;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RateStat {
    pub qso_count: usize,
    pub rate_per_hour: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LogWindowStats {
    pub last_10_contacts: RateStat,
    pub last_100_contacts: RateStat,
    pub last_30_minutes: RateStat,
    pub last_60_minutes: RateStat,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LogStatsSnapshot {
    pub generated_at: i64,
    pub overall: LogWindowStats,
    pub by_operator: BTreeMap<String, LogWindowStats>,
}

#[derive(Clone)]
pub struct StatsTracker {
    logs: Arc<Mutex<HashMap<i64, StatsLogState>>>,
    now_fn: Arc<dyn Fn() -> i64 + Send + Sync>,
}

#[derive(Debug, Clone, Default)]
struct StatsLogState {
    contacts: Vec<StatsEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatsEntry {
    id: i64,
    epoch: i64,
    operator: String,
}

impl Default for StatsTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl StatsTracker {
    pub fn new() -> Self {
        Self::with_now_fn(Arc::new(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or(0)
        }))
    }

    fn with_now_fn(now_fn: Arc<dyn Fn() -> i64 + Send + Sync>) -> Self {
        Self {
            logs: Arc::new(Mutex::new(HashMap::new())),
            now_fn,
        }
    }

    pub fn snapshot(&self, log_id: i64) -> Option<LogStatsSnapshot> {
        let now = (self.now_fn)();
        let logs = self.logs.lock().expect("stats tracker mutex poisoned");
        let state = logs.get(&log_id)?;
        Some(state.snapshot(now))
    }
}

impl LogCacheProcessor for StatsTracker {
    fn on_log_loaded(
        &self,
        log_id: i64,
        _module: Arc<ContestScoringModule>,
        contacts: &mut [Contact],
    ) {
        let mut logs = self.logs.lock().expect("stats tracker mutex poisoned");
        logs.insert(log_id, StatsLogState::from_contacts(contacts));
    }

    fn on_contacts_upserted(
        &self,
        log_id: i64,
        _module: Arc<ContestScoringModule>,
        _contacts: &mut [Contact],
        committed_contacts: &[Contact],
        previous_contacts: &[Option<Contact>],
    ) -> Vec<Contact> {
        let mut logs = self.logs.lock().expect("stats tracker mutex poisoned");
        let state = logs.entry(log_id).or_default();
        for previous_contact in previous_contacts.iter().flatten() {
            if let Some(id) = contact_id(previous_contact) {
                state.remove_contact(id);
            }
        }
        for committed_contact in committed_contacts {
            if let Some(entry) = stats_entry(committed_contact) {
                state.upsert_contact(entry);
            }
        }
        Vec::new()
    }

    fn on_contact_deleted(
        &self,
        log_id: i64,
        _module: Arc<ContestScoringModule>,
        _contacts: &mut [Contact],
        deleted_contact: &Contact,
    ) -> Vec<Contact> {
        let mut logs = self.logs.lock().expect("stats tracker mutex poisoned");
        if let Some(state) = logs.get_mut(&log_id)
            && let Some(id) = contact_id(deleted_contact)
        {
            state.remove_contact(id);
        }
        Vec::new()
    }

    fn on_log_removed(&self, log_id: i64) {
        let mut logs = self.logs.lock().expect("stats tracker mutex poisoned");
        logs.remove(&log_id);
    }
}

impl StatsLogState {
    fn from_contacts(contacts: &[Contact]) -> Self {
        let mut entries = contacts.iter().filter_map(stats_entry).collect::<Vec<_>>();
        entries.sort_by(stats_entry_cmp);
        Self { contacts: entries }
    }

    fn upsert_contact(&mut self, entry: StatsEntry) {
        self.remove_contact(entry.id);
        let index = self
            .contacts
            .binary_search_by(|existing| stats_entry_cmp(existing, &entry))
            .unwrap_or_else(|index| index);
        self.contacts.insert(index, entry);
    }

    fn remove_contact(&mut self, id: i64) {
        if let Some(index) = self.contacts.iter().position(|entry| entry.id == id) {
            self.contacts.remove(index);
        }
    }

    fn snapshot(&self, now: i64) -> LogStatsSnapshot {
        let overall = window_stats(&self.contacts, now);
        let mut grouped = BTreeMap::<String, Vec<StatsEntry>>::new();
        for entry in &self.contacts {
            if entry.operator.is_empty() {
                continue;
            }
            grouped
                .entry(entry.operator.clone())
                .or_default()
                .push(entry.clone());
        }
        let by_operator = grouped
            .into_iter()
            .map(|(operator, entries)| (operator, window_stats(&entries, now)))
            .collect();

        LogStatsSnapshot {
            generated_at: now,
            overall,
            by_operator,
        }
    }
}

fn stats_entry(contact: &Contact) -> Option<StatsEntry> {
    let id = contact_id(contact)?;
    let epoch = contact_adif_value(contact, "QSO_DATE_TIME_ON")?.as_i64()?;
    let operator = contact_adif_value(contact, "OPERATOR")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_uppercase();
    Some(StatsEntry {
        id,
        epoch,
        operator,
    })
}

fn stats_entry_cmp(left: &StatsEntry, right: &StatsEntry) -> Ordering {
    left.epoch.cmp(&right.epoch).then(left.id.cmp(&right.id))
}

fn window_stats(entries: &[StatsEntry], now: i64) -> LogWindowStats {
    LogWindowStats {
        last_10_contacts: contact_window_rate(entries, LAST_10_CONTACTS),
        last_100_contacts: contact_window_rate(entries, LAST_100_CONTACTS),
        last_30_minutes: time_window_rate(entries, now, LAST_30_MINUTES_SECONDS),
        last_60_minutes: time_window_rate(entries, now, LAST_60_MINUTES_SECONDS),
    }
}

fn contact_window_rate(entries: &[StatsEntry], size: usize) -> RateStat {
    let count = entries.len().min(size);
    if count == 0 {
        return RateStat {
            qso_count: 0,
            rate_per_hour: 0.0,
        };
    }
    if count == 1 {
        return RateStat {
            qso_count: 1,
            rate_per_hour: 0.0,
        };
    }

    let window = &entries[entries.len() - count..];
    let span_seconds = (window.last().map(|entry| entry.epoch).unwrap_or(0)
        - window.first().map(|entry| entry.epoch).unwrap_or(0))
    .max(1);

    RateStat {
        qso_count: count,
        rate_per_hour: count as f64 * 3600.0 / span_seconds as f64,
    }
}

fn time_window_rate(entries: &[StatsEntry], now: i64, window_seconds: i64) -> RateStat {
    let threshold = now - window_seconds;
    let count = entries
        .iter()
        .filter(|entry| entry.epoch >= threshold && entry.epoch <= now)
        .count();
    RateStat {
        qso_count: count,
        rate_per_hour: count as f64 * 3600.0 / window_seconds as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::build_contact;
    use crate::log_cache::LogCacheProcessor;
    use crate::scoring::ContestScoringModule;
    use serde_json::{Map, json};

    fn test_tracker(now: i64) -> StatsTracker {
        StatsTracker::with_now_fn(Arc::new(move || now))
    }

    fn contact(id: i64, epoch: i64, operator: &str) -> Contact {
        build_contact(
            Map::from_iter([("id".to_string(), json!(id))]),
            Map::from_iter([
                ("QSO_DATE_TIME_ON".to_string(), json!(epoch)),
                ("OPERATOR".to_string(), json!(operator)),
            ]),
        )
    }

    fn module() -> Arc<ContestScoringModule> {
        crate::scoring::ScoringModules::new().get(
            &crate::contest_rules::ContestRules {
                contest: "test".to_string(),
                display_name: "Test".to_string(),
                allowed_bands: Vec::new(),
                allowed_modes: Vec::new(),
                define: Vec::new(),
                exchange: Vec::new(),
                qso_columns: Vec::new(),
                qso_column_fields: std::collections::BTreeMap::new(),
                log_params: Vec::new(),
                qso_points: None,
                dupe_key: Vec::new(),
                multipliers: Vec::new(),
                bonus_points: Vec::new(),
                power_multiplier: Vec::new(),
                cabrillo: None,
                metadata: None,
            },
            serde_json::Value::Null,
        )
    }

    #[test]
    fn snapshot_reports_overall_and_operator_windows() {
        let tracker = test_tracker(4_000);
        let mut contacts = vec![
            contact(1, 1_000, "K1AAA"),
            contact(2, 2_000, "K1AAA"),
            contact(3, 2_500, "N1BBB"),
            contact(4, 3_900, "K1AAA"),
        ];
        tracker.on_log_loaded(7, module(), &mut contacts);

        let snapshot = tracker.snapshot(7).expect("stats should exist");
        assert_eq!(snapshot.overall.last_10_contacts.qso_count, 4);
        assert_eq!(snapshot.overall.last_100_contacts.qso_count, 4);
        assert_eq!(snapshot.overall.last_30_minutes.qso_count, 2);
        assert_eq!(snapshot.overall.last_60_minutes.qso_count, 4);
        assert!(snapshot.overall.last_10_contacts.rate_per_hour > 0.0);

        let k1aaa = snapshot.by_operator.get("K1AAA").expect("operator exists");
        assert_eq!(k1aaa.last_10_contacts.qso_count, 3);
        assert_eq!(k1aaa.last_30_minutes.qso_count, 1);

        let n1bbb = snapshot.by_operator.get("N1BBB").expect("operator exists");
        assert_eq!(n1bbb.last_10_contacts.qso_count, 1);
        assert_eq!(n1bbb.last_10_contacts.rate_per_hour, 0.0);
    }

    #[test]
    fn upsert_replaces_prior_operator_and_epoch() {
        let tracker = test_tracker(10_000);
        let module = module();
        let mut contacts = vec![contact(1, 8_000, "K1AAA")];
        tracker.on_log_loaded(3, Arc::clone(&module), &mut contacts);

        let committed = vec![contact(1, 9_500, "N1BBB")];
        let previous = vec![Some(contact(1, 8_000, "K1AAA"))];
        let mut empty_contacts = Vec::new();
        tracker.on_contacts_upserted(3, module, &mut empty_contacts, &committed, &previous);

        let snapshot = tracker.snapshot(3).expect("stats should exist");
        assert!(!snapshot.by_operator.contains_key("K1AAA"));
        let n1bbb = snapshot.by_operator.get("N1BBB").expect("operator exists");
        assert_eq!(n1bbb.last_10_contacts.qso_count, 1);
        assert_eq!(snapshot.overall.last_30_minutes.qso_count, 1);
    }

    #[test]
    fn delete_removes_contact_from_stats() {
        let tracker = test_tracker(10_000);
        let module = module();
        let mut contacts = vec![contact(1, 8_000, "K1AAA"), contact(2, 9_000, "K1AAA")];
        tracker.on_log_loaded(9, Arc::clone(&module), &mut contacts);
        let mut empty_contacts = Vec::new();
        tracker.on_contact_deleted(9, module, &mut empty_contacts, &contact(2, 9_000, "K1AAA"));

        let snapshot = tracker.snapshot(9).expect("stats should exist");
        assert_eq!(snapshot.overall.last_10_contacts.qso_count, 1);
        assert_eq!(snapshot.by_operator["K1AAA"].last_10_contacts.qso_count, 1);
    }
}
