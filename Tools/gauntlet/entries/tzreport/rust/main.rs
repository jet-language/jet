use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader};

fn number(bytes: &[u8], start: usize, len: usize) -> i64 {
    bytes[start..start + len]
        .iter()
        .fold(0, |value, byte| value * 10 + i64::from(byte - b'0'))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = if adjusted_year >= 0 { adjusted_year } else { adjusted_year - 399 } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719468;
    let era = if shifted >= 0 { shifted } else { shifted - 146096 } / 146097;
    let day_of_era = shifted - era * 146097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

fn parse_rfc3339_seconds(text: &str) -> i64 {
    let bytes = text.as_bytes();
    let year = number(bytes, 0, 4);
    let month = number(bytes, 5, 2);
    let day = number(bytes, 8, 2);
    let hour = number(bytes, 11, 2);
    let minute = number(bytes, 14, 2);
    let second = number(bytes, 17, 2);
    let local = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    if bytes[19] == b'Z' {
        return local;
    }
    let sign = if bytes[19] == b'+' { 1 } else { -1 };
    let offset = number(bytes, 20, 2) * 3_600 + number(bytes, 23, 2) * 60;
    local - sign * offset
}

fn main() -> io::Result<()> {
    let path = env::args().nth(1).expect("meetings path");
    let reader = BufReader::new(File::open(path)?);
    let mut meetings = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let (name, raw) = line.split_once('|').expect("meeting line");
        meetings.push((parse_rfc3339_seconds(raw), name.to_string()));
    }
    meetings.sort_by_key(|meeting| meeting.0);
    for (position, (seconds, name)) in meetings.iter().enumerate() {
        let days = seconds.div_euclid(86_400);
        let day_seconds = seconds.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        let hour = day_seconds / 3_600;
        let minute = day_seconds / 60 % 60;
        let second = day_seconds % 60;
        let weekday = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][(days + 3).rem_euclid(7) as usize];
        let gap = if position == 0 { "-".to_string() } else { ((seconds - meetings[position - 1].0) / 60).to_string() };
        println!("{} utc={:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z day={} gap={}", name, year, month, day, hour, minute, second, weekday, gap);
    }
    let span = (meetings.last().unwrap().0 - meetings.first().unwrap().0) / 60;
    println!("span {} minutes {}", span, meetings.len());
    Ok(())
}
