use crate::cw::DEFAULT_CW_MESSAGES;
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::thread;
use tokio::sync::{mpsc, oneshot};

pub type Contact = Map<String, Value>;

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
    pub rigctld_host: String,
    pub rigctld_port: u16,
    pub poll_frequency: f64,
    pub rigctld_timeout: f64,
    pub winkeyer_enabled: bool,
    pub winkeyer_serial_port: String,
    pub cw_messages: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthConfigView {
    pub login_user: String,
    pub login_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub login_user: String,
    pub login_password: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateAuthConfig {
    pub login_user: String,
    pub login_password: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewRadio {
    pub name: String,
    pub rigctld_host: String,
    pub rigctld_port: u16,
    pub poll_frequency: f64,
    pub rigctld_timeout: f64,
    pub winkeyer_enabled: bool,
    pub winkeyer_serial_port: String,
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
    UpdateAuthConfig {
        config: UpdateAuthConfig,
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
    UpsertContacts {
        log_id: i64,
        contacts: Vec<Contact>,
        response: oneshot::Sender<rusqlite::Result<Vec<Contact>>>,
    },
    ContactLogId {
        id: i64,
        response: oneshot::Sender<rusqlite::Result<Option<i64>>>,
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
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let (commands, command_rx) = mpsc::channel(DB_COMMAND_BUFFER);
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let path = path.to_string();

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

    pub async fn auth_config_view(&self) -> rusqlite::Result<AuthConfigView> {
        let config = self.auth_config().await?;
        let login_enabled =
            !config.login_user.trim().is_empty() && !config.login_password.is_empty();
        Ok(AuthConfigView {
            login_user: config.login_user,
            login_enabled,
        })
    }

    pub async fn update_auth_config(&self, config: UpdateAuthConfig) -> rusqlite::Result<()> {
        self.call(|response| DbCommand::UpdateAuthConfig { config, response })
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
            DbCommand::UpdateAuthConfig { config, response } => {
                let _ = response.send(db_update_auth_config(&connection, config));
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
        "SELECT ID, NAME, RIGCTLD_HOST, RIGCTLD_PORT, POLL_FREQUENCY, RIGCTLD_TIMEOUT, WINKEYER_ENABLED, WINKEYER_SERIAL_PORT, CW_MESSAGES FROM radios ORDER BY ID",
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

fn db_update_auth_config(
    connection: &Connection,
    config: UpdateAuthConfig,
) -> rusqlite::Result<()> {
    let updated = connection.execute(
        "UPDATE config SET LOGIN_USER = ?1, LOGIN_PASSWORD = ?2",
        params![config.login_user.trim(), config.login_password],
    )?;
    if updated == 0 {
        connection.execute(
            "INSERT INTO config (version, LOGIN_USER, LOGIN_PASSWORD) VALUES (1, ?1, ?2)",
            params![config.login_user.trim(), config.login_password],
        )?;
    }
    Ok(())
}

fn db_create_radio(connection: &Connection, radio: NewRadio) -> rusqlite::Result<RadioConfig> {
    connection.execute(
        "INSERT INTO radios (NAME, RIGCTLD_HOST, RIGCTLD_PORT, POLL_FREQUENCY, RIGCTLD_TIMEOUT, WINKEYER_ENABLED, WINKEYER_SERIAL_PORT, CW_MESSAGES) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            radio.name.trim(),
            radio.rigctld_host.trim(),
            radio.rigctld_port,
            radio.poll_frequency,
            radio.rigctld_timeout,
            radio.winkeyer_enabled,
            radio.winkeyer_serial_port.trim(),
            DEFAULT_CW_MESSAGES
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
        "UPDATE radios SET NAME = ?1, RIGCTLD_HOST = ?2, RIGCTLD_PORT = ?3, POLL_FREQUENCY = ?4, RIGCTLD_TIMEOUT = ?5, WINKEYER_ENABLED = ?6, WINKEYER_SERIAL_PORT = ?7 WHERE ID = ?8",
        params![
            radio.name.trim(),
            radio.rigctld_host.trim(),
            radio.rigctld_port,
            radio.poll_frequency,
            radio.rigctld_timeout,
            radio.winkeyer_enabled,
            radio.winkeyer_serial_port.trim(),
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
        contact.insert("_log_id".to_string(), Value::Number(log_id.into()));
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
            LOGIN_PASSWORD TEXT NOT NULL DEFAULT ''
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
            RIGCTLD_HOST TEXT NOT NULL,
            RIGCTLD_PORT INTEGER NOT NULL CHECK (RIGCTLD_PORT >= 0 AND RIGCTLD_PORT <= 65535),
            POLL_FREQUENCY REAL NOT NULL DEFAULT 0.25 CHECK (POLL_FREQUENCY > 0),
            RIGCTLD_TIMEOUT REAL NOT NULL DEFAULT 2.0 CHECK (RIGCTLD_TIMEOUT > 0),
            WINKEYER_ENABLED INTEGER NOT NULL DEFAULT 0,
            WINKEYER_SERIAL_PORT TEXT NOT NULL DEFAULT '',
            CW_MESSAGES TEXT NOT NULL
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
            "SELECT ID, NAME, RIGCTLD_HOST, RIGCTLD_PORT, POLL_FREQUENCY, RIGCTLD_TIMEOUT, WINKEYER_ENABLED, WINKEYER_SERIAL_PORT, CW_MESSAGES FROM radios WHERE ID = ?1",
            params![id],
            row_to_radio,
        )
        .optional()
}

fn row_to_radio(row: &rusqlite::Row<'_>) -> rusqlite::Result<RadioConfig> {
    let port: i64 = row.get("RIGCTLD_PORT")?;
    Ok(RadioConfig {
        id: row.get("ID")?,
        name: row.get("NAME")?,
        rigctld_host: row.get("RIGCTLD_HOST")?,
        rigctld_port: port as u16,
        poll_frequency: row.get("POLL_FREQUENCY")?,
        rigctld_timeout: row.get("RIGCTLD_TIMEOUT")?,
        winkeyer_enabled: row.get("WINKEYER_ENABLED")?,
        winkeyer_serial_port: row.get("WINKEYER_SERIAL_PORT")?,
        cw_messages: row.get("CW_MESSAGES")?,
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

fn upsert_contact(connection: &Connection, contact: Contact) -> rusqlite::Result<i64> {
    let id = json_i64(contact.get("_id")).or_else(|| json_i64(contact.get("ID")));
    let requested_log_id = json_i64(contact.get("_log_id")).unwrap_or(1);

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
                return SqlValue::Integer(json_i64(contact.get("_log_id")).unwrap_or(1));
            }
            if *column == "QSO_DATE_TIME_ON" {
                return json_i64(contact.get("QSO_DATE_TIME_ON"))
                    .or_else(|| legacy_epoch(contact))
                    .map(SqlValue::Integer)
                    .unwrap_or(SqlValue::Null);
            }
            if *column == "FREQ" {
                return frequency_hz(contact.get("FREQ"))
                    .map(SqlValue::Integer)
                    .unwrap_or(SqlValue::Null);
            }

            let value = contact.get(*column);
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
    let extra = contact
        .iter()
        .filter(|(key, _)| !key.starts_with('_') && !mapped_keys.contains(key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Map<_, _>>();
    Value::Object(extra).to_string()
}

fn mapped_json_keys() -> HashSet<&'static str> {
    let mut keys = HashSet::from(["_id", "ID", "_log_id", "_status"]);
    for column in QSO_COLUMNS {
        if *column != "JSON" && *column != "LOG_ID" {
            keys.insert(column);
        }
    }
    keys
}

fn row_to_contact(row: &rusqlite::Row<'_>) -> rusqlite::Result<Contact> {
    let mut contact = Map::new();

    let extra_json: Option<String> = row.get("JSON")?;
    if let Some(extra_json) = extra_json
        && let Ok(Value::Object(extra)) = serde_json::from_str::<Value>(&extra_json)
    {
        contact.extend(extra);
    }

    let id: i64 = row.get("ID")?;
    let log_id: i64 = row.get("LOG_ID")?;
    contact.insert("_id".to_string(), Value::Number(id.into()));
    contact.insert("_log_id".to_string(), Value::Number(log_id.into()));
    contact.insert(
        "_status".to_string(),
        Value::String("Committed".to_string()),
    );

    for column in QSO_COLUMNS {
        if *column == "JSON" || *column == "LOG_ID" {
            continue;
        }
        if INTEGER_COLUMNS.contains(column) {
            let value: Option<i64> = row.get(*column)?;
            if let Some(value) = value {
                contact.insert(column.to_string(), Value::Number(value.into()));
            }
        } else {
            let value: Option<String> = row.get(*column)?;
            if let Some(value) = value {
                contact.insert(column.to_string(), Value::String(value));
            }
        }
    }

    Ok(contact)
}

fn json_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().map(|value| value as i64)),
        Value::String(string) => string.parse::<i64>().ok(),
        _ => None,
    }
}

fn legacy_epoch(contact: &Contact) -> Option<i64> {
    let date = contact.get("QSO_DATE")?.as_str()?;
    let time = contact.get("TIME_ON")?.as_str()?;
    if date.len() != 8 || time.len() != 6 {
        return None;
    }
    let year = date[0..4].parse::<i32>().ok()?;
    let month = date[4..6].parse::<u32>().ok()?;
    let day = date[6..8].parse::<u32>().ok()?;
    let hour = time[0..2].parse::<u32>().ok()?;
    let minute = time[2..4].parse::<u32>().ok()?;
    let second = time[4..6].parse::<u32>().ok()?;
    let days = days_from_civil(year, month, day)?;
    Some(days * 86_400 + i64::from(hour * 3_600 + minute * 60 + second))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(i64::from(era * 146_097 + doe - 719_468))
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

    fn base_contact() -> Contact {
        Map::from_iter([
            ("QSO_DATE_TIME_ON".to_string(), json!(1_700_000_000_i64)),
            ("STATION_CALLSIGN".to_string(), json!("N0CALL")),
            ("CALL".to_string(), json!("K1ABC")),
            ("BAND".to_string(), json!("20m")),
            ("FREQ".to_string(), json!(14_074_000_i64)),
            ("MODE".to_string(), json!("FT8")),
        ])
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
        assert!(saved[0].get("_id").and_then(Value::as_i64).is_some());
        assert_eq!(
            saved[0].get("_log_id").and_then(Value::as_i64),
            Some(log.id)
        );
        assert_eq!(saved[0].get("CALL").and_then(Value::as_str), Some("K1ABC"));

        let contacts = database
            .contacts(log.id)
            .await
            .expect("contacts are listed");
        assert_eq!(contacts.len(), 1);
    }

    #[tokio::test]
    async fn upsert_contacts_updates_existing_contact() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let inserted = database
            .upsert_contacts(log.id, vec![base_contact()])
            .await
            .expect("contact is inserted");
        let contact_id = inserted[0]
            .get("_id")
            .and_then(Value::as_i64)
            .expect("inserted contact has an id");

        let mut updated_contact = base_contact();
        updated_contact.insert("_id".to_string(), json!(contact_id));
        updated_contact.insert("CALL".to_string(), json!("W9XYZ"));
        updated_contact.insert("COMMENT".to_string(), json!("updated"));

        let updated = database
            .upsert_contacts(log.id, vec![updated_contact])
            .await
            .expect("contact is updated");

        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0].get("_id").and_then(Value::as_i64),
            Some(contact_id)
        );
        assert_eq!(
            updated[0].get("CALL").and_then(Value::as_str),
            Some("W9XYZ")
        );
        assert_eq!(
            updated[0].get("COMMENT").and_then(Value::as_str),
            Some("updated")
        );

        let contacts = database
            .contacts(log.id)
            .await
            .expect("contacts are listed");
        assert_eq!(contacts.len(), 1);
        assert_eq!(
            contacts[0].get("CALL").and_then(Value::as_str),
            Some("W9XYZ")
        );
    }

    #[tokio::test]
    async fn upsert_contacts_treats_sql_like_values_as_data() {
        let database = test_database();
        let log = create_test_log(&database).await;
        let mut contact = base_contact();
        let sql_like_call = "K1ABC'); DROP TABLE logs; --";
        contact.insert("CALL".to_string(), json!(sql_like_call));

        let saved = database
            .upsert_contacts(log.id, vec![contact])
            .await
            .expect("contact with sql-like value is inserted");

        assert_eq!(
            saved[0].get("CALL").and_then(Value::as_str),
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
        let contact_id = inserted[0]
            .get("_id")
            .and_then(Value::as_i64)
            .expect("inserted contact has an id");

        let mut attempted_update = base_contact();
        attempted_update.insert("_id".to_string(), json!(contact_id));
        attempted_update.insert("CALL".to_string(), json!("W9XYZ"));

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
            first_log_contacts[0].get("CALL").and_then(Value::as_str),
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
}
