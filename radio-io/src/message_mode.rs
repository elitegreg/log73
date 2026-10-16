pub const RUN_MESSAGE_MODE: &str = "run";
pub const SEARCH_AND_POUNCE_MESSAGE_MODE: &str = "s&p";

pub fn normalize_message_mode(mode: &str) -> &'static str {
    match mode.trim().to_lowercase().as_str() {
        "run" => RUN_MESSAGE_MODE,
        "s&p" | "sp" | "search_and_pounce" | "search and pounce" => SEARCH_AND_POUNCE_MESSAGE_MODE,
        _ => SEARCH_AND_POUNCE_MESSAGE_MODE,
    }
}

pub fn is_valid_message_mode(mode: &str) -> bool {
    matches!(
        mode.trim().to_lowercase().as_str(),
        "run" | "s&p" | "sp" | "search_and_pounce" | "search and pounce"
    )
}

pub fn parse_message_mode_section_header(line: &str) -> Option<&'static str> {
    let upper = line.trim().to_uppercase();
    if upper.contains("RUN MESSAGES") {
        return Some(RUN_MESSAGE_MODE);
    }
    if upper.contains("S&P MESSAGES")
        || upper.contains("SP MESSAGES")
        || upper.contains("SEARCH AND POUNCE MESSAGES")
    {
        return Some(SEARCH_AND_POUNCE_MESSAGE_MODE);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Fixture {
        version: u32,
        normalization_cases: Vec<NormalizationCase>,
        header_cases: Vec<HeaderCase>,
    }

    #[derive(serde::Deserialize)]
    struct NormalizationCase {
        name: String,
        input: Option<String>,
        normalized: String,
    }

    #[derive(serde::Deserialize)]
    struct HeaderCase {
        name: String,
        input: Option<String>,
        mode: Option<String>,
    }

    #[test]
    fn shared_message_mode_contract() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/domain/message-modes.json"
        ))
        .expect("valid message mode fixture");
        assert_eq!(fixture.version, 1);
        assert!(!fixture.normalization_cases.is_empty());
        assert!(!fixture.header_cases.is_empty());
        for case in fixture.normalization_cases {
            assert_eq!(
                normalize_message_mode(case.input.as_deref().unwrap_or_default()),
                case.normalized,
                "{}",
                case.name
            );
        }
        for case in fixture.header_cases {
            assert_eq!(
                parse_message_mode_section_header(case.input.as_deref().unwrap_or_default()),
                case.mode.as_deref(),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn validates_message_mode_aliases_without_accepting_fallbacks() {
        for mode in [
            "run",
            " S&P ",
            "SP",
            "search_and_pounce",
            "search and pounce",
        ] {
            assert!(is_valid_message_mode(mode), "{mode}");
        }
        for mode in ["", "unknown"] {
            assert!(!is_valid_message_mode(mode), "{mode}");
        }
    }
}
