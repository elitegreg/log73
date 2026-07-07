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
        if line.starts_with('#') {
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
    if name.trim().eq_ignore_ascii_case("action") {
        Some(value.trim().to_string())
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
