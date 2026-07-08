use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Effects::Effect;
use crate::Syntax;
use crate::AST::{AccessConvention, Expr, Type};

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
                format!(
                    "`{}` is not defined in module `{}`",
                    &mangled[alias.len() + 2..],
                    alias
                ),
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
        if !self.func_pub.get(mangled).copied().unwrap_or(false)
            && !self.func_pkg_pub.get(mangled).copied().unwrap_or(false)
        {
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
                format!(
                    "check the definition of `{}` in module `{}`",
                    &mangled[alias.len() + 2..],
                    alias
                ),
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
            let is_pub = target.func_pub.get(name).copied().unwrap_or(false)
                || (self.same_package_scope(mod_idx)
                    && target.func_pkg_pub.get(name).copied().unwrap_or(false));
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
                                    Syntax::SIGIL_WRITE
                                ),
                                format!(
                                    "`{}` needs to edit (`&`) this value; passing it without `{}` grants only read access",
                                    name,
                                    Syntax::SIGIL_WRITE
                                ),
                                format!(
                                    "write `{}{}` when calling `{}`",
                                    Syntax::SIGIL_WRITE,
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
            let is_pub = self.type_is_pub_in(mod_idx, name);
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
            ("core.math", "pi" | "e" | "tau" | "infinity" | "nan") => Some(Type::Float),
            // D-ALLOC1/D-ALLOC-C (ratified 2026-06-19): `mem.Arena`, `mem.Bump`,
            // `mem.Pool`, `mem.Fixed` — accessed as a field on the `core.mem` alias,
            // then `.new()` is called on the sentinel type to construct the allocator.
            ("core.mem", "Arena") => Some(Type::Named(Syntax::MEM_ARENA.to_string())),
            ("core.mem", "Bump") => Some(Type::Named(Syntax::MEM_BUMP.to_string())),
            ("core.mem", "Pool") => Some(Type::Named(Syntax::MEM_POOL.to_string())),
            ("core.mem", "Fixed") => Some(Type::Named(Syntax::MEM_FIXED.to_string())),
            // D-OPTGC1: `gc.Gc` sentinel — `.new<T>(value)` constructs a traced handle.
            ("core.gc", "Gc") => Some(Type::Named(Syntax::GC_TYPE.to_string())),
            // D-SOLVER-LIB1=A: `solve.Solver.new(seed)` constructs explicit solver state.
            ("core.solve", "Solver") => Some(Type::Named(Syntax::SOLVER_TYPE.to_string())),
            // D-GAME1/2/3 + D-WD10: static sentinels for `game.Scene.new`,
            // `game.Replay.record`, `game.Backend.headless`, and `game.Budgets.new`.
            ("core.game", "Scene") => Some(Type::Named("GameSceneType".to_string())),
            ("core.game", "Replay") => Some(Type::Named("GameReplayType".to_string())),
            ("core.game", "Backend") => Some(Type::Named("GameBackendType".to_string())),
            ("core.game", "Budgets") => Some(Type::Named("GameBudgetsType".to_string())),
            // D-FIDELITY-API1=A: `core.perf.Perf` static API sentinel.
            ("core.perf", "Perf") => Some(Type::Named("Perf".to_string())),
            _ => {
                self.diags.push(unknown_core_item(module, name, span));
                let _ = alias_span;
                None
            }
        }
    }

    /// S58 (E2-M13): `alias.Ptr<T>.from_addr(addr)`. Gated by `use core.mem`
    /// (E3102) and an enclosing `#Unsafe` block (E3101). Returns `Ptr<T>`.
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
                    format!(
                        "`{}` needs an Int address, not {}",
                        Syntax::MEM_FROM_ADDR,
                        t.show()
                    ),
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
                Syntax::TYPE_PTR,
                Syntax::MEM_VOLATILE_READ
            ),
            format!(
                "add `use {};` and call through `{}.…`",
                Syntax::CORE_MEM_MODULE,
                alias
            ),
            Some(span),
        )
    }

    /// D-SERDE: a value type the `@[Codable]`/`@[Encode]` derive (or a blanket impl)
    /// can serialize. Primitives, the dynamic `Json` tree, and lists/options/maps of
    /// encodables qualify; a user type must derive `Encode`.
    fn is_encodable(&self, t: &Type) -> bool {
        match t {
            Type::Int
            | Type::Float
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::IntN { .. }
            | Type::Float32 => true,
            Type::List(e) | Type::Option(e) | Type::Shared(e) => self.is_encodable(e),
            Type::FixedList { elem, .. } => self.is_encodable(elem),
            Type::Map { key, value } => matches!(**key, Type::String) && self.is_encodable(value),
            Type::Named(n) => {
                is_json_type_name(n) || self.trait_reg.implements_trait(n, crate::Generics::ENCODE)
            }
            // D-SERDE9/10: a generic instantiation `Name<args>` is encodable when
            // `Name` derives Encode and every type arg that reaches the wire is
            // itself encodable. Phantom/skip-only params impose no obligation.
            Type::Apply { name, args } => {
                apply_serde_ok(name, args, self.trait_reg, crate::Generics::ENCODE, &|t| {
                    self.is_encodable(t)
                })
            }
            _ => false,
        }
    }

    /// D-SERDE: a type `decode<T>` can construct. Mirrors [`Self::is_encodable`] but a
    /// user type must derive `Decode` (the dynamic `Json` tree is reached by bare
    /// `decode`, not the typed path).
    fn is_decodable(&self, t: &Type) -> bool {
        match t {
            Type::Int
            | Type::Float
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::IntN { .. }
            | Type::Float32 => true,
            Type::List(e) | Type::Option(e) | Type::Shared(e) => self.is_decodable(e),
            Type::FixedList { elem, .. } => self.is_decodable(elem),
            Type::Map { key, value } => matches!(**key, Type::String) && self.is_decodable(value),
            Type::Named(n) => self.trait_reg.implements_trait(n, crate::Generics::DECODE),
            Type::Apply { name, args } => {
                apply_serde_ok(name, args, self.trait_reg, crate::Generics::DECODE, &|t| {
                    self.is_decodable(t)
                })
            }
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
            // D-EFFTREE1: Core calls stay tagged with a bare root — real
            // stdlib call sites are unchanged (no migration break: existing
            // diagnostics naming `Fs`/`Db`/… keep their exact wording). Leaf
            // precision (`Fs.Read`, `Db.Write`, …) is a user-declared-contract
            // concept (a function's own `#(…)` bound, D-PROP1-seeded into its
            // `direct` set) — see Registration.rs / Bundle.rs.
            self.record_effect(e.name());
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
            self.diags
                .push(e3401(&self.fn_name.clone(), &api, &[], span));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return core_fixed_sig(module, name).and_then(|(_, ret)| ret);
        }
        // D-EFF1: `@Pure` is the empty effect set, so any effectful Core call —
        // `Fs`/`Net`/`Env`/`Exec`/`Db`/`Log`/`Io` — is impure inside a `@Pure fn`.
        // (Time/Rand return early above via E3403; stdin via the E3401 check
        // above, so this catches the remaining effect-carrying Core modules.)
        if self.in_pure && self.det_suppress == 0 && core_effect(module, name).is_some() {
            let api = format!("{}.{}", module_short_name(module), name);
            self.diags
                .push(e3401(&self.fn_name.clone(), &api, &[], span));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return core_fixed_sig(module, name).and_then(|(_, ret)| ret);
        }
        // D-A11YGATE1=B (c134 Phase 6): E2930 (empty accessible label on an
        // interactive-role node) is checked here, on the raw call-site args,
        // independent of `sig`/arity checking below. It always runs — the
        // diagnostic is `Severity::Lint`, and CLI layers decide whether to
        // show it (`jet lint --a11y`) or suppress it (`jet build`/`jet run`),
        // per D-A11YGATE1's opt-in-surface, never-blocking contract.
        if module == "core.ui" && name == "node_role" {
            self.check_a11y_node_role_label(args, span);
        }
        let sig = core_fixed_sig(module, name);
        match (module, name) {
            ("core.game", "run") => {
                if let Some(scene) = args.get_mut(0) {
                    self.check_game_run_scene_edit(&scene.expr);
                    self.expect_core_arg("run", 0, &Type::Named("GameScene".to_string()), scene);
                }
                match args.len() {
                    1 => {}
                    2 => {
                        let label = args[1].label.as_ref().map(|(l, _)| l.clone());
                        match label.as_deref() {
                            Some("backend") => self.expect_core_arg(
                                "run",
                                1,
                                &Type::Named("GameBackend".to_string()),
                                &mut args[1],
                            ),
                            Some("replay") | None => self.expect_core_arg(
                                "run",
                                1,
                                &Type::Named("GameReplay".to_string()),
                                &mut args[1],
                            ),
                            Some(label) => {
                                game_run_label_error(&mut self.diags, label, &args[1], 1, span);
                                self.infer(&mut args[1].expr);
                            }
                        }
                    }
                    3 => {
                        let label1 = args[1].label.as_ref().map(|(l, _)| l.clone());
                        match label1.as_deref() {
                            Some("replay") | None => self.expect_core_arg(
                                "run",
                                1,
                                &Type::Named("GameReplay".to_string()),
                                &mut args[1],
                            ),
                            Some(label) => {
                                game_run_label_error(&mut self.diags, label, &args[1], 1, span);
                                self.infer(&mut args[1].expr);
                            }
                        }
                        let label2 = args[2].label.as_ref().map(|(l, _)| l.clone());
                        match label2.as_deref() {
                            Some("backend") | None => self.expect_core_arg(
                                "run",
                                2,
                                &Type::Named("GameBackend".to_string()),
                                &mut args[2],
                            ),
                            Some(label) => {
                                game_run_label_error(&mut self.diags, label, &args[2], 2, span);
                                self.infer(&mut args[2].expr);
                            }
                        }
                    }
                    _ => {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!("`game.run` expects 1 to 3 arguments, got {}", args.len()),
                            "`game.run` accepts a scene plus optional replay and backend handles"
                                .to_string(),
                            "write `game.run(scene)`, `game.run(scene, replay: replay)`, or `game.run(scene, replay: replay, backend: backend)`".to_string(),
                            Some(span),
                        ));
                        for a in args.iter_mut().skip(1) {
                            self.infer(&mut a.expr);
                        }
                    }
                }
                return Some(Type::String);
            }
            // D-ENC1 / D-SERDE6: typed encode/decode over the Encode/Decode model.
            // `to_string`/`to_string_pretty` accept any encodable value (the dynamic
            // `Json` / `[[String]]` / `Map` forms AND a `@[Codable]` value); the
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
            // D-MIGRATE3=A: `decode_traced<T>` — the one extra opt-in method beside
            // `decode` on every codec that shares the decode machinery. Same target
            // typing as `decode`, wrapped in `DecodeResult<T>` (`DecodeResult<[T]>`
            // for CSV) so the caller can ask `.migration.migrated` without `decode`
            // itself changing shape or cost (I8).
            (
                "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
                | "core.encoding.yaml",
                "decode_traced",
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
                let decode_result = Type::Apply {
                    name: "DecodeResult".to_string(),
                    args: vec![inner],
                };
                return Some(result_ty(decode_result, decode_error_ty()));
            }
            // D-DATA-SURFACE1=A: the beginner facade reuses typed CSV decoding,
            // then keeps table/stat selectors as ordinary typed Jet lambdas.
            ("core.data", "csv") if !type_args.is_empty() => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                for a in args.iter_mut() {
                    self.expect_core_arg(name, 0, &Type::String, a);
                }
                let t = type_args[0].clone();
                self.check_decodable(&t, span);
                return Some(result_ty(Type::List(Box::new(t)), decode_error_ty()));
            }
            ("core.data", "count") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Int);
                };
                let ty = self.infer(&mut arg.expr)?;
                let countable = match &ty {
                    Type::List(_) => true,
                    Type::Apply { name, .. } => {
                        matches!(name.as_str(), "Table" | "Series" | "LazyFrame")
                    }
                    _ => false,
                };
                if !countable {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`data.count` needs a typed table or series, not {}",
                            ty.show()
                        ),
                        "core.data counts rows from a list-backed table or series".to_string(),
                        "pass a `[T]` value, such as `data.csv<Row>(text)?`".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
                return Some(Type::Int);
            }
            ("core.data", "table" | "series") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(rows_arg) = args.get_mut(0) else {
                    return Some(Type::Apply {
                        name: if name == "table" { "Table" } else { "Series" }.to_string(),
                        args: vec![Type::Int],
                    });
                };
                let ty = self.infer(&mut rows_arg.expr);
                let elem = match ty {
                    Some(Type::List(inner)) => *inner,
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`data.{}` needs a list-backed value, not {}", name, other.show()),
                            "core.data tables and series are built from typed lists".to_string(),
                            "pass `[Row]` to `data.table` or `[T]` to `data.series`".to_string(),
                            Some(rows_arg.expr.span()),
                        ));
                        Type::Int
                    }
                    None => Type::Int,
                };
                return Some(Type::Apply {
                    name: if name == "table" { "Table" } else { "Series" }.to_string(),
                    args: vec![elem],
                });
            }
            ("core.data", "rows" | "values") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::List(Box::new(Type::Int)));
                };
                let want = if name == "rows" { "Table" } else { "Series" };
                let ty = self.infer(&mut arg.expr);
                let elem = match ty {
                    Some(Type::Apply { name: head, args }) if head == want && args.len() == 1 => {
                        args[0].clone()
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`data.{}` needs a `{}` value, not {}", name, want, other.show()),
                            "core.data unwraps typed table/series containers through explicit helpers".to_string(),
                            format!("pass a `{want}<T>` value"),
                            Some(arg.expr.span()),
                        ));
                        Type::Int
                    }
                    None => Type::Int,
                };
                return Some(Type::List(Box::new(elem)));
            }
            ("core.data", "missing_count") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Int);
                };
                let ty = self.infer(&mut arg.expr);
                if !matches!(&ty, Some(Type::Apply { name: head, args }) if head == "Series" && args.len() == 1) {
                    let shown = ty.map(|t| t.show()).unwrap_or_else(|| "<unknown>".to_string());
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`data.missing_count` needs a `Series<T?>`, not {}", shown),
                        "missing values are represented by Jet optionals in a typed series".to_string(),
                        "build a series from `[T?]` values with `data.series(values)`".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
                return Some(Type::Int);
            }
            ("core.data", "lazy") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Apply {
                        name: "LazyFrame".to_string(),
                        args: vec![Type::Int],
                    });
                };
                let ty = self.infer(&mut arg.expr);
                let elem = match ty {
                    Some(Type::Apply { name: head, args }) if head == "Table" && args.len() == 1 => {
                        args[0].clone()
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`data.lazy` needs a `Table<T>`, not {}", other.show()),
                            "lazy plans start from the same typed table model as eager helpers".to_string(),
                            "wrap rows with `data.table(rows)` first".to_string(),
                            Some(arg.expr.span()),
                        ));
                        Type::Int
                    }
                    None => Type::Int,
                };
                return Some(Type::Apply {
                    name: "LazyFrame".to_string(),
                    args: vec![elem],
                });
            }
            ("core.data", "collect" | "plan") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(if name == "collect" {
                        Type::Apply {
                            name: "Table".to_string(),
                            args: vec![Type::Int],
                        }
                    } else {
                        Type::List(Box::new(Type::String))
                    });
                };
                let ty = self.infer(&mut arg.expr);
                let elem = match ty {
                    Some(Type::Apply { name: head, args }) if head == "LazyFrame" && args.len() == 1 => {
                        args[0].clone()
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`data.{}` needs a `LazyFrame<T>`, not {}", name, other.show()),
                            "lazy plan inspection and collection operate on core.data lazy frames".to_string(),
                            "call `data.lazy(table)` first".to_string(),
                            Some(arg.expr.span()),
                        ));
                        Type::Int
                    }
                    None => Type::Int,
                };
                return Some(if name == "collect" {
                    Type::Apply {
                        name: "Table".to_string(),
                        args: vec![elem],
                    }
                } else {
                    Type::List(Box::new(Type::String))
                });
            }
            ("core.data", "lazy_filter" | "lazy_sort_by") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                let Some(frame_arg) = args.get_mut(0) else {
                    return Some(Type::Apply {
                        name: "LazyFrame".to_string(),
                        args: vec![Type::Int],
                    });
                };
                let frame_ty = self.infer(&mut frame_arg.expr);
                let row_ty = match frame_ty {
                    Some(Type::Apply { name: head, args }) if head == "LazyFrame" && args.len() == 1 => {
                        args[0].clone()
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`data.{}` needs a `LazyFrame<T>`, not {}", name, other.show()),
                            "lazy table operations keep a typed row model through the plan".to_string(),
                            "call `data.lazy(table)` first".to_string(),
                            Some(frame_arg.expr.span()),
                        ));
                        Type::Int
                    }
                    None => Type::Int,
                };
                if let Some(fn_arg) = args.get_mut(1) {
                    let ret = if name == "lazy_filter" {
                        Type::Bool
                    } else {
                        Type::String
                    };
                    let fn_ty = Type::Fn {
                        params: vec![row_ty.clone()],
                        ret: Some(Box::new(ret)),
                        effect_bound: None,
                    };
                    self.expect_core_arg(name, 1, &fn_ty, fn_arg);
                }
                return Some(Type::Apply {
                    name: "LazyFrame".to_string(),
                    args: vec![row_ty],
                });
            }
            ("core.data", "filter" | "sort_by") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                let Some(rows_arg) = args.get_mut(0) else {
                    return Some(Type::List(Box::new(Type::Int)));
                };
                let rows_ty = self.infer(&mut rows_arg.expr);
                let row_ty = match rows_ty {
                    Some(Type::List(inner)) => *inner,
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`data.{}` needs a typed table, not {}", name, other.show()),
                            "core.data pipelines rows from a list-backed typed table".to_string(),
                            "pass a `[Row]` value, such as `data.csv<Row>(text)?`".to_string(),
                            Some(rows_arg.expr.span()),
                        ));
                        Type::Int
                    }
                    None => Type::Int,
                };
                if let Some(fn_arg) = args.get_mut(1) {
                    let ret = if name == "filter" {
                        Type::Bool
                    } else {
                        Type::String
                    };
                    let fn_ty = Type::Fn {
                        params: vec![row_ty.clone()],
                        ret: Some(Box::new(ret)),
                        effect_bound: None,
                    };
                    self.expect_core_arg(name, 1, &fn_ty, fn_arg);
                }
                return Some(Type::List(Box::new(row_ty)));
            }
            ("core.data", "group_count" | "group_sum" | "group_mean") => {
                let want = if name == "group_count" { 2 } else { 3 };
                if args.len() != want {
                    self.diags
                        .push(wrong_core_arity(name, want, args.len(), span));
                }
                let Some(rows_arg) = args.get_mut(0) else {
                    return Some(Type::List(Box::new(Type::Named("DataGroup".to_string()))));
                };
                let rows_ty = self.infer(&mut rows_arg.expr);
                let row_ty = match rows_ty {
                    Some(Type::List(inner)) => *inner,
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`data.{}` needs a typed table, not {}", name, other.show()),
                            "core.data groups rows from a list-backed typed table".to_string(),
                            "pass a `[Row]` value, such as `data.csv<Row>(text)?`".to_string(),
                            Some(rows_arg.expr.span()),
                        ));
                        Type::Int
                    }
                    None => Type::Int,
                };
                if let Some(key_arg) = args.get_mut(1) {
                    let key_fn = Type::Fn {
                        params: vec![row_ty.clone()],
                        ret: Some(Box::new(Type::String)),
                        effect_bound: None,
                    };
                    self.expect_core_arg(name, 1, &key_fn, key_arg);
                }
                if name != "group_count" {
                    if let Some(value_arg) = args.get_mut(2) {
                        let value_fn = Type::Fn {
                            params: vec![row_ty],
                            ret: Some(Box::new(Type::Float)),
                            effect_bound: None,
                        };
                        self.expect_core_arg(name, 2, &value_fn, value_arg);
                    }
                }
                return Some(Type::List(Box::new(Type::Named("DataGroup".to_string()))));
            }
            ("core.data", "inner_join" | "left_join") => {
                if args.len() != 4 {
                    self.diags.push(wrong_core_arity(name, 4, args.len(), span));
                }
                let left_ty = args.get_mut(0).and_then(|a| self.infer(&mut a.expr));
                let right_ty = args.get_mut(1).and_then(|a| self.infer(&mut a.expr));
                let left_row = match left_ty {
                    Some(Type::List(inner)) => *inner,
                    Some(other) => {
                        if let Some(arg) = args.get(0) {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`data.{}` needs a typed left table, not {}", name, other.show()),
                                "core.data joins rows from list-backed typed tables".to_string(),
                                "pass `[LeftRow]` and `[RightRow]` values".to_string(),
                                Some(arg.expr.span()),
                            ));
                        }
                        Type::Int
                    }
                    None => Type::Int,
                };
                let right_row = match right_ty {
                    Some(Type::List(inner)) => *inner,
                    Some(other) => {
                        if let Some(arg) = args.get(1) {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`data.{}` needs a typed right table, not {}", name, other.show()),
                                "core.data joins rows from list-backed typed tables".to_string(),
                                "pass `[LeftRow]` and `[RightRow]` values".to_string(),
                                Some(arg.expr.span()),
                            ));
                        }
                        Type::Int
                    }
                    None => Type::Int,
                };
                if let Some(left_key) = args.get_mut(2) {
                    let key_fn = Type::Fn {
                        params: vec![left_row],
                        ret: Some(Box::new(Type::String)),
                        effect_bound: None,
                    };
                    self.expect_core_arg(name, 2, &key_fn, left_key);
                }
                if let Some(right_key) = args.get_mut(3) {
                    let key_fn = Type::Fn {
                        params: vec![right_row],
                        ret: Some(Box::new(Type::String)),
                        effect_bound: None,
                    };
                    self.expect_core_arg(name, 3, &key_fn, right_key);
                }
                return Some(Type::List(Box::new(Type::Named("DataGroup".to_string()))));
            }
            ("core.data", "pivot_sum") => {
                if args.len() != 4 {
                    self.diags.push(wrong_core_arity(name, 4, args.len(), span));
                }
                let Some(rows_arg) = args.get_mut(0) else {
                    return Some(Type::List(Box::new(Type::Named("DataGroup".to_string()))));
                };
                let rows_ty = self.infer(&mut rows_arg.expr);
                let row_ty = match rows_ty {
                    Some(Type::List(inner)) => *inner,
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`data.{}` needs a typed table, not {}", name, other.show()),
                            "core.data pivots rows from a list-backed typed table".to_string(),
                            "pass a `[Row]` value, such as `data.csv<Row>(text)?`".to_string(),
                            Some(rows_arg.expr.span()),
                        ));
                        Type::Int
                    }
                    None => Type::Int,
                };
                for idx in [1usize, 2usize] {
                    if let Some(arg) = args.get_mut(idx) {
                        let key_fn = Type::Fn {
                            params: vec![row_ty.clone()],
                            ret: Some(Box::new(Type::String)),
                            effect_bound: None,
                        };
                        self.expect_core_arg(name, idx, &key_fn, arg);
                    }
                }
                if let Some(value_arg) = args.get_mut(3) {
                    let value_fn = Type::Fn {
                        params: vec![row_ty],
                        ret: Some(Box::new(Type::Float)),
                        effect_bound: None,
                    };
                    self.expect_core_arg(name, 3, &value_fn, value_arg);
                }
                return Some(Type::List(Box::new(Type::Named("DataGroup".to_string()))));
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
                            format!(
                                "`{}` needs a `Ptr<T>`, not {}",
                                Syntax::MEM_VOLATILE_READ,
                                t.show()
                            ),
                            "a volatile read reads through a typed pointer".to_string(),
                            "build a pointer first with `mem.Ptr<T>.from_addr(addr)`".to_string(),
                            Some(arg.expr.span()),
                        ));
                        None
                    }
                };
            }
            ("core.mem", "volatile_write") => {
                if !self.in_unsafe {
                    self.diags.push(e3101(Syntax::MEM_VOLATILE_WRITE, span));
                }
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                    return None;
                }
                let ptr_arg = args.get_mut(0)?;
                let ptr_ty = self.infer(&mut ptr_arg.expr)?;
                let Some(elem) = ptr_elem(&ptr_ty) else {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` needs a `Ptr<T>`, not {}",
                            Syntax::MEM_VOLATILE_WRITE,
                            ptr_ty.show()
                        ),
                        "a volatile write writes through a typed pointer".to_string(),
                        "build a pointer first with `mem.Ptr<T>.from_addr(addr)`".to_string(),
                        Some(ptr_arg.expr.span()),
                    ));
                    return None;
                };
                let value_arg = args.get_mut(1)?;
                if let Some(value_ty) = self.infer(&mut value_arg.expr) {
                    self.check_type_assignable(&elem, &value_ty, value_arg.expr.span());
                }
                return Some(unit_ty());
            }
            ("core.mem", "address_of") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    return None;
                }
                // Taking an address is inert (S58): legal outside `#Unsafe`.
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
            // D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)` — the runtime reflection
            // floor. Legal wherever `x` is interpolatable (`"{x}"`) — the SAME
            // gate a trait-bounded variadic's `...Renderable` bound uses
            // (`is_displayable`, reuse, I8), not the looser `is_printable`
            // `print`/`io.eprint` accept: `Value.display()` is backed by
            // `jet_display()` (JetDisplay), not `jet_show()`/`{:?}`, so it
            // shows exactly what `"{x}"` would — never codegen's mangled Rust
            // field names, which `is_printable` would let through for a
            // struct with no auto/explicit `Display`.
            ("core.reflect", "of") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Named("Value".to_string()));
                }
                let arg = &mut args[0];
                if let Some(ty) = self.infer(&mut arg.expr) {
                    if !is_displayable(&ty, self.trait_reg) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("{} can't be reflected yet", ty.show()),
                            "`reflect.of` inspects the same values `\"{x}\"` interpolation can show"
                                .to_string(),
                            "implement `Display` for its type, or pass one of its fields instead"
                                .to_string(),
                            Some(arg.expr.span()),
                        ));
                    }
                }
                return Some(Type::Named("Value".to_string()));
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
            (
                "core.math",
                "sqrt" | "floor" | "ceil" | "sin" | "cos" | "tan" | "asin" | "acos"
                | "atan" | "sinh" | "cosh" | "tanh" | "exp" | "ln" | "log2" | "log10"
                | "trunc" | "fract" | "degrees" | "radians",
            ) => {
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
                        "math functions in this family operate on floating-point numbers".to_string(),
                        "pass a Float or F32 value".to_string(),
                        Some(arg.expr.span()),
                    ));
                    return None;
                }
                return Some(ty);
            }
            ("core.math", "atan2" | "hypot" | "lerp") => {
                if args.len() != if name == "lerp" { 3 } else { 2 } {
                    self.diags.push(wrong_core_arity(
                        name,
                        if name == "lerp" { 3 } else { 2 },
                        args.len(),
                        span,
                    ));
                }
                let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                    for a in args.iter_mut().skip(1) {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Float);
                };
                if !matches!(first, Type::Float | Type::Float32) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`{}` needs Float or F32, not {}", name, first.show()),
                        "this math function operates on floating-point numbers".to_string(),
                        "pass Float or F32 values".to_string(),
                        Some(args[0].expr.span()),
                    ));
                    return None;
                }
                for i in 1..args.len() {
                    if let Some(got) = args.get_mut(i).and_then(|a| self.infer(&mut a.expr)) {
                        if got != first {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`{}` needs all arguments to have the same float type", name),
                                "D-FLOATW1: mixing float widths is not allowed".to_string(),
                                "convert the arguments to the same float width".to_string(),
                                Some(args[i].expr.span()),
                            ));
                        }
                    }
                }
                return Some(first);
            }
            ("core.math", "is_nan" | "is_inf" | "is_finite") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Bool);
                };
                let ty = self.infer(&mut arg.expr)?;
                if !matches!(ty, Type::Float | Type::Float32) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`{}` needs Float or F32, not {}", name, ty.show()),
                        "floating-point classification only applies to floats".to_string(),
                        "pass a Float or F32 value".to_string(),
                        Some(arg.expr.span()),
                    ));
                    return None;
                }
                return Some(Type::Bool);
            }
            ("core.math", "sign") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    let ty = self.infer(&mut arg.expr)?;
                    if !matches!(ty, Type::Float | Type::Float32) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`sign` needs Float or F32, not {}", ty.show()),
                            "sign classifies a floating-point value as negative, zero, or positive".to_string(),
                            "pass a Float or F32 value".to_string(),
                            Some(arg.expr.span()),
                        ));
                        return None;
                    }
                }
                return Some(Type::Int);
            }
            ("core.math", "to_bits") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                return Some(Type::Int);
            }
            ("core.math", "from_bits") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                return Some(Type::Float);
            }
            (
                "core.math",
                "checked_add" | "checked_sub" | "checked_mul" | "checked_pow",
            ) => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                for (idx, arg) in args.iter_mut().enumerate() {
                    self.expect_core_arg(name, idx, &Type::Int, arg);
                }
                return Some(Type::Option(Box::new(Type::Int)));
            }
            (
                "core.math",
                "saturating_add" | "saturating_sub" | "saturating_mul" | "wrapping_add"
                | "wrapping_sub" | "wrapping_mul" | "gcd" | "lcm" | "int_pow",
            ) => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                for (idx, arg) in args.iter_mut().enumerate() {
                    self.expect_core_arg(name, idx, &Type::Int, arg);
                }
                return Some(Type::Int);
            }
            ("core.math", "pow") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                    for a in args.iter_mut().skip(1) {
                        self.infer(&mut a.expr);
                    }
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
            ("core.random", "sample") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::List(Box::new(Type::Int)));
                };
                let arg_span = arg.expr.span();
                let ty = self.infer(&mut arg.expr)?;
                if let Some(k) = args.get_mut(1).and_then(|a| self.infer(&mut a.expr)) {
                    if k != Type::Int {
                        let k_span = args.get(1).map(|a| a.expr.span()).unwrap_or(span);
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`sample` count must be Int, not {}", k.show()),
                            "random.sample chooses up to k items without replacement".to_string(),
                            "pass an Int count".to_string(),
                            Some(k_span),
                        ));
                    }
                }
                if let Type::List(inner) = ty {
                    return Some(Type::List(inner));
                }
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`sample` needs a list, not {}", ty.show()),
                    "random.sample chooses items from a List".to_string(),
                    "pass a `[T]` value".to_string(),
                    Some(arg_span),
                ));
                return None;
            }
            ("core.random", "weighted_pick") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                let Some(items_arg) = args.get_mut(0) else {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Option(Box::new(Type::Int)));
                };
                let items_span = items_arg.expr.span();
                let items_ty = self.infer(&mut items_arg.expr)?;
                if let Some(weights_arg) = args.get_mut(1) {
                    let weights_ty = self.infer(&mut weights_arg.expr);
                    if weights_ty != Some(Type::List(Box::new(Type::Float))) {
                        if let Some(got) = weights_ty {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`weighted_pick` weights must be [Float], not {}", got.show()),
                                "random.weighted_pick pairs each item with a non-negative Float weight".to_string(),
                                "pass a `[Float]` weights list".to_string(),
                                Some(weights_arg.expr.span()),
                            ));
                        }
                    }
                }
                if let Type::List(inner) = items_ty {
                    return Some(Type::Option(inner));
                }
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`weighted_pick` needs a list, not {}", items_ty.show()),
                    "random.weighted_pick chooses one weighted item from a List".to_string(),
                    "pass a `[T]` value".to_string(),
                    Some(items_span),
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
                        "write access (`&`) is required; the list must be passed with `&`"
                            .to_string(),
                        "write `random.shuffle(&xs)`".to_string(),
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
                if let Some(problem) = self.sendability_problem(&t, false) {
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
            ("core.files", "read") => {}
            // D-TASKRUNTIME1=A: scheduler timer channels. `after(ms)` emits a unit tick;
            // `after(ms, value)` emits a typed timeout value that can join a select.
            ("core.tasks", "after") => {
                if !(args.len() == 1 || args.len() == 2) {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`tasks.after` takes one duration and an optional value, got {} argument{}",
                            args.len(),
                            if args.len() == 1 { "" } else { "s" }
                        ),
                        "a one-shot timer channel fires after a whole-millisecond delay".to_string(),
                        "write `tasks.after(ms: 100)` or `tasks.after(ms: 100, value: fallback)`".to_string(),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let ms_ty = self.infer(&mut args[0].expr)?;
                if !(matches!(ms_ty, Type::Int)
                    || matches!(ms_ty, Type::Named(ref n) if n == "Int" || n == "I64" || n == "I32"))
                {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`tasks.after(ms: …)` needs an integer millisecond count, not {}",
                            ms_ty.show()
                        ),
                        "timer channels use whole milliseconds".to_string(),
                        "write `tasks.after(ms: 100)`".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                let elem = if args.len() == 2 {
                    self.infer(&mut args[1].expr)?
                } else {
                    Type::Named("Unit".to_string())
                };
                if let Some(problem) = self.sendability_problem(&elem, false) {
                    self.report_unsendable(
                        "timer value",
                        &elem,
                        problem,
                        SendCrossing::ChannelSend,
                        args.get(1).map(|a| a.expr.span()).unwrap_or(span),
                    );
                }
                return Some(Type::Apply {
                    name: "Receiver".to_string(),
                    args: vec![elem],
                });
            }
            // D-TASKRUNTIME1=A: interval timer sends tick numbers (1, 2, ...).
            ("core.tasks", "interval") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("interval", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let ms_ty = self.infer(&mut args[0].expr)?;
                if !(matches!(ms_ty, Type::Int)
                    || matches!(ms_ty, Type::Named(ref n) if n == "Int" || n == "I64" || n == "I32"))
                {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`tasks.interval(ms: …)` needs an integer millisecond count, not {}",
                            ms_ty.show()
                        ),
                        "interval channels use whole milliseconds".to_string(),
                        "write `tasks.interval(ms: 1000)`".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                return Some(Type::Apply {
                    name: "Receiver".to_string(),
                    args: vec![Type::Int],
                });
            }
            // D-TUPLE-DESTRUCT1: `tasks.channel<T>()` returns the `(Sender<T>,
            // Receiver<T>)` pair directly — mirrors the turbofish `decode<T>` pattern
            // above (the element type `T` comes from the explicit call-site type
            // argument, not a binding annotation; there's no combined "Channel" value
            // to infer against anymore).
            ("core.tasks", "channel") => {
                if args.len() > 1 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`tasks.channel` takes an optional capacity, got {} arguments",
                            args.len()
                        ),
                        "a channel may be unbounded or have one whole-number backpressure bound"
                            .to_string(),
                        "write `tasks.channel<T>()` or `tasks.channel<T>(capacity: 1)`".to_string(),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                if let Some(cap) = args.get_mut(0) {
                    let cap_ty = self.infer(&mut cap.expr)?;
                    if !(matches!(cap_ty, Type::Int)
                        || matches!(cap_ty, Type::Named(ref n) if n == "Int" || n == "I64" || n == "I32"))
                    {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`tasks.channel<T>(capacity: …)` needs an integer capacity, not {}",
                                cap_ty.show()
                            ),
                            "bounded channels use a whole-number memory/backpressure limit"
                                .to_string(),
                            "write `tasks.channel<T>(capacity: 1)`".to_string(),
                            Some(cap.expr.span()),
                        ));
                    }
                }
                let Some(t) = type_args.first().cloned() else {
                    self.diags.push(Diagnostic::error(
                        "E0904",
                        "`tasks.channel` needs a type argument to infer the element type"
                            .to_string(),
                        "the element type `T` can't be guessed without `<T>`".to_string(),
                        "call it with an explicit type argument: `tasks.channel<T>()`".to_string(),
                        Some(span),
                    ));
                    return None;
                };
                return Some(Type::Tuple(vec![
                    (
                        "sender".to_string(),
                        Box::new(Type::Apply {
                            name: "Sender".to_string(),
                            args: vec![t.clone()],
                        }),
                    ),
                    (
                        "receiver".to_string(),
                        Box::new(Type::Apply {
                            name: "Receiver".to_string(),
                            args: vec![t],
                        }),
                    ),
                ]));
            }
            // D-ROUTE1=A: jet.http.router() → HttpRouter.
            ("jet.http", "router") => {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity("router", 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                }
                return Some(Type::Named("HttpRouter".to_string()));
            }
            // D-ROUTE1=A: http.parse(raw_string) → HttpRequest (parses HTTP/1.1 bytes).
            ("jet.http", "parse") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("parse", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("parse", 0, &Type::String, &mut args[0]);
                return Some(Type::Named("HttpRequest".to_string()));
            }
            // D-ROUTE1=A: http.dispatch(router, req) → HttpResponse.
            ("jet.http", "dispatch") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("dispatch", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
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
                                format!(
                                    "`http.dispatch` needs an HttpRequest, not {}",
                                    other.show()
                                ),
                                "parse the raw request with `http.parse(raw)`".to_string(),
                                "write `http.dispatch(router, req)` where `req` is an HttpRequest"
                                    .to_string(),
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
                    self.diags
                        .push(wrong_core_arity("serve", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
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
                    self.diags
                        .push(wrong_core_arity("guard", 1, args.len(), span));
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
                    self.diags
                        .push(wrong_core_arity("signal", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
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
                    self.diags
                        .push(wrong_core_arity("derived", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let lam_ty = self.infer(&mut args[0].expr);
                let elem = match &lam_ty {
                    Some(Type::Fn { params, ret, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(reactive_lambda_arity(
                                "derived",
                                params.len(),
                                args[0].expr.span(),
                            ));
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
                        self.diags
                            .push(reactive_not_lambda("derived", other, args[0].expr.span()));
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
            // D-SIGNAL1: `reactive.computed` is a canonical alias for `derived`.
            ("jet.reactive", "computed") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("computed", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let lam_ty = self.infer(&mut args[0].expr);
                let elem = match &lam_ty {
                    Some(Type::Fn { params, ret, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(reactive_lambda_arity(
                                "computed",
                                params.len(),
                                args[0].expr.span(),
                            ));
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
                        self.diags.push(reactive_not_lambda(
                            "computed",
                            other,
                            args[0].expr.span(),
                        ));
                        return None;
                    }
                    None => return None,
                };
                if !self.reactive_value_ok(&elem, args[0].expr.span(), "computed") {
                    return None;
                }
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_COMPUTED.to_string(),
                    args: vec![elem],
                });
            }
            // D-RENDERTGT2=A (c133 M2): `ui.reactive_render(() => { … })` — reactive
            // measure/layout/paint loop; re-runs when a signal read inside changes.
            ("core.ui", "reactive_render") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("reactive_render", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let lam_ty = self.infer(&mut args[0].expr);
                match &lam_ty {
                    Some(Type::Fn { params, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(reactive_lambda_arity(
                                "reactive_render",
                                params.len(),
                                args[0].expr.span(),
                            ));
                            return None;
                        }
                    }
                    Some(other) => {
                        self.diags.push(reactive_not_lambda(
                            "reactive_render",
                            other,
                            args[0].expr.span(),
                        ));
                        return None;
                    }
                    None => return None,
                }
                return None;
            }
            // D-REACT1=B: reactive.effect(() => { … }) runs the body now and again
            // whenever a signal it read changes. The body is a zero-parameter,
            // unit-returning closure; the call itself yields nothing.
            ("jet.reactive", "effect") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("effect", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let lam_ty = self.infer(&mut args[0].expr);
                match &lam_ty {
                    Some(Type::Fn { params, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(reactive_lambda_arity(
                                "effect",
                                params.len(),
                                args[0].expr.span(),
                            ));
                            return None;
                        }
                    }
                    Some(other) => {
                        self.diags
                            .push(reactive_not_lambda("effect", other, args[0].expr.span()));
                        return None;
                    }
                    None => return None,
                }
                return None; // effect returns nothing
            }
            // D-EVENT1=D: first-party typed Event/Hook family. Constructors are
            // module functions so the semantic family is one Core library surface,
            // not new syntax.
            ("core.event", "scope") => {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity("scope", 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                return Some(Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()));
            }
            ("core.event", "policy_sync") => {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity("policy_sync", 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                return Some(Type::Named(crate::Syntax::TYPE_EVENT_POLICY.to_string()));
            }
            ("core.event", "policy_async") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("policy_async", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("policy_async", 0, &Type::Int, &mut args[0]);
                return Some(Type::Named(crate::Syntax::TYPE_EVENT_POLICY.to_string()));
            }
            ("core.event", "new") => {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity("new", 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                if type_args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0904",
                        "`event.new` needs one payload type".to_string(),
                        "`Event<T>` carries exactly one typed payload for each emit".to_string(),
                        "call it with an explicit type argument: `event.new<Click>()`".to_string(),
                        Some(span),
                    ));
                    return None;
                }
                self.check_declared_type(&type_args[0], span);
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_EVENT.to_string(),
                    args: vec![type_args[0].clone()],
                });
            }
            ("core.event", "with_policy") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("with_policy", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                if type_args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0904",
                        "`event.with_policy` needs one payload type".to_string(),
                        "`Event<T>` carries exactly one typed payload for each emit".to_string(),
                        "call it with an explicit type argument: `event.with_policy<Click>(policy)`".to_string(),
                        Some(span),
                    ));
                    return None;
                }
                self.check_declared_type(&type_args[0], span);
                self.expect_core_arg(
                    "with_policy",
                    0,
                    &Type::Named(crate::Syntax::TYPE_EVENT_POLICY.to_string()),
                    &mut args[0],
                );
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_EVENT.to_string(),
                    args: vec![type_args[0].clone()],
                });
            }
            ("core.event", "hook") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("hook", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                if type_args.len() != 2 {
                    self.diags.push(Diagnostic::error(
                        "E0904",
                        "`event.hook` needs payload and result types".to_string(),
                        "`Hook<T, R>` receives a typed payload and combines handler results into one `R`".to_string(),
                        "call it with explicit type arguments: `event.hook<Request, Decision>(fallback)`".to_string(),
                        Some(span),
                    ));
                    return None;
                }
                self.check_declared_type(&type_args[0], span);
                self.check_declared_type(&type_args[1], span);
                self.expect_core_arg("hook", 0, &type_args[1], &mut args[0]);
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_HOOK.to_string(),
                    args: vec![type_args[0].clone(), type_args[1].clone()],
                });
            }
            // D-PENDING1=B: Loadable<T,E> constructors — idle/loading/loaded/failed.
            ("core.async.loadable", "idle") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![
                        Type::Named("Unknown".to_string()),
                        Type::Named("Unknown".to_string()),
                    ],
                });
            }
            ("core.async.loadable", "loading") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![
                        Type::Named("Unknown".to_string()),
                        Type::Named("Unknown".to_string()),
                    ],
                });
            }
            ("core.async.loadable", "loaded") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("loaded", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let val_ty = self
                    .infer(&mut args[0].expr)
                    .unwrap_or(Type::Named("Unknown".to_string()));
                return Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![val_ty, Type::Named("Unknown".to_string())],
                });
            }
            ("core.async.loadable", "failed") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("failed", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let err_ty = self
                    .infer(&mut args[0].expr)
                    .unwrap_or(Type::Named("Unknown".to_string()));
                return Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![Type::Named("Unknown".to_string()), err_ty],
                });
            }
            // D-TTLVAL1=A: Expiring<T> — value + TTL + injectable Clock.
            ("core.time.expiring", "new") => {
                if args.len() != 3 {
                    self.diags
                        .push(wrong_core_arity("new", 3, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let val_ty = self
                    .infer(&mut args[0].expr)
                    .unwrap_or(Type::Named("Unknown".to_string()));
                self.expect_core_arg(
                    "new",
                    1,
                    &Type::Named(crate::Syntax::DURATION_TYPE.to_string()),
                    &mut args[1],
                );
                self.expect_core_arg(
                    "new",
                    2,
                    &Type::Named(crate::Syntax::CLOCK_TYPE.to_string()),
                    &mut args[2],
                );
                return Some(Type::Apply {
                    name: "Expiring".to_string(),
                    args: vec![val_ty],
                });
            }
            // D-TTLVAL1=A: Rotting<T> — secret with TTL + zeroize on expiry.
            ("core.secrets", "rotting_new") => {
                if args.len() != 3 {
                    self.diags
                        .push(wrong_core_arity("rotting_new", 3, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let val_ty = self
                    .infer(&mut args[0].expr)
                    .unwrap_or(Type::Named("Unknown".to_string()));
                self.expect_core_arg(
                    "rotting_new",
                    1,
                    &Type::Named(crate::Syntax::DURATION_TYPE.to_string()),
                    &mut args[1],
                );
                self.expect_core_arg(
                    "rotting_new",
                    2,
                    &Type::Named(crate::Syntax::CLOCK_TYPE.to_string()),
                    &mut args[2],
                );
                return Some(Type::Apply {
                    name: "Rotting".to_string(),
                    args: vec![val_ty],
                });
            }
            // D-CRYPTOENV1=A: expert-only raw crypto — requires import + #Unsafe gate.
            ("core.crypto.expert", _) => {
                let has_import = self
                    .core_imports
                    .values()
                    .any(|m| m == "core.crypto.expert");
                if !has_import {
                    self.diags.push(Diagnostic::error(
                        "E0510",
                        format!("`core.crypto.expert.{name}` bypasses the misuse-resistant envelope"),
                        "raw AES/ChaCha primitives are expert-only and hide none of the footguns that `crypto.seal`/`open` prevent (D-CRYPTOENV1)".to_string(),
                        "use `core.crypto.seal` / `core.crypto.open` for encryption, or add `use core.crypto.expert` inside an audited `#Unsafe(\"reason\")` region".to_string(),
                        Some(span),
                    ));
                } else if !self.in_unsafe {
                    self.diags.push(Diagnostic::error(
                        "E0510",
                        format!("`core.crypto.expert.{name}` requires an audited `#Unsafe` region"),
                        "raw crypto primitives may only run inside an explicit expert-tier gate (I1)".to_string(),
                        "wrap the call in `#Unsafe(\"crypto expert: …\") { … }` or use `crypto.seal`/`open` instead".to_string(),
                        Some(span),
                    ));
                }
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return core_fixed_sig(module, name).and_then(|(_, ret)| ret);
            }
            // D-NETDEP1=A / D-HTTPLIB1=A: HTTP constructors.
            ("core.http.client", "get") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("get", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_url_arg("get", 0, &mut args[0]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("HttpClientResp".to_string())),
                    err: Box::new(Type::String),
                });
            }
            ("core.http.client", "post") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("post", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_url_arg("post", 0, &mut args[0]);
                self.expect_core_arg("post", 1, &Type::String, &mut args[1]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("HttpClientResp".to_string())),
                    err: Box::new(Type::String),
                });
            }
            ("core.http.client", "request") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("request", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("request", 0, &Type::String, &mut args[0]);
                self.expect_url_arg("request", 1, &mut args[1]);
                return Some(Type::Named("HttpClientReq".to_string()));
            }
            ("core.http.server", "mux") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("HttpMux".to_string()));
            }
            ("core.http.server", "serve") => {
                if args.len() != 2 && args.len() != 3 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!("`serve` expects 2 arguments, or 3 with `tls:`, got {}", args.len()),
                        "HTTPS serving uses the named `tls:` option so plaintext and TLS share one entry point".to_string(),
                        "write `Server.serve(addr, mux)` or `Server.serve(addr, mux, tls: Server.tls(cert, key))`".to_string(),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("serve", 0, &Type::String, &mut args[0]);
                // second arg is a Mux — just infer it
                self.infer(&mut args[1].expr);
                if args.len() == 3 {
                    match args[2].label.as_ref().map(|(label, span)| (label.as_str(), *span)) {
                        Some(("tls", _)) => {}
                        Some((label, label_span)) => self.diags.push(Diagnostic::error(
                            "E0125",
                            format!("`serve` has no option named `{label}` here"),
                            "the third HTTP server argument is a named TLS option, not a positional value".to_string(),
                            "write `tls: Server.tls(cert, key)`".to_string(),
                            Some(label_span),
                        )),
                        None => self.diags.push(Diagnostic::error(
                            "E0125",
                            "`serve` needs `tls:` before the third argument".to_string(),
                            "the label makes the transport switch explicit at the call site".to_string(),
                            "write `Server.serve(addr, mux, tls: Server.tls(cert, key))`".to_string(),
                            Some(args[2].span),
                        )),
                    }
                    self.expect_core_arg(
                        "serve",
                        2,
                        &Type::Named("HttpServerTls".to_string()),
                        &mut args[2],
                    );
                }
                return Some(Type::Result {
                    ok: Box::new(unit_ty()),
                    err: Box::new(Type::String),
                });
            }
            ("core.http.server", "serve_once") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("serve_once", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("serve_once", 0, &Type::String, &mut args[0]);
                self.infer(&mut args[1].expr);
                return Some(Type::Result {
                    ok: Box::new(unit_ty()),
                    err: Box::new(Type::String),
                });
            }
            ("core.http.server", "serve_once_listener") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("serve_once_listener", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "serve_once_listener",
                    0,
                    &Type::Named("TcpListener".to_string()),
                    &mut args[0],
                );
                self.infer(&mut args[1].expr);
                return Some(Type::Result {
                    ok: Box::new(unit_ty()),
                    err: Box::new(Type::String),
                });
            }
            ("core.http.server", "tls") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("tls", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("tls", 0, &Type::String, &mut args[0]);
                self.expect_core_arg("tls", 1, &Type::String, &mut args[1]);
                return Some(Type::Named("HttpServerTls".to_string()));
            }
            ("core.http.server", "response") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("response", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("response", 0, &Type::Int, &mut args[0]);
                self.expect_core_arg("response", 1, &Type::String, &mut args[1]);
                return Some(Type::Named("HttpSrvResp".to_string()));
            }
            ("core.http.server", "sse") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("sse", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("sse", 0, &Type::String, &mut args[0]);
                return Some(Type::Named("HttpSrvResp".to_string()));
            }
            ("core.http.server", "static_file") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("static_file", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("static_file", 0, &Type::String, &mut args[0]);
                self.expect_core_arg("static_file", 1, &Type::String, &mut args[1]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("HttpSrvResp".to_string())),
                    err: Box::new(Type::String),
                });
            }
            ("core.http.server", "static_file_range") => {
                if args.len() != 3 {
                    self.diags
                        .push(wrong_core_arity("static_file_range", 3, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "static_file_range",
                    0,
                    &Type::Named("HttpSrvReq".to_string()),
                    &mut args[0],
                );
                self.expect_core_arg("static_file_range", 1, &Type::String, &mut args[1]);
                self.expect_core_arg("static_file_range", 2, &Type::String, &mut args[2]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("HttpSrvResp".to_string())),
                    err: Box::new(Type::String),
                });
            }
            ("core.http.server", "access_log") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("access_log", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "access_log",
                    0,
                    &Type::Named("HttpSrvReq".to_string()),
                    &mut args[0],
                );
                self.expect_core_arg("access_log", 1, &Type::Int, &mut args[1]);
                return Some(Type::String);
            }
            // D-TIMEDEPTH1=A: civil-time constructors.
            ("core.time.date", "new") => {
                if args.len() != 3 {
                    self.diags
                        .push(wrong_core_arity("new", 3, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("new", 0, &Type::Int, &mut args[0]);
                self.expect_core_arg("new", 1, &Type::Int, &mut args[1]);
                self.expect_core_arg("new", 2, &Type::Int, &mut args[2]);
                return Some(Type::Named("LocalDate".to_string()));
            }
            ("core.time.date", "today") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("LocalDate".to_string()));
            }
            ("core.time.date", "parse") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("parse", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("parse", 0, &Type::String, &mut args[0]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("LocalDate".to_string())),
                    err: Box::new(Type::String),
                });
            }
            ("core.time.datetime", "from_timestamp") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("from_timestamp", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("from_timestamp", 0, &Type::Int, &mut args[0]);
                return Some(Type::Named("DateTime".to_string()));
            }
            ("core.time.datetime", "now") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("DateTime".to_string()));
            }
            // D-APPROX1=A: sketch constructors.
            ("core.sketch.hll", "new") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("HyperLogLog".to_string()));
            }
            ("core.sketch.tdigest", "new") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("TDigest".to_string()));
            }
            ("core.sketch.cms", "new") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("CountMinSketch".to_string()));
            }
            ("core.sketch.reservoir", "new") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("new", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("new", 0, &Type::Int, &mut args[0]);
                return Some(Type::Named("ReservoirSampler".to_string()));
            }
            // D-HONESTNUM1=A: `M.from(value, uncertainty)` → `Measurement<Float>`.
            ("core.science.measurement", "from") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("from", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("from", 0, &Type::Float, &mut args[0]);
                self.expect_core_arg("from", 1, &Type::Float, &mut args[1]);
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_MEASUREMENT.to_string(),
                    args: vec![Type::Float],
                });
            }
            // D-DECIMAL1: `core.numeric.decimal(s)` → `Decimal`.
            ("core.numeric", "decimal") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("decimal", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("decimal", 0, &Type::String, &mut args[0]);
                return Some(Type::Named(crate::Syntax::TYPE_DECIMAL.to_string()));
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
                    format!(
                        "argument {} to `{}` requires write access (`&`)",
                        i + 1,
                        name
                    ),
                    "this standard library call edits that value in place".to_string(),
                    format!("write `{}value` for this argument", Syntax::SIGIL_WRITE),
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

    /// D-A11YGATE1=B (c134 Phase 6, E2930): flag `ui.node_role(label, w, h, role)`
    /// when `label` is a literal empty string and `role` is a literal interactive
    /// role (`ui.aria_role_button()` / `ui.aria_role_text_input()`). Static and
    /// literal-only by design: a computed label isn't traced, so this never
    /// second-guesses a runtime value — only a call site that's provably wrong.
    pub(crate) fn check_a11y_node_role_label(&mut self, args: &[crate::AST::CallArg], span: Span) {
        if args.len() != 4 {
            return;
        }
        if !is_empty_string_literal(&args[0].expr) {
            return;
        }
        let Some(role) = self.interactive_aria_role_name(&args[3].expr) else {
            return;
        };
        self.diags.push(a11y_unlabeled_control(role, span));
    }

    /// D-A11YGATE1=B (E2931): within an inline `[ui.node_role(...), ...]` list
    /// literal passed to `backend.set_focus_group(...)`, flag two interactive
    /// nodes that share the same non-empty literal label. Inline-construction
    /// only (like E2930) — a list of pre-bound variables isn't traced back to
    /// their `node_role` call sites, so this catches the common "copy-pasted a
    /// focus group" mistake, not every possible duplicate.
    pub(crate) fn check_a11y_focus_group_duplicates(
        &mut self,
        args: &[crate::AST::CallArg],
        span: Span,
    ) {
        let Some(list_arg) = args.first() else {
            return;
        };
        let Expr::ListLit(items, _) = &list_arg.expr else {
            return;
        };
        let mut seen: std::collections::HashMap<String, ()> = std::collections::HashMap::new();
        for item in items {
            let Expr::MethodCall {
                receiver,
                method,
                args: call_args,
                ..
            } = item
            else {
                continue;
            };
            if method != "node_role" || call_args.len() != 4 {
                continue;
            }
            let Expr::Ident(alias, _) = &**receiver else {
                continue;
            };
            if self.core_imports.get(alias).map(|m| m.as_str()) != Some("core.ui") {
                continue;
            }
            if self
                .interactive_aria_role_name(&call_args[3].expr)
                .is_none()
            {
                continue;
            }
            let Some(label) = literal_string_value(&call_args[0].expr) else {
                continue;
            };
            if label.is_empty() {
                continue;
            }
            if seen.insert(label.clone(), ()).is_some() {
                self.diags.push(a11y_duplicate_label(&label, span));
                return;
            }
        }
    }

    /// D-A11YGATE1=B: is `expr` a literal `ui.aria_role_button()` /
    /// `ui.aria_role_text_input()` call through a `use core.ui as ui` alias?
    /// Returns the display name (`"button"` / `"text input"`) when so.
    fn interactive_aria_role_name(&self, expr: &Expr) -> Option<&'static str> {
        let Expr::MethodCall {
            receiver, method, ..
        } = expr
        else {
            return None;
        };
        let Expr::Ident(alias, _) = &**receiver else {
            return None;
        };
        if self.core_imports.get(alias).map(|m| m.as_str()) != Some("core.ui") {
            return None;
        }
        match method.as_str() {
            "aria_role_button" => Some("button"),
            "aria_role_text_input" => Some("text input"),
            _ => None,
        }
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
                    "the dynamic `Data` value exposes Null/Bool/Int/Float/Text/Array/Object"
                        .to_string(),
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
                self.expect_core_arg_consuming(variant, i, want, arg);
            } else {
                self.infer(&mut arg.expr);
            }
        }
        Some(json)
    }

    /// D-DBDRIVER1: `DbValue.Null` / `.Int(n)` / `.Float(f)` / `.Text(s)` / `.Bool(b)`
    /// — the tagged SQL parameter/column value construction. Mirrors
    /// `check_core_json_lit` exactly (same dynamic-value mechanism, SQL-shaped
    /// variants); `Int` stays `Type::Int` (64-bit), never widened through `Float`.
    pub(crate) fn check_core_dbvalue_lit(
        &mut self,
        variant: &str,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let dbvalue = Type::Named(Syntax::TYPE_DB_VALUE.to_string());
        let expected = match variant {
            "Null" => Vec::new(),
            "Int" => vec![Type::Int],
            "Float" => vec![Type::Float],
            "Text" => vec![Type::String],
            "Bool" => vec![Type::Bool],
            _ => {
                let candidates = ["Null", "Int", "Float", "Text", "Bool"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>();
                let mut fix = "check the variant name".to_string();
                if let Some(s) = suggest_field(variant, &candidates) {
                    fix = format!("did you mean `{}`?", s);
                }
                self.diags.push(Diagnostic::error(
                    "E0304",
                    format!("`{}` has no variant `{}`", Syntax::TYPE_DB_VALUE, variant),
                    "`DbValue` is the tagged SQL parameter/column value: Null/Int/Float/Text/Bool"
                        .to_string(),
                    fix,
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(dbvalue);
            }
        };
        if args.len() != expected.len() {
            self.diags.push(Diagnostic::error(
                "E0306",
                format!(
                    "`{}.{}` expects {} value{}, got {}",
                    Syntax::TYPE_DB_VALUE,
                    variant,
                    expected.len(),
                    if expected.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                "each `DbValue` variant has a fixed payload (Int→64-bit int, Float→float, Text→String, Bool→bool, Null→none)".to_string(),
                "check the variant payload".to_string(),
                Some(span),
            ));
        }
        for (i, arg) in args.iter_mut().enumerate() {
            if let Some(want) = expected.get(i) {
                self.expect_core_arg_consuming(variant, i, want, arg);
            } else {
                self.infer(&mut arg.expr);
            }
        }
        Some(dbvalue)
    }

    pub(crate) fn expect_core_arg(
        &mut self,
        call_name: &str,
        idx: usize,
        param_ty: &Type,
        arg: &mut crate::AST::CallArg,
    ) {
        self.expect_core_arg_impl(call_name, idx, param_ty, arg, false);
    }

    pub(crate) fn expect_url_arg(
        &mut self,
        call_name: &str,
        idx: usize,
        arg: &mut crate::AST::CallArg,
    ) {
        self.borrow_ctx = true;
        let got = self.infer(&mut arg.expr);
        if let Some(got) = got {
            if matches!(got, Type::String) || matches!(got, Type::Named(ref n) if n == "Url") {
                return;
            }
            self.diags.push(Diagnostic::error(
                "E0112",
                format!(
                    "`{}` wants String or Url for argument {}, but this is {}",
                    call_name,
                    idx + 1,
                    got.show()
                ),
                "HTTP client calls accept raw strings or typed Url values".to_string(),
                "pass a String, or build a Url with core.url.parse".to_string(),
                Some(arg.expr.span()),
            ));
        }
    }

    /// Same elaboration as `expect_core_arg`, but for the handful of std
    /// constructors that genuinely store the argument as their own payload
    /// (`Json.Text`/`DbValue.Text` own their `String`, etc. — see
    /// `check_core_json_lit` / `check_core_dbvalue_lit`). Only these call
    /// sites may trip the implicit-clone E0209 below; an ordinary read-only
    /// stdlib call (e.g. `fs.read(path)`) must not, even though its param
    /// type is also String/List/Map (D-MEM1/S2 false-positive fix).
    pub(crate) fn expect_core_arg_consuming(
        &mut self,
        call_name: &str,
        idx: usize,
        param_ty: &Type,
        arg: &mut crate::AST::CallArg,
    ) {
        self.expect_core_arg_impl(call_name, idx, param_ty, arg, true);
    }

    fn expect_core_arg_impl(
        &mut self,
        call_name: &str,
        idx: usize,
        param_ty: &Type,
        arg: &mut crate::AST::CallArg,
        consumes: bool,
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
        if consumes
            && matches!(arg.convention, AccessConvention::Read)
            && matches!(param_ty, Type::String | Type::List(_) | Type::Map { .. })
        {
            if let Expr::Ident(name, ispan) = &arg.expr {
                let name = name.clone();
                let ispan = *ispan;
                if self.is_borrowed_binding(&name) {
                    arg.flags.implicit_clone = true;
                    // D-MEM1/S2 (was D-L0201 lint): a hard error now, regardless
                    // of liveness — no clone is ever silent. Unlike a Move-param
                    // user function, this is a fixed std read-only signature —
                    // `^` is never accepted here (E0203), so `copy name` (D-CAP2,
                    // D-MEM1/S4) is the only fix, not a liveness-dependent
                    // move/reorder menu.
                    self.diags.push(Diagnostic::error(
                        "E0209",
                        format!("implicit clone of `{}`", name),
                        format!("`{}` stores its own copy of this value", call_name),
                        format!("write `{} {}` to copy explicitly", Syntax::KW_COPY, name),
                        Some(ispan),
                    ));
                }
            }
        }
    }

    /// D-DBDRIVER1: `(sql: String, params: [DbValue])` argument elaboration shared
    /// by `.query`/`.query_one`/`.execute` — SQL text plus a separate bind list,
    /// never a raw execute(sql) escape (the ratified build plan is explicit that
    /// a generic `execute_raw(sql)` must not exist).
    fn check_db_sql_params_args(
        &mut self,
        name: &str,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) {
        if args.len() != 2 {
            self.diags.push(wrong_core_arity(name, 2, args.len(), span));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return;
        }
        let params_ty = Type::List(Box::new(Type::Named(Syntax::TYPE_DB_VALUE.to_string())));
        self.expect_core_arg(name, 0, &Type::String, &mut args[0]);
        self.expect_core_arg(name, 1, &params_ty, &mut args[1]);
    }

    /// D-DBDRIVER1: instance methods on a `DbConnection` handle (produced by
    /// `db.open`/`db.open_memory`, mirroring `core.files`'s `open`/`create`
    /// producing a `FileReader`/`FileWriter`). The one generic driver interface:
    /// SQL text plus a separate `[DbValue]` bind list. `query`/`query_one`/
    /// `execute` are fallible (`? DbError`); `begin`/`commit`/`rollback`/`close`
    /// report plain success/failure (`Bool`) — there is nothing else to recover
    /// from a transaction control statement or a close.
    pub(crate) fn check_db_connection_method(
        &mut self,
        method: &str,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Option<Type>> {
        match method {
            "query" => {
                self.check_db_sql_params_args("query", args, span);
                self.record_effect(Effect::Db.name());
                Some(Some(result_ty(
                    Type::List(Box::new(db_row_ty())),
                    db_error_ty(),
                )))
            }
            "query_one" => {
                self.check_db_sql_params_args("query_one", args, span);
                self.record_effect(Effect::Db.name());
                Some(Some(result_ty(
                    Type::Option(Box::new(db_row_ty())),
                    db_error_ty(),
                )))
            }
            "execute" => {
                self.check_db_sql_params_args("execute", args, span);
                self.record_effect(Effect::Db.name());
                Some(Some(result_ty(Type::Int, db_error_ty())))
            }
            "begin" | "commit" | "rollback" | "close" => {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity(method, 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                }
                self.record_effect(Effect::Db.name());
                Some(Some(Type::Bool))
            }
            _ => None,
        }
    }
}

/// D-DBDRIVER1: the resolved return type of a covered `DbConnection` method, read
/// from `check_db_connection_method`'s authoritative match (arity/diagnostics
/// already ran in sema; this is a pure lookup for codegen's TIR totality
/// bookkeeping, mirroring `handle_method_return_ty`'s other sources).
pub fn db_connection_method_return_ty(method: &str) -> Option<Type> {
    match method {
        "query" => Some(result_ty(Type::List(Box::new(db_row_ty())), db_error_ty())),
        "query_one" => Some(result_ty(
            Type::Option(Box::new(db_row_ty())),
            db_error_ty(),
        )),
        "execute" => Some(result_ty(Type::Int, db_error_ty())),
        "begin" | "commit" | "rollback" | "close" => Some(Type::Bool),
        _ => None,
    }
}

impl<'a> Checker<'a> {
    /// D-DEP-WASM1=A / D-PLUGIN1=B (c81): `(name: String, args: [T]) -> T ?
    /// String` argument elaboration shared by `.call`/`.call_int` — a plugin
    /// export name plus a homogeneous scalar argument list. v1 supports
    /// exactly two scalar shapes (`Float` via `.call`, `Int` via `.call_int`);
    /// see `Prelude/Plugin.rs` for why Bool/Text aren't wired yet.
    fn check_plugin_call_args(
        &mut self,
        name: &str,
        arg_ty: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) {
        if args.len() != 2 {
            self.diags.push(wrong_core_arity(name, 2, args.len(), span));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return;
        }
        let list_ty = Type::List(Box::new(arg_ty.clone()));
        self.expect_core_arg(name, 0, &Type::String, &mut args[0]);
        self.expect_core_arg(name, 1, &list_ty, &mut args[1]);
    }

    /// D-DEP-WASM1=A / D-PLUGIN1=B (c81): instance methods on a `Plugin` handle
    /// (produced by `core.plugin`'s `load`). `call`/`call_int` are fallible
    /// (`? String`, naming a missing export or a param/type mismatch against
    /// the plugin's actual `.wit` signature) — the sandboxed loader never
    /// crashes the host program, it reports (I2).
    pub(crate) fn check_plugin_method(
        &mut self,
        method: &str,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Option<Type>> {
        match method {
            "call" => {
                self.check_plugin_call_args("call", &Type::Float, args, span);
                self.record_effect(Effect::Exec.name());
                Some(Some(result_ty(Type::Float, Type::String)))
            }
            "call_int" => {
                self.check_plugin_call_args("call_int", &Type::Int, args, span);
                self.record_effect(Effect::Exec.name());
                Some(Some(result_ty(Type::Int, Type::String)))
            }
            _ => None,
        }
    }
}

/// D-DEP-WASM1=A: the resolved return type of a covered `Plugin` method, read
/// from `check_plugin_method`'s authoritative match — a pure lookup for
/// codegen's TIR totality bookkeeping (mirrors `db_connection_method_return_ty`).
pub fn plugin_method_return_ty(method: &str) -> Option<Type> {
    match method {
        "call" => Some(result_ty(Type::Float, Type::String)),
        "call_int" => Some(result_ty(Type::Int, Type::String)),
        _ => None,
    }
}

/// D-MUSTUSE1 (c18iwxqx): built-in handle types whose bare statement result must
/// not be silently ignored (E0419). `scope.guard` returns `ScopeGuard` — bind it
/// or cleanup runs at end of the statement, not scope exit. `TransactionGuard` is
/// a phantom return from `on_commit`/`on_rollback` (registration is side-effect);
/// those calls are intentionally ignorable. `Task` stays on L1101.
pub(crate) fn core_must_use_type(name: &str) -> bool {
    matches!(name, "ScopeGuard")
}

pub(crate) fn unit_ty() -> Type {
    Type::Named("Unit".to_string())
}

pub(crate) fn u8_ty() -> Type {
    Type::IntN {
        signed: false,
        bits: 8,
    }
}

pub(crate) fn is_u8_ty(ty: &Type) -> bool {
    matches!(
        ty,
        Type::IntN {
            signed: false,
            bits: 8
        }
    )
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

// D-DBDRIVER1: the `DbValue` dynamic tagged SQL value.
pub(crate) fn is_db_value_type_name(name: &str) -> bool {
    Syntax::is_db_value_type_name(name)
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
        "Unit" | "Void" | "U8" | "Error" | "ProcessResult" | "ProcessSpec" | "ProcessChild" | "Stopwatch" | "Closed"
        // D-DET1: deterministic injected capability handles.
        // D-DET-CAPAPI: `Duration` value type for the widened clock surface.
        | "Clock" | "Rng" | "Duration"
        | "GameScene" | "GameAssets" | "GameInputMap" | "GameBudgetsSlot" | "GameBudgets"
        | "GameBackend" | "GameReplay" | "GameImage" | "GameSound" | "GameFrame"
        | "GameInputSnapshot" | "GameSceneType" | "GameReplayType" | "GameBackendType"
        | "GameBudgetsType"
        // D-BIGINT1 / D-DECIMAL1: arbitrary-precision numerics.
        | "BigInt" | "Decimal"
        | "FileReader" | "FileWriter" | "FileLines"
        | "StdinHandle" | "StdinLines" | "Stdout" | "Stderr"
        // D-LSDIR1/D-FSOPS1/D-WATCH-SCOPE1: filesystem and watcher values.
        | "DirEntry" | "Stat" | "WalkEntry" | "TempDir" | "TempFile" | "FileLock"
        | "WatchEvent" | "WatchHandle" | "WatchSet"
        // D-DATA-SURFACE1=A / D-DATA-STATUS1=A: data summary/status values.
        | "DataGroup" | "DataStatus" | "DataSummary"
        // D-LOGTRACE1=A: typed structured logging values.
        | "LogField" | "LogSpan"
        // D-ITERTOOLS1=A: expanded collection handles.
        | "BitSet" | "ByteBuffer"
        // E2-M10: networking opaque types.
        | "TcpListener" | "TcpStream" | "IpAddr" | "SocketAddr" | "UdpSocket" | "UdpPacket"
        | "DnsSrv" | "UnixListener" | "UnixStream" | "TlsStream"
        | "HttpRequest" | "HttpResponse" | "HttpRouter"
        // D-ALLOC1/D-ALLOC-C (ratified 2026-06-19): allocator opaque types.
        | "Arena" | "Bump" | "Pool" | "Fixed"
        // D-ARGS1 (ratified 2026-06-22): declarative CLI arg parsing types.
        | "ArgsSpec" | "ParsedArgs"
        // D-ANY-JAI1 (c7jaiany §6): runtime reflection floor handle types.
        | "Value" | "Field"
        // D-TERM1 (ratified 2026-06-22): terminal key-event enum.
        | "Key"
        // D-SERDE2: the format-agnostic value tree + typed-decode error.
        | "DataTree" | "DecodeError"
        // D-SIMD2 / D-LINALG1: built-in SIMD lane + linear-algebra value types.
        | "F32x4" | "F64x2"
        | "Vec2" | "Vec3" | "Vec4" | "Mat3" | "Mat4"
        // D-LAYOUT1 / D-LAYOUT-GATES1 (GATE 2, ratified 2026-06-28/29): the
        // built-in constraint-layout value types.
        | "HVar" | "VVar" | "LengthVar" | "Constraint" | "LayoutHandle"
        // D-REACT1=B: opt-in reactive handle types (used bare as `Signal<T>`/`Derived<T>`).
        | "Signal" | "Derived" | "Computed"
        // D-EVENT1=D: first-party typed Event/Hook family.
        | "Event" | "Hook" | "Subscription" | "EventScope" | "EventPolicy" | "EventTrace"
        // D-HONESTNUM1=A: Measurement<T> value ± uncertainty.
        | "Measurement"
        // D-PENDING1=B: async UI state machine.
        | "Loadable"
        // D-TTLVAL1=A: TTL-wrapped values and rotting secrets.
        | "Expired" | "Expiring" | "Rotting"
        // D-RENDERTGT2=A (c133 M1): UI backend seam types.
        | "Point" | "Size" | "Rect" | "SizeConstraint" | "UiNode" | "InputEvent"
        | "EventResult" | "NullBackend" | "TuiBackend"
        // D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend.
        | "GtkBackend"
        // D-A11YGATE1=B (c134 Phase 6): accessible-role opaque type.
        | "UiAriaRole"
        // c-devserver (owner-directed 2026-07-01): the configurable `jet dev`
        // server value returned by `core.devserver.for_app(...)`.
        | "DevServer"
        // D-APPROX1=A: approximate sketch data structures.
        | "HyperLogLog" | "TDigest" | "CountMinSketch" | "ReservoirSampler"
        // D-TIMEDEPTH1=A: civil-time types.
        | "Date" | "LocalDate" | "LocalTime" | "DateTime" | "Instant" | "Period" | "Zone"
        | "ZonedDateTime"
        // D-URL1=A: typed URL and MIME values.
        | "Url" | "Mime"
        // D-REGEXENGINE1=A: std-only linear regex values.
        | "Regex" | "RegexFlags" | "Match"
        // D-NETDEP1=A / D-HTTPLIB1=A: HTTP types.
        | "HttpClientReq" | "HttpClientResp" | "HttpMux" | "HttpSrvReq" | "HttpSrvResp" | "HttpServerTls"
        // D-TYPEDTEXT1=D: typed text — a checked query/markup template built by
        // expected-type elaboration of a string literal (E0149 guards a plain
        // runtime `String` from filling this position).
        | "Sql" | "Html"
        // D-SHIFT1 (c7shift): `binary.Reader` / `text.Cursor` — consuming,
        // fallible, `?`-composed cursors over `[U8]`/`String`.
        | "Reader" | "Cursor"
        // D-MIGRATE3=A: decode-time migration transparency. `DecodeResult<T>`
        // (generic, see `is_core_generic` in CheckerCore.rs) and its plain
        // `MigrationStatus` field both need the bare-name gate here too.
        | "DecodeResult" | "MigrationStatus"
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
    // D-MIGRATE3=A: `MigrationStatus` — `.migrated` false + `.from`/`.steps`
    // empty for fresh data and for non-`#PublishedSchema` types.
    if type_name == "MigrationStatus" {
        return match field {
            "migrated" => Some(Type::Bool),
            "from" => Some(Type::String),
            "steps" => Some(Type::List(Box::new(Type::String))),
            _ => None,
        };
    }
    if type_name == "DataGroup" {
        return match field {
            "key" => Some(Type::String),
            "count" => Some(Type::Int),
            "sum" | "mean" => Some(Type::Float),
            _ => None,
        };
    }
    if type_name == "DataStatus" {
        return match field {
            "step" | "path" | "replacement" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "DataSummary" {
        return match field {
            "count" => Some(Type::Int),
            "sum" | "mean" | "min" | "max" | "median" | "variance" | "stddev" => {
                Some(Type::Float)
            }
            _ => None,
        };
    }
    match (type_name, field) {
        // D-LSDIR1=A: DirEntry has name (bare filename), path (full path), is_dir.
        ("DirEntry", "name" | "path") => Some(Type::String),
        ("DirEntry", "is_dir") => Some(Type::Bool),
        // D-FSOPS1=A: typed filesystem metadata and recursive walk entries.
        ("Stat", "size" | "modified_ms" | "created_ms") => Some(Type::Int),
        ("Stat", "readonly" | "is_file" | "is_dir" | "is_symlink") => Some(Type::Bool),
        ("Stat", "kind") => Some(Type::String),
        ("WalkEntry", "path" | "relative") => Some(Type::String),
        ("WalkEntry", "is_dir") => Some(Type::Bool),
        ("WalkEntry", "depth") => Some(Type::Int),
        ("TempDir" | "TempFile" | "FileLock", "path") => Some(Type::String),
        ("WatchEvent", "domain" | "kind" | "path" | "detail") => Some(Type::String),
        ("WatchEvent", "pid" | "port") => Some(Type::Int),
        // D-RENDERTGT2=A (c133 M1): UI geometry fields.
        ("Point", "x" | "y") => Some(Type::Float),
        ("Size", "width" | "height") => Some(Type::Float),
        ("Rect", "x" | "y" | "width" | "height") => Some(Type::Float),
        ("SizeConstraint", "min_width" | "min_height" | "max_width" | "max_height") => {
            Some(Type::Float)
        }
        ("UiNode", "label") => Some(Type::String),
        ("UiNode", "width" | "height") => Some(Type::Float),
        ("ProcessResult", "code") => Some(Type::Int),
        ("ProcessResult", "success" | "timed_out") => Some(Type::Bool),
        ("ProcessResult", "signal") => Some(Type::Option(Box::new(Type::Int))),
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
        // D-GAME-*: scene-owned headless game substrate fields.
        ("GameScene", "assets") => Some(Type::Named("GameAssets".to_string())),
        ("GameScene", "input") => Some(Type::Named("GameInputMap".to_string())),
        ("GameScene", "budgets") => Some(Type::Named("GameBudgetsSlot".to_string())),
        ("GameFrame", "index") => Some(Type::Int),
        ("GameFrame", "input") => Some(Type::Named("GameInputSnapshot".to_string())),
        _ => None,
    }
}

impl<'a> Checker<'a> {
    fn check_game_run_scene_edit(&mut self, expr: &Expr) {
        let Some(root) = expr_root_ident(expr) else {
            self.diags.push(Diagnostic::error(
                "E0202",
                "`game.run` needs a mutable scene binding".to_string(),
                "running a scene advances its frame hooks and deterministic replay state"
                    .to_string(),
                "store the scene in `scene := game.Scene.new(...)`, then call `game.run(scene)`"
                    .to_string(),
                Some(expr.span()),
            ));
            return;
        };
        if let Some(info) = self.lookup(root) {
            if !info.mutable {
                self.diags.push(Diagnostic::error(
                    "E0202",
                    format!("`game.run` needs edit access to `{root}`"),
                    "running a scene advances its frame hooks and deterministic replay state"
                        .to_string(),
                    format!("declare `{root} := game.Scene.new(...)` before running it"),
                    Some(expr.span()),
                ));
            }
        }
    }
}

fn game_run_label_error(
    diags: &mut Vec<Diagnostic>,
    label: &str,
    arg: &crate::AST::CallArg,
    index: usize,
    span: Span,
) {
    let (expected, fix) = if index == 1 {
        ("replay or backend", "write `replay:` here, `backend:` here for a two-argument backend call, or drop the label")
    } else {
        ("backend", "write `backend:` here, or drop the label")
    };
    let label_span = arg.label.as_ref().map(|(_, s)| *s).unwrap_or(span);
    diags.push(Diagnostic::error(
        "E0125",
        format!("`game.run` has no `{label}:` option at argument {}", index + 1),
        format!("this position accepts {expected}; labels document the positional shape and never reorder arguments"),
        fix.to_string(),
        Some(label_span),
    ));
}

/// D-MIGRATE3=A: field access on the reserved generic `DecodeResult<T>` —
/// `.value: T` and `.migration: MigrationStatus`. Mirrors [`core_struct_field`]
/// for the one reserved core type that carries a generic type argument
/// (`Type::Apply`, not `Type::Named`); see the `Type::Apply` arm in
/// `CheckerInfer/expr.rs`'s member-access resolver.
pub(crate) fn core_generic_struct_field(
    type_name: &str,
    field: &str,
    args: &[Type],
) -> Option<Type> {
    if type_name == "DecodeResult" {
        return match field {
            "value" => args.first().cloned(),
            "migration" => Some(Type::Named("MigrationStatus".to_string())),
            _ => None,
        };
    }
    None
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
        "Enter" | "Escape" | "Backspace" | "Tab" | "Delete" | "Up" | "Down" | "Left" | "Right"
        | "Unknown" => Some(Vec::new()),
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
pub(crate) fn core_key_variants(
) -> std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)> {
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    let zero = Span::new(0, 0);
    let mut m = std::collections::HashMap::new();
    // Unit variants.
    for name in &[
        "Enter",
        "Escape",
        "Backspace",
        "Tab",
        "Delete",
        "Up",
        "Down",
        "Left",
        "Right",
        "Unknown",
    ] {
        m.insert((*name).to_string(), (zero, VariantPayload::Unit));
    }
    // Single-payload variants.
    m.insert(
        "Char".to_string(),
        (zero, VariantPayload::Single(Type::Char, zero)),
    );
    m.insert(
        "Ctrl".to_string(),
        (zero, VariantPayload::Single(Type::Char, zero)),
    );
    m.insert(
        "F".to_string(),
        (zero, VariantPayload::Single(Type::Int, zero)),
    );
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
            "write_line" if n_args == 1 => Some(Some(result_ty(unit.clone(), io.clone()))),
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
        // D-COREIO1=A: stdout/stderr stream methods.
        "Stdout" | "Stderr" => match method {
            "write" | "write_line" if n_args == 1 => {
                Some(Some(result_ty(unit.clone(), io.clone())))
            }
            "write_bytes" if n_args == 1 => Some(Some(result_ty(unit.clone(), io.clone()))),
            "flush" if n_args == 0 => Some(Some(result_ty(unit.clone(), io))),
            "is_tty" if n_args == 0 => Some(Some(Type::Bool)),
            _ => None,
        },
        _ => None,
    }
}

/// E2-M10: field definitions for compiler-known constructable struct types.
/// Returns `Some(fields)` when the named type is a prelude struct users can construct.
pub(crate) fn core_constructable_fields(type_name: &str) -> Option<Vec<(String, Type)>> {
    let str_ty = Type::String;
    let map_ty = Type::Map {
        key: Box::new(Type::String),
        value: Box::new(Type::String),
    };
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

/// D-SWIZZLE1: built-in vector/SIMD lane types that support `.xyz` member swizzles.
/// Matrices are not swizzleable.
pub fn is_swizzleable_math_type(name: &str) -> bool {
    matches!(name, "F32x4" | "F64x2" | "Vec2" | "Vec3" | "Vec4")
}

/// Outcome of parsing a swizzle member name (`xy`, `wzyx`, …) on a swizzleable type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwizzleParse {
    /// Valid lane indices in write order (x=0, y=1, z=2, w=3).
    Ok(Vec<usize>),
    /// A lane letter is out of range for this type (e.g. `.z` on `Vec2`).
    InvalidLane { lane: char },
    /// Not a swizzle pattern (wrong chars or length) — fall through to field lookup.
    NotSwizzle,
}

/// D-SWIZZLE1: parse `member` as a swizzle on `type_name`. Up to four `x`/`y`/`z`/`w`
/// letters; each must be in range for the type's lane count.
pub fn parse_swizzle_member(member: &str, type_name: &str) -> SwizzleParse {
    if !is_swizzleable_math_type(type_name) || member.is_empty() || member.len() > 4 {
        return SwizzleParse::NotSwizzle;
    }
    let max = math_arity(type_name);
    let mut lanes = Vec::with_capacity(member.len());
    for c in member.chars() {
        let idx = match c {
            'x' => 0,
            'y' => 1,
            'z' => 2,
            'w' => 3,
            _ => return SwizzleParse::NotSwizzle,
        };
        if idx >= max {
            return SwizzleParse::InvalidLane { lane: c };
        }
        lanes.push(idx);
    }
    SwizzleParse::Ok(lanes)
}

/// D-SWIZZLE1: the type of a read swizzle — one lane → scalar, N lanes → `VecN`
/// (or the same SIMD lane type when all lanes are selected).
pub fn swizzle_read_type(type_name: &str, lane_count: usize) -> Type {
    if lane_count == 1 {
        return math_scalar_ty(type_name);
    }
    if is_simd_lane_type(type_name) && lane_count == math_arity(type_name) {
        return Type::Named(type_name.to_string());
    }
    Type::Named(match lane_count {
        2 => "Vec2".to_string(),
        3 => "Vec3".to_string(),
        4 => "Vec4".to_string(),
        _ => unreachable!("swizzle lane count 2..=4"),
    })
}

/// D-SWIZZLE1: true when a write swizzle names the same source lane twice (`v.xx`).
pub fn swizzle_write_overlaps(lanes: &[usize]) -> bool {
    let mut seen = [false; 4];
    for &lane in lanes {
        if seen[lane] {
            return true;
        }
        seen[lane] = true;
    }
    false
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

/// D-LAYOUT1 / D-LAYOUT-GATES1: is `name` an axis-typed layout variable
/// (`HVar`/`VVar`/`LengthVar`)? `LengthVar` is axis-neutral: it combines with
/// either `HVar` or `VVar` without a mismatch, and is what a bare numeric
/// literal elaborates to in a layout-value position.
pub fn is_layout_axis_type(name: &str) -> bool {
    matches!(name, "HVar" | "VVar" | "LengthVar")
}

/// D-LAYOUT1: the full closed layout-value family (axis types + the
/// `Constraint`/`LayoutHandle` handles).
pub fn is_layout_type(name: &str) -> bool {
    is_layout_axis_type(name) || matches!(name, "Constraint" | "LayoutHandle")
}

/// D-LAYOUT1: the axis a value belongs to, for cross-axis checking. Plain
/// `Int`/`Float` are axis-neutral too (a bare numeric literal is allowed
/// anywhere a `LengthVar` is — same neutrality as `LengthVar` itself, so
/// `label.width >= 80.0` never needs an explicit `LengthVar(80.0)` wrapper).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutAxis {
    H,
    V,
    Neutral,
}

pub fn layout_axis_of(ty: &Type) -> Option<LayoutAxis> {
    match ty {
        Type::Named(n) if n == "HVar" => Some(LayoutAxis::H),
        Type::Named(n) if n == "VVar" => Some(LayoutAxis::V),
        Type::Named(n) if n == "LengthVar" => Some(LayoutAxis::Neutral),
        Type::Int | Type::Float => Some(LayoutAxis::Neutral),
        _ => None,
    }
}

/// D-LAYOUT1: combine two axes for `+`/`-` (same-axis closure) or a
/// comparison (`>=`/`<=`/`==`, GATE 1). `None` = cross-axis mismatch
/// (E2932, `E-LAYOUT-AXIS-MISMATCH`).
fn layout_axis_combine(a: LayoutAxis, b: LayoutAxis) -> Option<LayoutAxis> {
    use LayoutAxis::*;
    match (a, b) {
        (H, V) | (V, H) => None,
        (H, _) | (_, H) => Some(H),
        (V, _) | (_, V) => Some(V),
        (Neutral, Neutral) => Some(Neutral),
    }
}

fn layout_axis_type_name(axis: LayoutAxis) -> &'static str {
    match axis {
        LayoutAxis::H => "HVar",
        LayoutAxis::V => "VVar",
        LayoutAxis::Neutral => "LengthVar",
    }
}

/// D-LAYOUT1 / D-LAYOUT-GATES1: type-check a binary operator between two
/// layout values — mirrors `math_binop_result`'s closed-operator pattern
/// exactly (GATE 1 is what lets the comparison arms return `Constraint`
/// instead of `Bool`; this is the ONLY place that blessing is wired, no
/// parallel mechanism). `Some(Ok(ty))` = success; `Some(Err(()))` = axis
/// mismatch (caller emits E2932 naming both axes); `None` = not a layout
/// combination at all (caller falls through to normal operator checking).
pub fn layout_binop_result(
    op: crate::AST::BinOp,
    lt: &Type,
    rt: &Type,
) -> Option<Result<Type, ()>> {
    use crate::AST::BinOp;
    // At least one side must be an actual layout axis type — `Int + Float`
    // (both merely "neutral") is not our concern.
    let l_is_layout = matches!(lt, Type::Named(n) if is_layout_axis_type(n));
    let r_is_layout = matches!(rt, Type::Named(n) if is_layout_axis_type(n));
    if !l_is_layout && !r_is_layout {
        return None;
    }
    let (Some(la), Some(ra)) = (layout_axis_of(lt), layout_axis_of(rt)) else {
        return None;
    };
    match op {
        BinOp::Add | BinOp::Sub => match layout_axis_combine(la, ra) {
            Some(axis) => Some(Ok(Type::Named(layout_axis_type_name(axis).to_string()))),
            None => Some(Err(())),
        },
        BinOp::Ge | BinOp::Le | BinOp::Eq => match layout_axis_combine(la, ra) {
            Some(_) => Some(Ok(Type::Named("Constraint".to_string()))),
            None => Some(Err(())),
        },
        _ => None,
    }
}

/// D-LAYOUT1: type-check an instance method on `LayoutHandle`/`Constraint`.
/// Mirrors `math_method_return`'s pattern (a plain match table, not a
/// HashMap — this family is tiny).
pub fn layout_method_return(name: &str, method: &str, n_args: usize) -> Option<Type> {
    match name {
        "LayoutHandle" => match (method, n_args) {
            ("h", 2) => Some(Type::Named("HVar".to_string())),
            ("v", 2) => Some(Type::Named("VVar".to_string())),
            ("value", 1) => Some(Type::Float),
            ("suggest", 2) => Some(Type::Named("Unit".to_string())),
            ("is_feasible", 0) => Some(Type::Bool),
            ("conflict", 0) => Some(Type::List(Box::new(Type::String))),
            _ => None,
        },
        "Constraint" => match (method, n_args) {
            ("required" | "strong" | "medium" | "weak", 0) => {
                Some(Type::Named("Constraint".to_string()))
            }
            _ => None,
        },
        _ => None,
    }
}

/// D-LAYOUT1: the fixed argument type a `LayoutHandle` method expects, by
/// position. `None` means "no plain fixed type" — `.value(v)`/`.suggest(v, _)`'s
/// first argument accepts ANY of `HVar`/`VVar`/`LengthVar` (checked by the
/// caller via `is_layout_axis_type`, not a single `Type`).
pub fn layout_method_arg_ty(method: &str, arg_index: usize) -> Option<Type> {
    match (method, arg_index) {
        ("h", 0) | ("h", 1) | ("v", 0) | ("v", 1) => Some(Type::String),
        ("suggest", 1) => Some(Type::Float),
        _ => None,
    }
}

/// D-BIGINT1 / D-DECIMAL1: binary ops on precise numeric types (no Int promotion).
pub fn precise_binop_result(op: crate::AST::BinOp, lt: &str, rt: &str) -> Option<Type> {
    use crate::Numeric::{is_bigint_type_name, is_decimal_type_name};
    use crate::AST::BinOp;
    let same = lt == rt;
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul if same && is_bigint_type_name(lt) => {
            Some(Type::Named(crate::Syntax::TYPE_BIGINT.to_string()))
        }
        BinOp::Add | BinOp::Sub | BinOp::Mul if same && is_decimal_type_name(lt) => {
            Some(Type::Named(crate::Syntax::TYPE_DECIMAL.to_string()))
        }
        BinOp::Eq | BinOp::Ne if same && (is_bigint_type_name(lt) || is_decimal_type_name(lt)) => {
            Some(Type::Bool)
        }
        _ => None,
    }
}

/// D-BIGINT1: mixing fixed `Int` with `BigInt` is rejected — no silent promotion.
pub fn precise_mix_error(lt: &Type, rt: &Type) -> Option<(&'static str, String, String)> {
    use crate::Numeric::{type_is_bigint, type_is_decimal};
    let li = lt.is_integer();
    let ri = rt.is_integer();
    if (type_is_bigint(lt) && ri) || (type_is_bigint(rt) && li) {
        return Some((
            "E0130",
            format!(
                "`Int` and `BigInt` can't be mixed — got `{}` and `{}`",
                lt.show(),
                rt.show()
            ),
            "fixed-width `Int` never promotes to `BigInt`; construct a `BigInt` explicitly with `BigInt(…)` or `BigInt(\"…\")`".to_string(),
        ));
    }
    if (type_is_decimal(lt) && rt.is_float()) || (type_is_decimal(rt) && lt.is_float()) {
        return Some((
            "E0131",
            format!(
                "`Float` and `Decimal` can't be mixed — got `{}` and `{}`",
                lt.show(),
                rt.show()
            ),
            "use `Decimal(\"…\")` for exact money arithmetic; `Float` is for approximate science"
                .to_string(),
        ));
    }
    if (type_is_bigint(lt) && type_is_decimal(rt)) || (type_is_bigint(rt) && type_is_decimal(lt)) {
        return Some((
            "E0132",
            format!(
                "`BigInt` and `Decimal` can't be mixed — got `{}` and `{}`",
                lt.show(),
                rt.show()
            ),
            "convert explicitly with `to_string()` / `Decimal(\"…\")` at a boundary".to_string(),
        ));
    }
    None
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
        _ => None,
    }
}

/// D-REGEXENGINE1=A: method return types for std-only regex values.
pub fn regex_method_return(
    ty: &Type,
    method: &str,
    args: &[crate::AST::CallArg],
) -> Option<Option<Type>> {
    let argc = args.len();
    match ty {
        Type::Named(n) if n == "Regex" => match method {
            "is_match" if argc == 1 => Some(Some(Type::Bool)),
            "match" if argc == 1 => Some(Some(Type::Option(Box::new(Type::Named(
                "Match".to_string(),
            ))))),
            "find" if argc == 1 => Some(Some(Type::Option(Box::new(Type::String)))),
            "find_all" | "split" if argc == 1 => Some(Some(Type::List(Box::new(Type::String)))),
            "matches" if argc == 1 => {
                Some(Some(Type::List(Box::new(Type::Named("Match".to_string())))))
            }
            "replace" | "replace_all" if argc == 2 => Some(Some(Type::String)),
            "split_limit" if argc == 2 => Some(Some(Type::List(Box::new(Type::String)))),
            "replace_all_with" if argc == 2 => Some(Some(Type::String)),
            _ => None,
        },
        Type::Named(n) if n == "Match" => match method {
            "group" | "name" if argc == 1 => Some(Some(Type::Option(Box::new(Type::String)))),
            "start" | "end" if argc == 0 => Some(Some(Type::Int)),
            "group_start" | "group_end" if argc == 1 => {
                Some(Some(Type::Option(Box::new(Type::Int))))
            }
            _ => None,
        },
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
        "is_loaded" => Some(Some(Type::Bool)),
        "is_failed" => Some(Some(Type::Bool)),
        "is_idle" => Some(Some(Type::Bool)),
        // loaded() → T? — returns the value if in Loaded state, null otherwise.
        "loaded" => Some(Some(Type::Option(Box::new(val_ty)))),
        // or_else(default: T) → T
        "or_else" => Some(Some(val_ty)),
        _ => None,
    }
}

/// D-TTLVAL1=A: instance methods on `Expiring<T>` / `Rotting<T>`.
pub fn expiring_method_return(
    type_apply: &Type,
    method: &str,
    _n_args: usize,
) -> Option<Option<Type>> {
    let val_ty = match type_apply {
        Type::Apply { name, args } if name == "Expiring" && args.len() == 1 => args[0].clone(),
        _ => return None,
    };
    match method {
        "get" => Some(Some(Type::Result {
            ok: Box::new(val_ty),
            err: Box::new(Type::Named("Expired".to_string())),
        })),
        "is_valid" => Some(Some(Type::Bool)),
        "force" => Some(Some(val_ty)), // sema rejects the call site separately (E0511)
        _ => None,
    }
}

pub fn rotting_method_return(
    type_apply: &Type,
    method: &str,
    _n_args: usize,
) -> Option<Option<Type>> {
    let val_ty = match type_apply {
        Type::Apply { name, args } if name == "Rotting" && args.len() == 1 => args[0].clone(),
        _ => return None,
    };
    match method {
        "get" => Some(Some(Type::Result {
            ok: Box::new(val_ty),
            err: Box::new(Type::Named("Expired".to_string())),
        })),
        "is_valid" => Some(Some(Type::Bool)),
        "force" => Some(Some(val_ty)),
        _ => None,
    }
}

/// D-NETDEP1=A / D-HTTPLIB1=A: method return types for HTTP types.
pub fn http_type_method_return(
    ty: &Type,
    method: &str,
    _args: &[crate::AST::CallArg],
) -> Option<Option<Type>> {
    let mk = |n: &str| Some(Some(Type::Named(n.to_string())));
    let mk_str = || Some(Some(Type::String));
    let mk_int = || Some(Some(Type::Int));
    let mk_opt_str = || Some(Some(Type::Option(Box::new(Type::String))));
    match ty {
        Type::Named(n) if n == "HttpClientReq" => match method {
            "header" | "body" | "timeout" | "connect_timeout" | "read_timeout"
            | "total_timeout" | "redirects" | "proxy" | "cookie" | "form" | "multipart_text" => {
                mk("HttpClientReq")
            }
            "send" => Some(Some(Type::Result {
                ok: Box::new(Type::Named("HttpClientResp".to_string())),
                err: Box::new(Type::String),
            })),
            _ => None,
        },
        Type::Named(n) if n == "HttpClientResp" => match method {
            "status" => mk_int(),
            "body" => mk_str(),
            "header" => mk_opt_str(),
            "cookies" => Some(Some(Type::List(Box::new(Type::String)))),
            _ => None,
        },
        Type::Named(n) if n == "HttpMux" => match method {
            "get" | "post" | "put" | "delete" | "patch" => Some(None),
            _ => None,
        },
        Type::Named(n) if n == "HttpSrvReq" => match method {
            "method" | "path" | "body" => mk_str(),
            "param" | "header" => mk_opt_str(),
            "body_len" => mk_int(),
            "under_limit" => Some(Some(Type::Bool)),
            _ => None,
        },
        Type::Named(n) if n == "HttpSrvResp" => match method {
            "header" => mk("HttpSrvResp"),
            _ => None,
        },
        _ => None,
    }
}

/// D-URL1=A: method return types for typed URL and MIME values.
pub fn url_mime_method_return(
    ty: &Type,
    method: &str,
    args: &[crate::AST::CallArg],
) -> Option<Option<Type>> {
    let argc = args.len();
    match ty {
        Type::Named(n) if n == "Url" => match method {
            "scheme" | "path" | "query" | "to_string" if argc == 0 => Some(Some(Type::String)),
            "host" | "fragment" if argc == 0 => Some(Some(Type::Option(Box::new(Type::String)))),
            "port" if argc == 0 => Some(Some(Type::Option(Box::new(Type::Int)))),
            "path_segments" if argc == 0 => Some(Some(Type::List(Box::new(Type::String)))),
            "query_pairs" if argc == 0 => Some(Some(Type::List(Box::new(Type::List(Box::new(
                Type::String,
            )))))),
            "normalize" if argc == 0 => Some(Some(Type::Named("Url".to_string()))),
            "join" if argc == 1 => Some(Some(result_ty(
                Type::Named("Url".to_string()),
                Type::String,
            ))),
            "set_query" | "add_query" if argc == 2 => Some(Some(Type::Named("Url".to_string()))),
            _ => None,
        },
        Type::Named(n) if n == "Mime" => match method {
            "media_type" | "subtype" | "essence" | "to_string" if argc == 0 => {
                Some(Some(Type::String))
            }
            "param" if argc == 1 => Some(Some(Type::Option(Box::new(Type::String)))),
            "params" if argc == 0 => Some(Some(Type::List(Box::new(Type::List(Box::new(
                Type::String,
            )))))),
            _ => None,
        },
        _ => None,
    }
}

/// D-TIMEDEPTH1/D-TIME-CALENDAR1: method return types for civil-time values.
pub fn civil_time_method_return(
    ty: &Type,
    method: &str,
    args: &[crate::AST::CallArg],
) -> Option<Option<Type>> {
    let argc = args.len();
    match ty {
        Type::Named(n) if n == "Date" || n == "LocalDate" => match method {
            "year" | "month" | "day" | "weekday" | "iso_weekday" | "day_of_year" | "iso_week"
                if argc == 0 =>
            {
                Some(Some(Type::Int))
            }
            "diff_days" if argc == 1 => Some(Some(Type::Int)),
            "add_days" | "add_months" | "truncate" if argc == 1 => {
                Some(Some(Type::Named("LocalDate".to_string())))
            }
            "add_period" if argc == 1 => Some(Some(Type::Named("LocalDate".to_string()))),
            "to_string" if argc == 0 => Some(Some(Type::String)),
            "format" if argc == 1 => Some(Some(Type::String)),
            _ => None,
        },
        Type::Named(n) if n == "LocalTime" => match method {
            "hour" | "minute" | "second" if argc == 0 => Some(Some(Type::Int)),
            "to_string" if argc == 0 => Some(Some(Type::String)),
            _ => None,
        },
        Type::Named(n) if n == "DateTime" => match method {
            "hour" | "minute" | "second" | "to_timestamp" | "to_unix_ms" if argc == 0 => {
                Some(Some(Type::Int))
            }
            "date" if argc == 0 => Some(Some(Type::Named("LocalDate".to_string()))),
            "time" if argc == 0 => Some(Some(Type::Named("LocalTime".to_string()))),
            "plus_duration" | "truncate" | "round" if argc == 1 => {
                Some(Some(Type::Named("DateTime".to_string())))
            }
            "in_zone" if argc == 1 => Some(Some(Type::Named("ZonedDateTime".to_string()))),
            "to_string" | "format_rfc3339" if argc == 0 => Some(Some(Type::String)),
            "format" if argc == 1 => Some(Some(Type::String)),
            _ => None,
        },
        Type::Named(n) if n == "Instant" => match method {
            "elapsed_millis" if argc == 0 => Some(Some(Type::Int)),
            _ => None,
        },
        Type::Named(n) if n == "Duration" => match method {
            "millis" | "seconds" if argc == 0 => Some(Some(Type::Int)),
            _ => None,
        },
        Type::Named(n) if n == "Period" => match method {
            "to_string" if argc == 0 => Some(Some(Type::String)),
            _ => None,
        },
        Type::Named(n) if n == "Zone" => match method {
            "name" if argc == 0 => Some(Some(Type::String)),
            _ => None,
        },
        Type::Named(n) if n == "ZonedDateTime" => match method {
            "date" if argc == 0 => Some(Some(Type::Named("LocalDate".to_string()))),
            "time" if argc == 0 => Some(Some(Type::Named("LocalTime".to_string()))),
            "offset_seconds" if argc == 0 => Some(Some(Type::Int)),
            "to_datetime" if argc == 0 => Some(Some(Type::Named("DateTime".to_string()))),
            "zone" if argc == 0 => Some(Some(Type::Named("Zone".to_string()))),
            "add_duration" | "add_period" if argc == 1 => {
                Some(Some(Type::Named("ZonedDateTime".to_string())))
            }
            "to_string" if argc == 0 => Some(Some(Type::String)),
            "format" if argc == 1 => Some(Some(Type::String)),
            _ => None,
        },
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
        ("HyperLogLog", "add") => Some(None), // void
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
        "join" => Some(Some(path())),
        "parent" => Some(Some(Type::Option(Box::new(path())))),
        "extension" => Some(Some(Type::Option(Box::new(Type::String)))),
        "stem" => Some(Some(Type::Option(Box::new(Type::String)))),
        "to_string" => Some(Some(Type::String)),
        "write_atomic" => Some(Some(result_ty(unit_ty(), Type::String))),
        "walk" => Some(Some(Type::List(Box::new(path())))),
        _ => None,
    }
}

/// D-SHIFT1 (c7shift): a Jet-sized unsigned int type (`U8`/`U16`/`U32`/`U64`),
/// the return type of `Reader`'s width-specific reads.
fn uintn_ty(bits: u8) -> Type {
    Type::IntN {
        signed: false,
        bits,
    }
}

/// D-SHIFT1 (c7shift): method calls on `binary.Reader` (`Reader.over(bytes)`
/// static constructor is handled in `CheckerInfer/calls.rs`, mirroring
/// `Path.from`). Every read is fallible — a bounds miss is an ordinary `?`
/// error value (`Result<T, String>`, `path_method_return`'s exact error-type
/// convention above), never a panic or silent truncation (I1/L2).
pub fn binary_reader_method_return(
    type_name: &str,
    method: &str,
    n_args: usize,
) -> Option<Option<Type>> {
    if type_name != "Reader" {
        return None;
    }
    let bytes = || Type::List(Box::new(u8_ty()));
    match (method, n_args) {
        ("read_u8", 0) => Some(Some(result_ty(uintn_ty(8), Type::String))),
        ("read_u16_le" | "read_u16_be", 0) => Some(Some(result_ty(uintn_ty(16), Type::String))),
        ("read_u32_le" | "read_u32_be", 0) => Some(Some(result_ty(uintn_ty(32), Type::String))),
        ("read_u64_le" | "read_u64_be", 0) => Some(Some(result_ty(uintn_ty(64), Type::String))),
        ("take", 1) => Some(Some(result_ty(bytes(), Type::String))),
        ("remaining", 0) => Some(Some(Type::Int)),
        ("at_end", 0) => Some(Some(Type::Bool)),
        _ => None,
    }
}

/// D-SHIFT1 (c7shift): method calls on `text.Cursor` (`Cursor.over(s)` static
/// constructor is handled in `CheckerInfer/calls.rs`). `take_pattern` is NOT
/// listed here — it needs its pattern-literal argument's hole types to
/// compute a return type, so it's dispatched directly at the call site
/// (`CheckerInfer/calls.rs`), same reason `Gc.new<T>`/`Arena.alloc` are
/// resolved outside their generic method-return tables.
pub fn text_cursor_method_return(
    type_name: &str,
    method: &str,
    n_args: usize,
) -> Option<Option<Type>> {
    if type_name != "Cursor" {
        return None;
    }
    match (method, n_args) {
        ("take_until", 1) => Some(Some(result_ty(Type::String, Type::String))),
        ("skip_ws", 0) => Some(Some(unit_ty())),
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

/// D-OPTGC1: method calls on the `Gc` constructor sentinel (`gc.Gc.new<T>(…)`).
pub(crate) fn gc_method_return(
    method: &str,
    type_args: &[Type],
    args: &[crate::AST::CallArg],
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    match method {
        "new" => {
            if type_args.len() != 1 {
                diags.push(Diagnostic::error(
                    "E0103",
                    "`Gc.new` needs exactly one type argument".to_string(),
                    "pass the element type in angle brackets".to_string(),
                    "write `gc.Gc.new<Int>(value)`".to_string(),
                    Some(span),
                ));
                return Some(None);
            }
            if args.len() != 1 {
                diags.push(Diagnostic::error(
                    "E0103",
                    "`Gc.new` takes exactly one value argument".to_string(),
                    "pass the owned value to store in the traced heap".to_string(),
                    "write `gc.Gc.new<MyType>(value)`".to_string(),
                    Some(span),
                ));
                return Some(None);
            }
            Some(Some(Type::Apply {
                name: Syntax::GC_TYPE.to_string(),
                args: vec![type_args[0].clone()],
            }))
        }
        "with" | "with_mut" => {
            if args.len() != 1 {
                diags.push(Diagnostic::error(
                    "E0103",
                    format!("`Gc.{method}` takes exactly one closure argument"),
                    "pass a closure that receives the inner value".to_string(),
                    format!("write `handle.{method}(|v| {{ … }})`"),
                    Some(span),
                ));
                return Some(None);
            }
            Some(Some(Type::Named("__gc_with_infer__".to_string())))
        }
        _ => None,
    }
}

pub(crate) fn is_gc_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Apply { name, .. } if name == Syntax::GC_TYPE
    )
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
                    format!(
                        "write `mem.{}.new()` or `mem.{}.new(capacity: N)`",
                        type_name, type_name
                    ),
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
                format!("`{}` supports: `new`, `alloc`, `reset`, `free`", type_name),
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
            alloc_name, method, alloc_name
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

/// D-DBDRIVER1: the error type `.query`/`.query_one`/`.execute` fail with.
pub(crate) fn db_error_ty() -> Type {
    Type::Named("DbError".to_string())
}

/// D-DBDRIVER1: a `Row` is `Map<String, DbValue>` — the built-in `Map` already
/// gives `.get`/`.keys`/`.values`/`.contains_key`, so no separate nominal `Row`
/// type is registered (I8: reuse the existing collection instead of inventing one).
pub(crate) fn db_row_ty() -> Type {
    Type::Map {
        key: Box::new(Type::String),
        value: Box::new(Type::Named(Syntax::TYPE_DB_VALUE.to_string())),
    }
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
        "this operation can violate memory safety, so it must sit in an audited region".to_string(),
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
            | ("core.math", "sin")
            | ("core.math", "cos")
            | ("core.math", "tan")
            | ("core.math", "asin")
            | ("core.math", "acos")
            | ("core.math", "atan")
            | ("core.math", "atan2")
            | ("core.math", "sinh")
            | ("core.math", "cosh")
            | ("core.math", "tanh")
            | ("core.math", "exp")
            | ("core.math", "ln")
            | ("core.math", "log2")
            | ("core.math", "log10")
            | ("core.math", "hypot")
            | ("core.math", "trunc")
            | ("core.math", "fract")
            | ("core.math", "sign")
            | ("core.math", "is_nan")
            | ("core.math", "is_inf")
            | ("core.math", "is_finite")
            | ("core.math", "to_bits")
            | ("core.math", "from_bits")
            | ("core.math", "degrees")
            | ("core.math", "radians")
            | ("core.math", "lerp")
            | ("core.math", "checked_add")
            | ("core.math", "checked_sub")
            | ("core.math", "checked_mul")
            | ("core.math", "checked_pow")
            | ("core.math", "saturating_add")
            | ("core.math", "saturating_sub")
            | ("core.math", "saturating_mul")
            | ("core.math", "wrapping_add")
            | ("core.math", "wrapping_sub")
            | ("core.math", "wrapping_mul")
            | ("core.math", "int_pow")
            | ("core.math", "gcd")
            | ("core.math", "lcm")
            | ("core.random", "pick")
            | ("core.random", "weighted_pick")
            | ("core.random", "sample")
            | ("core.random", "shuffle")
            | ("core.io", "eprint")
            // D-ENC1 / D-SERDE6: typed encode/decode return types depend on the value
            // type / call-site `<T>`, so codegen reads them from resolved_ret (I3).
            // D-MIGRATE3=A: `decode_traced` is the same call-site-typed shape, one
            // layer deeper (`DecodeResult<T>`).
            | (
                "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
                | "core.encoding.yaml",
                "to_string" | "to_string_pretty" | "decode" | "decode_traced",
            )
            // D-REACT1=B: the reactive producers return `Signal<T>`/`Derived<T>` whose
            // element type is inferred from the initial value / closure return — not in
            // `core_fixed_sig`, so codegen reads it from resolved_ret (I3).
            | ("jet.reactive", "signal" | "derived")
            // D-TUPLE-DESTRUCT1: `tasks.channel<T>()` returns `(Sender<T>, Receiver<T>)`,
            // `T` read off the call-site turbofish — not in `core_fixed_sig`, so codegen
            // reads the whole tuple type from resolved_ret (I3).
            | ("core.tasks", "channel" | "after")
            | (
                "core.data",
                "csv" | "count" | "table" | "rows" | "series" | "values" | "missing_count"
                    | "lazy" | "lazy_filter" | "lazy_sort_by" | "collect" | "plan"
                    | "filter" | "sort_by" | "group_count" | "group_sum" | "group_mean",
            )
    )
}

pub fn core_fixed_sig(
    module: &str,
    name: &str,
) -> Option<(Vec<(AccessConvention, Type)>, Option<Type>)> {
    let normalized_module =
        Syntax::normalize_core_module(module).unwrap_or_else(|| module.to_string());
    let module = normalized_module.as_str();
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
        ("core.files", "read") => Some((vec![(read, string.clone())], Some(result_ty(string, io)))),
        ("core.files", "read_bytes") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8, io_error_ty())),
        )),
        // D-FILES-WRITE1 (merge) + D-FILES-APPEND1=A: `write`/`append_all` are the
        // whole-file convenience twins of the streaming `open`/`create`/`append`
        // handle constructors below. `append_all` (not `append`) so it doesn't
        // collide with the streaming handle's `.append(text)` method in the same
        // `core.files` namespace.
        ("core.files", "write" | "append_all") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(io_unit),
        )),
        ("core.files", "exists" | "is_dir") => Some((vec![(read, Type::String)], Some(bool_))),
        (
            "core.files",
            "remove" | "create_dir" | "create_dir_all" | "remove_dir" | "remove_all",
        ) => Some((
            vec![(read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "stat") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Stat".to_string()), io_error_ty())),
        )),
        ("core.files", "canonicalize" | "absolute") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::String, io_error_ty())),
        )),
        // D-LSDIR1=A: returns [DirEntry] ({name, path, is_dir}) — full path + type in one step.
        ("core.files", "list_dir") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("DirEntry".to_string()))),
                io_error_ty(),
            )),
        )),
        ("core.files", "copy" | "rename") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "copy_dir") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "symlink" | "hard_link") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "read_link") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::String, io_error_ty())),
        )),
        ("core.files", "walk") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("WalkEntry".to_string()))),
                io_error_ty(),
            )),
        )),
        ("core.files", "glob") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::List(Box::new(Type::String)), io_error_ty())),
        )),
        ("core.files", "read_at") => Some((
            vec![(read, Type::String), (read, Type::Int), (read, Type::Int)],
            Some(result_ty(Type::List(Box::new(u8_ty())), io_error_ty())),
        )),
        ("core.files", "write_at") => Some((
            vec![
                (read, Type::String),
                (read, Type::Int),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "fsync") => Some((
            vec![(read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "write_atomic") => Some((
            vec![(read, Type::String), (read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "temp_dir") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("TempDir".to_string()), io_error_ty())),
        )),
        ("core.files", "temp_file") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("TempFile".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.files", "lock") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("FileLock".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.watcher", "files") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("WatchHandle".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.watcher", "process_pid") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("WatchHandle".to_string())),
        )),
        ("core.watcher", "port") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(Type::Named("WatchHandle".to_string())),
        )),
        ("core.watcher", "set") => Some((vec![], Some(Type::Named("WatchSet".to_string())))),
        ("core.io", "args") => Some((vec![], Some(Type::List(Box::new(Type::String))))),
        ("core.io", "read_all_input") => {
            Some((vec![], Some(result_ty(Type::String, io_error_ty()))))
        }
        // D-STDIN1=A: streaming line-by-line stdin.
        ("core.io", "stdin") => Some((vec![], Some(Type::Named("StdinHandle".to_string())))),
        ("core.io", "stdout") => Some((vec![], Some(Type::Named("Stdout".to_string())))),
        ("core.io", "stderr") => Some((vec![], Some(Type::Named("Stderr".to_string())))),
        ("core.io", "terminal_width" | "terminal_height") => Some((vec![], Some(Type::Int))),
        ("core.io", "style" | "style_force") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(Type::String),
        )),
        ("core.io", "progress") => Some((
            vec![(read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.env", "get") => Some((
            vec![(read, Type::String)],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.env", "set") => Some((vec![(read, Type::String), (read, Type::String)], None)),
        ("core.env", "current_dir") => Some((vec![], Some(result_ty(Type::String, io_error_ty())))),
        ("core.env", "home_dir") => Some((vec![], Some(Type::Option(Box::new(Type::String))))),
        ("core.os", "name" | "family" | "arch" | "temp_dir" | "executable" | "hostname" | "username") => {
            Some((vec![], Some(Type::String)))
        }
        ("core.os", "pid" | "cpu_count") => Some((vec![], Some(Type::Int))),
        ("core.os", "set_current_dir") => {
            Some((vec![(read, Type::String)], Some(result_ty(unit_ty(), io_error_ty()))))
        }
        ("core.os", "on_interrupt") => Some((
            vec![(
                read,
                Type::Fn {
                    params: vec![],
                    ret: None,
                    effect_bound: None,
                },
            )],
            None,
        )),
        // U13 (D-JPK-SECRETCRYPTO1): `core.vault.get(name)` — a decrypted repo
        // secret, `None` if `name` isn't in the store. Same "may be missing"
        // shape as `core.env.get`.
        ("core.vault", "get") => Some((
            vec![(read, Type::String)],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.process", "exit") => Some((vec![(read, int)], None)),
        ("core.process", "run") => Some((
            vec![(read, Type::List(Box::new(Type::String)))],
            Some(result_ty(
                Type::Named("ProcessResult".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.process", "cmd") => Some((
            vec![(read, Type::List(Box::new(Type::String)))],
            Some(Type::Named("ProcessSpec".to_string())),
        )),
        ("core.process", "pipeline") => Some((
            vec![(
                read,
                Type::List(Box::new(Type::List(Box::new(Type::String)))),
            )],
            Some(result_ty(
                Type::Named("ProcessResult".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.testing", "snap" | "golden") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(Type::Bool),
        )),
        ("core.testing", "fixture" | "temp_dir") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        ("core.testing", "corpus") => Some((
            vec![(read, Type::String)],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.testing", "fake_clock") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("Clock".to_string())),
        )),
        ("core.testing", "fake_rng") => {
            Some((vec![(read, Type::Int)], Some(Type::Named("Rng".to_string()))))
        }
        ("core.testing", "bench_budget") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(Type::Bool),
        )),
        ("core.math", "sqrt" | "floor" | "ceil") => {
            Some((vec![(read, float.clone())], Some(float)))
        }
        ("core.math", "pow") => Some((
            vec![(read, Type::Float), (read, Type::Float)],
            Some(Type::Float),
        )),
        ("core.math", "round") => Some((vec![(read, Type::Float)], Some(Type::Int))),
        ("core.random", "int") => {
            Some((vec![(read, Type::Int), (read, Type::Int)], Some(Type::Int)))
        }
        ("core.random", "float") => Some((vec![], Some(Type::Float))),
        ("core.random", "float_range") => Some((
            vec![(read, Type::Float), (read, Type::Float)],
            Some(Type::Float),
        )),
        ("core.random", "bool") => Some((vec![(read, Type::Float)], Some(Type::Bool))),
        ("core.random", "normal") => Some((
            vec![(read, Type::Float), (read, Type::Float)],
            Some(Type::Float),
        )),
        ("core.random", "exponential") => Some((vec![(read, Type::Float)], Some(Type::Float))),
        ("core.random", "seed") => Some((vec![(read, Type::Int)], None)),
        // D-RANDSPLIT1=A: seedable PRNG bytes — fast, NOT cryptographically secure.
        // Returns raw `[Int8N]`; for crypto contexts use `core.crypto.random.bytes`.
        ("core.random", "bytes") => {
            Some((vec![(read, Type::Int)], Some(Type::List(Box::new(u8_ty())))))
        }
        // D-RANDSPLIT1=A: CSPRNG bytes via /dev/urandom — cryptographically secure.
        // Use for tokens, keys, nonces, and secrets.
        ("core.crypto.random", "bytes") => {
            Some((vec![(read, Type::Int)], Some(Type::List(Box::new(u8_ty())))))
        }
        // D-DET1: deterministic injected RNG capability. `random.rng(seed)` builds a
        // reproducible `Rng` from a caller-supplied seed (a pure value); a `@Pure fn`
        // may draw randomness through it (`rng.int(lo, hi)` / `rng.float()`) while the
        // ambient `random.int(…)` stays E3403.
        ("core.random", "rng") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named(crate::Syntax::RNG_TYPE.to_string())),
        )),
        ("core.random", "split") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named(crate::Syntax::RNG_TYPE.to_string())),
        )),
        ("core.time", "now") => Some((vec![], Some(Type::Int))),
        ("core.time", "sleep") => Some((vec![(read, Type::Int)], None)),
        ("core.tasks", "interval") => Some((
            vec![(read, Type::Int)],
            Some(Type::Apply {
                name: "Receiver".to_string(),
                args: vec![Type::Int],
            }),
        )),
        ("core.time", "start") => Some((vec![], Some(Type::Named("Stopwatch".to_string())))),
        ("core.time", "instant") => Some((vec![], Some(Type::Named("Instant".to_string())))),
        ("core.time", "now_utc") => Some((vec![], Some(Type::Named("DateTime".to_string())))),
        ("core.time", "from_unix_ms") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("DateTime".to_string())),
        )),
        ("core.time", "today") => Some((vec![], Some(Type::Named("LocalDate".to_string())))),
        ("core.time", "parse_rfc3339") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("DateTime".to_string()), Type::String)),
        )),
        ("core.time", "local_time") => Some((
            vec![(read, Type::Int), (read, Type::Int), (read, Type::Int)],
            Some(Type::Named("LocalTime".to_string())),
        )),
        ("core.time", "parse_time") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("LocalTime".to_string()),
                Type::String,
            )),
        )),
        ("core.time", "period") => Some((
            vec![(read, Type::Int), (read, Type::Int), (read, Type::Int)],
            Some(Type::Named("Period".to_string())),
        )),
        ("core.time", "period_days" | "period_months" | "period_years") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("Period".to_string())),
        )),
        ("core.time", "zone") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Zone".to_string()), Type::String)),
        )),
        ("core.time", "utc") => Some((vec![], Some(Type::Named("Zone".to_string())))),
        ("core.time", "zoned") => Some((
            vec![
                (read, Type::Named("DateTime".to_string())),
                (read, Type::Named("Zone".to_string())),
            ],
            Some(Type::Named("ZonedDateTime".to_string())),
        )),
        ("core.time", "zoned_local") => Some((
            vec![
                (read, Type::Named("LocalDate".to_string())),
                (read, Type::Named("LocalTime".to_string())),
                (read, Type::Named("Zone".to_string())),
            ],
            Some(Type::Named("ZonedDateTime".to_string())),
        )),
        // D-DET1: deterministic injected Clock capability. `time.clock(seed)` builds a
        // reproducible `Clock` from a caller-supplied start instant (a pure Int, ms);
        // a `@Pure fn` may read time through it (`clock.now()` / `clock.tick(ms)`)
        // while the ambient `time.now()` stays E3403.
        ("core.time", "clock") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named(crate::Syntax::CLOCK_TYPE.to_string())),
        )),
        // D-DET-CAPAPI: `time.ms(n)` / `time.secs(n)` mint a deterministic `Duration`
        // value (pure — no ambient effect, like `time.clock`). The clock advances by
        // one with `clock.wait(d)`; read it back with `duration.millis()`.
        ("core.time", "ms" | "secs" | "seconds" | "minutes" | "hours") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named(crate::Syntax::DURATION_TYPE.to_string())),
        )),
        ("core.game", "run") => Some((
            vec![(read, Type::Named("GameScene".to_string()))],
            Some(Type::String),
        )),
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
        ("core.encoding.json", "canonical" | "events") => {
            Some((vec![(read, json.clone())], Some(Type::String)))
        }
        ("core.encoding.jsonl", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::List(Box::new(json.clone())), json_error_ty())),
        )),
        ("core.encoding.jsonl", "to_string") => Some((
            vec![(read, Type::List(Box::new(json.clone())))],
            Some(Type::String),
        )),
        // jet.csv → core.encoding.csv: parse text into a list of rows (list of fields).
        ("core.encoding.csv", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::List(Box::new(Type::List(Box::new(Type::String)))),
                Type::String,
            )),
        )),
        ("core.encoding.csv", "to_string") => Some((
            vec![(
                read,
                Type::List(Box::new(Type::List(Box::new(Type::String)))),
            )],
            Some(Type::String),
        )),
        // D-DATA-SURFACE1=A / D-DATA-PLOT1=A / D-DATA-STATUS1=A: core.data
        // facade fixed-shape calls. Generic typed table calls are handled in
        // infer_core_call so selectors stay typed by sema.
        ("core.data", "sum" | "mean" | "min" | "max" | "median" | "variance" | "stddev") => Some((
            vec![(read, Type::List(Box::new(Type::Float)))],
            Some(Type::Float),
        )),
        ("core.data", "quantile") => Some((
            vec![(read, Type::List(Box::new(Type::Float))), (read, Type::Float)],
            Some(Type::Float),
        )),
        ("core.data", "rolling_mean") => Some((
            vec![(read, Type::List(Box::new(Type::Float))), (read, Type::Int)],
            Some(Type::List(Box::new(Type::Float))),
        )),
        ("core.data", "describe") => Some((
            vec![(read, Type::List(Box::new(Type::Float)))],
            Some(Type::Named("DataSummary".to_string())),
        )),
        ("core.data", "status") => Some((
            vec![],
            Some(Type::List(Box::new(Type::Named("DataStatus".to_string())))),
        )),
        ("core.data", "bar_text" | "bar_svg") => Some((
            vec![(
                read,
                Type::List(Box::new(Type::Named("DataGroup".to_string()))),
            )],
            Some(Type::String),
        )),
        ("core.fmt", "number" | "bytes" | "duration" | "ordinal") => {
            Some((vec![(read, Type::Int)], Some(Type::String)))
        }
        ("core.fmt", "decimal" | "percent") => Some((
            vec![(read, Type::Float), (read, Type::Int)],
            Some(Type::String),
        )),
        ("core.fmt", "plural") => Some((
            vec![(read, Type::Int), (read, Type::String), (read, Type::String)],
            Some(Type::String),
        )),
        ("core.fmt", "pad_left" | "pad_right" | "pad_center") => Some((
            vec![(read, Type::String), (read, Type::Int), (read, Type::String)],
            Some(Type::String),
        )),
        // D-ENC-DYN1=A+ (c152): TOML is a full adapter over the rich `Data` value —
        // `parse` returns `Toml` (= `Data`); `to_string` takes any encodable value.
        ("core.encoding.toml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("core.encoding.toml", "to_string") => {
            Some((vec![(read, json.clone())], Some(Type::String)))
        }
        // D-ENC-YAML1 = A (c152): YAML is a full adapter over the rich `Data` value.
        ("core.encoding.yaml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("core.encoding.yaml", "to_string") => {
            Some((vec![(read, json.clone())], Some(Type::String)))
        }
        ("core.encoding.xml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), Type::String)),
        )),
        ("core.encoding.xml", "to_string") => Some((vec![(read, json.clone())], Some(Type::String))),
        ("core.encoding.cbor", "encode") => Some((
            vec![(read, json.clone())],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.encoding.cbor", "decode") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(json.clone(), Type::String)),
        )),
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
        ("core.path", "parent" | "extension" | "normalize") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        // D-URL1=A: typed URLs, query strings, component escaping, and MIME values.
        ("core.url", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Url".to_string()), Type::String)),
        )),
        ("core.url", "from_parts") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::String),
                (
                    read,
                    Type::List(Box::new(Type::List(Box::new(Type::String)))),
                ),
                (read, Type::String),
            ],
            Some(result_ty(Type::Named("Url".to_string()), Type::String)),
        )),
        ("core.url", "file") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("Url".to_string())),
        )),
        ("core.url", "data") => Some((
            vec![
                (read, Type::Named("Mime".to_string())),
                (read, Type::String),
            ],
            Some(Type::Named("Url".to_string())),
        )),
        ("core.url", "query") => Some((
            vec![(
                read,
                Type::List(Box::new(Type::List(Box::new(Type::String)))),
            )],
            Some(Type::String),
        )),
        ("core.url", "percent_encode") => Some((vec![(read, Type::String)], Some(Type::String))),
        ("core.url", "percent_decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.mime", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Mime".to_string()), Type::String)),
        )),
        ("core.mime", "from_extension" | "extension") => Some((
            vec![(read, Type::String)],
            Some(Type::Option(Box::new(Type::String))),
        )),
        // D-TEXTUNICODE1: std-only Unicode scalar helpers.
        ("core.text.unicode", "scalar_count" | "byte_count") => {
            Some((vec![(read, Type::String)], Some(Type::Int)))
        }
        ("core.text.unicode", "is_ascii") => Some((vec![(read, Type::String)], Some(Type::Bool))),
        ("core.text.unicode", "lower" | "upper") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        ("core.text.unicode", "scalars") => Some((
            vec![(read, Type::String)],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.text", "nfc" | "nfd" | "nfkc" | "nfkd" | "casefold" | "lower" | "upper") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        ("core.text", "caseless_eq") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(Type::Bool),
        )),
        ("core.text", "graphemes" | "words" | "sentences" | "scalars") => Some((
            vec![(read, Type::String)],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.text", "width" | "scalar_count" | "byte_count") => {
            Some((vec![(read, Type::String)], Some(Type::Int)))
        }
        ("core.text", "is_alphabetic" | "is_numeric" | "is_whitespace" | "is_ascii") => {
            Some((vec![(read, Type::String)], Some(Type::Bool)))
        }
        ("core.text", "splitn" | "rsplitn") => Some((
            vec![(read, Type::String), (read, Type::String), (read, Type::Int)],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.text", "trim" | "trim_start" | "trim_end") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        ("core.text", "pad_start" | "pad_end" | "center") => Some((
            vec![(read, Type::String), (read, Type::Int), (read, Type::String)],
            Some(Type::String),
        )),
        ("core.text", "starts_any" | "ends_any") => Some((
            vec![(read, Type::String), (read, Type::List(Box::new(Type::String)))],
            Some(Type::Bool),
        )),
        ("core.text", "char_indices") => Some((
            vec![(read, Type::String)],
            Some(Type::List(Box::new(Type::String))),
        )),
        // jet.log/core.log: structured logging, typed fields, spans, sinks.
        ("jet.log", "info" | "warn" | "error" | "debug") => {
            Some((vec![(read, string.clone())], None))
        }
        ("jet.log", "field") => Some((
            vec![(read, string.clone()), (read, string.clone())],
            Some(Type::Named("LogField".to_string())),
        )),
        ("jet.log", "int") => Some((
            vec![(read, string.clone()), (read, Type::Int)],
            Some(Type::Named("LogField".to_string())),
        )),
        ("jet.log", "float") => Some((
            vec![(read, string.clone()), (read, Type::Float)],
            Some(Type::Named("LogField".to_string())),
        )),
        ("jet.log", "bool") => Some((
            vec![(read, string.clone()), (read, Type::Bool)],
            Some(Type::Named("LogField".to_string())),
        )),
        ("jet.log", "redact") => Some((
            vec![(read, string.clone())],
            Some(Type::Named("LogField".to_string())),
        )),
        ("jet.log", "info_fields" | "warn_fields" | "error_fields" | "debug_fields") => Some((
            vec![
                (read, string.clone()),
                (read, Type::List(Box::new(Type::Named("LogField".to_string())))),
            ],
            None,
        )),
        ("jet.log", "span") => Some((
            vec![(read, string.clone())],
            Some(Type::Named("LogSpan".to_string())),
        )),
        ("jet.log", "enter" | "close") => {
            Some((vec![(read, Type::Named("LogSpan".to_string()))], None))
        }
        ("jet.log", "set_sink") => {
            Some((vec![(read, string.clone()), (read, string.clone())], None))
        }
        ("jet.log", "sample_every") => Some((vec![(read, Type::Int)], None)),
        ("jet.log", "counter") => Some((
            vec![(read, string.clone()), (read, Type::Int)],
            Some(Type::Named("LogField".to_string())),
        )),
        ("jet.log", "otlp_file") => Some((vec![(read, string.clone())], None)),
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
        ("jet.crypto", "sha256") => Some((vec![(read, Type::String)], Some(Type::String))),
        ("jet.crypto", "sha256_bytes") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::String),
        )),
        ("jet.crypto", "sha512_bytes" | "blake3_bytes") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::String),
        )),
        ("jet.crypto", "constant_time_eq") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(Type::Bool),
        )),
        ("jet.crypto", "hkdf_sha256") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::Int),
            ],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("jet.crypto", "x25519_public") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("jet.crypto", "x25519_shared") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("jet.crypto", "password_hash") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("jet.crypto", "password_hash_with_salt") => Some((
            vec![(read, Type::String), (read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("jet.crypto", "password_verify") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(Type::Bool),
        )),
        // D-CRYPTOENV1=A: misuse-resistant envelope (RustCrypto via FFI bridge).
        ("jet.crypto", "seal" | "file_seal") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("jet.crypto", "open" | "file_open") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("jet.crypto", "sign") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("jet.crypto", "verify") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(unit_ty(), Type::String)),
        )),
        // D-CRYPTOENV1=A: expert-only raw AEAD (requires #Unsafe + expert import).
        ("core.crypto.expert", "aes256_gcm_seal" | "chacha20_seal") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("core.crypto.expert", "aes256_gcm_open" | "chacha20_open") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        // E2-M10: core.net — blocking TCP/UDP sockets (std::net, zero external deps).
        ("core.net", "tcp_listen") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("TcpListener".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "ip_addr") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("IpAddr".to_string()), Type::String)),
        )),
        ("core.net", "ip_to_string") => Some((
            vec![(read, Type::Named("IpAddr".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "ip_is_ipv4") => Some((
            vec![(read, Type::Named("IpAddr".to_string()))],
            Some(Type::Bool),
        )),
        ("core.net", "socket_addr") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(
                Type::Named("SocketAddr".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "socket_addr_parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("SocketAddr".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "socket_host" | "socket_to_string") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "socket_port") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(Type::Int),
        )),
        ("core.net", "tcp_listen_addr") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(result_ty(
                Type::Named("TcpListener".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "tcp_accept") => Some((
            vec![(
                AccessConvention::Read,
                Type::Named("TcpListener".to_string()),
            )],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "tcp_connect") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "tcp_connect_addr") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "tcp_connect_timeout") => Some((
            vec![
                (read, Type::Named("SocketAddr".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "tcp_connect_happy") => Some((
            vec![(read, Type::String), (read, Type::Int), (read, Type::Int)],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "tcp_read") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("TcpStream".to_string()),
            )],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.net", "tcp_write") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::String)),
        )),
        ("core.net", "tcp_local_addr" | "tcp_peer_addr") => Some((
            vec![(read, Type::Named("TcpStream".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "tcp_local_socket_addr" | "tcp_peer_socket_addr") => Some((
            vec![(read, Type::Named("TcpStream".to_string()))],
            Some(Type::Named("SocketAddr".to_string())),
        )),
        ("core.net", "listener_local_socket_addr") => Some((
            vec![(read, Type::Named("TcpListener".to_string()))],
            Some(Type::Named("SocketAddr".to_string())),
        )),
        ("core.net", "set_timeout") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::Int),
            ],
            None,
        )),
        ("core.net", "set_read_timeout" | "set_write_timeout") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::Int),
            ],
            Some(result_ty(unit_ty(), Type::String)),
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
        ("core.net", "udp_bind") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("UdpSocket".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "udp_bind_addr") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(result_ty(
                Type::Named("UdpSocket".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "udp_local_addr") => Some((
            vec![(read, Type::Named("UdpSocket".to_string()))],
            Some(Type::Named("SocketAddr".to_string())),
        )),
        ("core.net", "udp_set_timeout") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(unit_ty(), Type::String)),
        )),
        ("core.net", "udp_send_to") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::String),
                (read, Type::Named("SocketAddr".to_string())),
            ],
            Some(result_ty(Type::Int, Type::String)),
        )),
        ("core.net", "udp_recv_from") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("UdpPacket".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "udp_packet_data") => Some((
            vec![(read, Type::Named("UdpPacket".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "udp_packet_addr") => Some((
            vec![(read, Type::Named("UdpPacket".to_string()))],
            Some(Type::Named("SocketAddr".to_string())),
        )),
        ("core.net", "unix_listen") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("UnixListener".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "unix_accept") => Some((
            vec![(read, Type::Named("UnixListener".to_string()))],
            Some(result_ty(
                Type::Named("UnixStream".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "unix_connect") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("UnixStream".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "unix_read") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("UnixStream".to_string()),
            )],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.net", "unix_write") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("UnixStream".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::String)),
        )),
        ("core.net", "dns_a" | "dns_aaaa") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("IpAddr".to_string()))),
                Type::String,
            )),
        )),
        ("core.net", "dns_a_at" | "dns_aaaa_at") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(Type::Named("IpAddr".to_string()))),
                Type::String,
            )),
        )),
        ("core.net", "dns_txt") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(Type::List(Box::new(Type::String)), Type::String)),
        )),
        ("core.net", "dns_txt_at") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(Type::List(Box::new(Type::String)), Type::String)),
        )),
        ("core.net", "dns_srv") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("DnsSrv".to_string()))),
                Type::String,
            )),
        )),
        ("core.net", "dns_srv_at") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(Type::Named("DnsSrv".to_string()))),
                Type::String,
            )),
        )),
        ("core.net", "dns_srv_target") => Some((
            vec![(read, Type::Named("DnsSrv".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "dns_srv_port" | "dns_srv_priority" | "dns_srv_weight") => Some((
            vec![(read, Type::Named("DnsSrv".to_string()))],
            Some(Type::Int),
        )),
        ("core.net", "tls_connect") => Some((
            vec![
                (AccessConvention::Move, Type::Named("TcpStream".to_string())),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("TlsStream".to_string()),
                Type::String,
            )),
        )),
        ("core.net", "tls_read") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("TlsStream".to_string()),
            )],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.net", "tls_write") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TlsStream".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::String)),
        )),
        ("core.net", "tls_close") => Some((
            vec![(AccessConvention::Move, Type::Named("TlsStream".to_string()))],
            Some(result_ty(unit_ty(), Type::String)),
        )),
        // E2-M10: jet.http — HTTP client/server over blocking I/O.
        // GET / HEAD / DELETE requests (no body sent).
        ("jet.http", "get") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("HttpResponse".to_string()),
                Type::String,
            )),
        )),
        // POST / PUT / PATCH requests (body sent).
        ("jet.http", "post") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(
                Type::Named("HttpResponse".to_string()),
                Type::String,
            )),
        )),
        // serve blocks until the listener is closed; handler is called per request.
        // The handler type is resolved at the call site (lambda / fn pointer).
        ("jet.http", "serve") => None, // special-cased in check_core_call
        // D-REGEXENGINE1=A: core.regex — std-only linear regex. Every parsing
        // call returns a Result; the `Err` is a bad-pattern message at the boundary.
        ("jet.regex", "flags") => Some((
            vec![(read, Type::Bool), (read, Type::Bool), (read, Type::Bool)],
            Some(Type::Named("RegexFlags".to_string())),
        )),
        ("jet.regex", "compile") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Regex".to_string()), Type::String)),
        )),
        ("jet.regex", "compile_with") => Some((
            vec![
                (read, Type::String),
                (read, Type::Named("RegexFlags".to_string())),
            ],
            Some(result_ty(Type::Named("Regex".to_string()), Type::String)),
        )),
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
            Some(result_ty(Type::List(Box::new(Type::String)), Type::String)),
        )),
        ("jet.regex", "matches") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("Match".to_string()))),
                Type::String,
            )),
        )),
        ("jet.regex", "split_limit") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(Type::List(Box::new(Type::String)), Type::String)),
        )),
        ("jet.regex", "replace" | "replace_all") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::String),
            ],
            Some(result_ty(Type::String, Type::String)),
        )),
        // D-DEP-ARCHIVE1=A: core.archive — gzip compress/decompress via the `flate2` crate FFI bridge.
        // Both functions take `[U8]` and return `[U8]`. Compression is infallible; decompression
        // returns an empty list on corrupt input (no error path exposed to the caller).
        ("core.archive", "gzip_compress" | "gzip_decompress") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: zip_compress — create a single-entry zip archive.
        // Takes (name: String, data: [U8]) → [U8].
        ("core.archive", "zip_compress") => Some((
            vec![(read, Type::String), (read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: zip_decompress — extract first entry from a zip archive.
        // Takes [U8] → [U8]. Returns empty list on invalid input.
        ("core.archive", "zip_decompress") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: tar_add — append/replace a named entry in a tar archive.
        // Takes (archive: [U8], name: String, data: [U8]) → [U8].
        ("core.archive", "tar_add") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::String),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: tar_get — extract a named entry from a tar archive.
        // Takes (archive: [U8], name: String) → [U8]. Empty on not-found or bad input.
        ("core.archive", "tar_get") => Some((
            vec![(read, Type::List(Box::new(u8_ty()))), (read, Type::String)],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: tar_names_json — list entry names as a JSON array string.
        // Takes [U8] → String. Returns "[]" on empty or invalid archive.
        ("core.archive", "tar_names_json") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::String),
        )),
        // D-RAYLIB1=A / D-FLAGSHIP-RAYLIB1=A: first bounded `core.raylib`
        // bridge. The surface is intentionally tiny and display-gated.
        ("core.raylib", "window_open") => Some((
            vec![(read, Type::Int), (read, Type::Int), (read, Type::String)],
            Some(Type::Named("RaylibWindow".to_string())),
        )),
        ("core.raylib", "window_should_close") => Some((
            vec![(read, Type::Named("RaylibWindow".to_string()))],
            Some(Type::Bool),
        )),
        ("core.raylib", "begin_drawing") => {
            Some((vec![(read, Type::Named("RaylibWindow".to_string()))], None))
        }
        ("core.raylib", "clear_background") => {
            Some((vec![(read, Type::Named("RaylibColor".to_string()))], None))
        }
        ("core.raylib", "draw_text") => Some((
            vec![
                (read, Type::String),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Named("RaylibColor".to_string())),
            ],
            None,
        )),
        ("core.raylib", "end_drawing") => Some((vec![], None)),
        ("core.raylib", "close_window") => {
            Some((vec![(read, Type::Named("RaylibWindow".to_string()))], None))
        }
        ("core.raylib", "color") => Some((
            vec![
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
            ],
            Some(Type::Named("RaylibColor".to_string())),
        )),
        // D-CODECS1: core.compress.gzip / core.compress.zstd — standalone codec APIs,
        // separate from core.archive. `compress` takes `[U8]` and is infallible;
        // `decompress` is fallible (malformed compressed stream → `Err(String)`),
        // following the same house style as core.encoding.hex/base64 `decode`.
        ("core.compress.gzip", "compress") | ("core.compress.zstd", "compress") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.compress.gzip", "decompress") | ("core.compress.zstd", "decompress") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        // D-DBDRIVER1: jet.db — SQLite via rusqlite (bundled). `open`/`open_memory`
        // are the only module-level entry points; they PRODUCE a `DbConnection`
        // handle (mirrors `core.files`'s `open`/`create` producing a `FileReader`/
        // `FileWriter`). Every other operation — `query`/`query_one`/`execute`/
        // `begin`/`commit`/`rollback`/`close` — is an INSTANCE method dispatched
        // by the receiver's `DbConnection` type (see `check_db_connection_method`
        // below), not a second module-call surface. There is no raw-string
        // `execute(sql)` escape (D-DBDRIVER1's build plan: "must not expose a
        // generic `execute_raw(sql)` escape").
        ("jet.db", "open") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("DbConnection".to_string())),
        )),
        ("jet.db", "open_memory") => Some((vec![], Some(Type::Named("DbConnection".to_string())))),
        ("jet.db", "params") => Some((
            vec![(read, Type::Named("Sql".to_string()))],
            Some(Type::List(Box::new(Type::Named(Syntax::TYPE_DB_VALUE.to_string())))),
        )),
        ("jet.db", "row_value") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::Named(Syntax::TYPE_DB_VALUE.to_string())),
                err: Box::new(Type::String),
            }),
        )),
        ("jet.db", "row_int") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::Int),
                err: Box::new(Type::String),
            }),
        )),
        ("jet.db", "row_float") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::Float),
                err: Box::new(Type::String),
            }),
        )),
        ("jet.db", "row_text") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::String),
            }),
        )),
        ("jet.db", "row_bool") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::Bool),
                err: Box::new(Type::String),
            }),
        )),
        ("jet.db", "transaction") | ("jet.db", "migrate") => Some((
            vec![
                (read, Type::Named("DbConnection".to_string())),
                (read, Type::String),
                (read, Type::List(Box::new(Type::String))),
            ],
            Some(result_ty(Type::Int, db_error_ty())),
        )),
        // D-DEP-WASM1=A / D-PLUGIN1=B (c81): `core.plugin` — sandboxed WASM
        // Component Model plugin loader (wasmtime, runtime-side only, I6).
        // `load` is the only module-level entry point; it PRODUCES a `Plugin`
        // handle (mirrors `jet.db`'s `open` producing a `DbConnection`). The
        // actual calls (`.call`/`.call_int`) are instance methods dispatched by
        // the receiver's `Plugin` type (see `check_plugin_method` below).
        ("jet.plugin", "load") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("Plugin".to_string())),
        )),
        // D-UUIDENC1=A: hex and base64 codecs. `encode` is infallible; `decode`
        // returns `[Byte] ? String` (invalid input → Err).
        ("core.encoding.hex", "encode") => {
            Some((vec![(read, list_u8.clone())], Some(Type::String)))
        }
        ("core.encoding.hex", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        ("core.encoding.base64", "encode") => {
            Some((vec![(read, list_u8.clone())], Some(Type::String)))
        }
        ("core.encoding.base64", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        ("core.encoding.base64", "encode_url") => {
            Some((vec![(read, list_u8.clone())], Some(Type::String)))
        }
        ("core.encoding.base64", "decode_url") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        ("core.encoding.base32", "encode") => {
            Some((vec![(read, list_u8.clone())], Some(Type::String)))
        }
        ("core.encoding.base32", "decode") => Some((
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
        // D-OPTGC1: run a mark-sweep collection over traced `Gc<T>` roots.
        ("core.gc", "collect") => Some((vec![], Some(unit))),
        ("core.args", "spec") => Some((vec![], Some(Type::Named("ArgsSpec".to_string())))),
        // D-TERM1 (ratified 2026-06-22): terminal direct-input.
        // `term.read_key()` → `Key` (the key-event enum). No arguments.
        ("core.term", "read_key") => Some((
            vec![],
            Some(Type::Named(crate::Syntax::TYPE_KEY.to_string())),
        )),
        // D-FIDELITY-API1=A: runtime-global fidelity signal.
        ("core.perf", "fidelity") => Some((vec![], Some(float))),
        ("core.perf", "default_fidelity") => Some((vec![], Some(float.clone()))),
        ("core.perf", "override_fidelity") => Some((
            vec![(read, float)],
            Some(result_ty(unit.clone(), Type::String)),
        )),
        ("core.perf", "reset_fidelity") => Some((vec![], Some(unit))),
        // D-DECIMAL1: exact decimal parse from string.
        ("core.numeric", "decimal") => Some((
            vec![(read, string.clone())],
            Some(Type::Named(crate::Syntax::TYPE_DECIMAL.to_string())),
        )),
        // D-RENDERTGT2=A (c133 M1): UI geometry constructors.
        ("core.ui", "null_backend") => Some((vec![], Some(Type::Named("NullBackend".to_string())))),
        ("core.ui", "tui_backend") => Some((vec![], Some(Type::Named("TuiBackend".to_string())))),
        // D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend constructor.
        ("core.ui", "gtk_backend") => Some((vec![], Some(Type::Named("GtkBackend".to_string())))),
        ("core.ui", "point") => Some((
            vec![(read, float.clone()), (read, float)],
            Some(Type::Named("Point".to_string())),
        )),
        ("core.ui", "size") => Some((
            vec![(read, float.clone()), (read, float)],
            Some(Type::Named("Size".to_string())),
        )),
        ("core.ui", "rect") => Some((
            vec![
                (read, float.clone()),
                (read, float.clone()),
                (read, float.clone()),
                (read, float),
            ],
            Some(Type::Named("Rect".to_string())),
        )),
        ("core.ui", "constraint") => Some((
            vec![
                (read, float.clone()),
                (read, float.clone()),
                (read, float.clone()),
                (read, float),
            ],
            Some(Type::Named("SizeConstraint".to_string())),
        )),
        ("core.ui", "node") => Some((
            vec![(read, string.clone()), (read, float.clone()), (read, float)],
            Some(Type::Named("UiNode".to_string())),
        )),
        ("core.ui", "key_event") => Some((
            vec![(read, string)],
            Some(Type::Named("InputEvent".to_string())),
        )),
        ("core.ui", "resize_event") => Some((
            vec![(read, float.clone()), (read, float)],
            Some(Type::Named("InputEvent".to_string())),
        )),
        // D-A11YGATE1=B (c134 Phase 6): accessible-role node constructor + role
        // constants. `node_role` is the a11y-checked sibling of `node` — it's the
        // only UiNode constructor that carries a role, so it's the only one E2930
        // (unlabeled interactive control) needs to watch.
        ("core.ui", "node_role") => Some((
            vec![
                (read, string),
                (read, float.clone()),
                (read, float),
                (read, Type::Named("UiAriaRole".to_string())),
            ],
            Some(Type::Named("UiNode".to_string())),
        )),
        // D-STYLESHAPE1=A wiring: a node with an explicit fill color (a `#RRGGBB`
        // string, matching `JetPaintCmd::FillRect`'s existing color representation —
        // no new opaque type needed, this just makes the field settable from Jet).
        ("core.ui", "node_color") => Some((
            vec![
                (read, string.clone()),
                (read, float.clone()),
                (read, float),
                (read, string),
            ],
            Some(Type::Named("UiNode".to_string())),
        )),
        (
            "core.ui",
            "aria_role_button" | "aria_role_text_input" | "aria_role_label" | "aria_role_container",
        ) => Some((vec![], Some(Type::Named("UiAriaRole".to_string())))),
        // D-FLAGSHIP-WEBAPI1=A: first-party browser API for web flagship slices.
        ("core.web", "on") => Some((
            vec![
                (read, string.clone()),
                (read, string.clone()),
                (
                    read,
                    Type::Fn {
                        params: vec![Type::Named("WebEvent".to_string())],
                        ret: None,
                        effect_bound: None,
                    },
                ),
            ],
            None,
        )),
        ("core.web", "value") => Some((vec![(read, string.clone())], Some(Type::String))),
        ("core.web.storage.local" | "core.web.storage.session", "get") => Some((
            vec![(read, string.clone())],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.web.storage.local" | "core.web.storage.session", "set") => {
            Some((vec![(read, string.clone()), (read, string.clone())], None))
        }
        ("core.web.storage.local" | "core.web.storage.session", "remove") => {
            Some((vec![(read, string)], None))
        }
        ("core.web.storage.local" | "core.web.storage.session", "clear") => Some((vec![], None)),
        // c-devserver (owner-directed 2026-07-01): `devserver.for_app(file)` —
        // the constructor for a configurable `jet dev` server value. The
        // builder methods (`.html`/`.port`/`.serve`) are instance methods on
        // `DevServer`, dispatched through `devserver_method_return` (mirrors
        // `ui_backend_method_return`), not module-level names here.
        ("core.devserver", "for_app") => Some((
            vec![(read, string)],
            Some(Type::Named("DevServer".to_string())),
        )),
        // `devserver.app()` — zero-arg: watch the file `jet dev` launched
        // (passed to the running program via JET_DEV_FILE). The common case:
        // the file defining `fn dev()` is the file to watch, so no path is
        // spelled out at all.
        ("core.devserver", "app") => Some((vec![], Some(Type::Named("DevServer".to_string())))),
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
        ("flag_short", 3) => Some(Some(spec_ty)),
        // .option("name", "help text", "METAVAR") → ArgsSpec  (value option)
        ("option", 3) => Some(Some(spec_ty)),
        ("option_short", 4)
        | ("option_default", 4)
        | ("option_env", 4)
        | ("option_choice", 4) => Some(Some(spec_ty)),
        ("option_int", 3)
        | ("option_float", 3)
        | ("repeat", 3)
        | ("required_option", 3) => Some(Some(spec_ty)),
        // .positional("name", "help text") → ArgsSpec  (positional argument)
        ("positional", 2) => Some(Some(spec_ty)),
        ("subcommand", 3) => Some(Some(spec_ty)),
        ("version", 1) => Some(Some(spec_ty)),
        ("completion", 1) => Some(Some(Type::String)),
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
        ("flag_short", _) => {
            diags.push(Diagnostic::error(
                "E1301",
                format!(
                    "`flag_short` expects 3 arguments (name, short, help), got {}",
                    n_args
                ),
                "`ArgsSpec.flag_short(name, short, help)` registers a boolean flag with a `-v` alias".to_string(),
                "pass exactly three strings: long name, one-letter short name, and help text".to_string(),
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
        (
            "option_short"
            | "option_default"
            | "option_env"
            | "option_int"
            | "option_float"
            | "option_choice"
            | "repeat"
            | "required_option",
            _,
        ) => {
            diags.push(Diagnostic::error(
                "E1302",
                format!("`{}` was called with the wrong number of arguments", method),
                "`core.args` option builders declare long names, help text, and a value name; variants add one extra string where needed".to_string(),
                "check the `core.args` builder signature and pass the required strings".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("positional", _) => {
            diags.push(Diagnostic::error(
                "E1303",
                format!(
                    "`positional` expects 2 arguments (name, help), got {}",
                    n_args
                ),
                "`ArgsSpec.positional(name, help)` registers a required positional argument"
                    .to_string(),
                "pass exactly two strings: the positional name and a help description".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("subcommand", _) => {
            diags.push(Diagnostic::error(
                "E1303",
                format!(
                    "`subcommand` expects 3 arguments (name, help, spec), got {}",
                    n_args
                ),
                "`ArgsSpec.subcommand(name, help, spec)` gives a subcommand its own nested ArgsSpec".to_string(),
                "pass the subcommand name, help text, and an ArgsSpec built with `args.spec()`".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("version" | "completion", _) => {
            diags.push(Diagnostic::error(
                "E1303",
                format!("`{}` expects 1 argument, got {}", method, n_args),
                "`version(text)` configures `--version`; `completion(shell)` renders shell completion text".to_string(),
                "pass exactly one string".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("parse", _) => {
            diags.push(Diagnostic::error(
                "E1304",
                format!("`parse` expects 1 argument (argv), got {}", n_args),
                "`ArgsSpec.parse(argv)` parses a `[String]` (from `io.args()`) against the spec"
                    .to_string(),
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
        ("option_int", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("option_float", 1) => Some(Some(Type::Option(Box::new(Type::Float)))),
        ("options", 1) => Some(Some(Type::List(Box::new(Type::String)))),
        // .positional(n) → String?
        ("positional", 1) => Some(Some(Type::Option(Box::new(Type::String)))),
        ("subcommand", 0) => Some(Some(Type::Option(Box::new(Type::String)))),
        ("flag", _) => {
            diags.push(Diagnostic::error(
                "E1301",
                format!(
                    "`ParsedArgs.flag` expects 1 argument (flag name), got {}",
                    n_args
                ),
                "`parsed.flag(\"verbose\")` returns `true` when `--verbose` was passed".to_string(),
                "pass exactly one string: the flag name (without leading `--`)".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("option", _) => {
            diags.push(Diagnostic::error(
                "E1302",
                format!(
                    "`ParsedArgs.option` expects 1 argument (option name), got {}",
                    n_args
                ),
                format!(
                    "`parsed.option(\"output\")` returns the value of `--output VALUE`, or `{}`",
                    Syntax::LIT_NULL
                ),
                "pass exactly one string: the option name (without leading `--`)".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("option_int" | "option_float" | "options", _) => {
            diags.push(Diagnostic::error(
                "E1302",
                format!(
                    "`ParsedArgs.{}` expects 1 argument (option name), got {}",
                    method, n_args
                ),
                "`ParsedArgs` typed option queries read values already validated by `ArgsSpec.parse`".to_string(),
                "pass exactly one string: the option name (without leading `--`)".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("positional", _) => {
            diags.push(Diagnostic::error(
                "E1303",
                format!(
                    "`ParsedArgs.positional` expects 1 argument (index), got {}",
                    n_args
                ),
                format!(
                    "`parsed.positional(0)` returns the first positional argument, or `{}`",
                    Syntax::LIT_NULL
                ),
                "pass exactly one Int: the zero-based positional argument index".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("subcommand", _) => {
            diags.push(Diagnostic::error(
                "E1303",
                format!("`ParsedArgs.subcommand` expects 0 arguments, got {}", n_args),
                "`ParsedArgs.subcommand()` returns the matched subcommand name, if any".to_string(),
                "call it with no arguments".to_string(),
                Some(span),
            ));
            Some(None)
        }
        _ => None,
    }
}

/// D-PROCESS1: type-check `ProcessSpec` builder/run/spawn methods.
pub(crate) fn process_spec_method_return(
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let spec_ty = Type::Named("ProcessSpec".to_string());
    match (method, n_args) {
        ("cwd" | "env_remove" | "stdin_text" | "stdout" | "stderr", 1) => Some(Some(spec_ty)),
        ("env", 2) => Some(Some(spec_ty)),
        (
            "env_clear" | "stdout_capture" | "stdout_inherit" | "stdout_discard" | "stderr_capture"
            | "stderr_inherit" | "stderr_discard" | "detached",
            0,
        ) => Some(Some(spec_ty)),
        ("timeout_ms" | "output_limit", 1) => Some(Some(spec_ty)),
        ("run", 0) => Some(Some(result_ty(
            Type::Named("ProcessResult".to_string()),
            io_error_ty(),
        ))),
        ("spawn", 0) => Some(Some(result_ty(
            Type::Named("ProcessChild".to_string()),
            io_error_ty(),
        ))),
        ("cwd" | "env_remove" | "stdin_text" | "stdout" | "stderr", _) => {
            diags.push(wrong_core_arity(method, 1, n_args, span));
            Some(None)
        }
        ("env", _) => {
            diags.push(wrong_core_arity(method, 2, n_args, span));
            Some(None)
        }
        (
            "env_clear" | "stdout_capture" | "stdout_inherit" | "stdout_discard" | "stderr_capture"
            | "stderr_inherit" | "stderr_discard" | "detached" | "run" | "spawn",
            _,
        ) => {
            diags.push(wrong_core_arity(method, 0, n_args, span));
            Some(None)
        }
        ("timeout_ms" | "output_limit", _) => {
            diags.push(wrong_core_arity(method, 1, n_args, span));
            Some(None)
        }
        _ => None,
    }
}

/// D-PROCESS1: type-check `ProcessChild` streaming/control methods.
pub(crate) fn process_child_method_return(
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let io = io_error_ty();
    match (method, n_args) {
        ("id", 0) => Some(Some(Type::Int)),
        ("wait", 0) => Some(Some(result_ty(
            Type::Named("ProcessResult".to_string()),
            io.clone(),
        ))),
        ("kill" | "terminate" | "interrupt", 0) => Some(Some(result_ty(unit_ty(), io.clone()))),
        ("write_stdin", 1) => Some(Some(result_ty(unit_ty(), io.clone()))),
        ("read_stdout_line" | "read_stderr_line", 0) => {
            Some(Some(result_ty(Type::Option(Box::new(Type::String)), io)))
        }
        ("write_stdin", _) => {
            diags.push(wrong_core_arity(method, 1, n_args, span));
            Some(None)
        }
        (
            "id" | "wait" | "kill" | "terminate" | "interrupt" | "read_stdout_line"
            | "read_stderr_line",
            _,
        ) => {
            diags.push(wrong_core_arity(method, 0, n_args, span));
            Some(None)
        }
        _ => None,
    }
}

/// D-RENDERTGT2=A (c133 M1/M2): type-check method calls on UI backends.
pub(crate) fn ui_backend_method_return(
    backend: &str,
    method: &str,
    n_args: usize,
    _span: Span,
    _diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let size_ty = Type::Named("Size".to_string());
    let unit = unit_ty();
    match (backend, method, n_args) {
        ("NullBackend" | "TuiBackend" | "GtkBackend", "measure", 2) => Some(Some(size_ty)),
        ("NullBackend" | "TuiBackend" | "GtkBackend", "layout", 2) => Some(Some(unit)),
        ("NullBackend" | "TuiBackend" | "GtkBackend", "paint", 1) => Some(Some(unit)),
        ("NullBackend" | "TuiBackend" | "GtkBackend", "on_event", 1) => {
            Some(Some(Type::Named("EventResult".to_string())))
        }
        ("NullBackend", "commands", 0) => Some(Some(Type::List(Box::new(Type::String)))),
        ("TuiBackend", "frame_lines", 0) => Some(Some(Type::List(Box::new(Type::String)))),
        ("TuiBackend", "render_count", 0) => Some(Some(Type::Int)),
        // D-A11YGATE1=B (c134 Phase 6): keyboard focus routing over a flat
        // list of interactive nodes.
        ("NullBackend" | "TuiBackend" | "GtkBackend", "set_focus_group", 1) => Some(Some(unit)),
        ("NullBackend" | "TuiBackend" | "GtkBackend", "focused_label", 0) => {
            Some(Some(Type::String))
        }
        // D-UIDEVSHELL1=A (c134 Phase 8): native GTK4 retained-widget surface.
        // `label`/`button` create a widget and return its handle; `set_text`/
        // `set_size`/`set_color` mutate a live widget; `on_click(id, handler)`
        // wires a button; `present(title)` opens the window (no-op headless).
        ("GtkBackend", "label", 1) => Some(Some(Type::Int)),
        ("GtkBackend", "button", 1) => Some(Some(Type::Int)),
        ("GtkBackend", "set_text", 2) => Some(Some(unit)),
        ("GtkBackend", "set_size", 3) => Some(Some(unit)),
        ("GtkBackend", "set_color", 2) => Some(Some(unit)),
        ("GtkBackend", "on_click", 2) => Some(Some(unit)),
        ("GtkBackend", "present", 1) => Some(Some(unit)),
        _ => None,
    }
}

/// c-devserver (owner-directed 2026-07-01): type-check builder method calls
/// on a `DevServer` value (`.html`/`.port`/`.serve`). `.html`/`.port` return
/// `DevServer` for chaining, but are equally valid as bare statements (they
/// are not `@MustUse`) — the reference example calls them as plain
/// statements without reassigning. `.serve()` blocks forever and returns
/// nothing.
pub(crate) fn devserver_method_return(
    method: &str,
    n_args: usize,
    _span: Span,
    _diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let devserver_ty = Type::Named("DevServer".to_string());
    let unit = unit_ty();
    match (method, n_args) {
        ("html", 1) => Some(Some(devserver_ty)),
        ("port", 1) => Some(Some(devserver_ty)),
        ("serve", 0) => Some(Some(unit)),
        _ => None,
    }
}

pub(crate) fn core_module_items(module: &str) -> Vec<String> {
    let normalized_module =
        Syntax::normalize_core_module(module).unwrap_or_else(|| module.to_string());
    let module = normalized_module.as_str();
    let items: &[&str] = match module {
        "core.io" => &[
            "args",
            "input",
            "read_all_input",
            "eprint",
            "stdin",
            "stdout",
            "stderr",
            "terminal_width",
            "terminal_height",
            "style",
            "style_force",
            "progress",
        ],
        "core.env" => &["get", "set", "current_dir", "home_dir"],
        "core.os" => &[
            "name",
            "family",
            "arch",
            "cpu_count",
            "temp_dir",
            "executable",
            "pid",
            "hostname",
            "username",
            "set_current_dir",
            "on_interrupt",
        ],
        "core.process" => &["exit", "run", "cmd", "pipeline"],
        "core.math" => &[
            "sqrt",
            "pow",
            "abs",
            "min",
            "max",
            "floor",
            "ceil",
            "round",
            "pi",
            "e",
            "tau",
            "infinity",
            "nan",
            "clamp",
            "sin",
            "cos",
            "tan",
            "asin",
            "acos",
            "atan",
            "atan2",
            "sinh",
            "cosh",
            "tanh",
            "exp",
            "ln",
            "log2",
            "log10",
            "hypot",
            "trunc",
            "fract",
            "sign",
            "is_nan",
            "is_inf",
            "is_finite",
            "to_bits",
            "from_bits",
            "degrees",
            "radians",
            "lerp",
            "checked_add",
            "checked_sub",
            "checked_mul",
            "checked_pow",
            "saturating_add",
            "saturating_sub",
            "saturating_mul",
            "wrapping_add",
            "wrapping_sub",
            "wrapping_mul",
            "int_pow",
            "gcd",
            "lcm",
        ],
        // D-DET1: `rng` builds a deterministic injected RNG capability.
        // D-RANDSPLIT1=A: `bytes(n)` returns n PRNG bytes — fast, NOT crypto-safe.
        "core.random" => &[
            "int",
            "float",
            "float_range",
            "bool",
            "normal",
            "exponential",
            "pick",
            "weighted_pick",
            "sample",
            "shuffle",
            "seed",
            "rng",
            "split",
            "bytes",
        ],
        // D-RANDSPLIT1=A: CSPRNG namespace — cryptographically secure random bytes.
        "core.crypto.random" => &["bytes"],
        // D-DET1: `clock` builds a deterministic injected Clock capability.
        // D-DET-CAPAPI: `ms`/`secs` mint a deterministic `Duration` value.
        "core.time" => &[
            "now",
            "sleep",
            "start",
            "clock",
            "ms",
            "secs",
            "seconds",
            "minutes",
            "hours",
            "instant",
            "now_utc",
            "from_unix_ms",
            "today",
            "parse_rfc3339",
            "local_time",
            "parse_time",
            "period",
            "period_days",
            "period_months",
            "period_years",
            "zone",
            "utc",
            "zoned",
            "zoned_local",
        ],
        // D-ENC1: unified serialization. `core.encoding` is the library root (no direct
        // verbs — formats are submodules); each format submodule carries the verbs.
        "core.encoding" => &[],
        // D-JSONVERB1: `to_string`/`to_string_pretty` (compact/pretty); `parse` → dynamic
        // JSON value; `decode` → lenient typed decode (D-JSON3). D-MIGRATE3=A:
        // `decode_traced<T>` rides alongside `decode` on every format.
        "core.encoding.json" => &[
            "parse",
            "decode",
            "decode_traced",
            "to_string",
            "to_string_pretty",
            "canonical",
            "events",
        ],
        "core.encoding.jsonl" => &["parse", "to_string"],
        // D-SERDE6: typed `decode<T>` rides every format submodule alongside `parse`.
        "core.encoding.csv" => &["parse", "decode", "decode_traced", "to_string"],
        "core.encoding.toml" => &["parse", "decode", "decode_traced", "to_string"],
        "core.encoding.yaml" => &["parse", "decode", "decode_traced", "to_string"],
        "core.encoding.xml" => &["parse", "to_string"],
        "core.encoding.cbor" => &["encode", "decode"],
        // D-UUIDENC1=A: hex/base64 codecs and UUID generator.
        "core.encoding.hex" => &["encode", "decode"],
        "core.encoding.base64" => &["encode", "decode", "encode_url", "decode_url"],
        "core.encoding.base32" => &["encode", "decode"],
        // D-TEXTUNICODE1: std-only Unicode scalar helpers.
        "core.text.unicode" => &[
            "scalar_count",
            "byte_count",
            "is_ascii",
            "lower",
            "upper",
            "scalars",
        ],
        "core.uuid" => &["v4", "v7"],
        "core.mem" => &[
            "Ptr",
            "from_addr",
            "volatile_read",
            "volatile_write",
            "address_of",
            "Arena",
            "Bump",
            "Pool",
            "Fixed",
        ],
        // D-ALLOC-C (ratified 2026-06-19): wider allocator API bucket.
        "core.mem.alloc" => &["Arena", "Bump", "Pool", "Fixed"],
        "core.gc" => &["Gc", "collect"],
        "core.solve" => &["Solver"],
        "core.game" => &["Scene", "Replay", "Backend", "Budgets", "run"],
        "core.data" => &[
            "csv",
            "count",
            "table",
            "rows",
            "series",
            "values",
            "missing_count",
            "lazy",
            "lazy_filter",
            "lazy_sort_by",
            "collect",
            "plan",
            "filter",
            "sort_by",
            "inner_join",
            "left_join",
            "pivot_sum",
            "sum",
            "mean",
            "min",
            "max",
            "median",
            "quantile",
            "rolling_mean",
            "variance",
            "stddev",
            "describe",
            "group_count",
            "group_sum",
            "group_mean",
            "status",
            "bar_text",
            "bar_svg",
        ],
        "core.tasks" => &["spawn", "channel", "after", "interval"],
        "core.testing" => &[
            "snap",
            "golden",
            "fixture",
            "temp_dir",
            "corpus",
            "fake_clock",
            "fake_rng",
            "bench_budget",
        ],
        // D-FILES-WRITE1 (merge, was `core.fs` + `core.files`): one module for
        // both whole-file convenience helpers and streaming handle constructors.
        // D-FILES-APPEND1=A: whole-file one-shot append is `append_all`, kept
        // distinct from the streaming handle's `.append(text)` method.
        "core.files" => &[
            "read",
            "read_bytes",
            "write",
            "append_all",
            "exists",
            "remove",
            "remove_dir",
            "remove_all",
            "list_dir",
            "create_dir",
            "create_dir_all",
            "is_dir",
            "copy",
            "copy_dir",
            "symlink",
            "read_link",
            "hard_link",
            "rename",
            "stat",
            "canonicalize",
            "absolute",
            "walk",
            "glob",
            "read_at",
            "write_at",
            "fsync",
            "write_atomic",
            "temp_dir",
            "temp_file",
            "lock",
            "open",
            "create",
            "append",
        ],
        "core.watcher" => &["files", "process_pid", "port", "set"],
        "core.path" => &["join", "parent", "extension", "normalize"],
        "core.url" => &[
            "parse",
            "from_parts",
            "file",
            "data",
            "query",
            "percent_encode",
            "percent_decode",
        ],
        "core.mime" => &["parse", "from_extension", "extension"],
        // D-DEFER1 option B: scope-exit guard.
        "core.scope" => &["guard"],
        // D-ARGS1 (ratified 2026-06-22): declarative CLI arg parsing.
        "core.args" => &["spec"],
        // D-ANY-JAI1 (c7jaiany §6): the runtime reflection floor.
        "core.reflect" => &["of"],
        // D-SHIFT1 (c7shift): `Reader.over(bytes)`/`Cursor.over(s)` are bare
        // static constructors (no import needed, D-PATHFS1's `Path.from`
        // shape) — these module entries exist for discoverability/docs only.
        "core.binary" => &["Reader"],
        "core.text" => &[
            "Cursor",
            "nfc",
            "nfd",
            "nfkc",
            "nfkd",
            "casefold",
            "caseless_eq",
            "lower",
            "upper",
            "graphemes",
            "words",
            "sentences",
            "width",
            "scalar_count",
            "byte_count",
            "is_alphabetic",
            "is_numeric",
            "is_whitespace",
            "is_ascii",
            "scalars",
            "splitn",
            "rsplitn",
            "trim",
            "trim_start",
            "trim_end",
            "pad_start",
            "pad_end",
            "center",
            "starts_any",
            "ends_any",
            "char_indices",
        ],
        "core.fmt" => &[
            "number",
            "decimal",
            "percent",
            "bytes",
            "duration",
            "ordinal",
            "plural",
            "pad_left",
            "pad_right",
            "pad_center",
        ],
        // D-TERM1 (ratified 2026-06-22): terminal direct-input primitive.
        "core.term" => &["read_key"],
        "core" => &[],
        // D-CORENS-CANON1: ring packages normalize to `jet.*` internal key via
        // normalize_core_module; `core.*` is only the user-facing spelling.
        "jet.log" => &[
            "info",
            "warn",
            "error",
            "debug",
            "field",
            "int",
            "float",
            "bool",
            "redact",
            "info_fields",
            "warn_fields",
            "error_fields",
            "debug_fields",
            "span",
            "enter",
            "close",
            "set_sink",
            "sample_every",
            "counter",
            "otlp_file",
            "set_level",
            "set_trace_id",
            "setup",
        ],
        "jet.crypto" => &[
            "sha256",
            "sha256_bytes",
            "sha512_bytes",
            "blake3_bytes",
            "constant_time_eq",
            "hkdf_sha256",
            "x25519_public",
            "x25519_shared",
            "password_hash",
            "password_hash_with_salt",
            "password_verify",
            "seal",
            "open",
            "file_seal",
            "file_open",
            "sign",
            "verify",
        ],
        // D-CRYPTOENV1=A: expert-only raw primitives — misuse lint at call site.
        "core.crypto.expert" => &[
            "aes256_gcm_seal",
            "aes256_gcm_open",
            "chacha20_seal",
            "chacha20_open",
        ],
        // D-TTLVAL1=A: TTL-wrapped values and rotting secrets.
        "core.time.expiring" => &["new"],
        "core.secrets" => &["rotting_new"],
        // E2-M10: networking modules.
        "core.net" => &[
            "ip_addr",
            "ip_to_string",
            "ip_is_ipv4",
            "socket_addr",
            "socket_addr_parse",
            "socket_host",
            "socket_port",
            "socket_to_string",
            "tcp_listen",
            "tcp_listen_addr",
            "tcp_accept",
            "tcp_connect",
            "tcp_connect_addr",
            "tcp_connect_timeout",
            "tcp_connect_happy",
            "tcp_read",
            "tcp_write",
            "tcp_local_addr",
            "tcp_peer_addr",
            "tcp_local_socket_addr",
            "tcp_peer_socket_addr",
            "listener_local_socket_addr",
            "set_timeout",
            "set_read_timeout",
            "set_write_timeout",
            "tcp_reply",
            "udp_bind",
            "udp_bind_addr",
            "udp_local_addr",
            "udp_set_timeout",
            "udp_send_to",
            "udp_recv_from",
            "udp_packet_data",
            "udp_packet_addr",
            "unix_listen",
            "unix_accept",
            "unix_connect",
            "unix_read",
            "unix_write",
            "dns_a",
            "dns_aaaa",
            "dns_a_at",
            "dns_aaaa_at",
            "dns_txt",
            "dns_txt_at",
            "dns_srv",
            "dns_srv_at",
            "dns_srv_target",
            "dns_srv_port",
            "dns_srv_priority",
            "dns_srv_weight",
            "tls_connect",
            "tls_read",
            "tls_write",
            "tls_close",
        ],
        "jet.http" => &["get", "post", "serve"],
        // D-REGEXENGINE1=A: std-only linear regex package.
        "jet.regex" => &[
            "flags",
            "compile",
            "compile_with",
            "is_match",
            "match",
            "find",
            "find_all",
            "matches",
            "replace",
            "replace_all",
            "split",
            "split_limit",
        ],
        // D-DEP-ARCHIVE1=A: archive ring package — gzip + zip + tar.
        "core.archive" => &[
            "gzip_compress",
            "gzip_decompress",
            "zip_compress",
            "zip_decompress",
            "tar_add",
            "tar_get",
            "tar_names_json",
        ],
        // D-RAYLIB1=A: typed graphics bridge.
        "core.raylib" => &[
            "window_open",
            "window_should_close",
            "begin_drawing",
            "clear_background",
            "draw_text",
            "end_drawing",
            "close_window",
            "color",
        ],
        // D-CODECS1: standalone compression codecs, separate from core.archive.
        "core.compress.gzip" => &["compress", "decompress"],
        "core.compress.zstd" => &["compress", "decompress"],
        // D-DEP-DB1: SQLite ring package.
        // D-DBDRIVER1: `close`/`query`/`query_one`/`execute`/`begin`/`commit`/
        // `rollback` are `DbConnection` instance methods, not module items.
        "jet.db" => &[
            "open",
            "open_memory",
            "params",
            "row_value",
            "row_int",
            "row_float",
            "row_text",
            "row_bool",
            "transaction",
            "migrate",
        ],
        // D-DEP-WASM1=A: sandboxed WASM plugin loader ring package.
        "jet.plugin" => &["load"],
        // D-REACT1=B: opt-in reactive library — signals/derived/effects.
        // D-SIGNAL1: "computed" is the canonical alias for "derived".
        "jet.reactive" => &["signal", "derived", "computed", "effect"],
        // D-EVENT1=D: first-party typed Event/Hook family.
        "core.event" => &[
            "new",
            "with_policy",
            "hook",
            "scope",
            "policy_sync",
            "policy_async",
        ],
        // D-HONESTNUM1=A: Measurement<T> constructor.
        "core.science.measurement" => &["from"],
        // D-DECIMAL1: exact decimal constructor alias.
        "core.numeric" => &["decimal"],
        // D-PENDING1=B: Loadable<T,E> constructors.
        "core.async.loadable" => &["idle", "loading", "loaded", "failed"],
        // D-FIDELITY-API1=A: runtime-global fidelity signal.
        "core.perf" => &[
            "Perf",
            "fidelity",
            "default_fidelity",
            "override_fidelity",
            "reset_fidelity",
        ],
        // D-RENDERTGT2=A (c133 M1): UI backend seam.
        "core.ui" => &[
            "null_backend",
            "tui_backend",
            // D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend.
            "gtk_backend",
            "reactive_render",
            "point",
            "size",
            "rect",
            "constraint",
            "node",
            "key_event",
            "resize_event",
            // D-A11YGATE1=B (c134 Phase 6): accessible-role node + role constants.
            "node_role",
            "aria_role_button",
            "aria_role_text_input",
            "aria_role_label",
            "aria_role_container",
            // D-STYLESHAPE1=A (c134 Phase 3/7 wiring): a typed-Color-backed node —
            // the paint pipeline's fill color is no longer hardcoded.
            "node_color",
        ],
        // D-FLAGSHIP-WEBAPI1=A: browser events and storage. The intermediate
        // `storage` module exists so `web.storage.local.get(...)` resolves as a
        // real nested core module, not a magic field.
        "core.web" => &["on", "value", "storage"],
        "core.web.storage" => &["local", "session"],
        "core.web.storage.local" | "core.web.storage.session" => &["get", "set", "remove", "clear"],
        // c-devserver (owner-directed 2026-07-01): `jet dev` server builder.
        "core.devserver" => &["for_app", "app"],
        // D-APPROX1=A: approximate sketch data structures.
        "core.sketch.hll" => &["new"],
        "core.sketch.tdigest" => &["new"],
        "core.sketch.cms" => &["new"],
        "core.sketch.reservoir" => &["new"],
        // D-TIMEDEPTH1=A: civil-time constructors.
        "core.time.date" => &["new", "today", "parse"],
        "core.time.datetime" => &["from_timestamp", "now"],
        // D-NETDEP1=A / D-HTTPLIB1=A / D-HTTPLIB2=B: HTTP library.
        "core.http.client" => &["get", "post", "request"],
        "core.http.server" => &[
            "mux",
            "serve",
            "serve_once",
            "serve_once_listener",
            "response",
            "tls",
            "sse",
            "static_file",
            "static_file_range",
            "access_log",
        ],
        // U13 (D-JPK-SECRETCRYPTO1): decrypted-repo-secret read, age-style
        // crypto FFI bridge.
        "core.vault" => &["get"],
        _ => &[],
    };
    items.iter().map(|s| s.to_string()).collect()
}

/// E2-M15: modules that require an OS and are forbidden in `--freestanding` builds.
pub(crate) fn is_freestanding_forbidden(module: &str) -> bool {
    matches!(
        module,
        "core.files" | "core.watcher" | "core.io" | "core.net" | "core.tasks"
            | "core.process" | "core.time" | "jet.http" | "jet.log"
            // D-TERM1: terminal I/O requires an OS terminal device.
            | "core.term"
            // U13 (D-JPK-SECRETCRYPTO1): reading the encrypted repo store is
            // filesystem I/O — same OS dependency as `core.files`.
            | "core.vault"
    )
}

/// Return a short display name for the module alias (the part after the dot).
pub(crate) fn module_short_name(module: &str) -> &str {
    module.split('.').last().unwrap_or(module)
}

/// Fix hint for E3301 depending on the forbidden module.
pub(crate) fn freestanding_hint(module: &str) -> &'static str {
    match module {
        "core.files" => {
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
        "core.process" | "core.time" => {
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
        "import a specific core module, like `import core.files as fs;`".to_string()
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
/// hasn't opted in with `@[Codable]`/`@[Encode]`/`@[Decode]`.
pub(crate) fn e2411(ty: &str, encode: bool, span: Span) -> Diagnostic {
    let (verb, marker) = if encode {
        ("serialized", "`@[Codable]` or `@[Encode]`")
    } else {
        ("decoded", "`@[Codable]` or `@[Decode]`")
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
        "flatten splices another struct's keys into this object, so the field must be a `@[Codable]` struct — not a primitive, list, or map".to_string(),
        format!("give `{field}` a `@[Codable]` struct type, or drop `#[Flatten]`"),
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
        Some(idxs) => idxs
            .iter()
            .all(|&i| args.get(i).map_or(true, |t| elem_ok(t))),
        // No recorded wire params (imported/non-generic): trust every arg is fine
        // only if each is codable — be conservative and check them all.
        None => args.iter().all(|t| elem_ok(t)),
    }
}

pub(crate) fn is_encodable_ty(ty: &Type, reg: &TraitRegistry) -> bool {
    match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::IntN { .. }
        | Type::Float32 => true,
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

pub(crate) fn is_decodable_ty(ty: &Type, reg: &TraitRegistry) -> bool {
    match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::IntN { .. }
        | Type::Float32 => true,
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

/// D-SERDE: validate serde markers on every `@[Codable]`/`@[Encode]`/`@[Decode]` type
/// (E2407–E2412). Runs after the trait registry is built so field types resolve. This
/// keeps generated code rustc-clean (I2): a field with no wire form is caught here, not
/// by rustc on the emitted `impl`.
pub(crate) fn validate_serde_items(
    items: &[crate::AST::Item],
    reg: &TraitRegistry,
) -> Vec<Diagnostic> {
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
        // D-SERDE12: generic `@[Codable]` is first-class — no `type_params > 0`
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
                let flatten = f
                    .serde_markers
                    .iter()
                    .any(|m| m.name == Syntax::ATTR_FLATTEN);
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

/// D-A11YGATE1=B: is `expr` a single, non-interpolated literal string part?
/// Shared by E2930 (empty-label check) and E2931 (duplicate-label check).
fn is_empty_string_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Str(parts, _) if parts.iter().all(|p| matches!(p, crate::AST::StrPart::Lit(s) if s.is_empty()))
    )
}

/// D-A11YGATE1=B: the literal text of `expr` when it's a plain (non-interpolated)
/// string literal, else `None`.
fn literal_string_value(expr: &Expr) -> Option<String> {
    let Expr::Str(parts, _) = expr else {
        return None;
    };
    if parts.len() != 1 {
        return None;
    }
    match &parts[0] {
        crate::AST::StrPart::Lit(s) => Some(s.clone()),
        _ => None,
    }
}

/// D-A11YGATE1=B (c134 Phase 6, E2930): an interactive-role `UiNode` with an
/// empty accessible label.
pub(crate) fn a11y_unlabeled_control(role: &str, span: Span) -> Diagnostic {
    Diagnostic::lint(
        "E2930",
        format!("this {role} has no accessible label"),
        "screen readers announce a control by its accessible label — an empty label is invisible to assistive tech".to_string(),
        "pass a real label, e.g. `ui.node_role(\"Submit\", w, h, ui.aria_role_button())`".to_string(),
        Some(span),
    )
}

/// D-A11YGATE1=B (c134 Phase 6, E2931): two interactive nodes in the same
/// inline focus group share an accessible label.
pub(crate) fn a11y_duplicate_label(label: &str, span: Span) -> Diagnostic {
    Diagnostic::lint(
        "E2931",
        format!("two interactive nodes both have the label \"{label}\""),
        "assistive tech announces controls by their label — identical labels make them indistinguishable (WCAG 2.5.3)".to_string(),
        "give each interactive node a distinct, descriptive label".to_string(),
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
        "a derived value is recomputed from its signals; its lambda has to return the value"
            .to_string(),
        "return a value from the body, or use `reactive.effect(() => { … })` for a side effect"
            .to_string(),
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

/// D-DBDRIVER1: accessor methods on `DbValue` — read back the tagged value a
/// query bound or a row column carried. Mirrors `datatree_method_return`'s
/// shape exactly (`Result<T, String>`); `int` stays 64-bit (never `Float`).
pub fn db_value_method_return(method: &str, n_args: usize) -> Option<Type> {
    match (method, n_args) {
        ("int", 0) => Some(Type::Result {
            ok: Box::new(Type::Int),
            err: Box::new(Type::String),
        }),
        ("float", 0) => Some(Type::Result {
            ok: Box::new(Type::Float),
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
        ("is_null", 0) => Some(Type::Bool),
        _ => None,
    }
}

/// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)`'s handle types. `Value` is the
/// whole-value handle; `.fields()` returns `[Field]` (only populated for a
/// struct receiver — every other displayable shape gets an empty list,
/// resolved at codegen from `cx.struct_fields`, I3: sema only decides the
/// TYPES here, codegen does the per-call-site enumeration).
pub fn reflect_method_return(type_name: &str, method: &str, n_args: usize) -> Option<Type> {
    match (type_name, method, n_args) {
        ("Value", "type_name", 0) => Some(Type::String),
        ("Value", "display", 0) => Some(Type::String),
        ("Value", "fields", 0) => Some(Type::List(Box::new(Type::Named("Field".to_string())))),
        ("Field", "name", 0) => Some(Type::String),
        ("Field", "value", 0) => Some(Type::String),
        _ => None,
    }
}

pub fn is_reflect_type_name(name: &str) -> bool {
    matches!(name, "Value" | "Field")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raylib_skeleton_signatures_are_registered() {
        let window = core_fixed_sig("core.raylib", "window_open")
            .expect("raylib window_open signature")
            .1
            .expect("window_open return type");
        assert_eq!(window, Type::Named("RaylibWindow".to_string()));

        let color = core_fixed_sig("core.raylib", "color")
            .expect("raylib color signature")
            .1
            .expect("color return type");
        assert_eq!(color, Type::Named("RaylibColor".to_string()));

        let items = core_module_items("core.raylib");
        assert!(items.contains(&"draw_text".to_string()));
        assert!(items.contains(&"close_window".to_string()));
    }
}
