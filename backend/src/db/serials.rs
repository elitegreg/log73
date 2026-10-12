use super::models::SerialAllocation;
use rusqlite::{Connection, params};

/// The worker calculates the value from its committed contact/reservation indexes.
pub(super) fn db_allocate_serial(
    connection: &mut Connection,
    log_id: i64,
    field_adif: &str,
    serial: i64,
) -> rusqlite::Result<SerialAllocation> {
    let next_serial = serial.checked_add(1).ok_or_else(|| {
        rusqlite::Error::InvalidParameterName("serial allocation overflow".to_string())
    })?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO log_serial_state (LOG_ID, FIELD_ADIF, NEXT_SERIAL) VALUES (?1, ?2, ?3)
         ON CONFLICT(LOG_ID, FIELD_ADIF) DO UPDATE SET NEXT_SERIAL = excluded.NEXT_SERIAL",
        params![log_id, field_adif, next_serial],
    )?;
    transaction.commit()?;
    Ok(SerialAllocation {
        log_id,
        field_adif: field_adif.to_string(),
        serial,
    })
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
    async fn allocate_serial_reuses_larger_of_stored_and_committed_next_values() {
        let database = test_database();
        let log_id = create_test_log(&database).await;

        let mut stx_contact = base_contact();
        set_contact_adif(&mut stx_contact, "STX", json!(42));
        database
            .upsert_contacts(log_id, vec![stx_contact])
            .await
            .expect("contact is inserted");

        let first = database
            .allocate_serial(log_id, "STX".to_string())
            .await
            .expect("serial allocated");
        assert_eq!(first.serial, 43);

        let second = database
            .allocate_serial(log_id, "STX".to_string())
            .await
            .expect("next serial allocated");
        assert_eq!(second.serial, 44);
    }
}
