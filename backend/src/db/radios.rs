use super::models::{RadioConfig, RadioPayload};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn normalized_optional_device_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn db_radios(connection: &Connection) -> rusqlite::Result<Vec<RadioConfig>> {
    let mut statement = connection.prepare(
        "SELECT ID, NAME, RADIO_KIND, TRANSPORT_KIND, TCP_HOST, TCP_PORT, SERIAL_PORT, SERIAL_BAUD_RATE, OPTIONS, CW_TUNING_INCREMENT_HZ, SSB_TUNING_INCREMENT_HZ, RIT_CLEAR_ON_LOG, VOICE_INPUT_DEVICE_ID, VOICE_OUTPUT_DEVICE_ID, CW_KEYER_TYPE, WINKEYER_SERIAL_PORT, CW_SERIAL_PORT, CW_SERIAL_BAUD_RATE, CW_SERIAL_LINE, CW_MESSAGES, VOICE_MESSAGES FROM radios ORDER BY ID",
    )?;
    let rows = statement.query_map([], row_to_radio)?;
    rows.collect()
}

pub(super) fn db_create_radio(
    connection: &Connection,
    radio: RadioPayload,
) -> rusqlite::Result<RadioConfig> {
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

pub(super) fn db_update_radio(
    connection: &Connection,
    id: i64,
    radio: RadioPayload,
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

pub(super) fn db_delete_radio(connection: &Connection, id: i64) -> rusqlite::Result<bool> {
    Ok(connection.execute("DELETE FROM radios WHERE ID = ?1", params![id])? > 0)
}

pub(super) fn select_radio(
    connection: &Connection,
    id: i64,
) -> rusqlite::Result<Option<RadioConfig>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_optional_device_id_trims_and_rejects_blanks() {
        assert_eq!(
            normalized_optional_device_id(Some(" alsa:hw:1,0 ")),
            Some("alsa:hw:1,0".to_string())
        );
        assert_eq!(normalized_optional_device_id(Some("   ")), None);
        assert_eq!(normalized_optional_device_id(None), None);
    }
}
