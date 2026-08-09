// D-FMT1: pure formatting value kernel shared by AOT, JIT, and comptime.

pub(crate) fn jet_fmt_number(value: i64) -> String {
    comma_int(value)
}

pub(crate) fn jet_fmt_decimal(value: f64, precision: i64) -> String {
    let precision = precision.clamp(0, 9) as usize;
    comma_decimal(format!("{:.*}", precision, value))
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
