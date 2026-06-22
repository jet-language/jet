//! TIR — a small, *typed* intermediate representation for codegen (c109 Phase 1).
//!
//! ## Why this exists
//!
//! Today codegen (`emit_func` and friends) re-derives semantic facts while it
//! emits Rust: it calls `expr_jet_ty` to re-infer expression types and
//! `operand_is_integer` to re-decide which operator traps on overflow. That is
//! exactly the "codegen re-derives / falls back" smell that invariant I3 ("codegen
//! is dumb") forbids, and it is the bug class that produced the I2 holes the
//! checked-IR effort (`tools/Tower/docs/sidequests/checked-ir-design.md`) is
//! built to kill.
//!
//! The TIR is the fix. It is a distinct, post-sema representation whose defining
//! property is **TOTALITY**: every fact codegen needs is carried *concretely* on
//! the node — never re-inferred, never an `Option` codegen has to fall back from.
//! Every `TExpr` carries its resolved `Type`; every `Binary` carries its overflow
//! decision as a plain `bool`; every `Let` carries the resolved binding type. The
//! emitter (`emit_tir_func`) makes ZERO decisions: it pattern-matches TIR fields
//! and formats Rust. It never calls `expr_jet_ty` or `operand_is_integer`.
//!
//! ## Phase 1 scope (deliberately tiny)
//!
//! This is the foundational slice. It covers only the *simplest* top-level
//! functions — scalar/String params, arithmetic/logic/comparison, bindings,
//! assignments, returns, `if`, calls to plain functions and `print`. The gate
//! `tir_covers` decides, conservatively, whether a function is fully inside that
//! subset; anything outside stays on the existing AST `emit_func` path, untouched.
//! The two paths must produce byte-identical Rust (golden parity, `tests/golden.rs`),
//! which is how we prove the rest of the compiler is undisturbed.
//!
//! Later phases widen `tir_covers` and add TIR nodes until the AST codegen path
//! is deleted. So the rule for this module is: **add a node only when its
//! construct is in the covered subset, and make every field total.**

use super::*;
use crate::AST::{
    AccessConvention, BinOp, ElseBranch, Expr, Func, IfStmt, LValue, Param, Stmt, StrPart, Type,
    UnOp,
};
use crate::Syntax;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// TIR types. Every node carries the facts codegen needs, pre-resolved (totality).
// ---------------------------------------------------------------------------

/// A lowered top-level function. `params` are already mangled to their Rust
/// names and carry their resolved Jet `Type`; `ret` is the resolved return type.
pub(crate) struct TFunc {
    /// Jet function name (unmangled) — the emitter mangles via `cx.mangle_name`.
    pub(crate) name: String,
    /// `(mangled rust name, resolved jet type, convention)` per parameter. The
    /// convention is kept so the emitter reproduces the `&`/by-value Rust form
    /// without re-deciding (it mirrors `rust_param_type`).
    pub(crate) params: Vec<(String, Type, AccessConvention)>,
    /// Resolved return type, or `None` for a unit-returning function.
    pub(crate) ret: Option<Type>,
    pub(crate) is_main: bool,
    pub(crate) body: Vec<TStmt>,
}

/// A lowered statement. Only the constructs the Phase-1 subset allows.
pub(crate) enum TStmt {
    /// `let [mut] name[: ty] = init;`. `mutable` reproduces `let mut`; `annotated`
    /// records whether the source binding carried an explicit type annotation, so
    /// the emitted Rust matches the AST path byte-for-byte (an inferred binding
    /// emits no `: ty`). `ty` is always total — inferred once here at lowering if
    /// the source omitted it.
    Let {
        name: String,
        ty: Type,
        annotated: bool,
        mutable: bool,
        init: TExpr,
    },
    /// `place [op]= value;` to a plain local (subset excludes indexed assigns).
    /// `op` is the compound-assignment operator (`+=` etc.) or `None` for `=`.
    Assign {
        /// The Rust *place* string for the local, already resolved (e.g. `user_x`
        /// or `(*user_x)` for a deref'd parameter). Codegen does not re-resolve it.
        place: String,
        op: Option<BinOp>,
        value: TExpr,
    },
    Return(Option<TExpr>),
    /// A call used for effect: `print(x);`, `helper(a);`.
    ExprStmt(TExpr),
    /// Statement-form `if`/`else`. `else_body` is `None` for a bare `if`.
    If {
        cond: TExpr,
        then_body: Vec<TStmt>,
        else_body: Option<Vec<TStmt>>,
    },
}

/// A lowered expression: a resolved `Type` plus its kind. `ty` is **total** — it
/// is never absent, and codegen never recomputes it.
pub(crate) struct TExpr {
    pub(crate) ty: Type,
    pub(crate) kind: TExprKind,
}

pub(crate) enum TExprKind {
    /// Integer literal with its D-SG9 width (`None` = default `Int`/i64). The
    /// width is the elaborated `(signed, bits)` sema attached to the AST node.
    IntLit(i64, Option<(bool, u8)>),
    FloatLit(f64),
    BoolLit(bool),
    CharLit(char),
    /// String literal / interpolation. Each part is literal text or an
    /// interpolated TExpr (totally typed, like every other node).
    StrLit(Vec<TStrPart>),
    /// A local or parameter, rendered as its already-resolved Rust *place*
    /// string (handles parameter deref). No env lookup at emit time.
    Local(String),
    /// Call to a plain top-level function. Each arg carries its emit decisions.
    Call { name: String, args: Vec<TCallArg> },
    /// `print(x)` — the one builtin the subset covers.
    Print(Box<TExpr>),
    /// Binary op. `overflow` is the *computed* decision (true → emit the
    /// trapping `jet_add`/… helper). It mirrors today's `operand_is_integer`
    /// logic but is decided here, at lowering, from the total operand types.
    /// `line` is the source line of the operator, resolved at lowering, so the
    /// trapping helper's panic location matches the AST path byte-for-byte (the
    /// emitter never touches `cx.src`).
    Binary {
        op: BinOp,
        overflow: bool,
        line: u32,
        lhs: Box<TExpr>,
        rhs: Box<TExpr>,
    },
    Unary { op: UnOp, operand: Box<TExpr> },
    /// `if`-expression form (S68 / D-SG2). Both arms are value blocks.
    IfExpr {
        cond: Box<TExpr>,
        then_body: Vec<TStmt>,
        then_value: Box<TExpr>,
        else_body: Vec<TStmt>,
        else_value: Box<TExpr>,
    },
}

pub(crate) enum TStrPart {
    Lit(String),
    Interp(TExpr),
}

/// One lowered call argument, with the borrow/clone decisions already made (so
/// the emitter reproduces `emit_call_args` without consulting `cx.sigs`).
pub(crate) struct TCallArg {
    pub(crate) value: TExpr,
    /// Emit `&(...)` around the value (a String passed by `Read` convention).
    pub(crate) borrow: bool,
    /// Emit `(...).clone()` (a String passed by `Move` with an implicit clone).
    pub(crate) clone: bool,
}

// ---------------------------------------------------------------------------
// The gate: is this function fully inside the Phase-1 subset?
// ---------------------------------------------------------------------------

/// Conservative structural test: `true` only if `f` is a top-level plain
/// function whose entire body is inside the Phase-1 subset. The rule is
/// **exclude on any doubt** — a false negative just keeps the function on the
/// existing AST path (always safe), while a false positive risks an I2 bug. So
/// every check below bails to `false` the moment it sees anything unrecognised.
///
/// `cx` is consulted to exclude functions that reference program-level names the
/// subset does not lower — a comptime `const` (inlined at use) or a bare
/// function-as-value ident. Those use sites need codegen the TIR omits in Phase 1.
pub(crate) fn tir_covers(f: &Func, cx: &Cx) -> bool {
    // Signature shape: no generics, not an unsafe/pure-special function.
    if !f.type_params.is_empty() || f.is_unsafe || f.is_pure {
        return false;
    }
    // A method always has a `self` first parameter; the subset is top-level
    // functions only. (Top-level funcs never have `self`, but check anyway.)
    if f.params.iter().any(|p| p.name == Syntax::KW_SELF) {
        return false;
    }
    // `is_view_return` returns a borrow — outside the subset.
    if f.is_view_return {
        return false;
    }
    // Params must be scalars or String, with no default expressions.
    for p in &f.params {
        if p.default.is_some() || !is_subset_param_ty(&p.ty) {
            return false;
        }
    }
    // Return type, if present, must be a scalar or String (matches the body's
    // value types; the subset never returns structs/lists/options/etc.).
    if let Some(rt) = &f.return_type {
        if !is_subset_param_ty(rt) {
            return false;
        }
    }
    // Track parameter names so identifier references can be classified: a name
    // that is neither a local/param binding nor a builtin is a program-level
    // reference (const or fn-value), which the subset excludes.
    let mut locals: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
    f.body.iter().all(|s| stmt_in_subset(s, cx, &mut locals))
}

/// A param/return type the subset allows: scalar (Int/IntN/Float/F32/Bool) or
/// Char or String. Nothing compound.
fn is_subset_param_ty(ty: &Type) -> bool {
    ty.is_scalar() || matches!(ty, Type::Char | Type::String)
}

/// `locals` is the set of names bound as params/locals so far in this scope.
/// It is threaded so an `Expr::Ident` can be classified: a name that is not a
/// local must not be a const/fn-value (excluded). Bindings extend it in order.
fn stmt_in_subset(s: &Stmt, cx: &Cx, locals: &mut HashSet<String>) -> bool {
    match s {
        Stmt::Val(b) => {
            // No destructuring patterns, no comptime, no uninit/arena views.
            let ok = b.pattern.is_none()
                && !b.is_comptime
                && !b.uninit
                && !b.arena_view
                && expr_in_subset(&b.init, cx, locals);
            // The binding's name is in scope for subsequent statements.
            locals.insert(b.name.clone());
            ok
        }
        Stmt::Assign { target, value, .. } => {
            matches!(target, LValue::Local { .. }) && expr_in_subset(value, cx, locals)
        }
        Stmt::Return(Some(e), _) => expr_in_subset(e, cx, locals),
        Stmt::Return(None, _) => true,
        Stmt::Expr(e) => expr_in_subset(e, cx, locals),
        Stmt::If(ifs) => if_in_subset(ifs, cx, locals),
        // Loops, switch, unsafe, region, caps, comptime-if, context — all out.
        _ => false,
    }
}

fn if_in_subset(ifs: &IfStmt, cx: &Cx, locals: &mut HashSet<String>) -> bool {
    if !expr_in_subset(&ifs.cond, cx, locals) {
        return false;
    }
    // Each branch scopes its own bindings; check on a clone so a `let` in the
    // `then` arm doesn't leak into the `else` arm's classification.
    let mut then_locals = locals.clone();
    if !ifs.then_body.iter().all(|s| stmt_in_subset(s, cx, &mut then_locals)) {
        return false;
    }
    match &ifs.else_branch {
        None => true,
        Some(ElseBranch::Else(body)) => {
            let mut else_locals = locals.clone();
            body.iter().all(|s| stmt_in_subset(s, cx, &mut else_locals))
        }
        Some(ElseBranch::ElseIf(next)) => if_in_subset(next, cx, locals),
    }
}

fn expr_in_subset(e: &Expr, cx: &Cx, locals: &HashSet<String>) -> bool {
    match e {
        Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Char(..) => true,
        Expr::Str(parts, _) => parts.iter().all(|p| match p {
            StrPart::Lit(_) => true,
            StrPart::Interp(e) => expr_in_subset(e, cx, locals),
        }),
        // An ident must resolve to a local/param. A non-local name is a
        // program-level reference — a comptime `const` (inlined) or a bare
        // function-as-value — whose emission the Phase-1 TIR does not cover.
        Expr::Ident(name, _) => locals.contains(name),
        Expr::Unary(_, inner, _) => expr_in_subset(inner, cx, locals),
        Expr::Binary(_, l, r, _) => {
            expr_in_subset(l, cx, locals) && expr_in_subset(r, cx, locals)
        }
        Expr::Call(c) => {
            // `print` is the one builtin the subset covers (exactly one arg).
            let is_print = c.name == Syntax::BUILTIN_PRINT
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && c.args.len() == 1;
            // Otherwise the callee must be a known *plain* top-level function:
            // in `cx.sigs`, not a local, and NOT an extern/FFI function or an
            // unqualified module import (those lower to different call forms the
            // subset does not emit).
            let is_plain_fn = !locals.contains(&c.name)
                && cx.sigs.contains_key(&c.name)
                && !cx.extern_funcs.contains_key(&c.name)
                && !cx.unqualified_inline.contains_key(&c.name)
                && !cx.unqualified_file.contains_key(&c.name);
            (is_print || is_plain_fn)
                && c.args.iter().all(|a| {
                    // No labels, no shared-auto-clone (Arc) in the subset.
                    a.label.is_none()
                        && !a.flags.shared_auto_clone
                        && arg_conv_in_subset(a)
                        && expr_in_subset(&a.expr, cx, locals)
                })
        }
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            if !expr_in_subset(cond, cx, locals) {
                return false;
            }
            let mut then_locals = locals.clone();
            if !then_body.iter().all(|s| stmt_in_subset(s, cx, &mut then_locals)) {
                return false;
            }
            if !expr_in_subset(then_value, cx, &then_locals) {
                return false;
            }
            let mut else_locals = locals.clone();
            else_body.iter().all(|s| stmt_in_subset(s, cx, &mut else_locals))
                && expr_in_subset(else_value, cx, &else_locals)
        }
        // Everything else (method calls, indexing, slices, struct/enum lits,
        // collections, lambdas, ?/try, ??, fan-out, optionals, fields, …) is out.
        _ => false,
    }
}

/// A call argument is in-subset only if its convention is one the emitter
/// reproduces: a `Read` borrow or a by-`Move` value (with an optional implicit
/// clone). `Mutate` args would need `&mut place` handling we don't yet emit.
fn arg_conv_in_subset(a: &crate::AST::CallArg) -> bool {
    !matches!(a.convention, AccessConvention::Mutate)
}

// ---------------------------------------------------------------------------
// Lowering: AST -> TIR. This is where every fact is resolved ONCE.
// ---------------------------------------------------------------------------

/// Per-function lowering environment: a local name -> (Rust place string, type).
/// Built from params, extended by `let` bindings. The "place" already accounts
/// for parameter deref, so `Local` emission needs no further resolution.
struct LowerEnv {
    locals: HashMap<String, (String, Type)>,
}

impl LowerEnv {
    fn place_of(&self, name: &str) -> String {
        match self.locals.get(name) {
            Some((place, _)) => place.clone(),
            None => mangle(name),
        }
    }
    fn ty_of(&self, name: &str) -> Option<Type> {
        self.locals.get(name).map(|(_, t)| t.clone())
    }
}

pub(crate) fn lower_func(f: &Func, cx: &Cx) -> TFunc {
    let mut env = LowerEnv {
        locals: HashMap::new(),
    };
    // Mirror emit_func's parameter slot construction: a non-scalar `Read` param
    // (String, Char) is a borrow in Rust and reads as `(*name)`.
    let mut params = Vec::new();
    for p in &f.params {
        let rust_name = cx.mangle_name(&p.name);
        let place = param_place(&rust_name, p);
        env.locals
            .insert(p.name.clone(), (place, p.ty.clone()));
        params.push((rust_name, p.ty.clone(), p.convention));
    }
    let body = lower_stmts(&f.body, cx, &mut env);
    TFunc {
        name: f.name.clone(),
        params,
        ret: f.return_type.clone(),
        is_main: f.name == "main",
        body,
    }
}

/// The Rust place a parameter reads as, mirroring `emit_func`'s `deref` logic:
/// a `Read` parameter of non-scalar type (String/Char) is a `&T` and must be
/// dereferenced; `Mutate` is `&mut T` (deref'd); `Move`/scalar-`Read` is by value.
fn param_place(rust_name: &str, p: &Param) -> String {
    let deref = match p.convention {
        AccessConvention::Read if p.ty.is_scalar() => false,
        AccessConvention::Read => true,
        AccessConvention::Mutate => true,
        AccessConvention::Move => false,
    };
    if deref {
        format!("(*{})", rust_name)
    } else {
        rust_name.to_string()
    }
}

fn lower_stmts(stmts: &[Stmt], cx: &Cx, env: &mut LowerEnv) -> Vec<TStmt> {
    stmts.iter().map(|s| lower_stmt(s, cx, env)).collect()
}

fn lower_stmt(s: &Stmt, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    match s {
        Stmt::Val(b) => {
            let init = lower_expr(&b.init, cx, env);
            // Totality: if the source omitted the type, infer it ONCE here from
            // the init's already-resolved type. Codegen never infers.
            let annotated = b.ty.is_some();
            let ty = b.ty.clone().unwrap_or_else(|| init.ty.clone());
            env.locals
                .insert(b.name.clone(), (mangle(&b.name), ty.clone()));
            TStmt::Let {
                name: b.name.clone(),
                ty,
                annotated,
                mutable: b.mutable,
                init,
            }
        }
        Stmt::Assign {
            target, op, value, ..
        } => {
            let place = match target {
                LValue::Local { name, .. } => env.place_of(name),
                // Excluded by the gate; defensive.
                LValue::Index { .. } => unreachable!("indexed assign not in subset"),
            };
            TStmt::Assign {
                place,
                op: *op,
                value: lower_expr(value, cx, env),
            }
        }
        Stmt::Return(Some(e), _) => TStmt::Return(Some(lower_expr(e, cx, env))),
        Stmt::Return(None, _) => TStmt::Return(None),
        Stmt::Expr(e) => TStmt::ExprStmt(lower_expr(e, cx, env)),
        Stmt::If(ifs) => lower_if(ifs, cx, env),
        _ => unreachable!("statement not in TIR subset"),
    }
}

fn lower_if(ifs: &IfStmt, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    let cond = lower_expr(&ifs.cond, cx, env);
    // Each branch gets its own scope; bindings inside must not leak. Clone the
    // env so a `let` in the `then` arm is not visible after the `if`.
    let then_body = {
        let mut branch = clone_env(env);
        lower_stmts(&ifs.then_body, cx, &mut branch)
    };
    let else_body = match &ifs.else_branch {
        None => None,
        Some(ElseBranch::Else(body)) => {
            let mut branch = clone_env(env);
            Some(lower_stmts(body, cx, &mut branch))
        }
        // `else if` nests as an else-body holding a single `If`.
        Some(ElseBranch::ElseIf(next)) => {
            let mut branch = clone_env(env);
            Some(vec![lower_if(next, cx, &mut branch)])
        }
    };
    TStmt::If {
        cond,
        then_body,
        else_body,
    }
}

fn clone_env(env: &LowerEnv) -> LowerEnv {
    LowerEnv {
        locals: env.locals.clone(),
    }
}

fn lower_expr(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    match e {
        Expr::Int(n, _, width) => TExpr {
            ty: int_lit_type(width),
            kind: TExprKind::IntLit(*n, *width),
        },
        Expr::Float(v, _) => TExpr {
            ty: Type::Float,
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
            let ty = env.ty_of(name).unwrap_or(Type::Int);
            TExpr {
                ty,
                kind: TExprKind::Local(env.place_of(name)),
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
        Expr::Binary(op, l, r, span) => {
            let lhs = lower_expr(l, cx, env);
            let rhs = lower_expr(r, cx, env);
            // Overflow decision, computed here from total operand types — this is
            // the fact that today's `operand_is_integer` re-derives in codegen.
            // Note: `operand_is_integer` only inspects the LEFT spine of nested
            // arithmetic, so mirror that exactly (lhs first, then rhs).
            let overflow = matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
                && (lhs.ty.is_integer() || rhs.ty.is_integer());
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0 as u32;
            // A comparison/logical op yields Bool; arithmetic keeps the operand type.
            let ty = if op.is_comparison() || matches!(op, BinOp::And | BinOp::Or) {
                Type::Bool
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
            // `print` is ambient only when the user has not defined their own
            // `print` function (matches emit_call; sema enforces the shadowing).
            if call.name == Syntax::BUILTIN_PRINT && !cx.sigs.contains_key(&call.name) {
                let arg = lower_expr(&call.args[0].expr, cx, env);
                return TExpr {
                    ty: unit_type(),
                    kind: TExprKind::Print(Box::new(arg)),
                };
            }
            // Resolve the callee's signature so each arg's borrow/clone is decided
            // here, totally — mirroring `emit_call_args` for scalar/String params.
            let sig = cx.sigs.get(&call.name);
            let args = call
                .args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let value = lower_expr(&a.expr, cx, env);
                    let conv = sig.and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
                    // String (`Read`) → `&(...)`. Scalars never borrow.
                    let borrow = matches!(
                        &conv,
                        Some((AccessConvention::Read, t)) if !t.is_scalar()
                    );
                    // An implicit clone (a String passed by value).
                    let clone = a.flags.implicit_clone;
                    TCallArg {
                        value,
                        borrow,
                        clone,
                    }
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
        _ => unreachable!("expression not in TIR subset"),
    }
}

/// The type of an integer literal given its elaborated width.
fn int_lit_type(width: &Option<(bool, u8)>) -> Type {
    match width {
        Some((signed, bits)) => Type::IntN {
            signed: *signed,
            bits: *bits,
        },
        None => Type::Int,
    }
}

fn unit_type() -> Type {
    Type::Named("Unit".to_string())
}

/// The resolved return type of a called plain function: its declared return
/// type if known, else `Unit`. (In the subset, callees return scalar/String/Unit.)
/// Read from `cx.fn_types`, which sema-built `Type::Fn { ret, .. }` per function.
fn call_return_type(cx: &Cx, name: &str) -> Type {
    match cx.fn_types.get(name) {
        Some(Type::Fn { ret: Some(r), .. }) => (**r).clone(),
        _ => unit_type(),
    }
}

// ---------------------------------------------------------------------------
// Emission: TIR -> Rust. PURE formatting. No type inference, no decisions.
// ---------------------------------------------------------------------------

/// Emit a covered function from its TIR, reusing the same pure formatting helpers
/// as `emit_func` so the output is byte-identical to the AST path (golden parity).
/// The only difference is that every decision is *read off the TIR* rather than
/// recomputed — there is no `expr_jet_ty` / `operand_is_integer` call anywhere.
pub(crate) fn emit_tir_func(tir: &TFunc, cx: &Cx, out: &mut String) {
    let ret_clause = match &tir.ret {
        Some(t) => format!(" -> {}", rust_return_type(cx, t, false)),
        None => String::new(),
    };
    let params = tir
        .params
        .iter()
        .map(|(rust_name, ty, conv)| {
            format!("{}: {}", rust_name, rust_param_type(cx, *conv, ty))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let vis = if tir.is_main { "" } else { "pub " };
    // E2-M12 D-OBS1: track the current function name for rich panic reports —
    // matches `emit_func` so panic output is identical.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(&format!(
        "{vis}fn {name}({params}){ret} {{\n",
        name = cx.mangle_name(&tir.name),
        params = params,
        ret = ret_clause,
    ));
    emit_tir_stmts(&tir.body, cx, out, 1);
    out.push_str("}\n\n");
}

fn emit_tir_stmts(stmts: &[TStmt], cx: &Cx, out: &mut String, indent: usize) {
    for s in stmts {
        emit_tir_stmt(s, cx, out, indent);
    }
}

fn emit_tir_stmt(s: &TStmt, cx: &Cx, out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    match s {
        TStmt::Let {
            name,
            ty,
            annotated,
            mutable,
            init,
        } => {
            let kw = if *mutable { "let mut" } else { "let" };
            let ty_clause = if *annotated {
                format!(": {}", cx.rust_type(ty))
            } else {
                String::new()
            };
            out.push_str(&format!(
                "{}{} {}{} = {};\n",
                pad,
                kw,
                mangle(name),
                ty_clause,
                emit_tir_expr(init, cx),
            ));
        }
        TStmt::Assign { place, op, value } => {
            let v = emit_tir_expr(value, cx);
            match op {
                Some(op) => out.push_str(&format!("{}{} {}= {};\n", pad, place, op.spell(), v)),
                None => out.push_str(&format!("{}{} = {};\n", pad, place, v)),
            }
        }
        TStmt::Return(Some(e)) => {
            out.push_str(&format!("{}return {};\n", pad, emit_tir_expr(e, cx)));
        }
        TStmt::Return(None) => {
            out.push_str(&format!("{}return;\n", pad));
        }
        TStmt::ExprStmt(e) => {
            out.push_str(&format!("{}{};\n", pad, emit_tir_expr(e, cx)));
        }
        TStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            out.push_str(&format!("{}if {} {{\n", pad, emit_tir_expr(cond, cx)));
            emit_tir_stmts(then_body, cx, out, indent + 1);
            match else_body {
                None => out.push_str(&format!("{}}}\n", pad)),
                Some(body) => {
                    // Match the AST path's `else if` flattening: a one-statement
                    // else-body holding a single `If` is emitted as `} else if …`.
                    if let [TStmt::If { .. }] = body.as_slice() {
                        out.push_str(&format!("{}}} else ", pad));
                        let mut nested = String::new();
                        emit_tir_stmt(&body[0], cx, &mut nested, indent);
                        out.push_str(nested.trim_start_matches(&pad as &str));
                    } else {
                        out.push_str(&format!("{}}} else {{\n", pad));
                        emit_tir_stmts(body, cx, out, indent + 1);
                        out.push_str(&format!("{}}}\n", pad));
                    }
                }
            }
        }
    }
}

fn emit_tir_expr(e: &TExpr, cx: &Cx) -> String {
    match &e.kind {
        // D-SG9: width suffix is read straight off the literal — no re-inference.
        TExprKind::IntLit(n, width) => match width {
            Some((signed, bits)) => format!("{}{}{}", n, if *signed { 'i' } else { 'u' }, bits),
            None => format!("{}i64", n),
        },
        TExprKind::FloatLit(v) => format!("{:?}", v),
        TExprKind::BoolLit(b) => b.to_string(),
        TExprKind::CharLit(c) => format!("{:?}", c),
        TExprKind::StrLit(parts) => emit_tir_str(parts, cx),
        TExprKind::Local(place) => place.clone(),
        TExprKind::Print(arg) => {
            format!("println!(\"{{}}\", ({}).jet_show())", emit_tir_expr(arg, cx))
        }
        TExprKind::Call { name, args } => {
            let arg_str = args
                .iter()
                .map(|a| {
                    let mut s = emit_tir_expr(&a.value, cx);
                    if a.clone {
                        s = format!("({}).clone()", s);
                    }
                    if a.borrow {
                        s = format!("&({})", s);
                    }
                    s
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}({})", cx.mangle_name(name), arg_str)
        }
        TExprKind::Binary {
            op,
            overflow,
            line,
            lhs,
            rhs,
        } => {
            let ls = emit_tir_expr(lhs, cx);
            let rs = emit_tir_expr(rhs, cx);
            if *overflow {
                // Trapping helper: source location was resolved at lowering, so
                // the panic message matches the AST path exactly.
                let (file, line) = (&cx.file, *line);
                let method = match op {
                    BinOp::Add => "jet_add",
                    BinOp::Sub => "jet_sub",
                    BinOp::Mul => "jet_mul",
                    BinOp::Div => "jet_div",
                    _ => unreachable!("overflow flag only set for +,-,*,/"),
                };
                format!("({}).{}(({}), {:?}, {})", ls, method, rs, file, line)
            } else {
                format!("(({}) {} ({}))", ls, op.spell(), rs)
            }
        }
        TExprKind::Unary { op, operand } => {
            let i = emit_tir_expr(operand, cx);
            match op {
                UnOp::Neg => format!("(-({}))", i),
                UnOp::Not => format!("(!({}))", i),
            }
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            let c = emit_tir_expr(cond, cx);
            let then_block = emit_tir_value_block(then_body, then_value, cx);
            let else_block = emit_tir_value_block(else_body, else_value, cx);
            format!("if {} {} else {}", c, then_block, else_block)
        }
    }
}

fn emit_tir_value_block(stmts: &[TStmt], value: &TExpr, cx: &Cx) -> String {
    let mut inner = String::new();
    emit_tir_stmts(stmts, cx, &mut inner, 1);
    format!("{{ {} {} }}", inner, emit_tir_expr(value, cx))
}

fn emit_tir_str(parts: &[TStrPart], cx: &Cx) -> String {
    if parts.len() == 1 {
        if let TStrPart::Lit(s) = &parts[0] {
            return format!("{:?}.to_string()", s);
        }
    }
    let mut body = String::from("{ let mut _jet_s = String::new(); ");
    for p in parts {
        match p {
            TStrPart::Lit(s) => {
                if !s.is_empty() {
                    body.push_str(&format!("_jet_s.push_str({:?}); ", s));
                }
            }
            TStrPart::Interp(e) => {
                body.push_str(&format!("_jet_s.push_str(&({}).jet_show()); ", emit_tir_expr(e, cx)));
            }
        }
    }
    body.push_str("_jet_s }");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AST::Item;

    /// Parse `src` (no full sema needed — `tir_covers` is structural plus
    /// program-table lookups that `build_cx` fills) and return whether the
    /// named function is covered by the Phase-1 TIR gate.
    fn covers(src: &str, fn_name: &str) -> bool {
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        let cx = build_cx(&prog, src, "test.jet");
        let f = prog
            .items
            .iter()
            .find_map(|i| match i {
                Item::Func(f) if f.name == fn_name => Some(f),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no fn {fn_name}"));
        tir_covers(f, &cx)
    }

    #[test]
    fn covers_simple_arithmetic_fn() {
        assert!(covers("fn add(a: Int, b: Int) -> Int {\n return (a + b)\n}\n", "add"));
    }

    #[test]
    fn covers_print_and_string_param() {
        assert!(covers("fn greet(s: String) {\n print(\"hi {s}\")\n}\n", "greet"));
    }

    #[test]
    fn covers_if_else_chain() {
        let src = "fn f(n: Int) -> Int {\n if (n > 0) {\n return 1\n } else {\n return 0\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn rejects_generic_fn() {
        assert!(!covers("fn id<T>(x: T) -> T {\n return x\n}\n", "id"));
    }

    #[test]
    fn rejects_list_param() {
        // A list parameter is outside the scalar/String subset.
        assert!(!covers("fn sum(xs: [Int]) -> Int {\n return 0\n}\n", "sum"));
    }

    #[test]
    fn rejects_method_call_in_body() {
        // A method call (`.bumped()`) is not a covered construct.
        let src = "struct C { n: Int }\nimpl C {\n fn bumped(self) -> Int {\n return (self.n + 1)\n }\n}\nfn use_it(c: Int) -> Int {\n return c\n}\nfn caller() -> Int {\n x @= C { n: 1 }\n return x.bumped()\n}\n";
        assert!(!covers(src, "caller"));
    }

    #[test]
    fn rejects_loop_in_body() {
        let src = "fn f() {\n loop n in 1..3 {\n print(n)\n }\n}\n";
        assert!(!covers(src, "f"));
    }
}
