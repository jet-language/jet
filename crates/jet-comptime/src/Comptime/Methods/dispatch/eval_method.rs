use super::*;

impl<'a> Interp<'a> {
    pub(in super::super::super) fn eval_method(
        &mut self,
        receiver: &Expr,
        method: &str,
        span: Span,
        type_args: &[Type],
        recv_type: Option<&str>,
        resolved_ret: Option<&Type>,
        args: &[crate::AST::CallArg],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let sequence_result_ty = resolved_ret.cloned().or_else(|| {
            if !matches!(method, "sum" | "product") {
                return None;
            }
            let Expr::Ident(name, _) = receiver else {
                return None;
            };
            match self.binding_types.get(name) {
                Some(Type::List(inner) | Type::FixedList { elem: inner, .. }) => {
                    Some((**inner).clone())
                }
                _ => None,
            }
        });
        // D-ENC-XML-SURFACE1=A: qualified safe whole-value XML constructors.
        if method == "safe" && args.is_empty() {
            if let Expr::Field(base, type_name, _) = receiver {
                if let Expr::Ident(alias, _) = base.as_ref() {
                    if self.core_imports.get(alias).map(String::as_str) == Some("core.encoding.xml") {
                        if type_name == "XMLLimits" {
                            return Ok(super::super::super::EncodingLite::xml_safe_limits_value());
                        }
                        if type_name == "XMLParseOptions" {
                            return Ok(super::super::super::EncodingLite::xml_safe_options_value());
                        }
                    }
                }
            }
        }
        // D-SOLVER-LIB1=A: `solve.Solver.new(seed)` — module-field sentinel constructor.
        if method == "new" {
            let solver_ctor = match receiver {
                Expr::Ident(type_name, _) if type_name == crate::Syntax::SOLVER_TYPE => true,
                Expr::Field(base, type_name, _)
                    if type_name == crate::Syntax::SOLVER_TYPE
                        && matches!(
                            base.as_ref(),
                            Expr::Ident(alias, _)
                                if self.core_imports.get(alias).map(String::as_str)
                                    == Some("core.solve")
                        ) =>
                {
                    true
                }
                _ => false,
            };
            if solver_ctor {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval(&a.expr, scope)?);
                }
                return solver_new(&argv, span);
            }
        }
        // D-SHAPE-CONVERT1=A: numeric-backed distinct/unit conversion is the
        // existing distinct constructor with destination-owned spelling.
        if let Expr::Ident(type_name, _) = receiver {
            if let Some(range) = self.distinct_ranges.get(type_name).copied() {
                if let Some(base) = self.distinct_bases.get(type_name) {
                    if !base.is_numeric()
                        && crate::Syntax::conversion_method_for_source(&base.name()) == method
                        && args.len() == 1
                    {
                        return self.eval(&args[0].expr, scope);
                    }
                }
                if crate::Syntax::numeric_conversion_source(method).is_some() && args.len() == 1 {
                    let base = self
                        .distinct_bases
                        .get(type_name)
                        .ok_or_else(|| unsupported("a distinct conversion without its base type", span))?;
                    let value = self.eval(&args[0].expr, scope)?;
                    let converted = super::super::super::Builtins::apply_static_type_method(
                        &base.name(),
                        method,
                        vec![value],
                        span,
                    )
                    .ok_or_else(|| unsupported("this distinct numeric conversion", span))??;
                    let check_range = |value: CtValue| -> Result<CtValue, Diagnostic> {
                        let Some((lo, hi)) = range else {
                            return Ok(value);
                        };
                        let n = as_int(&value, args[0].expr.span())?;
                        Ok(if (lo..=hi).contains(&n) {
                            CtValue::ResOk(Box::new(CtValue::Int(n)))
                        } else {
                            CtValue::ResErr(Box::new(CtValue::Str(format!(
                                "{} out of range {}..{}",
                                n, lo, hi
                            ))))
                        })
                    };
                    if let (Some((lo, hi)), Some(n)) = (range, literal_int(&args[0].expr)) {
                        if (lo..=hi).contains(&n) {
                            return Ok(CtValue::Int(n));
                        }
                    }
                    return match converted {
                        CtValue::ResOk(value) if range.is_some() => check_range(*value),
                        CtValue::ResErr(error) if range.is_some() => Ok(CtValue::ResErr(error)),
                        value => check_range(value),
                    };
                }
            }
        }
        if let Expr::Ident(type_name, _) = receiver {
            if type_name == crate::Syntax::MEM_POOL && method == "new" {
                return Ok(super::super::pool::new_value());
            }
            if type_name == crate::Syntax::TYPE_BYTE_BUFFER
                && matches!(method, "new" | "from")
            {
                let bytes = if method == "new" {
                    Vec::new()
                } else {
                    as_bytes(&self.eval(&args[0].expr, scope)?, span)?
                };
                return Ok(CtValue::Struct {
                    type_name: crate::Syntax::TYPE_BYTE_BUFFER.to_string(),
                    fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
                });
            }
            if type_name == crate::Syntax::TYPE_LRU && method == "new" {
                let capacity = as_int(&self.eval(&args[0].expr, scope)?, span)?.max(0);
                return Ok(CtValue::Struct {
                    type_name: crate::Syntax::TYPE_LRU.to_string(),
                    fields: vec![
                        ("capacity".to_string(), CtValue::Int(capacity)),
                        ("entries".to_string(), CtValue::List(Vec::new())),
                    ],
                });
            }
            if type_name == crate::Syntax::TYPE_DEQUE && method == "new" {
                return Ok(CtValue::Struct {
                    type_name: crate::Syntax::TYPE_DEQUE.to_string(),
                    fields: vec![("items".to_string(), CtValue::List(Vec::new()))],
                });
            }
            if type_name == crate::Syntax::TYPE_BIT_SET && method == "new" {
                return Ok(CtValue::Struct {
                    type_name: crate::Syntax::TYPE_BIT_SET.to_string(),
                    fields: vec![("bits".to_string(), CtValue::List(Vec::new()))],
                });
            }
            if type_name == "Bag" && method == "new" {
                return Ok(CtValue::Struct {
                    type_name: "Bag".to_string(),
                    fields: vec![
                        ("items".to_string(), CtValue::List(Vec::new())),
                        ("counts".to_string(), CtValue::List(Vec::new())),
                    ],
                });
            }
            if type_name == crate::Syntax::TYPE_SET && method == "from" {
                let items = match self.eval(&args[0].expr, scope)? {
                    CtValue::List(items) => unique_values(items),
                    _ => return Err(unsupported("Set.from with a non-list", span)),
                };
                return Ok(CtValue::Struct {
                    type_name: crate::Syntax::TYPE_SET.to_string(),
                    fields: vec![("items".to_string(), CtValue::List(items))],
                });
            }
            if type_name == crate::Syntax::TYPE_PRIORITY_QUEUE
                && matches!(method, "new" | "from")
            {
                let items = if method == "new" {
                    Vec::new()
                } else {
                    match self.eval(&args[0].expr, scope)? {
                        CtValue::List(items) => sorted_descending(items, span)?,
                        _ => return Err(unsupported("PriorityQueue.from with a non-list", span)),
                    }
                };
                return Ok(CtValue::Struct {
                    type_name: crate::Syntax::TYPE_PRIORITY_QUEUE.to_string(),
                    fields: vec![("items".to_string(), CtValue::List(items))],
                });
            }
            if type_name == crate::Syntax::TYPE_SORTED_SET
                && matches!(method, "new" | "from")
            {
                let items = if method == "new" {
                    Vec::new()
                } else {
                    match self.eval(&args[0].expr, scope)? {
                        CtValue::List(items) => sorted_unique(items, span)?,
                        _ => return Err(unsupported("SortedSet.from with a non-list", span)),
                    }
                };
                return Ok(CtValue::Struct {
                    type_name: crate::Syntax::TYPE_SORTED_SET.to_string(),
                    fields: vec![("items".to_string(), CtValue::List(items))],
                });
            }
        }
        // c97/D-STRPARSE1: static method on a built-in type name (e.g. `Int.parse(s)`).
        // Check *before* evaluating the receiver so `Int`/`Float` don't fail scope lookup.
        if let Expr::Ident(type_name, _) = receiver {
            if type_name == crate::Syntax::DURATION_TYPE {
                let Some(unit) = crate::Syntax::duration_unit_for_constructor(method) else {
                    return Err(super::super::super::Diagnostics::unsupported(
                        &format!("`{}.{}()`", type_name, method),
                        span,
                    ));
                };
                let scale = match unit {
                    "Milliseconds" => 1,
                    "Seconds" => 1_000,
                    "Minutes" => 60_000,
                    "Hours" => 3_600_000,
                    _ => unreachable!("Syntax returned a closed duration unit"),
                };
                let value = self.eval(&args[0].expr, scope)?;
                let ms = match value {
                    CtValue::Int(n) => n.checked_mul(scale),
                    CtValue::Float(n) => {
                        let scaled = n.as_f64() * scale as f64;
                        (scaled.is_finite()
                            && scaled >= i64::MIN as f64
                            && scaled < 9_223_372_036_854_775_808.0)
                            .then_some(scaled.trunc() as i64)
                    }
                    _ => None,
                };
                return Ok(match ms {
                    Some(ms) => CtValue::ResOk(Box::new(CtValue::Struct {
                        type_name: crate::Syntax::DURATION_TYPE.to_string(),
                        fields: vec![("ms".to_string(), CtValue::Int(ms))],
                    })),
                    None => CtValue::ResErr(Box::new(CtValue::Struct {
                        type_name: crate::Syntax::DURATION_RANGE_ERROR_TYPE.to_string(),
                        fields: vec![(
                            "reason".to_string(),
                            CtValue::Str("duration must be finite and inside the supported range".to_string()),
                        )],
                    })),
                });
            }
            // D-SOLVER-LIB1=A: bare `Solver.new(seed)` (same state as `solve.Solver.new`).
            if type_name == crate::Syntax::SOLVER_TYPE && method == "new" {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval(&a.expr, scope)?);
                }
                return solver_new(&argv, span);
            }
            // Only intercept known built-in type names; user struct names use normal path.
            let is_builtin_type = crate::AST::numeric_type_from_name(type_name).is_some()
                || matches!(type_name.as_str(), "Bool" | "String")
                || super::super::super::MathLayout::is_math_type(type_name);
            if is_builtin_type {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval(&a.expr, scope)?);
                }
                if let Some(result) = apply_static_type_method(type_name, method, argv, span) {
                    return result;
                }
                // If the static dispatch didn't match, fall through to the error below —
                // a built-in type name is not a valid receiver for unknown methods.
                return Err(super::super::super::Diagnostics::unsupported(
                    &format!("`{}.{}()`", type_name, method),
                    span,
                ));
            }
        }
        // D-CTCORE1 (ratified 2026-06-22): module alias calls like `math.sqrt(x)`.
        // Check *before* evaluating the receiver so unknown aliases don't fail.
        if let Expr::Ident(alias, _) = receiver {
            if let Some(module) = self.core_imports.get(alias.as_str()).cloned() {
                // D-DET1: `random.shuffle(&xs)` edits its list in place (E0202 requires
                // write access) — the one `core.random` call that mutates a caller
                // binding rather than returning a value, so it needs `write_back`
                // (the same mechanism `.sort`/`.push` use) instead of the generic
                // by-value `apply_core_call` dispatch below.
                if matches!((module.as_str(), method), ("core.random", "shuffle")) {
                    let Some(arg) = args.first() else {
                        return Err(unsupported("random.shuffle(): missing arg 0", span));
                    };
                    let list = self.eval(&arg.expr, scope)?;
                    let CtValue::List(mut items) = list else {
                        return Err(unsupported("random.shuffle needs a list", span));
                    };
                    if self.repl_mode {
                        let request = super::super::super::ReplEffectRequest {
                            root: "Rand".to_string(),
                            operation: "Draw".to_string(),
                            resource: "shuffle".to_string(),
                        };
                        if !self.repl_grants.iter().any(|cap| cap == "Rand") {
                            return Err(Diagnostic::error(
                                "E1803",
                                "Rand.Draw for `shuffle` has no REPL runtime authority".to_string(),
                                "REPL ambient randomness requires lexical `#Grant(Rand)` authority; the RNG state did not advance".to_string(),
                                "wrap this draw in `#Grant(Rand) { caps -> ... }` and approve it or pass `--allow-rand`".to_string(),
                                Some(span),
                            ));
                        }
                        let Some(authorizer) = self.repl_authorizer.as_deref_mut() else {
                            return Err(Diagnostic::error(
                                "E1803",
                                "Rand.Draw for `shuffle` was denied".to_string(),
                                "this REPL mode has no runtime authority provider; the RNG state did not advance".to_string(),
                                "restart with `jet repl --allow-rand`".to_string(),
                                Some(span),
                            ));
                        };
                        authorizer.authorize(&request, span)?;
                    }
                    with_ambient_rng(|st| shuffle_ct_list(st, &mut items));
                    self.write_back(&arg.expr, CtValue::List(items), scope)?;
                    return Ok(CtValue::Unit);
                }
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval(&a.expr, scope)?);
                }
                // D-FE-REPL-INTERRUPT1=A: poll before and after every Core
                // runtime call. Long host calls stay marked so the raw UI can
                // explain why cancellation has not returned yet.
                self.poll_repl_interrupt();
                let _runtime_call = super::super::super::ReplRuntimeCallGuard::new(self.repl_interruptible);
                // Card #392 pass 5: `core.data`'s typed table/lazy pipeline — a
                // generic call-site-typed surface built from ordinary Jet
                // lambdas over dynamically-typed `CtValue` rows, so (unlike
                // `decode<T>` below) only `csv<T>`/`json<T>` actually read `type_args`.
                // The pre-existing fixed-signature stats/plot surface (`sum`/
                // `mean`/…/`bar_svg`, `DataLite.rs`) stays on the
                // `apply_core_call` path below — only the table/lazy pipeline
                // names are new here.
                if matches!(
                    (module.as_str(), method),
                    (
                        "core.data",
                        "csv" | "json" | "count" | "table" | "rows" | "series" | "values" | "schema"
                            | "missing_count" | "lazy" | "lazy_filter" | "lazy_sort_by" | "collect"
                            | "plan" | "filter" | "sort_by" | "group_count" | "group_sum"
                            | "group_mean" | "inner_join" | "left_join",
                    )
                ) {
                    let arg0_ty = args.first().and_then(|a| match &a.expr {
                        Expr::Ident(name, _) => self.binding_types.get(name).cloned(),
                        Expr::MethodCall {
                            resolved_ret: Some(ty),
                            ..
                        } => Some(ty.clone()),
                        _ => None,
                    });
                    return self.eval_data_call(
                        method,
                        argv,
                        type_args,
                        arg0_ty.as_ref(),
                        resolved_ret,
                        span,
                    );
                }
                // D-ENC-CBOR-SURFACE1: encoding a Codable value needs its
                // declared field types. CtValue intentionally erases `[U8]`
                // to an integer list, so generic by-value dispatch cannot
                // distinguish CBOR byte strings from ordinary arrays.
                if module == "core.encoding.cbor"
                    && matches!(method, "to_bytes" | "to_bytes_canonical")
                {
                    let Some(value) = argv.first() else {
                        return Err(unsupported(
                            "core.encoding.cbor.to_bytes(): missing arg 0",
                            span,
                        ));
                    };
                    return Ok(match super::super::super::EncodingLite::cbor_encode_typed(
                        value,
                        self.structs,
                        method == "to_bytes_canonical",
                    ) {
                        Ok(bytes) => CtValue::ResOk(Box::new(CtValue::Bytes(bytes))),
                        Err(reason) => CtValue::ResErr(Box::new(CtValue::Struct {
                            type_name: "CBORError".to_string(),
                            fields: vec![
                                (
                                    "kind".to_string(),
                                    CtValue::Enum {
                                        type_name: "CBORErrorKind".to_string(),
                                        variant: "Unsupported".to_string(),
                                        args: Vec::new(),
                                    },
                                ),
                                ("byte_offset".to_string(), CtValue::Int(0)),
                                ("path".to_string(), CtValue::Str("$".to_string())),
                                ("reason".to_string(), CtValue::Str(reason)),
                            ],
                        })),
                    });
                }
                // D-MIGRATE3=A / D-SERDE6: `decode<T>`/`decode_traced<T>` — typed
                // Decode dispatch. Untyped `.decode()` (no turbofish, D-JSON3
                // lenient form) keeps its existing `apply_core_call` arm below.
                if matches!(
                    (module.as_str(), method),
                    (
                        "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml" | "core.encoding.yaml",
                        "decode" | "decode_traced",
                    )
                ) && !type_args.is_empty()
                {
                    let Some(text) = argv.first().and_then(|v| match v {
                        CtValue::Str(s) => Some(s.clone()),
                        _ => None,
                    }) else {
                        return Err(unsupported(
                            &format!("`{}.{}()`: expected a string argument", module, method),
                            span,
                        ));
                    };
                    return self.eval_typed_decode(&module, method, &text, &type_args[0], span);
                }
                // D-ENC-CBOR-SURFACE1 / R12: generic whole-value CBOR decode
                // uses the same typed tree walker as every other codec.  Keep
                // this ahead of the compatibility `cbor.decode(DataTree)` arm
                // below: the type argument is the semantic distinction.
                if module == "core.encoding.cbor" && method == "decode" && !type_args.is_empty() {
                    let bytes = match argv.first() {
                        Some(value) => as_bytes(value, span)?,
                        None => return Err(unsupported("core.encoding.cbor.decode(): missing arg 0", span)),
                    };
                    let options = match super::super::super::EncodingLite::cbor_options(argv.get(1)) {
                        Ok(options) => options,
                        Err(error) => {
                            return Ok(CtValue::ResErr(Box::new(
                                super::super::super::EncodingLite::cbor_error_value(error),
                            )))
                        }
                    };
                    let tree = match super::super::super::EncodingLite::cbor_decode(&bytes, &options, true) {
                        Ok(tree) => tree,
                        Err(error) => {
                            return Ok(CtValue::ResErr(Box::new(
                                super::super::super::EncodingLite::cbor_error_value(error),
                            )))
                        }
                    };
                    return match self.typed_decode_top(&type_args[0], &tree, span) {
                        Ok((value, _)) => Ok(CtValue::ResOk(Box::new(value))),
                        Err(error) => {
                            let (path, reason) = match error {
                                CtValue::Struct { fields, .. } => {
                                    let path = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
                                        ("path", CtValue::Str(value)) => Some(value.clone()),
                                        _ => None,
                                    }).unwrap_or_default();
                                    let reason = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
                                        ("reason", CtValue::Str(value)) => Some(value.clone()),
                                        _ => None,
                                    }).unwrap_or_else(|| "CBOR value does not match requested type".to_string());
                                    let path = if path.is_empty() {
                                        "$".to_string()
                                    } else {
                                        format!("${path}")
                                    };
                                    (path, reason)
                                }
                                _ => (String::new(), "CBOR value does not match requested type".to_string()),
                            };
                            Ok(CtValue::ResErr(Box::new(CtValue::Struct {
                                type_name: "CBORError".to_string(),
                                fields: vec![
                                    ("kind".to_string(), CtValue::Enum {
                                        type_name: "CBORErrorKind".to_string(),
                                        variant: "TypeMismatch".to_string(),
                                        args: Vec::new(),
                                    }),
                                    ("byte_offset".to_string(), CtValue::Int(0)),
                                    ("path".to_string(), CtValue::Str(path)),
                                    ("reason".to_string(), CtValue::Str(reason)),
                                ],
                            })))
                        }
                    };
                }
                // D-CTEFFECT1 Tier-1: fetch is hermetic (sha256-pinned); no gate.
                if module == "core.net" && method == "fetch" {
                    return self.eval_net_fetch(argv, span);
                }
                // U13 (D-JPK-SECRETCRYPTO1): `core.vault.get` is denied at build time
                // unconditionally — unlike the Tier-2 effects below, there is no
                // `#Impure`/`--allow-impure` escape hatch, because a build artifact
                // must never bake in a decrypted secret (I1).
                if module == "core.vault" {
                    return Err(Diagnostic::error(
                        "E1265",
                        format!("`{}.{}()` can't be reached from a build-time context", module, method),
                        "module-field/comptime evaluation runs before secrets are ever decrypted; \
                         a repo's encrypted store is only ever opened at ordinary runtime, and — \
                         unlike the Tier-2 comptime effect gate — there is no `#Impure` escape hatch \
                         here.".to_string(),
                        "move the secret read out of comptime/module-field evaluation and into \
                         ordinary runtime code.".to_string(),
                        Some(span),
                    ));
                }
                // D-CTEFFECT1: Tier-2 effect calls require an #Impure gate (or REPL sandbox).
                let is_tier2 = (matches!(
                    module.as_str(),
                    "core.files"
                        | "core.env"
                        | "core.io"
                        | "core.exec"
                        | "core.net"
                        | "core.tls"
                        | "core.process"
                ) && !is_pure_tier2_call(&module, method))
                    || (self.repl_mode && module == "core.random" && method != "rng");
                if is_tier2 {
                    if self.repl_mode && matches!((module.as_str(), method), ("core.io", "eprint")) {
                        return apply_impure_core_call(
                            &module,
                            method,
                            argv,
                            span,
                            self.base_dir,
                            self.sink.as_deref_mut(),
                            true,
                            None,
                            None,
                        );
                    }
                    let ambient_random = self.repl_mode
                        && module == "core.random"
                        && !matches!(method, "rng");
                    if ambient_random {
                        // Ambient draws and global seeding consume/mutate session RNG state.
                        // Explicit `random.rng(seed)` is injected data and stays pure.
                    }
                    let mut repl_executable = None;
                    let mut repl_root = None;
                    if self.repl_mode {
                        if matches!((module.as_str(), method), ("core.process", "run")) {
                            repl_executable = Some(pin_repl_command(&mut argv, self.base_dir, span)?);
                        }
                        let request = repl_effect_request(&module, method, &argv);
                        let Some(authorizer) = self.repl_authorizer.as_deref_mut() else {
                            return Err(Diagnostic::error(
                                "E1803",
                                format!("{}.{} for `{}` was denied", request.root, request.operation, request.resource),
                                "this REPL mode has no runtime authority provider, so the host operation did not run".to_string(),
                                format!("restart with `jet repl --allow-{}` or use an interactive session and approve the exact operation", request.root.to_ascii_lowercase()),
                                Some(span),
                            ));
                        };
                        authorizer.preflight(&request, span)?;
                        let granted = self.repl_grants.iter().any(|cap| {
                            cap == &request.root || cap.starts_with(&format!("{}.", request.root))
                        });
                        if !granted {
                            return Err(Diagnostic::error(
                                "E1803",
                                format!("{}.{} for `{}` has no REPL runtime authority", request.root, request.operation, request.resource),
                                "REPL host effects require both lexical `#Grant` authority and invocation policy; no host operation ran".to_string(),
                                format!("wrap this operation in `#Grant({}) {{ caps -> ... }}`; interactive sessions then prompt, while non-TTY sessions also need `--allow-{}`", request.root, request.root.to_ascii_lowercase()),
                                Some(span),
                            ));
                        }
                        authorizer.authorize(&request, span)?;
                        if module == "core.files" {
                            return apply_repl_fs_call(method, &argv, span, authorizer);
                        }
                        if ambient_random {
                            return apply_core_call(&module, method, argv, span, true);
                        }
                        if module == "core.process" && method == "run" {
                            repl_root = Some(authorizer.verified_root().map_err(|error| {
                                unsupported(&format!("REPL project root handle is unavailable: {error}"), span)
                            })?);
                        }
                    }
                    if self.impure_depth == 0 {
                        if self.repl_mode {
                            unreachable!("REPL lexical grant checked above");
                        }
                        return Err(Diagnostic::error(
                            "E3410",
                            format!("`{}.{}()` is a Tier-2 comptime effect — it requires a `#Impure` gate", module, method),
                            "ambient I/O (filesystem, environment, process) is not allowed in \
                             pure comptime evaluation".to_string(),
                            "wrap the comptime binding in `#Impure(\"reason\") { … }` and \
                             pass `--allow-impure` to the build".to_string(),
                            Some(span),
                        ));
                    }
                    // Gate present (impure_depth > 0) but check --allow-impure flag too.
                    if !self.allow_impure {
                        return Err(Diagnostic::error(
                            "E3411",
                            format!("`{}.{}()` inside `#Impure` gate, but `--allow-impure` was not passed", module, method),
                            "the `#Impure` block opts in to ambient comptime I/O, but the build \
                             flag is required so CI can audit builds that touch the host".to_string(),
                            "add `--allow-impure` to your `jet build` / `jet run` invocation".to_string(),
                            Some(span),
                        ));
                    }
                    return apply_impure_core_call(
                        &module,
                        method,
                        argv,
                        span,
                        self.base_dir,
                            self.sink.as_deref_mut(),
                            self.repl_mode,
                            repl_executable.as_ref(),
                            repl_root.as_ref(),
                        );
                }
                if matches!((module.as_str(), method), ("core.data", "pivot_sum")) {
                    return self.eval_pivot_sum(argv, span);
                }
                return apply_core_call(&module, method, argv, span, self.repl_mode);
            }
        }

        // Enum-variant construction with a payload, called with an explicit type
        // name (`ParseError.BadDigit(raw)`), mirrors the no-arg `Field` fallback
        // below in interp.rs (`Color.Red`): sema already checked the variant
        // exists, so at eval time an unbound, capitalized receiver whose method
        // is also capitalized (variant-naming convention, S34) is a variant
        // literal, not a real method call. Checked after the builtin-type and
        // core-import cases above, and only when the receiver isn't a bound
        // local, so real static dispatch on a bound value is unaffected.
        if let Expr::Ident(type_name, _) = receiver {
            let is_type_name = type_name.chars().next().is_some_and(|c| c.is_uppercase());
            let is_variant_name = method.chars().next().is_some_and(|c| c.is_uppercase());
            if is_type_name && is_variant_name && !scope.contains_key(type_name.as_str()) {
                let mut out = Vec::with_capacity(args.len());
                for a in args {
                    let label = a.label.as_ref().map(|(n, _)| n.clone());
                    out.push((label, self.eval(&a.expr, scope)?));
                }
                return Ok(CtValue::Enum {
                    type_name: type_name.clone(),
                    variant: method.to_string(),
                    args: out,
                });
            }
        }

        // c139 JIT/interpreter-parity: a static/associated call (`Type.assoc_fn(…)`,
        // no `self` param — `impl`/in-struct methods registered in
        // `self.methods`) or a D-MOD2 code-module namespaced call
        // (`alias.fn(…)` — registered `alias__fn` in `self.funcs`), reached via
        // an unbound name. A real instance receiver is always a scoped value,
        // so it never lands here. Checked after the enum-variant-literal case
        // above (capitalized `Type.Variant(…)` wins first).
        if let Expr::Ident(name, _) = receiver {
            if !scope.contains_key(name.as_str()) {
                if let Some(f) = self
                    .methods
                    .get(&(name.clone(), method.to_string()))
                    .copied()
                {
                    if f.params.len() == args.len() {
                        let mut frame = HashMap::new();
                        for (p, a) in f.params.iter().zip(args) {
                            let v = self.eval(&a.expr, scope)?;
                            frame.insert(p.name.clone(), v);
                        }
                        return self.call_func(&format!("{}.{}", name, method), f, frame);
                    }
                }
                let qualified = format!("{name}::{method}");
                if let Some(f) = self.funcs.get(qualified.as_str()).copied() {
                    if f.params.len() == args.len() {
                        let mut frame = HashMap::new();
                        for (p, a) in f.params.iter().zip(args) {
                            let v = self.eval(&a.expr, scope)?;
                            frame.insert(p.name.clone(), v);
                        }
                        return self.call_func(&qualified, f, frame);
                    }
                }
                let mangled = format!("{}__{}", name, method);
                if let Some(f) = self.funcs.get(mangled.as_str()).copied() {
                    if f.params.len() == args.len() {
                        let mut frame = HashMap::new();
                        for (p, a) in f.params.iter().zip(args) {
                            let v = self.eval(&a.expr, scope)?;
                            frame.insert(p.name.clone(), v);
                        }
                        return self.call_func(&mangled, f, frame);
                    }
                }
            }
        }

        // D-HOLE1: `Option.lift2(f, a, b)` — a static call on the builtin
        // `Option` pseudo-type (never a real scoped value), so it's checked
        // here alongside the unbound-name case above rather than folded into
        // it (`Option` isn't in `self.methods`/`self.funcs`, and `f` is a
        // closure this dispatch must itself invoke).
        if let Expr::Ident(name, _) = receiver {
            if name == "Option" && method == "lift2" && !scope.contains_key("Option") {
                if args.len() != 3 {
                    return Err(unsupported(
                        "`Option.lift2` (wrong number of arguments)",
                        span,
                    ));
                }
                let f = self.eval(&args[0].expr, scope)?;
                let a = self.eval(&args[1].expr, scope)?;
                let b = self.eval(&args[2].expr, scope)?;
                return Ok(match (a, b) {
                    (CtValue::Some(av), CtValue::Some(bv)) => {
                        CtValue::Some(Box::new(self.call_inline_closure(
                            &f,
                            vec![*av, *bv],
                            span,
                            scope,
                        )?))
                    }
                    _ => CtValue::None(Type::Int),
                });
            }
        }

        // c139: higher-order methods — need `&mut self` to invoke a closure
        // argument, so (like the mutating methods just below) they can't be
        // plain `apply_method` entries. Guarded to the receiver shapes they
        // actually apply to; anything else falls through to the generic
        // dispatch at the end of this function.
        const HOF_METHODS: &[&str] = &["filter", "map", "each", "sort_by", "find"];
        if HOF_METHODS.contains(&method) {
            let recv = self.eval(receiver, scope)?;
            match (&recv, method) {
                (CtValue::List(xs), "filter") => {
                    let f = self.eval(&args[0].expr, scope)?;
                    let mut out = Vec::new();
                    for x in xs {
                        if as_bool(
                            &self.call_inline_closure(&f, vec![x.clone()], span, scope)?,
                            span,
                        )? {
                            out.push(x.clone());
                        }
                    }
                    return Ok(CtValue::List(out));
                }
                (CtValue::List(xs), "map") => {
                    let f = self.eval(&args[0].expr, scope)?;
                    let mut out = Vec::with_capacity(xs.len());
                    for x in xs {
                        out.push(self.call_inline_closure(
                            &f,
                            vec![x.clone()],
                            span,
                            scope,
                        )?);
                    }
                    return Ok(CtValue::List(out));
                }
                (CtValue::List(xs), "each") => {
                    let f = self.eval(&args[0].expr, scope)?;
                    for x in xs {
                        self.call_inline_closure(&f, vec![x.clone()], span, scope)?;
                    }
                    return Ok(CtValue::Unit);
                }
                (CtValue::Map(entries), "each") => {
                    let f = self.eval(&args[0].expr, scope)?;
                    for (key, value) in entries {
                        self.call_inline_closure(
                            &f,
                            vec![key.to_value(), value.clone()],
                            span,
                            scope,
                        )?;
                    }
                    return Ok(CtValue::Unit);
                }
                (CtValue::List(xs), "find") => {
                    let f = self.eval(&args[0].expr, scope)?;
                    for x in xs {
                        if as_bool(
                            &self.call_inline_closure(&f, vec![x.clone()], span, scope)?,
                            span,
                        )? {
                            return Ok(CtValue::Some(Box::new(x.clone())));
                        }
                    }
                    return Ok(CtValue::None(Type::Int));
                }
                // `.sort_by` writes back like the MUTATING list methods below
                // (D-BIND4 `:=` receiver) — key every element once, sort the
                // keyed pairs, then write the reordered list back through the
                // same lvalue path `push`/`pop`/… use.
                (CtValue::List(xs), "sort_by")
                    if matches!(receiver, Expr::Ident(..) | Expr::Field(..)) =>
                {
                    let f = self.eval(&args[0].expr, scope)?;
                    let mut keyed = Vec::with_capacity(xs.len());
                    for x in xs {
                        let k = self.call_inline_closure(
                            &f,
                            vec![x.clone()],
                            span,
                            scope,
                        )?;
                        keyed.push((k, x.clone()));
                    }
                    let mut sort_err = None;
                    keyed.sort_by(|a, b| match cmp(a.0.clone(), b.0.clone(), span) {
                        Ok(o) => o,
                        Err(e) => {
                            sort_err.get_or_insert(e);
                            std::cmp::Ordering::Equal
                        }
                    });
                    if let Some(e) = sort_err {
                        return Err(e);
                    }
                    let sorted = CtValue::List(keyed.into_iter().map(|(_, v)| v).collect());
                    self.write_back(receiver, sorted, scope)?;
                    return Ok(CtValue::Unit);
                }
                (CtValue::Some(inner), "map") => {
                    let f = self.eval(&args[0].expr, scope)?;
                    let v = self.call_inline_closure(
                        &f,
                        vec![(**inner).clone()],
                        span,
                        scope,
                    )?;
                    return Ok(CtValue::Some(Box::new(v)));
                }
                (CtValue::None(t), "map") => return Ok(CtValue::None(t.clone())),
                _ => {}
            }
        }

        // D-ANY-JAI1: `reflect.of(x).display()` — same Display-impl-aware
        // rendering `{x}`/`print(x)` use (`show_value`), so it needs `&mut
        // self` and can't be a plain `apply_method` entry like `.type_name`/
        // `.fields` just above it. Guarded specifically to the `__Reflect`
        // wrapper — a *user* type's own `.display()` call (the method a
        // `impl Type.Display` block defines) still falls through to the
        // ordinary instance-method dispatch below, unaffected.
        if method == "display" {
            let peek = self.eval(receiver, scope)?;
            if let CtValue::Struct { type_name, fields } = &peek {
                if type_name == "__Reflect" {
                    let inner = fields
                        .iter()
                        .find(|(n, _)| n == "value")
                        .map(|(_, v)| v.clone())
                        .unwrap_or(CtValue::Unit);
                    let s = self.show_value(&inner, span)?;
                    return Ok(CtValue::Str(s));
                }
            }
        }

        let mut evaluated_receiver = None;
        if matches!(method, "add" | "remove" | "ids") {
            let recv = self.eval(receiver, scope)?;
            if super::super::pool::is_method(&recv, method) {
                let mut argv = Vec::with_capacity(args.len());
                for arg in args {
                    argv.push(self.eval(&arg.expr, scope)?);
                }
                let outcome = super::super::pool::apply(&recv, method, &argv, resolved_ret, span)?;
                if let Some(updated) = outcome.updated {
                    if !matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
                        return Err(unsupported("Pool mutation on a temporary value", span));
                    }
                    self.write_back(receiver, updated, scope)?;
                }
                return Ok(outcome.value);
            }
            evaluated_receiver = Some(recv);
        }
        if matches!(
            method,
            "add"
                | "add_new"
                | "get"
                | "remove"
                | "has_key"
                | "keys"
                | "capacity"
                | "count"
                | "len"
                | "is_empty"
                | "clear"
                | "push"
                | "pop"
                | "peek"
                | "to_sorted_list"
                | "push_front"
                | "push_back"
                | "pop_front"
                | "pop_back"
                | "peek_front"
                | "peek_back"
                | "has"
                | "first"
                | "last"
                | "union"
                | "to_list"
                | "any"
        ) {
            let peek = match &evaluated_receiver {
                Some(value) => value.clone(),
                None => self.eval(receiver, scope)?,
            };
            match (&peek, method) {
                (
                    CtValue::Struct { type_name, fields },
                    method @ ("add" | "remove" | "has" | "count" | "len" | "is_empty" | "any"),
                ) if type_name == "Bag" => {
                    // ponytail: comptime bags are small; parallel equality-only
                    // vectors support every CtValue without a second hash model.
                    let mut items = fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("items", CtValue::List(items)) => Some(items.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let mut counts = fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("counts", CtValue::List(counts)) => Some(counts.clone()),
                            _ => None,
                        })
                        .unwrap_or_else(|| vec![CtValue::Int(1); items.len()]);
                    let mut argv = Vec::with_capacity(args.len());
                    for arg in args {
                        argv.push(self.eval(&arg.expr, scope)?);
                    }
                    let mut changed = false;
                    let result = match method {
                        "len" => CtValue::Int(
                            counts
                                .iter()
                                .filter_map(|count| match count {
                                    CtValue::Int(count) => Some(*count),
                                    _ => None,
                                })
                                .sum(),
                        ),
                        "is_empty" => CtValue::Bool(items.is_empty()),
                        "has" => CtValue::Bool(items.contains(&argv[0])),
                        "count" => CtValue::Int(
                            items
                                .iter()
                                .position(|item| item == &argv[0])
                                .and_then(|index| match counts.get(index) {
                                    Some(CtValue::Int(count)) => Some(*count),
                                    _ => None,
                                })
                                .unwrap_or(0),
                        ),
                        "any" => {
                            let mut found = false;
                            for item in &items {
                                if as_bool(
                                    &self.call_inline_closure(
                                        &argv[0],
                                        vec![item.clone()],
                                        span,
                                        scope,
                                    )?,
                                    span,
                                )? {
                                    found = true;
                                    break;
                                }
                            }
                            CtValue::Bool(found)
                        }
                        "add" => {
                            if let Some(index) = items.iter().position(|item| item == &argv[0]) {
                                if let Some(CtValue::Int(count)) = counts.get_mut(index) {
                                    *count += 1;
                                }
                            } else {
                                items.push(argv[0].clone());
                                counts.push(CtValue::Int(1));
                            }
                            changed = true;
                            CtValue::Bool(true)
                        }
                        "remove" => {
                            if let Some(index) = items.iter().position(|item| item == &argv[0]) {
                                let last = matches!(counts.get(index), Some(CtValue::Int(1)));
                                if last {
                                    items.remove(index);
                                    counts.remove(index);
                                } else if let Some(CtValue::Int(count)) = counts.get_mut(index) {
                                    *count -= 1;
                                }
                                changed = true;
                            }
                            CtValue::Unit
                        }
                        _ => unreachable!("Bag method set is closed"),
                    };
                    if changed && matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
                        self.write_back(
                            receiver,
                            CtValue::Struct {
                                type_name: "Bag".to_string(),
                                fields: vec![
                                    ("items".to_string(), CtValue::List(items)),
                                    ("counts".to_string(), CtValue::List(counts)),
                                ],
                            },
                            scope,
                        )?;
                    }
                    return Ok(result);
                }
                (
                    CtValue::Struct { type_name, fields },
                    method @ ("add"
                        | "remove"
                        | "has"
                        | "union"
                        | "to_list"
                        | "len"
                        | "is_empty"
                        | "clear"),
                ) if type_name == crate::Syntax::TYPE_SET => {
                    let mut items = fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("items", CtValue::List(items)) => Some(items.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let mut argv = Vec::with_capacity(args.len());
                    for arg in args {
                        argv.push(self.eval(&arg.expr, scope)?);
                    }
                    let mut changed = false;
                    let result = match method {
                        "len" => CtValue::Int(items.len() as i64),
                        "is_empty" => CtValue::Bool(items.is_empty()),
                        "has" => CtValue::Bool(items.contains(&argv[0])),
                        "to_list" => CtValue::List(items.clone()),
                        "add" => {
                            let added = !items.contains(&argv[0]);
                            if added {
                                items.push(argv[0].clone());
                                changed = true;
                            }
                            CtValue::Bool(added)
                        }
                        "remove" => {
                            if let Some(index) = items.iter().position(|item| item == &argv[0]) {
                                items.remove(index);
                                changed = true;
                            }
                            CtValue::Unit
                        }
                        "union" => {
                            let CtValue::Struct {
                                type_name: other_type,
                                fields: other_fields,
                            } = &argv[0]
                            else {
                                return Err(unsupported("Set.union with a non-set", span));
                            };
                            if other_type != crate::Syntax::TYPE_SET {
                                return Err(unsupported("Set.union with a non-set", span));
                            }
                            if let Some(CtValue::List(other)) = other_fields
                                .iter()
                                .find(|(name, _)| name == "items")
                                .map(|(_, value)| value)
                            {
                                items.extend(other.iter().cloned());
                            }
                            CtValue::Struct {
                                type_name: crate::Syntax::TYPE_SET.to_string(),
                                fields: vec![(
                                    "items".to_string(),
                                    CtValue::List(unique_values(items.clone())),
                                )],
                            }
                        }
                        "clear" => {
                            items.clear();
                            changed = true;
                            CtValue::Unit
                        }
                        _ => unreachable!("Set method set is closed"),
                    };
                    if changed && matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
                        self.write_back(
                            receiver,
                            CtValue::Struct {
                                type_name: crate::Syntax::TYPE_SET.to_string(),
                                fields: vec![("items".to_string(), CtValue::List(items))],
                            },
                            scope,
                        )?;
                    }
                    return Ok(result);
                }
                (
                    CtValue::Struct { type_name, fields },
                    method @ ("push"
                        | "pop"
                        | "peek"
                        | "to_sorted_list"
                        | "len"
                        | "is_empty"
                        | "clear"),
                ) if type_name == crate::Syntax::TYPE_PRIORITY_QUEUE => {
                    let mut items = fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("items", CtValue::List(items)) => Some(items.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let mut argv = Vec::with_capacity(args.len());
                    for arg in args {
                        argv.push(self.eval(&arg.expr, scope)?);
                    }
                    let option_none = || {
                        CtValue::None(match resolved_ret {
                            Some(Type::Option(inner)) => (**inner).clone(),
                            _ => Type::Int,
                        })
                    };
                    let mut changed = false;
                    let result = match method {
                        "len" => CtValue::Int(items.len() as i64),
                        "is_empty" => CtValue::Bool(items.is_empty()),
                        "peek" => items
                            .first()
                            .cloned()
                            .map_or_else(option_none, |value| CtValue::Some(Box::new(value))),
                        "to_sorted_list" => CtValue::List(items.clone()),
                        "push" => {
                            items.push(argv[0].clone());
                            items = sorted_descending(items, span)?;
                            changed = true;
                            CtValue::Unit
                        }
                        "pop" if items.is_empty() => option_none(),
                        "pop" => {
                            changed = true;
                            CtValue::Some(Box::new(items.remove(0)))
                        }
                        "clear" => {
                            items.clear();
                            changed = true;
                            CtValue::Unit
                        }
                        _ => unreachable!("PriorityQueue method set is closed"),
                    };
                    if changed && matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
                        self.write_back(
                            receiver,
                            CtValue::Struct {
                                type_name: crate::Syntax::TYPE_PRIORITY_QUEUE.to_string(),
                                fields: vec![("items".to_string(), CtValue::List(items))],
                            },
                            scope,
                        )?;
                    }
                    return Ok(result);
                }
                (
                    CtValue::Struct { type_name, fields },
                    method @ ("add"
                        | "remove"
                        | "has"
                        | "count"
                        | "len"
                        | "is_empty"
                        | "clear"
                        | "to_list"),
                ) if type_name == crate::Syntax::TYPE_BIT_SET => {
                    let mut bits = fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("bits", CtValue::List(bits)) => Some(bits.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let mut argv = Vec::with_capacity(args.len());
                    for arg in args {
                        argv.push(self.eval(&arg.expr, scope)?);
                    }
                    let mut changed = false;
                    let result = match method {
                        "count" => CtValue::Int(bits.len() as i64),
                        "len" => CtValue::Int(match bits.last() {
                            Some(CtValue::Int(bit)) => bit + 1,
                            _ => 0,
                        }),
                        "is_empty" => CtValue::Bool(bits.is_empty()),
                        "has" => CtValue::Bool(
                            bits.contains(&CtValue::Int(as_int(&argv[0], span)?)),
                        ),
                        "to_list" => CtValue::List(bits.clone()),
                        "add" => {
                            let bit = as_int(&argv[0], span)?;
                            let added = bit >= 0 && !bits.contains(&CtValue::Int(bit));
                            if added {
                                bits.push(CtValue::Int(bit));
                                bits = sorted_unique(bits, span)?;
                                changed = true;
                            }
                            CtValue::Bool(added)
                        }
                        "remove" => {
                            let bit = CtValue::Int(as_int(&argv[0], span)?);
                            if let Some(index) = bits.iter().position(|value| value == &bit) {
                                bits.remove(index);
                                changed = true;
                            }
                            CtValue::Unit
                        }
                        "clear" => {
                            bits.clear();
                            changed = true;
                            CtValue::Unit
                        }
                        _ => unreachable!("BitSet method set is closed"),
                    };
                    if changed && matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
                        self.write_back(
                            receiver,
                            CtValue::Struct {
                                type_name: crate::Syntax::TYPE_BIT_SET.to_string(),
                                fields: vec![("bits".to_string(), CtValue::List(bits))],
                            },
                            scope,
                        )?;
                    }
                    return Ok(result);
                }
                (
                    CtValue::Struct { type_name, fields },
                    method @ ("add"
                        | "remove"
                        | "has"
                        | "first"
                        | "last"
                        | "union"
                        | "to_list"
                        | "len"
                        | "is_empty"
                        | "clear"),
                ) if type_name == crate::Syntax::TYPE_SORTED_SET => {
                    let mut items = fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("items", CtValue::List(items)) => Some(items.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let mut argv = Vec::with_capacity(args.len());
                    for arg in args {
                        argv.push(self.eval(&arg.expr, scope)?);
                    }
                    let option_none = || {
                        CtValue::None(match resolved_ret {
                            Some(Type::Option(inner)) => (**inner).clone(),
                            _ => Type::Int,
                        })
                    };
                    let mut changed = false;
                    let result = match method {
                        "len" => CtValue::Int(items.len() as i64),
                        "is_empty" => CtValue::Bool(items.is_empty()),
                        "has" => CtValue::Bool(items.contains(&argv[0])),
                        "first" => items
                            .first()
                            .cloned()
                            .map_or_else(option_none, |value| CtValue::Some(Box::new(value))),
                        "last" => items
                            .last()
                            .cloned()
                            .map_or_else(option_none, |value| CtValue::Some(Box::new(value))),
                        "to_list" => CtValue::List(items.clone()),
                        "add" => {
                            let added = !items.contains(&argv[0]);
                            if added {
                                items.push(argv[0].clone());
                                items = sorted_unique(items, span)?;
                                changed = true;
                            }
                            CtValue::Bool(added)
                        }
                        "remove" => {
                            if let Some(index) = items.iter().position(|item| item == &argv[0]) {
                                items.remove(index);
                                changed = true;
                            }
                            CtValue::Unit
                        }
                        "union" => {
                            let CtValue::Struct {
                                type_name: other_type,
                                fields: other_fields,
                            } = &argv[0]
                            else {
                                return Err(unsupported("SortedSet.union with a non-set", span));
                            };
                            if other_type != crate::Syntax::TYPE_SORTED_SET {
                                return Err(unsupported("SortedSet.union with a non-set", span));
                            }
                            if let Some(CtValue::List(other)) = other_fields
                                .iter()
                                .find(|(name, _)| name == "items")
                                .map(|(_, value)| value)
                            {
                                items.extend(other.iter().cloned());
                            }
                            CtValue::Struct {
                                type_name: crate::Syntax::TYPE_SORTED_SET.to_string(),
                                fields: vec![(
                                    "items".to_string(),
                                    CtValue::List(sorted_unique(items.clone(), span)?),
                                )],
                            }
                        }
                        "clear" => {
                            items.clear();
                            changed = true;
                            CtValue::Unit
                        }
                        _ => unreachable!("SortedSet method set is closed"),
                    };
                    if changed && matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
                        self.write_back(
                            receiver,
                            CtValue::Struct {
                                type_name: crate::Syntax::TYPE_SORTED_SET.to_string(),
                                fields: vec![("items".to_string(), CtValue::List(items))],
                            },
                            scope,
                        )?;
                    }
                    return Ok(result);
                }
                (
                    CtValue::Struct { type_name, fields },
                    method @ ("push_front"
                        | "push_back"
                        | "pop_front"
                        | "pop_back"
                        | "peek_front"
                        | "peek_back"
                        | "len"
                        | "is_empty"
                        | "clear"),
                ) if type_name == crate::Syntax::TYPE_DEQUE => {
                    let mut items = fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("items", CtValue::List(items)) => Some(items.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let mut argv = Vec::with_capacity(args.len());
                    for arg in args {
                        argv.push(self.eval(&arg.expr, scope)?);
                    }
                    let option_none = || {
                        CtValue::None(match resolved_ret {
                            Some(Type::Option(inner)) => (**inner).clone(),
                            _ => Type::Int,
                        })
                    };
                    let mut changed = false;
                    let result = match method {
                        "len" => CtValue::Int(items.len() as i64),
                        "is_empty" => CtValue::Bool(items.is_empty()),
                        "peek_front" => items
                            .first()
                            .cloned()
                            .map_or_else(option_none, |value| CtValue::Some(Box::new(value))),
                        "peek_back" => items
                            .last()
                            .cloned()
                            .map_or_else(option_none, |value| CtValue::Some(Box::new(value))),
                        "push_front" => {
                            items.insert(0, argv[0].clone());
                            changed = true;
                            CtValue::Unit
                        }
                        "push_back" => {
                            items.push(argv[0].clone());
                            changed = true;
                            CtValue::Unit
                        }
                        "pop_front" if items.is_empty() => option_none(),
                        "pop_front" => {
                            changed = true;
                            CtValue::Some(Box::new(items.remove(0)))
                        }
                        "pop_back" => match items.pop() {
                            Some(value) => {
                                changed = true;
                                CtValue::Some(Box::new(value))
                            }
                            None => option_none(),
                        },
                        "clear" => {
                            items.clear();
                            changed = true;
                            CtValue::Unit
                        }
                        _ => unreachable!("Deque method set is closed"),
                    };
                    if changed && matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
                        self.write_back(
                            receiver,
                            CtValue::Struct {
                                type_name: crate::Syntax::TYPE_DEQUE.to_string(),
                                fields: vec![("items".to_string(), CtValue::List(items))],
                            },
                            scope,
                        )?;
                    }
                    return Ok(result);
                }
                (
                    CtValue::Struct { type_name, fields },
                    method @ ("add"
                        | "add_new"
                        | "get"
                        | "remove"
                        | "has_key"
                        | "keys"
                        | "capacity"
                        | "len"
                        | "is_empty"
                        | "clear"),
                ) if type_name == crate::Syntax::TYPE_LRU => {
                    let capacity = fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("capacity", CtValue::Int(capacity)) => Some(*capacity as usize),
                            _ => None,
                        })
                        .unwrap_or(0);
                    let mut entries = fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("entries", CtValue::List(entries)) => Some(entries.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let mut argv = Vec::with_capacity(args.len());
                    for arg in args {
                        argv.push(self.eval(&arg.expr, scope)?);
                    }
                    let option_none = || {
                        CtValue::None(match resolved_ret {
                            Some(Type::Option(inner)) => (**inner).clone(),
                            _ => Type::Int,
                        })
                    };
                    let key_position = |entries: &[CtValue], key: &CtValue| {
                        entries.iter().position(|entry| {
                            matches!(entry, CtValue::List(pair) if pair.first() == Some(key))
                        })
                    };
                    let mut changed = false;
                    let result = match method {
                        "len" => CtValue::Int(entries.len() as i64),
                        "is_empty" => CtValue::Bool(entries.is_empty()),
                        "capacity" => CtValue::Int(capacity as i64),
                        "has_key" => CtValue::Bool(key_position(&entries, &argv[0]).is_some()),
                        "keys" => CtValue::List(
                            entries
                                .iter()
                                .filter_map(|entry| match entry {
                                    CtValue::List(pair) => pair.first().cloned(),
                                    _ => None,
                                })
                                .collect(),
                        ),
                        "clear" => {
                            entries.clear();
                            changed = true;
                            CtValue::Unit
                        }
                        "add_new" => {
                            let added = capacity > 0 && key_position(&entries, &argv[0]).is_none();
                            if added {
                                entries.insert(
                                    0,
                                    CtValue::List(vec![argv[0].clone(), argv[1].clone()]),
                                );
                                if entries.len() > capacity {
                                    entries.pop();
                                }
                                changed = true;
                            }
                            CtValue::Bool(added)
                        }
                        "add" => {
                            if capacity == 0 {
                                option_none()
                            } else {
                                let displaced = key_position(&entries, &argv[0]).map(|index| {
                                    let CtValue::List(pair) = entries.remove(index) else {
                                        unreachable!("Lru entries are pairs")
                                    };
                                    pair[1].clone()
                                });
                                entries.insert(
                                    0,
                                    CtValue::List(vec![argv[0].clone(), argv[1].clone()]),
                                );
                                if entries.len() > capacity {
                                    entries.pop();
                                }
                                changed = true;
                                displaced.map_or_else(option_none, |value| {
                                    CtValue::Some(Box::new(value))
                                })
                            }
                        }
                        "get" => match key_position(&entries, &argv[0]) {
                            Some(index) => {
                                let entry = entries.remove(index);
                                let CtValue::List(pair) = &entry else {
                                    unreachable!("Lru entries are pairs")
                                };
                                let value = pair[1].clone();
                                entries.insert(0, entry);
                                changed = true;
                                CtValue::Some(Box::new(value))
                            }
                            None => option_none(),
                        },
                        "remove" => match key_position(&entries, &argv[0]) {
                            Some(index) => {
                                let CtValue::List(pair) = entries.remove(index) else {
                                    unreachable!("Lru entries are pairs")
                                };
                                changed = true;
                                CtValue::Some(Box::new(pair[1].clone()))
                            }
                            None => option_none(),
                        },
                        _ => unreachable!("Lru method set is closed"),
                    };
                    if changed && matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
                        self.write_back(
                            receiver,
                            CtValue::Struct {
                                type_name: crate::Syntax::TYPE_LRU.to_string(),
                                fields: vec![
                                    ("capacity".to_string(), CtValue::Int(capacity as i64)),
                                    ("entries".to_string(), CtValue::List(entries)),
                                ],
                            },
                            scope,
                        )?;
                    }
                    return Ok(result);
                }
                _ => {}
            }
            evaluated_receiver = Some(peek);
        }

        // Mutating list/map methods on a named variable write back in place.
        const MUTATING: &[&str] = &[
            "push", "pop", "insert", "add", "add_new", "remove", "clear", "reverse", "sort",
        ];
        if MUTATING.contains(&method) && matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
            let mut container = match &evaluated_receiver {
                Some(value) => value.clone(),
                None => self.eval(receiver, scope)?,
            };
            let handled_here = matches!(&container, CtValue::List(_) | CtValue::Map(_))
                || matches!(
                    &container,
                    CtValue::Struct { type_name, .. }
                        if method == "add" && matches!(
                            type_name.as_str(),
                            "HyperLogLog" | "TDigest" | "CountMinSketch" | "ReservoirSampler"
                        )
                );
            if handled_here {
                let mut argv = Vec::new();
                for a in args {
                    argv.push(self.eval(&a.expr, scope)?);
                }
                if let Some(result) = sequence_parity::eval_sequence_method(
                    self,
                    &container,
                    method,
                    &argv,
                    sequence_result_ty.as_ref(),
                    span,
                    scope,
                ) {
                    return match result? {
                        sequence_parity::SequenceOutcome::Value(value) => Ok(value),
                        sequence_parity::SequenceOutcome::WriteBack(value) => {
                            self.write_back(receiver, value, scope)?;
                            Ok(CtValue::Unit)
                        }
                    };
                }
                if let Some(result) = sketch_add(&container, &argv, span) {
                    let (ret, updated) = result?;
                    self.write_back(receiver, updated, scope)?;
                    return Ok(ret);
                }
                let ret = apply_mutating(&mut container, method, argv, span)?;
                self.write_back(receiver, container, scope)?;
                return Ok(ret);
            }
            evaluated_receiver = Some(container);
        }
        let recv = match evaluated_receiver {
            Some(value) => value,
            None => self.eval(receiver, scope)?,
        };
        // c139: an instance method the user wrote — `impl Type { fn … }` /
        // in-struct `fn`/`impl Trait { … }`. `recv`'s own `type_name` (not the
        // receiver expression's static type) picks the impl, so a value bound
        // through a trait-typed parameter still dispatches to its concrete
        // type's method (matches trait dynamic dispatch, no vtable needed).
        // Checked before the generic builtin dispatch so a user method always
        // wins over a same-named builtin.
        let type_name = match &recv {
            CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } => {
                Some(type_name.clone())
            }
            _ => None,
        };
        if let Some(tn) = type_name {
            if let Some(f) = self.methods.get(&(tn.clone(), method.to_string())).copied() {
                if !f.params.is_empty() && f.params.len() == args.len() + 1 {
                    let mut frame = HashMap::new();
                    frame.insert(f.params[0].name.clone(), recv.clone());
                    for (p, a) in f.params[1..].iter().zip(args) {
                        let v = self.eval(&a.expr, scope)?;
                        frame.insert(p.name.clone(), v);
                    }
                    return self.call_func(&format!("{}.{}", tn, method), f, frame);
                }
            }
        }
        if let (
            CtValue::Int(value),
            method @ ("count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros"),
        ) = (&recv, method)
        {
            let width = recv_type
                .and_then(integer_width_from_name)
                .or_else(|| self.integer_width_for_expr(receiver))
                .unwrap_or(64);
            let mask = if width == 64 {
                u64::MAX
            } else {
                (1_u64 << width) - 1
            };
            let bits = (*value as u64) & mask;
            let ones = bits.count_ones();
            let count = match method {
                "count_ones" => ones,
                "count_zeros" => width - ones,
                "leading_zeros" => bits.leading_zeros() - (64 - width),
                "trailing_zeros" => bits.trailing_zeros().min(width),
                _ => unreachable!("integer bit-query set is closed"),
            };
            return Ok(CtValue::Int(i64::from(count)));
        }
        match (&recv, method) {
            (CtValue::Struct { type_name, fields }, method @ ("tick" | "advance" | "wait"))
                if type_name == crate::Syntax::CLOCK_TYPE =>
            {
                if !matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
                    return Err(unsupported("Clock method on a temporary value", span));
                }
                let mut argv = Vec::with_capacity(args.len());
                for arg in args {
                    argv.push(self.eval(&arg.expr, scope)?);
                }
                let now = fields
                    .iter()
                    .find_map(|(name, value)| match (name.as_str(), value) {
                        ("now", CtValue::Int(now)) => Some(*now),
                        _ => None,
                    })
                    .unwrap_or(0);
                let next = match method {
                    "tick" => now.wrapping_add(as_int(&argv[0], span)?),
                    "advance" => as_int(&argv[0], span)?,
                    "wait" => match &argv[0] {
                        CtValue::Struct { type_name, fields }
                            if type_name == crate::Syntax::DURATION_TYPE =>
                        {
                            let millis = fields
                                .iter()
                                .find_map(|(name, value)| match (name.as_str(), value) {
                                    ("ms", CtValue::Int(millis)) => Some(*millis),
                                    _ => None,
                                })
                                .unwrap_or(0);
                            now.wrapping_add(millis)
                        }
                        _ => return Err(unsupported("Clock.wait expects a Duration", span)),
                    },
                    _ => unreachable!("Clock mutation set is closed"),
                };
                self.write_back(
                    receiver,
                    CtValue::Struct {
                        type_name: crate::Syntax::CLOCK_TYPE.to_string(),
                        fields: vec![("now".to_string(), CtValue::Int(next))],
                    },
                    scope,
                )?;
                return Ok(CtValue::Int(next));
            }
            (
                CtValue::Struct { type_name, fields },
                method @ ("len"
                    | "is_empty"
                    | "clear"
                    | "to_bytes"
                    | "write_u8"
                    | "write_u16_le"
                    | "write_u16_be"
                    | "write_u32_le"
                    | "write_u32_be"
                    | "write_u64_le"
                    | "write_u64_be"
                    | "write_bytes"),
            ) if type_name == crate::Syntax::TYPE_BYTE_BUFFER => {
                let mut argv = Vec::with_capacity(args.len());
                for arg in args {
                    argv.push(self.eval(&arg.expr, scope)?);
                }
                let mut bytes = fields
                    .iter()
                    .find(|(name, _)| name == "bytes")
                    .map(|(_, value)| as_bytes(value, span))
                    .transpose()?
                    .unwrap_or_default();
                let result = match method {
                    "len" => return Ok(CtValue::Int(bytes.len() as i64)),
                    "is_empty" => return Ok(CtValue::Bool(bytes.is_empty())),
                    "to_bytes" => return Ok(CtValue::Bytes(bytes)),
                    "clear" => {
                        bytes.clear();
                        CtValue::Unit
                    }
                    "write_bytes" => {
                        bytes.extend(as_bytes(&argv[0], span)?);
                        CtValue::Unit
                    }
                    method => {
                        let value = as_int(&argv[0], span)?;
                        match method {
                            "write_u8" => bytes.push(value as u8),
                            "write_u16_le" => {
                                bytes.extend_from_slice(&(value as u16).to_le_bytes())
                            }
                            "write_u16_be" => {
                                bytes.extend_from_slice(&(value as u16).to_be_bytes())
                            }
                            "write_u32_le" => {
                                bytes.extend_from_slice(&(value as u32).to_le_bytes())
                            }
                            "write_u32_be" => {
                                bytes.extend_from_slice(&(value as u32).to_be_bytes())
                            }
                            "write_u64_le" => {
                                bytes.extend_from_slice(&(value as u64).to_le_bytes())
                            }
                            "write_u64_be" => {
                                bytes.extend_from_slice(&(value as u64).to_be_bytes())
                            }
                            _ => unreachable!("ByteBuffer method set is closed"),
                        }
                        CtValue::Unit
                    }
                };
                if matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
                    self.write_back(
                        receiver,
                        CtValue::Struct {
                            type_name: crate::Syntax::TYPE_BYTE_BUFFER.to_string(),
                            fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
                        },
                        scope,
                    )?;
                }
                return Ok(result);
            }
            _ => {}
        }
        let is_build_context = matches!(
            &recv,
            CtValue::Struct { type_name, .. }
                if type_name == crate::Syntax::TYPE_BUILD_CONTEXT
        );
        if is_build_context {
            match method {
                "find" => return self.eval_find(args, span),
                _ => {}
            }
        }
        let mut argv = Vec::new();
        for a in args {
            argv.push(self.eval(&a.expr, scope)?);
        }
        match (&recv, method) {
            (
                CtValue::Struct { type_name, fields },
                method @ ("int"
                    | "float"
                    | "float_range"
                    | "bool"
                    | "normal"
                    | "exponential"
                    | "bytes"
                    | "split"
                    | "pick"
                    | "weighted_pick"
                    | "sample"
                    | "shuffle"),
            ) if type_name == crate::Syntax::RNG_TYPE => {
                if !matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
                    return Err(unsupported("Rng method on a temporary value", span));
                }
                if method == "shuffle"
                    && !matches!(
                        args.first().map(|arg| &arg.expr),
                        Some(Expr::Ident(..) | Expr::Field(..))
                    )
                {
                    return Err(unsupported("Rng.shuffle with a temporary list", span));
                }
                let mut state = fields
                    .iter()
                    .find_map(|(name, value)| match (name.as_str(), value) {
                        ("state", CtValue::Int(state)) => Some(*state as u64),
                        _ => None,
                    })
                    .unwrap_or(0);
                let value = apply_seeded_rng_method(&mut state, method, &mut argv, span)?;
                if method == "shuffle" {
                    self.write_back(&args[0].expr, argv[0].clone(), scope)?;
                }
                self.write_back(
                    receiver,
                    CtValue::Struct {
                        type_name: crate::Syntax::RNG_TYPE.to_string(),
                        fields: vec![("state".to_string(), CtValue::Int(state as i64))],
                    },
                    scope,
                )?;
                return Ok(value);
            }
            // D-SOLVER-LIB1=A: `solver.require(ok)` records a Bool constraint in place.
            (CtValue::Struct { type_name, .. }, "require")
                if type_name == crate::Syntax::SOLVER_TYPE =>
            {
                if !matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
                    return Err(unsupported("Solver method on a temporary value", span));
                }
                let Some(result) = solver_require(&recv, &argv, span) else {
                    return Err(unsupported("`Solver.require`", span));
                };
                let (ret, updated) = result?;
                self.write_back(receiver, updated, scope)?;
                return Ok(ret);
            }
            _ => {}
        }
        if is_build_context && method == "fetch" {
            return self
                .eval_net_fetch(argv, span)
                .map(|value| CtValue::ResOk(Box::new(value)));
        }
        if is_build_context && method == "embed" {
            let rel = match argv.first() {
                Some(CtValue::Str(path)) => path,
                _ => return Err(unsupported("`b.embed` requires a path string", span)),
            };
            let path = std::path::Path::new(rel);
            if path.is_absolute()
                || path.components().any(|component| matches!(component, std::path::Component::ParentDir))
            {
                return Err(Diagnostic::error(
                    "E0957",
                    format!("`b.embed` path `{rel}` escapes the build root"),
                    "locked build inputs must stay beneath the selected source directory".to_string(),
                    "use a relative path returned by `b.find`, without `..`".to_string(),
                    Some(span),
                ));
            }
            let bytes = std::fs::read(self.base_dir.join(path)).map_err(|error| Diagnostic::error(
                "E0955",
                format!("`b.embed` cannot open `{rel}`"),
                error.to_string(),
                "check the locked relative path".to_string(),
                Some(span),
            ))?;
            self.embed_inputs.push(crate::AST::ComptimeInput {
                path: rel.clone(),
                hash: crate::SHA256::sha256_hex(&bytes),
            });
            return String::from_utf8(bytes).map(CtValue::Str).map_err(|_| Diagnostic::error(
                "E0955",
                format!("`b.embed` cannot decode `{rel}` as text"),
                "the embedded file is not valid UTF-8".to_string(),
                "embed a UTF-8 text file".to_string(),
                Some(span),
            ));
        }
        if let Some(result) = sequence_parity::eval_sequence_method(
            self,
            &recv,
            method,
            &argv,
            sequence_result_ty.as_ref(),
            span,
            scope,
        ) {
            return match result? {
                sequence_parity::SequenceOutcome::Value(value) => Ok(value),
                sequence_parity::SequenceOutcome::WriteBack(value) => {
                    self.write_back(receiver, value, scope)?;
                    Ok(CtValue::Unit)
                }
            };
        }
        // D-BUILDENTRY1: selected-root `BuildContext` is interpreter-owned.
        // Driver removes `fn build` before runtime codegen.
        if let Some(result) =
            super::super::super::Build::eval_program_build_method(
                &recv,
                method,
                argv.clone(),
                span,
                self.impure_depth > 0,
            )
        {
            return result;
        }
        apply_method(&recv, method, argv, span)
    }
}
