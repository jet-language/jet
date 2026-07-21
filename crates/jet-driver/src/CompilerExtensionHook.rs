//! Post-sema compiler-extension hook (D-DX5-HOOK1=A / Tower #549).
//!
//! When `JET_COMPILER_EXTENSION` names a `compiler-extension-v1` `.wasm`,
//! the driver freezes a typed read-only snapshot after sema and sends it to
//! the sibling `jetpack` compiler-extension host. Wasmtime never links into
//! the compiler process. No new user syntax — env registration only until a
//! spelling ballot. Failures are Jet-owned (E1402); guests never crash the
//! compiler or expose rustc (I2/I3).
//!
//! Tower #549 C5 focused proofs live in `tests` below (host/SDK/diagnostic +
//! AOT/dev fact parity). Full repository verification
//! (`scripts/agent/verify-full.sh`) remains the open C5 gate.

use crate::AST::{Func, Item, ProgramBundle};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::SemIndexEffectFacts;
use jet_pkg_model::CompilerExtension::{
    self, decode_and_validate_response, message_exposes_rustc, AnalyzeResponse, Capability,
    Finding, ProtocolError, SpanFact, SymbolFact, TypeFact, TypedSnapshot,
    ENV_COMPILER_EXTENSION, HOST_PROCESS_TIMEOUT_MS, HOST_SUBCOMMAND, MAX_SNAPSHOT_BYTES,
};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MAX_HOST_STDERR_BYTES: usize = 64 * 1024;

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

    match analyze_via_jetpack(path, &snapshot) {
        Ok(response) => findings_to_diagnostics(&snapshot, &response),
        Err(e) => {
            let msg = sanitize_host_message(&e.message);
            vec![host_failure(&msg, None)]
        }
    }
}

/// Run one bounded, versioned compiler-extension exchange in the shipped
/// sibling `jetpack` process. Resolution never consults PATH: release binaries
/// use the same directory; Cargo test binaries also check their parent `debug`
/// directory when running from `debug/deps`.
fn analyze_via_jetpack(
    wasm_path: &str,
    snapshot: &TypedSnapshot,
) -> Result<AnalyzeResponse, ProtocolError> {
    let host = compiler_extension_host_path()?;
    let mut command = Command::new(&host);
    command.arg(HOST_SUBCOMMAND).arg(wasm_path);
    analyze_via_host_command(
        command,
        snapshot,
        Duration::from_millis(HOST_PROCESS_TIMEOUT_MS),
    )
}

fn analyze_via_host_command(
    command: Command,
    snapshot: &TypedSnapshot,
    timeout: Duration,
) -> Result<AnalyzeResponse, ProtocolError> {
    let request = snapshot.encode()?;
    let response = run_host_process(
        command,
        request,
        snapshot.limits.max_response_bytes,
        timeout,
    )?;
    decode_and_validate_response(snapshot, &response)
}

fn run_host_process(
    mut command: Command,
    request: Vec<u8>,
    max_response: usize,
    timeout: Duration,
) -> Result<Vec<u8>, ProtocolError> {
    if request.len() > MAX_SNAPSHOT_BYTES {
        return Err(ProtocolError::new(format!(
            "compiler-extension snapshot exceeds IPC limit ({} > {MAX_SNAPSHOT_BYTES})",
            request.len()
        )));
    }

    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ProtocolError::new(format!("couldn't start compiler-extension host: {e}")))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| ProtocolError::new("compiler-extension host stdin is unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProtocolError::new("compiler-extension host stdout is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ProtocolError::new("compiler-extension host stderr is unavailable"))?;

    let writer = thread::spawn(move || {
        let result = stdin.write_all(&request);
        drop(stdin);
        result
    });
    let stdout_reader = thread::spawn(move || read_bounded(stdout, max_response));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, MAX_HOST_STDERR_BYTES));

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = writer.join();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(ProtocolError::new(format!(
                    "compiler-extension host timed out after {}ms",
                    timeout.as_millis()
                )));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = writer.join();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(ProtocolError::new(format!(
                    "couldn't wait for compiler-extension host: {e}"
                )));
            }
        }
    };

    let write_result = writer
        .join()
        .map_err(|_| ProtocolError::new("compiler-extension host input thread panicked"))?;
    let (stdout, stdout_overflow) = stdout_reader
        .join()
        .map_err(|_| ProtocolError::new("compiler-extension host output thread panicked"))?
        .map_err(|e| ProtocolError::new(format!("couldn't read compiler-extension host output: {e}")))?;
    let (stderr, stderr_overflow) = stderr_reader
        .join()
        .map_err(|_| ProtocolError::new("compiler-extension host error thread panicked"))?
        .map_err(|e| ProtocolError::new(format!("couldn't read compiler-extension host error: {e}")))?;

    if stdout_overflow {
        return Err(ProtocolError::new(format!(
            "compiler-extension host response exceeds IPC limit ({max_response} bytes)"
        )));
    }
    if stderr_overflow {
        return Err(ProtocolError::new(format!(
            "compiler-extension host error exceeds IPC limit ({MAX_HOST_STDERR_BYTES} bytes)"
        )));
    }
    if !status.success() {
        let message = String::from_utf8_lossy(&stderr);
        let message = message.trim();
        return Err(ProtocolError::new(if message.is_empty() {
            format!("compiler-extension host exited with {status}")
        } else {
            message.to_string()
        }));
    }
    write_result.map_err(|e| {
        ProtocolError::new(format!("couldn't send snapshot to compiler-extension host: {e}"))
    })?;
    Ok(stdout)
}

fn read_bounded(
    reader: impl Read,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    let overflow = bytes.len() > limit;
    if overflow {
        bytes.truncate(limit);
    }
    Ok((bytes, overflow))
}

fn compiler_extension_host_path() -> Result<PathBuf, ProtocolError> {
    let exe = std::env::current_exe().map_err(|e| {
        ProtocolError::new(format!("couldn't resolve compiler executable: {e}"))
    })?;
    compiler_extension_host_path_for(&exe)
}

fn compiler_extension_host_path_for(exe: &Path) -> Result<PathBuf, ProtocolError> {
    compiler_extension_host_candidates(exe)
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            ProtocolError::new(
                "compiler-extension host `jetpack` is missing beside the Jet compiler",
            )
        })
}

fn compiler_extension_host_candidates(exe: &Path) -> Vec<PathBuf> {
    let Some(dir) = exe.parent() else {
        return Vec::new();
    };
    let binary = format!(
        "{}{}",
        crate::Syntax::JETPACK_BINARY_NAME,
        std::env::consts::EXE_SUFFIX
    );
    let mut candidates = vec![dir.join(&binary)];
    if dir.file_name().is_some_and(|name| name == "deps") {
        if let Some(parent) = dir.parent() {
            candidates.push(parent.join(binary));
        }
    }
    candidates
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
    use std::sync::{Mutex, Once};

    static ENV_LOCK: Mutex<()> = Mutex::new(());
    static HOST_BUILD: Once = Once::new();

    fn fixture_wasm(name: &str) -> PathBuf {
        ensure_jetpack_host_built();
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../jet-pkg-model/fixtures/compiler_extension")
            .join(name)
    }

    fn ensure_jetpack_host_built() {
        HOST_BUILD.call_once(|| {
            let status = Command::new(env!("CARGO"))
                .args(["build", "-p", "jetpack-bin", "--bin", "jetpack"])
                .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
                .status()
                .expect("start jetpack compiler-extension host build");
            assert!(status.success(), "build jetpack compiler-extension host");
        });
    }

    #[test]
    fn missing_sibling_never_falls_back_to_path() {
        let dir = tempfile_dir();
        let exe = dir.join("debug/deps/jet-driver-test");
        let candidates = compiler_extension_host_candidates(&exe);
        assert_eq!(
            candidates,
            vec![
                dir.join(format!("debug/deps/jetpack{}", std::env::consts::EXE_SUFFIX)),
                dir.join(format!("debug/jetpack{}", std::env::consts::EXE_SUFFIX)),
            ]
        );
        let err = compiler_extension_host_path_for(&exe).expect_err("missing sibling must fail");
        assert!(err.message.contains("missing beside the Jet compiler"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn oversized_ipc_request_is_rejected_before_spawn() {
        let command = Command::new("this-host-must-not-run");
        let err = run_host_process(
            command,
            vec![0; MAX_SNAPSHOT_BYTES + 1],
            1,
            Duration::from_millis(10),
        )
        .expect_err("oversized request must fail before spawn");
        assert!(err.message.contains("snapshot exceeds IPC limit"));
        assert!(!err.message.contains("couldn't start"));
    }

    #[test]
    #[cfg(unix)]
    fn oversized_ipc_response_is_rejected() {
        let mut command = Command::new("sh");
        command.args(["-c", "head -c 300000 /dev/zero", "compiler-extension-host"]);
        let err = run_host_process(
            command,
            b"snapshot".to_vec(),
            128,
            Duration::from_secs(1),
        )
        .expect_err("oversized response must fail");
        assert_eq!(
            err.message,
            "compiler-extension host response exceeds IPC limit (128 bytes)"
        );
    }

    #[test]
    #[cfg(unix)]
    fn timeout_kills_and_reaps_host_process() {
        let dir = tempfile_dir();
        let pid_file = dir.join("host.pid");
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("printf '%s' \"$$\" > \"$1\"; exec sleep 60")
            .arg("compiler-extension-host")
            .arg(&pid_file);
        let err = run_host_process(
            command,
            b"snapshot".to_vec(),
            128,
            Duration::from_millis(100),
        )
        .expect_err("stuck host must time out");
        assert_eq!(err.message, "compiler-extension host timed out after 100ms");
        let pid = std::fs::read_to_string(&pid_file).expect("host must record pid");
        let alive = Command::new("kill")
            .args(["-0", pid.trim()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("probe host pid");
        assert!(!alive.success(), "timed-out host pid {pid} must be reaped");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn nonzero_host_stderr_is_sanitized_before_e1402() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "printf 'rustc error[E0123] leaked\\nsecond line\\n' >&2; exit 9",
            "compiler-extension-host",
        ]);
        let err = run_host_process(
            command,
            b"snapshot".to_vec(),
            128,
            Duration::from_secs(1),
        )
        .expect_err("nonzero host must fail");
        assert!(err.message.contains("rustc error[E0123]"));
        let sanitized = sanitize_host_message(&err.message);
        assert_eq!(sanitized, "compiler-extension failed with an internal message");
        let diagnostic = host_failure(&sanitized, None);
        assert_eq!(diagnostic.code, "E1402");
        assert!(!message_exposes_rustc(&diagnostic.what));
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

    // --- Tower #549 C5 focused proofs ------------------------------------
    // Host/SDK/diagnostic + AOT/dev fact parity where both paths carry facts.
    // Remaining C5 gate (not claimed here): independent Sol review + full
    // `scripts/agent/verify-full.sh` repository verification.

    /// Check vs Run modes freeze byte-identical typed snapshots when both
    /// have solved effect facts (AOT build uses Run; `jet check` uses Check).
    #[test]
    fn check_and_run_mode_effect_fact_snapshots_byte_identical() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(ENV_COMPILER_EXTENSION);
        let dir = tempfile_dir();
        let src_path = dir.join("main.jet");
        std::fs::write(&src_path, "fn run() {\n    print(1)\n}\n").unwrap();
        let path = src_path.to_str().unwrap();

        let mut bundle_check =
            crate::Loader::load_entry_with_overlay(path, None, false).expect("load check");
        let (diags_c, facts_c) = crate::Sema::check_bundle_with_effect_facts(
            &mut bundle_check,
            crate::Sema::CompileMode::Check,
        );
        assert!(
            !diags_c
                .iter()
                .any(|d| d.severity == crate::Diagnostics::Severity::Error),
            "Check sema must succeed; diags={diags_c:?}"
        );
        let enc_check = snapshot_from_bundle(&bundle_check, Some(&facts_c))
            .expect("check snapshot")
            .encode()
            .expect("encode check");

        let mut bundle_run =
            crate::Loader::load_entry_with_overlay(path, None, false).expect("load run");
        let (diags_r, facts_r) = crate::Sema::check_bundle_with_effect_facts(
            &mut bundle_run,
            crate::Sema::CompileMode::Run,
        );
        assert!(
            !diags_r
                .iter()
                .any(|d| d.severity == crate::Diagnostics::Severity::Error),
            "Run sema must succeed; diags={diags_r:?}"
        );
        let enc_run = snapshot_from_bundle(&bundle_run, Some(&facts_r))
            .expect("run snapshot")
            .encode()
            .expect("encode run");

        assert_eq!(
            enc_check, enc_run,
            "Check vs Run typed snapshots must be byte-identical when both have effect facts"
        );
        let snap = snapshot_from_bundle(&bundle_check, Some(&facts_c)).unwrap();
        assert!(
            snap.capabilities.contains(&Capability::ReadEffects),
            "fact-bearing snapshots must advertise ReadEffects; caps={:?}",
            snap.capabilities
        );
    }

    /// `jet check` and `jet build` (facts path) surface the same L1401 finding.
    #[test]
    fn check_and_build_surface_same_l1401() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile_dir();
        let src_path = dir.join("main.jet");
        std::fs::write(&src_path, "fn run() {\n    print(1)\n}\n").unwrap();
        let path = src_path.to_str().unwrap();
        let wasm = fixture_wasm("lint_no_x.wasm");
        std::env::set_var(ENV_COMPILER_EXTENSION, wasm.to_str().unwrap());

        let (check_diags, bundle, _) =
            crate::Driver::check_file_with_effect_facts(path, None, false);
        assert!(bundle.is_some(), "check sema must succeed; diags={check_diags:?}");
        let check_lint = check_diags
            .iter()
            .find(|d| d.code == "L1401")
            .unwrap_or_else(|| panic!("check must surface L1401; diags={check_diags:?}"));

        let build_out = crate::Driver::compile_bundle_path_build(
            path,
            crate::Driver::BuildRunOptions::default(),
        );
        std::env::remove_var(ENV_COMPILER_EXTENSION);
        let build_out = build_out.unwrap_or_else(|errs| {
            panic!("build must succeed with lint-only guest; errs={errs:?}")
        });
        let build_lint = build_out
            .compile
            .lints
            .iter()
            .find(|d| d.code == "L1401")
            .unwrap_or_else(|| {
                panic!(
                    "build must surface L1401; lints={:?}",
                    build_out.compile.lints
                )
            });

        assert_eq!(check_lint.what, build_lint.what);
        assert_eq!(check_lint.why, build_lint.why);
        assert!(check_lint.what.contains("no-x"));
        assert!(check_lint.what.contains("prefer y"));
    }

    /// AOT opts_full (`jet run`) and entry-swap (`jet dev`) both get L1401
    /// even with None effect facts (guest does not require ReadEffects).
    #[test]
    fn aot_opts_full_and_dev_entry_swap_surface_same_l1401() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile_dir();
        let src_path = dir.join("main.jet");
        std::fs::write(
            &src_path,
            "fn run() {\n    print(0)\n}\n\nfn dev() {\n    print(1)\n}\n",
        )
        .unwrap();
        let path = src_path.to_str().unwrap();
        let wasm = fixture_wasm("lint_no_x.wasm");
        std::env::set_var(ENV_COMPILER_EXTENSION, wasm.to_str().unwrap());

        let aot = crate::Driver::compile_bundle_path_opts(
            path,
            crate::Sema::CompileMode::Run,
            false,
            false,
            false,
            None,
        )
        .unwrap_or_else(|errs| panic!("opts_full must succeed with lint guest; errs={errs:?}"));
        let aot_lint = aot
            .lints
            .iter()
            .find(|d| d.code == "L1401")
            .unwrap_or_else(|| panic!("opts_full must surface L1401; lints={:?}", aot.lints));

        let dev = crate::Driver::compile_bundle_path_with_entry(path, "dev")
            .unwrap_or_else(|errs| panic!("entry-swap must succeed with lint guest; errs={errs:?}"));
        std::env::remove_var(ENV_COMPILER_EXTENSION);
        let dev_lint = dev
            .lints
            .iter()
            .find(|d| d.code == "L1401")
            .unwrap_or_else(|| panic!("entry-swap must surface L1401; lints={:?}", dev.lints));

        assert_eq!(aot_lint.what, dev_lint.what);
        assert_eq!(aot_lint.why, dev_lint.why);
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
