#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export all lower seams so driver source files can use `crate::AST`, `crate::Sema` etc.
pub use jet_codegen::{
    CanonicalAST, Codegen, Collections, Comptime, Diagnostics, Formatter, Generics, Lexer, Parser,
    Sema, Syntax, TargetProfile, Traits, AST, SHA256,
};

/// Install the canonical TIR evaluator into comptime/REPL/dev entry points.
#[inline]
pub fn boot_tir_eval() {
    Codegen::TIR::install_comptime_bridge();
}

const COMPILER_STACK_SIZE: usize = 32 * 1024 * 1024;

thread_local! {
    static ON_COMPILER_STACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run front-end work on Jet's fixed compiler stack.
///
/// Nested compiler entry points reuse the active worker.
pub fn run_compiler_work<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    if ON_COMPILER_STACK.with(std::cell::Cell::get) {
        return work();
    }
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("jet-compiler".to_string())
            .stack_size(COMPILER_STACK_SIZE)
            .spawn_scoped(scope, || {
                ON_COMPILER_STACK.with(|active| active.set(true));
                boot_tir_eval();
                work()
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
pub mod CompilerExtensionHook;
pub mod BuildPluginHook;
// Card #367 / D-PRODUCT-SPLIT1=C slice 3: `EffectBudget`/`LintPolicy` are
// pure policy computation over the manifest/effect-fixpoint data (no
// network/provider/shell), so they live in the shared read-only model too —
// the root `jet` package needs them for `build`/`run`'s effect-budget
// summary and lint-policy enforcement without depending on the full
// `jetpack` engine for that.
pub use jet_pkg_model::{
    AdaBind, CBind, CFFI, CobolBind, ComBind, CppBind, DartBind, DotNetBind, EffectBudget, FFI, FortranBind, GoBind, JavaBind, LuaBind, Package, PascalBind, PerlBind, PhpBind, Policy, RBind, RubyBind, PowerShellBind, TclBind, LintPolicy, Lock, Manifest, PackageManifest, ScriptDeps,
    Store,
};
pub use Compile::{bundle_uses_unsafe, Capabilities, CompileOutput};
