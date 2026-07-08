use csv::{ReaderBuilder, StringRecord};
use serde::Serialize;
use std::{fs::File, io::Read, path::Path};

#[derive(Clone, Debug, Default, Serialize)]
pub struct DxccDatabase {
    pub entities: Vec<DxccEntity>,
    pub rules: Vec<DxccRule>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DxccEntity {
    pub country_name: String,
    pub adif: u16,
    pub cq_zone: u8,
    pub itu_zone: u8,
    pub continent: String,
    pub latitude: f64,
    pub longitude: f64,
    pub utc_offset: f64,
    pub primary_prefix: String,
    pub waedc_cq_list: bool,
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
    pub adif: u16,
    pub cq_zone: u8,
    pub itu_zone: u8,
    pub continent: String,
    pub latitude: f64,
    pub longitude: f64,
    pub utc_offset: f64,
    pub primary_prefix: String,
    pub waedc_cq_list: bool,
}

impl DxccDatabase {
    pub fn load_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let mut file = File::open(path.as_ref())?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;

        Self::from_str(&text).map_err(std::io::Error::other)
    }

    pub fn from_str(text: &str) -> Result<Self, String> {
        let mut database = Self::default();
        let mut reader = ReaderBuilder::new()
            .has_headers(false)
            .trim(csv::Trim::All)
            .from_reader(text.as_bytes());

        for (record_index, record) in reader.records().enumerate() {
            let row_number = record_index + 1;
            let record =
                record.map_err(|error| format!("invalid CTY CSV row {row_number}: {error}"))?;
            if record.iter().all(|field| field.trim().is_empty()) {
                continue;
            }
            if record.len() != 10 {
                return Err(format!(
                    "invalid CTY CSV row {row_number}: expected 10 fields, found {}",
                    record.len()
                ));
            }

            let entity = parse_entity(&record, row_number)?;
            let entity_index = database.entities.len();
            let primary_prefix = entity.primary_prefix.clone();
            database.entities.push(entity);

            push_rule(&mut database.rules, &primary_prefix, entity_index)?;

            let aliases = record_field(&record, 9, row_number)?;
            for token in aliases.split_whitespace() {
                let token = token.trim().trim_end_matches(';').trim();
                if token.is_empty() {
                    continue;
                }
                push_rule(&mut database.rules, token, entity_index)?;
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

        if let Some(info) = self.lookup_exact(&normalized_callsign) {
            return Some(info);
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
        if let Some(info) = self.lookup_exact(&normalized_callsign) {
            return Some(info);
        }

        callsign_prefix(&normalized_callsign)?;

        let best_rule = self
            .rules
            .iter()
            .filter(|rule| !rule.exact && normalized_callsign.starts_with(&rule.pattern))
            .max_by_key(|rule| rule.pattern.len())?;

        Some(self.info_for_rule(best_rule))
    }

    fn lookup_exact(&self, normalized_callsign: &str) -> Option<DxccInfo> {
        let mut best_rule = None;
        for rule in &self.rules {
            if !rule.exact || rule.pattern != normalized_callsign {
                continue;
            }
            let is_waedc_cq = self.entities[rule.entity_index].waedc_cq_list;
            let best_is_waedc_cq = best_rule
                .map(|rule: &DxccRule| self.entities[rule.entity_index].waedc_cq_list)
                .unwrap_or(false);
            if best_rule.is_none() || (is_waedc_cq && !best_is_waedc_cq) {
                best_rule = Some(rule);
            }
        }
        best_rule.map(|rule| self.info_for_rule(rule))
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
            adif: entity.adif,
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
            waedc_cq_list: entity.waedc_cq_list,
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

fn parse_entity(record: &StringRecord, row_number: usize) -> Result<DxccEntity, String> {
    let raw_primary_prefix = record_field(record, 0, row_number)?;
    let waedc_cq_list = raw_primary_prefix.starts_with('*');
    let primary_prefix = raw_primary_prefix.trim_start_matches('*').to_string();

    Ok(DxccEntity {
        primary_prefix,
        country_name: record_field(record, 1, row_number)?.to_string(),
        adif: parse_u16_field(record_field(record, 2, row_number)?, "ADIF DXCC number")?,
        continent: record_field(record, 3, row_number)?.to_string(),
        cq_zone: parse_u8_field(record_field(record, 4, row_number)?, "CQ zone")?,
        itu_zone: parse_u8_field(record_field(record, 5, row_number)?, "ITU zone")?,
        latitude: parse_f64_field(record_field(record, 6, row_number)?, "latitude")?,
        longitude: parse_f64_field(record_field(record, 7, row_number)?, "longitude")?,
        utc_offset: parse_f64_field(record_field(record, 8, row_number)?, "UTC offset")?,
        waedc_cq_list,
    })
}

fn record_field(record: &StringRecord, index: usize, row_number: usize) -> Result<&str, String> {
    record
        .get(index)
        .map(str::trim)
        .ok_or_else(|| format!("invalid CTY CSV row {row_number}: missing field {index}"))
}

fn push_rule(rules: &mut Vec<DxccRule>, token: &str, entity_index: usize) -> Result<(), String> {
    rules.push(parse_rule(token, entity_index)?);
    Ok(())
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

fn parse_u16_field(value: &str, label: &str) -> Result<u16, String> {
    value
        .trim()
        .parse::<u16>()
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
T1,Testland,123,EU,10,20,50.00,-10.00,-1.0,T1 TA(11)[21]<51.0/11.0>{AF}~2.0~ =T1ABC;
1S,Spratly Islands,247,AS,26,50,9.88,-114.23,-8.0,9M0;
4O,Montenegro,514,EU,15,28,42.50,-19.28,-1.0,4O;
VE3,Canada,1,NA,4,9,56.00,96.00,5.0,VE3;
K,United States,291,NA,5,8,38.00,97.00,5.0,K N W =GM0AVR;
*GM/s,Shetland Islands,279,EU,14,27,60.50,1.50,0.0,=GM0AVR;
3Y/b,Bouvet,24,AF,38,67,-54.42,-3.38,-1.0,=3Y/LB5SH;
"#;

    #[test]
    fn parses_cty_csv_database() {
        let database = DxccDatabase::from_str(SAMPLE_CTY).expect("sample CTY should parse");

        assert_eq!(database.entity_count(), 7);
        assert!(database.rule_count() >= 13);
        assert_eq!(database.entities[0].country_name, "Testland");
        assert_eq!(database.entities[0].adif, 123);
        assert_eq!(database.entities[2].primary_prefix, "4O");
        assert_eq!(database.entities[5].primary_prefix, "GM/s");
        assert!(database.entities[5].waedc_cq_list);
    }

    #[test]
    fn parses_repository_cty_csv() {
        let database = DxccDatabase::from_str(include_str!("../../data/cty.csv"))
            .expect("repository CTY CSV should parse");

        assert!(database.entity_count() > 300);
        assert!(database.rule_count() > 4_000);
        assert_eq!(
            database.lookup("K1ABC").expect("K should resolve").adif,
            291
        );
        assert_eq!(
            database
                .lookup("3D2CCC")
                .expect("Conway Reef exact should resolve")
                .adif,
            489
        );
        assert!(
            database
                .lookup("4U1VIC")
                .expect("Vienna Intl Ctr exact should resolve")
                .waedc_cq_list
        );
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
                adif: 123,
                cq_zone: 10,
                itu_zone: 20,
                continent: "EU".to_string(),
                latitude: 50.0,
                longitude: -10.0,
                utc_offset: -1.0,
                primary_prefix: "T1".to_string(),
                waedc_cq_list: false,
            }
        );

        assert_eq!(
            database
                .lookup("TA9ZZ")
                .expect("prefix match should resolve"),
            DxccInfo {
                country_name: "Testland".to_string(),
                adif: 123,
                cq_zone: 11,
                itu_zone: 21,
                continent: "AF".to_string(),
                latitude: 51.0,
                longitude: 11.0,
                utc_offset: 2.0,
                primary_prefix: "T1".to_string(),
                waedc_cq_list: false,
            }
        );
    }

    #[test]
    fn primary_prefixes_are_lookup_rules_even_when_aliases_omit_them() {
        let database = DxccDatabase::from_str(SAMPLE_CTY).expect("sample CTY should parse");

        assert_eq!(
            database
                .lookup("1S1A")
                .expect("primary prefix should resolve")
                .adif,
            247
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
                adif: 1,
                cq_zone: 4,
                itu_zone: 9,
                continent: "NA".to_string(),
                latitude: 56.0,
                longitude: 96.0,
                utc_offset: 5.0,
                primary_prefix: "VE3".to_string(),
                waedc_cq_list: false,
            }
        );

        assert_eq!(
            database
                .lookup("NG4M/VE3")
                .expect("slash suffix DXCC should resolve"),
            DxccInfo {
                country_name: "Canada".to_string(),
                adif: 1,
                cq_zone: 4,
                itu_zone: 9,
                continent: "NA".to_string(),
                latitude: 56.0,
                longitude: 96.0,
                utc_offset: 5.0,
                primary_prefix: "VE3".to_string(),
                waedc_cq_list: false,
            }
        );
    }

    #[test]
    fn lookup_checks_exact_full_callsigns_before_slash_resolution() {
        let database = DxccDatabase::from_str(SAMPLE_CTY).expect("sample CTY should parse");

        assert_eq!(
            database
                .lookup("3Y/LB5SH")
                .expect("slash exact match should resolve")
                .adif,
            24
        );
    }

    #[test]
    fn lookup_returns_waedc_cq_flag() {
        let database = DxccDatabase::from_str(SAMPLE_CTY).expect("sample CTY should parse");

        let info = database
            .lookup("GM0AVR")
            .expect("exact WAE/CQ entity should resolve");
        assert_eq!(info.adif, 279);
        assert!(info.waedc_cq_list);
    }

    #[test]
    fn lookup_ignores_common_suffixes_and_falls_back_to_root_callsign() {
        let database = DxccDatabase::from_str(SAMPLE_CTY).expect("sample CTY should parse");
        let united_states = DxccInfo {
            country_name: "United States".to_string(),
            adif: 291,
            cq_zone: 5,
            itu_zone: 8,
            continent: "NA".to_string(),
            latitude: 38.0,
            longitude: 97.0,
            utc_offset: 5.0,
            primary_prefix: "K".to_string(),
            waedc_cq_list: false,
        };

        assert_eq!(database.lookup("NG4M/P"), Some(united_states.clone()));
        assert_eq!(database.lookup("NG4M/MM"), Some(united_states.clone()));
        assert_eq!(database.lookup("NG4M/QRP"), Some(united_states.clone()));
        assert_eq!(database.lookup("NG4M/1"), Some(united_states.clone()));
        assert_eq!(database.lookup("NG4M/XYZ"), Some(united_states));
    }
}
