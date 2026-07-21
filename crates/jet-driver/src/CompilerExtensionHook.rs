//! Post-sema compiler-extension hook (D-DX5-HOOK1=A / Tower #549).
//!
//! When `JET_COMPILER_EXTENSION` names a `compiler-extension-v1` `.wasm`,
//! the driver freezes a typed read-only snapshot after sema, runs the
//! jet-pkg-model wasmtime host, validates the response, and maps findings
//! to Jet diagnostics. No new user syntax — env registration only until a
//! spelling ballot. Failures are Jet-owned (E1402); guests never crash the
//! compiler or expose rustc (I2/I3).

use crate::AST::{Func, Item, ProgramBundle};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::SemIndexEffectFacts;
use jet_pkg_model::CompilerExtension::{
    self, message_exposes_rustc, AnalyzeResponse, Capability, Finding, SpanFact, SymbolFact,
    TypeFact, TypedSnapshot, ENV_COMPILER_EXTENSION,
};

/// Run configured compiler-extension(s) after a successful sema pass.
/// Returns diagnostics to merge with the check/compile result (empty when
/// the env var is unset).
///
/// `effect_facts` carries solved post-sema effects when the caller ran
/// `check_bundle_with_effect_facts`. When absent, `ReadEffects` is omitted
/// from advertised capabilities — never invent `"pure"` or other placeholders.
pub fn post_sema_diagnostics(
    bundle: &ProgramBundle,
    effect_facts: Option<&SemIndexEffectFacts>,
) -> Vec<Diagnostic> {
    let Ok(path) = std::env::var(ENV_COMPILER_EXTENSION) else {
        return Vec::new();
    };
    let path = path.trim();
    if path.is_empty() {
        return Vec::new();
    }

    let snapshot = match snapshot_from_bundle(bundle, effect_facts) {
        Ok(s) => s,
        Err(e) => return vec![host_failure(&e.message, None)],
    };

    match CompilerExtension::analyze_wasm_component(path, &snapshot) {
        Ok(response) => findings_to_diagnostics(&snapshot, &response),
        Err(e) => {
            let msg = sanitize_host_message(&e.message);
            vec![host_failure(&msg, None)]
        }
    }
}

fn sanitize_host_message(message: &str) -> String {
    if message_exposes_rustc(message) {
        return "compiler-extension failed with an internal message".into();
    }
    // Drop volatile wasmtime backtraces from user-facing copy (I4 snapshots).
    let first = message.lines().next().unwrap_or(message).trim();
    let trimmed = first
        .split("error while executing at wasm backtrace")
        .next()
        .unwrap_or(first)
        .trim()
        .trim_end_matches(':')
        .trim();
    if trimmed.is_empty() {
        "compiler-extension analyze trapped".into()
    } else {
        trimmed.to_string()
    }
}

/// Build a deterministic v1 snapshot from entry-module function symbols.
///
/// Types come from the post-sema AST signature. Effects come from solved
/// `SemIndexEffectFacts` when provided; otherwise `ReadEffects` is not
/// advertised and symbol `effects` stay empty.
pub fn snapshot_from_bundle(
    bundle: &ProgramBundle,
    effect_facts: Option<&SemIndexEffectFacts>,
) -> Result<TypedSnapshot, CompilerExtension::ProtocolError> {
    let module = &bundle.modules[bundle.entry];
    let file = module.display.clone();
    let mut capabilities = Capability::v1_defaults().to_vec();
    if effect_facts.is_none() {
        capabilities.retain(|c| *c != Capability::ReadEffects);
    }
    let mut types = Vec::new();
    let mut symbols = Vec::new();
    let mut spans = Vec::new();
    let mut n = 0u32;
    for item in &module.items {
        let Item::Func(func) = item else {
            continue;
        };
        n += 1;
        let tid = format!("t{n}");
        let sid = format!("s{n}");
        let spid = format!("sp{n}");
        types.push(TypeFact {
            id: tid.clone(),
            repr: fn_type_repr(func),
        });
        let effects = effect_facts
            .and_then(|facts| facts.solved.get(&format!("{}::{}", module.alias, func.name)))
            .map(|set| set.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        symbols.push(SymbolFact {
            id: sid,
            name: func.name.clone(),
            kind: "fn".into(),
            type_id: tid,
            span_id: spid.clone(),
            effects,
            provenance: "sema".into(),
        });
        spans.push(SpanFact {
            id: spid,
            file: file.clone(),
            start: func.name_span.start as u32,
            end: func.name_span.end as u32,
        });
    }
    if symbols.is_empty() {
        // Guests may still run; provide a file-level span so span_id refs can resolve.
        // No invented type/effect facts — drop read caps that would lie.
        capabilities.retain(|c| {
            *c != Capability::ReadTypes
                && *c != Capability::ReadSymbols
                && *c != Capability::ReadEffects
        });
        types.clear();
        spans.push(SpanFact {
            id: "sp1".into(),
            file,
            start: 0,
            end: 0,
        });
    }
    TypedSnapshot::new(capabilities, types, symbols, spans)
}

fn fn_type_repr(func: &Func) -> String {
    let params = func
        .params
        .iter()
        .map(|p| format!("{}{}: {}", p.convention.sigil(), p.name, p.ty.name()))
        .collect::<Vec<_>>()
        .join(", ");
    match &func.return_type {
        Some(ret) => format!("fn({params}) -> {}", ret.name()),
        None => format!("fn({params})"),
    }
}

fn findings_to_diagnostics(
    snapshot: &TypedSnapshot,
    response: &AnalyzeResponse,
) -> Vec<Diagnostic> {
    let mut out = Vec::with_capacity(response.findings.len());
    for finding in &response.findings {
        out.push(finding_to_diagnostic(snapshot, finding));
    }
    out
}

fn finding_to_diagnostic(snapshot: &TypedSnapshot, finding: &Finding) -> Diagnostic {
    let span = snapshot
        .spans
        .iter()
        .find(|s| s.id == finding.span_id)
        .map(|s| Span::new(s.start as usize, s.end as usize));
    let what = format!(
        "compiler-extension `{}` ({}): {}",
        finding.rule, finding.severity, finding.message
    );
    let why = "a configured compiler-extension component reported this finding after type checking (D-DX5-HOOK1)".to_string();
    let fix = "address the finding, or unset JET_COMPILER_EXTENSION to skip the extension".to_string();
    // V1 maps every guest finding to a lint (L1401). Teams wall via
    // `policy.lints.deny` (D-LINTPOLICY1).
    Diagnostic::lint("L1401", what, why, fix, span)
}

fn host_failure(message: &str, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E1402",
        format!("compiler-extension failed: {message}"),
        "the configured compiler-extension component could not complete analyze, or returned an invalid response (D-DX5-HOOK1)".to_string(),
        "fix the component, or unset JET_COMPILER_EXTENSION to skip the extension".to_string(),
        span,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn fixture_wasm(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../jet-pkg-model/fixtures/compiler_extension")
            .join(name)
    }

    #[test]
    fn post_sema_custom_lint_from_driver_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile_dir();
        let src_path = dir.join("main.jet");
        std::fs::write(
            &src_path,
            "fn run() {\n    print(1)\n}\n",
        )
        .unwrap();
        let wasm = fixture_wasm("lint_no_x.wasm");
        std::env::set_var(ENV_COMPILER_EXTENSION, wasm.to_str().unwrap());
        let (diags, bundle, _) =
            crate::Driver::check_file_with_effect_facts(src_path.to_str().unwrap(), None, false);
        std::env::remove_var(ENV_COMPILER_EXTENSION);
        assert!(bundle.is_some(), "sema must succeed; diags={diags:?}");
        let lint = diags
            .iter()
            .find(|d| d.code == "L1401")
            .unwrap_or_else(|| panic!("custom-lint finding must surface as L1401; diags={diags:?}"));
        assert!(lint.what.contains("no-x"), "got {}", lint.what);
        assert!(lint.what.contains("prefer y"), "got {}", lint.what);
        assert!(!message_exposes_rustc(&lint.what));
        assert!(!message_exposes_rustc(&lint.why));
    }

    #[test]
    fn post_sema_crash_guest_fail_closed_e1402() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile_dir();
        let src_path = dir.join("main.jet");
        std::fs::write(&src_path, "fn run() {\n    print(1)\n}\n").unwrap();
        let wasm = fixture_wasm("crash.wasm");
        std::env::set_var(ENV_COMPILER_EXTENSION, wasm.to_str().unwrap());
        let (diags, bundle, _) =
            crate::Driver::check_file_with_effect_facts(src_path.to_str().unwrap(), None, false);
        std::env::remove_var(ENV_COMPILER_EXTENSION);
        assert!(bundle.is_some());
        let err = diags
            .iter()
            .find(|d| d.code == "E1402")
            .expect("crash must become E1402");
        assert!(!message_exposes_rustc(&err.what));
        assert!(!message_exposes_rustc(&err.why));
        assert!(!diags.iter().any(|d| d.code == "L1401"));
    }

    #[test]
    fn snapshot_uses_solved_effects_not_invented_pure() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(ENV_COMPILER_EXTENSION);
        let dir = tempfile_dir();
        let src_path = dir.join("main.jet");
        std::fs::write(&src_path, "fn run() {\n    print(1)\n}\n").unwrap();
        let (diags, bundle, facts) =
            crate::Driver::check_file_with_effect_facts(src_path.to_str().unwrap(), None, false);
        assert!(
            !diags.iter().any(|d| d.severity == crate::Diagnostics::Severity::Error),
            "sema must succeed; diags={diags:?}"
        );
        let bundle = bundle.expect("bundle");
        let snap = snapshot_from_bundle(&bundle, Some(&facts)).expect("snapshot");
        assert!(
            snap.capabilities.contains(&Capability::ReadEffects),
            "facts present → advertise ReadEffects"
        );
        let run = snap
            .symbols
            .iter()
            .find(|s| s.name == "run")
            .expect("run symbol");
        assert!(
            !run.effects.iter().any(|e| e == "pure"),
            "must not invent effect name `pure`; got {:?}",
            run.effects
        );
        assert!(
            run.effects.iter().any(|e| e == "Io" || e == "Log"),
            "print must solve to a real effect; got {:?}",
            run.effects
        );
        assert!(
            snap.types.iter().any(|t| t.id == run.type_id && t.repr.starts_with("fn(")),
            "type_id must point at a real fn signature repr; types={:?}",
            snap.types
        );
    }

    /// `compile_bundle_path_opts_full` / entry-swap pass `None` facts: omit
    /// `ReadEffects` and leave symbol effects empty — never invent `"pure"`.
    #[test]
    fn snapshot_without_effect_facts_omits_read_effects() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(ENV_COMPILER_EXTENSION);
        let dir = tempfile_dir();
        let src_path = dir.join("main.jet");
        std::fs::write(&src_path, "fn run() {\n    print(1)\n}\n").unwrap();
        let (diags, bundle, _) =
            crate::Driver::check_file_with_effect_facts(src_path.to_str().unwrap(), None, false);
        assert!(
            !diags.iter().any(|d| d.severity == crate::Diagnostics::Severity::Error),
            "sema must succeed; diags={diags:?}"
        );
        let bundle = bundle.expect("bundle");
        let snap = snapshot_from_bundle(&bundle, None).expect("snapshot");
        assert!(
            !snap.capabilities.contains(&Capability::ReadEffects),
            "None facts must omit ReadEffects; caps={:?}",
            snap.capabilities
        );
        for sym in &snap.symbols {
            assert!(
                sym.effects.is_empty(),
                "None facts → empty effects (no invented pure); {:?} got {:?}",
                sym.name,
                sym.effects
            );
            assert!(
                !sym.effects.iter().any(|e| e == "pure"),
                "must not invent `pure`"
            );
        }
    }

    /// opts_full (`jet run` compile path) still runs the hook with `None` facts.
    #[test]
    fn compile_opts_full_crash_guest_fail_closed_e1402() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile_dir();
        let src_path = dir.join("main.jet");
        std::fs::write(&src_path, "fn run() {\n    print(1)\n}\n").unwrap();
        let wasm = fixture_wasm("crash.wasm");
        std::env::set_var(ENV_COMPILER_EXTENSION, wasm.to_str().unwrap());
        let err = crate::Driver::compile_bundle_path_opts(
            src_path.to_str().unwrap(),
            crate::Sema::CompileMode::Run,
            false,
            false,
            false,
            None,
        )
        .expect_err("crash guest must fail closed on opts_full");
        std::env::remove_var(ENV_COMPILER_EXTENSION);
        assert!(
            err.iter().any(|d| d.code == "E1402"),
            "opts_full None-facts path must still surface E1402; got {err:?}"
        );
    }

    /// Entry-swap (`jet dev` / `--task`) still runs the hook with `None` facts.
    #[test]
    fn compile_entry_swap_crash_guest_fail_closed_e1402() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile_dir();
        let src_path = dir.join("main.jet");
        std::fs::write(
            &src_path,
            "fn run() {\n    print(0)\n}\n\nfn dev() {\n    print(1)\n}\n",
        )
        .unwrap();
        let wasm = fixture_wasm("crash.wasm");
        std::env::set_var(ENV_COMPILER_EXTENSION, wasm.to_str().unwrap());
        let err = crate::Driver::compile_bundle_path_with_entry(src_path.to_str().unwrap(), "dev")
            .expect_err("crash guest must fail closed on entry-swap");
        std::env::remove_var(ENV_COMPILER_EXTENSION);
        assert!(
            err.iter().any(|d| d.code == "E1402"),
            "entry-swap None-facts path must still surface E1402; got {err:?}"
        );
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jet-cex-hook-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
