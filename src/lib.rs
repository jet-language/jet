//! jet — compiler library.
//!
//! Pipeline: lex -> parse -> sema -> codegen (docs/03-architecture.md).
//! The front end (everything before codegen) owns ALL user-facing
//! correctness and every diagnostic. The Rust backend is a verifier and
//! optimizer, never a source of user-facing errors.

pub mod ast;
pub mod collections;
pub mod codegen;
pub mod diag;
pub mod fmt;
pub mod lexer;
pub mod parser;
pub mod sema;
pub mod syntax;

use diag::{Diagnostic, Severity};

/// Result of a successful compile: generated Rust plus any lint warnings.
#[derive(Debug)]
pub struct CompileOutput {
    pub rust: String,
    pub lints: Vec<Diagnostic>,
}

/// Run the full front end on source text. All lex errors (then all parse
/// errors) surface in one run — M1 error recovery.
pub fn compile(src: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_with_path(src, "input.jet")
}

pub fn compile_with_path(src: &str, file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_with_mode(src, file, sema::CompileMode::Run)
}

/// Compile for `jet test`: optional `main`, at least one test block required.
pub fn compile_tests_with_path(src: &str, file: &str) -> Result<String, Vec<Diagnostic>> {
    let (toks, lex_diags) = lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let mut prog = parser::parse(&toks)?;
    let diags = sema::check_with_mode(&mut prog, sema::CompileMode::Test);
    let mut errors = Vec::new();
    for d in diags {
        if d.severity == Severity::Error {
            errors.push(d);
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(codegen::emit_tests(&prog, src, file))
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
    let diags = sema::check_with_mode(&mut prog, mode);
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
    Ok(CompileOutput {
        rust: codegen::emit(&prog, src, file),
        lints,
    })
}

/// Back-compat: compile and return only Rust (drops lints).
pub fn compile_rust(src: &str) -> Result<String, Vec<Diagnostic>> {
    compile(src).map(|o| o.rust)
}

pub use diag::render_all as render_diagnostics;

/// Pretty-print source to canonical Jet style (M6/S44).
pub fn format_source(src: &str) -> Result<String, Vec<Diagnostic>> {
    fmt::format_source(src)
}
