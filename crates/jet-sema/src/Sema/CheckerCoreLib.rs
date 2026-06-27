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
            // D-SG9: a fixed-width literal argument adopts the parameter's width.
            let saved = self.expected_type.clone();
            self.expected_type = Some(pty.clone());
            let aty = self.infer(&mut arg.expr);
            self.expected_type = saved;
            if let Some(aty) = aty {
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
                // D-SG9: a fixed-width literal argument adopts the parameter's width.
                let saved = self.expected_type.clone();
                self.expected_type = Some(pty.clone());
                let aty = self.infer(&mut arg.expr);
                self.expected_type = saved;
                if let Some(aty) = aty {
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
                        (AccessConvention::Write, AccessConvention::Read) => {
                            self.diags.push(Diagnostic::error(
                                "E0202",
                                format!(
                                    "parameter `{}` requires `{}` at the call site",
                                    n,
                                    Syntax::SIGIL_MUTATE
                                ),
                                format!(
                                    "`{}` needs to edit (`~`) this value; passing it without `{}` grants only read access",
                                    name,
                                    Syntax::SIGIL_MUTATE
                                ),
                                format!(
                                    "write `{}{}` when calling `{}`",
                                    Syntax::SIGIL_MUTATE,
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

    /// D-SERDE: a value type the `#[Codable]`/`#[Encode]` derive (or a blanket impl)
    /// can serialize. Primitives, the dynamic `Json` tree, and lists/options/maps of
    /// encodables qualify; a user type must derive `Encode`.
    fn is_encodable(&self, t: &Type) -> bool {
        match t {
            Type::Int | Type::Float | Type::Bool | Type::String | Type::Char
            | Type::IntN { .. } | Type::Float32 => true,
            Type::List(e) | Type::Option(e) | Type::Shared(e) => self.is_encodable(e),
            Type::FixedList { elem, .. } => self.is_encodable(elem),
            Type::Map { key, value } => matches!(**key, Type::String) && self.is_encodable(value),
            Type::Named(n) => is_json_type_name(n) || self.trait_reg.implements_trait(n, crate::Generics::ENCODE),
            // D-SERDE9/10: a generic instantiation `Name<args>` is encodable when
            // `Name` derives Encode and every type arg that reaches the wire is
            // itself encodable. Phantom/skip-only params impose no obligation.
            Type::Apply { name, args } => apply_serde_ok(
                name,
                args,
                self.trait_reg,
                crate::Generics::ENCODE,
                &|t| self.is_encodable(t),
            ),
            _ => false,
        }
    }

    /// D-SERDE: a type `decode<T>` can construct. Mirrors [`Self::is_encodable`] but a
    /// user type must derive `Decode` (the dynamic `Json` tree is reached by bare
    /// `decode`, not the typed path).
    fn is_decodable(&self, t: &Type) -> bool {
        match t {
            Type::Int | Type::Float | Type::Bool | Type::String | Type::Char
            | Type::IntN { .. } | Type::Float32 => true,
            Type::List(e) | Type::Option(e) | Type::Shared(e) => self.is_decodable(e),
            Type::FixedList { elem, .. } => self.is_decodable(elem),
            Type::Map { key, value } => matches!(**key, Type::String) && self.is_decodable(value),
            Type::Named(n) => self.trait_reg.implements_trait(n, crate::Generics::DECODE),
            Type::Apply { name, args } => apply_serde_ok(
                name,
                args,
                self.trait_reg,
                crate::Generics::DECODE,
                &|t| self.is_decodable(t),
            ),
            _ => false,
        }
    }

    fn check_encodable(&mut self, t: &Type, span: Span) {
        if !self.is_encodable(t) {
            self.diags.push(e2411(&t.show(), true, span));
        }
    }

    fn check_decodable(&mut self, t: &Type, span: Span) {
        if !self.is_decodable(t) {
            self.diags.push(e2411(&t.show(), false, span));
        }
    }

    /// D-REACT1=B: a reactive `Signal<T>`/`Derived<T>` holds ordinary data that can
    /// be cloned to its dependents. Reject a function-typed value (E2913); everything
    /// else is admitted in sema (the codegen coverage gate handles the precise subset).
    pub(crate) fn reactive_value_ok(&mut self, ty: &Type, span: Span, kind: &str) -> bool {
        if matches!(ty, Type::Fn { .. }) {
            self.diags.push(reactive_bad_value_type(kind, ty, span));
            return false;
        }
        true
    }

    pub(crate) fn infer_core_call(
        &mut self,
        module: &str,
        name: &str,
        alias_span: Span,
        span: Span,
        type_args: &[Type],
        args: &mut [crate::AST::CallArg],
    ) -> Option<Type> {
        // D-EFF1: record the effect this Core call contributes to the enclosing
        // function's inferred set (erased in codegen; purely a sema fact).
        if let Some(e) = core_effect(module, name) {
            self.record_effect(e);
            // D-TXN2: an irreversible effect (Net/Fs/Exec — a network/file/
            // subprocess effect) can't be rolled back, so it is rejected when it
            // occurs directly inside a `#Transact { … }` block (E0746). The fix
            // is to move it after the block, or register it via
            // `name.on_commit(() => { … })` so it runs only on a clean commit.
            if self.txn_depth > 0 && is_irreversible_effect(e) {
                let api = format!("{}.{}", module_short_name(module), name);
                self.diags.push(e0746(&api, e, span));
            }
        }
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
        // D-DET1: `assume_deterministic { … }` (det_suppress > 0) suspends the
        // determinism rejection — the expert escape hatch.
        if self.in_pure && self.det_suppress == 0 && is_nondeterministic_core(module, name) {
            let api = format!("{}.{}", module_short_name(module), name);
            self.diags.push(e3403(&api, Some(span)));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            // Return the declared type so the call site doesn't cascade.
            return core_fixed_sig(module, name).and_then(|(_, ret)| ret);
        }
        // D-STDIN1=A / E3401: `pure fn` cannot read from stdin.
        if self.in_pure && self.det_suppress == 0 && is_impure_core(module, name) {
            let api = format!("{}.{}", module_short_name(module), name);
            self.diags.push(e3401(&self.fn_name.clone(), &api, &[], span));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return core_fixed_sig(module, name).and_then(|(_, ret)| ret);
        }
        // D-EFF1: `#Pure` is the empty effect set, so any effectful Core call —
        // `Fs`/`Net`/`Env`/`Exec`/`Db`/`Log`/`Io` — is impure inside a `#Pure fn`.
        // (Time/Rand return early above via E3403; stdin via the E3401 check
        // above, so this catches the remaining effect-carrying Core modules.)
        if self.in_pure && self.det_suppress == 0 && core_effect(module, name).is_some() {
            let api = format!("{}.{}", module_short_name(module), name);
            self.diags.push(e3401(&self.fn_name.clone(), &api, &[], span));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return core_fixed_sig(module, name).and_then(|(_, ret)| ret);
        }
        let sig = core_fixed_sig(module, name);
        match (module, name) {
            // D-ENC1 / D-SERDE6: typed encode/decode over the Encode/Decode model.
            // `to_string`/`to_string_pretty` accept any encodable value (the dynamic
            // `Json` / `[[String]]` / `Map` forms AND a `#[Codable]` value); the
            // codegen routes by the lowered arg type. `decode<T>` is the typed decode
            // (→ `T`, or `[T]` for CSV) keyed by the call-site type argument.
            (
                "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
                | "core.encoding.yaml",
                "to_string" | "to_string_pretty",
            ) => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                for a in args.iter_mut() {
                    self.borrow_ctx = true;
                    if let Some(t) = self.infer(&mut a.expr) {
                        self.check_encodable(&t, a.expr.span());
                    }
                }
                return Some(Type::String);
            }
            (
                "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
                | "core.encoding.yaml",
                "decode",
            ) if !type_args.is_empty() => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                let t = type_args[0].clone();
                self.check_decodable(&t, span);
                let inner = if module == "core.encoding.csv" {
                    Type::List(Box::new(t))
                } else {
                    t
                };
                return Some(result_ty(inner, decode_error_ty()));
            }
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
            // D-FLOATW1 (ratified 2026-06-22): sqrt/floor/ceil/pow are width-generic —
            // they return the same float width they receive (Float→Float, F32→F32).
            // Mixing widths is a compile error; explicit .to_f32()/.to_f64() converts.
            ("core.math", "sqrt" | "floor" | "ceil") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Float);
                };
                let ty = self.infer(&mut arg.expr)?;
                if !matches!(ty, Type::Float | Type::Float32) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`{}` needs Float or F32, not {}", name, ty.show()),
                        "transcendental functions operate on floating-point numbers".to_string(),
                        "pass a Float or F32 value".to_string(),
                        Some(arg.expr.span()),
                    ));
                    return None;
                }
                return Some(ty);
            }
            ("core.math", "pow") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                    for a in args.iter_mut().skip(1) { self.infer(&mut a.expr); }
                    return None;
                };
                if !matches!(first, Type::Float | Type::Float32) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`pow` needs Float or F32, not {}", first.show()),
                        "pow operates on floating-point numbers".to_string(),
                        "pass a Float or F32 base".to_string(),
                        Some(args[0].expr.span()),
                    ));
                    return None;
                }
                if let Some(second) = args.get_mut(1).and_then(|a| self.infer(&mut a.expr)) {
                    if second != first {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`pow` needs both arguments to have the same float type, but base is {} and exponent is {}", first.show(), second.show()),
                            "D-FLOATW1: mixing float widths is not allowed — use the same width for both".to_string(),
                            format!("convert with `.to_f32()` or `.to_float()` to match"),
                            Some(args[1].expr.span()),
                        ));
                    }
                }
                return Some(first);
            }
            ("core.math", "abs") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Int);
                };
                let ty = self.infer(&mut arg.expr)?;
                // D-FLOATW1: abs also works on F32.
                if !matches!(ty, Type::Int | Type::Float | Type::Float32) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`abs` needs Int, Float, or F32, not {}", ty.show()),
                        "absolute value is only defined for numbers".to_string(),
                        "pass an Int, Float, or F32".to_string(),
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
                if arg.convention != AccessConvention::Write {
                    self.diags.push(Diagnostic::error(
                        "E0202",
                        "`shuffle` edits its list in place".to_string(),
                        "write access (`~`) is required; the list must be passed with `~`".to_string(),
                        "write `random.shuffle(~xs)`".to_string(),
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
                    Some(Type::Fn { params, ret, .. }) => {
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
            // D-ROUTE1=A: jet.http.router() → HttpRouter.
            ("jet.http", "router") => {
                if !args.is_empty() {
                    self.diags.push(wrong_core_arity("router", 0, args.len(), span));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                }
                return Some(Type::Named("HttpRouter".to_string()));
            }
            // D-ROUTE1=A: http.parse(raw_string) → HttpRequest (parses HTTP/1.1 bytes).
            ("jet.http", "parse") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity("parse", 1, args.len(), span));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                self.expect_core_arg("parse", 0, &Type::String, &mut args[0]);
                return Some(Type::Named("HttpRequest".to_string()));
            }
            // D-ROUTE1=A: http.dispatch(router, req) → HttpResponse.
            ("jet.http", "dispatch") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity("dispatch", 2, args.len(), span));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                let router_ty = self.infer(&mut args[0].expr);
                match &router_ty {
                    Some(Type::Named(n)) if n == "HttpRouter" => {}
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`http.dispatch` needs an HttpRouter, not {}", other.show()),
                            "build a router with `http.router()` and register routes with `.get/.post/…`".to_string(),
                            "write `http.dispatch(router, req)`".to_string(),
                            Some(args[0].expr.span()),
                        ));
                    }
                    _ => {}
                }
                if let Some(arg) = args.get_mut(1) {
                    let req_ty = self.infer(&mut arg.expr);
                    match &req_ty {
                        Some(Type::Named(n)) if n == "HttpRequest" => {}
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`http.dispatch` needs an HttpRequest, not {}", other.show()),
                                "parse the raw request with `http.parse(raw)`".to_string(),
                                "write `http.dispatch(router, req)` where `req` is an HttpRequest".to_string(),
                                Some(arg.expr.span()),
                            ));
                        }
                        _ => {}
                    }
                }
                return Some(Type::Named("HttpResponse".to_string()));
            }
            // E2-M10: jet.http.serve(addr, handler) — blocking accept loop.
            // handler: fn(HttpRequest) -> HttpResponse (lambda) or HttpRouter.
            ("jet.http", "serve") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity("serve", 2, args.len(), span));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                self.expect_core_arg("serve", 0, &Type::String, &mut args[0]);
                // Accept an HttpRouter or a callable (lambda/fn pointer).
                let handler_ty = self.infer(&mut args[1].expr);
                match &handler_ty {
                    Some(Type::Fn { .. }) => {}
                    Some(Type::Named(n)) if n == "HttpRouter" => {}
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`http.serve` handler must be a function or HttpRouter, not {}", other.show()),
                            "the handler is called with each incoming `HttpRequest`".to_string(),
                            "pass a router (`http.router()`) or a lambda: `(req) => HttpResponse { … }`".to_string(),
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
            // D-REACT1=B: reactive.signal(initial) → Signal<T>. The value type is
            // inferred from the initial value; an explicit annotation may guide an
            // empty/ambiguous literal via `expected_type`.
            ("jet.reactive", "signal") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity("signal", 1, args.len(), span));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                // If the binding is annotated `Signal<T>`, push `T` as the expected
                // type for the initial value so an ambiguous literal elaborates.
                let saved = self.expected_type.clone();
                if let Some(Type::Apply { name, args: ta }) = &self.expected_type {
                    if name == crate::Syntax::TYPE_SIGNAL && ta.len() == 1 {
                        self.expected_type = Some(ta[0].clone());
                    }
                }
                let init_ty = self.infer(&mut args[0].expr);
                self.expected_type = saved;
                let elem = init_ty.unwrap_or(Type::Int);
                if !self.reactive_value_ok(&elem, args[0].expr.span(), "signal") {
                    return None;
                }
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_SIGNAL.to_string(),
                    args: vec![elem],
                });
            }
            // D-REACT1=B: reactive.derived(() => expr) → Derived<T>. The compute
            // closure takes no parameters; `T` is its return type. Reading a signal
            // (`.get()`) inside the body subscribes the derived to it.
            ("jet.reactive", "derived") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity("derived", 1, args.len(), span));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                let lam_ty = self.infer(&mut args[0].expr);
                let elem = match &lam_ty {
                    Some(Type::Fn { params, ret, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(reactive_lambda_arity("derived", params.len(), args[0].expr.span()));
                            return None;
                        }
                        match ret {
                            Some(r) => (**r).clone(),
                            None => {
                                self.diags.push(reactive_derived_unit(args[0].expr.span()));
                                return None;
                            }
                        }
                    }
                    Some(other) => {
                        self.diags.push(reactive_not_lambda("derived", other, args[0].expr.span()));
                        return None;
                    }
                    None => return None,
                };
                if !self.reactive_value_ok(&elem, args[0].expr.span(), "derived") {
                    return None;
                }
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_DERIVED.to_string(),
                    args: vec![elem],
                });
            }
            // D-REACT1=B: reactive.effect(() => { … }) runs the body now and again
            // whenever a signal it read changes. The body is a zero-parameter,
            // unit-returning closure; the call itself yields nothing.
            ("jet.reactive", "effect") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity("effect", 1, args.len(), span));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                let lam_ty = self.infer(&mut args[0].expr);
                match &lam_ty {
                    Some(Type::Fn { params, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(reactive_lambda_arity("effect", params.len(), args[0].expr.span()));
                            return None;
                        }
                    }
                    Some(other) => {
                        self.diags.push(reactive_not_lambda("effect", other, args[0].expr.span()));
                        return None;
                    }
                    None => return None,
                }
                return None; // effect returns nothing
            }
            // D-PENDING1=B: Loadable<T,E> constructors — idle/loading/loaded/failed.
            ("core.async.loadable", "idle") => {
                for a in args.iter_mut() { self.infer(&mut a.expr); }
                return Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![Type::Named("Unknown".to_string()), Type::Named("Unknown".to_string())],
                });
            }
            ("core.async.loadable", "loading") => {
                for a in args.iter_mut() { self.infer(&mut a.expr); }
                return Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![Type::Named("Unknown".to_string()), Type::Named("Unknown".to_string())],
                });
            }
            ("core.async.loadable", "loaded") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity("loaded", 1, args.len(), span));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                let val_ty = self.infer(&mut args[0].expr).unwrap_or(Type::Named("Unknown".to_string()));
                return Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![val_ty, Type::Named("Unknown".to_string())],
                });
            }
            ("core.async.loadable", "failed") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity("failed", 1, args.len(), span));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                let err_ty = self.infer(&mut args[0].expr).unwrap_or(Type::Named("Unknown".to_string()));
                return Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![Type::Named("Unknown".to_string()), err_ty],
                });
            }
            // D-APPROX1=A: sketch constructors.
            ("core.sketch.hll", "new") => {
                for a in args.iter_mut() { self.infer(&mut a.expr); }
                return Some(Type::Named("HyperLogLog".to_string()));
            }
            ("core.sketch.tdigest", "new") => {
                for a in args.iter_mut() { self.infer(&mut a.expr); }
                return Some(Type::Named("TDigest".to_string()));
            }
            ("core.sketch.cms", "new") => {
                for a in args.iter_mut() { self.infer(&mut a.expr); }
                return Some(Type::Named("CountMinSketch".to_string()));
            }
            ("core.sketch.reservoir", "new") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity("new", 1, args.len(), span));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                self.expect_core_arg("new", 0, &Type::Int, &mut args[0]);
                return Some(Type::Named("ReservoirSampler".to_string()));
            }
            // D-HONESTNUM1=A: `M.from(value, uncertainty)` → `Measurement<Float>`.
            ("core.science.measurement", "from") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity("from", 2, args.len(), span));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                self.expect_core_arg("from", 0, &Type::Float, &mut args[0]);
                self.expect_core_arg("from", 1, &Type::Float, &mut args[1]);
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_MEASUREMENT.to_string(),
                    args: vec![Type::Float],
                });
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
            if *conv == AccessConvention::Write && arg.convention != AccessConvention::Write {
                self.diags.push(Diagnostic::error(
                    "E0202",
                    format!("argument {} to `{}` requires write access (`~`)", i + 1, name),
                    "this standard library call edits that value in place".to_string(),
                    format!("write `{}value` for this argument", Syntax::SIGIL_MUTATE),
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
            "Bool" => vec![Type::Bool],
            "Int" => vec![Type::Int],
            "Float" => vec![Type::Float],
            "Text" => vec![Type::String],
            "Array" => vec![Type::List(Box::new(json.clone()))],
            "Object" => vec![Type::Map {
                key: Box::new(Type::String),
                value: Box::new(json.clone()),
            }],
            _ => {
                let candidates = ["Null", "Bool", "Int", "Float", "Text", "Array", "Object"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>();
                let mut fix = "check the variant name".to_string();
                if let Some(s) = suggest_field(variant, &candidates) {
                    fix = format!("did you mean `{}`?", s);
                }
                self.diags.push(Diagnostic::error(
                    "E0304",
                    format!("`{}` has no variant `{}`", Syntax::TYPE_DATA, variant),
                    "the dynamic `Data` value exposes Null/Bool/Int/Float/Text/Array/Object".to_string(),
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
                    Syntax::TYPE_DATA,
                    variant,
                    expected.len(),
                    if expected.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                "each `Data` variant has a fixed payload (Bool/Int/Float→scalar, Text→String, Array→[Data], Object→Map)".to_string(),
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
                format!("`{}` passed to a parameter that does not consume", Syntax::SIGIL_MOVE),
                "standard library functions in M10 read their ordinary arguments unless documented otherwise"
                    .to_string(),
                format!("remove `{}` here", Syntax::SIGIL_MOVE),
                Some(arg.span),
            ));
        }
        if matches!(param_ty, Type::String | Type::List(_) | Type::Map { .. }) {
            self.borrow_ctx = true;
        }
        // D-SG9: expose the parameter's type to `infer` so a fixed-width integer
        // literal argument (`f(5)` where `f` takes a `U8`) adopts that width and
        // is range-checked (E1003) at the literal. Restored after the argument.
        let saved_expected = self.expected_type.clone();
        self.expected_type = Some(param_ty.clone());
        let got = self.infer(&mut arg.expr);
        self.expected_type = saved_expected;
        if let Some(got) = got {
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
                                "implicit clone of `{}`; this value is borrowed, so it is copied",
                                name
                            ),
                            format!("`{}` stores its own copy of this value", call_name),
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
    Type::IntN { signed: false, bits: 8 }
}

pub(crate) fn is_u8_ty(ty: &Type) -> bool {
    matches!(ty, Type::IntN { signed: false, bits: 8 })
}

pub(crate) fn json_ty() -> Type {
    Type::Named(Syntax::TYPE_DATA.to_string())
}

pub(crate) fn json_error_ty() -> Type {
    Type::Named(Syntax::TYPE_JSON_ERROR.to_string())
}

// D-ENC-DYN1=A+: the dynamic encoding value `Data` (+ aliases `Json`/`Toml`/
// `Yaml`/`Csv`).
pub(crate) fn is_json_type_name(name: &str) -> bool {
    Syntax::is_data_type_name(name)
}

/// D-SERDE2: the typed-decode error (`{ path, reason }`). Flows as the error arm
/// of `decode<T>` results; the user composes it with `??` and rarely names it.
pub(crate) fn decode_error_ty() -> Type {
    Type::Named("DecodeError".to_string())
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
        // D-DET1: deterministic injected capability handles.
        // D-DET-CAPAPI: `Duration` value type for the widened clock surface.
        | "Clock" | "Rng" | "Duration"
        | "FileReader" | "FileWriter" | "FileLines"
        | "StdinHandle" | "StdinLines"
        // D-LSDIR1=A: directory entry value.
        | "DirEntry"
        // E2-M10: networking opaque types.
        | "TcpListener" | "TcpStream" | "HttpRequest" | "HttpResponse" | "HttpRouter"
        // D-ALLOC1/D-ALLOC-C (ratified 2026-06-19): allocator opaque types.
        | "Arena" | "Bump" | "Pool" | "Fixed"
        // D-ARGS1 (ratified 2026-06-22): declarative CLI arg parsing types.
        | "ArgsSpec" | "ParsedArgs"
        // D-TERM1 (ratified 2026-06-22): terminal key-event enum.
        | "Key"
        // D-SERDE2: the format-agnostic value tree + typed-decode error.
        | "DataTree" | "DecodeError"
        // D-SIMD2 / D-LINALG1: built-in SIMD lane + linear-algebra value types.
        | "F32x4" | "F64x2"
        | "Vec2" | "Vec3" | "Vec4" | "Mat3" | "Mat4"
        // D-REACT1=B: opt-in reactive handle types (used bare as `Signal<T>`/`Derived<T>`).
        | "Signal" | "Derived"
        // D-HONESTNUM1=A: Measurement<T> value ± uncertainty.
        | "Measurement"
        // D-PENDING1=B: async UI state machine.
        | "Loadable"
        // D-APPROX1=A: approximate sketch data structures.
        | "HyperLogLog" | "TDigest" | "CountMinSketch" | "ReservoirSampler"
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
    // D-SERDE2: DecodeError exposes the field path and a plain reason.
    if type_name == "DecodeError" {
        return match field {
            "path" | "reason" => Some(Type::String),
            _ => None,
        };
    }
    match (type_name, field) {
        // D-LSDIR1=A: DirEntry has name (bare filename), path (full path), is_dir.
        ("DirEntry", "name" | "path") => Some(Type::String),
        ("DirEntry", "is_dir") => Some(Type::Bool),
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

pub fn core_json_pattern_types(variant: &str) -> Option<Vec<Type>> {
    let json = json_ty();
    match variant {
        "Null" => Some(Vec::new()),
        "Bool" => Some(vec![Type::Bool]),
        "Int" => Some(vec![Type::Int]),
        "Float" => Some(vec![Type::Float]),
        "Text" => Some(vec![Type::String]),
        "Array" => Some(vec![Type::List(Box::new(json.clone()))]),
        "Object" => Some(vec![Type::Map {
            key: Box::new(Type::String),
            value: Box::new(json),
        }]),
        _ => None,
    }
}

/// D-TERM1 (ratified 2026-06-22): pattern types for `Key` enum variants.
/// Used by the pattern checker to validate `if k == Key.Char(c)` etc.
pub(crate) fn core_key_pattern_types(variant: &str) -> Option<Vec<Type>> {
    match variant {
        // Unit variants — no payload.
        "Enter" | "Escape" | "Backspace" | "Tab" | "Delete"
        | "Up" | "Down" | "Left" | "Right" | "Unknown" => Some(Vec::new()),
        // `Key.Char(c)` — one Char payload.
        "Char" => Some(vec![Type::Char]),
        // `Key.Ctrl(c)` — one Char payload (the control character).
        "Ctrl" => Some(vec![Type::Char]),
        // `Key.F(n)` — one Int payload (function key number 1–12).
        "F" => Some(vec![Type::Int]),
        _ => None,
    }
}

/// D-TERM1 (ratified 2026-06-22): synthesised variant table for the `Key` enum.
/// Used by `resolve_enum_variants_cloned` so `Key.Char(c)` / `Key.Enter` literals
/// pass type-checking without `Key` being in the user type registry.
pub(crate) fn core_key_variants() -> std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    let zero = Span::new(0, 0);
    let mut m = std::collections::HashMap::new();
    // Unit variants.
    for name in &["Enter", "Escape", "Backspace", "Tab", "Delete",
                  "Up", "Down", "Left", "Right", "Unknown"]
    {
        m.insert((*name).to_string(), (zero, VariantPayload::Unit));
    }
    // Single-payload variants.
    m.insert("Char".to_string(),  (zero, VariantPayload::Single(Type::Char,  zero)));
    m.insert("Ctrl".to_string(),  (zero, VariantPayload::Single(Type::Char,  zero)));
    m.insert("F".to_string(),     (zero, VariantPayload::Single(Type::Int,   zero)));
    m
}

/// E2-M7: type-check a method call on a FileReader or FileWriter handle (D-IO2).
/// Returns `Some(return_type)` when the method is valid, or emits E2501 and
/// returns `None` for an invalid method / wrong-direction call.
pub fn file_handle_method_return(
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
        // D-STDIN1=A: StdinHandle methods.
        "StdinHandle" => match method {
            "lines" if n_args == 0 => Some(Some(Type::Named("StdinLines".to_string()))),
            "read_line" if n_args == 0 => {
                Some(Some(result_ty(Type::Option(Box::new(Type::String)), io)))
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

/// D-SIMD2 / D-LINALG1: is `name` a built-in math value type (lane or linalg)?
pub fn is_math_type(name: &str) -> bool {
    is_simd_lane_type(name) || is_linalg_type(name)
}

/// D-SIMD2: a portable SIMD lane type (`F32x4`/`F64x2`).
pub fn is_simd_lane_type(name: &str) -> bool {
    matches!(name, "F32x4" | "F64x2")
}

/// D-LINALG1: a linear-algebra value type (vectors + square matrices).
pub(crate) fn is_linalg_type(name: &str) -> bool {
    matches!(name, "Vec2" | "Vec3" | "Vec4" | "Mat3" | "Mat4")
}

/// The scalar component type of a math value type. SIMD lanes carry their named
/// float width (`F32x4` → `F32`/`Float32`, `F64x2` → `Float`); linalg types are
/// all `F64`/`Float`.
pub fn math_scalar_ty(name: &str) -> Type {
    match name {
        "F32x4" => Type::Float32,
        _ => Type::Float,
    }
}

/// The number of scalar slots in the positional constructor / `from_array` bridge.
/// Lanes: lane count. Vectors: dimension. Matrices: N*N (column-major flat).
pub(crate) fn math_arity(name: &str) -> usize {
    match name {
        "F32x4" => 4,
        "F64x2" => 2,
        "Vec2" => 2,
        "Vec3" => 3,
        "Vec4" => 4,
        "Mat3" => 9,
        "Mat4" => 16,
        _ => 0,
    }
}

/// D-SIMD2 / D-LINALG1: type-check a positional constructor `T(a, b, …)` for a
/// built-in math type. The arg types are bound by `expected` so a literal `1.0`
/// elaborates to the component type (`F32` for `F32x4`). Returns the field types
/// the caller must check each argument against; arity is `math_arity(name)`.
pub(crate) fn math_constructor_arg_types(name: &str) -> Option<Vec<Type>> {
    if !is_math_type(name) {
        return None;
    }
    let scalar = math_scalar_ty(name);
    // `from_array`-style construction of a matrix takes its N*N components in
    // column-major order; vectors/lanes take one scalar per slot.
    Some(vec![scalar; math_arity(name)])
}

/// D-SIMD2 / D-LINALG1: the `[T#N]` fixed-list bridge type for a math value type,
/// used by `T.from_array([..])` / `v.to_array()`. `None` for non-math types.
pub(crate) fn math_array_bridge_ty(name: &str) -> Option<Type> {
    if !is_math_type(name) {
        return None;
    }
    Some(Type::FixedList {
        elem: Box::new(math_scalar_ty(name)),
        len: math_arity(name) as u64,
    })
}

/// D-SIMD2 / D-LINALG1: type-check an INSTANCE method `recv.method(args)` on a
/// built-in math type. `Some(Some(t))` → returns `t`; `Some(None)` → not a method
/// (caller falls through to its normal "no such method" diagnostic).
pub fn math_method_return(name: &str, method: &str, n_args: usize) -> Option<Type> {
    let float = Type::Float;
    let scalar = math_scalar_ty(name);
    let self_ty = Type::Named(name.to_string());
    if is_simd_lane_type(name) {
        return match (method, n_args) {
            // Reductions collapse the lanes to a single scalar of the lane width.
            ("sum" | "product" | "min" | "max", 0) => Some(scalar),
            ("reduce", 1) => Some(scalar),
            // `[F32#4]` round-trip out.
            ("to_array", 0) => math_array_bridge_ty(name),
            _ => None,
        };
    }
    // linalg
    match name {
        "Vec2" | "Vec3" | "Vec4" => match (method, n_args) {
            ("dot", 1) => Some(float),
            // cross product is only defined for 3-vectors.
            ("cross", 1) if name == "Vec3" => Some(self_ty),
            ("length", 0) => Some(float),
            ("normalize", 0) => Some(self_ty),
            ("to_array", 0) => math_array_bridge_ty(name),
            _ => None,
        },
        "Mat3" | "Mat4" => match (method, n_args) {
            ("matmul", 1) => Some(self_ty.clone()),
            ("transpose", 0) => Some(self_ty),
            // `m * v` is the operator path; `transform` is the named method form.
            ("transform", 1) => Some(Type::Named(
                if name == "Mat3" { "Vec3" } else { "Vec4" }.to_string(),
            )),
            ("to_array", 0) => math_array_bridge_ty(name),
            _ => None,
        },
        _ => None,
    }
}

/// D-SIMD2 / D-LINALG1: type-check a STATIC method `T.method(args)` on a math
/// type. Only `splat` (lanes/vectors) and `from_array` are provided.
pub fn math_static_return(name: &str, method: &str, n_args: usize) -> Option<Type> {
    if !is_math_type(name) {
        return None;
    }
    let self_ty = Type::Named(name.to_string());
    match (method, n_args) {
        ("splat", 1) => Some(self_ty),
        ("from_array", 1) => Some(self_ty),
        _ => None,
    }
}

/// D-SIMD2 / D-LINALG1: the argument type a static method expects.
pub fn math_static_arg_ty(name: &str, method: &str) -> Option<Type> {
    match method {
        "splat" => Some(math_scalar_ty(name)),
        "from_array" => math_array_bridge_ty(name),
        _ => None,
    }
}

/// D-SIMD2 / D-LINALG1: the argument type an instance method expects (for the
/// single-arg methods). `None` means "no fixed arg type" (e.g. nullary methods).
pub(crate) fn math_method_arg_ty(name: &str, method: &str) -> Option<Type> {
    let self_ty = Type::Named(name.to_string());
    match (name, method) {
        (_, "dot") | (_, "cross") => Some(self_ty),
        (_, "matmul") => Some(self_ty),
        ("Mat3", "transform") => Some(Type::Named("Vec3".to_string())),
        ("Mat4", "transform") => Some(Type::Named("Vec4".to_string())),
        // `reduce(#Op)` takes a reduce-op marker, checked specially by the caller.
        _ => None,
    }
}

/// D-SIMD2: the closed set of reduce-op markers accepted by `v.reduce(#Op)`.
pub(crate) fn simd_reduce_markers() -> &'static [&'static str] {
    &["Add", "Mul", "Min", "Max"]
}

/// D-SIMD2 / D-LINALG1: type-check a binary operator between two math values.
/// Returns the result type, or `None` if the op isn't defined for these operands.
/// Operator overloading is blessed on this closed built-in family ONLY.
pub fn math_binop_result(op: crate::AST::BinOp, lt: &str, rt: &str) -> Option<Type> {
    use crate::AST::BinOp;
    let same = lt == rt;
    match op {
        // Element-wise add/sub require identical types.
        BinOp::Add | BinOp::Sub if same && is_math_type(lt) => Some(Type::Named(lt.to_string())),
        // Multiplication: lane×lane / vec×vec element-wise; matrix×vector transform.
        BinOp::Mul => match (lt, rt) {
            (a, b) if a == b && is_math_type(a) => Some(Type::Named(a.to_string())),
            ("Mat3", "Vec3") => Some(Type::Named("Vec3".to_string())),
            ("Mat4", "Vec4") => Some(Type::Named("Vec4".to_string())),
            _ => None,
        },
        // Division: lane÷lane element-wise (linalg has no `/`).
        BinOp::Div if same && is_simd_lane_type(lt) => Some(Type::Named(lt.to_string())),
        BinOp::Eq | BinOp::Ne if same && is_math_type(lt) => Some(Type::Bool),
        _ => None,
    }
}

/// E2-M10: type-check a method call on a networking opaque type.
/// Returns `Some(return_type)` when the method is valid.
pub fn net_method_return(
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
        // D-ROUTE1=A: req.param("name") → String? (none if not a param route or name absent).
        ("HttpRequest", "param") => Some(Some(Type::Option(Box::new(str_ty.clone())))),
        // D-ROUTE1=A: HttpRouter registration methods.
        ("HttpRouter", "get" | "post" | "put" | "delete") => Some(Some(unit.clone())),
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

/// D-PATHFS1: return type for `Path` instance method calls.
/// D-PENDING1=B: instance methods on `Loadable<T, E>`.
/// Returns `Some(Some(T))` for a valid method, `None` if not a Loadable method.
pub fn loadable_method_return(
    type_apply: &Type,
    method: &str,
    _n_args: usize,
) -> Option<Option<Type>> {
    let (val_ty, _err_ty) = match type_apply {
        Type::Apply { name, args } if name == "Loadable" && args.len() == 2 => {
            (args[0].clone(), args[1].clone())
        }
        _ => return None,
    };
    match method {
        "is_loading" => Some(Some(Type::Bool)),
        "is_loaded"  => Some(Some(Type::Bool)),
        "is_failed"  => Some(Some(Type::Bool)),
        "is_idle"    => Some(Some(Type::Bool)),
        // loaded() → T? — returns the value if in Loaded state, null otherwise.
        "loaded" => Some(Some(Type::Option(Box::new(val_ty)))),
        // or_else(default: T) → T
        "or_else" => Some(Some(val_ty)),
        _ => None,
    }
}

/// D-APPROX1=A: return the type name string for a sketch receiver type.
pub fn sketch_type_name(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Named(n) => match n.as_str() {
            "HyperLogLog" => Some("HyperLogLog"),
            "TDigest" => Some("TDigest"),
            "CountMinSketch" => Some("CountMinSketch"),
            "ReservoirSampler" => Some("ReservoirSampler"),
            _ => None,
        },
        _ => None,
    }
}

/// D-APPROX1=A: method return types for sketch data structures.
pub fn sketch_method_return(
    ty: &Type,
    method: &str,
    _args: &[crate::AST::CallArg],
) -> Option<Option<Type>> {
    let name = sketch_type_name(ty)?;
    match (name, method) {
        ("HyperLogLog", "add") => Some(None),   // void
        ("HyperLogLog", "count") => Some(Some(Type::Int)),
        ("TDigest", "add") => Some(None),
        ("TDigest", "quantile") => Some(Some(Type::Float)),
        ("CountMinSketch", "add") => Some(None),
        ("CountMinSketch", "count") => Some(Some(Type::Int)),
        ("ReservoirSampler", "add") => Some(None),
        ("ReservoirSampler", "sample") => Some(Some(Type::List(Box::new(Type::String)))),
        _ => None,
    }
}

pub fn path_method_return(
    type_name: &str,
    method: &str,
    _n_args: usize,
    _span: Span,
    _diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    if type_name != "Path" {
        return None;
    }
    let path = || Type::Named("Path".to_string());
    match method {
        "join"         => Some(Some(path())),
        "parent"       => Some(Some(Type::Option(Box::new(path())))),
        "extension"    => Some(Some(Type::Option(Box::new(Type::String)))),
        "stem"         => Some(Some(Type::Option(Box::new(Type::String)))),
        "to_string"    => Some(Some(Type::String)),
        "write_atomic" => Some(Some(result_ty(unit_ty(), Type::String))),
        "walk"         => Some(Some(Type::List(Box::new(path())))),
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
pub fn ptr_type(elem: Type) -> Type {
    Type::Apply {
        name: Syntax::TYPE_PTR.to_string(),
        args: vec![elem],
    }
}

/// S58 (E2-M13): the element type of a `Ptr<T>`, if `t` is one.
pub fn ptr_elem(t: &Type) -> Option<Type> {
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
            "wrap it: #{}(\"why this is safe\") {{ … }}",
            Syntax::KW_UNSAFE
        ),
        Some(span),
    )
}

/// c109 Phase 20: the polymorphic core specials whose return type is resolved by
/// `infer_core_call`'s bespoke arg-type logic (NOT the fixed `core_fixed_sig`
/// table). Sema writes the resolved return back onto the `Expr::MethodCall`
/// `resolved_ret` field for exactly these, so the TIR reads it totally (I3).
/// `io.input` is excluded — it IS in `core_fixed_sig` (`String?`), covered by
/// Phase 10. `core.mem` ptr ops have their own Phase-18 lowering.
pub fn is_polymorphic_core_special(module: &str, name: &str) -> bool {
    matches!(
        (module, name),
        ("core.math", "abs")
            | ("core.math", "min")
            | ("core.math", "max")
            | ("core.math", "clamp")
            // D-FLOATW1: sqrt/floor/ceil/pow are width-generic (Float→Float, F32→F32);
            // their return type is arg-type-dependent, so they use resolved_ret.
            | ("core.math", "sqrt")
            | ("core.math", "floor")
            | ("core.math", "ceil")
            | ("core.math", "pow")
            | ("core.random", "pick")
            | ("core.random", "shuffle")
            | ("core.io", "eprint")
            // D-ENC1 / D-SERDE6: typed encode/decode return types depend on the value
            // type / call-site `<T>`, so codegen reads them from resolved_ret (I3).
            | (
                "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
                | "core.encoding.yaml",
                "to_string" | "to_string_pretty" | "decode",
            )
            // D-REACT1=B: the reactive producers return `Signal<T>`/`Derived<T>` whose
            // element type is inferred from the initial value / closure return — not in
            // `core_fixed_sig`, so codegen reads it from resolved_ret (I3).
            | ("jet.reactive", "signal" | "derived")
    )
}

pub fn core_fixed_sig(
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
        // D-LSDIR1=A: returns [DirEntry] ({name, path, is_dir}) — full path + type in one step.
        ("core.fs", "list_dir") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::List(Box::new(Type::Named("DirEntry".to_string()))), io_error_ty())),
        )),
        ("core.fs", "copy" | "rename") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.io", "args") => Some((vec![], Some(Type::List(Box::new(Type::String))))),
        ("core.io", "read_all_input") => {
            Some((vec![], Some(result_ty(Type::String, io_error_ty()))))
        }
        // D-STDIN1=A: streaming line-by-line stdin.
        ("core.io", "stdin") => Some((vec![], Some(Type::Named("StdinHandle".to_string())))),
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
        // D-DET1: deterministic injected RNG capability. `random.rng(seed)` builds a
        // reproducible `Rng` from a caller-supplied seed (a pure value); a `#Pure fn`
        // may draw randomness through it (`rng.int(lo, hi)` / `rng.float()`) while the
        // ambient `random.int(…)` stays E3403.
        ("core.random", "rng") => {
            Some((vec![(read, Type::Int)], Some(Type::Named(crate::Syntax::RNG_TYPE.to_string()))))
        }
        ("core.time", "now") => Some((vec![], Some(Type::Int))),
        ("core.time", "sleep") => Some((vec![(read, Type::Int)], None)),
        ("core.time", "start") => Some((vec![], Some(Type::Named("Stopwatch".to_string())))),
        // D-DET1: deterministic injected Clock capability. `time.clock(seed)` builds a
        // reproducible `Clock` from a caller-supplied start instant (a pure Int, ms);
        // a `#Pure fn` may read time through it (`clock.now()` / `clock.tick(ms)`)
        // while the ambient `time.now()` stays E3403.
        ("core.time", "clock") => {
            Some((vec![(read, Type::Int)], Some(Type::Named(crate::Syntax::CLOCK_TYPE.to_string()))))
        }
        // D-DET-CAPAPI: `time.ms(n)` / `time.secs(n)` mint a deterministic `Duration`
        // value (pure — no ambient effect, like `time.clock`). The clock advances by
        // one with `clock.wait(d)`; read it back with `duration.millis()`.
        ("core.time", "ms" | "secs") => {
            Some((vec![(read, Type::Int)], Some(Type::Named(crate::Syntax::DURATION_TYPE.to_string()))))
        }
        // D-ENC1 + D-JSONVERB1: unified encoding. `parse` → dynamic JSON value; `decode`
        // → lenient typed decode (D-JSON3); `to_string`/`to_string_pretty` → serialize.
        ("core.encoding.json", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("core.encoding.json", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("core.encoding.json", "to_string" | "to_string_pretty") => {
            Some((vec![(read, json)], Some(Type::String)))
        }
        // jet.csv → core.encoding.csv: parse text into a list of rows (list of fields).
        ("core.encoding.csv", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::List(Box::new(Type::List(Box::new(Type::String)))),
                Type::String,
            )),
        )),
        ("core.encoding.csv", "to_string") => Some((
            vec![(read, Type::List(Box::new(Type::List(Box::new(Type::String)))))],
            Some(Type::String),
        )),
        // D-ENC-DYN1=A+ (c152): TOML is a full adapter over the rich `Data` value —
        // `parse` returns `Toml` (= `Data`); `to_string` takes any encodable value.
        ("core.encoding.toml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("core.encoding.toml", "to_string") => Some((vec![(read, json.clone())], Some(Type::String))),
        // D-ENC-YAML1 = A (c152): YAML is a full adapter over the rich `Data` value.
        ("core.encoding.yaml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("core.encoding.yaml", "to_string") => Some((vec![(read, json.clone())], Some(Type::String))),
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
        // jet.log: structured JSON logging to stderr (E2-M12, D-OBS3).
        ("jet.log", "info" | "warn" | "error" | "debug") => Some((vec![(read, string)], None)),
        ("jet.log", "set_level") => Some((vec![(read, Type::String)], None)),
        // D-OBS3: set OTel trace_id for all subsequent log entries on this thread.
        ("jet.log", "set_trace_id") => Some((vec![(read, Type::String)], None)),
        // D-LOGFMT1=A: override log output format ("json" | "text").
        ("jet.log", "setup") => Some((vec![(read, Type::String)], None)),
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
            vec![(AccessConvention::Write, Type::Named("TcpStream".to_string()))],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.net", "tcp_write") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TcpStream".to_string())),
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
                (AccessConvention::Write, Type::Named("TcpStream".to_string())),
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
        // D-DEP-ARCHIVE1=A: jet.archive — gzip compress/decompress via the `flate2` crate FFI bridge.
        // Both functions take `[U8]` and return `[U8]`. Compression is infallible; decompression
        // returns an empty list on corrupt input (no error path exposed to the caller).
        ("jet.archive", "gzip_compress" | "gzip_decompress") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: zip_compress — create a single-entry zip archive.
        // Takes (name: String, data: [U8]) → [U8].
        ("jet.archive", "zip_compress") => Some((
            vec![(read, Type::String), (read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: zip_decompress — extract first entry from a zip archive.
        // Takes [U8] → [U8]. Returns empty list on invalid input.
        ("jet.archive", "zip_decompress") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: tar_add — append/replace a named entry in a tar archive.
        // Takes (archive: [U8], name: String, data: [U8]) → [U8].
        ("jet.archive", "tar_add") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::String),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: tar_get — extract a named entry from a tar archive.
        // Takes (archive: [U8], name: String) → [U8]. Empty on not-found or bad input.
        ("jet.archive", "tar_get") => Some((
            vec![(read, Type::List(Box::new(u8_ty()))), (read, Type::String)],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: tar_names_json — list entry names as a JSON array string.
        // Takes [U8] → String. Returns "[]" on empty or invalid archive.
        ("jet.archive", "tar_names_json") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::String),
        )),
        // D-DEP-DB1: jet.db — SQLite via rusqlite (bundled).
        // open/open_memory return a u64 handle (0 = error).
        ("jet.db", "open") => Some((
            vec![(read, Type::String)],
            Some(Type::IntN { signed: false, bits: 64 }),
        )),
        ("jet.db", "open_memory") => Some((
            vec![],
            Some(Type::IntN { signed: false, bits: 64 }),
        )),
        // close/exec/query_json take the handle by value (u64 is a scalar).
        ("jet.db", "close") => Some((
            vec![(read, Type::IntN { signed: false, bits: 64 })],
            Some(Type::Bool),
        )),
        ("jet.db", "exec") => Some((
            vec![
                (read, Type::IntN { signed: false, bits: 64 }),
                (read, Type::String),
            ],
            Some(Type::Bool),
        )),
        ("jet.db", "query_json") => Some((
            vec![
                (read, Type::IntN { signed: false, bits: 64 }),
                (read, Type::String),
            ],
            Some(Type::String),
        )),
        // D-UUIDENC1=A: hex and base64 codecs. `encode` is infallible; `decode`
        // returns `[Byte] ? String` (invalid input → Err).
        ("core.encoding.hex", "encode") => Some((
            vec![(read, list_u8.clone())],
            Some(Type::String),
        )),
        ("core.encoding.hex", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        ("core.encoding.base64", "encode") => Some((
            vec![(read, list_u8.clone())],
            Some(Type::String),
        )),
        ("core.encoding.base64", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        // D-UUIDENC1=A: UUID v4 (system CSPRNG) and v7 (injectable Clock).
        // `v4()` reads /dev/urandom; `v7(clock)` extracts the timestamp from the
        // injected Clock so tests can produce a deterministic UUID.
        ("core.uuid", "v4") => Some((vec![], Some(Type::String))),
        ("core.uuid", "v7") => Some((
            vec![(read, Type::Named(crate::Syntax::CLOCK_TYPE.to_string()))],
            Some(Type::String),
        )),
        // D-ARGS1 (ratified 2026-06-22): `args.spec()` → `ArgsSpec` builder.
        // The builder methods (.flag/.option/.positional/.help/.parse) are handled
        // in `args_spec_method_return` / `parsed_args_method_return` below.
        ("core.args", "spec") => Some((vec![], Some(Type::Named("ArgsSpec".to_string())))),
        // D-TERM1 (ratified 2026-06-22): terminal direct-input.
        // `term.read_key()` → `Key` (the key-event enum). No arguments.
        ("core.term", "read_key") => Some((
            vec![],
            Some(Type::Named(crate::Syntax::TYPE_KEY.to_string())),
        )),
        // D-ADAPTFID1=A: adaptive fidelity signal.
        ("core.perf", "fidelity") => Some((vec![], Some(float))),
        ("core.perf", "set_fidelity") => Some((vec![(read, float)], Some(unit))),
        _ => None,
    }
}

/// D-ARGS1: type-check a method call on `ArgsSpec` (the builder).
/// Builder methods return `ArgsSpec` for chaining; `parse` returns `ParsedArgs ? String`.
/// Returns `Some(Some(ty))` for valid calls, `Some(None)` for void (none here),
/// `None` for unknown method (caller emits E0102).
pub(crate) fn args_spec_method_return(
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let spec_ty = Type::Named("ArgsSpec".to_string());
    match (method, n_args) {
        // .flag("name", "help text") → ArgsSpec  (boolean flag, no value)
        ("flag", 2) => Some(Some(spec_ty)),
        // .option("name", "help text", "METAVAR") → ArgsSpec  (value option)
        ("option", 3) => Some(Some(spec_ty)),
        // .positional("name", "help text") → ArgsSpec  (positional argument)
        ("positional", 2) => Some(Some(spec_ty)),
        // .help() → String  (render --help text)
        ("help", 0) => Some(Some(Type::String)),
        // .parse([String]) → ParsedArgs ? String
        ("parse", 1) => Some(Some(result_ty(
            Type::Named("ParsedArgs".to_string()),
            Type::String,
        ))),
        // Arity mismatches
        ("flag", _) => {
            diags.push(Diagnostic::error(
                "E1301",
                format!("`flag` expects 2 arguments (name, help), got {}", n_args),
                "`ArgsSpec.flag(name, help)` registers a boolean flag like `--verbose`".to_string(),
                "pass exactly two strings: the flag name and a help description".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("option", _) => {
            diags.push(Diagnostic::error(
                "E1302",
                format!("`option` expects 3 arguments (name, help, metavar), got {}", n_args),
                "`ArgsSpec.option(name, help, metavar)` registers a value option like `--output FILE`".to_string(),
                "pass three strings: the option name, a help description, and a metavar like `FILE`".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("positional", _) => {
            diags.push(Diagnostic::error(
                "E1303",
                format!("`positional` expects 2 arguments (name, help), got {}", n_args),
                "`ArgsSpec.positional(name, help)` registers a required positional argument".to_string(),
                "pass exactly two strings: the positional name and a help description".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("parse", _) => {
            diags.push(Diagnostic::error(
                "E1304",
                format!("`parse` expects 1 argument (argv), got {}", n_args),
                "`ArgsSpec.parse(argv)` parses a `[String]` (from `io.args()`) against the spec".to_string(),
                "pass exactly one argument: the argv list, e.g. `io.args()`".to_string(),
                Some(span),
            ));
            Some(None)
        }
        _ => None,
    }
}

/// D-ARGS1: type-check a method call on `ParsedArgs`.
pub(crate) fn parsed_args_method_return(
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    match (method, n_args) {
        // .flag("name") → Bool
        ("flag", 1) => Some(Some(Type::Bool)),
        // .option("name") → String?
        ("option", 1) => Some(Some(Type::Option(Box::new(Type::String)))),
        // .positional(n) → String?
        ("positional", 1) => Some(Some(Type::Option(Box::new(Type::String)))),
        ("flag", _) => {
            diags.push(Diagnostic::error(
                "E1301",
                format!("`ParsedArgs.flag` expects 1 argument (flag name), got {}", n_args),
                "`parsed.flag(\"verbose\")` returns `true` when `--verbose` was passed".to_string(),
                "pass exactly one string: the flag name (without leading `--`)".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("option", _) => {
            diags.push(Diagnostic::error(
                "E1302",
                format!("`ParsedArgs.option` expects 1 argument (option name), got {}", n_args),
                "`parsed.option(\"output\")` returns the value of `--output VALUE`, or `null`".to_string(),
                "pass exactly one string: the option name (without leading `--`)".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("positional", _) => {
            diags.push(Diagnostic::error(
                "E1303",
                format!("`ParsedArgs.positional` expects 1 argument (index), got {}", n_args),
                "`parsed.positional(0)` returns the first positional argument, or `null`".to_string(),
                "pass exactly one Int: the zero-based positional argument index".to_string(),
                Some(span),
            ));
            Some(None)
        }
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
        "core.io" => &["args", "input", "read_all_input", "eprint", "stdin"],
        "core.env" => &["get", "set", "current_dir", "home_dir"],
        "core.process" => &["exit", "run"],
        "core.math" => &[
            "sqrt", "pow", "abs", "min", "max", "floor", "ceil", "round", "pi", "e", "clamp",
        ],
        // D-DET1: `rng` builds a deterministic injected RNG capability.
        "core.random" => &["int", "float", "pick", "shuffle", "seed", "rng"],
        // D-DET1: `clock` builds a deterministic injected Clock capability.
        // D-DET-CAPAPI: `ms`/`secs` mint a deterministic `Duration` value.
        "core.time" => &["now", "sleep", "start", "clock", "ms", "secs"],
        // D-ENC1: unified serialization. `core.encoding` is the library root (no direct
        // verbs — formats are submodules); each format submodule carries the verbs.
        "core.encoding" => &[],
        // D-JSONVERB1: `to_string`/`to_string_pretty` (compact/pretty); `parse` → dynamic
        // JSON value; `decode` → lenient typed decode (D-JSON3).
        "core.encoding.json" => &["parse", "decode", "to_string", "to_string_pretty"],
        // D-SERDE6: typed `decode<T>` rides every format submodule alongside `parse`.
        "core.encoding.csv" => &["parse", "decode", "to_string"],
        "core.encoding.toml" => &["parse", "decode", "to_string"],
        "core.encoding.yaml" => &["parse", "decode", "to_string"],
        // D-UUIDENC1=A: hex/base64 codecs and UUID generator.
        "core.encoding.hex" => &["encode", "decode"],
        "core.encoding.base64" => &["encode", "decode"],
        "core.uuid" => &["v4", "v7"],
        "core.mem" => &["Ptr", "from_addr", "volatile_read", "address_of",
                        "Arena", "Bump", "Pool", "Fixed"],
        // D-ALLOC-C (ratified 2026-06-19): wider allocator API bucket.
        "core.mem.alloc" => &["Arena", "Bump", "Pool", "Fixed"],
        "core.tasks" => &["spawn", "channel"],
        "core.files" => &["open", "create", "append"],
        "core.path" => &["join", "parent", "extension", "normalize"],
        // D-DEFER1 option B: scope-exit guard.
        "core.scope" => &["guard"],
        // D-ARGS1 (ratified 2026-06-22): declarative CLI arg parsing.
        "core.args" => &["spec"],
        // D-TERM1 (ratified 2026-06-22): terminal direct-input primitive.
        "core.term" => &["read_key"],
        "core" => &[],
        // D-CORENS1: ring packages — `core.*` is the canonical user-facing name;
        // `jet.*` is the legacy / internal dispatch key. Both spellings are accepted.
        "core.log" | "jet.log" => &["info", "warn", "error", "debug", "set_level", "set_trace_id", "setup"],
        "jet.time" => &["now", "format"],
        "core.crypto" | "jet.crypto" => &["sha256", "sha256_bytes"],
        // E2-M10: networking modules.
        "core.net" => &[
            "tcp_listen", "tcp_accept", "tcp_connect",
            "tcp_read", "tcp_write", "tcp_local_addr", "tcp_peer_addr", "set_timeout",
            "tcp_reply",
        ],
        "core.http" | "jet.http" => &["get", "post", "serve"],
        // D-REGEX1: linear-time regex ring package.
        "core.regex" | "jet.regex" => &[
            "is_match", "match", "find", "find_all", "replace", "replace_all", "split",
        ],
        // D-DEP-ARCHIVE1=A: archive ring package — gzip + zip + tar.
        "core.archive" | "jet.archive" => &[
            "gzip_compress", "gzip_decompress",
            "zip_compress", "zip_decompress",
            "tar_add", "tar_get", "tar_names_json",
        ],
        // D-DEP-DB1: SQLite ring package.
        "core.db" | "jet.db" => &["open", "open_memory", "close", "exec", "query_json"],
        // D-REACT1=B: opt-in reactive library — signals/derived/effects.
        "core.reactive" | "jet.reactive" => &["signal", "derived", "effect"],
        // D-HONESTNUM1=A: Measurement<T> constructor.
        "core.science.measurement" => &["from"],
        // D-PENDING1=B: Loadable<T,E> constructors.
        "core.async.loadable" => &["idle", "loading", "loaded", "failed"],
        // D-ADAPTFID1=A: adaptive fidelity signal.
        "core.perf" => &["fidelity", "set_fidelity"],
        // D-APPROX1=A: approximate sketch data structures.
        "core.sketch.hll" => &["new"],
        "core.sketch.tdigest" => &["new"],
        "core.sketch.cms" => &["new"],
        "core.sketch.reservoir" => &["new"],
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
            // D-TERM1: terminal I/O requires an OS terminal device.
            | "core.term"
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
        "core.term" => {
            "Terminal I/O requires an OS terminal device. Build without `--freestanding`."
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

/// E2411 (D-SERDE): a type used with an encoding verb can't be (de)serialized — it
/// holds something with no wire form (a closure, handle, …), or a user type that
/// hasn't opted in with `#[Codable]`/`#[Encode]`/`#[Decode]`.
pub(crate) fn e2411(ty: &str, encode: bool, span: Span) -> Diagnostic {
    let (verb, marker) = if encode {
        ("serialized", "`#[Codable]` or `#[Encode]`")
    } else {
        ("decoded", "`#[Codable]` or `#[Decode]`")
    };
    Diagnostic::error(
        "E2411",
        format!("{ty} can't be {verb}"),
        format!("only types that opt in (and their fields) have a wire form; {ty} does not"),
        format!("add {marker} to {ty}, or remove it from the encoded value"),
        Some(span),
    )
}

/// E2407: `#[Rename(...)]` needs a single string-literal wire key.
pub(crate) fn e2407(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2407",
        "`#[Rename(...)]` needs a string literal".to_string(),
        "the wire key is a constant string, e.g. `#[Rename(\"customer\")]`".to_string(),
        "pass one quoted string — `#[Rename(\"wire_name\")]`".to_string(),
        Some(span),
    )
}

/// E2408: `#[Flatten]` needs a field whose type is itself a Codable struct.
pub(crate) fn e2408(field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2408",
        format!("`#[Flatten]` on `{field}` needs a struct-typed field"),
        "flatten splices another struct's keys into this object, so the field must be a `#[Codable]` struct — not a primitive, list, or map".to_string(),
        format!("give `{field}` a `#[Codable]` struct type, or drop `#[Flatten]`"),
        Some(span),
    )
}

/// E2409: `#[RenameAll(...)]` names a casing style outside the closed menu.
pub(crate) fn e2409(style: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2409",
        format!("`#[RenameAll({style})]` isn't a known casing style"),
        "the wire-casing menu is `camel`, `snake`, `pascal`, `kebab`, `screaming`".to_string(),
        "pick one of `camel` / `snake` / `pascal` / `kebab` / `screaming`".to_string(),
        Some(span),
    )
}

/// D-SERDE9/10: `Name<args>` satisfies the serde `trait_name` when `Name` derives
/// it (or is imported/non-local, hence trusted) and every type arg at a
/// wire-reaching position satisfies it too (`elem_ok`). A phantom/skip-only param
/// position imposes no obligation, so `Id<Kind>` is fine for any `Kind`.
fn apply_serde_ok(
    name: &str,
    args: &[Type],
    reg: &TraitRegistry,
    trait_name: &str,
    elem_ok: &dyn Fn(&Type) -> bool,
) -> bool {
    let head_ok = !reg.local_types.contains(name) || reg.implements_trait(name, trait_name);
    if !head_ok {
        return false;
    }
    match reg.serde_wire_params.get(name) {
        // Local generic Codable type: only the wire-reaching args must be codable.
        Some(idxs) => idxs.iter().all(|&i| args.get(i).map_or(true, |t| elem_ok(t))),
        // No recorded wire params (imported/non-generic): trust every arg is fine
        // only if each is codable — be conservative and check them all.
        None => args.iter().all(|t| elem_ok(t)),
    }
}

fn is_encodable_ty(ty: &Type, reg: &TraitRegistry) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char
        | Type::IntN { .. } | Type::Float32 => true,
        Type::List(e) | Type::Option(e) | Type::Shared(e) => is_encodable_ty(e, reg),
        Type::FixedList { elem, .. } => is_encodable_ty(elem, reg),
        Type::Map { key, value } => matches!(**key, Type::String) && is_encodable_ty(value, reg),
        // A non-local type (imported) is trusted; a local one must derive Encode.
        Type::Named(n) => {
            is_json_type_name(n)
                || !reg.local_types.contains(n)
                || reg.implements_trait(n, crate::Generics::ENCODE)
        }
        Type::Apply { name, args } => {
            apply_serde_ok(name, args, reg, crate::Generics::ENCODE, &|t| {
                is_encodable_ty(t, reg)
            })
        }
        _ => false,
    }
}

fn is_decodable_ty(ty: &Type, reg: &TraitRegistry) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char
        | Type::IntN { .. } | Type::Float32 => true,
        Type::List(e) | Type::Option(e) | Type::Shared(e) => is_decodable_ty(e, reg),
        Type::FixedList { elem, .. } => is_decodable_ty(elem, reg),
        Type::Map { key, value } => matches!(**key, Type::String) && is_decodable_ty(value, reg),
        Type::Named(n) => {
            !reg.local_types.contains(n) || reg.implements_trait(n, crate::Generics::DECODE)
        }
        Type::Apply { name, args } => {
            apply_serde_ok(name, args, reg, crate::Generics::DECODE, &|t| {
                is_decodable_ty(t, reg)
            })
        }
        _ => false,
    }
}

/// True for a `#[Flatten]`-able field type: a named struct (not a primitive/list/map).
fn is_struct_named(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if !is_json_type_name(n))
}

fn marker_is_string_literal(m: &crate::AST::Marker) -> bool {
    matches!(
        m.args.first(),
        Some(Expr::Str(parts, _)) if parts.len() == 1 && matches!(parts[0], crate::AST::StrPart::Lit(_))
    )
}

/// D-SERDE: validate serde markers on every `#[Codable]`/`#[Encode]`/`#[Decode]` type
/// (E2407–E2412). Runs after the trait registry is built so field types resolve. This
/// keeps generated code rustc-clean (I2): a field with no wire form is caught here, not
/// by rustc on the emitted `impl`.
pub(crate) fn validate_serde_items(items: &[crate::AST::Item], reg: &TraitRegistry) -> Vec<Diagnostic> {
    use crate::AST::Item;
    let mut out = Vec::new();
    for item in items {
        let (derives, container): (&[(String, Span)], &[crate::AST::Marker]) = match item {
            Item::Struct(s) => (&s.derives, &s.serde_markers),
            Item::Enum(e) => (&e.derives, &e.serde_markers),
            _ => continue,
        };
        let enc = derives.iter().any(|(t, _)| t == crate::Generics::ENCODE);
        let dec = derives.iter().any(|(t, _)| t == crate::Generics::DECODE);
        if !enc && !dec {
            continue;
        }
        // D-SERDE12: generic `#[Codable]` is first-class — no `type_params > 0`
        // gate. The per-field checks below run on generic types unchanged; a type
        // param `T` reads as a non-local `Type::Named`, so it's trusted here and
        // the codability obligation falls on the use site (E0905).
        // Container `#[RenameAll(style)]` casing menu (E2409).
        for m in container {
            if m.name == Syntax::ATTR_RENAME_ALL {
                match m.args.first() {
                    Some(Expr::Ident(style, sp)) => {
                        if !matches!(
                            style.as_str(),
                            Syntax::RENAME_ALL_CAMEL
                                | Syntax::RENAME_ALL_SNAKE
                                | Syntax::RENAME_ALL_PASCAL
                                | Syntax::RENAME_ALL_KEBAB
                                | Syntax::RENAME_ALL_SCREAMING
                        ) {
                            out.push(e2409(style, *sp));
                        }
                    }
                    _ => out.push(e2409("?", m.span)),
                }
            }
        }
        if let Item::Struct(s) = item {
            for f in &s.fields {
                let skip = f.serde_markers.iter().any(|m| m.name == Syntax::ATTR_SKIP);
                let flatten = f.serde_markers.iter().any(|m| m.name == Syntax::ATTR_FLATTEN);
                for m in &f.serde_markers {
                    // E2407: `#[Rename]` needs a string literal.
                    if m.name == Syntax::ATTR_RENAME && !marker_is_string_literal(m) {
                        out.push(e2407(m.span));
                    }
                }
                if flatten && !is_struct_named(&f.ty) {
                    out.push(e2408(&f.name, f.name_span));
                    continue;
                }
                if skip || flatten {
                    continue;
                }
                // E2411: every encoded/decoded field must have a wire form.
                if enc && !is_encodable_ty(&f.ty, reg) {
                    out.push(e2411(&f.ty.show(), true, f.name_span));
                }
                if dec && !is_decodable_ty(&f.ty, reg) {
                    out.push(e2411(&f.ty.show(), false, f.name_span));
                }
            }
        }
        if let Item::Enum(e) = item {
            for v in &e.variants {
                for m in &v.serde_markers {
                    if m.name == Syntax::ATTR_RENAME && !marker_is_string_literal(m) {
                        out.push(e2407(m.span));
                    }
                }
                let tys: Vec<&Type> = match &v.payload {
                    crate::AST::VariantPayload::Unit => vec![],
                    crate::AST::VariantPayload::Single(t, _) => vec![t],
                    crate::AST::VariantPayload::Named(fs) => fs.iter().map(|f| &f.ty).collect(),
                };
                for t in tys {
                    if enc && !is_encodable_ty(t, reg) {
                        out.push(e2411(&t.show(), true, v.name_span));
                    }
                    if dec && !is_decodable_ty(t, reg) {
                        out.push(e2411(&t.show(), false, v.name_span));
                    }
                }
            }
        }
    }
    out
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

/// D-REACT1=B (E2910): a `reactive.derived`/`effect` argument that isn't a lambda.
pub(crate) fn reactive_not_lambda(kind: &str, got: &Type, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2910",
        format!("`reactive.{kind}` needs a lambda, not {}", got.show()),
        format!(
            "a reactive {} is built from a `() => …` body so it can re-run when a signal changes",
            kind
        ),
        format!("write `reactive.{kind}(() => {{ … }})`"),
        Some(span),
    )
}

/// D-REACT1=B (E2911): a `reactive.derived`/`effect` lambda that takes parameters.
pub(crate) fn reactive_lambda_arity(kind: &str, n: usize, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2911",
        format!(
            "`reactive.{kind}` needs a zero-parameter lambda, got {} parameter{}",
            n,
            if n == 1 { "" } else { "s" }
        ),
        "the body takes no arguments — it reads the signals it depends on via `.get()`".to_string(),
        format!("write `reactive.{kind}(() => {{ … }})` with no parameters"),
        Some(span),
    )
}

/// D-REACT1=B (E2912): a `reactive.derived` whose lambda returns nothing.
pub(crate) fn reactive_derived_unit(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2912",
        "`reactive.derived` must compute and return a value".to_string(),
        "a derived value is recomputed from its signals; its lambda has to return the value".to_string(),
        "return a value from the body, or use `reactive.effect(() => { … })` for a side effect".to_string(),
        Some(span),
    )
}

/// D-REACT1=B (E2913): a reactive value type the library can't hold (e.g. a function).
pub(crate) fn reactive_bad_value_type(kind: &str, ty: &Type, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2913",
        format!("a reactive {} can't hold a {}", kind, ty.show()),
        "signals and derived values hold ordinary data so they can be copied to dependents".to_string(),
        "use a data value (number, text, list, struct, …); wrap behaviour in `reactive.effect` instead".to_string(),
        Some(span),
    )
}

/// D-NUMOPS1 (E1005): a `wrapping`/`saturating`/`checked` opt-in wasn't given a
/// single integer `+`/`-`/`*`/`/` to wrap.
pub(crate) fn overflow_opt_in_error(kind: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1005",
        format!("`{kind}` must wrap a single integer `+`, `-`, `*`, or `/`"),
        "the overflow opt-ins apply to one arithmetic operation on whole numbers".to_string(),
        format!("write it around one operation, e.g. `{kind}(a + b)`"),
        Some(span),
    )
}

/// D-NUMOPS1: the type of a numeric type-constant — `MIN`/`MAX` on any numeric
/// type, `INFINITY`/`NAN`/`EPSILON` on floats. `None` if `member` isn't one.
pub(crate) fn numeric_const_type(nt: &Type, member: &str) -> Option<Type> {
    match member {
        "MIN" | "MAX" => Some(nt.clone()),
        "INFINITY" | "NEG_INFINITY" | "NAN" | "EPSILON" if nt.is_float() => Some(nt.clone()),
        _ => None,
    }
}

/// D-SG9/D-NUMOPS1 (E1003): an integer literal doesn't fit its fixed-width type.
/// `U8` keeps its byte-framed wording; other widths get the general range message.
pub(crate) fn int_range_error(signed: bool, bits: u8, span: Span) -> Diagnostic {
    let (lo, hi) = crate::AST::int_range(signed, bits);
    let spelling = crate::AST::int_spelling(signed, bits);
    // "an I8" (the letter I reads as a vowel) vs "a U8".
    let article = if signed { "an" } else { "a" };
    let why = if !signed && bits == 8 {
        "binary APIs use one byte per value".to_string()
    } else {
        format!("{article} {spelling} is a fixed-width number — values outside its range can't fit")
    };
    Diagnostic::error(
        "E1003",
        format!("{article} {spelling} holds {lo}..{hi}"),
        why,
        format!("use a number from {lo} through {hi}"),
        Some(span),
    )
}

/// D-SERDE-ACCESS=B: accessor methods on `DataTree`.
/// `.field(name)` → `DataTree ? String`
/// `.at(i)` → `DataTree ? String`
/// `.int()` → `Int ? String`
/// `.text()` → `String ? String`
/// `.bool()` → `Bool ? String`
/// `.float()` → `Float ? String`
pub fn datatree_method_return(method: &str, n_args: usize) -> Option<Type> {
    match (method, n_args) {
        ("field", 1) => Some(Type::Result {
            ok: Box::new(Type::Named("DataTree".to_string())),
            err: Box::new(Type::String),
        }),
        ("at", 1) => Some(Type::Result {
            ok: Box::new(Type::Named("DataTree".to_string())),
            err: Box::new(Type::String),
        }),
        ("int", 0) => Some(Type::Result {
            ok: Box::new(Type::Int),
            err: Box::new(Type::String),
        }),
        ("text", 0) => Some(Type::Result {
            ok: Box::new(Type::String),
            err: Box::new(Type::String),
        }),
        ("bool", 0) => Some(Type::Result {
            ok: Box::new(Type::Bool),
            err: Box::new(Type::String),
        }),
        ("float", 0) => Some(Type::Result {
            ok: Box::new(Type::Float),
            err: Box::new(Type::String),
        }),
        _ => None,
    }
}
