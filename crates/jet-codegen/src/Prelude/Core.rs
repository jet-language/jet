// D-TIMEDEPTH1/D-TIME-CALENDAR1: civil-time types and calendar math.
// Pure Rust, no external crates (I6). Proleptic Gregorian calendar, Unix time
// as UTC seconds, and a small TZif reader for IANA zoneinfo files.
#[derive(Clone, Debug, PartialEq)]
struct JetDate {
    year: i64,
    month: i64,
    day: i64,
}
impl JetDate {
    fn is_leap(y: i64) -> bool {
        (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
    }
    fn days_in_month_of(y: i64, m: i64) -> i64 {
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
    fn new(y: i64, m: i64, d: i64) -> Self {
        let month = m.clamp(1, 12);
        let day = d.clamp(1, Self::days_in_month_of(y, month));
        JetDate {
            year: y,
            month,
            day,
        }
    }
    // Days since 0001-01-01 (proleptic Gregorian).
    fn to_day_number(&self) -> i64 {
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
    fn from_day_number(mut n: i64) -> Self {
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
    fn parse(s: &str) -> Result<JetDate, String> {
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
    fn year(&self) -> i64 {
        self.year
    }
    fn month(&self) -> i64 {
        self.month
    }
    fn day(&self) -> i64 {
        self.day
    }
    fn add_days(&self, n: i64) -> JetDate {
        Self::from_day_number(self.to_day_number() + n)
    }
    fn add_months(&self, n: i64) -> JetDate {
        let total = self.month - 1 + n;
        let y = self.year + total / 12;
        let m = total % 12 + 1;
        let d = self.day.min(Self::days_in_month_of(y, m));
        JetDate::new(y, m, d)
    }
    fn diff_days(&self, other: &JetDate) -> i64 {
        self.to_day_number() - other.to_day_number()
    }
    fn weekday(&self) -> i64 {
        // Legacy D-TIMEDEPTH1 shape: 0=Sunday, 6=Saturday.
        (self.to_day_number() + 6) % 7
    }
    fn iso_weekday(&self) -> i64 {
        (self.to_day_number() % 7) + 1
    }
    fn day_of_year(&self) -> i64 {
        self.to_day_number() - JetDate::new(self.year, 1, 1).to_day_number() + 1
    }
    fn quarter_of_year(&self) -> i64 {
        (self.month - 1) / 3 + 1
    }
    fn is_leap_year(&self) -> bool {
        Self::is_leap(self.year)
    }
    fn days_in_month(&self) -> i64 {
        Self::days_in_month_of(self.year, self.month)
    }
    fn iso_week(&self) -> i64 {
        let thursday = self.add_days(4 - self.iso_weekday());
        ((thursday.to_day_number() - JetDate::new(thursday.year, 1, 1).to_day_number()) / 7) + 1
    }
    fn truncate(&self, unit: &String) -> JetDate {
        match unit.as_str() {
            "year" => JetDate::new(self.year, 1, 1),
            "month" => JetDate::new(self.year, self.month, 1),
            _ => self.clone(),
        }
    }
    fn replace(&self, year: i64, month: i64, day: i64) -> JetDate {
        JetDate::new(year, month, day)
    }
    fn add_period(&self, p: &JetPeriod) -> JetDate {
        self.add_months(p.years.saturating_mul(12).saturating_add(p.months))
            .add_days(p.days)
    }
    fn format_pattern(&self, pattern: &String) -> String {
        jet_time_format_pattern(pattern, self, &JetLocalTime::new(0, 0, 0), None)
    }
    fn to_string_fmt(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
    fn today_utc() -> JetDate {
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
impl JetShow for JetDate {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct JetLocalTime {
    hour: i64,
    minute: i64,
    second: i64,
}
impl JetLocalTime {
    fn new(hour: i64, minute: i64, second: i64) -> Self {
        JetLocalTime {
            hour: hour.clamp(0, 23),
            minute: minute.clamp(0, 59),
            second: second.clamp(0, 59),
        }
    }
    fn parse(s: &str) -> Result<JetLocalTime, String> {
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
    fn hour(&self) -> i64 {
        self.hour
    }
    fn minute(&self) -> i64 {
        self.minute
    }
    fn second(&self) -> i64 {
        self.second
    }
    fn to_seconds(&self) -> i64 {
        self.hour * 3600 + self.minute * 60 + self.second
    }
    fn to_string_fmt(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}
impl JetShow for JetLocalTime {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct JetPeriod {
    years: i64,
    months: i64,
    days: i64,
}
impl JetPeriod {
    fn new(years: i64, months: i64, days: i64) -> Self {
        JetPeriod {
            years,
            months,
            days,
        }
    }
    fn days(n: i64) -> Self {
        Self::new(0, 0, n)
    }
    fn months(n: i64) -> Self {
        Self::new(0, n, 0)
    }
    fn years(n: i64) -> Self {
        Self::new(n, 0, 0)
    }
    fn to_string_fmt(&self) -> String {
        format!("P{}Y{}M{}D", self.years, self.months, self.days)
    }
}
impl JetShow for JetPeriod {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

#[derive(Clone, Debug)]
struct JetInstant {
    start: std::time::Instant,
}
impl JetInstant {
    fn now() -> Self {
        JetInstant {
            start: std::time::Instant::now(),
        }
    }
    fn elapsed_millis(&self) -> i64 {
        self.start.elapsed().as_millis() as i64
    }
    fn elapsed_nanos(&self) -> i64 {
        self.start.elapsed().as_nanos().min(i64::MAX as u128) as i64
    }
}
impl PartialEq for JetInstant {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start
    }
}
impl JetShow for JetInstant {
    fn jet_show(&self) -> String {
        "Instant".to_string()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct JetDateTime {
    secs: i64,
    nanos: u32,
} // seconds + nanosecond remainder since Unix epoch (UTC)
impl JetDateTime {
    fn from_timestamp(secs: i64) -> Self {
        JetDateTime { secs, nanos: 0 }
    }
    fn from_timestamp_ns(secs: i64, nanos: u32) -> Self {
        let mut secs = secs;
        let mut nanos = nanos;
        if nanos >= 1_000_000_000 {
            secs = secs.saturating_add((nanos / 1_000_000_000) as i64);
            nanos %= 1_000_000_000;
        }
        JetDateTime { secs, nanos }
    }
    fn from_parts(
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
    fn now() -> Self {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        JetDateTime {
            secs: d.as_secs() as i64,
            nanos: d.subsec_nanos(),
        }
    }
    fn date(&self) -> JetDate {
        let days = self.secs.div_euclid(86400);
        let epoch = JetDate::new(1970, 1, 1).to_day_number();
        JetDate::from_day_number(epoch + days)
    }
    fn time(&self) -> JetLocalTime {
        let sec = self.secs.rem_euclid(86400);
        JetLocalTime::new(sec / 3600, (sec / 60) % 60, sec % 60)
    }
    fn hour(&self) -> i64 {
        self.time().hour
    }
    fn minute(&self) -> i64 {
        self.time().minute
    }
    fn second(&self) -> i64 {
        self.secs.rem_euclid(60)
    }
    fn millisecond(&self) -> i64 {
        (self.nanos / 1_000_000) as i64
    }
    fn microsecond(&self) -> i64 {
        (self.nanos / 1_000) as i64
    }
    fn nanosecond(&self) -> i64 {
        self.nanos as i64
    }
    fn to_timestamp(&self) -> i64 {
        self.secs
    }
    fn to_unix_ms(&self) -> i64 {
        self.secs
            .saturating_mul(1000)
            .saturating_add((self.nanos / 1_000_000) as i64)
    }
    fn from_unix_ms(ms: i64) -> Self {
        let secs = ms.div_euclid(1000);
        let nanos = (ms.rem_euclid(1000) as u32).saturating_mul(1_000_000);
        JetDateTime { secs, nanos }
    }
    fn parse_rfc3339(s: &str) -> Result<Self, String> {
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
    fn format_rfc3339(&self) -> String {
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
    fn format_pattern(&self, pattern: &String) -> String {
        jet_time_format_pattern(pattern, &self.date(), &self.time(), None)
    }
    fn plus_duration_ns(&self, ns: i64) -> JetDateTime {
        let total = (self.secs as i128)
            .saturating_mul(1_000_000_000)
            .saturating_add(self.nanos as i128)
            .saturating_add(ns as i128);
        let secs = total.div_euclid(1_000_000_000) as i64;
        let nanos = total.rem_euclid(1_000_000_000) as u32;
        JetDateTime { secs, nanos }
    }
    fn difference_ns(&self, other: &JetDateTime) -> i64 {
        let a = (self.secs as i128)
            .saturating_mul(1_000_000_000)
            .saturating_add(self.nanos as i128);
        let b = (other.secs as i128)
            .saturating_mul(1_000_000_000)
            .saturating_add(other.nanos as i128);
        (a - b).clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }
    fn truncate(&self, unit: &String) -> JetDateTime {
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
    fn floor(&self, unit: &String) -> JetDateTime {
        self.truncate(unit)
    }
    fn ceil(&self, unit: &String) -> JetDateTime {
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
    fn round(&self, unit: &String) -> JetDateTime {
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
    fn replace(
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
    fn in_zone(&self, zone: &JetZone) -> JetZonedDateTime {
        JetZonedDateTime {
            instant: self.clone(),
            zone: zone.clone(),
        }
    }
    fn to_string_fmt(&self) -> String {
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
impl JetShow for JetDateTime {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct JetTtInfo {
    offset: i64,
    is_dst: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct JetZone {
    name: String,
    transitions: Vec<(i64, usize)>,
    infos: Vec<JetTtInfo>,
}
impl JetZone {
    fn utc() -> Self {
        JetZone {
            name: "UTC".to_string(),
            transitions: Vec::new(),
            infos: vec![JetTtInfo {
                offset: 0,
                is_dst: false,
            }],
        }
    }
    fn named(name: &String) -> Result<Self, String> {
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
    fn parse_tzif(name: String, bytes: &[u8]) -> Result<Self, String> {
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
    fn name(&self) -> String {
        self.name.clone()
    }
    fn offset_at_utc(&self, secs: i64) -> i64 {
        self.info_at_utc(secs).offset
    }
    fn info_at_utc(&self, secs: i64) -> &JetTtInfo {
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
    fn local_parts(&self, secs: i64) -> (JetDate, JetLocalTime, i64) {
        let offset = self.offset_at_utc(secs);
        let local = JetDateTime::from_timestamp(secs.saturating_add(offset));
        (local.date(), local.time(), offset)
    }
    fn local_to_utc(&self, date: &JetDate, time: &JetLocalTime) -> i64 {
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
impl JetShow for JetZone {
    fn jet_show(&self) -> String {
        self.name.clone()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct JetZonedDateTime {
    instant: JetDateTime,
    zone: JetZone,
}
impl JetZonedDateTime {
    fn now(zone: &JetZone) -> Self {
        JetDateTime::now().in_zone(zone)
    }
    fn from_local(date: &JetDate, time: &JetLocalTime, zone: &JetZone) -> Self {
        JetZonedDateTime {
            instant: JetDateTime::from_timestamp(zone.local_to_utc(date, time)),
            zone: zone.clone(),
        }
    }
    fn date(&self) -> JetDate {
        self.zone.local_parts(self.instant.secs).0
    }
    fn time(&self) -> JetLocalTime {
        self.zone.local_parts(self.instant.secs).1
    }
    fn offset_seconds(&self) -> i64 {
        self.zone.local_parts(self.instant.secs).2
    }
    fn is_dst(&self) -> bool {
        self.zone.info_at_utc(self.instant.secs).is_dst
    }
    fn to_datetime(&self) -> JetDateTime {
        self.instant.clone()
    }
    fn zone(&self) -> JetZone {
        self.zone.clone()
    }
    fn add_duration_ns(&self, ns: i64) -> JetZonedDateTime {
        JetZonedDateTime {
            instant: self.instant.plus_duration_ns(ns),
            zone: self.zone.clone(),
        }
    }
    fn add_period(&self, p: &JetPeriod) -> JetZonedDateTime {
        let date = self.date().add_period(p);
        let time = self.time();
        JetZonedDateTime::from_local(&date, &time, &self.zone)
    }
    fn format_pattern(&self, pattern: &String) -> String {
        let date = self.date();
        let time = self.time();
        jet_time_format_pattern(
            pattern,
            &date,
            &time,
            Some((&self.zone, self.offset_seconds())),
        )
    }
    fn to_string_fmt(&self) -> String {
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
impl JetShow for JetZonedDateTime {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

fn jet_time_utc_from_parts(date: &JetDate, time: &JetLocalTime) -> i64 {
    let epoch = JetDate::new(1970, 1, 1).to_day_number();
    (date.to_day_number() - epoch)
        .saturating_mul(86400)
        .saturating_add(time.to_seconds())
}

fn jet_time_offset_string(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let abs = offset.abs();
    format!("{}{:02}:{:02}", sign, abs / 3600, (abs / 60) % 60)
}

fn jet_time_format_pattern(
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

// D-PARCAPTURE1=D: one bounded indexed engine for every explicit parallel
// collection adapter. Chunk boundaries are fixed so scheduling cannot affect
// result order or `para_fold`'s merge tree; the number of worker threads is
// bounded by the host's available parallelism.
const JET_PARA_CHUNK_ITEMS: usize = 64;

struct JetParaFailure {
    index: usize,
    payload: Box<dyn std::any::Any + Send + 'static>,
}

enum JetParaRuntimeFailure {
    Simple {
        file: String,
        line: u32,
        msg: String,
    },
    Rich {
        file: String,
        line: u32,
        fn_name: String,
        src_line: String,
        col: u32,
        caret_len: u32,
        msg: String,
        locals: String,
    },
    Diagnostic {
        rendered: String,
    },
    Contract {
        file: String,
        line: u32,
        clause_kw: String,
        msg: String,
    },
    SchedulerFatal {
        msg: String,
    },
}

impl JetParaRuntimeFailure {
    fn raise(self) -> ! {
        match self {
            Self::Simple { file, line, msg } => jet_panic(&file, line, &msg),
            Self::Rich {
                file,
                line,
                fn_name,
                src_line,
                col,
                caret_len,
                msg,
                locals,
            } => jet_panic_rich(
                &file, line, &fn_name, &src_line, col, caret_len, &msg, &locals,
            ),
            Self::Diagnostic { rendered } => jet_runtime_diagnostic(rendered),
            Self::Contract {
                file,
                line,
                clause_kw,
                msg,
            } => jet_contract_fail(&file, line, &clause_kw, &msg),
            // The scheduler prelude is emitted only when task support is used,
            // while the parallel carrier is part of the always-present core
            // prelude.  Reproduce the scheduler's ordinary fatal boundary here
            // without creating a generated-code dependency on an optional item.
            Self::SchedulerFatal { msg } => {
                eprintln!("panic: {}", msg);
                std::process::exit(70);
            }
        }
    }
}

thread_local! {
    static JET_PARA_DEFER_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn jet_para_call<R, F>(index: usize, f: F) -> Result<R, JetParaFailure>
where
    F: FnOnce() -> R,
{
    let result = JET_PARA_DEFER_FAILURE.with(|defer| {
        let previous = defer.replace(true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        defer.set(previous);
        result
    });
    result.map_err(|payload| JetParaFailure { index, payload })
}

fn jet_para_raise_failure(failure: JetParaFailure) -> ! {
    match failure.payload.downcast::<JetParaRuntimeFailure>() {
        Ok(failure) => (*failure).raise(),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn jet_list_para_chunks<R, F>(len: usize, f: F) -> Vec<R>
where
    R: Send,
    F: Fn(std::ops::Range<usize>) -> Result<R, JetParaFailure> + Sync,
{
    let chunk_count = len.div_ceil(JET_PARA_CHUNK_ITEMS);
    if chunk_count == 0 {
        return Vec::new();
    }
    #[cfg(jet_para_test_workers)]
    let worker_count = 3.min(chunk_count);
    #[cfg(not(jet_para_test_workers))]
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(chunk_count);
    // A single chunk is the safe serial fast path. Keep the indexed chunk
    // boundaries when a host exposes only one CPU; para_fold's seed/merge
    // semantics depend on those boundaries even without parallel workers.
    if chunk_count == 1 {
        return match f(0..len) {
            Ok(result) => vec![result],
            Err(failure) => jet_para_raise_failure(failure),
        };
    }
    let mut indexed = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(worker_count);
        let f = &f;
        for worker in 0..worker_count {
            handles.push(scope.spawn(move || {
                let mut out = Vec::new();
                for chunk in (worker..chunk_count).step_by(worker_count) {
                    let start = chunk * JET_PARA_CHUNK_ITEMS;
                    let end = (start + JET_PARA_CHUNK_ITEMS).min(len);
                    out.push((chunk, f(start..end)));
                }
                out
            }));
        }
        let mut indexed = Vec::with_capacity(chunk_count);
        for handle in handles.into_iter().rev() {
            match handle.join() {
                Ok(results) => indexed.extend(results),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
        indexed
    });
    indexed.sort_unstable_by_key(|(chunk, _)| *chunk);
    let mut results = Vec::with_capacity(chunk_count);
    let mut first_failure: Option<JetParaFailure> = None;
    for (_, outcome) in indexed {
        match outcome {
            Ok(result) => results.push(result),
            Err(failure)
                if first_failure
                    .as_ref()
                    .is_none_or(|first| failure.index < first.index) =>
            {
                first_failure = Some(failure);
            }
            Err(_) => {}
        }
    }
    if let Some(failure) = first_failure {
        jet_para_raise_failure(failure);
    }
    results
}

fn jet_list_para_map<T, U, F>(xs: Vec<T>, f: F) -> Vec<U>
where
    T: Sync,
    U: Send,
    F: Fn(&T) -> U + Sync,
{
    jet_list_para_chunks(xs.len(), |range| {
        let mut out = Vec::with_capacity(range.len());
        for index in range {
            out.push(jet_para_call(index, || f(&xs[index]))?);
        }
        Ok(out)
    })
    .into_iter()
    .flatten()
    .collect()
}

fn jet_list_para_flags<T, F>(xs: &[T], f: F) -> Vec<bool>
where
    T: Sync,
    F: Fn(&T) -> bool + Sync,
{
    jet_list_para_chunks(xs.len(), |range| {
        let mut out = Vec::with_capacity(range.len());
        for index in range {
            out.push(jet_para_call(index, || f(&xs[index]))?);
        }
        Ok(out)
    })
    .into_iter()
    .flatten()
    .collect()
}

fn jet_list_para_filter<T, F>(xs: Vec<T>, f: F) -> Vec<T>
where
    T: Sync,
    F: Fn(&T) -> bool + Sync,
{
    let keep = jet_list_para_flags(&xs, f);
    xs.into_iter()
        .zip(keep)
        .filter_map(|(x, keep)| keep.then_some(x))
        .collect()
}

fn jet_list_para_partition<T, F, R, O>(xs: Vec<T>, f: F, out: O) -> R
where
    T: Sync,
    F: Fn(&T) -> bool + Sync,
    O: FnOnce(Vec<T>, Vec<T>) -> R,
{
    let matches = jet_list_para_flags(&xs, f);
    let mut false_items = Vec::new();
    let mut true_items = Vec::new();
    for (item, matched) in xs.into_iter().zip(matches) {
        if matched {
            true_items.push(item);
        } else {
            false_items.push(item);
        }
    }
    out(false_items, true_items)
}

fn jet_list_para_fold<T, U, S, F, M>(xs: Vec<T>, seed: S, step: F, merge: M) -> U
where
    T: Sync,
    U: Send,
    S: Fn() -> U + Sync,
    F: Fn(&U, &T) -> U + Sync,
    M: Fn(&U, &U) -> U + Sync,
{
    let mut partials = jet_list_para_chunks(xs.len(), |range| {
        let start = range.start;
        let mut acc = jet_para_call(start, &seed)?;
        for index in range {
            acc = jet_para_call(index, || step(&acc, &xs[index]))?;
        }
        Ok((start, acc))
    });
    if partials.is_empty() {
        return seed();
    }
    while partials.len() > 1 {
        let mut next = Vec::with_capacity(partials.len().div_ceil(2));
        let mut iter = partials.into_iter();
        while let Some((left_index, left)) = iter.next() {
            match iter.next() {
                Some((_, right)) => match jet_para_call(left_index, || merge(&left, &right)) {
                    Ok(merged) => next.push((left_index, merged)),
                    Err(failure) => jet_para_raise_failure(failure),
                },
                None => next.push((left_index, left)),
            }
        }
        partials = next;
    }
    partials.pop().expect("non-empty parallel fold lost its result").1
}

// D-FIDELITY-API1=A: runtime-global fidelity signal. App code decides policy.
const JET_PERF_DEFAULT_FIDELITY_BITS: u32 = 1065353216; // 1.0f32 bits
static JET_PERF_FIDELITY: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(JET_PERF_DEFAULT_FIDELITY_BITS);
fn jet_perf_fidelity() -> f64 {
    let bits = JET_PERF_FIDELITY.load(std::sync::atomic::Ordering::SeqCst);
    f32::from_bits(bits) as f64
}
fn jet_perf_default_fidelity() -> f64 {
    f32::from_bits(JET_PERF_DEFAULT_FIDELITY_BITS) as f64
}
fn jet_perf_store_fidelity(v: f64) {
    JET_PERF_FIDELITY.store((v as f32).to_bits(), std::sync::atomic::Ordering::SeqCst);
}
fn jet_perf_override_fidelity(v: f64) -> Result<(), String> {
    if !v.is_finite() || v < 0.0 || v > 1.0 {
        return Err(format!(
            "core.perf.Perf.override_fidelity needs 0.0 through 1.0, got {}",
            v
        ));
    }
    jet_perf_store_fidelity(v);
    Ok(())
}
fn jet_perf_reset_fidelity() {
    JET_PERF_FIDELITY.store(
        JET_PERF_DEFAULT_FIDELITY_BITS,
        std::sync::atomic::Ordering::SeqCst,
    );
}

// ── D-APPROX1=A: core.sketch — approximate data structures ────────────────────
// FNV-1a: deterministic, I6-safe, no external crates.
fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 14695981039346656037;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}
// Second independent hash (FNV with a different offset) for multi-hash sketches.
fn fnv1a_h2(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325u64.wrapping_add(0xdeadbeef);
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

// HyperLogLog — cardinality estimator (±2% error at 256 registers).
#[derive(Clone)]
struct JetHyperLogLog(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl JetHyperLogLog {
    fn new() -> Self {
        JetHyperLogLog(std::sync::Arc::new(std::sync::Mutex::new(vec![0u8; 256])))
    }
    fn add(&self, item: &str) {
        let h = fnv1a(item.as_bytes());
        let reg = (h & 0xFF) as usize; // bottom 8 bits → register index
        let rest = h >> 8; // remaining 56 bits
        let lz = if rest == 0 {
            57u8
        } else {
            rest.leading_zeros() as u8 + 1
        };
        let mut regs = self.0.lock().unwrap();
        if lz > regs[reg] {
            regs[reg] = lz;
        }
    }
    fn count(&self) -> i64 {
        let regs = self.0.lock().unwrap();
        let m = regs.len() as f64;
        // LinearCounting for small cardinalities.
        let zeros = regs.iter().filter(|&&v| v == 0).count();
        if zeros > 0 {
            let estimate = m * (m / zeros as f64).ln();
            return estimate.round() as i64;
        }
        // Normal HLL estimate with bias correction constant α_256.
        let sum: f64 = regs.iter().map(|&v| 2f64.powi(-(v as i32))).sum();
        let alpha = 0.7213 / (1.0 + 1.079 / m);
        (alpha * m * m / sum).round() as i64
    }
}
impl JetShow for JetHyperLogLog {
    fn jet_show(&self) -> String {
        format!("HyperLogLog(count={})", self.count())
    }
}

// TDigest — quantile estimator (~±0.5% error). Centroid merging sketch.
#[derive(Clone)]
struct JetTDigest(std::sync::Arc<std::sync::Mutex<Vec<(f64, f64)>>>); // (mean, weight)
impl JetTDigest {
    const DELTA: f64 = 100.0; // compression factor (higher = more accurate, more memory)
    fn new() -> Self {
        JetTDigest(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())))
    }
    fn add(&self, v: f64) {
        let mut cs = self.0.lock().unwrap();
        // Insert as singleton then merge nearby centroids.
        let idx = cs.partition_point(|&(m, _)| m < v);
        cs.insert(idx, (v, 1.0));
        let total: f64 = cs.iter().map(|(_, w)| w).sum();
        let mut merged: Vec<(f64, f64)> = Vec::with_capacity(cs.len());
        let mut cum = 0.0f64;
        for &(mean, weight) in cs.iter() {
            if merged.is_empty() {
                merged.push((mean, weight));
                cum += weight;
                continue;
            }
            let last = merged.last_mut().unwrap();
            let q = cum / total;
            let limit = 4.0 * total * q * (1.0 - q) / Self::DELTA;
            if last.1 + weight <= limit.max(1.0) {
                let new_w = last.1 + weight;
                last.0 = (last.0 * last.1 + mean * weight) / new_w;
                last.1 = new_w;
            } else {
                merged.push((mean, weight));
                cum += weight;
            }
        }
        *cs = merged;
    }
    fn quantile(&self, q: f64) -> f64 {
        let cs = self.0.lock().unwrap();
        if cs.is_empty() {
            return 0.0;
        }
        let total: f64 = cs.iter().map(|(_, w)| w).sum();
        let target = q * total;
        let mut cum = 0.0f64;
        for &(mean, weight) in cs.iter() {
            cum += weight;
            if cum >= target {
                return mean;
            }
        }
        cs.last().unwrap().0
    }
}
impl JetShow for JetTDigest {
    fn jet_show(&self) -> String {
        "TDigest".to_string()
    }
}

// CountMinSketch — frequency estimator. 4 rows × 256 cols; FNV + offset.
const CMS_COLS: usize = 256;
#[derive(Clone)]
struct JetCountMinSketch(std::sync::Arc<std::sync::Mutex<[[u32; CMS_COLS]; 4]>>);
impl JetCountMinSketch {
    fn new() -> Self {
        JetCountMinSketch(std::sync::Arc::new(std::sync::Mutex::new(
            [[0u32; CMS_COLS]; 4],
        )))
    }
    fn add(&self, key: &str) {
        let bytes = key.as_bytes();
        let h1 = fnv1a(bytes);
        let h2 = fnv1a_h2(bytes);
        let mut tbl = self.0.lock().unwrap();
        for row in 0..4usize {
            let col = ((h1.wrapping_add(h2.wrapping_mul(row as u64 + 1))) & 0xFF) as usize;
            tbl[row][col] = tbl[row][col].saturating_add(1);
        }
    }
    fn count(&self, key: &str) -> i64 {
        let bytes = key.as_bytes();
        let h1 = fnv1a(bytes);
        let h2 = fnv1a_h2(bytes);
        let tbl = self.0.lock().unwrap();
        (0..4usize)
            .map(|row| {
                let col = ((h1.wrapping_add(h2.wrapping_mul(row as u64 + 1))) & 0xFF) as usize;
                tbl[row][col]
            })
            .min()
            .unwrap() as i64
    }
}
impl JetShow for JetCountMinSketch {
    fn jet_show(&self) -> String {
        "CountMinSketch".to_string()
    }
}

// ReservoirSampler — uniform random sample. Seeded xorshift64 PRNG (I6-safe).
#[derive(Clone)]
struct JetReservoirSampler(std::sync::Arc<std::sync::Mutex<JetReservoirInner>>);
struct JetReservoirInner {
    capacity: usize,
    reservoir: Vec<String>,
    count: u64,
    rng: u64,
}
impl Clone for JetReservoirInner {
    fn clone(&self) -> Self {
        JetReservoirInner {
            capacity: self.capacity,
            reservoir: self.reservoir.clone(),
            count: self.count,
            rng: self.rng,
        }
    }
}
impl JetReservoirSampler {
    fn new(capacity: i64) -> Self {
        let cap = (capacity.max(1)) as usize;
        JetReservoirSampler(std::sync::Arc::new(std::sync::Mutex::new(
            JetReservoirInner {
                capacity: cap,
                reservoir: Vec::with_capacity(cap),
                count: 0,
                rng: 0xdeadbeef_cafebabe,
            },
        )))
    }
    fn add(&self, item: String) {
        let mut inner = self.0.lock().unwrap();
        inner.count += 1;
        if inner.reservoir.len() < inner.capacity {
            inner.reservoir.push(item);
        } else {
            // xorshift64
            let mut x = inner.rng;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            inner.rng = x;
            let j = (x % inner.count) as usize;
            if j < inner.capacity {
                inner.reservoir[j] = item;
            }
        }
    }
    fn sample(&self) -> Vec<String> {
        self.0.lock().unwrap().reservoir.clone()
    }
}
impl JetShow for JetReservoirSampler {
    fn jet_show(&self) -> String {
        format!("ReservoirSampler(n={})", self.0.lock().unwrap().count)
    }
}

thread_local! {
    static JET_IN_SCHEDULER_TASK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static JET_INTERRUPT_HANDLER_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

pub fn jet_scheduler_task_panic_enter() {
    JET_IN_SCHEDULER_TASK.with(|c| c.set(true));
}

pub fn jet_scheduler_task_panic_leave() {
    JET_IN_SCHEDULER_TASK.with(|c| c.set(false));
}

fn jet_scheduler_in_task() -> bool {
    JET_IN_SCHEDULER_TASK.with(|c| c.get())
}

pub fn jet_interrupt_handler_panic_enter() {
    JET_INTERRUPT_HANDLER_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
}

pub fn jet_interrupt_handler_panic_leave() {
    JET_INTERRUPT_HANDLER_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
}

fn jet_runtime_should_unwind() -> bool {
    jet_scheduler_in_task() || jet_interrupt_handler_should_unwind()
}

fn jet_interrupt_handler_should_unwind() -> bool {
    JET_INTERRUPT_HANDLER_DEPTH.with(|depth| depth.get() != 0)
}

fn jet_scheduler_panic_should_unwind() -> bool {
    jet_runtime_should_unwind()
}

struct JetRuntimeExit;

fn jet_runtime_boundary<F, T>(run: F) -> T
where
    F: FnOnce() -> T,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(value) => value,
        Err(payload) if payload.is::<JetRuntimeExit>() => std::process::exit(70),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn jet_runtime_exit() -> ! {
    std::panic::resume_unwind(Box::new(JetRuntimeExit))
}

fn jet_panic(file: &str, line: u32, msg: &str) -> ! {
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Simple {
            file: file.to_string(),
            line,
            msg: msg.to_string(),
        }));
    }
    jet_proof_record(2, 1, "panic", msg, file, line);
    if jet_runtime_should_unwind() {
        panic!("{} (at {}:{})", msg, file, line);
    }
    eprintln!("panic: {}", msg);
    eprintln!("  --> {}:{}", file, line);
    jet_runtime_exit();
}

fn jet_runtime_diagnostic(rendered: String) -> ! {
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Diagnostic { rendered }));
    }
    if jet_interrupt_handler_should_unwind() {
        panic!("{}", rendered);
    }
    eprintln!("{}", rendered);
    jet_runtime_exit();
}
/// E3005 (D-PREPOST1): a `#Pre`/`#Post` contract clause failed at runtime.
/// `clause_kw` is `"Pre"`/`"Post"`; `msg` is the clause's own message text
/// (the second argument to `#Pre(cond, "msg")`/`#Post(cond, "msg")`).
#[allow(dead_code)] // only called from generated code that has a #Pre/#Post
fn jet_contract_fail(file: &str, line: u32, clause_kw: &str, msg: &str) -> ! {
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Contract {
            file: file.to_string(),
            line,
            clause_kw: clause_kw.to_string(),
            msg: msg.to_string(),
        }));
    }
    if jet_runtime_should_unwind() {
        panic!(
            "#{} contract failed: {} (at {}:{})",
            clause_kw, msg, file, line
        );
    }
    eprintln!("#{} contract failed: {}", clause_kw, msg);
    eprintln!("  --> {}:{}", file, line);
    jet_runtime_exit();
}

/// Private structured producer channel used only when `jet prove` launches a
/// test harness. Length framing keeps user strings opaque; terminal text is
/// never parsed as evidence.
fn jet_proof_record(kind: u8, state: u8, name: &str, message: &str, file: &str, line: u32) {
    let Ok(path) = std::env::var("JET_TEST_PROOF_REPORT") else { return };
    let Ok(mut report) = std::fs::OpenOptions::new().create(true).append(true).open(path) else { return };
    use std::io::Write as _;
    if report.metadata().map(|m| m.len() == 0).unwrap_or(false) {
        let _ = report.write_all(b"JETTEST2");
    }
    let _ = report.write_all(&[kind, state]);
    let _ = report.write_all(&(line as u64).to_be_bytes());
    for bytes in [name.as_bytes(), message.as_bytes(), file.as_bytes()] {
        let _ = report.write_all(&(bytes.len() as u64).to_be_bytes());
        let _ = report.write_all(bytes);
    }
    let _ = report.flush();
}
// D-NUMOPS1: plain integer arithmetic traps on overflow (safe by default) — a
// silent corruption becomes a caught bug. Each arithmetic operator on a fixed-width
// integer lowers to one of these, which panic with the source location instead
// of wrapping. `wrapping(…)`/`saturating(…)`/`checked(…)` opt out at the use
// site. Floats and `#Numeric` distinct types keep the plain Rust operators.
trait JetArith: Copy {
    fn jet_add(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_sub(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_mul(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_div(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_rem(self, rhs: Self, file: &str, line: u32) -> Self;
    // D-NUMOPS1: a shift by a bit-count `>=` the value's width is undefined in C
    // and a panic in Rust — Jet traps it cleanly instead. The count comes in as
    // an `i128` so any integer width (signed or unsigned) reaches here losslessly.
    fn jet_shl(self, bits: i128, file: &str, line: u32) -> Self;
    fn jet_shr(self, bits: i128, file: &str, line: u32) -> Self;
}
macro_rules! jet_arith_impl {
    ($($t:ty),*) => { $(
        impl JetArith for $t {
            fn jet_add(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_add(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this addition overflows the value's type (the result is outside its range)")))
            }
            fn jet_sub(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_sub(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this subtraction overflows the value's type (the result is outside its range)")))
            }
            fn jet_mul(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_mul(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this multiplication overflows the value's type (the result is outside its range)")))
            }
            fn jet_div(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_div(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this division can't be done (dividing by zero, or overflow)")))
            }
            fn jet_rem(self, rhs: Self, file: &str, line: u32) -> Self {
                if rhs == 0 {
                    jet_panic(file, line, "divided by zero");
                }
                self.checked_rem(rhs).unwrap_or_else(|| jet_panic(file, line,
                    "attempt to calculate the remainder with overflow"))
            }
            fn jet_shl(self, bits: i128, file: &str, line: u32) -> Self {
                let w = (Self::BITS) as i128;
                if bits < 0 || bits >= w {
                    jet_panic(file, line, &format!(
                        "shifting left by {} bits is out of range (this type is {} bits wide)", bits, w));
                }
                self << (bits as u32)
            }
            fn jet_shr(self, bits: i128, file: &str, line: u32) -> Self {
                let w = (Self::BITS) as i128;
                if bits < 0 || bits >= w {
                    jet_panic(file, line, &format!(
                        "shifting right by {} bits is out of range (this type is {} bits wide)", bits, w));
                }
                self >> (bits as u32)
            }
        }
    )* };
}
jet_arith_impl!(i8, i16, i32, i64, u8, u16, u32, u64);
/// E3001 (E2-M12, D-OBS1/D-OBS2): rich panic report — includes the function name,
/// a source-line context box, and (in debug builds only) safe local variable values.
/// `col` is 1-based; `caret_len` covers the highlighted span in the source line.
/// `locals` is an empty string in release builds; "x = 1, y = false" in debug builds.
fn jet_panic_rich(
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
    msg: &str,
    locals: &str,
) -> ! {
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Rich {
            file: file.to_string(),
            line,
            fn_name: fn_name.to_string(),
            src_line: src_line.to_string(),
            col,
            caret_len,
            msg: msg.to_string(),
            locals: locals.to_string(),
        }));
    }
    jet_proof_record(2, 1, "panic", msg, file, line);
    let line_s = line.to_string();
    let margin = line_s.len();
    let pad = " ".repeat(margin);
    eprintln!("panic: {}", msg);
    eprintln!("  --> {}:{} in {}", file, line, fn_name);
    eprintln!("   {}|", pad);
    eprintln!("{} | {}", line_s, src_line);
    let col_offset = col.saturating_sub(1) as usize;
    let caret = "^".repeat(caret_len.max(1) as usize);
    eprintln!("   {}| {}{}", pad, " ".repeat(col_offset), caret);
    if !locals.is_empty() {
        eprintln!("locals: {}", locals);
    }
    if jet_runtime_should_unwind() {
        panic!("{} (at {}:{})", msg, file, line);
    }
    jet_runtime_exit();
}
/// E3002 / D-ERRCTX1=D: `?`-propagation trace in **dev** builds.
///
/// Gate is `not(jet_release)` (set by `--release` / `--profile=release`), not
/// `debug_assertions`: the default `jet run` profile passes `-O`, which turns
/// debug assertions off while still being a daily-driver /dev/ build.
///
/// Consecutive identical frames (same fn + file + line) collapse — Go wrap-noise
/// lesson — while each distinct site keeps its identity (Elixir lesson).
thread_local! {
    static JET_ERR_TRACE_LAST: std::cell::RefCell<Option<(String, String, u32)>> =
        const { std::cell::RefCell::new(None) };
}
fn jet_trace_err<T, E>(r: Result<T, E>, file: &str, line: u32, fn_name: &str) -> Result<T, E> {
    if cfg!(not(jet_release)) {
        if r.is_err() {
            let site = (fn_name.to_string(), file.to_string(), line);
            let fresh = JET_ERR_TRACE_LAST.with(|last| {
                let mut slot = last.borrow_mut();
                if slot.as_ref() == Some(&site) {
                    false
                } else {
                    *slot = Some(site);
                    true
                }
            });
            if fresh {
                eprintln!(
                    "error propagated from: {} ({}:{}) via ?",
                    fn_name, file, line
                );
            }
        } else {
            JET_ERR_TRACE_LAST.with(|last| *last.borrow_mut() = None);
        }
    }
    r
}
// D-ERRCTX1=D: `.context(msg)` — a lazily-evaluated human boundary message
// prepended to the error chain (errors are plain `String`s in Jet, so the
// chain is just accumulated text: origin, then each `.context()` crossed on
// the way out). `msg` runs only on the `Err` branch.
fn jet_context<T, F: FnOnce() -> String>(r: Result<T, String>, msg: F) -> Result<T, String> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => Err(format!("{}: {}", msg(), e)),
    }
}
// D-FIXARR1: index/unpack/slice helpers accept `&[T]` so that both growable
// `Vec<T>` and fixed-size `[T; N]` stack arrays coerce in without `.to_vec()`.
fn jet_index_vec<T: Clone>(xs: &[T], i: i64, file: &str, line: u32) -> T {
    let len = xs.len() as i64;
    if i < 0 || i >= len {
        jet_panic(
            file,
            line,
            &format!(
                "the list has {} items, so position {} doesn't exist",
                len, i
            ),
        );
    }
    xs[i as usize].clone()
}
fn jet_index_vec_mut<'a, T>(
    xs: &'a mut [T],
    i: i64,
    file: &str,
    line: u32,
) -> &'a mut T {
    let len = xs.len() as i64;
    if i < 0 || i >= len {
        jet_panic(
            file,
            line,
            &format!(
                "the list has {} items, so position {} doesn't exist",
                len, i
            ),
        );
    }
    &mut xs[i as usize]
}
fn jet_unpack_vec<T: Clone>(xs: &[T], want: usize, i: usize, file: &str, line: u32) -> T {
    if xs.len() != want {
        jet_panic(
            file,
            line,
            &format!(
                "this pattern needs exactly {} item{}, but the list has {}",
                want,
                if want == 1 { "" } else { "s" },
                xs.len()
            ),
        );
    }
    xs[i].clone()
}
fn jet_slice_vec<T: Clone>(xs: &[T], a: i64, b: i64, file: &str, line: u32) -> Vec<T> {
    let len = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!("can't slice {} items from {} to {} (inclusive)", len, a, b),
        );
    }
    xs[a as usize..=b as usize].to_vec()
}
fn jet_checked_range_bounds(
    len: i64,
    range: &JetRange,
    action: &str,
    file: &str,
    line: u32,
) -> std::ops::Range<usize> {
    let Some((start, end)) =
        jet_range_bounds(range.start, range.end, range.exclusive, len)
    else {
        jet_panic(
            file,
            line,
            &format!(
                "can't {} {} items from {} to {} ({})",
                action,
                len,
                range.start,
                range.end,
                if range.exclusive { "exclusive" } else { "inclusive" }
            ),
        );
    };
    start as usize..end as usize
}

fn jet_slice_range<T: Clone>(
    xs: &[T],
    range: &JetRange,
    file: &str,
    line: u32,
) -> Vec<T> {
    xs[jet_checked_range_bounds(xs.len() as i64, range, "slice", file, line)].to_vec()
}
// D-DYNARRAY1 / D-SHAPE-PLACE1: range places produce zero-copy windows.
// Their bounds share `jet_range_bounds` with owned slicing and every engine.
// The returned lifetime is tied to `xs`; sema proves the window cannot outlive
// the owner or survive a storage-changing mutation.
fn jet_view_new<'a, T>(xs: &'a [T], a: i64, b: i64, file: &str, line: u32) -> &'a [T] {
    let len = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!("can't view {} items from {} to {} (inclusive)", len, a, b),
        );
    }
    &xs[a as usize..=b as usize]
}

fn jet_view_mut_new<'a, T>(
    xs: &'a mut [T],
    a: i64,
    b: i64,
    file: &str,
    line: u32,
) -> &'a mut [T] {
    let len = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!("can't view {} items from {} to {} (inclusive)", len, a, b),
        );
    }
    &mut xs[a as usize..=b as usize]
}

fn jet_views_mut_new<'a, T>(
    xs: &'a mut [T],
    ranges: &[(i64, i64, u32)],
    file: &str,
) -> Vec<&'a mut [T]> {
    let len = xs.len() as i64;
    let mut ordered = Vec::with_capacity(ranges.len());
    for (index, &(start, end, line)) in ranges.iter().enumerate() {
        if start < 0 || end < 0 || start > end || end >= len {
            jet_panic(
                file,
                line,
                &format!(
                    "can't view {} items from {} to {} (inclusive)",
                    len, start, end
                ),
            );
        }
        ordered.push((start as usize, end as usize + 1, index));
    }
    ordered.sort_by_key(|&(start, end, _)| (start, end));
    if ordered.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        jet_panic(file, 0, "mutable view ranges overlap");
    }

    let mut pieces = Vec::with_capacity(ordered.len());
    let mut tail = xs;
    let mut offset = 0usize;
    for (start, end, index) in ordered {
        let (_, from_start) = tail.split_at_mut(start - offset);
        let (selected, after) = from_start.split_at_mut(end - start);
        pieces.push((index, selected));
        tail = after;
        offset = end;
    }
    pieces.sort_by_key(|(index, _)| *index);
    pieces.into_iter().map(|(_, selected)| selected).collect()
}

fn jet_views_mut_range_new<'a, T>(
    xs: &'a mut [T],
    ranges: &[(JetRange, u32)],
    file: &str,
) -> Vec<&'a mut [T]> {
    let bounds = ranges
        .iter()
        .map(|(range, line)| {
            let checked =
                jet_checked_range_bounds(xs.len() as i64, range, "view", file, *line);
            (checked.start as i64, checked.end as i64 - 1, *line)
        })
        .collect::<Vec<_>>();
    jet_views_mut_new(xs, &bounds, file)
}

// D-MEMDISJOINT1=A: runtime disjointness is proved once, before any mutable
// view exists. These helpers return the same Error family for bounds and
// overlap failures; engines only marshal their arguments and results.
fn jet_split_write<T>(
    xs: &mut [T],
    mid: i64,
) -> Result<(&mut [T], &mut [T]), String> {
    jet_disjoint_split_bounds(xs.len(), mid)?;
    Ok(xs.split_at_mut(mid as usize))
}

fn jet_get_disjoint_write<'a, T>(
    xs: &'a mut [T],
    indices: &[i64],
) -> Result<Vec<&'a mut [T]>, String> {
    let ordered = jet_disjoint_index_bounds(xs.len(), indices)?;
    let mut views = Vec::with_capacity(ordered.len());
    let mut tail = xs;
    let mut offset = 0usize;
    for (start, end, position) in ordered {
        let (_, from_index) = tail.split_at_mut(start - offset);
        let (selected, after) = from_index.split_at_mut(end - start);
        views.push((position, selected));
        tail = after;
        offset = end;
    }
    views.sort_by_key(|(position, _)| *position);
    Ok(views.into_iter().map(|(_, view)| view).collect())
}

fn jet_edit_disjoint<T, F>(xs: &mut [T], indices: &[i64], edit: F) -> Result<(), String>
where
    F: FnOnce(&mut [T], &mut [T]),
{
    if indices.len() != 2 {
        return Err("edit_disjoint needs exactly two indexes".to_string());
    }
    let mut views = jet_get_disjoint_write(xs, indices)?;
    let right = views.pop().expect("two disjoint views");
    let left = views.pop().expect("two disjoint views");
    edit(left, right);
    Ok(())
}

fn jet_view_range_new<'a, T>(
    xs: &'a [T],
    range: &JetRange,
    file: &str,
    line: u32,
) -> &'a [T] {
    &xs[jet_checked_range_bounds(xs.len() as i64, range, "view", file, line)]
}

fn jet_view_mut_range_new<'a, T>(
    xs: &'a mut [T],
    range: &JetRange,
    file: &str,
    line: u32,
) -> &'a mut [T] {
    let bounds = jet_checked_range_bounds(xs.len() as i64, range, "view", file, line);
    &mut xs[bounds]
}

fn jet_check_view_bounds(len: i64, a: i64, b: i64, file: &str, line: u32) {
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!("can't view {} items from {} to {} (inclusive)", len, a, b),
        );
    }
}
// D-DYNARRAY1: View<T> read-only closure surface. `xs` is already a borrow
// (never `.clone()`d to an owned `Vec` first, unlike the `jet_list_*` family
// above) — folding/mapping a view touches no allocation beyond the result.
fn jet_view_fold<T, U, F>(xs: &[T], init: U, mut f: F) -> U
where
    F: FnMut(&U, &T) -> U,
{
    let mut acc = init;
    for x in xs {
        acc = f(&acc, x);
    }
    acc
}
fn jet_view_map<T, U, F>(xs: &[T], f: F) -> Vec<U>
where
    F: FnMut(&T) -> U,
{
    xs.iter().map(f).collect()
}
#[derive(Clone, PartialEq, Eq)]
struct JetMap<K, V>(std::sync::Arc<std::collections::BTreeMap<K, V>>);

impl<K, V> JetMap<K, V> {
    fn new() -> Self {
        Self(std::sync::Arc::new(std::collections::BTreeMap::new()))
    }
}

// Codegen lowers map construction from a sequence of pairs to
// `.into_iter().collect()`, so the map has to be buildable from its own pairs.
// Without this, decoding a table into a typed map emitted Rust that rustc
// rejected (I2).
impl<K: Ord, V> FromIterator<(K, V)> for JetMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(pairs: I) -> Self {
        Self(std::sync::Arc::new(
            pairs.into_iter().collect::<std::collections::BTreeMap<K, V>>(),
        ))
    }
}

impl<K, V> std::ops::Deref for JetMap<K, V> {
    type Target = std::collections::BTreeMap<K, V>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K: Ord + Clone, V: Clone> std::ops::DerefMut for JetMap<K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        std::sync::Arc::make_mut(&mut self.0)
    }
}

impl<'a, K: Ord, V> IntoIterator for &'a JetMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::collections::btree_map::Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JetRemoveBy {
    Val,
    Slot,
}

fn jet_index_map<K: Ord + Clone, V: Clone>(
    m: &JetMap<K, V>,
    k: &K,
    file: &str,
    line: u32,
) -> V {
    match m.get(k) {
        Some(v) => v.clone(),
        None => jet_panic(file, line, &format!("the map has no entry for this key")),
    }
}
fn jet_map_insert<K: Ord + Clone, V: Clone>(m: &mut JetMap<K, V>, k: K, v: V) {
    m.insert(k, v);
}

/// D-MAP-MERGE1=E: merge `other` into a clone of `left`. Right wins on shared keys.
fn jet_map_merge<K: Ord + Clone, V: Clone>(
    left: &JetMap<K, V>,
    other: &JetMap<K, V>,
) -> JetMap<K, V> {
    let mut out = left.clone();
    for (k, v) in other {
        out.insert(k.clone(), v.clone());
    }
    out
}

/// D-MAP-MERGE1=E: merge with an explicit conflict callback `(key, left, right) -> V`.
fn jet_map_merge_with<K: Ord + Clone, V: Clone, F>(
    left: &JetMap<K, V>,
    other: &JetMap<K, V>,
    conflict: F,
) -> JetMap<K, V>
where
    F: Fn(&K, V, V) -> V,
{
    let mut out = left.clone();
    for (k, right) in other {
        match out.remove(k) {
            Some(left_v) => {
                let resolved = conflict(k, left_v, right.clone());
                out.insert(k.clone(), resolved);
            }
            None => {
                out.insert(k.clone(), right.clone());
            }
        }
    }
    out
}
// D-LISTMAP1: the view owns the map's Arc and advances by key. This keeps the
// iterator `'static` without copying the BTreeMap (or borrowing through a
// short-lived local Arc). Each pull clones only the yielded item; the map stays
// shared and untouched until a mutation triggers Arc::make_mut.
struct JetMapKeys<K, V> {
    map: std::sync::Arc<std::collections::BTreeMap<K, V>>,
    last: Option<K>,
}

impl<K: Ord + Clone, V> Iterator for JetMapKeys<K, V> {
    type Item = K;

    fn next(&mut self) -> Option<Self::Item> {
        let next = match self.last.as_ref() {
            Some(last) => self
                .map
                .range((std::ops::Bound::Excluded(last), std::ops::Bound::Unbounded))
                .next(),
            None => self.map.iter().next(),
        }?;
        let key = next.0.clone();
        self.last = Some(key.clone());
        Some(key)
    }
}

struct JetMapValues<K, V> {
    map: std::sync::Arc<std::collections::BTreeMap<K, V>>,
    last: Option<K>,
}

impl<K: Ord + Clone, V: Clone> Iterator for JetMapValues<K, V> {
    type Item = V;

    fn next(&mut self) -> Option<Self::Item> {
        let next = match self.last.as_ref() {
            Some(last) => self
                .map
                .range((std::ops::Bound::Excluded(last), std::ops::Bound::Unbounded))
                .next(),
            None => self.map.iter().next(),
        }?;
        let key = next.0.clone();
        let value = next.1.clone();
        self.last = Some(key);
        Some(value)
    }
}

fn jet_map_keys<K: Ord + Clone + 'static, V: 'static>(m: &JetMap<K, V>) -> JetIter<K> {
    JetIter(Box::new(JetMapKeys {
        map: std::sync::Arc::clone(&m.0),
        last: None,
    }))
}

fn jet_map_values<K: Ord + Clone + 'static, V: Clone + 'static>(m: &JetMap<K, V>) -> JetIter<V> {
    JetIter(Box::new(JetMapValues {
        map: std::sync::Arc::clone(&m.0),
        last: None,
    }))
}

fn jet_list_remove_value<T: Clone + PartialEq>(
    xs: &mut Vec<T>,
    value: T,
    _file: &str,
    _line: u32,
) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(
        xs.iter()
            .position(|item| *item == value)
            .map(|index| xs.remove(index)),
    )
}

fn jet_list_remove_slot<T: Clone>(xs: &mut Vec<T>, i: i64, file: &str, line: u32) -> JetOutcome<T, JetAbsent> {
    let len = xs.len() as i64;
    if i < 0 || i >= len {
        jet_panic(
            file,
            line,
            &format!(
                "the list has {} items, so position {} doesn't exist",
                len, i
            ),
        );
    }
    Ok(xs.remove(i as usize))
}

// D-LISTREMOVE1/F (criterion c6 on #1481): PriorityQueue.remove reuses List's
// exact value/slot selector shape. `BinaryHeap` has no native indexed or
// value-search removal, so both forms round-trip through an owned `Vec` —
// sorted highest-first, the same canonical order `peek`/`to_sorted_list`
// already publish (and the one the TIR-eval/comptime twin uses), so `.Slot`
// means the same position on every execution tier (I9).
fn jet_priority_queue_remove_value<T: Ord>(
    pq: &mut std::collections::BinaryHeap<T>,
    value: T,
) -> Option<T> {
    let mut items: Vec<T> = std::mem::take(pq).into_sorted_vec();
    items.reverse();
    let found = items
        .iter()
        .position(|item| *item == value)
        .map(|index| items.remove(index));
    *pq = items.into_iter().collect();
    found
}

fn jet_priority_queue_remove_slot<T: Ord>(
    pq: &mut std::collections::BinaryHeap<T>,
    i: i64,
    file: &str,
    line: u32,
) -> Option<T> {
    let mut items: Vec<T> = std::mem::take(pq).into_sorted_vec();
    items.reverse();
    let len = items.len() as i64;
    if i < 0 || i >= len {
        jet_panic(
            file,
            line,
            &format!(
                "the priority queue has {} items, so position {} doesn't exist",
                len, i
            ),
        );
    }
    let removed = items.remove(i as usize);
    *pq = items.into_iter().collect();
    Some(removed)
}

fn jet_list_count<T: PartialEq>(xs: &[T], value: &T) -> i64 {
    xs.iter().filter(|item| *item == value).count() as i64
}

fn jet_list_concat<T: Clone>(left: &[T], right: &[T]) -> Vec<T> {
    let mut out = left.to_vec();
    out.extend(right.iter().cloned());
    out
}
fn jet_char_len(s: &String) -> i64 {
    s.chars().count() as i64
}
// Eager materialize of the same pieces as `jet_iter_string_split` (AOT `String.split`
// emits the lazy helper; this Vec form remains for hosts that need a list handle).
fn jet_string_split(s: &String, sep: &str) -> Vec<String> {
    s.split(sep).map(|x| x.to_string()).collect()
}
// D-STR-AFTER1: first-occurrence substring split. `sep` absent -> the whole
// original string (both sides agree, mirroring `.replace`'s no-match-is-identity
// convention — no `Option`/empty-string special case to unwrap).
fn jet_string_after(s: &String, sep: &str) -> String {
    match s.find(sep) {
        Some(i) => s[i + sep.len()..].to_string(),
        None => s.clone(),
    }
}
fn jet_string_before(s: &String, sep: &str) -> String {
    match s.find(sep) {
        Some(i) => s[..i].to_string(),
        None => s.clone(),
    }
}
// D-MEM1 stage S5 (2026-07-04): zero-copy siblings of `jet_string_after`/
// `_before`/(inline `.trim()`) — a genuine borrow into `s`'s own buffer, no
// allocation, instead of a fresh owned `String`. Used ONLY when sema proves
// (E2307, `Binding::string_view`) the resulting binding can't outlive `s`'s
// scope — the same D-DYNARRAY1 soundness proof `View<T>`/`jet_view_new`
// already uses, applied to strings. `s: &str` (not `&String`) so a call
// chain of these composes without a materialize step in between.
fn jet_string_after_view<'a>(s: &'a str, sep: &str) -> &'a str {
    match s.find(sep) {
        Some(i) => &s[i + sep.len()..],
        None => s,
    }
}
fn jet_string_before_view<'a>(s: &'a str, sep: &str) -> &'a str {
    match s.find(sep) {
        Some(i) => &s[..i],
        None => s,
    }
}
fn jet_string_trim_view(s: &str) -> &str {
    jet_unicode_trim_view(s)
}
fn jet_string_lines(s: &String) -> Vec<String> {
    s.lines().map(|x| x.to_string()).collect()
}
fn jet_string_slice(s: &String, a: i64, b: i64, file: &str, line: u32) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!(
                "can't slice {} characters from {} to {} (inclusive)",
                len, a, b
            ),
        );
    }
    chars[a as usize..=b as usize].iter().collect()
}
fn jet_list_map<T, U, F>(xs: Vec<T>, f: F) -> Vec<U>
where
    F: Fn(&T) -> U,
{
    xs.iter().map(f).collect()
}
fn jet_list_map_mut<T, U, F>(xs: Vec<T>, mut f: F) -> Vec<U>
where
    F: FnMut(&T) -> U,
{
    xs.iter().map(|x| f(x)).collect()
}
fn jet_list_filter<T, F>(xs: Vec<T>, mut f: F) -> Vec<T>
where
    F: FnMut(&T) -> bool,
{
    xs.into_iter().filter(|x| f(x)).collect()
}
fn jet_list_each<T, F, I>(xs: I, f: F)
where
    I: IntoIterator<Item = T>,
    F: Fn(&T),
{
    for x in xs {
        f(&x);
    }
}
fn jet_list_each_ref<T, F>(xs: &Vec<T>, mut f: F)
where
    F: FnMut(&T),
{
    for x in xs.iter() {
        f(x);
    }
}
fn jet_list_each_mut<T, F, I>(xs: I, mut f: F)
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T),
{
    for x in xs {
        f(&x);
    }
}
fn jet_list_find<T, F, I>(xs: I, mut f: F) -> JetOutcome<T, JetAbsent>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> bool,
{
    jet_outcome_of(xs.into_iter().find(|x| f(x)))
}
fn jet_list_any<T, F, I>(xs: I, mut f: F) -> bool
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> bool,
{
    xs.into_iter().any(|x| f(&x))
}
fn jet_list_all<T, F, I>(xs: I, mut f: F) -> bool
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> bool,
{
    xs.into_iter().all(|x| f(&x))
}
fn jet_list_sort_by<T, K: Ord, F>(xs: &mut Vec<T>, f: F)
where
    F: FnMut(&T) -> K,
{
    xs.sort_by_key(f);
}
fn jet_list_reduce<T, U, F, I>(xs: I, init: U, mut f: F) -> U
where
    I: IntoIterator<Item = T>,
    F: FnMut(&U, &T) -> U,
{
    xs.into_iter().fold(init, |acc, x| f(&acc, &x))
}
fn jet_map_each<K: Ord, V, F>(m: JetMap<K, V>, mut f: F)
where
    F: FnMut(&K, &V),
{
    for (k, v) in &m {
        f(k, v);
    }
}

// #1477 Map ledger surface
fn jet_map_copy<K: Ord + Clone, V: Clone>(m: &JetMap<K, V>) -> JetMap<K, V> { m.clone() }
fn jet_map_equal<K: Ord + PartialEq, V: PartialEq>(a: &JetMap<K, V>, b: &JetMap<K, V>) -> bool { a == b }
fn jet_map_first_key<K: Ord + Clone, V>(m: &JetMap<K, V>) -> JetOutcome<K, JetAbsent> { jet_outcome_of(m.keys().next().cloned()) }
fn jet_map_to_list<K: Ord + Clone, V: Clone, R>(m: &JetMap<K, V>, build: impl Fn(K, V) -> R) -> Vec<R> {
    m.iter().map(|(k, v)| build(k.clone(), v.clone())).collect()
}
fn jet_map_any<K: Ord, V, F>(m: JetMap<K, V>, mut f: F) -> bool where F: FnMut(&K, &V) -> bool {
    m.iter().any(|(k, v)| f(k, v))
}
fn jet_map_all<K: Ord, V, F>(m: JetMap<K, V>, mut f: F) -> bool where F: FnMut(&K, &V) -> bool {
    m.iter().all(|(k, v)| f(k, v))
}
fn jet_map_filter<K: Ord + Clone, V: Clone, F>(m: JetMap<K, V>, mut f: F) -> JetMap<K, V>
where F: FnMut(&K, &V) -> bool {
    JetMap(std::sync::Arc::new(m.iter().filter(|(k,v)| f(k,v)).map(|(k,v)|(k.clone(),v.clone())).collect()))
}
fn jet_map_map_values<K: Ord + Clone, V, U, F>(m: JetMap<K, V>, mut f: F) -> JetMap<K, U>
where F: FnMut(&K, &V) -> U {
    JetMap(std::sync::Arc::new(m.iter().map(|(k,v)|(k.clone(), f(k,v))).collect()))
}
fn jet_map_fold<K: Ord, V, U, F>(m: JetMap<K, V>, init: U, mut f: F) -> U
where F: FnMut(&U, &K, &V) -> U {
    let mut acc = init;
    for (k, v) in &m {
        acc = f(&acc, k, v);
    }
    acc
}
fn jet_map_flat_map<K: Ord + Clone, V: Clone, F>(m: JetMap<K, V>, mut f: F) -> JetMap<K, V>
where F: FnMut(&K, &V) -> JetMap<K, V> {
    let mut out = JetMap::new();
    for (k, v) in &m {
        for (ik, iv) in f(k, v).iter() {
            out.insert(ik.clone(), iv.clone());
        }
    }
    out
}
fn jet_map_max_value<K: Ord, V: Ord + Clone>(m: &JetMap<K, V>) -> JetOutcome<V, JetAbsent> { jet_outcome_of(m.values().max().cloned()) }
fn jet_map_min_value<K: Ord, V: Ord + Clone>(m: &JetMap<K, V>) -> JetOutcome<V, JetAbsent> { jet_outcome_of(m.values().min().cloned()) }
fn jet_map_intersection<K: Ord + Clone, V: Clone>(left: &JetMap<K, V>, right: &JetMap<K, V>) -> JetMap<K, V> {
    JetMap(std::sync::Arc::new(left.iter().filter(|(k,_)| right.contains_key(k)).map(|(k,v)|(k.clone(),v.clone())).collect()))
}
fn jet_map_slice_keys<K: Ord + Clone, V: Clone>(m: &JetMap<K, V>, keys: Vec<K>) -> JetMap<K, V> {
    let mut out = JetMap::new();
    for k in keys { if let Some(v) = m.get(&k) { out.insert(k, v.clone()); } }
    out
}
fn jet_map_from_keys<K: Ord + Clone, V: Clone>(keys: Vec<K>, default: V) -> JetMap<K, V> {
    let mut out = JetMap::new();
    for k in keys { out.insert(k, default.clone()); }
    out
}
fn jet_map_contains_value<K: Ord, V: PartialEq>(m: &JetMap<K, V>, needle: &V) -> bool {
    m.values().any(|v| v == needle)
}
fn jet_map_pop_first<K: Ord + Clone, V: Clone>(m: &mut JetMap<K, V>) -> JetOutcome<V, JetAbsent> {
    let Some(key) = m.keys().next().cloned() else { return Err(JetAbsent) };
    jet_outcome_of(m.remove(&key))
}

