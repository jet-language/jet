#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export all lower seams so driver source files can use `crate::AST`, `crate::Sema` etc.
pub use jet_codegen::{
    program_allocator, CanonicalAST, Codegen, Collections, Comptime, Diagnostics, Formatter,
    Generics, Lexer, Parser, Sema, Syntax, TargetMachine, Traits, AST, SHA256,
};

/// Install the canonical TIR evaluator into comptime/REPL/dev entry points.
#[inline]
pub fn boot_tir_eval() {
    Codegen::TIR::install_comptime_bridge();
}

thread_local! {
    static ON_COMPILER_WORKER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The explicit stack every compiler entry point runs on.
///
/// Lowering is a recursive descent over user syntax, so the frame budget is
/// per nesting level, never per program size — and the front end already caps
/// that depth: `Diagnostics::MAX_SOURCE_NESTING` (256) is the deepest nesting
/// sema and the TIR evaluator accept, deeper source is reported as `E1403`.
/// So the worst case a valid program can demand is bounded arithmetic, not a
/// guess. Measured per level: ~144 KiB for TIR lowering (per method-call
/// level) and ~51 KiB for Cranelift lowering (per expression level).
///
/// * 256 x 144 KiB = 36 MiB — deepest TIR lowering alone
/// * 256 x 51 KiB = 12.75 MiB — deepest Cranelift lowering alone
/// * 256 x 195 KiB = 48.75 MiB — a program paying both at every level
///
/// 64 MiB covers that worst case with room for the parser/sema frames riding
/// along, and matches the canonical TIR evaluator's own worker. A thread
/// stack is reserved address space committed page by page, so an ordinary
/// compile still touches only the pages it uses.
const COMPILER_STACK_SIZE: usize = 64 * 1024 * 1024;

/// Raising the accepted nesting depth must raise the stack that lowers it.
const _: () = assert!(
    COMPILER_STACK_SIZE >= Diagnostics::MAX_SOURCE_NESTING * 195 * 1024,
    "the compiler worker stack must cover the deepest nesting the front end accepts",
);

/// Run front-end work on Jet's canonical compiler worker.
///
/// Every compile / check / run entry point funnels through here, so no caller
/// has to know the compiler's stack requirement: `jet` itself, the LSP, a
/// test-harness thread, and an embedder's thread all get the same budget.
/// Nested entry points reuse the active worker, so one invocation crosses the
/// boundary exactly once and never nests worker threads.
///
/// The worker is a different thread, so thread-local state a caller
/// established does not follow it. The comptime ambient hooks are the one
/// piece of such state installed *around* a compiler entry point
/// (`Comptime::with_ambient`), so they are carried across explicitly. Every
/// other compiler thread-local is established inside the work itself, from
/// bundle facts (`PackageEdition`), or is a per-thread cache.
///
/// Values and panics propagate unchanged: `join` returns the work's value,
/// and a panic is re-raised with `resume_unwind`, so the ICE path and the
/// diagnostics a caller catches keep their shape instead of being reshaped
/// into an error.
pub fn run_compiler_work<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    if ON_COMPILER_WORKER.with(std::cell::Cell::get) {
        return work();
    }
    let (ambient_core_call, ambient_handle) = Comptime::ambient_hooks();
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("jet-compiler".to_string())
            .stack_size(COMPILER_STACK_SIZE)
            .spawn_scoped(scope, move || {
                ON_COMPILER_WORKER.with(|active| active.set(true));
                boot_tir_eval();
                Comptime::with_ambient(ambient_core_call, ambient_handle, work)
            })
            .unwrap_or_else(|error| {
                jet_foundation::ice!(None, "could not start compiler worker: {error}")
            });
        worker
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}

pub mod Compile;
pub mod BudgetView;
pub mod Driver;
pub mod FixEngine;
pub mod Foreign;
pub mod InterpreterBoundary;
pub mod Loader;
pub mod PhaseTiming;
pub mod ProjectParts;
pub mod QueryService;
// Card #367 / D-PRODUCT-SPLIT1=C: the compiler's module loader needs the
// read-only package/config data model (manifest/lock/store-listing/script-
// deps/FFI-binding parsing), never the `jetpack` package-manager engine
// (provider/network/shell). `PluginExport` is driver-only (plugin export
// API-freeze validation via Sema) and was never used by `jetpack` itself, so
// it lives directly in this crate instead of the shared model.
pub mod PluginExport;
pub mod LibraryExport;
pub mod CompilerExtensionHook;
pub mod BuildPluginHook;
// Card #367 / D-PRODUCT-SPLIT1=C slice 3: `EffectBudget`/`LintPolicy` are
// pure policy computation over the manifest/effect-fixpoint data (no
// network/provider/shell), so they live in the shared read-only model too —
// the root `jet` package needs them for `build`/`run`'s effect-budget
// summary and lint-policy enforcement without depending on the full
// `jetpack` engine for that.
pub use jet_pkg_model::{
    AdaBind, CBind, CFFI, CobolBind, ComBind, CppBind, DartBind, DotNetBind, EffectBudget, FFI, FortranBind, GoBind, JavaBind, LuaBind, Package, PascalBind, PerlBind, PhpBind, Policy, RBind, RubyBind, PowerShellBind, TclBind, LintPolicy, Lock, Manifest, ScriptDeps,
    Store,
};
pub use jet_pkg_model::Authority;
pub use jet_pkg_model::JetLib::{JetLibArtifact, JetLibStamp};
pub use Compile::{bundle_uses_unsafe, Capabilities, CompileOutput};
