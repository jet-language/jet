//! D-AUTH1=A / I9: `core.auth` ambient includes Prelude `Auth.rs`.

use crate::AST::{CtValue, Type};
use crate::Diagnostics::{Diagnostic, Span};
use super::Diagnostics::unsupported;

#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../../jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../../jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs");

fn session_to_ct(s: &JetAuthSession) -> CtValue {
    CtValue::Struct {
        type_name: "Session".to_string(),
        fields: vec![
            ("id".to_string(), CtValue::Str(s.id.clone())),
            ("user_id".to_string(), CtValue::Str(s.user_id.clone())),
            ("expires_at".to_string(), CtValue::Int(s.expires_at)),
            ("cookie".to_string(), CtValue::Str(s.cookie.clone())),
        ],
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
            ("users_table".to_string(), CtValue::Str(a.users_table.clone())),
            (
                "providers".to_string(),
                CtValue::Str(a.providers.join(",")),
            ),
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

fn result_session(r: Result<JetAuthSession, String>) -> CtValue {
    match r {
        Ok(s) => CtValue::Present(Box::new(session_to_ct(&s))),
        Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
    }
}

fn result_unit(r: Result<(), String>) -> CtValue {
    match r {
        Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
        Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
    }
}

fn result_str(r: Result<String, String>) -> CtValue {
    match r {
        Ok(s) => CtValue::Present(Box::new(CtValue::Str(s))),
        Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
    }
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
    let as_int = |i: usize| match one(i)? {
        CtValue::Int(n) => Ok(*n),
        _ => Err(unsupported("Int", span)),
    };
    match method {
        "register_user" => Ok(result_unit(jet_auth_register_user(as_str(0)?, as_str(1)?))),
        "password_login" => Ok(result_session(jet_auth_password_login(
            as_str(0)?,
            as_str(1)?,
            as_int(2)?,
            as_int(3)?,
        ))),
        "session_validate" => Ok(result_session(jet_auth_session_validate(
            &as_str(0)?,
            as_int(1)?,
        ))),
        "session_show" => Ok(CtValue::Str(jet_auth_session_show(&ct_to_session(
            one(0)?, span,
        )?))),
        "session_user" => Ok(CtValue::Str(jet_auth_session_user(&ct_to_session(
            one(0)?, span,
        )?))),
        "session_cookie" => Ok(CtValue::Str(jet_auth_session_cookie(&ct_to_session(
            one(0)?, span,
        )?))),
        "session_id" => Ok(CtValue::Str(jet_auth_session_id(&ct_to_session(
            one(0)?, span,
        )?))),
        "magic_link_issue" => Ok(result_str(jet_auth_magic_link_issue(
            as_str(0)?,
            as_int(1)?,
            as_int(2)?,
        ))),
        "magic_link_consume" => Ok(result_session(jet_auth_magic_link_consume(
            as_str(0)?,
            as_int(1)?,
            as_int(2)?,
        ))),
        "oauth_begin" => Ok(result_str(jet_auth_oauth_begin(as_str(0)?))),
        "oauth_finish" => Ok(result_session(jet_auth_oauth_finish(
            as_str(0)?,
            as_str(1)?,
            as_int(2)?,
            as_int(3)?,
        ))),
        "auth" => Ok(auth_to_ct(&jet_app_auth(as_str(0)?))),
        "auth_oauth" => Ok(auth_to_ct(&jet_app_auth_oauth(
            ct_to_auth(one(0)?, span)?,
            as_str(1)?,
        ))),
        "auth_routes" => Ok(CtValue::Str(jet_app_auth_routes(&ct_to_auth(
            one(0)?, span,
        )?))),
        "auth_show" => Ok(CtValue::Str(jet_app_auth_show(&ct_to_auth(
            one(0)?, span,
        )?))),
        _ => Err(unsupported(&format!("`core.auth.{method}()`"), span)),
    }
}

#[allow(dead_code)]
fn _type_anchor() -> Type {
    Type::Named("Session".to_string())
}
