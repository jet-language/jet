use crate::AST::{AccessConvention, BinOp, Call, CallablePolicyChain, Expr, StrPart, Type};
use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Generics::{e0904, e0905};
use crate::Sema::Bundle::{fn_types_compatible, func_sig_to_fn_type};
use crate::Sema::Checker;
use crate::Sema::CheckerCoreLib::{
    io_error_ty, is_simd_lane_type, math_constructor_arg_types, overflow_opt_in_error, result_ty,
    wrong_core_arity,
};
use crate::Sema::CheckerOwnership::{e0142_aliased, e0143_drop_unaudited};
use crate::Sema::Diagnostics::{
    edit_distance, is_cloneable, is_printable, type_fix_hint, type_is_copy,
    typed_text_mismatch,
};
use crate::Sema::Effects::builtin_effect;
use crate::Sema::FFI::e3211;
use crate::Syntax;
use jet_foundation::Prelude as CorePrelude;
use std::collections::HashMap;
impl<'a> Checker<'a> {
        /// D-CALLPOLICY1=E: `apply(p1, …, fn)` replaces the callable's policy
        /// chain exactly. The last argument is the callable value; policy
        /// expressions are checked as typed values and are not ordinary calls.
        fn check_callable_policy_apply(&mut self, call: &mut Call) -> Option<Type> {
            let Some((callee, policy_args)) = call.args.split_last_mut() else {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    "`apply` needs a callable value".to_string(),
                    "a callable policy is applied to one function value; an empty chain is written `apply(fn)`"
                        .to_string(),
                    "write `apply(load_user)` or `apply(retry(3), load_user)`".to_string(),
                    Some(call.name_span),
                ));
                return None;
            };
            let policy_exprs: Vec<Expr> = policy_args.iter().map(|arg| arg.expr.clone()).collect();
            let chain = match CallablePolicyChain::parse(&policy_exprs) {
                Ok(chain) => chain,
                Err(reason) => {
                    self.diags.push(Diagnostic::error(
                        "E0355",
                        format!("`apply` received an invalid callable policy: {reason}"),
                        "policy values are checked before application and cannot change a callable's signature"
                            .to_string(),
                        "use `cache(…)`, `retry(…)`, `trace(…)`, `registration(…)`, or `route(…)`"
                            .to_string(),
                        Some(call.name_span),
                    ));
                    return None;
                }
            };
            for argument in policy_args {
                if let Expr::Call(policy_call) = &mut argument.expr {
                    for value in &mut policy_call.args {
                        self.infer(&mut value.expr);
                    }
                }
            }
            if callee.label.is_some() || callee.spread {
                self.diags.push(Diagnostic::error(
                    "E0764",
                    "the callable value in `apply` cannot have a label or spread".to_string(),
                    "the final argument is the value being wrapped; policy values come before it"
                        .to_string(),
                    "write `apply(policy(…), function)`".to_string(),
                    Some(callee.span),
                ));
                return None;
            }
            let callee_ty = match &callee.expr {
                Expr::Ident(name, span) => self.funcs.get(name).cloned().map(|sig| {
                    self.record_current_function_reference(name, *span);
                    func_sig_to_fn_type(&sig)
                }),
                _ => self.infer(&mut callee.expr),
            }?;
            let Some(replaced) = callee_ty.replace_callable_policies(chain.clone()) else {
                self.diags.push(Diagnostic::error(
                    "E0803",
                    format!("`apply` can wrap only a function value, not {}", callee_ty.show()),
                    "a callable policy is a function-to-function value with the same checked signature"
                        .to_string(),
                    "pass a function or function-typed binding as the final argument".to_string(),
                    Some(callee.span),
                ));
                return None;
            };
            if !fn_types_compatible(&callee_ty, &replaced)
                || !Type::obligations_satisfy(&callee_ty, &replaced)
            {
                self.diags.push(Diagnostic::error(
                    "E0803",
                    "callable policy replacement changed the checked function signature"
                        .to_string(),
                    "a policy is a function-to-function value with the same labels, access, effects, errors, variadics, and view provenance"
                        .to_string(),
                    "apply only a typed callable policy chain".to_string(),
                    Some(callee.span),
                ));
                return None;
            }
            // This fact is consumed by the shared lowerer. It lives on the
            // callable argument so no engine invents a second policy path.
            callee.flags.callable_policy = Some(chain);
            Some(replaced)
        }

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

        /// D-NUMWIDEN-CROSS1=E: mark one integer value as accepting an inexact
        /// float crossing. The surrounding widening consumes this marker.
        fn check_numeric_approx(&mut self, call: &mut Call) -> Option<Type> {
            if call.args.len() != 1 {
                for arg in call.args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!("`{}` takes one whole number", Syntax::BUILTIN_APPROX),
                    "`approx(value)` accepts possible precision loss at one integer-to-float crossing".to_string(),
                    format!("write `{}(value)`", Syntax::BUILTIN_APPROX),
                    Some(call.name_span),
                ));
                return None;
            }
            let ty = self.infer(&mut call.args[0].expr)?;
            if !ty.is_integer() {
                self.diags.push(Diagnostic::error(
                    "E0109",
                    format!("`{}` needs a whole number, but this is {}", Syntax::BUILTIN_APPROX, ty.show()),
                    "`approx(value)` only opts one integer-to-float crossing out of its exactness check".to_string(),
                    "remove `approx`, or pass a whole number that is crossing into a decimal".to_string(),
                    Some(call.args[0].expr.span()),
                ));
                return None;
            }
            call.widen_approx = true;
            Some(ty)
        }
    
        pub(crate) fn check_call(&mut self, call: &mut Call, _as_value: bool) -> Option<Option<Type>> {
            if call.name == Syntax::BUILTIN_CHECKED_TEXT_WRAP {
                let Some(Type::Named(type_name)) = call.type_args.first().cloned() else {
                    return Some(None);
                };
                if call.args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        "the checked text constructor needs one encoded String".to_string(),
                        "the compiler-internal checked text wrapper receives the head's complete encoded body".to_string(),
                        "use a declared text head literal or its audited `.raw()` escape".to_string(),
                        Some(call.name_span),
                    ));
                    return Some(None);
                }
                let arg_ty = self.infer(&mut call.args[0].expr);
                if let Some(ty) = arg_ty {
                    self.check_type_assignable(&Type::String, &ty, call.args[0].expr.span());
                }
                let result = crate::Sema::checked_text_type(type_name);
                call.resolved_ret = Some(result.clone());
                return Some(Some(result));
            }
            if call.name == "apply"
                && self.funcs.get(&call.name).is_none()
                && self.lookup(&call.name).is_none()
            {
                return self.check_callable_policy_apply(call).map(Some);
            }
            // D-SHAPE-RESOURCE2=A: `close(^value)` is ambient syntax sugar for
            // the sole nominal `Close.close(^self)` protocol. It is not a
            // name-based/free-function cleanup hook.
            if call.name == Syntax::RESOURCE_CLOSE {
                if call.args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!("`close` takes exactly one resource, got {}", call.args.len()),
                        "`close` consumes one value through its nominal `Close` implementation and the move-capability marker `^`"
                            .to_string(),
                        "write `close(^resource)` with the move-capability marker `^`".to_string(),
                        Some(call.name_span),
                    ));
                    for arg in call.args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return Some(None);
                }
                let arg = &mut call.args[0];
                let ty = self.infer(&mut arg.expr);
                if arg.convention != AccessConvention::Move {
                    self.diags.push(Diagnostic::error(
                        "E0201",
                        "`close` takes ownership of its resource".to_string(),
                        "`Close.close(^self)` is consuming through the move-capability marker `^`, so cleanup runs on exactly one owner"
                            .to_string(),
                        "write `close(^resource)` with the move-capability marker `^`".to_string(),
                        Some(arg.span),
                    ));
                } else if let Some(ty) = &ty {
                    let nominal = match ty {
                        Type::Named(name) | Type::Apply { name, .. } => Some(name.as_str()),
                        _ => None,
                    };
                    if !nominal.is_some_and(|name| {
                        self.trait_reg
                            .implements_trait(name, Syntax::TRAIT_CLOSE)
                    }) {
                        self.diags.push(e0905(&ty.name(), Syntax::TRAIT_CLOSE, arg.expr.span(), false));
                    }
                    if let Expr::Ident(name, span) = &arg.expr {
                        if !ty.is_scalar() {
                            self.mark_moved(name.clone(), *span);
                        }
                    }
                }
                return Some(None);
            }
            // D-NUMOPS1: `wrapping`/`saturating`/`checked` opt-ins wrap a single integer
            // `+`/`-`/`*`/`/`. A user-defined function of the same name shadows them.
            if matches!(
                call.name.as_str(),
                Syntax::BUILTIN_WRAPPING | Syntax::BUILTIN_SATURATING | Syntax::BUILTIN_CHECKED
            ) && !self.funcs.contains_key(&call.name)
            {
                return Some(self.check_overflow_opt_in(call));
            }
            if call.name == Syntax::BUILTIN_APPROX && !self.funcs.contains_key(&call.name) {
                return self.check_numeric_approx(call).map(Some);
            }
            // D-EFF1: an ambient prelude builtin (`print`/`input`) contributes the `IO`
            // effect, unless a user function of the same name shadows it (in which
            // case the edge to that user function is recorded below).
            if !self.funcs.contains_key(&call.name) {
                if let Some(e) = builtin_effect(&call.name) {
                    self.record_effect(e.name(), call.name_span);
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
                    format!("`{}` is not in Jet; use the `task` keyword instead", call.name),
                    "Jet uses blocking tasks and channels, not async/await — simpler and race-free"
                        .to_string(),
                    "write `task your_work()` or `task { … }`"
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
                Syntax::FOREIGN_MUTEX
                    | Syntax::FOREIGN_LOCK
                    | "RwLock"
                    | "mutex"
                    | "Semaphore"
                    | "semaphore"
            ) {
                let semaphore = matches!(call.name.as_str(), "Semaphore" | "semaphore");
                self.diags.push(Diagnostic::error(
                    "E0041",
                    if semaphore {
                        format!(
                            "`{}` is not in Jet; use a bounded channel as a token pool",
                            call.name
                        )
                    } else {
                        format!(
                            "`{}` is not in Jet; share data through channels",
                            call.name
                        )
                    },
                    if semaphore {
                        "each received token admits one worker until that worker sends the token back"
                            .to_string()
                    } else {
                        "Jet avoids shared mutable state: tasks communicate by sending messages, not sharing memory"
                            .to_string()
                    },
                    if semaphore {
                        "create `tasks.channel<Int>(capacity: N)`, seed N tokens, receive one before work, and send it back afterward"
                            .to_string()
                    } else {
                        "import `core.tasks as tasks`, create a channel, and use `sender.send`/`channel.receive`"
                            .to_string()
                    },
                    Some(call.name_span),
                ));
                for a in call.args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }

            // D-NAME-ALIAS1=A / D-PRELUDEX1=A: every readable prelude name
            // follows the one file-level opt-out. print and input retain their
            // detailed diagnostics below.
            if self.no_prelude
                && call.name != Syntax::BUILTIN_PRINT
                && call.name != Syntax::BUILTIN_INPUT
                && !self.funcs.contains_key(&call.name)
                && self.lookup(&call.name).is_none()
                && CorePrelude::entry(&call.name).is_some()
            {
                self.diags.push(Diagnostic::error(
                    "E0429",
                    format!(
                        "{} is not ambient here — this file opted out with #{}",
                        call.name,
                        Syntax::MARKER_NO_PRELUDE
                    ),
                    format!(
                        "#{} closes the readable Core prelude for this file",
                        Syntax::MARKER_NO_PRELUDE
                    ),
                    "write the qualified Core call, or remove #NoPrelude".to_string(),
                    Some(call.name_span),
                ));
                for arg in call.args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return Some(None);
            }
    
            if call.name == Syntax::BUILTIN_PRINT
                && self.funcs.get(Syntax::BUILTIN_PRINT).is_none()
                && self.lookup(Syntax::BUILTIN_PRINT).is_none()
            {
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
                            Syntax::BUILTIN_INPUT,
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
                if call.args.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!(
                            "`{}` needs at least one thing to print",
                            Syntax::BUILTIN_PRINT
                        ),
                        "printing nothing isn't meaningful".to_string(),
                        format!("e.g. {}(\"hello\")", Syntax::BUILTIN_PRINT),
                        Some(call.name_span),
                    ));
                    return None;
                }
                // D-VERDICT-1321-1: variadic print — every argument renders on
                // its own line. Each argument passes the same printable checks.
                for arg in call.args.iter_mut() {
                    self.borrow_ctx = true; // print borrows via `.jet_show()`
                    if let Some(t) = self.infer(&mut arg.expr) {
                        if !is_printable(&t, self.registry, self.trait_reg)
                            && !self.is_unit_type(&t)
                        {
                            if crate::Sema::Diagnostics::is_secret_bearing_crypto_type(&t) {
                                self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!("secret-bearing `{}` cannot be printed", t.name()),
                                    "printing could copy cryptographic secret material into terminal output or logs".to_string(),
                                    "print a public operation label or key identifier instead".to_string(),
                                    Some(arg.expr.span()),
                                ));
                            } else if crate::Sema::Diagnostics::is_one_pass_source(&t) {
                                let fix = crate::Sema::Diagnostics::one_pass_materializer(&t)
                                    .map_or_else(
                                        || {
                                            "consume it with a `loop` and print each item instead"
                                                .to_string()
                                        },
                                        |call| {
                                            format!(
                                                "materialize it first: add `{call}` before printing"
                                            )
                                        },
                                    );
                                self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!("`{}` cannot show the one-pass source {}", Syntax::BUILTIN_PRINT, t.show()),
                                    "reading this value consumes it, so showing it would spend the only pass".to_string(),
                                    fix,
                                    Some(arg.expr.span()),
                                ));
                            } else {
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
                    }
                }
                return Some(None);
            }
    
            // D-NAME-ALIAS1=A: `input` is prelude-declared — no `use core.io` needed.
            // Resolves to the same semantics as `io.input`: optional String prompt,
            // returns Result(String, IOError). Shadowed by any user-defined `input`.
            // D-PRELUDEX1=A: `#NoPrelude` turns the readable prelude off.
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
                            Syntax::BUILTIN_INPUT,
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
    
            if call.name == Syntax::BUILTIN_PANIC
                && self.funcs.get(Syntax::BUILTIN_PANIC).is_none()
                && self.lookup(Syntax::BUILTIN_PANIC).is_none()
            {
                // D-METADEPTH2: retain panic reachability in the checked call
                // graph consumed by ProgramInfo; this sentinel is not an effect.
                self.fx_edges.insert("__jet_panic__".to_string());
                self.check_panic_call(call);
                return Some(None);
            }
    
            if call.name == Syntax::BUILTIN_REQUIRE
                && self.funcs.get(Syntax::BUILTIN_REQUIRE).is_none()
                && self.lookup(Syntax::BUILTIN_REQUIRE).is_none()
            {
                self.check_require_call(call);
                return Some(None);
            }
    
            if call.name == Syntax::BUILTIN_REQUIRE_EQ {
                self.check_require_eq_call(call);
                return Some(None);
            }
    
            if call.name == Syntax::BUILTIN_FIND
                && self.in_comptime
                && self.funcs.get(Syntax::BUILTIN_FIND).is_none()
                && self.lookup(Syntax::BUILTIN_FIND).is_none()
            {
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
            if call.name == Syntax::BUILTIN_CONSUME
                && self.funcs.get(Syntax::BUILTIN_CONSUME).is_none()
                && self.lookup(Syntax::BUILTIN_CONSUME).is_none()
            {
                if call.args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!("`{}` discards exactly one value", Syntax::BUILTIN_CONSUME),
                        "`drop` throws a single value away, running its cleanup".to_string(),
                        format!("e.g. {}(x)", Syntax::BUILTIN_CONSUME),
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
                // D-NAME-WALK1=A: an inline body overlays its enclosing file's
                // unqualified maps with its own imports. The local scope wins;
                // the fallback preserves ordinary file-level `use` behavior.
                let inline_mangled = self
                    .inline_module
                    .as_ref()
                    .and_then(|module| {
                        self.inline_unqualified
                            .get(&(module.clone(), call.name.clone()))
                    })
                    .cloned();
                // D-MOD3: check unqualified inline-module imports (e.g. `use math.clamp`).
                if let Some(mangled) = inline_mangled.or_else(|| self.unqualified.get(&call.name).cloned()) {
                    let alias = mangled.split("__").next().unwrap_or(&mangled).to_string();
                    let result = self.infer_code_module_call(
                        &alias,
                        &mangled,
                        call.name_span,
                        call.name_span,
                        &call.type_args,
                        &mut call.args,
                    );
                    return Some(result);
                }
                let inline_file = self
                    .inline_module
                    .as_ref()
                    .and_then(|module| {
                        self.inline_unqualified_file
                            .get(&(module.clone(), call.name.clone()))
                    })
                    .cloned();
                // D-MOD3: check unqualified file-module imports (e.g. `use math.clamp` for a file module).
                if let Some((fn_name, mod_idx)) = inline_file.or_else(|| self.unqualified_file.get(&call.name).cloned()) {
                    let result = self.infer_import_call(
                        mod_idx,
                        &fn_name,
                        call.name_span,
                        call.name_span,
                        &call.type_args,
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

            // D-ZIPPAD1: free `zip`/`zip_short`/`zip_pad` calls are built-in
            // sequence family calls when no user function shadows the name.
            // Check them before ordinary function lookup so arbitrary input
            // counts keep their concrete element types.
            if self.funcs.get(&call.name).is_none()
                && self.lookup(&call.name).is_none()
                && matches!(call.name.as_str(), "zip" | "zip_short" | "zip_pad")
            {
                if let Some(result) = self.check_zip_family_free(call) {
                    return Some(result);
                }
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
    
            // D-SHAPE-CONVERT1=A: `Type(value)` is not a conversion alias.
            // Distinct and unit values use the same destination-owned spelling
            // as every other explicit conversion.
            if self.funcs.get(&call.name).is_none() {
                if let Some(base_ty) = self.registry.distinct_base(&call.name).cloned() {
                    let method = Syntax::conversion_method_for_source(&base_ty.name());
                    self.diags.push(Diagnostic::error(
                        "E0128",
                        format!("`{}(value)` is not a distinct conversion", call.name),
                        "explicit conversions are owned by the destination type and name their source"
                            .to_string(),
                        format!("write `{}.{method}(value)`", call.name),
                        Some(call.name_span),
                    ));
                    for arg in call.args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
            }

            // D-ZIPPAD1: free zip-family calls are compiler-owned variadic
            // forms. Keep them ahead of the ordinary unknown-function error;
            // a user declaration still wins through the shadowing check in
            // `check_zip_family_free`.
            if let Some(result) = self.check_zip_family_free(call) {
                return Some(result);
            }
    
            let Some(mut sig) = self.text_head_function_sig(&call.name) else {
                let mut fix = format!(
                    "define it first ({} {}() {{ ... }}), or call one that exists",
                    Syntax::KW_FN,
                    call.name
                );
                let mut candidates = Vec::new();
                let prelude_cands: Vec<&str> = if self.no_prelude {
                    Vec::new()
                } else {
                    CorePrelude::names().collect()
                };
                for cand in self
                    .funcs
                    .keys()
                    .map(|s| s.as_str())
                    .chain(
                        self.text_head_context
                            .into_iter()
                            .flat_map(|context| context.sigs.keys().map(String::as_str)),
                    )
                    .chain(prelude_cands.iter().copied())
                {
                    let d = edit_distance(&call.name, cand);
                    if d <= 2 && !candidates.iter().any(|(known, _)| *known == cand) {
                        candidates.push((cand, d));
                    }
                }
                let suggestion = candidates
                    .iter()
                    .map(|(_, distance)| *distance)
                    .min()
                    .map(|distance| {
                        candidates
                            .iter()
                            .filter(|(_, candidate_distance)| *candidate_distance == distance)
                            .map(|(candidate, _)| *candidate)
                            .collect::<Vec<_>>()
                    })
                    .filter(|matches| matches.len() == 1)
                    .map(|matches| matches[0]);
                if let Some(cand) = suggestion {
                    fix = format!("did you mean `{}`?", cand);
                }
                let mut diagnostic = Diagnostic::error(
                    "E0102",
                    format!("nothing named `{}` exists here", call.name),
                    format!(
                        "only functions that have been defined (or built in, like `{}` / `{}`) can be called",
                        Syntax::BUILTIN_PRINT, Syntax::BUILTIN_INPUT
                    ),
                    fix,
                    Some(call.name_span),
                );
                if let Some(cand) = suggestion {
                    diagnostic = diagnostic.with_edit(TextEdit {
                        span: call.name_span,
                        new_text: cand.to_string(),
                    });
                }
                self.diags.push(diagnostic);
                for arg in call.args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            };
            self.record_current_function_reference(&call.name, call.name_span);

            self.check_foreign_transaction_call(&sig, &call.name, call.name_span);

            // D-EFF1: record the call-graph edge for transitive effect inference.
            // A foreign (`extern`) callee has an un-inspectable body, so it forces
            // the maximal effect set; a Jet callee's effects flow in via its edge.
            if sig.is_extern {
                if let Some(effect) = &sig.foreign_effect_root {
                    self.record_open_memory_dispatch(call.name_span, "foreign function body");
                    self.record_effect(effect, call.name_span);
                } else {
                    self.record_maximal(call.name_span);
                }
            } else {
                self.record_edge(call.name.clone(), call.name_span);
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
    
            // D-APILABEL1=A: one binder resolves labels, zones, reordering, and
            // skipped defaults. It rewrites `call.args` into declaration order
            // and marks each argument with where the caller wrote it, so
            // lowering can keep the source evaluation order.
            let params = crate::Sema::CallBinder::bind_params_from_sig(&sig);
            let bound = crate::Sema::CallBinder::bind_call_args(
                &call.name,
                &params,
                &mut call.args,
                call.name_span,
                &mut self.diags,
            );
            self.register_binder_refs(&call.args);
            if bound.is_none() {
                // The call's arguments never resolved to parameters, so
                // arity and per-position type errors below would all be
                // about slots that do not exist. Report the argument
                // expressions' own problems and stop.
                for arg in call.args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return Some(sig.return_type.clone());
            }

            // E3211 (card #436): a `String` literal with a known interior NUL
            // byte can't cross into a C-boundary function — `CString::new`
            // would fail (C strings are NUL-terminated, not length-prefixed).
            // Run this after the shared binder so a reordered labelled call is
            // checked against the declaration slot it actually reaches. A
            // runtime-built String is caught by the codegen panic instead (see
            // `Codegen/CModule.rs`'s `NUL_PANIC`).
            if sig.is_c_abi {
                for (index, arg) in call.args.iter().enumerate() {
                    // The binder has already rewritten `call.args` into
                    // declaration order. `source_index` is only the caller's
                    // original position for evaluation-order lowering.
                    let declaration_index = index;
                    let is_string_param = matches!(
                        sig.params.get(declaration_index),
                        Some((_, Type::String))
                    );
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
                .text_head_function_params(&call.name)
                .or_else(|| self.trait_reg.fn_params.get(&call.name).cloned())
                .unwrap_or_default();
            let mut call_access = self.call_access_frame();
            let mut generic_subst = HashMap::new();
            let mut pre_inferred: Vec<Option<Type>> = Vec::new();
            if !fn_type_params.is_empty() && !call.type_args.is_empty() {
                if call.type_args.len() != fn_type_params.len() {
                    self.diags.push(Diagnostic::error(
                        "E0119",
                        format!(
                            "{} expects {} type argument{}, got {}",
                            call.name,
                            fn_type_params.len(),
                            if fn_type_params.len() == 1 { "" } else { "s" },
                            call.type_args.len()
                        ),
                        "a generic call must provide one type for every declared type parameter"
                            .to_string(),
                        format!(
                            "write {} with {} type argument{}",
                            call.name,
                            fn_type_params.len(),
                            if fn_type_params.len() == 1 { "" } else { "s" }
                        ),
                        Some(call.name_span),
                    ));
                } else {
                    for (param, actual) in fn_type_params.iter().zip(&call.type_args) {
                        let actual = self.resolve_type(actual.clone());
                        self.check_declared_type(&actual, call.name_span);
                        for bound in &param.bounds {
                            if !self.type_satisfies_bound(&actual, bound) {
                                self.diags.push(e0905(
                                    &actual.name(),
                                    bound,
                                    call.name_span,
                                    false,
                                ));
                            }
                        }
                        generic_subst.insert(param.name.clone(), actual);
                    }
                }
            } else if !fn_type_params.is_empty() {
                for (index, arg) in call.args.iter_mut().enumerate() {
                    pre_inferred.push(self.with_call_access(&mut call_access, |checker| {
                        if let Some((param_conv, param_ty)) = sig.params.get(index) {
                            checker.check_call_argument_access(
                                arg,
                                *param_conv,
                                param_ty,
                                !sig.is_extern,
                            );
                        }
                        // Move sites are diagnosed as E0219 (pin/change), not E0220.
                        let suppress = checker.suppress_partial_move_root_read;
                        if arg.convention == AccessConvention::Move {
                            checker.suppress_partial_move_root_read = true;
                        }
                        let inferred = checker.infer(&mut arg.expr);
                        checker.suppress_partial_move_root_read = suppress;
                        checker.check_call_argument_captures(&arg.expr);
                        inferred
                    }));
                }
                let arg_types: Vec<Type> = pre_inferred.iter().filter_map(|t| t.clone()).collect();
                if arg_types.len() == call.args.len() {
                    match self.trait_reg.infer_fn_subst_without_bounds(
                        &sig,
                        &arg_types,
                        &fn_type_params,
                        self.expected_type.as_ref(),
                    ) {
                        Ok(s) => {
                            if let Some((ty, bound)) = fn_type_params.iter().find_map(|param| {
                                let ty = s.get(&param.name)?;
                                param
                                    .bounds
                                    .iter()
                                    .find(|bound| !self.type_satisfies_bound(ty, bound))
                                    .map(|bound| (ty, bound))
                            }) {
                                self.diags.push(e0905(
                                    &ty.name(),
                                    bound,
                                    call.name_span,
                                    false,
                                ));
                            }
                            generic_subst = s;
                        }
                        Err(p) => self.diags.push(e0904(call.name_span, &p)),
                    }
                }
            } else if !call.type_args.is_empty() {
                self.diags.push(Diagnostic::error(
                    "E0119",
                    format!("{} is not generic", call.name),
                    "only functions declared with type parameters accept call-site type arguments"
                        .to_string(),
                    format!("call {} without type arguments", call.name),
                    Some(call.name_span),
                ));
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
    
            for (i, arg) in call.args.iter_mut().enumerate() {
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
                let memory_multiplier = self.memory_control_multiplier;
                if matches!(effective_params.get(i), Some((_, Type::Fn { .. }))) {
                    self.memory_control_multiplier = None;
                }
                let arg_ty = if args_pre_inferred {
                    pre_inferred.get(i).and_then(|t| t.clone())
                } else {
                    self.with_call_access(&mut call_access, |checker| {
                        if let Some((param_conv, param_ty)) = effective_params.get(i) {
                            checker.check_call_argument_access(
                                arg,
                                *param_conv,
                                param_ty,
                                !sig.is_extern,
                            );
                        }
                        // Move sites are diagnosed as E0219 (pin/change), not E0220.
                        let suppress = checker.suppress_partial_move_root_read;
                        if arg.convention == AccessConvention::Move {
                            checker.suppress_partial_move_root_read = true;
                        }
                        let inferred = checker.infer(&mut arg.expr);
                        checker.suppress_partial_move_root_read = suppress;
                        checker.check_call_argument_captures(&arg.expr);
                        inferred
                    })
                };
                if sig.is_pure
                    && effective_params
                        .get(i)
                        .is_some_and(|(_, ty)| crate::Sema::Diagnostics::is_clock_type(ty))
                    && arg_ty
                        .as_ref()
                        .is_some_and(|ty| !crate::Sema::Diagnostics::is_deterministic_clock_type(ty))
                {
                    self.diags.push(crate::Sema::e3403(
                        &format!("an unproven Clock passed to pure `{}`", call.name),
                        Some(arg.expr.span()),
                    ));
                }
                self.memory_control_multiplier = memory_multiplier;
                if effective_params.get(i).is_some_and(|(_, ty)| {
                    crate::Sema::FFI::is_callback_boundary_param(sig.is_c_abi, ty)
                }) {
                    let safe = match &arg.expr {
                        Expr::Ident(callback, _) => self.funcs.get(callback).is_some_and(|f| {
                            !f.is_extern && f.is_foreign_thread_safe
                        }) || arg_ty.as_ref().is_some_and(|ty| {
                            crate::Sema::FFI::cpp_callback_abi_type(ty).is_some()
                        }),
                        Expr::Lambda(lam) => crate::Sema::foreign_thread_safe_lambda(lam),
                        _ => false,
                    };
                    if safe {
                        arg.flags.c_callback_symbol = true;
                    } else if let Some((_, param_ty)) = effective_params.get(i) {
                        self.diags.push(crate::Sema::FFI::e3203(param_ty, arg.expr.span()));
                    }
                }
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
                let write_needs_a_name =
                    arg.convention == AccessConvention::Write && !matches!(arg.expr, Expr::Ident(_, _));
                if write_needs_a_name {
                    self.diags.push(Diagnostic::error(
                        "E0202",
                        format!(
                            "{} needs a plain named binding after it",
                            crate::Sema::Diagnostics::WRITE_CAPABILITY_MARKER
                        ),
                        "write access from the write-capability marker `&` can only be granted to a named binding, not an expression"
                            .to_string(),
                        self.non_name_write_argument_fix(&arg.expr),
                        Some(arg.span),
                    ));
                }
    
                if let Some(arg_ty) = &arg_ty {
                    let param_ty = self.resolve_type(param_ty.clone());
                    let mut arg_ty = self.resolve_type(arg_ty.clone());
                    if *param_conv == AccessConvention::Write
                        && matches!(
                            &param_ty,
                            Type::Apply { name, .. }
                                if name == crate::Syntax::TYPE_SHARED_GUARD
                        )
                        && !matches!(
                            &arg_ty,
                            Type::Tagged { marker, .. }
                                if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::SharedGuardEdit))
                        )
                        && !matches!(
                            &arg.expr,
                            Expr::Ident(name, _)
                                if self.lookup(name).is_some_and(|info| {
                                    info.param_conv == Some(AccessConvention::Write)
                                })
                        )
                    {
                        self.diags.push(Diagnostic::error(
                            "E0205",
                            "a read `SharedGuard` cannot enter a write helper".to_string(),
                            "the helper can edit through this parameter, so its caller must hold an exclusive Shared guard".to_string(),
                            "create the guard with `guard_edit()` before passing it with the write-capability marker `&`"
                                .to_string(),
                            Some(arg.expr.span()),
                        ));
                    }
                    if param_ty != arg_ty
                        && self.implicitly_convert_unit(&mut arg.expr, &param_ty, &arg_ty)
                    {
                        arg_ty = param_ty.clone();
                    }
                    if *param_conv != AccessConvention::Write
                        && param_ty != arg_ty
                        && arg_ty.numeric_widening_to(&param_ty).is_some()
                    {
                        self.widen_numeric_expr(&mut arg.expr, &arg_ty, &param_ty);
                        arg_ty = param_ty.clone();
                    }
                    let reads_expiring_secret_loan = arg.convention == AccessConvention::Read
                        && crate::Sema::Diagnostics::expiring_secret_loan_matches(
                            &param_ty, &arg_ty,
                        );
                    // E0202 already told the writer this argument needs a name;
                    // a follow-up type mismatch on the same span is noise.
                    let reported = write_needs_a_name
                        || reads_expiring_secret_loan
                        || self.check_type_assignable(&param_ty, &arg_ty, arg.expr.span());
                    // D-FIXARR1: [T#N] widens to [T] at a call site — compatible but codegen
                    // will emit .to_vec() on the argument.
                    let fixed_widens = matches!((&param_ty, &arg_ty),
                        (Type::List(pe), Type::FixedList { elem: ae, .. })
                            if pe == ae && Type::obligations_satisfy(pe, ae));
                    let union_widens = matches!(
                        &param_ty,
                        Type::Union(members) if members.iter().any(|m| {
                            m == &arg_ty && Type::obligations_satisfy(m, &arg_ty)
                        })
                    );
                    let exact_or_obligation_compatible =
                        if matches!(&param_ty, Type::Fn { .. })
                            && matches!(&arg_ty, Type::Fn { .. })
                        {
                            fn_types_compatible(&param_ty, &arg_ty)
                        } else {
                            arg_ty == param_ty
                                && Type::obligations_satisfy(&param_ty, &arg_ty)
                        };
                    let compatible = exact_or_obligation_compatible
                        || fixed_widens
                        || union_widens
                        || reads_expiring_secret_loan;
                    if !reported && !compatible {
                        // D-TYPEDTEXT1=D: a plain runtime `String` reaching a `SQL`/
                        // `HTML` parameter — teach the injection-safety fix instead of
                        // a generic E0112.
                        if let Some(diag) = typed_text_mismatch(&param_ty, &arg_ty, arg.expr.span()) {
                            self.diags.push(diag);
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
                            if type_is_copy(param_ty) {
                                // Copy values cross an owning parameter by bits.
                            } else if !self.is_resource_type(param_ty)
                                && is_cloneable(param_ty, self.registry)
                            {
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
                                        "`{}` needs the move-capability marker `^` here — this value can't be copied",
                                        call.name,
                                    ),
                                    format!(
                                        "parameter {} takes ownership through the move-capability marker `^`; passing `{}` without that marker would have to copy it, but this type can't be copied",
                                        i + 1,
                                        name
                                    ),
                                    format!(
                                        "write the move-capability marker `^` (`{}{}`) to move ownership to `{}`",
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
                            if !type_is_copy(param_ty) {
                                self.mark_moved(name.clone(), *span);
                            }
                        }
                    }
                    (AccessConvention::Write, AccessConvention::Read) => {
                        if let Expr::Ident(name, span) = &arg.expr {
                            self.diags.push(Diagnostic::error(
                                "E0202",
                                format!(
                                    "parameter `{}` requires the write-capability marker `&` at the call site",
                                    name
                                ),
                                format!(
                                    "`{}` needs to edit this value with the write-capability marker `&`; passing it without that marker grants only read access",
                                    call.name
                                ),
                                format!(
                                    "write the write-capability marker `&` (`{}{}`) when calling `{}`",
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
                                    let mut diagnostic = Diagnostic::error(
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
                                    );
                                    if let Some(sigil_span) = info.binding_sigil_span {
                                        diagnostic = diagnostic.with_edit(TextEdit {
                                            span: sigil_span,
                                            new_text: Syntax::SIGIL_BIND_MUT.to_string(),
                                        });
                                    }
                                    self.diags.push(diagnostic);
                                }
                            }
                        }
                    }
                    (AccessConvention::Read | AccessConvention::Write, AccessConvention::Move) => {
                        self.diags.push(Diagnostic::error(
                            "E0203",
                            "a value was passed with the move-capability marker `^` to a parameter that does not consume".to_string(),
                            "only parameters declared with the move-capability marker `^` accept a moved value at the call site"
                                .to_string(),
                            "remove the move-capability marker `^`, or declare the parameter with that marker to take ownership".to_string(),
                            Some(arg.span),
                        ));
                    }
                    _ => {}
                }
    
                self.check_write_arg_change(arg);
    
                if self.loop_depth > 0 {
                    if let Expr::Ident(name, span) = &arg.expr {
                        if let Some(info) = self.lookup(name) {
                            if matches!(info.ty, Type::Shared(_)) {
                                arg.flags.shared_auto_clone = true;
                                self.record_memory_event(crate::Sema::MemoryEvent::new(
                                    crate::Sema::MemoryEventKind::RetainRelease,
                                    *span,
                                    format!(
                                        "loop use of `{name}` auto-retains a shared reference"
                                    ),
                                ));
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
            self.check_param_view_from_requirements(&sig, &call.args);
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
                if self.unit_fact_for_type(&t).is_some() {
                    t
                } else {
                    self.resolve_type(t)
                }
            }))
        }
    
}
