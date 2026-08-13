use crate::jet_generated_format as jet_format;
use crate::AST::{BindPattern, Expr, ForKind, IndexKind, LValue, PlaceAccess, Stmt, Type, UnOp};
use crate::Codegen::Cx;
use crate::Codegen::mangle_generated;
#[cfg(test)]
use crate::Codegen::build_cx;
#[cfg(test)]
use crate::Diagnostics::Span;
use crate::Codegen::mangle;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::label_name;
use crate::Codegen::TIR::lower::collect_txn_mut_roots;
use crate::Codegen::TIR::lower::encoding_reader_item_type;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_owned_expr;
use crate::Codegen::TIR::lower_forin_collection;
use crate::Codegen::TIR::lower::lower_string_view_init;
use crate::Codegen::TIR::lower::reactive_block_env;
use crate::Codegen::TIR::lower::render_reactive_block_closure;
use crate::Codegen::TIR::lower::lower_lambda_with_shared_block;
use crate::Codegen::TIR::lower::lower_spawn_lambda_for_jit_with_shared_block;
use crate::Codegen::TIR::lower_switch;
use crate::Codegen::TIR::struct_field_type;
use crate::Codegen::TIR::lower::timeout_nanos;
use crate::Codegen::TIR::lower::tracked_float_slot;
use crate::Codegen::TIR::tir_recv_jet_ty;
use crate::Codegen::TIR::ScopeMemberKind;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TirWorklist;
use crate::Codegen::TIR::TLetTy;
use crate::Codegen::TIR::TFnValueKind;
use crate::Codegen::TIR::TForInMethod;
use crate::Codegen::TIR::TIndexFieldAssign;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TBindingOrigin;
use crate::Codegen::TIR::TPlace;
use crate::Codegen::TIR::TStmt;
use crate::Codegen::TIR::lower::lower_comptime_scalar;
use crate::Codegen::TIR::unit_type;
use crate::Syntax;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Preserve contextual union typing when a sema-resolved comptime value stays
/// as a `CtLit`. The literal must remain a single fact for JIT/interpreter, but
/// its serialized AOT form still needs the generated union enum wrapper.
fn bake_comptime_value_with_type(
    value: &crate::AST::CtValue,
    ty: &Type,
    cx: &Cx,
) -> crate::AST::CtValue {
    use crate::AST::CtValue;

    match ty {
        Type::Union(members) => {
            let enum_name = crate::AST::union_enum_name(members);
            if matches!(
                value,
                CtValue::Enum { type_name, .. } if type_name == &enum_name
            ) {
                return value.clone();
            }
            let Some(member) = members.iter().find(|member| value.jet_type() == **member) else {
                return value.clone();
            };
            CtValue::Enum {
                type_name: enum_name,
                variant: crate::AST::union_member_tag(member),
                args: vec![(
                    None,
                    bake_comptime_value_with_type(value, member, cx),
                )],
            }
        }
        Type::Named(_) | Type::Apply { .. } => {
            let CtValue::Struct { type_name, fields } = value else {
                return value.clone();
            };
            let typed_fields = fields
                .iter()
                .map(|(field, value)| {
                    let field_ty = struct_field_type(cx, ty, field).or_else(|| {
                        struct_field_type(cx, &Type::Named(type_name.clone()), field)
                    });
                    let value = field_ty
                        .as_ref()
                        .map(|field_ty| bake_comptime_value_with_type(value, field_ty, cx))
                        .unwrap_or_else(|| value.clone());
                    (field.clone(), value)
                })
                .collect();
            CtValue::Struct {
                type_name: type_name.clone(),
                fields: typed_fields,
            }
        }
        _ => value.clone(),
    }
}

fn interrupt_callback_ident(expr: &Expr) -> Option<&str> {
    let mut expr = expr;
    loop {
        match expr {
            Expr::Ident(name, _) => return Some(name),
            Expr::Paren(inner, _) => expr = inner,
            _ => return None,
        }
    }
}

fn interrupt_lambda(expr: &Expr) -> Option<&crate::AST::Lambda> {
    match expr {
        Expr::Lambda(lam) => Some(lam),
        Expr::Paren(inner, _) => interrupt_lambda(inner),
        _ => None,
    }
}

fn interrupt_lambda_captures(lam: &crate::AST::Lambda) -> HashSet<String> {
    match &lam.body {
        crate::AST::LambdaBody::Expr(body) => {
            crate::Sema::block_free_var_reads(&[Stmt::Expr((**body).clone())])
        }
        crate::AST::LambdaBody::Block(body) => crate::Sema::block_free_var_reads(body),
    }
}

fn is_core_os_receiver(expr: &Expr, cx: &Cx) -> bool {
    let mut expr = expr;
    loop {
        match expr {
            Expr::Ident(alias, _) => {
                return cx
                    .any_core_import_module(alias)
                    .is_some_and(|module| module == "core.os");
            }
            Expr::Field(base, leaf, _) if leaf == "os" => expr = base,
            _ => return false,
        }
    }
}

enum InterruptScanTask<'a> {
    Expr(&'a Expr),
    Stmts(&'a [Stmt]),
}

fn collect_interrupt_callback_names_expr(
    expr: &Expr,
    cx: &Cx,
    names: &mut HashSet<String>,
) {
    collect_interrupt_callback_scan(InterruptScanTask::Expr(expr), cx, names);
}

fn collect_interrupt_callback_names(stmts: &[Stmt], cx: &Cx, names: &mut HashSet<String>) {
    collect_interrupt_callback_scan(InterruptScanTask::Stmts(stmts), cx, names);
}

fn collect_interrupt_callback_scan(
    root: InterruptScanTask<'_>,
    cx: &Cx,
    names: &mut HashSet<String>,
) {
    let mut work = TirWorklist::new();
    work.push(root);
    while let Some(task) = work.pop() {
        match task {
            InterruptScanTask::Expr(expr) => match expr {
                Expr::MethodCall {
                    receiver,
                    method,
                    args,
                    ..
                } => {
                    if method == "on_interrupt" && is_core_os_receiver(receiver, cx) {
                        if let Some(callback) = args.first().map(|arg| &arg.expr) {
                            if let Some(name) = interrupt_callback_ident(callback) {
                                names.insert(name.to_string());
                            }
                            if let Some(lam) = interrupt_lambda(callback) {
                                names.extend(interrupt_lambda_captures(lam));
                            }
                        }
                    }
                    for arg in args.iter().rev() {
                        work.push(InterruptScanTask::Expr(&arg.expr));
                    }
                    work.push(InterruptScanTask::Expr(receiver));
                }
                Expr::Call(call) => {
                    for arg in call.args.iter().rev() {
                        work.push(InterruptScanTask::Expr(&arg.expr));
                    }
                }
                Expr::CallValue { callee, args, .. } => {
                    for arg in args.iter().rev() {
                        work.push(InterruptScanTask::Expr(&arg.expr));
                    }
                    work.push(InterruptScanTask::Expr(callee));
                }
                // A normal lambda is its own function boundary. Its body gets one scan
                // when that lambda is lowered; collecting/result loops are inline blocks
                // in the current function and therefore remain part of this scan.
                Expr::Lambda(lam)
                    if lam.meta.collecting_loop || lam.meta.result_loop => match &lam.body {
                    crate::AST::LambdaBody::Expr(body) => {
                        work.push(InterruptScanTask::Expr(body));
                    }
                    crate::AST::LambdaBody::Block(body) => {
                        work.push(InterruptScanTask::Stmts(body));
                    }
                },
                Expr::Lambda(_) => {}
                Expr::Paren(inner, _)
                | Expr::Unary(_, inner, _)
                | Expr::Deref(inner, _)
                | Expr::RawOf(inner, _)
                | Expr::Copy(inner, _)
                | Expr::Place(inner, _, _)
                | Expr::Tainted(inner, _, _)
                | Expr::Present(inner, _)
                | Expr::Ok(inner, _)
                | Expr::Err(inner, _)
                | Expr::Spread(inner, _)
                | Expr::IncDec { operand: inner, .. } => {
                    work.push(InterruptScanTask::Expr(inner));
                }
                Expr::Try(inner, _, _, note) => {
                    work.push(InterruptScanTask::Expr(inner));
                    if let Some(note) = note {
                        work.push(InterruptScanTask::Expr(note));
                    }
                }
                Expr::Binary(_, left, right, _) => {
                    work.push(InterruptScanTask::Expr(right));
                    work.push(InterruptScanTask::Expr(left));
                }
                Expr::CompareChain { operands, .. } => {
                    for operand in operands.iter().rev() {
                        work.push(InterruptScanTask::Expr(operand));
                    }
                }
                Expr::ListLit(items, _) => {
                    for item in items.iter().rev() {
                        work.push(InterruptScanTask::Expr(item));
                    }
                }
                Expr::MemberSpread { base, .. } => {
                    work.push(InterruptScanTask::Expr(base));
                }
                Expr::MapLit(entries, _) => {
                    for (key, value) in entries.iter().rev() {
                        work.push(InterruptScanTask::Expr(value));
                        work.push(InterruptScanTask::Expr(key));
                    }
                }
                Expr::Index { base, index, .. } => {
                    work.push(InterruptScanTask::Expr(index));
                    work.push(InterruptScanTask::Expr(base));
                }
                Expr::Slice {
                    base,
                    start,
                    end,
                    range,
                    ..
                } => {
                    if let Some(range) = range {
                        work.push(InterruptScanTask::Expr(range));
                    }
                    work.push(InterruptScanTask::Expr(end));
                    work.push(InterruptScanTask::Expr(start));
                    work.push(InterruptScanTask::Expr(base));
                }
                Expr::Range { start, end, .. } => {
                    work.push(InterruptScanTask::Expr(end));
                    work.push(InterruptScanTask::Expr(start));
                }
                Expr::Field(base, _, _) | Expr::OptField { base, .. } => {
                    work.push(InterruptScanTask::Expr(base));
                }
                Expr::StructLit { fields, .. } => {
                    for (_, _, value) in fields.iter().rev() {
                        work.push(InterruptScanTask::Expr(value));
                    }
                }
                Expr::TypedLit { body, .. } => match body {
                    crate::AST::TypedLitBody::Fields(fields) => {
                        for (_, _, value) in fields.iter().rev() {
                            work.push(InterruptScanTask::Expr(value));
                        }
                    }
                    crate::AST::TypedLitBody::Elements(elements) => {
                        for value in elements.iter().rev() {
                            work.push(InterruptScanTask::Expr(value));
                        }
                    }
                    crate::AST::TypedLitBody::Entries(entries) => {
                        for (key, value) in entries.iter().rev() {
                            work.push(InterruptScanTask::Expr(value));
                            work.push(InterruptScanTask::Expr(key));
                        }
                    }
                    crate::AST::TypedLitBody::Value(value) => {
                        work.push(InterruptScanTask::Expr(value));
                    }
                    crate::AST::TypedLitBody::Empty => {}
                },
                Expr::EnumLit { args, .. } => {
                    for arg in args.iter().rev() {
                        let value = match arg {
                            crate::AST::EnumLitArg::Positional(value)
                            | crate::AST::EnumLitArg::Named { expr: value, .. } => value,
                        };
                        work.push(InterruptScanTask::Expr(value));
                    }
                }
                Expr::Str(parts, _) => {
                    for part in parts.iter().rev() {
                        if let crate::AST::StrPart::Interp(value, _) = part {
                            work.push(InterruptScanTask::Expr(value));
                        }
                    }
                }
                Expr::PatternTest { subject, .. } => {
                    work.push(InterruptScanTask::Expr(subject));
                }
                Expr::If {
                    cond,
                    then_body,
                    then_value,
                    else_body,
                    else_value,
                    ..
                } => {
                    work.push(InterruptScanTask::Expr(else_value));
                    work.push(InterruptScanTask::Stmts(else_body));
                    work.push(InterruptScanTask::Expr(then_value));
                    work.push(InterruptScanTask::Stmts(then_body));
                    work.push(InterruptScanTask::Expr(cond));
                }
                Expr::TupleLit(fields, _, _) => {
                    for (_, value) in fields.iter().rev() {
                        work.push(InterruptScanTask::Expr(value));
                    }
                }
                Expr::PtrFromAddr { addr, .. } => {
                    work.push(InterruptScanTask::Expr(addr));
                }
                Expr::OrFallback { value, .. } => {
                    work.push(InterruptScanTask::Expr(value));
                }
                _ => {}
            },
            InterruptScanTask::Stmts(stmts) => {
                for stmt in stmts.iter().rev() {
                    match stmt {
                        Stmt::Expr(expr) => work.push(InterruptScanTask::Expr(expr)),
                        Stmt::Val(binding) => {
                            work.push(InterruptScanTask::Expr(&binding.init));
                        }
                        Stmt::Assign { value, .. } => {
                            work.push(InterruptScanTask::Expr(value));
                        }
                        Stmt::Return(Some(value), _) | Stmt::Yield(value, _) => {
                            work.push(InterruptScanTask::Expr(value));
                        }
                        Stmt::While { cond, body, .. } => {
                            work.push(InterruptScanTask::Stmts(body));
                            work.push(InterruptScanTask::Expr(cond));
                        }
                        Stmt::For { kind, body, .. } => {
                            work.push(InterruptScanTask::Stmts(body));
                            if let ForKind::In { collection, step } = kind {
                                if let Some(step) = step {
                                    work.push(InterruptScanTask::Expr(step));
                                }
                                work.push(InterruptScanTask::Expr(collection));
                            }
                        }
                        Stmt::Switch {
                            subject,
                            arms,
                            else_body,
                            ..
                        } => {
                            if let Some(body) = else_body {
                                work.push(InterruptScanTask::Stmts(body));
                            }
                            for arm in arms.iter().rev() {
                                work.push(InterruptScanTask::Stmts(&arm.body));
                                work.push(InterruptScanTask::Expr(&arm.cond));
                            }
                            work.push(InterruptScanTask::Expr(subject));
                        }
                        Stmt::Loop { body, .. }
                        | Stmt::Unsafe { body, .. }
                        | Stmt::Impure { body, .. }
                        | Stmt::Reactive { body, .. }
                        | Stmt::Shield { body, .. }
                        | Stmt::Switched { body, .. }
                        | Stmt::Region { body, .. }
                        | Stmt::Policy { body, .. }
                        | Stmt::TaskGroup { body, .. }
                        | Stmt::Layout { body, .. }
                        | Stmt::Caps { body, .. }
                        | Stmt::Grant { body, .. }
                        | Stmt::ContextBlock { body, .. }
                        | Stmt::Live { body, .. }
                        | Stmt::AssumeDet { body, .. }
                        | Stmt::Transact { body, .. }
                        | Stmt::ComptimeBlock { body, .. } => {
                            work.push(InterruptScanTask::Stmts(body));
                        }
                        Stmt::CountedLoop {
                            init,
                            cond,
                            step,
                            body,
                            ..
                        } => {
                            work.push(InterruptScanTask::Stmts(body));
                            if let Some(step) = step {
                                work.push(InterruptScanTask::Stmts(
                                    std::slice::from_ref(step.as_ref()),
                                ));
                            }
                            work.push(InterruptScanTask::Expr(cond));
                            work.push(InterruptScanTask::Expr(&init.init));
                        }
                        Stmt::ComptimeIf {
                            cond,
                            then_body,
                            else_body,
                            ..
                        } => {
                            if let Some(body) = else_body {
                                work.push(InterruptScanTask::Stmts(body));
                            }
                            work.push(InterruptScanTask::Stmts(then_body));
                            work.push(InterruptScanTask::Expr(cond));
                        }
                        Stmt::ScopeMember { args, body, .. } => {
                            work.push(InterruptScanTask::Stmts(body));
                            for arg in args.iter().rev() {
                                work.push(InterruptScanTask::Expr(arg));
                            }
                        }
                        Stmt::ComptimeSwitch {
                            subject,
                            arms,
                            else_body,
                            ..
                        } => {
                            if let Some(body) = else_body {
                                work.push(InterruptScanTask::Stmts(body));
                            }
                            for arm in arms.iter().rev() {
                                work.push(InterruptScanTask::Stmts(&arm.body));
                            }
                            work.push(InterruptScanTask::Expr(subject));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn collect_interrupt_aliases_expr(expr: &Expr, aliases: &mut Vec<(String, String)>) {
    match expr {
        Expr::Lambda(lambda) => match &lambda.body {
            crate::AST::LambdaBody::Expr(body) => {
                collect_interrupt_aliases_expr(body, aliases)
            }
            crate::AST::LambdaBody::Block(body) => collect_interrupt_aliases(body, aliases),
        },
        Expr::MethodCall { receiver, args, .. } => {
            collect_interrupt_aliases_expr(receiver, aliases);
            for arg in args {
                collect_interrupt_aliases_expr(&arg.expr, aliases);
            }
        }
        Expr::Call(call) => {
            for arg in &call.args {
                collect_interrupt_aliases_expr(&arg.expr, aliases);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            collect_interrupt_aliases_expr(callee, aliases);
            for arg in args {
                collect_interrupt_aliases_expr(&arg.expr, aliases);
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
            collect_interrupt_aliases_expr(cond, aliases);
            collect_interrupt_aliases(then_body, aliases);
            collect_interrupt_aliases_expr(then_value, aliases);
            collect_interrupt_aliases(else_body, aliases);
            collect_interrupt_aliases_expr(else_value, aliases);
        }
        Expr::Paren(inner, _)
        | Expr::Unary(_, inner, _)
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Spread(inner, _)
        | Expr::IncDec { operand: inner, .. } => collect_interrupt_aliases_expr(inner, aliases),
        Expr::Try(inner, _, _, note) => {
            collect_interrupt_aliases_expr(inner, aliases);
            if let Some(note) = note {
                collect_interrupt_aliases_expr(note, aliases);
            }
        }
        _ => {}
    }
}

fn collect_interrupt_aliases(stmts: &[Stmt], aliases: &mut Vec<(String, String)>) {
    let mut work = TirWorklist::new();
    work.push(stmts);
    while let Some(stmts) = work.pop() {
        for stmt in stmts {
            match stmt {
                Stmt::Val(binding) => {
                    if let Some(source) = interrupt_callback_ident(&binding.init) {
                        aliases.push((binding.name.clone(), source.to_string()));
                    }
                    collect_interrupt_aliases_expr(&binding.init, aliases);
                }
                Stmt::CountedLoop {
                    init,
                    step,
                    body,
                    ..
                } => {
                    if let Some(source) = interrupt_callback_ident(&init.init) {
                        aliases.push((init.name.clone(), source.to_string()));
                    }
                    collect_interrupt_aliases_expr(&init.init, aliases);
                    if let Some(step) = step.as_deref() {
                        if let Stmt::Assign { target, value, .. } = step {
                            if let LValue::Local { name, .. } = target {
                                if let Some(source) = interrupt_callback_ident(value) {
                                    aliases.push((name.clone(), source.to_string()));
                                }
                            }
                            collect_interrupt_aliases_expr(value, aliases);
                        }
                    }
                    work.push(body);
                }
                Stmt::While { body, .. }
                | Stmt::For { body, .. }
                | Stmt::Loop { body, .. }
                | Stmt::Unsafe { body, .. }
                | Stmt::Impure { body, .. }
                | Stmt::Reactive { body, .. }
                | Stmt::Shield { body, .. }
                | Stmt::Switched { body, .. }
                | Stmt::Region { body, .. }
                | Stmt::Policy { body, .. }
                | Stmt::TaskGroup { body, .. }
                | Stmt::Layout { body, .. }
                | Stmt::Caps { body, .. }
                | Stmt::Grant { body, .. }
                | Stmt::ContextBlock { body, .. }
                | Stmt::Live { body, .. }
                | Stmt::AssumeDet { body, .. }
                | Stmt::Transact { body, .. }
                | Stmt::ComptimeBlock { body, .. } => work.push(body),
                Stmt::Switch { arms, else_body, .. } => {
                    for arm in arms {
                        work.push(&arm.body);
                    }
                    if let Some(body) = else_body {
                        work.push(body);
                    }
                }
                Stmt::ComptimeIf {
                    then_body,
                    else_body,
                    ..
                } => {
                    work.push(then_body);
                    if let Some(body) = else_body {
                        work.push(body);
                    }
                }
                Stmt::ScopeMember { body, .. } => work.push(body),
                Stmt::ComptimeSwitch {
                    arms, else_body, ..
                } => {
                    for arm in arms {
                        work.push(&arm.body);
                    }
                    if let Some(body) = else_body {
                        work.push(body);
                    }
                }
                Stmt::Expr(expr) => collect_interrupt_aliases_expr(expr, aliases),
                Stmt::Assign { target, value, .. } => {
                    if let LValue::Local { name, .. } = target {
                        if let Some(source) = interrupt_callback_ident(value) {
                            aliases.push((name.clone(), source.to_string()));
                        }
                    }
                    collect_interrupt_aliases_expr(value, aliases);
                }
                Stmt::Return(Some(value), _) | Stmt::Yield(value, _) => {
                    collect_interrupt_aliases_expr(value, aliases)
                }
                _ => {}
            }
        }
    }
}
fn collect_interrupt_lambda_captures_expr(expr: &Expr, captures: &mut Vec<(String, String)>) {
    match expr {
        Expr::Lambda(lambda) => match &lambda.body {
            crate::AST::LambdaBody::Expr(body) => {
                collect_interrupt_lambda_captures_expr(body, captures)
            }
            crate::AST::LambdaBody::Block(body) => {
                collect_interrupt_lambda_captures(body, captures)
            }
        },
        Expr::MethodCall { receiver, args, .. } => {
            collect_interrupt_lambda_captures_expr(receiver, captures);
            for arg in args {
                collect_interrupt_lambda_captures_expr(&arg.expr, captures);
            }
        }
        Expr::Call(call) => {
            for arg in &call.args {
                collect_interrupt_lambda_captures_expr(&arg.expr, captures);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            collect_interrupt_lambda_captures_expr(callee, captures);
            for arg in args {
                collect_interrupt_lambda_captures_expr(&arg.expr, captures);
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
            collect_interrupt_lambda_captures_expr(cond, captures);
            collect_interrupt_lambda_captures(then_body, captures);
            collect_interrupt_lambda_captures_expr(then_value, captures);
            collect_interrupt_lambda_captures(else_body, captures);
            collect_interrupt_lambda_captures_expr(else_value, captures);
        }
        Expr::Paren(inner, _)
        | Expr::Unary(_, inner, _)
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Spread(inner, _)
        | Expr::IncDec { operand: inner, .. } => {
            collect_interrupt_lambda_captures_expr(inner, captures)
        }
        Expr::Try(inner, _, _, note) => {
            collect_interrupt_lambda_captures_expr(inner, captures);
            if let Some(note) = note {
                collect_interrupt_lambda_captures_expr(note, captures);
            }
        }
        _ => {}
    }
}

fn collect_interrupt_lambda_captures(stmts: &[Stmt], captures: &mut Vec<(String, String)>) {
    for stmt in stmts {
        match stmt {
            Stmt::Val(binding) => {
                if let Some(lam) = interrupt_lambda(&binding.init) {
                    for capture in interrupt_lambda_captures(lam) {
                        captures.push((binding.name.clone(), capture));
                    }
                }
                collect_interrupt_lambda_captures_expr(&binding.init, captures);
            }
            Stmt::Expr(expr) => collect_interrupt_lambda_captures_expr(expr, captures),
            Stmt::Assign { target, value, .. } => {
                if let LValue::Local { name, .. } = target {
                    if let Some(lam) = interrupt_lambda(value) {
                        for capture in interrupt_lambda_captures(lam) {
                            captures.push((name.clone(), capture));
                        }
                    }
                }
                collect_interrupt_lambda_captures_expr(value, captures);
            }
            Stmt::Return(Some(value), _) | Stmt::Yield(value, _) => {
                collect_interrupt_lambda_captures_expr(value, captures)
            }
            Stmt::CountedLoop { init, step, body, .. } => {
                if let Some(lam) = interrupt_lambda(&init.init) {
                    for capture in interrupt_lambda_captures(lam) {
                        captures.push((init.name.clone(), capture));
                    }
                }
                collect_interrupt_lambda_captures_expr(&init.init, captures);
                if let Some(step) = step.as_deref() {
                    if let Stmt::Assign { target, value, .. } = step {
                        if let LValue::Local { name, .. } = target {
                            if let Some(lam) = interrupt_lambda(value) {
                                for capture in interrupt_lambda_captures(lam) {
                                    captures.push((name.clone(), capture));
                                }
                            }
                        }
                        collect_interrupt_lambda_captures_expr(value, captures);
                    }
                }
                collect_interrupt_lambda_captures(body, captures);
            }
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. }
            | Stmt::Switched { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::Policy { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::ContextBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::ComptimeBlock { body, .. } => {
                collect_interrupt_lambda_captures(body, captures)
            }
            Stmt::Switch { arms, else_body, .. } => {
                for arm in arms {
                    collect_interrupt_lambda_captures(&arm.body, captures);
                }
                if let Some(body) = else_body {
                    collect_interrupt_lambda_captures(body, captures);
                }
            }
            Stmt::ComptimeIf {
                then_body,
                else_body,
                ..
            } => {
                collect_interrupt_lambda_captures(then_body, captures);
                if let Some(body) = else_body {
                    collect_interrupt_lambda_captures(body, captures);
                }
            }
            Stmt::ScopeMember { body, .. } => collect_interrupt_lambda_captures(body, captures),
            Stmt::ComptimeSwitch { arms, else_body, .. } => {
                for arm in arms {
                    collect_interrupt_lambda_captures(&arm.body, captures);
                }
                if let Some(body) = else_body {
                    collect_interrupt_lambda_captures(body, captures);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn prepare_interrupt_callback_locals(stmts: &[Stmt], cx: &Cx, env: &mut LowerEnv) {
    let mut names = HashSet::new();
    collect_interrupt_callback_names(stmts, cx, &mut names);
    let mut send = names;
    let mut aliases = Vec::new();
    collect_interrupt_aliases(stmts, &mut aliases);
    let mut lambda_captures = Vec::new();
    collect_interrupt_lambda_captures(stmts, &mut lambda_captures);
    loop {
        let before = send.len();
        for (target, source) in &aliases {
            if send.contains(target) {
                send.insert(source.clone());
            }
            if send.contains(source) {
                send.insert(target.clone());
            }
        }
        for (target, source) in &lambda_captures {
            if send.contains(target) {
                send.insert(source.clone());
            }
        }
        if send.len() == before {
            break;
        }
    }
    for name in send {
        env.mark_send_fn(&name);
    }
}

pub(super) fn prepare_interrupt_callback_local_expr(expr: &Expr, cx: &Cx, env: &mut LowerEnv) {
    let mut names = HashSet::new();
    collect_interrupt_callback_names_expr(expr, cx, &mut names);
    let mut aliases = Vec::new();
    collect_interrupt_aliases_expr(expr, &mut aliases);
    let mut lambda_captures = Vec::new();
    collect_interrupt_lambda_captures_expr(expr, &mut lambda_captures);
    loop {
        let before = names.len();
        for (target, source) in &aliases {
            if names.contains(target) {
                names.insert(source.clone());
            }
            if names.contains(source) {
                names.insert(target.clone());
            }
        }
        for (target, source) in &lambda_captures {
            if names.contains(target) {
                names.insert(source.clone());
            }
        }
        if names.len() == before {
            break;
        }
    }
    for name in names {
        env.mark_send_fn(&name);
    }
}

fn force_interrupt_callback_value(mut init: TExpr, cx: &Cx) -> TExpr {
    if matches!(
        &init.kind,
        TExprKind::FnValue {
            kind: TFnValueKind::Interrupt { .. },
        }
    ) {
        return init;
    }
    if let TExprKind::Lambda(lam) = &mut init.kind {
        lam.arc = true;
        lam.rc = false;
    } else if let Some(name) = match &init.kind {
        TExprKind::FnValue {
            kind: TFnValueKind::NamedFn {
                name: Some(name), ..
            },
        } => Some(name.clone()),
        _ => None,
    } {
        let ty = init.ty.clone();
        init.kind = TExprKind::FnValue {
            kind: TFnValueKind::NamedFn {
                wrapper: crate::Codegen::emit_named_fn_value_sync(cx, &name, &ty),
                name: Some(name),
                lambda: None,
            },
        };
    }
    let ty = init.ty.clone();
    TExpr {
        ty,
        kind: TExprKind::FnValue {
            kind: TFnValueKind::Interrupt {
                value: Box::new(init),
            },
        },
    }
}

pub(crate) struct LowerBody<'a> {
    stmts: &'a [Stmt],
    env: LowerEnv,
    markers: bool,
    inherit_env: bool,
    carry_env: bool,
    propagate_env: bool,
    prepare: Option<Box<dyn FnOnce(&Cx, &mut LowerEnv) + 'a>>,
}

impl<'a> LowerBody<'a> {
    pub(crate) fn scoped(stmts: &'a [Stmt], env: LowerEnv) -> Self {
        Self {
            stmts,
            env,
            markers: true,
            inherit_env: false,
            carry_env: false,
            propagate_env: false,
            prepare: None,
        }
    }

    pub(crate) fn direct(stmts: &'a [Stmt], env: LowerEnv) -> Self {
        Self {
            stmts,
            env,
            markers: false,
            inherit_env: false,
            carry_env: false,
            propagate_env: false,
            prepare: None,
        }
    }

    pub(crate) fn inline(stmts: &'a [Stmt], env: LowerEnv) -> Self {
        Self {
            stmts,
            env,
            markers: true,
            inherit_env: false,
            carry_env: false,
            propagate_env: true,
            prepare: None,
        }
    }

    pub(crate) fn inherited(stmts: &'a [Stmt], env: LowerEnv) -> Self {
        Self {
            stmts,
            env,
            markers: true,
            inherit_env: true,
            carry_env: false,
            propagate_env: false,
            prepare: None,
        }
    }

    pub(crate) fn prepare(
        mut self,
        prepare: impl FnOnce(&Cx, &mut LowerEnv) + 'a,
    ) -> Self {
        self.prepare = Some(Box::new(prepare));
        self
    }

    pub(crate) fn carry_env(mut self) -> Self {
        self.carry_env = true;
        self
    }
}

pub(crate) struct LowerStmtPlan<'a> {
    bodies: Vec<LowerBody<'a>>,
    finish: Box<dyn FnOnce(Vec<Vec<TStmt>>) -> TStmt + 'a>,
}

impl<'a> LowerStmtPlan<'a> {
    pub(super) fn ready(stmt: TStmt) -> Self {
        Self {
            bodies: Vec::new(),
            finish: Box::new(move |_| stmt),
        }
    }
}

pub(crate) fn deferred_stmt<'a>(
    mut bodies: Vec<LowerBody<'a>>,
    finish: impl FnOnce(Vec<Vec<TStmt>>) -> TStmt + 'a,
) -> LowerStmtPlan<'a> {
    // Plans are consumed from the end so each deferred body advances in source
    // order without shifting the remaining work on every step.
    bodies.reverse();
    LowerStmtPlan {
        bodies,
        finish: Box::new(finish),
    }
}

struct LowerBlockResult {
    body: Vec<TStmt>,
    env: LowerEnv,
}

enum LowerTask<'a> {
    Block(LowerBlock<'a>),
    Done(LowerBlockResult),
}

struct LowerBlock<'a> {
    stmts: &'a [Stmt],
    cx: &'a Cx,
    env: LowerEnv,
    out: Vec<TStmt>,
    split_views: HashMap<usize, PlannedSplitView>,
    index: usize,
    markers: bool,
    resume: Box<dyn FnOnce(LowerBlockResult) -> LowerTask<'a> + 'a>,
}

impl<'a> LowerBlock<'a> {
    fn new(
        stmts: &'a [Stmt],
        cx: &'a Cx,
        env: LowerEnv,
        markers: bool,
        resume: Box<dyn FnOnce(LowerBlockResult) -> LowerTask<'a> + 'a>,
    ) -> Self {
        let split_views = if markers {
            split_view_plan(stmts, cx, &env)
        } else {
            HashMap::new()
        };
        Self {
            stmts,
            cx,
            out: Vec::with_capacity(stmts.len() * if cx.debug_linemap { 3 } else { 2 }),
            env,
            split_views,
            index: 0,
            markers,
            resume,
        }
    }

    fn step(mut self) -> LowerTask<'a> {
        if self.index == self.stmts.len() {
            return (self.resume)(LowerBlockResult {
                body: self.out,
                env: self.env,
            });
        }

        let index = self.index;
        if let Some(view) = self.split_views.remove(&index) {
            let stmt = &self.stmts[index];
            if self.markers {
                self.out.push(TStmt::SourceSpan(stmt.span()));
                if self.cx.debug_linemap {
                    self.out.push(TStmt::LineMarker(view.candidate.line));
                }
            }
            let mut candidate = view.candidate;
            let elem_ty = match tir_recv_jet_ty(&candidate.owner, &self.env) {
                Some(Type::List(elem) | Type::FixedList { elem, .. }) => Some(*elem),
                _ => None,
            };
            let slot = if candidate.single {
                TLocal::user(&candidate.name).through_ref()
            } else {
                TLocal::user(&candidate.name)
            };
            let slot = match candidate.origin.take() {
                Some(origin) => slot.with_origin(origin),
                None => slot,
            };
            self.env.bind(&candidate.name, slot, candidate.ty.clone());
            // D-TASKBORROW1=A: engines that keep a window record rather than a
            // Rust reference need the window type when this local crosses into
            // a task group child. AOT ignores this fact.
            if let Some(elem) = elem_ty.clone() {
                let handle = if candidate.write {
                    Some(Type::Apply {
                        name: "ViewMut".to_string(),
                        args: vec![elem],
                    })
                } else if !candidate.single {
                    Some(Type::Apply {
                        name: "View".to_string(),
                        args: vec![elem],
                    })
                } else {
                    None
                };
                if let Some(handle) = handle {
                    self.env.mark_split_view(&candidate.name, handle);
                }
            }
            self.out.push(TStmt::SplitViews {
                owner: view
                    .initialize
                    .then(|| lower_expr(&candidate.owner, self.cx, &mut self.env)),
                root: view.root,
                len: view.len,
                source: view.source,
                source_start: view.source_start,
                before: view.before,
                split_tail: view.split_tail,
                segment: view.segment,
                after: view.after,
                name: candidate.name,
                start: candidate.start,
                end: candidate.end,
                single: candidate.single,
                write: candidate.write,
                elem_ty,
                line: candidate.line,
            });
            self.index += 1;
            return LowerTask::Block(self);
        }

        let stmt = &self.stmts[index];
        if self.markers {
            self.out.push(TStmt::SourceSpan(stmt.span()));
            if self.cx.debug_linemap {
                let line = crate::Diagnostics::span_line_col(&self.cx.src, stmt.span().start).0;
                self.out.push(TStmt::LineMarker(line));
            }
        }
        self.index += 1;
        let plan = lower_stmt_plan(stmt, self.cx, &mut self.env);
        if plan.bodies.is_empty() {
            self.out.push((plan.finish)(Vec::new()));
            return LowerTask::Block(self);
        }
        lower_body_chain(self, plan, Vec::new(), None)
    }
}

fn lower_body_chain<'a>(
    parent: LowerBlock<'a>,
    plan: LowerStmtPlan<'a>,
    mut lowered: Vec<Vec<TStmt>>,
    carried_env: Option<LowerEnv>,
) -> LowerTask<'a> {
    let LowerStmtPlan { mut bodies, finish } = plan;
    let mut body = bodies.pop().expect("deferred statement plan has no body");
    // An inherited body starts from its own planned env until a preceding body
    // explicitly carries one (the counted-loop step is the carried case).
    let mut child_env = match (body.inherit_env, carried_env) {
        (true, Some(carried_env)) => carried_env,
        (true, None) | (false, _) => body.env.clone(),
    };
    if let Some(prepare) = body.prepare.take() {
        prepare(parent.cx, &mut child_env);
    }
    let next_carried = body.carry_env;
    let propagate_env = body.propagate_env;
    LowerTask::Block(LowerBlock::new(
        body.stmts,
        parent.cx,
        child_env,
        body.markers,
        Box::new(move |result| {
            let LowerBlockResult { body, env } = result;
            lowered.push(body);
            let carried_env = next_carried.then(|| env.clone());
            let mut parent = parent;
            if propagate_env {
                parent.env = env;
            }
            if bodies.is_empty() {
                let stmt = (finish)(lowered);
                parent.out.push(stmt);
                LowerTask::Block(parent)
            } else {
                let next_plan = LowerStmtPlan { bodies, finish };
                lower_body_chain(parent, next_plan, lowered, carried_env)
            }
        }),
    ))
}

fn lower_stmts_with_markers(
    stmts: &[Stmt],
    cx: &Cx,
    env: &mut LowerEnv,
    markers: bool,
) -> Vec<TStmt> {
    let root_env = clone_env(env);
    let mut task = LowerTask::Block(LowerBlock::new(
        stmts,
        cx,
        root_env,
        markers,
        Box::new(LowerTask::Done),
    ));
    loop {
        task = match task {
            LowerTask::Block(block) => block.step(),
            LowerTask::Done(result) => {
                *env = result.env;
                return result.body;
            }
        };
    }
}

#[inline(never)]
pub(crate) fn lower_stmts(stmts: &[Stmt], cx: &Cx, env: &mut LowerEnv) -> Vec<TStmt> {
    // Child blocks are heap tasks. A nested source block therefore resumes its
    // parent through `LowerBlock::resume` instead of keeping the parent lowering
    // frame on the native stack.
    lower_stmts_with_markers(stmts, cx, env, true)
}

#[derive(Clone)]
struct SplitViewCandidate {
    stmt_index: usize,
    owner: Expr,
    owner_key: String,
    name: String,
    ty: Option<Type>,
    origin: Option<TBindingOrigin>,
    start: i64,
    end: i64,
    single: bool,
    write: bool,
    line: usize,
    last_use: usize,
}

struct PlannedSplitView {
    candidate: SplitViewCandidate,
    initialize: bool,
    root: String,
    len: String,
    source: String,
    source_start: i64,
    before: String,
    split_tail: String,
    segment: String,
    after: String,
}

#[derive(Clone)]
struct SplitRegion {
    name: String,
    start: i64,
    end: Option<i64>,
}

fn const_place_bound(expr: &Expr) -> Option<i64> {
    let mut expr = expr;
    let mut negations = 0usize;
    let mut value = loop {
        match expr {
            Expr::Int(value, ..) => break *value,
            Expr::Unary(UnOp::Neg, inner, _) => {
                negations += 1;
                expr = inner;
            }
            Expr::Paren(inner, _) => expr = inner,
            _ => return None,
        }
    };
    for _ in 0..negations {
        value = value.checked_neg()?;
    }
    Some(value)
}

fn split_owner_key(expr: &Expr) -> Option<String> {
    let mut expr = expr;
    let mut suffixes = Vec::new();
    let root = loop {
        match expr {
            Expr::Ident(name, _) => break Some(format!("name:{name}")),
            Expr::Field(base, field, _) => {
                suffixes.push(format!(".field:{field}"));
                expr = base;
            }
            Expr::Index { base, index, .. } => {
                suffixes.push(format!(".index:{}", const_place_bound(index)?));
                expr = base;
            }
            Expr::Paren(inner, _) | Expr::Place(inner, _, _) => expr = inner,
            _ => return None,
        }
    };
    let mut key = root?;
    for suffix in suffixes.into_iter().rev() {
        key.push_str(&suffix);
    }
    Some(key)
}

fn split_owner_root(expr: &Expr) -> Option<String> {
    let mut expr = expr;
    loop {
        match expr {
            Expr::Ident(name, _) => return Some(name.clone()),
            Expr::Field(base, _, _) | Expr::Index { base, .. } => expr = base,
            Expr::Paren(inner, _) | Expr::Place(inner, _, _) => expr = inner,
            _ => return None,
        }
    }
}

/// D-PIN1=A: `mem.pin(&place)` IS a write window on `place` — the pin adds a
/// sema-side no-move promise, not a second runtime shape. Every tier therefore
/// lowers it through the same path as a written `Expr::Place`, which is what
/// keeps the interpreter and JIT aliasing behaviour identical to AOT (I9).
/// Returns the windowed place and its access when `init` is either spelling.
pub(crate) fn place_window_init<'a>(
    init: &'a Expr,
    cx: &Cx,
) -> Option<(&'a Expr, PlaceAccess)> {
    match init {
        Expr::Place(inner, access, _) => Some((inner.as_ref(), *access)),
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } if method == crate::Syntax::MEM_PIN
            && matches!(receiver.as_ref(), Expr::Ident(alias, _)
                if cx
                    .any_core_import_module(alias)
                    .is_some_and(|m| m == crate::Syntax::CORE_MEM_MODULE)) =>
        {
            let inner = &args.first()?.expr;
            Some((
                match inner {
                    Expr::Place(place, _, _) => place.as_ref(),
                    other => other,
                },
                PlaceAccess::Write,
            ))
        }
        _ => None,
    }
}

fn split_view_candidate(stmt: &Stmt, stmt_index: usize, cx: &Cx) -> Option<SplitViewCandidate> {
    let Stmt::Val(binding) = stmt else {
        return None;
    };
    let (inner, access) = place_window_init(&binding.init, cx)?;
    let (base, start, end, single) = match inner {
        Expr::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            let (start, end) = match range.as_deref() {
                Some(Expr::Range {
                    start,
                    end,
                    exclusive,
                    ..
                }) => {
                    let start = const_place_bound(start)?;
                    let end = const_place_bound(end)?;
                    (start, if *exclusive { end.checked_sub(1)? } else { end })
                }
                Some(_) => return None,
                None => (const_place_bound(start)?, const_place_bound(end)?),
            };
            (base.as_ref(), start, end, false)
        }
        Expr::Index { base, index, .. } => {
            let index = const_place_bound(index)?;
            (base.as_ref(), index, index, true)
        }
        _ => return None,
    };
    let owner_key = split_owner_key(base)?;
    Some(SplitViewCandidate {
        stmt_index,
        owner: base.clone(),
        owner_key,
        name: binding.name.clone(),
        ty: binding.ty.clone(),
        origin: binding
            .ty
            .as_ref()
            .and_then(|ty| crate::Codegen::TIR::lower::tracked_float_origin(binding, ty)),
        start,
        end,
        single,
        write: matches!(access, PlaceAccess::Write),
        line: crate::Diagnostics::span_line_col(&cx.src, binding.name_span.start).0,
        last_use: stmt_index,
    })
}

fn split_view_plan(
    stmts: &[Stmt],
    cx: &Cx,
    env: &LowerEnv,
) -> HashMap<usize, PlannedSplitView> {
    // The planner emits Rust slice operations. Compute windows use
    // checked Prelude handles instead, so resolve block-local owners before
    // planning and leave those bindings on the normal `Expr::Place` path.
    // This temporary environment never affects lexical lowering; it only makes
    // the already-resolved local types visible while selecting candidates. It
    // must advance in source order: prebinding the whole block makes an earlier
    // owner look like a later shadow and can route a compute alias through Rust slices.
    let mut owner_env = env.clone();
    let mut owner_generation: HashMap<String, usize> = HashMap::new();
    let mut candidates = Vec::new();
    for (index, stmt) in stmts.iter().enumerate() {
        if let Some(mut view) = split_view_candidate(stmt, index, cx) {
            let is_tensor = matches!(
                tir_recv_jet_ty(&view.owner, &owner_env),
                Some(ty) if ty.is_compute_tensor_family()
            );
            if !is_tensor && view.start >= 0 && view.end >= view.start {
                let root = split_owner_root(&view.owner).unwrap_or_default();
                let generation = owner_generation.get(&root).copied().unwrap_or_default();
                view.owner_key = format!("{}@{generation}", view.owner_key);
                candidates.push(view);
            }
        }
        if let Stmt::Val(binding) = stmt {
            owner_generation
                .entry(binding.name.clone())
                .and_modify(|generation| *generation += 1)
                .or_insert(1);
            owner_env.bind(
                &binding.name,
                TLocal::user(&binding.name),
                binding.ty.clone(),
            );
        }
    }
    for candidate in &mut candidates {
        candidate.last_use = stmts[candidate.stmt_index + 1..]
            .iter()
            .enumerate()
            .filter(|(_, stmt)| crate::Sema::stmt_references_name_deep(stmt, &candidate.name))
            .map(|(offset, _)| candidate.stmt_index + 1 + offset)
            .next_back()
            .unwrap_or(candidate.stmt_index);
    }

    let mut parent: Vec<usize> = (0..candidates.len()).collect();
    fn find(parent: &mut [usize], mut index: usize) -> usize {
        while parent[index] != index {
            parent[index] = parent[parent[index]];
            index = parent[index];
        }
        index
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let a = find(parent, a);
        let b = find(parent, b);
        if a != b {
            parent[b] = a;
        }
    }
    for a in 0..candidates.len() {
        for b in a + 1..candidates.len() {
            let earlier = &candidates[a];
            let later = &candidates[b];
            if earlier.owner_key == later.owner_key
                && earlier.last_use >= later.stmt_index
                && (earlier.write || later.write)
                && (earlier.end < later.start || later.end < earlier.start)
            {
                union(&mut parent, a, b);
            }
        }
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for index in 0..candidates.len() {
        let root = find(&mut parent, index);
        groups.entry(root).or_default().push(index);
    }
    let mut groups: Vec<_> = groups.into_values().collect();
    groups.sort_by_key(|group| {
        group
            .iter()
            .map(|&index| candidates[index].stmt_index)
            .min()
            .unwrap_or(usize::MAX)
    });
    let mut planned = HashMap::new();
    for (plan_index, mut group) in groups.into_iter().enumerate() {
        // A lone read borrow needs no plan: reading a clone reads the same
        // values. A lone WRITE borrow does need one — without it the emitted
        // Rust borrows a temporary clone and the write is silently dropped,
        // while the JIT applies it. Same source, two answers (I9).
        let lone_write = group.len() == 1 && candidates[group[0]].write;
        if (group.len() < 2 && !lone_write)
            || group.iter().enumerate().any(|(i, &a)| {
                group.iter().skip(i + 1).any(|&b| {
                    let a = &candidates[a];
                    let b = &candidates[b];
                    !(a.end < b.start || b.end < a.start)
                })
            })
        {
            continue;
        }
        group.sort_by_key(|&index| candidates[index].stmt_index);
        let root = jet_format!("{jet_prefix}place_plan_{plan_index}_root");
        let len = jet_format!("{jet_prefix}place_plan_{plan_index}_len");
        let mut regions = vec![SplitRegion {
            name: root.clone(),
            start: 0,
            end: None,
        }];
        for (step, candidate_index) in group.into_iter().enumerate() {
            let candidate = candidates[candidate_index].clone();
            let Some(region_index) = regions.iter().position(|region| {
                candidate.start >= region.start
                    && region.end.is_none_or(|end| candidate.end <= end)
            }) else {
                continue;
            };
            let region = regions.remove(region_index);
            let prefix = jet_format!("{jet_prefix}place_plan_{plan_index}_{step}_before");
            let split_tail = jet_format!("{jet_prefix}place_plan_{plan_index}_{step}_tail");
            let segment = jet_format!("{jet_prefix}place_plan_{plan_index}_{step}_segment");
            let suffix = jet_format!("{jet_prefix}place_plan_{plan_index}_{step}_after");
            if candidate.start > region.start {
                regions.push(SplitRegion {
                    name: prefix.clone(),
                    start: region.start,
                    end: Some(candidate.start - 1),
                });
            }
            if region.end.is_none_or(|end| candidate.end < end) {
                regions.push(SplitRegion {
                    name: suffix.clone(),
                    start: candidate.end + 1,
                    end: region.end,
                });
            }
            planned.insert(
                candidate.stmt_index,
                PlannedSplitView {
                    candidate,
                    initialize: step == 0,
                    root: root.clone(),
                    len: len.clone(),
                    source: region.name,
                    source_start: region.start,
                    before: prefix,
                    split_tail,
                    segment,
                    after: suffix,
                },
            );
        }
    }
    planned
}

/// A typed list head can reach lowering as a one-element `ListLit` after sema
/// elaborates the head away. Preserve the asserted list shape:
/// the value already has the exact expected list type, so the wrapper is not
/// another list dimension.
pub(crate) fn preserve_typed_list_shape(expr: TExpr, expected: &Type, cx: &Cx) -> TExpr {
    if !matches!(expected, Type::List(_) | Type::FixedList { .. }) {
        return expr;
    }
    let mut expr = match expr {
        TExpr {
            ty,
            kind: TExprKind::ListLit(mut elems),
        } => {
            if elems.len() == 1 && elems[0].ty == *expected {
                elems.pop().unwrap()
            } else {
                TExpr {
                    ty,
                    kind: TExprKind::ListLit(elems),
                }
            }
        }
        other => other,
    };
    let expected_elem = match expected {
        Type::List(elem) | Type::FixedList { elem, .. } => elem.as_ref(),
        _ => return expr,
    };
    // D-SG9: typed `[U8].{…}` / `[I32].{…}` heads must drive each element's
    // Rust integer suffix. Sema may leave bare `IntLit(_, None)` when the list
    // type comes from the head alone; retag from the expected element type so
    // emit produces `104u8` rather than `104i64` (I2).
    if matches!(expected_elem, Type::IntN { .. } | Type::List(_) | Type::FixedList { .. }) {
        if let TExprKind::ListLit(elems) = &mut expr.kind {
            for elem in elems.iter_mut() {
                match (&mut elem.kind, expected_elem) {
                    (TExprKind::IntLit(_, width), Type::IntN { signed, bits }) => {
                        *width = Some((*signed, *bits));
                        elem.ty = expected_elem.clone();
                    }
                    // A nested list literal (`[[U8]]`) drives suffixes one
                    // dimension down with the same rule.
                    (TExprKind::ListLit(_), Type::List(_) | Type::FixedList { .. }) => {
                        let inner = std::mem::replace(elem, TExpr {
                            ty: Type::Int,
                            kind: TExprKind::ListLit(Vec::new()),
                        });
                        *elem = preserve_typed_list_shape(inner, expected_elem, cx);
                    }
                    _ => {}
                }
            }
            expr.ty = expected.clone();
            return expr;
        }
    }
    let trait_name = match expected_elem {
        Type::TraitObject(names) if names.len() == 1 => names.first(),
        Type::Named(name) if cx.trait_names.contains(name) => Some(name),
        _ => None,
    };
    let Some(trait_name) = trait_name else {
        expr.ty = expected.clone();
        return expr;
    };
    let TExprKind::ListLit(elems) = &mut expr.kind else {
        expr.ty = expected.clone();
        return expr;
    };
    for elem in elems {
        let concrete = match &elem.ty {
            Type::Named(name) => Some(name.clone()),
            Type::Apply { name, .. } => Some(name.clone()),
            _ => None,
        };
        let (Some(concrete), TExprKind::StructLit { as_trait, .. }) =
            (concrete, &mut elem.kind)
        else {
            continue;
        };
        if as_trait.is_none() {
            *as_trait = Some((trait_name.clone(), concrete));
            elem.ty = Type::TraitObject(vec![trait_name.clone()]);
        }
    }
    expr.ty = expected.clone();
    expr
}

#[inline(never)]
fn lower_stmt_plan<'a>(s: &'a Stmt, cx: &'a Cx, env: &mut LowerEnv) -> LowerStmtPlan<'a> {
    macro_rules! ready_return {
        ($stmt:expr) => {
            return LowerStmtPlan::ready($stmt);
        };
    }
    if let Stmt::Assign { target, value, .. } = s {
        let root_name = match target {
            LValue::Local { name, .. } => Some(name.as_str()),
            LValue::Index { base, .. } | LValue::Field { base, .. } => {
                match base.as_ref() {
                    Expr::Ident(name, _) => Some(name.as_str()),
                    _ => None,
                }
            }
        };
        if let Some(name) = root_name.filter(|name| env.is_gc(name)) {
            let root = env.place_of(name);
            let edges = env.gc_edges_for_expr(value, Some(name));
            let slot = match target {
                LValue::Local { name, .. } => format!("local:{name}"),
                LValue::Field { field, .. } => format!("field:{field}"),
                LValue::Index { span, .. } => format!("index:{}", span.start),
            };
            let mut lowered_source = s.clone();
            let index_temp = if let (
                LValue::Index { index, span, .. },
                Stmt::Assign { target, .. },
            ) = (target, &mut lowered_source)
            {
                let lowered = lower_expr(index, cx, env);
                let source_name = jet_format!("{jet_prefix}gc_index_{}", span.start);
                let rust_name = source_name.clone();
                let LValue::Index {
                    index: lowered_index,
                    ..
                } = target
                else {
                    unreachable!("matched index assignment")
                };
                *lowered_index = Box::new(Expr::Ident(source_name.clone(), *span));
                env.bind(
                    &source_name,
                    TLocal::generated(&rust_name),
                    Some(lowered.ty.clone()),
                );
                Some((rust_name, lowered))
            } else {
                None
            };
            let saved = env.locals.get(name).cloned();
            env.gc_locals.remove(name);
            env.bind(
                name,
                TLocal::generated("value").through_ref(),
                saved.as_ref().and_then(|(_, ty)| ty.clone()),
            );
            let plan = lower_stmt_plan(&lowered_source, cx, env);
            let LowerStmtPlan { bodies, finish } = plan;
            debug_assert!(bodies.is_empty(), "GC edit lowering has no nested body");
            let stmt = finish(Vec::new());
            if let Some((slot, ty)) = saved {
                env.bind(name, slot, ty);
            }
            env.mark_gc(name);
            if let Some((temp, _)) = &index_temp {
                env.locals.remove(temp);
            }
            ready_return!(TStmt::GcEdit {
                root,
                slot,
                edges,
                replace_all: matches!(target, LValue::Local { .. }),
                index_temp,
                stmt: Box::new(stmt),
            });
        }
    }
    LowerStmtPlan::ready(match s {
        Stmt::Val(b) if matches!(&b.pattern, Some(BindPattern::Struct { .. })) => {
            // c109: a struct-destructuring binding `Type { x, y } :: <init>`. Lower the
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
            let move_fields = fields.iter().any(|field| {
                field_tys
                    .get(&field.name)
                    .is_some_and(|ty| cx.type_contains_shared_guard(ty))
            });
            let tmp = jet_format!("{jet_prefix}d{}", span.start);
            let kw = if b.mutable { "let mut" } else { "let" };
            let mut binds = Vec::new();
            for f in fields {
                let field_rust = mangle(&f.name).to_string();
                let local_rust = mangle(f.local_name()).to_string();
                binds.push((local_rust, field_rust));
                env.bind(
                    f.local_name(),
                    TLocal::user(f.local_name()),
                    field_tys.get(&f.name).cloned(),
                );
            }
            ready_return!(TStmt::StructDestructure {
                tmp,
                init,
                kw,
                move_fields,
                binds,
            });
        }
        Stmt::Val(b) if matches!(&b.pattern, Some(BindPattern::Tuple { .. })) => {
            // c109 Phase 23: a tuple-destructuring binding `(a, b) :: <init>`. Lower the
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
                Type::Apply { name, args } if name == "VjpRun" && args.len() == 1 => [
                    ("value".to_string(), Type::Named("Tensor".to_string())),
                    (
                        "pull".to_string(),
                        struct_field_type(cx, &init.ty, "pull")
                            .unwrap_or_else(|| Type::Fn {
                                params: vec![Type::Named("Tensor".to_string())],
                                ret: Some(Box::new(args[0].clone())),
                                effect_bound: None,
                                param_contract: None,
                call_metadata: None,
                                return_view_provenance: None,
                            }),
                    ),
                ]
                .into_iter()
                .collect(),
                _ => Vec::new(),
            };
            let move_fields = canonical.iter().any(|(_, ty)| {
                cx.type_contains_shared_guard(ty)
                    || matches!(
                        ty,
                        Type::Apply { name, .. }
                            if matches!(name.as_str(), "CellReadGuard" | "CellEditGuard")
                    )
            });
            let tmp = jet_format!("{jet_prefix}d{}", span.start);
            let kw = if b.mutable { "let mut" } else { "let" };
            let mut binds = Vec::new();
            for (e, (fname, fty)) in elems.iter().zip(canonical.iter()) {
                let elem_rust = mangle(&e.name).to_string();
                let field_rust = crate::Codegen::TIR::core_struct_field_rust_name(cx, &init.ty, fname)
                    .unwrap_or_else(|| mangle(fname).to_string());
                binds.push((elem_rust, field_rust));
                env.bind(&e.name, TLocal::user(&e.name), Some(fty.clone()));
            }
            ready_return!(TStmt::TupleDestructure {
                tmp,
                init,
                kw,
                move_fields,
                binds,
            });
        }
        Stmt::Val(b) if matches!(&b.pattern, Some(BindPattern::List { .. })) => {
            // c109 Phase 26: a list-destructuring binding `[a, b, c] :: <init>`. Lower
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
            let tmp = jet_format!("{jet_prefix}d{}", span.start);
            let kw = if b.mutable { "let mut" } else { "let" };
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            let mut elem_names = Vec::new();
            for e in elems {
                elem_names.push(mangle(&e.name));
                env.bind(&e.name, TLocal::user(&e.name), elem_ty.clone());
            }
            ready_return!(TStmt::ListDestructure {
                tmp,
                init,
                kw,
                want: elems.len(),
                file: cx.file.clone(),
                line,
                elems: elem_names,
            });
        }
        Stmt::Val(b) => {
            // D-UNINIT1 engine, reused unchanged by D-UNINIT-SENTINEL2: lower
            // `name := T.{ uninit }` to
            //   `let mut name: T = unsafe { std::mem::MaybeUninit::<T>::uninit().assume_init() };`
            // The source's `use core.mem` + `Type.{ uninit }` is the expert-tier opt-in (I1: no
            // `unsafe` in generated code without a source-level gate). Sema proved
            // write-before-read (E0420), so every subsequent read is post-write — the
            // `assume_init()` at declaration yields garbage bytes that are always
            // overwritten before any read. The `is_pod_uninit_type` guard in sema
            // (E0423) ensures T has no Drop glue, so no destructor ever reads the garbage.
            if b.uninit {
                let ty =
                    b.ty.as_ref()
                        .expect("E0421 ensures a `Type.{ uninit }` binding has a type");
                let ty = ty.without_user_tags();
                let slot = if matches!(ty, Type::FixedList { .. }) {
                    env.mark_uninit_fixed(&b.name);
                    TLocal::user(&b.name).as_uninit_fixed()
                } else {
                    TLocal::user(&b.name).as_uninit_scalar()
                };
                let slot = tracked_float_slot(b, ty, slot);
                env.bind(&b.name, slot, b.ty.clone());
                ready_return!(TStmt::Let {
                    name: b.name.clone(),
                    kw: "let mut",
                    let_ty: crate::Codegen::TIR::let_ty_for_opt(Some(ty), cx, false, false, false),
                    init: TExpr {
                        ty: ty.clone(),
                        kind: TExprKind::Uninit,
                    },
                gc_promotion: None,
                gc_transferred: false,
                });
            }
            // c109 Phase 19: an arena `view` binding (`x :: arena.alloc(v)`). The AST
            // `emit_let`'s `arena_view` branch emits `let <x> = <init>;` (NO type clause,
            // NEVER `let mut` — a view is a non-reassignable `&mut T`) and binds a DEREF'd
            // slot (reads go through `(*x)`). Reproduce it exactly: a `Let` with `kw: "let"`,
            // empty `ty_clause`, and a deref'd slot place `(*<x>)`.
            if b.arena_view {
                let init = lower_expr(&b.init, cx, env);
                let slot = tracked_float_slot(
                    b,
                    b.ty.as_ref().unwrap_or(&init.ty),
                    TLocal::user(&b.name).through_ref(),
                );
                env.bind(&b.name, slot, b.ty.clone());
                ready_return!(TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    let_ty: TLetTy::Inferred,
                    init,
                gc_promotion: None,
                gc_transferred: false,
                });
            }
            // D-SHAPE-PLACE1=A: local place windows are references with no
            // written Rust type clause. Range windows already behave as slices;
            // whole/field/index windows bind a dereferenced transparent slot.
            let moved_view = if let Some((inner, _)) = place_window_init(&b.init, cx) {
                let moved = lower_owned_expr(inner, cx, env);
                if matches!(
                    &moved.ty,
                    Type::Apply { name, .. }
                        if matches!(name.as_str(), "ViewMut" | "ComputeViewMut")
                ) {
                    Some(moved)
                } else {
                    let init = lower_expr(&b.init, cx, env);
                    let range = matches!(inner, Expr::Slice { .. });
                    let slot = if range {
                        TLocal::user(&b.name)
                    } else {
                        TLocal::user(&b.name).through_ref()
                    };
                    let slot = tracked_float_slot(
                        b,
                        b.ty.as_ref().unwrap_or(&init.ty),
                        slot,
                    );
                    let binding_ty = if init.ty.is_compute_view_mut() {
                        Some(init.ty.clone())
                    } else {
                        b.ty.clone()
                    };
                    env.bind(&b.name, slot, binding_ty);
                    ready_return!(TStmt::Let {
                        name: b.name.clone(),
                        kw: if init.ty.is_compute_view_mut() {
                            "let mut"
                        } else {
                            "let"
                        },
                        let_ty: TLetTy::Inferred,
                        init,
                        gc_promotion: None,
                        gc_transferred: false,
                    });
                }
            } else {
                None
            };
            // D-MEM1 stage S5 (2026-07-04): a string-view binding (`x :: s.trim()` /
            // `x :: s.after(sep)` / `x :: s.before(sep)`; sema set `string_view`
            // after proving E2307-safety — see `CheckerCore.rs`'s binding check).
            // Unlike `arena_view` this binds a plain `&str` (no deref needed to
            // read it): `ty_clause: ": &str"`, `kw: "let"` (non-reassignable,
            // non-escaping local, I8, same as arena/list views), and the init
            // goes through the borrowed `_view` builtin op instead of
            // `resolve_builtin_op`'s owned default.
            if b.string_view {
                let init = lower_string_view_init(&b.init, cx, env);
                env.bind(&b.name, TLocal::user(&b.name), Some(Type::String));
                env.mark_string_view(&b.name);
                ready_return!(TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    let_ty: TLetTy::StrView,
                    init,
                gc_promotion: None,
                gc_transferred: false,
                });
            }
            // c109 (S57/M9.5): a comptime LOCAL `$name :: expr`. The AST `emit_let`
            // builds `init` from `b.ct.serialize()` (the sema-evaluated value rendered to a
            // Rust literal) — the runtime `init` expr is never emitted. Reproduce it: a
            // verbatim `ConstInline` of the same serialized string, with `kw: "let"` (the
            // `(b.mutable && !b.is_comptime)` guard makes it `let`, never `let mut`) and the
            // type clause from `b.ty` (rendered exactly as the non-comptime path below). All
            // facts are pre-resolved (I3): no inference here.
            // A comptime local inside a `$ { … }` block is evaluated by
            // the interpreter itself, so sema never pre-resolves `b.ct`. There
            // the binding is an ordinary one whose init runs now; only a
            // pre-resolved value becomes literal data.
            // D-FIXARR1: `[T#N]` must emit a Rust array. `CtValue::serialize` always
            // prints lists as `vec![…]`, so skip the comptime shortcut and lower the
            // source literal (retag below) instead of baking a CtLit.
            // D-SG9: same for `[U8]`/`[I32]`/… — serialize always suffixes `i64`, which
            // rustc rejects against `Vec<u8>` (I2). Lower + preserve_typed_list_shape.
            // Trait-object elements (`[Shape].{ Circle.{…}, Square.{…} }`) need the
            // same skip: `CtValue::serialize`/`bake_comptime_value_with_type` have no
            // concept of `Box<dyn Trait>` boxing, so a comptime-baked `CtLit` emits
            // bare struct literals where rustc expects `Box::new(...)` (I2). Lower +
            // preserve_typed_list_shape instead, which already boxes trait elements.
            let is_trait_elem = |elem: &Type| {
                matches!(elem, Type::TraitObject(_))
                    || matches!(elem, Type::Named(name) if cx.trait_names.contains(name))
            };
            let binding_ty = b.ty.as_ref().map(Type::without_user_tags);
            // Recursive: `[[U8]]` and deeper nestings need the same skip as
            // `[U8]` — serialize suffixes every level `i64`.
            fn list_needs_lowering(ty: &Type, is_trait_elem: &impl Fn(&Type) -> bool) -> bool {
                match ty {
                    Type::FixedList { .. } => true,
                    Type::List(elem) => {
                        matches!(elem.as_ref(), Type::IntN { .. })
                            || is_trait_elem(elem)
                            || list_needs_lowering(elem, is_trait_elem)
                    }
                    _ => false,
                }
            }
            let skip_ct_list_bake =
                binding_ty.is_some_and(|ty| list_needs_lowering(ty, &is_trait_elem));
            let skip_ct_view_bake = binding_ty.is_some_and(|ty| cx.type_contains_view(ty));
            let skip_ct_boxed_bake =
                binding_ty.is_some_and(|ty| cx.type_contains_boxed_edge(ty));
            let skip_ct_typed_literal_bake = binding_ty
                .is_some_and(|ty| cx.type_contains_typed_literal_edge(ty));
            // Enum literals need the TIR enum-prefix resolver: a comptime enum
            // serialization preserves the Jet dotted variant (`Fire.Burn`) but
            // does not know the flat Rust variant spelling.
            let skip_ct_enum_bake =
                matches!(b.ct, Some(crate::AST::CtValue::Enum { .. }));
            if b.ct.is_some()
                && !skip_ct_list_bake
                && !skip_ct_view_bake
                && !skip_ct_boxed_bake
                && !skip_ct_typed_literal_bake
                && !skip_ct_enum_bake
            {
                let let_ty = crate::Codegen::TIR::let_ty_for_opt(b.ty.as_ref(), cx, false, false, false);
                let init_ty = b
                    .ty
                    .as_ref()
                    .map(|ty| ty.without_user_tags().clone())
                    .unwrap_or(Type::Int);
                let init = TExpr {
                    ty: init_ty,
                    kind: lower_comptime_scalar(b.ct.as_ref(), b.ty.as_ref()).unwrap_or_else(|| {
                        b.ct
                            .as_ref()
                            .map(|v| {
                                let value = b.ty.as_ref().map_or_else(
                                    || v.clone(),
                                    |ty| bake_comptime_value_with_type(v, ty.without_user_tags(), cx),
                                );
                                TExprKind::CtLit(value)
                            })
                            .unwrap_or(TExprKind::DefaultLit)
                    }),
                };
                let slot = tracked_float_slot(
                    b,
                    b.ty.as_ref().unwrap_or(&init.ty),
                    TLocal::user(&b.name),
                );
                env.bind(&b.name, slot, b.ty.clone());
                ready_return!(TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    let_ty,
                    init,
                gc_promotion: None,
                gc_transferred: false,
                });
            }
            let mut init =
                moved_view.unwrap_or_else(|| lower_owned_expr(&b.init, cx, env));
            // No declared `b.ty`? A typed list head (`[Shape].{ Circle.{…}, … }`)
            // still carries its own resolved element type on `init.ty` — reuse it
            // self-referentially so trait-object elements still get boxed
            // (`Box::new(...)`) below. Without this, an inferred `shapes :: [Shape].{…}`
            // binding skipped the same coercion an explicit `shapes: [Shape] :: …`
            // binding got, and rustc rejected the un-boxed struct literals (I2).
            let want = b
                .ty
                .as_ref()
                .map(|ty| ty.without_user_tags().clone())
                .unwrap_or_else(|| init.ty.clone());
            init = preserve_typed_list_shape(init, &want, cx);
            // D-FIXARR1: if the binding type is `[T#N]` and the init lowered as a
            // growable list (e.g. a typed-head literal elaborated to ListLit), re-tag
            // so emit produces a Rust array `[e1, …]` instead of `vec![…]`.
            if let Some(fl @ Type::FixedList { .. }) = b.ty.as_ref().map(Type::without_user_tags) {
                init.ty = fl.clone();
            }
            // D-UNIONTYPE1=A: member → union inject at the binding boundary.
            if let Some(want) = b.ty.as_ref().map(Type::without_user_tags) {
                init = crate::Codegen::TIR::maybe_widen_expr_to_union(init, want);
            }
            // D-SOA1: an EMPTY list literal `[]` for a declared columnar `[S]` lowers
            // with an Int placeholder element type (no element to infer from), so it
            // came through as a plain `ListLit([])`/`vec![]`. Rewrite it to the
            // columnar empty constructor `__jet_<S>_columns::from_aos(vec![])` using
            // the binding's declared type.
            if let Some(decl @ Type::List(inner)) = b.ty.as_ref().map(Type::without_user_tags) {
                if let Some(columns_ty) = cx.columnar_list_type(inner) {
                    if matches!(&init.kind, TExprKind::ListLit(es) if es.is_empty()) {
                        init = TExpr {
                            ty: decl.clone(),
                            kind: TExprKind::ColumnarListLit {
                                columns_ty,
                                elems: Vec::new(),
                            },
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
                if let Some(Type::Fn {
                    params,
                    ret,
                    return_view_provenance,
                    ..
                }) = &b.ty {
                    let coerced = format!(
                        "{} as {}",
                        emit_tir_expr(&init, cx),
                        cx.rust_fn_trait(
                            params,
                            ret.as_deref(),
                            return_view_provenance.as_ref(),
                            true,
                        )
                    );
                    let init_ty = init.ty.clone();
                    let lambda = match std::mem::replace(&mut init.kind, TExprKind::Unit) {
                        TExprKind::Lambda(lambda) => Some(lambda),
                        other => {
                            init.kind = other;
                            None
                        }
                    };
                    init = TExpr {
                        ty: init_ty,
                        kind: TExprKind::FnValue {
                            kind: TFnValueKind::NamedFn {
                                wrapper: coerced,
                                name: None,
                                lambda,
                            },
                        },
                    };
                }
            }
            // Totality: if the source omitted the type, infer it ONCE here from
            // the init's already-resolved type. Codegen never infers.
            // A written Tensor place has an internal carrier that keeps the
            // owner/range for the shared Prelude window setter. It must stay
            // inferred; the source-facing ViewMut spelling is a sema type,
            // not the generated Rust carrier.
            let ty = if init.ty.is_compute_view_mut() {
                init.ty.clone()
            } else {
                b.ty
                    .as_ref()
                    .map(|ty| ty.without_user_tags().clone())
                    .unwrap_or_else(|| init.ty.clone())
            };
            let send_fn = env.is_send_fn(&b.name)
                && matches!(&ty, Type::Fn { .. })
                && !mut_fn;
            if send_fn {
                init = force_interrupt_callback_value(init, cx);
            }
            let is_resource = match &ty {
                Type::Named(name) | Type::Apply { name, .. } => cx.close_types.contains(name),
                _ => false,
            };
            if is_resource {
                init = TExpr {
                    ty: ty.clone(),
                    kind: TExprKind::ResourceNew(Box::new(init)),
                };
            }
            // E2-M7/E2-M10/D-ALLOC1/D-ROUTE1: a handle binding forces `let mut` even
            // when bound immutably (its methods take `&mut self`). Mirror
            // `emit_let`'s `is_file_handle` set exactly.
            // card #1859: `Mailer` (`jet_email::Mailer::send`, Prelude/CoreLib/Email.rs)
            // takes `&mut self` too and was missing from this set, so a `mailer ::
            // email.smtp(…)` local kept `let` and rustc rejected `.send()` (E0596, I2).
            let is_file_handle = matches!(
                &ty,
                Type::Named(n) if n == "FileReader" || n == "FileWriter"
                    || n == "JSONReader" || n == "JSONWriter"
                    || n == "JSONLReader" || n == "JSONLWriter"
                    || n == "CSVReader" || n == "CSVWriter"
                    || n == "XMLReader" || n == "XMLWriter"
                    || n == "CBORReader" || n == "CBORWriter"
                    || n == "Stdout" || n == "Stderr"
                    || n == "TcpStream" || n == "UnixStream" || n == "HTTPRouter"
                    || n == "Arena" || n == "Bump" || n == "Pool" || n == "Fixed"
                    || n == "Mailer"
            )
            // D-DATAFLOW1=A: DataStream.next / stream reducers take &mut.
            || matches!(
                &ty,
                Type::Apply { name, .. }
                    if name == "DataStream" && !cx.type_names.contains(name.as_str())
            )
            // D-SHIFT1 (c7shift): `Reader`/`Cursor` bindings are usually
            // written without an annotation (`r :: Reader.over(bytes)`), so
            // test the resolved type; every read advances `pos` (`&mut self`).
            // User-type-wins guard as everywhere else for these two names.
            || matches!(
                &ty,
                Type::Named(n) if (n == "Reader" || n == "Cursor")
                    && !cx.type_names.contains(n.as_str())
            )
            || matches!(
                &ty,
                Type::Tagged { marker, inner }
                    if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::SharedGuardEdit))
                        && matches!(
                            inner.as_ref(),
                            Type::Apply { name, .. }
                                if name == Syntax::TYPE_SHARED_GUARD
                        )
            );
            let kw = if (b.mutable && !b.is_comptime)
                || mut_fn
                || is_file_handle
                || cx.type_contains_mutable_view(&ty)
            {
                "let mut"
            } else {
                "let"
            };
            // The type annotation clause, rendered exactly as `emit_let`: a Fn type via
            // `rust_fn_trait(params, ret, mut_fn)`, others via `rust_type`. Empty for an
            // inferred binding.
            let let_ty = if ty.is_compute_view_mut() {
                TLetTy::Inferred
            } else if send_fn {
                TLetTy::SendFn(ty.clone())
            } else {
                crate::Codegen::TIR::let_ty_for_opt(
                    b.ty.as_ref(),
                    cx,
                    mut_fn,
                    is_resource,
                    b.gc_promotion.is_some() || b.gc_transferred,
                )
            };
            let binding_name = if is_resource {
                mangle_generated(&format!("resource_{}_{}", b.name, b.name_span.start))
            } else {
                b.name.clone()
            };
            let slot = if is_resource {
                TLocal::user(&binding_name).through_ref()
            } else if kw == "let mut" {
                TLocal::user(&binding_name).as_mutable()
            } else {
                TLocal::user(&binding_name)
            };
            let slot = tracked_float_slot(b, &ty, slot);
            env.bind(&b.name, slot, Some(ty));
            if b.gc_promotion.is_some() || b.gc_transferred {
                env.mark_gc(&b.name);
            }
            if is_resource {
                env.mark_resource(&b.name);
            }
            TStmt::Let {
                name: binding_name,
                kw,
                let_ty,
                init,
                gc_promotion: b.gc_promotion.clone(),
                gc_transferred: b.gc_transferred,
            }
        }
        Stmt::Assign {
            target,
            op,
            op_span,
            value,
        } => match target {
            LValue::Local { name, .. } => {
                // c150: mirror the lower_enum_arg clone predicate — a borrowed non-scalar
                // ident on the RHS would move out of a shared reference (E0507, I2).
                let clone_value = if let Expr::Ident(vname, _) = value {
                    env.is_borrowed(vname) && env.ty_of(vname).is_some_and(|t| !t.is_scalar())
                } else {
                    false
                };
                let mut value_t = lower_expr(value, cx, env);
                if env.is_send_fn(name) && matches!(&value_t.ty, Type::Fn { .. }) {
                    // Keep every write to an indirect callback in the same
                    // canonical representation as its declaration. Otherwise
                    // a later hot-swapped registration could load an ordinary
                    // Rc/raw function value into the Send crossing.
                    value_t = force_interrupt_callback_value(value_t, cx);
                }
                TStmt::Assign {
                    place: TPlace::Local(env.local_of(name)),
                    op: *op,
                    value: value_t,
                    clone_value,
                    line: crate::Diagnostics::span_line_col(&cx.src, op_span.start).0 as u32,
                }
            }
            // c109 Phase 5: `coll[i] = v`. The `IndexKind` is resolved by sema; carry
            // it as the total `is_map` fact (the gate excluded `Unknown`). No compound
            // op on an index lvalue (parser admits only `=`).
            LValue::Index {
                base,
                index,
                kind,
                span,
            } => {
                debug_assert!(
                    super::is_eval_fragment() || !matches!(kind, IndexKind::Unknown),
                    "sema-to-TIR handoff violated"
                );
                // Sema-to-TIR handoff assert (ice_regressions b5 bug class): the
                // subset gate must have already excluded `IndexKind::Unknown` before
                // routing here — an `Unknown` default reaching lowering means sema
                // left an index kind unresolved and the gate missed it.
                debug_assert!(
                    super::is_eval_fragment() || !matches!(kind, IndexKind::Unknown),
                    "sema-to-TIR handoff violated: unresolved index kind"
                );
                let kind = if matches!(kind, IndexKind::Unknown) {
                    &IndexKind::List
                } else {
                    kind
                };
                let base_t = lower_expr(base, cx, env);
                let index_t = lower_expr(index, cx, env);
                let value_t = lower_expr(value, cx, env);
                if let IndexKind::User(type_name) = kind {
                    ready_return!(TStmt::IndexHookAssign {
                        type_name: type_name.clone(),
                        base: base_t,
                        index: index_t,
                        value: value_t,
                    });
                }
                // D-MEM1 S6: `pool[id] = v` — a genuine mutable place through
                // `jet_pool_get_mut` (generation-checked, panics on a stale `id`),
                // not a value round-trip. Reuses the plain `TStmt::Assign` (a raw
                // Rust place string) rather than `IndexAssign`'s bool-keyed
                // List/Map dispatch, since Pool needs its own helper + panic text.
                if matches!(kind, IndexKind::Pool) {
                    let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
                    let elem_ty = value_t.ty.clone();
                    ready_return!(TStmt::Assign {
                        place: TPlace::Expr(Box::new(TExpr {
                            ty: elem_ty,
                            kind: TExprKind::PoolSlot {
                                pool: Box::new(base_t),
                                id: Box::new(index_t),
                                mutable: true,
                                field: None,
                                line,
                            },
                        })),
                        op: *op,
                        value: value_t,
                        clone_value: false,
                        line: crate::Diagnostics::span_line_col(&cx.src, op_span.start).0 as u32,
                    });
                }
                TStmt::IndexAssign {
                    uninit: matches!(base.as_ref(), Expr::Ident(name, _) if env.is_uninit_fixed(name)),
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
                if let Expr::Index {
                    base: collection,
                    index,
                    kind,
                    span: index_span,
                } = base.as_ref()
                {
                    let is_map = matches!(kind, IndexKind::Map);
                    let index_proven = matches!(kind, IndexKind::FixedListProof);
                    if is_map
                        || index_proven
                        || matches!(kind, IndexKind::List)
                    {
                        let collection_t = lower_expr(collection, cx, env);
                        let elem_ty = match &collection_t.ty {
                            Type::List(elem) | Type::FixedList { elem, .. } => {
                                Some((**elem).clone())
                            }
                            Type::Map { value, .. } => Some((**value).clone()),
                            // D-MEM-VIEWRET1 / #1163: `ViewMut<T>[i].field`
                            // must write through the slice element, not clone
                            // via `jet_index_vec` and mutate a temporary.
                            Type::Apply { name, args }
                                if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut")
                                    && args.len() == 1 =>
                            {
                                Some(args[0].clone())
                            }
                            _ => None,
                        };
                        if let Some(elem_ty) = elem_ty {
                            let field_ty =
                                struct_field_type(cx, &elem_ty, field).unwrap_or(Type::Int);
                            let clone_value = if let Expr::Ident(vname, _) = value {
                                env.is_borrowed(vname)
                                    && env.ty_of(vname).is_some_and(|t| !t.is_scalar())
                            } else {
                                false
                            };
                            let line = crate::Diagnostics::span_line_col(
                                &cx.src,
                                index_span.start,
                            )
                            .0;
                            ready_return!(TStmt::IndexFieldAssign(Box::new(TIndexFieldAssign {
                                base: collection_t,
                                index: lower_expr(index, cx, env),
                                is_map,
                                index_proven,
                                field: field.to_string(),
                                field_ty,
                                op: *op,
                                value: lower_expr(value, cx, env),
                                clone_value,
                                line,
                            })));
                        }
                    }
                }
                let base_t = lower_expr(base, cx, env);
                let swizzle_write = match &base_t.ty {
                    Type::Named(type_name)
                        if crate::Sema::is_swizzleable_math_type(type_name)
                            && !cx.struct_fields.contains_key(type_name) =>
                    {
                        match crate::Sema::parse_swizzle_member(field, type_name) {
                            crate::Sema::SwizzleParse::Ok(lanes) => {
                                let lanes_u8: Vec<u8> = lanes.iter().map(|&i| i as u8).collect();
                                Some((type_name.clone(), lanes_u8))
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some((type_name, lanes_u8)) = swizzle_write {
                    let clone_value = if let Expr::Ident(vname, _) = value {
                        env.is_borrowed(vname) && env.ty_of(vname).is_some_and(|t| !t.is_scalar())
                    } else {
                        false
                    };
                    ready_return!(TStmt::MathSwizzleAssign {
                        base: base_t,
                        type_name,
                        lanes: lanes_u8,
                        value: lower_expr(value, cx, env),
                        clone_value,
                    });
                }
                // D-MEM1 S6: `pool[id].field = v` — the general fallback below
                // resolves `place` by re-emitting the FIELD-READ expression (fine
                // for an owning local/`self`, but a `Pool` index-read is a value
                // clone via `jet_pool_get` — writing `.field` on that would edit a
                // throwaway copy and silently drop the change). Build a genuine
                // mutable place through `jet_pool_get_mut` instead.
                if let Expr::Index {
                    base: pool_expr,
                    index: id_expr,
                    kind: IndexKind::Pool,
                    span: idx_span,
                } = base.as_ref()
                {
                    let line = crate::Diagnostics::span_line_col(&cx.src, idx_span.start).0;
                    let pool_t = lower_expr(pool_expr, cx, env);
                    let id_t = lower_expr(id_expr, cx, env);
                    let elem_ty = match &pool_t.ty {
                        Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
                        _ => Type::Int,
                    };
                    let field_ty = struct_field_type(cx, &elem_ty, field).unwrap_or(Type::Int);
                    let place = TPlace::Expr(Box::new(TExpr {
                        ty: field_ty,
                        kind: TExprKind::PoolSlot {
                            pool: Box::new(pool_t),
                            id: Box::new(id_t),
                            mutable: true,
                            field: Some(field.to_string()),
                            line,
                        },
                    }));
                    let clone_value = if let Expr::Ident(vname, _) = value {
                        env.is_borrowed(vname) && env.ty_of(vname).is_some_and(|t| !t.is_scalar())
                    } else {
                        false
                    };
                    ready_return!(TStmt::Assign {
                        place,
                        op: *op,
                        value: lower_expr(value, cx, env),
                        clone_value,
                        line: crate::Diagnostics::span_line_col(&cx.src, op_span.start).0 as u32,
                    });
                }
                let field_expr = Expr::Field(base.clone(), field.clone(), *span);
                let place = TPlace::Expr(Box::new(lower_expr(&field_expr, cx, env)));
                // c150: mirror the lower_enum_arg clone predicate — a borrowed non-scalar
                // ident on the RHS would move out of a shared reference (E0507, I2).
                let clone_value = if let Expr::Ident(vname, _) = value {
                    env.is_borrowed(vname) && env.ty_of(vname).is_some_and(|t| !t.is_scalar())
                } else {
                    false
                };
                TStmt::Assign {
                    place,
                    op: *op,
                    value: lower_expr(value, cx, env),
                    clone_value,
                    line: crate::Diagnostics::span_line_col(&cx.src, op_span.start).0 as u32,
                }
            }
        },
        Stmt::Return(Some(Expr::Ident(name, _)), _) if env.gc_return && env.is_gc(name) => {
            TStmt::Return(Some(TExpr {
                ty: env.ty_of(name).unwrap_or(Type::Int),
                kind: TExprKind::Local(env.local_of(name)),
            }))
        }
        Stmt::Return(Some(e), _) => {
            let mut value = lower_owned_expr(e, cx, env);
            if let Some(want) = &env.ret_ty {
                value = crate::Codegen::TIR::maybe_widen_expr_to_union(value, want);
            }
            TStmt::Return(Some(value))
        }
        Stmt::Return(None, _) => TStmt::Return(None),
        // D-STREAMYIELD1: `yield e` inside a generator's spawned thread — send on
        // the channel the wrapping `Stream<T>` body opened (see `emit_generator_body`),
        // blocking (rendezvous, bound 0) until the consumer pulls. A closed receiver
        // (consumer stopped early) makes `send` fail; ignored — the thread just runs
        // to completion doing nothing further useful, rather than panicking.
        Stmt::Yield(e, _) => {
            let v = lower_expr(e, cx, env);
            TStmt::ExprStmt(TExpr {
                ty: unit_type(),
                kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::YieldSend {
                    value: Box::new(v),
                })),
            })
        }
        // D-IGNORERET2=A: `.drop("reason")` — lower only the receiver (for side effects).
        // The method call itself is erased; the "reason" string is audit-only.
        Stmt::Expr(Expr::Call(call)) if call.name == Syntax::INTERNAL_DEFER_CLOSE => {
            let close = call
                .args
                .first()
                .expect("parser creates one deferred close argument");
            let Expr::Call(close_call) = &close.expr else {
                unreachable!("parser creates a close call for deferred cleanup")
            };
            let Expr::Ident(resource, _) = &close_call.args[0].expr else {
                unreachable!("parser restricts deferred close to one resource binding")
            };
            TStmt::DeferClose {
                close: lower_expr(&close.expr, cx, env),
                resource: env.rust_name_of(resource),
                id: call.name_span.start,
            }
        }
        Stmt::Expr(Expr::MethodCall {
            receiver, method, ..
        }) if method == Syntax::METHOD_DROP => TStmt::ExprStmt(lower_expr(receiver, cx, env)),
        Stmt::Expr(e) => TStmt::ExprStmt(lower_expr(e, cx, env)),
        // c109 Phase 2: control-flow loops. Loop bodies are their own scope —
        // lower on a cloned env so bindings inside don't leak out.
        Stmt::Loop { body, label, .. } => {
            let branch = clone_env(env);
            let label = label_name(label);
            return deferred_stmt(
                vec![LowerBody::scoped(body, branch)],
                move |mut lowered| TStmt::Loop {
                    label,
                    body: lowered.pop().expect("loop body was deferred"),
                },
            );
        }
        Stmt::While {
            cond, body, label, ..
        } => {
            let cond = lower_expr(cond, cx, env);
            let branch = clone_env(env);
            let label = label_name(label);
            return deferred_stmt(
                vec![LowerBody::scoped(body, branch)],
                move |mut lowered| TStmt::While {
                    label,
                    cond,
                    body: lowered.pop().expect("while body was deferred"),
                },
            );
        }
        // D-LOOP-SEMICOLON1=A: `loop init; cond; step { body }` three-part counted loop.
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            label,
            ..
        } => {
            // The emitted outer Rust block owns the init binding and every loop-body
            // binding. Lower all of them in one child env so none survives the loop.
            let init_val = lower_expr(&init.init, cx, env);
            let init_ty = init
                .ty
                .clone()
                .unwrap_or_else(|| init_val.ty.clone());
            let mut_fn = interrupt_lambda(&init.init)
                .is_some_and(|lam| lam.meta.escapes && lam.meta.needs_fn_mut);
            let send_fn = env.is_send_fn(&init.name)
                && matches!(&init_ty, Type::Fn { .. })
                && !mut_fn;
            let init_val = if send_fn {
                force_interrupt_callback_value(init_val, cx)
            } else {
                init_val
            };
            let mut scoped = clone_env(env);
            scoped.bind(
                &init.name,
                TLocal::user(&init.name),
                Some(init_ty.clone()),
            );
            let init_stmt = Box::new(TStmt::Let {
                name: init.name.clone(),
                kw: "let mut",
                let_ty: if send_fn {
                    TLetTy::SendFn(init_ty)
                } else {
                    TLetTy::Inferred
                },
                init: init_val,
                gc_promotion: None,
                gc_transferred: false,
            });
            let cond = lower_expr(cond, cx, &mut scoped);
            let has_step = step.is_some();
            let mut bodies = Vec::with_capacity(if has_step { 2 } else { 1 });
            if let Some(step) = step {
                bodies.push(
                    LowerBody::direct(std::slice::from_ref(step.as_ref()), scoped.clone())
                        .carry_env(),
                );
            }
            bodies.push(LowerBody::inherited(body, scoped));
            let label = label_name(label);
            return deferred_stmt(bodies, move |mut lowered| {
                let body = lowered.pop().expect("counted-loop body was deferred");
                let step = has_step.then(|| {
                    Box::new(
                        lowered
                            .pop()
                            .expect("counted-loop step was deferred")
                            .into_iter()
                            .next()
                            .expect("counted-loop step was empty"),
                    )
                });
                TStmt::CountedLoop {
                    label,
                    init: init_stmt,
                    cond,
                    step,
                    body,
                }
            });
        }
        Stmt::For {
            var,
            var2,
            kind,
            body,
            label,
            ..
        } => match kind {
            ForKind::Range { start, end, step, exclusive } => {
                let start = lower_expr(start, cx, env);
                let end = lower_expr(end, cx, env);
                let step = step.as_ref().map(|s| lower_expr(s, cx, env));
                // The loop var is an `Int` local for the body's scope only. Panic
                // context inside the body sees it; a panic after the loop does not.
                let mut branch = clone_env(env);
                branch.bind(var, TLocal::user(var), Some(Type::Int));
                let label = label_name(label);
                let var = var.clone();
                let exclusive = *exclusive;
                return deferred_stmt(
                    vec![LowerBody::scoped(body, branch)],
                    move |mut lowered| TStmt::Range {
                        label,
                        var,
                        source: None,
                        start,
                        end,
                        step,
                        exclusive,
                        body: lowered.pop().expect("range body was deferred"),
                    },
                );
            }
            // c109 Phase 5: collection iteration `loop x; coll` / `loop k, v; map`.
            // The collection string is resolved once. The loop var(s) bind in the body
            // scope with an *unresolved* type (`None`) — matching the AST slot's
            // `jet_ty: None`, so they never enable the overflow trap (parity).
            ForKind::In { collection, step } => {
                // c109 Phase 22: classify a method-call collection into the matching
                // `emit_for_in` branch (`chars`/`lines`/the `.iter().cloned()` default),
                // resolving the receiver/collection string off the SAME node shape the
                // AST path reads. `method_kind == None` is the plain `.iter()` form.
                let (iter_source, method_kind) = lower_forin_collection(collection, cx, env);
                // Infer the element type from the lowered collection so the loop
                // variable binds with its concrete type. This lets `core_struct_field_rust_name`
                // emit plain field names (not `__jet_<field>`) for core types like DirEntry.
                let lowered_coll = lower_expr(collection, cx, env);
                if matches!(&lowered_coll.ty, Type::Named(name) if name == Syntax::TYPE_RANGE) {
                    let step = step.as_ref().map(|s| lower_expr(s, cx, env));
                    let zero = || TExpr {
                        ty: Type::Int,
                        kind: TExprKind::IntLit(0, None),
                    };
                    let mut branch = clone_env(env);
                    branch.bind(var, TLocal::user(var), Some(Type::Int));
                    let label = label_name(label);
                    let var = var.clone();
                    return deferred_stmt(
                        vec![LowerBody::scoped(body, branch)],
                        move |mut lowered| TStmt::Range {
                            label,
                            var,
                            source: Some(lowered_coll),
                            start: zero(),
                            end: zero(),
                            step,
                            exclusive: false,
                            body: lowered.pop().expect("range body was deferred"),
                        },
                    );
                }
                let mut method_kind = method_kind;
                let mut coll_elem_ty: Option<Type> = match &lowered_coll.ty {
                    Type::List(inner) => Some((**inner).clone()),
                    Type::FixedList { elem, .. } => Some((**elem).clone()),
                    Type::Map { key, value, .. } => Some(Type::Tuple(vec![
                        ("key".to_string(), Box::new((**key).clone())),
                        ("value".to_string(), Box::new((**value).clone())),
                    ])),
                    // D-STREAMYIELD1: a generator's `Stream<T>`.
                    Type::Apply { name, args } if name == "Stream" && args.len() == 1 => {
                        Some(args[0].clone())
                    }
                    // D-ITERTOOLS1=A: lazy `Iter<T>` view — element is T.
                    Type::Apply { name, args }
                        if name == crate::Syntax::TYPE_ITER && args.len() == 1 =>
                    {
                        Some(args[0].clone())
                    }
                    Type::Named(name) if name == "HTTPBodyChunks" => Some(Type::Result {
                        ok: Box::new(Type::List(Box::new(Type::Named("U8".to_string())))),
                        err: Box::new(Type::Named("HTTPError".to_string())),
                    }),
                    Type::Named(name) => encoding_reader_item_type(name),
                    // D-DYNARRAY1: `loop x; window` — a `View<T>`'s element type.
                    Type::Apply { name, args }
                        if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut")
                            && args.len() == 1 => {
                        Some(args[0].clone())
                    }
                    _ => None,
                };
                // A list whose elements cannot be cloned is iterated by value: the
                // loop consumes the collection instead of asking rustc for a
                // `.cloned()` that does not exist (I2 — that reaches the user as
                // an internal compiler error, never a diagnostic).
                // A `ViewMut` element is not cloneable either, but a list of them
                // already has its own iteration form below; leave it alone.
                let elem_is_view_mut = matches!(
                    coll_elem_ty.as_ref(),
                    Some(Type::Apply { name, .. })
                        if matches!(name.as_str(), "ViewMut" | "ComputeViewMut")
                );
                // Only a type carrying a task handle is consumed here. codegen's
                // `field_type_cloneable` answers a narrower question than sema's
                // `is_cloneable` (it treats every core `Named` type as
                // non-cloneable because no derive is emitted for one), so using
                // it directly would consume ordinary `[BigInt]` / `[Duration]`
                // lists that sema still believes are copied — an ICE, not a fix.
                // The shared predicate reaches inside containers, so a
                // `[[Task<Int>]]` element is uncopyable too (it is a `Vec` of a
                // type with no `Clone`), not just a bare `Task<T>`.
                let elem_is_cloneable = !coll_elem_ty
                    .as_ref()
                    .is_some_and(crate::Sema::type_holds_task_handle);
                let by_value = matches!(&lowered_coll.ty,
                    Type::Apply { name, .. } if name == "Stream" || name == crate::Syntax::TYPE_ITER
                ) || matches!(&lowered_coll.ty, Type::Named(name) if name == "HTTPBodyChunks")
                    || (method_kind.is_none()
                        && !elem_is_cloneable
                        && !elem_is_view_mut
                        && matches!(
                            &lowered_coll.ty,
                            Type::List(_) | Type::FixedList { .. }
                        ));
                if method_kind.is_none() {
                    if let Type::Named(n) = &lowered_coll.ty {
                        if let Some(hook) = cx.iterable_hooks.get(n) {
                            method_kind = Some(TForInMethod::Iterable {
                                coll_type: n.clone(),
                                iter_type: hook.iter_type.clone(),
                            });
                            coll_elem_ty = Some(hook.item_type.clone());
                        }
                    }
                }
                let mut branch = clone_env(env);
                branch.bind(var, TLocal::user(var), coll_elem_ty.clone());
                if let Some((v2, _)) = var2 {
                    // Two-binding: map → key/value; sequence → index/item (D-RANGE-EXCL1=C).
                    match &lowered_coll.ty {
                        Type::Map { key, value, .. } => {
                            branch.bind(var, TLocal::user(var), Some((**key).clone()));
                            branch.bind(v2, TLocal::user(v2), Some((**value).clone()));
                        }
                        Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                            branch.bind(var, TLocal::user(var), Some(Type::Int));
                            branch.bind(v2, TLocal::user(v2), Some((**inner).clone()));
                        }
                        Type::Apply { name, args }
                            if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut")
                                && args.len() == 1 =>
                        {
                            branch.bind(var, TLocal::user(var), Some(Type::Int));
                            branch.bind(v2, TLocal::user(v2), Some(args[0].clone()));
                        }
                        _ => {
                            branch.bind(v2, TLocal::user(v2), None);
                        }
                    }
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
                let step = step.as_ref().map(|step| lower_expr(step, cx, env));
                let label = label_name(label);
                let var = var.clone();
                let var2 = var2.as_ref().map(|(n, _)| n.clone());
                return deferred_stmt(
                    vec![LowerBody::scoped(body, branch)],
                    move |mut lowered| TStmt::ForIn {
                        label,
                        var,
                        var2,
                        source: iter_source,
                        collection: lowered_coll,
                        step,
                        method_kind,
                        columnar,
                        by_value,
                        body: lowered.pop().expect("for-in body was deferred"),
                    },
                );
            }
        },
        Stmt::Break(_) => TStmt::Break(None),
        Stmt::BreakValue(value, _) => TStmt::BreakValue {
            label: None,
            value: lower_expr(value, cx, env),
        },
        Stmt::Continue(_) => TStmt::Continue(None),
        Stmt::BreakLabel(name, _) => TStmt::Break(Some(name.clone())),
        Stmt::BreakLabelValue(name, _, value, _) => TStmt::BreakValue {
            label: Some(name.clone()),
            value: lower_expr(value, cx, env),
        },
        Stmt::ContinueLabel(name, _) => TStmt::Continue(Some(name.clone())),
        // c109 Phase 4: a `when`/match. The gate already classified it as either an
        // exhaustive enum match (shape A) or an all-range scalar switch (shape B).
        Stmt::Switch {
            subject,
            arms,
            else_body,
            span,
        } => return lower_switch(subject, arms, else_body, *span, cx, env),
        // D-META-STAGE1=B (formerly D-CTMARKER1, ratified 2026-06-25, piece 2): `$ { … }` runs at
        // build time and erases entirely — no runtime Rust is emitted (I3).
        Stmt::ComptimeBlock { .. } => TStmt::Inline(vec![]),
        // D-CANVASSTATE1=D: `#Off` type-checks in sema but emits no runtime TIR.
        Stmt::Switched { marker, .. } if crate::AST::switched_off(marker) => TStmt::Inline(vec![]),
        // D-CANVASSTATE1=D: `#DebugOnly` is a lexical debug-only region. Lower
        // on a cloned env so declarations cannot be required by release code.
        Stmt::Switched { body, .. } => {
            let scoped = clone_env(env);
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                |mut lowered| TStmt::DebugOnly(lowered.pop().expect("debug body was deferred")),
            );
        }
        // Lexical-scope rule: whenever `emit_tir_stmt` opens a Rust `{ ... }`, lower
        // declarations in a cloned env. Only three statement forms deliberately reuse
        // the parent env because emission is inline with no Rust block: selected
        // comptime-if, `layout`, and `.setup`.
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
            let scoped = clone_env(env);
            return deferred_stmt(
                vec![LowerBody::inline(chosen, scoped)],
                |mut lowered| TStmt::Inline(lowered.pop().expect("comptime-if body was deferred")),
            );
        }
        // c109 Phase 18: an audited `#Unsafe { … }` region (`Stmt::Unsafe`). Emission
        // adds a Rust lexical block, so lower its declarations in a child env. The `#Audit("…")`
        // annotation is dropped (codegen is dumb — it emits nothing, matching the AST).
        // I1: the source `#Unsafe` gate is 1:1 with this node, the only producer of a
        // Rust `unsafe` block.
        Stmt::Unsafe { body, .. } => {
            let scoped = clone_env(env);
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                |mut lowered| TStmt::Unsafe(lowered.pop().expect("unsafe body was deferred")),
            );
        }
        // D-CTEFFECT1: preserve the policy gate for canonical comptime evaluation.
        // AOT/JIT still execute its body as a plain lexical block.
        Stmt::Impure { body, .. } => {
            let scoped = clone_env(env);
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                |mut lowered| TStmt::Impure(lowered.pop().expect("impure body was deferred")),
            );
        }
        // D-REACTCORE1: `#Reactive { … }` lowers to `jet_reactive_effect(closure)`.
        // Clone outer captures into the closure (same as a stored lambda).
        Stmt::Reactive { body, span, .. } => {
            let outer_env = clone_env(env);
            let body_env = reactive_block_env(body, cx, env);
            // Synthetic zero-arg lambda so JIT can compile the body with captures.
            let synthetic = crate::AST::Lambda {
                take_names: Vec::new(),
                params: Vec::new(),
                body: crate::AST::LambdaBody::Block(body.clone()),
                span: *span,
                meta: {
                    let reads = crate::Sema::block_free_var_reads(body);
                    let mut meta = crate::AST::LambdaMeta::default();
                    meta.cloned_captures = reads
                        .into_iter()
                        .filter(|n| outer_env.locals.contains_key(n))
                        .collect();
                    meta.cloned_captures.sort();
                    meta.needs_fn_mut = true;
                    meta.escapes = true;
                    meta
                },
            };
            return deferred_stmt(
                vec![LowerBody::scoped(body, body_env)],
                move |mut lowered| {
                    let lowered = lowered.pop().expect("reactive body was deferred");
                    let shared: Arc<[TStmt]> = Arc::from(lowered.into_boxed_slice());
                    let closure =
                        render_reactive_block_closure(body, &shared[..], cx, &outer_env);
                    let executable = Box::new(lower_lambda_with_shared_block(
                        &synthetic,
                        cx,
                        &outer_env,
                        shared.clone(),
                    ));
                    let jit_lambda = lower_spawn_lambda_for_jit_with_shared_block(
                        &synthetic,
                        cx,
                        &outer_env,
                        shared,
                    );
                    cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
                    TStmt::Reactive {
                        closure,
                        executable,
                    }
                },
            );
        }
        // D-SHIELDNAME1=A: `#Shield { … }` lowers to a shield-guarded lexical block.
        Stmt::Shield { body, .. } => {
            let scoped = clone_env(env);
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                |mut lowered| TStmt::Shield {
                    body: lowered.pop().expect("shield body was deferred"),
                },
            );
        }
        // c109 Phase 19: an explicit `region r { … }` (D-REGION1) emits a plain
        // Rust lexical block.
        Stmt::Region { body, .. } => {
            let scoped = clone_env(env);
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                |mut lowered| TStmt::Region(lowered.pop().expect("region body was deferred")),
            );
        }
        Stmt::Policy { body, .. } => {
            let scoped = clone_env(env);
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                |mut lowered| TStmt::Region(lowered.pop().expect("policy body was deferred")),
            );
        }
        // D-TASKSCOPE1=A / D-TASKGROUP-PARAM1=A: the lexical block owns one
        // internal collector. Helpers borrow this same value.
        Stmt::TaskGroup { name, limit, body, .. } => {
            let mut scoped = clone_env(env);
            let group_ty = Type::Named(Syntax::TYPE_TASKGROUP.to_string());
            let group = TLocal::user(name);
            scoped.bind(name, group.clone(), Some(group_ty));
            let limit = limit.as_ref().map(|value| lower_expr(value, cx, env));
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                move |mut lowered| TStmt::TaskGroup {
                    group,
                    limit,
                    body: lowered.pop().expect("task-group body was deferred"),
                },
            );
        }
        // D-LAYOUT1 / D-LAYOUT-GATES1: `layout name { … }` needs a public
        // runtime object (unlike the compiler-private TaskGroup handle) — bind `name`
        // to a fresh `jet_layout::Handle` BEFORE lowering the body, so the
        // desugared `name.h(box, anchor)` calls inside resolve to it, exactly
        // like an ordinary `name :: jet_layout::Handle::new(…)` binding would.
        Stmt::Layout { name, body, .. } => {
            let handle = TLocal::user(name);
            let mut scoped = clone_env(env);
            scoped.bind(
                name,
                handle.clone(),
                Some(Type::Named(Syntax::LAYOUT_TYPE.to_string())),
            );
            let label = name.clone();
            return deferred_stmt(
                vec![LowerBody::inline(body, scoped)],
                move |mut lowered| TStmt::Layout {
                    handle,
                    label,
                    body: lowered.pop().expect("layout body was deferred"),
                },
            );
        }
        // c109 Phase 26: a `#Caps(IO) { … }` effect-restriction region (D-EFF1). `emit_stmt`'s
        // `Stmt::Caps` arm is byte-for-byte `Stmt::Region`; effects erase at codegen (I3).
        Stmt::Caps { body, .. } => {
            let scoped = clone_env(env);
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                |mut lowered| TStmt::Region(lowered.pop().expect("caps body was deferred")),
            );
        }
        // D-SCAP1: a `#grant(FS) { caps -> … }` grant region. The capability handle
        // is a compile-time-only fact (authority to perform the granted effects),
        // erased here (I3); the body emits as a plain lexical `TStmt::Region`.
        // No runtime grant/revoke value, no `unsafe`.
        Stmt::Grant { body, .. } => {
            let scoped = clone_env(env);
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                |mut lowered| TStmt::Region(lowered.pop().expect("grant body was deferred")),
            );
        }
        // c109 Phase 19: a `#Context(field: value) { … }` block (D-CTX1/D-DEADLINE1).
        // Resolve each field against the outer env, then lower the guarded Rust block
        // in a lexical child env.
        Stmt::ContextBlock { fields, body, .. } => {
            let guards = fields
                .iter()
                .map(|(name, v, _)| (name.clone(), lower_expr(v, cx, env)))
                .collect();
            let scoped = clone_env(env);
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                move |mut lowered| TStmt::ContextBlock {
                    guards,
                    body: lowered.pop().expect("context body was deferred"),
                },
            );
        }
        // D-TERM1: `live { … }` emits an enter/guard/leave Rust lexical block.
        Stmt::Live { body, .. } => {
            let scoped = clone_env(env);
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                |mut lowered| TStmt::Live {
                    body: lowered.pop().expect("live body was deferred"),
                },
            );
        }
        // D-DOTSCOPE1: a `#Test` scope member (`.setup`/`.expect_fail`/`.timeout`/
        // `.skip`). Legality/args were checked in sema; here we pick the lowering
        // kind and fold `.timeout`'s duration literal to a nanosecond budget.
        // `.setup` emits inline, so its bindings are visible to the rest of the test;
        // the others open their own scope in
        // `emit_tir_stmt`.
        Stmt::ScopeMember {
            name, args, body, ..
        } => {
            if crate::Syntax::is_stdlib_dsl_block_marker(name) {
                let scoped = clone_env(env);
                return deferred_stmt(
                    vec![LowerBody::scoped(body, scoped)],
                    |mut lowered| TStmt::Region(lowered.pop().expect("scope body was deferred")),
                );
            }
            let kind = if name == Syntax::SCOPE_TEST_SETUP {
                ScopeMemberKind::Setup
            } else if name == Syntax::SCOPE_TEST_EXPECT_FAIL {
                ScopeMemberKind::ExpectFail
            } else if name == Syntax::SCOPE_TEST_TIMEOUT {
                ScopeMemberKind::Timeout(timeout_nanos(args))
            } else {
                ScopeMemberKind::Skip
            };
            let scoped = clone_env(env);
            let body_plan = if matches!(&kind, ScopeMemberKind::Setup) {
                // `.setup` is emitted inline so its declarations intentionally remain
                // available to later statements in the test.
                LowerBody::inline(body, scoped)
            } else {
                LowerBody::scoped(body, scoped)
            };
            return deferred_stmt(vec![body_plan], move |mut lowered| TStmt::ScopeMember {
                kind,
                body: lowered.pop().expect("scope member body was deferred"),
            });
        }
        // D-DET1: `assume_deterministic { … }` erases to a plain `TStmt::Region`
        // (byte-for-byte the `Stmt::Region`/`Stmt::Caps` shape). The determinism
        // suspension is a sema-only fact; nothing runtime, no `unsafe` (I3).
        Stmt::AssumeDet { body, .. } => {
            let scoped = clone_env(env);
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                |mut lowered| TStmt::Region(lowered.pop().expect("determinism body was deferred")),
            );
        }
        // D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` block. Bind the
        // handle (typed `Transaction`) in a child env so `name.on_commit(…)` lowers
        // against it without escaping the emitted Rust block. The
        // `let mut <handle> = jet_transaction(); … <handle>.commit();` framing is
        // emitted in `emit_tir_stmt`; codegen is dumb (I3).
        Stmt::Transact { name, body, .. } => {
            let mut scoped = clone_env(env);
            let handle = name.as_ref().map(|name| {
                let slot = TLocal::user(name);
                scoped.bind(
                    name,
                    slot.clone(),
                    Some(Type::Named(Syntax::TXN_HANDLE_TYPE.to_string())),
                );
                slot
            });
            // D-TXN-ROLLBACK layer 1 (auto-snapshot): collect the root local names
            // assigned anywhere in the block (recursing into nested control flow, but
            // NOT into nested `#Transact` blocks or lambda bodies — those own their
            // own rollback scope / are deferred). Snapshot only roots ALREADY in scope
            // at block entry (params / outer locals): a local declared inside the block
            // needs no snapshot, since rollback discards it when the block scope ends.
            let mut roots: Vec<String> = Vec::new();
            collect_txn_mut_roots(body, &mut roots);
            let snapshots: Vec<(TLocal, Option<Type>)> = roots
                .iter()
                .filter(|r| env.locals.contains_key(*r))
                .map(|r| {
                    // D-TXN-ROLLBACK layer 2: if the root type implements Rollback,
                    // use snapshot_custom instead of the clone-based snapshot path.
                    let rollback_ty = env.ty_of(r).filter(|ty| {
                        matches!(ty, Type::Named(n) if cx.rollback_types.contains(n))
                    });
                    (env.local_of(r), rollback_ty)
                })
                .collect();
            // D-STM1=A (card #506): lower the body with `in_stm_transact` raised so a
            // `Shared<T>.edit` inside routes to the deferred `edit_txn`. `stm_touched`
            // is reset first and read after, so the emitted STM handle reflects THIS
            // block only
            // (save/restore isolates nested blocks); a Shared edit in a nested
            // `#Transact` attaches to that inner block's own transaction, not this one.
            let prev_in = cx.in_stm_transact.replace(true);
            let prev_touched = cx.stm_touched.replace(false);
            return deferred_stmt(
                vec![LowerBody::scoped(body, scoped)],
                move |mut lowered| {
                    let stm = cx.stm_touched.get().then(TLocal::stm);
                    cx.in_stm_transact.set(prev_in);
                    cx.stm_touched.set(prev_touched);
                    TStmt::Transact {
                        handle,
                        snapshots,
                        stm,
                        body: lowered.pop().expect("transaction body was deferred"),
                    }
                },
            );
        }
        // Forward-safety default: a Stmt variant not in the subset never reaches
        // lowering (`stmt_in_subset` returns false for it). Kept as a guard against a
        // future variant; currently unreachable because every covered variant is matched.
        #[allow(unreachable_patterns)]
        _ => unreachable!("statement not in TIR subset"),
    })
}

/// W4 (durability): proves the sema-to-TIR handoff `debug_assert`s in
/// `lower_expr`'s `Expr::Index` arm and this file's `LValue::Index` arm
/// actually trip on a leaked `IndexKind::Unknown` — the exact ice_regressions
/// b5 bug class (sema left the index kind unresolved; the subset gate is
/// supposed to exclude it, but a gate bug could let one through). These are
/// `#[should_panic]` because the debug_assert is the thing under test, not a
/// normal lowering path — the subset gate itself still excludes `Unknown` in
/// every real compile.
#[cfg(test)]
mod handoff_assert_tests {
    use super::*;

    fn empty_cx() -> Cx {
        let src = "fn run() {}\n";
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        build_cx(&prog, src, "test.jet")
    }

    #[test]
    #[should_panic(expected = "sema-to-TIR handoff violated")]
    fn index_read_unknown_kind_trips_handoff_assert() {
        let cx = empty_cx();
        let mut env = LowerEnv::new("run".to_string());
        let idx_expr = Expr::Index {
            base: Box::new(Expr::Int(0, Span::new(0, 0), None, None)),
            index: Box::new(Expr::Int(0, Span::new(0, 0), None, None)),
            span: Span::new(0, 0),
            kind: IndexKind::Unknown, // seeded leak: sema never resolved this
        };
        let _ = lower_expr(&idx_expr, &cx, &mut env);
    }

    #[test]
    #[should_panic(expected = "sema-to-TIR handoff violated")]
    fn index_assign_unknown_kind_trips_handoff_assert() {
        let cx = empty_cx();
        let mut env = LowerEnv::new("run".to_string());
        let stmt = Stmt::Assign {
            target: LValue::Index {
                base: Box::new(Expr::Int(0, Span::new(0, 0), None, None)),
                index: Box::new(Expr::Int(0, Span::new(0, 0), None, None)),
                span: Span::new(0, 0),
                kind: IndexKind::Unknown, // seeded leak: sema never resolved this
            },
            op: None,
            op_span: Span::new(0, 0),
            value: Expr::Int(1, Span::new(0, 0), None, None),
        };
        let _ = lower_stmts(std::slice::from_ref(&stmt), &cx, &mut env);
    }
}
