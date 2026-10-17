// Dependency-free, lossless JSON-number projection shared by every execution
// tier. The JSON tokenizer validates token grammar once. These helpers preserve
// the original token text, enforce common resource limits, and project it into
// the existing exact `Int` and `Decimal` carriers without a binary-float round
// trip. This file is also `include!`-embedded inside Prelude modules.

pub const JSON_NUMBER_MAX_DIGITS: usize = 1_000_000;
pub const JSON_NUMBER_MAX_EXPONENT: i64 = 1_000_000;

pub fn validate_json_number(text: &str) -> Result<(), String> {
    let digits = text.bytes().filter(|byte| byte.is_ascii_digit()).count();
    if digits > JSON_NUMBER_MAX_DIGITS {
        return Err(format!(
            "JSON number exceeds the {JSON_NUMBER_MAX_DIGITS}-digit limit"
        ));
    }
    let exponent = text
        .find(|ch| ch == 'e' || ch == 'E')
        .map(|index| text[index + 1..].parse::<i64>())
        .transpose()
        .map_err(|_| "JSON number exponent is out of range".to_string())?
        .unwrap_or(0);
    if exponent.unsigned_abs() > JSON_NUMBER_MAX_EXPONENT as u64 {
        return Err(format!(
            "JSON number exponent exceeds the ±{JSON_NUMBER_MAX_EXPONENT} limit"
        ));
    }
    Ok(())
}

/// Whether `text` is exactly one RFC 8259 number token:
/// `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`. A `Dyn.Number`
/// lexeme always has this spelling, so every renderer emits it verbatim.
pub fn is_json_number_token(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    let digits = |index: &mut usize| {
        let start = *index;
        while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
            *index += 1;
        }
        *index > start
    };
    match bytes.get(index) {
        Some(b'0') => index += 1,
        Some(b'1'..=b'9') => {
            digits(&mut index);
        }
        _ => return false,
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        if !digits(&mut index) {
            return false;
        }
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        if !digits(&mut index) {
            return false;
        }
    }
    index == bytes.len()
}

/// Spell a TOML or YAML numeric scalar as its exact JSON number lexeme
/// (#4602): `_` separators and a leading `+` drop, `0x`/`0o`/`0b` integers
/// become decimal digits, and `.5`/`5.` gain their zero. The value is
/// unchanged; `None` means the scalar is not a finite number (`inf`, `nan`,
/// text).
pub fn number_lexeme(scalar: &str) -> Option<String> {
    let text: String = scalar.trim().chars().filter(|ch| *ch != '_').collect();
    let text = text.strip_prefix('+').unwrap_or(&text);
    let (sign, body) = match text.strip_prefix('-') {
        Some(body) => ("-", body),
        None => ("", text),
    };
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(digits) = body.strip_prefix(prefix) {
            let value = crate::Numeric::CtBigInt::from_radix(digits, radix).ok()?;
            return Some(if value.is_zero() { "0".to_string() } else { format!("{sign}{}", value.to_string_rep()) });
        }
    }
    let mut body = body.to_string();
    if body.starts_with('.') {
        body.insert(0, '0');
    }
    let integer_end = body.find(|ch: char| !ch.is_ascii_digit()).unwrap_or(body.len());
    let significant = body[..integer_end].trim_start_matches('0');
    if integer_end > 0 && significant.len() < integer_end {
        let kept = if significant.is_empty() { "0" } else { significant };
        body = format!("{kept}{}", &body[integer_end..]);
    }
    if let Some(dot) = body.find('.') {
        let after = body.as_bytes().get(dot + 1).copied();
        if !after.is_some_and(|byte| byte.is_ascii_digit()) {
            body.insert(dot + 1, '0');
        }
    }
    let lexeme = format!("{sign}{body}");
    is_json_number_token(&lexeme).then_some(lexeme)
}

/// Return sign, integer mantissa digits, and decimal scale for one exact
/// decimal lexeme. The caller owns whether leading `+` is legal at its surface.
pub fn json_decimal_lexeme(s: &str) -> Result<(bool, Vec<u8>, u32), String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("empty decimal number".to_string());
    }
    let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = t.strip_prefix('+') {
        (false, rest)
    } else {
        (false, t)
    };
    let (mantissa, exponent) = body
        .find(|ch| ch == 'e' || ch == 'E')
        .map(|index| {
            let exponent = body[index + 1..]
                .parse::<i64>()
                .map_err(|_| "decimal exponent is out of range".to_string())?;
            Ok::<(&str, i64), String>((&body[..index], exponent))
        })
        .transpose()?
        .unwrap_or((body, 0));
    let mut parts = mantissa.split('.');
    let int_part = parts.next().unwrap_or("");
    let frac_part = parts.next().unwrap_or("");
    if parts.next().is_some()
        || int_part.is_empty() && frac_part.is_empty()
        || !int_part.bytes().all(|byte| byte.is_ascii_digit())
        || !frac_part.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(format!("invalid decimal number `{s}`"));
    }
    let raw_digits = int_part.len().saturating_add(frac_part.len());
    if raw_digits > JSON_NUMBER_MAX_DIGITS {
        return Err(format!(
            "decimal number exceeds the {JSON_NUMBER_MAX_DIGITS}-digit limit"
        ));
    }
    if exponent.unsigned_abs() > JSON_NUMBER_MAX_EXPONENT as u64 {
        return Err(format!(
            "decimal exponent exceeds the ±{JSON_NUMBER_MAX_EXPONENT} limit"
        ));
    }
    let scale = i64::try_from(frac_part.len())
        .map_err(|_| "decimal scale is out of range".to_string())?
        .checked_sub(exponent)
        .ok_or_else(|| "decimal scale is out of range".to_string())?;
    let mut digits: Vec<u8> = int_part
        .bytes()
        .chain(frac_part.bytes())
        .map(|byte| byte - b'0')
        .collect();
    if digits.is_empty() {
        digits.push(0);
    }
    if scale < 0 {
        let zeros =
            usize::try_from(-scale).map_err(|_| "decimal scale is out of range".to_string())?;
        let total = digits
            .len()
            .checked_add(zeros)
            .ok_or_else(|| "decimal number is too large".to_string())?;
        if total > JSON_NUMBER_MAX_DIGITS {
            return Err(format!(
                "decimal number exceeds the {JSON_NUMBER_MAX_DIGITS}-digit limit"
            ));
        }
        digits.resize(total, 0);
    }
    if let Some(first_nonzero) = digits.iter().position(|digit| *digit != 0) {
        if first_nonzero > 0 {
            digits.drain(..first_nonzero);
        }
    } else {
        digits.truncate(1);
    }
    let scale =
        u32::try_from(scale.max(0)).map_err(|_| "decimal scale is out of range".to_string())?;
    let negative = negative && digits.iter().any(|digit| *digit != 0);
    Ok((negative, digits, scale))
}
/// Project a decimal lexeme directly into the native small Decimal carrier.
/// `None` means the lexeme is valid but its canonical mantissa needs the
/// arbitrary-precision path. Validation and resource-limit errors are kept
/// identical to `json_decimal_lexeme`, while successful small values allocate
/// nothing.
pub fn json_decimal_small(s: &str) -> Result<Option<(bool, i128, u32)>, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("empty decimal number".to_string());
    }
    let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = t.strip_prefix('+') {
        (false, rest)
    } else {
        (false, t)
    };
    let (mantissa, exponent) = body
        .find(|ch| ch == 'e' || ch == 'E')
        .map(|index| {
            let exponent = body[index + 1..]
                .parse::<i64>()
                .map_err(|_| "decimal exponent is out of range".to_string())?;
            Ok::<(&str, i64), String>((&body[..index], exponent))
        })
        .transpose()?
        .unwrap_or((body, 0));
    let mut parts = mantissa.split('.');
    let int_part = parts.next().unwrap_or("");
    let frac_part = parts.next().unwrap_or("");
    if parts.next().is_some() || int_part.is_empty() && frac_part.is_empty() {
        return Err(format!("invalid decimal number `{s}`"));
    }

    let mut valid = true;
    let mut value = 0i128;
    let mut significant = 0usize;
    let mut nonzero = false;
    let mut fits = true;
    for digit in int_part.bytes().chain(frac_part.bytes()) {
        if !digit.is_ascii_digit() {
            valid = false;
            continue;
        }
        if !nonzero {
            if digit == b'0' {
                continue;
            }
            nonzero = true;
        }
        significant += 1;
        if fits {
            let Some(next) = value
                .checked_mul(10)
                .and_then(|value| value.checked_add(i128::from(digit - b'0')))
            else {
                fits = false;
                continue;
            };
            value = next;
        }
    }
    if !valid {
        return Err(format!("invalid decimal number `{s}`"));
    }

    let raw_digits = int_part.len().saturating_add(frac_part.len());
    if raw_digits > JSON_NUMBER_MAX_DIGITS {
        return Err(format!(
            "decimal number exceeds the {JSON_NUMBER_MAX_DIGITS}-digit limit"
        ));
    }
    if exponent.unsigned_abs() > JSON_NUMBER_MAX_EXPONENT as u64 {
        return Err(format!(
            "decimal exponent exceeds the ±{JSON_NUMBER_MAX_EXPONENT} limit"
        ));
    }
    let mut scale = i64::try_from(frac_part.len())
        .map_err(|_| "decimal scale is out of range".to_string())?
        .checked_sub(exponent)
        .ok_or_else(|| "decimal scale is out of range".to_string())?;
    if scale < 0 {
        let zeros =
            usize::try_from(-scale).map_err(|_| "decimal scale is out of range".to_string())?;
        let total = raw_digits
            .checked_add(zeros)
            .ok_or_else(|| "decimal number is too large".to_string())?;
        if total > JSON_NUMBER_MAX_DIGITS {
            return Err(format!(
                "decimal number exceeds the {JSON_NUMBER_MAX_DIGITS}-digit limit"
            ));
        }
    }
    if !nonzero {
        let scale =
            u32::try_from(scale.max(0)).map_err(|_| "decimal scale is out of range".to_string())?;
        return Ok(Some((false, 0, scale)));
    }
    if !fits {
        return Ok(None);
    }
    if scale < 0 {
        let zeros =
            usize::try_from(-scale).map_err(|_| "decimal scale is out of range".to_string())?;
        if significant
            .checked_add(zeros)
            .map_or(true, |digits| digits > 39)
        {
            return Ok(None);
        }
        for _ in 0..zeros {
            let Some(next) = value.checked_mul(10) else {
                return Ok(None);
            };
            value = next;
        }
        scale = 0;
    }
    let scale =
        u32::try_from(scale).map_err(|_| "decimal scale is out of range".to_string())?;
    Ok(Some((negative && value != 0, value, scale)))
}

/// Return signed integer text for a mathematically integral JSON number.
/// Default `Int` and fixed-width destinations share this projection; only the
/// destination range check differs.
pub fn json_exact_integer_text(s: &str) -> Result<String, String> {
    let (negative, digits, scale) = json_decimal_lexeme(s)?;
    let fractional = usize::try_from(scale).unwrap_or(usize::MAX);
    if fractional > 0
        && digits
            .iter()
            .rev()
            .take(fractional)
            .any(|digit| *digit != 0)
    {
        return Err(format!("JSON number `{s}` is not an exact integer"));
    }
    let keep = digits.len().saturating_sub(fractional);
    let mut integer = String::with_capacity(keep.max(1) + usize::from(negative));
    if negative {
        integer.push('-');
    }
    if keep == 0 {
        integer.push('0');
    } else {
        integer.extend(digits[..keep].iter().map(|digit| (b'0' + *digit) as char));
    }
    Ok(integer)
}

#[cfg(test)]
mod tests {
    use super::{json_decimal_lexeme, json_exact_integer_text};

    #[test]
    fn exact_projection_keeps_scale_exponents_and_adjacent_integers() {
        assert_eq!(
            json_decimal_lexeme("12.340").unwrap(),
            (false, vec![1, 2, 3, 4, 0], 3)
        );
        assert_eq!(json_decimal_lexeme("1E-5").unwrap(), (false, vec![1], 5));
        assert_eq!(
            json_exact_integer_text("9007199254740993").unwrap(),
            "9007199254740993"
        );
        assert_eq!(
            json_exact_integer_text("1e30").unwrap(),
            "1000000000000000000000000000000"
        );
        assert!(json_exact_integer_text("1.5").is_err());
        assert!(json_decimal_lexeme("NaN").is_err());
        assert!(json_decimal_lexeme("1e1000001").is_err());
    }
}
