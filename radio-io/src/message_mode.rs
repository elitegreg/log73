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

    #[test]
    fn normalizes_search_and_pounce_aliases() {
        assert_eq!(normalize_message_mode("sp"), SEARCH_AND_POUNCE_MESSAGE_MODE);
        assert_eq!(
            normalize_message_mode("search_and_pounce"),
            SEARCH_AND_POUNCE_MESSAGE_MODE
        );
        assert_eq!(
            normalize_message_mode("search and pounce"),
            SEARCH_AND_POUNCE_MESSAGE_MODE
        );
    }

    #[test]
    fn parses_message_mode_section_headers() {
        assert_eq!(
            parse_message_mode_section_header("# RUN Messages"),
            Some("run")
        );
        assert_eq!(
            parse_message_mode_section_header("# S&P Messages"),
            Some("s&p")
        );
        assert_eq!(
            parse_message_mode_section_header("# Search and Pounce Messages"),
            Some("s&p")
        );
    }
}
