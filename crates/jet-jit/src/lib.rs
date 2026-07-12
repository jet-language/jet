//! c139 (D-JITDEP1 / D-JIT2=A) — Cranelift JIT tier-1 backend.
//!
//! Architecture: CraneliftBackend<F: JitBackend> where F is the tier-0
//! fallback. M0 delegates everything to F; M1 uses try-compile lowering to
//! actually compile + run the covered subset natively.
//! M2 keeps a resident JIT module + live runtime heap across hot_swap.
//! M3 widens native lowering: arithmetic, bindings, if/else, calls, loops,
//! compound assign, &&/|| short-circuit.
//! M4: tasks/channels/spawn via scheduler host shims (D-ASYNCRT1=A).

#![allow(non_snake_case)]

mod Collections;
mod Concurrency;
mod Numeric;

use jet_codegen::scheduler::{
    JetSchedulerChannel, JetSchedulerJoin, JetSchedulerSender, JetTaskControl,
};
// I6: Cranelift crates live here, not in the compiler `jet` crate (`Source/`).
// The root package depends on jet-jit; jet-jit depends on cranelift-*.
// D-JITDEP1 approved this as a scoped runtime-side exception.

use jet_foundation::{
    Diagnostics::Diagnostic,
    JitBackend::{JitBackend, RunOutcome},
    AST::{BinOp, IncDecOp, ProgramBundle, Type, UnOp},
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
use std::panic::{catch_unwind, AssertUnwindSafe};

thread_local! {
    /// Compiled module + entry symbol; re-linked on hot_swap.
    static RESIDENT_MODULE: RefCell<Option<ResidentModule>> = const { RefCell::new(None) };
    /// Live heap preserved across type-stable hot_swap; reset on restart.
    static RESIDENT_RUNTIME: RefCell<Option<JitRuntime>> = const { RefCell::new(None) };
}

static TRY_COMPILE_PANIC_HOOK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Runtime-neutral Result carrier used by native JIT code. Cranelift functions
/// pass one i64 handle for every `Result<T, E>`; payload bits stay exact and
/// are decoded using the statically checked TIR payload type.
#[derive(Clone, Copy)]
struct JitResultValue {
    ok: bool,
    bits: u64,
}

include!("jit/runtime_host.rs");
include!("jit/safety.rs");
include!("jit/types_meta.rs");
include!("jit/lower_ctx.rs");
include!("jit/functions_compile.rs");
include!("jit/resident.rs");
include!("jit/api_debug.rs");
include!("jit/backend.rs");
