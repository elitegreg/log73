pub fn qso_epoch(date: &str, time: &str) -> Option<i64> {
    if date.len() != 8 || !date.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let year = date[0..4].parse::<i32>().ok()?;
    let month = date[4..6].parse::<u32>().ok()?;
    let day = date[6..8].parse::<u32>().ok()?;

    let time = time.trim();
    if !matches!(time.len(), 4 | 6) || !time.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let hour = time[0..2].parse::<i64>().ok()?;
    let minute = time[2..4].parse::<i64>().ok()?;
    let second = if time.len() == 6 {
        time[4..6].parse::<i64>().ok()?
    } else {
        0
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    let days = days_from_civil(year, month, day)?;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

pub fn qso_datetime_adif(epoch: i64) -> Result<(String, String), String> {
    let (year, month, day, hour, minute, second) = qso_datetime_parts(epoch)?;
    Ok((
        format!("{year:04}{month:02}{day:02}"),
        format!("{hour:02}{minute:02}{second:02}"),
    ))
}

pub fn qso_datetime_cabrillo(epoch: i64) -> Result<(String, String), String> {
    let (year, month, day, hour, minute, _second) = qso_datetime_parts(epoch)?;
    Ok((
        format!("{year:04}-{month:02}-{day:02}"),
        format!("{hour:02}{minute:02}"),
    ))
}

fn qso_datetime_parts(epoch: i64) -> Result<(i32, u32, u32, i64, i64, i64), String> {
    if epoch < 0 {
        return Err("contact QSO time must be positive".to_string());
    }
    let days = epoch.div_euclid(86_400);
    let seconds = epoch.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds / 3_600;
    let minute = (seconds % 3_600) / 60;
    let second = seconds % 60;
    Ok((year, month, day, hour, minute, second))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let original_year = year;
    let original_month = month;
    let original_day = day;
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = i64::from(month);
    let day = i64::from(day);
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let (roundtrip_year, roundtrip_month, roundtrip_day) = civil_from_days(days);
    if roundtrip_year == original_year
        && roundtrip_month == original_month
        && roundtrip_day == original_day
    {
        Some(days)
    } else {
        None
    }
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_qso_epoch_from_adif_date_and_time() {
        assert_eq!(qso_epoch("20231114", "221523"), Some(1_700_000_123));
        assert_eq!(qso_epoch("20231114", "2215"), Some(1_700_000_100));
        assert_eq!(qso_epoch("20230229", "221523"), None);
    }

    #[test]
    fn formats_qso_datetime_for_adif_and_cabrillo() {
        assert_eq!(
            qso_datetime_adif(1_700_000_123).unwrap(),
            ("20231114".to_string(), "221523".to_string())
        );
        assert_eq!(
            qso_datetime_cabrillo(1_700_000_123).unwrap(),
            ("2023-11-14".to_string(), "2215".to_string())
        );
    }
}
