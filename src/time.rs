use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedTimestamp {
    pub hour: u8,
    pub day_of_week: u8,
    pub epoch_seconds: i64,
}

#[derive(Debug, Error)]
pub enum TimestampError {
    #[error("timestamp must be UTC ISO-8601 in the form YYYY-MM-DDTHH:MM:SSZ: {0}")]
    InvalidFormat(String),
    #[error("timestamp contains an out-of-range field: {0}")]
    OutOfRange(String),
}

pub fn parse_utc_timestamp(input: &str) -> Result<ParsedTimestamp, TimestampError> {
    let bytes = input.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return Err(TimestampError::InvalidFormat(input.to_string()));
    }

    let year = digits(bytes, 0, 4, input)? as i32;
    let month = digits(bytes, 5, 2, input)? as u32;
    let day = digits(bytes, 8, 2, input)? as u32;
    let hour = digits(bytes, 11, 2, input)? as u32;
    let minute = digits(bytes, 14, 2, input)? as u32;
    let second = digits(bytes, 17, 2, input)? as u32;

    validate_date_time(year, month, day, hour, minute, second, input)?;

    let days = days_from_civil(year, month, day);
    let day_of_week = (days + 3).rem_euclid(7) as u8;
    let epoch_seconds = days * 86_400 + i64::from(hour * 3_600 + minute * 60 + second);

    Ok(ParsedTimestamp {
        hour: hour as u8,
        day_of_week,
        epoch_seconds,
    })
}

fn digits(bytes: &[u8], start: usize, len: usize, original: &str) -> Result<u32, TimestampError> {
    let mut value = 0_u32;
    for byte in bytes.iter().skip(start).take(len) {
        let digit = byte.wrapping_sub(b'0');
        if digit > 9 {
            return Err(TimestampError::InvalidFormat(original.to_string()));
        }
        value = value * 10 + u32::from(digit);
    }
    Ok(value)
}

fn validate_date_time(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    original: &str,
) -> Result<(), TimestampError> {
    if !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
        return Err(TimestampError::OutOfRange(original.to_string()));
    }
    let max_day = days_in_month(year, month);
    if day == 0 || day > max_day {
        return Err(TimestampError::OutOfRange(original.to_string()));
    }
    Ok(())
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(mut year: i32, month: u32, day: u32) -> i64 {
    year -= i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month as i32 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    i64::from(era * 146_097 + day_of_era - 719_468)
}
