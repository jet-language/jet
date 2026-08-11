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

/// #1633: one canonical listing per JIT host symbol.
///
/// Each per-module host-symbol table used to write every symbol four times:
/// the `extern "C" fn` (real code, unaffected here), a `FuncId` struct field,
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
                    builder.symbol($symbol, $host_fn as *const u8);
                    #[cfg(test)]
                    $crate::host_fns_audit::record_registered($symbol);
                )?
            )*
        }

        #[allow(unused_mut)]
        pub(crate) fn $declare_fn(
            $module: &mut cranelift_jit::JITModule,
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
pub(crate) use host_fns;

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
mod Marshal;
mod Math;
mod MathExtra;
mod Ffi;
mod Memory;
mod Mod;
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
mod testing_shared {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/TestingShared.rs");
}
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

/// Install the runtime ambient adapters around an explicitly forced
/// interpreter run, matching whole-program deopt.
pub fn with_interpreter_ambient<R>(body: impl FnOnce() -> R) -> R {
    jet_codegen::Comptime::with_ambient(
        Some(ambient_interp::ambient_core_call),
        Some(ambient_interp::ambient_handle),
        body,
    )
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
    resident_jit_safe_bundle, resident_jit_safe_bundle_detail, run_resident_strict_for_test,
    tir_lower_fail_reason, tir_lowers_bundle, try_compile_bundle,
};
pub use backend::CraneliftBackend;
pub use backend::plan_bundle_tiers;
pub use gap::{entry_run_name, is_e2211, JitGap};
pub use tiers::{publish_trace, set_trace_tiers, take_last_trace, trace_tiers_enabled, Tier, TierPlan, TierRow};
pub use tier_cache::{run_cached_module, take_last_tier_artifact};
pub use trace::{
    fallback_invoked_for_test, jit_executed_for_test, note_fallback_invoked_for_test,
    note_deopt_invoked_for_test, deopt_invoked_for_test, reset_jit_trace_for_test,
    jit_trace_flags_for_test, merge_jit_trace_flags_for_test, JitTraceFlags,
};
