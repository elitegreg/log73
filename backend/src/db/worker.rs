use super::config::{db_auth_config, db_dxcluster_config, db_iaru_region, db_update_config};
use super::contact::Contact;
use super::contacts::{
    db_contacts, db_delete_contact, db_upsert_contacts, select_contact, select_contact_log_id,
};
use super::logs::{
    db_create_log, db_delete_log, db_log_qso_count, db_logs, db_update_log, select_log,
};
use super::models::{
    AuthConfig, ConfigView, DxClusterConfig, Log, NewLog, RadioPayload, RadioRecord,
    SerialAllocation, UpdateConfig, UpdateLog,
};
use super::radios::{
    db_backend_radio_configs, db_create_radio, db_delete_radio, db_radios, db_update_radio,
    db_upsert_client_radio, select_radio,
};
use super::schema::initialize_schema;
use super::serials::db_allocate_serial;
use rusqlite::Connection;
use std::path::Path;
use std::thread;
use tokio::sync::{mpsc, oneshot};

const DB_COMMAND_BUFFER: usize = 64;

enum DbCommand {
    Logs {
        response: oneshot::Sender<rusqlite::Result<Vec<Log>>>,
    },
    Log {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<Option<Log>>>,
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
    LogQsoCount {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<usize>>,
    },
    Radios {
        response: oneshot::Sender<rusqlite::Result<Vec<RadioRecord>>>,
    },
    Radio {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<Option<RadioRecord>>>,
    },
    BackendRadioConfigs {
        response: oneshot::Sender<rusqlite::Result<Vec<radio_io::RadioConfig>>>,
    },
    AuthConfig {
        response: oneshot::Sender<rusqlite::Result<AuthConfig>>,
    },
    DxClusterConfig {
        response: oneshot::Sender<rusqlite::Result<DxClusterConfig>>,
    },
    IaruRegion {
        response: oneshot::Sender<rusqlite::Result<i64>>,
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
    #[allow(dead_code)]
    UpsertClientRadio {
        client_instance_id: String,
        radio_ws_url: String,
        radio: RadioPayload,
        response: oneshot::Sender<rusqlite::Result<RadioRecord>>,
    },
    Contacts {
        log_id: i64,
        response: oneshot::Sender<rusqlite::Result<Vec<Contact>>>,
    },
    Contact {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<Option<Contact>>>,
    },
    UpsertContacts {
        log_id: i64,
        contacts: Vec<Contact>,
        response: oneshot::Sender<rusqlite::Result<Vec<Contact>>>,
    },
    ContactLogId {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<Option<i64>>>,
    },
    AllocateSerial {
        log_id: i64,
        field_adif: String,
        response: oneshot::Sender<rusqlite::Result<SerialAllocation>>,
    },
    DeleteContact {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<Option<i64>>>,
    },
}

#[derive(Clone)]
pub struct Database {
    commands: mpsc::Sender<DbCommand>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let (commands, command_rx) = mpsc::channel(DB_COMMAND_BUFFER);
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let path = path.as_ref().to_path_buf();

        thread::Builder::new()
            .name("log73-db-worker".to_string())
            .spawn(move || {
                let connection = Connection::open(&path).and_then(|connection| {
                    connection.pragma_update(None, "foreign_keys", "ON")?;
                    initialize_schema(&connection)?;
                    Ok(connection)
                });

                match connection {
                    Ok(connection) => {
                        let _ = ready_tx.send(Ok(()));
                        run_db_worker(connection, command_rx);
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

        ready_rx.recv().map_err(|_| {
            rusqlite::Error::InvalidParameterName(
                "database worker failed to report initialization status".to_string(),
            )
        })??;

        Ok(Self { commands })
    }

    async fn call<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<rusqlite::Result<T>>) -> DbCommand,
    ) -> rusqlite::Result<T> {
        let (response_tx, response_rx) = oneshot::channel();
        self.commands
            .send(command(response_tx))
            .await
            .map_err(|_| database_worker_unavailable())?;
        response_rx
            .await
            .map_err(|_| database_worker_unavailable())?
    }

    pub async fn logs(&self) -> rusqlite::Result<Vec<Log>> {
        self.call(|response| DbCommand::Logs { response }).await
    }

    pub async fn log(&self, id: i64) -> rusqlite::Result<Option<Log>> {
        self.call(|response| DbCommand::Log { id, response }).await
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
        self.call(|response| DbCommand::DeleteLog { id, response })
            .await
    }

    pub async fn log_qso_count(&self, id: i64) -> rusqlite::Result<usize> {
        self.call(|response| DbCommand::LogQsoCount { id, response })
            .await
    }

    pub async fn radios(&self) -> rusqlite::Result<Vec<RadioRecord>> {
        self.call(|response| DbCommand::Radios { response }).await
    }

    pub async fn radio(&self, id: i64) -> rusqlite::Result<Option<RadioRecord>> {
        self.call(|response| DbCommand::Radio { id, response })
            .await
    }

    pub async fn backend_radio_configs(&self) -> rusqlite::Result<Vec<radio_io::RadioConfig>> {
        self.call(|response| DbCommand::BackendRadioConfigs { response })
            .await
    }

    pub async fn auth_config(&self) -> rusqlite::Result<AuthConfig> {
        self.call(|response| DbCommand::AuthConfig { response })
            .await
    }

    pub async fn dxcluster_config(&self) -> rusqlite::Result<DxClusterConfig> {
        self.call(|response| DbCommand::DxClusterConfig { response })
            .await
    }

    pub async fn iaru_region(&self) -> rusqlite::Result<i64> {
        self.call(|response| DbCommand::IaruRegion { response })
            .await
    }

    pub async fn config_view(&self) -> rusqlite::Result<ConfigView> {
        let auth_config = self.auth_config().await?;
        let dxcluster_config = self.dxcluster_config().await?;
        let login_enabled =
            !auth_config.login_user.trim().is_empty() && !auth_config.login_password.is_empty();
        Ok(ConfigView {
            iaru_region: self.iaru_region().await?,
            login_user: auth_config.login_user,
            login_enabled,
            dxcluster_enabled: dxcluster_config.enabled,
            dxcluster_host: dxcluster_config.host,
            dxcluster_port: dxcluster_config.port,
            dxcluster_callsign: dxcluster_config.callsign,
            dxcluster_max_age_min: dxcluster_config.max_age_min,
            dxcluster_commands: dxcluster_config.commands,
        })
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
        self.call(|response| DbCommand::DeleteRadio { id, response })
            .await
    }

    /// B2 registration endpoint boundary; exercised by persistence tests until the route lands.
    #[allow(dead_code)]
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

    pub async fn contacts(&self, log_id: i64) -> rusqlite::Result<Vec<Contact>> {
        self.call(|response| DbCommand::Contacts { log_id, response })
            .await
    }

    pub async fn contact(&self, id: i64) -> rusqlite::Result<Option<Contact>> {
        self.call(|response| DbCommand::Contact { id, response })
            .await
    }

    pub async fn upsert_contacts(
        &self,
        log_id: i64,
        contacts: Vec<Contact>,
    ) -> rusqlite::Result<Vec<Contact>> {
        self.call(|response| DbCommand::UpsertContacts {
            log_id,
            contacts,
            response,
        })
        .await
    }

    pub async fn contact_log_id(&self, id: i64) -> rusqlite::Result<Option<i64>> {
        self.call(|response| DbCommand::ContactLogId { id, response })
            .await
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

    pub async fn delete_contact(&self, id: i64) -> rusqlite::Result<Option<i64>> {
        self.call(|response| DbCommand::DeleteContact { id, response })
            .await
    }
}

fn database_worker_unavailable() -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName("database worker unavailable".to_string())
}

fn run_db_worker(mut connection: Connection, mut commands: mpsc::Receiver<DbCommand>) {
    while let Some(command) = commands.blocking_recv() {
        match command {
            DbCommand::Logs { response } => {
                let _ = response.send(db_logs(&connection));
            }
            DbCommand::Log { id, response } => {
                let _ = response.send(select_log(&connection, id));
            }
            DbCommand::CreateLog { log, response } => {
                let _ = response.send(db_create_log(&connection, log));
            }
            DbCommand::UpdateLog { id, log, response } => {
                let _ = response.send(db_update_log(&connection, id, log));
            }
            DbCommand::DeleteLog { id, response } => {
                let _ = response.send(db_delete_log(&connection, id));
            }
            DbCommand::LogQsoCount { id, response } => {
                let _ = response.send(db_log_qso_count(&connection, id));
            }
            DbCommand::Radios { response } => {
                let _ = response.send(db_radios(&connection));
            }
            DbCommand::Radio { id, response } => {
                let _ = response.send(select_radio(&connection, id));
            }
            DbCommand::BackendRadioConfigs { response } => {
                let _ = response.send(db_backend_radio_configs(&connection));
            }
            DbCommand::AuthConfig { response } => {
                let _ = response.send(db_auth_config(&connection));
            }
            DbCommand::DxClusterConfig { response } => {
                let _ = response.send(db_dxcluster_config(&connection));
            }
            DbCommand::IaruRegion { response } => {
                let _ = response.send(db_iaru_region(&connection));
            }
            DbCommand::UpdateConfig { config, response } => {
                let _ = response.send(db_update_config(&connection, config));
            }
            DbCommand::CreateRadio { radio, response } => {
                let _ = response.send(db_create_radio(&connection, radio));
            }
            DbCommand::UpdateRadio {
                id,
                radio,
                response,
            } => {
                let _ = response.send(db_update_radio(&connection, id, radio));
            }
            DbCommand::DeleteRadio { id, response } => {
                let _ = response.send(db_delete_radio(&connection, id));
            }
            DbCommand::UpsertClientRadio {
                client_instance_id,
                radio_ws_url,
                radio,
                response,
            } => {
                let _ = response.send(db_upsert_client_radio(
                    &connection,
                    &client_instance_id,
                    &radio_ws_url,
                    radio,
                ));
            }
            DbCommand::Contacts { log_id, response } => {
                let _ = response.send(db_contacts(&connection, log_id));
            }
            DbCommand::Contact { id, response } => {
                let _ = response.send(select_contact(&connection, id));
            }
            DbCommand::UpsertContacts {
                log_id,
                contacts,
                response,
            } => {
                let _ = response.send(db_upsert_contacts(&mut connection, log_id, contacts));
            }
            DbCommand::ContactLogId { id, response } => {
                let _ = response.send(select_contact_log_id(&connection, id));
            }
            DbCommand::AllocateSerial {
                log_id,
                field_adif,
                response,
            } => {
                let _ = response.send(db_allocate_serial(&mut connection, log_id, &field_adif));
            }
            DbCommand::DeleteContact { id, response } => {
                let _ = response.send(db_delete_contact(&connection, id));
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
        contact_adif_value, contact_id, contact_log_id, set_contact_adif, set_contact_meta,
    };
    use crate::db::models::LoginPasswordUpdate;
    use crate::voice_messages::DEFAULT_VOICE_MESSAGES;
    use serde_json::{Map, Value, json};

    fn test_database() -> Database {
        Database::open(":memory:").expect("in-memory database opens")
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
