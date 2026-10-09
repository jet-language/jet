// D-FMT1: pure formatting value kernel shared by AOT, JIT, and comptime.

pub(crate) fn jet_fmt_number(value: i64) -> String {
    comma_int(value)
}

/// Rulings #4588: fixed decimals round half away from zero and never print a
/// negative zero (`decimal(0.125, 2)` is `0.13`, `decimal(-0.0001, 2)` is
/// `0.00`).
pub(crate) fn jet_fmt_decimal(value: f64, precision: i64) -> String {
    fixed_half_away(value, precision)
}

/// D-QUANTITY-PRINT1: quantity display trims only the incidental `.0` from
/// ordinary Float spelling; fractional magnitudes retain their digits.
pub(crate) fn jet_fmt_quantity(value: f64) -> String {
    let mut text = format!("{:?}", value);
    if text.ends_with(".0") {
        text.truncate(text.len() - 2);
    }
    text
}

/// D-FMT-PLAIN1=A: explicit human grouping keeps the former decimal display.
pub(crate) fn jet_fmt_grouped(value: f64, precision: i64) -> String {
    comma_decimal(fixed_half_away(value, precision))
}

/// One rounding rule for every fixed-precision Float display, shared with
/// `float_fixed` in Core/text/fmt.jet: round `value * 10^precision` (the scale
/// built by repeated `* 10.0`) half away from zero. A rounded magnitude past
/// the exact `Int` range falls back to the value's exact binary expansion,
/// rounded half away from zero at the same place. Non-finite values keep their
/// ordinary Float display.
fn fixed_half_away(value: f64, precision: i64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let precision = precision.max(0) as usize;
    let mut scale = 1.0_f64;
    for _ in 0..precision {
        scale *= 10.0;
        if scale.is_infinite() {
            break;
        }
    }
    let rounded = (value * scale).round().abs();
    let mut text = if rounded < 9_223_372_036_854_775_808.0 {
        let mut digits = (rounded as u64).to_string();
        if precision > 0 {
            if digits.len() <= precision {
                digits.insert_str(0, &"0".repeat(precision + 1 - digits.len()));
            }
            digits.insert(digits.len() - precision, '.');
        }
        digits
    } else {
        exact_fixed_half_away(value.abs(), precision)
    };
    if value < 0.0 && text.bytes().any(|digit| matches!(digit, b'1'..=b'9')) {
        text.insert(0, '-');
    }
    text
}

/// Rounds the exact decimal expansion of finite, non-negative `magnitude` half
/// away from zero to `precision` fractional digits.
fn exact_fixed_half_away(magnitude: f64, precision: usize) -> String {
    let bits = magnitude.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i64;
    let fraction = bits & ((1_u64 << 52) - 1);
    let (significand, power) = if exponent == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1_u64 << 52), exponent - 1075)
    };
    // A binary fraction with k fractional bits has exactly k fractional
    // decimal digits, so formatting at k places is exact.
    let fraction_digits = if significand == 0 {
        0
    } else {
        (-(power + i64::from(significand.trailing_zeros()))).max(0) as usize
    };
    if fraction_digits <= precision {
        return format!("{:.*}", precision, magnitude);
    }
    let mut digits = format!("{:.*}", fraction_digits, magnitude).into_bytes();
    let dot = digits.len() - fraction_digits - 1;
    let round_up = digits[dot + 1 + precision] >= b'5';
    digits.truncate(if precision == 0 { dot } else { dot + 1 + precision });
    if round_up {
        let mut index = digits.len();
        loop {
            if index == 0 {
                digits.insert(0, b'1');
                break;
            }
            index -= 1;
            match digits[index] {
                b'.' => {}
                b'9' => digits[index] = b'0',
                _ => {
                    digits[index] += 1;
                    break;
                }
            }
        }
    }
    String::from_utf8(digits).expect("fixed decimal digits are ASCII")
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
    let grouped_digits = if grouped {
        Some(group_decimal_digits(digits))
    } else {
        None
    };
    let whole = grouped_digits.as_deref().unwrap_or(digits);
    let mut out = String::with_capacity(
        sign.len() + whole.len() + usize::from(precision > 0) + precision,
    );
    out.push_str(sign);
    out.push_str(whole);
    if precision > 0 {
        out.push('.');
        for _ in 0..precision {
            out.push('0');
        }
    }
    out
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
    let mut decimal = Vec::with_capacity(digits.len());
    for digit in digits.bytes() {
        let digit = digit.wrapping_sub(b'0');
        if digit > 9 {
            return value.to_string();
        }
        if digit != 0 || !decimal.is_empty() {
            decimal.push(digit);
        }
    }
    if decimal.is_empty() {
        if digits.is_empty() {
            return value.to_string();
        }
        decimal.push(0);
    }

    let mut converted = Vec::with_capacity(decimal.len());
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
    let padding = (width.max(0) as usize).saturating_sub(converted.len());
    let zero = converted.len() == 1 && converted[0] == '0';
    let signed = negative && !zero;
    let mut out = String::with_capacity(usize::from(signed) + padding + converted.len());
    if signed {
        out.push('-');
    }
    for _ in 0..padding {
        out.push('0');
    }
    out.extend(converted);
    out
}

/// Rulings #4588: normalized scientific notation with a plain exponent
/// (`1.23e3`, `1.23e-3`). The mantissa uses the fixed-decimal rounding rule, and
/// a mantissa that rounds up to ten renormalizes (`sci(9.999, 2)` is `1.00e1`).
/// The same steps as `sci` in Core/text/fmt.jet, so every tier agrees.
pub(crate) fn jet_fmt_sci(value: f64, precision: i64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    if value == 0.0 {
        return format!("{}e0", fixed_half_away(0.0, precision));
    }
    let mut magnitude = value.abs();
    let mut exponent = 0_i64;
    while magnitude >= 10.0 {
        magnitude /= 10.0;
        exponent += 1;
    }
    while magnitude < 1.0 {
        magnitude *= 10.0;
        exponent -= 1;
    }
    let mut mantissa = fixed_half_away(magnitude, precision);
    if mantissa.starts_with("10") {
        exponent += 1;
        mantissa = fixed_half_away(magnitude / 10.0, precision);
    }
    let sign = if value < 0.0 { "-" } else { "" };
    format!("{sign}{mantissa}e{exponent}")
}

/// D-FMT-PRETTY1=A: expand canonical Debug text without inspecting terminal
/// width. Strings are skipped while scanning, so braces in a string value do
/// not become structure. The canonical Debug renderer already owns field
/// order, redaction, and collection ordering; this kernel only lays it out:
/// one item per line with commas between items (rulings #4588).
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
    for (index, item) in split_top_level(body).into_iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
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

/// Rulings #4588: SI units (1000), one decimal below 10 using the
/// fixed-decimal rounding rule (`1536` is `1.5 KB`, `1000` is `1 KB`), and
/// whole units rounded half away from zero at or above 10.
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
        let shown = fixed_half_away(size, 1);
        format!("{}{} {}", sign, shown.strip_suffix(".0").unwrap_or(&shown), units[unit])
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

pub(crate) fn jet_fmt_plural(count: i64, singular: &str, plural: &str) -> String {
    let word = if count.abs() == 1 { singular } else { plural };
    format!("{} {}", comma_int(count), word)
}

pub(crate) fn jet_fmt_pad(text: &str, width: i64, fill: &str) -> String {
    jet_fmt_pad_right(text, width, fill)
}

pub(crate) fn jet_fmt_pad_left(text: &str, width: i64, fill: &str) -> String {
    let need = pad_need(text, width);
    format!("{}{}", pad_fill(fill, need), text)
}

pub(crate) fn jet_fmt_pad_right(text: &str, width: i64, fill: &str) -> String {
    let need = pad_need(text, width);
    format!("{}{}", text, pad_fill(fill, need))
}

pub(crate) fn jet_fmt_pad_center(text: &str, width: i64, fill: &str) -> String {
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
    let count = digits.chars().count();
    let mut out = String::with_capacity(
        sign.len() + digits.len() + count.saturating_sub(1) / 3,
    );
    out.push_str(sign);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (count - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
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
