//! D-LIVEQUERY1=A / I9: `app` ambient includes Prelude `LiveQuery.rs`.

use crate::AST::{CtValue, Type};
use crate::Diagnostics::{Diagnostic, Span};
use super::Diagnostics::unsupported;

include!("../../../jet-codegen/src/Prelude/CoreLib/Top/LiveQuery.rs");

fn live_to_ct(q: &JetLiveQuery) -> CtValue {
    CtValue::Struct {
        type_name: "LiveQuery".to_string(),
        fields: vec![
            ("id".to_string(), CtValue::Int(q.id.min(i64::MAX as u64) as i64)),
            ("footprint".to_string(), CtValue::Str(q.footprint.display())),
            ("value".to_string(), CtValue::Str(q.value.clone())),
            (
                "generation".to_string(),
                CtValue::Int(q.generation.min(i64::MAX as u64) as i64),
            ),
            ("active".to_string(), CtValue::Bool(q.active)),
            ("error".to_string(), CtValue::Str(q.error.clone())),
        ],
    }
}

fn ct_to_live(v: &CtValue, span: Span) -> Result<JetLiveQuery, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
        return Err(unsupported("LiveQuery", span));
    };
    if type_name != "LiveQuery" && type_name != "JetLiveQuery" {
        return Err(unsupported("LiveQuery", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("LiveQuery field", span))
    };
    let footprint = match field("footprint")? {
        CtValue::Str(s) if s.is_empty() => JetLiveFootprint { paths: Vec::new() },
        CtValue::Str(s) => JetLiveFootprint::parse(s)
            .ok_or_else(|| unsupported("footprint", span))?,
        _ => return Err(unsupported("footprint", span)),
    };
    let active = match field("active")? {
        CtValue::Bool(b) => *b,
        _ => return Err(unsupported("active", span)),
    };
    let error = match field("error")? {
        CtValue::Str(s) => s.clone(),
        _ => return Err(unsupported("error", span)),
    };
    if !active && error.is_empty() {
        return Err(unsupported("LiveQuery error", span));
    }
    Ok(JetLiveQuery {
        id: match field("id")? {
            CtValue::Int(n) if *n >= 0 => *n as u64,
            _ => return Err(unsupported("id", span)),
        },
        footprint,
        value: match field("value")? {
            CtValue::Str(s) => s.clone(),
            _ => return Err(unsupported("value", span)),
        },
        generation: match field("generation")? {
            CtValue::Int(n) if *n >= 0 => *n as u64,
            _ => return Err(unsupported("generation", span)),
        },
        active,
        dirty: false,
        error,
    })
}

pub fn apply(method: &str, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let one = |i: usize| {
        args.get(i)
            .ok_or_else(|| unsupported(&format!("app.{method} arg {i}"), span))
    };
    match method {
        "live" => {
            let footprint = match one(0)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("footprint", span)),
            };
            let initial = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("initial", span)),
            };
            Ok(live_to_ct(&jet_app_live(footprint, initial)))
        }
        "subscribe" => {
            let source = match one(0)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("source", span)),
            };
            Ok(live_to_ct(&jet_app_subscribe(source)))
        }
        "invalidate" => {
            let footprint = match one(0)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("footprint", span)),
            };
            Ok(CtValue::Int(jet_app_invalidate(footprint)))
        }
        "transact_invalidate" => {
            let write_set = match one(0)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("write_set", span)),
            };
            Ok(CtValue::Int(jet_app_transact_invalidate(write_set)))
        }
        "signal_push" => {
            let payload = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("payload", span)),
            };
            Ok(live_to_ct(&jet_app_signal_push(
                &ct_to_live(one(0)?, span)?,
                payload,
            )))
        }
        "live_get" => Ok(CtValue::Str(jet_app_live_get(&ct_to_live(one(0)?, span)?))),
        "live_show" => Ok(CtValue::Str(jet_app_live_show(&ct_to_live(one(0)?, span)?))),
        "live_stats" => Ok(CtValue::Str(jet_app_live_stats())),
        _ => Err(unsupported(&format!("`app.{method}()`"), span)),
    }
}

#[allow(dead_code)]
fn _type_anchor() -> Type {
    Type::Named("LiveQuery".to_string())
}
