//! jet — compiler library.
//!
//! Pipeline: lex -> parse -> sema -> codegen (docs/spec/architecture.md).
//! The front end (everything before codegen) owns ALL user-facing
//! correctness and every diagnostic. The Rust backend is a verifier and
//! optimizer, never a source of user-facing errors.

// Source files/modules use PascalCase names (owner decision), which trips the
// non_snake_case lint at module-name level.
#![allow(non_snake_case)]
// Warnings are errors: keeps the build warning-clean (card c115).
#![deny(warnings)]

pub mod AST;
pub mod BuildCache;
pub mod CBind;
pub mod CFFI;
pub mod CLI;
pub mod Codegen;
pub mod Collections;
pub mod Comptime;
pub mod Debug;
pub mod Diagnostics;
pub mod Doctest;
pub mod Doctor;
pub mod ExitCodes;
pub mod Explain;
pub mod Fetch;
pub mod FFI;
pub mod FixEngine;
pub mod Formatter;
pub mod Generics;
pub mod Interpreter;
pub mod JitBackend;
pub mod Jetpack;
pub mod Lexer;
pub mod Loader;
pub mod Lock;
pub mod LSP;
pub mod Traits;
pub mod Manifest;
pub mod Parser;
pub mod PhaseTiming;
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
    /// c110 (P0): derive capabilities from semantic facts, not from scanning
    /// generated Rust text. `used_core` is the set of resolved Core calls
    /// (`module::method`, alias-resolved in sema); `has_unsafe` is whether the
    /// program contains an `#Unsafe` gate or a `core.mem` low-level op;
    /// `has_ffi` is whether it declares `extern rust` or links a C library.
    ///
    /// This is the authoritative path (D-TOOL5 / audit card c110): capability
    /// reporting for policy, sandboxing, and package builds must rest on what
    /// sema resolved a program to do, never on the shape of the lowered Rust.
    pub fn from_sema(
        used_core: &std::collections::HashSet<String>,
        has_unsafe: bool,
        has_ffi: bool,
    ) -> Self {
        let any = |prefixes: &[&str]| {
            used_core
                .iter()
                .any(|k| prefixes.iter().any(|p| k.starts_with(p)))
        };
        Capabilities {
            uses_network: any(&["core.net", "jet.http"]),
            uses_file_io: any(&["core.fs", "core.io", "core.files", "core.path"]),
            uses_unsafe: has_unsafe || any(&["core.mem"]),
            uses_ffi: has_ffi,
            uses_crypto: any(&["jet.crypto"]),
            uses_concurrency: any(&["core.tasks", "core.time", "jet.time"]),
        }
    }

    /// Legacy capability detection by scanning generated Rust. Retained only as
    /// a cross-check for the c110 transition (see `tests/effects.rs` agreement
    /// test); `from_sema` is the path the compiler uses.
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

/// c110: true when the program contains an `#Unsafe fn` anywhere (top-level,
/// method, or trait-impl method). An `#Unsafe { … }` *block* always contains a
/// `core.mem` low-level op (that is what the gate is for), so it is detected via
/// the resolved-Core set in `from_sema`; this catches the whole-function form.
fn bundle_uses_unsafe(bundle: &AST::ProgramBundle) -> bool {
    use AST::Item;
    bundle.modules.iter().any(|m| {
        m.items.iter().any(|it| match it {
            Item::Func(f) => f.is_unsafe,
            Item::Struct(s) => {
                s.methods.iter().any(|x| x.is_unsafe)
                    || s.trait_impls.iter().any(|b| b.methods.iter().any(|x| x.is_unsafe))
            }
            Item::Enum(e) => {
                e.methods.iter().any(|x| x.is_unsafe)
                    || e.trait_impls.iter().any(|b| b.methods.iter().any(|x| x.is_unsafe))
            }
            Item::Impl(i) => i.methods.iter().any(|x| x.is_unsafe),
            _ => false,
        })
    })
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

/// Full sema type-check for `jet eval`: runs the same pipeline as `compile`
/// but with `CompileMode::Eval` so E0122 (main must return `()`) is relaxed
/// while all other diagnostics (type errors, unknown identifiers, etc.) still
/// fire. Returns the error diagnostics, or an empty vec on success.
pub fn check_for_eval(src: &str, file: &str) -> Vec<Diagnostic> {
    let (toks, lex_diags) = Lexer::lex(src);
    if !lex_diags.is_empty() {
        return lex_diags;
    }
    let mut prog = match Parser::parse(&toks) {
        Ok(p) => p,
        Err(ds) => return ds,
    };
    let mut bundle = AST::ProgramBundle {
        entry: 0,
        project_root: std::path::PathBuf::from(
            std::path::Path::new(file).parent().unwrap_or(std::path::Path::new(".")),
        ),
        modules: vec![AST::LoadedModule {
            path: std::path::PathBuf::from(file),
            display: file.to_string(),
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            source: src.to_string(),
        }],
        parse_teaching: Vec::new(),
        used_core: std::collections::HashSet::new(),
        cffi: CFFI::CFfi::default(),
    };
    bundle.cffi = match CFFI::assemble(&mut bundle) {
        Ok(c) => c,
        Err(diags) => return diags,
    };
    let diags = Sema::check_bundle(&mut bundle, Sema::CompileMode::Eval);
    diags
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
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
    let timing = PhaseTiming::enabled();
    let mut timer = PhaseTiming::PhaseTimer::new();
    let mut bundle = Loader::load_entry_with_overlay(file, None, false)?;
    if timing {
        timer.lap("load"); // lex + parse + module resolution
    }
    let diags = if freestanding {
        Sema::check_bundle_freestanding(&mut bundle, mode)
    } else {
        Sema::check_bundle(&mut bundle, mode)
    };
    if timing {
        timer.lap("sema");
    }
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
    if timing {
        timer.lap("ffi");
    }
    let rust = Codegen::emit_bundle(&bundle, mode, ffi.as_ref());
    if timing {
        timer.lap("codegen");
        timer.metric("rust_bytes", rust.len() as u128);
        timer.write_to(&bundle.project_root);
    }
    // c110: capabilities are derived from semantic facts (resolved Core calls,
    // `#Unsafe` gates, FFI declarations), not from scanning the lowered Rust.
    let capabilities = Capabilities::from_sema(
        &bundle.used_core,
        bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
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
    compile_tests_with_path_cov(src, file, false)
}

/// D-COV1: as `compile_tests_with_path`, but with optional `jet test --coverage`
/// instrumentation. `coverage = false` produces the historical, uninstrumented
/// harness.
pub fn compile_tests_with_path_cov(
    src: &str,
    file: &str,
    coverage: bool,
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
    Ok((Codegen::emit_bundle_tests_cov(&bundle, ffi.as_ref(), coverage), ffi))
}

/// D-BENCH1: compile for `jet bench` when the file has `#Bench` blocks —
/// optional `main`, bodies type-checked in `Bench` mode, then lowered to the
/// timing harness.
pub fn compile_benches_with_path(
    file: &str,
) -> Result<(String, Option<FFI::FfiLink>), Vec<Diagnostic>> {
    let mut bundle = Loader::load_entry_with_overlay(file, None, false)?;
    let diags = Sema::check_bundle(&mut bundle, Sema::CompileMode::Bench);
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
    Ok((Codegen::emit_bundle_benches(&bundle, ffi.as_ref()), ffi))
}

/// D-BENCH1: does this entry file declare any `#Bench` blocks? `jet bench`
/// uses per-region timing when it does, and falls back to whole-program timing
/// otherwise. A load failure returns `false` so the caller surfaces the real
/// compile error on its normal path.
pub fn has_bench_blocks(file: &str) -> bool {
    match Loader::load_entry_with_overlay(file, None, false) {
        Ok(bundle) => bundle.modules[bundle.entry]
            .items
            .iter()
            .any(|i| matches!(i, AST::Item::Bench(_))),
        Err(_) => false,
    }
}

/// Does the entry file declare any `#Test` block? `jet test` runs the test
/// harness when it does and skips it (running doctests only) when it doesn't, so
/// a file with only doctests is still testable. A load failure returns `true` so
/// the caller surfaces the real compile error on the normal harness path.
pub fn has_test_blocks(file: &str) -> bool {
    match Loader::load_entry_with_overlay(file, None, false) {
        Ok(bundle) => bundle.modules[bundle.entry]
            .items
            .iter()
            .any(|i| matches!(i, AST::Item::Test(_))),
        Err(_) => true,
    }
}

/// D-COV1: every user function the `jet test --coverage` probes can record, as
/// `(name, 1-based line)`. Mirrors the probe set: free functions, inherent
/// methods, and trait-impl methods in the entry file (`main` is excluded — it is
/// never probed). The runner diffs the recorded hit lines against this set to
/// report per-function / per-line coverage.
pub fn coverable_functions(file: &str) -> Vec<(String, usize)> {
    let bundle = match Loader::load_entry_with_overlay(file, None, false) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let entry = &bundle.modules[bundle.entry];
    let src = &entry.source;
    let line_of = |off: usize| src[..off.min(src.len())].bytes().filter(|&b| b == b'\n').count() + 1;
    let mut out = Vec::new();
    for item in &entry.items {
        match item {
            AST::Item::Func(f) if f.name != "main" => {
                out.push((f.name.clone(), line_of(f.name_span.start)));
            }
            AST::Item::Struct(s) => {
                for m in &s.methods {
                    out.push((format!("{}.{}", s.name, m.name), line_of(m.name_span.start)));
                }
                for b in &s.trait_impls {
                    for m in &b.methods {
                        out.push((format!("{}.{}", s.name, m.name), line_of(m.name_span.start)));
                    }
                }
            }
            AST::Item::Enum(e) => {
                for m in &e.methods {
                    out.push((format!("{}.{}", e.name, m.name), line_of(m.name_span.start)));
                }
                for b in &e.trait_impls {
                    for m in &b.methods {
                        out.push((format!("{}.{}", e.name, m.name), line_of(m.name_span.start)));
                    }
                }
            }
            AST::Item::Impl(i) => {
                for m in &i.methods {
                    out.push((format!("{}.{}", i.type_name, m.name), line_of(m.name_span.start)));
                }
            }
            _ => {}
        }
    }
    out
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
        used_core: std::collections::HashSet::new(),
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
    // c110: capabilities are derived from semantic facts (resolved Core calls,
    // `#Unsafe` gates, FFI declarations), not from scanning the lowered Rust.
    let capabilities = Capabilities::from_sema(
        &bundle.used_core,
        bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
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
pub use Sema::check_pure_program_root;
pub use Comptime::CtValue;

/// Pretty-print source to canonical Jet style (M6/S44).
pub fn format_source(src: &str) -> Result<String, Vec<Diagnostic>> {
    Formatter::format_source(src)
}

/// Front-end check for one document (LSP / editor integration).
pub fn check_document(path: &str, text: &str) -> Vec<Diagnostic> {
    LSP::check_document(path, text)
}

/// S60 / D-PURE1 (E2-M16): evaluate a pure Jet program via the comptime
/// interpreter and return the value `main()` returns as a `CtValue`.
/// Stdout is captured but not returned; callers render with `render_pretty()`
/// (human) or `to_json()` (machine/`--json`).
///
/// Returns `Err` diagnostics (E3401/E0951/E0952/E0953) on failure.
pub fn eval_pure_program_value(src: &str, file: &str) -> Result<CtValue, Vec<Diagnostic>> {
    use std::collections::HashMap;

    let (toks, lex_diags) = Lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let prog = Parser::parse(&toks)?;

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
            "pure evaluation needs a `#Pure fn main()` entry point".to_string(),
            "add `#Pure fn main() { … }` to the program".to_string(),
            None,
        )]
    })?;

    let base_dir = std::path::Path::new(file).parent().unwrap_or(std::path::Path::new("."));
    let mut sink = Comptime::DevSink::new();
    let value = Comptime::run_main_value(main_fn, &func_map, base_dir, &mut sink)
        .map_err(|d| vec![d])?;
    Ok(value)
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
            "pure evaluation needs a `#Pure fn main()` entry point".to_string(),
            "add `#Pure fn main() { … }` to the program".to_string(),
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
