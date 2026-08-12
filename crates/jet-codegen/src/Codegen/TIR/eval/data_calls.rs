//! D-DATAFLOW1=A: `core.data` on the canonical TIR evaluator (#777/#778 deopt).
//!
//! Default `jet run` deopts data-heavy `run` bodies to this path. Keep values,
//! ordering, and typed `DataError` / `[FieldError]` results aligned with AOT.

use std::collections::{BTreeMap, HashMap};

use crate::AST::{CtFloat, Type};
use crate::Codegen::TIR::TExpr;
use crate::Comptime::Builtins::as_bool;
use crate::Comptime::{apply_core_call, apply_data_line_call, CtReport, CtValue};
use crate::Diagnostics::{Diagnostic, Span};
use jet_foundation::PackageEdition;

use super::{unsupported, EvalCtx};

fn ok(v: CtValue) -> CtValue {
    CtValue::Present(Box::new(v))
}

fn err(v: CtValue) -> CtValue {
    CtValue::failed(Box::new(v))
}

fn list_elem(ty: &Type) -> Option<&Type> {
    match ty {
        Type::List(inner) => Some(inner.as_ref()),
        Type::Result { ok, .. } => list_elem(ok),
        _ => None,
    }
}

fn as_float_list(v: &CtValue, span: Span) -> Result<Vec<f64>, Diagnostic> {
    match v {
        CtValue::List(xs) => xs
            .iter()
            .map(|x| match x {
                CtValue::Float(f) => Ok(f.as_f64()),
                CtValue::Int(n) => Ok(*n as f64),
                _ => Err(unsupported("core.data: expected `[Float]`", span)),
            })
            .collect(),
        _ => Err(unsupported("core.data: expected `[Float]`", span)),
    }
}

fn ct_struct(type_name: &str, fields: Vec<(&str, CtValue)>) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: fields
            .into_iter()
            .map(|(n, v)| (n.to_string(), v))
            .collect(),
    }
}

fn decode_error(reason: impl Into<String>) -> CtValue {
    CtValue::List(vec![CtValue::Struct {
        type_name: "FieldError".to_string(),
        fields: vec![
            ("path".to_string(), CtValue::Str(String::new())),
            ("reason".to_string(), CtValue::Str(reason.into())),
        ],
    }])
}

fn parse_cell(ty: &Type, cell: &str) -> Result<CtValue, String> {
    match ty {
        Type::String => Ok(CtValue::Str(cell.to_string())),
        Type::Float | Type::Float32 => cell
            .parse::<f64>()
            .map(|f| CtValue::Float(CtFloat::f64(f)))
            .map_err(|_| format!("expected Float, got `{cell}`")),
        Type::Int | Type::IntN { .. } => cell
            .parse::<i64>()
            .map(CtValue::Int)
            .map_err(|_| format!("expected Int, got `{cell}`")),
        Type::Bool => match cell {
            "true" | "True" | "1" => Ok(CtValue::Bool(true)),
            "false" | "False" | "0" => Ok(CtValue::Bool(false)),
            _ => Err(format!("expected Bool, got `{cell}`")),
        },
        Type::Named(n) if n == "String" => Ok(CtValue::Str(cell.to_string())),
        Type::Named(n) if n == "Float" => cell
            .parse::<f64>()
            .map(|f| CtValue::Float(CtFloat::f64(f)))
            .map_err(|_| format!("expected Float, got `{cell}`")),
        Type::Named(n) if n == "Int" => cell
            .parse::<i64>()
            .map(CtValue::Int)
            .map_err(|_| format!("expected Int, got `{cell}`")),
        Type::Named(n) if n == "Bool" => parse_cell(&Type::Bool, cell),
        other => Err(format!("unsupported CSV field type `{other:?}`")),
    }
}

impl<'a> EvalCtx<'a> {
    /// Evaluate `core.data.*` without pre-evaluating lambda arguments.
    pub(super) fn eval_core_data_call(
        &mut self,
        method: &str,
        args: &'a [TExpr],
        call_ty: &Type,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let span = self.span();
        let checked = matches!(call_ty, Type::Result { .. })
            || PackageEdition::package_edition_at_least("2027");

        match method {
            "csv" | "json" => {
                let text = match self.eval_expr(&args[0], scope)? {
                    CtValue::Str(s) => s,
                    _ => return Err(unsupported(&format!("`data.{method}()`: expected string"), span)),
                };
                let elem = list_elem(call_ty).cloned().ok_or_else(|| {
                    unsupported(&format!("`data.{method}<T>()` needs a list result type"), span)
                })?;
                if method == "csv" {
                    Ok(self.decode_csv_rows(&text, &elem, span))
                } else {
                    Ok(self.decode_json_rows(&text, &elem, span))
                }
            }
            "count" => {
                let recv = self.eval_expr(&args[0], scope)?;
                let n = match &recv {
                    CtValue::List(xs) => xs.len() as i64,
                    CtValue::Struct { type_name, fields }
                        if type_name == "Table"
                            || type_name == "LazyFrame"
                            || type_name == "Series" =>
                    {
                        let key = if type_name == "Series" { "values" } else { "rows" };
                        fields
                            .iter()
                            .find(|(n, _)| n == key)
                            .and_then(|(_, v)| match v {
                                CtValue::List(xs) => Some(xs.len() as i64),
                                _ => None,
                            })
                            .unwrap_or(0)
                    }
                    _ => {
                        return Err(unsupported("`data.count()` needs a list/table/series", span));
                    }
                };
                Ok(CtValue::Int(n))
            }
            "status" => apply_core_call("core.data", "status", Vec::new(), span, self.repl_mode),
            "require_bridge" => {
                let provider = match self.eval_expr(&args[0], scope)? {
                    CtValue::Str(s) => s,
                    _ => return Err(unsupported("`data.require_bridge` needs a String", span)),
                };
                apply_core_call(
                    "core.data",
                    "require_bridge",
                    vec![CtValue::Str(provider)],
                    span,
                    self.repl_mode,
                )
            }
            "mean" | "sum" | "min" | "max" | "median" | "variance" | "stddev" => {
                let values = as_float_list(&self.eval_expr(&args[0], scope)?, span)?;
                self.eval_stat(method, &values)
            }
            "quantile" => {
                let values = as_float_list(&self.eval_expr(&args[0], scope)?, span)?;
                let q = match self.eval_expr(&args[1], scope)? {
                    CtValue::Float(f) => f.as_f64(),
                    CtValue::Int(n) => n as f64,
                    _ => return Err(unsupported("quantile q", span)),
                };
                self.eval_quantile(&values, q)
            }
            "rolling_mean" => {
                let values = as_float_list(&self.eval_expr(&args[0], scope)?, span)?;
                let width = match self.eval_expr(&args[1], scope)? {
                    CtValue::Int(width) => width,
                    _ => return Err(unsupported("rolling_mean width", span)),
                };
                apply_core_call(
                    "core.data",
                    "rolling_mean",
                    vec![
                        CtValue::List(
                            values
                                .into_iter()
                                .map(|value| CtValue::Float(CtFloat::f64(value)))
                                .collect(),
                        ),
                        CtValue::Int(width),
                    ],
                    span,
                    self.repl_mode,
                )
            }
            "group_count" | "group_sum" | "group_mean" => {
                let rows = match self.eval_expr(&args[0], scope)? {
                    CtValue::List(xs) => xs,
                    _ => return Err(unsupported(&format!("`data.{method}` needs a list"), span)),
                };
                let key_f = &args[1];
                let value_f = if method == "group_count" {
                    None
                } else {
                    Some(&args[2])
                };
                let mut key_callable = None;
                let mut value_callable = None;
                let mut groups: BTreeMap<String, (i64, f64)> = BTreeMap::new();
                for row in &rows {
                    let key = match self.apply_callable_once(
                        key_f,
                        &mut key_callable,
                        vec![row.clone()],
                        scope,
                    )? {
                        CtValue::Str(s) => s,
                        other => {
                            return Err(unsupported(
                                &format!("group key must be String, got {other:?}"),
                                span,
                            ))
                        }
                    };
                    let value = if let Some(vf) = value_f {
                        match self.apply_callable_once(
                            vf,
                            &mut value_callable,
                            vec![row.clone()],
                            scope,
                        )? {
                            CtValue::Float(f) => f.as_f64(),
                            CtValue::Int(n) => n as f64,
                            _ => return Err(unsupported("group value must be Float", span)),
                        }
                    } else {
                        0.0
                    };
                    let entry = groups.entry(key).or_insert((0, 0.0));
                    entry.0 += 1;
                    if value_f.is_some() {
                        entry.1 += value;
                    }
                }
                let out: Vec<CtValue> = groups
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
                if checked {
                    Ok(ok(CtValue::List(out)))
                } else {
                    Ok(CtValue::List(out))
                }
            }
            "filter" => {
                let rows = match self.eval_expr(&args[0], scope)? {
                    CtValue::List(xs) => xs,
                    _ => return Err(unsupported("`data.filter` needs a list", span)),
                };
                let pred = &args[1];
                let mut pred_callable = None;
                let mut out = Vec::new();
                for row in rows {
                    if as_bool(
                        &self.apply_callable_once(
                            pred,
                            &mut pred_callable,
                            vec![row.clone()],
                            scope,
                        )?,
                        span,
                    )? {
                        out.push(row);
                    }
                }
                Ok(CtValue::List(out))
            }
            "pivot_sum" => {
                let rows = match self.eval_expr(&args[0], scope)? {
                    CtValue::List(xs) => xs,
                    _ => return Err(unsupported("`data.pivot_sum` needs a list", span)),
                };
                let row_key = &args[1];
                let col_key = &args[2];
                let value_f = &args[3];
                let mut row_key_callable = None;
                let mut col_key_callable = None;
                let mut value_callable = None;
                let mut groups: BTreeMap<String, (i64, f64, String, String)> = BTreeMap::new();
                for row in &rows {
                    let left = self.apply_callable_once(
                        row_key,
                        &mut row_key_callable,
                        vec![row.clone()],
                        scope,
                    )?;
                    let right = self.apply_callable_once(
                        col_key,
                        &mut col_key_callable,
                        vec![row.clone()],
                        scope,
                    )?;
                    let left_s = match &left {
                        CtValue::Str(s) => s.clone(),
                        other => other.jet_show(),
                    };
                    let right_s = match &right {
                        CtValue::Str(s) => s.clone(),
                        other => other.jet_show(),
                    };
                    let key = format!("{left_s}|{right_s}");
                    let amount = match self.apply_callable_once(
                        value_f,
                        &mut value_callable,
                        vec![row.clone()],
                        scope,
                    )? {
                        CtValue::Float(f) => f.as_f64(),
                        CtValue::Int(n) => n as f64,
                        _ => {
                            return Err(unsupported(
                                "`data.pivot_sum` value closure must return Float",
                                span,
                            ));
                        }
                    };
                    let entry = groups
                        .entry(key)
                        .or_insert((0, 0.0, left_s, right_s));
                    entry.0 += 1;
                    entry.1 += amount;
                }
                let cell_ty = if checked { "DataPivotCell" } else { "DataGroup" };
                let out: Vec<CtValue> = groups
                    .into_iter()
                    .map(|(_, (count, sum, row, column))| {
                        let mean = if count == 0 {
                            0.0
                        } else {
                            sum / count as f64
                        };
                        // Always expose row_key/column_key — AOT DataPivotCell and
                        // the parity harness read those fields (not only `key`).
                        ct_struct(
                            cell_ty,
                            vec![
                                ("row_key", CtValue::Str(row.clone())),
                                ("column_key", CtValue::Str(column.clone())),
                                ("key", CtValue::Str(format!("{row}|{column}"))),
                                ("count", CtValue::Int(count)),
                                ("sum", CtValue::Float(CtFloat::f64(sum))),
                                ("mean", CtValue::Float(CtFloat::f64(mean))),
                            ],
                        )
                    })
                    .collect();
                if checked {
                    Ok(ok(CtValue::List(out)))
                } else {
                    Ok(CtValue::List(out))
                }
            }
            "bar_text" | "bar_svg" | "line_text" | "line_svg" => {
                let groups = self.eval_expr(&args[0], scope)?;
                let mut call_args = vec![groups];
                if args.len() > 1 {
                    call_args.push(self.eval_expr(&args[1], scope)?);
                }
                if matches!(method, "line_text" | "line_svg") {
                    apply_data_line_call(method, call_args, span)
                } else {
                    apply_core_call("core.data", method, call_args, span, self.repl_mode)
                }
            }
            "series" => {
                let values = self.eval_expr(&args[0], scope)?;
                Ok(ct_struct(
                    "Series",
                    vec![("values", values), ("missing", CtValue::Int(0))],
                ))
            }
            "values" => {
                let series = self.eval_expr(&args[0], scope)?;
                match series {
                    CtValue::Struct { type_name, fields }
                        if type_name == "Series" || type_name == "DataSeries" => fields
                            .into_iter()
                            .find_map(|(name, value)| (name == "values").then_some(value))
                            .ok_or_else(|| unsupported("`data.values()` needs a Series", span)),
                    _ => Err(unsupported("`data.values()` needs a Series", span)),
                }
            }
            "missing_count" => {
                let series = self.eval_expr(&args[0], scope)?;
                let CtValue::Struct { type_name, fields } = series else {
                    return Err(unsupported("`data.missing_count()` needs a Series", span));
                };
                if type_name != "Series" && type_name != "DataSeries" {
                    return Err(unsupported("`data.missing_count()` needs a Series", span));
                }
                let mut missing = fields
                    .iter()
                    .find_map(|(name, value)| {
                        (name == "missing").then(|| match value {
                            CtValue::Int(count) => *count,
                            _ => 0,
                        })
                    })
                    .unwrap_or(0);
                if let Some(CtValue::List(values)) = fields
                    .iter()
                    .find_map(|(name, value)| (name == "values").then_some(value))
                {
                    missing += values
                        .iter()
                        .filter(|value| matches!(value, CtValue::Failed(CtReport::Clean(_))))
                        .count() as i64;
                }
                Ok(CtValue::Int(missing))
            }
            "table" | "rows" | "schema" | "lazy" | "plan" | "lazy_filter"
            | "lazy_sort_by" | "collect" | "sort_by" | "inner_join" | "left_join"
            | "describe" | "csv_reader" | "json_reader" => Err(unsupported(
                &format!("`core.data.{method}()` at comptime (impure tier)"),
                span,
            )),
            _ => {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                if self.impure_depth > 0 {
                    apply_core_call("core.data", method, argv, span, self.repl_mode)
                } else {
                    apply_core_call("core.data", method, argv, span, self.repl_mode)
                }
            }
        }
    }

    /// #1657 / I9: one hop to the shared `core.data` entry, which runs the one
    /// Prelude kernel. The deopt tier decides nothing about empty input,
    /// finiteness, or overflow.
    fn eval_stat(&self, method: &str, values: &[f64]) -> Result<CtValue, Diagnostic> {
        apply_core_call(
            "core.data",
            method,
            vec![CtValue::List(
                values
                    .iter()
                    .copied()
                    .map(|f| CtValue::Float(CtFloat::f64(f)))
                    .collect(),
            )],
            self.span(),
            self.repl_mode,
        )
    }

    fn eval_quantile(&self, values: &[f64], q: f64) -> Result<CtValue, Diagnostic> {
        apply_core_call(
            "core.data",
            "quantile",
            vec![
                CtValue::List(
                    values
                        .iter()
                        .copied()
                        .map(|f| CtValue::Float(CtFloat::f64(f)))
                        .collect(),
                ),
                CtValue::Float(CtFloat::f64(q)),
            ],
            self.span(),
            self.repl_mode,
        )
    }

    fn decode_csv_rows(&self, text: &str, elem_ty: &Type, span: Span) -> CtValue {
        let rows = match crate::Comptime::runtime_csv_parse(text) {
            Ok(r) => r,
            Err(e) => return err(decode_error(e)),
        };
        let mut it = rows.into_iter();
        let Some(header) = it.next() else {
            return ok(CtValue::List(Vec::new()));
        };
        let Type::Named(type_name) = elem_ty else {
            return err(decode_error(format!(
                "data.csv currently decodes named structs on the JIT path, got {elem_ty:?}"
            )));
        };
        let fields = match self.struct_field_types.get(type_name) {
            Some(f) => f,
            None => {
                return err(decode_error(format!(
                    "unknown Codable type `{type_name}` for data.csv"
                )));
            }
        };
        let mut values = Vec::new();
        for (i, row) in it.enumerate() {
            let mut out_fields = Vec::with_capacity(fields.len());
            for (fname, fty) in fields {
                let col = header.iter().position(|h| h == fname);
                let cell = col
                    .and_then(|c| row.get(c))
                    .cloned()
                    .unwrap_or_default();
                match parse_cell(fty, &cell) {
                    Ok(v) => out_fields.push((fname.clone(), v)),
                    Err(reason) => {
                        return err(decode_error(format!("row {}: {reason}", i + 1)));
                    }
                }
            }
            values.push(CtValue::Struct {
                type_name: type_name.clone(),
                fields: out_fields,
            });
        }
        let _ = span;
        ok(CtValue::List(values))
    }

    fn decode_json_rows(&self, text: &str, elem_ty: &Type, span: Span) -> CtValue {
        let _ = (self, text, elem_ty, span);
        err(decode_error(
            "data.json on the JIT interpreter path is not wired yet; use `jet run --profile=debug`",
        ))
    }
}
