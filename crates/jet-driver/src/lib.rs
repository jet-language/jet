#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export all lower seams so driver source files can use `crate::AST`, `crate::Sema` etc.
pub use jet_codegen::{
    CanonicalAST, Codegen, Collections, Comptime, Diagnostics, Formatter, Generics, Lexer, Parser,
    Sema, Syntax, TargetProfile, Traits, AST, SHA256,
};
pub mod Compile;
pub mod Driver;
pub mod Foreign;
pub mod Loader;
pub mod PhaseTiming;
// Card #367 / D-PRODUCT-SPLIT1=C: the compiler's module loader needs the
// read-only package/config data model (manifest/lock/store-listing/script-
// deps/FFI-binding parsing), never the `jetpack` package-manager engine
// (provider/network/shell). `PluginExport` is driver-only (plugin export
// API-freeze validation via Sema) and was never used by `jetpack` itself, so
// it lives directly in this crate instead of the shared model.
pub mod PluginExport;
pub use jet_pkg_model::{CBind, CFFI, FFI, Lock, Manifest, PackageManifest, ScriptDeps, Store};
pub use Compile::{bundle_uses_unsafe, Capabilities, CompileOutput};
