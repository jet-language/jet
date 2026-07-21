//! Post-sema compiler-extension hook (D-DX5-HOOK1=A / Tower #549).
//!
//! When `JET_COMPILER_EXTENSION` names a `compiler-extension-v1` `.wasm`,
//! the driver freezes a typed read-only snapshot after sema, runs the
//! jet-pkg-model wasmtime host, validates the response, and maps findings
//! to Jet diagnostics. No new user syntax — env registration only until a
//! spelling ballot. Failures are Jet-owned (E1402); guests never crash the
//! compiler or expose rustc (I2/I3).

use crate::AST::{Item, ProgramBundle};
use crate::Diagnostics::{Diagnostic, Span};
use jet_pkg_model::CompilerExtension::{
    self, message_exposes_rustc, AnalyzeResponse, Capability, Finding, SpanFact, SymbolFact,
    TypeFact, TypedSnapshot, ENV_COMPILER_EXTENSION,
};

/// Run configured compiler-extension(s) after a successful sema pass.
/// Returns diagnostics to merge with the check/compile result (empty when
/// the env var is unset).
pub fn post_sema_diagnostics(bundle: &ProgramBundle) -> Vec<Diagnostic> {
    let Ok(path) = std::env::var(ENV_COMPILER_EXTENSION) else {
        return Vec::new();
    };
    let path = path.trim();
    if path.is_empty() {
        return Vec::new();
    }

    let snapshot = match snapshot_from_bundle(bundle) {
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
pub fn snapshot_from_bundle(bundle: &ProgramBundle) -> Result<TypedSnapshot, CompilerExtension::ProtocolError> {
    let module = &bundle.modules[bundle.entry];
    let file = module.display.clone();
    let mut types = vec![TypeFact {
        id: "t0".into(),
        repr: "Fn".into(),
    }];
    let mut symbols = Vec::new();
    let mut spans = Vec::new();
    let mut n = 0u32;
    for item in &module.items {
        let Item::Func(func) = item else {
            continue;
        };
        n += 1;
        let sid = format!("s{n}");
        let spid = format!("sp{n}");
        symbols.push(SymbolFact {
            id: sid,
            name: func.name.clone(),
            kind: "fn".into(),
            type_id: "t0".into(),
            span_id: spid.clone(),
            effects: vec!["pure".into()],
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
        types.clear();
        types.push(TypeFact {
            id: "t0".into(),
            repr: "Unit".into(),
        });
        spans.push(SpanFact {
            id: "sp1".into(),
            file,
            start: 0,
            end: 0,
        });
    }
    TypedSnapshot::new(
        Capability::v1_defaults().to_vec(),
        types,
        symbols,
        spans,
    )
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
