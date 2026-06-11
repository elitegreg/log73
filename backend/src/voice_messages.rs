use serde::Serialize;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

pub const DEFAULT_VOICE_MESSAGES: &str = r#"###################
#   RUN Messages
###################
F1 CQ,operator1/CQ.wav
F2 Exch,operator1/Exchange.wav
F3 TNX,operator1/Thanks.wav
F4 {STATION_CALLSIGN},operator1/Mycall.WAV
F5 -,
F6 -,
F7 QRZ?,operator1/QRZ.wav
F8 Agn?,operator1/AllAgain.wav
F9 Exchg?,operator1/Exchange query.wav
F10 -,
F11 -,
F12 Clear,{Action:Clear}
#
###################
#   S&P Messages
###################
F1 QRL?,operator1/QRL.wav
F2 Exch,operator1/Exchange.wav
F3 -,
F4 {STATION_CALLSIGN},operator1/Mycall.WAV
F5 -,
F6 {STATION_CALLSIGN},operator1/Mycall.wav
F7 -,
F8 Agn?,operator1/AllAgain.wav
F9 -,
F10 -,
F11 -,
F12 Clear,{Action:Clear}
"#;

#[derive(Debug, Clone, Serialize)]
pub struct VoiceLabels {
    pub run: Vec<VoiceLabel>,
    #[serde(rename = "s&p")]
    pub search_and_pounce: Vec<VoiceLabel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VoiceLabel {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceMessageEntry {
    pub mode: String,
    pub key: String,
    pub label: String,
    pub file_path: Option<String>,
    pub action: Option<String>,
}

#[derive(Debug, Clone)]
struct VoiceMessage {
    key: String,
    label: String,
    target: String,
}

#[derive(Debug, Default)]
struct VoiceMessages {
    run: Vec<VoiceMessage>,
    search_and_pounce: Vec<VoiceMessage>,
}

pub fn labels(config: &str) -> VoiceLabels {
    let messages = parse_messages(config);
    VoiceLabels {
        run: labels_for(messages.run),
        search_and_pounce: labels_for(messages.search_and_pounce),
    }
}

pub fn validate(config: &str) -> Result<VoiceLabels, String> {
    let mut current_mode = None::<&str>;
    let mut run_keys = HashSet::new();
    let mut search_and_pounce_keys = HashSet::new();

    for (index, raw_line) in config.lines().enumerate() {
        let line_number = index + 1;
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

        let mode = current_mode
            .ok_or_else(|| format!("line {line_number}: message is outside a mode section"))?;
        let message = parse_message_line(line)
            .ok_or_else(|| format!("line {line_number}: expected 'F# Label,File'"))?;
        if !is_valid_function_key(&message.key) {
            return Err(format!(
                "line {line_number}: message key must be F1 through F12"
            ));
        }
        if let Some(file_path) = file_path_from_target(&message.target) {
            validate_voice_file_path(&file_path)
                .map_err(|error| format!("line {line_number}: {error}"))?;
        }

        let keys = if mode == "run" {
            &mut run_keys
        } else {
            &mut search_and_pounce_keys
        };
        if !keys.insert(message.key.clone()) {
            return Err(format!(
                "line {line_number}: duplicate {} message in {mode} section",
                message.key
            ));
        }
    }

    if run_keys.is_empty() {
        return Err("Voice messages must include at least one Run message".to_string());
    }
    if search_and_pounce_keys.is_empty() {
        return Err("Voice messages must include at least one S&P message".to_string());
    }

    Ok(labels(config))
}

#[allow(dead_code)]
pub fn validate_with_voicekeyer_dir(
    config: &str,
    voicekeyer_dir: &Path,
) -> Result<VoiceLabels, String> {
    let labels = validate(config)?;
    for entry in entries(config) {
        let Some(file_path) = entry.file_path.as_deref() else {
            continue;
        };
        let path = voicekeyer_file_path(voicekeyer_dir, file_path)?;
        if !path.is_file() {
            return Err(format!(
                "{} {} voice file not found under voicekeyer/: {}",
                mode_label(&entry.mode),
                entry.key,
                file_path
            ));
        }
    }
    Ok(labels)
}

pub fn entries(config: &str) -> Vec<VoiceMessageEntry> {
    let messages = parse_messages(config);
    let mut entries = Vec::new();
    entries.extend(entries_for_mode("run", messages.run));
    entries.extend(entries_for_mode("s&p", messages.search_and_pounce));
    entries
}

pub fn file_path_for(config: &str, mode: &str, key: &str) -> Option<String> {
    let normalized_mode = normalize_message_mode(mode);
    let normalized_key = key.trim().to_uppercase();
    entries(config)
        .into_iter()
        .find(|entry| entry.mode == normalized_mode && entry.key == normalized_key)
        .and_then(|entry| entry.file_path)
}

pub fn normalize_message_mode(mode: &str) -> &'static str {
    match mode.trim().to_lowercase().as_str() {
        "run" => "run",
        _ => "s&p",
    }
}

pub fn voicekeyer_file_path(voicekeyer_dir: &Path, relative_path: &str) -> Result<PathBuf, String> {
    validate_voice_file_path(relative_path)?;
    Ok(voicekeyer_dir.join(relative_path.trim()))
}

fn entries_for_mode(mode: &str, messages: Vec<VoiceMessage>) -> Vec<VoiceMessageEntry> {
    messages
        .into_iter()
        .map(|message| {
            let action = action_from_template(&message.target);
            let file_path = if action.is_some() {
                None
            } else {
                file_path_from_target(&message.target)
            };
            VoiceMessageEntry {
                mode: mode.to_string(),
                key: message.key,
                label: message.label,
                file_path,
                action,
            }
        })
        .collect()
}

fn labels_for(messages: Vec<VoiceMessage>) -> Vec<VoiceLabel> {
    messages
        .into_iter()
        .map(|message| VoiceLabel {
            key: message.key,
            label: message.label,
        })
        .collect()
}

fn parse_messages(config: &str) -> VoiceMessages {
    let mut messages = VoiceMessages::default();
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

fn parse_message_line(line: &str) -> Option<VoiceMessage> {
    let (key_and_label, target) = line.split_once(',')?;
    let mut parts = key_and_label.splitn(2, char::is_whitespace);
    let key = parts.next()?.trim();
    let label = parts.next().unwrap_or("").trim();
    let normalized_key = key.to_uppercase();
    if !normalized_key.starts_with('F') {
        return None;
    }

    Some(VoiceMessage {
        key: normalized_key,
        label: label.to_string(),
        target: target.trim().to_string(),
    })
}

fn is_valid_function_key(key: &str) -> bool {
    matches!(
        key.trim().to_uppercase().as_str(),
        "F1" | "F2" | "F3" | "F4" | "F5" | "F6" | "F7" | "F8" | "F9" | "F10" | "F11" | "F12"
    )
}

fn action_from_template(template: &str) -> Option<String> {
    let inner = template.trim().strip_prefix('{')?.strip_suffix('}')?.trim();
    let (name, value) = inner.split_once(':')?;
    if name.trim().eq_ignore_ascii_case("action") {
        Some(value.trim().to_string())
    } else {
        None
    }
}

fn file_path_from_target(target: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() || action_from_template(target).is_some() {
        None
    } else {
        Some(target.to_string())
    }
}

fn validate_voice_file_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("voice message file path is required".to_string());
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err("voice message file path must be relative to voicekeyer/".to_string());
    }

    let mut has_normal_component = false;
    for component in path.components() {
        match component {
            Component::Normal(_) => has_normal_component = true,
            Component::ParentDir => {
                return Err("voice message file path cannot contain '..'".to_string());
            }
            Component::CurDir => {
                return Err("voice message file path cannot contain '.'".to_string());
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("voice message file path must be relative to voicekeyer/".to_string());
            }
        }
    }

    if !has_normal_component {
        return Err("voice message file path is required".to_string());
    }

    let is_wav = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("wav"))
        .unwrap_or(false);
    if !is_wav {
        return Err("voice message file must be a .wav file".to_string());
    }

    Ok(())
}

#[allow(dead_code)]
fn mode_label(mode: &str) -> &'static str {
    if mode == "run" { "Run" } else { "S&P" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const TEST_MESSAGES: &str = r#"
# RUN Messages
F1 CQ,operator1/CQ.wav
F12 Clear,{Action:Clear}
# S&P Messages
F1 QRL?,operator1/QRL.wav
F2 -,
"#;

    #[test]
    fn parses_voice_labels_by_mode() {
        let labels = labels(TEST_MESSAGES);

        assert_eq!(labels.run.len(), 2);
        assert_eq!(labels.run[0].key, "F1");
        assert_eq!(labels.run[0].label, "CQ");
        assert_eq!(labels.search_and_pounce.len(), 2);
        assert_eq!(labels.search_and_pounce[0].key, "F1");
        assert_eq!(labels.search_and_pounce[0].label, "QRL?");
    }

    #[test]
    fn validates_sensible_voice_messages() {
        let labels = validate(TEST_MESSAGES).expect("messages should validate");
        assert_eq!(labels.run.len(), 2);
        assert_eq!(labels.search_and_pounce.len(), 2);
    }

    #[test]
    fn rejects_invalid_voice_messages() {
        assert!(validate("F1 CQ,CQ.wav").is_err());
        assert!(validate("# RUN Messages\nF13 Bad,bad.wav\n# S&P Messages\nF1 Ok,ok.wav").is_err());
        assert!(validate("# RUN Messages\nF1 CQ,cq.wav\n# S&P Messages").is_err());
        assert!(
            validate(
                "# RUN Messages\nF1 CQ,cq.wav\nF1 Again,cq2.wav\n# S&P Messages\nF1 Ok,ok.wav"
            )
            .is_err()
        );
        assert!(
            validate("# RUN Messages\nF1 Bad,../bad.wav\n# S&P Messages\nF1 Ok,ok.wav").is_err()
        );
        assert!(validate("# RUN Messages\nF1 Bad,bad.mp3\n# S&P Messages\nF1 Ok,ok.wav").is_err());
    }

    #[test]
    fn returns_file_path_by_mode_and_key() {
        assert_eq!(
            file_path_for(TEST_MESSAGES, "run", "f1"),
            Some("operator1/CQ.wav".to_string())
        );
        assert_eq!(file_path_for(TEST_MESSAGES, "run", "F12"), None);
        assert_eq!(file_path_for(TEST_MESSAGES, "s&p", "F2"), None);
    }

    #[test]
    fn validate_with_voicekeyer_dir_checks_referenced_files() {
        let root =
            std::env::temp_dir().join(format!("log73-voice-messages-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("operator1")).expect("voicekeyer dir creates");
        fs::write(root.join("operator1/CQ.wav"), b"audio").expect("cq file writes");
        fs::write(root.join("operator1/QRL.wav"), b"audio").expect("qrl file writes");

        validate_with_voicekeyer_dir(TEST_MESSAGES, &root).expect("files exist");

        fs::remove_file(root.join("operator1/QRL.wav")).expect("qrl removed");
        let error =
            validate_with_voicekeyer_dir(TEST_MESSAGES, &root).expect_err("missing file fails");
        assert!(error.contains("QRL.wav"));

        let _ = fs::remove_dir_all(&root);
    }
}
