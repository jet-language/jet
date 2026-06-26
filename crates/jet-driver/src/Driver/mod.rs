//! Pipeline composition — the compiler's execution stages assembled in one place.
//!
//! `lib.rs` public functions are thin facades over these. `LSP/Check.rs` calls
//! `check_file` directly for document checking.

use std::path::Path;
use crate::Diagnostics::{Diagnostic, Severity};

/// Main pipeline: load from file path → sema → ffi → codegen.
pub fn compile_bundle_path_opts(
    file: &str,
    mode: crate::Sema::CompileMode,
    freestanding: bool,
    allow_impure: bool,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    let timing = crate::PhaseTiming::enabled();
    let mut timer = crate::PhaseTiming::PhaseTimer::new();
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    if timing {
        timer.lap("load"); // lex + parse + module resolution
    }
    let diags = if freestanding {
        crate::Sema::check_bundle_freestanding(&mut bundle, mode)
    } else if allow_impure {
        crate::Sema::check_bundle_allow_impure(&mut bundle, mode)
    } else {
        crate::Sema::check_bundle(&mut bundle, mode)
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
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    if timing {
        timer.lap("ffi");
    }
    let rust = crate::Codegen::emit_bundle(&bundle, mode, ffi.as_ref());
    if timing {
        timer.lap("codegen");
        timer.metric("rust_bytes", rust.len() as u128);
        timer.write_to(&bundle.project_root);
    }
    // c110: capabilities are derived from semantic facts (resolved Core calls,
    // `#Unsafe` gates, FFI declarations), not from scanning the lowered Rust.
    let capabilities = crate::Capabilities::from_sema(
        &bundle.used_core,
        crate::bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
    let comptime_inputs = std::mem::take(&mut bundle.comptime_inputs);
    Ok(crate::CompileOutput {
        rust,
        lints,
        ffi,
        // Native C link flags are resolved separately at build time (so that
        // codegen / front-end checks never depend on system link discovery);
        // see `resolve_c_links`.
        clinks: Vec::new(),
        capabilities,
        comptime_inputs,
    })
}

/// In-memory pipeline: lex → parse → bundle → sema → ffi → codegen.
pub fn compile_src(
    src: &str,
    file: &str,
    mode: crate::Sema::CompileMode,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    let (toks, lex_diags) = crate::Lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let mut prog = crate::Parser::parse(&toks)?;
    let mut bundle = crate::AST::ProgramBundle {
        entry: 0,
        project_root: std::path::PathBuf::from("."),
        modules: vec![crate::AST::LoadedModule {
            path: std::path::PathBuf::from(file),
            display: file.to_string(),
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            source: src.to_string(),
        }],
        parse_teaching: Vec::new(),
        used_core: std::collections::HashSet::new(),
        cffi: crate::CFFI::CFfi::default(),
        comptime_inputs: Vec::new(),
        import_targets: std::collections::HashMap::new(),
    };
    // S59: fold any in-file C FFI modules + resolve `use c.<lib>` forms.
    bundle.cffi = match crate::CFFI::assemble(&mut bundle) {
        Ok(c) => c,
        Err(diags) => return Err(diags),
    };
    let diags = crate::Sema::check_bundle(&mut bundle, mode);
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
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    let rust = crate::Codegen::emit_bundle(&bundle, mode, ffi.as_ref());
    // c110: capabilities are derived from semantic facts (resolved Core calls,
    // `#Unsafe` gates, FFI declarations), not from scanning the lowered Rust.
    let capabilities = crate::Capabilities::from_sema(
        &bundle.used_core,
        crate::bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
    let comptime_inputs = std::mem::take(&mut bundle.comptime_inputs);
    Ok(crate::CompileOutput {
        rust,
        lints,
        ffi,
        clinks: Vec::new(),
        capabilities,
        comptime_inputs,
    })
}

/// Check-only from file (+ optional in-memory overlay).
///
/// The `overlay` pair is `(canonical_path, text)` — the same shape
/// `Loader::load_entry_with_overlay` expects. Pass `None` for a plain
/// on-disk check; pass `Some((&abs, text))` for an LSP unsaved-buffer check.
/// `is_lsp` is forwarded as the `for_check` flag to the loader.
pub fn check_file(
    file: &str,
    overlay: Option<(&Path, &str)>,
    is_lsp: bool,
) -> (Vec<Diagnostic>, Option<crate::AST::ProgramBundle>) {
    match crate::Loader::load_entry_with_overlay(file, overlay, is_lsp) {
        Ok(mut bundle) => {
            let mut diags = std::mem::take(&mut bundle.parse_teaching);
            diags.extend(crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Check));
            (diags, Some(bundle))
        }
        Err(diags) => (diags, None),
    }
}

/// Check-only from source text (eval mode). Returns only error-severity diagnostics.
pub fn check_eval(src: &str, file: &str) -> Vec<Diagnostic> {
    let (toks, lex_diags) = crate::Lexer::lex(src);
    if !lex_diags.is_empty() {
        return lex_diags;
    }
    let mut prog = match crate::Parser::parse(&toks) {
        Ok(p) => p,
        Err(ds) => return ds,
    };
    let mut bundle = crate::AST::ProgramBundle {
        entry: 0,
        project_root: std::path::PathBuf::from(
            std::path::Path::new(file)
                .parent()
                .unwrap_or(std::path::Path::new(".")),
        ),
        modules: vec![crate::AST::LoadedModule {
            path: std::path::PathBuf::from(file),
            display: file.to_string(),
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            source: src.to_string(),
        }],
        parse_teaching: Vec::new(),
        used_core: std::collections::HashSet::new(),
        cffi: crate::CFFI::CFfi::default(),
        comptime_inputs: Vec::new(),
        import_targets: std::collections::HashMap::new(),
    };
    bundle.cffi = match crate::CFFI::assemble(&mut bundle) {
        Ok(c) => c,
        Err(diags) => return diags,
    };
    let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Eval);
    diags
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Test harness pipeline.
pub fn compile_tests(
    file: &str,
    coverage: bool,
) -> Result<(String, Option<crate::FFI::FfiLink>), Vec<Diagnostic>> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Test);
    let mut errors = Vec::new();
    for d in diags {
        if d.severity == Severity::Error {
            errors.push(d);
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    Ok((
        crate::Codegen::emit_bundle_tests_cov(&bundle, ffi.as_ref(), coverage),
        ffi,
    ))
}

/// Bench pipeline.
pub fn compile_benches(
    file: &str,
) -> Result<(String, Option<crate::FFI::FfiLink>), Vec<Diagnostic>> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Bench);
    let mut errors = Vec::new();
    for d in diags {
        if d.severity == Severity::Error {
            errors.push(d);
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    Ok((crate::Codegen::emit_bundle_benches(&bundle, ffi.as_ref()), ffi))
}
