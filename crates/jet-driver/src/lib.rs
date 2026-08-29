#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export all lower seams so driver source files can use `crate::AST`, `crate::Sema` etc.
pub use jet_codegen::{
    development_receipt, program_allocator, scheduler, CanonicalAST, Codegen, Collections,
    Comptime, Diagnostics, Formatter, Generics, Lexer, Parser, Sema, Syntax, TargetMachine,
    Traits, AST, SHA256,
};

/// Install the canonical TIR evaluator into comptime/REPL/dev entry points.
#[inline]
pub fn boot_tir_eval() {
    Codegen::TIR::install_comptime_bridge();
}

/// Run front-end work on Jet's canonical compiler worker.
///
/// Every compile / check / run entry point funnels through here, so no caller
/// has to know the compiler's stack requirement: `jet` itself, the LSP, a
/// test-harness thread, and an embedder's thread all get the same budget.
/// The size, the const assert tying it to the accepted nesting depth, and the
/// re-entrancy flag live in [`jet_foundation::CompilerStack`], because
/// `jet-jit` installs the same boundary at its own public entries and I6
/// forbids it from depending on this crate. Nested entry points reuse the
/// active worker, so one invocation crosses the boundary exactly once and
/// never nests worker threads — on either side of that seam.
///
/// The worker is a different thread, so thread-local state a caller
/// established does not follow it. The comptime ambient hooks are the one
/// piece of such state installed *around* a compiler entry point
/// (`Comptime::with_ambient`), so they are carried across explicitly. Every
/// other compiler thread-local is established inside the work itself, from
/// bundle facts (`PackageEdition`), or is a per-thread cache.
///
/// Values and panics propagate unchanged: a panic is re-raised with
/// `resume_unwind`, so the ICE path and the diagnostics a caller catches keep
/// their shape instead of being reshaped into an error.
pub fn run_compiler_work<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    if jet_foundation::CompilerStack::on_compiler_worker() {
        return work();
    }
    let (ambient_core_call, ambient_handle, ambient_extern_call) = Comptime::ambient_hooks();
    jet_foundation::CompilerStack::run_on_compiler_stack(move || {
        boot_tir_eval();
        Comptime::with_ambient(ambient_core_call, ambient_handle, ambient_extern_call, work)
    })
}

pub mod BudgetView;
pub mod Compile;
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
pub mod BuildPluginHook;
pub mod CompilerExtensionHook;
pub mod LibraryExport;
pub mod PluginExport;
// Card #367 / D-PRODUCT-SPLIT1=C slice 3: `EffectBudget`/`LintPolicy` are
// pure policy computation over the manifest/effect-fixpoint data (no
// network/provider/shell), so they live in the shared read-only model too —
// the root `jet` package needs them for `build`/`run`'s effect-budget
// summary and lint-policy enforcement without depending on the full
// `jetpack` engine for that.
pub use jet_pkg_model::Authority;
pub use jet_pkg_model::JetLib::{JetLibArtifact, JetLibStamp};
pub use jet_pkg_model::{
    AdaBind, CBind, CobolBind, ComBind, CppBind, DartBind, DotNetBind, EffectBudget, FortranBind,
    GoBind, JavaBind, JavaScriptBind, LintPolicy, Lock, LuaBind, Manifest, OctaveBind, Package,
    PascalBind, PerlBind, PhpBind, Policy, PowerShellBind, PythonBind, RBind, RubyBind, ScriptDeps,
    Store, TclBind, CFFI, FFI,
};
pub use Compile::CompileOutput;
