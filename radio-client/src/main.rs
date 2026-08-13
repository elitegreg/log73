mod configure;
mod settings;

use clap::Parser;
use iced::widget::image::Handle as ImageHandle;
use iced::widget::{Image, button, column, container, text};
use iced::{Element, Length, Task, Theme, application, window};
use settings::{RadioClientSettings, load_or_create, settings_file_path};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

const RADIO_CLIENT_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../static/log73-icon-512.png"
));
const WINDOW_WIDTH: f32 = 640.0;
const WINDOW_HEIGHT: f32 = 400.0;

#[derive(Debug, Parser)]
#[command(version, about = "Log73 client-side radio controller")]
struct Cli {
    #[arg(long, default_value = "info")]
    log_level: String,

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
    let _log_guard = match init_tracing(&cli.log_level, &paths.log_file) {
        Ok(guard) => guard,
        Err(error) => return startup_failure(format!("failed to initialize logging: {error}")),
    };
    let settings = match load_or_create(&paths.settings_file) {
        Ok(settings) => settings,
        Err(error) => return startup_failure(error),
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
            size: iced::Size::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            icon: radio_client_window_icon(),
            ..window::Settings::default()
        })
        .run_with(move || (RadioClient::new(settings, paths), Task::none()))
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
) -> std::io::Result<tracing_appender::non_blocking::WorkerGuard> {
    let filter = EnvFilter::try_new(log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)?;
    let (writer, guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .with(fmt::layer().with_writer(writer).with_ansi(false))
        .init();
    Ok(guard)
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
}

impl RadioClient {
    fn new(settings: RadioClientSettings, paths: AppPaths) -> Self {
        Self {
            settings,
            paths,
            icon: ImageHandle::from_bytes(RADIO_CLIENT_ICON_PNG),
            configure: None,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    ConfigurePressed,
    Configure(configure::Message),
}

fn update(state: &mut RadioClient, message: Message) -> Task<Message> {
    match message {
        Message::ConfigurePressed => {
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
                    Task::none()
                }
                configure::Outcome::Cancelled => {
                    state.configure = None;
                    Task::none()
                }
            }
        }
    }
}

fn view(state: &RadioClient) -> Element<'_, Message> {
    if let Some(configure) = &state.configure {
        return configure
            .view(&state.paths.voicekeyer_dir)
            .map(Message::Configure);
    }
    let status = if state.settings.is_radio_configured() {
        "Configured — stopped"
    } else {
        "Not configured"
    };
    let content = column![
        Image::new(state.icon.clone()).width(96).height(96),
        text("Log73 Radio Client").size(32),
        text(status).size(22),
        button("Configure").on_press(Message::ConfigurePressed),
        text(format!("Settings: {}", state.paths.settings_file.display())).size(14),
        text(format!(
            "Voice files: {}",
            state.paths.voicekeyer_dir.display()
        ))
        .size(14),
    ]
    .spacing(14)
    .align_x(iced::Alignment::Center);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(24)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_overrides_are_resolved_consistently() {
        let cli = Cli {
            log_level: "debug".to_string(),
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
            log_level: "info".to_string(),
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
}
