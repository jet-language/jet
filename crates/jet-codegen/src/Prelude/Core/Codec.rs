// D-SERDE2 / R12: named temporal and Decimal wire semantics live here once.
// AOT, JIT, and the interpreter pass their own value handles' components to
// these adapters; none of those tiers re-implement parsing or canonical
// formatting.

fn jet_codec_is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn jet_codec_days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if jet_codec_is_leap(year) => 29,
        2 => 28,
        _ => 30,
    }
}

fn jet_codec_date_day_number(year: i64, month: i64, day: i64) -> i64 {
    let prior_year = year - 1;
    365 * prior_year
        + prior_year / 4
        - prior_year / 100
        + prior_year / 400
        + [0i64, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334][(month - 1) as usize]
        + if month > 2 && jet_codec_is_leap(year) { 1 } else { 0 }
        + day
        - 1
}

fn jet_codec_date_from_day_number(mut day_number: i64) -> (i64, i64, i64) {
    let mut year = day_number / 365 + 1;
    loop {
        let start = jet_codec_date_day_number(year, 1, 1);
        let next = jet_codec_date_day_number(year + 1, 1, 1);
        if day_number >= start && day_number < next {
            break;
        }
        if day_number < start {
            year -= 1;
        } else {
            year += 1;
        }
    }
    day_number -= jet_codec_date_day_number(year, 1, 1);
    let mut month = 1i64;
    while month < 12 && day_number >= jet_codec_days_in_month(year, month) {
        day_number -= jet_codec_days_in_month(year, month);
        month += 1;
    }
    (year, month, day_number + 1)
}

fn jet_codec_date_components(year: i64, month: i64, day: i64) -> (i64, i64, i64) {
    let month = month.clamp(1, 12);
    (year, month, day.clamp(1, jet_codec_days_in_month(year, month)))
}

pub(crate) fn jet_codec_date_encode(year: i64, month: i64, day: i64) -> String {
    let (year, month, day) = jet_codec_date_components(year, month, day);
    format!("{year:04}-{month:02}-{day:02}")
}

pub(crate) fn jet_codec_date_decode(value: &str) -> Result<(i64, i64, i64), String> {
    let parts: Vec<&str> = value.splitn(3, '-').collect();
    if parts.len() != 3 {
        return Err(format!("invalid date: {value}"));
    }
    let year = parts[0]
        .parse::<i64>()
        .map_err(|_| format!("bad year: {}", parts[0]))?;
    let month = parts[1]
        .parse::<i64>()
        .map_err(|_| format!("bad month: {}", parts[1]))?;
    let day = parts[2]
        .parse::<i64>()
        .map_err(|_| format!("bad day: {}", parts[2]))?;
    if month < 1 || month > 12 || day < 1 || day > jet_codec_days_in_month(year, month) {
        return Err(format!("date out of range: {value}"));
    }
    Ok((year, month, day))
}

pub(crate) fn jet_codec_local_time_encode(hour: i64, minute: i64, second: i64) -> String {
    let hour = hour.clamp(0, 23);
    let minute = minute.clamp(0, 59);
    let second = second.clamp(0, 59);
    format!("{hour:02}:{minute:02}:{second:02}")
}

pub(crate) fn jet_codec_local_time_decode(value: &str) -> Result<(i64, i64, i64), String> {
    let parts: Vec<&str> = value.splitn(3, ':').collect();
    if parts.len() != 3 {
        return Err(format!("invalid time: {value}"));
    }
    let hour = parts[0]
        .parse::<i64>()
        .map_err(|_| format!("bad hour: {}", parts[0]))?;
    let minute = parts[1]
        .parse::<i64>()
        .map_err(|_| format!("bad minute: {}", parts[1]))?;
    let second = parts[2]
        .parse::<i64>()
        .map_err(|_| format!("bad second: {}", parts[2]))?;
    if hour < 0 || hour > 23 || minute < 0 || minute > 59 || second < 0 || second > 59 {
        return Err(format!("time out of range: {value}"));
    }
    Ok((hour, minute, second))
}

pub(crate) fn jet_codec_datetime_encode(secs: i64, nanos: u32) -> String {
    let secs = secs.saturating_add((nanos / 1_000_000_000) as i64);
    let nanos = nanos % 1_000_000_000;
    let epoch = jet_codec_date_day_number(1970, 1, 1);
    let (year, month, day) = jet_codec_date_from_day_number(epoch + secs.div_euclid(86400));
    let day_seconds = secs.rem_euclid(86400);
    let date = jet_codec_date_encode(year, month, day);
    let time = jet_codec_local_time_encode(
        day_seconds / 3600,
        (day_seconds / 60) % 60,
        day_seconds % 60,
    );
    if nanos == 0 {
        format!("{date}T{time}Z")
    } else {
        format!("{date}T{time}.{nanos:09}Z")
    }
}

pub(crate) fn jet_codec_datetime_decode(value: &str) -> Result<(i64, u32), String> {
    let (date_part, rest) = value
        .split_once('T')
        .ok_or_else(|| format!("invalid RFC3339 datetime: {value}"))?;
    let (year, month, day) = jet_codec_date_decode(date_part)?;
    let zone_pos = rest
        .find('Z')
        .or_else(|| rest.rfind('+'))
        .or_else(|| rest.get(1..).and_then(|tail| tail.rfind('-').map(|i| i + 1)))
        .ok_or_else(|| format!("RFC3339 datetime needs Z or an offset: {value}"))?;
    let (time_part, zone_part) = rest.split_at(zone_pos);
    let (clean_time, frac) = match time_part.split_once('.') {
        Some((time, fraction)) => (time, Some(fraction)),
        None => (time_part, None),
    };
    let (hour, minute, second) = jet_codec_local_time_decode(clean_time)?;
    let mut nanos = 0u32;
    if let Some(fraction) = frac {
        let digits: String = fraction
            .chars()
            .take(9)
            .filter(|ch| ch.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            let padded = format!("{digits:0<9}");
            nanos = padded.parse::<u32>().unwrap_or(0);
        }
    }
    let offset = if zone_part == "Z" {
        0
    } else {
        let sign = if zone_part.starts_with('-') { -1 } else { 1 };
        let zone = &zone_part[1..];
        let (zone_hour, zone_minute) = zone
            .split_once(':')
            .ok_or_else(|| format!("bad RFC3339 offset: {zone_part}"))?;
        let zone_hour = zone_hour
            .parse::<i64>()
            .map_err(|_| format!("bad RFC3339 offset hour: {zone_hour}"))?;
        let zone_minute = zone_minute
            .parse::<i64>()
            .map_err(|_| format!("bad RFC3339 offset minute: {zone_minute}"))?;
        sign * (zone_hour * 3600 + zone_minute * 60)
    };
    let epoch = jet_codec_date_day_number(1970, 1, 1);
    let secs = (jet_codec_date_day_number(year, month, day) - epoch)
        .saturating_mul(86400)
        .saturating_add(hour * 3600 + minute * 60 + second)
        .saturating_sub(offset);
    Ok((secs, nanos))
}

pub(crate) fn jet_codec_duration_encode(ns: i64) -> i64 {
    ns
}

pub(crate) fn jet_codec_duration_decode(ns: i64) -> i64 {
    ns
}

pub(crate) fn jet_codec_decimal_encode(value: &jet_std::JetDecimal) -> String {
    value.to_string_rep()
}

pub(crate) fn jet_codec_decimal_decode_text(
    value: &str,
) -> Result<jet_std::JetDecimal, String> {
    jet_std::JetDecimal::from_str(value)
}

pub(crate) fn jet_codec_decimal_decode_int(
    value: i64,
) -> Result<jet_std::JetDecimal, String> {
    jet_std::JetDecimal::from_str(&value.to_string())
}
