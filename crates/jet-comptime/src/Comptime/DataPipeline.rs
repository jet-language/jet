//! Card #392 pass 5: `core.data`'s typed table/lazy pipeline
//! (`table`/`rows`/`series`/`values`/`missing_count`/`csv`/`count`/`lazy`/
//! `lazy_filter`/`lazy_sort_by`/`collect`/`plan`/`filter`/`sort_by`/
//! `group_count`/`group_sum`/`group_mean`) at comptime. Mirrors
//! `Codegen/Prelude/CoreLib/Top/EncodingTraits.rs`'s `jet_data_*` helpers
//! byte-for-byte (R12) — `Table<T>`/`Series<T>`/`LazyFrame<T>` are plain
//! `CtValue::Struct` wrappers (`rows`/`missing`/`plan` or `values`/`missing`)
//! since `CtValue` is already dynamically typed, so (unlike `csv<T>`) none of
//! these need the call-site type argument at runtime — only ordinary Jet
//! lambdas over rows, applied through the same `call_closure` path
//! `list.map`/`.filter`/`.sort_by` already use.

use std::collections::BTreeMap;

use crate::AST::Type;
use crate::Diagnostics::{Diagnostic, Span};

use super::Diagnostics::unsupported;
use super::Interpreter::Interp;
use super::Methods::{as_float, as_string};
use super::Value::CtValue;

fn ct_struct(type_name: &str, fields: Vec<(&str, CtValue)>) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: fields.into_iter().map(|(n, v)| (n.to_string(), v)).collect(),
    }
}
fn struct_field<'a>(v: &'a CtValue, type_name: &str, field: &str) -> Option<&'a CtValue> {
    match v {
        CtValue::Struct { type_name: t, fields } if t == type_name => {
            fields.iter().find(|(n, _)| n == field).map(|(_, v)| v)
        }
        _ => None,
    }
}
fn expect_list<'a>(v: &'a CtValue, what: &str, span: Span) -> Result<&'a Vec<CtValue>, Diagnostic> {
    match v {
        CtValue::List(xs) => Ok(xs),
        _ => Err(unsupported(&format!("`data.{}` needs a list-backed value", what), span)),
    }
}
fn expect_struct<'a>(v: &'a CtValue, type_name: &str, what: &str, span: Span) -> Result<(&'a Vec<CtValue>, i64, Option<&'a Vec<CtValue>>), Diagnostic> {
    let field_key = if type_name == "Series" { "values" } else { "rows" };
    match struct_field(v, type_name, field_key) {
        Some(CtValue::List(xs)) => {
            let missing = match struct_field(v, type_name, "missing") {
                Some(CtValue::Int(n)) => *n,
                _ => 0,
            };
            let plan = struct_field(v, type_name, "plan").and_then(|p| match p {
                CtValue::List(ps) => Some(ps),
                _ => None,
            });
            Ok((xs, missing, plan))
        }
        _ => Err(unsupported(&format!("`data.{}` needs a `{}` value", what, type_name), span)),
    }
}

impl<'a> Interp<'a> {
    pub(super) fn eval_data_call(
        &mut self,
        method: &str,
        mut argv: Vec<CtValue>,
        type_args: &[Type],
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        match method {
            "csv" => {
                let Some(ty) = type_args.first() else {
                    return Err(unsupported("`data.csv<T>()` needs a type argument", span));
                };
                let text = match argv.first() {
                    Some(CtValue::Str(s)) => s.clone(),
                    _ => return Err(unsupported("`data.csv()`: expected a string argument", span)),
                };
                self.eval_typed_csv_decode("decode", &text, ty, span)
            }
            "count" => {
                let recv = argv.first().ok_or_else(|| unsupported("`data.count()`: missing argument", span))?;
                match recv {
                    CtValue::List(xs) => Ok(CtValue::Int(xs.len() as i64)),
                    CtValue::Struct { type_name, .. } if type_name == "Table" || type_name == "LazyFrame" => {
                        let (rows, ..) = expect_struct(recv, type_name, "count", span)?;
                        Ok(CtValue::Int(rows.len() as i64))
                    }
                    CtValue::Struct { type_name, .. } if type_name == "Series" => {
                        let (values, ..) = expect_struct(recv, "Series", "count", span)?;
                        Ok(CtValue::Int(values.len() as i64))
                    }
                    _ => Err(unsupported("`data.count()` needs a typed table or series", span)),
                }
            }
            "table" => {
                let rows = expect_list(&argv[0], "table", span)?.clone();
                Ok(ct_struct(
                    "Table",
                    vec![
                        ("rows", CtValue::List(rows)),
                        ("missing", CtValue::Int(0)),
                        ("plan", CtValue::List(vec![CtValue::Str("table".to_string())])),
                    ],
                ))
            }
            "rows" => {
                let (rows, ..) = expect_struct(&argv[0], "Table", "rows", span)?;
                Ok(CtValue::List(rows.clone()))
            }
            "series" => {
                let values = expect_list(&argv[0], "series", span)?.clone();
                Ok(ct_struct("Series", vec![("values", CtValue::List(values)), ("missing", CtValue::Int(0))]))
            }
            "values" => {
                let (values, ..) = expect_struct(&argv[0], "Series", "values", span)?;
                Ok(CtValue::List(values.clone()))
            }
            "missing_count" => {
                let (values, missing, _) = expect_struct(&argv[0], "Series", "missing_count", span)?;
                let none_count = values.iter().filter(|v| matches!(v, CtValue::None(_))).count() as i64;
                Ok(CtValue::Int(missing + none_count))
            }
            "lazy" => {
                let (rows, missing, plan) = expect_struct(&argv[0], "Table", "lazy", span)?;
                Ok(ct_struct(
                    "LazyFrame",
                    vec![
                        ("rows", CtValue::List(rows.clone())),
                        ("missing", CtValue::Int(missing)),
                        ("plan", CtValue::List(plan.cloned().unwrap_or_default())),
                    ],
                ))
            }
            "lazy_filter" | "lazy_sort_by" => {
                let f = argv.pop().unwrap();
                let (rows, missing, plan) = expect_struct(&argv[0], "LazyFrame", method, span)?;
                let mut plan = plan.cloned().unwrap_or_default();
                let new_rows = if method == "lazy_filter" {
                    let mut out = Vec::new();
                    for row in rows {
                        if super::Builtins::as_bool(&self.call_closure(&f, vec![row.clone()], span)?, span)? {
                            out.push(row.clone());
                        }
                    }
                    plan.push(CtValue::Str("filter".to_string()));
                    out
                } else {
                    let mut keyed = Vec::with_capacity(rows.len());
                    for row in rows {
                        let k = as_string(&self.call_closure(&f, vec![row.clone()], span)?, span)?.to_string();
                        keyed.push((k, row.clone()));
                    }
                    keyed.sort_by(|a, b| a.0.cmp(&b.0));
                    plan.push(CtValue::Str("sort_by".to_string()));
                    keyed.into_iter().map(|(_, r)| r).collect()
                };
                Ok(ct_struct(
                    "LazyFrame",
                    vec![("rows", CtValue::List(new_rows)), ("missing", CtValue::Int(missing)), ("plan", CtValue::List(plan))],
                ))
            }
            "collect" => {
                let (rows, missing, plan) = expect_struct(&argv[0], "LazyFrame", "collect", span)?;
                let mut plan = plan.cloned().unwrap_or_default();
                plan.push(CtValue::Str("collect".to_string()));
                Ok(ct_struct(
                    "Table",
                    vec![("rows", CtValue::List(rows.clone())), ("missing", CtValue::Int(missing)), ("plan", CtValue::List(plan))],
                ))
            }
            "plan" => {
                let (_, _, plan) = expect_struct(&argv[0], "LazyFrame", "plan", span)?;
                Ok(CtValue::List(plan.cloned().unwrap_or_default()))
            }
            "filter" | "sort_by" => {
                let f = argv.pop().unwrap();
                let rows = expect_list(&argv[0], method, span)?.clone();
                if method == "filter" {
                    let mut out = Vec::new();
                    for row in &rows {
                        if super::Builtins::as_bool(&self.call_closure(&f, vec![row.clone()], span)?, span)? {
                            out.push(row.clone());
                        }
                    }
                    Ok(CtValue::List(out))
                } else {
                    let mut keyed = Vec::with_capacity(rows.len());
                    for row in &rows {
                        let k = as_string(&self.call_closure(&f, vec![row.clone()], span)?, span)?.to_string();
                        keyed.push((k, row.clone()));
                    }
                    keyed.sort_by(|a, b| a.0.cmp(&b.0));
                    Ok(CtValue::List(keyed.into_iter().map(|(_, r)| r).collect()))
                }
            }
            "group_count" | "group_sum" | "group_mean" => {
                let value_f = if method == "group_count" { None } else { Some(argv.pop().unwrap()) };
                let key_f = argv.pop().unwrap();
                let rows = expect_list(&argv[0], method, span)?.clone();
                let mut groups: BTreeMap<String, (i64, f64)> = BTreeMap::new();
                for row in &rows {
                    let key = as_string(&self.call_closure(&key_f, vec![row.clone()], span)?, span)?.to_string();
                    let value = match &value_f {
                        Some(f) => as_float(&self.call_closure(f, vec![row.clone()], span)?, span)?,
                        None => 0.0,
                    };
                    let entry = groups.entry(key).or_insert((0, 0.0));
                    entry.0 += 1;
                    if value_f.is_some() {
                        entry.1 += value;
                    }
                }
                let out = groups
                    .into_iter()
                    .map(|(key, (count, sum))| {
                        let mean = if count == 0 { 0.0 } else { sum / count as f64 };
                        ct_struct(
                            "DataGroup",
                            vec![
                                ("key", CtValue::Str(key)),
                                ("count", CtValue::Int(count)),
                                ("sum", CtValue::Float(sum)),
                                ("mean", CtValue::Float(mean)),
                            ],
                        )
                    })
                    .collect();
                Ok(CtValue::List(out))
            }
            _ => Err(unsupported(&format!("`data.{}()` at comptime", method), span)),
        }
    }
}
