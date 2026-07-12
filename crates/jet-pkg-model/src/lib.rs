//! `jet-pkg-model` — the shared, read-only package/config data model (card
//! #367, D-PRODUCT-SPLIT1=C).
//!
//! This crate is the manifest/lock/store/ref/workspace/script *data* layer:
//! parsing `pkg.jet`, the compiler-facing `Manifest`/`Lock` types, the C FFI
//! binding-generation surface, inline script dependencies, and the read-only
//! subset of the Jetpack hangar store (root resolution + listing already
//! recorded entries). It holds no network/provider/shell engine code — that
//! stays in the `jetpack` crate, which re-exports every module here under
//! its historical paths so its own internal call sites are unchanged.
//!
//! `jet-driver` (the compiler's module loader) depends on this crate instead
//! of the full `jetpack` engine, so the compiler's dependency graph never
//! needs Jetpack's provider/network/shell machinery to resolve `use <pkg>`
//! imports against already-realized packages.

#![allow(non_snake_case)]
#![deny(warnings)]

// Re-export lower seams so files in this crate can use `crate::AST`,
// `crate::Diagnostics`, `crate::Syntax`, `crate::SHA256`, `crate::Lexer`,
// `crate::Parser`, `crate::Sema` without cross-crate path changes — same
// pattern `jetpack` itself already uses for `jet_codegen`'s re-exports.
// `Sema` is needed to validate the closed effect vocabulary a `pkg.jet`
// `policy: { trust:/lints: }` block names (jet-driver already depends on
// Sema transitively through jet-codegen, so this adds nothing new to its
// build graph — it is the compiler's checker, not Jetpack's engine).
pub use jet_sema::{Diagnostics, Lexer, Parser, Sema, Syntax, AST, SHA256};

pub mod CBind;
pub mod CFFI;
pub mod Envelope;
pub mod FFI;
pub mod JSON;
pub mod Lock;
pub mod Manifest;
pub mod PackageManifest;
pub mod Platform;
pub mod RefSpec;
pub mod ScriptDeps;
pub mod Store;
