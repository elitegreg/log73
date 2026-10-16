use super::cache::{CachedContacts, ContactDeletion, ContactMutation, DatabaseCache, LogSnapshot};
use super::config::db_update_config;
use super::contact::{Contact, contact_id};
use super::contacts::{
    db_contact_has_warnings, db_contacts, db_delete_contact, db_upsert_contacts,
};
use super::logs::{db_create_log, db_delete_log, db_update_log};
use super::models::{
    AuthConfig, ConfigView, DxClusterConfig, Log, NewLog, RadioControlLocation, RadioPayload,
    RadioRecord, SerialAllocation, UpdateConfig, UpdateLog,
};
use super::radios::{db_create_radio, db_delete_radio, db_update_radio, db_upsert_client_radio};
use super::schema::initialize_schema;
use super::serials::db_allocate_serial;
use crate::diagnostics::{self, Stage, StageTimer, Timings};
use rusqlite::Connection;
use std::path::Path;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicU64, Ordering},
};
use std::thread;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};

const DB_COMMAND_BUFFER: usize = 64;

#[derive(Default)]
struct CacheMetrics {
    commands: AtomicU64,
    contact_loads: AtomicU64,
    #[cfg(test)]
    sql_reads: AtomicU64,
}

#[cfg(test)]
thread_local! {
    static SQL_METRICS: std::cell::RefCell<Option<Arc<CacheMetrics>>> = const { std::cell::RefCell::new(None) };
}

struct QueuedCommand {
    command: DbCommand,
    queued_at: Instant,
    span: tracing::Span,
    timings: Option<Arc<Timings>>,
}

#[cfg(test)]
type TestAction = Box<dyn FnOnce(&mut Connection) -> rusqlite::Result<()> + Send>;

enum DbCommand {
    #[cfg(test)]
    TestControl {
        action: TestAction,
        response: oneshot::Sender<rusqlite::Result<()>>,
    },
    #[cfg(test)]
    TestClose {
        response: oneshot::Sender<rusqlite::Result<()>>,
    },
    CreateLog {
        log: NewLog,
        response: oneshot::Sender<rusqlite::Result<Log>>,
    },
    UpdateLog {
        id: i64,
        log: UpdateLog,
        response: oneshot::Sender<rusqlite::Result<Option<Log>>>,
    },
    DeleteLog {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<bool>>,
    },
    UpdateConfig {
        config: UpdateConfig,
        response: oneshot::Sender<rusqlite::Result<()>>,
    },
    CreateRadio {
        radio: RadioPayload,
        response: oneshot::Sender<rusqlite::Result<RadioRecord>>,
    },
    UpdateRadio {
        id: i64,
        radio: RadioPayload,
        response: oneshot::Sender<rusqlite::Result<Option<RadioRecord>>>,
    },
    DeleteRadio {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<bool>>,
    },
    UpsertClientRadio {
        client_instance_id: String,
        radio_ws_url: String,
        radio: RadioPayload,
        response: oneshot::Sender<rusqlite::Result<RadioRecord>>,
    },
    LoadContacts {
        log_id: i64,
        response: oneshot::Sender<rusqlite::Result<()>>,
    },
    UpsertContacts {
        log_id: i64,
        contacts: Vec<Contact>,
        response: oneshot::Sender<rusqlite::Result<ContactMutation>>,
    },
    AllocateSerial {
        log_id: i64,
        field_adif: String,
        response: oneshot::Sender<rusqlite::Result<SerialAllocation>>,
    },
    DeleteContact {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<Option<ContactDeletion>>>,
    },
    ContactHasWarnings {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<bool>>,
    },
}

#[derive(Clone)]
pub struct Database {
    commands: mpsc::Sender<QueuedCommand>,
    cache: Arc<RwLock<DatabaseCache>>,
    #[cfg(test)]
    metrics: Arc<CacheMetrics>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let (commands, command_rx) = mpsc::channel(DB_COMMAND_BUFFER);
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let path = path.as_ref().to_path_buf();
        let metrics = Arc::new(CacheMetrics::default());
        let worker_metrics = Arc::clone(&metrics);
        thread::Builder::new()
            .name("log73-db-worker".to_string())
            .spawn(move || {
                let initialization = Connection::open(&path).and_then(|connection| {
                    configure_connection(&connection)?;
                    initialize_schema(&connection)?;
                    let cache = Arc::new(RwLock::new(DatabaseCache::load(&connection)?));
                    Ok((connection, cache))
                });
                match initialization {
                    Ok((connection, cache)) => {
                        #[cfg(test)]
                        let mut connection = connection;
                        #[cfg(test)]
                        {
                            SQL_METRICS.with(|slot| {
                                *slot.borrow_mut() = Some(Arc::clone(&worker_metrics))
                            });
                            connection.trace(Some(|sql| {
                                if sql.trim_start().to_ascii_uppercase().starts_with("SELECT") {
                                    SQL_METRICS.with(|slot| {
                                        if let Some(metrics) = slot.borrow().as_ref() {
                                            metrics.sql_reads.fetch_add(1, Ordering::Relaxed);
                                        }
                                    });
                                }
                            }));
                        }
                        let _ = ready_tx.send(Ok(Arc::clone(&cache)));
                        run_db_worker(connection, command_rx, cache, worker_metrics);
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                    }
                }
            })
            .map_err(|error| {
                rusqlite::Error::InvalidParameterName(format!(
                    "failed to spawn database worker thread: {error}"
                ))
            })?;
        let cache = ready_rx
            .recv()
            .map_err(|_| database_worker_unavailable())??;
        Ok(Self {
            commands,
            cache,
            #[cfg(test)]
            metrics,
        })
    }

    async fn call<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<rusqlite::Result<T>>) -> DbCommand,
    ) -> rusqlite::Result<T> {
        let (response_tx, response_rx) = oneshot::channel();
        self.commands
            .send(QueuedCommand {
                command: command(response_tx),
                queued_at: Instant::now(),
                span: tracing::Span::current(),
                timings: diagnostics::current_timings(),
            })
            .await
            .map_err(|_| database_worker_unavailable())?;
        response_rx
            .await
            .map_err(|_| database_worker_unavailable())?
    }

    fn read<T>(&self, read: impl FnOnce(&DatabaseCache) -> T) -> T {
        diagnostics::cache_hit();
        read(&self.cache.read().expect("database cache poisoned"))
    }

    pub fn log_revision(&self, id: i64) -> Option<u64> {
        self.read(|cache| cache.logs.get(&id).map(|log| log.revision))
    }

    pub async fn logs(&self) -> rusqlite::Result<Vec<Log>> {
        Ok(self.read(|cache| {
            let mut logs = cache
                .logs
                .values()
                .map(|log| log.log.clone())
                .collect::<Vec<_>>();
            logs.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
            logs
        }))
    }

    pub async fn log(&self, id: i64) -> rusqlite::Result<Option<Log>> {
        Ok(self.read(|cache| cache.logs.get(&id).map(|log| log.log.clone())))
    }

    pub async fn create_log(&self, log: NewLog) -> rusqlite::Result<Log> {
        self.call(|response| DbCommand::CreateLog { log, response })
            .await
    }
    pub async fn update_log(&self, id: i64, log: UpdateLog) -> rusqlite::Result<Option<Log>> {
        self.call(|response| DbCommand::UpdateLog { id, log, response })
            .await
    }
    pub async fn delete_log(&self, id: i64) -> rusqlite::Result<bool> {
        if self.read(|cache| !cache.logs.contains_key(&id)) {
            return Ok(false);
        }
        self.call(|response| DbCommand::DeleteLog { id, response })
            .await
    }
    pub async fn log_qso_count(&self, id: i64) -> rusqlite::Result<usize> {
        Ok(self.read(|cache| cache.counts.get(&id).copied().unwrap_or_default()))
    }
    pub async fn radios(&self) -> rusqlite::Result<Vec<RadioRecord>> {
        Ok(self.read(|cache| cache.radios.values().cloned().collect()))
    }
    pub async fn radio(&self, id: i64) -> rusqlite::Result<Option<RadioRecord>> {
        Ok(self.read(|cache| cache.radios.get(&id).cloned()))
    }
    pub async fn backend_radio_configs(&self) -> rusqlite::Result<Vec<radio_io::RadioConfig>> {
        Ok(self.read(|cache| {
            cache
                .radios
                .values()
                .filter(|radio| radio.control_location == RadioControlLocation::Backend)
                .map(|radio| radio.config.clone())
                .collect()
        }))
    }
    pub async fn auth_config(&self) -> rusqlite::Result<AuthConfig> {
        Ok(self.read(|cache| cache.config.auth.clone()))
    }
    /// Synchronize consumers while holding the cache's read lock, preventing
    /// an older config snapshot from replacing a newer authentication generation.
    pub fn with_auth_config<T>(&self, consume: impl FnOnce(&AuthConfig) -> T) -> T {
        self.read(|cache| consume(&cache.config.auth))
    }
    pub async fn dxcluster_config(&self) -> rusqlite::Result<DxClusterConfig> {
        Ok(self.read(|cache| cache.config.dxcluster.clone()))
    }
    pub async fn iaru_region(&self) -> rusqlite::Result<i64> {
        Ok(self.read(|cache| cache.config.region))
    }
    pub async fn config_view(&self) -> rusqlite::Result<ConfigView> {
        Ok(self.read(|cache| cache.config.view()))
    }
    pub async fn update_config(&self, config: UpdateConfig) -> rusqlite::Result<()> {
        self.call(|response| DbCommand::UpdateConfig { config, response })
            .await
    }
    pub async fn create_radio(&self, radio: RadioPayload) -> rusqlite::Result<RadioRecord> {
        self.call(|response| DbCommand::CreateRadio { radio, response })
            .await
    }
    pub async fn update_radio(
        &self,
        id: i64,
        radio: RadioPayload,
    ) -> rusqlite::Result<Option<RadioRecord>> {
        self.call(|response| DbCommand::UpdateRadio {
            id,
            radio,
            response,
        })
        .await
    }
    pub async fn delete_radio(&self, id: i64) -> rusqlite::Result<bool> {
        if self.read(|cache| !cache.radios.contains_key(&id)) {
            return Ok(false);
        }
        self.call(|response| DbCommand::DeleteRadio { id, response })
            .await
    }
    pub async fn upsert_client_radio(
        &self,
        client_instance_id: String,
        radio_ws_url: String,
        radio: RadioPayload,
    ) -> rusqlite::Result<RadioRecord> {
        self.call(|response| DbCommand::UpsertClientRadio {
            client_instance_id,
            radio_ws_url,
            radio,
            response,
        })
        .await
    }

    /// Metadata and canonical contacts from the same committed cache revision.
    pub async fn log_snapshot(&self, id: i64) -> rusqlite::Result<Option<LogSnapshot>> {
        loop {
            let gate = {
                let cache = self.cache.read().expect("database cache poisoned");
                if let Some(snapshot) = cache.snapshot(id) {
                    diagnostics::cache_hit();
                    return Ok(Some(snapshot));
                }
                let Some(log) = cache.logs.get(&id) else {
                    diagnostics::cache_hit();
                    return Ok(None);
                };
                Arc::clone(&log.load_gate)
            };
            let _guard = gate.lock().await;
            {
                let cache = self.cache.read().expect("database cache poisoned");
                if let Some(snapshot) = cache.snapshot(id) {
                    diagnostics::cache_hit();
                    return Ok(Some(snapshot));
                }
                if !cache.logs.contains_key(&id) {
                    return Ok(None);
                }
            }
            diagnostics::cache_miss();
            self.call(|response| DbCommand::LoadContacts {
                log_id: id,
                response,
            })
            .await?;
        }
    }

    #[allow(dead_code)] // Non-versioned convenience API; callers needing metadata use snapshots.
    pub async fn contacts(&self, log_id: i64) -> rusqlite::Result<Vec<Contact>> {
        Ok(self
            .log_snapshot(log_id)
            .await?
            .map(|snapshot| snapshot.contacts.to_vec())
            .unwrap_or_default())
    }
    pub async fn contact(&self, id: i64) -> rusqlite::Result<Option<Contact>> {
        let Some(log_id) = self.contact_log_id(id).await? else {
            return Ok(None);
        };
        Ok(self
            .log_snapshot(log_id)
            .await?
            .and_then(|snapshot| snapshot.contacts.contact(id)))
    }
    #[allow(dead_code)] // Non-versioned convenience API for persistence callers.
    pub async fn upsert_contacts(
        &self,
        log_id: i64,
        contacts: Vec<Contact>,
    ) -> rusqlite::Result<Vec<Contact>> {
        Ok(self
            .upsert_contacts_versioned(log_id, contacts)
            .await?
            .contacts)
    }
    pub async fn upsert_contacts_versioned(
        &self,
        log_id: i64,
        contacts: Vec<Contact>,
    ) -> rusqlite::Result<ContactMutation> {
        self.call(|response| DbCommand::UpsertContacts {
            log_id,
            contacts,
            response,
        })
        .await
    }
    pub async fn contact_log_id(&self, id: i64) -> rusqlite::Result<Option<i64>> {
        Ok(self.read(|cache| cache.owners.get(&id).copied()))
    }
    pub async fn allocate_serial(
        &self,
        log_id: i64,
        field_adif: String,
    ) -> rusqlite::Result<SerialAllocation> {
        self.call(|response| DbCommand::AllocateSerial {
            log_id,
            field_adif,
            response,
        })
        .await
    }
    #[allow(dead_code)] // Kept as the non-versioned persistence API.
    pub async fn delete_contact(&self, id: i64) -> rusqlite::Result<Option<i64>> {
        Ok(self
            .delete_contact_versioned(id)
            .await?
            .map(|deleted| deleted.log_id))
    }
    pub async fn contact_has_warnings(&self, id: i64) -> rusqlite::Result<bool> {
        self.call(|response| DbCommand::ContactHasWarnings { id, response })
            .await
    }
    pub async fn delete_contact_versioned(
        &self,
        id: i64,
    ) -> rusqlite::Result<Option<ContactDeletion>> {
        if self.read(|cache| !cache.owners.contains_key(&id)) {
            return Ok(None);
        }
        self.call(|response| DbCommand::DeleteContact { id, response })
            .await
    }

    #[cfg(test)]
    pub fn test_counters(&self) -> (u64, u64, u64) {
        (
            self.metrics.commands.load(Ordering::Relaxed),
            self.metrics.contact_loads.load(Ordering::Relaxed),
            self.metrics.sql_reads.load(Ordering::Relaxed),
        )
    }

    #[cfg(test)]
    pub(crate) async fn test_control(
        &self,
        action: impl FnOnce(&mut Connection) -> rusqlite::Result<()> + Send + 'static,
    ) -> rusqlite::Result<()> {
        self.call(|response| DbCommand::TestControl {
            action: Box::new(action),
            response,
        })
        .await
    }

    #[cfg(test)]
    async fn test_close(&self) {
        self.call(|response| DbCommand::TestClose { response })
            .await
            .unwrap();
    }

    #[cfg(test)]
    pub(crate) fn test_pending_commands(&self) -> usize {
        DB_COMMAND_BUFFER - self.commands.capacity()
    }
}

fn database_worker_unavailable() -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName("database worker unavailable".to_string())
}

fn configure_connection(connection: &Connection) -> rusqlite::Result<()> {
    let database_file: String =
        connection.query_row("PRAGMA database_list", [], |row| row.get(2))?;
    let journal_mode: String =
        connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    if !database_file.is_empty() && !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "could not enable SQLite WAL mode (effective mode: {journal_mode})"
        )));
    }
    let synchronous = if cfg!(target_os = "windows") {
        "NORMAL"
    } else {
        "FULL"
    };
    connection.pragma_update(None, "synchronous", synchronous)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    tracing::info!(journal_mode, synchronous, "configured SQLite connection");
    Ok(())
}

impl DbCommand {
    fn name(&self) -> &'static str {
        match self {
            #[cfg(test)]
            Self::TestControl { .. } => "test_control",
            #[cfg(test)]
            Self::TestClose { .. } => "test_close",
            Self::CreateLog { .. } => "create_log",
            Self::UpdateLog { .. } => "update_log",
            Self::DeleteLog { .. } => "delete_log",
            Self::UpdateConfig { .. } => "update_config",
            Self::CreateRadio { .. } => "create_radio",
            Self::UpdateRadio { .. } => "update_radio",
            Self::DeleteRadio { .. } => "delete_radio",
            Self::UpsertClientRadio { .. } => "upsert_client_radio",
            Self::LoadContacts { .. } => "load_contacts",
            Self::UpsertContacts { .. } => "upsert_contacts",
            Self::AllocateSerial { .. } => "allocate_serial",
            Self::DeleteContact { .. } => "delete_contact",
            Self::ContactHasWarnings { .. } => "contact_has_warnings",
        }
    }
}

fn load_contacts(
    connection: &Connection,
    cache: &RwLock<DatabaseCache>,
    metrics: &CacheMetrics,
    id: i64,
) -> rusqlite::Result<()> {
    {
        let cache = cache.read().expect("database cache poisoned");
        if !cache.logs.contains_key(&id) || cache.contacts.contains_key(&id) {
            return Ok(());
        }
    }
    let contacts = CachedContacts::new(db_contacts(connection, id)?);
    metrics.contact_loads.fetch_add(1, Ordering::Relaxed);
    cache
        .write()
        .expect("database cache poisoned")
        .contacts
        .insert(id, Arc::new(contacts));
    Ok(())
}

fn write_contacts(
    connection: &mut Connection,
    cache: &RwLock<DatabaseCache>,
    metrics: &CacheMetrics,
    log_id: i64,
    contacts: Vec<Contact>,
) -> rusqlite::Result<ContactMutation> {
    load_contacts(connection, cache, metrics, log_id)?;
    let (contest_id, previous_revision) = {
        let cache = cache.read().expect("database cache poisoned");
        let log = cache
            .logs
            .get(&log_id)
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        for contact in &contacts {
            if let Some(id) = contact_id(contact)
                && let Some(owner) = cache.owners.get(&id)
                && *owner != log_id
            {
                return Err(rusqlite::Error::InvalidParameterName(format!(
                    "contact id {id} belongs to log {owner}, cannot write to log {log_id}"
                )));
            }
        }
        (log.log.contest_id.clone(), log.revision)
    };
    let contacts = db_upsert_contacts(connection, log_id, contacts, &contest_id)?;
    let mut cache = cache.write().expect("database cache poisoned");
    for contact in &contacts {
        let id = contact_id(contact).expect("persisted contact ID");
        if cache.owners.insert(id, log_id).is_none() {
            *cache.counts.entry(log_id).or_default() += 1;
        }
        Arc::make_mut(cache.contacts.get_mut(&log_id).expect("contacts loaded"))
            .upsert(contact.clone());
    }
    let revision = cache.bump_revision(log_id);
    Ok(ContactMutation {
        previous_revision,
        revision,
        contacts,
    })
}

fn reserve_serial(
    connection: &mut Connection,
    cache: &RwLock<DatabaseCache>,
    metrics: &CacheMetrics,
    log_id: i64,
    field_adif: String,
) -> rusqlite::Result<SerialAllocation> {
    let requested_field = field_adif.trim().to_string();
    let field_adif = requested_field.to_ascii_uppercase();
    if log_id <= 0 || field_adif.is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "log id must be positive and serial field must not be empty".to_string(),
        ));
    }
    load_contacts(connection, cache, metrics, log_id)?;
    let serial = {
        let cache = cache.read().expect("database cache poisoned");
        let contacts = cache
            .contacts
            .get(&log_id)
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let committed_next = match contacts.serial_max(&field_adif, None) {
            Some(value) => value.checked_add(1).ok_or_else(|| {
                rusqlite::Error::InvalidParameterName("serial allocation overflow".to_string())
            })?,
            None => 1,
        };
        cache
            .next_serial
            .get(&(log_id, field_adif.clone()))
            .copied()
            .unwrap_or(1)
            .max(committed_next)
            .max(1)
    };
    let mut allocation = db_allocate_serial(connection, log_id, &field_adif, serial)?;
    allocation.field_adif = requested_field;
    cache
        .write()
        .expect("database cache poisoned")
        .next_serial
        .insert((log_id, field_adif), serial + 1);
    Ok(allocation)
}

fn remove_contact(
    connection: &Connection,
    cache: &RwLock<DatabaseCache>,
    id: i64,
) -> rusqlite::Result<Option<ContactDeletion>> {
    let Some(log_id) = db_delete_contact(connection, id)? else {
        return Ok(None);
    };
    let mut cache = cache.write().expect("database cache poisoned");
    let previous_revision = cache
        .logs
        .get(&log_id)
        .expect("persisted contact log")
        .revision;
    cache.owners.remove(&id);
    *cache
        .counts
        .get_mut(&log_id)
        .expect("persisted contact count") -= 1;
    if let Some(contacts) = cache.contacts.get_mut(&log_id) {
        Arc::make_mut(contacts).remove(id);
    }
    let revision = cache.bump_revision(log_id);
    Ok(Some(ContactDeletion {
        log_id,
        previous_revision,
        revision,
    }))
}

fn run_db_worker(
    mut connection: Connection,
    mut commands: mpsc::Receiver<QueuedCommand>,
    cache: Arc<RwLock<DatabaseCache>>,
    metrics: Arc<CacheMetrics>,
) {
    while let Some(queued) = commands.blocking_recv() {
        let _span = queued.span.enter();
        let operation = queued.command.name();
        metrics.commands.fetch_add(1, Ordering::Relaxed);
        let is_load = matches!(queued.command, DbCommand::LoadContacts { .. });
        diagnostics::database_command(is_load, queued.timings.as_deref());
        diagnostics::record(
            Stage::DatabaseQueue,
            operation,
            queued.queued_at.elapsed(),
            queued.timings.as_deref(),
        );
        let request_timings = queued.timings.clone();
        let loads_before = metrics.contact_loads.load(Ordering::Relaxed);
        let timer = StageTimer::with_timings(Stage::DatabaseExecution, operation, queued.timings);
        macro_rules! respond {
            ($response:expr, $work:expr) => {{
                let result = $work;
                diagnostics::cache_load(
                    metrics.contact_loads.load(Ordering::Relaxed) - loads_before,
                    request_timings.as_deref(),
                );
                drop(timer);
                let _ = $response.send(result);
            }};
        }
        match queued.command {
            #[cfg(test)]
            DbCommand::TestControl { action, response } => {
                respond!(response, action(&mut connection))
            }
            #[cfg(test)]
            DbCommand::TestClose { response } => {
                drop(connection);
                let _ = response.send(Ok(()));
                break;
            }
            DbCommand::CreateLog { log, response } => respond!(
                response,
                db_create_log(&connection, log).inspect(|log| cache
                    .write()
                    .expect("database cache poisoned")
                    .publish_log(log.clone(), true))
            ),
            DbCommand::UpdateLog { id, log, response } => respond!(
                response,
                db_update_log(&connection, id, log).inspect(|log| {
                    if let Some(log) = log {
                        cache
                            .write()
                            .expect("database cache poisoned")
                            .publish_log(log.clone(), false);
                    }
                })
            ),
            DbCommand::DeleteLog { id, response } => respond!(
                response,
                db_delete_log(&connection, id).inspect(|deleted| {
                    if *deleted {
                        cache
                            .write()
                            .expect("database cache poisoned")
                            .remove_log(id);
                    }
                })
            ),
            DbCommand::UpdateConfig { config, response } => {
                let password = cache
                    .read()
                    .expect("database cache poisoned")
                    .config
                    .auth
                    .login_password
                    .clone();
                respond!(
                    response,
                    db_update_config(&connection, config, &password).map(|config| {
                        cache.write().expect("database cache poisoned").config = config;
                    })
                );
            }
            DbCommand::CreateRadio { radio, response } => respond!(
                response,
                db_create_radio(&connection, radio).inspect(|radio| {
                    cache
                        .write()
                        .expect("database cache poisoned")
                        .radios
                        .insert(radio.id, radio.clone());
                })
            ),
            DbCommand::UpdateRadio {
                id,
                radio,
                response,
            } => respond!(
                response,
                db_update_radio(&connection, id, radio).inspect(|radio| {
                    if let Some(radio) = radio {
                        cache
                            .write()
                            .expect("database cache poisoned")
                            .radios
                            .insert(radio.id, radio.clone());
                    }
                })
            ),
            DbCommand::DeleteRadio { id, response } => respond!(
                response,
                db_delete_radio(&connection, id).inspect(|deleted| {
                    if *deleted {
                        cache
                            .write()
                            .expect("database cache poisoned")
                            .radios
                            .remove(&id);
                    }
                })
            ),
            DbCommand::UpsertClientRadio {
                client_instance_id,
                radio_ws_url,
                radio,
                response,
            } => {
                let existing = cache
                    .read()
                    .expect("database cache poisoned")
                    .radios
                    .values()
                    .find(|radio| {
                        radio.client_instance_id.as_deref() == Some(client_instance_id.as_str())
                    })
                    .map(|radio| radio.id);
                respond!(
                    response,
                    db_upsert_client_radio(
                        &connection,
                        &client_instance_id,
                        &radio_ws_url,
                        radio,
                        existing
                    )
                    .inspect(|radio| {
                        cache
                            .write()
                            .expect("database cache poisoned")
                            .radios
                            .insert(radio.id, radio.clone());
                    })
                );
            }
            DbCommand::LoadContacts { log_id, response } => respond!(
                response,
                load_contacts(&connection, &cache, &metrics, log_id)
            ),
            DbCommand::UpsertContacts {
                log_id,
                contacts,
                response,
            } => respond!(
                response,
                write_contacts(&mut connection, &cache, &metrics, log_id, contacts)
            ),
            DbCommand::AllocateSerial {
                log_id,
                field_adif,
                response,
            } => respond!(
                response,
                reserve_serial(&mut connection, &cache, &metrics, log_id, field_adif)
            ),
            DbCommand::DeleteContact { id, response } => {
                respond!(response, remove_contact(&connection, &cache, id))
            }
            DbCommand::ContactHasWarnings { id, response } => {
                respond!(response, db_contact_has_warnings(&connection, id))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cw::DEFAULT_CW_MESSAGES;
    use crate::db::config::{DEFAULT_DXCLUSTER_MAX_AGE_MIN, DEFAULT_DXCLUSTER_PORT};
    use crate::db::contact::{
        contact_adif_value, contact_id, contact_log_id, contact_meta_value, set_contact_adif,
        set_contact_meta,
    };
    use crate::db::models::LoginPasswordUpdate;
    use crate::voice_messages::DEFAULT_VOICE_MESSAGES;
    use serde_json::{Map, Value, json};

    fn test_database() -> Database {
        Database::open(":memory:").expect("in-memory database opens")
    }

    #[tokio::test]
    async fn all_warm_and_missing_reads_bypass_sql_and_worker() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let radio = database.create_radio(tcp_radio()).await.unwrap();
        let saved = database
            .upsert_contacts(log.id, vec![base_contact()])
            .await
            .unwrap();
        let id = contact_id(&saved[0]).unwrap();
        let before = database.test_counters();
        for _ in 0..3 {
            assert_eq!(database.logs().await.unwrap().len(), 1);
            assert_eq!(database.log(log.id).await.unwrap().unwrap().id, log.id);
            assert!(database.log(-1).await.unwrap().is_none());
            assert_eq!(database.radios().await.unwrap().len(), 1);
            assert_eq!(
                database.radio(radio.id).await.unwrap().unwrap().id,
                radio.id
            );
            assert!(database.radio(-1).await.unwrap().is_none());
            assert_eq!(database.backend_radio_configs().await.unwrap().len(), 1);
            database.auth_config().await.unwrap();
            database.dxcluster_config().await.unwrap();
            database.iaru_region().await.unwrap();
            database.config_view().await.unwrap();
            assert_eq!(database.contacts(log.id).await.unwrap(), saved);
            assert!(database.contacts(-1).await.unwrap().is_empty());
            assert_eq!(database.contact(id).await.unwrap(), Some(saved[0].clone()));
            assert!(database.contact(-1).await.unwrap().is_none());
            assert_eq!(database.contact_log_id(id).await.unwrap(), Some(log.id));
            assert!(database.contact_log_id(-1).await.unwrap().is_none());
            assert_eq!(database.log_qso_count(log.id).await.unwrap(), 1);
            assert_eq!(database.log_qso_count(-1).await.unwrap(), 0);
            database.log_snapshot(log.id).await.unwrap().unwrap();
            assert!(database.log_snapshot(-1).await.unwrap().is_none());
            assert!(!database.delete_log(-1).await.unwrap());
            assert!(!database.delete_radio(-1).await.unwrap());
            assert!(database.delete_contact(-1).await.unwrap().is_none());
        }
        assert_eq!(database.test_counters(), before);
    }

    #[tokio::test]
    async fn concurrent_cold_loads_are_coalesced_and_empty_logs_need_no_load() {
        let database = test_database();
        let log = create_test_log(&database).await;
        assert!(database.contacts(log.id).await.unwrap().is_empty());
        assert_eq!(database.test_counters(), (1, 0, 0));
        let saved = database
            .upsert_contacts(log.id, vec![base_contact()])
            .await
            .unwrap();
        database.cache.write().unwrap().contacts.remove(&log.id);
        let before = database.test_counters();
        let mut readers = Vec::new();
        for _ in 0..32 {
            let database = database.clone();
            readers.push(tokio::spawn(async move {
                database.contacts(log.id).await.unwrap()
            }));
        }
        for reader in readers {
            assert_eq!(reader.await.unwrap(), saved);
        }
        let after = database.test_counters();
        assert_eq!(after, (before.0 + 1, before.1 + 1, before.2 + 1));
        database.contacts(log.id).await.unwrap();
        assert_eq!(database.test_counters(), after);
    }

    #[tokio::test]
    async fn failed_cold_load_can_be_retried() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let saved = database
            .upsert_contacts(log.id, vec![base_contact()])
            .await
            .unwrap();
        database.cache.write().unwrap().contacts.remove(&log.id);
        database
            .test_control(|connection| {
                connection.execute_batch("ALTER TABLE qsos RENAME TO unavailable_qsos")
            })
            .await
            .unwrap();
        assert!(database.contacts(log.id).await.is_err());
        assert!(
            !database
                .cache
                .read()
                .unwrap()
                .contacts
                .contains_key(&log.id)
        );
        database
            .test_control(|connection| {
                connection.execute_batch("ALTER TABLE unavailable_qsos RENAME TO qsos")
            })
            .await
            .unwrap();
        assert_eq!(database.contacts(log.id).await.unwrap(), saved);
        assert_eq!(database.test_counters().1, 1);
    }

    #[tokio::test]
    async fn failed_batch_is_atomic_and_duplicate_ids_count_once() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let saved = database
            .upsert_contacts(log.id, vec![base_contact()])
            .await
            .unwrap();
        let revision = database.log_revision(log.id);
        let mut invalid = base_contact();
        invalid
            .get_mut("adif")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("CALL");
        assert!(
            database
                .upsert_contacts(log.id, vec![base_contact(), invalid])
                .await
                .is_err()
        );
        assert_eq!(database.contacts(log.id).await.unwrap(), saved);
        assert_eq!(database.log_qso_count(log.id).await.unwrap(), 1);
        assert_eq!(database.log_revision(log.id), revision);
        let mut first = saved[0].clone();
        set_contact_adif(&mut first, "STX", json!(90));
        let mut last = first.clone();
        set_contact_adif(&mut last, "STX", json!(7));
        database
            .upsert_contacts(log.id, vec![first, last])
            .await
            .unwrap();
        assert_eq!(database.log_qso_count(log.id).await.unwrap(), 1);
        let snapshot = database.log_snapshot(log.id).await.unwrap().unwrap();
        assert_eq!(snapshot.contacts.serial_max("STX", None), Some(7));
        assert_eq!(
            database
                .allocate_serial(log.id, "STX".into())
                .await
                .unwrap()
                .serial,
            8
        );
        assert_eq!(database.test_counters().2, 0);
    }

    #[tokio::test]
    async fn serial_indexes_track_updates_deletes_custom_fields_and_overflow() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let mut first = base_contact();
        set_contact_adif(&mut first, "STX", json!(50));
        set_contact_adif(&mut first, "CUSTOM_SERIAL", json!(" 80 "));
        let mut second = base_contact();
        set_contact_adif(&mut second, "STX", json!(12));
        set_contact_adif(&mut second, "BAND", json!("40m"));
        let saved = database
            .upsert_contacts(log.id, vec![first, second])
            .await
            .unwrap();
        let old = database.log_snapshot(log.id).await.unwrap().unwrap();
        assert_eq!(old.contacts.serial_max("stx", Some("20M")), Some(50));
        database
            .delete_contact(contact_id(&saved[0]).unwrap())
            .await
            .unwrap();
        let current = database.log_snapshot(log.id).await.unwrap().unwrap();
        assert_eq!(current.contacts.serial_max("STX", None), Some(12));
        assert_eq!(current.contacts.serial_max("STX", Some("20m")), None);
        assert_eq!(current.contacts.serial_max("CUSTOM_SERIAL", None), None);
        assert_eq!(old.contacts.serial_max("CUSTOM_SERIAL", None), Some(80));
        let mut extreme = saved[1].clone();
        set_contact_adif(&mut extreme, "STX", json!(i64::MAX));
        database
            .upsert_contacts(log.id, vec![extreme])
            .await
            .unwrap();
        assert!(
            database
                .allocate_serial(log.id, "STX".into())
                .await
                .is_err()
        );
        assert!(
            !database
                .cache
                .read()
                .unwrap()
                .next_serial
                .contains_key(&(log.id, "STX".into()))
        );
        assert_eq!(database.test_counters().2, 0);
    }

    #[tokio::test]
    async fn concurrent_reservations_are_unique_and_use_no_selects() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let mut tasks = Vec::new();
        for _ in 0..32 {
            let database = database.clone();
            tasks.push(tokio::spawn(async move {
                database
                    .allocate_serial(log.id, "STX".into())
                    .await
                    .unwrap()
                    .serial
            }));
        }
        let mut serials = Vec::new();
        for task in tasks {
            serials.push(task.await.unwrap());
        }
        serials.sort_unstable();
        assert_eq!(serials, (1..=32).collect::<Vec<_>>());
        assert_eq!(database.test_counters().2, 0);
    }

    #[tokio::test]
    async fn serial_field_casing_shares_reservations_but_preserves_response_field() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let first = database
            .allocate_serial(log.id, " custom_serial ".into())
            .await
            .unwrap();
        assert_eq!(first.serial, 1);
        assert_eq!(first.field_adif, "custom_serial");
        let second = database
            .allocate_serial(log.id, "CUSTOM_SERIAL".into())
            .await
            .unwrap();
        assert_eq!(second.serial, 2);
        assert_eq!(database.test_counters().2, 0);
    }

    #[tokio::test]
    async fn cancelled_queued_write_still_publishes_and_warm_reads_do_not_wait() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let paused_database = database.clone();
        let pause = tokio::spawn(async move {
            paused_database
                .test_control(move |_| {
                    let _ = started_tx.send(());
                    let _ = release_rx.recv();
                    Ok(())
                })
                .await
                .unwrap();
        });
        started_rx.await.unwrap();
        let before = database.test_counters();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            assert_eq!(database.log(log.id).await.unwrap().unwrap().id, log.id);
            database.config_view().await.unwrap();
            database.contacts(log.id).await.unwrap();
        })
        .await
        .expect("warm reads must not wait for the blocked SQLite worker");
        assert_eq!(database.test_counters(), before);
        let writing_database = database.clone();
        let writer = tokio::spawn(async move {
            writing_database
                .upsert_contacts(log.id, vec![base_contact()])
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while database.commands.capacity() == DB_COMMAND_BUFFER {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        writer.abort();
        assert!(writer.await.unwrap_err().is_cancelled());
        release_tx.send(()).unwrap();
        pause.await.unwrap();
        database.test_control(|_| Ok(())).await.unwrap();
        assert_eq!(database.log_qso_count(log.id).await.unwrap(), 1);
        assert_eq!(database.contacts(log.id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn restart_restores_catalog_contacts_and_reservations_and_delete_cascades() {
        let directory =
            std::env::temp_dir().join(format!("log73-cache-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("log73.db");
        let database = Database::open(&path).unwrap();
        let log = create_test_log(&database).await;
        let radio = database.create_radio(tcp_radio()).await.unwrap();
        database.update_config(config_update(3)).await.unwrap();
        let saved = database
            .upsert_contacts(log.id, vec![base_contact()])
            .await
            .unwrap();
        let id = contact_id(&saved[0]).unwrap();
        database
            .allocate_serial(log.id, "CUSTOM_SERIAL".into())
            .await
            .unwrap();
        database.test_close().await;
        let reopened = Database::open(&path).unwrap();
        assert_eq!(reopened.log_qso_count(log.id).await.unwrap(), 1);
        assert_eq!(reopened.contact_log_id(id).await.unwrap(), Some(log.id));
        assert_eq!(reopened.iaru_region().await.unwrap(), 3);
        assert_eq!(
            reopened.radio(radio.id).await.unwrap().unwrap().config,
            radio.config
        );
        assert_eq!(reopened.test_counters(), (0, 0, 0));
        assert_eq!(reopened.contacts(log.id).await.unwrap(), saved);
        assert_eq!(reopened.test_counters(), (1, 1, 1));
        assert_eq!(
            reopened
                .allocate_serial(log.id, "CUSTOM_SERIAL".into())
                .await
                .unwrap()
                .serial,
            2
        );
        assert!(reopened.delete_log(log.id).await.unwrap());
        assert_eq!(reopened.contact_log_id(id).await.unwrap(), None);
        assert!(reopened.contacts(log.id).await.unwrap().is_empty());
        assert_eq!(reopened.log_qso_count(log.id).await.unwrap(), 0);
        assert!(reopened.cache.read().unwrap().next_serial.is_empty());
        let replacement = create_test_log(&reopened).await;
        assert!(reopened.contacts(replacement.id).await.unwrap().is_empty());
        assert_eq!(
            reopened
                .allocate_serial(replacement.id, "CUSTOM_SERIAL".into())
                .await
                .unwrap()
                .serial,
            1
        );
        reopened.test_close().await;
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn file_database_uses_platform_durability_and_persists_across_reopen() {
        let directory =
            std::env::temp_dir().join(format!("log73-wal-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("log73.db");
        {
            let connection = Connection::open(&path).unwrap();
            configure_connection(&connection).unwrap();
            initialize_schema(&connection).unwrap();
            let journal: String = connection
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .unwrap();
            let synchronous: i64 = connection
                .query_row("PRAGMA synchronous", [], |row| row.get(0))
                .unwrap();
            let foreign_keys: i64 = connection
                .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
                .unwrap();
            assert_eq!(journal, "wal");
            assert_eq!(synchronous, if cfg!(target_os = "windows") { 1 } else { 2 });
            assert_eq!(foreign_keys, 1);
            connection.execute("INSERT INTO logs (NAME, CONTEST_ID, STATION_CALLSIGN, CONTEST_PARAMS_JSON) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params!["durability test", "test-contest", "N0CALL", "{}"]).unwrap();
        }
        {
            let connection = Connection::open(&path).unwrap();
            configure_connection(&connection).unwrap();
            let name: String = connection
                .query_row("SELECT NAME FROM logs", [], |row| row.get(0))
                .unwrap();
            assert_eq!(name, "durability test");
            let integrity: String = connection
                .query_row("PRAGMA integrity_check", [], |row| row.get(0))
                .unwrap();
            assert_eq!(integrity, "ok");
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn in_memory_database_retains_supported_journal_mode() {
        let connection = Connection::open_in_memory().unwrap();
        configure_connection(&connection).unwrap();
        let journal: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal, "memory");
        initialize_schema(&connection).unwrap();
    }

    fn config_update(iaru_region: i64) -> UpdateConfig {
        UpdateConfig {
            iaru_region,
            login_user: String::new(),
            login_password: LoginPasswordUpdate::Preserve,
            dxcluster_enabled: false,
            dxcluster_host: String::new(),
            dxcluster_port: DEFAULT_DXCLUSTER_PORT,
            dxcluster_callsign: String::new(),
            dxcluster_max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
            dxcluster_commands: String::new(),
        }
    }

    #[tokio::test]
    async fn iaru_region_defaults_to_region_two_and_round_trips_valid_values() {
        let database = test_database();
        assert_eq!(
            database.iaru_region().await.expect("region loads"),
            crate::db::DEFAULT_IARU_REGION
        );
        assert_eq!(
            database
                .config_view()
                .await
                .expect("config view loads")
                .iaru_region,
            crate::db::DEFAULT_IARU_REGION
        );

        for region in 1..=3 {
            database
                .update_config(config_update(region))
                .await
                .expect("valid region is stored");
            assert_eq!(database.iaru_region().await.expect("region loads"), region);
            assert_eq!(
                database
                    .config_view()
                    .await
                    .expect("config view loads")
                    .iaru_region,
                region
            );
        }
    }

    #[tokio::test]
    async fn invalid_iaru_regions_are_rejected_without_changing_the_region() {
        let database = test_database();
        database
            .update_config(config_update(1))
            .await
            .expect("initial region is stored");

        for region in [0, 4, -1] {
            assert!(database.update_config(config_update(region)).await.is_err());
            assert_eq!(database.iaru_region().await.expect("region loads"), 1);
        }
    }

    #[test]
    fn fresh_schema_has_configured_region_and_no_bands_table() {
        let connection = Connection::open_in_memory().expect("in-memory connection opens");
        initialize_schema(&connection).expect("schema initializes");
        let region: i64 = connection
            .query_row("SELECT IARU_REGION FROM config", [], |row| row.get(0))
            .expect("configured region exists");
        let bands_table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'bands'",
                [],
                |row| row.get(0),
            )
            .expect("schema tables can be inspected");

        assert_eq!(region, crate::db::DEFAULT_IARU_REGION);
        assert_eq!(bands_table_count, 0);
    }

    async fn create_test_log(database: &Database) -> Log {
        database
            .create_log(NewLog {
                name: "Test log".to_string(),
                contest_id: "test-contest".to_string(),
                station_callsign: "N0CALL".to_string(),
                contest_params: Value::Object(Map::new()),
            })
            .await
            .expect("test log is created")
    }

    #[tokio::test]
    async fn dxcluster_config_defaults_and_updates() {
        let database = test_database();

        let defaults = database
            .dxcluster_config()
            .await
            .expect("dxcluster config loads");
        assert!(!defaults.enabled);
        assert_eq!(defaults.port, DEFAULT_DXCLUSTER_PORT);
        assert_eq!(defaults.max_age_min, DEFAULT_DXCLUSTER_MAX_AGE_MIN);

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "greg".to_string(),
                login_password: LoginPasswordUpdate::Set("hash".to_string()),
                dxcluster_enabled: true,
                dxcluster_host: "cluster.example.test".to_string(),
                dxcluster_port: 7300,
                dxcluster_callsign: "n0call".to_string(),
                dxcluster_max_age_min: 120,
                dxcluster_commands: "set/page 0\nsh/dx".to_string(),
            })
            .await
            .expect("config updates");

        let config = database.config_view().await.expect("config view loads");
        assert!(config.login_enabled);
        assert!(config.dxcluster_enabled);
        assert_eq!(config.dxcluster_host, "cluster.example.test");
        assert_eq!(config.dxcluster_port, 7300);
        assert_eq!(config.dxcluster_callsign, "N0CALL");
        assert_eq!(config.dxcluster_max_age_min, 120);
        assert_eq!(config.dxcluster_commands, "set/page 0\nsh/dx");
    }

    #[tokio::test]
    async fn username_only_update_preserves_existing_password_hash() {
        let database = test_database();
        let original_hash =
            crate::auth::hash_password("secret").expect("password hash should be generated");

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "greg".to_string(),
                login_password: LoginPasswordUpdate::Set(original_hash.clone()),
                dxcluster_enabled: false,
                dxcluster_host: String::new(),
                dxcluster_port: DEFAULT_DXCLUSTER_PORT,
                dxcluster_callsign: String::new(),
                dxcluster_max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
                dxcluster_commands: String::new(),
            })
            .await
            .expect("initial config updates");

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "gregory".to_string(),
                login_password: LoginPasswordUpdate::Preserve,
                dxcluster_enabled: true,
                dxcluster_host: "cluster.example.test".to_string(),
                dxcluster_port: 7373,
                dxcluster_callsign: "n0call".to_string(),
                dxcluster_max_age_min: 90,
                dxcluster_commands: "show/dx".to_string(),
            })
            .await
            .expect("config updates preserve password");

        let auth = database.auth_config().await.expect("auth config loads");
        assert_eq!(auth.login_user, "gregory");
        assert_eq!(auth.login_password, original_hash);
    }

    #[tokio::test]
    async fn blank_password_update_without_explicit_disable_preserves_hash() {
        let database = test_database();
        let original_hash =
            crate::auth::hash_password("secret").expect("password hash should be generated");

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "greg".to_string(),
                login_password: LoginPasswordUpdate::Set(original_hash.clone()),
                dxcluster_enabled: false,
                dxcluster_host: String::new(),
                dxcluster_port: DEFAULT_DXCLUSTER_PORT,
                dxcluster_callsign: String::new(),
                dxcluster_max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
                dxcluster_commands: String::new(),
            })
            .await
            .expect("initial config updates");

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "greg".to_string(),
                login_password: LoginPasswordUpdate::Preserve,
                dxcluster_enabled: true,
                dxcluster_host: "cluster.example.test".to_string(),
                dxcluster_port: 7300,
                dxcluster_callsign: "n0call".to_string(),
                dxcluster_max_age_min: 120,
                dxcluster_commands: String::new(),
            })
            .await
            .expect("config updates preserve password");

        let auth = database.auth_config().await.expect("auth config loads");
        assert_eq!(auth.login_password, original_hash);
    }

    #[tokio::test]
    async fn password_change_updates_hash_and_keeps_login_enabled() {
        let database = test_database();
        let original_hash =
            crate::auth::hash_password("secret").expect("password hash should be generated");
        let replacement_hash =
            crate::auth::hash_password("new-secret").expect("password hash should be generated");

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "greg".to_string(),
                login_password: LoginPasswordUpdate::Set(original_hash.clone()),
                dxcluster_enabled: false,
                dxcluster_host: String::new(),
                dxcluster_port: DEFAULT_DXCLUSTER_PORT,
                dxcluster_callsign: String::new(),
                dxcluster_max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
                dxcluster_commands: String::new(),
            })
            .await
            .expect("initial config updates");

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "greg".to_string(),
                login_password: LoginPasswordUpdate::Set(replacement_hash.clone()),
                dxcluster_enabled: false,
                dxcluster_host: String::new(),
                dxcluster_port: DEFAULT_DXCLUSTER_PORT,
                dxcluster_callsign: String::new(),
                dxcluster_max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
                dxcluster_commands: String::new(),
            })
            .await
            .expect("config updates password");

        let auth = database.auth_config().await.expect("auth config loads");
        let view = database.config_view().await.expect("config view loads");

        assert_ne!(auth.login_password, original_hash);
        assert_eq!(auth.login_password, replacement_hash);
        assert!(view.login_enabled);
    }

    #[tokio::test]
    async fn explicit_disable_clears_password_and_disables_login() {
        let database = test_database();
        let original_hash =
            crate::auth::hash_password("secret").expect("password hash should be generated");

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "greg".to_string(),
                login_password: LoginPasswordUpdate::Set(original_hash),
                dxcluster_enabled: false,
                dxcluster_host: String::new(),
                dxcluster_port: DEFAULT_DXCLUSTER_PORT,
                dxcluster_callsign: String::new(),
                dxcluster_max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
                dxcluster_commands: String::new(),
            })
            .await
            .expect("initial config updates");

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "greg".to_string(),
                login_password: LoginPasswordUpdate::Disable,
                dxcluster_enabled: false,
                dxcluster_host: String::new(),
                dxcluster_port: DEFAULT_DXCLUSTER_PORT,
                dxcluster_callsign: String::new(),
                dxcluster_max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
                dxcluster_commands: String::new(),
            })
            .await
            .expect("config disables auth");

        let auth = database.auth_config().await.expect("auth config loads");
        let view = database.config_view().await.expect("config view loads");

        assert_eq!(auth.login_password, "");
        assert!(!view.login_enabled);
    }

    #[tokio::test]
    async fn config_view_login_enabled_reflects_preserve_change_and_disable() {
        let database = test_database();
        let original_hash =
            crate::auth::hash_password("secret").expect("password hash should be generated");
        let replacement_hash =
            crate::auth::hash_password("new-secret").expect("password hash should be generated");

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "greg".to_string(),
                login_password: LoginPasswordUpdate::Set(original_hash.clone()),
                dxcluster_enabled: false,
                dxcluster_host: String::new(),
                dxcluster_port: DEFAULT_DXCLUSTER_PORT,
                dxcluster_callsign: String::new(),
                dxcluster_max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
                dxcluster_commands: String::new(),
            })
            .await
            .expect("initial config updates");
        assert!(
            database
                .config_view()
                .await
                .expect("config view loads")
                .login_enabled
        );

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "gregory".to_string(),
                login_password: LoginPasswordUpdate::Preserve,
                dxcluster_enabled: false,
                dxcluster_host: String::new(),
                dxcluster_port: DEFAULT_DXCLUSTER_PORT,
                dxcluster_callsign: String::new(),
                dxcluster_max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
                dxcluster_commands: String::new(),
            })
            .await
            .expect("config preserves password");
        assert!(
            database
                .config_view()
                .await
                .expect("config view loads")
                .login_enabled
        );

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "gregory".to_string(),
                login_password: LoginPasswordUpdate::Set(replacement_hash),
                dxcluster_enabled: false,
                dxcluster_host: String::new(),
                dxcluster_port: DEFAULT_DXCLUSTER_PORT,
                dxcluster_callsign: String::new(),
                dxcluster_max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
                dxcluster_commands: String::new(),
            })
            .await
            .expect("config changes password");
        assert!(
            database
                .config_view()
                .await
                .expect("config view loads")
                .login_enabled
        );

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "gregory".to_string(),
                login_password: LoginPasswordUpdate::Disable,
                dxcluster_enabled: false,
                dxcluster_host: String::new(),
                dxcluster_port: DEFAULT_DXCLUSTER_PORT,
                dxcluster_callsign: String::new(),
                dxcluster_max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
                dxcluster_commands: String::new(),
            })
            .await
            .expect("config disables auth");
        assert!(
            !database
                .config_view()
                .await
                .expect("config view loads")
                .login_enabled
        );
    }

    #[tokio::test]
    async fn auth_still_succeeds_after_non_auth_config_save() {
        let database = test_database();
        let original_hash =
            crate::auth::hash_password("secret").expect("password hash should be generated");

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "greg".to_string(),
                login_password: LoginPasswordUpdate::Set(original_hash),
                dxcluster_enabled: false,
                dxcluster_host: String::new(),
                dxcluster_port: DEFAULT_DXCLUSTER_PORT,
                dxcluster_callsign: String::new(),
                dxcluster_max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
                dxcluster_commands: String::new(),
            })
            .await
            .expect("initial config updates");

        database
            .update_config(UpdateConfig {
                iaru_region: crate::db::DEFAULT_IARU_REGION,
                login_user: "greg".to_string(),
                login_password: LoginPasswordUpdate::Preserve,
                dxcluster_enabled: true,
                dxcluster_host: "cluster.example.test".to_string(),
                dxcluster_port: 7300,
                dxcluster_callsign: "n0call".to_string(),
                dxcluster_max_age_min: 120,
                dxcluster_commands: "show/dx".to_string(),
            })
            .await
            .expect("non-auth config updates");

        let auth = database.auth_config().await.expect("auth config loads");
        assert!(crate::auth::verify_password_hash(
            "secret",
            &auth.login_password
        ));
    }

    fn tcp_radio() -> RadioPayload {
        RadioPayload {
            name: "Elecraft TCP".to_string(),
            radio_kind: "elecraft-k4".to_string(),
            transport_kind: "tcp".to_string(),
            tcp_host: "127.0.0.1".to_string(),
            tcp_port: 5002,
            serial_port: String::new(),
            serial_baud_rate: 115_200,
            options: String::new(),
            data_mode: "DATA-USB".to_string(),
            rtty_mode: "RTTY".to_string(),
            digital_program: "none".to_string(),
            wsjtx_enabled: false,
            wsjtx_bind_address: "127.0.0.1".to_string(),
            wsjtx_port: 2237,
            wsjtx_multicast_group: String::new(),
            fldigi_data_enabled: false,
            fldigi_rtty_enabled: false,
            fldigi_host: radio_io::DEFAULT_FLDIGI_HOST.to_string(),
            fldigi_port: radio_io::DEFAULT_FLDIGI_PORT,
            flrig_enabled: false,
            flrig_port: radio_io::DEFAULT_FLRIG_PORT,
            cw_tuning_increment_hz: crate::db::DEFAULT_CW_TUNING_INCREMENT_HZ,
            ssb_tuning_increment_hz: crate::db::DEFAULT_SSB_TUNING_INCREMENT_HZ,
            rit_clear_on_log: false,
            voice_input_device_id: None,
            voice_output_device_id: None,
            cw_keyer_type: "none".to_string(),
            winkeyer_serial_port: String::new(),
            cw_serial_port: String::new(),
            cw_serial_baud_rate: 9_600,
            cw_serial_line: "dtr".to_string(),
            cw_messages: DEFAULT_CW_MESSAGES.to_string(),
            digital_messages: radio_io::digital_messages::DEFAULT_DIGITAL_MESSAGES.to_string(),
            voice_messages: DEFAULT_VOICE_MESSAGES.to_string(),
        }
    }

    fn base_contact() -> Contact {
        crate::db::build_contact(
            Map::new(),
            Map::from_iter([
                ("QSO_DATE_TIME_ON".to_string(), json!(1_700_000_000_i64)),
                ("STATION_CALLSIGN".to_string(), json!("N0CALL")),
                ("CONTEST_ID".to_string(), json!("test-contest")),
                ("CALL".to_string(), json!("K1ABC")),
                ("BAND".to_string(), json!("20m")),
                ("FREQ".to_string(), json!(14_074_000_i64)),
                ("MODE".to_string(), json!("FT8")),
            ]),
        )
    }

    #[tokio::test]
    async fn diagnostic_warnings_round_trip_through_storage() {
        let database = test_database();
        let log = create_test_log(&database).await;

        let mut warned = base_contact();
        crate::db::set_contact_meta(
            &mut warned,
            "warnings",
            json!([{"message": "serial is required"}]),
        );
        let saved = database
            .upsert_contacts(log.id, vec![warned])
            .await
            .unwrap();
        let id = contact_id(&saved[0]).unwrap();
        assert!(database.contact_has_warnings(id).await.unwrap());

        let reloaded = database.contact(id).await.unwrap().unwrap();
        assert_eq!(
            contact_meta_value(&reloaded, "warnings"),
            Some(&json!([{"message": "serial is required"}]))
        );

        // Clearing warnings persists the cleared state.
        let mut cleared = reloaded;
        crate::db::set_contact_meta(&mut cleared, "warnings", json!([]));
        database
            .upsert_contacts(log.id, vec![cleared])
            .await
            .unwrap();
        assert!(!database.contact_has_warnings(id).await.unwrap());
        let reloaded = database.contact(id).await.unwrap().unwrap();
        assert_eq!(contact_meta_value(&reloaded, "warnings"), None);

        // Contacts without warnings read back without the key.
        let plain = database
            .upsert_contacts(log.id, vec![base_contact()])
            .await
            .unwrap();
        let plain_id = contact_id(&plain[0]).unwrap();
        assert!(!database.contact_has_warnings(plain_id).await.unwrap());
        assert!(!database.contact_has_warnings(-1).await.unwrap());
    }

    #[tokio::test]
    async fn upsert_contacts_inserts_contact() {
        let database = test_database();
        let log = create_test_log(&database).await;

        let saved = database
            .upsert_contacts(log.id, vec![base_contact()])
            .await
            .expect("contact is inserted");

        assert_eq!(saved.len(), 1);
        assert!(contact_id(&saved[0]).is_some());
        assert_eq!(contact_log_id(&saved[0]), Some(log.id));
        assert_eq!(
            contact_adif_value(&saved[0], "CALL").and_then(Value::as_str),
            Some("K1ABC")
        );

        let contacts = database
            .contacts(log.id)
            .await
            .expect("contacts are listed");
        assert_eq!(contacts.len(), 1);
    }

    #[tokio::test]
    async fn upsert_contacts_persists_contest_id_in_qso_column() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let mut contact = base_contact();
        set_contact_adif(&mut contact, "CONTEST_ID", json!("test-contest"));

        let saved = database
            .upsert_contacts(log.id, vec![contact])
            .await
            .expect("contact is inserted");

        assert_eq!(
            contact_adif_value(&saved[0], "CONTEST_ID").and_then(Value::as_str),
            Some("test-contest")
        );

        let contacts = database
            .contacts(log.id)
            .await
            .expect("contacts are listed");
        assert_eq!(
            contact_adif_value(&contacts[0], "CONTEST_ID").and_then(Value::as_str),
            Some("test-contest")
        );
    }

    #[tokio::test]
    async fn upsert_contacts_round_trips_geography_columns() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let mut contact = base_contact();
        set_contact_adif(&mut contact, "CONT", json!("EU"));
        set_contact_adif(&mut contact, "MY_DXCC", json!(291));
        set_contact_adif(&mut contact, "MY_CONT", json!("NA"));
        set_contact_adif(&mut contact, "APP_LOG73_DXCC_PFX", json!("F"));
        set_contact_adif(&mut contact, "APP_LOG73_MY_DXCC_PFX", json!("K"));
        set_contact_adif(&mut contact, "PFX", json!("F1"));

        let saved = database
            .upsert_contacts(log.id, vec![contact])
            .await
            .expect("contact is inserted");

        assert_eq!(contact_adif_value(&saved[0], "CONT"), Some(&json!("EU")));
        assert_eq!(contact_adif_value(&saved[0], "MY_DXCC"), Some(&json!(291)));
        assert_eq!(contact_adif_value(&saved[0], "MY_CONT"), Some(&json!("NA")));
        assert_eq!(
            contact_adif_value(&saved[0], "APP_LOG73_DXCC_PFX"),
            Some(&json!("F"))
        );
        assert_eq!(
            contact_adif_value(&saved[0], "APP_LOG73_MY_DXCC_PFX"),
            Some(&json!("K"))
        );
        assert_eq!(contact_adif_value(&saved[0], "PFX"), Some(&json!("F1")));
    }

    #[tokio::test]
    async fn upsert_contacts_fills_missing_contest_id_from_log() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let mut contact = base_contact();
        set_contact_adif(&mut contact, "CONTEST_ID", json!(""));

        let saved = database
            .upsert_contacts(log.id, vec![contact])
            .await
            .expect("contact is inserted");

        assert_eq!(
            contact_adif_value(&saved[0], "CONTEST_ID").and_then(Value::as_str),
            Some("test-contest")
        );
    }

    #[tokio::test]
    async fn allocate_serial_reserves_one_value_by_log_and_field() {
        let database = test_database();
        let first_log = create_test_log(&database).await;
        let second_log = database
            .create_log(NewLog {
                name: "Second log".to_string(),
                contest_id: "test-contest".to_string(),
                station_callsign: "N0CALL".to_string(),
                contest_params: Value::Object(Map::new()),
            })
            .await
            .expect("second log is created");

        let first = database
            .allocate_serial(first_log.id, "STX".to_string())
            .await
            .expect("serial is allocated");
        let second = database
            .allocate_serial(first_log.id, "STX".to_string())
            .await
            .expect("next serial is allocated");
        let other_log = database
            .allocate_serial(second_log.id, "STX".to_string())
            .await
            .expect("other log serial is allocated");
        let other_field = database
            .allocate_serial(first_log.id, "CUSTOM_SERIAL".to_string())
            .await
            .expect("other field serial is allocated");

        assert_eq!(first.serial, 1);
        assert_eq!(second.serial, 2);
        assert_eq!(other_log.serial, 1);
        assert_eq!(other_field.serial, 1);
    }

    #[tokio::test]
    async fn allocate_serial_starts_after_committed_column_or_json_serials() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let mut stx_contact = base_contact();
        set_contact_adif(&mut stx_contact, "STX", json!(42));
        let mut json_contact = base_contact();
        set_contact_adif(&mut json_contact, "CALL", json!("K1ABD"));
        set_contact_adif(&mut json_contact, "CUSTOM_SERIAL", json!(77));
        database
            .upsert_contacts(log.id, vec![stx_contact, json_contact])
            .await
            .expect("contacts are inserted");

        let stx = database
            .allocate_serial(log.id, "STX".to_string())
            .await
            .expect("STX serial is allocated");
        let custom = database
            .allocate_serial(log.id, "CUSTOM_SERIAL".to_string())
            .await
            .expect("custom serial is allocated");

        assert_eq!(stx.serial, 43);
        assert_eq!(custom.serial, 78);
    }

    #[tokio::test]
    async fn upsert_contacts_updates_existing_contact() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let inserted = database
            .upsert_contacts(log.id, vec![base_contact()])
            .await
            .expect("contact is inserted");
        let saved_contact_id = contact_id(&inserted[0]).expect("inserted contact has an id");

        let mut updated_contact = base_contact();
        set_contact_meta(&mut updated_contact, "id", json!(saved_contact_id));
        set_contact_adif(&mut updated_contact, "CALL", json!("W9XYZ"));
        set_contact_adif(&mut updated_contact, "COMMENT", json!("updated"));

        let updated = database
            .upsert_contacts(log.id, vec![updated_contact])
            .await
            .expect("contact is updated");

        assert_eq!(updated.len(), 1);
        assert_eq!(contact_id(&updated[0]), Some(saved_contact_id));
        assert_eq!(
            contact_adif_value(&updated[0], "CALL").and_then(Value::as_str),
            Some("W9XYZ")
        );
        assert_eq!(
            contact_adif_value(&updated[0], "COMMENT").and_then(Value::as_str),
            Some("updated")
        );

        let contacts = database
            .contacts(log.id)
            .await
            .expect("contacts are listed");
        assert_eq!(contacts.len(), 1);
        assert_eq!(
            contact_adif_value(&contacts[0], "CALL").and_then(Value::as_str),
            Some("W9XYZ")
        );
    }

    #[tokio::test]
    async fn upsert_contacts_treats_sql_like_values_as_data() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let mut contact = base_contact();
        let sql_like_call = "K1ABC'); DROP TABLE logs; --";
        set_contact_adif(&mut contact, "CALL", json!(sql_like_call));

        let saved = database
            .upsert_contacts(log.id, vec![contact])
            .await
            .expect("contact with sql-like value is inserted");

        assert_eq!(
            contact_adif_value(&saved[0], "CALL").and_then(Value::as_str),
            Some(sql_like_call)
        );
        assert_eq!(
            database
                .logs()
                .await
                .expect("logs table still exists")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn upsert_contacts_rejects_existing_contact_id_from_different_log() {
        let database = test_database();
        let first_log = create_test_log(&database).await;
        let second_log = database
            .create_log(NewLog {
                name: "Second test log".to_string(),
                contest_id: "test-contest".to_string(),
                station_callsign: "N0CALL".to_string(),
                contest_params: Value::Object(Map::new()),
            })
            .await
            .expect("second test log is created");
        let inserted = database
            .upsert_contacts(first_log.id, vec![base_contact()])
            .await
            .expect("contact is inserted");
        let contact_id = contact_id(&inserted[0]).expect("inserted contact has an id");

        let mut attempted_update = base_contact();
        set_contact_meta(&mut attempted_update, "id", json!(contact_id));
        set_contact_adif(&mut attempted_update, "CALL", json!("W9XYZ"));

        let error = database
            .upsert_contacts(second_log.id, vec![attempted_update])
            .await
            .expect_err("cross-log overwrite should be rejected");
        assert!(error.to_string().contains("belongs to log"));

        let first_log_contacts = database
            .contacts(first_log.id)
            .await
            .expect("first-log contacts are listed");
        assert_eq!(first_log_contacts.len(), 1);
        assert_eq!(
            contact_adif_value(&first_log_contacts[0], "CALL").and_then(Value::as_str),
            Some("K1ABC")
        );

        let second_log_contacts = database
            .contacts(second_log.id)
            .await
            .expect("second-log contacts are listed");
        assert!(second_log_contacts.is_empty());
    }

    #[tokio::test]
    async fn update_log_updates_contest_params() {
        let database = test_database();
        let log = create_test_log(&database).await;

        let updated = database
            .update_log(
                log.id,
                UpdateLog {
                    name: "Updated log".to_string(),
                    station_callsign: "K4ABC".to_string(),
                    contest_params: json!({
                        "CATEGORY-MODE": "MIXED",
                        "NAME": "Greg"
                    }),
                },
            )
            .await
            .expect("log update succeeds")
            .expect("log should exist");

        assert_eq!(updated.name, "Updated log");
        assert_eq!(updated.station_callsign, "K4ABC");
        assert_eq!(
            updated
                .contest_params
                .get("CATEGORY-MODE")
                .and_then(Value::as_str),
            Some("MIXED")
        );
        assert_eq!(
            updated.contest_params.get("NAME").and_then(Value::as_str),
            Some("Greg")
        );
    }

    #[tokio::test]
    async fn log_qso_count_returns_committed_qso_total() {
        let database = test_database();
        let log = create_test_log(&database).await;

        database
            .upsert_contacts(log.id, vec![base_contact(), base_contact()])
            .await
            .expect("contacts are inserted");

        let qso_count = database
            .log_qso_count(log.id)
            .await
            .expect("qso count loads");

        assert_eq!(qso_count, 2);
    }

    #[tokio::test]
    async fn delete_log_removes_populated_log_and_cascades_qsos() {
        let database = test_database();
        let log = create_test_log(&database).await;

        database
            .upsert_contacts(log.id, vec![base_contact()])
            .await
            .expect("contact is inserted");

        let deleted = database
            .delete_log(log.id)
            .await
            .expect("log delete succeeds");
        assert!(deleted);

        let log_after_delete = database.log(log.id).await.expect("log lookup succeeds");
        assert!(log_after_delete.is_none());

        let qso_count = database
            .log_qso_count(log.id)
            .await
            .expect("qso count loads after delete");
        assert_eq!(qso_count, 0);
    }

    #[tokio::test]
    async fn create_radio_persists_transport_specific_fields() {
        let database = test_database();

        let radio = database
            .create_radio(tcp_radio())
            .await
            .expect("radio is created");

        assert_eq!(radio.radio_kind, "elecraft-k4");
        assert_eq!(radio.transport_kind, "tcp");
        assert_eq!(radio.tcp_host, "127.0.0.1");
        assert_eq!(radio.tcp_port, 5002);
        assert_eq!(radio.serial_port, "");
        assert_eq!(radio.serial_baud_rate, 115_200);
        assert_eq!(radio.options, "");
        assert_eq!(radio.data_mode, "DATA-USB");
        assert_eq!(radio.rtty_mode, "RTTY");
        assert!(!radio.wsjtx_enabled);
        assert_eq!(radio.wsjtx_bind_address, "127.0.0.1");
        assert_eq!(radio.wsjtx_port, 2237);
        assert_eq!(radio.wsjtx_multicast_group, "");
        assert!(!radio.flrig_enabled);
        assert_eq!(radio.flrig_port, radio_io::DEFAULT_FLRIG_PORT);
        assert_eq!(
            radio.cw_tuning_increment_hz,
            crate::db::DEFAULT_CW_TUNING_INCREMENT_HZ
        );
        assert_eq!(
            radio.ssb_tuning_increment_hz,
            crate::db::DEFAULT_SSB_TUNING_INCREMENT_HZ
        );
        assert!(!radio.rit_clear_on_log);
        assert_eq!(radio.voice_input_device_id, None);
        assert_eq!(radio.voice_output_device_id, None);
        assert_eq!(radio.cw_keyer_type, "none");
        assert_eq!(radio.cw_serial_port, "");
        assert_eq!(radio.cw_serial_baud_rate, 9_600);
        assert_eq!(radio.cw_serial_line, "dtr");
        assert_eq!(
            radio.digital_messages,
            radio_io::digital_messages::DEFAULT_DIGITAL_MESSAGES
        );
        assert_eq!(radio.voice_messages, DEFAULT_VOICE_MESSAGES);
        assert_eq!(
            radio.control_location,
            crate::db::RadioControlLocation::Backend
        );
        assert_eq!(radio.client_instance_id, None);
        assert_eq!(radio.radio_ws_url, None);
    }

    #[tokio::test]
    async fn client_radio_registration_upsert_reuses_id_and_replaces_snapshot() {
        let database = test_database();
        let client_id = "b55168f4-5d76-4eed-a17f-67b42167ac42";
        let first = database
            .upsert_client_radio(
                client_id.to_string(),
                "ws://127.0.0.1:49152/radiows".to_string(),
                tcp_radio(),
            )
            .await
            .expect("client radio is registered");

        let mut replacement = tcp_radio();
        replacement.name = "Updated client radio".to_string();
        replacement.digital_program = "wsjtx".to_string();
        replacement.wsjtx_enabled = true;
        replacement.flrig_enabled = true;
        replacement.flrig_port = 23_456;
        let second = database
            .upsert_client_radio(
                client_id.to_string(),
                "ws://127.0.0.1:49153/radiows".to_string(),
                replacement,
            )
            .await
            .expect("client radio is re-registered");

        assert_eq!(first.id, second.id);
        assert_eq!(
            second.control_location,
            crate::db::RadioControlLocation::Client
        );
        assert_eq!(second.client_instance_id.as_deref(), Some(client_id));
        assert_eq!(
            second.radio_ws_url.as_deref(),
            Some("ws://127.0.0.1:49153/radiows")
        );
        assert_eq!(second.name, "Updated client radio");
        assert!(second.wsjtx_enabled);
        assert!(second.flrig_enabled);
        assert_eq!(second.flrig_port, 23_456);
    }

    #[tokio::test]
    async fn client_radios_do_not_reserve_local_service_ports_or_load_into_backend_runtime() {
        let database = test_database();
        let mut backend_radio = tcp_radio();
        backend_radio.digital_program = "wsjtx".to_string();
        backend_radio.wsjtx_enabled = true;
        backend_radio.flrig_enabled = true;
        let backend = database
            .create_radio(backend_radio)
            .await
            .expect("backend radio is created");

        let mut client_radio = tcp_radio();
        client_radio.digital_program = "wsjtx".to_string();
        client_radio.wsjtx_enabled = true;
        client_radio.flrig_enabled = true;
        let client = database
            .upsert_client_radio(
                "7bea2122-503a-48ef-9eb5-8da4f5802b81".to_string(),
                "ws://127.0.0.1:49152/radiows".to_string(),
                client_radio,
            )
            .await
            .expect("client radio may share the backend port");

        let runtime_configs = database
            .backend_radio_configs()
            .await
            .expect("backend runtime configs load");
        assert_eq!(runtime_configs.len(), 1);
        assert_eq!(runtime_configs[0].id, backend.id);
        assert_ne!(runtime_configs[0].id, client.id);
    }

    #[tokio::test]
    async fn enabled_wsjtx_ports_are_unique_but_disabled_ports_are_not_reserved() {
        let database = test_database();
        let mut first = tcp_radio();
        first.digital_program = "wsjtx".to_string();
        first.wsjtx_enabled = true;
        database
            .create_radio(first)
            .await
            .expect("first enabled WSJT-X port is accepted");

        let mut duplicate = tcp_radio();
        duplicate.name = "Duplicate".to_string();
        duplicate.digital_program = "wsjtx".to_string();
        duplicate.wsjtx_enabled = true;
        assert!(database.create_radio(duplicate).await.is_err());

        let mut disabled = tcp_radio();
        disabled.name = "Disabled".to_string();
        database
            .create_radio(disabled)
            .await
            .expect("disabled radio does not reserve its WSJT-X port");
    }

    #[tokio::test]
    async fn enabled_flrig_ports_are_unique_but_disabled_ports_are_not_reserved() {
        let database = test_database();
        let mut first = tcp_radio();
        first.flrig_enabled = true;
        database
            .create_radio(first)
            .await
            .expect("first enabled FLRig port is accepted");

        let mut duplicate = tcp_radio();
        duplicate.name = "Duplicate".to_string();
        duplicate.flrig_enabled = true;
        assert!(database.create_radio(duplicate).await.is_err());

        let mut disabled = tcp_radio();
        disabled.name = "Disabled".to_string();
        database
            .create_radio(disabled)
            .await
            .expect("disabled radio does not reserve its FLRig port");
    }

    #[tokio::test]
    async fn create_radio_persists_optional_voice_device_ids() {
        let database = test_database();
        let mut new_radio = tcp_radio();
        new_radio.voice_input_device_id = Some("alsa:hw:1,0".to_string());
        new_radio.voice_output_device_id = Some("wasapi:{output-device}".to_string());

        let radio = database
            .create_radio(new_radio)
            .await
            .expect("radio is created");
        let listed = database.radios().await.expect("radios list");
        let selected = database
            .radio(radio.id)
            .await
            .expect("radio loads")
            .expect("radio exists");

        assert_eq!(radio.voice_input_device_id.as_deref(), Some("alsa:hw:1,0"));
        assert_eq!(
            radio.voice_output_device_id.as_deref(),
            Some("wasapi:{output-device}")
        );
        assert_eq!(listed[0].voice_input_device_id, radio.voice_input_device_id);
        assert_eq!(
            selected.voice_output_device_id,
            radio.voice_output_device_id
        );
    }

    #[tokio::test]
    async fn update_radio_can_change_and_clear_voice_device_ids() {
        let database = test_database();
        let mut new_radio = tcp_radio();
        new_radio.voice_input_device_id = Some("alsa:mic-1".to_string());
        new_radio.voice_output_device_id = Some("alsa:out-1".to_string());
        let radio = database
            .create_radio(new_radio)
            .await
            .expect("radio is created");

        let mut update = tcp_radio();
        update.name = "Updated".to_string();
        update.voice_input_device_id = Some("   ".to_string());
        update.voice_output_device_id = Some("alsa:out-2".to_string());
        let updated = database
            .update_radio(radio.id, update)
            .await
            .expect("radio updates")
            .expect("radio exists");

        assert_eq!(updated.name, "Updated");
        assert_eq!(updated.voice_input_device_id, None);
        assert_eq!(
            updated.voice_output_device_id.as_deref(),
            Some("alsa:out-2")
        );
    }

    #[tokio::test]
    async fn update_radio_rejects_client_owned_records() {
        let database = test_database();
        let client = database
            .upsert_client_radio(
                "1e2d3c4b-5a69-4870-91b2-c3d4e5f60718".to_string(),
                "ws://127.0.0.1:49152/radiows".to_string(),
                tcp_radio(),
            )
            .await
            .expect("client radio is registered");

        let mut update = tcp_radio();
        update.name = "Attempted backend edit".to_string();
        assert!(
            database
                .update_radio(client.id, update)
                .await
                .expect("client radio update query runs")
                .is_none()
        );

        let persisted = database
            .radio(client.id)
            .await
            .expect("client radio loads")
            .expect("client radio remains");
        assert_eq!(persisted.name, client.name);
    }
}
