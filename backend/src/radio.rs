use crate::bands::band_for_frequency;
use crate::db::RadioConfig;
use radio_cat_rs::{Frequency, Mode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    RadioStatus(RadioStatus),
    RadioState(RadioState),
    Pong {
        request_id: String,
    },
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
    Ping {
        request_id: String,
    },
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
        Mode::Lsb
        | Mode::Usb
        | Mode::LsbD1
        | Mode::UsbD1
        | Mode::LsbD2
        | Mode::UsbD2
        | Mode::LsbD3
        | Mode::UsbD3 => "SSB".to_string(),
        Mode::Cw => "CW".to_string(),
        Mode::Cwr => "CW-R".to_string(),
        Mode::Fm | Mode::Fmn | Mode::Wfm => "FM".to_string(),
        Mode::Am => "AM".to_string(),
        Mode::Psk | Mode::Pskr => "PSK".to_string(),
        Mode::Rtty
        | Mode::Rttyr
        | Mode::PktLsb
        | Mode::PktUsb
        | Mode::PktFm
        | Mode::PktAm
        | Mode::PktFmn => "RTTY".to_string(),
        _ => "RTTY".to_string(),
    }
}

pub fn mode_candidates_for_request(requested: &str, frequency_hz: u64) -> Vec<Mode> {
    match requested.trim().to_uppercase().as_str() {
        "CW" => vec![Mode::Cw],
        "CW-R" => vec![Mode::Cwr, Mode::Cw],
        "FM" => vec![Mode::Fm],
        "SSB" => vec![ssb_mode_for_frequency(frequency_hz)],
        "FT8" | "JT65" | "JT9" | "MFSK" | "PSK" => vec![Mode::PktUsb, Mode::Rtty],
        "RTTY" => vec![Mode::Rtty, Mode::PktUsb],
        _ => Vec::new(),
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

    #[test]
    fn serializes_pong_server_message() {
        let message = ServerMessage::Pong {
            request_id: "ping-123".to_string(),
        };
        let json = serde_json::to_value(message).expect("pong should serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "type": "pong",
                "request_id": "ping-123"
            })
        );
    }

    #[test]
    fn deserializes_ping_client_message() {
        let message: ClientMessage = serde_json::from_value(serde_json::json!({
            "type": "ping",
            "request_id": "ping-123"
        }))
        .expect("ping should deserialize");

        match message {
            ClientMessage::Ping { request_id } => assert_eq!(request_id, "ping-123"),
            other => panic!("unexpected client message: {other:?}"),
        }
    }

    #[test]
    fn normalizes_cat_modes_to_logger_modes() {
        assert_eq!(normalize_mode(&Mode::Usb), "SSB");
        assert_eq!(normalize_mode(&Mode::Cwr), "CW-R");
        assert_eq!(normalize_mode(&Mode::Fmn), "FM");
        assert_eq!(normalize_mode(&Mode::Am), "AM");
        assert_eq!(normalize_mode(&Mode::PktUsb), "RTTY");
        assert_eq!(normalize_mode(&Mode::Psk), "PSK");
        assert_eq!(normalize_mode(&Mode::DStar), "RTTY");
    }

    #[test]
    fn mode_candidates_for_request_use_fallbacks() {
        assert_eq!(
            mode_candidates_for_request("CW", 14_000_000),
            vec![Mode::Cw]
        );
        assert_eq!(
            mode_candidates_for_request("CW-R", 14_000_000),
            vec![Mode::Cwr, Mode::Cw]
        );
        assert_eq!(
            mode_candidates_for_request("FT8", 14_000_000),
            vec![Mode::PktUsb, Mode::Rtty]
        );
        assert_eq!(
            mode_candidates_for_request("RTTY", 14_000_000),
            vec![Mode::Rtty, Mode::PktUsb]
        );
    }

    #[test]
    fn ssb_request_uses_band_dependent_sideband() {
        assert_eq!(
            mode_candidates_for_request("SSB", 7_200_000),
            vec![Mode::Lsb]
        );
        assert_eq!(
            mode_candidates_for_request("SSB", 14_200_000),
            vec![Mode::Usb]
        );
    }
}
