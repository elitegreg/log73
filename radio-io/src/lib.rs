mod bands;
mod cat_keyer;
mod config;
mod digital_io;
mod flrig;
mod message_mode;
mod messages;
mod radio;
mod radio_manager;
mod websocket;
mod wsjtx;

pub mod cw;
pub mod modes;
pub mod voice_keyer;
pub mod voice_messages;

pub use bands::{Band, BandCatalog, band_for_frequency};
pub use config::{
    ConfiguredRadio, DEFAULT_CW_SERIAL_BAUD_RATE, DEFAULT_CW_SERIAL_LINE,
    DEFAULT_CW_TUNING_INCREMENT_HZ, DEFAULT_FLDIGI_HOST, DEFAULT_FLDIGI_PORT, DEFAULT_FLRIG_PORT,
    DEFAULT_SSB_TUNING_INCREMENT_HZ, DEFAULT_WSJTX_BIND_ADDRESS, DEFAULT_WSJTX_PORT, RadioConfig,
    RadioIoConfig, RadioSettings, normalize_radio_settings, validate_cw_messages,
    validate_radio_settings, validate_voice_messages,
};
pub use digital_io::{DigitalIoEvent, DigitalIoManager, DigitalIoTargetState};
pub use ham_radio_digital_interfacing::fldigi::{
    DEFAULT_FLDIGI_ENDPOINT, DEFAULT_FLDIGI_POLL_INTERVAL, DEFAULT_FLDIGI_REQUEST_TIMEOUT,
    FldigiCommand, FldigiConfig, FldigiError, FldigiInterface,
};
pub use message_mode::is_valid_message_mode;
pub use radio::{
    RadioClientMessage, RadioCommand, RadioServerMessage, RadioState, RadioStatus,
    logger_mode_from_cat_mode, mode_candidates_for_request, mode_is_phone, normalize_mode,
};
pub use radio_cat_rs::{Mode, list_serial_ports, supported_drivers};
pub use radio_manager::{RadioHandle, RadioManager};
pub use websocket::{
    AcquiredRadio, RadioAcquireError, RadioMutationError, RadioMutationPermit, RadioWebSocketState,
    SingleRadioWebSocketState, radio_ws_handler, single_radio_ws_handler,
};
pub use wsjtx::{WsjtXEvent, WsjtXManager, WsjtXTargetState};
