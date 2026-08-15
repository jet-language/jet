// Dependency-free, lossless JSON-number projection shared by every execution
// tier. The JSON tokenizer validates token grammar once. These helpers preserve
// the original token text, enforce common resource limits, and project it into
// the existing exact `Int` and `Decimal` carriers without a binary-float round
// trip. This file is also `include!`-embedded inside Prelude modules.

pub const JSON_NUMBER_MAX_DIGITS: usize = 1_000_000;
pub const JSON_NUMBER_MAX_EXPONENT: i64 = 1_000_000;

pub fn validate_json_number(text: &str) -> Result<(), String> {
    let digits = text
        .bytes()
        .filter(|byte| byte.is_ascii_digit())
        .count();
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
        let zeros = usize::try_from(-scale)
            .map_err(|_| "decimal scale is out of range".to_string())?;
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
    let scale = u32::try_from(scale.max(0))
        .map_err(|_| "decimal scale is out of range".to_string())?;
    let negative = negative && digits.iter().any(|digit| *digit != 0);
    Ok((negative, digits, scale))
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
        assert_eq!(
            json_decimal_lexeme("1E-5").unwrap(),
            (false, vec![1], 5)
        );
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
