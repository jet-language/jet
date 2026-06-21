use super::*;
use crate::AST::{
    AccessConvention,
    Expr, Type,
};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;

impl<'a> Checker<'a> {
    /// D-MOD2: check a call `alias.method(args)` where `alias` is an inline code module.
    /// The function was registered as `{alias}__{method}` in `self.funcs`.
    pub(crate) fn infer_code_module_call(
        &mut self,
        alias: &str,
        mangled: &str,
        alias_span: Span,
        span: Span,
        args: &mut [crate::AST::CallArg],
    ) -> Option<Type> {
        let Some(sig) = self.funcs.get(mangled).cloned() else {
            self.diags.push(Diagnostic::error(
                "E0608",
                format!("`{}` is not defined in module `{}`", &mangled[alias.len() + 2..], alias),
                "check the module body for the function you're calling".to_string(),
                "make sure the function name is spelled correctly".to_string(),
                Some(alias_span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        };
        // D-MOD2/3: a qualified `M.item` call from outside the module reaches only
        // its `pub` items — a bare private item escapes its module otherwise.
        if !self.func_pub.get(mangled).copied().unwrap_or(false) {
            let item = &mangled[alias.len() + 2..];
            self.diags.push(Diagnostic::error(
                "E0609",
                format!("`{}` is private in module `{}`", item, alias),
                "only `pub` items in an inline module are reachable from outside it".to_string(),
                format!("add `pub` before `fn {}` in module `{}`", item, alias),
                Some(alias_span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        if args.len() != sig.params.len() {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`{}` expects {} argument{}, got {}",
                    &mangled[alias.len() + 2..],
                    sig.params.len(),
                    if sig.params.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                "every argument must match a parameter".to_string(),
                format!("check the definition of `{}` in module `{}`", &mangled[alias.len() + 2..], alias),
                Some(span),
            ));
        }
        for (arg, (pconv, pty)) in args.iter_mut().zip(sig.params.iter()) {
            if matches!(pconv, AccessConvention::Read) && !pty.is_scalar() {
                self.borrow_ctx = true;
            }
            if let Some(aty) = self.infer(&mut arg.expr) {
                let arg_span = arg.expr.span();
                self.check_type_assignable(pty, &aty, arg_span);
            }
        }
        sig.return_type
    }

    pub(crate) fn infer_import_call(
        &mut self,
        mod_idx: usize,
        name: &str,
        alias_span: Span,
        span: Span,
        args: &mut [crate::AST::CallArg],
    ) -> Option<Type> {
        let Some(mods) = self.modules else {
            return None;
        };
        let target = &mods[mod_idx];
        // D-MOD4: `pub use` re-export — `thismod.Item` where Item is defined in a
        // submodule and re-exported. Redirect to the real definition.
        if let Some((real_name, real_idx)) = target.reexports.get(name) {
            let (real_name, real_idx) = (real_name.clone(), *real_idx);
            return self.infer_import_call(real_idx, &real_name, alias_span, span, args);
        }
        if target.funcs.contains_key(name) {
            let is_pub = target.func_pub.get(name).copied().unwrap_or(false);
            if !is_pub && mod_idx != self.module_idx {
                self.diags.push(private_item(name, span));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
            let sig = target.funcs.get(name).unwrap().clone();
            if args.len() != sig.params.len() {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!(
                        "`{}` expects {} argument{}, got {}",
                        name,
                        sig.params.len(),
                        if sig.params.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                    "every argument must match a parameter".to_string(),
                    format!("check the definition of `{}` in the imported file", name),
                    Some(span),
                ));
            }
            for (arg, (pconv, pty)) in args.iter_mut().zip(sig.params.iter()) {
                if matches!(pconv, AccessConvention::Read) && !pty.is_scalar() {
                    self.borrow_ctx = true;
                }
                if let Some(aty) = self.infer(&mut arg.expr) {
                    let span = arg.expr.span();
                    let reported = self.check_type_assignable(pty, &aty, span);
                    if !reported && aty != *pty {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`{}` wants {} here, but this is {}",
                                name,
                                pty.show(),
                                aty.show()
                            ),
                            "every argument must match its parameter's type".to_string(),
                            type_fix_hint(pty, &aty),
                            Some(span),
                        ));
                    }
                }
                // Cross-file calls follow the same ownership rules.
                if let Expr::Ident(n, nspan) = &arg.expr {
                    match (pconv, arg.convention) {
                        (AccessConvention::Move, AccessConvention::Move) => {
                            if !pty.is_scalar() {
                                self.mark_moved(n.clone(), *nspan);
                            }
                        }
                        (AccessConvention::Move, AccessConvention::Read) => {
                            if !pty.is_scalar() {
                                arg.flags.implicit_clone = true;
                            }
                        }
                        (AccessConvention::Mutate, AccessConvention::Read) => {
                            self.diags.push(Diagnostic::error(
                                "E0202",
                                format!(
                                    "parameter `{}` requires `{}` at the call site",
                                    n,
                                    Syntax::KW_MUTATE
                                ),
                                format!(
                                    "`{}` needs to change this value while it borrows it",
                                    name
                                ),
                                format!(
                                    "write `{} {}` when calling `{}`",
                                    Syntax::KW_MUTATE,
                                    n,
                                    name
                                ),
                                Some(*nspan),
                            ));
                        }
                        _ => {}
                    }
                }
            }
            return sig.return_type.clone();
        }
        if target.registry.contains(name) {
            let is_pub = target.type_pub.get(name).copied().unwrap_or(false);
            if !is_pub && mod_idx != self.module_idx {
                self.diags.push(private_item(name, span));
            } else {
                self.diags.push(Diagnostic::error(
                    "E0102",
                    format!("nothing named `{}` exists in this import", name),
                    "only `pub` functions and types from the other file are reachable here"
                        .to_string(),
                    "check the spelling, or mark the item `pub` in its file".to_string(),
                    Some(span),
                ));
            }
        } else {
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("nothing named `{}` exists in this import", name),
                "only `pub` functions and types from the other file are reachable here".to_string(),
                "check the spelling, or mark the item `pub` in its file".to_string(),
                Some(alias_span),
            ));
        }
        for a in args.iter_mut() {
            self.infer(&mut a.expr);
        }
        None
    }

    pub(crate) fn infer_core_field(
        &mut self,
        module: &str,
        name: &str,
        alias_span: Span,
        span: Span,
    ) -> Option<Type> {
        match (module, name) {
            ("core.math", "pi" | "e") => Some(Type::Float),
            // D-ALLOC1/D-ALLOC-C (ratified 2026-06-19): `mem.Arena`, `mem.Bump`,
            // `mem.Pool`, `mem.Fixed` — accessed as a field on the `core.mem` alias,
            // then `.new()` is called on the sentinel type to construct the allocator.
            ("core.mem", "Arena") => Some(Type::Named(Syntax::MEM_ARENA.to_string())),
            ("core.mem", "Bump") => Some(Type::Named(Syntax::MEM_BUMP.to_string())),
            ("core.mem", "Pool") => Some(Type::Named(Syntax::MEM_POOL.to_string())),
            ("core.mem", "Fixed") => Some(Type::Named(Syntax::MEM_FIXED.to_string())),
            _ => {
                self.diags.push(unknown_core_item(module, name, span));
                let _ = alias_span;
                None
            }
        }
    }

    /// S58 (E2-M13): `alias.Ptr<T>.from_addr(addr)`. Gated by `use core.mem`
    /// (E3102) and an enclosing `@unsafe` block (E3101). Returns `Ptr<T>`.
    pub(crate) fn infer_ptr_from_addr(
        &mut self,
        alias: &str,
        alias_span: Span,
        elem: &Type,
        addr: &mut Expr,
        span: Span,
    ) -> Option<Type> {
        // E3102: the discovery gate — the alias must be a `core.mem` import.
        let is_mem = self
            .core_imports
            .get(alias)
            .map(|m| m == Syntax::CORE_MEM_MODULE)
            .unwrap_or(false);
        if !is_mem {
            self.diags.push(self.e3102(alias, alias_span));
            self.infer(addr);
            return None;
        }
        // E3101: pointer construction is a low-level operation; it needs the
        // audit gate.
        if !self.in_unsafe {
            self.diags.push(e3101(Syntax::MEM_FROM_ADDR, span));
        }
        // The address is a plain Int.
        if let Some(t) = self.infer(addr) {
            if t != Type::Int {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`{}` needs an Int address, not {}", Syntax::MEM_FROM_ADDR, t.show()),
                    "a pointer is built from a numeric machine address".to_string(),
                    "pass an Int, e.g. from `mem.address_of(x)`".to_string(),
                    Some(addr.span()),
                ));
            }
        }
        Some(ptr_type(elem.clone()))
    }

    /// E3102: a `core.mem` item was named without `use core.mem`.
    pub(crate) fn e3102(&self, alias: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            "E3102",
            format!("`{}` is part of the low-level tier", Syntax::TYPE_PTR),
            format!(
                "naming `{}`, `{}`, or an allocator needs the discovery gate",
                Syntax::TYPE_PTR, Syntax::MEM_VOLATILE_READ
            ),
            format!("add `use {};` and call through `{}.…`", Syntax::CORE_MEM_MODULE, alias),
            Some(span),
        )
    }

    pub(crate) fn infer_core_call(
        &mut self,
        module: &str,
        name: &str,
        alias_span: Span,
        span: Span,
        args: &mut [crate::AST::CallArg],
    ) -> Option<Type> {
        // E2-M15 / E3301: reject OS-dependent APIs in freestanding builds.
        if self.freestanding && is_freestanding_forbidden(module) {
            let api = format!("{}.{}", module_short_name(module), name);
            let hint = freestanding_hint(module);
            self.diags.push(e3301(&api, hint, span));
            // Still infer args to avoid cascading errors.
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        // E2-M16 / E3403: a `pure fn` cannot reach a non-deterministic std call
        // (time/random). `jet eval --pure` requires every fn to be `pure`, so
        // this covers the --pure path too.
        if self.in_pure && is_nondeterministic_core(module, name) {
            let api = format!("{}.{}", module_short_name(module), name);
            self.diags.push(e3403(&api, Some(span)));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            // Return the declared type so the call site doesn't cascade.
            return core_fixed_sig(module, name).and_then(|(_, ret)| ret);
        }
        let sig = core_fixed_sig(module, name);
        match (module, name) {
            ("core.mem", "volatile_read") => {
                if !self.in_unsafe {
                    self.diags.push(e3101(Syntax::MEM_VOLATILE_READ, span));
                }
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    return None;
                }
                let arg = args.get_mut(0)?;
                let t = self.infer(&mut arg.expr)?;
                return match ptr_elem(&t) {
                    Some(elem) => Some(elem),
                    None => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs a `Ptr<T>`, not {}", Syntax::MEM_VOLATILE_READ, t.show()),
                            "a volatile read reads through a typed pointer".to_string(),
                            "build a pointer first with `mem.Ptr<T>.from_addr(addr)`".to_string(),
                            Some(arg.expr.span()),
                        ));
                        None
                    }
                };
            }
            ("core.mem", "address_of") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    return None;
                }
                // Taking an address is inert (S58): legal outside `@unsafe`.
                let arg = args.get_mut(0)?;
                self.infer(&mut arg.expr);
                let _ = alias_span;
                return Some(Type::Int);
            }
            ("core.io", "eprint") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.borrow_ctx = true;
                    if let Some(ty) = self.infer(&mut arg.expr) {
                        if !is_printable(&ty, self.registry) {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("{} can't be printed yet", ty.show()),
                                "`io.eprint` prints the same values as `print`, but writes to stderr"
                                    .to_string(),
                                "print one of its fields, or make it a printable type".to_string(),
                                Some(arg.expr.span()),
                            ));
                        }
                    }
                }
                return None;
            }
            ("core.io", "input") => {
                if args.len() > 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::String, arg);
                }
                return Some(result_ty(Type::String, io_error_ty()));
            }
            ("core.math", "abs") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Int);
                };
                let ty = self.infer(&mut arg.expr)?;
                if !matches!(ty, Type::Int | Type::Float) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`abs` needs Int or Float, not {}", ty.show()),
                        "absolute value is only defined for numbers".to_string(),
                        "pass an Int or Float".to_string(),
                        Some(arg.expr.span()),
                    ));
                    return None;
                }
                return Some(ty);
            }
            ("core.math", "min" | "max") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                    for a in args.iter_mut().skip(1) {
                        self.infer(&mut a.expr);
                    }
                    return None;
                };
                if !types_comparable(&first, self.registry) {
                    self.diags.push(Diagnostic::error(
                        "E0905",
                        format!("`{}` needs comparable values", name),
                        "min/max compare their two arguments".to_string(),
                        "use Int, Float, String, Char, Bool, or a comparable type".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                if let Some(second) = args.get_mut(1).and_then(|a| self.infer(&mut a.expr)) {
                    if second != first {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs two values of the same type", name),
                            "min/max compare like with like".to_string(),
                            type_fix_hint(&first, &second),
                            Some(args[1].expr.span()),
                        ));
                    }
                }
                return Some(first);
            }
            ("core.math", "clamp") => {
                if args.len() != 3 {
                    self.diags.push(wrong_core_arity(name, 3, args.len(), span));
                }
                let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                    for a in args.iter_mut().skip(1) {
                        self.infer(&mut a.expr);
                    }
                    return None;
                };
                if !types_comparable(&first, self.registry) {
                    self.diags.push(Diagnostic::error(
                        "E0905",
                        "`clamp` needs comparable values".to_string(),
                        "clamp compares the value with its lower and upper bounds".to_string(),
                        "use Int, Float, String, Char, Bool, or a comparable type".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                for i in 1..3 {
                    if let Some(got) = args.get_mut(i).and_then(|a| self.infer(&mut a.expr)) {
                        if got != first {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`clamp` needs all three values to have the same type"),
                                "the value and both bounds are compared together".to_string(),
                                type_fix_hint(&first, &got),
                                Some(args[i].expr.span()),
                            ));
                        }
                    }
                }
                return Some(first);
            }
            ("core.random", "pick") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Option(Box::new(Type::Int)));
                };
                let ty = self.infer(&mut arg.expr)?;
                if let Type::List(inner) = ty {
                    return Some(Type::Option(inner));
                }
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`pick` needs a list, not {}", ty.show()),
                    "random.pick chooses one item from a List".to_string(),
                    "pass a `[T]` value".to_string(),
                    Some(arg.expr.span()),
                ));
                return None;
            }
            ("core.random", "shuffle") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return None;
                };
                if arg.convention != AccessConvention::Mutate {
                    self.diags.push(Diagnostic::error(
                        "E0202",
                        "`shuffle` changes its list".to_string(),
                        "a changing argument must be passed with `mut`".to_string(),
                        "write `random.shuffle(mut xs)`".to_string(),
                        Some(arg.span),
                    ));
                }
                let ty = self.infer(&mut arg.expr)?;
                if !matches!(ty, Type::List(_)) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`shuffle` needs a list, not {}", ty.show()),
                        "random.shuffle reorders a List in place".to_string(),
                        "pass a `[T]` value".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
                return None;
            }
            ("core.tasks", "spawn") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("spawn", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let saved_esc = self.lambda_escapes;
                let saved_task = self.is_task_spawn;
                self.lambda_escapes = true;
                self.is_task_spawn = true;
                let lam_ty = self.infer(&mut args[0].expr);
                let view_return_span = match &args[0].expr {
                    Expr::Lambda(lam) => lambda_body_view_return_span(self, &lam.body),
                    expr if self.is_view_call(expr) => Some(expr.span()),
                    _ => None,
                };
                self.lambda_escapes = saved_esc;
                self.is_task_spawn = saved_task;
                // Extract the return type from the closure's function type.
                let t = match lam_ty {
                    Some(Type::Fn { params, ret }) => {
                        if !params.is_empty() {
                            self.diags.push(Diagnostic::error(
                                "E0104",
                                format!(
                                    "`spawn` needs a zero-parameter lambda, got {} parameter{}",
                                    params.len(),
                                    if params.len() == 1 { "" } else { "s" }
                                ),
                                "a task starts by calling the lambda with no arguments"
                                    .to_string(),
                                "move data into the task with `take(name)` instead of lambda parameters"
                                    .to_string(),
                                Some(args[0].expr.span()),
                            ));
                        }
                        ret.map(|r| *r)
                            .unwrap_or_else(|| Type::Named("Unit".to_string()))
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`spawn` needs a lambda, not {}", other.show()),
                            "a task starts by running a zero-parameter lambda".to_string(),
                            "write `tasks.spawn(() => work())`".to_string(),
                            Some(args[0].expr.span()),
                        ));
                        Type::Named("Unit".to_string())
                    }
                    None => Type::Named("Unit".to_string()),
                };
                if let Some(span) = view_return_span {
                    self.report_unsendable(
                        "task result",
                        &t,
                        SendabilityProblem {
                            root: None,
                            path: Vec::new(),
                            kind: SendProblemKind::ViewBorrow,
                        },
                        SendCrossing::TaskResult,
                        span,
                    );
                } else if let Some(problem) = self.sendability_problem(&t, false) {
                    self.report_unsendable(
                        "task result",
                        &t,
                        problem,
                        SendCrossing::TaskResult,
                        args[0].expr.span(),
                    );
                }
                return Some(Type::Apply {
                    name: "Task".to_string(),
                    args: vec![t],
                });
            }
            // L2501 is reserved for "whole-file read advisory" but intentionally not
            // emitted here: `fs.read` is kept as sugar (D-IO3) and firing on every call
            // site is too noisy (breaks showcase golden tests via path-specific output).
            // Revisit when the test harness can normalise paths in exact comparisons.
            ("core.fs", "read") => {}
            ("core.tasks", "channel") => {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity("channel", 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let t = match &self.expected_type {
                    Some(Type::Apply { name, args }) if name == "Channel" && args.len() == 1 => {
                        args[0].clone()
                    }
                    _ => {
                        self.diags.push(Diagnostic::error(
                            "E0904",
                            "`tasks.channel` needs a type annotation to infer the element type"
                                .to_string(),
                            "the element type `T` can't be guessed without a type annotation"
                                .to_string(),
                            "annotate the binding: `val ch: Channel<T> = tasks.channel();`"
                                .to_string(),
                            Some(span),
                        ));
                        return None;
                    }
                };
                return Some(Type::Apply {
                    name: "Channel".to_string(),
                    args: vec![t],
                });
            }
            // E2-M10: jet.http.serve(addr, handler) — blocking accept loop.
            // handler: fn(HttpRequest) -> HttpResponse (lambda or fn reference).
            ("jet.http", "serve") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity("serve", 2, args.len(), span));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                self.expect_core_arg("serve", 0, &Type::String, &mut args[0]);
                // Check the handler arg — accept any callable (lambda or fn pointer).
                let handler_ty = self.infer(&mut args[1].expr);
                match &handler_ty {
                    Some(Type::Fn { .. }) => {}
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`http.serve` handler must be a function, not {}", other.show()),
                            "the handler is called with each incoming `HttpRequest`".to_string(),
                            "write a lambda: `(req) => HttpResponse { status: \"200 OK\", body: req.body, headers: [:] }`".to_string(),
                            Some(args[1].expr.span()),
                        ));
                    }
                    None => {}
                }
                return None; // serve runs forever; no meaningful return type
            }
            // D-DEFER1 option B: scope.guard(() => { … }) → ScopeGuard
            // The argument must be a zero-parameter lambda. LIFO drop order is
            // guaranteed by Rust's reverse-declaration semantics.
            ("core.scope", "guard") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity("guard", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let lam_ty = self.infer(&mut args[0].expr);
                match &lam_ty {
                    Some(Type::Fn { params, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(Diagnostic::error(
                                "E0104",
                                format!(
                                    "`scope.guard` needs a zero-parameter lambda, got {} parameter{}",
                                    params.len(),
                                    if params.len() == 1 { "" } else { "s" }
                                ),
                                "the guard body takes no arguments — it captures what it needs via closure".to_string(),
                                "write `scope.guard(() => { cleanup_code })` with no parameters".to_string(),
                                Some(args[0].expr.span()),
                            ));
                        }
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`scope.guard` needs a lambda, not {}", other.show()),
                            "a scope guard runs a cleanup lambda when the binding goes out of scope".to_string(),
                            "write `scope.guard(() => { cleanup_code })`".to_string(),
                            Some(args[0].expr.span()),
                        ));
                    }
                    None => {}
                }
                return Some(Type::Named("ScopeGuard".to_string()));
            }
            _ => {}
        }

        let Some((params, ret)) = sig else {
            self.diags.push(unknown_core_item(module, name, span));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            let _ = alias_span;
            return None;
        };
        if args.len() != params.len() {
            self.diags
                .push(wrong_core_arity(name, params.len(), args.len(), span));
        }
        for (i, ((conv, param_ty), arg)) in params.iter().zip(args.iter_mut()).enumerate() {
            if *conv == AccessConvention::Mutate && arg.convention != AccessConvention::Mutate {
                self.diags.push(Diagnostic::error(
                    "E0202",
                    format!("argument {} to `{}` must be passed with `mut`", i + 1, name),
                    "this standard library call changes that value".to_string(),
                    format!("write `{} value` for this argument", Syntax::KW_MUTATE),
                    Some(arg.span),
                ));
            }
            self.expect_core_arg(name, i, param_ty, arg);
        }
        for arg in args.iter_mut().skip(params.len()) {
            self.infer(&mut arg.expr);
        }
        ret
    }

    pub(crate) fn check_core_json_lit(
        &mut self,
        variant: &str,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let json = json_ty();
        let expected = match variant {
            "Null" => Vec::new(),
            "Boolean" => vec![Type::Bool],
            "Number" => vec![Type::Float],
            "Text" => vec![Type::String],
            "Array" => vec![Type::List(Box::new(json.clone()))],
            "Object" => vec![Type::Map {
                key: Box::new(Type::String),
                value: Box::new(json.clone()),
            }],
            _ => {
                let candidates = ["Null", "Boolean", "Number", "Text", "Array", "Object"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>();
                let mut fix = "check the variant name".to_string();
                if let Some(s) = suggest_field(variant, &candidates) {
                    fix = format!("did you mean `{}`?", s);
                }
                self.diags.push(Diagnostic::error(
                    "E0304",
                    format!("`{}` has no variant `{}`", Syntax::TYPE_JSON, variant),
                    "core.json exposes the dynamic JSON variants from the M10 API".to_string(),
                    fix,
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(json);
            }
        };
        if args.len() != expected.len() {
            self.diags.push(Diagnostic::error(
                "E0306",
                format!(
                    "`{}.{}` expects {} value{}, got {}",
                    Syntax::TYPE_JSON,
                    variant,
                    expected.len(),
                    if expected.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                "each JSON variant has the payload listed in the M10 std API".to_string(),
                "check the variant payload".to_string(),
                Some(span),
            ));
        }
        for (i, arg) in args.iter_mut().enumerate() {
            if let Some(want) = expected.get(i) {
                self.expect_core_arg(variant, i, want, arg);
            } else {
                self.infer(&mut arg.expr);
            }
        }
        Some(json)
    }

    pub(crate) fn expect_core_arg(
        &mut self,
        call_name: &str,
        idx: usize,
        param_ty: &Type,
        arg: &mut crate::AST::CallArg,
    ) {
        if matches!(arg.convention, AccessConvention::Move)
            && !matches!(param_ty, Type::Named(n) if n == "Unit")
        {
            self.diags.push(Diagnostic::error(
                "E0203",
                format!("`{}` passed to a parameter that does not consume", Syntax::KW_MOVE),
                "standard library functions in M10 read their ordinary arguments unless documented otherwise"
                    .to_string(),
                format!("remove `{}` here", Syntax::KW_MOVE),
                Some(arg.span),
            ));
        }
        if matches!(param_ty, Type::String | Type::List(_) | Type::Map { .. }) {
            self.borrow_ctx = true;
        }
        let got = self.infer(&mut arg.expr);
        if let Some(got) = got {
            if is_u8_ty(param_ty) && got == Type::Int {
                if let Expr::Int(n, span) = arg.expr {
                    if !(0..=255).contains(&n) {
                        self.diags.push(u8_range_error(span));
                    }
                }
                return;
            }
            let reported = self.check_type_assignable(param_ty, &got, arg.expr.span());
            if !reported && got != *param_ty {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "`{}` wants {} for argument {}, but this is {}",
                        call_name,
                        param_ty.show(),
                        idx + 1,
                        got.show()
                    ),
                    "every argument must match its parameter's type".to_string(),
                    type_fix_hint(param_ty, &got),
                    Some(arg.expr.span()),
                ));
            }
        }
        // A std constructor that stores a non-scalar payload (e.g. `JSON.Text`
        // owns its `String`) consumes the argument. When the value is read from
        // a borrowed binding (a `view` parameter), moving it out would not
        // compile — insert a clone, exactly as a consuming `fn` call does (B1).
        if matches!(arg.convention, AccessConvention::Read)
            && matches!(param_ty, Type::String | Type::List(_) | Type::Map { .. })
        {
            if let Expr::Ident(name, ispan) = &arg.expr {
                let name = name.clone();
                let ispan = *ispan;
                if self.is_borrowed_binding(&name) {
                    arg.flags.implicit_clone = true;
                    // D-L0201: only warn when the value is dead after
                    // this call (a wasteful clone).
                    if !self.is_name_live_after(&name) {
                        self.diags.push(Diagnostic::lint(
                            "L0201",
                            format!(
                                "implicit clone of `{}`; this value is borrowed, so it is copied into the JSON value",
                                name
                            ),
                            format!("`{}.{}` stores its own copy of this value", Syntax::TYPE_JSON, call_name),
                            format!("write `{} .clone()` to copy explicitly and silence this warning", name),
                            Some(ispan),
                        ));
                    }
                }
            }
        }
    }

}

pub(crate) fn unit_ty() -> Type {
    Type::Named("Unit".to_string())
}

pub(crate) fn u8_ty() -> Type {
    Type::Named("U8".to_string())
}

pub(crate) fn is_u8_ty(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if n == "U8")
}

pub(crate) fn json_ty() -> Type {
    Type::Named(Syntax::TYPE_JSON.to_string())
}

pub(crate) fn json_error_ty() -> Type {
    Type::Named(Syntax::TYPE_JSON_ERROR.to_string())
}

pub(crate) fn is_json_type_name(name: &str) -> bool {
    name == Syntax::TYPE_JSON || name == "Json"
}

pub(crate) fn is_json_error_type_name(name: &str) -> bool {
    name == Syntax::TYPE_JSON_ERROR || name == "JsonError"
}

pub(crate) fn is_io_error_type_name(name: &str) -> bool {
    name == Syntax::TYPE_IO_ERROR || name == "IoError"
}

pub(crate) fn is_utf8_error_type_name(name: &str) -> bool {
    name == Syntax::TYPE_UTF8_ERROR || name == "Utf8Error"
}

pub(crate) fn core_type_known(name: &str) -> bool {
    matches!(
        name,
        "Unit" | "U8" | "Error" | "ProcessResult" | "Stopwatch" | "Closed"
        | "FileReader" | "FileWriter" | "FileLines"
        // E2-M10: networking opaque types.
        | "TcpListener" | "TcpStream" | "HttpRequest" | "HttpResponse"
        // D-ALLOC1/D-ALLOC-C (ratified 2026-06-19): allocator opaque types.
        | "Arena" | "Bump" | "Pool" | "Fixed"
    ) || is_json_type_name(name)
        || is_json_error_type_name(name)
        || is_io_error_type_name(name)
        || is_utf8_error_type_name(name)
}

pub(crate) fn core_struct_field(type_name: &str, field: &str) -> Option<Type> {
    if is_json_error_type_name(type_name) {
        return match field {
            "line" => Some(Type::Int),
            "message" => Some(Type::String),
            _ => None,
        };
    }
    if is_utf8_error_type_name(type_name) {
        return match field {
            "message" => Some(Type::String),
            _ => None,
        };
    }
    match (type_name, field) {
        ("ProcessResult", "code") => Some(Type::Int),
        ("ProcessResult", "output" | "errors") => Some(Type::String),
        // E2-M10: HTTP request fields exposed to handlers.
        ("HttpRequest", "method" | "path" | "body") => Some(Type::String),
        ("HttpRequest", "headers") => Some(Type::Map {
            key: Box::new(Type::String),
            value: Box::new(Type::String),
        }),
        // E2-M10: HTTP response fields.
        ("HttpResponse", "status" | "body") => Some(Type::String),
        ("HttpResponse", "headers") => Some(Type::Map {
            key: Box::new(Type::String),
            value: Box::new(Type::String),
        }),
        _ => None,
    }
}

pub(crate) fn core_json_pattern_types(variant: &str) -> Option<Vec<Type>> {
    let json = json_ty();
    match variant {
        "Null" => Some(Vec::new()),
        "Boolean" => Some(vec![Type::Bool]),
        "Number" => Some(vec![Type::Float]),
        "Text" => Some(vec![Type::String]),
        "Array" => Some(vec![Type::List(Box::new(json.clone()))]),
        "Object" => Some(vec![Type::Map {
            key: Box::new(Type::String),
            value: Box::new(json),
        }]),
        _ => None,
    }
}

/// E2-M7: type-check a method call on a FileReader or FileWriter handle (D-IO2).
/// Returns `Some(return_type)` when the method is valid, or emits E2501 and
/// returns `None` for an invalid method / wrong-direction call.
pub(crate) fn file_handle_method_return(
    handle_ty: &str,
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let io = io_error_ty();
    let unit = unit_ty();
    match handle_ty {
        "FileReader" => match method {
            // `.lines()` — returns the handle as a streaming source for `loop … in`.
            // We encode the return as `Named("FileLines")` so the loop body knows
            // the element type is `String`.
            "lines" if n_args == 0 => Some(Some(Type::Named("FileLines".to_string()))),
            // `.read_line()` — returns one line or `None` at EOF.
            "read_line" if n_args == 0 => {
                Some(Some(result_ty(Type::Option(Box::new(Type::String)), io)))
            }
            // Wrong direction: writing to a reader.
            "write_line" | "flush" => {
                diags.push(Diagnostic::error(
                    "E2501",
                    format!("`{}` is not available on a read-only file handle", method),
                    "`files.open` returns a read-only handle; it can only read lines or bytes"
                        .to_string(),
                    "use `files.create` or `files.append` to get a writable handle".to_string(),
                    Some(span),
                ));
                Some(None)
            }
            _ => None,
        },
        "FileWriter" => match method {
            // `.write_line(text)` — writes a line followed by a newline.
            "write_line" if n_args == 1 => {
                Some(Some(result_ty(unit.clone(), io.clone())))
            }
            // `.flush()` — ensure buffered bytes reach disk.
            "flush" if n_args == 0 => Some(Some(result_ty(unit, io))),
            // Wrong direction: reading from a writer.
            "lines" | "read_line" => {
                diags.push(Diagnostic::error(
                    "E2501",
                    format!("`{}` is not available on a write-only file handle", method),
                    "`files.create` returns a write-only handle; it can only write lines"
                        .to_string(),
                    "use `files.open` to get a readable handle".to_string(),
                    Some(span),
                ));
                Some(None)
            }
            _ => None,
        },
        _ => None,
    }
}

/// E2-M10: field definitions for compiler-known constructable struct types.
/// Returns `Some(fields)` when the named type is a prelude struct users can construct.
pub(crate) fn core_constructable_fields(type_name: &str) -> Option<Vec<(String, Type)>> {
    let str_ty = Type::String;
    let map_ty = Type::Map { key: Box::new(Type::String), value: Box::new(Type::String) };
    match type_name {
        "HttpResponse" => Some(vec![
            ("status".to_string(), str_ty.clone()),
            ("body".to_string(), str_ty),
            ("headers".to_string(), map_ty),
        ]),
        "HttpRequest" => Some(vec![
            ("method".to_string(), str_ty.clone()),
            ("path".to_string(), str_ty.clone()),
            ("body".to_string(), str_ty),
            ("headers".to_string(), map_ty),
        ]),
        _ => None,
    }
}

/// E2-M10: type-check a method call on a networking opaque type.
/// Returns `Some(return_type)` when the method is valid.
pub(crate) fn net_method_return(
    type_name: &str,
    method: &str,
    _n_args: usize,
    _span: Span,
    _diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let str_ty = Type::String;
    let unit = unit_ty();
    let err = str_ty.clone();
    match (type_name, method) {
        // HttpResponse field accessors (via method-style read, auto-generated by codegen).
        ("HttpResponse", "status") => Some(Some(str_ty.clone())),
        ("HttpResponse", "body") => Some(Some(str_ty.clone())),
        ("HttpResponse", "header") => Some(Some(Type::Option(Box::new(str_ty.clone())))),
        // HttpRequest field accessors.
        ("HttpRequest", "method") => Some(Some(str_ty.clone())),
        ("HttpRequest", "path") => Some(Some(str_ty.clone())),
        ("HttpRequest", "body") => Some(Some(str_ty.clone())),
        ("HttpRequest", "header") => Some(Some(Type::Option(Box::new(str_ty.clone())))),
        // TcpListener methods.
        ("TcpListener", "accept") => Some(Some(result_ty(
            Type::Named("TcpStream".to_string()),
            err.clone(),
        ))),
        ("TcpListener", "local_addr") => Some(Some(str_ty.clone())),
        // TcpStream methods.
        ("TcpStream", "read") => Some(Some(result_ty(str_ty.clone(), err.clone()))),
        ("TcpStream", "write") => Some(Some(result_ty(unit.clone(), err.clone()))),
        ("TcpStream", "peer_addr") => Some(Some(str_ty.clone())),
        ("TcpStream", "local_addr") => Some(Some(str_ty.clone())),
        ("TcpStream", "close") => Some(Some(unit)),
        // D-REGEX1: a regex `Match`. `group(0)` is the whole match; `group(n)` is
        // capture group n, `none` if the group did not participate.
        ("Match", "group") => Some(Some(Type::Option(Box::new(str_ty.clone())))),
        _ => None,
    }
}

/// D-ALLOC1/D-ALLOC-C/D-ALLOC-D (ratified 2026-06-19): method calls on the four
/// allocator opaque types (Arena, Bump, Pool, Fixed).
/// Returns `Some(Some(T))` for a valid method with return type T, `Some(None)` for
/// a void method, `None` if the type_name is not an allocator type.
/// D-ALLOC1/D-ALLOC2: is `ty` one of the four allocator handle types?
pub(crate) fn is_allocator_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if matches!(n.as_str(), "Arena" | "Bump" | "Pool" | "Fixed"))
}

pub(crate) fn alloc_method_return(
    type_name: &str,
    method: &str,
    args: &[crate::AST::CallArg],
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    use Syntax::{MEM_ARENA, MEM_BUMP, MEM_POOL, MEM_FIXED};
    if !matches!(type_name, "Arena" | "Bump" | "Pool" | "Fixed") {
        return None;
    }
    let unit = unit_ty();
    match method {
        // D-ALLOC1: `new()` is a static constructor — handled via `infer_core_field`
        // returning a Named sentinel, then `.new()` dispatched here as an instance method
        // on the sentinel. Return the same Named type (the allocator handle).
        "new" => {
            // Optional capacity/slots/size arg.
            if args.len() > 1 {
                diags.push(Diagnostic::error(
                    "E0103",
                    format!("`{}.new` takes at most one optional argument", type_name),
                    "the only optional argument is `capacity:` / `slots:` / `size:`".to_string(),
                    format!("write `mem.{}.new()` or `mem.{}.new(capacity: N)`", type_name, type_name),
                    Some(span),
                ));
            }
            Some(Some(Type::Named(type_name.to_string())))
        }
        // D-ALLOC1: `alloc(value)` — allocates a value into the arena.
        // Returns the value's type; we infer it from the argument.
        "alloc" => {
            if args.len() != 1 {
                diags.push(Diagnostic::error(
                    "E0103",
                    format!("`{}.alloc` takes exactly one argument", type_name),
                    "pass the value you want to store in the allocator".to_string(),
                    format!("write `arena.alloc(my_value)`"),
                    Some(span),
                ));
                return Some(None);
            }
            // Return type is inferred from argument — caller handles inference.
            // We return a sentinel; actual type inference is done in the caller.
            Some(Some(Type::Named("__alloc_infer__".to_string())))
        }
        // D-ALLOC-D: `reset()` — keeps the backing buffer, marks all allocations invalid.
        "reset" => {
            if !args.is_empty() {
                diags.push(Diagnostic::error(
                    "E0103",
                    format!("`{}.reset` takes no arguments", type_name),
                    "reset clears all allocations, keeping the backing buffer".to_string(),
                    "write `arena.reset()`".to_string(),
                    Some(span),
                ));
            }
            Some(Some(unit))
        }
        // D-ALLOC-D: `free()` — returns the backing memory to the OS.
        "free" => {
            if !args.is_empty() {
                diags.push(Diagnostic::error(
                    "E0103",
                    format!("`{}.free` takes no arguments", type_name),
                    "free releases the backing memory to the OS".to_string(),
                    "write `arena.free()`".to_string(),
                    Some(span),
                ));
            }
            Some(Some(unit))
        }
        _ => {
            diags.push(Diagnostic::error(
                "E0102",
                format!("`{}` has no method `{}`", type_name, method),
                format!(
                    "`{}` supports: `new`, `alloc`, `reset`, `free`",
                    type_name
                ),
                format!("check the method name — valid methods are `alloc`, `reset`, `free`"),
                Some(span),
            ));
            None
        }
    }
}

/// D-ALLOC-D (ratified 2026-06-19): E3104 — use of an allocator value after it was
/// freed or reset.
pub(crate) fn e3104(alloc_name: &str, method: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3104",
        format!(
            "`{}` was already {}; this value lives in `{}` which is gone",
            alloc_name,
            method,
            alloc_name
        ),
        format!(
            "calling `{}.{}` invalidated all values allocated in `{}`",
            alloc_name, method, alloc_name
        ),
        format!(
            "move the `alloc` call before the `{}`, or create a new allocator",
            method
        ),
        Some(span),
    )
}

pub(crate) fn io_error_ty() -> Type {
    Type::Named(Syntax::TYPE_IO_ERROR.to_string())
}

pub(crate) fn result_ty(ok: Type, err: Type) -> Type {
    Type::Result {
        ok: Box::new(ok),
        err: Box::new(err),
    }
}

/// S58 (E2-M13): `Ptr<T>`.
pub(crate) fn ptr_type(elem: Type) -> Type {
    Type::Apply {
        name: Syntax::TYPE_PTR.to_string(),
        args: vec![elem],
    }
}

/// S58 (E2-M13): the element type of a `Ptr<T>`, if `t` is one.
pub(crate) fn ptr_elem(t: &Type) -> Option<Type> {
    match t {
        Type::Apply { name, args } if name == Syntax::TYPE_PTR && args.len() == 1 => {
            Some(args[0].clone())
        }
        _ => None,
    }
}

/// E3101: a low-level memory operation used outside an `#Unsafe` block.
pub(crate) fn e3101(op: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3101",
        format!("`{}` can only run inside an `#Unsafe` block", op),
        "this operation can violate memory safety, so it must sit in an audited region"
            .to_string(),
        format!(
            "wrap it: #{}(\"why this is safe\") #{} {{ … }}",
            Syntax::ATTR_AUDIT,
            Syntax::KW_UNSAFE
        ),
        Some(span),
    )
}

pub(crate) fn core_fixed_sig(
    module: &str,
    name: &str,
) -> Option<(Vec<(AccessConvention, Type)>, Option<Type>)> {
    let read = AccessConvention::Read;
    let string = Type::String;
    let int = Type::Int;
    let float = Type::Float;
    let bool_ = Type::Bool;
    let unit = unit_ty();
    let io = io_error_ty();
    let json = json_ty();
    let list_string = Type::List(Box::new(Type::String));
    let list_u8 = Type::List(Box::new(u8_ty()));
    let io_unit = result_ty(unit.clone(), io.clone());
    match (module, name) {
        ("core.fs", "read") => Some((vec![(read, string.clone())], Some(result_ty(string, io)))),
        ("core.fs", "read_bytes") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8, io_error_ty())),
        )),
        ("core.fs", "write" | "append") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(io_unit),
        )),
        ("core.fs", "exists" | "is_dir") => Some((vec![(read, Type::String)], Some(bool_))),
        ("core.fs", "remove" | "create_dir") => Some((
            vec![(read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.fs", "list_dir") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_string, io_error_ty())),
        )),
        ("core.fs", "copy" | "rename") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.io", "args") => Some((vec![], Some(Type::List(Box::new(Type::String))))),
        ("core.io", "read_all_input") => {
            Some((vec![], Some(result_ty(Type::String, io_error_ty()))))
        }
        ("core.env", "get") => Some((
            vec![(read, Type::String)],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.env", "set") => Some((vec![(read, Type::String), (read, Type::String)], None)),
        ("core.env", "current_dir") => Some((vec![], Some(result_ty(Type::String, io_error_ty())))),
        ("core.env", "home_dir") => Some((vec![], Some(Type::Option(Box::new(Type::String))))),
        ("core.process", "exit") => Some((vec![(read, int)], None)),
        ("core.process", "run") => Some((
            vec![(read, Type::List(Box::new(Type::String)))],
            Some(result_ty(
                Type::Named("ProcessResult".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.math", "sqrt" | "floor" | "ceil") => Some((vec![(read, float.clone())], Some(float))),
        ("core.math", "pow") => Some((
            vec![(read, Type::Float), (read, Type::Float)],
            Some(Type::Float),
        )),
        ("core.math", "round") => Some((vec![(read, Type::Float)], Some(Type::Int))),
        ("core.random", "int") => {
            Some((vec![(read, Type::Int), (read, Type::Int)], Some(Type::Int)))
        }
        ("core.random", "float") => Some((vec![], Some(Type::Float))),
        ("core.random", "seed") => Some((vec![(read, Type::Int)], None)),
        ("core.time", "now") => Some((vec![], Some(Type::Int))),
        ("core.time", "sleep") => Some((vec![(read, Type::Int)], None)),
        ("core.time", "start") => Some((vec![], Some(Type::Named("Stopwatch".to_string())))),
        ("core.json", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("core.json", "render" | "render_pretty") => Some((vec![(read, json)], Some(Type::String))),
        // E2-M7: streaming file handles (D-IO2, files.open / files.create).
        ("core.files", "open" | "append") => Some((
            vec![(read, string.clone())],
            Some(result_ty(Type::Named("FileReader".to_string()), io.clone())),
        )),
        ("core.files", "create") => Some((
            vec![(read, string.clone())],
            Some(result_ty(Type::Named("FileWriter".to_string()), io.clone())),
        )),
        // E2-M7: std.path helpers (D-IO1).
        ("core.path", "join") => Some((
            vec![(read, string.clone()), (read, string.clone())],
            Some(string),
        )),
        ("core.path", "parent" | "extension" | "normalize") => Some((
            vec![(read, Type::String)],
            Some(Type::String),
        )),
        // E2-M9: first-party ring packages.
        // jet.csv: parse CSV text into a list of rows (each row is a list of fields).
        ("jet.csv", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::List(Box::new(Type::List(Box::new(Type::String)))),
                Type::String,
            )),
        )),
        ("jet.csv", "render") => Some((
            vec![(read, Type::List(Box::new(Type::List(Box::new(Type::String)))))],
            Some(Type::String),
        )),
        // jet.toml: simplified flat key-value parsing.
        ("jet.toml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Map { key: Box::new(Type::String), value: Box::new(Type::String) },
                Type::String,
            )),
        )),
        ("jet.toml", "render") => Some((
            vec![(read, Type::Map { key: Box::new(Type::String), value: Box::new(Type::String) })],
            Some(Type::String),
        )),
        // jet.yaml: simplified flat key-value parsing.
        ("jet.yaml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Map { key: Box::new(Type::String), value: Box::new(Type::String) },
                Type::String,
            )),
        )),
        ("jet.yaml", "render") => Some((
            vec![(read, Type::Map { key: Box::new(Type::String), value: Box::new(Type::String) })],
            Some(Type::String),
        )),
        // jet.log: structured JSON logging to stderr (E2-M12, D-OBS3).
        ("jet.log", "info" | "warn" | "error" | "debug") => Some((vec![(read, string)], None)),
        ("jet.log", "set_level") => Some((vec![(read, Type::String)], None)),
        // D-OBS3: set OTel trace_id for all subsequent log entries on this thread.
        ("jet.log", "set_trace_id") => Some((vec![(read, Type::String)], None)),
        // jet.json: first-party JSON with coercion surfacing (D-JSON1, D-JSON3).
        // `decode` is the lenient variant: coerces string→number/bool, emits one log
        // line per coercion (D-JSON3=B), and returns the value plain.
        ("jet.json", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("jet.json", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("jet.json", "render" | "render_pretty") => {
            Some((vec![(read, json)], Some(Type::String)))
        }
        // jet.time: extended time utilities.
        ("jet.time", "now") => Some((vec![], Some(Type::Int))),
        ("jet.time", "format") => Some((
            vec![(read, Type::Int), (read, Type::String)],
            Some(Type::String),
        )),
        // jet.crypto: vetted hash functions (D-LR3).
        ("jet.crypto", "sha256") => Some((
            vec![(read, Type::String)],
            Some(Type::String),
        )),
        ("jet.crypto", "sha256_bytes") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::String),
        )),
        // E2-M10: core.net — blocking TCP/UDP sockets (std::net, zero external deps).
        ("core.net", "tcp_listen") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("TcpListener".to_string()), Type::String)),
        )),
        ("core.net", "tcp_accept") => Some((
            vec![(AccessConvention::Read, Type::Named("TcpListener".to_string()))],
            Some(result_ty(Type::Named("TcpStream".to_string()), Type::String)),
        )),
        ("core.net", "tcp_connect") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("TcpStream".to_string()), Type::String)),
        )),
        ("core.net", "tcp_read") => Some((
            vec![(AccessConvention::Mutate, Type::Named("TcpStream".to_string()))],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.net", "tcp_write") => Some((
            vec![
                (AccessConvention::Mutate, Type::Named("TcpStream".to_string())),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::String)),
        )),
        ("core.net", "tcp_local_addr" | "tcp_peer_addr") => Some((
            vec![(read, Type::Named("TcpStream".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "set_timeout") => Some((
            vec![
                (AccessConvention::Mutate, Type::Named("TcpStream".to_string())),
                (read, Type::Int),
            ],
            None,
        )),
        // Convenience: send a complete HTTP/1.1 response and close the stream.
        ("core.net", "tcp_reply") => Some((
            vec![
                (AccessConvention::Move, Type::Named("TcpStream".to_string())),
                (read, Type::String),
                (read, Type::String),
            ],
            None,
        )),
        // E2-M10: jet.http — HTTP client/server over blocking I/O.
        // GET / HEAD / DELETE requests (no body sent).
        ("jet.http", "get") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("HttpResponse".to_string()), Type::String)),
        )),
        // POST / PUT / PATCH requests (body sent).
        ("jet.http", "post") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(Type::Named("HttpResponse".to_string()), Type::String)),
        )),
        // serve blocks until the listener is closed; handler is called per request.
        // The handler type is resolved at the call site (lambda / fn pointer).
        ("jet.http", "serve") => None, // special-cased in check_core_call
        // D-REGEX1: jet.regex — linear-time regex on the `regex` crate. Every call
        // returns a Result; the `Err` is a bad-pattern message at the boundary.
        ("jet.regex", "is_match") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(Type::Bool, Type::String)),
        )),
        // First match anywhere: `Match?` (none when nothing matches).
        ("jet.regex", "match") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(
                Type::Option(Box::new(Type::Named("Match".to_string()))),
                Type::String,
            )),
        )),
        // First matched substring, or none.
        ("jet.regex", "find") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(
                Type::Option(Box::new(Type::String)),
                Type::String,
            )),
        )),
        ("jet.regex", "find_all" | "split") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(
                Type::List(Box::new(Type::String)),
                Type::String,
            )),
        )),
        ("jet.regex", "replace" | "replace_all") => Some((
            vec![(read, Type::String), (read, Type::String), (read, Type::String)],
            Some(result_ty(Type::String, Type::String)),
        )),
        _ => None,
    }
}

pub(crate) fn core_module_items(module: &str) -> Vec<String> {
    let items: &[&str] = match module {
        "core.fs" => &[
            "read",
            "read_bytes",
            "write",
            "append",
            "exists",
            "remove",
            "list_dir",
            "create_dir",
            "is_dir",
            "copy",
            "rename",
        ],
        "core.io" => &["args", "input", "read_all_input", "eprint"],
        "core.env" => &["get", "set", "current_dir", "home_dir"],
        "core.process" => &["exit", "run"],
        "core.math" => &[
            "sqrt", "pow", "abs", "min", "max", "floor", "ceil", "round", "pi", "e", "clamp",
        ],
        "core.random" => &["int", "float", "pick", "shuffle", "seed"],
        "core.time" => &["now", "sleep", "start"],
        "core.json" => &["parse", "render", "render_pretty"],
        "core.mem" => &["Ptr", "from_addr", "volatile_read", "address_of",
                        "Arena", "Bump", "Pool", "Fixed"],
        // D-ALLOC-C (ratified 2026-06-19): wider allocator API bucket.
        "core.mem.alloc" => &["Arena", "Bump", "Pool", "Fixed"],
        "core.tasks" => &["spawn", "channel"],
        "core.files" => &["open", "create", "append"],
        "core.path" => &["join", "parent", "extension", "normalize"],
        // D-DEFER1 option B: scope-exit guard.
        "core.scope" => &["guard"],
        "core" => &[],
        // E2-M9: ring packages.
        "jet.csv" => &["parse", "render"],
        "jet.toml" => &["parse", "render"],
        "jet.yaml" => &["parse", "render"],
        "jet.log" => &["info", "warn", "error", "debug", "set_level", "set_trace_id"],
        "jet.json" => &["parse", "decode", "render", "render_pretty"],
        "jet.time" => &["now", "format"],
        "jet.crypto" => &["sha256", "sha256_bytes"],
        // E2-M10: networking modules.
        "core.net" => &[
            "tcp_listen", "tcp_accept", "tcp_connect",
            "tcp_read", "tcp_write", "tcp_local_addr", "tcp_peer_addr", "set_timeout",
            "tcp_reply",
        ],
        "jet.http" => &["get", "post", "serve"],
        // D-REGEX1: linear-time regex ring package.
        "jet.regex" => &[
            "is_match", "match", "find", "find_all", "replace", "replace_all", "split",
        ],
        _ => &[],
    };
    items.iter().map(|s| s.to_string()).collect()
}

/// E2-M15: modules that require an OS and are forbidden in `--freestanding` builds.
pub(crate) fn is_freestanding_forbidden(module: &str) -> bool {
    matches!(
        module,
        "core.fs" | "core.files" | "core.io" | "core.net" | "core.tasks"
            | "core.process" | "core.time" | "jet.http" | "jet.log" | "jet.time"
    )
}

/// Return a short display name for the module alias (the part after the dot).
pub(crate) fn module_short_name(module: &str) -> &str {
    module.split('.').last().unwrap_or(module)
}

/// Fix hint for E3301 depending on the forbidden module.
pub(crate) fn freestanding_hint(module: &str) -> &'static str {
    match module {
        "core.fs" | "core.files" => {
            "Embed the data at compile time with `@embed(\"file\")`, or build without `--freestanding`."
        }
        "core.net" | "jet.http" => {
            "Freestanding targets have no network stack. Build without `--freestanding`, or use a bare-metal driver."
        }
        "core.tasks" => {
            "OS threads are not available without an OS. Use cooperative or interrupt-driven concurrency."
        }
        "core.io" => {
            "Standard I/O requires an OS. Use a platform-specific write routine or build without `--freestanding`."
        }
        "core.process" | "core.time" | "jet.time" => {
            "System calls are not available in a freestanding build. Build without `--freestanding`."
        }
        "jet.log" => {
            "The log module writes to stderr (an OS resource). Use a bare-metal write routine or build without `--freestanding`."
        }
        _ => "Build without `--freestanding`, or replace this call with a core-level alternative.",
    }
}

pub(crate) fn unknown_core_item(module: &str, name: &str, span: Span) -> Diagnostic {
    let items = core_module_items(module);
    let mut fix = if items.is_empty() {
        "import a specific core module, like `import core.fs as fs;`".to_string()
    } else {
        format!("use one of: {}", items.join(", "))
    };
    if let Some(s) = suggest_field(name, &items) {
        fix = format!("did you mean `{}`?", s);
    }
    Diagnostic::error(
        "E1004",
        format!("`{}` has no item `{}`", module, name),
        "standard library modules expose only their documented M10 items".to_string(),
        fix,
        Some(span),
    )
}

pub(crate) fn wrong_core_arity(name: &str, want: usize, got: usize, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0104",
        format!(
            "`{}` expects {} argument{}, got {}",
            name,
            want,
            if want == 1 { "" } else { "s" },
            got
        ),
        "every argument must match a standard library function parameter".to_string(),
        format!("check the call to `{}`", name),
        Some(span),
    )
}

pub(crate) fn u8_range_error(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1003",
        "a U8 holds 0..255".to_string(),
        "binary APIs use one byte per value".to_string(),
        "use a number from 0 through 255".to_string(),
        Some(span),
    )
}

