use crate::bandmap::BandMapSpot;
use crate::dxcluster::DxClusterSpot;
use serde::{Deserialize, Serialize};

pub use radio_io::{RadioCommand, RadioState, RadioStatus};

#[cfg(test)]
use radio_io::{
    logger_mode_from_cat_mode, mode_candidates_for_request, mode_is_phone, normalize_mode,
};

#[cfg(test)]
use crate::bands::Band;
#[cfg(test)]
use radio_io::Mode;

#[allow(clippy::large_enum_variant)]
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
    SupercheckpartialUpdate {
        callsigns: Vec<String>,
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
    WsjtXError {
        radio_id: i64,
        log_id: i64,
        message: String,
    },
    WsjtXTarget {
        radio_id: i64,
        logger_id: Option<String>,
        log_id: Option<i64>,
    },
    #[serde(rename = "dxcluster_spot")]
    DxClusterSpot {
        spot: Box<DxClusterSpot>,
    },
    #[serde(rename = "dxcluster_spot_deleted")]
    DxClusterSpotDeleted {
        id: u64,
    },
    #[serde(rename = "bandmap_subscription_ready")]
    BandMapSubscriptionReady,
    #[serde(rename = "bandmap_sequence")]
    BandMapSequence {
        sequence: u64,
    },
    #[serde(rename = "bandmap_spot")]
    BandMapSpot {
        sequence: u64,
        spot: Box<BandMapSpot>,
    },
    #[serde(rename = "bandmap_spot_deleted")]
    BandMapSpotDeleted {
        sequence: u64,
        id: u64,
    },
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
        #[serde(default = "default_wait_for_completion")]
        wait_for_completion: bool,
    },
    #[serde(rename = "send_dxcluster_spot")]
    SendDxClusterSpot {
        frequency_hz: u64,
        call: String,
        comment: String,
    },
    #[serde(rename = "stop_keying")]
    StopKeying,
    SetWpm {
        wpm: u8,
    },
    SetWsjtXTarget {
        enabled: bool,
    },
    #[serde(rename = "set_dxcluster_enabled")]
    SetDxClusterEnabled {
        enabled: bool,
    },
    #[serde(rename = "set_bandmap_enabled")]
    SetBandMapEnabled {
        enabled: bool,
    },
}

fn default_wait_for_completion() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bands() -> Vec<Band> {
        vec![
            Band {
                iaru_region: 2,
                name: "40m".to_string(),
                lower_hz: 7_000_000,
                upper_hz: 7_300_000,
                default_ssb_mode: "LSB".to_string(),
                sort_order: 1,
                cabrillo: "khz".to_string(),
            },
            Band {
                iaru_region: 2,
                name: "20m".to_string(),
                lower_hz: 14_000_000,
                upper_hz: 14_350_000,
                default_ssb_mode: "USB".to_string(),
                sort_order: 2,
                cabrillo: "khz".to_string(),
            },
        ]
    }

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
    fn serializes_bandmap_spot_server_message() {
        let message = ServerMessage::BandMapSpot {
            sequence: 12,
            spot: Box::new(BandMapSpot {
                id: 9,
                received_at: 1_700_000_000,
                spot_type: crate::bandmap::BandMapSpotType::Local,
                source: "local".to_string(),
                call_de: "LOCAL".to_string(),
                call_dx: "K1ABC".to_string(),
                frequency_hz: 14_074_000,
                utc: 1234,
                loc: None,
                comment: Some("worked".to_string()),
                rbn: None,
                band_name: Some("20m".to_string()),
                radio_id: Some(4),
                radio_name: Some("K4".to_string()),
                log_id: Some(7),
                exchange_fields: None,
            }),
        };
        let json = serde_json::to_value(message).expect("bandmap spot should serialize");

        assert_eq!(json["type"], "bandmap_spot");
        assert_eq!(json["sequence"], 12);
        assert_eq!(json["spot"]["id"], 9);
        assert_eq!(json["spot"]["spot_type"], "local");
    }

    #[test]
    fn serializes_bandmap_subscription_ready_server_message() {
        let json = serde_json::to_value(ServerMessage::BandMapSubscriptionReady)
            .expect("bandmap ready should serialize");

        assert_eq!(
            json,
            serde_json::json!({ "type": "bandmap_subscription_ready" })
        );
    }

    #[test]
    fn serializes_bandmap_sequence_server_message() {
        let json = serde_json::to_value(ServerMessage::BandMapSequence { sequence: 44 })
            .expect("bandmap sequence should serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "type": "bandmap_sequence",
                "sequence": 44
            })
        );
    }

    #[test]
    fn serializes_bandmap_spot_deleted_server_message() {
        let message = ServerMessage::BandMapSpotDeleted {
            sequence: 13,
            id: 9,
        };
        let json = serde_json::to_value(message).expect("bandmap delete should serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "type": "bandmap_spot_deleted",
                "sequence": 13,
                "id": 9
            })
        );
    }

    #[test]
    fn serializes_supercheckpartial_update_server_message() {
        let message = ServerMessage::SupercheckpartialUpdate {
            callsigns: vec!["K1ABC".to_string(), "W1AW".to_string()],
        };
        let json = serde_json::to_value(message).expect("scp update should serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "type": "supercheckpartial_update",
                "callsigns": ["K1ABC", "W1AW"]
            })
        );
    }

    #[test]
    fn serializes_wsjtx_errors_and_target_state() {
        let wsjtx = serde_json::to_value(ServerMessage::WsjtXError {
            radio_id: 2,
            log_id: 7,
            message: "bind failed".to_string(),
        })
        .expect("WSJT-X error should serialize");
        assert_eq!(
            wsjtx,
            serde_json::json!({
                "type": "wsjt_x_error",
                "radio_id": 2,
                "log_id": 7,
                "message": "bind failed"
            })
        );

        let target = serde_json::to_value(ServerMessage::WsjtXTarget {
            radio_id: 2,
            logger_id: Some("logger-1".to_string()),
            log_id: Some(7),
        })
        .expect("WSJT-X target should serialize");
        assert_eq!(
            target,
            serde_json::json!({
                "type": "wsjt_x_target",
                "radio_id": 2,
                "logger_id": "logger-1",
                "log_id": 7
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
    fn deserializes_wsjtx_target_client_message() {
        let message: ClientMessage = serde_json::from_value(serde_json::json!({
            "type": "set_wsjt_x_target",
            "enabled": true
        }))
        .expect("WSJT-X target message should deserialize");

        match message {
            ClientMessage::SetWsjtXTarget { enabled } => assert!(enabled),
            other => panic!("unexpected message: {other:?}"),
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
    fn deserializes_set_bandmap_enabled_client_message() {
        let message: ClientMessage = serde_json::from_value(serde_json::json!({
            "type": "set_bandmap_enabled",
            "enabled": true
        }))
        .expect("set_bandmap_enabled should deserialize");

        match message {
            ClientMessage::SetBandMapEnabled { enabled } => assert!(enabled),
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
            "text": "CQ ",
            "wait_for_completion": false
        }))
        .expect("send_cw_text should deserialize");

        match message {
            ClientMessage::SendCwText {
                request_id,
                text,
                wait_for_completion,
            } => {
                assert_eq!(request_id, "cw-123");
                assert_eq!(text, "CQ ");
                assert!(!wait_for_completion);
            }
            other => panic!("unexpected client message: {other:?}"),
        }
    }

    #[test]
    fn deserializes_stop_keying_client_message() {
        let message: ClientMessage = serde_json::from_value(serde_json::json!({
            "type": "stop_keying"
        }))
        .expect("stop_keying should deserialize");

        assert!(matches!(message, ClientMessage::StopKeying));
        assert!(
            serde_json::from_value::<ClientMessage>(serde_json::json!({
                "type": "stop_cw"
            }))
            .is_err()
        );
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
        assert_eq!(normalize_mode(&Mode::DataFm), "DATA");
        assert_eq!(normalize_mode(&Mode::Am), "AM");
        assert_eq!(normalize_mode(&Mode::DataUsb), "DATA");
        assert_eq!(normalize_mode(&Mode::Psk), "DATA");
        assert_eq!(normalize_mode(&Mode::DigitalVoice), "DATA");
        assert_eq!(normalize_mode(&Mode::RttyReverse), "RTTY");
    }

    #[test]
    fn mode_candidates_for_request_use_configured_digital_mappings() {
        assert_eq!(
            mode_candidates_for_request("CW", 14_000_000, &test_bands(), "DATA-USB", "RTTY"),
            vec![Mode::Cw]
        );
        assert_eq!(
            mode_candidates_for_request("CW-R", 14_000_000, &test_bands(), "DATA-USB", "RTTY"),
            vec![Mode::CwReverse, Mode::Cw]
        );
        assert_eq!(
            mode_candidates_for_request("DATA", 14_000_000, &test_bands(), "USB", "DATA-USB"),
            vec![Mode::Usb]
        );
        assert_eq!(
            mode_candidates_for_request("RTTY", 14_000_000, &test_bands(), "USB", "DATA-USB"),
            vec![Mode::DataUsb]
        );
        assert_eq!(
            mode_candidates_for_request("AM", 14_000_000, &test_bands(), "DATA-USB", "RTTY"),
            vec![Mode::Am]
        );
    }

    #[test]
    fn mapped_mode_preserves_matching_digital_logger_state_only() {
        assert_eq!(
            logger_mode_from_cat_mode(&Mode::Usb, Some("DATA"), "USB", "RTTY"),
            "DATA"
        );
        assert_eq!(
            logger_mode_from_cat_mode(&Mode::Usb, Some("RTTY"), "DATA-USB", "USB"),
            "RTTY"
        );
        assert_eq!(
            logger_mode_from_cat_mode(&Mode::Usb, Some("SSB"), "USB", "USB"),
            "SSB"
        );
        assert_eq!(
            logger_mode_from_cat_mode(&Mode::Usb, Some("CW"), "USB", "USB"),
            "SSB"
        );
    }

    #[test]
    fn classifies_phone_modes() {
        assert!(mode_is_phone("SSB"));
        assert!(mode_is_phone("fm"));
        assert!(mode_is_phone(" am "));
        assert!(!mode_is_phone("CW"));
        assert!(!mode_is_phone("RTTY"));
    }

    #[test]
    fn ssb_request_uses_band_dependent_sideband() {
        assert_eq!(
            mode_candidates_for_request("SSB", 7_200_000, &test_bands(), "DATA-USB", "RTTY"),
            vec![Mode::Lsb]
        );
        assert_eq!(
            mode_candidates_for_request("SSB", 14_200_000, &test_bands(), "DATA-USB", "RTTY"),
            vec![Mode::Usb]
        );
    }
}
