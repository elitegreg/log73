use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Band {
    pub iaru_region: i64,
    pub name: String,
    pub lower_hz: i64,
    pub upper_hz: i64,
    pub default_ssb_mode: String,
    pub sort_order: i64,
    pub cabrillo: String,
}

#[derive(Debug, Clone)]
pub struct BandCatalog {
    inner: Arc<RwLock<Arc<Vec<Band>>>>,
}

impl BandCatalog {
    pub fn new(bands: Vec<Band>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Arc::new(bands))),
        }
    }

    pub fn snapshot(&self) -> Arc<Vec<Band>> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn replace(&self, bands: Vec<Band>) {
        *self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(bands);
    }
}

impl Default for BandCatalog {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl From<Arc<Vec<Band>>> for BandCatalog {
    fn from(bands: Arc<Vec<Band>>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(bands)),
        }
    }
}

pub fn band_for_frequency(bands: &[Band], frequency_hz: u64) -> Option<&Band> {
    let frequency_hz = i64::try_from(frequency_hz).ok()?;
    bands
        .iter()
        .find(|band| frequency_hz >= band.lower_hz && frequency_hz <= band.upper_hz)
}
