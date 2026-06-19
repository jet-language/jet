//! jet — compiler library.
//!
//! Pipeline: lex -> parse -> sema -> codegen (docs/spec/architecture.md).
//! The front end (everything before codegen) owns ALL user-facing
//! correctness and every diagnostic. The Rust backend is a verifier and
//! optimizer, never a source of user-facing errors.

// Source files/modules use PascalCase names (owner decision), which trips the
// non_snake_case lint at module-name level.
#![allow(non_snake_case)]

pub mod AST;
pub mod BuildCache;
pub mod CBind;
pub mod CFFI;
pub mod CLI;
pub mod Codegen;
pub mod Collections;
pub mod Comptime;
pub mod Diagnostics;
pub mod Doctor;
pub mod ExitCodes;
pub mod Explain;
pub mod Fetch;
pub mod FFI;
pub mod FixEngine;
pub mod Formatter;
pub mod Generics;
pub mod Interpreter;
pub mod Jetpack;
pub mod Lexer;
pub mod Loader;
pub mod Lock;
pub mod LSP;
pub mod M9;
pub mod Manifest;
pub mod Parser;
pub mod Publish;
pub mod REPL;
pub mod Sema;
pub mod SHA256;
pub mod Store;
pub mod Syntax;

use Diagnostics::{Diagnostic, Severity};

/// Result of a successful compile: generated Rust plus any lint warnings.
#[derive(Debug)]
pub struct CompileOutput {
    pub rust: String,
    pub lints: Vec<Diagnostic>,
    /// Built FFI bridge when the program declares `extern rust` (M7).
    pub ffi: Option<FFI::FfiLink>,
    /// Native C-library linker args (S59 / E2-M14), ready for `rustc`.
    pub clinks: Vec<String>,
    /// D-TOOL5 (E2-M11): capability flags inferred from the generated code.
    pub capabilities: Capabilities,
}

/// D-TOOL5 (E2-M11, ratified as option C): capability summary emitted by
/// `jet build`. Human-readable by default; `--capabilities-json` for tooling.
#[derive(Debug, Default)]
pub struct Capabilities {
    pub uses_network: bool,
    pub uses_file_io: bool,
    pub uses_unsafe: bool,
    pub uses_ffi: bool,
    pub uses_crypto: bool,
    pub uses_concurrency: bool,
}

impl Capabilities {
    /// Detect capabilities from the generated Rust source.
    pub fn from_rust(rust: &str) -> Self {
        Capabilities {
            uses_network: rust.contains("jet_net_") || rust.contains("jet_http_"),
            uses_file_io: rust.contains("jet_fs_") || rust.contains("jet_io_"),
            uses_unsafe: rust.contains("unsafe {") || rust.contains("unsafe fn"),
            uses_ffi: rust.contains("extern \"C\"") || rust.contains("jet_ffi"),
            uses_crypto: rust.contains("jet_crypto_"),
            uses_concurrency: rust.contains("jet_tasks_") || rust.contains("jet_time_"),
        }
    }

    /// Render a human-readable one-line summary (empty when no special capabilities).
    pub fn summary(&self) -> String {
        let mut caps = Vec::new();
        if self.uses_network { caps.push("network"); }
        if self.uses_file_io { caps.push("file-io"); }
        if self.uses_crypto { caps.push("crypto"); }
        if self.uses_concurrency { caps.push("concurrency"); }
        if self.uses_ffi { caps.push("ffi"); }
        if self.uses_unsafe { caps.push("unsafe"); }
        if caps.is_empty() {
            "capabilities: none".to_string()
        } else {
            format!("capabilities: {}", caps.join(", "))
        }
    }

    /// Render machine-readable JSON for `--capabilities-json`.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"network\":{},\"file_io\":{},\"unsafe\":{},\"ffi\":{},\"crypto\":{},\"concurrency\":{}}}",
            self.uses_network,
            self.uses_file_io,
            self.uses_unsafe,
            self.uses_ffi,
            self.uses_crypto,
            self.uses_concurrency,
        )
    }
}

/// Run the full front end on source text. All lex errors (then all parse
/// errors) surface in one run — M1 error recovery.
pub fn compile(src: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_with_mode(src, "input.jet", Sema::CompileMode::Run)
}

pub fn compile_with_path(src: &str, file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    let _ = src;
    compile_bundle_path(file, Sema::CompileMode::Run)
}

/// Front-end check for a file on disk (and its imports). Library modules
/// need not define `main`; use `compile_with_path` when building or running.
pub fn check_with_path(file: &str) -> Vec<Diagnostic> {
    match Loader::load_entry_with_overlay(file, None, true) {
        Ok(mut bundle) => {
            let mut diags = std::mem::take(&mut bundle.parse_teaching);
            diags.extend(Sema::check_bundle(&mut bundle, Sema::CompileMode::Check));
            diags
        }
        Err(diags) => diags,
    }
}

fn compile_bundle_path(
    file: &str,
    mode: Sema::CompileMode,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts(file, mode, false)
}

/// Like `compile_with_path` but with `--freestanding` mode (E2-M15).
/// Rejects OS-dependent std APIs (E3301) and emits `panic = "abort"` hint.
pub fn compile_freestanding(file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts(file, Sema::CompileMode::Run, true)
}

fn compile_bundle_path_opts(
    file: &str,
    mode: Sema::CompileMode,
    freestanding: bool,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    let mut bundle = Loader::load_entry_with_overlay(file, None, false)?;
    let diags = if freestanding {
        Sema::check_bundle_freestanding(&mut bundle, mode)
    } else {
        Sema::check_bundle(&mut bundle, mode)
    };
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
    let ffi = match FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    let rust = Codegen::emit_bundle(&bundle, mode, ffi.as_ref());
    let capabilities = Capabilities::from_rust(&rust);
    Ok(CompileOutput {
        rust,
        lints,
        ffi,
        // Native C link flags are resolved separately at build time (so that
        // codegen / front-end checks never depend on system link discovery);
        // see `resolve_c_links`.
        clinks: Vec::new(),
        capabilities,
    })
}

/// Resolve native C-library link args for a built program (S59 / E2-M14),
/// surfacing E3201 when a library cannot be located via hangar or pkg-config.
/// Build/run paths call this AFTER a successful compile; codegen and front-end
/// checks do not, keeping link discovery out of semantic checking (I3).
pub fn resolve_c_links(file: &str) -> Result<Vec<String>, Vec<Diagnostic>> {
    let bundle = Loader::load_entry_with_overlay(file, None, false)?;
    if !bundle.cffi.links_c() {
        return Ok(Vec::new());
    }
    bundle.cffi.rustc_link_args(&bundle.project_root)
}

/// Compile for `jet test`: optional `main`, at least one test block required.
pub fn compile_tests_with_path(
    src: &str,
    file: &str,
) -> Result<(String, Option<FFI::FfiLink>), Vec<Diagnostic>> {
    let _ = src;
    let mut bundle = Loader::load_entry_with_overlay(file, None, false)?;
    let diags = Sema::check_bundle(&mut bundle, Sema::CompileMode::Test);
    let mut errors = Vec::new();
    for d in diags {
        if d.severity == Severity::Error {
            errors.push(d);
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi = match FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    Ok((Codegen::emit_bundle_tests(&bundle, ffi.as_ref()), ffi))
}

fn compile_with_mode(
    src: &str,
    file: &str,
    mode: Sema::CompileMode,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    let (toks, lex_diags) = Lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let mut prog = Parser::parse(&toks)?;
    let mut bundle = AST::ProgramBundle {
        entry: 0,
        project_root: std::path::PathBuf::from("."),
        modules: vec![AST::LoadedModule {
            path: std::path::PathBuf::from(file),
            display: file.to_string(),
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            source: src.to_string(),
        }],
        parse_teaching: Vec::new(),
        used_std: std::collections::HashSet::new(),
        cffi: CFFI::CFfi::default(),
    };
    // S59: fold any in-file C FFI modules + resolve `use c.<lib>` forms.
    bundle.cffi = match CFFI::assemble(&mut bundle) {
        Ok(c) => c,
        Err(diags) => return Err(diags),
    };
    let diags = Sema::check_bundle(&mut bundle, mode);
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
    let ffi = match FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    let rust = Codegen::emit_bundle(&bundle, mode, ffi.as_ref());
    let capabilities = Capabilities::from_rust(&rust);
    Ok(CompileOutput {
        rust,
        lints,
        ffi,
        clinks: Vec::new(),
        capabilities,
    })
}

/// Back-compat: compile and return only Rust (drops lints).
pub fn compile_rust(src: &str) -> Result<String, Vec<Diagnostic>> {
    compile(src).map(|o| o.rust)
}

pub use Diagnostics::render_all as render_diagnostics;
pub use Diagnostics::{render_all_colored, render_all_json, render_all_linked};

/// Pretty-print source to canonical Jet style (M6/S44).
pub fn format_source(src: &str) -> Result<String, Vec<Diagnostic>> {
    Formatter::format_source(src)
}

/// Front-end check for one document (LSP / editor integration).
pub fn check_document(path: &str, text: &str) -> Vec<Diagnostic> {
    LSP::check_document(path, text)
}

/// S60 / D-PURE1 (E2-M16): evaluate a pure Jet program via the comptime
/// interpreter and return its output as a stable JSON string. The program's
/// `main()` function is interpreted using the comptime engine; any print calls
/// are captured; the captured output is returned as a JSON string value.
///
/// Returns `Err` diagnostics (E3401/E0951/E0952/E0953) on failure.
pub fn eval_pure_program(src: &str, file: &str) -> Result<String, Vec<Diagnostic>> {
    use std::collections::HashMap;

    let (toks, lex_diags) = Lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let prog = Parser::parse(&toks)?;

    // Collect functions into a map for the comptime evaluator.
    let func_map: HashMap<String, &AST::Func> = prog
        .items
        .iter()
        .filter_map(|item| {
            if let AST::Item::Func(f) = item {
                Some((f.name.clone(), f))
            } else {
                None
            }
        })
        .collect();

    let main_fn = func_map.get("main").ok_or_else(|| {
        vec![Diagnostics::Diagnostic::error(
            "E3401",
            "no `main` function found for `jet eval`".to_string(),
            "pure evaluation needs a `pure fn main()` entry point".to_string(),
            "add `pure fn main() { … }` to the program".to_string(),
            None,
        )]
    })?;

    // Run main() via the comptime engine with a dev sink capturing print output.
    let base_dir = std::path::Path::new(file).parent().unwrap_or(std::path::Path::new("."));
    let mut sink = Comptime::DevSink::new();
    Comptime::run_main(main_fn, &func_map, base_dir, &mut sink)
        .map_err(|d| vec![d])?;
    let text = sink.stdout;
    // Render the captured output as a JSON string.
    let json = if text.trim().is_empty() {
        "null".to_string()
    } else {
        // Try to parse as a number or bool for cleaner output; otherwise quote it.
        let trimmed = text.trim();
        if trimmed == "true" || trimmed == "false" {
            trimmed.to_string()
        } else if trimmed.parse::<i64>().is_ok() || trimmed.parse::<f64>().is_ok() {
            trimmed.to_string()
        } else {
            format!("{:?}", trimmed)
        }
    };
    Ok(json)
}
