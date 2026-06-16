//! jet — compiler library.
//!
//! Pipeline: lex -> parse -> sema -> codegen (docs/spec/architecture.md).
//! The front end (everything before codegen) owns ALL user-facing
//! correctness and every diagnostic. The Rust backend is a verifier and
//! optimizer, never a source of user-facing errors.

pub mod ast;
pub mod build_cache;
pub mod cffi;
pub mod cli_spec;
pub mod codegen;
pub mod collections;
pub mod comptime;
pub mod diag;
pub mod diagjson;
pub mod doctor;
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

/// Render diagnostics as machine-readable JSON Lines (D-DX1, E2-M3): one
/// versioned JSON object per diagnostic, never any ANSI. The shared serializer
/// lives in `diagjson` so `jet fix` and the LSP reuse the exact same schema.
pub use diagjson::render_all_json as render_diagnostics_json;

/// Render diagnostics for the CLI, with the E2-M3 "learn more" footer.
///
/// When `color` is true (an attached terminal, not piped/NO_COLOR — see
/// `stderr_color` in the driver), each rendered diagnostic gains one dim
/// trailing line pointing at `jet explain <code>`. When `color` is false the
/// output is byte-for-byte `render_diagnostics`, so piped/CI/golden output is
/// unchanged and existing snapshots stay green.
pub fn render_diagnostics_cli(
    file: &str,
    src: &str,
    diags: &[Diagnostic],
    color: bool,
) -> String {
    if !color {
        return diag::render_all(file, src, diags);
    }
    // Dim each diagnostic's footer; one unobtrusive line per diagnostic. When
    // color is on (an attached terminal), also weave OSC-8 hyperlinks (D-DX6)
    // into the location line and the error code so a click jumps to the source
    // or to `jet explain`. Hyperlinks ride the same gate as color, so piped /
    // NO_COLOR / `--color=never` output is byte-identical to `render_all`.
    let links = osc8_supported();
    let blocks: Vec<String> = diags
        .iter()
        .map(|d| {
            let mut body = d.render(file, src);
            if links {
                body = link_diagnostic(&body, file, src, d);
            }
            format!(
                "{body}\x1b[2mrun `{bin} explain {code}` to learn more\x1b[0m\n",
                bin = syntax::BINARY_NAME,
                code = d.code,
            )
        })
        .collect();
    blocks.join("\n")
}

/// True when OSC-8 hyperlinks should be emitted. This is *gated on top of the
/// already-resolved color decision* (callers only reach here with color on), so
/// the only extra signal we honour is an explicit opt-out: a terminal that
/// advertises no hyperlink support. We keep it conservative and env-driven
/// rather than probing terminfo (std-only, I6). `FORCE_COLOR` (used by tests and
/// CI-style "always color" runs) implies links so the feature is testable
/// without a real TTY.
fn osc8_supported() -> bool {
    // An explicit kill switch wins (lets users disable links while keeping color).
    if std::env::var_os("NO_HYPERLINKS").is_some() {
        return false;
    }
    true
}

/// Build an OSC-8 hyperlink: `ESC ] 8 ; ; URI ST  TEXT  ESC ] 8 ; ; ST`.
/// We use BEL (`\x07`) as the string terminator for the widest compatibility.
fn osc8(uri: &str, text: &str) -> String {
    format!("\x1b]8;;{uri}\x07{text}\x1b]8;;\x07")
}

/// Rewrite a rendered diagnostic so the `--> file:line:col` location becomes a
/// `file://` link to the source and the `[CODE]` token links to a `jet explain`
/// affordance. Only the link wrappers are inserted; the visible characters are
/// unchanged, so a terminal that ignores OSC-8 shows identical text.
fn link_diagnostic(body: &str, file: &str, src: &str, d: &diag::Diagnostic) -> String {
    let mut out = body.to_string();

    // 1) The error code in the header line: `Error [E0102]: ...`. Link the code
    //    token (with brackets) to a `jet explain` helper URL so a click teaches.
    let code = d.code;
    let needle = format!("[{code}]");
    let explain_uri = format!("{}:{}", syntax::BINARY_NAME, code); // e.g. `jet:E0102`
    let linked_code = osc8(&explain_uri, &needle);
    out = out.replacen(&needle, &linked_code, 1);

    // 2) The location line: `  --> file:line:col`. Link `file:line:col` to a
    //    `file://` URI with a `:line:col` fragment many terminals understand.
    if let Some(span) = d.span {
        let (line, col) = diag::span_line_col(src, span.start);
        let loc = format!("{file}:{line}:{col}");
        let abs = std::fs::canonicalize(file)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| file.to_string());
        let uri = format!("file://{abs}#{line}:{col}");
        let linked_loc = osc8(&uri, &loc);
        out = out.replacen(&loc, &linked_loc, 1);
    }
    out
}

/// Pretty-print source to canonical Jet style (M6/S44).
pub fn format_source(src: &str) -> Result<String, Vec<Diagnostic>> {
    fmt::format_source(src)
}

/// Front-end check for one document (LSP / editor integration).
pub fn check_document(path: &str, text: &str) -> Vec<Diagnostic> {
    lsp::check_document(path, text)
}
