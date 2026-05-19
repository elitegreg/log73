use crate::contest_rules::{ContestRules, MultiplierRule, QsoPoints, ScoringCondition};
use crate::db::Contact;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct ScoringModules {
    modules: Arc<Mutex<HashMap<String, Arc<ContestScoringModule>>>>,
}

impl ScoringModules {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, rules: &ContestRules, contest_params: Value) -> Arc<ContestScoringModule> {
        let cache_key = scoring_module_key(&rules.contest, &contest_params);
        let mut modules = self.modules.lock().expect("scoring modules mutex poisoned");
        if let Some(module) = modules.get(&cache_key) {
            return Arc::clone(module);
        }

        let module = Arc::new(ContestScoringModule::new(rules.clone(), contest_params));
        modules.insert(cache_key, Arc::clone(&module));
        module
    }
}

fn scoring_module_key(contest_id: &str, contest_params: &Value) -> String {
    format!(
        "{}:{}",
        contest_id,
        serde_json::to_string(contest_params).unwrap_or_default()
    )
}

pub struct ContestScoringModule {
    rules: ContestRules,
    #[allow(dead_code)]
    contest_params: Value,
}

#[derive(Debug, Clone, Default)]
pub struct ScoreTotals {
    pub qso_count: usize,
    pub qso_points: i64,
    pub multipliers: i64,
    pub bonus_points: i64,
    pub score: i64,
}

#[derive(Default)]
pub struct ContestScorer {
    module: Arc<ContestScoringModule>,
    dupe_keys: HashSet<String>,
    multiplier_keys: HashSet<String>,
    bonus_keys: HashSet<String>,
    totals: ScoreTotals,
}

impl ContestScoringModule {
    fn new(rules: ContestRules, contest_params: Value) -> Self {
        Self {
            rules,
            contest_params,
        }
    }

    pub fn scorer(self: &Arc<Self>) -> ContestScorer {
        ContestScorer {
            module: Arc::clone(self),
            ..ContestScorer::default()
        }
    }
}

impl Default for ContestScoringModule {
    fn default() -> Self {
        Self {
            rules: ContestRules {
                contest: String::new(),
                display_name: String::new(),
                allowed_bands: Vec::new(),
                allowed_modes: Vec::new(),
                define: Vec::new(),
                exchange: Vec::new(),
                qso_columns: Vec::new(),
                qso_column_fields: Default::default(),
                log_params: Vec::new(),
                qso_points: None,
                dupe_key: Vec::new(),
                multipliers: Vec::new(),
                bonus_points: Vec::new(),
                metadata: None,
            },
            contest_params: Value::Null,
        }
    }
}

impl ContestScorer {
    pub fn reset(&mut self) {
        self.dupe_keys.clear();
        self.multiplier_keys.clear();
        self.bonus_keys.clear();
        self.totals = ScoreTotals::default();
    }

    pub fn add_qso(&mut self, contact: &mut Contact) -> ScoreTotals {
        self.totals.qso_count += 1;

        let is_dupe = self.is_dupe(contact);
        let (points, mults, bonus) = if is_dupe {
            (0, 0, 0)
        } else {
            (
                self.qso_points(contact),
                self.multipliers(contact),
                self.bonus_points(contact),
            )
        };

        self.totals.qso_points += points;
        self.totals.multipliers += mults;
        self.totals.bonus_points += bonus;
        self.totals.score =
            self.totals.qso_points * self.totals.multipliers + self.totals.bonus_points;

        contact.insert("_pts".to_string(), Value::Number(points.into()));
        contact.insert("_mult".to_string(), Value::Number(mults.into()));
        contact.insert("_bonus".to_string(), Value::Number(bonus.into()));

        self.totals.clone()
    }

    #[allow(dead_code)]
    pub fn totals(&self) -> ScoreTotals {
        self.totals.clone()
    }

    fn is_dupe(&mut self, contact: &Contact) -> bool {
        let dupe_key = &self.module.rules.dupe_key;
        if dupe_key.is_empty() {
            return false;
        }

        let key = self.key(contact, dupe_key);
        !self.dupe_keys.insert(key)
    }

    fn qso_points(&self, contact: &Contact) -> i64 {
        let Some(qso_points) = &self.module.rules.qso_points else {
            return 0;
        };

        score_qso_points(qso_points, contact, &self.module.rules).unwrap_or(0)
    }

    fn multipliers(&mut self, contact: &Contact) -> i64 {
        let mut new_multipliers = 0;
        let multipliers = self.module.rules.multipliers.clone();
        for multiplier in &multipliers {
            if !multiplier_matches(multiplier, contact, &self.module.rules) {
                continue;
            }
            let key = self.key(contact, &multiplier.key);
            if self
                .multiplier_keys
                .insert(format!("{}:{key}", multiplier.name.to_uppercase()))
            {
                new_multipliers += 1;
            }
        }
        new_multipliers
    }

    fn bonus_points(&mut self, contact: &Contact) -> i64 {
        let mut bonus_points = 0;
        let bonuses = self.module.rules.bonus_points.clone();
        for bonus in &bonuses {
            let Some(value) = field_value(contact, &self.module.rules, &bonus.field) else {
                continue;
            };
            let Some(points) = bonus.values.get(&value) else {
                continue;
            };
            let key = self.key(contact, &bonus.key);
            if self
                .bonus_keys
                .insert(format!("{}:{key}", bonus.name.to_uppercase()))
            {
                bonus_points += points;
            }
        }
        bonus_points
    }

    fn key(&self, contact: &Contact, fields: &[String]) -> String {
        fields
            .iter()
            .map(|field| field_value(contact, &self.module.rules, field).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("|")
    }
}

#[allow(dead_code)]
pub fn score_contacts(
    rules: &ContestRules,
    contest_params: Value,
    contacts: &mut [Contact],
) -> ScoreTotals {
    let module = Arc::new(ContestScoringModule::new(rules.clone(), contest_params));
    let mut scorer = module.scorer();
    scorer.reset();
    for contact in contacts {
        scorer.add_qso(contact);
    }
    scorer.totals()
}

fn score_qso_points(
    qso_points: &QsoPoints,
    contact: &Contact,
    rules: &ContestRules,
) -> Option<i64> {
    if let Some(points) = qso_points.points {
        return Some(points);
    }

    for rule in &qso_points.rules {
        let matches = rule
            .when
            .as_ref()
            .map(|condition| condition_matches(condition, contact, rules))
            .unwrap_or(true);
        if matches {
            return Some(rule.points);
        }
    }

    None
}

fn condition_matches(
    condition: &ScoringCondition,
    contact: &Contact,
    rules: &ContestRules,
) -> bool {
    let Some(value) = field_value(contact, rules, &condition.field) else {
        return false;
    };

    let valid_values = condition
        .valid_values
        .iter()
        .chain(condition.values.iter())
        .map(|value| value.to_uppercase())
        .collect::<HashSet<_>>();

    valid_values.is_empty() || valid_values.contains(&value)
}

fn multiplier_matches(
    multiplier: &MultiplierRule,
    contact: &Contact,
    rules: &ContestRules,
) -> bool {
    let Some(value) = field_value(contact, rules, &multiplier.field) else {
        return false;
    };

    multiplier.valid_values.is_empty()
        || multiplier
            .valid_values
            .iter()
            .any(|valid_value| valid_value.eq_ignore_ascii_case(&value))
}

fn field_value(contact: &Map<String, Value>, rules: &ContestRules, field: &str) -> Option<String> {
    json_string(contact.get(field))
        .or_else(|| {
            rules
                .qso_column_fields
                .get(field)
                .and_then(|adif| json_string(contact.get(adif)))
        })
        .map(|value| normalized_field_value(field, &value))
        .filter(|value| !value.is_empty())
}

fn normalized_field_value(field: &str, value: &str) -> String {
    let normalized = value.trim().to_uppercase();
    if field.eq_ignore_ascii_case("CALL") {
        return normalized_callsign(&normalized);
    }
    normalized
}

fn normalized_callsign(callsign: &str) -> String {
    callsign
        .split_once('/')
        .map(|(base, _)| base.to_string())
        .unwrap_or_else(|| callsign.to_string())
}

fn json_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(string) => Some(string.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}
