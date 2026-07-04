//! Pipeline composition — the compiler's execution stages assembled in one place.
//!
//! `lib.rs` public functions are thin facades over these. `LSP/Check.rs` calls
//! `check_file` directly for document checking.

use crate::Diagnostics::{Diagnostic, Severity};
use std::path::Path;

/// Main pipeline: load from file path → sema → ffi → codegen.
///
/// D-OSTARGET1=A (ratified 2026-07-01, c134): `cross_target` is the raw
/// `--target=<triple>` string (or `None`) — reused as-is from the existing
/// E2-M15 cross-compile flag, resolved to a native OS bucket in
/// `compile_bundle_path_opts_dbg` (host OS when `None` or unrecognized).
pub fn compile_bundle_path_opts(
    file: &str,
    mode: crate::Sema::CompileMode,
    freestanding: bool,
    allow_impure: bool,
    web_target: bool,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(
        file,
        mode,
        freestanding,
        allow_impure,
        web_target,
        false,
        false,
        cross_target,
    )
}

/// Like `compile_bundle_path_opts`, but for `jet build --target=plugin`
/// (D-PLUGIN1=B / D-DEP-WASM1=A, c81): also emits the guest `.wit` + wasm32
/// Rust artifacts (`Codegen::emit_plugin`).
pub fn compile_bundle_path_opts_plugin(
    file: &str,
    mode: crate::Sema::CompileMode,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(file, mode, false, false, false, true, false, cross_target)
}

/// Like `compile_bundle_path_opts`, but `debug_linemap = true` routes codegen
/// through `emit_bundle_dbg` (D-DBG3 step 2 / dap-debugger): every generated
/// statement gets a `// jet:line N` marker the native `jet debug` backend reads
/// back into a rust-line -> jet-line table. Used ONLY by the native debug build
/// path — every other caller keeps `debug_linemap = false` (byte-identical output).
pub fn compile_bundle_path_opts_dbg(
    file: &str,
    mode: crate::Sema::CompileMode,
    freestanding: bool,
    allow_impure: bool,
    web_target: bool,
    debug_linemap: bool,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(
        file,
        mode,
        freestanding,
        allow_impure,
        web_target,
        false,
        debug_linemap,
        cross_target,
    )
}

/// The real implementation behind every `compile_bundle_path_opts*` facade —
/// see `compile_bundle_path_opts` (native) / `compile_bundle_path_opts_dbg`
/// (native debug) / `compile_bundle_path_opts_plugin` (c81 plugin guest) for
/// the public entry points.
fn compile_bundle_path_opts_full(
    file: &str,
    mode: crate::Sema::CompileMode,
    freestanding: bool,
    allow_impure: bool,
    web_target: bool,
    plugin_target: bool,
    debug_linemap: bool,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    // D-OSTARGET1=A: resolve the active native OS bucket once, from the same
    // `--target=<triple>` flag E2-M15 already threads through (host OS when
    // absent or unrecognized, e.g. a wasm/web pseudo-target).
    let active_os = crate::Syntax::OsTarget::active(cross_target);
    let timing = crate::PhaseTiming::enabled();
    let mut timer = crate::PhaseTiming::PhaseTimer::new();
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    // D-OSTARGET2=B: the `comptime if build.os == { … }` desugar (run in sema)
    // must fold to the same OS bucket codegen filters `impl`s by, so seed the
    // bundle from the same resolved `active_os` as `emit_bundle`.
    bundle.active_os = active_os;
    if web_target {
        bundle.web_partition_enforced = true;
    }
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
    // U11 (D-JPK-SCRIPTDEP1=A) and any other loader-time teaching diagnostic
    // (`bundle.parse_teaching`) ride the same errors/lints split as sema's —
    // `check_file` already does this for `jet check`/LSP; `jet run`/`build`
    // was dropping them on the floor (parse_teaching had no active producer
    // before U11's L0203, so the gap went unnoticed).
    let mut errors = Vec::new();
    let mut lints = Vec::new();
    for d in std::mem::take(&mut bundle.parse_teaching).into_iter().chain(diags) {
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
    let rust = crate::Codegen::emit_bundle_dbg(&bundle, ffi.as_ref(), debug_linemap, active_os);
    let web = if web_target {
        Some(crate::Codegen::emit_web(&bundle, mode, ffi.as_ref()))
    } else {
        None
    };
    // D-PLUGIN1=B / D-DEP-WASM1=A / D-PLUGIN-EXPORT1=A (c81): the guest side of
    // a `target: plugin` build — a `.wit` world + wasm32 guest Rust, generated
    // from the entry module's exportable (`Int`/`Float`-only) `pub fn`s.
    let plugin = if plugin_target {
        // E1260: every `pub fn` in the entry module must be exportable —
        // never a silent skip (I3/I4).
        let surface_errors = crate::Jetpack::PluginExport::validate_export_surface(&bundle);
        if !surface_errors.is_empty() {
            return Err(surface_errors);
        }
        let export_name = crate::Jetpack::PluginExport::resolve_export_name(&bundle);
        // D-PLUGIN-VERSION1=A: freeze/diff the exported interface (E1257 on an
        // incompatible change) before handing artifacts to the wasm build step.
        crate::Jetpack::PluginExport::check_and_freeze_version(&bundle, &export_name)?;
        Some(crate::Codegen::emit_plugin(&bundle, &rust, &export_name))
    } else {
        None
    };
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
    if let Some(mf) = crate::Manifest::load(&bundle.project_root).and_then(|r| r.ok()) {
        crate::Lock::record_inferred_layer(
            &bundle.project_root,
            &mf.package.name,
            bundle.inferred_layer,
        );
    }
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
        web,
        web_partition_report: bundle.web_partition_report.clone(),
        plugin,
        inferred_layer: bundle.inferred_layer,
        layer_ceiling: bundle.layer_ceiling,
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
            web_target_ceiling: prog.web_target_ceiling,
            pub_file: prog.pub_file,
            html_path: prog.html_path.clone(),
        }],
        parse_teaching: Vec::new(),
        used_core: std::collections::HashSet::new(),
        cffi: crate::CFFI::CFfi::default(),
        comptime_inputs: Vec::new(),
        import_targets: std::collections::HashMap::new(),
        layer_ceiling: None,
        inferred_layer: crate::Syntax::RuntimeLayer::Core,
        web_partitions: std::collections::HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots: std::collections::HashMap::new(),
        active_os: crate::Syntax::OsTarget::host(),
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
        web: None,
        web_partition_report: bundle.web_partition_report.clone(),
        plugin: None,
        inferred_layer: bundle.inferred_layer,
        layer_ceiling: bundle.layer_ceiling,
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
    let (diags, bundle, _facts) = check_file_with_effect_facts(file, overlay, is_lsp);
    (diags, bundle)
}

/// Like `check_file` but also returns effect facts for D-SEMINDEX1.
pub fn check_file_with_effect_facts(
    file: &str,
    overlay: Option<(&Path, &str)>,
    is_lsp: bool,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    match crate::Loader::load_entry_with_overlay(file, overlay, is_lsp) {
        Ok(mut bundle) => {
            let mut diags = std::mem::take(&mut bundle.parse_teaching);
            let (check_diags, facts) = crate::Sema::check_bundle_with_effect_facts(
                &mut bundle,
                crate::Sema::CompileMode::Check,
            );
            diags.extend(check_diags);
            (diags, Some(bundle), facts)
        }
        Err(diags) => (diags, None, crate::Sema::SemIndexEffectFacts::default()),
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
            web_target_ceiling: prog.web_target_ceiling,
            pub_file: prog.pub_file,
            html_path: prog.html_path.clone(),
        }],
        parse_teaching: Vec::new(),
        used_core: std::collections::HashSet::new(),
        cffi: crate::CFFI::CFfi::default(),
        comptime_inputs: Vec::new(),
        import_targets: std::collections::HashMap::new(),
        layer_ceiling: None,
        inferred_layer: crate::Syntax::RuntimeLayer::Core,
        web_partitions: std::collections::HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots: std::collections::HashMap::new(),
        active_os: crate::Syntax::OsTarget::host(),
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

/// c-devserver (owner-directed 2026-07-01): `jet dev <file>` when the file
/// defines a top-level `fn dev()` — compiles NATIVELY with `dev()` swapped in
/// as the program's real entry point instead of `run()`. Mechanically: an
/// AST-level rename before sema ever runs (I3: codegen stays dumb; sema never
/// special-cases any entry name other than `"run"` — see
/// `Registration.rs`/`Bundle.rs`'s `funcs.get("run")` checks. The function
/// literally named `entry_fn` becomes literally named `run`; whatever was
/// previously named `run` (if anything) is renamed to a collision-free name
/// first, so a file with both `fn run()` and `fn dev()` never has two entry
/// candidates.
/// Native only — never freestanding/impure/web (those toggles don't apply to
/// the `fn dev()` entry path; a `dev()` function's job is to configure and run
/// an ordinary value like `core.devserver`, nothing more).
pub fn compile_bundle_path_with_entry(
    file: &str,
    entry_fn: &str,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    swap_entry_point(&mut bundle, entry_fn);
    let mode = crate::Sema::CompileMode::Run;
    let diags = crate::Sema::check_bundle(&mut bundle, mode);
    // U11 (D-JPK-SCRIPTDEP1=A): see the matching comment in
    // `compile_bundle_path_opts_dbg` — `parse_teaching` rides along here too.
    let mut errors = Vec::new();
    let mut lints = Vec::new();
    for d in std::mem::take(&mut bundle.parse_teaching).into_iter().chain(diags) {
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
    // D-OSTARGET1=A: `jet dev`'s entry-swap path never cross-compiles — host OS.
    let rust = crate::Codegen::emit_bundle_dbg(
        &bundle,
        ffi.as_ref(),
        false,
        crate::Syntax::OsTarget::host(),
    );
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
        web: None,
        web_partition_report: bundle.web_partition_report.clone(),
        plugin: None,
        inferred_layer: bundle.inferred_layer,
        layer_ceiling: bundle.layer_ceiling,
    })
}

/// Rename the function literally named `entry_fn` in the entry module to
/// `run`, first moving any pre-existing `run` out of the way. A no-op when
/// `entry_fn` is already `"run"`.
fn swap_entry_point(bundle: &mut crate::AST::ProgramBundle, entry_fn: &str) {
    if entry_fn == "run" {
        return;
    }
    let items = &mut bundle.modules[bundle.entry].items;
    for item in items.iter_mut() {
        if let crate::AST::Item::Func(f) = item {
            if f.name == "run" {
                f.name = "__jet_unused_run".to_string();
            }
        }
    }
    for item in items.iter_mut() {
        if let crate::AST::Item::Func(f) = item {
            if f.name == entry_fn {
                f.name = "run".to_string();
            }
        }
    }
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
    Ok((
        crate::Codegen::emit_bundle_benches(&bundle, ffi.as_ref()),
        ffi,
    ))
}
