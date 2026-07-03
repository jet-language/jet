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

// Seam crates — re-export everything so callers use `jet::AST`, `jet::Sema`, etc.
// unchanged. Within Source/, `crate::AST` etc. resolve through these re-exports.
pub use jet_driver::{
    // Top-level re-exports from Compile module:
    bundle_uses_unsafe,
    CBind,
    Capabilities,
    Codegen,
    Collections,
    Compile,
    CompileOutput,
    Comptime,
    Diagnostics,
    Driver,
    Formatter,
    Generics,
    Jetpack,
    Lexer,
    Loader,
    Lock,
    Manifest,
    Parser,
    PhaseTiming,
    Sema,
    Syntax,
    Traits,
    AST,
    CFFI,
    FFI,
    SHA256,
};
pub mod BuildCache;
pub mod CLI;
pub mod Debug;
pub mod Doctest;
pub mod Doctor;
pub mod ExitCodes;
pub mod Explain;
pub mod Fetch;
pub mod FixEngine;
pub mod Interpreter;
pub mod JitBackend;
pub mod LSP;
pub mod Publish;
pub mod REPL;
pub mod Store;

use Diagnostics::Diagnostic;

/// Run the full front end on source text. All lex errors (then all parse
/// errors) surface in one run — M1 error recovery.
pub fn compile(src: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_with_mode(src, "input.jet", Sema::CompileMode::Run)
}

pub fn compile_with_path(src: &str, file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    let _ = src;
    compile_bundle_path(file, Sema::CompileMode::Run, None)
}

/// Like `compile_with_path`, but threads a `--target=<triple>` (or `None`)
/// through to codegen's native OS-target gating (D-OSTARGET1=A, ratified
/// 2026-07-01, c134) — an `impl` gated to a different `#Target(Os.*)` than
/// the resolved active OS is skipped entirely. `jet build`/`jet run`'s real
/// `--target=` flag is the only caller; `compile_with_path` keeps its
/// existing host-OS-default behavior unchanged for every other caller.
pub fn compile_with_target(
    src: &str,
    file: &str,
    cross_target: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    let _ = src;
    compile_bundle_path(file, Sema::CompileMode::Run, cross_target)
}

/// Front-end check for a file on disk (and its imports). Library modules
/// need not define `main`; use `compile_with_path` when building or running.
pub fn check_with_path(file: &str) -> Vec<Diagnostic> {
    let (diags, _) = Driver::check_file(file, None, true);
    diags
}

/// Full sema type-check for `jet eval`: runs the same pipeline as `compile`
/// but with `CompileMode::Eval` so E0122 (main must return `()`) is relaxed
/// while all other diagnostics (type errors, unknown identifiers, etc.) still
/// fire. Returns the error diagnostics, or an empty vec on success.
pub fn check_for_eval(src: &str, file: &str) -> Vec<Diagnostic> {
    Driver::check_eval(src, file)
}

fn compile_bundle_path(
    file: &str,
    mode: Sema::CompileMode,
    cross_target: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts(file, mode, false, false, false, cross_target)
}

/// Like `compile_with_path` but with `--freestanding` mode (E2-M15).
/// Rejects OS-dependent std APIs (E3301) and emits `panic = "abort"` hint.
pub fn compile_freestanding(file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts(file, Sema::CompileMode::Run, true, false, false, None)
}

/// Like `compile_with_path` but with `--allow-impure` (D-CTEFFECT1).
/// Enables Tier-2 ambient comptime effects inside `#Impure` gates.
pub fn compile_allow_impure(file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts(file, Sema::CompileMode::Run, false, true, false, None)
}

fn compile_bundle_path_opts(
    file: &str,
    mode: Sema::CompileMode,
    freestanding: bool,
    allow_impure: bool,
    web_target: bool,
    cross_target: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    Driver::compile_bundle_path_opts(
        file,
        mode,
        freestanding,
        allow_impure,
        web_target,
        cross_target,
    )
}

/// Like `compile_with_path` but for `jet build --target=web` (D-WEBBACKEND1 M2).
pub fn compile_web(file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts(file, Sema::CompileMode::Run, false, false, true, None)
}

/// D-DBG3 step 2 (dap-debugger): compile for the native `jet debug` backend — a
/// normal build with `debug_linemap = true`, so the generated Rust carries the
/// `// jet:line N` table `Source/Debug/LineMap.rs` reads back.
pub fn compile_for_debug(file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    Driver::compile_bundle_path_opts_dbg(
        file,
        Sema::CompileMode::Run,
        false,
        false,
        false,
        true,
        None,
    )
}

/// c-devserver (owner-directed 2026-07-01): `jet dev <file>` when `file`
/// defines a top-level `fn dev()` — a normal native compile, but with `dev()`
/// swapped in as the program's real entry point instead of `main()` (see
/// `Driver::compile_bundle_path_with_entry`).
pub fn compile_with_entry(file: &str, entry_fn: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    Driver::compile_bundle_path_with_entry(file, entry_fn)
}

/// In-memory web-target compile (used by integration tests).
pub fn compile_web_with_path(src: &str, file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
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
            web_target_ceiling: prog.web_target_ceiling,
            pub_file: prog.pub_file,
            html_path: prog.html_path.clone(),
        }],
        parse_teaching: Vec::new(),
        used_core: std::collections::HashSet::new(),
        cffi: CFFI::CFfi::default(),
        comptime_inputs: Vec::new(),
        import_targets: std::collections::HashMap::new(),
        layer_ceiling: None,
        inferred_layer: Syntax::RuntimeLayer::Core,
        web_partitions: std::collections::HashMap::new(),
        web_partition_enforced: true,
        web_partition_report: None,
        dep_roots: std::collections::HashMap::new(),
    };
    bundle.cffi = match CFFI::assemble(&mut bundle) {
        Ok(c) => c,
        Err(diags) => return Err(diags),
    };
    let diags = Sema::check_bundle(&mut bundle, Sema::CompileMode::Run);
    let mut errors = Vec::new();
    let mut lints = Vec::new();
    for d in diags {
        match d.severity {
            Diagnostics::Severity::Error => errors.push(d),
            Diagnostics::Severity::Lint => lints.push(d),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi = match FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    let rust = Codegen::emit_bundle(&bundle, Sema::CompileMode::Run, ffi.as_ref());
    let web = Some(Codegen::emit_web(
        &bundle,
        Sema::CompileMode::Run,
        ffi.as_ref(),
    ));
    let capabilities = Capabilities::from_sema(
        &bundle.used_core,
        bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
    let comptime_inputs = std::mem::take(&mut bundle.comptime_inputs);
    Ok(CompileOutput {
        rust,
        lints,
        ffi,
        clinks: Vec::new(),
        capabilities,
        comptime_inputs,
        web,
        web_partition_report: bundle.web_partition_report.clone(),
        inferred_layer: bundle.inferred_layer,
        layer_ceiling: bundle.layer_ceiling,
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
    crate::CFFI::rustc_link_args(&bundle.cffi, &bundle.project_root)
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
    Driver::compile_tests(file, coverage)
}

/// D-BENCH1: compile for `jet bench` when the file has `#Bench` blocks —
/// optional `main`, bodies type-checked in `Bench` mode, then lowered to the
/// timing harness.
pub fn compile_benches_with_path(
    file: &str,
) -> Result<(String, Option<FFI::FfiLink>), Vec<Diagnostic>> {
    Driver::compile_benches(file)
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
    let line_of = |off: usize| {
        src[..off.min(src.len())]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1
    };
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
                    out.push((
                        format!("{}.{}", i.type_name, m.name),
                        line_of(m.name_span.start),
                    ));
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
    Driver::compile_src(src, file, mode)
}

/// Back-compat: compile and return only Rust (drops lints).
pub fn compile_rust(src: &str) -> Result<String, Vec<Diagnostic>> {
    compile(src).map(|o| o.rust)
}

pub use Comptime::CtValue;
pub use Diagnostics::render_all as render_diagnostics;
pub use Diagnostics::{render_all_colored, render_all_json, render_all_linked};
pub use Sema::check_pure_program_root;

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
            "pure evaluation needs a `@Pure fn main()` entry point".to_string(),
            "add `@Pure fn main() { … }` to the program".to_string(),
            None,
        )]
    })?;

    let base_dir = std::path::Path::new(file)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let mut sink = Comptime::DevSink::new();
    let value =
        Comptime::run_main_value(main_fn, &func_map, base_dir, &mut sink).map_err(|d| vec![d])?;
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
            "pure evaluation needs a `@Pure fn main()` entry point".to_string(),
            "add `@Pure fn main() { … }` to the program".to_string(),
            None,
        )]
    })?;

    // Run main() via the comptime engine with a dev sink capturing print output.
    let base_dir = std::path::Path::new(file)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let mut sink = Comptime::DevSink::new();
    Comptime::run_main(main_fn, &func_map, base_dir, &mut sink).map_err(|d| vec![d])?;
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
