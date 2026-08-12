// D-AUTH2=A / D-AUTH-TOKENPOLICY1=A: strict standalone token verification.
// JSON goes through Jet's one RFC 8259 parser; Ed25519 goes through the vetted
// crypto bridge. No token-controlled algorithm or purpose is ever dispatched.

#[derive(Debug, Clone)]
pub struct JetAuthClaims {
    pub subject: Option<String>,
    pub audience: String,
    pub issuer: Option<String>,
    pub expires_at: i64,
    pub not_before: Option<i64>,
    pub issued_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub enum JetAuthError {
    MalformedToken(String),
    UnsupportedToken(String),
    InvalidSignature,
    WeakKey,
    MissingClaim(String),
    WrongAudience { expected: String, actual: String },
    WrongIssuer { expected: String, actual: Option<String> },
    TokenExpired,
    DecodeError(String),
    TokenNotYetValid,
}

// `parse_json_strict` returns the internal JSON tree, not DataTree. Keep the
// verifier's claim boundary on that one representation in every emitted tier.
type JetAuthJSON = jet_std::JSON;
type JetAuthFields = std::collections::BTreeMap<String, JetAuthJSON>;

fn jet_auth_b64_decode_inner(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len().saturating_mul(3) / 4);
    let mut buf = 0u32;
    let mut bits = 0u8;
    let mut padding = false;
    for b in s.bytes() {
        if b == b'=' {
            padding = true;
            continue;
        }
        if padding {
            return Err("base64 padding must be final".to_string());
        }
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err("invalid base64 character".to_string()),
        };
        // At most seven residual bits survive each byte emission. Keeping a
        // bounded accumulator prevents long attacker-controlled input from
        // overflowing before malformed input is rejected.
        buf = ((buf << 6) | v as u32) & 0x00ff_ffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    if bits != 0 && buf & ((1u32 << bits) - 1) != 0 {
        return Err("non-canonical base64 trailing bits".to_string());
    }
    Ok(out)
}

fn jet_auth_b64url_decode(text: &str) -> Result<Vec<u8>, String> {
    if text.bytes().any(|b| {
        !matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_')
    }) {
        return Err("invalid base64url character".to_string());
    }
    let mut s = text.replace('-', "+").replace('_', "/");
    if s.len() % 4 == 1 {
        return Err("invalid base64url length".to_string());
    }
    while s.len() % 4 != 0 {
        s.push('=');
    }
    jet_auth_b64_decode_inner(&s)
}

fn jet_hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&jet_sha256_raw(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(BLOCK + data.len());
    let mut outer = Vec::with_capacity(BLOCK + 32);
    for byte in k { inner.push(byte ^ 0x36); }
    inner.extend_from_slice(data);
    for byte in k { outer.push(byte ^ 0x5c); }
    outer.extend_from_slice(&jet_sha256_raw(&inner));
    jet_sha256_raw(&outer)
}

fn jet_auth_ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut difference = 0u8;
    difference |= u8::from(a.len() != b.len());
    for index in 0..a.len().max(b.len()) {
        let left = a.get(index).copied().unwrap_or(0);
        let right = b.get(index).copied().unwrap_or(0);
        difference |= left ^ right;
    }
    difference == 0
}

fn jet_auth_object(text: &str) -> Result<JetAuthFields, JetAuthError> {
    match jet_std::parse_json_strict(text) {
        Ok(jet_std::JSON::Object(fields)) => Ok(fields),
        Ok(_) => Err(JetAuthError::MalformedToken("token JSON must be an object".to_string())),
        Err(error) => Err(JetAuthError::DecodeError(error.message)),
    }
}

fn jet_auth_json_hex4(chars: &[char], pos: &mut usize) -> Option<u32> {
    let mut value = 0u32;
    for _ in 0..4 {
        let digit = chars.get(*pos)?.to_digit(16)?;
        *pos += 1;
        value = value * 16 + digit;
    }
    Some(value)
}

fn jet_auth_json_string(chars: &[char], pos: &mut usize) -> Option<String> {
    if chars.get(*pos) != Some(&'"') {
        return None;
    }
    *pos += 1;
    let mut out = String::new();
    while let Some(character) = chars.get(*pos).copied() {
        *pos += 1;
        match character {
            '"' => return Some(out),
            '\\' => {
                let escaped = chars.get(*pos).copied()?;
                *pos += 1;
                match escaped {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\u{0008}'),
                    'f' => out.push('\u{000c}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'u' => {
                        let high = jet_auth_json_hex4(chars, pos)?;
                        if (0xD800..=0xDBFF).contains(&high) {
                            if chars.get(*pos) != Some(&'\\') {
                                return None;
                            }
                            *pos += 1;
                            if chars.get(*pos) != Some(&'u') {
                                return None;
                            }
                            *pos += 1;
                            let low = jet_auth_json_hex4(chars, pos)?;
                            if !(0xDC00..=0xDFFF).contains(&low) {
                                return None;
                            }
                            let combined = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                            out.push(char::from_u32(combined)?);
                        } else if (0xDC00..=0xDFFF).contains(&high) {
                            return None;
                        } else {
                            out.push(char::from_u32(high)?);
                        }
                    }
                    _ => return None,
                }
            }
            character if (character as u32) < 0x20 => return None,
            _ => out.push(character),
        }
    }
    None
}

fn jet_auth_json_skip_value(chars: &[char], pos: &mut usize) {
    let Some(first) = chars.get(*pos).copied() else {
        return;
    };
    if first == '"' {
        let _ = jet_auth_json_string(chars, pos);
        return;
    }
    if first == '{' || first == '[' {
        let mut closers = vec![if first == '{' { '}' } else { ']' }];
        *pos += 1;
        while let Some(character) = chars.get(*pos).copied() {
            if character == '"' {
                let _ = jet_auth_json_string(chars, pos);
                continue;
            }
            *pos += 1;
            match character {
                '{' => closers.push('}'),
                '[' => closers.push(']'),
                '}' | ']' if closers.last() == Some(&character) => {
                    closers.pop();
                    if closers.is_empty() {
                        return;
                    }
                }
                _ => {}
            }
        }
        return;
    }
    while chars
        .get(*pos)
        .is_some_and(|character| {
            !jet_std::is_json_structural_whitespace(*character)
                && *character != ','
                && *character != '}'
                && *character != ']'
        })
    {
        *pos += 1;
    }
}

// `jet_std::JSON::Number` is deliberately still the shared f64 tree. Auth
// claims need one extra lexical pass so an i64 NumericDate never crosses a
// rounding boundary before the verifier checks it.
fn jet_auth_number_lexeme(text: &str, name: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut pos = 0usize;
    while chars
        .get(pos)
        .is_some_and(|character| jet_std::is_json_structural_whitespace(*character))
    {
        pos += 1;
    }
    if chars.get(pos) != Some(&'{') {
        return None;
    }
    pos += 1;
    loop {
        while chars
            .get(pos)
            .is_some_and(|character| jet_std::is_json_structural_whitespace(*character))
        {
            pos += 1;
        }
        if chars.get(pos) == Some(&'}') {
            return None;
        }
        let key = jet_auth_json_string(&chars, &mut pos)?;
        while chars
            .get(pos)
            .is_some_and(|character| jet_std::is_json_structural_whitespace(*character))
        {
            pos += 1;
        }
        if chars.get(pos) != Some(&':') {
            return None;
        }
        pos += 1;
        while chars
            .get(pos)
            .is_some_and(|character| jet_std::is_json_structural_whitespace(*character))
        {
            pos += 1;
        }
        let value_start = pos;
        let is_number = chars
            .get(pos)
            .is_some_and(|character| *character == '-' || character.is_ascii_digit());
        if is_number {
            while chars.get(pos).is_some_and(|character| {
                !jet_std::is_json_structural_whitespace(*character)
                    && *character != ','
                    && *character != '}'
            }) {
                pos += 1;
            }
            if key == name {
                return Some(chars[value_start..pos].iter().copied().collect());
            }
        } else {
            jet_auth_json_skip_value(&chars, &mut pos);
        }
        while chars
            .get(pos)
            .is_some_and(|character| jet_std::is_json_structural_whitespace(*character))
        {
            pos += 1;
        }
        match chars.get(pos).copied() {
            Some(',') => pos += 1,
            Some('}') | None => return None,
            _ => return None,
        }
    }
}

// NumericDate is an exact Jet Int, while the shared JSON tree stores numbers
// as f64. Parse the recovered integer lexeme with checked decimal arithmetic;
// never narrow the rounded JSON number back to i64.
fn jet_auth_parse_i64_decimal(lexeme: &str) -> Option<i64> {
    let (negative, digits) = lexeme
        .strip_prefix('-')
        .map_or((false, lexeme), |digits| (true, digits));
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // `-0` is a JSON number, but not the canonical integer spelling.
    if negative && digits == "0" {
        return None;
    }
    let limit = if negative { 1u64 << 63 } else { i64::MAX as u64 };
    let magnitude = digits.bytes().try_fold(0u64, |value, byte| {
        let value = value
            .checked_mul(10)?
            .checked_add(u64::from(byte - b'0'))?;
        (value <= limit).then_some(value)
    })?;
    if negative {
        if magnitude == 1u64 << 63 {
            Some(i64::MIN)
        } else {
            i64::try_from(magnitude).ok()?.checked_neg()
        }
    } else {
        i64::try_from(magnitude).ok()
    }
}

fn jet_auth_optional_text(
    fields: &JetAuthFields,
    name: &str,
) -> Result<Option<String>, JetAuthError> {
    match fields.get(name) {
        None => Ok(None),
        Some(jet_std::JSON::Text(value)) => Ok(Some(value.clone())),
        Some(_) => Err(JetAuthError::MalformedToken(format!("claim `{name}` must be text"))),
    }
}

fn jet_auth_i64_claim(
    fields: &JetAuthFields,
    text: &str,
    name: &str,
) -> Result<Option<i64>, JetAuthError> {
    match fields.get(name) {
        None => Ok(None),
        Some(jet_std::JSON::Integer(_)) => {
            let lexeme = jet_auth_number_lexeme(text, name).ok_or_else(|| {
                JetAuthError::MalformedToken(format!("claim `{name}` must be an exact integer"))
            })?;
            let value = jet_auth_parse_i64_decimal(&lexeme).ok_or_else(|| {
                JetAuthError::MalformedToken(format!("claim `{name}` must be an exact integer"))
            })?;
            Ok(Some(value))
        }
        // `Number` is only the JSON type check. Range and integrality come
        // from the source lexeme, never from the rounded f64 payload.
        Some(jet_std::JSON::Number(_)) => {
            let lexeme = jet_auth_number_lexeme(text, name).ok_or_else(|| {
                JetAuthError::MalformedToken(format!("claim `{name}` must be an exact integer"))
            })?;
            let value = jet_auth_parse_i64_decimal(&lexeme).ok_or_else(|| {
                JetAuthError::MalformedToken(format!("claim `{name}` must be an exact integer"))
            })?;
            Ok(Some(value))
        }
        Some(_) => Err(JetAuthError::MalformedToken(format!("claim `{name}` must be an integer"))),
    }
}

fn jet_auth_required_i64(
    fields: &JetAuthFields,
    text: &str,
    name: &str,
) -> Result<i64, JetAuthError> {
    jet_auth_i64_claim(fields, text, name)?.ok_or_else(|| JetAuthError::MissingClaim(name.to_string()))
}

fn jet_auth_optional_i64(
    fields: &JetAuthFields,
    text: &str,
    name: &str,
) -> Result<Option<i64>, JetAuthError> {
    jet_auth_i64_claim(fields, text, name)
}

fn jet_auth_audience_values(value: &JetAuthJSON) -> Result<Vec<String>, JetAuthError> {
    match value {
        jet_std::JSON::Text(value) => Ok(vec![value.clone()]),
        jet_std::JSON::Array(values) if values.is_empty() => {
            return Err(JetAuthError::MalformedToken(
                "claim `aud` must contain at least one text".to_string(),
            ));
        }
        jet_std::JSON::Array(values) => values
            .iter()
            .map(|value| match value {
                jet_std::JSON::Text(value) => Ok(value.clone()),
                _ => Err(JetAuthError::MalformedToken("claim `aud` must contain only text".to_string())),
            })
            .collect(),
        _ => Err(JetAuthError::MalformedToken("claim `aud` must be text or a text list".to_string())),
    }
}

fn jet_auth_audience(fields: &JetAuthFields, expected: &str) -> Result<String, JetAuthError> {
    let value = fields
        .get("aud")
        .ok_or_else(|| JetAuthError::MissingClaim("aud".to_string()))?;
    let values = jet_auth_audience_values(value)?;
    if values.iter().any(|value| value == expected) {
        Ok(expected.to_string())
    } else {
        Err(JetAuthError::WrongAudience {
            expected: expected.to_string(),
            actual: values.join(","),
        })
    }
}

const JET_AUTH_NANOS_PER_SECOND: i64 = 1_000_000_000;

fn jet_auth_clock_skew_ns(clock_skew_ns: Option<i64>) -> i64 {
    clock_skew_ns.unwrap_or(0)
}

fn jet_auth_timestamp_ns(
    timestamp: i64,
    name: &str,
) -> Result<i128, JetAuthError> {
    i128::from(timestamp)
        .checked_mul(i128::from(JET_AUTH_NANOS_PER_SECOND))
        .ok_or_else(|| {
            JetAuthError::MalformedToken(format!(
                "claim `{name}` is outside the supported nanosecond range"
            ))
        })
}

fn jet_auth_expiry_deadline_ns(
    expires_at: i64,
    clock_skew_ns: i64,
) -> Result<i128, JetAuthError> {
    let expires_at_ns = jet_auth_timestamp_ns(expires_at, "exp")?;
    // Keep signed sub-millisecond skew exact in the i128 comparison.
    expires_at_ns
        .checked_add(i128::from(clock_skew_ns))
        .ok_or_else(|| JetAuthError::MalformedToken("clock_skew overflows the token expiry".to_string()))
}

fn jet_auth_not_before_threshold_ns(
    not_before: i64,
    clock_skew_ns: i64,
) -> Result<i128, JetAuthError> {
    jet_auth_timestamp_ns(not_before, "nbf")?
        .checked_sub(i128::from(clock_skew_ns))
        .ok_or_else(|| JetAuthError::MalformedToken("clock_skew underflows the token not-before".to_string()))
}

fn jet_auth_now_ns() -> i128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i128::try_from(duration.as_nanos()).ok())
        .unwrap_or(i128::MAX)
}

fn jet_auth_claims_at(
    payload: &[u8],
    audience: &str,
    issuer: Option<&str>,
    clock_skew_ns: i64,
    now_ns: i128,
) -> Result<JetAuthClaims, JetAuthError> {
    let text = std::str::from_utf8(payload)
        .map_err(|_| JetAuthError::MalformedToken("claims are not valid UTF-8".to_string()))?;
    let fields = jet_auth_object(text)?;
    let expires_at = jet_auth_required_i64(&fields, text, "exp")?;
    let subject = jet_auth_optional_text(&fields, "sub")?;
    let expected_audience = jet_auth_audience(&fields, audience)?;
    let actual_issuer = jet_auth_optional_text(&fields, "iss")?;
    let not_before = jet_auth_optional_i64(&fields, text, "nbf")?;
    let issued_at = jet_auth_optional_i64(&fields, text, "iat")?;
    if let Some(expected) = issuer {
        if actual_issuer.as_deref() != Some(expected) {
            return Err(JetAuthError::WrongIssuer {
                expected: expected.to_string(), actual: actual_issuer,
            });
        }
    }
    let expiry_deadline_ns = jet_auth_expiry_deadline_ns(expires_at, clock_skew_ns)?;
    if now_ns >= expiry_deadline_ns {
        return Err(JetAuthError::TokenExpired);
    }
    if let Some(not_before) = not_before {
        let valid_from_ns = jet_auth_not_before_threshold_ns(not_before, clock_skew_ns)?;
        if now_ns < valid_from_ns {
            return Err(JetAuthError::TokenNotYetValid);
        }
    }
    Ok(JetAuthClaims {
        subject,
        audience: expected_audience,
        issuer: actual_issuer,
        expires_at,
        not_before,
        issued_at,
    })
}

fn jet_auth_claims(
    payload: &[u8],
    audience: &str,
    issuer: Option<&str>,
    clock_skew_ns: i64,
) -> Result<JetAuthClaims, JetAuthError> {
    jet_auth_claims_at(
        payload,
        audience,
        issuer,
        clock_skew_ns,
        jet_auth_now_ns(),
    )
}

fn jet_auth_verify_jwt_impl(
    token: &String,
    key: &Vec<u8>,
    audience: &String,
    issuer: Option<&String>,
    clock_skew_ns: i64,
) -> Result<JetAuthClaims, JetAuthError> {
    if key.len() < 32 { return Err(JetAuthError::WeakKey); }
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
        return Err(JetAuthError::MalformedToken("JWT must have exactly three non-empty parts".to_string()));
    }
    let header = jet_auth_b64url_decode(parts[0]).map_err(JetAuthError::DecodeError)?;
    let header = std::str::from_utf8(&header)
        .map_err(|_| JetAuthError::MalformedToken("JWT header is not valid UTF-8".to_string()))?;
    let header = jet_auth_object(header)?;
    match header.get("alg") {
        Some(jet_std::JSON::Text(algorithm)) if algorithm == "HS256" => {}
        Some(jet_std::JSON::Text(algorithm)) => {
            return Err(JetAuthError::UnsupportedToken(format!("unsupported JWT algorithm `{algorithm}`")));
        }
        _ => return Err(JetAuthError::MalformedToken("JWT header requires text `alg`".to_string())),
    }
    let signature = jet_auth_b64url_decode(parts[2]).map_err(|_| JetAuthError::InvalidSignature)?;
    let expected = jet_hmac_sha256(key, format!("{}.{}", parts[0], parts[1]).as_bytes());
    if !jet_auth_ct_eq(&expected, &signature) { return Err(JetAuthError::InvalidSignature); }
    let payload = jet_auth_b64url_decode(parts[1]).map_err(JetAuthError::DecodeError)?;
    jet_auth_claims(&payload, audience, issuer.map(String::as_str), clock_skew_ns)
}

fn jet_auth_verify_jwt_defaulted(
    token: &String,
    key: &Vec<u8>,
    audience: &String,
    issuer: Option<&String>,
    clock_skew_ns: Option<i64>,
) -> Result<JetAuthClaims, JetAuthError> {
    let clock_skew_ns = jet_auth_clock_skew_ns(clock_skew_ns);
    jet_auth_verify_jwt_impl(
        token,
        key,
        audience,
        issuer,
        clock_skew_ns,
    )
}

fn jet_auth_pae(pieces: &[&[u8]]) -> Vec<u8> {
    let capacity = 8 + pieces.iter().map(|piece| 8 + piece.len()).sum::<usize>();
    let mut out = Vec::with_capacity(capacity);
    out.extend_from_slice(&(pieces.len() as u64).to_le_bytes());
    for piece in pieces {
        out.extend_from_slice(&(piece.len() as u64).to_le_bytes());
        out.extend_from_slice(piece);
    }
    out
}

fn jet_auth_verify_paseto_impl<F, E>(
    token: &String,
    key: &Vec<u8>,
    audience: &String,
    issuer: Option<&String>,
    clock_skew_ns: i64,
    footer: &Vec<u8>,
    implicit: &Vec<u8>,
    verify: F,
) -> Result<JetAuthClaims, JetAuthError>
where
    F: Fn(&Vec<u8>, &Vec<u8>, &Vec<u8>) -> Result<bool, E>,
{
    if key.len() != 32 { return Err(JetAuthError::WeakKey); }
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() < 3 || parts.len() > 4 || parts[0] != "v4" || parts[1] != "public" {
        return Err(JetAuthError::UnsupportedToken("only PASETO v4.public is supported".to_string()));
    }
    let body = jet_auth_b64url_decode(parts[2]).map_err(JetAuthError::DecodeError)?;
    if body.len() < 64 { return Err(JetAuthError::MalformedToken("PASETO body is shorter than its signature".to_string())); }
    let token_footer = if let Some(encoded) = parts.get(3) {
        jet_auth_b64url_decode(encoded).map_err(JetAuthError::DecodeError)?
    } else { Vec::new() };
    if !jet_auth_ct_eq(&token_footer, footer) { return Err(JetAuthError::InvalidSignature); }
    let split = body.len() - 64;
    let message = body[..split].to_vec();
    let signature = body[split..].to_vec();
    let signed = jet_auth_pae(&[b"v4.public.", &message, footer, implicit]);
    if !verify(key, &signed, &signature).unwrap_or(false) {
        return Err(JetAuthError::InvalidSignature);
    }
    jet_auth_claims(&message, audience, issuer.map(String::as_str), clock_skew_ns)
}

fn jet_auth_verify_paseto_defaulted<F, E>(
    token: &String,
    key: &Vec<u8>,
    audience: &String,
    issuer: Option<&String>,
    clock_skew_ns: Option<i64>,
    footer: Option<&Vec<u8>>,
    implicit: Option<&Vec<u8>>,
    verify: F,
) -> Result<JetAuthClaims, JetAuthError>
where
    F: Fn(&Vec<u8>, &Vec<u8>, &Vec<u8>) -> Result<bool, E>,
{
    let empty_footer = Vec::new();
    let empty_implicit = Vec::new();
    let clock_skew_ns = jet_auth_clock_skew_ns(clock_skew_ns);
    jet_auth_verify_paseto_impl(
        token,
        key,
        audience,
        issuer,
        clock_skew_ns,
        footer.unwrap_or(&empty_footer),
        implicit.unwrap_or(&empty_implicit),
        verify,
    )
}

#[cfg(test)]
mod auth_tests {
    use super::*;

    #[test]
    fn numeric_date_boundaries_keep_nanoseconds() {
        let expiry = br#"{"aud":"gateway","exp":10}"#;
        let expiry_ns = 10_i128 * i128::from(JET_AUTH_NANOS_PER_SECOND);
        let skew = 7_i64;
        let deadline = expiry_ns + i128::from(skew);
        assert!(matches!(
            jet_auth_claims_at(expiry, "gateway", None, skew, deadline - 1),
            Ok(_)
        ));
        assert!(matches!(
            jet_auth_claims_at(expiry, "gateway", None, skew, deadline),
            Err(JetAuthError::TokenExpired)
        ));
        assert!(matches!(
            jet_auth_claims_at(expiry, "gateway", None, skew, deadline + 1),
            Err(JetAuthError::TokenExpired)
        ));

        let not_before = br#"{"aud":"gateway","exp":30,"nbf":20,"iat":-9223372036854775808}"#;
        let not_before_ns = 20_i128 * i128::from(JET_AUTH_NANOS_PER_SECOND);
        let threshold = not_before_ns - i128::from(skew);
        assert!(matches!(
            jet_auth_claims_at(not_before, "gateway", None, skew, threshold - 1),
            Err(JetAuthError::TokenNotYetValid)
        ));
        let claims = jet_auth_claims_at(not_before, "gateway", None, skew, threshold)
            .expect("not-before boundary should be accepted");
        assert_eq!(claims.not_before, Some(20));
        assert_eq!(claims.issued_at, Some(i64::MIN));

        let maximum = br#"{"aud":"gateway","exp":30,"iat":9223372036854775807}"#;
        let claims = jet_auth_claims_at(maximum, "gateway", None, 0, 0)
            .expect("maximum issued-at should remain representable");
        assert_eq!(claims.issued_at, Some(i64::MAX));
    }
}
