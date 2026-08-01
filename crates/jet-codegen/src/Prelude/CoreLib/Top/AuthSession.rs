// ── D-AUTH1=A (#506): sessions, password login, OAuth, magic links ───────────
// `app.auth` reuses these same Prelude symbols (one mechanism; I9).

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
struct JetAuthUserStore {
    users: Vec<(String, String)>, // user_id, password_hash_label
    sessions: Vec<JetAuthSession>,
    magic_tokens: Vec<(String, String, i64)>, // token, user_id, expires
    oauth_states: Vec<(String, String)>, // state, provider
    next_id: i64,
}

thread_local! {
    static JET_AUTH_STORE: std::cell::RefCell<JetAuthUserStore> =
        std::cell::RefCell::new(JetAuthUserStore {
            users: Vec::new(),
            sessions: Vec::new(),
            magic_tokens: Vec::new(),
            oauth_states: Vec::new(),
            next_id: 0,
        });
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
    JET_AUTH_STORE.with(|store| {
        let mut s = store.borrow_mut();
        if s.users.iter().any(|(u, _)| u == &user_id) {
            return Err(format!(
                "user `{user_id}` already registered"
            ));
        }
        s.users.push((user_id, password_hash));
        Ok(())
    })
}

fn jet_auth_password_login(
    user_id: String,
    password_hash: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    JET_AUTH_STORE.with(|store| {
        let mut s = store.borrow_mut();
        let ok = s
            .users
            .iter()
            .any(|(u, h)| u == &user_id && h == &password_hash);
        if !ok {
            return Err("invalid credentials".to_string());
        }
        s.next_id += 1;
        let id = format!("sess-{}", s.next_id);
        let cookie = format!(
            "jet_session={id}; HttpOnly; Secure; SameSite=Lax; Path=/"
        );
        let session = JetAuthSession {
            id: id.clone(),
            user_id,
            expires_at: now_ms + ttl_ms,
            cookie,
        };
        s.sessions.push(session.clone());
        Ok(session)
    })
}

fn jet_auth_session_validate(
    session_id: &String,
    now_ms: i64,
) -> Result<JetAuthSession, String> {
    JET_AUTH_STORE.with(|store| {
        let s = store.borrow();
        s.sessions
            .iter()
            .find(|sess| &sess.id == session_id)
            .cloned()
            .ok_or("missing: session".to_string())
            .and_then(|sess| {
                if sess.expires_at < now_ms {
                    Err("token expired".to_string())
                } else {
                    Ok(sess)
                }
            })
    })
}

fn jet_auth_magic_link_issue(
    user_id: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<String, String> {
    JET_AUTH_STORE.with(|store| {
        let mut s = store.borrow_mut();
        s.next_id += 1;
        let token = format!("magic-{}-{}", s.next_id, user_id);
        s.magic_tokens
            .push((token.clone(), user_id, now_ms + ttl_ms));
        Ok(token)
    })
}

fn jet_auth_magic_link_consume(
    token: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    JET_AUTH_STORE.with(|store| {
        let mut s = store.borrow_mut();
        let idx = s
            .magic_tokens
            .iter()
            .position(|(t, _, exp)| t == &token && *exp >= now_ms)
            .ok_or("token expired".to_string())?;
        let (_, user_id, _) = s.magic_tokens.remove(idx);
        s.next_id += 1;
        let id = format!("sess-{}", s.next_id);
        let cookie =
            format!("jet_session={id}; HttpOnly; Secure; SameSite=Lax; Path=/");
        let session = JetAuthSession {
            id,
            user_id,
            expires_at: now_ms + ttl_ms,
            cookie,
        };
        s.sessions.push(session.clone());
        Ok(session)
    })
}

fn jet_auth_oauth_begin(provider: String) -> Result<String, String> {
    if provider.trim().is_empty() {
        return Err(
            "OAuth provider must be non-empty".to_string(),
        );
    }
    JET_AUTH_STORE.with(|store| {
        let mut s = store.borrow_mut();
        s.next_id += 1;
        let state = format!("oauth-{}-{}", s.next_id, provider);
        s.oauth_states.push((state.clone(), provider));
        Ok(state)
    })
}

fn jet_auth_oauth_finish(
    state: String,
    subject: String,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<JetAuthSession, String> {
    JET_AUTH_STORE.with(|store| {
        let mut s = store.borrow_mut();
        let idx = s
            .oauth_states
            .iter()
            .position(|(st, _)| st == &state)
            .ok_or("missing: oauth_state".to_string())?;
        let (_, provider) = s.oauth_states.remove(idx);
        let user_id = format!("{provider}:{subject}");
        if !s.users.iter().any(|(u, _)| u == &user_id) {
            s.users.push((user_id.clone(), "oauth".to_string()));
        }
        s.next_id += 1;
        let id = format!("sess-{}", s.next_id);
        let cookie =
            format!("jet_session={id}; HttpOnly; Secure; SameSite=Lax; Path=/");
        let session = JetAuthSession {
            id,
            user_id,
            expires_at: now_ms + ttl_ms,
            cookie,
        };
        s.sessions.push(session.clone());
        Ok(session)
    })
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
        let p = part.trim();
        if !p.is_empty() && !auth.providers.iter().any(|x| x == p) {
            auth.providers.push(p.to_string());
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
    for p in &auth.providers {
        routes.push(format!("GET /oauth/{p}/begin"));
        routes.push(format!("GET /oauth/{p}/callback"));
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
