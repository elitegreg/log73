use super::super::cw_task::{CwKeyer, CwKeyerStatus, CwSendCompletion};
use cw_serial_keyer::{Config as CwSerialConfig, ControlLine, SerialKeyer};
use futures_util::future::{BoxFuture, FutureExt};
use std::time::Duration;
use tracing::{info, warn};

pub(crate) type CwSerialDevice = SerialKeyer;

pub(super) struct SerialLineKeyer {
    pub(super) serial_port: String,
    pub(super) baud_rate: u32,
    pub(super) line: String,
    pub(super) device: Option<CwSerialDevice>,
}

impl CwKeyer for SerialLineKeyer {
    fn name(&self) -> &'static str {
        "Serial"
    }

    fn connect<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()> {
        async move {
            connect_serial_keyer(radio_id, self).await;
        }
        .boxed()
    }

    fn send_text<'a>(
        &'a mut self,
        radio_id: i64,
        text: &'a str,
    ) -> BoxFuture<'a, Result<CwSendCompletion, String>> {
        async move {
            let result = {
                let Some(serial_keyer) = ensure_serial_keyer_connected(radio_id, self).await else {
                    return Err("serial CW keyer unavailable".to_string());
                };
                serial_keyer.send_text(text).await
            };
            if let Err(error) = result {
                warn!(radio_id, %error, "failed to send cw text through serial keyer");
                self.device = None;
                return Err(error.to_string());
            }
            Ok(CwSendCompletion::PollStatus {
                wait_for_busy: false,
            })
        }
        .boxed()
    }

    fn status<'a>(
        &'a mut self,
        radio_id: i64,
    ) -> BoxFuture<'a, Result<Option<CwKeyerStatus>, String>> {
        async move {
            let result = {
                let Some(serial_keyer) = ensure_serial_keyer_connected(radio_id, self).await else {
                    return Err("serial CW keyer unavailable".to_string());
                };
                serial_keyer.status().await
            };
            match result {
                Ok(status) => Ok(Some(CwKeyerStatus {
                    busy: status.busy
                        || status.key_down
                        || status.ptt_on
                        || status.queued_messages > 0,
                })),
                Err(error) => {
                    warn!(radio_id, %error, "failed to read serial CW keyer status");
                    self.device = None;
                    Err(error.to_string())
                }
            }
        }
        .boxed()
    }

    fn clear_buffer<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, Result<(), String>> {
        async move {
            let result = {
                let Some(serial_keyer) = ensure_serial_keyer_connected(radio_id, self).await else {
                    return Err("serial CW keyer unavailable".to_string());
                };
                serial_keyer.clear_buffer().await
            };
            if let Err(error) = result {
                self.device = None;
                return Err(error.to_string());
            }
            Ok(())
        }
        .boxed()
    }

    fn set_wpm<'a>(&'a mut self, radio_id: i64, wpm: u8) -> BoxFuture<'a, Result<(), String>> {
        async move {
            let result = {
                let Some(serial_keyer) = ensure_serial_keyer_connected(radio_id, self).await else {
                    return Err("serial CW keyer unavailable".to_string());
                };
                serial_keyer.set_wpm(wpm).await
            };
            if let Err(error) = result {
                self.device = None;
                return Err(error.to_string());
            }
            Ok(())
        }
        .boxed()
    }

    fn close<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()> {
        async move {
            if let Some(mut serial_keyer) = self.device.take()
                && let Err(error) = serial_keyer.close().await
            {
                warn!(radio_id, %error, "failed to close serial CW keyer");
            }
        }
        .boxed()
    }
}

fn cw_serial_control_line(line: &str) -> ControlLine {
    match line.trim().to_ascii_lowercase().as_str() {
        "rts" => ControlLine::Rts,
        _ => ControlLine::Dtr,
    }
}

fn cw_serial_config(serial_port: &str, baud_rate: u32, line: &str) -> CwSerialConfig {
    CwSerialConfig::new(serial_port)
        .baud_rate(baud_rate)
        .key_line(cw_serial_control_line(line))
        .ptt_line(None)
}

pub(crate) async fn open_serial_keyer(
    serial_port: &str,
    baud_rate: u32,
    line: &str,
) -> cw_serial_keyer::Result<CwSerialDevice> {
    let config = cw_serial_config(serial_port, baud_rate, line);
    let mut serial_keyer = CwSerialDevice::open_with_config(config).await?;
    serial_keyer.set_timeout(Duration::from_millis(500));
    Ok(serial_keyer)
}

async fn connect_serial_keyer(radio_id: i64, keyer: &mut SerialLineKeyer) {
    if keyer.serial_port.trim().is_empty() {
        warn!(radio_id, "Serial CW keying selected without a serial port");
        return;
    }
    if keyer.device.is_some() {
        return;
    }

    match open_serial_keyer(&keyer.serial_port, keyer.baud_rate, &keyer.line).await {
        Ok(serial_keyer) => {
            info!(
                radio_id,
                serial_port = %keyer.serial_port,
                baud_rate = keyer.baud_rate,
                line = %keyer.line,
                "connected to serial CW keyer"
            );
            keyer.device = Some(serial_keyer);
        }
        Err(error) => {
            warn!(
                radio_id,
                serial_port = %keyer.serial_port,
                baud_rate = keyer.baud_rate,
                line = %keyer.line,
                %error,
                "failed to connect to serial CW keyer"
            );
            keyer.device = None;
        }
    }
}

async fn ensure_serial_keyer_connected(
    radio_id: i64,
    keyer: &mut SerialLineKeyer,
) -> Option<&mut CwSerialDevice> {
    if keyer.device.is_none() {
        connect_serial_keyer(radio_id, keyer).await;
    }
    keyer.device.as_mut()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_control_line_defaults_to_dtr() {
        assert!(matches!(cw_serial_control_line("dtr"), ControlLine::Dtr));
        assert!(matches!(
            cw_serial_control_line("unknown"),
            ControlLine::Dtr
        ));
    }

    #[test]
    fn serial_control_line_accepts_rts() {
        assert!(matches!(cw_serial_control_line("rts"), ControlLine::Rts));
        assert!(matches!(cw_serial_control_line(" RTS "), ControlLine::Rts));
    }
}
