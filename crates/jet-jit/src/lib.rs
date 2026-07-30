//! c139 (D-JITDEP1 / D-JIT2=A) — Cranelift JIT tier-1 backend.
//!
//! Architecture: tiered `CraneliftBackend` for default `jet run` / `jet dev`
//! (D-ONECORE1=A / D-LENS-RUN2=A). Covered functions run native; named gaps
//! deopt to the canonical TIR interpreter. E2211 is retired — silent deopt,
//! `--trace-tiers` for experts. No AOT fallback.
//! M2 keeps a resident JIT module + live runtime heap across hot_swap.
//! M3 widens native lowering: arithmetic, bindings, if/else, calls, loops,
//! compound assign, &&/|| short-circuit.
//! M4: tasks/channels/spawn via scheduler host shims (D-ASYNCRT1=A).

#![allow(non_snake_case)]

mod Archive;
mod Args;
mod ambient_interp;
mod Cell;
mod CLI;
mod Collections;
mod Compress;
mod Concurrency;
mod CoreHost;
mod Crypto;
mod Data;
mod DB;
mod Encoding;
mod enc_stream;
mod Fmt;
mod Game;
mod IO;
mod Layout;
mod Math;
mod Ffi;
mod Memory;
mod net_http_rt;
mod Net;
mod Numeric;
mod Parse;
mod Process;
mod Random;
mod Raylib;
mod Reactive;
mod Sketch;
mod Solver;
mod Text;
mod Time;
mod Ui;
mod Watcher;
mod Web;

/// Shared by prelude `include!` fragments that impl `crate::JetShow`.
pub(crate) trait JetShow {
    fn jet_show(&self) -> String;
}

/// Canonical XML pull engine — EncodingStream refers to `crate::jet_xml_pull`.
pub mod jet_xml_pull {
    pub use jet_foundation::XmlPull::*;
}

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
    static PROGRAM_ARGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

static TRY_COMPILE_PANIC_HOOK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct ProgramArgsGuard(Vec<String>);

impl Drop for ProgramArgsGuard {
    fn drop(&mut self) {
        PROGRAM_ARGS.with(|slot| {
            *slot.borrow_mut() = std::mem::take(&mut self.0);
        });
    }
}

/// Install argv for one JIT run (`argv[0]` = entry path, then program args).
pub fn with_program_args<R>(args: &[String], run: impl FnOnce() -> R) -> R {
    let previous =
        PROGRAM_ARGS.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), args.to_vec()));
    let _guard = ProgramArgsGuard(previous);
    // Keep impure `core.io.args` in lockstep for interpreter deopt (#778).
    jet_codegen::Comptime::with_runtime_argv(args, run)
}

pub(crate) fn program_args() -> Vec<String> {
    PROGRAM_ARGS.with(|slot| slot.borrow().clone())
}

/// Runtime-neutral Result carrier used by native JIT code. Cranelift functions
/// pass one i64 handle for every `Result<T, E>`; payload bits stay exact and
/// are decoded using the statically checked TIR payload type.
#[derive(Clone, Copy)]
pub(crate) struct JitResultValue {
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
#[path = "jit/tiers.rs"]
mod tiers;
#[path = "jit/deopt.rs"]
mod deopt;
#[path = "jit/resident.rs"]
mod resident;
#[path = "jit/api_debug.rs"]
mod api_debug;
#[path = "jit/backend.rs"]
mod backend;
#[path = "jit/gap.rs"]
mod gap;
#[path = "jit/trace.rs"]
mod trace;
#[path = "jit/tier_cache.rs"]
mod tier_cache;

// `Concurrency.rs` (a real sibling module, not an include! fragment) reaches
// `JitRuntime` via `super::JitRuntime` — keep that path alive at crate root.
pub(crate) use runtime_host::JitRuntime;
#[doc(hidden)]
pub use runtime_host::{reset_struct_new_count_for_test, struct_new_count_for_test};

pub use api_debug::{
    cranelift_host_supported, jit_dump_main_ops, jit_dump_main_stmts, jit_dump_mixed_switch_conds,
    jit_expr_tag, jit_main_uncovered_detail, jit_program_func_names, jit_select_arm_counts,
    jit_spawn_stats, jit_stmt_tag, resident_invocations_for_test, resident_jit_func_safety_detail,
    resident_jit_safe_bundle, resident_jit_safe_bundle_detail, tir_lower_fail_reason,
    tir_lowers_bundle, try_compile_bundle,
};
pub use backend::CraneliftBackend;
pub use backend::plan_bundle_tiers;
pub use gap::{entry_run_name, is_e2211, JitGap};
pub use tiers::{set_trace_tiers, take_last_trace, trace_tiers_enabled, Tier, TierPlan, TierRow};
pub use tier_cache::{run_cached_module, take_last_tier_artifact};
pub use trace::{
    fallback_invoked_for_test, jit_executed_for_test, note_fallback_invoked_for_test,
    note_deopt_invoked_for_test, deopt_invoked_for_test, reset_jit_trace_for_test,
    jit_trace_flags_for_test, merge_jit_trace_flags_for_test, JitTraceFlags,
};
