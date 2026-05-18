use crate::bands::band_for_frequency;
use crate::frequency::Frequency;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    RadioState(RadioState),
    LogEntry {
        contact: serde_json::Map<String, serde_json::Value>,
    },
    ContactDeleted {
        id: i64,
        log_id: i64,
    },
    CwSent {
        request_id: String,
    },
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
}

pub fn normalize_mode(mode: &rigctld::Mode) -> String {
    match mode {
        rigctld::Mode::USB | rigctld::Mode::LSB => "SSB".to_string(),
        other => other.to_string(),
    }
}

pub fn mode_for_request(requested: &str, frequency_hz: u64) -> Option<rigctld::Mode> {
    match requested.to_uppercase().as_str() {
        "CW" => Some(rigctld::Mode::CW),
        "FM" => Some(rigctld::Mode::FM),
        "AM" => Some(rigctld::Mode::AM),
        "SSB" => Some(ssb_mode_for_frequency(frequency_hz)),
        "USB" => Some(rigctld::Mode::USB),
        "LSB" => Some(rigctld::Mode::LSB),
        _ => None,
    }
}

fn ssb_mode_for_frequency(frequency_hz: u64) -> rigctld::Mode {
    let frequency = Frequency::from_hz(frequency_hz);

    match band_for_frequency(frequency).map(|band| band.meters) {
        Some(meters) if meters >= 40 => rigctld::Mode::LSB,
        _ => rigctld::Mode::USB,
    }
}
