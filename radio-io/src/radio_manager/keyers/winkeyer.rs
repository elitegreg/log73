use super::super::cw_task::{CwKeyer, CwKeyerStatus, CwSendCompletion};
use futures_util::future::{BoxFuture, FutureExt};
use std::time::Duration;
use tracing::{info, warn};

pub(super) struct WinkeyerKeyer {
    pub(super) serial_port: String,
    pub(super) device: Option<winkeyer::WinKeyer>,
}

impl CwKeyer for WinkeyerKeyer {
    fn name(&self) -> &'static str {
        "Winkeyer"
    }

    fn connect<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()> {
        async move {
            connect_winkeyer(radio_id, self).await;
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
                let Some(winkeyer) = ensure_winkeyer_connected(radio_id, self).await else {
                    return Err("winkeyer unavailable".to_string());
                };
                winkeyer.send_text(text).await
            };
            if let Err(error) = result {
                warn!(radio_id, %error, "failed to send cw text through winkeyer");
                self.device = None;
                return Err(error.to_string());
            }
            Ok(CwSendCompletion::PollStatus {
                wait_for_busy: true,
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
                let Some(winkeyer) = ensure_winkeyer_connected(radio_id, self).await else {
                    return Err("winkeyer unavailable".to_string());
                };
                winkeyer.status().await
            };
            match result {
                Ok(status) => Ok(Some(CwKeyerStatus {
                    busy: status.busy || status.wait || status.key_down,
                })),
                Err(error) => {
                    warn!(radio_id, %error, "failed to read winkeyer status");
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
                let Some(winkeyer) = ensure_winkeyer_connected(radio_id, self).await else {
                    return Err("winkeyer unavailable".to_string());
                };
                winkeyer.clear_buffer().await
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
                let Some(winkeyer) = ensure_winkeyer_connected(radio_id, self).await else {
                    return Err("winkeyer unavailable".to_string());
                };
                winkeyer.set_wpm(wpm).await
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
            if let Some(mut winkeyer) = self.device.take()
                && let Err(error) = winkeyer.close().await
            {
                warn!(radio_id, %error, "failed to close winkeyer");
            }
        }
        .boxed()
    }
}

async fn connect_winkeyer(radio_id: i64, keyer: &mut WinkeyerKeyer) {
    if keyer.serial_port.trim().is_empty() {
        warn!(
            radio_id,
            "Winkeyer CW keying selected without a serial port"
        );
        return;
    }
    if keyer.device.is_some() {
        return;
    }

    match winkeyer::WinKeyer::open(&keyer.serial_port).await {
        Ok((mut winkeyer, revision)) => {
            winkeyer.set_timeout(Duration::from_millis(500));
            info!(
                radio_id,
                serial_port = %keyer.serial_port,
                revision,
                "connected to winkeyer"
            );
            keyer.device = Some(winkeyer);
        }
        Err(error) => {
            warn!(
                radio_id,
                serial_port = %keyer.serial_port,
                %error,
                "failed to connect to winkeyer"
            );
            keyer.device = None;
        }
    }
}

async fn ensure_winkeyer_connected(
    radio_id: i64,
    keyer: &mut WinkeyerKeyer,
) -> Option<&mut winkeyer::WinKeyer> {
    if keyer.device.is_none() {
        connect_winkeyer(radio_id, keyer).await;
    }
    keyer.device.as_mut()
}
