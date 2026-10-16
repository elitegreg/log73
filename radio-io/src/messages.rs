use crate::message_mode::{RUN_MESSAGE_MODE, parse_message_mode_section_header};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMessageEntry {
    pub mode: String,
    pub key: String,
    pub label: String,
    pub target: String,
}

pub fn parse_message_entries(config: &str) -> Vec<ParsedMessageEntry> {
    let mut entries = Vec::new();
    let mut current_mode = None::<&str>;

    for raw_line in config.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(mode) = parse_message_mode_section_header(line) {
            current_mode = Some(mode);
            continue;
        }
        if line.starts_with('#') || current_mode.is_none() {
            continue;
        }

        let Some(entry) = parse_message_line(line, current_mode) else {
            continue;
        };
        entries.push(entry);
    }

    entries
}

pub fn validate_message_config(config: &str, target_label: &str) -> Result<(), String> {
    let mut current_mode = None::<&str>;
    let mut run_keys = HashSet::new();
    let mut search_and_pounce_keys = HashSet::new();

    for (index, raw_line) in config.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(mode) = parse_message_mode_section_header(line) {
            current_mode = Some(mode);
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        let mode = current_mode
            .ok_or_else(|| format!("line {line_number}: message is outside a mode section"))?;
        let entry = parse_message_line(line, Some(mode))
            .ok_or_else(|| format!("line {line_number}: expected 'F# Label,{target_label}'"))?;
        if !is_valid_function_key(&entry.key) {
            return Err(format!(
                "line {line_number}: message key must be F1 through F12"
            ));
        }

        let keys = if mode == RUN_MESSAGE_MODE {
            &mut run_keys
        } else {
            &mut search_and_pounce_keys
        };
        if !keys.insert(entry.key.clone()) {
            return Err(format!(
                "line {line_number}: duplicate {} message in {mode} section",
                entry.key
            ));
        }
    }

    if run_keys.is_empty() {
        return Err("messages must include at least one Run message".to_string());
    }
    if search_and_pounce_keys.is_empty() {
        return Err("messages must include at least one S&P message".to_string());
    }

    Ok(())
}

pub fn action_from_template(template: &str) -> Option<String> {
    let inner = template.trim().strip_prefix('{')?.strip_suffix('}')?.trim();
    let (name, value) = inner.split_once(':')?;
    let value = value.trim();
    if name.trim().eq_ignore_ascii_case("action")
        && !value.is_empty()
        && !value.contains(['{', '}'])
    {
        Some(value.to_string())
    } else {
        None
    }
}

fn parse_message_line(line: &str, current_mode: Option<&str>) -> Option<ParsedMessageEntry> {
    let (key_and_label, target) = line.split_once(',')?;
    let mut parts = key_and_label.splitn(2, char::is_whitespace);
    let key = parts.next()?.trim();
    let label = parts.next().unwrap_or("").trim();
    let normalized_key = key.to_uppercase();
    if !normalized_key.starts_with('F') {
        return None;
    }

    Some(ParsedMessageEntry {
        mode: current_mode.unwrap_or_default().to_string(),
        key: normalized_key,
        label: label.to_string(),
        target: target.trim().to_string(),
    })
}

pub fn is_valid_function_key(key: &str) -> bool {
    matches!(
        key.trim().to_uppercase().as_str(),
        "F1" | "F2" | "F3" | "F4" | "F5" | "F6" | "F7" | "F8" | "F9" | "F10" | "F11" | "F12"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MESSAGES: &str = r#"
# RUN Messages
F1 Cq,CQ TEST
F12 Clear,{Action:Clear}
# S&P Messages
F1 Qrl?,QRL?
"#;

    #[derive(serde::Deserialize)]
    struct Fixture {
        version: u32,
        parser_cases: Vec<ParserCase>,
        action_cases: Vec<ActionCase>,
        default_configs: Vec<DefaultConfig>,
    }

    #[derive(serde::Deserialize)]
    struct ParserCase {
        name: String,
        config: Option<String>,
        entries: Vec<FixtureEntry>,
    }

    #[derive(serde::Deserialize)]
    struct ActionCase {
        name: String,
        template: Option<String>,
        action: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct DefaultConfig {
        kind: String,
        config: String,
        entries: Vec<FixtureEntry>,
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct FixtureEntry {
        mode: String,
        key: String,
        label: String,
        target: String,
    }

    fn load_fixture() -> Fixture {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/domain/message-entries.json"
        ))
        .expect("valid message entry fixture");
        assert_eq!(fixture.version, 1);
        assert!(!fixture.parser_cases.is_empty());
        assert!(!fixture.action_cases.is_empty());
        assert!(!fixture.default_configs.is_empty());
        fixture
    }

    fn assert_entries(config: &str, expected: Vec<FixtureEntry>, name: &str) {
        let actual: Vec<_> = parse_message_entries(config)
            .into_iter()
            .map(|entry| FixtureEntry {
                mode: entry.mode,
                key: entry.key,
                label: entry.label,
                target: entry.target,
            })
            .collect();
        assert_eq!(actual, expected, "{name}");
    }

    #[test]
    fn shared_message_entry_contract() {
        for case in load_fixture().parser_cases {
            assert_entries(
                case.config.as_deref().unwrap_or_default(),
                case.entries,
                &case.name,
            );
        }
    }

    #[test]
    fn shared_message_action_contract() {
        for case in load_fixture().action_cases {
            assert_eq!(
                action_from_template(case.template.as_deref().unwrap_or_default()),
                case.action,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn shared_default_message_configs_match_production() {
        for case in load_fixture().default_configs {
            let production = match case.kind.as_str() {
                "cw" => crate::cw::DEFAULT_CW_MESSAGES,
                "digital" => crate::digital_messages::DEFAULT_DIGITAL_MESSAGES,
                "voice" => crate::voice_messages::DEFAULT_VOICE_MESSAGES,
                other => panic!("unknown default config: {other}"),
            };
            assert_eq!(
                production, case.config,
                "{} default fixture is stale",
                case.kind
            );
            assert_entries(production, case.entries, &case.kind);
            validate_message_config(production, "Target").expect("default config must validate");
            if case.kind == "voice" {
                crate::voice_messages::validate(production)
                    .expect("default voice paths must validate");
            } else {
                crate::cw::validate(production).expect("default text messages must validate");
            }
        }
    }

    #[test]
    fn shared_header_cases_drive_message_sections() {
        #[derive(serde::Deserialize)]
        struct HeaderFixture {
            version: u32,
            header_cases: Vec<HeaderCase>,
        }
        #[derive(serde::Deserialize)]
        struct HeaderCase {
            name: String,
            input: Option<String>,
            mode: Option<String>,
        }
        let fixture: HeaderFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/domain/message-modes.json"
        ))
        .expect("valid message mode fixture");
        assert_eq!(fixture.version, 1);
        assert!(!fixture.header_cases.is_empty());
        for case in fixture.header_cases {
            let config = format!(
                "{}\nF1 Label,target",
                case.input.as_deref().unwrap_or_default()
            );
            let expected = case
                .mode
                .into_iter()
                .map(|mode| FixtureEntry {
                    mode,
                    key: "F1".to_string(),
                    label: "Label".to_string(),
                    target: "target".to_string(),
                })
                .collect();
            assert_entries(&config, expected, &case.name);
        }
    }

    #[test]
    fn validation_remains_stricter_than_entry_parsing() {
        for invalid in ["F13 Extra", "Foo Text", "F Bare"] {
            let config = format!("# RUN Messages\n{invalid},CQ\n# S&P Messages\nF1 Qrl,QRL?");
            assert_eq!(parse_message_entries(&config).len(), 2);
            assert!(validate_message_config(&config, "Message").is_err());
        }
        validate_message_config("# RUN Messages\nF1,CQ\n# S&P Messages\nF1,", "Message")
            .expect("empty labels and targets are valid parser entries");
    }

    #[test]
    fn parses_entries_and_action_tokens() {
        let entries = parse_message_entries(TEST_MESSAGES);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].mode, "run");
        assert_eq!(entries[0].key, "F1");
        assert_eq!(entries[0].label, "Cq");
        assert_eq!(
            action_from_template(&entries[1].target),
            Some("Clear".to_string())
        );
    }

    #[test]
    fn validates_message_config_sections_and_duplicates() {
        assert!(validate_message_config(TEST_MESSAGES, "Message").is_ok());
        assert!(validate_message_config("F1 Bad,CQ", "Message").is_err());
        assert!(
            validate_message_config(
                "# RUN Messages\nF1 A,CQ\nF1 B,CQ\n# S&P Messages\nF1 C,CQ",
                "Message"
            )
            .is_err()
        );
    }
}
