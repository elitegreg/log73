use serde::Serialize;
use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

#[derive(Clone, Debug, Default, Serialize)]
pub struct DxccDatabase {
    pub entities: Vec<DxccEntity>,
    pub rules: Vec<DxccRule>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DxccEntity {
    pub country_name: String,
    pub cq_zone: u8,
    pub itu_zone: u8,
    pub continent: String,
    pub latitude: f64,
    pub longitude: f64,
    pub utc_offset: f64,
    pub primary_prefix: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DxccRule {
    pub pattern: String,
    pub exact: bool,
    pub entity_index: usize,
    pub cq_zone: Option<u8>,
    pub itu_zone: Option<u8>,
    pub continent: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub utc_offset: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DxccInfo {
    pub country_name: String,
    pub cq_zone: u8,
    pub itu_zone: u8,
    pub continent: String,
    pub latitude: f64,
    pub longitude: f64,
    pub utc_offset: f64,
    pub primary_prefix: String,
}

impl DxccDatabase {
    pub fn load_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let file = File::open(path.as_ref())?;
        let reader = BufReader::new(file);
        let mut text = String::new();

        for line in reader.lines() {
            let line = line?;
            text.push_str(&line);
            text.push('\n');
        }

        Self::from_str(&text).map_err(std::io::Error::other)
    }

    pub fn from_str(text: &str) -> Result<Self, String> {
        let mut database = Self::default();
        let mut lines = text.lines().peekable();
        let mut seen_exact_callsigns = HashSet::new();

        while let Some(raw_line) = lines.next() {
            let line = raw_line.trim_end();
            if line.trim().is_empty() {
                continue;
            }

            let entity = parse_entity(line)?;
            let entity_index = database.entities.len();
            database.entities.push(entity);

            let mut aliases = String::new();
            loop {
                let Some(alias_line) = lines.next() else {
                    return Err("unexpected end of file while reading alias list".to_string());
                };
                let trimmed = alias_line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                aliases.push_str(trimmed);
                if trimmed.ends_with(';') {
                    break;
                }
            }

            for token in aliases.split(',') {
                let token = token.trim().trim_end_matches(';').trim();
                if token.is_empty() {
                    continue;
                }
                let rule = parse_rule(token, entity_index)?;
                if rule.exact && !seen_exact_callsigns.insert(rule.pattern.clone()) {
                    continue;
                }
                database.rules.push(rule);
            }
        }

        Ok(database)
    }

    #[allow(dead_code)]
    pub fn lookup(&self, callsign: &str) -> Option<DxccInfo> {
        // Keep this slash-callsign DXCC resolution logic in sync with
        // src/domain/dxcc.js when changing either side.
        let normalized_callsign = normalize_callsign(callsign);
        if normalized_callsign.is_empty() {
            return None;
        }

        let Some((left, right)) = split_slash_callsign(&normalized_callsign) else {
            return self.lookup_direct(&normalized_callsign);
        };

        if left.len() < right.len() {
            return self.lookup_direct(left);
        }

        if is_ignored_slash_suffix(right) {
            return self.lookup_direct(left);
        }

        self.lookup_direct(right)
            .or_else(|| self.lookup_direct(left))
    }

    fn lookup_direct(&self, callsign: &str) -> Option<DxccInfo> {
        let normalized_callsign = normalize_callsign(callsign);
        callsign_prefix(&normalized_callsign)?;

        if let Some(rule) = self
            .rules
            .iter()
            .find(|rule| rule.exact && rule.pattern == normalized_callsign)
        {
            return Some(self.info_for_rule(rule));
        }

        let best_rule = self
            .rules
            .iter()
            .filter(|rule| !rule.exact && normalized_callsign.starts_with(&rule.pattern))
            .max_by_key(|rule| rule.pattern.len())?;

        Some(self.info_for_rule(best_rule))
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    #[allow(dead_code)]
    fn info_for_rule(&self, rule: &DxccRule) -> DxccInfo {
        let entity = &self.entities[rule.entity_index];
        DxccInfo {
            country_name: entity.country_name.clone(),
            cq_zone: rule.cq_zone.unwrap_or(entity.cq_zone),
            itu_zone: rule.itu_zone.unwrap_or(entity.itu_zone),
            continent: rule
                .continent
                .clone()
                .unwrap_or_else(|| entity.continent.clone()),
            latitude: rule.latitude.unwrap_or(entity.latitude),
            longitude: rule.longitude.unwrap_or(entity.longitude),
            utc_offset: rule.utc_offset.unwrap_or(entity.utc_offset),
            primary_prefix: entity.primary_prefix.clone(),
        }
    }
}

pub fn callsign_prefix(callsign: &str) -> Option<String> {
    let normalized = normalize_callsign(callsign);
    if normalized.is_empty() {
        return None;
    }

    let chars = normalized.chars().collect::<Vec<_>>();
    if chars.first()?.is_ascii_digit() {
        let second_digit_index = chars
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(index, character)| character.is_ascii_digit().then_some(index))?;
        Some(chars[..second_digit_index].iter().collect())
    } else {
        let first_digit_index = chars
            .iter()
            .enumerate()
            .find_map(|(index, character)| character.is_ascii_digit().then_some(index))?;
        (first_digit_index > 0).then(|| chars[..first_digit_index].iter().collect())
    }
}

fn normalize_callsign(callsign: &str) -> String {
    callsign.trim().to_uppercase()
}

fn split_slash_callsign(callsign: &str) -> Option<(&str, &str)> {
    let slash_index = callsign.find('/')?;
    if slash_index == 0 || slash_index != callsign.rfind('/')? || slash_index >= callsign.len() - 1
    {
        return None;
    }

    Some((&callsign[..slash_index], &callsign[slash_index + 1..]))
}

fn is_ignored_slash_suffix(part: &str) -> bool {
    matches!(part, "M" | "P" | "MM" | "QRP")
        || (part.len() == 1 && part.chars().all(|character| character.is_ascii_digit()))
}

fn parse_entity(line: &str) -> Result<DxccEntity, String> {
    let fields = line.split(':').collect::<Vec<_>>();
    if fields.len() < 8 {
        return Err(format!("invalid CTY entity line: {line}"));
    }

    Ok(DxccEntity {
        country_name: fields[0].trim().to_string(),
        cq_zone: parse_u8_field(fields[1], "CQ zone")?,
        itu_zone: parse_u8_field(fields[2], "ITU zone")?,
        continent: fields[3].trim().to_string(),
        latitude: parse_f64_field(fields[4], "latitude")?,
        longitude: parse_f64_field(fields[5], "longitude")?,
        utc_offset: parse_f64_field(fields[6], "UTC offset")?,
        primary_prefix: fields[7].trim().trim_start_matches('*').to_string(),
    })
}

fn parse_rule(token: &str, entity_index: usize) -> Result<DxccRule, String> {
    let mut rest = token.trim();
    let exact = rest.starts_with('=');
    if exact {
        rest = &rest[1..];
    }

    let pattern_end = rest.find(['(', '[', '<', '{', '~']).unwrap_or(rest.len());
    let pattern = rest[..pattern_end].trim().to_uppercase();
    let mut rule = DxccRule {
        pattern,
        exact,
        entity_index,
        cq_zone: None,
        itu_zone: None,
        continent: None,
        latitude: None,
        longitude: None,
        utc_offset: None,
    };

    let mut modifiers = &rest[pattern_end..];
    while !modifiers.is_empty() {
        if let Some(remaining) = parse_wrapped_modifier(modifiers, '(', ')', |value| {
            rule.cq_zone = Some(parse_u8_field(value, "override CQ zone")?);
            Ok(())
        })? {
            modifiers = remaining;
            continue;
        }
        if let Some(remaining) = parse_wrapped_modifier(modifiers, '[', ']', |value| {
            rule.itu_zone = Some(parse_u8_field(value, "override ITU zone")?);
            Ok(())
        })? {
            modifiers = remaining;
            continue;
        }
        if let Some(remaining) = parse_wrapped_modifier(modifiers, '{', '}', |value| {
            rule.continent = Some(value.trim().to_string());
            Ok(())
        })? {
            modifiers = remaining;
            continue;
        }
        if let Some(remaining) = parse_wrapped_modifier(modifiers, '~', '~', |value| {
            rule.utc_offset = Some(parse_f64_field(value, "override UTC offset")?);
            Ok(())
        })? {
            modifiers = remaining;
            continue;
        }
        if let Some(remaining) = parse_wrapped_modifier(modifiers, '<', '>', |value| {
            let (latitude, longitude) = value
                .split_once('/')
                .ok_or_else(|| format!("invalid latitude/longitude override: <{value}>"))?;
            rule.latitude = Some(parse_f64_field(latitude, "override latitude")?);
            rule.longitude = Some(parse_f64_field(longitude, "override longitude")?);
            Ok(())
        })? {
            modifiers = remaining;
            continue;
        }

        return Err(format!("invalid CTY modifier sequence: {token}"));
    }

    if rule.pattern.is_empty() {
        return Err(format!("invalid CTY alias token: {token}"));
    }

    Ok(rule)
}

fn parse_wrapped_modifier<F>(
    input: &str,
    open: char,
    close: char,
    mut apply: F,
) -> Result<Option<&str>, String>
where
    F: FnMut(&str) -> Result<(), String>,
{
    let Some(rest) = input.strip_prefix(open) else {
        return Ok(None);
    };
    let end_index = rest
        .find(close)
        .ok_or_else(|| format!("unterminated CTY modifier: {input}"))?;
    let value = &rest[..end_index];
    apply(value)?;
    Ok(Some(&rest[end_index + close.len_utf8()..]))
}

fn parse_u8_field(value: &str, label: &str) -> Result<u8, String> {
    value
        .trim()
        .parse::<u8>()
        .map_err(|error| format!("invalid {label}: {error}"))
}

fn parse_f64_field(value: &str, label: &str) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|error| format!("invalid {label}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CTY: &str = r#"
Testland:                 10:  20:  EU:   50.00:   -10.00:    -1.0:  T1:
    T1,TA(11)[21]<51.0/11.0>{AF}~2.0~,=T1ABC;
Otherland:                12:  22:  NA:   40.00:    70.00:     5.0:  4O:
    4O;
Canada:                   4:   9:   NA:   56.00:    96.00:     5.0:  VE3:
    VE3;
United States:            5:   8:   NA:   38.00:    97.00:     5.0:  K:
    K,N,W;
"#;

    #[test]
    fn parses_cty_database() {
        let database = DxccDatabase::from_str(SAMPLE_CTY).expect("sample CTY should parse");

        assert_eq!(database.entity_count(), 4);
        assert!(database.rule_count() >= 6);
        assert_eq!(database.entities[0].country_name, "Testland");
        assert_eq!(database.entities[1].primary_prefix, "4O");
    }

    #[test]
    fn lookup_prefers_exact_match_then_longest_prefix() {
        let database = DxccDatabase::from_str(SAMPLE_CTY).expect("sample CTY should parse");

        assert_eq!(
            database
                .lookup("T1ABC")
                .expect("exact match should resolve"),
            DxccInfo {
                country_name: "Testland".to_string(),
                cq_zone: 10,
                itu_zone: 20,
                continent: "EU".to_string(),
                latitude: 50.0,
                longitude: -10.0,
                utc_offset: -1.0,
                primary_prefix: "T1".to_string(),
            }
        );

        assert_eq!(
            database
                .lookup("TA9ZZ")
                .expect("prefix match should resolve"),
            DxccInfo {
                country_name: "Testland".to_string(),
                cq_zone: 11,
                itu_zone: 21,
                continent: "AF".to_string(),
                latitude: 51.0,
                longitude: 11.0,
                utc_offset: 2.0,
                primary_prefix: "T1".to_string(),
            }
        );
    }

    #[test]
    fn callsign_prefix_uses_digit_rules() {
        assert_eq!(callsign_prefix("KP2M"), Some("KP".to_string()));
        assert_eq!(callsign_prefix("4O9A"), Some("4O".to_string()));
        assert_eq!(callsign_prefix("K"), None);
        assert_eq!(callsign_prefix("KP"), None);
        assert_eq!(callsign_prefix("4O"), None);
    }

    #[test]
    fn lookup_resolves_slash_prefixed_and_slash_suffixed_dxccs() {
        let database = DxccDatabase::from_str(SAMPLE_CTY).expect("sample CTY should parse");

        assert_eq!(
            database
                .lookup("VE3/NG4M")
                .expect("slash prefix should resolve"),
            DxccInfo {
                country_name: "Canada".to_string(),
                cq_zone: 4,
                itu_zone: 9,
                continent: "NA".to_string(),
                latitude: 56.0,
                longitude: 96.0,
                utc_offset: 5.0,
                primary_prefix: "VE3".to_string(),
            }
        );

        assert_eq!(
            database
                .lookup("NG4M/VE3")
                .expect("slash suffix DXCC should resolve"),
            DxccInfo {
                country_name: "Canada".to_string(),
                cq_zone: 4,
                itu_zone: 9,
                continent: "NA".to_string(),
                latitude: 56.0,
                longitude: 96.0,
                utc_offset: 5.0,
                primary_prefix: "VE3".to_string(),
            }
        );
    }

    #[test]
    fn lookup_ignores_common_suffixes_and_falls_back_to_root_callsign() {
        let database = DxccDatabase::from_str(SAMPLE_CTY).expect("sample CTY should parse");
        let united_states = DxccInfo {
            country_name: "United States".to_string(),
            cq_zone: 5,
            itu_zone: 8,
            continent: "NA".to_string(),
            latitude: 38.0,
            longitude: 97.0,
            utc_offset: 5.0,
            primary_prefix: "K".to_string(),
        };

        assert_eq!(database.lookup("NG4M/P"), Some(united_states.clone()));
        assert_eq!(database.lookup("NG4M/MM"), Some(united_states.clone()));
        assert_eq!(database.lookup("NG4M/QRP"), Some(united_states.clone()));
        assert_eq!(database.lookup("NG4M/1"), Some(united_states.clone()));
        assert_eq!(database.lookup("NG4M/XYZ"), Some(united_states));
    }
}
