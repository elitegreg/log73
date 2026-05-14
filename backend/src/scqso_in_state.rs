use serde::Serialize;

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
}

impl ContestRules {
    pub fn new() -> Self {
        Self {
            contest: "SC-QSO-PARTY",
            allowed_bands: &[160, 80, 40, 20, 15, 10, 6, 2],
            allowed_modes: &["SSB", "FM", "AM", "CW"],
            exchange: vec![
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
            ],
            qso_columns: vec![
                "Time", "Freq", "Mode", "Call", "RST(s)", "RST(r)", "Mult", "Pts", "Op",
            ],
        }
    }
}
