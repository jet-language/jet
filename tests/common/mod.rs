//! Shared helpers for the integration-test suites (`mod common;`).
//!
//! Each tests/*.rs binary compiles its own copy of this module, so not every
//! suite uses every item — hence the file-level `allow(dead_code)`.

#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Collision-safe scratch dir under `std::env::temp_dir()`: prefix + pid +
/// per-process counter, so concurrent tests in one binary never share a dir.
pub fn unique_tmp(prefix: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), n))
}

pub fn have_rustc() -> bool {
    Command::new("rustc").arg("--version").output().is_ok()
}

/// Cross-process advisory lock serializing access to Jet's hidden global FFI
/// bridge cache (`~/.cache/jet/ffi/<key>/`, keyed by a hash of the `extern
/// rust`/`jet.regex`/`core.archive`/etc. signature — see
/// `crates/jet-driver/src/FFI.rs` `build_bridge`/`cache_dir`).
///
/// That cache has no synchronization of its own: `build_bridge` unconditionally
/// (over)writes `Cargo.toml` + `src/lib.rs` at the cache path and then spawns
/// `cargo build`, every single call, even when a same-keyed build is already
/// cached or in flight. Two `jet::compile_with_path` calls that land on the
/// same cache key — including from *different test binaries*, i.e. different
/// OS processes, e.g. tests/golden.rs and tests/cffi.rs both building the FFI
/// bridge for `examples/features/lowlevel/ffi.jet` — race on those writes: one
/// process's `fs::write` truncates the file out from under a concurrent
/// `cargo build` reading it, which can hand `rustc`/`cargo` a momentarily
/// truncated `lib.rs` and surface as a spurious E0704/E0705 from the *other*
/// process. This is a real product-level concurrency bug (any two real
/// concurrent `jet build`/`jet run` invocations sharing an `extern rust`
/// signature can hit it too), reported separately — not silently patched here.
/// This lock only serializes the *test suite's* known collisions so CI doesn't
/// flake on it.
pub struct FfiBridgeLock {
    dir: PathBuf,
}

impl FfiBridgeLock {
    /// Blocks until the lock is held. Steals a stale lock (mtime older than 2
    /// minutes — far longer than any single FFI bridge `cargo build` takes)
    /// so a killed/timed-out test process can't wedge every later run.
    pub fn acquire() -> FfiBridgeLock {
        let dir = std::env::temp_dir().join("jet_ffi_bridge_test.lock");
        loop {
            match fs::create_dir(&dir) {
                Ok(()) => return FfiBridgeLock { dir },
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Ok(meta) = fs::metadata(&dir) {
                        if let Ok(age) = meta.modified().and_then(|m| {
                            m.elapsed()
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                        }) {
                            if age > std::time::Duration::from_secs(120) {
                                let _ = fs::remove_dir(&dir);
                                continue;
                            }
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => panic!("couldn't create FFI bridge test lock dir: {e}"),
            }
        }
    }
}

impl Drop for FfiBridgeLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.dir);
    }
}

/// Compile `src` through the real front end, build it with rustc (linking the
/// FFI bridge when the compile produces one), run the binary, and return
/// (exit code, stdout, stderr).
///
/// Panics if the front end rejects the source or rustc rejects the generated
/// code (I2). `prefix` names the scratch dir (e.g. "jet_tir_test") so suites
/// stay distinguishable in /tmp. Callers must gate on `have_rustc()` first.
pub fn build_and_run(prefix: &str, name: &str, src: &str) -> (i32, String, String) {
    let dir = unique_tmp(prefix);
    fs::create_dir_all(&dir).unwrap();
    // `compile_with_path` loads the entry from disk, so write the .jet first.
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd.args([
        "--edition",
        "2021",
        rs.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    if let Some(link) = &out.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        if link.deps_dir.is_dir() {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", link.deps_dir.display()));
        }
    }
    let rustc = rustc_cmd.output().unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}
