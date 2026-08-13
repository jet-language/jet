use crate::AST::{AccessConvention, BinOp, ContractClause, Expr, Func, Param, Stmt, Type};
use crate::Codegen::Cx;
use crate::Codegen::mangle;
use crate::Codegen::mangle_generated;
use crate::Codegen::rust_param_type;
use crate::Codegen::rust_return_type;
use crate::Codegen::TIR::emit_tir_stmts;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower::prepare_interrupt_callback_locals;
use crate::Codegen::TIR::lower_stmts;
use crate::Codegen::TIR::resolve_self_ty;
use crate::Codegen::TIR::SerdeCodec;
use crate::Codegen::TIR::TFunc;
use crate::Codegen::TIR::TFuncKind;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::{
    TContract, TContractDisposition, TContractKind, TExpr, TExprKind, TStmt,
};
use crate::Codegen::TIR::TWebParamReconstruction;
use crate::Syntax;
use std::collections::HashMap;

/// D-COV1: 1-based line number of a byte offset in the source, for coverage probes.
pub(crate) fn cov_line(cx: &Cx, offset: usize) -> usize {
    line_at_byte_offset(&cx.src, offset)
}

fn line_at_byte_offset(src: &str, offset: usize) -> usize {
    src.as_bytes()[..offset.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

fn bind_resource_param(
    source_name: &str,
    ty: &Type,
    convention: AccessConvention,
    cx: &Cx,
    env: &mut LowerEnv,
    guards: &mut Vec<TStmt>,
    ordinary_slot: TLocal,
) {
    let resource = matches!(convention, AccessConvention::Move)
        && matches!(ty, Type::Named(name) | Type::Apply { name, .. } if cx.close_types.contains(name));
    if !resource {
        let local_ty = match ty {
            Type::Apply { name, .. } if name == Syntax::TYPE_SHARED_GUARD => Type::Tagged {
                marker: crate::AST::TagMarker::Internal(if convention == AccessConvention::Write {
                    crate::AST::InternalTag::SharedGuardEdit
                } else {
                    crate::AST::InternalTag::SharedGuardRead
                }),
                inner: Box::new(ty.clone()),
            },
            _ => ty.clone(),
        };
        env.bind(source_name, ordinary_slot, Some(local_ty));
        return;
    }
    let guard_name = mangle_generated(&format!("resource_param_{source_name}"));
    guards.push(TStmt::Let {
        name: guard_name.clone(),
        kw: "let mut",
        let_ty: crate::Codegen::TIR::TLetTy::resource(ty.clone()),
        init: TExpr {
            ty: ty.clone(),
            kind: TExprKind::ResourceNew(Box::new(TExpr {
                ty: ty.clone(),
                kind: TExprKind::Local(TLocal::user(source_name)),
            })),
        },
                gc_promotion: None,
                gc_transferred: false,
    });
    env.bind(
        source_name,
        TLocal::user(&guard_name).through_ref(),
        Some(ty.clone()),
    );
    env.mark_resource(source_name);
}

#[cfg(test)]
mod tests {
    use super::line_at_byte_offset;

    #[test]
    fn coverage_line_accepts_offsets_inside_multibyte_prefixes() {
        let src = "é🚀—λ\nfn run() {}\n";
        for offset in 0..="é🚀—λ".len() {
            assert_eq!(line_at_byte_offset(src, offset), 1, "offset {offset}");
        }
        assert_eq!(line_at_byte_offset(src, "é🚀—λ\n".len()), 2);
        assert_eq!(line_at_byte_offset(src, usize::MAX), 3);
    }
}

pub(crate) fn lower_func(f: &Func, cx: &Cx) -> TFunc {
    lower_func_with_web_boundary(f, cx, false)
}

pub(crate) fn lower_error_conv(
    conversion: &crate::AST::ErrorConvDef,
    cx: &Cx,
) -> TFunc {
    let name = crate::Sema::error_conv_fn_name(&conversion.from_ty, &conversion.to_ty);
    let from_ty = Type::Named(conversion.from_ty.clone());
    let to_ty = Type::Named(conversion.to_ty.clone());
    let mut env = LowerEnv::new(name.clone());
    env.ret_ty = Some(to_ty.clone());
    env.bind(
        Syntax::KW_SELF,
        TLocal::user(Syntax::KW_SELF),
        Some(from_ty.clone()),
    );
    prepare_interrupt_callback_locals(&conversion.body, cx, &mut env);
    TFunc {
        name,
        source_span: conversion.from_span,
        params: vec![(
            cx.mangle_name(Syntax::KW_SELF),
            from_ty,
            AccessConvention::Move,
        )],
        web_param_reconstructions: Vec::new(),
        ret: Some(to_ty),
        gc_return: false,
        return_view_provenance: None,
        generics: String::new(),
        clone_types: Vec::new(),
        is_main: false,
        line: cov_line(cx, conversion.from_span.start),
        is_unsafe: false,
        is_pure: true,
        is_reactive: false,
        reactive_upgrades: Vec::new(),
        is_inline: false,
        is_inline_always: false,
        kernel_proof: None,
        body: lower_stmts(&conversion.body, cx, &mut env),
        kind: TFuncKind::TopLevel,
    }
}

/// Lower a web function through the same executable TIR as every other target,
/// but retain the one target-boundary fact a flattened `#WasmExport` needs:
/// an all-integer Codable struct parameter is an owned typed local inside the
/// function and scalar fields only at the external ABI. Sema already proved the
/// export type legal; this pass only materializes resolved names/types.
pub(crate) fn lower_web_func(f: &Func, cx: &Cx) -> TFunc {
    lower_func_with_web_boundary(
        f,
        cx,
        f.web_marker == Some(crate::Syntax::WebPartitionMarker::WasmExport),
    )
}

fn lower_func_with_web_boundary(f: &Func, cx: &Cx, reconstruct_web_params: bool) -> TFunc {
    let mut env = LowerEnv::new(f.name.clone());
    env.gc_return = f.gc_return;
    env.ret_ty = f.return_type.as_ref().map(|ty| cx.expand_type_aliases(ty));
    // Mirror emit_func's parameter slot construction: a non-scalar `Read` param
    // (String, Char) is a borrow in Rust and reads as `(*name)`.
    let mut params = Vec::new();
    let mut resource_param_guards = Vec::new();
    let mut web_param_reconstructions = Vec::new();
    for p in &f.params {
        let rust_name = cx.mangle_name(&p.name);
        let param_ty = cx.expand_type_aliases(&if p.variadic {
            Type::List(Box::new(p.ty.clone()))
        } else {
            p.ty.clone()
        });
        // c109 Phase 17: a param TYPED as a bare type parameter (`item: T`) is forced to
        // the `Move` convention for the slot deref (it is passed by value — `rust_param_type`
        // renders it `T`, no `&`), EXACTLY as `emit_func` forces `conv = Move` for an
        // `is_type_param` param. A param typed `Stack<T>` is NOT a type-var param — it keeps
        // its source convention (`Read` → `&__jet_Stack<T>`, deref'd place `(*__jet_s)`).
        if reconstruct_web_params {
            if let Type::Named(type_name) = &param_ty {
                if let Some(fields) = cx.struct_fields.get(type_name) {
                    if !fields.is_empty()
                        && fields
                            .iter()
                            .all(|(_, ty)| matches!(ty, Type::Int | Type::IntN { .. }))
                    {
                        let flat_fields = fields
                            .iter()
                            .map(|(field, ty)| {
                                (
                                    cx.mangle_name(field),
                                    cx.mangle_name(&format!("{}_{}", p.name, field)),
                                    ty.clone(),
                                )
                            })
                            .collect();
                        env.bind(&p.name, TLocal::user(&p.name), Some(param_ty.clone()));
                        params.push((rust_name.clone(), param_ty.clone(), p.convention));
                        web_param_reconstructions.push(TWebParamReconstruction {
                            local: TLocal::user(&p.name),
                            ty: Type::Named(type_name.to_string()),
                            fields: flat_fields,
                        });
                        continue;
                    }
                }
            }
        }
        let mut slot_param = p.clone();
        slot_param.ty = param_ty.clone();
        let place = param_place_generic(&p.name, &slot_param, &f.type_params);
        bind_resource_param(
            &p.name,
            &param_ty,
            p.convention,
            cx,
            &mut env,
            &mut resource_param_guards,
            place,
        );
        params.push((rust_name, param_ty, p.convention));
    }
    let mut body = resource_param_guards;
    prepare_interrupt_callback_locals(&f.body, cx, &mut env);
    body.extend(lower_stmts(&f.body, cx, &mut env));
    let mut clone_types = env.cloned_types.borrow().clone();
    for param in &f.params {
        collect_signature_clone_types(&param.ty, cx, &mut clone_types);
    }
    if let Some(return_type) = &f.return_type {
        collect_signature_clone_types(return_type, cx, &mut clone_types);
    }
    let generics = render_generics(&f.type_params, &clone_types);
    let body = wrap_contract_scope(
        f,
        body,
        None,
        f.return_type.as_ref().map(|ty| cx.expand_type_aliases(ty)),
        cx,
    );
    TFunc {
        name: f.name.clone(),
        source_span: f.span,
        params,
        web_param_reconstructions,
        ret: f.return_type.as_ref().map(|ty| cx.expand_type_aliases(ty)),
        gc_return: f.gc_return,
        return_view_provenance: f.return_view_provenance.clone(),
        generics,
        clone_types,
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        is_unsafe: f.is_unsafe,
        is_pure: f.is_pure,
        is_reactive: f.is_reactive,
        reactive_upgrades: f.reactive_upgrades.clone(),
        is_inline: f.is_inline,
        is_inline_always: f.is_inline_always,
        kernel_proof: f.kernel.as_ref().and_then(|marker| marker.proof),
        body,
        kind: TFuncKind::TopLevel,
    }
}

fn lower_contract_cond(
    f: &Func,
    cond: &Expr,
    result_binding: Option<(&str, &Type)>,
    owner_type: Option<&str>,
    cx: &Cx,
) -> TExpr {
    let mut env = LowerEnv::new(f.name.clone());
    env.gc_return = f.gc_return;
    for p in &f.params {
        let mut param_ty = if p.variadic {
            Type::List(Box::new(p.ty.clone()))
        } else {
            p.ty.clone()
        };
        if let Some(owner) = owner_type {
            param_ty = resolve_self_ty(&param_ty, owner);
        }
        let param_ty = cx.expand_type_aliases(&param_ty);
        let mut slot_param = p.clone();
        slot_param.ty = param_ty.clone();
        let place = if owner_type.is_some() && p.name == Syntax::KW_SELF {
            if matches!(p.convention, AccessConvention::Write) {
                TLocal::generated(Syntax::KW_SELF).through_ref()
            } else {
                TLocal::generated(Syntax::KW_SELF)
            }
        } else {
            param_place_generic(&p.name, &slot_param, &f.type_params)
        };
        env.bind(&p.name, place, Some(param_ty));
    }
    if let Some((rust_name, ty)) = result_binding {
        env.bind("result", TLocal::generated(rust_name), Some(ty.clone()));
    }
    lower_expr(cond, cx, &mut env)
}

fn contract_interval(
    expr: &Expr,
    bindings: &HashMap<String, Type>,
    cx: &Cx,
) -> Option<(i64, i64)> {
    match expr {
        Expr::Int(value, ..) => Some((*value, *value)),
        Expr::Ident(name, _) => match bindings.get(name)? {
            Type::Named(type_name) => cx.distinct_ranges.get(type_name).copied(),
            Type::Tagged { inner, .. } => match inner.as_ref() {
                Type::Named(type_name) => cx.distinct_ranges.get(type_name).copied(),
                _ => None,
            },
            _ => None,
        },
        // D-RANGETYPE1: a distinct value's raw integer projection preserves
        // the declaration interval. This fact only controls disposition;
        // the condition itself still lowers as an ordinary TIR expression.
        Expr::MethodCall {
            receiver, method, ..
        } if method == "raw" => contract_interval(receiver, bindings, cx),
        _ => None,
    }
}

fn interval_comparison(op: BinOp, left: (i64, i64), right: (i64, i64)) -> bool {
    let (left_lo, left_hi) = left;
    let (right_lo, right_hi) = right;
    match op {
        BinOp::Eq => left_lo == left_hi && left == right,
        BinOp::Ne => left_hi < right_lo || right_hi < left_lo,
        BinOp::Lt => left_hi < right_lo,
        BinOp::Gt => left_lo > right_hi,
        BinOp::Le => left_hi <= right_lo,
        BinOp::Ge => left_lo >= right_hi,
        _ => false,
    }
}

/// D-FAIL-TIER1: a range fact is a proof disposition, not an engine-local
/// optimization.  The resulting `Proven` node is retained in TIR so every
/// backend honors the same erasure disposition.
pub(crate) fn contract_expr_proven(
    expr: &Expr,
    bindings: &HashMap<String, Type>,
    cx: &Cx,
) -> bool {
    match expr {
        Expr::Bool(value, _) => *value,
        Expr::Binary(BinOp::And, left, right, _) => {
            contract_expr_proven(left, bindings, cx)
                && contract_expr_proven(right, bindings, cx)
        }
        Expr::Binary(BinOp::Or, left, right, _) => {
            contract_expr_proven(left, bindings, cx)
                || contract_expr_proven(right, bindings, cx)
        }
        Expr::Binary(op, left, right, _) if op.is_comparison() => {
            match (
                contract_interval(left, bindings, cx),
                contract_interval(right, bindings, cx),
            ) {
                (Some(left), Some(right)) => interval_comparison(*op, left, right),
                _ => false,
            }
        }
        Expr::CompareChain { operands, ops, .. } => operands
            .windows(2)
            .zip(ops)
            .all(|(pair, op)| {
                match (
                    contract_interval(&pair[0], bindings, cx),
                    contract_interval(&pair[1], bindings, cx),
                ) {
                    (Some(left), Some(right)) => interval_comparison(*op, left, right),
                    _ => false,
                }
            }),
        _ => false,
    }
}

fn contract_fact_bindings(
    f: &Func,
    result_binding: Option<(&str, &Type)>,
    owner_type: Option<&str>,
    cx: &Cx,
) -> HashMap<String, Type> {
    let mut bindings = HashMap::new();
    for param in &f.params {
        let mut ty = if param.variadic {
            Type::List(Box::new(param.ty.clone()))
        } else {
            param.ty.clone()
        };
        if let Some(owner) = owner_type {
            ty = resolve_self_ty(&ty, owner);
        }
        let ty = cx.expand_type_aliases(&ty);
        bindings.insert(param.name.clone(), ty);
    }
    if let Some((name, ty)) = result_binding {
        bindings.insert(name.to_string(), ty.clone());
        bindings.insert("result".to_string(), ty.clone());
    }
    bindings
}

fn lower_contract_clause(
    f: &Func,
    clause: &ContractClause,
    result_binding: Option<(&str, &Type)>,
    kind: TContractKind,
    owner_type: Option<&str>,
    cx: &Cx,
) -> TContract {
    let (_, line, _) = crate::Codegen::TIR::tir_src_line_at(&cx.src, clause.span.start);
    let bindings = contract_fact_bindings(f, result_binding, owner_type, cx);
    TContract {
        kind,
        condition: lower_contract_cond(f, &clause.cond, result_binding, owner_type, cx),
        message: lower_contract_cond(f, &clause.message_expr, result_binding, owner_type, cx),
        file: cx.file.clone(),
        line,
        span: clause.span,
        disposition: if contract_expr_proven(&clause.cond, &bindings, cx) {
            TContractDisposition::Proven
        } else {
            TContractDisposition::Check
        },
    }
}

fn lower_post_contracts_for_owner(
    f: &Func,
    owner_type: Option<&str>,
    cx: &Cx,
) -> Vec<TContract> {
    let ret_ty = f
        .return_type
        .as_ref()
        .map(|ty| {
            let ty = owner_type
                .map(|owner| resolve_self_ty(ty, owner))
                .unwrap_or_else(|| ty.clone());
            cx.expand_type_aliases(&ty)
        })
        .unwrap_or(Type::Named("Unit".to_string()));
    let result_name = mangle_generated("result");
    let post = f
        .post
        .iter()
        .map(|clause| {
            lower_contract_clause(
                f,
                clause,
                Some((&result_name, &ret_ty)),
                TContractKind::Post,
                owner_type,
                cx,
            )
        })
        .collect();
    post
}

fn wrap_contract_scope(
    f: &Func,
    body: Vec<TStmt>,
    owner_type: Option<&str>,
    ret: Option<Type>,
    cx: &Cx,
) -> Vec<TStmt> {
    let post = lower_post_contracts_for_owner(f, owner_type, cx);
    if post.is_empty() {
        body
    } else {
        vec![TStmt::ContractScope {
            // D-FAIL-TIER1: #Pre is a call-site node. Keeping it out of the
            // callee scope prevents duplicate checks and keeps its source
            // arrow on the caller that supplied the bad arguments.
            pre: Vec::new(),
            body,
            post,
            ret,
        }]
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
    prepare_interrupt_callback_locals(body, cx, &mut env);
    let tbody = lower_stmts(body, cx, &mut env);
    emit_tir_stmts(&tbody, cx, out, 1);
}

/// D-TEST1: lower + emit a property-test body. Identical to `emit_tir_test_body`
/// except each property parameter is bound into the env first (by its mangled
/// name, by value) so references inside the body resolve to the generated input.
/// The caller emits `fn jet_prop_N(p0: T0, …) -> Result<(), String>` so the
/// param names are real Rust locals; this binds them in the lowering env.
pub(crate) fn emit_tir_property_test_body(
    body: &[Stmt],
    params: &[Param],
    cx: &Cx,
    out: &mut String,
) {
    let mut env = LowerEnv::new(cx.current_fn.borrow().clone());
    for p in params {
        env.bind(&p.name, TLocal::user(&p.name), Some(p.ty.clone()));
    }
    prepare_interrupt_callback_locals(body, cx, &mut env);
    let tbody = lower_stmts(body, cx, &mut env);
    emit_tir_stmts(&tbody, cx, out, 1);
}

/// c109: lower + emit an error-conversion `impl Old => New { … }` body through the TIR,
/// reproducing `emit_error_conv`'s `emit_stmts(cx, body, &mut env, out, 1, false)`
/// byte-for-byte. `emit_error_conv` already emitted the signature + opening brace and set
/// `cx.current_fn` to the conversion fn name; it binds `self` to its canonical machine name
/// (Move, the Old named type), so the env's `self` place is that same machine name. The body's
/// `return <e>` lowers the expr as-is (sema
/// already inserted any wrapping); emitted at indent 1, the closing brace is the caller's.
pub(crate) fn emit_tir_error_conv_body(body: &[Stmt], from_ty: &str, cx: &Cx, out: &mut String) {
    let mut env = LowerEnv::new(cx.current_fn.borrow().clone());
    env.bind(
        Syntax::KW_SELF,
        // Error-conversion parameters are ordinary generated locals, not Rust
        // receiver syntax; keep the binding aligned with `emit_error_conv`.
        TLocal::generated("__self"),
        Some(Type::Named(from_ty.to_string())),
    );
    prepare_interrupt_callback_locals(body, cx, &mut env);
    let tbody = lower_stmts(body, cx, &mut env);
    emit_tir_stmts(&tbody, cx, out, 1);
}

/// Render a Rust generic clause with `Clone` only for type parameters reached by
/// an actual lowered clone. Read-only generic functions remain usable with
/// non-Clone values such as callbacks and trait objects.
pub(crate) fn render_generics(
    type_params: &[crate::AST::TypeParam],
    cloned_types: &[Type],
) -> String {
    if type_params.is_empty() {
        return String::new();
    }
    let names: std::collections::HashSet<&str> =
        type_params.iter().map(|p| p.name.as_str()).collect();
    let mut cloned = std::collections::HashSet::new();
    for ty in cloned_types {
        crate::Generics::collect_clone_type_param_mentions(ty, &names, &mut cloned);
    }
    let extra = cloned
        .into_iter()
        .map(|name| (name, vec!["Clone".to_string()]))
        .collect();
    crate::Generics::rust_type_param_list(type_params, &extra)
}

/// Collect the concrete arguments that a derived `Clone` implementation
/// requires. A generic nominal can be nested under any container or another
/// generic nominal, so the walk is structural and uses only canonical Cx keys.
fn collect_signature_clone_types(ty: &Type, cx: &Cx, out: &mut Vec<Type>) {
    let expanded = cx.expand_type_aliases(ty);
    match &expanded {
        Type::Apply { name, args } => {
            let carries_clone_bound = cx
                .struct_type_params
                .get(name)
                .is_some_and(|params| !params.is_empty())
                && cx.cloneable.contains(name);
            if carries_clone_bound {
                out.extend(args.iter().cloned());
            }
            for arg in args {
                collect_signature_clone_types(arg, cx, out);
            }
        }
        Type::List(inner)
        | Type::Option(inner)
        | Type::FixedList { elem: inner, .. }
        | Type::Tagged { inner, .. }
        | Type::Quantity { base: inner, .. } => {
            collect_signature_clone_types(inner, cx, out);
        }
        Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
            collect_signature_clone_types(key, cx, out);
            collect_signature_clone_types(value, cx, out);
        }
        Type::Shared(_) | Type::Fn { .. } | Type::TraitObject(_) => {}
        Type::Tuple(fields) => {
            for (_, field) in fields {
                collect_signature_clone_types(field, cx, out);
            }
        }
        Type::Union(members) => {
            for member in members {
                collect_signature_clone_types(member, cx, out);
            }
        }
        _ => {}
    }
}
/// c109 Phase 17: `param_place` for a (possibly generic) free function.
/// Generic parameters preserve their declared access convention exactly like
/// concrete parameters; `&stream: T` therefore dereferences its Rust `&mut T`.
pub(crate) fn param_place_generic(
    name: &str,
    p: &Param,
    _type_params: &[crate::AST::TypeParam],
) -> TLocal {
    param_place(name, p)
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
    let owner_ty = match cx.struct_type_param_order.get(type_name) {
        Some(params) if !params.is_empty() => Type::Apply {
            name: type_name.to_string(),
            args: params.iter().cloned().map(Type::Named).collect(),
        },
        _ => Type::Named(type_name.to_string()),
    };
    lower_method_for_owner(f, type_name, owner_ty, cx)
}

pub(crate) fn lower_method_for_owner(
    f: &Func,
    type_name: &str,
    owner_ty: Type,
    cx: &Cx,
) -> TFunc {
    let previous_type_params = cx.current_type_params.borrow().clone();
    let mut method_type_params = previous_type_params.clone();
    method_type_params.extend(f.type_params.iter().map(|param| param.name.clone()));
    cx.current_type_params.replace(method_type_params);
    let mut env = LowerEnv::new(f.name.clone());
    env.gc_return = f.gc_return;
    env.ret_ty = f.return_type.clone();
    env.self_owner = Some(type_name.to_string());
    let mut params = Vec::new();
    let mut resource_param_guards = Vec::new();
    let mut self_conv: Option<AccessConvention> = None;
    let mut is_static = true;
    for p in &f.params {
        if p.name == Syntax::KW_SELF {
            // The self slot, parity with `emit_method`: place `self`, type None. A
            // `mut self` receiver is `&mut Self`, so its place DEREFS (`(*self)`) —
            // `self.field = v` → `((*self)).field = v`, whole-`self` `self = New{}` →
            // `(*self) = New{}` (D-MUTSELF1). `self`/`take self` carry no deref.
            let place = if matches!(p.convention, AccessConvention::Write) {
                TLocal::generated("self").through_ref()
            } else {
                TLocal::generated("self")
            };
            env.bind(Syntax::KW_SELF, place, Some(owner_ty.clone()));
            if matches!(p.convention, AccessConvention::Read) {
                env.mark_borrowed(Syntax::KW_SELF);
            }
            self_conv = Some(p.convention);
            is_static = false;
            continue;
        }
        let rust_name = mangle(&p.name);
        let place = param_place(&p.name, p);
        // A `Self`-typed param resolves to the owning type for totality.
        let pty = resolve_self_ty(&p.ty, type_name);
        let pty = if p.variadic {
            Type::List(Box::new(pty))
        } else {
            pty
        };
        bind_resource_param(
            &p.name,
            &pty,
            p.convention,
            cx,
            &mut env,
            &mut resource_param_guards,
            place,
        );
        params.push((rust_name, pty, p.convention));
    }
    let mut body = resource_param_guards;
    prepare_interrupt_callback_locals(&f.body, cx, &mut env);
    body.extend(lower_stmts(&f.body, cx, &mut env));
    let body = wrap_contract_scope(
        f,
        body,
        Some(type_name),
        f.return_type
            .as_ref()
            .map(|ty| resolve_self_ty(&cx.expand_type_aliases(ty), type_name)),
        cx,
    );
    let mut clone_types = env.cloned_types.borrow().clone();
    collect_signature_clone_types(&owner_ty, cx, &mut clone_types);
    for param in &f.params {
        collect_signature_clone_types(&param.ty, cx, &mut clone_types);
    }
    if let Some(return_type) = &f.return_type {
        collect_signature_clone_types(return_type, cx, &mut clone_types);
    }
    let generics = render_generics(&f.type_params, &clone_types);
    cx.current_type_params.replace(previous_type_params);
    // An instance method carries `Some(conv)`; a static method carries `None`.
    let kind = TFuncKind::Method {
        self_conv: if is_static { None } else { self_conv },
        owner_type: owner_ty,
    };
    TFunc {
        name: f.name.clone(),
        source_span: f.span,
        params,
        web_param_reconstructions: Vec::new(),
        ret: f
            .return_type
            .as_ref()
            .map(|t| resolve_self_ty(t, type_name)),
        gc_return: f.gc_return,
        return_view_provenance: f.return_view_provenance.clone(),
        // The enclosing owner params live on `impl<T>`. Method-owned params
        // remain on the method itself; `emit_type_impl` appends any owner
        // `Clone` bounds required by this body.
        generics,
        clone_types,
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        is_unsafe: f.is_unsafe,
        is_pure: f.is_pure,
        is_reactive: f.is_reactive,
        reactive_upgrades: f.reactive_upgrades.clone(),
        is_inline: f.is_inline,
        is_inline_always: f.is_inline_always,
        kernel_proof: f.kernel.as_ref().and_then(|marker| marker.proof),
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
///
/// D-SERDE2 (card #131 S1-bridge): `trait_name` selects a codec bridge when it is
/// `Encode`/`Decode` — a hand `impl T.Encode`/`impl T.Decode` whose user-facing
/// `encode`/`decode` verbs + Jet signatures must lower to the Rust trait's
/// `jet_encode`/`jet_decode`. `Encode` is an ordinary instance method (`&self`),
/// only its NAME is bridged. `Decode` is STATIC: the by-value `tree: Data` param
/// binds as an owned local (a clone the emit prepends), so its place is the bare
/// mangled name — no receiver, no `param_place` deref.
pub(crate) fn lower_trait_method(f: &Func, type_name: &str, cx: &Cx, trait_name: &str) -> TFunc {
    let serde = match trait_name {
        crate::Generics::ENCODE => Some(SerdeCodec::Encode),
        crate::Generics::DECODE => Some(SerdeCodec::Decode),
        _ => None,
    };
    let owner_ty = match cx.struct_type_param_order.get(type_name) {
        Some(params) if !params.is_empty() => Type::Apply {
            name: type_name.to_string(),
            args: params.iter().cloned().map(Type::Named).collect(),
        },
        _ => Type::Named(type_name.to_string()),
    };
    let mut env = LowerEnv::new(f.name.clone());
    env.gc_return = f.gc_return;
    env.ret_ty = f.return_type.clone();
    env.self_owner = Some(type_name.to_string());
    let mut params = Vec::new();
    let mut resource_param_guards = Vec::new();
    let mut self_conv = AccessConvention::Read;
    for p in &f.params {
        if p.name == Syntax::KW_SELF {
            self_conv = p.convention;
            // The self slot, EXACTLY `emit_trait_method`'s: type `Some(Named(type_name))`
            // (NOT `None` like `emit_method`). D-MUTSELF1: a `mut self` receiver is
            // `&mut self`, so its place DEREFS (`(*self)`); `self`/`take self` do not.
            let place = if matches!(p.convention, AccessConvention::Write) {
                TLocal::generated("self").through_ref()
            } else {
                TLocal::generated("self")
            };
            env.bind(
                Syntax::KW_SELF,
                place,
                Some(owner_ty.clone()),
            );
            if matches!(p.convention, AccessConvention::Read) {
                env.mark_borrowed(Syntax::KW_SELF);
            }
            continue;
        }
        let rust_name = cx.mangle_name(&p.name);
        // D-SERDE2: a `Decode.decode(tree: Data)` param is emitted as `&jet_std::DataTree`
        // and re-bound to an owned clone at the function head, so the body sees an owned
        // `Data` local — its place is the bare name, NOT `param_place`'s non-scalar deref.
        let place = if serde == Some(SerdeCodec::Decode) {
            TLocal::user(&p.name)
        } else {
            param_place(&p.name, p)
        };
        let pty = resolve_self_ty(&p.ty, type_name);
        bind_resource_param(
            &p.name,
            &pty,
            p.convention,
            cx,
            &mut env,
            &mut resource_param_guards,
            place,
        );
        params.push((rust_name, pty, p.convention));
    }
    let mut body = resource_param_guards;
    prepare_interrupt_callback_locals(&f.body, cx, &mut env);
    body.extend(lower_stmts(&f.body, cx, &mut env));
    let body = wrap_contract_scope(
        f,
        body,
        Some(type_name),
        f.return_type
            .as_ref()
            .map(|ty| resolve_self_ty(&cx.expand_type_aliases(ty), type_name)),
        cx,
    );
    let mut clone_types = env.cloned_types.borrow().clone();
    collect_signature_clone_types(&owner_ty, cx, &mut clone_types);
    for param in &f.params {
        collect_signature_clone_types(&param.ty, cx, &mut clone_types);
    }
    if let Some(return_type) = &f.return_type {
        collect_signature_clone_types(return_type, cx, &mut clone_types);
    }
    let generics = render_generics(&f.type_params, &clone_types);
    TFunc {
        name: f.name.clone(),
        source_span: f.span,
        params,
        web_param_reconstructions: Vec::new(),
        ret: f
            .return_type
            .as_ref()
            .map(|t| resolve_self_ty(t, type_name)),
        gc_return: f.gc_return,
        return_view_provenance: f.return_view_provenance.clone(),
        generics,
        clone_types,
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        // The trait-method `unsafe` prefix rides on `TFuncKind::TraitMethod.is_unsafe`
        // (the dedicated trait-method emit reads it there); the top-level flag is unused
        // for this kind, but keep it consistent.
        is_unsafe: f.is_unsafe,
        is_pure: f.is_pure,
        is_reactive: f.is_reactive,
        reactive_upgrades: f.reactive_upgrades.clone(),
        is_inline: f.is_inline,
        is_inline_always: f.is_inline_always,
        kernel_proof: f.kernel.as_ref().and_then(|marker| marker.proof),
        body,
        kind: TFuncKind::TraitMethod {
            is_unsafe: f.is_unsafe,
            self_conv,
            serde,
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
/// name (no `__jet_` mangle, as the trait owns it in Rust).
pub(crate) fn lower_delegation_method(f: &Func, field: &str, cx: &Cx) -> TFunc {
    let ret = f
        .return_type
        .as_ref()
        .map(|t| rust_return_type(cx, t))
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
                format!(
                    "{}: {}",
                    mangle(&p.name),
                    rust_param_type(cx, p.convention, &p.ty)
                )
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
        source_span: f.span,
        params: Vec::new(),
        web_param_reconstructions: Vec::new(),
        ret: f.return_type.clone(),
        gc_return: f.gc_return,
        return_view_provenance: f.return_view_provenance.clone(),
        // The signature is fully pre-rendered (`sig`); `is_view`/`generics` are unused for delegation.
        generics: String::new(),
        clone_types: Vec::new(),
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        // A delegation method has no body and never carries `#Unsafe fn` (sema rejects it).
        // Same for `#Inline`/`#Inline(Always)` — a delegation method is pure forwarding,
        // never parsed with an inline marker.
        is_unsafe: false,
        is_pure: false,
        is_reactive: false,
        reactive_upgrades: Vec::new(),
        is_inline: false,
        is_inline_always: false,
        kernel_proof: None,
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
pub(crate) fn param_place(name: &str, p: &Param) -> TLocal {
    let deref = match p.convention {
        AccessConvention::Read if p.ty.is_scalar() => {
            false
        }
        AccessConvention::Read => true,
        AccessConvention::Write => true,
        AccessConvention::Move => false,
    };
    let slot = TLocal::user(name);
    if deref {
        slot.through_ref()
    } else {
        slot
    }
}
