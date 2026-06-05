use crate::bands::{USA_AMATEUR_BANDS, band_for_frequency};
use crate::contest_rules::{ContestParam, ContestRules, ContestRulesStore, ExchangeField};
use crate::cw;
use crate::db::{self, Contact, Database, NewLog, NewRadio, UpdateLog};
use radio_cat_rs::{Frequency, RadioKind};
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;

const MAX_LOG_NAME_LEN: usize = 100;
const MAX_CONTEST_ID_LEN: usize = 100;
const MAX_CALLSIGN_LEN: usize = 12;
const MAX_RADIO_NAME_LEN: usize = 100;
const MAX_RADIO_HOST_LEN: usize = 255;
const MAX_SERIAL_PORT_LEN: usize = 255;
const MIN_RADIO_SECONDS: f64 = 0.01;
const MAX_RADIO_SECONDS: f64 = 3600.0;
const MAX_LOGIN_USER_LEN: usize = 64;
const MAX_LOGIN_PASSWORD_LEN: usize = 256;
const MAX_DXCLUSTER_HOST_LEN: usize = 255;
const MAX_DXCLUSTER_CALLSIGN_LEN: usize = 32;
const MAX_DXCLUSTER_COMMANDS_LEN: usize = 16_384;
const MAX_DXCLUSTER_SPOT_COMMENT_LEN: usize = 256;
const MAX_CONTACTS_PER_UPLOAD: usize = 100;
const MAX_CONTACT_FIELDS: usize = 100;
const MAX_CONTACT_KEY_LEN: usize = 64;
const MAX_CONTACT_STRING_LEN: usize = 1024;
const MAX_CONTACT_ARRAY_ITEMS: usize = 100;
const MAX_CONTACT_OBJECT_FIELDS: usize = 100;
const MAX_CONTACT_JSON_DEPTH: usize = 4;
const MIN_QSO_EPOCH: i64 = 0;
const MAX_QSO_EPOCH: i64 = 4_102_444_800; // 2100-01-01T00:00:00Z
const MAX_RADIO_FREQUENCY_HZ: u64 = 500_000_000;
const MIN_CW_WPM: u8 = 5;
const MAX_CW_WPM: u8 = 60;
const MAX_CW_REQUEST_ID_LEN: usize = 64;
const MAX_CW_TEXT_LEN: usize = 256;
const MAX_CW_MESSAGES_LEN: usize = 16_384;
const MAX_WS_FIELDS: usize = 100;
const ALLOWED_CW_KEYER_TYPES: &[&str] = &["none", "winkeyer", "cat", "serial"];
const LOGGER_MODE_OPTIONS: &[&str] = &[
    "CW", "CW-R", "SSB", "FM", "FT8", "JT65", "JT9", "MFSK", "PSK", "RTTY",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedFieldType {
    kind: String,
    max_length: usize,
}

pub fn validate_new_log(contest_rules: &ContestRulesStore, payload: &NewLog) -> Result<(), String> {
    validate_required_text("log name", &payload.name, MAX_LOG_NAME_LEN)?;
    validate_required_text(
        "station callsign",
        &payload.station_callsign,
        MAX_CALLSIGN_LEN,
    )?;

    let contest_id = payload.contest_id.trim();
    validate_required_text("contest", contest_id, MAX_CONTEST_ID_LEN)?;
    let rules = contest_rules
        .get(contest_id)
        .ok_or_else(|| format!("unknown contest: {contest_id}"))?;
    validate_persisted_log_params(rules, &payload.contest_params)
}

pub fn validate_update_log(rules: &ContestRules, payload: &UpdateLog) -> Result<(), String> {
    validate_required_text("log name", &payload.name, MAX_LOG_NAME_LEN)?;
    validate_required_text(
        "station callsign",
        &payload.station_callsign,
        MAX_CALLSIGN_LEN,
    )?;
    validate_persisted_log_params(rules, &payload.contest_params)
}

pub fn validate_cabrillo_export_params(
    rules: &ContestRules,
    export_params: &Value,
) -> Result<(), String> {
    validate_configured_params(cabrillo_export_fields(rules), export_params)
}

pub fn validate_radio(payload: &NewRadio) -> Result<(), String> {
    validate_required_text("radio name", &payload.name, MAX_RADIO_NAME_LEN)?;

    payload
        .radio_kind
        .trim()
        .parse::<RadioKind>()
        .map_err(|error| error.to_string())?;

    let transport_kind = payload.transport_kind.trim().to_ascii_lowercase();
    if !matches!(transport_kind.as_str(), "tcp" | "serial") {
        return Err("transport kind must be tcp or serial".to_string());
    }

    validate_seconds("poll frequency", payload.poll_frequency)?;
    validate_seconds("CAT timeout", payload.cat_timeout)?;
    let cw_keyer_type = payload.cw_keyer_type.trim().to_ascii_lowercase();
    if !ALLOWED_CW_KEYER_TYPES.contains(&cw_keyer_type.as_str()) {
        return Err("CW keyer type must be one of: none, winkeyer, cat, serial".to_string());
    }

    match transport_kind.as_str() {
        "tcp" => {
            validate_required_text("TCP host", &payload.tcp_host, MAX_RADIO_HOST_LEN)?;
            validate_host("TCP host", &payload.tcp_host)?;
            if payload.tcp_port == 0 {
                return Err("TCP port must be between 1 and 65535".to_string());
            }
            if payload.serial_port.chars().count() > MAX_SERIAL_PORT_LEN {
                return Err(format!(
                    "serial port must be at most {MAX_SERIAL_PORT_LEN} characters"
                ));
            }
        }
        "serial" => {
            validate_required_text("serial port", &payload.serial_port, MAX_SERIAL_PORT_LEN)?;
            validate_serial_port("serial port", &payload.serial_port)?;
            if payload.serial_baud_rate == 0 {
                return Err("serial baud rate must be greater than 0".to_string());
            }
            if payload.tcp_host.chars().count() > MAX_RADIO_HOST_LEN {
                return Err(format!(
                    "TCP host must be at most {MAX_RADIO_HOST_LEN} characters"
                ));
            }
        }
        _ => unreachable!(),
    }

    if cw_keyer_type == "winkeyer" {
        validate_required_text(
            "Winkeyer serial port",
            &payload.winkeyer_serial_port,
            MAX_SERIAL_PORT_LEN,
        )?;
    } else if payload.winkeyer_serial_port.chars().count() > MAX_SERIAL_PORT_LEN {
        return Err(format!(
            "Winkeyer serial port must be at most {MAX_SERIAL_PORT_LEN} characters"
        ));
    }
    validate_serial_port("Winkeyer serial port", &payload.winkeyer_serial_port)?;

    if cw_keyer_type == "serial" {
        validate_required_text(
            "CW serial port",
            &payload.cw_serial_port,
            MAX_SERIAL_PORT_LEN,
        )?;
    } else if payload.cw_serial_port.chars().count() > MAX_SERIAL_PORT_LEN {
        return Err(format!(
            "CW serial port must be at most {MAX_SERIAL_PORT_LEN} characters"
        ));
    }
    validate_serial_port("CW serial port", &payload.cw_serial_port)?;

    if payload.cw_serial_baud_rate == 0 {
        return Err("CW serial baud rate must be greater than 0".to_string());
    }

    let cw_serial_line = payload.cw_serial_line.trim().to_ascii_lowercase();
    if !matches!(cw_serial_line.as_str(), "dtr" | "rts") {
        return Err("CW serial line must be dtr or rts".to_string());
    }

    if transport_kind == "serial"
        && cw_keyer_type == "serial"
        && payload.serial_port.trim() == payload.cw_serial_port.trim()
        && payload.serial_baud_rate != payload.cw_serial_baud_rate
    {
        return Err(
            "CAT serial baud rate and CW serial baud rate must match when sharing a serial port"
                .to_string(),
        );
    }

    validate_cw_messages(&payload.cw_messages)?;

    Ok(())
}

pub fn validate_cw_messages(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("CW messages are required".to_string());
    }
    if value.chars().count() > MAX_CW_MESSAGES_LEN {
        return Err(format!(
            "CW messages must be at most {MAX_CW_MESSAGES_LEN} characters"
        ));
    }
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err("CW messages cannot contain control characters".to_string());
    }

    cw::validate(value).map(|_| ())
}

pub fn validate_auth_config(
    login_user: &str,
    login_password: &str,
    login_password_confirm: &str,
) -> Result<(), String> {
    if login_password != login_password_confirm {
        return Err("passwords do not match".to_string());
    }

    if login_user.chars().count() > MAX_LOGIN_USER_LEN {
        return Err(format!(
            "username must be at most {MAX_LOGIN_USER_LEN} characters"
        ));
    }
    if login_password.chars().count() > MAX_LOGIN_PASSWORD_LEN {
        return Err(format!(
            "password must be at most {MAX_LOGIN_PASSWORD_LEN} characters"
        ));
    }

    let trimmed_user = login_user.trim();
    if !trimmed_user.is_empty() {
        if trimmed_user.contains(':') {
            return Err("username cannot contain ':'".to_string());
        }
        if trimmed_user.chars().any(char::is_control) {
            return Err("username cannot contain control characters".to_string());
        }
    }

    if login_password.chars().any(char::is_control) {
        return Err("password cannot contain control characters".to_string());
    }

    Ok(())
}

pub fn validate_dxcluster_config(
    host: &str,
    _port: u16,
    callsign: &str,
    max_age_min: u16,
    commands: &str,
) -> Result<(), String> {
    validate_optional_plain_text("DX cluster host", host, MAX_DXCLUSTER_HOST_LEN)?;
    validate_host("DX cluster host", host)?;
    validate_optional_plain_text("DX cluster callsign", callsign, MAX_DXCLUSTER_CALLSIGN_LEN)?;
    if callsign.trim().chars().any(char::is_whitespace) {
        return Err("DX cluster callsign cannot contain whitespace".to_string());
    }
    if !(db::MIN_DXCLUSTER_MAX_AGE_MIN..=db::MAX_DXCLUSTER_MAX_AGE_MIN).contains(&max_age_min) {
        return Err(format!(
            "DX cluster max age must be between {} and {} minutes",
            db::MIN_DXCLUSTER_MAX_AGE_MIN,
            db::MAX_DXCLUSTER_MAX_AGE_MIN
        ));
    }
    if commands.chars().count() > MAX_DXCLUSTER_COMMANDS_LEN {
        return Err(format!(
            "DX cluster commands must be at most {MAX_DXCLUSTER_COMMANDS_LEN} characters"
        ));
    }
    if commands
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err("DX cluster commands cannot contain control characters".to_string());
    }

    Ok(())
}

pub async fn validate_contacts(
    database: &Database,
    contest_rules: &ContestRulesStore,
    log_id: i64,
    contacts: &[Contact],
) -> Result<(), String> {
    if log_id <= 0 {
        return Err("log id must be positive".to_string());
    }
    if contacts.is_empty() {
        return Err("contacts payload must contain at least one contact".to_string());
    }
    if contacts.len() > MAX_CONTACTS_PER_UPLOAD {
        return Err(format!(
            "contacts payload cannot contain more than {MAX_CONTACTS_PER_UPLOAD} contacts"
        ));
    }

    let log = database
        .log(log_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("log {log_id} not found"))?;
    let rules = contest_rules
        .get(&log.contest_id)
        .ok_or_else(|| format!("unknown contest: {}", log.contest_id))?;

    for (index, contact) in contacts.iter().enumerate() {
        if force_commit_requested(contact) {
            validate_contact_shape(contact)
                .map_err(|error| format!("contact {}: {error}", index + 1))?;
        } else {
            validate_contact(rules, log_id, contact)
                .map_err(|error| format!("contact {}: {error}", index + 1))?;
        }

        if let Some(contact_id) = contact_id(contact)
            && let Some(existing_log_id) = database
                .contact_log_id(contact_id)
                .await
                .map_err(|error| format!("contact {}: {error}", index + 1))?
            && existing_log_id != log_id
        {
            return Err(format!(
                "contact {}: contact id {contact_id} belongs to log {existing_log_id}",
                index + 1
            ));
        }
    }

    Ok(())
}

pub fn force_commit_requested(contact: &Contact) -> bool {
    contact
        .get("_force")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn validate_radio_frequency_hz(frequency_hz: u64) -> Result<(), String> {
    if frequency_hz == 0 || frequency_hz > MAX_RADIO_FREQUENCY_HZ {
        return Err(format!(
            "frequency must be between 1 and {MAX_RADIO_FREQUENCY_HZ} Hz"
        ));
    }
    Ok(())
}

pub fn validate_radio_mode(mode: &str) -> Result<(), String> {
    let mode = mode.trim().to_uppercase();
    if LOGGER_MODE_OPTIONS.contains(&mode.as_str()) {
        Ok(())
    } else {
        Err(format!(
            "mode must be one of: {}",
            LOGGER_MODE_OPTIONS.join(", ")
        ))
    }
}

pub fn validate_message_request(
    request_id: &str,
    mode: &str,
    key: &str,
    fields: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    validate_required_text("Message request id", request_id, MAX_CW_REQUEST_ID_LEN)?;

    let normalized_mode = mode.trim().to_lowercase();
    if normalized_mode != "run" && normalized_mode != "s&p" && normalized_mode != "sp" {
        return Err("Message mode must be run or s&p".to_string());
    }

    let normalized_key = key.trim().to_uppercase();
    if !matches!(
        normalized_key.as_str(),
        "F1" | "F2" | "F3" | "F4" | "F5" | "F6" | "F7" | "F8" | "F9" | "F10" | "F11" | "F12"
    ) {
        return Err("Message key must be F1 through F12".to_string());
    }

    if fields.len() > MAX_WS_FIELDS {
        return Err(format!(
            "Message fields cannot contain more than {MAX_WS_FIELDS} entries"
        ));
    }
    for (key, value) in fields {
        validate_contact_key(key)?;
        validate_json_value_size(value, 0).map_err(|error| format!("{key}: {error}"))?;
    }

    Ok(())
}

pub fn validate_cw_text_request(request_id: &str, text: &str) -> Result<(), String> {
    validate_required_text("CW request id", request_id, MAX_CW_REQUEST_ID_LEN)?;
    validate_required_text("CW text", text, MAX_CW_TEXT_LEN)?;
    Ok(())
}

pub fn validate_dxcluster_spot_request(
    frequency_hz: u64,
    call: &str,
    comment: &str,
) -> Result<(), String> {
    validate_radio_frequency_hz(frequency_hz)?;
    validate_required_text("DX spot callsign", call, MAX_CALLSIGN_LEN)?;
    if call.trim().chars().any(char::is_whitespace) {
        return Err("DX spot callsign cannot contain whitespace".to_string());
    }
    validate_optional_plain_text("DX spot comment", comment, MAX_DXCLUSTER_SPOT_COMMENT_LEN)?;
    Ok(())
}

pub fn validate_cw_wpm(wpm: u8) -> Result<(), String> {
    if !(MIN_CW_WPM..=MAX_CW_WPM).contains(&wpm) {
        return Err(format!(
            "CW WPM must be between {MIN_CW_WPM} and {MAX_CW_WPM}"
        ));
    }
    Ok(())
}

fn persisted_log_fields(rules: &ContestRules) -> Vec<&ContestParam> {
    let mut fields = rules.log_params.iter().collect::<Vec<_>>();
    if let Some(cabrillo) = &rules.cabrillo {
        fields.extend(cabrillo.log_fields.iter());
    }
    fields
}

fn cabrillo_export_fields(rules: &ContestRules) -> Vec<&ContestParam> {
    rules
        .cabrillo
        .as_ref()
        .map(|cabrillo| cabrillo.export_fields.iter().collect())
        .unwrap_or_default()
}

fn validate_persisted_log_params(
    rules: &ContestRules,
    contest_params: &Value,
) -> Result<(), String> {
    validate_configured_params(persisted_log_fields(rules), contest_params)
}

fn validate_configured_params(
    fields: Vec<&ContestParam>,
    contest_params: &Value,
) -> Result<(), String> {
    let empty_params = serde_json::Map::new();
    let params = match contest_params.as_object() {
        Some(params) => params,
        None if contest_params.is_null() => &empty_params,
        None => return Err("contest parameters must be an object".to_string()),
    };
    let known_params = fields
        .iter()
        .map(|param| param.name.as_str())
        .collect::<HashSet<_>>();

    for key in params.keys() {
        if !known_params.contains(key.as_str()) {
            return Err(format!("unknown contest parameter: {key}"));
        }
        validate_contact_key(key)?;
    }

    for param in fields {
        validate_contest_param(param, params.get(&param.name))?;
    }

    Ok(())
}

fn validate_contest_param(param: &ContestParam, value: Option<&Value>) -> Result<(), String> {
    let label = param.label.as_str();
    let value = json_trimmed_string(value).unwrap_or_default();
    let multiline = param
        .widget
        .as_deref()
        .map(|widget| widget.eq_ignore_ascii_case("textarea"))
        .unwrap_or(false)
        || param.max_lines.is_some();

    if value.is_empty() {
        if param.required == Some(false) {
            return Ok(());
        }
        return Err(format!("{label} is required"));
    }

    if multiline {
        let lines = value.lines().collect::<Vec<_>>();
        if let Some(max_lines) = param.max_lines
            && lines.len() > max_lines
        {
            return Err(format!("{label} must be at most {max_lines} lines"));
        }
        for line in lines {
            if line.trim().is_empty() {
                return Err(format!("{label} cannot contain blank lines"));
            }
            if line.chars().any(char::is_control) {
                return Err(format!("{label} cannot contain control characters"));
            }
            validate_typed_field(
                label,
                &param.field_type,
                line,
                &param.valid_values,
                param.regex.as_deref(),
                "CW",
            )?;
        }
        return Ok(());
    }

    if value.chars().any(char::is_control) {
        return Err(format!("{label} cannot contain control characters"));
    }

    validate_typed_field(
        label,
        &param.field_type,
        &value,
        &param.valid_values,
        param.regex.as_deref(),
        "CW",
    )
}

fn validate_contact(rules: &ContestRules, log_id: i64, contact: &Contact) -> Result<(), String> {
    validate_contact_shape(contact)?;

    if let Some(id) = contact_id(contact)
        && id <= 0
    {
        return Err("contact id must be positive".to_string());
    }

    if let Some(contact_log_id) = json_i64(contact.get("_log_id"))
        && contact_log_id != log_id
    {
        return Err("contact log id does not match request log id".to_string());
    }

    if let Some(contest_id) = json_trimmed_string(contact.get("CONTEST_ID"))
        && !contest_id.eq_ignore_ascii_case(&rules.contest)
    {
        return Err("contact contest id does not match log contest".to_string());
    }

    validate_qso_epoch(contact)?;
    let station_callsign = json_trimmed_string(contact.get("STATION_CALLSIGN")).unwrap_or_default();
    validate_required_text("station callsign", &station_callsign, MAX_CALLSIGN_LEN)?;
    if let Some(operator) = json_trimmed_string(contact.get("OPERATOR"))
        && !operator.is_empty()
    {
        validate_required_text("operator callsign", &operator, MAX_CALLSIGN_LEN)?;
    }
    let callsign = json_trimmed_string(contact.get("CALL")).unwrap_or_default();
    validate_required_text("callsign", &callsign, MAX_CALLSIGN_LEN)?;
    validate_contact_band_and_frequency(rules, contact)?;
    let mode = validate_contact_mode(rules, contact)?;

    if let Some(client_id) = json_trimmed_string(contact.get("_client_id")) {
        validate_required_text("client id", &client_id, MAX_CW_REQUEST_ID_LEN)?;
    }
    if let Some(session_id) = json_trimmed_string(contact.get("_session_id")) {
        validate_required_text("session id", &session_id, MAX_CONTACT_STRING_LEN)?;
    }

    for field in &rules.exchange {
        validate_exchange_field(field, contact, &mode)?;
    }

    Ok(())
}

fn validate_contact_shape(contact: &Contact) -> Result<(), String> {
    if contact.len() > MAX_CONTACT_FIELDS {
        return Err(format!(
            "contact cannot contain more than {MAX_CONTACT_FIELDS} fields"
        ));
    }

    for (key, value) in contact {
        validate_contact_key(key)?;
        validate_json_value_size(value, 0).map_err(|error| format!("{key}: {error}"))?;
    }

    Ok(())
}

fn validate_contact_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("field names cannot be empty".to_string());
    }
    if key.chars().count() > MAX_CONTACT_KEY_LEN {
        return Err(format!(
            "field name {key} must be at most {MAX_CONTACT_KEY_LEN} characters"
        ));
    }
    if key.chars().any(char::is_control) {
        return Err(format!(
            "field name {key} cannot contain control characters"
        ));
    }
    Ok(())
}

fn validate_json_value_size(value: &Value, depth: usize) -> Result<(), String> {
    if depth > MAX_CONTACT_JSON_DEPTH {
        return Err(format!(
            "nested JSON cannot be deeper than {MAX_CONTACT_JSON_DEPTH} levels"
        ));
    }

    match value {
        Value::String(value) => {
            if value.chars().count() > MAX_CONTACT_STRING_LEN {
                Err(format!(
                    "string value must be at most {MAX_CONTACT_STRING_LEN} characters"
                ))
            } else {
                Ok(())
            }
        }
        Value::Array(values) => {
            if values.len() > MAX_CONTACT_ARRAY_ITEMS {
                return Err(format!(
                    "array value cannot contain more than {MAX_CONTACT_ARRAY_ITEMS} items"
                ));
            }
            for value in values {
                validate_json_value_size(value, depth + 1)?;
            }
            Ok(())
        }
        Value::Object(values) => {
            if values.len() > MAX_CONTACT_OBJECT_FIELDS {
                return Err(format!(
                    "object value cannot contain more than {MAX_CONTACT_OBJECT_FIELDS} fields"
                ));
            }
            for (key, value) in values {
                validate_contact_key(key)?;
                validate_json_value_size(value, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_qso_epoch(contact: &Contact) -> Result<(), String> {
    let epoch = json_i64(contact.get("QSO_DATE_TIME_ON")).or_else(|| legacy_epoch(contact));
    let Some(epoch) = epoch else {
        return Err("QSO date/time is required".to_string());
    };

    if !(MIN_QSO_EPOCH..=MAX_QSO_EPOCH).contains(&epoch) {
        return Err("QSO date/time is out of range".to_string());
    }

    Ok(())
}

fn validate_contact_band_and_frequency(
    rules: &ContestRules,
    contact: &Contact,
) -> Result<(), String> {
    let band_text = json_trimmed_string(contact.get("BAND")).unwrap_or_default();
    if band_text.is_empty() {
        return Err("band is required".to_string());
    }
    let band_meters = band_meters(&band_text).ok_or_else(|| "band is invalid".to_string())?;
    if !rules.allowed_bands.is_empty() && !rules.allowed_bands.contains(&band_meters) {
        return Err(format!("band must be one of: {}", allowed_bands(rules)));
    }

    let frequency_hz = contact_frequency_hz(contact.get("FREQ"))
        .ok_or_else(|| "frequency is required".to_string())?;
    validate_radio_frequency_hz(frequency_hz)?;
    let frequency_band = band_for_frequency(Frequency::from_hz(frequency_hz))
        .ok_or_else(|| "frequency is outside supported amateur bands".to_string())?;
    if frequency_band.meters != band_meters {
        return Err("frequency does not match band".to_string());
    }

    Ok(())
}

fn validate_contact_mode(rules: &ContestRules, contact: &Contact) -> Result<String, String> {
    let mode = json_trimmed_string(contact.get("MODE")).unwrap_or_default();
    if mode.is_empty() {
        return Err("mode is required".to_string());
    }
    let mode = mode.to_uppercase();
    if !rules.allowed_modes.is_empty()
        && !rules
            .allowed_modes
            .iter()
            .any(|allowed_mode| allowed_mode.eq_ignore_ascii_case(&mode))
    {
        return Err(format!(
            "mode must be one of: {}",
            rules.allowed_modes.join(", ")
        ));
    }
    Ok(mode)
}

fn validate_exchange_field(
    field: &ExchangeField,
    contact: &Contact,
    radio_mode: &str,
) -> Result<(), String> {
    let value = json_trimmed_string(contact.get(&field.adif)).unwrap_or_default();
    if value.is_empty() {
        return Err(format!("{} is required", field.name));
    }
    validate_typed_field(
        &field.name,
        &field.field_type,
        &value,
        &field.valid_values,
        field.regex.as_deref(),
        radio_mode,
    )
}

fn validate_typed_field(
    label: &str,
    field_type: &str,
    value: &str,
    valid_values: &[String],
    pattern: Option<&str>,
    radio_mode: &str,
) -> Result<(), String> {
    let normalized_value = value.trim().to_uppercase();
    if normalized_value.is_empty() {
        return Err(format!("{label} is required"));
    }

    let parsed = parse_field_type(field_type, radio_mode);
    if normalized_value.chars().count() > parsed.max_length {
        return Err(format!(
            "{label} must be at most {} characters",
            parsed.max_length
        ));
    }

    if parsed.kind == "RST" {
        let expected_length = if mode_is_cw(radio_mode) { 3 } else { 2 };
        if !is_valid_rst(&normalized_value, expected_length) {
            return Err(format!(
                "{label} must be a valid {expected_length}-digit RST"
            ));
        }
    } else if parsed.kind == "NUMERIC"
        && !normalized_value
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err(format!("{label} must be numeric"));
    }

    if !valid_values.is_empty()
        && !valid_values
            .iter()
            .any(|valid_value| valid_value.eq_ignore_ascii_case(&normalized_value))
    {
        return Err(format!(
            "{label} must be one of: {}",
            valid_values.join(", ")
        ));
    }

    if let Some(pattern) = pattern {
        let regex =
            Regex::new(pattern).map_err(|error| format!("invalid regex for {label}: {error}"))?;
        if !regex.is_match(&normalized_value) {
            return Err(format!("{label} is invalid"));
        }
    }

    Ok(())
}

fn parse_field_type(field_type: &str, radio_mode: &str) -> ParsedFieldType {
    let mut parts = field_type.split(':');
    let kind = parts.next().unwrap_or("STRING").trim().to_uppercase();
    let raw_length = parts.next().unwrap_or("8");
    let max_length = if kind == "RST" {
        if mode_is_cw(radio_mode) { 3 } else { 2 }
    } else {
        raw_length
            .parse::<usize>()
            .ok()
            .filter(|length| *length > 0)
            .unwrap_or(8)
    };

    ParsedFieldType { kind, max_length }
}

fn mode_is_cw(mode: &str) -> bool {
    matches!(mode.trim().to_uppercase().as_str(), "CW" | "CW-R")
}

fn is_valid_rst(value: &str, expected_length: usize) -> bool {
    value.len() == expected_length
        && value.len() >= 2
        && value.len() <= 3
        && value.as_bytes()[0].is_ascii_digit()
        && (b'1'..=b'5').contains(&value.as_bytes()[0])
        && value.as_bytes()[1..]
            .iter()
            .all(|digit| (b'1'..=b'9').contains(digit))
}

fn validate_required_text(label: &str, value: &str, max_length: usize) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label} is required"));
    }
    if value.chars().count() > max_length {
        return Err(format!("{label} must be at most {max_length} characters"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} cannot contain control characters"));
    }
    Ok(())
}

fn validate_optional_plain_text(label: &str, value: &str, max_length: usize) -> Result<(), String> {
    let value = value.trim();
    if value.chars().count() > max_length {
        return Err(format!("{label} must be at most {max_length} characters"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} cannot contain control characters"));
    }
    Ok(())
}

fn validate_host(label: &str, value: &str) -> Result<(), String> {
    let value = value.trim();
    if value
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(format!(
            "{label} cannot contain whitespace or control characters"
        ));
    }
    Ok(())
}

fn validate_serial_port(label: &str, value: &str) -> Result<(), String> {
    if value.chars().any(char::is_control) {
        return Err(format!("{label} cannot contain control characters"));
    }
    Ok(())
}

fn validate_seconds(label: &str, value: f64) -> Result<(), String> {
    if !value.is_finite() || !(MIN_RADIO_SECONDS..=MAX_RADIO_SECONDS).contains(&value) {
        return Err(format!(
            "{label} must be between {MIN_RADIO_SECONDS} and {MAX_RADIO_SECONDS} seconds"
        ));
    }
    Ok(())
}

fn allowed_bands(rules: &ContestRules) -> String {
    rules
        .allowed_bands
        .iter()
        .map(|meters| format!("{meters}m"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn band_meters(value: &str) -> Option<u16> {
    let normalized = value.trim().to_lowercase();
    USA_AMATEUR_BANDS
        .iter()
        .find(|band| band.name.eq_ignore_ascii_case(&normalized))
        .map(|band| band.meters)
        .or_else(|| {
            normalized
                .strip_suffix('m')
                .unwrap_or(&normalized)
                .parse::<u16>()
                .ok()
        })
}

fn contact_frequency_hz(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_i64().and_then(|value| u64::try_from(value).ok()))
            .or_else(|| number.as_f64().and_then(decimal_frequency_to_hz)),
        Value::String(string) => {
            let string = string.trim();
            if string.contains('.') {
                string.parse::<f64>().ok().and_then(decimal_frequency_to_hz)
            } else {
                string.parse::<u64>().ok()
            }
        }
        _ => None,
    }
}

fn decimal_frequency_to_hz(value: f64) -> Option<u64> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let hz = if value.abs() < 1_000_000.0 {
        value * 1_000_000.0
    } else {
        value
    };
    if hz > u64::MAX as f64 {
        None
    } else {
        Some(hz.round() as u64)
    }
}

fn contact_id(contact: &Contact) -> Option<i64> {
    contact
        .get("_id")
        .or_else(|| contact.get("ID"))
        .and_then(json_i64_value)
}

fn json_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(json_i64_value)
}

fn json_i64_value(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok())),
        Value::String(string) => string.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn json_trimmed_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(string) => Some(string.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn legacy_epoch(contact: &Contact) -> Option<i64> {
    let date = contact.get("QSO_DATE")?.as_str()?;
    let time = contact.get("TIME_ON")?.as_str()?;
    if date.len() != 8 || time.len() != 6 {
        return None;
    }
    let year = date[0..4].parse::<i32>().ok()?;
    let month = date[4..6].parse::<u32>().ok()?;
    let day = date[6..8].parse::<u32>().ok()?;
    let hour = time[0..2].parse::<u32>().ok()?;
    let minute = time[2..4].parse::<u32>().ok()?;
    let second = time[4..6].parse::<u32>().ok()?;
    let days = days_from_civil(year, month, day)?;
    Some(days * 86_400 + i64::from(hour * 3_600 + minute * 60 + second))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(i64::from(era * 146_097 + doe - 719_468))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    fn test_rules() -> ContestRules {
        ContestRules {
            contest: "TEST".to_string(),
            display_name: "Test".to_string(),
            allowed_bands: vec![20],
            allowed_modes: vec!["CW".to_string(), "SSB".to_string()],
            define: Vec::new(),
            exchange: vec![ExchangeField {
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
            }],
            qso_columns: Vec::new(),
            qso_column_fields: Default::default(),
            log_params: Vec::new(),
            qso_points: None,
            dupe_key: Vec::new(),
            multipliers: Vec::new(),
            bonus_points: Vec::new(),
            cabrillo: None,
            metadata: None,
        }
    }

    fn test_contact() -> Contact {
        Map::from_iter([
            ("_log_id".to_string(), json!(1)),
            ("CONTEST_ID".to_string(), json!("TEST")),
            ("QSO_DATE_TIME_ON".to_string(), json!(1_700_000_000_i64)),
            ("STATION_CALLSIGN".to_string(), json!("N0CALL")),
            ("CALL".to_string(), json!("K1ABC")),
            ("BAND".to_string(), json!("20m")),
            ("FREQ".to_string(), json!(14_074_000_i64)),
            ("MODE".to_string(), json!("CW")),
            ("RST_RCVD".to_string(), json!(599)),
        ])
    }

    fn test_radio() -> NewRadio {
        NewRadio {
            name: "Elecraft TCP".to_string(),
            radio_kind: "k4".to_string(),
            transport_kind: "tcp".to_string(),
            tcp_host: "127.0.0.1".to_string(),
            tcp_port: 5002,
            serial_port: String::new(),
            serial_baud_rate: 115_200,
            options: String::new(),
            poll_frequency: 0.25,
            cat_timeout: 2.0,
            cw_keyer_type: "none".to_string(),
            winkeyer_serial_port: String::new(),
            cw_serial_port: String::new(),
            cw_serial_baud_rate: 9_600,
            cw_serial_line: "dtr".to_string(),
            cw_messages: cw::DEFAULT_CW_MESSAGES.to_string(),
        }
    }

    #[test]
    fn validates_typed_fields_like_frontend() {
        assert!(validate_typed_field("RST", "RST", "599", &[], None, "CW").is_ok());
        assert!(validate_typed_field("RST", "RST", "599", &[], None, "CW-R").is_ok());
        assert!(validate_typed_field("RST", "RST", "59", &[], None, "CW").is_err());
        assert!(validate_typed_field("Serial", "Numeric:3", "123", &[], None, "CW").is_ok());
        assert!(validate_typed_field("Serial", "Numeric:3", "12A", &[], None, "CW").is_err());
        assert!(
            validate_typed_field("Section", "String:3", "SC", &["SC".to_string()], None, "CW")
                .is_ok()
        );
        assert!(
            validate_typed_field("Section", "String:3", "GA", &["SC".to_string()], None, "CW")
                .is_err()
        );
    }

    #[test]
    fn validates_logger_mode_requests() {
        for mode in LOGGER_MODE_OPTIONS {
            assert!(validate_radio_mode(mode).is_ok());
        }
        assert!(validate_radio_mode("AM").is_err());
    }

    #[test]
    fn validates_contact_payload() {
        let rules = test_rules();
        assert!(validate_contact(&rules, 1, &test_contact()).is_ok());
    }

    #[test]
    fn rejects_invalid_contact_exchange() {
        let rules = test_rules();
        let mut contact = test_contact();
        contact.insert("RST_RCVD".to_string(), json!(59));
        assert!(validate_contact(&rules, 1, &contact).is_err());
    }

    #[test]
    fn rejects_mismatched_contact_frequency_and_band() {
        let rules = test_rules();
        let mut contact = test_contact();
        contact.insert("BAND".to_string(), json!("40m"));
        assert!(validate_contact(&rules, 1, &contact).is_err());
    }

    #[test]
    fn validates_tcp_radio_config() {
        assert!(validate_radio(&test_radio()).is_ok());
    }

    #[test]
    fn validates_serial_radio_config() {
        let mut radio = test_radio();
        radio.transport_kind = "serial".to_string();
        radio.tcp_host = String::new();
        radio.tcp_port = 0;
        radio.serial_port = "/dev/ttyUSB0".to_string();

        assert!(validate_radio(&radio).is_ok());
    }

    #[test]
    fn rejects_unknown_radio_kind() {
        let mut radio = test_radio();
        radio.radio_kind = "not-a-radio".to_string();

        let error = validate_radio(&radio).expect_err("radio kind should be rejected");
        assert!(error.contains("unsupported radio kind"));
    }

    #[test]
    fn rejects_tcp_radio_without_host() {
        let mut radio = test_radio();
        radio.tcp_host = String::new();

        let error = validate_radio(&radio).expect_err("missing host should fail");
        assert!(error.contains("TCP host"));
    }

    #[test]
    fn validates_cat_cw_keyer_type() {
        let mut radio = test_radio();
        radio.cw_keyer_type = "cat".to_string();

        assert!(validate_radio(&radio).is_ok());
    }

    #[test]
    fn winkeyer_requires_serial_port() {
        let mut radio = test_radio();
        radio.cw_keyer_type = "winkeyer".to_string();

        let error = validate_radio(&radio).expect_err("winkeyer port should be required");
        assert!(error.contains("Winkeyer serial port"));
    }

    #[test]
    fn validates_serial_cw_keyer_type() {
        let mut radio = test_radio();
        radio.cw_keyer_type = "serial".to_string();
        radio.cw_serial_port = "/dev/ttyUSB1".to_string();
        radio.cw_serial_baud_rate = 9_600;
        radio.cw_serial_line = "rts".to_string();

        assert!(validate_radio(&radio).is_ok());
    }

    #[test]
    fn serial_cw_keyer_requires_serial_port() {
        let mut radio = test_radio();
        radio.cw_keyer_type = "serial".to_string();

        let error = validate_radio(&radio).expect_err("CW serial port should be required");
        assert!(error.contains("CW serial port"));
    }

    #[test]
    fn serial_cw_keyer_rejects_unknown_line() {
        let mut radio = test_radio();
        radio.cw_keyer_type = "serial".to_string();
        radio.cw_serial_port = "/dev/ttyUSB1".to_string();
        radio.cw_serial_line = "both".to_string();

        let error = validate_radio(&radio).expect_err("CW serial line should be rejected");
        assert!(error.contains("CW serial line"));
    }

    #[test]
    fn serial_cw_keyer_allows_shared_cat_port_with_matching_baud_rate() {
        let mut radio = test_radio();
        radio.transport_kind = "serial".to_string();
        radio.tcp_host = String::new();
        radio.tcp_port = 0;
        radio.serial_port = "/dev/ttyUSB0".to_string();
        radio.serial_baud_rate = 9_600;
        radio.cw_keyer_type = "serial".to_string();
        radio.cw_serial_port = "/dev/ttyUSB0".to_string();
        radio.cw_serial_baud_rate = 9_600;

        assert!(validate_radio(&radio).is_ok());
    }

    #[test]
    fn serial_cw_keyer_rejects_shared_cat_port_with_different_baud_rate() {
        let mut radio = test_radio();
        radio.transport_kind = "serial".to_string();
        radio.tcp_host = String::new();
        radio.tcp_port = 0;
        radio.serial_port = "/dev/ttyUSB0".to_string();
        radio.serial_baud_rate = 115_200;
        radio.cw_keyer_type = "serial".to_string();
        radio.cw_serial_port = "/dev/ttyUSB0".to_string();
        radio.cw_serial_baud_rate = 9_600;

        let error = validate_radio(&radio).expect_err("shared baud rate mismatch should fail");
        assert!(error.contains("baud rate"));
    }

    #[test]
    fn serial_cw_keyer_allows_different_cat_port_with_different_baud_rate() {
        let mut radio = test_radio();
        radio.transport_kind = "serial".to_string();
        radio.tcp_host = String::new();
        radio.tcp_port = 0;
        radio.serial_port = "/dev/ttyUSB0".to_string();
        radio.serial_baud_rate = 115_200;
        radio.cw_keyer_type = "serial".to_string();
        radio.cw_serial_port = "/dev/ttyUSB1".to_string();
        radio.cw_serial_baud_rate = 9_600;

        assert!(validate_radio(&radio).is_ok());
    }

    #[test]
    fn rejects_unknown_cw_keyer_type() {
        let mut radio = test_radio();
        radio.cw_keyer_type = "laser".to_string();

        let error = validate_radio(&radio).expect_err("cw keyer type should be rejected");
        assert!(error.contains("CW keyer type"));
    }
}
