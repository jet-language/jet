//! Canonical host adapter for compiler-inserted Prelude and effect rows.
//!
//! Callers provide the checked route row and canonical runtime values. This
//! boundary only marshals those values into approved shared kernels; route
//! selection, ownership, fallibility, and diagnostics remain MIR facts.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Comptime::AmbientMirPreludeResult;
use jet_foundation::MIR::{
    MirCallFallibility, MirPreludeAbi, MirPreludeCall, MirPreludeFamily, MirRuntimeValue, MirType,
    MirTypeKind,
};
#[path = "NativeNumericBridge.rs"]
mod NativeNumericBridge;
#[allow(dead_code)]
mod typed_text_prelude {
    include!("../Prelude/TypedText.rs");
}
#[allow(dead_code)]
mod time_prelude {
    fn jet_scheduler_world_now_ms() -> Option<i64> {
        crate::scheduler::jet_scheduler_world_now_ms()
    }
    include!("../Prelude/Core/Duration.rs");
    include!("../Prelude/Core/Time.rs");
}

fn bridge_error(span: Span, what: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        what.into(),
        "the checked Prelude row reached its canonical native adapter with an invalid value shape"
            .to_string(),
        "preserve the MIR route signature and pass values produced by the checked caller".to_string(),
        Some(span),
    )
}

fn integer(args: &[MirRuntimeValue], index: usize, span: Span) -> Result<i64, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Int(value)) => Ok(*value),
        _ => Err(bridge_error(
            span,
            format!("native Prelude argument {index} requires an integer"),
        )),
    }
}

fn float(args: &[MirRuntimeValue], index: usize, span: Span) -> Result<f64, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Float { value, .. }) => Ok(*value),
        _ => Err(bridge_error(
            span,
            format!("native Prelude argument {index} requires a float"),
        )),
    }
}

fn boolean(args: &[MirRuntimeValue], index: usize, span: Span) -> Result<bool, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Bool(value)) => Ok(*value),
        _ => Err(bridge_error(
            span,
            format!("native Prelude argument {index} requires a boolean"),
        )),
    }
}

fn text<'a>(args: &'a [MirRuntimeValue], index: usize, span: Span) -> Result<&'a str, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::String(value)) => Ok(value),
        _ => Err(bridge_error(
            span,
            format!("native Prelude argument {index} requires text"),
        )),
    }
}
fn optional_text<'a>(
    args: &'a [MirRuntimeValue],
    index: usize,
    span: Span,
) -> Result<Option<&'a str>, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::String(value)) => Ok(Some(value)),
        Some(MirRuntimeValue::Absent { element }) if element.nominal_name() == Some("String") => {
            Ok(None)
        }
        _ => Err(bridge_error(
            span,
            format!("native Prelude argument {index} requires text or an absent text"),
        )),
    }
}

fn stop_integer(
    args: &[MirRuntimeValue],
    index: usize,
    name: &str,
    span: Span,
) -> Result<u32, Diagnostic> {
    u32::try_from(integer(args, index, span)?).map_err(|_| {
        bridge_error(
            span,
            format!("native Prelude runtime-stop {name} requires a nonnegative 32-bit integer"),
        )
    })
}

fn runtime_stop_control(
    args: &[MirRuntimeValue],
    condition: Option<bool>,
    message: &str,
    context_start: usize,
    span: Span,
) -> Result<AmbientMirPreludeResult, Diagnostic> {
    if condition == Some(true) {
        return Ok(AmbientMirPreludeResult::Value(MirRuntimeValue::Unit));
    }
    let file = text(args, context_start, span)?;
    let line = stop_integer(args, context_start + 1, "line", span)?;
    let function = text(args, context_start + 2, span)?;
    let source_line = text(args, context_start + 3, span)?;
    let column = stop_integer(args, context_start + 4, "column", span)?;
    let caret = stop_integer(args, context_start + 5, "caret", span)?;
    let locals = text(args, context_start + 6, span)?;
    let report = jet_foundation::Outcome::jet_render_runtime_stop(
        "E3001",
        file,
        line,
        function,
        source_line,
        column,
        caret,
        message,
        locals,
    );
    Ok(AmbientMirPreludeResult::Control {
        value: MirRuntimeValue::Unit,
        stdout: Vec::new(),
        stderr: report.rendered.into_bytes(),
        exit_code: report.exit_code,
    })
}

fn list<'a>(
    args: &'a [MirRuntimeValue],
    index: usize,
    span: Span,
) -> Result<&'a [MirRuntimeValue], Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::List(values)) => Ok(values),
        _ => Err(bridge_error(
            span,
            format!("native Prelude argument {index} requires a list"),
        )),
    }
}

fn string_list<'a>(
    args: &'a [MirRuntimeValue],
    index: usize,
    span: Span,
) -> Result<Vec<&'a str>, Diagnostic> {
    list(args, index, span)?
        .iter()
        .enumerate()
        .map(|(position, value)| match value {
            MirRuntimeValue::String(value) => Ok(value.as_str()),
            _ => Err(bridge_error(
                span,
                format!("native Prelude list argument {index} element {position} requires text"),
            )),
        })
        .collect()
}

fn boolean_list(
    args: &[MirRuntimeValue],
    index: usize,
    span: Span,
) -> Result<Vec<bool>, Diagnostic> {
    list(args, index, span)?
        .iter()
        .enumerate()
        .map(|(position, value)| match value {
            MirRuntimeValue::Bool(value) => Ok(*value),
            _ => Err(bridge_error(
                span,
                format!("native Prelude list argument {index} element {position} requires a boolean"),
            )),
        })
        .collect()
}

fn sql_parts<'a>(
    args: &'a [MirRuntimeValue],
    index: usize,
    span: Span,
) -> Result<(&'a str, &'a [MirRuntimeValue]), Diagnostic> {
    let Some(MirRuntimeValue::Struct { type_name, fields }) = args.get(index) else {
        return Err(bridge_error(
            span,
            format!("native Prelude argument {index} requires an SQL value"),
        ));
    };
    if type_name != "SQL" {
        return Err(bridge_error(
            span,
            format!("native Prelude argument {index} has non-canonical SQL type"),
        ));
    }
    let mut template = None;
    let mut params = None;
    for (name, value) in fields {
        match name.as_str() {
            "template" => {
                if template.is_some() {
                    return Err(bridge_error(span, "SQL value has a duplicate template field"));
                }
                match value {
                    MirRuntimeValue::String(value) => template = Some(value.as_str()),
                    _ => return Err(bridge_error(span, "SQL template field requires text")),
                }
            }
            "params" => {
                if params.is_some() {
                    return Err(bridge_error(span, "SQL value has a duplicate params field"));
                }
                match value {
                    MirRuntimeValue::List(values) => params = Some(values.as_slice()),
                    _ => return Err(bridge_error(span, "SQL params field requires a list")),
                }
            }
            _ => {
                return Err(bridge_error(
                    span,
                    format!("SQL value has an unknown `{name}` field"),
                ));
            }
        }
    }
    let template = template.ok_or_else(|| bridge_error(span, "SQL value has no template field"))?;
    let params = params.ok_or_else(|| bridge_error(span, "SQL value has no params field"))?;
    Ok((template, params))
}

fn sql_value(template: String, params: Vec<MirRuntimeValue>) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "SQL".to_string(),
        fields: vec![
            ("template".to_string(), MirRuntimeValue::String(template)),
            ("params".to_string(), MirRuntimeValue::List(params)),
        ],
    }
}



fn fixed(value: i128) -> MirRuntimeValue {
    match i64::try_from(value) {
        Ok(value) => MirRuntimeValue::Int(value),
        Err(_) => MirRuntimeValue::BigInt(value.to_string()),
    }
}

fn decimal(value: f64) -> MirRuntimeValue {
    MirRuntimeValue::Float { value, f32: false }
}
fn require_nominal_result(
    result_ty: Option<&MirType>,
    expected_name: &str,
    span: Span,
) -> Result<(), Diagnostic> {
    let result_ty = result_ty.ok_or_else(|| {
        bridge_error(
            span,
            format!("typed {expected_name} literal requires a checked result type"),
        )
    })?;
    let valid = result_ty.identity.is_some()
        && matches!(
            &result_ty.kind,
            MirTypeKind::Apply { name, args }
                if name.name == expected_name && args.is_empty()
        );
    if valid {
        Ok(())
    } else {
        Err(bridge_error(
            span,
            format!("typed {expected_name} literal has the wrong checked result type"),
        ))
    }
}

fn outcome(result: Result<MirRuntimeValue, String>) -> MirRuntimeValue {
    match result {
        Ok(value) => MirRuntimeValue::Present(Box::new(value)),
        Err(error) => MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(error))),
    }
}

fn exact_option_none(
    result_ty: Option<&MirType>,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    let element = result_ty
        .and_then(MirType::option_inner)
        .ok_or_else(|| bridge_error(span, "exact unit conversion requires an Option result"))?;
    Ok(MirRuntimeValue::Absent {
        element: element.clone(),
    })
}

#[derive(Clone, Copy)]
struct NativePreludeRoute<'a> {
    module: &'a str,
    member: &'a str,
    symbol: &'a str,
    family_overflow: bool,
    abi_value: bool,
    infallible: bool,
    effect_name: Option<&'a str>,
    arity: usize,
    max_arity: usize,
    borrow_mask: &'a [bool],
}

fn is_native_route(symbol: &str) -> bool {
    matches!(
        symbol,
        "jet_inline_range_from_int"
            | "jet_typed_sql_raw"
            | "jet_typed_html_raw"
            | "jet_typed_sh_raw"
            | "jet_typed_sql_template"
            | "jet_typed_sql_params"
            | "jet_typed_html_text"
            | "jet_typed_sql_interpolate"
            | "jet_typed_html_interpolate"
            | "jet_typed_sh_interpolate"
            | "jet_term_write_stdout_line"
            | "jet_std_io_eprint"
            | "jet_require"
            | "jet_require_eq"
            | "jet_panic_rich"
            | "jet_typed_path_literal"
            | "jet_typed_datetime_literal"
            | "jet_unit_conversion_exact"
            | "jet_std::jet_unit_conversion_exact_measurement"
            | "jet_unit_conversion_rounded"
            | "jet_std::jet_unit_conversion_rounded_measurement"
    ) || stdio_route(symbol).is_some()
}

/// D-COREIO1=A: `Stdout`/`Stderr` handle rows (`jet_std_io_{stream}_{method}`).
/// The handles are tokens for the process streams, so only the stream name and
/// method select the write.
fn stdio_route(symbol: &str) -> Option<(&'static str, &str)> {
    let rest = symbol.strip_prefix("jet_std_io_")?;
    let (stream, method) = if let Some(method) = rest.strip_prefix("stdout_") {
        ("stdout", method)
    } else {
        ("stderr", rest.strip_prefix("stderr_")?)
    };
    matches!(method, "write" | "write_line" | "write_bytes" | "flush" | "is_tty")
        .then_some((stream, method))
}

fn byte_list(args: &[MirRuntimeValue], index: usize, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Bytes(bytes)) => Ok(bytes.clone()),
        Some(MirRuntimeValue::List(values)) => values
            .iter()
            .map(|value| match value {
                MirRuntimeValue::Int(byte) => u8::try_from(*byte).map_err(|_| {
                    bridge_error(span, format!("native Prelude argument {index} has a non-U8 item"))
                }),
                _ => Err(bridge_error(
                    span,
                    format!("native Prelude argument {index} has a non-U8 item"),
                )),
            })
            .collect(),
        _ => Err(bridge_error(
            span,
            format!("native Prelude argument {index} requires [U8]"),
        )),
    }
}

/// Dispatch one checked native Prelude row.
///
/// `None` means that this adapter does not own the row; `Some(Err(_))` is a
/// checked route/value failure and must be surfaced by the caller unchanged.
pub(crate) fn dispatch(
    row: &MirPreludeCall,
    args: Vec<MirRuntimeValue>,
    result_ty: Option<MirType>,
    span: Span,
) -> Option<Result<AmbientMirPreludeResult, Diagnostic>> {
    dispatch_route(
        NativePreludeRoute {
            module: row.module.as_str(),
            member: row.member.as_str(),
            symbol: row.symbol.name(),
            family_overflow: row.family == MirPreludeFamily::Overflow,
            abi_value: row.abi == MirPreludeAbi::Value,
            infallible: matches!(&row.fallibility, MirCallFallibility::Infallible),
            effect_name: row.effect.as_ref().map(|effect| effect.name()),
            arity: row.signature.arity,
            max_arity: row.signature.max_arity,
            borrow_mask: &row.signature.borrow_mask,
        },
        args,
        result_ty,
        span,
    )
}

fn dispatch_route(
    route: NativePreludeRoute<'_>,
    args: Vec<MirRuntimeValue>,
    result_ty: Option<MirType>,
    span: Span,
) -> Option<Result<AmbientMirPreludeResult, Diagnostic>> {
    if NativeNumericBridge::owns_symbol(route.symbol) {
        return NativeNumericBridge::dispatch(route, args, result_ty, span);
    }
    if !is_native_route(route.symbol) {
        return None;
    }
    if route.arity != args.len()
        || route.max_arity != route.arity
        || route.borrow_mask.len() != route.arity
    {
        return Some(Err(bridge_error(
            span,
            format!(
                "native Prelude row `{}.{}` has an invalid checked signature",
                route.module, route.member
            ),
        )));
    }

    let symbol = route.symbol;
    if symbol == "jet_term_write_stdout_line" {
        let result = (|| -> Result<AmbientMirPreludeResult, Diagnostic> {
            let text = text(&args, 0, span)?;
            let _flush = boolean(&args, 1, span)?;
            let frame = crate::terminal_runtime::jet_term_print_frame(text);
            Ok(AmbientMirPreludeResult::Effect {
                value: MirRuntimeValue::Unit,
                stdout: frame.into_bytes(),
                stderr: Vec::new(),
            })
        })();
        return Some(result);
    }
    if symbol == "jet_std_io_eprint" {
        let result = (|| -> Result<AmbientMirPreludeResult, Diagnostic> {
            let text = text(&args, 0, span)?;
            let frame = crate::terminal_runtime::jet_term_print_frame(text);
            Ok(AmbientMirPreludeResult::Effect {
                value: MirRuntimeValue::Unit,
                stdout: Vec::new(),
                stderr: frame.into_bytes(),
            })
        })();
        return Some(result);
    }
    if let Some((stream, method)) = stdio_route(symbol) {
        let result = (|| -> Result<AmbientMirPreludeResult, Diagnostic> {
            let bytes = match method {
                "write" => text(&args, 1, span)?.as_bytes().to_vec(),
                "write_line" => format!("{}\n", text(&args, 1, span)?).into_bytes(),
                "write_bytes" => byte_list(&args, 1, span)?,
                "flush" => Vec::new(),
                _ => {
                    return Ok(AmbientMirPreludeResult::Value(MirRuntimeValue::Bool(
                        if stream == "stdout" {
                            crate::terminal_runtime::jet_term_stdout_is_terminal()
                        } else {
                            crate::terminal_runtime::jet_term_stderr_is_terminal()
                        },
                    )));
                }
            };
            let (stdout, stderr) = if stream == "stdout" {
                (bytes, Vec::new())
            } else {
                (Vec::new(), bytes)
            };
            Ok(AmbientMirPreludeResult::Effect {
                value: MirRuntimeValue::Present(Box::new(MirRuntimeValue::Unit)),
                stdout,
                stderr,
            })
        })();
        return Some(result);
    }
    if symbol == "jet_require" {
        let result = (|| -> Result<AmbientMirPreludeResult, Diagnostic> {
            let condition = boolean(&args, 0, span)?;
            let message = jet_foundation::Outcome::jet_require_message(
                optional_text(&args, 1, span)?,
            )
            .to_string();
            runtime_stop_control(&args, Some(condition), &message, 2, span)
        })();
        return Some(result);
    }
    if symbol == "jet_require_eq" {
        let result = (|| -> Result<AmbientMirPreludeResult, Diagnostic> {
            let condition = boolean(&args, 0, span)?;
            let left = text(&args, 1, span)?;
            let right = text(&args, 2, span)?;
            let message =
                jet_foundation::Outcome::jet_require_eq_message(&left, &right).to_string();
            runtime_stop_control(&args, Some(condition), &message, 3, span)
        })();
        return Some(result);
    }
    if symbol == "jet_panic_rich" {
        let result = (|| -> Result<AmbientMirPreludeResult, Diagnostic> {
            let message = text(&args, 6, span)?.to_string();
            runtime_stop_control(&args, None, &message, 0, span)
        })();
        return Some(result);
    }
    let result: Result<AmbientMirPreludeResult, Diagnostic> = (|| {
        let value = match symbol {
            "jet_typed_sql_raw" => {
                require_nominal_result(result_ty.as_ref(), "SQL", span)?;
                let (template, params) =
                    typed_text_prelude::jet_typed_sql_raw::<MirRuntimeValue>(
                        text(&args, 0, span)?.to_string(),
                    );
                sql_value(template, params)
            }
            "jet_typed_html_raw" => {
                MirRuntimeValue::String(typed_text_prelude::jet_typed_html_raw(
                    text(&args, 0, span)?.to_string(),
                ))
            }
            "jet_typed_sh_raw" => MirRuntimeValue::List(
                typed_text_prelude::jet_typed_sh_raw(text(&args, 0, span)?.to_string())
                    .into_iter()
                    .map(MirRuntimeValue::String)
                    .collect(),
            ),
            "jet_typed_sql_template" => {
                let (template, _) = sql_parts(&args, 0, span)?;
                MirRuntimeValue::String(template.to_string())
            }
            "jet_typed_sql_params" => {
                let (_, params) = sql_parts(&args, 0, span)?;
                MirRuntimeValue::List(params.to_vec())
            }
            "jet_typed_html_text" => {
                MirRuntimeValue::String(typed_text_prelude::jet_typed_html_text(
                    text(&args, 0, span)?.to_string(),
                ))
            }
            "jet_typed_sql_interpolate" => {
                require_nominal_result(result_ty.as_ref(), "SQL", span)?;
                let literals = string_list(&args, 0, span)?;
                let holes = list(&args, 1, span)?.to_vec();
                if literals.len() != holes.len().saturating_add(1) {
                    return Err(bridge_error(
                        span,
                        "typed SQL literal and hole counts do not match",
                    ));
                }
                let (template, params) =
                    typed_text_prelude::jet_typed_sql_interpolate(&literals, holes);
                sql_value(template, params)
            }
            "jet_typed_html_interpolate" => {
                let literals = string_list(&args, 0, span)?;
                let holes = string_list(&args, 1, span)?
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let trusted_html = boolean_list(&args, 2, span)?;
                if literals.len() != holes.len().saturating_add(1) {
                    return Err(bridge_error(
                        span,
                        "typed HTML literal and hole counts do not match",
                    ));
                }
                if trusted_html.len() != holes.len() {
                    return Err(bridge_error(
                        span,
                        "typed HTML interpolation trust metadata does not match holes",
                    ));
                }
                if literals.len() != holes.len().saturating_add(1) {
                    return Err(bridge_error(
                        span,
                        "typed shell literal and hole counts do not match",
                    ));
                }
                MirRuntimeValue::String(typed_text_prelude::jet_typed_html_interpolate(
                    &literals,
                    holes,
                    &trusted_html,
                ))
            }
            "jet_typed_sh_interpolate" => {
                let literals = string_list(&args, 0, span)?;
                let holes = string_list(&args, 1, span)?
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                MirRuntimeValue::List(
                    typed_text_prelude::jet_typed_sh_interpolate(&literals, holes)
                        .into_iter()
                        .map(MirRuntimeValue::String)
                        .collect(),
                )
            }
            "jet_typed_path_literal" => {
                require_nominal_result(result_ty.as_ref(), "Path", span)?;
                let literals = string_list(&args, 0, span)?;
                let holes = string_list(&args, 1, span)?
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if literals.len() != holes.len().saturating_add(1) {
                    return Err(bridge_error(
                        span,
                        "typed Path literal and hole counts do not match",
                    ));
                }
                typed_text_prelude::jet_validate_typed_path_literal(&literals)
                    .map_err(|error| bridge_error(span, error))?;
                let value = typed_text_prelude::jet_typed_path_interpolate(&literals, &holes);
                MirRuntimeValue::Struct {
                    type_name: "Path".to_string(),
                    fields: vec![("inner".to_string(), MirRuntimeValue::String(value))],
                }
            }
            "jet_typed_datetime_literal" => {
                require_nominal_result(result_ty.as_ref(), "DateTime", span)?;
                let literals = string_list(&args, 0, span)?;
                let holes = string_list(&args, 1, span)?
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if !holes.is_empty() {
                    return Err(bridge_error(
                        span,
                        "typed DateTime literals cannot contain interpolation",
                    ));
                }
                if literals.len() != holes.len().saturating_add(1) {
                    return Err(bridge_error(
                        span,
                        "typed DateTime literal and hole counts do not match",
                    ));
                }
                let text =
                    typed_text_prelude::jet_typed_datetime_interpolate(&literals, &holes);
                let datetime = time_prelude::jet_time_parse_rfc3339(&text)
                    .map_err(|error| bridge_error(span, error))?;
                MirRuntimeValue::Struct {
                    type_name: "DateTime".to_string(),
                    fields: vec![
                        (
                            "secs".to_string(),
                            MirRuntimeValue::Int(datetime.unix_seconds_anchor()),
                        ),
                        (
                            "nanos".to_string(),
                            MirRuntimeValue::Int(datetime.nanosecond()),
                        ),
                        (
                            "leap_second".to_string(),
                            MirRuntimeValue::Bool(datetime.is_leap_second()),
                        ),
                    ],
                }
            }
            "jet_inline_range_from_int" => crate::inline_range::jet_inline_range_from_int(
                integer(&args, 0, span)?,
                integer(&args, 1, span)?,
                integer(&args, 2, span)?,
            )
            .map(|value| outcome(Ok(fixed(i128::from(value)))))
            .unwrap_or_else(|_| outcome(Err("inline range value is outside its checked bounds".to_string()))),
            "jet_unit_conversion_exact" | "jet_std::jet_unit_conversion_exact_measurement" => {
                let value = jet_foundation::jet_unit_conversion_exact(
                    float(&args, 0, span)?,
                    text(&args, 1, span)?,
                    text(&args, 2, span)?,
                    text(&args, 3, span)?,
                    text(&args, 4, span)?,
                );
                match value {
                    Some(value) if symbol == "jet_unit_conversion_exact" => {
                        MirRuntimeValue::Present(Box::new(decimal(value)))
                    }
                    Some(value) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Struct {
                        type_name: "Measurement".to_string(),
                        fields: vec![
                            ("value".to_string(), decimal(value)),
                            ("uncertainty".to_string(), decimal(float(&args, 5, span)?)),
                        ],
                    })),
                    None => exact_option_none(result_ty.as_ref(), span)?,
                }
            }
            "jet_unit_conversion_rounded"
            | "jet_std::jet_unit_conversion_rounded_measurement" => {
                let mode = match integer(&args, 5, span)? {
                    0 => jet_foundation::UnitRoundingMode::TowardZero,
                    1 => jet_foundation::UnitRoundingMode::Floor,
                    2 => jet_foundation::UnitRoundingMode::Ceiling,
                    3 => jet_foundation::UnitRoundingMode::NearestEven,
                    _ => return Err(bridge_error(span, "invalid checked unit rounding tag")),
                };
                let value = jet_foundation::jet_unit_conversion_rounded(
                    float(&args, 0, span)?,
                    text(&args, 1, span)?,
                    text(&args, 2, span)?,
                    text(&args, 3, span)?,
                    text(&args, 4, span)?,
                    mode,
                    integer(&args, 6, span)?,
                );
                match value {
                    Ok(value) if symbol == "jet_unit_conversion_rounded" => outcome(Ok(decimal(value))),
                    Ok(value) => outcome(Ok(MirRuntimeValue::Struct {
                        type_name: "Measurement".to_string(),
                        fields: vec![
                            ("value".to_string(), decimal(value)),
                            ("uncertainty".to_string(), decimal(float(&args, 7, span)?)),
                        ],
                    })),
                    Err(error) => outcome(Err(error.to_string())),
                }
            }
            _ => unreachable!("native route table and dispatch diverged"),
        };
        Ok(AmbientMirPreludeResult::Value(value))
    })();
    Some(result)
}

/// Private bootstrap facade used by the host crate's ambient Prelude hook.
#[doc(hidden)]
pub fn ambient_call(
    row: &MirPreludeCall,
    args: Vec<MirRuntimeValue>,
    result_ty: Option<MirType>,
    span: Span,
) -> Option<Result<AmbientMirPreludeResult, Diagnostic>> {
    dispatch(row, args, result_ty, span)
}
/// Private source-coupled facade for a checked route row whose local MIR schema
/// is not the canonical Foundation `MirPreludeCall` carrier.
#[doc(hidden)]
pub fn ambient_call_route(
    module: &str,
    member: &str,
    symbol: &str,
    arity: usize,
    family: &str,
    abi: &str,
    fallibility: &str,
    effect_name: Option<&str>,
    max_arity: usize,
    borrow_mask: &[bool],
    args: Vec<MirRuntimeValue>,
    result_ty: Option<MirType>,
    span: Span,
) -> Option<Result<AmbientMirPreludeResult, Diagnostic>> {
    dispatch_route(
        NativePreludeRoute {
            module,
            member,
            symbol,
            arity,
            family_overflow: family == "Overflow",
            abi_value: abi == "Value",
            infallible: fallibility == "Infallible",
            effect_name,
            max_arity,
            borrow_mask,
        },
        args,
        result_ty,
        span,
    )
}
