use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Clone)]
pub struct ExchangeField {
    pub name: &'static str,
    #[serde(rename = "type")]
    pub field_type: &'static str,
    pub adif: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

#[derive(Serialize, Clone)]
pub struct ContestRules {
    pub contest: &'static str,
    pub allowed_bands: &'static [u16],
    pub allowed_modes: &'static [&'static str],
    pub exchange: Vec<ExchangeField>,
    pub qso_columns: Vec<&'static str>,
    pub qso_column_fields: BTreeMap<&'static str, &'static str>,
}

impl ContestRules {
    pub fn new() -> Self {
        let exchange = vec![
            ExchangeField {
                name: "RST(s)",
                field_type: "RST",
                adif: "RST_SENT",
                fixed: None,
                default: Some(serde_json::json!(599)),
            },
            ExchangeField {
                name: "County",
                field_type: "String:4",
                adif: "STX_STRING",
                fixed: Some(true),
                default: Some(serde_json::json!("BERK")),
            },
            ExchangeField {
                name: "RST(r)",
                field_type: "RST",
                adif: "RST_RCVD",
                fixed: None,
                default: None,
            },
            ExchangeField {
                name: "State",
                field_type: "String:4",
                adif: "SRX_STRING",
                fixed: None,
                default: None,
            },
        ];
        let mut qso_column_fields = BTreeMap::from([
            ("Freq", "FREQ"),
            ("Mode", "MODE"),
            ("Call", "CALL"),
            ("Op", "OPERATOR"),
        ]);

        for field in &exchange {
            qso_column_fields.insert(field.name, field.adif);
        }

        Self {
            contest: "SC-QSO-PARTY",
            allowed_bands: &[160, 80, 40, 20, 15, 10, 6, 2],
            allowed_modes: &["SSB", "FM", "AM", "CW"],
            exchange,
            qso_columns: vec![
                "Date/Time (UTC)",
                "Freq",
                "Mode",
                "Call",
                "RST(s)",
                "RST(r)",
                "Mult",
                "Pts",
                "Op",
            ],
            qso_column_fields,
        }
    }
}
