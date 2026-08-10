use crate::bands::{Band, band_by_name};
use crate::contest_rules::{ContestRules, ExchangeDirection, ExchangeField, SetupField};
use crate::db::{Contact, Log, contact_adif_value};
use crate::qso_time::qso_datetime_cabrillo;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

const START_OF_LOG_VERSION: &str = "3.0";
const CREATED_BY_VALUE: &str = "Log73";

pub fn export_filename(log: &Log) -> String {
    format!("{}.log", sanitized_export_stem(&log.station_callsign))
}

fn sanitized_export_stem(callsign: &str) -> String {
    let sanitized = callsign
        .trim()
        .to_uppercase()
        .chars()
        .map(|character| match character {
            'A'..='Z' | '0'..='9' | '_' | '-' => character,
            _ => '_',
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if sanitized.is_empty() {
        "LOG73".to_string()
    } else {
        sanitized
    }
}

pub fn render_log(
    rules: &ContestRules,
    log: &Log,
    contacts: &[Contact],
    export_params: &Value,
    claimed_score: i64,
    bands: &[Band],
) -> Result<String, String> {
    let cabrillo = rules
        .cabrillo
        .as_ref()
        .ok_or_else(|| format!("contest {} does not define Cabrillo export", rules.id))?;
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
    append_header_line(&mut lines, "CONTEST", cabrillo_contest_id(rules))?;
    append_header_line(&mut lines, "CLAIMED-SCORE", &claimed_score.to_string())?;

    for field in &cabrillo.fixed_headers {
        if is_reserved_tag(&field.name) {
            continue;
        }
        append_header_value(&mut lines, &field.name, &field.value, None)?;
    }

    for field in rules
        .setup_fields
        .iter()
        .filter(|field| field.cabrillo_header.is_some())
    {
        let header = field.cabrillo_header.as_deref().expect("filtered header");
        if is_reserved_tag(header) {
            continue;
        }
        if let Some(value) = parameter_value(log_values, field) {
            append_header_value(&mut lines, header, &value, field.max_lines)?;
        }
    }

    for field in &cabrillo.export_fields {
        if is_reserved_tag(&field.key) {
            continue;
        }
        if let Some(value) = parameter_value(export_values, field) {
            append_header_value(&mut lines, &field.key, &value, field.max_lines)?;
        }
    }

    append_operators_lines(&mut lines, contacts, &log.station_callsign)?;

    for contact in contacts {
        lines.push(render_qso_line(rules, log, contact, bands)?);
    }

    lines.push("END-OF-LOG:".to_string());
    Ok(lines.join("\r\n") + "\r\n")
}

fn cabrillo_contest_id(rules: &ContestRules) -> &str {
    rules.id.split_whitespace().next().unwrap_or(&rules.id)
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
        .filter_map(|contact| token_string(contact_adif_value(contact, "OPERATOR")))
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

fn render_qso_line(
    rules: &ContestRules,
    log: &Log,
    contact: &Contact,
    bands: &[Band],
) -> Result<String, String> {
    let frequency = qso_frequency_token(contact, bands)?;
    let mode = qso_mode_token(contact).ok_or_else(|| "contact is missing mode".to_string())?;
    let epoch = contact_i64(contact_adif_value(contact, "QSO_DATE_TIME_ON"))
        .ok_or_else(|| "contact is missing QSO date/time".to_string())?;
    let (date, time) = qso_datetime_cabrillo(epoch)?;
    let station_callsign = contact_string(contact_adif_value(contact, "STATION_CALLSIGN"))
        .map(|value| value.to_uppercase())
        .unwrap_or_else(|| log.station_callsign.trim().to_uppercase());
    let their_callsign = token_string(contact_adif_value(contact, "CALL"))
        .ok_or_else(|| "contact is missing callsign".to_string())?;

    let sent_fields = rules
        .exchange
        .iter()
        .filter(|field| {
            field.direction == ExchangeDirection::Sent
                && crate::validation::exchange_field_applies(field, contact)
        })
        .map(|field| exchange_token(field, log, contact))
        .collect::<Result<Vec<_>, _>>()?;
    let received_fields = rules
        .exchange
        .iter()
        .filter(|field| {
            field.direction == ExchangeDirection::Received
                && crate::validation::exchange_field_applies(field, contact)
        })
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
    if uses_transmitter_id(rules, log) {
        parts.push(
            contact_i64(contact_adif_value(contact, "APP_LOG73_TX_ID"))
                .unwrap_or(0)
                .to_string(),
        );
    }

    Ok(parts.join(" "))
}

fn uses_transmitter_id(rules: &ContestRules, log: &Log) -> bool {
    if rules.cabrillo.is_none() {
        return false;
    }
    let Some(log_values) = log.contest_params.as_object() else {
        return false;
    };
    let operator = cabrillo_header_value(rules, log_values, "CATEGORY-OPERATOR");
    if operator.as_deref() != Some("MULTI-OP") {
        return false;
    }

    let transmitter = cabrillo_header_value(rules, log_values, "CATEGORY-TRANSMITTER");
    match transmitter.as_deref() {
        Some("TWO") => true,
        Some("ONE") => rules
            .setup_fields
            .iter()
            .find(|field| {
                field
                    .cabrillo_header
                    .as_deref()
                    .is_some_and(|header| normalized_tag(header) == "CATEGORY-TRANSMITTER")
            })
            .is_some_and(|field| field.multi_single_has_mult_transmitter),
        _ => false,
    }
}

fn cabrillo_header_value(
    rules: &ContestRules,
    log_values: &Map<String, Value>,
    name: &str,
) -> Option<String> {
    let cabrillo = rules.cabrillo.as_ref()?;
    cabrillo
        .fixed_headers
        .iter()
        .find(|field| normalized_tag(&field.name) == name)
        .map(|field| field.value.trim().to_uppercase())
        .or_else(|| {
            rules
                .setup_fields
                .iter()
                .find(|field| {
                    field
                        .cabrillo_header
                        .as_deref()
                        .is_some_and(|header| normalized_tag(header) == name)
                })
                .and_then(|field| parameter_value(log_values, field))
                .map(|value| value.trim().to_uppercase())
        })
}

fn exchange_token(field: &ExchangeField, log: &Log, contact: &Contact) -> Result<String, String> {
    if let Some(value) = token_string(contact_adif_value(contact, &field.adif)) {
        return Ok(value);
    }
    if let Some(source_param) = &field.source
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
    Err(format!("contact is missing {}", field.label))
}

fn qso_frequency_token(contact: &Contact, bands: &[Band]) -> Result<String, String> {
    let frequency_hz = contact_i64(contact_adif_value(contact, "FREQ"))
        .filter(|frequency_hz| *frequency_hz > 0)
        .ok_or_else(|| "contact is missing frequency for Cabrillo export".to_string())?;
    let band_name = token_string(contact_adif_value(contact, "BAND"))
        .ok_or_else(|| "contact is missing band for Cabrillo export".to_string())?;
    let band = band_by_name(bands, &band_name).ok_or_else(|| {
        format!("contact band {band_name} is not available in the configured IARU region")
    })?;
    if band.cabrillo == "khz" {
        Ok((frequency_hz / 1_000).to_string())
    } else {
        Ok(band.cabrillo.clone())
    }
}

fn qso_mode_token(contact: &Contact) -> Option<String> {
    let mode = token_string(contact_adif_value(contact, "MODE"))?;
    Some(cabrillo_mode_token(&mode).to_string())
}

fn cabrillo_mode_token(mode: &str) -> &'static str {
    match mode.trim().to_uppercase().as_str() {
        "CW" | "CW-R" => "CW",
        "SSB" | "USB" | "LSB" | "FM" | "FMN" | "WFM" | "AM" | "PH" => "PH",
        "RTTY" => "RY",
        _ => "DG",
    }
}

fn parameter_value(values: &Map<String, Value>, field: &SetupField) -> Option<String> {
    let value = values
        .get(&field.key)
        .and_then(|value| contact_string(Some(value)))
        .or_else(|| {
            field
                .default
                .as_ref()
                .and_then(|value| contact_string(Some(value)))
        })?;
    let value = if field.preserve_case {
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
    use crate::contest_rules::{
        CabrilloFixedHeader, CabrilloRules, ContestRulesStore, ExchangeDirection, SetupField,
        test_exchange_field, test_setup_field,
    };
    use serde_json::json;
    use std::path::PathBuf;

    fn test_bands() -> Vec<Band> {
        [
            ("20M", 14_000_000, 14_350_000, "khz"),
            ("6M", 50_000_000, 54_000_000, "50"),
            ("23cm", 1_240_000_000, 1_300_000_000, "1.2G"),
            ("10Ghz", 10_000_000_000, 10_500_000_000, "10G"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (name, lower_hz, upper_hz, cabrillo))| Band {
            iaru_region: 2,
            name: name.to_string(),
            lower_hz,
            upper_hz,
            default_ssb_mode: "USB".to_string(),
            sort_order: i64::try_from(index + 1).unwrap(),
            cabrillo: cabrillo.to_string(),
        })
        .collect()
    }

    fn render_log(
        rules: &ContestRules,
        log: &Log,
        contacts: &[Contact],
        export_params: &Value,
        claimed_score: i64,
    ) -> Result<String, String> {
        super::render_log(
            rules,
            log,
            contacts,
            export_params,
            claimed_score,
            &test_bands(),
        )
    }

    fn test_rules() -> ContestRules {
        let mut county = test_exchange_field(
            "county-sent",
            "County",
            "String:4",
            "STX_STRING",
            ExchangeDirection::Sent,
        );
        county.fixed = true;
        county.source = Some("County".to_string());
        let mut category_mode = test_setup_field(
            "category-mode",
            "CATEGORY-MODE",
            "Category Mode",
            "String:8",
        );
        category_mode.validation.values = vec!["CW".to_string(), "MIXED".to_string()];
        category_mode.widget = Some("select".to_string());
        category_mode.cabrillo_header = Some("CATEGORY-MODE".to_string());

        let mut name = test_setup_field("name", "NAME", "Name", "String:75");
        name.preserve_case = true;
        let mut address = test_setup_field("address", "ADDRESS", "Address", "String:45");
        address.widget = Some("textarea".to_string());
        address.max_lines = Some(6);
        address.preserve_case = true;

        ContestRules {
            id: "SC-QSO-PARTY".to_string(),
            name: "SC QSO Party".to_string(),
            bands: vec!["20m".to_string()],
            modes: vec!["CW".to_string(), "SSB".to_string()],
            setup_fields: vec![category_mode],
            exchange: vec![
                {
                    let mut field = test_exchange_field(
                        "rst-sent",
                        "RST(s)",
                        "RST",
                        "RST_SENT",
                        ExchangeDirection::Sent,
                    );
                    field.default = Some(json!(599));
                    field
                },
                county,
                test_exchange_field(
                    "rst-received",
                    "RST(r)",
                    "RST",
                    "RST_RCVD",
                    ExchangeDirection::Received,
                ),
                test_exchange_field(
                    "exchange-received",
                    "Exchange",
                    "String:4",
                    "SRX_STRING",
                    ExchangeDirection::Received,
                ),
            ],
            cabrillo: Some(CabrilloRules {
                fixed_headers: vec![CabrilloFixedHeader {
                    id: "category-band".to_string(),
                    name: "CATEGORY-BAND".to_string(),
                    value: "ALL".to_string(),
                }],
                export_fields: vec![name, address],
            }),
            ..ContestRules::default()
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

    #[test]
    fn export_filename_sanitizes_station_callsign() {
        let mut log = test_log();
        log.station_callsign = "N0/CALL:\"bad\"".to_string();

        assert_eq!(export_filename(&log), "N0_CALL__BAD.log");
    }

    fn test_contact(operator: &str, call: &str, epoch: i64) -> Contact {
        crate::db::build_contact(
            Map::new(),
            Map::from_iter([
                ("QSO_DATE_TIME_ON".to_string(), json!(epoch)),
                ("STATION_CALLSIGN".to_string(), json!("N0CALL")),
                ("OPERATOR".to_string(), json!(operator)),
                ("CALL".to_string(), json!(call)),
                ("BAND".to_string(), json!("20m")),
                ("FREQ".to_string(), json!(14_250_000_i64)),
                ("MODE".to_string(), json!("SSB")),
                ("RST_SENT".to_string(), json!(59)),
                ("STX_STRING".to_string(), json!("ABBE")),
                ("RST_RCVD".to_string(), json!(59)),
                ("SRX_STRING".to_string(), json!("NC")),
            ]),
        )
    }

    fn category_param(name: &str, multi_single_has_mult_transmitter: bool) -> SetupField {
        let mut field = test_setup_field(&name.to_ascii_lowercase(), name, name, "String:16");
        field.widget = Some("select".to_string());
        field.cabrillo_header = Some(name.to_string());
        field.multi_single_has_mult_transmitter = multi_single_has_mult_transmitter;
        field
    }

    fn categorized_rules(multi_single_has_mult_transmitter: bool) -> ContestRules {
        let mut rules = test_rules();
        rules
            .setup_fields
            .push(category_param("CATEGORY-OPERATOR", false));
        rules.setup_fields.push(category_param(
            "CATEGORY-TRANSMITTER",
            multi_single_has_mult_transmitter,
        ));
        rules
    }

    fn categorized_log(operator: &str, transmitter: &str) -> Log {
        let mut log = test_log();
        log.contest_params["CATEGORY-OPERATOR"] = json!(operator);
        log.contest_params["CATEGORY-TRANSMITTER"] = json!(transmitter);
        log
    }

    fn qso_line(text: &str) -> &str {
        text.lines()
            .find(|line| line.starts_with("QSO:"))
            .expect("rendered log should contain a QSO")
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
    fn render_log_uses_the_first_word_of_the_rule_id_as_contest_id() {
        let mut rules = test_rules();
        rules.id = "SC-QSO-PARTY (In-state)".to_string();
        let mut log = test_log();
        log.contest_id = "IGNORED-LOG-ID".to_string();

        let text = render_log(
            &rules,
            &log,
            &[test_contact("K1ABC", "W1AW", 1_700_000_000)],
            &json!({
                "NAME": "Greg",
                "ADDRESS": "123 Main St"
            }),
            1,
        )
        .expect("export should render");

        assert!(text.lines().any(|line| line == "CONTEST: SC-QSO-PARTY"));
    }

    #[test]
    fn ohio_qso_party_exports_the_required_ten_qso_fields() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store
            .get("OH-QSO-PARTY")
            .expect("Ohio QSO Party rules should load");
        let mut log = test_log();
        log.contest_id = "OH-QSO-PARTY".to_string();
        log.contest_params = json!({ "Location": "PA" });
        let mut contact = test_contact("K1ABC", "K8MAD", 1_700_000_000);
        crate::db::set_contact_adif(&mut contact, "STX_STRING", json!("PA"));
        crate::db::set_contact_adif(&mut contact, "SRX_STRING", json!("ADAM"));

        let text = render_log(rules, &log, &[contact], &json!({}), 1)
            .expect("Ohio QSO Party Cabrillo should render");
        assert!(text.lines().any(|line| line == "CONTEST: OH-QSO-PARTY"));
        let fields = qso_line(&text).split_whitespace().collect::<Vec<_>>();
        assert_eq!(fields.len(), 11);
        assert_eq!(fields[0], "QSO:");
        assert_eq!(fields[2], "PH");
        assert_eq!(fields[8], "K8MAD");
        assert_eq!(fields[10], "ADAM");
    }

    #[test]
    fn arrl_sweepstakes_exports_serial_precedence_check_and_section() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store
            .get("ARRL-SS-CW")
            .expect("ARRL Sweepstakes CW rules should load");
        let mut log = test_log();
        log.contest_id = "ARRL-SS-CW".to_string();
        log.contest_params = json!({
            "Precedence": "A",
            "Check": "79",
            "Section": "SC"
        });
        let mut contact = test_contact("K1ABC", "W1AW", 1_700_000_000);
        crate::db::set_contact_adif(&mut contact, "STX", json!(123));
        crate::db::set_contact_adif(&mut contact, "STX_STRING", json!("A"));
        crate::db::set_contact_adif(&mut contact, "MY_CHECK", json!("79"));
        crate::db::set_contact_adif(&mut contact, "MY_ARRL_SECT", json!("SC"));
        crate::db::set_contact_adif(&mut contact, "SRX", json!(43));
        crate::db::set_contact_adif(&mut contact, "SRX_STRING", json!("M"));
        crate::db::set_contact_adif(&mut contact, "CHECK", json!("31"));
        crate::db::set_contact_adif(&mut contact, "ARRL_SECT", json!("CT"));

        let text = render_log(rules, &log, &[contact], &json!({}), 2)
            .expect("ARRL Sweepstakes Cabrillo should render");
        assert!(text.lines().any(|line| line == "CONTEST: ARRL-SS-CW"));
        assert!(text.lines().any(|line| line == "CATEGORY-MODE: CW"));
        let fields = qso_line(&text).split_whitespace().collect::<Vec<_>>();
        assert_eq!(fields.len(), 15);
        assert_eq!(
            &fields[6..],
            ["123", "A", "79", "SC", "W1AW", "43", "M", "31", "CT"]
        );
    }

    #[test]
    fn cqwpx_exports_rst_serial_exchange_and_multi_two_transmitter() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store.get("CQ-WPX-CW").expect("CQ WPX CW rules should load");
        let mut log = test_log();
        log.contest_id = "CQ-WPX-CW".to_string();
        log.contest_params = json!({
            "CATEGORY-OPERATOR": "MULTI-OP",
            "CATEGORY-ASSISTED": "ASSISTED",
            "CATEGORY-BAND": "ALL",
            "CATEGORY-POWER": "HIGH",
            "CATEGORY-STATION": "FIXED",
            "CATEGORY-TRANSMITTER": "TWO"
        });
        let mut contact = test_contact("K1ABC", "OL25LP", 1_700_000_000);
        crate::db::set_contact_adif(&mut contact, "MODE", json!("CW"));
        crate::db::set_contact_adif(&mut contact, "RST_SENT", json!(599));
        crate::db::set_contact_adif(&mut contact, "STX", json!(123));
        crate::db::set_contact_adif(&mut contact, "RST_RCVD", json!(599));
        crate::db::set_contact_adif(&mut contact, "SRX", json!(456));
        crate::db::set_contact_adif(&mut contact, "APP_LOG73_TX_ID", json!(1));

        let text = render_log(rules, &log, &[contact], &json!({}), 42)
            .expect("CQ WPX Cabrillo should render");

        assert!(text.lines().any(|line| line == "CONTEST: CQ-WPX-CW"));
        assert!(text.lines().any(|line| line == "CATEGORY-MODE: CW"));
        assert!(text.lines().any(|line| line == "CATEGORY-TRANSMITTER: TWO"));
        let fields = qso_line(&text).split_whitespace().collect::<Vec<_>>();
        assert_eq!(&fields[6..], ["599", "123", "OL25LP", "599", "456", "1"]);
    }

    #[test]
    fn na_sprint_ssb_exports_required_headers_and_exchange_order() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store
            .get("NA-SPRINT-SSB (North America)")
            .expect("North America NA Sprint rules should load");
        let mut log = test_log();
        log.contest_id = "NA-SPRINT-SSB (North America)".to_string();
        log.contest_params = json!({
            "QTH": "MA",
            "NAME": "Scott",
            "LOCATION": "MA",
            "CATEGORY-OPERATOR": "SINGLE-OP",
            "CATEGORY-ASSISTED": "NON-ASSISTED",
            "CATEGORY-POWER": "LOW"
        });
        let mut contact = test_contact("K1ABC", "VE3ABC", 1_700_000_000);
        crate::db::set_contact_adif(&mut contact, "STX", json!(1));
        crate::db::set_contact_adif(&mut contact, "MY_NAME", json!("SCOTT"));
        crate::db::set_contact_adif(&mut contact, "STX_STRING", json!("MA"));
        crate::db::set_contact_adif(&mut contact, "SRX", json!(2));
        crate::db::set_contact_adif(&mut contact, "NAME", json!("TIM"));
        crate::db::set_contact_adif(&mut contact, "SRX_STRING", json!("ON"));

        let text = render_log(rules, &log, &[contact], &json!({}), 2)
            .expect("NA Sprint SSB Cabrillo should render");

        assert!(text.lines().any(|line| line == "CONTEST: NA-SPRINT-SSB"));
        assert!(text.lines().any(|line| line == "CATEGORY-BAND: ALL"));
        assert!(text.lines().any(|line| line == "CATEGORY-MODE: SSB"));
        assert!(text.lines().any(|line| line == "CATEGORY-TRANSMITTER: ONE"));
        let fields = qso_line(&text).split_whitespace().collect::<Vec<_>>();
        assert_eq!(
            &fields[6..],
            ["1", "SCOTT", "MA", "VE3ABC", "2", "TIM", "ON"]
        );
    }

    #[test]
    fn na_sprint_cw_exports_required_headers_and_exchange_order() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store
            .get("NA-SPRINT-CW (North America)")
            .expect("North America NA Sprint CW rules should load");
        let mut log = test_log();
        log.contest_id = "NA-SPRINT-CW (North America)".to_string();
        log.contest_params = json!({
            "QTH": "MA",
            "NAME": "Scott",
            "LOCATION": "MA",
            "CATEGORY-OPERATOR": "SINGLE-OP",
            "CATEGORY-POWER": "LOW"
        });
        let mut contact = test_contact("K1ABC", "VE3ABC", 1_700_000_000);
        crate::db::set_contact_adif(&mut contact, "STX", json!(1));
        crate::db::set_contact_adif(&mut contact, "MY_NAME", json!("SCOTT"));
        crate::db::set_contact_adif(&mut contact, "STX_STRING", json!("MA"));
        crate::db::set_contact_adif(&mut contact, "SRX", json!(2));
        crate::db::set_contact_adif(&mut contact, "NAME", json!("TIM"));
        crate::db::set_contact_adif(&mut contact, "SRX_STRING", json!("ON"));

        let text = render_log(rules, &log, &[contact], &json!({}), 2)
            .expect("NA Sprint CW Cabrillo should render");

        assert!(text.lines().any(|line| line == "CONTEST: NA-SPRINT-CW"));
        assert!(text.lines().any(|line| line == "CATEGORY-BAND: ALL"));
        assert!(text.lines().any(|line| line == "CATEGORY-MODE: CW"));
        assert!(
            text.lines()
                .any(|line| line == "CATEGORY-ASSISTED: NON-ASSISTED")
        );
        let fields = qso_line(&text).split_whitespace().collect::<Vec<_>>();
        assert_eq!(
            &fields[6..],
            ["1", "SCOTT", "MA", "VE3ABC", "2", "TIM", "ON"]
        );
    }

    #[test]
    fn naqp_cw_exports_multi_two_headers_and_exchange_order() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store
            .get("NAQP-CW (North America)")
            .expect("North America NAQP CW rules should load");
        let mut log = test_log();
        log.contest_id = "NAQP-CW (North America)".to_string();
        log.contest_params = json!({
            "QTH": "MA",
            "NAME": "Scott",
            "LOCATION": "MA",
            "CATEGORY-OPERATOR": "MULTI-OP",
            "CATEGORY-ASSISTED": "ASSISTED",
            "CATEGORY-POWER": "LOW",
            "CATEGORY-TRANSMITTER": "TWO"
        });
        let mut contact = test_contact("K1ABC", "VE3ABC", 1_700_000_000);
        crate::db::set_contact_adif(&mut contact, "MY_NAME", json!("SCOTT"));
        crate::db::set_contact_adif(&mut contact, "STX_STRING", json!("MA"));
        crate::db::set_contact_adif(&mut contact, "NAME", json!("TIM"));
        crate::db::set_contact_adif(&mut contact, "SRX_STRING", json!("ON"));
        crate::db::set_contact_adif(&mut contact, "APP_LOG73_TX_ID", json!(1));

        let text = render_log(rules, &log, &[contact], &json!({}), 2)
            .expect("NAQP CW Cabrillo should render");

        assert!(text.lines().any(|line| line == "CONTEST: NAQP-CW"));
        assert!(text.lines().any(|line| line == "CATEGORY-BAND: ALL"));
        assert!(text.lines().any(|line| line == "CATEGORY-MODE: CW"));
        assert!(text.lines().any(|line| line == "CATEGORY-TRANSMITTER: TWO"));
        let fields = qso_line(&text).split_whitespace().collect::<Vec<_>>();
        assert_eq!(&fields[6..], ["SCOTT", "MA", "VE3ABC", "TIM", "ON", "1"]);
    }

    #[test]
    fn multi_two_qso_uses_stored_transmitter_id() {
        let mut contact = test_contact("K1ABC", "W1AW", 1_700_000_000);
        crate::db::set_contact_adif(&mut contact, "APP_LOG73_TX_ID", json!("1"));

        let text = render_log(
            &categorized_rules(false),
            &categorized_log("MULTI-OP", "TWO"),
            &[contact],
            &json!({}),
            1,
        )
        .expect("export should render");

        assert!(qso_line(&text).ends_with(" NC 1"));
    }

    #[test]
    fn flagged_multi_single_qso_defaults_missing_transmitter_id_to_zero() {
        let text = render_log(
            &categorized_rules(true),
            &categorized_log("MULTI-OP", "ONE"),
            &[test_contact("K1ABC", "W1AW", 1_700_000_000)],
            &json!({}),
            1,
        )
        .expect("export should render");

        assert!(qso_line(&text).ends_with(" NC 0"));
    }

    #[test]
    fn ineligible_categories_omit_transmitter_id() {
        for (operator, transmitter, flagged) in [
            ("MULTI-OP", "ONE", false),
            ("SINGLE-OP", "TWO", true),
            ("MULTI-OP", "UNLIMITED", true),
        ] {
            let mut contact = test_contact("K1ABC", "W1AW", 1_700_000_000);
            crate::db::set_contact_adif(&mut contact, "APP_LOG73_TX_ID", json!(1));
            let text = render_log(
                &categorized_rules(flagged),
                &categorized_log(operator, transmitter),
                &[contact],
                &json!({}),
                1,
            )
            .expect("export should render");

            assert!(
                qso_line(&text).ends_with(" NC"),
                "{operator}/{transmitter} unexpectedly emitted a transmitter ID"
            );
        }
    }

    #[test]
    fn qso_mode_token_maps_logger_modes_to_cabrillo_groups() {
        for (mode, token) in [
            ("CW", "CW"),
            ("CW-R", "CW"),
            ("SSB", "PH"),
            ("FM", "PH"),
            ("AM", "PH"),
            ("RTTY", "RY"),
            ("PKT", "DG"),
            ("FT8", "DG"),
            ("PSK", "DG"),
        ] {
            let contact = crate::db::build_contact(
                Map::new(),
                Map::from_iter([("MODE".to_string(), json!(mode))]),
            );
            assert_eq!(qso_mode_token(&contact).as_deref(), Some(token));
        }
    }

    #[test]
    fn cabrillo_khz_bands_use_the_contacts_actual_frequency() {
        let mut contact = test_contact("K1ABC", "W1AW", 1_700_000_000);
        crate::db::set_contact_adif(&mut contact, "FREQ", json!(14_123_456));

        assert_eq!(
            qso_frequency_token(&contact, &test_bands()),
            Ok("14123".to_string())
        );
    }

    #[test]
    fn cabrillo_literal_is_used_for_every_frequency_on_the_band() {
        for frequency_hz in [50_000_000, 50_313_000, 53_999_999] {
            let mut contact = test_contact("K1ABC", "W1AW", 1_700_000_000);
            crate::db::set_contact_adif(&mut contact, "BAND", json!("6m"));
            crate::db::set_contact_adif(&mut contact, "FREQ", json!(frequency_hz));
            assert_eq!(
                qso_frequency_token(&contact, &test_bands()),
                Ok("50".to_string())
            );
        }
    }

    #[test]
    fn rendered_qso_line_uses_the_band_cabrillo_literal() {
        let mut contact = test_contact("K1ABC", "W1AW", 1_700_000_000);
        crate::db::set_contact_adif(&mut contact, "BAND", json!("6M"));
        crate::db::set_contact_adif(&mut contact, "FREQ", json!(50_313_000));

        let text = render_log(&test_rules(), &test_log(), &[contact], &json!({}), 1)
            .expect("Cabrillo log renders");
        assert!(qso_line(&text).starts_with("QSO: 50 PH "));
    }

    #[test]
    fn cabrillo_23cm_and_microwave_literals_are_preserved() {
        for (band, frequency_hz, expected) in [
            ("23CM", 1_296_000_000_i64, "1.2G"),
            ("10ghz", 10_368_000_000_i64, "10G"),
        ] {
            let mut contact = test_contact("K1ABC", "W1AW", 1_700_000_000);
            crate::db::set_contact_adif(&mut contact, "BAND", json!(band));
            crate::db::set_contact_adif(&mut contact, "FREQ", json!(frequency_hz));
            assert_eq!(
                qso_frequency_token(&contact, &test_bands()),
                Ok(expected.to_string())
            );
        }
    }

    #[test]
    fn cabrillo_frequency_rejects_missing_frequency_band_and_unknown_band() {
        let bands = test_bands();
        let mut contact = test_contact("K1ABC", "W1AW", 1_700_000_000);
        crate::db::set_contact_adif(&mut contact, "FREQ", Value::Null);
        assert!(
            qso_frequency_token(&contact, &bands)
                .unwrap_err()
                .contains("frequency")
        );

        let mut contact = test_contact("K1ABC", "W1AW", 1_700_000_000);
        crate::db::set_contact_adif(&mut contact, "BAND", Value::Null);
        assert!(
            qso_frequency_token(&contact, &bands)
                .unwrap_err()
                .contains("band")
        );

        let mut contact = test_contact("K1ABC", "W1AW", 1_700_000_000);
        crate::db::set_contact_adif(&mut contact, "BAND", json!("4M"));
        let error = qso_frequency_token(&contact, &bands).unwrap_err();
        assert!(error.contains("4M"));
        assert!(error.contains("configured IARU region"));
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
