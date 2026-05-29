use crate::db::{Contact, Log};
use serde_json::Value;
use std::collections::BTreeMap;

const ADIF_VERSION: &str = "3.1.0";
const PROGRAM_ID: &str = "Log73";
const PROGRAM_VERSION: &str = env!("CARGO_PKG_VERSION");

const PREFERRED_FIELD_ORDER: &[&str] = &[
    "QSO_DATE",
    "TIME_ON",
    "STATION_CALLSIGN",
    "OPERATOR",
    "CALL",
    "BAND",
    "FREQ",
    "MODE",
    "RST_SENT",
    "STX",
    "STX_STRING",
    "MY_STATE",
    "MY_CNTY",
    "MY_ARRL_SECT",
    "MY_GRIDSQUARE",
    "MY_CQ_ZONE",
    "RST_RCVD",
    "SRX",
    "SRX_STRING",
    "STATE",
    "CNTY",
    "ARRL_SECT",
    "GRIDSQUARE",
    "CQZ",
    "DXCC",
    "TX_PWR",
];

pub fn export_filename(log: &Log) -> String {
    format!("{}.adi", log.station_callsign.trim())
}

pub fn render_log(log: &Log, contacts: &[Contact]) -> Result<String, String> {
    let mut output = String::new();
    output.push_str("Log73 ADIF Export\n");
    output.push_str(&serialize_field("ADIF_VER", ADIF_VERSION));
    output.push_str(&serialize_field("PROGRAMID", PROGRAM_ID));
    output.push_str(&serialize_field("PROGRAMVERSION", PROGRAM_VERSION));
    output.push_str("<EOH>\n");

    for contact in contacts {
        for (name, value) in ordered_fields(contact_fields(contact, &log.station_callsign)?) {
            output.push_str(&serialize_field(&name, &value));
        }
        output.push_str("<EOR>\n");
    }

    Ok(output)
}

fn contact_fields(
    contact: &Contact,
    station_callsign: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut fields = BTreeMap::new();
    let has_epoch = contact.get("QSO_DATE_TIME_ON").is_some();

    if let Some(epoch) = contact_i64(contact.get("QSO_DATE_TIME_ON")) {
        let (date, time) = qso_datetime(epoch)?;
        fields.insert("QSO_DATE".to_string(), date);
        fields.insert("TIME_ON".to_string(), time);
    } else if has_epoch {
        return Err("contact has invalid QSO_DATE_TIME_ON".to_string());
    } else {
        return Err("contact is missing QSO_DATE_TIME_ON".to_string());
    }

    for (key, value) in contact {
        if should_skip_field(key) || (has_epoch && matches!(key.as_str(), "QSO_DATE" | "TIME_ON")) {
            continue;
        }

        let normalized = normalize_field_name(key);
        if normalized.is_empty() {
            continue;
        }

        let serialized = match normalized.as_str() {
            "FREQ" => frequency_value_string(value),
            "MODE" => mode_value_string(value),
            _ => value_string(value),
        };

        if let Some(serialized) = serialized
            && !serialized.is_empty()
        {
            fields.insert(normalized, serialized);
        }
    }

    if !fields.contains_key("STATION_CALLSIGN") {
        let callsign = station_callsign.trim().to_uppercase();
        if !callsign.is_empty() {
            fields.insert("STATION_CALLSIGN".to_string(), callsign);
        }
    }

    Ok(fields)
}

fn should_skip_field(name: &str) -> bool {
    matches!(
        name,
        "_id" | "_log_id" | "_status" | "ID" | "LOG_ID" | "JSON" | "QSO_DATE_TIME_ON"
    )
}

fn ordered_fields(fields: BTreeMap<String, String>) -> Vec<(String, String)> {
    let mut ordered = Vec::with_capacity(fields.len());
    let mut remaining = fields;

    for key in PREFERRED_FIELD_ORDER {
        if let Some(value) = remaining.remove(*key) {
            ordered.push(((*key).to_string(), value));
        }
    }

    ordered.extend(remaining);
    ordered
}

fn serialize_field(name: &str, value: &str) -> String {
    let name = normalize_field_name(name);
    let value = value.replace("\r\n", "\n").replace(['\r', '\n'], " ");
    format!("<{name}:{}>{value}", value.len())
}

fn normalize_field_name(name: &str) -> String {
    name.trim()
        .chars()
        .map(|ch| match ch {
            'a'..='z' => ch.to_ascii_uppercase(),
            'A'..='Z' | '0'..='9' | '_' => ch,
            _ => '_',
        })
        .collect()
}

fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(string) => Some(string.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn frequency_value_string(value: &Value) -> Option<String> {
    let hz = contact_i64(Some(value))?;
    Some(format_frequency_mhz(hz))
}

fn mode_value_string(value: &Value) -> Option<String> {
    value_string(value).map(|mode| match mode.trim().to_uppercase().as_str() {
        "CW-R" => "CW".to_string(),
        mode => mode.to_string(),
    })
}

fn format_frequency_mhz(hz: i64) -> String {
    let mhz = hz / 1_000_000;
    let fractional = (hz % 1_000_000).abs();
    if fractional == 0 {
        return mhz.to_string();
    }

    let mut text = format!("{mhz}.{fractional:06}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn contact_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64(),
        Value::String(string) => string.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn qso_datetime(epoch: i64) -> Result<(String, String), String> {
    if epoch < 0 {
        return Err("contact QSO time must be positive".to_string());
    }
    let days = epoch.div_euclid(86_400);
    let seconds = epoch.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds / 3_600;
    let minute = (seconds % 3_600) / 60;
    let second = seconds % 60;
    Ok((
        format!("{year:04}{month:02}{day:02}"),
        format!("{hour:02}{minute:02}{second:02}"),
    ))
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    fn test_log() -> Log {
        Log {
            id: 1,
            name: "Test".to_string(),
            contest_id: "SC-QSO-PARTY".to_string(),
            station_callsign: "N0CALL".to_string(),
            contest_params: json!({}),
        }
    }

    fn test_contact() -> Contact {
        Map::from_iter([
            ("QSO_DATE_TIME_ON".to_string(), json!(1_700_000_123_i64)),
            ("OPERATOR".to_string(), json!("K1ABC")),
            ("CALL".to_string(), json!("W1AW")),
            ("BAND".to_string(), json!("20m")),
            ("FREQ".to_string(), json!(14_250_000_i64)),
            ("MODE".to_string(), json!("SSB")),
            ("RST_SENT".to_string(), json!(59)),
            ("RST_RCVD".to_string(), json!(59)),
            ("SRX_STRING".to_string(), json!("NC")),
            ("STX_STRING".to_string(), json!("ABBE")),
            ("APP_LOG73_FOO".to_string(), json!("bar")),
            ("_id".to_string(), json!(99)),
            ("LOG_ID".to_string(), json!(1)),
        ])
    }

    #[test]
    fn export_filename_uses_adi_extension() {
        assert_eq!(export_filename(&test_log()), "N0CALL.adi");
    }

    #[test]
    fn render_log_emits_adif_header_and_records() {
        let text = render_log(&test_log(), &[test_contact()]).expect("ADIF export should render");

        assert!(text.starts_with("Log73 ADIF Export\n<ADIF_VER:5>3.1.0"));
        assert!(text.contains("<PROGRAMID:5>Log73"));
        assert!(text.contains("<EOH>\n"));
        assert!(text.contains("<QSO_DATE:8>20231114"));
        assert!(text.contains("<TIME_ON:6>221523"));
        assert!(text.contains("<STATION_CALLSIGN:6>N0CALL"));
        assert!(text.contains("<CALL:4>W1AW"));
        assert!(text.contains("<BAND:3>20m"));
        assert!(text.contains("<FREQ:5>14.25"));
        assert!(text.contains("<APP_LOG73_FOO:3>bar"));
        assert!(text.contains("<EOR>\n"));
        assert!(!text.contains("_id"));
        assert!(!text.contains("LOG_ID"));
    }

    #[test]
    fn render_log_maps_cw_reverse_to_adif_cw() {
        let mut contact = test_contact();
        contact.insert("MODE".to_string(), json!("CW-R"));
        let text = render_log(&test_log(), &[contact]).expect("ADIF export should render");

        assert!(text.contains("<MODE:2>CW"));
        assert!(!text.contains("CW-R"));
    }

    #[test]
    fn render_log_rejects_missing_epoch() {
        let error = render_log(&test_log(), &[Map::new()]).expect_err("epoch is required");
        assert_eq!(error, "contact is missing QSO_DATE_TIME_ON");
    }

    #[tokio::test]
    async fn render_log_output_parses_with_radif_tokio() {
        let mut contact = test_contact();
        contact.remove("APP_LOG73_FOO");
        let text = render_log(&test_log(), &[contact]).expect("ADIF export should render");
        let (mut writer, reader) = tokio::io::duplex(4096);
        let bytes = text.into_bytes();

        let writer_task = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;

            writer
                .write_all(&bytes)
                .await
                .expect("test ADIF should write to duplex stream");
            writer
                .shutdown()
                .await
                .expect("test ADIF duplex stream should close");
        });

        let adif = radif::parse_tokio(reader)
            .await
            .expect("generated ADIF should parse");
        writer_task.await.expect("writer task should complete");

        assert_eq!(adif.qso_count(), 1);
    }
}
