use crate::bands::band_for_frequency;
use crate::db::RadioConfig;
use crate::dxcluster::DxClusterSpot;
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
    MessageSent {
        request_id: String,
    },
    #[serde(rename = "dxcluster_spot")]
    DxClusterSpot {
        spot: Box<DxClusterSpot>,
    },
    #[serde(rename = "dxcluster_spot_deleted")]
    DxClusterSpotDeleted {
        id: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RadioStatus {
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RadioState {
    pub frequency_hz: u64,
    pub mode: String,
    pub rit_offset_hz: i32,
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
    #[serde(rename = "rit_clear")]
    RitClear,
    #[serde(rename = "rit_increment")]
    RitIncrement {
        hz: i32,
    },
    #[serde(rename = "rit_decrement")]
    RitDecrement {
        hz: i32,
    },
    SendMessage {
        request_id: String,
        mode: String,
        keys: Vec<String>,
        fields: serde_json::Map<String, serde_json::Value>,
    },
    SendCwText {
        request_id: String,
        text: String,
    },
    #[serde(rename = "send_dxcluster_spot")]
    SendDxClusterSpot {
        frequency_hz: u64,
        call: String,
        comment: String,
    },
    StopCw,
    SetWpm {
        wpm: u8,
    },
    #[serde(rename = "set_dxcluster_enabled")]
    SetDxClusterEnabled {
        enabled: bool,
    },
}

#[derive(Debug)]
pub enum RadioCommand {
    SetFrequency(u64),
    SetMode(String),
    RitClear,
    RitIncrement(i32),
    RitDecrement(i32),
    SendMessage {
        mode: String,
        keys: Vec<String>,
        fields: serde_json::Map<String, serde_json::Value>,
        completed: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SendCwText {
        text: String,
        completed: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    StopCw,
    SetWpm(u8),
    ReloadConfig(Box<RadioConfig>),
}

pub fn normalize_mode(mode: &Mode) -> String {
    match mode {
        Mode::Lsb | Mode::Usb => "SSB".to_string(),
        Mode::Cw => "CW".to_string(),
        Mode::CwReverse => "CW-R".to_string(),
        Mode::Fm | Mode::Wfm => "FM".to_string(),
        Mode::Am => "AM".to_string(),
        Mode::Rtty
        | Mode::RttyReverse
        | Mode::Psk
        | Mode::PskReverse
        | Mode::DataLsb
        | Mode::DataUsb
        | Mode::DataFm
        | Mode::DataAm
        | Mode::DigitalVoice => "RTTY".to_string(),
    }
}

pub fn mode_candidates_for_request(requested: &str, frequency_hz: u64) -> Vec<Mode> {
    match requested.trim().to_uppercase().as_str() {
        "CW" => vec![Mode::Cw],
        "CW-R" => vec![Mode::CwReverse, Mode::Cw],
        "FM" => vec![Mode::Fm],
        "SSB" => vec![ssb_mode_for_frequency(frequency_hz)],
        "FT8" | "JT65" | "JT9" | "MFSK" | "PSK" => vec![Mode::DataUsb, Mode::Rtty],
        "RTTY" => vec![Mode::Rtty, Mode::DataUsb],
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
    fn serializes_dxcluster_spot_server_message() {
        let message = ServerMessage::DxClusterSpot {
            spot: Box::new(DxClusterSpot {
                id: 7,
                received_at: 1_700_000_000,
                source: "dx".to_string(),
                call_de: "N0CALL".to_string(),
                call_dx: "K1ABC".to_string(),
                frequency_hz: 14_074_000,
                utc: 1234,
                loc: None,
                comment: Some("test".to_string()),
                rbn: None,
            }),
        };
        let json = serde_json::to_value(message).expect("dxcluster spot should serialize");

        assert_eq!(json["type"], "dxcluster_spot");
        assert_eq!(json["spot"]["id"], 7);
        assert_eq!(json["spot"]["call_dx"], "K1ABC");
    }

    #[test]
    fn serializes_dxcluster_spot_deleted_server_message() {
        let message = ServerMessage::DxClusterSpotDeleted { id: 7 };
        let json = serde_json::to_value(message).expect("dxcluster delete should serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "type": "dxcluster_spot_deleted",
                "id": 7
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
    fn deserializes_set_dxcluster_enabled_client_message() {
        let message: ClientMessage = serde_json::from_value(serde_json::json!({
            "type": "set_dxcluster_enabled",
            "enabled": true
        }))
        .expect("set_dxcluster_enabled should deserialize");

        match message {
            ClientMessage::SetDxClusterEnabled { enabled } => assert!(enabled),
            other => panic!("unexpected client message: {other:?}"),
        }
    }

    #[test]
    fn deserializes_rit_client_messages() {
        let clear_message: ClientMessage = serde_json::from_value(serde_json::json!({
            "type": "rit_clear"
        }))
        .expect("rit_clear should deserialize");
        assert!(matches!(clear_message, ClientMessage::RitClear));

        let increment_message: ClientMessage = serde_json::from_value(serde_json::json!({
            "type": "rit_increment",
            "hz": 25
        }))
        .expect("rit_increment should deserialize");
        match increment_message {
            ClientMessage::RitIncrement { hz } => assert_eq!(hz, 25),
            other => panic!("unexpected client message: {other:?}"),
        }

        let decrement_message: ClientMessage = serde_json::from_value(serde_json::json!({
            "type": "rit_decrement",
            "hz": 10
        }))
        .expect("rit_decrement should deserialize");
        match decrement_message {
            ClientMessage::RitDecrement { hz } => assert_eq!(hz, 10),
            other => panic!("unexpected client message: {other:?}"),
        }
    }

    #[test]
    fn deserializes_send_message_client_message() {
        let message: ClientMessage = serde_json::from_value(serde_json::json!({
            "type": "send_message",
            "request_id": "msg-123",
            "mode": "run",
            "keys": ["F1", "F2"],
            "fields": {
                "CALL": "K1ABC"
            }
        }))
        .expect("send_message should deserialize");

        match message {
            ClientMessage::SendMessage {
                request_id,
                mode,
                keys,
                fields,
            } => {
                assert_eq!(request_id, "msg-123");
                assert_eq!(mode, "run");
                assert_eq!(keys, vec!["F1".to_string(), "F2".to_string()]);
                assert_eq!(fields.get("CALL"), Some(&serde_json::json!("K1ABC")));
            }
            other => panic!("unexpected client message: {other:?}"),
        }
    }

    #[test]
    fn deserializes_send_cw_text_client_message() {
        let message: ClientMessage = serde_json::from_value(serde_json::json!({
            "type": "send_cw_text",
            "request_id": "cw-123",
            "text": "CQ "
        }))
        .expect("send_cw_text should deserialize");

        match message {
            ClientMessage::SendCwText { request_id, text } => {
                assert_eq!(request_id, "cw-123");
                assert_eq!(text, "CQ ");
            }
            other => panic!("unexpected client message: {other:?}"),
        }
    }

    #[test]
    fn deserializes_send_dxcluster_spot_client_message() {
        let message: ClientMessage = serde_json::from_value(serde_json::json!({
            "type": "send_dxcluster_spot",
            "frequency_hz": 14_074_000,
            "call": "K1ABC",
            "comment": "CQ TEST"
        }))
        .expect("send_dxcluster_spot should deserialize");

        match message {
            ClientMessage::SendDxClusterSpot {
                frequency_hz,
                call,
                comment,
            } => {
                assert_eq!(frequency_hz, 14_074_000);
                assert_eq!(call, "K1ABC");
                assert_eq!(comment, "CQ TEST");
            }
            other => panic!("unexpected client message: {other:?}"),
        }
    }

    #[test]
    fn normalizes_cat_modes_to_logger_modes() {
        assert_eq!(normalize_mode(&Mode::Usb), "SSB");
        assert_eq!(normalize_mode(&Mode::CwReverse), "CW-R");
        assert_eq!(normalize_mode(&Mode::DataFm), "FM");
        assert_eq!(normalize_mode(&Mode::Am), "AM");
        assert_eq!(normalize_mode(&Mode::DataUsb), "RTTY");
        assert_eq!(normalize_mode(&Mode::Digital), "RTTY");
        assert_eq!(normalize_mode(&Mode::DigitalVoice), "RTTY");
    }

    #[test]
    fn mode_candidates_for_request_use_fallbacks() {
        assert_eq!(
            mode_candidates_for_request("CW", 14_000_000),
            vec![Mode::Cw]
        );
        assert_eq!(
            mode_candidates_for_request("CW-R", 14_000_000),
            vec![Mode::CwReverse, Mode::Cw]
        );
        assert_eq!(
            mode_candidates_for_request("FT8", 14_000_000),
            vec![Mode::DataUsb, Mode::Rtty]
        );
        assert_eq!(
            mode_candidates_for_request("RTTY", 14_000_000),
            vec![Mode::Rtty, Mode::DataUsb]
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
