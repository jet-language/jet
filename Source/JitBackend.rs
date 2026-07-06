//! c77 (D-JIT1=D) — the stable `JitBackend` execution seam.
//!
//! The `JitBackend` trait and `RunOutcome` live in `jet-foundation`
//! (moved by c139) so the `jet-jit/` workspace member can implement the trait
//! without a dependency cycle. Re-exported here for callers that use the
//! `jet::JitBackend::*` path.

// Re-export the seam types from jet-foundation.
pub use jet_foundation::JitBackend::{JitBackend, RunOutcome};

use crate::Diagnostics::Diagnostic;
use crate::Interpreter::{detect_dev_mode, run_checked, DevMode};
use crate::AST::ProgramBundle;
use std::process::Command;

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
        try_aot_subprocess(bundle).unwrap_or_else(|| self.fallback.run(bundle, try_anyway))
    }

    fn hot_swap(
        &mut self,
        module_name: &str,
        bundle: &ProgramBundle,
        try_anyway: bool,
    ) -> Result<RunOutcome, Vec<Diagnostic>> {
        if let Some(outcome) = try_aot_subprocess(bundle) {
            return Ok(outcome);
        }
        self.fallback.hot_swap(module_name, bundle, try_anyway)
    }

    fn restart(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        try_aot_subprocess(bundle).unwrap_or_else(|| self.fallback.restart(bundle, try_anyway))
    }
}

fn try_aot_subprocess(bundle: &ProgramBundle) -> Option<RunOutcome> {
    if matches!(detect_dev_mode(bundle), DevMode::Resident) {
        return None;
    }
    let entry = bundle.modules.get(bundle.entry)?;
    let file = entry.display.as_str();
    let compiled = crate::compile_with_path(&entry.source, file).ok()?;
    let clinks = crate::resolve_c_links(file).ok()?;
    let root = std::env::temp_dir().join(format!(
        "jet-dev-aot-{}-{}",
        std::process::id(),
        unique_nanos()
    ));
    std::fs::create_dir_all(&root).ok()?;
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
            .arg(&rs)
            .arg("-o")
            .arg(&bin);
        if let Some(link) = &compiled.ffi {
            rustc
                .arg("--extern")
                .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
            if link.deps_dir.is_dir() {
                rustc
                    .arg("-L")
                    .arg(format!("dependency={}", link.deps_dir.display()));
            }
        }
        for arg in clinks {
            rustc.arg(arg);
        }
        let built = rustc.output().ok()?;
        if !built.status.success() {
            return None;
        }
        let run = Command::new(&bin).output().ok()?;
        Some(RunOutcome::Ran {
            stdout: String::from_utf8_lossy(&run.stdout).to_string(),
            stderr: String::from_utf8_lossy(&run.stderr).to_string(),
            exit_code: run.status.code().unwrap_or(1),
        })
    })();
    let _ = std::fs::remove_dir_all(root);
    result
}

fn unique_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}
