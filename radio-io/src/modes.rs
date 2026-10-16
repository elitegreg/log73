use radio_cat_rs::{DriverDescriptor, Mode, RadioRegion, supported_drivers};

pub const LOGGER_MODE_OPTIONS: &[&str] = &["CW", "CW-R", "SSB", "FM", "AM", "DATA", "RTTY"];

pub fn normalize_logger_mode(mode: &str) -> String {
    mode.trim().to_uppercase()
}

pub fn is_selectable_mode(mode: &str) -> bool {
    LOGGER_MODE_OPTIONS.contains(&normalize_logger_mode(mode).as_str())
}

pub fn mode_is_cw(mode: &str) -> bool {
    matches!(normalize_logger_mode(mode).as_str(), "CW" | "CW-R")
}

pub fn mode_is_phone(mode: &str) -> bool {
    matches!(normalize_logger_mode(mode).as_str(), "SSB" | "FM" | "AM")
}

pub fn mode_is_digital(mode: &str) -> bool {
    matches!(normalize_logger_mode(mode).as_str(), "DATA" | "RTTY")
}

pub fn adif_mode_for_logger_mode(mode: &str) -> String {
    let normalized = normalize_logger_mode(mode);
    match normalized.as_str() {
        "CW-R" => "CW".to_string(),
        "DATA" => "RTTY".to_string(),
        _ => normalized,
    }
}

pub fn transmit_modes_for_radio_kind(radio_kind: &str) -> Result<&'static [Mode], String> {
    let driver = driver_for_radio_kind(radio_kind)?;
    let capabilities = driver
        .capabilities(capability_region(driver))
        .map_err(|error| error.to_string())?;

    Ok(capabilities.tx.map_or(&[], |tx| tx.modes))
}

pub fn supports_cat_cw_keying_for_radio_kind(radio_kind: &str) -> Result<bool, String> {
    let driver = driver_for_radio_kind(radio_kind)?;
    let capabilities = driver
        .capabilities(capability_region(driver))
        .map_err(|error| error.to_string())?;

    Ok(capabilities
        .keyer
        .is_some_and(|keyer| keyer.send_cw.is_supported()))
}

fn driver_for_radio_kind(radio_kind: &str) -> Result<DriverDescriptor, String> {
    supported_drivers()
        .iter()
        .copied()
        .find(|driver| driver.id.eq_ignore_ascii_case(radio_kind.trim()))
        .ok_or_else(|| format!("unsupported radio driver: {}", radio_kind.trim()))
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

    #[derive(serde::Deserialize)]
    struct Fixture {
        version: u32,
        logger_modes: Vec<String>,
        cases: Vec<Case>,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        input: Option<String>,
        normalized: String,
        selectable: bool,
        cw: bool,
        phone: bool,
        digital: bool,
        adif: String,
    }

    #[test]
    fn shared_logger_mode_contract() {
        let fixture: Fixture =
            serde_json::from_str(include_str!("../../tests/fixtures/domain/modes.json"))
                .expect("valid logger mode fixture");
        assert_eq!(fixture.version, 1);
        assert!(!fixture.cases.is_empty());
        assert!(!fixture.logger_modes.is_empty());
        assert_eq!(LOGGER_MODE_OPTIONS, fixture.logger_modes);
        for case in fixture.cases {
            let input = case.input.as_deref().unwrap_or_default();
            assert_eq!(
                normalize_logger_mode(input),
                case.normalized,
                "{}",
                case.name
            );
            assert_eq!(is_selectable_mode(input), case.selectable, "{}", case.name);
            assert_eq!(mode_is_cw(input), case.cw, "{}", case.name);
            assert_eq!(mode_is_phone(input), case.phone, "{}", case.name);
            assert_eq!(mode_is_digital(input), case.digital, "{}", case.name);
            assert_eq!(adif_mode_for_logger_mode(input), case.adif, "{}", case.name);
        }
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
    fn cat_cw_keying_support_comes_from_driver_capabilities() {
        assert!(supports_cat_cw_keying_for_radio_kind("elecraft-k4").expect("K4 capabilities"));
        assert!(
            !supports_cat_cw_keying_for_radio_kind("yaesu-ftdx10").expect("FTDX10 capabilities")
        );
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
