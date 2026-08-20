// D-FMT1: pure formatting value kernel shared by AOT, JIT, and comptime.

pub(crate) fn jet_fmt_number(value: i64) -> String {
    comma_int(value)
}

pub(crate) fn jet_fmt_decimal(value: f64, precision: i64) -> String {
    let precision = precision.clamp(0, 9) as usize;
    comma_decimal(format!("{:.*}", precision, value))
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
    format!("{}%", jet_fmt_decimal(value * 100.0, precision))
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
    let raw = value.abs().to_string();
    let mut out = String::new();
    for (index, ch) in raw.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    let mut text: String = out.chars().rev().collect();
    if value < 0 {
        text.insert(0, '-');
    }
    text
}

fn comma_decimal(raw: String) -> String {
    let (sign, rest) = raw.strip_prefix('-').map_or(("", raw.as_str()), |s| ("-", s));
    let mut split = rest.splitn(2, '.');
    let whole = split.next().unwrap_or("0");
    let frac = split.next();
    let whole_value = whole.parse::<i64>().unwrap_or(0);
    let whole_text = comma_int(whole_value);
    match frac {
        Some(frac) => format!("{}{}.{}", sign, whole_text, frac),
        None => format!("{}{}", sign, whole_text),
    }
}
