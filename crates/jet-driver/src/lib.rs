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
pub use jetpack as Jetpack;
pub use jetpack::{CBind, CFFI, FFI, Lock, Manifest, PluginExport};
pub use Compile::{bundle_uses_unsafe, Capabilities, CompileOutput};
