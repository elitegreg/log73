use radio_cat_rs::Radio;
use std::time::Duration;
use tracing::{debug, trace, warn};

const DEFAULT_WPM: u8 = 20;
const CW_SECONDS_PER_CHARACTER_AT_ONE_WPM: f64 = 12.0;

pub struct CatKeyer {
    radio_id: i64,
    radio: Radio,
    wpm: u8,
}

impl CatKeyer {
    pub async fn open(radio_id: i64, radio: Radio) -> Self {
        let wpm = radio
            .latest_state()
            .keyer
            .as_ref()
            .and_then(|keyer| keyer.speed_wpm)
            .unwrap_or_else(|| {
                warn!(
                    radio_id,
                    default_wpm = DEFAULT_WPM,
                    "CAT keyer WPM is unknown; using default"
                );
                DEFAULT_WPM
            });

        debug!(radio_id, wpm, "initialized CAT keyer WPM from radio state");

        Self {
            radio_id,
            radio,
            wpm,
        }
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
        self.wpm = wpm;
        Ok(())
    }

    pub fn estimated_send_duration(&self, text: &str) -> Duration {
        let seconds = estimated_send_seconds(text.chars().count(), self.wpm);
        trace!(
            radio_id = self.radio_id,
            wpm = self.wpm,
            char_count = text.chars().count(),
            estimated_seconds = seconds,
            "estimated CAT CW send duration"
        );
        Duration::from_secs_f64(seconds)
    }

    pub async fn close(&mut self) {
        trace!(radio_id = self.radio_id, "closing CAT keyer");
    }
}

fn estimated_send_seconds(char_count: usize, wpm: u8) -> f64 {
    (char_count as f64 * CW_SECONDS_PER_CHARACTER_AT_ONE_WPM) / f64::from(wpm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_send_seconds_from_issue_formula() {
        assert_eq!(estimated_send_seconds(10, 20), 6.0);
        assert_eq!(estimated_send_seconds(7, 35), 2.4);
    }
}
