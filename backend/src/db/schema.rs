use rusqlite::Connection;

pub(super) fn initialize_schema(connection: &Connection) -> rusqlite::Result<()> {
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
