//! D-AUTH1=A / I9: `core.auth` ambient includes Prelude `AuthSession.rs` — the
//! session carrier accessors, the auth store byte cell behind the Jet store
//! (`Core/auth/auth.jet`), and `app.auth`. Session/password/magic-link/OAuth
//! policy is the Jet source on every tier; this adapter only marshals the
//! cell and the carriers. `verify_jwt`/`verify_paseto` are Jet as well.

use super::Diagnostics::unsupported;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtValue, Type};

include!("../../../jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs");

fn store_bytes(value: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match value {
        CtValue::Bytes(bytes) => Ok(bytes.clone()),
        CtValue::List(values) => values
            .iter()
            .map(|value| match value {
                CtValue::Int(value) if (0..=255).contains(value) => Ok(*value as u8),
                _ => Err(unsupported("auth store bytes must be a [U8] value", span)),
            })
            .collect(),
        _ => Err(unsupported("auth store bytes must be a [U8] value", span)),
    }
}

fn ct_to_session(v: &CtValue, span: Span) -> Result<JetAuthSession, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
        return Err(unsupported("Session", span));
    };
    if type_name != "Session" && type_name != "JetAuthSession" {
        return Err(unsupported("Session", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("Session field", span))
    };
    Ok(JetAuthSession {
        id: match field("id")? {
            CtValue::Str(s) => s.clone(),
            _ => return Err(unsupported("id", span)),
        },
        user_id: match field("user_id")? {
            CtValue::Str(s) => s.clone(),
            _ => return Err(unsupported("user_id", span)),
        },
        expires_at: match field("expires_at")? {
            CtValue::Int(n) => *n,
            _ => return Err(unsupported("expires_at", span)),
        },
        cookie: match field("cookie")? {
            CtValue::Str(s) => s.clone(),
            _ => return Err(unsupported("cookie", span)),
        },
    })
}

fn auth_to_ct(a: &JetAuthApp) -> CtValue {
    CtValue::Struct {
        type_name: "Auth".to_string(),
        fields: vec![
            (
                "users_table".to_string(),
                CtValue::Str(a.users_table.clone()),
            ),
            ("providers".to_string(), CtValue::Str(a.providers.join(","))),
        ],
    }
}

fn ct_to_auth(v: &CtValue, span: Span) -> Result<JetAuthApp, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
        return Err(unsupported("Auth", span));
    };
    if type_name != "Auth" && type_name != "JetAuthApp" {
        return Err(unsupported("Auth", span));
    }
    let users = fields
        .iter()
        .find(|(n, _)| n == "users_table")
        .and_then(|(_, v)| match v {
            CtValue::Str(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| unsupported("users_table", span))?;
    let providers = fields
        .iter()
        .find(|(n, _)| n == "providers")
        .and_then(|(_, v)| match v {
            CtValue::Str(s) => Some(
                s.split(',')
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect(),
            ),
            _ => None,
        })
        .ok_or_else(|| unsupported("providers", span))?;
    Ok(JetAuthApp {
        users_table: users,
        providers,
    })
}
pub fn apply(method: &str, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let one = |i: usize| {
        args.get(i)
            .ok_or_else(|| unsupported(&format!("core.auth.{method} arg {i}"), span))
    };
    let as_str = |i: usize| match one(i)? {
        CtValue::Str(s) => Ok(s.clone()),
        _ => Err(unsupported("String", span)),
    };
    match method {
        "__store_load" => Ok(CtValue::Bytes(jet_auth_store_load())),
        "__store_replace" => Ok(CtValue::Bool(jet_auth_store_replace(
            &store_bytes(one(0)?, span)?,
            store_bytes(one(1)?, span)?,
        ))),
        "session_show" => Ok(CtValue::Str(jet_auth_session_show(&ct_to_session(
            one(0)?,
            span,
        )?))),
        "session_user" => Ok(CtValue::Str(jet_auth_session_user(&ct_to_session(
            one(0)?,
            span,
        )?))),
        "session_cookie" => Ok(CtValue::Str(jet_auth_session_cookie(&ct_to_session(
            one(0)?,
            span,
        )?))),
        "session_id" => Ok(CtValue::Str(jet_auth_session_id(&ct_to_session(
            one(0)?,
            span,
        )?))),
        "auth" => Ok(auth_to_ct(&jet_app_auth(as_str(0)?))),
        "auth_oauth" => Ok(auth_to_ct(&jet_app_auth_oauth(
            ct_to_auth(one(0)?, span)?,
            as_str(1)?,
        ))),
        "auth_routes" => Ok(CtValue::Str(jet_app_auth_routes(&ct_to_auth(
            one(0)?,
            span,
        )?))),
        "auth_show" => Ok(CtValue::Str(jet_app_auth_show(&ct_to_auth(one(0)?, span)?))),
        _ => Err(unsupported(&format!("`core.auth.{method}()`"), span)),
    }
}

#[allow(dead_code)]
fn _type_anchor() -> Type {
    Type::Named("Session".to_string())
}
