// D-TIMEDEPTH1/D-TIME-CALENDAR1: civil-time types and calendar math.
// Pure Rust, no external crates (I6). Proleptic Gregorian calendar, Unix time
// as UTC seconds, and a small TZif reader for IANA zoneinfo files.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JetDate {
    year: i64,
    month: i64,
    day: i64,
}
impl JetDate {
    pub(crate) fn is_leap(y: i64) -> bool {
        (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
    }
    pub(crate) fn days_in_month_of(y: i64, m: i64) -> i64 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if Self::is_leap(y) {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    }
    pub(crate) fn new(y: i64, m: i64, d: i64) -> Self {
        let month = m.clamp(1, 12);
        let day = d.clamp(1, Self::days_in_month_of(y, month));
        JetDate {
            year: y,
            month,
            day,
        }
    }
    // Days since 0001-01-01 (proleptic Gregorian).
    pub(crate) fn to_day_number(&self) -> i64 {
        let y = self.year - 1;
        365 * y + y / 4 - y / 100
            + y / 400
            + [0i64, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334][(self.month - 1) as usize]
            + if self.month > 2 && Self::is_leap(self.year) {
                1
            } else {
                0
            }
            + self.day
            - 1
    }
    pub(crate) fn from_day_number(mut n: i64) -> Self {
        let mut y = n / 365 + 1;
        loop {
            let start = JetDate::new(y, 1, 1).to_day_number();
            let next = JetDate::new(y + 1, 1, 1).to_day_number();
            if n >= start && n < next {
                break;
            }
            if n < start {
                y -= 1;
            } else {
                y += 1;
            }
        }
        n -= JetDate::new(y, 1, 1).to_day_number();
        let mut m = 1i64;
        while m < 12 && n >= Self::days_in_month_of(y, m) {
            n -= Self::days_in_month_of(y, m);
            m += 1;
        }
        JetDate::new(y, m, n + 1)
    }
    pub(crate) fn parse(s: &str) -> Result<JetDate, String> {
        let parts: Vec<&str> = s.splitn(3, '-').collect();
        if parts.len() != 3 {
            return Err(format!("invalid date: {}", s));
        }
        let y = parts[0]
            .parse::<i64>()
            .map_err(|_| format!("bad year: {}", parts[0]))?;
        let m = parts[1]
            .parse::<i64>()
            .map_err(|_| format!("bad month: {}", parts[1]))?;
        let d = parts[2]
            .parse::<i64>()
            .map_err(|_| format!("bad day: {}", parts[2]))?;
        if m < 1 || m > 12 || d < 1 || d > Self::days_in_month_of(y, m) {
            return Err(format!("date out of range: {}", s));
        }
        Ok(JetDate::new(y, m, d))
    }
    pub(crate) fn year(&self) -> i64 {
        self.year
    }
    pub(crate) fn month(&self) -> i64 {
        self.month
    }
    pub(crate) fn day(&self) -> i64 {
        self.day
    }
    pub(crate) fn add_days(&self, n: i64) -> JetDate {
        Self::from_day_number(self.to_day_number() + n)
    }
    pub(crate) fn add_months(&self, n: i64) -> JetDate {
        let total = self.month - 1 + n;
        let y = self.year + total / 12;
        let m = total % 12 + 1;
        let d = self.day.min(Self::days_in_month_of(y, m));
        JetDate::new(y, m, d)
    }
    pub(crate) fn diff_days(&self, other: &JetDate) -> i64 {
        self.to_day_number() - other.to_day_number()
    }
    pub(crate) fn weekday(&self) -> i64 {
        // Legacy D-TIMEDEPTH1 shape: 0=Sunday, 6=Saturday.
        (self.to_day_number() + 6) % 7
    }
    pub(crate) fn iso_weekday(&self) -> i64 {
        (self.to_day_number() % 7) + 1
    }
    pub(crate) fn day_of_year(&self) -> i64 {
        self.to_day_number() - JetDate::new(self.year, 1, 1).to_day_number() + 1
    }
    pub(crate) fn quarter_of_year(&self) -> i64 {
        (self.month - 1) / 3 + 1
    }
    pub(crate) fn is_leap_year(&self) -> bool {
        Self::is_leap(self.year)
    }
    pub(crate) fn days_in_month(&self) -> i64 {
        Self::days_in_month_of(self.year, self.month)
    }
    pub(crate) fn iso_week(&self) -> i64 {
        let thursday = self.add_days(4 - self.iso_weekday());
        ((thursday.to_day_number() - JetDate::new(thursday.year, 1, 1).to_day_number()) / 7) + 1
    }
    pub(crate) fn truncate(&self, unit: &String) -> JetDate {
        match unit.as_str() {
            "year" => JetDate::new(self.year, 1, 1),
            "month" => JetDate::new(self.year, self.month, 1),
            _ => self.clone(),
        }
    }
    pub(crate) fn replace(&self, year: i64, month: i64, day: i64) -> JetDate {
        JetDate::new(year, month, day)
    }
    pub(crate) fn add_period(&self, p: &JetPeriod) -> JetDate {
        self.add_months(p.years.saturating_mul(12).saturating_add(p.months))
            .add_days(p.days)
    }
    pub(crate) fn format_pattern(&self, pattern: &String) -> String {
        jet_time_format_pattern(pattern, self, &JetLocalTime::new(0, 0, 0), None)
    }
    pub(crate) fn to_string_fmt(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
    pub(crate) fn today_utc() -> JetDate {
        // Seconds since Unix epoch ÷ 86400 days.
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;
        let days_since_1970 = secs / 86400;
        let epoch = JetDate::new(1970, 1, 1).to_day_number();
        JetDate::from_day_number(epoch + days_since_1970)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JetLocalTime {
    hour: i64,
    minute: i64,
    second: i64,
}
impl JetLocalTime {
    pub(crate) fn new(hour: i64, minute: i64, second: i64) -> Self {
        JetLocalTime {
            hour: hour.clamp(0, 23),
            minute: minute.clamp(0, 59),
            second: second.clamp(0, 59),
        }
    }
    pub(crate) fn parse(s: &str) -> Result<JetLocalTime, String> {
        let parts: Vec<&str> = s.splitn(3, ':').collect();
        if parts.len() != 3 {
            return Err(format!("invalid time: {}", s));
        }
        let h = parts[0]
            .parse::<i64>()
            .map_err(|_| format!("bad hour: {}", parts[0]))?;
        let m = parts[1]
            .parse::<i64>()
            .map_err(|_| format!("bad minute: {}", parts[1]))?;
        let sec = parts[2]
            .parse::<i64>()
            .map_err(|_| format!("bad second: {}", parts[2]))?;
        if h < 0 || h > 23 || m < 0 || m > 59 || sec < 0 || sec > 59 {
            return Err(format!("time out of range: {}", s));
        }
        Ok(Self::new(h, m, sec))
    }
    pub(crate) fn hour(&self) -> i64 {
        self.hour
    }
    pub(crate) fn minute(&self) -> i64 {
        self.minute
    }
    pub(crate) fn second(&self) -> i64 {
        self.second
    }
    pub(crate) fn to_seconds(&self) -> i64 {
        self.hour * 3600 + self.minute * 60 + self.second
    }
    pub(crate) fn to_string_fmt(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct JetPeriod {
    years: i64,
    months: i64,
    days: i64,
}
impl JetPeriod {
    pub(crate) fn new(years: i64, months: i64, days: i64) -> Self {
        JetPeriod {
            years,
            months,
            days,
        }
    }
    pub(crate) fn days(n: i64) -> Self {
        Self::new(0, 0, n)
    }
    pub(crate) fn months(n: i64) -> Self {
        Self::new(0, n, 0)
    }
    pub(crate) fn years(n: i64) -> Self {
        Self::new(n, 0, 0)
    }
    pub(crate) fn to_string_fmt(&self) -> String {
        format!("P{}Y{}M{}D", self.years, self.months, self.days)
    }
    pub(crate) fn components(&self) -> (i64, i64, i64) {
        (self.years, self.months, self.days)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct JetInstant {
    start: std::time::Instant,
}

impl JetInstant {
    pub(crate) fn now() -> Self {
        JetInstant {
            start: std::time::Instant::now(),
        }
    }
    pub(crate) fn elapsed_millis(&self) -> i64 {
        self.start.elapsed().as_millis() as i64
    }
    pub(crate) fn elapsed_nanos(&self) -> i64 {
        self.start.elapsed().as_nanos().min(i64::MAX as u128) as i64
    }

    pub(crate) fn to_string_fmt(&self) -> String {
        "Instant".to_string()
    }
}
impl PartialEq for JetInstant {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JetDateTime {
    secs: i64,
    nanos: u32,
} // seconds + nanosecond remainder since Unix epoch (UTC)
impl JetDateTime {
    pub(crate) fn from_timestamp(secs: i64) -> Self {
        JetDateTime { secs, nanos: 0 }
    }
    pub(crate) fn from_timestamp_ns(secs: i64, nanos: u32) -> Self {
        let mut secs = secs;
        let mut nanos = nanos;
        if nanos >= 1_000_000_000 {
            secs = secs.saturating_add((nanos / 1_000_000_000) as i64);
            nanos %= 1_000_000_000;
        }
        JetDateTime { secs, nanos }
    }
    pub(crate) fn from_parts(
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
        nanos: u32,
    ) -> Self {
        let date = JetDate::new(year, month, day);
        let time = JetLocalTime::new(hour, minute, second);
        Self::from_timestamp_ns(jet_time_utc_from_parts(&date, &time), nanos)
    }
    pub(crate) fn now() -> Self {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        JetDateTime {
            secs: d.as_secs() as i64,
            nanos: d.subsec_nanos(),
        }
    }
    pub(crate) fn date(&self) -> JetDate {
        let days = self.secs.div_euclid(86400);
        let epoch = JetDate::new(1970, 1, 1).to_day_number();
        JetDate::from_day_number(epoch + days)
    }
    pub(crate) fn time(&self) -> JetLocalTime {
        let sec = self.secs.rem_euclid(86400);
        JetLocalTime::new(sec / 3600, (sec / 60) % 60, sec % 60)
    }
    pub(crate) fn hour(&self) -> i64 {
        self.time().hour
    }
    pub(crate) fn minute(&self) -> i64 {
        self.time().minute
    }
    pub(crate) fn second(&self) -> i64 {
        self.secs.rem_euclid(60)
    }
    pub(crate) fn millisecond(&self) -> i64 {
        (self.nanos / 1_000_000) as i64
    }
    pub(crate) fn microsecond(&self) -> i64 {
        (self.nanos / 1_000) as i64
    }
    pub(crate) fn nanosecond(&self) -> i64 {
        self.nanos as i64
    }
    pub(crate) fn to_timestamp(&self) -> i64 {
        self.secs
    }
    pub(crate) fn to_unix_ms(&self) -> i64 {
        self.secs
            .saturating_mul(1000)
            .saturating_add((self.nanos / 1_000_000) as i64)
    }
    pub(crate) fn from_unix_ms(ms: i64) -> Self {
        let secs = ms.div_euclid(1000);
        let nanos = (ms.rem_euclid(1000) as u32).saturating_mul(1_000_000);
        JetDateTime { secs, nanos }
    }
    pub(crate) fn parse_rfc3339(s: &str) -> Result<Self, String> {
        let (date_part, rest) = s
            .split_once('T')
            .ok_or_else(|| format!("invalid RFC3339 datetime: {}", s))?;
        let date = JetDate::parse(date_part)?;
        let zone_pos = rest
            .find('Z')
            .or_else(|| rest.rfind('+'))
            .or_else(|| {
                rest.get(1..)
                    .and_then(|tail| tail.rfind('-').map(|i| i + 1))
            })
            .ok_or_else(|| format!("RFC3339 datetime needs Z or an offset: {}", s))?;
        let (time_part, zone_part) = rest.split_at(zone_pos);
        let (clean_time, frac) = match time_part.split_once('.') {
            Some((t, f)) => (t, Some(f)),
            None => (time_part, None),
        };
        let time = JetLocalTime::parse(clean_time)?;
        let mut nanos = 0u32;
        if let Some(f) = frac {
            let digits: String = f.chars().take(9).filter(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                let padded = format!("{:0<9}", digits);
                nanos = padded.parse::<u32>().unwrap_or(0);
            }
        }
        let offset = if zone_part == "Z" {
            0
        } else {
            let sign = if zone_part.starts_with('-') { -1 } else { 1 };
            let z = &zone_part[1..];
            let (hh, mm) = z
                .split_once(':')
                .ok_or_else(|| format!("bad RFC3339 offset: {}", zone_part))?;
            let h = hh
                .parse::<i64>()
                .map_err(|_| format!("bad RFC3339 offset hour: {}", hh))?;
            let m = mm
                .parse::<i64>()
                .map_err(|_| format!("bad RFC3339 offset minute: {}", mm))?;
            sign * (h * 3600 + m * 60)
        };
        Ok(JetDateTime {
            secs: jet_time_utc_from_parts(&date, &time) - offset,
            nanos,
        })
    }
    pub(crate) fn format_rfc3339(&self) -> String {
        let d = self.date();
        let t = self.time();
        if self.nanos == 0 {
            format!("{}T{}Z", d.to_string_fmt(), t.to_string_fmt())
        } else {
            format!(
                "{}T{}.{:09}Z",
                d.to_string_fmt(),
                t.to_string_fmt(),
                self.nanos
            )
        }
    }
    pub(crate) fn format_pattern(&self, pattern: &String) -> String {
        jet_time_format_pattern(pattern, &self.date(), &self.time(), None)
    }
    pub(crate) fn plus_duration_ns(&self, ns: i64) -> JetDateTime {
        let total = (self.secs as i128)
            .saturating_mul(1_000_000_000)
            .saturating_add(self.nanos as i128)
            .saturating_add(ns as i128);
        let secs = total.div_euclid(1_000_000_000) as i64;
        let nanos = total.rem_euclid(1_000_000_000) as u32;
        JetDateTime { secs, nanos }
    }
    pub(crate) fn difference_ns(&self, other: &JetDateTime) -> i64 {
        let a = (self.secs as i128)
            .saturating_mul(1_000_000_000)
            .saturating_add(self.nanos as i128);
        let b = (other.secs as i128)
            .saturating_mul(1_000_000_000)
            .saturating_add(other.nanos as i128);
        (a - b).clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }
    pub(crate) fn truncate(&self, unit: &String) -> JetDateTime {
        match unit.as_str() {
            "day" => JetDateTime {
                secs: self.secs.div_euclid(86400) * 86400,
                nanos: 0,
            },
            "hour" => JetDateTime {
                secs: self.secs.div_euclid(3600) * 3600,
                nanos: 0,
            },
            "minute" => JetDateTime {
                secs: self.secs.div_euclid(60) * 60,
                nanos: 0,
            },
            "second" => JetDateTime {
                secs: self.secs,
                nanos: 0,
            },
            "millisecond" => JetDateTime {
                secs: self.secs,
                nanos: (self.nanos / 1_000_000) * 1_000_000,
            },
            "microsecond" => JetDateTime {
                secs: self.secs,
                nanos: (self.nanos / 1_000) * 1_000,
            },
            _ => self.clone(),
        }
    }
    pub(crate) fn floor(&self, unit: &String) -> JetDateTime {
        self.truncate(unit)
    }
    pub(crate) fn ceil(&self, unit: &String) -> JetDateTime {
        let floored = self.truncate(unit);
        if &floored == self {
            return floored;
        }
        match unit.as_str() {
            "day" => floored.plus_duration_ns(86400 * 1_000_000_000),
            "hour" => floored.plus_duration_ns(3600 * 1_000_000_000),
            "minute" => floored.plus_duration_ns(60 * 1_000_000_000),
            "second" => floored.plus_duration_ns(1_000_000_000),
            "millisecond" => floored.plus_duration_ns(1_000_000),
            "microsecond" => floored.plus_duration_ns(1_000),
            _ => self.clone(),
        }
    }
    pub(crate) fn round(&self, unit: &String) -> JetDateTime {
        let size_ns: i64 = match unit.as_str() {
            "day" => 86400 * 1_000_000_000,
            "hour" => 3600 * 1_000_000_000,
            "minute" => 60 * 1_000_000_000,
            "second" => 1_000_000_000,
            "millisecond" => 1_000_000,
            "microsecond" => 1_000,
            _ => return self.clone(),
        };
        let total = (self.secs as i128)
            .saturating_mul(1_000_000_000)
            .saturating_add(self.nanos as i128);
        let rounded = (total + (size_ns as i128) / 2).div_euclid(size_ns as i128)
            * (size_ns as i128);
        let secs = rounded.div_euclid(1_000_000_000) as i64;
        let nanos = rounded.rem_euclid(1_000_000_000) as u32;
        JetDateTime { secs, nanos }
    }
    pub(crate) fn replace(
        &self,
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
    ) -> JetDateTime {
        Self::from_parts(year, month, day, hour, minute, second, self.nanos)
    }
    pub(crate) fn in_zone(&self, zone: &JetZone) -> JetZonedDateTime {
        JetZonedDateTime {
            instant: self.clone(),
            zone: zone.clone(),
        }
    }
    pub(crate) fn to_string_fmt(&self) -> String {
        let d = self.date();
        format!(
            "{} {:02}:{:02}:{:02} UTC",
            d.to_string_fmt(),
            self.hour(),
            self.minute(),
            self.second()
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JetTtInfo {
    offset: i64,
    is_dst: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JetZone {
    name: String,
    transitions: Vec<(i64, usize)>,
    infos: Vec<JetTtInfo>,
}
impl JetZone {
    pub(crate) fn to_string_fmt(&self) -> String {
        self.name.clone()
    }

    pub(crate) fn utc() -> Self {
        JetZone {
            name: "UTC".to_string(),
            transitions: Vec::new(),
            infos: vec![JetTtInfo {
                offset: 0,
                is_dst: false,
            }],
        }
    }
    pub(crate) fn named(name: &String) -> Result<Self, String> {
        if name == "UTC" || name == "Etc/UTC" || name == "Z" {
            return Ok(Self::utc());
        }
        if name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
            return Err(format!("invalid time zone name: {}", name));
        }
        let rel = name
            .trim_start_matches("posix/")
            .trim_start_matches("right/");
        let mut roots = Vec::new();
        if let Some(dir) = std::env::var_os("JET_TZDB_DIR") {
            roots.push(std::path::PathBuf::from(dir));
        }
        if let Some(dir) = std::env::var_os("TZDIR") {
            roots.push(std::path::PathBuf::from(dir));
        }
        if let Some(root) = std::env::var_os("JET_ROOT") {
            roots.push(std::path::PathBuf::from(root).join("corelib/tzdb"));
        }
        roots.push(std::path::PathBuf::from("corelib/tzdb"));
        roots.push(std::path::PathBuf::from("/usr/share/zoneinfo"));
        roots.push(std::path::PathBuf::from("/usr/share/lib/zoneinfo"));
        roots.push(std::path::PathBuf::from("/etc/zoneinfo"));
        for base in roots {
            let path = base.join(rel);
            if let Ok(bytes) = std::fs::read(&path) {
                return Self::parse_tzif(name.clone(), &bytes);
            }
        }
        Err(format!(
            "unknown IANA time zone: {}; set JET_TZDB_DIR or TZDIR to an IANA TZif database",
            name
        ))
    }
    pub(crate) fn parse_tzif(name: String, bytes: &[u8]) -> Result<Self, String> {
        fn be_u32(bytes: &[u8], i: usize) -> Result<u32, String> {
            let chunk = bytes
                .get(i..i + 4)
                .ok_or_else(|| "truncated tzif".to_string())?;
            Ok(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        }
        fn be_i32(bytes: &[u8], i: usize) -> Result<i32, String> {
            Ok(be_u32(bytes, i)? as i32)
        }
        fn be_i64(bytes: &[u8], i: usize) -> Result<i64, String> {
            let c = bytes
                .get(i..i + 8)
                .ok_or_else(|| "truncated tzif".to_string())?;
            Ok(i64::from_be_bytes([
                c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7],
            ]))
        }
        fn header(bytes: &[u8], base: usize) -> Result<(u8, [usize; 6]), String> {
            if bytes.get(base..base + 4) != Some(b"TZif") {
                return Err("invalid tzif header".to_string());
            }
            let version = *bytes.get(base + 4).unwrap_or(&0);
            let mut counts = [0usize; 6];
            for i in 0..6 {
                counts[i] = be_u32(bytes, base + 20 + i * 4)? as usize;
            }
            Ok((version, counts))
        }
        let (version, c1) = header(bytes, 0)?;
        let block32 = c1[3] * 4 + c1[3] + c1[4] * 6 + c1[5] + c1[2] * 8 + c1[0] + c1[1];
        let mut base = 44;
        let mut wide = false;
        if version == b'2' || version == b'3' || version == b'4' {
            base = 44 + block32;
            let _ = header(bytes, base)?;
            base += 44;
            wide = true;
        }
        let counts = if wide {
            header(bytes, base - 44)?.1
        } else {
            c1
        };
        let timecnt = counts[3];
        let typecnt = counts[4].max(1);
        let time_size = if wide { 8 } else { 4 };
        let mut pos = base;
        let mut times = Vec::new();
        for _ in 0..timecnt {
            let t = if wide {
                be_i64(bytes, pos)?
            } else {
                be_i32(bytes, pos)? as i64
            };
            times.push(t);
            pos += time_size;
        }
        let idxs = bytes
            .get(pos..pos + timecnt)
            .ok_or_else(|| "truncated tzif index".to_string())?;
        pos += timecnt;
        let mut infos = Vec::new();
        for _ in 0..typecnt {
            let offset = be_i32(bytes, pos)? as i64;
            let is_dst = *bytes.get(pos + 4).unwrap_or(&0) != 0;
            infos.push(JetTtInfo { offset, is_dst });
            pos += 6;
        }
        if infos.is_empty() {
            infos.push(JetTtInfo {
                offset: 0,
                is_dst: false,
            });
        }
        let mut transitions = Vec::new();
        for (t, idx) in times.into_iter().zip(idxs.iter().copied()) {
            transitions.push((t, (idx as usize).min(infos.len() - 1)));
        }
        Ok(JetZone {
            name,
            transitions,
            infos,
        })
    }
    pub(crate) fn name(&self) -> String {
        self.name.clone()
    }
    pub(crate) fn offset_at_utc(&self, secs: i64) -> i64 {
        self.info_at_utc(secs).offset
    }
    pub(crate) fn info_at_utc(&self, secs: i64) -> &JetTtInfo {
        if self.transitions.is_empty() {
            return &self.infos[0];
        }
        let mut lo = 0usize;
        let mut hi = self.transitions.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.transitions[mid].0 <= secs {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let idx = if lo == 0 {
            self.transitions.first().map(|(_, i)| *i).unwrap_or(0)
        } else {
            self.transitions[lo - 1].1
        };
        &self.infos[idx]
    }
    pub(crate) fn local_parts(&self, secs: i64) -> (JetDate, JetLocalTime, i64) {
        let offset = self.offset_at_utc(secs);
        let local = JetDateTime::from_timestamp(secs.saturating_add(offset));
        (local.date(), local.time(), offset)
    }
    pub(crate) fn local_to_utc(&self, date: &JetDate, time: &JetLocalTime) -> i64 {
        let mut guess = jet_time_utc_from_parts(date, time);
        for _ in 0..4 {
            let next =
                jet_time_utc_from_parts(date, time).saturating_sub(self.offset_at_utc(guess));
            if next == guess {
                break;
            }
            guess = next;
        }
        guess
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JetZonedDateTime {
    instant: JetDateTime,
    zone: JetZone,
}
impl JetZonedDateTime {
    pub(crate) fn now(zone: &JetZone) -> Self {
        JetDateTime::now().in_zone(zone)
    }
    pub(crate) fn from_local(date: &JetDate, time: &JetLocalTime, zone: &JetZone) -> Self {
        JetZonedDateTime {
            instant: JetDateTime::from_timestamp(zone.local_to_utc(date, time)),
            zone: zone.clone(),
        }
    }
    pub(crate) fn date(&self) -> JetDate {
        self.zone.local_parts(self.instant.secs).0
    }
    pub(crate) fn time(&self) -> JetLocalTime {
        self.zone.local_parts(self.instant.secs).1
    }
    pub(crate) fn offset_seconds(&self) -> i64 {
        self.zone.local_parts(self.instant.secs).2
    }
    pub(crate) fn is_dst(&self) -> bool {
        self.zone.info_at_utc(self.instant.secs).is_dst
    }
    pub(crate) fn to_datetime(&self) -> JetDateTime {
        self.instant.clone()
    }
    pub(crate) fn zone(&self) -> JetZone {
        self.zone.clone()
    }
    pub(crate) fn add_duration_ns(&self, ns: i64) -> JetZonedDateTime {
        JetZonedDateTime {
            instant: self.instant.plus_duration_ns(ns),
            zone: self.zone.clone(),
        }
    }
    pub(crate) fn add_period(&self, p: &JetPeriod) -> JetZonedDateTime {
        let date = self.date().add_period(p);
        let time = self.time();
        JetZonedDateTime::from_local(&date, &time, &self.zone)
    }
    pub(crate) fn format_pattern(&self, pattern: &String) -> String {
        let date = self.date();
        let time = self.time();
        jet_time_format_pattern(
            pattern,
            &date,
            &time,
            Some((&self.zone, self.offset_seconds())),
        )
    }
    pub(crate) fn to_string_fmt(&self) -> String {
        let off = self.offset_seconds();
        format!(
            "{} {} {} ({})",
            self.date().to_string_fmt(),
            self.time().to_string_fmt(),
            self.zone.name,
            jet_time_offset_string(off)
        )
    }
}

pub(crate) fn jet_time_utc_from_parts(date: &JetDate, time: &JetLocalTime) -> i64 {
    let epoch = JetDate::new(1970, 1, 1).to_day_number();
    (date.to_day_number() - epoch)
        .saturating_mul(86400)
        .saturating_add(time.to_seconds())
}

pub(crate) fn jet_time_offset_string(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let abs = offset.abs();
    format!("{}{:02}:{:02}", sign, abs / 3600, (abs / 60) % 60)
}

pub(crate) fn jet_time_format_pattern(
    pattern: &String,
    date: &JetDate,
    time: &JetLocalTime,
    zone: Option<(&JetZone, i64)>,
) -> String {
    let mut out = pattern.clone();
    let weekday =
        ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][(date.iso_weekday() - 1) as usize];
    out = out.replace("yyyy", &format!("{:04}", date.year));
    out = out.replace("DDD", &format!("{:03}", date.day_of_year()));
    out = out.replace("EEE", weekday);
    out = out.replace("MM", &format!("{:02}", date.month));
    out = out.replace("dd", &format!("{:02}", date.day));
    out = out.replace("HH", &format!("{:02}", time.hour));
    out = out.replace("mm", &format!("{:02}", time.minute));
    out = out.replace("ss", &format!("{:02}", time.second));
    if let Some((z, off)) = zone {
        out = out.replace("VV", &z.name);
        out = out.replace("XXX", &jet_time_offset_string(off));
    }
    out
}
