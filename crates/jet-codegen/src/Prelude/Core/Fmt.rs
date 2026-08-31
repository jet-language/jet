// D-FMT1: pure formatting value kernel shared by AOT, JIT, and comptime.

pub(crate) fn jet_fmt_number(value: i64) -> String {
    comma_int(value)
}

pub(crate) fn jet_fmt_decimal(value: f64, precision: i64) -> String {
    let precision = precision.max(0) as usize;
    format!("{:.*}", precision, value)
}

/// D-FMT-PLAIN1=A: explicit human grouping keeps the former decimal display.
pub(crate) fn jet_fmt_grouped(value: f64, precision: i64) -> String {
    let precision = precision.max(0) as usize;
    comma_decimal(format!("{:.*}", precision, value))
}

/// D-FMT-INTERP3=B: format an exact integer with fixed decimal places. The
/// caller supplies the carrier's decimal spelling so native and spilled `Int`
/// values use one kernel without a lossy float conversion.
pub(crate) fn jet_fmt_decimal_int(value: &str, precision: i64) -> String {
    format_decimal_int(value, precision, false)
}

/// D-FMT-PLAIN1=A: the grouped integer rail uses the same exact-int kernel as
/// `jet_fmt_decimal_int`; only the explicit grouping policy differs.
pub(crate) fn jet_fmt_grouped_int(value: &str, precision: i64) -> String {
    format_decimal_int(value, precision, true)
}

fn format_decimal_int(value: &str, precision: i64, grouped: bool) -> String {
    let precision = precision.max(0) as usize;
    let value = value.trim();
    let (negative, digits) = if let Some(rest) = value.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = value.strip_prefix('+') {
        (false, rest)
    } else {
        (false, value)
    };
    if digits.is_empty() || !digits.bytes().all(|digit| digit.is_ascii_digit()) {
        return value.to_string();
    }
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let sign = if negative && digits != "0" { "-" } else { "" };
    let whole = if grouped {
        group_decimal_digits(digits)
    } else {
        digits.to_string()
    };
    if precision == 0 {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{}", "0".repeat(precision))
    }
}

/// D-FMT-INTERP3=B: convert an exact decimal integer spelling to lowercase
/// hexadecimal, then zero-pad its digits to the requested width. The caller
/// supplies the carrier's decimal spelling so the same kernel handles native
/// small and spilled `Int` values.
pub(crate) fn jet_fmt_hex_decimal(value: &str, width: i64) -> String {
    jet_fmt_radix_decimal(value, 16, width)
}

pub(crate) fn jet_fmt_bin_decimal(value: &str) -> String {
    jet_fmt_radix_decimal(value, 2, 0)
}

pub(crate) fn jet_fmt_oct_decimal(value: &str) -> String {
    jet_fmt_radix_decimal(value, 8, 0)
}

fn jet_fmt_radix_decimal(value: &str, radix: u16, width: i64) -> String {
    let value = value.trim();
    let (negative, digits) = if let Some(rest) = value.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = value.strip_prefix('+') {
        (false, rest)
    } else {
        (false, value)
    };
    let mut decimal = digits
        .bytes()
        .map(|digit| digit.wrapping_sub(b'0'))
        .collect::<Vec<_>>();
    if decimal.is_empty() || decimal.iter().any(|digit| *digit > 9) {
        return value.to_string();
    }
    while decimal.len() > 1 && decimal[0] == 0 {
        decimal.remove(0);
    }

    let mut converted = Vec::new();
    while decimal.iter().any(|digit| *digit != 0) {
        let mut carry = 0u16;
        let mut quotient = Vec::with_capacity(decimal.len());
        for digit in decimal {
            let current = carry * 10 + u16::from(digit);
            let next = current / radix;
            carry = current % radix;
            if !quotient.is_empty() || next != 0 {
                quotient.push(next as u8);
            }
        }
        converted.push(b"0123456789abcdef"[carry as usize] as char);
        decimal = quotient;
    }
    if converted.is_empty() {
        converted.push('0');
    } else {
        converted.reverse();
    }
    let converted = converted.into_iter().collect::<String>();
    let padding = "0".repeat((width.max(0) as usize).saturating_sub(converted.len()));
    if negative && converted != "0" {
        format!("-{padding}{converted}")
    } else {
        format!("{padding}{converted}")
    }
}

pub(crate) fn jet_fmt_sci(value: f64, precision: i64) -> String {
    let precision = precision.max(0) as usize;
    format!("{:.*e}", precision, value)
}

/// D-FMT-PRETTY1=A: expand canonical Debug text without inspecting terminal
/// width. Strings are skipped while scanning, so braces in a string value do
/// not become structure. The canonical Debug renderer already owns field
/// order, redaction, and collection ordering; this kernel only lays it out.
pub(crate) fn jet_fmt_pretty(value: &str) -> String {
    pretty_fragment(value, 0)
}

fn pretty_fragment(value: &str, indent: usize) -> String {
    let value = value.trim();
    let Some((open_at, open, close)) = first_structure(value) else {
        return value.to_string();
    };
    let Some(end) = matching_close(value, open, close) else {
        return value.to_string();
    };
    if !value[end + close.len_utf8()..].trim().is_empty() {
        return value.to_string();
    }
    let prefix = value[..open_at].trim_end();
    let body = &value[open_at + open.len_utf8()..end];
    if body.trim().is_empty() {
        return format!("{prefix} {open}{close}").trim_start().to_string();
    }
    if open == '['
        && (body.trim() == ":" || body.trim().eq_ignore_ascii_case("redacted"))
    {
        return value.to_string();
    }
    let mut out = String::new();
    if !prefix.is_empty() {
        out.push_str(prefix);
        out.push(' ');
    }
    out.push(open);
    for item in split_top_level(body) {
        out.push('\n');
        out.push_str(&" ".repeat(indent + 2));
        out.push_str(&pretty_fragment(item, indent + 2));
    }
    out.push('\n');
    out.push_str(&" ".repeat(indent));
    out.push(close);
    out
}

/// Returns the byte offset of the first structural opener plus its pair. The
/// offset is what the caller slices with, so it must come from the scan rather
/// than being recomputed from the char.
fn first_structure(value: &str) -> Option<(usize, char, char)> {
    let mut quoted = false;
    let mut escaped = false;
    for (offset, ch) in value.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        if ch == '"' {
            quoted = true;
        } else if let Some(close) = close_for(ch) {
            return Some((offset, ch, close));
        }
    }
    None
}

fn close_for(open: char) -> Option<char> {
    match open {
        '{' => Some('}'),
        '[' => Some(']'),
        '(' => Some(')'),
        _ => None,
    }
}

fn matching_close(value: &str, start: char, expected: char) -> Option<usize> {
    let mut stack = vec![expected];
    let mut quoted = false;
    let mut escaped = false;
    let mut seen_start = false;
    for (index, ch) in value.char_indices() {
        if !seen_start {
            if ch == start {
                seen_start = true;
            }
            continue;
        }
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        if ch == '"' {
            quoted = true;
            continue;
        }
        if let Some(close) = close_for(ch) {
            stack.push(close);
        } else if stack.last().copied() == Some(ch) {
            stack.pop();
            if stack.is_empty() {
                return Some(index);
            }
        }
    }
    None
}

fn split_top_level(value: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0;
    let mut stack = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        if ch == '"' {
            quoted = true;
        } else if let Some(close) = close_for(ch) {
            stack.push(close);
        } else if stack.last().copied() == Some(ch) {
            stack.pop();
        } else if ch == ',' && stack.is_empty() {
            if !value[start..index].trim().is_empty() {
                items.push(value[start..index].trim());
            }
            start = index + ch.len_utf8();
        }
    }
    if !value[start..].trim().is_empty() {
        items.push(value[start..].trim());
    }
    items
}

pub(crate) fn jet_fmt_percent(value: f64, precision: i64) -> String {
    format!("{}%", jet_fmt_grouped(value * 100.0, precision))
}

pub(crate) fn jet_fmt_bytes(value: i64) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let mut size = (value as f64).abs();
    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let mut unit = 0usize;
    while size >= 1000.0 && unit + 1 < units.len() {
        size /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}{} {}", sign, size as i64, units[unit])
    } else if size >= 10.0 {
        format!("{}{} {}", sign, size.round() as i64, units[unit])
    } else {
        let shown = format!("{:.1}", size);
        format!("{}{} {}", sign, shown.trim_end_matches(".0"), units[unit])
    }
}

pub(crate) fn jet_fmt_duration(ms: i64) -> String {
    let sign = if ms < 0 { "-" } else { "" };
    let mut rest = ms.abs();
    if rest < 1000 {
        return format!("{}{}ms", sign, rest);
    }
    let days = rest / 86_400_000;
    rest %= 86_400_000;
    let hours = rest / 3_600_000;
    rest %= 3_600_000;
    let minutes = rest / 60_000;
    rest %= 60_000;
    let seconds = rest / 1000;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{}s", seconds));
    }
    format!("{}{}", sign, parts.into_iter().take(3).collect::<Vec<_>>().join(" "))
}

pub(crate) fn jet_fmt_ordinal(value: i64) -> String {
    let n = value.abs();
    let suffix = if (11..=13).contains(&(n % 100)) {
        "th"
    } else {
        match n % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    format!("{}{}", comma_int(value), suffix)
}

pub(crate) fn jet_fmt_plural(count: i64, singular: &String, plural: &String) -> String {
    let word = if count.abs() == 1 { singular } else { plural };
    format!("{} {}", comma_int(count), word)
}

pub(crate) fn jet_fmt_pad(text: &String, width: i64, fill: &String) -> String {
    jet_fmt_pad_right(text, width, fill)
}

pub(crate) fn jet_fmt_pad_left(text: &String, width: i64, fill: &String) -> String {
    let need = pad_need(text, width);
    format!("{}{}", pad_fill(fill, need), text)
}

pub(crate) fn jet_fmt_pad_right(text: &String, width: i64, fill: &String) -> String {
    let need = pad_need(text, width);
    format!("{}{}", text, pad_fill(fill, need))
}

pub(crate) fn jet_fmt_pad_center(text: &String, width: i64, fill: &String) -> String {
    let need = pad_need(text, width);
    let left = need / 2;
    let right = need - left;
    format!("{}{}{}", pad_fill(fill, left), text, pad_fill(fill, right))
}

fn pad_need(text: &str, width: i64) -> usize {
    (width.max(0) as usize).saturating_sub(text.chars().count())
}

fn pad_fill(fill: &str, len: usize) -> String {
    if len == 0 {
        return String::new();
    }
    let fill = if fill.is_empty() { " " } else { fill };
    let mut out = String::new();
    while out.chars().count() < len {
        out.push_str(fill);
    }
    out.chars().take(len).collect()
}

fn comma_int(value: i64) -> String {
    group_decimal_digits(&value.to_string())
}

fn group_decimal_digits(raw: &str) -> String {
    let (sign, digits) = if let Some(rest) = raw.strip_prefix('-') {
        ("-", rest)
    } else if let Some(rest) = raw.strip_prefix('+') {
        ("+", rest)
    } else {
        ("", raw)
    };
    let mut out = String::new();
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{sign}{}", out.chars().rev().collect::<String>())
}

fn comma_decimal(raw: String) -> String {
    let (sign, rest) = raw.strip_prefix('-').map_or(("", raw.as_str()), |s| ("-", s));
    let mut split = rest.splitn(2, '.');
    let whole = split.next().unwrap_or("0");
    let frac = split.next();
    // Keep the formatted value as decimal text. Parsing the whole part as an
    // i64 turns values above i64::MAX into zero, which made 1e21 render as
    // 0.00 and also discarded large exact integer spellings.
    let whole_text = group_decimal_digits(whole);
    match frac {
        Some(frac) => format!("{}{}.{}", sign, whole_text, frac),
        None => format!("{}{}", sign, whole_text),
    }
}
