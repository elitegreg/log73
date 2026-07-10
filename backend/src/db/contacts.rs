use super::contact::{
    Contact, build_contact, contact_adif, contact_adif_value, contact_id, contact_log_id,
    contact_meta_value, frequency_hz, json_i64, json_string, set_contact_adif, set_contact_meta,
};
use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value};
use std::collections::HashSet;

const QSO_COLUMNS: &[&str] = &[
    "LOG_ID",
    "QSO_DATE_TIME_ON",
    "STATION_CALLSIGN",
    "OPERATOR",
    "CONTEST_ID",
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
    "DXCC_PREFIX",
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
    CONTEST_ID,
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
    DXCC_PREFIX,
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
    ?28,
    ?29
)
"#;

const UPSERT_QSO_SQL: &str = r#"
INSERT INTO qsos (
    ID,
    LOG_ID,
    QSO_DATE_TIME_ON,
    STATION_CALLSIGN,
    OPERATOR,
    CONTEST_ID,
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
    DXCC_PREFIX,
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
    ?28,
    ?29,
    ?30
)
ON CONFLICT(ID) DO UPDATE SET
    LOG_ID = excluded.LOG_ID,
    QSO_DATE_TIME_ON = excluded.QSO_DATE_TIME_ON,
    STATION_CALLSIGN = excluded.STATION_CALLSIGN,
    OPERATOR = excluded.OPERATOR,
    CONTEST_ID = excluded.CONTEST_ID,
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
    DXCC_PREFIX = excluded.DXCC_PREFIX,
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

pub(super) fn db_contacts(connection: &Connection, log_id: i64) -> rusqlite::Result<Vec<Contact>> {
    let mut statement = connection
        .prepare("SELECT * FROM qsos WHERE LOG_ID = ?1 ORDER BY QSO_DATE_TIME_ON DESC, ID DESC")?;
    let rows = statement.query_map(params![log_id], row_to_contact)?;
    rows.collect()
}

pub(super) fn db_upsert_contacts(
    connection: &mut Connection,
    log_id: i64,
    contacts: Vec<Contact>,
) -> rusqlite::Result<Vec<Contact>> {
    let transaction = connection.transaction()?;
    let mut committed = Vec::with_capacity(contacts.len());
    let contest_id = select_log_contest_id(&transaction, log_id)?;

    for mut contact in contacts {
        set_contact_meta(&mut contact, "logId", Value::Number(log_id.into()));
        if contact_adif_value(&contact, "CONTEST_ID")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_none_or(str::is_empty)
        {
            set_contact_adif(
                &mut contact,
                "CONTEST_ID",
                Value::String(contest_id.clone()),
            );
        }
        let id = upsert_contact(&transaction, contact)?;
        if let Some(saved) = select_contact(&transaction, id)? {
            committed.push(saved);
        }
    }

    transaction.commit()?;
    Ok(committed)
}

pub(super) fn db_delete_contact(connection: &Connection, id: i64) -> rusqlite::Result<Option<i64>> {
    let Some(log_id) = select_contact_log_id(connection, id)? else {
        return Ok(None);
    };

    let deleted = connection.execute("DELETE FROM qsos WHERE ID = ?1", params![id])? > 0;
    Ok(deleted.then_some(log_id))
}

pub(super) fn select_contact(
    connection: &Connection,
    id: i64,
) -> rusqlite::Result<Option<Contact>> {
    connection
        .query_row(
            "SELECT * FROM qsos WHERE ID = ?1",
            params![id],
            row_to_contact,
        )
        .optional()
}

pub(super) fn select_contact_log_id(
    connection: &Connection,
    id: i64,
) -> rusqlite::Result<Option<i64>> {
    connection
        .query_row(
            "SELECT LOG_ID FROM qsos WHERE ID = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
}

fn select_log_contest_id(connection: &Connection, log_id: i64) -> rusqlite::Result<String> {
    connection.query_row(
        "SELECT CONTEST_ID FROM logs WHERE ID = ?1",
        params![log_id],
        |row| row.get(0),
    )
}

pub(super) fn qso_column_for_adif(field_adif: &str) -> Option<&'static str> {
    QSO_COLUMNS.iter().copied().find(|column| {
        !matches!(*column, "LOG_ID" | "JSON" | "DXCC_PREFIX")
            && column.eq_ignore_ascii_case(field_adif)
    })
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
            if *column == "DXCC_PREFIX" {
                return contact_meta_value(contact, "DXCC_PREFIX")
                    .and_then(|value| json_string(Some(value)))
                    .map(SqlValue::Text)
                    .unwrap_or(SqlValue::Null);
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
        if *column == "DXCC_PREFIX" {
            let value: Option<String> = row.get(*column)?;
            if let Some(value) = value {
                meta.insert(column.to_string(), Value::String(value));
            }
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

pub(super) fn sql_serial_value(value: ValueRef<'_>) -> Option<i64> {
    match value {
        ValueRef::Integer(value) => Some(value),
        ValueRef::Real(value) if value.is_finite() && value.fract() == 0.0 => Some(value as i64),
        ValueRef::Text(value) => std::str::from_utf8(value).ok()?.trim().parse().ok(),
        _ => None,
    }
}

pub(super) fn json_serial_value(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extra_json_only_keeps_unmapped_adif_fields() {
        let contact = build_contact(
            Map::new(),
            Map::from_iter([
                ("CALL".to_string(), json!("K1ABC")),
                ("CONTEST_ID".to_string(), json!("TEST-CONTEST")),
                ("STX".to_string(), json!(12)),
                ("COMMENT".to_string(), json!("hello")),
                ("CUSTOM_SERIAL".to_string(), json!(88)),
            ]),
        );

        let parsed: Value = serde_json::from_str(&extra_json(&contact)).expect("json parses");
        assert_eq!(parsed.get("COMMENT"), Some(&json!("hello")));
        assert_eq!(parsed.get("CUSTOM_SERIAL"), Some(&json!(88)));
        assert_eq!(parsed.get("CALL"), None);
        assert_eq!(parsed.get("CONTEST_ID"), None);
        assert_eq!(parsed.get("STX"), None);
    }

    #[test]
    fn serial_value_helpers_accept_integer_like_sql_and_json_values() {
        assert_eq!(sql_serial_value(ValueRef::Integer(7)), Some(7));
        assert_eq!(sql_serial_value(ValueRef::Text(b" 42 ")), Some(42));
        assert_eq!(json_serial_value(&json!("99")), Some(99));
        assert_eq!(json_serial_value(&json!(99)), Some(99));
    }
}
