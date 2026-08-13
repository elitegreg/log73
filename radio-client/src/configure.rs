use crate::settings::{BackendSettings, RadioClientSettings, save_atomic};
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_editor, text_input,
};
use iced::{Element, Length, Task};
use radio_io::voice_keyer::{AudioDeviceInfo, VoiceKeyer};
use radio_io::{RadioSettings, list_serial_ports, supported_drivers};
use std::path::PathBuf;

pub struct ConfigureScreen {
    pub draft: RadioClientSettings,
    serial_ports: Vec<String>,
    input_devices: Vec<AudioDeviceInfo>,
    output_devices: Vec<AudioDeviceInfo>,
    audio_host: Option<String>,
    pub validation_message: Option<String>,
    pub connection_message: Option<String>,
    pub testing_connection: bool,
    tcp_port: String,
    serial_baud_rate: String,
    wsjtx_port: String,
    flrig_port: String,
    cw_increment: String,
    ssb_increment: String,
    cw_serial_baud_rate: String,
    cw_message_editor: text_editor::Content,
    voice_message_editor: text_editor::Content,
}

#[derive(Debug, Clone)]
pub enum Message {
    BackendUrlChanged(String),
    BackendUsernameChanged(String),
    BackendPasswordChanged(String),
    NameChanged(String),
    DriverSelected(String),
    TransportSelected(String),
    TcpHostChanged(String),
    TcpPortChanged(String),
    SerialPortSelected(String),
    SerialBaudChanged(String),
    OptionsChanged(String),
    DataModeSelected(String),
    RttyModeSelected(String),
    WsjtxEnabledChanged(bool),
    WsjtxBindSelected(String),
    WsjtxPortChanged(String),
    WsjtxMulticastChanged(String),
    FlrigEnabledChanged(bool),
    FlrigPortChanged(String),
    CwIncrementChanged(String),
    SsbIncrementChanged(String),
    RitClearChanged(bool),
    AudioHostSelected(String),
    InputDeviceSelected(String),
    OutputDeviceSelected(String),
    KeyerSelected(String),
    WinkeyerPortSelected(String),
    CwSerialPortSelected(String),
    CwSerialBaudChanged(String),
    CwSerialLineSelected(String),
    CwMessagesEdited(text_editor::Action),
    VoiceMessagesEdited(text_editor::Action),
    ValidateCwMessages,
    ValidateVoiceMessages,
    ResetCwMessages,
    ResetVoiceMessages,
    SetDefaults,
    Save,
    Cancel,
    TestConnection,
    ConnectionTestFinished(Result<String, String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    None,
    Saved(Box<RadioClientSettings>),
    Cancelled,
}

impl ConfigureScreen {
    pub fn new(settings: &RadioClientSettings, voicekeyer_dir: PathBuf) -> Self {
        let voice_keyer = VoiceKeyer::with_voicekeyer_dir(voicekeyer_dir);
        let serial_ports = list_serial_ports()
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        let input_devices = voice_keyer.input_devices().unwrap_or_default();
        let output_devices = voice_keyer.output_devices().unwrap_or_default();
        let audio_host = selected_device_host(settings, &input_devices, &output_devices);
        Self {
            draft: settings.clone(),
            serial_ports,
            input_devices,
            output_devices,
            audio_host,
            validation_message: None,
            connection_message: None,
            testing_connection: false,
            tcp_port: settings.radio.tcp_port.to_string(),
            serial_baud_rate: settings.radio.serial_baud_rate.to_string(),
            wsjtx_port: settings.radio.wsjtx_port.to_string(),
            flrig_port: settings.radio.flrig_port.to_string(),
            cw_increment: settings.radio.cw_tuning_increment_hz.to_string(),
            ssb_increment: settings.radio.ssb_tuning_increment_hz.to_string(),
            cw_serial_baud_rate: settings.radio.cw_serial_baud_rate.to_string(),
            cw_message_editor: text_editor::Content::with_text(&settings.radio.cw_messages),
            voice_message_editor: text_editor::Content::with_text(&settings.radio.voice_messages),
        }
    }

    pub fn update(
        &mut self,
        message: Message,
        settings_path: &std::path::Path,
    ) -> (Outcome, Task<Message>) {
        self.validation_message = None;
        match message {
            Message::BackendUrlChanged(value) => self.draft.backend.base_url = value,
            Message::BackendUsernameChanged(value) => self.draft.backend.username = value,
            Message::BackendPasswordChanged(value) => self.draft.backend.password = value,
            Message::NameChanged(value) => self.draft.radio.name = value,
            Message::DriverSelected(value) => self.set_driver(value),
            Message::TransportSelected(value) => self.draft.radio.transport_kind = value,
            Message::TcpHostChanged(value) => self.draft.radio.tcp_host = value,
            Message::TcpPortChanged(value) => self.tcp_port = value,
            Message::SerialPortSelected(value) => {
                self.draft.radio.serial_port = none_to_empty(value)
            }
            Message::SerialBaudChanged(value) => self.serial_baud_rate = value,
            Message::OptionsChanged(value) => self.draft.radio.options = value,
            Message::DataModeSelected(value) => self.draft.radio.data_mode = value,
            Message::RttyModeSelected(value) => self.draft.radio.rtty_mode = value,
            Message::WsjtxEnabledChanged(value) => self.draft.radio.wsjtx_enabled = value,
            Message::WsjtxBindSelected(value) => self.draft.radio.wsjtx_bind_address = value,
            Message::WsjtxPortChanged(value) => self.wsjtx_port = value,
            Message::WsjtxMulticastChanged(value) => self.draft.radio.wsjtx_multicast_group = value,
            Message::FlrigEnabledChanged(value) => self.draft.radio.flrig_enabled = value,
            Message::FlrigPortChanged(value) => self.flrig_port = value,
            Message::CwIncrementChanged(value) => self.cw_increment = value,
            Message::SsbIncrementChanged(value) => self.ssb_increment = value,
            Message::RitClearChanged(value) => self.draft.radio.rit_clear_on_log = value,
            Message::AudioHostSelected(value) => {
                self.audio_host = (value != "All subsystems").then_some(value)
            }
            Message::InputDeviceSelected(value) => {
                self.draft.radio.voice_input_device_id =
                    selected_device_id(&self.input_devices, value)
            }
            Message::OutputDeviceSelected(value) => {
                self.draft.radio.voice_output_device_id =
                    selected_device_id(&self.output_devices, value)
            }
            Message::KeyerSelected(value) => self.draft.radio.cw_keyer_type = value,
            Message::WinkeyerPortSelected(value) => {
                self.draft.radio.winkeyer_serial_port = none_to_empty(value)
            }
            Message::CwSerialPortSelected(value) => {
                self.draft.radio.cw_serial_port = none_to_empty(value)
            }
            Message::CwSerialBaudChanged(value) => self.cw_serial_baud_rate = value,
            Message::CwSerialLineSelected(value) => self.draft.radio.cw_serial_line = value,
            Message::CwMessagesEdited(action) => {
                self.cw_message_editor.perform(action);
                self.draft.radio.cw_messages = self.cw_message_editor.text();
            }
            Message::VoiceMessagesEdited(action) => {
                self.voice_message_editor.perform(action);
                self.draft.radio.voice_messages = self.voice_message_editor.text();
            }
            Message::ValidateCwMessages => {
                self.validation_message =
                    radio_io::validate_cw_messages(&self.draft.radio.cw_messages)
                        .err()
                        .or(Some("CW messages are valid.".to_string()))
            }
            Message::ValidateVoiceMessages => {
                self.validation_message =
                    radio_io::validate_voice_messages(&self.draft.radio.voice_messages)
                        .err()
                        .or(Some("Voice messages are valid.".to_string()))
            }
            Message::ResetCwMessages => {
                self.draft.radio.cw_messages = RadioSettings::default().cw_messages;
                self.cw_message_editor =
                    text_editor::Content::with_text(&self.draft.radio.cw_messages);
            }
            Message::ResetVoiceMessages => {
                self.draft.radio.voice_messages = RadioSettings::default().voice_messages;
                self.voice_message_editor =
                    text_editor::Content::with_text(&self.draft.radio.voice_messages);
            }
            Message::SetDefaults => {
                self.draft.reset_radio_defaults();
                self.reset_numeric_fields();
                self.cw_message_editor =
                    text_editor::Content::with_text(&self.draft.radio.cw_messages);
                self.voice_message_editor =
                    text_editor::Content::with_text(&self.draft.radio.voice_messages);
            }
            Message::Save => match self.validated_settings() {
                Ok(settings) => match save_atomic(settings_path, &settings) {
                    Ok(()) => return (Outcome::Saved(Box::new(settings)), Task::none()),
                    Err(error) => self.validation_message = Some(error),
                },
                Err(error) => self.validation_message = Some(error),
            },
            Message::Cancel => return (Outcome::Cancelled, Task::none()),
            Message::TestConnection => {
                self.connection_message = None;
                self.testing_connection = true;
                let backend = self.draft.backend.clone();
                return (
                    Outcome::None,
                    Task::perform(test_connection(backend), Message::ConnectionTestFinished),
                );
            }
            Message::ConnectionTestFinished(result) => {
                self.testing_connection = false;
                self.connection_message = Some(match result {
                    Ok(message) | Err(message) => message,
                });
            }
        }
        (Outcome::None, Task::none())
    }

    fn set_driver(&mut self, value: String) {
        self.draft.radio.radio_kind = value;
        let modes = driver_modes(&self.draft.radio.radio_kind);
        if !modes.contains(&self.draft.radio.data_mode) {
            self.draft.radio.data_mode =
                driver_default_data_mode(&self.draft.radio.radio_kind).unwrap_or_default();
        }
        if !modes.contains(&self.draft.radio.rtty_mode) {
            self.draft.radio.rtty_mode =
                driver_default_rtty_mode(&self.draft.radio.radio_kind).unwrap_or_default();
        }
        if self.draft.radio.radio_kind.eq_ignore_ascii_case("dummy") {
            self.draft.radio.transport_kind = "none".to_string();
        } else if self.draft.radio.transport_kind == "none" {
            self.draft.radio.transport_kind = "serial".to_string();
        }
        if !driver_supports_cat(&self.draft.radio.radio_kind)
            && self.draft.radio.cw_keyer_type == "cat"
        {
            self.draft.radio.cw_keyer_type = "none".to_string();
        }
    }

    fn reset_numeric_fields(&mut self) {
        self.tcp_port = self.draft.radio.tcp_port.to_string();
        self.serial_baud_rate = self.draft.radio.serial_baud_rate.to_string();
        self.wsjtx_port = self.draft.radio.wsjtx_port.to_string();
        self.flrig_port = self.draft.radio.flrig_port.to_string();
        self.cw_increment = self.draft.radio.cw_tuning_increment_hz.to_string();
        self.ssb_increment = self.draft.radio.ssb_tuning_increment_hz.to_string();
        self.cw_serial_baud_rate = self.draft.radio.cw_serial_baud_rate.to_string();
    }

    fn validated_settings(&self) -> Result<RadioClientSettings, String> {
        let mut settings = self.draft.clone();
        settings.radio.tcp_port = parse_u16(&self.tcp_port);
        settings.radio.serial_baud_rate = parse_u32(&self.serial_baud_rate);
        settings.radio.wsjtx_port = parse_u16(&self.wsjtx_port);
        settings.radio.flrig_port = parse_u16(&self.flrig_port);
        settings.radio.cw_tuning_increment_hz = parse_u32(&self.cw_increment);
        settings.radio.ssb_tuning_increment_hz = parse_u32(&self.ssb_increment);
        settings.radio.cw_serial_baud_rate = parse_u32(&self.cw_serial_baud_rate);
        validated_settings(&settings)
    }

    pub fn view(&self, voicekeyer_dir: &std::path::Path) -> Element<'_, Message> {
        let driver_options = supported_drivers()
            .iter()
            .map(|driver| driver.id.to_string())
            .collect::<Vec<_>>();
        let selected_driver = selected_value(&driver_options, &self.draft.radio.radio_kind);
        let modes = driver_modes(&self.draft.radio.radio_kind);
        let serial_options = with_missing(
            "No selection",
            &self.serial_ports,
            &self.draft.radio.serial_port,
        );
        let winkeyer_options = with_missing(
            "No selection",
            &self.serial_ports,
            &self.draft.radio.winkeyer_serial_port,
        );
        let cw_serial_options = with_missing(
            "No selection",
            &self.serial_ports,
            &self.draft.radio.cw_serial_port,
        );
        let hosts = audio_hosts(&self.input_devices, &self.output_devices);
        let host_options = std::iter::once("All subsystems".to_string())
            .chain(hosts)
            .collect::<Vec<_>>();
        let input_options = device_options(
            &self.input_devices,
            self.audio_host.as_deref(),
            self.draft.radio.voice_input_device_id.as_deref(),
        );
        let output_options = device_options(
            &self.output_devices,
            self.audio_host.as_deref(),
            self.draft.radio.voice_output_device_id.as_deref(),
        );
        let keyers = keyer_options(&self.draft.radio.radio_kind);
        let status = self
            .validation_message
            .as_ref()
            .or(self.connection_message.as_ref());

        let mut content = column![
            text("Configure Radio Client").size(30),
            section(
                "Backend",
                column![
                    field(
                        "Backend URL",
                        text_input("http://127.0.0.1:7300", &self.draft.backend.base_url)
                            .on_input(Message::BackendUrlChanged)
                    ),
                    field(
                        "Username",
                        text_input(
                            "Optional when authentication is disabled",
                            &self.draft.backend.username
                        )
                        .on_input(Message::BackendUsernameChanged)
                    ),
                    field(
                        "Password",
                        text_input("Password", &self.draft.backend.password)
                            .secure(true)
                            .on_input(Message::BackendPasswordChanged)
                    ),
                    button(if self.testing_connection {
                        "Testing connection…"
                    } else {
                        "Test connection"
                    })
                    .on_press_maybe((!self.testing_connection).then_some(Message::TestConnection)),
                ]
            ),
            section(
                "Radio",
                column![
                    field(
                        "Name",
                        text_input("Radio name", &self.draft.radio.name)
                            .on_input(Message::NameChanged)
                    ),
                    field(
                        "Driver",
                        pick_list(driver_options, selected_driver, Message::DriverSelected)
                    ),
                    field(
                        "Transport",
                        pick_list(
                            vec!["none".to_string(), "tcp".to_string(), "serial".to_string()],
                            selected_value(
                                &["none".to_string(), "tcp".to_string(), "serial".to_string()],
                                &self.draft.radio.transport_kind
                            ),
                            Message::TransportSelected
                        )
                    ),
                ]
            ),
        ]
        .spacing(12)
        .padding(20);

        if self.draft.radio.transport_kind == "tcp" {
            content = content.push(section(
                "TCP transport",
                column![
                    field(
                        "Host",
                        text_input("127.0.0.1", &self.draft.radio.tcp_host)
                            .on_input(Message::TcpHostChanged)
                    ),
                    field(
                        "Port",
                        numeric_input(&self.tcp_port, Message::TcpPortChanged)
                    ),
                ],
            ));
        } else if self.draft.radio.transport_kind == "serial" {
            content = content.push(section(
                "Serial transport",
                column![
                    field(
                        "Serial port",
                        pick_list(
                            serial_options,
                            selected_serial_value(
                                &self.serial_ports,
                                &self.draft.radio.serial_port
                            ),
                            Message::SerialPortSelected
                        )
                    ),
                    field(
                        "Baud rate",
                        numeric_input(&self.serial_baud_rate, Message::SerialBaudChanged)
                    ),
                ],
            ));
        }

        content = content.push(section(
            "Radio modes",
            column![
                field(
                    "Driver options",
                    text_input("Optional driver options", &self.draft.radio.options)
                        .on_input(Message::OptionsChanged)
                ),
                field(
                    "DATA radio mode",
                    pick_list(
                        modes.clone(),
                        selected_value(&modes, &self.draft.radio.data_mode),
                        Message::DataModeSelected
                    )
                ),
                field(
                    "RTTY radio mode",
                    pick_list(
                        modes,
                        selected_value(
                            &driver_modes(&self.draft.radio.radio_kind),
                            &self.draft.radio.rtty_mode
                        ),
                        Message::RttyModeSelected
                    )
                ),
            ],
        ));
        content = content.push(section(
            "WSJT-X",
            wsjtx_fields(&self.draft.radio, &self.wsjtx_port),
        ));
        content = content.push(section(
            "FLRig emulation",
            flrig_fields(&self.draft.radio, &self.flrig_port),
        ));
        content = content.push(section(
            "Tuning",
            column![
                field(
                    "CW increment (Hz)",
                    numeric_input(&self.cw_increment, Message::CwIncrementChanged)
                ),
                field(
                    "SSB increment (Hz)",
                    numeric_input(&self.ssb_increment, Message::SsbIncrementChanged)
                ),
                checkbox("Clear RIT when logging", self.draft.radio.rit_clear_on_log)
                    .on_toggle(Message::RitClearChanged),
            ],
        ));
        content = content.push(section(
            "Audio",
            column![
                field(
                    "Audio subsystem",
                    pick_list(
                        host_options,
                        self.audio_host
                            .clone()
                            .or(Some("All subsystems".to_string())),
                        Message::AudioHostSelected
                    )
                ),
                field(
                    "Voice input",
                    pick_list(
                        input_options,
                        selected_device_value(
                            &self.input_devices,
                            self.draft.radio.voice_input_device_id.as_deref()
                        ),
                        Message::InputDeviceSelected
                    )
                ),
                field(
                    "Voice output",
                    pick_list(
                        output_options,
                        selected_device_value(
                            &self.output_devices,
                            self.draft.radio.voice_output_device_id.as_deref()
                        ),
                        Message::OutputDeviceSelected
                    )
                ),
            ],
        ));
        content = content.push(section(
            "CW keying",
            column![field(
                "Keyer",
                pick_list(
                    keyers.clone(),
                    selected_value(&keyers, &self.draft.radio.cw_keyer_type),
                    Message::KeyerSelected
                )
            ),]
            .push_maybe((self.draft.radio.cw_keyer_type == "winkeyer").then(|| {
                field(
                    "Winkeyer serial port",
                    pick_list(
                        winkeyer_options,
                        selected_serial_value(
                            &self.serial_ports,
                            &self.draft.radio.winkeyer_serial_port,
                        ),
                        Message::WinkeyerPortSelected,
                    ),
                )
            }))
            .push_maybe((self.draft.radio.cw_keyer_type == "serial").then(|| {
                column![
                    field(
                        "CW serial port",
                        pick_list(
                            cw_serial_options,
                            selected_serial_value(
                                &self.serial_ports,
                                &self.draft.radio.cw_serial_port
                            ),
                            Message::CwSerialPortSelected
                        )
                    ),
                    field(
                        "CW serial baud",
                        numeric_input(&self.cw_serial_baud_rate, Message::CwSerialBaudChanged)
                    ),
                    field(
                        "CW serial line",
                        pick_list(
                            vec!["dtr".to_string(), "rts".to_string()],
                            selected_value(
                                &["dtr".to_string(), "rts".to_string()],
                                &self.draft.radio.cw_serial_line
                            ),
                            Message::CwSerialLineSelected
                        )
                    ),
                ]
            })),
        ));
        content = content.push(section(
            "CW messages",
            column![
                text_editor(&self.cw_message_editor)
                    .placeholder("CW messages")
                    .on_action(Message::CwMessagesEdited)
                    .height(220),
                row![
                    button("Validate").on_press(Message::ValidateCwMessages),
                    button("Reset to defaults").on_press(Message::ResetCwMessages)
                ]
                .spacing(10),
            ],
        ));
        content = content.push(section(
            "Voice messages",
            column![
                text(format!("Local voice files: {}", voicekeyer_dir.display())).size(14),
                text_editor(&self.voice_message_editor)
                    .placeholder("Voice messages")
                    .on_action(Message::VoiceMessagesEdited)
                    .height(220),
                row![
                    button("Validate").on_press(Message::ValidateVoiceMessages),
                    button("Reset to defaults").on_press(Message::ResetVoiceMessages)
                ]
                .spacing(10),
            ],
        ));
        if let Some(status) = status {
            content = content.push(text(status));
        }
        content = content.push(
            row![
                button("Save").on_press(Message::Save),
                button("Cancel").on_press(Message::Cancel),
                button("Set defaults").on_press(Message::SetDefaults),
            ]
            .spacing(12),
        );
        scrollable(container(content).width(Length::Fill)).into()
    }
}

fn section<'a>(title: &'a str, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        column![text(title).size(22), body.into()]
            .spacing(8)
            .padding(12),
    )
    .width(Length::Fill)
    .into()
}
fn field<'a>(label: &'a str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![text(label), control.into()].spacing(4).into()
}
fn numeric_input<'a>(
    value: &'a str,
    message: fn(String) -> Message,
) -> iced::widget::TextInput<'a, Message> {
    text_input("", value).on_input(message)
}
fn parse_u16(value: &str) -> u16 {
    value.parse().unwrap_or(0)
}
fn parse_u32(value: &str) -> u32 {
    value.parse().unwrap_or(0)
}
fn none_to_empty(value: String) -> String {
    if value == "No selection" {
        String::new()
    } else {
        value
            .strip_suffix(" (unavailable)")
            .unwrap_or(&value)
            .to_string()
    }
}
fn selected_device_id(devices: &[AudioDeviceInfo], value: String) -> Option<String> {
    if value == "No device" {
        return None;
    }
    devices
        .iter()
        .find(|device| device_option_label(device) == value)
        .map(|device| device.id.clone())
        .or_else(|| {
            value
                .strip_suffix(" (unavailable)")
                .map(ToString::to_string)
        })
}

fn validated_settings(draft: &RadioClientSettings) -> Result<RadioClientSettings, String> {
    let mut settings = draft.clone();
    radio_io::normalize_radio_settings(&mut settings.radio)?;
    radio_io::validate_radio_settings(&settings.radio)?;
    Ok(settings)
}

async fn test_connection(backend: BackendSettings) -> Result<String, String> {
    let url = connection_endpoint(&backend.base_url)?;
    let client = reqwest::Client::new();
    let request = if !backend_credentials_configured(&backend) {
        client.get(url)
    } else {
        client
            .get(url)
            .basic_auth(backend.username, Some(backend.password))
    };
    let response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            "Backend connection timed out.".to_string()
        } else if error.is_connect() {
            "Unable to connect to the backend.".to_string()
        } else {
            "Backend connection failed.".to_string()
        }
    })?;
    if response.status().is_success() {
        Ok("Authenticated backend connection succeeded.".to_string())
    } else if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        Err("Backend rejected the configured credentials.".to_string())
    } else {
        Err(format!(
            "Backend returned HTTP {}.",
            response.status().as_u16()
        ))
    }
}

fn connection_endpoint(base_url: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|_| "Enter a valid HTTP or HTTPS backend URL.".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(
            "Enter a valid HTTP or HTTPS backend URL without embedded credentials.".to_string(),
        );
    }
    let path = format!("{}/api/config", url.path().trim_end_matches('/'));
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn backend_credentials_configured(backend: &BackendSettings) -> bool {
    !backend.username.is_empty() || !backend.password.is_empty()
}

fn driver_modes(driver: &str) -> Vec<String> {
    radio_io::modes::transmit_modes_for_radio_kind(driver)
        .map(|modes| modes.iter().map(ToString::to_string).collect())
        .unwrap_or_default()
}
fn driver_default_data_mode(driver: &str) -> Result<String, String> {
    let modes = radio_io::modes::transmit_modes_for_radio_kind(driver)?;
    Ok(radio_io::modes::default_data_mode(modes).to_string())
}
fn driver_default_rtty_mode(driver: &str) -> Result<String, String> {
    let modes = radio_io::modes::transmit_modes_for_radio_kind(driver)?;
    Ok(radio_io::modes::default_rtty_mode(modes).to_string())
}
fn driver_supports_cat(driver: &str) -> bool {
    radio_io::modes::supports_cat_cw_keying_for_radio_kind(driver).unwrap_or(false)
}
fn selected_value(options: &[String], value: &str) -> Option<String> {
    options
        .iter()
        .find(|option| option.eq_ignore_ascii_case(value))
        .cloned()
}
fn with_missing(default: &str, options: &[String], selected: &str) -> Vec<String> {
    let mut values = vec![default.to_string()];
    values.extend(options.iter().cloned());
    if !selected.is_empty() && !values.iter().any(|value| value == selected) {
        values.push(format!("{selected} (unavailable)"));
    }
    values
}
fn selected_serial_value(options: &[String], selected: &str) -> Option<String> {
    if selected.is_empty() {
        Some("No selection".to_string())
    } else if options.iter().any(|value| value == selected) {
        Some(selected.to_string())
    } else {
        Some(format!("{selected} (unavailable)"))
    }
}
fn audio_hosts(input: &[AudioDeviceInfo], output: &[AudioDeviceInfo]) -> Vec<String> {
    let mut hosts = input
        .iter()
        .chain(output)
        .map(|device| device.host.clone())
        .filter(|host| !host.is_empty())
        .collect::<Vec<_>>();
    hosts.sort();
    hosts.dedup();
    hosts
}
fn selected_device_host(
    settings: &RadioClientSettings,
    input: &[AudioDeviceInfo],
    output: &[AudioDeviceInfo],
) -> Option<String> {
    let id = settings
        .radio
        .voice_input_device_id
        .as_deref()
        .or(settings.radio.voice_output_device_id.as_deref())?;
    input
        .iter()
        .chain(output)
        .find(|device| device.id == id)
        .map(|device| device.host.clone())
}
fn device_options(
    devices: &[AudioDeviceInfo],
    host: Option<&str>,
    selected: Option<&str>,
) -> Vec<String> {
    let mut options = vec!["No device".to_string()];
    options.extend(
        devices
            .iter()
            .filter(|device| host.is_none_or(|host| device.host == host))
            .map(device_option_label),
    );
    if let Some(selected) = selected.filter(|selected| !selected.is_empty()) {
        let selected_label = devices
            .iter()
            .find(|device| device.id == selected)
            .map(device_option_label)
            .unwrap_or_else(|| format!("{selected} (unavailable)"));
        if !options.contains(&selected_label) {
            options.push(selected_label);
        }
    }
    options
}
fn device_option_label(device: &AudioDeviceInfo) -> String {
    format!(
        "{}{} [{}]",
        device.name,
        if device.is_default { " (default)" } else { "" },
        device.id
    )
}
fn selected_device_value(devices: &[AudioDeviceInfo], selected: Option<&str>) -> Option<String> {
    selected
        .filter(|value| !value.is_empty())
        .map(|id| {
            devices
                .iter()
                .find(|device| device.id == id)
                .map(device_option_label)
                .unwrap_or_else(|| format!("{id} (unavailable)"))
        })
        .or(Some("No device".to_string()))
}
fn keyer_options(driver: &str) -> Vec<String> {
    let mut values = vec!["none".to_string(), "winkeyer".to_string()];
    if driver_supports_cat(driver) {
        values.push("cat".to_string());
    }
    values.push("serial".to_string());
    values
}

fn wsjtx_fields<'a>(radio: &'a RadioSettings, port: &'a str) -> Element<'a, Message> {
    let mut body = column![
        checkbox("Enable WSJT-X in DATA mode", radio.wsjtx_enabled)
            .on_toggle(Message::WsjtxEnabledChanged)
    ]
    .spacing(8);
    if radio.wsjtx_enabled {
        body = body
            .push(field(
                "Bind address",
                pick_list(
                    vec!["127.0.0.1".to_string(), "0.0.0.0".to_string()],
                    selected_value(
                        &["127.0.0.1".to_string(), "0.0.0.0".to_string()],
                        &radio.wsjtx_bind_address,
                    ),
                    Message::WsjtxBindSelected,
                ),
            ))
            .push(field(
                "UDP port",
                numeric_input(port, Message::WsjtxPortChanged),
            ))
            .push(field(
                "Multicast group",
                text_input(
                    "Optional IPv4 multicast address",
                    &radio.wsjtx_multicast_group,
                )
                .on_input(Message::WsjtxMulticastChanged),
            ));
    }
    body.into()
}

fn flrig_fields<'a>(radio: &'a RadioSettings, port: &'a str) -> Element<'a, Message> {
    let mut body = column![
        checkbox("Enable FLRig emulation", radio.flrig_enabled)
            .on_toggle(Message::FlrigEnabledChanged)
    ]
    .spacing(8);
    if radio.flrig_enabled {
        body = body.push(field(
            "TCP port",
            numeric_input(port, Message::FlrigPortChanged),
        ));
    }
    body.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_settings() -> RadioClientSettings {
        let mut settings = RadioClientSettings::default();
        settings.radio.name = "Test radio".to_string();
        settings.radio.radio_kind = "dummy".to_string();
        settings.radio.transport_kind = "none".to_string();
        settings
    }

    #[test]
    fn save_validation_normalizes_driver_mode_defaults() {
        let settings = valid_settings();
        let normalized = validated_settings(&settings).unwrap();
        assert!(!normalized.radio.data_mode.is_empty());
        assert!(!normalized.radio.rtty_mode.is_empty());
    }

    #[test]
    fn unavailable_device_is_preserved_in_options() {
        let options = device_options(&[], None, Some("missing-device"));
        assert!(options.contains(&"missing-device (unavailable)".to_string()));
    }

    #[test]
    fn unavailable_serial_port_is_preserved_in_options() {
        assert_eq!(
            selected_serial_value(&[], "/dev/ttyUSB9"),
            Some("/dev/ttyUSB9 (unavailable)".to_string())
        );
    }

    #[test]
    fn resetting_defaults_retains_backend_credentials_and_identity() {
        let mut screen = ConfigureScreen::new(&valid_settings(), PathBuf::from("voicekeyer"));
        let id = screen.draft.client_instance_id;
        screen.draft.backend.username = "operator".to_string();
        screen.draft.backend.password = "secret".to_string();
        let _ = screen.update(Message::SetDefaults, std::path::Path::new("settings.json"));
        assert_eq!(screen.draft.client_instance_id, id);
        assert_eq!(screen.draft.backend.password, "secret");
    }

    #[test]
    fn save_persists_valid_normalized_settings_and_rejects_invalid_drafts() {
        let path = std::env::temp_dir().join(format!(
            "log73-radio-client-configure-{}.json",
            uuid::Uuid::new_v4()
        ));
        let mut screen = ConfigureScreen::new(&valid_settings(), PathBuf::from("voicekeyer"));
        let (outcome, _) = screen.update(Message::Save, &path);
        assert!(matches!(outcome, Outcome::Saved(_)));
        assert!(path.exists());
        let saved = std::fs::read_to_string(&path).unwrap();

        screen.draft.radio.name.clear();
        let (outcome, _) = screen.update(Message::Save, &path);
        assert_eq!(outcome, Outcome::None);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
        assert!(
            screen
                .validation_message
                .as_deref()
                .is_some_and(|message| message.contains("radio name"))
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn connection_endpoint_targets_config_without_leaking_credentials() {
        assert_eq!(
            connection_endpoint("http://logger.example:7300/")
                .unwrap()
                .as_str(),
            "http://logger.example:7300/api/config"
        );
        assert!(connection_endpoint("http://operator:secret@logger.example").is_err());
        assert!(!backend_credentials_configured(&BackendSettings::default()));
        assert!(backend_credentials_configured(&BackendSettings {
            username: "operator".to_string(),
            ..BackendSettings::default()
        }));
    }
}
