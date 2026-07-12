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
mod Solver;

// I6: Cranelift crates live here, not in the compiler `jet` crate (`Source/`).
// The root package depends on jet-jit; jet-jit depends on cranelift-*.
// D-JITDEP1 approved this as a scoped runtime-side exception.

use std::cell::RefCell;

use runtime_host::ResidentModule;

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

#[path = "jit/runtime_host.rs"]
mod runtime_host;
#[path = "jit/safety.rs"]
mod safety;
#[path = "jit/types_meta.rs"]
mod types_meta;
#[path = "jit/lower_ctx.rs"]
mod lower_ctx;
#[path = "jit/functions_compile.rs"]
mod functions_compile;
#[path = "jit/resident.rs"]
mod resident;
#[path = "jit/api_debug.rs"]
mod api_debug;
#[path = "jit/backend.rs"]
mod backend;

// `Concurrency.rs` (a real sibling module, not an include! fragment) reaches
// `JitRuntime` via `super::JitRuntime` — keep that path alive at crate root.
pub(crate) use runtime_host::JitRuntime;

pub use api_debug::{
    cranelift_host_supported, jit_dump_main_ops, jit_dump_main_stmts, jit_dump_mixed_switch_conds,
    jit_expr_tag, jit_main_uncovered_detail, jit_program_func_names, jit_select_arm_counts,
    jit_spawn_stats, jit_stmt_tag, resident_invocations_for_test, resident_jit_func_safety_detail,
    resident_jit_safe_bundle, resident_jit_safe_bundle_detail, tir_lower_fail_reason,
    tir_lowers_bundle, try_compile_bundle,
};
pub use backend::CraneliftBackend;
