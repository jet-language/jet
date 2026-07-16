// D-AUTH2=A (ratified 2026-07-13): `core.auth.verify_jwt(token, key)` —
// HMAC-SHA256 JWT verification. Pure std-only Rust; no external crates (I6).
//
// This file is entirely self-contained: it re-uses `jet_sha256_raw` and the
// base64url decoder, both of which are defined earlier in the concatenated
// prelude (RingCsvLogTimeCrypto.rs and EncodingCodecs.rs respectively).
// When this file is used in isolation (e.g. `include!` in a test module),
// stub versions of those two functions are expected in scope.
//
// Return type mirrors the Jet sema-level `Result<Claims, AuthError>`:
//   - `JetAuthClaims`  → the typed claims record
//   - `JetAuthError`   → structured error
//
// This is a standalone function; app.auth (card #438) will call it too —
// one mechanism, two entrypoints (I8).

#[derive(Debug, Clone)]
pub struct JetAuthClaims {
    /// `sub` claim — identifies the principal.
    pub subject: Option<String>,
    /// `aud` claim — may be a single string or JSON array; stored raw.
    pub audience: Option<String>,
    /// `exp` claim as Unix seconds.
    pub expires_at: Option<i64>,
    /// `iat` claim as Unix seconds.
    pub issued_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub enum JetAuthError {
    MalformedToken(String),
    InvalidSignature,
    TokenExpired,
    DecodeError(String),
}

// ── Internal b64url helper ────────────────────────────────────────────────────
// Mirrors jet_std_b64url_decode from EncodingCodecs.rs. Inlined here so that
// Auth.rs is self-contained when tested in isolation.

const JET_AUTH_B64_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn jet_auth_b64_decode_inner(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let bytes = s.as_bytes();
    let mut buf = 0u32;
    let mut bits = 0u8;
    for &b in bytes {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return Err(format!("invalid base64 character: {:?}", b as char)),
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
        !matches!(
            b,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'
        )
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

fn jet_auth_b64url_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(JET_AUTH_B64_CHARS[((n >> 18) & 63) as usize] as char);
        out.push(JET_AUTH_B64_CHARS[((n >> 12) & 63) as usize] as char);
        if i + 1 < bytes.len() { out.push(JET_AUTH_B64_CHARS[((n >> 6) & 63) as usize] as char); }
        if i + 2 < bytes.len() { out.push(JET_AUTH_B64_CHARS[(n & 63) as usize] as char); }
        i += 3;
    }
    // Strip trailing '=' equivalent (URL-safe, no padding).
    out
}

// ── HMAC-SHA256 ───────────────────────────────────────────────────────────────

/// HMAC-SHA256: RFC 2104. Uses `jet_sha256_raw` for the underlying digest.
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

// ── Minimal JSON field extraction ─────────────────────────────────────────────

/// Extract a JSON string value for `key` from a flat JSON object `src`.
fn jet_auth_json_string(src: &str, key: &str) -> Option<String> {
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
        let end = after.find(']')? + 1;
        Some(after[..end].to_string())
    } else {
        None
    }
}

/// Extract a JSON integer value for `key` from a flat JSON object `src`.
fn jet_auth_json_i64(src: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{}\"", key);
    let pos = src.find(needle.as_str())?;
    let after = src[pos + needle.len()..].trim_start();
    let after = after.strip_prefix(':')?;
    let after = after.trim_start();
    let end = after.find(|c: char| !c.is_ascii_digit() && c != '-').unwrap_or(after.len());
    after[..end].parse().ok()
}

// ── Public entry point ────────────────────────────────────────────────────────

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
    let parts: Vec<&str> = token.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err(JetAuthError::MalformedToken(
            "JWT must have exactly three dot-separated parts".to_string(),
        ));
    }
    let header_b64 = parts[0];
    let payload_b64 = parts[1];
    let sig_b64 = parts[2];

    // Verify signature.
    let signed_input = format!("{}.{}", header_b64, payload_b64);
    let expected_sig = jet_hmac_sha256(key, signed_input.as_bytes());
    let actual_sig_bytes = jet_auth_b64url_decode(sig_b64)
        .map_err(|_| JetAuthError::InvalidSignature)?;
    if actual_sig_bytes.len() != 32 {
        return Err(JetAuthError::InvalidSignature);
    }
    let mut actual_arr = [0u8; 32];
    actual_arr.copy_from_slice(&actual_sig_bytes);
    if !jet_ct_eq_32(&expected_sig, &actual_arr) {
        return Err(JetAuthError::InvalidSignature);
    }

    // Decode header; check algorithm.
    let header_bytes = jet_auth_b64url_decode(header_b64)
        .map_err(|e| JetAuthError::DecodeError(e))?;
    let header_str = String::from_utf8(header_bytes)
        .map_err(|_| JetAuthError::MalformedToken("header is not valid UTF-8".to_string()))?;
    let alg = jet_auth_json_string(&header_str, "alg");
    if alg.as_deref() != Some("HS256") {
        return Err(JetAuthError::MalformedToken(format!(
            "unsupported algorithm: {}",
            alg.unwrap_or_else(|| "(missing)".to_string())
        )));
    }

    // Decode payload.
    let payload_bytes = jet_auth_b64url_decode(payload_b64)
        .map_err(|e| JetAuthError::DecodeError(e))?;
    let payload_str = String::from_utf8(payload_bytes)
        .map_err(|_| JetAuthError::MalformedToken("payload is not valid UTF-8".to_string()))?;

    // Check expiry.
    let expires_at = jet_auth_json_i64(&payload_str, "exp");
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
        subject: jet_auth_json_string(&payload_str, "sub"),
        audience: jet_auth_json_string(&payload_str, "aud"),
        expires_at,
        issued_at: jet_auth_json_i64(&payload_str, "iat"),
    })
}
