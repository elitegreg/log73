use crate::contest_rules::{ContestRules, MultiplierRule, QsoPoints, ScoringCondition};
use crate::db::{Contact, contact_adif_value, contact_id, contact_meta_value, set_contact_meta};
use crate::dxcc::callsign_prefix;
use crate::grid_distance::grid_distance_kilometers;
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
        let cache_key = scoring_module_key(&rules.id, &contest_params);
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
    score_factor: i64,
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
    raw_multiplier_count: i64,
    multiplier_keys: HashSet<String>,
    multiplier_counts: HashMap<String, usize>,
    bonus_keys: HashSet<String>,
    qso_count_bonus_counts: HashMap<String, usize>,
    direct_bonus_points: i64,
    totals: ScoreTotals,
}

#[derive(Clone)]
struct MultiplierCandidate {
    id: String,
    cap_group: Option<String>,
    key: String,
    max_count: Option<usize>,
}

impl ContestScoringModule {
    fn new(rules: ContestRules, contest_params: Value) -> Self {
        let score_factor = score_factor_for(&rules, &contest_params);
        Self {
            rules,
            contest_params,
            score_factor,
        }
    }

    pub fn scorer(self: &Arc<Self>) -> ContestScorer {
        ContestScorer {
            module: Arc::clone(self),
            ..ContestScorer::default()
        }
    }

    pub fn has_multipliers(&self) -> bool {
        !self.rules.scoring.multipliers.is_empty()
    }

    fn minimum_multiplier_count(&self) -> i64 {
        self.rules.scoring.minimum_multiplier_count
    }

    fn has_capped_multipliers(&self) -> bool {
        self.rules
            .scoring
            .multipliers
            .iter()
            .any(|multiplier| multiplier.max_count.is_some())
    }

    pub fn score_factor(&self) -> i64 {
        self.score_factor
    }

    pub fn dupe_key_for(&self, contact: &Contact) -> Option<String> {
        if self.rules.scoring.dupe_key.is_empty() {
            return None;
        }
        Some(dupe_scoring_key(
            contact,
            &self.rules,
            &self.rules.scoring.dupe_key,
        ))
    }

    pub fn qso_points_for(&self, contact: &Contact) -> i64 {
        if !contact_in_category_band(&self.rules, &self.contest_params, contact) {
            return 0;
        }
        let Some(qso_points) = &self.rules.scoring.qso_points else {
            return 0;
        };

        score_qso_points(qso_points, contact, &self.rules).unwrap_or(0)
    }

    pub fn multiplier_keys_for(&self, contact: &Contact) -> Vec<String> {
        self.multiplier_candidates_for(contact)
            .into_iter()
            .map(|candidate| candidate.key)
            .collect()
    }

    fn multiplier_candidates_for(&self, contact: &Contact) -> Vec<MultiplierCandidate> {
        if !contact_in_category_band(&self.rules, &self.contest_params, contact) {
            return Vec::new();
        }
        self.rules
            .scoring
            .multipliers
            .iter()
            .filter(|multiplier| multiplier_matches(multiplier, contact, &self.rules))
            .map(|multiplier| {
                let key = multiplier
                    .fixed_key
                    .clone()
                    .unwrap_or_else(|| scoring_key(contact, &self.rules, &multiplier.key));
                MultiplierCandidate {
                    id: multiplier.id.clone(),
                    cap_group: multiplier.cap_group.clone(),
                    key: format!("{}:{}", multiplier.name.to_uppercase(), key),
                    max_count: multiplier.max_count,
                }
            })
            .collect()
    }

    pub fn bonus_keys_for(&self, contact: &Contact) -> Vec<(String, i64)> {
        let mut keys = Vec::new();
        for bonus in &self.rules.scoring.bonus_points {
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

    fn qso_count_bonus_keys_for(&self, contact: &Contact) -> Vec<String> {
        self.rules
            .scoring
            .qso_count_bonus_points
            .iter()
            .filter(|bonus| {
                let Some(param) = &bonus.param else {
                    return true;
                };
                let selected = self
                    .contest_params
                    .as_object()
                    .and_then(|params| {
                        params
                            .iter()
                            .find(|(name, _)| name.eq_ignore_ascii_case(param))
                    })
                    .and_then(|(_, value)| value.as_str())
                    .map(str::trim);
                let Some(selected) = selected else {
                    return false;
                };
                bonus.values.is_empty()
                    || bonus
                        .values
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(selected))
            })
            .filter_map(|bonus| {
                field_value(contact, &self.rules, &bonus.field)
                    .map(|value| format!("{}:{value}", bonus.id))
            })
            .collect()
    }

    fn multiplier_count_bonus_points<'a, I>(&self, multiplier_keys: I) -> i64
    where
        I: IntoIterator<Item = &'a String>,
    {
        let multiplier_keys = multiplier_keys.into_iter().collect::<Vec<_>>();
        self.rules
            .scoring
            .multiplier_count_bonus_points
            .iter()
            .map(|bonus| {
                let prefix = format!("{}:", bonus.multiplier.trim().to_uppercase());
                let count = multiplier_keys
                    .iter()
                    .filter(|key| key.starts_with(&prefix))
                    .count();
                bonus
                    .thresholds
                    .iter()
                    .rfind(|(threshold, _)| **threshold <= count)
                    .map(|(_, points)| *points)
                    .unwrap_or(0)
            })
            .sum()
    }

    fn qso_count_bonus_points(&self, counts: &HashMap<String, usize>) -> i64 {
        self.rules
            .scoring
            .qso_count_bonus_points
            .iter()
            .map(|bonus| {
                let prefix = format!("{}:", bonus.id);
                counts
                    .iter()
                    .filter(|(key, _)| key.starts_with(&prefix))
                    .map(|(_, count)| {
                        bonus
                            .thresholds
                            .iter()
                            .rfind(|(threshold, _)| **threshold <= *count)
                            .map(|(_, points)| *points)
                            .unwrap_or(0)
                    })
                    .sum::<i64>()
            })
            .sum()
    }
}

impl Default for ContestScoringModule {
    fn default() -> Self {
        Self {
            rules: ContestRules::default(),
            contest_params: Value::Null,
            score_factor: 1,
        }
    }
}

impl ContestScorer {
    pub fn reset(&mut self) {
        self.dupe_keys.clear();
        self.raw_multiplier_count = 0;
        self.multiplier_keys.clear();
        self.multiplier_counts.clear();
        self.bonus_keys.clear();
        self.qso_count_bonus_counts.clear();
        self.direct_bonus_points = 0;
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
        self.raw_multiplier_count += mults;
        self.direct_bonus_points += bonus;
        if !is_dupe && points > 0 {
            for key in self.module.qso_count_bonus_keys_for(contact) {
                *self.qso_count_bonus_counts.entry(key).or_default() += 1;
            }
        }
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
        self.raw_multiplier_count -= scored_i64(contact, "mult");
        self.direct_bonus_points -= scored_i64(contact, "bonus");
        if !is_dupe_contact(contact) && scored_i64(contact, "pts") > 0 {
            for key in self.module.qso_count_bonus_keys_for(contact) {
                decrement_count(&mut self.qso_count_bonus_counts, &key);
            }
        }
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
            self.raw_multiplier_count
                .max(self.module.minimum_multiplier_count())
        } else {
            1
        };
        self.totals.multipliers = if self.module.has_multipliers() {
            multiplier_factor
        } else {
            0
        };
        self.totals.bonus_points = self.direct_bonus_points
            + self
                .module
                .multiplier_count_bonus_points(self.multiplier_keys.iter())
            + self
                .module
                .qso_count_bonus_points(&self.qso_count_bonus_counts);
        self.totals.score = self.totals.qso_points * multiplier_factor * self.module.score_factor()
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
            .multiplier_candidates_for(contact)
            .into_iter()
            .filter(|candidate| {
                if self.multiplier_keys.contains(&candidate.key)
                    || candidate.max_count.is_some_and(|maximum| {
                        self.multiplier_counts
                            .get(candidate.cap_group.as_ref().unwrap_or(&candidate.id))
                            .copied()
                            .unwrap_or(0)
                            >= maximum
                    })
                {
                    return false;
                }
                self.multiplier_keys.insert(candidate.key.clone());
                *self
                    .multiplier_counts
                    .entry(candidate.cap_group.clone().unwrap_or(candidate.id.clone()))
                    .or_default() += 1;
                true
            })
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

fn score_factor_for(rules: &ContestRules, contest_params: &Value) -> i64 {
    rules
        .scoring
        .param_multipliers
        .iter()
        .map(|multiplier| {
            let selected = contest_params
                .as_object()
                .and_then(|params| {
                    params
                        .iter()
                        .find(|(name, _)| name.eq_ignore_ascii_case(&multiplier.param))
                })
                .and_then(|(_, value)| value.as_str())
                .map(str::trim);
            let Some(selected) = selected else {
                return 1;
            };
            multiplier
                .values
                .iter()
                .find(|(value, _)| value.eq_ignore_ascii_case(selected))
                .map(|(_, factor)| *factor)
                .unwrap_or(1)
        })
        .product()
}

fn scoring_key(contact: &Contact, rules: &ContestRules, fields: &[String]) -> String {
    fields
        .iter()
        .map(|field| field_value(contact, rules, field).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("|")
}

fn dupe_scoring_key(contact: &Contact, rules: &ContestRules, fields: &[String]) -> String {
    fields
        .iter()
        .map(|field| {
            if field.eq_ignore_ascii_case("MODE") {
                dupe_mode_class(contact, rules).unwrap_or_default()
            } else {
                field_value(contact, rules, field).unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn dupe_mode_class(contact: &Contact, rules: &ContestRules) -> Option<String> {
    let mode = field_value(contact, rules, "MODE")?;
    Some(match mode.as_str() {
        "CW" | "CW-R" => "CW".to_string(),
        "SSB" => "SSB".to_string(),
        "FM" => "FM".to_string(),
        _ => "DATA".to_string(),
    })
}

fn score_qso_points(
    qso_points: &QsoPoints,
    contact: &Contact,
    rules: &ContestRules,
) -> Option<i64> {
    let base_points = if let Some(grid_distance) = &qso_points.grid_distance {
        let station_grid = field_value(contact, rules, &grid_distance.station_grid_field)?;
        let contact_grid = field_value(contact, rules, &grid_distance.contact_grid_field)?;
        let distance = grid_distance_kilometers(&station_grid, &contact_grid)?;
        let distance_points = (distance / grid_distance.kilometers_per_point as f64).ceil() as i64;
        Some(grid_distance.base_points + distance_points.max(grid_distance.minimum_distance_points))
    } else if let Some(geography) = &qso_points.geography {
        let band = field_value(contact, rules, "BAND");
        let Some(country) = field_value(contact, rules, &geography.country_field) else {
            return Some(
                geography.unresolved.for_band(band.as_deref())
                    + time_bonus_points(qso_points, contact),
            );
        };
        let Some(station_country) = field_value(contact, rules, &geography.station_country_field)
        else {
            return Some(
                geography.unresolved.for_band(band.as_deref())
                    + time_bonus_points(qso_points, contact),
            );
        };
        let Some(continent) = field_value(contact, rules, &geography.continent_field) else {
            return Some(
                geography.unresolved.for_band(band.as_deref())
                    + time_bonus_points(qso_points, contact),
            );
        };
        let Some(station_continent) =
            field_value(contact, rules, &geography.station_continent_field)
        else {
            return Some(
                geography.unresolved.for_band(band.as_deref())
                    + time_bonus_points(qso_points, contact),
            );
        };

        if country == station_country {
            Some(geography.same_country.for_band(band.as_deref()))
        } else if continent != station_continent {
            Some(geography.different_continent.for_band(band.as_deref()))
        } else if continent == "NA" {
            Some(
                geography
                    .different_country_north_america
                    .for_band(band.as_deref()),
            )
        } else {
            Some(
                geography
                    .different_country_same_continent
                    .for_band(band.as_deref()),
            )
        }
    } else if let Some(points) = qso_points.points {
        Some(points)
    } else {
        qso_points.rules.iter().find_map(|rule| {
            let matches = rule
                .when
                .as_ref()
                .map(|condition| condition_matches(condition, contact, rules))
                .unwrap_or(true)
                && rule
                    .when_all
                    .iter()
                    .all(|condition| condition_matches(condition, contact, rules));
            matches.then_some(rule.points)
        })
    }?;

    Some(base_points + time_bonus_points(qso_points, contact))
}

fn time_bonus_points(qso_points: &QsoPoints, contact: &Contact) -> i64 {
    let Some(timestamp) = contact_adif_value(contact, "QSO_DATE_TIME_ON").and_then(Value::as_i64)
    else {
        return 0;
    };
    let minute = (timestamp.rem_euclid(24 * 60 * 60) / 60) as u16;

    qso_points
        .time_bonus_points
        .iter()
        .filter_map(|rule| {
            let start = parse_utc_minute(&rule.start_utc)?;
            let end = parse_utc_minute(&rule.end_utc)?;
            let matches = if start == end {
                true
            } else if start < end {
                minute >= start && minute < end
            } else {
                minute >= start || minute < end
            };
            matches.then_some(rule.points)
        })
        .sum()
}

fn parse_utc_minute(value: &str) -> Option<u16> {
    let (hour, minute) = value.trim().split_once(':')?;
    let hour = hour.parse::<u16>().ok()?;
    let minute = minute.parse::<u16>().ok()?;
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

fn condition_matches(
    condition: &ScoringCondition,
    contact: &Contact,
    rules: &ContestRules,
) -> bool {
    let Some(value) = field_value(contact, rules, &condition.field) else {
        return false;
    };
    if let Some(other_field) = &condition.matches_field {
        let Some(other_value) = field_value(contact, rules, other_field) else {
            return false;
        };
        if !value.eq_ignore_ascii_case(&other_value) {
            return false;
        }
    }
    if let Some(other_field) = &condition.not_matches_field {
        let Some(other_value) = field_value(contact, rules, other_field) else {
            return false;
        };
        if value.eq_ignore_ascii_case(&other_value) {
            return false;
        }
    }
    let suffix_value = json_string(contact_adif_value(contact, &condition.field))
        .or_else(|| json_string(contact_meta_value(contact, &condition.field)))
        .map(|value| value.trim().to_uppercase())
        .unwrap_or_else(|| value.clone());

    let valid_values = condition
        .values
        .iter()
        .map(|value| value.to_uppercase())
        .collect::<HashSet<_>>();

    let excluded_values = condition
        .exclude_values
        .iter()
        .map(|value| value.to_uppercase())
        .collect::<HashSet<_>>();
    let suffixes_match = condition.suffixes.is_empty()
        || condition
            .suffixes
            .iter()
            .any(|suffix| suffix_value.ends_with(&suffix.trim().to_uppercase()));
    let excluded_suffix_matches = condition
        .exclude_suffixes
        .iter()
        .any(|suffix| suffix_value.ends_with(&suffix.trim().to_uppercase()));

    (valid_values.is_empty() || valid_values.contains(&value))
        && !excluded_values.contains(&value)
        && suffixes_match
        && !excluded_suffix_matches
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
    if multiplier
        .when
        .as_ref()
        .is_some_and(|condition| !condition_matches(condition, contact, rules))
    {
        return false;
    }
    if !multiplier
        .when_all
        .iter()
        .all(|condition| condition_matches(condition, contact, rules))
    {
        return false;
    }
    let Some(value) = field_value(contact, rules, &multiplier.field) else {
        return false;
    };
    if multiplier
        .exclude_values
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(&value))
    {
        return false;
    }

    multiplier.values.is_empty()
        || multiplier
            .values
            .iter()
            .any(|valid_value| valid_value.eq_ignore_ascii_case(&value))
}

fn field_value(contact: &Map<String, Value>, _rules: &ContestRules, field: &str) -> Option<String> {
    if field.eq_ignore_ascii_case("MODE_CLASS") {
        let mode = json_string(contact_adif_value(contact, "MODE"))?;
        return Some(match mode.trim().to_uppercase().as_str() {
            "CW" | "CW-R" => "CW".to_string(),
            "SSB" | "FM" | "AM" => "PHONE".to_string(),
            other => other.to_string(),
        });
    }
    if field.eq_ignore_ascii_case("WPX_PREFIX") {
        return contact_prefix(contact);
    }
    if field.eq_ignore_ascii_case("GRID_FIELD") {
        return json_string(contact_adif_value(contact, "GRIDSQUARE"))
            .map(|grid| grid.trim().to_uppercase())
            .filter(|grid| grid.chars().count() >= 2)
            .map(|grid| grid.chars().take(2).collect());
    }
    if field.eq_ignore_ascii_case("JARL_CALL_AREA") {
        return jarl_call_area(contact);
    }
    if field.eq_ignore_ascii_case("JARL_ENTITY") {
        let entity = json_string(contact_adif_value(contact, "DXCC"))
            .map(|value| value.trim().to_uppercase())
            .filter(|value| !value.is_empty());
        return jarl_call_area(contact)
            .is_none()
            .then_some(entity)
            .flatten();
    }
    if field.eq_ignore_ascii_case("DOK_AREA") {
        return json_string(contact_adif_value(contact, "SRX_STRING"))
            .or_else(|| json_string(contact_adif_value(contact, "SRX")))
            .and_then(|exchange| {
                exchange
                    .chars()
                    .find(|character| character.is_ascii_alphabetic())
                    .map(|character| character.to_ascii_uppercase().to_string())
            });
    }
    if field.eq_ignore_ascii_case("SAC_AREA") {
        return sac_area(contact);
    }
    json_string(contact_adif_value(contact, field))
        .or_else(|| json_string(contact_meta_value(contact, field)))
        .map(|value| normalized_field_value(field, &value))
        .filter(|value| !value.is_empty())
}

fn contact_prefix(contact: &Contact) -> Option<String> {
    json_string(contact_adif_value(contact, "PFX"))
        .map(|prefix| prefix.trim().to_uppercase())
        .filter(|prefix| !prefix.is_empty())
        .or_else(|| {
            json_string(contact_adif_value(contact, "CALL"))
                .and_then(|callsign| callsign_prefix(&callsign))
        })
}

fn jarl_call_area(contact: &Contact) -> Option<String> {
    let callsign = json_string(contact_adif_value(contact, "CALL"))?;
    let prefix = callsign_prefix(&callsign)?;
    let area = prefix
        .chars()
        .rev()
        .find(|character| character.is_ascii_digit())
        .unwrap_or('0');
    let dxcc = json_string(contact_adif_value(contact, "DXCC"))
        .and_then(|value| value.trim().parse::<u16>().ok());

    match dxcc {
        Some(339) if !prefix.starts_with("JD1") => Some(format!("JA{area}")),
        Some(291) if is_jarl_us_mainland_prefix(&prefix) => Some(format!("W{area}")),
        Some(1) if is_jarl_canadian_prefix(&prefix) => Some(format!("VE{area}")),
        Some(150) if is_jarl_australian_prefix(&prefix) => Some(format!("VK{area}")),
        Some(_) => None,
        None if is_jarl_japanese_prefix(&prefix) && !prefix.starts_with("JD1") => {
            Some(format!("JA{area}"))
        }
        None if is_jarl_us_mainland_prefix(&prefix) => Some(format!("W{area}")),
        None if is_jarl_canadian_prefix(&prefix) => Some(format!("VE{area}")),
        None if is_jarl_australian_prefix(&prefix) => Some(format!("VK{area}")),
        None => None,
    }
}

fn is_jarl_japanese_prefix(prefix: &str) -> bool {
    matches!(
        prefix.get(..2),
        Some(
            "JA" | "JE"
                | "JF"
                | "JG"
                | "JH"
                | "JI"
                | "JJ"
                | "JK"
                | "JL"
                | "JM"
                | "JN"
                | "JO"
                | "JP"
                | "JQ"
                | "JR"
                | "JS"
                | "7J"
                | "7K"
                | "7L"
                | "7M"
                | "8J"
        )
    )
}

fn is_jarl_us_mainland_prefix(prefix: &str) -> bool {
    matches!(prefix.chars().next(), Some('K' | 'N' | 'W'))
        || (prefix.starts_with('A')
            && prefix
                .chars()
                .nth(1)
                .is_some_and(|character| ('A'..='L').contains(&character)))
}

fn is_jarl_canadian_prefix(prefix: &str) -> bool {
    prefix.starts_with("VA")
        || prefix.starts_with("VE")
        || prefix.starts_with("VO")
        || prefix.starts_with("VY")
        || prefix.starts_with("CY")
}

fn is_jarl_australian_prefix(prefix: &str) -> bool {
    prefix.starts_with("AX") || prefix.starts_with("VI") || prefix.starts_with("VK")
}

fn sac_area(contact: &Contact) -> Option<String> {
    let dxcc = json_string(contact_adif_value(contact, "DXCC"))?
        .parse::<u16>()
        .ok()?;
    let country = match dxcc {
        259 => "JW",
        118 => "JX",
        266 => "LA",
        224 => "OH",
        237 => "OX",
        222 => "OY",
        221 => "OZ",
        284 => "SM",
        242 => "TF",
        _ => return None,
    };
    let area = contact_prefix(contact)
        .and_then(|prefix| prefix.chars().find(|character| character.is_ascii_digit()))
        .unwrap_or('0');
    Some(format!("{country}{area}"))
}

fn contact_in_category_band(
    rules: &ContestRules,
    contest_params: &Value,
    contact: &Contact,
) -> bool {
    let Some(param_name) = rules
        .scoring
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
    raw_multiplier_count: i64,
    direct_bonus_points: i64,
    dupe_counts: HashMap<String, usize>,
    dupe_owners: HashMap<String, i64>,
    multiplier_owners: HashMap<String, i64>,
    multiplier_counts: HashMap<String, usize>,
    bonus_owners: HashMap<String, i64>,
    qso_count_bonus_counts: HashMap<String, usize>,
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

        if state.module.has_capped_multipliers() {
            state.reset(Arc::clone(&module), contacts);
            let committed_ids = committed_contacts
                .iter()
                .filter_map(contact_id_for)
                .collect::<HashSet<_>>();
            return collect_changed_contacts(contacts, &all_contact_ids(contacts), &committed_ids);
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

        if state.module.has_capped_multipliers() {
            state.reset(Arc::clone(&module), contacts);
            return contacts.to_vec();
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
            raw_multiplier_count: 0,
            direct_bonus_points: 0,
            dupe_counts: HashMap::new(),
            dupe_owners: HashMap::new(),
            multiplier_owners: HashMap::new(),
            multiplier_counts: HashMap::new(),
            bonus_owners: HashMap::new(),
            qso_count_bonus_counts: HashMap::new(),
        }
    }

    fn reset(&mut self, module: Arc<ContestScoringModule>, contacts: &mut [Contact]) {
        self.module = module;
        self.totals = ScoreTotals::default();
        self.raw_multiplier_count = 0;
        self.direct_bonus_points = 0;
        self.dupe_counts.clear();
        self.dupe_owners.clear();
        self.multiplier_owners.clear();
        self.multiplier_counts.clear();
        self.bonus_owners.clear();
        self.qso_count_bonus_counts.clear();

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
        self.raw_multiplier_count += mults;
        self.direct_bonus_points += bonus;
        if points > 0 {
            for key in self.module.qso_count_bonus_keys_for(contact) {
                *self.qso_count_bonus_counts.entry(key).or_default() += 1;
            }
        }
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
        self.raw_multiplier_count -= scored_i64(deleted_contact, "mult");
        self.direct_bonus_points -= scored_i64(deleted_contact, "bonus");
        if !is_dupe_contact(deleted_contact) && scored_i64(deleted_contact, "pts") > 0 {
            for key in self.module.qso_count_bonus_keys_for(deleted_contact) {
                decrement_count(&mut self.qso_count_bonus_counts, &key);
            }
        }

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
            self.raw_multiplier_count += 1;
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
            self.direct_bonus_points += points;
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
        self.raw_multiplier_count += mults;
        self.direct_bonus_points += bonus;
        if points > 0 {
            for key in self.module.qso_count_bonus_keys_for(contact) {
                *self.qso_count_bonus_counts.entry(key).or_default() += 1;
            }
        }
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

        for candidate in self.module.multiplier_candidates_for(contact) {
            if let Some(contact_id) = contact_id
                && !self.multiplier_owners.contains_key(&candidate.key)
                && !candidate.max_count.is_some_and(|maximum| {
                    self.multiplier_counts
                        .get(candidate.cap_group.as_ref().unwrap_or(&candidate.id))
                        .copied()
                        .unwrap_or(0)
                        >= maximum
                })
            {
                self.multiplier_owners.insert(candidate.key, contact_id);
                *self
                    .multiplier_counts
                    .entry(candidate.cap_group.unwrap_or(candidate.id))
                    .or_default() += 1;
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
            self.raw_multiplier_count
                .max(self.module.minimum_multiplier_count())
        } else {
            1
        };
        self.totals.multipliers = if self.module.has_multipliers() {
            multiplier_factor
        } else {
            0
        };
        self.totals.bonus_points = self.direct_bonus_points
            + self
                .module
                .multiplier_count_bonus_points(self.multiplier_owners.keys())
            + self
                .module
                .qso_count_bonus_points(&self.qso_count_bonus_counts);
        self.totals.score = self.totals.qso_points * multiplier_factor * self.module.score_factor()
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

fn decrement_count(counts: &mut HashMap<String, usize>, key: &str) {
    let Some(count) = counts.get_mut(key) else {
        return;
    };
    if *count <= 1 {
        counts.remove(key);
    } else {
        *count -= 1;
    }
}

fn all_contact_ids(contacts: &[Contact]) -> HashSet<i64> {
    contacts.iter().filter_map(contact_id_for).collect()
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
        BonusPointRule, ContestRules, ContestRulesStore, GeographyQsoPoints, GridDistanceQsoPoints,
        MultiplierCountBonusRule, ParamMultiplierRule, QsoCountBonusRule, QsoPointRule, QsoPoints,
        ScoringRules, TimeBonusPointRule, test_multiplier_rule, test_scoring_condition,
        test_setup_field,
    };
    use serde_json::json;
    use std::{collections::BTreeMap, path::PathBuf};

    fn test_rules(
        qso_points: QsoPoints,
        dupe_key: Vec<&str>,
        multipliers: Vec<MultiplierRule>,
        bonus_points: Vec<BonusPointRule>,
        score_factors: Vec<i64>,
        category_power_values: Vec<&str>,
    ) -> ContestRules {
        let factor_values = category_power_values
            .iter()
            .zip(score_factors)
            .map(|(value, factor)| ((*value).to_string(), factor))
            .collect::<BTreeMap<_, _>>();
        let mut category_power = test_setup_field(
            "category-power",
            "CATEGORY-POWER",
            "Category Power",
            "String:16",
        );
        category_power.validation.values = category_power_values
            .iter()
            .map(|value| (*value).to_string())
            .collect();
        ContestRules {
            id: "TEST".to_string(),
            name: "Test".to_string(),
            setup_fields: (!category_power_values.is_empty())
                .then_some(category_power)
                .into_iter()
                .collect(),
            scoring: ScoringRules {
                qso_points: Some(qso_points),
                minimum_multiplier_count: 0,
                dupe_key: dupe_key.into_iter().map(str::to_string).collect(),
                multipliers,
                bonus_points,
                param_multipliers: (!factor_values.is_empty())
                    .then_some(ParamMultiplierRule {
                        id: "category-power".to_string(),
                        param: "CATEGORY-POWER".to_string(),
                        values: factor_values,
                    })
                    .into_iter()
                    .collect(),
                multiplier_count_bonus_points: Vec::new(),
                qso_count_bonus_points: Vec::new(),
            },
            ..ContestRules::default()
        }
    }

    fn fixed_points(points: i64) -> QsoPoints {
        QsoPoints {
            points: Some(points),
            rules: Vec::new(),
            time_bonus_points: Vec::new(),
            geography: None,
            grid_distance: None,
            category_band_param: None,
        }
    }

    fn mode_points() -> QsoPoints {
        QsoPoints {
            points: None,
            rules: vec![
                QsoPointRule {
                    id: "ssb".to_string(),
                    when: Some(test_scoring_condition("MODE", &["SSB"])),
                    when_all: Vec::new(),
                    points: 1,
                },
                QsoPointRule {
                    id: "default".to_string(),
                    when: None,
                    when_all: Vec::new(),
                    points: 2,
                },
            ],
            time_bonus_points: Vec::new(),
            geography: None,
            grid_distance: None,
            category_band_param: None,
        }
    }

    fn state_multiplier() -> MultiplierRule {
        let mut multiplier = test_multiplier_rule("state", "State", "STATE");
        multiplier.key = vec!["STATE".to_string()];
        multiplier
    }

    fn geography_points() -> QsoPoints {
        QsoPoints {
            points: None,
            rules: Vec::new(),
            time_bonus_points: Vec::new(),
            geography: Some(GeographyQsoPoints {
                country_field: "APP_LOG73_DXCC_PFX".to_string(),
                station_country_field: "APP_LOG73_MY_DXCC_PFX".to_string(),
                continent_field: "CONT".to_string(),
                station_continent_field: "MY_CONT".to_string(),
                same_country: 0.into(),
                different_country_north_america: 2.into(),
                different_country_same_continent: 1.into(),
                different_continent: 3.into(),
                unresolved: 0.into(),
            }),
            grid_distance: None,
            category_band_param: None,
        }
    }

    fn bonus_station(points: i64) -> BonusPointRule {
        BonusPointRule {
            id: "bonus-station".to_string(),
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
                | "pts" | "mult" | "bonus" | "dupe" => {
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
    fn derived_contest_fields_normalize_grid_dok_and_sac_area_values() {
        let rules = test_rules(
            fixed_points(1),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let contact = contact(vec![
            ("GRIDSQUARE", json!("fn31")),
            ("SRX_STRING", json!("C25")),
            ("DXCC", json!(284)),
            ("PFX", json!("SI3")),
        ]);

        assert_eq!(
            field_value(&contact, &rules, "GRID_FIELD").as_deref(),
            Some("FN")
        );
        assert_eq!(
            field_value(&contact, &rules, "DOK_AREA").as_deref(),
            Some("C")
        );
        assert_eq!(
            field_value(&contact, &rules, "SAC_AREA").as_deref(),
            Some("SM3")
        );
    }

    #[test]
    fn derived_jarl_fields_separate_mainland_call_areas_and_entities() {
        let rules = test_rules(
            fixed_points(1),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let ja = contact(vec![("CALL", json!("JA1ABC")), ("DXCC", json!(339))]);
        let jd1 = contact(vec![("CALL", json!("JD1ABC")), ("DXCC", json!(339))]);
        let us = contact(vec![("CALL", json!("W7ABC")), ("DXCC", json!(291))]);

        assert_eq!(
            field_value(&ja, &rules, "JARL_CALL_AREA").as_deref(),
            Some("JA1")
        );
        assert_eq!(field_value(&ja, &rules, "JARL_ENTITY"), None);
        assert_eq!(field_value(&jd1, &rules, "JARL_CALL_AREA"), None);
        assert_eq!(
            field_value(&jd1, &rules, "JARL_ENTITY").as_deref(),
            Some("339")
        );
        assert_eq!(
            field_value(&us, &rules, "JARL_CALL_AREA").as_deref(),
            Some("W7")
        );
        assert_eq!(field_value(&us, &rules, "JARL_ENTITY"), None);
    }

    #[test]
    fn qso_time_bonuses_handle_overnight_utc_windows() {
        let mut points = fixed_points(1);
        points.time_bonus_points = vec![TimeBonusPointRule {
            id: "overnight".to_string(),
            start_utc: "23:00".to_string(),
            end_utc: "05:00".to_string(),
            points: 2,
        }];
        let rules = test_rules(
            points,
            vec!["CALL"],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut contacts = [
            ("W1AAA", 22 * 60 * 60 + 59 * 60),
            ("W1BBB", 23 * 60 * 60),
            ("W1CCC", 4 * 60 * 60 + 59 * 60),
            ("W1DDD", 5 * 60 * 60),
        ]
        .into_iter()
        .map(|(call, timestamp)| {
            contact(vec![
                ("CALL", json!(call)),
                ("QSO_DATE_TIME_ON", json!(timestamp)),
            ])
        })
        .collect::<Vec<_>>();

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 8);
        assert_eq!(contact_meta_value(&contacts[0], "pts"), Some(&json!(1)));
        assert_eq!(contact_meta_value(&contacts[1], "pts"), Some(&json!(3)));
        assert_eq!(contact_meta_value(&contacts[2], "pts"), Some(&json!(3)));
        assert_eq!(contact_meta_value(&contacts[3], "pts"), Some(&json!(1)));
    }

    #[test]
    fn bundled_new_contests_score_their_special_multiplier_rules() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");

        let mut nine_a = vec![contact(vec![
            ("CALL", json!("9A1AAA")),
            ("BAND", json!("160M")),
            ("MODE", json!("CW")),
            ("DXCC", json!(497)),
            ("CONT", json!("EU")),
            ("SRX_STRING", json!("15")),
            ("QSO_DATE_TIME_ON", json!(23 * 60 * 60)),
        ])];
        let nine_a_totals = score_contacts(
            store.get("9A-DX").expect("9A rules should load"),
            Value::Null,
            &mut nine_a,
        );
        assert_eq!(nine_a_totals.qso_points, 3);
        assert_eq!(nine_a_totals.multipliers, 2);
        assert_eq!(nine_a_totals.score, 6);

        let mut ukraine = vec![contact(vec![
            ("CALL", json!("UT1AAA")),
            ("BAND", json!("20M")),
            ("MODE", json!("SSB")),
            ("DXCC", json!(288)),
            ("SRX_STRING", json!("CH")),
        ])];
        let ukraine_totals = score_contacts(
            store
                .get("UKRAINDX (DX)")
                .expect("Ukrainian DX rules should load"),
            Value::Null,
            &mut ukraine,
        );
        assert_eq!(ukraine_totals.qso_points, 10);
        assert_eq!(ukraine_totals.multipliers, 3);
        assert_eq!(ukraine_totals.score, 30);

        let mut wag = vec![
            contact(vec![
                ("CALL", json!("DL1AAA")),
                ("BAND", json!("20M")),
                ("MODE", json!("CW")),
                ("DXCC", json!(230)),
                ("SRX_STRING", json!("C25")),
            ]),
            contact(vec![
                ("CALL", json!("DL2BBB")),
                ("BAND", json!("20M")),
                ("MODE", json!("SSB")),
                ("DXCC", json!(230)),
                ("SRX_STRING", json!("C29")),
            ]),
        ];
        let wag_totals = score_contacts(
            store.get("WAG (DX)").expect("WAG rules should load"),
            Value::Null,
            &mut wag,
        );
        assert_eq!(wag_totals.qso_points, 6);
        assert_eq!(wag_totals.multipliers, 2);
        assert_eq!(wag_totals.score, 12);

        let mut sac = vec![
            contact(vec![
                ("CALL", json!("SM3AAA")),
                ("PFX", json!("SI3")),
                ("BAND", json!("20M")),
                ("MODE", json!("CW")),
                ("DXCC", json!(284)),
            ]),
            contact(vec![
                ("CALL", json!("SK3BBB")),
                ("PFX", json!("SK3")),
                ("BAND", json!("20M")),
                ("MODE", json!("CW")),
                ("DXCC", json!(284)),
            ]),
            contact(vec![
                ("CALL", json!("OH1CCC")),
                ("PFX", json!("OH1")),
                ("BAND", json!("40M")),
                ("MODE", json!("CW")),
                ("DXCC", json!(224)),
            ]),
        ];
        let sac_totals = score_contacts(
            store.get("SAC-CW (DX)").expect("SAC rules should load"),
            Value::Null,
            &mut sac,
        );
        assert_eq!(sac_totals.qso_points, 5);
        assert_eq!(sac_totals.multipliers, 2);
        assert_eq!(sac_totals.score, 10);

        let mut digi = vec![
            contact(vec![
                ("CALL", json!("W1AAA")),
                ("BAND", json!("20M")),
                ("MODE", json!("FT4")),
                ("MY_GRIDSQUARE", json!("FN31")),
                ("GRIDSQUARE", json!("FN31")),
            ]),
            contact(vec![
                ("CALL", json!("W1AAA")),
                ("BAND", json!("20M")),
                ("MODE", json!("FT8")),
                ("MY_GRIDSQUARE", json!("FN31")),
                ("GRIDSQUARE", json!("FN31")),
            ]),
        ];
        let digi_totals = score_contacts(
            store.get("WW-DIGI").expect("WW Digi rules should load"),
            Value::Null,
            &mut digi,
        );
        assert_eq!(digi_totals.qso_points, 1);
        assert_eq!(digi_totals.multipliers, 1);
        assert_eq!(digi_totals.score, 1);
        assert_eq!(contact_meta_value(&digi[1], "dupe"), Some(&json!(true)));

        let mut iota_world = vec![
            contact(vec![
                ("CALL", json!("W1IOTA")),
                ("BAND", json!("20M")),
                ("MODE", json!("CW")),
                ("SRX_STRING", json!("0")),
            ]),
            contact(vec![
                ("CALL", json!("EA5IOTA")),
                ("BAND", json!("20M")),
                ("MODE", json!("CW")),
                ("SRX_STRING", json!("EU-005")),
            ]),
        ];
        let iota_world_totals = score_contacts(
            store
                .get("RSGB-IOTA (World)")
                .expect("IOTA World rules should load"),
            Value::Null,
            &mut iota_world,
        );
        assert_eq!(iota_world_totals.qso_points, 17);
        assert_eq!(iota_world_totals.multipliers, 1);
        assert_eq!(iota_world_totals.score, 17);

        let mut iota_island = vec![
            contact(vec![
                ("CALL", json!("W1IOTA")),
                ("BAND", json!("20M")),
                ("MODE", json!("SSB")),
                ("MY_IOTA_REF", json!("EU-005")),
                ("SRX_STRING", json!("0")),
            ]),
            contact(vec![
                ("CALL", json!("M6IOTA")),
                ("BAND", json!("20M")),
                ("MODE", json!("SSB")),
                ("MY_IOTA_REF", json!("EU-005")),
                ("SRX_STRING", json!("EU-005")),
            ]),
            contact(vec![
                ("CALL", json!("EA5IOTA")),
                ("BAND", json!("20M")),
                ("MODE", json!("SSB")),
                ("MY_IOTA_REF", json!("EU-005")),
                ("SRX_STRING", json!("EU-123")),
            ]),
        ];
        let iota_island_totals = score_contacts(
            store
                .get("RSGB-IOTA (Island)")
                .expect("IOTA Island rules should load"),
            Value::Null,
            &mut iota_island,
        );
        assert_eq!(iota_island_totals.qso_points, 25);
        assert_eq!(iota_island_totals.multipliers, 2);
        assert_eq!(iota_island_totals.score, 50);

        let mut jarl = vec![
            contact(vec![
                ("CALL", json!("JA1AAA")),
                ("BAND", json!("20M")),
                ("MODE", json!("RTTY")),
                ("DXCC", json!(339)),
                ("CONT", json!("AS")),
                ("MY_CONT", json!("EU")),
            ]),
            contact(vec![
                ("CALL", json!("JD1AAA")),
                ("BAND", json!("20M")),
                ("MODE", json!("RTTY")),
                ("DXCC", json!(339)),
                ("CONT", json!("AS")),
                ("MY_CONT", json!("EU")),
            ]),
            contact(vec![
                ("CALL", json!("W1AAA")),
                ("BAND", json!("20M")),
                ("MODE", json!("RTTY")),
                ("DXCC", json!(291)),
                ("CONT", json!("NA")),
                ("MY_CONT", json!("EU")),
            ]),
            contact(vec![
                ("CALL", json!("DL1AAA")),
                ("BAND", json!("20M")),
                ("MODE", json!("RTTY")),
                ("DXCC", json!(230)),
                ("CONT", json!("EU")),
                ("MY_CONT", json!("EU")),
            ]),
            contact(vec![
                ("CALL", json!("W2AAA")),
                ("BAND", json!("20M")),
                ("MODE", json!("RTTY")),
                ("DXCC", json!(291)),
                ("CONT", json!("NA")),
                ("MY_CONT", json!("EU")),
            ]),
            contact(vec![
                ("CALL", json!("W3AAA/MM")),
                ("BAND", json!("20M")),
                ("MODE", json!("RTTY")),
                ("DXCC", json!(291)),
                ("CONT", json!("NA")),
                ("MY_CONT", json!("EU")),
            ]),
        ];
        let jarl_totals = score_contacts(
            store.get("JARL-RTTY").expect("JARL rules should load"),
            Value::Null,
            &mut jarl,
        );
        assert_eq!(jarl_totals.qso_points, 16);
        assert_eq!(jarl_totals.multipliers, 5);
        assert_eq!(jarl_totals.score, 80);
    }

    fn seed_scoring_field(contact: &mut Contact, field: &str, value: &str) {
        crate::db::set_contact_adif(contact, field, json!(value));
    }

    fn representative_mode(rules: &ContestRules) -> String {
        let preferred = rules
            .scoring
            .qso_points
            .as_ref()
            .and_then(|qso_points| {
                qso_points.rules.iter().find_map(|rule| {
                    let condition = rule.when.as_ref()?;
                    if !condition.field.eq_ignore_ascii_case("MODE_CLASS") {
                        return None;
                    }
                    condition
                        .values
                        .iter()
                        .find_map(|value| match value.to_uppercase().as_str() {
                            "PHONE" => Some("SSB"),
                            "CW" => Some("CW"),
                            "DIGITAL" => Some("RTTY"),
                            _ => None,
                        })
                })
            })
            .unwrap_or_else(|| rules.modes.first().map(String::as_str).unwrap_or("CW"));
        match preferred.to_uppercase().as_str() {
            "PHONE" => "SSB".to_string(),
            "DIGITAL" => "RTTY".to_string(),
            _ => preferred.to_string(),
        }
    }

    fn representative_bundled_contact(rules: &ContestRules) -> Contact {
        let mut result = contact(vec![
            ("CALL", json!("W1ABC")),
            (
                "BAND",
                json!(rules.bands.first().map(String::as_str).unwrap_or("20M")),
            ),
            ("MODE", json!(representative_mode(rules))),
            ("DXCC", json!(230)),
            ("MY_DXCC", json!(291)),
            ("CONT", json!("EU")),
            ("MY_CONT", json!("NA")),
            ("APP_LOG73_DXCC_PFX", json!("DL")),
            ("APP_LOG73_MY_DXCC_PFX", json!("K")),
            ("GRID_SQUARE", json!("FN20")),
            ("MY_GRIDSQUARE", json!("FN20")),
            ("PFX", json!("K1")),
            ("STX", json!("1")),
            ("SRX", json!("1")),
        ]);

        if rules.id.starts_with("AADX-") {
            crate::db::set_contact_adif(&mut result, "DXCC", json!(339));
            crate::db::set_contact_adif(&mut result, "CONT", json!("AS"));
            crate::db::set_contact_adif(&mut result, "APP_LOG73_DXCC_PFX", json!("JA"));
            crate::db::set_contact_adif(&mut result, "PFX", json!("JA1"));
        }
        if rules.id == "PACC (DX)" {
            crate::db::set_contact_adif(&mut result, "DXCC", json!(263));
            crate::db::set_contact_adif(&mut result, "APP_LOG73_DXCC_PFX", json!("PA"));
            crate::db::set_contact_adif(&mut result, "CONT", json!("EU"));
        }
        if rules.id == "UKRAINDX (DX)" {
            crate::db::set_contact_adif(&mut result, "DXCC", json!(288));
            crate::db::set_contact_adif(&mut result, "CONT", json!("EU"));
            crate::db::set_contact_adif(&mut result, "SRX_STRING", json!("CH"));
        }
        if rules.id == "WAG (DX)" {
            crate::db::set_contact_adif(&mut result, "DXCC", json!(230));
            crate::db::set_contact_adif(&mut result, "CONT", json!("EU"));
            crate::db::set_contact_adif(&mut result, "SRX_STRING", json!("C25"));
        }
        if rules.id.ends_with("SAC-CW (DX)") || rules.id.ends_with("SAC-SSB (DX)") {
            crate::db::set_contact_adif(&mut result, "DXCC", json!(284));
            crate::db::set_contact_adif(&mut result, "CONT", json!("EU"));
            crate::db::set_contact_adif(&mut result, "PFX", json!("SM3"));
        }

        for field in &rules.exchange {
            let value = field
                .validation
                .values
                .first()
                .map(String::as_str)
                .unwrap_or_else(|| {
                    if field.adif.to_uppercase().contains("GRID") {
                        "FN20"
                    } else if field.adif.to_uppercase().contains("RST") {
                        "59"
                    } else {
                        "DX"
                    }
                });
            seed_scoring_field(&mut result, &field.adif, value);
        }

        let mut conditions = Vec::new();
        if let Some(qso_points) = &rules.scoring.qso_points {
            conditions.extend(
                qso_points
                    .rules
                    .iter()
                    .filter_map(|rule| rule.when.as_ref()),
            );
            for rule in &qso_points.rules {
                conditions.extend(rule.when_all.iter());
            }
        }
        for multiplier in &rules.scoring.multipliers {
            conditions.extend(multiplier.when.iter());
            conditions.extend(multiplier.when_all.iter());
            if let Some(value) = multiplier.values.first() {
                seed_scoring_field(&mut result, &multiplier.field, value);
            }
        }
        for condition in conditions {
            if condition.field.eq_ignore_ascii_case("MODE_CLASS") {
                continue;
            }
            if let Some(value) = condition.values.first() {
                seed_scoring_field(&mut result, &condition.field, value);
            }
        }
        if rules.id == "UKRAINDX (DX)" {
            crate::db::set_contact_adif(&mut result, "DXCC", json!(288));
            crate::db::set_contact_adif(&mut result, "CONT", json!("EU"));
            crate::db::set_contact_adif(&mut result, "SRX_STRING", json!("CH"));
        }
        if rules.id == "WAG (DX)" {
            crate::db::set_contact_adif(&mut result, "DXCC", json!(230));
            crate::db::set_contact_adif(&mut result, "CONT", json!("EU"));
            crate::db::set_contact_adif(&mut result, "SRX_STRING", json!("C25"));
        }
        if rules.id.ends_with("SAC-CW (DX)") || rules.id.ends_with("SAC-SSB (DX)") {
            crate::db::set_contact_adif(&mut result, "DXCC", json!(284));
            crate::db::set_contact_adif(&mut result, "CONT", json!("EU"));
            crate::db::set_contact_adif(&mut result, "PFX", json!("SM3"));
        }
        result
    }

    #[test]
    fn wpx_prefix_field_prefers_pfx_and_falls_back_to_callsign() {
        let rules = test_rules(
            fixed_points(1),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let stored = contact(vec![("CALL", json!("K1ABC")), ("PFX", json!(" w7 "))]);
        let derived = contact(vec![("CALL", json!("W7DX"))]);
        let empty = contact(vec![("CALL", json!("W7DX")), ("PFX", json!(" "))]);

        assert_eq!(
            field_value(&stored, &rules, "WPX_PREFIX").as_deref(),
            Some("W7")
        );
        assert_eq!(
            field_value(&derived, &rules, "WPX_PREFIX").as_deref(),
            Some("W7")
        );
        assert_eq!(
            field_value(&empty, &rules, "WPX_PREFIX").as_deref(),
            Some("W7")
        );
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
    fn bundled_arrl_10_scores_per_mode_and_classifies_multiplier_types() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store.get("ARRL-10").expect("ARRL-10 rules should load");
        let mut contacts = vec![
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("MODE", json!("CW")),
                ("DXCC", json!(291)),
                ("SRX_STRING", json!("CT")),
            ]),
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("MODE", json!("SSB")),
                ("DXCC", json!(291)),
                ("SRX_STRING", json!("CT")),
            ]),
            contact(vec![
                ("CALL", json!("DL1ABC")),
                ("MODE", json!("CW")),
                ("DXCC", json!(230)),
                ("SRX_STRING", json!(12)),
            ]),
            contact(vec![
                ("CALL", json!("F1ABC/MM")),
                ("MODE", json!("CW")),
                ("DXCC", json!(230)),
                ("SRX_STRING", json!(2)),
            ]),
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("MODE", json!("CW")),
                ("DXCC", json!(291)),
                ("SRX_STRING", json!("CT")),
            ]),
        ];

        let totals = score_contacts(rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 14);
        assert_eq!(totals.multipliers, 4);
        assert_eq!(totals.score, 56);
        assert_eq!(contact_meta_value(&contacts[4], "dupe"), Some(&json!(true)));
    }

    #[test]
    fn grid_distance_points_round_up_each_500_kilometers() {
        let points = QsoPoints {
            points: None,
            rules: Vec::new(),
            time_bonus_points: Vec::new(),
            geography: None,
            grid_distance: Some(GridDistanceQsoPoints {
                station_grid_field: "MY_GRIDSQUARE".to_string(),
                contact_grid_field: "GRIDSQUARE".to_string(),
                base_points: 1,
                kilometers_per_point: 500,
                minimum_distance_points: 1,
            }),
            category_band_param: None,
        };
        let rules = test_rules(
            points,
            vec!["CALL", "BAND"],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut contacts = vec![
            contact(vec![
                ("CALL", json!("W1AW")),
                ("BAND", json!("20m")),
                ("MY_GRIDSQUARE", json!("FN31")),
                ("GRIDSQUARE", json!("FN31")),
            ]),
            contact(vec![
                ("CALL", json!("K9XYZ")),
                ("BAND", json!("20m")),
                ("MY_GRIDSQUARE", json!("FN31")),
                ("GRIDSQUARE", json!("EN50")),
            ]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(contact_meta_value(&contacts[0], "pts"), Some(&json!(2)));
        assert_eq!(contact_meta_value(&contacts[1], "pts"), Some(&json!(4)));
        assert_eq!(totals.qso_points, 6);
    }

    #[test]
    fn conditional_point_rules_distinguish_arrl_160_domestic_and_dx_contacts() {
        let domestic = || test_scoring_condition("DXCC", &["1", "291"]);
        let station_domestic = || test_scoring_condition("MY_DXCC", &["1", "291"]);
        let points = QsoPoints {
            points: None,
            rules: vec![
                QsoPointRule {
                    id: "both-domestic".to_string(),
                    when: None,
                    when_all: vec![domestic(), station_domestic()],
                    points: 2,
                },
                QsoPointRule {
                    id: "worked-domestic".to_string(),
                    when: Some(domestic()),
                    when_all: Vec::new(),
                    points: 5,
                },
                QsoPointRule {
                    id: "station-domestic".to_string(),
                    when: Some(station_domestic()),
                    when_all: Vec::new(),
                    points: 5,
                },
                QsoPointRule {
                    id: "default".to_string(),
                    when: None,
                    when_all: Vec::new(),
                    points: 0,
                },
            ],
            time_bonus_points: Vec::new(),
            geography: None,
            grid_distance: None,
            category_band_param: None,
        };
        let rules = test_rules(
            points,
            vec!["CALL"],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut contacts = vec![
            contact(vec![
                ("CALL", json!("K1AAA")),
                ("DXCC", json!(291)),
                ("MY_DXCC", json!(1)),
            ]),
            contact(vec![
                ("CALL", json!("F1AAA")),
                ("DXCC", json!(227)),
                ("MY_DXCC", json!(291)),
            ]),
            contact(vec![
                ("CALL", json!("VE1AAA")),
                ("DXCC", json!(1)),
                ("MY_DXCC", json!(227)),
            ]),
            contact(vec![
                ("CALL", json!("DL1AAA")),
                ("DXCC", json!(230)),
                ("MY_DXCC", json!(227)),
            ]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 12);
        assert_eq!(contact_meta_value(&contacts[0], "pts"), Some(&json!(2)));
        assert_eq!(contact_meta_value(&contacts[1], "pts"), Some(&json!(5)));
        assert_eq!(contact_meta_value(&contacts[2], "pts"), Some(&json!(5)));
        assert_eq!(contact_meta_value(&contacts[3], "pts"), Some(&json!(0)));
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
        let station = vec![
            ("APP_LOG73_MY_DXCC_PFX", json!("K")),
            ("MY_CONT", json!("NA")),
        ];
        let mut contacts = vec![
            contact(
                station
                    .clone()
                    .into_iter()
                    .chain([("APP_LOG73_DXCC_PFX", json!("K")), ("CONT", json!("NA"))])
                    .collect(),
            ),
            contact(
                station
                    .clone()
                    .into_iter()
                    .chain([("APP_LOG73_DXCC_PFX", json!("VE")), ("CONT", json!("NA"))])
                    .collect(),
            ),
            contact(
                station
                    .into_iter()
                    .chain([("APP_LOG73_DXCC_PFX", json!("F")), ("CONT", json!("EU"))])
                    .collect(),
            ),
            contact(vec![
                ("APP_LOG73_MY_DXCC_PFX", json!("F")),
                ("MY_CONT", json!("EU")),
                ("APP_LOG73_DXCC_PFX", json!("DL")),
                ("CONT", json!("EU")),
            ]),
            contact(vec![
                ("APP_LOG73_MY_DXCC_PFX", json!("K")),
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
    fn bundled_cqwpx_scores_band_weighted_geography_and_unique_prefixes() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store.get("CQ-WPX-CW").expect("CQ-WPX-CW rules should load");
        let station = [
            ("APP_LOG73_MY_DXCC_PFX", json!("K")),
            ("MY_CONT", json!("NA")),
        ];
        let mut contacts = [
            ("K1ABC", "20m", "K", "NA"),
            ("VE3XYZ", "20m", "VE", "NA"),
            ("DL1ABC", "20m", "DL", "EU"),
            ("DL1ZZZ", "40m", "DL", "EU"),
            ("F2ABC", "40m", "F", "EU"),
            ("K1ABC", "40m", "K", "NA"),
        ]
        .into_iter()
        .map(|(call, band, country, continent)| {
            contact(
                station
                    .clone()
                    .into_iter()
                    .chain([
                        ("CALL", json!(call)),
                        ("BAND", json!(band)),
                        ("MODE", json!("CW")),
                        ("APP_LOG73_DXCC_PFX", json!(country)),
                        ("CONT", json!(continent)),
                    ])
                    .collect(),
            )
        })
        .collect::<Vec<_>>();

        let totals = score_contacts(rules, json!({ "CATEGORY-BAND": "ALL" }), &mut contacts);

        assert_eq!(totals.qso_count, 6);
        assert_eq!(totals.qso_points, 19);
        assert_eq!(totals.multipliers, 4);
        assert_eq!(totals.score, 76);
        assert_eq!(contact_meta_value(&contacts[0], "pts"), Some(&json!(1)));
        assert_eq!(contact_meta_value(&contacts[1], "pts"), Some(&json!(2)));
        assert_eq!(contact_meta_value(&contacts[2], "pts"), Some(&json!(3)));
        assert_eq!(contact_meta_value(&contacts[3], "pts"), Some(&json!(6)));
        assert_eq!(contact_meta_value(&contacts[5], "pts"), Some(&json!(1)));
        assert_eq!(contact_meta_value(&contacts[3], "mult"), Some(&json!(0)));
        assert_eq!(
            contact_meta_value(&contacts[5], "dupe"),
            Some(&json!(false))
        );

        let european_station = [
            ("APP_LOG73_MY_DXCC_PFX", json!("F")),
            ("MY_CONT", json!("EU")),
        ];
        let mut same_continent_contacts = [
            ("F1ABC", "40m", "F"),
            ("DL1ABC", "20m", "DL"),
            ("DL1XYZ", "40m", "DL"),
        ]
        .into_iter()
        .map(|(call, band, country)| {
            contact(
                european_station
                    .clone()
                    .into_iter()
                    .chain([
                        ("CALL", json!(call)),
                        ("BAND", json!(band)),
                        ("MODE", json!("CW")),
                        ("APP_LOG73_DXCC_PFX", json!(country)),
                        ("CONT", json!("EU")),
                    ])
                    .collect(),
            )
        })
        .collect::<Vec<_>>();

        let same_continent_totals = score_contacts(
            rules,
            json!({ "CATEGORY-BAND": "ALL" }),
            &mut same_continent_contacts,
        );
        assert_eq!(same_continent_totals.qso_points, 4);
        assert_eq!(same_continent_totals.multipliers, 2);
        assert_eq!(same_continent_totals.score, 8);
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
        country.field = "APP_LOG73_DXCC_PFX".to_string();
        country.key = vec!["APP_LOG73_DXCC_PFX".to_string(), "BAND".to_string()];
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
            ("APP_LOG73_DXCC_PFX", json!("K")),
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
    fn fixed_multiplier_key_collapses_matching_values() {
        let mut county = state_multiplier();
        county.name = "Kansas".to_string();
        county.field = "COUNTY".to_string();
        county.key = vec!["COUNTY".to_string()];
        county.fixed_key = Some("KS".to_string());
        let rules = test_rules(
            fixed_points(1),
            Vec::new(),
            vec![county],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut contacts = vec![
            contact(vec![("COUNTY", json!("ALL"))]),
            contact(vec![("COUNTY", json!("WYA"))]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.multipliers, 1);
        assert_eq!(totals.score, 2);
    }

    #[test]
    fn parameter_multiplier_scales_score_as_separate_multiplier() {
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
    fn parameter_multiplier_defaults_to_one_when_not_configured() {
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
    fn parameter_multiplier_defaults_to_one_for_invalid_value() {
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
    fn parameter_multipliers_are_combined() {
        let mut rules = test_rules(
            fixed_points(1),
            Vec::new(),
            vec![state_multiplier()],
            Vec::new(),
            vec![1, 2, 3],
            vec!["HIGH", "LOW", "QRP"],
        );
        rules.scoring.param_multipliers.push(ParamMultiplierRule {
            id: "category-station".to_string(),
            param: "CATEGORY-STATION".to_string(),
            values: BTreeMap::from([
                ("FIXED".to_string(), 1),
                ("MOBILE".to_string(), 2),
                ("ROVER".to_string(), 4),
            ]),
        });
        let mut contacts = vec![
            contact(vec![("STATE", json!("SC"))]),
            contact(vec![("STATE", json!("NC"))]),
        ];

        let totals = score_contacts(
            &rules,
            json!({
                "CATEGORY-POWER": "LOW",
                "CATEGORY-STATION": "ROVER"
            }),
            &mut contacts,
        );

        assert_eq!(totals.qso_points, 2);
        assert_eq!(totals.multipliers, 2);
        assert_eq!(totals.score, 32);
    }

    #[test]
    fn mode_class_groups_phone_modes_for_dupes() {
        let rules = test_rules(
            fixed_points(1),
            vec!["CALL", "BAND", "MODE_CLASS"],
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
                ("CALL", json!("K1ABC")),
                ("BAND", json!("20m")),
                ("MODE", json!("FM")),
            ]),
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("BAND", json!("20m")),
                ("MODE", json!("CW-R")),
            ]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 2);
        assert_eq!(
            contact_meta_value(&contacts[0], "dupe"),
            Some(&json!(false))
        );
        assert_eq!(contact_meta_value(&contacts[1], "dupe"), Some(&json!(true)));
        assert_eq!(
            contact_meta_value(&contacts[2], "dupe"),
            Some(&json!(false))
        );
    }

    #[test]
    fn mode_dupe_key_groups_digital_modes_and_keeps_ssb_cw_and_fm_distinct() {
        let rules = test_rules(
            fixed_points(1),
            vec!["CALL", "BAND", "MODE"],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut contacts = vec![
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("BAND", json!("10m")),
                ("MODE", json!("RTTY")),
            ]),
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("BAND", json!("10m")),
                ("MODE", json!("FT8")),
            ]),
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("BAND", json!("10m")),
                ("MODE", json!("PSK")),
            ]),
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("BAND", json!("10m")),
                ("MODE", json!("SSB")),
            ]),
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("BAND", json!("10m")),
                ("MODE", json!("CW")),
            ]),
            contact(vec![
                ("CALL", json!("K1ABC")),
                ("BAND", json!("10m")),
                ("MODE", json!("FM")),
            ]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 4);
        assert_eq!(
            contact_meta_value(&contacts[0], "dupe"),
            Some(&json!(false))
        );
        assert_eq!(contact_meta_value(&contacts[1], "dupe"), Some(&json!(true)));
        assert_eq!(contact_meta_value(&contacts[2], "dupe"), Some(&json!(true)));
        assert_eq!(
            contact_meta_value(&contacts[3], "dupe"),
            Some(&json!(false))
        );
        assert_eq!(
            contact_meta_value(&contacts[4], "dupe"),
            Some(&json!(false))
        );
        assert_eq!(
            contact_meta_value(&contacts[5], "dupe"),
            Some(&json!(false))
        );
    }

    #[test]
    fn multiplier_excludes_configured_values() {
        let mut country = state_multiplier();
        country.name = "Country".to_string();
        country.field = "DXCC".to_string();
        country.key = vec!["DXCC".to_string()];
        country.exclude_values = vec!["1".to_string(), "291".to_string()];
        let rules = test_rules(
            fixed_points(1),
            Vec::new(),
            vec![country],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut contacts = vec![
            contact(vec![("DXCC", json!(291))]),
            contact(vec![("DXCC", json!(1))]),
            contact(vec![("DXCC", json!(230))]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.multipliers, 1);
        assert_eq!(totals.score, 3);
    }

    #[test]
    fn multiplier_max_count_limits_distinct_values() {
        let mut state = state_multiplier();
        state.max_count = Some(2);
        let rules = test_rules(
            fixed_points(1),
            Vec::new(),
            vec![state],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut contacts = vec![
            contact(vec![("STATE", json!("AL"))]),
            contact(vec![("STATE", json!("AK"))]),
            contact(vec![("STATE", json!("AZ"))]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 3);
        assert_eq!(totals.multipliers, 2);
        assert_eq!(totals.score, 6);
    }

    #[test]
    fn qso_count_bonus_applies_per_sent_location_and_category() {
        let mut rules = test_rules(
            fixed_points(1),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        rules
            .scoring
            .qso_count_bonus_points
            .push(QsoCountBonusRule {
                id: "mobile-county".to_string(),
                name: "Mobile County".to_string(),
                field: "STX_STRING".to_string(),
                param: Some("CATEGORY-STATION".to_string()),
                values: vec!["MOBILE".to_string(), "ROVER".to_string()],
                thresholds: BTreeMap::from([(2, 500)]),
            });
        let mut contacts = vec![
            contact(vec![("STX_STRING", json!("AAA"))]),
            contact(vec![("STX_STRING", json!("AAA"))]),
            contact(vec![("STX_STRING", json!("BBB"))]),
        ];

        let mobile = score_contacts(&rules, json!({"CATEGORY-STATION": "MOBILE"}), &mut contacts);
        assert_eq!(mobile.qso_points, 3);
        assert_eq!(mobile.bonus_points, 500);
        assert_eq!(mobile.score, 503);

        let fixed = score_contacts(&rules, json!({"CATEGORY-STATION": "FIXED"}), &mut contacts);
        assert_eq!(fixed.bonus_points, 0);
        assert_eq!(fixed.score, 3);
    }

    #[test]
    fn bundled_cqp_paqp_and_ilqp_use_declarative_scoring_extensions() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");

        let cqp = store
            .get("CA-QSO-PARTY (In State)")
            .expect("California rules should load");
        let mut cqp_locations = cqp.value_sets["States"]
            .iter()
            .chain(cqp.value_sets["Canadian Provinces"].iter())
            .cloned()
            .collect::<Vec<_>>();
        cqp_locations.push("ALAM".to_string());
        let mut cqp_contacts = cqp_locations
            .iter()
            .enumerate()
            .map(|(index, location)| {
                contact(vec![
                    ("CALL", json!(format!("K{index}CA"))),
                    ("BAND", json!("20M")),
                    ("MODE", json!("CW")),
                    ("STX_STRING", json!("ALAM")),
                    ("SRX_STRING", json!(location)),
                ])
            })
            .collect::<Vec<_>>();
        let cqp_totals = score_contacts(cqp, Value::Null, &mut cqp_contacts);
        assert_eq!(cqp_totals.qso_points, 189);
        assert_eq!(cqp_totals.multipliers, 58);
        assert_eq!(cqp_totals.score, 10_962);

        let paqp = store
            .get("PA-QSO-PARTY (In State)")
            .expect("Pennsylvania rules should load");
        let mut paqp_contacts = (0..10)
            .map(|index| {
                contact(vec![
                    ("CALL", json!(format!("K{index}PA"))),
                    ("BAND", json!("20M")),
                    ("MODE", json!("CW")),
                    ("STX_STRING", json!("ADA")),
                    ("SRX_STRING", json!("CO")),
                ])
            })
            .collect::<Vec<_>>();
        let paqp_totals = score_contacts(
            paqp,
            json!({"CATEGORY-STATION": "MOBILE", "CATEGORY-POWER": "LOW"}),
            &mut paqp_contacts,
        );
        assert_eq!(paqp_totals.qso_points, 20);
        assert_eq!(paqp_totals.bonus_points, 500);

        let ilqp = store
            .get("IL-QSO-PARTY (In State)")
            .expect("Illinois rules should load");
        let mut ilqp_contacts = (230..236)
            .map(|dxcc| {
                contact(vec![
                    ("CALL", json!(format!("DL{dxcc}ABC"))),
                    ("BAND", json!("20M")),
                    ("MODE", json!("CW")),
                    ("STX_STRING", json!("ADAM")),
                    ("SRX_STRING", json!("DX")),
                    ("DXCC", json!(dxcc)),
                ])
            })
            .collect::<Vec<_>>();
        let ilqp_totals = score_contacts(ilqp, Value::Null, &mut ilqp_contacts);
        assert_eq!(ilqp_totals.qso_points, 12);
        assert_eq!(ilqp_totals.multipliers, 5);
        assert_eq!(ilqp_totals.score, 60);
    }

    #[test]
    fn multiplier_count_bonus_uses_only_highest_reached_threshold() {
        let mut rules = test_rules(
            fixed_points(1),
            Vec::new(),
            vec![state_multiplier()],
            vec![bonus_station(50)],
            Vec::new(),
            Vec::new(),
        );
        rules
            .scoring
            .multiplier_count_bonus_points
            .push(MultiplierCountBonusRule {
                id: "state-sweep".to_string(),
                name: "State Sweep".to_string(),
                multiplier: "State".to_string(),
                thresholds: BTreeMap::from([(2, 250), (3, 500)]),
            });
        let mut contacts = vec![
            contact(vec![("CALL", json!("W4CAE")), ("STATE", json!("SC"))]),
            contact(vec![("CALL", json!("K1ABC")), ("STATE", json!("NC"))]),
            contact(vec![("CALL", json!("K1XYZ")), ("STATE", json!("GA"))]),
        ];

        let totals = score_contacts(&rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 3);
        assert_eq!(totals.multipliers, 3);
        assert_eq!(totals.bonus_points, 550);
        assert_eq!(totals.score, 559);
        assert_eq!(contact_meta_value(&contacts[0], "bonus"), Some(&json!(50)));
        assert_eq!(contact_meta_value(&contacts[1], "bonus"), Some(&json!(0)));
        assert_eq!(contact_meta_value(&contacts[2], "bonus"), Some(&json!(0)));
    }

    #[test]
    fn bundled_outside_mdc_rules_score_category_factors_and_jurisdiction_bonus() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store
            .get("MDC-QSO-PARTY")
            .expect("outside-MDC rules should load");
        let jurisdictions = [
            "ALY", "ANA", "BAL", "BCT", "CLV", "CLN", "CRL", "CEC", "CHS", "DRC", "FRD", "GAR",
            "HFD",
        ];
        let mut contacts = jurisdictions
            .iter()
            .enumerate()
            .map(|(index, jurisdiction)| {
                contact(vec![
                    ("CALL", json!(if index == 0 { "W3VPR" } else { "K1ABC" })),
                    ("BAND", json!("20m")),
                    ("MODE", json!("CW")),
                    ("STX_STRING", json!("VA")),
                    ("SRX_STRING", json!(jurisdiction)),
                ])
            })
            .collect::<Vec<_>>();

        let totals = score_contacts(
            rules,
            json!({
                "CATEGORY-POWER": "QRP",
                "CATEGORY-STATION": "ROVER"
            }),
            &mut contacts,
        );

        assert_eq!(totals.qso_points, 39);
        assert_eq!(totals.multipliers, 13);
        assert_eq!(totals.bonus_points, 300);
        assert_eq!(totals.score, 6_384);
    }

    #[test]
    fn bundled_outside_hi_rules_score_modes_districts_per_band_and_dupes() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store
            .get("HI-QSO-PARTY")
            .expect("outside-Hawaii rules should load");
        let mut contacts = [
            ("KH6AAA", "20m", "CW", "HIL"),
            ("KH6AAA", "20m", "CW", "HIL"),
            ("KH6AAA", "20m", "CW", "KON"),
            ("KH6BBB", "40m", "SSB", "HIL"),
            ("KH6CCC", "20m", "SSB", "HIL"),
        ]
        .into_iter()
        .map(|(call, band, mode, district)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!(mode)),
                ("STX_STRING", json!("CA")),
                ("SRX_STRING", json!(district)),
            ])
        })
        .collect::<Vec<_>>();

        let totals = score_contacts(rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 10);
        assert_eq!(totals.multipliers, 3);
        assert_eq!(totals.score, 30);
        assert_eq!(contact_meta_value(&contacts[1], "dupe"), Some(&json!(true)));
        assert_eq!(
            contact_meta_value(&contacts[2], "dupe"),
            Some(&json!(false))
        );
    }

    #[test]
    fn bundled_in_state_hi_rules_score_location_and_dxcc_multipliers_once() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store
            .get("HI-QSO-PARTY (In State)")
            .expect("in-state Hawaii rules should load");
        let mut contacts = [
            ("KH6AAA", "20m", "CW", "HIL", 110),
            ("KH6BBB", "40m", "SSB", "HIL", 110),
            ("K1ABC", "20m", "CW", "CA", 291),
            ("W3ABC", "20m", "CW", "DC", 291),
            ("VE3ABC", "20m", "CW", "ON", 1),
            ("DL1ABC", "20m", "CW", "DX", 230),
            ("F1ABC", "20m", "CW", "DX", 227),
            ("KL7ABC", "20m", "CW", "AK", 6),
            ("KH6CCC", "20m", "CW", "KON", 110),
        ]
        .into_iter()
        .map(|(call, band, mode, exchange, dxcc)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!(mode)),
                ("STX_STRING", json!("MAU")),
                ("SRX_STRING", json!(exchange)),
                ("DXCC", json!(dxcc)),
            ])
        })
        .collect::<Vec<_>>();

        let totals = score_contacts(rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_points, 26);
        assert_eq!(totals.multipliers, 8);
        assert_eq!(totals.score, 208);
    }

    #[test]
    fn bundled_in_state_mdc_rules_combine_jurisdiction_state_province_and_dxcc_multipliers() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store
            .get("MDC-QSO-PARTY (In State)")
            .expect("in-state MDC rules should load");
        let mut contacts = [
            ("K1ALY", "ALY", 291),
            ("K1NC", "NC", 291),
            ("VE3ABC", "ON", 1),
            ("DL1ABC", "DL", 230),
            ("KH6ABC", "HI", 110),
            ("K1MD", "MD", 291),
        ]
        .into_iter()
        .map(|(call, exchange, dxcc)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!("20m")),
                ("MODE", json!("CW")),
                ("STX_STRING", json!("ANA")),
                ("SRX_STRING", json!(exchange)),
                ("DXCC", json!(dxcc)),
            ])
        })
        .collect::<Vec<_>>();

        let totals = score_contacts(
            rules,
            json!({
                "CATEGORY-POWER": "HIGH",
                "CATEGORY-STATION": "FIXED"
            }),
            &mut contacts,
        );

        assert_eq!(totals.qso_points, 18);
        assert_eq!(totals.multipliers, 5);
        assert_eq!(totals.bonus_points, 0);
        assert_eq!(totals.score, 90);
    }

    #[test]
    fn bundled_new_state_qso_party_rules_score_basic_contacts() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let cases = [
            ("VT-QSO-PARTY (In State)", "ADD", "BEN", "CW", 3),
            ("MN-QSO-PARTY (In State)", "AIT", "AIT", "SSB", 2),
            ("NC-QSO-PARTY (In State)", "ALA", "CAB", "SSB", 2),
            ("OK-QSO-PARTY (In State)", "ADA", "CAD", "SSB", 2),
            ("ID-QSO-PARTY (In State)", "ADA", "WA", "CW", 2),
            ("WI-QSO-PARTY (In State)", "ADA", "ASH", "SSB", 1),
            ("VA-QSO-PARTY (In State)", "ACC", "ALB", "SSB", 1),
        ];

        for (contest_id, sent, received, mode, points) in cases {
            let rules = store
                .get(contest_id)
                .expect("new contest rules should load");
            let mut contacts = vec![contact(vec![
                ("CALL", json!("W1ABC")),
                ("BAND", json!("20M")),
                ("MODE", json!(mode)),
                ("STX_STRING", json!(sent)),
                ("SRX_STRING", json!(received)),
            ])];
            let totals = score_contacts(rules, Value::Null, &mut contacts);
            assert_eq!(totals.qso_points, points, "{contest_id}");
            assert_eq!(totals.multipliers, 1, "{contest_id}");
            assert_eq!(totals.score, points, "{contest_id}");
        }
    }

    #[test]
    fn every_bundled_scored_contest_has_representative_scoring_coverage() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let mut scored_contests = 0;

        for summary in store.summaries() {
            let rules = store
                .get(&summary.id)
                .expect("bundled contest should resolve");
            if rules.scoring.qso_points.is_none() {
                continue;
            }
            scored_contests += 1;
            let mut contacts = vec![representative_bundled_contact(rules)];
            let totals = score_contacts(rules, Value::Null, &mut contacts);

            assert!(
                totals.qso_points > 0,
                "{} representative contact should score points",
                rules.id
            );
            let multiplier_factor = if rules.scoring.multipliers.is_empty() {
                1
            } else {
                totals.multipliers
            };
            assert_eq!(
                totals.score,
                totals.qso_points * multiplier_factor,
                "{} representative score",
                rules.id
            );
        }

        assert_eq!(scored_contests, store.summaries().len());
    }

    #[test]
    fn eu_dx_rules_score_eu_and_dx_point_tables() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");

        let contact = |my_continent: &str,
                       my_prefix: &str,
                       call: &str,
                       dxcc: i64,
                       prefix: &str,
                       continent: &str,
                       received: &str| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!("20M")),
                ("MODE", json!("CW")),
                ("DXCC", json!(dxcc)),
                ("CONT", json!(continent)),
                ("MY_CONT", json!(my_continent)),
                ("APP_LOG73_DXCC_PFX", json!(prefix)),
                ("APP_LOG73_MY_DXCC_PFX", json!(my_prefix)),
                ("SRX_STRING", json!(received)),
            ])
        };

        let eu_rules = store.get("EUDX").expect("EU station rules should load");
        let mut eu_contacts = vec![
            contact("EU", "HA", "HA1ABC", 239, "HA", "EU", "HU01"),
            contact("EU", "HA", "DL1ABC", 230, "DL", "EU", "DE01"),
            contact("EU", "HA", "G1ABC", 223, "G", "EU", "27"),
            contact("EU", "HA", "K1ABC", 291, "K", "NA", "8"),
        ];
        let eu_totals = score_contacts(eu_rules, Value::Null, &mut eu_contacts);
        assert_eq!(eu_totals.qso_points, 20);
        assert_eq!(eu_totals.multipliers, 6);
        assert_eq!(eu_totals.score, 120);

        let dx_rules = store
            .get("EUDX (DX)")
            .expect("DX station rules should load");
        let mut dx_contacts = vec![
            contact("NA", "K", "DL1ABC", 230, "DL", "EU", "DE01"),
            contact("NA", "K", "K1ABC", 291, "K", "NA", "DE01"),
            contact("NA", "K", "VE3ABC", 1, "VE", "NA", "DE01"),
            contact("NA", "K", "JA1ABC", 339, "JA", "AS", "DE01"),
        ];
        let dx_totals = score_contacts(dx_rules, Value::Null, &mut dx_contacts);
        assert_eq!(dx_totals.qso_points, 20);
        assert_eq!(dx_totals.multipliers, 5);
        assert_eq!(dx_totals.score, 100);

        let mut mode_contacts = vec![
            contact("EU", "HA", "HA1ABC", 239, "HA", "EU", "HU01"),
            contact("EU", "HA", "HA1ABC", 239, "HA", "EU", "HU01"),
        ];
        crate::db::set_contact_adif(&mut mode_contacts[1], "MODE", json!("SSB"));
        let mode_totals = score_contacts(eu_rules, Value::Null, &mut mode_contacts);
        assert_eq!(mode_totals.qso_points, 4);
        assert_eq!(mode_totals.multipliers, 2);
        assert_eq!(mode_totals.score, 8);
        assert_eq!(
            contact_meta_value(&mode_contacts[0], "dupe"),
            Some(&json!(false))
        );
        assert_eq!(
            contact_meta_value(&mode_contacts[1], "dupe"),
            Some(&json!(false))
        );
    }

    #[test]
    fn pacc_and_all_asian_rules_score_their_exchange_and_multiplier_tables() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");

        let pacc_contact = |call: &str, mode: &str, dxcc: i64, prefix: &str, pfx: &str| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!("20M")),
                ("MODE", json!(mode)),
                ("DXCC", json!(dxcc)),
                ("APP_LOG73_DXCC_PFX", json!(prefix)),
                ("PFX", json!(pfx)),
                ("SRX_STRING", json!("NH")),
            ])
        };
        let pacc = store.get("PACC").expect("PACC rules should load");
        let mut pacc_contacts = vec![
            pacc_contact("DL1ABC", "CW", 230, "DL", "DL1"),
            pacc_contact("DL1ABC", "SSB", 230, "DL", "DL1"),
            pacc_contact("K1ABC", "CW", 291, "K", "K1"),
        ];
        let pacc_totals = score_contacts(pacc, Value::Null, &mut pacc_contacts);
        assert_eq!(pacc_totals.qso_points, 3);
        assert_eq!(pacc_totals.multipliers, 3);
        assert_eq!(pacc_totals.score, 9);

        let pacc_dx = store.get("PACC (DX)").expect("PACC DX rules should load");
        let mut pacc_dx_contacts = vec![
            pacc_contact("PA1ABC", "CW", 263, "PA", "PA1"),
            pacc_contact("DL1ABC", "CW", 230, "DL", "DL1"),
        ];
        let pacc_dx_totals = score_contacts(pacc_dx, Value::Null, &mut pacc_dx_contacts);
        assert_eq!(pacc_dx_totals.qso_points, 1);
        assert_eq!(pacc_dx_totals.multipliers, 1);
        assert_eq!(pacc_dx_totals.score, 1);

        let aadx = store
            .get("AADX-CW")
            .expect("All Asian CW rules should load");
        let aadx_contact =
            |call: &str, band: &str, dxcc: i64, continent: &str, prefix: &str, pfx: &str| {
                contact(vec![
                    ("CALL", json!(call)),
                    ("BAND", json!(band)),
                    ("MODE", json!("CW")),
                    ("DXCC", json!(dxcc)),
                    ("MY_DXCC", json!(339)),
                    ("CONT", json!(continent)),
                    ("MY_CONT", json!("AS")),
                    ("APP_LOG73_DXCC_PFX", json!(prefix)),
                    ("PFX", json!(pfx)),
                ])
            };
        let mut asian_contacts = vec![
            aadx_contact("JA1SAME", "20M", 339, "JA", "JA1", "JA1"),
            aadx_contact("HL1ABC", "160M", 137, "AS", "HL", "HL1"),
            aadx_contact("K1ABC", "80M", 291, "NA", "K", "K1"),
        ];
        let asian_totals = score_contacts(aadx, Value::Null, &mut asian_contacts);
        assert_eq!(asian_totals.qso_points, 9);
        assert_eq!(asian_totals.multipliers, 1);
        assert_eq!(asian_totals.score, 9);

        let mut non_asian_contacts = vec![
            contact(vec![
                ("CALL", json!("JA1ABC")),
                ("BAND", json!("160M")),
                ("MODE", json!("CW")),
                ("DXCC", json!(339)),
                ("MY_DXCC", json!(291)),
                ("CONT", json!("AS")),
                ("MY_CONT", json!("NA")),
                ("PFX", json!("JA1")),
            ]),
            contact(vec![
                ("CALL", json!("HL1ABC")),
                ("BAND", json!("80M")),
                ("MODE", json!("CW")),
                ("DXCC", json!(137)),
                ("MY_DXCC", json!(291)),
                ("CONT", json!("AS")),
                ("MY_CONT", json!("NA")),
                ("PFX", json!("HL1")),
            ]),
        ];
        let non_asian_totals = score_contacts(aadx, Value::Null, &mut non_asian_contacts);
        assert_eq!(non_asian_totals.qso_points, 5);
        assert_eq!(non_asian_totals.multipliers, 2);
        assert_eq!(non_asian_totals.score, 10);
    }

    #[test]
    fn new_dx_rules_score_home_and_foreign_variants() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");

        let dx_contact = |call: &str,
                          band: &str,
                          mode: &str,
                          dxcc: i64,
                          my_dxcc: i64,
                          continent: &str,
                          my_continent: &str,
                          received: &str| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!(mode)),
                ("DXCC", json!(dxcc)),
                ("MY_DXCC", json!(my_dxcc)),
                ("CONT", json!(continent)),
                ("MY_CONT", json!(my_continent)),
                ("SRX_STRING", json!(received)),
            ])
        };

        let ref_rules = store.get("REF-CW").expect("REF rules should load");
        let mut ref_contacts = vec![
            dx_contact("F1REF", "20M", "CW", 227, 227, "EU", "EU", "01"),
            dx_contact("DL1REF", "20M", "CW", 230, 227, "EU", "EU", "14"),
            dx_contact("K1REF", "20M", "CW", 291, 227, "NA", "EU", "8"),
            dx_contact("MM1REF/MM", "20M", "CW", 291, 227, "NA", "EU", "9"),
        ];
        let ref_totals = score_contacts(ref_rules, Value::Null, &mut ref_contacts);
        assert_eq!(ref_totals.qso_points, 12);
        assert_eq!(ref_totals.multipliers, 3);
        assert_eq!(ref_totals.score, 36);

        let ref_dx_rules = store.get("REF-CW (DX)").expect("REF DX rules should load");
        let mut ref_dx_contacts = vec![
            dx_contact("F1DX", "20M", "CW", 227, 291, "EU", "EU", "01"),
            dx_contact("TK1DX", "20M", "CW", 214, 291, "EU", "EU", "2A"),
            dx_contact("F2DX", "20M", "CW", 227, 291, "NA", "EU", "00"),
            dx_contact("K1DX", "20M", "CW", 291, 291, "NA", "EU", "8"),
        ];
        let ref_dx_totals = score_contacts(ref_dx_rules, Value::Null, &mut ref_dx_contacts);
        assert_eq!(ref_dx_totals.qso_points, 5);
        assert_eq!(ref_dx_totals.multipliers, 3);
        assert_eq!(ref_dx_totals.score, 15);

        let rdxc_rules = store.get("RDXC").expect("RDXC rules should load");
        let mut rdxc_contacts = vec![
            dx_contact("RA1RDX", "20M", "CW", 54, 54, "EU", "EU", "SP"),
            dx_contact("R9RDX", "20M", "CW", 15, 54, "AS", "EU", "AL"),
            dx_contact("DL1RDX", "20M", "CW", 230, 54, "EU", "EU", "001"),
            dx_contact("K1RDX", "20M", "CW", 291, 54, "NA", "EU", "002"),
            dx_contact("MM1RDX/MM", "20M", "CW", 291, 54, "NA", "EU", "003"),
        ];
        let rdxc_totals = score_contacts(rdxc_rules, Value::Null, &mut rdxc_contacts);
        assert_eq!(rdxc_totals.qso_points, 20);
        assert_eq!(rdxc_totals.multipliers, 6);
        assert_eq!(rdxc_totals.score, 120);

        let rdxc_dx_rules = store.get("RDXC (DX)").expect("RDXC DX rules should load");
        let mut rdxc_dx_contacts = vec![
            dx_contact("RA1DX", "20M", "CW", 54, 291, "EU", "NA", "SP"),
            dx_contact("R9DX", "20M", "CW", 15, 291, "AS", "NA", "AL"),
            dx_contact("K1DX", "20M", "CW", 291, 291, "NA", "NA", "001"),
            dx_contact("VE1DX", "20M", "CW", 1, 291, "NA", "NA", "002"),
            dx_contact("DL1DX", "20M", "CW", 230, 291, "EU", "NA", "003"),
        ];
        let rdxc_dx_totals = score_contacts(rdxc_dx_rules, Value::Null, &mut rdxc_dx_contacts);
        assert_eq!(rdxc_dx_totals.qso_points, 30);
        assert_eq!(rdxc_dx_totals.multipliers, 7);
        assert_eq!(rdxc_dx_totals.score, 210);

        let ari_rules = store.get("ARI-DX").expect("ARI rules should load");
        let mut ari_contacts = vec![
            dx_contact("I1ARI", "20M", "CW", 248, 248, "EU", "EU", "AL"),
            dx_contact("IS0ARI", "20M", "CW", 225, 248, "EU", "EU", "CA"),
            dx_contact("DL1ARI", "20M", "CW", 230, 248, "EU", "EU", "001"),
            dx_contact("K1ARI", "20M", "CW", 291, 248, "NA", "EU", "002"),
        ];
        let ari_totals = score_contacts(ari_rules, Value::Null, &mut ari_contacts);
        assert_eq!(ari_totals.qso_points, 24);
        assert_eq!(ari_totals.multipliers, 4);
        assert_eq!(ari_totals.score, 96);

        let ari_dx_rules = store.get("ARI-DX (DX)").expect("ARI DX rules should load");
        let mut ari_dx_contacts = vec![
            dx_contact("I1DX", "20M", "CW", 248, 291, "EU", "NA", "AL"),
            dx_contact("K1DX", "20M", "CW", 291, 291, "NA", "NA", "001"),
            dx_contact("VE1DX", "20M", "CW", 1, 291, "NA", "NA", "002"),
            dx_contact("DL1DX", "20M", "CW", 230, 291, "EU", "NA", "003"),
        ];
        let ari_dx_totals = score_contacts(ari_dx_rules, Value::Null, &mut ari_dx_contacts);
        assert_eq!(ari_dx_totals.qso_points, 14);
        assert_eq!(ari_dx_totals.multipliers, 1);
        assert_eq!(ari_dx_totals.score, 14);

        let euhfc_rules = store.get("EUHFC").expect("EUHFC rules should load");
        let mut euhfc_contacts = vec![
            dx_contact("DL1EU", "20M", "CW", 230, 227, "EU", "EU", "82"),
            dx_contact("F1EU", "20M", "SSB", 227, 227, "EU", "EU", "82"),
            dx_contact("G1EU", "20M", "CW", 223, 227, "EU", "EU", "17"),
            dx_contact("K1EU", "20M", "CW", 291, 227, "NA", "EU", "99"),
        ];
        let euhfc_totals = score_contacts(euhfc_rules, Value::Null, &mut euhfc_contacts);
        assert_eq!(euhfc_totals.qso_points, 3);
        assert_eq!(euhfc_totals.multipliers, 2);
        assert_eq!(euhfc_totals.score, 6);

        let lz_rules = store.get("LZDX").expect("LZDX rules should load");
        let mut lz_contacts = vec![
            dx_contact("LZ1LZ", "20M", "CW", 212, 212, "EU", "EU", "BU"),
            dx_contact("DL1LZ", "20M", "CW", 230, 212, "EU", "EU", "14"),
            dx_contact("K1LZ", "20M", "CW", 291, 212, "NA", "EU", "8"),
        ];
        let lz_totals = score_contacts(lz_rules, Value::Null, &mut lz_contacts);
        assert_eq!(lz_totals.qso_points, 14);
        assert_eq!(lz_totals.multipliers, 5);
        assert_eq!(lz_totals.score, 70);

        let lz_dx_rules = store.get("LZDX (DX)").expect("LZDX DX rules should load");
        let mut lz_dx_contacts = vec![
            dx_contact("LZ1DX", "20M", "CW", 212, 291, "EU", "NA", "BU"),
            dx_contact("K1DX", "20M", "CW", 291, 291, "NA", "NA", "8"),
            dx_contact("DL1DX", "20M", "CW", 230, 291, "EU", "NA", "14"),
        ];
        let lz_dx_totals = score_contacts(lz_dx_rules, Value::Null, &mut lz_dx_contacts);
        assert_eq!(lz_dx_totals.qso_points, 14);
        assert_eq!(lz_dx_totals.multipliers, 3);
        assert_eq!(lz_dx_totals.score, 42);
    }

    #[test]
    fn rac_rules_score_official_stations_and_canadian_multipliers() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");

        let rac_contact =
            |id: i64, call: &str, band: &str, mode: &str, dxcc: i64, received: &str| {
                contact(vec![
                    ("id", json!(id)),
                    ("CALL", json!(call)),
                    ("BAND", json!(band)),
                    ("MODE", json!(mode)),
                    ("DXCC", json!(dxcc)),
                    ("MY_DXCC", json!(1)),
                    ("RAC_SECT", json!(received)),
                ])
            };

        let canada_day = store
            .get("CANADA-DAY")
            .expect("Canada Day rules should load");
        let mut canada_day_contacts = vec![
            rac_contact(1, "VA3RAC", "20M", "CW", 1, "ON"),
            rac_contact(2, "VE1ABC", "20M", "CW", 1, "NS"),
            rac_contact(3, "K1ABC", "20M", "CW", 291, "001"),
            rac_contact(4, "VE1ABC", "20M", "SSB", 1, "NS"),
        ];
        let canada_day_totals = score_contacts(canada_day, Value::Null, &mut canada_day_contacts);
        assert_eq!(canada_day_totals.qso_points, 42);
        assert_eq!(canada_day_totals.multipliers, 3);
        assert_eq!(canada_day_totals.score, 126);

        let mut no_canadian_contacts = vec![rac_contact(5, "K1DX", "20M", "CW", 291, "001")];
        let no_canadian_totals = score_contacts(canada_day, Value::Null, &mut no_canadian_contacts);
        assert_eq!(no_canadian_totals.qso_points, 2);
        assert_eq!(no_canadian_totals.multipliers, 1);
        assert_eq!(no_canadian_totals.score, 2);

        let canada_winter = store
            .get("CANADA-WINTER")
            .expect("Canada Winter rules should load");
        let mut canada_winter_contacts = vec![
            rac_contact(6, "VE3WIN", "2M", "CW", 1, "ON"),
            rac_contact(7, "W1WIN", "2M", "CW", 291, "001"),
        ];
        let canada_winter_totals =
            score_contacts(canada_winter, Value::Null, &mut canada_winter_contacts);
        assert_eq!(canada_winter_totals.qso_points, 12);
        assert_eq!(canada_winter_totals.multipliers, 1);
        assert_eq!(canada_winter_totals.score, 12);

        let module = Arc::new(ContestScoringModule::new(canada_day.clone(), Value::Null));
        let tracker = IncrementalScoreTracker::new();
        let mut incremental_contacts = vec![rac_contact(8, "K1INC", "20M", "CW", 291, "001")];
        tracker.on_log_loaded(8, Arc::clone(&module), &mut incremental_contacts);
        let incremental_totals = tracker.totals(8).expect("incremental totals should exist");
        assert_eq!(incremental_totals.multipliers, 1);
        assert_eq!(incremental_totals.score, 2);
    }

    #[test]
    fn new_state_qso_parties_score_cw_contacts_and_first_multipliers() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let cases = [
            ("LA-QSO-PARTY (In State)", "Louisiana Parishes", 4),
            ("MS-QSO-PARTY (In State)", "Mississippi Counties", 2),
            ("NM-QSO-PARTY (In State)", "New Mexico Counties", 2),
            ("MO-QSO-PARTY (In State)", "Missouri Counties", 2),
            ("GA-QSO-PARTY (In State)", "Georgia Counties", 2),
            ("ND-QSO-PARTY (In State)", "North Dakota Counties", 1),
            ("MI-QSO-PARTY (In State)", "Michigan Counties", 2),
            ("NE-QSO-PARTY (In State)", "Nebraska Counties", 3),
            ("FL-QSO-PARTY (In State)", "States", 2),
        ];

        for (contest_id, value_set, expected_points) in cases {
            let rules = store
                .get(contest_id)
                .expect("new contest rules should load");
            let location = rules.value_sets[value_set]
                .first()
                .expect("contest should have a representative multiplier value");
            let mut contacts = vec![contact(vec![
                ("CALL", json!("W1ABC")),
                ("BAND", json!(rules.bands.first().expect("contest band"))),
                ("MODE", json!("CW")),
                ("STX_STRING", json!(location)),
                ("SRX_STRING", json!(location)),
                ("DXCC", json!(230)),
            ])];
            let totals = score_contacts(rules, Value::Null, &mut contacts);
            assert_eq!(totals.qso_points, expected_points, "{contest_id}");
            assert!(totals.multipliers > 0, "{contest_id}");
            assert_eq!(
                totals.score,
                expected_points * totals.multipliers,
                "{contest_id}"
            );
        }
    }

    #[test]
    fn newly_added_qso_parties_score_representative_contacts() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let cases = [
            ("7-QSO-PARTY (In State)", "7QP Counties", "CA", 3),
            ("7-QSO-PARTY", "7QP Counties", "AZAPH", 3),
            ("IN-QSO-PARTY (In State)", "Indiana Counties", "CA", 2),
            ("IN-QSO-PARTY", "Indiana Counties", "INADA", 2),
            ("DE-QSO-PARTY (In State)", "Delaware Counties", "CA", 2),
            ("DE-QSO-PARTY", "Delaware Counties", "KDE", 20),
            (
                "NEW-ENGLAND-QSO-PARTY (In State)",
                "New England Counties",
                "CTCAP",
                2,
            ),
            ("NEW-ENGLAND-QSO-PARTY", "New England Counties", "CTCAP", 2),
            ("AR-QSO-PARTY (In State)", "Arkansas Counties", "ARK", 1),
            ("AR-QSO-PARTY", "Arkansas Counties", "ARK", 1),
            ("KY-QSO-PARTY (In State)", "Kentucky Counties", "CA", 2),
            ("KY-QSO-PARTY", "Kentucky Counties", "ADA", 2),
            ("AL-QSO-PARTY (In State)", "Alabama Counties", "CA", 2),
            ("AL-QSO-PARTY", "Alabama Counties", "AUTA", 2),
        ];

        for (contest_id, county_set, received, expected_points) in cases {
            let rules = store
                .get(contest_id)
                .expect("new contest rules should load");
            let sent = if contest_id.ends_with("(In State)") {
                rules.value_sets[county_set]
                    .first()
                    .expect("contest should have a representative sent location")
                    .clone()
            } else {
                "CA".to_string()
            };
            let mut contacts = vec![contact(vec![
                ("CALL", json!("W1ABC")),
                ("BAND", json!(rules.bands.first().expect("contest band"))),
                ("MODE", json!("CW")),
                ("STX_STRING", json!(sent)),
                ("SRX_STRING", json!(received)),
                ("DXCC", json!(291)),
            ])];
            let totals = score_contacts(rules, Value::Null, &mut contacts);
            assert_eq!(totals.qso_points, expected_points, "{contest_id}");
            assert!(totals.multipliers > 0, "{contest_id}");
            assert_eq!(
                totals.score,
                expected_points * totals.multipliers,
                "{contest_id}"
            );
        }
    }

    #[test]
    fn bundled_tn_qso_party_rules_score_bands_locations_and_bonus_station() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let in_state = store
            .get("TN-QSO-PARTY (In State)")
            .expect("in-state Tennessee rules should load");
        let outside = store
            .get("TN-QSO-PARTY")
            .expect("outside-Tennessee rules should load");

        let mut in_state_contacts = [
            ("K4TCG", "20m", "CW", "ANDE", "BEDF", 291),
            ("K4TCG", "20m", "SSB", "ANDE", "BEDF", 291),
            ("K1ABC", "20m", "CW", "ANDE", "NC", 291),
            ("VE3ABC", "20m", "CW", "ANDE", "ON", 1),
            ("DL1ABC", "20m", "CW", "ANDE", "DL", 230),
            ("KL7ABC", "20m", "CW", "ANDE", "AK", 6),
        ]
        .into_iter()
        .map(|(call, band, mode, sent, received, dxcc)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!(mode)),
                ("STX_STRING", json!(sent)),
                ("SRX_STRING", json!(received)),
                ("DXCC", json!(dxcc)),
            ])
        })
        .collect::<Vec<_>>();
        let in_state_totals = score_contacts(in_state, Value::Null, &mut in_state_contacts);
        assert_eq!(in_state_totals.qso_points, 18);
        assert_eq!(in_state_totals.multipliers, 5);
        assert_eq!(in_state_totals.bonus_points, 200);
        assert_eq!(in_state_totals.score, 290);

        let mut outside_contacts = [
            ("W4AAA", "20m", "CW", "GA", "ANDE"),
            ("W4AAA", "20m", "SSB", "GA", "ANDE"),
            ("W4BBB", "40m", "CW", "GA", "ANDE"),
            ("K4TCG", "20m", "CW", "GA", "BEDF"),
        ]
        .into_iter()
        .map(|(call, band, mode, sent, received)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!(mode)),
                ("STX_STRING", json!(sent)),
                ("SRX_STRING", json!(received)),
            ])
        })
        .collect::<Vec<_>>();
        let outside_totals = score_contacts(outside, Value::Null, &mut outside_contacts);
        assert_eq!(outside_totals.qso_points, 12);
        assert_eq!(outside_totals.multipliers, 3);
        assert_eq!(outside_totals.bonus_points, 100);
        assert_eq!(outside_totals.score, 136);
    }

    #[test]
    fn bundled_ks_qso_party_rules_score_county_changes_and_bonus_station() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let in_state = store
            .get("KS-QSO-PARTY (In State)")
            .expect("in-state Kansas rules should load");
        let outside = store
            .get("KS-QSO-PARTY")
            .expect("outside-Kansas rules should load");

        let mut in_state_contacts = [
            ("K0MO", "20m", "CW", "ALL", "MO"),
            ("W0WYA", "20m", "CW", "ALL", "WYA"),
            ("W0ALL", "20m", "SSB", "ALL", "ALL"),
            ("K0MO", "20m", "CW", "WYA", "MO"),
            ("KS0KS", "40m", "SSB", "WYA", "DX"),
        ]
        .into_iter()
        .map(|(call, band, mode, sent, received)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!(mode)),
                ("STX_STRING", json!(sent)),
                ("SRX_STRING", json!(received)),
            ])
        })
        .collect::<Vec<_>>();

        let in_state_totals = score_contacts(in_state, Value::Null, &mut in_state_contacts);
        assert_eq!(in_state_totals.qso_points, 13);
        assert_eq!(in_state_totals.multipliers, 3);
        assert_eq!(in_state_totals.bonus_points, 100);
        assert_eq!(in_state_totals.score, 139);
        assert_eq!(
            contact_meta_value(&in_state_contacts[3], "dupe"),
            Some(&json!(false))
        );

        let mut outside_contacts = [
            ("W0ALL", "20m", "CW", "MO", "ALL"),
            ("W0WYA", "20m", "CW", "MO", "WYA"),
            ("W0ALL", "20m", "SSB", "MO", "ALL"),
        ]
        .into_iter()
        .map(|(call, band, mode, sent, received)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!(mode)),
                ("STX_STRING", json!(sent)),
                ("SRX_STRING", json!(received)),
            ])
        })
        .collect::<Vec<_>>();

        let outside_totals = score_contacts(outside, Value::Null, &mut outside_contacts);
        assert_eq!(outside_totals.qso_points, 8);
        assert_eq!(outside_totals.multipliers, 2);
        assert_eq!(outside_totals.score, 16);
    }

    #[test]
    fn bundled_oh_qso_party_rules_score_modes_and_mobile_locations() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let in_state = store
            .get("OH-QSO-PARTY (In State)")
            .expect("in-state Ohio rules should load");
        let outside = store
            .get("OH-QSO-PARTY")
            .expect("outside-Ohio rules should load");

        let mut in_state_contacts = [
            ("K1AAA", "20m", "CW", "FRAN", "MD"),
            ("K1AAA", "20m", "CW", "FRAN", "MD"),
            ("K1AAA", "20m", "CW", "DELA", "MD"),
            ("K1BBB", "40m", "SSB", "DELA", "MD"),
            ("K8CCC", "20m", "CW", "DELA", "ADAM"),
        ]
        .into_iter()
        .map(|(call, band, mode, sent, received)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!(mode)),
                ("STX_STRING", json!(sent)),
                ("SRX_STRING", json!(received)),
            ])
        })
        .collect::<Vec<_>>();
        let in_state_totals = score_contacts(in_state, Value::Null, &mut in_state_contacts);
        assert_eq!(in_state_totals.qso_points, 7);
        assert_eq!(in_state_totals.multipliers, 3);
        assert_eq!(in_state_totals.score, 21);
        assert_eq!(
            contact_meta_value(&in_state_contacts[1], "dupe"),
            Some(&json!(true))
        );
        assert_eq!(
            contact_meta_value(&in_state_contacts[2], "dupe"),
            Some(&json!(false))
        );

        let mut outside_contacts = [
            ("K8AAA", "20m", "CW", "PA", "ADAM"),
            ("K8AAA", "20m", "CW", "WV", "ADAM"),
            ("K8BBB", "40m", "SSB", "WV", "ADAM"),
        ]
        .into_iter()
        .map(|(call, band, mode, sent, received)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!(mode)),
                ("STX_STRING", json!(sent)),
                ("SRX_STRING", json!(received)),
            ])
        })
        .collect::<Vec<_>>();
        let outside_totals = score_contacts(outside, Value::Null, &mut outside_contacts);
        assert_eq!(outside_totals.qso_points, 5);
        assert_eq!(outside_totals.multipliers, 2);
        assert_eq!(outside_totals.score, 10);
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

    #[test]
    fn incremental_tracker_recalculates_multiplier_count_bonus_after_delete() {
        let mut rules = test_rules(
            fixed_points(1),
            vec!["CALL", "BAND", "MODE"],
            vec![state_multiplier()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        rules
            .scoring
            .multiplier_count_bonus_points
            .push(MultiplierCountBonusRule {
                id: "state-sweep".to_string(),
                name: "State Sweep".to_string(),
                multiplier: "State".to_string(),
                thresholds: BTreeMap::from([(2, 250), (3, 500)]),
            });
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
                ("STATE", json!("GA")),
            ]),
        ];

        tracker.on_log_loaded(9, Arc::clone(&module), &mut contacts);
        let totals = tracker.totals(9).expect("totals should exist");
        assert_eq!(totals.bonus_points, 500);
        assert_eq!(totals.score, 509);

        let deleted = contacts.remove(2);
        tracker.on_contact_deleted(9, module, &mut contacts, &deleted);

        let totals = tracker.totals(9).expect("totals should exist");
        assert_eq!(totals.bonus_points, 250);
        assert_eq!(totals.score, 254);
    }

    #[test]
    fn bundled_arrl_sweepstakes_scores_sections_once_and_dupes_calls_across_bands() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let rules = store
            .get("ARRL-SS-CW")
            .expect("ARRL Sweepstakes CW rules should load");
        let mut contacts = vec![
            contact(vec![
                ("CALL", json!("W1AW")),
                ("BAND", json!("20m")),
                ("ARRL_SECT", json!("CT")),
            ]),
            contact(vec![
                ("CALL", json!("W1AW")),
                ("BAND", json!("40m")),
                ("ARRL_SECT", json!("CT")),
            ]),
            contact(vec![
                ("CALL", json!("VE3ABC")),
                ("BAND", json!("20m")),
                ("ARRL_SECT", json!("ONE")),
            ]),
        ];

        let totals = score_contacts(rules, Value::Null, &mut contacts);

        assert_eq!(totals.qso_count, 3);
        assert_eq!(totals.qso_points, 4);
        assert_eq!(totals.multipliers, 2);
        assert_eq!(totals.score, 8);
        assert_eq!(contact_meta_value(&contacts[1], "dupe"), Some(&json!(true)));
    }

    #[test]
    fn bundled_na_sprint_ssb_scores_na_and_dx_entries() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let north_america = store
            .get("NA-SPRINT-SSB (North America)")
            .expect("North America NA Sprint rules should load");
        let dx = store
            .get("NA-SPRINT-SSB (DX)")
            .expect("DX NA Sprint rules should load");

        let mut north_america_contacts = [
            ("DL1ABC", "20m", "DX"),
            ("VE3ABC", "40m", "ON"),
            ("XE1ABC", "80m", "XE"),
        ]
        .into_iter()
        .map(|(call, band, qth)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!("SSB")),
                ("STX_STRING", json!("MA")),
                ("SRX_STRING", json!(qth)),
            ])
        })
        .collect::<Vec<_>>();
        let north_america_totals =
            score_contacts(north_america, Value::Null, &mut north_america_contacts);
        assert_eq!(north_america_totals.qso_points, 3);
        assert_eq!(north_america_totals.multipliers, 2);
        assert_eq!(north_america_totals.score, 6);

        let mut dx_contacts = [("K1ABC", "20m", "MA"), ("VE3ABC", "40m", "ON")]
            .into_iter()
            .map(|(call, band, qth)| {
                contact(vec![
                    ("CALL", json!(call)),
                    ("BAND", json!(band)),
                    ("MODE", json!("SSB")),
                    ("STX_STRING", json!("DX")),
                    ("SRX_STRING", json!(qth)),
                ])
            })
            .collect::<Vec<_>>();
        let dx_totals = score_contacts(dx, Value::Null, &mut dx_contacts);
        assert_eq!(dx_totals.qso_points, 2);
        assert_eq!(dx_totals.multipliers, 2);
        assert_eq!(dx_totals.score, 4);
    }

    #[test]
    fn bundled_na_sprint_cw_scores_na_and_dx_entries() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let north_america = store
            .get("NA-SPRINT-CW (North America)")
            .expect("North America NA Sprint CW rules should load");
        let dx = store
            .get("NA-SPRINT-CW (DX)")
            .expect("DX NA Sprint CW rules should load");

        let mut north_america_contacts = [("DL1ABC", "20m", "DX"), ("VP9ABC", "40m", "VP9")]
            .into_iter()
            .map(|(call, band, qth)| {
                contact(vec![
                    ("CALL", json!(call)),
                    ("BAND", json!(band)),
                    ("MODE", json!("CW")),
                    ("STX_STRING", json!("MA")),
                    ("SRX_STRING", json!(qth)),
                ])
            })
            .collect::<Vec<_>>();
        let north_america_totals =
            score_contacts(north_america, Value::Null, &mut north_america_contacts);
        assert_eq!(north_america_totals.qso_points, 2);
        assert_eq!(north_america_totals.multipliers, 1);
        assert_eq!(north_america_totals.score, 2);

        let mut dx_contacts = [("K1ABC", "20m", "MA"), ("DL1ABC", "40m", "DX")]
            .into_iter()
            .map(|(call, band, qth)| {
                contact(vec![
                    ("CALL", json!(call)),
                    ("BAND", json!(band)),
                    ("MODE", json!("CW")),
                    ("STX_STRING", json!("DX")),
                    ("SRX_STRING", json!(qth)),
                ])
            })
            .collect::<Vec<_>>();
        let dx_totals = score_contacts(dx, Value::Null, &mut dx_contacts);
        assert_eq!(dx_totals.qso_points, 1);
        assert_eq!(dx_totals.multipliers, 1);
        assert_eq!(dx_totals.score, 1);
    }

    #[test]
    fn bundled_arrl_dx_scores_wve_and_dx_entries_per_band() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let wve = store
            .get("ARRL-DX-CW")
            .expect("W/VE ARRL DX rules should load");
        let dx = store
            .get("ARRL-DX-CW (DX)")
            .expect("DX ARRL DX rules should load");

        let mut wve_contacts = [
            ("DL1ABC", "20m", 230, "KW"),
            ("DL1ABC", "40m", 230, "KW"),
            ("F1ABC", "20m", 227, "500"),
            ("VE3ABC", "20m", 1, "100"),
        ]
        .into_iter()
        .map(|(call, band, dxcc, power)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!("CW")),
                ("DXCC", json!(dxcc)),
                ("SRX_STRING", json!(power)),
            ])
        })
        .collect::<Vec<_>>();
        let wve_totals = score_contacts(wve, Value::Null, &mut wve_contacts);
        assert_eq!(wve_totals.qso_points, 9);
        assert_eq!(wve_totals.multipliers, 3);
        assert_eq!(wve_totals.score, 27);
        assert_eq!(contact_meta_value(&wve_contacts[3], "pts"), Some(&json!(0)));

        let mut single_band_contacts = [("DL1ABC", "20m", 230), ("F1ABC", "40m", 227)]
            .into_iter()
            .map(|(call, band, dxcc)| {
                contact(vec![
                    ("CALL", json!(call)),
                    ("BAND", json!(band)),
                    ("MODE", json!("CW")),
                    ("DXCC", json!(dxcc)),
                    ("SRX_STRING", json!("KW")),
                ])
            })
            .collect::<Vec<_>>();
        let single_band_totals = score_contacts(
            wve,
            json!({ "CATEGORY-BAND": "20M" }),
            &mut single_band_contacts,
        );
        assert_eq!(single_band_totals.qso_points, 3);
        assert_eq!(single_band_totals.multipliers, 1);
        assert_eq!(single_band_totals.score, 3);

        let mut dx_contacts = [
            ("K1ABC", "20m", 291, "MA"),
            ("K1ABC", "40m", 291, "MA"),
            ("VE1ABC", "20m", 1, "LB"),
            ("KL7ABC", "20m", 6, "AK"),
        ]
        .into_iter()
        .map(|(call, band, dxcc, location)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!("CW")),
                ("DXCC", json!(dxcc)),
                ("SRX_STRING", json!(location)),
            ])
        })
        .collect::<Vec<_>>();
        let dx_totals = score_contacts(dx, Value::Null, &mut dx_contacts);
        assert_eq!(dx_totals.qso_points, 9);
        assert_eq!(dx_totals.multipliers, 3);
        assert_eq!(dx_totals.score, 27);
        assert_eq!(contact_meta_value(&dx_contacts[3], "pts"), Some(&json!(0)));
    }

    #[test]
    fn bundled_naqp_scores_per_band_and_excludes_dx_entries() {
        let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/contest-rules");
        let store = ContestRulesStore::load_dirs([rules_dir.as_path()])
            .expect("bundled contest rules should load");
        let north_america = store
            .get("NAQP-CW (North America)")
            .expect("North America NAQP CW rules should load");
        let dx = store
            .get("NAQP-CW (DX)")
            .expect("DX NAQP CW rules should load");

        let mut north_america_contacts = [
            ("DL1ABC", "20m", "DX"),
            ("VE3ABC", "20m", "ON"),
            ("VE3ABC", "40m", "ON"),
            ("VE3ABC", "40m", "ON"),
        ]
        .into_iter()
        .map(|(call, band, qth)| {
            contact(vec![
                ("CALL", json!(call)),
                ("BAND", json!(band)),
                ("MODE", json!("CW")),
                ("SRX_STRING", json!(qth)),
            ])
        })
        .collect::<Vec<_>>();
        let north_america_totals =
            score_contacts(north_america, Value::Null, &mut north_america_contacts);
        assert_eq!(north_america_totals.qso_points, 3);
        assert_eq!(north_america_totals.multipliers, 2);
        assert_eq!(north_america_totals.score, 6);

        let mut dx_contacts = [("K1ABC", "20m", "MA"), ("DL1ABC", "40m", "DX")]
            .into_iter()
            .map(|(call, band, qth)| {
                contact(vec![
                    ("CALL", json!(call)),
                    ("BAND", json!(band)),
                    ("MODE", json!("CW")),
                    ("SRX_STRING", json!(qth)),
                ])
            })
            .collect::<Vec<_>>();
        let dx_totals = score_contacts(dx, Value::Null, &mut dx_contacts);
        assert_eq!(dx_totals.qso_points, 1);
        assert_eq!(dx_totals.multipliers, 1);
        assert_eq!(dx_totals.score, 1);
    }

    fn contact_by_id(contacts: &[Contact], id: i64) -> Contact {
        contacts
            .iter()
            .find(|contact| contact_id_for(contact) == Some(id))
            .cloned()
            .expect("contact id should exist")
    }
}
