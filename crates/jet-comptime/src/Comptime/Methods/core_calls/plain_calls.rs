use super::*;
fn apply_duration_in(args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let [duration, unit] = args else {
        return Err(unsupported(
            "core.handle.duration.in expects a Duration and a DurationUnit",
            span,
        ));
    };
    crate::Comptime::Builtins::apply_method(
        duration,
        crate::Syntax::METHOD_DURATION_IN,
        vec![unit.clone()],
        span,
    )
}


/// Evaluate an effect-approved, implemented Core call at comptime / in the REPL.
/// `module` is the full path (e.g. `"core.math"`, `"core.regex"`).
pub fn apply_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    repl_mode: bool,
) -> Result<CtValue, Diagnostic> {
    apply_core_call_with_type(module, method, args, span, repl_mode, None)
}

/// Evaluate one registered pure Core route without consulting its interpreter
/// adapter. Ambient adapters use this to marshal handles around the same pure
/// value constructor without recursively re-entering the ambient dispatcher.
pub fn apply_core_pure_call(
    module: &str,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let row = jet_foundation::Syntax::core_call(module, method).filter(|row| {
        row.pure_route != jet_foundation::Syntax::CoreCallPureRoute::None && !row.is_receiver()
    })?;
    core_pure_parity::evaluate(row, args, span)
}

/// Apply a Core call with sema's resolved return type available to erased
/// adapters. The type is marshalling metadata only; effects and policy remain
/// owned by the existing registries and Prelude kernels.
pub fn apply_core_call_with_type(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    repl_mode: bool,
    resolved_ret: Option<&Type>,
) -> Result<CtValue, Diagnostic> {
    apply_core_call_with_type_args(
        module,
        method,
        args,
        span,
        repl_mode,
        &[],
        resolved_ret,
    )
}

/// Apply a Core call while retaining explicit generic type arguments.  The
/// public Jet call remains five positional values; this slice is checked
/// compiler metadata used by typed Core adapters.
pub fn apply_core_call_with_type_args(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    repl_mode: bool,
    type_args: &[Type],
    resolved_ret: Option<&Type>,
) -> Result<CtValue, Diagnostic> {
    let args = normalize_path_args(module, method, args, span)?;
    validate_core_call_projection(
        module,
        method,
        args.len(),
        jet_foundation::Syntax::CoreCallCoverage::COMPTIME,
        span,
    )?;
    validate_interpreter_route(module, method, span)?;
    // Ambient rows cross the host boundary once. If no installed adapter owns
    // the row, continue through the direct shared Comptime dispatcher below.
    let route = jet_foundation::Syntax::core_call(module, method).map(|row| row.interpreter_route);
    if route.is_none()
        || matches!(
            route,
            Some(jet_foundation::Syntax::CoreCallInterpreterRoute::Ambient)
        )
    {
        if let Some(result) = crate::Comptime::try_ambient_core_call_typed(
            module,
            method,
            args.clone(),
            span,
            resolved_ret.cloned(),
        ) {
            return result;
        }
    }
    apply_core_call_without_ambient_with_type_args(
        module,
        method,
        args,
        span,
        repl_mode,
        type_args,
        resolved_ret,
    )
}

/// Evaluate a Core call directly through the shared Comptime implementation.
/// This entry point never consults an ambient adapter; MIR evaluation uses it
/// after its one explicit ambient attempt so a declining hook cannot recurse.
pub fn apply_core_call_without_ambient(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    repl_mode: bool,
) -> Result<CtValue, Diagnostic> {
    apply_core_call_without_ambient_with_type(module, method, args, span, repl_mode, None)
}

/// Typed variant of [`apply_core_call_without_ambient`]. The resolved return
/// type is available only as metadata for precise constructors and adapters.
pub fn apply_core_call_without_ambient_with_type(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    repl_mode: bool,
    resolved_ret: Option<&Type>,
) -> Result<CtValue, Diagnostic> {
    apply_core_call_without_ambient_with_type_args(
        module,
        method,
        args,
        span,
        repl_mode,
        &[],
        resolved_ret,
    )
}

/// Typed Core evaluation retaining explicit generic type arguments for the
/// selected adapter. The canonical history call carries six arguments:
/// seed, cases, optional strategy, model, actual, and observe.
pub fn apply_core_call_without_ambient_with_type_args(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    repl_mode: bool,
    type_args: &[Type],
    resolved_ret: Option<&Type>,
) -> Result<CtValue, Diagnostic> {
    apply_core_call_without_ambient_with_type_args_and_history_schema(
        module,
        method,
        args,
        span,
        repl_mode,
        type_args,
        resolved_ret,
        None,
    )
}

/// Typed Core evaluation with the checked command schema retained for
/// `testing.histories<T>` on the canonical comptime evaluator path.
pub fn apply_core_call_without_ambient_with_type_args_and_history_schema(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    repl_mode: bool,
    type_args: &[Type],
    resolved_ret: Option<&Type>,
    history_schema: Option<&HistoryCommandSchema>,
) -> Result<CtValue, Diagnostic> {
    let args = normalize_path_args(module, method, args, span)?;
    validate_core_call_projection(
        module,
        method,
        args.len(),
        jet_foundation::Syntax::CoreCallCoverage::COMPTIME,
        span,
    )?;
    // The foundation row owns the effect classification for every plain
    // symbol call. Only effect-free rows may enter the pure parity evaluator;
    // this prevents a new effectful row from accidentally gaining a second
    // comptime implementation.
    if let Some(row) = jet_foundation::Syntax::core_call(module, method)
        .filter(|row| core_call_allows_pure_parity(row))
    {
        if let Some(result) = core_pure_parity::evaluate(row, &args, span) {
            return result;
        }
    }

    if repl_mode {
        if let Some(_) = repl_native_only_module(module) {
            return Err(repl_native_module_diag(module, method, span));
        }
    }

    if module == "core.testing" && method == "histories" {
        return apply_testing_histories(args, type_args, span, history_schema);
    }
    if let Some(result) = apply_raylib_core_call(module, method, &args, span) {
        return result;
    }
    if module == "core.handle"
        && matches!(
            method,
            "path.from" | "path.join" | "path.parent" | "path.extension"
                | "path.stem" | "path.normalize" | "path.is_within"
        )
    {
        return apply_path_call(method, &args, span);
    }
    let one = |i: usize| {
        args.get(i).ok_or_else(|| {
            unsupported(&format!("{}.{}(): missing arg {}", module, method, i), span)
        })
    };
    let args_bool = |index: usize, default: bool| -> Result<bool, Diagnostic> {
        match args.get(index) {
            Some(CtValue::Bool(value)) => Ok(*value),
            Some(_) => Err(unsupported(
                &format!(
                    "{}.{}(): argument {} must be Bool",
                    module,
                    method,
                    index + 1
                ),
                span,
            )),
            None => Ok(default),
        }
    };
    let csv_options = || -> Result<(String, bool, bool), Diagnostic> {
        let delimiter = args
            .get(1)
            .map(|value| as_string(value, span).map(str::to_owned))
            .transpose()?
            .unwrap_or_else(|| ",".to_string());
        Ok((delimiter, args_bool(2, false)?, args_bool(3, false)?))
    };

    match (module, method) {
        // D-BENCH-KEEP1=A: comptime binds the same generic identity behavior;
        // the AOT-only black-box effect has no observable CtValue equivalent.
        // D-OBS3: retain trace context in the shared log Prelude state even
        // when the default evaluator executes a service call.
        ("core.log", "set_trace_id") => {
            log_kernel::set_trace_id(as_string(one(0)?, span)?);
            Ok(CtValue::Unit)
        }

        ("core.prelude", "keep") => Ok(args.first().cloned().unwrap_or(CtValue::Unit)),
        ("jet.unit", "magnitude") => Ok(CtValue::Str(as_float(one(0)?, span)?.to_string())),
        // D-CORE-COMPRESS1=A / card #392 C4: pure gzip stays inside
        // tier-0. No native bridge, Boundary classification, or AOT fallback.
        ("core.archive.gzip", "compress") => Ok(CtValue::Bytes(
            crate::Comptime::ArchiveLite::gzip_compress(&as_bytes(one(0)?, span)?),
        )),
        ("core.archive.gzip", "decompress") => Ok(
            match crate::Comptime::ArchiveLite::gzip_decompress(&as_bytes(one(0)?, span)?) {
                Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            },
        ),
        // The std-only resident codec accepts ordinary dictionaryless zstd
        // frames. The encoder deliberately chooses interoperable raw blocks.
        ("core.archive.zstd", "compress") => Ok(CtValue::Bytes(
            crate::Comptime::ArchiveLite::zstd_compress(&as_bytes(one(0)?, span)?),
        )),
        ("core.archive.zstd", "decompress") => Ok(
            match crate::Comptime::ArchiveLite::zstd_decompress(&as_bytes(one(0)?, span)?) {
                Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            },
        ),
        // D-CORE-COMPRESS1=A / card #392 C4: archive containers are pure byte
        // transforms. Keep them interpreter-resident; never route through the
        // native FFI bridge or an AOT fallback.
        ("core.archive", "zip_compress") => {
            Ok(CtValue::Bytes(crate::Comptime::ArchiveLite::zip_compress(
                as_string(one(0)?, span)?,
                &as_bytes(one(1)?, span)?,
            )))
        }
        ("core.archive", "zip_decompress") => Ok(CtValue::Bytes(
            crate::Comptime::ArchiveLite::zip_decompress(&as_bytes(one(0)?, span)?),
        )),
        ("core.archive", "crc32") => Ok(CtValue::Int(crate::Comptime::ArchiveLite::crc32_value(
            &as_bytes(one(0)?, span)?,
        ))),
        ("core.archive", "adler32") => Ok(CtValue::Int(crate::Comptime::ArchiveLite::adler32(
            &as_bytes(one(0)?, span)?,
        ))),
        ("core.archive", "deflate") => Ok(CtValue::Bytes(crate::Comptime::ArchiveLite::deflate(
            &as_bytes(one(0)?, span)?,
        ))),
        ("core.archive", "inflate") => Ok(CtValue::Bytes(
            crate::Comptime::ArchiveLite::inflate_bytes(&as_bytes(one(0)?, span)?),
        )),
        ("core.archive", "zip_names_json") => Ok(CtValue::Str(
            crate::Comptime::ArchiveLite::zip_names_json(&as_bytes(one(0)?, span)?),
        )),
        ("core.archive", "zip_open") => Ok(CtValue::Bytes(crate::Comptime::ArchiveLite::zip_open(
            &as_bytes(one(0)?, span)?,
        ))),
        ("core.archive", "zip_next") => Ok(CtValue::Str(crate::Comptime::ArchiveLite::zip_next(
            &as_bytes(one(0)?, span)?,
            as_int(one(1)?, span)?,
        ))),
        ("core.archive", "zip_read") => Ok(CtValue::Bytes(crate::Comptime::ArchiveLite::zip_read(
            &as_bytes(one(0)?, span)?,
            &as_string(one(1)?, span)?,
        ))),
        ("core.archive", "zip_write") => Ok(CtValue::Bytes(crate::Comptime::ArchiveLite::zip_write(
            &as_bytes(one(0)?, span)?,
            &as_string(one(1)?, span)?,
            &as_bytes(one(2)?, span)?,
        ))),
        ("core.archive", "zip_close") => Ok(CtValue::Bytes(crate::Comptime::ArchiveLite::zip_close(
            &as_bytes(one(0)?, span)?,
        ))),
        ("core.archive", "zip_extract") => {
            Ok(CtValue::Bytes(crate::Comptime::ArchiveLite::zip_extract(
                &as_bytes(one(0)?, span)?,
                &as_string(one(1)?, span)?,
            )))
        }
        ("core.archive", "unzip") => Ok(CtValue::Bytes(crate::Comptime::ArchiveLite::unzip(
            &as_bytes(one(0)?, span)?,
            &as_string(one(1)?, span)?,
        ))),
        ("core.archive", "tar_add") => Ok(CtValue::Bytes(crate::Comptime::ArchiveLite::tar_add(
            &as_bytes(one(0)?, span)?,
            as_string(one(1)?, span)?,
            &as_bytes(one(2)?, span)?,
        ))),
        ("core.archive", "tar_get") => Ok(CtValue::Bytes(crate::Comptime::ArchiveLite::tar_get(
            &as_bytes(one(0)?, span)?,
            as_string(one(1)?, span)?,
        ))),
        ("core.archive", "tar_names_json") => Ok(CtValue::Str(
            crate::Comptime::ArchiveLite::tar_names_json(&as_bytes(one(0)?, span)?),
        )),
        // D-PENDING1=B: the same four enum variants AOT lowers to JetLoadable.
        ("core.reactive.loadable", state @ ("idle" | "loading")) => Ok(CtValue::Enum {
            type_name: "Loadable".to_string(),
            variant: loadable_variant(state).to_string(),
            args: Vec::new(),
        }),
        ("core.reactive.loadable", state @ ("loaded" | "failed")) => Ok(CtValue::Enum {
            type_name: "Loadable".to_string(),
            variant: loadable_variant(state).to_string(),
            args: vec![(None, one(0)?.clone())],
        }),
        // D-FIDELITY-API1=A: explicit runtime-global signal. Interpreter owns
        // same f32-backed range and validation contract as AOT/JIT.
        ("core.perf", "fidelity") => Ok(CtValue::Float(CtFloat::f64(f32::from_bits(
            PERF_FIDELITY.with(Cell::get),
        ) as f64))),
        ("core.perf", "default_fidelity") => Ok(CtValue::Float(CtFloat::f64(f32::from_bits(
            PERF_DEFAULT_FIDELITY_BITS,
        ) as f64))),
        ("core.perf", "override_fidelity") => {
            let value = as_float(one(0)?, span)?;
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Ok(CtValue::failed(Box::new(CtValue::Str(format!(
                    "core.perf.Perf.override_fidelity needs 0.0 through 1.0, got {}",
                    value
                )))));
            }
            PERF_FIDELITY.with(|c| c.set((value as f32).to_bits()));
            Ok(CtValue::Present(Box::new(CtValue::Unit)))
        }
        ("core.perf", "reset_fidelity") => {
            PERF_FIDELITY.with(|c| c.set(PERF_DEFAULT_FIDELITY_BITS));
            Ok(CtValue::Unit)
        }
        // --- core.math implementation surface ---
        ("core.math", "sqrt") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.sqrt())),
        ("core.math", "floor") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.floor())),
        ("core.math", "ceil") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.ceil())),
        ("core.math", "round") => Ok(CtValue::Int(as_ct_float(one(0)?, span)?.round_i64())),
        ("core.math", "abs") => match one(0)? {
            value if exact_big(&value).is_some() => {
                let value = exact_big(&value).expect("whole-number abs");
                Ok(exact_int_value(value.abs()))
            }
            CtValue::Float(f) => Ok(CtValue::Float(core_math_float_abs(*f))),
            value
                if matches!(
                    &value,
                    CtValue::Struct { type_name, .. }
                        if type_name == crate::Syntax::TYPE_COMPLEX
                ) =>
            {
                crate::Comptime::ComplexParity::abs(&value)
                    .map(|magnitude| CtValue::Float(CtFloat::f64(magnitude)))
                    .ok_or_else(|| unsupported("malformed Complex value", span))
            }
            _ => Err(unsupported("core.math.abs: non-numeric argument", span)),
        },
        ("core.math", "pow") => {
            let a = as_ct_float(one(0)?, span)?;
            let b = as_ct_float(one(1)?, span)?;
            Ok(CtValue::Float(
                a.powf(b)
                    .ok_or_else(|| unsupported("mixing float widths", span))?,
            ))
        }
        ("core.math", "min") => {
            let left = one(0)?;
            let right = one(1)?;
            match (&left, &right) {
                (CtValue::Int(left), CtValue::Int(right)) => Ok(CtValue::Int(
                    math_lib_pure::jet_std_math_min_i64(*left, *right),
                )),
                (CtValue::Float(left), CtValue::Float(right)) => {
                    Ok(CtValue::Float(core_math_float_min(*left, *right, span)?))
                }
                _ => Ok(CtValue::Float(core_math_float_min(
                    as_ct_float(&left, span)
                        .map_err(|_| unsupported("core.math.min: non-numeric arguments", span))?,
                    as_ct_float(&right, span)
                        .map_err(|_| unsupported("core.math.min: non-numeric arguments", span))?,
                    span,
                )?)),
            }
        }
        ("core.math", "max") => {
            let left = one(0)?;
            let right = one(1)?;
            match (&left, &right) {
                (CtValue::Int(left), CtValue::Int(right)) => Ok(CtValue::Int(
                    math_lib_pure::jet_std_math_max_i64(*left, *right),
                )),
                (CtValue::Float(left), CtValue::Float(right)) => {
                    Ok(CtValue::Float(core_math_float_max(*left, *right, span)?))
                }
                _ => Ok(CtValue::Float(core_math_float_max(
                    as_ct_float(&left, span)
                        .map_err(|_| unsupported("core.math.max: non-numeric arguments", span))?,
                    as_ct_float(&right, span)
                        .map_err(|_| unsupported("core.math.max: non-numeric arguments", span))?,
                    span,
                )?)),
            }
        }
        ("core.math", "clamp") => match (one(0)?, one(1)?, one(2)?) {
            (CtValue::Int(value), CtValue::Int(low), CtValue::Int(high)) => Ok(CtValue::Int(
                math_lib_pure::jet_std_math_clamp_i64(*value, *low, *high),
            )),
            (CtValue::Float(value), CtValue::Float(low), CtValue::Float(high)) => Ok(
                CtValue::Float(core_math_float_clamp(*value, *low, *high, span)?),
            ),
            _ => Err(unsupported("core.math.clamp: non-numeric arguments", span)),
        },
        ("core.math", "log2") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.log2())),
        ("core.math", "log10") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.log10())),
        // card #392 gap fix: the rest of `core.math` — mechanical ports of
        // the same one-line Rust std calls AOT's codegen emits
        // (`Codegen/TIR/emit/core_calls.rs`), so results match exactly.
        ("core.math", "sin") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.sin())),
        ("core.math", "cos") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.cos())),
        ("core.math", "tan") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.tan())),
        ("core.math", "asin") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.asin())),
        ("core.math", "acos") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.acos())),
        ("core.math", "atan") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.atan())),
        ("core.math", "sinh") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.sinh())),
        ("core.math", "cosh") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.cosh())),
        ("core.math", "tanh") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.tanh())),
        ("core.math", "exp") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.exp())),
        ("core.math", "ln") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.ln())),
        ("core.math", "acosh") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.acosh())),
        ("core.math", "asinh") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.asinh())),
        ("core.math", "atanh") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.atanh())),
        ("core.math", "cbrt") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.cbrt())),
        ("core.math", "exp2") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.exp2())),
        ("core.math", "exp_m1") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.exp_m1())),
        ("core.math", "ln_1p") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.ln_1p())),
        ("core.math", "signum") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.signum())),
        ("core.math", "trunc") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.trunc())),
        ("core.math", "fract") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.fract())),
        ("core.math", "degrees") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.to_degrees())),
        ("core.math", "radians") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.to_radians())),
        ("core.math", "atan2") => {
            let left = as_ct_float(one(0)?, span)?;
            let right = as_ct_float(one(1)?, span)?;
            Ok(CtValue::Float(
                left.atan2(right)
                    .ok_or_else(|| unsupported("mixing float widths", span))?,
            ))
        }
        ("core.math", "copysign") => {
            let left = as_ct_float(one(0)?, span)?;
            let right = as_ct_float(one(1)?, span)?;
            Ok(CtValue::Float(
                left.copysign(right)
                    .ok_or_else(|| unsupported("mixing float widths", span))?,
            ))
        }
        ("core.math", "log") => {
            let left = as_ct_float(one(0)?, span)?;
            let right = as_ct_float(one(1)?, span)?;
            Ok(CtValue::Float(
                left.log(right)
                    .ok_or_else(|| unsupported("mixing float widths", span))?,
            ))
        }
        ("core.math", "fma") => {
            let a = as_ct_float(one(0)?, span)?;
            let b = as_ct_float(one(1)?, span)?;
            let c = as_ct_float(one(2)?, span)?;
            Ok(CtValue::Float(
                a.mul_add(b, c)
                    .ok_or_else(|| unsupported("mixing float widths", span))?,
            ))
        }
        ("core.math", "factorial") => {
            let value = exact_big(one(0)?).ok_or_else(|| {
                unsupported("this operation on a value that is not a whole number", span)
            })?;
            if value.negative {
                return Ok(CtValue::absent(Type::Int));
            }
            let mut current = crate::Numeric::CtBigInt::from_int(2);
            let mut result = crate::Numeric::CtBigInt::from_int(1);
            while current.compare(&value) != std::cmp::Ordering::Greater {
                result = result.mul(&current);
                current = current.add(&crate::Numeric::CtBigInt::from_int(1));
            }
            Ok(CtValue::Present(Box::new(exact_int_value(result))))
        }
        ("core.math", method @ ("isqrt" | "checked_abs" | "checked_neg")) => {
            let value = exact_big(one(0)?).ok_or_else(|| {
                unsupported("this operation on a value that is not a whole number", span)
            })?;
            let result = match method {
                "isqrt" => value.isqrt(),
                "checked_abs" => Some(value.abs()),
                _ => Some(value.neg()),
            };
            Ok(match result {
                Some(value) => CtValue::Present(Box::new(exact_int_value(value))),
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.math", method @ ("is_even" | "is_odd")) => {
            let value = exact_big(one(0)?)
                .ok_or_else(|| unsupported("parity of a value that is not a whole number", span))?;
            Ok(CtValue::Bool(if method == "is_even" {
                value.is_even()
            } else {
                value.is_odd()
            }))
        }
        ("core.math", method @ ("leading_ones" | "trailing_ones" | "digits")) => {
            let value = exact_big(one(0)?)
                .ok_or_else(|| unsupported("this operation needs a whole number", span))?;
            Ok(CtValue::Int(if method == "leading_ones" {
                value.leading_ones()
            } else if method == "trailing_ones" {
                value.trailing_ones()
            } else {
                value.digits()
            }))
        }
        ("core.math", "binomial") => {
            let n = exact_big(one(0)?)
                .ok_or_else(|| unsupported("binomial needs whole numbers", span))?;
            let k = exact_big(one(1)?)
                .ok_or_else(|| unsupported("binomial needs whole numbers", span))?;
            Ok(match crate::Numeric::CtBigInt::binomial(&n, &k) {
                Some(value) => CtValue::Present(Box::new(exact_int_value(value))),
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.math", method @ ("checked_add" | "checked_sub" | "checked_mul")) => {
            let left = exact_big(one(0)?)
                .ok_or_else(|| unsupported("checked arithmetic needs whole numbers", span))?;
            let right = exact_big(one(1)?)
                .ok_or_else(|| unsupported("checked arithmetic needs whole numbers", span))?;
            let value = match method {
                "checked_add" => left.add(&right),
                "checked_sub" => left.sub(&right),
                _ => left.mul(&right),
            };
            Ok(CtValue::Present(Box::new(exact_int_value(value))))
        }
        ("core.math", method @ ("checked_div" | "checked_rem")) => {
            let left = exact_big(one(0)?)
                .ok_or_else(|| unsupported("checked division needs whole numbers", span))?;
            let right = exact_big(one(1)?)
                .ok_or_else(|| unsupported("checked division needs whole numbers", span))?;
            Ok(match left.div_rem(&right) {
                Some((quotient, remainder)) => {
                    CtValue::Present(Box::new(exact_int_value(if method == "checked_div" {
                        quotient
                    } else {
                        remainder
                    })))
                }
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.math", "checked_pow") => {
            let base = exact_big(one(0)?)
                .ok_or_else(|| unsupported("checked power needs whole numbers", span))?;
            let exponent = exact_big(one(1)?)
                .ok_or_else(|| unsupported("checked power needs whole numbers", span))?;
            Ok(match base.pow(&exponent) {
                Some(value) => CtValue::Present(Box::new(exact_int_value(value))),
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.math", method @ ("saturating_add" | "saturating_sub" | "saturating_mul")) => {
            let left = exact_big(one(0)?)
                .ok_or_else(|| unsupported("arithmetic needs whole numbers", span))?;
            let right = exact_big(one(1)?)
                .ok_or_else(|| unsupported("arithmetic needs whole numbers", span))?;
            let value = match method {
                "saturating_add" => left.add(&right),
                "saturating_sub" => left.sub(&right),
                "saturating_mul" => left.mul(&right),
                _ => unreachable!("whole-number arithmetic guard"),
            };
            Ok(exact_int_value(value))
        }
        ("core.math", "int_pow") => {
            let base = exact_big(one(0)?)
                .ok_or_else(|| unsupported("integer power needs whole numbers", span))?;
            let exponent = exact_big(one(1)?)
                .ok_or_else(|| unsupported("integer power needs whole numbers", span))?;
            Ok(exact_int_value(
                base.pow(&exponent)
                    .unwrap_or_else(|| crate::Numeric::CtBigInt::from_int(0)),
            ))
        }
        ("core.math", method @ ("gcd" | "lcm")) => {
            let left = exact_big(one(0)?)
                .ok_or_else(|| unsupported("number theory needs whole numbers", span))?;
            let right = exact_big(one(1)?)
                .ok_or_else(|| unsupported("number theory needs whole numbers", span))?;
            Ok(exact_int_value(if method == "gcd" {
                crate::Numeric::CtBigInt::gcd(&left, &right)
            } else {
                crate::Numeric::CtBigInt::lcm(&left, &right)
            }))
        }
        ("core.math", "div_mod" | "div_rem") => {
            let left = exact_big(one(0)?)
                .ok_or_else(|| unsupported("division needs whole numbers", span))?;
            let right = exact_big(one(1)?)
                .ok_or_else(|| unsupported("division needs whole numbers", span))?;
            let Some((mut quotient, mut remainder)) = left.div_rem(&right) else {
                return Err(unsupported("division by zero", span));
            };
            if method == "div_mod" && !remainder.is_zero() && left.negative != right.negative {
                quotient = quotient.sub(&crate::Numeric::CtBigInt::from_int(1));
                remainder = remainder.add(&right);
            }
            Ok(named_tuple(&[
                ("quot", exact_int_value(quotient)),
                ("rem", exact_int_value(remainder)),
            ]))
        }
        ("core.math", "is_normal") => Ok(CtValue::Bool(
            as_ct_float(one(0)?, span)?.as_f64().is_normal(),
        )),
        ("core.math", "is_subnormal") => Ok(CtValue::Bool(
            as_ct_float(one(0)?, span)?.as_f64().is_subnormal(),
        )),
        ("core.math", "is_canonical") => {
            let x = as_ct_float(one(0)?, span)?;
            Ok(CtValue::Bool(x.is_finite() || x.is_nan()))
        }
        ("core.math", "is_signed" | "sign_bit") => Ok(CtValue::Bool(
            as_ct_float(one(0)?, span)?.as_f64().is_sign_negative(),
        )),
        ("core.math", "is_zero") => Ok(CtValue::Bool(as_ct_float(one(0)?, span)?.as_f64() == 0.0)),
        ("core.math", "is_integer") => {
            let x = as_ct_float(one(0)?, span)?;
            Ok(CtValue::Bool(x.is_finite() && x.as_f64().fract() == 0.0))
        }
        ("core.math", "next_up") => Ok(CtValue::Float(CtFloat::f64(
            as_ct_float(one(0)?, span)?.as_f64().next_up(),
        ))),
        ("core.math", "next_down") => Ok(CtValue::Float(CtFloat::f64(
            as_ct_float(one(0)?, span)?.as_f64().next_down(),
        ))),
        ("core.math", "copy") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?)),
        ("core.math", "cot") => {
            let x = as_ct_float(one(0)?, span)?.as_f64();
            Ok(CtValue::Float(CtFloat::f64(1.0 / x.tan())))
        }
        ("core.math", "inv") => {
            let x = as_ct_float(one(0)?, span)?.as_f64();
            Ok(CtValue::Float(CtFloat::f64(1.0 / x)))
        }
        ("core.math", "zero") => Ok(CtValue::Float(CtFloat::f64(0.0))),
        ("core.math", "radix") => Ok(CtValue::Int(2)),
        ("core.math", "cmp") => {
            let a = as_ct_float(one(0)?, span)?.as_f64();
            let b = as_ct_float(one(1)?, span)?.as_f64();
            Ok(CtValue::Int(math_lib_pure::jet_std_math_cmp(a, b)))
        }
        ("core.math", "next_after") => {
            let a = as_ct_float(one(0)?, span)?.as_f64();
            let b = as_ct_float(one(1)?, span)?.as_f64();
            Ok(CtValue::Float(CtFloat::f64(
                math_lib_pure::jet_std_math_next_after(a, b),
            )))
        }
        ("core.math", "ldexp" | "scaleb") => {
            let x = as_ct_float(one(0)?, span)?.as_f64();
            let exp = match one(1)? {
                CtValue::Int(v) => *v,
                _ => return Err(unsupported("ldexp needs a whole-number exponent", span)),
            };
            Ok(CtValue::Float(CtFloat::f64(
                math_lib_pure::jet_std_math_ldexp(x, exp),
            )))
        }
        ("core.math", "ilogb") => {
            let x = as_ct_float(one(0)?, span)?.as_f64();
            Ok(match math_lib_pure::jet_std_math_ilogb(x) {
                Some(e) => CtValue::Present(Box::new(CtValue::Int(e))),
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.math", "logb") => Ok(CtValue::Float(CtFloat::f64(
            math_lib_pure::jet_std_math_logb(as_ct_float(one(0)?, span)?.as_f64()),
        ))),
        ("core.math", "significand") => Ok(CtValue::Float(CtFloat::f64(
            math_lib_pure::jet_std_math_significand(as_ct_float(one(0)?, span)?.as_f64()),
        ))),
        ("core.math", "ulp") => Ok(CtValue::Float(CtFloat::f64(
            math_lib_pure::jet_std_math_ulp(as_ct_float(one(0)?, span)?.as_f64()),
        ))),
        ("core.math", "erf") => Ok(CtValue::Float(CtFloat::f64(
            math_lib_pure::jet_std_math_erf(as_ct_float(one(0)?, span)?.as_f64()),
        ))),
        ("core.math", "erfc") => Ok(CtValue::Float(CtFloat::f64(
            math_lib_pure::jet_std_math_erfc(as_ct_float(one(0)?, span)?.as_f64()),
        ))),
        ("core.math", "gamma") => Ok(CtValue::Float(CtFloat::f64(
            math_lib_pure::jet_std_math_gamma(as_ct_float(one(0)?, span)?.as_f64()),
        ))),
        ("core.math", "lgamma") => Ok(CtValue::Float(CtFloat::f64(
            math_lib_pure::jet_std_math_lgamma(as_ct_float(one(0)?, span)?.as_f64()),
        ))),
        ("core.math", "sin_cos") => {
            let x = as_ct_float(one(0)?, span)?.as_f64();
            let (s, c) = x.sin_cos();
            Ok(named_tuple(&[
                ("sin", CtValue::Float(CtFloat::f64(s))),
                ("cos", CtValue::Float(CtFloat::f64(c))),
            ]))
        }
        ("core.math", "modf") => {
            let x = as_ct_float(one(0)?, span)?;
            Ok(named_tuple(&[
                ("fract", CtValue::Float(x.fract())),
                ("whole", CtValue::Float(x.trunc())),
            ]))
        }
        ("core.math", "frexp") => {
            let x = as_ct_float(one(0)?, span)?.as_f64();
            let exp = math_lib_pure::jet_std_math_ilogb(x).unwrap_or(0);
            let frac = if x == 0.0 || !x.is_finite() {
                x
            } else {
                math_lib_pure::jet_std_math_ldexp(x, -exp)
            };
            Ok(named_tuple(&[
                ("frac", CtValue::Float(CtFloat::f64(frac))),
                ("exp", CtValue::Int(exp)),
            ]))
        }
        ("core.math", "hypot") => {
            let left = as_ct_float(one(0)?, span)?;
            let right = as_ct_float(one(1)?, span)?;
            Ok(CtValue::Float(
                left.hypot(right)
                    .ok_or_else(|| unsupported("mixing float widths", span))?,
            ))
        }
        ("core.math", "lerp") => {
            let left = as_ct_float(one(0)?, span)?;
            let right = as_ct_float(one(1)?, span)?;
            let t = as_ct_float(one(2)?, span)?;
            Ok(CtValue::Float(
                left.lerp(right, t)
                    .ok_or_else(|| unsupported("mixing float widths", span))?,
            ))
        }
        ("core.math", "is_nan") => Ok(CtValue::Bool(as_ct_float(one(0)?, span)?.is_nan())),
        ("core.math", "is_inf") => Ok(CtValue::Bool(as_ct_float(one(0)?, span)?.is_infinite())),
        ("core.math", "is_finite") => Ok(CtValue::Bool(as_ct_float(one(0)?, span)?.is_finite())),
        ("core.math", "sign") => Ok(CtValue::Int(as_ct_float(one(0)?, span)?.sign())),
        // --- core.text module implementation surface (card #392: `"core.string"` was a
        // dead key here — no import ever resolves to it, `core.text` is the
        // only ratified spelling (KNOWN_CORE_MODULES), so every arm below was
        // unreachable and every `use core.text as t; t.trim(s)`-style call
        // hit the E0956 fallback. Logic ported verbatim from AOT's
        // `jet_text_*` prelude fns via `TextLite` — R12 parity. ---
        ("core.text", "nfc") => Ok(CtValue::Str(crate::Comptime::TextLite::nfc(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "nfd") => Ok(CtValue::Str(crate::Comptime::TextLite::nfd(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "nfkc") => Ok(CtValue::Str(crate::Comptime::TextLite::nfkc(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "nfkd") => Ok(CtValue::Str(crate::Comptime::TextLite::nfkd(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "casefold") => Ok(CtValue::Str(crate::Comptime::TextLite::casefold(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "lower") => Ok(CtValue::Str(crate::Comptime::TextLite::lower(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "upper") => Ok(CtValue::Str(crate::Comptime::TextLite::upper(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "caseless_eq") => Ok(CtValue::Bool(crate::Comptime::TextLite::caseless_eq(
            as_string(one(0)?, span)?,
            as_string(one(1)?, span)?,
        ))),
        ("core.text", "graphemes") => Ok(CtValue::List(
            crate::Comptime::TextLite::graphemes(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        ("core.text", "words") => Ok(CtValue::List(
            crate::Comptime::TextLite::words(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        ("core.text", "sentences") => Ok(CtValue::List(
            crate::Comptime::TextLite::sentences(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        ("core.text", "scalars") => Ok(CtValue::List(
            as_string(one(0)?, span)?
                .chars()
                .map(|c| CtValue::Str(c.to_string()))
                .collect(),
        )),
        ("core.text", "inspect") => Ok(CtValue::List(
            crate::Comptime::TextLite::inspect(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        // D-TEXTWIDTH1=B: 1-arg call uses the portable default policy and
        // returns a bare `Int`; the 2-arg (`policy:`) call can reject a
        // control character under `.Reject`, so it returns `Int !TextError`.
        // `TextWidth`'s two enum fields evaluate generically (`CtValue::Struct`/
        // `CtValue::Enum`, no per-type interpreter code needed) — this arm
        // just reads them back out.
        ("core.text", "display_width") => {
            let s = as_string(one(0)?, span)?;
            if let Some(policy) = args.get(1) {
                let (ambiguous_wide, controls_reject) = text_width_policy_flags(policy);
                match crate::Comptime::TextLite::display_width_policy(
                    s,
                    ambiguous_wide,
                    controls_reject,
                ) {
                    Ok(n) => Ok(CtValue::Present(Box::new(CtValue::Int(n)))),
                    Err(message) => Ok(CtValue::failed(Box::new(CtValue::Struct {
                        type_name: "TextError".to_string(),
                        fields: vec![("message".to_string(), CtValue::Str(message))],
                    }))),
                }
            } else {
                Ok(CtValue::Int(crate::Comptime::TextLite::display_width_default(
                    s,
                )))
            }
        }
        ("core.text", "scalar_count") => Ok(CtValue::Int(
            as_string(one(0)?, span)?.chars().count() as i64,
        )),
        ("core.text", "byte_count") => Ok(CtValue::Int(as_string(one(0)?, span)?.len() as i64)),
        ("core.text", "is_alphabetic") => Ok(CtValue::Bool(crate::Comptime::TextLite::is_alphabetic(
            as_string(one(0)?, span)?,
        ))),
        ("core.text", "is_numeric") => Ok(CtValue::Bool(crate::Comptime::TextLite::is_numeric(
            as_string(one(0)?, span)?,
        ))),
        ("core.text", "is_whitespace") => Ok(CtValue::Bool(crate::Comptime::TextLite::is_whitespace(
            as_string(one(0)?, span)?,
        ))),
        ("core.text", "is_ascii") => Ok(CtValue::Bool(as_string(one(0)?, span)?.is_ascii())),
        ("core.text", "splitn") => {
            let s = as_string(one(0)?, span)?.to_string();
            let pat = as_string(one(1)?, span)?.to_string();
            let n = as_int(one(2)?, span)?;
            Ok(CtValue::List(
                crate::Comptime::TextLite::splitn(&s, &pat, n)
                    .into_iter()
                    .map(CtValue::Str)
                    .collect(),
            ))
        }
        ("core.text", "rsplitn") => {
            let s = as_string(one(0)?, span)?.to_string();
            let pat = as_string(one(1)?, span)?.to_string();
            let n = as_int(one(2)?, span)?;
            Ok(CtValue::List(
                crate::Comptime::TextLite::rsplitn(&s, &pat, n)
                    .into_iter()
                    .map(CtValue::Str)
                    .collect(),
            ))
        }
        ("core.text", "trim") => Ok(CtValue::Str(crate::Comptime::TextLite::trim(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "trim_start") => Ok(CtValue::Str(crate::Comptime::TextLite::trim_start(
            as_string(one(0)?, span)?,
        ))),
        ("core.text", "trim_end") => Ok(CtValue::Str(crate::Comptime::TextLite::trim_end(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "pad_start") => {
            let s = as_string(one(0)?, span)?.to_string();
            let w = as_int(one(1)?, span)?;
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(crate::Comptime::TextLite::pad_start(
                &s, w, &fill,
            )))
        }
        ("core.text", "pad_end") => {
            let s = as_string(one(0)?, span)?.to_string();
            let w = as_int(one(1)?, span)?;
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(crate::Comptime::TextLite::pad_end(&s, w, &fill)))
        }
        ("core.text", "center") => {
            let s = as_string(one(0)?, span)?.to_string();
            let w = as_int(one(1)?, span)?;
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(crate::Comptime::TextLite::center(&s, w, &fill)))
        }
        ("core.text", "starts_any") => {
            let s = as_string(one(0)?, span)?.to_string();
            let prefixes = match one(1)? {
                CtValue::List(xs) => xs
                    .iter()
                    .map(|v| as_string(v, span).map(|s| s.to_string()))
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err(unsupported("core.text.starts_any: non-list argument", span)),
            };
            Ok(CtValue::Bool(crate::Comptime::TextLite::starts_any(
                &s, &prefixes,
            )))
        }
        ("core.text", "ends_any") => {
            let s = as_string(one(0)?, span)?.to_string();
            let suffixes = match one(1)? {
                CtValue::List(xs) => xs
                    .iter()
                    .map(|v| as_string(v, span).map(|s| s.to_string()))
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err(unsupported("core.text.ends_any: non-list argument", span)),
            };
            Ok(CtValue::Bool(crate::Comptime::TextLite::ends_any(
                &s, &suffixes,
            )))
        }
        ("core.text", "char_indices") => Ok(CtValue::List(
            crate::Comptime::TextLite::char_indices(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        // D-ARGS1 / runtime-tier: empty ArgsSpec builder (same as AOT jet_args_spec).
        ("core.args", "spec") => Ok(crate::Comptime::core_args_spec()),
        // --- core.event (shared TIR evaluator / deopt; mirrors AOT JetEvent*) ---
        // parity: guard tests/event_hooks.rs::decision_hook_outcomes_transform_and_short_circuit
        ("core.event", "scope") => Ok(crate::Comptime::core_event_scope()),
        ("core.event", "policy_sync") => Ok(crate::Comptime::core_event_policy_sync()),
        ("core.event", "new") => Ok(crate::Comptime::core_event_new()),
        ("core.event", "with_policy") => {
            Ok(crate::Comptime::core_event_with_policy(one(0)?.clone()))
        }
        ("core.event", "hook") => Ok(crate::Comptime::core_event_hook(one(0)?.clone())),
        ("core.event", "decision_hook") => {
            Ok(crate::Comptime::core_event_decision_hook(one(0)?.clone()))
        }
        ("core.event", "async_result") => {
            crate::Comptime::core_event_async_result(one(0)?, one(1)?, span)
        }
        // --- core.encoding.json ---
        ("core.encoding.json", "parse") => {
            let text = as_string(one(0)?, span)?;
            match crate::Comptime::JSONInterp::parse_json(text) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(
                    crate::Comptime::JSONInterp::encoding_error_value(e),
                ))),
            }
        }
        ("core.encoding.json", "decode") => {
            // D-JSON3's lenient coercions emit structured audit records. The
            // comptime interpreter has no runtime log-effect seam, so claiming
            // this call would silently drop observable behavior. Stop at the
            // honest boundary; default dev transparently executes the AOT TIR.
            Err(unsupported(
                "JSON lenient decode coercion audit effects",
                span,
            ))
        }
        ("core.encoding.json", "to_string") => {
            let v = one(0)?;
            Ok(CtValue::Str(crate::Comptime::JSONInterp::render_json_pretty(
                v, false, 0,
            )))
        }
        ("core.encoding.json", "to_string_pretty") => {
            let v = one(0)?;
            Ok(CtValue::Str(crate::Comptime::JSONInterp::render_json_pretty(
                v, true, 0,
            )))
        }
        // --- card #392 pass 4 / #1394: core.encoding.json.canonical/events ---
        ("core.encoding.json", "canonical") => {
            let v = one(0)?;
            if jet_foundation::PackageEdition::package_edition_at_least("2027") {
                let limits = if args.len() >= 2 {
                    match crate::Comptime::EncodingLite::encoding_limits_from_value(one(1)?) {
                        Ok(limits) => limits,
                        Err(message) => {
                            return Err(unsupported(&message, span));
                        }
                    }
                } else {
                    crate::Comptime::EncodingLite::EncodingLimitsLite::safe()
                };
                match crate::Comptime::EncodingLite::json_canonical_jcs(v, &limits) {
                    Ok(text) => Ok(CtValue::Present(Box::new(CtValue::Str(text)))),
                    Err(error) => Ok(CtValue::failed(Box::new(error))),
                }
            } else {
                Ok(CtValue::Str(crate::Comptime::EncodingLite::json_canonical(v)))
            }
        }
        ("core.encoding.json", "events") => Ok(CtValue::Str(
            crate::Comptime::EncodingLite::json_events(one(0)?),
        )),
        // --- core.encoding.jsonl (ported verbatim, `EncodingLite.rs`) ---
        ("core.encoding.jsonl", "parse") => {
            let text = as_string(one(0)?, span)?;
            match crate::Comptime::EncodingLite::jsonl_parse(text) {
                Ok(rows) => Ok(CtValue::Present(Box::new(CtValue::List(rows)))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        ("core.encoding.jsonl", "to_string") => {
            let rows = match one(0)? {
                CtValue::List(xs) => xs.clone(),
                _ => {
                    return Err(unsupported(
                        "core.encoding.jsonl.to_string: expected a list",
                        span,
                    ))
                }
            };
            Ok(CtValue::Str(crate::Comptime::EncodingLite::jsonl_render(
                &rows,
            )))
        }
        // --- core.encoding.csv (ported verbatim, `EncodingLite.rs`) ---
        ("core.encoding.csv", "parse") => {
            let text = as_string(one(0)?, span)?;
            let (delimiter, header, skip_blank) = csv_options()?;
            match crate::Comptime::EncodingLite::csv_parse(text, &delimiter, header, skip_blank) {
                Ok(rows) => Ok(CtValue::Present(Box::new(CtValue::List(
                    rows.into_iter()
                        .map(|row| {
                            CtValue::List(row.fields.into_iter().map(CtValue::Str).collect())
                        })
                        .collect(),
                )))),
                Err(e) => Ok(CtValue::failed(Box::new(CtValue::Str(e)))),
            }
        }
        ("core.encoding.csv", "rows") => {
            let text = as_string(one(0)?, span)?;
            let (delimiter, header, skip_blank) = csv_options()?;
            match crate::Comptime::EncodingLite::csv_parse(text, &delimiter, header, skip_blank) {
                Ok(rows) => Ok(CtValue::Present(Box::new(CtValue::List(
                    rows.into_iter().map(csv_row_value).collect(),
                )))),
                Err(e) => Ok(CtValue::failed(Box::new(CtValue::Str(e)))),
            }
        }
        ("core.encoding.csv", "to_string") => {
            // Two shapes, same as AOT and the resident JIT: dynamic `[[String]]`
            // rows, or a typed `[T]` list of `#Codable` values (#1269).
            let arg = one(0)?;
            let rows = match csv_rows_from_records(arg) {
                Some(rows) => rows,
                None => as_string_rows(arg, span)?,
            };
            Ok(CtValue::Str(crate::Comptime::EncodingLite::csv_render(&rows)))
        }
        // --- core.encoding.toml (ported verbatim, `EncodingLite.rs`) ---
        ("core.encoding.toml", "parse") => {
            let text = as_string(one(0)?, span)?;
            match crate::Comptime::EncodingLite::toml_parse(text) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        ("core.encoding.toml", "to_string") => Ok(CtValue::Str(
            crate::Comptime::EncodingLite::toml_render(one(0)?),
        )),
        // --- core.encoding.yaml (ported verbatim, `EncodingLite.rs`) ---
        ("core.encoding.yaml", "parse") => {
            let text = as_string(one(0)?, span)?;
            match crate::Comptime::EncodingLite::yaml_parse(text) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        ("core.encoding.yaml", "to_string") => Ok(CtValue::Str(
            crate::Comptime::EncodingLite::yaml_render(one(0)?),
        )),
        // --- core.encoding.xml (CtValue adapters over XmlKernel) ---
        ("core.encoding.xml", "parse") => {
            let text = as_string(one(0)?, span)?;
            match crate::Comptime::EncodingLite::xml_parse(text) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(
                    crate::Comptime::EncodingLite::xml_error_value(e),
                ))),
            }
        }
        ("core.encoding.xml", "parse_with") => {
            let text = as_string(one(0)?, span)?;
            match crate::Comptime::EncodingLite::xml_parse_with(text, one(1)?) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(
                    crate::Comptime::EncodingLite::xml_error_value(e),
                ))),
            }
        }
        ("core.encoding.xml", "parse_bytes") => {
            let bytes = as_bytes(one(0)?, span)?;
            match crate::Comptime::EncodingLite::xml_parse_bytes(&bytes, args.get(1)) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(
                    crate::Comptime::EncodingLite::xml_source_error_value(e),
                ))),
            }
        }
        ("core.encoding.xml", "to_string") => Ok(CtValue::Str(
            crate::Comptime::EncodingLite::xml_render(one(0)?),
        )),
        ("core.encoding.xml", "to_bytes") => {
            match crate::Comptime::EncodingLite::xml_to_bytes(one(0)?, args.get(1)) {
                Ok(bytes) => Ok(CtValue::Present(Box::new(CtValue::Bytes(bytes)))),
                Err(error) => Ok(CtValue::failed(Box::new(error))),
            }
        }
        // D-ENCXML-PROJECTION1=A: focused helpers (shared foundation projection).
        ("core.encoding.xml", "root") => match crate::Comptime::EncodingLite::xml_root(one(0)?) {
            Ok(v) => Ok(CtValue::Present(Box::new(v))),
            Err(e) => Ok(CtValue::failed(Box::new(e))),
        },
        ("core.encoding.xml", "expanded_name") => {
            match crate::Comptime::EncodingLite::xml_expanded_name(one(0)?) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        ("core.encoding.xml", "attribute") => {
            let name = as_string(one(1)?, span)?;
            match crate::Comptime::EncodingLite::xml_attribute(one(0)?, name) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        ("core.encoding.xml", "content") => {
            match crate::Comptime::EncodingLite::xml_content(one(0)?) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        // --- core.encoding.cbor (shared Foundation kernel adapter) ---
        // D-ENC-CBOR-SURFACE1: current whole-value names return the same
        // Result shape as AOT. Edition compatibility names remain below.
        ("core.encoding.cbor", "to_bytes") => {
            match crate::Comptime::EncodingLite::cbor_encode(one(0)?) {
                Ok(bytes) => Ok(CtValue::Present(Box::new(CtValue::Bytes(bytes)))),
                Err(error) => Ok(CtValue::failed(Box::new(
                    crate::Comptime::EncodingLite::cbor_encoding_error_value(error),
                ))),
            }
        }
        ("core.encoding.cbor", "to_bytes_canonical") => {
            match crate::Comptime::EncodingLite::cbor_encode_canonical(one(0)?) {
                Ok(bytes) => Ok(CtValue::Present(Box::new(CtValue::Bytes(bytes)))),
                Err(error) => Ok(CtValue::failed(Box::new(
                    crate::Comptime::EncodingLite::cbor_encoding_error_value(error),
                ))),
            }
        }
        ("core.encoding.cbor", "parse") => {
            let bytes = as_bytes(one(0)?, span)?;
            let options = match crate::Comptime::EncodingLite::cbor_options(args.get(1)) {
                Ok(options) => options,
                Err(error) => {
                    return Ok(CtValue::failed(Box::new(
                        crate::Comptime::EncodingLite::cbor_encoding_error_value(error),
                    )))
                }
            };
            match crate::Comptime::EncodingLite::cbor_decode(&bytes, &options, false) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(error) => Ok(CtValue::failed(Box::new(
                    crate::Comptime::EncodingLite::cbor_encoding_error_value(error),
                ))),
            }
        }
        ("core.encoding.cbor", "encode") => crate::Comptime::EncodingLite::cbor_encode(one(0)?)
            .map(CtValue::Bytes)
            .map_err(|error| unsupported(&error.reason, span)),
        ("core.encoding.cbor", "decode") => {
            let bytes = as_bytes(one(0)?, span)?;
            let options = crate::Comptime::EncodingLite::cbor_safe_options();
            match crate::Comptime::EncodingLite::cbor_decode(&bytes, &options, false) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(CtValue::Str(e.reason)))),
            }
        }
        // --- core.time pure constructors ---
        // Runtime-only clock reads stay on the same Prelude time kernel as
        // AOT/JIT. The fold gate rejects them before this adapter is reached.
        ("core.time", "now") => Ok(CtValue::Int(time_deadline_kernel::jet_std_time_now())),
        ("core.time", "now_utc") => Ok(runtime_datetime_value(time_kernel::JetDateTime::now())),
        ("core.time", "today") => Ok(runtime_date_value(time_kernel::JetDate::today_utc())),
        ("core.time", "instant") => Ok(CtValue::Struct {
            type_name: "Instant".to_string(),
            fields: vec![(
                "start_ns".to_string(),
                CtValue::Int(time_kernel::jet_time_monotonic_now_ns()),
            )],
        }),
        ("core.time", "sleep") => {
            let nanos = match one(0)? {
                CtValue::Struct { type_name, fields }
                    if type_name == crate::Syntax::DURATION_TYPE =>
                {
                    fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("ns", CtValue::Int(value)) => Some(*value),
                            _ => None,
                        })
                        .ok_or_else(|| unsupported("malformed Duration value", span))?
                }
                _ => return Err(unsupported("time.sleep expects a Duration", span)),
            };
            time_deadline_kernel::jet_std_time_sleep_duration_ns(nanos);
            Ok(CtValue::Unit)
        }
        ("core.time", "sleep_until") => {
            let deadline = match one(0)? {
                CtValue::Struct { type_name, fields } if type_name == "Instant" => fields
                    .iter()
                    .find_map(|(name, value)| match (name.as_str(), value) {
                        ("start_ns", CtValue::Int(value)) => Some(*value),
                        _ => None,
                    })
                    .ok_or_else(|| unsupported("malformed Instant value", span))?,
                _ => return Err(unsupported("time.sleep_until expects an Instant", span)),
            };
            time_deadline_kernel::jet_time_sleep_until(deadline);
            Ok(CtValue::Unit)
        }
        ("core.handle", "duration.in") => apply_duration_in(&args, span),
        ("core.handle", "realtime.next_deadline") => {
            Ok(realtime_stream_field(one(0)?, "next_deadline", span)?.clone())
        }
        ("core.handle", "realtime.receipt") => {
            let stream = one(0)?;
            let field = |name| realtime_stream_field(stream, name, span).map(Clone::clone);
            Ok(CtValue::Struct {
                type_name: "RealtimeReceipt".to_string(),
                fields: vec![
                    ("requested_rate_hz".to_string(), field("requested_rate_hz")?),
                    ("requested_frames".to_string(), field("requested_frames")?),
                    ("completed_callbacks".to_string(), field("completed_callbacks")?),
                    ("completed_frames".to_string(), field("completed_frames")?),
                    ("missed".to_string(), field("missed")?),
                    ("max_lateness_ns".to_string(), field("max_lateness_ns")?),
                    ("start_identity".to_string(), field("start_identity")?),
                    ("end_identity".to_string(), field("end_identity")?),
                ],
            })
        }
        ("core.handle", "realtime.cancel") => Ok(CtValue::Unit),
        ("core.handle", "realtime.is_cancelled") => {
            Ok(realtime_stream_field(one(0)?, "cancelled", span)?.clone())
        }
        ("core.tasks", "timeout") => {
            let nanos = match one(0)? {
                CtValue::Struct { type_name, fields }
                    if type_name == crate::Syntax::DURATION_TYPE =>
                {
                    fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("ns", CtValue::Int(value)) => Some(*value),
                            _ => None,
                        })
                        .ok_or_else(|| unsupported("malformed Duration value", span))?
                }
                _ => return Err(unsupported("task.timeout expects a Duration", span)),
            };
            time_deadline_kernel::jet_task_timeout_duration_ns(nanos);
            Ok(CtValue::Unit)
        }
        ("core.time", "start") => Ok(CtValue::Struct {
            type_name: "Stopwatch".to_string(),
            fields: vec![(
                "start_ms".to_string(),
                CtValue::Int(time_kernel::jet_time_monotonic_now_ns() / 1_000_000),
            )],
        }),
        // D-DET1: testing.fake_clock is the test-facing spelling of the
        // caller-seeded deterministic Clock capability built by Clock.new.
        ("core.testing", "fake_clock") => {
            let seed = match one(0)? {
                CtValue::Int(v) => *v,
                _ => {
                    return Err(unsupported("testing.fake_clock expects an Int seed", span));
                }
            };
            Ok(CtValue::Struct {
                type_name: crate::Syntax::CLOCK_TYPE.to_string(),
                fields: vec![("now".to_string(), CtValue::Int(seed))],
            })
        }
        // --- core.regex / core.regex (D-REGEXENGINE1) ---
        ("core.regex", "flags") => regex_flags(args, span),
        ("core.regex", "compile" | "compile_with") => regex_compile(args, span),
        ("core.regex", "literal") => {
            let pattern = as_string(one(0)?, span)?;
            jet_foundation::RegexSyntax::validate(pattern).map_err(|error| {
                Diagnostic::error(
                    "E0152",
                    format!("this regex pattern is invalid at position {}", error.offset),
                    error.reason,
                    "fix the pattern at the reported position".to_string(),
                    Some(span),
                )
            })?;
            Ok(CtValue::Struct {
                type_name: "__JetRegex".to_string(),
                fields: vec![("pattern".to_string(), CtValue::Str(pattern.to_string()))],
            })
        }
        ("core.regex", "escape") => regex_escape(args, span),
        ("core.regex", "is_match") => regex_is_match(args, span),
        ("core.regex", "full_match") => regex_full_match(args, span),
        ("core.regex", "find") => regex_find(args, span),
        ("core.regex", "find_all") => regex_find_all(args, span),
        ("core.regex", "matches") => regex_matches(args, span),
        ("core.regex", "split") => regex_split(args, span),
        ("core.regex", "split_limit") => regex_split_limit(args, span),
        ("core.regex", "replace") => regex_replace(args, span),
        ("core.regex", "replace_first") => regex_replace_first(args, span),
        ("core.regex", "match") => regex_match(args, span),
        // --- core.math.random (ambient; seed for deterministic REPL transcripts) ---
        ("core.math.random", "seed") => {
            let seed = match one(0)? {
                CtValue::Int(n) => *n as u64,
                _ => return Err(unsupported("random.seed expects an Int", span)),
            };
            ambient_random_kernel::seed(seed as i64);
            Ok(CtValue::Unit)
        }
        ("core.math.random", "int") => {
            let low = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("random.int expects Int bounds", span)),
            };
            let high = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("random.int expects Int bounds", span)),
            };
            Ok(CtValue::Int(ambient_random_kernel::int(low, high)))
        }
        ("core.math.random", "float") => {
            Ok(CtValue::Float(CtFloat::f64(ambient_random_kernel::float())))
        }
        // D-DET1: testing.fake_rng is the test-facing spelling of the same
        // caller-seeded deterministic Rng capability as random.rng.
        ("core.math.random", "rng") | ("core.testing", "fake_rng") => {
            let seed = match one(0)? {
                CtValue::Int(n) => *n as u64,
                _ => {
                    let api = if method == "rng" {
                        "random.rng"
                    } else {
                        "testing.fake_rng"
                    };
                    return Err(unsupported(&format!("{api} expects an Int seed"), span));
                }
            };
            Ok(CtValue::Struct {
                type_name: crate::Syntax::RNG_TYPE.to_string(),
                fields: vec![("state".to_string(), CtValue::Int(seed as i64))],
            })
        }
        ("core.testing", "fake_data") => {
            let seed = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("testing.fake_data expects an Int seed", span)),
            };
            let fake = fake_data_kernel::jet_testing_fake_new(seed);
            Ok(CtValue::Struct {
                type_name: crate::Syntax::FAKE_TYPE.to_string(),
                fields: vec![
                    ("state".to_string(), CtValue::Int(fake.state as i64)),
                    ("locale".to_string(), CtValue::Int(fake.locale as i64)),
                ],
            })
        }
        ("core.testing", "compare") => {
            let cases = match one(0)? {
                CtValue::List(values) => values.clone(),
                _ => return Err(unsupported("testing.compare expects a List of cases", span)),
            };
            let reference = one(1)?.clone();
            let candidate = one(2)?.clone();
            let relation = as_string(one(3)?, span)?.to_string();
            let mut reference_values = Vec::with_capacity(cases.len());
            let mut candidate_values = Vec::with_capacity(cases.len());
            let mut case_ids = Vec::with_capacity(cases.len());
            let mut first_difference = -1_i64;
            let mut unavailable = None;
            for (index, input) in cases.iter().enumerate() {
                let reference_value =
                    match crate::Comptime::try_ambient_standalone_closure(
                        &reference,
                        vec![input.clone()],
                        span,
                    ) {
                        Some(Ok(value)) => value,
                        Some(Err(error)) => {
                            unavailable = Some(error.what.clone());
                            break;
                        }
                        None => {
                            unavailable = Some(
                                "reference callback has no interpreter callable owner".to_string(),
                            );
                            break;
                        }
                    };
                let candidate_value =
                    match crate::Comptime::try_ambient_standalone_closure(
                        &candidate,
                        vec![input.clone()],
                        span,
                    ) {
                        Some(Ok(value)) => value,
                        Some(Err(error)) => {
                            unavailable = Some(error.what.clone());
                            break;
                        }
                        None => {
                            unavailable = Some(
                                "candidate callback has no interpreter callable owner".to_string(),
                            );
                            break;
                        }
                    };
                if !matches!(
                    relation.as_str(),
                    "typed_equality" | "typed_failure" | "ordered_effects"
                ) {
                    unavailable = Some(format!("unknown observation relation `{relation}`"));
                    break;
                }
                if reference_value != candidate_value && first_difference < 0 {
                    first_difference = index as i64;
                }
                case_ids.push(CtValue::Str(format!("case-{index}")));
                reference_values.push(reference_value);
                candidate_values.push(candidate_value);
            }
            let (status, reason) = if let Some(error) = unavailable {
                ("unavailable".to_string(), error)
            } else if cases.is_empty() {
                ("empty".to_string(), "comparison corpus is empty".to_string())
            } else if first_difference >= 0 {
                (
                    "mismatch".to_string(),
                    format!("first differing observation is case-{first_difference}"),
                )
            } else {
                (
                    "matched".to_string(),
                    format!("{} observed case(s) matched", cases.len()),
                )
            };
            Ok(CtValue::Struct {
                type_name: "TestComparison".to_string(),
                fields: vec![
                    ("status".to_string(), CtValue::Str(status)),
                    ("relation".to_string(), CtValue::Str(relation)),
                    ("source".to_string(), CtValue::Str("core.testing".to_string())),
                    ("tool".to_string(), CtValue::Str("interpreter".to_string())),
                    ("target".to_string(), CtValue::Str("comptime".to_string())),
                    ("case_ids".to_string(), CtValue::List(case_ids)),
                    ("inputs".to_string(), CtValue::List(cases)),
                    ("reference".to_string(), CtValue::List(reference_values)),
                    ("candidate".to_string(), CtValue::List(candidate_values)),
                    ("first_difference".to_string(), CtValue::Int(first_difference)),
                    ("reason".to_string(), CtValue::Str(reason)),
                    ("universal_proof".to_string(), CtValue::Bool(false)),
                    ("seed".to_string(), CtValue::absent(Type::Int)),
                ],
            })
        }
        ("core.testing", "assert_equal") => {
            let CtValue::Struct { fields, .. } = one(0)? else {
                return Err(unsupported("testing.assert_equal expects TestComparison", span));
            };
            let status = fields.iter().find_map(|(name, value)| {
                (name == "status").then(|| matches!(value, CtValue::Str(status) if status == "matched"))
            });
            let universal = fields.iter().find_map(|(name, value)| {
                (name == "universal_proof").then(|| matches!(value, CtValue::Bool(true)))
            });
            let nonempty = fields.iter().find_map(|(name, value)| {
                (name == "inputs").then(|| matches!(value, CtValue::List(values) if !values.is_empty()))
            });
            Ok(CtValue::Bool(status == Some(true) && universal != Some(true) && nonempty == Some(true)))
        }
        ("core.testing", "status") => {
            let CtValue::Struct { fields, .. } = one(0)? else {
                return Err(unsupported("testing.status expects TestComparison", span));
            };
            fields
                .iter()
                .find_map(|(name, value)| (name == "status").then(|| value.clone()))
                .ok_or_else(|| unsupported("malformed TestComparison status", span))
        }
        ("core.math.random", "split") => {
            let seed = match one(0)? {
                CtValue::Int(n) => *n as u64,
                _ => return Err(unsupported("random.split expects an Int seed", span)),
            };
            let mixed = ambient_random_kernel::split(seed as i64);
            Ok(CtValue::Struct {
                type_name: crate::Syntax::RNG_TYPE.to_string(),
                fields: vec![("state".to_string(), CtValue::Int(mixed as i64))],
            })
        }
        ("core.math.random", "float_range") => {
            let low = as_float(one(0)?, span)?;
            let high = as_float(one(1)?, span)?;
            Ok(CtValue::Float(CtFloat::f64(
                ambient_random_kernel::float_range(low, high),
            )))
        }
        ("core.math.random", "bool") => {
            let p = as_float(one(0)?, span)?;
            Ok(CtValue::Bool(ambient_random_kernel::bool_p(p)))
        }
        ("core.math.random", "normal") => {
            let mean = as_float(one(0)?, span)?;
            let stddev = as_float(one(1)?, span)?;
            Ok(CtValue::Float(CtFloat::f64(ambient_random_kernel::normal(
                mean, stddev,
            ))))
        }
        ("core.math.random", "exponential") => {
            let lambda = as_float(one(0)?, span)?;
            Ok(CtValue::Float(CtFloat::f64(
                ambient_random_kernel::exponential(lambda),
            )))
        }
        ("core.math.random", "bytes") => {
            let n = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("random.bytes expects an Int count", span)),
            };
            Ok(CtValue::Bytes(ambient_random_kernel::bytes(n)))
        }
        ("core.math.random", "pick") => {
            let CtValue::List(xs) = one(0)?.clone() else {
                return Err(unsupported("random.pick needs a list", span));
            };
            match ambient_random_kernel::pick(&xs) {
                Some(v) => Ok(CtValue::Present(Box::new(v))),
                None => Ok(CtValue::absent(
                    CtValue::resolved_option_element_type(resolved_ret).ok_or_else(|| {
                        unsupported("random.pick needs a resolved element type", span)
                    })?,
                )),
            }
        }
        ("core.math.random", "weighted_pick") => {
            let CtValue::List(xs) = one(0)?.clone() else {
                return Err(unsupported("random.weighted_pick needs a list", span));
            };
            let CtValue::List(ws) = one(1)?.clone() else {
                return Err(unsupported(
                    "random.weighted_pick needs a [Float] weights list",
                    span,
                ));
            };
            let weights: Vec<f64> = ws
                .iter()
                .map(|w| as_float(w, span))
                .collect::<Result<_, _>>()?;
            match ambient_random_kernel::weighted_pick(&xs, &weights) {
                Some(v) => Ok(CtValue::Present(Box::new(v))),
                None => Ok(CtValue::absent(
                    CtValue::resolved_option_element_type(resolved_ret).ok_or_else(|| {
                        unsupported("random.weighted_pick needs a resolved element type", span)
                    })?,
                )),
            }
        }
        ("core.math.random", "sample") => {
            let CtValue::List(xs) = one(0)?.clone() else {
                return Err(unsupported("random.sample needs a list", span));
            };
            let k = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("random.sample count must be Int", span)),
            };
            Ok(CtValue::List(ambient_random_kernel::sample(&xs, k)))
        }
        ("core.math.random", "shuffle") => {
            let CtValue::List(mut xs) = one(0)?.clone() else {
                return Err(unsupported("random.shuffle needs a list", span));
            };
            ambient_random_kernel::shuffle(&mut xs);
            // TIR writes this returned list back through the borrowed place;
            // the AST dispatcher owns the equivalent write-back path.
            Ok(CtValue::List(xs))
        }
        ("core.crypto.random", "bytes") => {
            let count = match one(0)? {
                CtValue::Int(value) => *value,
                _ => return Err(unsupported("crypto.random.bytes expects an Int", span)),
            };
            crypto_entropy_kernel::jet_crypto_entropy_bytes(count)
                .map(CtValue::Bytes)
                .map_err(|error| unsupported(&error.to_string(), span))
        }
        // --- core.text.fmt: CtValue adapters over the shared Prelude kernel ---
        // The Display route `"{value}"` / `print(value)` lower to on every
        // tier: the same value-to-text rule `core.term.print` applies.
        ("core.text.fmt", "display") => {
            let value = one(0)?;
            Ok(CtValue::Str(
                display_core_pure_value(value).unwrap_or_else(|| value.jet_show()),
            ))
        }
        ("core.text.fmt", "debug") => {
            let value = one(0)?;
            let text = core_pure_parity::debug(value).ok_or_else(|| {
                unsupported("value has no checked Debug representation", span)
            })?;
            Ok(CtValue::Str(text))
        }
        ("core.text.fmt", "pretty") => {
            let value = as_string(one(0)?, span)?;
            Ok(CtValue::Str(fmt_kernel::jet_fmt_pretty(value)))
        }
        ("core.text.fmt", "number") => {
            let n = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.number expects an Int", span)),
            };
            Ok(CtValue::Str(fmt_kernel::jet_fmt_number(n)))
        }
        ("core.text.fmt", "decimal" | "grouped") => {
            let value = one(0)?;
            let precision = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.decimal precision must be Int", span)),
            };
            let formatted = match value {
                CtValue::Float(value) if method == "grouped" => {
                    fmt_kernel::jet_fmt_grouped(value.as_f64(), precision)
                }
                CtValue::Float(value) => fmt_kernel::jet_fmt_decimal(value.as_f64(), precision),
                CtValue::Int(value) => {
                    let value = value.to_string();
                    if method == "grouped" {
                        fmt_kernel::jet_fmt_grouped_int(&value, precision)
                    } else {
                        fmt_kernel::jet_fmt_decimal_int(&value, precision)
                    }
                }
                CtValue::BigInt(value) => {
                    let value = value.to_string_rep();
                    if method == "grouped" {
                        fmt_kernel::jet_fmt_grouped_int(&value, precision)
                    } else {
                        fmt_kernel::jet_fmt_decimal_int(&value, precision)
                    }
                }
                _ => return Err(unsupported("fmt.decimal expects a Float or Int", span)),
            };
            Ok(CtValue::Str(formatted))
        }
        ("core.text.fmt", "hex") => {
            let value = match one(0)? {
                CtValue::Int(value) => value.to_string(),
                CtValue::BigInt(value) => value.to_string_rep(),
                _ => return Err(unsupported("fmt.hex expects an Int", span)),
            };
            let width = match one(1)? {
                CtValue::Int(width) => *width,
                _ => return Err(unsupported("fmt.hex width must be Int", span)),
            };
            Ok(CtValue::Str(fmt_kernel::jet_fmt_hex_decimal(&value, width)))
        }
        ("core.text.fmt", "sci") => {
            let value = as_float(one(0)?, span)?;
            let precision = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.sci precision must be Int", span)),
            };
            Ok(CtValue::Str(fmt_kernel::jet_fmt_sci(value, precision)))
        }
        ("core.text.fmt", "percent") => {
            let value = as_float(one(0)?, span)?;
            let precision = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.percent precision must be Int", span)),
            };
            Ok(CtValue::Str(fmt_kernel::jet_fmt_percent(value, precision)))
        }
        ("core.text.fmt", "bin" | "oct") => {
            let value = match one(0)? {
                CtValue::Int(value) => value.to_string(),
                CtValue::BigInt(value) => value.to_string_rep(),
                _ => return Err(unsupported("fmt radix selectors expect an Int", span)),
            };
            let formatted = if method == "bin" {
                fmt_kernel::jet_fmt_bin_decimal(&value)
            } else {
                fmt_kernel::jet_fmt_oct_decimal(&value)
            };
            Ok(CtValue::Str(formatted))
        }
        ("core.text.fmt", "bytes") => {
            let n = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.bytes expects an Int", span)),
            };
            Ok(CtValue::Str(fmt_kernel::jet_fmt_bytes(n)))
        }
        ("core.text.fmt", "duration") => {
            let ms = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.duration expects an Int (ms)", span)),
            };
            Ok(CtValue::Str(fmt_kernel::jet_fmt_duration(ms)))
        }
        ("core.text.fmt", "ordinal") => {
            let n = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.ordinal expects an Int", span)),
            };
            Ok(CtValue::Str(fmt_kernel::jet_fmt_ordinal(n)))
        }
        ("core.text.fmt", "plural") => {
            let count = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.plural count must be Int", span)),
            };
            let singular = as_string(one(1)?, span)?.to_string();
            let plural = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(fmt_kernel::jet_fmt_plural(
                count, &singular, &plural,
            )))
        }
        ("core.text.fmt", "pad") => {
            let text = as_string(one(0)?, span)?.to_string();
            let width = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.pad width must be Int", span)),
            };
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(fmt_kernel::jet_fmt_pad(&text, width, &fill)))
        }
        ("core.text.fmt", "pad_left") => {
            let text = as_string(one(0)?, span)?.to_string();
            let width = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.pad_left width must be Int", span)),
            };
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(fmt_kernel::jet_fmt_pad_left(
                &text, width, &fill,
            )))
        }
        ("core.text.fmt", "pad_right") => {
            let text = as_string(one(0)?, span)?.to_string();
            let width = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.pad_right width must be Int", span)),
            };
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(fmt_kernel::jet_fmt_pad_right(
                &text, width, &fill,
            )))
        }
        ("core.text.fmt", "pad_center") => {
            let text = as_string(one(0)?, span)?.to_string();
            let width = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.pad_center width must be Int", span)),
            };
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(fmt_kernel::jet_fmt_pad_center(
                &text, width, &fill,
            )))
        }
        // --- D-UUIDENC1=A: core.encoding.hex / core.encoding.base64 (pure) ---
        ("core.encoding.hex", "encode") => {
            let bytes = as_bytes(one(0)?, span)?;
            Ok(CtValue::Str(hex_encode(bytes)))
        }
        ("core.encoding.hex", "decode") => {
            let s = as_string(one(0)?, span)?;
            Ok(match hex_decode(s) {
                Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            })
        }
        ("core.encoding.base64", "encode") => {
            let bytes = as_bytes(one(0)?, span)?;
            Ok(CtValue::Str(base64_encode(bytes)))
        }
        ("core.encoding.base64", "decode") => {
            let s = as_string(one(0)?, span)?;
            let allow_whitespace = args_bool(1, false)?;
            let allow_missing_padding = args_bool(2, false)?;
            let edition = jet_foundation::PackageEdition::package_edition();
            Ok(
                match jet_foundation::base_encoding_dispatch::decode_base64(
                    &edition,
                    s,
                    allow_whitespace,
                    allow_missing_padding,
                ) {
                    Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                    Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
                },
            )
        }
        // --- core.encoding.base64 URL-safe variant (pure; mirrors AOT's
        // `jet_std_b64url_*`, EncodingCodecs.rs — the same alphabet with
        // `+`/`/` swapped for `-`/`_` and no padding) ---
        // parity: include path=crates/jet-codegen/src/Prelude/Core/EncodingBase.rs
        ("core.encoding.base64", "encode_url") => {
            let bytes = as_bytes(one(0)?, span)?;
            Ok(CtValue::Str(encoding_base_kernel::jet_std_b64url_encode(
                &bytes,
            )))
        }
        ("core.encoding.base64", "decode_url") => {
            let s = as_string(one(0)?, span)?;
            let allow_whitespace = args_bool(1, false)?;
            let allow_padding = args_bool(2, false)?;
            let edition = jet_foundation::PackageEdition::package_edition();
            Ok(
                match jet_foundation::base_encoding_dispatch::decode_base64url(
                    &edition,
                    s,
                    allow_whitespace,
                    allow_padding,
                ) {
                    Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                    Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
                },
            )
        }
        // --- core.encoding.base32 (pure; mirrors AOT's `jet_std_base32_*`,
        // EncodingCodecs.rs, byte-for-byte — same alphabet, same bit-packing) ---
        // parity: guard tests/encoding_parity.rs::whole_value_codecs_match_aot_comptime_and_default_dev
        ("core.encoding.base32", "encode") => {
            let bytes = as_bytes(one(0)?, span)?;
            Ok(CtValue::Str(base32_encode(&bytes)))
        }
        ("core.encoding.base32", "decode") => {
            let s = as_string(one(0)?, span)?;
            let allow_whitespace = args_bool(1, false)?;
            let allow_missing_padding = args_bool(2, false)?;
            let allow_lowercase = args_bool(3, false)?;
            let edition = jet_foundation::PackageEdition::package_edition();
            Ok(
                match jet_foundation::base_encoding_dispatch::decode_base32(
                    &edition,
                    s,
                    allow_whitespace,
                    allow_missing_padding,
                    allow_lowercase,
                ) {
                    Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                    Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
                },
            )
        }
        // --- D-URL1=A: core.net.url (pure RFC-3986-shaped parser, ported
        // verbatim from AOT's `JetURL`/`jet_url_*` in `UrlMime.rs` — see
        // `UrlLite.rs`) ---
        ("core.net.url", "parse") => {
            let s = as_string(one(0)?, span)?;
            Ok(match crate::Comptime::UrlLite::parse(s) {
                Ok(u) => CtValue::Present(Box::new(url_parts_to_ct(&u))),
                Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
            })
        }
        ("core.net.url", "from_parts") => {
            let scheme = as_string(one(0)?, span)?.to_string();
            let host = as_string(one(1)?, span)?.to_string();
            let path = as_string(one(2)?, span)?.to_string();
            let query = as_string_rows(one(3)?, span)?;
            let fragment = as_string(one(4)?, span)?.to_string();
            Ok(
                match crate::Comptime::UrlLite::from_parts(&scheme, &host, &path, &query, &fragment) {
                    Ok(u) => CtValue::Present(Box::new(url_parts_to_ct(&u))),
                    Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
                },
            )
        }
        ("core.net.url", "file") => {
            let path = as_string(one(0)?, span)?;
            Ok(url_parts_to_ct(&crate::Comptime::UrlLite::file(path)))
        }
        ("core.net.url", "data") => {
            // `mime` arg is a `CtValue::Struct { type_name: "Mime", .. }`
            // (D-URL1's `Mime` type) with `top`/`sub`/`params` fields — the
            // `core.net.mime` module port isn't in this card's slice, so render
            // its essence + params here the same way AOT's
            // `JetMIME::to_string_value` does, matching field-for-field.
            let mime = one(0)?;
            let text = as_string(one(1)?, span)?;
            let rendered = match mime {
                CtValue::Struct { type_name, fields } if type_name == "Mime" => {
                    let get = |name: &str| {
                        fields
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, v)| v.clone())
                    };
                    let top = match get("top") {
                        Some(CtValue::Str(s)) => s,
                        _ => {
                            return Err(unsupported(
                                "core.net.url.data: mime.top must be String",
                                span,
                            ))
                        }
                    };
                    let sub = match get("sub") {
                        Some(CtValue::Str(s)) => s,
                        _ => {
                            return Err(unsupported(
                                "core.net.url.data: mime.sub must be String",
                                span,
                            ))
                        }
                    };
                    let mut out = format!("{}/{}", top, sub);
                    if let Some(CtValue::List(params)) = get("params") {
                        for p in params {
                            if let CtValue::List(kv) = p {
                                if let [CtValue::Str(k), CtValue::Str(v)] = &kv[..] {
                                    out.push_str("; ");
                                    out.push_str(k);
                                    out.push('=');
                                    out.push_str(v);
                                }
                            }
                        }
                    }
                    out
                }
                _ => {
                    return Err(unsupported(
                        "core.net.url.data: first argument must be a Mime",
                        span,
                    ))
                }
            };
            Ok(url_parts_to_ct(&crate::Comptime::UrlLite::data(
                &rendered, text,
            )))
        }
        ("core.net.url", "query") => {
            let rows = as_string_rows(one(0)?, span)?;
            let pairs: Vec<(String, String)> = rows
                .iter()
                .filter(|r| !r.is_empty())
                .map(|r| {
                    (
                        r.get(0).cloned().unwrap_or_default(),
                        r.get(1).cloned().unwrap_or_default(),
                    )
                })
                .collect();
            Ok(CtValue::Str(crate::Comptime::UrlLite::url_render_query(
                &pairs,
            )))
        }
        ("core.net.url", "percent_encode") => {
            let s = as_string(one(0)?, span)?;
            Ok(CtValue::Str(crate::Comptime::UrlLite::url_percent_encode(
                s, false,
            )))
        }
        ("core.net.url", "percent_decode") => {
            let s = as_string(one(0)?, span)?;
            Ok(match crate::Comptime::UrlLite::url_percent_decode_str(s) {
                Ok(v) => CtValue::Present(Box::new(CtValue::Str(v))),
                Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
            })
        }
        // D-COMPUTE1=D / I9: same Prelude as AOT (`ComputeLite` includes Compute.rs).
        ("core.compute", method) => crate::Comptime::ComputeLite::apply(method, &args, span),
        // D-SERVICE1=D / I9: typed `core.service` constructors use the same
        // Prelude adapter as tree methods; only the public module path differs.
        ("core.service", "tree") => crate::Comptime::ServicesLite::apply("tree", &args, span),
        ("core.service", "runtime") => crate::Comptime::ServicesLite::apply("runtime", &args, span),
        ("core.service", "state_store") => {
            crate::Comptime::ServicesLite::apply("state_store", &args, span)
        }
        ("core.service", "restart_one_for_one") => {
            crate::Comptime::ServicesLite::apply("restart_one_for_one", &args, span)
        }
        ("core.service", "restart_one_for_all") => {
            crate::Comptime::ServicesLite::apply("restart_one_for_all", &args, span)
        }
        ("core.service", "restart_rest_for_one") => {
            crate::Comptime::ServicesLite::apply("restart_rest_for_one", &args, span)
        }
        ("core.service", "delivery_at_most_once") => {
            crate::Comptime::ServicesLite::apply("delivery_at_most_once", &args, span)
        }
        ("core.service", "delivery_durable") => {
            crate::Comptime::ServicesLite::apply("delivery_durable", &args, span)
        }
        ("core.service", method) => crate::Comptime::ServicesLite::apply(method, &args, span),
        // D-JOBS1=D / I9: queue construction uses the provider-owned
        // dispatcher. The queue provider owns authority, request, and receipt
        // marshalling; this tier only forwards the registered Core call.
        ("core.jobs", method) => crate::Comptime::ServicesLite::apply_jobs(method, &args, span),
        // D-AUTH1=A / I9: session batteries (JWT/PASETO stay on AOT/subset path).
        // Stateful store ops are Tier-2 (`is_tier2_core_call`) so pure
        // `evaluate_constant` cannot fold them into Ok(literals) while leaving
        // the runtime `JET_AUTH_STORE` empty. AuthLite still serves impure /
        // interpreter ambient via `apply_impure_core_call` → here.
        ("core.auth", method)
            if matches!(
                method,
                "register_user"
                    | "password_login"
                    | "session_validate"
                    | "session_show"
                    | "session_user"
                    | "session_cookie"
                    | "session_id"
                    | "magic_link_issue"
                    | "magic_link_consume"
                    | "oauth_begin"
                    | "oauth_finish"
            ) =>
        {
            crate::Comptime::AuthLite::apply(method, &args, span)
        }
        // D-SYNC1=A / D-DBPOLICY1=A / I9.
        ("core.sync", method) => crate::Comptime::SyncLite::apply(method, &args, span),
        ("core.reactive", "signal" | "derived" | "computed" | "effect") => {
            crate::Comptime::AppLite::apply_suite(module, method, &args, span, resolved_ret)
        }
        ("core.web", "openapi" | "page") => {
            crate::Comptime::AppLite::apply_suite(module, method, &args, span, resolved_ret)
        }
        // D-DX-SUITE1=C / I9: every first-party web module crosses the same
        // Prelude kernel boundary; AppLite only marshals CtValues.
        (module, method) if module.starts_with("core.web.") => {
            crate::Comptime::AppLite::apply_suite(module, method, &args, span, resolved_ret)
        }
        // D-LIVEQUERY1=A / I9: same Prelude as AOT (`AppLite` includes LiveQuery.rs).
        ("app" | "core.web", method)
            if matches!(
                method,
                "live"
                    | "subscribe"
                    | "invalidate"
                    | "transact_invalidate"
                    | "signal_push"
                    | "live_get"
                    | "live_show"
                    | "live_stats"
                    | "auth"
                    | "auth_oauth"
                    | "auth_routes"
                    | "auth_show"
                    | "sync"
            ) =>
        {
            if matches!(method, "auth" | "auth_oauth" | "auth_routes" | "auth_show") {
                crate::Comptime::AuthLite::apply(method, &args, span)
            } else if method == "sync" {
                crate::Comptime::SyncLite::apply(method, &args, span)
            } else {
                crate::Comptime::AppLite::apply(method, &args, span)
            }
        }
        // --- D-DATA-SURFACE1/PLOT1/STATUS1: fixed-signature data kernels.
        // Stats stay on `data_kernel`; table/lazy/query calls are dispatched by
        // the typed evaluator, while loader and stream rows cross the
        // DataPipeline carrier boundary below.
        ("core.data", "sum" | "mean" | "min" | "max" | "median" | "variance" | "stddev") => {
            let values = as_float_list(one(0)?, span)?;
            let (checked, unchecked): (_, fn(&Vec<f64>) -> f64) = match method {
                "sum" => (
                    data_kernel::jet_data_sum_checked(&values),
                    data_kernel::jet_data_sum,
                ),
                "mean" => (
                    data_kernel::jet_data_mean_checked(&values),
                    data_kernel::jet_data_mean,
                ),
                "min" => (
                    data_kernel::jet_data_min_checked(&values),
                    data_kernel::jet_data_min,
                ),
                "max" => (
                    data_kernel::jet_data_max_checked(&values),
                    data_kernel::jet_data_max,
                ),
                "median" => (
                    data_kernel::jet_data_median_checked(&values),
                    data_kernel::jet_data_median,
                ),
                "variance" => (
                    data_kernel::jet_data_variance_checked(&values),
                    data_kernel::jet_data_variance,
                ),
                _ => (
                    data_kernel::jet_data_stddev_checked(&values),
                    data_kernel::jet_data_stddev,
                ),
            };
            Ok(data_result_value(
                checked,
                || unchecked(&values),
                data_float_value,
            ))
        }
        ("core.data", "quantile") => {
            let values = as_float_list(one(0)?, span)?;
            let q = as_float(one(1)?, span)?;
            Ok(data_result_value(
                data_kernel::jet_data_quantile_checked(&values, q),
                || data_kernel::jet_data_quantile(&values, q),
                data_float_value,
            ))
        }
        ("core.data", "rolling_mean") => {
            let values = as_float_list(one(0)?, span)?;
            let width = as_int(one(1)?, span)?;
            Ok(data_result_value(
                data_kernel::jet_data_rolling_mean_checked(&values, width),
                || data_kernel::jet_data_rolling_mean(&values, width),
                |means| CtValue::List(means.into_iter().map(data_float_value).collect()),
            ))
        }
        ("core.data", "describe") => {
            let values = as_float_list(one(0)?, span)?;
            Ok(data_result_value_for_type(
                data_kernel::jet_data_describe_checked(&values),
                || data_kernel::jet_data_describe(&values),
                data_summary_value,
                resolved_ret,
            ))
        }
        ("core.data", "status") => Ok(CtValue::List(
            data_kernel::jet_data_status()
                .into_iter()
                .map(|row| CtValue::Struct {
                    type_name: "DataStatus".to_string(),
                    fields: vec![
                        ("step".to_string(), CtValue::Str(row.step)),
                        ("path".to_string(), CtValue::Str(row.path)),
                        ("copy".to_string(), CtValue::Str(row.copy)),
                        ("ownership".to_string(), CtValue::Str(row.ownership)),
                        ("trust".to_string(), CtValue::Str(row.trust)),
                        ("fallback".to_string(), CtValue::Str(row.fallback)),
                        ("replacement".to_string(), CtValue::Str(row.replacement)),
                    ],
                })
                .collect(),
        )),
        ("core.data", "require_bridge") => {
            let provider = as_string(one(0)?, span)?.to_string();
            Ok(match data_kernel::jet_data_require_bridge(&provider) {
                Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                Err(error) => CtValue::failed(Box::new(data_error_value(&error))),
            })
        }
        ("core.data", "bar_text") => {
            let groups = as_data_bar_groups(one(0)?, span)?;
            Ok(data_result_value_for_type(
                data_kernel::jet_data_bar_text_checked(&groups),
                || data_kernel::jet_data_bar_text(&groups),
                CtValue::Str,
                resolved_ret,
            ))
        }
        ("core.data", "bar_svg") => {
            let groups = as_data_bar_groups(one(0)?, span)?;
            Ok(data_result_value_for_type(
                data_kernel::jet_data_bar_svg_checked(&groups),
                || data_kernel::jet_data_bar_svg(&groups),
                CtValue::Str,
                resolved_ret,
            ))
        }
        ("core.data", "line_text" | "line_svg") => {
            apply_data_line_call(method, args, resolved_ret, span)
        }
        // --- impure / build-time I/O → teaching diagnostic (reached only when
        // no #Impure gate intercepts first in eval_method) ---
        ("core.files", _)
        | ("core.sys", _)
        | ("core.term", _)
        | ("core.net", _)
        | ("core.net.tls", _) => Err(Diagnostic::error(
            "E3410",
            format!(
                "`{}.{}()` is a Tier-2 comptime effect — it requires a `#Impure` gate",
                module, method
            ),
            "ambient I/O (filesystem, environment, process) is not allowed in \
                 pure comptime evaluation"
                .to_string(),
            format!(
                "wrap the comptime binding in `#Impure(\"reason\") {{ … }}` and \
                         pass `--gate impure=allow` to the build"
            ),
            Some(span),
        )),
        // --- unknown / not yet implemented ---
        //
        // I4: this adapter serves BOTH comptime folding and the runtime TIR
        // evaluator that default `jet run` deopts into, so the `what` names the
        // construct and never the phase. Saying "at comptime" here described a
        // runtime call site as a compile-time one, which sent readers looking
        // for a `comptime` block that was not in their program. E0956 already
        // renders "isn't supported by the current evaluator yet"; a genuinely
        // compile-time-only refusal is a different row (E3410/E3412).
        _ => {
            if repl_mode {
                if let Some(_) = repl_native_only_module(module) {
                    return Err(repl_native_module_diag(module, method, span));
                }
            }
            Err(unsupported(&format!("`{}.{}()`", module, method), span))
        }
    }
}
