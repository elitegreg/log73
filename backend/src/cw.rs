use serde::Serialize;
use serde_json::{Map, Value};

pub const DEFAULT_CW_MESSAGES: &str = r#"###################
#   RUN Messages
###################
F1 Cq,Cq Test {STATION_CALLSIGN}
F2 Exch,{EXCH}
F3 Tu,Tu
F4 {STATION_CALLSIGN},{STATION_CALLSIGN}
F5 His Call,{CALL}
F6 Repeat,{EXCH} {EXCH}
F7 ?, ?
F8 Agn?,Agn?
F9 Nr?,Nr?
F10 Call?,Cl?
F11 -,
F12 Clear,{Action:Clear}
#
###################
#   S&P Messages
###################
F1 Qrl?,Qrl? de {STATION_CALLSIGN}
F2 Exch,{EXCH}
F3 Tu,Tu
F4 {STATION_CALLSIGN},{STATION_CALLSIGN}
F5 His Call,{CALL}
F6 Repeat,{EXCH} {EXCH}
F7 ?,?
F8 Agn?,Agn?
F9 Nr?,Nr?
F10 Call?,Cl?
F11 -,
F12 Clear,{Action:Clear}
"#;

#[derive(Debug, Clone, Serialize)]
pub struct CwLabels {
    pub run: Vec<CwLabel>,
    #[serde(rename = "s&p")]
    pub search_and_pounce: Vec<CwLabel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CwLabel {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone)]
struct CwMessage {
    key: String,
    label: String,
    message: String,
}

#[derive(Debug, Default)]
struct CwMessages {
    run: Vec<CwMessage>,
    search_and_pounce: Vec<CwMessage>,
}

pub fn labels(config: &str) -> CwLabels {
    let messages = parse_messages(config);
    CwLabels {
        run: labels_for(messages.run),
        search_and_pounce: labels_for(messages.search_and_pounce),
    }
}

pub fn render(config: &str, mode: &str, key: &str, fields: &Map<String, Value>) -> Option<String> {
    let messages = parse_messages(config);
    let mode_messages = if normalize_mode(mode) == "run" {
        messages.run
    } else {
        messages.search_and_pounce
    };
    let message = mode_messages
        .iter()
        .find(|message| message.key.eq_ignore_ascii_case(key))?;
    Some(render_template(&message.message, fields).trim().to_string())
}

fn labels_for(messages: Vec<CwMessage>) -> Vec<CwLabel> {
    messages
        .into_iter()
        .map(|message| CwLabel {
            key: message.key,
            label: message.label,
        })
        .collect()
}

fn parse_messages(config: &str) -> CwMessages {
    let mut messages = CwMessages::default();
    let mut current_mode = None::<&str>;

    for raw_line in config.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        let upper = line.to_uppercase();
        if upper.contains("RUN MESSAGES") {
            current_mode = Some("run");
            continue;
        }
        if upper.contains("S&P MESSAGES") || upper.contains("SP MESSAGES") {
            current_mode = Some("s&p");
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        let Some(message) = parse_message_line(line) else {
            continue;
        };
        match current_mode {
            Some("run") => messages.run.push(message),
            Some("s&p") => messages.search_and_pounce.push(message),
            _ => {}
        }
    }

    messages
}

fn parse_message_line(line: &str) -> Option<CwMessage> {
    let (key_and_label, message) = line.split_once(',')?;
    let mut parts = key_and_label.splitn(2, char::is_whitespace);
    let key = parts.next()?.trim();
    let label = parts.next().unwrap_or("").trim();
    if !key.to_uppercase().starts_with('F') {
        return None;
    }

    Some(CwMessage {
        key: key.to_uppercase(),
        label: label.to_string(),
        message: message.trim().to_string(),
    })
}

fn render_template(template: &str, fields: &Map<String, Value>) -> String {
    template
        .replace(
            "{STATION_CALLSIGN}",
            &field_string(fields, "STATION_CALLSIGN"),
        )
        .replace("{CALL}", &field_string(fields, "CALL"))
        .replace("{RST_SENT}", &sent_rst_cut(fields))
        .replace("{EXCH}", &field_string(fields, "EXCH"))
        .replace("{SENTRSTCUT}", &sent_rst_cut(fields))
}

fn sent_rst_cut(fields: &Map<String, Value>) -> String {
    cut_numbers(&field_string(fields, "RST_SENT"))
}

fn cut_numbers(value: &str) -> String {
    value.trim().to_uppercase().replace('9', "N")
}

fn field_string(fields: &Map<String, Value>, key: &str) -> String {
    match fields.get(key) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn normalize_mode(mode: &str) -> String {
    match mode.trim().to_lowercase().as_str() {
        "run" => "run".to_string(),
        _ => "s&p".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEST_MESSAGES: &str = r#"
# RUN Messages
F1 Cq,CQ {STATION_CALLSIGN}
F2 Exch,{RST_SENT} {EXCH} {CALL}
# S&P Messages
F1 His Call,{CALL}
"#;

    #[test]
    fn parses_cw_labels_by_mode() {
        let labels = labels(TEST_MESSAGES);

        assert_eq!(labels.run.len(), 2);
        assert_eq!(labels.run[0].key, "F1");
        assert_eq!(labels.run[0].label, "Cq");
        assert_eq!(labels.search_and_pounce.len(), 1);
        assert_eq!(labels.search_and_pounce[0].key, "F1");
        assert_eq!(labels.search_and_pounce[0].label, "His Call");
    }

    #[test]
    fn renders_cw_template_fields() {
        let fields = json!({
            "STATION_CALLSIGN": "N0CALL",
            "CALL": "K1ABC",
            "EXCH": "5NN BERK",
            "RST_SENT": 599
        })
        .as_object()
        .expect("test fields should be an object")
        .clone();

        assert_eq!(
            render(TEST_MESSAGES, "run", "F2", &fields),
            Some("5NN 5NN BERK K1ABC".to_string())
        );
        assert_eq!(
            render(TEST_MESSAGES, "s&p", "F1", &fields),
            Some("K1ABC".to_string())
        );
    }

    #[test]
    fn rst_sent_placeholder_uses_cut_numbers() {
        let fields = json!({
            "RST_SENT": 599
        })
        .as_object()
        .expect("test fields should be an object")
        .clone();

        assert_eq!(render_template("{RST_SENT}", &fields), "5NN");
        assert_eq!(render_template("{SENTRSTCUT}", &fields), "5NN");
    }

    #[test]
    fn render_returns_none_for_missing_key() {
        let fields = Map::new();
        assert_eq!(render(TEST_MESSAGES, "run", "F12", &fields), None);
    }
}
