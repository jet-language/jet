//! Statement/control-flow lowering from checked TIR to canonical MIR.
//!
//! This module deliberately knows only the checked TIR shapes and the shared
//! MIR vocabulary.  It never consults AST nodes or chooses a backend spelling.

use super::tir_to_mir_expr::lower_expr;
use super::mir::{LowerCtx, LowerError};
use crate::AST::{BinOp, Type};
use crate::Codegen::TIR::{
    ScopeMemberKind, TCoreClosureKind, TExpr, TExprKind, TForInMethod, TIfCond, TIndexFieldAssign, TLocal, TMatchArm,
    TCallArg, TMethodRef, TNumericOp, TPattern, TPlace, TStaticOwner, TStmt, TLetTy, TBuiltinOp,
};
use jet_foundation::MIR::{
    MirAccess, MirBinaryDispatch, MirBlockId, MirCallArg, MirCallee, MirConstant, MirIndexKind,
    MirLoopSourceKind, MirOperation, MirScopeId, MirScopeKind, MirTerminator,
    MirTestScopeMember,
};
use jet_foundation::CanonicalPass;

/// Lower a sequence of checked TIR statements in the current MIR block.
///
/// A terminator ends the current path.  The caller owns creation and selection
/// of any continuation block, so statements after a terminator are unreachable
pub(super) fn lower_stmts(ctx: &mut LowerCtx, stmts: &[TStmt]) -> Result<(), LowerError> {
    let before_payload = CanonicalPass::enabled().then(|| {
        super::canonical_statements_payload(stmts)
    });
    let before_identity = CanonicalPass::enabled().then(|| {
        super::canonical_statements_identity(stmts)
    });
    for stmt in stmts {
        if ctx.is_terminated() {
            break;
        }
        lower_stmt(ctx, stmt)?;
    }
    if let (Some(before_payload), Some(before_identity)) = (before_payload, before_identity) {
        CanonicalPass::record(
            "lowering",
            "mir.lower-statements",
            "crates/jet-codegen/src/Codegen/TIR/tir_to_mir_stmt.rs",
            "tir",
            before_payload,
            before_identity,
            "mir",
            CanonicalPass::debug_payload("mir", "mir.lower-statements", &ctx.blocks),
            CanonicalPass::debug_identity("mir", "mir.lower-statements", &ctx.blocks),
            "preserve",
        );
    }
    Ok(())
}

/// Lower one checked TIR statement into canonical MIR.
///
/// Every `TStmt` variant is handled here.  Backend-specific presentation facts
/// remain facts on the TIR node; only executable values, places, scopes, and
/// control-flow edges are emitted.
pub(super) fn lower_stmt(ctx: &mut LowerCtx, stmt: &TStmt) -> Result<(), LowerError> {
    match stmt {
        TStmt::InvariantViolation { construct, span } => Err(ctx.error(*span, construct.clone())),
        TStmt::Contract { contract } => ctx.lower_contract(contract),
        TStmt::ContractScope {
            pre,
            body,
            post,
            result,
        } => ctx.lower_contract_scope(pre, body, post, result),

        TStmt::Let {
            name,
            kw,
            let_ty,
            init,
            gc_promotion: _,
            gc_transferred: _,
        } => {
            let is_uninit = matches!(&init.kind, TExprKind::Uninit);
            let value = if is_uninit {
                None
            } else {
                Some(lower_expr(ctx, init)?)
            };
            let ty = let_binding_type(let_ty, &init.ty);
            let local = local_for_binding(name, kw);
            let place = if matches!(let_ty, TLetTy::SendFn(_)) {
                ctx.bind_local_send_fn(
                    &local,
                    ty,
                    local.mutable,
                    false,
                    is_uninit,
                )?
            } else {
                ctx.bind_local(
                    &local,
                    ty,
                    local.mutable,
                    false,
                    is_uninit,
                )?
            };
            if is_uninit {
                ctx.emit(
                    "stmt.let.initialize-uninit",
                    None,
                    MirOperation::InitializeUninit { place },
                )?;
            } else if let Some(value) = value {
                ctx.emit(
                    "stmt.let.write",
                    None,
                    MirOperation::WritePlace { place, value },
                )?;
            }
            Ok(())
        }

        TStmt::RefutableBind {
            pattern,
            init,
            fallback,
        } => lower_refutable_bind(ctx, pattern, init, fallback),

        TStmt::GcEdit {
            root,
            slot,
            edges: _,
            replace_all: _,
            index_temp,
            stmt,
        } => {
            // GC edit is a checked authority region.  The collector metadata is
            // retained by the scope owner; the executable body remains ordinary
            // place lowering, never a second GC IR.
            let scope = ctx.enter_scope(
                MirScopeKind::Authority,
                ctx.span(),
                Some(format!("{root}.{slot}")),
            )?;
            if let Some((name, init)) = index_temp {
                lower_let_like(ctx, name, init, true)?;
            }
            lower_stmt(ctx, stmt)?;
            ctx.exit_scope(scope)?;
            Ok(())
        }

        TStmt::SplitViews {
            owner,
            root,
            len: _,
            source: _,
            source_start: _,
            before: _,
            split_tail: _,
            segment: _,
            after: _,
            name,
            start,
            end,
            single,
            write,
            elem_ty,
            line: _,
        } => lower_split_view(ctx, owner.as_ref(), root, name, *start, *end, *single, *write, elem_ty.as_ref()),

        TStmt::TupleDestructure {
            tmp: _,
            init,
            kw,
            move_fields,
            binds,
        } => lower_tuple_destructure(ctx, init, kw, *move_fields, binds),

        TStmt::StructDestructure {
            tmp: _,
            init,
            kw,
            move_fields,
            binds,
        } => lower_struct_destructure(ctx, init, kw, *move_fields, binds),

        TStmt::ListDestructure {
            tmp: _,
            init,
            kw,
            want,
            file: _,
            line: _,
            elems,
        } => lower_list_destructure(ctx, init, kw, *want, elems),

        TStmt::Assign {
            place,
            op,
            value,
            clone_value,
            line: _,
        } => lower_assign(ctx, place, *op, value, *clone_value),

        TStmt::Return(value) => ctx.lower_return(value.as_ref()),

        TStmt::ExprStmt(expr) => {
            let value = lower_expr(ctx, expr)?;
            // Expression lowering already emits the effectful operation.  Keep a
            // value-producing expression alive in MIR; an unused result is legal
            // and avoids inventing a discard operation with target policy.
            let _ = value;
            Ok(())
        }

        TStmt::TaskGroup {
            group,
            limit,
            body,
        } => {
            let scope = ctx.enter_scope(
                MirScopeKind::TaskGroup,
                ctx.span(),
                Some(group.name.clone()),
            )?;
            let call_arg = |value, borrow| TCallArg {
                value,
                template_items: None,
                borrow,
                mut_borrow: false,
                clone: false,
                arc_clone: false,
                fn_coerce: None,
                widen_to_vec: false,
                widen_to_union: None,
                box_as_trait: None,
            };
            let group_ty = Type::Named(crate::Syntax::TYPE_TASKGROUP.to_string());
            let constructor_args = if let Some(limit) = limit {
                let Some(TNumericOp::TryFrom { host_kind, dst_rust, dst_spelling }) =
                    super::resolve_numeric_conversion_op("I64", "Int")
                else {
                    return Err(ctx.error(ctx.span(), "missing task group limit conversion"));
                };
                let line = ctx.source_texts
                    .get(&ctx.function.source_file)
                    .map(|source| crate::Diagnostics::span_line_col(source, ctx.span().start).0)
                    .unwrap_or(ctx.function.line) as u32;
                vec![call_arg(TExpr {
                    ty: Type::IntN { signed: true, bits: 64 },
                    kind: TExprKind::NumericMethod {
                        recv: Box::new(limit.clone()),
                        op: TNumericOp::CheckedIntToFixed {
                            host_kind,
                            dst_rust,
                            dst_spelling,
                            line,
                        },
                    },
                }, false)]
            } else {
                Vec::new()
            };
            let owner = TStaticOwner::Prelude {
                rooted: true,
                path: "jet_std::JetTaskGroup".to_string(),
                generics: Vec::new(),
            };
            let constructor = TExpr {
                ty: group_ty.clone(),
                kind: TExprKind::StaticCall {
                    owner: owner.clone(),
                    owner_type: None,
                    method: TMethodRef::bare(if limit.is_some() { "with_limit" } else { "new" }),
                    type_args: Vec::new(),
                    args: constructor_args,
                },
            };
            let group_place = ctx.bind_local(
                group,
                group_ty.clone(),
                group.mutable,
                false,
                false,
            )?;
            let group_value = lower_expr(ctx, &constructor)?;
            ctx.emit(
                "stmt.task-group.write",
                None,
                MirOperation::WritePlace {
                    place: group_place,
                    value: group_value,
                },
            )?;
            let close = TExpr {
                ty: Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string()),
                kind: TExprKind::StaticCall {
                    owner,
                    owner_type: None,
                    method: TMethodRef::bare("close"),
                    type_args: Vec::new(),
                    args: vec![call_arg(TExpr {
                        ty: group_ty,
                        kind: TExprKind::Local(group.clone()),
                    }, true)],
                },
            };
            let thunk = ctx.lower_defer_thunk(
                &close,
                &group.name,
                format!("task-group-{}", scope.0),
            )?;
            ctx.register_defer(thunk)?;
            lower_stmts(ctx, body)?;
            ctx.exit_scope(scope)?;
            Ok(())
        }

        TStmt::DeferClose { close, resource, id } => {
            let thunk = ctx.lower_defer_thunk(close, resource, *id)?;
            ctx.register_defer(thunk)?;
            Ok(())
        }

        TStmt::If {
            cond,
            then_body,
            else_body,
            else_is_elseif: _,
        } => lower_if(ctx, cond, then_body, else_body.as_deref()),

        TStmt::Loop { label, body } => lower_loop(ctx, label.as_deref(), body),
        TStmt::While { label, cond, body } => lower_while(ctx, label.as_deref(), cond, body),
        TStmt::CountedLoop {
            label,
            init,
            cond,
            step,
            body,
            ..
        } => lower_counted_loop(ctx, label.as_deref(), init, cond, step.as_deref(), body),
        TStmt::Range {
            label,
            var,
            source,
            start,
            end,
            step,
            exclusive,
            auto_vectorization: _,
            body,
        } => lower_range_loop(
            ctx,
            label.as_deref(),
            var,
            source.as_ref(),
            start,
            end,
            step.as_ref(),
            *exclusive,
            body,
        ),
        TStmt::Break(label) => {
            let target = ctx.resolve_break(label.as_deref())?;
            let depth = ctx.break_cleanup_depth(label.as_deref())?;
            ctx.terminate_with_cleanup(
                MirTerminator::Break {
                    target,
                    value: None,
                },
                depth,
            )?;
            Ok(())
        }
        TStmt::BreakValue { label, value } => {
            let value = lower_expr(ctx, value)?;
            let target = ctx.resolve_break(label.as_deref())?;
            let depth = ctx.break_cleanup_depth(label.as_deref())?;
            ctx.terminate_with_cleanup(
                MirTerminator::Break {
                    target,
                    value: Some(value),
                },
                depth,
            )?;
            Ok(())
        }
        TStmt::Continue(label) => {
            let target = ctx.resolve_continue(label.as_deref())?;
            let depth = ctx.continue_cleanup_depth(label.as_deref())?;
            ctx.terminate_with_cleanup(MirTerminator::Continue { target }, depth)?;
            Ok(())
        }

        TStmt::EnumMatch {
            scrutinee,
            clone_subject,
            arms,
            else_body,
            fallthrough,
        } => lower_enum_match(ctx, scrutinee, *clone_subject, arms, else_body.as_deref(), *fallthrough),

        TStmt::RangeSwitch {
            subject,
            arms,
            else_body,
        } => lower_range_switch(ctx, subject, arms, else_body),

        TStmt::IndexAssign {
            uninit,
            base,
            index,
            is_map,
            value,
        } => {
            // Match regular assignment: evaluate the RHS before the indexed
            // place, whose receiver and index expressions may have effects.
            let value = lower_expr(ctx, value)?;
            let place = ctx.lower_index_place(
                base,
                index,
                if *uninit {
                    MirIndexKind::FixedListProof
                } else if *is_map {
                    MirIndexKind::Map
                } else {
                    MirIndexKind::List
                },
                MirAccess::Write,
            )?;
            ctx.emit(
                "stmt.index.write",
                None,
                MirOperation::WritePlace { place, value },
            )?;
            Ok(())
        }

        TStmt::IndexFieldAssign(assign) => lower_index_field_assign(ctx, assign),

        TStmt::IndexHookAssign {
            type_name,
            base,
            index,
            value,
        } => {
            let value = lower_expr(ctx, value)?;
            let base_place = match &base.kind {
                TExprKind::Local(local) => {
                    ctx.lower_place(&TPlace::Local(local.clone()), MirAccess::Write)?
                }
                _ => ctx.lower_place(
                    &TPlace::Expr(Box::new(base.clone())),
                    MirAccess::Write,
                )?,
            };
            let base = ctx.emit(
                "index-hook-place",
                Some(base.ty.clone()),
                MirOperation::ReadPlace(base_place),
            )?;
            let index = lower_expr(ctx, index)?;
            let span = ctx.span();
            let callee = ctx.function_id_for(&format!("{type_name}::IndexMut::set"))?;
            let args = vec![
                MirCallArg {
                    value: base,
                    place: Some(base_place),
                    access: MirAccess::Write,
                    span,
                    label: None,
                    source_index: Some(0),
                    binder_slot: None,
                    spread: false,
                    implicit_clone: false,
                    shared_auto_clone: false,
                    owned_last_use: true,
                    authority_boundary: false,
                    fn_coercion: None,
                    widen_fixed_to_list: false,
                    widen_to_union: None,
                    box_as_trait: None,
                },
                MirCallArg {
                    value: index,
                    place: None,
                    access: MirAccess::Read,
                    span,
                    label: None,
                    source_index: Some(1),
                    binder_slot: None,
                    spread: false,
                    implicit_clone: false,
                    shared_auto_clone: false,
                    owned_last_use: false,
                    authority_boundary: false,
                    fn_coercion: None,
                    widen_fixed_to_list: false,
                    widen_to_union: None,
                    box_as_trait: None,
                },
                MirCallArg {
                    value,
                    place: None,
                    access: MirAccess::Read,
                    span,
                    label: None,
                    source_index: Some(2),
                    binder_slot: None,
                    spread: false,
                    implicit_clone: false,
                    shared_auto_clone: false,
                    owned_last_use: false,
                    authority_boundary: false,
                    fn_coercion: None,
                    widen_fixed_to_list: false,
                    widen_to_union: None,
                    box_as_trait: None,
                },
            ];
            ctx.emit(
                "stmt.index-hook.set",
                None,
                MirOperation::Call {
                    callee: MirCallee::User(callee),
                    args,
                    type_args: Vec::new(),
                },
            )?;
            Ok(())
        }

        TStmt::MathSwizzleAssign {
            base,
            type_name: _,
            lanes,
            value,
            clone_value,
        } => lower_swizzle_assign(ctx, base, lanes, value, *clone_value),

        TStmt::ForIn {
            label,
            var,
            var2,
            source,
            collection,
            step,
            method_kind,
            columnar: _,
            by_value,
            body,
        } => lower_for_in(
            ctx,
            label.as_deref(),
            var,
            var2.as_deref(),
            source,
            collection,
            step.as_ref(),
            method_kind.as_ref(),
            *by_value,
            body,
        ),

        TStmt::Inline(body) => lower_stmts(ctx, body),
        TStmt::DebugOnly(body) => lower_debug_only(ctx, body),

        TStmt::MixedSwitch {
            subject,
            class: _,
            arms,
            else_body,
        } => lower_mixed_switch(ctx, subject, arms, else_body.as_deref()),

        TStmt::Unsafe { gate, body } => {
            lower_scoped(ctx, MirScopeKind::Unsafe, Some(gate.reason.clone()), body)
        }
        TStmt::SentryPolicy { enabled, body } => lower_scoped(
            ctx,
            MirScopeKind::Policy,
            Some(if *enabled { "enabled" } else { "disabled" }.to_string()),
            body,
        ),
        TStmt::Impure(body) => lower_scoped(ctx, MirScopeKind::Impure, None, body),

        TStmt::Reactive { executable } => {
            let site = ctx.site_id_for(ctx.span(), "reactive").0 as usize;
            let expr = TExpr {
                ty: Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string()),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::ReactiveEffect {
                        executable: executable.clone(),
                        site,
                    },
                },
            };
            let _ = lower_expr(ctx, &expr)?;
            Ok(())
        }

        TStmt::Region(body) => lower_scoped(ctx, MirScopeKind::Region, None, body),

        TStmt::Layout { handle, label, body } => {
            let scope = ctx.enter_scope(MirScopeKind::Layout, ctx.span(), Some(label.clone()))?;
            let handle_ty = Type::Named("LayoutHandle".to_string());
            let place = ctx.bind_local(
                handle,
                handle_ty.clone(),
                handle.mutable,
                false,
                false,
            )?;
            let value = ctx.emit(
                "stmt.layout.global",
                Some(handle_ty),
                MirOperation::Global {
                    name: format!("layout::{label}"),
                },
            )?;
            ctx.emit(
                "stmt.layout.write",
                None,
                MirOperation::WritePlace { place, value },
            )?;
            lower_stmts(ctx, body)?;
            ctx.exit_scope(scope)?;
            Ok(())
        }

        TStmt::ContextBlock { guards, body } => {
            let scope = ctx.enter_scope(MirScopeKind::Context, ctx.span(), None)?;
            for (_, value) in guards {
                let _ = lower_expr(ctx, value)?;
            }
            lower_stmts(ctx, body)?;
            ctx.exit_scope(scope)?;
            Ok(())
        }

        TStmt::Live { body } => lower_scoped(ctx, MirScopeKind::Live, None, body),
        TStmt::Shield { body } => lower_scoped(ctx, MirScopeKind::Shield, None, body),

        TStmt::ScopeMember { kind, body } => lower_scope_member(ctx, kind, body),

        TStmt::Transact {
            handle,
            snapshots,
            stm,
            body,
        } => lower_transaction(ctx, handle.as_ref(), snapshots, stm.as_ref(), body),

        TStmt::LineMarker(line) => {
            let line = u32::try_from(*line)
                .map_err(|_| LowerError::new(ctx.span(), "checked source line exceeds MIR line range"))?;
            ctx.set_line_marker(line);
            Ok(())
        }
        TStmt::SourceSpan(span) => {
            ctx.set_span(*span);
            Ok(())
        }
        TStmt::Erased {
            construct,
            span,
            reason,
        } => {
            ctx.record_erasure(construct, *span, *reason);
            Ok(())
        }
    }
}

fn let_binding_type(let_ty: &TLetTy, init_ty: &Type) -> Type {
    match let_ty {
        TLetTy::Inferred | TLetTy::StrView => init_ty.clone(),
        TLetTy::Annotated { ty, .. } => ty.clone(),
        TLetTy::Tuple(types) => Type::Tuple(
            types
                .iter()
                .enumerate()
                .map(|(i, ty)| (i.to_string(), Box::new(ty.clone())))
                .collect(),
        ),
        TLetTy::SendFn(ty) => ty.clone(),
    }
}

fn local_for_binding(name: &str, kw: &str) -> TLocal {
    if kw.contains("mut") {
        TLocal::user(name).as_mutable()
    } else {
        TLocal::user(name)
    }
}

fn lower_let_like(ctx: &mut LowerCtx, name: &str, init: &TExpr, mutable: bool) -> Result<(), LowerError> {
    let is_uninit = matches!(&init.kind, TExprKind::Uninit);
    let value = if is_uninit {
        None
    } else {
        Some(lower_expr(ctx, init)?)
    };
    let local = if mutable {
        TLocal::user(name).as_mutable()
    } else {
        TLocal::user(name)
    };
    let place = ctx.bind_local(
        &local,
        init.ty.clone(),
        mutable,
        false,
        is_uninit,
    )?;
    if is_uninit {
        ctx.emit(
            "stmt.let-like.initialize-uninit",
            None,
            MirOperation::InitializeUninit { place },
        )?;
    } else if let Some(value) = value {
        ctx.emit(
            "stmt.let-like.write",
            None,
            MirOperation::WritePlace { place, value },
        )?;
    }
    Ok(())
}

fn maybe_copy(
    ctx: &mut LowerCtx,
    value: jet_foundation::MIR::MirValueId,
    ty: &Type,
    copy: bool,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    if copy {
        ctx.emit("stmt.copy", Some(ty.clone()), MirOperation::Copy { value })
    } else {
        Ok(value)
    }
}

fn lower_refutable_bind(
    ctx: &mut LowerCtx,
    pattern: &TPattern,
    init: &TExpr,
    fallback: &[TStmt],
) -> Result<(), LowerError> {
    let subject = lower_expr(ctx, init)?;
    let pattern = ctx.lower_pattern(pattern)?;
    let test = ctx.lower_pattern_condition(subject, &pattern)?;
    let success = ctx.new_block(ctx.span(), "refutable.success")?;
    let miss = ctx.new_block(ctx.span(), "refutable.miss")?;
    let join = ctx.new_block(ctx.span(), "refutable.join")?;
    ctx.terminate(MirTerminator::Branch {
        condition: test,
        then_target: success,
        else_target: miss,
    });

    ctx.switch_to(success);
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
    }
    ctx.switch_to(miss);
    lower_stmts(ctx, fallback)?;
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
    }
    ctx.switch_to(join);
    Ok(())
}

fn lower_split_view(
    ctx: &mut LowerCtx,
    owner: Option<&TExpr>,
    root: &str,
    name: &str,
    start: i64,
    end: i64,
    single: bool,
    write: bool,
    elem_ty: Option<&Type>,
) -> Result<(), LowerError> {
    let element = elem_ty
        .cloned()
        .ok_or_else(|| LowerError::new(ctx.span(), format!("split view `{name}` has no checked element type")))?;
    let root_expr = TExpr {
        ty: Type::List(Box::new(element.clone())),
        kind: TExprKind::Local(TLocal::user(root)),
    };
    let base_expr = owner.unwrap_or(&root_expr);
    let start_expr = TExpr {
        ty: Type::Int,
        kind: TExprKind::IntLit(start, None),
    };
    let end_expr = TExpr {
        ty: Type::Int,
        kind: TExprKind::IntLit(end, None),
    };
    let ty = if single {
        element.clone()
    } else {
        Type::List(Box::new(element.clone()))
    };
    let value = if single {
        ctx.lower_index_value(
            base_expr,
            &start_expr,
            MirIndexKind::FixedListProof,
            &element,
            MirAccess::Read,
        )?
    } else {
        ctx.lower_slice_value(base_expr, &start_expr, &end_expr, None, &ty)?
    };
    let local = if write {
        TLocal::user(name).as_mutable()
    } else {
        TLocal::user(name)
    };
    let place = ctx.bind_local(&local, ty, write, false, false)?;
    ctx.emit(
        "stmt.split.write",
        None,
        MirOperation::WritePlace { place, value },
    )?;
    Ok(())
}

fn lower_tuple_destructure(
    ctx: &mut LowerCtx,
    init: &TExpr,
    kw: &str,
    move_fields: bool,
    binds: &[(String, String)],
) -> Result<(), LowerError> {
    let subject = lower_expr(ctx, init)?;
    let mutable = kw.contains("mut");
    for (index, (local_name, _)) in binds.iter().enumerate() {
        let field_name = ctx.field_name_for_type(&init.ty, index)?;
        let field_ty = ctx.checked_field_type(&init.ty, &field_name)?;
        let field_id = ctx.field_id_for_type(&init.ty, &field_name)?;
        let projected = ctx.emit(
            "stmt.tuple.field",
            Some(field_ty.clone()),
            MirOperation::Field {
                base: subject,
                field: field_id,
            },
        )?;
        let value = maybe_copy(ctx, projected, &field_ty, !move_fields)?;
        let local = if mutable {
            TLocal::user(local_name).as_mutable()
        } else {
            TLocal::user(local_name)
        };
        let place = ctx.bind_local(&local, field_ty, mutable, false, false)?;
        ctx.emit(
            "stmt.tuple.write",
            None,
            MirOperation::WritePlace { place, value },
        )?;
    }
    Ok(())
}

fn lower_struct_destructure(
    ctx: &mut LowerCtx,
    init: &TExpr,
    kw: &str,
    move_fields: bool,
    binds: &[(String, String)],
) -> Result<(), LowerError> {
    let subject = lower_expr(ctx, init)?;
    let mutable = kw.contains("mut");
    for (local_name, field_name) in binds {
        let field_ty = ctx.checked_field_type(&init.ty, field_name)?;
        let field_id = ctx.field_id_for_type(&init.ty, field_name)?;
        let projected = ctx.emit(
            "stmt.struct.field",
            Some(field_ty.clone()),
            MirOperation::Field {
                base: subject,
                field: field_id,
            },
        )?;
        let value = maybe_copy(ctx, projected, &field_ty, !move_fields)?;
        let local = if mutable {
            TLocal::user(local_name).as_mutable()
        } else {
            TLocal::user(local_name)
        };
        let place = ctx.bind_local(&local, field_ty, mutable, false, false)?;
        ctx.emit(
            "stmt.struct.write",
            None,
            MirOperation::WritePlace { place, value },
        )?;
    }
    Ok(())
}

fn lower_list_destructure(
    ctx: &mut LowerCtx,
    init: &TExpr,
    kw: &str,
    want: usize,
    elems: &[String],
) -> Result<(), LowerError> {
    let subject = lower_expr(ctx, init)?;
    let mutable = kw.contains("mut");
    let element_ty = match &init.ty {
        Type::List(ty) | Type::FixedList { elem: ty, .. } => (**ty).clone(),
        _ => init.ty.clone(),
    };
    for (index, local_name) in elems.iter().enumerate().take(want) {
        let index_value = const_int(ctx, index as i64)?;
        let projected = ctx.emit_index_value(
            subject,
            index_value,
            MirIndexKind::List,
            &element_ty,
            MirAccess::Read,
            "stmt.list-destructure.index",
        )?;
        let local = if mutable {
            TLocal::user(local_name).as_mutable()
        } else {
            TLocal::user(local_name)
        };
        let place = ctx.bind_local(
            &local,
            element_ty.clone(),
            mutable,
            false,
            false,
        )?;
        ctx.emit(
            "stmt.list-destructure.write",
            None,
            MirOperation::WritePlace { place, value: projected },
        )?;
    }
    Ok(())
}

fn lower_assign(
    ctx: &mut LowerCtx,
    place: &crate::Codegen::TIR::TPlace,
    op: Option<BinOp>,
    value: &TExpr,
    clone_value: bool,
) -> Result<(), LowerError> {
    // A structured place can evaluate a receiver/index.  TIR's assignment
    // emitter evaluates the RHS before that place, so retain the same order.
    let rhs = lower_expr(ctx, value)?;
    let rhs = maybe_copy(ctx, rhs, &value.ty, clone_value)?;
    let place_id = ctx.lower_place(place, MirAccess::Write)?;
    let assigned = if let Some(op) = op {
        let old = ctx.emit(
            "stmt.assign.read",
            Some(value.ty.clone()),
            MirOperation::ReadPlace(place_id),
        )?;
        ctx.emit(
            "stmt.assign.rmw",
            Some(value.ty.clone()),
            MirOperation::Binary {
                op: super::mir_binary_op(op),
                dispatch: MirBinaryDispatch::Primitive,
                left: old,
                right: rhs,
            },
        )?
    } else {
        rhs
    };
    ctx.emit(
        "stmt.assign.write",
        None,
        MirOperation::WritePlace {
            place: place_id,
            value: assigned,
        },
    )?;
    Ok(())
}

fn lower_index_field_assign(ctx: &mut LowerCtx, assign: &TIndexFieldAssign) -> Result<(), LowerError> {
    // Match regular assignment: evaluate the RHS before the indexed field
    // place, whose receiver and index expressions may have effects.
    let rhs = lower_expr(ctx, &assign.value)?;
    let rhs = maybe_copy(ctx, rhs, &assign.value.ty, assign.clone_value)?;
    let base_place = ctx.lower_index_place(
        &assign.base,
        &assign.index,
        if assign.is_map {
            MirIndexKind::Map
        } else {
            MirIndexKind::List
        },
        MirAccess::Write,
    )?;
    let place = ctx.project_field_place(
        base_place,
        &assign.field,
        assign.field_ty.clone(),
        ctx.span(),
    )?;
    let assigned = if let Some(op) = assign.op {
        let old = ctx.emit(
            "stmt.index-field.read",
            Some(assign.field_ty.clone()),
            MirOperation::ReadPlace(place),
        )?;
        ctx.emit(
            "stmt.index-field.rmw",
            Some(assign.field_ty.clone()),
            MirOperation::Binary {
                op: super::mir_binary_op(op),
                dispatch: MirBinaryDispatch::Primitive,
                left: old,
                right: rhs,
            },
        )?
    } else {
        rhs
    };
    ctx.emit(
        "stmt.index-field.write",
        None,
        MirOperation::WritePlace { place, value: assigned },
    )?;
    Ok(())
}
fn lower_swizzle_assign(
    ctx: &mut LowerCtx,
    base: &TExpr,
    lanes: &[u8],
    value: &TExpr,
    clone_value: bool,
) -> Result<(), LowerError> {
    let rhs = lower_expr(ctx, value)?;
    let places = ctx.lower_swizzle_place(base, lanes, MirAccess::Write)?;
    for (index, place) in places.into_iter().enumerate() {
        let value = maybe_copy(
            ctx,
            rhs,
            &value.ty,
            clone_value || index + 1 < lanes.len(),
        )?;
        ctx.emit(
            "stmt.swizzle.write",
            None,
            MirOperation::WritePlace { place, value },
        )?;
    }
    Ok(())
}

fn lower_if(
    ctx: &mut LowerCtx,
    cond: &TIfCond,
    then_body: &[TStmt],
    else_body: Option<&[TStmt]>,
) -> Result<(), LowerError> {
    let then_block = ctx.new_block(ctx.span(), "if.then")?;
    let else_block = ctx.new_block(ctx.span(), "if.else")?;
    let join = ctx.new_block(ctx.span(), "if.join")?;
    lower_cond(ctx, cond, then_block, else_block)?;

    ctx.switch_to(then_block);
    lower_stmts(ctx, then_body)?;
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
    }

    ctx.switch_to(else_block);
    if let Some(body) = else_body {
        lower_stmts(ctx, body)?;
    }
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
    }
    ctx.switch_to(join);
    Ok(())
}

fn lower_cond(
    ctx: &mut LowerCtx,
    cond: &TIfCond,
    then_target: MirBlockId,
    else_target: MirBlockId,
) -> Result<(), LowerError> {
    match cond {
        TIfCond::Plain(expr) => {
            let value = lower_expr(ctx, expr)?;
            ctx.terminate(MirTerminator::Branch {
                condition: value,
                then_target,
                else_target,
            });
        }
        TIfCond::And { left, right } => {
            let right_block = ctx.new_block(ctx.span(), "if.and.right")?;
            lower_cond(ctx, left, right_block, else_target)?;
            ctx.switch_to(right_block);
            lower_cond(ctx, right, then_target, else_target)?;
        }
        TIfCond::IfLet { pattern, subj } => {
            let subject = lower_expr(ctx, subj)?;
            let pattern = ctx.lower_pattern(pattern)?;
            let test = ctx.lower_pattern_condition(subject, &pattern)?;
            ctx.terminate(MirTerminator::Branch {
                condition: test,
                then_target,
                else_target,
            });
        }
        TIfCond::IsNone { subj } => {
            let subject = lower_expr(ctx, subj)?;
            let present = ctx.emit(
                "if.is-none.test",
                Some(Type::Bool),
                MirOperation::OptionIsSome { subject },
            )?;
            ctx.terminate(MirTerminator::Branch {
                condition: present,
                then_target: else_target,
                else_target: then_target,
            });
        }
        TIfCond::Matches { pattern, subj } => {
            let subject = lower_expr(ctx, subj)?;
            let pattern = ctx.lower_pattern(pattern)?;
            let test = ctx.lower_pattern_condition(subject, &pattern)?;
            ctx.terminate(MirTerminator::Branch {
                condition: test,
                then_target,
                else_target,
            });
        }
        TIfCond::WithPrelude { prelude, cond } => {
            lower_stmts(ctx, prelude)?;
            if !ctx.is_terminated() {
                lower_cond(ctx, cond, then_target, else_target)?;
            }
        }
    }
    Ok(())
}

fn lower_loop(ctx: &mut LowerCtx, label: Option<&str>, body: &[TStmt]) -> Result<(), LowerError> {
    let header = ctx.new_block(ctx.span(), "loop.header")?;
    let body_block = ctx.new_block(ctx.span(), "loop.body")?;
    let exit = ctx.new_block(ctx.span(), "loop.exit")?;
    ctx.terminate(MirTerminator::Jump { target: header });
    ctx.switch_to(header);
    ctx.terminate(MirTerminator::Jump { target: body_block });
    ctx.switch_to(body_block);
    ctx.push_lexical_frame();
    ctx.push_loop(label.map(str::to_string), exit, header);
    lower_stmts(ctx, body)?;
    ctx.pop_loop();
    ctx.pop_lexical_frame()?;
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: header });
    }
    ctx.switch_to(exit);
    Ok(())
}

fn lower_while(
    ctx: &mut LowerCtx,
    label: Option<&str>,
    cond: &TExpr,
    body: &[TStmt],
) -> Result<(), LowerError> {
    let header = ctx.new_block(ctx.span(), "while.header")?;
    let body_block = ctx.new_block(ctx.span(), "while.body")?;
    let exit = ctx.new_block(ctx.span(), "while.exit")?;
    ctx.terminate(MirTerminator::Jump { target: header });
    ctx.switch_to(header);
    let condition = lower_expr(ctx, cond)?;
    ctx.terminate(MirTerminator::Branch {
        condition,
        then_target: body_block,
        else_target: exit,
    });
    ctx.switch_to(body_block);
    ctx.push_lexical_frame();
    ctx.push_loop(label.map(str::to_string), exit, header);
    lower_stmts(ctx, body)?;
    ctx.pop_loop();
    ctx.pop_lexical_frame()?;
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: header });
    }
    ctx.switch_to(exit);
    Ok(())
}

fn lower_counted_loop(
    ctx: &mut LowerCtx,
    label: Option<&str>,
    init: &TStmt,
    cond: &TExpr,
    step: Option<&TStmt>,
    body: &[TStmt],
) -> Result<(), LowerError> {
    lower_stmt(ctx, init)?;
    if ctx.is_terminated() {
        return Ok(());
    }
    let header = ctx.new_block(ctx.span(), "counted.header")?;
    let body_block = ctx.new_block(ctx.span(), "counted.body")?;
    let step_block = ctx.new_block(ctx.span(), "counted.step")?;
    let exit = ctx.new_block(ctx.span(), "counted.exit")?;
    ctx.terminate(MirTerminator::Jump { target: header });
    ctx.switch_to(header);
    let condition = lower_expr(ctx, cond)?;
    ctx.terminate(MirTerminator::Branch {
        condition,
        then_target: body_block,
        else_target: exit,
    });
    ctx.switch_to(body_block);
    ctx.push_lexical_frame();
    ctx.push_loop(label.map(str::to_string), exit, step_block);
    lower_stmts(ctx, body)?;
    ctx.pop_loop();
    ctx.pop_lexical_frame()?;
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: step_block });
    }
    ctx.switch_to(step_block);
    if let Some(step) = step {
        lower_stmt(ctx, step)?;
    }
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: header });
    }
    ctx.switch_to(exit);
    Ok(())
}

fn lower_range_loop(
    ctx: &mut LowerCtx,
    label: Option<&str>,
    var: &str,
    source: Option<&TExpr>,
    start: &TExpr,
    end: &TExpr,
    step: Option<&TExpr>,
    exclusive: bool,
    body: &[TStmt],
) -> Result<(), LowerError> {
    let routes = super::loop_route_bundle();
    let range_init_call = ctx.intern_prelude_route(routes.range_init)?;
    let range_has_next_call = ctx.intern_prelude_route(routes.range_has_next)?;
    let range_value_call = ctx.intern_prelude_route(routes.range_value)?;
    let range_advance_call = ctx.intern_prelude_route(routes.range_advance)?;
    let iter_init_call = ctx.intern_prelude_route(routes.iter_init)?;
    let iter_has_next_call = ctx.intern_prelude_route(routes.iter_has_next)?;
    let iter_value_call = ctx.intern_prelude_route(routes.iter_value)?;
    let iter_advance_call = ctx.intern_prelude_route(routes.iter_advance)?;
    let (cursor, item_ty, iterator) = if let Some(source) = source {
        let collection = lower_expr(ctx, source)?;
        let step_value = step.map(|step| lower_expr(ctx, step)).transpose()?;
        let cursor_ty = Type::Named("RangeCursor".to_string());
        let cursor = ctx.emit(
            "range.iter-init",
            Some(cursor_ty),
            MirOperation::LoopIterInit {
                call: iter_init_call,
                collection,
                step: step_value,
                by_value: false,
                source_kind: MirLoopSourceKind::Plain,
            },
        )?;
        (cursor, start.ty.clone(), true)
    } else {
        let start_value = lower_expr(ctx, start)?;
        let end_value = lower_expr(ctx, end)?;
        let step_value = step.map(|step| lower_expr(ctx, step)).transpose()?;
        let cursor_ty = Type::Named("RangeCursor".to_string());
        let cursor = ctx.emit(
            "range.init",
            Some(cursor_ty),
            MirOperation::LoopRangeInit {
                call: range_init_call,
                start: start_value,
                end: end_value,
                step: step_value,
                exclusive,
            },
        )?;
        (cursor, start.ty.clone(), false)
    };
    let header = ctx.new_block(ctx.span(), "range.header")?;
    let body_block = ctx.new_block(ctx.span(), "range.body")?;
    let advance = ctx.new_block(ctx.span(), "range.advance")?;
    let exit = ctx.new_block(ctx.span(), "range.exit")?;
    ctx.terminate(MirTerminator::Jump { target: header });
    ctx.switch_to(header);
    let has_next = if iterator {
        ctx.emit(
            "range.iter-has-next",
            Some(Type::Bool),
            MirOperation::LoopIterHasNext {
                call: iter_has_next_call,
                cursor,
            },
        )?
    } else {
        ctx.emit(
            "range.has-next",
            Some(Type::Bool),
            MirOperation::LoopRangeHasNext {
                call: range_has_next_call,
                cursor,
            },
        )?
    };
    ctx.terminate(MirTerminator::Branch {
        condition: has_next,
        then_target: body_block,
        else_target: exit,
    });
    ctx.switch_to(body_block);
    let item = if iterator {
        ctx.emit(
            "range.iter-value",
            Some(item_ty.clone()),
            MirOperation::LoopIterValue {
                call: iter_value_call,
                cursor,
            },
        )?
    } else {
        ctx.emit(
            "range.value",
            Some(item_ty.clone()),
            MirOperation::LoopRangeValue {
                call: range_value_call,
                cursor,
            },
        )?
    };
    bind_value(ctx, var, item, item_ty, false)?;
    ctx.push_lexical_frame();
    ctx.push_loop(label.map(str::to_string), exit, advance);
    lower_stmts(ctx, body)?;
    ctx.pop_loop();
    ctx.pop_lexical_frame()?;
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: advance });
    }
    ctx.switch_to(advance);
    if iterator {
        ctx.emit(
            "range.iter-advance",
            None,
            MirOperation::LoopIterAdvance {
                call: iter_advance_call,
                cursor,
            },
        )?;
    } else {
        ctx.emit(
            "range.advance",
            None,
            MirOperation::LoopRangeAdvance {
                call: range_advance_call,
                cursor,
            },
        )?;
    }
    ctx.terminate(MirTerminator::Jump { target: header });
    ctx.switch_to(exit);
    Ok(())
}
fn lower_for_in(
    ctx: &mut LowerCtx,
    label: Option<&str>,
    var: &str,
    var2: Option<&str>,
    source: &TExpr,
    collection: &TExpr,
    step: Option<&TExpr>,
    method_kind: Option<&crate::Codegen::TIR::TForInMethod>,
    by_value: bool,
    body: &[TStmt],
) -> Result<(), LowerError> {
    let routes = super::loop_route_bundle();
    let iter_init_call = ctx.intern_prelude_route(routes.iter_init)?;
    let iter_has_next_call = ctx.intern_prelude_route(routes.iter_has_next)?;
    let iter_value_call = ctx.intern_prelude_route(routes.iter_value)?;
    let iter_advance_call = ctx.intern_prelude_route(routes.iter_advance)?;
    let source_kind = loop_source_kind(method_kind);
    // A sequence's two-binding form is `(index, item)`, not a projection from
    // the element itself. Reuse the canonical indexed adapter so the cursor
    // receives the same named tuple shape as an explicit `.indexed()` call.
    let (loop_collection, indexed_pair_types) = match (method_kind, &collection.ty, var2) {
        (
            None,
            Type::List(elem) | Type::FixedList { elem, .. },
            Some(_),
        ) => {
            let elem_ty = (**elem).clone();
            let fields = vec![
                ("idx".to_string(), Type::Int),
                ("item".to_string(), elem_ty.clone()),
            ];
            let row_ty = Type::Tuple(
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), Box::new(ty.clone())))
                    .collect(),
            );
            let indexed = TExpr {
                ty: crate::Collections::iter_ty(row_ty.clone()),
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(collection.clone()),
                    op: TBuiltinOp::Indexed {
                        tuple_struct: crate::Codegen::Tuples::tuple_struct_name(&fields),
                    },
                    args: Vec::new(),
                },
            };
            (indexed, Some((row_ty, elem_ty)))
        }
        _ => (collection.clone(), None),
    };
    let owns_source = match &source_kind {
        MirLoopSourceKind::LinesFile
        | MirLoopSourceKind::LinesStdin
        | MirLoopSourceKind::LinesProcessStream
        | MirLoopSourceKind::ChannelReceiver
        | MirLoopSourceKind::EncodingReader { .. }
        | MirLoopSourceKind::Iterable { .. } => true,
        MirLoopSourceKind::Plain => crate::Collections::iter_elem(&loop_collection.ty).is_some(),
        MirLoopSourceKind::Chars => false,
    };

    let effective_by_value = owns_source || by_value;
    let source = if method_kind.is_some() {
        source
    } else {
        &loop_collection
    };
    let source_value = match (&source.kind, owns_source) {
        (TExprKind::Local(local), true) => {
            let place = ctx.place_for_local(local, MirAccess::Move)?;
            ctx.emit(
                "for-in.source-move",
                Some(source.ty.clone()),
                MirOperation::MovePlace { place },
            )?
        }
        _ => lower_expr(ctx, source)?,
    };
    let step_value = step.map(|step| lower_expr(ctx, step)).transpose()?;
    let cursor_ty = Type::Named("IterCursor".to_string());
    let cursor = ctx.emit(
        "for-in.iter-init",
        Some(cursor_ty),
        MirOperation::LoopIterInit {
            call: iter_init_call,
            collection: source_value,
            step: step_value,
            by_value: effective_by_value,
            source_kind,
        },
    )?;
    let item_ty = indexed_pair_types
        .as_ref()
        .map(|(row_ty, _)| row_ty.clone())
        .unwrap_or_else(|| for_item_type(collection, method_kind));
    let header = ctx.new_block(ctx.span(), "for-in.header")?;
    let body_block = ctx.new_block(ctx.span(), "for-in.body")?;
    let advance = ctx.new_block(ctx.span(), "for-in.advance")?;
    let exit = ctx.new_block(ctx.span(), "for-in.exit")?;
    ctx.terminate(MirTerminator::Jump { target: header });
    ctx.switch_to(header);
    let has_next = ctx.emit(
        "for-in.has-next",
        Some(Type::Bool),
        MirOperation::LoopIterHasNext {
            call: iter_has_next_call,
            cursor,
        },
    )?;
    ctx.terminate(MirTerminator::Branch {
        condition: has_next,
        then_target: body_block,
        else_target: exit,
    });
    ctx.switch_to(body_block);
    let item = ctx.emit(
        "for-in.value",
        Some(item_ty.clone()),
        MirOperation::LoopIterValue {
            call: iter_value_call,
            cursor,
        },
    )?;
    if let Some(var2) = var2 {
        let (key_ty, value_ty, key_name, value_name) =
            match (&collection.ty, indexed_pair_types.as_ref()) {
                (Type::Map { key, value, .. }, _) => (
                    (**key).clone(),
                    (**value).clone(),
                    "key",
                    "value",
                ),
                (_, Some((_, elem_ty))) => {
                    (Type::Int, elem_ty.clone(), "idx", "item")
                }
                _ => (Type::Int, item_ty.clone(), "key", "value"),
            };
        let key_field = ctx.field_id_for_type(&item_ty, key_name)?;
        let value_field = ctx.field_id_for_type(&item_ty, value_name)?;
        let key = ctx.emit(
            "for-in.key",
            Some(key_ty.clone()),
            MirOperation::Field {
                base: item,
                field: key_field,
            },
        )?;
        let val = ctx.emit(
            "for-in.value-field",
            Some(value_ty.clone()),
            MirOperation::Field {
                base: item,
                field: value_field,
            },
        )?;
        bind_value(ctx, var, key, key_ty, false)?;
        bind_value(ctx, var2, val, value_ty, false)?;
    } else {
        bind_value(ctx, var, item, item_ty, false)?;
    }
    ctx.push_lexical_frame();
    ctx.push_loop(label.map(str::to_string), exit, advance);
    lower_stmts(ctx, body)?;
    ctx.pop_loop();
    ctx.pop_lexical_frame()?;
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: advance });
    }
    ctx.switch_to(advance);
    ctx.emit(
        "for-in.advance",
        None,
        MirOperation::LoopIterAdvance {
            call: iter_advance_call,
            cursor,
        },
    )?;
    ctx.terminate(MirTerminator::Jump { target: header });
    ctx.switch_to(exit);
    Ok(())
}

fn loop_source_kind(method_kind: Option<&TForInMethod>) -> MirLoopSourceKind {
    match method_kind {
        None => MirLoopSourceKind::Plain,
        Some(TForInMethod::Chars) => MirLoopSourceKind::Chars,
        Some(TForInMethod::LinesFile) => MirLoopSourceKind::LinesFile,
        Some(TForInMethod::LinesStdin) => MirLoopSourceKind::LinesStdin,
        Some(TForInMethod::LinesProcessStream) => MirLoopSourceKind::LinesProcessStream,
        Some(TForInMethod::ChannelReceiver) => MirLoopSourceKind::ChannelReceiver,
        Some(TForInMethod::EncodingReader { reader_type }) => {
            MirLoopSourceKind::EncodingReader {
                reader_type: reader_type.clone(),
            }
        }
        Some(TForInMethod::Iterable {
            coll_type,
            iter_type,
            iter_symbol,
            next_symbol,
        }) => MirLoopSourceKind::Iterable {
            coll_type: coll_type.clone(),
            iter_type: iter_type.clone(),
            iter_symbol: iter_symbol.clone(),
            next_symbol: next_symbol.clone(),
        },
    }
}

fn for_item_type(
    collection: &TExpr,
    method_kind: Option<&crate::Codegen::TIR::TForInMethod>,
) -> Type {
    if let Some(elem) = crate::Collections::iter_elem(&collection.ty) {
        return elem.clone();
    }
    if let Some(method) = method_kind {
        return match method {
            crate::Codegen::TIR::TForInMethod::Chars => Type::Char,
            crate::Codegen::TIR::TForInMethod::LinesFile
            | crate::Codegen::TIR::TForInMethod::LinesStdin
            | crate::Codegen::TIR::TForInMethod::LinesProcessStream => Type::String,
            _ => match &collection.ty {
                Type::List(elem) | Type::FixedList { elem, .. } => (**elem).clone(),
                _ => collection.ty.clone(),
            },
        };
    }
    match &collection.ty {
        Type::List(elem) | Type::FixedList { elem, .. } => (**elem).clone(),
        Type::Map { key, value, .. } => Type::Tuple(vec![
            ("key".to_string(), key.clone()),
            ("value".to_string(), value.clone()),
        ]),
        _ => collection.ty.clone(),
    }
}

fn bind_value(
    ctx: &mut LowerCtx,
    name: &str,
    value: jet_foundation::MIR::MirValueId,
    ty: Type,
    mutable: bool,
) -> Result<(), LowerError> {
    let local = if mutable {
        TLocal::user(name).as_mutable()
    } else {
        TLocal::user(name)
    };
    let place = ctx.bind_local(&local, ty, mutable, false, false)?;
    ctx.emit(
        "stmt.bind.write",
        None,
        MirOperation::WritePlace { place, value },
    )?;
    Ok(())
}

fn lower_enum_match(
    ctx: &mut LowerCtx,
    scrutinee: &TExpr,
    clone_subject: bool,
    arms: &[TMatchArm],
    else_body: Option<&[TStmt]>,
    fallthrough: bool,
) -> Result<(), LowerError> {
    let mut subject = lower_expr(ctx, scrutinee)?;
    if clone_subject {
        subject = ctx.emit(
            "match.subject.copy",
            Some(scrutinee.ty.clone()),
            MirOperation::Copy { value: subject },
        )?;
    }
    let join = ctx.new_block(ctx.span(), "match.join")?;
    let otherwise = ctx.new_block(ctx.span(), "match.otherwise")?;
    let mut tests = Vec::with_capacity(arms.len());
    let mut bodies = Vec::with_capacity(arms.len());
    for index in 0..arms.len() {
        tests.push(ctx.new_block(ctx.span(), &format!("match.test.{index}"))?);
        bodies.push(ctx.new_block(ctx.span(), &format!("match.body.{index}"))?);
    }
    if let Some(first) = tests.first().copied() {
        ctx.terminate(MirTerminator::Jump { target: first });
    } else {
        ctx.terminate(MirTerminator::Jump { target: otherwise });
    }
    for (index, arm) in arms.iter().enumerate() {
        ctx.switch_to(tests[index]);
        let pattern = ctx.lower_pattern(&arm.pattern)?;
        let condition = ctx.lower_pattern_condition(subject, &pattern)?;
        let next = tests.get(index + 1).copied().unwrap_or(otherwise);
        ctx.terminate(MirTerminator::Branch {
            condition,
            then_target: bodies[index],
            else_target: next,
        });
        ctx.switch_to(bodies[index]);
        ctx.push_lexical_frame();
        lower_stmts(ctx, &arm.body)?;
        ctx.pop_lexical_frame()?;
        if !ctx.is_terminated() {
            ctx.terminate(MirTerminator::Jump { target: join });
        }
    }
    ctx.switch_to(otherwise);
    ctx.push_lexical_frame();
    if let Some(body) = else_body {
        lower_stmts(ctx, body)?;
    } else if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Unreachable {
            reason: if fallthrough {
                "checked exhaustive match fallthrough".to_string()
            } else {
                "checked exhaustive match otherwise".to_string()
            },
        });
    }
    ctx.pop_lexical_frame()?;
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
    }
    ctx.switch_to(join);
    Ok(())
}

fn lower_range_switch(
    ctx: &mut LowerCtx,
    subject: &TExpr,
    arms: &[(i64, i64, Vec<TStmt>)],
    else_body: &[TStmt],
) -> Result<(), LowerError> {
    let subject_value = lower_expr(ctx, subject)?;
    let join = ctx.new_block(ctx.span(), "range-switch.join")?;
    let otherwise = ctx.new_block(ctx.span(), "range-switch.otherwise")?;
    let mut tests = Vec::with_capacity(arms.len());
    let mut bodies = Vec::with_capacity(arms.len());
    for index in 0..arms.len() {
        tests.push(ctx.new_block(ctx.span(), &format!("range-switch.test.{index}"))?);
        bodies.push(ctx.new_block(ctx.span(), &format!("range-switch.body.{index}"))?);
    }
    if let Some(first) = tests.first().copied() {
        ctx.terminate(MirTerminator::Jump { target: first });
    } else {
        ctx.terminate(MirTerminator::Jump { target: otherwise });
    }
    for (index, (lo, hi, _)) in arms.iter().enumerate() {
        ctx.switch_to(tests[index]);
        let lo = const_int(ctx, *lo)?;
        let hi = const_int(ctx, *hi)?;
        let lower = ctx.emit(
            &format!("range-switch.{index}.lower"),
            Some(Type::Bool),
            MirOperation::Binary {
                op: super::mir_binary_op(BinOp::Ge),
                dispatch: MirBinaryDispatch::Primitive,
                left: subject_value,
                right: lo,
            },
        )?;
        let upper = ctx.emit(
            &format!("range-switch.{index}.upper"),
            Some(Type::Bool),
            MirOperation::Binary {
                op: super::mir_binary_op(BinOp::Le),
                dispatch: MirBinaryDispatch::Primitive,
                left: subject_value,
                right: hi,
            },
        )?;
        let condition = ctx.emit(
            &format!("range-switch.{index}.condition"),
            Some(Type::Bool),
            MirOperation::Binary {
                op: super::mir_binary_op(BinOp::And),
                dispatch: MirBinaryDispatch::Primitive,
                left: lower,
                right: upper,
            },
        )?;
        let next = tests.get(index + 1).copied().unwrap_or(otherwise);
        ctx.terminate(MirTerminator::Branch {
            condition,
            then_target: bodies[index],
            else_target: next,
        });
        ctx.switch_to(bodies[index]);
        ctx.push_lexical_frame();
        lower_stmts(ctx, &arms[index].2)?;
        ctx.pop_lexical_frame()?;
        if !ctx.is_terminated() {
            ctx.terminate(MirTerminator::Jump { target: join });
        }
    }
    ctx.switch_to(otherwise);
    ctx.push_lexical_frame();
    lower_stmts(ctx, else_body)?;
    ctx.pop_lexical_frame()?;
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
    }
    ctx.switch_to(join);
    Ok(())
}

fn lower_mixed_switch(
    ctx: &mut LowerCtx,
    subject: &TExpr,
    arms: &[(TExpr, Vec<TStmt>)],
    else_body: Option<&[TStmt]>,
) -> Result<(), LowerError> {
    let subject_value = lower_expr(ctx, subject)?;
    let join = ctx.new_block(ctx.span(), "mixed-switch.join")?;
    let otherwise = ctx.new_block(ctx.span(), "mixed-switch.otherwise")?;
    let mut tests = Vec::with_capacity(arms.len());
    let mut bodies = Vec::with_capacity(arms.len());
    for index in 0..arms.len() {
        tests.push(ctx.new_block(ctx.span(), &format!("mixed-switch.test.{index}"))?);
        bodies.push(ctx.new_block(ctx.span(), &format!("mixed-switch.body.{index}"))?);
    }
    if let Some(first) = tests.first().copied() {
        ctx.terminate(MirTerminator::Jump { target: first });
    } else {
        ctx.terminate(MirTerminator::Jump { target: otherwise });
    }
    for (index, (condition, body)) in arms.iter().enumerate() {
        ctx.switch_to(tests[index]);
        let condition = ctx.with_switch_subject(subject_value, |ctx| lower_expr(ctx, condition))?;
        let next = tests.get(index + 1).copied().unwrap_or(otherwise);
        ctx.terminate(MirTerminator::Branch {
            condition,
            then_target: bodies[index],
            else_target: next,
        });
        ctx.with_switch_subject(subject_value, |ctx| {
            ctx.push_lexical_frame();
            lower_stmts(ctx, body)?;
            ctx.pop_lexical_frame()
        })?;
        if !ctx.is_terminated() {
            ctx.terminate(MirTerminator::Jump { target: join });
        }
    }
    ctx.switch_to(otherwise);
    ctx.push_lexical_frame();
    if let Some(body) = else_body {
        lower_stmts(ctx, body)?;
    }
    ctx.pop_lexical_frame()?;
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
    }
    ctx.switch_to(join);
    Ok(())
}

fn lower_debug_only(ctx: &mut LowerCtx, body: &[TStmt]) -> Result<(), LowerError> {
    let scope = ctx.enter_scope(
        MirScopeKind::DebugOnly,
        ctx.span(),
        Some("debug-only".to_string()),
    )?;
    lower_stmts(ctx, body)?;
    ctx.exit_scope(scope)?;
    Ok(())
}

fn lower_scoped(
    ctx: &mut LowerCtx,
    kind: MirScopeKind,
    name: Option<String>,
    body: &[TStmt],
) -> Result<(), LowerError> {
    let scope = ctx.enter_scope(kind, ctx.span(), name)?;
    lower_stmts(ctx, body)?;
    ctx.exit_scope(scope)?;
    Ok(())
}

fn lower_scope_member(
    ctx: &mut LowerCtx,
    kind: &ScopeMemberKind,
    body: &[TStmt],
) -> Result<(), LowerError> {
    match kind {
        ScopeMemberKind::Setup => lower_stmts(ctx, body),
        ScopeMemberKind::ExpectFail(code) => {
            let scope = ctx.enter_scope(MirScopeKind::ScopeMember, ctx.span(), code.clone())?;
            attach_scope_member(
                ctx,
                scope,
                MirTestScopeMember::ExpectFail {
                    expected_code: code.clone(),
                },
            )?;
            lower_scope_member_body(ctx, scope, body)
        }
        ScopeMemberKind::Timeout(duration) => {
            let value = lower_expr(ctx, duration)?;
            let scope = ctx.enter_scope(
                MirScopeKind::ScopeMember,
                ctx.span(),
                Some("timeout".to_string()),
            )?;
            attach_scope_member(ctx, scope, MirTestScopeMember::Timeout { duration: value })?;
            lower_scope_member_body(ctx, scope, body)
        }
        ScopeMemberKind::Measure => {
            let scope = ctx.enter_scope(
                MirScopeKind::ScopeMember,
                ctx.span(),
                Some("measure".to_string()),
            )?;
            attach_scope_member(ctx, scope, MirTestScopeMember::Measure)?;
            lower_scope_member_body(ctx, scope, body)
        }
        ScopeMemberKind::Skip => {
            let whole_test = ctx.scopes.is_empty()
                && ctx.current == ctx.entry
                && ctx
                    .blocks
                    .iter()
                    .find(|block| block.id == ctx.current)
                    .is_some_and(|block| block.instructions.is_empty());
            let scope = ctx.enter_scope(
                MirScopeKind::ScopeMember,
                ctx.span(),
                Some("skip".to_string()),
            )?;
            attach_scope_member(ctx, scope, MirTestScopeMember::Skip { whole_test })?;

            // Keep the checked body in MIR for diagnostics, but make its path
            // unreachable exactly as the test harness's `if false` shape does.
            let body_block = ctx.new_block(ctx.span(), "scope-member.skip.body")?;
            let skipped_block = ctx.new_block(ctx.span(), "scope-member.skip.skipped")?;
            let exit = ctx.new_block(ctx.span(), "scope-member.skip.exit")?;
            let join = ctx.new_block(ctx.span(), "scope-member.skip.join")?;
            let false_value = ctx.emit(
                "scope-member.skip.condition",
                Some(Type::Bool),
                MirOperation::Constant(MirConstant::Bool(false)),
            )?;
            ctx.terminate(MirTerminator::Branch {
                condition: false_value,
                then_target: body_block,
                else_target: skipped_block,
            });
            ctx.switch_to(body_block);
            let body_block_start = ctx.blocks.len();
            ctx.push_lexical_frame();
            lower_stmts(ctx, body)?;
            ctx.pop_lexical_frame()?;
            emit_scope_exits_on_early_paths(ctx, scope, body_block, body_block_start)?;
            if !ctx.is_terminated() {
                ctx.terminate(MirTerminator::Jump { target: exit });
            }
            ctx.switch_to(skipped_block);
            ctx.terminate(MirTerminator::Jump { target: exit });
            ctx.switch_to(exit);
            ctx.exit_scope(scope)?;
            if !ctx.is_terminated() {
                ctx.terminate(MirTerminator::Jump { target: join });
            }
            ctx.switch_to(join);
            Ok(())
        }
    }
}

fn attach_scope_member(
    ctx: &mut LowerCtx,
    scope: MirScopeId,
    member: MirTestScopeMember,
) -> Result<(), LowerError> {
    let current = ctx.current_block();
    let span = ctx.span();
    let Some(instruction) = ctx
        .blocks
        .iter_mut()
        .find(|block| block.id == current)
        .and_then(|block| block.instructions.last_mut())
    else {
        return Err(ctx.error(span, "MIR scope member has no scope-enter operation"));
    };
    match &mut instruction.operation {
        MirOperation::ScopeEnter {
            scope: candidate,
            test_member,
        } if *candidate == scope => {
            *test_member = Some(member);
            Ok(())
        }
        _ => Err(ctx.error(span, "MIR scope member has no scope-enter operation")),
    }
}

fn emit_scope_exits_on_early_paths(
    ctx: &mut LowerCtx,
    scope: MirScopeId,
    body_entry: MirBlockId,
    body_block_start: usize,
) -> Result<(), LowerError> {
    let body_blocks = ctx.blocks[body_block_start..]
        .iter()
        .map(|block| block.id)
        .chain(std::iter::once(body_entry))
        .collect::<Vec<_>>();
    let early_exits = body_blocks
        .iter()
        .copied()
        .filter(|block_id| {
            let Some(block) = ctx.blocks.iter().find(|block| block.id == *block_id) else {
                return false;
            };
            let leaves_scope = match &block.terminator {
                MirTerminator::Return { .. } => true,
                MirTerminator::Break { target, .. } | MirTerminator::Continue { target } => {
                    !body_blocks.contains(target)
                }
                _ => false,
            };
            leaves_scope
                && !block.instructions.iter().any(|instruction| {
                    matches!(
                        &instruction.operation,
                        MirOperation::ScopeExit { scope: candidate } if *candidate == scope
                    )
                })
        })
        .collect::<Vec<_>>();
    let current = ctx.current_block();
    for block in early_exits {
        ctx.switch_to(block);
        ctx.emit(
            &format!("scope-member.exit.early.{}", block.0),
            None,
            MirOperation::ScopeExit { scope },
        )?;
    }
    ctx.switch_to(current);
    Ok(())
}

fn lower_scope_member_body(
    ctx: &mut LowerCtx,
    scope: MirScopeId,
    body: &[TStmt],
) -> Result<(), LowerError> {
    let body_entry = ctx.current_block();
    let body_block_start = ctx.blocks.len();
    lower_stmts(ctx, body)?;
    emit_scope_exits_on_early_paths(ctx, scope, body_entry, body_block_start)?;
    if ctx.is_terminated() {
        ctx.exit_scope(scope)?;
        return Ok(());
    }
    let exit = ctx.new_block(ctx.span(), "scope-member.exit")?;
    let join = ctx.new_block(ctx.span(), "scope-member.join")?;
    ctx.terminate(MirTerminator::Jump { target: exit });
    ctx.switch_to(exit);
    ctx.exit_scope(scope)?;
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
    }
    ctx.switch_to(join);
    Ok(())
}

fn lower_transaction(
    ctx: &mut LowerCtx,
    handle: Option<&TLocal>,
    snapshots: &[(TLocal, Option<Type>)],
    stm: Option<&TLocal>,
    body: &[TStmt],
) -> Result<(), LowerError> {
    let scope = ctx.enter_scope(MirScopeKind::Transaction, ctx.span(), None)?;
    if let Some(handle) = handle {
        let ty = Type::Named("Transaction".to_string());
        let place = ctx.bind_local(handle, ty.clone(), handle.mutable, false, false)?;
        let value = ctx.emit(
            "transaction.handle.global",
            Some(ty),
            MirOperation::Global {
                name: "transaction".to_string(),
            },
        )?;
        ctx.emit(
            "transaction.handle.write",
            None,
            MirOperation::WritePlace { place, value },
        )?;
    }
    if let Some(stm) = stm {
        let ty = Type::Named("Stm".to_string());
        let place = ctx.bind_local(stm, ty.clone(), stm.mutable, false, false)?;
        let value = ctx.emit(
            "transaction.stm.global",
            Some(ty),
            MirOperation::Global {
                name: "stm".to_string(),
            },
        )?;
        ctx.emit(
            "transaction.stm.write",
            None,
            MirOperation::WritePlace { place, value },
        )?;
    }
    for (index, (local, ty)) in snapshots.iter().enumerate() {
        let place = ctx.lower_place(
            &crate::Codegen::TIR::TPlace::Local(local.clone()),
            MirAccess::Read,
        )?;
        let snapshot_ty = ty.clone().unwrap_or(Type::Named("Snapshot".to_string()));
        let value = ctx.emit(
            &format!("transaction.snapshot.{index}.read"),
            Some(snapshot_ty.clone()),
            MirOperation::ReadPlace(place),
        )?;
        let _ = ctx.emit(
            &format!("transaction.snapshot.{index}.copy"),
            Some(snapshot_ty),
            MirOperation::Copy { value },
        )?;
    }
    lower_stmts(ctx, body)?;
    ctx.exit_scope(scope)?;
    Ok(())
}

fn const_int(
    ctx: &mut LowerCtx,
    value: i64,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    ctx.emit(
        "stmt.constant-int",
        Some(Type::Int),
        MirOperation::Constant(MirConstant::Int {
            value,
            width: None,
            spelling: None,
        }),
    )
}
