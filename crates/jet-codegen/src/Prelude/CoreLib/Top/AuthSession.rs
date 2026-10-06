// ── D-AUTH1=A (#506): session carriers, the auth store cell, `app.auth` ───────
// Session, password, magic-link and OAuth policy is Jet (`Core/auth/auth.jet`).
// This file keeps the `Session` carrier, the `app.auth` route descriptor, and
// the one host primitive the Jet store needs: a process-global byte cell with
// an atomic compare-and-replace. `app.auth` reuses these Prelude symbols (I9).

use std::sync::Mutex as JetAuthMutex;

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

/// The auth store's bytes. Their layout and meaning belong to the Jet store.
static JET_AUTH_STORE_CELL: JetAuthMutex<Vec<u8>> = JetAuthMutex::new(Vec::new());

fn jet_auth_store_cell() -> std::sync::MutexGuard<'static, Vec<u8>> {
    match JET_AUTH_STORE_CELL.lock() {
        Ok(cell) => cell,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// `core.auth.__store_load`: a copy of the current store bytes.
fn jet_auth_store_load() -> Vec<u8> {
    jet_auth_store_cell().clone()
}

/// `core.auth.__store_replace`: install `next` only while the cell still holds
/// `expected` (the bytes the caller read); reports whether it did.
fn jet_auth_store_replace(expected: &Vec<u8>, next: Vec<u8>) -> bool {
    let mut cell = jet_auth_store_cell();
    if *cell != *expected {
        return false;
    }
    *cell = next;
    true
}

fn jet_auth_valid_identifier(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && value.chars().all(|c| !c.is_control())
}

fn jet_auth_session_show(session: &JetAuthSession) -> String {
    format!(
        "Session(id=<redacted>, user={}, exp={}, cookie_len={})",
        session.user_id,
        session.expires_at,
        session.cookie.len()
    )
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

// One-shot evaluator runs start from an empty store.
pub fn auth_runtime_reset() {
    jet_auth_store_cell().clear();
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
