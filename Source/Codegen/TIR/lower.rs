//! TIR lowering: AST -> TIR (`LowerEnv`, `lower_*`, render helpers).
//!
//! Split out of the original `TIR.rs` for maintainability; behavior unchanged.

use super::*;
use crate::AST::{
    AccessConvention, BinOp, BindPattern, ElseBranch, EnumLitArg, Expr, ForKind, Func, IfStmt,
    IndexKind, Lambda, LambdaBody, LValue, OrFallback, Param, PatSlot, Pattern, Stmt, StrPart,
    SwitchArm, TryConvert, Type, UnOp, VariantPayload,
};
use crate::Diagnostics::Span;
use crate::Syntax;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Per-function lowering environment: a local name -> (Rust place string, type).
/// Built from params, extended by `let` bindings. The "place" already accounts
/// for parameter deref, so `Local` emission needs no further resolution.
///
/// The type is `Option<Type>`: a binding can carry a *resolved* type, or `None`
/// when the AST path's slot had `jet_ty: None` and we must reproduce that
/// partiality. The load-bearing case (c109 Phase 5) is a `loop x in coll`
/// iteration variable: `emit_for_in` binds its slot with `jet_ty: None`, so
/// `operand_is_integer`/`expr_jet_ty` resolve the var to `None` and it never
/// enables the overflow trap. Carrying `Some(elem_ty)` here would diverge —
/// `x + 1` would wrongly trap. So the iteration var is stored as `None`,
/// matching the AST path bit-for-bit (the Phase-3 "reproduce the AST's
/// partiality where it is load-bearing" lesson, again).
pub(crate) struct LowerEnv {
    locals: HashMap<String, (String, Option<Type>)>,
    /// c109 Phase 8: the enclosing function's unmangled Jet name, used by a `?`
    /// (`TExprKind::Try`) to embed the trace-frame function name — exactly the value
    /// the AST path reads from `cx.current_fn` at emit time (set to `f.name`).
    fn_name: String,
    /// c109 Phase 15: the `safe_locals_expr` env replica for the `a ?? panic(…)` form.
    /// `safe_locals_expr` (Source/Codegen/Statement.rs) dumps the FULL codegen `env`
    /// (`HashMap<String, Slot>`), filtered to scalar Int/Float/Bool slots, sorted by
    /// name, at the panic site. The AST codegen `env` LEAKS: a `let` inside a plain
    /// block / loop / mixed-or-range switch arm / comptime-if branch stays in the
    /// shared `&mut env` after the block (sema scopes the *name* so it is never read,
    /// but `safe_locals_expr` dumps the raw env regardless). Only the two
    /// `emit_pattern_match_switch` arm-body boundaries and lambda bodies clone the env
    /// (no leak). To reproduce the dump byte-exact this replica is shared (`Rc<RefCell>`)
    /// across leaky branches via `clone_env`, and DEEP-COPIED via `fork_panic` at the
    /// non-leaky boundaries. It is updated in lock-step with `locals` through `bind`.
    panic_locals: Rc<RefCell<HashMap<String, (String, Option<Type>)>>>,
    /// c109 Phase 17: the enclosing function returns `-> view T` (a borrow). When set,
    /// a `return <e>` lowers via the view-return shape (`emit_view_return`): an `Ident`
    /// becomes `&name`/`name` (deref) / `&<const>`, a field read `&(<place>)`, anything
    /// else a plain expr — resolved at lowering into a `TStmt::ViewReturn`. The AST path
    /// reads this off `view_return` threaded through `emit_stmts`; the TIR carries it on
    /// the env so the `Return` lowering reproduces it byte-for-byte.
    view_return: bool,
}

impl LowerEnv {
    /// A fresh root env for a function/method body (an empty `panic_locals` replica).
    fn new(fn_name: String) -> LowerEnv {
        LowerEnv {
            locals: HashMap::new(),
            fn_name,
            panic_locals: Rc::new(RefCell::new(HashMap::new())),
            view_return: false,
        }
    }
    /// Bind `name` to its resolved Rust place + type, updating BOTH `locals` (used for
    /// place/type resolution) and the `panic_locals` replica (used only for the `??`
    /// panic locals dump). Every covered binding site routes through here so the two
    /// stay in lock-step.
    fn bind(&mut self, name: &str, place: String, ty: Option<Type>) {
        self.locals.insert(name.to_string(), (place.clone(), ty.clone()));
        self.panic_locals
            .borrow_mut()
            .insert(name.to_string(), (place, ty));
    }
    fn place_of(&self, name: &str) -> String {
        match self.locals.get(name) {
            Some((place, _)) => place.clone(),
            None => mangle(name),
        }
    }
    fn ty_of(&self, name: &str) -> Option<Type> {
        self.locals.get(name).and_then(|(_, t)| t.clone())
    }
    /// c109 Phase 4: a name reads as a borrow when its resolved place is a deref
    /// (`(*name)`) — a by-reference parameter slot. The match lowering clones such
    /// a subject so the `match` owns the value, mirroring `emit_pattern_match_switch`.
    fn is_borrowed(&self, name: &str) -> bool {
        matches!(self.locals.get(name), Some((place, _)) if place.starts_with("(*"))
    }
    /// The bare Rust binding name (without the deref wrapper), e.g. `user_light`
    /// for a slot whose place is `(*user_light)`. Used by the match-subject clone,
    /// which clones the borrow itself (`(user_light).clone()`), not `(*user_light)`.
    fn rust_name_of(&self, name: &str) -> String {
        match self.locals.get(name) {
            Some((place, _)) if place.starts_with("(*") && place.ends_with(')') => {
                place[2..place.len() - 1].to_string()
            }
            Some((place, _)) => place.clone(),
            None => mangle(name),
        }
    }
}

/// c109 Phase 17: lower a `return <e>` from a `-> view T` function, reproducing
/// `emit_view_return` (Source/Codegen/Statement.rs) byte-for-byte. The view-return
/// subset only admits an `Ident` (a parameter/const borrowed back) or a `Field` read
/// (`field.name` — a borrow into a field of an owned root); sema's E2301/E2304 reject
/// index/slice and a non-owning local, so those never reach here.
///  - an `Ident` resolving to a deref'd slot (`(*name)`) returns the BARE borrow `name`
///    (the deref stripped) — `ViewWrap::Bare` over a `Local(rust_name)`;
///  - an `Ident` resolving to a non-deref slot returns `&name` — `ViewWrap::Addr`;
///  - an `Ident` that is a const returns `&<const>` — `ViewWrap::Addr` over the inlined
///    const value (the same `Local` path the AST takes via `cx.consts`);
///  - an `Ident` not in scope returns `place_of(name)` with no `&` — `ViewWrap::Bare`;
///  - a `Field` read returns `&(<place>)` — `ViewWrap::Addr` over the lowered field;
///  - anything else passes straight to `emit_expr` — `ViewWrap::Bare`.
pub(crate) fn lower_view_return(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    match e {
        Expr::Ident(name, _) => {
            // A comptime const inlines at the use site (the AST reads `cx.consts`); take
            // its address. Lower as a normal expr (which inlines the const) wrapped in `&`.
            if cx.consts.contains_key(name) {
                return TStmt::ViewReturn {
                    value: lower_expr(e, cx, env),
                    wrap: ViewWrap::Addr,
                };
            }
            match env.locals.get(name) {
                Some((place, ty)) if place.starts_with("(*") && place.ends_with(')') => {
                    // Deref'd (by-reference) slot: return the bare borrow `name`.
                    let bare = place[2..place.len() - 1].to_string();
                    TStmt::ViewReturn {
                        value: TExpr {
                            ty: ty.clone().unwrap_or(Type::Int),
                            kind: TExprKind::Local(bare),
                        },
                        wrap: ViewWrap::Bare,
                    }
                }
                Some(_) => TStmt::ViewReturn {
                    value: lower_expr(e, cx, env),
                    wrap: ViewWrap::Addr,
                },
                // Not in env: `place_of` returns the mangled name with no `&` (Bare).
                None => TStmt::ViewReturn {
                    value: lower_expr(e, cx, env),
                    wrap: ViewWrap::Bare,
                },
            }
        }
        Expr::Field(..) => TStmt::ViewReturn {
            value: lower_expr(e, cx, env),
            wrap: ViewWrap::Addr,
        },
        _ => TStmt::ViewReturn {
            value: lower_expr(e, cx, env),
            wrap: ViewWrap::Bare,
        },
    }
}

/// D-TXN-ROLLBACK layer 1: collect the root local names that are *assigned* anywhere
/// in a `#Transact` body — `x = …`, `x += …`, `x.f = …`, `x[i] = …` — so each can be
/// auto-snapshotted at block entry and restored on a `?`-failure. Recurses through
/// nested control flow (if/while/for/switch/loop/region/etc.) but stops at:
///   • nested `#Transact` blocks — they establish their own rollback scope; and
///   • lambda bodies — a deferred execution context (the same reason `on_commit`
///     lambdas escape the enclosing transaction's effect check).
/// Each root is recorded once, in first-seen order. v1 covers assignment targets,
/// the clearly-analyzable, fully-correct case; mutation reached *only* through a
/// `~self` method call (no assignment) or a deep alias is the documented deferred
/// corner (D-TXN-ROLLBACK). This is a syntactic over-approximation filtered by the
/// caller to roots in scope at block entry.
fn collect_txn_mut_roots(body: &[Stmt], out: &mut Vec<String>) {
    fn push(out: &mut Vec<String>, name: &str) {
        if !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    /// The root local ident of an assignable place, if any.
    fn lvalue_root(lv: &LValue) -> Option<&str> {
        match lv {
            LValue::Local { name, .. } => Some(name),
            LValue::Index { base, .. } | LValue::Field { base, .. } => expr_root(base),
        }
    }
    fn expr_root(e: &Expr) -> Option<&str> {
        match e {
            Expr::Ident(name, _) => Some(name),
            Expr::Field(base, _, _) => expr_root(base),
            Expr::Index { base, .. } => expr_root(base),
            _ => None,
        }
    }
    for s in body {
        match s {
            Stmt::Assign { target, .. } => {
                if let Some(root) = lvalue_root(target) {
                    push(out, root);
                }
            }
            Stmt::If(ifs) => walk_if(ifs, out),
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::ContextBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::AssumeDet { body, .. } => collect_txn_mut_roots(body, out),
            Stmt::Switch { arms, else_body, .. } => {
                for arm in arms {
                    collect_txn_mut_roots(&arm.body, out);
                }
                if let Some(eb) = else_body {
                    collect_txn_mut_roots(eb, out);
                }
            }
            Stmt::ComptimeIf { then_body, else_body, .. } => {
                collect_txn_mut_roots(then_body, out);
                if let Some(eb) = else_body {
                    collect_txn_mut_roots(eb, out);
                }
            }
            // A nested `#Transact` owns its own rollback scope — don't pull its
            // mutations up into the enclosing block.
            Stmt::Transact { .. } => {}
            // Other statements (Expr/Val/Return/Break/…) introduce no assignment
            // targets we snapshot at block entry. (A `~self` mutating method call
            // hides inside `Stmt::Expr` — the documented deferred corner.)
            _ => {}
        }
    }
    fn walk_if(ifs: &crate::AST::IfStmt, out: &mut Vec<String>) {
        collect_txn_mut_roots(&ifs.then_body, out);
        match &ifs.else_branch {
            Some(crate::AST::ElseBranch::ElseIf(inner)) => walk_if(inner, out),
            Some(crate::AST::ElseBranch::Else(body)) => collect_txn_mut_roots(body, out),
            None => {}
        }
    }
}

/// D-COV1: 1-based line number of a byte offset in the source, for coverage probes.
pub(crate) fn cov_line(cx: &Cx, offset: usize) -> usize {
    cx.src[..offset.min(cx.src.len())].bytes().filter(|&b| b == b'\n').count() + 1
}

pub(crate) fn lower_func(f: &Func, cx: &Cx) -> TFunc {
    let mut env = LowerEnv::new(f.name.clone());
    env.view_return = f.is_view_return;
    // Mirror emit_func's parameter slot construction: a non-scalar `Read` param
    // (String, Char) is a borrow in Rust and reads as `(*name)`.
    let mut params = Vec::new();
    for p in &f.params {
        let rust_name = cx.mangle_name(&p.name);
        // c109 Phase 17: a param TYPED as a bare type parameter (`item: T`) is forced to
        // the `Move` convention for the slot deref (it is passed by value — `rust_param_type`
        // renders it `T`, no `&`), EXACTLY as `emit_func` forces `conv = Move` for an
        // `is_type_param` param. A param typed `Stack<T>` is NOT a type-var param — it keeps
        // its source convention (`Read` → `&user_Stack<T>`, deref'd place `(*user_s)`).
        let place = param_place_generic(&rust_name, p, &f.type_params);
        env.bind(&p.name, place, Some(p.ty.clone()));
        params.push((rust_name, p.ty.clone(), p.convention));
    }
    let body = lower_stmts(&f.body, cx, &mut env);
    TFunc {
        name: f.name.clone(),
        params,
        ret: f.return_type.clone(),
        is_view: f.is_view_return,
        generics: render_generics(&f.type_params),
        is_main: f.name == "main",
        line: cov_line(cx, f.name_span.start),
        is_unsafe: f.is_unsafe,
        body,
        kind: TFuncKind::TopLevel,
    }
}

/// c109: lower + emit a `#Test` block body through the TIR, reproducing the legacy
/// `emit_stmts(cx, body, &mut env, out, 1, false)` byte-for-byte. The body is a bare
/// statement list with no params and an empty env, emitted at indent 1 inside the
/// `fn jet_test_N() -> Result<(), String>` the caller already opened. The env's
/// `fn_name` is taken LIVE from `cx.current_fn` — exactly the value the legacy `?`/panic
/// emitters read (`emit_*_tests` never resets `cx.current_fn` before the test loop, so
/// both paths embed the same trailing function name in any `?`/panic frame).
pub(crate) fn emit_tir_test_body(body: &[Stmt], cx: &Cx, out: &mut String) {
    let mut env = LowerEnv::new(cx.current_fn.borrow().clone());
    let tbody = lower_stmts(body, cx, &mut env);
    emit_tir_stmts(&tbody, cx, out, 1);
}

/// D-TEST1: lower + emit a property-test body. Identical to `emit_tir_test_body`
/// except each property parameter is bound into the env first (by its mangled
/// name, by value) so references inside the body resolve to the generated input.
/// The caller emits `fn jet_prop_N(p0: T0, …) -> Result<(), String>` so the
/// param names are real Rust locals; this binds them in the lowering env.
pub(crate) fn emit_tir_property_test_body(body: &[Stmt], params: &[Param], cx: &Cx, out: &mut String) {
    let mut env = LowerEnv::new(cx.current_fn.borrow().clone());
    for p in params {
        let rust_name = mangle(&p.name);
        env.bind(&p.name, rust_name, Some(p.ty.clone()));
    }
    let tbody = lower_stmts(body, cx, &mut env);
    emit_tir_stmts(&tbody, cx, out, 1);
}

/// c109: lower + emit an error-conversion `impl Old -> New { … }` body through the TIR,
/// reproducing `emit_error_conv`'s `emit_stmts(cx, body, &mut env, out, 1, false)`
/// byte-for-byte. `emit_error_conv` already emitted the signature + opening brace and set
/// `cx.current_fn` to the conversion fn name; it binds `self` to `user_self` (Move, the
/// Old named type — Slot `{rust_name:"user_self", deref:false}`), so the env's `self`
/// place is the bare `user_self`. The body's `return <e>` lowers the expr as-is (sema
/// already inserted any wrapping); emitted at indent 1, the closing brace is the caller's.
pub(crate) fn emit_tir_error_conv_body(
    body: &[Stmt],
    from_ty: &str,
    cx: &Cx,
    out: &mut String,
) {
    let mut env = LowerEnv::new(cx.current_fn.borrow().clone());
    env.bind(
        Syntax::KW_SELF,
        "user_self".to_string(),
        Some(Type::Named(from_ty.to_string())),
    );
    let tbody = lower_stmts(body, cx, &mut env);
    emit_tir_stmts(&tbody, cx, out, 1);
}

/// c109 Phase 17: render the Rust generic clause exactly as `emit_func` does — every type
/// param carries an extra `Clone` bound (`rust_extra_clone_bounds`), so `<T>` → `<T: Clone>`
/// and `<T: Comparable>` → `<T: PartialOrd + Clone>`. Empty for a non-generic function.
pub(crate) fn render_generics(type_params: &[crate::AST::TypeParam]) -> String {
    if type_params.is_empty() {
        return String::new();
    }
    let extra = crate::Generics::rust_extra_clone_bounds(type_params);
    crate::Generics::rust_type_param_list(type_params, &extra)
}

/// c109 Phase 17: `param_place` for a (possibly generic) free function. A param whose type
/// is a bare type-parameter name (`Type::Named(T)` where `T` is one of `type_params`) is
/// forced to `Move` for the deref decision (it is by-value), mirroring `emit_func`'s
/// `is_type_param` branch; any other param uses `param_place`'s convention-based deref.
pub(crate) fn param_place_generic(rust_name: &str, p: &Param, type_params: &[crate::AST::TypeParam]) -> String {
    let is_type_param = type_params
        .iter()
        .any(|tp| matches!(&p.ty, Type::Named(n) if n == &tp.name));
    if is_type_param {
        // Forced `Move` → no deref (by-value), exactly `emit_func`.
        rust_name.to_string()
    } else {
        param_place(rust_name, p)
    }
}

/// c109 Phase 7: lower an inherent method (instance or static) of `type_name` to a
/// `TFunc`. Mirrors `emit_method`'s slot construction exactly:
///  - the `self` parameter (if any) becomes a slot whose place is the bare `self`
///    (rust_name `self`, NO deref — `self.field` reads emit `(self).field`, and a
///    `when self` match scrutinee emits `self` with no clone, exactly as the AST
///    path does for a `&self`/`&mut self`/`self` receiver) and whose type is `None`
///    (matching `emit_method`'s `jet_ty: None` so overflow decisions are identical);
///  - non-self params get the same `param_place` deref logic as a free function.
/// The `self_conv` (instance) / `None` (static) and the resolved return type drive
/// the receiver/signature in `emit_tir_func`.
pub(crate) fn lower_method(f: &Func, type_name: &str, cx: &Cx) -> TFunc {
    let mut env = LowerEnv::new(f.name.clone());
    env.view_return = f.is_view_return;
    let mut params = Vec::new();
    let mut self_conv: Option<AccessConvention> = None;
    let mut is_static = true;
    for p in &f.params {
        if p.name == Syntax::KW_SELF {
            // The self slot, parity with `emit_method`: place `self`, type None. A
            // `mut self` receiver is `&mut Self`, so its place DEREFS (`(*self)`) —
            // `self.field = v` → `((*self)).field = v`, whole-`self` `self = New{}` →
            // `(*self) = New{}` (D-MUTSELF1). `self`/`take self` carry no deref.
            let place = if matches!(p.convention, AccessConvention::Write) {
                "(*self)".to_string()
            } else {
                "self".to_string()
            };
            env.bind(Syntax::KW_SELF, place, None);
            self_conv = Some(p.convention);
            is_static = false;
            continue;
        }
        let rust_name = mangle(&p.name);
        let place = param_place(&rust_name, p);
        // A `Self`-typed param resolves to the owning type for totality.
        let pty = resolve_self_ty(&p.ty, type_name);
        env.bind(&p.name, place, Some(pty.clone()));
        params.push((rust_name, pty, p.convention));
    }
    let body = lower_stmts(&f.body, cx, &mut env);
    // An instance method carries `Some(conv)`; a static method carries `None`.
    let kind = TFuncKind::Method {
        self_conv: if is_static { None } else { self_conv },
    };
    TFunc {
        name: f.name.clone(),
        params,
        ret: f.return_type.as_ref().map(|t| resolve_self_ty(t, type_name)),
        is_view: f.is_view_return,
        // A method's generic params live on the enclosing `impl<T> user_<T>` block (the
        // caller opened it); `emit_method` renders no per-method clause.
        generics: String::new(),
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        is_unsafe: f.is_unsafe,
        body,
        kind,
    }
}

/// c109 Phase 12: lower a TRAIT-IMPL method of `type_name` to a `TFunc`. Mirrors
/// `emit_trait_method`'s slot construction (Source/Codegen/Items.rs) EXACTLY — which
/// differs from `emit_method`:
///  - the `self` slot's type is `Some(Type::Named(type_name))` (NOT `None` as in
///    `emit_method`); place `self`, no deref. This is load-bearing for overflow-trap
///    decisions that consult the self slot — though in the covered subset `self` is a
///    struct/enum (never a bare arithmetic operand), so the decision never differs.
///  - non-self params use the same deref logic, but `emit_trait_method` has no
///    `Read if scalar` short-circuit branch — it computes `deref = !p.ty.is_scalar()`
///    for `Read`, which is identical to `param_place` for `Read` (scalar → false).
/// The `TraitMethod` kind drives a bare name, no `pub`, always-`&self` signature.
pub(crate) fn lower_trait_method(f: &Func, type_name: &str, cx: &Cx) -> TFunc {
    let mut env = LowerEnv::new(f.name.clone());
    env.view_return = f.is_view_return;
    let mut params = Vec::new();
    let mut self_conv = AccessConvention::Read;
    for p in &f.params {
        if p.name == Syntax::KW_SELF {
            self_conv = p.convention;
            // The self slot, EXACTLY `emit_trait_method`'s: type `Some(Named(type_name))`
            // (NOT `None` like `emit_method`). D-MUTSELF1: a `mut self` receiver is
            // `&mut self`, so its place DEREFS (`(*self)`); `self`/`take self` do not.
            let place = if matches!(p.convention, AccessConvention::Write) {
                "(*self)".to_string()
            } else {
                "self".to_string()
            };
            env.bind(
                Syntax::KW_SELF,
                place,
                Some(Type::Named(type_name.to_string())),
            );
            continue;
        }
        let rust_name = cx.mangle_name(&p.name);
        let place = param_place(&rust_name, p);
        let pty = resolve_self_ty(&p.ty, type_name);
        env.bind(&p.name, place, Some(pty.clone()));
        params.push((rust_name, pty, p.convention));
    }
    let body = lower_stmts(&f.body, cx, &mut env);
    TFunc {
        name: f.name.clone(),
        params,
        ret: f.return_type.as_ref().map(|t| resolve_self_ty(t, type_name)),
        is_view: f.is_view_return,
        generics: String::new(),
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        // The trait-method `unsafe` prefix rides on `TFuncKind::TraitMethod.is_unsafe`
        // (the dedicated trait-method emit reads it there); the top-level flag is unused
        // for this kind, but keep it consistent.
        is_unsafe: f.is_unsafe,
        body,
        kind: TFuncKind::TraitMethod {
            is_unsafe: f.is_unsafe,
            self_conv,
        },
    }
}

/// c109 Phase 15: is a DELEGATION trait method (`using field`) coverable? Always — the
/// method is purely structural: a fixed forwarding call `(self).<field>.<method>(args)`
/// with the bare trait method name, and a signature rendered by the SAME
/// `rust_param_type`/`rust_return_type` the AST path uses. There is no body to lower, no
/// type to re-infer; the forward + signature are deterministic. (The `field`/method/
/// args come straight off the `ImplDef`; nothing here can produce code rustc rejects
/// that the AST path wouldn't.) Returns `true` for any delegation method.
pub(crate) fn tir_covers_delegation_method(_f: &Func, _field: &str, _cx: &Cx) -> bool {
    true
}

/// c109 Phase 15: lower a delegation trait method to a `TFunc` with a `Delegation` kind,
/// reproducing `emit_delegation_method` (Source/Codegen/Items.rs) byte-for-byte: the
/// signature line (incl. its quirky two-space `  {`), and the forwarding call. There is
/// no body — the method only forwards to the delegated field with the BARE trait method
/// name (no `user_` mangle, as the trait owns it in Rust).
pub(crate) fn lower_delegation_method(f: &Func, field: &str, cx: &Cx) -> TFunc {
    let ret = f
        .return_type
        .as_ref()
        .map(|t| rust_return_type(cx, t, f.is_view_return))
        .unwrap_or_default();
    let ret_clause = if ret.is_empty() {
        String::new()
    } else {
        format!(" -> {}", ret)
    };
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            if p.name == Syntax::KW_SELF {
                "&self".to_string()
            } else {
                format!("{}: {}", mangle(&p.name), rust_param_type(cx, p.convention, &p.ty))
            }
        })
        .collect();
    // The signature line, EXACTLY `emit_delegation_method`'s format (note the two spaces
    // before `{` and the ` {ret}` only when there is a return).
    let sig = format!(
        "    fn {}({}){}  {{\n",
        f.name,
        params.join(", "),
        if ret_clause.is_empty() {
            String::new()
        } else {
            format!(" {}", ret_clause.trim())
        }
    );
    let fwd_args: Vec<String> = f
        .params
        .iter()
        .filter(|p| p.name != Syntax::KW_SELF)
        .map(|p| mangle(&p.name).to_string())
        .collect();
    let field_rust = mangle(field);
    let fwd = format!("(self).{}.{}({})", field_rust, f.name, fwd_args.join(", "));
    TFunc {
        name: f.name.clone(),
        params: Vec::new(),
        ret: f.return_type.clone(),
        // The signature is fully pre-rendered (`sig`); `is_view`/`generics` are unused for delegation.
        is_view: f.is_view_return,
        generics: String::new(),
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        // A delegation method has no body and never carries `#Unsafe fn` (sema rejects it).
        is_unsafe: false,
        body: Vec::new(),
        kind: TFuncKind::Delegation {
            sig,
            fwd,
            has_return: f.return_type.is_some(),
        },
    }
}

/// The Rust place a parameter reads as, mirroring `emit_func`'s `deref` logic:
/// a `Read` parameter of non-scalar type (String/Char) is a `&T` and must be
/// dereferenced; `Mutate` is `&mut T` (deref'd); `Move`/scalar-`Read` is by value.
pub(crate) fn param_place(rust_name: &str, p: &Param) -> String {
    let deref = match p.convention {
        // D-CAP8/9: Infer/Share/Raw follow Read until their phases specialize them.
        AccessConvention::Read
        | AccessConvention::Infer
        | AccessConvention::Share
        | AccessConvention::Raw
            if p.ty.is_scalar() =>
        {
            false
        }
        AccessConvention::Read
        | AccessConvention::Infer
        | AccessConvention::Share
        | AccessConvention::Raw => true,
        AccessConvention::Write => true,
        AccessConvention::Move => false,
    };
    if deref {
        format!("(*{})", rust_name)
    } else {
        rust_name.to_string()
    }
}

pub(crate) fn lower_stmts(stmts: &[Stmt], cx: &Cx, env: &mut LowerEnv) -> Vec<TStmt> {
    stmts.iter().map(|s| lower_stmt(s, cx, env)).collect()
}

pub(crate) fn lower_stmt(s: &Stmt, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    match s {
        Stmt::Val(b) if matches!(&b.pattern, Some(BindPattern::Struct { .. })) => {
            // c109: a struct-destructuring binding `Type { x, y } @= <init>`. Lower the
            // init ONCE; its total `.ty` is a `Type::Named`/`Apply` naming a struct
            // (sema guarantees it). The per-field type comes from `cx.struct_fields`,
            // reproducing `emit_stmt`'s `BindPattern::Struct` arm. Each field binds with
            // its resolved type and a non-deref'd slot (the clone owns the value); the
            // pattern's field name is BOTH the bound local and the `.field` read.
            let Some(BindPattern::Struct { fields, span, .. }) = &b.pattern else {
                unreachable!("guard matched a struct pattern")
            };
            let init = lower_expr(&b.init, cx, env);
            let field_tys: HashMap<String, Type> = match &init.ty {
                Type::Named(n) | Type::Apply { name: n, .. } => cx
                    .struct_fields
                    .get(n)
                    .map(|fs| fs.iter().cloned().collect())
                    .unwrap_or_default(),
                _ => HashMap::new(),
            };
            let tmp = format!("__jet_d{}", span.start);
            let kw = if b.mutable { "let mut" } else { "let" };
            let mut field_names = Vec::new();
            for f in fields {
                let m = mangle(&f.name).to_string();
                field_names.push(m.clone());
                env.bind(&f.name, m, field_tys.get(&f.name).cloned());
            }
            return TStmt::StructDestructure {
                tmp,
                init,
                kw,
                fields: field_names,
            };
        }
        Stmt::Val(b) if matches!(&b.pattern, Some(BindPattern::Tuple { .. })) => {
            // c109 Phase 23: a tuple-destructuring binding `(a, b) @= <init>`. Lower the
            // init ONCE; its total `.ty` is a `Type::Tuple` (sema guarantees it). Pair the
            // pattern elements to the tuple's CANONICAL fields by position, reproducing
            // `emit_stmt`'s `BindPattern::Tuple` arm. Each element binds with its resolved
            // field type and a non-deref'd slot (the clone owns the value).
            let Some(BindPattern::Tuple { elems, span }) = &b.pattern else {
                unreachable!("guard matched a tuple pattern")
            };
            let init = lower_expr(&b.init, cx, env);
            let canonical: Vec<(String, Type)> = match &init.ty {
                Type::Tuple(fs) => fs.iter().map(|(n, t)| (n.clone(), (**t).clone())).collect(),
                _ => Vec::new(),
            };
            let tmp = format!("__jet_d{}", span.start);
            let kw = if b.mutable { "let mut" } else { "let" };
            let mut binds = Vec::new();
            for (e, (fname, fty)) in elems.iter().zip(canonical.iter()) {
                let elem_rust = mangle(&e.name).to_string();
                let field_rust = mangle(fname).to_string();
                binds.push((elem_rust.clone(), field_rust));
                env.bind(&e.name, elem_rust, Some(fty.clone()));
            }
            return TStmt::TupleDestructure {
                tmp,
                init,
                kw,
                binds,
            };
        }
        Stmt::Val(b) if matches!(&b.pattern, Some(BindPattern::List { .. })) => {
            // c109 Phase 26: a list-destructuring binding `[a, b, c] @= <init>`. Lower
            // the init ONCE, then bind each element via `jet_unpack_vec(tmp, want, i,
            // file, line)`, reproducing `emit_stmt`'s `BindPattern::List` arm. The
            // element slot type reproduces `expr_jet_ty(init)`'s `Some(List(inner))`-only
            // match: the LOWERED init's `.ty` is exactly what `expr_jet_ty(&b.init)`
            // resolves (an Ident → its slot type), so a non-`List` init (e.g. a `[T#N]`
            // fan-out result) yields a `None` element type — byte-identical partiality.
            let Some(BindPattern::List { elems, span }) = &b.pattern else {
                unreachable!("guard matched a list pattern")
            };
            let init = lower_expr(&b.init, cx, env);
            let elem_ty = match &init.ty {
                Type::List(inner) => Some((**inner).clone()),
                _ => None,
            };
            let tmp = format!("__jet_d{}", span.start);
            let kw = if b.mutable { "let mut" } else { "let" };
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            let mut elem_names = Vec::new();
            for e in elems {
                let m = mangle(&e.name).to_string();
                elem_names.push(m.clone());
                env.bind(&e.name, m, elem_ty.clone());
            }
            return TStmt::ListDestructure {
                tmp,
                init,
                kw,
                want: elems.len(),
                file: cx.file.clone(),
                line,
                elems: elem_names,
            };
        }
        Stmt::Val(b) => {
            // D-UNINIT1 (ratified 2026-06-21, opt C): lower `#Uninit name: T` to
            //   `let mut name: T = unsafe { std::mem::MaybeUninit::<T>::uninit().assume_init() };`
            // The source's `use core.mem` + `#Uninit` is the expert-tier opt-in (I1: no
            // `unsafe` in generated code without a source-level gate). Sema proved
            // write-before-read (E0420), so every subsequent read is post-write — the
            // `assume_init()` at declaration yields garbage bytes that are always
            // overwritten before any read. The `is_pod_uninit_type` guard in sema
            // (E0423) ensures T has no Drop glue, so no destructor ever reads the garbage.
            if b.uninit {
                let ty = b.ty.as_ref().expect("E0421 ensures #Uninit binding has a type");
                let rust_ty = cx.rust_type(ty);
                let init_str = format!(
                    "unsafe {{ std::mem::MaybeUninit::<{}>::uninit().assume_init() }}",
                    rust_ty
                );
                env.bind(&b.name, mangle(&b.name), b.ty.clone());
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let mut",
                    ty_clause: format!(": {}", rust_ty),
                    init: TExpr {
                        ty: ty.clone(),
                        kind: TExprKind::ConstInline(init_str),
                    },
                };
            }
            // c109 Phase 19: an arena `view` binding (`x @= arena.alloc(v)`). The AST
            // `emit_let`'s `arena_view` branch emits `let <x> = <init>;` (NO type clause,
            // NEVER `let mut` — a view is a non-reassignable `&mut T`) and binds a DEREF'd
            // slot (reads go through `(*x)`). Reproduce it exactly: a `Let` with `kw: "let"`,
            // empty `ty_clause`, and a deref'd slot place `(*<x>)`.
            if b.arena_view {
                let init = lower_expr(&b.init, cx, env);
                env.bind(&b.name, format!("(*{})", mangle(&b.name)), b.ty.clone());
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    ty_clause: String::new(),
                    init,
                };
            }
            // c109 (S57/M9.5): a comptime LOCAL `comptime NAME = expr`. The AST `emit_let`
            // builds `init` from `b.ct.serialize()` (the sema-evaluated value rendered to a
            // Rust literal) — the runtime `init` expr is never emitted. Reproduce it: a
            // verbatim `ConstInline` of the same serialized string, with `kw: "let"` (the
            // `(b.mutable && !b.is_comptime)` guard makes it `let`, never `let mut`) and the
            // type clause from `b.ty` (rendered exactly as the non-comptime path below). All
            // facts are pre-resolved (I3): no inference here.
            if b.is_comptime {
                let serialized = b
                    .ct
                    .as_ref()
                    .map(|v| v.serialize())
                    .unwrap_or_else(|| "Default::default()".to_string());
                // Mirror `emit_let`'s type clause exactly (a Fn type via `rust_fn_trait`,
                // others via `rust_type`). A comptime value is never fn-typed, but match
                // the AST shape verbatim for total byte-parity.
                let ty_clause = b
                    .ty
                    .as_ref()
                    .map(|t| {
                        if let Type::Fn { params, ret, .. } = t {
                            format!(": {}", cx.rust_fn_trait(params, ret.as_deref(), false))
                        } else {
                            format!(": {}", cx.rust_type(t))
                        }
                    })
                    .unwrap_or_default();
                let init = TExpr {
                    ty: b.ty.clone().unwrap_or(Type::Int),
                    kind: TExprKind::ConstInline(serialized),
                };
                env.bind(&b.name, mangle(&b.name), b.ty.clone());
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    ty_clause,
                    init,
                };
            }
            let mut init = lower_expr(&b.init, cx, env);
            // D-FIXARR1: if the binding annotation is `[T#N]` and the init lowered as a
            // growable list (e.g. a plain list literal), re-tag the TExpr type so the emit
            // produces a Rust array literal `[e1, …]` instead of `vec![…]`.
            if let Some(fl @ Type::FixedList { .. }) = &b.ty {
                if matches!(init.ty, Type::List(_)) && matches!(init.kind, TExprKind::ListLit(_)) {
                    init.ty = fl.clone();
                }
            }
            // D-SOA1: an EMPTY list literal `[]` for a declared columnar `[S]` lowers
            // with an Int placeholder element type (no element to infer from), so it
            // came through as a plain `ListLit([])`/`vec![]`. Rewrite it to the
            // columnar empty constructor `user_<S>_columns::from_aos(vec![])` using
            // the binding's declared type.
            if let Some(decl @ Type::List(inner)) = &b.ty {
                if let Some(columns_ty) = cx.columnar_list_type(inner) {
                    if matches!(&init.kind, TExprKind::ListLit(es) if es.is_empty()) {
                        init = TExpr {
                            ty: decl.clone(),
                            kind: TExprKind::ColumnarListLit { columns_ty, elems: Vec::new() },
                        };
                    }
                }
            }
            // c109 Phase 13: reproduce `emit_let`'s `mut_fn` form — an escaping FnMut
            // lambda binding gets `let mut` AND an `as <fn-trait(mut)>` init coercion +
            // a `: <fn-trait(mut)>` annotation. Decided here from `Lambda.meta`.
            let mut_fn = matches!(
                &b.init,
                Expr::Lambda(l) if l.meta.escapes && l.meta.needs_fn_mut
            );
            if mut_fn {
                if let Some(Type::Fn { params, ret, .. }) = &b.ty {
                    let coerced = format!(
                        "{} as {}",
                        emit_tir_expr(&init, cx),
                        cx.rust_fn_trait(params, ret.as_deref(), true)
                    );
                    init = TExpr {
                        ty: init.ty.clone(),
                        kind: TExprKind::FnValue {
                            kind: TFnValueKind::NamedFn { wrapper: coerced },
                        },
                    };
                }
            }
            // Totality: if the source omitted the type, infer it ONCE here from
            // the init's already-resolved type. Codegen never infers.
            let ty = b.ty.clone().unwrap_or_else(|| init.ty.clone());
            // E2-M7/E2-M10/D-ALLOC1/D-ROUTE1: a handle binding forces `let mut` even
            // when bound immutably (its methods take `&mut self`). Mirror
            // `emit_let`'s `is_file_handle` set exactly.
            let is_file_handle = matches!(
                &b.ty,
                Some(Type::Named(n)) if n == "FileReader" || n == "FileWriter"
                    || n == "TcpStream" || n == "HttpRouter"
                    || n == "Arena" || n == "Bump" || n == "Pool" || n == "Fixed"
            );
            let kw = if (b.mutable && !b.is_comptime) || mut_fn || is_file_handle {
                "let mut"
            } else {
                "let"
            };
            // The type annotation clause, rendered exactly as `emit_let`: a Fn type via
            // `rust_fn_trait(params, ret, mut_fn)`, others via `rust_type`. Empty for an
            // inferred binding.
            let ty_clause = b
                .ty
                .as_ref()
                .map(|t| {
                    if let Type::Fn { params, ret, .. } = t {
                        format!(": {}", cx.rust_fn_trait(params, ret.as_deref(), mut_fn))
                    } else {
                        format!(": {}", cx.rust_type(t))
                    }
                })
                .unwrap_or_default();
            env.bind(&b.name, mangle(&b.name), Some(ty));
            TStmt::Let {
                name: b.name.clone(),
                kw,
                ty_clause,
                init,
            }
        }
        Stmt::Assign {
            target, op, value, ..
        } => match target {
            LValue::Local { name, .. } => {
                // c150: mirror the lower_enum_arg clone predicate — a borrowed non-scalar
                // ident on the RHS would move out of a shared reference (E0507, I2).
                let clone_value = if let Expr::Ident(vname, _) = value {
                    env.is_borrowed(vname)
                        && env.ty_of(vname).is_some_and(|t| !t.is_scalar())
                } else {
                    false
                };
                TStmt::Assign {
                    place: env.place_of(name),
                    op: *op,
                    value: lower_expr(value, cx, env),
                    clone_value,
                }
            }
            // c109 Phase 5: `coll[i] = v`. The `IndexKind` is resolved by sema; carry
            // it as the total `is_map` fact (the gate excluded `Unknown`). No compound
            // op on an index lvalue (parser admits only `=`).
            LValue::Index { base, index, kind, .. } => {
                let base_t = lower_expr(base, cx, env);
                let index_t = lower_expr(index, cx, env);
                let value_t = lower_expr(value, cx, env);
                TStmt::IndexAssign {
                    base: base_t,
                    index: index_t,
                    is_map: matches!(kind, IndexKind::Map),
                    value: value_t,
                }
            }
            // D-MUTSELF1: a field-assignment `place.field [op]= v`. The place is the
            // field READ lowered to its resolved Rust string (`((*self)).field` once
            // the `mut self` slot derefs), reusing the same `Expr::Field` lowering the
            // read path uses — byte-for-byte the AST `LValue::Field` form. Carried as a
            // plain `TStmt::Assign` so the `op` compound form rides the shared emit.
            LValue::Field { base, field, span } => {
                let field_expr = Expr::Field(base.clone(), field.clone(), *span);
                let place = emit_tir_expr(&lower_expr(&field_expr, cx, env), cx);
                // c150: mirror the lower_enum_arg clone predicate — a borrowed non-scalar
                // ident on the RHS would move out of a shared reference (E0507, I2).
                let clone_value = if let Expr::Ident(vname, _) = value {
                    env.is_borrowed(vname)
                        && env.ty_of(vname).is_some_and(|t| !t.is_scalar())
                } else {
                    false
                };
                TStmt::Assign {
                    place,
                    op: *op,
                    value: lower_expr(value, cx, env),
                    clone_value,
                }
            }
        },
        Stmt::Return(Some(e), _) if env.view_return => lower_view_return(e, cx, env),
        Stmt::Return(Some(e), _) => TStmt::Return(Some(lower_expr(e, cx, env))),
        Stmt::Return(None, _) => TStmt::Return(None),
        Stmt::Expr(e) => TStmt::ExprStmt(lower_expr(e, cx, env)),
        Stmt::If(ifs) => lower_if(ifs, cx, env),
        // c109 Phase 2: control-flow loops. Loop bodies are their own scope —
        // lower on a cloned env so bindings inside don't leak out.
        Stmt::Loop { body, label, .. } => {
            let mut branch = clone_env(env);
            TStmt::Loop {
                label: label_name(label),
                body: lower_stmts(body, cx, &mut branch),
            }
        }
        Stmt::While { cond, body, label, .. } => {
            let cond = lower_expr(cond, cx, env);
            let mut branch = clone_env(env);
            TStmt::While {
                label: label_name(label),
                cond,
                body: lower_stmts(body, cx, &mut branch),
            }
        }
        Stmt::For { var, var2, kind, body, label, .. } => match kind {
            ForKind::Range { start, end, step } => {
                let start = lower_expr(start, cx, env);
                let end = lower_expr(end, cx, env);
                let step = step.as_ref().map(|s| lower_expr(s, cx, env));
                // The loop var is an `Int` local for the body's scope only. The AST
                // (`Statement.rs`) inserts it into the shared env, emits the body, then
                // RESTORES the prior binding — so a scalar `??` panic dump INSIDE the body
                // sees the var, but one after the loop does not. Reproduce that exactly:
                // bind it on the shared `panic_locals`, lower the body, then restore.
                let mut branch = clone_env(env);
                let prev = branch.panic_locals.borrow().get(var).cloned();
                branch.bind(var, mangle(var), Some(Type::Int));
                let lowered_body = lower_stmts(body, cx, &mut branch);
                match prev {
                    Some(p) => {
                        branch.panic_locals.borrow_mut().insert(var.clone(), p);
                    }
                    None => {
                        branch.panic_locals.borrow_mut().remove(var);
                    }
                }
                TStmt::Range {
                    label: label_name(label),
                    var: var.clone(),
                    start,
                    end,
                    step,
                    body: lowered_body,
                }
            }
            // c109 Phase 5: collection iteration `loop x in coll` / `loop k, v in map`.
            // The collection string is resolved once. The loop var(s) bind in the body
            // scope with an *unresolved* type (`None`) — matching the AST slot's
            // `jet_ty: None`, so they never enable the overflow trap (parity).
            ForKind::In { collection } => {
                // c109 Phase 22: classify a method-call collection into the matching
                // `emit_for_in` branch (`chars`/`lines`/the `.iter().cloned()` default),
                // resolving the receiver/collection string off the SAME node shape the
                // AST path reads. `method_kind == None` is the plain `.iter()` form.
                let (collection_str, method_kind) = lower_forin_collection(collection, cx, env);
                // Infer the element type from the lowered collection so the loop
                // variable binds with its concrete type. This lets `core_struct_field_rust_name`
                // emit plain field names (not `user_<field>`) for core types like DirEntry.
                let coll_elem_ty: Option<Type> = {
                    let lowered_coll = lower_expr(collection, cx, env);
                    match &lowered_coll.ty {
                        Type::List(inner) => Some((**inner).clone()),
                        Type::FixedList { elem, .. } => Some((**elem).clone()),
                        // Map iteration: key type for single-binding form.
                        Type::Map { key, .. } => Some((**key).clone()),
                        _ => None,
                    }
                };
                let mut branch = clone_env(env);
                branch
                    .locals
                    .insert(var.clone(), (mangle(var), coll_elem_ty.clone()));
                if let Some((v2, _)) = var2 {
                    // Two-binding map form: v2 gets the value type.
                    let v2_ty = match &coll_elem_ty {
                        _ => None, // map value type is not tracked here; keep None for v2
                    };
                    branch
                        .locals
                        .insert(v2.clone(), (mangle(v2), v2_ty));
                }
                // D-SOA1: a single-binding loop over a columnar list iterates the
                // gathered AoS view (`iter_aos`), not `Vec::iter` (which the columns
                // type doesn't expose).
                let columnar = var2.is_none()
                    && method_kind.is_none()
                    && coll_elem_ty
                        .as_ref()
                        .map(|t| cx.columnar_list_type(t).is_some())
                        .unwrap_or(false);
                TStmt::ForIn {
                    label: label_name(label),
                    var: var.clone(),
                    var2: var2.as_ref().map(|(n, _)| n.clone()),
                    collection_str,
                    method_kind,
                    columnar,
                    body: lower_stmts(body, cx, &mut branch),
                }
            }
        },
        Stmt::Break(_) => TStmt::Break(None),
        Stmt::Continue(_) => TStmt::Continue(None),
        Stmt::BreakLabel(name, _) => TStmt::Break(Some(name.clone())),
        Stmt::ContinueLabel(name, _) => TStmt::Continue(Some(name.clone())),
        // c109 Phase 4: a `when`/match. The gate already classified it as either an
        // exhaustive enum match (shape A) or an all-range scalar switch (shape B).
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => lower_switch(subject, arms, else_body, cx, env),
        // c109 Phase 15: a resolved comptime-if (`Stmt::ComptimeIf`). Sema chose the
        // branch (`selected_then`); the AST `emit_stmts` emits ONLY that branch's
        // statements INLINE on the SAME `&mut env` at the SAME indent (no `if`, no
        // block — its `let`s leak into the outer scope). Reproduce both: lower the
        // selected branch's statements on the SAME `env` (so their bindings leak, like
        // the AST shared env) and wrap them in a flat `Inline` node.
        Stmt::ComptimeIf {
            then_body,
            else_body,
            selected_then,
            ..
        } => {
            let chosen: &[Stmt] = match selected_then {
                Some(true) => then_body,
                Some(false) => else_body.as_deref().unwrap_or(&[]),
                // Sema didn't resolve (earlier error) — emit nothing (I3), like the AST.
                None => &[],
            };
            TStmt::Inline(lower_stmts(chosen, cx, env))
        }
        // c109 Phase 18: an audited `#Unsafe { … }` region (`Stmt::Unsafe`). The AST
        // `emit_stmts` emits `unsafe { … }` and lowers the body on the SAME `&mut env`
        // (the body's `let`s leak into the outer scope). Reproduce: lower the body on the
        // SAME `env` (so bindings leak) and wrap in `TStmt::Unsafe`. The `#Audit("…")`
        // annotation is dropped (codegen is dumb — it emits nothing, matching the AST).
        // I1: the source `#Unsafe` gate is 1:1 with this node, the only producer of a
        // Rust `unsafe` block.
        Stmt::Unsafe { body, .. } => TStmt::Unsafe(lower_stmts(body, cx, env)),
        // c109 Phase 19: an explicit `region r { … }` (D-REGION1). The AST emits a plain
        // block and lowers the body on the SAME `&mut env` (its `let`s leak into the outer
        // scope). Reproduce: lower the body on the SAME `env`, wrap in `TStmt::Region`.
        Stmt::Region { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // c109 Phase 26: a `#Caps(Io) { … }` effect-restriction region (D-EFF1). `emit_stmt`'s
        // `Stmt::Caps` arm is byte-for-byte `Stmt::Region` — a plain block with the body lowered
        // on the SAME `&mut env` (its `let`s leak). Effects erase at codegen (I3); reuse the
        // `TStmt::Region` shape.
        Stmt::Caps { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // D-SCAP1: a `#grant(Fs) { caps -> … }` grant region. The capability handle
        // is a compile-time-only fact (authority to perform the granted effects),
        // erased here (I3); the body lowers on the SAME `&mut env` (its `let`s leak)
        // into a plain `TStmt::Region` — byte-for-byte the `Stmt::Region`/`Stmt::Caps`
        // shape. No runtime grant/revoke value, no `unsafe`.
        Stmt::Grant { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // c109 Phase 19: a `#Context(field: value) { … }` block (D-CTX1). Resolve each
        // field into an `(is_allocator, value)` guard at lowering, then lower the body on
        // the SAME `env` (it leaks like a region). Emit reproduces `emit_stmts`'s
        // `Stmt::ContextBlock` arm byte-for-byte.
        Stmt::ContextBlock { fields, body, .. } => {
            let guards = fields
                .iter()
                .map(|(name, v, _)| {
                    let is_alloc = name == Syntax::CTX_FIELD_ALLOCATOR;
                    (is_alloc, lower_expr(v, cx, env))
                })
                .collect();
            TStmt::ContextBlock {
                guards,
                body: lower_stmts(body, cx, env),
            }
        }
        // D-TERM1 (ratified 2026-06-22): `live { … }` block. The body leaks into the
        // enclosing `env` (same as `Stmt::Region`) so let-bindings inside are visible
        // after the block (consistent with all other Jet lexical blocks). Lowering only
        // records the body; the enter/guard/leave preamble is emitted in `emit_tir_stmt`.
        Stmt::Live { body, .. } => TStmt::Live {
            body: lower_stmts(body, cx, env),
        },
        // D-DET1: `assume_deterministic { … }` erases to a plain `TStmt::Region`
        // (byte-for-byte the `Stmt::Region`/`Stmt::Caps` shape). The determinism
        // suspension is a sema-only fact; nothing runtime, no `unsafe` (I3).
        Stmt::AssumeDet { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` block. Bind the
        // handle (typed `Transaction`) in `env` so `name.on_commit(…)` lowers against
        // it, then lower the body on the SAME `env` (it leaks like a region). The
        // `let mut <handle> = jet_transaction(); … <handle>.commit();` framing is
        // emitted in `emit_tir_stmt`; codegen is dumb (I3).
        Stmt::Transact { name, body, .. } => {
            let handle = name.as_ref().map(|name| {
                let h = mangle(name);
                env.bind(
                    name,
                    h.clone(),
                    Some(Type::Named(Syntax::TXN_HANDLE_TYPE.to_string())),
                );
                h
            });
            // D-TXN-ROLLBACK layer 1 (auto-snapshot): collect the root local names
            // assigned anywhere in the block (recursing into nested control flow, but
            // NOT into nested `#Transact` blocks or lambda bodies — those own their
            // own rollback scope / are deferred). Snapshot only roots ALREADY in scope
            // at block entry (params / outer locals): a local declared inside the block
            // needs no snapshot, since rollback discards it when the block scope ends.
            // Each becomes `&mut <place>` so the prelude can clone+restore it.
            let mut roots: Vec<String> = Vec::new();
            collect_txn_mut_roots(body, &mut roots);
            let snapshots: Vec<(String, Option<String>)> = roots
                .iter()
                .filter(|r| env.locals.contains_key(*r))
                .map(|r| {
                    let place_ref = format!("&mut {}", env.place_of(r));
                    // D-TXN-ROLLBACK layer 2: if the root type implements Rollback,
                    // use snapshot_custom instead of the clone-based snapshot path.
                    let rollback_ty = env.ty_of(r).and_then(|ty| {
                        if let crate::AST::Type::Named(n) = ty {
                            if cx.rollback_types.contains(&n) {
                                return Some(format!("user_{n}"));
                            }
                        }
                        None
                    });
                    (place_ref, rollback_ty)
                })
                .collect();
            TStmt::Transact {
                handle,
                snapshots,
                body: lower_stmts(body, cx, env),
            }
        }
        // Forward-safety default: a Stmt variant not in the subset never reaches
        // lowering (`stmt_in_subset` returns false for it). Kept as a guard against a
        // future variant; currently unreachable because every covered variant is matched.
        #[allow(unreachable_patterns)]
        _ => unreachable!("statement not in TIR subset"),
    }
}

/// Pull the bare label name out of an `@name` loop label, dropping the span. The
/// emitter renders it as `'jet_<name>:` (mirroring `loop_label_prefix`).
pub(crate) fn label_name(label: &Option<(String, Span)>) -> Option<String> {
    label.as_ref().map(|(n, _)| n.clone())
}

/// c109 Phase 22: resolve a `loop x in <coll>` collection into its emitted Rust
/// string + (for a method-call collection) the iteration form, reproducing
/// `emit_for_in`'s branch selection (Source/Codegen/Statement.rs) byte-for-byte.
/// For `chars`/`lines` the returned string is the *receiver* (the form emits
/// `({recv}).chars()` / `BufRead::lines(&mut ({recv}).inner)`); for the plain form
/// (incl. a non-special method call routed to `.iter().cloned()`) it is the whole
/// collection. The FileReader-vs-stdin `lines` split mirrors the AST's
/// `expr_jet_ty(receiver)` / inline-`io.stdin()` test exactly.
pub(crate) fn lower_forin_collection(
    collection: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
) -> (String, Option<TForInMethod>) {
    if let Expr::MethodCall {
        receiver, method, ..
    } = collection
    {
        match method.as_str() {
            "chars" => {
                let recv = emit_tir_expr(&lower_expr(receiver, cx, env), cx);
                return (recv, Some(TForInMethod::Chars));
            }
            "lines" => {
                // FileReader streaming vs stdin streaming — the AST tests
                // `expr_jet_ty(receiver)` (reproduced by `tir_recv_jet_ty`) for the
                // FileReader case, then a `StdinHandle` type OR an inline `io.stdin()`
                // receiver for the stdin case. Checked in the SAME order as
                // `emit_for_in` (FileReader first).
                let recv = emit_tir_expr(&lower_expr(receiver, cx, env), cx);
                if matches!(tir_recv_jet_ty(receiver, env), Some(Type::Named(n)) if n == "FileReader")
                {
                    return (recv, Some(TForInMethod::LinesFile));
                }
                // stdin: a `StdinHandle`-typed receiver OR an inline `io.stdin()` call.
                let is_stdin = matches!(tir_recv_jet_ty(receiver, env), Some(Type::Named(n)) if n == "StdinHandle")
                    || matches!(receiver.as_ref(), Expr::MethodCall { method: m, .. } if m == "stdin");
                if is_stdin {
                    return (recv, Some(TForInMethod::LinesStdin));
                }
                // A `.lines()` on neither (unreachable in valid Jet — sema E2502
                // restricts `.lines()` to a FileReader/StdinHandle loop position) would
                // fall to the AST `else` default; reproduce that for totality.
                let coll = emit_tir_expr(&lower_expr(collection, cx, env), cx);
                (coll, None)
            }
            _ => {
                // The `.iter().cloned()` default: emit the WHOLE method call as the
                // collection value (e.g. a `.split(…)` builtin returning a `[String]`).
                let coll = emit_tir_expr(&lower_expr(collection, cx, env), cx);
                (coll, None)
            }
        }
    } else {
        let coll = emit_tir_expr(&lower_expr(collection, cx, env), cx);
        (coll, None)
    }
}

/// c109 Phase 22: lower an `if` condition into a `TIfCond`, plus the optional
/// then-branch binding the condition introduces (name, rust place, resolved type).
/// Reproduces `emit_if`'s condition handling (Source/Codegen/Statement.rs):
///  - `x == null` (`Pattern::Absent`) → `IsNone` (no binding);
///  - `value(b)`/`ok(b)`/`err(b)` → `IfLet` with the Rust pattern from
///    `emit_if_let_pattern`, the binding's type resolved off the subject's lowered
///    `Option`/`Result` (mirroring `add_pattern_bindings`);
///  - anything else → `Plain`.
pub(crate) fn lower_if_cond(
    cond: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
) -> (TIfCond, Option<(String, String, Option<Type>)>, Vec<TStmt>) {
    if let Expr::PatternTest {
        subject,
        pattern: Pattern::Absent(_),
        ..
    } = cond
    {
        let subj = lower_expr(subject, cx, env);
        return (TIfCond::IsNone { subj }, None, Vec::new());
    }
    // D-ENC-DYN1=A+: a dynamic `Data` variant if-let (`if data == Object(entries)` /
    // `if n == Int(v)`). The Rust if-let pattern is `{root}jet_std::DataTree::<Variant>(…)`;
    // the binding's type comes from `core_json_pattern_types`. Scalars/`Array` bind their
    // inner field directly. `Object` is special: `DataTree::Object` is ordered
    // `Vec<(String, DataTree)>`, but the user-facing payload is a `Map<String, Data>`, so
    // the pattern binds the pairs to a temp and a then-body prefix `let` collects them into
    // a `BTreeMap` (the value the body sees).
    if let Expr::PatternTest {
        subject,
        pattern: pattern @ Pattern::Variant { variant, bindings, span: pat_span },
        ..
    } = cond
    {
        if is_json_variant(variant) {
            if let Some(PatSlot::Bind(name)) = bindings.first() {
                let subj = lower_expr(subject, cx, env);
                let ty = crate::Sema::core_json_pattern_types(variant)
                    .and_then(|ts| ts.into_iter().next());
                let place = mangle(name);
                if variant == "Object" {
                    let obj_tmp = format!("__jet_obj{}", pat_span.start);
                    let pat_str = format!(
                        "{}jet_std::DataTree::Object({})",
                        cx.root_prefix, obj_tmp
                    );
                    let map_ty = ty.clone().unwrap_or(Type::Map {
                        key: Box::new(Type::String),
                        value: Box::new(Type::Named(Syntax::TYPE_DATA.to_string())),
                    });
                    let prefix = TStmt::Let {
                        name: name.clone(),
                        kw: "let",
                        ty_clause: format!(": {}", cx.rust_type(&map_ty)),
                        init: TExpr {
                            ty: map_ty.clone(),
                            kind: TExprKind::ConstInline(format!(
                                "{}.into_iter().collect()",
                                obj_tmp
                            )),
                        },
                    };
                    return (
                        TIfCond::IfLet { pat_str, subj },
                        Some((name.clone(), place, Some(map_ty))),
                        vec![prefix],
                    );
                }
                let pat_str = emit_if_let_pattern(cx, pattern);
                return (
                    TIfCond::IfLet { pat_str, subj },
                    Some((name.clone(), place, ty)),
                    Vec::new(),
                );
            }
        }
        // c109 (B4): a USER-enum variant if-let (`if m == Ping(n)`). The Rust if-let
        // pattern is the same `emit_if_let_pattern` (`user_E::user_V(user_b)`), and the
        // binding's type is the variant's first payload type from `variant_binding_types`
        // (the same total fact `add_pattern_bindings` reads on the AST path).
        if !is_json_variant(variant) {
            if let Some(PatSlot::Bind(name)) = bindings.first() {
                let subj = lower_expr(subject, cx, env);
                let pat_str = emit_if_let_pattern(cx, pattern);
                let ty = variant_binding_types(cx, variant)
                    .and_then(|ts| ts.into_iter().next());
                let place = mangle(name);
                return (
                    TIfCond::IfLet { pat_str, subj },
                    Some((name.clone(), place, ty)),
                    Vec::new(),
                );
            }
            // c109 (D-PATW): a WILDCARD payload slot (`if w == Some(_)`). `_` binds
            // nothing, so the if-let introduces NO then-branch binding; the pattern
            // renders the slot as `_` (`emit_if_let_pattern`), byte-for-byte the AST.
            if let Some(PatSlot::Wildcard) = bindings.first() {
                let subj = lower_expr(subject, cx, env);
                let pat_str = emit_if_let_pattern(cx, pattern);
                return (TIfCond::IfLet { pat_str, subj }, None, Vec::new());
            }
        }
    }
    if let Expr::PatternTest {
        subject, pattern, ..
    } = cond
    {
        if matches!(
            pattern,
            Pattern::Present { .. } | Pattern::Ok { .. } | Pattern::Err { .. }
        ) {
            let subj = lower_expr(subject, cx, env);
            let pat_str = emit_if_let_pattern(cx, pattern);
            // The bound name + its inner type, off the subject's resolved Option/Result
            // (totality — never re-inferred). Mirrors `add_pattern_bindings`.
            let binding = match pattern {
                Pattern::Present { binding, .. } => {
                    let ty = match &subj.ty {
                        Type::Option(inner) => Some((**inner).clone()),
                        _ => None,
                    };
                    (binding.clone(), ty)
                }
                Pattern::Ok { binding, .. } => {
                    let ty = match &subj.ty {
                        Type::Result { ok, .. } => Some((**ok).clone()),
                        _ => None,
                    };
                    (binding.clone(), ty)
                }
                Pattern::Err { binding, .. } => {
                    let ty = match &subj.ty {
                        Type::Result { err, .. } => Some((**err).clone()),
                        _ => None,
                    };
                    (binding.clone(), ty)
                }
                _ => unreachable!("checked above"),
            };
            let (name, ty) = binding;
            let place = mangle(&name);
            return (
                TIfCond::IfLet { pat_str, subj },
                Some((name, place, ty)),
                Vec::new(),
            );
        }
    }
    (TIfCond::Plain(lower_expr(cond, cx, env)), None, Vec::new())
}

pub(crate) fn lower_if(ifs: &IfStmt, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    // c109 Phase 22: classify the condition (plain / if-let / is_none), reproducing
    // `emit_if`'s three head shapes. The if-let form binds its name into the
    // then-branch scope (mirroring `add_pattern_bindings`).
    let (cond, then_binding, then_prefix) = lower_if_cond(&ifs.cond, cx, env);
    // Each branch gets its own `locals` scope (deep-cloned, so a `let` is not visible
    // after the `if`). The panic-dump replica leaks for a plain/`is_none` `if` (the AST
    // `emit_if` passes the SHARED `&mut env` to `emit_stmts` → `clone_env`), but does
    // NOT leak for an if-let condition (the AST clones the env into a fresh `body_env`
    // before `add_pattern_bindings` → `fork_panic`, a deep-copied replica), so a `let`
    // inside an if-let then-body is scoped exactly as the AST's `body_env`.
    let then_body = {
        let mut branch = if then_binding.is_some() {
            fork_panic(env)
        } else {
            clone_env(env)
        };
        if let Some((name, place, ty)) = then_binding {
            branch.bind(&name, place, ty);
        }
        // D-ENC-DYN1=A+: a `Data` `Object(entries)` if-let prepends a `let` that
        // collects the matched `Vec<(String, DataTree)>` pairs into the `BTreeMap` the
        // body sees. Emitted before the source body statements.
        let mut body = then_prefix;
        body.extend(lower_stmts(&ifs.then_body, cx, &mut branch));
        body
    };
    let (else_body, else_is_elseif) = match &ifs.else_branch {
        None => (None, false),
        Some(ElseBranch::Else(body)) => {
            let mut branch = clone_env(env);
            (Some(lower_stmts(body, cx, &mut branch)), false)
        }
        // `else if` nests as an else-body holding a single `If`; the flag marks it so
        // emit renders `} else if …` (an explicit `else { if … }` block does NOT).
        Some(ElseBranch::ElseIf(next)) => {
            let mut branch = clone_env(env);
            (Some(vec![lower_if(next, cx, &mut branch)]), true)
        }
    };
    TStmt::If {
        cond,
        then_body,
        else_body,
        else_is_elseif,
    }
}

/// c109 Phase 4: lower a `when`/match. The gate (`switch_in_subset`) has already
/// proved one of the two covered shapes; pick the matching lowering.
pub(crate) fn lower_switch(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // Shape B: all arm-head ranges + else → if/else chain (`emit_mixed_switch`).
    if else_body.is_some() && arms.iter().all(|a| arm_head_range(cx, &a.cond, subject).is_some()) {
        return lower_range_switch(subject, arms, else_body, cx, env);
    }
    // Shape C (c109 Phase 8): all arms are fallible/optional patterns → a Rust match
    // over the subject's Result/Option (`Ok(..)`/`Err(..)`/`Some(..)`/`None`).
    if arms
        .iter()
        .all(|a| arm_fallible_pattern(cx, &a.cond, subject).is_some())
    {
        return lower_fallible_match(subject, arms, else_body, cx, env);
    }
    // Shape D (c109 Phase 15): all arms are plain comparison/Bool conds — or D-IF3 range
    // heads mixed in — → the general mixed `if/else if … else` chain. (An all-range +
    // else switch already routed to shape B above; this catches the value+range mix.)
    if arms
        .iter()
        .all(|a| arm_is_plain_cond(cx, &a.cond, subject) || arm_head_range(cx, &a.cond, subject).is_some())
    {
        return lower_mixed_switch(subject, arms, else_body, cx, env);
    }
    // Shape A: exhaustive enum match (`emit_pattern_match_switch`).
    lower_enum_match(subject, arms, else_body, cx, env)
}

/// c109 Phase 15: lower a MIXED comparison/Bool `when` switch (shape D) to a
/// `TStmt::MixedSwitch`, reproducing `emit_mixed_switch` (Source/Codegen/Statement.rs).
/// The subject is bound once to `_jet_switch_subject = &(subject)` (emitted for parity);
/// each arm's PLAIN condition is resolved to a Rust string at lowering (`emit_expr`); the
/// arm bodies + `else` are lowered on a SHARED env (leaky, like the AST `&mut env`).
pub(crate) fn lower_mixed_switch(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // The subject's emitted string, used once for the `_jet_switch_subject` borrow —
    // exactly as `emit_mixed_switch` re-emits `emit_expr(subject)`.
    let subject_str = emit_tir_expr(&lower_expr(subject, cx, env), cx);
    let mut tarms = Vec::new();
    for arm in arms {
        // D-IF3: a range head (`400..499 ->`) becomes `subject >= lo && subject <= hi`,
        // reusing the subject's emitted form; a plain comparison/Bool head →
        // `emit_switch_arm_cond`'s `emit_expr(cond)`.
        let cond_str = if let Some((lo, hi)) = arm_head_range(cx, &arm.cond, subject) {
            format!("{0} >= {1} && {0} <= {2}", subject_str, lo, hi)
        } else {
            emit_tir_expr(&lower_expr(&arm.cond, cx, env), cx)
        };
        // The arm body uses the SHARED `&mut env` in `emit_mixed_switch` (leaks).
        let mut branch = clone_env(env);
        let body = lower_stmts(&arm.body, cx, &mut branch);
        tarms.push((cond_str, body));
    }
    let else_lowered = else_body.as_ref().map(|body| {
        let mut branch = clone_env(env);
        lower_stmts(body, cx, &mut branch)
    });
    TStmt::MixedSwitch {
        subject_str,
        arms: tarms,
        else_body: else_lowered,
    }
}

/// c109 Phase 8: lower a fallible/optional pattern match (`when … { it == ok(n) ->
/// … }`). Reuses the `EnumMatch` TStmt — the scrutinee is the subject's emitted form
/// (a covered fallible/optional value: a user fallible fn call, an optional local,
/// etc.; no by-reference clone arises since those subjects are not deref'd enum
/// params), and each arm's pattern is the Rust `Ok(b)`/`Err(b)`/`Some(b)`/`None`,
/// mirroring `emit_match_pattern`. Binding payload types come from the subject's
/// resolved Result/Option type (totality), reproducing `add_pattern_bindings`.
pub(crate) fn lower_fallible_match(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // The subject's resolved type carries the ok/err/present payload types. Lower the
    // subject once to get both its emitted string and its total type.
    let subject_t = lower_expr(subject, cx, env);
    let subject_ty = subject_t.ty.clone();
    // A by-reference enum param is cloned in the enum-match path; a fallible/optional
    // subject in-subset is never a deref'd slot (it is a fn-call value or an owned
    // local), so the scrutinee is the plain emitted form — matching the AST path,
    // whose `subj` clone branch only fires for a deref'd `Ident`.
    let scrutinee = match subject {
        Expr::Ident(name, _) if env.is_borrowed(name) => {
            format!("({}).clone()", env.rust_name_of(name))
        }
        _ => emit_tir_expr(&subject_t, cx),
    };
    let mut tarms = Vec::new();
    for arm in arms {
        let pattern = arm_fallible_pattern(cx, &arm.cond, subject).expect("gate proved fallible arm");
        let pat = tir_fallible_pattern(&pattern);
        // An arm body is a CLONED env in `emit_pattern_match_switch` (no leak) — fork.
        let mut body_env = fork_panic(env);
        tir_add_fallible_binding(&pattern, &mut body_env, &subject_ty);
        let body = lower_stmts(&arm.body, cx, &mut body_env);
        tarms.push(TMatchArm { pattern: pat, guard: None, body });
    }
    // The `else` arm uses the SHARED `&mut env` in `emit_pattern_match_switch` (leaks).
    let else_lowered = else_body.as_ref().map(|body| {
        let mut branch = clone_env(env);
        lower_stmts(body, cx, &mut branch)
    });
    // No explicit `else` → the AST path (`emit_pattern_match_switch`) appends
    // `_ => unreachable!(…)` so rustc sees a complete match (sema proved E0307).
    let fallthrough = else_body.is_none();
    TStmt::EnumMatch {
        scrutinee,
        arms: tarms,
        else_body: else_lowered,
        fallthrough,
    }
}

/// c109 Phase 8: the Rust match pattern for a fallible/optional pattern, mirroring
/// `emit_match_pattern`'s Ok/Err/Present/Absent arms (Statement.rs).
pub(crate) fn tir_fallible_pattern(pattern: &Pattern) -> String {
    match pattern {
        Pattern::Ok { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Err { binding, .. } => format!("Err({})", mangle(binding)),
        Pattern::Present { binding, .. } => format!("Some({})", mangle(binding)),
        Pattern::Absent(_) => "None".to_string(),
        _ => unreachable!("non-fallible pattern in fallible match (gate)"),
    }
}

/// c109 Phase 8: bind the ok/err/present payload to its resolved type, read from the
/// subject's Result/Option type. Mirrors `add_pattern_bindings`'s Ok/Err/Present
/// arms (the binding's `jet_ty` is the inner type so any arithmetic on it traps
/// exactly as the AST path; `null` binds nothing).
pub(crate) fn tir_add_fallible_binding(pattern: &Pattern, env: &mut LowerEnv, subject_ty: &Type) {
    let (binding, ty) = match (pattern, subject_ty) {
        (Pattern::Ok { binding, .. }, Type::Result { ok, .. }) => {
            (binding.clone(), Some((**ok).clone()))
        }
        (Pattern::Err { binding, .. }, Type::Result { err, .. }) => {
            (binding.clone(), Some((**err).clone()))
        }
        (Pattern::Present { binding, .. }, Type::Option(inner)) => {
            (binding.clone(), Some((**inner).clone()))
        }
        // The subject type didn't resolve to the expected shape (impossible for a
        // covered subject — sema validated it); bind with no type (matches the AST
        // path's `jet_ty: None` fallback).
        (Pattern::Ok { binding, .. }, _)
        | (Pattern::Err { binding, .. }, _)
        | (Pattern::Present { binding, .. }, _) => (binding.clone(), None),
        // `null` (Absent) binds nothing.
        _ => return,
    };
    env.bind(&binding, mangle(&binding), ty);
}

pub(crate) fn lower_enum_match(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // The match owns the value. Mirror `emit_pattern_match_switch`: a by-reference
    // subject (a deref'd enum param) is cloned as `({rust_name}).clone()` — the
    // borrow itself is cloned, NOT the deref'd place. Any other subject emits its
    // plain form.
    let scrutinee = match subject {
        Expr::Ident(name, _) if env.is_borrowed(name) => {
            format!("({}).clone()", env.rust_name_of(name))
        }
        _ => emit_tir_expr(&lower_expr(subject, cx, env), cx),
    };
    // Resolve the owning enum once — drives the Rust variant prefix in patterns.
    let enum_type = arms.iter().find_map(|a| {
        arm_variant_pattern(cx, &a.cond, subject).and_then(|p| variant_pattern_enum(cx, &p))
    });
    // The subject's resolved Jet type carries the variant binding payload types.
    let subject_ty = expr_ast_jet_ty(subject, env);
    let mut tarms = Vec::new();
    for arm in arms {
        let pattern =
            arm_variant_pattern(cx, &arm.cond, subject).expect("gate proved variant arm");
        let pat = tir_match_pattern(cx, &pattern, enum_type.as_deref());
        let guard = tir_range_guard(&pattern);
        // The arm body sees the variant's payload bindings, typed from the layout. The
        // arm body is a CLONED env in `emit_pattern_match_switch` (no leak) — fork.
        let mut body_env = fork_panic(env);
        tir_add_pattern_bindings(cx, &pattern, &mut body_env, subject_ty.as_ref());
        let body = lower_stmts(&arm.body, cx, &mut body_env);
        tarms.push(TMatchArm { pattern: pat, guard, body });
    }
    // The `else` arm uses the SHARED `&mut env` in `emit_pattern_match_switch` (leaks).
    let else_lowered = else_body.as_ref().map(|body| {
        let mut branch = clone_env(env);
        lower_stmts(body, cx, &mut branch)
    });
    // No explicit `else` → the AST path appends `_ => unreachable!(…)` so rustc
    // sees a complete match (sema already proved exhaustiveness — E0307).
    let fallthrough = else_body.is_none();
    TStmt::EnumMatch {
        scrutinee,
        arms: tarms,
        else_body: else_lowered,
        fallthrough,
    }
}

pub(crate) fn lower_range_switch(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // The subject's emitted string — used for the borrow binding and each range
    // condition, exactly as `emit_mixed_switch` re-emits the subject.
    let subject_str = emit_tir_expr(&lower_expr(subject, cx, env), cx);
    let mut tarms = Vec::new();
    for arm in arms {
        let (lo, hi) = arm_head_range(cx, &arm.cond, subject).expect("gate proved range arm");
        let mut branch = clone_env(env);
        let body = lower_stmts(&arm.body, cx, &mut branch);
        tarms.push((lo, hi, body));
    }
    let else_lowered = {
        let body = else_body.as_ref().expect("range switch requires else (gate)");
        let mut branch = clone_env(env);
        lower_stmts(body, cx, &mut branch)
    };
    TStmt::RangeSwitch {
        subject_str,
        arms: tarms,
        else_body: else_lowered,
    }
}

/// TIR-local reproduction of codegen's `emit_match_pattern` for the enum-match (shape
/// A) case the subset covers. c109 Phase 24: this now DELEGATES to the AST
/// `emit_match_pattern` (made `pub(crate)`), which is PURE formatting (it takes only
/// `cx` + the pattern + the resolved enum type — no env, no inference), so reusing it is
/// byte-parity-safe and automatically handles the FOREIGN-enum (`{root}{mod}::user_<T>::
/// user_<V>`) and JSON (`{root}jet_std::Json::<Variant>`, non-mangled) variant prefixes
/// the subset now admits — the same reuse Phase 22 made for `emit_if_let_pattern`.
pub(crate) fn tir_match_pattern(cx: &Cx, pattern: &Pattern, enum_type: Option<&str>) -> String {
    emit_match_pattern(cx, pattern, enum_type)
}

/// TIR-local reproduction of codegen's `emit_range_guard` (Statement.rs): a payload
/// range slot becomes `__jet_range_i >= lo && __jet_range_i <= hi`. `None` when no
/// slot is a range. Or-patterns reuse the first alt's ranges (all alts bind alike).
pub(crate) fn tir_range_guard(pattern: &Pattern) -> Option<String> {
    match pattern {
        Pattern::Variant { bindings, .. } => {
            let guards: Vec<String> = bindings
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    if let PatSlot::Range { lo, hi } = s {
                        Some(format!(
                            "__jet_range_{} >= {} && __jet_range_{} <= {}",
                            i, lo, i, hi
                        ))
                    } else {
                        None
                    }
                })
                .collect();
            if guards.is_empty() {
                None
            } else {
                Some(guards.join(" && "))
            }
        }
        Pattern::Or(alts, _) => alts.first().and_then(tir_range_guard),
        _ => None,
    }
}

/// TIR-local reproduction of codegen's `add_pattern_bindings`/`variant_binding_types`
/// for the user-enum case: bind each `Bind` slot to its payload field type, read
/// from the resolved enum layout (`cx.enum_variants`). Wildcard/Range slots bind
/// nothing. Or-patterns bind the first alt's names (all alts bind alike — E0317).
pub(crate) fn tir_add_pattern_bindings(
    cx: &Cx,
    pattern: &Pattern,
    env: &mut LowerEnv,
    _subject_ty: Option<&Type>,
) {
    match pattern {
        Pattern::Variant { variant, bindings, .. } => {
            let tys = variant_payload_types(cx, variant);
            for (i, slot) in bindings.iter().enumerate() {
                if let PatSlot::Bind(b) = slot {
                    // Payload types are scalar/Char (the enum is covered), so the
                    // binding is a by-value local; default to Int if unresolved
                    // (impossible for a covered enum — sema validated the access).
                    let ty = tys
                        .as_ref()
                        .and_then(|ts| ts.get(i).cloned())
                        .unwrap_or(Type::Int);
                    env.bind(b, mangle(b), Some(ty));
                }
            }
        }
        Pattern::Or(alts, _) => {
            if let Some(first) = alts.first() {
                tir_add_pattern_bindings(cx, first, env, _subject_ty);
            }
        }
        _ => {}
    }
}

/// The payload field types a variant binds, from the resolved enum layout. c109
/// Phase 24: DELEGATES to the AST `variant_binding_types` (made `pub(crate)`), which
/// handles the JSON enum (`core_json_pattern_types`) AND user/foreign enums
/// (`cx.variant_owner` → `cx.enum_variants`) — pure table lookups, no env/inference —
/// so the bound payload type is byte-parity-faithful for every covered enum (e.g. a
/// foreign `ParseError.NoFrontmatter(p)` binds `p: String`).
pub(crate) fn variant_payload_types(cx: &Cx, variant: &str) -> Option<Vec<Type>> {
    variant_binding_types(cx, variant)
}

/// c109 Phase 24: the Rust enum-literal head `{prefix}::{mangle(variant)}` for a payload
/// or named enum literal, reproducing `emit_enum_lit`'s `type_prefix` (Expression.rs): a
/// FOREIGN (imported) enum → `{root}{mod}::user_<T>::user_<V>`, a local enum →
/// `user_<T>::user_<V>`. Keyed on the ENUM name in `cx.foreign_types`, byte-for-byte.
pub(crate) fn tir_enum_lit_prefix(cx: &Cx, type_name: &str, variant: &str) -> String {
    // D-TERM1 (ratified 2026-06-22): `Key` is a prelude enum; its Rust name is `JetKey`.
    // Variant names are not mangled (Char, Enter, …).
    if type_name == crate::Syntax::TYPE_KEY {
        return format!("{}JetKey::{}", cx.root_prefix, variant);
    }
    let type_prefix = match cx.foreign_types.get(type_name) {
        Some(rust_mod) => format!("{}{}::user_{}", cx.root_prefix, rust_mod, type_name),
        None => format!("user_{}", type_name),
    };
    format!("{}::{}", type_prefix, mangle(variant))
}

/// c109 Phase 16: the single-payload type of `(type_name, edge)`, mirroring the AST
/// `enum_variant_payload_type` (Expression.rs). `edge` is the VARIANT name for a
/// positional arg, or `"Variant.label"` for a named arg — the latter never matches a
/// variant name, so it returns `None` (the AST never clones a named-payload arg), as
/// `enum_variant_payload_type` does. Only `Single(t)` / single-field `Named` resolve.
pub(crate) fn enum_variant_payload_type<'a>(cx: &'a Cx, type_name: &str, edge: &str) -> Option<&'a Type> {
    let variants = cx.enum_variants.get(type_name)?;
    let (_, payload) = variants.iter().find(|(v, _)| v == edge)?;
    match payload {
        VariantPayload::Single(t, _) => Some(t),
        VariantPayload::Named(fs) if fs.len() == 1 => Some(&fs[0].ty),
        _ => None,
    }
}

/// c109 Phase 16: lower one enum-literal payload arg, resolving the `clone`/`boxed`
/// decisions as TOTAL facts, reproducing `emit_boxed_enum_arg` (Expression.rs)
/// byte-for-byte. `edge` is the variant name (positional) or `"Variant.label"`
/// (named). A non-scalar single-payload type whose arg is a borrowed-in-env ident
/// gets `(…).clone()`; a recursive (`boxed_edge`) edge gets `Box::new(…)`.
pub(crate) fn lower_enum_arg(
    type_name: &str,
    variant: &str,
    edge: &str,
    e: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TEnumArg {
    let payload_ty = enum_variant_payload_type(cx, type_name, edge);
    let borrowed = matches!(e, Expr::Ident(name, _) if env.is_borrowed(name));
    let clone = payload_ty.is_some_and(|t| !t.is_scalar()) && borrowed;
    let boxed = cx
        .boxed_edges
        .contains(&(type_name.to_string(), edge.to_string()));
    let _ = variant;
    TEnumArg {
        value: lower_expr(e, cx, env),
        clone,
        boxed,
    }
}

/// Resolve the subject's Jet type for binding payloads, mirroring `expr_jet_ty`'s
/// reach (only an Ident resolves via its slot). Enough for the covered subset (the
/// subject is an enum-typed local/param). Other forms resolve to `None` (the
/// payload types come from `cx.enum_variants` regardless).
pub(crate) fn expr_ast_jet_ty(e: &Expr, env: &LowerEnv) -> Option<Type> {
    match e {
        Expr::Ident(name, _) => env.ty_of(name),
        _ => None,
    }
}

/// Clone the env for a LEAKY child scope (a plain block / loop / mixed-or-range switch
/// arm / comptime-if branch / enum-match else). `locals` is deep-cloned (each branch
/// scopes its own bindings for resolution), but `panic_locals` is SHARED (the Rc is
/// cloned, not its contents) so a `let` inside the child leaks into the parent's panic
/// dump — exactly as the AST codegen `&mut env` does (`safe_locals_expr`).
pub(crate) fn clone_env(env: &LowerEnv) -> LowerEnv {
    LowerEnv {
        locals: env.locals.clone(),
        fn_name: env.fn_name.clone(),
        panic_locals: Rc::clone(&env.panic_locals),
        view_return: env.view_return,
    }
}

/// Clone the env for a NON-LEAKY child scope — the two `emit_pattern_match_switch` arm
/// bodies (an enum or fallible/optional match arm; the AST uses `env.clone()` there) and
/// a lambda body (`emit_lambda` clones the env). Here `panic_locals` is DEEP-COPIED, so
/// bindings inside the arm/lambda do NOT leak into the enclosing function's panic dump,
/// matching the AST's cloned `body_env`/`lam_env`.
pub(crate) fn fork_panic(env: &LowerEnv) -> LowerEnv {
    LowerEnv {
        locals: env.locals.clone(),
        fn_name: env.fn_name.clone(),
        panic_locals: Rc::new(RefCell::new(env.panic_locals.borrow().clone())),
        view_return: env.view_return,
    }
}

/// c109 Phase 15: render the `{ jet_panic_rich(…); }` statement string for a
/// `a ?? panic(msg)` fallback, byte-for-byte `emit_panic_stop`
/// (Source/Codegen/Statement.rs). Every input — the panic message (lowered from the
/// message expression), the source-line text / line / column / caret width (from
/// `cx.src` at the `panic` name span), the escaped file + enclosing function name, and
/// the sorted scalar-locals snapshot — is resolved here so emit reads nothing from
/// `cx.src`/`cx.current_fn` (I3).
pub(crate) fn render_panic_stop(
    name_span: &Span,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &mut LowerEnv,
) -> String {
    let msg = render_panic_message(&args[0].expr, cx, env);
    let (src_line, line, col) = tir_src_line_at(&cx.src, name_span.start);
    let caret_len = (name_span.end - name_span.start) as u32;
    let fn_name = env.fn_name.clone();
    let locals_expr = render_safe_locals(env);
    format!(
        "{{ jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }}",
        file = escape_rust_str(&cx.file),
        line = line,
        fn_name_esc = escape_rust_str(&fn_name),
        src_line_esc = escape_rust_str(src_line.trim_end()),
        col = col,
        caret = caret_len,
        msg = msg,
        locals = locals_expr,
    )
}

/// c109 Phase 26: render `require(cond[, msg])` (S36), byte-for-byte `emit_require`
/// (Source/Codegen/Statement.rs). The default build emits a guarded `jet_panic_rich`;
/// `cx.test_mode` emits a `return Err(<msg>)` form. The condition + message are lowered
/// via the TIR; every source-position/locals fact is resolved here (I3).
pub(crate) fn render_require(call: &crate::AST::Call, cx: &Cx, env: &mut LowerEnv) -> String {
    let cond = emit_tir_expr(&lower_expr(&call.args[0].expr, cx, env), cx);
    let msg = if call.args.len() == 2 {
        render_panic_message(&call.args[1].expr, cx, env)
    } else {
        "\"condition failed\"".to_string()
    };
    if cx.test_mode {
        let msg_expr = if call.args.len() == 2 {
            msg
        } else {
            "\"condition failed\".to_string()".to_string()
        };
        return format!("{{ if !({}) {{ return Err({}); }} }}", cond, msg_expr);
    }
    let (src_line, line, col) = tir_src_line_at(&cx.src, call.name_span.start);
    let caret_len = (call.name_span.end - call.name_span.start) as u32;
    let fn_name = env.fn_name.clone();
    let locals_expr = render_safe_locals(env);
    let msg_used = if call.args.len() == 2 {
        msg
    } else {
        "\"condition failed\".to_string()".to_string()
    };
    format!(
        "{{ if !({cond}) {{ jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
        cond = cond,
        file = escape_rust_str(&cx.file),
        line = line,
        fn_name_esc = escape_rust_str(&fn_name),
        src_line_esc = escape_rust_str(src_line.trim_end()),
        col = col,
        caret = caret_len,
        msg = msg_used,
        locals = locals_expr,
    )
}

/// c109 Phase 26: render `require_eq(left, right)` (S36), byte-for-byte
/// `emit_require_eq` (Source/Codegen/Statement.rs). Binds the two operands into temps,
/// then compares; on inequality emits the test-mode `return Err(…)` or the default
/// `jet_panic_rich` with a `left: {}, right: {}` message.
pub(crate) fn render_require_eq(call: &crate::AST::Call, cx: &Cx, env: &mut LowerEnv) -> String {
    let left = emit_tir_expr(&lower_expr(&call.args[0].expr, cx, env), cx);
    let right = emit_tir_expr(&lower_expr(&call.args[1].expr, cx, env), cx);
    if cx.test_mode {
        return format!(
            "{{ let _jet_left = ({}); let _jet_right = ({}); if !(_jet_left == _jet_right) {{ return Err(format!(\"left: {{}}, right: {{}}\", _jet_left.jet_show(), _jet_right.jet_show())); }} }}",
            left, right
        );
    }
    let (src_line, line, col) = tir_src_line_at(&cx.src, call.name_span.start);
    let caret_len = (call.name_span.end - call.name_span.start) as u32;
    let fn_name = env.fn_name.clone();
    let locals_expr = render_safe_locals(env);
    format!(
        "{{ let _jet_left = ({left}); let _jet_right = ({right}); if !(_jet_left == _jet_right) {{ jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &format!(\"left: {{}}, right: {{}}\", _jet_left.jet_show(), _jet_right.jet_show()), &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
        left = left,
        right = right,
        file = escape_rust_str(&cx.file),
        line = line,
        fn_name_esc = escape_rust_str(&fn_name),
        src_line_esc = escape_rust_str(src_line.trim_end()),
        col = col,
        caret = caret_len,
        locals = locals_expr,
    )
}

/// c109 Phase 15: reproduce `emit_panic_message` (Statement.rs): a `Str` literal emits
/// its interpolated form directly; any other expression is `({…}).jet_show()`. The
/// message expression is lowered + emitted via the TIR (= `emit_expr`).
pub(crate) fn render_panic_message(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> String {
    match e {
        Expr::Str(_, _) => emit_tir_expr(&lower_expr(e, cx, env), cx),
        other => format!("({}).jet_show()", emit_tir_expr(&lower_expr(other, cx, env), cx)),
    }
}

/// c109 Phase 15: reproduce `src_line_at` (Statement.rs) — the (line text, 1-based line,
/// 1-based column) for a byte offset.
pub(crate) fn tir_src_line_at(src: &str, offset: usize) -> (&str, u32, u32) {
    let (line, col) = crate::Diagnostics::span_line_col(src, offset);
    let line_start = src[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line_end = src[offset..].find('\n').map(|p| offset + p).unwrap_or(src.len());
    (&src[line_start..line_end], line as u32, col as u32)
}

/// c109 Phase 15: reproduce `safe_locals_expr` (Statement.rs) from the `panic_locals`
/// env replica (which mirrors the AST codegen `env` leak semantics — see `LowerEnv`).
/// Dumps the FULL replica filtered to scalar Int/Float/Bool slots, sorted by name, as a
/// `format!("name = {}, …", (place).jet_show(), …)` expression. A deref'd slot uses
/// `(*name).jet_show()` (the place already carries the `(*…)` wrapper, which is the bare
/// `(*name)` form, NOT a double-paren). Empty → `String::new()`.
pub(crate) fn render_safe_locals(env: &LowerEnv) -> String {
    let replica = env.panic_locals.borrow();
    let mut parts: Vec<(String, String)> = replica
        .iter()
        .filter_map(|(name, (place, jet_ty))| {
            let safe = jet_ty
                .as_ref()
                .map_or(false, |t| matches!(t, Type::Int | Type::Float | Type::Bool));
            if !safe {
                return None;
            }
            // `safe_locals_expr` builds `(*rust_name).jet_show()` for a deref'd slot and
            // `(rust_name).jet_show()` otherwise. The replica's `place` is exactly
            // `(*rust_name)` (deref) or `rust_name` — decode it back so the rendered
            // string is byte-identical (NOT `((*rust_name)).jet_show()`).
            let value_expr = if place.starts_with("(*") && place.ends_with(')') {
                let rust_name = &place[2..place.len() - 1];
                format!("(*{}).jet_show()", rust_name)
            } else {
                format!("({}).jet_show()", place)
            };
            Some((name.clone(), value_expr))
        })
        .collect();
    parts.sort_by(|a, b| a.0.cmp(&b.0));
    if parts.is_empty() {
        return "String::new()".to_string();
    }
    let fmt_str = parts
        .iter()
        .map(|(n, _)| format!("{} = {{}}", n))
        .collect::<Vec<_>>()
        .join(", ");
    let args = parts
        .iter()
        .map(|(_, e)| e.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("format!(\"{}\", {})", fmt_str, args)
}

pub(crate) fn lower_expr(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    match e {
        Expr::Int(n, _, width) => TExpr {
            ty: int_lit_type(width),
            kind: TExprKind::IntLit(*n, *width),
        },
        Expr::Float(v, _, is_f32) => TExpr {
            // D-FLOATW1: sema resolves F32 context and writes `is_f32=true` on the
            // node; carry that width through to TIR so emit produces the right suffix.
            ty: if *is_f32 { Type::Float32 } else { Type::Float },
            kind: TExprKind::FloatLit(*v),
        },
        Expr::Bool(b, _) => TExpr {
            ty: Type::Bool,
            kind: TExprKind::BoolLit(*b),
        },
        Expr::Char(c, _) => TExpr {
            ty: Type::Char,
            kind: TExprKind::CharLit(*c),
        },
        Expr::Str(parts, _) => {
            let tparts = parts
                .iter()
                .map(|p| match p {
                    StrPart::Lit(s) => TStrPart::Lit(s.clone()),
                    StrPart::Interp(e) => TStrPart::Interp(lower_expr(e, cx, env)),
                })
                .collect();
            TExpr {
                ty: Type::String,
                kind: TExprKind::StrLit(tparts),
            }
        }
        Expr::Ident(name, _) => {
            // c109 Phase 24: a comptime CONST inlines its pre-rendered value FIRST (the
            // AST `emit_expr` Ident arm returns `cx.consts[name]` before any env/fn-value
            // check — so a const takes precedence even over a same-named local, matching
            // byte-for-byte). The `ty` is a placeholder (never read — see `ConstInline`).
            if let Some(val) = cx.consts.get(name) {
                return TExpr {
                    ty: env.ty_of(name).unwrap_or(Type::Int),
                    kind: TExprKind::ConstInline(val.clone()),
                };
            }
            // c109 Phase 13: a bare function name used as a VALUE (not a local, not a
            // const) emits `emit_named_fn_value` — `Box::new(move |…| user_<name>(…))
            // as <fn-type>`. Mirrors `emit_expr`'s `Expr::Ident` arm (Expression.rs).
            if !env.locals.contains_key(name) && !cx.consts.contains_key(name) {
                if let Some(ft @ Type::Fn { .. }) = cx.fn_types.get(name) {
                    return TExpr {
                        ty: ft.clone(),
                        kind: TExprKind::FnValue {
                            kind: TFnValueKind::NamedFn {
                                wrapper: emit_named_fn_value(cx, name, ft),
                            },
                        },
                    };
                }
            }
            let ty = env.ty_of(name).unwrap_or(Type::Int);
            TExpr {
                ty,
                kind: TExprKind::Local(env.place_of(name)),
            }
        }
        // c109 Phase 13: a call THROUGH a fn-value `(f)(args)` (`Expr::CallValue`). The
        // AST path (`emit_expr`'s `Expr::CallValue`) emits `({callee})({args})` with the
        // args lowered PLAINLY (it passes `None` to `emit_call_args` → no convention/
        // clone/borrow wrappers). Reproduce exactly: lower the callee, lower each arg
        // with `conv = None`. The result type is the callee fn-type's return (total).
        Expr::CallValue { callee, args, .. } => {
            let callee_t = lower_expr(callee, cx, env);
            let ret_ty = match &callee_t.ty {
                Type::Fn { ret: Some(r), .. } => (**r).clone(),
                _ => unit_type(),
            };
            let targs = args
                .iter()
                .map(|a| lower_one_call_arg(a, None, env, cx))
                .collect();
            TExpr {
                ty: ret_ty,
                kind: TExprKind::FnValue {
                    kind: TFnValueKind::Call {
                        callee: Box::new(callee_t),
                        args: targs,
                    },
                },
            }
        }
        Expr::Unary(op, inner, _) => {
            let operand = lower_expr(inner, cx, env);
            let ty = operand.ty.clone();
            TExpr {
                ty,
                kind: TExprKind::Unary {
                    op: *op,
                    operand: Box::new(operand),
                },
            }
        }
        // D-CAP9: postfix `p.*` deref. Result type is the pointer's element type.
        Expr::Deref(inner, _) => {
            let operand = lower_expr(inner, cx, env);
            let ty = crate::Sema::ptr_elem(&operand.ty).unwrap_or_else(|| operand.ty.clone());
            TExpr { ty, kind: TExprKind::Deref(Box::new(operand)) }
        }
        // D-CAP9: prefix `*x` raw-pointer-of. Result type is `*T` (`Ptr<T>`).
        Expr::RawOf(inner, _) => {
            let operand = lower_expr(inner, cx, env);
            let ty = crate::Sema::ptr_type(operand.ty.clone());
            TExpr { ty, kind: TExprKind::RawOf(Box::new(operand)) }
        }
        Expr::Binary(op, l, r, span) => {
            let lhs = lower_expr(l, cx, env);
            let rhs = lower_expr(r, cx, env);
            // Overflow decision, computed here once — this is the fact today's
            // `operand_is_integer` re-derives in codegen. It must mirror that
            // function EXACTLY (Codegen/Expression.rs): only a *resolvable*
            // integer operand traps. A struct-field read resolves to `None` in the
            // AST path (`expr_jet_ty` has no `Field` arm), so it does NOT trap —
            // hence we can't just inspect `TExpr.ty`, which is total even for a
            // field. We instead replay `operand_is_integer` on the AST operands.
            // `operand_is_integer` inspects only the LEFT spine of nested
            // arithmetic, so check the left operand first, then the right.
            // D-NUMOPS1: `+`/`-`/`*`/`/` trap on value overflow; `<<`/`>>` trap on a
            // bit-count out of the type's width (both via the `JetArith` helpers, so
            // no raw Rust overflow panic leaks — I2). A shift's overflow is governed
            // by its LEFT operand's integer-ness (the value), never the count.
            let arith_overflow = matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
                && (ast_operand_is_integer(l, env) == Some(true)
                    || ast_operand_is_integer(r, env) == Some(true));
            let shift_overflow = matches!(op, BinOp::Shl | BinOp::Shr)
                && ast_operand_is_integer(l, env) == Some(true);
            let overflow = arith_overflow || shift_overflow;
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0 as u32;
            // A comparison/logical op yields Bool; arithmetic keeps the operand type.
            // D-SIMD2 / D-LINALG1: a math-type operator's result follows the closed
            // family's rule (e.g. `Mat3 * Vec3 → Vec3`), not the left operand — read
            // it from the same sema table so the node's `ty` stays honest.
            let ty = if op.is_comparison() || matches!(op, BinOp::And | BinOp::Or) {
                Type::Bool
            } else if let (Type::Named(ln), Type::Named(rn)) = (&lhs.ty, &rhs.ty) {
                let lm = crate::Sema::is_math_type(ln) && !cx.type_names.contains(ln);
                let rm = crate::Sema::is_math_type(rn) && !cx.type_names.contains(rn);
                if lm || rm {
                    crate::Sema::math_binop_result(*op, ln, rn).unwrap_or_else(|| lhs.ty.clone())
                } else {
                    lhs.ty.clone()
                }
            } else {
                lhs.ty.clone()
            };
            TExpr {
                ty,
                kind: TExprKind::Binary {
                    op: *op,
                    overflow,
                    line,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            }
        }
        Expr::Call(call) => {
            // c109 Phase 13: `f(args)` where `f` is a LOCAL (a fn-typed binding/param)
            // parses as `Expr::Call`. The AST path (`emit_call`, env-contains-name
            // branch) emits `(place)(args)` with args PLAIN (`emit_call_args(.., None)`).
            // Reproduce as a `FnValue::Call` whose callee is the local's place.
            if env.locals.contains_key(&call.name) && !cx.consts.contains_key(&call.name) {
                let callee_ty = env.ty_of(&call.name).unwrap_or_else(unit_type);
                let ret_ty = match &callee_ty {
                    Type::Fn { ret: Some(r), .. } => (**r).clone(),
                    _ => unit_type(),
                };
                let callee_t = TExpr {
                    ty: callee_ty,
                    kind: TExprKind::Local(env.place_of(&call.name)),
                };
                let targs = call
                    .args
                    .iter()
                    .map(|a| lower_one_call_arg(a, None, env, cx))
                    .collect();
                return TExpr {
                    ty: ret_ty,
                    kind: TExprKind::FnValue {
                        kind: TFnValueKind::Call {
                            callee: Box::new(callee_t),
                            args: targs,
                        },
                    },
                };
            }
            // `print` is ambient only when the user has not defined their own
            // `print` function (matches emit_call; sema enforces the shadowing).
            if call.name == Syntax::BUILTIN_PRINT && !cx.sigs.contains_key(&call.name) {
                let arg = lower_expr(&call.args[0].expr, cx, env);
                return TExpr {
                    ty: unit_type(),
                    kind: TExprKind::Print(Box::new(arg)),
                };
            }
            // D-LIN1-DROP: `drop(x)` — discard the value (move-to-nowhere). Sema
            // proved the discard is audited when the value is `#SingleUse`. Lowers
            // to a plain `drop(arg)`; no `unsafe` (I3). Disjoint from a user `drop`
            // fn or local of that name (`cx.sigs`/`env.locals` would be set then).
            if call.name == Syntax::BUILTIN_DROP
                && !cx.sigs.contains_key(&call.name)
                && !env.locals.contains_key(&call.name)
            {
                let arg = lower_expr(&call.args[0].expr, cx, env);
                return TExpr {
                    ty: unit_type(),
                    kind: TExprKind::Drop(Box::new(arg)),
                };
            }
            // c109 Phase 26: the rich-runtime-report builtins (S36) — render the whole
            // emit string at lowering, byte-for-byte the AST helper. `require`/`panic`
            // are statement-position calls (a `()` result); the string is the `{ … }`
            // block emit emits as an expr-statement. Disjoint from a user fn of the same
            // name (`cx.sigs.contains_key` would be true then).
            if !cx.sigs.contains_key(&call.name) && !env.locals.contains_key(&call.name) {
                if call.name == Syntax::BUILTIN_REQUIRE {
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::RequireStop(render_require(call, cx, env)),
                    };
                }
                if call.name == Syntax::BUILTIN_REQUIRE_EQ {
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::RequireStop(render_require_eq(call, cx, env)),
                    };
                }
                if call.name == Syntax::BUILTIN_PANIC {
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::RequireStop(render_panic_stop(
                            &call.name_span,
                            &call.args,
                            cx,
                            env,
                        )),
                    };
                }
            }
            // c109 Phase 25: the ambient prelude `input(...)` (D-PRELUDE1 = B). Same
            // lowering as `io.input(...)` (CoreCall would also work, but the bare-call
            // surface has no module alias, so it is its own node). Resolves to
            // `Result<String, IOError>` (matching sema), so it composes with the
            // Phase-8 `??` fallback. The prompt arg (if any) is lowered in-subset.
            if call.name == Syntax::BUILTIN_INPUT
                && !cx.sigs.contains_key(&call.name)
                && !env.locals.contains_key(&call.name)
            {
                let prompt = call.args.first().map(|a| Box::new(lower_expr(&a.expr, cx, env)));
                return TExpr {
                    ty: Type::Result {
                        ok: Box::new(Type::String),
                        err: Box::new(Type::Named(Syntax::TYPE_IO_ERROR.to_string())),
                    },
                    kind: TExprKind::AmbientInput { prompt },
                };
            }
            // c109 Phase 28: the overflow opt-out builtins `wrapping(e)`/`saturating(e)`/
            // `checked(e)` (D-NUMOPS1). The gate proved the name is one of the three (not
            // shadowed) and the sole arg is an integer `Expr::Binary`. Reproduce
            // `emit_call`'s arm (Expression.rs ~L1756): `(lhs).{name}_{op}(rhs)` with PLAIN
            // operands (no trap helper). `checked_*` returns `Option<T>`; the others return
            // `T` — set the result type accordingly so a `checked(...) ?? x` composes.
            if matches!(
                call.name.as_str(),
                Syntax::BUILTIN_WRAPPING | Syntax::BUILTIN_SATURATING | Syntax::BUILTIN_CHECKED
            ) && !cx.sigs.contains_key(&call.name)
                && !env.locals.contains_key(&call.name)
            {
                if let Some(Expr::Binary(op, l, r, _)) = call.args.first().map(|a| &a.expr) {
                    let op_suffix = match op {
                        BinOp::Add => "add",
                        BinOp::Sub => "sub",
                        BinOp::Mul => "mul",
                        BinOp::Div => "div",
                        // Sema validated an arithmetic op; mirror the AST default.
                        _ => "add",
                    };
                    let lhs = lower_expr(l, cx, env);
                    let rhs = lower_expr(r, cx, env);
                    let val_ty = lhs.ty.clone();
                    let result_ty = if call.name == Syntax::BUILTIN_CHECKED {
                        Type::Option(Box::new(val_ty))
                    } else {
                        val_ty
                    };
                    return TExpr {
                        ty: result_ty,
                        kind: TExprKind::OverflowOpt {
                            prefix: call.name.clone(),
                            op: op_suffix,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                    };
                }
            }
            // c109 Phase 14: an FFI extern call (`emit_call`'s `extern_funcs` arm).
            // Checked BEFORE the unqualified arms, matching `emit_call`'s order. Args
            // use `emit_extern_call_args` (a non-scalar `Read` is `(…).clone()`).
            if !env.locals.contains_key(&call.name) {
                if let Some(wrapper) = cx.extern_funcs.get(&call.name).cloned() {
                    let sig = cx.sigs.get(&call.name).cloned();
                    let eargs = call
                        .args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let conv = sig.as_ref().and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
                            lower_extern_call_arg(a, conv, env, cx)
                        })
                        .collect();
                    // The extern fn's return type lives in `cx.fn_types` only if the
                    // function is also a normal sig; extern fns are not in `fn_types`,
                    // so fall back to Unit (the binding carries the real type — the call
                    // result type is rarely load-bearing, like every covered call).
                    return TExpr {
                        ty: call_return_type(cx, &call.name),
                        kind: TExprKind::ExternCall { wrapper, args: eargs },
                    };
                }
                // c109 Phase 14: unqualified inline-module import (`emit_call`'s
                // `unqualified_inline` arm) → `{root}user_{mangled}(args)`.
                if let Some(mangled_key) = cx.unqualified_inline.get(&call.name).cloned() {
                    let sig = cx.sigs.get(&mangled_key).cloned();
                    let args = call
                        .args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let conv = sig.as_ref().and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
                            lower_one_call_arg(a, conv, env, cx)
                        })
                        .collect();
                    return TExpr {
                        ty: call_return_type(cx, &mangled_key),
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::InlineMangled { mangled: mangled_key },
                            args,
                        },
                    };
                }
                // c109 Phase 14: unqualified file-module import (`emit_call`'s
                // `unqualified_file` arm) → `{root}{rust_mod}::{mangle(fn)}(args)`. The
                // AST looks up the sig under `(call.name, fn_name)`.
                if let Some((rust_mod, fn_name)) = cx.unqualified_file.get(&call.name).cloned() {
                    let sig = cx
                        .import_sigs
                        .get(&(call.name.clone(), fn_name.clone()))
                        .cloned();
                    let args = call
                        .args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let conv = sig.as_ref().and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
                            lower_one_call_arg(a, conv, env, cx)
                        })
                        .collect();
                    let ret = cx
                        .import_rets
                        .get(&(call.name.clone(), fn_name.clone()))
                        .cloned()
                        .flatten()
                        .unwrap_or_else(unit_type);
                    return TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod,
                                rust_fn: mangle(&fn_name).to_string(),
                            },
                            args,
                        },
                    };
                }
            }
            // D-SIMD2 / D-LINALG1: a built-in math-type constructor. Plainly lower the
            // float components and emit `{root}jet_math_<T>_new(…)`.
            if !env.locals.contains_key(&call.name)
                && crate::Sema::is_math_type(&call.name)
                && !cx.type_names.contains(&call.name)
            {
                let targs: Vec<TExpr> =
                    call.args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
                return TExpr {
                    ty: Type::Named(call.name.clone()),
                    kind: TExprKind::MathBuiltin {
                        type_name: call.name.clone(),
                        func: "new".to_string(),
                        args: targs,
                    },
                };
            }
            // Resolve the callee's signature so each arg's borrow/clone/fn-coercion is
            // decided here, totally — via the shared `lower_one_call_arg` (the single
            // `emit_call_args` reproduction). c109 Phase 13: a callee with a Fn-typed
            // param (now in subset) routes its arg through the Box-coercion form.
            let sig = cx.sigs.get(&call.name).cloned();
            let args = call
                .args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let conv = sig.as_ref().and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
                    lower_one_call_arg(a, conv, env, cx)
                })
                .collect();
            let ret = call_return_type(cx, &call.name);
            TExpr {
                ty: ret,
                kind: TExprKind::Call {
                    name: call.name.clone(),
                    args,
                },
            }
        }
        // c109 Phase 6: a method call. The gate (`method_call_in_subset`) admitted
        // exactly the synthetic `.clone()` or a user instance method on a covered
        // type; lower accordingly. Every dispatch fact is resolved here (totality).
        Expr::MethodCall { receiver, method, method_span, type_args: _, args, recv_type, resolved_ret } => {
            // D-SERDE6: codegen reads the decode target `T` from `resolved_ret`
            // (`Result<T,…>`), so the call-site `type_args` need no separate threading.
            lower_method_call(receiver, method, *method_span, args, recv_type, resolved_ret.as_ref(), cx, env)
        }
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            let c = lower_expr(cond, cx, env);
            // Value blocks scope their own bindings (like lambda block bodies).
            let mut then_env = clone_env(env);
            let t_body = lower_stmts(then_body, cx, &mut then_env);
            let t_val = lower_expr(then_value, cx, &mut then_env);
            let mut else_env = clone_env(env);
            let e_body = lower_stmts(else_body, cx, &mut else_env);
            let e_val = lower_expr(else_value, cx, &mut else_env);
            // Both arms share a type (sema guaranteed it); take the then arm's.
            let ty = t_val.ty.clone();
            TExpr {
                ty,
                kind: TExprKind::IfExpr {
                    cond: Box::new(c),
                    then_body: t_body,
                    then_value: Box::new(t_val),
                    else_body: e_body,
                    else_value: Box::new(e_val),
                },
            }
        }
        // c109 Phase 3: a struct literal. The gate already proved the type is a
        // plain covered user struct (no trait coercion, no import namespace, no
        // generic args), so the Rust head is `user_<name>` and field names mangle.
        // Field values are lowered as-is — no clone/coercion at the literal site
        // (mirrors the AST path; a value's own move/clone facts live in itself).
        Expr::StructLit {
            type_name,
            type_args,
            import_ns,
            as_trait,
            fields,
            ..
        } => {
            // c109 Phase 30: a trait-object coercion (`Circle {…}` in a `[Shape]` list). The
            // AST wraps the rendered literal `Box::new(<lit>) as Box<dyn user_<Trait>>`; the
            // value's type is the trait object. Resolved here (totality) — only the plain
            // user-struct branch below carries it (a coerced import_ns/prelude literal is
            // not a construct any covered program produces; the gate keeps those uncoerced).
            let trait_coerce = as_trait
                .as_ref()
                .map(|t| crate::Generics::user_trait_rust(t));
            // c109 Phase 19: a FOREIGN (imported user) struct literal `alias.Type { … }`
            // (`import_ns`). The AST `emit_struct_lit` `import_ns` branch emits
            // `{root}{import_mods[alias]}::{mangle(Type)}[::<args>]` with MANGLED fields.
            // Resolve the head here (totality); a missing alias falls to `user_unknown`,
            // exactly as the AST path (the gate already required the alias to resolve).
            if let Some(alias) = import_ns {
                let mod_name = cx
                    .import_mods
                    .get(alias)
                    .map(|s| s.as_str())
                    .unwrap_or("user_unknown");
                let rust_type = if type_args.is_empty() {
                    format!("{}{}::{}", cx.root_prefix, mod_name, mangle(type_name))
                } else {
                    format!(
                        "{}{}::{}::<{}>",
                        cx.root_prefix,
                        mod_name,
                        mangle(type_name),
                        type_args
                            .iter()
                            .map(|a| cx.rust_type(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                // A foreign struct's fields are never local boxed edges (boxed_edges
                // hold this module's recursive structs), so no field is boxed here.
                let tfields = fields
                    .iter()
                    .map(|(n, _, fe)| (mangle(n), lower_expr(fe, cx, env), false))
                    .collect();
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type,
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            // c109 Phase 17: a PRELUDE struct literal (HttpRequest/HttpResponse) uses the
            // `is_prelude_struct` branch of `emit_struct_lit`: a `<root>Jet…` Rust head,
            // PLAIN (unmangled) field names, and — for HttpRequest — an injected
            // `params: std::collections::BTreeMap::new()` field. Reproduce it byte-for-byte.
            if let Some(rust) = net_handle_rust_type(type_name) {
                // A prelude struct has no boxed (recursive) edges.
                let mut tfields: Vec<(String, TExpr, bool)> = fields
                    .iter()
                    .map(|(n, _, fe)| (n.clone(), lower_expr(fe, cx, env), false))
                    .collect();
                let extra = if type_name == "HttpRequest" {
                    Some("params: std::collections::BTreeMap::new()".to_string())
                } else {
                    None
                };
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type: format!("{}{}", cx.root_prefix, rust),
                        fields: tfields.drain(..).collect(),
                        extra,
                        as_trait: None,
                    },
                };
            }
            // c109 Phase 19: a GENERIC struct literal carries `type_args` (`Pair<T> {…}`).
            // The Rust head is the turbofish `user_<Name>::<args>` (`user_type_apply_rust`),
            // resolved at lowering; fields mangle. A non-generic literal renders `user_<Name>`.
            // c109: an UNqualified FOREIGN struct (`Note { … }`, no `import_ns`) prefixes its
            // module head (`{root}user_<mod>::user_<Note>`), exactly as `user_type_apply_rust`
            // — or rustc can't find the type (E0422). A local struct keeps the plain head.
            let head = match cx.foreign_types.get(type_name) {
                Some(rust_mod) => format!("{}{}::user_{}", cx.root_prefix, rust_mod, type_name),
                None => format!("user_{}", type_name),
            };
            let rust_type = if type_args.is_empty() {
                head
            } else {
                format!(
                    "{}::<{}>",
                    head,
                    type_args
                        .iter()
                        .map(|a| cx.rust_type(a))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            // c109: a self-referential field (`child: Tree?` on `Tree`) has Rust type
            // `Box<…>` (`cx.boxed_edges`); resolve the `boxed` flag here (a total fact)
            // so emit can wrap the value in `Box::new(…)`, exactly as `emit_struct_lit`.
            let tfields = fields
                .iter()
                .map(|(n, _, fe)| {
                    let boxed = cx
                        .boxed_edges
                        .contains(&(type_name.clone(), n.clone()));
                    (mangle(n), lower_expr(fe, cx, env), boxed)
                })
                .collect();
            // c109 Phase 30: a trait-coerced literal's value type is the trait object (so a
            // list of them types `[Shape]`); an uncoerced literal keeps its struct type.
            let ty = match as_trait {
                Some(t) => Type::TraitObject(t.clone()),
                None => Type::Named(type_name.clone()),
            };
            TExpr {
                ty,
                kind: TExprKind::StructLit {
                    rust_type,
                    fields: tfields,
                    extra: None,
                    as_trait: trait_coerce,
                },
            }
        }
        // c109 Phase 3: a struct field read in borrow position. Resolve the field
        // type ONCE here from the receiver's resolved struct type (totality). A
        // covered function never reaches here with a non-struct receiver (sema
        // guarantees field reads target struct values).
        Expr::Field(receiver, member, _) => {
            // c109 Phase 4: a *unit* enum literal (`Light.Yellow`) reaches codegen as
            // a `Field` whose receiver is the enum-name ident (sema re-types but does
            // not rewrite the node). The gate proved this is a covered enum + unit
            // variant; emit `user_<Enum>::user_<variant>` (the AST path's form).
            if let Expr::Ident(enum_name, _) = receiver.as_ref() {
                if env.ty_of(enum_name).is_none()
                    && cx.variant_owner.get(member).map(String::as_str)
                        == Some(enum_name.as_str())
                {
                    // c109 Phase 24: a FOREIGN enum's unit literal (`NoteType.User` in
                    // search.jet) qualifies with the module path, exactly as `emit_expr`'s
                    // `Field` arm (Expression.rs ~L232): `{root}{mod}::user_<Enum>::<V>`.
                    // Keyed on the ENUM-name (`enum_name`, the receiver) in `cx.foreign_types`,
                    // NOT the variant — matching the AST byte-for-byte.
                    let prefix = match cx.foreign_types.get(enum_name.as_str()) {
                        Some(rust_mod) => format!(
                            "{}{}::user_{}::{}",
                            cx.root_prefix,
                            rust_mod,
                            enum_name,
                            mangle(member)
                        ),
                        None => format!("user_{}::{}", enum_name, mangle(member)),
                    };
                    return TExpr {
                        ty: Type::Named(enum_name.clone()),
                        kind: TExprKind::EnumLit {
                            prefix,
                            payload: TEnumPayload::Unit,
                        },
                    };
                }
                // D-ENC-DYN1=A+: `Data.Null` → `{root}jet_std::DataTree::Null` (a unit
                // construction reaching codegen as a `Field`, the gate proved it).
                if env.ty_of(enum_name).is_none()
                    && is_json_type_name(enum_name)
                    && member == "Null"
                {
                    return TExpr {
                        ty: Type::Named(Syntax::TYPE_DATA.to_string()),
                        kind: TExprKind::JsonLit {
                            variant: "Null".to_string(),
                            arg: None,
                        },
                    };
                }
                // c109 Phase 28: a numeric BOUNDS constant (`U8.MAX`/`I32.MIN`/
                // `Float.INFINITY`/…). The gate proved the receiver is a numeric type
                // name and `member` a bounds-const name. Reproduce the AST `emit_expr`
                // Field arm (Expression.rs ~L224): `{rust_type(nt)}::{member}`. The
                // rendered Rust string is total here; the result type is the numeric
                // type itself (`U8` for `U8.MAX`, `Float` for `Float.INFINITY`).
                if env.ty_of(enum_name).is_none() {
                    if let Some(nt) = crate::AST::numeric_type_from_name(enum_name) {
                        if is_numeric_bounds_const(member) {
                            return TExpr {
                                ty: nt.clone(),
                                kind: TExprKind::ConstInline(format!(
                                    "{}::{}",
                                    cx.rust_type(&nt),
                                    member
                                )),
                            };
                        }
                    }
                }
            }
            // D-SOA1: a fused `xs[i].field` where `xs` is a columnar list reads the
            // field's column directly (`jet_index_vec(&(base).user_<field>, i, …)`),
            // the cache-friendly path — no whole-`S` gather. The result is the same
            // owned, bounds-checked field value the AoS form would produce.
            if let Expr::Index { base, index, span, kind } = receiver.as_ref() {
                if matches!(kind, IndexKind::List) {
                    let base_t = lower_expr(base, cx, env);
                    if let Type::List(elem) = &base_t.ty {
                        if cx.columnar_list_type(elem).is_some() {
                            let field_ty =
                                struct_field_type(cx, elem, member).unwrap_or(Type::Int);
                            let index_t = lower_expr(index, cx, env);
                            let line =
                                crate::Diagnostics::span_line_col(&cx.src, span.start).0;
                            return TExpr {
                                ty: field_ty,
                                kind: TExprKind::ColumnarColumnRead {
                                    base: Box::new(base_t),
                                    index: Box::new(index_t),
                                    column_rust: mangle(member).to_string(),
                                    line,
                                },
                            };
                        }
                    }
                }
            }
            let recv = lower_expr(receiver, cx, env);
            let field_ty = struct_field_type(cx, &recv.ty, member).unwrap_or(Type::Int);
            // A field of a CORE struct (`ProcessResult.code`, `JsonError.message`, …) is
            // emitted by its PLAIN Rust name, never `user_<name>` (the core structs in
            // Source/Prelude/Core.rs declare unprefixed fields — B2). Reproduce
            // `core_struct_field_rust_name` (Expression.rs) from the resolved receiver
            // type so the field read is byte-exact for both core and user structs.
            let field_rust =
                core_struct_field_rust_name(&recv.ty, member).unwrap_or_else(|| mangle(member));
            // A self-referential (recursive) edge has Rust type `Box<…>`; the read derefs
            // to the inner type (total fact from `cx.boxed_edges`, keyed on the receiver's
            // resolved struct name — mirrors the AST `boxed_field_read`).
            let boxed = match &recv.ty {
                Type::Named(n) => cx.boxed_edges.contains(&(n.clone(), member.to_string())),
                _ => false,
            };
            TExpr {
                ty: field_ty,
                kind: TExprKind::Field {
                    recv: Box::new(recv),
                    field_rust,
                    boxed,
                },
            }
        }
        // c109 Phase 4/16: an enum literal. Each payload arg carries its resolved
        // `clone`/`boxed` decisions (`emit_boxed_enum_arg`): a non-scalar payload from
        // a borrowed-in-env ident → `(…).clone()`; a recursive boxed edge →
        // `Box::new(…)`. For a scalar payload from a non-borrowed value both are false
        // (the Phase-4 no-op), so emit is byte-identical. Positional edges key on the
        // variant name; named edges on `"Variant.label"` (never a clone — matches AST).
        Expr::EnumLit { type_name, variant, args, .. } => {
            let prefix = tir_enum_lit_prefix(cx, type_name, variant);
            let payload = if args.is_empty() {
                TEnumPayload::Unit
            } else if args.iter().all(|a| matches!(a, EnumLitArg::Positional(_))) {
                let pos = args
                    .iter()
                    .map(|a| match a {
                        EnumLitArg::Positional(e) => {
                            lower_enum_arg(type_name, variant, variant, e, cx, env)
                        }
                        _ => unreachable!("all positional in this branch"),
                    })
                    .collect();
                TEnumPayload::Positional(pos)
            } else {
                // Named-payload variant: each field carries its mangled Rust name.
                let named = args
                    .iter()
                    .map(|a| match a {
                        EnumLitArg::Named { label, expr } => {
                            let edge = format!("{}.{}", variant, label);
                            (
                                mangle(label),
                                lower_enum_arg(type_name, variant, &edge, expr, cx, env),
                            )
                        }
                        // A positional arg mixed with named is a sema error that
                        // never reaches a covered function; default to a field.
                        EnumLitArg::Positional(e) => (
                            String::new(),
                            lower_enum_arg(type_name, variant, variant, e, cx, env),
                        ),
                    })
                    .collect();
                TEnumPayload::Named(named)
            };
            TExpr {
                ty: Type::Named(type_name.clone()),
                kind: TExprKind::EnumLit { prefix, payload },
            }
        }
        // c109 Phase 5: a list literal. Lowers each element as-is (mirrors the AST
        // `vec![…]` form — no clone/coercion at the literal site). The result type
        // is `[E]` with `E` taken from the first element; an empty `[]` has no
        // element to read, so its element type is unresolved (`Int` placeholder),
        // but the emitted `vec![]` is type-inferred by Rust from the binding context.
        Expr::ListLit(elems, _) => {
            let telems: Vec<TExpr> = elems.iter().map(|e| lower_expr(e, cx, env)).collect();
            let elem_ty = telems.first().map(|e| e.ty.clone()).unwrap_or(Type::Int);
            // D-SOA1: a list of a columnar struct builds via `from_aos`.
            if let Some(columns_ty) = cx.columnar_list_type(&elem_ty) {
                return TExpr {
                    ty: Type::List(Box::new(elem_ty)),
                    kind: TExprKind::ColumnarListLit { columns_ty, elems: telems },
                };
            }
            TExpr {
                ty: Type::List(Box::new(elem_ty)),
                kind: TExprKind::ListLit(telems),
            }
        }
        // c109 Phase 23: a named-tuple literal → a generated `JetTup_<hash>` struct
        // literal. The gate guaranteed `ty` is `Some(Type::Tuple)`. Reproduce
        // `emit_expr`'s `TupleLit` arm: the CANONICAL field order + struct name come
        // from the type; each canonical field's value is taken from the literal (by
        // name) and lowered. Fields are emitted as `user_<f>: <v>` in canonical order.
        Expr::TupleLit(lit_fields, _, ty) => {
            let canonical = match ty {
                Some(Type::Tuple(fs)) => tuple_fields_plain(fs),
                _ => Vec::new(),
            };
            let struct_name = tuple_struct_name(&canonical);
            // Map field-name → its literal value expr (the literal may list fields in
            // any order; the type fixes the canonical order — exactly the AST path).
            let mut value_of: std::collections::HashMap<&str, &Expr> =
                std::collections::HashMap::new();
            for (n, e) in lit_fields {
                value_of.insert(n.as_str(), e);
            }
            let fields: Vec<(String, TExpr)> = canonical
                .iter()
                .map(|(n, fty)| {
                    let v = match value_of.get(n.as_str()) {
                        Some(e) => lower_expr(e, cx, env),
                        // A missing field never occurs in a sema-checked tuple literal;
                        // mirror the AST's `0i64` default defensively (an Int literal).
                        None => TExpr {
                            ty: fty.clone(),
                            kind: TExprKind::IntLit(0, None),
                        },
                    };
                    (mangle(n).to_string(), v)
                })
                .collect();
            TExpr {
                ty: ty.clone().unwrap_or_else(|| Type::Tuple(Vec::new())),
                kind: TExprKind::TupleLit {
                    struct_name,
                    fields,
                },
            }
        }
        // c109 Phase 5: a map literal `[k: v, …]` / `[:]`. Keys/values lower as-is;
        // the result type is `[K, V]` from the first entry (empty `[:]` → unresolved
        // placeholder, type-inferred by Rust from context like `vec![]`).
        Expr::MapLit(entries, _) => {
            let tentries: Vec<(TExpr, TExpr)> = entries
                .iter()
                .map(|(k, v)| (lower_expr(k, cx, env), lower_expr(v, cx, env)))
                .collect();
            let (kt, vt) = tentries
                .first()
                .map(|(k, v)| (k.ty.clone(), v.ty.clone()))
                .unwrap_or((Type::String, Type::Int));
            TExpr {
                ty: Type::Map {
                    key: Box::new(kt),
                    value: Box::new(vt),
                },
                kind: TExprKind::MapLit(tentries),
            }
        }
        // c109 Phase 5: indexing `coll[i]`. The `IndexKind` (List/Map) is the total
        // sema fact (`is_map`); the helper line is resolved at lowering. The result
        // type is the list element / map value type, read from the base's resolved
        // type (totality) — never re-inferred in emit.
        Expr::Index { base, index, span, kind } => {
            let base_t = lower_expr(base, cx, env);
            let index_t = lower_expr(index, cx, env);
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            // D-SIMD2: `v[i]` lane access on a SIMD lane type → a bounds-checked lane
            // read. The result is the lane scalar; sema resolved `IndexKind::Lane`.
            if let IndexKind::Lane(lane_ty) = kind {
                return TExpr {
                    ty: crate::Sema::math_scalar_ty(lane_ty),
                    kind: TExprKind::MathLaneIndex {
                        lane_ty: lane_ty.clone(),
                        base: Box::new(base_t),
                        index: Box::new(index_t),
                        line: line as u32,
                    },
                };
            }
            let result_ty = match &base_t.ty {
                Type::List(elem) => (**elem).clone(),
                Type::Map { value, .. } => (**value).clone(),
                Type::FixedList { elem, .. } => (**elem).clone(),
                _ => Type::Int,
            };
            // D-SOA1: `xs[i]` on a columnar list gathers the logical `S` from the
            // columns. (A fused `xs[i].field` is handled in the `Field` arm before
            // this point — that path reads a single column directly.)
            if let Type::List(elem) = &base_t.ty {
                if cx.columnar_list_type(elem).is_some() {
                    return TExpr {
                        ty: result_ty,
                        kind: TExprKind::ColumnarGather {
                            base: Box::new(base_t),
                            index: Box::new(index_t),
                            line,
                        },
                    };
                }
            }
            TExpr {
                ty: result_ty,
                kind: TExprKind::Index {
                    base: Box::new(base_t),
                    index: Box::new(index_t),
                    is_map: matches!(kind, IndexKind::Map),
                    line,
                },
            }
        }
        // c109 Phase 5: an inclusive copy slice `coll[a..b]` (lists). Lowers to the
        // `jet_slice_vec` helper; the result is a list of the same element type.
        Expr::Slice { base, start, end, span } => {
            let base_t = lower_expr(base, cx, env);
            let start_t = lower_expr(start, cx, env);
            let end_t = lower_expr(end, cx, env);
            let result_ty = base_t.ty.clone();
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: result_ty,
                kind: TExprKind::Slice {
                    base: Box::new(base_t),
                    start: Box::new(start_t),
                    end: Box::new(end_t),
                    line,
                },
            }
        }
        // D-TAINT1: `#Tainted expr` — the value-fact tag is **erased in codegen**
        // (I3). Lower the inner expression unchanged; taint exists only as a
        // compile-time sema proof, never a runtime value.
        Expr::Tainted(inner, _) => lower_expr(inner, cx, env),
        // c109 Phase 8: `value(x)` → `Some(x)`. The result type is `T?` where `T` is
        // the inner's resolved type (totality). Mirrors `Expr::Present`.
        Expr::Present(inner, _) => {
            let t = lower_expr(inner, cx, env);
            TExpr {
                ty: Type::Option(Box::new(t.ty.clone())),
                kind: TExprKind::Present(Box::new(t)),
            }
        }
        // c109 Phase 8: bare `null` → `None`. The element type is unresolved here
        // (`Int` placeholder) — like an empty `vec![]`, Rust infers it from the
        // binding/return context. Mirrors `Expr::Absent`.
        Expr::Absent(_) => TExpr {
            ty: Type::Option(Box::new(Type::Int)),
            kind: TExprKind::Absent,
        },
        // c109 Phase 23: a `#Todo` typed hole → diverging `todo!(…)`. The expected-type
        // STRING is the total sema fact (gate guarantees `Some`); the source line is
        // resolved here. The result `ty` is never load-bearing (a `todo!()` diverges and
        // is never an arithmetic operand), so a placeholder suffices — the emitted Rust
        // reads only `expected_type`/`line`/`cx.file`, byte-for-byte Expression.rs.
        Expr::Todo { span, expected_type } => {
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: Type::Named("Unit".to_string()),
                kind: TExprKind::Todo {
                    line,
                    expected_type: expected_type.clone().unwrap_or_else(|| "(unknown)".to_string()),
                },
            }
        }
        // c109 Phase 8: `ok(x)` → `Ok(x)`. The result is a `Result` whose ok type is
        // the inner's; the err type is unresolved here (Rust infers it from the
        // function return context, exactly as the AST path's bare `Ok(x)` does).
        Expr::Ok(inner, _) => {
            let t = lower_expr(inner, cx, env);
            TExpr {
                ty: Type::Result {
                    ok: Box::new(t.ty.clone()),
                    err: Box::new(Type::Named("Error".to_string())),
                },
                kind: TExprKind::Ok(Box::new(t)),
            }
        }
        // c109 Phase 8: `err(e)` → `Err(e)`. The err type is the inner's; the ok type
        // is unresolved here (inferred from the function return context).
        Expr::Err(inner, _) => {
            let t = lower_expr(inner, cx, env);
            TExpr {
                ty: Type::Result {
                    ok: Box::new(Type::Int),
                    err: Box::new(t.ty.clone()),
                },
                kind: TExprKind::Err(Box::new(t)),
            }
        }
        // c109 Phase 8: the `?` propagation operator. The `TryConvert` decision is the
        // total sema fact — reproduce it exactly (none/Fallible/Typed). The result
        // type is the inner `Result`'s ok type (the `?` unwraps it). The trace-frame
        // location is resolved here so emit never reads `cx.current_fn`/`cx.src`.
        Expr::Try(inner, span, convert) => {
            let inner_t = lower_expr(inner, cx, env);
            // `?` unwraps a `Result<T, E>` to `T` (the value type). If the inner type
            // resolved to a Result, take its ok type; else fall back to the inner type
            // (never load-bearing in the covered subset — a `?` result feeds a binding
            // carrying sema's `b.ty`, or an `ok(...)` wrap whose own type is total).
            let result_ty = match &inner_t.ty {
                Type::Result { ok, .. } => (**ok).clone(),
                other => other.clone(),
            };
            let tconvert = match convert {
                TryConvert::None => TTryConvert::None,
                TryConvert::Fallible => TTryConvert::Fallible,
                TryConvert::Typed(fn_name) => TTryConvert::Typed(fn_name.clone()),
            };
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: result_ty,
                kind: TExprKind::Try {
                    inner: Box::new(inner_t),
                    convert: tconvert,
                    file: escape_rust_str(&cx.file),
                    line,
                    fn_name: escape_rust_str(&env.fn_name),
                },
            }
        }
        // c109 Phase 8: the `??` fallback operator. `is_option` is the total sema fact
        // (Result vs Option). The value + fallback are lowered; the result type is the
        // unwrapped value type (Some/Ok payload). Mirrors `emit_or_fallback`.
        Expr::OrFallback { value, fallback, is_option, .. } => {
            let value_t = lower_expr(value, cx, env);
            let result_ty = match &value_t.ty {
                Type::Option(inner) => (**inner).clone(),
                Type::Result { ok, .. } => (**ok).clone(),
                other => other.clone(),
            };
            let tfallback = match fallback {
                OrFallback::Value(e) => TOrFallback::Value(Box::new(lower_expr(e, cx, env))),
                OrFallback::Return(None, _) => TOrFallback::Return(None),
                OrFallback::Return(Some(e), _) => {
                    TOrFallback::Return(Some(Box::new(lower_expr(e, cx, env))))
                }
                // c109 Phase 15: the `panic(…)` form — render the whole
                // `{ jet_panic_rich(…); }` statement string at lowering, byte-for-byte
                // `emit_panic_stop`/`safe_locals_expr`, so emit reads nothing from
                // `cx.src`/`cx.current_fn`.
                OrFallback::Panic { name_span, args } => {
                    TOrFallback::Panic(render_panic_stop(name_span, args, cx, env))
                }
            };
            TExpr {
                ty: result_ty,
                kind: TExprKind::OrFallback {
                    value: Box::new(value_t),
                    fallback: tfallback,
                    is_option: *is_option,
                },
            }
        }
        // c109 Phase 8: optional chaining `base?.member`. The `flatten` fact is total
        // (from sema): true → `.and_then`, false → `.map`. The result type is `T?`;
        // resolving the inner field type here is not load-bearing (emit only formats
        // the combinator + member access), so carry the base's optional type.
        Expr::OptField { base, member, flatten, .. } => {
            let base_t = lower_expr(base, cx, env);
            TExpr {
                ty: base_t.ty.clone(),
                kind: TExprKind::OptField {
                    base: Box::new(base_t),
                    member_rust: mangle(member),
                    flatten: *flatten,
                },
            }
        }
        // c109 Phase 11: a lambda/closure literal. The gate proved the body is
        // in-subset; lower it via `lower_lambda` (capture/escape facts total from
        // `Lambda.meta`). The result type is the closure's fn type — rarely
        // load-bearing in emit (a closure is consumed in arg position), so carry a
        // placeholder `Fn` type; the binding/arg context supplies the real Rust type.
        Expr::Lambda(lam) => {
            let tl = lower_lambda(lam, cx, env);
            TExpr {
                ty: Type::Fn {
                    params: Vec::new(),
                    ret: None,
                    effect_bound: None,
                },
                kind: TExprKind::Lambda(Box::new(tl)),
            }
        }
        // c109 Phase 11: fan-out `f.[a, b, c]` (S75/S76). The gate proved the callee
        // is a plain top-level fn ident and every item is in-subset. The AST path
        // routes the Ident callee through `emit_call` with a SYNTHETIC single-arg
        // `Call` (`convention: Read`, default flags) per item; reproduce that exactly
        // as a `TExprKind::Call` per item, then `vec![…]`. The result type is `[T#N]`
        // (S76), erased to a list of the callee's return type.
        Expr::FanOut { callee, items, .. } => {
            let Expr::Ident(name, _) = callee.as_ref() else {
                unreachable!("gate proved fan-out callee is a plain fn ident");
            };
            // The callee's signature drives each synthetic arg's borrow wrapper,
            // exactly as `emit_call_args` does for the synthetic `Read` arg (whose
            // `implicit_clone` is false — the synthetic CallArg carries default flags).
            let sig = cx.sigs.get(name);
            let borrow = matches!(
                sig.and_then(|ps| ps.first()),
                // D-CAP8/9: Infer/Share borrow like Read (see lower_one_call_arg).
                Some((
                    AccessConvention::Read | AccessConvention::Infer | AccessConvention::Share,
                    t,
                )) if !t.is_scalar()
            );
            let calls: Vec<TExpr> = items
                .iter()
                .map(|item| {
                    let value = lower_expr(item, cx, env);
                    TExpr {
                        ty: call_return_type(cx, name),
                        kind: TExprKind::Call {
                            name: name.clone(),
                            args: vec![TCallArg {
                                value,
                                borrow,
                                mut_borrow: false,
                                clone: false,
                                arc_clone: false,
                                fn_coerce: None,
                                widen_to_vec: false,
                            }],
                        },
                    }
                })
                .collect();
            // D-FIXARR1: fan-out result is `[T#N]` — a real Rust stack array.
            let elem_ty = call_return_type(cx, name);
            let len = items.len() as u64;
            TExpr {
                ty: Type::FixedList { elem: Box::new(elem_ty), len },
                kind: TExprKind::FanOut { calls },
            }
        }
        // c109 Phase 18: `mem.Ptr<T>.from_addr(addr)` (S58). The result type is
        // `Ptr<elem>` (`ptr_type`), total from the node's `elem`. The element's Rust type
        // is resolved here (`cx.rust_type`) so emit makes no decision (I3). The cast is
        // safe Rust (no `unsafe`).
        Expr::PtrFromAddr { elem, addr, .. } => {
            let taddr = lower_expr(addr, cx, env);
            TExpr {
                ty: crate::Sema::ptr_type(elem.clone()),
                kind: TExprKind::PtrFromAddr {
                    elem_rust: cx.rust_type(elem),
                    addr: Box::new(taddr),
                },
            }
        }
        _ => unreachable!("expression not in TIR subset"),
    }
}


/// Replay codegen's `operand_is_integer` (Codegen/Expression.rs) on an AST
/// operand, using the lowering env for identifier types. The result MUST match
/// that function bit-for-bit so the TIR's overflow-trap decision is identical to
/// the AST path's. Like the original: literals/negation/nested-arithmetic-left
/// resolve structurally; an `Ident` resolves via its slot type; everything else
/// (notably a struct-field read) is unresolved (`None`) and so never traps.
pub(crate) fn ast_operand_is_integer(e: &Expr, env: &LowerEnv) -> Option<bool> {
    match e {
        Expr::Int(..) => Some(true),
        Expr::Float(..) => Some(false),
        Expr::Unary(UnOp::Neg, inner, _) => ast_operand_is_integer(inner, env),
        Expr::Binary(_, l, _, _) => ast_operand_is_integer(l, env),
        // Mirror `expr_jet_ty`: only `Ident`/`Str`/`Char` resolve here. A `Field`
        // (and anything else) resolves to `None` — exactly as the AST path does,
        // so a field operand never enables the overflow trap.
        Expr::Ident(name, _) => env.ty_of(name).map(|t| t.is_integer()),
        Expr::Str(..) => Some(false),
        Expr::Char(..) => Some(false),
        _ => None,
    }
}

/// c109 Phase 15: the PLAIN Rust field name for a CORE-struct field read, mirroring
/// `core_struct_field_rust_name` (Source/Codegen/Expression.rs) — but keyed on the
/// RESOLVED receiver type (the TIR's total `recv.ty`) instead of `expr_jet_ty(env)`.
/// Returns `Some(plain_name)` for a known core-struct field (so it is emitted
/// unprefixed, B2), `None` otherwise (the caller falls back to `mangle(member)`).
pub(crate) fn core_struct_field_rust_name(recv_ty: &Type, member: &str) -> Option<String> {
    let Type::Named(type_name) = recv_ty else {
        return None;
    };
    let known = match type_name.as_str() {
        "ProcessResult" => matches!(member, "code" | "output" | "errors"),
        n if n == Syntax::TYPE_JSON_ERROR || n == "JsonError" => {
            matches!(member, "line" | "message")
        }
        n if n == Syntax::TYPE_UTF8_ERROR || n == "Utf8Error" => member == "message",
        // D-LSDIR1=A: DirEntry fields — name (bare filename), path (full path), is_dir.
        "DirEntry" => matches!(member, "name" | "path" | "is_dir"),
        // E2-M10: HttpRequest / HttpResponse field access.
        "HttpRequest" | "HttpResponse" => {
            matches!(member, "method" | "path" | "body" | "headers" | "status")
        }
        _ => false,
    };
    if known {
        Some(member.to_string())
    } else {
        None
    }
}

/// Look up a field's declared type on a resolved struct receiver type. Returns
/// `None` when the receiver is not a known struct or the field is absent — both
/// impossible for a covered function (sema validated the access).
pub(crate) fn struct_field_type(cx: &Cx, recv_ty: &Type, field: &str) -> Option<Type> {
    // c109 Phase 23: a named-tuple field read (`p.x`) — resolve the field's type off
    // the `Type::Tuple` directly (a tuple has no `cx.struct_fields` entry; its struct
    // is the generated `JetTup_<hash>`). Keeps the field read's result type total.
    if let Type::Tuple(fields) = recv_ty {
        return fields
            .iter()
            .find(|(f, _)| f == field)
            .map(|(_, t)| (**t).clone());
    }
    let Type::Named(name) = recv_ty else {
        return None;
    };
    cx.struct_fields
        .get(name)?
        .iter()
        .find(|(f, _)| f == field)
        .map(|(_, t)| t.clone())
}

/// The type of an integer literal given its elaborated width.
pub(crate) fn int_lit_type(width: &Option<(bool, u8)>) -> Type {
    match width {
        Some((signed, bits)) => Type::IntN {
            signed: *signed,
            bits: *bits,
        },
        None => Type::Int,
    }
}

pub(crate) fn unit_type() -> Type {
    Type::Named("Unit".to_string())
}

/// The resolved return type of a called plain function: its declared return
/// type if known, else `Unit`. (In the subset, callees return scalar/String/Unit.)
/// Read from `cx.fn_types`, which sema-built `Type::Fn { ret, .. }` per function.
pub(crate) fn call_return_type(cx: &Cx, name: &str) -> Type {
    match cx.fn_types.get(name) {
        Some(Type::Fn { ret: Some(r), .. }) => (**r).clone(),
        // c109 Phase 23: a distinct-type constructor `UserId(x)` yields the distinct
        // type itself (it has no `fn_types` entry). Keeps the call's result type total.
        _ if cx.distinct_types.contains_key(name) => Type::Named(name.to_string()),
        _ => unit_type(),
    }
}

/// c109 Phase 6: lower a method call. The gate proved it is the synthetic `.clone()`
/// or a user instance method on a covered type; resolve every dispatch fact here.
pub(crate) fn lower_method_call(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    resolved_ret: Option<&Type>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    // D-TXN3/D-TXN4: `<handle>.on_commit(() => { … })` on a `#Transact` handle.
    // The gate proved `recv_type == Some("Transaction")` and a single literal
    // zero-param lambda arg. Lower to `<handle>.on_commit(Box::new(move || { … }))`;
    // the Drop-backed LIFO-on-commit semantics live in the `JetTransaction` prelude
    // type. The receiver is the bound handle ident → its mangled Rust place.
    if method == Syntax::TXN_ON_COMMIT
        && recv_type.as_deref() == Some(Syntax::TXN_HANDLE_TYPE)
    {
        // The handle is always a bound ident (sema typed it `Transaction` from a
        // `#Transact(name)` binding); its mangled place is `user_<name>`.
        let handle = match receiver {
            Expr::Ident(name, _) => mangle(name),
            // Defensive: a non-ident receiver can't be a transaction handle, but
            // lowering it keeps the place well-formed if one ever appears.
            other => emit_tir_expr(&lower_expr(other, cx, env), cx),
        };
        if let Some(crate::AST::Expr::Lambda(lam)) = args.first().map(|a| &a.expr) {
            // Build the closure directly (not via `render_lambda_str`, which may add
            // its own `Box::new(…)` wrapper). The hook is stored in the transaction's
            // `Vec<Box<dyn FnOnce()>>`, so it must be a `move` closure boxed exactly
            // once by the `TCoreClosureKind::OnCommit` emit (no double-box).
            let tl = lower_lambda(lam, cx, env);
            let inner = format!("move |{}| {}", tl.params.join(", "), tl.body);
            let closure = if tl.prep.is_empty() {
                inner
            } else {
                format!("{{ {} {} }}", tl.prep, inner)
            };
            return TExpr {
                ty: Type::Named("TransactionGuard".to_string()),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::OnCommit { handle, closure },
                },
            };
        }
    }
    // D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(() => { … })` on a `#Transact`
    // handle — the exact mirror of `on_commit`. Lower to
    // `<handle>.on_rollback(Box::new(move || { … }))`; the Drop-backed run-on-rollback
    // semantics live in the `JetTransaction` prelude type.
    if method == Syntax::TXN_ON_ROLLBACK
        && recv_type.as_deref() == Some(Syntax::TXN_HANDLE_TYPE)
    {
        let handle = match receiver {
            Expr::Ident(name, _) => mangle(name),
            other => emit_tir_expr(&lower_expr(other, cx, env), cx),
        };
        if let Some(crate::AST::Expr::Lambda(lam)) = args.first().map(|a| &a.expr) {
            let tl = lower_lambda(lam, cx, env);
            let inner = format!("move |{}| {}", tl.params.join(", "), tl.body);
            let closure = if tl.prep.is_empty() {
                inner
            } else {
                format!("{{ {} {} }}", tl.prep, inner)
            };
            return TExpr {
                ty: Type::Named("TransactionGuard".to_string()),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::OnRollback { handle, closure },
                },
            };
        }
    }
    // The sema-inserted `.clone()`: emit `(recv).clone()`, result is the receiver's
    // type (a clone preserves it). Mirrors `emit_method_call`'s `clone` early return.
    if method == "clone" {
        let recv = lower_expr(receiver, cx, env);
        let ty = recv.ty.clone();
        return TExpr {
            ty,
            kind: TExprKind::Clone(Box::new(recv)),
        };
    }
    // c109 Phase 23: `.raw()` on a distinct type → `({recv}).0`. The receiver's resolved
    // type names the distinct; its base type (from `cx.distinct_types`) is the total
    // result type. Mirrors `emit_method_call`'s `METHOD_DISTINCT_RAW` early return.
    if method == Syntax::METHOD_DISTINCT_RAW {
        let recv = lower_expr(receiver, cx, env);
        let base = match &recv.ty {
            Type::Named(n) => cx
                .distinct_types
                .get(n)
                .map(|(b, _)| b.clone())
                .unwrap_or_else(unit_type),
            _ => unit_type(),
        };
        return TExpr {
            ty: base,
            kind: TExprKind::DistinctRaw(Box::new(recv)),
        };
    }
    // c109 Phase 27: a CALL THROUGH a fn-typed struct field — `w.step(4)`. The gate
    // proved `recv_type == Some(<CoveredStruct>)` and the named field is `Type::Fn`. The
    // AST `emit_method_call` (Expression.rs ~L1573) emits `(({recv}).{user_<field>})({args})`
    // with PLAIN args. Resolve the field's Rust name + the call's result type (the Fn's
    // return) here; emit just splices. (Tried before the JSON/core/user shapes, mirroring
    // the AST dispatch order — a fn-field check fires before user-method dispatch.)
    if let Some(Type::Fn { ret, .. }) = fn_field_call_ty(method, recv_type, cx) {
        let ret_ty = ret.as_deref().cloned().unwrap_or_else(unit_type);
        let recv = lower_expr(receiver, cx, env);
        // Args emit PLAINLY (AST `emit_call_args(.., None, ..)` — no convention), but the
        // arg's own `implicit_clone`/`shared_auto_clone` flags still apply: pass `conv:
        // None` to `lower_one_call_arg`, the single `emit_call_args` reproduction.
        let targs: Vec<TCallArg> =
            args.iter().map(|a| lower_one_call_arg(a, None, env, cx)).collect();
        return TExpr {
            ty: ret_ty,
            kind: TExprKind::FnFieldCall {
                recv: Box::new(recv),
                field_rust: mangle(method),
                args: targs,
            },
        };
    }
    // D-ENC-DYN1=A+: a dynamic `Data` construction `Data.<Variant>(arg)` (the gate
    // proved the receiver is a `Data`/`Json`/… type-name ident and `method` is a `Data`
    // variant). Lower to `TExprKind::JsonLit`, carrying the payload's `implicit_clone`
    // flag as a total fact. The result type is `Data`.
    if let Expr::Ident(type_name, _) = receiver {
        if !env.locals.contains_key(type_name)
            && is_json_type_name(type_name)
            && is_json_variant(method)
        {
            let arg = args.first().map(|a| {
                Box::new((lower_expr(&a.expr, cx, env), a.flags.implicit_clone))
            });
            return TExpr {
                ty: Type::Named(Syntax::TYPE_DATA.to_string()),
                kind: TExprKind::JsonLit {
                    variant: method.to_string(),
                    arg,
                },
            };
        }
    }
    // c109 Phase 19: the arena allocator constructor `mem.Arena.new(…)` (D-ALLOC1). The
    // gate proved the receiver is `Field(Ident(mem-alias), <AllocType>)` + method `new`.
    // Render the whole ctor call HERE (totality), reproducing `emit_method_call`'s arena
    // branch (Expression.rs ~L1515): `jet_mem::Jet<Alloc>::new()` (no arg) or
    // `::with_capacity|with_slots|with_size((arg) as usize)` (one optional arg). The
    // result type is the allocator handle `Named(<AllocType>)` (`alloc_method_return`'s
    // `new` arm). The allocator's only `unsafe` lives in the vetted `jet_mem` prelude (I1).
    {
        let locals: HashSet<String> = env.locals.keys().cloned().collect();
        {
            if let Some(alloc_type) = alloc_new_type(receiver, method, cx, &locals) {
                let rust_type = alloc_handle_rust_type(alloc_type).unwrap_or("jet_mem::JetArena");
                let ctor = if args.is_empty() {
                    format!("{}::new()", rust_type)
                } else {
                    let ctor_fn = match alloc_type {
                        "Pool" => "with_slots",
                        "Fixed" => "with_size",
                        _ => "with_capacity",
                    };
                    let a0 = emit_tir_expr(&lower_expr(&args[0].expr, cx, env), cx);
                    format!("{}::{}({} as usize)", rust_type, ctor_fn, a0)
                };
                return TExpr {
                    ty: Type::Named(alloc_type.to_string()),
                    kind: TExprKind::AllocNew { ctor },
                };
            }
        }
    }
    // c109 Phase 16: an enum-variant CONSTRUCTION `Enum.Variant(args)` reaching codegen
    // as a `MethodCall` (sema never rewrites a payload variant to `Expr::EnumLit`). The
    // AST `emit_method_call` routes it to `emit_enum_lit` with all-positional args; we
    // reproduce that, resolving each arg's `clone`/`boxed` decisions via `lower_enum_arg`
    // (`emit_boxed_enum_arg` byte-for-byte). This is the construction half of the
    // string/struct/collection-payload + recursive (boxed) enum coverage.
    if recv_type.is_none() {
        if let Expr::Ident(type_name, _) = receiver {
            if !env.locals.contains_key(type_name) {
                // D-TERM1 (ratified 2026-06-22): `Key` is a prelude enum not in
                // `cx.enum_variants`; lower `Key.Variant(args)` to a TIR enum lit.
                let key_type = crate::Syntax::TYPE_KEY;
                if type_name == key_type && is_key_variant(method) {
                    let prefix = tir_enum_lit_prefix(cx, type_name, method);
                    let payload = if args.is_empty() {
                        TEnumPayload::Unit
                    } else {
                        let pos = args
                            .iter()
                            .map(|a| {
                                // Key payload args are always scalar/Char — no clone/box needed.
                                TEnumArg { value: lower_expr(&a.expr, cx, env), clone: false, boxed: false }
                            })
                            .collect();
                        TEnumPayload::Positional(pos)
                    };
                    return TExpr {
                        ty: Type::Named(type_name.clone()),
                        kind: TExprKind::EnumLit { prefix, payload },
                    };
                }
                if let Some(variants) = cx.enum_variants.get(type_name) {
                    if variants.iter().any(|(v, _)| v == method) {
                        let prefix = tir_enum_lit_prefix(cx, type_name, method);
                        let payload = if args.is_empty() {
                            TEnumPayload::Unit
                        } else {
                            let pos = args
                                .iter()
                                .map(|a| {
                                    lower_enum_arg(type_name, method, method, &a.expr, cx, env)
                                })
                                .collect();
                            TEnumPayload::Positional(pos)
                        };
                        return TExpr {
                            ty: Type::Named(type_name.clone()),
                            kind: TExprKind::EnumLit { prefix, payload },
                        };
                    }
                }
            }
        }
    }
    // c109 Phase 10: a core/stdlib module call `alias.method(args)`. The gate proved
    // `recv_type == None` + receiver is a core-import alias + `core_call_covered`.
    // Mirror `emit_core_call` (Source/Codegen/Expression.rs): resolve the module here
    // (total), lower args PLAINLY (no clone/borrow wrappers — `emit_core_call`'s
    // `arg(i)` is a raw `emit_expr`), and carry the return type from the authoritative
    // `core_fixed_sig` table. Tried BEFORE the builtin shape (a core method named
    // `get`/`split`/… must not be claimed by the receiver-keyed builtin op).
    if recv_type.is_none() {
        // D-ENC1: nested-namespace core call `<alias>.<leaf>.method(args)`. The subset
        // gate admitted it; resolve the submodule and build a plain `CoreCall` (the
        // encoding calls are all monomorphic, so the type comes from `core_fixed_sig`).
        if let Expr::Field(base, leaf, _) = receiver {
            if let Expr::Ident(alias, _) = &**base {
                if !env.locals.contains_key(alias) {
                    if let Some(ns) = cx.core_imports.get(alias).cloned() {
                        let submodule = format!("{}.{}", ns, leaf);
                        if crate::Loader::is_known_core_module(&submodule) {
                            let targs: Vec<TExpr> =
                                args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
                            return TExpr {
                                ty: core_call_return_ty(&submodule, method),
                                kind: TExprKind::CoreCall {
                                    module: submodule,
                                    method: method.to_string(),
                                    args: targs,
                                },
                            };
                        }
                    }
                }
            }
        }
        if let Expr::Ident(alias, _) = receiver {
            if !env.locals.contains_key(alias) {
                if let Some(module) = cx.core_imports.get(alias).cloned() {
                    // c109 Phase 13: a closure-taking core call (spawn/serve/guard).
                    // The gate proved a literal-lambda closure arg. Each renders its
                    // bespoke shape at lowering (lambda in subset — Phase 11).
                    if let Some(t) = lower_core_closure_call(&module, method, args, cx, env) {
                        return t;
                    }
                    let targs: Vec<TExpr> =
                        args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
                    // c109 Phase 18: the `core.mem` pointer ops carry a non-fixed return
                    // type. `address_of` is always `Int`; `volatile_read(p)` reads through
                    // the typed pointer, so its result is `ptr_elem(p.ty)` — the `T` of the
                    // `Ptr<T>` arg, recovered from the LOWERED arg's total `ty` (no emit-time
                    // inference, I3). A defensive `Unit` fallback (an ill-typed arg sema
                    // would already have rejected) keeps the fact total.
                    let ty = if module == "core.mem" {
                        match method {
                            "address_of" => Type::Int,
                            "volatile_read" => targs
                                .first()
                                .and_then(|a| crate::Sema::ptr_elem(&a.ty))
                                .unwrap_or_else(unit_type),
                            _ => core_call_return_ty(&module, method),
                        }
                    } else if crate::Sema::is_polymorphic_core_special(&module, method) {
                        // c109 Phase 20: the polymorphic special's return type is NOT in
                        // `core_fixed_sig` — sema resolved it (arg-type dependent) and wrote
                        // it onto the node's `resolved_ret`. Read it totally (I3); a unit
                        // fallback (eprint/shuffle return nothing) keeps the fact total.
                        resolved_ret.cloned().unwrap_or_else(unit_type)
                    } else {
                        core_call_return_ty(&module, method)
                    };
                    return TExpr {
                        ty,
                        kind: TExprKind::CoreCall {
                            module,
                            method: method.to_string(),
                            args: targs,
                        },
                    };
                }
                // c109 Phase 14: a qualified cross-module call `alias.method(args)`.
                // The gate proved the alias is a re-export / import_mod / code_module.
                // Mirror `emit_method_call`'s arms IN ORDER (reexport, import_mods,
                // code_modules) — resolving the path pieces here so emit decides nothing.
                if let Some((real_mod, real_fn)) =
                    cx.reexport_calls.get(&(alias.clone(), method.to_string())).cloned()
                {
                    let sig = cx
                        .import_sigs
                        .get(&(alias.clone(), method.to_string()))
                        .cloned();
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    let ret = cx
                        .import_rets
                        .get(&(alias.clone(), method.to_string()))
                        .cloned()
                        .flatten()
                        .unwrap_or_else(unit_type);
                    return TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod: real_mod,
                                rust_fn: mangle(&real_fn).to_string(),
                            },
                            args: targs,
                        },
                    };
                }
                if let Some(mod_name) = cx.import_mods.get(alias).cloned() {
                    let sig = cx
                        .import_sigs
                        .get(&(alias.clone(), method.to_string()))
                        .cloned();
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    let ret = cx
                        .import_rets
                        .get(&(alias.clone(), method.to_string()))
                        .cloned()
                        .flatten()
                        .unwrap_or_else(unit_type);
                    return TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod: mod_name,
                                rust_fn: mangle(method).to_string(),
                            },
                            args: targs,
                        },
                    };
                }
                if cx.code_modules.contains(alias.as_str()) {
                    let mangled_key = format!("{}__{}", alias, method);
                    let sig = cx.sigs.get(&mangled_key).cloned();
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    return TExpr {
                        ty: call_return_type(cx, &mangled_key),
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::InlineMangled { mangled: mangled_key },
                            args: targs,
                        },
                    };
                }
            }
        }
    }
    // c109 Phase 9: a built-in collection/string method (`emit_builtin_method`). The
    // gate proved `recv_type == None` + a covered builtin name + an in-subset value
    // receiver. Resolve the Map-vs-List-vs-String emit branch HERE from the
    // receiver's type (reproducing `expr_jet_ty`, incl. its `None` partiality), so
    // emit makes no type decision (I3). The result type comes from the builtin's
    // sema return (`Collections::builtin_method_return`) for totality.
    if recv_type.is_none() {
        if let Some(op) = resolve_builtin_op(receiver, method, method_span, args, env, cx) {
            let recv_t = lower_expr(receiver, cx, env);
            let recv_ast_ty = tir_recv_jet_ty(receiver, env);
            let result_ty = builtin_result_ty(method, args.len(), recv_ast_ty.as_ref());
            // Args are emitted plainly (no clone/borrow wrappers), exactly as
            // `emit_builtin_method`'s `arg(i)` = raw `emit_expr`.
            let targs = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
            return TExpr {
                ty: result_ty,
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(recv_t),
                    op,
                    args: targs,
                },
            };
        }
    }
    // c109 Phase 19: `Stopwatch.elapsed_millis()` (gate shape d2). The gate proved
    // `recv_type == None` + the `elapsed_millis` name + an in-subset value receiver.
    // Lower to the existing `THandleOp::StopwatchElapsedMillis` (`{root}jet_stopwatch_
    // elapsed_millis(&(recv))`), the same node the Phase-13 handle shape uses — emit is
    // byte-identical to `emit_builtin_method`'s name-keyed `elapsed_millis` arm. The
    // result type is `Int` (`stopwatch_method_return`), kept total per the design.
    if recv_type.is_none() && method == "elapsed_millis" && args.is_empty() {
        let recv_t = lower_expr(receiver, cx, env);
        return TExpr {
            ty: Type::Int,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::StopwatchElapsedMillis,
                args: Vec::new(),
            },
        };
    }
    // c109 Phase 24: `Match.group(n)` (gate shape d4). The gate proved `recv_type ==
    // Some("Match")` + `group`/1 + an in-subset value receiver. Lower to `BuiltinMethod`/
    // `MatchGroup`, byte-for-byte `emit_builtin_method`'s `("Match", "group")` arm. The
    // result type is `String?`. Placed BEFORE the user-instance shape (also `recv_type ==
    // Some`) — `Match` is never a covered user struct/enum, so the two never collide.
    if recv_type.as_deref() == Some("Match") && method == "group" && args.len() == 1 {
        let recv_t = lower_expr(receiver, cx, env);
        let arg0 = lower_expr(&args[0].expr, cx, env);
        return TExpr {
            ty: Type::Option(Box::new(Type::String)),
            kind: TExprKind::BuiltinMethod {
                recv: Box::new(recv_t),
                op: TBuiltinOp::MatchGroup,
                args: vec![arg0],
            },
        };
    }
    // D-REACT1=B: a reactive `Signal`/`Derived` method (gate shape d5). The gate proved
    // `recv_type == Some("Signal"|"Derived")` + `get`/0 or `set`/1. Resolve the op +
    // result type HERE from the receiver's already-resolved `Apply<T>` slot (I3):
    // `Signal.get()`/`Derived.get()` → `T`; `Signal.set(v)` → Unit.
    if matches!(recv_type.as_deref(), Some("Signal") | Some("Derived"))
        && is_reactive_method_name(method, args.len())
    {
        let recv_t = lower_expr(receiver, cx, env);
        let elem = match &recv_t.ty {
            Type::Apply { args, .. } => args.first().cloned(),
            _ => None,
        }
        .unwrap_or_else(unit_type);
        let (op, ty) = match method {
            "get" => (THandleOp::ReactiveGet, elem),
            "set" => (THandleOp::ReactiveSet, unit_type()),
            _ => unreachable!("is_reactive_method_name admitted only get/set"),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // c109 Phase 21: a Task/Channel/Sender concurrency method (gate shape d3). The gate
    // proved `recv_type == None` + a disjoint concurrency name+arity. Resolve the op +
    // result type HERE (totality). The result type comes from `Collections::
    // builtin_method_return`'s `Type::Apply` arms (Source/Collections.rs), read off the
    // receiver's already-resolved type `Task<T>`/`Channel<T>`/`Sender<T>` (the LOWERED
    // receiver's `.ty`, total from the binding's annotated/inferred slot — never
    // re-inferred in emit, I3): `join` → `T`; `detach`/`send` → Unit; `receive` →
    // `Result<T, Closed>`; `sender` → `Sender<T>`. Args lowered PLAINLY (the AST
    // `emit_builtin_method`'s `arg(i)` is a raw `emit_expr`).
    if recv_type.is_none() && is_concurrency_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        // The element type `T` from the receiver's `Apply<T>` (the first type arg).
        let elem = match &recv_t.ty {
            Type::Apply { args, .. } => args.first().cloned(),
            _ => None,
        };
        let elem = elem.unwrap_or_else(unit_type);
        let (op, ty) = match method {
            "join" => (THandleOp::TaskJoin, elem),
            "detach" => (THandleOp::TaskDetach, unit_type()),
            "receive" => (
                THandleOp::ChannelReceive,
                Type::Result {
                    ok: Box::new(elem),
                    err: Box::new(Type::Named("Closed".to_string())),
                },
            ),
            "sender" => (
                THandleOp::ChannelSender,
                Type::Apply {
                    name: "Sender".to_string(),
                    args: vec![elem],
                },
            ),
            "send" => (THandleOp::SenderSend, unit_type()),
            _ => unreachable!("is_concurrency_method_name admitted only these names"),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // c109 Phase 11: a closure-taking collection method (`map`/`filter`/`each`/…).
    // The gate proved `recv_type == None` + a closure-method name + a literal lambda
    // arg. Resolve the receiver-type + Fn-vs-FnMut dispatch HERE into a total
    // `TClosureOp` (reproducing `emit_builtin_method`'s closure arms, incl. its
    // `expr_jet_ty(receiver)` Map/trait-object branches), so emit makes no decision.
    if recv_type.is_none() && crate::Collections::is_closure_method(method) {
        let op = resolve_closure_op(receiver, method, args, env, cx);
        let recv_t = lower_expr(receiver, cx, env);
        let recv_ast_ty = tir_recv_jet_ty(receiver, env);
        let result_ty = builtin_result_ty(method, args.len(), recv_ast_ty.as_ref());
        // Args lowered PLAINLY (the lambda + any seed) — `emit_builtin_method`'s
        // `arg(i)` is a raw `emit_expr`, no clone/borrow wrappers.
        let targs = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::ClosureMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // c109 Phase 12: a numeric predicate / bit-pop / width-conversion method
    // (`is_nan`/`count_ones`/`to_i32`/…). The gate proved `recv_type ==
    // Some(<numeric name>)` + a covered nullary numeric op. Resolve the receiver
    // width source/target + the widening-vs-narrowing branch HERE (reproducing
    // `numeric_conversion`/`conv_rust_target` from Expression.rs) into a total
    // `TNumericOp`, so emit makes no decision (I3). The result type comes from
    // `numeric_method_return` (the sema table), keyed on the receiver type recovered
    // from `recv_type` (the total width source — `src = recv_type.or_else(rty.name())`
    // on the AST side, where `recv_type` is always `Some` for these).
    if let Some(numeric_name) = recv_type {
        if let Some(recv_ty) = crate::AST::numeric_type_from_name(numeric_name) {
            if let Some(op) = resolve_numeric_op(method, numeric_name) {
                let recv_t = lower_expr(receiver, cx, env);
                let result_ty = builtin_result_ty(method, args.len(), Some(&recv_ty));
                return TExpr {
                    ty: result_ty,
                    kind: TExprKind::NumericMethod {
                        recv: Box::new(recv_t),
                        op,
                    },
                };
            }
        }
    }
    // c109 Phase 25: HttpRouter route registration `router.get/post/put/delete(path,
    // handler)` (D-ROUTE1=A). The gate (`router_register_in_subset`) proved the receiver
    // + path in-subset and the handler a named-fn/lambda. Render the handler closure HERE
    // (the `emit_router_handler` reproduction); emit assembles the register call. Result
    // is Unit (the registration is a statement effect).
    if recv_type.as_deref() == Some("HttpRouter")
        && matches!(method, "get" | "post" | "put" | "delete")
        && args.len() == 2
    {
        let verb = match method {
            "get" => "GET",
            "post" => "POST",
            "put" => "PUT",
            "delete" => "DELETE",
            _ => unreachable!(),
        };
        let recv_t = lower_expr(receiver, cx, env);
        let path_t = lower_expr(&args[0].expr, cx, env);
        let handler = render_router_handler(args, cx, env);
        return TExpr {
            ty: unit_type(),
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::HttpRouterRegister { verb, handler },
                args: vec![path_t],
            },
        };
    }
    // c109 Phase 13: a method ON a handle. The gate proved `recv_type ==
    // Some(<handle>)` + a covered handle op. Resolve the handle-receiver branch HERE
    // into a total `THandleOp` (reproducing the handle arms of `emit_builtin_method`),
    // so emit makes no type decision (I3). Args lowered PLAINLY (`arg(i)` = raw
    // `emit_expr`). The return type is the total sema handle-table fact.
    // D-SIMD2 / D-LINALG1: a method on a built-in math value type. Resolve the
    // reduce-op marker (which is NOT a lowerable expression) here so emit makes no
    // decision (I3). The return type is the total sema math-method fact.
    if let Some(handle) = recv_type {
        if crate::Sema::is_math_type(handle) && !cx.type_names.contains(handle) {
            let is_reduce = method == "reduce" && crate::Sema::is_simd_lane_type(handle);
            if is_reduce || crate::Sema::math_method_return(handle, method, args.len()).is_some() {
                let recv_t = lower_expr(receiver, cx, env);
                let (reduce_op, value_args): (Option<String>, Vec<TExpr>) = if is_reduce {
                    let op = match args.first().map(|a| &a.expr) {
                        Some(Expr::ReduceMarker(name, _)) => name.clone(),
                        _ => "Add".to_string(),
                    };
                    (Some(op), Vec::new())
                } else {
                    (None, args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect())
                };
                let ty = if is_reduce {
                    crate::Sema::math_scalar_ty(handle)
                } else {
                    crate::Sema::math_method_return(handle, method, args.len())
                        .unwrap_or_else(unit_type)
                };
                return TExpr {
                    ty,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(recv_t),
                        op: THandleOp::MathMethod {
                            type_name: handle.to_string(),
                            method: method.to_string(),
                            reduce_op,
                        },
                        args: value_args,
                    },
                };
            }
        }
    }
    if let Some(handle) = recv_type {
        if let Some(op) = handle_method_op(handle, method, args.len()) {
            let recv_t = lower_expr(receiver, cx, env);
            let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
            // c109 Phase 19: an arena `alloc(v)` returns a `&mut T` view whose VALUE type is
            // the arg's type (sema's `alloc_method_return` returns a `__alloc_infer__`
            // sentinel, resolved from the arg). The result `ty` is rarely load-bearing (an
            // `arena_view` binding emits no type annotation), but kept total per the design —
            // recovered from the LOWERED arg's total `ty`, never re-inferred (I3).
            let ty = match op {
                THandleOp::AllocAlloc => targs
                    .first()
                    .map(|a| a.ty.clone())
                    .unwrap_or_else(unit_type),
                THandleOp::AllocReset | THandleOp::AllocFree => unit_type(),
                _ => handle_method_return_ty(handle, method, args.len()),
            };
            return TExpr {
                ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op,
                    args: targs,
                },
            };
        }
    }
    // c109 Phase 7: a STATIC method call `Type.make(args)`. The gate
    // (`static_method_call_in_subset`) proved the receiver is a covered type-name
    // ident and `method` is a registered static method. Mirror the AST path
    // (Expression.rs ~L1644): `user_<Type>::user_<method>(args)`.
    if recv_type.is_none() {
        let Expr::Ident(type_name, _) = receiver else {
            unreachable!("gate proved static receiver is a type-name ident");
        };
        // D-SIMD2 / D-LINALG1: a static method on a built-in math type → the prelude
        // free function `{root}jet_math_<T>_<method>(args)`.
        if crate::Sema::is_math_type(type_name) && !cx.type_names.contains(type_name) {
            if let Some(ret) = crate::Sema::math_static_return(type_name, method, args.len()) {
                let bridge = crate::Sema::math_static_arg_ty(type_name, method);
                let targs: Vec<TExpr> = args
                    .iter()
                    .map(|a| {
                        let mut t = lower_expr(&a.expr, cx, env);
                        // D-FIXARR1 bridge: a `[..]` literal arg to `from_array` lowered
                        // as a growable list; re-tag it to the `[T#N]` fixed array so emit
                        // produces `[e1, …]` (a Rust stack array), not `vec![…]`.
                        if let Some(fl @ Type::FixedList { .. }) = &bridge {
                            if matches!(t.ty, Type::List(_)) && matches!(t.kind, TExprKind::ListLit(_)) {
                                t.ty = fl.clone();
                            }
                        }
                        t
                    })
                    .collect();
                return TExpr {
                    ty: ret,
                    kind: TExprKind::MathBuiltin {
                        type_name: type_name.clone(),
                        func: method.to_string(),
                        args: targs,
                    },
                };
            }
        }
        let sig = cx
            .method_sigs
            .get(&(type_name.clone(), method.to_string()))
            .cloned()
            .unwrap_or_default();
        let targs = lower_method_args(args, &sig, env, cx);
        let ret_ty = cx
            .method_rets
            .get(&(type_name.clone(), method.to_string()))
            .cloned()
            .flatten()
            .map(|t| resolve_self_ty(&t, type_name))
            .unwrap_or_else(unit_type);
        return TExpr {
            ty: ret_ty,
            kind: TExprKind::StaticCall {
                // The AST path uses `cx.type_prefix(type_name)` = `user_<T>`.
                type_prefix: cx.type_prefix(type_name),
                method_rust: mangle(method),
                args: targs,
            },
        };
    }
    // c109 Phase 30: DYNAMIC dispatch on a TRAIT-OBJECT receiver (`s.name()`/`s.area()`,
    // `s: Box<dyn user_Shape>`). The gate proved `recv_type == Some(<trait>)` with the
    // trait in `cx.trait_names`. The AST `emit_method_call` (Expression.rs ~L1657) emits
    // `({recv}).{method}({args})` — the BARE (unmangled) method name (vtable dispatch),
    // args lowered PLAINLY (`emit_call_args(.., None, ..)` — no sig). Reuse the `MethodCall`
    // node (`({recv}).{method_rust}({args})`) with the bare method name. The result type is
    // NOT load-bearing here (the AST carries no sig/return for a trait-object call, and a
    // trait-method return isn't registered in `cx.method_rets` by trait name) — it is read
    // only where the call result feeds a type-driven decision, which the sole reachable
    // program (`print("{s.name()}: {s.area()}")` — string interpolation calls `.jet_show()`,
    // type-agnostic) never does. Carry `unit_type`, exactly as the AST has no return fact.
    if let Some(ty) = recv_type {
        if cx.trait_names.contains(ty) {
            let recv = lower_expr(receiver, cx, env);
            // Plain args (conv None), reproducing `emit_call_args(cx, None, args, env)`.
            let targs: Vec<TCallArg> =
                args.iter().map(|a| lower_one_call_arg(a, None, env, cx)).collect();
            return TExpr {
                ty: unit_type(),
                kind: TExprKind::MethodCall {
                    recv: Box::new(recv),
                    method_rust: method.to_string(),
                    args: targs,
                },
            };
        }
    }
    // A user instance method on a covered type. `recv_type` is total (gate proved
    // `Some`). Resolve the param conventions from `method_sigs` and the Rust method
    // name (trait-impl methods keep their bare name; others get the `user_` mangle).
    let ty_name = recv_type.clone().expect("gate proved recv_type is Some");
    let sig = cx
        .method_sigs
        .get(&(ty_name.clone(), method.to_string()))
        .cloned()
        .unwrap_or_default();
    let recv = lower_expr(receiver, cx, env);
    let targs = lower_method_args(args, &sig, env, cx);
    // S62: a trait-impl method is called by its bare name (the trait impl owns it);
    // a plain user method is `user_<method>`. This mirrors `emit_method_call`'s
    // `trait_methods` check exactly — decided here, total, never re-derived in emit.
    let method_rust = if cx
        .trait_methods
        .contains(&(ty_name.clone(), method.to_string()))
    {
        method.to_string()
    } else {
        mangle(method)
    };
    // The result type, read from the resolved method return (total fact). It is
    // rarely load-bearing in emit (a binding carries sema's `b.ty`; arithmetic on a
    // method result doesn't trap — matching the AST `expr_jet_ty`/`operand_is_integer`),
    // but the TIR keeps it total per the design principle.
    let ret_ty = cx
        .method_rets
        .get(&(ty_name.clone(), method.to_string()))
        .cloned()
        .flatten()
        .unwrap_or_else(unit_type);
    TExpr {
        ty: ret_ty,
        kind: TExprKind::MethodCall {
            recv: Box::new(recv),
            method_rust,
            args: targs,
        },
    }
}

/// c109 Phase 13: lower a closure-taking core call (`tasks.spawn`/`http.serve`/
/// `scope.guard`) into a bespoke `CoreClosureCall` node, reproducing `emit_core_call`
/// (Source/Codegen/Expression.rs) byte-for-byte. Returns `None` when `(module,
/// method)` isn't one of the three (so the caller falls through to the plain
/// `CoreCall`). The gate (`core_closure_call_in_subset`) already proved a literal
/// in-subset lambda in the closure-arg position.
pub(crate) fn lower_core_closure_call(
    module: &str,
    method: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &mut LowerEnv,
) -> Option<TExpr> {
    let lam_at = |i: usize| match args.get(i).map(|a| &a.expr) {
        Some(Expr::Lambda(lam)) => Some(lam),
        _ => None,
    };
    let kind = match (module, method) {
        ("core.tasks", "spawn") => {
            let lam = lam_at(0)?;
            // The spawned body's type (the lambda's return) is the Task's element type.
            let body_ty = lambda_body_ty(lam, cx, env);
            let spawn_closure = render_spawn_lambda(lam, cx, env);
            return Some(TExpr {
                ty: core_closure_call_return_ty(module, method, body_ty),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::Spawn { spawn_closure },
                },
            });
        }
        ("jet.http", "serve") => {
            let lam = lam_at(1)?;
            let addr = lower_expr(&args[0].expr, cx, env);
            let closure = render_lambda_str(lam, cx, env);
            TCoreClosureKind::Serve {
                addr: Box::new(addr),
                closure,
            }
        }
        ("core.scope", "guard") => {
            let lam = lam_at(0)?;
            let closure = render_lambda_str(lam, cx, env);
            TCoreClosureKind::Guard { closure }
        }
        // D-REACT1=B: the `derived` closure's body type is the `Derived<T>` element.
        ("jet.reactive", "derived") => {
            let lam = lam_at(0)?;
            let body_ty = lambda_body_ty(lam, cx, env);
            let closure = render_lambda_str(lam, cx, env);
            return Some(TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_DERIVED.to_string(),
                    args: vec![body_ty],
                },
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::ReactiveDerived { closure },
                },
            });
        }
        ("jet.reactive", "effect") => {
            let lam = lam_at(0)?;
            let closure = render_lambda_str(lam, cx, env);
            TCoreClosureKind::ReactiveEffect { closure }
        }
        _ => return None,
    };
    Some(TExpr {
        ty: core_closure_call_return_ty(module, method, unit_type()),
        kind: TExprKind::CoreClosureCall { kind },
    })
}

/// c109 Phase 13: the type of a lambda's body (its return), used for a `spawn`ed
/// closure's `Task<T>` element type. An expression body's type is the lowered expr's
/// `ty`; a block body's type is rarely load-bearing in the subset (the Task type is
/// not read by emit), so a `Unit` placeholder is fine for a block.
pub(crate) fn lambda_body_ty(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> Type {
    match &lam.body {
        LambdaBody::Expr(e) => {
            let mut lam_env = clone_env(env);
            for p in &lam.params {
                lam_env
                    .locals
                    .insert(p.name.clone(), (mangle(&p.name), p.ty.clone()));
            }
            lower_expr(e, cx, &mut lam_env).ty
        }
        LambdaBody::Block(_) => unit_type(),
    }
}

/// c109 Phase 6/13: lower method-call arguments, mirroring `emit_call_args`
/// (Source/Codegen/Expression.rs). The clone/Arc wrappers, the borrow/mut-borrow
/// wrappers, and the Fn-typed Box-coercion are all decided here from total facts
/// (`CallArg.flags` + the resolved param convention/type), never re-derived in emit.
pub(crate) fn lower_method_args(
    args: &[crate::AST::CallArg],
    sig: &[(AccessConvention, Type)],
    env: &mut LowerEnv,
    cx: &Cx,
) -> Vec<TCallArg> {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let conv = sig.get(i).map(|(c, t)| (*c, t.clone()));
            lower_one_call_arg(a, conv, env, cx)
        })
        .collect()
}

/// c109 Phase 13: lower ONE call argument, reproducing `emit_call_args`
/// (Source/Codegen/Expression.rs) byte-for-byte — the single source of truth for
/// the clone/Arc, Fn-coercion, and borrow/mut-borrow wrapper order. `conv` is the
/// resolved param `(convention, type)` for this position (`None` when the callee has
/// no known signature, e.g. a `CallValue`). The emit order is exactly the AST path's:
///   1. the implicit-clone / Arc-clone wrapper (`(…).clone()` / `Arc::clone(&…)`);
///   2. the Fn-typed Box-coercion (`Box::new(…) as <fn-type>`, or just ` as <fn-type>`
///      when already boxed);
///   3. the borrow wrapper (`&(…)` for a `Read` non-scalar non-Fn, `&mut (…)` for a
///      `Mutate`).
pub(crate) fn lower_one_call_arg(
    a: &crate::AST::CallArg,
    conv: Option<(AccessConvention, Type)>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> TCallArg {
    // A bare lambda flowing into a user fn-typed parameter takes its param
    // types from that fn-type so codegen emits the Rust closure-param types
    // rustc needs (c142). Other args lower normally.
    let value = match (&a.expr, &conv) {
        (Expr::Lambda(lam), Some((_, Type::Fn { params, .. }))) => {
            let tl = lower_lambda_expecting(lam, cx, env, Some(params.as_slice()));
            TExpr {
                ty: Type::Fn {
                    params: Vec::new(),
                    ret: None,
                    effect_bound: None,
                },
                kind: TExprKind::Lambda(Box::new(tl)),
            }
        }
        _ => lower_expr(&a.expr, cx, env),
    };
    let clone = a.flags.implicit_clone;
    let arc_clone = a.flags.shared_auto_clone;
    // The Fn-typed Box-coercion (`emit_call_args`' `if let Some((_, Type::Fn …))`).
    let fn_coerce = match &conv {
        Some((_, Type::Fn { .. })) => {
            // `already_boxed`: the value already produces a `Box::new(…)`. The AST
            // checks two cases — the emitted string starts with `Box::new(` (only a
            // bare fn-name value does, in subset — `emit_named_fn_value`), OR the
            // value is a fn-typed local ident. Resolve both at lowering.
            let already_boxed = ast_arg_is_named_fn_value(&a.expr, cx, env)
                || matches!(
                    &a.expr,
                    Expr::Ident(name, _)
                        if env.ty_of(name).is_some_and(|t| matches!(t, Type::Fn { .. }))
                );
            let (_, ty) = conv.as_ref().expect("matched Some above");
            Some(TFnCoerce {
                fn_type_rust: cx.rust_type(ty),
                already_boxed,
            })
        }
        _ => None,
    };
    // D-FIXARR1: when a [T#N] (Rust [T; N]) is passed where a [T] (Vec<T>) is expected,
    // widen by copying into a growable list (`.to_vec()`).
    let widen_to_vec = matches!(
        (&value.ty, conv.as_ref().map(|(_, t)| t)),
        (Type::FixedList { elem: arg_elem, .. }, Some(Type::List(param_elem)))
            if arg_elem == param_elem
    );
    // Borrow wrappers (applied after the clone + fn-coerce wrappers). A `Read`
    // non-scalar (non-Fn) is `&(…)`; a `Mutate` is `&mut (…)`. A Fn-typed `Read` is
    // NOT borrowed (the AST `match conv` skips it), so the fn-coerce form stands alone.
    // When widening to Vec, the borrow wrapper applies to the widened Vec (not the array).
    let (borrow, mut_borrow) = match &conv {
        // D-CAP8/9: Infer (pre-resolution default) and Share borrow like Read (`&(…)`)
        // until their phases specialize them; Raw (never produced yet) stays by-value.
        Some((
            AccessConvention::Read | AccessConvention::Infer | AccessConvention::Share,
            t,
        )) if !t.is_scalar() && !matches!(t, Type::Fn { .. }) => (true, false),
        Some((AccessConvention::Write, _)) => (false, true),
        _ => (false, false),
    };
    TCallArg {
        value,
        borrow,
        mut_borrow,
        clone,
        arc_clone,
        fn_coerce,
        widen_to_vec,
    }
}

/// c109 Phase 14: lower a cross-module call's arguments against the callee's import
/// signature, reproducing `emit_call_args`. Each arg's borrow/clone/fn-coercion is
/// resolved from the sig param convention (the same `lower_one_call_arg` used by the
/// plain-call path).
pub(crate) fn lower_module_args(
    args: &[crate::AST::CallArg],
    sig: Option<&[(AccessConvention, Type)]>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> Vec<TCallArg> {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let conv = sig.and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
            lower_one_call_arg(a, conv, env, cx)
        })
        .collect()
}

/// c109 Phase 14: lower one FFI extern-call argument, reproducing
/// `emit_extern_call_args` (Source/Codegen/Expression.rs). The value is wrapped in
/// `(…).clone()` when the arg carries `implicit_clone`, OR when its param is a
/// non-scalar `Read`-convention type and `implicit_clone` is NOT already set (the AST
/// `if a.flags.implicit_clone { … } else if … } if let Some((_, ty)) = sig … if
/// !ty.is_scalar() && !implicit_clone`). The Arc (`shared_auto_clone`) form is excluded
/// from the subset, so it never reaches here.
pub(crate) fn lower_extern_call_arg(
    a: &crate::AST::CallArg,
    conv: Option<(AccessConvention, Type)>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> TExternArg {
    let value = lower_expr(&a.expr, cx, env);
    let non_scalar_param = conv
        .as_ref()
        .map(|(_, ty)| !ty.is_scalar())
        .unwrap_or(false);
    // `(…).clone()` is emitted once: either the explicit implicit_clone flag, or the
    // non-scalar-param clone (when implicit_clone is false). The two never stack — the
    // AST applies the param clone only `&& !a.flags.implicit_clone`.
    let clone = a.flags.implicit_clone || (non_scalar_param && !a.flags.implicit_clone);
    TExternArg { value, clone }
}

/// c109 Phase 13: does this AST arg expression emit as a `Box::new(…)` (a bare
/// fn-name value via `emit_named_fn_value`)? That is exactly an `Expr::Ident` which
/// is NOT a local and resolves to a `Type::Fn` in `cx.fn_types` (a top-level fn used
/// as a value). Mirrors `emit_expr`'s `Expr::Ident` arm + `emit_call_args`'
/// `s.starts_with("Box::new(")` check, resolved at lowering.
pub(crate) fn ast_arg_is_named_fn_value(e: &Expr, cx: &Cx, env: &LowerEnv) -> bool {
    if let Expr::Ident(name, _) = e {
        if !env.locals.contains_key(name) && !cx.consts.contains_key(name) {
            return matches!(cx.fn_types.get(name), Some(Type::Fn { .. }));
        }
    }
    false
}

/// c109 Phase 9: reproduce codegen's `expr_jet_ty(receiver, env)`
/// (Source/Codegen/Expression.rs) for a built-in method receiver, using the TIR
/// lowering env's slot types. This MUST match `expr_jet_ty` bit-for-bit (incl. its
/// `None` results) because the Map-vs-List-vs-String emit branch in
/// `emit_builtin_method` is keyed on it: a divergence here flips a branch and breaks
/// byte-parity. Only `Ident` (via its slot type), `Str`/`Char`, and chained
/// `chars`/`split`/other method calls resolve; everything else (notably a struct
/// `Field` read) is `None` — exactly as `expr_jet_ty` does, so a `None`-typed
/// receiver lands on the AST's default branch (the list/else arm).
pub(crate) fn tir_recv_jet_ty(e: &Expr, env: &LowerEnv) -> Option<Type> {
    match e {
        Expr::Ident(name, _) => env.ty_of(name),
        Expr::Str(_, _) => Some(Type::String),
        Expr::Char(_, _) => Some(Type::Char),
        Expr::TupleLit(_, _, Some(ty)) => Some(ty.clone()),
        Expr::MethodCall { receiver, method, .. } => {
            if method == "chars" {
                return Some(Type::List(Box::new(Type::Char)));
            }
            if method == "split" {
                return Some(Type::List(Box::new(Type::String)));
            }
            tir_recv_jet_ty(receiver, env)
        }
        _ => None,
    }
}

/// c109 Phase 9: resolve the built-in method op from the method name, arg count, and
/// the receiver's resolved type — reproducing `emit_builtin_method`'s name+`rty`
/// dispatch (Source/Codegen/Expression.rs) exactly. The Map-vs-List branch
/// (`insert`/`remove`/`get`) and the String-vs-list branch (`len`) come from
/// `tir_recv_jet_ty` (matching the AST's `rty`); a `None` or non-Map/non-String
/// receiver falls to the list/else branch, byte-for-byte the AST default. Returns
/// `None` for any name/shape the TIR does not lower (the caller stays on the AST
/// path — the gate already excluded these, so this is a defensive belt).
pub(crate) fn resolve_builtin_op(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[crate::AST::CallArg],
    env: &LowerEnv,
    cx: &Cx,
) -> Option<TBuiltinOp> {
    if crate::Collections::is_closure_method(method) {
        return None;
    }
    let rty = tir_recv_jet_ty(receiver, env);
    let is_string = matches!(rty, Some(Type::String));
    let is_map = matches!(rty, Some(Type::Map { .. }));
    Some(match (method, args.len()) {
        ("len", 0) => {
            if is_string {
                TBuiltinOp::LenString
            } else {
                TBuiltinOp::LenList
            }
        }
        ("is_empty", 0) => TBuiltinOp::IsEmpty,
        ("push", 1) => TBuiltinOp::Push,
        ("pop", 0) => TBuiltinOp::Pop,
        ("insert", 2) => {
            if is_map {
                TBuiltinOp::InsertMap
            } else {
                TBuiltinOp::InsertList
            }
        }
        ("remove", 1) => {
            if is_map {
                TBuiltinOp::RemoveMap
            } else {
                // The list form embeds the *method-span* line for its bounds panic,
                // exactly as `emit_builtin_method` reads `span_line_col(method_span.start)`.
                let line = crate::Diagnostics::span_line_col(&cx.src, method_span.start).0;
                TBuiltinOp::RemoveList { line }
            }
        }
        ("get", 1) => {
            if is_map {
                TBuiltinOp::GetMap
            } else {
                TBuiltinOp::GetList
            }
        }
        ("first", 0) => TBuiltinOp::First,
        ("last", 0) => TBuiltinOp::Last,
        ("contains", 1) => TBuiltinOp::Contains,
        ("index_of", 1) => TBuiltinOp::IndexOf,
        ("reverse", 0) => TBuiltinOp::Reverse,
        ("sort", 0) => TBuiltinOp::Sort,
        ("join", 1) => TBuiltinOp::JoinSep,
        ("clear", 0) => TBuiltinOp::Clear,
        ("chars", 0) => TBuiltinOp::Chars,
        ("bytes", 0) => TBuiltinOp::Bytes,
        ("trim", 0) => TBuiltinOp::Trim,
        ("split", 1) => TBuiltinOp::Split,
        // c97/D-STRPARSE1: String-only builtins. The numeric `to_int` is a `MethodCall`
        // with a numeric `recv_type` (→ `NumericMethod`) and never reaches this path;
        // the stream-handle `lines` carries a handle `recv_type` and is likewise routed
        // elsewhere. There is no list/map `to_int`/`lines`, so an unguarded match here is
        // unambiguous — no `is_string` test (the loop-var receiver carries `jet_ty: None`,
        // so it would spuriously fail one).
        ("lines", 0) => TBuiltinOp::Lines,
        ("to_int", 0) => TBuiltinOp::ToIntString,
        ("starts_with", 1) => TBuiltinOp::StartsWith,
        ("ends_with", 1) => TBuiltinOp::EndsWith,
        ("replace", 2) => TBuiltinOp::Replace,
        ("to_upper", 0) => TBuiltinOp::ToUpper,
        ("to_lower", 0) => TBuiltinOp::ToLower,
        ("repeat", 1) => TBuiltinOp::Repeat,
        ("slice", 2) => {
            // The string-slice form embeds the *receiver-span* line for its bounds panic.
            let line = crate::Diagnostics::span_line_col(&cx.src, receiver.span().start).0;
            TBuiltinOp::Slice { line }
        }
        ("keys", 0) => TBuiltinOp::Keys,
        ("values", 0) => TBuiltinOp::Values,
        ("contains_key", 1) => TBuiltinOp::ContainsKey,
        ("to_string", 0) => TBuiltinOp::ToString,
        // D-ITER1: non-closure list adapters.
        ("take", 1) => TBuiltinOp::Take,
        ("skip", 1) => TBuiltinOp::Skip,
        ("step_by", 1) => TBuiltinOp::StepBy,
        ("dedup", 0) => TBuiltinOp::Dedup,
        ("chunks", 1) => TBuiltinOp::Chunks,
        ("windows", 1) => TBuiltinOp::Windows,
        ("enumerate", 0) => {
            // Build the tuple struct name for `(idx: Int, item: T)`.
            // Fields are alpha-sorted: idx < item.
            let elem_ty = match &rty {
                Some(Type::List(inner)) => *inner.clone(),
                _ => Type::Int,
            };
            let fields = vec![
                ("idx".to_string(), Type::Int),
                ("item".to_string(), elem_ty),
            ];
            let ts = crate::Codegen::Tuples::tuple_struct_name(&fields);
            TBuiltinOp::Enumerate { tuple_struct: ts }
        }
        ("zip", 1) => {
            // Build the tuple struct name for `(a: T, b: U)`.
            let a_ty = match &rty {
                Some(Type::List(inner)) => *inner.clone(),
                _ => Type::Int,
            };
            let b_ty = match tir_recv_jet_ty(&args[0].expr, env) {
                Some(Type::List(inner)) => *inner,
                _ => Type::Int,
            };
            let fields = vec![
                ("a".to_string(), a_ty),
                ("b".to_string(), b_ty),
            ];
            let ts = crate::Codegen::Tuples::tuple_struct_name(&fields);
            TBuiltinOp::Zip { tuple_struct: ts }
        }
        _ => return None,
    })
}

/// c109 Phase 9: the resolved return type of a built-in collection/string method,
/// from `Collections::builtin_method_return` (the sema table). Kept total per the
/// design principle; rarely load-bearing in emit (a binding carries sema's `b.ty`),
/// but resolved here so the TIR never guesses. Falls back to `Unit` for a void
/// method or an unresolved receiver type (impossible for a covered call — sema
/// validated it).
pub(crate) fn builtin_result_ty(method: &str, nargs: usize, recv_ty: Option<&Type>) -> Type {
    match recv_ty.and_then(|rt| crate::Collections::builtin_method_return(rt, method, nargs, false)) {
        Some(Some(t)) => t,
        _ => unit_type(),
    }
}

/// c109 Phase 11: resolve a closure-taking collection method into a total
/// `TClosureOp`, reproducing `emit_builtin_method`'s closure arms
/// (Source/Codegen/Expression.rs) exactly. The receiver-type branch
/// (`rty = expr_jet_ty(receiver)`) picks Map (`EachMap`) vs trait-object list
/// (`EachRef`) vs plain list; the Fn-vs-FnMut branch reads the lambda arg's
/// `needs_fn_mut` meta. All decisions made HERE, never in emit (I3). The gate
/// proved a literal lambda arg, so `needs_fn_mut` is always readable; a non-lambda
/// arg defaults to the non-mut form, matching the AST `else` branch.
pub(crate) fn resolve_closure_op(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    env: &LowerEnv,
    cx: &Cx,
) -> TClosureOp {
    let rty = tir_recv_jet_ty(receiver, env);
    // The lambda arg's FnMut fact (the AST checks `args[0]` for map/each).
    let fn_mut = matches!(args.first().map(|a| &a.expr), Some(Expr::Lambda(l)) if l.meta.needs_fn_mut);
    match method {
        "map" => {
            if fn_mut {
                TClosureOp::MapMut
            } else {
                TClosureOp::Map
            }
        }
        "filter" => TClosureOp::Filter,
        "each" => {
            // The AST: `match rty { Map => jet_map_each, _ => list_each }`, where
            // `list_each` checks trait-object-list FIRST, then lambda FnMut.
            match &rty {
                Some(Type::Map { .. }) => TClosureOp::EachMap,
                Some(Type::List(inner)) if list_carries_trait(cx, inner) => TClosureOp::EachRef,
                _ if fn_mut => TClosureOp::EachMut,
                _ => TClosureOp::Each,
            }
        }
        "find" => TClosureOp::Find,
        "any" => TClosureOp::Any,
        "all" => TClosureOp::All,
        "sort_by" => TClosureOp::SortBy,
        "reduce" => TClosureOp::Reduce,
        // D-ITER1: new closure adapters.
        "take_while" => TClosureOp::TakeWhile,
        "skip_while" => TClosureOp::SkipWhile,
        "flat_map" => TClosureOp::FlatMap,
        "scan" => TClosureOp::Scan,
        "fold" => TClosureOp::Fold,
        "position" => TClosureOp::Position,
        "min_by" => TClosureOp::MinBy,
        "max_by" => TClosureOp::MaxBy,
        "group_by" => TClosureOp::GroupBy,
        "partition" => {
            // Compute the tuple struct name from the receiver element type.
            // recv = List<T>; partition returns (false_: [T], true_: [T]).
            let elem_ty = match tir_recv_jet_ty(receiver, env) {
                Some(Type::List(inner)) => *inner,
                _ => Type::Int,
            };
            let list_ty = Type::List(Box::new(elem_ty.clone()));
            let fields = vec![
                ("false_".to_string(), list_ty.clone()),
                ("true_".to_string(), list_ty),
            ];
            let ts = crate::Codegen::Tuples::tuple_struct_name(&fields);
            TClosureOp::Partition { tuple_struct: ts }
        }
        // The gate (`is_closure_method`) admits only the names above.
        _ => unreachable!("non-closure method in resolve_closure_op (gate)"),
    }
}

/// c109 Phase 11: TIR-local reproduction of codegen's `list_carries_trait`
/// (Source/Codegen/Expression.rs) — a list element type that is a trait object or a
/// named trait. Used by the `each`-on-trait-object-list emit branch (`jet_list_each_ref`).
/// In the covered collection subset a trait-object element type is excluded, so this
/// is always false for a covered receiver; reproduced for exactness regardless.
pub(crate) fn list_carries_trait(cx: &Cx, inner: &Type) -> bool {
    matches!(inner, Type::TraitObject(_))
        || matches!(inner, Type::Named(n) if cx.trait_names.contains(n))
}

/// c109 Phase 11: lower a lambda/closure literal (`Expr::Lambda`) to a `TLambda`,
/// reproducing `emit_lambda` (Source/Codegen/Expression.rs) byte-for-byte. Every
/// capture/escape/Fn-vs-FnMut decision is the TOTAL `Lambda.meta` fact — no capture
/// analysis here. The body is lowered on a CLONED env extended with: the cloned
/// captures (rebound to `_jet_cap_<n>`, place = that name, type `None` — matching the
/// AST slot) and the params (place = mangled name, type from the annotation). The
/// rendered closure body string is produced now so emit is a pure wrapper.
pub(crate) fn lower_lambda(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> TLambda {
    lower_lambda_expecting(lam, cx, env, None)
}

/// `lower_lambda`, but with the expected parameter types from the fn-typed slot
/// this lambda flows into (a user fn-typed parameter). A bare lambda param
/// (`(x) => …`, no annotation) takes its Rust type from there so codegen emits
/// `move |user_x: i64| …` instead of an un-annotated `move |user_x| …` that
/// rustc can't infer (c142). Builtin closure methods (`.each`/`.map`/…) keep
/// passing `None`: their helper signatures drive the closure-param type (often
/// by-ref), so annotating it would mismatch.
pub(crate) fn lower_lambda_expecting(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: Option<&[Type]>,
) -> TLambda {
    // `emit_lambda` clones the env (`lam_env = env.clone()`), so a `??` panic inside the
    // lambda body dumps the lambda's env (outer locals + captures + params) and does NOT
    // leak into the enclosing fn — a NON-leaky boundary, so fork the panic replica.
    let mut lam_env = fork_panic(env);
    // The clone-capture prelude: `let _jet_cap_<n> = (<outer place>).clone();`. The
    // outer place comes from the *outer* env (the capture is an outer local). The cap
    // rebinds the name with place `_jet_cap_<n>`, no deref, type `None` (matching the
    // AST slot `{ rust_name: cap, deref: false, jet_ty: None }`).
    let mut prep = String::new();
    for name in &lam.meta.cloned_captures {
        let cap = format!("_jet_cap_{}", mangle(name));
        prep.push_str(&format!(
            "let {} = ({}).clone();\n    ",
            cap,
            env.place_of(name)
        ));
        lam_env.bind(name, cap, None);
    }
    // Params bind as `mangle(name)` (no deref), typed from the annotation (or `None`).
    for p in &lam.params {
        lam_env.bind(&p.name, mangle(&p.name), p.ty.clone());
    }
    // The rendered param list: `name[: ty]`, exactly as `emit_lambda`. A bare
    // param (no annotation) falls back to the expected fn-type's param at the
    // same position (c142), so a closure passed to a user fn-typed parameter
    // always carries the Rust type rustc needs.
    let params: Vec<String> = lam
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let ty = p
                .ty
                .clone()
                .or_else(|| expected_params.and_then(|ps| ps.get(i)).cloned())
                .map(|t| format!(": {}", cx.rust_type(&t)))
                .unwrap_or_default();
            format!("{}{}", mangle(&p.name), ty)
        })
        .collect();
    // The body: an expression body lowers + emits directly; a block body lowers its
    // statements (on the lambda env) and emits a `{ … }` at indent 1 — byte-for-byte
    // `emit_lambda`'s `emit_stmts(…, 1, false)` then `format!("{{ {} }}", inner)`.
    let body = match &lam.body {
        LambdaBody::Expr(e) => emit_tir_expr(&lower_expr(e, cx, &mut lam_env), cx),
        LambdaBody::Block(stmts) => {
            let lowered = lower_stmts(stmts, cx, &mut lam_env);
            let mut inner = String::new();
            emit_tir_stmts(&lowered, cx, &mut inner, 1);
            format!("{{ {} }}", inner)
        }
    };
    // `move ` keyword: the AST emits it UNLESS the lambda is FnMut and does not escape.
    let is_move = !(lam.meta.needs_fn_mut && !lam.meta.escapes);
    TLambda {
        prep,
        params,
        body,
        is_move,
        boxed: lam.meta.escapes,
    }
}

/// c109 Phase 13: render a `tasks.spawn` lambda, reproducing `emit_spawn_lambda`
/// (Source/Codegen/Expression.rs) byte-for-byte. It is `emit_lambda` minus the
/// Fn-vs-FnMut and escape logic: ALWAYS `move`, NEVER `Box::new`. The clone-capture
/// prelude is identical. Returns the full rendered closure string (wrapped in
/// `{ <prep> <closure> }` when there are cloned captures).
pub(crate) fn render_spawn_lambda(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> String {
    let mut lam_env = fork_panic(env);
    let mut prep = String::new();
    for name in &lam.meta.cloned_captures {
        let cap = format!("_jet_cap_{}", mangle(name));
        prep.push_str(&format!("let {} = ({}).clone();\n    ", cap, env.place_of(name)));
        lam_env.bind(name, cap, None);
    }
    for p in &lam.params {
        lam_env.bind(&p.name, mangle(&p.name), p.ty.clone());
    }
    let params: Vec<String> = lam
        .params
        .iter()
        .map(|p| {
            let ty = p
                .ty
                .as_ref()
                .map(|t| format!(": {}", cx.rust_type(t)))
                .unwrap_or_default();
            format!("{}{}", mangle(&p.name), ty)
        })
        .collect();
    let body = match &lam.body {
        LambdaBody::Expr(e) => emit_tir_expr(&lower_expr(e, cx, &mut lam_env), cx),
        LambdaBody::Block(stmts) => {
            let lowered = lower_stmts(stmts, cx, &mut lam_env);
            let mut inner = String::new();
            emit_tir_stmts(&lowered, cx, &mut inner, 1);
            format!("{{ {} }}", inner)
        }
    };
    let closure = format!("move |{}| {}", params.join(", "), body);
    if prep.is_empty() {
        closure
    } else {
        format!("{{ {} {} }}", prep, closure)
    }
}

/// c109 Phase 13: render a lambda via the plain `emit_lambda` form (used by
/// `http.serve`'s lambda handler and `scope.guard`). Returns the full closure string.
pub(crate) fn render_lambda_str(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> String {
    let tl = lower_lambda(lam, cx, env);
    let move_kw = if tl.is_move { "move " } else { "" };
    let closure = format!("{}|{}| {}", move_kw, tl.params.join(", "), tl.body);
    let wrapped = if tl.boxed {
        format!("Box::new({})", closure)
    } else {
        closure
    };
    if tl.prep.is_empty() {
        wrapped
    } else {
        format!("{{ {} {} }}", tl.prep, wrapped)
    }
}

// ---------------------------------------------------------------------------
// Emission: TIR -> Rust. PURE formatting. No type inference, no decisions.
// ---------------------------------------------------------------------------


/// c109 Phase 25: render the router handler (arg 1) exactly as `emit_router_handler`
/// (Source/Codegen/Expression.rs) does, at lowering. A bare top-level fn name (not a
/// local) becomes the `Box::new(move |__req: …| user_<fn>(&__req)) as Box<dyn Fn(…) -> …
/// + Send + Sync>` wrapper; a lambda becomes `Box::new(<lambda>) as Box<…>`.
pub(crate) fn render_router_handler(args: &[crate::AST::CallArg], cx: &Cx, env: &LowerEnv) -> String {
    let root = &cx.root_prefix;
    let boxed_dyn = format!(
        "as Box<dyn Fn({}JetHttpRequest) -> {}JetHttpResponse + Send + Sync>",
        root, root
    );
    match &args[1].expr {
        Expr::Ident(name, _) if !env.locals.contains_key(name) => {
            let rust_name = mangle(name);
            format!(
                "Box::new(move |__req: {}JetHttpRequest| {}(&__req)) {}",
                root, rust_name, boxed_dyn
            )
        }
        Expr::Lambda(lam) => {
            format!("Box::new({}) {}", render_lambda_str(lam, cx, env), boxed_dyn)
        }
        // The gate (`router_register_in_subset`) proved arg 1 is one of the two above.
        _ => unreachable!("router handler gate proved a named-fn or lambda handler"),
    }
}
