use radio_cat_rs::Frequency;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Band {
    pub iaru_region: i64,
    pub name: String,
    pub lower_hz: i64,
    pub upper_hz: i64,
    pub default_ssb_mode: String,
    pub sort_order: i64,
}

pub fn band_for_frequency(bands: &[Band], frequency: Frequency) -> Option<&Band> {
    let frequency_hz = i64::try_from(frequency.hz()).ok()?;
    bands
        .iter()
        .find(|band| frequency_hz >= band.lower_hz && frequency_hz <= band.upper_hz)
}

pub fn band_by_name<'a>(bands: &'a [Band], name: &str) -> Option<&'a Band> {
    let normalized = name.trim();
    bands
        .iter()
        .find(|band| band.name.eq_ignore_ascii_case(normalized))
}
