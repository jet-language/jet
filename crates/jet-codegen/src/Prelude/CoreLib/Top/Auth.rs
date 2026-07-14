// D-AUTH2=A (ratified 2026-07-13): `core.auth.verify_jwt(token, key)` —
// HMAC-SHA256 JWT verification. Pure std-only Rust; no external crates (I6).
// Builds on `jet_sha256_raw` and `jet_std_b64url_decode` already in the prelude.
//
// Return type mirrors the Jet sema-level `Result<Claims, AuthError>`:
//   - `JetAuthClaims`  → the typed claims record
//   - `JetAuthError`   → structured error
//
// This is a standalone function; app.auth (card #438) will call it too —
// one mechanism, two entrypoints (I8).

#[derive(Debug, Clone)]
struct JetAuthClaims {
    /// `sub` claim — identifies the principal.
    subject: Option<String>,
    /// `aud` claim — may be a single string or JSON array; stored raw.
    audience: Option<String>,
    /// `exp` claim as Unix seconds.
    expires_at: Option<i64>,
    /// `iat` claim as Unix seconds.
    issued_at: Option<i64>,
}

#[derive(Debug, Clone)]
enum JetAuthError {
    MalformedToken(String),
    InvalidSignature,
    TokenExpired,
    DecodeError(String),
}

/// HMAC-SHA256: RFC 2104.  Uses `jet_sha256_raw` for the underlying digest.
/// Key is zero-padded / hashed to 64 bytes (SHA-256 block size) as required.
fn jet_hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        let h = jet_sha256_raw(key);
        k[..32].copy_from_slice(&h);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0u8; BLOCK];
    let mut opad = [0u8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] = k[i] ^ 0x36;
        opad[i] = k[i] ^ 0x5c;
    }
    let mut inner_msg = Vec::with_capacity(BLOCK + data.len());
    inner_msg.extend_from_slice(&ipad);
    inner_msg.extend_from_slice(data);
    let inner_hash = jet_sha256_raw(&inner_msg);
    let mut outer_msg = Vec::with_capacity(BLOCK + 32);
    outer_msg.extend_from_slice(&opad);
    outer_msg.extend_from_slice(&inner_hash);
    jet_sha256_raw(&outer_msg)
}

/// Constant-time equality for 32-byte arrays (prevents timing attacks).
fn jet_ct_eq_32(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut acc = 0u8;
    for i in 0..32 {
        acc |= a[i] ^ b[i];
    }
    acc == 0
}

/// Extract a JSON string value for `key` from a flat JSON object `src`.
/// Handles basic `"key":"value"` and `"key": "value"` forms.
/// Returns `None` if the key is absent or the value is not a quoted string.
fn jet_json_string(src: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let pos = src.find(needle.as_str())?;
    let after = src[pos + needle.len()..].trim_start();
    let after = after.strip_prefix(':')?;
    let after = after.trim_start();
    if after.starts_with('"') {
        let inner = &after[1..];
        let end = inner.find('"')?;
        Some(inner[..end].to_string())
    } else if after.starts_with('[') {
        // Array — return the raw array string for the audience field.
        let end = after.find(']')? + 1;
        Some(after[..end].to_string())
    } else {
        None
    }
}

/// Extract a JSON integer value for `key` from a flat JSON object `src`.
fn jet_json_i64(src: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{}\"", key);
    let pos = src.find(needle.as_str())?;
    let after = src[pos + needle.len()..].trim_start();
    let after = after.strip_prefix(':')?;
    let after = after.trim_start();
    let end = after.find(|c: char| !c.is_ascii_digit() && c != '-').unwrap_or(after.len());
    after[..end].parse().ok()
}

/// `core.auth.verify_jwt(token: String, key: [Int8N]) -> Result<Claims, AuthError>`
///
/// Validates:
/// 1. Three-part `header.payload.signature` structure.
/// 2. `alg: HS256` in header.
/// 3. HMAC-SHA256 signature over `header_b64.payload_b64` with `key`.
/// 4. `exp` not in the past (checked against the system clock).
fn jet_auth_verify_jwt_impl(
    token: &String,
    key: &Vec<u8>,
) -> Result<JetAuthClaims, JetAuthError> {
    // Split into three base64url parts.
    let parts: Vec<&str> = token.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err(JetAuthError::MalformedToken(
            "JWT must have exactly three dot-separated parts".to_string(),
        ));
    }
    let header_b64 = parts[0];
    let payload_b64 = parts[1];
    let sig_b64 = parts[2];

    // Decode and verify signature.
    let signed_input = format!("{}.{}", header_b64, payload_b64);
    let expected_sig = jet_hmac_sha256(key, signed_input.as_bytes());
    let actual_sig_bytes = jet_std_b64url_decode(&sig_b64.to_string())
        .map_err(|e| JetAuthError::DecodeError(e))?;
    if actual_sig_bytes.len() != 32 {
        return Err(JetAuthError::InvalidSignature);
    }
    let mut actual_arr = [0u8; 32];
    actual_arr.copy_from_slice(&actual_sig_bytes);
    if !jet_ct_eq_32(&expected_sig, &actual_arr) {
        return Err(JetAuthError::InvalidSignature);
    }

    // Decode header; check algorithm.
    let header_bytes = jet_std_b64url_decode(&header_b64.to_string())
        .map_err(|e| JetAuthError::DecodeError(e))?;
    let header_str = String::from_utf8(header_bytes)
        .map_err(|_| JetAuthError::MalformedToken("header is not valid UTF-8".to_string()))?;
    let alg = jet_json_string(&header_str, "alg");
    if alg.as_deref() != Some("HS256") {
        return Err(JetAuthError::MalformedToken(format!(
            "unsupported algorithm: {}",
            alg.unwrap_or_else(|| "(missing)".to_string())
        )));
    }

    // Decode payload.
    let payload_bytes = jet_std_b64url_decode(&payload_b64.to_string())
        .map_err(|e| JetAuthError::DecodeError(e))?;
    let payload_str = String::from_utf8(payload_bytes)
        .map_err(|_| JetAuthError::MalformedToken("payload is not valid UTF-8".to_string()))?;

    // Check expiry.
    let expires_at = jet_json_i64(&payload_str, "exp");
    if let Some(exp) = expires_at {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if now > exp {
            return Err(JetAuthError::TokenExpired);
        }
    }

    Ok(JetAuthClaims {
        subject: jet_json_string(&payload_str, "sub"),
        audience: jet_json_string(&payload_str, "aud"),
        expires_at,
        issued_at: jet_json_i64(&payload_str, "iat"),
    })
}
