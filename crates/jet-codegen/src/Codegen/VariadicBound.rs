//! D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): trait-bounded heterogeneous variadic
//! parameters (`parts: ...Renderable` / `parts: ...[A, B]`).
//!
//! S48 says a trait name in type position means boxed dynamic dispatch, and
//! `<T: Trait>` means monomorphization — the ballot wants zero boxing, so this
//! has to be the second shape, not the first. Rust has no variadic generics,
//! so one Jet function with a trait-bounded rest parameter becomes **one
//! specialized Rust function per call-site arity**: `log_all(a, b)` and
//! `log_all(a, b, c)` each get their own monomorphic-per-instantiation
//! function (`user_log_all__va2`, `user_log_all__va3`), each with that many
//! fresh generic type parameters bound to the trait — exactly the same Rust
//! generics + rustc-monomorphization "existing machinery" an ordinary
//! `fn f<T: Trait>(x: T)` already relies on. Every call sharing an arity
//! shares one specialized function; rustc does the per-type-argument
//! monomorphization for free.
//!
//! The one open problem a plain per-arity generic function doesn't solve on
//! its own: the *body*. `parts` was written as a single name the source
//! iterates (`loop p; parts { … }`), but there is no zero-cost Rust value
//! that stands for "N values of N different (trait-bounded) types" — a real
//! `Vec`/tuple can't express it without boxing or macros. So sema restricts a
//! trait-bounded variadic's body to exactly one shape it CAN compile away for
//! free (`Sema/Registration.rs::check_variadic_bound_body_shape`, E1314): a
//! single top-level `loop x; parts { … }` loop. Here, that loop is unrolled
//! into `arity` copies, the loop variable rebound to each synthetic parameter
//! in turn — after which the synthesized function is an entirely ordinary
//! generic Jet function, run through the *unmodified* `emit_func` /
//! TIR-lowering path (I3: codegen for this feature is exactly zero new TIR
//! node kinds — it's AST synthesis feeding the existing generic-function
//! pipeline).

use super::*;
use crate::AST::{Binding, Expr, ForKind, Func, Param, Stmt, Type, TypeParam};

/// D-ANY-JAI1: the per-arity specialized Rust-function name shared by the
/// call-site router (`lower_variadic_bound_call`, below) and the
/// definition-side synthesizer (`build_variadic_bound_func`, below) — both
/// must agree so the call and its callee's `cx.mangle_name` land on the same
/// Rust symbol.
pub(crate) fn variadic_bound_fn_name(base: &str, arity: usize) -> String {
    format!("{base}__va{arity}")
}

/// D-ANY-JAI1: lower a call to a trait-bounded variadic function. Sema left
/// the trailing arguments as individual `call.args` entries past the fixed
/// prefix (`Sema/CheckerInfer/calls.rs::check_variadic_bound_tail`) — no
/// packed list, since a heterogeneous tail has no single element type a real
/// list literal could carry — so the arity is just `call.args.len() - fixed`.
/// Records the arity into `cx.needed_variadic_arities` (read back by
/// `emit_variadic_bound_specializations` once every call site in the program
/// has been lowered) and routes the call to that arity's specialized
/// function.
pub(crate) fn lower_variadic_bound_call(
    call: &crate::AST::Call,
    fixed: usize,
    cx: &Cx,
    env: &mut crate::Codegen::TIR::LowerEnv,
) -> crate::Codegen::TIR::TExpr {
    use crate::Codegen::TIR::{call_return_type, lower_one_call_arg, TExpr, TExprKind};
    let sig = cx.sigs.get(&call.name).cloned();
    let tail_convention = sig
        .as_ref()
        .and_then(|params| params.get(fixed))
        .map(|(convention, _)| *convention)
        .expect("trait-bounded variadic signature must retain its tail convention");
    let arity = call.args.len().saturating_sub(fixed);
    cx.needed_variadic_arities
        .borrow_mut()
        .entry(call.name.clone())
        .or_default()
        .insert(arity);
    let args: Vec<crate::Codegen::TIR::TCallArg> = call
        .args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            // The fixed prefix uses its declared signature. Each synthetic tail
            // slot keeps the original variadic parameter's access convention and
            // has a fresh generic type. The generated specialization renders a
            // default-Read tail as `&JaiVarN`, so the call must borrow every tail
            // argument, including scalar concrete instantiations.
            let conv = if i < fixed {
                sig.as_ref()
                    .and_then(|ps| ps.get(i))
                    .map(|(c, t)| (*c, t.clone()))
            } else {
                Some((
                    tail_convention,
                    Type::Named(format!("JaiVar{}", i - fixed)),
                ))
            };
            lower_one_call_arg(a, conv, env, cx)
        })
        .collect();
    cx.jit_generic_calls
        .borrow_mut()
        .entry(variadic_bound_fn_name(&call.name, arity))
        .or_default()
        .push(args.iter().map(|arg| arg.value.ty.clone()).collect());
    let ret = call_return_type(cx, &call.name);
    TExpr {
        ty: ret,
        kind: TExprKind::Call {
            name: variadic_bound_fn_name(&call.name, arity),
            type_args: Vec::new(),
            args,
        },
    }
}

/// D-ANY-JAI1: after the main function-emission pass has run (and so has
/// discovered, via every call site it lowered, every `(fn, arity)` pair a
/// trait-bounded variadic function is actually called with — see
/// `Cx::needed_variadic_arities`'s doc comment), emit one specialized Rust
/// function per pair.
pub(crate) fn emit_variadic_bound_specializations(cx: &Cx, items: &[Item], out: &mut String) {
    let needed = cx.needed_variadic_arities.borrow();
    if needed.is_empty() {
        return;
    }
    for (fn_name, arities) in needed.iter() {
        let Some((_, bounds)) = cx.variadic_bound_fns.get(fn_name) else {
            continue;
        };
        let Some(f) = items.iter().find_map(|it| match it {
            Item::Func(f) if &f.name == fn_name => Some(f),
            _ => None,
        }) else {
            continue;
        };
        for &arity in arities {
            let specialized = build_variadic_bound_func(f, bounds, arity);
            crate::Codegen::Items::emit_func(cx, &specialized, out);
        }
    }
}

/// D-ANY-JAI1: build the specialized Rust-backing `Func` for `f` (a
/// trait-bounded variadic function) at one call-site `arity` — `arity` fresh
/// generic type parameters (each bound to `bounds`) replace the trailing
/// variadic parameter, one per call-site argument, and the body's one legal
/// `loop x; <variadic> { … }` loop is unrolled to match.
pub(crate) fn build_variadic_bound_func(f: &Func, bounds: &[String], arity: usize) -> Func {
    let last = f
        .params
        .last()
        .expect("internal compiler error: trait-bounded variadic func has no trailing param (I2/D-ANY-JAI1)");
    let mut params: Vec<Param> = f.params[..f.params.len() - 1].to_vec();
    let mut type_params = f.type_params.clone();
    for i in 0..arity {
        let tname = format!("JaiVar{i}");
        type_params.push(TypeParam {
            name: tname.clone(),
            name_span: last.name_span,
            bounds: bounds.to_vec(),
        });
        params.push(Param {
            convention: last.convention,
            root: false,
            name: variadic_slot_name(&last.name, i),
            name_span: last.name_span,
            ty: Type::Named(tname),
            ty_span: last.ty_span,
            default: None,
            variadic: false,
            variadic_bound_list: None, declared_view_from_names: None, public_label: None, zone: crate::AST::ParamZone::Either,
        });
    }
    let body = match unroll_variadic_body(&f.body, &last.name, arity) {
        Ok(body) => body,
        Err(msg) => jet_foundation::ice!(
            None,
            "trait-bounded variadic `{}` — {} — codegen only covers a \
             single top-level `loop x; {}` loop; sema's E1314 (Sema/Registration.rs::\
             check_variadic_bound_body_shape) should have rejected this body already (D-ANY-JAI1)",
            f.name, msg, last.name
        ),
    };
    Func {
        name: variadic_bound_fn_name(&f.name, arity),
        type_params,
        params,
        body,
        ..f.clone()
    }
}

fn variadic_slot_name(param_name: &str, i: usize) -> String {
    format!("{param_name}__va{i}")
}

/// D-ANY-JAI1: replace the (sema-guaranteed unique, top-level) `loop x in
/// target { … }` loop with `arity` unrolled copies, the loop variable rebound
/// to each synthetic slot (`variadic_slot_name`) in turn. `Err` names the
/// unsupported shape found — an internal-compiler-error backstop; sema's
/// E1314 should already have rejected every case that reaches it, but this
/// function makes zero assumptions codegen can't verify locally (never
/// silently emits wrong Rust, I2).
fn unroll_variadic_body(stmts: &[Stmt], target: &str, arity: usize) -> Result<Vec<Stmt>, String> {
    let mut out = Vec::new();
    let mut seen_loop = false;
    for s in stmts {
        match s {
            Stmt::For {
                var,
                var_span,
                var2: None,
                kind: ForKind::In { collection, .. },
                body,
                label: None,
                ..
            } if matches!(collection, Expr::Ident(n, _) if n == target) => {
                if seen_loop {
                    return Err(format!("more than one `for … in {target}` loop"));
                }
                seen_loop = true;
                for i in 0..arity {
                    out.push(Stmt::Val(Binding {
                        mutable: false,
                        markers: Vec::new(),
                reactive_upgrade: false,
                        meta: None,
                        name: var.clone(),
                        name_span: *var_span,
                        pattern: None,
                        ty: None,
                        ty_span: None,
                        init: Expr::Ident(variadic_slot_name(target, i), *var_span),
                        is_comptime: false,
                        ct: None,
                        uninit: false,
                        arena_view: false,
                string_view: false,
                gc_promotion: None,
                gc_transferred: false,
                    }));
                    out.extend(body.clone());
                }
            }
            _ => {
                if stmt_references_ident(s, target) {
                    return Err(format!(
                        "`{target}` used outside a `loop x; {target} {{ … }}` loop"
                    ));
                }
                out.push(s.clone());
            }
        }
    }
    Ok(out)
}

/// Best-effort (mirrors `Sema/Registration.rs::scan_stmt_for_variadic_uses`'s
/// coverage — not exhaustive over every `Stmt`/`Expr` variant) check for
/// whether `name` is referenced anywhere in `s`.
fn stmt_references_ident(s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::Expr(e) | Stmt::Return(Some(e), _) => expr_references_ident(e, name),
        Stmt::Val(b) => expr_references_ident(&b.init, name),
        Stmt::Assign { target, value, .. } => {
            let target_hit = match target {
                crate::AST::LValue::Index { base, index, .. } => {
                    expr_references_ident(base, name) || expr_references_ident(index, name)
                }
                crate::AST::LValue::Field { base, .. } => expr_references_ident(base, name),
                crate::AST::LValue::Local { name: n, .. } => n == name,
            };
            target_hit || expr_references_ident(value, name)
        }
        Stmt::While { cond, body, .. } => {
            expr_references_ident(cond, name) || body.iter().any(|s| stmt_references_ident(s, name))
        }
        Stmt::For { kind, body, .. } => {
            let kind_hit = match kind {
                ForKind::Range { start, end, step, exclusive: _ } => {
                    expr_references_ident(start, name)
                        || expr_references_ident(end, name)
                        || step
                            .as_ref()
                            .is_some_and(|s| expr_references_ident(s, name))
                }
                ForKind::In { collection, step } => expr_references_ident(collection, name)
                    || step.as_ref().is_some_and(|s| expr_references_ident(s, name)),
            };
            kind_hit || body.iter().any(|s| stmt_references_ident(s, name))
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            expr_references_ident(subject, name)
                || arms.iter().any(|a| {
                    expr_references_ident(&a.cond, name)
                        || a.body.iter().any(|s| stmt_references_ident(s, name))
                })
                || else_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| stmt_references_ident(s, name)))
        }
        Stmt::Loop { body, .. } => body.iter().any(|s| stmt_references_ident(s, name)),
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            expr_references_ident(&init.init, name)
                || expr_references_ident(cond, name)
                || step.as_ref().is_some_and(|step| stmt_references_ident(step, name))
                || body.iter().any(|s| stmt_references_ident(s, name))
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::BreakLabel(_, _) | Stmt::ContinueLabel(_, _) => false,
        // Every other statement kind (lexical-scope wrappers like `#Unsafe { }`,
        // `region`, `#Transact`, `#Known { }`, …) — conservatively assume a
        // reference so an unsupported-but-undetected body shape becomes a loud
        // internal-compiler-error, never silently-wrong Rust (I2).
        _ => true,
    }
}

fn expr_references_ident(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Ident(n, _) => n == name,
        Expr::Call(c) => c.args.iter().any(|a| expr_references_ident(&a.expr, name)),
        Expr::MethodCall { receiver, args, .. } => {
            expr_references_ident(receiver, name)
                || args.iter().any(|a| expr_references_ident(&a.expr, name))
        }
        Expr::CallValue { callee, args, .. } => {
            expr_references_ident(callee, name)
                || args.iter().any(|a| expr_references_ident(&a.expr, name))
        }
        Expr::Binary(_, l, r, _) => {
            expr_references_ident(l, name) || expr_references_ident(r, name)
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
        | Expr::Try(inner, _, _) => expr_references_ident(inner, name),
        Expr::OptField { base, .. } => expr_references_ident(base, name),
        Expr::Index { base, index, .. } => {
            expr_references_ident(base, name) || expr_references_ident(index, name)
        }
        Expr::Slice {
            base, start, end, range, ..
        } => {
            expr_references_ident(base, name)
                || range.as_deref().map_or_else(
                    || {
                        expr_references_ident(start, name)
                            || expr_references_ident(end, name)
                    },
                    |range| expr_references_ident(range, name),
                )
        }
        Expr::ListLit(items, _) => items.iter().any(|i| expr_references_ident(i, name)),
        Expr::MapLit(pairs, _) => pairs
            .iter()
            .any(|(k, v)| expr_references_ident(k, name) || expr_references_ident(v, name)),
        Expr::StructLit { fields, .. } => fields
            .iter()
            .any(|(_, _, v)| expr_references_ident(v, name)),
        Expr::TypedLit { body, .. } => {
            let mut hit = false;
            body.for_each_expr(|v| {
                if expr_references_ident(v, name) {
                    hit = true;
                }
            });
            hit
        }
        Expr::TupleLit(fields, _, _) => fields.iter().any(|(_, v)| expr_references_ident(v, name)),
        Expr::EnumLit { args, .. } => args.iter().any(|a| {
            let e = match a {
                crate::AST::EnumLitArg::Positional(e) => e,
                crate::AST::EnumLitArg::Named { expr, .. } => expr,
            };
            expr_references_ident(e, name)
        }),
        Expr::Str(parts, _) => parts.iter().any(|p| match p {
            crate::AST::StrPart::Interp(inner, _) => expr_references_ident(inner, name),
            crate::AST::StrPart::Lit(_) => false,
        }),
        Expr::Int(..)
        | Expr::Float(..)
        | Expr::Bool(..)
        | Expr::Char(..)
        | Expr::ReduceMarker(..) => false,
        // Anything else (lambdas, comprehensions, closures, …) is conservatively
        // "yes, might reference it" — this function only exists to turn a body
        // shape sema's E1314 somehow missed into a loud internal-compiler-error
        // instead of silently-wrong Rust (I2), so an over-eager panic is the
        // safe failure direction, never an under-eager "looks fine".
        _ => true,
    }
}
