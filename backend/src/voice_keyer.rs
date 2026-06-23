use crate::db::RadioConfig;
use crate::voice_messages;
use rodio::Source;
use rodio::cpal::{self, traits::DeviceTrait, traits::HostTrait};
use std::any::Any;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, mpsc as std_mpsc};
use std::thread;
use std::time::Duration;

const DEFAULT_OUTPUT_DEVICE_KEY: &str = "__default_output__";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDeviceKind {
    Input,
    Output,
}

impl AudioDeviceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AudioStreamConfig {
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_format: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub host: String,
    pub name: String,
    pub description: String,
    pub manufacturer: Option<String>,
    pub driver: Option<String>,
    pub device_type: String,
    pub interface_type: String,
    pub direction: String,
    pub address: Option<String>,
    pub extended: Vec<String>,
    pub is_default: bool,
    pub default_config: Option<AudioStreamConfig>,
}

#[derive(Clone)]
pub struct AudioDeviceReference {
    pub info: AudioDeviceInfo,
    _handle: Arc<dyn Any + Send + Sync>,
}

impl std::fmt::Debug for AudioDeviceReference {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AudioDeviceReference")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RegisteredVoiceFile {
    pub key: String,
    pub file_name: String,
    pub byte_len: usize,
}

#[derive(Clone)]
pub struct VoiceKeyer {
    audio: Arc<dyn AudioBackend>,
    files: Arc<RwLock<HashMap<String, RegisteredVoiceFileData>>>,
    voicekeyer_dir: Arc<PathBuf>,
}

#[allow(dead_code)]
#[derive(Clone)]
struct RegisteredVoiceFileData {
    file_name: String,
    bytes: Arc<[u8]>,
}

pub struct VoicePlayback {
    duration: Duration,
    handle: Box<dyn PlaybackHandle>,
}

impl std::fmt::Debug for VoicePlayback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VoicePlayback")
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

impl VoicePlayback {
    fn new(duration: Duration, handle: Box<dyn PlaybackHandle>) -> Self {
        Self { duration, handle }
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn stop(&mut self) {
        self.handle.stop();
    }
}

trait PlaybackHandle: Send {
    fn stop(&mut self);
}

pub struct VoicePlaybackThread {
    radio_id: i64,
    commands: std_mpsc::Sender<VoicePlaybackThreadCommand>,
    handle: Option<thread::JoinHandle<()>>,
}

enum VoicePlayTarget {
    RegisteredKey(String),
    RelativePath(String),
}

struct VoicePlayCommand {
    target: VoicePlayTarget,
    completed: tokio::sync::oneshot::Sender<Result<Duration, String>>,
}

enum VoicePlaybackThreadCommand {
    Play(VoicePlayCommand),
    Stop,
    Shutdown,
}

impl VoicePlaybackThread {
    pub fn spawn(
        radio_id: i64,
        voice_keyer: VoiceKeyer,
        output_device_id: Option<String>,
    ) -> Result<Self, String> {
        let (commands, command_rx) = std_mpsc::channel();
        let handle = thread::Builder::new()
            .name(format!("log73-voice-keyer-{radio_id}"))
            .spawn(move || {
                run_voice_playback_thread(radio_id, voice_keyer, output_device_id, command_rx)
            })
            .map_err(|error| format!("failed to start voice keyer thread: {error}"))?;

        Ok(Self {
            radio_id,
            commands,
            handle: Some(handle),
        })
    }

    #[allow(dead_code)]
    pub fn play_key(
        &self,
        key: &str,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<Duration, String>>, String> {
        let key = normalize_voice_key(key)?;
        self.play_registered_key(key)
    }

    pub fn play_message(
        &self,
        mode: &str,
        key: &str,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<Duration, String>>, String> {
        self.play_registered_key(voice_message_registry_key(self.radio_id, mode, key)?)
    }

    pub fn play_file_path(
        &self,
        relative_path: &str,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<Duration, String>>, String> {
        let (completed, completion_rx) = tokio::sync::oneshot::channel();
        self.commands
            .send(VoicePlaybackThreadCommand::Play(VoicePlayCommand {
                target: VoicePlayTarget::RelativePath(relative_path.trim().to_string()),
                completed,
            }))
            .map_err(|_| "voice keyer thread unavailable".to_string())?;
        Ok(completion_rx)
    }

    fn play_registered_key(
        &self,
        key: String,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<Duration, String>>, String> {
        let (completed, completion_rx) = tokio::sync::oneshot::channel();
        self.commands
            .send(VoicePlaybackThreadCommand::Play(VoicePlayCommand {
                target: VoicePlayTarget::RegisteredKey(key),
                completed,
            }))
            .map_err(|_| "voice keyer thread unavailable".to_string())?;
        Ok(completion_rx)
    }

    pub fn stop_keying(&self) {
        let _ = self.commands.send(VoicePlaybackThreadCommand::Stop);
    }

    pub fn shutdown(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = self.commands.send(VoicePlaybackThreadCommand::Shutdown);
            let _ = handle.join();
        }
    }
}

impl Drop for VoicePlaybackThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_voice_playback_thread(
    radio_id: i64,
    voice_keyer: VoiceKeyer,
    configured_output_device_id: Option<String>,
    commands: std_mpsc::Receiver<VoicePlaybackThreadCommand>,
) {
    while let Ok(command) = commands.recv() {
        match command {
            VoicePlaybackThreadCommand::Play(command) => {
                let playback = match match command.target {
                    VoicePlayTarget::RegisteredKey(key) => voice_keyer
                        .play_registered_key(&key, configured_output_device_id.as_deref()),
                    VoicePlayTarget::RelativePath(relative_path) => voice_keyer
                        .play_relative_path(&relative_path, configured_output_device_id.as_deref()),
                } {
                    Ok(playback) => playback,
                    Err(error) => {
                        let _ = command.completed.send(Err(error));
                        continue;
                    }
                };

                if !wait_for_voice_playback(radio_id, playback, command.completed, &commands) {
                    break;
                }
            }
            VoicePlaybackThreadCommand::Stop => {}
            VoicePlaybackThreadCommand::Shutdown => break,
        }
    }
}

fn wait_for_voice_playback(
    radio_id: i64,
    mut playback: VoicePlayback,
    completed: tokio::sync::oneshot::Sender<Result<Duration, String>>,
    commands: &std_mpsc::Receiver<VoicePlaybackThreadCommand>,
) -> bool {
    let duration = playback.duration();
    let wait_duration = if duration.is_zero() {
        Duration::from_millis(500)
    } else {
        duration
    };
    let deadline = std::time::Instant::now() + wait_duration;

    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            let _ = completed.send(Ok(duration));
            return true;
        }

        match commands.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(VoicePlaybackThreadCommand::Stop) => {
                playback.stop();
                let _ = completed.send(Err("keying stopped".to_string()));
                return true;
            }
            Ok(VoicePlaybackThreadCommand::Shutdown) => {
                playback.stop();
                let _ = completed.send(Err("keying shutdown".to_string()));
                return false;
            }
            Ok(VoicePlaybackThreadCommand::Play(command)) => {
                let _ = command.completed.send(Err(format!(
                    "voice keyer thread for radio {radio_id} is busy"
                )));
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => {
                let _ = completed.send(Ok(duration));
                return true;
            }
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                playback.stop();
                let _ = completed.send(Err("voice keyer thread disconnected".to_string()));
                return false;
            }
        }
    }
}

trait AudioBackend: Send + Sync {
    fn input_devices(&self) -> Result<Vec<AudioDeviceInfo>, String>;
    fn output_devices(&self) -> Result<Vec<AudioDeviceInfo>, String>;
    fn input_device_ref(&self, id: &str) -> Option<Arc<AudioDeviceReference>>;
    fn output_device_ref(&self, id: &str) -> Option<Arc<AudioDeviceReference>>;
    fn play_output(
        &self,
        output_device_id: Option<&str>,
        data: Arc<[u8]>,
    ) -> Result<VoicePlayback, String>;
}

impl Default for VoiceKeyer {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceKeyer {
    pub fn new() -> Self {
        Self::with_voicekeyer_dir(log73_paths::data_dir().join("voicekeyer"))
    }

    pub fn with_voicekeyer_dir(voicekeyer_dir: impl Into<PathBuf>) -> Self {
        Self::with_backend_and_voicekeyer_dir(
            Arc::new(RodioAudioBackend::default()),
            voicekeyer_dir.into(),
        )
    }

    #[allow(dead_code)]
    fn with_backend(audio: Arc<dyn AudioBackend>) -> Self {
        Self::with_backend_and_voicekeyer_dir(
            audio,
            std::env::temp_dir().join("log73-test-voicekeyer"),
        )
    }

    fn with_backend_and_voicekeyer_dir(
        audio: Arc<dyn AudioBackend>,
        voicekeyer_dir: PathBuf,
    ) -> Self {
        Self {
            audio,
            files: Arc::new(RwLock::new(HashMap::new())),
            voicekeyer_dir: Arc::new(voicekeyer_dir),
        }
    }

    #[allow(dead_code)]
    pub fn register_file(&self, file_name: impl AsRef<Path>, key: &str) -> Result<(), String> {
        let file_name_ref = file_name.as_ref();
        let bytes = fs::read(file_name_ref).map_err(|error| {
            format!(
                "failed to read voice keyer file '{}' : {error}",
                file_name_ref.display()
            )
        })?;
        self.register_bytes(file_name_ref.to_string_lossy(), key, bytes)
    }

    #[allow(dead_code)]
    pub fn register_bytes(
        &self,
        file_name: impl Into<String>,
        key: &str,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<(), String> {
        let key = normalize_voice_key(key)?;
        self.register_registry_bytes(file_name, key, bytes)
    }

    fn register_registry_file(
        &self,
        file_name: impl AsRef<Path>,
        registry_key: impl Into<String>,
    ) -> Result<(), String> {
        let file_name_ref = file_name.as_ref();
        let bytes = fs::read(file_name_ref).map_err(|error| {
            format!(
                "failed to read voice keyer file '{}' : {error}",
                file_name_ref.display()
            )
        })?;
        self.register_registry_bytes(file_name_ref.to_string_lossy(), registry_key, bytes)
    }

    fn register_registry_bytes(
        &self,
        file_name: impl Into<String>,
        registry_key: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<(), String> {
        let registry_key = registry_key.into();
        if registry_key.trim().is_empty() {
            return Err("voice keyer registry key is required".to_string());
        }
        let bytes = bytes.into();
        if bytes.is_empty() {
            return Err("voice keyer file data is empty".to_string());
        }

        let mut files = self
            .files
            .write()
            .map_err(|_| "voice keyer registry unavailable".to_string())?;
        files.insert(
            registry_key,
            RegisteredVoiceFileData {
                file_name: file_name.into(),
                bytes: Arc::<[u8]>::from(bytes),
            },
        );
        Ok(())
    }

    #[allow(dead_code)]
    pub fn unregister_key(&self, key: &str) -> Result<bool, String> {
        let key = normalize_voice_key(key)?;
        self.unregister_registry_key(&key)
    }

    fn unregister_registry_key(&self, registry_key: &str) -> Result<bool, String> {
        let mut files = self
            .files
            .write()
            .map_err(|_| "voice keyer registry unavailable".to_string())?;
        Ok(files.remove(registry_key).is_some())
    }

    pub fn validate_voice_messages(
        &self,
        config: &str,
    ) -> Result<voice_messages::VoiceLabels, String> {
        voice_messages::validate(config)
    }

    pub fn sync_radio_messages(&self, config: &RadioConfig) -> Result<(), String> {
        self.validate_voice_messages(&config.voice_messages)?;
        self.clear_radio_messages(config.id)?;

        let mut errors = Vec::new();
        for entry in voice_messages::entries(&config.voice_messages) {
            let Some(file_path) = entry.file_path.as_deref() else {
                continue;
            };
            if voice_messages::file_path_has_template(file_path) {
                continue;
            }
            let registry_key = voice_message_registry_key(config.id, &entry.mode, &entry.key)?;
            let path = voice_messages::voicekeyer_file_path(&self.voicekeyer_dir, file_path)?;
            if let Err(error) = self.register_registry_file(path, registry_key) {
                errors.push(format!("{} {}: {error}", entry.mode, entry.key));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    pub fn clear_radio_messages(&self, radio_id: i64) -> Result<(), String> {
        for mode in ["run", "s&p"] {
            for key_number in 1..=12 {
                let registry_key =
                    voice_message_registry_key(radio_id, mode, &format!("F{key_number}"))?;
                let _ = self.unregister_registry_key(&registry_key)?;
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn registered_files(&self) -> Result<Vec<RegisteredVoiceFile>, String> {
        let files = self
            .files
            .read()
            .map_err(|_| "voice keyer registry unavailable".to_string())?;
        let mut registered = files
            .iter()
            .map(|(key, data)| RegisteredVoiceFile {
                key: key.clone(),
                file_name: data.file_name.clone(),
                byte_len: data.bytes.len(),
            })
            .collect::<Vec<_>>();
        registered.sort_by(|left, right| {
            voice_key_number(&left.key)
                .unwrap_or(usize::MAX)
                .cmp(&voice_key_number(&right.key).unwrap_or(usize::MAX))
                .then_with(|| left.key.cmp(&right.key))
        });
        Ok(registered)
    }

    #[allow(dead_code)]
    pub fn play_key(
        &self,
        key: &str,
        output_device_id: Option<&str>,
    ) -> Result<VoicePlayback, String> {
        let key = normalize_voice_key(key)?;
        self.play_registered_key(&key, output_device_id)
    }

    pub fn play_registered_key(
        &self,
        registry_key: &str,
        output_device_id: Option<&str>,
    ) -> Result<VoicePlayback, String> {
        let registry_key = registry_key.trim();
        if registry_key.is_empty() {
            return Err("voice keyer registry key is required".to_string());
        }
        let data = {
            let files = self
                .files
                .read()
                .map_err(|_| "voice keyer registry unavailable".to_string())?;
            files
                .get(registry_key)
                .map(|file| file.bytes.clone())
                .ok_or_else(|| format!("no voice keyer file registered for {registry_key}"))?
        };
        self.audio
            .play_output(normalized_optional_id(output_device_id), data)
    }

    pub fn play_relative_path(
        &self,
        relative_path: &str,
        output_device_id: Option<&str>,
    ) -> Result<VoicePlayback, String> {
        let path = voice_messages::voicekeyer_file_path(&self.voicekeyer_dir, relative_path)?;
        let bytes = fs::read(&path).map_err(|error| {
            format!(
                "failed to read voice keyer file relative_path='{}' absolute_path='{}': {error}",
                relative_path.trim(),
                path.display()
            )
        })?;
        self.audio.play_output(
            normalized_optional_id(output_device_id),
            Arc::<[u8]>::from(bytes),
        )
    }

    pub fn input_devices(&self) -> Result<Vec<AudioDeviceInfo>, String> {
        self.audio.input_devices()
    }

    pub fn output_devices(&self) -> Result<Vec<AudioDeviceInfo>, String> {
        self.audio.output_devices()
    }

    pub fn input_device_ref(&self, id: &str) -> Option<Arc<AudioDeviceReference>> {
        self.audio.input_device_ref(id.trim())
    }

    pub fn output_device_ref(&self, id: &str) -> Option<Arc<AudioDeviceReference>> {
        self.audio.output_device_ref(id.trim())
    }

    pub fn sanitize_radio_config(&self, config: &mut RadioConfig) {
        config.voice_input_device_id = self.sanitized_device_id(
            AudioDeviceKind::Input,
            config.voice_input_device_id.as_deref(),
        );
        config.voice_output_device_id = self.sanitized_device_id(
            AudioDeviceKind::Output,
            config.voice_output_device_id.as_deref(),
        );
    }

    fn sanitized_device_id(&self, kind: AudioDeviceKind, value: Option<&str>) -> Option<String> {
        let id = normalized_optional_id(value)?;
        let exists = match kind {
            AudioDeviceKind::Input => self.input_device_ref(id).is_some(),
            AudioDeviceKind::Output => self.output_device_ref(id).is_some(),
        };
        exists.then(|| id.to_string())
    }
}

#[derive(Default)]
struct RodioAudioBackend {
    output_streams: Mutex<HashMap<String, Arc<rodio::MixerDeviceSink>>>,
}

struct RodioDeviceReference {
    _device: rodio::Device,
}

struct RodioPlaybackHandle {
    player: rodio::Player,
    _stream: Arc<rodio::MixerDeviceSink>,
}

impl PlaybackHandle for RodioPlaybackHandle {
    fn stop(&mut self) {
        self.player.stop();
    }
}

impl AudioBackend for RodioAudioBackend {
    fn input_devices(&self) -> Result<Vec<AudioDeviceInfo>, String> {
        list_devices(AudioDeviceKind::Input)
    }

    fn output_devices(&self) -> Result<Vec<AudioDeviceInfo>, String> {
        list_devices(AudioDeviceKind::Output)
    }

    fn input_device_ref(&self, id: &str) -> Option<Arc<AudioDeviceReference>> {
        let device = device_by_id(id)?;
        let info = self
            .input_devices()
            .ok()?
            .into_iter()
            .find(|device| device.id == id)?;
        Some(Arc::new(AudioDeviceReference {
            info,
            _handle: Arc::new(RodioDeviceReference { _device: device }),
        }))
    }

    fn output_device_ref(&self, id: &str) -> Option<Arc<AudioDeviceReference>> {
        let device = device_by_id(id)?;
        let info = self
            .output_devices()
            .ok()?
            .into_iter()
            .find(|device| device.id == id)?;
        Some(Arc::new(AudioDeviceReference {
            info,
            _handle: Arc::new(RodioDeviceReference { _device: device }),
        }))
    }

    fn play_output(
        &self,
        output_device_id: Option<&str>,
        data: Arc<[u8]>,
    ) -> Result<VoicePlayback, String> {
        let (stream_key, device) = output_device_for_id(output_device_id)?;
        let stream = self.output_stream(&stream_key, device)?;
        let cursor = Cursor::new(data);
        let source = rodio::Decoder::try_from(cursor)
            .map_err(|error| format!("failed to decode voice keyer audio: {error}"))?;
        let duration = source.total_duration().unwrap_or(Duration::ZERO);
        let player = rodio::Player::connect_new(stream.mixer());
        player.append(source);

        Ok(VoicePlayback::new(
            duration,
            Box::new(RodioPlaybackHandle {
                player,
                _stream: stream,
            }),
        ))
    }
}

impl RodioAudioBackend {
    fn output_stream(
        &self,
        stream_key: &str,
        device: rodio::Device,
    ) -> Result<Arc<rodio::MixerDeviceSink>, String> {
        let mut streams = self
            .output_streams
            .lock()
            .map_err(|_| "voice keyer output stream cache unavailable".to_string())?;
        if let Some(stream) = streams.get(stream_key) {
            return Ok(stream.clone());
        }

        let builder = rodio::DeviceSinkBuilder::from_device(device)
            .map_err(|error| format!("failed to configure voice keyer output device: {error}"))?;
        let mut stream = builder
            .open_stream()
            .map_err(|error| format!("failed to open voice keyer output device: {error}"))?;
        stream.log_on_drop(false);
        let stream = Arc::new(stream);
        streams.insert(stream_key.to_string(), stream.clone());
        Ok(stream)
    }
}

fn list_devices(kind: AudioDeviceKind) -> Result<Vec<AudioDeviceInfo>, String> {
    let mut devices = Vec::new();

    for host_id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(host_id) else {
            continue;
        };
        let default_id = default_device_id(&host, kind);
        let host_devices = match kind {
            AudioDeviceKind::Input => host.input_devices(),
            AudioDeviceKind::Output => host.output_devices(),
        };
        let Ok(host_devices) = host_devices else {
            continue;
        };

        for device in host_devices {
            if let Some(info) = device_info(host_id, &device, kind, default_id.as_deref()) {
                devices.push(info);
            }
        }
    }

    devices.sort_by(|left, right| {
        left.host
            .cmp(&right.host)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(devices)
}

fn default_device_id(host: &cpal::Host, kind: AudioDeviceKind) -> Option<String> {
    let device = match kind {
        AudioDeviceKind::Input => host.default_input_device(),
        AudioDeviceKind::Output => host.default_output_device(),
    }?;
    device.id().ok().map(|id| id.to_string())
}

fn device_info(
    host_id: cpal::HostId,
    device: &rodio::Device,
    kind: AudioDeviceKind,
    default_id: Option<&str>,
) -> Option<AudioDeviceInfo> {
    let id = device.id().ok()?.to_string();
    let description = device.description().ok();
    let name = description
        .as_ref()
        .map(|description| description.name().to_string())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| id.clone());
    let default_config = default_stream_config(device, kind);

    Some(AudioDeviceInfo {
        id: id.clone(),
        host: host_id.to_string(),
        name: name.clone(),
        description: description
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or(name),
        manufacturer: description
            .as_ref()
            .and_then(|description| description.manufacturer())
            .map(str::to_string),
        driver: description
            .as_ref()
            .and_then(|description| description.driver())
            .map(str::to_string),
        device_type: description
            .as_ref()
            .map(|description| description.device_type().to_string())
            .unwrap_or_else(|| "Unknown".to_string()),
        interface_type: description
            .as_ref()
            .map(|description| description.interface_type().to_string())
            .unwrap_or_else(|| "Unknown".to_string()),
        direction: description
            .as_ref()
            .map(|description| description.direction().to_string())
            .unwrap_or_else(|| kind.as_str().to_string()),
        address: description
            .as_ref()
            .and_then(|description| description.address())
            .map(str::to_string),
        extended: description
            .as_ref()
            .map(|description| description.extended().to_vec())
            .unwrap_or_default(),
        is_default: default_id == Some(id.as_str()),
        default_config,
    })
}

fn default_stream_config(
    device: &rodio::Device,
    kind: AudioDeviceKind,
) -> Option<AudioStreamConfig> {
    let config = match kind {
        AudioDeviceKind::Input => device.default_input_config(),
        AudioDeviceKind::Output => device.default_output_config(),
    }
    .ok()?;

    Some(AudioStreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        sample_format: format!("{:?}", config.sample_format()),
    })
}

fn device_by_id(id: &str) -> Option<rodio::Device> {
    let id = normalized_optional_id(Some(id))?;
    let device_id = id.parse::<cpal::DeviceId>().ok()?;
    let host = cpal::host_from_id(device_id.0).ok()?;
    host.device_by_id(&device_id)
}

fn output_device_for_id(output_device_id: Option<&str>) -> Result<(String, rodio::Device), String> {
    if let Some(id) = normalized_optional_id(output_device_id) {
        let device =
            device_by_id(id).ok_or_else(|| format!("output audio device not found: {id}"))?;
        return Ok((id.to_string(), device));
    }

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no default output audio device is available".to_string())?;
    let key = device
        .id()
        .ok()
        .map(|id| id.to_string())
        .unwrap_or_else(|| DEFAULT_OUTPUT_DEVICE_KEY.to_string());
    Ok((key, device))
}

pub fn normalize_voice_key(key: &str) -> Result<String, String> {
    let normalized = key.trim().to_uppercase();
    if voice_key_number(&normalized).is_some() {
        Ok(normalized)
    } else {
        Err("voice key must be F1 through F12".to_string())
    }
}

fn voice_message_registry_key(radio_id: i64, mode: &str, key: &str) -> Result<String, String> {
    let key = normalize_voice_key(key)?;
    Ok(format!(
        "radio:{radio_id}:{}:{key}",
        voice_messages::normalize_message_mode(mode)
    ))
}

fn voice_key_number(key: &str) -> Option<usize> {
    let number = key.trim().strip_prefix('F')?.parse::<usize>().ok()?;
    (1..=12).contains(&number).then_some(number)
}

fn normalized_optional_id(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::RadioConfig;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Barrier, Condvar};

    #[derive(Default)]
    struct FakeAudioBackend {
        inputs: Mutex<Vec<AudioDeviceInfo>>,
        outputs: Mutex<Vec<AudioDeviceInfo>>,
        plays: Mutex<Vec<FakePlay>>,
        plays_changed: Condvar,
        stop_count: Arc<AtomicUsize>,
        duration: Mutex<Duration>,
        fail_play: Mutex<Option<String>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct FakePlay {
        output_device_id: Option<String>,
        data: Vec<u8>,
        thread_id: thread::ThreadId,
    }

    struct FakePlaybackHandle {
        stop_count: Arc<AtomicUsize>,
    }

    impl PlaybackHandle for FakePlaybackHandle {
        fn stop(&mut self) {
            self.stop_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl AudioBackend for FakeAudioBackend {
        fn input_devices(&self) -> Result<Vec<AudioDeviceInfo>, String> {
            Ok(self.inputs.lock().unwrap().clone())
        }

        fn output_devices(&self) -> Result<Vec<AudioDeviceInfo>, String> {
            Ok(self.outputs.lock().unwrap().clone())
        }

        fn input_device_ref(&self, id: &str) -> Option<Arc<AudioDeviceReference>> {
            self.inputs
                .lock()
                .unwrap()
                .iter()
                .find(|device| device.id == id)
                .cloned()
                .map(fake_device_ref)
        }

        fn output_device_ref(&self, id: &str) -> Option<Arc<AudioDeviceReference>> {
            self.outputs
                .lock()
                .unwrap()
                .iter()
                .find(|device| device.id == id)
                .cloned()
                .map(fake_device_ref)
        }

        fn play_output(
            &self,
            output_device_id: Option<&str>,
            data: Arc<[u8]>,
        ) -> Result<VoicePlayback, String> {
            if let Some(error) = self.fail_play.lock().unwrap().clone() {
                return Err(error);
            }
            self.plays.lock().unwrap().push(FakePlay {
                output_device_id: output_device_id.map(str::to_string),
                data: data.to_vec(),
                thread_id: thread::current().id(),
            });
            self.plays_changed.notify_all();
            Ok(VoicePlayback::new(
                *self.duration.lock().unwrap(),
                Box::new(FakePlaybackHandle {
                    stop_count: self.stop_count.clone(),
                }),
            ))
        }
    }

    fn fake_device_ref(info: AudioDeviceInfo) -> Arc<AudioDeviceReference> {
        Arc::new(AudioDeviceReference {
            info,
            _handle: Arc::new(()),
        })
    }

    fn fake_device(
        id: &str,
        name: &str,
        kind: AudioDeviceKind,
        is_default: bool,
    ) -> AudioDeviceInfo {
        AudioDeviceInfo {
            id: id.to_string(),
            host: id.split(':').next().unwrap_or("test").to_string(),
            name: name.to_string(),
            description: format!("{name} description"),
            manufacturer: Some("Acme Audio".to_string()),
            driver: Some("test-driver".to_string()),
            device_type: match kind {
                AudioDeviceKind::Input => "Microphone".to_string(),
                AudioDeviceKind::Output => "Speaker".to_string(),
            },
            interface_type: "Usb".to_string(),
            direction: kind.as_str().to_string(),
            address: Some(format!("addr-{id}")),
            extended: vec![format!("extended-{id}")],
            is_default,
            default_config: Some(AudioStreamConfig {
                channels: 2,
                sample_rate: 48_000,
                sample_format: "F32".to_string(),
            }),
        }
    }

    fn test_keyer() -> (VoiceKeyer, Arc<FakeAudioBackend>) {
        let backend = Arc::new(FakeAudioBackend::default());
        *backend.duration.lock().unwrap() = Duration::from_millis(123);
        (VoiceKeyer::with_backend(backend.clone()), backend)
    }

    fn wait_for_play_count(backend: &Arc<FakeAudioBackend>, count: usize) -> Vec<FakePlay> {
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let mut plays = backend.plays.lock().unwrap();
        while plays.len() < count {
            let now = std::time::Instant::now();
            if now >= deadline {
                break;
            }
            let timeout = deadline.saturating_duration_since(now);
            let (next_plays, _) = backend
                .plays_changed
                .wait_timeout(plays, timeout)
                .expect("play wait should not poison");
            plays = next_plays;
        }
        plays.clone()
    }

    fn test_radio_config() -> RadioConfig {
        RadioConfig {
            id: 1,
            name: "Test".to_string(),
            radio_kind: "k4".to_string(),
            transport_kind: "tcp".to_string(),
            tcp_host: "127.0.0.1".to_string(),
            tcp_port: 5002,
            serial_port: String::new(),
            serial_baud_rate: 115_200,
            options: String::new(),
            cw_tuning_increment_hz: 20,
            ssb_tuning_increment_hz: 100,
            rit_clear_on_log: false,
            voice_input_device_id: None,
            voice_output_device_id: None,
            cw_keyer_type: "none".to_string(),
            winkeyer_serial_port: String::new(),
            cw_serial_port: String::new(),
            cw_serial_baud_rate: 9_600,
            cw_serial_line: "dtr".to_string(),
            cw_messages: String::new(),
            voice_messages: crate::voice_messages::DEFAULT_VOICE_MESSAGES.to_string(),
        }
    }

    #[test]
    fn normalizes_function_keys_case_and_whitespace() {
        assert_eq!(normalize_voice_key(" f1 "), Ok("F1".to_string()));
        assert_eq!(normalize_voice_key("f12"), Ok("F12".to_string()));
        assert!(normalize_voice_key("F0").is_err());
        assert!(normalize_voice_key("F13").is_err());
        assert!(normalize_voice_key("CQ").is_err());
    }

    #[test]
    fn register_bytes_stores_file_data_by_normalized_key() {
        let (keyer, _) = test_keyer();

        keyer
            .register_bytes("NG4M_F1.wav", " f1 ", b"audio-one".to_vec())
            .expect("bytes register");

        let registered = keyer.registered_files().expect("registry lists");
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].key, "F1");
        assert_eq!(registered[0].file_name, "NG4M_F1.wav");
        assert_eq!(registered[0].byte_len, b"audio-one".len());
    }

    #[test]
    fn register_file_loads_data_into_memory_immediately() {
        let (keyer, backend) = test_keyer();
        let path = std::env::temp_dir().join(format!(
            "log73-voicekeyer-{}-memory.wav",
            std::process::id()
        ));
        fs::write(&path, b"original-data").expect("write original temp file");

        keyer.register_file(&path, "F2").expect("file registers");
        fs::write(&path, b"changed-on-disk").expect("rewrite temp file");
        let _ = fs::remove_file(&path);

        keyer.play_key("F2", None).expect("registered key plays");

        let plays = backend.plays.lock().unwrap();
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].data, b"original-data");
    }

    #[test]
    fn register_bytes_replaces_existing_key_without_duplicating_registry_entries() {
        let (keyer, backend) = test_keyer();

        keyer
            .register_bytes("first.wav", "F3", b"first".to_vec())
            .expect("first registers");
        keyer
            .register_bytes("second.wav", "f3", b"second".to_vec())
            .expect("second replaces first");
        keyer.play_key("F3", None).expect("registered key plays");

        let registered = keyer.registered_files().expect("registry lists");
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].file_name, "second.wav");
        assert_eq!(registered[0].byte_len, b"second".len());
        assert_eq!(backend.plays.lock().unwrap()[0].data, b"second");
    }

    #[test]
    fn registered_files_are_sorted_by_function_key_number() {
        let (keyer, _) = test_keyer();

        keyer
            .register_bytes("ten.wav", "F10", b"ten".to_vec())
            .expect("F10 registers");
        keyer
            .register_bytes("two.wav", "F2", b"two".to_vec())
            .expect("F2 registers");
        keyer
            .register_bytes("one.wav", "F1", b"one".to_vec())
            .expect("F1 registers");

        let keys = keyer
            .registered_files()
            .expect("registry lists")
            .into_iter()
            .map(|file| file.key)
            .collect::<Vec<_>>();
        assert_eq!(keys, vec!["F1", "F2", "F10"]);
    }

    #[test]
    fn play_key_uses_selected_output_device_id_and_returns_duration() {
        let (keyer, backend) = test_keyer();
        keyer
            .register_bytes("msg.wav", "F4", b"message".to_vec())
            .expect("bytes register");

        let playback = keyer
            .play_key(" f4 ", Some(" test-output "))
            .expect("registered key plays");

        assert_eq!(playback.duration(), Duration::from_millis(123));
        let plays = backend.plays.lock().unwrap();
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].output_device_id.as_deref(), Some("test-output"));
        assert_eq!(plays[0].data, b"message");
    }

    #[test]
    fn play_key_rejects_unregistered_keys_without_touching_audio_backend() {
        let (keyer, backend) = test_keyer();

        let error = keyer
            .play_key("F5", None)
            .expect_err("missing key should fail");

        assert!(error.contains("F5"));
        assert!(backend.plays.lock().unwrap().is_empty());
    }

    #[test]
    fn unregister_key_removes_registered_audio() {
        let (keyer, _) = test_keyer();
        keyer
            .register_bytes("msg.wav", "F6", b"message".to_vec())
            .expect("bytes register");

        assert!(keyer.unregister_key("f6").expect("unregister succeeds"));
        assert!(!keyer.unregister_key("F6").expect("unregister succeeds"));
        assert!(keyer.play_key("F6", None).is_err());
    }

    #[test]
    fn sync_radio_messages_registers_and_unregisters_scoped_files() {
        let voicekeyer_dir =
            std::env::temp_dir().join(format!("log73-voicekeyer-{}-sync", std::process::id()));
        let _ = fs::remove_dir_all(&voicekeyer_dir);
        fs::create_dir_all(voicekeyer_dir.join("operator1")).expect("voicekeyer dir creates");
        fs::write(voicekeyer_dir.join("operator1/run.wav"), b"run-audio").expect("run file writes");
        fs::write(voicekeyer_dir.join("operator1/sp.wav"), b"sp-audio").expect("sp file writes");
        let backend = Arc::new(FakeAudioBackend::default());
        let keyer =
            VoiceKeyer::with_backend_and_voicekeyer_dir(backend.clone(), voicekeyer_dir.clone());
        let mut config = test_radio_config();
        config.id = 73;
        config.voice_messages = r#"
# RUN Messages
F1 CQ,operator1/run.wav
F2 -,
# S&P Messages
F1 QRL,operator1/sp.wav
"#
        .to_string();

        keyer
            .sync_radio_messages(&config)
            .expect("voice messages sync");

        let run_key = voice_message_registry_key(73, "run", "F1").expect("run key builds");
        let empty_key = voice_message_registry_key(73, "run", "F2").expect("empty key builds");
        let sp_key = voice_message_registry_key(73, "s&p", "F1").expect("s&p key builds");
        let registered_keys = keyer
            .registered_files()
            .expect("registry lists")
            .into_iter()
            .map(|file| file.key)
            .collect::<Vec<_>>();
        assert!(registered_keys.contains(&run_key));
        assert!(registered_keys.contains(&sp_key));
        assert!(!registered_keys.contains(&empty_key));

        keyer
            .play_registered_key(&run_key, None)
            .expect("registered run message plays");
        assert_eq!(backend.plays.lock().unwrap()[0].data, b"run-audio");

        config.voice_messages = r#"
# RUN Messages
F1 -,
# S&P Messages
F1 QRL,operator1/sp.wav
"#
        .to_string();
        keyer
            .sync_radio_messages(&config)
            .expect("voice messages resync");
        let registered_keys = keyer
            .registered_files()
            .expect("registry lists")
            .into_iter()
            .map(|file| file.key)
            .collect::<Vec<_>>();
        assert!(!registered_keys.contains(&run_key));
        assert!(registered_keys.contains(&sp_key));

        let _ = fs::remove_dir_all(&voicekeyer_dir);
    }

    #[test]
    fn sync_radio_messages_skips_template_paths_until_playback() {
        let voicekeyer_dir =
            std::env::temp_dir().join(format!("log73-voicekeyer-{}-template", std::process::id()));
        let _ = fs::remove_dir_all(&voicekeyer_dir);
        fs::create_dir_all(voicekeyer_dir.join("operator1")).expect("voicekeyer dir creates");
        fs::write(voicekeyer_dir.join("operator1/run.wav"), b"run-audio").expect("run file writes");
        let backend = Arc::new(FakeAudioBackend::default());
        let keyer =
            VoiceKeyer::with_backend_and_voicekeyer_dir(backend.clone(), voicekeyer_dir.clone());
        let mut config = test_radio_config();
        config.id = 74;
        config.voice_messages = r#"
# RUN Messages
F1 CQ,{OPERATOR}/run.wav
# S&P Messages
F1 QRL,operator1/run.wav
"#
        .to_string();

        keyer
            .sync_radio_messages(&config)
            .expect("voice messages sync");

        let run_key = voice_message_registry_key(74, "run", "F1").expect("run key builds");
        let sp_key = voice_message_registry_key(74, "s&p", "F1").expect("s&p key builds");
        let registered_keys = keyer
            .registered_files()
            .expect("registry lists")
            .into_iter()
            .map(|file| file.key)
            .collect::<Vec<_>>();
        assert!(!registered_keys.contains(&run_key));
        assert!(registered_keys.contains(&sp_key));

        keyer
            .play_relative_path("operator1/run.wav", None)
            .expect("template-backed file plays");
        assert_eq!(backend.plays.lock().unwrap()[0].data, b"run-audio");

        let _ = fs::remove_dir_all(&voicekeyer_dir);
    }

    #[test]
    fn device_lists_are_separate_and_preserve_specific_metadata() {
        let (keyer, backend) = test_keyer();
        backend.inputs.lock().unwrap().push(fake_device(
            "test:mic-1",
            "USB Microphone",
            AudioDeviceKind::Input,
            true,
        ));
        backend.outputs.lock().unwrap().push(fake_device(
            "test:speaker-1",
            "USB Speaker",
            AudioDeviceKind::Output,
            false,
        ));

        let inputs = keyer.input_devices().expect("inputs list");
        let outputs = keyer.output_devices().expect("outputs list");

        assert_eq!(inputs.len(), 1);
        assert_eq!(outputs.len(), 1);
        assert_eq!(inputs[0].id, "test:mic-1");
        assert_eq!(inputs[0].name, "USB Microphone");
        assert_eq!(inputs[0].device_type, "Microphone");
        assert!(inputs[0].is_default);
        assert_eq!(outputs[0].id, "test:speaker-1");
        assert_eq!(outputs[0].device_type, "Speaker");
        assert_eq!(
            outputs[0].default_config.as_ref().unwrap().sample_rate,
            48_000
        );
    }

    #[test]
    fn device_ref_returns_arc_reference_for_known_device_only() {
        let (keyer, backend) = test_keyer();
        backend.outputs.lock().unwrap().push(fake_device(
            "test:speaker-2",
            "Line Out",
            AudioDeviceKind::Output,
            true,
        ));

        let reference = keyer
            .output_device_ref("test:speaker-2")
            .expect("device reference exists");

        assert_eq!(reference.info.id, "test:speaker-2");
        assert_eq!(Arc::strong_count(&reference), 1);
        assert!(keyer.output_device_ref("test:missing").is_none());
        assert!(keyer.input_device_ref("test:speaker-2").is_none());
    }

    #[test]
    fn sanitize_radio_config_clears_missing_or_blank_devices_and_keeps_existing_devices() {
        let (keyer, backend) = test_keyer();
        backend.inputs.lock().unwrap().push(fake_device(
            "test:mic-3",
            "Valid Mic",
            AudioDeviceKind::Input,
            false,
        ));
        backend.outputs.lock().unwrap().push(fake_device(
            "test:speaker-3",
            "Valid Speaker",
            AudioDeviceKind::Output,
            false,
        ));
        let mut valid = test_radio_config();
        valid.voice_input_device_id = Some(" test:mic-3 ".to_string());
        valid.voice_output_device_id = Some("test:speaker-3".to_string());
        let mut missing = test_radio_config();
        missing.voice_input_device_id = Some("".to_string());
        missing.voice_output_device_id = Some("test:missing".to_string());

        keyer.sanitize_radio_config(&mut valid);
        keyer.sanitize_radio_config(&mut missing);

        assert_eq!(valid.voice_input_device_id.as_deref(), Some("test:mic-3"));
        assert_eq!(
            valid.voice_output_device_id.as_deref(),
            Some("test:speaker-3")
        );
        assert_eq!(missing.voice_input_device_id, None);
        assert_eq!(missing.voice_output_device_id, None);
    }

    #[test]
    fn voice_playback_stop_delegates_to_backend_handle() {
        let (keyer, backend) = test_keyer();
        keyer
            .register_bytes("msg.wav", "F7", b"message".to_vec())
            .expect("bytes register");
        let mut playback = keyer.play_key("F7", None).expect("registered key plays");

        playback.stop();
        playback.stop();

        assert_eq!(backend.stop_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn concurrent_registry_reads_allow_many_play_key_callers() {
        let (keyer, backend) = test_keyer();
        keyer
            .register_bytes("msg.wav", "F8", b"parallel".to_vec())
            .expect("bytes register");
        let caller_count = 8;
        let barrier = Arc::new(Barrier::new(caller_count));
        let mut threads = Vec::new();

        for _ in 0..caller_count {
            let keyer = keyer.clone();
            let barrier = barrier.clone();
            threads.push(thread::spawn(move || {
                barrier.wait();
                keyer.play_key("F8", None).expect("registered key plays");
            }));
        }

        for thread in threads {
            thread.join().expect("caller thread joins");
        }

        let plays = backend.plays.lock().unwrap();
        assert_eq!(plays.len(), caller_count);
        assert!(plays.iter().all(|play| play.data == b"parallel"));
    }

    #[tokio::test]
    async fn voice_playback_thread_runs_playback_on_worker_thread() {
        let (keyer, backend) = test_keyer();
        *backend.duration.lock().unwrap() = Duration::from_millis(10);
        keyer
            .register_bytes("msg.wav", "F9", b"threaded".to_vec())
            .expect("bytes register");
        let caller_thread_id = thread::current().id();
        let mut worker = VoicePlaybackThread::spawn(73, keyer, Some("test-output".to_string()))
            .expect("voice worker starts");

        let completed = worker.play_key("F9").expect("play command queues");
        let plays = wait_for_play_count(&backend, 1);
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].data, b"threaded");
        assert_eq!(plays[0].output_device_id.as_deref(), Some("test-output"));
        assert_ne!(plays[0].thread_id, caller_thread_id);
        assert_eq!(completed.await.unwrap().unwrap(), Duration::from_millis(10));

        worker.shutdown();
    }

    #[tokio::test]
    async fn separate_voice_playback_threads_can_play_simultaneously() {
        let (keyer, backend) = test_keyer();
        *backend.duration.lock().unwrap() = Duration::from_millis(100);
        keyer
            .register_bytes("msg.wav", "F10", b"simultaneous".to_vec())
            .expect("bytes register");
        let caller_thread_id = thread::current().id();
        let mut first =
            VoicePlaybackThread::spawn(1, keyer.clone(), None).expect("first voice worker starts");
        let mut second =
            VoicePlaybackThread::spawn(2, keyer, None).expect("second voice worker starts");

        let first_done = first.play_key("F10").expect("first play queues");
        let second_done = second.play_key("F10").expect("second play queues");
        let plays = wait_for_play_count(&backend, 2);

        assert_eq!(plays.len(), 2);
        assert_ne!(plays[0].thread_id, caller_thread_id);
        assert_ne!(plays[1].thread_id, caller_thread_id);
        assert_ne!(plays[0].thread_id, plays[1].thread_id);
        assert_eq!(
            first_done.await.unwrap().unwrap(),
            Duration::from_millis(100)
        );
        assert_eq!(
            second_done.await.unwrap().unwrap(),
            Duration::from_millis(100)
        );

        first.shutdown();
        second.shutdown();
    }

    #[tokio::test]
    async fn voice_playback_thread_stop_interrupts_active_playback() {
        let (keyer, backend) = test_keyer();
        *backend.duration.lock().unwrap() = Duration::from_secs(5);
        keyer
            .register_bytes("msg.wav", "F11", b"long".to_vec())
            .expect("bytes register");
        let mut worker = VoicePlaybackThread::spawn(11, keyer, None).expect("voice worker starts");

        let completed = worker.play_key("F11").expect("play command queues");
        assert_eq!(wait_for_play_count(&backend, 1).len(), 1);
        worker.stop_keying();

        assert_eq!(completed.await.unwrap(), Err("keying stopped".to_string()));
        assert_eq!(backend.stop_count.load(Ordering::SeqCst), 1);

        worker.shutdown();
    }
}
