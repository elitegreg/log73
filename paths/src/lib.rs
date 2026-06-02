use std::path::{Path, PathBuf};

const APP_NAME: &str = "log73";

pub fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "log73", APP_NAME)
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| fallback_config_dir().join(APP_NAME))
}

pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "log73", APP_NAME)
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| fallback_data_dir().join(APP_NAME))
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

pub fn app_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| app_root_from_executable(&path))
        .unwrap_or_else(default_app_root)
}

pub fn app_root_from_executable(executable: impl AsRef<Path>) -> Option<PathBuf> {
    let bin_dir = executable.as_ref().parent()?;
    if bin_dir.file_name().and_then(|name| name.to_str()) == Some("bin") {
        return bin_dir.parent().map(Path::to_path_buf);
    }

    None
}

pub fn backend_path(app_root: impl AsRef<Path>) -> PathBuf {
    let executable_name = if cfg!(windows) {
        "log73-backend.exe"
    } else {
        "log73-backend"
    };

    app_root.as_ref().join("bin").join(executable_name)
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
        PathBuf::from("/Applications/log73")
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

    #[test]
    fn app_root_ignores_non_bin_layouts() {
        let executable = Path::new("workspace")
            .join("target")
            .join("debug")
            .join("log73-backend");
        assert_eq!(app_root_from_executable(executable), None);
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
    }
}
