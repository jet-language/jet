//! D-SYNC1=A / D-DBPOLICY1=A / I9: `core.sync` ambient includes Prelude `Sync.rs`.

use crate::AST::{CtValue, Type};
use crate::Diagnostics::{Diagnostic, Span};
use super::Diagnostics::unsupported;

include!("../../../jet-codegen/src/Prelude/CoreLib/Top/Sync.rs");

fn text_to_ct(doc: &JetSyncText) -> CtValue {
    CtValue::Struct {
        type_name: "SyncText".to_string(),
        fields: vec![(
            "show".to_string(),
            CtValue::Str(jet_sync_text_show(doc)),
        )],
    }
}

fn counter_to_ct(c: &JetSyncCounter) -> CtValue {
    CtValue::Struct {
        type_name: "SyncCounter".to_string(),
        fields: vec![("value".to_string(), CtValue::Int(jet_sync_counter_value(c)))],
    }
}

fn map_to_ct(m: &JetSyncMap) -> CtValue {
    CtValue::Struct {
        type_name: "SyncMap".to_string(),
        fields: vec![("show".to_string(), CtValue::Str(jet_sync_map_show(m)))],
    }
}

fn list_to_ct(l: &JetSyncList) -> CtValue {
    CtValue::Struct {
        type_name: "SyncList".to_string(),
        fields: vec![("show".to_string(), CtValue::Str(jet_sync_list_show(l)))],
    }
}

fn policy_to_ct(p: &JetRowPolicy) -> CtValue {
    CtValue::Struct {
        type_name: "RowPolicy".to_string(),
        fields: vec![
            ("table".to_string(), CtValue::Str(p.table.clone())),
            ("expression".to_string(), CtValue::Str(p.expression.clone())),
        ],
    }
}

fn ct_to_text(v: &CtValue, span: Span) -> Result<JetSyncText, Diagnostic> {
    match v {
        CtValue::Struct { type_name, fields }
            if type_name == "SyncText" || type_name == "JetSyncText" =>
        {
            let show = fields
                .iter()
                .find(|(n, _)| n == "show")
                .and_then(|(_, v)| match v {
                    CtValue::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            // Round-trip via replica encoding `SyncText(a:x|b:y)`.
            let body = show
                .strip_prefix("SyncText(")
                .and_then(|s| s.strip_suffix(')'))
                .unwrap_or("");
            let mut doc = JetSyncText {
                replicas: Vec::new(),
            };
            if !body.is_empty() {
                for part in body.split('|') {
                    if let Some((r, t)) = part.split_once(':') {
                        doc.replicas.push((r.to_string(), t.to_string()));
                    }
                }
            }
            Ok(doc)
        }
        _ => Err(unsupported("SyncText", span)),
    }
}

fn ct_to_counter(v: &CtValue, span: Span) -> Result<JetSyncCounter, Diagnostic> {
    match v {
        CtValue::Struct { type_name, fields }
            if type_name == "SyncCounter" || type_name == "JetSyncCounter" =>
        {
            let value = fields
                .iter()
                .find(|(n, _)| n == "value")
                .and_then(|(_, v)| match v {
                    CtValue::Int(n) => Some(*n),
                    _ => None,
                })
                .unwrap_or(0);
            Ok(JetSyncCounter {
                counts: vec![("ambient".to_string(), value)],
            })
        }
        _ => Err(unsupported("SyncCounter", span)),
    }
}

fn ct_to_map(v: &CtValue, span: Span) -> Result<JetSyncMap, Diagnostic> {
    match v {
        CtValue::Struct { type_name, fields }
            if type_name == "SyncMap" || type_name == "JetSyncMap" =>
        {
            let show = fields
                .iter()
                .find(|(n, _)| n == "show")
                .and_then(|(_, v)| match v {
                    CtValue::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let body = show
                .strip_prefix("SyncMap(")
                .and_then(|s| s.strip_suffix(')'))
                .unwrap_or("");
            let mut map = JetSyncMap {
                entries: Vec::new(),
            };
            if !body.is_empty() {
                for part in body.split(',') {
                    if let Some((k, val)) = part.split_once('=') {
                        map.entries.push((k.to_string(), val.to_string()));
                    }
                }
            }
            Ok(map)
        }
        _ => Err(unsupported("SyncMap", span)),
    }
}

fn ct_to_list(v: &CtValue, span: Span) -> Result<JetSyncList, Diagnostic> {
    match v {
        CtValue::Struct { type_name, fields }
            if type_name == "SyncList" || type_name == "JetSyncList" =>
        {
            let show = fields
                .iter()
                .find(|(n, _)| n == "show")
                .and_then(|(_, v)| match v {
                    CtValue::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let body = show
                .strip_prefix("SyncList(")
                .and_then(|s| s.strip_suffix(')'))
                .unwrap_or("");
            let mut list = JetSyncList { items: Vec::new() };
            if !body.is_empty() {
                for part in body.split('|') {
                    if let Some((r, i)) = part.split_once(':') {
                        list.items.push((r.to_string(), i.to_string()));
                    }
                }
            }
            Ok(list)
        }
        _ => Err(unsupported("SyncList", span)),
    }
}

fn ct_to_policy(v: &CtValue, span: Span) -> Result<JetRowPolicy, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
        return Err(unsupported("RowPolicy", span));
    };
    if type_name != "RowPolicy" && type_name != "JetRowPolicy" {
        return Err(unsupported("RowPolicy", span));
    }
    let table = fields
        .iter()
        .find(|(n, _)| n == "table")
        .and_then(|(_, v)| match v {
            CtValue::Str(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| unsupported("table", span))?;
    let expression = fields
        .iter()
        .find(|(n, _)| n == "expression")
        .and_then(|(_, v)| match v {
            CtValue::Str(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| unsupported("expression", span))?;
    Ok(JetRowPolicy { table, expression })
}

pub fn apply(method: &str, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let one = |i: usize| {
        args.get(i)
            .ok_or_else(|| unsupported(&format!("core.sync.{method} arg {i}"), span))
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
        "text_new" => Ok(text_to_ct(&jet_sync_text_new(as_str(0)?, as_str(1)?))),
        "text_set" => Ok(text_to_ct(&jet_sync_text_set(
            ct_to_text(one(0)?, span)?,
            as_str(1)?,
            as_str(2)?,
        ))),
        "text_merge" => Ok(text_to_ct(&jet_sync_text_merge(
            &ct_to_text(one(0)?, span)?,
            &ct_to_text(one(1)?, span)?,
        ))),
        "text_show" => Ok(CtValue::Str(jet_sync_text_show(&ct_to_text(
            one(0)?, span,
        )?))),
        "counter_new" => Ok(counter_to_ct(&jet_sync_counter_new(as_str(0)?, as_int(1)?))),
        "counter_inc" => Ok(counter_to_ct(&jet_sync_counter_inc(
            ct_to_counter(one(0)?, span)?,
            as_str(1)?,
            as_int(2)?,
        ))),
        "counter_merge" => Ok(counter_to_ct(&jet_sync_counter_merge(
            &ct_to_counter(one(0)?, span)?,
            &ct_to_counter(one(1)?, span)?,
        ))),
        "counter_value" => Ok(CtValue::Int(jet_sync_counter_value(&ct_to_counter(
            one(0)?, span,
        )?))),
        "map_new" => Ok(map_to_ct(&jet_sync_map_new())),
        "map_set" => Ok(map_to_ct(&jet_sync_map_set(
            ct_to_map(one(0)?, span)?,
            as_str(1)?,
            as_str(2)?,
        ))),
        "map_get" => Ok(match jet_sync_map_get(&ct_to_map(one(0)?, span)?, &as_str(1)?) {
            Some(s) => CtValue::Some(Box::new(CtValue::Str(s))),
            None => CtValue::None(Type::String),
        }),
        "map_merge" => Ok(map_to_ct(&jet_sync_map_merge(
            &ct_to_map(one(0)?, span)?,
            &ct_to_map(one(1)?, span)?,
        ))),
        "map_show" => Ok(CtValue::Str(jet_sync_map_show(&ct_to_map(one(0)?, span)?))),
        "list_new" => Ok(list_to_ct(&jet_sync_list_new())),
        "list_push" => Ok(list_to_ct(&jet_sync_list_push(
            ct_to_list(one(0)?, span)?,
            as_str(1)?,
            as_str(2)?,
        ))),
        "list_merge" => Ok(list_to_ct(&jet_sync_list_merge(
            &ct_to_list(one(0)?, span)?,
            &ct_to_list(one(1)?, span)?,
        ))),
        "list_show" => Ok(CtValue::Str(jet_sync_list_show(&ct_to_list(
            one(0)?, span,
        )?))),
        "policy_new" => Ok(match jet_db_policy_new(as_str(0)?, as_str(1)?) {
            Ok(p) => CtValue::ResOk(Box::new(policy_to_ct(&p))),
            Err(e) => CtValue::ResErr(Box::new(CtValue::Str(e))),
        }),
        "policy_allows" => Ok(CtValue::Bool(jet_db_policy_allows(
            &ct_to_policy(one(0)?, span)?,
            &as_str(1)?,
            &as_str(2)?,
        ))),
        "policy_show" => Ok(CtValue::Str(jet_db_policy_show(&ct_to_policy(
            one(0)?, span,
        )?))),
        "sync_over" => Ok(CtValue::Str(jet_app_sync_over(as_str(0)?, as_str(1)?))),
        _ => Err(unsupported(&format!("`core.sync.{method}()`"), span)),
    }
}

#[allow(dead_code)]
fn _type_anchor() -> Type {
    Type::Named("SyncText".to_string())
}
