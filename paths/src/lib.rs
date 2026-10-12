use std::path::{Path, PathBuf};

const APP_NAME: &str = "log73";

pub fn config_dir() -> PathBuf {
    project_dirs()
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(fallback_project_config_dir)
}

pub fn data_dir() -> PathBuf {
    project_dirs()
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(fallback_project_data_dir)
}

fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("com", "", APP_NAME)
}

pub fn installed_data_dir(app_root: impl AsRef<Path>) -> PathBuf {
    let app_root = app_root.as_ref();
    if is_macos_app_bundle(app_root) {
        app_root.join("Contents").join("Resources").join("data")
    } else {
        app_root.join("data")
    }
}

pub fn contest_rules_dir(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join("contest-rules")
}

pub fn database_path(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join("log73.db")
}

pub fn log_file_path(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join("log73-backend.log")
}

pub fn launcher_log_file_path(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join("log73-launcher.log")
}

pub fn radio_client_log_file_path(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join("log73-radio-client.log")
}

pub fn app_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| app_root_from_executable(&path))
        .unwrap_or_else(default_app_root)
}

pub fn app_root_from_executable(executable: impl AsRef<Path>) -> Option<PathBuf> {
    let executable_dir = executable.as_ref().parent()?;
    if executable_dir.file_name().and_then(|name| name.to_str()) == Some("bin") {
        return executable_dir.parent().map(Path::to_path_buf);
    }

    if matches!(
        executable_dir.file_name().and_then(|name| name.to_str()),
        Some("MacOS" | "Helpers")
    ) {
        let contents_dir = executable_dir.parent()?;
        if contents_dir.file_name().and_then(|name| name.to_str()) == Some("Contents") {
            let bundle_dir = contents_dir.parent()?;
            if is_macos_app_bundle(bundle_dir) {
                return Some(bundle_dir.to_path_buf());
            }
        }
    }

    None
}

pub fn backend_path(app_root: impl AsRef<Path>) -> PathBuf {
    let executable_name = if cfg!(windows) {
        "log73-backend.exe"
    } else {
        "log73-backend"
    };

    let app_root = app_root.as_ref();
    if is_macos_app_bundle(app_root) {
        app_root
            .join("Contents")
            .join("Helpers")
            .join(executable_name)
    } else {
        app_root.join("bin").join(executable_name)
    }
}

fn is_macos_app_bundle(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some("app")
}

fn fallback_config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| fallback_home_dir().join("AppData").join("Roaming"))
    }

    #[cfg(target_os = "macos")]
    {
        fallback_home_dir()
            .join("Library")
            .join("Application Support")
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| fallback_home_dir().join(".config"))
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        fallback_home_dir().join(".config")
    }
}

fn fallback_project_config_dir() -> PathBuf {
    let project_dir = fallback_config_dir().join(APP_NAME);

    #[cfg(target_os = "windows")]
    {
        project_dir.join("config")
    }

    #[cfg(not(target_os = "windows"))]
    {
        project_dir
    }
}

fn fallback_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| fallback_home_dir().join("AppData").join("Roaming"))
    }

    #[cfg(target_os = "macos")]
    {
        fallback_home_dir()
            .join("Library")
            .join("Application Support")
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| fallback_home_dir().join(".local").join("share"))
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        fallback_home_dir().join(".local").join("share")
    }
}

fn fallback_project_data_dir() -> PathBuf {
    let project_dir = fallback_data_dir().join(APP_NAME);

    #[cfg(target_os = "windows")]
    {
        project_dir.join("data")
    }

    #[cfg(not(target_os = "windows"))]
    {
        project_dir
    }
}

fn fallback_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_app_root() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("PROGRAMFILES")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
            .join("log73")
    }

    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Applications/log73-launcher.app")
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        PathBuf::from("/opt/log73")
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        PathBuf::from("/opt/log73")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_root_is_parent_of_bin_dir() {
        let executable = Path::new("app-root").join("bin").join("log73-backend");
        assert_eq!(
            app_root_from_executable(executable),
            Some(PathBuf::from("app-root"))
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_project_path_contains_app_name_once() {
        let dirs = project_dirs().expect("Windows project directories are available");

        assert_eq!(dirs.project_path(), Path::new(APP_NAME));
        assert!(
            dirs.config_dir()
                .ends_with(Path::new(APP_NAME).join("config"))
        );
        assert!(dirs.data_dir().ends_with(Path::new(APP_NAME).join("data")));
    }

    #[test]
    fn app_root_ignores_non_bin_layouts() {
        let executable = Path::new("workspace")
            .join("target")
            .join("debug")
            .join("log73-backend");
        assert_eq!(app_root_from_executable(executable), None);
    }

    #[test]
    fn app_root_is_macos_bundle_parent() {
        let executable = Path::new("log73-launcher.app")
            .join("Contents")
            .join("MacOS")
            .join("log73-launcher");
        assert_eq!(
            app_root_from_executable(executable),
            Some(PathBuf::from("log73-launcher.app"))
        );

        let helper = Path::new("log73-launcher.app")
            .join("Contents")
            .join("Helpers")
            .join("log73-backend");
        assert_eq!(
            app_root_from_executable(helper),
            Some(PathBuf::from("log73-launcher.app"))
        );
    }

    #[test]
    fn derived_paths_live_under_data_dir() {
        let data_dir = Path::new("log73-data");
        assert_eq!(
            contest_rules_dir(data_dir),
            Path::new("log73-data").join("contest-rules")
        );
        assert_eq!(
            database_path(data_dir),
            Path::new("log73-data").join("log73.db")
        );
        assert_eq!(
            launcher_log_file_path(data_dir),
            Path::new("log73-data").join("log73-launcher.log")
        );
        assert_eq!(
            radio_client_log_file_path(data_dir),
            Path::new("log73-data").join("log73-radio-client.log")
        );
        assert_eq!(
            installed_data_dir(Path::new("app-root")),
            Path::new("app-root").join("data")
        );

        let bundle_root = Path::new("log73-launcher.app");
        assert_eq!(
            installed_data_dir(bundle_root),
            bundle_root.join("Contents").join("Resources").join("data")
        );
        assert_eq!(
            backend_path(bundle_root),
            bundle_root
                .join("Contents")
                .join("Helpers")
                .join(if cfg!(windows) {
                    "log73-backend.exe"
                } else {
                    "log73-backend"
                })
        );
    }
}
