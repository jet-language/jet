//! Canonical projection of checked TIR expressions and patterns into MIR.
//!
//! This module is deliberately a projection only. It consumes facts already
//! present in checked TIR; it never re-derives meaning from AST.
use jet_foundation::MIR::{
    stable_id, MirAccess, MirAllocatorKind, MirBinaryDispatch, MirBinaryPatternPart, MirBlockId,
    MirCallbackAdapter, MirCallbackId, MirCallee, MirCallArg, MirConstKey, MirConstReport,
    MirConstant, MirCoreCall, MirCoreClosureKind, MirDataPlan, MirDataPlanCallable,
    MirDataPlanColumn, MirDataPlanNode, MirDataPlanNodeId, MirDataPlanPhysicalNode, MirConversion,
    MirEnumArg, MirForeignAbi, MirHardwareOp, MirPlace, MirPlaceBase, MirPlaceId,
    MirGcEditSiteId, MirIndexKind, MirLayoutCompareOp, MirOperation, MirOwnership, MirParam,
    MirPattern, MirPatternBinding, MirPatternField, MirPatternPosition, MirPatternShape,
    MirPanicContext, MirPanicLoc, MirPreludeAbi, MirPreludeFamily, MirRequireKind, MirSemanticOp,
    MirPreludeTypeArg,
    MirSelectKind, MirStringPart, MirStructExtra, MirSymbol, MirTaskGroupKind, MirTerminator,
    MirTextHoleKind, MirTextPatternPart, MirType, MirCallSignature, MirValueId,
};

use std::collections::BTreeMap;
use super::{
    ListSpreadPart, TCallArg, TExpr, TExprKind, TFailureCarrier, TFnValueKind,
    THostCall, THandleOp, TMethodRef, TOptionProbe, TPattern, TPatternBinding, TPatternField,
    TPatternPosition, TPatternShape, TStrPart, TTryConvert, TPlace, TLocal, TNumericOp,
    TEnumArg, TEnumPayload, TTextPatternPart, TBinaryPatternPart, TBuiltinOp, TCoreClosureKind, TLambda,
    TLambdaBody, TEffectFacts, TGcEditKind, TPreludeRoute, TCaptureFacts,
};
use jet_foundation::CanonicalPass;

use crate::AST::{BinOp, Type};
use super::mir::{LowerCtx, LowerError};

fn zip_callback(
    ctx: &mut LowerCtx,
    params: Vec<Type>,
    body: TExpr,
) -> Result<MirValueId, LowerError> {
    let lambda = TLambda {
        source_params: (0..params.len()).map(|index| format!("zip_{index}")).collect(),
        param_types: params,
        ret: Some(body.ty.clone()),
        failure_carrier: TFailureCarrier::Infallible,
        executable: TLambdaBody::Expr(Box::new(body)),
        source_span: ctx.span(),
        frame_schedule: None,
        frame_schedule_derivation: None,
        capture_facts: TCaptureFacts::default(),
        effects: TEffectFacts::default(),
        jit_name: String::new(),
        is_move: true,
        boxed: false,
        rc: false,
        arc: false,
        captures: Vec::new(),
        materialized_captures: Vec::new(),
        frozen_captures: Vec::new(),
        uses_stack_sentry: false,
    };
    ctx.lower_synthetic_lambda(&lambda, "zip")
}

fn zip_closure_call(
    ctx: &mut LowerCtx,
    result: Type,
    route: TPreludeRoute,
    receiver: MirValueId,
    values: Vec<(MirValueId, MirAccess)>,
) -> Result<MirValueId, LowerError> {
    let call = ctx.intern_prelude_route(route)?;
    let args = values.into_iter()
        .map(|(value, access)| mir_value_arg_with_access(ctx, value, access))
        .collect();
    ctx.emit(
        "zip-closure",
        Some(result),
        MirOperation::Semantic(MirSemanticOp::ClosureMethod { call, receiver, args }),
    )
}

fn lower_unzip(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
) -> Result<MirValueId, LowerError> {
    let Type::Tuple(fields) = expr.ty.without_user_tags() else {
        return Err(ctx.error(ctx.span(), "checked unzip result is not a tuple"));
    };
    let item_ty = match recv.ty.without_user_tags() {
        Type::List(element) | Type::FixedList { elem: element, .. } => (**element).clone(),
        Type::Apply { args, .. }
            if crate::Collections::is_iter_type(&recv.ty) && args.len() == 1 =>
        {
            args[0].clone()
        }
        _ => return Err(ctx.error(ctx.span(), "checked unzip receiver is not a sequence")),
    };
    let source = if crate::Collections::is_iter_type(&recv.ty) {
        if let TExprKind::Local(local) = &recv.kind {
            let place = lower_local_place(ctx, local, MirAccess::Move)?;
            ctx.emit("unzip-source", Some(recv.ty.clone()), MirOperation::MovePlace { place })?
        } else {
            ctx.lower_child(recv)?
        }
    } else {
        ctx.lower_child(recv)?
    };
    let mut columns = Vec::with_capacity(fields.len());
    let unit_ty = Type::Named("Unit".to_string());
    for (name, column_ty) in fields {
        let Type::List(element) = column_ty.without_user_tags() else {
            return Err(ctx.error(ctx.span(), "checked unzip column is not a List"));
        };
        let local = TLocal::generated(format!("unzip_{}_{}", source.0, name)).as_mutable();
        let place = ctx.bind_local(&local, (**column_ty).clone(), true, false, false)?;
        let empty = ctx.emit(
            "unzip-empty",
            Some((**column_ty).clone()),
            MirOperation::BuildList { values: Vec::new() },
        )?;
        ctx.emit("unzip-column", None, MirOperation::WritePlace { place, value: empty })?;
        let input_field = ctx.field_id_for_type(&item_ty, name)?;
        let output_field = ctx.field_id_for_type(&expr.ty, name)?;
        let push = ctx.intern_prelude_route(TBuiltinOp::Push.prelude_route(
            column_ty,
            &unit_ty,
            &TFailureCarrier::Infallible,
        )?)?;
        columns.push((local, place, (**column_ty).clone(), (**element).clone(), input_field, output_field, push));
    }
    let routes = super::loop_route_bundle();
    let init = ctx.intern_prelude_route(routes.iter_init)?;
    let has_next = ctx.intern_prelude_route(routes.iter_has_next)?;
    let value = ctx.intern_prelude_route(routes.iter_value)?;
    let advance = ctx.intern_prelude_route(routes.iter_advance)?;
    let cursor = ctx.emit(
        "unzip-cursor",
        Some(Type::Named("IterCursor".to_string())),
        MirOperation::LoopIterInit {
            call: init,
            collection: source,
            step: None,
            by_value: true,
            source_kind: jet_foundation::MIR::MirLoopSourceKind::Plain,
        },
    )?;
    let header = ctx.new_block(ctx.span(), "unzip.header")?;
    let body = ctx.new_block(ctx.span(), "unzip.body")?;
    let exit = ctx.new_block(ctx.span(), "unzip.exit")?;
    ctx.terminate(MirTerminator::Jump { target: header });
    ctx.switch_to(header);
    let condition = ctx.emit(
        "unzip-has-next",
        Some(Type::Bool),
        MirOperation::LoopIterHasNext { call: has_next, cursor },
    )?;
    ctx.terminate(MirTerminator::Branch {
        condition,
        then_target: body,
        else_target: exit,
    });
    ctx.switch_to(body);
    let item = ctx.emit(
        "unzip-row",
        Some(item_ty),
        MirOperation::LoopIterValue { call: value, cursor },
    )?;
    for (_, place, column_ty, element, input_field, _, push) in &columns {
        let value = ctx.emit(
            "unzip-field",
            Some(element.clone()),
            MirOperation::Field { base: item, field: *input_field },
        )?;
        let receiver = ctx.emit(
            "unzip-column-ref",
            Some(column_ty.clone()),
            MirOperation::AddressOf { place: *place, access: MirAccess::Write },
        )?;
        ctx.emit(
            "unzip-push",
            None,
            MirOperation::Semantic(MirSemanticOp::BuiltinMethod {
                call: *push,
                receiver,
                receiver_place: Some(*place),
                args: vec![value],
                aggregate_fields: None,
            }),
        )?;
    }
    ctx.emit(
        "unzip-advance",
        None,
        MirOperation::LoopIterAdvance { call: advance, cursor },
    )?;
    ctx.terminate(MirTerminator::Jump { target: header });
    ctx.switch_to(exit);
    let fields = columns
        .into_iter()
        .map(|(local, _, column_ty, _, _, field, _)| {
            let place = lower_local_place(ctx, &local, MirAccess::Move)?;
            let value = ctx.emit(
                "unzip-column-result",
                Some(column_ty),
                MirOperation::MovePlace { place },
            )?;
            Ok((field, value))
        })
        .collect::<Result<Vec<_>, LowerError>>()?;
    let owner = ctx.mir_type(&expr.ty)?.identity.ok_or_else(|| {
        ctx.error(ctx.span(), "checked unzip tuple has no canonical identity")
    })?;
    ctx.emit(
        "unzip-result",
        Some(expr.ty.clone()),
        MirOperation::Tuple { type_id: owner, fields },
    )
}

fn lower_zip(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
    op: &TBuiltinOp,
    args: &[TExpr],
) -> Result<MirValueId, LowerError> {
    let TBuiltinOp::Zip {
        mode, fields, flatten, input_count, fill_mode, field_types, ..
    } = op else {
        unreachable!("zip projection receives the checked zip operation")
    };
    let fill_count = usize::from(
        *mode == super::TZipMode::Pad && *fill_mode != super::TZipFillMode::DefaultNone,
    );
    if *flatten || *input_count < 2
        || fields.len() != *input_count || field_types.len() != *input_count
        || args.len() != input_count - 1 + fill_count
    {
        return Err(ctx.error(ctx.span(), "checked zip has inconsistent column facts"));
    }
    // Evaluate source expressions and the fill expression once, in source order.
    let mut inputs = Vec::with_capacity(*input_count);
    inputs.push((lower_builtin_receiver(ctx, recv, false)?.0, &recv.ty));
    for input in &args[..input_count - 1] {
        inputs.push((lower_builtin_receiver(ctx, input, false)?.0, &input.ty));
    }
    let fill = if fill_count == 1 {
        let fill = &args[input_count - 1];
        Some((ctx.lower_child(fill)?, &fill.ty))
    } else {
        None
    };
    let mut columns = Vec::with_capacity(*input_count);
    let mut fills = Vec::with_capacity(*input_count);
    for (index, (value, input_ty)) in inputs.into_iter().enumerate() {
        let element = match input_ty.without_user_tags() {
            Type::List(element) | Type::FixedList { elem: element, .. } => (**element).clone(),
            Type::Apply { args, .. }
                if crate::Collections::is_iter_type(input_ty) && args.len() == 1 =>
            {
                args[0].clone()
            }
            _ => return Err(ctx.error(ctx.span(), "checked zip input has no sequence element")),
        };
        let iter_ty = crate::Collections::iter_ty(element.clone());
        let mut value = if crate::Collections::is_iter_type(input_ty) {
            value
        } else {
            let route = TBuiltinOp::ListLazy.prelude_route(
                input_ty, &iter_ty, &TFailureCarrier::Infallible,
            )?;
            let call = ctx.intern_prelude_route(route)?;
            ctx.emit(
                "zip-source",
                Some(iter_ty.clone()),
                MirOperation::Semantic(MirSemanticOp::BuiltinMethod {
                    call, receiver: value, receiver_place: None, args: Vec::new(),
                    aggregate_fields: None,
                }),
            )?
        };
        if *mode == super::TZipMode::Pad {
            let target = &field_types[index];
            let fill_value = if let Some((fill, fill_ty)) = fill {
                let (fill, actual) = if *fill_mode == super::TZipFillMode::Columns {
                    let Type::Tuple(shape) = fill_ty else {
                        return Err(ctx.error(ctx.span(), "checked zip column fills have no tuple"));
                    };
                    let actual = shape.iter().find(|(name, _)| name == &fields[index])
                        .map(|(_, ty)| ty.as_ref())
                        .ok_or_else(|| ctx.error(ctx.span(), "checked zip fill column is missing"))?;
                    let field = ctx.field_id_for_type(fill_ty, &fields[index])?;
                    let fill = ctx.emit(
                        "zip-fill-column", Some(actual.clone()),
                        MirOperation::Field { base: fill, field },
                    )?;
                    (fill, actual)
                } else {
                    (fill, fill_ty)
                };
                if actual == target {
                    fill
                } else {
                    let mir_target = ctx.mir_type(target)?;
                    ctx.emit(
                        "zip-fill-widen", Some(target.clone()),
                        MirOperation::Convert {
                            value: fill, parameters: Vec::new(), target: mir_target,
                            conversion: MirConversion::NumericCast,
                        },
                    )?
                }
            } else {
                let local = TExpr {
                    ty: element.clone(),
                    kind: TExprKind::Local(TLocal::user("zip_0")),
                };
                let callback = zip_callback(ctx, vec![element], TExpr {
                    ty: target.clone(),
                    kind: TExprKind::Present(Box::new(local)),
                })?;
                let result = crate::Collections::iter_ty(target.clone());
                let route = super::TClosureOp::Map.prelude_route(
                    &iter_ty, &result, &TFailureCarrier::Infallible,
                )?;
                value = zip_closure_call(ctx, result, route, value, vec![(callback, MirAccess::Move)])?;
                ctx.emit("zip-fill-absent", Some(target.clone()), MirOperation::Absent)?
            };
            fills.push(fill_value);
        }
        columns.push(value);
    }
    let mut left = columns[0];
    let mut left_ty = field_types[0].clone();
    let mut left_fill = fills.first().copied();
    for column in 1..*input_count {
        let shape: Vec<_> = fields[..=column].iter().cloned()
            .zip(field_types[..=column].iter().cloned()).collect();
        let row_ty = Type::Tuple(shape.iter()
            .map(|(name, ty)| (name.clone(), Box::new(ty.clone()))).collect());
        let left_local = TExpr {
            ty: left_ty.clone(), kind: TExprKind::Local(TLocal::user("zip_0")),
        };
        let mut row_fields = Vec::with_capacity(column + 1);
        for index in 0..column {
            let value = if column == 1 {
                left_local.clone()
            } else {
                TExpr {
                    ty: field_types[index].clone(),
                    kind: TExprKind::Field {
                        recv: Box::new(left_local.clone()), field: fields[index].clone(), boxed: false,
                    },
                }
            };
            row_fields.push((fields[index].clone(), value));
        }
        row_fields.push((fields[column].clone(), TExpr {
            ty: field_types[column].clone(),
            kind: TExprKind::Local(TLocal::user("zip_1")),
        }));
        let callback = zip_callback(
            ctx, vec![left_ty, field_types[column].clone()],
            TExpr {
                ty: row_ty.clone(),
                kind: TExprKind::TupleLit {
                    struct_name: crate::Codegen::Tuples::tuple_struct_name(&shape),
                    fields: row_fields,
                },
            },
        )?;
        let mut values = vec![(columns[column], MirAccess::Move)];
        if let Some(fill) = left_fill {
            values.extend([(fill, MirAccess::Read), (fills[column], MirAccess::Read)]);
        }
        values.push((callback, MirAccess::Move));
        let result = if column + 1 == *input_count {
            expr.ty.clone()
        } else {
            crate::Collections::iter_ty(row_ty.clone())
        };
        left = zip_closure_call(ctx, result, super::zip_closure_route(*mode), left, values)?;
        if left_fill.is_some() && column + 1 < *input_count {
            let owner = ctx.mir_type(&row_ty)?.identity
                .ok_or_else(|| ctx.error(ctx.span(), "zip fill tuple has no type identity"))?;
            let tuple_fields = fields[..=column].iter().zip(&fills)
                .map(|(name, value)| Ok((ctx.field_id_for_type(&row_ty, name)?, *value)))
                .collect::<Result<Vec<_>, LowerError>>()?;
            left_fill = Some(ctx.emit(
                "zip-fill-prefix", Some(row_ty.clone()),
                MirOperation::Tuple { type_id: owner, fields: tuple_fields },
            )?);
        }
        left_ty = row_ty;
    }
    Ok(left)
}

fn csv_query_row_type(ty: &Type) -> Option<Type> {
    let Type::Result { ok, .. } = ty.without_user_tags() else {
        return None;
    };
    let Type::Apply { name, args } = ok.without_user_tags() else {
        return None;
    };
    (name == "Query" && args.len() == 1).then(|| args[0].clone())
}
/// Recover the single checked `T` carried by typed DataLoader/DataSnapshot/
/// DataStream calls.  The call result retains the full fallible/optional/list
/// shape, so peel only those wrappers and never infer a second wire type.
fn data_row_type(ty: &Type) -> Option<Type> {
    match ty.without_user_tags() {
        Type::Result { ok, .. } | Type::Option(ok) => data_row_type(ok),
        Type::Apply { name, args }
            if matches!(name.as_str(), "DataLoader" | "DataSnapshot" | "DataStream")
                && args.len() == 1 =>
        {
            Some(args[0].clone())
        }
        Type::List(inner) | Type::FixedList { elem: inner, .. } => Some((**inner).clone()),
        _ => None,
    }
}



fn lower_data_plan_node_id(id: crate::AST::DataPlanNodeId) -> MirDataPlanNodeId {
    MirDataPlanNodeId(id.index() as u64)
}

fn lower_data_plan(
    ctx: &mut LowerCtx<'_>,
    plan: &super::TDataPlan,
) -> Result<MirDataPlan, LowerError> {
    let logical = plan
        .logical
        .iter()
        .map(|node| {
            let callable = match &node.callable {
                Some(callable) => Some(MirDataPlanCallable {
                    label: callable.label.clone(),
                    parameter: ctx.mir_type(&callable.parameter)?,
                    result: ctx.mir_type(&callable.result)?,
                    span: callable.span,
                }),
                None => None,
            };
            Ok(MirDataPlanNode {
                id: lower_data_plan_node_id(node.id),
                operation: node.operation,
                inputs: node
                    .inputs
                    .iter()
                    .copied()
                    .map(lower_data_plan_node_id)
                    .collect(),
                source: node.source,
                row_type: ctx.mir_type(&node.schema.row_type)?,
                columns: node
                    .schema
                    .columns
                    .iter()
                    .map(|column| {
                        Ok(MirDataPlanColumn {
                            name: column.name.clone(),
                            ty: ctx.mir_type(&column.ty)?,
                            nullable: column.nullable,
                        })
                    })
                    .collect::<Result<Vec<_>, LowerError>>()?,
                callable,
                stream: node.stream.clone(),
                span: node.span,
            })
        })
        .collect::<Result<Vec<_>, LowerError>>()?;
    let physical = plan
        .physical
        .iter()
        .map(|node| MirDataPlanPhysicalNode {
            logical: lower_data_plan_node_id(node.logical),
            operator: node.operator,
            stream: node.stream.clone(),
            reason: node.reason.clone(),
        })
        .collect();
    Ok(MirDataPlan {
        schema_version: MirDataPlan::SCHEMA_VERSION,
        source: plan.source,
        logical,
        physical,
        output: lower_data_plan_node_id(plan.output),
        source_span: plan.source_span,
    })
}
fn mir_const_key(key: &crate::AST::CtKey) -> MirConstKey {
    match key {
        crate::AST::CtKey::Int(value) => MirConstKey::Int(*value),
        crate::AST::CtKey::Str(value) => MirConstKey::String(value.clone()),
        crate::AST::CtKey::Bool(value) => MirConstKey::Bool(*value),
        crate::AST::CtKey::Char(value) => MirConstKey::Char(*value),
        crate::AST::CtKey::Tuple(fields) => MirConstKey::Tuple(
            fields
                .iter()
                .map(|(name, key)| (name.clone(), mir_const_key(key)))
                .collect(),
        ),
        crate::AST::CtKey::Struct { type_name, fields } => MirConstKey::Struct {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(name, key)| (name.clone(), mir_const_key(key)))
                .collect(),
        },
        crate::AST::CtKey::Enum {
            type_name,
            variant,
        } => MirConstKey::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
        },
    }
}

fn mir_constant(
    ctx: &mut LowerCtx,
    value: &crate::AST::CtValue,
) -> Result<MirConstant, LowerError> {
    Ok(match value {
        crate::AST::CtValue::Int(value) => MirConstant::Int {
            value: *value,
            width: None,
            spelling: None,
        },
        crate::AST::CtValue::Float(value) => MirConstant::Float {
            value: value.as_f64(),
            f32: matches!(value, crate::AST::CtFloat::F32(_)),
            spelling: None,
        },
        crate::AST::CtValue::Bool(value) => MirConstant::Bool(*value),
        crate::AST::CtValue::Char(value) => MirConstant::Char(*value),
        crate::AST::CtValue::Str(value) => MirConstant::String(value.clone()),
        crate::AST::CtValue::BigInt(value) => MirConstant::BigInt(value.to_string_rep()),
        crate::AST::CtValue::Bytes(value) => MirConstant::Bytes(value.clone()),
        crate::AST::CtValue::List(values) => MirConstant::List(
            values
                .iter()
                .map(|value| mir_constant(ctx, value))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        crate::AST::CtValue::Map(entries) => MirConstant::Map(
            entries
                .iter()
                .map(|(key, value)| Ok((mir_const_key(key), mir_constant(ctx, value)?)))
                .collect::<Result<_, LowerError>>()?,
        ),
        crate::AST::CtValue::Struct { type_name, fields } => MirConstant::Struct {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(name, value)| Ok((name.clone(), mir_constant(ctx, value)?)))
                .collect::<Result<_, LowerError>>()?,
        },
        crate::AST::CtValue::Enum {
            type_name,
            variant,
            args,
        } => MirConstant::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: args
                .iter()
                .map(|(name, value)| Ok((name.clone(), mir_constant(ctx, value)?)))
                .collect::<Result<_, LowerError>>()?,
        },
        crate::AST::CtValue::Present(value) => {
            MirConstant::Present(Box::new(mir_constant(ctx, value)?))
        }
        crate::AST::CtValue::Failed(report) => MirConstant::Failed(match report {
            crate::AST::CtReport::Clean(ty) => MirConstReport::Clean(ctx.mir_type(ty)?),
            crate::AST::CtReport::Told(value) => {
                MirConstReport::Told(Box::new(mir_constant(ctx, value)?))
            }
        }),
        crate::AST::CtValue::Unit => MirConstant::Unit,
        crate::AST::CtValue::Closure(_) => {
            return Err(ctx.error(
                ctx.span(),
                "checked comptime closure cannot lower as a neutral MIR constant",
            ))
        }
    })
}

fn lower_persistent_place(
    ctx: &mut LowerCtx,
    local: &TLocal,
    access: MirAccess,
) -> Result<MirPlaceId, LowerError> {
    let Some(key) = local.persist_key.as_ref() else {
        return ctx.place_for_local(local, access);
    };
    if let Some(place) = ctx
        .places
        .iter_mut()
        .find(|place| place.persist_key.as_ref() == Some(key))
    {
        place.access = match (place.access, access) {
            (MirAccess::Move, _) | (_, MirAccess::Move) => MirAccess::Move,
            (MirAccess::Write, _) | (_, MirAccess::Write) => MirAccess::Write,
            _ => MirAccess::Read,
        };
        return Ok(place.id);
    }
    let ty = local.persist_ty.clone().ok_or_else(|| {
        ctx.error(
            ctx.span(),
            format!("persistent local `{}` has no checked type", local.name),
        )
    })?;
    let span = ctx.span();
    let id = MirPlaceId(stable_id(
        "mir-place",
        &format!("{}|persistent|{key}", ctx.function.key),
    ));
    let mir_ty = ctx.mir_type(&ty)?;
    ctx.places.push(MirPlace {
        id,
        span,
        ty: mir_ty,
        base: MirPlaceBase::Static(key.clone()),
        projections: Vec::new(),
        access,
        persist_key: Some(key.clone()),
    });
    Ok(id)
}

fn lower_local_place(
    ctx: &mut LowerCtx,
    local: &TLocal,
    access: MirAccess,
) -> Result<MirPlaceId, LowerError> {
    if local.is_persistent() {
        lower_persistent_place(ctx, local, access)
    } else {
        let mangled_name = crate::Codegen::mangle(&local.name);
        if !local.generated
            && mangled_name != local.name
            && ctx.local_places.contains_key(&mangled_name)
        {
            return ctx.place_for_local(&TLocal::user(mangled_name), access);
        }
        ctx.place_for_local(local, access)
    }
}
fn lower_local_value(
    ctx: &mut LowerCtx,
    local: &TLocal,
    ty: &Type,
) -> Result<MirValueId, LowerError> {
    let place = lower_local_place(ctx, local, MirAccess::Read)?;
    let value = ctx.emit("local.read", Some(ty.clone()), MirOperation::ReadPlace(place))?;
    ctx.local_values.insert(local.name.clone(), value);
    Ok(value)
}

pub(super) fn lower_receiver_place(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    access: MirAccess,
) -> Result<Option<jet_foundation::MIR::MirPlaceId>, LowerError> {
    let place = match &expr.kind {
        TExprKind::Local(local) => Some(lower_local_place(ctx, local, access)?),
        TExprKind::Index {
            base,
            index,
            is_map,
            uninit_fixed,
            ..
        } => {
            let kind = if *is_map {
                MirIndexKind::Map
            } else if *uninit_fixed {
                MirIndexKind::FixedListProof
            } else {
                MirIndexKind::List
            };
            Some(ctx.lower_index_place(base, index, kind, access)?)
        }
        TExprKind::Field { recv, field, .. } => {
            let Some(base) = lower_receiver_place(ctx, recv, access)? else {
                return Ok(None);
            };
            Some(ctx.project_field_place(
                base,
                field,
                expr.ty.clone(),
                ctx.span(),
            )?)
        }
        TExprKind::PoolSlot {
            pool,
            id,
            field,
            ..
        } => {
            let base = ctx.lower_index_place(pool, id, MirIndexKind::Pool, access)?;
            match field {
                Some(field) => Some(ctx.project_field_place(
                    base,
                    field,
                    expr.ty.clone(),
                    ctx.span(),
                )?),
                None => Some(base),
            }
        }
        _ => None,
    };
    Ok(place)
}

fn lower_builtin_receiver(
    ctx: &mut LowerCtx,
    recv: &TExpr,
    mutating: bool,
) -> Result<
    (
        jet_foundation::MIR::MirValueId,
        Option<jet_foundation::MIR::MirPlaceId>,
    ),
    LowerError,
> {
    if !mutating {
        if crate::Collections::is_iter_type(&recv.ty) {
            if let Some(place) = lower_receiver_place(ctx, recv, MirAccess::Move)? {
                let receiver = ctx.emit(
                    "builtin-owned-receiver",
                    Some(recv.ty.clone()),
                    MirOperation::MovePlace { place },
                )?;
                return Ok((receiver, None));
            }
        }
        return Ok((ctx.lower_child(recv)?, None));
    }
    let Some(place) = lower_receiver_place(ctx, recv, MirAccess::Write)? else {
        return Ok((ctx.lower_child(recv)?, None));
    };
    let receiver = ctx.emit(
        "builtin-mutating-receiver",
        Some(recv.ty.clone()),
        MirOperation::AddressOf {
            place,
            access: MirAccess::Write,
        },
    )?;
    Ok((receiver, Some(place)))
}
fn untagged_type(ty: &Type) -> &Type {
    match ty {
        Type::Tagged { inner, .. } => untagged_type(inner),
        _ => ty,
    }
}

fn lower_list_min_max(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
    op: &TBuiltinOp,
    args: &[TExpr],
    carrier: &TFailureCarrier,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    if !args.is_empty() {
        return Err(ctx.error(
            ctx.span(),
            "checked List.min_max operation received unexpected arguments",
        ));
    }
    let Type::Option(inner) = untagged_type(&expr.ty) else {
        return Err(ctx.error(
            ctx.span(),
            "checked List.min_max result is not optional",
        ));
    };
    let tuple_ty = untagged_type(inner);
    let Type::Tuple(fields) = tuple_ty else {
        return Err(ctx.error(
            ctx.span(),
            "checked List.min_max result has no named tuple fields",
        ));
    };
    if fields.len() != 2
        || !fields.iter().any(|(name, _)| name == "min")
        || !fields.iter().any(|(name, _)| name == "max")
    {
        return Err(ctx.error(
            ctx.span(),
            "checked List.min_max result must contain exactly min and max fields",
        ));
    }
    let aggregate_fields = ["min", "max"]
        .into_iter()
        .map(|name| ctx.field_id_for_type(tuple_ty, name))
        .collect::<Result<Vec<_>, LowerError>>()?;
    let (receiver, receiver_place) = lower_builtin_receiver(ctx, recv, false)?;
    let route = op.prelude_route(&recv.ty, &expr.ty, carrier).map_err(|error| {
        ctx.error(ctx.span(), format!("builtin method {op:?}: {error:?}"))
    })?;
    let call = ctx.intern_prelude_route(route)?;
    ctx.emit(
        "list-min-max",
        Some(expr.ty.clone()),
        MirOperation::Semantic(MirSemanticOp::BuiltinMethod {
            call,
            receiver,
            receiver_place,
            args: Vec::new(),
            aggregate_fields: Some(aggregate_fields),
        }),
    )
}


fn lower_consuming_task_value(
    ctx: &mut LowerCtx,
    value: &TExpr,
) -> Result<MirValueId, LowerError> {
    if let Some(place) = lower_receiver_place(ctx, value, MirAccess::Move)? {
        return ctx.emit(
            "task-consuming-value",
            Some(value.ty.clone()),
            MirOperation::MovePlace { place },
        );
    }
    ctx.lower_child(value)
}

fn math_builtin_type_id(
    ctx: &mut LowerCtx,
    type_name: &str,
) -> Result<jet_foundation::MIR::MirTypeId, LowerError> {
    let source = match type_name {
        "Int" => Type::Int,
        "Float" => Type::Float,
        "F32" | "Float32" => Type::Float32,
        _ => Type::Named(type_name.to_string()),
    };
    let identity = ctx.mir_type(&source)?.identity;
    identity.ok_or_else(|| {
        ctx.error(
            ctx.span(),
            format!("checked MIR math builtin type `{type_name}` has no identity"),
        )
    })
}

/// Lower one checked TIR expression into one canonical MIR value.
///
/// Every `TExprKind` is named explicitly.  The match is intentionally not a
/// wildcard: adding a TIR expression requires an explicit canonical-MIR
/// decision (or an explicit boundary error) here.
pub(super) fn lower_expr(
    ctx: &mut LowerCtx,
    expr: &TExpr,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
    let value = match &expr.kind {
        TExprKind::IntLit(value, width) => ctx.emit("int-literal", 
            Some(expr.ty.clone()),
            MirOperation::Constant(MirConstant::Int {
                value: *value,
                width: *width,
                spelling: None,
            }),
        ),
        TExprKind::FloatLit(value) => ctx.emit("float-literal", 
            Some(expr.ty.clone()),
            MirOperation::Constant(MirConstant::Float {
                value: *value,
                f32: matches!(&expr.ty, crate::AST::Type::Float32),
                spelling: None,
            }),
        ),
        TExprKind::BoolLit(value) => ctx.emit("bool-literal", 
            Some(expr.ty.clone()),
            MirOperation::Constant(MirConstant::Bool(*value)),
        ),
        TExprKind::CharLit(value) => ctx.emit("char-literal", 
            Some(expr.ty.clone()),
            MirOperation::Constant(MirConstant::Char(*value)),
        ),
        TExprKind::StrLit(parts) => {
            let mut lowered = Vec::with_capacity(parts.len());
            for part in parts {
                match part {
                    TStrPart::Lit(text) => lowered.push(MirStringPart::Literal(text.clone())),
                    TStrPart::Interp(value, format) => {
                        let value_id = ctx.lower_child(value)?;
                        let formatted = match format {
                            crate::AST::StrFormat::Display
                            | crate::AST::StrFormat::Debug
                            | crate::AST::StrFormat::Unit(_) => {
                                lower_direct_string_format(ctx, value_id, &value.ty, format, &expr.ty)?
                            }
                            _ => {
                                let call = ctx.intern_prelude_route(super::string_format_route(
                                    format,
                                    &value.ty,
                                    &expr.ty,
                                    &carrier,
                                )?)?;
                                let args = lower_string_format_args(ctx, value_id, format)?;
                                ctx.emit(
                                    "string-format",
                                    Some(crate::AST::Type::String),
                                    MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
                                        call,
                                        args,
                                        owner_type_args: Vec::new(),
                                        type_args: Vec::new(),
                                    }),
                                )?
                            }
                        };
                        lowered.push(MirStringPart::Value(formatted));
                    }
                }
            }
            ctx.emit("string-literal", Some(expr.ty.clone()), MirOperation::BuildString { parts: lowered })
        }
        TExprKind::Local(local) => lower_local_value(ctx, local, &expr.ty),
        TExprKind::Unit => ctx.emit("unit-literal", 
            Some(expr.ty.clone()),
            MirOperation::Constant(MirConstant::Unit),
        ),
        TExprKind::InlineBlock(stmts) => lower_inline_block(ctx, stmts),
        TExprKind::DefaultLit => {
            let value = mir_constant(ctx, &default_constant(&expr.ty))?;
            ctx.emit("default-literal", Some(expr.ty.clone()), MirOperation::Constant(value))
        }
        TExprKind::Uninit => unsupported_expr(
            ctx,
            "TExprKind::Uninit has no canonical MIR value; checked TIR invariant violated",
        ),
        TExprKind::CtLit(value) => {
            let value = mir_constant(ctx, value)?;
            ctx.emit("constant-literal", Some(expr.ty.clone()), MirOperation::Constant(value))
        }
        TExprKind::HostCall(host) => lower_host_call(ctx, expr, host),
        TExprKind::ConstRef(name) => ctx.emit("const-reference", 
            Some(expr.ty.clone()),
            MirOperation::Global { name: name.clone() },
        ),
        TExprKind::DataEntriesToMap(local) => {
            let local = ctx.local_id_for(local)?;
            let call = ctx.intern_prelude_route(super::data_entries_to_map_route(
                &expr.ty,
                &carrier,
            )?)?;
            ctx.emit("data-entries-to-map", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::DataEntriesToMap { call, local }),
            )
        }
        TExprKind::Call {
            name,
            type_args,
            args,
        } => {
            let args = lower_call_args(ctx, args)?;
            let function = ctx.function_id_for(name)?;
            let type_args = lower_mir_types(ctx, type_args)?;
            ctx.emit("call", 
                Some(expr.ty.clone()),
                MirOperation::Call {
                    callee: MirCallee::User(function),
                    args,
                    type_args,
                },
            )
        }
        TExprKind::DistinctCtor { name, arg, .. } => {
            let value = ctx.lower_child(arg)?;
            let target = ctx.mir_type(&crate::AST::Type::Named(name.clone()))?;
            ctx.emit(
                "distinct-constructor",
                Some(expr.ty.clone()),
                MirOperation::Convert {
                    value,
                    parameters: Vec::new(),
                    target,
                    conversion: MirConversion::Transparent,
                },
            )
        }
        TExprKind::RangeCheckedCtor { name, arg } => {
            let value = ctx.lower_child(arg)?;
            let Some((lo, hi)) = ctx.distinct_range(name) else {
                return unsupported_expr(
                    ctx,
                    format!("range-checked constructor `{name}` has no checked bounds"),
                );
            };
            let parameters = vec![lower_conversion_int(ctx, lo)?, lower_conversion_int(ctx, hi)?];
            let route = super::range_checked_ctor_route(name, &expr.ty, &carrier)?;
            lower_prelude_conversion(
                ctx,
                value,
                parameters,
                &expr.ty,
                route,
                &carrier,
            )
        }
        TExprKind::DistinctConvert {
            name,
            arg,
            op,
            range,
            fallible,
        } => {
            let value = ctx.lower_child(arg)?;
            if matches!(op, TNumericOp::CastAs { .. }) {
                let base = ctx.distinct_base(name).ok_or_else(|| {
                    ctx.error(ctx.span(), format!("distinct conversion `{name}` has no checked base"))
                })?;
                let value = if arg.ty == base {
                    value
                } else {
                    let target = ctx.mir_type(&base)?;
                    ctx.emit(
                        "distinct-base-cast",
                        Some(base),
                        MirOperation::Convert {
                            value,
                            parameters: Vec::new(),
                            target,
                            conversion: MirConversion::NumericCast,
                        },
                    )?
                };
                if let Some((lo, hi)) = range.filter(|_| *fallible) {
                    let parameters = vec![
                        lower_conversion_int(ctx, lo)?,
                        lower_conversion_int(ctx, hi)?,
                    ];
                    let route = super::range_checked_ctor_route(name, &expr.ty, &carrier)?;
                    return lower_prelude_conversion(
                        ctx, value, parameters, &expr.ty, route, &carrier,
                    );
                }
                let nominal = Type::Named(name.clone());
                let target = ctx.mir_type(&nominal)?;
                let value = ctx.emit(
                    "distinct-conversion",
                    Some(nominal),
                    MirOperation::Convert {
                        value,
                        parameters: Vec::new(),
                        target,
                        conversion: MirConversion::Transparent,
                    },
                )?;
                return if *fallible {
                    ctx.emit("distinct-conversion-ok", Some(expr.ty.clone()), MirOperation::ResultOk { value })
                } else {
                    Ok(value)
                };
            }
            let route = super::distinct_conversion_route(
                name,
                &arg.ty,
                op,
                *range,
                &expr.ty,
                &carrier,
            )?;
            let parameters = match op {
                TNumericOp::CheckedIntToFloat {
                    target_f32,
                    ..
                } if matches!(arg.ty.without_user_tags(), Type::Int) => {
                    vec![lower_conversion_bool(ctx, *target_f32)?]
                }
                TNumericOp::CheckedIntToFloat {
                    source_signed,
                    target_f32,
                    ..
                } => vec![
                    lower_conversion_bool(ctx, *source_signed)?,
                    lower_conversion_bool(ctx, *target_f32)?,
                ],
                TNumericOp::CheckedIntToFixed { host_kind, .. }
                | TNumericOp::FloatToInt { host_kind, .. } => {
                    vec![lower_conversion_int(ctx, *host_kind)?]
                }
                TNumericOp::TryFrom { host_kind, .. }
                    if matches!(arg.ty.without_user_tags(), Type::Int) =>
                {
                    vec![lower_conversion_int(ctx, *host_kind)?]
                }
                TNumericOp::TryFrom { host_kind, .. } => {
                    let source_signed =
                        matches!(arg.ty.without_user_tags(), Type::IntN { signed: true, .. });
                    vec![
                        lower_conversion_bool(ctx, source_signed)?,
                        lower_conversion_int(ctx, *host_kind)?,
                    ]
                }
                TNumericOp::FloatNarrow { .. } => Vec::new(),
                TNumericOp::InlineRange { lo, hi, .. } => vec![
                    lower_conversion_int(ctx, *lo)?,
                    lower_conversion_int(ctx, *hi)?,
                ],
                _ => Vec::new(),
            };
            lower_prelude_conversion(
                ctx,
                value,
                parameters,
                &expr.ty,
                route,
                &carrier,
            )
        }
        TExprKind::UnitConvert {
            destination,
            arg,
            scale,
            offset,
            rounding,
            relative_uncertainty,
            ..
        } => {
            let value = ctx.lower_child(arg)?;
            let rounding_mode = rounding.as_ref().map(|(mode, _)| *mode);
            let route = super::unit_conversion_route(
                destination,
                scale,
                offset,
                rounding_mode,
                *relative_uncertainty,
                &expr.ty,
                &carrier,
            )?;
            let mut parameters = vec![
                lower_conversion_string(ctx, scale.num.to_string())?,
                lower_conversion_string(ctx, scale.den.to_string())?,
                lower_conversion_string(ctx, offset.num.to_string())?,
                lower_conversion_string(ctx, offset.den.to_string())?,
            ];
            if let Some((mode, digits)) = rounding {
                parameters.push(lower_conversion_int(ctx, *mode as i64)?);
                parameters.push(ctx.lower_child(digits)?);
            }
            if let Some(relative_uncertainty) = *relative_uncertainty {
                parameters.push(lower_conversion_float(ctx, relative_uncertainty)?);
            }
            lower_prelude_conversion(
                ctx,
                value,
                parameters,
                &expr.ty,
                route,
                &carrier,
            )
        }
        TExprKind::MathBuiltin {
            type_name, func, args,
        } => {
            let args = lower_values(ctx, args)?;
            let type_id = math_builtin_type_id(ctx, type_name)?;
            let route = if crate::Sema::is_geometry_type(type_name) {
                super::geometry_builtin_route(
                    type_name,
                    func,
                    args.len(),
                    &expr.ty,
                    &carrier,
                )?
            } else {
                super::math_builtin_route(
                    type_name,
                    func,
                    args.len(),
                    &expr.ty,
                    &carrier,
                )?
            };
            let call = ctx.intern_prelude_route(route)?;
            ctx.emit("math-builtin", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::MathBuiltin {
                    type_id,
                    call,
                    args,
                }),
            )
        }
        TExprKind::PreciseBuiltin {
            type_name, func, args,
        } => {
            let args = lower_values(ctx, args)?;
            let type_id = ctx.nominal_type_id(type_name)?;
            let call = ctx.intern_prelude_route(super::precise_builtin_route(
                type_name,
                func,
                args.len(),
                &expr.ty,
                &carrier,
            )?)?;
            ctx.emit("precise-builtin", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::PreciseBuiltin {
                    type_id,
                    call,
                    args,
                }),
            )
        }
        TExprKind::Print(inner) => {
            // Keep the checked Printable operand typed through MIR. Explicit
            // Display implementations have already produced String values.
            // Each execution tier marshals the value through its JetShow
            // adapter before calling the terminal text row.
            let value = ctx.lower_child(inner)?;
            let call = ctx.intern_prelude_route(super::print_route(&expr.ty, &carrier)?)?;
            ctx.emit("print", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::Print { call, value }),
            )
        }
        TExprKind::Drop(inner) => {
            let value = ctx.lower_child(inner)?;
            let kind = ctx.ownership_for(&inner.ty).drop;
            ctx.emit("drop", 
                Some(expr.ty.clone()),
                MirOperation::Drop { value, kind },
            )
        }
        TExprKind::Close(inner) => {
            // Allocator close is the terminal ownership operation. Lower it to
            // the same MIR drop edge used by native/AOT values; the interpreter
            // adapter marks only this consumed owner closed.
            if matches!(
                inner.ty.name().as_str(),
                "Arena" | "Bump" | "Pool" | "Fixed"
            ) {
                let value = ctx.lower_child(inner)?;
                let kind = ctx.ownership_for(&inner.ty).drop;
                return ctx.emit(
                    "allocator-close",
                    Some(expr.ty.clone()),
                    MirOperation::Drop { value, kind },
                );
            }
            if inner.ty.name() == "DbLease" {
                let receiver = ctx.lower_child(inner)?;
                let call = ctx.intern_prelude_route(
                    THandleOp::DBLeaseClose.prelude_route(&inner.ty, &carrier)?,
                )?;
                return ctx.emit(
                    "db-lease-close",
                    Some(expr.ty.clone()),
                    MirOperation::Semantic(MirSemanticOp::HandleMethod {
                        frame_schedule: None,
                        frame_schedule_derivation: None,
                        call,
                        receiver,
                        args: Vec::new(),
                    }),
                );
            }
            let function_name = format!("{}::close", inner.ty.name());
            let function = ctx.function_id_for(&function_name)?;
            let mut arg = ctx.lower_plain_arg(inner)?;
            arg.access = MirAccess::Move;
            ctx.emit(
                "close",
                Some(expr.ty.clone()),
                MirOperation::Call {
                    callee: MirCallee::User(function),
                    args: vec![arg],
                    type_args: Vec::new(),
                },
            )
        }
        TExprKind::ResourceNew(inner) => {
            let value = ctx.lower_child(inner)?;
            ctx.emit("resource-new", Some(expr.ty.clone()), MirOperation::Move { value })
        }
        TExprKind::ResourceTake(name) => {
            let place = lower_local_place(
                ctx,
                &super::TLocal::user(name.clone()),
                MirAccess::Move,
            )?;
            ctx.emit(
                "resource-take",
                Some(expr.ty.clone()),
                MirOperation::MovePlace { place },
            )
        }
        TExprKind::AmbientInput { prompt } => {
            let prompt = prompt
                .as_deref()
                .map(|value| ctx.lower_child(value))
                .transpose()?;
            let call = ctx.intern_prelude_route(super::ambient_input_route(
                prompt.is_some(),
                &expr.ty,
                &carrier,
            )?)?;
            ctx.emit("ambient-input", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::AmbientInput { call, prompt }),
            )
        }
        TExprKind::RequireStop {
            kind,
            always_stops,
            loc,
        } => lower_require_stop(ctx, expr, kind, *always_stops, loc),
        TExprKind::Binary {
            op,
            overflow,
            line,
            lhs,
            rhs,
        } => {
            let left = ctx.lower_child(lhs)?;
            let right = ctx.lower_child(rhs)?;
            let dispatch = match super::binary_route(
                *op,
                *overflow,
                &lhs.ty,
                &rhs.ty,
                &expr.ty,
                &carrier,
            )? {
                super::TRoutePlan::Primitive => MirBinaryDispatch::Primitive,
                super::TRoutePlan::Prelude(route) => MirBinaryDispatch::Prelude {
                    call: ctx.intern_prelude_route(route)?,
                    location: Some(panic_location(ctx, *line as usize)),
                },
            };
            ctx.emit(
                "binary",
                Some(expr.ty.clone()),
                MirOperation::Binary {
                    op: super::mir_binary_op(*op),
                    left,
                    right,
                    dispatch,
                },
            )
        }
        TExprKind::CompareChain {
            operands,
            ops,
            hooks,
        } => lower_compare_chain(ctx, expr, operands, ops, hooks),
        TExprKind::LayoutCompare { op, lhs, rhs } => {
            let op = match op {
                BinOp::Eq => MirLayoutCompareOp::Equal,
                BinOp::Le => MirLayoutCompareOp::LessEqual,
                BinOp::Ge => MirLayoutCompareOp::GreaterEqual,
                _ => return unsupported_expr(ctx, "TExprKind::LayoutCompare (invalid op)"),
            };
            let left = ctx.lower_child(lhs)?;
            let right = ctx.lower_child(rhs)?;
            let call = ctx.intern_prelude_route(super::layout_compare_route(
                op,
                &expr.ty,
                &carrier,
            )?)?;
            ctx.emit("layout-compare", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::LayoutCompare {
                    call,
                    op,
                    left,
                    right,
                }),
            )
        }
        TExprKind::LayoutLit { inner } => {
            let value = ctx.lower_child(inner)?;
            ctx.emit("layout-literal", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::LayoutLiteral { inner: value }),
            )
        }
        TExprKind::Unary { op, operand } => {
            let value = ctx.lower_child(operand)?;
            ctx.emit("unary", 
                Some(expr.ty.clone()),
                MirOperation::Unary {
                    op: super::mir_unary_op(*op),
                    value,
                },
            )
        }
        TExprKind::IncDec {
            op,
            place,
            postfix,
            ..
        } => lower_inc_dec(ctx, expr, op, place, *postfix),
        TExprKind::StructLit {
            fields,
            extra,
            as_trait,
        } => {
            let owner = match as_trait {
                Some((_, concrete)) => ctx.type_id_for(concrete)?,
                None => ctx.field_owner_id_for_type(&expr.ty)?,
            };
            let lowered_fields = fields
                .iter()
                .map(|(name, value, _)| {
                    Ok((ctx.field_id_for(owner, name)?, ctx.lower_child(value)?))
                })
                .collect::<Result<Vec<_>, LowerError>>()?;
            let extra = extra.as_ref().map(|extra| match extra {
                super::TStructExtra::HTTPRequestParams => MirStructExtra::HttpRequestParams,
            });
            let trait_coercion = as_trait
                .as_ref()
                .map(|(trait_name, _)| ctx.type_id_for(trait_name))
                .transpose()?;
            let boxed_fields = fields
                .iter()
                .filter(|(_, _, boxed)| *boxed)
                .map(|(name, _, _)| ctx.field_id_for(owner, name))
                .collect::<Result<Vec<_>, _>>()?;
            ctx.emit("struct-literal", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::StructLiteral {
                    type_id: owner,
                    fields: lowered_fields,
                    extra,
                    trait_coercion,
                    boxed_fields,
                }),
            )
        },
        TExprKind::Field {
            recv,
            field,
            boxed: _,
        } => {
            let field = crate::Syntax::compiler_fact_member(field).unwrap_or(field);
            let computed_grads = field == "grads"
                && matches!(recv.ty.without_user_tags(), Type::Apply { name, args }
                    if name == "VjpRun" && args.len() == 1);
            let stored_ty = if computed_grads {
                ctx.checked_field_type(&recv.ty, field)?
            } else {
                expr.ty.clone()
            };
            let field_id = ctx.field_id_for_type(&recv.ty, field)?;
            let base = ctx.lower_child(recv)?;
            let value = ctx.emit("field", 
                Some(stored_ty),
                MirOperation::Field {
                    base,
                    field: field_id,
                },
            )?;
            if computed_grads {
                return ctx.emit(
                    "computed-field",
                    Some(expr.ty.clone()),
                    MirOperation::IndirectCall {
                        callee: value,
                        args: Vec::new(),
                        type_args: Vec::new(),
                    },
                );
            }
            Ok(value)
        }
        TExprKind::SharedGuardValue { guard, .. } => {
            let value = ctx.lower_child(guard)?;
            ctx.emit("shared-guard-deref", Some(expr.ty.clone()), MirOperation::Deref { value })
        }
        TExprKind::SharedGuardMap {
            guard,
            path,
            editable,
        } => {
            let path = ctx.shared_guard_field_path(&guard.ty, path)?;
            let guard = ctx.lower_child(guard)?;
            let call = ctx.intern_prelude_route(super::shared_guard_map_route(
                *editable,
                &expr.ty,
                &carrier,
            )?)?;
            ctx.emit(
                "shared-guard-map",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::SharedGuardMap {
                    call,
                    guard,
                    path,
                    editable: *editable,
                }),
            )
        }
        TExprKind::SharedGuardSplit {
            guard,
            first,
            second,
            editable,
        } => {
            let first = ctx.shared_guard_field_path(&guard.ty, first)?;
            let second = ctx.shared_guard_field_path(&guard.ty, second)?;
            let guard = ctx.lower_child(guard)?;
            let map_call = ctx.intern_prelude_route(super::shared_guard_map_route(
                *editable,
                &expr.ty,
                &carrier,
            )?)?;
            let call = ctx.intern_prelude_route(super::shared_guard_split_route(
                *editable,
                &expr.ty,
                &carrier,
            )?)?;
            ctx.emit(
                "shared-guard-split",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::SharedGuardSplit {
                    call,
                    map_call,
                    guard,
                    first,
                    second,
                    editable: *editable,
                }),
            )
        }
        TExprKind::SharedGuardWait {
            guard,
            condition,
            predicate,
        } => {
            let guard = ctx.lower_child(guard)?;
            let condition = ctx.lower_child(condition)?;
            let predicate = ctx.lower_lambda(predicate)?;
            let call = ctx.intern_prelude_route(super::shared_guard_wait_route(
                &expr.ty,
                &carrier,
            )?)?;
            ctx.emit("shared-guard-wait", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::SharedGuardWait {
                    call,
                    guard,
                    condition,
                    predicate,
                }),
            )
        }
        TExprKind::ConditionNotify { condition, all } => {
            let condition = ctx.lower_child(condition)?;
            let call = ctx.intern_prelude_route(super::condition_notify_route(
                *all,
                &expr.ty,
                &carrier,
            )?)?;
            ctx.emit("condition-notify", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::ConditionNotify {
                    call,
                    condition,
                    all: *all,
                }),
            )
        }
        TExprKind::PtrFromAddr { elem, addr } => {
            let addr = ctx.lower_child(addr)?;
            let element = ctx.mir_type(elem)?;
            ctx.emit("ptr-from-addr", 
                Some(expr.ty.clone()),
                MirOperation::PtrFromAddr {
                    addr,
                    element,
                },
            )
        }
        TExprKind::Deref(inner) => {
            let value = ctx.lower_child(inner)?;
            ctx.emit("deref", Some(expr.ty.clone()), MirOperation::Deref { value })
        }
        TExprKind::RawOf(inner) => {
            let place = ctx.lower_place(
                &TPlace::Expr(Box::new((**inner).clone())),
                MirAccess::Read,
            )?;
            ctx.emit("raw-of", 
                Some(expr.ty.clone()),
                MirOperation::RawAddressOf { place },
            )
        }
        TExprKind::AllocNew { ctor, args } => {
            let kind = match ctor {
                super::TAllocCtor::Arena => MirAllocatorKind::Arena,
                super::TAllocCtor::Bump => MirAllocatorKind::Bump,
                super::TAllocCtor::Pool => MirAllocatorKind::Pool,
                super::TAllocCtor::Fixed { .. } | super::TAllocCtor::FixedOver => {
                    MirAllocatorKind::Fixed
                }
            };
            let inline_size = match ctor {
                super::TAllocCtor::Fixed { size } => Some(*size),
                _ => None,
            };
            let route = super::alloc_new_route(ctor, &expr.ty, args.len(), &carrier)?;
            let mut lowered_args = lower_call_args(ctx, args)?;
            apply_route_access(ctx, &route, &mut lowered_args)?;
            let call = ctx.intern_prelude_route(route)?;
            ctx.emit(
                "alloc-new",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::AllocNew {
                    call,
                    kind,
                    inline_size,
                    args: lowered_args,
                }),
            )
        }
        TExprKind::EnumLit {
            enum_type,
            variant,
            payload,
        } => {
            let type_id = ctx.type_id_for(enum_type)?;
            let args = lower_enum_payload(ctx, enum_type, variant, payload)?;
            ctx.emit("enum-payload", 
                Some(expr.ty.clone()),
                MirOperation::Enum {
                    type_id,
                    variant: variant.clone(),
                    args,
                },
            )
        }
        TExprKind::JSONLit { variant, arg } => {
            if variant == "Object" {
                let Some((value, _)) = arg.as_deref() else {
                    return Err(ctx.error(ctx.span(), "DataTree.Object is missing its checked map payload"));
                };
                return lower_container_encode(
                    ctx,
                    expr,
                    value,
                    &Type::Named(crate::Syntax::TYPE_DATA.to_string()),
                    true,
                );
            }
            let type_id = ctx.type_id_for("DataTree")?;
            let args = arg
                .as_deref()
                .map(|(value, clone)| lower_cloned_value(ctx, value, *clone))
                .transpose()?
                .into_iter()
                .map(|value| MirEnumArg {
                    field: None,
                    value,
                    boxed: false,
                })
                .collect();
            ctx.emit("enum-present", 
                Some(expr.ty.clone()),
                MirOperation::Enum {
                    type_id,
                    variant: variant.clone(),
                    args,
                },
            )
        }
        TExprKind::DBValueLit { variant, arg } => {
            let type_id = ctx.type_id_for("DBValue")?;
            let args = arg
                .as_deref()
                .map(|(value, clone)| lower_cloned_value(ctx, value, *clone))
                .transpose()?
                .into_iter()
                .map(|value| MirEnumArg {
                    field: None,
                    value,
                    boxed: false,
                })
                .collect();
            ctx.emit("enum-absent", 
                Some(expr.ty.clone()),
                MirOperation::Enum {
                    type_id,
                    variant: variant.clone(),
                    args,
                },
            )
        }
        TExprKind::ListLit(items) => {
            let values = lower_values(ctx, items)?;
            ctx.emit(
                "list-literal",
                Some(expr.ty.clone()),
                MirOperation::BuildList { values },
            )
        }
        TExprKind::ListSpread { parts } => {
            if !matches!(expr.ty.without_user_tags(), crate::AST::Type::List(_)) {
                return unsupported_expr(
                    ctx,
                    "TExprKind::ListSpread requires a homogeneous List<T> result",
                );
            }
            let mut accumulator = ctx.emit(
                "list-spread",
                Some(expr.ty.clone()),
                MirOperation::BuildList { values: Vec::new() },
            )?;
            for part in parts {
                match part {
                    ListSpreadPart::Elem(value) => {
                        let value_id = ctx.lower_child(value)?;
                        let singleton = ctx.emit(
                            "list-spread-element",
                            Some(expr.ty.clone()),
                            MirOperation::BuildList {
                                values: vec![value_id],
                            },
                        )?;
                        accumulator =
                            emit_concat_list(ctx, &expr.ty, &carrier, accumulator, singleton)?;
                    }
                    ListSpreadPart::Spread(value) => {
                        if !matches!(value.ty.without_user_tags(), crate::AST::Type::List(_)) {
                            return unsupported_expr(
                                ctx,
                                "TExprKind::ListSpread has a non-List spread operand",
                            );
                        }
                        let value_id = ctx.lower_child(value)?;
                        accumulator =
                            emit_concat_list(ctx, &expr.ty, &carrier, accumulator, value_id)?;
                    }
                }
            }
            Ok(accumulator)
        }
        TExprKind::ColumnarListLit { elems, .. } => {
            let values = lower_values(ctx, elems)?;
            ctx.emit(
                "columnar-list-literal",
                Some(expr.ty.clone()),
                MirOperation::BuildList { values },
            )
        }
        TExprKind::ColumnarGather { base, index, line } => {
            let kind = MirIndexKind::List;
            let access = MirAccess::Read;
            let call = ctx.intern_prelude_route(super::index_route(
                kind,
                access,
                &expr.ty,
                &carrier,
            )?)?;
            let location = panic_location(ctx, *line);
            let base = ctx.lower_child(base)?;
            let index = ctx.lower_child(index)?;
            ctx.emit("columnar-gather", 
                Some(expr.ty.clone()),
                MirOperation::Index {
                    call,
                    base,
                    index,
                    kind,
                    access,
                    location,
                    context: ctx.panic_context_at(*line as u32, None),
                },
            )
        }
        TExprKind::ColumnarColumnRead {
            base,
            index,
            owner,
            field,
            column,
            line: _,
        } => {
            let owner_id = ctx.type_id_for(owner)?;
            let column_id = ctx.field_id_for(owner_id, field)?;
            let call = ctx.intern_prelude_route(super::columnar_read_route(
                owner,
                field,
                *column,
                &expr.ty,
                &carrier,
            )?)?;
            let base = ctx.lower_child(base)?;
            let index = ctx.lower_child(index)?;
            ctx.emit("columnar-column-read", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::ColumnarRead {
                    accessor: call,
                    base,
                    index,
                    column: column_id,
                    column_index: *column,
                }),
            )
        }
        TExprKind::TupleLit {
            struct_name: _,
            fields,
        } => {
            let owner = ctx
                .mir_type(&expr.ty)?
                .identity
                .ok_or_else(|| ctx.error(ctx.span(), "checked tuple type has no canonical identity"))?;
            let fields = fields
                .iter()
                .map(|(name, value)| {
                    Ok((ctx.field_id_for_type(&expr.ty, name)?, ctx.lower_child(value)?))
                })
                .collect::<Result<Vec<_>, LowerError>>()?;
            ctx.emit("tuple-literal", 
                Some(expr.ty.clone()),
                MirOperation::Tuple {
                    type_id: owner,
                    fields,
                },
            )
        }
        TExprKind::MapLit(entries) => {
            let entries = entries
                .iter()
                .map(|(key, value)| {
                    Ok((ctx.lower_child(key)?, ctx.lower_child(value)?))
                })
                .collect::<Result<Vec<_>, LowerError>>()?;
            ctx.emit(
                "map-literal",
                Some(expr.ty.clone()),
                MirOperation::BuildMap { entries },
            )
        }
        TExprKind::Index {
            base,
            index,
            is_map,
            uninit_fixed,
            line,
        } => {
            let kind = if *is_map {
                MirIndexKind::Map
            } else if *uninit_fixed {
                MirIndexKind::FixedListProof
            } else {
                MirIndexKind::List
            };
            let access = MirAccess::Read;
            let call = ctx.intern_prelude_route(super::index_route(
                kind,
                access,
                &expr.ty,
                &carrier,
            )?)?;
            let location = panic_location(ctx, *line);
            let base = ctx.lower_child(base)?;
            let index = ctx.lower_child(index)?;
            ctx.emit("list-index", 
                Some(expr.ty.clone()),
                MirOperation::Index {
                    call,
                    base,
                    index,
                    kind,
                    access,
                    location,
                    context: ctx.panic_context_at(*line as u32, None),
                },
            )
        }
        TExprKind::PoolSlot {
            pool,
            id,
            mutable,
            field,
            line,
            src_line,
        } => {
            let element_ty = match pool.ty.without_user_tags() {
                crate::AST::Type::Apply { args, .. } if args.len() == 1 => args[0].clone(),
                _ => {
                    return unsupported_expr(
                        ctx,
                        "TExprKind::PoolSlot has no checked element type",
                    )
                }
            };
            let kind = MirIndexKind::Pool;
            let access = if *mutable {
                MirAccess::Write
            } else {
                MirAccess::Read
            };
            let call = ctx.intern_prelude_route(super::index_route(
                kind,
                access,
                &expr.ty,
                &carrier,
            )?)?;
            let location = panic_location(ctx, *line);
            let pool_value = ctx.lower_child(pool)?;
            let id = ctx.lower_child(id)?;
            let slot = ctx.emit("pool-slot", 
                Some(element_ty.clone()),
                MirOperation::Index {
                    call,
                    base: pool_value,
                    index: id,
                    kind,
                    access,
                    location,
                    context: ctx.panic_context_at(*line as u32, Some(src_line)),
                },
            )?;
            if let Some(field) = field {
                let field = ctx.field_id_for_type(&element_ty, field)?;
                ctx.emit(
                    "pool-field",
                    Some(expr.ty.clone()),
                    MirOperation::Field { base: slot, field },
                )
            } else {
                Ok(slot)
            }
        }
        TExprKind::IndexHook {
            type_name,
            base,
            index,
            line,
        } => lower_index_hook(ctx, expr, type_name, base, index, *line),
        TExprKind::MathLaneIndex {
            lane_ty,
            base,
            index,
            line,
        } => {
            let kind = MirIndexKind::Lane;
            let access = MirAccess::Read;
            let call = ctx.intern_prelude_route(super::lane_index_route(
                lane_ty,
                &expr.ty,
                &carrier,
            )?)?;
            let location = panic_location(ctx, *line as usize);
            let base = ctx.lower_child(base)?;
            let index = ctx.lower_child(index)?;
            ctx.emit(
                "math-lane-index",
                Some(expr.ty.clone()),
                MirOperation::Index {
                    call,
                    base,
                    index,
                    kind,
                    access,
                    location,
                    context: ctx.panic_context_at(*line, None),
                },
            )
        }
        TExprKind::MathSwizzleRead {
            type_name,
            recv,
            lanes,
            ..
        } => {
            let owner = ctx.type_id_for(type_name)?;
            let base = ctx.lower_child(recv)?;
            let members = lanes
                .iter()
                .map(|lane| ctx.field_id_for(owner, &lane.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            ctx.emit("project-members", 
                Some(expr.ty.clone()),
                MirOperation::ProjectMembers { base, members },
            )
        }
        TExprKind::Slice {
            base,
            start,
            end,
            range,
            line,
        } => {
            let call = ctx.intern_prelude_route(super::slice_route(
                &base.ty,
                range.is_some(),
                &expr.ty,
                &carrier,
            )?)?;
            let location = panic_location(ctx, *line);
            let base = ctx.lower_child(base)?;
            let start = ctx.lower_child(start)?;
            let end = ctx.lower_child(end)?;
            let range = range
                .as_deref()
                .map(|range| ctx.lower_child(range))
                .transpose()?;
            ctx.emit("slice", 
                Some(expr.ty.clone()),
                MirOperation::Slice {
                    call,
                    base,
                    start,
                    end,
                    range,
                    location,
                },
            )
        }
        TExprKind::Clone(inner)
        | TExprKind::ExplicitCopy(inner)
        | TExprKind::MaterializeView(inner) => {
            let value = ctx.lower_child(inner)?;
            ctx.emit("copy", Some(expr.ty.clone()), MirOperation::Copy { value })
        }
        TExprKind::Borrow { place, mutable } => {
            let place = ctx.lower_place(
                &TPlace::Expr(Box::new((**place).clone())),
                if *mutable {
                    MirAccess::Write
                } else {
                    MirAccess::Read
                },
            )?;
            ctx.emit("borrow", 
                Some(expr.ty.clone()),
                MirOperation::AddressOf {
                    place,
                    access: if *mutable {
                        MirAccess::Write
                    } else {
                        MirAccess::Read
                    },
                },
            )
        }
        TExprKind::MethodCall {
            method,
            type_args,
            args,
            recv,
            ..
        } => {
            let owner = ctx.mir_type(&recv.ty)?;
            let (callee, access) = if matches!(recv.ty.without_user_tags(), Type::TraitObject(_)) {
                let trait_name = method.trait_owner.as_deref().ok_or_else(|| {
                    ctx.error(
                        ctx.span(),
                        format!("trait-object method `{}` is missing its checked trait owner", method.name),
                    )
                })?;
                let (trait_ref, method_id, access) =
                    ctx.trait_method_identity(trait_name, &method.name)?;
                let access = access.ok_or_else(|| {
                    ctx.error(ctx.span(), "checked instance trait method has no receiver convention")
                })?;
                (MirCallee::TraitMethod { method: method_id, trait_ref, receiver: owner }, access)
            } else {
                let function = ctx.function_id_for(&instance_method_lookup(method, &recv.ty))?;
                let access = ctx.function_registry.receiver_access.get(&function).copied().ok_or_else(|| {
                    ctx.error(ctx.span(), "checked instance method has no receiver convention")
                })?;
                (MirCallee::Method { function, owner }, access)
            };
            let receiver = if access == MirAccess::Read {
                ctx.lower_plain_arg(recv)?
            } else {
                let mut place = lower_receiver_place(ctx, recv, access)?;
                if place.is_none() && access == MirAccess::Write {
                    let value = ctx.lower_child(recv)?;
                    let local = TLocal::generated(format!("method_receiver_{}", value.0)).as_mutable();
                    let target = ctx.bind_local(&local, recv.ty.clone(), true, false, false)?;
                    ctx.emit("method-receiver-temp", None, MirOperation::WritePlace { place: target, value })?;
                    place = Some(lower_local_place(ctx, &local, access)?);
                }
                let value = match place {
                    Some(place) => ctx.emit(
                        "method-receiver",
                        Some(recv.ty.clone()),
                        if access == MirAccess::Write {
                            MirOperation::AddressOf { place, access }
                        } else {
                            MirOperation::MovePlace { place }
                        },
                    )?,
                    None => ctx.lower_child(recv)?,
                };
                let mut argument = mir_value_arg_with_access(ctx, value, access);
                argument.place = if access == MirAccess::Write { place } else { None };
                argument
            };
            let mut lowered = Vec::with_capacity(args.len() + 1);
            lowered.push(receiver);
            lowered.extend(lower_call_args(ctx, args)?);
            let type_args = lower_mir_types(ctx, type_args)?;
            ctx.emit(
                "method-call",
                Some(expr.ty.clone()),
                MirOperation::Call {
                    callee,
                    args: lowered,
                    type_args,
                },
            )
        }
        TExprKind::StaticCall {
            owner,
            owner_type,
            method,
            type_args,
            args,
            ..
        } => match owner {
            super::TStaticOwner::User(owner) => {
                let resolved_method_key = if method.operator_identity.is_some() {
                    method_key(method)
                } else {
                    ctx.trait_method_traits
                        .get(&(owner.clone(), method.name.clone()))
                        .map_or_else(|| method_key(method), |trait_name| {
                            format!("{trait_name}::{}", method.name)
                        })
                };
                let name = if method.operator_identity.is_some() {
                    resolved_method_key
                } else {
                    format!("{owner}::{resolved_method_key}")
                };
                let function = ctx.function_id_for(&name)?;
                let owner_ty = owner_type
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| crate::AST::Type::Named(owner.clone()));
                let args = lower_call_args(ctx, args)?;
                let owner = ctx.mir_type(&owner_ty)?;
                let type_args = lower_mir_types(ctx, type_args)?;
                ctx.emit("static-call", 
                    Some(expr.ty.clone()),
                    MirOperation::Call {
                        callee: MirCallee::Associated {
                            function,
                            owner,
                        },
                        args,
                        type_args,
                    },
                )
            }
            super::TStaticOwner::Prelude {
                rooted,
                path,
                generics,
            } => {
                let borrow_mask = args.iter()
                    .map(|arg| arg.borrow || arg.mut_borrow)
                    .collect::<Vec<_>>();
                let args = lower_call_args(ctx, args)?;
                let owner_type_args = generics
                    .iter()
                    .map(|generic| match generic {
                        super::TPreludeArg::Jet(ty) => {
                            Ok(MirPreludeTypeArg::Type(ctx.mir_type(ty)?))
                        }
                        super::TPreludeArg::HostUsize => Ok(MirPreludeTypeArg::HostUsize),
                    })
                    .collect::<Result<Vec<_>, LowerError>>()?;
                let type_args = lower_mir_types(ctx, type_args)?;
                let member = method_key(method);
                let module = if *rooted {
                    format!("::{path}")
                } else {
                    path.clone()
                };
                if module == "core.data.plot" && member == "column" && args.len() == 2 {
                    let mut call_args = args.into_iter();
                    let receiver = call_args
                        .next()
                        .expect("plot column marker has a field receiver");
                    let call = ctx.intern_prelude_route(super::TPreludeRoute {
                        family: MirPreludeFamily::ClosureMethod,
                        module: "core.data.plot".to_string(),
                        member: "column".to_string(),
                        symbol: MirSymbol::Prelude("jet_data_plot_column".to_string()),
                        signature: MirCallSignature {
                            arity: 2,
                            max_arity: 2,
                            borrow_mask: vec![false, false],
                        },
                        effect: None,
                        fallibility: carrier.clone(),
                        abi: MirPreludeAbi::Value,
                        authority: None,
                        db_metadata: None,
                    })?;
                    ctx.emit(
                        "plot-column",
                        Some(expr.ty.clone()),
                        MirOperation::Semantic(MirSemanticOp::ClosureMethod {
                            receiver: receiver.value,
                            args: call_args.collect(),
                            call,
                        }),
                    )
                } else {
                    let call = ctx.intern_prelude_route(super::static_prelude_route(
                        &module,
                        &member,
                        &borrow_mask,
                        &expr.ty,
                        &carrier,
                    )?)?;
                    ctx.emit("static-prelude-call",
                        Some(expr.ty.clone()),
                        MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
                            call,
                            args,
                            owner_type_args,
                            type_args,
                        }),
                    )
                }
            }
        },
        TExprKind::DecodeUnder { segment, inner } => {
            let segment = ctx.lower_child(segment)?;
            let inner = ctx.lower_child(inner)?;
            let call = ctx.intern_prelude_route(super::decode_under_route(
                &expr.ty,
                &carrier,
            )?)?;
            ctx.emit("decode-under", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::DecodeUnder {
                    call,
                    segment,
                    inner,
                }),
            )
        }
        TExprKind::BuiltinMethod { recv, op, args } => {
            if matches!(op, TBuiltinOp::Zip { .. }) {
                return lower_zip(ctx, expr, recv, op, args);
            }
            if matches!(op, TBuiltinOp::Unzip { .. }) {
                return lower_unzip(ctx, expr, recv);
            }
            if matches!(op, TBuiltinOp::Indexed { .. } | TBuiltinOp::IterSplit { .. }) {
                return lower_tuple_builtin(ctx, expr, recv, op, args, &carrier);
            }
            if matches!(op, TBuiltinOp::ListMinMax { .. }) {
                return lower_list_min_max(ctx, expr, recv, op, args, &carrier);
            }
            let (mut receiver, receiver_place) =
                lower_builtin_receiver(ctx, recv, op.needs_mut_receiver_place())?;
            let mut lowered_args = lower_values(ctx, args)?;
            if matches!(op, TBuiltinOp::ByteBufferWithCapacity) {
                receiver = lower_builtin_native_int(ctx, receiver, &recv.ty)?;
            }
            if matches!(op, TBuiltinOp::ListReplace | TBuiltinOp::MatchGroup) {
                let Some(arg) = args.first() else {
                    return Err(ctx.error(ctx.span(), "checked builtin requires an index operand"));
                };
                lowered_args[0] = lower_builtin_native_int(ctx, lowered_args[0], &arg.ty)?;
            }
            let route = op.prelude_route(&recv.ty, &expr.ty, &carrier).map_err(|error| {
                ctx.error(ctx.span(), format!("builtin method {op:?}: {error:?}"))
            })?;
            if matches!(op, TBuiltinOp::MapFromKeys)
                && matches!(recv.ty.without_user_tags(), Type::FixedList { .. })
            {
                let mut operands = vec![mir_value_arg_with_access(
                    ctx, receiver, ctx.call_value_access(receiver, false)?,
                )];
                operands[0].widen_fixed_to_list = true;
                for value in lowered_args {
                    operands.push(mir_value_arg_with_access(
                        ctx, value, ctx.call_value_access(value, false)?,
                    ));
                }
                let call = ctx.intern_prelude_route(route)?;
                return ctx.emit(
                    "map-from-fixed-keys",
                    Some(expr.ty.clone()),
                    MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
                        call,
                        args: operands,
                        owner_type_args: Vec::new(),
                        type_args: Vec::new(),
                    }),
                );
            }
            let call = ctx.intern_prelude_route(route)?;
            ctx.emit("builtin-method",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::BuiltinMethod {
                    call,
                    receiver,
                    receiver_place,
                    args: lowered_args,
                    aggregate_fields: None,
                }),
            )
        }
        TExprKind::CoreCall {
            record,
            args,
            source_span: _,
            type_args,
            widen_to_vec,
            data_plan,
            fallibility,
        } => {
            if record.module == "core.http" && record.member == "router" {
                if !args.is_empty() {
                    return Err(ctx.error(
                        ctx.span(),
                        "HTTPRouter constructor MIR projection received arguments",
                    ));
                }
                let route = ctx.intern_core_route(record, fallibility)?;
                return ctx.emit(
                    "http-router-new",
                    Some(expr.ty.clone()),
                    MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
                        call: route,
                        args: Vec::new(),
                        owner_type_args: Vec::new(),
                        type_args: Vec::new(),
                    }),
                );
            }
            if record.module == "core.web" && record.member == "openapi" {
                let receiver = args.first().ok_or_else(|| {
                    ctx.error(
                        ctx.span(),
                        "HTTPRouter OpenAPI MIR projection requires one router operand",
                    )
                })?;
                if args.len() != 1 {
                    return Err(ctx.error(
                        ctx.span(),
                        "HTTPRouter OpenAPI MIR projection received unexpected arguments",
                    ));
                }
                let receiver = ctx.lower_core_call_arg(receiver, 0, record, false)?.value;
                let route = ctx.intern_core_route(record, fallibility)?;
                return ctx.emit(
                    "http-router-openapi",
                    Some(expr.ty.clone()),
                    MirOperation::Semantic(MirSemanticOp::HandleMethod {
                        frame_schedule: None,
                        frame_schedule_derivation: None,
                        call: route,
                        receiver,
                        args: Vec::new(),
                    }),
                );
            }
            let data_plan = data_plan
                .as_ref()
                .map(|plan| lower_data_plan(ctx, plan))
                .transpose()?;
            let call = MirCoreCall::from_record(record).id;
            let route = ctx.intern_core_route(record, fallibility)?;
            let lowered_fallibility = ctx.lower_call_fallibility(fallibility)?;
            let lowered = args
                .iter()
                .enumerate()
                .map(|(index, arg)| {
                    let widen = widen_to_vec.get(index).copied().unwrap_or(false);
                    ctx.lower_core_call_arg(arg, index, record, widen)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let type_args = if !type_args.is_empty() {
                type_args
                    .iter()
                    .map(|ty| ctx.mir_type(ty))
                    .collect::<Result<Vec<_>, _>>()?
            } else if record.module.is_empty() && record.member == "query" {
                let row = args
                    .first()
                    .and_then(|arg| match &arg.ty {
                        Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                            Some((**inner).clone())
                        }
                        Type::Apply { name, args } if name == "DataStream" && args.len() == 1 => {
                            Some(args[0].clone())
                        }
                        _ => None,
                    })
                    .ok_or_else(|| {
                        ctx.error(
                            ctx.span(),
                            "list query MIR projection needs a retained row type",
                        )
                    })?;
                vec![ctx.mir_type(&row)?]
            } else if record.module == "core.encoding.csv" && record.member == "query" {
                let row = csv_query_row_type(&expr.ty).ok_or_else(|| {
                    ctx.error(
                        ctx.span(),
                        "CSV query MIR projection needs Result<Query<T>, List<FieldError>>",
                    )
                })?;
                vec![ctx.mir_type(&row)?]
            } else if (record.module == "core.data"
                && matches!(
                    record.member,
                    "load"
                        | "load_default"
                        | "file"
                        | "file_member"
                        | "url"
                        | "database"
                        | "value"
                        | "snapshot"
                ))
                || (record.module == "core.data.loader" && record.member == "stream")
                || (record.module == "core.data.stream"
                    && matches!(record.member, "next" | "collect"))
            {
                let row = data_row_type(&expr.ty)
                    .or_else(|| args.first().and_then(|arg| data_row_type(&arg.ty)))
                    .ok_or_else(|| {
                        ctx.error(
                            ctx.span(),
                            "typed data CoreCall MIR projection needs one retained row type",
                        )
                    })?;
                vec![ctx.mir_type(&row)?]
            } else if (record.module == "core.args" && matches!(record.member, "decode" | "merge"))
                || (record.module == "core.sys" && record.member == "decode")
                || (record.module == "core.db" && record.member == "decode")
                || (record.module == "core.encoding.json"
                    && record.member == "decode"
                    && matches!(&expr.ty, Type::Result { err, .. }
                        if **err == Type::List(Box::new(Type::Named("FieldError".to_string())))))
            {
                let target = match &expr.ty {
                    Type::Result { ok, .. } => ok.as_ref(),
                    _ => {
                        return Err(ctx.error(
                            ctx.span(),
                            "typed shape decode/merge MIR projection needs Result<T, List<FieldError>>",
                        ));
                    }
                };
                vec![ctx.mir_type(target)?]
            } else {
                Vec::new()
            };

            ctx.emit("core-call", 
                Some(expr.ty.clone()),
                MirOperation::CoreCall {
                    call,
                    route,
                    args: lowered,
                    type_args,
                    fallibility: lowered_fallibility,
                    data_plan,
                },
            )
        }
        TExprKind::CoreClosureCall { kind } => lower_core_closure_call(ctx, expr, kind),
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => lower_if_expr(
            ctx,
            expr,
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        ),
        TExprKind::InvariantViolation { construct, span } => {
            Err(ctx.error(*span, construct.clone()))
        }
        TExprKind::Unreachable { line } => {
            let value = ctx.emit(
                "exhaustive-dispatch-end",
                Some(Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string())),
                MirOperation::Constant(MirConstant::Unit),
            )?;
            ctx.terminate(MirTerminator::Unreachable {
                reason: format!("checked exhaustive dispatch at line {line}"),
            });
            Ok(value)
        }
        TExprKind::Todo { line } => {
            let call = ctx.intern_prelude_route(super::todo_route(&expr.ty, &carrier)?)?;
            let location = panic_location(ctx, *line);
            let expected_type = Some(ctx.mir_type(&expr.ty)?);
            ctx.emit("todo", 
                Some(expr.ty.clone()),
                MirOperation::Todo {
                    call,
                    location,
                    expected_type,
                },
            )
        }
        TExprKind::DistinctRaw(inner) => {
            let value = ctx.lower_child(inner)?;
            let target = ctx.mir_type(&expr.ty)?;
            ctx.emit(
                "distinct-raw",
                Some(expr.ty.clone()),
                MirOperation::Convert {
                    value,
                    parameters: Vec::new(),
                    target,
                    conversion: MirConversion::Transparent,
                },
            )
        }
        TExprKind::Present(inner) => {
            let value = ctx.lower_child(inner)?;
            if ctx.is_terminated() {
                return Ok(value);
            }
            ctx.emit("present", Some(expr.ty.clone()), MirOperation::Present { value })
        }
        TExprKind::Absent => ctx.emit("absent", Some(expr.ty.clone()), MirOperation::Absent),
        TExprKind::Ok(inner) => {
            let value = ctx.lower_child(inner)?;
            if ctx.is_terminated() {
                return Ok(value);
            }
            ctx.emit("result-ok", Some(expr.ty.clone()), MirOperation::ResultOk { value })
        }
        TExprKind::Err(inner) => {
            let value = ctx.lower_child(inner)?;
            if ctx.is_terminated() {
                return Ok(value);
            }
            ctx.emit("result-err", Some(expr.ty.clone()), MirOperation::ResultErr { value })
        }
        TExprKind::Try {
            inner,
            note,
            convert,
            file,
            line,
            fn_name,
        } => {
            let input = ctx.lower_child(inner)?;
            if try_child_already_propagated(inner) {
                Ok(input)
            } else {
                lower_try_value(
                    ctx,
                    input,
                    &expr.ty,
                    &inner.ty,
                    note.as_deref(),
                    convert,
                    Some((file, *line, fn_name)),
                )
            }
        }
        TExprKind::OrFallback { value, fallback } => {
            lower_or_fallback_expr(ctx, expr, value, fallback)
        }
        TExprKind::OptField {
            base,
            member,
            flatten,
        } => lower_optional_field(ctx, expr, base, member, *flatten),
        TExprKind::Lambda(lambda) => ctx.lower_lambda(lambda),
        TExprKind::PatternMatches { subj, pattern } => {
            let subject = ctx.lower_child(subj)?;
            let pattern = lower_pattern(ctx, pattern)?;
            ctx.lower_pattern_condition(subject, &pattern)
        }
        TExprKind::OptionLift2 { f, a, b } => {
            let function = ctx.lower_child(f)?;
            let left = ctx.lower_child(a)?;
            let right = ctx.lower_child(b)?;
            let call = ctx.intern_prelude_route(direct_prelude_route(
                "core.prelude",
                "option_lift2",
                "jet_option_lift2",
                3,
                &[false, false, false],
                &carrier,
            ))?;
            ctx.emit(
                "option-lift2",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::OptionLift2 {
                    call,
                    function,
                    left,
                    right,
                }),
            )
        }
        TExprKind::TaskGroupAll { tasks } => {
            let call = ctx.intern_prelude_route(task_group_route(
                MirTaskGroupKind::All,
                &tasks.ty,
                &carrier,
            ))?;
            let tasks = vec![lower_consuming_task_value(ctx, tasks)?];
            ctx.emit(
                "task-group-all",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::TaskGroup {
                    call,
                    kind: MirTaskGroupKind::All,
                    tasks,
                }),
            )
        }
        TExprKind::TaskGroupRace { tasks } => {
            let call = ctx.intern_prelude_route(task_group_route(
                MirTaskGroupKind::Race,
                &tasks.ty,
                &carrier,
            ))?;
            let tasks = vec![lower_consuming_task_value(ctx, tasks)?];
            ctx.emit(
                "task-group-race",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::TaskGroup {
                    call,
                    kind: MirTaskGroupKind::Race,
                    tasks,
                }),
            )
        }
        TExprKind::TaskGroupAny { tasks } => {
            let call = ctx.intern_prelude_route(task_group_route(
                MirTaskGroupKind::Any,
                &tasks.ty,
                &carrier,
            ))?;
            let tasks = vec![lower_consuming_task_value(ctx, tasks)?];
            ctx.emit(
                "task-group-any",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::TaskGroup {
                    call,
                    kind: MirTaskGroupKind::Any,
                    tasks,
                }),
            )
        }
        TExprKind::SelectStart
        | TExprKind::SelectRecv { .. }
        | TExprKind::SelectAfter { .. } => Err(ctx.error(
            ctx.span(),
            "readiness-table construction is only valid as the operand of select wait",
        )),
        TExprKind::SelectWait {
            builder,
            nonblocking,
        } => lower_select_wait(ctx, expr, builder, *nonblocking, &carrier),
        TExprKind::ClosureMethod { recv, op, args } => {
            let default_para_limit = matches!(op, super::TClosureOp::ParaMap) && args.len() == 1;
            let route = op.prelude_route(&recv.ty, &expr.ty, &carrier)?;
            let operand_count = args.len() + 1 + usize::from(default_para_limit);
            if operand_count != route.signature.arity {
                return Err(ctx.error(
                    ctx.span(),
                    format!(
                        "checked closure method has {} operands, route requires {}",
                        operand_count,
                        route.signature.arity
                    ),
                ));
            }
            let (receiver, _) = lower_builtin_receiver(ctx, recv, false)?;
            let mut args = args
                .iter()
                .map(|arg| ctx.lower_plain_arg(arg))
                .collect::<Result<Vec<_>, _>>()?;
            if default_para_limit {
                let limit = ctx.emit(
                    "para-map-default-limit",
                    Some(Type::Int),
                    MirOperation::Constant(MirConstant::Int {
                        value: i64::MAX,
                        width: None,
                        spelling: None,
                    }),
                )?;
                args.push(mir_value_arg_with_access(ctx, limit, MirAccess::Read));
            }
            for (index, arg) in args.iter_mut().enumerate() {
                arg.access = ctx.call_value_access(
                    arg.value,
                    route.signature.borrow_mask.get(index + 1).copied().unwrap_or(false),
                )?;
            }
            let call = ctx.intern_prelude_route(route)?;
            ctx.emit("closure-method", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::ClosureMethod {
                    receiver,
                    args,
                    call,
                }),
            )
        }
        TExprKind::HostBorrowCallback { callable, params } => {
            let callable = ctx.lower_child(callable)?;
            let params = lower_mir_types(ctx, params)?;
            ctx.emit("host-borrow-callback", 
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::HostBorrowCallback {
                    callable,
                    params,
                }),
            )
        }
        TExprKind::NumericMethod { recv, op } => {
            lower_numeric_method(ctx, expr, recv, op)
        }
        TExprKind::NumericBinaryMethod { recv, op, arg } => {
            let receiver = ctx.lower_child(recv)?;
            let argument = ctx.lower_child(arg)?;
            let call = ctx.intern_prelude_route(op.prelude_route(&expr.ty, &carrier)?)?;
            ctx.emit(
                "numeric-binary-method",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::NumericBinaryMethod {
                    call,
                    receiver,
                    argument,
                }),
            )
        }
        TExprKind::OverflowOpt {
            prefix,
            op,
            line,
            policy: _,
            lhs,
            rhs,
        } => {
            let left = ctx.lower_child(lhs)?;
            let right = ctx.lower_child(rhs)?;
            let result_ty = expr.ty.without_user_tags();
            let unbounded_int = matches!(result_ty, Type::Int)
                && super::fixed_width_name(&lhs.ty).is_none();
            if unbounded_int && matches!(prefix.as_ref(), "wrapping" | "saturating") {
                let binop = match op.as_ref() {
                    "add" => BinOp::Add,
                    "sub" => BinOp::Sub,
                    "mul" => BinOp::Mul,
                    "div" => BinOp::Div,
                    "rem" => BinOp::Rem,
                    other => {
                        return Err(ctx.error(
                            ctx.span(),
                            format!("overflow option `{prefix}_{other}` has no Int route"),
                        ));
                    }
                };
                return ctx.emit(
                    "overflow-int",
                    Some(expr.ty.clone()),
                    MirOperation::Binary {
                        op: super::mir_binary_op(binop),
                        left,
                        right,
                        dispatch: MirBinaryDispatch::Primitive,
                    },
                );
            }
            let route = super::overflow_opt_route(
                prefix,
                op,
                *line,
                &lhs.ty,
                &expr.ty,
                &carrier,
            )?;
            let location = if route.signature.arity == 4 && route.signature.max_arity == 4 {
                Some(ctx.panic_location_at(*line))
            } else {
                None
            };
            let call = ctx.intern_prelude_route(route)?;
            ctx.emit(
                "overflow-option",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::OverflowOption {
                    call,
                    left,
                    right,
                    location,
                }),
            )
        }
        TExprKind::HandleMethod { recv, op, args } => {
            if let super::THandleOp::EventMethod { .. } = op {
                let route = op.prelude_route(&recv.ty, &carrier)?;
                let operand_count = args.len() + 1;
                if operand_count < route.signature.arity
                    || operand_count > route.signature.max_arity
                {
                    return Err(ctx.error(
                        ctx.span(),
                        format!(
                            "checked event method has {operand_count} operands, route requires {}..={}",
                            route.signature.arity, route.signature.max_arity
                        ),
                    ));
                }
                let receiver = ctx.lower_child(recv)?;
                let mut args = args
                    .iter()
                    .map(|arg| ctx.lower_plain_arg(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                for (index, arg) in args.iter_mut().enumerate() {
                    arg.access = ctx.call_value_access(
                        arg.value,
                        route.signature.borrow_mask.get(index + 1).copied().unwrap_or(false),
                    )?;
                }
                let call = ctx.intern_prelude_route(route)?;
                return ctx.emit(
                    "event-method",
                    Some(expr.ty.clone()),
                    MirOperation::Semantic(MirSemanticOp::ClosureMethod {
                        receiver,
                        args,
                        call,
                    }),
                );
            }
            if let super::THandleOp::Hardware(hardware_op) = op {
                let receiver = match hardware_op {
                    super::THardwareCall::DmaWait { .. } => Some(ctx.lower_child(recv)?),
                    super::THardwareCall::RegisterRead { .. }
                    | super::THardwareCall::RegisterWrite { .. }
                    | super::THardwareCall::DmaStart { .. } => None,
                };
                let source_args: &[TExpr] = match hardware_op {
                    super::THardwareCall::DmaStart { .. } => args.get(1..).ok_or_else(|| {
                        ctx.error(
                            ctx.span(),
                            "DMA start MIR projection requires a checked buffer operand",
                        )
                    })?,
                    _ => args,
                };
                let mut lowered_args = Vec::with_capacity(source_args.len());
                for (index, arg) in source_args.iter().enumerate() {
                    let mut lowered = ctx.lower_plain_arg(arg)?;
                    if matches!(hardware_op, super::THardwareCall::DmaStart { .. }) && index == 0 {
                        lowered.access = MirAccess::Move;
                        lowered.owned_last_use = true;
                    }
                    lowered_args.push(lowered);
                }
                let call = ctx.intern_prelude_route(op.prelude_route(&recv.ty, &carrier)?)?;
                let hardware_op = lower_hardware_op(ctx, hardware_op)?;
                return ctx.emit(
                    "hardware-call",
                    Some(expr.ty.clone()),
                    MirOperation::Semantic(MirSemanticOp::HardwareCall {
                        call,
                        op: hardware_op,
                        receiver,
                        args: lowered_args,
                    }),
                );
            }
            if matches!(op, super::THandleOp::ReceiptAttach) {
                let record = crate::Syntax::core_receiver_method(
                    crate::Syntax::INTERNAL_RECEIPT_HANDLE,
                    crate::Syntax::METHOD_RECEIPT_ATTACH,
                )
                .ok_or_else(|| {
                    ctx.error(ctx.span(), "receipt.attach Core row is not registered")
                })?;
                if args.len() != 4 {
                    return Err(ctx.error(
                        ctx.span(),
                        "receipt.attach MIR projection requires payload and three metadata operands",
                    ));
                }
                let call = MirCoreCall::from_record(record).id;
                let route = ctx.intern_core_route(record, &carrier)?;
                let lowered_fallibility = ctx.lower_call_fallibility(&carrier)?;
                let lowered = args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        ctx.lower_core_call_arg(arg, index, record, false)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let payload_ty = ctx.mir_type(&args[0].ty)?;
                return ctx.emit(
                    "receipt-attach",
                    Some(expr.ty.clone()),
                    MirOperation::CoreCall {
                        call,
                        route,
                        args: lowered,
                        // Preserve the checked payload type for interpreter
                        // receipt encoding; AOT uses it as the generic argument.
                        type_args: vec![payload_ty],
                        fallibility: lowered_fallibility,
                        data_plan: None,
                    },
                );
            }
            if let THandleOp::HTTPRouterRegister {
                verb: method,
                handler,
                file,
                line,
                handler_param_names,
                contract_json,
            } = op
            {
                if args.len() != 1 {
                    return Err(ctx.error(
                        ctx.span(),
                        "HTTP route registration MIR projection requires one path operand",
                    ));
                }
                let receiver = ctx.lower_child(recv)?;
                let path = ctx.lower_child(&args[0])?;
                let handler = ctx.lower_child(handler)?;
                let call = ctx.intern_prelude_route(op.prelude_route(&recv.ty, &carrier)?)?;
                let location = MirPanicLoc {
                    file: ctx.source_file_id_for(file),
                    line: *line as u32,
                    column: 0,
                };
                return ctx.emit(
                    "http-route-register",
                    Some(expr.ty.clone()),
                    MirOperation::Semantic(MirSemanticOp::HttpRouterRegister {
                        call,
                        receiver,
                        path,
                        handler,
                        method: *method,
                        handler_param_names: handler_param_names.clone(),
                        contract_json: contract_json.clone(),
                        location,
                    }),
                );
            }
            if let super::THandleOp::PluginInvoke {
                export_name,
                signature,
            } = op
            {
                let handle = ctx.lower_child(recv)?;
                let args = lower_values(ctx, args)?;
                let call = ctx.intern_prelude_route(op.prelude_route(&recv.ty, &carrier)?)?;
                return ctx.emit(
                    "plugin-invoke",
                    Some(expr.ty.clone()),
                    MirOperation::Semantic(MirSemanticOp::PluginInvoke {
                        call,
                        handle,
                        export_name: export_name.clone(),
                        signature: signature.clone(),
                        args,
                    }),
                );
            }
            if datatree_access_route(op, &carrier).is_some() {
                return lower_datatree_access(ctx, expr, recv, args, op, &carrier);
            }

            if matches!(op, super::THandleOp::SerdeEncode) {
                return lower_serde_encode(ctx, expr, recv);
            }
            if let super::THandleOp::DataTreeDecode(target) = op {
                return lower_datatree_decode(ctx, expr, recv, target);
            }
            if let Some(receiver) = http_json_method_receiver(&recv.ty, op) {
                return lower_http_json_method(ctx, expr, recv, args, receiver);
            }
            if matches!(op, super::THandleOp::TaskJoin | super::THandleOp::TaskDetach) {
                if !args.is_empty() {
                    return Err(ctx.error(
                        ctx.span(),
                        "checked consuming task operation takes no arguments",
                    ));
                }
                let call = ctx.intern_prelude_route(op.prelude_route(&recv.ty, &carrier)?)?;
                let receiver = lower_consuming_task_value(ctx, recv)?;
                return ctx.emit(
                    "task-consume",
                    Some(expr.ty.clone()),
                    MirOperation::Semantic(MirSemanticOp::HandleMethod {
                        call,
                        receiver,
                        args: Vec::new(),
                        frame_schedule: None,
                        frame_schedule_derivation: None,
                    }),
                );
            }
            let frame_schedule = match op {
                super::THandleOp::GameSceneOnFrame { schedule, .. } => schedule.clone(),
                _ => None,
            };
            let frame_schedule_derivation = match op {
                super::THandleOp::GameSceneOnFrame { derivation, .. } => derivation.clone(),
                _ => None,
            };
            let (receiver, _) = lower_builtin_receiver(
                ctx,
                recv,
                matches!(
                    op,
                    THandleOp::FileReaderReadLine
                        | THandleOp::AllocReset
                        | THandleOp::FileWriterWriteLine
                        | THandleOp::FileWriterFlush
                        | THandleOp::ClockTick
                        | THandleOp::ClockAdvance
                        | THandleOp::ClockWait
                        | THandleOp::FakeName
                        | THandleOp::FakeEmail
                        | THandleOp::FakeHost
                        | THandleOp::FakeAddress
                ),
            )?;
            let mut args = lower_handle_method_args(ctx, args)?;
            if let THandleOp::DurationNew { unit, .. } = op {
                let type_id = ctx.type_id_for(crate::Syntax::DURATION_UNIT_TYPE)?;
                args.push(ctx.emit(
                    "duration-constructor-unit",
                    Some(Type::Named(crate::Syntax::DURATION_UNIT_TYPE.to_string())),
                    MirOperation::Enum {
                        type_id,
                        variant: (*unit).to_string(),
                        args: Vec::new(),
                    },
                )?);
            }
            let route = op.prelude_route(&recv.ty, &carrier)?;
            let call = ctx.intern_prelude_route(route)?;
            ctx.emit(
                "handle-method",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::HandleMethod {
                    call,
                    receiver,
                    args,
                    frame_schedule,
                    frame_schedule_derivation,
                }),
            )
        }
        TExprKind::FnValue { kind } => lower_fn_value(ctx, expr, kind),
        TExprKind::ModuleCall {
            form,
            target_return,
            type_args,
            args,
        } => {
            let name = match form {
                super::TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                    format!("{rust_mod}::{rust_fn}")
                }
                super::TModuleCallForm::InlineMangled { mangled } => mangled.clone(),
            };
            let function = ctx.function_id_for(&name)?;
            let args = lower_call_args(ctx, args)?;
            let type_args = lower_mir_types(ctx, type_args)?;
            let value = ctx.emit(
                "module-call",
                Some(target_return.as_ref().unwrap_or(&expr.ty).clone()),
                MirOperation::Call {
                    callee: MirCallee::User(function),
                    args,
                    type_args,
                },
            )?;
            match target_return {
                Some(target @ (Type::Result { .. } | Type::Option(_)))
                    if target != &expr.ty => lower_try_value(
                    ctx,
                    value,
                    &expr.ty,
                    target,
                    None,
                    &TTryConvert::None,
                    None,
                ),
                _ => Ok(value),
            }
        }
        TExprKind::ExternCall {
            symbol,
            args,
            ..
        } => {
            let args = args
                .iter()
                .map(|arg| lower_extern_arg(ctx, arg))
                .collect::<Result<Vec<_>, _>>()?;
            ctx.emit("extern-call", 
                Some(expr.ty.clone()),
                MirOperation::Call {
                    callee: MirCallee::Foreign(ctx.foreign_id_for(symbol)?),
                    args,
                    type_args: Vec::new(),
                },
            )
        }
    };
    let value = value?;
    if CanonicalPass::enabled() {
        CanonicalPass::record(
            "lowering",
            "mir.lower-expression",
            "crates/jet-codegen/src/Codegen/TIR/tir_to_mir_expr.rs",
            "tir",
            super::canonical_expression_payload(expr),
            super::canonical_expression_identity(expr),
            "mir",
            CanonicalPass::debug_payload("mir", "mir.lower-expression", &value),
            CanonicalPass::debug_identity("mir", "mir.lower-expression", &value),
            "preserve",
        );
    }
    Ok(value)
}

fn lower_values(
    ctx: &mut LowerCtx,
    values: &[TExpr],
) -> Result<Vec<jet_foundation::MIR::MirValueId>, LowerError> {
    values
        .iter()
        .map(|value| ctx.lower_child(value))
        .collect()
}

/// Lower `HandleMethod` arguments so every host-borrow callback is created
/// immediately before the call that borrows it.
///
/// The legality verifier requires a `HostBorrowCallback` result to be consumed
/// by the very next instruction: the callback lends the host a checked
/// callable for exactly one call, and nothing may sit between the borrow and
/// that call.  App route/page/layout/loader rows carry a devtools binding text
/// after the handler, so ordinary source-order lowering would interpose that
/// constant.  Building a callback is side-effect free (a named function, a
/// capture-free lambda, or a local read), so deferring it keeps the checked
/// evaluation order of every other argument while the returned values keep
/// their source positions.
fn lower_handle_method_args(
    ctx: &mut LowerCtx,
    args: &[TExpr],
) -> Result<Vec<jet_foundation::MIR::MirValueId>, LowerError> {
    let mut values = vec![None; args.len()];
    for (index, arg) in args.iter().enumerate() {
        if !is_host_borrow_callback(arg) {
            values[index] = Some(ctx.lower_child(arg)?);
        }
    }
    for (index, arg) in args.iter().enumerate() {
        if is_host_borrow_callback(arg) {
            values[index] = Some(ctx.lower_child(arg)?);
        }
    }
    Ok(values.into_iter().flatten().collect())
}

fn is_host_borrow_callback(arg: &TExpr) -> bool {
    match &arg.kind {
        TExprKind::HostBorrowCallback { .. } => true,
        TExprKind::FnValue {
            kind: TFnValueKind::Send { value },
        } => matches!(value.kind, TExprKind::HostBorrowCallback { .. }),
        _ => false,
    }
}

fn lower_hardware_op(
    ctx: &mut LowerCtx,
    op: &super::THardwareCall,
) -> Result<MirHardwareOp, LowerError> {
    let op = match op {
        super::THardwareCall::RegisterRead {
            profile_id,
            block,
            register,
            width,
        } => MirHardwareOp::RegisterRead {
            profile_id: profile_id.clone(),
            block: block.clone(),
            register: register.clone(),
            width: *width,
        },
        super::THardwareCall::RegisterWrite {
            profile_id,
            block,
            register,
            width,
        } => MirHardwareOp::RegisterWrite {
            profile_id: profile_id.clone(),
            block: block.clone(),
            register: register.clone(),
            width: *width,
        },
        super::THardwareCall::DmaStart {
            profile_id,
            channel,
            buffer_ty,
        } => MirHardwareOp::DmaStart {
            profile_id: profile_id.clone(),
            channel: channel.clone(),
            buffer_ty: ctx.mir_type(buffer_ty)?,
        },
        super::THardwareCall::DmaWait {
            profile_id,
            channel,
            buffer_ty,
        } => MirHardwareOp::DmaWait {
            profile_id: profile_id.clone(),
            channel: channel.clone(),
            buffer_ty: ctx.mir_type(buffer_ty)?,
        },
    };
    Ok(op)
}
fn lower_mir_types(
    ctx: &mut LowerCtx,
    types: &[crate::AST::Type],
) -> Result<Vec<MirType>, LowerError> {
    types
        .iter()
        .map(|ty| ctx.mir_type(ty))
        .collect()
}

fn lower_require_stop(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    kind: &super::TRequireKind,
    always_stops: bool,
    loc: &super::TPanicLoc,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let (kind, condition, values) = match kind {
        super::TRequireKind::Require { cond, msg } => {
            let condition = ctx.lower_child(cond)?;
            let mut values = Vec::new();
            if let Some(msg) = msg.as_deref() {
                values.push(ctx.lower_child(msg)?);
            }
            (MirRequireKind::Require, Some(condition), values)
        }
        super::TRequireKind::RequireEq { left, right } => {
            let left_ty = left.ty.clone();
            let right_ty = right.ty.clone();
            let left = ctx.lower_child(left)?;
            let right = ctx.lower_child(right)?;
            let route = super::require_eq_condition_route(&left_ty, &right_ty)?;
            let call = ctx.intern_prelude_route(route)?;
            let condition = ctx.emit(
                "require-eq-condition",
                Some(crate::AST::Type::Bool),
                MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
                    call,
                    args: vec![mir_value_arg(ctx, left), mir_value_arg(ctx, right)],
                    owner_type_args: Vec::new(),
                    type_args: Vec::new(),
                }),
            )?;
            (MirRequireKind::RequireEq, Some(condition), vec![left, right])
        }
        super::TRequireKind::Panic { msg } => (
            MirRequireKind::Panic,
            None,
            vec![ctx.lower_child(msg)?],
        ),
    };
    let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
    let kind_key = match kind {
        MirRequireKind::Require => "require",
        MirRequireKind::RequireEq => "require_eq",
        MirRequireKind::Panic => "panic",
    };
    let call = ctx.intern_prelude_route(super::require_stop_route(
        kind_key,
        values.len(),
        &expr.ty,
        &carrier,
    )?)?;
    let location = MirPanicLoc {
        file: ctx.source_file_id_for(&loc.file),
        line: loc.line,
        column: loc.col,
    };
    let context = MirPanicContext {
        function: loc.fn_name.clone(),
        source_line: loc.src_line.clone(),
        caret: loc.caret,
        locals: loc
            .locals
            .iter()
            .map(|(name, local)| Ok((name.clone(), ctx.local_id_for(local)?)))
            .collect::<Result<Vec<_>, LowerError>>()?,
    };
    ctx.emit(
        "require-stop",
        Some(expr.ty.clone()),
        MirOperation::Semantic(MirSemanticOp::RequireStop {
            call,
            kind,
            condition,
            location,
            context,
            values,
            always_stops,
        }),
    )
}

fn lower_optional_field(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    base: &TExpr,
    member: &str,
    flatten: bool,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let inner_ty = match &base.ty {
        crate::AST::Type::Option(inner) => inner.as_ref(),
        _ => return unsupported_expr(ctx, "TExprKind::OptField (non-optional base)"),
    };
    let field = ctx.field_id_for_type(inner_ty, member)?;
    let field_ty = ctx
        .checked_field_type(inner_ty, member)
        .map_err(|_| ctx.error(ctx.span(), format!("missing checked optional field `{member}`")))?;
    let subject = ctx.lower_child(base)?;
    let condition = ctx.emit(
        "optional-field-condition",
        Some(crate::AST::Type::Bool),
        MirOperation::OptionIsSome { subject },
    )?;
    let present_block = ctx.new_block(ctx.span(), "optional-field-present")?;
    let absent_block = ctx.new_block(ctx.span(), "optional-field-absent")?;
    let join = ctx.new_block(ctx.span(), "optional-field-join")?;
    ctx.terminate(MirTerminator::Branch {
        condition,
        then_target: present_block,
        else_target: absent_block,
    });

    let mut incoming = Vec::with_capacity(2);
    ctx.switch_to(present_block);
    let unwrapped = ctx.emit(
        "optional-field-value",
        Some(inner_ty.clone()),
        MirOperation::OptionValue { subject },
    )?;
    let selected = ctx.emit(
        "optional-field-member",
        Some(field_ty.clone()),
        MirOperation::Field {
            base: unwrapped,
            field,
        },
    )?;
    let present = if flatten {
        selected
    } else {
        ctx.emit(
            "optional-field-present-wrap",
            Some(expr.ty.clone()),
            MirOperation::Present { value: selected },
        )?
    };
    let source = ctx.current_block();
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
        incoming.push((source, present));
    }

    ctx.switch_to(absent_block);
    let absent = ctx.emit("optional-field-absent-value", Some(expr.ty.clone()), MirOperation::Absent)?;
    let source = ctx.current_block();
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
        incoming.push((source, absent));
    }

    if incoming.is_empty() {
        return unsupported_expr(ctx, "TExprKind::OptField (no reachable branch)");
    }
    ctx.switch_to(join);
    ctx.emit(
        "optional-field-phi",
        Some(expr.ty.clone()),
        MirOperation::Phi { incoming },
    )
}

fn panic_location(ctx: &mut LowerCtx, line: usize) -> MirPanicLoc {
    let file = ctx.function.source_file.clone();
    MirPanicLoc {
        file: ctx.source_file_id_for(&file),
        line: line as u32,

        column: 1,
    }
}

fn lower_index_hook(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    _type_name: &str,
    base: &TExpr,
    index: &TExpr,
    line: usize,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    // Index reads are ordinary user trait calls. Build the same resolved
    // MethodCall shape used by source-level dispatch so the concrete
    // `Grid::Index::get` identity and receiver convention stay intact.
    let option_ty = crate::AST::Type::Option(Box::new(expr.ty.clone()));
    let call_expr = TExpr {
        ty: option_ty.clone(),
        kind: TExprKind::MethodCall {
            recv: Box::new(base.clone()),
            method: TMethodRef::trait_method(crate::Syntax::TRAIT_INDEX, "get"),
            type_args: Vec::new(),
            args: vec![TCallArg {
                value: index.clone(),
                template_items: None,
                borrow: !index.ty.is_scalar(),
                mut_borrow: false,
                clone: false,
                arc_clone: false,
                fn_coerce: None,
                widen_to_vec: false,
                widen_to_union: None,
                box_as_trait: None,
            }],
            source_first_string_literal: None,
            operator_line: None,
        },
    };
    let call = lower_expr(ctx, &call_expr)?;
    let success_block = ctx.new_block(ctx.span(), "index-hook-success")?;
    let failure_block = ctx.new_block(ctx.span(), "index-hook-failure")?;
    let join = ctx.new_block(ctx.span(), "index-hook-join")?;
    let condition = ctx.emit(
        "index-hook-condition",
        Some(crate::AST::Type::Bool),
        MirOperation::OptionIsSome { subject: call },
    )?;
    ctx.terminate(MirTerminator::Branch {
        condition,
        then_target: success_block,
        else_target: failure_block,
    });
    let mut incoming = Vec::with_capacity(1);
    ctx.switch_to(success_block);
    let value = ctx.emit(
        "index-hook-value",
        Some(expr.ty.clone()),
        MirOperation::OptionValue { subject: call },
    )?;
    let source = ctx.current_block();
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
        incoming.push((source, value));
    }
    ctx.switch_to(failure_block);
    ctx.lower_index_hook_failure(line)?;
    if incoming.is_empty() {
        return unsupported_expr(ctx, "TExprKind::IndexHook (no reachable success branch)");
    }
    ctx.switch_to(join);
    ctx.emit(
        "index-hook-phi",
        Some(expr.ty.clone()),
        MirOperation::Phi { incoming },
    )
}


fn lower_inline_block(
    ctx: &mut LowerCtx,
    stmts: &[super::TStmt],
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let Some((tail, prefix)) = stmts.split_last() else {
        return unsupported_expr(ctx, "TExprKind::InlineBlock (empty)");
    };
    ctx.lower_nested_stmts(prefix)?;
    match tail {
        super::TStmt::ExprStmt(value) => ctx.lower_child(value),
        _ => unsupported_expr(ctx, "TExprKind::InlineBlock (non-expression tail)"),
    }
}

fn lower_inc_dec(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    op: &crate::AST::IncDecOp,
    place: &TPlace,
    postfix: bool,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let place_id = ctx.lower_place(place, MirAccess::Write)?;
    let old = ctx.emit(
        "inc-dec-read",
        Some(expr.ty.clone()),
        MirOperation::ReadPlace(place_id),
    )?;
    let one = ctx.emit(
        "inc-dec-one",
        Some(expr.ty.clone()),
        MirOperation::Constant(MirConstant::Int {
            value: 1,
            width: None,
            spelling: None,
        }),
    )?;
    let binary_op = match op {
        crate::AST::IncDecOp::Inc => crate::AST::BinOp::Add,
        crate::AST::IncDecOp::Dec => crate::AST::BinOp::Sub,
    };
    let next = ctx.emit(
        "inc-dec-update",
        Some(expr.ty.clone()),
        MirOperation::Binary {
            op: super::mir_binary_op(binary_op),
            left: old,
            right: one,
            dispatch: MirBinaryDispatch::Primitive,
        },
    )?;
    ctx.emit(
        "inc-dec-write",
        None,
        MirOperation::WritePlace {
            place: place_id,
            value: next,
        },
    )?;
    Ok(if postfix { old } else { next })
}

fn lower_compare_chain(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    operands: &[TExpr],
    ops: &[crate::AST::BinOp],
    hooks: &[bool],
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    if operands.len() != ops.len().saturating_add(1) || operands.is_empty() {
        return unsupported_expr(ctx, "TExprKind::CompareChain (invalid arity)");
    }
    let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
    let false_block = ctx.new_block(ctx.span(), "compare-chain-false")?;
    let true_block = ctx.new_block(ctx.span(), "compare-chain-true")?;
    let join = ctx.new_block(ctx.span(), "compare-chain-join")?;
    let mut left = ctx.lower_child(&operands[0])?;

    for (index, op) in ops.iter().enumerate() {
        let right = ctx.lower_child(&operands[index + 1])?;
        let hook = hooks.get(index).copied().unwrap_or(false);
        let comparison = match super::compare_chain_route(
            *op,
            hook,
            &operands[index].ty,
            &expr.ty,
            &carrier,
        )? {
            super::TRoutePlan::Primitive => ctx.emit(
                "compare-chain-link",
                Some(crate::AST::Type::Bool),
                MirOperation::Binary {
                    op: super::mir_binary_op(*op),
                    left,
                    right,
                    dispatch: MirBinaryDispatch::Primitive,
                },
            )?,
            super::TRoutePlan::Prelude(route) => {
                let call = ctx.intern_prelude_route(route)?;
                ctx.emit(
                    "compare-chain-link",
                    Some(crate::AST::Type::Bool),
                    MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
                        call,
                        args: vec![mir_value_arg(ctx, left), mir_value_arg(ctx, right)],
                        owner_type_args: Vec::new(),
                        type_args: Vec::new(),
                    }),
                )?
            }
        };
        let next = if index + 1 == ops.len() {
            true_block
        } else {
            ctx.new_block(ctx.span(), "compare-chain-next")?
        };
        ctx.terminate(MirTerminator::Branch {
            condition: comparison,
            then_target: next,
            else_target: false_block,
        });
        if index + 1 != ops.len() {
            ctx.switch_to(next);
            left = right;
        }
    }

    let mut incoming = Vec::with_capacity(2);
    ctx.switch_to(true_block);
    let true_value = ctx.emit(
        "compare-chain-true-value",
        Some(crate::AST::Type::Bool),
        MirOperation::Constant(MirConstant::Bool(true)),
    )?;
    let true_source = ctx.current_block();
    ctx.terminate(MirTerminator::Jump { target: join });
    incoming.push((true_source, true_value));

    ctx.switch_to(false_block);
    let false_value = ctx.emit(
        "compare-chain-false-value",
        Some(crate::AST::Type::Bool),
        MirOperation::Constant(MirConstant::Bool(false)),
    )?;
    let false_source = ctx.current_block();
    ctx.terminate(MirTerminator::Jump { target: join });
    incoming.push((false_source, false_value));

    ctx.switch_to(join);
    ctx.emit(
        "compare-chain-phi",
        Some(expr.ty.clone()),
        MirOperation::Phi { incoming },
    )
}
fn lower_if_expr(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    cond: &super::TIfCond,
    then_body: &[super::TStmt],
    then_value: &TExpr,
    else_body: &[super::TStmt],
    else_value: &TExpr,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let then_block = ctx.new_block(ctx.span(), "if-then")?;
    let else_block = ctx.new_block(ctx.span(), "if-else")?;
    let join = ctx.new_block(ctx.span(), "if-join")?;

    lower_if_cond(ctx, cond, then_block, else_block)?;

    let mut incoming = Vec::with_capacity(2);

    ctx.switch_to(then_block);
    ctx.lower_nested_stmts(then_body)?;
    if !ctx.is_terminated() {
        let value = ctx.lower_child(then_value)?;
        let source = ctx.current_block();
        if !ctx.is_terminated() {
            ctx.terminate(MirTerminator::Jump { target: join });
            incoming.push((source, value));
        }
    }

    ctx.switch_to(else_block);
    ctx.lower_nested_stmts(else_body)?;
    if !ctx.is_terminated() {
        let value = ctx.lower_child(else_value)?;
        let source = ctx.current_block();
        if !ctx.is_terminated() {
            ctx.terminate(MirTerminator::Jump { target: join });
            incoming.push((source, value));
        }
    }

    if incoming.is_empty() {
        ctx.block_mut(join)?.terminator = MirTerminator::Unreachable {
            reason: "every conditional arm diverges".to_string(),
        };
        return ctx.emit(
            "diverging-conditional",
            Some(Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string())),
            MirOperation::Constant(MirConstant::Unit),
        );
    }
    ctx.switch_to(join);
    ctx.emit(
        "if-phi",
        Some(expr.ty.clone()),
        MirOperation::Phi { incoming },
    )
}
fn try_child_already_propagated(inner: &TExpr) -> bool {
    match &inner.kind {
        TExprKind::ModuleCall {
            target_return: Some(target),
            ..
        } => {
            matches!(target, Type::Result { .. } | Type::Option(_)) && target != &inner.ty
        }
        TExprKind::MethodCall { recv, .. } => {
            matches!(
                recv.ty.without_user_tags(),
                Type::Named(name)
                    if name.ends_with(".Client") || name.ends_with(".Server")
            )
        }
        _ => false,
    }
}
fn checked_failure_return_type(ctx: &LowerCtx, carrier: &TFailureCarrier) -> Option<Type> {
    ctx.function.ret.clone().or_else(|| match carrier {
        TFailureCarrier::Result { success, error } => Some(Type::Result {
            ok: Box::new(success.clone()),
            err: Box::new(error.clone()),
        }),
        TFailureCarrier::Optional { value } => Some(Type::Option(Box::new(value.clone()))),
        TFailureCarrier::Diverges { value } => Some(value.clone()),
        TFailureCarrier::Infallible => None,
    })
}



fn lower_if_cond(
    ctx: &mut LowerCtx,
    cond: &super::TIfCond,
    then_target: MirBlockId,
    else_target: MirBlockId,
) -> Result<(), LowerError> {
    match cond {
        super::TIfCond::Plain(expr) => {
            let condition = ctx.lower_child(expr)?;
            ctx.terminate(MirTerminator::Branch {
                condition,
                then_target,
                else_target,
            });
        }
        super::TIfCond::And { left, right } => {
            let right_block = ctx.new_block(ctx.span(), "if-and-right")?;
            lower_if_cond(ctx, left, right_block, else_target)?;
            ctx.switch_to(right_block);
            lower_if_cond(ctx, right, then_target, else_target)?;
        }
        super::TIfCond::IfLet { pattern, subj } => {
            let subject = ctx.lower_child(subj)?;
            let pattern = lower_pattern(ctx, pattern)?;
            let condition = ctx.lower_pattern_condition(subject, &pattern)?;
            ctx.terminate(MirTerminator::Branch {
                condition,
                then_target,
                else_target,
            });
        }
        super::TIfCond::IsNone { subj } => {
            let subject = ctx.lower_child(subj)?;
            let pattern = MirPattern {
                shape: MirPatternShape::Absent(ctx.span()),
                owner: None,
                position: MirPatternPosition::OptionBinding,
                mutable: false,
                boxed: false,
            };
            let condition = ctx.lower_pattern_condition(subject, &pattern)?;
            ctx.terminate(MirTerminator::Branch {
                condition,
                then_target,
                else_target,
            });
        }
        super::TIfCond::Matches { pattern, subj } => {
            let subject = ctx.lower_child(subj)?;
            let pattern = lower_pattern(ctx, pattern)?;
            let condition = ctx.lower_pattern_condition(subject, &pattern)?;
            ctx.terminate(MirTerminator::Branch {
                condition,
                then_target,
                else_target,
            });
        }
        super::TIfCond::WithPrelude { prelude, cond } => {
            ctx.lower_nested_stmts(prelude)?;
            if !ctx.is_terminated() {
                lower_if_cond(ctx, cond, then_target, else_target)?;
            }
        }
    }
    Ok(())
}

fn lower_try_value(
    ctx: &mut LowerCtx,
    input: jet_foundation::MIR::MirValueId,
    result: &Type,
    input_ty: &Type,
    note: Option<&TExpr>,
    convert: &TTryConvert,
    location: Option<(&str, usize, &str)>,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let is_result = matches!(input_ty, crate::AST::Type::Result { .. });
    let is_option = matches!(input_ty, crate::AST::Type::Option(_));
    if !is_result && !is_option {
        return unsupported_expr(ctx, "TExprKind::Try (non-carrier operand)");
    }
    let success_block = ctx.new_block(ctx.span(), "try-success")?;
    let failure_block = ctx.new_block(ctx.span(), "try-failure")?;
    let join = ctx.new_block(ctx.span(), "try-join")?;
    let mut incoming = Vec::with_capacity(2);

    let condition = if is_result {
        ctx.emit(
            "try-result-condition",
            Some(crate::AST::Type::Bool),
            MirOperation::ResultIsOk { subject: input },
        )?
    } else {
        ctx.emit(
            "try-option-condition",
            Some(crate::AST::Type::Bool),
            MirOperation::OptionIsSome { subject: input },
        )?
    };
    ctx.terminate(MirTerminator::Branch {
        condition,
        then_target: success_block,
        else_target: failure_block,
    });

    ctx.switch_to(success_block);
    if location.is_some() {
        let reset_route = super::journey_reset_route()?;
        let unit_ty = Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string());
        let _ = emit_static_call(ctx, &unit_ty, reset_route, Vec::new())?;
    }
    let success = if is_result {
        ctx.emit(
            "try-result-value",
            Some(result.clone()),
            MirOperation::ResultValue {
                subject: input,
                ok: true,
            },
        )?
    } else {
        ctx.emit(
            "try-option-value",
            Some(result.clone()),
            MirOperation::OptionValue { subject: input },
        )?
    };
    let success_source = ctx.current_block();
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
        incoming.push((success_source, success));
    }

    ctx.switch_to(failure_block);
    let carrier = ctx.function.failure_carrier.clone();
    match &carrier {
        super::TFailureCarrier::Result { .. } => {
            if matches!(convert, TTryConvert::Never) {
                ctx.terminate(MirTerminator::Unreachable {
                    reason: "try conversion is unreachable".to_string(),
                });
            } else {
                let error = ctx.emit(
                    "try-result-error-value",
                    Some(
                        input_ty
                            .unwrap_result()
                            .map(|(_, error)| error.clone())
                            .ok_or_else(|| {
                                ctx.error(ctx.span(), "checked try result has no error type")
                            })?,
                    ),
                    MirOperation::ResultValue {
                        subject: input,
                        ok: false,
                    },
                )?;
                if matches!(convert, TTryConvert::ProtocolExit) {
                    let route = super::try_conversion_route(
                        convert,
                        input_ty,
                        result,
                        &carrier,
                    )?;
                    let never_ty = Type::Named(crate::Syntax::TYPE_NEVER.to_string());
                    let _ = emit_static_call(ctx, &never_ty, route, vec![error])?;
                    ctx.terminate(MirTerminator::Unreachable {
                        reason: "protocol error exit diverges".to_string(),
                    });
                } else {
                    let note_value = note.map(|value| ctx.lower_child(value)).transpose()?;
                    let converted = lower_try_failure(
                        ctx,
                        error,
                        input_ty,
                        convert,
                        note_value,
                        result,
                        location,
                        &carrier,
                    )?;
                    let return_ty = checked_failure_return_type(ctx, &carrier).ok_or_else(|| {
                        ctx.error(ctx.span(), "checked try has no result return type")
                    })?;
                    let failure = ctx.emit(
                        "try-result-error",
                        Some(return_ty),
                        MirOperation::ResultErr { value: converted },
                    )?;
                    ctx.terminate(MirTerminator::Return {
                        value: Some(failure),
                    });
                }
            }
        }
        super::TFailureCarrier::Optional { .. } => {
            if matches!(convert, TTryConvert::Never) {
                ctx.terminate(MirTerminator::Unreachable {
                    reason: "try conversion is unreachable".to_string(),
                });
            } else if !matches!(convert, TTryConvert::None) {
                return unsupported_expr(ctx, "TExprKind::Try (optional conversion)");
            } else {
                let note_value = note.map(|value| ctx.lower_child(value)).transpose()?;
                if let Some((file, line, fn_name)) = location {
                    lower_try_journey(
                        ctx,
                        result,
                        &carrier,
                        note_value,
                        file,
                        line,
                        fn_name,
                    )?;
                }
                let return_ty = checked_failure_return_type(ctx, &carrier).ok_or_else(|| {
                    ctx.error(ctx.span(), "checked try has no optional return type")
                })?;
                let failure = ctx.emit("try-optional-absent", Some(return_ty), MirOperation::Absent)?;
                ctx.terminate(MirTerminator::Return {
                    value: Some(failure),
                });
            }
        }
        super::TFailureCarrier::Diverges { .. } => {
            if matches!(convert, TTryConvert::Never) {
                ctx.terminate(MirTerminator::Unreachable {
                    reason: "try conversion is unreachable".to_string(),
                });
            } else if matches!(convert, TTryConvert::ProtocolExit) && is_result {
                let error = ctx.emit(
                    "try-result-error-value",
                    Some(
                        input_ty
                            .unwrap_result()
                            .map(|(_, error)| error.clone())
                            .ok_or_else(|| {
                                ctx.error(ctx.span(), "checked try result has no error type")
                            })?,
                    ),
                    MirOperation::ResultValue {
                        subject: input,
                        ok: false,
                    },
                )?;
                let route = super::try_conversion_route(
                    convert,
                    input_ty,
                    result,
                    &carrier,
                )?;
                let never_ty = Type::Named(crate::Syntax::TYPE_NEVER.to_string());
                let _ = emit_static_call(ctx, &never_ty, route, vec![error])?;
                ctx.terminate(MirTerminator::Unreachable {
                    reason: "protocol error exit diverges".to_string(),
                });
            } else {
                return unsupported_expr(ctx, "TExprKind::Try (diverging carrier)");
            }
        }
        super::TFailureCarrier::Infallible => {
            ctx.switch_to(failure_block);
            ctx.terminate(MirTerminator::Unreachable {
                reason: "try in an infallible function diverges on failure".to_string(),
            });
        }
    }

    if incoming.is_empty() {
        return unsupported_expr(ctx, "TExprKind::Try (no reachable success)");
    }
    ctx.switch_to(join);
    ctx.emit(
        "try-phi",
        Some(result.clone()),
        MirOperation::Phi { incoming },
    )
}

fn decode_try_location(value: &str) -> String {
    let value = value.strip_prefix('"').unwrap_or(value);
    let value = value.strip_suffix('"').unwrap_or(value);
    let mut decoded = String::with_capacity(value.len());
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            if matches!(ch, '\\' | '"') {
                decoded.push(ch);
            } else {
                decoded.push('\\');
                decoded.push(ch);
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            decoded.push(ch);
        }
    }
    if escaped {
        decoded.push('\\');
    }
    decoded
}

fn lower_try_context_values(
    ctx: &mut LowerCtx,
    file: &str,
    line: usize,
    fn_name: &str,
) -> Result<Vec<jet_foundation::MIR::MirValueId>, LowerError> {
    let line = u32::try_from(line)
        .map_err(|_| ctx.error(ctx.span(), "checked try source line does not fit u32"))?;
    let file_value = ctx.emit(
        "try-context-file",
        Some(Type::String),
        MirOperation::Constant(MirConstant::String(decode_try_location(file))),
    )?;
    let line_value = ctx.emit(
        "try-context-line",
        Some(Type::IntN {
            signed: false,
            bits: 32,
        }),
        MirOperation::Constant(MirConstant::Int {
            value: i64::from(line),
            width: Some((false, 32)),
            spelling: None,
        }),
    )?;
    let fn_value = ctx.emit(
        "try-context-function",
        Some(Type::String),
        MirOperation::Constant(MirConstant::String(decode_try_location(fn_name))),
    )?;
    Ok(vec![file_value, line_value, fn_value])
}

fn lower_try_journey(
    ctx: &mut LowerCtx,
    result: &Type,
    carrier: &super::TFailureCarrier,
    note: Option<jet_foundation::MIR::MirValueId>,
    file: &str,
    line: usize,
    fn_name: &str,
) -> Result<(), LowerError> {
    let mut values = lower_try_context_values(ctx, file, line, fn_name)?;
    let note = match note {
        Some(note) => note,
        None => ctx.emit(
            "try-context-empty-note",
            Some(Type::String),
            MirOperation::Constant(MirConstant::String(String::new())),
        )?,
    };
    values.push(note);
    let route = super::failure_note_route(result, carrier)?;
    let unit_ty = Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string());
    let _ = emit_static_call(ctx, &unit_ty, route, values)?;
    Ok(())
}

fn lower_try_failure(
    ctx: &mut LowerCtx,
    error: jet_foundation::MIR::MirValueId,
    input_ty: &Type,
    convert: &TTryConvert,
    note: Option<jet_foundation::MIR::MirValueId>,
    result: &Type,
    location: Option<(&str, usize, &str)>,
    carrier: &super::TFailureCarrier,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let converted = match convert {
        TTryConvert::None => error,
        TTryConvert::DefaultErr => {
            let route = super::try_conversion_route(convert, input_ty, result, carrier)?;
            let target = Type::Named(crate::Syntax::TYPE_ERR.to_string());
            emit_static_call(ctx, &target, route, vec![error])?
        }
        TTryConvert::Typed {
            fn_name: conversion_fn,
            target,
            ..
        } => {
            let function = ctx.function_id_for(conversion_fn)?;
            ctx.emit(
                "try-typed-error-conversion",
                Some(target.clone()),
                MirOperation::Call {
                    callee: MirCallee::User(function),
                    args: vec![mir_value_arg_with_access(ctx, error, MirAccess::Move)],
                    type_args: Vec::new(),
                },
            )?
        }
        TTryConvert::WidenUnion { enum_name, tag } => {
            let type_id = ctx.type_id_for(enum_name)?;
            ctx.emit(
                "try-union-error-conversion",
                Some(Type::Named(enum_name.clone())),
                MirOperation::Enum {
                    type_id,
                    variant: tag.clone(),
                    args: vec![MirEnumArg {
                        field: None,
                        value: error,
                        boxed: false,
                    }],
                },
            )?
        }
        TTryConvert::Never | TTryConvert::ProtocolExit => {
            return unsupported_expr(ctx, "TExprKind::Try (diverging failure conversion)");
        }
    };

    let Some((file, line, fn_name)) = location else {
        return Ok(converted);
    };
    if super::try_target_is_default_error(input_ty, convert) {
        if let Some(note) = note {
            let mut values = vec![converted];
            values.extend(lower_try_context_values(ctx, file, line, fn_name)?);
            values.push(note);
            let route = super::failure_context_route(result, carrier)?;
            let target = Type::Named(crate::Syntax::TYPE_ERR.to_string());
            return emit_static_call(ctx, &target, route, values);
        }
    }
    lower_try_journey(ctx, result, carrier, note, file, line, fn_name)?;
    Ok(converted)
}

fn lower_or_fallback_expr(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    value: &TExpr,
    fallback: &super::TOrFallback,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let value_id = ctx.lower_child(value)?;
    let carrier = match &value.ty {
        crate::AST::Type::Option(_) => "option",
        crate::AST::Type::Result { .. } => "result",
        _ => return unsupported_expr(ctx, &format!(
            "TExprKind::OrFallback (non-carrier operand type {:?}, operation {:?})",
            value.ty, std::mem::discriminant(&value.kind),
        )),
    };
    let success_block = ctx.new_block(ctx.span(), "or-fallback-success")?;
    let failure_block = ctx.new_block(ctx.span(), "or-fallback-failure")?;
    let join = ctx.new_block(ctx.span(), "or-fallback-join")?;
    let condition = if carrier == "option" {
        ctx.emit(
            "or-fallback-option-condition",
            Some(crate::AST::Type::Bool),
            MirOperation::OptionIsSome { subject: value_id },
        )?
    } else {
        ctx.emit(
            "or-fallback-result-condition",
            Some(crate::AST::Type::Bool),
            MirOperation::ResultIsOk { subject: value_id },
        )?
    };
    ctx.terminate(MirTerminator::Branch {
        condition,
        then_target: success_block,
        else_target: failure_block,
    });

    let mut incoming = Vec::with_capacity(2);
    ctx.switch_to(success_block);
    let success = if carrier == "option" {
        ctx.emit(
            "or-fallback-option-value",
            Some(expr.ty.clone()),
            MirOperation::OptionValue { subject: value_id },
        )?
    } else {
        ctx.emit(
            "or-fallback-result-value",
            Some(expr.ty.clone()),
            MirOperation::ResultValue {
                subject: value_id,
                ok: true,
            },
        )?
    };
    let source = ctx.current_block();
    if !ctx.is_terminated() {
        ctx.terminate(MirTerminator::Jump { target: join });
        incoming.push((source, success));
    }

    ctx.switch_to(failure_block);
    let error_binding = if let crate::AST::Type::Result { err, .. } = &value.ty {
        let local = super::ambient_err_local();
        let previous_type = ctx.local_types.remove(&local.name);
        let previous_place = ctx.local_places.remove(&local.name);
        let previous_value = ctx.local_values.remove(&local.name);
        let error = ctx.emit(
            "or-fallback-error-value",
            Some((**err).clone()),
            MirOperation::ResultValue { subject: value_id, ok: false },
        )?;
        let place = ctx.bind_local(&local, (**err).clone(), false, false, false)?;
        ctx.emit(
            "or-fallback-error-binding",
            None,
            MirOperation::WritePlace { place, value: error },
        )?;
        Some((local.name, previous_type, previous_place, previous_value))
    } else {
        None
    };
    let fallback_value = lower_fallback_block(ctx, expr, fallback)?;
    if let Some((name, previous_type, previous_place, previous_value)) = error_binding {
        ctx.local_types.remove(&name);
        ctx.local_places.remove(&name);
        ctx.local_values.remove(&name);
        if let Some(ty) = previous_type {
            ctx.local_types.insert(name.clone(), ty);
        }
        if let Some(place) = previous_place {
            ctx.local_places.insert(name.clone(), place);
        }
        if let Some(value) = previous_value {
            ctx.local_values.insert(name, value);
        }
    }
    if let Some(fallback_value) = fallback_value {
        let source = ctx.current_block();
        if !ctx.is_terminated() {
            ctx.terminate(MirTerminator::Jump { target: join });
            incoming.push((source, fallback_value));
        }
    }
    if incoming.is_empty() {
        return unsupported_expr(ctx, "TExprKind::OrFallback (no reachable value)");
    }
    ctx.switch_to(join);
    ctx.emit(
        "or-fallback-phi",
        Some(expr.ty.clone()),
        MirOperation::Phi { incoming },
    )
}

fn lower_fallback_block(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    fallback: &super::TOrFallback,
) -> Result<Option<jet_foundation::MIR::MirValueId>, LowerError> {
    match fallback {
        super::TOrFallback::Value(value) => {
            if let TExprKind::InlineBlock(stmts) = &value.kind {
                if matches!(
                    stmts.last(),
                    Some(
                        super::TStmt::Return(_)
                            | super::TStmt::Break(_)
                            | super::TStmt::BreakValue { .. }
                            | super::TStmt::Continue(_)
                    )
                ) {
                    ctx.lower_nested_stmts(stmts)?;
                    return Ok(None);
                }
            }
            Ok(Some(ctx.lower_child(value)?))
        }
        super::TOrFallback::Return(value) => {
            let value = value
                .as_deref()
                .map(|value| ctx.lower_child(value))
                .transpose()?;
            ctx.terminate(MirTerminator::Return { value });
            Ok(None)
        }
        super::TOrFallback::Panic { msg, loc } => {
            let values = vec![ctx.lower_child(msg)?];
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let call = ctx.intern_prelude_route(super::require_stop_route(
                "panic",
                values.len(),
                &expr.ty,
                &carrier,
            )?)?;
            let location = MirPanicLoc {
                file: ctx.source_file_id_for(&loc.file),
                line: loc.line,
                column: loc.col,
            };
            let context = MirPanicContext {
                function: loc.fn_name.clone(),
                source_line: loc.src_line.clone(),
                caret: loc.caret,
                locals: loc
                    .locals
                    .iter()
                    .map(|(name, local)| Ok((name.clone(), ctx.local_id_for(local)?)))
                    .collect::<Result<Vec<_>, LowerError>>()?,
            };
            ctx.emit(
                "or-fallback-panic",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::RequireStop {
                    call,
                    kind: MirRequireKind::Panic,
                    condition: None,
                    location,
                    context,
                    values,
                    always_stops: true,
                }),
            )?;
            ctx.terminate(MirTerminator::Unreachable {
                reason: "or-fallback panic".to_string(),
            });
            Ok(None)
        }
        super::TOrFallback::Break => {
            let target = ctx
                .loops
                .last()
                .map(|(_, break_target, _)| *break_target)
                .ok_or_else(|| ctx.error(ctx.span(), "fallback break outside loop"))?;
            ctx.terminate(MirTerminator::Jump { target });
            Ok(None)
        }
        super::TOrFallback::Continue => {
            let target = ctx
                .loops
                .last()
                .map(|(_, _, continue_target)| *continue_target)
                .ok_or_else(|| ctx.error(ctx.span(), "fallback continue outside loop"))?;
            ctx.terminate(MirTerminator::Jump { target });
            Ok(None)
        }
        super::TOrFallback::BreakLabel(label) => {
            let target = ctx
                .loops
                .iter()
                .rev()
                .find(|(name, _, _)| name.as_deref() == Some(label.as_str()))
                .map(|(_, break_target, _)| *break_target)
                .ok_or_else(|| ctx.error(ctx.span(), "fallback break label not found"))?;
            ctx.terminate(MirTerminator::Jump { target });
            Ok(None)
        }
        super::TOrFallback::ContinueLabel(label) => {
            let target = ctx
                .loops
                .iter()
                .rev()
                .find(|(name, _, _)| name.as_deref() == Some(label.as_str()))
                .map(|(_, _, continue_target)| *continue_target)
                .ok_or_else(|| ctx.error(ctx.span(), "fallback continue label not found"))?;
            ctx.terminate(MirTerminator::Jump { target });
            Ok(None)
        }
    }
}

fn lower_call_args(
    ctx: &mut LowerCtx,
    args: &[TCallArg],
) -> Result<Vec<jet_foundation::MIR::MirCallArg>, LowerError> {
    args.iter()
        .enumerate()
        .map(|(index, arg)| {
            let mut lowered = ctx.lower_call_arg(arg)?;
            lowered.source_index = Some(index);
            Ok(lowered)
        })
        .collect::<Result<Vec<_>, LowerError>>()
}

fn lower_extern_arg(
    ctx: &mut LowerCtx,
    arg: &super::TExternArg,
) -> Result<jet_foundation::MIR::MirCallArg, LowerError> {
    let mut lowered = ctx.lower_plain_arg(&arg.value)?;
    let resource_take = matches!(&arg.value.kind, super::TExprKind::ResourceTake(_));
    lowered.access = if arg.mut_borrow {
        MirAccess::Write
    } else if arg.clone || resource_take {
        MirAccess::Move
    } else {
        MirAccess::Read
    };
    lowered.owned_last_use = matches!(lowered.access, MirAccess::Move);
    lowered.implicit_clone = arg.clone;
    Ok(lowered)
}

fn lower_enum_payload(
    ctx: &mut LowerCtx,
    enum_type: &str,
    variant: &str,
    payload: &TEnumPayload,
) -> Result<Vec<MirEnumArg>, LowerError> {
    match payload {
        TEnumPayload::Unit => Ok(Vec::new()),
        TEnumPayload::Positional(args) => args
            .iter()
            .map(|arg| {
                Ok(MirEnumArg {
                    field: None,
                    value: lower_enum_arg(ctx, arg)?,
                    boxed: arg.boxed,
                })
            })
            .collect(),
        TEnumPayload::Named(args) => {
            let mut lowered = Vec::with_capacity(args.len());
            for (name, arg) in args {
                let (field, order) =
                    ctx.enum_named_payload_field(enum_type, variant, name)?;
                lowered.push((
                    order,
                    MirEnumArg {
                        field: Some(field),
                        value: lower_enum_arg(ctx, arg)?,
                        boxed: arg.boxed,
                    },
                ));
            }
            lowered.sort_by_key(|(order, _)| *order);
            if lowered
                .windows(2)
                .any(|pair| pair[0].0 == pair[1].0)
            {
                return Err(ctx.error(
                    ctx.span(),
                    format!("duplicate checked enum field in `{enum_type}::{variant}`"),
                ));
            }
            Ok(lowered.into_iter().map(|(_, arg)| arg).collect())
        }
    }
}


fn lower_enum_arg(
    ctx: &mut LowerCtx,
    arg: &TEnumArg,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    lower_cloned_value(ctx, &arg.value, arg.clone)
}
fn lower_conversion_int(
    ctx: &mut LowerCtx,
    value: i64,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    ctx.emit(
        "conversion-parameter-int",
        Some(Type::Int),
        MirOperation::Constant(MirConstant::Int {
            value,
            width: None,
            spelling: None,
        }),
    )
}
fn lower_conversion_float(
    ctx: &mut LowerCtx,
    value: f64,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    ctx.emit(
        "conversion-parameter-float",
        Some(Type::Float),
        MirOperation::Constant(MirConstant::Float {
            value,
            f32: false,
            spelling: None,
        }),
    )
}

fn lower_conversion_bool(
    ctx: &mut LowerCtx,
    value: bool,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    ctx.emit(
        "conversion-parameter-bool",
        Some(Type::Bool),
        MirOperation::Constant(MirConstant::Bool(value)),
    )
}

fn lower_conversion_string(
    ctx: &mut LowerCtx,
    value: String,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    ctx.emit(
        "conversion-parameter-string",
        Some(Type::String),
        MirOperation::Constant(MirConstant::String(value)),
    )
}

fn direct_prelude_route(
    module: &str,
    member: &str,
    symbol: &str,
    arity: usize,
    borrow_mask: &[bool],
    carrier: &super::TFailureCarrier,
) -> TPreludeRoute {
    TPreludeRoute {
        family: MirPreludeFamily::StaticPrelude,
        module: module.to_string(),
        member: member.to_string(),
        symbol: MirSymbol::Prelude(symbol.to_string()),
        signature: MirCallSignature {
            arity,
            max_arity: arity,
            borrow_mask: borrow_mask.to_vec(),
        },
        effect: None,
        fallibility: carrier.clone(),
        abi: MirPreludeAbi::Value,
        authority: None,
        db_metadata: None,
    }
}

fn task_group_result_tasks(ty: &Type) -> bool {
    fn task_result(ty: &Type) -> bool {
        matches!(
            ty.without_user_tags(),
            Type::Apply { name, args }
                if name == "Task"
                    && args.len() == 1
                    && matches!(args[0].without_user_tags(), Type::Result { .. })
        )
    }
    match ty.without_user_tags() {
        Type::List(inner) | Type::FixedList { elem: inner, .. } => task_result(inner),
        Type::Tuple(fields) => fields.iter().all(|(_, field)| task_result(field)),
        _ => false,
    }
}

fn task_group_route(
    kind: MirTaskGroupKind,
    tasks_ty: &Type,
    carrier: &super::TFailureCarrier,
) -> TPreludeRoute {
    let result = task_group_result_tasks(tasks_ty);
    let (name, symbol) = match (kind, result) {
        (MirTaskGroupKind::All, false) => ("all", "jet_std::jet_task_all"),
        (MirTaskGroupKind::Race, false) => ("race", "jet_std::jet_task_race"),
        (MirTaskGroupKind::Any, false) => ("any", "jet_std::jet_task_any"),
        (MirTaskGroupKind::All, true) => ("all_result", "jet_std::jet_task_all_result"),
        (MirTaskGroupKind::Race, true) => ("race_result", "jet_std::jet_task_race_result"),
        (MirTaskGroupKind::Any, true) => ("any_result", "jet_std::jet_task_any_result"),
    };
    direct_prelude_route("core.tasks", name, symbol, 1, &[false], carrier)
}

fn lower_select_builder(
    ctx: &mut LowerCtx,
    builder: &TExpr,
    receivers: &mut Vec<jet_foundation::MIR::MirValueId>,
    timers: &mut Vec<jet_foundation::MIR::MirValueId>,
) -> Result<(), LowerError> {
    match &builder.kind {
        TExprKind::SelectStart => Ok(()),
        TExprKind::SelectRecv { builder, channel } => {
            receivers.push(ctx.lower_child(channel)?);
            lower_select_builder(ctx, builder, receivers, timers)
        }
        TExprKind::SelectAfter {
            builder,
            duration,
            value,
        } => {
            if value.is_some() {
                return Err(ctx.error(
                    ctx.span(),
                    "readiness timer arm unexpectedly carries a value",
                ));
            }
            timers.push(ctx.lower_child(duration)?);
            lower_select_builder(ctx, builder, receivers, timers)
        }
        _ => Err(ctx.error(
            ctx.span(),
            "select wait has a non-canonical readiness-table builder",
        )),
    }
}

fn select_payload_type(ty: &Type) -> Type {
    match ty.without_user_tags() {
        Type::Result { ok, .. } => select_payload_type(ok),
        Type::Tuple(fields) => fields
            .iter()
            .find(|(name, _)| name == "value")
            .map(|(_, value)| select_payload_type(value))
            .unwrap_or(Type::Int),
        Type::Option(inner) => (**inner).clone(),
        _ => Type::Int,
    }
}

fn lower_select_wait(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    builder: &TExpr,
    nonblocking: bool,
    carrier: &super::TFailureCarrier,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let mut receivers = Vec::new();
    let mut timers = Vec::new();
    lower_select_builder(ctx, builder, &mut receivers, &mut timers)?;
    let payload = select_payload_type(&expr.ty);
    let receiver_ty = Type::Apply {
        name: "Receiver".to_string(),
        args: vec![payload],
    };
    let receiver_list = ctx.emit(
        "select-receivers",
        Some(Type::List(Box::new(receiver_ty))),
        MirOperation::BuildList { values: receivers },
    )?;
    let timer_list = ctx.emit(
        "select-timers",
        Some(Type::List(Box::new(Type::Int))),
        MirOperation::BuildList { values: timers },
    )?;
    let (member, symbol) = if nonblocking {
        ("select_try_wait_tagged", "jet_std::jet_select_try_wait_tagged")
    } else {
        ("select_wait_tagged", "jet_std::jet_select_wait_tagged")
    };
    let call = ctx.intern_prelude_route(direct_prelude_route(
        "core.tasks",
        member,
        symbol,
        2,
        &[true, false],
        carrier,
    ))?;
    ctx.emit(
        "select-wait",
        Some(expr.ty.clone()),
        MirOperation::Semantic(MirSemanticOp::Select {
            call,
            kind: MirSelectKind::Wait,
            values: vec![receiver_list, timer_list],
        }),
    )
}

fn lower_prelude_conversion(
    ctx: &mut LowerCtx,
    value: jet_foundation::MIR::MirValueId,
    parameters: Vec<jet_foundation::MIR::MirValueId>,
    target: &crate::AST::Type,
    route: super::TPreludeRoute,
    carrier: &super::TFailureCarrier,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let call = ctx.intern_prelude_route(route)?;
    let line = ctx.current_line.unwrap_or(ctx.function.line as u32) as usize;
    let location = panic_location(ctx, line);
    let fallibility = ctx.lower_call_fallibility(carrier)?;
    let target_type = ctx.mir_type(target)?;
    ctx.emit(
        "prelude-conversion",
        Some(target.clone()),
        MirOperation::Convert {
            value,
            parameters,
            target: target_type,
            conversion: MirConversion::Prelude {
                call,
                location,
                fallibility,
            },
        },
    )
}

fn lower_cloned_value(
    ctx: &mut LowerCtx,
    value: &TExpr,
    clone: bool,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let value_id = ctx.lower_child(value)?;
    Ok(if clone {
        ctx.emit(
            "cloned-value",
            Some(value.ty.clone()),
            MirOperation::Copy { value: value_id },
        )?
    } else {
        value_id
    })
}
fn lower_core_closure_call(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    kind: &TCoreClosureKind,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let site = ctx.site_id_for(ctx.span(), "core-closure");
    let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
    let mut values = Vec::new();
    let mut closure = None;
    let (mir_kind, grouped_spawn, label) = match kind {
        TCoreClosureKind::Spawn {
            group,
            site: spawn_site,
            label,
            executable,
            ..
        } => {
            if let Some(group) = group {
                values.push(ctx.lower_child(group)?);
            }
            let site_value = ctx.emit(
                "core-closure.spawn-site",
                Some(crate::AST::Type::Int),
                MirOperation::Constant(MirConstant::Int {
                    value: *spawn_site as i64,
                    width: None,
                    spelling: None,
                }),
            )?;
            let label_text = label.clone().unwrap_or_default();
            let label_value = ctx.emit(
                "core-closure.spawn-label",
                Some(crate::AST::Type::String),
                MirOperation::Constant(MirConstant::String(label_text.clone())),
            )?;
            values.extend([site_value, label_value]);
            closure = Some(ctx.lower_lambda(executable)?);
            (
                MirCoreClosureKind::Spawn,
                group.is_some(),
                label_text,
            )
        }
        TCoreClosureKind::Realtime {
            rate,
            frames,
            executable,
        } => {
            values.push(ctx.lower_child(rate)?);
            values.push(ctx.lower_child(frames)?);
            closure = Some(ctx.lower_lambda(executable)?);
            (MirCoreClosureKind::Realtime, false, String::new())
        }
        TCoreClosureKind::Serve {
            addr, executable, ..
        } => {
            values.push(ctx.lower_child(addr)?);
            closure = Some(ctx.lower_lambda_send(executable)?);
            (MirCoreClosureKind::Serve, false, String::new())
        }
        TCoreClosureKind::OnInterrupt { callback } => {
            values.push(ctx.lower_child(callback)?);
            (MirCoreClosureKind::OnInterrupt, false, String::new())
        }
        TCoreClosureKind::Guard { executable, .. } => {
            closure = Some(ctx.lower_lambda(executable)?);
            (MirCoreClosureKind::Guard, false, String::new())
        }
        TCoreClosureKind::OnCommit {
            handle_name,
            executable,
            ..
        } => {
            let handle_ty = ctx
                .local_types
                .get(handle_name)
                .cloned()
                .ok_or_else(|| {
                    ctx.error(
                        ctx.span(),
                        format!("missing checked transaction handle `{handle_name}`"),
                    )
                })?;
            values.push(ctx.lower_child(&TExpr {
                ty: handle_ty,
                kind: TExprKind::Local(TLocal::user(handle_name.clone())),
            })?);
            closure = Some(ctx.lower_lambda(executable)?);
            (MirCoreClosureKind::OnCommit, false, String::new())
        }
        TCoreClosureKind::OnRollback {
            handle_name,
            executable,
            ..
        } => {
            let handle_ty = ctx
                .local_types
                .get(handle_name)
                .cloned()
                .ok_or_else(|| {
                    ctx.error(
                        ctx.span(),
                        format!("missing checked transaction handle `{handle_name}`"),
                    )
                })?;
            values.push(ctx.lower_child(&TExpr {
                ty: handle_ty,
                kind: TExprKind::Local(TLocal::user(handle_name.clone())),
            })?);
            closure = Some(ctx.lower_lambda(executable)?);
            (MirCoreClosureKind::OnRollback, false, String::new())
        }
        TCoreClosureKind::ReactiveDerived { executable, .. } => {
            closure = Some(ctx.lower_lambda_send(executable)?);
            (MirCoreClosureKind::ReactiveDerived, false, String::new())
        }
        TCoreClosureKind::ReactiveEffect { executable, .. } => {
            closure = Some(ctx.lower_lambda_send(executable)?);
            (MirCoreClosureKind::ReactiveEffect, false, String::new())
        }
        TCoreClosureKind::UiReactiveRender { executable, .. } => {
            closure = Some(ctx.lower_lambda_send(executable)?);
            (MirCoreClosureKind::UiMount, false, String::new())
        }
        TCoreClosureKind::UiPreview {
            name,
            viewport,
            executable,
            playground,
            source_file,
            source_span,
            source_start_line,
            source_start_column,
            source_end_line,
            source_end_column,
            build_id,
            revision,
            ..
        } => {
            values.push(ctx.lower_child(name)?);
            if let Some(viewport) = viewport {
                values.push(ctx.lower_child(viewport)?);
            }
            closure = Some(ctx.lower_lambda_send(executable)?);
            (
                MirCoreClosureKind::UiPreview {
                    playground: *playground,
                    source_file: source_file.clone(),
                    source_span: *source_span,
                    source_start_line: *source_start_line,
                    source_start_column: *source_start_column,
                    source_end_line: *source_end_line,
                    source_end_column: *source_end_column,
                    build_id: build_id.clone(),
                    revision: revision.clone(),
                },
                false,
                String::new(),
            )
        }
        TCoreClosureKind::UiButtonOnClick {
            label,
            shortcut,
            accessible_label,
            executable,
            ..
        } => {
            values.push(ctx.lower_child(label)?);
            values.push(ctx.lower_child(shortcut)?);
            values.push(ctx.lower_child(accessible_label)?);
            closure = Some(ctx.lower_lambda_send(executable)?);
            (MirCoreClosureKind::UiAction, false, String::new())
        }
        TCoreClosureKind::UiTextInputOnDrop {
            state,
            ime,
            executable,
            ..
        } => {
            values.push(ctx.lower_child(state)?);
            values.push(ctx.lower_child(ime)?);
            closure = Some(ctx.lower_lambda_send(executable)?);
            (MirCoreClosureKind::UiTextInputOnDrop, false, String::new())
        }
    };
    let call = ctx.intern_prelude_route(super::core_closure_route(
        mir_kind.clone(),
        grouped_spawn,
        &expr.ty,
        &carrier,
    )?)?;
    ctx.emit(
        "core-closure-call",
        Some(expr.ty.clone()),
        MirOperation::Semantic(MirSemanticOp::CoreClosureCall {
            call,
            kind: mir_kind,
            values,
            closure,
            site,
            label,
        }),
    )
}
fn lower_tlocal_value(
    ctx: &mut LowerCtx,
    local: &TLocal,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let ty = ctx
        .local_types
        .get(&local.name)
        .cloned()
        .or_else(|| local.persist_ty.clone())
        .ok_or_else(|| {
            ctx.error(
                ctx.span(),
                format!("checked MIR local `{}` has no type", local.name),
            )
        })?;
    ctx.lower_child(&TExpr {
        ty,
        kind: TExprKind::Local(local.clone()),
    })
}

fn mir_gc_edit_kind(kind: TGcEditKind) -> jet_foundation::MIR::MirGcEditKind {
    match kind {
        TGcEditKind::Clear => jet_foundation::MIR::MirGcEditKind::Clear,
        TGcEditKind::Pop => jet_foundation::MIR::MirGcEditKind::Pop,
        TGcEditKind::RemoveIndex => jet_foundation::MIR::MirGcEditKind::RemoveIndex,
        TGcEditKind::InsertIndex => jet_foundation::MIR::MirGcEditKind::InsertIndex,
        TGcEditKind::Prepend => jet_foundation::MIR::MirGcEditKind::Prepend,
        TGcEditKind::Additive => jet_foundation::MIR::MirGcEditKind::Additive,
        TGcEditKind::Plain => jet_foundation::MIR::MirGcEditKind::Plain,
        TGcEditKind::EdgeSlot => jet_foundation::MIR::MirGcEditKind::EdgeSlot,
    }
}

fn gc_edit_site(ctx: &LowerCtx) -> MirGcEditSiteId {
    let span = ctx.span();
    MirGcEditSiteId(stable_id(
        "gc-edit-site",
        &format!(
            "{}|{}|{}|{}",
            ctx.function.key, ctx.function.source_file, span.start, span.end
        ),
    ))
}

/// Turn the checked edit body into an executable callback whose first source
/// slot is the rebound GC payload. The old TIR expression remains the body;
/// MIR carries the resulting closure value instead of eagerly executing it.
fn gc_edit_callback(ctx: &LowerCtx, edit: &TExpr, root_ty: Type) -> TLambda {
    let param = TLocal::generated("value");
    let span = ctx.span();
    TLambda {
        executable: TLambdaBody::Expr(Box::new(edit.clone())),
        source_span: span,
        frame_schedule: None,
        frame_schedule_derivation: None,
        capture_facts: TCaptureFacts::default(),
        failure_carrier: super::TFailureCarrier::from_checked_type(&edit.ty),
        effects: TEffectFacts::default(),
        source_params: vec![param.name.clone()],
        jit_name: format!("gc_edit_{}_{}", span.start, span.end),
        param_types: vec![root_ty],
        ret: (!matches!(
            &edit.ty,
            Type::Named(name) if name == crate::Syntax::INTERNAL_UNIT_TYPE
        ))
            .then_some(edit.ty.clone()),
        is_move: true,
        boxed: false,
        rc: false,
        arc: false,
        captures: Vec::new(),
        materialized_captures: Vec::new(),
        frozen_captures: Vec::new(),
        uses_stack_sentry: false,
    }
}



fn lower_pattern_probe(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    matched: jet_foundation::MIR::MirValueId,
    probe: &super::TMatchProbe,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    if matches!(probe, super::TMatchProbe::IsSome) {
        return ctx.emit(
            "pattern-matched",
            Some(Type::Bool),
            MirOperation::PatternMatched { matched },
        );
    }
    let Type::Tuple(types) = &expr.ty else {
        return Err(ctx.error(ctx.span(), "checked pattern captures have no tuple type"));
    };
    let type_id = ctx.mir_type(&expr.ty)?.identity
        .ok_or_else(|| ctx.error(ctx.span(), "checked pattern tuple has no MIR identity"))?;
    let fields = types.iter().enumerate().map(|(index, (name, ty))| {
        let field = ctx.field_id_for_type(&expr.ty, name)?;
        let value = ctx.emit(
            "pattern-capture",
            Some((**ty).clone()),
            MirOperation::PatternCapture { matched, index },
        )?;
        Ok((field, value))
    }).collect::<Result<Vec<_>, LowerError>>()?;
    ctx.emit(
        "pattern-captures",
        Some(expr.ty.clone()),
        MirOperation::Tuple { type_id, fields },
    )
}

fn lower_host_call(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    host: &THostCall,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    match host {
        THostCall::CarrierFact { recv, field, notes } => {
            let field_id = ctx.field_id_for_type(&recv.ty, field)?;
            let receiver = ctx.lower_child(recv)?;
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let call = ctx.intern_prelude_route(super::carrier_fact_route(
                *notes,
                &expr.ty,
                &carrier,
            )?)?;
            ctx.emit(
                "host-carrier-fact",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::CarrierFact {
                    call,
                    receiver,
                    field: field_id,
                    notes: *notes,
                }),
            )
        }
        THostCall::FixedListIndex { base, index, line } => {
            let kind = MirIndexKind::FixedListProof;
            let access = MirAccess::Read;
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let call = ctx.intern_prelude_route(super::index_route(
                kind,
                access,
                &expr.ty,
                &carrier,
            )?)?;
            let location = panic_location(ctx, *line as usize);
            let base = ctx.lower_child(base)?;
            let index = ctx.lower_child(index)?;
            ctx.emit(
                "host-fixed-list-index",
                Some(expr.ty.clone()),
                MirOperation::Index {
                    call,
                    base,
                    index,
                    kind,
                    access,
                    location,
                    context: ctx.panic_context_at(*line, None),
                },
            )
        }
        THostCall::OptionProbe {
            inner,
            kind: TOptionProbe::Field(field),
        } => {
            lower_optional_field(ctx, expr, inner, field, false)
        }
        THostCall::TupleIndex { base, index } => {
            let field = ctx.field_id_for_type(&base.ty, &index.to_string())?;
            let base = ctx.lower_child(base)?;
            ctx.emit(
                "host-tuple-index",
                Some(expr.ty.clone()),
                MirOperation::Field { base, field },
            )
        }
        THostCall::FnName(name) => {
            let function = ctx.function_id_for(name)?;
            ctx.emit(
                "host-function-value",
                Some(expr.ty.clone()),
                MirOperation::Closure {
                    function,
                    captures: Vec::new(),
                    facts: jet_foundation::MIR::MirCaptureFacts::default(),
                },
            )
        }
        THostCall::Helper { kind, args, .. } => {
            let args = lower_host_args(ctx, args)?;
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let route = super::helper_route(*kind, &expr.ty, &carrier)?;
            emit_host_route_call(ctx, expr, route, args)
        }
        THostCall::Method { recv, method, args } => {
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let route =
                super::host_method_route(&recv.ty, method, args.len(), &expr.ty, &carrier)?;
            let mut lowered = Vec::with_capacity(route.signature.arity);
            let mutates_receiver = matches!(
                (recv.ty.without_user_tags(), method.as_str()),
                (Type::Apply { name, .. }, "add" | "remove") if name == "Pool"
            );
            let receiver = if mutates_receiver {
                let place = lower_receiver_place(ctx, recv, MirAccess::Write)?
                    .ok_or_else(|| ctx.error(ctx.span(), "mutable host receiver has no checked place"))?;
                let value = ctx.emit(
                    "host-call-place",
                    Some(recv.ty.clone()),
                    MirOperation::ReadPlace(place),
                )?;
                let mut argument = mir_value_arg_with_access(ctx, value, MirAccess::Write);
                argument.place = Some(place);
                argument
            } else {
                ctx.lower_plain_arg(recv)?
            };
            lowered.push(receiver);
            if matches!(method.as_str(), "read_txn" | "edit_txn" | "capture_txn") {
                let stm = TLocal::stm();
                let stm = lower_tlocal_value(ctx, &stm)?;
                lowered.push(mir_value_arg(ctx, stm));
            }
            lowered.extend(
                args.iter()
                    .map(|arg| ctx.lower_plain_arg(arg))
                    .collect::<Result<Vec<_>, _>>()?,
            );
            if lowered.len() != route.signature.arity {
                return Err(ctx.error(
                    ctx.span(),
                    format!(
                        "checked host method `{method}` has {} operands, route requires {}",
                        lowered.len(),
                        route.signature.arity
                    ),
                ));
            }
            if matches!(
                method.as_str(),
                "read_txn" | "edit_txn" | "capture_txn"
            ) {
                lowered[1].access = MirAccess::Write;
                lowered[1].place = Some(lower_local_place(ctx, &TLocal::stm(), MirAccess::Write)?);
            }
            emit_host_route_call(ctx, expr, route, lowered)
        }
        THostCall::CellGuardProject {
            recv,
            paths,
            result_ty,
            editable,
            edit_paths_disjoint,
        } => {
            let guard = ctx.lower_plain_arg(recv)?.value;
            let mut field_paths = Vec::with_capacity(paths.len());
            for path in paths {
                let mut owner_ty = recv.ty.clone();
                let mut fields = Vec::with_capacity(path.len());
                for field in path {
                    fields.push(ctx.field_id_for_type(&owner_ty, field)?);
                    owner_ty = ctx.checked_field_type(&owner_ty, field)?;
                }
                field_paths.push(fields);
            }
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let map_call = ctx.intern_prelude_route(super::cell_guard_map_route(
                *editable,
                result_ty,
                &carrier,
            )?)?;
            let split_call = if *edit_paths_disjoint && field_paths.len() == 2 {
                Some(ctx.intern_prelude_route(super::cell_guard_split_route(
                    *editable,
                    result_ty,
                    &carrier,
                )?)?)
            } else {
                None
            };
            ctx.emit(
                "cell-guard-project",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::CellGuardProject {
                    map_call,
                    split_call,
                    guard,
                    paths: field_paths,
                    editable: *editable,
                    edit_paths_disjoint: *edit_paths_disjoint,
                }),
            )
        }
        THostCall::TypedText { kind, arg } => {
            let args = vec![ctx.lower_plain_arg(arg)?];
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let route = super::typed_text_route(*kind, &expr.ty, &carrier)?;
            emit_host_route_call(ctx, expr, route, args)
        }
        THostCall::GcEdit {
            root,
            edges,
            edit,
            index_temp,
            kind,
            ..
        } => {
            let root_ty = ctx
                .local_types
                .get(&root.name)
                .cloned()
                .or_else(|| root.persist_ty.clone())
                .ok_or_else(|| {
                    ctx.error(
                        ctx.span(),
                        format!("checked MIR GC root `{}` has no type", root.name),
                    )
                })?;
            let root_value = lower_tlocal_value(ctx, root)?;
            let edge_values = edges
                .iter()
                .map(|edge| lower_tlocal_value(ctx, edge))
                .collect::<Result<Vec<_>, _>>()?;
            let edit_lambda = gc_edit_callback(ctx, edit, root_ty);
            let edit_value = ctx.lower_lambda(&edit_lambda)?;
            let index = index_temp
                .as_ref()
                .map(|(_, value)| ctx.lower_plain_arg(value))
                .transpose()?
                .map(|arg| arg.value);
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let call = ctx.intern_prelude_route(super::gc_edit_route(
                *kind,
                &expr.ty,
                &carrier,
            )?)?;
            ctx.emit(
                "host-gc-edit",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::GcEdit {
                    call,
                    root: root_value,
                    edges: edge_values,
                    edit: edit_value,
                    index,
                    kind: mir_gc_edit_kind(*kind),
                    site: gc_edit_site(ctx),
                }),
            )
        }
        THostCall::GcRead { root } => {
            let root_value = lower_tlocal_value(ctx, root)?;
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let route = super::gc_read_route(&expr.ty, &carrier)?;
            let args = vec![mir_value_arg(ctx, root_value)];
            emit_host_route_call(ctx, expr, route, args)
        }
        THostCall::OptionProbe {
            inner,
            kind: TOptionProbe::IsSome,
        } => {
            let subject = ctx.lower_child(inner)?;
            ctx.emit(
                "option-probe-is-some",
                Some(expr.ty.clone()),
                MirOperation::OptionIsSome { subject },
            )
        }
        THostCall::OptionProbe {
            inner,
            kind: TOptionProbe::Unwrap,
        } => {
            let subject = ctx.lower_child(inner)?;
            ctx.emit(
                "option-probe-unwrap",
                Some(expr.ty.clone()),
                MirOperation::OptionValue { subject },
            )
        }
        THostCall::StrMatchScan {
            subject,
            parts,
            probe,
        } => {
            let subject = ctx.lower_plain_arg(subject)?.value;
            let pattern = parts
                .iter()
                .map(|part| match part {
                    crate::AST::StrMatchPart::Lit(text) => Ok(MirTextPatternPart::Literal(text.clone())),
                    crate::AST::StrMatchPart::Hole { ty, span, .. } => {
                        let ty = ty.clone().unwrap_or(Type::String);
                        Ok(MirTextPatternPart::Hole {
                            kind: lower_text_hole_kind(ctx, &ty, *span)?,
                            ty: lower_pattern_type(ctx, &ty)?,
                            span: *span,
                        })
                    }
                })
                .collect::<Result<Vec<_>, LowerError>>()?;
            let shape = MirPatternShape::Text(pattern, ctx.span());
            let matched = ctx.lower_pattern_match(subject, &shape)?;
            lower_pattern_probe(ctx, expr, matched, probe)
        }
        THostCall::BinMatchScan {
            subject,
            parts,
            probe,
        } => {
            let subject = ctx.lower_plain_arg(subject)?.value;
            let pattern = parts
                .iter()
                .map(|part| match part {
                    crate::AST::BinMatchPart::Lit(bytes) => {
                        Ok(MirBinaryPatternPart::Literal(bytes.clone()))
                    }
                    crate::AST::BinMatchPart::Hole { spec, span, .. } => match spec {
                        crate::AST::BinSpec::Bits { width, endian } => {
                            let ty = Type::IntN {
                                signed: false,
                                bits: *width,
                            };
                            Ok(MirBinaryPatternPart::Bits {
                                width: *width,
                                ty: lower_pattern_type(ctx, &ty)?,
                                little: matches!(endian, crate::AST::BinEndian::Little),
                                span: *span,
                            })
                        }
                        crate::AST::BinSpec::Rest => Ok(MirBinaryPatternPart::Rest {
                            ty: lower_pattern_type(ctx, &Type::Named(crate::Syntax::TYPE_BYTES.to_string()))?,
                            span: *span,
                        }),
                    },
                })
                .collect::<Result<Vec<_>, LowerError>>()?;
            let shape = MirPatternShape::Binary(pattern, ctx.span());
            let matched = ctx.lower_pattern_match(subject, &shape)?;
            lower_pattern_probe(ctx, expr, matched, probe)
        }
        THostCall::SwitchSubjectField { field } => {
            let subject = ctx.switch_subject()?;
            let subject_ty = ctx.value_source_type(subject)?;
            let field = ctx.field_id_for_type(&subject_ty, field)?;
            ctx.emit(
                "switch-subject-field",
                Some(expr.ty.clone()),
                MirOperation::Field {
                    base: subject,
                    field,
                },
            )
        }
        THostCall::SwitchSubjectValue => ctx.switch_subject(),
        THostCall::YieldSend { value } => {
            let value = ctx.lower_child(value)?;
            let resume = ctx.new_block(ctx.span(), "yield.resume")?;
            ctx.terminate(MirTerminator::Yield { value, resume });
            ctx.switch_to(resume);
            ctx.emit(
                "yield.unit",
                Some(expr.ty.clone()),
                MirOperation::Constant(MirConstant::Unit),
            )
        }
        THostCall::TypedTextInterp {
            kind,
            literals,
            holes,
        } => {
            let holes = holes
                .iter()
                .map(|hole| ctx.lower_plain_arg(hole).map(|arg| arg.value))
                .collect::<Result<Vec<_>, _>>()?;
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let call = ctx.intern_prelude_route(super::typed_text_interp_route(
                *kind,
                &expr.ty,
                &carrier,
            )?)?;
            ctx.emit(
                "host-typed-text-interp",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::TypedTextInterp {
                    call,
                    kind: *kind,
                    literals: literals.clone(),
                    holes,
                }),
            )
        }
        THostCall::ExpectSnapshot { value, snap_path } => {
            let value_ty = value.ty.clone();
            let value = ctx.lower_child(value)?;
            let shown = lower_direct_string_format(
                ctx,
                value,
                &value_ty,
                &crate::AST::StrFormat::Display,
                &Type::String,
            )?;
            let path = ctx.emit(
                "expect-snapshot-path",
                Some(Type::String),
                MirOperation::Constant(MirConstant::String(snap_path.clone())),
            )?;
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let route = super::expect_snapshot_route(&expr.ty, &carrier)?;
            let args = vec![mir_value_arg(ctx, shown), mir_value_arg(ctx, path)];
            emit_host_route_call(ctx, expr, route, args)
        }
        THostCall::EnvSet { name, value, loc } => {
            let record = crate::Syntax::core_call("core.sys", "set").ok_or_else(|| {
                ctx.error(ctx.span(), "missing checked Core registry row `core.sys.set`")
            })?;
            let mut args = vec![ctx.lower_plain_arg(name)?, ctx.lower_plain_arg(value)?];
            for (index, arg) in args.iter_mut().enumerate() {
                arg.access = if record.signature.borrow_mask.get(index).copied().unwrap_or(false) {
                    MirAccess::Read
                } else {
                    MirAccess::Move
                };
            }
            let core_result_ty = Type::Result {
                ok: Box::new(Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string())),
                err: Box::new(Type::Named("EnvError".to_string())),
            };
            let core_carrier = super::TFailureCarrier::from_checked_type(&core_result_ty);
            let core_call = MirCoreCall::from_record(record).id;
            let core_route = ctx.intern_core_route(record, &core_carrier)?;
            let core_fallibility = ctx.lower_call_fallibility(&core_carrier)?;
            let result = ctx.emit(
                "core-sys-set",
                Some(core_result_ty.clone()),
                MirOperation::CoreCall {
                    call: core_call,
                    route: core_route,
                    args,
                    type_args: Vec::new(),
                    fallibility: core_fallibility,
                    data_plan: None,
                },
            )?;
            let condition = ctx.emit(
                "core-sys-set-ok",
                Some(Type::Bool),
                MirOperation::ResultIsOk { subject: result },
            )?;
            let error = ctx.emit(
                "core-sys-set-error",
                Some(Type::Named("EnvError".to_string())),
                MirOperation::ResultValue {
                    subject: result,
                    ok: false,
                },
            )?;
            let error = lower_direct_string_format(
                ctx,
                error,
                &Type::Named("EnvError".to_string()),
                &crate::AST::StrFormat::Display,
                &Type::String,
            )?;
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let route = super::require_stop_route("require", 1, &expr.ty, &carrier)?;
            let call = ctx.intern_prelude_route(route)?;
            let location = MirPanicLoc {
                file: ctx.source_file_id_for(&loc.file),
                line: loc.line,
                column: loc.col,
            };
            let context = MirPanicContext {
                function: loc.fn_name.clone(),
                source_line: loc.src_line.clone(),
                caret: loc.caret,
                locals: loc
                    .locals
                    .iter()
                    .map(|(name, local)| Ok((name.clone(), ctx.local_id_for(local)?)))
                    .collect::<Result<Vec<_>, LowerError>>()?,
            };
            ctx.emit(
                "core-sys-set-stop",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::RequireStop {
                    call,
                    kind: MirRequireKind::Require,
                    condition: Some(condition),
                    location,
                    context,
                    values: vec![error],
                    always_stops: false,
                }),
            )
        }
        THostCall::NumericBounds { ty, member } => {
            let constant = numeric_bound_constant(ty, member).ok_or_else(|| {
                ctx.error(
                    ctx.span(),
                    format!("unsupported numeric bound `{}` on {}", member, ty.name()),
                )
            })?;
            ctx.emit(
                "host-numeric-bound",
                Some(expr.ty.clone()),
                MirOperation::Constant(constant),
            )
        }

        THostCall::ExpiringSecretNew {
            value,
            duration,
            clock,
            elem,
        } => {
            let mut args = vec![
                ctx.lower_plain_arg(value)?,
                ctx.lower_plain_arg(duration)?,
                ctx.lower_plain_arg(clock)?,
            ];
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let route = super::expiring_route(true, &expr.ty, &carrier)?;
            apply_route_access(ctx, &route, &mut args)?;
            let call = ctx.intern_prelude_route(route)?;
            let type_args = vec![ctx.mir_type(elem)?];
            ctx.emit(
                "host-expiring-secret-new",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
                    call,
                    args,
                    owner_type_args: Vec::new(),
                    type_args,
                }),
            )
        }
        THostCall::ExpiringValueNew {
            value,
            duration,
            clock,
        } => {
            let value_ty = value.ty.clone();
            let mut args = vec![
                ctx.lower_plain_arg(value)?,
                ctx.lower_plain_arg(duration)?,
                ctx.lower_plain_arg(clock)?,
            ];
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let route = super::expiring_route(false, &expr.ty, &carrier)?;
            apply_route_access(ctx, &route, &mut args)?;
            let call = ctx.intern_prelude_route(route)?;
            let type_args = vec![ctx.mir_type(&value_ty)?];
            ctx.emit(
                "host-expiring-value-new",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
                    call,
                    args,
                    owner_type_args: Vec::new(),
                    type_args,
                }),
            )
        }
        THostCall::CCallback {
            symbol,
            lambda,
            ret,
            managed,
            plan_digest,
            callback_identity,
        } => {
            let value = ctx.lower_lambda(lambda)?;
            let key = format!(
                "{}::lambda@{}..{}",
                ctx.function.key, lambda.source_span.start, lambda.source_span.end
            );
            let function = ctx
                .nested_functions
                .iter()
                .find(|candidate| candidate.key == key)
                .map(|candidate| candidate.id)
                .ok_or_else(|| ctx.error(ctx.span(), "missing lowered C callback lambda"))?;
            if lambda.source_params.len() != lambda.param_types.len() {
                return Err(ctx.error(
                    lambda.source_span,
                    "C callback lambda parameter names and types differ",
                ));
            }
            let params = lambda
                .source_params
                .iter()
                .cloned()
                .zip(lambda.param_types.iter())
                .enumerate()
                .map(|(index, (name, ty))| {
                    Ok::<_, LowerError>(MirParam {
                        index,
                        name: name.clone(),
                        span: lambda.source_span,
                        ty: ctx.mir_type(ty)?,
                        access: MirAccess::Read,
                        ownership: MirOwnership::from_access(MirAccess::Read),
                        public_label: name.clone(),
                        variadic: false,
                        default_present: false,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let return_type = ret
                .as_ref()
                .map(|ty| ctx.mir_type(ty))
                .transpose()?;
            let callback = MirCallbackAdapter {
                id: MirCallbackId(stable_id(
                    "mir-callback",
                    &format!("{symbol}|{}", function.0),
                )),
                symbol: symbol.clone(),
                function,
                params,
                return_type,
                abi: MirForeignAbi::C,
                managed: *managed,
                plan_digest: plan_digest.clone(),
                callback_identity: callback_identity.clone(),
            };
            if let Some(existing) = ctx.callbacks.iter().find(|row| row.id == callback.id) {
                if format!("{existing:?}") != format!("{callback:?}") {
                    return Err(ctx.error(ctx.span(), "conflicting C callback adapter"));
                }
            } else {
                ctx.callbacks.push(callback.clone());
            }
            let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
            let route = super::c_callback_route(&expr.ty, &carrier)?;
            let call = ctx.intern_prelude_route(route)?;
            ctx.emit(
                "c-callback",
                Some(expr.ty.clone()),
                MirOperation::Semantic(MirSemanticOp::CCallback {
                    call,
                    callback: callback.id,
                    lambda: value,
                }),
            )
        }
    }
}
fn numeric_bound_constant(ty: &Type, member: &str) -> Option<MirConstant> {
    match (ty, member) {
        (Type::Float32, "MAX") => Some(MirConstant::Float {
            value: f32::MAX as f64,
            f32: true,
            spelling: None,
        }),
        (Type::Float32, "MIN") => Some(MirConstant::Float {
            value: f32::MIN as f64,
            f32: true,
            spelling: None,
        }),
        (Type::Float32, "NAN") => Some(MirConstant::Float {
            value: f32::NAN as f64,
            f32: true,
            spelling: None,
        }),
        (Type::Float32, "INFINITY") => Some(MirConstant::Float {
            value: f32::INFINITY as f64,
            f32: true,
            spelling: None,
        }),
        (Type::Float32, "NEG_INFINITY") => Some(MirConstant::Float {
            value: f32::NEG_INFINITY as f64,
            f32: true,
            spelling: None,
        }),
        (Type::Float32, "EPSILON") => Some(MirConstant::Float {
            value: f32::EPSILON as f64,
            f32: true,
            spelling: None,
        }),
        (Type::Float, "MAX") => Some(MirConstant::Float {
            value: f64::MAX,
            f32: false,
            spelling: None,
        }),
        (Type::Float, "MIN") => Some(MirConstant::Float {
            value: f64::MIN,
            f32: false,
            spelling: None,
        }),
        (Type::Float, "NAN") => Some(MirConstant::Float {
            value: f64::NAN,
            f32: false,
            spelling: None,
        }),
        (Type::Float, "INFINITY") => Some(MirConstant::Float {
            value: f64::INFINITY,
            f32: false,
            spelling: None,
        }),
        (Type::Float, "NEG_INFINITY") => Some(MirConstant::Float {
            value: f64::NEG_INFINITY,
            f32: false,
            spelling: None,
        }),
        (Type::Float, "EPSILON") => Some(MirConstant::Float {
            value: f64::EPSILON,
            f32: false,
            spelling: None,
        }),
        (Type::Int, "MAX") => Some(MirConstant::Int {
            value: i64::MAX,
            width: None,
            spelling: None,
        }),
        (Type::Int, "MIN") => Some(MirConstant::Int {
            value: i64::MIN,
            width: None,
            spelling: None,
        }),
        (Type::IntN { signed, bits }, "MAX" | "MIN") => {
            let (lo, hi) = crate::AST::int_range(*signed, *bits);
            let value = crate::Comptime::MathLayout::integer_narrow(
                if member == "MAX" { hi } else { lo },
                *signed,
                *bits,
            );
            Some(MirConstant::Int {
                value,
                width: Some((*signed, *bits)),
                spelling: None,
            })
        }
        _ => None,
    }
}

fn lower_host_args(
    ctx: &mut LowerCtx,
    args: &[super::THostArg],
) -> Result<Vec<MirCallArg>, LowerError> {
    args.iter()
        .map(|arg| match arg {
            super::THostArg::Expr(value) => ctx.lower_plain_arg(value),
            super::THostArg::Borrow(value) => {
                let mut lowered = ctx.lower_plain_arg(value)?;
                lowered.access = MirAccess::Read;
                Ok(lowered)
            }
            super::THostArg::Lambda(lambda) => {
                let value = ctx.lower_lambda(lambda)?;
                Ok(MirCallArg {
                    value,
                    place: None,
                    access: MirAccess::Read,
                    span: ctx.span(),
                    label: None,
                    source_index: None,
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
                })
            }
        })
        .collect()
}

fn apply_route_access(
    ctx: &LowerCtx,
    route: &super::TPreludeRoute,
    args: &mut [MirCallArg],
) -> Result<(), LowerError> {
    for (index, arg) in args.iter_mut().enumerate() {
        if arg.access != MirAccess::Write {
            arg.access = ctx.call_value_access(
                arg.value,
                route.signature.borrow_mask.get(index).copied().unwrap_or(false),
            )?;
        }
    }
    Ok(())
}

fn emit_host_route_call(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    route: super::TPreludeRoute,
    mut args: Vec<MirCallArg>,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    if args.len() != route.signature.arity {
        return Err(ctx.error(
            ctx.span(),
            format!(
                "checked host operation has {} operands, route requires {}",
                args.len(),
                route.signature.arity
            ),
        ));
    }
    apply_route_access(ctx, &route, &mut args)?;
    let call = ctx.intern_prelude_route(route)?;
    ctx.emit(
        "host-call",
        Some(expr.ty.clone()),
        MirOperation::Semantic(MirSemanticOp::HostCall { call, args }),
    )
}


fn lower_string_format_args(
    ctx: &mut LowerCtx,
    value: jet_foundation::MIR::MirValueId,
    format: &crate::AST::StrFormat,
) -> Result<Vec<MirCallArg>, LowerError> {
    let mut args = vec![mir_value_arg(ctx, value)];
    let mut constant =
        |ty: crate::AST::Type, operation: MirOperation, access: MirAccess| -> Result<(), LowerError> {
            let value = ctx.emit("string-format-constant", Some(ty), operation)?;
            args.push(mir_value_arg_with_access(ctx, value, access));
            Ok(())
        };
    match format {
        crate::AST::StrFormat::Fixed(value)
        | crate::AST::StrFormat::Grouped(value)
        | crate::AST::StrFormat::Hex(value)
        | crate::AST::StrFormat::Sci(value)
        | crate::AST::StrFormat::Percent(value) => {
            constant(
                crate::AST::Type::Int,
                MirOperation::Constant(MirConstant::Int {
                    value: *value,
                    width: None,
                    spelling: None,
                }),
                MirAccess::Read,
            )?;
        }
        crate::AST::StrFormat::Pad { width, fill }
        | crate::AST::StrFormat::PadLeft { width, fill } => {
            constant(
                crate::AST::Type::Int,
                MirOperation::Constant(MirConstant::Int {
                    value: *width,
                    width: None,
                    spelling: None,
                }),
                MirAccess::Read,
            )?;
            constant(
                crate::AST::Type::String,
                MirOperation::Constant(MirConstant::String(fill.clone())),
                MirAccess::Read,
            )?;
        }
        crate::AST::StrFormat::Display
        | crate::AST::StrFormat::Debug
        | crate::AST::StrFormat::Pretty
        | crate::AST::StrFormat::Bin
        | crate::AST::StrFormat::Oct
        | crate::AST::StrFormat::Unit(_) => {}
    }
    Ok(args)
}
fn mir_value_arg(ctx: &LowerCtx, value: jet_foundation::MIR::MirValueId) -> MirCallArg {
    MirCallArg {
        value,
        place: None,
        access: MirAccess::Read,
        span: ctx.span(),
        label: None,
        source_index: None,
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
    }
}

fn http_json_method_receiver(recv: &Type, op: &THandleOp) -> Option<&'static str> {
    let method = match op {
        THandleOp::HTTPClientMethod { method, .. }
        | THandleOp::HTTPServerMethod { method, .. } => method.as_str(),
        _ => return None,
    };
    if method != "json" {
        return None;
    }
    match recv.without_user_tags() {
        Type::Named(name) if name == "HTTPRequest" => Some("HTTPRequest"),
        Type::Named(name) if name == "HTTPResponse" => Some("HTTPResponse"),
        Type::Named(name) if name == "HTTPBody" => Some("HTTPBody"),
        _ => None,
    }
}

fn http_text_route(receiver: &str, with_limit: bool) -> TPreludeRoute {
    let (member, symbol) = match (receiver, with_limit) {
        ("HTTPRequest", false) => ("request_text", "jet_http_request_text"),
        ("HTTPRequest", true) => ("request_text_with_limit", "jet_http_request_text_with_limit"),
        ("HTTPResponse", false) => ("response_text", "jet_http_response_text"),
        ("HTTPResponse", true) => ("response_text_with_limit", "jet_http_response_text_with_limit"),
        ("HTTPBody", true) => ("body_text", "jet_http_body_text"),
        _ => unreachable!("checked HTTP JSON receiver has no text route"),
    };
    let arity = if with_limit { 2 } else { 1 };
    TPreludeRoute {
        family: MirPreludeFamily::HandleMethod,
        module: "core.http".to_string(),
        member: member.to_string(),
        symbol: MirSymbol::Prelude(symbol.to_string()),
        signature: MirCallSignature {
            arity,
            max_arity: arity,
            borrow_mask: if with_limit {
                vec![true, false]
            } else {
                vec![true]
            },
        },
        effect: Some(jet_foundation::Effects::Effect::Net),
        fallibility: TFailureCarrier::Result {
            success: Type::String,
            error: Type::Named("HTTPError".to_string()),
        },
        abi: MirPreludeAbi::Value,
        authority: None,
        db_metadata: None,
    }
}
fn http_json_project_decode_route() -> TPreludeRoute {
    TPreludeRoute {
        family: MirPreludeFamily::StaticPrelude,
        module: "core.http".to_string(),
        member: "project_json_decode_error".to_string(),
        symbol: MirSymbol::Prelude("jet_http_project_json_decode_error".to_string()),
        signature: MirCallSignature {
            arity: 1,
            max_arity: 1,
            borrow_mask: vec![false],
        },
        effect: None,
        fallibility: TFailureCarrier::Infallible,
        abi: MirPreludeAbi::Value,
        authority: None,
        db_metadata: None,
    }
}

fn lower_http_json_method(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
    args: &[TExpr],
    receiver: &'static str,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let Type::Result { ok, err } = expr.ty.without_user_tags() else {
        return unsupported_expr(ctx, "HTTP json method is not a Result");
    };
    if !matches!(
        err.as_ref().without_user_tags(),
        Type::Named(name) if name == "HTTPError"
    ) {
        return unsupported_expr(ctx, "HTTP json method does not return HTTPError");
    }
    let target = ok.as_ref().clone();
    let (with_limit, expected_args) = match receiver {
        "HTTPRequest" => (false, 0),
        "HTTPResponse" | "HTTPBody" => (true, 1),
        _ => unreachable!("checked HTTP JSON receiver"),
    };
    if args.len() != expected_args {
        return Err(ctx.error(
            ctx.span(),
            format!(
                "checked HTTP json method has {} arguments; expected {expected_args}",
                args.len()
            ),
        ));
    }

    let mut text_operands = vec![ctx.lower_child(recv)?];
    if with_limit {
        text_operands.extend(lower_handle_method_args(ctx, args)?);
    }
    let text_ty = Type::Result {
        ok: Box::new(Type::String),
        err: Box::new(Type::Named("HTTPError".to_string())),
    };
    let text = emit_static_call(
        ctx,
        &text_ty,
        http_text_route(receiver, with_limit),
        text_operands,
    )?;

    let decode_error = Type::List(Box::new(Type::Named("FieldError".to_string())));
    let decode_ty = Type::Result {
        ok: Box::new(target.clone()),
        err: Box::new(decode_error),
    };
    let record = crate::Syntax::core_call("core.encoding.json", "decode")
        .ok_or_else(|| ctx.error(ctx.span(), "typed HTTP JSON decode Core row is not registered"))?;
    let decode_carrier = TFailureCarrier::from_checked_type(&decode_ty);
    let decode_type_arg = ctx.mir_type(&target)?;
    let decode_route = ctx.intern_core_route(record, &decode_carrier)?;
    let decode_fallibility = ctx.lower_call_fallibility(&decode_carrier)?;

    let text_ok = ctx.emit(
        "http-json-text-is-ok",
        Some(Type::Bool),
        MirOperation::ResultIsOk { subject: text },
    )?;
    let text_success = ctx.new_block(ctx.span(), "http-json-text-success")?;
    let text_failure = ctx.new_block(ctx.span(), "http-json-text-failure")?;
    let join = ctx.new_block(ctx.span(), "http-json-join")?;
    ctx.terminate(MirTerminator::Branch {
        condition: text_ok,
        then_target: text_success,
        else_target: text_failure,
    });

    let mut incoming = Vec::with_capacity(2);
    ctx.switch_to(text_failure);
    let text_error = ctx.emit(
        "http-json-text-error",
        Some(Type::Named("HTTPError".to_string())),
        MirOperation::ResultValue {
            subject: text,
            ok: false,
        },
    )?;
    let text_result = ctx.emit(
        "http-json-text-result-error",
        Some(expr.ty.clone()),
        MirOperation::ResultErr { value: text_error },
    )?;
    let source = ctx.current_block();
    ctx.terminate(MirTerminator::Jump { target: join });
    incoming.push((source, text_result));

    ctx.switch_to(text_success);
    let text_value = ctx.emit(
        "http-json-text-value",
        Some(Type::String),
        MirOperation::ResultValue {
            subject: text,
            ok: true,
        },
    )?;
    let decode = ctx.emit(
        "http-json-decode",
        Some(decode_ty.clone()),
        MirOperation::CoreCall {
            call: MirCoreCall::from_record(record).id,
            route: decode_route,
            args: vec![mir_value_arg(ctx, text_value)],
            type_args: vec![decode_type_arg],
            fallibility: decode_fallibility,
            data_plan: None,
        },
    )?;
    let projected = emit_static_call(
        ctx,
        &expr.ty,
        http_json_project_decode_route(),
        vec![decode],
    )?;
    let source = ctx.current_block();
    ctx.terminate(MirTerminator::Jump { target: join });
    incoming.push((source, projected));

    ctx.switch_to(join);
    ctx.emit(
        "http-json-result",
        Some(expr.ty.clone()),
        MirOperation::Phi { incoming },
    )
}

fn emit_static_call(
    ctx: &mut LowerCtx,
    result: &crate::AST::Type,
    route: super::TPreludeRoute,
    values: Vec<jet_foundation::MIR::MirValueId>,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let args = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let access = ctx.call_value_access(
                value,
                route.signature.borrow_mask.get(index).copied().unwrap_or(false),
            )?;
            Ok(mir_value_arg_with_access(ctx, value, access))
        })
        .collect::<Result<Vec<_>, LowerError>>()?;
    let call = ctx.intern_prelude_route(route)?;
    ctx.emit(
        "static-prelude-call",
        Some(result.clone()),
        MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
            call,
            args,
            owner_type_args: Vec::new(),
            type_args: Vec::new(),
        }),
    )
}
fn emit_concat_list(
    ctx: &mut LowerCtx,
    result: &crate::AST::Type,
    carrier: &super::TFailureCarrier,
    left: jet_foundation::MIR::MirValueId,
    right: jet_foundation::MIR::MirValueId,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let route = match TBuiltinOp::ConcatList.route_plan(result, carrier)? {
        super::TRoutePlan::Prelude(route) => route,
        super::TRoutePlan::Primitive => {
            return unsupported_expr(ctx, "TBuiltinOp::ConcatList has no Prelude route")
        }
    };
    emit_static_call(ctx, result, route, vec![left, right])
}

fn lower_direct_string_format(
    ctx: &mut LowerCtx,
    value: jet_foundation::MIR::MirValueId,
    value_ty: &crate::AST::Type,
    format: &crate::AST::StrFormat,
    result: &crate::AST::Type,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let (trait_name, suffix) = match format {
        crate::AST::StrFormat::Display => ("Display", "display"),
        crate::AST::StrFormat::Debug => ("Debug", "debug"),
        crate::AST::StrFormat::Unit(_) => {
            return unsupported_expr(
                ctx,
                "unit interpolation must already be lowered to String",
            )
        }
        _ => return unsupported_expr(ctx, "direct string format for non-display form"),
    };
    let function_name = format!("{}::{trait_name}::{suffix}", value_ty.name());
    match ctx.function_id_for(&function_name) {
        Ok(function) => ctx.emit(
            "string-format-user-call",
            Some(result.clone()),
            MirOperation::Call {
                callee: MirCallee::User(function),
                args: vec![mir_value_arg(ctx, value)],
                type_args: Vec::new(),
            },
        ),
        Err(error) if error.message.starts_with("missing checked function target") => {
            let route =
                super::string_format_route(format, value_ty, result, &super::TFailureCarrier::Infallible)?;
            let call = ctx.intern_prelude_route(route)?;
            ctx.emit(
                "string-format-prelude-call",
                Some(result.clone()),
                MirOperation::Call {
                    callee: MirCallee::Prelude(call),
                    args: vec![mir_value_arg(ctx, value)],
                    type_args: Vec::new(),
                },
            )
        }
        Err(error) => Err(error),
    }
}

fn mir_value_arg_with_access(
    ctx: &LowerCtx,
    value: jet_foundation::MIR::MirValueId,
    access: MirAccess,
) -> MirCallArg {
    MirCallArg {
        value,
        place: None,
        access,
        span: ctx.span(),
        label: None,
        source_index: None,
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
    }
}


/// Marshal an exact Int through the existing checked native-width conversion.
fn lower_builtin_native_int(
    ctx: &mut LowerCtx,
    value: MirValueId,
    source: &Type,
) -> Result<MirValueId, LowerError> {
    if !matches!(source.without_user_tags(), Type::Int) {
        return Ok(value);
    }
    let Some(TNumericOp::TryFrom { host_kind, dst_rust, dst_spelling }) =
        super::resolve_numeric_conversion_op("I64", "Int")
    else {
        return Err(ctx.error(ctx.span(), "native Int operand has no checked I64 conversion"));
    };
    let target = Type::IntN { signed: true, bits: 64 };
    let carrier = TFailureCarrier::Infallible;
    let op = TNumericOp::CheckedIntToFixed {
        host_kind,
        dst_rust,
        dst_spelling,
        line: ctx.current_line.unwrap_or(ctx.function.line as u32),
    };
    let route = super::distinct_conversion_route("", source, &op, None, &target, &carrier)?;
    let kind = lower_conversion_int(ctx, host_kind)?;
    lower_prelude_conversion(ctx, value, vec![kind], &target, route, &carrier)
}

fn lower_tuple_builtin(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
    op: &TBuiltinOp,
    args: &[TExpr],
    carrier: &TFailureCarrier,
) -> Result<MirValueId, LowerError> {
    let (struct_name, row_ty, indexed) = match op {
        TBuiltinOp::Indexed { tuple_struct } => {
            let row = match expr.ty.without_user_tags() {
                Type::Apply { args, .. } if crate::Collections::is_iter_type(&expr.ty) && args.len() == 1 => &args[0],
                Type::List(element) => element.as_ref(),
                _ => return Err(ctx.error(ctx.span(), "checked indexed result has no row type")),
            };
            (tuple_struct, row, true)
        }
        TBuiltinOp::IterSplit { tuple_struct } => (tuple_struct, &expr.ty, false),
        _ => unreachable!("tuple builtin projection receives indexed or split"),
    };
    let Type::Tuple(fields) = row_ty.without_user_tags() else {
        return Err(ctx.error(ctx.span(), "checked tuple builtin has no tuple fields"));
    };
    if fields.len() != 2 || args.len() != usize::from(!indexed) {
        return Err(ctx.error(ctx.span(), "checked tuple builtin has inconsistent operands"));
    }
    let element = match recv.ty.without_user_tags() {
        Type::List(element) | Type::FixedList { elem: element, .. } => element.as_ref(),
        Type::Apply { args, .. } if crate::Collections::is_iter_type(&recv.ty) && args.len() == 1 => &args[0],
        _ => return Err(ctx.error(ctx.span(), "checked tuple builtin receiver is not a sequence")),
    };
    let (mut receiver, _) = lower_builtin_receiver(ctx, recv, false)?;
    let mut operands = lower_values(ctx, args)?;
    if !indexed {
        operands[0] = lower_builtin_native_int(ctx, operands[0], &args[0].ty)?;
    }
    if !crate::Collections::is_iter_type(&recv.ty) {
        let iter_ty = crate::Collections::iter_ty(element.clone());
        let route = TBuiltinOp::ListLazy.prelude_route(&recv.ty, &iter_ty, &TFailureCarrier::Infallible)?;
        let mut arg = mir_value_arg_with_access(ctx, receiver, ctx.call_value_access(receiver, false)?);
        arg.widen_fixed_to_list = matches!(recv.ty.without_user_tags(), Type::FixedList { .. });
        let call = ctx.intern_prelude_route(route)?;
        receiver = ctx.emit(
            "tuple-builtin-iterator",
            Some(iter_ty),
            MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
                call,
                args: vec![arg],
                owner_type_args: Vec::new(),
                type_args: Vec::new(),
            }),
        )?;
    }
    let params = if indexed {
        vec![Type::IntN { signed: true, bits: 64 }, element.clone()]
    } else {
        fields.iter().map(|(_, ty)| ty.as_ref().clone()).collect()
    };
    let row_fields = fields.iter().enumerate().map(|(index, (name, ty))| {
        let local = TExpr {
            ty: params[index].clone(),
            kind: TExprKind::Local(TLocal::user(format!("zip_{index}"))),
        };
        let value = if indexed && index == 0 {
            TExpr {
                ty: ty.as_ref().clone(),
                kind: TExprKind::NumericMethod {
                    recv: Box::new(local),
                    op: TNumericOp::CastAs { dst_rust: "i64".to_string() },
                },
            }
        } else {
            local
        };
        (name.clone(), value)
    }).collect();
    let callback = zip_callback(ctx, params, TExpr {
        ty: row_ty.clone(),
        kind: TExprKind::TupleLit { struct_name: struct_name.clone(), fields: row_fields },
    })?;
    let mut values = operands.into_iter().map(|value| (value, MirAccess::Move)).collect::<Vec<_>>();
    values.push((callback, MirAccess::Move));
    let call_ty = if indexed { crate::Collections::iter_ty(row_ty.clone()) } else { expr.ty.clone() };
    let route = op.prelude_route(&recv.ty, &call_ty, carrier)?;
    let result = zip_closure_call(ctx, call_ty.clone(), route, receiver, values)?;
    if indexed && matches!(expr.ty.without_user_tags(), Type::List(_)) {
        let route = TBuiltinOp::IterToList.prelude_route(&call_ty, &expr.ty, carrier)?;
        emit_static_call(ctx, &expr.ty, route, vec![result])
    } else {
        Ok(result)
    }
}

fn lower_numeric_method(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
    op: &TNumericOp,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let receiver = ctx.lower_child(recv)?;
    if matches!(op, TNumericOp::CastAs { .. }) {
        let target = ctx.mir_type(&expr.ty)?;
        return ctx.emit(
            "numeric-cast",
            Some(expr.ty.clone()),
            MirOperation::Convert {
                value: receiver,
                parameters: Vec::new(),
                target,
                conversion: MirConversion::NumericCast,
            },
        );
    }
    if let TNumericOp::BitCount { method, width } = op {
        let operation = match method.as_str() {
            "count_ones" => 0,
            "count_zeros" => 1,
            "leading_zeros" => 2,
            "trailing_zeros" => 3,
            _ => return Err(ctx.error(ctx.span(), "unknown checked integer population operation")),
        };
        let exact = matches!(recv.ty.without_user_tags(), Type::Int);
        let receiver = if exact {
            receiver
        } else {
            let raw_type = Type::IntN { signed: true, bits: 64 };
            let target = ctx.mir_type(&raw_type)?;
            ctx.emit(
                "numeric-bits",
                Some(raw_type),
                MirOperation::Convert {
                    value: receiver,
                    parameters: Vec::new(),
                    target,
                    conversion: MirConversion::NumericCast,
                },
            )?
        };
        let operation = lower_conversion_int(ctx, operation)?;
        let width = lower_conversion_int(ctx, i64::from(*width))?;
        let member = if exact { "int_bit_count" } else { "bit_count" };
        let call = ctx.intern_prelude_route(super::TPreludeRoute {
            family: jet_foundation::MIR::MirPreludeFamily::BuiltinMethod,
            module: "core.numeric".to_string(),
            member: member.to_string(),
            symbol: jet_foundation::MIR::MirSymbol::Prelude(format!("jet_numeric_{member}")),
            signature: jet_foundation::MIR::MirCallSignature {
                arity: 3,
                max_arity: 3,
                borrow_mask: vec![false; 3],
            },
            effect: None,
            fallibility: super::TFailureCarrier::Infallible,
            abi: jet_foundation::MIR::MirPreludeAbi::Value,
            authority: None,
            db_metadata: None,
        })?;
        return ctx.emit(
            "integer-population",
            Some(expr.ty.clone()),
            MirOperation::Call {
                callee: MirCallee::Prelude(call),
                args: vec![
                    mir_value_arg(ctx, receiver),
                    mir_value_arg(ctx, operation),
                    mir_value_arg(ctx, width),
                ],
                type_args: Vec::new(),
            },
        );
    }
    if matches!(op, TNumericOp::ToShow) {
        return lower_direct_string_format(
            ctx,
            receiver,
            &recv.ty,
            &crate::AST::StrFormat::Display,
            &expr.ty,
        );
    }
    let carrier = super::TFailureCarrier::from_checked_type(&expr.ty);
    let conversion = matches!(
        op,
        TNumericOp::CheckedIntToFloat { .. }
            | TNumericOp::CheckedIntToFixed { .. }
            | TNumericOp::TryFrom { .. }
            | TNumericOp::FloatToInt { .. }
            | TNumericOp::FloatNarrow { .. }
            | TNumericOp::InlineRange { .. }
    );
    if conversion {
        if matches!(op, TNumericOp::InlineRange { fallible: false, .. }) {
            return ctx.emit(
                "proven-range-conversion",
                Some(expr.ty.clone()),
                MirOperation::Copy { value: receiver },
            );
        }
        let route = super::distinct_conversion_route(
            "",
            recv.ty.without_user_tags(),
            op,
            None,
            &expr.ty,
            &carrier,
        )?;
        let parameters = match op {
            TNumericOp::CheckedIntToFloat {
                target_f32,
                ..
            } if matches!(recv.ty.without_user_tags(), Type::Int) => {
                vec![lower_conversion_bool(ctx, *target_f32)?]
            }
            TNumericOp::CheckedIntToFloat {
                source_signed,
                target_f32,
                ..
            } => vec![
                lower_conversion_bool(ctx, *source_signed)?,
                lower_conversion_bool(ctx, *target_f32)?,
            ],
            TNumericOp::CheckedIntToFixed { host_kind, .. } => {
                vec![lower_conversion_int(ctx, *host_kind)?]
            }
            TNumericOp::TryFrom { host_kind, .. }
                if matches!(recv.ty.without_user_tags(), Type::Int) =>
            {
                vec![lower_conversion_int(ctx, *host_kind)?]
            }
            TNumericOp::TryFrom { host_kind, .. } => vec![
                lower_conversion_bool(
                    ctx,
                    matches!(recv.ty.without_user_tags(), Type::IntN { signed: true, .. }),
                )?,
                lower_conversion_int(ctx, *host_kind)?,
            ],
            TNumericOp::FloatToInt { host_kind, .. } => {
                vec![lower_conversion_int(ctx, *host_kind)?]
            }
            TNumericOp::FloatNarrow { .. } => Vec::new(),
            TNumericOp::InlineRange { lo, hi, .. } => vec![
                lower_conversion_int(ctx, *lo)?,
                lower_conversion_int(ctx, *hi)?,
            ],
            _ => Vec::new(),
        };
        return lower_prelude_conversion(ctx, receiver, parameters, &expr.ty, route, &carrier);
    }
    let route = op.prelude_route(&expr.ty, &carrier)?;
    let call = ctx.intern_prelude_route(route)?;
    ctx.emit(
        "numeric-method",
        Some(expr.ty.clone()),
        MirOperation::Semantic(MirSemanticOp::NumericMethod { call, receiver }),
    )

}
fn lower_fn_value(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    kind: &TFnValueKind,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    match kind {
        TFnValueKind::NamedFn {
            name: Some(name),
            lambda: None,
            ..
        } => {
            let function = ctx.function_id_for(name)?;
            ctx.emit(
                "named-function-value",
                Some(expr.ty.clone()),
                MirOperation::Closure {
                    function,
                    captures: Vec::new(),
                    facts: jet_foundation::MIR::MirCaptureFacts::default(),
                },
            )
        }
        TFnValueKind::NamedFn {
            lambda: Some(lambda), ..
        } => ctx.lower_lambda(lambda),
        TFnValueKind::Call { callee, args } => {
            let callee = ctx.lower_child(callee)?;
            let args = lower_call_args(ctx, args)?;
            ctx.emit(
                "indirect-function-call",
                Some(expr.ty.clone()),
                MirOperation::IndirectCall {
                    callee,
                    args,
                    type_args: Vec::new(),
                },
            )
        }
        TFnValueKind::Policy {
            policy_args,
            callee,
            policy_conventions,
            ..
        } => {
            let callee = ctx.lower_child(callee)?;
            let mut args = lower_call_args(ctx, policy_args)?;
            for (index, arg) in args.iter_mut().enumerate() {
                arg.access = match policy_conventions.get(index).copied() {
                    Some(crate::AST::AccessConvention::Move) => MirAccess::Move,
                    Some(crate::AST::AccessConvention::Write) => MirAccess::Write,
                    _ => MirAccess::Read,
                };
            }
            ctx.emit(
                "policy-function-call",
                Some(expr.ty.clone()),
                MirOperation::IndirectCall {
                    callee,
                    args,
                    type_args: Vec::new(),
                },
            )
        }
        TFnValueKind::Send { value } => match &value.kind {
            TExprKind::Lambda(lambda) => ctx.lower_lambda_send(lambda),
            TExprKind::FnValue {
                kind:
                    TFnValueKind::NamedFn {
                        name: Some(name),
                        lambda: None,
                    },
            } => ctx.lower_named_fn_send(name, &value.ty),
            TExprKind::FnValue {
                kind: TFnValueKind::NamedFn {
                    lambda: Some(lambda), ..
                },
            } => ctx.lower_lambda_send(lambda),
            TExprKind::HostBorrowCallback { callable, params } => {
                let callable = match &callable.kind {
                    TExprKind::Lambda(lambda) => ctx.lower_lambda_send(lambda)?,
                    TExprKind::FnValue {
                        kind:
                            TFnValueKind::NamedFn {
                                name: Some(name),
                                lambda: None,
                            },
                    } => ctx.lower_named_fn_send(name, &callable.ty)?,
                    TExprKind::FnValue {
                        kind: TFnValueKind::Send { .. },
                    }
                    | TExprKind::Local(_) => ctx.lower_child(callable)?,
                    _ => {
                        return unsupported_expr(
                            ctx,
                            "SendFn HostBorrowCallback requires a named function, lambda, or SendFn local",
                        )
                    }
                };
                let target = ctx.mir_send_fn_type(&expr.ty)?;
                let params = params
                    .iter()
                    .map(|ty| ctx.mir_type(ty))
                    .collect::<Result<Vec<_>, _>>()?;
                ctx.emit_mir_type(
                    "send-host-borrow-callback",
                    Some(target),
                    MirOperation::Semantic(MirSemanticOp::HostBorrowCallback { callable, params }),
                )
            }
            TExprKind::Local(_) | TExprKind::FnValue {
                kind: TFnValueKind::Send { .. },
            } => {
                let value = ctx.lower_child(value)?;
                let target = ctx.mir_send_fn_type(&expr.ty)?;
                ctx.emit_mir_type(
                    "send-function-carrier",
                    Some(target.clone()),
                    MirOperation::Convert {
                        value,
                        parameters: Vec::new(),
                        target,
                        conversion: MirConversion::SendFn,
                    },
                )
            }
            _ => unsupported_expr(ctx, "SendFn requires a named function, lambda, or SendFn local"),
        },
        TFnValueKind::NamedFn { .. } => unsupported_expr(ctx, "TFnValueKind::NamedFn"),
    }
}



fn datatree_access_route(
    op: &THandleOp,
    carrier: &TFailureCarrier,
) -> Option<TPreludeRoute> {
    let (member, symbol, arity, borrow_mask) = match op {
        THandleOp::DataTreeField | THandleOp::JSONField => {
            ("field", "jet_datatree_field", 2, vec![true, true])
        }
        THandleOp::DataTreeAt | THandleOp::JSONAt => {
            ("at", "jet_datatree_at", 2, vec![true, false])
        }
        THandleOp::DataTreeInt | THandleOp::JSONInt => {
            ("int", "jet_datatree_int", 1, vec![true])
        }
        THandleOp::DataTreeText | THandleOp::JSONText => {
            ("text", "jet_datatree_text", 1, vec![true])
        }
        THandleOp::DataTreeBool | THandleOp::JSONBool => {
            ("bool", "jet_datatree_bool", 1, vec![true])
        }
        THandleOp::DataTreeFloat | THandleOp::JSONFloat => {
            ("float", "jet_datatree_float", 1, vec![true])
        }
        THandleOp::DataTreeToText | THandleOp::JSONToText => {
            ("to_text", "jet_datatree_to_text", 1, vec![true])
        }
        THandleOp::DataTreeEqualUnordered | THandleOp::JSONEqualUnordered => {
            ("equal_unordered", "jet_datatree_equal_unordered", 2, vec![true, true])
        }
        _ => return None,
    };
    Some(TPreludeRoute {
        family: MirPreludeFamily::HandleMethod,
        module: "core.encoding.datatree".to_string(),
        member: member.to_string(),
        symbol: MirSymbol::Prelude(symbol.to_string()),
        signature: MirCallSignature {
            arity,
            max_arity: arity,
            borrow_mask,
        },
        effect: None,
        fallibility: carrier.clone(),
        abi: MirPreludeAbi::Value,
        authority: None,
        db_metadata: None,
    })
}

fn lower_datatree_access(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
    source_args: &[TExpr],
    op: &THandleOp,
    carrier: &TFailureCarrier,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let route = datatree_access_route(op, carrier)
        .expect("DataTree access route checked before structural lowering");
    let receiver = ctx.lower_child(recv)?;
    let args = source_args
        .iter()
        .map(|arg| ctx.lower_child(arg))
        .collect::<Result<Vec<_>, _>>()?;
    let call = ctx.intern_prelude_route(route)?;
    ctx.emit(
        "datatree-access",
        Some(expr.ty.clone()),
        MirOperation::Semantic(MirSemanticOp::HandleMethod {
            call,
            receiver,
            args,
            frame_schedule: None,
            frame_schedule_derivation: None,
        }),
    )
}

fn typed_codec_target(ty: &Type) -> bool {
    match ty.without_user_tags() {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::IntN { .. }
        | Type::Float32
        | Type::List(_)
        | Type::FixedList { .. }
        | Type::Option(_)
        | Type::InlineRange { .. } => true,
        Type::Map { key, .. } => matches!(key.without_user_tags(), Type::String),
        Type::Named(name) => matches!(
            name.as_str(),
            "Data"
                | "DataTree"
                | "U8"
                | "I8"
                | "I16"
                | "I32"
                | "I64"
                | "I128"
                | "U16"
                | "U32"
                | "U64"
                | "U128"
        ),
        _ => false,
    }
}

fn lower_typed_codec(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
    type_arg: &Type,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let route = TPreludeRoute {
        family: MirPreludeFamily::StaticPrelude,
        module: "core.encoding.codec".to_string(),
        member: "decode_typed".to_string(),
        symbol: MirSymbol::Prelude("jet_codec_decode_typed".to_string()),
        signature: MirCallSignature {
            arity: 1,
            max_arity: 1,
            borrow_mask: vec![true],
        },
        effect: None,
        fallibility: TFailureCarrier::from_checked_type(&expr.ty),
        abi: MirPreludeAbi::Value,
        authority: None,
        db_metadata: None,
    };
    let call = ctx.intern_prelude_route(route)?;
    let value = ctx.lower_child(recv)?;
    let args = vec![mir_value_arg_with_access(ctx, value, MirAccess::Read)];
    let type_args = vec![ctx.mir_type(type_arg)?];
    ctx.emit(
        "serde-typed-decode-call",
        Some(expr.ty.clone()),
        MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
            call,
            args,
            owner_type_args: Vec::new(),
            type_args,
        }),
    )
}
fn lower_typed_encode(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
    type_arg: &Type,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let route = TPreludeRoute {
        family: MirPreludeFamily::StaticPrelude,
        module: "core.encoding.codec".to_string(),
        member: "encode".to_string(),
        symbol: MirSymbol::Prelude("jet_codec_encode".to_string()),
        signature: MirCallSignature {
            arity: 2,
            max_arity: 2,
            borrow_mask: vec![false, true],
        },
        effect: None,
        fallibility: TFailureCarrier::from_checked_type(&expr.ty),
        abi: MirPreludeAbi::Value,
        authority: None,
        db_metadata: None,
    };
    let call = ctx.intern_prelude_route(route)?;
    // Generic `jet_codec_encode` keeps the builtin codec kind slot in its ABI;
    // its `T: __jet_Encode` implementation ignores this sentinel.
    let kind_value = ctx.emit(
        "serde-codec-kind",
        Some(Type::IntN { signed: true, bits: 64 }),
        MirOperation::Constant(MirConstant::Int {
            value: 0,
            width: Some((true, 64)),
            spelling: None,
        }),
    )?;
    let value = ctx.lower_child(recv)?;
    let args = vec![
        mir_value_arg_with_access(ctx, kind_value, MirAccess::Read),
        mir_value_arg_with_access(ctx, value, MirAccess::Read),
    ];
    let type_args = vec![ctx.mir_type(type_arg)?];
    ctx.emit(
        "serde-typed-encode-call",
        Some(expr.ty.clone()),
        MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
            call,
            args,
            owner_type_args: Vec::new(),
            type_args,
        }),
    )
}


fn missing_function_target(error: &LowerError) -> bool {
    error.message.starts_with("missing checked function target")
}

fn builtin_codec_kind(ty: &Type) -> Option<i64> {
    match super::builtin_codec_name(ty)? {
        "Date" => Some(0),
        "LocalDate" => Some(1),
        "LocalTime" => Some(2),
        "DateTime" => Some(3),
        "Duration" => Some(4),
        "Decimal" => Some(5),
        _ => None,
    }
}

fn lower_builtin_codec(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
    type_arg: &Type,
    kind: i64,
    encode: bool,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let member = if encode { "encode" } else { "decode" };
    let symbol = format!("jet_codec_{member}");
    let route = TPreludeRoute {
        family: MirPreludeFamily::StaticPrelude,
        module: "core.encoding.codec".to_string(),
        member: member.to_string(),
        symbol: MirSymbol::Prelude(symbol),
        signature: MirCallSignature {
            arity: 2,
            max_arity: 2,
            borrow_mask: vec![false, true],
        },
        effect: None,
        fallibility: TFailureCarrier::from_checked_type(&expr.ty),
        abi: MirPreludeAbi::Value,
        authority: None,
        db_metadata: None,
    };
    let call = ctx.intern_prelude_route(route)?;
    let kind_value = ctx.emit(
        "serde-codec-kind",
        Some(Type::IntN { signed: true, bits: 64 }),
        MirOperation::Constant(MirConstant::Int {
            value: kind,
            width: Some((true, 64)),
            spelling: None,
        }),
    )?;
    let value = ctx.lower_child(recv)?;
    let args = vec![
        mir_value_arg_with_access(ctx, kind_value, MirAccess::Read),
        mir_value_arg_with_access(ctx, value, MirAccess::Read),
    ];
    let type_args = vec![ctx.mir_type(type_arg)?];
    ctx.emit(
        "serde-codec-call",
        Some(expr.ty.clone()),
        MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
            call,
            args,
            owner_type_args: Vec::new(),
            type_args,
        }),
    )
}

fn lower_optional_encode(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
    inner: &Type,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let subject = ctx.lower_child(recv)?;
    let condition = ctx.emit("encode-option-present", Some(Type::Bool), MirOperation::OptionIsSome { subject })?;
    let present = ctx.new_block(ctx.span(), "encode-option-present")?;
    let absent = ctx.new_block(ctx.span(), "encode-option-absent")?;
    let join = ctx.new_block(ctx.span(), "encode-option-join")?;
    ctx.terminate(MirTerminator::Branch { condition, then_target: present, else_target: absent });
    ctx.switch_to(present);
    let value = ctx.emit("encode-option-value", Some(inner.clone()), MirOperation::OptionValue { subject })?;
    let local = TLocal::generated(format!("encode_option_{}", subject.0));
    let place = ctx.bind_local(&local, inner.clone(), false, false, false)?;
    ctx.emit("encode-option-bind", None, MirOperation::WritePlace { place, value })?;
    let encoded = lower_serde_encode(ctx, expr, &TExpr { ty: inner.clone(), kind: TExprKind::Local(local) })?;
    let present_end = ctx.current_block();
    ctx.terminate(MirTerminator::Jump { target: join });
    ctx.switch_to(absent);
    let type_id = ctx.type_id_for(crate::Syntax::TYPE_DATA)?;
    let null = ctx.emit("encode-option-null", Some(expr.ty.clone()), MirOperation::Enum {
        type_id,
        variant: "Null".to_string(),
        args: Vec::new(),
    })?;
    let absent_end = ctx.current_block();
    ctx.terminate(MirTerminator::Jump { target: join });
    ctx.switch_to(join);
    ctx.emit("encode-option-result", Some(expr.ty.clone()), MirOperation::Phi {
        incoming: vec![(present_end, encoded), (absent_end, null)],
    })
}

fn lower_container_encode(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
    element: &Type,
    map: bool,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let source_value = ctx.lower_child(recv)?;
    let source = TLocal::generated(format!("encode_source_{}", source_value.0));
    let source_place = ctx.bind_local(&source, recv.ty.clone(), false, false, false)?;
    ctx.emit("encode-source", None, MirOperation::WritePlace { place: source_place, value: source_value })?;
    let source_expr = TExpr { ty: recv.ty.clone(), kind: TExprKind::Local(source) };
    let tree_ty = Type::Named(crate::Syntax::TYPE_DATA.to_string());
    let item = TLocal::generated(format!("encode_item_{}", source_value.0));
    let item_ty = if map {
        Type::Tuple(vec![
            ("key".to_string(), Box::new(Type::String)),
            ("value".to_string(), Box::new(element.clone())),
        ])
    } else {
        element.clone()
    };
    let item_expr = TExpr { ty: item_ty, kind: TExprKind::Local(item.clone()) };
    let item_value = if map {
        TExpr {
            ty: element.clone(),
            kind: TExprKind::Field {
                recv: Box::new(item_expr.clone()),
                field: "value".to_string(),
                boxed: false,
            },
        }
    } else {
        item_expr.clone()
    };
    let mut encoded = TExpr {
        ty: tree_ty.clone(),
        kind: TExprKind::HandleMethod {
            recv: Box::new(item_value),
            op: THandleOp::SerdeEncode,
            args: Vec::new(),
        },
    };
    if map {
        let shape = vec![("key".to_string(), Type::String), ("value".to_string(), tree_ty.clone())];
        encoded = TExpr {
            ty: Type::Tuple(shape.iter().map(|(name, ty)| (name.clone(), Box::new(ty.clone()))).collect()),
            kind: TExprKind::TupleLit {
                struct_name: crate::Codegen::Tuples::tuple_struct_name(&shape),
                fields: vec![
                    ("key".to_string(), TExpr {
                        ty: Type::String,
                        kind: TExprKind::Field {
                            recv: Box::new(item_expr),
                            field: "key".to_string(),
                            boxed: false,
                        },
                    }),
                    ("value".to_string(), encoded),
                ],
            },
        };
    }
    let output_ty = Type::List(Box::new(encoded.ty.clone()));
    let output = TLocal::generated(format!("encode_output_{}", source_value.0)).as_mutable();
    let output_place = ctx.bind_local(&output, output_ty.clone(), true, false, false)?;
    let empty = ctx.emit("encode-empty", Some(output_ty.clone()), MirOperation::BuildList { values: Vec::new() })?;
    ctx.emit("encode-output", None, MirOperation::WritePlace { place: output_place, value: empty })?;
    super::tir_to_mir_stmt::lower_stmt(ctx, &super::TStmt::ForIn {
        label: None,
        var: item.name,
        var2: None,
        source: source_expr.clone(),
        collection: source_expr,
        step: None,
        method_kind: None,
        columnar: false,
        by_value: false,
        body: vec![super::TStmt::ExprStmt(TExpr {
            ty: Type::Named("Unit".to_string()),
            kind: TExprKind::BuiltinMethod {
                recv: Box::new(TExpr { ty: output_ty.clone(), kind: TExprKind::Local(output.clone()) }),
                op: TBuiltinOp::Push,
                args: vec![encoded],
            },
        })],
    })?;
    let output_place = lower_local_place(ctx, &output, MirAccess::Move)?;
    let payload = ctx.emit("encode-container", Some(output_ty), MirOperation::MovePlace { place: output_place })?;
    let type_id = ctx.type_id_for(crate::Syntax::TYPE_DATA)?;
    ctx.emit("encode-tree", Some(expr.ty.clone()), MirOperation::Enum {
        type_id,
        variant: if map { "Object" } else { "Array" }.to_string(),
        args: vec![MirEnumArg { field: None, value: payload, boxed: false }],
    })
}

fn lower_serde_encode(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let ty = recv.ty.without_user_tags();
    let function_name = match &ty {
        Type::Union(members) => {
            format!("{}::encode", crate::AST::union_enum_name(members))
        }
        _ => format!("{}::encode", ty.name()),
    };
    match ctx.function_id_for(&function_name) {
        Ok(function) => {
            let owner_ty = match &ty {
                Type::Union(members) => {
                    Type::Named(crate::AST::union_enum_name(members))
                }
                _ => recv.ty.clone(),
            };
            let owner = ctx.mir_type(&owner_ty)?;
            let args = vec![ctx.lower_plain_arg(recv)?];
            return ctx.emit(
                "serde-encode-call",
                Some(expr.ty.clone()),
                MirOperation::Call {
                    callee: MirCallee::Method { function, owner },
                    args,
                    type_args: Vec::new(),
                },
            );
        }
        Err(error) if missing_function_target(&error) => {}
        Err(error) => return Err(error),
    }
    if matches!(ty, Type::Named(name) if name == crate::Syntax::TYPE_DATA) {
        return ctx.lower_child(recv);
    }
    if let Some(kind) = builtin_codec_kind(ty) {
        return lower_builtin_codec(ctx, expr, recv, ty, kind, true);
    }
    match ty {
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if !matches!(inner.without_user_tags(), Type::IntN { signed: false, bits: 8 }) =>
        {
            return lower_container_encode(ctx, expr, recv, inner, false);
        }
        Type::Map { key, value, .. } if matches!(key.without_user_tags(), Type::String) => {
            return lower_container_encode(ctx, expr, recv, value, true);
        }
        Type::Option(inner) => return lower_optional_encode(ctx, expr, recv, inner),
        _ => {}
    }
    if typed_codec_target(ty) {
        return lower_typed_encode(ctx, expr, recv, ty);
    }
    let variant = data_tree_encode_variant(ty).ok_or_else(|| {
        ctx.error(
            ctx.span(),
            format!("serde encode of `{}` requires a checked encode method", ty.name()),
        )
    })?;
    let type_id = ctx.type_id_for(crate::Syntax::TYPE_DATA)?;
    let value = ctx.lower_child(recv)?;
    ctx.emit(
        "serde-encode-tree",
        Some(expr.ty.clone()),
        MirOperation::Enum {
            type_id,
            variant: variant.to_string(),
            args: vec![MirEnumArg {
                field: None,
                value,
                boxed: false,
            }],
        },
    )
}

fn lower_datatree_decode(
    ctx: &mut LowerCtx,
    expr: &TExpr,
    recv: &TExpr,
    target: &Type,
) -> Result<jet_foundation::MIR::MirValueId, LowerError> {
    let target_ty = target.without_user_tags();
    let function_name = match &target_ty {
        Type::Union(members) => {
            format!("{}::decode", crate::AST::union_enum_name(members))
        }
        _ => format!("{}::decode", target.name()),
    };
    match ctx.function_id_for(&function_name) {
        Ok(function) => {
            let owner_ty = match &target_ty {
                Type::Union(members) => {
                    Type::Named(crate::AST::union_enum_name(members))
                }
                _ => target.clone(),
            };
            let owner = ctx.mir_type(&owner_ty)?;
            let args = vec![ctx.lower_plain_arg(recv)?];
            return ctx.emit(
                "datatree-decode-call",
                Some(expr.ty.clone()),
                MirOperation::Call {
                    callee: MirCallee::Associated { function, owner },
                    args,
                    type_args: Vec::new(),
                },
            );
        }
        Err(error) if missing_function_target(&error) => {}
        Err(error) => return Err(error),
    }
    if let Some(kind) = builtin_codec_kind(target) {
        return lower_builtin_codec(ctx, expr, recv, target, kind, false);
    }
    if typed_codec_target(target) {
        return lower_typed_codec(ctx, expr, recv, target);
    }
    Err(ctx.error(
        ctx.span(),
        format!("serde decode of `{}` requires a checked decode method", target.name()),
    ))
}

fn data_tree_encode_variant(ty: &Type) -> Option<&'static str> {
    match ty.without_user_tags() {
        Type::Int | Type::IntN { .. } => Some("Int"),
        Type::Float | Type::Float32 => Some("Float"),
        Type::Bool => Some("Bool"),
        Type::String => Some("Text"),
        Type::Char => Some("Text"),
        _ => None,
    }
}

fn instance_method_lookup(method: &super::TMethodRef, recv_ty: &Type) -> String {
    let key = method_key(method);
    let owner = match recv_ty.without_user_tags() {
        Type::Named(name) => Some(name.clone()),
        Type::Apply { name, .. } => Some(name.clone()),
        Type::Tagged { inner, .. } => {
            return instance_method_lookup(method, inner);
        }
        _ => None,
    };
    let Some(owner) = owner else {
        return key;
    };
    if key == owner || key.starts_with(&format!("{owner}::")) {
        return key;
    }
    format!("{owner}::{key}")
}

fn method_key(method: &super::TMethodRef) -> String {
    if let Some(identity) = &method.operator_identity {
        return identity.clone();
    }
    match &method.trait_owner {
        Some(owner) => format!("{owner}::{}", method.name),
        None => method.name.clone(),
    }
}


/// Project the fully checked TIR pattern into canonical MIR.  This mapper
/// consumes typed pattern shapes and never revisits source AST syntax.
pub(super) fn lower_pattern(
    ctx: &mut LowerCtx,
    pattern: &TPattern,
) -> Result<MirPattern, LowerError> {
    let owner_name = pattern.enum_type.as_deref().or_else(|| {
        matches!(
            &pattern.shape,
            TPatternShape::Variant { variant, .. }
                if crate::Codegen::is_key_variant(variant)
        )
        .then_some(crate::Syntax::TYPE_KEY)
    });
    let owner = owner_name.map(|key| ctx.type_id_for(key)).transpose()?;
    let position = match &pattern.position {
        TPatternPosition::Binding => MirPatternPosition::Binding,
        TPatternPosition::OptionBinding => MirPatternPosition::OptionBinding,
        TPatternPosition::Arm => MirPatternPosition::Arm,
        TPatternPosition::VariantPath => MirPatternPosition::VariantPath,
        TPatternPosition::DataEntries { temp } => {
            let owner = owner.ok_or_else(|| {
                ctx.error(ctx.span(), "checked data-entry pattern is missing its owner")
            })?;
            let variant = match &pattern.shape {
                TPatternShape::Variant { variant, .. } => variant,
                _ => {
                    return Err(ctx.error(
                        ctx.span(),
                        "checked data-entry pattern is missing its enum variant",
                    ));
                }
            };
            let local = super::TLocal::generated(temp);
            let temp_id = ctx.bind_data_entries_temp(&local, owner, variant)?;
            MirPatternPosition::DataEntries { temp: temp_id }
        }
    };
    let shape = lower_pattern_shape(ctx, &pattern.shape, owner)?;
    Ok(MirPattern {
        shape,
        owner,
        position,
        mutable: pattern.mutable,
        boxed: pattern.boxed,
    })
}
fn lower_pattern_type(
    ctx: &mut LowerCtx,
    ty: &Type,
) -> Result<jet_foundation::MIR::MirTypeId, LowerError> {
    let mir = ctx.mir_type(ty)?;
    if let Some(identity) = mir.identity {
        Ok(identity)
    } else {
        Err(ctx.error(
            ctx.span(),
            "checked pattern type has no canonical MIR identity",
        ))
    }
}

fn lower_pattern_shape(
    ctx: &mut LowerCtx,
    shape: &TPatternShape,
    owner: Option<jet_foundation::MIR::MirTypeId>,
) -> Result<MirPatternShape, LowerError> {
    Ok(match shape {
        TPatternShape::Variant {
            variant,
            bindings,
            leading_dot,
            span,
        } => {
            let bindings = bindings.iter().map(lower_pattern_binding).collect::<Vec<_>>();
            if bindings.is_empty() {
                if let Some(leaves) = owner
                    .and_then(|owner| ctx.enum_group_leaves(owner, variant))
                {
                    MirPatternShape::Or {
                        alternatives: leaves
                            .into_iter()
                            .map(|variant| MirPatternShape::Variant {
                                variant,
                                bindings: Vec::new(),
                                leading_dot: *leading_dot,
                                span: *span,
                            })
                            .collect(),
                        span: *span,
                    }
                } else {
                    MirPatternShape::Variant {
                        variant: variant.clone(),
                        bindings,
                        leading_dot: *leading_dot,
                        span: *span,
                    }
                }
            } else {
                MirPatternShape::Variant {
                    variant: variant.clone(),
                    bindings,
                    leading_dot: *leading_dot,
                    span: *span,
                }
            }
        },
        TPatternShape::Present {
            binding,
            binding_span,
            span,
        } => MirPatternShape::Present {
            binding: binding.clone(),
            binding_span: *binding_span,
            span: *span,
        },
        TPatternShape::Absent(span) => MirPatternShape::Absent(*span),
        TPatternShape::Ok {
            binding,
            binding_span,
            span,
        } => MirPatternShape::Ok {
            binding: binding.clone(),
            binding_span: *binding_span,
            span: *span,
        },
        TPatternShape::Err {
            binding,
            binding_span,
            span,
        } => MirPatternShape::Err {
            binding: binding.clone(),
            binding_span: *binding_span,
            span: *span,
        },
        TPatternShape::Range { lo, hi, span } => MirPatternShape::Range {
            lo: *lo,
            hi: *hi,
            span: *span,
        },
        TPatternShape::Or(alternatives, span) => MirPatternShape::Or {
            alternatives: alternatives
                .iter()
                .map(|alternative| lower_pattern_shape(ctx, alternative, owner))
                .collect::<Result<Vec<_>, _>>()?,
            span: *span,
        },
        TPatternShape::Struct { fields, rest, span } => {
            let owner = owner.ok_or_else(|| {
                ctx.error(*span, "checked struct pattern is missing its owner type")
            })?;
            let fields = fields
                .iter()
                .map(|field| match field {
                    TPatternField::Bind {
                        field,
                        local,
                        span,
                    } => Ok(MirPatternField::Bind {
                        field: ctx.field_id_for(owner, field)?,
                        local: local.clone(),
                        span: *span,
                    }),
                    TPatternField::Value { field, value, span } => Ok(MirPatternField::Value {
                        field: ctx.field_id_for(owner, field)?,
                        value: ctx.lower_child(value)?,
                        span: *span,
                    }),
                })
                .collect::<Result<Vec<_>, LowerError>>()?;
            MirPatternShape::Struct {
                fields,
                rest: *rest,
                span: *span,
            }
        }
        TPatternShape::Text(parts, span) => MirPatternShape::Text(
            parts
                .iter()
                .map(|part| match part {
                    TTextPatternPart::Literal(text) => {
                        Ok(MirTextPatternPart::Literal(text.clone()))
                    }
                    TTextPatternPart::Hole { ty, span, .. } => Ok(MirTextPatternPart::Hole {
                        kind: lower_text_hole_kind(ctx, ty, *span)?,
                        ty: lower_pattern_type(ctx, ty)?,
                        span: *span,
                    }),
                })
                .collect::<Result<Vec<_>, LowerError>>()?,
            *span,
        ),
        TPatternShape::Binary(parts, span) => MirPatternShape::Binary(
            parts
                .iter()
                .map(|part| match part {
                    TBinaryPatternPart::Literal(bytes) => {
                        Ok(MirBinaryPatternPart::Literal(bytes.clone()))
                    }
                    TBinaryPatternPart::Bits {
                        width,
                        big_endian,
                        ty,
                        span,
                        ..
                    } => Ok(MirBinaryPatternPart::Bits {
                        width: *width,
                        ty: lower_pattern_type(ctx, ty)?,
                        little: !*big_endian,
                        span: *span,
                    }),
                    TBinaryPatternPart::Rest { ty, span, .. } => {
                        Ok(MirBinaryPatternPart::Rest {
                            ty: lower_pattern_type(ctx, ty)?,
                            span: *span,
                        })
                    }
                })
                .collect::<Result<Vec<_>, LowerError>>()?,
            *span,
        ),
    })
}
fn lower_text_hole_kind(
    ctx: &mut LowerCtx,
    ty: &crate::AST::Type,
    _span: crate::Diagnostics::Span,
) -> Result<MirTextHoleKind, LowerError> {
    Ok(match ty {
        crate::AST::Type::String | crate::AST::Type::Char => MirTextHoleKind::Text,
        crate::AST::Type::Int
        | crate::AST::Type::IntN { .. }
        | crate::AST::Type::Measure(_) => MirTextHoleKind::Int,
        crate::AST::Type::Float | crate::AST::Type::Float32 => MirTextHoleKind::Float,
        crate::AST::Type::Bool => MirTextHoleKind::Bool,
        crate::AST::Type::InlineRange { lo, hi, .. } => {
            MirTextHoleKind::InlineRange { lo: *lo, hi: *hi }
        }
        _ => return unsupported_pattern(ctx, "TPatternShape::Text (invalid hole type)"),
    })
}

fn lower_pattern_binding(binding: &TPatternBinding) -> MirPatternBinding {
    match binding {
        TPatternBinding::Wildcard => MirPatternBinding::Wildcard,
        TPatternBinding::Bind { name, span } => MirPatternBinding::Bind {
            name: name.clone(),
            span: *span,
        },
        TPatternBinding::Range { lo, hi } => MirPatternBinding::Range { lo: *lo, hi: *hi },
    }
}


fn default_constant(ty: &crate::AST::Type) -> crate::AST::CtValue {
    use crate::AST::{CtFloat, CtValue, Type};
    match ty {
        Type::Int | Type::IntN { .. } | Type::Measure(_) => CtValue::Int(0),
        Type::Float => CtValue::Float(CtFloat::f64(0.0)),
        Type::Float32 => CtValue::Float(CtFloat::f32(0.0)),
        Type::Bool => CtValue::Bool(false),
        Type::String => CtValue::Str(String::new()),
        Type::Char => CtValue::Char('\0'),
        Type::List(_) | Type::FixedList { .. } => CtValue::List(Vec::new()),
        Type::Map { .. } => CtValue::Map(BTreeMap::new()),
        Type::Shared(inner)
        | Type::InlineRange { base: inner, .. }
        | Type::Tagged { inner, .. }
        | Type::Quantity { base: inner, .. } => default_constant(inner),
        Type::Option(inner) => CtValue::absent((**inner).clone()),
        Type::Result { ok, .. } => CtValue::Present(Box::new(default_constant(ok))),
        Type::Tuple(fields) => CtValue::Struct {
            type_name: "Tuple".to_string(),
            fields: fields
                .iter()
                .map(|(name, ty)| (name.clone(), default_constant(ty)))
                .collect(),
        },
        Type::Fn { .. }
        | Type::Named(_)
        | Type::Apply { .. }
        | Type::TraitObject(_)
        | Type::Union(_) => CtValue::Unit,
    }
}

fn unsupported_expr<T>(
    ctx: &mut LowerCtx,
    variant: impl Into<String>,
) -> Result<T, LowerError> {
    let span = ctx.span();
    let variant = variant.into();
    Err(ctx.error(span, format!("{variant} has no canonical MIR operation")))
}

fn unsupported_pattern<T>(
    ctx: &mut LowerCtx,
    variant: impl Into<String>,
) -> Result<T, LowerError> {
    let span = ctx.span();
    let variant = variant.into();
    Err(ctx.error(span, format!("{variant} has no canonical MIR projection")))
}
