use crate::contest_rules::{ContestParam, ContestRules, ExchangeField};
use crate::db::{Contact, Log};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

const START_OF_LOG_VERSION: &str = "3.0";
const CREATED_BY_VALUE: &str = "Log73";

pub fn export_filename(log: &Log) -> String {
    format!("{}.log", log.station_callsign.trim())
}

pub fn render_log(
    rules: &ContestRules,
    log: &Log,
    contacts: &[Contact],
    export_params: &Value,
    claimed_score: i64,
) -> Result<String, String> {
    let cabrillo = rules
        .cabrillo
        .as_ref()
        .ok_or_else(|| format!("contest {} does not define Cabrillo export", rules.contest))?;
    let empty_export_values = Map::new();
    let export_values = match export_params.as_object() {
        Some(values) => values,
        None if export_params.is_null() => &empty_export_values,
        None => return Err("export parameters must be an object".to_string()),
    };
    let empty_log_values = Map::new();
    let log_values = match log.contest_params.as_object() {
        Some(values) => values,
        None if log.contest_params.is_null() => &empty_log_values,
        None => return Err("log contest parameters must be an object".to_string()),
    };

    let mut lines = vec![format!("START-OF-LOG: {START_OF_LOG_VERSION}")];
    append_header_line(&mut lines, "CREATED-BY", CREATED_BY_VALUE)?;
    append_header_line(&mut lines, "CALLSIGN", log.station_callsign.trim())?;
    append_header_line(&mut lines, "CONTEST", &log.contest_id)?;
    append_header_line(&mut lines, "CLAIMED-SCORE", &claimed_score.to_string())?;

    for field in &cabrillo.fixed_fields {
        if is_reserved_tag(&field.name) {
            continue;
        }
        append_header_value(&mut lines, &field.name, &field.value, None)?;
    }

    for field in &cabrillo.log_fields {
        if is_reserved_tag(&field.name) {
            continue;
        }
        if let Some(value) = parameter_value(log_values, field) {
            append_header_value(&mut lines, &field.name, &value, field.max_lines)?;
        }
    }

    for field in &cabrillo.export_fields {
        if is_reserved_tag(&field.name) {
            continue;
        }
        if let Some(value) = parameter_value(export_values, field) {
            append_header_value(&mut lines, &field.name, &value, field.max_lines)?;
        }
    }

    append_operators_lines(&mut lines, contacts, &log.station_callsign)?;

    for contact in contacts {
        lines.push(render_qso_line(rules, log, contact)?);
    }

    lines.push("END-OF-LOG:".to_string());
    Ok(lines.join("\r\n") + "\r\n")
}

fn is_reserved_tag(tag: &str) -> bool {
    matches!(
        tag.trim().to_uppercase().as_str(),
        "CREATED-BY" | "CALLSIGN" | "CONTEST" | "CLAIMED-SCORE" | "OPERATORS"
    )
}

fn append_header_line(lines: &mut Vec<String>, tag: &str, value: &str) -> Result<(), String> {
    let tag = normalized_tag(tag);
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    lines.push(format!("{tag}: {value}"));
    Ok(())
}

fn append_header_value(
    lines: &mut Vec<String>,
    tag: &str,
    value: &str,
    max_lines: Option<usize>,
) -> Result<(), String> {
    let tag = normalized_tag(tag);
    let split_lines = split_multiline_value(value, max_lines)?;
    for line in split_lines {
        append_header_line(lines, &tag, &line)?;
    }
    Ok(())
}

fn split_multiline_value(value: &str, max_lines: Option<usize>) -> Result<Vec<String>, String> {
    let mut lines = value
        .replace("\r\n", "\n")
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    if let Some(max_lines) = max_lines
        && lines.len() > max_lines
    {
        return Err(format!("value exceeds maximum line count of {max_lines}"));
    }
    if lines.is_empty() {
        lines.push(value.trim().to_string());
    }
    Ok(lines)
}

fn append_operators_lines(
    lines: &mut Vec<String>,
    contacts: &[Contact],
    station_callsign: &str,
) -> Result<(), String> {
    let mut operators = contacts
        .iter()
        .filter_map(|contact| token_string(contact.get("OPERATOR")))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    if operators.is_empty() {
        operators.push(station_callsign.trim().to_uppercase());
    }

    let mut current = String::new();
    for operator in operators {
        let candidate = if current.is_empty() {
            operator.clone()
        } else {
            format!("{current} {operator}")
        };
        if candidate.chars().count() > 75 && !current.is_empty() {
            append_header_line(lines, "OPERATORS", &current)?;
            current = operator;
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        append_header_line(lines, "OPERATORS", &current)?;
    }

    Ok(())
}

fn render_qso_line(rules: &ContestRules, log: &Log, contact: &Contact) -> Result<String, String> {
    let frequency = qso_frequency_token(contact)
        .ok_or_else(|| "contact is missing frequency for Cabrillo export".to_string())?;
    let mode = qso_mode_token(contact).ok_or_else(|| "contact is missing mode".to_string())?;
    let epoch = contact_i64(contact.get("QSO_DATE_TIME_ON"))
        .ok_or_else(|| "contact is missing QSO date/time".to_string())?;
    let (date, time) = qso_datetime(epoch)?;
    let station_callsign = contact_string(contact.get("STATION_CALLSIGN"))
        .map(|value| value.to_uppercase())
        .unwrap_or_else(|| log.station_callsign.trim().to_uppercase());
    let their_callsign = token_string(contact.get("CALL"))
        .ok_or_else(|| "contact is missing callsign".to_string())?;

    let sent_fields = rules
        .exchange
        .iter()
        .filter(|field| field.is_sent)
        .map(|field| exchange_token(field, log, contact))
        .collect::<Result<Vec<_>, _>>()?;
    let received_fields = rules
        .exchange
        .iter()
        .filter(|field| !field.is_sent)
        .map(|field| exchange_token(field, log, contact))
        .collect::<Result<Vec<_>, _>>()?;

    let mut parts = vec![
        "QSO:".to_string(),
        frequency,
        mode,
        date,
        time,
        station_callsign,
    ];
    parts.extend(sent_fields);
    parts.push(their_callsign);
    parts.extend(received_fields);

    Ok(parts.join(" "))
}

fn exchange_token(field: &ExchangeField, log: &Log, contact: &Contact) -> Result<String, String> {
    if let Some(value) = token_string(contact.get(&field.adif)) {
        return Ok(value);
    }
    if let Some(source_param) = &field.source_param
        && let Some(value) = log
            .contest_params
            .as_object()
            .and_then(|params| params.get(source_param))
            .and_then(|value| token_string(Some(value)))
    {
        return Ok(value);
    }
    if let Some(value) = field
        .default
        .as_ref()
        .and_then(|value| token_string(Some(value)))
    {
        return Ok(value);
    }
    Err(format!("contact is missing {}", field.name))
}

fn qso_frequency_token(contact: &Contact) -> Option<String> {
    contact_i64(contact.get("FREQ")).map(|frequency_hz| (frequency_hz / 1000).to_string())
}

fn qso_mode_token(contact: &Contact) -> Option<String> {
    let mode = token_string(contact.get("MODE"))?;
    Some(cabrillo_mode_token(&mode).to_string())
}

fn cabrillo_mode_token(mode: &str) -> &'static str {
    match mode.trim().to_uppercase().as_str() {
        "CW" | "CW-R" => "CW",
        "SSB" | "USB" | "LSB" | "FM" | "FMN" | "WFM" | "AM" | "PH" => "PH",
        _ => "DG",
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
    Ok((
        format!("{year:04}-{month:02}-{day:02}"),
        format!("{hour:02}{minute:02}"),
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

fn parameter_value(values: &Map<String, Value>, field: &ContestParam) -> Option<String> {
    let value = values
        .get(&field.name)
        .and_then(|value| contact_string(Some(value)))
        .or_else(|| {
            field
                .default
                .as_ref()
                .and_then(|value| contact_string(Some(value)))
        })?;
    let value = if field.preserve_case == Some(true) {
        value
    } else {
        value.to_uppercase()
    };
    (!value.trim().is_empty()).then_some(value)
}

fn normalized_tag(tag: &str) -> String {
    tag.trim().to_uppercase()
}

fn contact_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(string) => Some(string.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn token_string(value: Option<&Value>) -> Option<String> {
    contact_string(value).map(|value| value.to_uppercase())
}

fn contact_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64(),
        Value::String(string) => string.trim().parse::<i64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest_rules::{CabrilloFixedField, CabrilloRules, ContestParam};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn test_rules() -> ContestRules {
        ContestRules {
            contest: "SC-QSO-PARTY".to_string(),
            display_name: "SC QSO Party".to_string(),
            allowed_bands: vec![20],
            allowed_modes: vec!["CW".to_string(), "SSB".to_string()],
            define: Vec::new(),
            exchange: vec![
                ExchangeField {
                    name: "RST(s)".to_string(),
                    field_type: "RST".to_string(),
                    adif: "RST_SENT".to_string(),
                    fixed: None,
                    default: Some(json!(599)),
                    source_param: None,
                    regex: None,
                    in_sets: Vec::new(),
                    valid_values: Vec::new(),
                    is_sent: true,
                },
                ExchangeField {
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
                ExchangeField {
                    name: "RST(r)".to_string(),
                    field_type: "RST".to_string(),
                    adif: "RST_RCVD".to_string(),
                    fixed: None,
                    default: None,
                    source_param: None,
                    regex: None,
                    in_sets: Vec::new(),
                    valid_values: Vec::new(),
                    is_sent: false,
                },
                ExchangeField {
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
            cabrillo: Some(CabrilloRules {
                fixed_fields: vec![CabrilloFixedField {
                    name: "CATEGORY-BAND".to_string(),
                    value: "ALL".to_string(),
                }],
                log_fields: vec![ContestParam {
                    name: "CATEGORY-MODE".to_string(),
                    label: "Category Mode".to_string(),
                    field_type: "String:8".to_string(),
                    required: None,
                    regex: None,
                    default: None,
                    in_sets: Vec::new(),
                    valid_values: vec!["CW".to_string(), "MIXED".to_string()],
                    widget: Some("select".to_string()),
                    help_text: None,
                    max_lines: None,
                    preserve_case: None,
                }],
                export_fields: vec![
                    ContestParam {
                        name: "NAME".to_string(),
                        label: "Name".to_string(),
                        field_type: "String:75".to_string(),
                        required: None,
                        regex: None,
                        default: None,
                        in_sets: Vec::new(),
                        valid_values: Vec::new(),
                        widget: None,
                        help_text: None,
                        max_lines: None,
                        preserve_case: Some(true),
                    },
                    ContestParam {
                        name: "ADDRESS".to_string(),
                        label: "Address".to_string(),
                        field_type: "String:45".to_string(),
                        required: None,
                        regex: None,
                        default: None,
                        in_sets: Vec::new(),
                        valid_values: Vec::new(),
                        widget: Some("textarea".to_string()),
                        help_text: None,
                        max_lines: Some(6),
                        preserve_case: Some(true),
                    },
                ],
            }),
            metadata: None,
        }
    }

    fn test_log() -> Log {
        Log {
            id: 1,
            name: "Test".to_string(),
            contest_id: "SC-QSO-PARTY".to_string(),
            station_callsign: "N0CALL".to_string(),
            contest_params: json!({
                "County": "ABBE",
                "CATEGORY-MODE": "MIXED"
            }),
        }
    }

    fn test_contact(operator: &str, call: &str, epoch: i64) -> Contact {
        Map::from_iter([
            ("QSO_DATE_TIME_ON".to_string(), json!(epoch)),
            ("STATION_CALLSIGN".to_string(), json!("N0CALL")),
            ("OPERATOR".to_string(), json!(operator)),
            ("CALL".to_string(), json!(call)),
            ("FREQ".to_string(), json!(14_250_000_i64)),
            ("MODE".to_string(), json!("SSB")),
            ("RST_SENT".to_string(), json!(59)),
            ("STX_STRING".to_string(), json!("ABBE")),
            ("RST_RCVD".to_string(), json!(59)),
            ("SRX_STRING".to_string(), json!("NC")),
        ])
    }

    #[test]
    fn render_log_emits_required_headers_and_qsos() {
        let text = render_log(
            &test_rules(),
            &test_log(),
            &[
                test_contact("K1ABC", "W1AW", 1_700_000_000),
                test_contact("K1ABC", "N5KO", 1_700_000_060),
            ],
            &json!({
                "NAME": "Greg",
                "ADDRESS": "123 Main St\nTown, SC"
            }),
            1234,
        )
        .expect("export should render");

        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines[0], "START-OF-LOG: 3.0");
        assert_eq!(lines[1], "CREATED-BY: Log73");
        assert_eq!(lines[2], "CALLSIGN: N0CALL");
        assert_eq!(lines[3], "CONTEST: SC-QSO-PARTY");
        assert!(lines.contains(&"CLAIMED-SCORE: 1234"));
        assert!(lines.contains(&"CATEGORY-BAND: ALL"));
        assert!(lines.contains(&"CATEGORY-MODE: MIXED"));
        assert!(lines.contains(&"NAME: Greg"));
        assert!(lines.contains(&"ADDRESS: 123 Main St"));
        assert!(lines.contains(&"ADDRESS: Town, SC"));
        assert!(lines.contains(&"OPERATORS: K1ABC"));
        assert!(lines.iter().any(|line| line.starts_with("QSO: 14250 PH ")));
        assert!(text.ends_with("\r\n"));
    }

    #[test]
    fn qso_mode_token_maps_logger_modes_to_cabrillo_groups() {
        for (mode, token) in [
            ("CW", "CW"),
            ("CW-R", "CW"),
            ("SSB", "PH"),
            ("FM", "PH"),
            ("AM", "PH"),
            ("RTTY", "DG"),
            ("FT8", "DG"),
            ("PSK", "DG"),
        ] {
            let contact = Map::from_iter([("MODE".to_string(), json!(mode))]);
            assert_eq!(qso_mode_token(&contact).as_deref(), Some(token));
        }
    }

    #[test]
    fn render_log_wraps_operators_across_multiple_lines() {
        let text = render_log(
            &test_rules(),
            &test_log(),
            &[
                test_contact("K1ABC", "W1AW", 1_700_000_000),
                test_contact("N5XYZ", "N5KO", 1_700_000_060),
                test_contact("W9QRS", "K3LR", 1_700_000_120),
            ],
            &json!({ "NAME": "Greg" }),
            10,
        )
        .expect("export should render");

        let operator_lines = text
            .lines()
            .filter(|line| line.starts_with("OPERATORS:"))
            .collect::<Vec<_>>();
        assert!(!operator_lines.is_empty());
        assert!(operator_lines.iter().all(|line| line.len() <= 86));
    }
}
