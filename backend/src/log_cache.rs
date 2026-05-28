use crate::contest_rules::ContestRulesStore;
use crate::db::{Contact, Database};
use crate::scoring::{ContestScoringModule, ScoringModules};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
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
    logs: Mutex<HashMap<i64, CachedLog>>,
    processors: Mutex<Vec<Arc<dyn LogCacheProcessor>>>,
}

struct CachedLog {
    module: Arc<ContestScoringModule>,
    contacts: Vec<Contact>,
}

impl LogCache {
    pub fn new(
        db: Database,
        contest_rules: ContestRulesStore,
        scoring_modules: ScoringModules,
    ) -> Self {
        Self {
            inner: Arc::new(LogCacheInner {
                db,
                contest_rules,
                scoring_modules,
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
        logs.insert(log_id, CachedLog { module, contacts });
        Ok(())
    }

    pub async fn contacts_display_page(
        &self,
        log_id: i64,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Contact>, String> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        self.ensure_loaded(log_id).await?;
        let logs = self.inner.logs.lock().expect("log cache mutex poisoned");
        let Some(cached_log) = logs.get(&log_id) else {
            return Ok(Vec::new());
        };

        if offset >= cached_log.contacts.len() {
            return Ok(Vec::new());
        }

        let end = offset.saturating_add(limit).min(cached_log.contacts.len());
        Ok(cached_log.contacts[offset..end].to_vec())
    }

    pub async fn upsert_contacts(
        &self,
        log_id: i64,
        contacts: Vec<Contact>,
    ) -> Result<LogCacheUpsertResult, String> {
        self.ensure_loaded(log_id).await?;

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
                contact_id(committed_contact).and_then(|id| {
                    cached_log
                        .contacts
                        .iter()
                        .find(|contact| contact_id(contact) == Some(id))
                        .cloned()
                })
            })
            .collect::<Vec<_>>();

        for committed_contact in &committed_contacts {
            merge_contact(&mut cached_log.contacts, committed_contact);
        }
        sort_contacts_desc(&mut cached_log.contacts);

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

        let changed_contacts = dedupe_contacts(changed_contacts, &committed_contact_ids);

        let contacts = committed_contacts
            .iter()
            .filter_map(|committed_contact| {
                let id = contact_id(committed_contact)?;
                cached_log
                    .contacts
                    .iter()
                    .find(|contact| contact_id(contact) == Some(id))
                    .cloned()
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
        sort_contacts_desc(&mut contacts);

        Ok((module, contacts))
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

fn sort_contacts_desc(contacts: &mut [Contact]) {
    contacts.sort_by_key(|contact| Reverse(contact_order_key(contact)));
}

fn contact_order_key(contact: &Contact) -> (i64, i64) {
    (
        contact
            .get("QSO_DATE_TIME_ON")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0),
        contact_id(contact).unwrap_or(0),
    )
}

fn contact_id(contact: &Contact) -> Option<i64> {
    contact
        .get("_id")
        .or_else(|| contact.get("ID"))
        .and_then(serde_json::Value::as_i64)
}
