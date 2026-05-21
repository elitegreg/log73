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

#[derive(Clone, Default)]
pub struct ContestScoreTracker {
    logs: Arc<Mutex<HashMap<i64, TrackedLogScore>>>,
}

#[derive(Clone)]
struct TrackedLogScore {
    contacts: Vec<Contact>,
    scorer: ContestScorer,
    totals: ScoreTotals,
}

impl ContestScoreTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_log(
        &self,
        log_id: i64,
        module: Arc<ContestScoringModule>,
        contacts: &mut [Contact],
    ) -> ScoreTotals {
        let mut scorer = module.scorer();
        scorer.reset();
        for contact in contacts.iter_mut() {
            scorer.add_qso(contact);
        }
        let totals = scorer.totals();

        let mut logs = self.logs.lock().expect("score tracker mutex poisoned");
        logs.insert(
            log_id,
            TrackedLogScore {
                contacts: contacts.to_vec(),
                scorer,
                totals: totals.clone(),
            },
        );

        totals
    }

    pub fn totals(&self, log_id: i64) -> Option<ScoreTotals> {
        let logs = self.logs.lock().expect("score tracker mutex poisoned");
        logs.get(&log_id).map(|score| score.totals.clone())
    }

    pub fn contact(&self, log_id: i64, contact_id: i64) -> Option<Contact> {
        let logs = self.logs.lock().expect("score tracker mutex poisoned");
        logs.get(&log_id).and_then(|score| {
            score
                .contacts
                .iter()
                .find(|contact| contact_id_for(contact) == Some(contact_id))
                .cloned()
        })
    }

    pub fn contacts(&self, log_id: i64) -> Vec<Contact> {
        let logs = self.logs.lock().expect("score tracker mutex poisoned");
        logs.get(&log_id)
            .map(|score| score.contacts.clone())
            .unwrap_or_default()
    }

    pub fn contacts_display_page(&self, log_id: i64, offset: usize, limit: usize) -> Vec<Contact> {
        if limit == 0 {
            return Vec::new();
        }

        let logs = self.logs.lock().expect("score tracker mutex poisoned");
        let Some(score) = logs.get(&log_id) else {
            return Vec::new();
        };

        let total = score.contacts.len();
        if offset >= total {
            return Vec::new();
        }

        let end_from_newest = offset.saturating_add(limit).min(total);
        let start = total - end_from_newest;
        let end = total - offset;

        let mut page = score.contacts[start..end].to_vec();
        page.reverse();
        page
    }

    pub fn can_append(&self, log_id: i64, contact: &Contact) -> bool {
        let logs = self.logs.lock().expect("score tracker mutex poisoned");
        let Some(score) = logs.get(&log_id) else {
            return false;
        };
        score
            .contacts
            .last()
            .map(|last_contact| contact_score_order(last_contact) <= contact_score_order(contact))
            .unwrap_or(true)
    }

    pub fn removing_contact_affects_dupes(&self, log_id: i64, contact: &Contact) -> bool {
        if contact
            .get("_dupe")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return false;
        }

        let logs = self.logs.lock().expect("score tracker mutex poisoned");
        let Some(score) = logs.get(&log_id) else {
            return true;
        };
        let Some(contact_key) = score.scorer.dupe_key(contact) else {
            return false;
        };
        let contact_id = contact_id_for(contact);

        score.contacts.iter().any(|other| {
            contact_id_for(other) != contact_id
                && score.scorer.dupe_key(other).as_deref() == Some(contact_key.as_str())
        })
    }

    pub fn is_last_contact(&self, log_id: i64, contact: &Contact) -> bool {
        let contact_id = contact_id_for(contact);
        let logs = self.logs.lock().expect("score tracker mutex poisoned");
        logs.get(&log_id)
            .and_then(|score| score.contacts.last())
            .map(|last_contact| contact_id_for(last_contact) == contact_id)
            .unwrap_or(false)
    }

    pub fn add_incremental(
        &self,
        log_id: i64,
        mut contact: Contact,
    ) -> Option<(Contact, ScoreTotals)> {
        let mut logs = self.logs.lock().expect("score tracker mutex poisoned");
        let score = logs.get_mut(&log_id)?;

        score.scorer.add_qso(&mut contact);
        score.contacts.push(contact.clone());
        score.totals = score.scorer.totals();

        Some((contact, score.totals.clone()))
    }

    pub fn replace_incremental(
        &self,
        log_id: i64,
        mut contact: Contact,
    ) -> Option<(Contact, ScoreTotals)> {
        let contact_id = contact_id_for(&contact)?;
        let mut logs = self.logs.lock().expect("score tracker mutex poisoned");
        let score = logs.get_mut(&log_id)?;
        let index = score
            .contacts
            .iter()
            .position(|current| contact_id_for(current) == Some(contact_id))?;

        let old_contact = score.contacts[index].clone();
        score.scorer.remove_scored_qso(&old_contact);
        score.scorer.add_qso(&mut contact);
        score.contacts[index] = contact.clone();
        score.totals = score.scorer.totals();

        Some((contact, score.totals.clone()))
    }

    pub fn delete_incremental(&self, log_id: i64, contact_id: i64) -> Option<ScoreTotals> {
        let mut logs = self.logs.lock().expect("score tracker mutex poisoned");
        let score = logs.get_mut(&log_id)?;
        let index = score
            .contacts
            .iter()
            .position(|current| contact_id_for(current) == Some(contact_id))?;

        let old_contact = score.contacts.remove(index);
        score.scorer.remove_scored_qso(&old_contact);
        score.totals = score.scorer.totals();

        Some(score.totals.clone())
    }
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

#[derive(Clone, Default)]
pub struct ContestScorer {
    module: Arc<ContestScoringModule>,
    dupe_keys: HashMap<String, usize>,
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
        self.recalculate_score();

        contact.insert("_pts".to_string(), Value::Number(points.into()));
        contact.insert("_mult".to_string(), Value::Number(mults.into()));
        contact.insert("_bonus".to_string(), Value::Number(bonus.into()));
        contact.insert("_dupe".to_string(), Value::Bool(is_dupe));

        self.totals.clone()
    }

    pub fn remove_scored_qso(&mut self, contact: &Contact) -> ScoreTotals {
        self.totals.qso_count = self.totals.qso_count.saturating_sub(1);
        self.totals.qso_points -= scored_i64(contact, "_pts");
        self.totals.multipliers -= scored_i64(contact, "_mult");
        self.totals.bonus_points -= scored_i64(contact, "_bonus");
        self.remove_dupe_key(contact);
        self.recalculate_score();
        self.totals.clone()
    }

    #[allow(dead_code)]
    pub fn totals(&self) -> ScoreTotals {
        self.totals.clone()
    }

    pub fn dupe_key(&self, contact: &Contact) -> Option<String> {
        let dupe_key = &self.module.rules.dupe_key;
        if dupe_key.is_empty() {
            return None;
        }

        Some(self.key(contact, dupe_key))
    }

    fn recalculate_score(&mut self) {
        self.totals.score = if self.module.rules.multipliers.is_empty() {
            self.totals.qso_points + self.totals.bonus_points
        } else {
            self.totals.qso_points * self.totals.multipliers + self.totals.bonus_points
        };
    }

    fn is_dupe(&mut self, contact: &Contact) -> bool {
        let Some(key) = self.dupe_key(contact) else {
            return false;
        };

        let count = self.dupe_keys.entry(key).or_insert(0);
        let is_dupe = *count > 0;
        *count += 1;
        is_dupe
    }

    fn remove_dupe_key(&mut self, contact: &Contact) {
        let Some(key) = self.dupe_key(contact) else {
            return;
        };
        let Some(count) = self.dupe_keys.get_mut(&key) else {
            return;
        };

        if *count <= 1 {
            self.dupe_keys.remove(&key);
        } else {
            *count -= 1;
        }
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

fn scored_i64(contact: &Contact, field: &str) -> i64 {
    contact.get(field).and_then(Value::as_i64).unwrap_or(0)
}

fn contact_id_for(contact: &Contact) -> Option<i64> {
    contact
        .get("_id")
        .or_else(|| contact.get("ID"))
        .and_then(Value::as_i64)
}

fn contact_score_order(contact: &Contact) -> (i64, i64) {
    (
        contact
            .get("QSO_DATE_TIME_ON")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        contact_id_for(contact).unwrap_or(0),
    )
}

fn json_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(string) => Some(string.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest_rules::{BonusPointRule, ContestRules, QsoPointRule, QsoPoints};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn test_rules(
        qso_points: QsoPoints,
        dupe_key: Vec<&str>,
        multipliers: Vec<MultiplierRule>,
        bonus_points: Vec<BonusPointRule>,
    ) -> ContestRules {
        ContestRules {
            contest: "TEST".to_string(),
            display_name: "Test".to_string(),
            allowed_bands: Vec::new(),
            allowed_modes: Vec::new(),
            define: Vec::new(),
            exchange: Vec::new(),
            qso_columns: Vec::new(),
            qso_column_fields: BTreeMap::new(),
            log_params: Vec::new(),
            qso_points: Some(qso_points),
            dupe_key: dupe_key.into_iter().map(str::to_string).collect(),
            multipliers,
            bonus_points,
            metadata: None,
        }
    }

    fn fixed_points(points: i64) -> QsoPoints {
        QsoPoints {
            points: Some(points),
            rules: Vec::new(),
        }
    }

    fn mode_points() -> QsoPoints {
        QsoPoints {
            points: None,
            rules: vec![
                QsoPointRule {
                    when: Some(ScoringCondition {
                        field: "MODE".to_string(),
                        in_set: None,
                        in_sets: Vec::new(),
                        values: vec!["SSB".to_string()],
                        valid_values: Vec::new(),
                    }),
                    points: 1,
                },
                QsoPointRule {
                    when: None,
                    points: 2,
                },
            ],
        }
    }

    fn state_multiplier() -> MultiplierRule {
        MultiplierRule {
            name: "State".to_string(),
            field: "STATE".to_string(),
            key: vec!["STATE".to_string()],
            in_sets: Vec::new(),
            valid_values: Vec::new(),
        }
    }

    fn bonus_station(points: i64) -> BonusPointRule {
        BonusPointRule {
            name: "Bonus Station".to_string(),
            field: "CALL".to_string(),
            key: vec!["CALL".to_string(), "BAND".to_string()],
            values: BTreeMap::from([("W4CAE".to_string(), points)]),
        }
    }

    fn contact(fields: Vec<(&str, Value)>) -> Contact {
        fields
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect()
    }

    #[test]
    fn scores_without_multipliers_use_qso_points_directly() {
        let rules = test_rules(
            mode_points(),
            vec!["CALL", "BAND", "MODE"],
            Vec::new(),
            Vec::new(),
        );
        let mut contacts = vec![
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("BAND", json!("20m")),
                ("MODE", json!("SSB")),
            ]),
            contact(vec![
                ("CALL", json!("N1XYZ")),
                ("BAND", json!("20m")),
                ("MODE", json!("CW")),
            ]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_count, 2);
        assert_eq!(totals.qso_points, 3);
        assert_eq!(totals.multipliers, 0);
        assert_eq!(totals.score, 3);
        assert_eq!(contacts[0].get("_pts"), Some(&json!(1)));
        assert_eq!(contacts[1].get("_pts"), Some(&json!(2)));
    }

    #[test]
    fn scores_with_multipliers_multiply_qso_points_by_multiplier_count() {
        let rules = test_rules(
            fixed_points(2),
            Vec::new(),
            vec![state_multiplier()],
            Vec::new(),
        );
        let mut contacts = vec![
            contact(vec![("STATE", json!("SC"))]),
            contact(vec![("STATE", json!("NC"))]),
            contact(vec![("STATE", json!("SC"))]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 6);
        assert_eq!(totals.multipliers, 2);
        assert_eq!(totals.score, 12);
        assert_eq!(contacts[0].get("_mult"), Some(&json!(1)));
        assert_eq!(contacts[1].get("_mult"), Some(&json!(1)));
        assert_eq!(contacts[2].get("_mult"), Some(&json!(0)));
    }

    #[test]
    fn duplicate_qsos_score_zero() {
        let rules = test_rules(
            fixed_points(2),
            vec!["CALL", "BAND", "MODE"],
            Vec::new(),
            Vec::new(),
        );
        let mut contacts = vec![
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("BAND", json!("20m")),
                ("MODE", json!("CW")),
            ]),
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("BAND", json!("20m")),
                ("MODE", json!("CW")),
            ]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_count, 2);
        assert_eq!(totals.qso_points, 2);
        assert_eq!(totals.score, 2);
        assert_eq!(contacts[0].get("_dupe"), Some(&json!(false)));
        assert_eq!(contacts[1].get("_dupe"), Some(&json!(true)));
        assert_eq!(contacts[1].get("_pts"), Some(&json!(0)));
    }

    #[test]
    fn bonus_points_are_awarded_once_per_bonus_key() {
        let rules = test_rules(
            fixed_points(2),
            Vec::new(),
            vec![state_multiplier()],
            vec![bonus_station(350)],
        );
        let mut contacts = vec![
            contact(vec![
                ("CALL", json!("W4CAE")),
                ("BAND", json!("20m")),
                ("STATE", json!("SC")),
            ]),
            contact(vec![
                ("CALL", json!("W4CAE")),
                ("BAND", json!("20m")),
                ("STATE", json!("NC")),
            ]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 4);
        assert_eq!(totals.multipliers, 2);
        assert_eq!(totals.bonus_points, 350);
        assert_eq!(totals.score, 358);
        assert_eq!(contacts[0].get("_bonus"), Some(&json!(350)));
        assert_eq!(contacts[1].get("_bonus"), Some(&json!(0)));
    }
}
