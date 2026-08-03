// ── D-AUTH1=A (#506): sessions, password login, OAuth, magic links ───────────
// `app.auth` reuses these same Prelude symbols (one mechanism; I9).

use std::sync::{Mutex as JetAuthMutex, OnceLock as JetAuthOnceLock};

#[derive(Clone, Debug)]
pub struct JetAuthSession {
    pub id: String,
    pub user_id: String,
    pub expires_at: i64,
    pub cookie: String,
}

#[derive(Clone, Debug)]
pub struct JetAuthApp {
    pub users_table: String,
    pub providers: Vec<String>,
}

#[derive(Clone, Debug)]
struct JetAuthUser {
    user_id: String,
    password_hash: String,
}

#[derive(Clone, Debug)]
struct JetAuthMagicToken {
    token: String,
    user_id: String,
    expires_at: i64,
}

#[derive(Clone, Debug)]
struct JetAuthOAuthState {
    state: String,
    provider: String,
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

fn jet_auth_session_value(
    user_id: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    let expires_at = jet_auth_expiry(now_ms, ttl_ms)?;
    let id = jet_auth_opaque_token("sess")?;
    let cookie = format!(
        "jet_session={id}; HttpOnly; Secure; SameSite=Lax; Path=/"
    );
    Ok(JetAuthSession {
        id,
        user_id,
        expires_at,
        cookie,
    })
}

fn jet_auth_session_show(session: &JetAuthSession) -> String {
    format!(
        "Session(id={}, user={}, exp={}, cookie_len={})",
        session.id,
        session.user_id,
        session.expires_at,
        session.cookie.len()
    )
}

fn jet_auth_register_user(user_id: String, password_hash: String) -> Result<(), String> {
    if !jet_auth_valid_identifier(&user_id, 512) {
        return Err("user id is invalid".to_string());
    }
    if password_hash.trim().is_empty() || password_hash.len() > 4096 {
        return Err("password hash is invalid".to_string());
    }
    let Ok(mut store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    if store.users.iter().any(|user| user.user_id == user_id) {
        return Err(format!("user `{user_id}` already registered"));
    }
    store.users.push(JetAuthUser {
        user_id,
        password_hash,
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
        .any(|user| user.user_id == user_id && user.password_hash == password_hash);
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
    if !jet_auth_valid_identifier(session_id, 256) {
        return Err("missing: session".to_string());
    }
    let Ok(store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    store
        .sessions
        .iter()
        .find(|session| &session.id == session_id)
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
    let token = jet_auth_opaque_token("magic")?;
    let Ok(mut store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    store.magic_tokens.push(JetAuthMagicToken {
        token: token.clone(),
        user_id,
        expires_at,
    });
    Ok(token)
}

fn jet_auth_magic_link_consume(
    token: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    let _ = jet_auth_expiry(now_ms, ttl_ms)?;
    if !jet_auth_valid_identifier(&token, 256) {
        return Err("token expired".to_string());
    }
    let id = jet_auth_opaque_token("sess")?;
    let Ok(mut store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    let idx = store
        .magic_tokens
        .iter()
        .position(|entry| entry.token == token && now_ms < entry.expires_at)
        .ok_or_else(|| "token expired".to_string())?;
    let entry = store.magic_tokens.remove(idx);
    let expires_at = jet_auth_expiry(now_ms, ttl_ms)?;
    let session = JetAuthSession {
        id: id.clone(),
        user_id: entry.user_id,
        expires_at,
        cookie: format!(
            "jet_session={id}; HttpOnly; Secure; SameSite=Lax; Path=/"
        ),
    };
    store.sessions.push(session.clone());
    Ok(session)
}

fn jet_auth_oauth_begin(provider: String) -> Result<String, String> {
    if !jet_auth_valid_identifier(&provider, 128) {
        return Err("OAuth provider is invalid".to_string());
    }
    let state = jet_auth_opaque_token("oauth")?;
    let Ok(mut store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    store.oauth_states.push(JetAuthOAuthState {
        state: state.clone(),
        provider,
    });
    Ok(state)
}

fn jet_auth_oauth_finish(
    state: String,
    subject: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    let _ = jet_auth_expiry(now_ms, ttl_ms)?;
    if !jet_auth_valid_identifier(&state, 256)
        || !jet_auth_valid_identifier(&subject, 512)
    {
        return Err("OAuth callback is invalid".to_string());
    }
    let id = jet_auth_opaque_token("sess")?;
    let Ok(mut store) = jet_auth_store().lock() else {
        return Err("auth store is unavailable".to_string());
    };
    let idx = store
        .oauth_states
        .iter()
        .position(|entry| entry.state == state)
        .ok_or_else(|| "missing: oauth_state".to_string())?;
    let provider = store.oauth_states.remove(idx).provider;
    let user_id = format!("{provider}:{subject}");
    if !store.users.iter().any(|user| user.user_id == user_id) {
        store.users.push(JetAuthUser {
            user_id: user_id.clone(),
            password_hash: "oauth".to_string(),
        });
    }
    let expires_at = jet_auth_expiry(now_ms, ttl_ms)?;
    let session = JetAuthSession {
        id: id.clone(),
        user_id,
        expires_at,
        cookie: format!(
            "jet_session={id}; HttpOnly; Secure; SameSite=Lax; Path=/"
        ),
    };
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
