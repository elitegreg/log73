pub const LOGGER_MODE_OPTIONS: &[&str] = &[
    "CW", "CW-R", "SSB", "FM", "AM", "FT8", "JT65", "JT9", "MFSK", "PSK", "RTTY",
];

pub fn mode_is_cw(mode: &str) -> bool {
    matches!(mode.trim().to_uppercase().as_str(), "CW" | "CW-R")
}
