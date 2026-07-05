//! c139 (D-JITDEP1 / D-JIT2=A) — Cranelift JIT tier-1 backend.
//!
//! Architecture: CraneliftBackend<F: JitBackend> where F is the tier-0
//! fallback. M0 delegates everything to F; M1 adds jit_covers() and
//! lower_tir_clif() to actually compile + run the covered subset natively.
//! M2 keeps a resident JIT module + live runtime heap across hot_swap.
//! M3 widens jit_covers: arithmetic, bindings, if/else, calls, loops,
//! compound assign, &&/|| short-circuit.
//! M4: tasks/channels/spawn via scheduler host shims (D-ASYNCRT1=A).

#![allow(non_snake_case)]

mod Collections;
mod Concurrency;

use jet_codegen::scheduler::{
    JetSchedulerChannel, JetSchedulerJoin, JetSchedulerSender, JetTaskControl,
};
// I6: Cranelift crates live here, not in the compiler `jet` crate (`Source/`).
// The root package depends on jet-jit; jet-jit depends on cranelift-*.
// D-JITDEP1 approved this as a scoped runtime-side exception.

use jet_foundation::{
    Diagnostics::Diagnostic,
    JitBackend::{JitBackend, RunOutcome},
    AST::{BinOp, ProgramBundle, Type, UnOp},
};

use cranelift_codegen::ir::{
    condcodes::{FloatCC, IntCC},
    types, AbiParam, Block, InstBuilder, Signature, TrapCode, Value,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use jet_codegen::Codegen::TIR::{
    self, JitProgram, JitSpawnCapture, TBuiltinOp, TCallArg, TCoreClosureKind, TEnumPayload, TExpr,
    TExprKind, TFunc, TFuncKind, THandleOp, TIfCond, TJitSpawnBody, TJitSpawnLambda, TOrFallback,
    TStmt, TStrPart,
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
    lists: Vec<Vec<i64>>,
    structs_f64: Vec<Vec<f64>>,
    invocations: u64,
    channels: Vec<JetSchedulerChannel<i64>>,
    senders: Vec<JetSchedulerSender<i64>>,
    tasks: Vec<Option<JetSchedulerJoin<i64>>>,
    task_controls: Vec<std::sync::Arc<JetTaskControl>>,
    /// Set by a host shim when the user program hits a runtime panic (overflow,
    /// list index/slice OOB, a couple of concurrency panics). Non-`None` makes
    /// JIT-generated code branch to its epilogue on the next `emit_trap_check`,
    /// so the trap unwinds through pure Cranelift control flow (never a Rust
    /// panic through a JIT frame — I1). `resident_invoke` turns it into an
    /// `E0953` diagnostic, exactly as the tier-0 interpreter reports the same
    /// panic. Keeps the FIRST message; later traps on the unwind path are noise.
    trapped: Option<String>,
}

impl JitRuntime {
    /// Record a runtime panic. Keeps the first message (the unwind branch may
    /// re-enter trap sites with dummy values before the epilogue is reached).
    fn set_trap(&mut self, msg: &str) {
        if self.trapped.is_none() {
            self.trapped = Some(msg.to_string());
        }
    }
}

struct ResidentModule {
    module: JITModule,
    host: HostFns,
    main_id: FuncId,
}

fn with_runtime_mut<F: FnOnce(&mut JitRuntime)>(f: F) {
    Concurrency::with_runtime_mut(f);
}

fn render_float(v: f64) -> String {
    // Match `JetShow for f64` (`format!("{:?}", self)` in Core.rs).
    format!("{v:?}")
}

/// Record an arithmetic overflow/div-by-zero trap. Returns normally (the
/// caller yields a dummy `0`); JIT code branches to its epilogue at the next
/// `emit_trap_check`. Message text is unchanged from the old exit-70 path.
fn jet_trap_overflow(op: &str) {
    let msg = match op {
        "add" => "this addition overflows the value's type (the result is outside its range)",
        "sub" => "this subtraction overflows the value's type (the result is outside its range)",
        "mul" => "this multiplication overflows the value's type (the result is outside its range)",
        "div" => "this division can't be done (dividing by zero, or overflow)",
        _ => "this operation overflows the value's type (the result is outside its range)",
    };
    with_runtime_mut(|rt| rt.set_trap(msg));
}

/// Reads the resident runtime's trapped flag from JIT code. `1` = a trap is
/// pending (branch to epilogue); `0` = keep going.
extern "C" fn jet_jit_is_trapped() -> i64 {
    Concurrency::with_runtime_mut(|rt| i64::from(rt.trapped.is_some()))
}

extern "C" fn jet_jit_add_i64(a: i64, b: i64, _line: u32) -> i64 {
    match a.checked_add(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("add");
            0
        }
    }
}

extern "C" fn jet_jit_sub_i64(a: i64, b: i64, _line: u32) -> i64 {
    match a.checked_sub(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("sub");
            0
        }
    }
}

extern "C" fn jet_jit_mul_i64(a: i64, b: i64, _line: u32) -> i64 {
    match a.checked_mul(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("mul");
            0
        }
    }
}

extern "C" fn jet_jit_div_i64(a: i64, b: i64, _line: u32) -> i64 {
    match a.checked_div(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("div");
            0
        }
    }
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

extern "C" fn jet_jit_str_begin() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let id = rt.strings.len() as i64;
        rt.strings.push(String::new());
        id
    })
}

extern "C" fn jet_jit_str_push_lit(buf_id: i64, lit_id: i64) {
    with_runtime_mut(|rt| {
        let Some(lit) = rt.strings.get(lit_id as usize).cloned() else {
            return;
        };
        if let Some(buf) = rt.strings.get_mut(buf_id as usize) {
            buf.push_str(&lit);
        }
    });
}

extern "C" fn jet_jit_str_push_i64(buf_id: i64, v: i64) {
    with_runtime_mut(|rt| {
        if let Some(buf) = rt.strings.get_mut(buf_id as usize) {
            buf.push_str(&v.to_string());
        }
    });
}

extern "C" fn jet_jit_str_push_f64(buf_id: i64, v: f64) {
    with_runtime_mut(|rt| {
        if let Some(buf) = rt.strings.get_mut(buf_id as usize) {
            buf.push_str(&render_float(v));
        }
    });
}

extern "C" fn jet_jit_str_push_bool(buf_id: i64, v: i8) {
    with_runtime_mut(|rt| {
        if let Some(buf) = rt.strings.get_mut(buf_id as usize) {
            buf.push_str(if v == 0 { "false" } else { "true" });
        }
    });
}

extern "C" fn jet_jit_str_push_char(buf_id: i64, v: i32) {
    with_runtime_mut(|rt| {
        if let Some(buf) = rt.strings.get_mut(buf_id as usize) {
            match char::from_u32(v as u32) {
                Some(ch) => buf.push(ch),
                None => buf.push('?'),
            }
        }
    });
}

extern "C" fn jet_jit_str_push_str(buf_id: i64, str_id: i64) {
    with_runtime_mut(|rt| {
        let Some(s) = rt.strings.get(str_id as usize).cloned() else {
            return;
        };
        if let Some(buf) = rt.strings.get_mut(buf_id as usize) {
            buf.push_str(&s);
        }
    });
}

extern "C" fn jet_jit_str_eq(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        match (
            rt.strings.get(a as usize).map(String::as_str),
            rt.strings.get(b as usize).map(String::as_str),
        ) {
            (Some(x), Some(y)) => i8::from(x == y),
            _ => 0,
        }
    })
}

extern "C" fn jet_jit_struct_new_f64(n: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let id = rt.structs_f64.len() as i64;
        rt.structs_f64.push(vec![0.0; n as usize]);
        id
    })
}

extern "C" fn jet_jit_struct_get_f64(h: i64, idx: i64) -> f64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.structs_f64
            .get(h as usize)
            .and_then(|s| s.get(idx as usize).copied())
            .unwrap_or(0.0)
    })
}

extern "C" fn jet_jit_struct_set_f64(h: i64, idx: i64, v: f64) {
    with_runtime_mut(|rt| {
        if let Some(s) = rt.structs_f64.get_mut(h as usize) {
            if let Some(slot) = s.get_mut(idx as usize) {
                *slot = v;
            }
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
    str_begin: FuncId,
    str_push_lit: FuncId,
    str_push_i64: FuncId,
    str_push_f64: FuncId,
    str_push_bool: FuncId,
    str_push_char: FuncId,
    str_push_str: FuncId,
    str_eq: FuncId,
    struct_new_f64: FuncId,
    struct_get_f64: FuncId,
    struct_set_f64: FuncId,
    is_trapped: FuncId,
    coll: Collections::CollectionsHostFns,
    conc: Concurrency::ConcurrencyHostFns,
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
    builder.symbol("jet_jit_str_begin", jet_jit_str_begin as *const u8);
    builder.symbol("jet_jit_str_push_lit", jet_jit_str_push_lit as *const u8);
    builder.symbol("jet_jit_str_push_i64", jet_jit_str_push_i64 as *const u8);
    builder.symbol("jet_jit_str_push_f64", jet_jit_str_push_f64 as *const u8);
    builder.symbol("jet_jit_str_push_bool", jet_jit_str_push_bool as *const u8);
    builder.symbol("jet_jit_str_push_char", jet_jit_str_push_char as *const u8);
    builder.symbol("jet_jit_str_push_str", jet_jit_str_push_str as *const u8);
    builder.symbol("jet_jit_str_eq", jet_jit_str_eq as *const u8);
    builder.symbol(
        "jet_jit_struct_new_f64",
        jet_jit_struct_new_f64 as *const u8,
    );
    builder.symbol(
        "jet_jit_struct_get_f64",
        jet_jit_struct_get_f64 as *const u8,
    );
    builder.symbol(
        "jet_jit_struct_set_f64",
        jet_jit_struct_set_f64 as *const u8,
    );
    builder.symbol("jet_jit_is_trapped", jet_jit_is_trapped as *const u8);
    Collections::register_collections_symbols(&mut builder);
    Concurrency::register_concurrency_symbols(&mut builder);
    let mut module = JITModule::new(builder);
    let coll = Collections::declare_collections_host_fns(&mut module)?;
    let conc = Concurrency::declare_concurrency_host_fns(&mut module)?;
    let host = declare_host_fns(&mut module, coll, conc)?;
    Ok((module, host))
}

fn declare_host_fns(
    module: &mut JITModule,
    coll: Collections::CollectionsHostFns,
    conc: Concurrency::ConcurrencyHostFns,
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
    let mut sig_str_push_lit = Signature::new(cc);
    sig_str_push_lit.params.push(AbiParam::new(types::I64));
    sig_str_push_lit.params.push(AbiParam::new(types::I64));
    let mut sig_str_push_i64 = Signature::new(cc);
    sig_str_push_i64.params.push(AbiParam::new(types::I64));
    sig_str_push_i64.params.push(AbiParam::new(types::I64));
    let mut sig_str_push_f64 = Signature::new(cc);
    sig_str_push_f64.params.push(AbiParam::new(types::I64));
    sig_str_push_f64.params.push(AbiParam::new(types::F64));
    let mut sig_str_push_bool = Signature::new(cc);
    sig_str_push_bool.params.push(AbiParam::new(types::I64));
    sig_str_push_bool.params.push(AbiParam::new(types::I8));
    let mut sig_str_push_char = Signature::new(cc);
    sig_str_push_char.params.push(AbiParam::new(types::I64));
    sig_str_push_char.params.push(AbiParam::new(types::I32));
    let mut sig_str_eq = Signature::new(cc);
    sig_str_eq.params.push(AbiParam::new(types::I64));
    sig_str_eq.params.push(AbiParam::new(types::I64));
    sig_str_eq.returns.push(AbiParam::new(types::I8));
    let mut sig_str_begin = Signature::new(cc);
    sig_str_begin.returns.push(AbiParam::new(types::I64));
    let mut sig_struct_new = Signature::new(cc);
    sig_struct_new.params.push(AbiParam::new(types::I64));
    sig_struct_new.returns.push(AbiParam::new(types::I64));
    let mut sig_struct_get = Signature::new(cc);
    sig_struct_get.params.push(AbiParam::new(types::I64));
    sig_struct_get.params.push(AbiParam::new(types::I64));
    sig_struct_get.returns.push(AbiParam::new(types::F64));
    let mut sig_struct_set = Signature::new(cc);
    sig_struct_set.params.push(AbiParam::new(types::I64));
    sig_struct_set.params.push(AbiParam::new(types::I64));
    sig_struct_set.params.push(AbiParam::new(types::F64));
    let mut sig_is_trapped = Signature::new(cc);
    sig_is_trapped.returns.push(AbiParam::new(types::I64));

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
        str_begin: import("jet_jit_str_begin", &sig_str_begin)?,
        str_push_lit: import("jet_jit_str_push_lit", &sig_str_push_lit)?,
        str_push_i64: import("jet_jit_str_push_i64", &sig_str_push_i64)?,
        str_push_f64: import("jet_jit_str_push_f64", &sig_str_push_f64)?,
        str_push_bool: import("jet_jit_str_push_bool", &sig_str_push_bool)?,
        str_push_char: import("jet_jit_str_push_char", &sig_str_push_char)?,
        str_push_str: import("jet_jit_str_push_str", &sig_str_push_lit)?,
        str_eq: import("jet_jit_str_eq", &sig_str_eq)?,
        struct_new_f64: import("jet_jit_struct_new_f64", &sig_struct_new)?,
        struct_get_f64: import("jet_jit_struct_get_f64", &sig_struct_get)?,
        struct_set_f64: import("jet_jit_struct_set_f64", &sig_struct_set)?,
        is_trapped: import("jet_jit_is_trapped", &sig_is_trapped)?,
        coll,
        conc,
    })
}

fn flatten_string(parts: &[TStrPart]) -> Option<String> {
    let mut out = String::new();
    for p in parts {
        match p {
            TStrPart::Lit(s) => out.push_str(s),
            TStrPart::Interp(_, _) => return None,
        }
    }
    Some(out)
}

fn jit_covers_string_parts(parts: &[TStrPart], callees: &HashSet<String>) -> bool {
    parts.iter().all(|p| match p {
        TStrPart::Lit(_) => true,
        TStrPart::Interp(e, _) => jit_covers_expr(e, callees),
    })
}

fn jit_scalar_type(ty: &Type) -> bool {
    jit_value_type(ty)
}

fn jit_list_int_type(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if matches!(inner.as_ref(), Type::Int))
}

fn jit_list_task_int_type(ty: &Type) -> bool {
    if let Type::List(inner) = ty {
        if let Type::Apply { name, args } = inner.as_ref() {
            return name == "Task" && args.len() == 1 && matches!(&args[0], Type::Int);
        }
    }
    false
}

fn jit_optional_scalar_type(ty: &Type) -> bool {
    matches!(ty, Type::Option(inner) if jit_scalar_type(inner))
}

fn user_type_name(ty: &Type) -> Option<&str> {
    match ty {
        Type::Named(n) if n != "Unit" => Some(n.as_str()),
        Type::Apply { name, .. } => Some(name.as_str()),
        _ => None,
    }
}

fn jit_struct_type(ty: &Type) -> bool {
    user_type_name(ty).is_some()
}

fn jit_enum_type(ty: &Type) -> bool {
    user_type_name(ty).is_some()
}

fn jit_compound_type(ty: &Type) -> bool {
    jit_list_int_type(ty)
        || jit_list_task_int_type(ty)
        || jit_struct_type(ty)
        || jit_enum_type(ty)
        || jit_optional_scalar_type(ty)
}

fn jit_concurrency_elem(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if n == "Unit") || jit_scalar_type(ty)
}

fn jit_concurrency_type(ty: &Type) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    matches!(name.as_str(), "Task" | "Receiver" | "Sender")
        && args.len() == 1
        && jit_concurrency_elem(&args[0])
}

fn jit_value_type(ty: &Type) -> bool {
    match ty {
        Type::Named(n) if n == "Unit" => true,
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => true,
        other if jit_concurrency_type(other) => true,
        other if jit_compound_type(other) => true,
        _ => false,
    }
}

fn jit_covers_expr(expr: &TExpr, callees: &HashSet<String>) -> bool {
    match &expr.kind {
        TExprKind::Print(inner) => jit_covers_expr(inner, callees),
        TExprKind::Call { name, args } => {
            if !callees.contains(name) {
                return false;
            }
            args.iter().all(|a| jit_covers_call_arg(a, callees))
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
            jit_covers_expr_list(args, callees)
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
            if *is_option {
                jit_covers_expr(value, callees)
                    && matches!(fallback, TOrFallback::Value(_) | TOrFallback::Panic(_))
            } else {
                !is_option
                    && jit_covers_expr(value, callees)
                    && matches!(fallback, TOrFallback::Panic(_))
            }
        }
        TExprKind::ListLit(elems) => {
            (jit_list_int_type(&expr.ty)
                && elems
                    .iter()
                    .all(|e| matches!(&e.ty, Type::Int) && jit_covers_expr(e, callees)))
                || (jit_list_task_int_type(&expr.ty)
                    && elems.iter().all(|e| jit_covers_expr(e, callees)))
        }
        TExprKind::Index {
            base,
            index,
            is_map,
            ..
        } => {
            !is_map
                && jit_list_int_type(&base.ty)
                && matches!(&index.ty, Type::Int)
                && jit_covers_expr(base, callees)
                && jit_covers_expr(index, callees)
        }
        TExprKind::Slice {
            base, start, end, ..
        } => {
            jit_list_int_type(&base.ty)
                && matches!(&start.ty, Type::Int)
                && matches!(&end.ty, Type::Int)
                && jit_covers_expr(base, callees)
                && jit_covers_expr(start, callees)
                && jit_covers_expr(end, callees)
        }
        TExprKind::BuiltinMethod { recv, op, args } => {
            jit_covers_builtin_op(op, recv, args, callees)
        }
        TExprKind::StructLit { fields, .. } => {
            jit_struct_type(&expr.ty) && fields.iter().all(|(_, v, _)| jit_covers_expr(v, callees))
        }
        TExprKind::Field { recv, .. } => jit_covers_expr(recv, callees),
        TExprKind::MethodCall { recv, args, .. } => {
            jit_covers_expr(recv, callees) && args.iter().all(|a| jit_covers_call_arg(a, callees))
        }
        TExprKind::StaticCall { args, .. } => args.iter().all(|a| jit_covers_call_arg(a, callees)),
        TExprKind::EnumLit { payload, .. } => {
            jit_enum_type(&expr.ty) && jit_covers_enum_payload(payload, callees)
        }
        TExprKind::Present(inner) | TExprKind::Ok(inner) | TExprKind::Err(inner) => {
            jit_covers_expr(inner, callees)
        }
        TExprKind::Absent => true,
        _ if !jit_value_type(&expr.ty) => false,
        TExprKind::IntLit(_, _)
        | TExprKind::FloatLit(_)
        | TExprKind::BoolLit(_)
        | TExprKind::CharLit(_) => true,
        TExprKind::StrLit(parts) => jit_covers_string_parts(parts, callees),
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
        TExprKind::TaskGroupAll { tasks } => {
            jit_list_int_type(&expr.ty) && jit_covers_task_list_expr(tasks, callees)
        }
        TExprKind::TaskGroupRace { tasks } | TExprKind::TaskGroupAny { tasks } => {
            matches!(&expr.ty, Type::Int) && jit_covers_task_list_expr(tasks, callees)
        }
        TExprKind::SelectStart => true,
        TExprKind::SelectRecv { builder, channel } => {
            jit_covers_expr(builder, callees)
                && jit_concurrency_type(&channel.ty)
                && jit_covers_expr(channel, callees)
        }
        TExprKind::SelectAfter { builder, millis } => {
            jit_covers_expr(builder, callees)
                && matches!(&millis.ty, Type::Int)
                && jit_covers_expr(millis, callees)
        }
        TExprKind::SelectRead { builder, .. } => jit_covers_expr(builder, callees),
        TExprKind::SelectWait { builder } => {
            jit_value_type(&expr.ty) && jit_covers_select_wait(builder, callees)
        }
        _ => false,
    }
}

fn jit_covers_call_arg(arg: &TCallArg, callees: &HashSet<String>) -> bool {
    // TIR marks non-scalar params as `borrow`; JIT passes them by handle/discriminant.
    (!arg.borrow || jit_value_type(&arg.value.ty))
        && !arg.mut_borrow
        && !arg.clone
        && !arg.arc_clone
        && arg.fn_coerce.is_none()
        && !arg.widen_to_vec
        && (!jit_struct_type(&arg.value.ty) || arg.borrow)
        && jit_covers_expr(&arg.value, callees)
}

fn jit_covers_enum_payload(payload: &TEnumPayload, callees: &HashSet<String>) -> bool {
    match payload {
        TEnumPayload::Unit => true,
        TEnumPayload::Positional(vals) => vals.iter().all(|a| jit_covers_expr(&a.value, callees)),
        TEnumPayload::Named(fields) => fields
            .iter()
            .all(|(_, a)| jit_covers_expr(&a.value, callees)),
    }
}

fn jit_covers_builtin_op(
    op: &TBuiltinOp,
    recv: &TExpr,
    args: &[TExpr],
    callees: &HashSet<String>,
) -> bool {
    if !jit_covers_expr(recv, callees) {
        return false;
    }
    match op {
        TBuiltinOp::Push => {
            jit_list_int_type(&recv.ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && jit_covers_expr(&args[0], callees)
        }
        TBuiltinOp::Sort | TBuiltinOp::LenList => jit_list_int_type(&recv.ty) && args.is_empty(),
        TBuiltinOp::GetList => {
            jit_list_int_type(&recv.ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && jit_covers_expr(&args[0], callees)
        }
        TBuiltinOp::JoinSep => {
            jit_list_int_type(&recv.ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && jit_covers_expr(&args[0], callees)
        }
        TBuiltinOp::Slice { .. } => {
            jit_list_int_type(&recv.ty)
                && args.len() == 2
                && matches!(&args[0].ty, Type::Int)
                && matches!(&args[1].ty, Type::Int)
                && jit_covers_expr(&args[0], callees)
                && jit_covers_expr(&args[1], callees)
        }
        _ => false,
    }
}

fn jit_covers_stmt(stmt: &TStmt, callees: &HashSet<String>) -> bool {
    match stmt {
        TStmt::Let { init, .. } => jit_covers_expr(init, callees),
        // D-TUPLE-DESTRUCT1: `(tx, rx) := tasks.channel<T>()` — the one
        // tuple-destructure shape this tier covers (general `TupleDestructure` /
        // `StructDestructure` / `ListDestructure` are not covered at all otherwise;
        // that's unrelated pre-existing scope, not narrowed here). Mirrors the old
        // single-handle `let ch := tasks.channel()` + `ch.sender()` shape exactly:
        // one `channel_new` call for the receiver handle, one `channel_sender` call
        // on it for the sender handle — same two host calls, now both fired at the
        // producer site instead of at a later `.sender()` call.
        TStmt::TupleDestructure { init, binds, .. } => {
            binds.len() == 2
                && matches!(
                    &init.kind,
                    TExprKind::CoreCall { module, method, args }
                        if module == "core.tasks" && method == "channel" && args.is_empty()
                )
        }
        TStmt::Assign {
            value, clone_value, ..
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
                TIfCond::Matches { .. } => false,
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
        TStmt::While { label, cond, body } => {
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
        TStmt::IndexAssign {
            base,
            index,
            is_map,
            value,
        } => {
            !is_map
                && jit_list_int_type(&base.ty)
                && matches!(&index.ty, Type::Int)
                && matches!(&value.ty, Type::Int)
                && jit_covers_expr(base, callees)
                && jit_covers_expr(index, callees)
                && jit_covers_expr(value, callees)
        }
        TStmt::ForIn {
            label,
            var2,
            method_kind,
            columnar,
            body,
            ..
        } => {
            label.is_none()
                && var2.is_none()
                && method_kind.is_none()
                && !columnar
                && body.iter().all(|s| jit_covers_stmt(s, callees))
        }
        TStmt::EnumMatch {
            arms, else_body, ..
        } => {
            arms.iter()
                .all(|a| a.body.iter().all(|s| jit_covers_stmt(s, callees)))
                && else_body
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| jit_covers_stmt(s, callees)))
        }
        TStmt::MixedSwitch {
            arms, else_body, ..
        } => {
            arms.iter()
                .all(|(_, b)| b.iter().all(|s| jit_covers_stmt(s, callees)))
                && else_body
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| jit_covers_stmt(s, callees)))
        }
        TStmt::Region(body) => body.iter().all(|s| jit_covers_stmt(s, callees)),
        _ => false,
    }
}

fn jit_covers_func(tir: &TFunc, callees: &HashSet<String>) -> bool {
    jit_covers_func_detail(tir, callees).is_none()
}

fn jit_covers_func_detail(tir: &TFunc, callees: &HashSet<String>) -> Option<String> {
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
        if !jit_covers_stmt(s, callees) {
            return Some(format!("body stmt {i}"));
        }
    }
    None
}

fn jit_covers_program(program: &JitProgram) -> bool {
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let main_ok = program.funcs.iter().any(|f| {
        f.name == "run" && f.params.is_empty() && f.ret.is_none() && jit_covers_func(f, &names)
    });
    if !main_ok {
        return false;
    }
    if !program.funcs.iter().all(|f| jit_covers_func(f, &names)) {
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

fn jit_covers_expr_list(exprs: &[TExpr], callees: &HashSet<String>) -> bool {
    exprs.iter().all(|e| jit_covers_expr(e, callees))
}

fn jit_covers_task_list_expr(tasks: &TExpr, callees: &HashSet<String>) -> bool {
    jit_list_task_int_type(&tasks.ty) && jit_covers_expr(tasks, callees)
}

fn jit_covers_select_wait(builder: &TExpr, callees: &HashSet<String>) -> bool {
    let (recvs, afters) = collect_select_arms_jit(builder);
    !recvs.is_empty()
        && recvs
            .iter()
            .all(|ch| jit_concurrency_type(&ch.ty) && jit_covers_expr(ch, callees))
        && afters
            .iter()
            .all(|ms| matches!(&ms.ty, Type::Int) && jit_covers_expr(ms, callees))
}

fn collect_select_arms_jit<'a>(builder: &'a TExpr) -> (Vec<&'a TExpr>, Vec<&'a TExpr>) {
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
            } => {
                afters.push(millis.as_ref());
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
    if !lam.params.iter().all(|(_, ty)| jit_value_type(ty)) {
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
        THandleOp::TaskJoin | THandleOp::TaskCancel => {
            args.is_empty() && jit_concurrency_type(&recv.ty)
        }
        THandleOp::ChannelReceive => {
            args.is_empty() && matches!(&recv.ty, Type::Apply { name, .. } if name == "Receiver")
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
    if matches!(&init.ty, Type::List(_)) {
        return Ok(types::I64);
    }
    if matches!(&init.ty, Type::Named(_)) {
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
    if jit_list_int_type(ty)
        || jit_list_task_int_type(ty)
        || jit_struct_type(ty)
        || jit_enum_type(ty)
    {
        return Some(types::I64);
    }
    if jit_optional_scalar_type(ty) {
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
    if matches!(tir.kind, TFuncKind::Method { self_conv: Some(_) }) {
        sig.params.push(AbiParam::new(types::I64));
    }
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
    if name == "run" {
        "jet_jit_main".to_string()
    } else {
        format!("jet_jit_fn_{}", name.replace("::", "__"))
    }
}

struct JitMeta<'a> {
    struct_fields: &'a HashMap<String, Vec<String>>,
    struct_field_types: &'a HashMap<String, Vec<Type>>,
    enum_variants: &'a HashMap<String, Vec<String>>,
}

impl<'a> JitMeta<'a> {
    fn from_program(program: &'a JitProgram) -> Self {
        JitMeta {
            struct_fields: &program.struct_fields,
            struct_field_types: &program.struct_field_types,
            enum_variants: &program.enum_variants,
        }
    }

    fn struct_field_index(&self, type_name: &str, field_rust: &str) -> Option<usize> {
        self.struct_fields
            .get(type_name)?
            .iter()
            .position(|f| f == field_rust)
    }

    fn struct_field_type(&self, type_name: &str, field_rust: &str) -> Option<Type> {
        let idx = self.struct_field_index(type_name, field_rust)?;
        self.struct_field_types.get(type_name)?.get(idx).cloned()
    }

    fn enum_variant_disc(&self, prefix: &str) -> Option<i64> {
        let (enum_part, variant) = prefix.rsplit_once("::")?;
        let enum_name = enum_part.strip_prefix("user_").unwrap_or(enum_part);
        let variants = self.enum_variants.get(enum_name)?;
        let variant_key = variant.strip_prefix("user_").unwrap_or(variant);
        variants
            .iter()
            .position(|v| v == variant || v.strip_prefix("user_").unwrap_or(v) == variant_key)
            .map(|i| i as i64)
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
    meta: &'a JitMeta<'a>,
    vars: &'a mut HashMap<String, Variable>,
    func_ids: &'a HashMap<String, FuncId>,
    spawn_site: &'a mut usize,
    spawn_func_ids: &'a [FuncId],
    spawn_lambdas: &'a [TJitSpawnLambda],
    loop_stack: Vec<LoopTargets>,
    dead: bool,
    next_var: u32,
    /// Owning struct for inherent methods (`Point::dist_sq` → `Point`).
    method_struct: Option<String>,
    /// CLIF return type of the function being lowered (`None` = returns void).
    /// Drives the dummy value `emit_trap_check` returns on the trap-unwind path.
    ret_clif: Option<types::Type>,
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

    /// After any call that may set the runtime's trapped flag — a fallible host
    /// shim (checked arith, list get/set/slice, channel receive/panic) or a call
    /// to another jet function (a callee's trap must propagate transitively) —
    /// read the flag and, if set, branch to this function's epilogue returning a
    /// dummy value. The whole unwind is Cranelift control flow: no Rust panic is
    /// ever unwound through a JIT frame (cranelift-jit emits no unwind tables —
    /// doing so would be UB, forbidden by I1). `resident_invoke` observes the
    /// flag after `main` returns and reports the trap as `E0953`.
    fn emit_trap_check(&mut self) -> Result<(), String> {
        let is_ref = self
            .module
            .declare_func_in_func(self.host.is_trapped, self.b.func);
        let call = self.b.ins().call(is_ref, &[]);
        let flag = self.b.inst_results(call)[0];
        let zero = self.b.ins().iconst(types::I64, 0);
        let trapped = self.b.ins().icmp(IntCC::NotEqual, flag, zero);
        let epilogue = self.b.create_block();
        let cont = self.b.create_block();
        self.b.ins().brif(trapped, epilogue, &[], cont, &[]);

        self.b.switch_to_block(epilogue);
        self.b.seal_block(epilogue);
        match self.ret_clif {
            Some(ty) => {
                let dv = if ty == types::F64 {
                    self.b.ins().f64const(0.0)
                } else {
                    self.b.ins().iconst(ty, 0)
                };
                self.b.ins().return_(&[dv]);
            }
            None => {
                self.b.ins().return_(&[]);
            }
        }

        self.b.switch_to_block(cont);
        self.b.seal_block(cont);
        Ok(())
    }

    fn lower_stmts_scoped(&mut self, stmts: &[TStmt]) -> Result<(), String> {
        self.dead = false;
        self.lower_stmts(stmts)
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
            // D-TUPLE-DESTRUCT1: `(tx, rx) := tasks.channel<T>()`. The coverage gate
            // (`jit_covers_stmt`) admitted only this exact shape: a 2-element
            // `TupleDestructure` whose init is the `tasks.channel` producer, canonical
            // field order `(sender, receiver)`. Reproduce the old single-handle
            // `let ch := tasks.channel(); s := ch.sender();` host calls — `channel_new`
            // for the receiver handle, then `channel_sender` on it for the sender
            // handle — both fired here instead of at a later `.sender()` call.
            TStmt::TupleDestructure { binds, .. } => {
                let ch_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.channel_new, self.b.func);
                let ch_call = self.b.ins().call(ch_ref, &[]);
                let ch_val = self.b.inst_results(ch_call)[0];
                let tx_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.channel_sender, self.b.func);
                let tx_call = self.b.ins().call(tx_ref, &[ch_val]);
                let tx_val = self.b.inst_results(tx_call)[0];
                // `binds[i].0` is already the mangled Rust name (`mangle(elem.name)`,
                // set at TIR lowering — unlike plain `Let.name`, which is the raw Jet
                // name and needs `local_place`'s own `mangle` call). Use it directly.
                let tx_var = self.fresh_var(types::I64);
                self.b.def_var(tx_var, tx_val);
                self.vars.insert(binds[0].0.clone(), tx_var);
                let ch_var = self.fresh_var(types::I64);
                self.b.def_var(ch_var, ch_val);
                self.vars.insert(binds[1].0.clone(), ch_var);
            }
            TStmt::Assign {
                place, op, value, ..
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
                self.b.ins().brif(cond_val, body_block, &[], exit, &[]);

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
                self.b.ins().brif(cond_val, body_block, &[], exit, &[]);

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
                let past_end = self.b.ins().icmp(IntCC::SignedGreaterThan, cur, end_val);
                self.b.ins().brif(past_end, exit, &[], body_block, &[]);

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
            TStmt::IndexAssign {
                base,
                index,
                is_map,
                value,
            } => {
                if *is_map {
                    return Err("jit map assign unsupported".to_string());
                }
                let list = self.lower_expr(base)?;
                let idx = self.lower_expr(index)?;
                let val = self.lower_expr(value)?;
                let line = self.b.ins().iconst(types::I32, 1);
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_set, self.b.func);
                self.b.ins().call(host_ref, &[list, idx, val, line]);
                self.emit_trap_check()?;
            }
            TStmt::ForIn {
                var,
                collection_str,
                body,
                ..
            } => {
                let coll_place = collection_str.trim().to_string();
                let coll_var = self
                    .vars
                    .get(&coll_place)
                    .copied()
                    .ok_or_else(|| format!("jit for-in unknown collection `{coll_place}`"))?;
                let header = self.b.create_block();
                let body_block = self.b.create_block();
                let step_block = self.b.create_block();
                let exit = self.b.create_block();
                let idx_var = self.fresh_var(types::I64);
                let zero = self.b.ins().iconst(types::I64, 0);
                self.b.def_var(idx_var, zero);
                self.b.ins().jump(header, &[]);

                self.b.switch_to_block(header);
                let idx = self.b.use_var(idx_var);
                let coll = self.b.use_var(coll_var);
                let len_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_len, self.b.func);
                let len_call = self.b.ins().call(len_ref, &[coll]);
                let len = self.b.inst_results(len_call)[0];
                let done = self.b.ins().icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
                self.b.ins().brif(done, exit, &[], body_block, &[]);

                self.loop_stack.push(LoopTargets {
                    continue_block: step_block,
                    break_block: exit,
                });
                self.b.switch_to_block(body_block);
                self.b.seal_block(body_block);
                let line = self.b.ins().iconst(types::I32, 1);
                let get_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_get, self.b.func);
                let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
                let elem = self.b.inst_results(get_call)[0];
                self.emit_trap_check()?;
                let loop_var = self.fresh_var(types::I64);
                self.b.def_var(loop_var, elem);
                self.vars.insert(TIR::local_place(var), loop_var);
                self.lower_stmts_scoped(body)?;
                self.loop_stack.pop();
                if !self.dead {
                    self.b.ins().jump(step_block, &[]);
                }

                self.b.switch_to_block(step_block);
                self.b.seal_block(step_block);
                let idx = self.b.use_var(idx_var);
                let one = self.b.ins().iconst(types::I64, 1);
                let next = self.b.ins().iadd(idx, one);
                self.b.def_var(idx_var, next);
                self.b.ins().jump(header, &[]);
                self.b.seal_block(header);

                self.b.switch_to_block(exit);
                self.b.seal_block(exit);
                self.dead = false;
            }
            TStmt::EnumMatch {
                scrutinee,
                arms,
                else_body,
                fallthrough,
            } => {
                let subj = self.scrutinee_value(scrutinee)?;
                let merge = self.b.create_block();
                let mut tail = self.b.create_block();
                self.b.ins().jump(tail, &[]);
                for arm in arms {
                    self.b.switch_to_block(tail);
                    self.b.seal_block(tail);
                    let disc = self
                        .meta
                        .enum_variant_disc(&arm.pattern)
                        .ok_or_else(|| format!("jit enum pattern `{}`", arm.pattern))?;
                    let then_block = self.b.create_block();
                    let next = self.b.create_block();
                    let disc_const = self.b.ins().iconst(types::I64, disc);
                    let eq = self.bool_from_icmp(IntCC::Equal, subj, disc_const);
                    self.b.ins().brif(eq, then_block, &[], next, &[]);
                    self.b.switch_to_block(then_block);
                    self.b.seal_block(then_block);
                    self.lower_stmts_scoped(&arm.body)?;
                    if !self.dead {
                        self.b.ins().jump(merge, &[]);
                    }
                    tail = next;
                }
                self.b.switch_to_block(tail);
                self.b.seal_block(tail);
                if let Some(body) = else_body {
                    self.lower_stmts_scoped(body)?;
                    if !self.dead {
                        self.b.ins().jump(merge, &[]);
                    }
                } else if *fallthrough {
                    self.b.ins().trap(TrapCode::UnreachableCodeReached);
                } else if !self.dead {
                    self.b.ins().jump(merge, &[]);
                }
                self.b.switch_to_block(merge);
                self.b.seal_block(merge);
                self.dead = false;
            }
            TStmt::MixedSwitch {
                arms, else_body, ..
            } => {
                let merge = self.b.create_block();
                let mut tail = self.b.create_block();
                self.b.ins().jump(tail, &[]);
                for (cond, body) in arms {
                    self.b.switch_to_block(tail);
                    self.b.seal_block(tail);
                    let cond_val = self.lower_expr(cond)?;
                    let then_block = self.b.create_block();
                    let next = self.b.create_block();
                    self.b.ins().brif(cond_val, then_block, &[], next, &[]);
                    self.b.switch_to_block(then_block);
                    self.b.seal_block(then_block);
                    self.lower_stmts_scoped(body)?;
                    if !self.dead {
                        self.b.ins().jump(merge, &[]);
                    }
                    tail = next;
                }
                self.b.switch_to_block(tail);
                self.b.seal_block(tail);
                if let Some(body) = else_body {
                    self.lower_stmts_scoped(body)?;
                }
                if !self.dead {
                    self.b.ins().jump(merge, &[]);
                }
                self.b.switch_to_block(merge);
                self.b.seal_block(merge);
                self.dead = false;
            }
            TStmt::Region(body) => {
                self.lower_stmts_scoped(body)?;
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
            (BinOp::BitAnd, Type::Int) => self.b.ins().band(current, rhs),
            (BinOp::BitOr, Type::Int) => self.b.ins().bor(current, rhs),
            (BinOp::BitXor, Type::Int) => self.b.ins().bxor(current, rhs),
            (BinOp::Shl, Type::Int) => self.b.ins().ishl(current, rhs),
            (BinOp::Shr, Type::Int) => self.b.ins().sshr(current, rhs),
            (BinOp::Add, Type::Float) => self.b.ins().fadd(current, rhs),
            (BinOp::Sub, Type::Float) => self.b.ins().fsub(current, rhs),
            (BinOp::Mul, Type::Float) => self.b.ins().fmul(current, rhs),
            (BinOp::Div, Type::Float) => self.b.ins().fdiv(current, rhs),
            _ => return Err("jit compound assign unsupported".to_string()),
        })
    }

    fn lower_call_arg(&mut self, arg: &TCallArg) -> Result<Value, String> {
        if arg.mut_borrow
            || arg.clone
            || arg.arc_clone
            || arg.fn_coerce.is_some()
            || arg.widen_to_vec
        {
            return Err("jit call arg wrapper unsupported".to_string());
        }
        if arg.borrow && !jit_value_type(&arg.value.ty) {
            return Err("jit call arg borrow unsupported".to_string());
        }
        self.lower_expr(&arg.value)
    }

    fn lower_string_lit(&mut self, parts: &[TStrPart]) -> Result<Value, String> {
        if let Some(text) = flatten_string(parts) {
            let id = self.runtime.strings.len() as i64;
            self.runtime.strings.push(text);
            return Ok(self.b.ins().iconst(types::I64, id));
        }
        let begin_ref = self
            .module
            .declare_func_in_func(self.host.str_begin, self.b.func);
        let begin_call = self.b.ins().call(begin_ref, &[]);
        let buf_id = self.b.inst_results(begin_call)[0];
        for part in parts {
            match part {
                TStrPart::Lit(s) => {
                    let lit_id = self.runtime.strings.len() as i64;
                    self.runtime.strings.push(s.clone());
                    let lit_const = self.b.ins().iconst(types::I64, lit_id);
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.str_push_lit, self.b.func);
                    self.b.ins().call(host_ref, &[buf_id, lit_const]);
                }
                TStrPart::Interp(e, _) => {
                    let val = self.lower_expr(e)?;
                    let host_id = match &e.ty {
                        Type::Int => self.host.str_push_i64,
                        Type::Float => self.host.str_push_f64,
                        Type::Bool => self.host.str_push_bool,
                        Type::Char => self.host.str_push_char,
                        Type::String => self.host.str_push_str,
                        other => {
                            return Err(format!("jit string interp type unsupported: {other:?}"));
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    match &e.ty {
                        Type::Float => self.b.ins().call(host_ref, &[buf_id, val]),
                        _ => self.b.ins().call(host_ref, &[buf_id, val]),
                    };
                }
            }
        }
        Ok(buf_id)
    }

    fn normalize_place(&self, place: &str) -> Result<String, String> {
        let place = place.trim();
        if let Some(inner) = place.strip_prefix("(*").and_then(|s| s.strip_suffix(')')) {
            return self.normalize_place(inner);
        }
        if self.vars.contains_key(place) {
            return Ok(place.to_string());
        }
        if let Some(name) = place.strip_prefix("user_") {
            return Ok(TIR::local_place(name));
        }
        Ok(place.to_string())
    }

    fn load_place(&mut self, place: &str) -> Result<Value, String> {
        let key = self.normalize_place(place)?;
        let var = self
            .vars
            .get(&key)
            .copied()
            .ok_or_else(|| format!("jit unknown place `{place}`"))?;
        Ok(self.b.use_var(var))
    }

    fn scrutinee_value(&mut self, s: &str) -> Result<Value, String> {
        let trimmed = s.trim();
        if let Some(stripped) = trimmed.strip_suffix(".clone()") {
            let inner = stripped
                .trim()
                .trim_start_matches('(')
                .trim_end_matches(')');
            return self.load_place(inner);
        }
        self.load_place(trimmed)
    }

    fn method_key(recv_ty: &Type, method_rust: &str) -> Option<String> {
        let type_name = user_type_name(recv_ty)?;
        let method = method_rust.strip_prefix("user_").unwrap_or(method_rust);
        Some(format!("{type_name}::{method}"))
    }

    fn static_method_key(type_prefix: &str, method_rust: &str) -> Option<String> {
        let type_name = type_prefix.strip_prefix("user_")?;
        let method = method_rust.strip_prefix("user_").unwrap_or(method_rust);
        Some(format!("{type_name}::{method}"))
    }

    fn lower_struct_lit(&mut self, fields: &[(String, TExpr, bool)]) -> Result<Value, String> {
        let n = self.b.ins().iconst(types::I64, fields.len() as i64);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.struct_new_f64, self.b.func);
        let new_call = self.b.ins().call(new_ref, &[n]);
        let handle = self.b.inst_results(new_call)[0];
        for (i, (_, v, _)) in fields.iter().enumerate() {
            let val = self.lower_expr(v)?;
            let idx = self.b.ins().iconst(types::I64, i as i64);
            let set_ref = self
                .module
                .declare_func_in_func(self.host.struct_set_f64, self.b.func);
            self.b.ins().call(set_ref, &[handle, idx, val]);
        }
        Ok(handle)
    }

    fn lower_list_lit(&mut self, elems: &[TExpr]) -> Result<Value, String> {
        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let new_call = self.b.ins().call(new_ref, &[]);
        let handle = self.b.inst_results(new_call)[0];
        for e in elems {
            let v = self.lower_expr(e)?;
            let push_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_push, self.b.func);
            self.b.ins().call(push_ref, &[handle, v]);
        }
        Ok(handle)
    }

    fn lower_i64_value_list(&mut self, vals: &[Value]) -> Result<Value, String> {
        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let new_call = self.b.ins().call(new_ref, &[]);
        let handle = self.b.inst_results(new_call)[0];
        for v in vals {
            let push_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_push, self.b.func);
            self.b.ins().call(push_ref, &[handle, *v]);
        }
        Ok(handle)
    }

    fn lower_expr(&mut self, expr: &TExpr) -> Result<Value, String> {
        match &expr.kind {
            TExprKind::IntLit(v, _) => Ok(self.b.ins().iconst(types::I64, *v)),
            TExprKind::FloatLit(v) => Ok(self.b.ins().f64const(*v)),
            TExprKind::BoolLit(v) => Ok(self.b.ins().iconst(types::I8, if *v { 1 } else { 0 })),
            TExprKind::CharLit(v) => Ok(self.b.ins().iconst(types::I32, *v as i64)),
            TExprKind::StrLit(parts) => self.lower_string_lit(parts),
            TExprKind::Local(place) => {
                let key = self.normalize_place(place)?;
                let var = self
                    .vars
                    .get(&key)
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
                let result = clif_ty(&expr.ty).map(|_| self.b.inst_results(call)[0]);
                self.emit_trap_check()?;
                Ok(result.unwrap_or_else(|| self.b.ins().iconst(types::I8, 0)))
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
            TExprKind::CoreCall {
                module,
                method,
                args,
            } => {
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
            TExprKind::TaskGroupAll { tasks } => {
                let list = self.lower_expr(tasks)?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_all, self.b.func);
                let call = self.b.ins().call(host_ref, &[list]);
                Ok(self.b.inst_results(call)[0])
            }
            TExprKind::TaskGroupRace { tasks } => {
                let list = self.lower_expr(tasks)?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_race, self.b.func);
                let call = self.b.ins().call(host_ref, &[list]);
                Ok(self.b.inst_results(call)[0])
            }
            TExprKind::TaskGroupAny { tasks } => {
                let list = self.lower_expr(tasks)?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_any, self.b.func);
                let call = self.b.ins().call(host_ref, &[list]);
                Ok(self.b.inst_results(call)[0])
            }
            TExprKind::SelectStart => Ok(self.b.ins().iconst(types::I64, 0)),
            TExprKind::SelectRecv { builder, channel } => {
                let _ = self.lower_expr(builder)?;
                self.lower_expr(channel)
            }
            TExprKind::SelectAfter { builder, millis } => {
                let _ = self.lower_expr(builder)?;
                self.lower_expr(millis)
            }
            TExprKind::SelectRead { builder, .. } => self.lower_expr(builder),
            TExprKind::SelectWait { builder } => {
                let (recvs, afters) = collect_select_arms_jit(builder);
                let mut recv_vals = Vec::new();
                for ch in recvs {
                    recv_vals.push(self.lower_expr(ch)?);
                }
                let mut after_vals = Vec::new();
                for ms in afters {
                    after_vals.push(self.lower_expr(ms)?);
                }
                let recv_list = self.lower_i64_value_list(&recv_vals)?;
                let after_list = self.lower_i64_value_list(&after_vals)?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.select_wait, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_list, after_list]);
                Ok(self.b.inst_results(call)[0])
            }
            TExprKind::OrFallback {
                value,
                fallback,
                is_option,
            } => {
                if *is_option {
                    let status = self.lower_list_get_opt_status(value)?;
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
                    let fb = match fallback {
                        TOrFallback::Value(e) => self.lower_expr(e)?,
                        _ => return Err("jit option fallback unsupported".to_string()),
                    };
                    self.b.ins().jump(merge, &[fb]);
                    self.b.switch_to_block(merge);
                    self.b.seal_block(merge);
                    return Ok(self.b.block_params(merge)[0]);
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
                        let host_ref = self
                            .module
                            .declare_func_in_func(self.host.conc.panic_channel_closed, self.b.func);
                        let call = self.b.ins().call(host_ref, &[line]);
                        let panic_val = self.b.inst_results(call)[0];
                        self.emit_trap_check()?;
                        self.b.ins().jump(merge, &[panic_val]);
                    }
                    _ => return Err("jit or-fallback unsupported".to_string()),
                }
                self.b.switch_to_block(merge);
                self.b.seal_block(merge);
                Ok(self.b.block_params(merge)[0])
            }
            TExprKind::ListLit(elems) => self.lower_list_lit(elems),
            TExprKind::Index {
                base,
                index,
                is_map,
                line,
            } => {
                if *is_map {
                    return Err("jit map index unsupported".to_string());
                }
                let list = self.lower_expr(base)?;
                let idx = self.lower_expr(index)?;
                let line_const = self.b.ins().iconst(types::I32, *line as i64);
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_get, self.b.func);
                let call = self.b.ins().call(host_ref, &[list, idx, line_const]);
                let result = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                Ok(result)
            }
            TExprKind::Slice {
                base,
                start,
                end,
                line,
            } => {
                let list = self.lower_expr(base)?;
                let s = self.lower_expr(start)?;
                let e = self.lower_expr(end)?;
                let line_const = self.b.ins().iconst(types::I32, *line as i64);
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_slice, self.b.func);
                let call = self.b.ins().call(host_ref, &[list, s, e, line_const]);
                let result = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                Ok(result)
            }
            TExprKind::BuiltinMethod { recv, op, args } => {
                self.lower_builtin_method(recv, op, args, &expr.ty)
            }
            TExprKind::StructLit { fields, .. } => self.lower_struct_lit(fields),
            TExprKind::Field {
                recv, field_rust, ..
            } => {
                let handle = self.lower_expr(recv)?;
                let type_name = user_type_name(&recv.ty)
                    .map(str::to_string)
                    .or_else(|| self.method_struct.clone())
                    .ok_or("jit field recv type")?;
                let idx = self
                    .meta
                    .struct_field_index(&type_name, field_rust)
                    .ok_or_else(|| format!("jit field `{field_rust}` on `{type_name}`"))?
                    as i64;
                let idx_val = self.b.ins().iconst(types::I64, idx);
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.struct_get_f64, self.b.func);
                let call = self.b.ins().call(host_ref, &[handle, idx_val]);
                let raw = self.b.inst_results(call)[0];
                let field_ty = self
                    .meta
                    .struct_field_type(&type_name, field_rust)
                    .unwrap_or_else(|| expr.ty.clone());
                Ok(match field_ty {
                    Type::Float => raw,
                    Type::Int => self.b.ins().fcvt_to_sint(types::I64, raw),
                    other => {
                        return Err(format!("jit field type unsupported: {other:?}"));
                    }
                })
            }
            TExprKind::MethodCall {
                recv,
                method_rust,
                args,
            } => {
                let key = Self::method_key(&recv.ty, method_rust)
                    .ok_or_else(|| format!("jit method on {:?}", recv.ty))?;
                let func_id = self
                    .func_ids
                    .get(&key)
                    .copied()
                    .ok_or_else(|| format!("jit missing method `{key}`"))?;
                let mut arg_vals = vec![self.lower_expr(recv)?];
                for a in args {
                    arg_vals.push(self.lower_call_arg(a)?);
                }
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &arg_vals);
                let result = clif_ty(&expr.ty).map(|_| self.b.inst_results(call)[0]);
                self.emit_trap_check()?;
                Ok(result.unwrap_or_else(|| self.b.ins().iconst(types::I8, 0)))
            }
            TExprKind::StaticCall {
                type_prefix,
                method_rust,
                args,
            } => {
                let key = Self::static_method_key(type_prefix, method_rust)
                    .ok_or_else(|| format!("jit static `{type_prefix}::{method_rust}`"))?;
                let func_id = self
                    .func_ids
                    .get(&key)
                    .copied()
                    .ok_or_else(|| format!("jit missing static `{key}`"))?;
                let arg_vals: Result<Vec<_>, _> =
                    args.iter().map(|a| self.lower_call_arg(a)).collect();
                let arg_vals = arg_vals?;
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &arg_vals);
                let result = clif_ty(&expr.ty).map(|_| self.b.inst_results(call)[0]);
                self.emit_trap_check()?;
                Ok(result.unwrap_or_else(|| self.b.ins().iconst(types::I8, 0)))
            }
            TExprKind::EnumLit { prefix, payload } => match payload {
                TEnumPayload::Unit => {
                    let disc = self
                        .meta
                        .enum_variant_disc(prefix)
                        .ok_or_else(|| format!("jit enum lit `{prefix}`"))?;
                    Ok(self.b.ins().iconst(types::I64, disc))
                }
                _ => Err("jit enum payload unsupported".to_string()),
            },
            TExprKind::Present(inner) => {
                let v = self.lower_expr(inner)?;
                // Encode optional Some as value+1 (0 = None elsewhere).
                let one = self.b.ins().iconst(types::I64, 1);
                Ok(self.b.ins().iadd(v, one))
            }
            TExprKind::Absent => Ok(self.b.ins().iconst(types::I64, 0)),
            _ => Err("jit expression unsupported".to_string()),
        }
    }

    fn lower_list_get_opt_status(&mut self, value: &TExpr) -> Result<Value, String> {
        if let TExprKind::BuiltinMethod {
            recv,
            op: TBuiltinOp::GetList,
            args,
        } = &value.kind
        {
            let list = self.lower_expr(recv)?;
            let idx = self.lower_expr(&args[0])?;
            let host_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_get_opt, self.b.func);
            let call = self.b.ins().call(host_ref, &[list, idx]);
            return Ok(self.b.inst_results(call)[0]);
        }
        Err("jit list get_opt status unsupported".to_string())
    }

    fn lower_builtin_method(
        &mut self,
        recv: &TExpr,
        op: &TBuiltinOp,
        args: &[TExpr],
        _ret_ty: &Type,
    ) -> Result<Value, String> {
        let recv_val = self.lower_expr(recv)?;
        match op {
            TBuiltinOp::Push => {
                let v = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_push, self.b.func);
                self.b.ins().call(host_ref, &[recv_val, v]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::Sort => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_sort, self.b.func);
                self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::LenList => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_len, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::GetList => {
                let idx = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_get_opt, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, idx]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::JoinSep => {
                let sep = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_join_str, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, sep]);
                Ok(self.b.inst_results(call)[0])
            }
            _ => Err("jit builtin method unsupported".to_string()),
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
            THandleOp::TaskCancel => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_cancel, self.b.func);
                self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THandleOp::ChannelReceive => {
                let line = self.b.ins().iconst(types::I32, 1);
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.channel_receive, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, line]);
                let result = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                Ok(result)
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
            let host_ref = self
                .module
                .declare_func_in_func(self.host.conc.channel_receive_status, self.b.func);
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

    fn expr_field_type(&self, expr: &TExpr) -> Option<Type> {
        let TExprKind::Field {
            recv, field_rust, ..
        } = &expr.kind
        else {
            return None;
        };
        let type_name = user_type_name(&recv.ty)
            .map(str::to_string)
            .or_else(|| self.method_struct.clone())?;
        self.meta.struct_field_type(&type_name, field_rust)
    }

    fn expr_arith_type(&self, expr: &TExpr) -> Type {
        if let Some(t) = self.expr_field_type(expr) {
            return t;
        }
        if let TExprKind::Binary { lhs, rhs, .. } = &expr.kind {
            let lt = self.expr_arith_type(lhs);
            let rt = self.expr_arith_type(rhs);
            if lt == Type::Float || rt == Type::Float {
                return Type::Float;
            }
            if lt == Type::Int && rt == Type::Int {
                return Type::Int;
            }
        }
        expr.ty.clone()
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
            let result = self.b.inst_results(call)[0];
            self.emit_trap_check()?;
            return Ok(result);
        }
        let lhs_ty = self.expr_arith_type(lhs);
        let _rhs_ty = self.expr_arith_type(rhs);
        Ok(match (&lhs_ty, op) {
            (Type::Int, BinOp::Add) => self.b.ins().iadd(l, r),
            (Type::Int, BinOp::Sub) => self.b.ins().isub(l, r),
            (Type::Int, BinOp::Mul) => self.b.ins().imul(l, r),
            (Type::Int, BinOp::Div) => self.b.ins().sdiv(l, r),
            (Type::Int, BinOp::Rem) => self.b.ins().srem(l, r),
            (Type::Int, BinOp::BitAnd) => self.b.ins().band(l, r),
            (Type::Int, BinOp::BitOr) => self.b.ins().bor(l, r),
            (Type::Int, BinOp::BitXor) => self.b.ins().bxor(l, r),
            (Type::Int, BinOp::Shl) => self.b.ins().ishl(l, r),
            (Type::Int, BinOp::Shr) => self.b.ins().sshr(l, r),
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
            (Type::String, BinOp::Eq) => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_eq, self.b.func);
                let call = self.b.ins().call(host_ref, &[l, r]);
                self.b.inst_results(call)[0]
            }
            (Type::String, BinOp::Ne) => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_eq, self.b.func);
                let call = self.b.ins().call(host_ref, &[l, r]);
                let eq = self.b.inst_results(call)[0];
                let one = self.b.ins().iconst(types::I8, 1);
                self.b.ins().isub(one, eq)
            }
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
            TExprKind::CharLit(v) => (
                self.host.print_char,
                self.b.ins().iconst(types::I32, *v as i64),
            ),
            TExprKind::StrLit(parts) => {
                let id = self.lower_string_lit(parts)?;
                (self.host.print_str, id)
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
        sig.params
            .push(AbiParam::new(clif_ty(ty).unwrap_or(types::I64)));
    }
    if clif_ty(&lam.ret).is_some() {
        sig.returns.push(AbiParam::new(types::I64));
    }
    sig
}

fn lower_spawn_function(
    module: &mut JITModule,
    host: &HostFns,
    meta: &JitMeta<'_>,
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
            meta,
            vars: &mut vars,
            func_ids,
            spawn_site: &mut spawn_site,
            spawn_func_ids,
            spawn_lambdas,
            loop_stack: Vec::new(),
            dead: false,
            next_var: 0,
            method_struct: None,
            ret_clif: clif_ty(&lam.ret),
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

fn stmt_has_return(stmts: &[TStmt]) -> bool {
    stmts.iter().any(|s| match s {
        TStmt::Return(_) => true,
        TStmt::If {
            then_body,
            else_body,
            ..
        } => stmt_has_return(then_body) || else_body.as_ref().is_some_and(|b| stmt_has_return(b)),
        TStmt::Loop { body, .. }
        | TStmt::While { body, .. }
        | TStmt::Range { body, .. }
        | TStmt::ForIn { body, .. }
        | TStmt::Region(body) => stmt_has_return(body),
        TStmt::CountedLoop {
            init, step, body, ..
        } => {
            stmt_has_return(std::slice::from_ref(init))
                || stmt_has_return(std::slice::from_ref(step))
                || stmt_has_return(body)
        }
        TStmt::EnumMatch {
            arms, else_body, ..
        } => {
            arms.iter().any(|a| stmt_has_return(&a.body))
                || else_body.as_ref().is_some_and(|b| stmt_has_return(b))
        }
        TStmt::MixedSwitch {
            arms, else_body, ..
        } => {
            arms.iter().any(|(_, b)| stmt_has_return(b))
                || else_body.as_ref().is_some_and(|b| stmt_has_return(b))
        }
        _ => false,
    })
}

fn lower_function(
    module: &mut JITModule,
    host: &HostFns,
    meta: &JitMeta<'_>,
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
        let mut param_idx = 0usize;
        let method_struct = match &tir.kind {
            TFuncKind::Method { .. } => tir.name.split_once("::").map(|(t, _)| t.to_string()),
            _ => None,
        };
        let mut lctx = LowerCtx {
            b: &mut b,
            module,
            host,
            runtime,
            meta,
            vars: &mut vars,
            func_ids,
            spawn_site,
            spawn_func_ids,
            spawn_lambdas,
            loop_stack: Vec::new(),
            dead: false,
            next_var: 0,
            method_struct,
            ret_clif: tir.ret.as_ref().and_then(clif_ty),
        };
        if matches!(tir.kind, TFuncKind::Method { self_conv: Some(_) }) {
            let self_var = lctx.fresh_var(types::I64);
            lctx.b.def_var(self_var, param_vals[0]);
            lctx.vars.insert("self".to_string(), self_var);
            param_idx = 1;
        }
        for (i, (name, ty, _)) in tir.params.iter().enumerate() {
            let clif = clif_ty(ty).ok_or("jit param clif type")?;
            let var = lctx.fresh_var(clif);
            lctx.b.def_var(var, param_vals[param_idx + i]);
            lctx.vars.insert(name.clone(), var);
        }

        lctx.lower_stmts(&tir.body)?;
        if !stmt_has_return(&tir.body) {
            if let Some(ret) = &tir.ret {
                if clif_ty(ret).is_some() {
                    return Err("jit function missing return".to_string());
                }
            }
            b.ins().return_(&[]);
        }
        b.finalize();
    }
    if let Err(e) = cranelift_codegen::verify_function(&ctx.func, module.isa()) {
        return Err(format!("{}: verifier: {e:?}", tir.name));
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
    let meta = JitMeta::from_program(program);

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
        let id = if f.name == "run" {
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
            &meta,
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
            &meta,
            f,
            id,
            &func_ids,
            &spawn_func_ids,
            spawn_lambdas,
            &mut spawn_site,
            runtime,
        )
        .map_err(|e| format!("{}: {e}", f.name))?;
    }

    module.finalize_definitions().map_err(|e| e.to_string())?;
    Ok(func_ids
        .get("run")
        .copied()
        .ok_or_else(|| "jit program missing run".to_string())?)
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
        task_controls: Vec::new(),
        lists: Vec::new(),
        structs_f64: Vec::new(),
        trapped: None,
    }
}

/// Build the E0953 diagnostic for a trapped run, matching the tier-0
/// interpreter's own voice for the identical panic (the dev interpreter IS the
/// comptime tree-walker, so its runtime panics already render this way — see
/// `crates/jet-comptime/src/Comptime/Diagnostics.rs::comptime_panic`). The JIT
/// tier must report the SAME code/voice, not a new one, for parity.
fn jit_panic_diag(msg: &str) -> Diagnostic {
    Diagnostic::error(
        "E0953",
        "your comptime code stopped the build".to_string(),
        format!("while computing this value at compile time, the program panicked: {msg}"),
        "this is the sanctioned way to validate at compile time — fix the input the check rejects"
            .to_string(),
        None,
    )
}

/// Scrub heap state a trapped (partial) run created, so the NEXT resident
/// invocation (hot-reload iteration or plain re-run) in this same process
/// starts clean — a crashed run must never leak lists/strings/channels/tasks
/// into the following one. `source_file`/`invocations` are run-loop
/// bookkeeping, not per-run heap, and are left alone.
fn reset_run_heap(rt: &mut JitRuntime) {
    rt.strings.clear();
    rt.lists.clear();
    rt.structs_f64.clear();
    rt.channels.clear();
    rt.senders.clear();
    rt.tasks.clear();
    rt.task_controls.clear();
}

fn resident_teardown() {
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = None);
    Concurrency::set_active_runtime(None);
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
        Concurrency::set_active_runtime(Some(ptr));
        let entry: extern "C" fn() = unsafe { std::mem::transmute(code) };
        entry();
        jet_codegen::scheduler::jet_scheduler_drain();
        Concurrency::set_active_runtime(None);
        if let Some(msg) = runtime.trapped.take() {
            // A runtime panic unwound to `main`'s epilogue via the trapped-flag
            // branches (no Rust panic crossed a JIT frame — I1). Report it exactly
            // as the tier-0 interpreter reports the same panic (E0953), and scrub
            // the partial run's heap so the next hot-reload iteration in this
            // resident process starts clean.
            reset_run_heap(runtime);
            return Ok(RunOutcome::Problems(vec![jit_panic_diag(&msg)]));
        }
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
    let mut runtime =
        RESIDENT_RUNTIME.with(|slot| slot.borrow_mut().take().unwrap_or_else(fresh_runtime));
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

/// Test hook: MixedSwitch arm condition strings from lowered `main`.
#[doc(hidden)]
pub fn jit_dump_mixed_switch_conds(bundle: &ProgramBundle) -> Vec<String> {
    let Some(program) = TIR::lower_jit_program(bundle) else {
        return vec!["<no program>".into()];
    };
    let mut out = Vec::new();
    for f in &program.funcs {
        for s in &f.body {
            if let TStmt::MixedSwitch { arms, .. } = s {
                for (c, _) in arms {
                    out.push(format!("{}: {:?}", f.name, c.ty));
                }
            }
        }
    }
    out
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

/// Test hook: lowered function names in the JIT program.
#[doc(hidden)]
pub fn jit_program_func_names(bundle: &ProgramBundle) -> Vec<String> {
    let Some(program) = TIR::lower_jit_program(bundle) else {
        return vec!["<no program>".into()];
    };
    program.funcs.iter().map(|f| f.name.clone()).collect()
}

/// Test hook: per-function jit coverage detail (`None` = covered).
#[doc(hidden)]
pub fn jit_func_coverage_detail(bundle: &ProgramBundle, name: &str) -> Option<String> {
    let program = TIR::lower_jit_program(bundle)?;
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let f = program.funcs.iter().find(|f| f.name == name)?;
    jit_covers_func_detail(f, &names)
}

/// Test hook: dump lowered run stmt tags.
#[doc(hidden)]
pub fn jit_dump_main_stmts(bundle: &ProgramBundle) -> Vec<String> {
    let Some(program) = TIR::lower_jit_program(bundle) else {
        return vec!["<no program>".into()];
    };
    let Some(m) = program.funcs.iter().find(|f| f.name == "run") else {
        return vec!["<no run>".into()];
    };
    m.body
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{i}:{}", jit_stmt_tag(s)))
        .collect()
}

/// Test hook: count select recv/timer arms on the first `SelectWait` in `run`.
#[doc(hidden)]
pub fn jit_select_arm_counts(bundle: &ProgramBundle) -> Option<(usize, usize)> {
    let program = TIR::lower_jit_program(bundle)?;
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let m = program.funcs.iter().find(|f| f.name == "run")?;
    for s in &m.body {
        if let TStmt::Region(body) = s {
            for inner in body {
                if let TStmt::Let { init, .. } = inner {
                    if let TExprKind::SelectWait { builder } = &init.kind {
                        let (r, a) = collect_select_arms_jit(builder);
                        let _ = &names;
                        return Some((r.len(), a.len()));
                    }
                }
            }
        }
    }
    None
}
#[doc(hidden)]
pub fn jit_main_uncovered_detail(bundle: &ProgramBundle) -> Option<String> {
    let program = TIR::lower_jit_program(bundle)?;
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let m = program.funcs.iter().find(|f| f.name == "run")?;
    for (i, s) in m.body.iter().enumerate() {
        if jit_covers_stmt(s, &names) {
            continue;
        }
        if let TStmt::Region(body) = s {
            for (j, inner) in body.iter().enumerate() {
                if !jit_covers_stmt(inner, &names) {
                    let extra = if let TStmt::Let { init, .. } = inner {
                        if let TExprKind::TaskGroupAll { tasks } = &init.kind {
                            format!(
                                ", init=TaskGroupAll tasks={} list_ok={} tasks_ok={}",
                                jit_expr_tag(tasks),
                                jit_list_task_int_type(&tasks.ty),
                                jit_covers_expr(tasks, &names)
                            )
                        } else {
                            format!(", init={}", jit_expr_tag(init))
                        }
                    } else if let TStmt::ExprStmt(e) = inner {
                        format!(", expr={}", jit_expr_tag(e))
                    } else {
                        String::new()
                    };
                    return Some(format!(
                        "main[{i}] region[{j}]={}{extra}",
                        jit_stmt_tag(inner)
                    ));
                }
            }
        }
        return Some(format!("main[{i}]={}", jit_stmt_tag(s)));
    }
    None
}

/// Test hook: label a lowered expr for diagnostics.
#[doc(hidden)]
pub fn jit_expr_tag(expr: &TExpr) -> &'static str {
    match &expr.kind {
        TExprKind::Print(_) => "Print",
        TExprKind::Call { .. } => "Call",
        TExprKind::CoreCall { .. } => "CoreCall",
        TExprKind::CoreClosureCall { .. } => "CoreClosureCall",
        TExprKind::HandleMethod { .. } => "HandleMethod",
        TExprKind::ListLit(_) => "ListLit",
        TExprKind::TaskGroupAll { .. } => "TaskGroupAll",
        TExprKind::TaskGroupRace { .. } => "TaskGroupRace",
        TExprKind::TaskGroupAny { .. } => "TaskGroupAny",
        TExprKind::SelectStart => "SelectStart",
        TExprKind::SelectRecv { .. } => "SelectRecv",
        TExprKind::SelectAfter { .. } => "SelectAfter",
        TExprKind::SelectRead { .. } => "SelectRead",
        TExprKind::SelectWait { .. } => "SelectWait",
        TExprKind::MethodCall { .. } => "MethodCall",
        TExprKind::Local(_) => "Local",
        TExprKind::Binary { .. } => "Binary",
        TExprKind::Index { .. } => "Index",
        _ => "Other",
    }
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

/// Test hook: whether TIR lowers this bundle for JIT (`lower_jit_program` gate).
#[doc(hidden)]
pub fn tir_lowers_bundle(bundle: &ProgramBundle) -> bool {
    TIR::lower_jit_program(bundle).is_some()
}

/// Test hook: why `lower_jit_program` returned `None`.
#[doc(hidden)]
pub fn tir_lower_fail_reason(bundle: &ProgramBundle) -> String {
    TIR::lower_jit_program_fail_reason(bundle)
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
        f.name == "run" && f.params.is_empty() && f.ret.is_none() && jit_covers_func(f, &names)
    });
    if !main_ok {
        for f in &program.funcs {
            if f.name == "run" {
                if let Some(d) = jit_covers_func_detail(f, &names) {
                    return format!("run not jit-covered: {d}");
                }
            }
        }
        return "run not jit-covered".to_string();
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
    RESIDENT_RUNTIME.with(|slot| slot.borrow().as_ref().map(|r| r.invocations).unwrap_or(0))
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
