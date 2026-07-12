use super::*;
use crate::Collections::is_reserved_type;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Numeric::{allows_float_money, is_money_like_name};
use crate::Syntax;
use crate::Traits::TraitRegistry;
use crate::AST::{
    AccessConvention, ConstAttr, DistinctDef, ElseBranch, EnumDef, Expr, Func, IfStmt, Item, Param,
    Program, RustConstKind, Stmt, StructDef, Type,
};
use std::collections::{HashMap, HashSet};

fn is_fallible_void_return(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Result { ok, err }
            if matches!(ok.as_ref(), Type::Named(n) if n == Syntax::TYPE_VOID)
                && matches!(err.as_ref(), Type::Named(n) if n == Syntax::TYPE_ERROR)
    )
}

impl<'a> Checker<'a> {
    /// Shared tail of `check_func_body` / `check_func_body_bundle`:
    /// declare parameters, check the body, enforce definite return.
    pub(crate) fn check_params_and_body(&mut self, f: &mut Func, owner_type: Option<&str>) {
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
                            task_has_view_capture: false,
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
                // `#SingleUse` value to a `^` parameter IS its terminal consumption
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
                        task_has_view_capture: false,
                    },
                );
            }
        }
        // D-UNSAFE2 / D-LIN1-DROP: an `#Unsafe fn` body is an audited region just
        // like an `#Unsafe { … }` block — its reason is the audit note. Mark the
        // whole body unsafe so `drop(x)` of a `#SingleUse` value is permitted
        // (and any other unsafe-gated operation is reachable directly).
        if f.is_unsafe && f.unsafe_reason.is_none() {
            self.diags.push(Diagnostic::lint(
                "L3101",
                "this `#Unsafe` function has no reason".to_string(),
                "every gated function records why callers can rely on its unsafe contract"
                    .to_string(),
                "add the reason: `#Unsafe(\"why this is safe\") fn ...`".to_string(),
                f.unsafe_span.or(Some(f.name_span)),
            ));
        }
        if f.is_reactive {
            if f.is_pure {
                self.diags.push(Diagnostic::error(
                    "E2914",
                    "`#Reactive fn` can't also be `@Pure fn`".to_string(),
                    "a reactive effect re-runs when signals change, so it is not a pure function"
                        .to_string(),
                    "drop `@Pure` or use `reactive.effect` inside a plain `fn`".to_string(),
                    Some(f.name_span),
                ));
            }
            if f.return_type
                .as_ref()
                .is_some_and(|t| !matches!(t, Type::Named(n) if n == "Unit"))
            {
                self.diags.push(Diagnostic::error(
                    "E2914",
                    "`#Reactive fn` must not return a value".to_string(),
                    "`#Reactive fn` lowers to a reactive effect scope — effects don't produce a value (D-REACTCORE1)"
                        .to_string(),
                    "drop the return type, or use `#Reactive { … }` inside a plain `fn`"
                        .to_string(),
                    Some(f.name_span),
                ));
            }
        }
        // D-PREPOST1: check `@Pre`/`@Post` contract clauses. Params are already
        // in scope above; a condition is pure (same checker as `#Pure fn`,
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
                    task_has_view_capture: false,
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
        // never `pop_scope`d, so check its `#SingleUse` locals here (E0140).
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
    /// it to another call, use inside a nested block (`#Unsafe { }`, `if`, …) —
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
        // Every other statement kind (lexical-scope wrappers like `#Unsafe { }`,
        // `region`, `taskgroup`, `#Transact`, `comptime { }`, …) is out of scope
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
        | Expr::Tainted(inner, _)
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

pub fn check(prog: &mut Program) -> Vec<Diagnostic> {
    check_with_mode(prog, CompileMode::Run)
}

pub fn check_with_mode(prog: &mut Program, mode: CompileMode) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut funcs: HashMap<String, FuncSig> = HashMap::new();
    let mut tests: HashMap<String, Span> = HashMap::new();
    let mut registry = TypeRegistry {
        types: HashMap::new(),
        computed_fields: HashMap::new(),
    };
    let mut consts: HashMap<String, Type> = HashMap::new();
    let mut trait_reg = TraitRegistry::default();
    // Legacy M2 struct field-type map, kept for the cloneable helper.
    let mut struct_fields_legacy: HashMap<String, Vec<Type>> = HashMap::new();

    // D-FIELDPOL1: computed-field cycle check (E0338) + `self.field` rewrite +
    // synthesized getter methods, before anything else sees the struct.
    process_computed_fields(&mut prog.items, &mut diags);
    // D-PATCH1: synthetic `T.Patch` structs before the registration pass.
    inject_patchable_types(&mut prog.items, &mut diags);

    // --- registration pass (M3) -----------------------------------------
    for item in &prog.items {
        match item {
            Item::Trait(t) => {
                if name_defined(&t.name, &funcs, &registry, &consts) {
                    diags.push(defined_twice(
                        &t.name,
                        "every trait needs a unique name",
                        t.name_span,
                    ));
                }
            }
            // D-QUAL2: a `tag` is a marker qualifier with no methods. Name
            // uniqueness is checked here; E0732 (a method in a tag body) is
            // reported in `TraitRegistry::register_items` so it fires on every
            // sema path (single-program and bundle).
            Item::Tag(t) => {
                if name_defined(&t.name, &funcs, &registry, &consts) {
                    diags.push(defined_twice(
                        &t.name,
                        "every tag needs a unique name",
                        t.name_span,
                    ));
                }
            }
            Item::Func(f) => {
                if f.name == Syntax::BUILTIN_PRINT
                    || f.name == Syntax::BUILTIN_PANIC
                    || f.name == Syntax::BUILTIN_REQUIRE
                    || f.name == Syntax::BUILTIN_REQUIRE_EQ
                    || f.name == Syntax::BUILTIN_FIND
                    || f.name == Syntax::BUILTIN_EXPECT
                {
                    diags.push(Diagnostic::error(
                        "E0106",
                        format!("the name `{}` is built in and can't be redefined", f.name),
                        format!("`{}` is provided by the language itself", f.name),
                        "choose a different name for this function".to_string(),
                        Some(f.name_span),
                    ));
                } else if name_defined(&f.name, &funcs, &registry, &consts) {
                    diags.push(defined_twice(
                        &f.name,
                        "every function needs a unique name so calls aren't ambiguous",
                        f.name_span,
                    ));
                } else {
                    // L2401: advisory — public fn with a positional Bool parameter.
                    if f.is_pub {
                        for (idx, p) in f.params.iter().enumerate() {
                            if matches!(p.ty, Type::Bool)
                                && p.name != Syntax::KW_SELF
                                && p.default.is_none()
                            {
                                diags.push(Diagnostic::lint(
                                    "L2401",
                                    format!(
                                        "public function `{}` has a positional `Bool` parameter `{}`",
                                        f.name, p.name
                                    ),
                                    "positional booleans are easy to transpose at the call site"
                                        .to_string(),
                                    format!(
                                        "callers can write `{}: true` to make the intent clear (S61 labels)",
                                        p.name
                                    ),
                                    Some(p.name_span),
                                ));
                                let _ = idx;
                            }
                        }
                    }
                    // D-NARG-D2 (E0126): check defaults don't ref later params.
                    check_default_forward_refs(&f.params, &f.name, &mut diags);
                    funcs.insert(f.name.clone(), func_to_sig(f));
                }
            }
            Item::Struct(s) => register_struct(
                s,
                &mut registry,
                &mut struct_fields_legacy,
                &mut diags,
                &funcs,
                &consts,
            ),
            Item::Enum(e) => register_enum(e, &mut registry, &mut diags, &funcs, &consts),
            Item::Impl(i) => {
                if !registry.contains(&i.type_name) {
                    diags.push(Diagnostic::error(
                        "E0301",
                        format!("`impl {}` names a type that doesn't exist", i.type_name),
                        format!("`{}` hasn't been defined as a struct or enum", i.type_name),
                        format!(
                            "define `struct {}` or `enum {}` first",
                            i.type_name, i.type_name
                        ),
                        Some(i.type_span),
                    ));
                }
            }
            Item::Distinct(d) => register_distinct(d, &mut registry, &mut diags, &funcs, &consts),
            Item::TypeAlias(a) => {
                register_type_alias(a, &mut registry, &mut diags, &funcs, &consts);
            }
            // D-QUAL3: a unit family lowers to one `@Numeric` distinct type per
            // member, each erasing to `Float` — register them via the same path.
            Item::UnitFamily(uf) => {
                for d in uf.distinct_defs() {
                    register_distinct(&d, &mut registry, &mut diags, &funcs, &consts);
                }
            }
            Item::Const(c) => register_const(c, &mut consts, &mut diags, &funcs, &registry),
            Item::Test(t) => {
                if name_defined(&t.name, &funcs, &registry, &consts) || tests.contains_key(&t.name)
                {
                    diags.push(defined_twice(
                        &t.name,
                        "every test needs a unique name so failures are easy to find",
                        t.name_span,
                    ));
                } else {
                    tests.insert(t.name.clone(), t.name_span);
                }
            }
            // D-BENCH1: `#Bench` blocks are compiled by the bundle path; this
            // legacy single-Program path neither names nor runs them.
            Item::Bench(_) => {}
            Item::ExternRust(block) => {
                if check_extern_block(block, &registry, &mut diags) {
                    for ef in &block.functions {
                        register_extern_fn(ef, &mut funcs, &registry, &consts, &mut diags, false);
                    }
                }
            }
            // Stage 1a: modules are parsed but not yet type-checked; the U5
            // merge / eval pipeline consumes them. No runtime contribution.
            Item::Module(_) | Item::CodeModule(_) => {}
            // S59: C FFI modules are folded by CFFI::assemble before the
            // bundle path runs; this legacy single-Program path ignores them.
            Item::CModule(_) => {}
            // D-ERR-CONV: registration happens in trait_reg.register_items below.
            Item::ErrorConv(_) => {}
            // D-MIGRATE1: migration decls are handled by the schema diff pass.
            Item::Migration(_) => {}
            // D-STATE-DECL: state-set declarations are sema-only (I3); no type to register.
            Item::StateDecl(_) => {}
            Item::ProtocolDecl(_) => {}
            // D-METADERIVE1=A: user-authored derive blocks are expanded below.
            Item::UserDerive(_) => {}
            // D-GENMOD2=A: templates and aliases are erased before codegen.
            Item::GenericModule(_) | Item::ModuleAlias(_) => {}
        }
    }

    // D-METADERIVE1=A: user-derive expansion — run after registration so derive bodies
    // can reference helper functions and TypeInfo. All items are local; no orphan possible.
    {
        let user_derives: Vec<(String, String, Vec<crate::AST::Stmt>)> = prog
            .items
            .iter()
            .filter_map(|i| {
                if let Item::UserDerive(d) = i {
                    Some((d.trait_name.clone(), d.type_param.clone(), d.body.clone()))
                } else {
                    None
                }
            })
            .collect();

        if !user_derives.is_empty() {
            let struct_infos: Vec<&crate::AST::StructDef> = prog
                .items
                .iter()
                .filter_map(|i| {
                    if let Item::Struct(s) = i {
                        Some(s)
                    } else {
                        None
                    }
                })
                .collect();

            let actual_funcs: HashMap<String, &crate::AST::Func> = prog
                .items
                .iter()
                .filter_map(|i| {
                    if let Item::Func(f) = i {
                        Some((f.name.clone(), f))
                    } else {
                        None
                    }
                })
                .collect();

            let mut new_items: Vec<Item> = Vec::new();
            let project_root = std::env::current_dir().unwrap_or_default();

            for s in &struct_infos {
                for (derive_name, derive_span) in &s.derives {
                    if let Some((_, type_param, body)) =
                        user_derives.iter().find(|(tn, _, _)| tn == derive_name)
                    {
                        let type_info = crate::Comptime::build_struct_type_info(s);

                        match crate::Comptime::evaluate_derive_body(
                            body,
                            type_param,
                            type_info,
                            &actual_funcs,
                            &project_root,
                        ) {
                            Ok(fragments) => {
                                for fragment in fragments {
                                    let (toks, lex_diags) = crate::Lexer::lex(&fragment);
                                    if !lex_diags.is_empty() {
                                        let detail = lex_diags
                                            .first()
                                            .map(|d| d.what.as_str())
                                            .unwrap_or("the generated text could not be read");
                                        diags.push(Diagnostic::error(
                                            "E2710",
                                            format!(
                                                "`derive T.{}` generated invalid Jet while expanding `#[{}]` on `{}`",
                                                derive_name, derive_name, s.name
                                            ),
                                            format!(
                                                "generated source did not pass the ordinary lexer and parser: {detail}"
                                            ),
                                            "fix the `derive` body so every emitted fragment is valid Jet source".to_string(),
                                            Some(*derive_span),
                                        ));
                                        continue;
                                    }
                                    match crate::Parser::parse(&toks) {
                                        Ok(mut p) => new_items.extend(p.items.drain(..)),
                                        Err(parse_diags) => {
                                            let detail = parse_diags
                                                .first()
                                                .map(|d| d.what.as_str())
                                                .unwrap_or("the generated text was not valid Jet");
                                            diags.push(Diagnostic::error(
                                                "E2710",
                                                format!(
                                                    "`derive T.{}` generated invalid Jet while expanding `#[{}]` on `{}`",
                                                    derive_name, derive_name, s.name
                                                ),
                                                format!(
                                                    "generated source did not pass the ordinary lexer and parser: {detail}"
                                                ),
                                                "fix the `derive` body so every emitted fragment is valid Jet source".to_string(),
                                                Some(*derive_span),
                                            ));
                                        }
                                    }
                                }
                            }
                            Err(inner) => diags.push(Diagnostic::error(
                                "E2710",
                                format!(
                                    "`derive T.{}` body failed while expanding `#[{}]` on `{}`",
                                    derive_name, derive_name, s.name
                                ),
                                inner.what.clone(),
                                "fix the `derive` body so it generates valid Jet at compile time"
                                    .to_string(),
                                Some(*derive_span),
                            )),
                        }
                    }
                }
            }

            // Register new funcs so subsequent sema sees them.
            for item in &new_items {
                if let Item::Func(f) = item {
                    funcs.insert(f.name.clone(), func_to_sig(f));
                }
            }
            prog.items.extend(new_items);
        }
    }

    register_type_methods(&prog.items, &mut registry, &mut diags);
    register_patchable_methods(&prog.items, &mut registry);
    // Defaults must be evaluated before serde source expansion so generated
    // Decode bodies can embed the exact compile-time value as Jet source.
    let serde_core_imports: HashMap<String, String> = prog
        .imports
        .iter()
        .filter_map(|imp| Some((imp.alias.clone(), imp.core_module_path()?)))
        .collect();
    eval_default_markers(
        &mut prog.items,
        std::path::Path::new("."),
        &mut diags,
        &serde_core_imports,
    );
    // D-SERDE2=A/R11: built-in serde derives become ordinary Jet impl source,
    // then re-enter lexer/parser before any impl registration or checking.
    expand_builtin_serde_source(prog, &mut diags);

    // S62 + D-LIB2: synthesise before register_impl_methods so synthesised
    // Func nodes are visible when method lookup is registered.
    synthesize_impls(&mut prog.items);
    register_impl_methods(&prog.items, &mut registry, &mut diags);
    trait_reg.register_items(&prog.items, &mut diags);

    // S62: delegation validation — check the field exists and implements the trait.
    // Runs after trait_reg.register_items so implements_trait is populated.
    for item in &prog.items {
        if let Item::Impl(i) = item {
            if let (Some(trait_name), Some(field_name)) = (&i.trait_name, &i.delegation_field) {
                if let Some(fields) = registry.struct_fields(&i.type_name) {
                    if let Some((_, _, field_ty, _)) =
                        fields.iter().find(|(n, _, _, _)| n == field_name)
                    {
                        let field_type_name = field_ty.name();
                        if !trait_reg.implements_trait(&field_type_name, trait_name) {
                            diags.push(Diagnostic::error(
                                "E2401",
                                format!(
                                    "`{}` doesn't implement `{}`, so it can't delegate",
                                    field_type_name, trait_name
                                ),
                                format!(
                                    "`impl {}.{} using {}` forwards `{}` methods to the `{}` field, but `{}` doesn't implement `{}`",
                                    i.type_name, trait_name, field_name,
                                    trait_name, field_name,
                                    field_type_name, trait_name
                                ),
                                format!(
                                    "implement `impl {}.{}` on the field's type, or choose a different field",
                                    field_type_name, trait_name
                                ),
                                Some(i.type_span),
                            ));
                        }
                    } else {
                        diags.push(Diagnostic::error(
                            "E2401",
                            format!("`{}` has no field `{}`", i.type_name, field_name),
                            format!(
                                "`impl {}.{} using {}` needs `{}` to have a field named `{}`",
                                i.type_name, trait_name, field_name, i.type_name, field_name
                            ),
                            format!("add `{}: Type` to `struct {}`", field_name, i.type_name),
                            Some(i.type_span),
                        ));
                    }
                }
            }
        }
    }

    if mode == CompileMode::Run || mode == CompileMode::Eval {
        match funcs.get("run") {
            None => {
                diags.push(Diagnostic::error(
                    "E0101",
                    "this program has no `run` function".to_string(),
                    "running a program starts at `fn run`, and this file doesn't define one"
                        .to_string(),
                    "add one to this file: fn run() { ... }".to_string(),
                    None,
                ));
            }
            Some(sig) => {
                // E0122: in Run mode zero-arg `run` either returns nothing or
                // returns `Void ?` for top-level error reporting (D-S80-RUN1).
                // A single typed CLI parameter is checked later in Bundle.
                if mode == CompileMode::Run
                    && sig.params.is_empty()
                    && sig
                        .return_type
                        .as_ref()
                        .is_some_and(|ret| !is_fallible_void_return(ret))
                {
                    let span = prog.items.iter().find_map(|i| match i {
                        Item::Func(f) if f.name == "run" => Some(f.name_span),
                        _ => None,
                    });
                    diags.push(Diagnostic::error(
                        "E0122",
                        "`run` returns the wrong kind of value".to_string(),
                        "`run` is where running starts; it either returns nothing or reports top-level errors with `Void ?`".to_string(),
                        "write `fn run() { ... }`, or `fn run() -> Void ? { ... }` if the entry uses `?`".to_string(),
                        span,
                    ));
                }
            }
        }
    }
    match mode {
        CompileMode::Test if tests.is_empty() => {
            diags.push(Diagnostic::error(
                "E0601",
                format!("no `#{}` blocks found to run", Syntax::KW_TEST),
                format!(
                    "add at least one top-level block: #{} \"describes what this checks\" {{ ... }}",
                    Syntax::KW_TEST
                ),
                format!(
                    "use `{}` and `{}` inside the block to check results",
                    Syntax::BUILTIN_REQUIRE,
                    Syntax::BUILTIN_REQUIRE_EQ
                ),
                None,
            ));
        }
        CompileMode::Bench
        | CompileMode::Test
        | CompileMode::Run
        | CompileMode::Check
        | CompileMode::Eval => {}
    }

    // S57 (M9.5): evaluate comptime bindings before bodies are checked, so
    // references to them resolve. Single-file mode has no path; embed_file
    // resolves against the current directory.
    // D-CTCORE1: build core_imports from the file's `use` declarations so the
    // comptime interpreter can dispatch whitelisted pure Core calls.
    let single_core_imports: HashMap<String, String> = prog
        .imports
        .iter()
        .filter_map(|imp| {
            let module = imp.core_module_path()?;
            Some((imp.alias.clone(), module))
        })
        .collect();
    eval_comptime_items(
        &mut prog.items,
        &mut consts,
        std::path::Path::new("."),
        &mut diags,
        &single_core_imports,
        None,
    );
    let const_names: Vec<String> = consts.keys().cloned().collect();
    let mut address_taken: HashSet<String> = HashSet::new();
    for item in &prog.items {
        match item {
            Item::Func(f) => walk_stmts_for_const_refs(&f.body, &const_names, &mut address_taken),
            Item::Struct(s) => {
                for m in &s.methods {
                    walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                }
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                }
            }
            _ => {}
        }
    }
    for item in &mut prog.items {
        if let Item::Const(c) = item {
            let force_static = c.attrs.contains(&ConstAttr::ForceStatic);
            c.rust_kind = if force_static || address_taken.contains(&c.name) {
                RustConstKind::Static
            } else {
                RustConstKind::Const
            };
        }
    }

    let (ct_funcs, ct_externs, ct_globals) = comptime_context_from_items(&prog.items);
    let ct_base_dir = std::path::Path::new(".");

    // --- per-item body checks ---------------------------------------------
    // D-EFF1: per-function effect summaries collected during body checks; the
    // whole-program fixpoint + boundary checks run after the loop.
    let mut effect_summaries: HashMap<String, EffectSummary> = HashMap::new();
    // D-METHODMACRO1=A: top-level function names whose address was taken
    // anywhere in the program, accumulated across every function body check
    // below; the `@InlineAlways` address-taken pass (E0918) runs after the
    // loop, once this set is complete.
    let mut global_addr_taken: HashSet<String> = HashSet::new();
    // D-MEM1/S7 (D-NOALLOC-SEM1=A): captured once — every function body check
    // below for this file gets the same file-scoped `policy no_alloc` state.
    let no_alloc = prog.no_alloc_policy.is_some();
    let no_prelude = prog.no_prelude;
    for item in &mut prog.items {
        match item {
            Item::Func(f) => {
                diags.extend(check_func_body(
                    f,
                    &funcs,
                    &registry,
                    &struct_fields_legacy,
                    &consts,
                    &trait_reg,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    ct_base_dir,
                    &ct_globals,
                    false,
                    &mut effect_summaries,
                    &mut global_addr_taken,
                    no_alloc,
                no_prelude,
                ));
            }
            Item::Impl(i) => {
                for m in &mut i.methods {
                    diags.extend(check_func_body(
                        m,
                        &funcs,
                        &registry,
                        &struct_fields_legacy,
                        &consts,
                        &trait_reg,
                        Some(&i.type_name),
                        &ct_funcs,
                        &ct_externs,
                        ct_base_dir,
                        &ct_globals,
                        false,
                        &mut effect_summaries,
                        &mut global_addr_taken,
                        no_alloc,
                    no_prelude,
                    ));
                }
            }
            Item::Struct(s) => {
                for m in &mut s.methods {
                    diags.extend(check_func_body(
                        m,
                        &funcs,
                        &registry,
                        &struct_fields_legacy,
                        &consts,
                        &trait_reg,
                        Some(&s.name),
                        &ct_funcs,
                        &ct_externs,
                        ct_base_dir,
                        &ct_globals,
                        false,
                        &mut effect_summaries,
                        &mut global_addr_taken,
                        no_alloc,
                    no_prelude,
                    ));
                }
                for block in &mut s.trait_impls {
                    for m in &mut block.methods {
                        diags.extend(check_func_body(
                            m,
                            &funcs,
                            &registry,
                            &struct_fields_legacy,
                            &consts,
                            &trait_reg,
                            Some(&s.name),
                            &ct_funcs,
                            &ct_externs,
                            ct_base_dir,
                            &ct_globals,
                            false,
                            &mut effect_summaries,
                            &mut global_addr_taken,
                            no_alloc,
                        no_prelude,
                        ));
                    }
                }
            }
            Item::Enum(e) => {
                for m in &mut e.methods {
                    diags.extend(check_func_body(
                        m,
                        &funcs,
                        &registry,
                        &struct_fields_legacy,
                        &consts,
                        &trait_reg,
                        Some(&e.name),
                        &ct_funcs,
                        &ct_externs,
                        ct_base_dir,
                        &ct_globals,
                        false,
                        &mut effect_summaries,
                        &mut global_addr_taken,
                        no_alloc,
                    no_prelude,
                    ));
                }
            }
            Item::Test(t) => {
                let mut synthetic = crate::AST::Func {
                    span: t.name_span,
                    is_pub: false,
                    is_package_pub: false,
                    external_type: None,
                    name: format!("__test_{}", t.name),
                    name_span: t.name_span,
                    meta: None,
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: None,
                    return_type_span: None,
                    is_unsafe: false,
                    unsafe_reason: None,
                    unsafe_span: None,
                    is_pure: false,
                    is_reactive: false,
            is_replayable: false,
            replayable_span: None,
                    is_sanitizer: false,
                    declared_effects: None,
        effect_via: None,
        state_requires: None,
            state_transition: None,
            web_marker: None,
            pre: Vec::new(),
            post: Vec::new(),
            is_must_use: false,
            must_use_span: None,
            maturity: None,
            maturity_span: None,
            is_inline: false,
            is_inline_always: false,
            inline_span: None,
                    body: std::mem::take(&mut t.body),
                };
                diags.extend(check_func_body(
                    &mut synthetic,
                    &funcs,
                    &registry,
                    &struct_fields_legacy,
                    &consts,
                    &trait_reg,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    ct_base_dir,
                    &ct_globals,
                    false,
                    &mut effect_summaries,
                    &mut global_addr_taken,
                    no_alloc,
                no_prelude,
                ));
                t.body = synthetic.body;
            }
            // D-ERR-CONV: type-check the conversion body with `self: from_ty`, return `to_ty`.
            Item::ErrorConv(ec) => {
                diags.extend(check_error_conv_body(
                    ec,
                    &funcs,
                    &registry,
                    &struct_fields_legacy,
                    &consts,
                    &trait_reg,
                    &ct_funcs,
                    &ct_externs,
                    ct_base_dir,
                    &ct_globals,
                    no_alloc,
                    no_prelude,
                ));
            }
            Item::Const(_)
            | Item::ExternRust(_)
            | Item::Trait(_)
            | Item::Tag(_) // D-QUAL2: tags carry no body to check
            | Item::Module(_)
            | Item::CModule(_)
            | Item::CodeModule(_)
            | Item::Distinct(_)
            | Item::TypeAlias(_) // D-TYPEALIAS1: erases at codegen
            | Item::UnitFamily(_) // D-QUAL3: lowered to distinct types at registration
            | Item::Bench(_)
            | Item::Migration(_) // D-MIGRATE1
            | Item::StateDecl(_) // D-STATE-DECL
            | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: state-set decls are sema-only (I3)
            | Item::UserDerive(_) // D-METADERIVE1=A: expanded in Bundle.rs
            | Item::GenericModule(_) // D-GENMOD2=A: template — erases
            | Item::ModuleAlias(_) => {} // D-GENMOD2=A: alias — erases after expansion
        }
    }

    // D-METHODMACRO1=A: E0918 (address-taken) needs every function body checked
    // first — `global_addr_taken` is only complete once the loop above has run.
    // Methods can't appear in it (Jet's grammar has no way to read a method's
    // bare name as a value), so this only ever fires for top-level functions.
    for item in &prog.items {
        if let Item::Func(f) = item {
            if f.is_inline_always && global_addr_taken.contains(&f.name) {
                diags.push(super::e0918_address_taken(
                    &f.name,
                    f.inline_span.unwrap_or(f.name_span),
                ));
            }
        }
    }

    // D-EFF2 (`#(via f)`): seed each via-fn's summary with its callback's bound
    // before the fixpoint, so its published effect set is a tight pass-through.
    apply_effect_via(&prog.items, &mut effect_summaries, &mut diags);
    // D-EFF1: whole-program effect fixpoint, then enforce every `#(…)` bound and
    // every `#Caps(…)` region restriction.
    let solved = solve(&effect_summaries);
    check_effect_boundaries(&prog.items, &solved, &mut diags);
    check_replayable_effects(&prog.items, &solved, &mut diags);
    check_region_caps(&effect_summaries, &solved, &mut diags);
    // D-EFF2: callback param effect bounds (E0747).
    check_callback_bounds(&effect_summaries, &solved, &mut diags);
    // U13 (D-JPK-SECRETCRYPTO1): a `core.vault.get` reach requires `Secret` in
    // the reaching function's own declared `#(…)` bound — E1264.
    check_secret_grants(&prog.items, &effect_summaries, &mut diags);

    // D-TAINT1: taint tracking — `#Tainted` value-facts propagate, a `#Sanitizer
    // fn` clears taint, and a tainted value reaching a sink effect (Db/Exec/Net)
    // without sanitizing is E0721. Static, erased in codegen (I3).
    let mut sanitizers: HashSet<String> = HashSet::new();
    collect_sanitizers(&prog.items, &mut sanitizers);
    check_program_taint(&prog.items, &sanitizers, &single_core_imports, &mut diags);

    // D-STATE1 / D-STATE-DECL: typestate — state-set declarations validated (E0151,
    // L0151); per-body forward dataflow checks wrong-state calls (E0150). Erased (I3).
    let state_tbl = crate::Sema::StateTable::build(&prog.items);
    if !state_tbl.is_empty() {
        state_tbl.validate_declarations(&prog.items, &mut diags);
        crate::Sema::check_items_state(&prog.items, &state_tbl, &mut diags);
    }

    diags
}

/// D-SERDE2=A: render the built-in struct field walk as the same hand-writable
/// `Encode`/`Decode` impl a user writes, parse it, and attach the parsed methods.
/// No AST or Rust body is synthesized here: malformed output is E2710 and every
/// generated method proceeds through ordinary registration, sema, TIR, and codegen.
pub(super) fn expand_builtin_serde_source(prog: &mut crate::AST::Program, diags: &mut Vec<Diagnostic>) {
    expand_builtin_serde_items(&mut prog.items, diags);
}

pub(super) fn expand_builtin_serde_items(items: &mut Vec<Item>, diags: &mut Vec<Diagnostic>) {
    let mut generated_items = Vec::new();
    for item in items.iter_mut() {
        if let Item::Enum(e) = item {
            expand_builtin_enum_serde(e, diags, &mut generated_items);
            continue;
        }
        let Item::Struct(s) = item else { continue };
        let enc = s.derives.iter().any(|(n, _)| n == crate::Generics::ENCODE);
        let dec = s.derives.iter().any(|(n, _)| n == crate::Generics::DECODE);
        if !enc && !dec { continue; }

        // The synthetic container exists only to make the generated codec pass
        // through the ordinary parser/checker.  Its inherited parameters need
        // the same wire bounds that the final Rust impl receives; otherwise a
        // field of type `T` is (correctly) rejected as not encodable while the
        // generated Encode body is checked.
        let mut codec_params = s.type_params.clone();
        let wire_types = s.fields.iter()
            .filter(|f| !f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_SKIP))
            .map(|f| &f.ty)
            .collect::<Vec<_>>();
        for param in &mut codec_params {
            let reaches_wire = wire_types.iter()
                .any(|ty| crate::Generics::free_type_params(ty).contains(&param.name));
            if reaches_wire && enc && !param.bounds.iter().any(|b| b == crate::Generics::ENCODE) {
                param.bounds.push(crate::Generics::ENCODE.to_string());
            }
            if reaches_wire && dec && !param.bounds.iter().any(|b| b == crate::Generics::DECODE) {
                param.bounds.push(crate::Generics::DECODE.to_string());
            }
        }
        let params = crate::Generics::format_type_params(&codec_params);
        let target = format!("{}{}", s.name, serde_type_arg_names(&s.type_params));
        let mut source = String::new();
        if enc {
            source.push_str(&format!("impl {}.Encode {{\nfn encode{params}(self) -> DataTree {{\n", s.name));
            let active: Vec<_> = s.fields.iter().filter(|f|
                !f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_SKIP)
            ).collect();
            let needs_mutation = active.iter().any(|f|
                f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_FLATTEN)
                || matches!(f.ty, Type::Option(_))
            );
            if !needs_mutation {
                let pairs = active.iter().map(|f| {
                    let key = serde_source_field_key(&s.serde_markers, f);
                    format!("{key:?}: self.{}.encode()", f.name)
                }).collect::<Vec<_>>().join(", ");
                source.push_str(&format!("return DataTree.Object([{pairs}])\n"));
            } else {
                source.push_str("out: [String: DataTree] := []\n");
            for f in &s.fields {
                if f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_SKIP)
                { continue; }
                let key = serde_source_field_key(&s.serde_markers, f);
                if f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_FLATTEN) {
                    source.push_str(&format!(
                        "nested :: self.{}.encode()\nif nested == .Object(entries) {{ loop key, value in entries {{ out[key] = value }} }}\n",
                        f.name
                    ));
                } else if matches!(f.ty, Type::Option(_)) {
                    source.push_str(&format!(
                        "if self.{} == Val(value) {{ out[{:?}] = (copy value).encode() }}\n",
                        f.name, key
                    ));
                } else {
                    source.push_str(&format!("out[{:?}] = self.{}.encode()\n", key, f.name));
                }
            }
                source.push_str("return DataTree.Object(out)\n");
            }
            source.push_str("}\n}\n");
        }
        if dec {
            source.push_str(&format!("impl {}.Decode {{\n", s.name));
            source.push_str(&format!("fn decode{params}(tree: DataTree) -> {target} ? DecodeError {{\n"));
            let deny_unknown = s.serde_markers.iter().any(|m|
                m.name == crate::Syntax::ATTR_DENY_UNKNOWN_FIELDS
            );
            let has_flatten = s.fields.iter().any(|f|
                f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_FLATTEN)
            );
            if deny_unknown && !has_flatten {
                let keys = s.fields.iter()
                    .filter(|f| !f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_SKIP))
                    .map(|f| format!("{:?}", serde_source_field_key(&s.serde_markers, f)))
                    .collect::<Vec<_>>()
                    .join(", ");
                source.push_str(&format!(
                    "if (copy tree) == .Object(entries) {{ loop key, value in entries {{ if ![{keys}].contains(key) {{ return err(DecodeError.{{ path: copy key, reason: \"E2412: unknown field `{{key}}`\" }}) }} }} }}\n"
                ));
            }
            source.push_str(&format!("return ok({target}.{{\n"));
            for f in s.fields.iter().filter(|f| f.computed.is_none()) {
                let value = if f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_SKIP) {
                    serde_source_default(f).unwrap_or_else(|| serde_source_zero(&f.ty))
                } else if f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_FLATTEN) {
                    format!("tree.decode<{}>()?", serde_type_source(&f.ty))
                } else {
                    let key = serde_source_field_key(&s.serde_markers, f);
                    let subtree = if matches!(f.ty, Type::Option(_)) {
                        format!("(tree.field({key:?}) ?? DataTree.Null)")
                    } else if let Some(default) = serde_source_default(f) {
                        format!("(tree.field({key:?}) ?? {default}.encode())")
                    } else {
                        format!("(tree.field({key:?})?)")
                    };
                    format!("{subtree}.decode<{}>()?", serde_type_source(&f.ty))
                };
                source.push_str(&format!("{}: {},\n", f.name, value));
            }
            source.push_str("})\n}\n}\n");
        }
        let trigger_span = s.derives.iter()
            .find(|(name, _)| matches!(name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE))
            .map(|(_, span)| *span)
            .unwrap_or(s.name_span);
        match parse_builtin_serde_fragment(&source, &s.name, trigger_span, diags) {
            Some(generated) => {
                generated_items.extend(generated.into_iter().filter_map(|item| match item {
                    Item::Impl(mut imp) => {
                        imp.is_generated_serde = true;
                        Some(Item::Impl(imp))
                    }
                    _ => None,
                }));
            }
            None => {}
        }
    }
    items.extend(generated_items);
}

fn expand_builtin_enum_serde(
    e: &mut crate::AST::EnumDef,
    diags: &mut Vec<Diagnostic>,
    generated_items: &mut Vec<Item>,
) {
    let enc = e.derives.iter().any(|(n, _)| n == crate::Generics::ENCODE);
    let dec = e.derives.iter().any(|(n, _)| n == crate::Generics::DECODE);
    if !enc && !dec { return; }
    let mut codec_params = e.type_params.clone();
    let wire_types = e.variants.iter().flat_map(|v| match &v.payload {
        crate::AST::VariantPayload::Unit => Vec::new(),
        crate::AST::VariantPayload::Single(t, _) => vec![t],
        crate::AST::VariantPayload::Named(fs) => fs.iter().map(|f| &f.ty).collect(),
    }).collect::<Vec<_>>();
    for param in &mut codec_params {
        let reaches_wire = wire_types.iter().any(|ty|
            crate::Generics::free_type_params(ty).contains(&param.name));
        if reaches_wire && enc { param.bounds.push(crate::Generics::ENCODE.to_string()); }
        if reaches_wire && dec { param.bounds.push(crate::Generics::DECODE.to_string()); }
    }
    let params = crate::Generics::format_type_params(&codec_params);
    let target = format!("{}{}", e.name, serde_type_arg_names(&e.type_params));
    let tag = e.serde_markers.iter().find(|m| m.name == crate::Syntax::ATTR_TAG)
        .and_then(|m| m.args.first()).and_then(|x| match x {
            crate::AST::Expr::Str(parts, _) => parts.first().and_then(|p| match p {
                crate::AST::StrPart::Lit(s) => Some(s.clone()), _ => None,
            }), _ => None,
        });
    let untagged = e.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_UNTAGGED);
    let mut source = String::new();
    if enc {
        source.push_str(&format!("impl {}.Encode {{\nfn encode{params}(self) -> DataTree {{\nif self == {{\n", e.name));
        for v in &e.variants {
            let wire = serde_enum_variant_key(v);
            let (pattern, payload) = serde_enum_pattern_and_value(v);
            let value = if untagged {
                match &v.payload {
                    crate::AST::VariantPayload::Unit => "DataTree.Null".to_string(),
                    crate::AST::VariantPayload::Single(..) => "(copy v0).encode()".to_string(),
                    crate::AST::VariantPayload::Named(fs) => format!("DataTree.Object([{}])", serde_enum_named_pairs(fs)),
                }
            } else if let Some(tag_key) = &tag {
                match &v.payload {
                    crate::AST::VariantPayload::Unit => format!("DataTree.Object([{tag_key:?}: DataTree.Text({wire:?})])"),
                    crate::AST::VariantPayload::Named(fs) => {
                        let pairs = serde_enum_named_pairs(fs);
                        format!("DataTree.Object([{tag_key:?}: DataTree.Text({wire:?}){}{}])", if pairs.is_empty(){""}else{" ,"}, pairs)
                    }
                    crate::AST::VariantPayload::Single(..) => format!(
                        "DataTree.Object([{tag_key:?}: DataTree.Text({wire:?}), \"value\": {payload}])"
                    ),
                }
            } else {
                match &v.payload {
                    crate::AST::VariantPayload::Unit => format!("DataTree.Text({wire:?})"),
                    crate::AST::VariantPayload::Single(..) => format!("DataTree.Object([{wire:?}: {payload}])"),
                    crate::AST::VariantPayload::Named(fs) => format!("DataTree.Object([{wire:?}: DataTree.Object([{}])])", serde_enum_named_pairs(fs)),
                }
            };
            source.push_str(&format!("{pattern} -> {{ return {value} }}\n"));
        }
        source.push_str("}\n}\n}\n");
    }
    if dec {
        source.push_str(&format!("impl {}.Decode {{\nfn decode{params}(tree: DataTree) -> {target} ? DecodeError {{\n", e.name));
        if untagged {
            for v in &e.variants {
                source.push_str(&serde_enum_decode_attempt(&target, v, "tree", true));
            }
        } else if let Some(tag_key) = &tag {
            source.push_str(&format!("tag_tree := tree.field({tag_key:?})?\ntag_value := tag_tree.text()?\n"));
            for v in &e.variants {
                let wire = serde_enum_variant_key(v);
                let payload_source = if matches!(v.payload, crate::AST::VariantPayload::Single(..)) {
                    "(tree.field(\"value\")?)"
                } else {
                    "tree"
                };
                source.push_str(&format!(
                    "if tag_value == {wire:?} {{ {} }}\n",
                    serde_enum_decode_return(&target, v, payload_source)
                ));
            }
        } else {
            let mut object_arms = String::new();
            for (variant_index, v) in e.variants.iter().enumerate() {
                match &v.payload {
                    crate::AST::VariantPayload::Unit => {
                        let wire = serde_enum_variant_key(v);
                        source.push_str(&format!("if (copy tree) == .Text(variant_name) {{ if variant_name == {wire:?} {{ return ok({target}.{}) }} }}\n", v.name));
                    }
                    _ => {
                        let wire = serde_enum_variant_key(v);
                        let candidate = format!("candidate_{variant_index}");
                        object_arms.push_str(&format!("{candidate}: DataTree := (copy tree).field({wire:?}) ?? DataTree.Null\n"));
                        match &v.payload {
                            crate::AST::VariantPayload::Single(t, _) => {
                                let decoded = format!("decoded_{variant_index}");
                                object_arms.push_str(&format!("{decoded} := {candidate}.decode<{}>()\nif {decoded} == ok(decoded_value) {{ return ok({target}.{}(decoded_value)) }}\n", serde_type_source(t), v.name));
                            }
                            crate::AST::VariantPayload::Named(_) => {
                                object_arms.push_str(&format!("{}\n", serde_enum_decode_return(&target, v, &candidate)));
                            }
                            crate::AST::VariantPayload::Unit => {}
                        }
                    }
                }
            }
            source.push_str(&object_arms);
        }
        source.push_str("return err(DecodeError.{ path: \"\", reason: \"no matching enum variant\" })\n}\n}\n");
    }
    let trigger_span = e.derives.iter()
        .find(|(name, _)| matches!(name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE))
        .map(|(_, span)| *span)
        .unwrap_or(e.name_span);
    match parse_builtin_serde_fragment(&source, &e.name, trigger_span, diags) {
        Some(generated) => {
            generated_items.extend(generated.into_iter().filter_map(|item| match item {
                Item::Impl(mut imp) => {
                    imp.is_generated_serde = true;
                    Some(Item::Impl(imp))
                }
                _ => None,
            }));
        }
        None => {}
    }
}

fn parse_builtin_serde_fragment(
    source: &str,
    type_name: &str,
    trigger_span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Vec<Item>> {
    let (tokens, lex_diags) = crate::Lexer::lex(source);
    let parsed = if lex_diags.is_empty() {
        crate::Parser::parse(&tokens)
    } else {
        Err(lex_diags)
    };
    match parsed {
        Ok(generated) => Some(generated.items),
        Err(errors) => {
            let detail = errors
                .first()
                .map(|d| format!("{} at {:?}", d.what, d.span))
                .unwrap_or_else(|| "generated codec source was invalid".to_string());
            diags.push(Diagnostic::error(
                "E2710",
                format!("built-in codec derive generated invalid Jet for `{type_name}`"),
                format!(
                    "generated source did not pass the ordinary lexer and parser: {detail}; generated source:\n{source}"
                ),
                "report this compiler bug; built-in derives must emit valid ordinary Jet".to_string(),
                Some(trigger_span),
            ));
            None
        }
    }
}

#[cfg(test)]
mod serde_source_tests {
    use super::*;

    #[test]
    fn builtin_codecs_remain_parsed_top_level_impls() {
        let source = "@[Codable]\nstruct Point { x: Int }\n";
        let (tokens, lex_diags) = crate::Lexer::lex(source);
        assert!(lex_diags.is_empty());
        let mut program = crate::Parser::parse(&tokens).expect("source parses");
        let mut diags = Vec::new();
        expand_builtin_serde_items(&mut program.items, &mut diags);
        assert!(diags.is_empty(), "generated source must parse: {diags:?}");

        let point = program.items.iter().find_map(|item| match item {
            Item::Struct(s) if s.name == "Point" => Some(s),
            _ => None,
        }).expect("real type remains");
        assert!(point.trait_impls.is_empty(), "no parsed block may be transplanted into the type");
        assert!(!program.items.iter().any(|item| match item {
            Item::Struct(s) => s.name.starts_with("__JetSerde"),
            Item::Enum(e) => e.name.starts_with("__JetSerde"),
            _ => false,
        }));
        let protocols: Vec<_> = program.items.iter().filter_map(|item| match item {
            Item::Impl(i) if i.type_name == "Point" => i.trait_name.as_deref(),
            _ => None,
        }).collect();
        assert_eq!(protocols, vec!["Encode", "Decode"]);
    }

    #[test]
    fn malformed_builtin_codec_points_at_derive_trigger() {
        let trigger = Span::new(17, 26);
        let mut diags = Vec::new();
        assert!(parse_builtin_serde_fragment(
            "impl Broken.Encode { fn encode(self) -> DataTree {",
            "Broken",
            trigger,
            &mut diags,
        ).is_none());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "E2710");
        assert_eq!(diags[0].span, Some(trigger));
    }
}

fn serde_enum_variant_key(v: &crate::AST::Variant) -> String {
    v.serde_markers.iter().find(|m| m.name == crate::Syntax::ATTR_RENAME)
        .and_then(|m| m.args.first()).and_then(|e| match e {
            crate::AST::Expr::Str(parts, _) => parts.first().and_then(|p| match p { crate::AST::StrPart::Lit(s) => Some(s.clone()), _ => None }), _ => None,
        }).unwrap_or_else(|| v.name.clone())
}

fn serde_enum_pattern_and_value(v: &crate::AST::Variant) -> (String, String) {
    match &v.payload {
        crate::AST::VariantPayload::Unit => (format!(".{}", v.name), String::new()),
        crate::AST::VariantPayload::Single(..) => (format!(".{}(v0)", v.name), "(copy v0).encode()".to_string()),
        crate::AST::VariantPayload::Named(fs) => {
            let names = (0..fs.len()).map(|i| format!("v{i}")).collect::<Vec<_>>();
            (format!(".{}({})", v.name, names.join(", ")), String::new())
        }
    }
}

fn serde_enum_named_pairs(fs: &[crate::AST::VariantField]) -> String {
    fs.iter().enumerate().map(|(i, f)| format!("{:?}: (copy v{i}).encode()", f.name)).collect::<Vec<_>>().join(", ")
}

fn serde_enum_decode_attempt(target: &str, v: &crate::AST::Variant, src: &str, guarded: bool) -> String {
    if guarded { format!("if {src}.decode<{}>() == ok(v0) {{ {} }}\n", serde_enum_payload_type(v), serde_enum_decode_return(target, v, src)) }
    else { serde_enum_decode_return(target, v, src) }
}

fn serde_enum_decode_return(target: &str, v: &crate::AST::Variant, src: &str) -> String {
    let cons = serde_enum_decode_constructor(target, v, src);
    if matches!(v.payload, crate::AST::VariantPayload::Named(_)) {
        format!("decoded_variant: {target} := {cons}\nreturn ok(decoded_variant)")
    } else {
        format!("return ok({cons})")
    }
}

fn serde_enum_payload_type(v: &crate::AST::Variant) -> String {
    match &v.payload {
        crate::AST::VariantPayload::Unit => "DataTree".to_string(),
        crate::AST::VariantPayload::Single(t, _) => serde_type_source(t),
        crate::AST::VariantPayload::Named(_) => "DataTree".to_string(),
    }
}

fn serde_enum_decode_constructor(target: &str, v: &crate::AST::Variant, src: &str) -> String {
    match &v.payload {
        crate::AST::VariantPayload::Unit => format!("{target}.{}", v.name),
        crate::AST::VariantPayload::Single(t, _) => format!("{target}.{}({src}.decode<{}>()?)", v.name, serde_type_source(t)),
        crate::AST::VariantPayload::Named(fs) => format!(".{}.{{ {} }}", v.name, fs.iter().map(|f| format!("{}: ((copy {src}).field({:?})?).decode<{}>()?", f.name, f.name, serde_type_source(&f.ty))).collect::<Vec<_>>().join(", ")),
    }
}

fn serde_source_field_key(container: &[crate::AST::Marker], f: &crate::AST::Field) -> String {
    if let Some(marker) = f.serde_markers.iter().find(|m| m.name == crate::Syntax::ATTR_RENAME) {
        if let Some(crate::AST::Expr::Str(parts, _)) = marker.args.first() {
            if let Some(crate::AST::StrPart::Lit(value)) = parts.first() { return value.clone(); }
        }
    }
    let style = container.iter().find(|m| m.name == crate::Syntax::ATTR_RENAME_ALL)
        .and_then(|m| m.args.first()).and_then(|e| match e { crate::AST::Expr::Ident(n, _) => Some(n.as_str()), _ => None });
    match style {
        Some("camel") => { let mut parts = f.name.split('_'); let mut out = parts.next().unwrap_or("").to_string(); for p in parts { let mut c=p.chars(); if let Some(x)=c.next(){out.extend(x.to_uppercase());out.push_str(c.as_str());} } out }
        Some("kebab") => f.name.replace('_', "-"),
        Some("screaming") => f.name.to_uppercase(),
        Some("pascal") => f.name.split('_').map(|p| { let mut c=p.chars(); c.next().map(|x| x.to_uppercase().collect::<String>()+c.as_str()).unwrap_or_default() }).collect(),
        _ => f.name.clone(),
    }
}

fn serde_source_default(f: &crate::AST::Field) -> Option<String> {
    let marker = f.serde_markers.iter().find(|m| m.name == crate::Syntax::ATTR_DEFAULT)?;
    match (marker.args.first(), marker.ct.as_ref()) {
        (Some(_), Some(value)) => serde_ct_source(value),
        (Some(expr), None) => serde_source_literal(expr),
        (None, _) => None,
    }
}

fn serde_ct_source(value: &crate::AST::CtValue) -> Option<String> {
    use crate::AST::CtValue;
    Some(match value {
        CtValue::Int(v) => v.to_string(),
        CtValue::Float(v) => format!("{v:?}"),
        CtValue::Bool(v) => v.to_string(),
        CtValue::Char(v) => format!("{v:?}"),
        CtValue::Str(v) => format!("{v:?}"),
        CtValue::BigInt(v) => format!("BigInt({:?})", v.to_string_rep()),
        CtValue::Bytes(values) => format!(
            "[{}]",
            values.iter().map(u8::to_string).collect::<Vec<_>>().join(", ")
        ),
        CtValue::List(values) => format!(
            "[{}]",
            values.iter().map(serde_ct_source).collect::<Option<Vec<_>>>()?.join(", ")
        ),
        CtValue::Map(values) => format!(
            "[{}]",
            values.iter().map(|(key, value)| Some(format!(
                "{}: {}",
                serde_ct_source(&key.to_value())?,
                serde_ct_source(value)?
            ))).collect::<Option<Vec<_>>>()?.join(", ")
        ),
        CtValue::Struct { type_name, fields } => format!(
            "{type_name}.{{ {} }}",
            fields.iter().map(|(name, value)| Some(format!(
                "{name}: {}",
                serde_ct_source(value)?
            ))).collect::<Option<Vec<_>>>()?.join(", ")
        ),
        CtValue::Enum { type_name, variant, args } => {
            if args.is_empty() {
                format!("{type_name}.{variant}")
            } else if args.iter().all(|(label, _)| label.is_none()) {
                format!(
                    "{type_name}.{variant}({})",
                    args.iter().map(|(_, value)| serde_ct_source(value)).collect::<Option<Vec<_>>>()?.join(", ")
                )
            } else {
                format!(
                    "{type_name}.{variant}.{{ {} }}",
                    args.iter().map(|(label, value)| Some(format!(
                        "{}: {}",
                        label.as_ref()?,
                        serde_ct_source(value)?
                    ))).collect::<Option<Vec<_>>>()?.join(", ")
                )
            }
        }
        CtValue::Some(value) => format!("Val({})", serde_ct_source(value)?),
        CtValue::None(_) => "None".to_string(),
        CtValue::ResOk(value) => format!("ok({})", serde_ct_source(value)?),
        CtValue::ResErr(value) => format!("err({})", serde_ct_source(value)?),
        CtValue::Unit | CtValue::Closure(_) => return None,
    })
}

fn serde_source_literal(e: &crate::AST::Expr) -> Option<String> {
    match e {
        crate::AST::Expr::Int(v, _, _) => Some(v.to_string()),
        crate::AST::Expr::Float(v, _, _) => Some(v.to_string()),
        crate::AST::Expr::Bool(v, _) => Some(v.to_string()),
        crate::AST::Expr::Char(v, _) => Some(format!("{v:?}")),
        crate::AST::Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
            crate::AST::StrPart::Lit(v) => Some(format!("{v:?}")),
            _ => None,
        },
        crate::AST::Expr::ListLit(values, _) => Some(format!("[{}]", values.iter().map(serde_source_literal).collect::<Option<Vec<_>>>()?.join(", "))),
        crate::AST::Expr::MapLit(values, _) => Some(format!("[{}]", values.iter().map(|(k,v)| Some(format!("{}: {}", serde_source_literal(k)?, serde_source_literal(v)?))).collect::<Option<Vec<_>>>()?.join(", "))),
        _ => None,
    }
}

fn serde_source_zero(ty: &Type) -> String {
    match ty {
        Type::Int | Type::IntN { .. } => "0".to_string(),
        Type::Float | Type::Float32 => "0.0".to_string(),
        Type::Bool => "false".to_string(),
        Type::String => "\"\"".to_string(),
        Type::Option(_) => "None".to_string(),
        Type::List(_) | Type::Map { .. } => "[]".to_string(),
        _ => format!("{}.{{}}", serde_type_source(ty)),
    }
}

fn serde_type_arg_names(params: &[crate::AST::TypeParam]) -> String {
    if params.is_empty() { String::new() } else {
        format!("<{}>", params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "))
    }
}

fn serde_type_source(ty: &Type) -> String {
    match ty {
        Type::Int => "Int".to_string(), Type::Float => "Float".to_string(),
        Type::Bool => "Bool".to_string(), Type::String => "String".to_string(),
        Type::Char => "Char".to_string(), Type::Named(n) => n.clone(),
        Type::List(t) => format!("[{}]", serde_type_source(t)),
        Type::Map { key, value } => format!("[{}: {}]", serde_type_source(key), serde_type_source(value)),
        Type::Option(t) => format!("{}?", serde_type_source(t)),
        Type::Result { ok, err } => format!("{} ? {}", serde_type_source(ok), serde_type_source(err)),
        Type::Apply { name, args } => format!("{}<{}>", name, args.iter().map(serde_type_source).collect::<Vec<_>>().join(", ")),
        Type::IntN { signed, bits } => format!("{}{}", if *signed { "I" } else { "U" }, bits),
        Type::Float32 => "F32".to_string(),
        Type::FixedList { elem, len } => format!("[{}#{}]", serde_type_source(elem), len),
        Type::Shared(t) => format!("shared {}", serde_type_source(t)),
        Type::Tagged { marker, inner } => format!("#{} {}", marker, serde_type_source(inner)),
        Type::Tuple(fields) => format!("({})", fields.iter().map(|(n,t)| format!("{}: {}", n, serde_type_source(t))).collect::<Vec<_>>().join(", ")),
        Type::TraitObject(names) => format!("dyn {}", names.join(" + ")),
        Type::Fn { .. } => "fn()".to_string(),
    }
}

/// D-TAINT1: run the taint pass over every function/method body in a single
/// Program (the legacy single-file path). `core_imports` resolves Core aliases
/// to module paths so a sink call (Db/Exec/Net effect) can be recognized.
fn check_program_taint(
    items: &[Item],
    sanitizers: &HashSet<String>,
    core_imports: &HashMap<String, String>,
    diags: &mut Vec<Diagnostic>,
) {
    let run = |body: &[crate::AST::Stmt], diags: &mut Vec<Diagnostic>| {
        diags.extend(check_func_taint(body, sanitizers, core_imports));
    };
    for item in items {
        match item {
            Item::Func(f) => run(&f.body, diags),
            Item::Impl(i) => {
                for m in &i.methods {
                    run(&m.body, diags);
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    run(&m.body, diags);
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        run(&m.body, diags);
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    run(&m.body, diags);
                }
            }
            Item::Test(t) => run(&t.body, diags),
            Item::ErrorConv(ec) => run(&ec.body, diags),
            _ => {}
        }
    }
}

/// D-EFF1: enforce declared `#(…)` effect bounds against inferred sets. For each
/// function (and method) carrying a `#(…)` list, the inferred set must be a
/// subset of the declared set — any extra effect is E0740. A `@Pure fn` with a
/// non-empty `#(…)` list is the contradiction E0745 (the empty-set contract of
/// `@Pure` is otherwise enforced by E3401). `@Pure` with no list needs no check
/// here: its purity is the E3401 path.
pub(crate) fn check_effect_boundaries(
    items: &[Item],
    solved: &HashMap<String, EffectSet>,
    diags: &mut Vec<Diagnostic>,
) {
    fn check_one(
        f: &Func,
        owner: Option<&str>,
        solved: &HashMap<String, EffectSet>,
        diags: &mut Vec<Diagnostic>,
    ) {
        let Some(declared_list) = &f.declared_effects else {
            return;
        };
        // E0745: `@Pure` already pins the empty set; a `#(…)` list contradicts it.
        if f.is_pure && !declared_list.is_empty() {
            diags.push(e0745(&f.name, f.name_span));
            return;
        }
        // Validate names and build the declared set; an unknown name is E0119.
        // A bad name leaves the declared set incomplete, so skip the subset
        // check to avoid a misleading E0740 piled on top of the real problem.
        // D-PROP2=A: names starting with `!` are prohibitions — validated separately.
        let mut declared: EffectSet = EffectSet::new();
        let mut prohibited: EffectSet = EffectSet::new();
        let mut bad_name = false;
        for (name, span) in declared_list {
            let (is_prohibited, base_name) = if name.starts_with('!') {
                (true, &name[1..])
            } else {
                (false, name.as_str())
            };
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
            .get(&effect_key(owner, &f.name))
            .cloned()
            .unwrap_or_default();
        // E0740: only check positive bounds — prohibition-only annotations (`#(!Net)`)
        // have no upper-bound constraint; only the prohibition check applies.
        // D-EFFTREE1: `declared` may name ancestor roots — an ancestor entry
        // covers any effect at or below it, so this is a subsumption-aware
        // check, not a flat set difference.
        if !declared.is_empty() {
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
            Item::Func(f) => check_one(f, None, solved, diags),
            Item::Impl(i) => {
                for m in &i.methods {
                    check_one(m, Some(&i.type_name), solved, diags);
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    check_one(m, Some(&s.name), solved, diags);
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        check_one(m, Some(&s.name), solved, diags);
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    check_one(m, Some(&e.name), solved, diags);
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
                        for (name, span) in list {
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
                        if ok {
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

pub(crate) fn name_defined(
    name: &str,
    funcs: &HashMap<String, FuncSig>,
    registry: &TypeRegistry,
    consts: &HashMap<String, Type>,
) -> bool {
    funcs.contains_key(name) || registry.contains(name) || consts.contains_key(name)
}

/// D-DIST1/D-DIST3: register a distinct type declaration.
pub(crate) fn register_distinct(
    d: &DistinctDef,
    registry: &mut TypeRegistry,
    diags: &mut Vec<Diagnostic>,
    funcs: &HashMap<String, FuncSig>,
    consts: &HashMap<String, Type>,
) {
    if is_reserved_type(&d.name) {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", d.name),
            format!("`{}` is provided by the language itself", d.name),
            "choose a different name for this distinct type".to_string(),
            Some(d.name_span),
        ));
        return;
    }
    if name_defined(&d.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &d.name,
            "every distinct type needs a unique name",
            d.name_span,
        ));
        return;
    }
    // E0129: base must be a concrete value type, not itself a distinct type.
    // We can only detect pre-registered distinct bases here; forward-declared
    // bases are checked lazily in sema (resolve_type / type check).
    if let Type::Named(base_name) = &d.base {
        if registry.is_distinct(base_name) {
            diags.push(Diagnostic::error(
                "E0129",
                format!(
                    "`{}` can't be built on `{}` — `{}` is itself a distinct type",
                    d.name, base_name, base_name
                ),
                format!(
                    "`distinct`-over-`distinct` chaining is not allowed in v1; `{}` is already a distinct type",
                    base_name
                ),
                format!("use `{}` directly, or build on the shared base type", base_name),
                Some(d.base_span),
            ));
            return;
        }
    }
    // D-RANGETYPE1: a range constraint (`distinct Int(0..10)`) only makes
    // sense on `Int` — reject it on any other base rather than silently
    // ignoring it.
    if let Some((lo, hi, range_span)) = &d.range {
        if d.base != Type::Int {
            diags.push(Diagnostic::error(
                "E0003",
                format!(
                    "a range constraint only works on `Int`, but `{}` is {}",
                    d.name,
                    d.base.show()
                ),
                "`distinct Base(lo..hi)` provably bounds a whole-number value".to_string(),
                format!("use `distinct Int({}..{})`, or drop the range", lo, hi),
                Some(*range_span),
            ));
        }
    }
    registry.types.insert(
        d.name.clone(),
        TypeDef::Distinct {
            name_span: d.name_span,
            base: d.base.clone(),
            is_numeric: d.is_numeric,
            is_comparable: d.is_comparable,
            is_printable: d.is_printable,
            is_codable_as_base: d.is_codable_as_base,
            range: d.range.map(|(lo, hi, _)| (lo, hi)),
        },
    );
}

/// D-TYPEALIAS1: register `alias Name<T> = …` — generic shortcuts only.
pub(crate) fn register_type_alias(
    a: &crate::AST::TypeAliasDef,
    registry: &mut TypeRegistry,
    diags: &mut Vec<Diagnostic>,
    funcs: &HashMap<String, FuncSig>,
    consts: &HashMap<String, Type>,
) {
    if is_reserved_type(&a.name) {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", a.name),
            format!("`{}` is provided by the language itself", a.name),
            "choose a different name for this type alias".to_string(),
            Some(a.name_span),
        ));
        return;
    }
    if name_defined(&a.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &a.name,
            "every type alias needs a unique name",
            a.name_span,
        ));
        return;
    }
    if a.type_params.is_empty() {
        diags.push(crate::Generics::e0324(a.name_span));
        return;
    }
    registry.types.insert(
        a.name.clone(),
        TypeDef::Alias {
            name_span: a.name_span,
            params: a.type_params.clone(),
            target: a.target.clone(),
        },
    );
}

/// S57 (M9.5): evaluate every `comptime NAME = expr;` in `items`. Purity and
/// fuel are enforced by the interpreter (E0951/E0952); panics surface as
/// E0953. Each result's Jet type is registered in `consts` so references
/// type-check, and the value is stashed on the item for codegen to inline.
pub(crate) fn eval_comptime_items(
    items: &mut [Item],
    consts: &mut HashMap<String, Type>,
    base_dir: &std::path::Path,
    diags: &mut Vec<Diagnostic>,
    // D-CTCORE1: module alias → Core path so the interpreter can evaluate
    // whitelisted pure Core calls (e.g. `comptime X = math.sqrt(4.0)`).
    core_imports: &HashMap<String, String>,
    mut embed_inputs_out: Option<&mut Vec<crate::AST::ComptimeInput>>,
) {
    if !items
        .iter()
        .any(|i| matches!(i, Item::Const(c) if c.is_comptime))
    {
        return;
    }
    let mut results: Vec<(String, crate::Comptime::CtValue)> = Vec::new();
    {
        let mut funcs: HashMap<String, &Func> = HashMap::new();
        let mut externs: HashSet<String> = HashSet::new();
        for item in items.iter() {
            match item {
                Item::Func(f) => {
                    funcs.insert(f.name.clone(), f);
                }
                Item::ExternRust(b) => {
                    for ef in &b.functions {
                        externs.insert(ef.name.clone());
                    }
                }
                _ => {}
            }
        }
        // Earlier comptime bindings are in scope for later ones.
        let mut globals: HashMap<String, crate::Comptime::CtValue> = HashMap::new();
        for item in items.iter() {
            if let Item::Const(c) = item {
                if c.is_comptime {
                    // D-CTCORE1: evaluate_with_imports so Core whitelist calls work.
                    match crate::Comptime::evaluate_with_imports_opts_collecting(
                        &c.value,
                        &funcs,
                        &externs,
                        base_dir,
                        &globals,
                        core_imports,
                        false,
                        0,
                    ) {
                        Ok((v, inputs)) => {
                            consts.insert(c.name.clone(), v.jet_type());
                            globals.insert(c.name.clone(), v.clone());
                            results.push((c.name.clone(), v));
                            if let Some(out) = embed_inputs_out.as_deref_mut() {
                                out.extend(inputs);
                            }
                        }
                        Err(d) => diags.push(d),
                    }
                }
            }
        }
    }
    for item in items.iter_mut() {
        if let Item::Const(c) = item {
            if c.is_comptime {
                if let Some(pos) = results.iter().position(|(n, _)| n == &c.name) {
                    c.ct = Some(results.remove(pos).1);
                }
            }
        }
    }
}

/// Card #131 / D-SERDE5: pre-evaluate every `#[Default(expr)]` argument on a
/// `@[Codable]`/`@[Encode]`/`@[Decode]` struct field to a compile-time value,
/// stashed on the marker (`Marker::ct`). Runs after `eval_comptime_items`, so a
/// default may reference a `comptime` const. Codegen serializes this value and
/// the comptime decode tier reuses it, so the two tiers bake the same default —
/// a non-primitive `#[Default(expr)]` never silently degrades to
/// `Default::default()` (R11/R12). A non-const argument is E2414.
pub(crate) fn eval_default_markers(
    items: &mut [Item],
    base_dir: &std::path::Path,
    diags: &mut Vec<Diagnostic>,
    core_imports: &HashMap<String, String>,
) {
    // Every struct that carries `#[Default(expr)]` needs the baked value —
    // Codable decode absents AND `@Cli` entry-arg absents read it (one
    // mechanism, I8); gating on Codable silently zeroed CLI defaults.
    let any = items.iter().any(|i| matches!(i, Item::Struct(s)
        if s.fields.iter().any(|f| f.serde_markers.iter().any(|m|
            m.name == crate::Syntax::ATTR_DEFAULT && !m.args.is_empty()))));
    if !any {
        return;
    }
    let (funcs_owned, externs, globals) = comptime_context_from_items(items);
    let funcs: HashMap<String, &Func> = funcs_owned.iter().map(|(k, v)| (k.clone(), v)).collect();
    for item in items.iter_mut() {
        let Item::Struct(s) = item else { continue };
        for f in &mut s.fields {
            let field_name = f.name.clone();
            for m in &mut f.serde_markers {
                if m.name != crate::Syntax::ATTR_DEFAULT {
                    continue;
                }
                let Some(expr) = m.args.first() else { continue };
                match crate::Comptime::evaluate_with_imports_opts_collecting(
                    expr,
                    &funcs,
                    &externs,
                    base_dir,
                    &globals,
                    core_imports,
                    false,
                    0,
                ) {
                    Ok((v, _)) => m.ct = Some(v),
                    Err(_) => diags.push(crate::Sema::e2414(&field_name, m.span)),
                }
            }
        }
    }
}

pub(crate) fn comptime_context_from_items(
    items: &[Item],
) -> (
    HashMap<String, Func>,
    HashSet<String>,
    HashMap<String, crate::Comptime::CtValue>,
) {
    let mut funcs = HashMap::new();
    let mut externs = HashSet::new();
    let mut globals = HashMap::new();
    for item in items {
        match item {
            Item::Func(f) => {
                funcs.insert(f.name.clone(), f.clone());
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    funcs.insert(m.name.clone(), m.clone());
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        funcs.insert(m.name.clone(), m.clone());
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    funcs.insert(m.name.clone(), m.clone());
                }
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    funcs.insert(m.name.clone(), m.clone());
                }
            }
            Item::Const(c) if c.is_comptime => {
                if let Some(v) = &c.ct {
                    globals.insert(c.name.clone(), v.clone());
                }
            }
            Item::ExternRust(b) => {
                for ef in &b.functions {
                    externs.insert(ef.name.clone());
                }
            }
            Item::Test(_)
            | Item::Bench(_)
            | Item::Const(_)
            | Item::Trait(_)
            | Item::Tag(_) // D-QUAL2: tags contribute no comptime context
            | Item::Module(_)
            | Item::CModule(_) | Item::CodeModule(_)
            | Item::Distinct(_)
            | Item::TypeAlias(_) // D-TYPEALIAS1: erases at codegen
            | Item::UnitFamily(_) // D-QUAL3: contributes no comptime context
            | Item::ErrorConv(_)
            | Item::Migration(_) // D-MIGRATE1
            | Item::StateDecl(_) // D-STATE-DECL
            | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2
            | Item::UserDerive(_) // D-METADERIVE1=A: expanded in Bundle.rs
            | Item::GenericModule(_) // D-GENMOD2=A: template — erases
            | Item::ModuleAlias(_) => {} // D-GENMOD2=A: alias — erases after expansion
        }
    }
    (funcs, externs, globals)
}

pub(crate) fn register_const(
    c: &crate::AST::ConstDef,
    consts: &mut HashMap<String, Type>,
    diags: &mut Vec<Diagnostic>,
    funcs: &HashMap<String, FuncSig>,
    registry: &TypeRegistry,
) {
    if name_defined(&c.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &c.name,
            "every const needs a unique name",
            c.name_span,
        ));
        return;
    }
    // S57 (M9.5): comptime bindings are evaluated by a dedicated pass
    // (`eval_comptime_items`), which registers their type from the result.
    if c.is_comptime {
        return;
    }
    let ty = match &c.value {
        Expr::Int(_, _, _) => Some(Type::Int),
        Expr::Float(_, _, _) => Some(Type::Float),
        Expr::Bool(_, _) => Some(Type::Bool),
        _ => None,
    };
    match ty {
        Some(t) => {
            consts.insert(c.name.clone(), t);
        }
        None => {
            diags.push(Diagnostic::error(
                "E0109",
                "a const holds a plain number or `true`/`false` for now".to_string(),
                "richer const values arrive with later milestones".to_string(),
                "give the const a number, like `const LIMIT = 10;`".to_string(),
                Some(c.value.span()),
            ));
        }
    }
}

pub(crate) fn register_struct(
    s: &StructDef,
    registry: &mut TypeRegistry,
    legacy: &mut HashMap<String, Vec<Type>>,
    diags: &mut Vec<Diagnostic>,
    funcs: &HashMap<String, FuncSig>,
    consts: &HashMap<String, Type>,
) {
    if is_reserved_type(&s.name) {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", s.name),
            format!("`{}` is provided by the language itself", s.name),
            "choose a different name for this struct".to_string(),
            Some(s.name_span),
        ));
        return;
    }
    if name_defined(&s.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &s.name,
            "every struct needs a unique name",
            s.name_span,
        ));
        return;
    }
    let mut field_names = HashSet::new();
    let mut fields = Vec::new();
    // D-FIELDPOL1: struct name → computed field name → (span, type). A
    // computed field is never a stored field — it's excluded from `fields`
    // entirely (so it's never required/allowed in a struct literal, E0339)
    // and resolved for reads through this side table instead.
    let mut computed_fields: HashMap<String, (Span, Type)> = HashMap::new();
    for f in &s.fields {
        if !field_names.insert(f.name.clone()) {
            diags.push(Diagnostic::error(
                "E0105",
                format!("field `{}` is defined twice in `{}`", f.name, s.name),
                "each field name may appear only once".to_string(),
                "rename or remove the duplicate field".to_string(),
                Some(f.name_span),
            ));
        }
        if f.computed.is_some() {
            computed_fields.insert(f.name.clone(), (f.name_span, f.ty.clone()));
        } else {
            fields.push((f.name.clone(), f.name_span, f.ty.clone(), f.is_pub));
        }
        if f.ty.is_float()
            && is_money_like_name(&f.name)
            && !allows_float_money(&f.serde_markers)
            && !allows_float_money(&s.serde_markers)
        {
            diags.push(Diagnostic::lint(
                "L0504",
                format!(
                    "field `{}` looks like money but has type `Float`",
                    f.name
                ),
                "floating-point money loses cents on common values like `0.1 + 0.2`".to_string(),
                "use `Decimal` for exact money, or suppress with `#[allow(float_money)]` on the field".to_string(),
                Some(f.name_span),
            ));
        }
    }
    registry.types.insert(
        s.name.clone(),
        TypeDef::Struct {
            name_span: s.name_span,
            fields,
            methods: HashMap::new(),
            single_use: s.is_single_use,
            must_use: s.is_must_use,
            columnar: s.layout == Some(crate::AST::StructLayout::Columnar),
            is_c_layout: s.layout == Some(crate::AST::StructLayout::C),
        },
    );
    if !computed_fields.is_empty() {
        registry
            .computed_fields
            .insert(s.name.clone(), computed_fields);
    }
    legacy.insert(
        s.name.clone(),
        s.fields.iter().map(|f| f.ty.clone()).collect(),
    );
    // D-REPRC1: `#layout(c)` structs may not contain growable fields.
    if s.layout == Some(crate::AST::StructLayout::C) {
        for f in &s.fields {
            let growable = matches!(&f.ty, Type::List(_) | Type::Map { .. } | Type::String);
            if growable {
                let layout_span = s.layout_span.unwrap_or(s.name_span);
                diags.push(Diagnostic::error(
                    "E1104",
                    format!(
                        "`#Layout(c)` struct `{}` has a growable field `{}` ({})",
                        s.name,
                        f.name,
                        f.ty.name()
                    ),
                    "growable types (`[T]`, `Map`, `String`) don't have a stable C layout"
                        .to_string(),
                    "use a fixed-size array `[T#N]` instead, or remove `#Layout(c)`".to_string(),
                    Some(layout_span),
                ));
            }
        }
    }
}

pub(crate) fn register_enum(
    e: &EnumDef,
    registry: &mut TypeRegistry,
    diags: &mut Vec<Diagnostic>,
    funcs: &HashMap<String, FuncSig>,
    consts: &HashMap<String, Type>,
) {
    if is_reserved_type(&e.name) {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", e.name),
            format!("`{}` is provided by the language itself", e.name),
            "choose a different name for this enum".to_string(),
            Some(e.name_span),
        ));
        return;
    }
    if name_defined(&e.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &e.name,
            "every enum needs a unique name",
            e.name_span,
        ));
        return;
    }
    let mut variants = HashMap::new();
    let mut variant_order = Vec::new();
    let mut seen = HashSet::new();
    // D-TAG1: leaf names are full dotted paths; the flattened Rust variant name
    // joins segments with `__`, so two distinct paths that mangle identically
    // (`Fire.Burn` vs `Fire__Burn`) must be rejected here, not by rustc (I2).
    let mut mangled = HashMap::new();
    for v in &e.variants {
        if !seen.insert(v.name.clone()) {
            diags.push(Diagnostic::error(
                "E0105",
                format!("variant `{}` is defined twice in `{}`", v.name, e.name),
                "each variant name may appear only once".to_string(),
                "rename or remove the duplicate variant".to_string(),
                Some(v.name_span),
            ));
            continue;
        }
        if let Some(other) = mangled.insert(v.name.replace('.', "__"), v.name.clone()) {
            diags.push(Diagnostic::error(
                "E0105",
                format!(
                    "variant `{}` collides with `{}` in `{}`",
                    v.name, other, e.name
                ),
                "a grouped path and an underscored name flatten to the same variant".to_string(),
                "rename one of the two variants".to_string(),
                Some(v.name_span),
            ));
            continue;
        }
        variant_order.push(v.name.clone());
        variants.insert(v.name.clone(), (v.name_span, v.payload.clone()));
    }
    // D-TAG1: record each group's subtree (ordered leaf paths). A group path
    // that also names a leaf is a duplicate definition (one name, two meanings).
    let mut groups: HashMap<String, (Span, Vec<String>)> = HashMap::new();
    for g in &e.groups {
        if variants.contains_key(&g.path) || groups.contains_key(&g.path) {
            diags.push(Diagnostic::error(
                "E0105",
                format!("variant `{}` is defined twice in `{}`", g.path, e.name),
                "a group name and a variant name share one namespace".to_string(),
                "rename or remove the duplicate variant".to_string(),
                Some(g.name_span),
            ));
            continue;
        }
        let prefix = format!("{}.", g.path);
        let leaves: Vec<String> = variant_order
            .iter()
            .filter(|v| v.starts_with(&prefix))
            .cloned()
            .collect();
        groups.insert(g.path.clone(), (g.name_span, leaves));
    }
    registry.types.insert(
        e.name.clone(),
        TypeDef::Enum {
            name_span: e.name_span,
            variants,
            variant_order,
            groups,
            methods: HashMap::new(),
            single_use: e.is_single_use,
            must_use: e.is_must_use,
        },
    );
}

pub(crate) fn register_type_methods(
    items: &[Item],
    registry: &mut TypeRegistry,
    diags: &mut Vec<Diagnostic>,
) {
    for item in items {
        let (type_name, methods, field_names) = match item {
            Item::Struct(s) => (s.name.as_str(), &s.methods, registry.field_names(&s.name)),
            Item::Enum(e) => (e.name.as_str(), &e.methods, Vec::new()),
            _ => continue,
        };
        let Some(type_def) = registry.types.get_mut(type_name) else {
            continue;
        };
        let methods_map = match type_def {
            TypeDef::Struct { methods, .. } | TypeDef::Enum { methods, .. } => methods,
            TypeDef::Distinct { .. } | TypeDef::Alias { .. } => continue,
        };
        for m in methods {
            if field_names.iter().any(|f| f == &m.name) {
                diags.push(method_field_clash(&m.name, type_name, m.name_span));
            }
            if methods_map.contains_key(&m.name) {
                let is_ctor = m.self_param().is_none();
                diags.push(method_defined_twice(
                    &m.name,
                    type_name,
                    m.name_span,
                    is_ctor,
                ));
            } else {
                // L2401 (D-NARG1): pub method with a positional Bool param.
                if m.is_pub {
                    for p in m.params.iter().filter(|p| p.name != "self") {
                        if matches!(p.ty, Type::Bool) && p.default.is_none() {
                            diags.push(Diagnostic::lint(
                                "L2401",
                                format!(
                                    "public method `{}` has a positional `Bool` parameter `{}`",
                                    m.name, p.name
                                ),
                                "positional booleans are easy to transpose at the call site"
                                    .to_string(),
                                format!(
                                    "callers can write `{}: true` to make the intent clear (S61 labels)",
                                    p.name
                                ),
                                Some(p.name_span),
                            ));
                        }
                    }
                }
                // D-NARG-D2 (E0126): check defaults don't ref later params.
                let non_self: Vec<_> = m
                    .params
                    .iter()
                    .filter(|p| p.name != "self")
                    .cloned()
                    .collect();
                check_default_forward_refs(&non_self, &m.name, diags);
                methods_map.insert(m.name.clone(), func_to_method_sig(m));
            }
        }
    }
}

pub(crate) fn register_impl_methods(
    items: &[Item],
    registry: &mut TypeRegistry,
    diags: &mut Vec<Diagnostic>,
) {
    for item in items {
        let Item::Impl(i) = item else { continue };
        if !registry.contains(&i.type_name) {
            continue;
        }
        let field_names = registry.field_names(&i.type_name);
        let Some(type_def) = registry.types.get_mut(&i.type_name) else {
            continue;
        };
        let methods_map = match type_def {
            TypeDef::Struct { methods, .. } | TypeDef::Enum { methods, .. } => methods,
            TypeDef::Distinct { .. } | TypeDef::Alias { .. } => continue,
        };
        for m in &i.methods {
            if field_names.iter().any(|f| f == &m.name) {
                diags.push(method_field_clash(&m.name, &i.type_name, m.name_span));
            }
            if methods_map.contains_key(&m.name) {
                let is_ctor = m.self_param().is_none();
                diags.push(method_defined_twice(
                    &m.name,
                    &i.type_name,
                    m.name_span,
                    is_ctor,
                ));
            } else {
                // L2401 (D-NARG1): pub method with a positional Bool param.
                if m.is_pub {
                    for p in m.params.iter().filter(|p| p.name != "self") {
                        if matches!(p.ty, Type::Bool) && p.default.is_none() {
                            diags.push(Diagnostic::lint(
                                "L2401",
                                format!(
                                    "public method `{}` has a positional `Bool` parameter `{}`",
                                    m.name, p.name
                                ),
                                "positional booleans are easy to transpose at the call site"
                                    .to_string(),
                                format!(
                                    "callers can write `{}: true` to make the intent clear (S61 labels)",
                                    p.name
                                ),
                                Some(p.name_span),
                            ));
                        }
                    }
                }
                // D-NARG-D2 (E0126): check defaults don't ref later params.
                let non_self: Vec<_> = m
                    .params
                    .iter()
                    .filter(|p| p.name != "self")
                    .cloned()
                    .collect();
                check_default_forward_refs(&non_self, &m.name, diags);
                methods_map.insert(m.name.clone(), func_to_method_sig(m));
            }
        }
    }
}

pub(crate) fn check_func_body(
    f: &mut Func,
    funcs: &HashMap<String, FuncSig>,
    registry: &TypeRegistry,
    structs: &HashMap<String, Vec<Type>>,
    consts: &HashMap<String, Type>,
    trait_reg: &TraitRegistry,
    owner_type: Option<&str>,
    ct_funcs: &HashMap<String, Func>,
    ct_externs: &HashSet<String>,
    ct_base_dir: &std::path::Path,
    ct_globals: &HashMap<String, crate::Comptime::CtValue>,
    freestanding: bool,
    summaries: &mut HashMap<String, EffectSummary>,
    global_addr_taken: &mut HashSet<String>,
    // D-MEM1/S7 (D-NOALLOC-SEM1=A): this module's `policy no_alloc` state.
    no_alloc: bool,
    // D-PRELUDEX1=A: this file's `#NoPrelude` state.
    no_prelude: bool,
) -> Vec<Diagnostic> {
    let empty_imports = HashMap::new();
    let empty_core_imports = HashMap::new();
    let empty_code_modules = HashMap::new();
    let empty_unqualified: HashMap<String, String> = HashMap::new();
    let empty_unqualified_file: HashMap<String, (String, usize)> = HashMap::new();
    let empty_func_pub: HashMap<String, bool> = HashMap::new();
    let empty_func_pkg_pub: HashMap<String, bool> = HashMap::new();
    let mut reference_anchors = HashMap::new();
    let mut ck = Checker {
        funcs,
        registry,
        structs,
        consts,
        modules: None,
        module_idx: 0,
        imports: &empty_imports,
        core_imports: &empty_core_imports,
        code_modules: &empty_code_modules,
        unqualified: &empty_unqualified,
        unqualified_file: &empty_unqualified_file,
        func_pub: &empty_func_pub,
        func_pkg_pub: &empty_func_pkg_pub,
        module_path: "<memory>",
        reference_anchors: &mut reference_anchors,
        diags: Vec::new(),
        scopes: vec![HashMap::new()],
        moved: HashMap::new(),
        loop_depth: 0,
        loop_labels: Vec::new(),
        fx_direct: std::collections::BTreeSet::new(),
        fx_edges: std::collections::BTreeSet::new(),
        fx_maximal: false,
        region_stack: Vec::new(),
        fx_regions: Vec::new(),
        fx_callback_obligations: Vec::new(),
        txn_depth: 0,
        det_suppress: 0,
        context_depth: 0,
        context_allocator_active: false,
        // S58 (E2-M13): an `#Unsafe fn` body is itself an audited region — its
        // statements may use low-level ops directly without a nested `#Unsafe`
        // block. Calling such a fn is gated separately (E3103).
        in_unsafe: f.is_unsafe,
        suppress_must_use: false,
        in_pure: f.is_pure,
        no_alloc,
        no_prelude,
        in_pre_clause: false,
        in_comptime: false,
        ret: f.return_type.clone(),
        fn_name: f.name.clone(),
        expected_type: None,
        iter_borrowed: HashSet::new(),
        freed_allocators: HashMap::new(),
        arena_views: HashMap::new(),
        list_views: HashMap::new(),
        string_views: HashMap::new(),
        uninit: HashMap::new(),
        borrow_ctx: false,
        allow_string_view_read: false,
        lambda_escapes: true,
        is_task_spawn: false,
        lambda_param_mutable: false,
        view_capture_tasks: HashSet::new(),
        view_borrow_escape_tasks: HashSet::new(),
        current_binding_name: None,
        lambda_binding: None,
        lambda_mut_borrow_stack: vec![HashSet::new()],
        trait_reg,
        ct_funcs,
        ct_externs,
        ct_base_dir,
        ct_globals,
        ct_scopes: vec![HashMap::new()],
        type_param_scope: f.type_params.clone(),
        freestanding,
        allow_impure: false,
        ct_impure_depth: 0,
        ct_embed_inputs: Vec::new(),
        in_dropped_comptime_arm: false,
        stmt_tail_ptr: std::ptr::null(),
        stmt_tail_len: 0,
        liveness_frames: Vec::new(),
        taskgroup_stack: Vec::new(),
        in_taskgroup_spawn: false,
        inline_addr_taken: HashSet::new(),
    };
    if let Some(meta) = &f.meta {
        ck.check_meta_attr(meta);
    }
    ck.check_params_and_body(f, owner_type);
    // S60 (E2-M16): purity enforcement for `pure fn` bodies.
    if f.is_pure {
        ck.diags.extend(check_pure_fn(f, funcs));
    }
    // D-METHODMACRO1=A: the local half of the `@InlineAlways` check (self-
    // recursion E0917 + size ceiling E0919); roll this function's
    // address-taken names into the whole-program accumulator so the E0918
    // pass after the full registration loop can see them.
    if f.is_inline_always {
        ck.diags.extend(check_inline_always_fn(f));
    }
    global_addr_taken.extend(std::mem::take(&mut ck.inline_addr_taken));
    // D-EFF1: record this function's effect summary for the whole-program fixpoint.
    // D-PROP1=A: seed direct with declared positives so solve() propagates the
    // contract of functions whose bodies don't yet exercise the declared effect.
    let mut direct = std::mem::take(&mut ck.fx_direct);
    if let Some(declared_list) = &f.declared_effects {
        for (name, _) in declared_list {
            if !name.starts_with('!') {
                if let Some(e) = parse_effect_name(name.as_str()) {
                    direct.insert(e);
                }
            }
        }
    }
    summaries.insert(
        effect_key(owner_type, &f.name),
        EffectSummary {
            direct,
            edges: std::mem::take(&mut ck.fx_edges),
            maximal: ck.fx_maximal,
            regions: std::mem::take(&mut ck.fx_regions),
            callback_obligations: std::mem::take(&mut ck.fx_callback_obligations),
        },
    );
    ck.diags
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

/// D-ERR-CONV: type-check an `impl Source -> Target { body }` conversion body.
/// `self` is bound to the source error type; the block must return the target type.
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_error_conv_body(
    ec: &mut crate::AST::ErrorConvDef,
    funcs: &HashMap<String, FuncSig>,
    registry: &TypeRegistry,
    structs: &HashMap<String, Vec<Type>>,
    consts: &HashMap<String, Type>,
    trait_reg: &TraitRegistry,
    ct_funcs: &HashMap<String, Func>,
    ct_externs: &HashSet<String>,
    ct_base_dir: &std::path::Path,
    ct_globals: &HashMap<String, crate::Comptime::CtValue>,
    // D-MEM1/S7 (D-NOALLOC-SEM1=A): this module's `policy no_alloc` state.
    no_alloc: bool,
    // D-PRELUDEX1=A: this file's `#NoPrelude` state.
    no_prelude: bool,
) -> Vec<Diagnostic> {
    // Synthesise a pseudo-function to reuse check_func_body.
    let mut synthetic = Func {
        span: ec.body_span,
        is_pub: false,
        is_package_pub: false,
        external_type: None,
        name: format!(
            "__errconv_{}_to_{}",
            ec.from_ty.replace('.', "_"),
            ec.to_ty.replace('.', "_")
        ),
        name_span: ec.from_span,
        meta: None,
                    type_params: Vec::new(),
        params: vec![Param {
            name: crate::Syntax::KW_SELF.to_string(),
            name_span: ec.from_span,
            ty: Type::Named(String::new()), // sema fills self type from owner_type
            ty_span: ec.from_span,
            convention: AccessConvention::Move,
            default: None,
            variadic: false,
            variadic_bound_list: None,
        }],
        return_type: Some(Type::Named(ec.to_ty.clone())),
        return_type_span: Some(ec.to_span),
        is_unsafe: false,
        unsafe_reason: None,
        unsafe_span: None,
        is_pure: false,
        is_reactive: false,
        is_replayable: false,
        replayable_span: None,
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
        body: std::mem::take(&mut ec.body),
    };
    let d = check_func_body(
        &mut synthetic,
        funcs,
        registry,
        structs,
        consts,
        trait_reg,
        Some(&ec.from_ty),
        ct_funcs,
        ct_externs,
        ct_base_dir,
        ct_globals,
        false,
        &mut HashMap::new(),
        &mut HashSet::new(),
        no_alloc,
        no_prelude,
    );
    ec.body = synthetic.body;
    d
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
        is_unsafe: false,
        unsafe_reason: None,
        unsafe_span: None,
        is_pure: false,
        is_reactive: false,
        is_replayable: false,
        replayable_span: None,
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
        is_unsafe: false,
        unsafe_reason: None,
        unsafe_span: None,
        is_pure: false,
        is_reactive: false,
        is_replayable: false,
        replayable_span: None,
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
