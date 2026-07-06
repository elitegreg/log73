use super::models::{AuthConfig, DxClusterConfig, LoginPasswordUpdate, UpdateConfig};
use rusqlite::{Connection, OptionalExtension, params};

pub const DEFAULT_DXCLUSTER_PORT: u16 = 23;
pub const DEFAULT_DXCLUSTER_MAX_AGE_MIN: u16 = 60;
pub const MIN_DXCLUSTER_MAX_AGE_MIN: u16 = 15;
pub const MAX_DXCLUSTER_MAX_AGE_MIN: u16 = 360;

impl Default for DxClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: String::new(),
            port: DEFAULT_DXCLUSTER_PORT,
            callsign: String::new(),
            max_age_min: DEFAULT_DXCLUSTER_MAX_AGE_MIN,
            commands: String::new(),
        }
    }
}

pub(super) fn db_auth_config(connection: &Connection) -> rusqlite::Result<AuthConfig> {
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

pub(super) fn db_dxcluster_config(connection: &Connection) -> rusqlite::Result<DxClusterConfig> {
    match connection
        .query_row(
            "SELECT DXCLUSTER_ENABLED, DXCLUSTER_HOST, DXCLUSTER_PORT, DXCLUSTER_CALLSIGN, DXCLUSTER_MAX_AGE_MIN, DXCLUSTER_COMMANDS FROM config LIMIT 1",
            [],
            |row| {
                Ok(DxClusterConfig {
                    enabled: row.get(0)?,
                    host: row.get(1)?,
                    port: row.get(2)?,
                    callsign: row.get(3)?,
                    max_age_min: row.get(4)?,
                    commands: row.get(5)?,
                })
            },
        )
        .optional()
    {
        Ok(Some(config)) => Ok(config),
        Ok(None) => Ok(DxClusterConfig::default()),
        Err(error) if is_missing_config_column(&error) => Ok(DxClusterConfig::default()),
        Err(error) => Err(error),
    }
}

pub(super) fn db_update_config(
    connection: &Connection,
    config: UpdateConfig,
) -> rusqlite::Result<()> {
    let max_age_min = config
        .dxcluster_max_age_min
        .clamp(MIN_DXCLUSTER_MAX_AGE_MIN, MAX_DXCLUSTER_MAX_AGE_MIN);
    let login_password = match config.login_password {
        LoginPasswordUpdate::Preserve => db_auth_config(connection)?.login_password,
        LoginPasswordUpdate::Set(login_password) => login_password,
        LoginPasswordUpdate::Disable => String::new(),
    };
    let login_user = config.login_user.trim();
    let dxcluster_host = config.dxcluster_host.trim();
    let dxcluster_callsign = config.dxcluster_callsign.trim().to_uppercase();
    let dxcluster_commands = &config.dxcluster_commands;

    let updated = connection.execute(
        "UPDATE config SET LOGIN_USER = ?1, LOGIN_PASSWORD = ?2, DXCLUSTER_ENABLED = ?3, DXCLUSTER_HOST = ?4, DXCLUSTER_PORT = ?5, DXCLUSTER_CALLSIGN = ?6, DXCLUSTER_MAX_AGE_MIN = ?7, DXCLUSTER_COMMANDS = ?8",
        params![
            login_user,
            &login_password,
            config.dxcluster_enabled,
            dxcluster_host,
            config.dxcluster_port,
            &dxcluster_callsign,
            max_age_min,
            dxcluster_commands
        ],
    )?;
    if updated == 0 {
        connection.execute(
            "INSERT INTO config (version, LOGIN_USER, LOGIN_PASSWORD, DXCLUSTER_ENABLED, DXCLUSTER_HOST, DXCLUSTER_PORT, DXCLUSTER_CALLSIGN, DXCLUSTER_MAX_AGE_MIN, DXCLUSTER_COMMANDS) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                login_user,
                &login_password,
                config.dxcluster_enabled,
                dxcluster_host,
                config.dxcluster_port,
                &dxcluster_callsign,
                max_age_min,
                dxcluster_commands
            ],
        )?;
    }
    Ok(())
}

pub(super) fn is_missing_config_column(error: &rusqlite::Error) -> bool {
    error.to_string().contains("no such column")
}
