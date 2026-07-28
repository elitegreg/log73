use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};
use tracing::info;

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueSet {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeField {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub adif: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_param: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub in_sets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub valid_values: Vec<String>,
    /// When set, this exchange is required only for matching contacts and must
    /// otherwise be blank.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only_when: Option<ScoringCondition>,
    pub is_sent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContestParam {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub in_sets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub valid_values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserve_case: Option<bool>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub multi_single_has_mult_transmitter: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CabrilloFixedField {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CabrilloRules {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contest_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixed_fields: Vec<CabrilloFixedField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub log_fields: Vec<ContestParam>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub export_fields: Vec<ContestParam>,
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
    pub in_set: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub in_sets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub valid_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QsoPointRule {
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
    pub category_band_param: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeographyQsoPoints {
    pub country_field: String,
    pub station_country_field: String,
    pub continent_field: String,
    pub station_continent_field: String,
    pub same_country: i64,
    pub different_country_north_america: i64,
    pub different_country_same_continent: i64,
    pub different_continent: i64,
    #[serde(default)]
    pub unresolved: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiplierRule {
    pub name: String,
    pub field: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub in_sets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub valid_values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<ScoringCondition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_call_suffixes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_values: Vec<String>,
    /// An optional literal used to collapse every matching contact into one multiplier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BonusPointRule {
    pub name: String,
    pub field: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key: Vec<String>,
    pub values: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamMultiplierRule {
    pub param: String,
    pub values: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiplierCountBonusRule {
    pub name: String,
    pub multiplier: String,
    pub thresholds: BTreeMap<usize, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContestRules {
    pub contest: String,
    #[serde(default)]
    pub display_name: String,
    pub allowed_bands: Vec<String>,
    pub allowed_modes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub define: Vec<ValueSet>,
    pub exchange: Vec<ExchangeField>,
    pub qso_columns: Vec<String>,
    pub qso_column_fields: BTreeMap<String, String>,
    #[serde(default)]
    pub log_params: Vec<ContestParam>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cabrillo: Option<CabrilloRules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ContestMetadata>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContestSummary {
    pub contest: String,
    pub display_name: String,
    pub log_params: Vec<ContestParam>,
}

#[derive(Debug, Clone)]
pub struct ContestRulesStore {
    contests: BTreeMap<String, ContestRules>,
}

#[derive(Debug, Deserialize)]
struct RulesFile {
    contests: Vec<RawContestRules>,
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
    id: String,
    #[serde(default)]
    extends: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    allowed_bands: Option<Vec<AllowedBandValue>>,
    #[serde(default)]
    allowed_modes: Option<Vec<String>>,
    #[serde(default)]
    define: Option<Vec<RawValueSet>>,
    #[serde(default)]
    exchange: Option<Vec<ExchangeField>>,
    #[serde(default)]
    qso_columns: Option<Vec<String>>,
    #[serde(default)]
    qso_column_fields: Option<BTreeMap<String, String>>,
    #[serde(default)]
    log_params: Option<Vec<ContestParam>>,
    #[serde(default)]
    cabrillo: Option<RawCabrilloRules>,
    #[serde(default)]
    scoring: Option<RawScoringRules>,
    #[serde(default)]
    qso_points: Option<QsoPoints>,
    #[serde(default)]
    dupe_key: Option<Vec<String>>,
    #[serde(default)]
    multipliers: Option<Vec<MultiplierRule>>,
    #[serde(default)]
    bonus_points: Option<Vec<BonusPointRule>>,
    #[serde(default)]
    param_multipliers: Option<Vec<ParamMultiplierRule>>,
    #[serde(default)]
    multiplier_count_bonus_points: Option<Vec<MultiplierCountBonusRule>>,
    #[serde(default)]
    metadata: Option<ContestMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawValueSet {
    name: String,
    #[serde(default)]
    values: Option<Vec<String>>,
    #[serde(default)]
    values_from_file: Option<PathBuf>,
    #[serde(default)]
    exclude: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawCabrilloRules {
    #[serde(default)]
    contest_id: Option<String>,
    #[serde(default)]
    fixed_fields: Option<Vec<CabrilloFixedField>>,
    #[serde(default)]
    log_fields: Option<Vec<ContestParam>>,
    #[serde(default)]
    export_fields: Option<Vec<ContestParam>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScoringRules {
    #[serde(default)]
    qso_points: Option<QsoPoints>,
    #[serde(default)]
    dupe_key: Option<Vec<String>>,
    #[serde(default)]
    multipliers: Option<Vec<MultiplierRule>>,
    #[serde(default)]
    bonus_points: Option<Vec<BonusPointRule>>,
    #[serde(default)]
    param_multipliers: Option<Vec<ParamMultiplierRule>>,
    #[serde(default)]
    multiplier_count_bonus_points: Option<Vec<MultiplierCountBonusRule>>,
}

fn allowed_band_name(value: &AllowedBandValue) -> String {
    match value {
        AllowedBandValue::Name(name) => name.trim().to_string(),
        AllowedBandValue::Meters(meters) => format!("{meters}m"),
    }
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
        let mut raw_contests = BTreeMap::new();

        info!(
            paths = %format_paths(&search_paths),
            "searching contest rules directories"
        );
        for path in &search_paths {
            let stats = load_raw_contests_dir(path, &mut raw_contests)?;
            info!(
                path = %path.display(),
                yaml_files = stats.yaml_files,
                contests = stats.contests,
                "finished contest rules directory"
            );
        }

        let mut contests = BTreeMap::new();
        let ids = raw_contests.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let contest = resolve_contest(
                &id,
                &raw_contests,
                &search_paths,
                &mut contests,
                &mut Vec::new(),
            )?;
            contests.insert(id, contest);
        }

        if contests.is_empty() {
            return Err(format!(
                "no contest rules found in searched directories: {}",
                format_paths(&search_paths)
            ));
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
                contest: contest.contest.clone(),
                display_name: contest.display_name.clone(),
                log_params: contest.log_params.clone(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ContestRulesDirStats {
    yaml_files: usize,
    contests: usize,
}

fn load_raw_contests_dir(
    path: &Path,
    raw_contests: &mut BTreeMap<String, RawContestRules>,
) -> Result<ContestRulesDirStats, String> {
    info!(path = %path.display(), "looking for contest rules directory");
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            info!(path = %path.display(), "contest rules directory not found; skipping");
            return Ok(ContestRulesDirStats::default());
        }
        Err(error) => {
            return Err(format!(
                "unable to read contest rules dir {}: {error}",
                path.display()
            ));
        }
    };

    let mut stats = ContestRulesDirStats::default();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("unable to read contest rules entry: {error}"))?;
        let path = entry.path();
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            continue;
        };
        if extension != "yaml" && extension != "yml" {
            continue;
        }
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
        let rules_file: RulesFile = serde_yaml::from_str(&text)
            .map_err(|error| format!("unable to parse {}: {error}", path.display()))?;
        let contest_count = rules_file.contests.len();
        info!(
            path = %path.display(),
            contests = contest_count,
            "loaded contest rules file"
        );
        stats.yaml_files += 1;
        stats.contests += contest_count;
        for contest in rules_file.contests {
            raw_contests.insert(contest.id.clone(), contest);
        }
    }

    Ok(stats)
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

fn resolve_value_set(raw: &RawValueSet, search_paths: &[PathBuf]) -> Result<ValueSet, String> {
    let values = match (&raw.values, &raw.values_from_file) {
        (Some(_), Some(file_name)) => {
            return Err(format!(
                "value set {} defines both values and values_from_file ({})",
                raw.name,
                file_name.display()
            ));
        }
        (Some(values), None) => values.clone(),
        (None, Some(file_name)) => {
            let path = value_set_file_path(file_name, search_paths)?;
            fs::read_to_string(&path)
                .map_err(|error| {
                    format!("unable to read value-set file {}: {error}", path.display())
                })?
                .lines()
                .map(str::trim)
                .filter(|value| !value.is_empty() && !value.starts_with('#'))
                .map(str::to_string)
                .collect()
        }
        (None, None) => {
            return Err(format!(
                "value set {} must define values or values_from_file",
                raw.name
            ));
        }
    };
    let excluded = raw
        .exclude
        .iter()
        .map(|value| value.trim())
        .collect::<Vec<_>>();

    Ok(ValueSet {
        name: raw.name.clone(),
        values: values
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty() && !excluded.contains(&value.as_str()))
            .collect(),
    })
}

fn resolve_value_sets(
    raw_sets: &[RawValueSet],
    search_paths: &[PathBuf],
) -> Result<Vec<ValueSet>, String> {
    raw_sets
        .iter()
        .map(|raw| resolve_value_set(raw, search_paths))
        .collect()
}

fn apply_defines(current: &mut Vec<ValueSet>, updates: &[ValueSet]) {
    for update in updates {
        if let Some(existing) = current
            .iter_mut()
            .find(|value_set| value_set.name == update.name)
        {
            *existing = update.clone();
        } else {
            current.push(update.clone());
        }
    }
}

fn defined_values(define: &[ValueSet], in_sets: &[String]) -> Result<Vec<String>, String> {
    let mut values = Vec::new();
    for set_name in in_sets {
        if set_name == "*" {
            continue;
        }
        let value_set = define
            .iter()
            .find(|value_set| &value_set.name == set_name)
            .ok_or_else(|| format!("unknown value set referenced by in_sets: {set_name}"))?;
        values.extend(value_set.values.clone());
    }
    Ok(values)
}

fn scoring_condition_in_sets(condition: &ScoringCondition) -> Vec<String> {
    let mut in_sets = condition.in_sets.clone();
    if let Some(in_set) = &condition.in_set {
        in_sets.push(in_set.clone());
    }
    in_sets
}

fn resolve_scoring_condition_in_sets(
    define: &[ValueSet],
    condition: &mut ScoringCondition,
) -> Result<(), String> {
    let in_sets = scoring_condition_in_sets(condition);
    if !in_sets.is_empty() {
        condition.valid_values = defined_values(define, &in_sets)?;
    }
    Ok(())
}

const STANDARD_QSO_COLUMNS: &[&str] = &["Date/Time (UTC)", "Freq", "Mode", "Call"];
const SERIAL_BATCH_SIZE_PARAM: &str = "SERIAL_BATCH_SIZE";
const DEFAULT_SERIAL_BATCH_SIZE: i64 = 10;

fn prepend_standard_qso_columns(contest: &mut ContestRules) {
    let existing = contest.qso_columns.clone();
    contest.qso_columns = STANDARD_QSO_COLUMNS
        .iter()
        .map(|column| (*column).to_string())
        .chain(
            existing
                .into_iter()
                .filter(|column| !STANDARD_QSO_COLUMNS.contains(&column.as_str())),
        )
        .collect();
}

fn field_type_kind(field_type: &str) -> String {
    field_type
        .split(':')
        .next()
        .unwrap_or("STRING")
        .trim()
        .to_uppercase()
}

fn is_sent_serial_field(field: &ExchangeField) -> bool {
    field.is_sent && field_type_kind(&field.field_type) == "SERIAL"
}

fn ensure_serial_batch_size_param(contest: &mut ContestRules) {
    if !contest.exchange.iter().any(is_sent_serial_field) {
        return;
    }
    if contest
        .log_params
        .iter()
        .any(|param| param.name == SERIAL_BATCH_SIZE_PARAM)
    {
        return;
    }

    contest.log_params.push(ContestParam {
        name: SERIAL_BATCH_SIZE_PARAM.to_string(),
        label: "Serial Batch Size".to_string(),
        field_type: "Numeric:4".to_string(),
        required: Some(true),
        regex: None,
        default: Some(Value::from(DEFAULT_SERIAL_BATCH_SIZE)),
        in_sets: Vec::new(),
        valid_values: Vec::new(),
        widget: None,
        help_text: Some(
            "How many sent serial numbers to reserve at a time for offline logging.".to_string(),
        ),
        max_lines: None,
        preserve_case: None,
        multi_single_has_mult_transmitter: false,
    });
}

fn apply_field_valid_values(
    fields: &mut [ContestParam],
    define: &[ValueSet],
) -> Result<(), String> {
    for field in fields {
        if !field.in_sets.is_empty() {
            field.valid_values = defined_values(define, &field.in_sets)?;
        }
    }
    Ok(())
}

fn apply_scoring_rules(contest: &mut ContestRules, scoring: &RawScoringRules) {
    if let Some(qso_points) = &scoring.qso_points {
        contest.qso_points = Some(qso_points.clone());
    }
    if let Some(dupe_key) = &scoring.dupe_key {
        contest.dupe_key = dupe_key.clone();
    }
    if let Some(multipliers) = &scoring.multipliers {
        contest.multipliers = multipliers.clone();
    }
    if let Some(bonus_points) = &scoring.bonus_points {
        contest.bonus_points = bonus_points.clone();
    }
    if let Some(param_multipliers) = &scoring.param_multipliers {
        contest.param_multipliers = param_multipliers.clone();
    }
    if let Some(multiplier_count_bonus_points) = &scoring.multiplier_count_bonus_points {
        contest.multiplier_count_bonus_points = multiplier_count_bonus_points.clone();
    }
}

fn apply_cabrillo_rules(contest: &mut ContestRules, cabrillo: &RawCabrilloRules) {
    let current = contest.cabrillo.get_or_insert_with(CabrilloRules::default);
    if let Some(contest_id) = &cabrillo.contest_id {
        current.contest_id = Some(contest_id.clone());
    }
    if let Some(fixed_fields) = &cabrillo.fixed_fields {
        current.fixed_fields = fixed_fields.clone();
    }
    if let Some(log_fields) = &cabrillo.log_fields {
        current.log_fields = log_fields.clone();
    }
    if let Some(export_fields) = &cabrillo.export_fields {
        current.export_fields = export_fields.clone();
    }
}

fn resolve_in_sets(contest: &mut ContestRules) -> Result<(), String> {
    apply_field_valid_values(&mut contest.log_params, &contest.define)?;

    for field in &mut contest.exchange {
        if !field.in_sets.is_empty() {
            field.valid_values = defined_values(&contest.define, &field.in_sets)?;
        }
        if let Some(condition) = &mut field.only_when {
            resolve_scoring_condition_in_sets(&contest.define, condition)?;
        }
    }

    if let Some(qso_points) = &mut contest.qso_points {
        for rule in &mut qso_points.rules {
            if let Some(condition) = &mut rule.when {
                resolve_scoring_condition_in_sets(&contest.define, condition)?;
            }
            for condition in &mut rule.when_all {
                resolve_scoring_condition_in_sets(&contest.define, condition)?;
            }
        }
    }

    for multiplier in &mut contest.multipliers {
        if !multiplier.in_sets.is_empty() {
            multiplier.valid_values = defined_values(&contest.define, &multiplier.in_sets)?;
        }
        if let Some(condition) = &mut multiplier.when {
            resolve_scoring_condition_in_sets(&contest.define, condition)?;
        }
    }

    if let Some(cabrillo) = &mut contest.cabrillo {
        apply_field_valid_values(&mut cabrillo.log_fields, &contest.define)?;
        apply_field_valid_values(&mut cabrillo.export_fields, &contest.define)?;
    }

    Ok(())
}

fn scoring_param<'a>(contest: &'a ContestRules, name: &str) -> Option<&'a ContestParam> {
    contest
        .log_params
        .iter()
        .chain(
            contest
                .cabrillo
                .iter()
                .flat_map(|cabrillo| cabrillo.log_fields.iter()),
        )
        .find(|param| param.name.eq_ignore_ascii_case(name))
}

fn validate_scoring_config(contest: &ContestRules) -> Result<(), String> {
    for multiplier in &contest.param_multipliers {
        let param = scoring_param(contest, &multiplier.param).ok_or_else(|| {
            format!(
                "param_multipliers references unknown log parameter: {}",
                multiplier.param
            )
        })?;
        if multiplier.values.is_empty() {
            return Err(format!(
                "param_multipliers for {} must define at least one value",
                multiplier.param
            ));
        }
        for (value, factor) in &multiplier.values {
            if *factor <= 0 {
                return Err(format!(
                    "param_multipliers factor for {}={} must be positive",
                    multiplier.param, value
                ));
            }
            if !param.valid_values.is_empty()
                && !param
                    .valid_values
                    .iter()
                    .any(|valid| valid.eq_ignore_ascii_case(value))
            {
                return Err(format!(
                    "param_multipliers value {} is not valid for {}",
                    value, multiplier.param
                ));
            }
        }
    }

    for bonus in &contest.multiplier_count_bonus_points {
        if !contest
            .multipliers
            .iter()
            .any(|multiplier| multiplier.name.eq_ignore_ascii_case(&bonus.multiplier))
        {
            return Err(format!(
                "multiplier_count_bonus_points references unknown multiplier: {}",
                bonus.multiplier
            ));
        }
        if bonus.thresholds.is_empty() {
            return Err(format!(
                "multiplier_count_bonus_points {} must define at least one threshold",
                bonus.name
            ));
        }
        for (threshold, points) in &bonus.thresholds {
            if *threshold == 0 || *points <= 0 {
                return Err(format!(
                    "multiplier_count_bonus_points {} thresholds and points must be positive",
                    bonus.name
                ));
            }
        }
    }

    Ok(())
}

fn resolve_contest(
    id: &str,
    raw_contests: &BTreeMap<String, RawContestRules>,
    search_paths: &[PathBuf],
    resolved: &mut BTreeMap<String, ContestRules>,
    stack: &mut Vec<String>,
) -> Result<ContestRules, String> {
    if let Some(contest) = resolved.get(id) {
        return Ok(contest.clone());
    }
    if stack.iter().any(|stack_id| stack_id == id) {
        return Err(format!(
            "contest inheritance cycle: {} -> {id}",
            stack.join(" -> ")
        ));
    }

    let raw = raw_contests
        .get(id)
        .ok_or_else(|| format!("contest rules {id} not found"))?;
    stack.push(id.to_string());

    let mut contest = if let Some(parent_id) = &raw.extends {
        resolve_contest(parent_id, raw_contests, search_paths, resolved, stack)?
    } else {
        ContestRules {
            contest: id.to_string(),
            display_name: id.to_string(),
            allowed_bands: Vec::new(),
            allowed_modes: Vec::new(),
            define: Vec::new(),
            exchange: Vec::new(),
            qso_columns: Vec::new(),
            qso_column_fields: BTreeMap::new(),
            log_params: Vec::new(),
            qso_points: None,
            dupe_key: Vec::new(),
            multipliers: Vec::new(),
            bonus_points: Vec::new(),
            param_multipliers: Vec::new(),
            multiplier_count_bonus_points: Vec::new(),
            cabrillo: None,
            metadata: None,
        }
    };

    contest.contest = id.to_string();
    if let Some(display_name) = &raw.display_name {
        contest.display_name = display_name.clone();
    } else if raw.extends.is_none() {
        contest.display_name = id.to_string();
    }
    if let Some(allowed_bands) = &raw.allowed_bands {
        contest.allowed_bands = allowed_bands.iter().map(allowed_band_name).collect();
    }
    if let Some(allowed_modes) = &raw.allowed_modes {
        contest.allowed_modes = allowed_modes.clone();
    }
    if let Some(define) = &raw.define {
        let define = resolve_value_sets(define, search_paths)?;
        apply_defines(&mut contest.define, &define);
    }
    if let Some(exchange) = &raw.exchange {
        contest.exchange = exchange.clone();
    }
    if let Some(qso_columns) = &raw.qso_columns {
        contest.qso_columns = qso_columns.clone();
    }
    if let Some(qso_column_fields) = &raw.qso_column_fields {
        contest.qso_column_fields = qso_column_fields.clone();
    }
    if let Some(log_params) = &raw.log_params {
        contest.log_params = log_params.clone();
    }
    if let Some(cabrillo) = &raw.cabrillo {
        apply_cabrillo_rules(&mut contest, cabrillo);
    }
    apply_scoring_rules(
        &mut contest,
        &RawScoringRules {
            qso_points: raw.qso_points.clone(),
            dupe_key: raw.dupe_key.clone(),
            multipliers: raw.multipliers.clone(),
            bonus_points: raw.bonus_points.clone(),
            param_multipliers: raw.param_multipliers.clone(),
            multiplier_count_bonus_points: raw.multiplier_count_bonus_points.clone(),
        },
    );
    if let Some(scoring) = &raw.scoring {
        apply_scoring_rules(&mut contest, scoring);
    }
    if let Some(metadata) = &raw.metadata {
        contest.metadata = Some(metadata.clone());
    }

    ensure_serial_batch_size_param(&mut contest);
    resolve_in_sets(&mut contest)?;
    validate_scoring_config(&contest)?;
    prepend_standard_qso_columns(&mut contest);

    stack.pop();
    resolved.insert(id.to_string(), contest.clone());
    Ok(contest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "log73-contest-rules-test-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("test dir should be created");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_rules_file(dir: &Path, file_name: &str, yaml: &str) {
        fs::write(dir.join(file_name), yaml).expect("rules file should be written");
    }

    fn resolve_yaml_contest(yaml: &str, id: &str) -> ContestRules {
        let rules_file: RulesFile = serde_yaml::from_str(yaml).expect("yaml should parse");
        let raw_contests = rules_file
            .contests
            .into_iter()
            .map(|contest| (contest.id.clone(), contest))
            .collect::<BTreeMap<_, _>>();

        resolve_contest(
            id,
            &raw_contests,
            &[],
            &mut BTreeMap::new(),
            &mut Vec::new(),
        )
        .expect("contest should resolve")
    }

    #[test]
    fn user_rules_override_installed_rules_and_union_is_loaded() {
        let installed = TestDir::new();
        let user = TestDir::new();

        write_rules_file(
            installed.path(),
            "installed.yaml",
            r#"
contests:
  - id: SHARED
    display_name: Installed Shared
    allowed_bands: [20]
    allowed_modes: ['CW']
    exchange: []
    qso_columns: []
    qso_column_fields: {}
  - id: INSTALLED_ONLY
    display_name: Installed Only
    allowed_bands: [40]
    allowed_modes: ['SSB']
    exchange: []
    qso_columns: []
    qso_column_fields: {}
"#,
        );
        write_rules_file(
            user.path(),
            "user.yaml",
            r#"
contests:
  - id: SHARED
    display_name: User Shared
    allowed_bands: [15]
    allowed_modes: ['RTTY']
    exchange: []
    qso_columns: []
    qso_column_fields: {}
  - id: USER_ONLY
    display_name: User Only
    allowed_bands: [10]
    allowed_modes: ['CW']
    exchange: []
    qso_columns: []
    qso_column_fields: {}
"#,
        );

        let store = ContestRulesStore::load_dirs([installed.path(), user.path()])
            .expect("rules should load");

        assert_eq!(
            store.get("SHARED").map(|contest| &contest.display_name),
            Some(&"User Shared".to_string())
        );
        assert!(store.get("INSTALLED_ONLY").is_some());
        assert!(store.get("USER_ONLY").is_some());
    }

    #[test]
    fn file_backed_value_sets_use_user_data_and_apply_exclusions() {
        let installed = TestDir::new();
        let user = TestDir::new();

        write_rules_file(
            installed.path(),
            "contest.yaml",
            r#"
contests:
  - id: TEST
    allowed_bands: [20]
    allowed_modes: ['CW']
    define:
      - name: Sections
        values_from_file: sections.dat
        exclude: ['SC']
    exchange:
      - name: Section
        type: 'String:3'
        adif: 'SRX_STRING'
        is_sent: false
        in_sets: ['Sections']
    qso_columns: []
    qso_column_fields: {}
"#,
        );
        fs::write(installed.path().join("sections.dat"), "AA\nSC\n")
            .expect("installed data file should be written");
        fs::write(user.path().join("sections.dat"), "BB\nSC\n\n# comment\n")
            .expect("user data file should be written");

        let store = ContestRulesStore::load_dirs([installed.path(), user.path()])
            .expect("rules should load");
        let contest = store.get("TEST").expect("test contest should load");

        assert_eq!(contest.define[0].values, vec!["BB".to_string()]);
        assert_eq!(contest.exchange[0].valid_values, vec!["BB".to_string()]);
    }

    #[test]
    fn file_backed_value_sets_report_missing_and_unsafe_files() {
        let rules = TestDir::new();
        write_rules_file(
            rules.path(),
            "contest.yaml",
            r#"
contests:
  - id: MISSING
    allowed_bands: [20]
    allowed_modes: ['CW']
    define:
      - name: Sections
        values_from_file: missing.dat
    exchange: []
    qso_columns: []
    qso_column_fields: {}
"#,
        );
        let error = ContestRulesStore::load_dirs([rules.path()])
            .expect_err("a missing value-set file should fail loading");
        assert!(error.contains("missing.dat"));
        assert!(error.contains("not found"));

        write_rules_file(
            rules.path(),
            "contest.yaml",
            r#"
contests:
  - id: UNSAFE
    allowed_bands: [20]
    allowed_modes: ['CW']
    define:
      - name: Sections
        values_from_file: ../outside.dat
    exchange: []
    qso_columns: []
    qso_column_fields: {}
"#,
        );
        let error = ContestRulesStore::load_dirs([rules.path()])
            .expect_err("a path outside contest-rules should fail loading");
        assert!(error.contains("must be a file name"));
    }

    #[test]
    fn missing_rules_dirs_are_ignored_when_other_rules_exist() {
        let installed = TestDir::new();
        let missing_user = TestDir::new();
        let missing_user_path = missing_user.path().to_path_buf();
        drop(missing_user);

        write_rules_file(
            installed.path(),
            "installed.yaml",
            r#"
contests:
  - id: INSTALLED_ONLY
    display_name: Installed Only
    allowed_bands: [40]
    allowed_modes: ['SSB']
    exchange: []
    qso_columns: []
    qso_column_fields: {}
"#,
        );

        let store = ContestRulesStore::load_dirs([installed.path(), missing_user_path.as_path()])
            .expect("rules should load");

        assert!(store.get("INSTALLED_ONLY").is_some());
    }

    #[test]
    fn empty_rules_error_lists_searched_dirs() {
        let first = TestDir::new();
        let second = TestDir::new();
        let first_path = first.path().to_path_buf();
        let second_path = second.path().to_path_buf();
        drop(first);
        drop(second);

        let error = ContestRulesStore::load_dirs([first_path.as_path(), second_path.as_path()])
            .expect_err("missing rules should fail when no other rules exist");

        assert!(error.contains("no contest rules found"));
        assert!(error.contains(&first_path.display().to_string()));
        assert!(error.contains(&second_path.display().to_string()));
    }

    #[test]
    fn nested_scoring_block_populates_internal_scoring_fields() {
        let contest = resolve_yaml_contest(
            r#"
contests:
  - id: TEST
    allowed_bands: [20]
    allowed_modes: ['CW']
    define:
      - name: 'Modes'
        values: ['CW']
      - name: 'Sections'
        values: ['SC']
    exchange: []
    qso_columns: []
    qso_column_fields: {}
    scoring:
      qso_points:
        rules:
          - when:
              field: 'MODE'
              in_set: 'Modes'
            points: 2
      dupe_key: ['CALL', 'MODE']
      multipliers:
        - name: 'Section'
          field: 'SECTION'
          key: ['SECTION']
          in_sets: ['Sections']
      bonus_points:
        - name: 'Bonus Station'
          field: 'CALL'
          key: ['CALL']
          values:
            W1AW: 100
      param_multipliers:
        - param: 'CATEGORY-POWER'
          values:
            HIGH: 1
            LOW: 2
            QRP: 5
      multiplier_count_bonus_points:
        - name: 'Section Sweep'
          multiplier: 'Section'
          thresholds:
            2: 100
    cabrillo:
      log_fields:
        - name: 'CATEGORY-POWER'
          label: 'Category Power'
          type: 'String:16'
          valid_values: ['HIGH', 'LOW', 'QRP']
"#,
            "TEST",
        );

        let qso_points = contest.qso_points.expect("qso points should be set");
        assert_eq!(qso_points.rules.len(), 1);
        assert_eq!(qso_points.rules[0].points, 2);
        assert_eq!(
            qso_points.rules[0]
                .when
                .as_ref()
                .expect("condition should exist")
                .valid_values,
            vec!["CW".to_string()]
        );
        assert_eq!(
            contest.dupe_key,
            vec!["CALL".to_string(), "MODE".to_string()]
        );
        assert_eq!(contest.multipliers.len(), 1);
        assert_eq!(contest.multipliers[0].valid_values, vec!["SC".to_string()]);
        assert_eq!(contest.bonus_points.len(), 1);
        assert_eq!(contest.bonus_points[0].values.get("W1AW"), Some(&100));
        assert_eq!(contest.param_multipliers.len(), 1);
        assert_eq!(contest.param_multipliers[0].values.get("LOW"), Some(&2));
        assert_eq!(contest.multiplier_count_bonus_points.len(), 1);
    }

    #[test]
    fn removed_power_multiplier_is_rejected() {
        let rules = TestDir::new();
        write_rules_file(
            rules.path(),
            "contest.yaml",
            r#"
contests:
  - id: TEST
    allowed_bands: [20]
    allowed_modes: ['CW']
    exchange: []
    qso_columns: []
    qso_column_fields: {}
    scoring:
      power_multiplier: [1, 2, 5]
"#,
        );

        let error = ContestRulesStore::load_dirs([rules.path()])
            .expect_err("removed power_multiplier should fail loading");
        assert!(error.contains("power_multiplier"));
        assert!(error.contains("unknown field"));
    }

    #[test]
    fn nested_scoring_fields_override_flat_fields_after_inheritance() {
        let contest = resolve_yaml_contest(
            r#"
contests:
  - id: BASE
    allowed_bands: [20]
    allowed_modes: ['CW']
    exchange: []
    qso_columns: []
    qso_column_fields: {}
    qso_points:
      points: 1
    dupe_key: ['CALL']
  - id: CHILD
    extends: BASE
    scoring:
      dupe_key: ['CALL', 'BAND']
"#,
            "CHILD",
        );

        assert_eq!(contest.qso_points.and_then(|points| points.points), Some(1));
        assert_eq!(
            contest.dupe_key,
            vec!["CALL".to_string(), "BAND".to_string()]
        );
    }

    #[test]
    fn cabrillo_fields_inherit_and_resolve_valid_values() {
        let contest = resolve_yaml_contest(
            r#"
contests:
  - id: BASE
    allowed_bands: [20]
    allowed_modes: ['CW']
    define:
      - name: 'Modes'
        values: ['CW', 'SSB']
    exchange: []
    qso_columns: []
    qso_column_fields: {}
    cabrillo:
      fixed_fields:
        - name: 'CATEGORY-BAND'
          value: 'ALL'
      log_fields:
        - name: 'CATEGORY-MODE'
          label: 'Category Mode'
          type: 'String:8'
          widget: 'select'
          in_sets: ['Modes']
      export_fields:
        - name: 'NAME'
          label: 'Name'
          type: 'String:75'
          preserve_case: true
  - id: CHILD
    extends: BASE
    cabrillo:
      export_fields:
        - name: 'EMAIL'
          label: 'Email'
          type: 'String:75'
"#,
            "CHILD",
        );

        let cabrillo = contest.cabrillo.expect("cabrillo should exist");
        assert_eq!(cabrillo.fixed_fields.len(), 1);
        assert_eq!(cabrillo.log_fields.len(), 1);
        assert_eq!(
            cabrillo.log_fields[0].valid_values,
            vec!["CW".to_string(), "SSB".to_string()]
        );
        assert_eq!(cabrillo.export_fields.len(), 1);
        assert_eq!(cabrillo.export_fields[0].name, "EMAIL");
    }

    #[test]
    fn bundled_sc_qso_party_rules_resolve_file_backed_value_sets() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let contest = store
            .get("SC-QSO-PARTY")
            .expect("SC QSO Party rules should load");

        let states = contest
            .define
            .iter()
            .find(|value_set| value_set.name == "States")
            .expect("States value set should exist");
        assert!(states.values.contains(&"AL".to_string()));
        assert!(!states.values.contains(&"SC".to_string()));

        let received_state = contest
            .exchange
            .iter()
            .find(|field| field.name == "State")
            .expect("received State exchange field should exist");
        assert!(received_state.valid_values.contains(&"DC".to_string()));
        assert!(received_state.valid_values.contains(&"AB".to_string()));
    }

    #[test]
    fn bundled_hi_qso_party_rules_resolve_both_locations() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let in_state = store
            .get("HI-QSO-PARTY (In State)")
            .expect("in-state Hawaii QSO Party rules should load");
        let outside = store
            .get("HI-QSO-PARTY")
            .expect("outside-Hawaii QSO Party rules should load");

        let districts = in_state
            .define
            .iter()
            .find(|value_set| value_set.name == "Hawaii Districts")
            .expect("Hawaii districts should exist");
        assert_eq!(districts.values.len(), 14);
        assert!(districts.values.contains(&"HIL".to_string()));
        assert!(districts.values.contains(&"LNI".to_string()));
        assert!(districts.values.contains(&"WHN".to_string()));

        let states = in_state
            .define
            .iter()
            .find(|value_set| value_set.name == "States")
            .expect("states should exist");
        assert!(!states.values.contains(&"HI".to_string()));
        assert!(states.values.contains(&"AK".to_string()));

        assert_eq!(in_state.allowed_modes, vec!["CW", "SSB"]);
        assert_eq!(in_state.log_params[0].name, "District");
        assert_eq!(outside.log_params[0].name, "Location");
        assert_eq!(
            outside.multipliers[0].key,
            vec!["SRX_STRING".to_string(), "BAND".to_string()]
        );

        let received = outside
            .exchange
            .iter()
            .find(|field| field.name == "District")
            .expect("outside-Hawaii received district should exist");
        assert_eq!(received.valid_values.len(), 14);

        for rules in [in_state, outside] {
            let cabrillo = rules
                .cabrillo
                .as_ref()
                .expect("Hawaii QSO Party Cabrillo rules should exist");
            assert_eq!(cabrillo.contest_id.as_deref(), Some("HI-QSO-PARTY"));
            let transmitter = cabrillo
                .log_fields
                .iter()
                .find(|field| field.name == "CATEGORY-TRANSMITTER")
                .expect("category transmitter should exist");
            assert_eq!(transmitter.default, Some(Value::String("ONE".to_string())));
            assert_eq!(transmitter.valid_values, vec!["ONE", "UNLIMITED"]);
        }
    }

    #[test]
    fn bundled_mdc_qso_party_rules_resolve_both_locations() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let in_state = store
            .get("MDC-QSO-PARTY (In State)")
            .expect("in-state MDC QSO Party rules should load");
        let outside = store
            .get("MDC-QSO-PARTY")
            .expect("outside-MDC QSO Party rules should load");

        let jurisdictions = in_state
            .define
            .iter()
            .find(|value_set| value_set.name == "Counties")
            .expect("Counties should exist");
        assert_eq!(jurisdictions.values.len(), 25);
        assert!(jurisdictions.values.contains(&"BAL".to_string()));
        assert!(jurisdictions.values.contains(&"BCT".to_string()));
        assert!(jurisdictions.values.contains(&"WDC".to_string()));

        let states = in_state
            .define
            .iter()
            .find(|value_set| value_set.name == "States")
            .expect("states should exist");
        assert!(!states.values.contains(&"MD".to_string()));
        assert!(states.values.contains(&"AK".to_string()));
        assert!(states.values.contains(&"HI".to_string()));

        assert_eq!(in_state.param_multipliers.len(), 2);
        assert_eq!(in_state.param_multipliers[0].values.get("QRP"), Some(&3));
        assert_eq!(in_state.param_multipliers[1].values.get("ROVER"), Some(&4));
        assert_eq!(in_state.multiplier_count_bonus_points.len(), 1);
        assert_eq!(
            in_state.multiplier_count_bonus_points[0]
                .thresholds
                .get(&25),
            Some(&500)
        );
        assert_eq!(
            in_state
                .cabrillo
                .as_ref()
                .and_then(|cabrillo| cabrillo.contest_id.as_deref()),
            Some("MDC-QSO-PARTY")
        );

        assert_eq!(outside.multipliers.len(), 1);
        assert_eq!(outside.multipliers[0].name, "County");
        assert_eq!(outside.log_params[0].name, "Location");
        let in_state_received = in_state
            .exchange
            .iter()
            .find(|field| field.name == "Location" && !field.is_sent)
            .expect("in-state MDC received location should exist");
        assert_eq!(in_state_received.field_type, "String:16");
        assert!(
            in_state_received
                .in_sets
                .iter()
                .any(|set_name| set_name == "*")
        );
        assert!(in_state_received.valid_values.contains(&"BAL".to_string()));
        assert!(in_state_received.valid_values.contains(&"SC".to_string()));
        assert!(in_state_received.valid_values.contains(&"SK".to_string()));
        assert!(!in_state_received.valid_values.contains(&"DX".to_string()));
        let received = outside
            .exchange
            .iter()
            .find(|field| field.name == "County")
            .expect("outside-MDC received county should exist");
        assert_eq!(received.valid_values.len(), 25);
    }

    #[test]
    fn bundled_tn_qso_party_rules_resolve_both_locations() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let in_state = store
            .get("TN-QSO-PARTY (In State)")
            .expect("in-state Tennessee QSO Party rules should load");
        let outside = store
            .get("TN-QSO-PARTY")
            .expect("outside-Tennessee QSO Party rules should load");

        let counties = in_state
            .define
            .iter()
            .find(|value_set| value_set.name == "Tennessee Counties")
            .expect("Tennessee Counties should exist");
        assert_eq!(counties.values.len(), 95);
        assert!(counties.values.contains(&"ANDE".to_string()));
        assert!(counties.values.contains(&"WILS".to_string()));

        let states = in_state
            .define
            .iter()
            .find(|value_set| value_set.name == "States")
            .expect("States should exist");
        assert!(!states.values.contains(&"TN".to_string()));
        assert!(states.values.contains(&"AK".to_string()));
        assert!(states.values.contains(&"HI".to_string()));

        assert_eq!(in_state.allowed_modes, vec!["CW", "SSB"]);
        let sent_county = in_state
            .exchange
            .iter()
            .find(|field| field.name == "County" && field.is_sent)
            .expect("in-state sent county should exist");
        assert_eq!(sent_county.fixed, Some(true));

        let received_location = in_state
            .exchange
            .iter()
            .find(|field| field.name == "Location" && !field.is_sent)
            .expect("in-state received location should exist");
        assert_eq!(received_location.field_type, "String:4");
        assert!(
            received_location
                .in_sets
                .iter()
                .any(|set_name| set_name == "*")
        );
        assert!(received_location.valid_values.contains(&"ANDE".to_string()));
        assert!(received_location.valid_values.contains(&"SC".to_string()));
        assert!(received_location.valid_values.contains(&"SK".to_string()));

        assert_eq!(outside.log_params[0].name, "Location");
        assert_eq!(outside.log_params[0].field_type, "String:4");
        assert!(
            outside.log_params[0]
                .in_sets
                .iter()
                .any(|set_name| set_name == "*")
        );
        assert_eq!(outside.multipliers.len(), 1);
        assert_eq!(outside.multipliers[0].name, "Tennessee County");
    }

    #[test]
    fn bundled_oh_qso_party_rules_resolve_both_locations() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let in_state = store
            .get("OH-QSO-PARTY (In State)")
            .expect("in-state Ohio QSO Party rules should load");
        let outside = store
            .get("OH-QSO-PARTY")
            .expect("outside-Ohio QSO Party rules should load");

        let counties = in_state
            .define
            .iter()
            .find(|value_set| value_set.name == "Ohio Counties")
            .expect("Ohio Counties should exist");
        assert_eq!(counties.values.len(), 88);
        assert!(counties.values.contains(&"ADAM".to_string()));
        assert!(counties.values.contains(&"WYAN".to_string()));

        let states = in_state
            .define
            .iter()
            .find(|value_set| value_set.name == "States")
            .expect("States should exist");
        assert!(!states.values.contains(&"OH".to_string()));
        assert!(!states.values.contains(&"DC".to_string()));
        assert!(states.values.contains(&"MD".to_string()));

        assert_eq!(in_state.allowed_modes, vec!["CW", "SSB"]);
        assert_eq!(in_state.log_params[0].name, "County");
        assert_eq!(outside.log_params[0].name, "Location");
        assert_eq!(in_state.exchange[1].fixed, None);
        assert_eq!(outside.exchange[1].fixed, None);
        assert_eq!(outside.multipliers.len(), 1);
        assert_eq!(outside.multipliers[0].key, vec!["SRX_STRING", "MODE"]);
        assert_eq!(
            in_state
                .cabrillo
                .as_ref()
                .and_then(|cabrillo| cabrillo.contest_id.as_deref()),
            Some("OH-QSO-PARTY")
        );
    }

    #[test]
    fn bundled_arrl_field_day_uses_parameter_multiplier() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let contest = store
            .get("ARRL-FIELD-DAY")
            .expect("ARRL Field Day rules should load");

        assert_eq!(contest.param_multipliers.len(), 1);
        assert_eq!(
            contest.param_multipliers[0].values,
            BTreeMap::from([
                ("HIGH".to_string(), 1),
                ("LOW".to_string(), 2),
                ("QRP".to_string(), 5),
            ])
        );
    }

    #[test]
    fn bundled_cqww_rules_resolve_cw_and_ssb_variants() {
        let yaml = include_str!("../../data/contest-rules/cqww.yaml");
        let cw = resolve_yaml_contest(yaml, "CQ-WW-CW");
        let ssb = resolve_yaml_contest(yaml, "CQ-WW-SSB");

        assert_eq!(cw.allowed_modes, vec!["CW".to_string()]);
        assert_eq!(ssb.allowed_modes, vec!["SSB".to_string()]);
        assert_eq!(
            cw.allowed_bands,
            vec!["160m", "80m", "40m", "20m", "15m", "10m"]
        );
        assert_eq!(cw.multipliers.len(), 2);
        let transmitter_field = cw
            .cabrillo
            .as_ref()
            .and_then(|cabrillo| {
                cabrillo
                    .log_fields
                    .iter()
                    .find(|field| field.name == "CATEGORY-TRANSMITTER")
            })
            .expect("CQWW should define CATEGORY-TRANSMITTER");
        assert!(transmitter_field.multi_single_has_mult_transmitter);
        assert_eq!(
            serde_json::to_value(transmitter_field)
                .expect("field should serialize")
                .get("multi_single_has_mult_transmitter"),
            Some(&Value::Bool(true))
        );
        assert!(
            ssb.cabrillo
                .as_ref()
                .and_then(|cabrillo| {
                    cabrillo
                        .log_fields
                        .iter()
                        .find(|field| field.name == "CATEGORY-TRANSMITTER")
                })
                .is_some_and(|field| field.multi_single_has_mult_transmitter)
        );
        assert_eq!(
            cw.qso_points
                .as_ref()
                .and_then(|points| points.geography.as_ref())
                .map(|geography| geography.country_field.as_str()),
            Some("DXCC_PREFIX")
        );
        assert_eq!(
            cw.qso_points
                .as_ref()
                .and_then(|points| points.category_band_param.as_deref()),
            Some("CATEGORY-BAND")
        );
        assert_eq!(
            ssb.cabrillo
                .as_ref()
                .and_then(|cabrillo| cabrillo.fixed_fields.first())
                .map(|field| field.value.as_str()),
            Some("SSB")
        );
    }

    #[test]
    fn bundled_arrl_sweepstakes_rules_resolve_cw_and_ssb_variants() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let cw = store
            .get("ARRL-SS-CW")
            .expect("ARRL Sweepstakes CW rules should load");
        let ssb = store
            .get("ARRL-SS-SSB")
            .expect("ARRL Sweepstakes SSB rules should load");

        assert_eq!(cw.allowed_modes, ["CW"]);
        assert_eq!(ssb.allowed_modes, ["SSB"]);
        assert_eq!(cw.dupe_key, ["CALL"]);
        assert_eq!(
            cw.qso_points.as_ref().and_then(|points| points.points),
            Some(2)
        );
        assert_eq!(cw.multipliers[0].valid_values.len(), 85);
        assert_eq!(cw.exchange[0].field_type, "Serial:4");
        assert_eq!(cw.exchange[0].adif, "STX");
        assert_eq!(cw.exchange[4].adif, "SRX");
        assert_eq!(
            ssb.cabrillo
                .as_ref()
                .and_then(|cabrillo| cabrillo.contest_id.as_deref()),
            Some("ARRL-SS-SSB")
        );
    }

    #[test]
    fn bundled_arrl_160_rules_resolve_conditional_sections() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let contest = store.get("ARRL-160").expect("ARRL 160 rules should load");

        assert_eq!(contest.allowed_bands, ["160m"]);
        assert_eq!(contest.allowed_modes, ["CW"]);
        assert_eq!(contest.dupe_key, ["CALL"]);
        assert_eq!(
            contest.qso_points.as_ref().map(|points| points.rules.len()),
            Some(4)
        );
        let received_section = contest
            .exchange
            .iter()
            .find(|field| field.name == "Section" && !field.is_sent)
            .expect("received Section should exist");
        assert!(received_section.valid_values.contains(&"EMA".to_string()));
        assert_eq!(
            received_section
                .only_when
                .as_ref()
                .map(|condition| condition.valid_values.len()),
            Some(15)
        );
    }

    #[test]
    fn bundled_na_sprint_ssb_rules_resolve_na_and_dx_variants() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let north_america = store
            .get("NA-SPRINT-SSB (North America)")
            .expect("North America NA Sprint rules should load");
        let dx = store
            .get("NA-SPRINT-SSB (DX)")
            .expect("DX NA Sprint rules should load");

        assert_eq!(north_america.allowed_bands, ["80m", "40m", "20m"]);
        assert_eq!(north_america.allowed_modes, ["SSB"]);
        assert_eq!(north_america.dupe_key, ["CALL", "BAND"]);
        assert_eq!(north_america.multipliers[0].valid_values.len(), 106);
        assert!(
            north_america.multipliers[0]
                .valid_values
                .contains(&"FO/C".to_string())
        );

        let north_america_received = north_america
            .exchange
            .iter()
            .find(|field| field.name == "QTH" && !field.is_sent)
            .expect("North America received QTH should exist");
        assert!(
            north_america_received
                .valid_values
                .contains(&"DX".to_string())
        );

        let dx_received = dx
            .exchange
            .iter()
            .find(|field| field.name == "QTH" && !field.is_sent)
            .expect("DX received QTH should exist");
        assert!(!dx_received.valid_values.contains(&"DX".to_string()));
        assert!(dx_received.valid_values.contains(&"MA".to_string()));
        assert_eq!(
            dx.qso_points
                .as_ref()
                .and_then(|points| points.rules.first())
                .map(|rule| rule.points),
            Some(1)
        );
        assert_eq!(
            dx.cabrillo
                .as_ref()
                .and_then(|cabrillo| cabrillo.contest_id.as_deref()),
            Some("NA-SPRINT-SSB")
        );
    }

    #[test]
    fn bundled_na_sprint_cw_rules_resolve_na_and_dx_variants() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let north_america = store
            .get("NA-SPRINT-CW (North America)")
            .expect("North America NA Sprint CW rules should load");
        let dx = store
            .get("NA-SPRINT-CW (DX)")
            .expect("DX NA Sprint CW rules should load");

        assert_eq!(north_america.allowed_bands, ["80m", "40m", "20m"]);
        assert_eq!(north_america.allowed_modes, ["CW"]);
        assert_eq!(north_america.dupe_key, ["CALL", "BAND"]);
        assert!(
            north_america.multipliers[0]
                .valid_values
                .contains(&"4U1UN".to_string())
        );
        assert!(
            north_america.multipliers[0]
                .valid_values
                .contains(&"VP9".to_string())
        );

        let north_america_received = north_america
            .exchange
            .iter()
            .find(|field| field.name == "QTH" && !field.is_sent)
            .expect("North America received QTH should exist");
        assert!(
            north_america_received
                .valid_values
                .contains(&"DX".to_string())
        );

        let dx_received = dx
            .exchange
            .iter()
            .find(|field| field.name == "QTH" && !field.is_sent)
            .expect("DX received QTH should exist");
        assert!(!dx_received.valid_values.contains(&"DX".to_string()));
        assert!(dx_received.valid_values.contains(&"MA".to_string()));
        assert_eq!(
            dx.qso_points
                .as_ref()
                .and_then(|points| points.rules.first())
                .map(|rule| rule.points),
            Some(1)
        );
        let cabrillo = north_america.cabrillo.as_ref().expect("Cabrillo rules");
        assert_eq!(cabrillo.contest_id.as_deref(), Some("NA-SPRINT-CW"));
        assert!(
            cabrillo.fixed_fields.iter().any(|field| {
                field.name == "CATEGORY-ASSISTED" && field.value == "NON-ASSISTED"
            })
        );
    }

    #[test]
    fn bundled_naqp_rules_resolve_cw_and_ssb_na_and_dx_variants() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let cw_north_america = store
            .get("NAQP-CW (North America)")
            .expect("North America NAQP CW rules should load");
        let cw_dx = store
            .get("NAQP-CW (DX)")
            .expect("DX NAQP CW rules should load");
        let ssb_north_america = store
            .get("NAQP-SSB (North America)")
            .expect("North America NAQP SSB rules should load");
        let ssb_dx = store
            .get("NAQP-SSB (DX)")
            .expect("DX NAQP SSB rules should load");

        assert_eq!(
            cw_north_america.allowed_bands,
            ["160m", "80m", "40m", "20m", "15m", "10m"]
        );
        assert_eq!(cw_north_america.allowed_modes, ["CW"]);
        assert_eq!(ssb_north_america.allowed_modes, ["SSB"]);
        assert_eq!(cw_north_america.dupe_key, ["CALL", "BAND"]);
        assert_eq!(cw_north_america.multipliers[0].key, ["SRX_STRING", "BAND"]);
        assert!(
            cw_north_america.multipliers[0]
                .valid_values
                .contains(&"VP9".to_string())
        );
        assert!(cw_dx.log_params.is_empty());
        assert_eq!(cw_dx.exchange.len(), 3);
        assert_eq!(ssb_dx.exchange.len(), 3);
        assert_eq!(
            cw_north_america
                .cabrillo
                .as_ref()
                .and_then(|cabrillo| cabrillo.contest_id.as_deref()),
            Some("NAQP-CW")
        );
        assert_eq!(
            ssb_north_america
                .cabrillo
                .as_ref()
                .and_then(|cabrillo| cabrillo.contest_id.as_deref()),
            Some("NAQP-SSB")
        );
        assert!(
            ssb_north_america
                .cabrillo
                .as_ref()
                .expect("Cabrillo rules")
                .fixed_fields
                .iter()
                .any(|field| field.name == "CATEGORY-BAND" && field.value == "ALL")
        );
    }
}
