use super::cache::CachedConfig;
use super::models::{AuthConfig, DxClusterConfig, LoginPasswordUpdate, UpdateConfig};
use rusqlite::{Connection, params};

pub const DEFAULT_DXCLUSTER_PORT: u16 = 23;
pub const DEFAULT_DXCLUSTER_MAX_AGE_MIN: u16 = 60;
pub const MIN_DXCLUSTER_MAX_AGE_MIN: u16 = 15;
pub const MAX_DXCLUSTER_MAX_AGE_MIN: u16 = 360;
pub const DEFAULT_IARU_REGION: i64 = 2;

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

pub(super) fn db_config(connection: &Connection) -> rusqlite::Result<CachedConfig> {
    connection.query_row(
        "SELECT IARU_REGION, LOGIN_USER, LOGIN_PASSWORD, DXCLUSTER_ENABLED, DXCLUSTER_HOST, DXCLUSTER_PORT, DXCLUSTER_CALLSIGN, DXCLUSTER_MAX_AGE_MIN, DXCLUSTER_COMMANDS FROM config LIMIT 1",
        [], |row| Ok(CachedConfig {
            region: row.get(0)?,
            auth: AuthConfig { login_user: row.get(1)?, login_password: row.get(2)? },
            dxcluster: DxClusterConfig { enabled: row.get(3)?, host: row.get(4)?, port: row.get(5)?,
                callsign: row.get(6)?, max_age_min: row.get(7)?, commands: row.get(8)? },
        }),
    )
}

pub(super) fn db_update_config(
    connection: &Connection,
    config: UpdateConfig,
    preserved_password: &str,
) -> rusqlite::Result<CachedConfig> {
    let max_age_min = config
        .dxcluster_max_age_min
        .clamp(MIN_DXCLUSTER_MAX_AGE_MIN, MAX_DXCLUSTER_MAX_AGE_MIN);
    let login_password = match config.login_password {
        LoginPasswordUpdate::Preserve => preserved_password.to_string(),
        LoginPasswordUpdate::Set(login_password) => login_password,
        LoginPasswordUpdate::Disable => String::new(),
    };
    let login_user = config.login_user.trim();
    let dxcluster_host = config.dxcluster_host.trim();
    let dxcluster_callsign = config.dxcluster_callsign.trim().to_uppercase();
    let dxcluster_commands = &config.dxcluster_commands;

    let updated = connection.execute(
        "UPDATE config SET IARU_REGION = ?1, LOGIN_USER = ?2, LOGIN_PASSWORD = ?3, DXCLUSTER_ENABLED = ?4, DXCLUSTER_HOST = ?5, DXCLUSTER_PORT = ?6, DXCLUSTER_CALLSIGN = ?7, DXCLUSTER_MAX_AGE_MIN = ?8, DXCLUSTER_COMMANDS = ?9",
        params![
            config.iaru_region,
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
            "INSERT INTO config (version, IARU_REGION, LOGIN_USER, LOGIN_PASSWORD, DXCLUSTER_ENABLED, DXCLUSTER_HOST, DXCLUSTER_PORT, DXCLUSTER_CALLSIGN, DXCLUSTER_MAX_AGE_MIN, DXCLUSTER_COMMANDS) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                config.iaru_region,
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
    Ok(CachedConfig {
        region: config.iaru_region,
        auth: AuthConfig {
            login_user: login_user.to_string(),
            login_password,
        },
        dxcluster: DxClusterConfig {
            enabled: config.dxcluster_enabled,
            host: dxcluster_host.to_string(),
            port: config.dxcluster_port,
            callsign: dxcluster_callsign,
            max_age_min,
            commands: config.dxcluster_commands,
        },
    })
}
