impl<'a> Checker<'a> {
        /// Check a call. Returns:
        ///   None             — problem already reported
        ///   Some(None)       — fine, no value handed back
        ///   Some(Some(ty))   — fine, hands back `ty`
        /// D-NUMOPS1: type a `wrapping`/`saturating`/`checked` opt-in. The single
        /// argument must be one integer `+`/`-`/`*`/`/`; `wrapping`/`saturating`
        /// return the operand width, `checked` returns it optional (`null` on
        /// overflow). E1005 otherwise.
        fn check_overflow_opt_in(&mut self, call: &mut Call) -> Option<Type> {
            let kind = call.name.clone();
            if call.args.len() != 1 {
                let mut ty = None;
                for a in call.args.iter_mut() {
                    ty = ty.or(self.infer(&mut a.expr));
                }
                self.diags
                    .push(overflow_opt_in_error(&kind, call.name_span));
                // Hand back a plausible type so the use site doesn't cascade.
                return ty.filter(Type::is_integer).or(Some(Type::Int));
            }
            let arg_ty = self.infer(&mut call.args[0].expr);
            let is_arith = matches!(
                &call.args[0].expr,
                Expr::Binary(op, _, _, _)
                    if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
            );
            let int_ok = arg_ty.as_ref().is_some_and(|t| t.is_integer());
            if !is_arith || !int_ok {
                self.diags
                    .push(overflow_opt_in_error(&kind, call.name_span));
                return arg_ty.filter(Type::is_integer).or(Some(Type::Int));
            }
            let t = arg_ty.unwrap();
            if kind == Syntax::BUILTIN_CHECKED {
                Some(Type::Option(Box::new(t)))
            } else {
                Some(t)
            }
        }
    
        pub(crate) fn check_call(&mut self, call: &mut Call, _as_value: bool) -> Option<Option<Type>> {
            // D-NUMOPS1: `wrapping`/`saturating`/`checked` opt-ins wrap a single integer
            // `+`/`-`/`*`/`/`. A user-defined function of the same name shadows them.
            if matches!(
                call.name.as_str(),
                Syntax::BUILTIN_WRAPPING | Syntax::BUILTIN_SATURATING | Syntax::BUILTIN_CHECKED
            ) && !self.funcs.contains_key(&call.name)
            {
                return Some(self.check_overflow_opt_in(call));
            }
            // D-EFF1: an ambient builtin (`print`/`input`) contributes the `Io`
            // effect, unless a user function of the same name shadows it (in which
            // case the edge to that user function is recorded below).
            if !self.funcs.contains_key(&call.name) {
                if let Some(e) = builtin_effect(&call.name) {
                    self.record_effect(e.name());
                }
            }
            if call.name == Syntax::FOREIGN_PRINTLN || call.name == Syntax::FOREIGN_EPRINTLN {
                let target = if call.name == Syntax::FOREIGN_EPRINTLN {
                    "io.eprint"
                } else {
                    Syntax::BUILTIN_PRINT
                };
                self.diags.push(Diagnostic::error(
                    "E0037",
                    format!(
                        "{} calls it `{}`, not `{}`",
                        Syntax::LANG_NAME,
                        target,
                        call.name
                    ),
                    "`print` writes to stdout; `io.eprint` is the stderr twin in `core.io`".to_string(),
                    format!("replace `{}` with `{}`", call.name, target),
                    Some(call.name_span),
                ));
                for arg in call.args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
    
            if call.name == Syntax::FOREIGN_OPEN {
                self.diags.push(Diagnostic::error(
                    "E0038",
                    "`open` is not the M10 file API".to_string(),
                    "M10 uses whole-file helpers in `core.files`, not a bare `open` handle call"
                        .to_string(),
                    "import `core.files as fs` and call `fs.read(path)` or `fs.write(path, text)` \
                     (or `fs.open(path)` for a streaming handle)"
                        .to_string(),
                    Some(call.name_span),
                ));
                for arg in call.args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
    
            if call.name == Syntax::FOREIGN_GETENV {
                self.diags.push(Diagnostic::error(
                    "E0039",
                    "`getenv` is written `env.get` in Jet".to_string(),
                    "environment access lives in the `core.env` module".to_string(),
                    "import `core.env as env` and call `env.get(name)`".to_string(),
                    Some(call.name_span),
                ));
                for arg in call.args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
    
            if matches!(
                call.name.as_str(),
                Syntax::FOREIGN_ASYNC | Syntax::FOREIGN_AWAIT
            ) {
                self.diags.push(Diagnostic::error(
                    "E0040",
                    format!("`{}` is not in Jet; use `tasks.spawn` instead", call.name),
                    "Jet uses blocking tasks and channels, not async/await — simpler and race-free"
                        .to_string(),
                    "import `core.tasks as tasks` and call `tasks.spawn(() => your_work())`"
                        .to_string(),
                    Some(call.name_span),
                ));
                for a in call.args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
    
            if matches!(
                call.name.as_str(),
                Syntax::FOREIGN_MUTEX | Syntax::FOREIGN_LOCK | "RwLock" | "mutex"
            ) {
                self.diags.push(Diagnostic::error(
                    "E0041",
                    format!(
                        "`{}` is not in Jet; share data through channels",
                        call.name
                    ),
                    "Jet avoids shared mutable state: tasks communicate by sending messages, not sharing memory"
                        .to_string(),
                    "import `core.tasks as tasks`, create a channel, and use `sender.send`/`channel.receive`"
                        .to_string(),
                    Some(call.name_span),
                ));
                for a in call.args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
    
            if call.name == Syntax::BUILTIN_PRINT {
                if self.no_prelude {
                    self.diags.push(Diagnostic::error(
                        "E0429",
                        format!(
                            "`{}` is not ambient here — this file opted out with `#{}`",
                            Syntax::BUILTIN_PRINT,
                            Syntax::MARKER_NO_PRELUDE
                        ),
                        format!(
                            "`#{}` disables the curated prelude auto-imports (`{}` / `{}`)",
                            Syntax::MARKER_NO_PRELUDE,
                            Syntax::BUILTIN_PRINT,
                            Syntax::BUILTIN_INPUT
                        ),
                        format!(
                            "write `use core.io as io` and call `io.{}(…)`, or remove `#{}`",
                            Syntax::BUILTIN_PRINT,
                            Syntax::MARKER_NO_PRELUDE
                        ),
                        Some(call.name_span),
                    ));
                    for arg in call.args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return Some(None);
                }
                if call.args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!(
                            "`{}` needs exactly one thing to print",
                            Syntax::BUILTIN_PRINT
                        ),
                        "printing nothing isn't meaningful".to_string(),
                        format!("e.g. {}(\"hello\")", Syntax::BUILTIN_PRINT),
                        Some(call.name_span),
                    ));
                    for arg in call.args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                let arg = &mut call.args[0];
                self.borrow_ctx = true; // print borrows via `.jet_show()`
                if let Some(t) = self.infer(&mut arg.expr) {
                    if !is_printable(&t, self.registry) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`{}` doesn't know how to show {}",
                                Syntax::BUILTIN_PRINT,
                                t.show()
                            ),
                            "print shows values that have a display".to_string(),
                            "print one of its parts instead".to_string(),
                            Some(arg.expr.span()),
                        ));
                    }
                }
                return Some(None);
            }
    
            // D-PRELUDE1 = B: `input` is ambient — no `use core.io` needed.
            // Resolves to the same semantics as `io.input`: optional String prompt,
            // returns Result(String, IoError). Shadowed by any user-defined `input`.
            // D-PRELUDEX1=A: `#NoPrelude` turns the ambient off.
            if call.name == Syntax::BUILTIN_INPUT
                && self.funcs.get(Syntax::BUILTIN_INPUT).is_none()
                && self.lookup(Syntax::BUILTIN_INPUT).is_none()
            {
                if self.no_prelude {
                    self.diags.push(Diagnostic::error(
                        "E0429",
                        format!(
                            "`{}` is not ambient here — this file opted out with `#{}`",
                            Syntax::BUILTIN_INPUT,
                            Syntax::MARKER_NO_PRELUDE
                        ),
                        format!(
                            "`#{}` disables the curated prelude auto-imports (`{}` / `{}`)",
                            Syntax::MARKER_NO_PRELUDE,
                            Syntax::BUILTIN_PRINT,
                            Syntax::BUILTIN_INPUT
                        ),
                        format!(
                            "write `use core.io as io` and call `io.{}(…)`, or remove `#{}`",
                            Syntax::BUILTIN_INPUT,
                            Syntax::MARKER_NO_PRELUDE
                        ),
                        Some(call.name_span),
                    ));
                    for arg in call.args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return Some(None);
                }
                if call.args.len() > 1 {
                    self.diags.push(wrong_core_arity(
                        Syntax::BUILTIN_INPUT,
                        1,
                        call.args.len(),
                        call.name_span,
                    ));
                }
                if let Some(arg) = call.args.get_mut(0) {
                    self.expect_core_arg(Syntax::BUILTIN_INPUT, 0, &Type::String, arg);
                }
                return Some(Some(result_ty(Type::String, io_error_ty())));
            }
    
            if call.name == Syntax::BUILTIN_PANIC {
                // D-METADEPTH2: retain panic reachability in the checked call
                // graph consumed by ProgramInfo; this sentinel is not an effect.
                self.fx_edges.insert("__jet_panic__".to_string());
                self.check_panic_call(call);
                return Some(None);
            }
    
            if call.name == Syntax::BUILTIN_REQUIRE {
                self.check_require_call(call);
                return Some(None);
            }
    
            if call.name == Syntax::BUILTIN_REQUIRE_EQ {
                self.check_require_eq_call(call);
                return Some(None);
            }
    
            if call.name == Syntax::BUILTIN_FIND && self.in_comptime {
                if call.args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        "`find` needs exactly one glob".to_string(),
                        "`find(glob)` expands one build-time glob inside a `comptime` binding"
                            .to_string(),
                        "write `find(\"content/**/*.md\")`".to_string(),
                        Some(call.name_span),
                    ));
                    for arg in call.args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                self.expect_core_arg(Syntax::BUILTIN_FIND, 0, &Type::String, &mut call.args[0]);
                return Some(Some(Type::List(Box::new(Type::String))));
            }
    
            // D-LIN1-DROP (ratified 2026-06-25): `drop(x)` deliberately discards a
            // value by moving it to nowhere — its `Drop` runs. The blessed use is to
            // satisfy a `#SingleUse` value's consume duty when there is genuinely no
            // job left to do; that decision must be audited, so `drop` of a
            // `#SingleUse` value is legal only inside an `#Unsafe("reason")`
            // region/fn (the reason IS the audit note) — otherwise E0143. Shadowed
            // by any user `drop` fn or local of that name.
            if call.name == Syntax::BUILTIN_DROP
                && self.funcs.get(Syntax::BUILTIN_DROP).is_none()
                && self.lookup(Syntax::BUILTIN_DROP).is_none()
            {
                if call.args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!("`{}` discards exactly one value", Syntax::BUILTIN_DROP),
                        "`drop` throws a single value away, running its cleanup".to_string(),
                        format!("e.g. {}(x)", Syntax::BUILTIN_DROP),
                        Some(call.name_span),
                    ));
                    for a in call.args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(None);
                }
                self.infer(&mut call.args[0].expr);
                if let Expr::Ident(name, span) = &call.args[0].expr {
                    let single_use = self
                        .lookup(name)
                        .map(|info| info.single_use_span.is_some())
                        .unwrap_or(false);
                    if single_use && !self.in_unsafe {
                        self.diags.push(e0143_drop_unaudited(name, *span));
                    }
                    // The value is given away for real — discharges the consume duty
                    // (E0140/E0141) and prevents any later reuse (E0121). Mark it
                    // consumed even on the E0143 path so the unaudited-drop error is
                    // not buried under a cascade E0140 "unconsumed" at scope end.
                    self.mark_moved(name.clone(), *span);
                }
                return Some(None);
            }
    
            // D-TOOL4 (E2-M11): `expect(x)` — test-only builtin that wraps a value
            // for snapshot testing. The expression `expect(x).snapshot()` is the
            // full form; `.snapshot()` is handled in the method-call path below.
            if call.name == Syntax::BUILTIN_EXPECT {
                if call.args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E2901",
                        format!(
                            "`{}` needs exactly one value to test",
                            Syntax::BUILTIN_EXPECT
                        ),
                        "snapshot testing wraps one value at a time".to_string(),
                        format!("e.g. {}(my_value).snapshot()", Syntax::BUILTIN_EXPECT),
                        Some(call.name_span),
                    ));
                } else {
                    self.infer(&mut call.args[0].expr);
                }
                // Returns a Named type marker so the `.snapshot()` call can detect it.
                return Some(Some(Type::Named("__JetExpect__".to_string())));
            }
    
            if matches!(
                call.name.as_str(),
                Syntax::TYPED_TEXT_SQL_PREFIX_CALL | Syntax::TYPED_TEXT_HTML_PREFIX_CALL
            ) {
                let type_name = if call.name == Syntax::TYPED_TEXT_SQL_PREFIX_CALL {
                    "Sql"
                } else {
                    "Html"
                };
                if let Some(arg) = call.args.get_mut(0) {
                    let span = arg.span;
                    let mut expr = std::mem::replace(&mut arg.expr, Expr::Absent(span));
                    let ty = self.rewrite_typed_text_literal(&mut expr, type_name.to_string(), span);
                    if let Expr::Call(rewritten) = expr {
                        *call = rewritten;
                    } else {
                        arg.expr = expr;
                        call.name = type_name.to_string();
                    }
                    return Some(ty);
                }
                return Some(Some(Type::Named(type_name.to_string())));
            }
    
            if self.funcs.get(&call.name).is_none() {
                if let Some(info) = self.lookup(&call.name) {
                    if matches!(info.ty, Type::Fn { .. }) {
                        let name_span = call.name_span;
                        let mut callee = Box::new(Expr::Ident(call.name.clone(), name_span));
                        let mut args = std::mem::take(&mut call.args);
                        let end = args
                            .last()
                            .map(|a| a.expr.span().end)
                            .unwrap_or(name_span.end);
                        let span = Span::new(name_span.start, end);
                        let ret = self.infer_call_value(&mut callee, &mut args, span);
                        call.args = args;
                        return Some(ret);
                    }
                }
                // D-MOD3: check unqualified inline-module imports (e.g. `use math.clamp`).
                if let Some(mangled) = self.unqualified.get(&call.name).cloned() {
                    let alias = mangled.split("__").next().unwrap_or(&mangled).to_string();
                    let result = self.infer_code_module_call(
                        &alias,
                        &mangled,
                        call.name_span,
                        call.name_span,
                        &mut call.args,
                    );
                    return Some(result);
                }
                // D-MOD3: check unqualified file-module imports (e.g. `use math.clamp` for a file module).
                if let Some((fn_name, mod_idx)) = self.unqualified_file.get(&call.name).cloned() {
                    let result = self.infer_import_call(
                        mod_idx,
                        &fn_name,
                        call.name_span,
                        call.name_span,
                        &mut call.args,
                    );
                    return Some(result);
                }
            }
    
            // D-SIMD2 / D-LINALG1: `F32x4(a,b,c,d)` / `Vec3(x,y,z)` / `Mat3(…)` —
            // positional construction of a built-in math value type. Each argument is
            // elaborated against the component type (so `1.0` becomes `F32` for a
            // `F32x4`) and checked in order; arity is fixed by the type.
            if self.funcs.get(&call.name).is_none() && !self.registry.contains(&call.name) {
                if let Some(arg_types) = math_constructor_arg_types(&call.name) {
                    let arity = arg_types.len();
                    if call.args.len() != arity {
                        self.diags.push(Diagnostic::error(
                            "E0103",
                            format!(
                                "`{}` takes exactly {} component{}, got {}",
                                call.name,
                                arity,
                                if arity == 1 { "" } else { "s" },
                                call.args.len()
                            ),
                            format!(
                                "`{}` is a built-in {} type — construct it from its {} components",
                                call.name,
                                if is_simd_lane_type(&call.name) {
                                    "SIMD lane"
                                } else {
                                    "linear-algebra"
                                },
                                arity
                            ),
                            format!("write `{}({})`", call.name, vec!["…"; arity].join(", ")),
                            Some(call.name_span),
                        ));
                        for a in call.args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return Some(Some(Type::Named(call.name.clone())));
                    }
                    for (i, want) in arg_types.iter().enumerate() {
                        let old = self.expected_type.replace(want.clone());
                        let got = self.infer(&mut call.args[i].expr);
                        self.expected_type = old;
                        if let Some(at) = got {
                            if at != *want {
                                self.diags.push(Diagnostic::error(
                                    "E0128",
                                    format!(
                                        "component {} of `{}` must be `{}`, got `{}`",
                                        i + 1,
                                        call.name,
                                        want.name(),
                                        at.name()
                                    ),
                                    format!(
                                        "every component of `{}` is a `{}`",
                                        call.name,
                                        want.name()
                                    ),
                                    format!("write a `{}` value here", want.name()),
                                    Some(call.args[i].expr.span()),
                                ));
                            }
                        }
                    }
                    return Some(Some(Type::Named(call.name.clone())));
                }
            }
    
            // D-BIGINT1: `BigInt(100)` or `BigInt("…")` — explicit construction only.
            if self.funcs.get(&call.name).is_none()
                && !self.registry.contains(&call.name)
                && call.name == crate::Syntax::TYPE_BIGINT
            {
                if call.args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!(
                            "`BigInt` takes exactly one argument, got {}",
                            call.args.len()
                        ),
                        "`BigInt` constructs an arbitrary-precision integer from an `Int` or `String`"
                            .to_string(),
                        "write `BigInt(100)` or `BigInt(\"999…\")`".to_string(),
                        Some(call.name_span),
                    ));
                    for a in call.args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Some(Type::Named(call.name.clone())));
                }
                let got = self.infer(&mut call.args[0].expr);
                match got {
                    Some(Type::Int) | Some(Type::String) => {}
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0128",
                            format!("`BigInt` expects `Int` or `String`, got `{}`", other.name()),
                            "`BigInt` never converts from `Float` or promotes silently".to_string(),
                            "pass an integer literal or a decimal string".to_string(),
                            Some(call.args[0].expr.span()),
                        ));
                    }
                    None => {}
                }
                return Some(Some(Type::Named(call.name.clone())));
            }
    
            // D-DECIMAL1: `Decimal("12.34")` — exact base-10 parse.
            if self.funcs.get(&call.name).is_none()
                && !self.registry.contains(&call.name)
                && call.name == crate::Syntax::TYPE_DECIMAL
            {
                if call.args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!(
                            "`Decimal` takes exactly one argument, got {}",
                            call.args.len()
                        ),
                        "`Decimal` parses an exact base-10 decimal from a `String`".to_string(),
                        "write `Decimal(\"12.34\")`".to_string(),
                        Some(call.name_span),
                    ));
                    for a in call.args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Some(Type::Named(call.name.clone())));
                }
                self.expect_core_arg("Decimal", 0, &Type::String, &mut call.args[0]);
                return Some(Some(Type::Named(call.name.clone())));
            }
    
            // D-DIST3 (ratified 2026-06-20): `DistinctType(expr)` — construct a distinct value.
            if self.funcs.get(&call.name).is_none() {
                if let Some(base_ty) = self.registry.distinct_base(&call.name).cloned() {
                    if call.args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "E0103",
                            format!(
                                "`{}` takes exactly one argument, got {}",
                                call.name,
                                call.args.len()
                            ),
                            format!(
                                "`{}` is a distinct type; construct it with `{}(value)`",
                                call.name, call.name
                            ),
                            format!(
                                "write `{}(expr)` with a single value of type `{}`",
                                call.name,
                                base_ty.name()
                            ),
                            Some(call.name_span),
                        ));
                        for a in call.args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    let old_expected = self.expected_type.replace(base_ty.clone());
                    let arg_ty = self.infer(&mut call.args[0].expr);
                    self.expected_type = old_expected;
                    if let Some(at) = arg_ty {
                        if at != base_ty {
                            self.diags.push(Diagnostic::error(
                                "E0128",
                                format!(
                                    "a `{}` can't be used where a `{}` is expected",
                                    at.name(), call.name
                                ),
                                format!(
                                    "`{}` and `{}` are different types — even though `{}` is built on `{}`, one is never accepted in place of the other",
                                    call.name, at.name(), call.name, base_ty.name()
                                ),
                                format!("construct a `{}`: `{}({})`", call.name, call.name, "expr"),
                                Some(call.args[0].expr.span()),
                            ));
                            return None;
                        }
                    }
                    // D-RANGETYPE1: `Severity :: distinct Int(0..10)` — a literal
                    // argument is checked NOW (E0135); a runtime value needs the
                    // fallible `?` form (E0136 otherwise). "Parse, don't
                    // validate" as a type.
                    if let Some((lo, hi)) = self.registry.distinct_range(&call.name) {
                        match &call.args[0].expr {
                            Expr::Int(n, span, _) => {
                                if *n < lo || *n > hi {
                                    self.diags.push(Diagnostic::error(
                                        "E0135",
                                        format!("`{}` is outside `{}`'s range {}..{}", n, call.name, lo, hi),
                                        format!(
                                            "a range type only holds values inside its bounds; `{}` can never be a `{}`",
                                            n, call.name
                                        ),
                                        format!("use a value in `{}..{}`, or widen the type's range", lo, hi),
                                        Some(*span),
                                    ));
                                }
                            }
                            _ => {
                                if call.range_checked {
                                    return Some(Some(Type::Result {
                                        ok: Box::new(Type::Named(call.name.clone())),
                                        err: Box::new(Type::String),
                                    }));
                                } else {
                                    self.diags.push(Diagnostic::error(
                                        "E0136",
                                        format!("making a `{}` from a runtime value can fail", call.name),
                                        "only a literal is checked at compile time; a runtime number needs the fallible form so a bad value is handled".to_string(),
                                        format!("write `{}(raw)?` and handle the failure", call.name),
                                        Some(call.args[0].expr.span()),
                                    ));
                                }
                            }
                        }
                    }
                    return Some(Some(Type::Named(call.name.clone())));
                }
            }
    
            let Some(mut sig) = self.funcs.get(&call.name).cloned() else {
                let mut fix = format!(
                    "define it first ({} {}() {{ ... }}), or call one that exists",
                    Syntax::KW_FN,
                    call.name
                );
                let mut best: Option<(&str, usize)> = None;
                let prelude_cands: &[&str] = if self.no_prelude {
                    &[]
                } else {
                    Syntax::PRELUDE_IDENTS
                };
                for cand in self
                    .funcs
                    .keys()
                    .map(|s| s.as_str())
                    .chain(prelude_cands.iter().copied())
                {
                    let d = edit_distance(&call.name, cand);
                    if d <= 2 && best.map_or(true, |(_, bd)| d < bd) {
                        best = Some((cand, d));
                    }
                }
                if let Some((cand, _)) = best {
                    fix = format!("did you mean `{}`?", cand);
                }
                self.diags.push(Diagnostic::error(
                    "E0102",
                    format!("nothing named `{}` exists here", call.name),
                    format!(
                        "only functions that have been defined (or built in, like `{}` / `{}`) can be called",
                        Syntax::BUILTIN_PRINT, Syntax::BUILTIN_INPUT
                    ),
                    fix,
                    Some(call.name_span),
                ));
                for arg in call.args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            };
            self.record_current_function_reference(&call.name, call.name_span);
    
            // D-EFF1: record the call-graph edge for transitive effect inference.
            // A foreign (`extern`) callee has an un-inspectable body, so it forces
            // the maximal effect set; a Jet callee's effects flow in via its edge.
            if sig.is_extern {
                self.record_maximal();
            } else {
                self.record_edge(call.name.clone());
            }
    
            // E3211 (card #436): a `String` literal with a known interior NUL
            // byte can't cross into a C-boundary function — `CString::new`
            // would fail (C strings are NUL-terminated, not length-prefixed).
            // Only checked for a literal (fully known at compile time); a
            // runtime-built String is caught by a codegen panic instead (see
            // `Codegen/CModule.rs`'s `NUL_PANIC`).
            // E3211 (card #436): same check for a directly-called (same-file,
            // no import alias) C-boundary function — see
            // `CheckerCoreLib/imports.rs::infer_import_call` for the more
            // common cross-module-alias path (`use c.<lib> as x; x.f(...)`).
            if sig.is_c_abi {
                for (i, arg) in call.args.iter().enumerate() {
                    let is_string_param =
                        matches!(sig.params.get(i), Some((_, Type::String)));
                    if !is_string_param {
                        continue;
                    }
                    if let Expr::Str(parts, span) = &arg.expr {
                        let literal: Option<String> = parts
                            .iter()
                            .map(|p| match p {
                                StrPart::Lit(s) => Some(s.clone()),
                                StrPart::Interp(..) => None,
                            })
                            .collect();
                        if let Some(text) = literal {
                            if text.contains('\0') {
                                self.diags.push(e3211(*span));
                            }
                        }
                    }
                }
            }

            // E3103 (S58): an `#Unsafe fn` is a whole-function contract; callers
            // must take responsibility inside their own `#Unsafe` block.
            if sig.is_unsafe && !self.in_unsafe {
                self.diags.push(Diagnostic::error(
                    "E3103",
                    format!("`{}` is an `#Unsafe` function", call.name),
                    "its contract can't be checked by the compiler, so the caller must vouch for it"
                        .to_string(),
                    format!("call it inside `#{}(\"…\") {{ … }}`", Syntax::KW_UNSAFE),
                    Some(call.name_span),
                ));
            }
    
            // D-NARG-D4 (S61, E0125): label validation — if a call arg has
            // `name: val`, verify it matches the parameter name at that position.
            // Labels never reorder.
            if !sig.param_info.is_empty() {
                let all_param_names: Vec<&str> =
                    sig.param_info.iter().map(|(n, _)| n.as_str()).collect();
                for (i, arg) in call.args.iter().enumerate() {
                    if let Some((label, label_span)) = &arg.label {
                        if let Some((param_name, _)) = sig.param_info.get(i) {
                            if label != param_name {
                                // Is the label a real param name at a different position?
                                if all_param_names.contains(&label.as_str()) {
                                    // Transposed: label names a real param, but wrong position.
                                    self.diags.push(Diagnostic::error(
                                        "E0125",
                                        format!(
                                            "label `{}:` doesn't match the parameter `{}` here",
                                            label, param_name
                                        ),
                                        "labels are checked documentation — each names the parameter at its own position, and arguments stay in the order they're declared".to_string(),
                                        format!(
                                            "write `{}:` here, or drop the label",
                                            param_name
                                        ),
                                        Some(*label_span),
                                    ));
                                } else {
                                    // Unknown: label doesn't name any parameter.
                                    self.diags.push(Diagnostic::error(
                                        "E0125",
                                        format!(
                                            "`{}` has no parameter named `{}`",
                                            call.name, label
                                        ),
                                        format!(
                                            "a label must name the parameter at its position; `{}` takes {}",
                                            call.name,
                                            all_param_names.join(", ")
                                        ),
                                        format!(
                                            "use one of `{}`'s parameter names, or drop the label",
                                            call.name
                                        ),
                                        Some(*label_span),
                                    ));
                                }
                            }
                        }
                    }
                }
                // L2401: advisory lint — public API has a positional Bool parameter.
                // (Only warn on the callee definition side, not every call site.)
            }
    
            // D-NARG-D2 (S61): default-value filling — append defaults for omitted
            // trailing params. Earlier-param refs in defaults are substituted with
            // the supplied argument expression so codegen never sees an unresolved
            // identifier (invariant I2).
            if call.args.len() < sig.params.len() && !sig.defaults.is_empty() {
                let provided = call.args.len();
                let required: usize = sig.defaults.iter().take_while(|d| d.is_none()).count();
                if provided >= required {
                    // fill trailing omitted params with their defaults. We build
                    // `earlier_names` incrementally so a default like `d: Int = h`
                    // can reference an earlier-defaulted param `h` that was already
                    // filled (and is now in call.args at position 1).
                    let all_param_names: Vec<String> =
                        sig.param_info.iter().map(|(n, _)| n.clone()).collect();
                    for i in provided..sig.params.len() {
                        if let Some(Some(default_expr)) = sig.defaults.get(i) {
                            // earlier_names covers all params up to (not including) i.
                            let earlier_names: Vec<String> =
                                all_param_names.iter().take(i).cloned().collect();
                            // Substitute any earlier-param idents with the supplied arg.
                            let resolved = super::substitute_param_refs(
                                default_expr.clone(),
                                &earlier_names,
                                &call.args,
                            );
                            call.args.push(crate::AST::CallArg {
                                convention: sig.params[i].0,
                                expr: resolved,
                                span: call.name_span,
                                flags: Default::default(),
                                label: None,
                                spread: false,
                            });
                        }
                    }
                }
            }
    
            let variadic = sig.param_variadic.last().copied().unwrap_or(false);
            // D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): a trait-bounded variadic
            // (`...Trait` / `...[A, B]`) can't be packed into one `[T]` list —
            // its elements can have different concrete types, so there's no single
            // element type a real list literal could carry. Bound-check the tail
            // arguments (E1313) against a temporarily *removed* slice so the
            // ordinary per-index checking below (moves, borrows, type-compat) never
            // sees them and never re-`infer`s them; `sig` is shrunk (a local, owned
            // clone) the same way, so the two arg counts still line up. The tail is
            // spliced back onto `call.args` right before this function returns, so
            // codegen reads the arity straight off `call.args.len()` same as any
            // other call.
            let bound_variadic = variadic
                .then(|| sig.variadic_bounds.clone())
                .flatten()
                .or_else(|| {
                    if !variadic {
                        return None;
                    }
                    match sig.params.last() {
                        Some((_, Type::List(elem))) => match elem.as_ref() {
                            Type::Named(n) if self.trait_reg.is_trait_name(n) => Some(vec![n.clone()]),
                            _ => None,
                        },
                        _ => None,
                    }
                });
            let mut bound_variadic_tail: Option<Vec<crate::AST::CallArg>> = None;
            if let Some(bounds) = bound_variadic {
                let fixed = sig.params.len().saturating_sub(1);
                if call.args.len() < fixed {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`{}` expects at least {} argument{}, got {}",
                            call.name,
                            fixed,
                            if fixed == 1 { "" } else { "s" },
                            call.args.len()
                        ),
                        "every fixed parameter must receive a value before the variadic tail"
                            .to_string(),
                        format!("check the definition of `{}`", call.name),
                        Some(call.name_span),
                    ));
                }
                let mut tail = if call.args.len() > fixed {
                    call.args.split_off(fixed)
                } else {
                    Vec::new()
                };
                self.check_variadic_bound_tail(call, &sig, &mut tail, &bounds);
                sig.params.pop();
                sig.param_info.pop();
                sig.defaults.pop();
                sig.param_variadic.pop();
                bound_variadic_tail = Some(tail);
            } else {
                if variadic {
                    self.normalize_variadic_call(call, &sig);
                } else if call.args.iter().any(|a| a.spread) {
                    self.diags.push(Diagnostic::error(
                        "E1312",
                        format!("`{}` doesn't accept a spread argument", call.name),
                        "call spread `f(...xs)` only works when the callee has a final `...` rest parameter"
                            .to_string(),
                        "pass arguments individually, or call a function whose last parameter is variadic"
                            .to_string(),
                        Some(call.name_span),
                    ));
                    for arg in call.args.iter_mut() {
                        if arg.spread {
                            self.infer(&mut arg.expr);
                        }
                    }
                }
    
                if call.args.len() != sig.params.len() {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`{}` expects {} argument{}, got {}",
                            call.name,
                            sig.params.len(),
                            if sig.params.len() == 1 { "" } else { "s" },
                            call.args.len()
                        ),
                        "every argument must match a parameter".to_string(),
                        format!("check the definition of `{}`", call.name),
                        Some(call.name_span),
                    ));
                }
            }
    
            let fn_type_params = self
                .trait_reg
                .fn_params
                .get(&call.name)
                .cloned()
                .unwrap_or_default();
            let mut generic_subst = HashMap::new();
            let mut pre_inferred: Vec<Option<Type>> = Vec::new();
            if !fn_type_params.is_empty() {
                for arg in call.args.iter_mut() {
                    pre_inferred.push(self.infer(&mut arg.expr));
                }
                let arg_types: Vec<Type> = pre_inferred.iter().filter_map(|t| t.clone()).collect();
                if arg_types.len() == call.args.len() {
                    match self.trait_reg.infer_fn_subst(
                        &sig,
                        &arg_types,
                        &fn_type_params,
                        self.expected_type.as_ref(),
                    ) {
                        Ok(s) => generic_subst = s,
                        Err(p) => self.diags.push(e0904(call.name_span, &p)),
                    }
                }
            }
            let effective_params: Vec<(AccessConvention, Type)> = if generic_subst.is_empty() {
                sig.params.clone()
            } else {
                sig.params
                    .iter()
                    .map(|(c, t)| (*c, self.trait_reg.instantiate_type(t, &generic_subst)))
                    .collect()
            };
            let args_pre_inferred = !generic_subst.is_empty() && pre_inferred.len() == call.args.len();
    
            let mut mut_borrowed: HashSet<String> = HashSet::new();
            let mut read_borrowed: HashSet<String> = HashSet::new();
    
            for (i, arg) in call.args.iter_mut().enumerate() {
                if let Expr::Ident(name, span) = &arg.expr {
                    if mut_borrowed.contains(name) {
                        self.diags.push(aliasing_while_mut(name, *span));
                    } else if arg.convention == AccessConvention::Write && read_borrowed.contains(name)
                    {
                        self.diags.push(aliasing_mut_after_read(name, *span));
                    }
                }
                if !sig.is_extern {
                    if let Some((AccessConvention::Read, pty)) = effective_params.get(i) {
                        if !pty.is_scalar() {
                            self.borrow_ctx = true;
                        }
                    }
                } else if let Some((_, pty)) = effective_params.get(i) {
                    if !pty.is_scalar() {
                        arg.flags.implicit_clone = true;
                    }
                }
                let saved_exp = self.expected_type.clone();
                let saved_esc = self.lambda_escapes;
                if let Some((param_conv, param_ty)) = effective_params.get(i) {
                    if matches!(param_ty, Type::Fn { .. }) {
                        self.expected_type = Some(param_ty.clone());
                        self.lambda_escapes = matches!(param_conv, AccessConvention::Move);
                    } else if matches!(param_ty, Type::IntN { .. } | Type::Float32) {
                        // D-SG9: let a fixed-width literal argument adopt the parameter's
                        // width and be range-checked at the literal.
                        self.expected_type = Some(param_ty.clone());
                    } else if matches!(param_ty, Type::Named(_) | Type::Option(_)) {
                        // D-ENUMDOT2=A: propagate named/optional type so `.Variant` can
                        // resolve to the correct enum from context.
                        self.expected_type = Some(param_ty.clone());
                    }
                }
                // D-EFF2 (callback param bound): snapshot the effect accumulator
                // before walking a function-typed argument so the callback's own
                // effect contribution (the delta) can be checked against the
                // parameter's declared bound after the walk.
                let cb_bound: Option<Vec<(String, Span)>> = match effective_params.get(i) {
                    Some((
                        _,
                        Type::Fn {
                            effect_bound: Some(b),
                            ..
                        },
                    )) => Some(b.clone()),
                    _ => None,
                };
                let cb_snapshot = cb_bound.as_ref().map(|_| {
                    (
                        self.fx_direct.clone(),
                        self.fx_edges.clone(),
                        self.fx_maximal,
                    )
                });
                let arg_ty = if args_pre_inferred {
                    pre_inferred.get(i).and_then(|t| t.clone())
                } else {
                    self.infer(&mut arg.expr)
                };
                self.expected_type = saved_exp;
                self.lambda_escapes = saved_esc;
                let Some((param_conv, param_ty)) = effective_params.get(i) else {
                    continue;
                };
                // D-EFF2: a function value passed to a function-typed parameter flows
                // its effects through to this caller (transparent flow-through).
                if matches!(param_ty, Type::Fn { .. }) {
                    self.attribute_fn_arg(&arg.expr);
                }
                // D-EFF2 (callback param bound): record the obligation now that the
                // callback's effects are in the accumulator (including the edge added
                // by `attribute_fn_arg` for a named-fn callback). Checked post-solve.
                if let (Some(bound), Some((bd, be, bm))) = (&cb_bound, &cb_snapshot) {
                    self.record_callback_obligation(bound, bd, be, *bm, arg.expr.span());
                }
                if arg.convention == AccessConvention::Write && !matches!(arg.expr, Expr::Ident(_, _)) {
                    self.diags.push(Diagnostic::error(
                        "E0202",
                        format!(
                            "`{}` needs a plain named binding after it",
                            Syntax::SIGIL_WRITE
                        ),
                        "write access (`&`) can only be granted to a named binding, not an expression"
                            .to_string(),
                        format!(
                            "bind the value first: `x {} ...` then pass `{}x`",
                            Syntax::SIGIL_BIND_MUT,
                            Syntax::SIGIL_WRITE
                        ),
                        Some(arg.span),
                    ));
                }
    
                if let Some(arg_ty) = &arg_ty {
                    let param_ty = self.resolve_type(param_ty.clone());
                    let arg_ty = self.resolve_type(arg_ty.clone());
                    let reported = self.check_type_assignable(&param_ty, &arg_ty, arg.expr.span());
                    // D-FIXARR1: [T#N] widens to [T] at a call site — compatible but codegen
                    // will emit .to_vec() on the argument.
                    let fixed_widens = matches!((&param_ty, &arg_ty),
                        (Type::List(pe), Type::FixedList { elem: ae, .. }) if pe == ae);
                    let compatible = arg_ty == param_ty
                        || fixed_widens
                        || (matches!(&param_ty, Type::Fn { .. })
                            && matches!(&arg_ty, Type::Fn { .. })
                            && fn_types_compatible(&param_ty, &arg_ty));
                    if !reported && !compatible {
                        // D-TYPEDTEXT1=D: a plain runtime `String` reaching a `Sql`/
                        // `Html` parameter — teach the injection-safety fix instead of
                        // a generic E0112.
                        if let Some(diag) = typed_text_mismatch(&param_ty, &arg_ty, arg.expr.span()) {
                            self.diags.push(diag);
                        } else
                        // D-TRAILBLOCK1: a trailing `{ }` block always desugars to a
                        // ZERO-parameter lambda argument. If the parameter it lands in
                        // isn't a zero-parameter function, that's not an ordinary type
                        // mismatch — teach the actual shape instead of a generic E0112.
                        if arg.flags.is_trailing_block {
                            self.diags.push(Diagnostic::error(
                                "E0334",
                                format!("`{}` doesn't take a trailing block", call.name),
                                format!(
                                    "a trailing `{{ }}` block fills a last argument that is a function taking no parameters; this call's last parameter is {}",
                                    param_ty.show()
                                ),
                                "pass it inside the parentheses, or give the function a zero-parameter last argument".to_string(),
                                Some(arg.expr.span()),
                            ));
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`{}` wants {} for argument {}, but this is {}",
                                    call.name,
                                    param_ty.show(),
                                    i + 1,
                                    arg_ty.show()
                                ),
                                "every argument must match its parameter's type".to_string(),
                                type_fix_hint(&param_ty, &arg_ty),
                                Some(arg.expr.span()),
                            ));
                        }
                    }
                }
    
                // D-LIN1 / E0142: a `#SingleUse` value may only be moved/consumed. If
                // it reaches a parameter that does not take ownership (`^`), the call
                // would borrow it (`&`/`view`/read) or copy it (an implicit clone) —
                // both are forbidden, since the value has exactly one use to give.
                if !matches!(param_conv, AccessConvention::Move) {
                    if let Expr::Ident(name, span) = &arg.expr {
                        let is_single_use = self
                            .lookup(name)
                            .map(|info| info.single_use_span.is_some())
                            .unwrap_or(false);
                        if is_single_use {
                            self.diags.push(e0142_aliased(name, &call.name, *span));
                            continue;
                        }
                    }
                }
    
                match (param_conv, arg.convention) {
                    (AccessConvention::Move, AccessConvention::Read) => {
                        if let Expr::Ident(name, span) = &arg.expr {
                            if is_cloneable(param_ty, self.registry, self.structs) {
                                arg.flags.implicit_clone = true;
                                // D-MEM1/S2 (was D-L0201 lint): a hard error now,
                                // regardless of liveness — no clone is ever silent.
                                let diag = self.e0209_implicit_clone(
                                    format!("implicit clone of `{}`", name),
                                    format!("`{}` expects to take ownership of this value", call.name),
                                    name,
                                    *span,
                                );
                                self.diags.push(diag);
                            } else {
                                self.diags.push(Diagnostic::error(
                                    "E0201",
                                    format!(
                                        "`{}` needs `{}` here — this value can't be copied",
                                        call.name,
                                        Syntax::SIGIL_MOVE
                                    ),
                                    format!(
                                        "parameter {} takes ownership (`^`); passing `{}` without `{}` would have to copy it, but this type can't be copied",
                                        i + 1,
                                        name,
                                        Syntax::SIGIL_MOVE
                                    ),
                                    format!(
                                        "write `{}{}` to move ownership to `{}`",
                                        Syntax::SIGIL_MOVE,
                                        name,
                                        call.name
                                    ),
                                    Some(*span),
                                ));
                            }
                        }
                    }
                    (AccessConvention::Move, AccessConvention::Move) => {
                        // The value is given away for real.
                        if let Expr::Ident(name, span) = &arg.expr {
                            if !param_ty.is_scalar() {
                                self.mark_moved(name.clone(), *span);
                            }
                        }
                    }
                    (AccessConvention::Write, AccessConvention::Read) => {
                        if let Expr::Ident(name, span) = &arg.expr {
                            self.diags.push(Diagnostic::error(
                                "E0202",
                                format!(
                                    "parameter `{}` requires write access (`&`) at the call site",
                                    name
                                ),
                                format!(
                                    "`{}` needs to edit (`&`) this value; passing it without `{}` grants only read access",
                                    call.name,
                                    Syntax::SIGIL_WRITE
                                ),
                                format!(
                                    "write `{}{}` when calling `{}`",
                                    Syntax::SIGIL_WRITE,
                                    name,
                                    call.name
                                ),
                                Some(*span),
                            ));
                        }
                    }
                    (AccessConvention::Write, AccessConvention::Write) => {
                        // `mut x` at the call site: x itself must be changeable.
                        if let Expr::Ident(name, span) = &arg.expr {
                            if let Some(info) = self.lookup(name) {
                                if !info.mutable {
                                    self.diags.push(Diagnostic::error(
                                        "E0111",
                                        format!(
                                            "`{}` was made with `{}`, so it can't be changed",
                                            name,
                                            Syntax::SIGIL_BIND_IMMUT
                                        ),
                                        format!(
                                            "`{}` will change this value, so it must be mutable (`{}`)",
                                            call.name,
                                            Syntax::SIGIL_BIND_MUT
                                        ),
                                        format!(
                                            "declare it with `{} {} ...`",
                                            name,
                                            Syntax::SIGIL_BIND_MUT
                                        ),
                                        Some(*span),
                                    ));
                                }
                            }
                        }
                    }
                    (AccessConvention::Read | AccessConvention::Write, AccessConvention::Move) => {
                        self.diags.push(Diagnostic::error(
                            "E0203",
                            format!(
                                "`{}` passed to a parameter that does not consume",
                                Syntax::SIGIL_MOVE
                            ),
                            "only move (`^`) parameters accept a moved value at the call site"
                                .to_string(),
                            format!(
                                "remove `{}` or change the parameter to take ownership (`{}`)",
                                Syntax::SIGIL_MOVE,
                                Syntax::SIGIL_MOVE
                            ),
                            Some(arg.span),
                        ));
                    }
                    _ => {}
                }
    
                if arg.convention == AccessConvention::Write {
                    if let Expr::Ident(name, _) = &arg.expr {
                        mut_borrowed.insert(name.clone());
                    }
                }
                if let (Some((param_conv, param_ty)), Expr::Ident(name, _)) =
                    (effective_params.get(i), &arg.expr)
                {
                    if matches!(param_conv, AccessConvention::Read)
                        && arg.convention == AccessConvention::Read
                        && !param_ty.is_scalar()
                    {
                        read_borrowed.insert(name.clone());
                    }
                }
    
                if self.loop_depth > 0 {
                    if let Expr::Ident(name, span) = &arg.expr {
                        if let Some(info) = self.lookup(name) {
                            if matches!(info.ty, Type::Shared(_)) {
                                arg.flags.shared_auto_clone = true;
                                self.diags.push(Diagnostic::lint(
                                    "L0202",
                                    format!(
                                        "auto-cloned `{}` inside a loop; consider hoisting or caching",
                                        name
                                    ),
                                    "shared handles are cloned when used across a loop boundary"
                                        .to_string(),
                                    format!("hoist `{}` before the loop, or clone once outside", name),
                                    Some(*span),
                                ));
                            }
                        }
                    }
                }
            }
    
            // D-ANY-JAI1: put the (already fully checked) trait-bounded variadic
            // tail back so codegen sees the real call shape.
            if let Some(tail) = bound_variadic_tail {
                call.args.extend(tail);
            }
    
            Some(sig.return_type.as_ref().map(|t| {
                let t = if generic_subst.is_empty() {
                    t.clone()
                } else {
                    self.trait_reg.instantiate_type(t, &generic_subst)
                };
                self.resolve_type(t)
            }))
        }
    
}
