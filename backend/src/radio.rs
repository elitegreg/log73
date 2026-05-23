use crate::bands::band_for_frequency;
use crate::db::RadioConfig;
use radio_cat_rs::{Frequency, Mode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    RadioStatus(RadioStatus),
    RadioState(RadioState),
    LogEntry {
        contact: serde_json::Map<String, serde_json::Value>,
    },
    ContactDeleted {
        id: i64,
        log_id: i64,
    },
    ScoreUpdate {
        log_id: i64,
        qso_count: usize,
        multipliers: i64,
        bonus_points: i64,
        total_score: i64,
    },
    CwSent {
        request_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RadioStatus {
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadioState {
    pub frequency_hz: u64,
    pub mode: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    SetFrequency {
        frequency_hz: u64,
    },
    SetMode {
        mode: String,
    },
    SendCw {
        request_id: String,
        mode: String,
        key: String,
        fields: serde_json::Map<String, serde_json::Value>,
    },
    StopCw,
    SetWpm {
        wpm: u8,
    },
}

#[derive(Debug)]
pub enum RadioCommand {
    SetFrequency(u64),
    SetMode(String),
    SendCw {
        mode: String,
        key: String,
        fields: serde_json::Map<String, serde_json::Value>,
        completed: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    StopCw,
    SetWpm(u8),
    ReloadConfig(RadioConfig),
}

pub fn normalize_mode(mode: &Mode) -> String {
    match mode {
        Mode::Usb | Mode::Lsb => "SSB".to_string(),
        other => other.to_string(),
    }
}

pub fn mode_for_request(requested: &str, frequency_hz: u64) -> Option<Mode> {
    match requested.to_uppercase().as_str() {
        "CW" => Some(Mode::Cw),
        "FM" => Some(Mode::Fm),
        "SSB" => Some(ssb_mode_for_frequency(frequency_hz)),
        "USB" => Some(Mode::Usb),
        "LSB" => Some(Mode::Lsb),
        _ => None,
    }
}

fn ssb_mode_for_frequency(frequency_hz: u64) -> Mode {
    let frequency = Frequency::from_hz(frequency_hz);

    match band_for_frequency(frequency).map(|band| band.meters) {
        Some(meters) if meters >= 40 => Mode::Lsb,
        _ => Mode::Usb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_radio_status_server_message() {
        let message = ServerMessage::RadioStatus(RadioStatus { online: true });
        let json = serde_json::to_value(message).expect("radio status should serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "type": "radio_status",
                "online": true
            })
        );
    }
}
