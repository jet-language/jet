// D-AUTH2=A / D-AUTH-TOKENPOLICY1=A: strict standalone token verification.
// JSON goes through Jet's one RFC 8259 parser; Ed25519 goes through the vetted
// crypto bridge. No token-controlled algorithm or purpose is ever dispatched.

#[derive(Debug, Clone)]
pub struct JetAuthClaims {
    pub subject: Option<String>,
    pub audience: String,
    pub issuer: Option<String>,
    pub expires_at: i64,
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
}

fn jet_auth_b64_decode_inner(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u8;
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return Err("invalid base64 character".to_string()),
        };
        buf = (buf << 6) | v as u32;
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
    if a.len() != b.len() { return false; }
    let mut difference = 0u8;
    for (left, right) in a.iter().zip(b) { difference |= left ^ right; }
    difference == 0
}

fn jet_auth_object(text: &str) -> Result<std::collections::BTreeMap<String, jet_std::JSON>, JetAuthError> {
    match jet_std::parse_json_strict(text) {
        Ok(jet_std::JSON::Object(fields)) => Ok(fields),
        Ok(_) => Err(JetAuthError::MalformedToken("token JSON must be an object".to_string())),
        Err(error) => Err(JetAuthError::DecodeError(error.message)),
    }
}

fn jet_auth_optional_text(
    fields: &std::collections::BTreeMap<String, jet_std::JSON>,
    name: &str,
) -> Result<Option<String>, JetAuthError> {
    match fields.get(name) {
        None => Ok(None),
        Some(jet_std::JSON::Text(value)) => Ok(Some(value.clone())),
        Some(_) => Err(JetAuthError::MalformedToken(format!("claim `{name}` must be text"))),
    }
}

fn jet_auth_required_i64(
    fields: &std::collections::BTreeMap<String, jet_std::JSON>,
    name: &str,
) -> Result<i64, JetAuthError> {
    match fields.get(name) {
        None => Err(JetAuthError::MissingClaim(name.to_string())),
        Some(jet_std::JSON::Number(value))
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value <= i64::MAX as f64 => Ok(*value as i64),
        Some(_) => Err(JetAuthError::MalformedToken(format!("claim `{name}` must be an integer"))),
    }
}

fn jet_auth_optional_i64(
    fields: &std::collections::BTreeMap<String, jet_std::JSON>,
    name: &str,
) -> Result<Option<i64>, JetAuthError> {
    match fields.get(name) {
        None => Ok(None),
        Some(jet_std::JSON::Number(value))
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value <= i64::MAX as f64 => Ok(Some(*value as i64)),
        Some(_) => Err(JetAuthError::MalformedToken(format!("claim `{name}` must be an integer"))),
    }
}

fn jet_auth_audience(
    fields: &std::collections::BTreeMap<String, jet_std::JSON>,
    expected: &str,
) -> Result<String, JetAuthError> {
    match fields.get("aud") {
        None => Err(JetAuthError::MissingClaim("aud".to_string())),
        Some(jet_std::JSON::Text(actual)) if actual == expected => Ok(actual.clone()),
        Some(jet_std::JSON::Text(actual)) => Err(JetAuthError::WrongAudience {
            expected: expected.to_string(), actual: actual.clone(),
        }),
        Some(jet_std::JSON::Array(actual)) => {
            let values = actual.iter().map(|value| match value {
                jet_std::JSON::Text(text) => Ok(text),
                _ => Err(JetAuthError::MalformedToken("claim `aud` must contain only text".to_string())),
            }).collect::<Result<Vec<_>, _>>()?;
            if values.iter().any(|value| value.as_str() == expected) {
                Ok(expected.to_string())
            } else {
                Err(JetAuthError::WrongAudience {
                    expected: expected.to_string(),
                    actual: values.iter().map(|value| value.as_str()).collect::<Vec<_>>().join(","),
                })
            }
        }
        Some(_) => Err(JetAuthError::MalformedToken("claim `aud` must be text or a text list".to_string())),
    }
}

fn jet_auth_claims(
    payload: &[u8],
    audience: &str,
    issuer: Option<&str>,
    clock_skew_ms: i64,
) -> Result<JetAuthClaims, JetAuthError> {
    let text = std::str::from_utf8(payload)
        .map_err(|_| JetAuthError::MalformedToken("claims are not valid UTF-8".to_string()))?;
    let fields = jet_auth_object(text)?;
    let expires_at = jet_auth_required_i64(&fields, "exp")?;
    let actual_issuer = jet_auth_optional_text(&fields, "iss")?;
    if let Some(expected) = issuer {
        if actual_issuer.as_deref() != Some(expected) {
            return Err(JetAuthError::WrongIssuer {
                expected: expected.to_string(), actual: actual_issuer,
            });
        }
    }
    if clock_skew_ms < 0 {
        return Err(JetAuthError::MalformedToken("clock_skew cannot be negative".to_string()));
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX);
    let expires_at_ms = expires_at
        .checked_mul(1_000)
        .ok_or_else(|| JetAuthError::MalformedToken("claim `exp` is outside the supported millisecond range".to_string()))?;
    let valid_until_ms = expires_at_ms
        .checked_add(clock_skew_ms)
        .ok_or_else(|| JetAuthError::MalformedToken("clock_skew overflows the token expiry".to_string()))?;
    if now_ms >= valid_until_ms {
        return Err(JetAuthError::TokenExpired);
    }
    Ok(JetAuthClaims {
        subject: jet_auth_optional_text(&fields, "sub")?,
        audience: jet_auth_audience(&fields, audience)?,
        issuer: jet_auth_optional_text(&fields, "iss")?,
        expires_at,
        issued_at: jet_auth_optional_i64(&fields, "iat")?,
    })
}

fn jet_auth_verify_jwt_impl(
    token: &String,
    key: &Vec<u8>,
    audience: &String,
    issuer: Option<&String>,
    clock_skew_ms: i64,
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
    jet_auth_claims(&payload, audience, issuer.map(String::as_str), clock_skew_ms)
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
    clock_skew_ms: i64,
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
    jet_auth_claims(&message, audience, issuer.map(String::as_str), clock_skew_ms)
}
