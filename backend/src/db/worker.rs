use super::bands::db_bands;
use super::config::{db_auth_config, db_dxcluster_config, db_update_config};
use super::contact::Contact;
use super::contacts::{
    db_contacts, db_delete_contact, db_upsert_contacts, select_contact, select_contact_log_id,
};
use super::logs::{
    db_create_log, db_delete_log, db_log_qso_count, db_logs, db_update_log, select_log,
};
use super::models::{
    AuthConfig, ConfigView, DxClusterConfig, Log, NewLog, RadioConfig, RadioPayload,
    SerialAllocation, UpdateConfig, UpdateLog,
};
use super::radios::{db_create_radio, db_delete_radio, db_radios, db_update_radio, select_radio};
use super::schema::initialize_schema;
use super::serials::db_allocate_serials;
use crate::bands::Band;
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
    Bands {
        iaru_region: i64,
        response: oneshot::Sender<rusqlite::Result<Vec<Band>>>,
    },
    Radios {
        response: oneshot::Sender<rusqlite::Result<Vec<RadioConfig>>>,
    },
    Radio {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<Option<RadioConfig>>>,
    },
    AuthConfig {
        response: oneshot::Sender<rusqlite::Result<AuthConfig>>,
    },
    DxClusterConfig {
        response: oneshot::Sender<rusqlite::Result<DxClusterConfig>>,
    },
    UpdateConfig {
        config: UpdateConfig,
        response: oneshot::Sender<rusqlite::Result<()>>,
    },
    CreateRadio {
        radio: RadioPayload,
        response: oneshot::Sender<rusqlite::Result<RadioConfig>>,
    },
    UpdateRadio {
        id: i64,
        radio: RadioPayload,
        response: oneshot::Sender<rusqlite::Result<Option<RadioConfig>>>,
    },
    DeleteRadio {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<bool>>,
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
    AllocateSerials {
        log_id: i64,
        field_adif: String,
        count: i64,
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

    pub async fn bands(&self, iaru_region: i64) -> rusqlite::Result<Vec<Band>> {
        self.call(|response| DbCommand::Bands {
            iaru_region,
            response,
        })
        .await
    }

    pub async fn radios(&self) -> rusqlite::Result<Vec<RadioConfig>> {
        self.call(|response| DbCommand::Radios { response }).await
    }

    pub async fn radio(&self, id: i64) -> rusqlite::Result<Option<RadioConfig>> {
        self.call(|response| DbCommand::Radio { id, response })
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

    pub async fn config_view(&self) -> rusqlite::Result<ConfigView> {
        let auth_config = self.auth_config().await?;
        let dxcluster_config = self.dxcluster_config().await?;
        let login_enabled =
            !auth_config.login_user.trim().is_empty() && !auth_config.login_password.is_empty();
        Ok(ConfigView {
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

    pub async fn create_radio(&self, radio: RadioPayload) -> rusqlite::Result<RadioConfig> {
        self.call(|response| DbCommand::CreateRadio { radio, response })
            .await
    }

    pub async fn update_radio(
        &self,
        id: i64,
        radio: RadioPayload,
    ) -> rusqlite::Result<Option<RadioConfig>> {
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

    pub async fn allocate_serials(
        &self,
        log_id: i64,
        field_adif: String,
        count: i64,
    ) -> rusqlite::Result<SerialAllocation> {
        self.call(|response| DbCommand::AllocateSerials {
            log_id,
            field_adif,
            count,
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
            DbCommand::Bands {
                iaru_region,
                response,
            } => {
                let _ = response.send(db_bands(&connection, iaru_region));
            }
            DbCommand::Radios { response } => {
                let _ = response.send(db_radios(&connection));
            }
            DbCommand::Radio { id, response } => {
                let _ = response.send(select_radio(&connection, id));
            }
            DbCommand::AuthConfig { response } => {
                let _ = response.send(db_auth_config(&connection));
            }
            DbCommand::DxClusterConfig { response } => {
                let _ = response.send(db_dxcluster_config(&connection));
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
            DbCommand::AllocateSerials {
                log_id,
                field_adif,
                count,
                response,
            } => {
                let _ = response.send(db_allocate_serials(
                    &mut connection,
                    log_id,
                    &field_adif,
                    count,
                ));
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
    async fn allocate_serials_reserves_ranges_by_log_and_field() {
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
            .allocate_serials(first_log.id, "STX".to_string(), 10)
            .await
            .expect("serials are allocated");
        let second = database
            .allocate_serials(first_log.id, "STX".to_string(), 10)
            .await
            .expect("next serials are allocated");
        let other_log = database
            .allocate_serials(second_log.id, "STX".to_string(), 10)
            .await
            .expect("other log serials are allocated");
        let other_field = database
            .allocate_serials(first_log.id, "CUSTOM_SERIAL".to_string(), 3)
            .await
            .expect("other field serials are allocated");

        assert_eq!((first.start, first.end), (1, 10));
        assert_eq!((second.start, second.end), (11, 20));
        assert_eq!((other_log.start, other_log.end), (1, 10));
        assert_eq!((other_field.start, other_field.end), (1, 3));
    }

    #[tokio::test]
    async fn allocate_serials_starts_after_committed_column_or_json_serials() {
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
            .allocate_serials(log.id, "STX".to_string(), 5)
            .await
            .expect("STX serials are allocated");
        let custom = database
            .allocate_serials(log.id, "CUSTOM_SERIAL".to_string(), 5)
            .await
            .expect("custom serials are allocated");

        assert_eq!((stx.start, stx.end), (43, 47));
        assert_eq!((custom.start, custom.end), (78, 82));
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
        assert_eq!(radio.voice_messages, DEFAULT_VOICE_MESSAGES);
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
}
