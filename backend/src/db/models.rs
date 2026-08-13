pub use radio_io::{ConfiguredRadio as RadioConfig, RadioSettings as RadioPayload};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct Log {
    pub id: i64,
    pub name: String,
    pub contest_id: String,
    pub station_callsign: String,
    pub contest_params: Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewLog {
    pub name: String,
    pub contest_id: String,
    pub station_callsign: String,
    #[serde(default)]
    pub contest_params: Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateLog {
    pub name: String,
    pub station_callsign: String,
    #[serde(default)]
    pub contest_params: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct SerialAllocation {
    pub log_id: i64,
    pub field_adif: String,
    pub serial: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigView {
    pub iaru_region: i64,
    pub login_user: String,
    pub login_enabled: bool,
    pub dxcluster_enabled: bool,
    pub dxcluster_host: String,
    pub dxcluster_port: u16,
    pub dxcluster_callsign: String,
    pub dxcluster_max_age_min: u16,
    pub dxcluster_commands: String,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub login_user: String,
    pub login_password: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DxClusterConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub callsign: String,
    pub max_age_min: u16,
    pub commands: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum LoginPasswordUpdate {
    Preserve,
    Set(String),
    Disable,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateConfig {
    pub iaru_region: i64,
    pub login_user: String,
    pub login_password: LoginPasswordUpdate,
    pub dxcluster_enabled: bool,
    pub dxcluster_host: String,
    pub dxcluster_port: u16,
    pub dxcluster_callsign: String,
    pub dxcluster_max_age_min: u16,
    pub dxcluster_commands: String,
}

pub use radio_io::{DEFAULT_CW_TUNING_INCREMENT_HZ, DEFAULT_SSB_TUNING_INCREMENT_HZ};
