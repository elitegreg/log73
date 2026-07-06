use super::contacts::{json_serial_value, qso_column_for_adif, sql_serial_value};
use super::models::SerialAllocation;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

pub(super) fn db_allocate_serials(
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

#[cfg(test)]
mod tests {
    use crate::db::{Database, NewLog, build_contact, set_contact_adif};
    use serde_json::{Map, Value, json};

    fn test_database() -> Database {
        Database::open(":memory:").expect("in-memory database opens")
    }

    async fn create_test_log(database: &Database) -> i64 {
        database
            .create_log(NewLog {
                name: "Test log".to_string(),
                contest_id: "test-contest".to_string(),
                station_callsign: "N0CALL".to_string(),
                contest_params: Value::Object(Map::new()),
            })
            .await
            .expect("test log is created")
            .id
    }

    fn base_contact() -> crate::db::Contact {
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
    async fn allocate_serials_reuses_larger_of_stored_and_committed_next_values() {
        let database = test_database();
        let log_id = create_test_log(&database).await;

        let mut stx_contact = base_contact();
        set_contact_adif(&mut stx_contact, "STX", json!(42));
        database
            .upsert_contacts(log_id, vec![stx_contact])
            .await
            .expect("contact is inserted");

        let first = database
            .allocate_serials(log_id, "STX".to_string(), 3)
            .await
            .expect("serials allocated");
        assert_eq!((first.start, first.end), (43, 45));

        let second = database
            .allocate_serials(log_id, "STX".to_string(), 2)
            .await
            .expect("next serials allocated");
        assert_eq!((second.start, second.end), (46, 47));
    }
}
