use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader};

fn parse_rfc3339_seconds(text: &str) -> i64 {
    let bytes = text.as_bytes();
    let digit = |start: usize, len: usize| -> i64 {
        bytes[start..start + len]
            .iter()
            .fold(0, |value, byte| value * 10 + i64::from(byte - b'0'))
    };
    let year = digit(0, 4);
    let month = digit(5, 2);
    let day = digit(8, 2);
    let hour = digit(11, 2);
    let minute = digit(14, 2);
    let second = digit(17, 2);
    let days_before_year = |year: i64| 365 * (year - 1970) + (year - 1969) / 4 - (year - 1901) / 100 + (year - 1601) / 400;
    let month_days = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = days_before_year(year) + month_days[(month - 1) as usize] + day - 1 + i64::from(leap && month > 2);
    days * 86_400 + hour * 3_600 + minute * 60 + second
}

fn main() -> io::Result<()> {
    let path = env::args().nth(1).unwrap_or_else(|| "app.log".to_string());
    let reader = BufReader::new(File::open(path)?);
    let levels = ["DEBUG", "INFO", "WARN", "ERROR"];
    let components = ["api", "auth", "cache", "db", "jobs", "mailer", "payments", "queue", "search", "storage", "worker", "web"];
    let mut level_counts = [0_i64; 4];
    let mut error_counts = [0_i64; 12];
    let mut first_text = String::new();
    let mut last_text = String::new();
    let mut first_seconds = 0_i64;
    let mut last_seconds = 0_i64;
    let mut total = 0_i64;
    for line in reader.lines() {
        let line = line?;
        let mut fields = line.splitn(4, ' ');
        let timestamp_text = fields.next().unwrap();
        let level = fields.next().unwrap();
        let component = fields.next().unwrap();
        let seconds = parse_rfc3339_seconds(timestamp_text);
        if total == 0 {
            first_text = timestamp_text.to_string();
            first_seconds = seconds;
        }
        last_text = timestamp_text.to_string();
        last_seconds = seconds;
        let level_index = levels.iter().position(|item| *item == level).unwrap();
        level_counts[level_index] += 1;
        if level == "ERROR" {
            let component_index = components.iter().position(|item| *item == component).unwrap();
            error_counts[component_index] += 1;
        }
        total += 1;
    }
    for (index, level) in levels.iter().enumerate() {
        println!("{} {}", level, level_counts[index]);
    }
    println!("top-error-components:");
    let mut top = Vec::new();
    for _ in 0..3 {
        let mut best = None;
        for (index, component) in components.iter().enumerate() {
            if top.contains(&index) {
                continue;
            }
            if best.map_or(true, |best_index| error_counts[index] > error_counts[best_index] || (error_counts[index] == error_counts[best_index] && component < &components[best_index])) {
                best = Some(index);
            }
        }
        let index = best.unwrap();
        top.push(index);
        println!("{} {}", error_counts[index], components[index]);
    }
    println!("span {} .. {} ({}s)", first_text, last_text, last_seconds - first_seconds);
    Ok(())
}
