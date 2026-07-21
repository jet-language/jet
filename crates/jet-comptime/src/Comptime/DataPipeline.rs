//! Card #392 pass 5: `core.data`'s typed table/lazy pipeline
//! (`table`/`rows`/`series`/`values`/`schema`/`missing_count`/`csv`/`json`/`count`/`lazy`/
//! `lazy_filter`/`lazy_sort_by`/`collect`/`plan`/`filter`/`sort_by`/
//! `group_count`/`group_sum`/`group_mean`) at comptime. Mirrors
//! `Codegen/Prelude/CoreLib/Top/EncodingTraits.rs`'s `jet_data_*` helpers
//! byte-for-byte (R12) — `Table<T>`/`Series<T>`/`LazyFrame<T>` are plain
//! `CtValue::Struct` wrappers (`rows`/`missing`/`plan` or `values`/`missing`)
//! since `CtValue` is already dynamically typed, so (unlike `csv<T>`/`json<T>`) none of
//! these need the call-site type argument at runtime — only ordinary Jet
//! lambdas over rows, applied through the same `call_closure` path
//! `list.map`/`.filter`/`.sort_by` already use.

use std::collections::BTreeMap;

use crate::AST::{CtFloat, Type};
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

fn data_schema_elem_ty(arg_ty: &Type) -> Option<&Type> {
    match arg_ty {
        Type::List(inner) => Some(inner.as_ref()),
        Type::Apply { name, args }
            if matches!(name.as_str(), "Table" | "Series" | "LazyFrame") && args.len() == 1 =>
        {
            Some(&args[0])
        }
        _ => None,
    }
}

fn data_schema_expand_struct(arg_ty: &Type) -> bool {
    !matches!(arg_ty, Type::Apply { name, .. } if name == "Series")
}

fn data_column(name: &str, type_name: &str) -> CtValue {
    ct_struct(
        "DataColumn",
        vec![
            ("name", CtValue::Str(name.to_string())),
            ("type_name", CtValue::Str(type_name.to_string())),
        ],
    )
}

fn list_elem_type_name(
    arg0_ty: Option<&Type>,
    call_ret: Option<&Type>,
    items: &[CtValue],
) -> String {
    if let Some(Type::List(inner)) = arg0_ty {
        return inner.name();
    }
    if let Some(elem) = call_ret.and_then(data_schema_elem_ty) {
        return elem.name();
    }
    items
        .first()
        .map(ct_value_type_name)
        .unwrap_or_else(|| "Unknown".to_string())
}

fn container_elem_type_name(recv: &CtValue, type_name: &str) -> Option<String> {
    match struct_field(recv, type_name, "elem_type") {
        Some(CtValue::Str(name)) if !name.is_empty() && name != "Unknown" => Some(name.clone()),
        _ => None,
    }
}

fn ct_value_type_name(v: &CtValue) -> String {
    match v {
        CtValue::Int(_) => "Int".to_string(),
        CtValue::Float(value) => value.jet_type().name(),
        CtValue::Bool(_) => "Bool".to_string(),
        CtValue::Char(_) => "Char".to_string(),
        CtValue::Str(_) => "String".to_string(),
        CtValue::BigInt(_) => "BigInt".to_string(),
        CtValue::Bytes(_) => "[U8]".to_string(),
        CtValue::List(_) => "List".to_string(),
        CtValue::Map(_) => "Map".to_string(),
        CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } => type_name.clone(),
        CtValue::Some(inner) => format!("{}?", ct_value_type_name(inner)),
        CtValue::None(ty) => format!("{}?", ty.name()),
        CtValue::ResOk(_) | CtValue::ResErr(_) => "Result".to_string(),
        CtValue::Unit => "()".to_string(),
        CtValue::Closure(_) => "Fn".to_string(),
    }
}

impl<'a> Interp<'a> {
    fn materialize_lazy(&mut self, frame: &CtValue, span: Span) -> Result<Vec<CtValue>, Diagnostic> {
        let (rows, _, _) = expect_struct(frame, "LazyFrame", "collect", span)?;
        let mut rows = rows.clone();
        let operations = match struct_field(frame, "LazyFrame", "operations") {
            Some(CtValue::List(operations)) => operations.clone(),
            _ => Vec::new(),
        };
        for operation in operations {
            let CtValue::Struct { type_name, fields } = operation else {
                return Err(unsupported("`data.collect()` found an invalid lazy operation", span));
            };
            if type_name != "DataLazyOperation" {
                return Err(unsupported("`data.collect()` found an invalid lazy operation", span));
            }
            let kind = fields.iter().find(|(name, _)| name == "kind").map(|(_, value)| value);
            let function = fields.iter().find(|(name, _)| name == "function").map(|(_, value)| value);
            match (kind, function) {
                (Some(CtValue::Str(kind)), Some(function)) if kind == "filter" => {
                    let mut out = Vec::new();
                    for row in rows {
                        if super::Builtins::as_bool(
                            &self.call_closure(function, vec![row.clone()], span)?,
                            span,
                        )? {
                            out.push(row);
                        }
                    }
                    rows = out;
                }
                (Some(CtValue::Str(kind)), Some(function)) if kind == "sort_by" => {
                    let mut keyed = Vec::with_capacity(rows.len());
                    for row in rows {
                        let key = as_string(
                            &self.call_closure(function, vec![row.clone()], span)?,
                            span,
                        )?
                        .to_string();
                        keyed.push((key, row));
                    }
                    keyed.sort_by(|a, b| a.0.cmp(&b.0));
                    rows = keyed.into_iter().map(|(_, row)| row).collect();
                }
                _ => return Err(unsupported("`data.collect()` found an invalid lazy operation", span)),
            }
        }
        Ok(rows)
    }

    pub(super) fn eval_data_call(
        &mut self,
        method: &str,
        mut argv: Vec<CtValue>,
        type_args: &[Type],
        arg0_ty: Option<&Type>,
        call_ret: Option<&Type>,
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
            "json" => {
                let Some(ty) = type_args.first() else {
                    return Err(unsupported("`data.json<T>()` needs a type argument", span));
                };
                let text = match argv.first() {
                    Some(CtValue::Str(s)) => s.clone(),
                    _ => return Err(unsupported("`data.json()`: expected a string argument", span)),
                };
                // Array-of-objects → `[T]`, same Decode model as `encoding.json.decode<[T]>`.
                self.eval_typed_decode(
                    "core.encoding.json",
                    "decode",
                    &text,
                    &Type::List(Box::new(ty.clone())),
                    span,
                )
            }
            "count" => {
                let recv = argv.first().ok_or_else(|| unsupported("`data.count()`: missing argument", span))?;
                match recv {
                    CtValue::List(xs) => Ok(CtValue::Int(xs.len() as i64)),
                    CtValue::Struct { type_name, .. } if type_name == "Table" || type_name == "LazyFrame" => {
                        let count = if type_name == "LazyFrame" {
                            self.materialize_lazy(recv, span)?.len()
                        } else {
                            expect_struct(recv, type_name, "count", span)?.0.len()
                        };
                        Ok(CtValue::Int(count as i64))
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
                let elem_type = list_elem_type_name(arg0_ty, call_ret, &rows);
                Ok(ct_struct(
                    "Table",
                    vec![
                        ("rows", CtValue::List(rows)),
                        ("missing", CtValue::Int(0)),
                        ("plan", CtValue::List(vec![CtValue::Str("table".to_string())])),
                        ("elem_type", CtValue::Str(elem_type)),
                    ],
                ))
            }
            "rows" => {
                let (rows, ..) = expect_struct(&argv[0], "Table", "rows", span)?;
                Ok(CtValue::List(rows.clone()))
            }
            "series" => {
                let values = expect_list(&argv[0], "series", span)?.clone();
                let elem_type = list_elem_type_name(arg0_ty, call_ret, &values);
                Ok(ct_struct(
                    "Series",
                    vec![
                        ("values", CtValue::List(values)),
                        ("missing", CtValue::Int(0)),
                        ("elem_type", CtValue::Str(elem_type)),
                    ],
                ))
            }
            "values" => {
                let (values, ..) = expect_struct(&argv[0], "Series", "values", span)?;
                Ok(CtValue::List(values.clone()))
            }
            "schema" => {
                let recv = argv.first().ok_or_else(|| {
                    unsupported("`data.schema()`: missing argument", span)
                })?;
                let expand = match recv {
                    CtValue::Struct { type_name, .. } if type_name == "Series" => false,
                    _ => arg0_ty.map(data_schema_expand_struct).unwrap_or(true),
                };
                let sample = match recv {
                    CtValue::List(xs) => xs.first(),
                    CtValue::Struct { type_name, .. }
                        if type_name == "Table" || type_name == "LazyFrame" =>
                    {
                        // Schema is the row model, not deferred filter results — read
                        // the stored source rows without materializing lazy ops.
                        expect_struct(recv, type_name, "schema", span)?.0.first()
                    }
                    CtValue::Struct { type_name, .. } if type_name == "Series" => {
                        expect_struct(recv, "Series", "schema", span)?.0.first()
                    }
                    _ => {
                        return Err(unsupported(
                            "`data.schema()` needs a typed table or series",
                            span,
                        ));
                    }
                };
                let columns = match sample {
                    Some(value) if !expand => {
                        // Series law: one `value` column even when the element is a struct.
                        vec![data_column("value", &ct_value_type_name(value))]
                    }
                    Some(CtValue::Struct { type_name, fields }) => {
                        if let Some(def) = self.structs.get(type_name.as_str()) {
                            def.fields
                                .iter()
                                .map(|f| data_column(&f.name, &f.ty.name()))
                                .collect()
                        } else {
                            // ponytail: fall back to runtime field names when the
                            // struct def is out of the comptime registry.
                            fields
                                .iter()
                                .map(|(name, value)| data_column(name, &ct_value_type_name(value)))
                                .collect()
                        }
                    }
                    Some(value) => vec![data_column("value", &ct_value_type_name(value))],
                    // Empty containers: match AOT — schema is type-driven.
                    None => {
                        let from_arg = arg0_ty.and_then(data_schema_elem_ty);
                        let from_stored = match recv {
                            CtValue::Struct { type_name, .. }
                                if type_name == "Table"
                                    || type_name == "Series"
                                    || type_name == "LazyFrame" =>
                            {
                                container_elem_type_name(recv, type_name)
                            }
                            _ => None,
                        };
                        match from_arg {
                            Some(elem) if !expand => {
                                vec![data_column("value", &elem.name())]
                            }
                            Some(Type::Named(struct_name)) => {
                                if let Some(def) = self.structs.get(struct_name.as_str()) {
                                    def.fields
                                        .iter()
                                        .map(|f| data_column(&f.name, &f.ty.name()))
                                        .collect()
                                } else {
                                    vec![data_column("value", struct_name)]
                                }
                            }
                            Some(elem) => vec![data_column("value", &elem.name())],
                            None => match from_stored {
                                Some(name) if !expand => {
                                    vec![data_column("value", &name)]
                                }
                                Some(name) => {
                                    if let Some(def) = self.structs.get(name.as_str()) {
                                        def.fields
                                            .iter()
                                            .map(|f| data_column(&f.name, &f.ty.name()))
                                            .collect()
                                    } else {
                                        vec![data_column("value", &name)]
                                    }
                                }
                                None => Vec::new(),
                            },
                        }
                    }
                };
                Ok(CtValue::List(columns))
            }
            "missing_count" => {
                let (values, missing, _) = expect_struct(&argv[0], "Series", "missing_count", span)?;
                let none_count = values.iter().filter(|v| matches!(v, CtValue::None(_))).count() as i64;
                Ok(CtValue::Int(missing + none_count))
            }
            "lazy" => {
                let (rows, missing, plan) = expect_struct(&argv[0], "Table", "lazy", span)?;
                let elem_type = container_elem_type_name(&argv[0], "Table")
                    .or_else(|| arg0_ty.and_then(data_schema_elem_ty).map(|t| t.name()))
                    .unwrap_or_else(|| "Unknown".to_string());
                Ok(ct_struct(
                    "LazyFrame",
                    vec![
                        ("rows", CtValue::List(rows.clone())),
                        ("missing", CtValue::Int(missing)),
                        ("plan", CtValue::List(plan.cloned().unwrap_or_default())),
                        ("operations", CtValue::List(Vec::new())),
                        ("elem_type", CtValue::Str(elem_type)),
                    ],
                ))
            }
            "lazy_filter" | "lazy_sort_by" => {
                let f = argv.pop().unwrap();
                let (rows, missing, plan) = expect_struct(&argv[0], "LazyFrame", method, span)?;
                let mut plan = plan.cloned().unwrap_or_default();
                let mut operations = match struct_field(&argv[0], "LazyFrame", "operations") {
                    Some(CtValue::List(operations)) => operations.clone(),
                    _ => Vec::new(),
                };
                let kind = if method == "lazy_filter" {
                    plan.push(CtValue::Str("filter".to_string()));
                    "filter"
                } else {
                    plan.push(CtValue::Str("sort_by".to_string()));
                    "sort_by"
                };
                operations.push(ct_struct(
                    "DataLazyOperation",
                    vec![("kind", CtValue::Str(kind.to_string())), ("function", f)],
                ));
                let elem_type = container_elem_type_name(&argv[0], "LazyFrame")
                    .unwrap_or_else(|| "Unknown".to_string());
                Ok(ct_struct(
                    "LazyFrame",
                    vec![
                        ("rows", CtValue::List(rows.clone())),
                        ("missing", CtValue::Int(missing)),
                        ("plan", CtValue::List(plan)),
                        ("operations", CtValue::List(operations)),
                        ("elem_type", CtValue::Str(elem_type)),
                    ],
                ))
            }
            "collect" => {
                let (_, missing, plan) = expect_struct(&argv[0], "LazyFrame", "collect", span)?;
                let rows = self.materialize_lazy(&argv[0], span)?;
                let mut plan = plan.cloned().unwrap_or_default();
                plan.push(CtValue::Str("collect".to_string()));
                let elem_type = container_elem_type_name(&argv[0], "LazyFrame")
                    .unwrap_or_else(|| list_elem_type_name(None, None, &rows));
                Ok(ct_struct(
                    "Table",
                    vec![
                        ("rows", CtValue::List(rows)),
                        ("missing", CtValue::Int(missing)),
                        ("plan", CtValue::List(plan)),
                        ("elem_type", CtValue::Str(elem_type)),
                    ],
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
                                ("sum", CtValue::Float(CtFloat::f64(sum))),
                                ("mean", CtValue::Float(CtFloat::f64(mean))),
                            ],
                        )
                    })
                    .collect();
                Ok(CtValue::List(out))
            }
            "inner_join" | "left_join" => {
                let right_key = argv.pop().unwrap();
                let left_key = argv.pop().unwrap();
                let right = expect_list(&argv[1], method, span)?.clone();
                let left = expect_list(&argv[0], method, span)?.clone();
                let mut right_rows = BTreeMap::<String, Vec<CtValue>>::new();
                for row in right {
                    let key = as_string(
                        &self.call_closure(&right_key, vec![row.clone()], span)?,
                        span,
                    )?
                    .to_string();
                    right_rows.entry(key).or_default().push(row);
                }
                let mut joined = Vec::new();
                for left_row in left {
                    let key = as_string(
                        &self.call_closure(&left_key, vec![left_row.clone()], span)?,
                        span,
                    )?
                    .to_string();
                    match right_rows.get(&key) {
                        Some(matches) => {
                            for right_row in matches {
                                joined.push(ct_struct(
                                    "DataJoin",
                                    vec![
                                        ("left", left_row.clone()),
                                        (
                                            "right",
                                            if method == "left_join" {
                                                CtValue::Some(Box::new(right_row.clone()))
                                            } else {
                                                right_row.clone()
                                            },
                                        ),
                                    ],
                                ));
                            }
                        }
                        None if method == "left_join" => joined.push(ct_struct(
                            "DataJoin",
                            vec![("left", left_row), ("right", CtValue::None(Type::Named("Unknown".to_string())))],
                        )),
                        None => {}
                    }
                }
                Ok(CtValue::List(joined))
            }
            _ => Err(unsupported(&format!("`data.{}()` at comptime", method), span)),
        }
    }
}
