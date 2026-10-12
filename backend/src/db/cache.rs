//! Canonical persisted data. Only the SQLite worker publishes mutations here.
//! A backend owns its database exclusively; outside edits require a restart.
use super::config::db_config;
use super::contact::{Contact, contact_adif, contact_adif_value, contact_id};
use super::contacts::json_serial_value;
use super::logs::db_logs;
use super::models::{AuthConfig, ConfigView, DxClusterConfig, Log, RadioRecord};
use super::radios::db_radios;
use rusqlite::Connection;
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

#[derive(Clone)]
pub struct LogSnapshot {
    pub log: Log,
    pub revision: u64,
    pub contacts: Arc<CachedContacts>,
}

pub struct ContactMutation {
    pub previous_revision: u64,
    pub revision: u64,
    pub contacts: Vec<Contact>,
}

pub struct ContactDeletion {
    pub log_id: i64,
    pub previous_revision: u64,
    pub revision: u64,
}

#[derive(Clone, Default)]
struct SerialValues {
    global: BTreeMap<i64, usize>,
    bands: HashMap<String, BTreeMap<i64, usize>>,
}

#[derive(Clone, Default)]
pub struct CachedContacts {
    ordered: BTreeMap<Reverse<(i64, i64)>, Arc<Contact>>,
    by_id: HashMap<i64, Arc<Contact>>,
    serials: HashMap<String, SerialValues>,
}

impl CachedContacts {
    pub(super) fn new(contacts: Vec<Contact>) -> Self {
        let mut cached = Self::default();
        for contact in contacts {
            cached.upsert(contact);
        }
        cached
    }

    pub fn to_vec(&self) -> Vec<Contact> {
        self.ordered
            .values()
            .map(|contact| contact.as_ref().clone())
            .collect()
    }

    pub fn contact(&self, id: i64) -> Option<Contact> {
        self.by_id.get(&id).map(|contact| contact.as_ref().clone())
    }

    pub fn serial_max(&self, field: &str, band: Option<&str>) -> Option<i64> {
        let values = self.serials.get(&field.trim().to_ascii_uppercase())?;
        let values = match band {
            Some(band) => values.bands.get(&band.to_ascii_lowercase())?,
            None => &values.global,
        };
        values.last_key_value().map(|(value, _)| *value)
    }

    pub(super) fn upsert(&mut self, contact: Contact) {
        let id = contact_id(&contact).expect("persisted contact has an ID");
        self.remove(id);
        self.update_serials(&contact, true);
        let contact = Arc::new(contact);
        self.ordered
            .insert(order_key(&contact), Arc::clone(&contact));
        self.by_id.insert(id, contact);
    }

    pub(super) fn remove(&mut self, id: i64) {
        if let Some(contact) = self.by_id.remove(&id) {
            self.ordered.remove(&order_key(&contact));
            self.update_serials(&contact, false);
        }
    }

    fn update_serials(&mut self, contact: &Contact, insert: bool) {
        let band = contact_adif_value(contact, "BAND")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        for (field, value) in contact_adif(contact).into_iter().flatten() {
            let Some(value) = json_serial_value(value) else {
                continue;
            };
            let values = self.serials.entry(field.clone()).or_default();
            update_count(&mut values.global, value, insert);
            let band_values = values.bands.entry(band.clone()).or_default();
            update_count(band_values, value, insert);
            if band_values.is_empty() {
                values.bands.remove(&band);
            }
            if values.global.is_empty() {
                self.serials.remove(field);
            }
        }
    }
}

fn update_count(values: &mut BTreeMap<i64, usize>, value: i64, insert: bool) {
    if insert {
        *values.entry(value).or_default() += 1;
    } else if let Some(count) = values.get_mut(&value) {
        *count -= 1;
        if *count == 0 {
            values.remove(&value);
        }
    }
}

fn order_key(contact: &Contact) -> Reverse<(i64, i64)> {
    Reverse((
        contact_adif_value(contact, "QSO_DATE_TIME_ON")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default(),
        contact_id(contact).unwrap_or_default(),
    ))
}

pub(super) struct CachedLog {
    pub log: Log,
    pub revision: u64,
    pub load_gate: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Clone)]
pub(super) struct CachedConfig {
    pub auth: AuthConfig,
    pub dxcluster: DxClusterConfig,
    pub region: i64,
}

impl CachedConfig {
    pub fn view(&self) -> ConfigView {
        ConfigView {
            iaru_region: self.region,
            login_user: self.auth.login_user.clone(),
            login_enabled: !self.auth.login_user.trim().is_empty()
                && !self.auth.login_password.is_empty(),
            dxcluster_enabled: self.dxcluster.enabled,
            dxcluster_host: self.dxcluster.host.clone(),
            dxcluster_port: self.dxcluster.port,
            dxcluster_callsign: self.dxcluster.callsign.clone(),
            dxcluster_max_age_min: self.dxcluster.max_age_min,
            dxcluster_commands: self.dxcluster.commands.clone(),
        }
    }
}

pub(super) struct DatabaseCache {
    pub config: CachedConfig,
    pub logs: HashMap<i64, CachedLog>,
    pub radios: BTreeMap<i64, RadioRecord>,
    pub owners: HashMap<i64, i64>,
    pub counts: HashMap<i64, usize>,
    pub contacts: HashMap<i64, Arc<CachedContacts>>,
    pub next_serial: HashMap<(i64, String), i64>,
    next_revision: u64,
}

impl DatabaseCache {
    pub fn load(connection: &Connection) -> rusqlite::Result<Self> {
        let config = db_config(connection)?;
        let mut cache = Self {
            config,
            logs: HashMap::new(),
            radios: db_radios(connection)?
                .into_iter()
                .map(|r| (r.id, r))
                .collect(),
            owners: HashMap::new(),
            counts: HashMap::new(),
            contacts: HashMap::new(),
            next_serial: HashMap::new(),
            next_revision: 0,
        };
        for log in db_logs(connection)? {
            cache.publish_log(log, false);
        }
        let mut statement = connection.prepare("SELECT ID, LOG_ID FROM qsos")?;
        for row in
            statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
        {
            let (id, log_id) = row?;
            cache.owners.insert(id, log_id);
            *cache.counts.entry(log_id).or_default() += 1;
        }
        for log_id in cache.logs.keys() {
            if cache.counts.get(log_id).copied().unwrap_or_default() == 0 {
                cache
                    .contacts
                    .insert(*log_id, Arc::new(CachedContacts::default()));
            }
        }
        let mut statement =
            connection.prepare("SELECT LOG_ID, FIELD_ADIF, NEXT_SERIAL FROM log_serial_state")?;
        for row in statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })? {
            let (log_id, field, next) = row?;
            let entry = cache
                .next_serial
                .entry((log_id, field.trim().to_ascii_uppercase()))
                .or_default();
            *entry = (*entry).max(next);
        }
        Ok(cache)
    }

    pub fn bump_revision(&mut self, log_id: i64) -> u64 {
        self.next_revision = self
            .next_revision
            .checked_add(1)
            .expect("cache revision overflow");
        if let Some(log) = self.logs.get_mut(&log_id) {
            log.revision = self.next_revision;
        }
        self.next_revision
    }

    pub fn publish_log(&mut self, log: Log, new: bool) {
        let id = log.id;
        if let Some(cached) = self.logs.get_mut(&id) {
            cached.log = log;
        } else {
            self.logs.insert(
                id,
                CachedLog {
                    log,
                    revision: 0,
                    load_gate: Arc::new(tokio::sync::Mutex::new(())),
                },
            );
        }
        self.bump_revision(id);
        if new {
            self.counts.insert(id, 0);
            self.contacts
                .insert(id, Arc::new(CachedContacts::default()));
        }
    }

    pub fn remove_log(&mut self, id: i64) {
        self.logs.remove(&id);
        self.contacts.remove(&id);
        self.counts.remove(&id);
        self.owners.retain(|_, log_id| *log_id != id);
        self.next_serial.retain(|(log_id, _), _| *log_id != id);
    }

    pub fn snapshot(&self, id: i64) -> Option<LogSnapshot> {
        let log = self.logs.get(&id)?;
        Some(LogSnapshot {
            log: log.log.clone(),
            revision: log.revision,
            contacts: Arc::clone(self.contacts.get(&id)?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_preload_fails_closed_if_auth_columns_are_missing() {
        let connection = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&connection).unwrap();
        connection
            .execute_batch("ALTER TABLE config DROP COLUMN LOGIN_PASSWORD")
            .unwrap();
        assert!(DatabaseCache::load(&connection).is_err());
    }
}
