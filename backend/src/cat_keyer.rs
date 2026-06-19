use radio_cat_rs::{Radio, StateUpdate};
use tokio::sync::broadcast;
use tracing::{debug, trace};

pub struct CatKeyer {
    radio_id: i64,
    radio: Radio,
}

impl CatKeyer {
    pub async fn open(radio_id: i64, radio: Radio) -> Self {
        debug!(radio_id, "initialized CAT keyer");

        Self { radio_id, radio }
    }

    pub fn subscribe_updates(&self) -> broadcast::Receiver<StateUpdate> {
        self.radio.subscribe_updates()
    }

    pub async fn send_text(&self, text: &str) -> Result<(), String> {
        debug!(radio_id = self.radio_id, text, "sending CAT CW text");
        self.radio
            .send_cw(text)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn clear_buffer(&self) -> Result<(), String> {
        debug!(radio_id = self.radio_id, "stopping CAT CW");
        self.radio
            .stop_cw()
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn set_wpm(&mut self, wpm: u8) -> Result<(), String> {
        debug!(radio_id = self.radio_id, wpm, "setting CAT CW WPM");
        self.radio
            .set_keyer_speed(wpm)
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub async fn close(&mut self) {
        trace!(radio_id = self.radio_id, "closing CAT keyer");
    }
}
