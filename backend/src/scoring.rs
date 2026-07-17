use crate::contest_rules::{ContestRules, MultiplierRule, QsoPoints, ScoringCondition};
use crate::db::{Contact, contact_adif_value, contact_id, contact_meta_value, set_contact_meta};
use crate::log_cache::LogCacheProcessor;
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
    power_multiplier: i64,
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
        let power_multiplier = power_multiplier_for(&rules, &contest_params);
        Self {
            rules,
            contest_params,
            power_multiplier,
        }
    }

    pub fn scorer(self: &Arc<Self>) -> ContestScorer {
        ContestScorer {
            module: Arc::clone(self),
            ..ContestScorer::default()
        }
    }

    pub fn has_multipliers(&self) -> bool {
        !self.rules.multipliers.is_empty()
    }

    pub fn power_multiplier(&self) -> i64 {
        self.power_multiplier
    }

    pub fn dupe_key_for(&self, contact: &Contact) -> Option<String> {
        if self.rules.dupe_key.is_empty() {
            return None;
        }
        Some(scoring_key(contact, &self.rules, &self.rules.dupe_key))
    }

    pub fn qso_points_for(&self, contact: &Contact) -> i64 {
        if !contact_in_category_band(&self.rules, &self.contest_params, contact) {
            return 0;
        }
        let Some(qso_points) = &self.rules.qso_points else {
            return 0;
        };

        score_qso_points(qso_points, contact, &self.rules).unwrap_or(0)
    }

    pub fn multiplier_keys_for(&self, contact: &Contact) -> Vec<String> {
        if !contact_in_category_band(&self.rules, &self.contest_params, contact) {
            return Vec::new();
        }
        self.rules
            .multipliers
            .iter()
            .filter(|multiplier| multiplier_matches(multiplier, contact, &self.rules))
            .map(|multiplier| {
                format!(
                    "{}:{}",
                    multiplier.name.to_uppercase(),
                    scoring_key(contact, &self.rules, &multiplier.key)
                )
            })
            .collect()
    }

    pub fn bonus_keys_for(&self, contact: &Contact) -> Vec<(String, i64)> {
        let mut keys = Vec::new();
        for bonus in &self.rules.bonus_points {
            let Some(value) = field_value(contact, &self.rules, &bonus.field) else {
                continue;
            };
            let Some(points) = bonus.values.get(&value) else {
                continue;
            };

            keys.push((
                format!(
                    "{}:{}",
                    bonus.name.to_uppercase(),
                    scoring_key(contact, &self.rules, &bonus.key)
                ),
                *points,
            ));
        }
        keys
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
                power_multiplier: Vec::new(),
                cabrillo: None,
                metadata: None,
            },
            contest_params: Value::Null,
            power_multiplier: 1,
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

        set_contact_meta(contact, "pts", Value::Number(points.into()));
        set_contact_meta(contact, "mult", Value::Number(mults.into()));
        set_contact_meta(contact, "bonus", Value::Number(bonus.into()));
        set_contact_meta(contact, "dupe", Value::Bool(is_dupe));

        self.totals.clone()
    }

    #[allow(dead_code)]
    pub fn remove_scored_qso(&mut self, contact: &Contact) -> ScoreTotals {
        self.totals.qso_count = self.totals.qso_count.saturating_sub(1);
        self.totals.qso_points -= scored_i64(contact, "pts");
        self.totals.multipliers -= scored_i64(contact, "mult");
        self.totals.bonus_points -= scored_i64(contact, "bonus");
        self.remove_dupe_key(contact);
        self.recalculate_score();
        self.totals.clone()
    }

    #[allow(dead_code)]
    pub fn totals(&self) -> ScoreTotals {
        self.totals.clone()
    }

    pub fn dupe_key(&self, contact: &Contact) -> Option<String> {
        self.module.dupe_key_for(contact)
    }

    fn recalculate_score(&mut self) {
        let multiplier_factor = if self.module.has_multipliers() {
            self.totals.multipliers
        } else {
            1
        };
        self.totals.score =
            self.totals.qso_points * multiplier_factor * self.module.power_multiplier()
                + self.totals.bonus_points;
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

    #[allow(dead_code)]
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
        self.module.qso_points_for(contact)
    }

    fn multipliers(&mut self, contact: &Contact) -> i64 {
        self.module
            .multiplier_keys_for(contact)
            .into_iter()
            .filter(|key| self.multiplier_keys.insert(key.clone()))
            .count() as i64
    }

    fn bonus_points(&mut self, contact: &Contact) -> i64 {
        self.module
            .bonus_keys_for(contact)
            .into_iter()
            .filter_map(|(key, points)| self.bonus_keys.insert(key).then_some(points))
            .sum()
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

fn power_multiplier_for(rules: &ContestRules, contest_params: &Value) -> i64 {
    if rules.power_multiplier.is_empty() {
        return 1;
    }

    let Some(category_power_values) = rules
        .cabrillo
        .as_ref()
        .and_then(|cabrillo| {
            cabrillo
                .log_fields
                .iter()
                .find(|field| field.name.eq_ignore_ascii_case("CATEGORY-POWER"))
        })
        .map(|field| &field.valid_values)
    else {
        return 1;
    };

    let Some(selected_power) = contest_params
        .as_object()
        .and_then(|params| params.get("CATEGORY-POWER"))
        .and_then(Value::as_str)
        .map(str::trim)
    else {
        return 1;
    };

    let Some(index) = category_power_values
        .iter()
        .position(|value| value.eq_ignore_ascii_case(selected_power))
    else {
        return 1;
    };

    rules
        .power_multiplier
        .get(index)
        .copied()
        .filter(|value| *value > 0)
        .unwrap_or(1)
}

fn scoring_key(contact: &Contact, rules: &ContestRules, fields: &[String]) -> String {
    fields
        .iter()
        .map(|field| field_value(contact, rules, field).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("|")
}

fn score_qso_points(
    qso_points: &QsoPoints,
    contact: &Contact,
    rules: &ContestRules,
) -> Option<i64> {
    if let Some(geography) = &qso_points.geography {
        let Some(country) = field_value(contact, rules, &geography.country_field) else {
            return Some(geography.unresolved);
        };
        let Some(station_country) = field_value(contact, rules, &geography.station_country_field)
        else {
            return Some(geography.unresolved);
        };
        let Some(continent) = field_value(contact, rules, &geography.continent_field) else {
            return Some(geography.unresolved);
        };
        let Some(station_continent) =
            field_value(contact, rules, &geography.station_continent_field)
        else {
            return Some(geography.unresolved);
        };

        if country == station_country {
            return Some(geography.same_country);
        }
        if continent != station_continent {
            return Some(geography.different_continent);
        }
        if continent == "NA" {
            return Some(geography.different_country_north_america);
        }
        return Some(geography.different_country_same_continent);
    }

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
    let call = json_string(contact_adif_value(contact, "CALL"))
        .unwrap_or_default()
        .trim()
        .to_uppercase();
    if multiplier
        .exclude_call_suffixes
        .iter()
        .any(|suffix| call.ends_with(&suffix.trim().to_uppercase()))
    {
        return false;
    }
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
    json_string(contact_adif_value(contact, field))
        .or_else(|| json_string(contact_meta_value(contact, field)))
        .or_else(|| {
            rules
                .qso_column_fields
                .get(field)
                .and_then(|adif| json_string(contact_adif_value(contact, adif)))
        })
        .map(|value| normalized_field_value(field, &value))
        .filter(|value| !value.is_empty())
}

fn contact_in_category_band(
    rules: &ContestRules,
    contest_params: &Value,
    contact: &Contact,
) -> bool {
    let Some(param_name) = rules
        .qso_points
        .as_ref()
        .and_then(|qso_points| qso_points.category_band_param.as_deref())
    else {
        return true;
    };
    let Some(category_band) = contest_params
        .as_object()
        .and_then(|params| params.get(param_name))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return true;
    };
    if category_band.eq_ignore_ascii_case("ALL") {
        return true;
    }
    let Some(contact_band) = field_value(contact, rules, "BAND") else {
        return false;
    };
    category_band.eq_ignore_ascii_case(&contact_band)
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
    contact_meta_value(contact, field)
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn contact_id_for(contact: &Contact) -> Option<i64> {
    contact_id(contact)
}

#[allow(dead_code)]
fn contact_score_order(contact: &Contact) -> (i64, i64) {
    (
        contact_adif_value(contact, "QSO_DATE_TIME_ON")
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

#[derive(Clone, Default)]
pub struct IncrementalScoreTracker {
    logs: Arc<Mutex<HashMap<i64, IncrementalLogState>>>,
}

#[derive(Clone)]
struct IncrementalLogState {
    module: Arc<ContestScoringModule>,
    totals: ScoreTotals,
    dupe_counts: HashMap<String, usize>,
    dupe_owners: HashMap<String, i64>,
    multiplier_owners: HashMap<String, i64>,
    bonus_owners: HashMap<String, i64>,
}

impl IncrementalScoreTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn totals(&self, log_id: i64) -> Option<ScoreTotals> {
        let logs = self
            .logs
            .lock()
            .expect("incremental score tracker mutex poisoned");
        logs.get(&log_id).map(|state| state.totals.clone())
    }
}

impl LogCacheProcessor for IncrementalScoreTracker {
    fn on_log_loaded(
        &self,
        log_id: i64,
        module: Arc<ContestScoringModule>,
        contacts: &mut [Contact],
    ) {
        let mut logs = self
            .logs
            .lock()
            .expect("incremental score tracker mutex poisoned");
        let state = logs
            .entry(log_id)
            .or_insert_with(|| IncrementalLogState::new(Arc::clone(&module)));
        state.reset(module, contacts);
    }

    fn on_contacts_upserted(
        &self,
        log_id: i64,
        module: Arc<ContestScoringModule>,
        contacts: &mut [Contact],
        committed_contacts: &[Contact],
        previous_contacts: &[Option<Contact>],
    ) -> Vec<Contact> {
        let mut logs = self
            .logs
            .lock()
            .expect("incremental score tracker mutex poisoned");
        let state = logs
            .entry(log_id)
            .or_insert_with(|| IncrementalLogState::new(Arc::clone(&module)));
        if !Arc::ptr_eq(&state.module, &module) {
            state.reset(Arc::clone(&module), contacts);
        }

        let mut changed_contact_ids = HashSet::new();
        for previous_contact in previous_contacts.iter().flatten() {
            state.remove_contact(
                previous_contact,
                contacts,
                &mut changed_contact_ids,
                contact_id_for(previous_contact),
            );
        }

        for committed_contact in committed_contacts {
            let Some(committed_id) = contact_id_for(committed_contact) else {
                continue;
            };
            if let Some(index) = contacts
                .iter()
                .position(|contact| contact_id_for(contact) == Some(committed_id))
            {
                state.add_contact(&mut contacts[index]);
            }
        }

        let committed_ids = committed_contacts
            .iter()
            .filter_map(contact_id_for)
            .collect::<HashSet<_>>();

        collect_changed_contacts(contacts, &changed_contact_ids, &committed_ids)
    }

    fn on_contact_deleted(
        &self,
        log_id: i64,
        module: Arc<ContestScoringModule>,
        contacts: &mut [Contact],
        deleted_contact: &Contact,
    ) -> Vec<Contact> {
        let mut logs = self
            .logs
            .lock()
            .expect("incremental score tracker mutex poisoned");
        let Some(state) = logs.get_mut(&log_id) else {
            return Vec::new();
        };
        if !Arc::ptr_eq(&state.module, &module) {
            state.reset(Arc::clone(&module), contacts);
            return Vec::new();
        }

        let mut changed_contact_ids = HashSet::new();
        state.remove_contact(deleted_contact, contacts, &mut changed_contact_ids, None);

        collect_changed_contacts(contacts, &changed_contact_ids, &HashSet::new())
    }

    fn on_log_removed(&self, log_id: i64) {
        let mut logs = self
            .logs
            .lock()
            .expect("incremental score tracker mutex poisoned");
        logs.remove(&log_id);
    }
}

impl IncrementalLogState {
    fn new(module: Arc<ContestScoringModule>) -> Self {
        Self {
            module,
            totals: ScoreTotals::default(),
            dupe_counts: HashMap::new(),
            dupe_owners: HashMap::new(),
            multiplier_owners: HashMap::new(),
            bonus_owners: HashMap::new(),
        }
    }

    fn reset(&mut self, module: Arc<ContestScoringModule>, contacts: &mut [Contact]) {
        self.module = module;
        self.totals = ScoreTotals::default();
        self.dupe_counts.clear();
        self.dupe_owners.clear();
        self.multiplier_owners.clear();
        self.bonus_owners.clear();

        for contact in contacts {
            self.add_contact(contact);
        }
    }

    fn add_contact(&mut self, contact: &mut Contact) {
        self.totals.qso_count += 1;

        let contact_id = contact_id_for(contact);
        let mut is_dupe = false;
        if let Some(dupe_key) = self.module.dupe_key_for(contact) {
            let count = self.dupe_counts.entry(dupe_key.clone()).or_insert(0);
            is_dupe = *count > 0;
            *count += 1;

            if !is_dupe && let Some(contact_id) = contact_id {
                self.dupe_owners.entry(dupe_key).or_insert(contact_id);
            }
        }

        if is_dupe {
            set_contact_score_fields(contact, 0, 0, 0, true);
            self.recalculate_score();
            return;
        }

        let (points, mults, bonus) = self.score_non_dupe_contact(contact, contact_id);
        self.totals.qso_points += points;
        self.totals.multipliers += mults;
        self.totals.bonus_points += bonus;
        set_contact_score_fields(contact, points, mults, bonus, false);

        self.recalculate_score();
    }

    fn remove_contact(
        &mut self,
        deleted_contact: &Contact,
        contacts: &mut [Contact],
        changed_contact_ids: &mut HashSet<i64>,
        skip_candidate_id: Option<i64>,
    ) {
        self.totals.qso_count = self.totals.qso_count.saturating_sub(1);
        self.totals.qso_points -= scored_i64(deleted_contact, "pts");
        self.totals.multipliers -= scored_i64(deleted_contact, "mult");
        self.totals.bonus_points -= scored_i64(deleted_contact, "bonus");

        let deleted_contact_id = contact_id_for(deleted_contact);
        let deleted_dupe_key = self.module.dupe_key_for(deleted_contact);

        let mut dupe_replacement_index = None;
        if let Some(dupe_key) = deleted_dupe_key.as_deref() {
            if let Some(count) = self.dupe_counts.get_mut(dupe_key) {
                if *count <= 1 {
                    self.dupe_counts.remove(dupe_key);
                } else {
                    *count -= 1;
                }
            }

            if let Some(deleted_contact_id) = deleted_contact_id
                && self.dupe_owners.get(dupe_key) == Some(&deleted_contact_id)
            {
                self.dupe_owners.remove(dupe_key);
                dupe_replacement_index =
                    self.find_dupe_replacement_index(contacts, dupe_key, skip_candidate_id);
                if let Some(index) = dupe_replacement_index
                    && let Some(replacement_contact_id) = contact_id_for(&contacts[index])
                {
                    self.dupe_owners
                        .insert(dupe_key.to_string(), replacement_contact_id);
                }
            }
        }

        let freed_multiplier_keys = deleted_contact_id
            .map(|contact_id| {
                IncrementalLogState::remove_owned_keys(&mut self.multiplier_owners, contact_id)
            })
            .unwrap_or_default();
        let freed_bonus_keys = deleted_contact_id
            .map(|contact_id| {
                IncrementalLogState::remove_owned_keys(&mut self.bonus_owners, contact_id)
            })
            .unwrap_or_default();

        if let Some(index) = dupe_replacement_index {
            self.promote_contact(index, contacts, changed_contact_ids);
        }

        for multiplier_key in freed_multiplier_keys {
            if self.multiplier_owners.contains_key(&multiplier_key) {
                continue;
            }
            let Some(index) = self.find_multiplier_replacement_index(
                contacts,
                &multiplier_key,
                skip_candidate_id,
            ) else {
                continue;
            };
            let Some(contact_id) = contact_id_for(&contacts[index]) else {
                continue;
            };

            self.multiplier_owners.insert(multiplier_key, contact_id);
            increment_contact_score_field(&mut contacts[index], "mult", 1);
            self.totals.multipliers += 1;
            changed_contact_ids.insert(contact_id);
        }

        for bonus_key in freed_bonus_keys {
            if self.bonus_owners.contains_key(&bonus_key) {
                continue;
            }
            let Some((index, points)) =
                self.find_bonus_replacement(contacts, &bonus_key, skip_candidate_id)
            else {
                continue;
            };
            let Some(contact_id) = contact_id_for(&contacts[index]) else {
                continue;
            };

            self.bonus_owners.insert(bonus_key, contact_id);
            increment_contact_score_field(&mut contacts[index], "bonus", points);
            self.totals.bonus_points += points;
            changed_contact_ids.insert(contact_id);
        }

        self.recalculate_score();
    }

    fn promote_contact(
        &mut self,
        index: usize,
        contacts: &mut [Contact],
        changed_contact_ids: &mut HashSet<i64>,
    ) {
        let Some(contact) = contacts.get_mut(index) else {
            return;
        };
        if !is_dupe_contact(contact) {
            return;
        }

        let contact_id = contact_id_for(contact);
        let (points, mults, bonus) = self.score_non_dupe_contact(contact, contact_id);
        self.totals.qso_points += points;
        self.totals.multipliers += mults;
        self.totals.bonus_points += bonus;
        set_contact_score_fields(contact, points, mults, bonus, false);

        if let Some(contact_id) = contact_id {
            changed_contact_ids.insert(contact_id);
        }
    }

    fn score_non_dupe_contact(
        &mut self,
        contact: &Contact,
        contact_id: Option<i64>,
    ) -> (i64, i64, i64) {
        let points = self.module.qso_points_for(contact);
        let mut mults = 0;
        let mut bonus = 0;

        for multiplier_key in self.module.multiplier_keys_for(contact) {
            if let Some(contact_id) = contact_id
                && !self.multiplier_owners.contains_key(&multiplier_key)
            {
                self.multiplier_owners.insert(multiplier_key, contact_id);
                mults += 1;
            }
        }

        for (bonus_key, points) in self.module.bonus_keys_for(contact) {
            if let Some(contact_id) = contact_id
                && !self.bonus_owners.contains_key(&bonus_key)
            {
                self.bonus_owners.insert(bonus_key, contact_id);
                bonus += points;
            }
        }

        (points, mults, bonus)
    }

    fn remove_owned_keys(owners: &mut HashMap<String, i64>, contact_id: i64) -> Vec<String> {
        let keys = owners
            .iter()
            .filter_map(|(key, owner_id)| (*owner_id == contact_id).then_some(key.clone()))
            .collect::<Vec<_>>();
        for key in &keys {
            owners.remove(key);
        }
        keys
    }

    fn find_dupe_replacement_index(
        &self,
        contacts: &[Contact],
        dupe_key: &str,
        skip_candidate_id: Option<i64>,
    ) -> Option<usize> {
        contacts.iter().position(|contact| {
            let Some(contact_id) = contact_id_for(contact) else {
                return false;
            };
            if skip_candidate_id == Some(contact_id) {
                return false;
            }
            self.module.dupe_key_for(contact).as_deref() == Some(dupe_key)
        })
    }

    fn find_multiplier_replacement_index(
        &self,
        contacts: &[Contact],
        multiplier_key: &str,
        skip_candidate_id: Option<i64>,
    ) -> Option<usize> {
        contacts.iter().position(|contact| {
            let Some(contact_id) = contact_id_for(contact) else {
                return false;
            };
            if skip_candidate_id == Some(contact_id) || is_dupe_contact(contact) {
                return false;
            }
            self.module
                .multiplier_keys_for(contact)
                .iter()
                .any(|key| key == multiplier_key)
        })
    }

    fn find_bonus_replacement(
        &self,
        contacts: &[Contact],
        bonus_key: &str,
        skip_candidate_id: Option<i64>,
    ) -> Option<(usize, i64)> {
        contacts.iter().enumerate().find_map(|(index, contact)| {
            let contact_id = contact_id_for(contact)?;
            if skip_candidate_id == Some(contact_id) || is_dupe_contact(contact) {
                return None;
            }

            self.module
                .bonus_keys_for(contact)
                .into_iter()
                .find_map(|(key, points)| (key == bonus_key).then_some((index, points)))
        })
    }

    fn recalculate_score(&mut self) {
        let multiplier_factor = if self.module.has_multipliers() {
            self.totals.multipliers
        } else {
            1
        };
        self.totals.score =
            self.totals.qso_points * multiplier_factor * self.module.power_multiplier()
                + self.totals.bonus_points;
    }
}

fn set_contact_score_fields(
    contact: &mut Contact,
    points: i64,
    mults: i64,
    bonus: i64,
    is_dupe: bool,
) {
    set_contact_meta(contact, "pts", Value::Number(points.into()));
    set_contact_meta(contact, "mult", Value::Number(mults.into()));
    set_contact_meta(contact, "bonus", Value::Number(bonus.into()));
    set_contact_meta(contact, "dupe", Value::Bool(is_dupe));
}

fn increment_contact_score_field(contact: &mut Contact, field: &str, delta: i64) {
    let value = scored_i64(contact, field) + delta;
    set_contact_meta(contact, field, Value::Number(value.into()));
}

fn is_dupe_contact(contact: &Contact) -> bool {
    contact_meta_value(contact, "dupe")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn collect_changed_contacts(
    contacts: &[Contact],
    changed_contact_ids: &HashSet<i64>,
    excluded_contact_ids: &HashSet<i64>,
) -> Vec<Contact> {
    contacts
        .iter()
        .filter_map(|contact| {
            let contact_id = contact_id_for(contact)?;
            if !changed_contact_ids.contains(&contact_id)
                || excluded_contact_ids.contains(&contact_id)
            {
                return None;
            }
            Some(contact.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest_rules::{
        BonusPointRule, CabrilloRules, ContestParam, ContestRules, GeographyQsoPoints,
        QsoPointRule, QsoPoints,
    };
    use serde_json::json;
    use std::collections::BTreeMap;

    fn test_rules(
        qso_points: QsoPoints,
        dupe_key: Vec<&str>,
        multipliers: Vec<MultiplierRule>,
        bonus_points: Vec<BonusPointRule>,
        power_multiplier: Vec<i64>,
        category_power_values: Vec<&str>,
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
            power_multiplier,
            cabrillo: (!category_power_values.is_empty()).then_some(CabrilloRules {
                fixed_fields: Vec::new(),
                log_fields: vec![ContestParam {
                    name: "CATEGORY-POWER".to_string(),
                    label: "Category Power".to_string(),
                    field_type: "String:16".to_string(),
                    required: None,
                    regex: None,
                    default: None,
                    in_sets: Vec::new(),
                    valid_values: category_power_values
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    widget: None,
                    help_text: None,
                    max_lines: None,
                    preserve_case: None,
                }],
                export_fields: Vec::new(),
            }),
            metadata: None,
        }
    }

    fn fixed_points(points: i64) -> QsoPoints {
        QsoPoints {
            points: Some(points),
            rules: Vec::new(),
            geography: None,
            category_band_param: None,
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
            geography: None,
            category_band_param: None,
        }
    }

    fn state_multiplier() -> MultiplierRule {
        MultiplierRule {
            name: "State".to_string(),
            field: "STATE".to_string(),
            key: vec!["STATE".to_string()],
            in_sets: Vec::new(),
            valid_values: Vec::new(),
            exclude_call_suffixes: Vec::new(),
        }
    }

    fn geography_points() -> QsoPoints {
        QsoPoints {
            points: None,
            rules: Vec::new(),
            geography: Some(GeographyQsoPoints {
                country_field: "DXCC_PREFIX".to_string(),
                station_country_field: "MY_DXCC_PREFIX".to_string(),
                continent_field: "CONT".to_string(),
                station_continent_field: "MY_CONT".to_string(),
                same_country: 0,
                different_country_north_america: 2,
                different_country_same_continent: 1,
                different_continent: 3,
                unresolved: 0,
            }),
            category_band_param: None,
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
        let mut meta = Map::new();
        let mut adif = Map::new();
        for (key, value) in fields {
            match key {
                "id" | "logId" | "status" | "sessionId" | "clientId" | "force" | "error"
                | "pts" | "mult" | "bonus" | "dupe" | "DXCC_PREFIX" | "MY_DXCC_PREFIX" => {
                    meta.insert(key.to_string(), value);
                }
                _ => {
                    adif.insert(key.to_string(), value);
                }
            }
        }
        crate::db::build_contact(meta, adif)
    }

    #[test]
    fn scores_without_multipliers_use_qso_points_directly() {
        let rules = test_rules(
            mode_points(),
            vec!["CALL", "BAND", "MODE"],
            Vec::new(),
            Vec::new(),
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
        assert_eq!(contact_meta_value(&contacts[0], "pts"), Some(&json!(1)));
        assert_eq!(contact_meta_value(&contacts[1], "pts"), Some(&json!(2)));
    }

    #[test]
    fn geography_points_use_only_stamped_contact_fields() {
        let rules = test_rules(
            geography_points(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let station = vec![("MY_DXCC_PREFIX", json!("K")), ("MY_CONT", json!("NA"))];
        let mut contacts = vec![
            contact(
                station
                    .clone()
                    .into_iter()
                    .chain([("DXCC_PREFIX", json!("K")), ("CONT", json!("NA"))])
                    .collect(),
            ),
            contact(
                station
                    .clone()
                    .into_iter()
                    .chain([("DXCC_PREFIX", json!("VE")), ("CONT", json!("NA"))])
                    .collect(),
            ),
            contact(
                station
                    .into_iter()
                    .chain([("DXCC_PREFIX", json!("F")), ("CONT", json!("EU"))])
                    .collect(),
            ),
            contact(vec![
                ("MY_DXCC_PREFIX", json!("F")),
                ("MY_CONT", json!("EU")),
                ("DXCC_PREFIX", json!("DL")),
                ("CONT", json!("EU")),
            ]),
            contact(vec![
                ("MY_DXCC_PREFIX", json!("K")),
                ("MY_CONT", json!("NA")),
            ]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 6);
        assert_eq!(contact_meta_value(&contacts[0], "pts"), Some(&json!(0)));
        assert_eq!(contact_meta_value(&contacts[1], "pts"), Some(&json!(2)));
        assert_eq!(contact_meta_value(&contacts[2], "pts"), Some(&json!(3)));
        assert_eq!(contact_meta_value(&contacts[3], "pts"), Some(&json!(1)));
        assert_eq!(contact_meta_value(&contacts[4], "pts"), Some(&json!(0)));
    }

    #[test]
    fn category_band_limits_points_and_multipliers() {
        let mut points = fixed_points(1);
        points.category_band_param = Some("CATEGORY-BAND".to_string());
        let rules = test_rules(
            points,
            Vec::new(),
            vec![state_multiplier()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut contacts = vec![
            contact(vec![("BAND", json!("20m")), ("STATE", json!("SC"))]),
            contact(vec![("BAND", json!("40m")), ("STATE", json!("NC"))]),
        ];

        let totals = score_contacts(&rules, json!({ "CATEGORY-BAND": "20M" }), &mut contacts);

        assert_eq!(totals.qso_count, 2);
        assert_eq!(totals.qso_points, 1);
        assert_eq!(totals.multipliers, 1);
        assert_eq!(totals.score, 1);
    }

    #[test]
    fn multiplier_excludes_configured_callsign_suffixes() {
        let mut country = state_multiplier();
        country.name = "Country".to_string();
        country.field = "DXCC_PREFIX".to_string();
        country.key = vec!["DXCC_PREFIX".to_string(), "BAND".to_string()];
        country.exclude_call_suffixes = vec!["/MM".to_string()];
        let rules = test_rules(
            fixed_points(1),
            Vec::new(),
            vec![country],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut contacts = vec![contact(vec![
            ("CALL", json!("K1ABC/MM")),
            ("BAND", json!("20m")),
            ("DXCC_PREFIX", json!("K")),
        ])];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 1);
        assert_eq!(totals.multipliers, 0);
    }

    #[test]
    fn scores_with_multipliers_multiply_qso_points_by_multiplier_count() {
        let rules = test_rules(
            fixed_points(2),
            Vec::new(),
            vec![state_multiplier()],
            Vec::new(),
            Vec::new(),
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
        assert_eq!(contact_meta_value(&contacts[0], "mult"), Some(&json!(1)));
        assert_eq!(contact_meta_value(&contacts[1], "mult"), Some(&json!(1)));
        assert_eq!(contact_meta_value(&contacts[2], "mult"), Some(&json!(0)));
    }

    #[test]
    fn power_multiplier_scales_score_as_separate_multiplier() {
        let rules = test_rules(
            fixed_points(1),
            Vec::new(),
            vec![state_multiplier()],
            Vec::new(),
            vec![1, 2, 5],
            vec!["HIGH", "LOW", "QRP"],
        );
        let mut contacts = vec![
            contact(vec![("STATE", json!("SC"))]),
            contact(vec![("STATE", json!("NC"))]),
        ];

        let totals = score_contacts(
            &rules,
            json!({
                "CATEGORY-POWER": "LOW"
            }),
            &mut contacts,
        );

        assert_eq!(totals.qso_points, 2);
        assert_eq!(totals.multipliers, 2);
        assert_eq!(totals.score, 8);
    }

    #[test]
    fn power_multiplier_defaults_to_one_when_not_configured() {
        let rules = test_rules(
            fixed_points(2),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut contacts = vec![contact(vec![("CALL", json!("K1ABC"))])];

        let totals = score_contacts(
            &rules,
            json!({
                "CATEGORY-POWER": "QRP"
            }),
            &mut contacts,
        );

        assert_eq!(totals.qso_points, 2);
        assert_eq!(totals.score, 2);
    }

    #[test]
    fn power_multiplier_defaults_to_one_for_invalid_value() {
        let rules = test_rules(
            fixed_points(2),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![1, 2],
            vec!["HIGH", "LOW", "QRP"],
        );
        let mut contacts = vec![contact(vec![("CALL", json!("K1ABC"))])];

        let totals = score_contacts(
            &rules,
            json!({
                "CATEGORY-POWER": "QRP"
            }),
            &mut contacts,
        );

        assert_eq!(totals.qso_points, 2);
        assert_eq!(totals.score, 2);
    }

    #[test]
    fn duplicate_qsos_score_zero() {
        let rules = test_rules(
            fixed_points(2),
            vec!["CALL", "BAND", "MODE"],
            Vec::new(),
            Vec::new(),
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
        assert_eq!(
            contact_meta_value(&contacts[0], "dupe"),
            Some(&json!(false))
        );
        assert_eq!(contact_meta_value(&contacts[1], "dupe"), Some(&json!(true)));
        assert_eq!(contact_meta_value(&contacts[1], "pts"), Some(&json!(0)));
    }

    #[test]
    fn bonus_points_are_awarded_once_per_bonus_key() {
        let rules = test_rules(
            fixed_points(2),
            Vec::new(),
            vec![state_multiplier()],
            vec![bonus_station(350)],
            Vec::new(),
            Vec::new(),
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
        assert_eq!(contact_meta_value(&contacts[0], "bonus"), Some(&json!(350)));
        assert_eq!(contact_meta_value(&contacts[1], "bonus"), Some(&json!(0)));
    }

    #[test]
    fn incremental_tracker_promotes_dupe_when_owner_is_deleted() {
        let rules = test_rules(
            fixed_points(2),
            vec!["CALL", "BAND", "MODE"],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let module = Arc::new(ContestScoringModule::new(rules, Value::Null));
        let tracker = IncrementalScoreTracker::new();
        let mut contacts = vec![
            contact(vec![
                ("id", json!(1)),
                ("CALL", json!("K1ABC")),
                ("BAND", json!("20m")),
                ("MODE", json!("CW")),
            ]),
            contact(vec![
                ("id", json!(2)),
                ("CALL", json!("K1ABC")),
                ("BAND", json!("20m")),
                ("MODE", json!("CW")),
            ]),
        ];

        tracker.on_log_loaded(1, Arc::clone(&module), &mut contacts);
        assert_eq!(
            contact_meta_value(&contacts[0], "dupe"),
            Some(&json!(false))
        );
        assert_eq!(contact_meta_value(&contacts[1], "dupe"), Some(&json!(true)));

        let deleted = contacts.remove(0);
        let changed = tracker.on_contact_deleted(1, module, &mut contacts, &deleted);

        assert_eq!(
            contact_meta_value(&contacts[0], "dupe"),
            Some(&json!(false))
        );
        assert_eq!(contact_meta_value(&contacts[0], "pts"), Some(&json!(2)));
        assert_eq!(changed.len(), 1);
        assert_eq!(contact_id_for(&changed[0]), Some(2));

        let totals = tracker.totals(1).expect("totals should exist");
        assert_eq!(totals.qso_count, 1);
        assert_eq!(totals.qso_points, 2);
        assert_eq!(totals.score, 2);
    }

    #[test]
    fn incremental_tracker_reclaims_multipliers_after_owner_delete() {
        let rules = test_rules(
            fixed_points(1),
            vec!["CALL", "BAND", "MODE"],
            vec![state_multiplier()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let module = Arc::new(ContestScoringModule::new(rules, Value::Null));
        let tracker = IncrementalScoreTracker::new();
        let mut contacts = vec![
            contact(vec![
                ("id", json!(1)),
                ("CALL", json!("K1AAA")),
                ("BAND", json!("20m")),
                ("MODE", json!("CW")),
                ("STATE", json!("SC")),
            ]),
            contact(vec![
                ("id", json!(2)),
                ("CALL", json!("K1BBB")),
                ("BAND", json!("20m")),
                ("MODE", json!("CW")),
                ("STATE", json!("NC")),
            ]),
            contact(vec![
                ("id", json!(3)),
                ("CALL", json!("K1CCC")),
                ("BAND", json!("20m")),
                ("MODE", json!("CW")),
                ("STATE", json!("SC")),
            ]),
        ];

        tracker.on_log_loaded(7, Arc::clone(&module), &mut contacts);
        assert_eq!(
            contact_meta_value(&contact_by_id(&contacts, 1), "mult"),
            Some(&json!(1))
        );
        assert_eq!(
            contact_meta_value(&contact_by_id(&contacts, 2), "mult"),
            Some(&json!(1))
        );
        assert_eq!(
            contact_meta_value(&contact_by_id(&contacts, 3), "mult"),
            Some(&json!(0))
        );

        let deleted = contacts.remove(0);
        let changed = tracker.on_contact_deleted(7, module, &mut contacts, &deleted);

        assert_eq!(
            contact_meta_value(&contact_by_id(&contacts, 3), "mult"),
            Some(&json!(1))
        );
        assert_eq!(changed.len(), 1);
        assert_eq!(contact_id_for(&changed[0]), Some(3));

        let totals = tracker.totals(7).expect("totals should exist");
        assert_eq!(totals.qso_count, 2);
        assert_eq!(totals.qso_points, 2);
        assert_eq!(totals.multipliers, 2);
        assert_eq!(totals.score, 4);
    }

    fn contact_by_id(contacts: &[Contact], id: i64) -> Contact {
        contacts
            .iter()
            .find(|contact| contact_id_for(contact) == Some(id))
            .cloned()
            .expect("contact id should exist")
    }
}
