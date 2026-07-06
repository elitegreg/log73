use crate::db::RadioConfig;
use crate::voice_keyer::{VoiceKeyer, VoicePlaybackThread};
use radio_cat_rs::Radio;
use tracing::{debug, warn};

pub(super) fn spawn_voice_playback_thread(
    config: &RadioConfig,
    voice_keyer: VoiceKeyer,
) -> Option<VoicePlaybackThread> {
    match VoicePlaybackThread::spawn(
        config.id,
        voice_keyer,
        config.voice_output_device_id.clone(),
    ) {
        Ok(worker) => Some(worker),
        Err(error) => {
            warn!(radio_id = config.id, %error, "failed to start voice keyer thread");
            None
        }
    }
}

pub(super) struct VoiceDataPttGuard {
    radio_id: i64,
    radio: Radio,
    active: bool,
}

impl VoiceDataPttGuard {
    pub(super) async fn acquire(radio_id: i64, radio: Radio, supported: bool) -> Self {
        let mut guard = Self {
            radio_id,
            radio,
            active: false,
        };

        if !supported {
            return guard;
        }

        match guard.radio.set_data_ptt(true).await {
            Ok(()) => {
                guard.active = true;
                debug!(
                    radio_id = guard.radio_id,
                    "enabled data ptt for voice playback"
                );
            }
            Err(error) => {
                warn!(radio_id = guard.radio_id, %error, "failed to enable data ptt for voice playback");
            }
        }

        guard
    }

    pub(super) async fn release(&mut self) {
        if !self.active {
            return;
        }

        match self.radio.set_data_ptt(false).await {
            Ok(()) => {
                self.active = false;
                debug!(
                    radio_id = self.radio_id,
                    "disabled data ptt after voice playback"
                );
            }
            Err(error) => {
                warn!(radio_id = self.radio_id, %error, "failed to disable data ptt after voice playback");
            }
        }
    }
}

impl Drop for VoiceDataPttGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }

        let radio = self.radio.clone();
        let radio_id = self.radio_id;
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    if let Err(error) = radio.set_data_ptt(false).await {
                        warn!(radio_id, %error, "failed to disable data ptt after voice playback");
                    } else {
                        debug!(radio_id, "disabled data ptt after voice playback");
                    }
                });
            }
            Err(_) => {
                warn!(
                    radio_id,
                    "voice data ptt guard dropped without a tokio runtime; unable to schedule cleanup"
                );
            }
        }
    }
}
