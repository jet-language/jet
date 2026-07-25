use jet_codegen::Codegen::TIR::{
    self, JitProgram, JitSpawnCapture, TBuiltinOp, TCallArg, TCoreClosureKind, TEnumPayload, TExpr,
    TExprKind, TFunc, TFuncKind, THandleOp, THostCall, TIfCond, TJitSpawnBody, TJitSpawnLambda, TModuleCallForm, TOrFallback,
    TStmt, TStrPart,
    TNumericOp,
};
use jet_foundation::AST::{BinOp, Pattern, Type, UnOp};
use std::collections::HashSet;

pub(crate) fn flatten_string(parts: &[TStrPart]) -> Option<String> {
    let mut out = String::new();
    for p in parts {
        match p {
            TStrPart::Lit(s) => out.push_str(s),
            TStrPart::Interp(_, _) => return None,
        }
    }
    Some(out)
}

fn resident_safe_string_parts(parts: &[TStrPart], callees: &HashSet<String>) -> bool {
    parts.iter().all(|p| match p {
        TStrPart::Lit(_) => true,
        TStrPart::Interp(e, _) => resident_safe_expr(e, callees),
    })
}

fn jit_scalar_type(ty: &Type) -> bool {
    jit_value_type(ty)
}

pub(crate) fn jit_list_int_type(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if matches!(inner.as_ref(), Type::Int))
        || matches!(ty, Type::FixedList { elem, .. } if matches!(elem.as_ref(), Type::Int))
}

pub(crate) fn jit_list_float_type(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if matches!(inner.as_ref(), Type::Float))
        || matches!(ty, Type::FixedList { elem, .. } if matches!(elem.as_ref(), Type::Float))
}

pub(crate) fn jit_list_string_type(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if matches!(inner.as_ref(), Type::String))
        || matches!(ty, Type::FixedList { elem, .. } if matches!(elem.as_ref(), Type::String))
}

pub(crate) fn jit_list_native_type(ty: &Type) -> bool {
    jit_list_int_type(ty) || jit_list_float_type(ty) || jit_list_string_type(ty)
}

/// `[[Int]]` / `Iter<[Int]>` — flat_map identity receivers in iter_adapters.
pub(crate) fn jit_list_of_int_list_type(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if jit_list_int_type(inner))
        || matches!(
            ty,
            Type::Apply { name, args }
                if name == jet_foundation::Syntax::TYPE_ITER
                    && args.len() == 1
                    && jit_list_int_type(&args[0])
        )
}

pub(crate) fn jit_list_iter_elem_type(ty: &Type) -> Option<Type> {
    match ty {
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if matches!(
                inner.as_ref(),
                Type::Int | Type::Float | Type::String | Type::Char
            ) =>
        {
            Some(inner.as_ref().clone())
        }
        // JIT ABI: `Iter<T>` / `View<T>` / `ViewMut<T>` producers materialize list
        // handles — scalar elems share list for-in / join / len; record elems too.
        Type::Apply { name, args }
            if (name == jet_foundation::Syntax::TYPE_ITER
                || matches!(name.as_str(), "View" | "ViewMut"))
                && args.len() == 1
                && (matches!(
                    &args[0],
                    Type::Int | Type::Float | Type::String | Type::Char
                ) || record_type_key(&args[0]).is_some()) =>
        {
            Some(args[0].clone())
        }
        _ => None,
    }
}

/// Closure adapter receivers: scalar list/Iter/View elems, or user struct handles.
pub(crate) fn jit_closure_elem_type(ty: &Type) -> Option<Type> {
    if let Some(elem) = jit_list_iter_elem_type(ty) {
        return Some(elem);
    }
    match ty {
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if record_type_key(inner).is_some() =>
        {
            Some(inner.as_ref().clone())
        }
        Type::Apply { name, args }
            if (name == jet_foundation::Syntax::TYPE_ITER
                || matches!(name.as_str(), "View" | "ViewMut"))
                && args.len() == 1
                && record_type_key(&args[0]).is_some() =>
        {
            Some(args[0].clone())
        }
        _ => None,
    }
}

/// `[T?E]` / `Iter<T?E>` with JIT-representable payloads.
pub(crate) fn jit_result_list_elem(ty: &Type) -> Option<(Type, Type)> {
    let elem = match ty {
        Type::List(inner) | Type::FixedList { elem: inner, .. } => inner.as_ref(),
        Type::Apply { name, args }
            if name == jet_foundation::Syntax::TYPE_ITER && args.len() == 1 =>
        {
            &args[0]
        }
        _ => return None,
    };
    match elem {
        Type::Result { ok, err }
            if jit_result_payload_type(ok) && jit_result_payload_type(err) =>
        {
            Some((ok.as_ref().clone(), err.as_ref().clone()))
        }
        _ => None,
    }
}

pub(crate) fn jit_map_string_type(ty: &Type) -> bool {
    matches!(ty, Type::Map { key, .. } if matches!(key.as_ref(), Type::String))
}

pub(crate) fn jit_list_task_int_type(ty: &Type) -> bool {
    if let Type::List(inner) = ty {
        if let Type::Apply { name, args } = inner.as_ref() {
            return name == "Task" && args.len() == 1 && matches!(&args[0], Type::Int);
        }
    }
    false
}

pub(crate) fn jit_optional_scalar_type(ty: &Type) -> bool {
    matches!(ty, Type::Option(inner) if jit_scalar_type(inner))
}

pub(crate) fn user_type_name(ty: &Type) -> Option<&str> {
    match ty {
        Type::Named(n) if n != "Unit" => Some(n.as_str()),
        Type::Apply { name, .. } => Some(name.as_str()),
        _ => None,
    }
}

pub(crate) fn record_type_key(ty: &Type) -> Option<String> {
    match ty {
        Type::Tuple(_) => Some(ty.name()),
        _ => user_type_name(ty).map(str::to_string),
    }
}

pub(crate) fn jit_struct_type(ty: &Type) -> bool {
    user_type_name(ty).is_some()
}

pub(crate) fn jit_list_record_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::List(elem) | Type::FixedList { elem, .. }
            if record_type_key(elem).is_some()
    )
}

pub(crate) fn jit_tuple_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(fields) if fields.iter().all(|(_, t)| matches!(t.as_ref(), Type::Int | Type::Float)))
}

pub(crate) fn jit_enum_type(ty: &Type) -> bool {
    user_type_name(ty).is_some()
}

fn jit_compound_type(ty: &Type) -> bool {
    jit_list_native_type(ty)
        || jit_list_of_int_list_type(ty)
        || jit_list_task_int_type(ty)
        || jit_list_record_type(ty)
        || jit_map_string_type(ty)
        || jit_struct_type(ty)
        || jit_tuple_type(ty)
        || jit_enum_type(ty)
        || jit_optional_scalar_type(ty)
}

fn jit_concurrency_elem(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if n == "Unit") || jit_scalar_type(ty)
}

pub(crate) fn jit_concurrency_type(ty: &Type) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    matches!(name.as_str(), "Task" | "Receiver" | "Sender")
        && args.len() == 1
        && jit_concurrency_elem(&args[0])
}

pub(crate) fn jit_value_type(ty: &Type) -> bool {
    if let Some((base, _)) = ty.quantity_parts() {
        return jit_value_type(base);
    }
    match ty {
        Type::Named(n)
            if matches!(
                n.as_str(),
                "Unit" | "Duration" | "DurationUnit" | "RangeError" | "ParseError"
            ) =>
        {
            true
        }
        Type::Int
        | Type::IntN { .. }
        | Type::Float
        | Type::Float32
        | Type::Bool
        | Type::String
        | Type::Char => true,
        Type::Result { ok, err } => {
            jit_result_payload_type(ok.as_ref()) && jit_result_payload_type(err.as_ref())
        }
        other if jit_concurrency_type(other) => true,
        other if jit_compound_type(other) => true,
        _ => false,
    }
}

pub(crate) fn jit_result_payload_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if n == "Unit" || n == "Void" || n == "Error")
        || jit_value_type(ty)
}

pub(crate) fn resident_safe_expr(expr: &TExpr, callees: &HashSet<String>) -> bool {
    match &expr.kind {
        TExprKind::Print(inner) => resident_safe_expr(inner, callees),
        TExprKind::Call { name, args } => {
            if !callees.contains(name) {
                return false;
            }
            args.iter().all(|a| resident_safe_call_arg(a, callees))
        }
        TExprKind::ModuleCall { form: TModuleCallForm::InlineMangled { mangled }, args } => {
            callees.contains(mangled) && args.iter().all(|arg| resident_safe_call_arg(arg, callees))
        }
        TExprKind::CoreCall {
            module,
            method,
            args,
            ..
        } => {
            if module == "core.text" {
                return matches!(args.as_slice(), [arg]
                    if matches!(method.as_str(), "lower" | "upper" | "trim")
                        && matches!(&arg.ty, Type::String)
                        && resident_safe_expr(arg, callees));
            }
            if module == "core.io" && method == "args" {
                return args.is_empty();
            }
            if module == "core.tasks" && method == "channel" {
                return false;
            }
            // Other core modules retain their existing resident coverage. core.text
            // alone is exact above: unsupported methods cannot fail into fallback.
            resident_safe_expr_list(args, callees)
        }
        TExprKind::CoreClosureCall { kind } => match kind {
            TCoreClosureKind::Spawn { .. } => true,
            _ => false,
        },
        TExprKind::HandleMethod { recv, op, args } => {
            resident_safe_expr(recv, callees)
                && args.iter().all(|a| resident_safe_expr(a, callees))
                && resident_safe_handle_op(op, recv, args)
        }
        TExprKind::OrFallback {
            value,
            fallback,
            is_option,
        } => {
            if *is_option {
                resident_safe_expr(value, callees)
                    && matches!(
                        fallback,
                        TOrFallback::Value(_)
                            | TOrFallback::Panic { .. }
                            | TOrFallback::Break
                            | TOrFallback::Continue
                            | TOrFallback::BreakLabel(_)
                            | TOrFallback::ContinueLabel(_)
                    )
            } else {
                resident_safe_expr(value, callees)
                    && match fallback {
                        TOrFallback::Value(e) => resident_safe_expr(e, callees),
                        TOrFallback::Return(None) => true,
                        TOrFallback::Return(Some(e)) => resident_safe_expr(e, callees),
                        TOrFallback::Panic { .. }
                        | TOrFallback::Break
                        | TOrFallback::Continue
                        | TOrFallback::BreakLabel(_)
                        | TOrFallback::ContinueLabel(_) => true,
                    }
            }
        }
        TExprKind::MapLit(entries) => {
            jit_map_string_type(&expr.ty)
                && entries.iter().all(|(k, v)| {
                    matches!(&k.ty, Type::String)
                        && jit_value_type(&v.ty)
                        && resident_safe_expr(k, callees)
                        && resident_safe_expr(v, callees)
                })
        }
        TExprKind::ListLit(elems) => {
            (jit_list_native_type(&expr.ty)
                && elems.iter().all(|e| {
                    matches!(&e.ty, Type::Int | Type::Float | Type::String | Type::Char)
                        && resident_safe_expr(e, callees)
                }))
                || (jit_list_of_int_list_type(&expr.ty)
                    && elems.iter().all(|e| resident_safe_expr(e, callees)))
                || (jit_list_task_int_type(&expr.ty)
                    && elems.iter().all(|e| resident_safe_expr(e, callees)))
                || (jit_list_record_type(&expr.ty)
                    && elems.iter().all(|e| resident_safe_expr(e, callees)))
        }
        TExprKind::Index {
            base,
            index,
            is_map,
            ..
        } => {
            if *is_map {
                jit_map_string_type(&base.ty)
                    && matches!(&index.ty, Type::String)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
            } else {
                (jit_list_native_type(&base.ty)
                    || jit_list_record_type(&base.ty)
                    || jit_list_iter_elem_type(&base.ty).is_some()
                    || jit_closure_elem_type(&base.ty).is_some())
                    && matches!(&index.ty, Type::Int)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
            }
        }
        TExprKind::Slice {
            base, start, end, ..
        } => {
            (jit_list_native_type(&base.ty) || jit_list_record_type(&base.ty))
                && matches!(&start.ty, Type::Int)
                && matches!(&end.ty, Type::Int)
                && resident_safe_expr(base, callees)
                && resident_safe_expr(start, callees)
                && resident_safe_expr(end, callees)
        }
        TExprKind::BuiltinMethod { recv, op, args } => {
            resident_safe_builtin_op(op, recv, args, callees)
        }
        TExprKind::NumericMethod { recv, op } => {
            resident_safe_expr(recv, callees)
                && match op {
                    TNumericOp::Predicate(name) => matches!(&recv.ty, Type::Float)
                        && matches!(name.as_str(), "is_nan" | "is_infinite" | "is_finite"),
                    TNumericOp::BitCount { method: name, width } => {
                        (*width == 64 && matches!(&recv.ty, Type::Int))
                            && matches!(
                                name.as_str(),
                                "count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros"
                            )
                    }
                    TNumericOp::ToShow => matches!(&recv.ty, Type::Int | Type::Float),
                    TNumericOp::CastAs { dst_rust } => {
                        recv.ty.is_numeric()
                            && matches!(dst_rust.as_str(), "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "f32" | "f64")
                    }
                    TNumericOp::FloatToInt { .. } | TNumericOp::FloatNarrow { .. } => recv.ty.is_float(),
                    TNumericOp::TryFrom { .. } => recv.ty.is_integer(),
                    TNumericOp::Origin => false,
                }
        }
        TExprKind::DistinctConvert { arg, .. } | TExprKind::DistinctRaw(arg) => {
            resident_safe_expr(arg, callees)
        }
        TExprKind::UnitConvert { arg, .. } => resident_safe_expr(arg, callees),
        TExprKind::PreciseBuiltin {
            type_name,
            func,
            args,
        } => {
            type_name == "BigInt"
                && matches!(
                    (func.as_str(), args.len()),
                    ("from_int" | "from_str", 1) | ("add" | "sub" | "mul", 2)
                )
                && args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        TExprKind::StructLit { fields, .. } => {
            jit_struct_type(&expr.ty)
                && fields
                    .iter()
                    .all(|(_, v, _)| resident_safe_expr(v, callees))
        }
        TExprKind::TupleLit { fields, .. } => {
            jit_tuple_type(&expr.ty) && resident_safe_tuple_fields(fields, callees)
        }
        TExprKind::Field { recv, .. } => match &recv.kind {
            TExprKind::Index {
                base,
                index,
                is_map,
                ..
            } => {
                !is_map
                    && (jit_list_record_type(&base.ty)
                        || matches!(
                            &base.ty,
                            Type::Apply { name, args }
                                if matches!(name.as_str(), "View" | "ViewMut")
                                    && args.len() == 1
                                    && record_type_key(&args[0]).is_some()
                        ))
                    && matches!(&index.ty, Type::Int)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
            }
            _ => resident_safe_expr(recv, callees),
        },
        TExprKind::MethodCall { recv, args, .. } => {
            resident_safe_expr(recv, callees)
                && args.iter().all(|a| resident_safe_call_arg(a, callees))
        }
        TExprKind::StaticCall { args, .. } => {
            args.iter().all(|a| resident_safe_call_arg(a, callees))
        }
        TExprKind::EnumLit { payload, .. } => {
            jit_enum_type(&expr.ty) && resident_safe_enum_payload(payload, callees)
        }
        TExprKind::Present(inner) | TExprKind::Ok(inner) | TExprKind::Err(inner) => {
            resident_safe_expr(inner, callees)
        }
        TExprKind::Try { inner, convert, .. } => {
            matches!(convert, TIR::TTryConvert::None) && resident_safe_expr(inner, callees)
        }
        TExprKind::Absent => true,
        TExprKind::DistinctCtor { arg, base, .. } => {
            jit_value_type(base) && resident_safe_expr(arg, callees)
        }
        _ if !jit_value_type(&expr.ty) => false,
        TExprKind::IntLit(_, _)
        | TExprKind::FloatLit(_)
        | TExprKind::BoolLit(_)
        | TExprKind::CharLit(_) => true,
        TExprKind::CtLit(jet_foundation::AST::CtValue::Int(_)) => true,
        TExprKind::StrLit(parts) => resident_safe_string_parts(parts, callees),
        TExprKind::Local(_) => true,
        TExprKind::Unary { op, operand } => {
            matches!(op, UnOp::Neg | UnOp::Not) && resident_safe_expr(operand, callees)
        }
        TExprKind::IncDec { ty, .. } => matches!(ty, Type::Int),
        TExprKind::Binary {
            op,
            overflow,
            lhs,
            rhs,
            ..
        } => {
            if matches!(op, BinOp::And | BinOp::Or) {
                return matches!(&lhs.ty, Type::Bool)
                    && matches!(&rhs.ty, Type::Bool)
                    && resident_safe_expr(lhs, callees)
                    && resident_safe_expr(rhs, callees);
            }
            if *overflow && (!matches!(&lhs.ty, Type::Int) || !matches!(&rhs.ty, Type::Int)) {
                return false;
            }
            resident_safe_expr(lhs, callees) && resident_safe_expr(rhs, callees)
        }
        TExprKind::CompareChain { operands, ops, hooks } => {
            operands.len() == ops.len() + 1
                && hooks.len() == ops.len()
                && operands.iter().all(|e| resident_safe_expr(e, callees))
                && ops.iter().enumerate().all(|(i, _)| {
                    hooks[i]
                        || matches!(&operands[i].ty, Type::Int | Type::Float)
                })
                && ops
                    .iter()
                    .all(|op| matches!(op, BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge))
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            matches!(cond.as_ref(), TIfCond::Plain(e) if matches!(&e.ty, Type::Bool) && resident_safe_expr(e, callees))
                && then_body.iter().all(|s| resident_safe_stmt(s, callees))
                && resident_safe_expr(then_value, callees)
                && else_body.iter().all(|s| resident_safe_stmt(s, callees))
                && resident_safe_expr(else_value, callees)
        }
        TExprKind::Clone(inner) => resident_safe_expr(inner, callees),
        TExprKind::Borrow { place, .. } => resident_safe_expr(place, callees),
        TExprKind::OptionLift2 { f, a, b } => {
            matches!(
                &f.kind,
                TExprKind::Lambda(lam)
                    if lam.prep.is_empty()
                        && lam.source_params.len() == 2
                        && matches!(
                            &lam.executable,
                            TIR::TLambdaBody::Expr(e) if resident_safe_expr(e, callees)
                        )
            ) && resident_safe_expr(a, callees)
                && resident_safe_expr(b, callees)
                && matches!(&expr.ty, Type::Option(inner) if jit_scalar_type(inner) || matches!(inner.as_ref(), Type::Named(_)|Type::Tuple(_)))
        }
        TExprKind::ClosureMethod { recv, op, args } => {
            resident_safe_closure_method(recv, op, args, callees)
        }
        TExprKind::TaskGroupAll { tasks } => {
            jit_list_int_type(&expr.ty) && resident_safe_task_list_expr(tasks, callees)
        }
        TExprKind::TaskGroupRace { tasks } | TExprKind::TaskGroupAny { tasks } => {
            matches!(&expr.ty, Type::Int) && resident_safe_task_list_expr(tasks, callees)
        }
        TExprKind::SelectStart => true,
        TExprKind::SelectRecv { builder, channel } => {
            resident_safe_expr(builder, callees)
                && jit_concurrency_type(&channel.ty)
                && resident_safe_expr(channel, callees)
        }
        TExprKind::SelectAfter {
            builder,
            millis,
            value,
        } => {
            resident_safe_expr(builder, callees)
                && matches!(&millis.ty, Type::Int)
                && resident_safe_expr(millis, callees)
                && value.as_ref().map_or(true, |v| {
                    matches!(&v.ty, Type::Int) && resident_safe_expr(v, callees)
                })
        }
        TExprKind::SelectRead { builder, .. } => resident_safe_expr(builder, callees),
        TExprKind::SelectWait { builder } => {
            jit_value_type(&expr.ty) && resident_safe_select_wait(builder, callees)
        }
        // D-OPTGC1: GcRead/GcEdit lower to Variable load/store of the payload
        // handle — same value snapshots AOT clones out of AutomaticRoot.
        TExprKind::HostCall(host) => match host.as_ref() {
            THostCall::GcRead { .. } => true,
            THostCall::GcEdit {
                edit,
                index_temp,
                ..
            } => {
                resident_safe_expr(edit, callees)
                    && index_temp
                        .as_ref()
                        .is_none_or(|(_, e)| resident_safe_expr(e, callees))
            }
            _ => false,
        },
        _ => false,
    }
}

fn resident_safe_call_arg(arg: &TCallArg, callees: &HashSet<String>) -> bool {
    // TIR marks non-scalar params as `borrow`; JIT passes them by handle/discriminant.
    (!arg.borrow || jit_value_type(&arg.value.ty))
        && !arg.mut_borrow
        && !arg.clone
        && !arg.arc_clone
        && arg.fn_coerce.is_none()
        && !arg.widen_to_vec
        && (!jit_struct_type(&arg.value.ty) || arg.borrow)
        && resident_safe_expr(&arg.value, callees)
}

fn resident_safe_unary_lambda(args: &[TExpr], callees: &HashSet<String>) -> bool {
    args.len() == 1
        && matches!(
            &args[0].kind,
            TExprKind::Lambda(lam)
                if lam.prep.is_empty()
                    && lam.source_params.len() == 1
                    && matches!(
                        &lam.executable,
                        TIR::TLambdaBody::Expr(e) if resident_safe_expr(e, callees)
                    )
        )
}

fn resident_safe_fold_lambda(args: &[TExpr], callees: &HashSet<String>) -> bool {
    args.len() == 2
        && matches!(&args[0].ty, Type::Int)
        && resident_safe_expr(&args[0], callees)
        && matches!(
            &args[1].kind,
            TExprKind::Lambda(lam)
                if lam.prep.is_empty()
                    && lam.source_params.len() == 2
                    && matches!(
                        &lam.executable,
                        TIR::TLambdaBody::Expr(e) if resident_safe_expr(e, callees)
                    )
        )
}

fn resident_safe_closure_method(
    recv: &TExpr,
    op: &TIR::TClosureOp,
    args: &[TExpr],
    callees: &HashSet<String>,
) -> bool {
    if !resident_safe_expr(recv, callees) {
        return false;
    }
    match op {
        TIR::TClosureOp::Map | TIR::TClosureOp::MapMut | TIR::TClosureOp::ViewMap => {
            jit_closure_elem_type(&recv.ty).is_some_and(|elem| {
                matches!(elem, Type::Int | Type::String | Type::Named(_))
            }) && resident_safe_unary_lambda(args, callees)
        }
        // D-HOLE1: Option.map — packed Option ABI; unary lambda over the payload.
        TIR::TClosureOp::OptionMap => {
            matches!(
                &recv.ty,
                Type::Option(inner)
                    if jit_scalar_type(inner)
                        || matches!(inner.as_ref(), Type::Named(_) | Type::Tuple(_))
            ) && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::Filter => {
            jit_closure_elem_type(&recv.ty).is_some_and(|elem| {
                matches!(elem, Type::Int | Type::String | Type::Named(_))
            }) && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::FilterMap => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::String))
                && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::SortBy => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::String))
                && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::TakeWhile
        | TIR::TClosureOp::SkipWhile
        | TIR::TClosureOp::Position => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::Int))
                && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::Fold | TIR::TClosureOp::Reduce | TIR::TClosureOp::ViewFold => {
            jit_closure_elem_type(&recv.ty)
                .or_else(|| jit_list_iter_elem_type(&recv.ty))
                .is_some_and(|elem| matches!(elem, Type::Int | Type::Named(_)))
                && resident_safe_fold_lambda(args, callees)
        }
        TIR::TClosureOp::MinBy | TIR::TClosureOp::MaxBy => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::String))
                && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::FlatMap => {
            jit_list_of_int_list_type(&recv.ty) && resident_safe_unary_lambda(args, callees)
        }
        _ => false,
    }
}

fn resident_safe_enum_payload(payload: &TEnumPayload, callees: &HashSet<String>) -> bool {
    match payload {
        TEnumPayload::Unit => true,
        TEnumPayload::Positional(vals) => vals.len() == 1
            && matches!(
                vals[0].value.ty,
                Type::Int | Type::Float | Type::Float32 | Type::Named(_)
            )
            && resident_safe_expr(&vals[0].value, callees),
        TEnumPayload::Named(fields) => fields
            .iter()
            .all(|(_, a)| resident_safe_expr(&a.value, callees)),
    }
}

fn resident_safe_tuple_fields(fields: &[(String, TExpr)], callees: &HashSet<String>) -> bool {
    fields.iter().all(|(_, value)| {
        matches!(&value.ty, Type::Int | Type::Float) && resident_safe_expr(value, callees)
    })
}

fn resident_safe_builtin_op(
    op: &TBuiltinOp,
    recv: &TExpr,
    args: &[TExpr],
    callees: &HashSet<String>,
) -> bool {
    if !resident_safe_expr(recv, callees) {
        return false;
    }
    match op {
        TBuiltinOp::LenString => matches!(&recv.ty, Type::String) && args.is_empty(),
        TBuiltinOp::Trim | TBuiltinOp::ToUpper | TBuiltinOp::ToLower => {
            matches!(&recv.ty, Type::String) && args.is_empty()
        }
        TBuiltinOp::Replace => {
            matches!(&recv.ty, Type::String)
                && args.len() == 2
                && args
                    .iter()
                    .all(|a| matches!(&a.ty, Type::String) && resident_safe_expr(a, callees))
        }
        TBuiltinOp::Push => {
            jit_list_native_type(&recv.ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int | Type::Float)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Sort => jit_list_int_type(&recv.ty) && args.is_empty(),
        TBuiltinOp::LenList => {
            (matches!(&recv.ty, Type::String)
                || jit_list_native_type(&recv.ty)
                || jit_list_iter_elem_type(&recv.ty).is_some()
                || jit_closure_elem_type(&recv.ty).is_some())
                && args.is_empty()
        }
        TBuiltinOp::GetList => {
            jit_list_native_type(&recv.ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::GetMap => {
            jit_map_string_type(&recv.ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::JoinSep => {
            (jit_list_native_type(&recv.ty) || jit_list_iter_elem_type(&recv.ty).is_some())
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::IsEmpty => {
            (jit_list_native_type(&recv.ty) || jit_list_iter_elem_type(&recv.ty).is_some())
                && args.is_empty()
        }
        TBuiltinOp::ParseInt | TBuiltinOp::ParseFloat => {
            matches!(&recv.ty, Type::String) && args.is_empty()
        }
        TBuiltinOp::Slice { .. } => {
            jit_list_int_type(&recv.ty)
                && args.len() == 2
                && matches!(&args[0].ty, Type::Int)
                && matches!(&args[1].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
                && resident_safe_expr(&args[1], callees)
        }
        TBuiltinOp::Lines => matches!(&recv.ty, Type::String) && args.is_empty(),
        TBuiltinOp::Split => {
            matches!(&recv.ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Chars => matches!(&recv.ty, Type::String) && args.is_empty(),
        TBuiltinOp::After | TBuiltinOp::Before => {
            matches!(&recv.ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        // JIT ABI: Iter producers already materialize list handles.
        TBuiltinOp::IterToList | TBuiltinOp::IterCollect => {
            (jit_list_iter_elem_type(&recv.ty).is_some()
                || jit_closure_elem_type(&recv.ty).is_some())
                && args.is_empty()
        }
        TBuiltinOp::Take | TBuiltinOp::Skip | TBuiltinOp::StepBy | TBuiltinOp::Chunks
        | TBuiltinOp::Windows => {
            jit_list_iter_elem_type(&recv.ty).is_some()
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Dedup => jit_list_iter_elem_type(&recv.ty).is_some() && args.is_empty(),
        TBuiltinOp::Sum { float: false } => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::Int)) && args.is_empty()
        }
        TBuiltinOp::TryCollect => {
            jit_result_list_elem(&recv.ty).is_some() && args.is_empty()
        }
        // D-HOLE1: Option.zip — both sides packed Option; builds a Present pair.
        TBuiltinOp::OptionZip { .. } => {
            matches!(&recv.ty, Type::Option(_))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Option(_))
                && resident_safe_expr(&args[0], callees)
        }
        // JIT ABI: View/ViewMut materialize as owned list handles (inclusive slice).
        TBuiltinOp::ViewNew { .. } | TBuiltinOp::ViewMutNew { .. } => {
            (jit_list_native_type(&recv.ty) || jit_list_record_type(&recv.ty))
                && args.len() == 2
                && matches!(&args[0].ty, Type::Int)
                && matches!(&args[1].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
                && resident_safe_expr(&args[1], callees)
        }
        _ => false,
    }
}

pub(crate) fn resident_safe_stmt(stmt: &TStmt, callees: &HashSet<String>) -> bool {
    match stmt {
        TStmt::Let { init, gc_promotion: _, gc_transferred: _, .. } => {
            // Promotion/transfer only wraps the same payload handle for the
            // collector; JIT stores the finite snapshot directly (D-OPTGC1).
            // `gc_transferred` is a call result that is already a root handle.
            resident_safe_expr(init, callees)
        }
        // D-TUPLE-DESTRUCT1: `(tx, rx) := tasks.channel<T>()` — the one
        // tuple-destructure shape this tier covers (general `TupleDestructure` /
        // `StructDestructure` / `ListDestructure` are not covered at all otherwise;
        // that's unrelated pre-existing scope, not narrowed here). Mirrors the old
        // single-handle `let ch := tasks.channel()` + `ch.sender()` shape exactly:
        // one `channel_new` call for the receiver handle, one `channel_sender` call
        // on it for the sender handle — same two host calls, now both fired at the
        // producer site instead of at a later `.sender()` call.
        TStmt::TupleDestructure { init, binds, .. } => {
            (binds.len() == 2
                && matches!(
                    &init.kind,
                    TExprKind::CoreCall { module, method, args, .. }
                        if module == "core.tasks" && method == "channel" && args.is_empty()
                ))
                || (jit_tuple_type(&init.ty)
                    && binds.len()
                        == match &init.ty {
                            Type::Tuple(fields) => fields.len(),
                            _ => 0,
                        }
                    && resident_safe_expr(init, callees))
        }
        TStmt::ListDestructure { init, elems, .. } => {
            jit_list_native_type(&init.ty)
                && !elems.is_empty()
                && resident_safe_expr(init, callees)
        }
        TStmt::Assign {
            place,
            op,
            value,
            clone_value,
        } => {
            let local = place.as_local().is_some_and(|local| {
                local
                    .name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            });
            let field = op.is_none() && structured_record_field_place(place);
            !clone_value && (local || field) && resident_safe_expr(value, callees)
        }
        TStmt::Return(ret) => ret.as_ref().is_none_or(|e| resident_safe_expr(e, callees)),
        TStmt::ExprStmt(e) => resident_safe_expr(e, callees),
        TStmt::If {
            cond,
            then_body,
            else_body,
            ..
        } => {
            let cond_ok = match cond {
                TIfCond::Plain(e) => matches!(&e.ty, Type::Bool) && resident_safe_expr(e, callees),
                TIfCond::IfLet { pattern, subj } => {
                    matches!(
                        &pattern.pattern,
                        Pattern::Ok { .. } | Pattern::Err { .. }
                    ) && matches!(&subj.ty, Type::Result { .. })
                        && resident_safe_expr(subj, callees)
                }
                TIfCond::Matches { .. } => false,
                TIfCond::And { .. } | TIfCond::IsNone { .. } => false,
            };
            cond_ok
                && then_body.iter().all(|s| resident_safe_stmt(s, callees))
                && else_body
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| resident_safe_stmt(s, callees)))
        }
        TStmt::Loop { body, .. } => body.iter().all(|s| resident_safe_stmt(s, callees)),
        TStmt::While { cond, body, .. } => {
            matches!(&cond.ty, Type::Bool)
                && resident_safe_expr(cond, callees)
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            resident_safe_stmt(init, callees)
                && matches!(&cond.ty, Type::Bool)
                && resident_safe_expr(cond, callees)
                && step.as_ref().is_none_or(|step| resident_safe_stmt(step, callees))
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::Range {
            start,
            end,
            step,
            body,
            ..
        } => {
            matches!(&start.ty, Type::Int)
                && matches!(&end.ty, Type::Int)
                && step.is_none()
                && resident_safe_expr(start, callees)
                && resident_safe_expr(end, callees)
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::Break(_) | TStmt::Continue(_) => true,
        TStmt::IndexAssign {
            base,
            index,
            is_map,
            value,
        } => {
            if *is_map {
                jit_map_string_type(&base.ty)
                    && matches!(&index.ty, Type::String)
                    && jit_value_type(&value.ty)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
                    && resident_safe_expr(value, callees)
            } else {
                (jit_list_native_type(&base.ty)
                    || jit_list_iter_elem_type(&base.ty).is_some()
                    || jit_closure_elem_type(&base.ty).is_some())
                    && matches!(&index.ty, Type::Int)
                    && matches!(&value.ty, Type::Int | Type::Float | Type::String | Type::Char)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
                    && resident_safe_expr(value, callees)
            }
        }
        TStmt::IndexFieldAssign(assign) => {
            !assign.is_map
                && !assign.clone_value
                && jit_list_record_type(&assign.base.ty)
                && matches!(&assign.index.ty, Type::Int)
                && assign
                    .op
                    .is_none_or(|_| matches!(&assign.field_ty, Type::Int | Type::Float))
                && resident_safe_expr(&assign.base, callees)
                && resident_safe_expr(&assign.index, callees)
                && resident_safe_expr(&assign.value, callees)
        }
        TStmt::ForIn {
            var2,
            source,
            collection,
            method_kind,
            columnar,
            by_value,
            step,
            body,
            ..
        } => {
            use jet_codegen::Codegen::TIR::TForInMethod;
            let chars_ok = matches!(method_kind, Some(TForInMethod::Chars))
                && matches!(&source.ty, Type::String);
            let list_ok = method_kind.is_none()
                && var2.is_none()
                && (jit_list_iter_elem_type(&collection.ty).is_some()
                    || jit_closure_elem_type(&collection.ty).is_some()
                    || jit_list_record_type(&collection.ty));
            let map_ok = method_kind.is_none()
                && var2.is_some()
                && jit_map_string_type(&collection.ty);
            // `by_value` marks Stream/Iter/HttpBodyChunks. Only Iter<T> is list-
            // backed under the JIT host ABI (true lazy handles don't cross).
            let by_value_ok = !*by_value
                || jet_foundation::Collections::is_iter_type(&collection.ty);
            (chars_ok || list_ok || map_ok)
                && !columnar
                && by_value_ok
                && resident_safe_expr(source, callees)
                && resident_safe_expr(collection, callees)
                && step.as_ref().is_none_or(|step| resident_safe_expr(step, callees))
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::EnumMatch {
            arms, else_body, ..
        } => {
            arms.iter()
                .all(|a| a.body.iter().all(|s| resident_safe_stmt(s, callees)))
                && else_body
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| resident_safe_stmt(s, callees)))
        }
        TStmt::MixedSwitch {
            arms, else_body, ..
        } => {
            arms.iter()
                .all(|(_, b)| b.iter().all(|s| resident_safe_stmt(s, callees)))
                && else_body
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| resident_safe_stmt(s, callees)))
        }
        TStmt::Region(body) | TStmt::Shield { body } => {
            body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        // JIT materializes split views as independent list slices / element copies.
        TStmt::SplitViews {
            owner,
            start: _,
            end: _,
            single: _,
            ..
        } => owner
            .as_ref()
            .is_none_or(|o| resident_safe_expr(o, callees)),
        // Whole-value GC assign (`replace_all`): nested stmt is a plain assign
        // of a finite payload snapshot into the root Variable.
        TStmt::GcEdit {
            replace_all,
            index_temp,
            stmt,
            ..
        } => {
            *replace_all
                && index_temp
                    .as_ref()
                    .is_none_or(|(_, e)| resident_safe_expr(e, callees))
                && resident_safe_stmt(stmt, callees)
        }
        _ => false,
    }
}

fn structured_record_field_place(place: &TIR::TPlace) -> bool {
    let TIR::TPlace::Expr(expr) = place else {
        return false;
    };
    matches!(
        &expr.kind,
        TIR::TExprKind::Field {
            recv,
            boxed: false,
            ..
        } if matches!(&recv.kind, TIR::TExprKind::Local(_))
    )
}

pub(crate) fn resident_safe_func(tir: &TFunc, callees: &HashSet<String>) -> bool {
    resident_safe_func_detail(tir, callees).is_none()
}

pub(crate) fn resident_safe_func_detail(tir: &TFunc, callees: &HashSet<String>) -> Option<String> {
    if !matches!(
        tir.kind,
        TFuncKind::TopLevel | TFuncKind::Method { .. } | TFuncKind::TraitMethod { .. }
    ) {
        return Some("not top-level".into());
    }
    if !tir.generics.is_empty() || tir.is_unsafe || tir.is_reactive {
        return Some("func attrs unsupported".into());
    }
    if !tir.params.iter().all(|(_, ty, _)| jit_value_type(ty)) {
        return Some("param type unsupported".into());
    }
    if let Some(ret) = &tir.ret {
        if !jit_value_type(ret) {
            return Some("return type unsupported".into());
        }
    }
    for (i, s) in tir.body.iter().enumerate() {
        if !resident_safe_stmt(s, callees) {
            return Some(format!("body stmt {i}"));
        }
    }
    None
}

pub(crate) fn resident_safe_program(program: &JitProgram) -> bool {
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let main_ok = program.funcs.iter().any(|f| {
        f.name == program.entry
            && f.params.is_empty()
            && (f.ret.is_none()
                || matches!(&f.ret, Some(Type::Result { ok, err })
                    if matches!(ok.as_ref(), Type::Named(n) if n == "Void" || n == "Unit")
                        && matches!(err.as_ref(), Type::String | Type::Named(_))))
            && resident_safe_func(f, &names)
    });
    if !main_ok {
        return false;
    }
    if !program.funcs.iter().all(|f| resident_safe_func(f, &names)) {
        return false;
    }
    let spawn_sites = count_spawn_sites(program);
    if spawn_sites != program.spawn_lambdas.len() {
        return false;
    }
    program
        .spawn_lambdas
        .iter()
        .all(|lam| resident_safe_spawn_lambda(lam, &names))
}

pub(crate) fn count_spawn_sites(program: &JitProgram) -> usize {
    let mut n = 0usize;
    for f in &program.funcs {
        count_spawn_sites_stmts(&f.body, &mut n);
    }
    n
}

fn count_spawn_sites_stmts(stmts: &[TStmt], n: &mut usize) {
    for s in stmts {
        match s {
            TStmt::Let { init, .. }
            | TStmt::Assign { value: init, .. }
            | TStmt::Return(Some(init))
            | TStmt::ExprStmt(init) => count_spawn_sites_expr(init, n),
            TStmt::IndexFieldAssign(assign) => {
                count_spawn_sites_expr(&assign.base, n);
                count_spawn_sites_expr(&assign.index, n);
                count_spawn_sites_expr(&assign.value, n);
            }
            TStmt::If {
                then_body,
                else_body,
                ..
            } => {
                count_spawn_sites_stmts(then_body, n);
                if let Some(b) = else_body {
                    count_spawn_sites_stmts(b, n);
                }
            }
            TStmt::Loop { body, .. } | TStmt::While { body, .. } | TStmt::Range { body, .. } => {
                count_spawn_sites_stmts(body, n)
            }
            TStmt::CountedLoop {
                init, step, body, ..
            } => {
                count_spawn_sites_stmts(std::slice::from_ref(init), n);
                if let Some(step) = step {
                    count_spawn_sites_stmts(std::slice::from_ref(step), n);
                }
                count_spawn_sites_stmts(body, n);
            }
            TStmt::Region(body) => count_spawn_sites_stmts(body, n),
            TStmt::ForIn { body, .. } => count_spawn_sites_stmts(body, n),
            TStmt::EnumMatch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    count_spawn_sites_stmts(&arm.body, n);
                }
                if let Some(b) = else_body {
                    count_spawn_sites_stmts(b, n);
                }
            }
            TStmt::MixedSwitch {
                arms, else_body, ..
            } => {
                for (_, b) in arms {
                    count_spawn_sites_stmts(b, n);
                }
                if let Some(b) = else_body {
                    count_spawn_sites_stmts(b, n);
                }
            }
            _ => {}
        }
    }
}

fn count_spawn_sites_expr(expr: &TExpr, n: &mut usize) {
    if matches!(
        expr.kind,
        TExprKind::CoreClosureCall {
            kind: TCoreClosureKind::Spawn { .. }
        }
    ) {
        *n += 1;
    }
    match &expr.kind {
        TExprKind::Print(inner)
        | TExprKind::Unary { operand: inner, .. }
        | TExprKind::Clone(inner)
        | TExprKind::Ok(inner)
        | TExprKind::Err(inner) => count_spawn_sites_expr(inner, n),
        TExprKind::Binary { lhs, rhs, .. } => {
            count_spawn_sites_expr(lhs, n);
            count_spawn_sites_expr(rhs, n);
        }
        TExprKind::Call { args, .. } => {
            for a in args {
                count_spawn_sites_expr(&a.value, n);
            }
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            count_spawn_sites_if_cond(cond, n);
            count_spawn_sites_stmts(then_body, n);
            count_spawn_sites_expr(then_value, n);
            count_spawn_sites_stmts(else_body, n);
            count_spawn_sites_expr(else_value, n);
        }
        TExprKind::HandleMethod { recv, args, .. } => {
            count_spawn_sites_expr(recv, n);
            for a in args {
                count_spawn_sites_expr(a, n);
            }
        }
        TExprKind::OrFallback { value, .. } => count_spawn_sites_expr(value, n),
        TExprKind::TaskGroupAll { tasks }
        | TExprKind::TaskGroupRace { tasks }
        | TExprKind::TaskGroupAny { tasks } => count_spawn_sites_expr(tasks, n),
        _ => {}
    }
}

fn count_spawn_sites_if_cond(cond: &TIfCond, n: &mut usize) {
    match cond {
        TIfCond::Plain(expr) => count_spawn_sites_expr(expr, n),
        TIfCond::And { left, right } => {
            count_spawn_sites_if_cond(left, n);
            count_spawn_sites_if_cond(right, n);
        }
        TIfCond::IfLet { subj, .. }
        | TIfCond::IsNone { subj }
        | TIfCond::Matches { subj, .. } => count_spawn_sites_expr(subj, n),
    }
}

fn resident_safe_expr_list(exprs: &[TExpr], callees: &HashSet<String>) -> bool {
    exprs.iter().all(|e| resident_safe_expr(e, callees))
}

fn resident_safe_task_list_expr(tasks: &TExpr, callees: &HashSet<String>) -> bool {
    jit_list_task_int_type(&tasks.ty) && resident_safe_expr(tasks, callees)
}

fn resident_safe_select_wait(builder: &TExpr, callees: &HashSet<String>) -> bool {
    let (recvs, afters) = collect_select_arms_jit(builder);
    !recvs.is_empty()
        && recvs
            .iter()
            .all(|ch| jit_concurrency_type(&ch.ty) && resident_safe_expr(ch, callees))
        && afters.iter().all(|(ms, value)| {
            matches!(&ms.ty, Type::Int) && resident_safe_expr(ms, callees) && value.is_none()
        })
}

pub(crate) fn collect_select_arms_jit<'a>(
    builder: &'a TExpr,
) -> (Vec<&'a TExpr>, Vec<(&'a TExpr, Option<&'a TExpr>)>) {
    let mut recvs = Vec::new();
    let mut afters = Vec::new();
    let mut cur = builder;
    loop {
        match &cur.kind {
            TExprKind::SelectStart => break,
            TExprKind::SelectRecv {
                builder: inner,
                channel,
            } => {
                recvs.push(channel.as_ref());
                cur = inner;
            }
            TExprKind::SelectAfter {
                builder: inner,
                millis,
                value,
            } => {
                afters.push((millis.as_ref(), value.as_deref()));
                cur = inner;
            }
            TExprKind::SelectRead { builder: inner, .. } => {
                cur = inner;
            }
            _ => break,
        }
    }
    (recvs, afters)
}

pub(crate) fn resident_safe_spawn_lambda(lam: &TJitSpawnLambda, callees: &HashSet<String>) -> bool {
    if lam.captures.len() > 4 {
        return false;
    }
    if !lam
        .captures
        .iter()
        .all(|c| jit_value_type(&c.ty) && resident_safe_capture_policy(c))
    {
        return false;
    }
    if !lam.params.iter().all(|(_, ty)| jit_value_type(ty)) {
        return false;
    }
    if !jit_value_type(&lam.ret) {
        return false;
    }
    match &lam.body {
        TJitSpawnBody::Expr(e) => resident_safe_expr(e, callees),
        TJitSpawnBody::Block { prefix, tail } => {
            prefix.iter().all(|s| resident_safe_stmt(s, callees))
                && tail.as_ref().is_none_or(|t| resident_safe_expr(t, callees))
        }
    }
}

fn resident_safe_capture_policy(c: &JitSpawnCapture) -> bool {
    if c.clone_at_spawn {
        matches!(&c.ty, Type::Apply { name, .. } if name == "Sender")
    } else {
        true
    }
}

fn resident_safe_handle_op(op: &THandleOp, recv: &TExpr, args: &[TExpr]) -> bool {
    match op {
        THandleOp::TaskJoin | THandleOp::TaskCancel => {
            args.is_empty() && jit_concurrency_type(&recv.ty)
        }
        THandleOp::ChannelReceive => {
            args.is_empty() && matches!(&recv.ty, Type::Apply { name, .. } if name == "Receiver")
        }
        THandleOp::SenderSend => {
            args.len() == 1 && matches!(&recv.ty, Type::Apply { name, .. } if name == "Sender")
        }
        THandleOp::SolverNew => args.is_empty() && recv.ty == Type::Int,
        THandleOp::SolverRequire => args.len() == 1 && recv.ty == Type::Named("Solver".into()) && args[0].ty == Type::Bool,
        THandleOp::SolverFailureCount | THandleOp::SolverStatus => {
            args.is_empty() && recv.ty == Type::Named("Solver".into())
        }
        THandleOp::PreciseMethod { type_name, method } => {
            type_name == "BigInt"
                && recv.ty == Type::Named("BigInt".into())
                && matches!(
                    (method.as_str(), args.len()),
                    ("add" | "sub" | "mul", 1) | ("neg" | "to_string", 0)
                )
        }
        _ => false,
    }
}
