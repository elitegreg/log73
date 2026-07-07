use crate::bands::Band;
use rusqlite::{Connection, params};

pub(super) fn db_bands(connection: &Connection, iaru_region: i64) -> rusqlite::Result<Vec<Band>> {
    let mut statement = connection.prepare(
        "SELECT IARU_REGION, NAME, LOWER_HZ, UPPER_HZ, DEFAULT_SSB_MODE, SORT_ORDER
         FROM bands
         WHERE IARU_REGION = ?1
         ORDER BY SORT_ORDER, LOWER_HZ, NAME",
    )?;
    let rows = statement.query_map(params![iaru_region], |row| {
        Ok(Band {
            iaru_region: row.get("IARU_REGION")?,
            name: row.get("NAME")?,
            lower_hz: row.get("LOWER_HZ")?,
            upper_hz: row.get("UPPER_HZ")?,
            default_ssb_mode: row.get("DEFAULT_SSB_MODE")?,
            sort_order: row.get("SORT_ORDER")?,
        })
    })?;
    rows.collect()
}
