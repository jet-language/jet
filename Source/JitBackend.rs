//! c77 (D-JIT1=D) — the stable `JitBackend` execution seam.
//!
//! The `JitBackend` trait and `RunOutcome` live in `jet-foundation`
//! (moved by c139) so the `jet-jit/` workspace member can implement the trait
//! without a dependency cycle. Re-exported here for callers that use the
//! `jet::JitBackend::*` path.

// Re-export the seam types from jet-foundation.
pub use jet_foundation::JitBackend::{JitBackend, RunOutcome};

use crate::Diagnostics::Diagnostic;
use crate::Interpreter::run_checked;
use crate::AST::ProgramBundle;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Tier-0 backend: the comptime interpreter. Stateless between runs (no
/// resident heap), so every method funnels into [`run_checked`].
///
/// This is the permanent fallback (D-JIT1): even when Cranelift tier-1 is
/// active, unsupported resident paths fall back here, never to silence.
#[derive(Default)]
pub struct InterpreterBackend;

impl InterpreterBackend {
    pub fn new() -> Self {
        InterpreterBackend
    }
}

impl JitBackend for InterpreterBackend {
    fn run(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        run_checked(bundle, try_anyway)
    }

    fn hot_swap(
        &mut self,
        _module_name: &str,
        bundle: &ProgramBundle,
        try_anyway: bool,
    ) -> Result<RunOutcome, Vec<Diagnostic>> {
        match run_checked(bundle, try_anyway) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => Ok(RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            }),
            RunOutcome::Problems(diags) => Err(diags),
        }
    }

    fn restart(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        run_checked(bundle, try_anyway)
    }
}

/// Default-dev fallback ladder: when resident JIT cannot host a checked bundle,
/// try the same native AOT path `jet run` uses before falling back to the
/// interpreter boundary. This keeps backend gaps internal for AOT-runnable
/// programs while preserving the old tier-0 behavior when rustc/linking cannot
/// run in this process.
pub struct AotFallbackBackend<F: JitBackend> {
    fallback: F,
}

impl<F: JitBackend> AotFallbackBackend<F> {
    pub fn new(fallback: F) -> Self {
        Self { fallback }
    }
}

impl<F: JitBackend> JitBackend for AotFallbackBackend<F> {
    fn run(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        match try_aot_subprocess(bundle) {
            AotAttempt::Outcome(outcome) => outcome,
            AotAttempt::Unavailable => self.fallback.run(bundle, try_anyway),
        }
    }

    fn hot_swap(
        &mut self,
        module_name: &str,
        bundle: &ProgramBundle,
        try_anyway: bool,
    ) -> Result<RunOutcome, Vec<Diagnostic>> {
        match try_aot_subprocess(bundle) {
            AotAttempt::Outcome(outcome) => return Ok(outcome),
            AotAttempt::Unavailable => {}
        }
        self.fallback.hot_swap(module_name, bundle, try_anyway)
    }

    fn restart(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        match try_aot_subprocess(bundle) {
            AotAttempt::Outcome(outcome) => outcome,
            AotAttempt::Unavailable => self.fallback.restart(bundle, try_anyway),
        }
    }
}

enum AotAttempt {
    Outcome(RunOutcome),
    Unavailable,
}

fn try_aot_subprocess(bundle: &ProgramBundle) -> AotAttempt {
    let Some(entry) = bundle.modules.get(bundle.entry) else {
        return AotAttempt::Unavailable;
    };
    let file = entry.display.as_str();
    let compiled = match crate::compile_with_path(&entry.source, file) {
        Ok(compiled) => compiled,
        Err(diags) => return AotAttempt::Outcome(RunOutcome::Problems(diags)),
    };
    let clinks = match crate::resolve_c_links(file) {
        Ok(clinks) => clinks,
        Err(diags) => return AotAttempt::Outcome(RunOutcome::Problems(diags)),
    };
    let root = std::env::temp_dir().join(format!(
        "jet-dev-aot-{}-{}",
        std::process::id(),
        unique_nanos()
    ));
    if std::fs::create_dir_all(&root).is_err() {
        return AotAttempt::Unavailable;
    }
    let rs = root.join("main.rs");
    let bin = root.join("main");
    let result = (|| {
        std::fs::write(&rs, &compiled.rust).ok()?;
        let mut rustc = Command::new("rustc");
        rustc
            .arg("--edition")
            .arg("2021")
            .arg("--crate-name")
            .arg("jet_dev_aot")
            // Transparent fallback must use the same optimization mode as
            // default `jet run`; otherwise cfg(debug_assertions) changes
            // observable runtime behavior such as E3002 propagation traces.
            .arg("-O")
            .arg(&rs)
            .arg("-o")
            .arg(&bin);
        if let Some(link) = &compiled.ffi {
            rustc
                .arg("--extern")
                .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
            for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
                rustc
                    .arg("-L")
                    .arg(format!("dependency={}", deps_dir.display()));
            }
        }
        for arg in clinks {
            rustc.arg(arg);
        }
        let built = match rustc.output() {
            Ok(output) => output,
            Err(_) => return None,
        };
        if !built.status.success() {
            return Some(ice_rustc_rejected(&rs, &built.stderr));
        }
        let run = run_aot_binary_with_timeout(&bin, Duration::from_secs(5))?;
        Some(RunOutcome::Ran {
            stdout: String::from_utf8_lossy(&run.stdout).to_string(),
            stderr: String::from_utf8_lossy(&run.stderr).to_string(),
            exit_code: run.status.code().unwrap_or(1),
        })
    })();
    let _ = std::fs::remove_dir_all(root);
    result
        .map(AotAttempt::Outcome)
        .unwrap_or(AotAttempt::Unavailable)
}

fn ice_rustc_rejected(rs_path: &std::path::Path, stderr: &[u8]) -> RunOutcome {
    let stderr = String::from_utf8_lossy(stderr);
    RunOutcome::Ran {
        stdout: String::new(),
        stderr: format!(
            "internal compiler error: the generated Rust did not compile.\n\
             This is a bug in {}, NOT in your program. Please report it,\n\
             attaching your source file and the generated file below.\n\
               generated: {}\n\
             --- rustc said ---\n\
             {}\n",
            crate::Syntax::BINARY_NAME,
            rs_path.display(),
            stderr
        ),
        exit_code: 101,
    }
}

fn run_aot_binary_with_timeout(
    bin: &std::path::Path,
    timeout: Duration,
) -> Option<std::process::Output> {
    let mut child = Command::new(bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().ok()?.is_some() {
            return child.wait_with_output().ok();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn unique_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}
