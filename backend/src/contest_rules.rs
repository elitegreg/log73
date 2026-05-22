use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path};

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CabrilloFixedField {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CabrilloRules {
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
    pub points: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QsoPoints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<QsoPointRule>,
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
pub struct ContestRules {
    pub contest: String,
    #[serde(default)]
    pub display_name: String,
    pub allowed_bands: Vec<u16>,
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
struct RawContestRules {
    id: String,
    #[serde(default)]
    extends: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    allowed_bands: Option<Vec<u16>>,
    #[serde(default)]
    allowed_modes: Option<Vec<String>>,
    #[serde(default)]
    define: Option<Vec<ValueSet>>,
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
    metadata: Option<ContestMetadata>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawCabrilloRules {
    #[serde(default)]
    fixed_fields: Option<Vec<CabrilloFixedField>>,
    #[serde(default)]
    log_fields: Option<Vec<ContestParam>>,
    #[serde(default)]
    export_fields: Option<Vec<ContestParam>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawScoringRules {
    #[serde(default)]
    qso_points: Option<QsoPoints>,
    #[serde(default)]
    dupe_key: Option<Vec<String>>,
    #[serde(default)]
    multipliers: Option<Vec<MultiplierRule>>,
    #[serde(default)]
    bonus_points: Option<Vec<BonusPointRule>>,
}

impl ContestRulesStore {
    pub fn load_dir(path: impl AsRef<Path>) -> Result<Self, String> {
        let mut raw_contests = BTreeMap::new();
        let entries = fs::read_dir(path.as_ref()).map_err(|error| {
            format!(
                "unable to read contest rules dir {}: {error}",
                path.as_ref().display()
            )
        })?;

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
            for contest in rules_file.contests {
                raw_contests.insert(contest.id.clone(), contest);
            }
        }

        let mut contests = BTreeMap::new();
        let ids = raw_contests.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let contest = resolve_contest(&id, &raw_contests, &mut contests, &mut Vec::new())?;
            contests.insert(id, contest);
        }

        if contests.is_empty() {
            return Err("no contest rules found".to_string());
        }

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
}

fn apply_cabrillo_rules(contest: &mut ContestRules, cabrillo: &RawCabrilloRules) {
    let current = contest.cabrillo.get_or_insert_with(CabrilloRules::default);
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
    }

    if let Some(qso_points) = &mut contest.qso_points {
        for rule in &mut qso_points.rules {
            if let Some(condition) = &mut rule.when {
                resolve_scoring_condition_in_sets(&contest.define, condition)?;
            }
        }
    }

    for multiplier in &mut contest.multipliers {
        if !multiplier.in_sets.is_empty() {
            multiplier.valid_values = defined_values(&contest.define, &multiplier.in_sets)?;
        }
    }

    if let Some(cabrillo) = &mut contest.cabrillo {
        apply_field_valid_values(&mut cabrillo.log_fields, &contest.define)?;
        apply_field_valid_values(&mut cabrillo.export_fields, &contest.define)?;
    }

    Ok(())
}

fn resolve_contest(
    id: &str,
    raw_contests: &BTreeMap<String, RawContestRules>,
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
        resolve_contest(parent_id, raw_contests, resolved, stack)?
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
        contest.allowed_bands = allowed_bands.clone();
    }
    if let Some(allowed_modes) = &raw.allowed_modes {
        contest.allowed_modes = allowed_modes.clone();
    }
    if let Some(define) = &raw.define {
        apply_defines(&mut contest.define, define);
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
        },
    );
    if let Some(scoring) = &raw.scoring {
        apply_scoring_rules(&mut contest, scoring);
    }
    if let Some(metadata) = &raw.metadata {
        contest.metadata = Some(metadata.clone());
    }

    resolve_in_sets(&mut contest)?;
    prepend_standard_qso_columns(&mut contest);

    stack.pop();
    resolved.insert(id.to_string(), contest.clone());
    Ok(contest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve_yaml_contest(yaml: &str, id: &str) -> ContestRules {
        let rules_file: RulesFile = serde_yaml::from_str(yaml).expect("yaml should parse");
        let raw_contests = rules_file
            .contests
            .into_iter()
            .map(|contest| (contest.id.clone(), contest))
            .collect::<BTreeMap<_, _>>();

        resolve_contest(id, &raw_contests, &mut BTreeMap::new(), &mut Vec::new())
            .expect("contest should resolve")
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
}
