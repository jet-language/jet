#![allow(dead_code)]
use crate::Codegen::mangle_generated;
use crate::Codegen::Cx;
use crate::Codegen::TIR::lower::lower_value_block;
use crate::Codegen::TIR::lower::note_stack_sentry_in_tir;
use crate::Codegen::TIR::lower::prepare_interrupt_callback_locals;
use crate::Codegen::TIR::lower::return_type_has_value;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_stmts;
use crate::Codegen::TIR::resolve_self_ty;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::SerdeCodec;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TUnsafeGate;
use crate::Codegen::TIR::TWebParamReconstruction;
use crate::Codegen::TIR::{
    function_effect_facts, function_failure_carrier, function_foreign_provenance,
    function_semantic_key, function_target_applicability, function_visibility, TContract,
    TContractDisposition, TContractKind, TContractResult, TContractResultMode, TEffectFacts, TExpr,
    TExprKind, TFailureCarrier, TFunc, TFuncKind, TGenericParam, TStmt, TTargetApplicability,
    TVisibility,
};
use crate::Syntax;
use crate::AST::{AccessConvention, BinOp, ContractClause, Expr, Func, Param, Type};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

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

/// Construct the checked identity for a method implementation. The owner is
/// part of the identity because the source leaf (`equal`, `compare`, etc.) is
/// shared by every type in a module. Keep operator trait identity (including
/// its RHS) when sema supplied it; generated protocol impls do not always have
/// a normalized RHS yet, so the owner-plus-trait spelling remains the safe
/// fallback.
fn method_semantic_key(
    module: &str,
    owner: &Type,
    trait_name: Option<&str>,
    operator_rhs: Option<&Type>,
    method: &str,
) -> String {
    let owner_name = owner.name();
    let method_name = match trait_name {
        Some(trait_name)
            if matches!(
                trait_name,
                crate::Syntax::TRAIT_ADD
                    | crate::Syntax::TRAIT_SUB
                    | crate::Syntax::TRAIT_MUL
                    | crate::Syntax::TRAIT_DIV
                    | crate::Syntax::TRAIT_EQUATABLE
                    | crate::Syntax::TRAIT_COMPARABLE
            ) =>
        {
            crate::Traits::operator_method_identity(
                &owner_name,
                trait_name,
                method,
                operator_rhs.unwrap_or(owner),
            )
        }
        Some(trait_name) => format!("{owner_name}::{trait_name}::{method}"),
        None => format!("{owner_name}::{method}"),
    };
    function_semantic_key(module, &method_name)
}

fn generic_params_from(type_params: &[crate::AST::TypeParam]) -> Vec<TGenericParam> {
    type_params
        .iter()
        .map(|param| TGenericParam {
            name: param.name.clone(),
            bounds: param.bounds.clone(),
        })
        .collect()
}

fn type_param_names(f: &Func) -> Vec<String> {
    f.type_params
        .iter()
        .map(|param| param.name.clone())
        .collect()
}

fn default_target_applicability() -> TTargetApplicability {
    TTargetApplicability {
        rust_aot: true,
        cranelift: true,
        interpreter: true,
        web: true,
    }
}

fn sentries_enabled_for_function(f: &Func, cx: &Cx) -> bool {
    let declarations = cx
        .policy_declarations
        .iter()
        .filter(|declaration| {
            declaration.key == crate::Policy::PolicyKey::Sentries
                && (matches!(
                    declaration.scope,
                    crate::Policy::PolicyScope::Organization
                        | crate::Policy::PolicyScope::Package
                        | crate::Policy::PolicyScope::Module
                ) || declaration.target == Some(f.span))
        })
        .cloned();
    crate::Policy::resolve(crate::Policy::PolicyKey::Sentries, declarations)
        .ok()
        .flatten()
        .is_none_or(|policy| policy.value == crate::Policy::PolicyValue::On)
}

fn unsafe_gate(f: &Func, cx: &Cx, enabled: bool) -> Option<TUnsafeGate> {
    f.is_unsafe.then(|| {
        let span = f.unsafe_span.unwrap_or(f.span);
        TUnsafeGate {
            file: cx.file.clone(),
            line: cov_line(cx, span.start) as u32,
            reason: f.unsafe_reason.clone().unwrap_or_default(),
            enabled,
            fenced: cx.dependency_fenced,
        }
    })
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

pub(crate) fn lower_error_conv(conversion: &crate::AST::ErrorConvDef, cx: &Cx) -> TFunc {
    lower_error_conv_inner(conversion, cx)
}

fn lower_error_conv_inner(conversion: &crate::AST::ErrorConvDef, cx: &Cx) -> TFunc {
    let name = crate::Sema::error_conv_fn_name(&conversion.from_ty, &conversion.to_ty);
    let from_ty = Type::Named(conversion.from_ty.clone());
    let to_ty = Type::Named(conversion.to_ty.clone());
    let mut env = LowerEnv::new(name.clone());
    env.sentries_fenced = cx.dependency_fenced;
    env.ret_ty = Some(to_ty.clone());
    env.bind(
        Syntax::KW_SELF,
        TLocal::user(Syntax::KW_SELF),
        Some(from_ty.clone()),
    );
    prepare_interrupt_callback_locals(&conversion.body, cx, &mut env);
    let body = lower_stmts(&conversion.body, cx, &mut env);
    note_stack_sentry_in_tir(&body, &env);
    TFunc {
        name: name.clone(),
        module: cx.module_identity.clone(),
        key: function_semantic_key(&cx.module_identity, &name),
        source_file: cx.file.clone(),
        source_span: conversion.from_span,
        failure_carrier: TFailureCarrier::from_checked_type(&to_ty),
        effects: TEffectFacts::default(),
        target_applicability: default_target_applicability(),
        web_bucket: None,
        web_marker: None,
        visibility: TVisibility::Private,
        foreign: None,
        params: vec![(Syntax::KW_SELF.to_string(), from_ty, AccessConvention::Move)],
        web_param_reconstructions: Vec::new(),
        ret: Some(to_ty),
        gc_return: false,
        gc_scope: false,
        return_view_provenance: None,
        generic_params: Vec::new(),
        clone_types: Vec::new(),
        is_main: false,
        line: cov_line(cx, conversion.from_span.start),
        synthetic: false,
        is_unsafe: false,
        unsafe_gate: None,
        is_pure: true,
        memo_bound: None,
        is_reactive: false,
        reactive_upgrades: Vec::new(),
        is_inline: false,
        is_inline_always: false,
        is_scalar: false,
        kernel_proof: None,
        memo_field: None,
        uses_stack_sentry: env.stack_sentry_needed(),
        body,
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
    // D-STREAMYIELD1: a stream producer's declared `Stream<T>` is its
    // executable protocol, not an ordinary fallible value carrier. Keep that
    // raw type in TIR so Web can select the generator path and every engine
    // sees the same yield contract.
    let return_type = f
        .return_type
        .as_ref()
        .filter(|ty| {
            matches!(
                ty,
                Type::Apply { name, args }
                    if name == Syntax::TYPE_STREAM && args.len() == 1
            )
        })
        .cloned()
        .unwrap_or_else(|| f.effective_return_type());
    let type_param_names = type_param_names(f);
    let return_type = cx.canonicalize_checked_type(&return_type, &type_param_names);
    let mut env = LowerEnv::new(f.name.clone());
    env.sentries_enabled = sentries_enabled_for_function(f, cx);
    env.sentries_fenced = cx.dependency_fenced;
    env.gc_return = f.gc_return;
    env.ret_ty = Some(return_type.clone());
    // Mirror emit_func's parameter slot construction: a non-scalar `Read` param
    // (String, Char) is a borrow in Rust and reads as `(*name)`.
    let mut params = Vec::new();
    let mut resource_param_guards = Vec::new();
    let mut web_param_reconstructions = Vec::new();
    for p in &f.params {
        let rust_name = p.name.clone();
        let declared_param_ty = if p.variadic {
            Type::List(Box::new(p.ty.clone()))
        } else {
            p.ty.clone()
        };
        // Function parameters expose their source return spelling to users, but
        // callable values cross the executable boundary through the shared
        // Result carrier. Keep TIR on that ABI shape, as sema does in
        // `func_to_sig`.
        let effective_param_ty = cx
            .expand_type_aliases(&declared_param_ty)
            .with_effective_fn_returns();
        let param_ty = cx.canonicalize_checked_type(&effective_param_ty, &type_param_names);
        // c109 Phase 17: a param TYPED as a bare type parameter (`item: T`) is forced to
        // the `Move` convention for the slot deref (it is passed by value — `rust_param_type`
        // renders it `T`, no `&`), EXACTLY as `emit_func` forces `conv = Move` for an
        // `is_type_param` param. A param typed `Stack<T>` is NOT a type-var param — it keeps
        // its source convention (`Read` → `&__jet_Stack<T>`, deref'd place `(*__jet_s)`).
        if reconstruct_web_params {
            if let Type::Named(type_name) = &param_ty {
                if let Some(fields) = cx.struct_fields.get(type_name) {
                    if !fields.is_empty()
                        && fields.iter().all(|(_, ty)| {
                            matches!(ty, Type::Int | Type::IntN { .. } | Type::InlineRange { .. })
                        })
                    {
                        let flat_fields = fields
                            .iter()
                            .map(|(field, ty)| {
                                (
                                    field.to_string(),
                                    format!("{}_{}", p.name, field),
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
        let convention = effective_generic_convention(&slot_param, &f.type_params);
        slot_param.convention = convention;
        let place = param_place_generic(&p.name, &slot_param, &f.type_params);
        bind_resource_param(
            &p.name,
            &param_ty,
            convention,
            cx,
            &mut env,
            &mut resource_param_guards,
            place,
        );
        params.push((rust_name, param_ty, convention));
    }
    let mut body = resource_param_guards;
    prepare_interrupt_callback_locals(&f.body, cx, &mut env);
    let lowered_body = if return_type_has_value(&return_type) {
        lower_value_block(&f.body, cx, &mut env)
    } else {
        lower_stmts(&f.body, cx, &mut env)
    };
    body.extend(lowered_body);
    let mut clone_types = env.cloned_types.borrow().clone();
    for param in &f.params {
        collect_signature_clone_types(&param.ty, cx, &mut clone_types);
    }
    collect_signature_clone_types(&return_type, cx, &mut clone_types);
    let body = wrap_contract_scope(
        f,
        body,
        None,
        Some(return_type.clone()),
        &env.stack_sentry_needed,
        cx,
    );
    note_stack_sentry_in_tir(&body, &env);
    let uses_stack_sentry = env.stack_sentry_needed();
    TFunc {
        name: f.name.clone(),
        module: cx.module_identity.clone(),
        key: function_semantic_key(&cx.module_identity, &f.name),
        source_file: cx.file.clone(),
        source_span: f.span,
        failure_carrier: function_failure_carrier(f),
        effects: function_effect_facts(f),
        target_applicability: function_target_applicability(f),
        web_bucket: None,
        web_marker: f.web_marker.clone(),
        visibility: function_visibility(f),
        foreign: function_foreign_provenance(f),
        params,
        web_param_reconstructions,
        ret: Some(return_type),
        gc_return: f.gc_return,
        gc_scope: f.gc_scope,
        return_view_provenance: f.return_view_provenance.clone(),
        generic_params: generic_params_from(&f.type_params),
        clone_types,
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        synthetic: f.compiler_generated,
        is_unsafe: f.is_unsafe,
        unsafe_gate: unsafe_gate(f, cx, env.sentries_enabled),
        is_pure: f.is_pure,
        memo_bound: crate::AST::memo_bound_from_markers(&f.markers),
        is_reactive: f.is_reactive,
        reactive_upgrades: f.reactive_upgrades.clone(),
        is_inline: f.is_inline,
        is_inline_always: f.is_inline_always,
        is_scalar: f
            .markers
            .iter()
            .any(|marker| marker.name == crate::Syntax::MARKER_SCALAR),
        kernel_proof: f.kernel.as_ref().and_then(|marker| marker.proof),
        memo_field: None,
        uses_stack_sentry,
        body,
        kind: TFuncKind::TopLevel,
    }
}

fn lower_contract_cond(
    f: &Func,
    cond: &Expr,
    result_binding: Option<(&str, &Type)>,
    owner_type: Option<&str>,
    stack_sentry_needed: &Rc<Cell<bool>>,
    cx: &Cx,
) -> TExpr {
    let mut env = LowerEnv::new(f.name.clone());
    env.sentries_enabled = sentries_enabled_for_function(f, cx);
    env.sentries_fenced = cx.dependency_fenced;
    env.gc_return = f.gc_return;
    let type_param_names = type_param_names(f);
    for p in &f.params {
        let mut param_ty = if p.variadic {
            Type::List(Box::new(p.ty.clone()))
        } else {
            p.ty.clone()
        };
        if let Some(owner) = owner_type {
            param_ty = resolve_self_ty(&param_ty, owner);
        }
        let param_ty = cx.canonicalize_checked_type(&param_ty, &type_param_names);
        let mut slot_param = p.clone();
        slot_param.ty = param_ty.clone();
        let place = if owner_type.is_some() && p.name == Syntax::KW_SELF {
            if matches!(p.convention, AccessConvention::Write) {
                TLocal::generated(Syntax::KW_SELF).through_ref()
            } else if matches!(p.convention, AccessConvention::Read) {
                TLocal::generated(Syntax::KW_SELF)
                    .with_address_lifetime(crate::Codegen::TIR::TAddressLifetime::Borrowed)
            } else {
                TLocal::generated(Syntax::KW_SELF)
            }
        } else {
            param_place_generic(&p.name, &slot_param, &f.type_params)
        };
        env.bind(&p.name, place, Some(param_ty));
    }
    if let Some((rust_name, ty)) = result_binding {
        env.bind(
            "result",
            TLocal::generated(rust_name),
            Some(cx.canonicalize_checked_type(ty, &type_param_names)),
        );
    }
    env.stack_sentry_needed = stack_sentry_needed.clone();
    lower_expr(cond, cx, &mut env)
}

fn contract_interval(expr: &Expr, bindings: &HashMap<String, Type>, cx: &Cx) -> Option<(i64, i64)> {
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
pub(crate) fn contract_expr_proven(expr: &Expr, bindings: &HashMap<String, Type>, cx: &Cx) -> bool {
    match expr {
        Expr::Bool(value, _) => *value,
        Expr::Binary(BinOp::And, left, right, _) => {
            contract_expr_proven(left, bindings, cx) && contract_expr_proven(right, bindings, cx)
        }
        Expr::Binary(BinOp::Or, left, right, _) => {
            contract_expr_proven(left, bindings, cx) || contract_expr_proven(right, bindings, cx)
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
        Expr::CompareChain { operands, ops, .. } => {
            operands.windows(2).zip(ops).all(|(pair, op)| {
                match (
                    contract_interval(&pair[0], bindings, cx),
                    contract_interval(&pair[1], bindings, cx),
                ) {
                    (Some(left), Some(right)) => interval_comparison(*op, left, right),
                    _ => false,
                }
            })
        }
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
    let type_param_names = type_param_names(f);
    for param in &f.params {
        let mut ty = if param.variadic {
            Type::List(Box::new(param.ty.clone()))
        } else {
            param.ty.clone()
        };
        if let Some(owner) = owner_type {
            ty = resolve_self_ty(&ty, owner);
        }
        let ty = cx.canonicalize_checked_type(&ty, &type_param_names);
        bindings.insert(param.name.clone(), ty);
    }
    if let Some((name, ty)) = result_binding {
        let ty = cx.canonicalize_checked_type(ty, &type_param_names);
        bindings.insert(name.to_string(), ty.clone());
        bindings.insert("result".to_string(), ty);
    }
    bindings
}

fn lower_contract_clause(
    f: &Func,
    clause: &ContractClause,
    result_binding: Option<(&str, &Type)>,
    kind: TContractKind,
    owner_type: Option<&str>,
    stack_sentry_needed: &Rc<Cell<bool>>,
    cx: &Cx,
) -> TContract {
    let (_, line, _) = crate::Codegen::TIR::tir_src_line_at(&cx.src, clause.span.start);
    let bindings = contract_fact_bindings(f, result_binding, owner_type, cx);
    TContract {
        kind,
        condition: lower_contract_cond(
            f,
            &clause.cond,
            result_binding,
            owner_type,
            stack_sentry_needed,
            cx,
        ),
        message: lower_contract_cond(
            f,
            &clause.message_expr,
            result_binding,
            owner_type,
            stack_sentry_needed,
            cx,
        ),
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

fn contract_result_mode(carrier_ty: &Type, binding_ty: &Type) -> TContractResultMode {
    if carrier_ty == binding_ty {
        return TContractResultMode::Direct;
    }
    match carrier_ty {
        Type::Result { .. } => TContractResultMode::ResultPayload,
        Type::Option(_) => TContractResultMode::OptionPayload,
        Type::Tagged { inner, .. } => contract_result_mode(inner, binding_ty),
        other => {
            debug_assert_eq!(
                other, binding_ty,
                "contract result binding must match a non-carrier return type"
            );
            TContractResultMode::Direct
        }
    }
}

fn contract_result_for_scope(
    f: &Func,
    owner_type: Option<&str>,
    ret: Option<Type>,
    cx: &Cx,
) -> TContractResult {
    let type_param_names = type_param_names(f);
    let declared = f
        .return_type
        .clone()
        .unwrap_or_else(|| Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string()));
    let declared = owner_type
        .map(|owner| resolve_self_ty(&declared, owner))
        .unwrap_or(declared);
    let binding_ty = canonical_owner_type(
        cx,
        &cx.canonicalize_checked_type(&declared, &type_param_names),
    );
    let carrier_ty = ret
        .map(|ty| cx.canonicalize_checked_type(&ty, &type_param_names))
        .map(|ty| canonical_owner_type(cx, &ty))
        .unwrap_or_else(|| Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string()));
    let mode = contract_result_mode(&carrier_ty, &binding_ty);
    let binding_local = TLocal::generated("result");
    let carrier_local = match mode {
        TContractResultMode::Direct => binding_local.clone(),
        TContractResultMode::ResultPayload | TContractResultMode::OptionPayload => {
            TLocal::generated("result_carrier")
        }
    };
    TContractResult {
        carrier_ty,
        binding_ty,
        carrier_local,
        binding_local,
        mode,
    }
}

fn lower_post_contracts_for_owner(
    f: &Func,
    owner_type: Option<&str>,
    result: &TContractResult,
    stack_sentry_needed: &Rc<Cell<bool>>,
    cx: &Cx,
) -> Vec<TContract> {
    f.post
        .iter()
        .map(|clause| {
            lower_contract_clause(
                f,
                clause,
                Some((&result.binding_local.name, &result.binding_ty)),
                TContractKind::Post,
                owner_type,
                stack_sentry_needed,
                cx,
            )
        })
        .collect()
}

fn wrap_contract_scope(
    f: &Func,
    body: Vec<TStmt>,
    owner_type: Option<&str>,
    ret: Option<Type>,
    stack_sentry_needed: &Rc<Cell<bool>>,
    cx: &Cx,
) -> Vec<TStmt> {
    let result = contract_result_for_scope(f, owner_type, ret, cx);
    let post = lower_post_contracts_for_owner(f, owner_type, &result, stack_sentry_needed, cx);
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
            result,
        }]
    }
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
        | Type::Quantity { base: inner, .. }
        | Type::InlineRange { base: inner, .. } => {
            collect_signature_clone_types(inner, cx, out);
        }
        Type::Map { key, value, .. }
        | Type::Result {
            ok: key,
            err: value,
        } => {
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
/// Bare generic parameters keep the source access convention. A default `Read`
/// remains a borrowed `&T` slot; explicit `Move` (for example `^T`) is owned.
fn effective_generic_convention(
    p: &Param,
    _type_params: &[crate::AST::TypeParam],
) -> AccessConvention {
    p.convention
}

/// c109 Phase 17: `param_place` for a (possibly generic) free function.
/// Explicit access conventions stay aligned with the emitted Rust signature.
pub(crate) fn param_place_generic(
    name: &str,
    p: &Param,
    type_params: &[crate::AST::TypeParam],
) -> TLocal {
    let mut p = p.clone();
    p.convention = effective_generic_convention(&p, type_params);
    param_place(name, &p)
}

/// Project a method owner through the same canonical nominal identities used
/// by field and MIR lowering. Trait-generated methods can otherwise retain a
/// bare local owner and become ambiguous when an imported type shares its leaf.
fn canonical_owner_type(cx: &Cx, ty: &Type) -> Type {
    let entry_local = cx.jit_local_call_prefix.is_none();
    let mapped = ty.map_named_types(&|name| {
        if entry_local && cx.local_type_names.contains(name) {
            return None;
        }
        let canonical = crate::Codegen::TIR::canonical_enum_owner(cx, name);
        (canonical != name).then_some(canonical)
    });
    match mapped {
        Type::Apply { name, args } => Type::Apply {
            name: crate::Codegen::TIR::canonical_enum_owner(cx, &name),
            args,
        },
        other => other,
    }
}

fn canonical_owner_name(cx: &Cx, name: &str) -> String {
    crate::Codegen::TIR::canonical_enum_owner(cx, name)
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
    lower_method_for_owner_inner(f, type_name, owner_ty, cx, false)
}

/// `raw_protocol_return` is supplied by the enclosing generated-trait
/// provenance. Ordinary owner-specialized methods keep the fallible ABI.
pub(crate) fn lower_method_for_owner(
    f: &Func,
    type_name: &str,
    owner_ty: Type,
    cx: &Cx,
    raw_protocol_return: bool,
) -> TFunc {
    lower_method_for_owner_inner(f, type_name, owner_ty, cx, raw_protocol_return)
}

fn lower_method_for_owner_inner(
    f: &Func,
    type_name: &str,
    owner_ty: Type,
    cx: &Cx,
    raw_protocol_return: bool,
) -> TFunc {
    let owner_ty = canonical_owner_type(cx, &owner_ty);
    let declared_return_type = f
        .return_type
        .clone()
        .unwrap_or_else(|| Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string()));
    let return_type = if raw_protocol_return {
        debug_assert!(
            f.return_type.is_some(),
            "state-transition methods must declare their raw return type",
        );
        resolve_self_ty(&declared_return_type, type_name)
    } else {
        resolve_self_ty(&f.effective_return_type(), type_name)
    };
    let previous_type_params = cx.current_type_params.borrow().clone();
    let mut method_type_params = previous_type_params.clone();
    if let Some(owner_params) = cx.struct_type_param_order.get(type_name) {
        method_type_params.extend(owner_params.iter().cloned());
    }
    method_type_params.extend(f.type_params.iter().map(|param| param.name.clone()));
    cx.current_type_params.replace(method_type_params.clone());
    let return_type =
        canonical_owner_type(cx, &cx.canonicalize_checked_type(&return_type, &method_type_params));
    let mut env = LowerEnv::new(f.name.clone());
    env.sentries_fenced = cx.dependency_fenced;
    env.gc_return = f.gc_return;
    env.ret_ty = Some(return_type.clone());
    env.raw_protocol_return = raw_protocol_return;
    env.self_owner = Some(canonical_owner_name(cx, type_name));
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
            } else if matches!(p.convention, AccessConvention::Read) {
                TLocal::generated("self")
                    .with_address_lifetime(crate::Codegen::TIR::TAddressLifetime::Borrowed)
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
        let rust_name = p.name.clone();
        // Callable parameters use the effective carrier in TIR. The source
        // declaration remains available to diagnostics and callback bindings.
        let mut pty = resolve_self_ty(&p.ty.with_effective_fn_returns(), type_name);
        if p.variadic {
            pty = Type::List(Box::new(pty));
        }
        pty = canonical_owner_type(cx, &cx.canonicalize_checked_type(&pty, &method_type_params));
        let mut slot_param = p.clone();
        slot_param.ty = pty.clone();
        let convention = effective_generic_convention(&slot_param, &f.type_params);
        slot_param.convention = convention;
        let place = param_place_generic(&p.name, &slot_param, &f.type_params);
        bind_resource_param(
            &p.name,
            &pty,
            convention,
            cx,
            &mut env,
            &mut resource_param_guards,
            place,
        );
        params.push((rust_name, pty, convention));
    }
    let mut body = resource_param_guards;
    prepare_interrupt_callback_locals(&f.body, cx, &mut env);
    let lowered_body = if return_type_has_value(&return_type) {
        lower_value_block(&f.body, cx, &mut env)
    } else {
        lower_stmts(&f.body, cx, &mut env)
    };
    body.extend(lowered_body);
    let body = wrap_contract_scope(
        f,
        body,
        Some(type_name),
        Some(return_type.clone()),
        &env.stack_sentry_needed,
        cx,
    );
    let mut clone_types = env.cloned_types.borrow().clone();
    collect_signature_clone_types(&owner_ty, cx, &mut clone_types);
    for param in &f.params {
        collect_signature_clone_types(&param.ty, cx, &mut clone_types);
    }
    collect_signature_clone_types(&return_type, cx, &mut clone_types);
    cx.current_type_params.replace(previous_type_params);
    // An instance method carries `Some(conv)`; a static method carries `None`.
    let semantic_key = method_semantic_key(&cx.module_identity, &owner_ty, None, None, &f.name);
    let kind = TFuncKind::Method {
        self_conv: if is_static { None } else { self_conv },
        owner_type: owner_ty,
    };
    let memo_field = cx
        .memo_fields
        .get(type_name)
        .and_then(|fields| fields.contains_key(&f.name).then(|| f.name.clone()));
    note_stack_sentry_in_tir(&body, &env);
    let uses_stack_sentry = env.stack_sentry_needed();
    TFunc {
        name: f.name.clone(),
        module: cx.module_identity.clone(),
        key: semantic_key,
        source_file: cx.file.clone(),
        source_span: f.span,
        failure_carrier: function_failure_carrier(f),
        effects: function_effect_facts(f),
        target_applicability: function_target_applicability(f),
        web_bucket: None,
        web_marker: f.web_marker.clone(),
        visibility: function_visibility(f),
        foreign: function_foreign_provenance(f),
        params,
        web_param_reconstructions: Vec::new(),
        ret: Some(return_type),
        gc_return: f.gc_return,
        gc_scope: f.gc_scope,
        return_view_provenance: f.return_view_provenance.clone(),
        generic_params: generic_params_from(&f.type_params),
        clone_types,
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        synthetic: f.compiler_generated,
        is_unsafe: f.is_unsafe,
        unsafe_gate: unsafe_gate(f, cx, env.sentries_enabled),
        is_pure: f.is_pure,
        memo_bound: None,
        is_reactive: f.is_reactive,
        reactive_upgrades: f.reactive_upgrades.clone(),
        is_inline: f.is_inline,
        is_inline_always: f.is_inline_always,
        is_scalar: f
            .markers
            .iter()
            .any(|marker| marker.name == crate::Syntax::MARKER_SCALAR),
        kernel_proof: f.kernel.as_ref().and_then(|marker| marker.proof),
        memo_field,
        uses_stack_sentry,
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
pub(crate) fn lower_trait_method(
    f: &Func,
    type_name: &str,
    cx: &Cx,
    trait_name: &str,
    raw_protocol_return: bool,
    operator_rhs: Option<&Type>,
) -> TFunc {
    lower_trait_method_inner(
        f,
        type_name,
        cx,
        trait_name,
        raw_protocol_return,
        operator_rhs,
    )
}

fn lower_trait_method_inner(
    f: &Func,
    type_name: &str,
    cx: &Cx,
    trait_name: &str,
    raw_protocol_return: bool,
    operator_rhs: Option<&Type>,
) -> TFunc {
    // D-FAILURE-FOUNDATION1: protocol bodies keep their declared ABI. Encode,
    // Decode, Display, Debug, Equatable, Comparable, Close, and same-type
    // arithmetic are Rust trait methods (`String`, `bool`, `Ordering`, the
    // owner type, and unit — never `Result`), so projecting an omitted contract
    // onto a hand-written body would emit `Result<raw, Err>` and violate the
    // trait ABI just as it would for a compiler-generated body.
    // Other user trait methods still use the implicit Error carrier.
    let raw_protocol_return = raw_protocol_return
        || matches!(
            trait_name,
            crate::Generics::ENCODE
                | crate::Generics::DECODE
                | crate::Generics::DISPLAY
                | crate::Generics::DEBUG
                | crate::Generics::EQUATABLE
                | crate::Generics::COMPARABLE
                | crate::Generics::CLOSE
                | crate::Generics::ADD
                | crate::Generics::SUB
                | crate::Generics::MUL
                | crate::Generics::DIV
        );
    let declared_return_type = f
        .return_type
        .clone()
        .unwrap_or_else(|| Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string()));
    let return_type = if raw_protocol_return {
        debug_assert!(
            f.return_type.is_some() || trait_name == crate::Generics::CLOSE,
            "protocol methods must declare their raw return type"
        );
        resolve_self_ty(&declared_return_type, type_name)
    } else {
        resolve_self_ty(&f.effective_return_type(), type_name)
    };
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
    let owner_ty = canonical_owner_type(cx, &owner_ty);
    let previous_type_params = cx.current_type_params.borrow().clone();
    let mut method_type_params = previous_type_params.clone();
    if let Some(owner_params) = cx.struct_type_param_order.get(type_name) {
        method_type_params.extend(owner_params.iter().cloned());
    }
    method_type_params.extend(f.type_params.iter().map(|param| param.name.clone()));
    cx.current_type_params.replace(method_type_params.clone());
    let return_type =
        canonical_owner_type(cx, &cx.canonicalize_checked_type(&return_type, &method_type_params));
    let mut env = LowerEnv::new(f.name.clone());
    env.sentries_enabled = sentries_enabled_for_function(f, cx);
    env.sentries_fenced = cx.dependency_fenced;
    env.gc_return = f.gc_return;
    env.ret_ty = Some(return_type.clone());
    env.raw_protocol_return = raw_protocol_return;
    env.self_owner = Some(canonical_owner_name(cx, type_name));
    let mut params = Vec::new();
    let mut resource_param_guards = Vec::new();
    let mut self_conv = None;
    for p in &f.params {
        if p.name == Syntax::KW_SELF {
            self_conv = Some(p.convention);
            // The self slot, EXACTLY `emit_trait_method`'s: type `Some(Named(type_name))`
            // (NOT `None` like `emit_method`). D-MUTSELF1: a `mut self` receiver is
            // `&mut self`, so its place DEREFS (`(*self)`); `self`/`take self` do not.
            let place = if matches!(p.convention, AccessConvention::Write) {
                TLocal::generated("self").through_ref()
            } else if matches!(p.convention, AccessConvention::Read) {
                TLocal::generated("self")
                    .with_address_lifetime(crate::Codegen::TIR::TAddressLifetime::Borrowed)
            } else {
                TLocal::generated("self")
            };
            env.bind(Syntax::KW_SELF, place, Some(owner_ty.clone()));
            if matches!(p.convention, AccessConvention::Read) {
                env.mark_borrowed(Syntax::KW_SELF);
            }
            continue;
        }
        let rust_name = p.name.clone();
        // D-SERDE2: a `Decode.decode(tree: Data)` param is emitted as `&jet_std::DataTree`
        // and re-bound to an owned clone at the function head, so the body sees an owned
        // `Data` local — its place is the bare name, NOT `param_place`'s non-scalar deref.
        let place = if serde == Some(SerdeCodec::Decode) {
            TLocal::user(&p.name)
        } else {
            param_place(&p.name, p)
        };
        let pty = canonical_owner_type(
            cx,
            &cx.canonicalize_checked_type(
                &resolve_self_ty(&p.ty, type_name),
                &method_type_params,
            ),
        );
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
    let lowered_body = if return_type_has_value(&return_type) {
        lower_value_block(&f.body, cx, &mut env)
    } else {
        lower_stmts(&f.body, cx, &mut env)
    };
    body.extend(lowered_body);
    let body = wrap_contract_scope(
        f,
        body,
        Some(type_name),
        Some(return_type.clone()),
        &env.stack_sentry_needed,
        cx,
    );
    let mut body = body;
    if serde == Some(SerdeCodec::Encode) && cx.published_schemas.contains(type_name) {
        for stmt in &mut body {
            if let TStmt::Return(Some(value)) = stmt {
                let known = std::mem::replace(
                    value,
                    TExpr {
                        ty: Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string()),
                        kind: TExprKind::Unit,
                    },
                );
                let holder = TExpr {
                    ty: Type::Named(Syntax::TYPE_DATA.to_string()),
                    kind: TExprKind::Field {
                        recv: Box::new(TExpr {
                            ty: owner_ty.clone(),
                            kind: TExprKind::Local(TLocal::generated(Syntax::KW_SELF)),
                        }),
                        field: Syntax::PUBLISHED_UNKNOWN_FIELDS.to_string(),
                        boxed: false,
                    },
                };
                let known_ty = known.ty.clone();
                *value = TExpr {
                    ty: known_ty.clone(),
                    kind: match crate::Syntax::core_call_projection(
                        "core.encoding",
                        "__published_schema_merge",
                        crate::Syntax::CoreCallCoverage::TIR_SUBSET,
                        2,
                    ) {
                        Ok(record) => TExprKind::CoreCall {
                            record,
                            args: vec![known, holder],
                            source_span: f.span,
                            type_args: Vec::new(),
                            widen_to_vec: vec![false, false],
                            data_plan: None,
                            fallibility: TFailureCarrier::from_checked_type(&known_ty),
                        },
                        Err(_) => known.kind,
                    },
                };
            }
        }
    }
    let mut clone_types = env.cloned_types.borrow().clone();
    collect_signature_clone_types(&owner_ty, cx, &mut clone_types);
    for param in &f.params {
        collect_signature_clone_types(&param.ty, cx, &mut clone_types);
    }
    collect_signature_clone_types(&return_type, cx, &mut clone_types);
    cx.current_type_params.replace(previous_type_params);
    note_stack_sentry_in_tir(&body, &env);
    let uses_stack_sentry = env.stack_sentry_needed();
    TFunc {
        name: f.name.clone(),
        module: cx.module_identity.clone(),
        key: method_semantic_key(
            &cx.module_identity,
            &owner_ty,
            Some(trait_name),
            operator_rhs,
            &f.name,
        ),
        source_file: cx.file.clone(),
        source_span: f.span,
        failure_carrier: function_failure_carrier(f),
        effects: function_effect_facts(f),
        target_applicability: function_target_applicability(f),
        web_bucket: None,
        web_marker: f.web_marker.clone(),
        visibility: function_visibility(f),
        foreign: function_foreign_provenance(f),
        params,
        web_param_reconstructions: Vec::new(),
        ret: Some(return_type),
        gc_return: f.gc_return,
        gc_scope: f.gc_scope,
        return_view_provenance: f.return_view_provenance.clone(),
        generic_params: generic_params_from(&f.type_params),
        clone_types,
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        synthetic: f.compiler_generated,
        // The trait-method `unsafe` prefix rides on `TFuncKind::TraitMethod.is_unsafe`
        // (the dedicated trait-method emit reads it there); the top-level flag is unused
        // for this kind, but keep it consistent.
        is_unsafe: f.is_unsafe,
        unsafe_gate: unsafe_gate(f, cx, env.sentries_enabled),
        is_pure: f.is_pure,
        memo_bound: None,
        is_reactive: f.is_reactive,
        reactive_upgrades: f.reactive_upgrades.clone(),
        is_inline: f.is_inline,
        is_inline_always: f.is_inline_always,
        is_scalar: f
            .markers
            .iter()
            .any(|marker| marker.name == crate::Syntax::MARKER_SCALAR),
        kernel_proof: f.kernel.as_ref().and_then(|marker| marker.proof),
        memo_field: None,
        uses_stack_sentry,
        body,
        kind: TFuncKind::TraitMethod {
            is_unsafe: f.is_unsafe,
            self_conv,
            owner_type: owner_ty,
            trait_name: trait_name.to_string(),
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
    lower_delegation_method_inner(f, field, cx)
}

fn lower_delegation_method_inner(f: &Func, _field: &str, cx: &Cx) -> TFunc {
    let type_param_names = type_param_names(f);
    let return_type = cx.canonicalize_checked_type(
        &f.effective_return_type(),
        &type_param_names,
    );
    let owner_ty = f
        .params
        .iter()
        .find(|param| param.name == Syntax::KW_SELF)
        .map(|param| cx.canonicalize_checked_type(&param.ty, &type_param_names))
        .unwrap_or_else(|| Type::Named(Syntax::KW_SELF.to_string()));
    let self_conv = f
        .params
        .iter()
        .find(|param| param.name == Syntax::KW_SELF)
        .map(|param| param.convention);
    let params = f
        .params
        .iter()
        .filter(|param| param.name != Syntax::KW_SELF)
        .map(|param| {
            (
                param.name.clone(),
                cx.canonicalize_checked_type(&param.ty, &type_param_names),
                param.convention,
            )
        })
        .collect();
    TFunc {
        name: f.name.clone(),
        module: cx.module_identity.clone(),
        key: method_semantic_key(&cx.module_identity, &owner_ty, None, None, &f.name),
        source_file: cx.file.clone(),
        source_span: f.span,
        failure_carrier: function_failure_carrier(f),
        effects: function_effect_facts(f),
        target_applicability: function_target_applicability(f),
        web_bucket: None,
        web_marker: f.web_marker.clone(),
        visibility: function_visibility(f),
        foreign: function_foreign_provenance(f),
        params,
        web_param_reconstructions: Vec::new(),
        ret: Some(return_type),
        gc_return: f.gc_return,
        gc_scope: f.gc_scope,
        return_view_provenance: f.return_view_provenance.clone(),
        generic_params: generic_params_from(&f.type_params),
        clone_types: Vec::new(),
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        synthetic: f.compiler_generated,
        is_unsafe: false,
        unsafe_gate: None,
        is_pure: false,
        memo_bound: None,
        is_reactive: false,
        reactive_upgrades: Vec::new(),
        is_inline: false,
        is_inline_always: false,
        is_scalar: false,
        kernel_proof: None,
        memo_field: None,
        uses_stack_sentry: false,
        body: Vec::new(),
        kind: TFuncKind::TraitMethod {
            is_unsafe: false,
            self_conv,
            owner_type: owner_ty,
            trait_name: String::new(),
            serde: None,
        },
    }
}

/// The Rust place a parameter reads as, mirroring `emit_func`'s `deref` logic:
/// a `Read` parameter of non-scalar type (String/Char) is a `&T` and must be
/// dereferenced; `Mutate` is `&mut T` (deref'd); `Move`/scalar-`Read` is by value.
pub(crate) fn param_place(name: &str, p: &Param) -> TLocal {
    let deref = match p.convention {
        AccessConvention::Read if p.ty.is_scalar() => false,
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
