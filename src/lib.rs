//! jet — compiler library.
//!
//! Pipeline: lex -> parse -> sema -> codegen (docs/spec/architecture.md).
//! The front end (everything before codegen) owns ALL user-facing
//! correctness and every diagnostic. The Rust backend is a verifier and
//! optimizer, never a source of user-facing errors.

pub mod ast;
pub mod build_cache;
pub mod cffi;
pub mod cli;
pub mod codegen;
pub mod collections;
pub mod comptime;
pub mod diag;
pub mod doctor;
pub mod exit_codes;
pub mod explain;
pub mod fetch;
pub mod ffi;
pub mod fmt;
pub mod generics;
pub mod jetpack;
pub mod lexer;
pub mod loader;
pub mod lock;
pub mod lsp;
pub mod m9;
pub mod manifest;
pub mod parser;
pub mod sema;
pub mod sha256;
pub mod store;
pub mod syntax;

use diag::{Diagnostic, Severity};

/// Result of a successful compile: generated Rust plus any lint warnings.
#[derive(Debug)]
pub struct CompileOutput {
    pub rust: String,
    pub lints: Vec<Diagnostic>,
    /// Built FFI bridge when the program declares `extern rust` (M7).
    pub ffi: Option<ffi::FfiLink>,
    /// Native C-library linker args (S59 / E2-M14), ready for `rustc`.
    pub clinks: Vec<String>,
}

/// Run the full front end on source text. All lex errors (then all parse
/// errors) surface in one run — M1 error recovery.
pub fn compile(src: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_with_mode(src, "input.jet", sema::CompileMode::Run)
}

pub fn compile_with_path(src: &str, file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    let _ = src;
    compile_bundle_path(file, sema::CompileMode::Run)
}

/// Front-end check for a file on disk (and its imports). Library modules
/// need not define `main`; use `compile_with_path` when building or running.
pub fn check_with_path(file: &str) -> Vec<Diagnostic> {
    match loader::load_entry_with_overlay(file, None, true) {
        Ok(mut bundle) => {
            let mut diags = std::mem::take(&mut bundle.parse_teaching);
            diags.extend(sema::check_bundle(&mut bundle, sema::CompileMode::Check));
            diags
        }
        Err(diags) => diags,
    }
}

fn compile_bundle_path(
    file: &str,
    mode: sema::CompileMode,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    let mut bundle = loader::load_entry_with_overlay(file, None, false)?;
    let diags = sema::check_bundle(&mut bundle, mode);
    let mut errors = Vec::new();
    let mut lints = Vec::new();
    for d in diags {
        match d.severity {
            Severity::Error => errors.push(d),
            Severity::Lint => lints.push(d),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi = match ffi::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    Ok(CompileOutput {
        rust: codegen::emit_bundle(&bundle, mode, ffi.as_ref()),
        lints,
        ffi,
        // Native C link flags are resolved separately at build time (so that
        // codegen / front-end checks never depend on system link discovery);
        // see `resolve_c_links`.
        clinks: Vec::new(),
    })
}

/// Resolve native C-library link args for a built program (S59 / E2-M14),
/// surfacing E3201 when a library cannot be located via hangar or pkg-config.
/// Build/run paths call this AFTER a successful compile; codegen and front-end
/// checks do not, keeping link discovery out of semantic checking (I3).
pub fn resolve_c_links(file: &str) -> Result<Vec<String>, Vec<Diagnostic>> {
    let bundle = loader::load_entry_with_overlay(file, None, false)?;
    if !bundle.cffi.links_c() {
        return Ok(Vec::new());
    }
    bundle.cffi.rustc_link_args(&bundle.project_root)
}

/// Compile for `jet test`: optional `main`, at least one test block required.
pub fn compile_tests_with_path(
    src: &str,
    file: &str,
) -> Result<(String, Option<ffi::FfiLink>), Vec<Diagnostic>> {
    let _ = src;
    let mut bundle = loader::load_entry_with_overlay(file, None, false)?;
    let diags = sema::check_bundle(&mut bundle, sema::CompileMode::Test);
    let mut errors = Vec::new();
    for d in diags {
        if d.severity == Severity::Error {
            errors.push(d);
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi = match ffi::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    Ok((codegen::emit_bundle_tests(&bundle, ffi.as_ref()), ffi))
}

fn compile_with_mode(
    src: &str,
    file: &str,
    mode: sema::CompileMode,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    let (toks, lex_diags) = lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let mut prog = parser::parse(&toks)?;
    let mut bundle = ast::ProgramBundle {
        entry: 0,
        project_root: std::path::PathBuf::from("."),
        modules: vec![ast::LoadedModule {
            path: std::path::PathBuf::from(file),
            display: file.to_string(),
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            source: src.to_string(),
        }],
        parse_teaching: Vec::new(),
        used_std: std::collections::HashSet::new(),
        cffi: cffi::CFfi::default(),
    };
    // S59: fold any in-file C FFI modules + resolve `use c.<lib>` forms.
    bundle.cffi = match cffi::assemble(&mut bundle) {
        Ok(c) => c,
        Err(diags) => return Err(diags),
    };
    let diags = sema::check_bundle(&mut bundle, mode);
    let mut errors = Vec::new();
    let mut lints = Vec::new();
    for d in diags {
        match d.severity {
            Severity::Error => errors.push(d),
            Severity::Lint => lints.push(d),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi = match ffi::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    Ok(CompileOutput {
        rust: codegen::emit_bundle(&bundle, mode, ffi.as_ref()),
        lints,
        ffi,
        clinks: Vec::new(),
    })
}

/// Back-compat: compile and return only Rust (drops lints).
pub fn compile_rust(src: &str) -> Result<String, Vec<Diagnostic>> {
    compile(src).map(|o| o.rust)
}

pub use diag::render_all as render_diagnostics;
pub use diag::{render_all_colored, render_all_json, render_all_linked};

/// Pretty-print source to canonical Jet style (M6/S44).
pub fn format_source(src: &str) -> Result<String, Vec<Diagnostic>> {
    fmt::format_source(src)
}

/// Front-end check for one document (LSP / editor integration).
pub fn check_document(path: &str, text: &str) -> Vec<Diagnostic> {
    lsp::check_document(path, text)
}
