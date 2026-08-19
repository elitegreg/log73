use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldInputKind {
    String,
    Rst,
    Numeric,
    Serial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldInput {
    pub kind: FieldInputKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
}

impl FieldInput {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut parts = spec.split(':');
        let kind = match parts
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase()
            .as_str()
        {
            "STRING" => FieldInputKind::String,
            "RST" => FieldInputKind::Rst,
            "NUMERIC" => FieldInputKind::Numeric,
            "SERIAL" => FieldInputKind::Serial,
            other => return Err(format!("unknown field input type: {other}")),
        };
        let max_length = parts
            .next()
            .map(|value| {
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| format!("invalid field input length in {spec}"))
            })
            .transpose()?;
        if parts.next().is_some() {
            return Err(format!("invalid field input type: {spec}"));
        }
        Ok(Self { kind, max_length })
    }

    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            FieldInputKind::String => "STRING",
            FieldInputKind::Rst => "RST",
            FieldInputKind::Numeric => "NUMERIC",
            FieldInputKind::Serial => "SERIAL",
        }
    }

    pub fn as_spec(&self) -> String {
        match self.max_length {
            Some(max_length) => format!("{}:{max_length}", self.kind_name()),
            None => self.kind_name().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationMatch {
    #[default]
    All,
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldValidation {
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "is_validation_all")]
    pub match_mode: ValidationMatch,
}

fn is_validation_all(value: &ValidationMatch) -> bool {
    *value == ValidationMatch::All
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupField {
    pub id: String,
    pub key: String,
    pub label: String,
    pub input: FieldInput,
    pub validation: FieldValidation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<usize>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub preserve_case: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cabrillo_header: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub multi_single_has_mult_transmitter: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeDirection {
    Sent,
    Received,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerialScope {
    #[default]
    Global,
    Band,
    CategoryTransmitter,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableVisibility {
    #[default]
    Auto,
    Show,
    Hide,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeField {
    pub id: String,
    pub label: String,
    pub input: FieldInput,
    pub adif: String,
    pub direction: ExchangeDirection,
    pub validation: FieldValidation,
    #[serde(default, skip_serializing_if = "is_false")]
    pub fixed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "is_global_serial_scope")]
    pub serial_scope: SerialScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only_when: Option<ScoringCondition>,
    #[serde(default, skip_serializing_if = "is_auto_visibility")]
    pub table: TableVisibility,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_global_serial_scope(value: &SerialScope) -> bool {
    *value == SerialScope::Global
}

fn is_auto_visibility(value: &TableVisibility) -> bool {
    *value == TableVisibility::Auto
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CabrilloFixedHeader {
    pub id: String,
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CabrilloRules {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixed_headers: Vec<CabrilloFixedHeader>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub export_fields: Vec<SetupField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContestMetadata {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub valid_multipliers: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub valid_exchanges: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringCondition {
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matches_field: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_values: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suffixes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_suffixes: Vec<String>,
    #[serde(default, skip_serializing)]
    in_set: Option<String>,
    #[serde(default, skip_serializing)]
    in_sets: Vec<String>,
    #[serde(default, skip_serializing)]
    exclude_in_sets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QsoPointRule {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<ScoringCondition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_all: Vec<ScoringCondition>,
    pub points: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QsoPoints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<QsoPointRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geography: Option<GeographyQsoPoints>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_distance: Option<GridDistanceQsoPoints>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_band_param: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridDistanceQsoPoints {
    pub station_grid_field: String,
    pub contact_grid_field: String,
    pub base_points: i64,
    pub kilometers_per_point: i64,
    pub minimum_distance_points: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GeographyPointValue {
    Fixed(i64),
    ByBand {
        default: i64,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        by_band: BTreeMap<String, i64>,
    },
}

impl Default for GeographyPointValue {
    fn default() -> Self {
        Self::Fixed(0)
    }
}

impl From<i64> for GeographyPointValue {
    fn from(points: i64) -> Self {
        Self::Fixed(points)
    }
}

impl GeographyPointValue {
    pub fn for_band(&self, band: Option<&str>) -> i64 {
        match self {
            Self::Fixed(points) => *points,
            Self::ByBand { default, by_band } => band
                .and_then(|band| {
                    by_band
                        .iter()
                        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(band))
                        .map(|(_, points)| *points)
                })
                .unwrap_or(*default),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeographyQsoPoints {
    pub country_field: String,
    pub station_country_field: String,
    pub continent_field: String,
    pub station_continent_field: String,
    pub same_country: GeographyPointValue,
    pub different_country_north_america: GeographyPointValue,
    pub different_country_same_continent: GeographyPointValue,
    pub different_continent: GeographyPointValue,
    #[serde(default)]
    pub unresolved: GeographyPointValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiplierRule {
    pub id: String,
    pub name: String,
    pub field: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<ScoringCondition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_all: Vec<ScoringCondition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_call_suffixes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap_group: Option<String>,
    #[serde(default, skip_serializing)]
    in_sets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BonusPointRule {
    pub id: String,
    pub name: String,
    pub field: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key: Vec<String>,
    pub values: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamMultiplierRule {
    pub id: String,
    pub param: String,
    pub values: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiplierCountBonusRule {
    pub id: String,
    pub name: String,
    pub multiplier: String,
    #[serde(deserialize_with = "deserialize_thresholds")]
    pub thresholds: BTreeMap<usize, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QsoCountBonusRule {
    pub id: String,
    pub name: String,
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(deserialize_with = "deserialize_thresholds")]
    pub thresholds: BTreeMap<usize, i64>,
}

fn deserialize_thresholds<'de, D>(deserializer: D) -> Result<BTreeMap<usize, i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = BTreeMap::<String, i64>::deserialize(deserializer)?;
    values
        .into_iter()
        .map(|(threshold, points)| {
            threshold
                .parse::<usize>()
                .map(|threshold| (threshold, points))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScoringRules {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qso_points: Option<QsoPoints>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dupe_key: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub multipliers: Vec<MultiplierRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bonus_points: Vec<BonusPointRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_multipliers: Vec<ParamMultiplierRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub multiplier_count_bonus_points: Vec<MultiplierCountBonusRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub qso_count_bonus_points: Vec<QsoCountBonusRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QsoColumnSource {
    Adif,
    Meta,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QsoColumnFormat {
    #[default]
    Text,
    DateTimeUtc,
    FrequencyKhz,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QsoColumn {
    pub id: String,
    pub label: String,
    pub source: QsoColumnSource,
    pub field: String,
    #[serde(default)]
    pub format: QsoColumnFormat,
    pub editable: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QsoTable {
    pub columns: Vec<QsoColumn>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContestRules {
    pub id: String,
    pub name: String,
    pub bands: Vec<String>,
    pub modes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_modes: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub value_sets: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub setup_fields: Vec<SetupField>,
    pub exchange: Vec<ExchangeField>,
    pub qso_table: QsoTable,
    #[serde(default)]
    pub scoring: ScoringRules,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cabrillo: Option<CabrilloRules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ContestMetadata>,
}

#[cfg(test)]
pub(crate) fn test_exchange_field(
    id: &str,
    label: &str,
    input: &str,
    adif: &str,
    direction: ExchangeDirection,
) -> ExchangeField {
    ExchangeField {
        id: id.to_string(),
        label: label.to_string(),
        input: FieldInput::parse(input).expect("valid test field input"),
        adif: adif.to_string(),
        direction,
        validation: FieldValidation {
            required: false,
            pattern: None,
            values: Vec::new(),
            match_mode: ValidationMatch::All,
        },
        fixed: false,
        default: None,
        source: None,
        serial_scope: SerialScope::Global,
        only_when: None,
        table: TableVisibility::Auto,
    }
}

#[cfg(test)]
pub(crate) fn test_setup_field(id: &str, key: &str, label: &str, input: &str) -> SetupField {
    SetupField {
        id: id.to_string(),
        key: key.to_string(),
        label: label.to_string(),
        input: FieldInput::parse(input).expect("valid test field input"),
        validation: FieldValidation {
            required: false,
            pattern: None,
            values: Vec::new(),
            match_mode: ValidationMatch::All,
        },
        default: None,
        widget: None,
        help_text: None,
        max_lines: None,
        preserve_case: false,
        cabrillo_header: None,
        multi_single_has_mult_transmitter: false,
    }
}

#[cfg(test)]
pub(crate) fn test_scoring_condition(field: &str, values: &[&str]) -> ScoringCondition {
    ScoringCondition {
        field: field.to_string(),
        matches_field: None,
        values: values.iter().map(|value| (*value).to_string()).collect(),
        exclude_values: Vec::new(),
        suffixes: Vec::new(),
        exclude_suffixes: Vec::new(),
        in_set: None,
        in_sets: Vec::new(),
        exclude_in_sets: Vec::new(),
    }
}

#[cfg(test)]
pub(crate) fn test_multiplier_rule(id: &str, name: &str, field: &str) -> MultiplierRule {
    MultiplierRule {
        id: id.to_string(),
        name: name.to_string(),
        field: field.to_string(),
        key: Vec::new(),
        values: Vec::new(),
        when: None,
        when_all: Vec::new(),
        exclude_call_suffixes: Vec::new(),
        exclude_values: Vec::new(),
        fixed_key: None,
        max_count: None,
        cap_group: None,
        in_sets: Vec::new(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ContestSummary {
    pub id: String,
    pub name: String,
    pub setup_fields: Vec<SetupField>,
}

#[derive(Debug, Clone)]
pub struct ContestRulesStore {
    contests: BTreeMap<String, ContestRules>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RulesFile {
    schema: u8,
    #[serde(default)]
    value_sets: BTreeMap<String, YamlValue>,
    #[serde(default)]
    presets: BTreeMap<String, YamlValue>,
    #[serde(default)]
    profiles: BTreeMap<String, YamlValue>,
    #[serde(default)]
    contests: BTreeMap<String, YamlValue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum AllowedBandValue {
    Name(String),
    Meters(u16),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContestRules {
    #[serde(default)]
    name: Option<String>,
    bands: Vec<AllowedBandValue>,
    modes: Vec<String>,
    #[serde(default)]
    excluded_modes: Vec<String>,
    #[serde(default)]
    value_sets: BTreeMap<String, RawValueSet>,
    #[serde(default)]
    setup_fields: Vec<RawSetupField>,
    exchange: Vec<RawExchangeField>,
    #[serde(default)]
    qso_table: Option<RawQsoTable>,
    #[serde(default)]
    scoring: ScoringRules,
    #[serde(default)]
    cabrillo: Option<RawCabrilloRules>,
    #[serde(default)]
    metadata: Option<ContestMetadata>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawValueSet {
    #[serde(default)]
    r#use: Option<String>,
    #[serde(default)]
    values: Option<Vec<String>>,
    #[serde(default)]
    values_from_file: Option<PathBuf>,
    #[serde(default)]
    exclude: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSetupField {
    id: String,
    key: String,
    label: String,
    #[serde(rename = "type")]
    field_type: String,
    #[serde(default)]
    required: Option<bool>,
    #[serde(default)]
    regex: Option<String>,
    #[serde(default)]
    valid_values_or_regex: bool,
    #[serde(default)]
    default: Option<Value>,
    #[serde(default)]
    in_sets: Vec<String>,
    #[serde(default)]
    valid_values: Vec<String>,
    #[serde(default)]
    widget: Option<String>,
    #[serde(default)]
    help_text: Option<String>,
    #[serde(default)]
    max_lines: Option<usize>,
    #[serde(default)]
    preserve_case: bool,
    #[serde(default)]
    cabrillo_header: Option<String>,
    #[serde(default)]
    multi_single_has_mult_transmitter: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExchangeField {
    id: String,
    label: String,
    #[serde(rename = "type")]
    field_type: String,
    adif: String,
    direction: ExchangeDirection,
    #[serde(default)]
    fixed: bool,
    #[serde(default)]
    default: Option<Value>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    regex: Option<String>,
    #[serde(default)]
    valid_values_or_regex: bool,
    #[serde(default)]
    in_sets: Vec<String>,
    #[serde(default)]
    valid_values: Vec<String>,
    #[serde(default)]
    serial_scope: SerialScope,
    #[serde(default)]
    only_when: Option<ScoringCondition>,
    #[serde(default)]
    table: TableVisibility,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawQsoTable {
    columns: Vec<QsoColumn>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCabrilloRules {
    #[serde(default)]
    fixed_headers: Vec<CabrilloFixedHeader>,
    #[serde(default)]
    export_fields: Vec<RawSetupField>,
}

#[derive(Default)]
struct Catalog {
    value_sets: BTreeMap<String, YamlValue>,
    presets: BTreeMap<String, YamlValue>,
    profiles: BTreeMap<String, YamlValue>,
    contests: BTreeMap<String, YamlValue>,
}

impl ContestRulesStore {
    pub fn load_dirs<I, P>(paths: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let search_paths = paths
            .into_iter()
            .map(|path| path.as_ref().to_path_buf())
            .collect::<Vec<_>>();
        let catalog = load_catalog(&search_paths)?;
        if catalog.contests.is_empty() {
            return Err(format!(
                "no contest rules found in searched directories: {}",
                format_paths(&search_paths)
            ));
        }

        let mut composed = BTreeMap::new();
        for id in catalog.contests.keys() {
            let value = compose_contest(id, &catalog, &mut composed, &mut Vec::new())?;
            composed.insert(id.clone(), value);
        }

        let available_sets = resolve_global_value_sets(&catalog.value_sets, &search_paths)?;
        let mut contests = BTreeMap::new();
        for (id, value) in composed {
            let expanded = expand_presets(&value, &catalog.presets, &mut Vec::new())?;
            let raw: RawContestRules = serde_yaml::from_value(expanded)
                .map_err(|error| format!("invalid resolved contest {id}: {error}"))?;
            let contest = normalize_contest(&id, raw, &available_sets, &search_paths)?;
            contests.insert(id, contest);
        }

        info!(contests = contests.len(), "loaded contest rules");
        Ok(Self { contests })
    }

    pub fn get(&self, id: &str) -> Option<&ContestRules> {
        self.contests.get(id)
    }

    pub fn default_contest(&self) -> Option<&ContestRules> {
        self.contests.values().next()
    }

    pub fn summaries(&self) -> Vec<ContestSummary> {
        self.contests
            .values()
            .map(|contest| ContestSummary {
                id: contest.id.clone(),
                name: contest.name.clone(),
                setup_fields: contest.setup_fields.clone(),
            })
            .collect()
    }
}

fn load_catalog(search_paths: &[PathBuf]) -> Result<Catalog, String> {
    let mut catalog = Catalog::default();
    for directory in search_paths {
        info!(path = %directory.display(), "looking for contest rules directory");
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "unable to read contest rules dir {}: {error}",
                    directory.display()
                ));
            }
        };
        let mut files = entries
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("unable to read contest rules entry: {error}"))?;
        files.sort();
        for path in files {
            let extension = path.extension().and_then(|extension| extension.to_str());
            if !matches!(extension, Some("yaml" | "yml")) {
                continue;
            }
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
            let file: RulesFile = serde_yaml::from_str(&text)
                .map_err(|error| format!("unable to parse {}: {error}", path.display()))?;
            if file.schema != 2 {
                return Err(format!(
                    "unsupported contest-rules schema {} in {}; expected 2",
                    file.schema,
                    path.display()
                ));
            }
            merge_catalog_entries(&mut catalog.value_sets, file.value_sets, "value set", &path)?;
            merge_catalog_entries(&mut catalog.presets, file.presets, "preset", &path)?;
            merge_catalog_entries(&mut catalog.profiles, file.profiles, "profile", &path)?;
            merge_catalog_entries(&mut catalog.contests, file.contests, "contest", &path)?;
        }
    }
    Ok(catalog)
}

fn merge_catalog_entries(
    target: &mut BTreeMap<String, YamlValue>,
    entries: BTreeMap<String, YamlValue>,
    kind: &str,
    path: &Path,
) -> Result<(), String> {
    for (name, value) in entries {
        if target.insert(name.clone(), value).is_some() {
            return Err(format!(
                "duplicate {kind} {name} while loading {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn compose_contest(
    id: &str,
    catalog: &Catalog,
    cache: &mut BTreeMap<String, YamlValue>,
    stack: &mut Vec<String>,
) -> Result<YamlValue, String> {
    if let Some(value) = cache.get(id) {
        return Ok(value.clone());
    }
    let token = format!("contest:{id}");
    push_resolution(&token, stack)?;
    let body = catalog
        .contests
        .get(id)
        .ok_or_else(|| format!("unknown contest: {id}"))?;
    let mapping = body
        .as_mapping()
        .ok_or_else(|| format!("contest {id} must be a mapping"))?;
    let mut value = YamlValue::Mapping(YamlMapping::new());
    for profile in string_list(
        mapping.get(YamlValue::String("profiles".to_string())),
        "profiles",
    )? {
        let resolved = compose_profile(&profile, catalog, &mut BTreeMap::new(), stack)?;
        value = merge_yaml(value, resolved, &format!("contest {id}"))?;
    }
    if let Some(parent) = optional_string(
        mapping.get(YamlValue::String("extends".to_string())),
        "extends",
    )? {
        let resolved = compose_contest(&parent, catalog, cache, stack)?;
        value = merge_yaml(value, resolved, &format!("contest {id}"))?;
    }
    let mut own = mapping.clone();
    own.remove(YamlValue::String("profiles".to_string()));
    own.remove(YamlValue::String("extends".to_string()));
    value = merge_yaml(value, YamlValue::Mapping(own), &format!("contest {id}"))?;
    stack.pop();
    cache.insert(id.to_string(), value.clone());
    Ok(value)
}

fn compose_profile(
    id: &str,
    catalog: &Catalog,
    cache: &mut BTreeMap<String, YamlValue>,
    stack: &mut Vec<String>,
) -> Result<YamlValue, String> {
    if let Some(value) = cache.get(id) {
        return Ok(value.clone());
    }
    let token = format!("profile:{id}");
    push_resolution(&token, stack)?;
    let body = catalog
        .profiles
        .get(id)
        .ok_or_else(|| format!("unknown profile: {id}"))?;
    let mapping = body
        .as_mapping()
        .ok_or_else(|| format!("profile {id} must be a mapping"))?;
    let mut value = YamlValue::Mapping(YamlMapping::new());
    for profile in string_list(
        mapping.get(YamlValue::String("profiles".to_string())),
        "profiles",
    )? {
        let resolved = compose_profile(&profile, catalog, cache, stack)?;
        value = merge_yaml(value, resolved, &format!("profile {id}"))?;
    }
    let mut own = mapping.clone();
    own.remove(YamlValue::String("profiles".to_string()));
    value = merge_yaml(value, YamlValue::Mapping(own), &format!("profile {id}"))?;
    stack.pop();
    cache.insert(id.to_string(), value.clone());
    Ok(value)
}

fn push_resolution(token: &str, stack: &mut Vec<String>) -> Result<(), String> {
    if let Some(position) = stack.iter().position(|entry| entry == token) {
        return Err(format!(
            "contest-rules reference cycle: {} -> {token}",
            stack[position..].join(" -> ")
        ));
    }
    stack.push(token.to_string());
    Ok(())
}

fn merge_yaml(base: YamlValue, overlay: YamlValue, context: &str) -> Result<YamlValue, String> {
    match (base, overlay) {
        (YamlValue::Mapping(mut base), YamlValue::Mapping(overlay)) => {
            for (key, value) in overlay {
                let merged = match base.remove(&key) {
                    Some(existing) => merge_yaml(existing, value, context)?,
                    None => value,
                };
                base.insert(key, merged);
            }
            Ok(YamlValue::Mapping(base))
        }
        (YamlValue::Sequence(base), YamlValue::Sequence(overlay))
            if is_keyed_collection(&base) || is_keyed_collection(&overlay) =>
        {
            merge_keyed_collection(base, overlay, context).map(YamlValue::Sequence)
        }
        (_, overlay) => Ok(overlay),
    }
}

fn is_keyed_collection(values: &[YamlValue]) -> bool {
    !values.is_empty() && values.iter().all(|value| item_id(value).is_some())
}

fn item_id(value: &YamlValue) -> Option<&str> {
    value
        .as_mapping()?
        .get(YamlValue::String("id".to_string()))?
        .as_str()
}

fn merge_keyed_collection(
    mut base: Vec<YamlValue>,
    overlay: Vec<YamlValue>,
    context: &str,
) -> Result<Vec<YamlValue>, String> {
    if base
        .iter()
        .chain(&overlay)
        .any(|value| item_id(value).is_none())
    {
        return Err(format!("mixed keyed and unkeyed collection in {context}"));
    }
    validate_unique_ids(&base, context)?;
    validate_unique_ids(&overlay, context)?;
    if overlay.is_empty() {
        return Ok(Vec::new());
    }
    for item in overlay {
        let id = item_id(&item)
            .ok_or_else(|| format!("mixed keyed and unkeyed collection in {context}"))?
            .to_string();
        let mapping = item.as_mapping().expect("item with id should be mapping");
        let remove = mapping
            .get(YamlValue::String("remove".to_string()))
            .and_then(YamlValue::as_bool)
            .unwrap_or(false);
        let position = base
            .iter()
            .position(|candidate| item_id(candidate) == Some(&id));
        if remove {
            let Some(position) = position else {
                return Err(format!("cannot remove unknown id {id} in {context}"));
            };
            base.remove(position);
            continue;
        }
        let before = optional_string(
            mapping.get(YamlValue::String("before".to_string())),
            "before",
        )?;
        let after = optional_string(mapping.get(YamlValue::String("after".to_string())), "after")?;
        if before.is_some() && after.is_some() {
            return Err(format!("id {id} sets both before and after in {context}"));
        }
        let mut clean = mapping.clone();
        clean.remove(YamlValue::String("remove".to_string()));
        clean.remove(YamlValue::String("before".to_string()));
        clean.remove(YamlValue::String("after".to_string()));
        let clean = YamlValue::Mapping(clean);
        let merged = if let Some(position) = position {
            merge_yaml(base.remove(position), clean, context)?
        } else {
            clean
        };
        let target = if let Some(target) = before {
            base.iter()
                .position(|candidate| item_id(candidate) == Some(target.as_str()))
                .ok_or_else(|| format!("unknown before target {target} for {id} in {context}"))?
        } else if let Some(target) = after {
            base.iter()
                .position(|candidate| item_id(candidate) == Some(target.as_str()))
                .map(|position| position + 1)
                .ok_or_else(|| format!("unknown after target {target} for {id} in {context}"))?
        } else if let Some(position) = position {
            position.min(base.len())
        } else {
            base.len()
        };
        base.insert(target, merged);
    }
    validate_unique_ids(&base, context)?;
    Ok(base)
}

fn validate_unique_ids(values: &[YamlValue], context: &str) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    for value in values {
        let Some(id) = item_id(value) else {
            continue;
        };
        if !ids.insert(id) {
            return Err(format!("duplicate id {id} in {context}"));
        }
    }
    Ok(())
}

fn expand_presets(
    value: &YamlValue,
    presets: &BTreeMap<String, YamlValue>,
    stack: &mut Vec<String>,
) -> Result<YamlValue, String> {
    match value {
        YamlValue::Mapping(mapping) => {
            let preset_name = mapping
                .get(YamlValue::String("preset".to_string()))
                .and_then(YamlValue::as_str);
            let mut expanded = if let Some(name) = preset_name {
                let token = format!("preset:{name}");
                push_resolution(&token, stack)?;
                let preset = presets
                    .get(name)
                    .ok_or_else(|| format!("unknown preset: {name}"))?;
                let preset = expand_presets(preset, presets, stack)?;
                stack.pop();
                let mut overlay = mapping.clone();
                overlay.remove(YamlValue::String("preset".to_string()));
                merge_yaml(
                    preset,
                    YamlValue::Mapping(overlay),
                    &format!("preset {name}"),
                )?
            } else {
                YamlValue::Mapping(mapping.clone())
            };
            let result = expanded
                .as_mapping_mut()
                .expect("expanded mapping should remain a mapping");
            let keys = result.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let child = result.get(&key).expect("existing key").clone();
                result.insert(key, expand_presets(&child, presets, stack)?);
            }
            Ok(expanded)
        }
        YamlValue::Sequence(values) => values
            .iter()
            .map(|value| expand_presets(value, presets, stack))
            .collect::<Result<Vec<_>, _>>()
            .map(YamlValue::Sequence),
        _ => Ok(value.clone()),
    }
}

fn resolve_global_value_sets(
    raw: &BTreeMap<String, YamlValue>,
    search_paths: &[PathBuf],
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let parsed = raw
        .iter()
        .map(|(name, value)| {
            serde_yaml::from_value::<RawValueSet>(value.clone())
                .map(|value| (name.clone(), value))
                .map_err(|error| format!("invalid value set {name}: {error}"))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let mut resolved = BTreeMap::new();
    for name in parsed.keys() {
        resolve_named_value_set(name, &parsed, search_paths, &mut resolved, &mut Vec::new())?;
    }
    Ok(resolved)
}

fn resolve_named_value_set(
    name: &str,
    raw: &BTreeMap<String, RawValueSet>,
    search_paths: &[PathBuf],
    resolved: &mut BTreeMap<String, Vec<String>>,
    stack: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    if let Some(values) = resolved.get(name) {
        return Ok(values.clone());
    }
    push_resolution(&format!("value-set:{name}"), stack)?;
    let value_set = raw
        .get(name)
        .ok_or_else(|| format!("unknown value set: {name}"))?;
    let base = if let Some(parent) = &value_set.r#use {
        resolve_named_value_set(parent, raw, search_paths, resolved, stack)?
    } else {
        Vec::new()
    };
    let values = resolve_value_set_body(value_set, base, search_paths, name)?;
    stack.pop();
    resolved.insert(name.to_string(), values.clone());
    Ok(values)
}

fn resolve_value_set_body(
    raw: &RawValueSet,
    base: Vec<String>,
    search_paths: &[PathBuf],
    name: &str,
) -> Result<Vec<String>, String> {
    if raw.values.is_some() && raw.values_from_file.is_some() {
        return Err(format!(
            "value set {name} defines both values and values_from_file"
        ));
    }
    let values = if let Some(values) = &raw.values {
        values.clone()
    } else if let Some(file_name) = &raw.values_from_file {
        let path = value_set_file_path(file_name, search_paths)?;
        fs::read_to_string(&path)
            .map_err(|error| format!("unable to read value-set file {}: {error}", path.display()))?
            .lines()
            .map(str::trim)
            .filter(|value| !value.is_empty() && !value.starts_with('#'))
            .map(str::to_string)
            .collect()
    } else if raw.r#use.is_some() {
        base
    } else {
        return Err(format!(
            "value set {name} must define use, values, or values_from_file"
        ));
    };
    let excluded = raw
        .exclude
        .iter()
        .map(|value| value.trim())
        .collect::<BTreeSet<_>>();
    Ok(values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !excluded.contains(value.as_str()))
        .collect())
}

fn normalize_contest(
    id: &str,
    raw: RawContestRules,
    global_sets: &BTreeMap<String, Vec<String>>,
    search_paths: &[PathBuf],
) -> Result<ContestRules, String> {
    let available_sets = resolve_contest_value_sets(&raw.value_sets, global_sets, search_paths)?;
    let mut referenced_sets = BTreeSet::new();
    let setup_fields = raw
        .setup_fields
        .into_iter()
        .map(|field| normalize_setup_field(field, &available_sets, &mut referenced_sets))
        .collect::<Result<Vec<_>, _>>()?;
    let mut exchange = raw
        .exchange
        .into_iter()
        .map(|field| normalize_exchange_field(field, &available_sets, &mut referenced_sets))
        .collect::<Result<Vec<_>, _>>()?;
    let mut scoring = raw.scoring;
    resolve_scoring_sets(&mut scoring, &available_sets, &mut referenced_sets)?;
    let cabrillo = raw
        .cabrillo
        .map(|cabrillo| {
            cabrillo
                .export_fields
                .into_iter()
                .map(|field| normalize_setup_field(field, &available_sets, &mut referenced_sets))
                .collect::<Result<Vec<_>, _>>()
                .map(|export_fields| CabrilloRules {
                    fixed_headers: cabrillo.fixed_headers,
                    export_fields,
                })
        })
        .transpose()?;
    for field in &mut exchange {
        if let Some(condition) = &mut field.only_when {
            resolve_condition_sets(condition, &available_sets, &mut referenced_sets)?;
        }
    }
    let mut contest = ContestRules {
        id: id.to_string(),
        name: raw.name.unwrap_or_else(|| id.to_string()),
        bands: raw.bands.iter().map(allowed_band_name).collect(),
        modes: raw.modes,
        excluded_modes: raw.excluded_modes,
        value_sets: referenced_sets
            .into_iter()
            .filter_map(|name| {
                available_sets
                    .get(&name)
                    .cloned()
                    .map(|values| (name, values))
            })
            .collect(),
        setup_fields,
        exchange,
        qso_table: QsoTable::default(),
        scoring,
        cabrillo,
        metadata: raw.metadata,
    };
    contest.qso_table = raw
        .qso_table
        .map(|table| QsoTable {
            columns: table.columns,
        })
        .unwrap_or_else(|| derive_qso_table(&contest));
    validate_contest(&contest)?;
    Ok(contest)
}

fn resolve_contest_value_sets(
    raw: &BTreeMap<String, RawValueSet>,
    global: &BTreeMap<String, Vec<String>>,
    search_paths: &[PathBuf],
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut local = BTreeMap::new();
    for name in raw.keys() {
        resolve_contest_value_set(name, raw, global, search_paths, &mut local, &mut Vec::new())?;
    }
    let mut available = global.clone();
    available.extend(local);
    Ok(available)
}

fn resolve_contest_value_set(
    name: &str,
    raw: &BTreeMap<String, RawValueSet>,
    global: &BTreeMap<String, Vec<String>>,
    search_paths: &[PathBuf],
    resolved: &mut BTreeMap<String, Vec<String>>,
    stack: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    if let Some(values) = resolved.get(name) {
        return Ok(values.clone());
    }
    push_resolution(&format!("contest-value-set:{name}"), stack)?;
    let value_set = raw
        .get(name)
        .ok_or_else(|| format!("unknown contest value set: {name}"))?;
    let base = if let Some(parent) = &value_set.r#use {
        if parent == name {
            global.get(parent).cloned().ok_or_else(|| {
                format!("unknown global value set {parent} extended by contest value set {name}")
            })?
        } else if raw.contains_key(parent) {
            resolve_contest_value_set(parent, raw, global, search_paths, resolved, stack)?
        } else {
            global
                .get(parent)
                .cloned()
                .ok_or_else(|| format!("unknown value set {parent} referenced by {name}"))?
        }
    } else {
        Vec::new()
    };
    let values = resolve_value_set_body(value_set, base, search_paths, name)?;
    stack.pop();
    resolved.insert(name.to_string(), values.clone());
    Ok(values)
}

fn normalize_setup_field(
    raw: RawSetupField,
    sets: &BTreeMap<String, Vec<String>>,
    referenced: &mut BTreeSet<String>,
) -> Result<SetupField, String> {
    let values = field_values(&raw.in_sets, &raw.valid_values, sets, referenced)?;
    Ok(SetupField {
        id: raw.id,
        key: raw.key,
        label: raw.label,
        input: FieldInput::parse(&raw.field_type)?,
        validation: FieldValidation {
            required: raw.required.unwrap_or(true),
            pattern: raw.regex,
            values,
            match_mode: if raw.valid_values_or_regex {
                ValidationMatch::Any
            } else {
                ValidationMatch::All
            },
        },
        default: raw.default,
        widget: raw.widget,
        help_text: raw.help_text,
        max_lines: raw.max_lines,
        preserve_case: raw.preserve_case,
        cabrillo_header: raw.cabrillo_header,
        multi_single_has_mult_transmitter: raw.multi_single_has_mult_transmitter,
    })
}

fn normalize_exchange_field(
    raw: RawExchangeField,
    sets: &BTreeMap<String, Vec<String>>,
    referenced: &mut BTreeSet<String>,
) -> Result<ExchangeField, String> {
    let values = field_values(&raw.in_sets, &raw.valid_values, sets, referenced)?;
    Ok(ExchangeField {
        id: raw.id,
        label: raw.label,
        input: FieldInput::parse(&raw.field_type)?,
        adif: raw.adif,
        direction: raw.direction,
        validation: FieldValidation {
            required: true,
            pattern: raw.regex,
            values,
            match_mode: if raw.valid_values_or_regex {
                ValidationMatch::Any
            } else {
                ValidationMatch::All
            },
        },
        fixed: raw.fixed,
        default: raw.default,
        source: raw.source,
        serial_scope: raw.serial_scope,
        only_when: raw.only_when,
        table: raw.table,
    })
}

fn field_values(
    in_sets: &[String],
    explicit: &[String],
    sets: &BTreeMap<String, Vec<String>>,
    referenced: &mut BTreeSet<String>,
) -> Result<Vec<String>, String> {
    let mut values = Vec::new();
    for name in in_sets {
        referenced.insert(name.clone());
        values.extend(
            sets.get(name)
                .ok_or_else(|| format!("unknown value set referenced by in_sets: {name}"))?
                .clone(),
        );
    }
    values.extend(explicit.iter().cloned());
    Ok(values)
}

fn resolve_scoring_sets(
    scoring: &mut ScoringRules,
    sets: &BTreeMap<String, Vec<String>>,
    referenced: &mut BTreeSet<String>,
) -> Result<(), String> {
    if let Some(points) = &mut scoring.qso_points {
        for rule in &mut points.rules {
            if let Some(condition) = &mut rule.when {
                resolve_condition_sets(condition, sets, referenced)?;
            }
            for condition in &mut rule.when_all {
                resolve_condition_sets(condition, sets, referenced)?;
            }
        }
    }
    for multiplier in &mut scoring.multipliers {
        for name in std::mem::take(&mut multiplier.in_sets) {
            referenced.insert(name.clone());
            multiplier.values.extend(
                sets.get(&name)
                    .ok_or_else(|| format!("unknown multiplier value set: {name}"))?
                    .clone(),
            );
        }
        if multiplier.key.is_empty() {
            multiplier.key.push(multiplier.field.clone());
        }
        if let Some(condition) = &mut multiplier.when {
            resolve_condition_sets(condition, sets, referenced)?;
        }
        for condition in &mut multiplier.when_all {
            resolve_condition_sets(condition, sets, referenced)?;
        }
    }
    Ok(())
}

fn resolve_condition_sets(
    condition: &mut ScoringCondition,
    sets: &BTreeMap<String, Vec<String>>,
    referenced: &mut BTreeSet<String>,
) -> Result<(), String> {
    let mut names = std::mem::take(&mut condition.in_sets);
    if let Some(name) = condition.in_set.take() {
        names.push(name);
    }
    for name in names {
        referenced.insert(name.clone());
        condition.values.extend(
            sets.get(&name)
                .ok_or_else(|| format!("unknown scoring value set: {name}"))?
                .clone(),
        );
    }
    for name in std::mem::take(&mut condition.exclude_in_sets) {
        referenced.insert(name.clone());
        condition.exclude_values.extend(
            sets.get(&name)
                .ok_or_else(|| format!("unknown excluded scoring value set: {name}"))?
                .clone(),
        );
    }
    Ok(())
}

fn validate_contest(contest: &ContestRules) -> Result<(), String> {
    validate_ids(
        contest.setup_fields.iter().map(|field| field.id.as_str()),
        "setup field",
    )?;
    validate_ids(
        contest.setup_fields.iter().map(|field| field.key.as_str()),
        "setup field key",
    )?;
    validate_ids(
        contest.exchange.iter().map(|field| field.id.as_str()),
        "exchange field",
    )?;
    validate_ids(
        contest
            .cabrillo
            .iter()
            .flat_map(|rules| rules.export_fields.iter())
            .map(|field| field.id.as_str()),
        "Cabrillo export field",
    )?;
    validate_ids(
        contest
            .cabrillo
            .iter()
            .flat_map(|rules| rules.export_fields.iter())
            .map(|field| field.key.as_str()),
        "Cabrillo export field key",
    )?;
    validate_ids(
        contest
            .cabrillo
            .iter()
            .flat_map(|rules| rules.fixed_headers.iter())
            .map(|field| field.id.as_str()),
        "Cabrillo fixed header",
    )?;
    validate_ids(
        contest
            .scoring
            .qso_points
            .iter()
            .flat_map(|points| points.rules.iter())
            .map(|rule| rule.id.as_str()),
        "QSO point rule",
    )?;
    validate_ids(
        contest
            .scoring
            .multipliers
            .iter()
            .map(|rule| rule.id.as_str()),
        "multiplier rule",
    )?;
    validate_ids(
        contest
            .scoring
            .bonus_points
            .iter()
            .map(|rule| rule.id.as_str()),
        "bonus point rule",
    )?;
    validate_ids(
        contest
            .scoring
            .param_multipliers
            .iter()
            .map(|rule| rule.id.as_str()),
        "parameter multiplier rule",
    )?;
    validate_ids(
        contest
            .scoring
            .multiplier_count_bonus_points
            .iter()
            .map(|rule| rule.id.as_str()),
        "multiplier-count bonus rule",
    )?;
    validate_ids(
        contest
            .scoring
            .qso_count_bonus_points
            .iter()
            .map(|rule| rule.id.as_str()),
        "QSO-count bonus rule",
    )?;
    validate_ids(
        contest
            .qso_table
            .columns
            .iter()
            .map(|column| column.id.as_str()),
        "QSO table column",
    )?;
    for field in &contest.exchange {
        if field.serial_scope != SerialScope::Global && field.input.kind != FieldInputKind::Serial {
            return Err(format!(
                "exchange field {} sets serial_scope but is not serial",
                field.id
            ));
        }
        if field.direction == ExchangeDirection::Received && field.source.is_some() {
            return Err(format!(
                "received exchange field {} cannot define source",
                field.id
            ));
        }
    }
    if let Some(grid_distance) = contest
        .scoring
        .qso_points
        .as_ref()
        .and_then(|points| points.grid_distance.as_ref())
        && (grid_distance.base_points < 0
            || grid_distance.kilometers_per_point <= 0
            || grid_distance.minimum_distance_points < 0)
    {
        return Err(
            "grid_distance scoring values must be non-negative and use a positive distance step"
                .to_string(),
        );
    }
    for multiplier in &contest.scoring.param_multipliers {
        let field = contest
            .setup_fields
            .iter()
            .find(|field| field.key.eq_ignore_ascii_case(&multiplier.param))
            .ok_or_else(|| {
                format!(
                    "param_multipliers references unknown setup field: {}",
                    multiplier.param
                )
            })?;
        if multiplier.values.is_empty() || multiplier.values.values().any(|factor| *factor <= 0) {
            return Err(format!(
                "param_multipliers for {} must define positive factors",
                multiplier.param
            ));
        }
        for value in multiplier.values.keys() {
            if !field.validation.values.is_empty()
                && !field
                    .validation
                    .values
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(value))
            {
                return Err(format!(
                    "param_multipliers value {value} is not valid for {}",
                    multiplier.param
                ));
            }
        }
    }
    for bonus in &contest.scoring.multiplier_count_bonus_points {
        if !contest
            .scoring
            .multipliers
            .iter()
            .any(|multiplier| multiplier.name.eq_ignore_ascii_case(&bonus.multiplier))
        {
            return Err(format!(
                "multiplier_count_bonus_points references unknown multiplier: {}",
                bonus.multiplier
            ));
        }
        if bonus.thresholds.is_empty()
            || bonus
                .thresholds
                .iter()
                .any(|(threshold, points)| *threshold == 0 || *points <= 0)
        {
            return Err(format!(
                "multiplier_count_bonus_points {} must define positive thresholds and points",
                bonus.name
            ));
        }
    }
    for multiplier in &contest.scoring.multipliers {
        if multiplier.max_count == Some(0) {
            return Err(format!(
                "multiplier {} max_count must be greater than zero",
                multiplier.id
            ));
        }
    }
    for bonus in &contest.scoring.qso_count_bonus_points {
        if bonus.field.trim().is_empty()
            || bonus.thresholds.is_empty()
            || bonus
                .thresholds
                .iter()
                .any(|(threshold, points)| *threshold == 0 || *points <= 0)
            || (bonus.param.is_none() && !bonus.values.is_empty())
        {
            return Err(format!(
                "qso_count_bonus_points {} must define a field, positive thresholds and points, and a parameter when values are set",
                bonus.name
            ));
        }
    }
    Ok(())
}

fn validate_ids<'a>(ids: impl IntoIterator<Item = &'a str>, kind: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if id.trim().is_empty() {
            return Err(format!("{kind} id cannot be empty"));
        }
        if !seen.insert(id) {
            return Err(format!("duplicate {kind} id: {id}"));
        }
    }
    Ok(())
}

fn derive_qso_table(contest: &ContestRules) -> QsoTable {
    let mut columns = vec![
        QsoColumn {
            id: "date-time".to_string(),
            label: "Date/Time (UTC)".to_string(),
            source: QsoColumnSource::Adif,
            field: "QSO_DATE_TIME_ON".to_string(),
            format: QsoColumnFormat::DateTimeUtc,
            editable: true,
        },
        QsoColumn {
            id: "frequency".to_string(),
            label: "Freq".to_string(),
            source: QsoColumnSource::Adif,
            field: "FREQ".to_string(),
            format: QsoColumnFormat::FrequencyKhz,
            editable: true,
        },
        QsoColumn {
            id: "mode".to_string(),
            label: "Mode".to_string(),
            source: QsoColumnSource::Adif,
            field: "MODE".to_string(),
            format: QsoColumnFormat::Text,
            editable: true,
        },
        QsoColumn {
            id: "call".to_string(),
            label: "Call".to_string(),
            source: QsoColumnSource::Adif,
            field: "CALL".to_string(),
            format: QsoColumnFormat::Text,
            editable: true,
        },
    ];
    for field in &contest.exchange {
        let include = match field.table {
            TableVisibility::Show => true,
            TableVisibility::Hide => false,
            TableVisibility::Auto => {
                field.direction == ExchangeDirection::Received
                    || !field.fixed
                    || field.input.kind == FieldInputKind::Serial
            }
        };
        if include {
            columns.push(QsoColumn {
                id: format!("exchange-{}", field.id),
                label: field.label.clone(),
                source: QsoColumnSource::Adif,
                field: field.adif.clone(),
                format: QsoColumnFormat::Text,
                editable: !field.fixed
                    && !(field.direction == ExchangeDirection::Sent
                        && field.input.kind == FieldInputKind::Serial),
            });
        }
    }
    if !contest.scoring.multipliers.is_empty() {
        columns.push(QsoColumn {
            id: "multipliers".to_string(),
            label: "Mult".to_string(),
            source: QsoColumnSource::Meta,
            field: "mult".to_string(),
            format: QsoColumnFormat::Text,
            editable: false,
        });
    }
    if contest.scoring.qso_points.is_some() {
        columns.push(QsoColumn {
            id: "points".to_string(),
            label: "Pts".to_string(),
            source: QsoColumnSource::Meta,
            field: "pts".to_string(),
            format: QsoColumnFormat::Text,
            editable: false,
        });
    }
    columns.push(QsoColumn {
        id: "operator".to_string(),
        label: "Op".to_string(),
        source: QsoColumnSource::Adif,
        field: "OPERATOR".to_string(),
        format: QsoColumnFormat::Text,
        editable: true,
    });
    QsoTable { columns }
}

fn allowed_band_name(value: &AllowedBandValue) -> String {
    match value {
        AllowedBandValue::Name(name) => name.trim().to_string(),
        AllowedBandValue::Meters(meters) => format!("{meters}m"),
    }
}

fn optional_string(value: Option<&YamlValue>, name: &str) -> Result<Option<String>, String> {
    value
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{name} must be a string"))
        })
        .transpose()
}

fn string_list(value: Option<&YamlValue>, name: &str) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .as_sequence()
        .ok_or_else(|| format!("{name} must be a list"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{name} entries must be strings"))
        })
        .collect()
}

fn format_paths(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "<none>".to_string();
    }
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn value_set_file_path(file_name: &Path, search_paths: &[PathBuf]) -> Result<PathBuf, String> {
    if file_name.is_absolute()
        || file_name.components().count() != 1
        || !matches!(file_name.components().next(), Some(Component::Normal(_)))
    {
        return Err(format!(
            "value-set file must be a file name within contest-rules directories: {}",
            file_name.display()
        ));
    }
    for directory in search_paths.iter().rev() {
        let candidate = directory.join(file_name);
        match fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => return Ok(candidate),
            Ok(_) => {
                return Err(format!(
                    "value-set file is not a regular file: {}",
                    candidate.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "unable to inspect value-set file {}: {error}",
                    candidate.display()
                ));
            }
        }
    }
    Err(format!(
        "value-set file {} not found in contest rules directories: {}",
        file_name.display(),
        format_paths(search_paths)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "log73-contest-rules-v2-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn write(&self, name: &str, contents: &str) {
            fs::write(self.0.join(name), contents).expect("write test file");
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const MINIMAL: &str = r#"
schema: 2
contests:
  TEST:
    name: Test
    bands: [20]
    modes: [CW]
    exchange:
      - id: rst-received
        label: RST
        type: RST
        adif: RST_RCVD
        direction: received
"#;

    #[test]
    fn loads_minimal_v2_and_derives_table() {
        let dir = TestDir::new();
        dir.write("test.yaml", MINIMAL);
        let store = ContestRulesStore::load_dirs([&dir.0]).expect("rules load");
        let rules = store.get("TEST").expect("test contest");
        assert_eq!(rules.id, "TEST");
        assert_eq!(rules.bands, ["20m"]);
        assert!(
            rules
                .qso_table
                .columns
                .iter()
                .any(|column| column.field == "RST_RCVD")
        );
    }

    #[test]
    fn resolves_catalogs_data_files_and_the_public_api_shape() {
        let dir = TestDir::new();
        dir.write("locations.dat", "# accepted locations\nSC\nNC\nGA\n");
        dir.write(
            "catalog.yaml",
            r#"
schema: 2
value_sets:
  locations:
    values_from_file: locations.dat
  locations-without-ga:
    use: locations
    exclude: [GA]
presets:
  received-location:
    label: Location
    type: String:2
    adif: SRX_STRING
    direction: received
    in_sets: [locations-without-ga]
profiles:
  hf-cw:
    bands: [40, 20]
    modes: [CW]
"#,
        );
        dir.write(
            "contest.yaml",
            r#"
schema: 2
contests:
  TEST:
    name: Test Contest
    profiles: [hf-cw]
    value_sets:
      local-final:
        use: local-base
        exclude: [NC]
      local-base:
        use: locations-without-ga
    setup_fields:
      - id: county
        key: County
        label: County
        type: String:2
        in_sets: [local-final]
    exchange:
      - id: fixed-sent
        label: Fixed
        type: String:2
        adif: STX_STRING
        direction: sent
        fixed: true
        source: County
      - id: mutable-sent
        label: Mutable
        type: String:2
        adif: MY_STATE
        direction: sent
      - id: serial-sent
        label: Serial
        type: Serial:4
        adif: STX
        direction: sent
      - id: serial-received
        label: Serial
        type: Serial:4
        adif: SRX
        direction: received
      - id: location-received
        preset: received-location
    scoring:
      qso_points:
        points: 1
"#,
        );

        let store = ContestRulesStore::load_dirs([&dir.0]).expect("rules load");
        let rules = store.get("TEST").expect("test contest");
        assert_eq!(rules.bands, ["40m", "20m"]);
        assert_eq!(rules.modes, ["CW"]);
        assert_eq!(rules.value_sets["locations-without-ga"], ["SC", "NC"]);
        assert_eq!(rules.value_sets["local-final"], ["SC"]);
        assert_eq!(rules.exchange[4].validation.values, ["SC", "NC"]);

        let columns = rules
            .qso_table
            .columns
            .iter()
            .map(|column| (column.field.as_str(), column.editable))
            .collect::<Vec<_>>();
        assert!(!columns.iter().any(|(field, _)| *field == "STX_STRING"));
        assert!(columns.contains(&("MY_STATE", true)));
        assert!(columns.contains(&("STX", false)));
        assert!(columns.contains(&("SRX", true)));
        assert!(columns.contains(&("SRX_STRING", true)));
        assert!(columns.contains(&("pts", false)));

        let api = serde_json::to_value(rules).expect("serialize resolved rules");
        assert_eq!(api["id"], "TEST");
        assert_eq!(api["exchange"][4]["input"]["kind"], "string");
        assert_eq!(api["exchange"][4]["validation"]["values"][0], "SC");
        assert!(api["exchange"][4].get("preset").is_none());
        assert!(api["exchange"][4].get("in_sets").is_none());
        assert!(api.get("allowed_bands").is_none());
    }

    #[test]
    fn explicit_qso_table_replaces_the_derived_table() {
        let dir = TestDir::new();
        dir.write(
            "table.yaml",
            r#"
schema: 2
contests:
  TEST:
    bands: [20]
    modes: [CW]
    exchange: []
    qso_table:
      columns:
        - id: custom
          label: Custom
          source: adif
          field: COMMENT
          editable: false
"#,
        );

        let store = ContestRulesStore::load_dirs([&dir.0]).expect("rules load");
        let columns = &store.get("TEST").expect("test contest").qso_table.columns;
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].id, "custom");
        assert_eq!(columns[0].field, "COMMENT");
    }

    #[test]
    fn keyed_overlays_patch_remove_and_place_entries() {
        let base: YamlValue =
            serde_yaml::from_str("items: [{id: one, value: 1}, {id: two, value: 2}]").unwrap();
        let overlay: YamlValue = serde_yaml::from_str(
            "items: [{id: one, value: 3}, {id: two, remove: true}, {id: zero, value: 0, before: one}]",
        )
        .unwrap();
        let merged = merge_yaml(base, overlay, "test").unwrap();
        let ids = merged["items"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|value| item_id(value).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["zero", "one"]);
        assert_eq!(merged["items"][1]["value"].as_i64(), Some(3));
    }

    #[test]
    fn rejects_reference_cycles() {
        let dir = TestDir::new();
        dir.write(
            "cycle.yaml",
            "schema: 2\nprofiles:\n  one: {profiles: [two]}\n  two: {profiles: [one]}\ncontests:\n  TEST: {profiles: [one]}\n",
        );
        let error = ContestRulesStore::load_dirs([&dir.0]).expect_err("cycle should fail");
        assert!(error.contains("reference cycle"));
    }

    #[test]
    fn rejects_legacy_schema_and_value_set_path_traversal() {
        let legacy = TestDir::new();
        legacy.write("legacy.yaml", "schema: 1\ncontests: {}\n");
        let error = ContestRulesStore::load_dirs([&legacy.0]).expect_err("v1 should fail");
        assert!(error.contains("expected 2"));

        let traversal = TestDir::new();
        traversal.write(
            "traversal.yaml",
            r#"
schema: 2
value_sets:
  unsafe:
    values_from_file: ../outside.dat
contests:
  TEST:
    bands: [20]
    modes: [CW]
    exchange: []
"#,
        );
        let error =
            ContestRulesStore::load_dirs([&traversal.0]).expect_err("traversal should fail");
        assert!(error.contains("must be a file name"));
    }

    #[test]
    fn bundled_catalog_resolves_all_contest_variants() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir]).expect("bundled rules load");

        assert_eq!(store.summaries().len(), 123);
        for id in [
            "ARRL-10",
            "ARRL-SS-CW",
            "CQ-WW-RTTY",
            "CQ-WPX-RTTY",
            "CQ-WPX-SSB",
            "NA-SPRINT-CW (North America)",
            "SC-QSO-PARTY (In State)",
            "ARRL-DIGITAL",
            "ARRL-RTTY",
            "IARU-HF",
            "CQ-160-CW",
            "NJ-QSO-PARTY (In State)",
            "TX-QSO-PARTY (In State)",
            "CO-QSO-PARTY (In State)",
            "ME-QSO-PARTY (In State)",
            "CA-QSO-PARTY (In State)",
            "AZ-QSO-PARTY (In State)",
            "PA-QSO-PARTY (In State)",
            "SD-QSO-PARTY (In State)",
            "NY-QSO-PARTY (In State)",
            "IL-QSO-PARTY (In State)",
            "VT-QSO-PARTY (In State)",
            "MN-QSO-PARTY (In State)",
            "NC-QSO-PARTY (In State)",
            "OK-QSO-PARTY (In State)",
            "ID-QSO-PARTY (In State)",
            "WI-QSO-PARTY (In State)",
            "VA-QSO-PARTY (In State)",
            "LA-QSO-PARTY (In State)",
            "MS-QSO-PARTY (In State)",
            "NM-QSO-PARTY (In State)",
            "MO-QSO-PARTY (In State)",
            "GA-QSO-PARTY (In State)",
            "ND-QSO-PARTY (In State)",
            "MI-QSO-PARTY (In State)",
            "NE-QSO-PARTY (In State)",
            "7-QSO-PARTY (In State)",
            "IN-QSO-PARTY (In State)",
            "DE-QSO-PARTY (In State)",
            "NEW-ENGLAND-QSO-PARTY (In State)",
            "AR-QSO-PARTY (In State)",
            "KY-QSO-PARTY (In State)",
            "AL-QSO-PARTY (In State)",
            "7-QSO-PARTY",
            "IN-QSO-PARTY",
            "DE-QSO-PARTY",
            "NEW-ENGLAND-QSO-PARTY",
            "AR-QSO-PARTY",
            "KY-QSO-PARTY",
            "AL-QSO-PARTY",
            "FL-QSO-PARTY (In State)",
        ] {
            assert!(store.get(id).is_some(), "missing bundled contest {id}");
        }
        for rules in store.contests.values() {
            validate_ids(
                rules
                    .qso_table
                    .columns
                    .iter()
                    .map(|column| column.id.as_str()),
                "QSO table column",
            )
            .unwrap_or_else(|error| panic!("{}: {error}", rules.id));
        }
    }
}
