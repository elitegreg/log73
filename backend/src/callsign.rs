//! Shared callsign parsing contract.
//!
//! This module is the Rust half of the portable-callsign contract; its mirror
//! is `src/domain/callsign.js`. Keep the two implementations behaviorally
//! identical. The contract is locked by `tests/fixtures/domain/callsigns.json`,
//! which both languages execute directly (see `tests/fixtures/domain/README.md`).
//!
//! Three outputs are deliberately separate:
//!
//! - `base`: station identity, used for validation and duplicate detection.
//! - WPX prefix (`crate::dxcc::callsign_prefix`): operating-prefix extraction.
//! - DXCC entity (`crate::dxcc::DxccDatabase::lookup`): operating geography.
//!
//! A portable contact such as `VP2M/K1ABC` has dupe identity `K1ABC` but WPX
//! prefix `VP2`; that disagreement is intended.

/// Normalized input plus resolved slash components.
///
/// - `normalized`: trimmed, uppercased input.
/// - `base`: station identity component, or `None` when components cannot be
///   resolved (bad slash structure, or neither component looks like a callsign).
/// - `modifier`: the other slash component (operating prefix or suffix), or
///   `None` when there is no slash or no resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallsignParts {
    pub normalized: String,
    pub base: Option<String>,
    pub modifier: Option<String>,
}

/// Structural validation failure for a normalized callsign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureError {
    InvalidCharacter,
    TooManySlashes,
    EdgeSlash,
}

/// Trim surrounding whitespace and uppercase the input.
pub fn normalize(input: &str) -> String {
    input.trim().to_uppercase()
}

/// Report whether a single component looks like a callsign: a digit in the
/// 2nd or 3rd character. Callers pass normalized components.
pub fn is_callsign_like(component: &str) -> bool {
    let mut characters = component.chars();
    let second = characters.nth(1).unwrap_or_default();
    let third = characters.next().unwrap_or_default();
    second.is_ascii_digit() || third.is_ascii_digit()
}

/// Check charset and slash placement of a normalized callsign.
///
/// Query mode additionally permits `?` wildcards. Length and required-ness are
/// the caller's field-specific validation, not part of this check.
pub fn check_structure(normalized: &str, allow_query: bool) -> Result<(), StructureError> {
    let valid_charset = normalized.chars().all(|character| {
        character.is_ascii_uppercase()
            || character.is_ascii_digit()
            || character == '/'
            || (allow_query && character == '?')
    });
    if !valid_charset {
        return Err(StructureError::InvalidCharacter);
    }
    if normalized
        .chars()
        .filter(|character| *character == '/')
        .count()
        > 1
    {
        return Err(StructureError::TooManySlashes);
    }
    if normalized.starts_with('/') || normalized.ends_with('/') {
        return Err(StructureError::EdgeSlash);
    }
    Ok(())
}

/// Resolve the normalized input into base identity and operating modifier.
pub fn parse(input: &str) -> CallsignParts {
    let normalized = normalize(input);
    let (base, modifier) = match split_middle_slash(&normalized) {
        Slash::None => (Some(normalized.clone()), None),
        Slash::Pair(left, right) => resolve_pair(left, right),
        Slash::Invalid => (None, None),
    };
    CallsignParts {
        normalized,
        base,
        modifier,
    }
}

/// Duplicate identity for any input, including legacy or forced values that the
/// strict logging parser cannot parse: the resolved base when available, else
/// the normalized full input so distinct values never collide.
pub fn dupe_identity(input: &str) -> String {
    let parts = parse(input);
    parts.base.unwrap_or(parts.normalized)
}

enum Slash<'a> {
    None,
    Pair(&'a str, &'a str),
    Invalid,
}

fn split_middle_slash(normalized: &str) -> Slash<'_> {
    let mut components = normalized.split('/');
    let Some(first) = components.next() else {
        return Slash::Invalid;
    };
    let Some(second) = components.next() else {
        return Slash::None;
    };
    if components.next().is_some() || first.is_empty() || second.is_empty() {
        return Slash::Invalid;
    }
    Slash::Pair(first, second)
}

fn resolve_pair(left: &str, right: &str) -> (Option<String>, Option<String>) {
    match (is_callsign_like(left), is_callsign_like(right)) {
        // Operating prefix form: DL/W1AW identifies station W1AW.
        (false, true) => (Some(right.to_string()), Some(left.to_string())),
        // Operating suffix form: K1ABC/P, W9ABC/4, NG4M/XYZ identify the left.
        (true, false) => (Some(left.to_string()), Some(right.to_string())),
        // Ambiguous: both components look like callsigns (VP2M/K1ABC,
        // K1ABC/VE3). The longer component is the station identity; ties
        // resolve to the right so W1AW/VE3 and VE3/W1AW agree.
        (true, true) if left.chars().count() > right.chars().count() => {
            (Some(left.to_string()), Some(right.to_string()))
        }
        (true, true) => (Some(right.to_string()), Some(left.to_string())),
        (false, false) => (None, None),
    }
}

/// One row of `tests/fixtures/domain/callsigns.json`. See
/// `tests/fixtures/domain/README.md` for the format contract.
#[cfg(test)]
#[derive(Debug, serde::Deserialize)]
pub struct FixtureCase {
    pub input: String,
    pub logging_valid: bool,
    pub query_valid: bool,
    pub base: Option<String>,
    pub modifier: Option<String>,
    pub dupe_identity: String,
    pub wpx_prefix: Option<String>,
    pub dxcc: Option<FixtureDxcc>,
}

/// Expected DXCC lookup result within the fixture's embedded database.
#[cfg(test)]
#[derive(Debug, serde::Deserialize)]
pub struct FixtureDxcc {
    pub adif: u16,
    pub primary_prefix: String,
}

/// Parsed `tests/fixtures/domain/callsigns.json`, shared by the Rust harnesses
/// in `callsign`, `validation`, `scoring`, and `dxcc` tests.
#[cfg(test)]
#[derive(Debug, serde::Deserialize)]
pub struct FixtureFile {
    pub version: u32,
    pub dxcc_db: crate::dxcc::DxccDatabase,
    pub cases: Vec<FixtureCase>,
}

#[cfg(test)]
pub fn load_fixture() -> FixtureFile {
    let text = include_str!("../../tests/fixtures/domain/callsigns.json");
    serde_json::from_str(text).expect("callsign fixture should parse")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_fixture_matches_base_modifier_and_identity() {
        let fixture = load_fixture();
        assert_eq!(fixture.version, 1);
        assert!(!fixture.cases.is_empty());
        for case in &fixture.cases {
            let parts = parse(&case.input);
            assert_eq!(parts.base, case.base, "base for {:?}", case.input);
            assert_eq!(
                parts.modifier, case.modifier,
                "modifier for {:?}",
                case.input
            );
            assert_eq!(
                dupe_identity(&case.input),
                case.dupe_identity,
                "identity for {:?}",
                case.input
            );
        }
    }

    #[test]
    fn resolves_single_callsigns_without_modifier() {
        assert_eq!(
            parse("K1ABC"),
            CallsignParts {
                normalized: "K1ABC".to_string(),
                base: Some("K1ABC".to_string()),
                modifier: None,
            }
        );
    }

    #[test]
    fn normalizes_whitespace_and_case_before_resolving() {
        let parts = parse("  dl/w1aw  ");
        assert_eq!(parts.normalized, "DL/W1AW");
        assert_eq!(parts.base.as_deref(), Some("W1AW"));
        assert_eq!(parts.modifier.as_deref(), Some("DL"));
    }

    #[test]
    fn resolves_prefix_form_to_home_station() {
        for input in ["DL/W1AW", "F/W1AW", "PA/W1AW", "EA/W1AW", "G/W1AW"] {
            let parts = parse(input);
            assert_eq!(parts.base.as_deref(), Some("W1AW"), "base for {input}");
            assert_eq!(dupe_identity(input), "W1AW");
        }
    }

    #[test]
    fn resolves_suffix_form_to_left_component() {
        for (input, base) in [
            ("K1ABC/P", "K1ABC"),
            ("N0CALL/M", "N0CALL"),
            ("W3AAA/MM", "W3AAA"),
            ("K1ABC/QRP", "K1ABC"),
            ("W9ABC/4", "W9ABC"),
            ("NG4M/1", "NG4M"),
            ("NG4M/XYZ", "NG4M"),
        ] {
            let parts = parse(input);
            assert_eq!(parts.base.as_deref(), Some(base), "base for {input}");
            assert_eq!(dupe_identity(input), base);
        }
    }

    #[test]
    fn resolves_ambiguous_pairs_by_length_with_right_tiebreak() {
        assert_eq!(parse("VP2M/K1ABC").base.as_deref(), Some("K1ABC"));
        assert_eq!(parse("K1ABC/VE3").base.as_deref(), Some("K1ABC"));
        assert_eq!(parse("KL7RA/WK9").base.as_deref(), Some("KL7RA"));
        assert_eq!(parse("EA8/K1A").base.as_deref(), Some("K1A"));
        // Order-independent: both slash orders identify the same station.
        assert_eq!(parse("W1AW/VE3").base, parse("VE3/W1AW").base);
    }

    #[test]
    fn distinct_operators_sharing_a_prefix_never_collide() {
        assert_ne!(dupe_identity("VE3/K1ABC"), dupe_identity("VE3/N2XYZ"));
        assert_eq!(dupe_identity("VE3/K1ABC"), "K1ABC");
        assert_eq!(dupe_identity("VE3/N2XYZ"), "N2XYZ");
    }

    #[test]
    fn falls_back_to_full_input_when_unresolvable() {
        for input in ["", "AB/CD", "/K1ABC", "K1ABC/", "K1/A/BC", "A/B/C"] {
            let parts = parse(input);
            if input.is_empty() {
                assert_eq!(parts.base.as_deref(), Some(""));
            } else {
                assert_eq!(parts.base, None, "base for {input}");
            }
            assert_eq!(dupe_identity(input), normalize(input));
        }
    }

    #[test]
    fn checks_charset_and_slash_placement() {
        assert_eq!(check_structure("K1ABC", false), Ok(()));
        assert_eq!(check_structure("DL/W1AW", false), Ok(()));
        assert_eq!(
            check_structure("K1A*", false),
            Err(StructureError::InvalidCharacter)
        );
        assert_eq!(
            check_structure("W1?", false),
            Err(StructureError::InvalidCharacter)
        );
        assert_eq!(check_structure("W1?", true), Ok(()));
        assert_eq!(
            check_structure("K1/A/BC", false),
            Err(StructureError::TooManySlashes)
        );
        assert_eq!(
            check_structure("/K1ABC", false),
            Err(StructureError::EdgeSlash)
        );
        assert_eq!(
            check_structure("K1ABC/", false),
            Err(StructureError::EdgeSlash)
        );
    }
}
