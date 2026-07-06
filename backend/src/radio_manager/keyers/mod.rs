mod cat;
mod serial;
mod winkeyer;

use super::cw_task::CwKeyer;
use crate::cat_keyer::CatKeyer;
use crate::db::RadioConfig;
use radio_cat_rs::Radio;

use serial::SerialLineKeyer;
pub(super) use serial::{CwSerialDevice, open_serial_keyer};
use winkeyer::WinkeyerKeyer;

pub(super) async fn cw_keyer_for_config(
    config: &RadioConfig,
    radio: Radio,
    shared_cw_serial_keyer: Option<CwSerialDevice>,
) -> Option<Box<dyn CwKeyer>> {
    match config.cw_keyer_type.trim().to_ascii_lowercase().as_str() {
        "winkeyer" => Some(Box::new(WinkeyerKeyer {
            serial_port: config.winkeyer_serial_port.clone(),
            device: None,
        })),
        "cat" => Some(Box::new(CatKeyer::open(config.id, radio).await)),
        "serial" => Some(Box::new(SerialLineKeyer {
            serial_port: config.cw_serial_port.clone(),
            baud_rate: config.cw_serial_baud_rate,
            line: config.cw_serial_line.clone(),
            device: shared_cw_serial_keyer,
        })),
        _ => None,
    }
}
