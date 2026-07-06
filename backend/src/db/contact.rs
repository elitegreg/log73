use serde_json::{Map, Value};

pub type Contact = Map<String, Value>;
pub type ContactFields = Map<String, Value>;

const META_KEY: &str = "meta";
const ADIF_KEY: &str = "adif";

pub fn build_contact(meta: ContactFields, adif: ContactFields) -> Contact {
    let mut contact = Map::new();
    contact.insert(META_KEY.to_string(), Value::Object(meta));
    contact.insert(ADIF_KEY.to_string(), Value::Object(adif));
    contact
}

pub fn contact_meta(contact: &Contact) -> Option<&ContactFields> {
    contact.get(META_KEY).and_then(Value::as_object)
}

pub fn contact_adif(contact: &Contact) -> Option<&ContactFields> {
    contact.get(ADIF_KEY).and_then(Value::as_object)
}

pub fn contact_meta_value<'a>(contact: &'a Contact, key: &str) -> Option<&'a Value> {
    contact_meta(contact).and_then(|meta| meta.get(key))
}

pub fn contact_adif_value<'a>(contact: &'a Contact, key: &str) -> Option<&'a Value> {
    contact_adif(contact).and_then(|adif| adif.get(key))
}

pub fn set_contact_meta(contact: &mut Contact, key: &str, value: Value) {
    if !matches!(contact.get(META_KEY), Some(Value::Object(_))) {
        contact.insert(META_KEY.to_string(), Value::Object(Map::new()));
    }
    if let Some(meta) = contact.get_mut(META_KEY).and_then(Value::as_object_mut) {
        meta.insert(key.to_string(), value);
    }
}

#[cfg(test)]
pub fn set_contact_adif(contact: &mut Contact, key: &str, value: Value) {
    if !matches!(contact.get(ADIF_KEY), Some(Value::Object(_))) {
        contact.insert(ADIF_KEY.to_string(), Value::Object(Map::new()));
    }
    if let Some(adif) = contact.get_mut(ADIF_KEY).and_then(Value::as_object_mut) {
        adif.insert(key.to_string(), value);
    }
}

pub fn contact_id(contact: &Contact) -> Option<i64> {
    contact_meta_value(contact, "id").and_then(json_i64_value)
}

pub fn contact_log_id(contact: &Contact) -> Option<i64> {
    contact_meta_value(contact, "logId").and_then(json_i64_value)
}

pub(super) fn json_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(json_i64_value)
}

fn json_i64_value(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().map(|value| value as i64)),
        Value::String(string) => string.parse::<i64>().ok(),
        _ => None,
    }
}

pub(super) fn frequency_hz(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(decimal_frequency_to_hz)),
        Value::String(string) => {
            if string.contains('.') {
                string.parse::<f64>().ok().map(decimal_frequency_to_hz)
            } else {
                string.parse::<i64>().ok()
            }
        }
        _ => None,
    }
}

fn decimal_frequency_to_hz(value: f64) -> i64 {
    if value.abs() < 1_000_000.0 {
        (value * 1_000_000.0).round() as i64
    } else {
        value.round() as i64
    }
}

pub(super) fn json_string(value: Option<&Value>) -> Option<String> {
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
    use serde_json::json;

    #[test]
    fn frequency_hz_normalizes_integer_and_decimal_values() {
        assert_eq!(frequency_hz(Some(&json!(14074000))), Some(14_074_000));
        assert_eq!(frequency_hz(Some(&json!("14.074"))), Some(14_074_000));
        assert_eq!(frequency_hz(Some(&json!(14.074))), Some(14_074_000));
    }

    #[test]
    fn contact_helpers_expose_meta_and_adif_views() {
        let contact = build_contact(
            Map::from_iter([("id".to_string(), json!(42))]),
            Map::from_iter([("CALL".to_string(), json!("K1ABC"))]),
        );

        assert_eq!(contact_id(&contact), Some(42));
        assert_eq!(contact_adif_value(&contact, "CALL"), Some(&json!("K1ABC")));
    }
}
