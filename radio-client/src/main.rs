mod backend_client;
mod configure;
mod lifecycle;
mod settings;

use clap::Parser;
use iced::widget::image::Handle as ImageHandle;
use iced::widget::{Image, button, checkbox, column, container, row, scrollable, text};
use iced::{Element, Length, Subscription, Task, Theme, application, window};
use settings::{RadioClientSettings, load_or_create, save_atomic, settings_file_path};
use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*, reload};

const RADIO_CLIENT_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../static/log73-icon-512.png"
));
const WINDOW_WIDTH: f32 = 640.0;
const EVENT_LOG_CAPACITY: usize = 1_000;

#[derive(Debug, Parser)]
#[command(version, about = "Log73 client-side radio controller")]
struct Cli {
    #[arg(long)]
    log_level: Option<String>,

    #[arg(long)]
    log_file: Option<PathBuf>,

    #[arg(long)]
    config_dir: Option<PathBuf>,

    #[arg(long)]
    data_dir: Option<PathBuf>,

    #[arg(long)]
    app_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppPaths {
    config_dir: PathBuf,
    data_dir: PathBuf,
    app_dir: PathBuf,
    settings_file: PathBuf,
    voicekeyer_dir: PathBuf,
    log_file: PathBuf,
}

impl AppPaths {
    fn resolve(cli: &Cli) -> Self {
        let config_dir = cli
            .config_dir
            .clone()
            .unwrap_or_else(log73_paths::config_dir);
        let data_dir = cli.data_dir.clone().unwrap_or_else(log73_paths::data_dir);
        let app_dir = cli.app_dir.clone().unwrap_or_else(log73_paths::app_root);
        let log_file = cli
            .log_file
            .clone()
            .unwrap_or_else(|| log73_paths::radio_client_log_file_path(&data_dir));
        Self {
            settings_file: settings_file_path(&config_dir),
            voicekeyer_dir: data_dir.join("voicekeyer"),
            config_dir,
            data_dir,
            app_dir,
            log_file,
        }
    }

    fn ensure_directories(&self) -> Result<(), String> {
        for directory in [&self.config_dir, &self.data_dir, &self.voicekeyer_dir] {
            fs::create_dir_all(directory).map_err(|error| {
                format!(
                    "failed to create directory {}: {error}",
                    directory.display()
                )
            })?;
        }
        if let Some(parent) = self
            .log_file
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create log directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        Ok(())
    }
}

fn main() -> iced::Result {
    let cli = Cli::parse();
    let paths = AppPaths::resolve(&cli);
    if let Err(error) = paths.ensure_directories() {
        return startup_failure(error);
    }
    let settings = match load_or_create(&paths.settings_file) {
        Ok(settings) => settings,
        Err(error) => return startup_failure(error),
    };
    let log_level = effective_log_level(cli.log_level.as_deref(), settings.debug_logging_enabled);
    let (_log_guard, log_filter) = match init_tracing(&log_level, &paths.log_file) {
        Ok(logging) => logging,
        Err(error) => return startup_failure(format!("failed to initialize logging: {error}")),
    };

    info!(
        config_dir = %paths.config_dir.display(),
        data_dir = %paths.data_dir.display(),
        app_dir = %paths.app_dir.display(),
        settings = %settings.diagnostic_summary(),
        "starting Log73 Radio Client"
    );

    application("Log73 Radio Client", update, view)
        .theme(|_| Theme::Light)
        .style(|_, _| iced::application::Appearance {
            background_color: iced::Color::WHITE,
            text_color: iced::Color::BLACK,
        })
        .window(window::Settings {
            size: iced::Size::new(WINDOW_WIDTH, 680.0),
            icon: radio_client_window_icon(),
            ..window::Settings::default()
        })
        .subscription(subscription)
        .exit_on_close_request(false)
        .run_with(move || {
            (
                RadioClient::new(settings, paths, Some(log_filter), cli.log_level),
                Task::none(),
            )
        })
}

fn startup_failure(error: impl std::fmt::Display) -> iced::Result {
    eprintln!("log73-radio-client: {error}");
    Err(iced::Error::WindowCreationFailed(Box::new(
        std::io::Error::other(error.to_string()),
    )))
}

fn init_tracing(
    log_level: &str,
    log_file: &Path,
) -> std::io::Result<(tracing_appender::non_blocking::WorkerGuard, LogFilterHandle)> {
    let filter = EnvFilter::try_new(log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)?;
    let (writer, guard) = tracing_appender::non_blocking(file);
    let (filter_layer, filter_handle) = reload::Layer::new(filter);
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt::layer())
        .with(fmt::layer().with_writer(writer).with_ansi(false))
        .init();
    Ok((guard, filter_handle))
}

type LogFilterHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;

fn effective_log_level(log_level_override: Option<&str>, debug_logging_enabled: bool) -> String {
    log_level_override
        .unwrap_or(if debug_logging_enabled {
            "debug"
        } else {
            "info"
        })
        .to_string()
}

fn radio_client_window_icon() -> Option<window::Icon> {
    match window::icon::from_file_data(RADIO_CLIENT_ICON_PNG, None) {
        Ok(icon) => Some(icon),
        Err(error) => {
            eprintln!("log73-radio-client: failed to decode window icon: {error}");
            None
        }
    }
}

struct RadioClient {
    settings: RadioClientSettings,
    paths: AppPaths,
    icon: ImageHandle,
    configure: Option<configure::ConfigureScreen>,
    lifecycle: LifecycleState,
    host: Option<lifecycle::RunningHost>,
    backend_status: Option<backend_client::BackendStatus>,
    events: VecDeque<EventEntry>,
    log_filter: Option<LogFilterHandle>,
    log_level_override: Option<String>,
    pending_close_window: Option<window::Id>,
}

impl RadioClient {
    fn new(
        settings: RadioClientSettings,
        paths: AppPaths,
        log_filter: Option<LogFilterHandle>,
        log_level_override: Option<String>,
    ) -> Self {
        let lifecycle = LifecycleState::initial(&settings);
        Self {
            settings,
            paths,
            icon: ImageHandle::from_bytes(RADIO_CLIENT_ICON_PNG),
            configure: None,
            lifecycle,
            host: None,
            backend_status: None,
            events: VecDeque::from([EventEntry::new("Radio Client is ready.")]),
            log_filter,
            log_level_override,
            pending_close_window: None,
        }
    }
}

enum Message {
    ConfigurePressed,
    Configure(configure::Message),
    StartPressed,
    StopPressed,
    DebugLoggingChanged(bool),
    StartFinished(std::sync::Arc<std::sync::Mutex<Option<Result<lifecycle::StartResult, String>>>>),
    StopFinished(Vec<String>),
    BackendTick,
    WindowCloseRequested(window::Id),
}

impl Clone for Message {
    fn clone(&self) -> Self {
        match self {
            Self::ConfigurePressed => Self::ConfigurePressed,
            Self::Configure(message) => Self::Configure(message.clone()),
            Self::StartPressed => Self::StartPressed,
            Self::StopPressed => Self::StopPressed,
            Self::DebugLoggingChanged(enabled) => Self::DebugLoggingChanged(*enabled),
            Self::StartFinished(result) => Self::StartFinished(result.clone()),
            Self::StopFinished(events) => Self::StopFinished(events.clone()),
            Self::BackendTick => Self::BackendTick,
            Self::WindowCloseRequested(window_id) => Self::WindowCloseRequested(*window_id),
        }
    }
}

impl std::fmt::Debug for Message {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigurePressed => formatter.write_str("ConfigurePressed"),
            Self::Configure(message) => formatter.debug_tuple("Configure").field(message).finish(),
            Self::StartPressed => formatter.write_str("StartPressed"),
            Self::StopPressed => formatter.write_str("StopPressed"),
            Self::DebugLoggingChanged(enabled) => formatter
                .debug_tuple("DebugLoggingChanged")
                .field(enabled)
                .finish(),
            Self::StartFinished(_) => formatter.write_str("StartFinished"),
            Self::StopFinished(events) => {
                formatter.debug_tuple("StopFinished").field(events).finish()
            }
            Self::BackendTick => formatter.write_str("BackendTick"),
            Self::WindowCloseRequested(window_id) => formatter
                .debug_tuple("WindowCloseRequested")
                .field(window_id)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LifecycleState {
    NotConfigured,
    Stopped,
    Starting,
    RunningLocal,
    Stopping,
    Error,
}

impl LifecycleState {
    fn initial(settings: &RadioClientSettings) -> Self {
        if settings.is_radio_configured() {
            Self::Stopped
        } else {
            Self::NotConfigured
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::NotConfigured => "Not configured",
            Self::Stopped => "Stopped",
            Self::Starting => "Starting…",
            Self::RunningLocal => "Running locally",
            Self::Stopping => "Stopping…",
            Self::Error => "Error",
        }
    }
}

struct EventEntry {
    timestamp: u64,
    message: String,
}

impl EventEntry {
    fn new(message: impl Into<String>) -> Self {
        Self {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            message: message.into(),
        }
    }
}

fn subscription(_state: &RadioClient) -> Subscription<Message> {
    Subscription::batch([
        window::close_requests().map(Message::WindowCloseRequested),
        iced::time::every(std::time::Duration::from_millis(250)).map(|_| Message::BackendTick),
    ])
}

fn update(state: &mut RadioClient, message: Message) -> Task<Message> {
    match message {
        Message::ConfigurePressed => {
            if state.lifecycle == LifecycleState::RunningLocal
                || state.lifecycle == LifecycleState::Starting
                || state.lifecycle == LifecycleState::Stopping
            {
                return Task::none();
            }
            state.configure = Some(configure::ConfigureScreen::new(
                &state.settings,
                state.paths.voicekeyer_dir.clone(),
            ));
            Task::none()
        }
        Message::Configure(message) => {
            let Some(configure) = state.configure.as_mut() else {
                return Task::none();
            };
            let (outcome, task) = configure.update(message, &state.paths.settings_file);
            match outcome {
                configure::Outcome::None => task.map(Message::Configure),
                configure::Outcome::Saved(settings) => {
                    state.settings = *settings;
                    state.configure = None;
                    state.lifecycle = LifecycleState::initial(&state.settings);
                    state.push_event("Saved Radio Client settings.");
                    Task::none()
                }
                configure::Outcome::Cancelled => {
                    state.configure = None;
                    Task::none()
                }
            }
        }
        Message::StartPressed => {
            if !matches!(
                state.lifecycle,
                LifecycleState::Stopped | LifecycleState::Error
            ) {
                return Task::none();
            }
            state.lifecycle = LifecycleState::Starting;
            state.push_event("Starting local radio host…");
            Task::perform(lifecycle::start(state.paths.clone()), |result| {
                Message::StartFinished(std::sync::Arc::new(std::sync::Mutex::new(Some(result))))
            })
        }
        Message::StopPressed => {
            if state.lifecycle == LifecycleState::Starting {
                state.lifecycle = LifecycleState::Stopping;
                state.push_event("Startup will stop as soon as the local host is ready.");
                return Task::none();
            }
            let Some(host) = state.host.take() else {
                return Task::none();
            };
            state.lifecycle = LifecycleState::Stopping;
            state.push_event("Stopping local radio host…");
            Task::perform(lifecycle::stop(host), Message::StopFinished)
        }
        Message::DebugLoggingChanged(enabled) => {
            let previous = state.settings.debug_logging_enabled;
            state.settings.debug_logging_enabled = enabled;
            if let Err(error) = save_atomic(&state.paths.settings_file, &state.settings) {
                state.settings.debug_logging_enabled = previous;
                state.push_event(format!("Unable to save debug logging setting: {error}"));
                return Task::none();
            }

            if let Some(log_level) = &state.log_level_override {
                state.push_event(format!(
                    "Saved debug logging setting. Command-line log level '{log_level}' remains active."
                ));
                return Task::none();
            }

            let log_level = effective_log_level(None, enabled);
            if let Some(filter) = &state.log_filter
                && let Err(error) = filter.reload(EnvFilter::new(log_level))
            {
                state.push_event(format!("Unable to update log level: {error}"));
                return Task::none();
            }
            state.push_event(if enabled {
                "Debug logging enabled."
            } else {
                "Debug logging disabled."
            });
            Task::none()
        }
        Message::StartFinished(result) => match result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            None => Task::none(),
            Some(result) => match result {
                Ok(result) => {
                    for event in result.events {
                        state.push_event(event);
                    }
                    state.host = Some(result.host);
                    state.backend_status = None;
                    if state.lifecycle == LifecycleState::Stopping
                        || state.pending_close_window.is_some()
                    {
                        update(state, Message::StopPressed)
                    } else {
                        state.lifecycle = LifecycleState::RunningLocal;
                        state.push_event("Local radio host is running.");
                        Task::none()
                    }
                }
                Err(error) => {
                    state.lifecycle = LifecycleState::Error;
                    state.push_event(format!("Unable to start local radio host: {error}"));
                    close_if_requested(state)
                }
            },
        },
        Message::StopFinished(events) => {
            for event in events {
                state.push_event(event);
            }
            state.host = None;
            state.backend_status = None;
            state.lifecycle = LifecycleState::Stopped;
            state.push_event("Local radio host stopped.");
            close_if_requested(state)
        }
        Message::BackendTick => {
            if let Some(host) = &state.host {
                for event in host.drain_backend_events() {
                    if state.backend_status.as_ref() != Some(&event.status) {
                        state.push_event(event.status.label());
                    }
                    state.backend_status = Some(event.status);
                }
            }
            Task::none()
        }
        Message::WindowCloseRequested(window_id) => {
            state.pending_close_window = Some(window_id);
            if state.host.is_some() || state.lifecycle == LifecycleState::Starting {
                update(state, Message::StopPressed)
            } else {
                close_if_requested(state)
            }
        }
    }
}

fn close_if_requested(state: &mut RadioClient) -> Task<Message> {
    state
        .pending_close_window
        .take()
        .map_or_else(Task::none, window::close)
}

impl RadioClient {
    fn push_event(&mut self, message: impl Into<String>) {
        self.events.push_back(EventEntry::new(message));
        if self.events.len() > EVENT_LOG_CAPACITY {
            self.events.pop_front();
        }
    }
}

fn view(state: &RadioClient) -> Element<'_, Message> {
    if let Some(configure) = &state.configure {
        return configure
            .view(&state.paths.voicekeyer_dir)
            .map(Message::Configure);
    }
    let configured = state.settings.is_radio_configured();
    let can_start = configured
        && matches!(
            state.lifecycle,
            LifecycleState::Stopped | LifecycleState::Error
        );
    let can_stop = matches!(
        state.lifecycle,
        LifecycleState::Starting | LifecycleState::RunningLocal
    );
    let can_configure = !matches!(
        state.lifecycle,
        LifecycleState::Starting | LifecycleState::RunningLocal | LifecycleState::Stopping
    );
    let mut details = Vec::new();
    if let Some(host) = &state.host {
        details.push(format!("Local WebSocket: {}", host.websocket_url));
    }
    if state.lifecycle == LifecycleState::RunningLocal {
        if let Some(status) = &state.backend_status {
            details.push(status.label());
        } else {
            details.push("Backend: registering…".to_string());
        }
    }
    let event_lines = state
        .events
        .iter()
        .map(|event| {
            text(format!("[{}] {}", event.timestamp, event.message))
                .size(13)
                .into()
        })
        .collect::<Vec<_>>();
    let header = row![
        Image::new(state.icon.clone()).width(96).height(96),
        column![
            text("Log73 Radio Client").size(32),
            text(state.lifecycle.label()).size(22),
            row![
                button("Configure")
                    .on_press_maybe(can_configure.then_some(Message::ConfigurePressed)),
                button("Start").on_press_maybe(can_start.then_some(Message::StartPressed)),
                button("Stop").on_press_maybe(can_stop.then_some(Message::StopPressed)),
            ]
            .spacing(12),
        ]
        .spacing(10)
        .align_x(iced::Alignment::Center),
    ]
    .spacing(18)
    .align_y(iced::alignment::Vertical::Center);
    let content = column![
        header,
        checkbox("Enable debug logging", state.settings.debug_logging_enabled)
            .on_toggle(Message::DebugLoggingChanged),
        column(details.into_iter().map(|detail| text(detail).into())).spacing(4),
        text(format!("Settings: {}", state.paths.settings_file.display())).size(14),
        text(format!(
            "Voice files: {}",
            state.paths.voicekeyer_dir.display()
        ))
        .size(14),
        text("Event log").size(18),
        scrollable(column(event_lines).spacing(3)).height(Length::Fill),
    ]
    .spacing(14)
    .align_x(iced::Alignment::Center);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .padding(24)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_overrides_are_resolved_consistently() {
        let cli = Cli {
            log_level: Some("debug".to_string()),
            log_file: Some(PathBuf::from("custom/log.txt")),
            config_dir: Some(PathBuf::from("custom/config")),
            data_dir: Some(PathBuf::from("custom/data")),
            app_dir: Some(PathBuf::from("custom/app")),
        };
        let paths = AppPaths::resolve(&cli);

        assert_eq!(
            paths.settings_file,
            PathBuf::from("custom/config/log73-radio-client.json")
        );
        assert_eq!(
            paths.voicekeyer_dir,
            PathBuf::from("custom/data/voicekeyer")
        );
        assert_eq!(paths.log_file, PathBuf::from("custom/log.txt"));
        assert_eq!(paths.app_dir, PathBuf::from("custom/app"));
    }

    #[test]
    fn default_log_path_follows_the_selected_data_directory() {
        let cli = Cli {
            log_level: None,
            log_file: None,
            config_dir: None,
            data_dir: Some(PathBuf::from("custom/data")),
            app_dir: None,
        };

        assert_eq!(
            AppPaths::resolve(&cli).log_file,
            PathBuf::from("custom/data/log73-radio-client.log")
        );
    }

    #[test]
    fn lifecycle_starts_stopped_only_for_valid_settings() {
        let mut settings = RadioClientSettings::default();
        assert_eq!(
            LifecycleState::initial(&settings),
            LifecycleState::NotConfigured
        );
        settings.radio.name = "Dummy".to_string();
        settings.radio.radio_kind = "dummy".to_string();
        settings.radio.transport_kind = "none".to_string();
        assert_eq!(LifecycleState::initial(&settings), LifecycleState::Stopped);
    }

    #[test]
    fn event_log_discards_entries_beyond_its_capacity() {
        let settings = RadioClientSettings::default();
        let paths = AppPaths {
            config_dir: PathBuf::new(),
            data_dir: PathBuf::new(),
            app_dir: PathBuf::new(),
            settings_file: PathBuf::from("settings.json"),
            voicekeyer_dir: PathBuf::from("voicekeyer"),
            log_file: PathBuf::from("radio-client.log"),
        };
        let mut client = RadioClient::new(settings, paths, None, None);
        for index in 0..=EVENT_LOG_CAPACITY {
            client.push_event(format!("event {index}"));
        }
        assert_eq!(client.events.len(), EVENT_LOG_CAPACITY);
        assert_eq!(client.events.front().unwrap().message, "event 1");
    }

    #[test]
    fn configured_debug_logging_sets_the_default_log_level() {
        assert_eq!(effective_log_level(None, false), "info");
        assert_eq!(effective_log_level(None, true), "debug");
        assert_eq!(effective_log_level(Some("trace"), true), "trace");
    }
}
