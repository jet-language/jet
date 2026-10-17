//! Canonical dependency-free data tree shared by Foundation and the embedded Prelude.
//!
//! Every text format and protocol parser projects into this ordered carrier.  The
//! object representation deliberately stays a list so insertion order is retained;
//! parsers enforce duplicate-key policy at their boundary instead of collapsing data
//! through a map.

use super::Numeric::JetInt;

/// An instant and its frozen RFC 3339 offset. Leap labels retain the preceding
/// POSIX second, just like JetDateTime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DynDateTime {
    pub unix_seconds: i64,
    pub nanos: u32,
    pub leap_second: bool,
    pub offset_seconds: i32,
}

impl DynDateTime {
    pub fn instant_key(&self) -> (i64, bool, u32) {
        (self.unix_seconds, self.leap_second, self.nanos)
    }

    pub fn offset_text(&self) -> String {
        if self.offset_seconds == 0 { return "Z".to_owned(); }
        let offset = self.offset_seconds.abs();
        format!("{}{:02}:{:02}", if self.offset_seconds < 0 { "-" } else { "+" },
            offset / 3600, offset / 60 % 60)
    }

    pub fn to_rfc3339(&self) -> String {
        let local = self.unix_seconds as i128 + self.offset_seconds as i128;
        let days = local.div_euclid(86400);
        let seconds = local.rem_euclid(86400);
        // Gregorian civil date from Unix days (400-year eras).
        let z = days + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let mut year = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = mp + if mp < 10 { 3 } else { -9 };
        year += if month <= 2 { 1 } else { 0 };
        let mut text = format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
            seconds / 3600, seconds / 60 % 60,
            if self.leap_second { 60 } else { seconds % 60 });
        if self.nanos != 0 {
            let fraction = format!("{:09}", self.nanos);
            text.push('.');
            text.push_str(fraction.trim_end_matches('0'));
        }
        text.push_str(&self.offset_text());
        text
    }

    pub fn parse_rfc3339(text: &str) -> Result<Self, String> {
        fn invalid() -> String { "expected RFC 3339 date-time with a minute-aligned offset".to_owned() }
        let b = text.as_bytes();
        if b.len() < 20 || !text.is_ascii() || b[4] != b'-' || b[7] != b'-'
            || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' || b[16] != b':' {
            return Err(invalid());
        }
        fn number(b: &[u8]) -> Result<i64, String> {
            if !b.iter().all(u8::is_ascii_digit) { return Err(invalid()); }
            Ok(b.iter().fold(0, |n, c| n * 10 + i64::from(c - b'0')))
        }
        let year = number(&b[..4])?;
        let month = number(&b[5..7])?;
        let day = number(&b[8..10])?;
        let hour = number(&b[11..13])?;
        let minute = number(&b[14..16])?;
        let second = number(&b[17..19])?;
        let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days_in_month = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => if leap_year { 29 } else { 28 },
            _ => return Err(invalid()),
        };
        if !(1..=days_in_month).contains(&day) || hour > 23 || minute > 59 || second > 60 {
            return Err(invalid());
        }
        let mut pos = 19;
        let mut nanos = 0;
        if b[pos] == b'.' {
            pos += 1;
            let start = pos;
            while pos < b.len() && b[pos].is_ascii_digit() { pos += 1; }
            let count = pos - start;
            if count == 0 || count > 9 { return Err(invalid()); }
            nanos = number(&b[start..pos])? as u32 * 10u32.pow(9 - count as u32);
        }
        let offset_seconds = match &b[pos..] {
            [b'Z' | b'z'] => 0,
            [sign @ (b'+' | b'-'), a, c, b':', d, e] => {
                let h = number(&[*a, *c])?;
                let m = number(&[*d, *e])?;
                if h > 23 || m > 59 { return Err(invalid()); }
                ((h * 3600 + m * 60) * if *sign == b'-' { -1 } else { 1 }) as i32
            }
            _ => return Err(invalid()),
        };
        let y = year - if month <= 2 { 1 } else { 0 };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = month + if month > 2 { -3 } else { 9 };
        let doy = (153 * mp + 2) / 5 + day - 1;
        let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
        Ok(Self {
            unix_seconds: days * 86400 + hour * 3600 + minute * 60 + second.min(59)
                - i64::from(offset_seconds),
            nanos, leap_second: second == 60, offset_seconds,
        })
    }
}

/// RFC 4648 standard base64, shared by all text renderers.
pub fn dyn_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        out.push(TABLE[(a >> 2) as usize] as char);
        out.push(TABLE[((a & 3) << 4 | b >> 4) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((b & 15) << 2 | c >> 6) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(c & 63) as usize] as char } else { '=' });
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub enum Dyn {
    Null,
    Bool(bool),
    Int(JetInt),
    Float(f64),
    Number(String),
    Text(String),
    Bytes(Vec<u8>),
    DateTime(DynDateTime),
    Array(Vec<Dyn>),
    Object(Vec<(String, Dyn)>),
}

impl Dyn {
    pub fn bytes_text(bytes: &[u8]) -> String { dyn_base64(bytes) }

    pub fn datetime(unix_seconds: i64, nanos: u32, leap_second: bool, offset_seconds: i32) -> Self {
        Self::DateTime(DynDateTime { unix_seconds, nanos, leap_second, offset_seconds })
    }

    pub fn parse_datetime(text: &str) -> Result<Self, String> {
        DynDateTime::parse_rfc3339(text).map(Self::DateTime)
    }
    pub fn as_array(&self) -> Result<&Vec<Dyn>, String> {
        match self {
            Self::Array(values) => Ok(values),
            _ => Err("expected a Dyn array".to_string()),
        }
    }

    pub fn as_object(&self) -> Result<&Vec<(String, Dyn)>, String> {
        match self {
            Self::Object(values) => Ok(values),
            _ => Err("expected a Dyn object".to_string()),
        }
    }

    pub fn as_str(&self) -> Result<&str, String> {
        match self {
            Self::Text(value) => Ok(value),
            _ => Err("expected a Dyn string".to_string()),
        }
    }

    pub fn get<'a>(&'a self, key: &str) -> Result<&'a Dyn, String> {
        self.get_opt(key)
            .ok_or_else(|| format!("missing key `{key}`"))
    }

    pub fn get_opt(&self, key: &str) -> Option<&Dyn> {
        match self {
            Self::Object(values) => values
                .iter()
                .find_map(|(name, value)| (name == key).then_some(value)),
            _ => None,
        }
    }

    pub fn object_entries(&self) -> Option<&[(String, Dyn)]> {
        match self {
            Self::Object(values) => Some(values),
            _ => None,
        }
    }
}

impl std::hash::Hash for Dyn {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Null => {}
            Self::Bool(v) => v.hash(state),
            Self::Int(v) => v.hash(state),
            Self::Float(v) => (if *v == 0.0 { 0 } else { v.to_bits() }).hash(state),
            Self::Number(v) | Self::Text(v) => v.hash(state),
            Self::Bytes(v) => v.hash(state),
            Self::DateTime(v) => v.hash(state),
            Self::Array(v) => v.hash(state),
            Self::Object(v) => v.hash(state),
        }
    }
}

#[cfg(test)]
mod dyn_tests {
    use super::*;
    use std::hash::{Hash, Hasher};

    #[test]
    fn datetime_canonical_text_preserves_instant_offset_and_leap_label() {
        for text in ["1970-01-01T00:00:00Z", "1969-12-31T18:30:00-05:30",
            "2024-02-29T12:34:56.1234+05:30", "2016-12-31T23:59:60Z"] {
            let value = DynDateTime::parse_rfc3339(text).unwrap();
            assert_eq!(value.to_rfc3339(), text);
        }
        let a = DynDateTime::parse_rfc3339("1970-01-01T00:00:00Z").unwrap();
        let b = DynDateTime::parse_rfc3339("1970-01-01T05:30:00+05:30").unwrap();
        assert_eq!(a.instant_key(), b.instant_key());
        assert_ne!(a, b);
        assert_eq!(DynDateTime::parse_rfc3339("1970-01-01T00:00:00.1200+00:00")
            .unwrap().to_rfc3339(), "1970-01-01T00:00:00.12Z");
        for invalid in ["2023-02-29T00:00:00Z", "2024-01-01T00:00:00",
            "2024-01-01T00:00:00+05:30:15", "2024-01-01T00:00:00+24:00"] {
            assert!(DynDateTime::parse_rfc3339(invalid).is_err());
        }
    }

    #[test]
    fn binary_text_and_content_hash_match_equality() {
        for (bytes, text) in [(&b""[..], ""), (&b"f"[..], "Zg=="), (&b"fo"[..], "Zm8="),
            (&b"foo"[..], "Zm9v"), (&b"\0\xff"[..], "AP8=")] {
            assert_eq!(Dyn::bytes_text(bytes), text);
        }
        fn hash(value: &Dyn) -> u64 {
            let mut state = std::collections::hash_map::DefaultHasher::new();
            value.hash(&mut state);
            state.finish()
        }
        assert_eq!(hash(&Dyn::Float(0.0)), hash(&Dyn::Float(-0.0)));
        let value = Dyn::parse_datetime("2024-01-01T00:00:00Z").unwrap();
        assert_eq!(hash(&value), hash(&value.clone()));
        assert_ne!(Dyn::Int(1.into()), Dyn::Float(1.0));
        assert_ne!(Dyn::Object(vec![("b".into(), Dyn::Null), ("a".into(), Dyn::Null)]),
            Dyn::Object(vec![("a".into(), Dyn::Null), ("b".into(), Dyn::Null)]));
    }
}
