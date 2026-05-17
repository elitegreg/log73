use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub type Contact = Map<String, Value>;

#[derive(Debug, Clone, Serialize)]
pub struct Log {
    pub id: i64,
    pub name: String,
    pub contest_id: String,
    pub station_callsign: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewLog {
    pub name: String,
    pub contest_id: String,
    pub station_callsign: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RadioConfig {
    pub id: i64,
    pub name: String,
    pub rigctld_host: String,
    pub rigctld_port: u16,
    pub poll_frequency: f64,
    pub rigctld_timeout: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewRadio {
    pub name: String,
    pub rigctld_host: String,
    pub rigctld_port: u16,
    pub poll_frequency: f64,
    pub rigctld_timeout: f64,
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

#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        initialize_schema(&connection)?;

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn logs(&self) -> rusqlite::Result<Vec<Log>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let mut statement = connection
            .prepare("SELECT ID, NAME, CONTEST_ID, STATION_CALLSIGN FROM logs ORDER BY NAME, ID")?;
        let rows = statement.query_map([], row_to_log)?;
        rows.collect()
    }

    pub fn log(&self, id: i64) -> rusqlite::Result<Option<Log>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        select_log(&connection, id)
    }

    pub fn create_log(&self, log: NewLog) -> rusqlite::Result<Log> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection.execute(
            "INSERT INTO logs (NAME, CONTEST_ID, STATION_CALLSIGN) VALUES (?1, ?2, ?3)",
            params![
                log.name.trim(),
                log.contest_id.trim(),
                log.station_callsign.trim().to_uppercase()
            ],
        )?;
        select_log(&connection, connection.last_insert_rowid())?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn delete_log(&self, id: i64) -> rusqlite::Result<bool> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let qso_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM qsos WHERE LOG_ID = ?1",
            params![id],
            |row| row.get(0),
        )?;
        if qso_count > 0 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(connection.execute("DELETE FROM logs WHERE ID = ?1", params![id])? > 0)
    }

    pub fn radios(&self) -> rusqlite::Result<Vec<RadioConfig>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT ID, NAME, RIGCTLD_HOST, RIGCTLD_PORT, POLL_FREQUENCY, RIGCTLD_TIMEOUT FROM radios ORDER BY ID",
        )?;
        let rows = statement.query_map([], row_to_radio)?;
        rows.collect()
    }

    pub fn radio(&self, id: i64) -> rusqlite::Result<Option<RadioConfig>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        select_radio(&connection, id)
    }

    pub fn create_radio(&self, radio: NewRadio) -> rusqlite::Result<RadioConfig> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection.execute(
            "INSERT INTO radios (NAME, RIGCTLD_HOST, RIGCTLD_PORT, POLL_FREQUENCY, RIGCTLD_TIMEOUT) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                radio.name.trim(),
                radio.rigctld_host.trim(),
                radio.rigctld_port,
                radio.poll_frequency,
                radio.rigctld_timeout
            ],
        )?;
        select_radio(&connection, connection.last_insert_rowid())?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn delete_radio(&self, id: i64) -> rusqlite::Result<bool> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        Ok(connection.execute("DELETE FROM radios WHERE ID = ?1", params![id])? > 0)
    }

    pub fn contacts(&self, log_id: i64) -> rusqlite::Result<Vec<Contact>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT * FROM qsos WHERE LOG_ID = ?1 ORDER BY QSO_DATE_TIME_ON DESC, ID DESC",
        )?;
        let rows = statement.query_map(params![log_id], row_to_contact)?;
        rows.collect()
    }

    pub fn upsert_contacts(
        &self,
        log_id: i64,
        contacts: Vec<Contact>,
    ) -> rusqlite::Result<Vec<Contact>> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
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

    pub fn delete_contact(&self, id: i64) -> rusqlite::Result<bool> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        Ok(connection.execute("DELETE FROM qsos WHERE ID = ?1", params![id])? > 0)
    }
}

fn initialize_schema(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS config (
            version INTEGER NOT NULL
        ) STRICT;

        CREATE TABLE IF NOT EXISTS logs (
            ID INTEGER PRIMARY KEY,
            NAME TEXT NOT NULL,
            CONTEST_ID TEXT NOT NULL,
            STATION_CALLSIGN TEXT NOT NULL
        ) STRICT;

        CREATE TABLE IF NOT EXISTS radios (
            ID INTEGER PRIMARY KEY,
            NAME TEXT NOT NULL,
            RIGCTLD_HOST TEXT NOT NULL,
            RIGCTLD_PORT INTEGER NOT NULL CHECK (RIGCTLD_PORT >= 0 AND RIGCTLD_PORT <= 65535),
            POLL_FREQUENCY REAL NOT NULL DEFAULT 0.25 CHECK (POLL_FREQUENCY > 0),
            RIGCTLD_TIMEOUT REAL NOT NULL DEFAULT 2.0 CHECK (RIGCTLD_TIMEOUT > 0)
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

fn select_log(connection: &Connection, id: i64) -> rusqlite::Result<Option<Log>> {
    connection
        .query_row(
            "SELECT ID, NAME, CONTEST_ID, STATION_CALLSIGN FROM logs WHERE ID = ?1",
            params![id],
            row_to_log,
        )
        .optional()
}

fn row_to_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<Log> {
    Ok(Log {
        id: row.get("ID")?,
        name: row.get("NAME")?,
        contest_id: row.get("CONTEST_ID")?,
        station_callsign: row.get("STATION_CALLSIGN")?,
    })
}

fn select_radio(connection: &Connection, id: i64) -> rusqlite::Result<Option<RadioConfig>> {
    connection
        .query_row(
            "SELECT ID, NAME, RIGCTLD_HOST, RIGCTLD_PORT, POLL_FREQUENCY, RIGCTLD_TIMEOUT FROM radios WHERE ID = ?1",
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

fn upsert_contact(connection: &Connection, contact: Contact) -> rusqlite::Result<i64> {
    let id = json_i64(contact.get("_id")).or_else(|| json_i64(contact.get("ID")));
    let values = contact_to_sql_values(&contact);
    let placeholders = QSO_COLUMNS
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(", ");
    let update_assignments = QSO_COLUMNS
        .iter()
        .map(|column| format!("{column} = excluded.{column}"))
        .collect::<Vec<_>>()
        .join(", ");

    let mut sql_values = Vec::new();
    let sql = if let Some(id) = id {
        sql_values.push(SqlValue::Integer(id));
        sql_values.extend(values);
        format!(
            "INSERT INTO qsos (ID, {}) VALUES (?, {}) ON CONFLICT(ID) DO UPDATE SET {}",
            QSO_COLUMNS.join(", "),
            placeholders,
            update_assignments,
        )
    } else {
        sql_values.extend(values);
        format!(
            "INSERT INTO qsos ({}) VALUES ({})",
            QSO_COLUMNS.join(", "),
            placeholders
        )
    };

    connection.execute(&sql, rusqlite::params_from_iter(sql_values))?;
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
    if let Some(extra_json) = extra_json {
        if let Ok(Value::Object(extra)) = serde_json::from_str::<Value>(&extra_json) {
            contact.extend(extra);
        }
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
