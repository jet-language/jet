use jet_codegen::Codegen::TIR::{
    self, JitProgram, JitSpawnCapture, TBuiltinOp, TCallArg, TCoreClosureKind, TEnumPayload, TExpr,
    TExprKind, TFunc, TFuncKind, THandleOp, TIfCond, TJitSpawnBody, TJitSpawnLambda, TOrFallback,
    TStmt, TStrPart,
};
use jet_foundation::AST::{BinOp, Type, UnOp};
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
}

pub(crate) fn jit_list_float_type(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if matches!(inner.as_ref(), Type::Float))
}

fn jit_list_native_type(ty: &Type) -> bool {
    jit_list_int_type(ty) || jit_list_float_type(ty)
}

pub(crate) fn jit_list_iter_elem_type(ty: &Type) -> Option<Type> {
    match ty {
        Type::List(inner) if matches!(inner.as_ref(), Type::Int | Type::Float) => {
            Some(inner.as_ref().clone())
        }
        _ => None,
    }
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

pub(crate) fn jit_tuple_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(fields) if fields.iter().all(|(_, t)| matches!(t.as_ref(), Type::Int | Type::Float)))
}

pub(crate) fn jit_enum_type(ty: &Type) -> bool {
    user_type_name(ty).is_some()
}

fn jit_compound_type(ty: &Type) -> bool {
    jit_list_native_type(ty)
        || jit_list_task_int_type(ty)
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
    match ty {
        Type::Named(n) if n == "Unit" => true,
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => true,
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
        TExprKind::CoreCall {
            module,
            method,
            args,
        } => {
            // D-TUPLE-DESTRUCT1: `tasks.channel<T>()` now returns a `(Sender<T>,
            // Receiver<T>)` tuple bound via a tuple-destructure `let` — a statement
            // shape this tier has no representation for at all (never covered), so
            // the producer itself is never claimed here either. Falls back to the
            // tier-0 interpreter (JIT is an optional accelerator, not load-bearing).
            if module == "core.tasks" && method == "channel" {
                return false;
            }
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
                    && matches!(fallback, TOrFallback::Value(_) | TOrFallback::Panic(_))
            } else {
                !is_option
                    && resident_safe_expr(value, callees)
                    && matches!(fallback, TOrFallback::Panic(_))
            }
        }
        TExprKind::ListLit(elems) => {
            (jit_list_native_type(&expr.ty)
                && elems.iter().all(|e| {
                    matches!(&e.ty, Type::Int | Type::Float) && resident_safe_expr(e, callees)
                }))
                || (jit_list_task_int_type(&expr.ty)
                    && elems.iter().all(|e| resident_safe_expr(e, callees)))
        }
        TExprKind::Index {
            base,
            index,
            is_map,
            ..
        } => {
            !is_map
                && jit_list_native_type(&base.ty)
                && matches!(&index.ty, Type::Int)
                && resident_safe_expr(base, callees)
                && resident_safe_expr(index, callees)
        }
        TExprKind::Slice {
            base, start, end, ..
        } => {
            jit_list_native_type(&base.ty)
                && matches!(&start.ty, Type::Int)
                && matches!(&end.ty, Type::Int)
                && resident_safe_expr(base, callees)
                && resident_safe_expr(start, callees)
                && resident_safe_expr(end, callees)
        }
        TExprKind::BuiltinMethod { recv, op, args } => {
            resident_safe_builtin_op(op, recv, args, callees)
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
        TExprKind::Field { recv, .. } => resident_safe_expr(recv, callees),
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
        _ if !jit_value_type(&expr.ty) => false,
        TExprKind::IntLit(_, _)
        | TExprKind::FloatLit(_)
        | TExprKind::BoolLit(_)
        | TExprKind::CharLit(_) => true,
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
        TExprKind::CompareChain { operands, ops } => {
            operands.len() == ops.len() + 1
                && operands.iter().all(|e| {
                    matches!(&e.ty, Type::Int | Type::Float) && resident_safe_expr(e, callees)
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
            matches!(&cond.ty, Type::Bool)
                && then_body.iter().all(|s| resident_safe_stmt(s, callees))
                && resident_safe_expr(then_value, callees)
                && else_body.iter().all(|s| resident_safe_stmt(s, callees))
                && resident_safe_expr(else_value, callees)
        }
        TExprKind::Clone(inner) => resident_safe_expr(inner, callees),
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

fn resident_safe_enum_payload(payload: &TEnumPayload, callees: &HashSet<String>) -> bool {
    match payload {
        TEnumPayload::Unit => true,
        TEnumPayload::Positional(vals) => {
            vals.iter().all(|a| resident_safe_expr(&a.value, callees))
        }
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
        TBuiltinOp::LenList => jit_list_native_type(&recv.ty) && args.is_empty(),
        TBuiltinOp::GetList => {
            jit_list_int_type(&recv.ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::JoinSep => {
            jit_list_native_type(&recv.ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Slice { .. } => {
            jit_list_int_type(&recv.ty)
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
        TStmt::Let { init, .. } => resident_safe_expr(init, callees),
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
                    TExprKind::CoreCall { module, method, args }
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
        TStmt::Assign {
            value, clone_value, ..
        } => !clone_value && resident_safe_expr(value, callees),
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
                TIfCond::Matches { .. } => false,
                TIfCond::IfLet { .. } | TIfCond::IsNone { .. } => false,
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
                && resident_safe_stmt(step, callees)
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
                && step.as_ref().is_none_or(|s| matches!(&s.ty, Type::Int))
                && resident_safe_expr(start, callees)
                && resident_safe_expr(end, callees)
                && step.as_ref().is_none_or(|s| resident_safe_expr(s, callees))
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::Break(_) | TStmt::Continue(_) => true,
        TStmt::IndexAssign {
            base,
            index,
            is_map,
            value,
        } => {
            !is_map
                && jit_list_native_type(&base.ty)
                && matches!(&index.ty, Type::Int)
                && matches!(&value.ty, Type::Int | Type::Float)
                && resident_safe_expr(base, callees)
                && resident_safe_expr(index, callees)
                && resident_safe_expr(value, callees)
        }
        TStmt::ForIn {
            var2,
            method_kind,
            columnar,
            body,
            ..
        } => {
            var2.is_none()
                && method_kind.is_none()
                && !columnar
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
        _ => false,
    }
}

pub(crate) fn resident_safe_func(tir: &TFunc, callees: &HashSet<String>) -> bool {
    resident_safe_func_detail(tir, callees).is_none()
}

pub(crate) fn resident_safe_func_detail(tir: &TFunc, callees: &HashSet<String>) -> Option<String> {
    if !matches!(tir.kind, TFuncKind::TopLevel | TFuncKind::Method { .. }) {
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
        f.name == "run"
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
                count_spawn_sites_stmts(std::slice::from_ref(step), n);
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
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
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
        _ => false,
    }
}
