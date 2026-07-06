use crate::cw::DEFAULT_CW_MESSAGES;
use crate::voice_messages::DEFAULT_VOICE_MESSAGES;
use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::path::Path;
use std::thread;
use tokio::sync::{mpsc, oneshot};

pub type Contact = Map<String, Value>;
pub type ContactFields = Map<String, Value>;

const META_KEY: &str = "meta";
const ADIF_KEY: &str = "adif";

pub fn build_contact(meta: ContactFields, adif: ContactFields) -> Contact {
    let mut contact = Map::new();
    contact.insert(META_KEY.to_string(), Value::Object(meta));
    contact.insert(ADIF_KEY.to_string(), Value::Object(adif));
    contact
}

pub fn contact_meta(contact: &Contact) -> Option<&ContactFields> {
    contact.get(META_KEY).and_then(Value::as_object)
}

pub fn contact_adif(contact: &Contact) -> Option<&ContactFields> {
    contact.get(ADIF_KEY).and_then(Value::as_object)
}

pub fn contact_meta_value<'a>(contact: &'a Contact, key: &str) -> Option<&'a Value> {
    contact_meta(contact).and_then(|meta| meta.get(key))
}

pub fn contact_adif_value<'a>(contact: &'a Contact, key: &str) -> Option<&'a Value> {
    contact_adif(contact).and_then(|adif| adif.get(key))
}

pub fn set_contact_meta(contact: &mut Contact, key: &str, value: Value) {
    if !matches!(contact.get(META_KEY), Some(Value::Object(_))) {
        contact.insert(META_KEY.to_string(), Value::Object(Map::new()));
    }
    if let Some(meta) = contact.get_mut(META_KEY).and_then(Value::as_object_mut) {
        meta.insert(key.to_string(), value);
    }
}

#[cfg(test)]
pub fn set_contact_adif(contact: &mut Contact, key: &str, value: Value) {
    if !matches!(contact.get(ADIF_KEY), Some(Value::Object(_))) {
        contact.insert(ADIF_KEY.to_string(), Value::Object(Map::new()));
    }
    if let Some(adif) = contact.get_mut(ADIF_KEY).and_then(Value::as_object_mut) {
        adif.insert(key.to_string(), value);
    }
}

pub fn contact_id(contact: &Contact) -> Option<i64> {
    contact_meta_value(contact, "id").and_then(json_i64_value)
}

pub fn contact_log_id(contact: &Contact) -> Option<i64> {
    contact_meta_value(contact, "logId").and_then(json_i64_value)
}

#[derive(Debug, Clone, Serialize)]
pub struct Log {
    pub id: i64,
    pub name: String,
    pub contest_id: String,
    pub station_callsign: String,
    pub contest_params: Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewLog {
    pub name: String,
    pub contest_id: String,
    pub station_callsign: String,
    #[serde(default)]
    pub contest_params: Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateLog {
    pub name: String,
    pub station_callsign: String,
    #[serde(default)]
    pub contest_params: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RadioConfig {
    pub id: i64,
    pub name: String,
    pub radio_kind: String,
    pub transport_kind: String,
    pub tcp_host: String,
    pub tcp_port: u16,
    pub serial_port: String,
    pub serial_baud_rate: u32,
    pub options: String,
    pub cw_tuning_increment_hz: u32,
    pub ssb_tuning_increment_hz: u32,
    pub rit_clear_on_log: bool,
    pub voice_input_device_id: Option<String>,
    pub voice_output_device_id: Option<String>,
    pub cw_keyer_type: String,
    pub winkeyer_serial_port: String,
    pub cw_serial_port: String,
    pub cw_serial_baud_rate: u32,
    pub cw_serial_line: String,
    pub cw_messages: String,
    pub voice_messages: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SerialAllocation {
    pub log_id: i64,
    pub field_adif: String,
    pub start: i64,
    pub end: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigView {
    pub login_user: String,
    pub login_enabled: bool,
    pub dxcluster_enabled: bool,
    pub dxcluster_host: String,
    pub dxcluster_port: u16,
    pub dxcluster_callsign: String,
    pub dxcluster_max_age_min: u16,
    pub dxcluster_commands: String,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub login_user: String,
    pub login_password: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DxClusterConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub callsign: String,
    pub max_age_min: u16,
    pub commands: String,
}

impl Default for DxClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: String::new(),
            port: DEFAULT_DXCLUSTER_PORT,
            callsign: String::new(),
            max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
            commands: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum LoginPasswordUpdate {
    Preserve,
    Set(String),
    Disable,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateConfig {
    pub login_user: String,
    pub login_password: LoginPasswordUpdate,
    pub dxcluster_enabled: bool,
    pub dxcluster_host: String,
    pub dxcluster_port: u16,
    pub dxcluster_callsign: String,
    pub dxcluster_max_age_min: u16,
    pub dxcluster_commands: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewRadio {
    pub name: String,
    pub radio_kind: String,
    pub transport_kind: String,
    pub tcp_host: String,
    pub tcp_port: u16,
    pub serial_port: String,
    pub serial_baud_rate: u32,
    #[serde(default)]
    pub options: String,
    #[serde(default = "default_cw_tuning_increment_hz")]
    pub cw_tuning_increment_hz: u32,
    #[serde(default = "default_ssb_tuning_increment_hz")]
    pub ssb_tuning_increment_hz: u32,
    #[serde(default)]
    pub rit_clear_on_log: bool,
    #[serde(default)]
    pub voice_input_device_id: Option<String>,
    #[serde(default)]
    pub voice_output_device_id: Option<String>,
    pub cw_keyer_type: String,
    pub winkeyer_serial_port: String,
    #[serde(default)]
    pub cw_serial_port: String,
    #[serde(default = "default_cw_serial_baud_rate")]
    pub cw_serial_baud_rate: u32,
    #[serde(default = "default_cw_serial_line")]
    pub cw_serial_line: String,
    #[serde(default = "default_cw_messages")]
    pub cw_messages: String,
    #[serde(default = "default_voice_messages")]
    pub voice_messages: String,
}

pub const DEFAULT_CW_TUNING_INCREMENT_HZ: u32 = 20;
pub const DEFAULT_SSB_TUNING_INCREMENT_HZ: u32 = 100;

fn default_cw_tuning_increment_hz() -> u32 {
    DEFAULT_CW_TUNING_INCREMENT_HZ
}

fn default_ssb_tuning_increment_hz() -> u32 {
    DEFAULT_SSB_TUNING_INCREMENT_HZ
}

fn default_cw_serial_baud_rate() -> u32 {
    9_600
}

fn default_cw_serial_line() -> String {
    "dtr".to_string()
}

fn default_cw_messages() -> String {
    DEFAULT_CW_MESSAGES.to_string()
}

fn default_voice_messages() -> String {
    DEFAULT_VOICE_MESSAGES.to_string()
}

const QSO_COLUMNS: &[&str] = &[
    "LOG_ID",
    "QSO_DATE_TIME_ON",
    "STATION_CALLSIGN",
    "OPERATOR",
    "CALL",
    "BAND",
    "FREQ",
    "MODE",
    "RST_SENT",
    "RST_RCVD",
    "ARRL_SECT",
    "CNTY",
    "CQZ",
    "DXCC",
    "GRIDSQUARE",
    "MY_CNTY",
    "MY_CQ_ZONE",
    "MY_GRIDSQUARE",
    "MY_STATE",
    "MY_ARRL_SECT",
    "SRX",
    "SRX_STRING",
    "STATE",
    "STX",
    "STX_STRING",
    "TX_PWR",
    "JSON",
];

const INTEGER_COLUMNS: &[&str] = &[
    "LOG_ID",
    "QSO_DATE_TIME_ON",
    "FREQ",
    "RST_SENT",
    "RST_RCVD",
    "CQZ",
    "DXCC",
    "MY_CQ_ZONE",
    "SRX",
    "STX",
    "TX_PWR",
];

const INSERT_QSO_SQL: &str = r#"
INSERT INTO qsos (
    LOG_ID,
    QSO_DATE_TIME_ON,
    STATION_CALLSIGN,
    OPERATOR,
    CALL,
    BAND,
    FREQ,
    MODE,
    RST_SENT,
    RST_RCVD,
    ARRL_SECT,
    CNTY,
    CQZ,
    DXCC,
    GRIDSQUARE,
    MY_CNTY,
    MY_CQ_ZONE,
    MY_GRIDSQUARE,
    MY_STATE,
    MY_ARRL_SECT,
    SRX,
    SRX_STRING,
    STATE,
    STX,
    STX_STRING,
    TX_PWR,
    JSON
) VALUES (
    ?1,
    ?2,
    ?3,
    ?4,
    ?5,
    ?6,
    ?7,
    ?8,
    ?9,
    ?10,
    ?11,
    ?12,
    ?13,
    ?14,
    ?15,
    ?16,
    ?17,
    ?18,
    ?19,
    ?20,
    ?21,
    ?22,
    ?23,
    ?24,
    ?25,
    ?26,
    ?27
)
"#;

const UPSERT_QSO_SQL: &str = r#"
INSERT INTO qsos (
    ID,
    LOG_ID,
    QSO_DATE_TIME_ON,
    STATION_CALLSIGN,
    OPERATOR,
    CALL,
    BAND,
    FREQ,
    MODE,
    RST_SENT,
    RST_RCVD,
    ARRL_SECT,
    CNTY,
    CQZ,
    DXCC,
    GRIDSQUARE,
    MY_CNTY,
    MY_CQ_ZONE,
    MY_GRIDSQUARE,
    MY_STATE,
    MY_ARRL_SECT,
    SRX,
    SRX_STRING,
    STATE,
    STX,
    STX_STRING,
    TX_PWR,
    JSON
) VALUES (
    ?1,
    ?2,
    ?3,
    ?4,
    ?5,
    ?6,
    ?7,
    ?8,
    ?9,
    ?10,
    ?11,
    ?12,
    ?13,
    ?14,
    ?15,
    ?16,
    ?17,
    ?18,
    ?19,
    ?20,
    ?21,
    ?22,
    ?23,
    ?24,
    ?25,
    ?26,
    ?27,
    ?28
)
ON CONFLICT(ID) DO UPDATE SET
    LOG_ID = excluded.LOG_ID,
    QSO_DATE_TIME_ON = excluded.QSO_DATE_TIME_ON,
    STATION_CALLSIGN = excluded.STATION_CALLSIGN,
    OPERATOR = excluded.OPERATOR,
    CALL = excluded.CALL,
    BAND = excluded.BAND,
    FREQ = excluded.FREQ,
    MODE = excluded.MODE,
    RST_SENT = excluded.RST_SENT,
    RST_RCVD = excluded.RST_RCVD,
    ARRL_SECT = excluded.ARRL_SECT,
    CNTY = excluded.CNTY,
    CQZ = excluded.CQZ,
    DXCC = excluded.DXCC,
    GRIDSQUARE = excluded.GRIDSQUARE,
    MY_CNTY = excluded.MY_CNTY,
    MY_CQ_ZONE = excluded.MY_CQ_ZONE,
    MY_GRIDSQUARE = excluded.MY_GRIDSQUARE,
    MY_STATE = excluded.MY_STATE,
    MY_ARRL_SECT = excluded.MY_ARRL_SECT,
    SRX = excluded.SRX,
    SRX_STRING = excluded.SRX_STRING,
    STATE = excluded.STATE,
    STX = excluded.STX,
    STX_STRING = excluded.STX_STRING,
    TX_PWR = excluded.TX_PWR,
    JSON = excluded.JSON
"#;

pub const DEFAULT_DXCLUSTER_PORT: u16 = 23;
pub const DEFAULT_DXCLUSTER_MAX_AGE_MIN: u16 = 60;
pub const MIN_DXCLUSTER_MAX_AGE_MIN: u16 = 15;
pub const MAX_DXCLUSTER_MAX_AGE_MIN: u16 = 360;

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
        radio: NewRadio,
        response: oneshot::Sender<rusqlite::Result<RadioConfig>>,
    },
    UpdateRadio {
        id: i64,
        radio: NewRadio,
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

    pub async fn create_radio(&self, radio: NewRadio) -> rusqlite::Result<RadioConfig> {
        self.call(|response| DbCommand::CreateRadio { radio, response })
            .await
    }

    pub async fn update_radio(
        &self,
        id: i64,
        radio: NewRadio,
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

fn db_logs(connection: &Connection) -> rusqlite::Result<Vec<Log>> {
    let mut statement = connection.prepare(
        "SELECT ID, NAME, CONTEST_ID, STATION_CALLSIGN, CONTEST_PARAMS_JSON FROM logs ORDER BY NAME, ID",
    )?;
    let rows = statement.query_map([], row_to_log)?;
    rows.collect()
}

fn db_create_log(connection: &Connection, log: NewLog) -> rusqlite::Result<Log> {
    connection.execute(
        "INSERT INTO logs (NAME, CONTEST_ID, STATION_CALLSIGN, CONTEST_PARAMS_JSON) VALUES (?1, ?2, ?3, ?4)",
        params![
            log.name.trim(),
            log.contest_id.trim(),
            log.station_callsign.trim().to_uppercase(),
            log.contest_params.to_string()
        ],
    )?;
    select_log(connection, connection.last_insert_rowid())?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)
}

fn db_update_log(
    connection: &Connection,
    id: i64,
    log: UpdateLog,
) -> rusqlite::Result<Option<Log>> {
    let updated = connection.execute(
        "UPDATE logs SET NAME = ?1, STATION_CALLSIGN = ?2, CONTEST_PARAMS_JSON = ?3 WHERE ID = ?4",
        params![
            log.name.trim(),
            log.station_callsign.trim().to_uppercase(),
            log.contest_params.to_string(),
            id
        ],
    )?;
    if updated == 0 {
        return Ok(None);
    }
    select_log(connection, id)
}

fn db_delete_log(connection: &Connection, id: i64) -> rusqlite::Result<bool> {
    Ok(connection.execute("DELETE FROM logs WHERE ID = ?1", params![id])? > 0)
}

fn db_log_qso_count(connection: &Connection, id: i64) -> rusqlite::Result<usize> {
    connection.query_row(
        "SELECT COUNT(*) FROM qsos WHERE LOG_ID = ?1",
        params![id],
        |row| row.get(0),
    )
}

fn db_radios(connection: &Connection) -> rusqlite::Result<Vec<RadioConfig>> {
    let mut statement = connection.prepare(
        "SELECT ID, NAME, RADIO_KIND, TRANSPORT_KIND, TCP_HOST, TCP_PORT, SERIAL_PORT, SERIAL_BAUD_RATE, OPTIONS, CW_TUNING_INCREMENT_HZ, SSB_TUNING_INCREMENT_HZ, RIT_CLEAR_ON_LOG, VOICE_INPUT_DEVICE_ID, VOICE_OUTPUT_DEVICE_ID, CW_KEYER_TYPE, WINKEYER_SERIAL_PORT, CW_SERIAL_PORT, CW_SERIAL_BAUD_RATE, CW_SERIAL_LINE, CW_MESSAGES, VOICE_MESSAGES FROM radios ORDER BY ID",
    )?;
    let rows = statement.query_map([], row_to_radio)?;
    rows.collect()
}

fn db_auth_config(connection: &Connection) -> rusqlite::Result<AuthConfig> {
    match connection
        .query_row(
            "SELECT LOGIN_USER, LOGIN_PASSWORD FROM config LIMIT 1",
            [],
            |row| {
                Ok(AuthConfig {
                    login_user: row.get(0)?,
                    login_password: row.get(1)?,
                })
            },
        )
        .optional()
    {
        Ok(Some(config)) => Ok(config),
        Ok(None) => Ok(AuthConfig {
            login_user: String::new(),
            login_password: String::new(),
        }),
        Err(error) if is_missing_config_column(&error) => Ok(AuthConfig {
            login_user: String::new(),
            login_password: String::new(),
        }),
        Err(error) => Err(error),
    }
}

fn db_dxcluster_config(connection: &Connection) -> rusqlite::Result<DxClusterConfig> {
    match connection
        .query_row(
            "SELECT DXCLUSTER_ENABLED, DXCLUSTER_HOST, DXCLUSTER_PORT, DXCLUSTER_CALLSIGN, DXCLUSTER_MAX_AGE_MIN, DXCLUSTER_COMMANDS FROM config LIMIT 1",
            [],
            |row| {
                Ok(DxClusterConfig {
                    enabled: row.get(0)?,
                    host: row.get(1)?,
                    port: row.get(2)?,
                    callsign: row.get(3)?,
                    max_age_min: row.get(4)?,
                    commands: row.get(5)?,
                })
            },
        )
        .optional()
    {
        Ok(Some(config)) => Ok(config),
        Ok(None) => Ok(DxClusterConfig::default()),
        Err(error) if is_missing_config_column(&error) => Ok(DxClusterConfig::default()),
        Err(error) => Err(error),
    }
}

fn db_update_config(connection: &Connection, config: UpdateConfig) -> rusqlite::Result<()> {
    let max_age_min = config
        .dxcluster_max_age_min
        .clamp(MIN_DXCLUSTER_MAX_AGE_MIN, MAX_DXCLUSTER_MAX_AGE_MIN);
    let login_password = match config.login_password {
        LoginPasswordUpdate::Preserve => db_auth_config(connection)?.login_password,
        LoginPasswordUpdate::Set(login_password) => login_password,
        LoginPasswordUpdate::Disable => String::new(),
    };
    let login_user = config.login_user.trim();
    let dxcluster_host = config.dxcluster_host.trim();
    let dxcluster_callsign = config.dxcluster_callsign.trim().to_uppercase();
    let dxcluster_commands = &config.dxcluster_commands;

    let updated = connection.execute(
        "UPDATE config SET LOGIN_USER = ?1, LOGIN_PASSWORD = ?2, DXCLUSTER_ENABLED = ?3, DXCLUSTER_HOST = ?4, DXCLUSTER_PORT = ?5, DXCLUSTER_CALLSIGN = ?6, DXCLUSTER_MAX_AGE_MIN = ?7, DXCLUSTER_COMMANDS = ?8",
        params![
            login_user,
            &login_password,
            config.dxcluster_enabled,
            dxcluster_host,
            config.dxcluster_port,
            &dxcluster_callsign,
            max_age_min,
            dxcluster_commands
        ],
    )?;
    if updated == 0 {
        connection.execute(
            "INSERT INTO config (version, LOGIN_USER, LOGIN_PASSWORD, DXCLUSTER_ENABLED, DXCLUSTER_HOST, DXCLUSTER_PORT, DXCLUSTER_CALLSIGN, DXCLUSTER_MAX_AGE_MIN, DXCLUSTER_COMMANDS) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                login_user,
                &login_password,
                config.dxcluster_enabled,
                dxcluster_host,
                config.dxcluster_port,
                &dxcluster_callsign,
                max_age_min,
                dxcluster_commands
            ],
        )?;
    }
    Ok(())
}

fn normalized_optional_device_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn db_create_radio(connection: &Connection, radio: NewRadio) -> rusqlite::Result<RadioConfig> {
    connection.execute(
        "INSERT INTO radios (NAME, RADIO_KIND, TRANSPORT_KIND, TCP_HOST, TCP_PORT, SERIAL_PORT, SERIAL_BAUD_RATE, OPTIONS, CW_TUNING_INCREMENT_HZ, SSB_TUNING_INCREMENT_HZ, RIT_CLEAR_ON_LOG, VOICE_INPUT_DEVICE_ID, VOICE_OUTPUT_DEVICE_ID, CW_KEYER_TYPE, WINKEYER_SERIAL_PORT, CW_SERIAL_PORT, CW_SERIAL_BAUD_RATE, CW_SERIAL_LINE, CW_MESSAGES, VOICE_MESSAGES) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            radio.name.trim(),
            radio.radio_kind.trim(),
            radio.transport_kind.trim(),
            radio.tcp_host.trim(),
            radio.tcp_port,
            radio.serial_port.trim(),
            radio.serial_baud_rate,
            radio.options,
            radio.cw_tuning_increment_hz,
            radio.ssb_tuning_increment_hz,
            radio.rit_clear_on_log,
            normalized_optional_device_id(radio.voice_input_device_id.as_deref()),
            normalized_optional_device_id(radio.voice_output_device_id.as_deref()),
            radio.cw_keyer_type.trim(),
            radio.winkeyer_serial_port.trim(),
            radio.cw_serial_port.trim(),
            radio.cw_serial_baud_rate,
            radio.cw_serial_line.trim(),
            radio.cw_messages,
            radio.voice_messages
        ],
    )?;
    select_radio(connection, connection.last_insert_rowid())?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)
}

fn db_update_radio(
    connection: &Connection,
    id: i64,
    radio: NewRadio,
) -> rusqlite::Result<Option<RadioConfig>> {
    let updated = connection.execute(
        "UPDATE radios SET NAME = ?1, RADIO_KIND = ?2, TRANSPORT_KIND = ?3, TCP_HOST = ?4, TCP_PORT = ?5, SERIAL_PORT = ?6, SERIAL_BAUD_RATE = ?7, OPTIONS = ?8, CW_TUNING_INCREMENT_HZ = ?9, SSB_TUNING_INCREMENT_HZ = ?10, RIT_CLEAR_ON_LOG = ?11, VOICE_INPUT_DEVICE_ID = ?12, VOICE_OUTPUT_DEVICE_ID = ?13, CW_KEYER_TYPE = ?14, WINKEYER_SERIAL_PORT = ?15, CW_SERIAL_PORT = ?16, CW_SERIAL_BAUD_RATE = ?17, CW_SERIAL_LINE = ?18, CW_MESSAGES = ?19, VOICE_MESSAGES = ?20 WHERE ID = ?21",
        params![
            radio.name.trim(),
            radio.radio_kind.trim(),
            radio.transport_kind.trim(),
            radio.tcp_host.trim(),
            radio.tcp_port,
            radio.serial_port.trim(),
            radio.serial_baud_rate,
            radio.options,
            radio.cw_tuning_increment_hz,
            radio.ssb_tuning_increment_hz,
            radio.rit_clear_on_log,
            normalized_optional_device_id(radio.voice_input_device_id.as_deref()),
            normalized_optional_device_id(radio.voice_output_device_id.as_deref()),
            radio.cw_keyer_type.trim(),
            radio.winkeyer_serial_port.trim(),
            radio.cw_serial_port.trim(),
            radio.cw_serial_baud_rate,
            radio.cw_serial_line.trim(),
            radio.cw_messages,
            radio.voice_messages,
            id
        ],
    )?;
    if updated == 0 {
        return Ok(None);
    }
    select_radio(connection, id)
}

fn db_delete_radio(connection: &Connection, id: i64) -> rusqlite::Result<bool> {
    Ok(connection.execute("DELETE FROM radios WHERE ID = ?1", params![id])? > 0)
}

fn db_contacts(connection: &Connection, log_id: i64) -> rusqlite::Result<Vec<Contact>> {
    let mut statement = connection
        .prepare("SELECT * FROM qsos WHERE LOG_ID = ?1 ORDER BY QSO_DATE_TIME_ON DESC, ID DESC")?;
    let rows = statement.query_map(params![log_id], row_to_contact)?;
    rows.collect()
}

fn db_upsert_contacts(
    connection: &mut Connection,
    log_id: i64,
    contacts: Vec<Contact>,
) -> rusqlite::Result<Vec<Contact>> {
    let transaction = connection.transaction()?;
    let mut committed = Vec::with_capacity(contacts.len());

    for mut contact in contacts {
        set_contact_meta(&mut contact, "logId", Value::Number(log_id.into()));
        let id = upsert_contact(&transaction, contact)?;
        if let Some(saved) = select_contact(&transaction, id)? {
            committed.push(saved);
        }
    }

    transaction.commit()?;
    Ok(committed)
}

fn db_delete_contact(connection: &Connection, id: i64) -> rusqlite::Result<Option<i64>> {
    let Some(log_id) = select_contact_log_id(connection, id)? else {
        return Ok(None);
    };

    let deleted = connection.execute("DELETE FROM qsos WHERE ID = ?1", params![id])? > 0;
    Ok(deleted.then_some(log_id))
}

fn initialize_schema(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS config (
            version INTEGER NOT NULL,
            LOGIN_USER TEXT NOT NULL DEFAULT '',
            LOGIN_PASSWORD TEXT NOT NULL DEFAULT '',
            DXCLUSTER_ENABLED INTEGER NOT NULL DEFAULT 0 CHECK (DXCLUSTER_ENABLED IN (0, 1)),
            DXCLUSTER_HOST TEXT NOT NULL DEFAULT '',
            DXCLUSTER_PORT INTEGER NOT NULL DEFAULT 23 CHECK (DXCLUSTER_PORT >= 0 AND DXCLUSTER_PORT <= 65535),
            DXCLUSTER_CALLSIGN TEXT NOT NULL DEFAULT '',
            DXCLUSTER_MAX_AGE_MIN INTEGER NOT NULL DEFAULT 60 CHECK (DXCLUSTER_MAX_AGE_MIN >= 15 AND DXCLUSTER_MAX_AGE_MIN <= 360),
            DXCLUSTER_COMMANDS TEXT NOT NULL DEFAULT ''
        ) STRICT;

        CREATE TABLE IF NOT EXISTS logs (
            ID INTEGER PRIMARY KEY,
            NAME TEXT NOT NULL,
            CONTEST_ID TEXT NOT NULL,
            STATION_CALLSIGN TEXT NOT NULL,
            CONTEST_PARAMS_JSON TEXT NOT NULL
        ) STRICT;

        CREATE TABLE IF NOT EXISTS radios (
            ID INTEGER PRIMARY KEY,
            NAME TEXT NOT NULL,
            RADIO_KIND TEXT NOT NULL,
            TRANSPORT_KIND TEXT NOT NULL,
            TCP_HOST TEXT NOT NULL DEFAULT '',
            TCP_PORT INTEGER NOT NULL DEFAULT 0 CHECK (TCP_PORT >= 0 AND TCP_PORT <= 65535),
            SERIAL_PORT TEXT NOT NULL DEFAULT '',
            SERIAL_BAUD_RATE INTEGER NOT NULL DEFAULT 115200 CHECK (SERIAL_BAUD_RATE > 0),
            OPTIONS TEXT NOT NULL DEFAULT '',
            CW_TUNING_INCREMENT_HZ INTEGER NOT NULL DEFAULT 20 CHECK (CW_TUNING_INCREMENT_HZ > 0),
            SSB_TUNING_INCREMENT_HZ INTEGER NOT NULL DEFAULT 100 CHECK (SSB_TUNING_INCREMENT_HZ > 0),
            RIT_CLEAR_ON_LOG INTEGER NOT NULL DEFAULT 0 CHECK (RIT_CLEAR_ON_LOG IN (0, 1)),
            VOICE_INPUT_DEVICE_ID TEXT,
            VOICE_OUTPUT_DEVICE_ID TEXT,
            CW_KEYER_TYPE TEXT NOT NULL DEFAULT 'none',
            WINKEYER_SERIAL_PORT TEXT NOT NULL DEFAULT '',
            CW_SERIAL_PORT TEXT NOT NULL DEFAULT '',
            CW_SERIAL_BAUD_RATE INTEGER NOT NULL DEFAULT 9600 CHECK (CW_SERIAL_BAUD_RATE > 0),
            CW_SERIAL_LINE TEXT NOT NULL DEFAULT 'dtr',
            CW_MESSAGES TEXT NOT NULL,
            VOICE_MESSAGES TEXT NOT NULL
        ) STRICT;

        CREATE TABLE IF NOT EXISTS qsos (
            ID INTEGER PRIMARY KEY,
            LOG_ID INTEGER NOT NULL REFERENCES logs(ID) ON DELETE CASCADE,
            QSO_DATE_TIME_ON INTEGER NOT NULL,
            STATION_CALLSIGN TEXT NOT NULL,
            OPERATOR TEXT,
            CALL TEXT NOT NULL,
            BAND TEXT NOT NULL,
            FREQ INTEGER NOT NULL,
            MODE TEXT NOT NULL,
            RST_SENT INTEGER,
            RST_RCVD INTEGER,
            ARRL_SECT TEXT,
            CNTY TEXT,
            CQZ INTEGER,
            DXCC INTEGER,
            GRIDSQUARE TEXT,
            MY_CNTY TEXT,
            MY_CQ_ZONE INTEGER,
            MY_GRIDSQUARE TEXT,
            MY_STATE TEXT,
            MY_ARRL_SECT TEXT,
            SRX INTEGER,
            SRX_STRING TEXT,
            STATE TEXT,
            STX INTEGER,
            STX_STRING TEXT,
            TX_PWR INTEGER,
            JSON TEXT
        ) STRICT;

        CREATE INDEX IF NOT EXISTS idx_qsos_log_id ON qsos(LOG_ID);

        CREATE TABLE IF NOT EXISTS log_serial_state (
            LOG_ID INTEGER NOT NULL REFERENCES logs(ID) ON DELETE CASCADE,
            FIELD_ADIF TEXT NOT NULL,
            NEXT_SERIAL INTEGER NOT NULL CHECK (NEXT_SERIAL > 0),
            PRIMARY KEY (LOG_ID, FIELD_ADIF)
        ) STRICT;
        "#,
    )?;

    let config_count: i64 =
        connection.query_row("SELECT COUNT(*) FROM config", [], |row| row.get(0))?;
    if config_count == 0 {
        connection.execute("INSERT INTO config (version) VALUES (1)", [])?;
    } else {
        connection.execute("UPDATE config SET version = 1", [])?;
    }

    Ok(())
}

fn is_missing_config_column(error: &rusqlite::Error) -> bool {
    error.to_string().contains("no such column")
}

fn select_log(connection: &Connection, id: i64) -> rusqlite::Result<Option<Log>> {
    connection
        .query_row(
            "SELECT ID, NAME, CONTEST_ID, STATION_CALLSIGN, CONTEST_PARAMS_JSON FROM logs WHERE ID = ?1",
            params![id],
            row_to_log,
        )
        .optional()
}

fn row_to_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<Log> {
    let contest_params_json: String = row.get("CONTEST_PARAMS_JSON")?;
    let contest_params =
        serde_json::from_str(&contest_params_json).unwrap_or(Value::Object(Map::new()));

    Ok(Log {
        id: row.get("ID")?,
        name: row.get("NAME")?,
        contest_id: row.get("CONTEST_ID")?,
        station_callsign: row.get("STATION_CALLSIGN")?,
        contest_params,
    })
}

fn select_radio(connection: &Connection, id: i64) -> rusqlite::Result<Option<RadioConfig>> {
    connection
        .query_row(
            "SELECT ID, NAME, RADIO_KIND, TRANSPORT_KIND, TCP_HOST, TCP_PORT, SERIAL_PORT, SERIAL_BAUD_RATE, OPTIONS, CW_TUNING_INCREMENT_HZ, SSB_TUNING_INCREMENT_HZ, RIT_CLEAR_ON_LOG, VOICE_INPUT_DEVICE_ID, VOICE_OUTPUT_DEVICE_ID, CW_KEYER_TYPE, WINKEYER_SERIAL_PORT, CW_SERIAL_PORT, CW_SERIAL_BAUD_RATE, CW_SERIAL_LINE, CW_MESSAGES, VOICE_MESSAGES FROM radios WHERE ID = ?1",
            params![id],
            row_to_radio,
        )
        .optional()
}

fn row_to_radio(row: &rusqlite::Row<'_>) -> rusqlite::Result<RadioConfig> {
    let tcp_port: i64 = row.get("TCP_PORT")?;
    let serial_baud_rate: i64 = row.get("SERIAL_BAUD_RATE")?;
    let cw_tuning_increment_hz: i64 = row.get("CW_TUNING_INCREMENT_HZ")?;
    let ssb_tuning_increment_hz: i64 = row.get("SSB_TUNING_INCREMENT_HZ")?;
    let cw_serial_baud_rate: i64 = row.get("CW_SERIAL_BAUD_RATE")?;
    let voice_input_device_id: Option<String> = row.get("VOICE_INPUT_DEVICE_ID")?;
    let voice_output_device_id: Option<String> = row.get("VOICE_OUTPUT_DEVICE_ID")?;
    Ok(RadioConfig {
        id: row.get("ID")?,
        name: row.get("NAME")?,
        radio_kind: row.get("RADIO_KIND")?,
        transport_kind: row.get("TRANSPORT_KIND")?,
        tcp_host: row.get("TCP_HOST")?,
        tcp_port: tcp_port as u16,
        serial_port: row.get("SERIAL_PORT")?,
        serial_baud_rate: serial_baud_rate as u32,
        options: row.get("OPTIONS")?,
        cw_tuning_increment_hz: cw_tuning_increment_hz as u32,
        ssb_tuning_increment_hz: ssb_tuning_increment_hz as u32,
        rit_clear_on_log: row.get("RIT_CLEAR_ON_LOG")?,
        voice_input_device_id: normalized_optional_device_id(voice_input_device_id.as_deref()),
        voice_output_device_id: normalized_optional_device_id(voice_output_device_id.as_deref()),
        cw_keyer_type: row.get("CW_KEYER_TYPE")?,
        winkeyer_serial_port: row.get("WINKEYER_SERIAL_PORT")?,
        cw_serial_port: row.get("CW_SERIAL_PORT")?,
        cw_serial_baud_rate: cw_serial_baud_rate as u32,
        cw_serial_line: row.get("CW_SERIAL_LINE")?,
        cw_messages: row.get("CW_MESSAGES")?,
        voice_messages: row.get("VOICE_MESSAGES")?,
    })
}

fn select_contact(connection: &Connection, id: i64) -> rusqlite::Result<Option<Contact>> {
    connection
        .query_row(
            "SELECT * FROM qsos WHERE ID = ?1",
            params![id],
            row_to_contact,
        )
        .optional()
}

fn select_contact_log_id(connection: &Connection, id: i64) -> rusqlite::Result<Option<i64>> {
    connection
        .query_row(
            "SELECT LOG_ID FROM qsos WHERE ID = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
}

fn db_allocate_serials(
    connection: &mut Connection,
    log_id: i64,
    field_adif: &str,
    count: i64,
) -> rusqlite::Result<SerialAllocation> {
    if log_id <= 0 {
        return Err(rusqlite::Error::InvalidParameterName(
            "log id must be positive".to_string(),
        ));
    }
    if count <= 0 {
        return Err(rusqlite::Error::InvalidParameterName(
            "serial allocation count must be positive".to_string(),
        ));
    }
    let field_adif = field_adif.trim();
    if field_adif.is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "serial field ADIF name is required".to_string(),
        ));
    }

    let transaction = connection.transaction()?;
    let committed_next = max_committed_serial(&transaction, log_id, field_adif)?
        .and_then(|serial| serial.checked_add(1))
        .unwrap_or(1);
    let stored_next = transaction
        .query_row(
            "SELECT NEXT_SERIAL FROM log_serial_state WHERE LOG_ID = ?1 AND FIELD_ADIF = ?2",
            params![log_id, field_adif],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let start = stored_next
        .unwrap_or(committed_next)
        .max(committed_next)
        .max(1);
    let end = start.checked_add(count - 1).ok_or_else(|| {
        rusqlite::Error::InvalidParameterName("serial allocation overflow".to_string())
    })?;
    let next_serial = end.checked_add(1).ok_or_else(|| {
        rusqlite::Error::InvalidParameterName("serial allocation overflow".to_string())
    })?;

    transaction.execute(
        "INSERT INTO log_serial_state (LOG_ID, FIELD_ADIF, NEXT_SERIAL) VALUES (?1, ?2, ?3)
         ON CONFLICT(LOG_ID, FIELD_ADIF) DO UPDATE SET NEXT_SERIAL = excluded.NEXT_SERIAL",
        params![log_id, field_adif, next_serial],
    )?;
    transaction.commit()?;

    Ok(SerialAllocation {
        log_id,
        field_adif: field_adif.to_string(),
        start,
        end,
        count,
    })
}

fn max_committed_serial(
    connection: &Connection,
    log_id: i64,
    field_adif: &str,
) -> rusqlite::Result<Option<i64>> {
    if let Some(column) = qso_column_for_adif(field_adif) {
        return max_committed_column_serial(connection, log_id, column);
    }

    let mut statement =
        connection.prepare("SELECT JSON FROM qsos WHERE LOG_ID = ?1 AND JSON IS NOT NULL")?;
    let mut rows = statement.query(params![log_id])?;
    let mut max_serial = None;
    while let Some(row) = rows.next()? {
        let json_text: Option<String> = row.get(0)?;
        let Some(json_text) = json_text else {
            continue;
        };
        let Ok(Value::Object(extra)) = serde_json::from_str::<Value>(&json_text) else {
            continue;
        };
        if let Some(serial) = extra.get(field_adif).and_then(json_serial_value) {
            max_serial = Some(max_serial.map_or(serial, |current: i64| current.max(serial)));
        }
    }
    Ok(max_serial)
}

fn qso_column_for_adif(field_adif: &str) -> Option<&'static str> {
    QSO_COLUMNS.iter().copied().find(|column| {
        !matches!(*column, "LOG_ID" | "JSON") && column.eq_ignore_ascii_case(field_adif)
    })
}

fn max_committed_column_serial(
    connection: &Connection,
    log_id: i64,
    column: &str,
) -> rusqlite::Result<Option<i64>> {
    let sql = format!("SELECT {column} FROM qsos WHERE LOG_ID = ?1 AND {column} IS NOT NULL");
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement.query(params![log_id])?;
    let mut max_serial = None;
    while let Some(row) = rows.next()? {
        if let Some(serial) = sql_serial_value(row.get_ref(0)?) {
            max_serial = Some(max_serial.map_or(serial, |current: i64| current.max(serial)));
        }
    }
    Ok(max_serial)
}

fn sql_serial_value(value: ValueRef<'_>) -> Option<i64> {
    match value {
        ValueRef::Integer(value) => Some(value),
        ValueRef::Real(value) if value.is_finite() && value.fract() == 0.0 => Some(value as i64),
        ValueRef::Text(value) => std::str::from_utf8(value).ok()?.trim().parse().ok(),
        _ => None,
    }
}

fn json_serial_value(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }
}

fn upsert_contact(connection: &Connection, contact: Contact) -> rusqlite::Result<i64> {
    let id = contact_id(&contact);
    let requested_log_id = contact_log_id(&contact).unwrap_or(1);

    if let Some(id) = id
        && let Some(existing_log_id) = select_contact_log_id(connection, id)?
        && existing_log_id != requested_log_id
    {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "contact id {id} belongs to log {existing_log_id}, cannot write to log {requested_log_id}",
        )));
    }

    let values = contact_to_sql_values(&contact);
    let mut sql_values = Vec::with_capacity(values.len() + if id.is_some() { 1 } else { 0 });
    let sql = if let Some(id) = id {
        sql_values.push(SqlValue::Integer(id));
        sql_values.extend(values);
        UPSERT_QSO_SQL
    } else {
        sql_values.extend(values);
        INSERT_QSO_SQL
    };

    let mut statement = connection.prepare_cached(sql)?;
    statement.execute(rusqlite::params_from_iter(sql_values))?;
    Ok(id.unwrap_or_else(|| connection.last_insert_rowid()))
}

fn contact_to_sql_values(contact: &Contact) -> Vec<SqlValue> {
    QSO_COLUMNS
        .iter()
        .map(|column| {
            if *column == "JSON" {
                return SqlValue::Text(extra_json(contact));
            }
            if *column == "LOG_ID" {
                return SqlValue::Integer(contact_log_id(contact).unwrap_or(1));
            }
            if *column == "QSO_DATE_TIME_ON" {
                return json_i64(contact_adif_value(contact, "QSO_DATE_TIME_ON"))
                    .map(SqlValue::Integer)
                    .unwrap_or(SqlValue::Null);
            }
            if *column == "FREQ" {
                return frequency_hz(contact_adif_value(contact, "FREQ"))
                    .map(SqlValue::Integer)
                    .unwrap_or(SqlValue::Null);
            }

            let value = contact_adif_value(contact, column);
            if INTEGER_COLUMNS.contains(column) {
                json_i64(value)
                    .map(SqlValue::Integer)
                    .unwrap_or(SqlValue::Null)
            } else {
                json_string(value)
                    .map(SqlValue::Text)
                    .unwrap_or(SqlValue::Null)
            }
        })
        .collect()
}

fn extra_json(contact: &Contact) -> String {
    let mapped_keys = mapped_json_keys();
    let extra = if let Some(adif) = contact_adif(contact) {
        adif.iter()
            .filter(|(key, _)| !mapped_keys.contains(key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Map<_, _>>()
    } else {
        contact
            .iter()
            .filter(|(key, _)| !key.starts_with('_') && !mapped_keys.contains(key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Map<_, _>>()
    };
    Value::Object(extra).to_string()
}

fn mapped_json_keys() -> HashSet<&'static str> {
    let mut keys = HashSet::from(["ID"]);
    for column in QSO_COLUMNS {
        if *column != "JSON" && *column != "LOG_ID" {
            keys.insert(column);
        }
    }
    keys
}

fn row_to_contact(row: &rusqlite::Row<'_>) -> rusqlite::Result<Contact> {
    let mut meta = Map::new();
    let mut adif = Map::new();

    let extra_json: Option<String> = row.get("JSON")?;
    if let Some(extra_json) = extra_json
        && let Ok(Value::Object(extra)) = serde_json::from_str::<Value>(&extra_json)
    {
        adif.extend(extra);
    }

    let id: i64 = row.get("ID")?;
    let log_id: i64 = row.get("LOG_ID")?;
    meta.insert("id".to_string(), Value::Number(id.into()));
    meta.insert("logId".to_string(), Value::Number(log_id.into()));
    meta.insert("status".to_string(), Value::String("Committed".to_string()));

    for column in QSO_COLUMNS {
        if *column == "JSON" || *column == "LOG_ID" {
            continue;
        }
        if INTEGER_COLUMNS.contains(column) {
            let value: Option<i64> = row.get(*column)?;
            if let Some(value) = value {
                adif.insert(column.to_string(), Value::Number(value.into()));
            }
        } else {
            let value: Option<String> = row.get(*column)?;
            if let Some(value) = value {
                adif.insert(column.to_string(), Value::String(value));
            }
        }
    }

    Ok(build_contact(meta, adif))
}

fn json_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(json_i64_value)
}

fn json_i64_value(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().map(|value| value as i64)),
        Value::String(string) => string.parse::<i64>().ok(),
        _ => None,
    }
}

fn frequency_hz(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(decimal_frequency_to_hz)),
        Value::String(string) => {
            if string.contains('.') {
                string.parse::<f64>().ok().map(decimal_frequency_to_hz)
            } else {
                string.parse::<i64>().ok()
            }
        }
        _ => None,
    }
}

fn decimal_frequency_to_hz(value: f64) -> i64 {
    if value.abs() < 1_000_000.0 {
        (value * 1_000_000.0).round() as i64
    } else {
        value.round() as i64
    }
}

fn json_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(string) => Some(string.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    fn tcp_radio() -> NewRadio {
        NewRadio {
            name: "Elecraft TCP".to_string(),
            radio_kind: "elecraft-k4".to_string(),
            transport_kind: "tcp".to_string(),
            tcp_host: "127.0.0.1".to_string(),
            tcp_port: 5002,
            serial_port: String::new(),
            serial_baud_rate: 115_200,
            options: String::new(),
            cw_tuning_increment_hz: DEFAULT_CW_TUNING_INCREMENT_HZ,
            ssb_tuning_increment_hz: DEFAULT_SSB_TUNING_INCREMENT_HZ,
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
        build_contact(
            Map::new(),
            Map::from_iter([
                ("QSO_DATE_TIME_ON".to_string(), json!(1_700_000_000_i64)),
                ("STATION_CALLSIGN".to_string(), json!("N0CALL")),
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
        assert_eq!(radio.cw_tuning_increment_hz, DEFAULT_CW_TUNING_INCREMENT_HZ);
        assert_eq!(
            radio.ssb_tuning_increment_hz,
            DEFAULT_SSB_TUNING_INCREMENT_HZ
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
