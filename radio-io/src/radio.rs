use crate::{Band, band_for_frequency};
use radio_cat_rs::Mode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RadioClientMessage {
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
    #[serde(rename = "stop_keying")]
    StopKeying,
    SetWpm {
        wpm: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RadioServerMessage {
    RadioStatus(RadioStatus),
    RadioState(RadioState),
    Pong { request_id: String },
    MessageSent { request_id: String },
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

#[allow(clippy::large_enum_variant)]
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
        wait_for_completion: bool,
        completed: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    StopKeying,
    SetWpm(u8),
}

fn default_wait_for_completion() -> bool {
    true
}

pub fn normalize_mode(mode: &Mode) -> String {
    match mode {
        Mode::Lsb | Mode::Usb => "SSB".to_string(),
        Mode::Cw => "CW".to_string(),
        Mode::CwReverse => "CW-R".to_string(),
        Mode::Fm | Mode::Wfm => "FM".to_string(),
        Mode::Am => "AM".to_string(),
        Mode::Rtty | Mode::RttyReverse => "RTTY".to_string(),
        Mode::Psk
        | Mode::PskReverse
        | Mode::DataLsb
        | Mode::DataUsb
        | Mode::DataFm
        | Mode::DataAm
        | Mode::DigitalVoice => "DATA".to_string(),
    }
}

pub fn logger_mode_from_cat_mode(
    mode: &Mode,
    previous_logger_mode: Option<&str>,
    data_mode: &str,
    rtty_mode: &str,
) -> String {
    match previous_logger_mode.map(|mode| mode.trim().to_uppercase()) {
        Some(previous) if previous == "DATA" && configured_mode_is(data_mode, mode) => {
            "DATA".to_string()
        }
        Some(previous) if previous == "RTTY" && configured_mode_is(rtty_mode, mode) => {
            "RTTY".to_string()
        }
        _ => normalize_mode(mode),
    }
}

pub fn mode_candidates_for_request(
    requested: &str,
    frequency_hz: u64,
    bands: &[Band],
    data_mode: &str,
    rtty_mode: &str,
) -> Vec<Mode> {
    match requested.trim().to_uppercase().as_str() {
        "CW" => vec![Mode::Cw],
        "CW-R" => vec![Mode::CwReverse, Mode::Cw],
        "FM" => vec![Mode::Fm],
        "AM" => vec![Mode::Am],
        "SSB" => vec![ssb_mode_for_frequency(frequency_hz, bands)],
        "DATA" => data_mode.parse().into_iter().collect(),
        "RTTY" => rtty_mode.parse().into_iter().collect(),
        _ => Vec::new(),
    }
}

pub fn mode_is_phone(mode: &str) -> bool {
    matches!(mode.trim().to_uppercase().as_str(), "SSB" | "FM" | "AM")
}

fn ssb_mode_for_frequency(frequency_hz: u64, bands: &[Band]) -> Mode {
    match band_for_frequency(bands, frequency_hz)
        .map(|band| band.default_ssb_mode.trim().to_uppercase())
        .as_deref()
    {
        Some("LSB") => Mode::Lsb,
        _ => Mode::Usb,
    }
}

fn configured_mode_is(configured_mode: &str, observed_mode: &Mode) -> bool {
    configured_mode
        .parse::<Mode>()
        .is_ok_and(|mode| mode == *observed_mode)
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
    fn radio_protocol_serializes_status_and_deserializes_commands() {
        let status = serde_json::to_value(RadioServerMessage::RadioStatus(RadioStatus {
            online: true,
        }))
        .expect("status serializes");
        assert_eq!(
            status,
            serde_json::json!({ "type": "radio_status", "online": true })
        );

        let command = serde_json::from_value::<RadioClientMessage>(serde_json::json!({
            "type": "send_cw_text",
            "request_id": "cw-1",
            "text": "CQ"
        }))
        .expect("command deserializes");
        assert!(matches!(
            command,
            RadioClientMessage::SendCwText {
                wait_for_completion: true,
                ..
            }
        ));
    }

    #[test]
    fn mode_candidates_use_configured_digital_mappings() {
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
