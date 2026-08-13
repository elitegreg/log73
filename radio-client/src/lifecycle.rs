use crate::{AppPaths, settings};
use axum::{Extension, Router, routing::get};
use radio_io::voice_keyer::VoiceKeyer;
use radio_io::{Band, BandCatalog, RadioConfig, RadioManager, SingleRadioWebSocketState};
use serde::Deserialize;
use std::time::Duration;
use tokio::{sync::oneshot, task::JoinHandle, time::timeout};

const LOCAL_RADIO_ID: i64 = 1;
const STOP_TIMEOUT: Duration = Duration::from_secs(3);
const REGION_TWO_BANDS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/bands_2.csv"));

pub struct RunningHost {
    pub websocket_url: String,
    shutdown: Option<oneshot::Sender<()>>,
    server: JoinHandle<()>,
    radio: SingleRadioWebSocketState,
}

pub struct StartResult {
    pub host: RunningHost,
    pub events: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BandRow {
    band: String,
    start: String,
    end: String,
    cabrillo: String,
}

pub async fn start(paths: AppPaths) -> Result<StartResult, String> {
    paths.ensure_directories()?;
    let mut client_settings = settings::load_or_create(&paths.settings_file)?;
    radio_io::normalize_radio_settings(&mut client_settings.radio)?;
    radio_io::validate_radio_settings(&client_settings.radio)?;

    let mut events = configured_hardware_warnings(&client_settings.radio, &paths.voicekeyer_dir);
    let voice_keyer = VoiceKeyer::with_voicekeyer_dir(&paths.voicekeyer_dir);
    voice_keyer.validate_radio_voice_messages(&client_settings.radio.voice_messages)?;
    voice_keyer.load_voice_assets()?;
    events.push("Loaded local voice assets into memory.".to_string());

    let bands = BandCatalog::new(region_two_bands()?);
    let radio_manager = RadioManager::new(voice_keyer, bands);
    let radio = SingleRadioWebSocketState::new(
        radio_manager,
        RadioConfig::new(LOCAL_RADIO_ID, client_settings.radio),
    );
    let app = Router::new()
        .route("/radiows", get(radio_io::single_radio_ws_handler))
        .layer(Extension(radio.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| format!("failed to bind local radio WebSocket: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("failed to read local radio WebSocket address: {error}"))?;
    let (shutdown, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });
    let websocket_url = format!("ws://{address}/radiows");
    events.push(format!(
        "Local radio WebSocket listening at {websocket_url}."
    ));
    Ok(StartResult {
        host: RunningHost {
            websocket_url,
            shutdown: Some(shutdown),
            server,
            radio,
        },
        events,
    })
}

pub async fn stop(mut host: RunningHost) -> Vec<String> {
    let mut events = vec!["Stopping local radio host…".to_string()];
    if let Some(shutdown) = host.shutdown.take() {
        let _ = shutdown.send(());
    }
    match timeout(STOP_TIMEOUT, &mut host.server).await {
        Ok(_) => events.push("Local radio WebSocket stopped accepting connections.".to_string()),
        Err(_) => {
            host.server.abort();
            let _ = host.server.await;
            events.push(
                "Local radio WebSocket shutdown timed out; cancelled remaining connections."
                    .to_string(),
            );
        }
    }
    host.radio.shutdown().await;
    events.push("Local radio runtime and keying resources stopped.".to_string());
    events
}

fn configured_hardware_warnings(
    settings: &radio_io::RadioSettings,
    voicekeyer_dir: &std::path::Path,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let ports = radio_io::list_serial_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|port| port.name)
        .collect::<Vec<_>>();
    for (label, port) in [
        ("CAT serial", settings.serial_port.as_str()),
        ("Winkeyer", settings.winkeyer_serial_port.as_str()),
        ("CW serial", settings.cw_serial_port.as_str()),
    ] {
        if !port.trim().is_empty() && !ports.iter().any(|available| available == port.trim()) {
            warnings.push(format!(
                "{label} port '{port}' is unavailable; the radio will retry when it returns."
            ));
        }
    }
    let voice_keyer = VoiceKeyer::with_voicekeyer_dir(voicekeyer_dir);
    for (label, selected, devices) in [
        (
            "Voice input",
            settings.voice_input_device_id.as_deref(),
            voice_keyer.input_devices(),
        ),
        (
            "Voice output",
            settings.voice_output_device_id.as_deref(),
            voice_keyer.output_devices(),
        ),
    ] {
        if let (Some(selected), Ok(devices)) = (selected, devices)
            && !devices.iter().any(|device| device.id == selected)
        {
            warnings.push(format!("{label} device '{selected}' is unavailable."));
        }
    }
    warnings
}

fn region_two_bands() -> Result<Vec<Band>, String> {
    let mut reader = csv::Reader::from_reader(REGION_TWO_BANDS.as_bytes());
    reader
        .deserialize::<BandRow>()
        .enumerate()
        .map(|(index, row)| {
            let row = row.map_err(|error| {
                format!("invalid bundled Region 2 band row {}: {error}", index + 2)
            })?;
            let lower_hz = decimal_khz_to_hz(&row.start)?;
            let upper_hz = decimal_khz_to_hz(&row.end)?;
            Ok(Band {
                iaru_region: 2,
                name: row.band,
                lower_hz,
                upper_hz,
                default_ssb_mode: if lower_hz < 10_000_000 { "LSB" } else { "USB" }.to_string(),
                sort_order: i64::try_from(index + 1).unwrap_or(i64::MAX),
                cabrillo: row.cabrillo,
            })
        })
        .collect()
}

fn decimal_khz_to_hz(value: &str) -> Result<i64, String> {
    let value = value.trim();
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    let whole = whole
        .parse::<i64>()
        .map_err(|_| format!("invalid band frequency {value}"))?;
    let fraction = format!("{fraction:0<3}");
    let fraction = fraction
        .get(..3)
        .unwrap_or_default()
        .parse::<i64>()
        .map_err(|_| format!("invalid band frequency {value}"))?;
    whole
        .checked_mul(1_000)
        .and_then(|hz| hz.checked_add(fraction))
        .ok_or_else(|| format!("invalid band frequency {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{RadioClientSettings, save_atomic, settings_file_path};
    use std::{fs, path::PathBuf};

    #[test]
    fn bundled_region_two_catalog_parses() {
        let bands = region_two_bands().unwrap();
        assert!(bands.iter().any(|band| band.name == "20M"));
        assert!(bands.iter().all(|band| band.lower_hz < band.upper_hz));
    }

    #[test]
    fn decimal_khz_conversion_preserves_thousandths() {
        assert_eq!(decimal_khz_to_hz("14.074").unwrap(), 14_074);
    }

    #[test]
    fn unavailable_serial_hardware_is_reported_without_blocking_startup() {
        let settings = radio_io::RadioSettings {
            serial_port: "definitely-not-a-local-serial-port".to_string(),
            ..radio_io::RadioSettings::default()
        };
        let warnings = configured_hardware_warnings(&settings, std::path::Path::new("voicekeyer"));
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("CAT serial"))
        );
    }

    #[test]
    fn dummy_radio_host_binds_and_stops() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let root = std::env::temp_dir()
                .join(format!("log73-radio-client-host-{}", uuid::Uuid::new_v4()));
            let config_dir = root.join("config");
            let data_dir = root.join("data");
            let settings_path = settings_file_path(&config_dir);
            let mut settings = RadioClientSettings::default();
            settings.radio.name = "Dummy".to_string();
            settings.radio.radio_kind = "dummy".to_string();
            settings.radio.transport_kind = "none".to_string();
            save_atomic(&settings_path, &settings).unwrap();
            let paths = AppPaths {
                config_dir,
                data_dir: data_dir.clone(),
                app_dir: PathBuf::new(),
                settings_file: settings_path,
                voicekeyer_dir: data_dir.join("voicekeyer"),
                log_file: root.join("radio-client.log"),
            };

            let started = match start(paths).await {
                Ok(started) => started,
                Err(error) if error.contains("Operation not permitted") => return,
                Err(error) => panic!("dummy radio host starts: {error}"),
            };
            let address = started
                .host
                .websocket_url
                .trim_start_matches("ws://")
                .trim_end_matches("/radiows");
            assert!(tokio::net::TcpStream::connect(address).await.is_ok());
            let events = stop(started.host).await;
            assert!(events.iter().any(|event| event.contains("runtime")));
            let _ = fs::remove_dir_all(root);
        });
    }
}
