//! c139 (D-JITDEP1 / D-JIT2=A) — Cranelift JIT tier-1 backend.
//!
//! Architecture: CraneliftBackend<F: JitBackend> where F is the tier-0
//! fallback. M0 delegates everything to F; M1 adds jit_covers() and
//! lower_tir_clif() to actually compile + run the covered subset natively.
//! M2 keeps a resident JIT module + live runtime heap across hot_swap.
//! M3 widens jit_covers: arithmetic, bindings, if/else, calls, loops,
//! compound assign, &&/|| short-circuit.
//! M4: tasks/channels/spawn via scheduler host shims (D-ASYNCRT1=A).

mod concurrency;

use jet_codegen::scheduler::{JetSchedulerChannel, JetSchedulerJoin, JetSchedulerSender};
// I6: Cranelift crates live here, not in the compiler `jet` crate (`Source/`).
// The root package depends on jet-jit; jet-jit depends on cranelift-*.
// D-JITDEP1 approved this as a scoped runtime-side exception.

use jet_foundation::{
    AST::{BinOp, ProgramBundle, Type, UnOp},
    Diagnostics::Diagnostic,
    JitBackend::{JitBackend, RunOutcome},
};

use cranelift_codegen::ir::{
    condcodes::{FloatCC, IntCC},
    types, AbiParam, Block, InstBuilder, Signature, Value,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use jet_codegen::Codegen::TIR::{
    self, JitSpawnCapture, TJitSpawnBody, TJitSpawnLambda, JitProgram, TCallArg, TCoreClosureKind,
    TExpr, TExprKind, TFunc, TFuncKind, THandleOp, TIfCond, TOrFallback, TStmt, TStrPart,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

thread_local! {
    /// Compiled module + entry symbol; re-linked on hot_swap.
    static RESIDENT_MODULE: RefCell<Option<ResidentModule>> = const { RefCell::new(None) };
    /// Live heap preserved across type-stable hot_swap; reset on restart.
    static RESIDENT_RUNTIME: RefCell<Option<JitRuntime>> = const { RefCell::new(None) };
}

/// Live heap carried across type-stable hot_swap (M2). `invocations` counts
/// how many times `main` ran without a clean restart — preserved on swap,
/// reset on restart.
pub(crate) struct JitRuntime {
    source_file: String,
    stdout: String,
    stderr: String,
    strings: Vec<String>,
    invocations: u64,
    channels: Vec<JetSchedulerChannel<i64>>,
    senders: Vec<JetSchedulerSender<i64>>,
    tasks: Vec<Option<JetSchedulerJoin<i64>>>,
    next_spawn_id: u32,
}

struct ResidentModule {
    module: JITModule,
    host: HostFns,
    main_id: FuncId,
}

fn with_runtime_mut<F: FnOnce(&mut JitRuntime)>(f: F) {
    concurrency::with_runtime_mut(f);
}

fn render_float(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        v.to_string()
    }
}

fn jet_trap_overflow(op: &str, line: u32) -> ! {
    with_runtime_mut(|rt| {
        let msg = match op {
            "add" => "this addition overflows the value's type (the result is outside its range)",
            "sub" => "this subtraction overflows the value's type (the result is outside its range)",
            "mul" => "this multiplication overflows the value's type (the result is outside its range)",
            "div" => "this division can't be done (dividing by zero, or overflow)",
            _ => "this operation overflows the value's type (the result is outside its range)",
        };
        rt.stderr.push_str(&format!("panic: {msg}\n"));
        rt.stderr
            .push_str(&format!("  --> {}:{line}\n", rt.source_file));
    });
    std::process::exit(70);
}

extern "C" fn jet_jit_add_i64(a: i64, b: i64, line: u32) -> i64 {
    a.checked_add(b)
        .unwrap_or_else(|| jet_trap_overflow("add", line))
}

extern "C" fn jet_jit_sub_i64(a: i64, b: i64, line: u32) -> i64 {
    a.checked_sub(b)
        .unwrap_or_else(|| jet_trap_overflow("sub", line))
}

extern "C" fn jet_jit_mul_i64(a: i64, b: i64, line: u32) -> i64 {
    a.checked_mul(b)
        .unwrap_or_else(|| jet_trap_overflow("mul", line))
}

extern "C" fn jet_jit_div_i64(a: i64, b: i64, line: u32) -> i64 {
    a.checked_div(b)
        .unwrap_or_else(|| jet_trap_overflow("div", line))
}

extern "C" fn jet_jit_print_i64(v: i64) {
    with_runtime_mut(|rt| {
        rt.stdout.push_str(&v.to_string());
        rt.stdout.push('\n');
    });
}

extern "C" fn jet_jit_print_f64(v: f64) {
    with_runtime_mut(|rt| {
        rt.stdout.push_str(&render_float(v));
        rt.stdout.push('\n');
    });
}

extern "C" fn jet_jit_print_bool(v: i8) {
    with_runtime_mut(|rt| {
        rt.stdout.push_str(if v == 0 { "false" } else { "true" });
        rt.stdout.push('\n');
    });
}

extern "C" fn jet_jit_print_char(v: i32) {
    with_runtime_mut(|rt| {
        match char::from_u32(v as u32) {
            Some(ch) => rt.stdout.push(ch),
            None => rt.stdout.push('?'),
        }
        rt.stdout.push('\n');
    });
}

extern "C" fn jet_jit_print_str(id: i64) {
    with_runtime_mut(|rt| {
        if let Some(s) = rt.strings.get(id as usize) {
            rt.stdout.push_str(s);
            rt.stdout.push('\n');
        }
    });
}

struct HostFns {
    add_i64: FuncId,
    sub_i64: FuncId,
    mul_i64: FuncId,
    div_i64: FuncId,
    print_i64: FuncId,
    print_f64: FuncId,
    print_bool: FuncId,
    print_char: FuncId,
    print_str: FuncId,
    conc: concurrency::ConcurrencyHostFns,
}

fn new_jit_module() -> Result<(JITModule, HostFns), String> {
    let mut builder =
        JITBuilder::new(cranelift_module::default_libcall_names()).map_err(|e| e.to_string())?;
    builder.symbol("jet_jit_add_i64", jet_jit_add_i64 as *const u8);
    builder.symbol("jet_jit_sub_i64", jet_jit_sub_i64 as *const u8);
    builder.symbol("jet_jit_mul_i64", jet_jit_mul_i64 as *const u8);
    builder.symbol("jet_jit_div_i64", jet_jit_div_i64 as *const u8);
    builder.symbol("jet_jit_print_i64", jet_jit_print_i64 as *const u8);
    builder.symbol("jet_jit_print_f64", jet_jit_print_f64 as *const u8);
    builder.symbol("jet_jit_print_bool", jet_jit_print_bool as *const u8);
    builder.symbol("jet_jit_print_char", jet_jit_print_char as *const u8);
    builder.symbol("jet_jit_print_str", jet_jit_print_str as *const u8);
    concurrency::register_concurrency_symbols(&mut builder);
    let mut module = JITModule::new(builder);
    let conc = concurrency::declare_concurrency_host_fns(&mut module)?;
    let host = declare_host_fns(&mut module, conc)?;
    Ok((module, host))
}

fn declare_host_fns(
    module: &mut JITModule,
    conc: concurrency::ConcurrencyHostFns,
) -> Result<HostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut sig_bin_i64 = Signature::new(cc);
    sig_bin_i64.params.push(AbiParam::new(types::I64));
    sig_bin_i64.params.push(AbiParam::new(types::I64));
    sig_bin_i64.params.push(AbiParam::new(types::I32));
    sig_bin_i64.returns.push(AbiParam::new(types::I64));

    let mut sig_i64 = Signature::new(cc);
    sig_i64.params.push(AbiParam::new(types::I64));
    let mut sig_f64 = Signature::new(cc);
    sig_f64.params.push(AbiParam::new(types::F64));
    let mut sig_i8 = Signature::new(cc);
    sig_i8.params.push(AbiParam::new(types::I8));
    let mut sig_i32 = Signature::new(cc);
    sig_i32.params.push(AbiParam::new(types::I32));

    let mut import = |name: &str, sig: &Signature| -> Result<FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };

    Ok(HostFns {
        add_i64: import("jet_jit_add_i64", &sig_bin_i64)?,
        sub_i64: import("jet_jit_sub_i64", &sig_bin_i64)?,
        mul_i64: import("jet_jit_mul_i64", &sig_bin_i64)?,
        div_i64: import("jet_jit_div_i64", &sig_bin_i64)?,
        print_i64: import("jet_jit_print_i64", &sig_i64)?,
        print_f64: import("jet_jit_print_f64", &sig_f64)?,
        print_bool: import("jet_jit_print_bool", &sig_i8)?,
        print_char: import("jet_jit_print_char", &sig_i32)?,
        print_str: import("jet_jit_print_str", &sig_i64)?,
        conc,
    })
}

fn flatten_string(parts: &[TStrPart]) -> Option<String> {
    let mut out = String::new();
    for p in parts {
        match p {
            TStrPart::Lit(s) => out.push_str(s),
            TStrPart::Interp(_) => return None,
        }
    }
    Some(out)
}

fn jit_scalar_type(ty: &Type) -> bool {
    jit_value_type(ty)
}

fn jit_concurrency_elem(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if n == "Unit") || jit_scalar_type(ty)
}

fn jit_concurrency_type(ty: &Type) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    matches!(name.as_str(), "Task" | "Channel" | "Sender")
        && args.len() == 1
        && jit_concurrency_elem(&args[0])
}

fn jit_value_type(ty: &Type) -> bool {
    match ty {
        Type::Named(n) if n == "Unit" => true,
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => true,
        other => jit_concurrency_type(other),
    }
}

fn jit_covers_expr(expr: &TExpr, callees: &HashSet<String>) -> bool {
    match &expr.kind {
        TExprKind::Print(inner) => jit_covers_expr(inner, callees),
        TExprKind::Call { name, args } => {
            if !callees.contains(name) {
                return false;
            }
            args.iter().all(|a| {
                !a.borrow
                    && !a.mut_borrow
                    && !a.clone
                    && !a.arc_clone
                    && a.fn_coerce.is_none()
                    && !a.widen_to_vec
                    && jit_covers_expr(&a.value, callees)
            })
        }
        TExprKind::CoreCall { module, method, args } => {
            (module == "core.tasks" && method == "channel" && args.is_empty())
                || jit_covers_expr_list(args, callees)
        }
        TExprKind::CoreClosureCall { kind } => match kind {
            TCoreClosureKind::Spawn { .. } => true,
            _ => false,
        },
        TExprKind::HandleMethod { recv, op, args } => {
            jit_covers_expr(recv, callees)
                && args.iter().all(|a| jit_covers_expr(a, callees))
                && jit_covers_handle_op(op, recv, args)
        }
        TExprKind::OrFallback {
            value,
            fallback,
            is_option,
        } => {
            !is_option
                && jit_covers_expr(value, callees)
                && matches!(fallback, TOrFallback::Panic(_))
        }
        _ if !jit_value_type(&expr.ty) => false,
        TExprKind::IntLit(_, _)
        | TExprKind::FloatLit(_)
        | TExprKind::BoolLit(_)
        | TExprKind::CharLit(_) => true,
        TExprKind::StrLit(parts) => flatten_string(parts).is_some(),
        TExprKind::Local(_) => true,
        TExprKind::Unary { op, operand } => {
            matches!(op, UnOp::Neg | UnOp::Not) && jit_covers_expr(operand, callees)
        }
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
                    && jit_covers_expr(lhs, callees)
                    && jit_covers_expr(rhs, callees);
            }
            if *overflow && (!matches!(&lhs.ty, Type::Int) || !matches!(&rhs.ty, Type::Int)) {
                return false;
            }
            jit_covers_expr(lhs, callees) && jit_covers_expr(rhs, callees)
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            matches!(&cond.ty, Type::Bool)
                && then_body.iter().all(|s| jit_covers_stmt(s, callees))
                && jit_covers_expr(then_value, callees)
                && else_body.iter().all(|s| jit_covers_stmt(s, callees))
                && jit_covers_expr(else_value, callees)
        }
        TExprKind::Clone(inner) => jit_covers_expr(inner, callees),
        _ => false,
    }
}

fn jit_covers_stmt(stmt: &TStmt, callees: &HashSet<String>) -> bool {
    match stmt {
        TStmt::Let { init, .. } => jit_covers_expr(init, callees),
        TStmt::Assign {
            value,
            clone_value,
            ..
        } => !clone_value && jit_covers_expr(value, callees),
        TStmt::Return(ret) => ret.as_ref().is_none_or(|e| jit_covers_expr(e, callees)),
        TStmt::ExprStmt(e) => jit_covers_expr(e, callees),
        TStmt::If {
            cond,
            then_body,
            else_body,
            ..
        } => {
            let cond_ok = match cond {
                TIfCond::Plain(e) => matches!(&e.ty, Type::Bool) && jit_covers_expr(e, callees),
                TIfCond::IfLet { .. } | TIfCond::IsNone { .. } => false,
            };
            cond_ok
                && then_body.iter().all(|s| jit_covers_stmt(s, callees))
                && else_body
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| jit_covers_stmt(s, callees)))
        }
        TStmt::Loop { label, body } => {
            label.is_none() && body.iter().all(|s| jit_covers_stmt(s, callees))
        }
        TStmt::While {
            label,
            cond,
            body,
        } => {
            label.is_none()
                && matches!(&cond.ty, Type::Bool)
                && jit_covers_expr(cond, callees)
                && body.iter().all(|s| jit_covers_stmt(s, callees))
        }
        TStmt::CountedLoop {
            label,
            init,
            cond,
            step,
            body,
        } => {
            label.is_none()
                && jit_covers_stmt(init, callees)
                && matches!(&cond.ty, Type::Bool)
                && jit_covers_expr(cond, callees)
                && jit_covers_stmt(step, callees)
                && body.iter().all(|s| jit_covers_stmt(s, callees))
        }
        TStmt::Range {
            label,
            start,
            end,
            step,
            body,
            ..
        } => {
            label.is_none()
                && matches!(&start.ty, Type::Int)
                && matches!(&end.ty, Type::Int)
                && step.as_ref().is_none_or(|s| matches!(&s.ty, Type::Int))
                && jit_covers_expr(start, callees)
                && jit_covers_expr(end, callees)
                && step.as_ref().is_none_or(|s| jit_covers_expr(s, callees))
                && body.iter().all(|s| jit_covers_stmt(s, callees))
        }
        TStmt::Break(label) | TStmt::Continue(label) => label.is_none(),
        _ => false,
    }
}

fn jit_covers_func(tir: &TFunc, callees: &HashSet<String>) -> bool {
    jit_covers_func_detail(tir, callees).is_none()
}

fn jit_covers_func_detail(tir: &TFunc, callees: &HashSet<String>) -> Option<String> {
    if !matches!(tir.kind, TFuncKind::TopLevel) {
        return Some("not top-level".into());
    }
    if !tir.generics.is_empty() || tir.is_view || tir.is_unsafe || tir.is_reactive {
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
        if !jit_covers_stmt(s, callees) {
            return Some(format!("body stmt {i}"));
        }
    }
    None
}

fn jit_covers_program(program: &JitProgram) -> bool {
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let main_ok = program.funcs.iter().any(|f| {
        f.is_main && f.params.is_empty() && f.ret.is_none() && jit_covers_func(f, &names)
    });
    if !main_ok {
        return false;
    }
    if !program
        .funcs
        .iter()
        .all(|f| jit_covers_func(f, &names))
    {
        return false;
    }
    let spawn_sites = count_spawn_sites(program);
    if spawn_sites != program.spawn_lambdas.len() {
        return false;
    }
    program
        .spawn_lambdas
        .iter()
        .all(|lam| jit_covers_spawn_lambda(lam, &names))
}

fn count_spawn_sites(program: &JitProgram) -> usize {
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
            TStmt::Loop { body, .. }
            | TStmt::While { body, .. }
            | TStmt::Range { body, .. } => count_spawn_sites_stmts(body, n),
            TStmt::CountedLoop {
                init,
                step,
                body,
                ..
            } => {
                count_spawn_sites_stmts(std::slice::from_ref(init), n);
                count_spawn_sites_stmts(std::slice::from_ref(step), n);
                count_spawn_sites_stmts(body, n);
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
        _ => {}
    }
}

fn jit_covers_expr_list(exprs: &[TExpr], callees: &HashSet<String>) -> bool {
    exprs.iter().all(|e| jit_covers_expr(e, callees))
}

fn jit_covers_spawn_lambda(lam: &TJitSpawnLambda, callees: &HashSet<String>) -> bool {
    if lam.captures.len() > 4 {
        return false;
    }
    if !lam
        .captures
        .iter()
        .all(|c| jit_value_type(&c.ty) && jit_covers_capture_policy(c))
    {
        return false;
    }
    if !lam
        .params
        .iter()
        .all(|(_, ty)| jit_value_type(ty))
    {
        return false;
    }
    if !jit_value_type(&lam.ret) {
        return false;
    }
    match &lam.body {
        TJitSpawnBody::Expr(e) => jit_covers_expr(e, callees),
        TJitSpawnBody::Block { prefix, tail } => {
            prefix.iter().all(|s| jit_covers_stmt(s, callees))
                && tail.as_ref().is_none_or(|t| jit_covers_expr(t, callees))
        }
    }
}

fn jit_covers_capture_policy(c: &JitSpawnCapture) -> bool {
    if c.clone_at_spawn {
        matches!(&c.ty, Type::Apply { name, .. } if name == "Sender")
    } else {
        true
    }
}

fn jit_covers_handle_op(op: &THandleOp, recv: &TExpr, args: &[TExpr]) -> bool {
    match op {
        THandleOp::TaskJoin => args.is_empty() && jit_concurrency_type(&recv.ty),
        THandleOp::ChannelReceive | THandleOp::ChannelSender => {
            args.is_empty() && matches!(&recv.ty, Type::Apply { name, .. } if name == "Channel")
        }
        THandleOp::SenderSend => {
            args.len() == 1 && matches!(&recv.ty, Type::Apply { name, .. } if name == "Sender")
        }
        _ => false,
    }
}

fn init_clif_ty(init: &TExpr) -> Result<types::Type, String> {
    if let Some(t) = clif_ty(&init.ty) {
        return Ok(t);
    }
    // `tasks.channel()` carries `Unit` in TIR (annotation on the binding is load-bearing).
    if matches!(
        &init.kind,
        TExprKind::CoreCall {
            module,
            method,
            ..
        } if module == "core.tasks" && method == "channel"
    ) {
        return Ok(types::I64);
    }
    Err(format!("jit let type unsupported: {:?}", init.ty))
}

fn clif_ty(ty: &Type) -> Option<types::Type> {
    if matches!(ty, Type::Named(n) if n == "Unit") {
        return None;
    }
    if jit_concurrency_type(ty) {
        return Some(types::I64);
    }
    match ty {
        Type::Int | Type::String => Some(types::I64),
        Type::Float => Some(types::F64),
        Type::Bool => Some(types::I8),
        Type::Char => Some(types::I32),
        _ => None,
    }
}

fn func_signature(module: &JITModule, tir: &TFunc) -> Result<Signature, String> {
    let cc = module.target_config().default_call_conv;
    let mut sig = Signature::new(cc);
    for (_, ty, _) in &tir.params {
        sig.params.push(AbiParam::new(
            clif_ty(ty).ok_or_else(|| format!("jit param type unsupported: {ty:?}"))?,
        ));
    }
    if let Some(ret) = &tir.ret {
        if let Some(clif) = clif_ty(ret) {
            sig.returns.push(AbiParam::new(clif));
        }
    }
    Ok(sig)
}

fn jit_fn_name(name: &str) -> String {
    if name == "main" {
        "jet_jit_main".to_string()
    } else {
        format!("jet_jit_fn_{name}")
    }
}

struct LoopTargets {
    continue_block: Block,
    break_block: Block,
}

struct LowerCtx<'a, 'b> {
    b: &'a mut FunctionBuilder<'b>,
    module: &'a mut JITModule,
    host: &'a HostFns,
    runtime: &'a mut JitRuntime,
    vars: &'a mut HashMap<String, Variable>,
    func_ids: &'a HashMap<String, FuncId>,
    spawn_site: &'a mut usize,
    spawn_func_ids: &'a [FuncId],
    spawn_lambdas: &'a [TJitSpawnLambda],
    loop_stack: Vec<LoopTargets>,
    dead: bool,
    next_var: u32,
}

impl LowerCtx<'_, '_> {
    fn fresh_var(&mut self, ty: cranelift_codegen::ir::Type) -> Variable {
        let var = Variable::from_u32(self.next_var);
        self.next_var += 1;
        self.b.declare_var(var, ty);
        var
    }

    fn lower_stmts(&mut self, stmts: &[TStmt]) -> Result<(), String> {
        for stmt in stmts {
            self.lower_stmt(stmt)?;
        }
        Ok(())
    }

    fn lower_stmts_scoped(&mut self, stmts: &[TStmt]) -> Result<(), String> {
        let saved = self.dead;
        self.dead = false;
        self.lower_stmts(stmts)?;
        self.dead = saved;
        Ok(())
    }

    fn lower_stmt(&mut self, stmt: &TStmt) -> Result<(), String> {
        if self.dead {
            return Ok(());
        }
        match stmt {
            TStmt::Let { name, init, .. } => {
                let val = self.lower_expr(init)?;
                let ty = init_clif_ty(init)?;
                let var = self.fresh_var(ty);
                self.b.def_var(var, val);
                self.vars.insert(TIR::local_place(name), var);
            }
            TStmt::Assign {
                place,
                op,
                value,
                ..
            } => {
                let var = self
                    .vars
                    .get(place)
                    .copied()
                    .ok_or_else(|| format!("jit assign to unknown place `{place}`"))?;
                let val = if let Some(op) = op {
                    let current = self.b.use_var(var);
                    let rhs = self.lower_expr(value)?;
                    self.apply_binop_to_var(current, *op, rhs, &value.ty)?
                } else {
                    self.lower_expr(value)?
                };
                self.b.def_var(var, val);
            }
            TStmt::Return(Some(expr)) => {
                let val = self.lower_expr(expr)?;
                self.b.ins().return_(&[val]);
                self.dead = true;
            }
            TStmt::Return(None) => {
                self.b.ins().return_(&[]);
                self.dead = true;
            }
            TStmt::ExprStmt(expr) => {
                self.lower_expr(expr)?;
            }
            TStmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                let cond_val = match cond {
                    TIfCond::Plain(e) => self.lower_expr(e)?,
                    _ => return Err("jit if condition unsupported".to_string()),
                };
                let then_block = self.b.create_block();
                let else_block = self.b.create_block();
                let merge_block = self.b.create_block();
                self.b
                    .ins()
                    .brif(cond_val, then_block, &[], else_block, &[]);

                self.b.switch_to_block(then_block);
                self.b.seal_block(then_block);
                self.lower_stmts_scoped(then_body)?;
                if !self.dead {
                    self.b.ins().jump(merge_block, &[]);
                }

                self.b.switch_to_block(else_block);
                self.b.seal_block(else_block);
                if let Some(body) = else_body {
                    self.lower_stmts_scoped(body)?;
                }
                if !self.dead {
                    self.b.ins().jump(merge_block, &[]);
                }

                self.b.switch_to_block(merge_block);
                self.b.seal_block(merge_block);
                self.dead = false;
            }
            TStmt::Loop { body, .. } => {
                let header = self.b.create_block();
                let body_block = self.b.create_block();
                let exit = self.b.create_block();
                self.b.ins().jump(header, &[]);

                self.b.switch_to_block(header);
                self.b.seal_block(header);
                self.b.ins().jump(body_block, &[]);

                self.loop_stack.push(LoopTargets {
                    continue_block: header,
                    break_block: exit,
                });
                self.b.switch_to_block(body_block);
                self.b.seal_block(body_block);
                self.lower_stmts_scoped(body)?;
                self.loop_stack.pop();
                if !self.dead {
                    self.b.ins().jump(header, &[]);
                }

                self.b.switch_to_block(exit);
                self.b.seal_block(exit);
                self.dead = false;
            }
            TStmt::While { cond, body, .. } => {
                let header = self.b.create_block();
                let body_block = self.b.create_block();
                let exit = self.b.create_block();
                self.b.ins().jump(header, &[]);

                self.b.switch_to_block(header);
                let cond_val = self.lower_expr(cond)?;
                self.b
                    .ins()
                    .brif(cond_val, body_block, &[], exit, &[]);

                self.loop_stack.push(LoopTargets {
                    continue_block: header,
                    break_block: exit,
                });
                self.b.switch_to_block(body_block);
                self.b.seal_block(body_block);
                self.lower_stmts_scoped(body)?;
                self.loop_stack.pop();
                if !self.dead {
                    self.b.ins().jump(header, &[]);
                    self.b.seal_block(header);
                }

                self.b.switch_to_block(exit);
                self.b.seal_block(exit);
                self.dead = false;
            }
            TStmt::CountedLoop {
                init,
                cond,
                step,
                body,
                ..
            } => {
                self.lower_stmt(init)?;
                let header = self.b.create_block();
                let body_block = self.b.create_block();
                let exit = self.b.create_block();
                self.b.ins().jump(header, &[]);

                self.b.switch_to_block(header);
                let cond_val = self.lower_expr(cond)?;
                self.b
                    .ins()
                    .brif(cond_val, body_block, &[], exit, &[]);

                self.loop_stack.push(LoopTargets {
                    continue_block: header,
                    break_block: exit,
                });
                self.b.switch_to_block(body_block);
                self.b.seal_block(body_block);
                self.lower_stmts_scoped(body)?;
                if !self.dead {
                    self.lower_stmt(step)?;
                }
                self.loop_stack.pop();
                if !self.dead {
                    self.b.ins().jump(header, &[]);
                    self.b.seal_block(header);
                }

                self.b.switch_to_block(exit);
                self.b.seal_block(exit);
                self.dead = false;
            }
            TStmt::Range {
                var,
                start,
                end,
                step,
                body,
                ..
            } => {
                let start_val = self.lower_expr(start)?;
                let end_val = self.lower_expr(end)?;
                let loop_var = self.fresh_var(types::I64);
                self.b.def_var(loop_var, start_val);
                self.vars.insert(TIR::local_place(var), loop_var);

                let header = self.b.create_block();
                let body_block = self.b.create_block();
                let step_block = self.b.create_block();
                let exit = self.b.create_block();
                self.b.ins().jump(header, &[]);

                self.b.switch_to_block(header);
                let cur = self.b.use_var(loop_var);
                let past_end = self
                    .b
                    .ins()
                    .icmp(IntCC::SignedGreaterThan, cur, end_val);
                self.b
                    .ins()
                    .brif(past_end, exit, &[], body_block, &[]);

                self.loop_stack.push(LoopTargets {
                    continue_block: step_block,
                    break_block: exit,
                });
                self.b.switch_to_block(body_block);
                self.b.seal_block(body_block);
                self.lower_stmts_scoped(body)?;
                self.loop_stack.pop();
                if !self.dead {
                    self.b.ins().jump(step_block, &[]);
                }

                self.b.switch_to_block(step_block);
                self.b.seal_block(step_block);
                let cur = self.b.use_var(loop_var);
                let stride = if let Some(step_expr) = step {
                    self.lower_expr(step_expr)?
                } else {
                    self.b.ins().iconst(types::I64, 1)
                };
                let next = self.b.ins().iadd(cur, stride);
                self.b.def_var(loop_var, next);
                self.b.ins().jump(header, &[]);
                self.b.seal_block(header);

                self.b.switch_to_block(exit);
                self.b.seal_block(exit);
                self.dead = false;
            }
            TStmt::Break(_) => {
                let targets = self
                    .loop_stack
                    .last()
                    .ok_or_else(|| "jit break outside loop".to_string())?;
                self.b.ins().jump(targets.break_block, &[]);
                self.dead = true;
            }
            TStmt::Continue(_) => {
                let targets = self
                    .loop_stack
                    .last()
                    .ok_or_else(|| "jit continue outside loop".to_string())?;
                self.b.ins().jump(targets.continue_block, &[]);
                self.dead = true;
            }
            _ => return Err("jit statement unsupported".to_string()),
        }
        Ok(())
    }

    fn apply_binop_to_var(
        &mut self,
        current: Value,
        op: BinOp,
        rhs: Value,
        rhs_ty: &Type,
    ) -> Result<Value, String> {
        Ok(match (op, rhs_ty) {
            (BinOp::Add, Type::Int) => self.b.ins().iadd(current, rhs),
            (BinOp::Sub, Type::Int) => self.b.ins().isub(current, rhs),
            (BinOp::Mul, Type::Int) => self.b.ins().imul(current, rhs),
            (BinOp::Div, Type::Int) => self.b.ins().sdiv(current, rhs),
            (BinOp::Rem, Type::Int) => self.b.ins().srem(current, rhs),
            (BinOp::Add, Type::Float) => self.b.ins().fadd(current, rhs),
            (BinOp::Sub, Type::Float) => self.b.ins().fsub(current, rhs),
            (BinOp::Mul, Type::Float) => self.b.ins().fmul(current, rhs),
            (BinOp::Div, Type::Float) => self.b.ins().fdiv(current, rhs),
            _ => return Err("jit compound assign unsupported".to_string()),
        })
    }

    fn lower_call_arg(&mut self, arg: &TCallArg) -> Result<Value, String> {
        if arg.borrow
            || arg.mut_borrow
            || arg.clone
            || arg.arc_clone
            || arg.fn_coerce.is_some()
            || arg.widen_to_vec
        {
            return Err("jit call arg wrapper unsupported".to_string());
        }
        self.lower_expr(&arg.value)
    }

    fn lower_expr(&mut self, expr: &TExpr) -> Result<Value, String> {
        match &expr.kind {
            TExprKind::IntLit(v, _) => Ok(self.b.ins().iconst(types::I64, *v)),
            TExprKind::FloatLit(v) => Ok(self.b.ins().f64const(*v)),
            TExprKind::BoolLit(v) => Ok(self.b.ins().iconst(types::I8, if *v { 1 } else { 0 })),
            TExprKind::CharLit(v) => Ok(self.b.ins().iconst(types::I32, *v as i64)),
            TExprKind::StrLit(parts) => {
                let text = flatten_string(parts)
                    .ok_or_else(|| "jit string interpolation unsupported".to_string())?;
                let id = self.runtime.strings.len() as i64;
                self.runtime.strings.push(text);
                Ok(self.b.ins().iconst(types::I64, id))
            }
            TExprKind::Local(place) => {
                let var = self
                    .vars
                    .get(place)
                    .copied()
                    .ok_or_else(|| format!("jit unknown local `{place}`"))?;
                Ok(self.b.use_var(var))
            }
            TExprKind::Unary { op, operand } => {
                let inner = self.lower_expr(operand)?;
                Ok(match op {
                    UnOp::Neg => match &operand.ty {
                        Type::Int => self.b.ins().ineg(inner),
                        Type::Float => self.b.ins().fneg(inner),
                        _ => return Err("jit unary neg unsupported type".to_string()),
                    },
                    UnOp::Not => {
                        let zero = self.b.ins().iconst(types::I8, 0);
                        let one = self.b.ins().iconst(types::I8, 1);
                        let cmp = self.b.ins().icmp(IntCC::Equal, inner, zero);
                        self.b.ins().select(cmp, one, zero)
                    }
                })
            }
            TExprKind::Binary {
                op,
                overflow,
                line,
                lhs,
                rhs,
            } => {
                if matches!(op, BinOp::And | BinOp::Or) {
                    return self.lower_short_circuit(*op, lhs, rhs);
                }
                self.lower_binary(*op, *overflow, *line, lhs, rhs)
            }
            TExprKind::Call { name, args } => {
                let func_id = self
                    .func_ids
                    .get(name)
                    .copied()
                    .ok_or_else(|| format!("jit call to unknown function `{name}`"))?;
                let arg_vals: Result<Vec<_>, _> =
                    args.iter().map(|a| self.lower_call_arg(a)).collect();
                let arg_vals = arg_vals?;
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &arg_vals);
                if let Some(_) = clif_ty(&expr.ty) {
                    Ok(self.b.inst_results(call)[0])
                } else {
                    Ok(self.b.ins().iconst(types::I8, 0))
                }
            }
            TExprKind::Print(inner) => {
                self.emit_print(inner)?;
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TExprKind::IfExpr {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
            } => {
                let cond_val = self.lower_expr(cond)?;
                let ret_ty = clif_ty(&expr.ty).ok_or("jit if-expr result type unsupported")?;
                let then_block = self.b.create_block();
                let else_block = self.b.create_block();
                let merge_block = self.b.create_block();
                self.b.append_block_param(merge_block, ret_ty);
                self.b
                    .ins()
                    .brif(cond_val, then_block, &[], else_block, &[]);

                self.b.switch_to_block(then_block);
                self.b.seal_block(then_block);
                self.lower_stmts(then_body)?;
                let then_val = self.lower_expr(then_value)?;
                self.b.ins().jump(merge_block, &[then_val]);

                self.b.switch_to_block(else_block);
                self.b.seal_block(else_block);
                self.lower_stmts(else_body)?;
                let else_val = self.lower_expr(else_value)?;
                self.b.ins().jump(merge_block, &[else_val]);

                self.b.switch_to_block(merge_block);
                self.b.seal_block(merge_block);
                let phi = self.b.block_params(merge_block)[0];
                Ok(phi)
            }
            TExprKind::Clone(inner) => self.lower_clone(inner),
            TExprKind::CoreCall { module, method, args } => {
                if module == "core.tasks" && method == "channel" && args.is_empty() {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.conc.channel_new, self.b.func);
                    let call = self.b.ins().call(host_ref, &[]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                Err("jit core call unsupported".to_string())
            }
            TExprKind::CoreClosureCall { kind } => match kind {
                TCoreClosureKind::Spawn { .. } => self.lower_spawn(),
                _ => Err("jit core closure unsupported".to_string()),
            },
            TExprKind::HandleMethod { recv, op, args } => {
                self.lower_handle_method(recv, op, args, &expr.ty)
            }
            TExprKind::OrFallback {
                value,
                fallback,
                is_option,
            } => {
                if *is_option {
                    return Err("jit option fallback unsupported".to_string());
                }
                let status = self.lower_result_receive_status(value)?;
                let ok_block = self.b.create_block();
                let fail_block = self.b.create_block();
                let merge = self.b.create_block();
                self.b.append_block_param(merge, types::I64);
                let zero = self.b.ins().iconst(types::I64, 0);
                let gt = self.b.ins().icmp(IntCC::SignedGreaterThan, status, zero);
                self.b.ins().brif(gt, ok_block, &[], fail_block, &[]);
                self.b.switch_to_block(ok_block);
                self.b.seal_block(ok_block);
                let one = self.b.ins().iconst(types::I64, 1);
                let val = self.b.ins().isub(status, one);
                self.b.ins().jump(merge, &[val]);
                self.b.switch_to_block(fail_block);
                self.b.seal_block(fail_block);
                match fallback {
                    TOrFallback::Panic(_) => {
                        let line = self.b.ins().iconst(types::I32, 1);
                        let host_ref = self.module.declare_func_in_func(
                            self.host.conc.panic_channel_closed,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host_ref, &[line]);
                        let panic_val = self.b.inst_results(call)[0];
                        self.b.ins().jump(merge, &[panic_val]);
                    }
                    _ => return Err("jit or-fallback unsupported".to_string()),
                }
                self.b.switch_to_block(merge);
                self.b.seal_block(merge);
                Ok(self.b.block_params(merge)[0])
            }
            _ => Err("jit expression unsupported".to_string()),
        }
    }

    fn lower_clone(&mut self, inner: &TExpr) -> Result<Value, String> {
        if matches!(&inner.ty, Type::Apply { name, .. } if name == "Sender") {
            let val = self.lower_expr(inner)?;
            let host_ref = self
                .module
                .declare_func_in_func(self.host.conc.sender_clone, self.b.func);
            let call = self.b.ins().call(host_ref, &[val]);
            return Ok(self.b.inst_results(call)[0]);
        }
        Err("jit clone unsupported".to_string())
    }

    fn lower_spawn(&mut self) -> Result<Value, String> {
        let site = *self.spawn_site;
        *self.spawn_site += 1;
        let lam = self
            .spawn_lambdas
            .get(site)
            .ok_or_else(|| format!("jit spawn site {site} missing lambda"))?;
        let spawn_fn = self
            .spawn_func_ids
            .get(site)
            .copied()
            .ok_or_else(|| format!("jit spawn site {site} missing"))?;
        let mut cap_vals = Vec::new();
        for cap in &lam.captures {
            let mut val = self.lower_expr(&TExpr {
                ty: cap.ty.clone(),
                kind: TExprKind::Local(TIR::local_place(&cap.name)),
            })?;
            if cap.clone_at_spawn {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.sender_clone, self.b.func);
                let call = self.b.ins().call(host_ref, &[val]);
                val = self.b.inst_results(call)[0];
            }
            cap_vals.push(val);
        }
        let spawn_ref = self.module.declare_func_in_func(spawn_fn, self.b.func);
        let spawn_ptr = self.b.ins().func_addr(types::I64, spawn_ref);
        let (host_id, call_args) = match cap_vals.len() {
            0 => (self.host.conc.spawn0, vec![spawn_ptr]),
            1 => (self.host.conc.spawn1, vec![spawn_ptr, cap_vals[0]]),
            2 => (
                self.host.conc.spawn2,
                vec![spawn_ptr, cap_vals[0], cap_vals[1]],
            ),
            3 => (
                self.host.conc.spawn3,
                vec![spawn_ptr, cap_vals[0], cap_vals[1], cap_vals[2]],
            ),
            4 => (
                self.host.conc.spawn4,
                vec![
                    spawn_ptr,
                    cap_vals[0],
                    cap_vals[1],
                    cap_vals[2],
                    cap_vals[3],
                ],
            ),
            n => return Err(format!("jit spawn capture count {n} > 4")),
        };
        let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
        let call = self.b.ins().call(host_ref, &call_args);
        Ok(self.b.inst_results(call)[0])
    }

    fn lower_handle_method(
        &mut self,
        recv: &TExpr,
        op: &THandleOp,
        args: &[TExpr],
        ret_ty: &Type,
    ) -> Result<Value, String> {
        let recv_val = self.lower_expr(recv)?;
        match op {
            THandleOp::TaskJoin => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_join, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                if clif_ty(ret_ty).is_some() {
                    Ok(self.b.inst_results(call)[0])
                } else {
                    let _ = self.b.inst_results(call);
                    Ok(self.b.ins().iconst(types::I8, 0))
                }
            }
            THandleOp::ChannelSender => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.channel_sender, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ChannelReceive => {
                let line = self.b.ins().iconst(types::I32, 1);
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.channel_receive, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, line]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::SenderSend => {
                let val = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.sender_send, self.b.func);
                self.b.ins().call(host_ref, &[recv_val, val]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            _ => Err("jit handle method unsupported".to_string()),
        }
    }

    fn lower_result_receive_status(&mut self, value: &TExpr) -> Result<Value, String> {
        if let TExprKind::HandleMethod {
            recv,
            op: THandleOp::ChannelReceive,
            args,
        } = &value.kind
        {
            if !args.is_empty() {
                return Err("jit receive status arity".to_string());
            }
            let ch = self.lower_expr(recv)?;
            let host_ref = self.module.declare_func_in_func(
                self.host.conc.channel_receive_status,
                self.b.func,
            );
            let call = self.b.ins().call(host_ref, &[ch]);
            return Ok(self.b.inst_results(call)[0]);
        }
        Err("jit result status unsupported".to_string())
    }

    fn lower_short_circuit(
        &mut self,
        op: BinOp,
        lhs: &TExpr,
        rhs: &TExpr,
    ) -> Result<Value, String> {
        let lhs_val = self.lower_expr(lhs)?;
        let rhs_block = self.b.create_block();
        let merge_block = self.b.create_block();
        self.b.append_block_param(merge_block, types::I8);

        let short_val = if matches!(op, BinOp::And) {
            self.b.ins().iconst(types::I8, 0)
        } else {
            self.b.ins().iconst(types::I8, 1)
        };

        let zero = self.b.ins().iconst(types::I8, 0);
        let take_short = if matches!(op, BinOp::And) {
            self.b.ins().icmp(IntCC::Equal, lhs_val, zero)
        } else {
            self.b.ins().icmp(IntCC::NotEqual, lhs_val, zero)
        };
        self.b
            .ins()
            .brif(take_short, merge_block, &[short_val], rhs_block, &[]);

        self.b.switch_to_block(rhs_block);
        self.b.seal_block(rhs_block);
        let rhs_val = self.lower_expr(rhs)?;
        self.b.ins().jump(merge_block, &[rhs_val]);

        self.b.switch_to_block(merge_block);
        self.b.seal_block(merge_block);
        Ok(self.b.block_params(merge_block)[0])
    }

    fn lower_binary(
        &mut self,
        op: BinOp,
        overflow: bool,
        line: u32,
        lhs: &TExpr,
        rhs: &TExpr,
    ) -> Result<Value, String> {
        let l = self.lower_expr(lhs)?;
        let r = self.lower_expr(rhs)?;
        if overflow {
            let host_id = match op {
                BinOp::Add => self.host.add_i64,
                BinOp::Sub => self.host.sub_i64,
                BinOp::Mul => self.host.mul_i64,
                BinOp::Div => self.host.div_i64,
                _ => return Err("jit overflow op unsupported".to_string()),
            };
            let line_const = self.b.ins().iconst(types::I32, line as i64);
            let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
            let call = self.b.ins().call(host_ref, &[l, r, line_const]);
            return Ok(self.b.inst_results(call)[0]);
        }
        Ok(match (&lhs.ty, op) {
            (Type::Int, BinOp::Add) => self.b.ins().iadd(l, r),
            (Type::Int, BinOp::Sub) => self.b.ins().isub(l, r),
            (Type::Int, BinOp::Mul) => self.b.ins().imul(l, r),
            (Type::Int, BinOp::Div) => self.b.ins().sdiv(l, r),
            (Type::Int, BinOp::Rem) => self.b.ins().srem(l, r),
            (Type::Float, BinOp::Add) => self.b.ins().fadd(l, r),
            (Type::Float, BinOp::Sub) => self.b.ins().fsub(l, r),
            (Type::Float, BinOp::Mul) => self.b.ins().fmul(l, r),
            (Type::Float, BinOp::Div) => self.b.ins().fdiv(l, r),
            (Type::Int, BinOp::Eq) => self.bool_from_icmp(IntCC::Equal, l, r),
            (Type::Int, BinOp::Ne) => self.bool_from_icmp(IntCC::NotEqual, l, r),
            (Type::Int, BinOp::Lt) => self.bool_from_icmp(IntCC::SignedLessThan, l, r),
            (Type::Int, BinOp::Gt) => self.bool_from_icmp(IntCC::SignedGreaterThan, l, r),
            (Type::Int, BinOp::Le) => self.bool_from_icmp(IntCC::SignedLessThanOrEqual, l, r),
            (Type::Int, BinOp::Ge) => self.bool_from_icmp(IntCC::SignedGreaterThanOrEqual, l, r),
            (Type::Float, BinOp::Eq) => self.bool_from_fcmp(FloatCC::Equal, l, r),
            (Type::Float, BinOp::Ne) => self.bool_from_fcmp(FloatCC::NotEqual, l, r),
            (Type::Float, BinOp::Lt) => self.bool_from_fcmp(FloatCC::LessThan, l, r),
            (Type::Float, BinOp::Gt) => self.bool_from_fcmp(FloatCC::GreaterThan, l, r),
            (Type::Float, BinOp::Le) => self.bool_from_fcmp(FloatCC::LessThanOrEqual, l, r),
            (Type::Float, BinOp::Ge) => self.bool_from_fcmp(FloatCC::GreaterThanOrEqual, l, r),
            (Type::Bool, BinOp::Eq) => self.bool_from_icmp(IntCC::Equal, l, r),
            (Type::Bool, BinOp::Ne) => self.bool_from_icmp(IntCC::NotEqual, l, r),
            _ => return Err("jit binary op unsupported".to_string()),
        })
    }

    fn bool_from_icmp(&mut self, cc: IntCC, l: Value, r: Value) -> Value {
        let cmp = self.b.ins().icmp(cc, l, r);
        let one = self.b.ins().iconst(types::I8, 1);
        let zero = self.b.ins().iconst(types::I8, 0);
        self.b.ins().select(cmp, one, zero)
    }

    fn bool_from_fcmp(&mut self, cc: FloatCC, l: Value, r: Value) -> Value {
        let cmp = self.b.ins().fcmp(cc, l, r);
        let one = self.b.ins().iconst(types::I8, 1);
        let zero = self.b.ins().iconst(types::I8, 0);
        self.b.ins().select(cmp, one, zero)
    }

    fn emit_print(&mut self, inner: &TExpr) -> Result<(), String> {
        let (host_id, arg) = match &inner.kind {
            TExprKind::IntLit(v, _) => (self.host.print_i64, self.b.ins().iconst(types::I64, *v)),
            TExprKind::FloatLit(v) => (self.host.print_f64, self.b.ins().f64const(*v)),
            TExprKind::BoolLit(v) => (
                self.host.print_bool,
                self.b.ins().iconst(types::I8, if *v { 1 } else { 0 }),
            ),
            TExprKind::CharLit(v) => (self.host.print_char, self.b.ins().iconst(types::I32, *v as i64)),
            TExprKind::StrLit(parts) => {
                let text = flatten_string(parts)
                    .ok_or_else(|| "jit string interpolation unsupported".to_string())?;
                let id = self.runtime.strings.len() as i64;
                self.runtime.strings.push(text);
                (self.host.print_str, self.b.ins().iconst(types::I64, id))
            }
            _ => {
                let val = self.lower_expr(inner)?;
                let host_id = match &inner.ty {
                    Type::Int => self.host.print_i64,
                    Type::String => self.host.print_str,
                    Type::Float => self.host.print_f64,
                    Type::Bool => self.host.print_bool,
                    Type::Char => self.host.print_char,
                    _ => return Err("jit print type unsupported".to_string()),
                };
                let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                self.b.ins().call(host_ref, &[val]);
                return Ok(());
            }
        };
        let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
        self.b.ins().call(host_ref, &[arg]);
        Ok(())
    }
}

fn spawn_lambda_signature(module: &JITModule, lam: &TJitSpawnLambda) -> Signature {
    let cc = module.target_config().default_call_conv;
    let mut sig = Signature::new(cc);
    for _ in &lam.captures {
        sig.params.push(AbiParam::new(types::I64));
    }
    for (_, ty) in &lam.params {
        sig.params.push(AbiParam::new(
            clif_ty(ty).unwrap_or(types::I64),
        ));
    }
    if clif_ty(&lam.ret).is_some() {
        sig.returns.push(AbiParam::new(types::I64));
    }
    sig
}

fn lower_spawn_function(
    module: &mut JITModule,
    host: &HostFns,
    lam: &TJitSpawnLambda,
    func_id: FuncId,
    func_ids: &HashMap<String, FuncId>,
    spawn_func_ids: &[FuncId],
    spawn_lambdas: &[TJitSpawnLambda],
    runtime: &mut JitRuntime,
) -> Result<(), String> {
    let mut ctx = module.make_context();
    ctx.func.signature = spawn_lambda_signature(module, lam);
    let mut fbcx = FunctionBuilderContext::new();
    let mut vars = HashMap::new();
    let mut spawn_site = 0usize;
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbcx);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);
        let param_vals = b.block_params(entry).to_vec();
        let mut idx = 0usize;
        let mut lctx = LowerCtx {
            b: &mut b,
            module,
            host,
            runtime,
            vars: &mut vars,
            func_ids,
            spawn_site: &mut spawn_site,
            spawn_func_ids,
            spawn_lambdas,
            loop_stack: Vec::new(),
            dead: false,
            next_var: 0,
        };
        for cap in &lam.captures {
            let var = lctx.fresh_var(types::I64);
            lctx.b.def_var(var, param_vals[idx]);
            lctx.vars.insert(TIR::local_place(&cap.name), var);
            idx += 1;
        }
        for (name, ty) in &lam.params {
            let clif = clif_ty(ty).unwrap_or(types::I64);
            let var = lctx.fresh_var(clif);
            lctx.b.def_var(var, param_vals[idx]);
            lctx.vars.insert(TIR::local_place(name), var);
            idx += 1;
        }
        match &lam.body {
            TJitSpawnBody::Expr(e) => {
                let val = lctx.lower_expr(e)?;
                if clif_ty(&lam.ret).is_some() {
                    b.ins().return_(&[val]);
                } else {
                    let _ = val;
                    b.ins().return_(&[]);
                }
            }
            TJitSpawnBody::Block { prefix, tail } => {
                lctx.lower_stmts(prefix)?;
                if let Some(t) = tail {
                    let val = lctx.lower_expr(t)?;
                    if clif_ty(&lam.ret).is_some() {
                        b.ins().return_(&[val]);
                    } else {
                        let _ = val;
                        b.ins().return_(&[]);
                    }
                } else if clif_ty(&lam.ret).is_some() {
                    let zero = b.ins().iconst(types::I64, 0);
                    b.ins().return_(&[zero]);
                } else {
                    b.ins().return_(&[]);
                }
            }
        }
        b.finalize();
    }
    module
        .define_function(func_id, &mut ctx)
        .map_err(|e| e.to_string())?;
    module.clear_context(&mut ctx);
    Ok(())
}

fn lower_function(
    module: &mut JITModule,
    host: &HostFns,
    tir: &TFunc,
    func_id: FuncId,
    func_ids: &HashMap<String, FuncId>,
    spawn_func_ids: &[FuncId],
    spawn_lambdas: &[TJitSpawnLambda],
    spawn_site: &mut usize,
    runtime: &mut JitRuntime,
) -> Result<(), String> {
    let mut ctx = module.make_context();
    ctx.func.signature = func_signature(module, tir)?;
    let mut fbcx = FunctionBuilderContext::new();
    let mut vars = HashMap::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbcx);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);

        let param_vals = b.block_params(entry).to_vec();
        let mut lctx = LowerCtx {
            b: &mut b,
            module,
            host,
            runtime,
            vars: &mut vars,
            func_ids,
            spawn_site,
            spawn_func_ids,
            spawn_lambdas,
            loop_stack: Vec::new(),
            dead: false,
            next_var: 0,
        };
        for (i, (name, ty, _)) in tir.params.iter().enumerate() {
            let clif = clif_ty(ty).ok_or("jit param clif type")?;
            let var = lctx.fresh_var(clif);
            lctx.b.def_var(var, param_vals[i]);
            lctx.vars.insert(name.clone(), var);
        }

        lctx.lower_stmts(&tir.body)?;
        if !tir.body.iter().any(|s| matches!(s, TStmt::Return(_))) {
            if let Some(ret) = &tir.ret {
                if clif_ty(ret).is_some() {
                    return Err("jit function missing return".to_string());
                }
            }
            b.ins().return_(&[]);
        }
        b.finalize();
    }
    module
        .define_function(func_id, &mut ctx)
        .map_err(|e| e.to_string())?;
    module.clear_context(&mut ctx);
    Ok(())
}

fn compile_program(
    module: &mut JITModule,
    host: &HostFns,
    program: &JitProgram,
    runtime: &mut JitRuntime,
    existing_main: Option<FuncId>,
) -> Result<FuncId, String> {
    runtime.source_file = program.source_file.clone();

    let spawn_lambdas = &program.spawn_lambdas;
    let mut spawn_func_ids: Vec<FuncId> = Vec::new();
    for (i, lam) in spawn_lambdas.iter().enumerate() {
        let name = format!("jet_jit_spawn_body_{i}");
        let sig = spawn_lambda_signature(module, lam);
        let id = module
            .declare_function(&name, Linkage::Export, &sig)
            .map_err(|e| e.to_string())?;
        spawn_func_ids.push(id);
    }

    let mut func_ids: HashMap<String, FuncId> = HashMap::new();
    for f in &program.funcs {
        let sig = func_signature(module, f)?;
        let id = if f.is_main {
            match existing_main {
                Some(id) => id,
                None => module
                    .declare_function("jet_jit_main", Linkage::Export, &sig)
                    .map_err(|e| e.to_string())?,
            }
        } else {
            module
                .declare_function(&jit_fn_name(&f.name), Linkage::Export, &sig)
                .map_err(|e| e.to_string())?
        };
        func_ids.insert(f.name.clone(), id);
    }

    for (i, lam) in spawn_lambdas.iter().enumerate() {
        lower_spawn_function(
            module,
            host,
            lam,
            spawn_func_ids[i],
            &func_ids,
            &spawn_func_ids,
            spawn_lambdas,
            runtime,
        )?;
    }

    let mut spawn_site = 0usize;
    for f in &program.funcs {
        let id = func_ids[&f.name];
        lower_function(
            module,
            host,
            f,
            id,
            &func_ids,
            &spawn_func_ids,
            spawn_lambdas,
            &mut spawn_site,
            runtime,
        )?;
    }

    module.finalize_definitions().map_err(|e| e.to_string())?;
    Ok(func_ids
        .get("main")
        .copied()
        .ok_or_else(|| "jit program missing main".to_string())?)
}

fn fresh_runtime() -> JitRuntime {
    JitRuntime {
        source_file: String::new(),
        stdout: String::new(),
        stderr: String::new(),
        strings: Vec::new(),
        invocations: 0,
        channels: Vec::new(),
        senders: Vec::new(),
        tasks: Vec::new(),
        next_spawn_id: 0,
    }
}

fn resident_teardown() {
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = None);
    concurrency::set_active_runtime(None);
}

fn ensure_resident_module(program: &JitProgram) -> Result<(), String> {
    let need_create = RESIDENT_MODULE.with(|slot| slot.borrow().is_none());
    if need_create {
        let (mut module, host) = new_jit_module()?;
        let mut runtime = RESIDENT_RUNTIME
            .with(|slot| slot.borrow_mut().take())
            .unwrap_or_else(fresh_runtime);
        let main_id = compile_program(&mut module, &host, program, &mut runtime, None)?;
        RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
        RESIDENT_MODULE.with(|slot| {
            *slot.borrow_mut() = Some(ResidentModule {
                module,
                host,
                main_id,
            });
        });
        return Ok(());
    }

    RESIDENT_MODULE.with(|mod_slot| {
        let mut mod_guard = mod_slot.borrow_mut();
        let resident = mod_guard.as_mut().ok_or("resident module missing")?;
        RESIDENT_RUNTIME.with(|rt_slot| {
            let mut rt_guard = rt_slot.borrow_mut();
            let runtime = rt_guard.as_mut().ok_or("resident runtime missing")?;
            resident.main_id = compile_program(
                &mut resident.module,
                &resident.host,
                program,
                runtime,
                Some(resident.main_id),
            )?;
            Ok(())
        })
    })
}

fn resident_invoke() -> Result<RunOutcome, String> {
    let code = RESIDENT_MODULE
        .with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|r| r.module.get_finalized_function(r.main_id))
        })
        .ok_or_else(|| "resident module missing".to_string())?;

    RESIDENT_RUNTIME.with(|slot| {
        let mut rt_guard = slot.borrow_mut();
        let runtime = rt_guard.as_mut().ok_or("resident runtime missing")?;
        runtime.invocations += 1;
        runtime.stdout.clear();
        runtime.stderr.clear();
        let ptr: *mut JitRuntime = runtime;
        concurrency::set_active_runtime(Some(ptr));
        let entry: extern "C" fn() = unsafe { std::mem::transmute(code) };
        entry();
        jet_codegen::scheduler::jet_scheduler_drain();
        concurrency::set_active_runtime(None);
        Ok(RunOutcome::Ran {
            stdout: runtime.stdout.clone(),
            stderr: runtime.stderr.clone(),
        })
    })
}

fn resident_run_fresh(program: &JitProgram) -> Result<RunOutcome, String> {
    resident_teardown();
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(fresh_runtime()));
    ensure_resident_module(program)?;
    resident_invoke()
}

fn resident_hot_swap(program: &JitProgram) -> Result<RunOutcome, String> {
    // Rebuild the module (Cranelift rejects redefining `jet_jit_main`) but keep
    // the live runtime heap — the M2 contract.
    let mut runtime = RESIDENT_RUNTIME.with(|slot| {
        slot.borrow_mut().take().unwrap_or_else(fresh_runtime)
    });
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    let (mut module, host) = new_jit_module()?;
    let main_id = compile_program(&mut module, &host, program, &mut runtime, None)?;
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
    RESIDENT_MODULE.with(|slot| {
        *slot.borrow_mut() = Some(ResidentModule {
            module,
            host,
            main_id,
        });
    });
    resident_invoke()
}

fn try_resident(bundle: &ProgramBundle) -> Option<Result<RunOutcome, String>> {
    let program = TIR::lower_jit_program(bundle)?;
    if !jit_covers_program(&program) {
        return None;
    }
    Some(resident_run_fresh(&program))
}

fn try_resident_hot_swap(bundle: &ProgramBundle) -> Option<Result<RunOutcome, String>> {
    let program = TIR::lower_jit_program(bundle)?;
    if !jit_covers_program(&program) {
        return None;
    }
    Some(resident_hot_swap(&program))
}

fn try_resident_restart(bundle: &ProgramBundle) -> Option<Result<RunOutcome, String>> {
    let program = TIR::lower_jit_program(bundle)?;
    if !jit_covers_program(&program) {
        return None;
    }
    Some(resident_run_fresh(&program))
}

/// Test hook: try JIT-compile a checked bundle; surfaces lowering errors.
#[doc(hidden)]
pub fn try_compile_bundle(bundle: &ProgramBundle) -> Result<(), String> {
    let program = TIR::lower_jit_program(bundle).ok_or_else(|| {
        format!(
            "lower_jit_program returned None ({})",
            TIR::lower_jit_program_fail_reason(bundle)
        )
    })?;
    if !jit_covers_program(&program) {
        return Err(jit_covers_bundle_detail(bundle));
    }
    resident_teardown();
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(fresh_runtime()));
    ensure_resident_module(&program)
}

/// Test hook: dump lowered main stmt tags.
#[doc(hidden)]
pub fn jit_dump_main_stmts(bundle: &ProgramBundle) -> Vec<String> {
    let Some(program) = TIR::lower_jit_program(bundle) else {
        return vec!["<no program>".into()];
    };
    let Some(m) = program.funcs.iter().find(|f| f.is_main) else {
        return vec!["<no main>".into()];
    };
    m.body
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{i}:{}", jit_stmt_tag(s)))
        .collect()
}

/// Test hook: label a lowered stmt for diagnostics.
#[doc(hidden)]
pub fn jit_stmt_tag(stmt: &TStmt) -> &'static str {
    match stmt {
        TStmt::Let { .. } => "Let",
        TStmt::Assign { .. } => "Assign",
        TStmt::Return(_) => "Return",
        TStmt::ExprStmt(_) => "ExprStmt",
        TStmt::If { .. } => "If",
        TStmt::Loop { .. } => "Loop",
        TStmt::While { .. } => "While",
        TStmt::CountedLoop { .. } => "CountedLoop",
        TStmt::Range { .. } => "Range",
        TStmt::ForIn { .. } => "ForIn",
        TStmt::Break(_) => "Break",
        TStmt::Continue(_) => "Continue",
        _ => "Other",
    }
}

/// Test hook: spawn site vs lambda counts for a bundle.
#[doc(hidden)]
pub fn jit_spawn_stats(bundle: &ProgramBundle) -> (usize, usize) {
    let Some(program) = TIR::lower_jit_program(bundle) else {
        return (0, 0);
    };
    (count_spawn_sites(&program), program.spawn_lambdas.len())
}

/// Test hook: whether the bundle's entry module is inside `jit_covers`.
#[doc(hidden)]
pub fn jit_covers_bundle(bundle: &ProgramBundle) -> bool {
    jit_covers_bundle_detail(bundle).is_empty()
}

/// Test hook: empty string when covered; otherwise a short failure reason.
#[doc(hidden)]
pub fn jit_covers_bundle_detail(bundle: &ProgramBundle) -> String {
    let Some(program) = TIR::lower_jit_program(bundle) else {
        return format!(
            "lower_jit_program returned None ({})",
            TIR::lower_jit_program_fail_reason(bundle)
        );
    };
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let main_ok = program.funcs.iter().any(|f| {
        f.is_main && f.params.is_empty() && f.ret.is_none() && jit_covers_func(f, &names)
    });
    if !main_ok {
        for f in &program.funcs {
            if f.is_main {
                if let Some(d) = jit_covers_func_detail(f, &names) {
                    return format!("main not jit-covered: {d}");
                }
            }
        }
        return "main not jit-covered".to_string();
    }
    for f in &program.funcs {
        if !jit_covers_func(f, &names) {
            return format!("func `{}` not jit-covered", f.name);
        }
    }
    let spawn_sites = count_spawn_sites(&program);
    if spawn_sites != program.spawn_lambdas.len() {
        return format!(
            "spawn site count {spawn_sites} != lambda count {}",
            program.spawn_lambdas.len()
        );
    }
    for (i, lam) in program.spawn_lambdas.iter().enumerate() {
        if !jit_covers_spawn_lambda(lam, &names) {
            return format!("spawn lambda {i} not jit-covered");
        }
    }
    String::new()
}

/// Test hook: how many times resident `main` ran without a clean restart.
#[doc(hidden)]
pub fn resident_invocations_for_test() -> u64 {
    RESIDENT_RUNTIME.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|r| r.invocations)
            .unwrap_or(0)
    })
}

/// c139 tier-1 JIT backend over the `JitBackend` seam.
///
/// `F` is the tier-0 fallback (always `InterpreterBackend` in practice).
/// M0: every method delegates to `fallback`.
/// M1: `run` JIT-compiles functions inside `jit_covers()` and delegates only
///     the uncovered remainder to `fallback`.
/// M2: `hot_swap` re-links changed code in the resident process; `restart`
///     tears down live state.
pub struct CraneliftBackend<F: JitBackend> {
    fallback: F,
}

impl<F: JitBackend> CraneliftBackend<F> {
    /// Construct a CraneliftBackend wrapping `fallback` for tier-0 coverage.
    pub fn new(fallback: F) -> Self {
        CraneliftBackend { fallback }
    }
}

impl<F: JitBackend> JitBackend for CraneliftBackend<F> {
    fn run(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        if let Some(result) = try_resident(bundle) {
            match result {
                Ok(out) => return out,
                Err(_) => return self.fallback.run(bundle, try_anyway),
            }
        }
        self.fallback.run(bundle, try_anyway)
    }

    fn hot_swap(
        &mut self,
        module_name: &str,
        bundle: &ProgramBundle,
        try_anyway: bool,
    ) -> Result<RunOutcome, Vec<Diagnostic>> {
        if let Some(result) = try_resident_hot_swap(bundle) {
            return match result {
                Ok(out) => Ok(out),
                Err(_) => self.fallback.hot_swap(module_name, bundle, try_anyway),
            };
        }
        self.fallback.hot_swap(module_name, bundle, try_anyway)
    }

    fn restart(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        if let Some(result) = try_resident_restart(bundle) {
            match result {
                Ok(out) => return out,
                Err(_) => return self.fallback.restart(bundle, try_anyway),
            }
        }
        self.fallback.restart(bundle, try_anyway)
    }
}
