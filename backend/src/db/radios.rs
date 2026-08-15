use super::models::{RadioConfig, RadioControlLocation, RadioPayload, RadioRecord};
use rusqlite::{Connection, OptionalExtension, params};

const SELECT_RADIOS: &str = "SELECT ID, NAME, RADIO_KIND, TRANSPORT_KIND, TCP_HOST, TCP_PORT, SERIAL_PORT, SERIAL_BAUD_RATE, OPTIONS, DATA_MODE, RTTY_MODE, DIGITAL_PROGRAM, WSJTX_ENABLED, WSJTX_BIND_ADDRESS, WSJTX_PORT, WSJTX_MULTICAST_GROUP, FLDIGI_DATA_ENABLED, FLDIGI_RTTY_ENABLED, FLDIGI_HOST, FLDIGI_PORT, FLRIG_ENABLED, FLRIG_PORT, CW_TUNING_INCREMENT_HZ, SSB_TUNING_INCREMENT_HZ, RIT_CLEAR_ON_LOG, VOICE_INPUT_DEVICE_ID, VOICE_OUTPUT_DEVICE_ID, CW_KEYER_TYPE, WINKEYER_SERIAL_PORT, CW_SERIAL_PORT, CW_SERIAL_BAUD_RATE, CW_SERIAL_LINE, CW_MESSAGES, VOICE_MESSAGES, CONTROL_LOCATION, CLIENT_INSTANCE_ID, RADIO_WS_URL FROM radios ORDER BY ID";
const SELECT_RADIO: &str = "SELECT ID, NAME, RADIO_KIND, TRANSPORT_KIND, TCP_HOST, TCP_PORT, SERIAL_PORT, SERIAL_BAUD_RATE, OPTIONS, DATA_MODE, RTTY_MODE, DIGITAL_PROGRAM, WSJTX_ENABLED, WSJTX_BIND_ADDRESS, WSJTX_PORT, WSJTX_MULTICAST_GROUP, FLDIGI_DATA_ENABLED, FLDIGI_RTTY_ENABLED, FLDIGI_HOST, FLDIGI_PORT, FLRIG_ENABLED, FLRIG_PORT, CW_TUNING_INCREMENT_HZ, SSB_TUNING_INCREMENT_HZ, RIT_CLEAR_ON_LOG, VOICE_INPUT_DEVICE_ID, VOICE_OUTPUT_DEVICE_ID, CW_KEYER_TYPE, WINKEYER_SERIAL_PORT, CW_SERIAL_PORT, CW_SERIAL_BAUD_RATE, CW_SERIAL_LINE, CW_MESSAGES, VOICE_MESSAGES, CONTROL_LOCATION, CLIENT_INSTANCE_ID, RADIO_WS_URL FROM radios WHERE ID = ?1";
const SELECT_CLIENT_RADIO: &str = "SELECT ID, NAME, RADIO_KIND, TRANSPORT_KIND, TCP_HOST, TCP_PORT, SERIAL_PORT, SERIAL_BAUD_RATE, OPTIONS, DATA_MODE, RTTY_MODE, DIGITAL_PROGRAM, WSJTX_ENABLED, WSJTX_BIND_ADDRESS, WSJTX_PORT, WSJTX_MULTICAST_GROUP, FLDIGI_DATA_ENABLED, FLDIGI_RTTY_ENABLED, FLDIGI_HOST, FLDIGI_PORT, FLRIG_ENABLED, FLRIG_PORT, CW_TUNING_INCREMENT_HZ, SSB_TUNING_INCREMENT_HZ, RIT_CLEAR_ON_LOG, VOICE_INPUT_DEVICE_ID, VOICE_OUTPUT_DEVICE_ID, CW_KEYER_TYPE, WINKEYER_SERIAL_PORT, CW_SERIAL_PORT, CW_SERIAL_BAUD_RATE, CW_SERIAL_LINE, CW_MESSAGES, VOICE_MESSAGES, CONTROL_LOCATION, CLIENT_INSTANCE_ID, RADIO_WS_URL FROM radios WHERE CONTROL_LOCATION = 'client' AND CLIENT_INSTANCE_ID = ?1";
const SELECT_BACKEND_RADIOS: &str = "SELECT ID, NAME, RADIO_KIND, TRANSPORT_KIND, TCP_HOST, TCP_PORT, SERIAL_PORT, SERIAL_BAUD_RATE, OPTIONS, DATA_MODE, RTTY_MODE, DIGITAL_PROGRAM, WSJTX_ENABLED, WSJTX_BIND_ADDRESS, WSJTX_PORT, WSJTX_MULTICAST_GROUP, FLDIGI_DATA_ENABLED, FLDIGI_RTTY_ENABLED, FLDIGI_HOST, FLDIGI_PORT, FLRIG_ENABLED, FLRIG_PORT, CW_TUNING_INCREMENT_HZ, SSB_TUNING_INCREMENT_HZ, RIT_CLEAR_ON_LOG, VOICE_INPUT_DEVICE_ID, VOICE_OUTPUT_DEVICE_ID, CW_KEYER_TYPE, WINKEYER_SERIAL_PORT, CW_SERIAL_PORT, CW_SERIAL_BAUD_RATE, CW_SERIAL_LINE, CW_MESSAGES, VOICE_MESSAGES, CONTROL_LOCATION, CLIENT_INSTANCE_ID, RADIO_WS_URL FROM radios WHERE CONTROL_LOCATION = 'backend' ORDER BY ID";

pub(super) fn normalized_optional_device_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn db_radios(connection: &Connection) -> rusqlite::Result<Vec<RadioRecord>> {
    let mut statement = connection.prepare(SELECT_RADIOS)?;
    let rows = statement.query_map([], row_to_radio)?;
    rows.collect()
}

pub(super) fn db_backend_radio_configs(
    connection: &Connection,
) -> rusqlite::Result<Vec<RadioConfig>> {
    let mut statement = connection.prepare(SELECT_BACKEND_RADIOS)?;
    let rows = statement.query_map([], row_to_radio)?;
    rows.map(|record| record.map(|record| record.config))
        .collect()
}

pub(super) fn db_create_radio(
    connection: &Connection,
    radio: RadioPayload,
) -> rusqlite::Result<RadioRecord> {
    connection.execute(
        "INSERT INTO radios (NAME, RADIO_KIND, TRANSPORT_KIND, TCP_HOST, TCP_PORT, SERIAL_PORT, SERIAL_BAUD_RATE, OPTIONS, DATA_MODE, RTTY_MODE, DIGITAL_PROGRAM, WSJTX_ENABLED, WSJTX_BIND_ADDRESS, WSJTX_PORT, WSJTX_MULTICAST_GROUP, FLDIGI_DATA_ENABLED, FLDIGI_RTTY_ENABLED, FLDIGI_HOST, FLDIGI_PORT, FLRIG_ENABLED, FLRIG_PORT, CW_TUNING_INCREMENT_HZ, SSB_TUNING_INCREMENT_HZ, RIT_CLEAR_ON_LOG, VOICE_INPUT_DEVICE_ID, VOICE_OUTPUT_DEVICE_ID, CW_KEYER_TYPE, WINKEYER_SERIAL_PORT, CW_SERIAL_PORT, CW_SERIAL_BAUD_RATE, CW_SERIAL_LINE, CW_MESSAGES, VOICE_MESSAGES) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33)",
        params![
            radio.name.trim(),
            radio.radio_kind.trim(),
            radio.transport_kind.trim(),
            radio.tcp_host.trim(),
            radio.tcp_port,
            radio.serial_port.trim(),
            radio.serial_baud_rate,
            radio.options,
            radio.data_mode,
            radio.rtty_mode,
            radio.digital_program,
            radio.wsjtx_enabled,
            radio.wsjtx_bind_address.trim(),
            radio.wsjtx_port,
            radio.wsjtx_multicast_group.trim(),
            radio.fldigi_data_enabled,
            radio.fldigi_rtty_enabled,
            radio.fldigi_host.trim(),
            radio.fldigi_port,
            radio.flrig_enabled,
            radio.flrig_port,
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
) -> rusqlite::Result<Option<RadioRecord>> {
    let updated = connection.execute(
        "UPDATE radios SET NAME = ?1, RADIO_KIND = ?2, TRANSPORT_KIND = ?3, TCP_HOST = ?4, TCP_PORT = ?5, SERIAL_PORT = ?6, SERIAL_BAUD_RATE = ?7, OPTIONS = ?8, DATA_MODE = ?9, RTTY_MODE = ?10, DIGITAL_PROGRAM = ?11, WSJTX_ENABLED = ?12, WSJTX_BIND_ADDRESS = ?13, WSJTX_PORT = ?14, WSJTX_MULTICAST_GROUP = ?15, FLDIGI_DATA_ENABLED = ?16, FLDIGI_RTTY_ENABLED = ?17, FLDIGI_HOST = ?18, FLDIGI_PORT = ?19, FLRIG_ENABLED = ?20, FLRIG_PORT = ?21, CW_TUNING_INCREMENT_HZ = ?22, SSB_TUNING_INCREMENT_HZ = ?23, RIT_CLEAR_ON_LOG = ?24, VOICE_INPUT_DEVICE_ID = ?25, VOICE_OUTPUT_DEVICE_ID = ?26, CW_KEYER_TYPE = ?27, WINKEYER_SERIAL_PORT = ?28, CW_SERIAL_PORT = ?29, CW_SERIAL_BAUD_RATE = ?30, CW_SERIAL_LINE = ?31, CW_MESSAGES = ?32, VOICE_MESSAGES = ?33 WHERE ID = ?34 AND CONTROL_LOCATION = 'backend'",
        params![
            radio.name.trim(),
            radio.radio_kind.trim(),
            radio.transport_kind.trim(),
            radio.tcp_host.trim(),
            radio.tcp_port,
            radio.serial_port.trim(),
            radio.serial_baud_rate,
            radio.options,
            radio.data_mode,
            radio.rtty_mode,
            radio.digital_program,
            radio.wsjtx_enabled,
            radio.wsjtx_bind_address.trim(),
            radio.wsjtx_port,
            radio.wsjtx_multicast_group.trim(),
            radio.fldigi_data_enabled,
            radio.fldigi_rtty_enabled,
            radio.fldigi_host.trim(),
            radio.fldigi_port,
            radio.flrig_enabled,
            radio.flrig_port,
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
) -> rusqlite::Result<Option<RadioRecord>> {
    connection
        .query_row(SELECT_RADIO, params![id], row_to_radio)
        .optional()
}

pub(super) fn select_client_radio(
    connection: &Connection,
    client_instance_id: &str,
) -> rusqlite::Result<Option<RadioRecord>> {
    connection
        .query_row(
            SELECT_CLIENT_RADIO,
            params![client_instance_id],
            row_to_radio,
        )
        .optional()
}

pub(super) fn db_upsert_client_radio(
    connection: &Connection,
    client_instance_id: &str,
    radio_ws_url: &str,
    radio: RadioPayload,
) -> rusqlite::Result<RadioRecord> {
    if let Some(existing) = select_client_radio(connection, client_instance_id)? {
        connection.execute(
            "UPDATE radios SET NAME = ?1, RADIO_KIND = ?2, TRANSPORT_KIND = ?3, TCP_HOST = ?4, TCP_PORT = ?5, SERIAL_PORT = ?6, SERIAL_BAUD_RATE = ?7, OPTIONS = ?8, DATA_MODE = ?9, RTTY_MODE = ?10, DIGITAL_PROGRAM = ?11, WSJTX_ENABLED = ?12, WSJTX_BIND_ADDRESS = ?13, WSJTX_PORT = ?14, WSJTX_MULTICAST_GROUP = ?15, FLDIGI_DATA_ENABLED = ?16, FLDIGI_RTTY_ENABLED = ?17, FLDIGI_HOST = ?18, FLDIGI_PORT = ?19, FLRIG_ENABLED = ?20, FLRIG_PORT = ?21, CW_TUNING_INCREMENT_HZ = ?22, SSB_TUNING_INCREMENT_HZ = ?23, RIT_CLEAR_ON_LOG = ?24, VOICE_INPUT_DEVICE_ID = ?25, VOICE_OUTPUT_DEVICE_ID = ?26, CW_KEYER_TYPE = ?27, WINKEYER_SERIAL_PORT = ?28, CW_SERIAL_PORT = ?29, CW_SERIAL_BAUD_RATE = ?30, CW_SERIAL_LINE = ?31, CW_MESSAGES = ?32, VOICE_MESSAGES = ?33, RADIO_WS_URL = ?34 WHERE ID = ?35 AND CONTROL_LOCATION = 'client'",
            params![radio.name.trim(), radio.radio_kind.trim(), radio.transport_kind.trim(), radio.tcp_host.trim(), radio.tcp_port, radio.serial_port.trim(), radio.serial_baud_rate, radio.options, radio.data_mode, radio.rtty_mode, radio.digital_program, radio.wsjtx_enabled, radio.wsjtx_bind_address.trim(), radio.wsjtx_port, radio.wsjtx_multicast_group.trim(), radio.fldigi_data_enabled, radio.fldigi_rtty_enabled, radio.fldigi_host.trim(), radio.fldigi_port, radio.flrig_enabled, radio.flrig_port, radio.cw_tuning_increment_hz, radio.ssb_tuning_increment_hz, radio.rit_clear_on_log, normalized_optional_device_id(radio.voice_input_device_id.as_deref()), normalized_optional_device_id(radio.voice_output_device_id.as_deref()), radio.cw_keyer_type.trim(), radio.winkeyer_serial_port.trim(), radio.cw_serial_port.trim(), radio.cw_serial_baud_rate, radio.cw_serial_line.trim(), radio.cw_messages, radio.voice_messages, radio_ws_url.trim(), existing.id],
        )?;
        return select_radio(connection, existing.id)?.ok_or(rusqlite::Error::QueryReturnedNoRows);
    }

    connection.execute(
        "INSERT INTO radios (NAME, RADIO_KIND, TRANSPORT_KIND, TCP_HOST, TCP_PORT, SERIAL_PORT, SERIAL_BAUD_RATE, OPTIONS, DATA_MODE, RTTY_MODE, DIGITAL_PROGRAM, WSJTX_ENABLED, WSJTX_BIND_ADDRESS, WSJTX_PORT, WSJTX_MULTICAST_GROUP, FLDIGI_DATA_ENABLED, FLDIGI_RTTY_ENABLED, FLDIGI_HOST, FLDIGI_PORT, FLRIG_ENABLED, FLRIG_PORT, CW_TUNING_INCREMENT_HZ, SSB_TUNING_INCREMENT_HZ, RIT_CLEAR_ON_LOG, VOICE_INPUT_DEVICE_ID, VOICE_OUTPUT_DEVICE_ID, CW_KEYER_TYPE, WINKEYER_SERIAL_PORT, CW_SERIAL_PORT, CW_SERIAL_BAUD_RATE, CW_SERIAL_LINE, CW_MESSAGES, VOICE_MESSAGES, CONTROL_LOCATION, CLIENT_INSTANCE_ID, RADIO_WS_URL) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, 'client', ?34, ?35)",
        params![radio.name.trim(), radio.radio_kind.trim(), radio.transport_kind.trim(), radio.tcp_host.trim(), radio.tcp_port, radio.serial_port.trim(), radio.serial_baud_rate, radio.options, radio.data_mode, radio.rtty_mode, radio.digital_program, radio.wsjtx_enabled, radio.wsjtx_bind_address.trim(), radio.wsjtx_port, radio.wsjtx_multicast_group.trim(), radio.fldigi_data_enabled, radio.fldigi_rtty_enabled, radio.fldigi_host.trim(), radio.fldigi_port, radio.flrig_enabled, radio.flrig_port, radio.cw_tuning_increment_hz, radio.ssb_tuning_increment_hz, radio.rit_clear_on_log, normalized_optional_device_id(radio.voice_input_device_id.as_deref()), normalized_optional_device_id(radio.voice_output_device_id.as_deref()), radio.cw_keyer_type.trim(), radio.winkeyer_serial_port.trim(), radio.cw_serial_port.trim(), radio.cw_serial_baud_rate, radio.cw_serial_line.trim(), radio.cw_messages, radio.voice_messages, client_instance_id.trim(), radio_ws_url.trim()],
    )?;
    select_radio(connection, connection.last_insert_rowid())?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)
}

fn row_to_radio(row: &rusqlite::Row<'_>) -> rusqlite::Result<RadioRecord> {
    let tcp_port: i64 = row.get("TCP_PORT")?;
    let serial_baud_rate: i64 = row.get("SERIAL_BAUD_RATE")?;
    let cw_tuning_increment_hz: i64 = row.get("CW_TUNING_INCREMENT_HZ")?;
    let ssb_tuning_increment_hz: i64 = row.get("SSB_TUNING_INCREMENT_HZ")?;
    let cw_serial_baud_rate: i64 = row.get("CW_SERIAL_BAUD_RATE")?;
    let wsjtx_port: i64 = row.get("WSJTX_PORT")?;
    let fldigi_port: i64 = row.get("FLDIGI_PORT")?;
    let flrig_port: i64 = row.get("FLRIG_PORT")?;
    let voice_input_device_id: Option<String> = row.get("VOICE_INPUT_DEVICE_ID")?;
    let voice_output_device_id: Option<String> = row.get("VOICE_OUTPUT_DEVICE_ID")?;
    let control_location = RadioControlLocation::parse(&row.get::<_, String>("CONTROL_LOCATION")?)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            )
        })?;
    Ok(RadioRecord {
        config: RadioConfig::new(
            row.get("ID")?,
            radio_io::RadioSettings {
                name: row.get("NAME")?,
                radio_kind: row.get("RADIO_KIND")?,
                transport_kind: row.get("TRANSPORT_KIND")?,
                tcp_host: row.get("TCP_HOST")?,
                tcp_port: tcp_port as u16,
                serial_port: row.get("SERIAL_PORT")?,
                serial_baud_rate: serial_baud_rate as u32,
                options: row.get("OPTIONS")?,
                data_mode: row.get("DATA_MODE")?,
                rtty_mode: row.get("RTTY_MODE")?,
                digital_program: row.get("DIGITAL_PROGRAM")?,
                wsjtx_enabled: row.get("WSJTX_ENABLED")?,
                wsjtx_bind_address: row.get("WSJTX_BIND_ADDRESS")?,
                wsjtx_port: wsjtx_port as u16,
                wsjtx_multicast_group: row.get("WSJTX_MULTICAST_GROUP")?,
                fldigi_data_enabled: row.get("FLDIGI_DATA_ENABLED")?,
                fldigi_rtty_enabled: row.get("FLDIGI_RTTY_ENABLED")?,
                fldigi_host: row.get("FLDIGI_HOST")?,
                fldigi_port: fldigi_port as u16,
                flrig_enabled: row.get("FLRIG_ENABLED")?,
                flrig_port: flrig_port as u16,
                cw_tuning_increment_hz: cw_tuning_increment_hz as u32,
                ssb_tuning_increment_hz: ssb_tuning_increment_hz as u32,
                rit_clear_on_log: row.get("RIT_CLEAR_ON_LOG")?,
                voice_input_device_id: normalized_optional_device_id(
                    voice_input_device_id.as_deref(),
                ),
                voice_output_device_id: normalized_optional_device_id(
                    voice_output_device_id.as_deref(),
                ),
                cw_keyer_type: row.get("CW_KEYER_TYPE")?,
                winkeyer_serial_port: row.get("WINKEYER_SERIAL_PORT")?,
                cw_serial_port: row.get("CW_SERIAL_PORT")?,
                cw_serial_baud_rate: cw_serial_baud_rate as u32,
                cw_serial_line: row.get("CW_SERIAL_LINE")?,
                cw_messages: row.get("CW_MESSAGES")?,
                voice_messages: row.get("VOICE_MESSAGES")?,
            },
        ),
        control_location,
        client_instance_id: row.get("CLIENT_INSTANCE_ID")?,
        radio_ws_url: row.get("RADIO_WS_URL")?,
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
