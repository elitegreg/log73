use super::rit::{next_rit_offset_hz, set_rit_offset_hz};
use crate::bands::Band;
use crate::db::RadioConfig;
use crate::radio::{
    RadioCommand, RadioState, logger_mode_from_cat_mode, mode_candidates_for_request,
};
use radio_cat_rs::{Frequency, Radio, RadioError};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, trace, warn};

pub(super) fn logger_state_from_cat_state(
    cat_state: &radio_cat_rs::RadioState,
    previous: Option<&RadioState>,
    data_mode: &str,
    rtty_mode: &str,
) -> Option<RadioState> {
    let frequency_hz = cat_state
        .main_rx()
        .frequency()
        .map(|frequency| frequency.hz())
        .or_else(|| previous.map(|state| state.frequency_hz))?;
    let mode = cat_state
        .main_rx()
        .mode()
        .map(|mode| {
            logger_mode_from_cat_mode(
                &mode,
                previous.map(|state| state.mode.as_str()),
                data_mode,
                rtty_mode,
            )
        })
        .or_else(|| previous.map(|state| state.mode.clone()))?;
    let rit_offset_hz = cat_state
        .rit_xit()
        .rit_offset(radio_cat_rs::ReceiverPath::Main)
        .map(|offset| i32::from(offset.as_hz()))
        .or_else(|| previous.map(|state| state.rit_offset_hz))
        .unwrap_or(0);

    Some(RadioState {
        frequency_hz,
        mode,
        rit_offset_hz,
    })
}

pub(super) fn fail_unavailable_radio_command(command: RadioCommand, reason: &str) {
    match command {
        RadioCommand::SendMessage { completed, .. }
        | RadioCommand::SendCwText { completed, .. } => {
            let _ = completed.send(Err(reason.to_string()));
        }
        RadioCommand::StopKeying
        | RadioCommand::SetWpm(_)
        | RadioCommand::SetFrequency(_)
        | RadioCommand::SetMode(_)
        | RadioCommand::RitClear
        | RadioCommand::RitIncrement(_)
        | RadioCommand::RitDecrement(_)
        | RadioCommand::ReloadConfig(_) => {}
    }
}

pub(super) async fn apply_command(
    radio: &Radio,
    current: &Arc<RwLock<Option<RadioState>>>,
    command: RadioCommand,
    last_rit_offset_hz: &mut i32,
    bands: &[Band],
    config: &RadioConfig,
    updates: &broadcast::Sender<RadioState>,
) -> Result<(), RadioError> {
    match command {
        RadioCommand::SetFrequency(frequency_hz) => {
            debug!(frequency_hz, "setting CAT radio frequency");
            radio
                .set_main_frequency(Frequency::from_hz(frequency_hz))
                .await
        }
        RadioCommand::SetMode(mode) => {
            let frequency_hz = current
                .read()
                .await
                .as_ref()
                .map(|state| state.frequency_hz)
                .or_else(|| {
                    radio
                        .latest_state()
                        .main_rx()
                        .frequency()
                        .map(|frequency| frequency.hz())
                })
                .unwrap_or(14_000_000);
            trace!(
                requested_mode = %mode,
                resolved_frequency_hz = frequency_hz,
                "translating CAT mode request"
            );

            let radio_modes = mode_candidates_for_request(
                &mode,
                frequency_hz,
                bands,
                &config.data_mode,
                &config.rtty_mode,
            );
            if radio_modes.is_empty() {
                debug!(mode, frequency_hz, "ignoring unsupported CAT radio mode");
                return Ok(());
            }

            let mut last_error = None;
            for radio_mode in radio_modes {
                match radio.set_main_mode(radio_mode).await {
                    Ok(()) => {
                        debug!(
                            requested_mode = %mode,
                            applied_mode = %radio_mode,
                            resolved_frequency_hz = frequency_hz,
                            "setting CAT radio mode"
                        );
                        publish_requested_logger_mode(current, updates, &mode).await;
                        return Ok(());
                    }
                    Err(error) => {
                        warn!(
                            requested_mode = %mode,
                            attempted_mode = %radio_mode,
                            resolved_frequency_hz = frequency_hz,
                            %error,
                            "failed to set CAT radio mode candidate"
                        );
                        last_error = Some(error);
                    }
                }
            }

            if let Some(error) = last_error {
                Err(error)
            } else {
                Ok(())
            }
        }
        RadioCommand::RitClear => {
            debug!(
                tracked_rit_offset_hz = *last_rit_offset_hz,
                "clearing CAT radio RIT"
            );
            let applied = set_rit_offset_hz(radio, 0, false).await?;
            if applied {
                *last_rit_offset_hz = 0;
            }
            debug!(
                tracked_rit_offset_hz = *last_rit_offset_hz,
                applied, "RIT clear command completed"
            );
            Ok(())
        }
        RadioCommand::RitIncrement(hz) => {
            let current_offset_hz = *last_rit_offset_hz;
            let next_offset_hz = next_rit_offset_hz(current_offset_hz, hz);
            debug!(
                hz,
                current_offset_hz, next_offset_hz, "incrementing CAT radio RIT"
            );
            if next_offset_hz == current_offset_hz {
                debug!(
                    current_offset_hz,
                    "RIT increment did not change offset after clamping"
                );
                return Ok(());
            }
            let applied = set_rit_offset_hz(radio, next_offset_hz, true).await?;
            if applied {
                *last_rit_offset_hz = next_offset_hz;
            }
            debug!(
                hz,
                current_offset_hz,
                tracked_rit_offset_hz = *last_rit_offset_hz,
                next_offset_hz,
                applied,
                "RIT increment command completed"
            );
            Ok(())
        }
        RadioCommand::RitDecrement(hz) => {
            let delta_hz = hz.saturating_neg();
            let current_offset_hz = *last_rit_offset_hz;
            let next_offset_hz = next_rit_offset_hz(current_offset_hz, delta_hz);
            debug!(
                hz,
                delta_hz, current_offset_hz, next_offset_hz, "decrementing CAT radio RIT"
            );
            if next_offset_hz == current_offset_hz {
                debug!(
                    current_offset_hz,
                    "RIT decrement did not change offset after clamping"
                );
                return Ok(());
            }
            let applied = set_rit_offset_hz(radio, next_offset_hz, true).await?;
            if applied {
                *last_rit_offset_hz = next_offset_hz;
            }
            debug!(
                hz,
                delta_hz,
                current_offset_hz,
                tracked_rit_offset_hz = *last_rit_offset_hz,
                next_offset_hz,
                applied,
                "RIT decrement command completed"
            );
            Ok(())
        }
        RadioCommand::SendMessage { .. }
        | RadioCommand::SendCwText { .. }
        | RadioCommand::StopKeying
        | RadioCommand::SetWpm(_)
        | RadioCommand::ReloadConfig(_) => Ok(()),
    }
}

async fn publish_requested_logger_mode(
    current: &Arc<RwLock<Option<RadioState>>>,
    updates: &broadcast::Sender<RadioState>,
    requested_mode: &str,
) {
    let requested_mode = requested_mode.trim().to_uppercase();
    let mut current = current.write().await;
    let Some(state) = current.as_mut() else {
        return;
    };
    if state.mode == requested_mode {
        return;
    }
    state.mode = requested_mode;
    let _ = updates.send(state.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radio::RadioCommand;
    use serde_json::Map;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn fail_unavailable_radio_command_rejects_send_message() {
        let (completed_tx, completed_rx) = oneshot::channel();

        fail_unavailable_radio_command(
            RadioCommand::SendMessage {
                mode: "run".to_string(),
                keys: vec!["F1".to_string()],
                fields: Map::new(),
                completed: completed_tx,
            },
            "radio disconnected",
        );

        assert_eq!(
            completed_rx.await.unwrap(),
            Err("radio disconnected".to_string())
        );
    }

    #[test]
    fn logger_state_uses_previous_values_for_unknown_fields() {
        let previous = RadioState {
            frequency_hz: 14_000_000,
            mode: "CW".to_string(),
            rit_offset_hz: 20,
        };
        let cat_state = radio_cat_rs::RadioState::default();

        assert_eq!(
            logger_state_from_cat_state(&cat_state, Some(&previous), "DATA-USB", "RTTY"),
            Some(previous)
        );
    }

    #[tokio::test]
    async fn requested_logger_mode_is_published_before_cat_readback() {
        let current = Arc::new(RwLock::new(Some(RadioState {
            frequency_hz: 14_000_000,
            mode: "SSB".to_string(),
            rit_offset_hz: 0,
        })));
        let (updates, mut updates_rx) = broadcast::channel(1);

        publish_requested_logger_mode(&current, &updates, " data ").await;

        assert_eq!(current.read().await.as_ref().unwrap().mode, "DATA");
        assert_eq!(updates_rx.recv().await.unwrap().mode, "DATA");
    }
}
