use crate::cw::DEFAULT_CW_MESSAGES;
use crate::voice_messages::DEFAULT_VOICE_MESSAGES;
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
pub struct RadioConfig {
    pub id: i64,
    pub name: String,
    pub radio_kind: String,
    pub transport_kind: String,
    pub tcp_host: String,
    pub tcp_port: u16,
    pub serial_port: String,
    pub serial_baud_rate: u32,
    pub options: String,
    pub cw_tuning_increment_hz: u32,
    pub ssb_tuning_increment_hz: u32,
    pub rit_clear_on_log: bool,
    pub voice_input_device_id: Option<String>,
    pub voice_output_device_id: Option<String>,
    pub cw_keyer_type: String,
    pub winkeyer_serial_port: String,
    pub cw_serial_port: String,
    pub cw_serial_baud_rate: u32,
    pub cw_serial_line: String,
    pub cw_messages: String,
    pub voice_messages: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SerialAllocation {
    pub log_id: i64,
    pub field_adif: String,
    pub start: i64,
    pub end: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigView {
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
    pub login_user: String,
    pub login_password: LoginPasswordUpdate,
    pub dxcluster_enabled: bool,
    pub dxcluster_host: String,
    pub dxcluster_port: u16,
    pub dxcluster_callsign: String,
    pub dxcluster_max_age_min: u16,
    pub dxcluster_commands: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewRadio {
    pub name: String,
    pub radio_kind: String,
    pub transport_kind: String,
    pub tcp_host: String,
    pub tcp_port: u16,
    pub serial_port: String,
    pub serial_baud_rate: u32,
    #[serde(default)]
    pub options: String,
    #[serde(default = "default_cw_tuning_increment_hz")]
    pub cw_tuning_increment_hz: u32,
    #[serde(default = "default_ssb_tuning_increment_hz")]
    pub ssb_tuning_increment_hz: u32,
    #[serde(default)]
    pub rit_clear_on_log: bool,
    #[serde(default)]
    pub voice_input_device_id: Option<String>,
    #[serde(default)]
    pub voice_output_device_id: Option<String>,
    pub cw_keyer_type: String,
    pub winkeyer_serial_port: String,
    #[serde(default)]
    pub cw_serial_port: String,
    #[serde(default = "default_cw_serial_baud_rate")]
    pub cw_serial_baud_rate: u32,
    #[serde(default = "default_cw_serial_line")]
    pub cw_serial_line: String,
    #[serde(default = "default_cw_messages")]
    pub cw_messages: String,
    #[serde(default = "default_voice_messages")]
    pub voice_messages: String,
}

pub const DEFAULT_CW_TUNING_INCREMENT_HZ: u32 = 20;
pub const DEFAULT_SSB_TUNING_INCREMENT_HZ: u32 = 100;

fn default_cw_tuning_increment_hz() -> u32 {
    DEFAULT_CW_TUNING_INCREMENT_HZ
}

fn default_ssb_tuning_increment_hz() -> u32 {
    DEFAULT_SSB_TUNING_INCREMENT_HZ
}

fn default_cw_serial_baud_rate() -> u32 {
    9_600
}

fn default_cw_serial_line() -> String {
    "dtr".to_string()
}

fn default_cw_messages() -> String {
    DEFAULT_CW_MESSAGES.to_string()
}

fn default_voice_messages() -> String {
    DEFAULT_VOICE_MESSAGES.to_string()
}
