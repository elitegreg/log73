use radio_cat_rs::Frequency;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
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

#[derive(Debug, Deserialize)]
struct BandCsvRow {
    band: String,
    start: String,
    end: String,
    cabrillo: String,
}

pub fn load_region_bands(
    user_data_dir: &Path,
    installed_data_dir: &Path,
    iaru_region: i64,
) -> Result<Vec<Band>, String> {
    if !(1..=3).contains(&iaru_region) {
        return Err(format!(
            "IARU region must be 1, 2, or 3; received {iaru_region}"
        ));
    }

    let file_name = format!("bands_{iaru_region}.csv");
    let user_path = user_data_dir.join(&file_name);
    match load_band_file(&user_path, iaru_region) {
        Ok(bands) => Ok(bands),
        Err(user_error) => {
            let installed_path = installed_data_dir.join(file_name);
            load_band_file(&installed_path, iaru_region).map_err(|installed_error| {
                format!(
                    "failed to load IARU Region {iaru_region} bands; user file {} failed: \
                     {user_error}; application file {} failed: {installed_error}",
                    user_path.display(),
                    installed_path.display()
                )
            })
        }
    }
}

fn load_band_file(path: &Path, iaru_region: i64) -> Result<Vec<Band>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(false)
        .from_path(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let headers = reader
        .headers()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !headers.iter().eq(["band", "start", "end", "cabrillo"]) {
        return Err(format!(
            "{}: expected header band,start,end,cabrillo; found {}",
            path.display(),
            headers.iter().collect::<Vec<_>>().join(",")
        ));
    }

    let mut bands = Vec::new();
    let mut names = HashSet::new();
    for (index, row) in reader.deserialize::<BandCsvRow>().enumerate() {
        let line = index + 2;
        let row = row.map_err(|error| format!("{}:{line}: {error}", path.display()))?;
        let name = row.band.trim();
        if name.is_empty() {
            return Err(format!("{}:{line}: band name is empty", path.display()));
        }
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(format!(
                "{}:{line}: duplicate band name {name}",
                path.display()
            ));
        }

        let lower_hz = parse_khz_as_hz(row.start.trim())
            .map_err(|error| format!("{}:{line}: invalid start: {error}", path.display()))?;
        let upper_hz = parse_khz_as_hz(row.end.trim())
            .map_err(|error| format!("{}:{line}: invalid end: {error}", path.display()))?;
        if lower_hz >= upper_hz {
            return Err(format!(
                "{}:{line}: band {name} start must be lower than end",
                path.display()
            ));
        }

        let cabrillo = row.cabrillo.trim();
        if cabrillo.is_empty() {
            return Err(format!(
                "{}:{line}: Cabrillo value is empty",
                path.display()
            ));
        }

        bands.push(Band {
            iaru_region,
            name: name.to_string(),
            lower_hz,
            upper_hz,
            default_ssb_mode: default_ssb_mode(name, lower_hz).to_string(),
            sort_order: i64::try_from(index + 1)
                .map_err(|_| format!("{}: too many band rows", path.display()))?,
            cabrillo: cabrillo.to_string(),
        });
    }

    if bands.is_empty() {
        return Err(format!("{}: band catalog is empty", path.display()));
    }
    validate_non_overlapping(path, &bands)?;
    Ok(bands)
}

fn parse_khz_as_hz(value: &str) -> Result<i64, String> {
    if value.is_empty() {
        return Err("frequency is empty".to_string());
    }
    let (whole, fraction) = match value.split_once('.') {
        Some((_whole, "")) => {
            return Err(format!(
                "{value:?} must be a positive decimal kHz value with at most three decimal places"
            ));
        }
        Some(parts) => parts,
        None => (value, ""),
    };
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > 3
    {
        return Err(format!(
            "{value:?} must be a positive decimal kHz value with at most three decimal places"
        ));
    }
    let whole = whole
        .parse::<i64>()
        .map_err(|_| format!("{value:?} is outside the supported frequency range"))?;
    let fraction_hz = if fraction.is_empty() {
        0
    } else {
        let fraction_digits = fraction.len();
        let fraction = fraction
            .parse::<i64>()
            .map_err(|_| format!("{value:?} has an invalid decimal fraction"))?;
        fraction * 10_i64.pow(u32::try_from(3 - fraction_digits).unwrap_or(0))
    };
    let frequency_hz = whole
        .checked_mul(1_000)
        .and_then(|whole_hz| whole_hz.checked_add(fraction_hz))
        .ok_or_else(|| format!("{value:?} is outside the supported frequency range"))?;
    if frequency_hz <= 0 {
        return Err(format!("{value:?} must be greater than zero"));
    }
    Ok(frequency_hz)
}

fn default_ssb_mode(name: &str, lower_hz: i64) -> &'static str {
    if name.eq_ignore_ascii_case("60m") || lower_hz >= 10_000_000 {
        "USB"
    } else {
        "LSB"
    }
}

fn validate_non_overlapping(path: &Path, bands: &[Band]) -> Result<(), String> {
    let mut by_frequency = bands.iter().collect::<Vec<_>>();
    by_frequency.sort_by_key(|band| band.lower_hz);
    for pair in by_frequency.windows(2) {
        let previous = pair[0];
        let current = pair[1];
        if current.lower_hz <= previous.upper_hz {
            return Err(format!(
                "{}: bands {} and {} overlap",
                path.display(),
                previous.name,
                current.name
            ));
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);
    const VALID_CSV: &str = "band,start,end,cabrillo\n80M,3500,4000,khz\n6M,50000,54000,50\n";

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let unique = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "log73-bands-{label}-{}-{timestamp}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("test directory is created");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, contents).expect("test CSV is written");
            path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn band(name: &str, lower_hz: i64, upper_hz: i64) -> Band {
        Band {
            iaru_region: 2,
            name: name.to_string(),
            lower_hz,
            upper_hz,
            default_ssb_mode: "USB".to_string(),
            sort_order: 1,
            cabrillo: "khz".to_string(),
        }
    }

    #[test]
    fn checked_in_region_catalogs_parse_and_include_expected_mappings() {
        let data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
        for region in 1..=3 {
            let bands = load_band_file(&data_dir.join(format!("bands_{region}.csv")), region)
                .expect("bundled band catalog should load");
            assert!(bands.len() >= 26);
            assert!(bands.iter().all(|band| band.iaru_region == region));
            assert_eq!(band_by_name(&bands, "6m").unwrap().cabrillo, "50");
            assert_eq!(band_by_name(&bands, "23CM").unwrap().cabrillo, "1.2G");
            assert_eq!(band_by_name(&bands, "20m").unwrap().cabrillo, "khz");
            let literal_values = [
                "50", "70", "144", "222", "432", "902", "1.2G", "2.3G", "3.4G", "5.7G", "10G",
                "24G", "47G", "75G", "122G", "134G", "241G",
            ];
            for band in &bands {
                if band.lower_hz < 30_000_000 {
                    assert_eq!(
                        band.cabrillo, "khz",
                        "{} in Region {region} should use kHz",
                        band.name
                    );
                } else {
                    assert!(
                        literal_values.contains(&band.cabrillo.as_str()),
                        "{} in Region {region} has unexpected Cabrillo value {}",
                        band.name,
                        band.cabrillo
                    );
                }
            }
        }
    }

    #[test]
    fn decimal_khz_is_converted_to_exact_hz() {
        for (input, expected) in [
            ("135.7", 135_700),
            ("137.8", 137_800),
            ("5351.5", 5_351_500),
            ("1.001", 1_001),
            ("472", 472_000),
        ] {
            assert_eq!(parse_khz_as_hz(input), Ok(expected));
        }
    }

    #[test]
    fn invalid_decimal_khz_values_are_rejected() {
        for input in ["", "0", "-1", ".5", "1.", "1.0001", "1e3", "abc"] {
            assert!(parse_khz_as_hz(input).is_err(), "{input:?} was accepted");
        }
    }

    #[test]
    fn valid_user_catalog_takes_precedence() {
        let user = TestDir::new("user-precedence-user");
        let app = TestDir::new("user-precedence-app");
        user.write(
            "bands_2.csv",
            "band,start,end,cabrillo\nUserBand,100,200,khz\n",
        );
        app.write(
            "bands_2.csv",
            "band,start,end,cabrillo\nAppBand,300,400,khz\n",
        );

        let bands = load_region_bands(user.path(), app.path(), 2).expect("catalog loads");
        assert_eq!(bands[0].name, "UserBand");
    }

    #[test]
    fn missing_user_catalog_falls_back_to_application_catalog() {
        let user = TestDir::new("missing-user");
        let app = TestDir::new("missing-app");
        app.write("bands_2.csv", VALID_CSV);

        let bands = load_region_bands(user.path(), app.path(), 2).expect("fallback catalog loads");
        assert_eq!(bands.len(), 2);
    }

    #[test]
    fn malformed_user_catalog_falls_back_to_application_catalog() {
        let user = TestDir::new("malformed-user");
        let app = TestDir::new("malformed-app");
        user.write("bands_2.csv", "bad,header\nvalue,value\n");
        app.write("bands_2.csv", VALID_CSV);

        let bands = load_region_bands(user.path(), app.path(), 2).expect("fallback catalog loads");
        assert_eq!(bands[0].name, "80M");
    }

    #[test]
    fn semantically_invalid_user_catalog_falls_back_to_application_catalog() {
        let user = TestDir::new("semantic-user");
        let app = TestDir::new("semantic-app");
        user.write(
            "bands_2.csv",
            "band,start,end,cabrillo\nFirst,100,200,khz\nSecond,150,250,khz\n",
        );
        app.write("bands_2.csv", VALID_CSV);

        let bands = load_region_bands(user.path(), app.path(), 2).expect("fallback catalog loads");
        assert_eq!(bands[0].name, "80M");
    }

    #[test]
    fn unreadable_user_catalog_falls_back_to_application_catalog() {
        let user = TestDir::new("unreadable-user");
        let app = TestDir::new("unreadable-app");
        fs::create_dir(user.path().join("bands_2.csv")).expect("directory placeholder is created");
        app.write("bands_2.csv", VALID_CSV);

        let bands = load_region_bands(user.path(), app.path(), 2).expect("fallback catalog loads");
        assert_eq!(bands[1].name, "6M");
    }

    #[test]
    fn both_catalog_failures_include_both_paths_and_causes() {
        let user = TestDir::new("both-fail-user");
        let app = TestDir::new("both-fail-app");
        user.write("bands_3.csv", "bad,header\n");
        app.write("bands_3.csv", "band,start,end,cabrillo\n");

        let error = load_region_bands(user.path(), app.path(), 3).unwrap_err();
        assert!(error.contains(&user.path().join("bands_3.csv").display().to_string()));
        assert!(error.contains(&app.path().join("bands_3.csv").display().to_string()));
        assert!(error.contains("expected header"));
        assert!(error.contains("band catalog is empty"));
    }

    #[test]
    fn invalid_regions_are_rejected_before_file_access() {
        let dir = TestDir::new("invalid-region");
        for region in [0, 4, -1] {
            assert!(load_region_bands(dir.path(), dir.path(), region).is_err());
        }
    }

    #[test]
    fn malformed_catalog_rows_are_rejected() {
        let cases = [
            (
                "header",
                "name,start,end,cabrillo\n80M,3500,4000,khz\n",
                "expected header",
            ),
            (
                "missing-field",
                "band,start,end,cabrillo\n80M,3500,4000\n",
                "found record with 3 fields",
            ),
            (
                "extra-field",
                "band,start,end,cabrillo\n80M,3500,4000,khz,extra\n",
                "found record with 5 fields",
            ),
            (
                "empty-name",
                "band,start,end,cabrillo\n,3500,4000,khz\n",
                "band name is empty",
            ),
            (
                "invalid-number",
                "band,start,end,cabrillo\n80M,nope,4000,khz\n",
                "invalid start",
            ),
            (
                "empty-cabrillo",
                "band,start,end,cabrillo\n80M,3500,4000,\n",
                "Cabrillo value is empty",
            ),
            (
                "reversed",
                "band,start,end,cabrillo\n80M,4000,3500,khz\n",
                "start must be lower than end",
            ),
            (
                "duplicate",
                "band,start,end,cabrillo\n80M,3500,4000,khz\n80m,7000,7300,khz\n",
                "duplicate band name",
            ),
            (
                "overlap",
                "band,start,end,cabrillo\n80M,3500,4000,khz\nOther,3999,4100,khz\n",
                "overlap",
            ),
        ];

        for (label, csv, expected) in cases {
            let dir = TestDir::new(label);
            let path = dir.write("bands.csv", csv);
            let error = load_band_file(&path, 2).unwrap_err();
            assert!(
                error.contains(expected),
                "{label}: expected {expected:?} in {error:?}"
            );
        }
    }

    #[test]
    fn row_order_and_default_sidebands_are_derived() {
        let dir = TestDir::new("derived-fields");
        let path = dir.write(
            "bands.csv",
            "band,start,end,cabrillo\n160M,1800,2000,khz\n60M,5351.5,5366.5,khz\n30M,10100,10150,khz\n",
        );
        let bands = load_band_file(&path, 1).expect("catalog loads");

        assert_eq!(
            bands
                .iter()
                .map(|band| (band.sort_order, band.default_ssb_mode.as_str()))
                .collect::<Vec<_>>(),
            vec![(1, "LSB"), (2, "USB"), (3, "USB")]
        );
    }

    #[test]
    fn band_lookups_are_case_insensitive_and_include_boundaries() {
        let bands = vec![band("20M", 14_000_000, 14_350_000)];
        assert_eq!(band_by_name(&bands, " 20m ").unwrap().name, "20M");
        assert!(band_for_frequency(&bands, Frequency::from_hz(14_000_000)).is_some());
        assert!(band_for_frequency(&bands, Frequency::from_hz(14_350_000)).is_some());
        assert!(band_for_frequency(&bands, Frequency::from_hz(13_999_999)).is_none());
        assert!(band_for_frequency(&bands, Frequency::from_hz(14_350_001)).is_none());
    }

    #[test]
    fn catalog_snapshots_are_atomically_replaced() {
        let catalog = BandCatalog::new(vec![band("20M", 14_000_000, 14_350_000)]);
        let old_snapshot = catalog.snapshot();
        catalog.replace(vec![band("6M", 50_000_000, 54_000_000)]);

        assert_eq!(old_snapshot[0].name, "20M");
        assert_eq!(catalog.snapshot()[0].name, "6M");
    }

    #[test]
    fn serialized_band_api_shape_includes_cabrillo() {
        let value =
            serde_json::to_value(band("6M", 50_000_000, 54_000_000)).expect("band serializes");

        assert_eq!(value["iaru_region"], 2);
        assert_eq!(value["name"], "6M");
        assert_eq!(value["lower_hz"], 50_000_000);
        assert_eq!(value["upper_hz"], 54_000_000);
        assert_eq!(value["default_ssb_mode"], "USB");
        assert_eq!(value["sort_order"], 1);
        assert_eq!(value["cabrillo"], "khz");
    }
}
