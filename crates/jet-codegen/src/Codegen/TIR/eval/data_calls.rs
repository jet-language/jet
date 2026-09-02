//! D-DATAFLOW1=A: `core.data` on the canonical TIR evaluator (#777/#778 deopt).
//!
//! Default `jet run` deopts data-heavy `run` bodies to this path. Keep values,
//! ordering, and typed `DataError` / `[FieldError]` results aligned with AOT.

use std::collections::{BTreeMap, HashMap};

use crate::Codegen::TIR::TExpr;
use crate::Comptime::Builtins::as_bool;
use crate::Comptime::{
    apply_core_call, apply_core_call_with_type, apply_data_line_call, CtReport, CtValue,
    DataPipeline,
};
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtFloat, Type};
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
fn game_struct(type_name: &str, fields: Vec<(&str, CtValue)>) -> CtValue {
    ct_struct(type_name, fields)
}

#[allow(dead_code)]
mod game_kernel {
    trait JetShow {
        fn jet_show(&self) -> String;
    }

    trait JetDebug {
        fn jet_debug(&self) -> String;
    }

    include!("../../../Prelude/CoreLib/Top/Game.rs");

    pub(crate) fn run_erased(
        name: String,
        assets: Vec<(String, String)>,
        bindings: Vec<(String, String)>,
        components: Vec<String>,
        replay_path: Option<String>,
        backend: Option<(String, String, String, Option<i64>)>,
        callback_count: usize,
    ) -> String {
        let mut scene = jet_game_scene_new(&name);
        for (kind, path) in assets {
            match kind.as_str() {
                "image" => {
                    let _ = jet_game_assets_image(&scene.assets, &path);
                }
                "sound" => {
                    let _ = jet_game_assets_sound(&scene.assets, &path);
                }
                _ => {}
            }
        }
        for (action, key) in bindings {
            jet_game_input_bind(&scene.input, &action, &key);
        }
        for component in components {
            jet_game_scene_component(&mut scene, &component);
        }
        for _ in 0..callback_count {
            jet_game_scene_on_frame(&mut scene, Box::new(|_| {}));
        }
        let replay = replay_path.map(|path| jet_game_replay_record(&path));
        let backend = backend.map(|(renderer, audio, editor, frame_budget)| GameBackend {
            renderer,
            audio,
            editor,
            frame_budget,
        });
        jet_game_run(&mut scene, replay.as_ref(), backend.as_ref())
    }
}

fn game_value(value: &CtValue) -> Option<&CtValue> {
    match value {
        CtValue::Present(inner) => game_value(inner),
        CtValue::Failed(_) => None,
        _ => Some(value),
    }
}
fn game_optional_value(value: Option<&CtValue>) -> Option<&CtValue> {
    let value = value?;
    match value {
        CtValue::Unit | CtValue::Failed(CtReport::Clean(_)) => None,
        _ => game_value(value),
    }
}

fn game_field<'a>(value: &'a CtValue, name: &str) -> Option<&'a CtValue> {
    match game_value(value)? {
        CtValue::Struct { fields, .. } => fields
            .iter()
            .find_map(|(field, value)| (field == name).then_some(value)),
        _ => None,
    }
}

fn game_string_field(value: &CtValue, name: &str, span: Span) -> Result<String, Diagnostic> {
    match game_field(value, name) {
        Some(CtValue::Str(value)) => Ok(value.clone()),
        _ => Err(unsupported(&format!("game value field `{name}`"), span)),
    }
}

fn game_frame(index: i64, pressed: &[String]) -> CtValue {
    let pressed = CtValue::List(pressed.iter().cloned().map(CtValue::Str).collect());
    game_struct(
        "GameFrame",
        vec![
            ("index", CtValue::Int(index)),
            ("user_index", CtValue::Int(index)),
            (
                "input",
                game_struct(
                    "GameInputSnapshot",
                    vec![("pressed", pressed.clone())],
                ),
            ),
            (
                "user_input",
                game_struct("GameInputSnapshot", vec![("pressed", pressed)]),
            ),
        ],
    )
}

fn game_backend_parts(
    value: &CtValue,
    span: Span,
) -> Result<(String, String, String, Option<i64>), Diagnostic> {
    let renderer = game_string_field(value, "renderer", span)?;
    let audio = game_string_field(value, "audio", span)?;
    let editor = game_string_field(value, "editor", span)?;
    let frame_budget = match game_field(value, "frame_budget") {
        Some(CtValue::Int(value)) => Some(*value),
        Some(_) => return Err(unsupported("GameBackend.frame_budget", span)),
        None => None,
    };
    Ok((renderer, audio, editor, frame_budget))
}

fn game_assets(value: &CtValue, span: Span) -> Result<Vec<(String, String)>, Diagnostic> {
    let Some(CtValue::List(values)) = game_field(value, "assets") else {
        return Err(unsupported("GameAssets.assets", span));
    };
    values
        .iter()
        .map(|value| {
            Ok((
                game_string_field(value, "kind", span)?,
                game_string_field(value, "path", span)?,
            ))
        })
        .collect()
}

fn game_bindings(value: &CtValue, span: Span) -> Result<Vec<(String, String)>, Diagnostic> {
    let Some(CtValue::List(values)) = game_field(value, "bindings") else {
        return Err(unsupported("GameInputMap.bindings", span));
    };
    values
        .iter()
        .map(|value| {
            Ok((
                game_string_field(value, "action", span)?,
                game_string_field(value, "key", span)?,
            ))
        })
        .collect()
}

fn game_components(value: &CtValue, span: Span) -> Result<Vec<String>, Diagnostic> {
    let Some(CtValue::List(values)) = game_field(value, "components") else {
        return Err(unsupported("GameScene.components", span));
    };
    values
        .iter()
        .map(|value| match value {
            CtValue::Str(value) => Ok(value.clone()),
            _ => Err(unsupported("GameScene component", span)),
        })
        .collect()
}

fn data_join_key(value: CtValue, span: Span) -> Result<String, Diagnostic> {
    match value {
        CtValue::Str(value) => Ok(value),
        _ => Err(unsupported("core.data join key", span)),
    }
}

impl<'a, 'debug> EvalCtx<'a, 'debug> {
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
            // I9: `core.data.{csv,json}` IS `core.encoding.{csv,json}.decode<[T]>`.
            // AOT emits exactly that — `jet_enc_csv_decode::<Vec<T>>` and
            // `jet_enc_json_decode::<Vec<T>>` in TIR/emit/core_calls.rs:1450
            // and :1459 — so this tier marshals into the one typed-decode entry
            // instead of owning a second decoder.
            "csv" | "json" => {
                let text = match self.eval_expr(&args[0], scope)? {
                    CtValue::Str(s) => s,
                    _ => {
                        return Err(unsupported(
                            &format!("`data.{method}()`: expected string"),
                            span,
                        ))
                    }
                };
                let elem = list_elem(call_ty).cloned().ok_or_else(|| {
                    unsupported(
                        &format!("`data.{method}<T>()` needs a list result type"),
                        span,
                    )
                })?;
                let target = Type::List(Box::new(elem));
                let decoded = if method == "csv" {
                    self.decode_codec_rows(&target, text, span)?
                } else {
                    self.decode_codec_value("core.encoding.json", &target, text, span)?
                };
                Ok(match decoded {
                    Ok(value) => ok(value),
                    Err(error) => err(error),
                })
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
                        let key = if type_name == "Series" {
                            "values"
                        } else {
                            "rows"
                        };
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
                        return Err(unsupported(
                            "`data.count()` needs a list/table/series",
                            span,
                        ));
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
                    let entry = groups.entry(key).or_insert((0, 0.0, left_s, right_s));
                    entry.0 += 1;
                    entry.1 += amount;
                }
                let cell_ty = if checked {
                    "DataPivotCell"
                } else {
                    "DataGroup"
                };
                let out: Vec<CtValue> = groups
                    .into_iter()
                    .map(|(_, (count, sum, row, column))| {
                        let mean = if count == 0 { 0.0 } else { sum / count as f64 };
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
                        if type_name == "Series" || type_name == "DataSeries" =>
                    {
                        fields
                            .into_iter()
                            .find_map(|(name, value)| (name == "values").then_some(value))
                            .ok_or_else(|| unsupported("`data.values()` needs a Series", span))
                    }
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
            // I8/I9: `table`/`rows`/`schema` marshal into the one shared
            // `core.data` pipeline kernel, which mirrors the Prelude's
            // `jet_data_table` / `jet_data_rows` and the type-driven columns
            // AOT emits from `emit_data_schema_columns`. This tier owns no
            // table construction and no second schema derivation.
            "table" => {
                let rows = self.eval_expr(&args[0], scope)?;
                DataPipeline::table_value(
                    &rows,
                    args.first().map(|arg| &arg.ty),
                    Some(call_ty),
                    span,
                )
            }
            "rows" => {
                let table = self.eval_expr(&args[0], scope)?;
                DataPipeline::rows_value(&table, span)
            }
            "schema" => {
                let recv = self.eval_expr(&args[0], scope)?;
                // Sema's registered field/type tables are the same rows AOT
                // lowering reads, so an empty table still answers from its
                // declared row type instead of guessing from a missing sample.
                let this = &*self;
                let row: DataPipeline::SchemaRow<'_> = &|name: &str| {
                    // #2252: the row type may be declared by an imported
                    // module, whose shape rows are keyed by that module's
                    // canonical identity.
                    let canonical = this.canonical_nominal(name);
                    let key = canonical.as_deref().unwrap_or(name);
                    Some((
                        this.struct_type_params
                            .get(key)
                            .cloned()
                            .unwrap_or_default(),
                        this.struct_field_types.get(key)?.clone(),
                    ))
                };
                DataPipeline::schema_value(&recv, args.first().map(|arg| &arg.ty), row, span)
            }
            "lazy" => {
                let table = self.eval_expr(&args[0], scope)?;
                let CtValue::Struct { type_name, fields } = table else {
                    return Err(unsupported("`data.lazy()` needs a Table", span));
                };
                if type_name != "Table" {
                    return Err(unsupported("`data.lazy()` needs a Table", span));
                }
                let field = |name: &str| {
                    fields
                        .iter()
                        .find(|(field, _)| field == name)
                        .map(|(_, value)| value.clone())
                };
                Ok(ct_struct(
                    "LazyFrame",
                    vec![
                        (
                            "rows",
                            field("rows")
                                .ok_or_else(|| unsupported("Table rows are missing", span))?,
                        ),
                        ("missing", field("missing").unwrap_or(CtValue::Int(0))),
                        ("plan", field("plan").unwrap_or_else(|| CtValue::List(Vec::new()))),
                        ("operations", CtValue::List(Vec::new())),
                        (
                            "elem_type",
                            field("elem_type")
                                .unwrap_or_else(|| CtValue::Str("Unknown".to_string())),
                        ),
                    ],
                ))
            }
            "lazy_filter" | "lazy_sort_by" => {
                let frame = self.eval_expr(&args[0], scope)?;
                let function = self.eval_expr(&args[1], scope)?;
                let CtValue::Struct { type_name, fields } = frame else {
                    return Err(unsupported(
                        &format!("`data.{method}()` needs a LazyFrame"),
                        span,
                    ));
                };
                if type_name != "LazyFrame" {
                    return Err(unsupported(
                        &format!("`data.{method}()` needs a LazyFrame"),
                        span,
                    ));
                }
                let field = |name: &str| {
                    fields
                        .iter()
                        .find(|(field, _)| field == name)
                        .map(|(_, value)| value.clone())
                };
                let mut plan = match field("plan") {
                    Some(CtValue::List(values)) => values,
                    _ => Vec::new(),
                };
                let kind = if method == "lazy_filter" {
                    "filter"
                } else {
                    "sort_by"
                };
                plan.push(CtValue::Str(kind.to_string()));
                let mut operations = match field("operations") {
                    Some(CtValue::List(values)) => values,
                    _ => Vec::new(),
                };
                operations.push(ct_struct(
                    "DataLazyOperation",
                    vec![
                        ("kind", CtValue::Str(kind.to_string())),
                        ("function", function),
                    ],
                ));
                Ok(ct_struct(
                    "LazyFrame",
                    vec![
                        (
                            "rows",
                            field("rows")
                                .ok_or_else(|| unsupported("LazyFrame rows are missing", span))?,
                        ),
                        ("missing", field("missing").unwrap_or(CtValue::Int(0))),
                        ("plan", CtValue::List(plan)),
                        ("operations", CtValue::List(operations)),
                        (
                            "elem_type",
                            field("elem_type")
                                .unwrap_or_else(|| CtValue::Str("Unknown".to_string())),
                        ),
                    ],
                ))
            }
            "plan" => {
                let frame = self.eval_expr(&args[0], scope)?;
                match frame {
                    CtValue::Struct { type_name, fields } if type_name == "LazyFrame" => fields
                        .into_iter()
                        .find_map(|(name, value)| (name == "plan").then_some(value))
                        .ok_or_else(|| unsupported("LazyFrame plan is missing", span)),
                    _ => Err(unsupported("`data.plan()` needs a LazyFrame", span)),
                }
            }
            "collect" => {
                let frame = self.eval_expr(&args[0], scope)?;
                let CtValue::Struct { type_name, fields } = frame else {
                    return Err(unsupported("`data.collect()` needs a LazyFrame", span));
                };
                if type_name != "LazyFrame" {
                    return Err(unsupported("`data.collect()` needs a LazyFrame", span));
                }
                let mut rows = match fields.iter().find(|(name, _)| name == "rows") {
                    Some((_, CtValue::List(rows))) => rows.clone(),
                    _ => return Err(unsupported("LazyFrame rows are missing", span)),
                };
                let operations = match fields.iter().find(|(name, _)| name == "operations") {
                    Some((_, CtValue::List(values))) => values.clone(),
                    _ => Vec::new(),
                };
                for operation in operations {
                    let CtValue::Struct { type_name, fields } = operation else {
                        return Err(unsupported("invalid lazy operation", span));
                    };
                    if type_name != "DataLazyOperation" {
                        return Err(unsupported("invalid lazy operation", span));
                    }
                    let kind = fields.iter().find_map(|(name, value)| {
                        (name == "kind").then_some(value)
                    });
                    let function = fields.iter().find_map(|(name, value)| {
                        (name == "function").then_some(value)
                    });
                    let (Some(CtValue::Str(kind)), Some(function)) = (kind, function) else {
                        return Err(unsupported("invalid lazy operation", span));
                    };
                    if kind == "filter" {
                        let mut kept = Vec::new();
                        for row in rows {
                            if as_bool(
                                &self.call_callable_in_scope(function, vec![row.clone()], scope)?,
                                span,
                            )? {
                                kept.push(row);
                            }
                        }
                        rows = kept;
                    } else if kind == "sort_by" {
                        let mut keyed = Vec::with_capacity(rows.len());
                        for row in rows {
                            let key = self
                                .call_callable_in_scope(function, vec![row.clone()], scope)?
                                .jet_show();
                            keyed.push((key, row));
                        }
                        keyed.sort_by(|left, right| left.0.cmp(&right.0));
                        rows = keyed.into_iter().map(|(_, row)| row).collect();
                    } else {
                        return Err(unsupported("invalid lazy operation", span));
                    }
                }
                let mut plan = match fields.iter().find(|(name, _)| name == "plan") {
                    Some((_, CtValue::List(values))) => values.clone(),
                    _ => Vec::new(),
                };
                plan.push(CtValue::Str("collect".to_string()));
                Ok(ct_struct(
                    "Table",
                    vec![
                        ("rows", CtValue::List(rows)),
                        (
                            "missing",
                            fields
                                .iter()
                                .find(|(name, _)| name == "missing")
                                .map(|(_, value)| value.clone())
                                .unwrap_or(CtValue::Int(0)),
                        ),
                        ("plan", CtValue::List(plan)),
                        (
                            "elem_type",
                            fields
                                .iter()
                                .find(|(name, _)| name == "elem_type")
                                .map(|(_, value)| value.clone())
                                .unwrap_or_else(|| CtValue::Str("Unknown".to_string())),
                        ),
                    ],
                ))
            }
            "sort_by" => {
                let rows = match self.eval_expr(&args[0], scope)? {
                    CtValue::List(rows) => rows,
                    _ => return Err(unsupported("`data.sort_by()` needs a list", span)),
                };
                let function = &args[1];
                let mut callable = None;
                let mut keyed = Vec::with_capacity(rows.len());
                for row in rows {
                    let key = self
                        .apply_callable_once(function, &mut callable, vec![row.clone()], scope)?
                        .jet_show();
                    keyed.push((key, row));
                }
                keyed.sort_by(|left, right| left.0.cmp(&right.0));
                let value = CtValue::List(keyed.into_iter().map(|(_, row)| row).collect());
                Ok(if checked { ok(value) } else { value })
            }
            "inner_join" | "left_join" => {
                let operation = method;
                let left = match args.first() {
                    Some(arg) => match self.eval_expr(arg, scope)? {
                        CtValue::List(rows) => rows,
                        _ => {
                            return Err(unsupported(
                                &format!("`data.{operation}()` needs left rows"),
                                span,
                            ));
                        }
                    },
                    None => {
                        return Err(unsupported(
                            &format!("`data.{operation}()` needs left rows"),
                            span,
                        ));
                    }
                };
                let right = match args.get(1) {
                    Some(arg) => match self.eval_expr(arg, scope)? {
                        CtValue::List(rows) => rows,
                        _ => {
                            return Err(unsupported(
                                &format!("`data.{operation}()` needs right rows"),
                                span,
                            ));
                        }
                    },
                    None => {
                        return Err(unsupported(
                            &format!("`data.{operation}()` needs right rows"),
                            span,
                        ));
                    }
                };
                let left_key = args.get(2).ok_or_else(|| {
                    unsupported(
                        &format!("`data.{operation}()` needs a left key"),
                        span,
                    )
                })?;
                let right_key = args.get(3).ok_or_else(|| {
                    unsupported(
                        &format!("`data.{operation}()` needs a right key"),
                        span,
                    )
                })?;
                let mut right_callable = None;
                let mut right_rows = BTreeMap::<String, Vec<CtValue>>::new();
                for row in right {
                    let key = self.apply_callable_once(
                        right_key,
                        &mut right_callable,
                        vec![row.clone()],
                        scope,
                    )?;
                    right_rows
                        .entry(data_join_key(key, span)?)
                        .or_default()
                        .push(row);
                }
                let mut left_callable = None;
                let mut joined = Vec::new();
                for left_row in left {
                    let key = self.apply_callable_once(
                        left_key,
                        &mut left_callable,
                        vec![left_row.clone()],
                        scope,
                    )?;
                    match right_rows.get(&data_join_key(key, span)?) {
                        Some(matches) => {
                            for right_row in matches {
                                joined.push(ct_struct(
                                    "DataJoin",
                                    vec![
                                        ("left", left_row.clone()),
                                        (
                                            "right",
                                            if operation == "left_join" {
                                                CtValue::Present(Box::new(right_row.clone()))
                                            } else {
                                                right_row.clone()
                                            },
                                        ),
                                    ],
                                ));
                            }
                        }
                        None if operation == "left_join" => joined.push(ct_struct(
                            "DataJoin",
                            vec![
                                ("left", left_row),
                                (
                                    "right",
                                    CtValue::absent(Type::Named("Unknown".to_string())),
                                ),
                            ],
                        )),
                        None => {}
                    }
                }
                let value = CtValue::List(joined);
                Ok(if checked { ok(value) } else { value })
            }
            "query" => {
                let rows = self.eval_expr(&args[0], scope)?;
                let query = self.eval_expr(&args[1], scope)?;
                apply_core_call_with_type(
                    "core.data",
                    "query",
                    vec![rows, query],
                    span,
                    self.repl_mode,
                    Some(call_ty),
                )
            }
            "describe" => {
                let values = self.eval_expr(&args[0], scope)?;
                apply_core_call("core.data", "describe", vec![values], span, self.repl_mode)
            }
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
}
impl<'a, 'debug> EvalCtx<'a, 'debug> {
    /// Evaluate `core.game.run` by marshalling the structural evaluator
    /// carrier into the exact Prelude game kernel used by AOT. User callbacks
    /// remain evaluator callables, so this adapter drives their frame values
    /// before asking the Prelude to render the canonical run result.
    pub(super) fn eval_core_game_call(
        &mut self,
        args: &'a [TExpr],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let span = self.span();
        let values = args
            .iter()
            .map(|arg| self.eval_expr(arg, scope))
            .collect::<Result<Vec<_>, _>>()?;
        let scene = values
            .first()
            .and_then(game_value)
            .ok_or_else(|| unsupported("core.game.run scene", span))?;
        if !matches!(
            scene,
            CtValue::Struct { type_name, .. } if type_name == "GameScene"
        ) {
            return Err(unsupported("core.game.run scene", span));
        }
        let name = game_string_field(scene, "name", span)?;
        let assets_value = game_field(scene, "assets")
            .ok_or_else(|| unsupported("core.game.run scene assets", span))?;
        let assets = game_assets(assets_value, span)?;
        let input_value = game_field(scene, "input")
            .ok_or_else(|| unsupported("core.game.run scene input", span))?;
        let bindings = game_bindings(input_value, span)?;
        let components = game_components(scene, span)?;
        let callbacks = match game_field(scene, "callbacks") {
            Some(CtValue::List(values)) => values.clone(),
            _ => return Err(unsupported("core.game.run scene callbacks", span)),
        };
        let replay_path = game_optional_value(values.get(1))
            .map(|value| game_string_field(value, "path", span))
            .transpose()?;
        let backend = game_optional_value(values.get(2))
            .map(|value| game_backend_parts(value, span))
            .transpose()?;
        let frame_budget = backend
            .as_ref()
            .and_then(|(_, _, _, budget)| *budget)
            .unwrap_or(3);
        let measuring = std::env::var("JET_SCENE_PROBE")
            .ok()
            .is_some_and(|probe| probe == name);
        let frame_count = if measuring {
            120usize.saturating_add(600)
        } else {
            usize::try_from(frame_budget.max(0)).unwrap_or(usize::MAX)
        };
        for index in 0..frame_count {
            let pressed = if index == 1 {
                bindings
                    .iter()
                    .map(|(action, _)| action.clone())
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let frame = game_frame(index as i64, &pressed);
            for callback in &callbacks {
                self.call_callable(callback, vec![frame.clone()])?;
                self.sync_callable_captures(callback, scope);
            }
        }
        Ok(CtValue::Str(game_kernel::run_erased(
            name,
            assets,
            bindings,
            components,
            replay_path,
            backend,
            callbacks.len(),
        )))
    }
}
