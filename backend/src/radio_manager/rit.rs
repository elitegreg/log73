use radio_cat_rs::{Radio, RadioError, RitXitOffsetHz};
use tracing::debug;

const MIN_RIT_OFFSET_HZ: i32 = -9_999;
const MAX_RIT_OFFSET_HZ: i32 = 9_999;

pub(super) fn next_rit_offset_hz(current_offset_hz: i32, delta_hz: i32) -> i32 {
    current_offset_hz
        .saturating_add(delta_hz)
        .clamp(MIN_RIT_OFFSET_HZ, MAX_RIT_OFFSET_HZ)
}

pub(super) async fn set_rit_offset_hz(
    radio: &Radio,
    target_offset_hz: i32,
    enable_rit: bool,
) -> Result<bool, RadioError> {
    debug!(target_offset_hz, enable_rit, "applying CAT RIT offset");
    if enable_rit {
        match radio.set_main_rit_enabled(true).await {
            Ok(()) => {}
            Err(error) if is_unsupported_capability(&error) => {
                debug!(target_offset_hz, "CAT RIT enable unsupported by radio");
            }
            Err(error) => return Err(error),
        }
    }

    let offset =
        RitXitOffsetHz::new(target_offset_hz as i16).map_err(|error| RadioError::InvalidValue {
            field: "rit_offset_hz",
            message: error.to_string(),
        })?;

    match radio.set_main_rit_offset(offset).await {
        Ok(()) => {
            debug!(target_offset_hz, "CAT RIT offset applied");
            Ok(true)
        }
        Err(error) if is_unsupported_capability(&error) => {
            debug!(target_offset_hz, "CAT RIT offset unsupported by radio");
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

fn is_unsupported_capability(error: &RadioError) -> bool {
    matches!(error, RadioError::UnsupportedCapability { .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use radio_cat_rs::RadioConfig;

    #[test]
    fn next_rit_offset_hz_applies_delta_from_tracked_offset() {
        assert_eq!(next_rit_offset_hz(0, 20), 20);
        assert_eq!(next_rit_offset_hz(35, 15), 50);
        assert_eq!(next_rit_offset_hz(35, -15), 20);
    }

    #[test]
    fn next_rit_offset_hz_clamps_to_supported_range() {
        assert_eq!(next_rit_offset_hz(MAX_RIT_OFFSET_HZ, 1), MAX_RIT_OFFSET_HZ);
        assert_eq!(next_rit_offset_hz(MIN_RIT_OFFSET_HZ, -1), MIN_RIT_OFFSET_HZ);
    }

    #[tokio::test]
    async fn set_rit_offset_hz_false_preserves_enabled_state() {
        let radio = Radio::connect(RadioConfig::dummy())
            .await
            .expect("dummy radio connects");
        radio
            .set_main_rit_enabled(true)
            .await
            .expect("rit enables on dummy radio");

        let applied = set_rit_offset_hz(&radio, 125, false)
            .await
            .expect("rit offset applies");

        assert!(applied);
        let state = radio.latest_state();
        assert_eq!(state.rit_xit.main_rit_enabled, Some(true));
        assert_eq!(state.rit_xit.offset_hz, RitXitOffsetHz::new(125).ok());
    }

    #[tokio::test]
    async fn set_rit_offset_hz_true_enables_rit() {
        let radio = Radio::connect(RadioConfig::dummy())
            .await
            .expect("dummy radio connects");

        let applied = set_rit_offset_hz(&radio, 250, true)
            .await
            .expect("rit offset applies");

        assert!(applied);
        let state = radio.latest_state();
        assert_eq!(state.rit_xit.main_rit_enabled, Some(true));
        assert_eq!(state.rit_xit.offset_hz, RitXitOffsetHz::new(250).ok());
    }
}
