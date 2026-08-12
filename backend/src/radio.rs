use crate::bandmap::BandMapSpot;
use crate::dxcluster::DxClusterSpot;
use serde::{Deserialize, Serialize};

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
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
    #[serde(rename = "send_dxcluster_spot")]
    SendDxClusterSpot {
        frequency_hz: u64,
        call: String,
        comment: String,
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
