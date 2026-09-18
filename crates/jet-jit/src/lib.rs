//! c139 (D-JITDEP1 / D-JIT2=A) — Cranelift JIT tier-1 backend.
//!
//! Architecture: `CraneliftBackend` consumes checked canonical MIR. Native
//! execution is allowed only for functions whose upstream target facts include
//! Cranelift; unsupported MIR is reported at this execution boundary.
//! `--trace-tiers` remains available for experts.
//! M2 keeps a resident JIT module + live runtime heap across hot_swap.
//! M3 widens native lowering: arithmetic, bindings, if/else, calls, loops,
//! compound assign, &&/|| short-circuit.
//! M4: tasks/channels/spawn via scheduler host shims (D-ASYNCRT1=A).

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![expect(
    dead_code,
    reason = "#804: shared Prelude host adapters are compiled by multiple tiers"
)]
#![expect(
    non_snake_case,
    reason = "#804: shared Prelude symbols retain canonical Jet names"
)]

/// Canonical devtools event records are shared by every host, including the
/// generated job Prelude. Keep the names at the JIT root so that an included
/// Prelude sees the same types as the compiler/comptime host.
pub use jet_foundation::Devtools::{JetDevtoolsEvent, JET_DEVTOOLS_MAX_HISTORY};
pub use jet_foundation::DataTree;

/// D-JOB-SUBCMD1=C: the JIT and interpreter import the same Prelude selector
/// as generated AOT mains. Their only extra work is mapping the selected name
/// onto the already checked MIR entry.
pub mod Job {
    include!("../../jet-codegen/src/Prelude/JobQueueTypes.rs");
    include!("../../jet-codegen/src/Prelude/Job.rs");
}

/// D-DEVR-LAW1=A / I9: the same development-act receipt the AOT prelude embeds.
pub mod development_receipt {
    include!("../../jet-codegen/src/Prelude/DevelopmentReceipt.rs");
}


/// The dev watcher sleeps through the same Prelude TimerWheel as language
/// timers. This is only the host adapter; timer ownership stays in
/// `Prelude/Scheduler.rs`.
pub fn scheduler_sleep_ms(millis: u64) {
    jet_codegen::scheduler::jet_scheduler_sleep_ms(millis);
}

#[allow(unused_imports)]
pub(crate) use jet_foundation::EncodingErrors as jet_encoding_errors;
pub(crate) mod jet_json_number {
    include!("../../jet-foundation/src/JSONNumber.rs");
}
pub(crate) mod jet_encoding_json {
    include!("../../jet-foundation/src/EncodingJson.rs");
}
/// #1633: one canonical listing per JIT host symbol.
///
/// Each per-module host-symbol table used to write every symbol four times:
/// the host `fn` itself (real code, unaffected here), a `FuncId` struct field,
/// a `builder.symbol(...)` registration, and a `module.declare_function(...)`
/// import — about 1,300 symbols x the last three listings. Missing one of
/// those three did not fail the build; it failed silently at JIT run time.
/// `host_fns!` takes one entry per symbol and generates the struct, the
/// `builder.symbol` registration function, and the `declare_function` import
/// function from it, so a symbol with a missing piece is a compile error.
///
/// `#extra { field: Type, ... }` passes already-declared delegate values
/// (nested per-module `Host*Fns` structs) straight through as plain struct
/// fields / fn params, for the top-level table that composes them. The `#`
/// is load-bearing, not decoration: `macro_rules!` can't tell an optional
/// `extra { ... }` group apart from a `$field:ident` that happens to be
/// spelled `extra` (`local ambiguity` error) unless the group starts on a
/// token no field name can produce.
///
/// `@shared field: "symbol": sig;` (no `=> host_fn`) declares and imports a
/// `FuncId` whose `builder.symbol` registration is owned by a different
/// module's `host_fns!` block (a handful of symbols — e.g.
/// `jet_jit_event_scope` — are registered once by `Reactive` and imported by
/// both `Reactive` and `Watcher`). Left out of `$register_fn` so
/// registration stays single-owner per symbol.
///
/// D-JITUNWIND1 (#1995 / #1997): the same one-entry-per-symbol input also
/// generates the crate's **no-unwind boundary**. `builder.symbol` registers
/// `host_seam::guarded($host_fn)` — a shim with the same C signature that runs
/// the seam inside `guard_seam` — never `$host_fn` itself. A JIT frame carries
/// no unwind information (`cranelift-jit` registers none), so a Rust panic
/// raised above one aborts the process with `failed to initiate panic,
/// error 5` before any outer `catch_unwind` can exist. Boundary conversion was
/// chosen over registering FDEs for generated code, and its recorded cost is
/// that the guarantee must hold for every entry below — which is why it is
/// generated here instead of reviewed per call site. `host_seam.rs` carries the
/// full decision record and `tests/jit_no_unwind_boundary.rs` is the mechanical
/// check. See also `docs/spec/architecture.md` R13.
macro_rules! host_fns {
    (
        struct $StructName:ident;
        register: $register_fn:ident;
        declare: $declare_fn:ident($module:ident) { $($sigs:tt)* }
        $(#extra { $($extra_field:ident : $extra_ty:ty),* $(,)? })?
        $( $(@shared)? $field:ident : $symbol:literal $(=> $host_fn:path)? : $sig:expr ; )*
    ) => {
        pub(crate) struct $StructName {
            $( pub(crate) $field: cranelift_module::FuncId, )*
            $( $( pub(crate) $extra_field: $extra_ty, )* )?
        }

        impl $StructName {
            /// Resolve a registered host symbol without adding another
            /// per-module lookup table in lowering. The macro input is the
            /// one declaration/registration source for both operations.
            pub(crate) fn lookup(&self, symbol: &str) -> Option<cranelift_module::FuncId> {
                match symbol {
                    $( $symbol => Some(self.$field), )*
                    _ => {
                        $(
                            $(
                                if let Some(id) = self.$extra_field.lookup(symbol) {
                                    return Some(id);
                                }
                            )*
                        )?
                        None
                    }
                }
            }
        }

        pub(crate) fn $register_fn(builder: &mut cranelift_jit::JITBuilder) {
            $(
                $(
                    builder.symbol($symbol, $crate::host_seam::guarded($host_fn));
                    #[cfg(test)]
                    $crate::host_fns_audit::record_registered($symbol);
                )?
            )*
        }

        #[allow(unused_mut)]
        pub(crate) fn $declare_fn<M: cranelift_module::Module>(
            $module: &mut M,
            $( $( $extra_field: $extra_ty, )* )?
        ) -> Result<$StructName, String> {
            $($sigs)*
            let mut import = |name: &str, sig: &cranelift_codegen::ir::Signature| -> Result<cranelift_module::FuncId, String> {
                #[cfg(test)]
                $crate::host_fns_audit::record_declared(name);
                cranelift_module::Module::declare_function(
                    $module,
                    name,
                    cranelift_module::Linkage::Import,
                    sig,
                )
                .map_err(|e| e.to_string())
            };
            Ok($StructName {
                $( $field: import($symbol, &$sig)?, )*
                $( $( $extra_field, )* )?
            })
        }
    };
}

/// #1633 criterion #3 backstop: `JITModule::new`'s eager symbol resolution
/// (cranelift-jit 0.112.3, `backend.rs` `declare_function` for
/// `Linkage::Import`) does `lookup_symbol(name).unwrap_or(null)` and
/// installs a null PLT entry on a miss — it does not fail. So a declared
/// import with no matching registered symbol (e.g. an `@shared` entry whose
/// owning module's registration was deleted) builds and passes `new_jit_module`
/// silently, then calls a null pointer at run time. Every `host_fns!`
/// `register_fn`/`declare_fn` records into these two sets so a test can
/// compare them directly instead of trusting `JITModule::new`'s `Ok`.
#[cfg(test)]
pub(crate) mod host_fns_audit {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    static REGISTERED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    static DECLARED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

    pub(crate) fn record_registered(name: &str) {
        REGISTERED.lock().unwrap().insert(name.to_string());
    }

    pub(crate) fn record_declared(name: &str) {
        DECLARED.lock().unwrap().insert(name.to_string());
    }

    /// Snapshot both sets and clear them for the next test.
    pub(crate) fn take_snapshot() -> (BTreeSet<String>, BTreeSet<String>) {
        let mut registered = REGISTERED.lock().unwrap();
        let mut declared = DECLARED.lock().unwrap();
        (
            std::mem::take(&mut *registered),
            std::mem::take(&mut *declared),
        )
    }
}

mod Archive;
mod ambient_interp;
mod Args;
mod CLI;
mod Cell;
mod Collections;
mod Compress;
mod Compute;
mod Concurrency;
mod CoreHost;
mod Crypto;
mod DB;

pub fn install_job_queue_provider() {
    DB::jit_job_queue_install_provider();
}

mod Receipt;
mod Plugin;
/// The JIT always supplies checked authority-needs metadata through its
/// hidden plugin-load argument. This root symbol only satisfies the shared
/// Prelude's AOT default entry and fails closed if that path is used.
fn jet_plugin_declared_authority_needs() -> Result<Vec<String>, String> {
    Err("plugin load requires compiler-supplied declared authority needs".to_string())
}
mod Encoding;
mod Data;
mod Ffi;
mod Fmt;
mod Game;
mod IO;
mod Layout;
mod Marshal;
mod Math;
mod MathExtra;
mod Memory;
mod Mod;
mod ProcessPrelude;
mod enc_stream;
mod host_seam;
mod net_http_rt;
pub(crate) use jet_codegen::fault_injection;
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
mod testing_shared {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    // The typed evidence error is one type across tiers; the compiler's
    // `test_report` module is its canonical host-side home.
    pub(crate) use jet_codegen::Codegen::test_report::TestEvidenceError;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/TestingShared.rs");
}
mod Time;
mod Ui;
mod Watcher;
mod Web;

pub use DB::{
    inspect_migrations, migration_request_checksum, run_migration, ConsoleDbConnection,
    ConsoleDbQueryResult, ConsoleDbResource, ConsoleDbTransaction, ConsoleDbValue, MigrationLock,
    MigrationOperation, MigrationOutcome, MigrationRequest, MigrationSql, MigrationState,
    MigrationStateRequest, MIGRATION_SCHEMA_IDENTITY,
};
pub use net_http_rt::{ConsoleHttpRequest, ConsoleHttpResponse, ConsoleHttpRouter};

pub use ambient_interp::{
    register_hardware_interpreter_ambient, with_interpreter_ambient, InterpreterAmbientContext,
};

pub fn register_encoding_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    Encoding::register_interpreter_ambient(context);
}

pub fn register_receipt_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    Receipt::register_interpreter_ambient(context);
}

pub fn register_db_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    DB::register_interpreter_ambient(context);
}

pub fn register_ui_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    Ui::register_interpreter_ambient(context);
}

pub fn register_raylib_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    Raylib::register_interpreter_ambient(context);
}

pub fn register_plugin_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    Plugin::register_interpreter_ambient(context);
}

pub fn register_crypto_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    Crypto::register_interpreter_ambient(context);
}

/// Shared by prelude `include!` fragments that impl `crate::JetShow`.
pub(crate) trait JetShow {
    fn jet_show(&self) -> String;
}

/// Display/debug seams for included Prelude fragments (`impl crate::JetDisplay`).
pub(crate) trait JetDisplay {
    fn jet_display(&self) -> String;
}

pub(crate) trait JetDebug {
    fn jet_debug(&self) -> String;
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
}


/// Serializes whole resident JIT runs across the process.
///
/// Resident module and runtime state are thread-local handles with process-wide
/// host resources behind them. Keep one resident run at a time so hot-swap and
/// reset cannot interleave with another run.
static RESIDENT_JIT_RUN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Install argv for one JIT run (`argv[0]` = entry path, then program args).
pub fn with_program_args<R>(args: &[String], run: impl FnOnce() -> R) -> R {
    // `Comptime::RUNTIME_ARGV` is the one invocation carrier for native JIT
    // code and interpreter deopt (#778). Keep this adapter name for callers;
    // storage belongs to the shared Prelude seam, not to a JIT tier.
    jet_codegen::Comptime::with_runtime_argv(args, run)
}

/// Reset process-local Core state before a one-shot JIT or interpreter run.
///
/// AOT gets a fresh process. In-process tiers call this shared boundary so a
/// prior run cannot become observable. Resident hot-swap/restart skip it.
#[doc(hidden)]
pub fn reset_one_shot_core_state() {
    Crypto::runtime::auth_runtime_reset();
    jet_codegen::Comptime::AuthLite::auth_runtime_reset();
    // One-shot runs must not retain `core.watcher` handles or event scopes from
    // a previous in-process invocation. Resident teardown owns its own reset.
    Watcher::clear_watcher_state();
}


/// Clear native `Mod` loads created by the current interpreter invocation.
/// The engine exposes no second lifecycle policy: `JetMod::Drop` in the shared
/// Prelude closes the OS handle and removes its staged payload.
pub fn clear_loaded_modules() {
    Mod::clear();
}

/// Run JIT work on Jet's shared compiler worker. Callers pass a checked MIR
/// program, so this boundary adds only the stack and thread-local transport
/// needed by the resident adapter.
pub fn on_compiler_stack<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    if jet_foundation::CompilerStack::on_compiler_worker() {
        return work();
    }
    let argv = jet_codegen::Comptime::runtime_argv();
    let trace_tiers = tiers::trace_tiers_enabled();
    let fidelity = runtime_host::perf_fidelity_bits();
    let (out, flags, rows, struct_new, artifact, fidelity_out) =
        jet_foundation::CompilerStack::run_on_compiler_stack(move || {
            tiers::set_trace_tiers(trace_tiers);
            runtime_host::set_perf_fidelity_bits(fidelity);
            let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                jet_codegen::Comptime::with_ambient_mir_extern(
                    Some(Ffi::ambient_mir_extern_call_cranelift),
                    || match argv {
                        Some(args) => jet_codegen::Comptime::with_runtime_argv(&args, work),
                        None => work(),
                    },
                )
            }));
            (
                out,
                trace::jit_trace_flags_for_test(),
                tiers::take_last_trace(),
                runtime_host::struct_new_count_for_test(),
                tier_cache::take_last_tier_artifact(),
                runtime_host::perf_fidelity_bits(),
            )
        });
    trace::merge_jit_trace_flags_for_test(flags);
    tiers::record_trace(rows);
    runtime_host::add_struct_new_count_for_test(struct_new);
    tier_cache::publish_last_tier_artifact(artifact);
    runtime_host::set_perf_fidelity_bits(fidelity_out);
    out.unwrap_or_else(|payload| std::panic::resume_unwind(payload))
}

/// Runtime-neutral Result carrier used by native JIT code. Cranelift functions
/// pass one i64 handle for every `Result<T, E>`; payload bits stay exact and
/// are decoded using the statically checked MIR payload type.
#[derive(Clone, Copy)]
pub(crate) struct JitResultValue {
    ok: bool,
    bits: u64,
}

#[path = "jit/api_debug.rs"]
mod api_debug;
#[path = "jit/backend.rs"]
mod backend;
#[path = "jit/deopt.rs"]
mod deopt;
#[path = "jit/functions_compile.rs"]
mod functions_compile;
#[path = "jit/gap.rs"]
mod gap;
#[path = "jit/resident.rs"]
mod resident;
#[path = "jit/runtime_host.rs"]
mod runtime_host;
#[path = "jit/safety.rs"]
mod safety;
#[path = "jit/tier_cache.rs"]
mod tier_cache;
#[path = "jit/tiers.rs"]
mod tiers;
#[path = "jit/trace.rs"]
mod trace;
#[path = "jit/types_meta.rs"]
pub(crate) mod types_meta;

// `Concurrency.rs` (a real sibling module, not an include! fragment) reaches
// `JitRuntime` via `super::JitRuntime` — keep that path alive at crate root.
pub(crate) use runtime_host::JitRuntime;
#[doc(hidden)]
pub use runtime_host::{reset_struct_new_count_for_test, struct_new_count_for_test};

/// Declare that the program about to run owns the process's stdout and stderr,
/// so both in-process tiers write its output through as it is produced instead
/// of accumulating it for a caller that reads `RunOutcome` as data. The one-shot
/// `jet run` / `jet dev` verbs call this: they only re-print that buffer, and
/// withholding it made a piped program that prints and keeps running — a server,
/// a watch loop, anything with a `loop` — print nothing until it ended, while an
/// AOT build of the same source printed immediately. The fact itself, its name,
/// and the routing rule live in `Prelude/Term.rs` (I8/I9).
pub fn set_program_owns_streams() {
    std::env::set_var(IO::term_prelude::JET_TERM_PROGRAM_OWNS_STREAMS, "1");
}

pub use api_debug::{
    cranelift_host_supported, jit_dump_main_ops, jit_dump_main_stmts, jit_dump_mixed_switch_conds,
    jit_expr_tag, jit_main_uncovered_detail, jit_program_func_names, jit_select_arm_counts,
    jit_spawn_stats, jit_stmt_tag, resident_invocations_for_test, resident_jit_func_safety_detail,
    resident_jit_safe_program, resident_jit_safe_program_detail, run_resident_strict_for_test,
    try_compile_debug_aot, try_compile_program, DebugAotObject, ResidentJitSafety,
};
pub use backend::CraneliftBackend;
pub use Ffi::set_bridge_cdylib;
pub use resident::{
    apply_hot_swap, apply_hot_swap_with_program, discard_hot_swap_plan, resident_boot_console,
    ConsoleServiceBinding, ResidentConsoleLease,
};
pub use tier_cache::{cached_artifact_id, run_cached_module, take_last_tier_artifact};
pub use tiers::{
    plan_mir_tiers, record_trace, set_trace_tiers, take_last_trace,
    take_trace_aggregate, trace_tiers_enabled, write_trace_sidecar, MirTierPlan, Tier, TierRow,
    TierTraceAggregate,
};
pub use trace::{
    deopt_invoked_for_test, fallback_invoked_for_test, jit_executed_for_test,
    jit_trace_flags_for_test, merge_jit_trace_flags_for_test, note_deopt_invoked_for_test,
    note_fallback_invoked_for_test, reset_jit_trace_for_test, JitTraceFlags,
};
