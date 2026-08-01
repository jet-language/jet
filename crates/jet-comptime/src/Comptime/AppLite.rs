//! D-LIVEQUERY1=A / I9: `app` ambient includes Prelude `LiveQuery.rs`.

use crate::AST::{CtValue, Type};
use crate::Diagnostics::{Diagnostic, Span};
use super::Diagnostics::unsupported;

include!("../../../jet-codegen/src/Prelude/CoreLib/Top/LiveQuery.rs");

fn live_to_ct(q: &JetLiveQuery) -> CtValue {
    CtValue::Struct {
        type_name: "LiveQuery".to_string(),
        fields: vec![
            ("id".to_string(), CtValue::Str(q.id.clone())),
            ("footprint".to_string(), CtValue::Str(q.footprint.clone())),
            ("value".to_string(), CtValue::Str(q.value.clone())),
            ("generation".to_string(), CtValue::Int(q.generation)),
            ("active".to_string(), CtValue::Bool(q.active)),
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
    Ok(JetLiveQuery {
        id: match field("id")? {
            CtValue::Str(s) => s.clone(),
            _ => return Err(unsupported("id", span)),
        },
        footprint: match field("footprint")? {
            CtValue::Str(s) => s.clone(),
            _ => return Err(unsupported("footprint", span)),
        },
        value: match field("value")? {
            CtValue::Str(s) => s.clone(),
            _ => String::new(),
        },
        generation: match field("generation")? {
            CtValue::Int(n) => *n,
            _ => 1,
        },
        active: match field("active")? {
            CtValue::Bool(b) => *b,
            _ => true,
        },
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
