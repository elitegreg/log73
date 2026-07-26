use radio_cat_rs::{DriverDescriptor, Mode, RadioRegion, supported_drivers};

pub const LOGGER_MODE_OPTIONS: &[&str] = &["CW", "CW-R", "SSB", "FM", "AM", "DATA", "RTTY"];

pub fn mode_is_cw(mode: &str) -> bool {
    matches!(mode.trim().to_uppercase().as_str(), "CW" | "CW-R")
}

pub fn transmit_modes_for_radio_kind(radio_kind: &str) -> Result<&'static [Mode], String> {
    let driver = supported_drivers()
        .iter()
        .copied()
        .find(|driver| driver.id.eq_ignore_ascii_case(radio_kind.trim()))
        .ok_or_else(|| format!("unsupported radio driver: {}", radio_kind.trim()))?;
    let capabilities = driver
        .capabilities(capability_region(driver))
        .map_err(|error| error.to_string())?;

    Ok(capabilities.tx.map_or(&[], |tx| tx.modes))
}

pub fn default_data_mode(transmit_modes: &[Mode]) -> Mode {
    if transmit_modes.contains(&Mode::DataUsb) {
        Mode::DataUsb
    } else {
        Mode::Usb
    }
}

pub fn default_rtty_mode(transmit_modes: &[Mode]) -> Mode {
    if transmit_modes.contains(&Mode::Rtty) {
        Mode::Rtty
    } else if transmit_modes.contains(&Mode::DataUsb) {
        Mode::DataUsb
    } else {
        Mode::Usb
    }
}

pub fn resolved_mode_mappings(
    radio_kind: &str,
    data_mode: &str,
    rtty_mode: &str,
) -> Result<(String, String), String> {
    let transmit_modes = transmit_modes_for_radio_kind(radio_kind)?;
    let data_mode = resolved_mode_mapping(
        "DATA",
        data_mode,
        default_data_mode(transmit_modes),
        transmit_modes,
    )?;
    let rtty_mode = resolved_mode_mapping(
        "RTTY",
        rtty_mode,
        default_rtty_mode(transmit_modes),
        transmit_modes,
    )?;
    Ok((data_mode.to_string(), rtty_mode.to_string()))
}

fn resolved_mode_mapping(
    logger_mode: &str,
    configured_mode: &str,
    default_mode: Mode,
    transmit_modes: &[Mode],
) -> Result<Mode, String> {
    let mode = if configured_mode.trim().is_empty() {
        default_mode
    } else {
        configured_mode
            .parse::<Mode>()
            .map_err(|error| format!("{logger_mode} radio mode is invalid: {error}"))?
    };
    if !transmit_modes.contains(&mode) {
        return Err(format!(
            "{logger_mode} radio mode {mode} is not supported for transmit by this radio"
        ));
    }
    Ok(mode)
}

fn capability_region(driver: DriverDescriptor) -> Option<RadioRegion> {
    driver
        .supported_regions()
        .contains(&RadioRegion::IaruRegion2)
        .then_some(RadioRegion::IaruRegion2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logger_modes_are_the_supported_generic_modes() {
        assert_eq!(
            LOGGER_MODE_OPTIONS,
            ["CW", "CW-R", "SSB", "FM", "AM", "DATA", "RTTY"]
        );
    }

    #[test]
    fn data_default_prefers_data_usb_then_usb() {
        assert_eq!(
            default_data_mode(&[Mode::Usb, Mode::DataUsb]),
            Mode::DataUsb
        );
        assert_eq!(default_data_mode(&[Mode::Usb]), Mode::Usb);
    }

    #[test]
    fn rtty_default_uses_requested_fallback_order() {
        assert_eq!(
            default_rtty_mode(&[Mode::Usb, Mode::DataUsb, Mode::Rtty]),
            Mode::Rtty
        );
        assert_eq!(
            default_rtty_mode(&[Mode::Usb, Mode::DataUsb]),
            Mode::DataUsb
        );
        assert_eq!(default_rtty_mode(&[Mode::Usb]), Mode::Usb);
    }

    #[test]
    fn driver_transmit_modes_come_from_capabilities() {
        let modes = transmit_modes_for_radio_kind("dummy").expect("dummy capabilities");
        assert_eq!(modes, Mode::ALL);
    }

    #[test]
    fn resolved_mappings_apply_defaults_and_canonicalize_values() {
        assert_eq!(
            resolved_mode_mappings("dummy", "", "data_usb").expect("valid mappings"),
            ("DATA-USB".to_string(), "DATA-USB".to_string())
        );
    }

    #[test]
    fn resolved_mappings_reject_modes_outside_transmit_capabilities() {
        let error = resolved_mode_mappings("elecraft-k2", "WFM", "RTTY")
            .expect_err("K2 does not support WFM");
        assert!(error.contains("not supported for transmit"));
    }
}
