use crate::contest_rules::{ContestRules, ExchangeField};
use crate::db::{Contact, ContactFields, Log, build_contact, contact_adif, contact_adif_value};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
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

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportMapping {
    AdifField { field: String },
    FixedConfig,
    FixedValue { value: String },
}

pub type ImportMappings = BTreeMap<String, ImportMapping>;

#[derive(Debug, Clone, Serialize)]
pub struct ImportError {
    pub line: usize,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct ImportContact {
    pub line: usize,
    pub contact: Contact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdifRecord {
    pub line: usize,
    pub fields: BTreeMap<String, String>,
}

pub fn export_filename(log: &Log) -> String {
    format!("{}.adi", log.station_callsign.trim())
}

pub fn import_contacts(
    log: &Log,
    rules: &ContestRules,
    text: &str,
    mappings: &ImportMappings,
) -> Result<Vec<ImportContact>, ImportError> {
    let records = parse_records(text).map_err(|error| ImportError { line: 1, error })?;
    if records.is_empty() {
        return Err(ImportError {
            line: 1,
            error: "ADIF file does not contain any QSO records".to_string(),
        });
    }

    let mut contacts = Vec::with_capacity(records.len());
    for record in records {
        let contact = import_record(log, rules, &record, mappings)?;
        contacts.push(ImportContact {
            line: record.line,
            contact,
        });
    }
    Ok(contacts)
}

pub fn parse_records(text: &str) -> Result<Vec<AdifRecord>, String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut line = 1;
    let mut in_header = true;
    let mut pending = BTreeMap::new();
    let mut pending_line = None;
    let mut current = BTreeMap::new();
    let mut current_line = None;
    let mut records = Vec::new();

    while index < bytes.len() {
        if bytes[index] != b'<' {
            if bytes[index] == b'\n' {
                line += 1;
            }
            index += 1;
            continue;
        }

        let tag_line = line;
        let tag_start = index;
        index += 1;
        let tag_content_start = index;
        while index < bytes.len() && bytes[index] != b'>' {
            if bytes[index] == b'\n' {
                line += 1;
            }
            index += 1;
        }
        if index >= bytes.len() {
            return Err(format!("line {tag_line}: unterminated ADIF tag"));
        }

        let tag = std::str::from_utf8(&bytes[tag_content_start..index])
            .map_err(|_| format!("line {tag_line}: ADIF tag is not valid UTF-8"))?;
        index += 1;

        let normalized_tag = tag.trim().to_ascii_uppercase();
        if normalized_tag == "EOH" {
            in_header = false;
            pending.clear();
            pending_line = None;
            continue;
        }
        if normalized_tag == "EOR" {
            if in_header {
                if !pending.is_empty() {
                    records.push(AdifRecord {
                        line: pending_line.unwrap_or(tag_line),
                        fields: std::mem::take(&mut pending),
                    });
                }
            } else if !current.is_empty() {
                records.push(AdifRecord {
                    line: current_line.unwrap_or(tag_line),
                    fields: std::mem::take(&mut current),
                });
                current_line = None;
            }
            continue;
        }

        let Some((name, length)) = parse_tag(tag) else {
            return Err(format!("line {tag_line}: invalid ADIF tag <{tag}>"));
        };
        if index + length > bytes.len() {
            return Err(format!("line {tag_line}: ADIF field {name} is truncated"));
        }
        let value_bytes = &bytes[index..index + length];
        let value = std::str::from_utf8(value_bytes)
            .map_err(|_| format!("line {tag_line}: ADIF field {name} is not valid UTF-8"))?
            .trim()
            .to_string();
        line += value_bytes.iter().filter(|byte| **byte == b'\n').count();
        index += length;

        if name.is_empty() {
            return Err(format!("line {tag_line}: ADIF field name is empty"));
        }

        if in_header {
            if pending.is_empty() {
                pending_line = Some(tag_line);
            }
            pending.insert(name, value);
        } else {
            if current.is_empty() {
                current_line = Some(tag_line);
            }
            current.insert(name, value);
        }

        if tag_start == index {
            index += 1;
        }
    }

    if !current.is_empty() {
        records.push(AdifRecord {
            line: current_line.unwrap_or(line),
            fields: current,
        });
    } else if in_header && !pending.is_empty() {
        records.push(AdifRecord {
            line: pending_line.unwrap_or(line),
            fields: pending,
        });
    }

    Ok(records)
}

fn parse_tag(tag: &str) -> Option<(String, usize)> {
    let mut parts = tag.split(':');
    let name = normalize_field_name(parts.next()?);
    let length = parts.next()?.trim().parse::<usize>().ok()?;
    Some((name, length))
}

fn import_record(
    log: &Log,
    rules: &ContestRules,
    record: &AdifRecord,
    mappings: &ImportMappings,
) -> Result<Contact, ImportError> {
    let line = record.line;
    let mut adif = record
        .fields
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "ID" | "_ID" | "_LOG_ID" | "_STATUS"))
        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
        .collect::<ContactFields>();

    let epoch = import_qso_epoch(record).map_err(|error| ImportError { line, error })?;
    adif.insert("QSO_DATE_TIME_ON".to_string(), Value::Number(epoch.into()));

    let frequency =
        import_frequency_hz(required_field(record, "FREQ")?).ok_or_else(|| ImportError {
            line,
            error: "FREQ is invalid".to_string(),
        })?;
    adif.insert("FREQ".to_string(), Value::Number(frequency.into()));

    for name in ["STATION_CALLSIGN", "CALL", "BAND", "MODE"] {
        required_field(record, name)?;
    }

    for field in &rules.exchange {
        let Some(mapping) = mappings.get(&field.adif) else {
            return Err(ImportError {
                line,
                error: format!("{} mapping is required", field.name),
            });
        };
        let value = mapping_value(log, field, record, mapping)?;
        adif.insert(field.adif.clone(), Value::String(value));
    }

    Ok(build_contact(Map::new(), adif))
}

fn mapping_value(
    log: &Log,
    field: &ExchangeField,
    record: &AdifRecord,
    mapping: &ImportMapping,
) -> Result<String, ImportError> {
    let line = record.line;
    let value = match mapping {
        ImportMapping::AdifField { field: source } => {
            let source = normalize_field_name(source);
            record
                .fields
                .get(&source)
                .cloned()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| ImportError {
                    line,
                    error: format!("{} source field {source} is missing", field.name),
                })?
        }
        ImportMapping::FixedConfig => {
            fixed_config_value(log, field).ok_or_else(|| ImportError {
                line,
                error: format!("{} does not have a configured fixed value", field.name),
            })?
        }
        ImportMapping::FixedValue { value } => value.trim().to_string(),
    };

    if value.is_empty() {
        return Err(ImportError {
            line,
            error: format!("{} value is required", field.name),
        });
    }
    Ok(value)
}

fn fixed_config_value(log: &Log, field: &ExchangeField) -> Option<String> {
    if let Some(source_param) = &field.source_param {
        let value = log.contest_params.as_object()?.get(source_param)?;
        return value_string(value).filter(|value| !value.is_empty());
    }
    field.default.as_ref().and_then(value_string)
}

fn required_field<'a>(record: &'a AdifRecord, name: &str) -> Result<&'a str, ImportError> {
    record
        .fields
        .get(name)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ImportError {
            line: record.line,
            error: format!("{name} is required"),
        })
}

fn import_qso_epoch(record: &AdifRecord) -> Result<i64, String> {
    let date = required_field(record, "QSO_DATE").map_err(|error| error.error)?;
    let time = required_field(record, "TIME_ON").map_err(|error| error.error)?;
    qso_epoch(date, time).ok_or_else(|| "QSO_DATE/TIME_ON is invalid".to_string())
}

fn qso_epoch(date: &str, time: &str) -> Option<i64> {
    if date.len() != 8 || !date.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let year = date[0..4].parse::<i32>().ok()?;
    let month = date[4..6].parse::<u32>().ok()?;
    let day = date[6..8].parse::<u32>().ok()?;

    let time = time.trim();
    if !matches!(time.len(), 4 | 6) || !time.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let hour = time[0..2].parse::<i64>().ok()?;
    let minute = time[2..4].parse::<i64>().ok()?;
    let second = if time.len() == 6 {
        time[4..6].parse::<i64>().ok()?
    } else {
        0
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    let days = days_from_civil(year, month, day)?;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let original_year = year;
    let original_month = month;
    let original_day = day;
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = i64::from(month);
    let day = i64::from(day);
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let (roundtrip_year, roundtrip_month, roundtrip_day) = civil_from_days(days);
    if roundtrip_year == original_year
        && roundtrip_month == original_month
        && roundtrip_day == original_day
    {
        Some(days)
    } else {
        None
    }
}

fn import_frequency_hz(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(integer) = value.parse::<i64>() {
        if integer > 1000 {
            return Some(integer);
        }
    }
    let mhz = value.parse::<f64>().ok()?;
    if !mhz.is_finite() || mhz <= 0.0 {
        return None;
    }
    Some((mhz * 1_000_000.0).round() as i64)
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
    let has_epoch = contact_adif_value(contact, "QSO_DATE_TIME_ON").is_some();

    if let Some(epoch) = contact_i64(contact_adif_value(contact, "QSO_DATE_TIME_ON")) {
        let (date, time) = qso_datetime(epoch)?;
        fields.insert("QSO_DATE".to_string(), date);
        fields.insert("TIME_ON".to_string(), time);
    } else if has_epoch {
        return Err("contact has invalid QSO_DATE_TIME_ON".to_string());
    } else {
        return Err("contact is missing QSO_DATE_TIME_ON".to_string());
    }

    let fields_source = contact_adif(contact)
        .cloned()
        .unwrap_or_else(|| contact.clone());
    for (key, value) in &fields_source {
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
    name.starts_with('_') || matches!(name, "ID" | "LOG_ID" | "JSON" | "QSO_DATE_TIME_ON")
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
        crate::db::build_contact(
            Map::from_iter([
                ("id".to_string(), json!(99)),
                ("logId".to_string(), json!(1)),
            ]),
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
            ]),
        )
    }

    fn test_rules() -> ContestRules {
        ContestRules {
            contest: "SC-QSO-PARTY".to_string(),
            display_name: "SC QSO Party".to_string(),
            allowed_bands: Vec::new(),
            allowed_modes: Vec::new(),
            define: Vec::new(),
            exchange: vec![
                crate::contest_rules::ExchangeField {
                    name: "County".to_string(),
                    field_type: "String:4".to_string(),
                    adif: "STX_STRING".to_string(),
                    fixed: Some(true),
                    default: None,
                    source_param: Some("County".to_string()),
                    regex: None,
                    in_sets: Vec::new(),
                    valid_values: Vec::new(),
                    is_sent: true,
                },
                crate::contest_rules::ExchangeField {
                    name: "Exchange".to_string(),
                    field_type: "String:4".to_string(),
                    adif: "SRX_STRING".to_string(),
                    fixed: None,
                    default: None,
                    source_param: None,
                    regex: None,
                    in_sets: Vec::new(),
                    valid_values: Vec::new(),
                    is_sent: false,
                },
            ],
            qso_columns: Vec::new(),
            qso_column_fields: BTreeMap::new(),
            log_params: Vec::new(),
            qso_points: None,
            dupe_key: Vec::new(),
            multipliers: Vec::new(),
            bonus_points: Vec::new(),
            power_multiplier: Vec::new(),
            cabrillo: None,
            metadata: None,
        }
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
        crate::db::set_contact_adif(&mut contact, "MODE", json!("CW-R"));
        let text = render_log(&test_log(), &[contact]).expect("ADIF export should render");

        assert!(text.contains("<MODE:2>CW"));
        assert!(!text.contains("CW-R"));
    }

    #[test]
    fn render_log_rejects_missing_epoch() {
        let error = render_log(
            &test_log(),
            &[crate::db::build_contact(Map::new(), Map::new())],
        )
        .expect_err("epoch is required");
        assert_eq!(error, "contact is missing QSO_DATE_TIME_ON");
    }

    #[tokio::test]
    async fn render_log_output_parses_with_radif_tokio() {
        let mut contact = test_contact();
        if let Some(adif) = contact.get_mut("adif").and_then(Value::as_object_mut) {
            adif.remove("APP_LOG73_FOO");
        }
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

    #[test]
    fn parse_records_skips_header_and_tracks_record_line() {
        let records = parse_records("Header\n<ADIF_VER:5>3.1.0<EOH>\n<CALL:4>W1AW<EOR>\n")
            .expect("ADIF should parse");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].line, 3);
        assert_eq!(
            records[0].fields.get("CALL").map(String::as_str),
            Some("W1AW")
        );
    }

    #[test]
    fn parse_records_accepts_typed_tags() {
        let records = parse_records("<CALL:4:S>W1AW<EOR>").expect("ADIF should parse");

        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].fields.get("CALL").map(String::as_str),
            Some("W1AW")
        );
    }

    #[test]
    fn parse_records_ignores_typed_header_tags() {
        let records = parse_records("Log73\n<ADIF_VER:5:S>3.1.0<EOH>\n<CALL:4:S>W1AW<EOR>\n")
            .expect("ADIF should parse");

        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].fields.get("CALL").map(String::as_str),
            Some("W1AW")
        );
    }

    #[test]
    fn parse_records_ignores_extra_tag_suffixes() {
        let records = parse_records("<CALL:4:SOMETHING>W1AW<EOR>").expect("ADIF should parse");

        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].fields.get("CALL").map(String::as_str),
            Some("W1AW")
        );
    }

    #[test]
    fn parse_records_requires_length_in_second_tag_segment() {
        let error = parse_records("<CALL:S:4>W1AW<EOR>")
            .expect_err("invalid typed length should be rejected");

        assert!(error.contains("invalid ADIF tag <CALL:S:4>"));
    }

    #[test]
    fn parse_records_accepts_zero_length_typed_fields() {
        let records = parse_records("<COMMENT:0:S><EOR>").expect("ADIF should parse");

        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].fields.get("COMMENT").map(String::as_str),
            Some("")
        );
    }

    #[test]
    fn import_contacts_maps_required_exchange_and_keeps_source_field() {
        let mut log = test_log();
        log.contest_params = json!({ "County": "ABBE" });
        let mappings = BTreeMap::from([
            ("STX_STRING".to_string(), ImportMapping::FixedConfig),
            (
                "SRX_STRING".to_string(),
                ImportMapping::AdifField {
                    field: "N1MM_SECTION".to_string(),
                },
            ),
        ]);

        let imported = import_contacts(
            &log,
            &test_rules(),
            "<EOH><QSO_DATE:8>20231114<TIME_ON:6>221523<STATION_CALLSIGN:6>N0CALL<CALL:4>W1AW<BAND:3>20m<FREQ:5>14.25<MODE:2>CW<N1MM_SECTION:2>NC<EOR>",
            &mappings,
        )
        .expect("contact should import");
        let contact = &imported[0].contact;

        assert_eq!(contact.get("meta"), Some(&json!({})));
        assert!(contact.get("CALL").is_none());
        assert_eq!(
            contact_adif_value(contact, "QSO_DATE_TIME_ON"),
            Some(&json!(1_700_000_123_i64))
        );
        assert_eq!(
            contact_adif_value(contact, "FREQ"),
            Some(&json!(14_250_000_i64))
        );
        assert_eq!(
            contact_adif_value(contact, "STX_STRING"),
            Some(&json!("ABBE"))
        );
        assert_eq!(
            contact_adif_value(contact, "SRX_STRING"),
            Some(&json!("NC"))
        );
        assert_eq!(
            contact_adif_value(contact, "N1MM_SECTION"),
            Some(&json!("NC"))
        );
    }

    #[test]
    fn import_contacts_rejects_missing_core_field() {
        let mappings = BTreeMap::from([
            (
                "STX_STRING".to_string(),
                ImportMapping::FixedValue {
                    value: "ABBE".to_string(),
                },
            ),
            (
                "SRX_STRING".to_string(),
                ImportMapping::FixedValue {
                    value: "NC".to_string(),
                },
            ),
        ]);

        let error = import_contacts(
            &test_log(),
            &test_rules(),
            "<EOH><QSO_DATE:8>20231114<TIME_ON:6>221523<STATION_CALLSIGN:6>N0CALL<CALL:4>W1AW<FREQ:5>14.25<MODE:2>CW<EOR>",
            &mappings,
        )
        .expect_err("band is required");

        assert_eq!(error.error, "BAND is required");
    }
}
