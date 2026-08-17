use crate::message_mode::{RUN_MESSAGE_MODE, SEARCH_AND_POUNCE_MESSAGE_MODE};
use crate::messages::{ParsedMessageEntry, parse_message_entries, validate_message_config};
use serde::Serialize;

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

pub fn validate(config: &str) -> Result<CwLabels, String> {
    validate_message_config(config, "Message")
        .map_err(|error| error.replace("messages must", "CW messages must"))?;
    Ok(labels(config))
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

    for entry in parse_message_entries(config) {
        let mode = entry.mode.clone();
        let message = cw_message_from_entry(entry);
        match mode.as_str() {
            RUN_MESSAGE_MODE => messages.run.push(message),
            SEARCH_AND_POUNCE_MESSAGE_MODE => messages.search_and_pounce.push(message),
            _ => {}
        }
    }

    messages
}

fn cw_message_from_entry(entry: ParsedMessageEntry) -> CwMessage {
    CwMessage {
        key: entry.key,
        label: entry.label,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn validates_sensible_cw_messages() {
        let labels = validate(TEST_MESSAGES).expect("messages should validate");
        assert_eq!(labels.run.len(), 2);
        assert_eq!(labels.search_and_pounce.len(), 1);
    }

    #[test]
    fn rejects_invalid_cw_messages() {
        assert!(validate("F1 Cq,CQ").is_err());
        assert!(validate("# RUN Messages\nF13 Bad,BAD\n# S&P Messages\nF1 Ok,OK").is_err());
        assert!(validate("# RUN Messages\nF1 Cq,CQ\n# S&P Messages").is_err());
        assert!(
            validate("# RUN Messages\nF1 Cq,CQ\nF1 Again,CQ\n# S&P Messages\nF1 Ok,OK").is_err()
        );
    }
}
