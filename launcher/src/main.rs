use iced::widget::image::Handle as ImageHandle;
use iced::widget::{
    Image, button, column, container, pick_list, row, scrollable, text, text_input,
};
use iced::{Element, Font, Length, Subscription, Task, Theme, application, font, window};
use serde::{Deserialize, Serialize};
use shared_child::SharedChild;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_PORT: &str = "7300";
const APP_WINDOW_SIZE: &str = "1200,800";
const STOP_TIMEOUT: Duration = Duration::from_secs(3);
const LAUNCHER_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../static/log73-icon-512.png"
));
const LAUNCHER_MAIN_ICON_SIZE: f32 = 256.0;
const LAUNCHER_TITLE_TEXT_SIZE: f32 = 40.0;
const LAUNCHER_WINDOW_WIDTH: f32 = 720.0;
const LAUNCHER_WINDOW_HEIGHT: f32 = 420.0;

#[cfg(all(unix, not(target_os = "macos")))]
const LINUX_CHROME_COMMANDS: &[&str] = &["google-chrome", "google-chrome-stable", "chrome"];
#[cfg(all(unix, not(target_os = "macos")))]
const LINUX_CHROMIUM_COMMANDS: &[&str] = &["chromium-browser", "chromium"];
#[cfg(all(unix, not(target_os = "macos")))]
const LINUX_EDGE_COMMANDS: &[&str] = &[
    "microsoft-edge",
    "microsoft-edge-stable",
    "microsoft-edge-beta",
];

fn main() -> iced::Result {
    application("log73 Launcher", update, view)
        .theme(|_| Theme::Light)
        .style(|_, _| iced::application::Appearance {
            background_color: iced::Color::WHITE,
            text_color: iced::Color::BLACK,
        })
        .window(window::Settings {
            size: iced::Size::new(LAUNCHER_WINDOW_WIDTH, LAUNCHER_WINDOW_HEIGHT),
            icon: launcher_window_icon(),
            ..window::Settings::default()
        })
        .subscription(subscription)
        .exit_on_close_request(false)
        .run_with(|| (Launcher::default(), Task::none()))
}

fn launcher_window_icon() -> Option<window::Icon> {
    match window::icon::from_file_data(LAUNCHER_ICON_PNG, None) {
        Ok(icon) => Some(icon),
        Err(error) => {
            eprintln!("log73-launcher: failed to decode launcher window icon: {error}");
            None
        }
    }
}

fn launcher_image_handle() -> ImageHandle {
    ImageHandle::from_bytes(LAUNCHER_ICON_PNG)
}

#[derive(Debug)]
struct Launcher {
    screen: Screen,
    main_icon: ImageHandle,
    settings: LauncherSettings,
    settings_dirty: bool,
    status: String,
    child: Option<Arc<SharedChild>>,
    active_token: Option<u64>,
    next_token: u64,
    stop_in_progress: bool,
    pending_close_window: Option<window::Id>,
}

impl Default for Launcher {
    fn default() -> Self {
        Self {
            screen: Screen::Main,
            main_icon: launcher_image_handle(),
            settings: load_settings_or_default(),
            settings_dirty: false,
            status: "Backend is stopped.".to_string(),
            child: None,
            active_token: None,
            next_token: 1,
            stop_in_progress: false,
            pending_close_window: None,
        }
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        if let Some(child) = &self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Main,
    Settings,
}

#[derive(Debug, Clone)]
enum Message {
    OpenSettingsPressed,
    BackToMainPressed,
    SetDefaultsPressed,
    BackendPathChanged(String),
    ConfigDirPathChanged(String),
    DataDirPathChanged(String),
    AppDirPathChanged(String),
    LogLevelSelected(LogLevel),
    LogFilePathChanged(String),
    BindModeSelected(BindMode),
    PortChanged(String),
    AppBrowserSelected(AppBrowser),
    StartPressed,
    StopPressed,
    OpenLogPressed,
    OpenInBrowserPressed,
    OpenInAppModePressed,
    OpenActionFinished(ActionOutcome),
    WindowCloseRequested(window::Id),
    BackendExited(ProcessExit),
    StopAttemptFinished(StopOutcome),
}

#[derive(Debug, Clone)]
struct ActionOutcome {
    note: String,
}

#[derive(Debug, Clone, Copy)]
enum OpenAction {
    LogFile,
    Browser,
    BrowserAppMode,
}

#[derive(Debug, Clone)]
struct ProcessExit {
    token: u64,
    result: ExitResult,
}

#[derive(Debug, Clone)]
enum ExitResult {
    Success,
    Failure(String),
    Error(String),
}

#[derive(Debug, Clone)]
struct StopOutcome {
    token: u64,
    note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct LauncherSettings {
    backend_path: String,
    config_dir: String,
    data_dir: String,
    app_dir: String,
    log_level: LogLevel,
    log_file_path: String,
    bind_mode: BindMode,
    port: String,
    app_browser: AppBrowser,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            backend_path: default_backend_path(),
            config_dir: default_config_dir_path(),
            data_dir: default_data_dir_path(),
            app_dir: default_app_dir_path(),
            log_level: LogLevel::Info,
            log_file_path: default_log_file_path(),
            bind_mode: BindMode::LocalhostOnly,
            port: DEFAULT_PORT.to_string(),
            app_browser: AppBrowser::default_for_os(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl LogLevel {
    const ALL: [LogLevel; 5] = [
        LogLevel::Trace,
        LogLevel::Debug,
        LogLevel::Info,
        LogLevel::Warn,
        LogLevel::Error,
    ];

    fn as_arg(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_arg())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BindMode {
    #[default]
    LocalhostOnly,
    Open,
}

impl BindMode {
    const ALL: [BindMode; 2] = [BindMode::LocalhostOnly, BindMode::Open];

    fn bind_ip(self) -> &'static str {
        match self {
            BindMode::LocalhostOnly => "127.0.0.1",
            BindMode::Open => "0.0.0.0",
        }
    }

    fn label(self) -> &'static str {
        match self {
            BindMode::LocalhostOnly => "localhost only",
            BindMode::Open => "open",
        }
    }
}

impl fmt::Display for BindMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AppBrowser {
    Chrome,
    Chromium,
    Edge,
}

impl AppBrowser {
    const ALL: [AppBrowser; 3] = [AppBrowser::Chrome, AppBrowser::Chromium, AppBrowser::Edge];

    fn default_for_os() -> Self {
        #[cfg(target_os = "linux")]
        {
            AppBrowser::Chromium
        }

        #[cfg(not(target_os = "linux"))]
        {
            AppBrowser::Chrome
        }
    }

    fn label(self) -> &'static str {
        match self {
            AppBrowser::Chrome => "chrome",
            AppBrowser::Chromium => "chromium",
            AppBrowser::Edge => "edge",
        }
    }

    fn profile_dir_name(self) -> &'static str {
        self.label()
    }

    #[cfg(target_os = "windows")]
    fn windows_start_target(self) -> &'static str {
        match self {
            AppBrowser::Chrome => "chrome",
            AppBrowser::Chromium => "chromium",
            AppBrowser::Edge => "msedge",
        }
    }

    #[cfg(target_os = "macos")]
    fn macos_app_name(self) -> &'static str {
        match self {
            AppBrowser::Chrome => "Google Chrome",
            AppBrowser::Chromium => "Chromium",
            AppBrowser::Edge => "Microsoft Edge",
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn linux_commands(self) -> &'static [&'static str] {
        match self {
            AppBrowser::Chrome => LINUX_CHROME_COMMANDS,
            AppBrowser::Chromium => LINUX_CHROMIUM_COMMANDS,
            AppBrowser::Edge => LINUX_EDGE_COMMANDS,
        }
    }
}

impl Default for AppBrowser {
    fn default() -> Self {
        AppBrowser::default_for_os()
    }
}

impl fmt::Display for AppBrowser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

fn subscription(_state: &Launcher) -> Subscription<Message> {
    window::close_requests().map(Message::WindowCloseRequested)
}

fn update(state: &mut Launcher, message: Message) -> Task<Message> {
    match message {
        Message::OpenSettingsPressed => {
            state.screen = Screen::Settings;
            Task::none()
        }
        Message::BackToMainPressed => {
            persist_settings_if_dirty(state);
            state.screen = Screen::Main;
            Task::none()
        }
        Message::SetDefaultsPressed => {
            state.settings = LauncherSettings::default();
            state.settings_dirty = true;
            state.status = "Settings reset to defaults.".to_string();
            Task::none()
        }
        Message::BackendPathChanged(path) => {
            state.settings.backend_path = path;
            state.settings_dirty = true;
            Task::none()
        }
        Message::ConfigDirPathChanged(path) => {
            state.settings.config_dir = path;
            state.settings_dirty = true;
            Task::none()
        }
        Message::DataDirPathChanged(path) => {
            state.settings.data_dir = path;
            state.settings_dirty = true;
            Task::none()
        }
        Message::AppDirPathChanged(path) => {
            state.settings.app_dir = path;
            state.settings_dirty = true;
            Task::none()
        }
        Message::LogLevelSelected(level) => {
            state.settings.log_level = level;
            state.settings_dirty = true;
            Task::none()
        }
        Message::LogFilePathChanged(path) => {
            state.settings.log_file_path = path;
            state.settings_dirty = true;
            Task::none()
        }
        Message::BindModeSelected(bind_mode) => {
            state.settings.bind_mode = bind_mode;
            state.settings_dirty = true;
            Task::none()
        }
        Message::PortChanged(port) => {
            state.settings.port = port;
            state.settings_dirty = true;
            Task::none()
        }
        Message::AppBrowserSelected(browser) => {
            state.settings.app_browser = browser;
            state.settings_dirty = true;
            Task::none()
        }
        Message::StartPressed => start_backend(state),
        Message::StopPressed => stop_backend(state),
        Message::OpenLogPressed => Task::perform(
            perform_open_action(state.settings.clone(), OpenAction::LogFile),
            Message::OpenActionFinished,
        ),
        Message::OpenInBrowserPressed => Task::perform(
            perform_open_action(state.settings.clone(), OpenAction::Browser),
            Message::OpenActionFinished,
        ),
        Message::OpenInAppModePressed => Task::perform(
            perform_open_action(state.settings.clone(), OpenAction::BrowserAppMode),
            Message::OpenActionFinished,
        ),
        Message::OpenActionFinished(outcome) => {
            state.status = outcome.note;
            Task::none()
        }
        Message::WindowCloseRequested(window_id) => {
            eprintln!("log73-launcher: window close requested");
            persist_settings_if_dirty(state);
            state.pending_close_window = Some(window_id);

            if state.child.is_some() || state.stop_in_progress {
                if state.child.is_some() && !state.stop_in_progress {
                    eprintln!(
                        "log73-launcher: backend running; stopping backend before closing launcher"
                    );
                    state.status = "Stopping backend before exit...".to_string();
                    return stop_backend(state);
                }

                eprintln!("log73-launcher: backend stop already in progress; waiting before close");
                Task::none()
            } else {
                eprintln!("log73-launcher: backend not running; closing launcher now");
                window::close(window_id)
            }
        }
        Message::BackendExited(event) => handle_backend_exit(state, event),
        Message::StopAttemptFinished(outcome) => handle_stop_outcome(state, outcome),
    }
}

fn start_backend(state: &mut Launcher) -> Task<Message> {
    if state.child.is_some() || state.stop_in_progress {
        return Task::none();
    }

    persist_settings_if_dirty(state);

    let backend_path = state.settings.backend_path.trim();
    if backend_path.is_empty() {
        state.status = "Backend path is required.".to_string();
        return Task::none();
    }

    let port = match parse_port(&state.settings.port) {
        Ok(port) => port,
        Err(error) => {
            state.status = error;
            return Task::none();
        }
    };

    let config_dir = PathBuf::from(state.settings.config_dir.trim());
    if config_dir.as_os_str().is_empty() {
        state.status = "Config directory is required.".to_string();
        return Task::none();
    }

    let data_dir = PathBuf::from(state.settings.data_dir.trim());
    if data_dir.as_os_str().is_empty() {
        state.status = "Data directory is required.".to_string();
        return Task::none();
    }

    let app_dir = PathBuf::from(state.settings.app_dir.trim());
    if app_dir.as_os_str().is_empty() {
        state.status = "App directory is required.".to_string();
        return Task::none();
    }

    if let Err(error) = fs::create_dir_all(&config_dir) {
        state.status = format!("Failed to create config directory: {error}");
        return Task::none();
    }

    if let Err(error) = fs::create_dir_all(&data_dir) {
        state.status = format!("Failed to create data directory: {error}");
        return Task::none();
    }

    let bind_address = format!("{}:{port}", state.settings.bind_mode.bind_ip());
    let backend_binary_path = PathBuf::from(backend_path);

    let mut command = Command::new(&backend_binary_path);
    command
        .arg("--bind")
        .arg(&bind_address)
        .arg("--config-dir")
        .arg(&config_dir)
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("--app-dir")
        .arg(&app_dir)
        .arg("--log-level")
        .arg(state.settings.log_level.as_arg())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let log_file_path = state.settings.log_file_path.trim();
    if !log_file_path.is_empty() {
        command.arg("--log-file").arg(log_file_path);
    }

    eprintln!(
        "log73-launcher: starting backend path={} bind={} config_dir={} data_dir={} app_dir={} log_level={} log_file={}",
        backend_binary_path.display(),
        bind_address,
        config_dir.display(),
        data_dir.display(),
        app_dir.display(),
        state.settings.log_level,
        if log_file_path.is_empty() {
            "<stdout only>"
        } else {
            log_file_path
        }
    );

    match SharedChild::spawn(&mut command) {
        Ok(child) => {
            let child = Arc::new(child);
            let token = state.next_token;
            state.next_token = state.next_token.saturating_add(1);
            state.active_token = Some(token);
            state.child = Some(child.clone());
            state.stop_in_progress = false;
            state.status = format!("Backend is running on {bind_address}.");
            Task::perform(wait_for_exit(child, token), Message::BackendExited)
        }
        Err(error) => {
            state.status = format!("Failed to start backend: {error}");
            Task::none()
        }
    }
}

fn stop_backend(state: &mut Launcher) -> Task<Message> {
    if state.stop_in_progress {
        return Task::none();
    }

    let (Some(child), Some(token)) = (state.child.as_ref(), state.active_token) else {
        return Task::none();
    };

    state.stop_in_progress = true;
    state.status = "Stopping backend...".to_string();
    Task::perform(
        attempt_stop(child.clone(), token),
        Message::StopAttemptFinished,
    )
}

fn handle_backend_exit(state: &mut Launcher, event: ProcessExit) -> Task<Message> {
    if state.active_token != Some(event.token) {
        return Task::none();
    }

    state.child = None;
    state.active_token = None;
    state.stop_in_progress = false;

    state.status = match event.result {
        ExitResult::Success => "Backend exited normally.".to_string(),
        ExitResult::Failure(exit_status) => {
            format!("Backend exited with status {exit_status}. See launcher console for details.")
        }
        ExitResult::Error(error) => format!("Failed to read backend exit status: {error}"),
    };

    if let Some(window_id) = state.pending_close_window.take() {
        eprintln!("log73-launcher: backend stopped; closing launcher window");
        return window::close(window_id);
    }

    Task::none()
}

fn handle_stop_outcome(state: &mut Launcher, outcome: StopOutcome) -> Task<Message> {
    if state.active_token != Some(outcome.token) {
        return Task::none();
    }

    state.status = outcome.note;
    Task::none()
}

async fn perform_open_action(settings: LauncherSettings, action: OpenAction) -> ActionOutcome {
    let result = match action {
        OpenAction::LogFile => open_log_file(&settings),
        OpenAction::Browser => open_default_browser(&settings),
        OpenAction::BrowserAppMode => open_browser_app_mode(&settings),
    };

    match result {
        Ok(note) => ActionOutcome { note },
        Err(error) => ActionOutcome {
            note: format!("Action failed: {error}"),
        },
    }
}

fn open_log_file(settings: &LauncherSettings) -> Result<String, String> {
    let log_file_path = settings.log_file_path.trim();
    if log_file_path.is_empty() {
        return Err("log file path is empty".to_string());
    }

    let log_file = PathBuf::from(log_file_path);
    if !log_file.exists() {
        return Err(format!("log file does not exist: {}", log_file.display()));
    }

    eprintln!("log73-launcher: opening log file {}", log_file.display());
    open::that(&log_file).map_err(|error| error.to_string())?;
    Ok(format!("Opened log file: {}", log_file.display()))
}

fn open_default_browser(settings: &LauncherSettings) -> Result<String, String> {
    let url = app_url(settings)?;
    eprintln!("log73-launcher: opening default browser for {url}");
    open::that(&url).map_err(|error| error.to_string())?;
    Ok(format!("Opened app in default browser: {url}"))
}

fn open_browser_app_mode(settings: &LauncherSettings) -> Result<String, String> {
    let url = app_url(settings)?;

    eprintln!(
        "log73-launcher: opening {} app mode for {}",
        settings.app_browser, url
    );

    let _ = launch_app_mode(settings.app_browser, &url)?;
    Ok(format!("Opened app mode in {}.", settings.app_browser))
}

fn launch_app_mode(browser: AppBrowser, url: &str) -> Result<PathBuf, String> {
    let app_arg = format!("--app={url}");
    let size_arg = format!("--window-size={APP_WINDOW_SIZE}");
    let new_window_arg = "--new-window";

    #[cfg(target_os = "windows")]
    {
        let profile_dir = browser_user_data_dir(browser);
        fs::create_dir_all(&profile_dir).map_err(|error| error.to_string())?;
        let user_data_arg = format!("--user-data-dir={}", profile_dir.to_string_lossy());

        let status = Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(browser.windows_start_target())
            .arg(new_window_arg)
            .arg(&app_arg)
            .arg(&size_arg)
            .arg(&user_data_arg)
            .status()
            .map_err(|error| format!("failed to launch app mode on windows: {error}"))?;

        if !status.success() {
            return Err(format!("windows start command exited with status {status}"));
        }

        return Ok(profile_dir);
    }

    #[cfg(target_os = "macos")]
    {
        let profile_dir = browser_user_data_dir(browser);
        fs::create_dir_all(&profile_dir).map_err(|error| error.to_string())?;
        let user_data_arg = format!("--user-data-dir={}", profile_dir.to_string_lossy());

        let status = Command::new("open")
            .arg("-a")
            .arg(browser.macos_app_name())
            .arg("--args")
            .arg(new_window_arg)
            .arg(&app_arg)
            .arg(&size_arg)
            .arg(&user_data_arg)
            .status()
            .map_err(|error| format!("failed to launch app mode on macos: {error}"))?;

        if !status.success() {
            return Err(format!("macos open command exited with status {status}"));
        }

        return Ok(profile_dir);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let commands = browser.linux_commands();
        let mut errors = Vec::new();

        for command_name in commands {
            let profile_dir = linux_user_data_dir_for_command(browser, command_name);
            if let Err(error) = fs::create_dir_all(&profile_dir) {
                let detail = format!("{} profile dir create failed: {}", command_name, error);
                eprintln!("log73-launcher: {detail}");
                errors.push(detail);
                continue;
            }

            let user_data_arg = format!("--user-data-dir={}", profile_dir.to_string_lossy());
            eprintln!(
                "log73-launcher: trying linux app-mode command={} {} {} {}",
                command_name, new_window_arg, app_arg, size_arg
            );

            let spawn_result = Command::new(command_name)
                .arg(new_window_arg)
                .arg(&app_arg)
                .arg(&size_arg)
                .arg(&user_data_arg)
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn();

            match spawn_result {
                Ok(mut child) => {
                    std::thread::sleep(Duration::from_millis(250));
                    match child.try_wait() {
                        Ok(Some(status)) if !status.success() => {
                            let detail = format!(
                                "{} exited immediately with status {}",
                                command_name, status
                            );
                            eprintln!("log73-launcher: {detail}");
                            errors.push(detail);
                        }
                        Ok(Some(status)) => {
                            eprintln!(
                                "log73-launcher: {} launched and exited quickly with status {}",
                                command_name, status
                            );
                            return Ok(profile_dir);
                        }
                        Ok(None) => {
                            eprintln!(
                                "log73-launcher: launched app mode with {} (pid={})",
                                command_name,
                                child.id()
                            );
                            return Ok(profile_dir);
                        }
                        Err(error) => {
                            let detail = format!(
                                "{} launched but status check failed: {}",
                                command_name, error
                            );
                            eprintln!("log73-launcher: {detail}");
                            errors.push(detail);
                        }
                    }
                }
                Err(error) => {
                    let detail = format!("{} spawn failed: {}", command_name, error);
                    eprintln!("log73-launcher: {detail}");
                    errors.push(detail);
                }
            }
        }

        return Err(format!(
            "failed to launch {} app mode on linux; tried [{}]; errors: {}",
            browser,
            commands.join(", "),
            errors.join(" | ")
        ));
    }

    #[allow(unreachable_code)]
    Err("app mode launch is not supported on this platform".to_string())
}

fn app_url(settings: &LauncherSettings) -> Result<String, String> {
    let port = parse_port(&settings.port)?;
    Ok(format!("http://127.0.0.1:{port}"))
}

async fn wait_for_exit(child: Arc<SharedChild>, token: u64) -> ProcessExit {
    let result = tokio::task::spawn_blocking(move || child.wait()).await;

    let result = match result {
        Ok(Ok(exit_status)) if exit_status.success() => ExitResult::Success,
        Ok(Ok(exit_status)) => ExitResult::Failure(exit_status.to_string()),
        Ok(Err(error)) => ExitResult::Error(error.to_string()),
        Err(error) => ExitResult::Error(format!("wait task failed: {error}")),
    };

    ProcessExit { token, result }
}

async fn attempt_stop(child: Arc<SharedChild>, token: u64) -> StopOutcome {
    let graceful_note = match request_graceful_stop(&child) {
        Ok(()) => "Sent graceful stop signal".to_string(),
        Err(error) => format!("Graceful stop unavailable ({error})"),
    };

    let deadline = Instant::now() + STOP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return StopOutcome {
                    token,
                    note: format!("{graceful_note}; backend stopped."),
                };
            }
            Ok(None) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Ok(None) => break,
            Err(error) => {
                return StopOutcome {
                    token,
                    note: format!("Failed while checking backend status during stop: {error}"),
                };
            }
        }
    }

    match child.kill() {
        Ok(()) => StopOutcome {
            token,
            note: format!("{graceful_note}; backend did not stop in time, sent force stop signal."),
        },
        Err(error) => StopOutcome {
            token,
            note: format!(
                "{graceful_note}; backend did not stop in time and force stop failed: {error}"
            ),
        },
    }
}

#[cfg(unix)]
fn request_graceful_stop(child: &SharedChild) -> Result<(), String> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let pid = Pid::from_raw(child.id() as i32);
    kill(pid, Signal::SIGTERM).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn request_graceful_stop(_child: &SharedChild) -> Result<(), String> {
    Err("not supported on this platform".to_string())
}

fn parse_port(port_text: &str) -> Result<u16, String> {
    let trimmed = port_text.trim();
    if trimmed.is_empty() {
        return Err("Port is required.".to_string());
    }

    let port = trimmed
        .parse::<u16>()
        .map_err(|_| "Port must be a number between 1 and 65535.".to_string())?;

    if port == 0 {
        return Err("Port must be between 1 and 65535.".to_string());
    }

    Ok(port)
}

fn view(state: &Launcher) -> Element<'_, Message> {
    match state.screen {
        Screen::Main => view_main(state),
        Screen::Settings => view_settings(state),
    }
}

fn view_main(state: &Launcher) -> Element<'_, Message> {
    let running = state.child.is_some();
    let can_start = !running && !state.stop_in_progress;
    let can_stop = running && !state.stop_in_progress;
    let can_open_app = running && !state.stop_in_progress;

    let settings_button = button("Settings").on_press(Message::OpenSettingsPressed);

    let start_button = if can_start {
        button("Start").on_press(Message::StartPressed)
    } else {
        button("Start")
    };

    let stop_button = if can_stop {
        button("Stop").on_press(Message::StopPressed)
    } else {
        button("Stop")
    };

    let open_log_button = button("Open log").on_press(Message::OpenLogPressed);

    let open_browser_button = if can_open_app {
        button("Open In Browser").on_press(Message::OpenInBrowserPressed)
    } else {
        button("Open In Browser")
    };

    let open_app_mode_button = if can_open_app {
        button("Open as App").on_press(Message::OpenInAppModePressed)
    } else {
        button("Open as App")
    };

    let backend_state = if running {
        "running"
    } else if state.stop_in_progress {
        "stopping"
    } else {
        "stopped"
    };

    let header_icon = Image::new(state.main_icon.clone())
        .width(Length::Fixed(LAUNCHER_MAIN_ICON_SIZE))
        .height(Length::Fixed(LAUNCHER_MAIN_ICON_SIZE));

    let controls = column![
        text("log73 Launcher")
            .size(LAUNCHER_TITLE_TEXT_SIZE)
            .font(Font {
                weight: font::Weight::Bold,
                ..Font::DEFAULT
            }),
        settings_button,
        row![
            start_button,
            stop_button,
            text(format!("Backend status: {backend_state}"))
        ]
        .spacing(12)
        .align_y(iced::alignment::Vertical::Center),
        column![
            row![open_log_button, open_browser_button, open_app_mode_button].spacing(12),
            text(&state.status)
                .width(Length::Fill)
                .wrapping(iced::widget::text::Wrapping::Word),
        ]
        .spacing(8)
        .width(Length::Shrink),
    ]
    .spacing(12)
    .max_width(900);

    let content = row![header_icon, controls]
        .spacing(16)
        .align_y(iced::alignment::Vertical::Top)
        .padding(16);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn view_settings(state: &Launcher) -> Element<'_, Message> {
    let backend_path_input = text_input("Path to log73-backend", &state.settings.backend_path)
        .on_input(Message::BackendPathChanged)
        .width(Length::Fill);

    let config_dir_input = text_input("Config directory", &state.settings.config_dir)
        .on_input(Message::ConfigDirPathChanged)
        .width(Length::Fill);

    let data_dir_input = text_input("Data directory", &state.settings.data_dir)
        .on_input(Message::DataDirPathChanged)
        .width(Length::Fill);

    let app_dir_input = text_input("App directory", &state.settings.app_dir)
        .on_input(Message::AppDirPathChanged)
        .width(Length::Fill);

    let log_level_pick_list = pick_list(
        &LogLevel::ALL[..],
        Some(state.settings.log_level),
        Message::LogLevelSelected,
    )
    .width(Length::FillPortion(1));

    let bind_mode_pick_list = pick_list(
        &BindMode::ALL[..],
        Some(state.settings.bind_mode),
        Message::BindModeSelected,
    )
    .width(Length::FillPortion(1));

    let port_input = text_input("7300", &state.settings.port)
        .on_input(Message::PortChanged)
        .width(Length::Fixed(140.0));

    let log_file_input = text_input("Log file path", &state.settings.log_file_path)
        .on_input(Message::LogFilePathChanged)
        .width(Length::Fill);

    let app_browser_pick_list = pick_list(
        &AppBrowser::ALL[..],
        Some(state.settings.app_browser),
        Message::AppBrowserSelected,
    )
    .width(Length::FillPortion(1));

    let profile_dir = effective_browser_user_data_dir(state.settings.app_browser);

    let defaults_button = button("Set defaults").on_press(Message::SetDefaultsPressed);
    let back_button = button("Back").on_press(Message::BackToMainPressed);

    let content = column![
        text("Settings"),
        text("Backend binary path"),
        backend_path_input,
        text("Config directory"),
        config_dir_input,
        text("Data directory"),
        data_dir_input,
        text("App directory"),
        app_dir_input,
        row![text("Bind"), bind_mode_pick_list, text("Port"), port_input].spacing(12),
        row![text("Log level"), log_level_pick_list].spacing(12),
        text("Log file path"),
        log_file_input,
        row![text("App mode browser"), app_browser_pick_list].spacing(12),
        text(format!(
            "App mode user data dir: {}",
            profile_dir.to_string_lossy()
        )),
        row![defaults_button, back_button].spacing(12),
        text(&state.status),
    ]
    .spacing(12)
    .padding(16)
    .max_width(900);

    container(scrollable(content).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .into()
}

fn default_backend_path() -> String {
    let executable_name = if cfg!(windows) {
        "log73-backend.exe"
    } else {
        "log73-backend"
    };

    let mut candidates = vec![log73_paths::backend_path(log73_paths::app_root())];

    if let Ok(current_executable) = std::env::current_exe()
        && let Some(executable_dir) = current_executable.parent()
    {
        candidates.push(executable_dir.join(executable_name));
        candidates.push(executable_dir.join("..").join(executable_name));
    }

    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(
            current_dir
                .join("target")
                .join("debug")
                .join(executable_name),
        );
        candidates.push(current_dir.join(executable_name));
    }

    choose_existing_or_first(candidates)
}

fn default_config_dir_path() -> String {
    log73_paths::config_dir().to_string_lossy().into_owned()
}

fn default_data_dir_path() -> String {
    log73_paths::data_dir().to_string_lossy().into_owned()
}

fn default_app_dir_path() -> String {
    log73_paths::app_root().to_string_lossy().into_owned()
}

fn default_log_file_path() -> String {
    log73_paths::log_file_path(log73_paths::data_dir())
        .to_string_lossy()
        .into_owned()
}

fn choose_existing_or_first(candidates: Vec<PathBuf>) -> String {
    if let Some(existing) = candidates.iter().find(|path| path.exists()) {
        return existing.to_string_lossy().into_owned();
    }

    if let Some(first) = candidates.first() {
        return first.to_string_lossy().into_owned();
    }

    if cfg!(windows) {
        "log73-backend.exe".to_string()
    } else {
        "log73-backend".to_string()
    }
}

fn launcher_config_dir() -> PathBuf {
    log73_paths::config_dir()
}

fn settings_file_path() -> PathBuf {
    launcher_config_dir().join("launcher-settings.toml")
}

fn browser_user_data_dir(browser: AppBrowser) -> PathBuf {
    launcher_config_dir().join(browser.profile_dir_name())
}

fn effective_browser_user_data_dir(browser: AppBrowser) -> PathBuf {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(profile_dir) = detect_snap_profile_dir_for_browser(browser) {
            return profile_dir;
        }
    }

    browser_user_data_dir(browser)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_user_data_dir_for_command(browser: AppBrowser, command_name: &str) -> PathBuf {
    if let Some(snap_package) = snap_package_name_for_command(command_name) {
        return snap_browser_user_data_dir(&snap_package, browser);
    }

    browser_user_data_dir(browser)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn detect_snap_profile_dir_for_browser(browser: AppBrowser) -> Option<PathBuf> {
    browser
        .linux_commands()
        .iter()
        .find_map(|command_name| snap_package_name_for_command(command_name))
        .map(|snap_package| snap_browser_user_data_dir(&snap_package, browser))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn snap_browser_user_data_dir(snap_package: &str, browser: AppBrowser) -> PathBuf {
    if let Some(home_dir) = std::env::var_os("HOME").map(PathBuf::from) {
        return home_dir
            .join("snap")
            .join(snap_package)
            .join("common")
            .join(format!("log73-profile-{}", browser.label()));
    }

    browser_user_data_dir(browser)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn snap_package_name_for_command(command_name: &str) -> Option<String> {
    let resolved = resolve_command_in_path(command_name)?;

    if resolved.starts_with(Path::new("/snap/bin")) {
        return resolved
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
    }

    if command_name == "chromium-browser"
        && let Ok(wrapper_contents) = fs::read_to_string(&resolved)
        && (wrapper_contents.contains("/snap/bin/chromium")
            || wrapper_contents.contains("snap run chromium"))
    {
        return Some("chromium".to_string());
    }

    None
}

#[cfg(all(unix, not(target_os = "macos")))]
fn resolve_command_in_path(command_name: &str) -> Option<PathBuf> {
    if command_name.contains('/') {
        let path = PathBuf::from(command_name);
        return path.is_file().then_some(path);
    }

    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|directory| directory.join(command_name))
        .find(|candidate| candidate.is_file())
}

fn load_settings_or_default() -> LauncherSettings {
    load_settings_or_default_from_path(&settings_file_path())
}

fn load_settings_or_default_from_path(path: &Path) -> LauncherSettings {
    let settings = LauncherSettings::default();

    let Ok(contents) = fs::read_to_string(path) else {
        return settings;
    };

    match toml::from_str::<LauncherSettings>(&contents) {
        Ok(mut loaded) => {
            if loaded.backend_path.trim().is_empty() {
                loaded.backend_path = default_backend_path();
            }
            if loaded.config_dir.trim().is_empty() {
                loaded.config_dir = default_config_dir_path();
            }
            if loaded.data_dir.trim().is_empty() {
                loaded.data_dir = default_data_dir_path();
            }
            if loaded.app_dir.trim().is_empty() {
                loaded.app_dir = default_app_dir_path();
            }
            if loaded.log_file_path.trim().is_empty() {
                loaded.log_file_path = default_log_file_path();
            }
            if loaded.port.trim().is_empty() {
                loaded.port = DEFAULT_PORT.to_string();
            }
            loaded
        }
        Err(error) => {
            eprintln!("failed to parse launcher settings: {error}");
            settings
        }
    }
}

fn persist_settings_if_dirty(state: &mut Launcher) {
    persist_settings_if_dirty_to_path(state, &settings_file_path())
}

fn persist_settings_if_dirty_to_path(state: &mut Launcher, path: &Path) {
    if !state.settings_dirty {
        return;
    }

    match save_settings_to_path(&state.settings, path) {
        Ok(()) => state.settings_dirty = false,
        Err(error) => eprintln!("failed to save launcher settings: {error}"),
    }
}

fn save_settings_to_path(settings: &LauncherSettings, path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Err("settings path has no parent directory".to_string());
    };

    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let serialized = toml::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(path, serialized).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_launcher() -> Launcher {
        Launcher {
            screen: Screen::Settings,
            main_icon: launcher_image_handle(),
            settings: LauncherSettings::default(),
            settings_dirty: false,
            status: String::new(),
            child: None,
            active_token: None,
            next_token: 1,
            stop_in_progress: false,
            pending_close_window: None,
        }
    }

    fn temp_settings_path(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "log73-launcher-tests-{}-{}",
            std::process::id(),
            name
        ));
        let _ = fs::remove_dir_all(&base);
        base.join("launcher-settings.toml")
    }

    #[test]
    fn settings_edits_only_mark_state_dirty() {
        let mut launcher = test_launcher();

        let _ = update(
            &mut launcher,
            Message::BackendPathChanged("/tmp/custom-backend".to_string()),
        );
        let _ = update(&mut launcher, Message::PortChanged("7301".to_string()));

        assert_eq!(launcher.settings.backend_path, "/tmp/custom-backend");
        assert_eq!(launcher.settings.port, "7301");
        assert!(launcher.settings_dirty);
    }

    #[test]
    fn persist_settings_if_dirty_to_path_skips_clean_state_and_saves_dirty_state() {
        let path = temp_settings_path("dirty-save");
        let mut launcher = test_launcher();

        persist_settings_if_dirty_to_path(&mut launcher, &path);
        assert!(!path.exists());
        assert!(!launcher.settings_dirty);

        launcher.settings.backend_path = "/tmp/custom-backend".to_string();
        launcher.settings.port = "7400".to_string();
        launcher.settings_dirty = true;
        persist_settings_if_dirty_to_path(&mut launcher, &path);

        assert!(path.exists());
        assert!(!launcher.settings_dirty);
        let loaded = load_settings_or_default_from_path(&path);
        assert_eq!(loaded.backend_path, "/tmp/custom-backend");
        assert_eq!(loaded.port, "7400");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_and_load_settings_round_trip_custom_values() {
        let path = temp_settings_path("round-trip");
        let settings = LauncherSettings {
            backend_path: "/tmp/backend".to_string(),
            config_dir: "/tmp/config".to_string(),
            data_dir: "/tmp/data".to_string(),
            app_dir: "/tmp/app".to_string(),
            log_level: LogLevel::Debug,
            log_file_path: "/tmp/log73.log".to_string(),
            bind_mode: BindMode::Open,
            port: "8123".to_string(),
            app_browser: AppBrowser::Edge,
        };

        save_settings_to_path(&settings, &path).expect("settings save succeeds");
        let loaded = load_settings_or_default_from_path(&path);

        assert_eq!(loaded.backend_path, settings.backend_path);
        assert_eq!(loaded.config_dir, settings.config_dir);
        assert_eq!(loaded.data_dir, settings.data_dir);
        assert_eq!(loaded.app_dir, settings.app_dir);
        assert_eq!(loaded.log_level, settings.log_level);
        assert_eq!(loaded.log_file_path, settings.log_file_path);
        assert_eq!(loaded.bind_mode, settings.bind_mode);
        assert_eq!(loaded.port, settings.port);
        assert_eq!(loaded.app_browser, settings.app_browser);

        let _ = fs::remove_file(path);
    }
}
