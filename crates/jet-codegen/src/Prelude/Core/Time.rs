// D-TIMEDEPTH1/D-TIME-CALENDAR1: civil-time types and calendar math.
// Pure Rust, no external crates (I6). Proleptic Gregorian calendar, Unix time
// as UTC seconds, and a small TZif reader for IANA zoneinfo files.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
        365 * y + y.div_euclid(4) - y.div_euclid(100)
            + y.div_euclid(400)
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
        let mut y = n.div_euclid(365) + 1;
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
        let (year_part, month_part, day_part) = if let Some(rest) = s.strip_prefix('-') {
            let mut parts = rest.splitn(3, '-');
            (
                parts
                    .next()
                    .map(|year| format!("-{year}"))
                    .ok_or_else(|| format!("invalid date: {}", s))?,
                parts
                    .next()
                    .ok_or_else(|| format!("invalid date: {}", s))?
                    .to_string(),
                parts
                    .next()
                    .ok_or_else(|| format!("invalid date: {}", s))?
                    .to_string(),
            )
        } else {
            let mut parts = s.splitn(3, '-');
            (
                parts
                    .next()
                    .ok_or_else(|| format!("invalid date: {}", s))?
                    .to_string(),
                parts
                    .next()
                    .ok_or_else(|| format!("invalid date: {}", s))?
                    .to_string(),
                parts
                    .next()
                    .ok_or_else(|| format!("invalid date: {}", s))?
                    .to_string(),
            )
        };
        if year_part.is_empty() || month_part.is_empty() || day_part.is_empty() {
            return Err(format!("invalid date: {}", s));
        }
        let y = year_part
            .parse::<i64>()
            .map_err(|_| format!("bad year: {}", year_part))?;
        let m = month_part
            .parse::<i64>()
            .map_err(|_| format!("bad month: {}", month_part))?;
        let d = day_part
            .parse::<i64>()
            .map_err(|_| format!("bad day: {}", day_part))?;
        if m < 1 || m > 12 || d < 1 || d > Self::days_in_month_of(y, m) {
            return Err(format!("date out of range: {}", s));
        }
        Ok(JetDate::new(y, m, d))
    }
    pub(crate) fn parse_iso_week_date(s: &str) -> Result<JetDate, String> {
        let (year_text, week_text) = s
            .split_once("-W")
            .ok_or_else(|| format!("invalid ISO week date: {}", s))?;
        let (week_text, day_text) = week_text
            .split_once('-')
            .ok_or_else(|| format!("invalid ISO week date: {}", s))?;
        if year_text.is_empty()
            || week_text.len() != 2
            || day_text.len() != 1
            || !week_text.bytes().all(|byte| byte.is_ascii_digit())
            || !day_text.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(format!("invalid ISO week date: {}", s));
        }
        let year = year_text
            .parse::<i64>()
            .map_err(|_| format!("bad ISO week year: {}", year_text))?;
        let week = week_text
            .parse::<i64>()
            .map_err(|_| format!("bad ISO week: {}", week_text))?;
        let weekday = day_text
            .parse::<i64>()
            .map_err(|_| format!("bad ISO weekday: {}", day_text))?;
        Self::from_iso_week(year, week, weekday)
            .ok_or_else(|| format!("ISO week date out of range: {}", s))
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
        let y = self.year + total.div_euclid(12);
        let m = total.rem_euclid(12) + 1;
        let d = self.day.min(Self::days_in_month_of(y, m));
        JetDate::new(y, m, d)
    }
    pub(crate) fn diff_days(&self, other: &JetDate) -> i64 {
        self.to_day_number() - other.to_day_number()
    }
    pub(crate) fn weekday(&self) -> i64 {
        // Legacy D-TIMEDEPTH1 shape: 0=Sunday, 6=Saturday.
        (self.to_day_number() + 1).rem_euclid(7)
    }
    pub(crate) fn iso_weekday(&self) -> i64 {
        self.to_day_number().rem_euclid(7) + 1
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
        let january_fourth = JetDate::new(thursday.year, 1, 4);
        let week_one_monday = january_fourth.add_days(1 - january_fourth.iso_weekday());
        let this_monday = thursday.add_days(1 - thursday.iso_weekday());
        this_monday.diff_days(&week_one_monday).div_euclid(7) + 1
    }
    pub(crate) fn iso_week_year(&self) -> i64 {
        self.add_days(4 - self.iso_weekday()).year
    }
    pub(crate) fn iso_week_date(&self) -> (i64, i64, i64) {
        (self.iso_week_year(), self.iso_week(), self.iso_weekday())
    }
    pub(crate) fn from_iso_week(year: i64, week: i64, weekday: i64) -> Option<Self> {
        if !(1..=53).contains(&week) || !(1..=7).contains(&weekday) {
            return None;
        }
        let jan_four = Self::new(year, 1, 4);
        let monday = jan_four.add_days(1 - jan_four.iso_weekday());
        let date = monday.add_days((week - 1).saturating_mul(7).saturating_add(weekday - 1));
        (date.iso_week_year() == year && date.iso_week() == week).then_some(date)
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
    pub(crate) fn with_overflow(
        &self,
        year: i64,
        month: i64,
        day: i64,
        overflow: &str,
    ) -> Result<JetDate, String> {
        match overflow {
            "constrain" | "clamp" => Ok(Self::new(year, month, day)),
            "reject"
                if (1..=12).contains(&month)
                    && (1..=Self::days_in_month_of(year, month)).contains(&day) =>
            {
                Ok(Self::new(year, month, day))
            }
            "reject" => Err(format!(
                "date fields are outside the valid range: {year:04}-{month:02}-{day:02}"
            )),
            _ => Err(format!("invalid overflow policy: {overflow}")),
        }
    }
    pub(crate) fn add_period(&self, p: &JetPeriod) -> JetDate {
        self.add_months(p.years.saturating_mul(12).saturating_add(p.months))
            .add_days(p.days)
    }
    pub(crate) fn subtract_period(&self, p: &JetPeriod) -> JetDate {
        self.add_period(&p.negated())
    }
    pub(crate) fn until_ns(
        &self,
        other: &JetDate,
        _largest_unit: &str,
        smallest_unit: &str,
        rounding_mode: &str,
        increment: i64,
    ) -> i64 {
        jet_time_round_delta_ns(
            (other.diff_days(self) as i128).saturating_mul(86_400_000_000_000),
            smallest_unit,
            rounding_mode,
            increment,
        )
    }
    pub(crate) fn since_ns(
        &self,
        other: &JetDate,
        largest_unit: &str,
        smallest_unit: &str,
        rounding_mode: &str,
        increment: i64,
    ) -> i64 {
        other.until_ns(self, largest_unit, smallest_unit, rounding_mode, increment)
    }
    pub(crate) fn format_pattern(&self, pattern: &String) -> String {
        jet_time_format_pattern(pattern, self, &JetLocalTime::new(0, 0, 0), None)
    }
    pub(crate) fn format_checked(&self, pattern: &String) -> Result<String, String> {
        jet_time_format_pattern_checked(pattern, self, &JetLocalTime::new(0, 0, 0), None)
    }
    pub(crate) fn to_string_fmt(&self) -> String {
        format!(
            "{}-{:02}-{:02}",
            jet_time_year_string(self.year),
            self.month,
            self.day
        )
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct JetLocalTime {
    hour: i64,
    minute: i64,
    second: i64,
    nanos: u32,
}
impl JetLocalTime {
    pub(crate) fn new(hour: i64, minute: i64, second: i64) -> Self {
        Self::with_nanosecond(hour, minute, second, 0)
    }
    pub(crate) fn with_nanosecond(hour: i64, minute: i64, second: i64, nanos: u32) -> Self {
        JetLocalTime {
            hour: hour.clamp(0, 23),
            minute: minute.clamp(0, 59),
            second: second.clamp(0, 59),
            nanos: nanos.min(999_999_999),
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
        let (second_part, nanos) = match parts[2].split_once('.') {
            Some((second, fraction)) => {
                if fraction.is_empty()
                    || fraction.len() > 9
                    || !fraction.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err(format!("bad fractional second: {}", parts[2]));
                }
                let padded = format!("{:0<9}", fraction);
                (
                    second,
                    padded
                        .parse::<u32>()
                        .map_err(|_| format!("bad fractional second: {}", fraction))?,
                )
            }
            None => (parts[2], 0),
        };
        let sec = second_part
            .parse::<i64>()
            .map_err(|_| format!("bad second: {}", second_part))?;
        if h < 0 || h > 23 || m < 0 || m > 59 || sec < 0 || sec > 59 {
            return Err(format!("time out of range: {}", s));
        }
        Ok(Self::with_nanosecond(h, m, sec, nanos))
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
    pub(crate) fn millisecond(&self) -> i64 {
        (self.nanos / 1_000_000) as i64
    }
    pub(crate) fn microsecond(&self) -> i64 {
        (self.nanos / 1_000) as i64
    }
    pub(crate) fn nanosecond(&self) -> i64 {
        self.nanos as i64
    }
    pub(crate) fn to_seconds(&self) -> i64 {
        self.hour * 3600 + self.minute * 60 + self.second
    }
    pub(crate) fn to_nanoseconds(&self) -> i128 {
        (self.to_seconds() as i128) * 1_000_000_000 + self.nanos as i128
    }
    pub(crate) fn add_duration_ns(&self, ns: i64) -> JetLocalTime {
        const DAY_NS: i128 = 86_400_000_000_000;
        let value = (self.to_nanoseconds() + ns as i128).rem_euclid(DAY_NS);
        let seconds = value.div_euclid(1_000_000_000);
        Self::with_nanosecond(
            (seconds / 3600) as i64,
            ((seconds / 60) % 60) as i64,
            (seconds % 60) as i64,
            value.rem_euclid(1_000_000_000) as u32,
        )
    }
    pub(crate) fn subtract_duration_ns(&self, ns: i64) -> JetLocalTime {
        self.add_duration_ns(ns.saturating_neg())
    }
    pub(crate) fn difference_ns(&self, other: &JetLocalTime) -> i64 {
        (self.to_nanoseconds() - other.to_nanoseconds()).clamp(i64::MIN as i128, i64::MAX as i128)
            as i64
    }
    pub(crate) fn until_ns(
        &self,
        other: &JetLocalTime,
        _largest_unit: &str,
        smallest_unit: &str,
        rounding_mode: &str,
        increment: i64,
    ) -> i64 {
        jet_time_round_delta_ns(
            other.to_nanoseconds() - self.to_nanoseconds(),
            smallest_unit,
            rounding_mode,
            increment,
        )
    }
    pub(crate) fn since_ns(
        &self,
        other: &JetLocalTime,
        largest_unit: &str,
        smallest_unit: &str,
        rounding_mode: &str,
        increment: i64,
    ) -> i64 {
        other.until_ns(self, largest_unit, smallest_unit, rounding_mode, increment)
    }
    pub(crate) fn round_with(&self, unit: &str, increment: i64, mode: &str) -> JetLocalTime {
        let rounded = jet_time_round_epoch_ns(self.to_nanoseconds(), unit, mode, increment)
            .rem_euclid(JET_NANOS_PER_DAY);
        let seconds = rounded.div_euclid(JET_NANOS_PER_SECOND);
        Self::with_nanosecond(
            (seconds / 3600) as i64,
            ((seconds / 60) % 60) as i64,
            (seconds % 60) as i64,
            rounded.rem_euclid(JET_NANOS_PER_SECOND) as u32,
        )
    }
    pub(crate) fn truncate_with(&self, unit: &str, increment: i64) -> JetLocalTime {
        self.round_with(unit, increment, "trunc")
    }
    pub(crate) fn floor_with(&self, unit: &str, increment: i64) -> JetLocalTime {
        self.round_with(unit, increment, "floor")
    }
    pub(crate) fn ceil_with(&self, unit: &str, increment: i64) -> JetLocalTime {
        self.round_with(unit, increment, "ceil")
    }
    pub(crate) fn round(&self, unit: &String) -> JetLocalTime {
        self.round_with(unit, 1, "half_expand")
    }
    pub(crate) fn format_pattern(&self, pattern: &String) -> String {
        jet_time_format_pattern(pattern, &JetDate::new(1970, 1, 1), self, None)
    }
    pub(crate) fn format_checked(&self, pattern: &String) -> Result<String, String> {
        jet_time_format_pattern_checked(pattern, &JetDate::new(1970, 1, 1), self, None)
    }
    pub(crate) fn to_string_fmt(&self) -> String {
        if self.nanos == 0 {
            format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
        } else {
            format!(
                "{:02}:{:02}:{:02}.{:09}",
                self.hour, self.minute, self.second, self.nanos
            )
        }
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
    pub(crate) fn years_value(&self) -> i64 {
        self.years
    }
    pub(crate) fn months_value(&self) -> i64 {
        self.months
    }
    pub(crate) fn days_value(&self) -> i64 {
        self.days
    }
    pub(crate) fn sign(&self) -> i64 {
        if self.years != 0 {
            self.years.signum()
        } else if self.months != 0 {
            self.months.signum()
        } else {
            self.days.signum()
        }
    }
    pub(crate) fn is_zero(&self) -> bool {
        self.years == 0 && self.months == 0 && self.days == 0
    }
    pub(crate) fn negated(&self) -> Self {
        Self::new(
            self.years.saturating_neg(),
            self.months.saturating_neg(),
            self.days.saturating_neg(),
        )
    }
    pub(crate) fn abs(&self) -> Self {
        Self::new(
            self.years.saturating_abs(),
            self.months.saturating_abs(),
            self.days.saturating_abs(),
        )
    }
    pub(crate) fn add(&self, other: &Self) -> Self {
        Self::new(
            self.years.saturating_add(other.years),
            self.months.saturating_add(other.months),
            self.days.saturating_add(other.days),
        )
    }
    pub(crate) fn sub(&self, other: &Self) -> Self {
        self.add(&other.negated())
    }
    pub(crate) fn total_in_date(&self, unit: &str, anchor: &JetDate) -> f64 {
        let end = anchor.add_period(self);
        let month_delta = self.years.saturating_mul(12).saturating_add(self.months);
        let calendar_anchor = anchor.add_months(month_delta);
        let residual_days = end.diff_days(&calendar_anchor) as f64;
        match unit {
            "day" | "days" | "d" => end.diff_days(anchor) as f64,
            "week" | "weeks" | "w" => end.diff_days(anchor) as f64 / 7.0,
            "month" | "months" => {
                month_delta as f64 + residual_days / calendar_anchor.days_in_month() as f64
            }
            "year" | "years" | "y" => {
                month_delta as f64 / 12.0
                    + residual_days
                        / (if calendar_anchor.is_leap_year() {
                            366.0
                        } else {
                            365.0
                        })
            }
            _ => 0.0,
        }
    }
    pub(crate) fn total_in_datetime(&self, unit: &str, anchor: &JetDateTime) -> f64 {
        let end = anchor.add_period(self);
        let month_delta = self.years.saturating_mul(12).saturating_add(self.months);
        let calendar_date = anchor.date().add_months(month_delta);
        let anchor_time = anchor.time();
        let calendar_anchor = JetDateTime::from_parts(
            calendar_date.year(),
            calendar_date.month(),
            calendar_date.day(),
            anchor_time.hour(),
            anchor_time.minute(),
            anchor_time.second(),
            anchor_time.nanosecond() as u32,
        );
        let nanoseconds = (end.total_nanoseconds() - anchor.total_nanoseconds()) as f64;
        let residual_nanoseconds =
            (end.total_nanoseconds() - calendar_anchor.total_nanoseconds()) as f64;
        match unit {
            "nanosecond" | "nanoseconds" | "ns" => nanoseconds,
            "microsecond" | "microseconds" | "us" | "µs" => nanoseconds / 1_000.0,
            "millisecond" | "milliseconds" | "ms" => nanoseconds / 1_000_000.0,
            "second" | "seconds" | "s" => nanoseconds / 1_000_000_000.0,
            "minute" | "minutes" | "min" => nanoseconds / 60_000_000_000.0,
            "hour" | "hours" | "h" => nanoseconds / 3_600_000_000_000.0,
            "day" | "days" | "d" => nanoseconds / 86_400_000_000_000.0,
            "week" | "weeks" | "w" => nanoseconds / 604_800_000_000_000.0,
            "month" | "months" => {
                month_delta as f64
                    + residual_nanoseconds
                        / (calendar_date.days_in_month() as f64 * JET_NANOS_PER_DAY as f64)
            }
            "year" | "years" | "y" => {
                month_delta as f64 / 12.0
                    + residual_nanoseconds
                        / ((if calendar_date.is_leap_year() {
                            366
                        } else {
                            365
                        }) as f64
                            * JET_NANOS_PER_DAY as f64)
            }
            _ => 0.0,
        }
    }
    pub(crate) fn to_string_fmt(&self) -> String {
        format!("P{}Y{}M{}D", self.years, self.months, self.days)
    }
    pub(crate) fn components(&self) -> (i64, i64, i64) {
        (self.years, self.months, self.days)
    }
}

pub(crate) fn jet_time_instant_add_duration_ns(start_ns: i64, duration_ns: i64) -> i64 {
    start_ns.saturating_add(duration_ns)
}

pub(crate) fn jet_time_instant_sub_duration_ns(start_ns: i64, duration_ns: i64) -> i64 {
    start_ns.saturating_sub(duration_ns)
}

pub(crate) fn jet_time_instant_difference_ns(left_ns: i64, right_ns: i64) -> i64 {
    left_ns.saturating_sub(right_ns)
}

pub(crate) fn jet_time_instant_elapsed_ns(now_ns: i64, start_ns: i64) -> i64 {
    now_ns.saturating_sub(start_ns)
}

pub(crate) fn jet_time_instant_compare(left_ns: i64, right_ns: i64) -> i64 {
    match left_ns.cmp(&right_ns) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

const JET_NANOS_PER_SECOND: i128 = 1_000_000_000;
const JET_NANOS_PER_DAY: i128 = 86_400_000_000_000;

fn jet_time_round_delta_ns(value: i128, unit: &str, mode: &str, increment: i64) -> i64 {
    jet_duration_kernel_round_i128(value, unit, increment, mode)
        .unwrap_or(value)
        .clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

fn jet_time_round_epoch_ns(value: i128, unit: &str, mode: &str, increment: i64) -> i128 {
    jet_duration_kernel_round_i128(value, unit, increment, mode).unwrap_or(value)
}

fn jet_time_epoch_ns(secs: i64, nanos: u32) -> i128 {
    (secs as i128) * JET_NANOS_PER_SECOND + nanos as i128
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
    pub(crate) fn from_unix_seconds(seconds: i64) -> Self {
        Self::from_timestamp(seconds)
    }
    pub(crate) fn from_unix_microseconds(microseconds: i64) -> Self {
        let secs = microseconds.div_euclid(1_000_000);
        let nanos = microseconds.rem_euclid(1_000_000) as u32 * 1_000;
        Self::from_timestamp_ns(secs, nanos)
    }
    pub(crate) fn from_unix_nanoseconds(nanoseconds: i64) -> Self {
        Self::from_timestamp_ns(
            nanoseconds.div_euclid(1_000_000_000),
            nanoseconds.rem_euclid(1_000_000_000) as u32,
        )
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
        JetLocalTime::with_nanosecond(sec / 3600, (sec / 60) % 60, sec % 60, self.nanos)
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
        self.total_nanoseconds()
            .div_euclid(1_000_000)
            .clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }
    pub(crate) fn to_unix_seconds(&self) -> i64 {
        self.total_nanoseconds()
            .div_euclid(JET_NANOS_PER_SECOND)
            .clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }
    pub(crate) fn to_unix_microseconds(&self) -> Result<i64, String> {
        self.to_unix_unit(1_000, "microseconds")
    }
    pub(crate) fn to_unix_nanoseconds(&self) -> Result<i64, String> {
        self.to_unix_unit(1, "nanoseconds")
    }
    fn to_unix_unit(&self, nanos_per_unit: i128, unit: &str) -> Result<i64, String> {
        i64::try_from(self.total_nanoseconds().div_euclid(nanos_per_unit))
            .map_err(|_| format!("E2704: Unix epoch {unit} do not fit in Int"))
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
        if !rest.is_ascii() {
            return Err(format!("invalid RFC3339 datetime: {}", s));
        }
        let date = JetDate::parse(date_part)?;
        let (time_part, zone_part) = if rest.ends_with('Z') {
            (&rest[..rest.len() - 1], "Z")
        } else if rest.len() >= 6 {
            rest.split_at(rest.len() - 6)
        } else {
            return Err(format!("RFC3339 datetime needs Z or an offset: {}", s));
        };
        let time = JetLocalTime::parse(time_part)?;
        let nanos = time.nanosecond() as u32;
        let offset = jet_time_parse_offset(zone_part)?;
        Ok(JetDateTime {
            secs: jet_time_utc_from_parts(&date, &time).saturating_sub(offset),
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
                "{}T{:02}:{:02}:{:02}.{:09}Z",
                d.to_string_fmt(),
                t.hour(),
                t.minute(),
                t.second(),
                self.nanos
            )
        }
    }
    pub(crate) fn format_pattern(&self, pattern: &String) -> String {
        jet_time_format_pattern(pattern, &self.date(), &self.time(), None)
    }
    pub(crate) fn format_checked(&self, pattern: &String) -> Result<String, String> {
        jet_time_format_pattern_checked(pattern, &self.date(), &self.time(), None)
    }
    pub(crate) fn plus_duration_ns(&self, ns: i64) -> JetDateTime {
        let total = (self.secs as i128)
            .saturating_mul(1_000_000_000)
            .saturating_add(self.nanos as i128)
            .saturating_add(ns as i128);
        let secs = total
            .div_euclid(1_000_000_000)
            .clamp(i64::MIN as i128, i64::MAX as i128) as i64;
        let nanos = total.rem_euclid(1_000_000_000) as u32;
        JetDateTime { secs, nanos }
    }
    pub(crate) fn total_nanoseconds(&self) -> i128 {
        jet_time_epoch_ns(self.secs, self.nanos)
    }
    pub(crate) fn difference_ns(&self, other: &JetDateTime) -> i64 {
        (self.total_nanoseconds() - other.total_nanoseconds())
            .clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }
    pub(crate) fn add_period(&self, period: &JetPeriod) -> JetDateTime {
        let date = self.date().add_period(period);
        let time = self.time();
        Self::from_parts(
            date.year(),
            date.month(),
            date.day(),
            time.hour(),
            time.minute(),
            time.second(),
            time.nanosecond() as u32,
        )
    }
    pub(crate) fn subtract_duration_ns(&self, ns: i64) -> JetDateTime {
        self.plus_duration_ns(ns.saturating_neg())
    }
    pub(crate) fn subtract_period(&self, period: &JetPeriod) -> JetDateTime {
        self.add_period(&period.negated())
    }
    pub(crate) fn with_overflow(
        &self,
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
        overflow: &str,
    ) -> Result<JetDateTime, String> {
        let date = self.date().with_overflow(year, month, day, overflow)?;
        if !matches!(overflow, "constrain" | "clamp" | "reject") {
            return Err(format!("invalid overflow policy: {overflow}"));
        }
        if overflow == "reject"
            && (!(0..=23).contains(&hour)
                || !(0..=59).contains(&minute)
                || !(0..=59).contains(&second))
        {
            return Err(format!(
                "time fields are outside the valid range: {hour:02}:{minute:02}:{second:02}"
            ));
        }
        Ok(Self::from_parts(
            date.year(),
            date.month(),
            date.day(),
            hour,
            minute,
            second,
            self.nanos,
        ))
    }
    pub(crate) fn until_ns(
        &self,
        other: &JetDateTime,
        _largest_unit: &str,
        smallest_unit: &str,
        rounding_mode: &str,
        increment: i64,
    ) -> i64 {
        jet_time_round_delta_ns(
            other.total_nanoseconds() - self.total_nanoseconds(),
            smallest_unit,
            rounding_mode,
            increment,
        )
    }
    pub(crate) fn since_ns(
        &self,
        other: &JetDateTime,
        largest_unit: &str,
        smallest_unit: &str,
        rounding_mode: &str,
        increment: i64,
    ) -> i64 {
        other.until_ns(self, largest_unit, smallest_unit, rounding_mode, increment)
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
        self.round_with(unit, 1, "half_expand")
    }
    pub(crate) fn round_with(&self, unit: &str, increment: i64, mode: &str) -> JetDateTime {
        let rounded = jet_time_round_epoch_ns(self.total_nanoseconds(), unit, mode, increment);
        Self::from_timestamp_ns(
            rounded
                .div_euclid(JET_NANOS_PER_SECOND)
                .clamp(i64::MIN as i128, i64::MAX as i128) as i64,
            rounded.rem_euclid(JET_NANOS_PER_SECOND) as u32,
        )
    }
    pub(crate) fn truncate_with(&self, unit: &str, increment: i64) -> JetDateTime {
        self.round_with(unit, increment, "trunc")
    }
    pub(crate) fn floor_with(&self, unit: &str, increment: i64) -> JetDateTime {
        self.round_with(unit, increment, "floor")
    }
    pub(crate) fn ceil_with(&self, unit: &str, increment: i64) -> JetDateTime {
        self.round_with(unit, increment, "ceil")
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
        #[cfg(target_arch = "wasm32")]
        if let Some(bytes) = jet_time_embedded_zone_bytes(rel) {
            return Self::parse_tzif(name.clone(), bytes);
        }
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
            0
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
        self.local_to_utc_with_disambiguation(date, time, "compatible")
            .unwrap_or_else(|_| {
                let local = jet_time_utc_from_parts(date, time);
                local.saturating_sub(self.offset_at_utc(local))
            })
    }
    pub(crate) fn local_to_utc_with_disambiguation(
        &self,
        date: &JetDate,
        time: &JetLocalTime,
        disambiguation: &str,
    ) -> Result<i64, String> {
        if !matches!(
            disambiguation,
            "compatible" | "earlier" | "later" | "reject"
        ) {
            return Err(format!("invalid disambiguation: {}", disambiguation));
        }
        let local_seconds = jet_time_utc_from_parts(date, time);
        let mut offsets = Vec::new();
        for info in &self.infos {
            if !offsets.contains(&info.offset) {
                offsets.push(info.offset);
            }
        }
        let mut candidates = offsets
            .into_iter()
            .filter_map(|offset| {
                let utc = local_seconds.saturating_sub(offset);
                let (candidate_date, candidate_time, actual_offset) = self.local_parts(utc);
                (actual_offset == offset
                    && candidate_date == *date
                    // TZif transitions resolve whole UTC seconds.  The
                    // requested fraction is carried by the resulting
                    // DateTime, so compare the civil second here and retain
                    // `time.nanosecond()` at the caller.
                    && candidate_time.to_seconds() == time.to_seconds())
                .then_some(utc)
            })
            .collect::<Vec<_>>();
        candidates.sort_unstable();
        candidates.dedup();
        if candidates.len() == 1 {
            return Ok(candidates[0]);
        }
        if candidates.len() > 1 {
            return match disambiguation {
                "compatible" | "earlier" => Ok(candidates[0]),
                "later" => Ok(*candidates.last().unwrap_or(&candidates[0])),
                "reject" => Err(format!("ambiguous local time in {}", self.name)),
                _ => Err(format!("invalid disambiguation: {}", disambiguation)),
            };
        }

        for (index, (transition, after_index)) in self.transitions.iter().enumerate() {
            let before_offset = if index == 0 {
                self.infos.first().map(|info| info.offset).unwrap_or(0)
            } else {
                self.infos
                    .get(self.transitions[index - 1].1)
                    .map(|info| info.offset)
                    .unwrap_or(0)
            };
            let after_offset = self
                .infos
                .get(*after_index)
                .map(|info| info.offset)
                .unwrap_or(before_offset);
            if after_offset > before_offset {
                let gap_start = transition.saturating_add(before_offset);
                let gap_end = transition.saturating_add(after_offset);
                if local_seconds >= gap_start && local_seconds < gap_end {
                    return match disambiguation {
                        "compatible" | "later" => Ok(local_seconds.saturating_sub(before_offset)),
                        "earlier" => Ok(local_seconds.saturating_sub(after_offset)),
                        "reject" => Err(format!("nonexistent local time in {}", self.name)),
                        _ => unreachable!("disambiguation was validated above"),
                    };
                }
            }
        }
        match disambiguation {
            "reject" => Err(format!("local time is not valid in {}", self.name)),
            "compatible" | "earlier" | "later" => {
                Err(format!("could not resolve local time in {}", self.name))
            }
            _ => Err(format!("invalid disambiguation: {}", disambiguation)),
        }
    }
    pub(crate) fn local_to_utc_with_offset(
        &self,
        date: &JetDate,
        time: &JetLocalTime,
        offset: i64,
    ) -> Option<i64> {
        let utc = jet_time_utc_from_parts(date, time).saturating_sub(offset);
        let (candidate_date, candidate_time, actual_offset) = self.local_parts(utc);
        (actual_offset == offset
            && candidate_date == *date
            && candidate_time.to_seconds() == time.to_seconds())
        .then_some(utc)
    }
    pub(crate) fn next_transition(&self, utc_seconds: i64) -> Option<i64> {
        self.transitions
            .iter()
            .map(|(transition, _)| *transition)
            .find(|transition| *transition > utc_seconds)
    }
    pub(crate) fn previous_transition(&self, utc_seconds: i64) -> Option<i64> {
        self.transitions
            .iter()
            .rev()
            .map(|(transition, _)| *transition)
            .find(|transition| *transition < utc_seconds)
    }
    pub(crate) fn start_of_day(&self, date: &JetDate) -> i64 {
        self.local_to_utc(date, &JetLocalTime::new(0, 0, 0))
    }
    pub(crate) fn start_of_day_zoned(&self, date: &JetDate) -> JetZonedDateTime {
        JetZonedDateTime {
            instant: JetDateTime::from_timestamp(self.start_of_day(date)),
            zone: self.clone(),
        }
    }
    pub(crate) fn hours_in_day(&self, date: &JetDate) -> i64 {
        let start = self.start_of_day(date);
        let next = self.start_of_day(&date.add_days(1));
        next.saturating_sub(start) / 3600
    }
}

#[derive(Clone, Debug)]
pub(crate) struct JetZonedDateTime {
    instant: JetDateTime,
    zone: JetZone,
}

impl PartialEq for JetZonedDateTime {
    fn eq(&self, other: &Self) -> bool {
        self.instant == other.instant && self.zone.name == other.zone.name
    }
}

impl Eq for JetZonedDateTime {}

impl PartialOrd for JetZonedDateTime {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JetZonedDateTime {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.instant.cmp(&other.instant)
    }
}

impl JetZonedDateTime {
    pub(crate) fn now(zone: &JetZone) -> Self {
        JetDateTime::now().in_zone(zone)
    }
    pub(crate) fn from_local(date: &JetDate, time: &JetLocalTime, zone: &JetZone) -> Self {
        Self::from_local_with_disambiguation(date, time, zone, "compatible").unwrap_or_else(|_| {
            JetZonedDateTime {
                instant: JetDateTime::from_timestamp(zone.local_to_utc(date, time)),
                zone: zone.clone(),
            }
        })
    }
    pub(crate) fn from_local_with_disambiguation(
        date: &JetDate,
        time: &JetLocalTime,
        zone: &JetZone,
        disambiguation: &str,
    ) -> Result<Self, String> {
        let seconds = zone.local_to_utc_with_disambiguation(date, time, disambiguation)?;
        Ok(JetZonedDateTime {
            instant: JetDateTime::from_timestamp_ns(seconds, time.nanosecond() as u32),
            zone: zone.clone(),
        })
    }
    pub(crate) fn from_rfc9557(value: &str) -> Result<Self, String> {
        let head = value
            .strip_suffix(']')
            .ok_or_else(|| format!("RFC9557 datetime needs a [zone]: {}", value))?;
        let (datetime_text, zone_name) = head
            .rsplit_once('[')
            .ok_or_else(|| format!("RFC9557 datetime needs a [zone]: {}", value))?;
        let zone = JetZone::named(&zone_name.to_string())?;
        let (date_text, rest) = datetime_text
            .split_once('T')
            .ok_or_else(|| format!("invalid RFC9557 datetime: {}", value))?;
        if !rest.is_ascii() {
            return Err(format!("invalid RFC9557 datetime: {}", value));
        }
        let (time_text, offset_text) = if rest.ends_with('Z') {
            (&rest[..rest.len() - 1], "Z")
        } else if rest.len() >= 6 {
            rest.split_at(rest.len() - 6)
        } else {
            return Err(format!("RFC9557 datetime needs an offset: {}", value));
        };
        let offset = jet_time_parse_offset(offset_text)?;
        let date = JetDate::parse(date_text)?;
        let time = JetLocalTime::parse(time_text)?;
        let utc = zone
            .local_to_utc_with_offset(&date, &time, offset)
            .ok_or_else(|| format!("RFC9557 offset does not match zone {}", zone.name))?;
        let parsed = JetDateTime::parse_rfc3339(datetime_text)?;
        Ok(Self {
            instant: JetDateTime::from_timestamp_ns(utc, parsed.nanosecond() as u32),
            zone,
        })
    }
    pub(crate) fn date(&self) -> JetDate {
        self.zone.local_parts(self.instant.secs).0
    }
    pub(crate) fn time(&self) -> JetLocalTime {
        let (_, time, _) = self.zone.local_parts(self.instant.secs);
        JetLocalTime::with_nanosecond(
            time.hour(),
            time.minute(),
            time.second(),
            self.instant.nanosecond() as u32,
        )
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
    pub(crate) fn subtract_duration_ns(&self, ns: i64) -> JetZonedDateTime {
        self.add_duration_ns(ns.saturating_neg())
    }
    pub(crate) fn add_period(&self, p: &JetPeriod) -> JetZonedDateTime {
        let date = self.date().add_period(p);
        let time = self.time();
        JetZonedDateTime::from_local(&date, &time, &self.zone)
    }
    pub(crate) fn subtract_period(&self, p: &JetPeriod) -> JetZonedDateTime {
        self.add_period(&p.negated())
    }
    pub(crate) fn with_time(
        &self,
        time: &JetLocalTime,
        disambiguation: &str,
    ) -> Result<JetZonedDateTime, String> {
        Self::from_local_with_disambiguation(&self.date(), time, &self.zone, disambiguation)
    }
    pub(crate) fn with_zone(&self, zone: &JetZone) -> JetZonedDateTime {
        JetZonedDateTime {
            instant: self.instant.clone(),
            zone: zone.clone(),
        }
    }
    pub(crate) fn until_ns(
        &self,
        other: &JetZonedDateTime,
        largest_unit: &str,
        smallest_unit: &str,
        rounding_mode: &str,
        increment: i64,
    ) -> i64 {
        self.instant.until_ns(
            &other.instant,
            largest_unit,
            smallest_unit,
            rounding_mode,
            increment,
        )
    }
    pub(crate) fn since_ns(
        &self,
        other: &JetZonedDateTime,
        largest_unit: &str,
        smallest_unit: &str,
        rounding_mode: &str,
        increment: i64,
    ) -> i64 {
        other.until_ns(self, largest_unit, smallest_unit, rounding_mode, increment)
    }
    pub(crate) fn next_transition(&self) -> Option<i64> {
        self.zone.next_transition(self.instant.to_timestamp())
    }
    pub(crate) fn previous_transition(&self) -> Option<i64> {
        self.zone.previous_transition(self.instant.to_timestamp())
    }
    pub(crate) fn start_of_day(&self) -> JetZonedDateTime {
        JetZonedDateTime {
            instant: JetDateTime::from_timestamp(self.zone.start_of_day(&self.date())),
            zone: self.zone.clone(),
        }
    }
    pub(crate) fn hours_in_day(&self) -> i64 {
        self.zone.hours_in_day(&self.date())
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
    pub(crate) fn format_checked(&self, pattern: &String) -> Result<String, String> {
        let date = self.date();
        let time = self.time();
        jet_time_format_pattern_checked(
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
    pub(crate) fn format_rfc9557(&self) -> String {
        let (date, time, offset) = self.zone.local_parts(self.instant.to_timestamp());
        let fraction = if self.instant.nanosecond() == 0 {
            String::new()
        } else {
            format!(".{:09}", self.instant.nanosecond())
        };
        let zone_suffix = format!("[{}]", self.zone.name);
        format!(
            "{}T{:02}:{:02}:{:02}{}{}{}",
            date.to_string_fmt(),
            time.hour(),
            time.minute(),
            time.second(),
            fraction,
            if offset == 0 {
                "Z".to_string()
            } else {
                jet_time_offset_string(offset)
            },
            zone_suffix
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

fn jet_time_parse_offset(value: &str) -> Result<i64, String> {
    if value == "Z" {
        return Ok(0);
    }
    let bytes = value.as_bytes();
    if bytes.len() != 6 || bytes[3] != b':' {
        return Err(format!("bad RFC3339 offset: {}", value));
    }
    let sign = match bytes.first() {
        Some(b'-') => -1,
        Some(b'+') => 1,
        _ => return Err(format!("bad RFC3339 offset: {}", value)),
    };
    let hours = &value[1..3];
    let minutes = &value[4..6];
    if !hours.bytes().all(|byte| byte.is_ascii_digit())
        || !minutes.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(format!("bad RFC3339 offset: {}", value));
    }
    let hours = hours
        .parse::<i64>()
        .map_err(|_| format!("bad RFC3339 offset hour: {}", hours))?;
    let minutes = minutes
        .parse::<i64>()
        .map_err(|_| format!("bad RFC3339 offset minute: {}", minutes))?;
    if hours < 0 || hours > 23 || minutes < 0 || minutes > 59 {
        return Err(format!("RFC3339 offset out of range: {}", value));
    }
    Ok(sign * (hours * 3600 + minutes * 60))
}

fn jet_time_year_string(year: i64) -> String {
    if year < 0 {
        format!("-{:04}", year.saturating_abs())
    } else {
        format!("{:04}", year)
    }
}

pub(crate) fn jet_time_format_pattern(
    pattern: &String,
    date: &JetDate,
    time: &JetLocalTime,
    zone: Option<(&JetZone, i64)>,
) -> String {
    let mut out = pattern.clone();
    let weekday_index = (date.iso_weekday() - 1) as usize;
    let weekday = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][weekday_index];
    let weekday_full = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ][weekday_index];
    let month_short = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][(date.month - 1) as usize];
    let month_full = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ][(date.month - 1) as usize];
    let year = jet_time_year_string(date.year);
    let year_short = format!("{:02}", date.year.rem_euclid(100));
    let month = format!("{:02}", date.month);
    let day = format!("{:02}", date.day);
    let day_of_year = format!("{:03}", date.day_of_year());
    let hour = format!("{:02}", time.hour);
    let hour_12 = format!("{:02}", ((time.hour + 11) % 12) + 1);
    let minute = format!("{:02}", time.minute);
    let second = format!("{:02}", time.second);
    let milliseconds = format!("{:03}", time.millisecond());
    let microseconds = format!("{:06}", time.nanosecond() / 1_000);
    let nanoseconds = format!("{:09}", time.nanosecond());
    let meridiem = if time.hour < 12 { "AM" } else { "PM" };
    let offset = zone
        .map(|(_, seconds)| jet_time_offset_string(seconds))
        .unwrap_or_default();
    let zone_name = zone.map(|(z, _)| z.name.as_str()).unwrap_or_default();

    // Strftime-style codes. Replace %% first so a formatted value can never
    // be interpreted as another code.
    out = out.replace("%%", "%");
    out = out.replace("%A", weekday_full);
    out = out.replace("%a", weekday);
    out = out.replace("%B", month_full);
    out = out.replace("%b", month_short);
    out = out.replace("%Y", &year);
    out = out.replace("%y", &year_short);
    out = out.replace("%m", &month);
    out = out.replace("%d", &day);
    out = out.replace("%e", &format!("{:2}", date.day));
    out = out.replace("%j", &day_of_year);
    out = out.replace("%H", &hour);
    out = out.replace("%I", &hour_12);
    out = out.replace("%M", &minute);
    out = out.replace("%S", &second);
    out = out.replace("%p", meridiem);
    out = out.replace("%z", &offset);
    out = out.replace("%Z", zone_name);
    out = out.replace("%F", &format!("{}-{}-{}", year, month, day));
    out = out.replace("%T", &format!("{}:{}:{}", hour, minute, second));
    out = out.replace("%R", &format!("{}:{}", hour, minute));
    out = out.replace("%D", &format!("{}/{}/{}", month, day, year_short));
    out = out.replace("%f", &nanoseconds);

    // The original Jet tokens remain supported. Longer tokens must be
    // replaced first so `MMMM` is not read as four `MM` tokens.
    out = out.replace("EEEE", weekday_full);
    out = out.replace("MMMM", month_full);
    out = out.replace("MMM", month_short);
    out = out.replace("yyyy", &year);
    out = out.replace("DDD", &format!("{:03}", date.day_of_year()));
    out = out.replace("EEE", weekday);
    out = out.replace("MM", &format!("{:02}", date.month));
    out = out.replace("dd", &format!("{:02}", date.day));
    out = out.replace("HH", &format!("{:02}", time.hour));
    out = out.replace("mm", &format!("{:02}", time.minute));
    out = out.replace("ss", &format!("{:02}", time.second));
    out = out.replace("SSSSSSSSS", &nanoseconds);
    out = out.replace("SSSSSS", &microseconds);
    out = out.replace("SSS", &milliseconds);
    if let Some((z, off)) = zone {
        out = out.replace("VV", &z.name);
        out = out.replace("XXX", &jet_time_offset_string(off));
    }
    out
}

/// Validate the closed format grammar before applying the permissive legacy
/// formatter. Quoted text is protected from token replacement, so the checked
/// route cannot silently turn a typo into literal output.
pub(crate) fn jet_time_format_pattern_checked(
    pattern: &String,
    date: &JetDate,
    time: &JetLocalTime,
    zone: Option<(&JetZone, i64)>,
) -> Result<String, String> {
    const TOKENS: [&str; 16] = [
        "SSSSSSSSS",
        "EEEE",
        "MMMM",
        "yyyy",
        "DDD",
        "XXX",
        "SSSSSS",
        "MMM",
        "EEE",
        "VV",
        "MM",
        "dd",
        "HH",
        "mm",
        "ss",
        "SSS",
    ];
    let bytes = pattern.as_bytes();
    let mut normalized = String::new();
    let mut literals = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'\'' {
            i += 1;
            let mut literal = String::new();
            let mut closed = false;
            while i < bytes.len() {
                if bytes[i] == b'\'' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                        literal.push('\'');
                        i += 2;
                    } else {
                        i += 1;
                        closed = true;
                        break;
                    }
                } else {
                    let ch = pattern[i..]
                        .chars()
                        .next()
                        .ok_or_else(|| "E2703: invalid format literal".to_string())?;
                    literal.push(ch);
                    i += ch.len_utf8();
                }
            }
            if !closed {
                return Err("E2703: unterminated format literal".to_string());
            }
            let marker = format!("\u{e000}{0}\u{e001}", literals.len());
            normalized.push_str(&marker);
            literals.push((marker, literal));
            continue;
        }
        if byte == b'%' {
            let Some(code) = bytes.get(i + 1).copied() else {
                return Err("E2703: format ends after `%`".to_string());
            };
            if !matches!(
                code,
                b'%' | b'A'
                    | b'a'
                    | b'B'
                    | b'b'
                    | b'Y'
                    | b'y'
                    | b'm'
                    | b'd'
                    | b'e'
                    | b'j'
                    | b'H'
                    | b'I'
                    | b'M'
                    | b'S'
                    | b'p'
                    | b'z'
                    | b'Z'
                    | b'F'
                    | b'T'
                    | b'R'
                    | b'D'
                    | b'f'
            ) {
                return Err(format!(
                    "E2703: unsupported format token `%{}`",
                    code as char
                ));
            }
            if matches!(code, b'z' | b'Z') && zone.is_none() {
                return Err(format!(
                    "E2703: format token `%{}` requires a zone",
                    code as char
                ));
            }
            normalized.push('%');
            normalized.push(code as char);
            i += 2;
            continue;
        }
        if byte.is_ascii_alphabetic() {
            let token = TOKENS
                .iter()
                .find(|token| pattern[i..].starts_with(**token))
                .copied()
                .ok_or_else(|| format!("E2703: unsupported format token `{}`", byte as char))?;
            if matches!(token, "VV" | "XXX") && zone.is_none() {
                return Err(format!("E2703: format token `{token}` requires a zone"));
            }
            normalized.push_str(token);
            i += token.len();
            continue;
        }
        let ch = pattern[i..]
            .chars()
            .next()
            .ok_or_else(|| "E2703: invalid format character".to_string())?;
        normalized.push(ch);
        i += ch.len_utf8();
    }
    let mut out = jet_time_format_pattern(&normalized, date, time, zone);
    for (marker, literal) in literals {
        out = out.replace(&marker, &literal);
    }
    Ok(out)
}

// The public Core call and every evaluator route use this one Prelude symbol.
// Keeping the wrapper beside `JetDateTime` lets the evaluator reuse the exact
// parser without importing the larger time-surface fragment.
pub(crate) fn jet_time_parse_rfc3339(s: &String) -> Result<JetDateTime, String> {
    JetDateTime::parse_rfc3339(s)
}

pub(crate) fn jet_time_from_iso_week(
    year: i64,
    week: i64,
    weekday: i64,
) -> Result<JetDate, String> {
    JetDate::from_iso_week(year, week, weekday)
        .ok_or_else(|| format!("ISO week date out of range: {year}-W{week:02}-{weekday}"))
}

pub(crate) fn jet_time_parse_zoned(s: &String) -> Result<JetZonedDateTime, String> {
    JetZonedDateTime::from_rfc9557(s)
}
