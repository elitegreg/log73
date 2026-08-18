//! Maidenhead locator parsing and great-circle distance calculations.

const EARTH_RADIUS_KM: f64 = 6_371.008_8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridCenter {
    pub latitude_degrees: f64,
    pub longitude_degrees: f64,
}

/// Returns the geographic center of a four-character Maidenhead grid square.
///
/// The ARRL International Digital Contest exchanges four-character locators, so
/// longer locators deliberately use their first four characters here.
pub fn four_character_grid_center(locator: &str) -> Option<GridCenter> {
    let locator = locator.trim().to_ascii_uppercase();
    let bytes = locator.as_bytes();
    if bytes.len() < 4 {
        return None;
    }

    let field_longitude = letter_index(bytes[0])?;
    let field_latitude = letter_index(bytes[1])?;
    let square_longitude = digit_index(bytes[2])?;
    let square_latitude = digit_index(bytes[3])?;

    Some(GridCenter {
        longitude_degrees: -180.0
            + field_longitude as f64 * 20.0
            + square_longitude as f64 * 2.0
            + 1.0,
        latitude_degrees: -90.0 + field_latitude as f64 * 10.0 + square_latitude as f64 + 0.5,
    })
}

/// Calculates the great-circle distance in kilometres between two grid-square
/// centers using the haversine formula.
pub fn distance_kilometers(first: GridCenter, second: GridCenter) -> f64 {
    let latitude_delta = (second.latitude_degrees - first.latitude_degrees).to_radians();
    let longitude_delta = (second.longitude_degrees - first.longitude_degrees).to_radians();
    let first_latitude = first.latitude_degrees.to_radians();
    let second_latitude = second.latitude_degrees.to_radians();

    let haversine = (latitude_delta / 2.0).sin().powi(2)
        + first_latitude.cos() * second_latitude.cos() * (longitude_delta / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * haversine.clamp(0.0, 1.0).sqrt().asin()
}

pub fn grid_distance_kilometers(station_locator: &str, contact_locator: &str) -> Option<f64> {
    Some(distance_kilometers(
        four_character_grid_center(station_locator)?,
        four_character_grid_center(contact_locator)?,
    ))
}

fn letter_index(value: u8) -> Option<u8> {
    match value {
        b'A'..=b'R' => Some(value - b'A'),
        _ => None,
    }
}

fn digit_index(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_four_character_grids() {
        assert_eq!(
            four_character_grid_center("fn31"),
            Some(GridCenter {
                latitude_degrees: 41.5,
                longitude_degrees: -73.0,
            })
        );
        assert_eq!(four_character_grid_center("ZZ99"), None);
        assert_eq!(four_character_grid_center("FN3"), None);
    }

    #[test]
    fn calculates_great_circle_grid_distance() {
        let distance = grid_distance_kilometers("FN31", "EN50").expect("valid grids");
        assert!((distance - 1_345.0).abs() < 20.0, "distance was {distance}");
    }
}
