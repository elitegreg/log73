use super::models::{Log, NewLog, UpdateLog};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value};

pub(super) fn db_logs(connection: &Connection) -> rusqlite::Result<Vec<Log>> {
    let mut statement = connection.prepare(
        "SELECT ID, NAME, CONTEST_ID, STATION_CALLSIGN, CONTEST_PARAMS_JSON FROM logs ORDER BY NAME, ID",
    )?;
    let rows = statement.query_map([], row_to_log)?;
    rows.collect()
}

pub(super) fn db_create_log(connection: &Connection, log: NewLog) -> rusqlite::Result<Log> {
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

pub(super) fn db_update_log(
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

pub(super) fn db_delete_log(connection: &Connection, id: i64) -> rusqlite::Result<bool> {
    Ok(connection.execute("DELETE FROM logs WHERE ID = ?1", params![id])? > 0)
}

pub(super) fn db_log_qso_count(connection: &Connection, id: i64) -> rusqlite::Result<usize> {
    connection.query_row(
        "SELECT COUNT(*) FROM qsos WHERE LOG_ID = ?1",
        params![id],
        |row| row.get(0),
    )
}

pub(super) fn select_log(connection: &Connection, id: i64) -> rusqlite::Result<Option<Log>> {
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
