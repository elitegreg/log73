use crate::{cw, modes, voice_messages};
use radio_cat_rs::supported_drivers;
use serde::{Deserialize, Serialize};
use std::{
    net::Ipv4Addr,
    ops::{Deref, DerefMut},
};

pub const DEFAULT_CW_TUNING_INCREMENT_HZ: u32 = 20;
pub const DEFAULT_SSB_TUNING_INCREMENT_HZ: u32 = 100;
pub const DEFAULT_WSJTX_BIND_ADDRESS: &str = "127.0.0.1";
pub const DEFAULT_WSJTX_PORT: u16 = 2237;
pub const DEFAULT_FLRIG_PORT: u16 = 12_345;
pub const DEFAULT_FLDIGI_HOST: &str = "127.0.0.1";
pub const DEFAULT_FLDIGI_PORT: u16 = 7362;
pub const DEFAULT_CW_SERIAL_BAUD_RATE: u32 = 9_600;
pub const DEFAULT_CW_SERIAL_LINE: &str = "dtr";

const MAX_RADIO_NAME_LEN: usize = 100;
const MAX_RADIO_HOST_LEN: usize = 255;
const MAX_SERIAL_PORT_LEN: usize = 255;
const MAX_SOUND_DEVICE_ID_LEN: usize = 1024;
const MAX_RADIO_TUNING_INCREMENT_HZ: u32 = 9_999;
const MAX_CW_MESSAGES_LEN: usize = 16_384;
const MAX_VOICE_MESSAGES_LEN: usize = 16_384;
const ALLOWED_CW_KEYER_TYPES: &[&str] = &["none", "winkeyer", "cat", "serial"];

/// Radio settings without a backend database identity.
///
/// This is the shared wire and configuration model used by the backend and by
/// future local radio clients. A runtime/database owner supplies identity via
/// [`ConfiguredRadio`] instead of inventing an ID in these settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RadioSettings {
    pub name: String,
    pub radio_kind: String,
    pub transport_kind: String,
    pub tcp_host: String,
    pub tcp_port: u16,
    pub serial_port: String,
    pub serial_baud_rate: u32,
    pub options: String,
    pub data_mode: String,
    pub rtty_mode: String,
    pub digital_program: String,
    pub wsjtx_enabled: bool,
    pub wsjtx_bind_address: String,
    pub wsjtx_port: u16,
    pub wsjtx_multicast_group: String,
    pub fldigi_data_enabled: bool,
    pub fldigi_rtty_enabled: bool,
    pub fldigi_host: String,
    pub fldigi_port: u16,
    pub flrig_enabled: bool,
    pub flrig_port: u16,
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

impl Default for RadioSettings {
    fn default() -> Self {
        Self {
            name: String::new(),
            radio_kind: String::new(),
            transport_kind: String::new(),
            tcp_host: String::new(),
            tcp_port: 0,
            serial_port: String::new(),
            serial_baud_rate: 115_200,
            options: String::new(),
            data_mode: String::new(),
            rtty_mode: String::new(),
            digital_program: "none".to_string(),
            wsjtx_enabled: false,
            wsjtx_bind_address: DEFAULT_WSJTX_BIND_ADDRESS.to_string(),
            wsjtx_port: DEFAULT_WSJTX_PORT,
            wsjtx_multicast_group: String::new(),
            fldigi_data_enabled: false,
            fldigi_rtty_enabled: false,
            fldigi_host: DEFAULT_FLDIGI_HOST.to_string(),
            fldigi_port: DEFAULT_FLDIGI_PORT,
            flrig_enabled: false,
            flrig_port: DEFAULT_FLRIG_PORT,
            cw_tuning_increment_hz: DEFAULT_CW_TUNING_INCREMENT_HZ,
            ssb_tuning_increment_hz: DEFAULT_SSB_TUNING_INCREMENT_HZ,
            rit_clear_on_log: false,
            voice_input_device_id: None,
            voice_output_device_id: None,
            cw_keyer_type: "none".to_string(),
            winkeyer_serial_port: String::new(),
            cw_serial_port: String::new(),
            cw_serial_baud_rate: DEFAULT_CW_SERIAL_BAUD_RATE,
            cw_serial_line: DEFAULT_CW_SERIAL_LINE.to_string(),
            cw_messages: cw::DEFAULT_CW_MESSAGES.to_string(),
            voice_messages: voice_messages::DEFAULT_VOICE_MESSAGES.to_string(),
        }
    }
}

/// A backend/runtime-owned radio identity paired with shared settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfiguredRadio {
    pub id: i64,
    #[serde(flatten)]
    pub settings: RadioSettings,
}

impl ConfiguredRadio {
    pub fn new(id: i64, settings: RadioSettings) -> Self {
        Self { id, settings }
    }
}

impl Deref for ConfiguredRadio {
    type Target = RadioSettings;

    fn deref(&self) -> &Self::Target {
        &self.settings
    }
}

impl DerefMut for ConfiguredRadio {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.settings
    }
}

/// Compatibility name for existing runtime consumers. New settings-only
/// consumers should use [`RadioSettings`].
pub type RadioConfig = ConfiguredRadio;

#[derive(Clone, Debug, Default)]
pub struct RadioIoConfig {
    pub radios: Vec<ConfiguredRadio>,
}

impl RadioIoConfig {
    pub fn new(radios: Vec<ConfiguredRadio>) -> Self {
        Self { radios }
    }
}

pub fn normalize_radio_settings(settings: &mut RadioSettings) -> Result<(), String> {
    let (data_mode, rtty_mode) = modes::resolved_mode_mappings(
        &settings.radio_kind,
        &settings.data_mode,
        &settings.rtty_mode,
    )?;
    settings.data_mode = data_mode;
    settings.rtty_mode = rtty_mode;
    settings.digital_program = normalized_digital_program(&settings.digital_program)?;
    settings.wsjtx_enabled = settings.digital_program == "wsjtx";
    settings.fldigi_data_enabled = settings.digital_program == "fldigi";
    settings.fldigi_host = settings.fldigi_host.trim().to_string();
    settings.voice_input_device_id =
        normalized_optional_device_id(settings.voice_input_device_id.take());
    settings.voice_output_device_id =
        normalized_optional_device_id(settings.voice_output_device_id.take());
    Ok(())
}

pub fn validate_radio_settings(settings: &RadioSettings) -> Result<(), String> {
    validate_required_text("radio name", &settings.name, MAX_RADIO_NAME_LEN)?;
    let radio_kind = settings.radio_kind.trim();
    if !supported_drivers()
        .iter()
        .any(|driver| driver.id.eq_ignore_ascii_case(radio_kind))
    {
        return Err(format!("unsupported radio driver: {radio_kind}"));
    }
    modes::resolved_mode_mappings(radio_kind, &settings.data_mode, &settings.rtty_mode)?;
    let digital_program = normalized_digital_program(&settings.digital_program)?;
    if settings.wsjtx_enabled != (digital_program == "wsjtx") {
        return Err("WSJT-X enabled state must match the DATA digital program".to_string());
    }
    if settings.fldigi_data_enabled != (digital_program == "fldigi") {
        return Err("FLDigi DATA enabled state must match the DATA digital program".to_string());
    }

    let transport_kind = settings.transport_kind.trim().to_ascii_lowercase();
    if !matches!(transport_kind.as_str(), "none" | "tcp" | "serial") {
        return Err("transport kind must be none, tcp, or serial".to_string());
    }
    if transport_kind == "none" && !radio_kind.eq_ignore_ascii_case("dummy") {
        return Err("transport is required for non-dummy radios".to_string());
    }

    validate_tuning_increment_hz("CW tuning increment", settings.cw_tuning_increment_hz)?;
    validate_tuning_increment_hz("SSB tuning increment", settings.ssb_tuning_increment_hz)?;
    validate_optional_plain_text(
        "voice input sound device",
        settings.voice_input_device_id.as_deref().unwrap_or(""),
        MAX_SOUND_DEVICE_ID_LEN,
    )?;
    validate_optional_plain_text(
        "voice output sound device",
        settings.voice_output_device_id.as_deref().unwrap_or(""),
        MAX_SOUND_DEVICE_ID_LEN,
    )?;

    let cw_keyer_type = settings.cw_keyer_type.trim().to_ascii_lowercase();
    if !ALLOWED_CW_KEYER_TYPES.contains(&cw_keyer_type.as_str()) {
        return Err("CW keyer type must be one of: none, winkeyer, cat, serial".to_string());
    }
    if cw_keyer_type == "cat" && !modes::supports_cat_cw_keying_for_radio_kind(radio_kind)? {
        return Err("CAT CW keying is not supported by this radio".to_string());
    }

    match transport_kind.as_str() {
        "tcp" => {
            validate_required_text("TCP host", &settings.tcp_host, MAX_RADIO_HOST_LEN)?;
            validate_host("TCP host", &settings.tcp_host)?;
            if settings.tcp_port == 0 {
                return Err("TCP port must be between 1 and 65535".to_string());
            }
            validate_max_len("serial port", &settings.serial_port, MAX_SERIAL_PORT_LEN)?;
        }
        "serial" => {
            validate_required_text("serial port", &settings.serial_port, MAX_SERIAL_PORT_LEN)?;
            validate_serial_port("serial port", &settings.serial_port)?;
            if settings.serial_baud_rate == 0 {
                return Err("serial baud rate must be greater than 0".to_string());
            }
            validate_max_len("TCP host", &settings.tcp_host, MAX_RADIO_HOST_LEN)?;
        }
        "none" => {
            validate_max_len("TCP host", &settings.tcp_host, MAX_RADIO_HOST_LEN)?;
            validate_max_len("serial port", &settings.serial_port, MAX_SERIAL_PORT_LEN)?;
        }
        _ => unreachable!(),
    }

    if cw_keyer_type == "winkeyer" {
        validate_required_text(
            "Winkeyer serial port",
            &settings.winkeyer_serial_port,
            MAX_SERIAL_PORT_LEN,
        )?;
    } else {
        validate_max_len(
            "Winkeyer serial port",
            &settings.winkeyer_serial_port,
            MAX_SERIAL_PORT_LEN,
        )?;
    }
    validate_serial_port("Winkeyer serial port", &settings.winkeyer_serial_port)?;
    if cw_keyer_type == "serial" {
        validate_required_text(
            "CW serial port",
            &settings.cw_serial_port,
            MAX_SERIAL_PORT_LEN,
        )?;
    } else {
        validate_max_len(
            "CW serial port",
            &settings.cw_serial_port,
            MAX_SERIAL_PORT_LEN,
        )?;
    }
    validate_serial_port("CW serial port", &settings.cw_serial_port)?;
    if settings.cw_serial_baud_rate == 0 {
        return Err("CW serial baud rate must be greater than 0".to_string());
    }
    if !matches!(
        settings.cw_serial_line.trim().to_ascii_lowercase().as_str(),
        "dtr" | "rts"
    ) {
        return Err("CW serial line must be dtr or rts".to_string());
    }
    if transport_kind == "serial"
        && cw_keyer_type == "serial"
        && settings.serial_port.trim() == settings.cw_serial_port.trim()
        && settings.serial_baud_rate != settings.cw_serial_baud_rate
    {
        return Err(
            "CAT serial baud rate and CW serial baud rate must match when sharing a serial port"
                .to_string(),
        );
    }

    if !matches!(settings.wsjtx_bind_address.trim(), "127.0.0.1" | "0.0.0.0") {
        return Err("WSJT-X bind address must be 127.0.0.1 or 0.0.0.0".to_string());
    }
    if settings.wsjtx_port < 1024 {
        return Err("WSJT-X port must be between 1024 and 65535".to_string());
    }
    if settings.flrig_port < 1024 {
        return Err("FLRig port must be between 1024 and 65535".to_string());
    }
    if settings.fldigi_data_enabled || settings.fldigi_rtty_enabled {
        validate_required_text("FLDigi host", &settings.fldigi_host, MAX_RADIO_HOST_LEN)?;
        validate_host("FLDigi host", &settings.fldigi_host)?;
        if settings.fldigi_port < 1024 {
            return Err("FLDigi port must be between 1024 and 65535".to_string());
        }
    } else {
        validate_max_len("FLDigi host", &settings.fldigi_host, MAX_RADIO_HOST_LEN)?;
    }
    let multicast_group = settings.wsjtx_multicast_group.trim();
    if !multicast_group.is_empty() {
        let group = multicast_group
            .parse::<Ipv4Addr>()
            .map_err(|_| "WSJT-X multicast group must be a valid IPv4 address".to_string())?;
        if !group.is_multicast() {
            return Err("WSJT-X multicast group must be an IPv4 multicast address".to_string());
        }
    }
    validate_cw_messages(&settings.cw_messages)?;
    validate_voice_messages(&settings.voice_messages)
}

fn normalized_digital_program(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    if matches!(value.as_str(), "none" | "wsjtx" | "fldigi") {
        Ok(value)
    } else {
        Err("DATA digital program must be none, wsjtx, or fldigi".to_string())
    }
}

pub fn validate_cw_messages(value: &str) -> Result<(), String> {
    validate_message_text("CW messages", value, MAX_CW_MESSAGES_LEN)?;
    cw::validate(value).map(|_| ())
}

pub fn validate_voice_messages(value: &str) -> Result<(), String> {
    validate_message_text("Voice messages", value, MAX_VOICE_MESSAGES_LEN)?;
    voice_messages::validate(value).map(|_| ())
}

fn normalized_optional_device_id(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn validate_message_text(label: &str, value: &str, max_len: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} are required"));
    }
    if value.chars().count() > max_len {
        return Err(format!("{label} must be at most {max_len} characters"));
    }
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(format!("{label} cannot contain control characters"));
    }
    Ok(())
}

fn validate_required_text(label: &str, value: &str, max_len: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} is required"));
    }
    validate_max_len(label, value, max_len)?;
    if value.chars().any(char::is_control) {
        return Err(format!("{label} cannot contain control characters"));
    }
    Ok(())
}

fn validate_max_len(label: &str, value: &str, max_len: usize) -> Result<(), String> {
    if value.chars().count() > max_len {
        Err(format!("{label} must be at most {max_len} characters"))
    } else {
        Ok(())
    }
}

fn validate_optional_plain_text(label: &str, value: &str, max_len: usize) -> Result<(), String> {
    validate_max_len(label, value.trim(), max_len)?;
    if value.trim().chars().any(char::is_control) {
        return Err(format!("{label} cannot contain control characters"));
    }
    Ok(())
}

fn validate_host(label: &str, value: &str) -> Result<(), String> {
    if value
        .trim()
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(format!(
            "{label} cannot contain whitespace or control characters"
        ));
    }
    Ok(())
}

fn validate_serial_port(label: &str, value: &str) -> Result<(), String> {
    if value.chars().any(char::is_control) {
        Err(format!("{label} cannot contain control characters"))
    } else {
        Ok(())
    }
}

fn validate_tuning_increment_hz(label: &str, value: u32) -> Result<(), String> {
    if value == 0 || value > MAX_RADIO_TUNING_INCREMENT_HZ {
        Err(format!(
            "{label} must be between 1 and {MAX_RADIO_TUNING_INCREMENT_HZ} Hz"
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tcp_radio() -> RadioSettings {
        RadioSettings {
            name: "K4".to_string(),
            radio_kind: "elecraft-k4".to_string(),
            transport_kind: "tcp".to_string(),
            tcp_host: "127.0.0.1".to_string(),
            tcp_port: 5002,
            data_mode: "DATA-USB".to_string(),
            rtty_mode: "RTTY".to_string(),
            ..RadioSettings::default()
        }
    }

    #[test]
    fn settings_deserialize_with_existing_defaults() {
        let settings: RadioSettings = serde_json::from_value(
            serde_json::json!({ "name": "Dummy", "radio_kind": "dummy", "transport_kind": "none" }),
        )
        .expect("settings deserialize");
        assert_eq!(settings.wsjtx_bind_address, DEFAULT_WSJTX_BIND_ADDRESS);
        assert!(!settings.flrig_enabled);
        assert_eq!(settings.flrig_port, DEFAULT_FLRIG_PORT);
        assert_eq!(settings.cw_messages, cw::DEFAULT_CW_MESSAGES);
    }

    #[test]
    fn configured_radio_flattens_settings() {
        let value =
            serde_json::to_value(ConfiguredRadio::new(7, tcp_radio())).expect("config serializes");
        assert_eq!(value["id"], 7);
        assert_eq!(value["tcp_host"], "127.0.0.1");
        assert_eq!(value["flrig_enabled"], false);
        assert_eq!(value["flrig_port"], DEFAULT_FLRIG_PORT);
        assert!(value.get("settings").is_none());
    }

    #[test]
    fn normalizes_mode_mappings_and_optional_devices() {
        let mut settings = RadioSettings {
            name: "Dummy".to_string(),
            radio_kind: "dummy".to_string(),
            transport_kind: "none".to_string(),
            voice_input_device_id: Some("  ".to_string()),
            voice_output_device_id: Some(" alsa:hw:1,0 ".to_string()),
            ..RadioSettings::default()
        };
        normalize_radio_settings(&mut settings).expect("normalizes");
        assert_eq!(settings.data_mode, "DATA-USB");
        assert_eq!(settings.voice_input_device_id, None);
        assert_eq!(
            settings.voice_output_device_id.as_deref(),
            Some("alsa:hw:1,0")
        );
    }

    #[test]
    fn validates_tcp_serial_and_wsjtx_constraints() {
        assert!(validate_radio_settings(&tcp_radio()).is_ok());
        let mut serial = tcp_radio();
        serial.transport_kind = "serial".to_string();
        serial.tcp_host.clear();
        serial.tcp_port = 0;
        serial.serial_port = "/dev/ttyUSB0".to_string();
        assert!(validate_radio_settings(&serial).is_ok());
        serial.wsjtx_port = 1023;
        assert!(validate_radio_settings(&serial).is_err());
        serial.wsjtx_port = DEFAULT_WSJTX_PORT;
        serial.flrig_port = 1023;
        assert!(validate_radio_settings(&serial).is_err());
    }

    #[test]
    fn digital_program_selects_exactly_one_data_integration() {
        let mut settings = tcp_radio();
        settings.digital_program = " FLDIGI ".to_string();
        settings.wsjtx_enabled = true;
        normalize_radio_settings(&mut settings).expect("normalizes digital program");

        assert_eq!(settings.digital_program, "fldigi");
        assert!(!settings.wsjtx_enabled);
        assert!(settings.fldigi_data_enabled);
        assert!(validate_radio_settings(&settings).is_ok());

        settings.wsjtx_enabled = true;
        assert!(validate_radio_settings(&settings).is_err());
    }
}
