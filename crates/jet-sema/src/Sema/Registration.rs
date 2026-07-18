use super::*;
use crate::Collections::is_reserved_type;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Numeric::{allows_float_money, is_money_like_name};
use crate::Syntax;
use crate::AST::{
    AccessConvention, DistinctDef, ElseBranch, EnumDef, Expr, Func, IfStmt, Item, Stmt, StructDef,
    Type,
};
use std::collections::{HashMap, HashSet};

mod Items;
mod Serde;

pub(crate) use Items::{
    comptime_context_from_items, eval_comptime_items, eval_default_markers, name_defined,
    register_const, register_distinct, register_enum, register_impl_methods, register_struct,
    register_type_alias, register_type_methods,
};
pub(super) use Serde::expand_builtin_serde_items;

fn is_fallible_void_return(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Result { ok, err }
            if matches!(ok.as_ref(), Type::Named(n) if n == Syntax::TYPE_VOID)
                && matches!(err.as_ref(), Type::Named(n) if n == Syntax::TYPE_ERROR)
    )
}

impl<'a> Checker<'a> {
    /// D-FFI-INLINE1=A / D-FFI-ASM1=A / D-FFI-CPP1=A (card #501): validate an
    /// inline foreign tier function (`@FFI(<lang>) fn`). The systems floor ships
    /// `c`, `cpp`, and `asm`; every one is an unsafe foreign language, so an
    /// inline body requires the enclosing `@Unsafe("reason")` gate (I1/S58).
    /// Any other language name has no inline binder yet (E3220).
    fn check_inline_foreign_fn(&mut self, f: &Func) {
        let Some(inl) = &f.inline_foreign else {
            return;
        };
        // Systems-floor inline languages (card #501). All three are unsafe.
        const UNSAFE_INLINE_LANGS: &[&str] = &[
            Syntax::C_MODULE_ROOT,
            Syntax::CPP_MODULE_ROOT,
            Syntax::ASM_LANG,
        ];
        if !UNSAFE_INLINE_LANGS.contains(&inl.lang.as_str()) {
            self.diags.push(Diagnostic::error(
                "E3220",
                format!("no inline foreign binder for `{}` yet", inl.lang),
                "the inline foreign tier ships `c`, `cpp`, and `asm` first (the systems floor, card #501); other languages arrive on later polyglot cards".to_string(),
                "use `@FFI(c)`, `@FFI(cpp)`, or `@FFI(asm)`".to_string(),
                Some(inl.lang_span),
            ));
            return;
        }
        // I1/S58: an unsafe-language inline body must sit behind an `@Unsafe`
        // gate so the author states why calling it is sound.
        if !f.is_unsafe {
            self.diags.push(Diagnostic::error(
                "E3215",
                format!("`@FFI({})` needs an `@Unsafe(\"reason\")` gate", inl.lang),
                format!(
                    "`{}` is an unsafe foreign language — an inline `{}` body can break memory safety, so Jet requires you to state why it is sound to call (I1/S58)",
                    inl.lang, inl.lang
                ),
                format!("add the gate: `@Unsafe(\"…\") @FFI({}) fn …`", inl.lang),
                Some(inl.marker_span),
            ));
            return;
        }
        // D-FFI-INLINE1/ASM1/CPP1 (card #501): the front end (parse, contract
        // checking, `@Unsafe` gate, formatter, grammars) is live, but body
        // lowering is not yet wired for any language. Reject at sema so a valid
        // program never reaches codegen and emits uncompilable Rust (I2). This
        // gate lifts per language as its binder lands — asm awaits the ratified
        // operand/clobber model, C awaits a raw (non-interpolating) body form,
        // cpp awaits the clang shim toolchain.
        self.diags.push(Diagnostic::error(
            "E3221",
            format!("`@FFI({})` inline foreign body can't be compiled yet", inl.lang),
            "the inline foreign tier is parsed, contract-checked, and `@Unsafe`-gated, but body lowering for this language is still pending (card #501 systems floor)".to_string(),
            "track card #501 for when the inline binder for this language lands".to_string(),
            Some(inl.marker_span),
        ));
    }

    /// Shared tail of `check_func_body` / `check_func_body_bundle`:
    /// declare parameters, check the body, enforce definite return.
    pub(crate) fn check_params_and_body(&mut self, f: &mut Func, owner_type: Option<&str>) {
        for param in &f.type_params {
            self.warn_soft_public_declared_type(
                &Type::TraitObject(param.bounds.clone()),
                param.name_span,
            );
        }
        if let Some(return_type) = &f.return_type {
            self.warn_soft_public_declared_type(
                return_type,
                f.return_type_span.unwrap_or(f.name_span),
            );
        }
        for p in &f.params {
            // D-ANY-JAI1: the `...[TraitA, TraitB]` bound-list form parses `ty`
            // as an unused `Type::Named("")` placeholder (the real bound list is
            // `variadic_bound_list`) — nothing to declared-type-check there.
            let skip_type_check = (p.name == Syntax::KW_SELF || p.variadic_bound_list.is_some())
                && matches!(&p.ty, Type::Named(n) if n.is_empty());
            if !skip_type_check {
                let pty = self.resolve_type(p.ty.clone());
                self.check_declared_type(&pty, p.ty_span);
            }
            if p.name == Syntax::KW_SELF {
                if let Some(owner) = owner_type {
                    let self_ty = Type::Named(owner.to_string());
                    self.scopes.last_mut().unwrap().insert(
                        p.name.clone(),
                        LocalInfo {
                            def_span: p.name_span,
                            ty: self_ty,
                            mutable: matches!(p.convention, AccessConvention::Write),
                            param_conv: Some(p.convention),
                            decl_loop_depth: 0,
                            sendable: true,
                            task_lint_span: None,
                            single_use_span: None,
                        },
                    );
                }
                continue;
            }
            if self.lookup(&p.name).is_some() {
                self.diags.push(already_defined(&p.name, p.name_span));
            } else {
                let pty = self.resolve_type(if p.variadic {
                    // D-ANY-JAI1: a trait-bounded variadic (`...Trait` /
                    // `...[A, B]`) binds its loop element as a `Type::TraitObject`
                    // carrying every bound trait name, reusing the existing
                    // boxed-dispatch method/interpolation checking unchanged for the
                    // body-check pass only (codegen never sees this type — its own
                    // per-call-site synthesis in `Codegen/VariadicBound.rs` binds
                    // the loop var to a bare generic type param instead, so
                    // there's no boxing in the generated Rust). `Type::TraitObject`
                    // carries the full bound list (not just the first), so a method
                    // call in the loop body resolves against ALL bound traits —
                    // fixed from the v1 known gap (`Type::TraitObject` used to hold
                    // one name only).
                    match p.variadic_trait_bounds(|n| self.trait_reg.is_trait_name(n)) {
                        Some(bounds) => Type::List(Box::new(Type::TraitObject(bounds))),
                        None => Type::List(Box::new(p.ty.clone())),
                    }
                } else {
                    p.ty.clone()
                });
                // D-LIN1: a parameter never carries the consume duty. Passing a
                // `@SingleUse` value to a `^` parameter IS its terminal consumption
                // (spec: "passed to a take parameter … or returned"), so the `^`
                // recipient is where linearity is satisfied — making the param
                // re-consume would be infinite regress. Borrow/read params can't
                // own it at all. So `single_use_span` stays `None` for every param.
                self.scopes.last_mut().unwrap().insert(
                    p.name.clone(),
                    LocalInfo {
                        def_span: p.name_span,
                        ty: pty,
                        mutable: matches!(p.convention, AccessConvention::Write),
                        param_conv: Some(p.convention),
                        decl_loop_depth: 0,
                        sendable: true,
                        task_lint_span: None,
                        single_use_span: None,
                    },
                );
            }
        }
        // D-FFI-INLINE1=A (card #501): an inline foreign tier fn (`@FFI(<lang>)
        // fn`) has a foreign-source body, not Jet statements. Its parameters are
        // in scope above (the Jet signature is a real, checked contract at call
        // sites); validate the tier gate and skip the ordinary body/return
        // checker (the empty statement body would otherwise be E0114).
        if f.inline_foreign.is_some() {
            self.check_inline_foreign_fn(f);
            return;
        }
        // D-UNSAFE2 / D-LIN1-DROP: an `@Unsafe fn` body is an audited region just
        // like an `@Unsafe { … }` block — its reason is the audit note. Mark the
        // whole body unsafe so `drop(x)` of a `@SingleUse` value is permitted
        // (and any other unsafe-gated operation is reachable directly).
        if f.is_reactive {
            if f.is_pure {
                self.diags.push(Diagnostic::error(
                    "E2914",
                    "`@Reactive fn` can't also declare `--[]->`".to_string(),
                    "a reactive effect re-runs when signals change, so it is not a pure function"
                        .to_string(),
                    "drop the `--[]->` bound or use `reactive.effect` inside a plain `fn`".to_string(),
                    Some(f.name_span),
                ));
            }
            if f.return_type
                .as_ref()
                .is_some_and(|t| !matches!(t, Type::Named(n) if n == "Unit"))
            {
                self.diags.push(Diagnostic::error(
                    "E2914",
                    "`@Reactive fn` must not return a value".to_string(),
                    "`@Reactive fn` lowers to a reactive effect scope — effects don't produce a value (D-REACTCORE1)"
                        .to_string(),
                    "drop the return type, or use `@Reactive { … }` inside a plain `fn`"
                        .to_string(),
                    Some(f.name_span),
                ));
            }
        }
        // D-PREPOST1: check `@Pre`/`@Post` contract clauses. Params are already
        // in scope above; a condition is pure (same checker as `@Pure fn`,
        // E0139) and must be `Bool` (E0110 via `require_bool`). `@Post`
        // additionally binds `result` to the return type while its own
        // conditions are checked — `result` inside a `@Pre` is E0144 (see
        // the `Expr::Ident` dispatch in `CheckerInfer/expr.rs`).
        for clause in &mut f.pre {
            self.in_pre_clause = true;
            self.require_bool(&mut clause.cond, "a `@Pre` condition");
            self.in_pre_clause = false;
            if let Some(d) = check_pure_expr(&clause.cond, &f.name, self.funcs) {
                self.diags.push(e0139(Syntax::CONTRACT_PRE, d.span));
            }
        }
        if !f.post.is_empty() {
            let result_ty = self.resolve_type(
                f.return_type
                    .clone()
                    .unwrap_or(Type::Named("Unit".to_string())),
            );
            self.push_scope();
            self.declare(
                "result",
                f.name_span,
                LocalInfo {
                    def_span: f.name_span,
                    ty: result_ty,
                    mutable: false,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                    sendable: true,
                    task_lint_span: None,
                    single_use_span: None,
                },
            );
            for clause in &mut f.post {
                self.require_bool(&mut clause.cond, "a `@Post` condition");
                if let Some(d) = check_pure_expr(&clause.cond, &f.name, self.funcs) {
                    self.diags.push(e0139(Syntax::CONTRACT_POST, d.span));
                }
            }
            self.pop_scope();
        }
        let prev_unsafe = self.in_unsafe;
        self.in_unsafe = self.in_unsafe || f.is_unsafe;
        self.check_block(&mut f.body, false);
        self.in_unsafe = prev_unsafe;
        // D-ANY-JAI1: a trait-bounded variadic has no zero-cost representation
        // for arbitrary use (heterogeneous elements, no boxing allowed) — codegen
        // only covers one shape, a direct `loop x in parts { … }` loop, unrolled
        // per call site's arity. Reject anything else here (E1314) so codegen
        // never has to guess.
        self.check_variadic_bound_body_shape(f);
        self.lint_unjoined_tasks_in_current_scope();
        // D-LIN1: the function body's own scope (parameters + top-level locals) is
        // never `pop_scope`d, so check its `@SingleUse` locals here (E0140).
        self.check_single_use_consumed_in_current_scope();
        // D-STREAMYIELD1: a generator (`-> Stream<T>`) falling off the end is
        // exactly a bare `return;` — it just ends the stream. Never E0114.
        let is_generator =
            matches!(&f.return_type, Some(Type::Apply { name, .. }) if name == "Stream");
        let is_entry_fallible_void =
            f.name == "run" && f.return_type.as_ref().is_some_and(is_fallible_void_return);
        if !is_generator
            && !is_entry_fallible_void
            && f.return_type.is_some()
            && !block_definitely_returns(&f.body)
        {
            let rt = f.return_type.clone().unwrap();
            self.diags.push(Diagnostic::error(
                "E0114",
                format!(
                    "`{}` promises to return {}, but a path can reach the end without `return`",
                    f.name,
                    rt.show()
                ),
                "every way through the function must hand back a value".to_string(),
                format!(
                    "add a final `return ...;`, or an `{}` branch that returns",
                    Syntax::KW_ELSE
                ),
                Some(f.name_span),
            ));
        }
    }

    /// D-ANY-JAI1 (c7jaiany): validate that a trait-bounded variadic parameter
    /// is used *only* as the collection of a single top-level `loop x in parts {
    /// … }` loop — the one shape `crates/jet-codegen/src/Codegen/VariadicBound.rs`
    /// unrolls per call-site arity. `parts` has no zero-cost Rust representation
    /// outside that loop (heterogeneous elements, boxing is disallowed), so any
    /// other reference — a bare read, `.len()`, indexing, a second loop, passing
    /// it to another call, use inside a nested block (`@Unsafe { }`, `if`, …) —
    /// is E1314. v1 scope, matching the plan's "conservative v1" call for this
    /// card: exactly the shape the ballot's own `log_all` example needs.
    fn check_variadic_bound_body_shape(&mut self, f: &Func) {
        let Some(last) = f.params.last() else {
            return;
        };
        if last
            .variadic_trait_bounds(|n| self.trait_reg.is_trait_name(n))
            .is_none()
        {
            return;
        }
        let name = last.name.clone();
        let mut for_hits = 0usize;
        let mut other: Vec<Span> = Vec::new();
        for s in &f.body {
            scan_stmt_for_variadic_uses(s, &name, true, &mut for_hits, &mut other);
        }
        if for_hits > 1 {
            other.push(last.name_span);
        }
        for span in other {
            self.diags.push(e1314(&name, span));
        }
    }
}

/// D-ANY-JAI1: E1314 — a trait-bounded variadic parameter used outside the
/// one supported shape (a direct `loop x in name { … }` loop).
fn e1314(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1314",
        format!("`{name}` can only be used in a `loop … in {name}` loop here"),
        format!(
            "`{name}` is a trait-bounded variadic (`...Trait` / `...[A, B]`) — its elements can \
             have different concrete types, so there's no single Rust type to give `{name}` \
             outside a loop that visits each argument once"
        ),
        format!("iterate it with `loop x in {name} {{ … }}` — that's the only supported use"),
        Some(span),
    )
}

/// Walk one statement looking for uses of `name` (a trait-bounded variadic
/// parameter). `top_level` is true only for statements directly in the
/// function body (not nested in `if`/`while`/`loop`/`switch`/…) — the blessed
/// `loop x in name { … }` shape is only recognized there; `for_hits` counts how
/// many times it's seen; every other reference to `name` lands in `other`.
fn scan_stmt_for_variadic_uses(
    s: &Stmt,
    name: &str,
    top_level: bool,
    for_hits: &mut usize,
    other: &mut Vec<Span>,
) {
    match s {
        Stmt::For {
            var2: None,
            kind: crate::AST::ForKind::In { collection },
            body,
            label: None,
            ..
        } if top_level && matches!(collection, Expr::Ident(n, _) if n == name) => {
            *for_hits += 1;
            for st in body {
                scan_stmt_for_variadic_uses(st, name, false, for_hits, other);
            }
        }
        Stmt::Expr(e) | Stmt::Return(Some(e), _) => expr_uses(e, name, other),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::BreakLabel(_, _) | Stmt::ContinueLabel(_, _) => {}
        Stmt::Val(b) => expr_uses(&b.init, name, other),
        Stmt::Assign { target, value, .. } => {
            match target {
                crate::AST::LValue::Index { base, index, .. } => {
                    expr_uses(base, name, other);
                    expr_uses(index, name, other);
                }
                crate::AST::LValue::Field { base, .. } => expr_uses(base, name, other),
                crate::AST::LValue::Local { name: n, name_span } => {
                    if n == name {
                        other.push(*name_span);
                    }
                }
            }
            expr_uses(value, name, other);
        }
        Stmt::If(ifs) => scan_if(ifs, name, for_hits, other),
        Stmt::While { cond, body, .. } => {
            expr_uses(cond, name, other);
            for st in body {
                scan_stmt_for_variadic_uses(st, name, false, for_hits, other);
            }
        }
        Stmt::For {
            kind, body, span, ..
        } => {
            match kind {
                crate::AST::ForKind::Range { start, end, step } => {
                    expr_uses(start, name, other);
                    expr_uses(end, name, other);
                    if let Some(s) = step {
                        expr_uses(s, name, other);
                    }
                }
                crate::AST::ForKind::In { collection } => {
                    if matches!(collection, Expr::Ident(n, _) if n == name) {
                        other.push(*span);
                    } else {
                        expr_uses(collection, name, other);
                    }
                }
            }
            for st in body {
                scan_stmt_for_variadic_uses(st, name, false, for_hits, other);
            }
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            expr_uses(subject, name, other);
            for arm in arms {
                expr_uses(&arm.cond, name, other);
                for st in &arm.body {
                    scan_stmt_for_variadic_uses(st, name, false, for_hits, other);
                }
            }
            if let Some(body) = else_body {
                for st in body {
                    scan_stmt_for_variadic_uses(st, name, false, for_hits, other);
                }
            }
        }
        Stmt::Loop { body, .. } => {
            for st in body {
                scan_stmt_for_variadic_uses(st, name, false, for_hits, other);
            }
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            expr_uses(&init.init, name, other);
            expr_uses(cond, name, other);
            scan_stmt_for_variadic_uses(step, name, false, for_hits, other);
            for st in body {
                scan_stmt_for_variadic_uses(st, name, false, for_hits, other);
            }
        }
        // Every other statement kind (lexical-scope wrappers like `@Unsafe { }`,
        // `region`, `taskgroup`, `@Transact`, `comptime { }`, …) is out of scope
        // for v1 — a trait-bounded variadic used inside one of these isn't
        // caught here; codegen's own "internal compiler error" guard
        // (`VariadicBound.rs`) is the backstop.
        _ => {}
    }
}

fn scan_if(ifs: &IfStmt, name: &str, for_hits: &mut usize, other: &mut Vec<Span>) {
    expr_uses(&ifs.cond, name, other);
    for st in &ifs.then_body {
        scan_stmt_for_variadic_uses(st, name, false, for_hits, other);
    }
    match &ifs.else_branch {
        Some(ElseBranch::ElseIf(inner)) => scan_if(inner, name, for_hits, other),
        Some(ElseBranch::Else(body)) => {
            for st in body {
                scan_stmt_for_variadic_uses(st, name, false, for_hits, other);
            }
        }
        None => {}
    }
}

/// Best-effort (not exhaustive — mirrors `Purity.rs`'s existing call-collector
/// coverage) scan of an expression for a bare `Ident(name)`.
fn expr_uses(e: &Expr, name: &str, other: &mut Vec<Span>) {
    match e {
        Expr::Ident(n, span) => {
            if n == name {
                other.push(*span);
            }
        }
        Expr::Call(c) => {
            for a in &c.args {
                expr_uses(&a.expr, name, other);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_uses(receiver, name, other);
            for a in args {
                expr_uses(&a.expr, name, other);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            expr_uses(callee, name, other);
            for a in args {
                expr_uses(&a.expr, name, other);
            }
        }
        Expr::FanOut { callee, items, .. } => {
            expr_uses(callee, name, other);
            for item in items {
                expr_uses(item, name, other);
            }
        }
        Expr::Binary(_, l, r, _) => {
            expr_uses(l, name, other);
            expr_uses(r, name, other);
        }
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Field(inner, _, _)
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) => expr_uses(inner, name, other),
        Expr::OptField { base, .. } => expr_uses(base, name, other),
        Expr::Index { base, index, .. } => {
            expr_uses(base, name, other);
            expr_uses(index, name, other);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            expr_uses(base, name, other);
            expr_uses(start, name, other);
            expr_uses(end, name, other);
        }
        Expr::ListLit(items, _) => {
            for i in items {
                expr_uses(i, name, other);
            }
        }
        Expr::MapLit(pairs, _) => {
            for (k, v) in pairs {
                expr_uses(k, name, other);
                expr_uses(v, name, other);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, v) in fields {
                expr_uses(v, name, other);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, v) in fields {
                expr_uses(v, name, other);
            }
        }
        Expr::EnumLit { args, .. } => {
            for a in args {
                let e = match a {
                    crate::AST::EnumLitArg::Positional(e) => e,
                    crate::AST::EnumLitArg::Named { expr, .. } => expr,
                };
                expr_uses(e, name, other);
            }
        }
        Expr::Str(parts, _) => {
            for p in parts {
                if let crate::AST::StrPart::Interp(inner, _) = p {
                    expr_uses(inner, name, other);
                }
            }
        }
        _ => {}
    }
}


/// D-EFF1/D-SHAPE8: enforce declared effect-arrow bounds against inferred sets.
/// For each function (and method) carrying a row, the inferred set must be a
/// subset of the declared set — any extra effect is E0740.
pub(crate) fn check_effect_boundaries(
    items: &[Item],
    solved: &HashMap<String, EffectSet>,
    summaries: &HashMap<String, EffectSummary>,
    diags: &mut Vec<Diagnostic>,
) {
    fn unbounded_dispatch(
        key: &str,
        summaries: &HashMap<String, EffectSummary>,
        seen: &mut HashSet<String>,
    ) -> Option<(String, Span)> {
        if !seen.insert(key.to_string()) {
            return None;
        }
        let summary = summaries.get(key)?;
        for callee in &summary.edges {
            if summaries
                .get(callee)
                .is_some_and(|target| target.unbounded_trait_dispatch)
            {
                let span = summary
                    .memory
                    .calls
                    .iter()
                    .find(|call| call.callee == *callee)
                    .map(|call| call.span)
                    .unwrap_or_else(|| Span::new(0, 0));
                return Some((callee.clone(), span));
            }
            if let Some(found) = unbounded_dispatch(callee, summaries, seen) {
                return Some(found);
            }
        }
        None
    }

    fn check_one(
        f: &Func,
        owner: Option<&str>,
        identity: Option<&str>,
        solved: &HashMap<String, EffectSet>,
        summaries: &HashMap<String, EffectSummary>,
        diags: &mut Vec<Diagnostic>,
    ) {
        let Some(declared_list) = &f.declared_effects else {
            return;
        };
        let key = identity
            .map(str::to_owned)
            .unwrap_or_else(|| effect_key(owner, &f.name));
        if let Some((trait_method, span)) =
            unbounded_dispatch(&key, summaries, &mut HashSet::new())
        {
            diags.push(e0743(&trait_method, span));
            return;
        }
        // Validate names and build the declared set; an unknown name is E0119.
        // A bad name leaves the declared set incomplete, so skip the subset
        // check to avoid a misleading E0740 piled on top of the real problem.
        // D-PROP2=A: names starting with `!` are prohibitions — validated separately.
        let mut declared: EffectSet = EffectSet::new();
        let mut prohibited: EffectSet = EffectSet::new();
        let mut bad_name = false;
        let mut open_row = false;
        for (name, span) in declared_list {
            let (is_prohibited, base_name) = if name.starts_with('!') {
                (true, &name[1..])
            } else {
                (false, name.as_str())
            };
            if effect_row_var(base_name).is_some() {
                open_row = true;
                continue;
            }
            match parse_effect_name(base_name) {
                Some(e) => {
                    if is_prohibited {
                        prohibited.insert(e);
                    } else {
                        declared.insert(e);
                    }
                }
                None => {
                    diags.push(unknown_effect(base_name, *span));
                    bad_name = true;
                }
            }
        }
        if bad_name {
            return;
        }
        let inferred = solved
            .get(&key)
            .cloned()
            .unwrap_or_default();
        // E0740: only check positive bounds — prohibition-only annotations (`#(!Net)`)
        // have no upper-bound constraint; only the prohibition check applies.
        // D-EFFTREE1: `declared` may name ancestor roots — an ancestor entry
        // covers any effect at or below it, so this is a subsumption-aware
        // check, not a flat set difference.
        if !open_row && !declared.is_empty() {
            let over: EffectSet = effects_uncovered(&inferred, &declared);
            if !over.is_empty() {
                let span = declared_list
                    .first()
                    .map(|(_, s)| *s)
                    .unwrap_or(f.name_span);
                diags.push(e0740(&f.name, &over, &declared, span));
            }
        }
        // D-PROP1=A: check prohibitions — the inferred transitive set must not
        // contain any prohibited effect. E0749 names the effect and the function.
        // D-EFFTREE1: symmetric ancestor subsumption — a prohibited root
        // prohibits its whole subtree, so this is coverage, not intersection.
        if !prohibited.is_empty() {
            let reached_prohibited: EffectSet = effects_covered(&inferred, &prohibited);
            if !reached_prohibited.is_empty() {
                let span = declared_list
                    .iter()
                    .find(|(n, _)| n.starts_with('!'))
                    .map(|(_, s)| *s)
                    .unwrap_or(f.name_span);
                diags.push(e0749(&f.name, &reached_prohibited, &prohibited, span));
            }
        }
    }
    for item in items {
        match item {
            Item::Func(f) => check_one(f, None, None, solved, summaries, diags),
            Item::Impl(i) => {
                for m in &i.methods {
                    check_one(m, Some(&i.type_name), None, solved, summaries, diags);
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    check_one(m, Some(&s.name), None, solved, summaries, diags);
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        check_one(m, Some(&s.name), None, solved, summaries, diags);
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    check_one(m, Some(&e.name), None, solved, summaries, diags);
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    for item in body {
                        if let Item::Func(f) = item {
                            let identity = format!("{}__{}", module.name, f.name);
                            check_one(
                                f,
                                None,
                                Some(&identity),
                                solved,
                                summaries,
                                diags,
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // D-EFF3: trait-method effect bounds. Build the per-method bounds from every
    // trait, the (trait, type, method) list from every trait impl, then enforce
    // each impl's inferred effects ⊆ the trait's bound (E0742).
    let mut trait_bounds: HashMap<(String, String), EffectSet> = HashMap::new();
    let mut impls: Vec<(String, String, String, Span)> = Vec::new();
    for item in items {
        if let Item::Trait(t) = item {
            for m in &t.methods {
                let bound = match (m.is_pure, &m.declared_effects) {
                    (true, _) => Some(EffectSet::new()), // `@Pure` → empty bound
                    (false, Some(list)) => {
                        let mut set = EffectSet::new();
                        let mut ok = true;
                        let mut open = false;
                        for (name, span) in list {
                            if effect_row_var(name).is_some() {
                                open = true;
                                continue;
                            }
                            match parse_effect_name(name) {
                                Some(e) => {
                                    set.insert(e);
                                }
                                None => {
                                    diags.push(unknown_effect(name, *span));
                                    ok = false;
                                }
                            }
                        }
                        if ok && !open {
                            Some(set)
                        } else {
                            None
                        }
                    }
                    (false, None) => None, // un-annotated: per-impl, no obligation
                };
                if let Some(b) = bound {
                    trait_bounds.insert((t.name.clone(), m.name.clone()), b);
                }
            }
        }
    }
    let mut push_impl = |trait_name: &str, type_name: &str, methods: &[Func]| {
        for m in methods {
            impls.push((
                trait_name.to_string(),
                type_name.to_string(),
                m.name.clone(),
                m.name_span,
            ));
        }
    };
    for item in items {
        match item {
            Item::Impl(i) => {
                if let Some(t) = &i.trait_name {
                    push_impl(t, &i.type_name, &i.methods);
                }
            }
            Item::Struct(s) => {
                for block in &s.trait_impls {
                    push_impl(&block.trait_name, &s.name, &block.methods);
                }
            }
            Item::Enum(e) => {
                for block in &e.trait_impls {
                    push_impl(&block.trait_name, &e.name, &block.methods);
                }
            }
            _ => {}
        }
    }
    check_trait_obligations(&impls, &trait_bounds, solved, diags);
}


/// D-EFF1: the key a function is recorded under in the effect-summary map.
/// `Type::method` for methods (disambiguates same-named methods across types),
/// the bare name for top-level functions (so bare-call edges resolve).
pub fn effect_key(owner_type: Option<&str>, name: &str) -> String {
    match owner_type {
        Some(t) => format!("{t}::{name}"),
        None => name.to_string(),
    }
}


/// D-ERR-CONV: canonical Rust function name for the `impl From -> To` conversion.
/// Used by sema (to stamp into `TryConvert::Typed`) and codegen (to define + call it).
pub fn error_conv_fn_name(from: &str, to: &str) -> String {
    let f = from.replace('.', "_");
    let t = to.replace('.', "_");
    format!("__jet_errconv_{}_to_{}", f, t)
}

pub(crate) fn already_defined(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0118",
        format!("the name `{}` is already taken here", name),
        "inside one function, each name refers to exactly one thing".to_string(),
        format!(
            "pick a different name, or assign to the existing one with `{} = ...`",
            name
        ),
        Some(span),
    )
}

/// E0105: a top-level definition's name collides with another item. Every
/// item kind shares the same `what` and `fix`; callers pass the kind-specific
/// `why` (functions, structs, enums, consts, traits, tests, …).
pub(crate) fn defined_twice(name: &str, why: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0105",
        format!("`{}` is defined twice", name),
        why.to_string(),
        "rename or remove one of the definitions".to_string(),
        Some(span),
    )
}

/// E0105: a method's name collides with a field on the same type.
pub(crate) fn method_field_clash(method: &str, type_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0105",
        format!(
            "method `{}` can't share a name with a field on `{}`",
            method, type_name
        ),
        "a type's methods and fields must have different names".to_string(),
        "rename the method or the field".to_string(),
        Some(span),
    )
}

/// E0105: a method name appears twice on the same type.
/// `is_ctor` is true when the duplicate is a no-`self` static (a named
/// constructor per D-CTOR1), so the fix hint teaches constructor naming.
pub(crate) fn method_defined_twice(
    method: &str,
    type_name: &str,
    span: Span,
    is_ctor: bool,
) -> Diagnostic {
    let fix = if is_ctor {
        format!(
            "named constructors must each have a unique name — call them `{}.{}` and `{}.other_name`",
            type_name, method, type_name
        )
    } else {
        "rename or remove one of the definitions".to_string()
    };
    Diagnostic::error(
        "E0105",
        format!("method `{}` is defined twice on `{}`", method, type_name),
        "each method name may appear only once on a type".to_string(),
        fix,
        Some(span),
    )
}

pub(crate) fn impl_type_exists(
    type_name: &str,
    registry: &TypeRegistry,
    imports: &HashMap<String, usize>,
    states: Option<&[ModuleState]>,
) -> bool {
    if registry.contains(type_name) {
        return true;
    }
    if let Some((alias, local)) = type_name.rsplit_once('.') {
        let Some(states) = states else {
            return false;
        };
        let Some(&idx) = imports.get(alias) else {
            return false;
        };
        return states[idx].registry.contains(local);
    }
    false
}

pub(crate) fn synthesize_impls(items: &mut Vec<Item>) {
    // Build trait_name -> method sigs from the AST (no trait_reg needed).
    let mut trait_methods: HashMap<String, Vec<crate::AST::TraitMethodSig>> = HashMap::new();
    for item in items.iter() {
        if let Item::Trait(t) = item {
            trait_methods.insert(t.name.clone(), t.methods.clone());
        }
    }

    // Build (type_name, trait_name) impl pairs and struct field types from the AST.
    // Used to guard delegation synthesis — only synthesize if the field type actually
    // implements the trait (error is emitted later by E2401 validation if not).
    let mut impl_pairs: std::collections::HashSet<(String, String)> = Default::default();
    let mut struct_field_types: HashMap<String, HashMap<String, String>> = HashMap::new();
    for item in items.iter() {
        match item {
            Item::Impl(i) => {
                if let Some(trait_name) = &i.trait_name {
                    if i.delegation_field.is_none() {
                        impl_pairs.insert((i.type_name.clone(), trait_name.clone()));
                    }
                }
            }
            Item::Struct(s) => {
                // Also check inline trait impls (impl Trait { … } inside struct body)
                for block in &s.trait_impls {
                    impl_pairs.insert((s.name.clone(), block.trait_name.clone()));
                }
                let fields: HashMap<String, String> = s
                    .fields
                    .iter()
                    .map(|f| (f.name.clone(), f.ty.name()))
                    .collect();
                struct_field_types.insert(s.name.clone(), fields);
            }
            _ => {}
        }
    }

    // S62: delegation — build forwarding Func nodes only when the field type
    // implements the trait (guards against generating invalid code for E2401 cases).
    let mut delegations: Vec<(usize, String, String, String)> = Vec::new(); // (idx, type_name, trait_name, field_name)
    for (idx, item) in items.iter().enumerate() {
        if let Item::Impl(i) = item {
            if let (Some(trait_name), Some(field_name)) = (&i.trait_name, &i.delegation_field) {
                delegations.push((
                    idx,
                    i.type_name.clone(),
                    trait_name.clone(),
                    field_name.clone(),
                ));
            }
        }
    }
    for (idx, type_name, trait_name, field_name) in delegations {
        // Check if the field type implements the trait in the AST.
        let field_type_name = struct_field_types
            .get(&type_name)
            .and_then(|fields| fields.get(&field_name))
            .cloned();
        let can_delegate = field_type_name
            .as_ref()
            .is_some_and(|ft| impl_pairs.contains(&(ft.clone(), trait_name.clone())));
        if !can_delegate {
            // Skip synthesis; E2401 validation will emit the appropriate error.
            continue;
        }
        if let Some(sigs) = trait_methods.get(&trait_name) {
            let synthesized: Vec<crate::AST::Func> = sigs
                .iter()
                .map(|m| synthesize_delegation_method(m, &field_name))
                .collect();
            if let Item::Impl(i) = &mut items[idx] {
                i.methods = synthesized;
            }
        }
    }

    // D-LIB2: default method body injection.
    let mut trait_impls_to_fill: Vec<(usize, String)> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        if let Item::Impl(i) = item {
            if let Some(trait_name) = &i.trait_name {
                if i.delegation_field.is_none() {
                    trait_impls_to_fill.push((idx, trait_name.clone()));
                }
            }
        }
    }
    for (idx, trait_name) in trait_impls_to_fill {
        if let Some(sigs) = trait_methods.get(&trait_name) {
            let mut extras: Vec<crate::AST::Func> = Vec::new();
            if let Item::Impl(i) = &items[idx] {
                let provided: std::collections::HashSet<String> =
                    i.methods.iter().map(|m| m.name.clone()).collect();
                for sig in sigs {
                    if !provided.contains(&sig.name) {
                        if let Some(body) = &sig.default_body {
                            extras.push(synthesize_default_method(sig, body));
                        }
                    }
                }
            }
            if !extras.is_empty() {
                if let Item::Impl(i) = &mut items[idx] {
                    i.methods.extend(extras);
                }
            }
        }
    }
}

// S62: build a forwarding `Func` for one trait method sig, delegating to
// `self.<field>.<method>(args…)`.
pub(crate) fn synthesize_delegation_method(
    sig: &crate::AST::TraitMethodSig,
    field_name: &str,
) -> crate::AST::Func {
    use crate::Diagnostics::Span;
    use crate::AST::{AccessConvention, CallArg, CallArgFlags, Expr, Func, Param, Stmt, Type};

    let zero = Span::new(0, 0);

    // Build the forwarding call: self.<field>.<method>(non-self params...)
    let args: Vec<CallArg> = sig
        .params
        .iter()
        .filter(|p| p.name != Syntax::KW_SELF)
        .map(|p| CallArg {
            convention: p.convention,
            expr: Expr::Ident(p.name.clone(), zero),
            span: zero,
            flags: CallArgFlags::default(),
            label: None,
            spread: false,
        })
        .collect();

    let forward_call = Expr::MethodCall {
        receiver: Box::new(Expr::Field(
            Box::new(Expr::Ident(Syntax::KW_SELF.to_string(), zero)),
            field_name.to_string(),
            zero,
        )),
        method: sig.name.clone(),
        method_span: zero,
        type_args: Vec::new(),
        args,
        recv_type: None,
        resolved_ret: None,
    };

    // Wrap in a return stmt if there's a return type; otherwise a bare expr stmt.
    let body_stmt = if sig.return_type.is_some() {
        Stmt::Return(Some(forward_call), zero)
    } else {
        Stmt::Expr(forward_call)
    };

    // Build the `self` param.
    let self_param = Param {
        convention: AccessConvention::Read,
        name: Syntax::KW_SELF.to_string(),
        name_span: zero,
        ty: Type::Named(String::new()), // S27: sema fills in the actual type name
        ty_span: zero,
        default: None,
        variadic: false,
        variadic_bound_list: None,
    };

    let mut params = vec![self_param];
    params.extend(
        sig.params
            .iter()
            .filter(|p| p.name != Syntax::KW_SELF)
            .cloned(),
    );

    Func {
        span: sig.name_span,
        is_pub: false,
        is_package_pub: false,
        external_type: None,
        name: sig.name.clone(),
        name_span: sig.name_span,
        meta: None,
                    type_params: vec![],
        params,
        return_type: sig.return_type.clone(),
        return_type_span: None,
        return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
        is_unsafe: false,
        unsafe_reason: None,
        unsafe_span: None,
        is_pure: false,
        is_reactive: false,
        is_replayable: false,
        replayable_span: None,
        is_task: false,
        task_span: None,
        every: None,
        is_must_use: false,
        must_use_span: None,
        maturity: None,
        maturity_span: None,
        is_inline: false,
        is_inline_always: false,
        inline_span: None,
        is_sanitizer: false,
        declared_effects: None,
        effect_via: None,
        state_requires: None,
        state_transition: None,
        web_marker: None,
        pre: Vec::new(),
        post: Vec::new(),
        inline_foreign: None,
        body: vec![body_stmt],
    }
}

// D-LIB2: build a Func that uses the default body from the trait definition.
pub(crate) fn synthesize_default_method(
    sig: &crate::AST::TraitMethodSig,
    body: &[crate::AST::Stmt],
) -> crate::AST::Func {
    use crate::Diagnostics::Span;
    use crate::AST::{AccessConvention, Func, Param, Type};

    let zero = Span::new(0, 0);
    let self_param = Param {
        convention: AccessConvention::Read,
        name: Syntax::KW_SELF.to_string(),
        name_span: zero,
        ty: Type::Named(String::new()), // S27: sema fills in the actual type name
        ty_span: zero,
        default: None,
        variadic: false,
        variadic_bound_list: None,
    };
    let mut params = vec![self_param];
    params.extend(
        sig.params
            .iter()
            .filter(|p| p.name != Syntax::KW_SELF)
            .cloned(),
    );

    Func {
        span: sig.name_span,
        is_pub: false,
        is_package_pub: false,
        external_type: None,
        name: sig.name.clone(),
        name_span: sig.name_span,
        meta: None,
                    type_params: vec![],
        params,
        return_type: sig.return_type.clone(),
        return_type_span: None,
        return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
        is_unsafe: false,
        unsafe_reason: None,
        unsafe_span: None,
        is_pure: false,
        is_reactive: false,
        is_replayable: false,
        replayable_span: None,
        is_task: false,
        task_span: None,
        every: None,
        is_must_use: false,
        must_use_span: None,
        maturity: None,
        maturity_span: None,
        is_inline: false,
        is_inline_always: false,
        inline_span: None,
        is_sanitizer: false,
        declared_effects: None,
        effect_via: None,
        state_requires: None,
        state_transition: None,
        web_marker: None,
        pre: Vec::new(),
        post: Vec::new(),
        inline_foreign: None,
        body: body.to_vec(),
    }
}

// ─── D-NARG-D2: default-expression forward-reference check (E0126) ───────────

/// Check every default expression in a function/method for forward references —
/// i.e. an Ident that names a parameter declared *after* the one being defaulted.
/// Emits E0126 for each forward reference found.
/// `params` excludes `self`; `fn_name` is for the error message.
pub(crate) fn check_default_forward_refs(
    params: &[crate::AST::Param],
    fn_name: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    for (i, p) in params.iter().enumerate() {
        let Some(default) = &p.default else { continue };
        let forward_refs = super::find_forward_refs(default, &param_names, i);
        for (fwd_name, fwd_span) in forward_refs {
            diags.push(Diagnostic::error(
                "E0126",
                format!(
                    "default for `{}` in `{}` references `{}`, which comes after it",
                    p.name, fn_name, fwd_name
                ),
                "defaults fill left to right; a parameter isn't in scope until it appears in the list".to_string(),
                format!(
                    "reorder the parameters so `{}` comes before `{}`, or use a constant default",
                    fwd_name, p.name
                ),
                Some(fwd_span),
            ));
        }
    }
}

// ─── S60 / E2-M16: purity checking ───────────────────────────────────────────

#[cfg(test)]
mod structure_tests {
    #[test]
    fn registration_stays_split_without_reordering_passes() {
        const MAX_MODULE_LINES: usize = 2500;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |relative: &str| std::fs::read_to_string(root.join(relative)).unwrap();
        let registration = read("src/Sema/Registration.rs");
        let items = read("src/Sema/Registration/Items.rs");
        let serde = read("src/Sema/Registration/Serde.rs");
        let production = registration
            .split("#[cfg(test)]\nmod structure_tests")
            .next()
            .unwrap();
        for (relative, source) in [
            ("src/Sema/Registration.rs", production),
            ("src/Sema/Registration/Items.rs", items.as_str()),
            ("src/Sema/Registration/Serde.rs", serde.as_str()),
        ] {
            assert!(
                source.lines().count() < MAX_MODULE_LINES,
                "{relative} must stay below the card #510 module boundary"
            );
            assert!(!source.contains("include!("));
            assert!(!source.contains("#[path"));
        }

        assert!(production.contains("\nmod Items;\nmod Serde;\n"));
    }
}
