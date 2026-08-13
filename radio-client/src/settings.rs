use radio_io::RadioSettings;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;
pub const SETTINGS_FILE_NAME: &str = "log73-radio-client.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RadioClientSettings {
    pub schema_version: u32,
    pub client_instance_id: Uuid,
    pub backend: BackendSettings,
    pub radio: RadioSettings,
}

impl Default for RadioClientSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            client_instance_id: Uuid::new_v4(),
            backend: BackendSettings::default(),
            radio: RadioSettings::default(),
        }
    }
}

impl RadioClientSettings {
    #[allow(dead_code)] // Used by the C2 configuration screen's Set defaults action.
    pub fn reset_radio_defaults(&mut self) {
        self.radio = RadioSettings::default();
    }

    pub fn is_radio_configured(&self) -> bool {
        radio_io::validate_radio_settings(&self.radio).is_ok()
    }

    pub fn diagnostic_summary(&self) -> String {
        format!(
            "schema_version={} client_instance_id={} backend_base_url={} username={} password={} authorization={} radio={}",
            self.schema_version,
            self.client_instance_id,
            diagnostic_value("backend_base_url", &self.backend.base_url),
            diagnostic_value("username", &self.backend.username),
            diagnostic_value("password", &self.backend.password),
            diagnostic_value("authorization", "not-present"),
            diagnostic_value("radio", &self.radio.name),
        )
    }
}

fn diagnostic_value(field: &str, value: &str) -> String {
    let field = field.to_ascii_lowercase();
    if field.contains("password")
        || field.contains("authorization")
        || field.contains("username")
        || field.contains("base_url")
        || field == "radio"
    {
        return "<redacted>".to_string();
    }
    value.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct BackendSettings {
    pub base_url: String,
    pub username: String,
    pub password: String,
}

impl Default for BackendSettings {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:7300".to_string(),
            username: String::new(),
            password: String::new(),
        }
    }
}

pub fn settings_file_path(config_dir: impl AsRef<Path>) -> PathBuf {
    config_dir.as_ref().join(SETTINGS_FILE_NAME)
}

pub fn load_or_create(path: &Path) -> Result<RadioClientSettings, String> {
    match fs::read_to_string(path) {
        Ok(contents) => load_from_str(&contents),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let settings = RadioClientSettings::default();
            save_atomic(path, &settings)?;
            Ok(settings)
        }
        Err(error) => Err(format!(
            "failed to read Radio Client settings at {}: {error}",
            path.display()
        )),
    }
}

fn load_from_str(contents: &str) -> Result<RadioClientSettings, String> {
    let value: serde_json::Value = serde_json::from_str(contents)
        .map_err(|error| format!("invalid Radio Client settings JSON: {error}"))?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "Radio Client settings are missing schema_version".to_string())?;
    if schema_version != u64::from(SETTINGS_SCHEMA_VERSION) {
        return Err(format!(
            "unsupported Radio Client settings schema version {schema_version}; this version supports schema version {SETTINGS_SCHEMA_VERSION}"
        ));
    }
    serde_json::from_value(value).map_err(|error| format!("invalid Radio Client settings: {error}"))
}

pub fn save_atomic(path: &Path, settings: &RadioClientSettings) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| {
        format!(
            "Radio Client settings path has no parent: {}",
            path.display()
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "failed to create Radio Client config directory {}: {error}",
            parent.display()
        )
    })?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(SETTINGS_FILE_NAME);
    let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    let result = write_and_replace(&temporary, path, settings);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_and_replace(
    temporary: &Path,
    destination: &Path,
    settings: &RadioClientSettings,
) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(temporary)
        .map_err(|error| format!("failed to create temporary settings file: {error}"))?;
    let mut serialized = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("failed to serialize Radio Client settings: {error}"))?;
    serialized.push(b'\n');
    file.write_all(&serialized)
        .map_err(|error| format!("failed to write Radio Client settings: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("failed to flush Radio Client settings: {error}"))?;
    drop(file);
    replace_file(temporary, destination)
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(temporary, destination)
        .map_err(|error| format!("failed to replace Radio Client settings: {error}"))
}

#[cfg(windows)]
fn replace_file(temporary: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = temporary
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let replaced = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        return Err(format!(
            "failed to replace Radio Client settings: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "log73-radio-client-{name}-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn first_load_creates_and_reuses_a_stable_identity() {
        let path = test_path("identity");
        let first = load_or_create(&path).unwrap();
        let second = load_or_create(&path).unwrap();

        assert_eq!(first.client_instance_id, second.client_instance_id);
        assert!(path.exists());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn settings_round_trip_complete_shared_configuration() {
        let path = test_path("round-trip");
        let mut settings = RadioClientSettings::default();
        settings.backend.username = "operator".to_string();
        settings.backend.password = "secret".to_string();
        settings.radio.name = "Client radio".to_string();
        settings.radio.radio_kind = "dummy".to_string();
        settings.radio.transport_kind = "none".to_string();
        settings.radio.flrig_enabled = true;
        settings.radio.flrig_port = 23_456;

        save_atomic(&path, &settings).unwrap();
        assert_eq!(load_or_create(&path).unwrap(), settings);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn future_fields_are_ignored_but_future_schema_is_rejected() {
        let settings = RadioClientSettings::default();
        let mut value = serde_json::to_value(&settings).unwrap();
        value["future_field"] = serde_json::json!({ "anything": true });
        assert!(load_from_str(&value.to_string()).is_ok());

        value["schema_version"] = serde_json::json!(2);
        assert!(
            load_from_str(&value.to_string())
                .unwrap_err()
                .contains("supports schema version 1")
        );
    }

    #[test]
    fn reset_radio_defaults_preserves_identity_and_credentials() {
        let mut settings = RadioClientSettings::default();
        let identity = settings.client_instance_id;
        settings.backend.username = "operator".to_string();
        settings.backend.password = "secret".to_string();
        settings.radio.name = "Changed".to_string();

        settings.reset_radio_defaults();

        assert_eq!(settings.client_instance_id, identity);
        assert_eq!(settings.backend.username, "operator");
        assert_eq!(settings.backend.password, "secret");
        assert_eq!(settings.radio, RadioSettings::default());
    }

    #[test]
    fn diagnostics_do_not_include_credentials() {
        let mut settings = RadioClientSettings::default();
        settings.backend.base_url = "http://private.example:7300".to_string();
        settings.backend.username = "operator".to_string();
        settings.backend.password = "secret".to_string();
        let summary = settings.diagnostic_summary();

        assert!(!summary.contains("private.example"));
        assert!(!summary.contains("operator"));
        assert!(!summary.contains("secret"));
        assert!(summary.contains("password=<redacted>"));
        assert_eq!(
            diagnostic_value("Authorization", "Basic dXNlcjpzZWNyZXQ="),
            "<redacted>"
        );
    }

    #[cfg(unix)]
    #[test]
    fn settings_are_owner_only_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let path = test_path("permissions");
        save_atomic(&path, &RadioClientSettings::default()).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_file(path);
    }
}
