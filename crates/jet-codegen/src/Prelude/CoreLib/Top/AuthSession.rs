// ── D-AUTH1=A (#506): sessions, password login, OAuth, magic links ───────────
// `app.auth` reuses these same Prelude symbols (one mechanism; I9).

use std::sync::{Mutex as JetAuthMutex, OnceLock as JetAuthOnceLock};

#[derive(Clone)]
pub struct JetAuthSession {
    pub id: String,
    pub user_id: String,
    pub expires_at: i64,
    pub cookie: String,
}

impl std::fmt::Debug for JetAuthSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JetAuthSession")
            .field("id", &"<redacted>")
            .field("user_id", &self.user_id)
            .field("expires_at", &self.expires_at)
            .field("cookie", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct JetAuthApp {
    pub users_table: String,
    pub providers: Vec<String>,
}

#[derive(Clone)]
struct JetAuthUser {
    user_id: String,
    password_hash: String,
    delivery_capability: Option<String>,
}

impl std::fmt::Debug for JetAuthUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JetAuthUser")
            .field("user_id", &self.user_id)
            .field("password_hash", &"<redacted>")
            .field(
                "delivery_capability",
                &self.delivery_capability.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

#[derive(Clone)]
struct JetAuthMagicToken {
    token: String,
    user_id: String,
    expires_at: i64,
    delivery_capability: String,
}

impl std::fmt::Debug for JetAuthMagicToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JetAuthMagicToken")
            .field("token", &"<redacted>")
            .field("user_id", &self.user_id)
            .field("expires_at", &self.expires_at)
            .field("delivery_capability", &"<redacted>")
            .finish()
    }
}

#[derive(Default)]
struct JetAuthUserStore {
    users: Vec<JetAuthUser>,
    sessions: Vec<JetAuthSession>,
    magic_tokens: Vec<JetAuthMagicToken>,
    oauth_states: Vec<JetAuthOAuthState>,
}

static JET_AUTH_STORE: JetAuthOnceLock<JetAuthMutex<JetAuthUserStore>> = JetAuthOnceLock::new();

fn jet_auth_store() -> &'static JetAuthMutex<JetAuthUserStore> {
    JET_AUTH_STORE.get_or_init(|| JetAuthMutex::new(JetAuthUserStore::default()))
}

fn jet_auth_valid_identifier(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && value.chars().all(|c| !c.is_control())
}

// Secret-bearing session values must not use an early-exit string equality.
// The length is public framing; every byte position through the longer input
// is still visited before the result is returned.
fn jet_auth_constant_time_text_eq(left: &str, right: &str) -> bool {
    let mut difference = u8::from(left.len() != right.len());
    for index in 0..left.len().max(right.len()) {
        let a = left.as_bytes().get(index).copied().unwrap_or(0);
        let b = right.as_bytes().get(index).copied().unwrap_or(0);
        difference |= a ^ b;
    }
    difference == 0
}

fn jet_auth_delivery_capability(value: &str) -> Option<String> {
    if value.len() > 254 || value.matches('@').count() != 1 {
        return None;
    }
    let (local, domain) = value.split_once('@')?;
    if local.is_empty()
        || local.len() > 64
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~.".contains(&byte))
    {
        return None;
    }
    if domain.is_empty() || domain.len() > 253 {
        return None;
    }
    if domain.split('.').any(|label| {
        label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }) {
        return None;
    }
    Some(value.to_string())
}

fn jet_auth_expiry(now_ms: i64, ttl_ms: i64) -> Result<i64, String> {
    if ttl_ms <= 0 {
        return Err("auth lifetime must be positive".to_string());
    }
    now_ms
        .checked_add(ttl_ms)
        .ok_or_else(|| "auth lifetime is out of range".to_string())
}

fn jet_auth_opaque_token(prefix: &str) -> Result<String, String> {
    let bytes = jet_crypto_entropy_bytes(32)
        .map_err(|_| "cryptographic entropy is unavailable".to_string())?;
    let mut token = String::with_capacity(prefix.len() + 1 + bytes.len() * 2);
    token.push_str(prefix);
    token.push('-');
    for byte in bytes {
        token.push_str(&format!("{byte:02x}"));
    }
    Ok(token)
}
#[derive(Clone)]
struct JetAuthOAuthState {
    state: String,
    provider: String,
    nonce: String,
    expires_at: i64,
}

#[derive(Default)]
struct JetAuthOAuthClaims {
    algorithm: Option<String>,
    nonce: Option<String>,
    issuer: Option<String>,
    audience: Option<String>,
    subject: Option<String>,
    expires_at: Option<i64>,
    issued_at: Option<i64>,
}

fn jet_auth_valid_oauth_provider(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn jet_auth_oauth_env_suffix(provider: &str) -> Option<String> {
    jet_auth_valid_oauth_provider(provider).then(|| {
        provider
            .chars()
            .map(|character| character.to_ascii_uppercase())
            .collect()
    })
}

fn jet_auth_oauth_config(provider: &str) -> Result<(Vec<u8>, String, String), String> {
    let suffix = jet_auth_oauth_env_suffix(provider)
        .ok_or_else(|| "OAuth provider is invalid".to_string())?;
    let secret = std::env::var(format!("JET_OAUTH_{suffix}_SECRET"))
        .map_err(|_| "OAuth provider is not configured".to_string())?;
    if secret.as_bytes().len() < 32 {
        return Err("OAuth provider secret is too short".to_string());
    }
    let issuer = std::env::var(format!("JET_OAUTH_{suffix}_ISSUER"))
        .unwrap_or_else(|_| provider.to_string());
    let audience = std::env::var(format!("JET_OAUTH_{suffix}_AUDIENCE"))
        .unwrap_or_else(|_| provider.to_string());
    if !jet_auth_valid_identifier(&issuer, 1024) || !jet_auth_valid_identifier(&audience, 1024) {
        return Err("OAuth provider metadata is invalid".to_string());
    }
    Ok((secret.into_bytes(), issuer, audience))
}

fn jet_auth_now_ms() -> Result<i64, String> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "system clock is unavailable".to_string())?
        .as_millis();
    i64::try_from(millis).map_err(|_| "system clock is out of range".to_string())
}

fn jet_oauth_b64url_decode(text: &str) -> Result<Vec<u8>, String> {
    if text.is_empty() || text.len() > 16 * 1024 || text.len() % 4 == 1 {
        return Err("OAuth assertion encoding is invalid".to_string());
    }
    let mut out = Vec::with_capacity(text.len().saturating_mul(3) / 4);
    let mut accumulator = 0u32;
    let mut bits = 0u8;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err("OAuth assertion encoding is invalid".to_string()),
        };
        accumulator = ((accumulator << 6) | u32::from(value)) & 0x00ff_ffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((accumulator >> bits) & 0xff) as u8);
        }
    }
    if bits != 0 && (accumulator & ((1u32 << bits) - 1)) != 0 {
        return Err("OAuth assertion encoding is non-canonical".to_string());
    }
    Ok(out)
}

fn jet_oauth_json_hex4(chars: &[char], position: &mut usize) -> Option<u32> {
    let mut value = 0u32;
    for _ in 0..4 {
        let digit = chars.get(*position)?.to_digit(16)?;
        *position += 1;
        value = value * 16 + digit;
    }
    Some(value)
}

fn jet_oauth_json_string(chars: &[char], position: &mut usize) -> Option<String> {
    if chars.get(*position) != Some(&'"') {
        return None;
    }
    *position += 1;
    let mut out = String::new();
    while let Some(character) = chars.get(*position).copied() {
        *position += 1;
        match character {
            '"' => return Some(out),
            '\\' => {
                let escaped = chars.get(*position).copied()?;
                *position += 1;
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
                        let high = jet_oauth_json_hex4(chars, position)?;
                        if (0xD800..=0xDBFF).contains(&high) {
                            if chars.get(*position) != Some(&'\\') {
                                return None;
                            }
                            *position += 1;
                            if chars.get(*position) != Some(&'u') {
                                return None;
                            }
                            *position += 1;
                            let low = jet_oauth_json_hex4(chars, position)?;
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

fn jet_oauth_json_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\n' | '\r' | '\t')
}

fn jet_oauth_skip_whitespace(chars: &[char], position: &mut usize) {
    while chars
        .get(*position)
        .copied()
        .is_some_and(jet_oauth_json_whitespace)
    {
        *position += 1;
    }
}

fn jet_oauth_json_skip_value(chars: &[char], position: &mut usize) -> bool {
    let Some(first) = chars.get(*position).copied() else {
        return false;
    };
    if first == '"' {
        return jet_oauth_json_string(chars, position).is_some();
    }
    if first == '{' || first == '[' {
        let mut closers = vec![if first == '{' { '}' } else { ']' }];
        *position += 1;
        while let Some(character) = chars.get(*position).copied() {
            if character == '"' {
                if jet_oauth_json_string(chars, position).is_none() {
                    return false;
                }
                continue;
            }
            if character == '{' {
                closers.push('}');
                *position += 1;
                continue;
            }
            if character == '[' {
                closers.push(']');
                *position += 1;
                continue;
            }
            if character == '}' || character == ']' {
                if closers.last().copied() != Some(character) {
                    return false;
                }
                closers.pop();
                *position += 1;
                if closers.is_empty() {
                    return true;
                }
                continue;
            }
            *position += 1;
        }
        return false;
    }
    let start = *position;
    while let Some(character) = chars.get(*position).copied() {
        if jet_oauth_json_whitespace(character)
            || matches!(character, ',' | '}' | ']')
        {
            break;
        }
        *position += 1;
    }
    *position > start
}

fn jet_oauth_parse_i64(text: &str) -> Option<i64> {
    let (negative, digits) = text
        .strip_prefix('-')
        .map_or((false, text), |digits| (true, digits));
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
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

fn jet_oauth_parse_claims(text: &str) -> Result<JetAuthOAuthClaims, String> {
    if text.len() > 32 * 1024 {
        return Err("OAuth assertion payload is too large".to_string());
    }
    let chars: Vec<char> = text.chars().collect();
    let mut position = 0usize;
    jet_oauth_skip_whitespace(&chars, &mut position);
    if chars.get(position) != Some(&'{') {
        return Err("OAuth assertion payload is not an object".to_string());
    }
    position += 1;
    let mut claims = JetAuthOAuthClaims::default();
    loop {
        jet_oauth_skip_whitespace(&chars, &mut position);
        if chars.get(position) == Some(&'}') {
            position += 1;
            break;
        }
        let key = jet_oauth_json_string(&chars, &mut position)
            .ok_or_else(|| "OAuth assertion claim name is invalid".to_string())?;
        jet_oauth_skip_whitespace(&chars, &mut position);
        if chars.get(position) != Some(&':') {
            return Err("OAuth assertion claim separator is invalid".to_string());
        }
        position += 1;
        jet_oauth_skip_whitespace(&chars, &mut position);
        match key.as_str() {
            "alg" | "nonce" | "iss" | "aud" | "sub" => {
                let value = jet_oauth_json_string(&chars, &mut position)
                    .ok_or_else(|| format!("OAuth claim `{key}` must be text"))?;
                let slot = match key.as_str() {
                    "alg" => &mut claims.algorithm,
                    "nonce" => &mut claims.nonce,
                    "iss" => &mut claims.issuer,
                    "aud" => &mut claims.audience,
                    "sub" => &mut claims.subject,
                    _ => unreachable!(),
                };
                if slot.replace(value).is_some() {
                    return Err(format!("OAuth claim `{key}` is duplicated"));
                }
            }
            "exp" | "iat" => {
                let start = position;
                while let Some(character) = chars.get(position).copied() {
                    if jet_oauth_json_whitespace(character)
                        || matches!(character, ',' | '}' | ']')
                    {
                        break;
                    }
                    position += 1;
                }
                let value = jet_oauth_parse_i64(
                    &chars[start..position].iter().copied().collect::<String>(),
                )
                .ok_or_else(|| format!("OAuth claim `{key}` must be an integer"))?;
                let slot = if key == "exp" {
                    &mut claims.expires_at
                } else {
                    &mut claims.issued_at
                };
                if slot.replace(value).is_some() {
                    return Err(format!("OAuth claim `{key}` is duplicated"));
                }
            }
            _ => {
                if !jet_oauth_json_skip_value(&chars, &mut position) {
                    return Err("OAuth assertion claim value is invalid".to_string());
                }
            }
        }
        jet_oauth_skip_whitespace(&chars, &mut position);
        match chars.get(position).copied() {
            Some(',') => position += 1,
            Some('}') => {
                position += 1;
                break;
            }
            _ => return Err("OAuth assertion object is invalid".to_string()),
        }
    }
    jet_oauth_skip_whitespace(&chars, &mut position);
    if position != chars.len() {
        return Err("OAuth assertion has trailing data".to_string());
    }
    Ok(claims)
}

fn jet_auth_oauth_verify(
    entry: &JetAuthOAuthState,
    assertion: &str,
    now_ms: i64,
    secret: &[u8],
    issuer: &str,
    audience: &str,
) -> Result<String, String> {
    if assertion.len() > 64 * 1024 {
        return Err("OAuth assertion is too large".to_string());
    }
    let mut parts = assertion.split('.');
    let header_part = parts
        .next()
        .ok_or_else(|| "OAuth assertion is malformed".to_string())?;
    let payload_part = parts
        .next()
        .ok_or_else(|| "OAuth assertion is malformed".to_string())?;
    let signature_part = parts
        .next()
        .ok_or_else(|| "OAuth assertion is malformed".to_string())?;
    if parts.next().is_some()
        || header_part.is_empty()
        || payload_part.is_empty()
        || signature_part.is_empty()
    {
        return Err("OAuth assertion is malformed".to_string());
    }
    let header = String::from_utf8(jet_oauth_b64url_decode(header_part)?)
        .map_err(|_| "OAuth assertion header is not UTF-8".to_string())?;
    let payload = String::from_utf8(jet_oauth_b64url_decode(payload_part)?)
        .map_err(|_| "OAuth assertion payload is not UTF-8".to_string())?;
    let signature = jet_oauth_b64url_decode(signature_part)?;
    let header_claims = jet_oauth_parse_claims(&header)?;
    if header_claims.algorithm.as_deref() != Some("HS256") {
        return Err("OAuth assertion algorithm is not allowed".to_string());
    }
    let expected = jet_hmac_sha256(
        secret,
        format!("{header_part}.{payload_part}").as_bytes(),
    );
    if !jet_ct_eq(&expected, &signature) {
        return Err("OAuth assertion signature is invalid".to_string());
    }
    let claims = jet_oauth_parse_claims(&payload)?;
    if claims.nonce.as_deref().is_none_or(|nonce| {
        !jet_auth_constant_time_text_eq(nonce, &entry.nonce)
    }) {
        return Err("OAuth assertion nonce is invalid".to_string());
    }
    if claims.issuer.as_deref() != Some(issuer) {
        return Err("OAuth assertion issuer is invalid".to_string());
    }
    if claims.audience.as_deref() != Some(audience) {
        return Err("OAuth assertion audience is invalid".to_string());
    }
    let now_s = now_ms
        .checked_div(1_000)
        .ok_or_else(|| "OAuth clock is invalid".to_string())?;
    let expires_at = claims
        .expires_at
        .ok_or_else(|| "OAuth assertion expiry is missing".to_string())?;
    if expires_at <= now_s {
        return Err("OAuth assertion is expired".to_string());
    }
    if claims
        .issued_at
        .is_some_and(|issued_at| issued_at > now_s.saturating_add(60))
    {
        return Err("OAuth assertion was issued in the future".to_string());
    }
    let subject = claims
        .subject
        .ok_or_else(|| "OAuth assertion subject is missing".to_string())?;
    if !jet_auth_valid_identifier(&subject, 512) {
        return Err("OAuth assertion subject is invalid".to_string());
    }
    Ok(subject)
}

fn jet_auth_valid_session_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 69
        && bytes.starts_with(b"sess-")
        && bytes[5..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}


/// D-AUTH1=A: the session cookie's `HttpOnly`/`Secure`/`SameSite`/`Path`
/// defaults are ONE fact. Password login, magic-link consume, and OAuth finish
/// all mint a session cookie; spelling the flags at each site meant a hardening
/// change could reach two of the three.
///
/// Named `_mint` because `jet_auth_session_cookie` is already taken by the Core
/// accessor below, which is the registered symbol for `core.auth.session_cookie`
/// and returns a stored cookie rather than building one.
fn jet_auth_session_cookie_mint(id: &str) -> String {
    format!("jet_session={id}; HttpOnly; Secure; SameSite=Lax; Path=/")
}

fn jet_auth_session_value(
    user_id: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    let expires_at = jet_auth_expiry(now_ms, ttl_ms)?;
    let id = jet_auth_opaque_token("sess")?;
    let cookie = jet_auth_session_cookie_mint(&id);
    Ok(JetAuthSession {
        id,
        user_id,
        expires_at,
        cookie,
    })
}

fn jet_auth_session_show(session: &JetAuthSession) -> String {
    format!(
        "Session(id=<redacted>, user={}, exp={}, cookie_len={})",
        session.user_id,
        session.expires_at,
        session.cookie.len()
    )
}

pub(crate) fn jet_auth_register_user(user_id: String, password_hash: String) -> Result<(), String> {
    if !jet_auth_valid_identifier(&user_id, 512) {
        return Err("user id is invalid".to_string());
    }
    if password_hash.trim().is_empty() || password_hash.len() > 4096 {
        return Err("password hash is invalid".to_string());
    }
    let delivery_capability = jet_auth_delivery_capability(&user_id);
    let Ok(mut store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    if store.users.iter().any(|user| user.user_id == user_id) {
        return Err(format!("user `{user_id}` already registered"));
    }
    store.users.push(JetAuthUser {
        user_id,
        password_hash,
        delivery_capability,
    });
    Ok(())
}

fn jet_auth_password_login(
    user_id: String,
    password_hash: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    let session = jet_auth_session_value(user_id.clone(), now_ms, ttl_ms)?;
    let Ok(mut store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    let ok = store
        .users
        .iter()
        .any(|user| {
            user.user_id == user_id
                && jet_auth_constant_time_text_eq(&user.password_hash, &password_hash)
        });
    if !ok {
        return Err("invalid credentials".to_string());
    }
    store.sessions.push(session.clone());
    Ok(session)
}

fn jet_auth_session_validate(
    session_id: &String,
    now_ms: i64,
) -> Result<JetAuthSession, String> {
    if !jet_auth_valid_session_id(session_id) {
        return Err("missing: session".to_string());
    }
    let Ok(store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    store
        .sessions
        .iter()
        .find(|session| jet_auth_constant_time_text_eq(&session.id, session_id))
        .cloned()
        .ok_or_else(|| "missing: session".to_string())
        .and_then(|session| {
            if now_ms >= session.expires_at {
                Err("token expired".to_string())
            } else {
                Ok(session)
            }
        })
}

fn jet_auth_magic_link_issue(
    user_id: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<String, String> {
    let expires_at = jet_auth_expiry(now_ms, ttl_ms)?;
    if !jet_auth_valid_identifier(&user_id, 512) {
        return Err("user id is invalid".to_string());
    }
    let delivery_capability = jet_auth_delivery_capability(&user_id)
        .ok_or_else(|| "magic link requires a delivery-capable identity".to_string())?;
    let Ok(mut store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    if !store.users.iter().any(|user| {
        user.user_id == user_id
            && user.delivery_capability.as_deref() == Some(delivery_capability.as_str())
    }) {
        return Err("magic link requires a registered delivery identity".to_string());
    }
    let token = jet_auth_opaque_token("magic")?;
    store.magic_tokens.push(JetAuthMagicToken {
        token: token.clone(),
        user_id,
        expires_at,
        delivery_capability,
    });
    Ok(token)
}

pub(crate) fn jet_auth_magic_link_consume(
    token: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    let _ = jet_auth_expiry(now_ms, ttl_ms)?;
    if !jet_auth_valid_identifier(&token, 256) {
        return Err("token expired".to_string());
    }
    let Ok(mut store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    let idx = store
        .magic_tokens
        .iter()
        .position(|entry| {
            jet_auth_constant_time_text_eq(&entry.token, &token) && now_ms < entry.expires_at
        })
        .ok_or_else(|| "token expired".to_string())?;
    let entry = store.magic_tokens[idx].clone();
    let Some(user) = store.users.iter().find(|user| user.user_id == entry.user_id) else {
        return Err("magic link identity is no longer registered".to_string());
    };
    if user.delivery_capability.as_deref() != Some(entry.delivery_capability.as_str()) {
        return Err("magic link delivery identity is unavailable".to_string());
    }
    let id = jet_auth_opaque_token("sess")?;
    store.magic_tokens.remove(idx);
    let expires_at = jet_auth_expiry(now_ms, ttl_ms)?;
    let session = JetAuthSession {
        id: id.clone(),
        user_id: entry.user_id,
        expires_at,
        cookie: jet_auth_session_cookie_mint(&id),
    };
    store.sessions.push(session.clone());
    Ok(session)
}

fn jet_auth_oauth_begin(provider: String) -> Result<String, String> {
    let _ = jet_auth_oauth_config(&provider)?;
    let now_ms = jet_auth_now_ms()?;
    let expires_at = now_ms
        .checked_add(10 * 60 * 1_000)
        .ok_or_else(|| "OAuth state lifetime is out of range".to_string())?;
    let state = jet_auth_opaque_token("oauth")?;
    // The public API returns only `state`; use that opaque value as the OIDC
    // nonce too so callers can place the same value in the provider request.
    let nonce = state.clone();
    let Ok(mut store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    store.oauth_states.retain(|entry| entry.expires_at > now_ms);
    if store.oauth_states.len() >= 256 {
        store.oauth_states.remove(0);
    }
    store.oauth_states.push(JetAuthOAuthState {
        state: state.clone(),
        provider,
        nonce,
        expires_at,
    });
    Ok(state)
}

fn jet_auth_oauth_finish(
    state: String,
    assertion: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    if !jet_auth_valid_identifier(&state, 256) || assertion.is_empty() {
        return Err("OAuth completion is invalid".to_string());
    }
    let Ok(mut store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    let index = store
        .oauth_states
        .iter()
        .position(|entry| {
            jet_auth_constant_time_text_eq(&entry.state, &state) && now_ms < entry.expires_at
        })
        .ok_or_else(|| "OAuth state is expired or unknown".to_string())?;
    let entry = store.oauth_states[index].clone();
    let (secret, issuer, audience) = jet_auth_oauth_config(&entry.provider)?;
    let subject = jet_auth_oauth_verify(
        &entry,
        &assertion,
        now_ms,
        &secret,
        &issuer,
        &audience,
    )?;
    let session = jet_auth_session_value(subject, now_ms, ttl_ms)?;
    store.oauth_states.remove(index);
    store.sessions.push(session.clone());
    Ok(session)
}

fn jet_auth_session_user(session: &JetAuthSession) -> String {
    session.user_id.clone()
}

fn jet_auth_session_cookie(session: &JetAuthSession) -> String {
    session.cookie.clone()
}

fn jet_auth_session_id(session: &JetAuthSession) -> String {
    session.id.clone()
}

fn jet_app_auth(users_table: String) -> JetAuthApp {
    JetAuthApp {
        users_table,
        providers: Vec::new(),
    }
}

fn jet_app_auth_oauth(mut auth: JetAuthApp, providers: String) -> JetAuthApp {
    for part in providers.split(|c| c == ',' || c == ' ') {
        let provider = part.trim();
        if jet_auth_valid_identifier(provider, 128)
            && !auth.providers.iter().any(|existing| existing == provider)
        {
            auth.providers.push(provider.to_string());
        }
    }
    auth
}

fn jet_app_auth_routes(auth: &JetAuthApp) -> String {
    let mut routes = vec![
        format!("POST /login -> password ({})", auth.users_table),
        "POST /logout -> revoke session".to_string(),
        "GET /magic -> begin magic link".to_string(),
        "POST /magic -> consume magic link".to_string(),
    ];
    for provider in &auth.providers {
        routes.push(format!("GET /oauth/{provider}/begin"));
        routes.push(format!("GET /oauth/{provider}/callback"));
    }
    format!("AuthRoutes({})", routes.join("; "))
}

fn jet_app_auth_show(auth: &JetAuthApp) -> String {
    format!(
        "Auth(users={}, providers=[{}])",
        auth.users_table,
        auth.providers.join(",")
    )
}

// Resident/interpreter adapters use these wrappers for the remaining auth
// calls; register/consume adapters call the canonical Prelude symbols above.
// All paths remain in this one policy and state seam.
pub fn auth_runtime_reset() {
    let Some(store) = JET_AUTH_STORE.get() else {
        return;
    };
    let mut store = match store.lock() {
        Ok(store) => store,
        Err(poisoned) => poisoned.into_inner(),
    };
    *store = JetAuthUserStore::default();
}

pub fn auth_register_user(user_id: String, password_hash: String) -> Result<(), String> {
    jet_auth_register_user(user_id, password_hash)
}

pub fn auth_password_login(
    user_id: String,
    password_hash: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    jet_auth_password_login(user_id, password_hash, now_ms, ttl_ms)
}

pub fn auth_session_validate(
    session_id: &String,
    now_ms: i64,
) -> Result<JetAuthSession, String> {
    jet_auth_session_validate(session_id, now_ms)
}

pub fn auth_magic_link_issue(
    user_id: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<String, String> {
    jet_auth_magic_link_issue(user_id, now_ms, ttl_ms)
}

pub fn auth_magic_link_consume(
    token: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    jet_auth_magic_link_consume(token, now_ms, ttl_ms)
}

pub fn auth_oauth_begin(provider: String) -> Result<String, String> {
    jet_auth_oauth_begin(provider)
}

pub fn auth_oauth_finish(
    state: String,
    subject: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    jet_auth_oauth_finish(state, subject, now_ms, ttl_ms)
}

pub fn auth_session_show(session: &JetAuthSession) -> String {
    jet_auth_session_show(session)
}

pub fn auth_session_user(session: &JetAuthSession) -> String {
    jet_auth_session_user(session)
}

pub fn auth_session_cookie(session: &JetAuthSession) -> String {
    jet_auth_session_cookie(session)
}

pub fn auth_session_id(session: &JetAuthSession) -> String {
    jet_auth_session_id(session)
}

pub fn app_auth(users_table: String) -> JetAuthApp {
    jet_app_auth(users_table)
}

pub fn app_auth_oauth(auth: JetAuthApp, providers: String) -> JetAuthApp {
    jet_app_auth_oauth(auth, providers)
}

pub fn app_auth_routes(auth: &JetAuthApp) -> String {
    jet_app_auth_routes(auth)
}

pub fn app_auth_show(auth: &JetAuthApp) -> String {
    jet_app_auth_show(auth)
}
