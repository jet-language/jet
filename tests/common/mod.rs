//! Shared helpers for the integration-test suites (`mod common;`).
//!
//! Each tests/*.rs binary compiles its own copy of this module, so not every
//! suite uses every item — hence the file-level `allow(dead_code)`.

#![allow(dead_code)]

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::OnceLock;

// --- runaway-test guard rails -----------------------------------------------
//
// A runaway test binary (unbounded allocation loop, or a hang) can eat all
// RAM/swap and get the kernel to OOM-kill the whole agent session. Every
// tests/*.rs binary installs this as its global allocator (via `mod
// common;`), which caps live allocation and starts a wall-clock watchdog on
// first use. Plain atomics only: no locks, no deps, nothing allocated on the
// failure path beyond the one stderr line.

struct GuardedAlloc;

/// Cap in bytes; -1 means "not read from env yet". i64 (not u64) so the tiny,
/// unavoidable unaccounted allocation from the lazy env read below (see
/// `guard_on_alloc`) can dip `GUARD_LIVE_BYTES` negative without wrapping.
static GUARD_CAP_BYTES: AtomicI64 = AtomicI64::new(-1);
static GUARD_CAP_INIT_STARTED: AtomicBool = AtomicBool::new(false);
static GUARD_LIVE_BYTES: AtomicI64 = AtomicI64::new(0);
static GUARD_WATCHDOG_STARTED: AtomicBool = AtomicBool::new(false);

unsafe impl std::alloc::GlobalAlloc for GuardedAlloc {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let ptr = std::alloc::System.alloc(layout);
        if !ptr.is_null() {
            guard_on_alloc(layout.size() as i64);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        std::alloc::System.dealloc(ptr, layout);
        GUARD_LIVE_BYTES.fetch_sub(layout.size() as i64, Ordering::Relaxed);
    }
}

fn guard_on_alloc(size: i64) {
    let cap = GUARD_CAP_BYTES.load(Ordering::Relaxed);
    if cap < 0 {
        // Cap not read yet. Claim the read exactly once; std::env::var itself
        // allocates, so the nested alloc() call this triggers re-enters here,
        // sees the flag already set, and just falls through uncounted — a
        // one-time, few-byte startup blind spot the i64 counter absorbs.
        if !GUARD_CAP_INIT_STARTED.swap(true, Ordering::AcqRel) {
            let gb: i64 = std::env::var("JET_TEST_ALLOC_CAP_GB")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10);
            GUARD_CAP_BYTES.store(gb.max(1) * (1 << 30), Ordering::Relaxed);
        }
        return;
    }
    let total = GUARD_LIVE_BYTES.fetch_add(size, Ordering::Relaxed) + size;
    if total > cap {
        eprintln!(
            "jet test guard: allocation cap {} GB exceeded — aborting; raise JET_TEST_ALLOC_CAP_GB if legitimate",
            cap / (1 << 30)
        );
        std::process::abort();
    }
    guard_start_watchdog();
}

/// Lazily spawned on first (accounted) allocation, guarded by a one-shot
/// flag: a plain relaxed load short-circuits the common case (already
/// started) before paying for the atomic swap.
fn guard_start_watchdog() {
    if GUARD_WATCHDOG_STARTED.load(Ordering::Relaxed) {
        return;
    }
    if GUARD_WATCHDOG_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("jet-test-watchdog".into())
        .spawn(guard_watchdog_main);
    if spawned.is_err() {
        // Too early in process startup to spawn a thread (rare) — let a
        // later allocation retry.
        GUARD_WATCHDOG_STARTED.store(false, Ordering::Release);
    }
}

fn guard_watchdog_main() {
    // Backstop, not an accommodation: no test suite should run past 15-20
    // minutes. A suite that hits this is itself defective — split or speed
    // it up, don't raise the cap.
    let secs: u64 = std::env::var("JET_TEST_DEADLINE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(900);
    std::thread::sleep(std::time::Duration::from_secs(secs));
    eprintln!(
        "jet test guard: exceeded the {secs}s suite budget — aborting; this suite must be split or sped up, not given a longer deadline"
    );
    std::process::abort();
}

#[global_allocator]
static JET_TEST_GUARD: GuardedAlloc = GuardedAlloc;

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Collision-safe scratch dir under `std::env::temp_dir()`: prefix + pid +
/// per-process counter, so concurrent tests in one binary never share a dir.
pub fn unique_tmp(prefix: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), n))
}

/// Whether `rustc` is on PATH. Honors `JET_REQUIRE_RUSTC=1` (D-CI3): CI sets
/// this so a missing rustc is a loud failure — never a quiet self-skip that
/// silently drops I2 (rustc-must-accept) coverage.
pub fn have_rustc() -> bool {
    static PRESENT: OnceLock<bool> = OnceLock::new();
    let present = *PRESENT.get_or_init(|| Command::new("rustc").arg("--version").output().is_ok());
    if !present && std::env::var("JET_REQUIRE_RUSTC").as_deref() == Ok("1") {
        panic!(
            "JET_REQUIRE_RUSTC=1 but rustc not found on PATH — refusing to \
             silently skip I2 (rustc-must-accept) coverage. Fix the CI \
             environment; do not unset JET_REQUIRE_RUSTC to paper over this."
        );
    }
    present
}

/// Write generated Rust and add the one canonical cached-runtime dependency to
/// a raw rustc command. The caller still compiles and links the user program.
pub fn add_generated_rust(
    command: &mut Command,
    path: &Path,
    generated: &str,
    has_rust_ffi: bool,
    rustc_flags: &[&str],
) {
    let flags = rustc_flags
        .iter()
        .map(|flag| std::ffi::OsString::from(*flag))
        .collect::<Vec<_>>();
    // A raw Rust `--test` invocation asks rustc to synthesize a harness from
    // every embedded `#[cfg(test)]` module. Keep that uncommon inspection mode
    // inline; it is not the Jet test-harness build path.
    let rust_test_harness = rustc_flags.contains(&"--test");
    let prepared = if has_rust_ffi || rust_test_harness {
        jet::RuntimeCache::PreparedRuntime::inline(generated)
    } else {
        match jet::RuntimeCache::prepare(std::ffi::OsStr::new("rustc"), generated, &flags, &[]) {
            Ok(prepared) => prepared,
            Err(jet::RuntimeCache::Error::Cache(_)) => {
                jet::RuntimeCache::PreparedRuntime::inline(generated)
            }
            Err(error) => panic!("cached runtime build failed: {error}"),
        }
    };
    fs::write(path, prepared.rust()).unwrap();
    command
        .arg("--edition")
        .arg("2021")
        .args(rustc_flags.iter().copied())
        .arg(path);
    prepared.add_rustc_args(command);
}

/// A throwaway directory under the system temp dir, removed on drop.
///
/// `tag` names the case. The path also carries the pid, a nanosecond stamp and
/// the thread id, so concurrent tests never share a directory — in one binary
/// or across binaries.
pub struct Scratch {
    pub path: PathBuf,
}

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "jet-it-{tag}-{}-{nanos}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }

    pub fn join(&self, p: &str) -> PathBuf {
        self.path.join(p)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Store trees are realized read-only; chmod first or the directory
        // leaks in the temp dir.
        make_tree_writable(&self.path);
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
pub fn make_tree_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    // Runs inside Drop for every suite: a file the tested daemon removes
    // mid-walk, or a dir we cannot read yet, must not panic during unwind
    // and bury the original assertion.
    if !meta.file_type().is_symlink() {
        let mode = if meta.is_dir() {
            0o755
        } else {
            meta.permissions().mode() | 0o600
        };
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
    if meta.is_dir() {
        let Ok(entries) = fs::read_dir(path) else { return };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            make_tree_writable(&entry.path());
        }
    }
}

#[cfg(not(unix))]
pub fn make_tree_writable(path: &Path) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    let mut permissions = meta.permissions();
    permissions.set_readonly(false);
    let _ = fs::set_permissions(path, permissions);
}

pub fn test_worker_count(cap: usize) -> usize {
    let requested = std::env::var("JET_TEST_JOBS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0);
    let available = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    requested.unwrap_or(available).clamp(1, cap.max(1))
}

/// Locate the `jetpack` binary for integration tests.
///
/// `jetpack` moved to its own binary workspace package (`crates/jetpack-bin`,
/// card #367 / D-PRODUCT-SPLIT1=C), so Cargo no longer sets
/// `CARGO_BIN_EXE_jetpack` for tests compiled under the root `jet` package.
/// Fall back to the freshly built debug binary next to `target/debug/jet`,
/// building it on demand if it isn't there yet.
pub fn jetpack_bin() -> &'static PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| resolve_or_build_bin("jetpack", "jetpack-bin", "jetpack"))
}

/// Locate the `jetos` binary for integration tests. Same story as
/// [`jetpack_bin`]: `jetos` is its own workspace package now.
pub fn jetos_bin() -> &'static PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| resolve_or_build_bin("jetos", "jetos", "jetos"))
}

fn resolve_or_build_bin(env_suffix: &str, package: &str, bin_name: &str) -> PathBuf {
    if let Some(path) = std::env::var_os(format!("CARGO_BIN_EXE_{env_suffix}")) {
        return PathBuf::from(path);
    }
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
    let bin = target_dir
        .join("debug")
        .join(format!("{bin_name}{}", std::env::consts::EXE_SUFFIX));
    if !bin.is_file() {
        let status = Command::new(env!("CARGO"))
            .args(["build", "-p", package, "--bin", bin_name])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .status()
            .unwrap();
        assert!(status.success(), "building {bin_name} test binary failed");
    }
    bin
}

pub fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else {
        "panic with non-string payload".to_string()
    }
}

/// Read a fixture substring filter and normalize it to a repository-relative,
/// forward-slash path. Absolute paths and parent traversal are rejected so a
/// copied CI command behaves the same from every checkout.
pub fn fixture_filter(var: &str) -> Option<String> {
    let raw = std::env::var(var).ok()?;
    Some(normalize_fixture_selector(var, &raw))
}

pub fn normalize_fixture_selector(var: &str, raw: &str) -> String {
    let normalized_separators = raw.trim().replace('\\', "/");
    let raw = normalized_separators.as_str();
    assert!(!raw.is_empty(), "{var} must not be empty");
    let path = Path::new(raw);
    assert!(
        !path.is_absolute(),
        "{var} must be repository-relative: {raw}"
    );
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                panic!("{var} must not escape the repository: {raw}")
            }
        }
    }
    assert!(!parts.is_empty(), "{var} must name a fixture");
    parts.join("/")
}

pub fn fixture_matches(filter: Option<&str>, canonical_relative: &str) -> bool {
    filter.map_or(true, |needle| canonical_relative.contains(needle))
}

pub const UNIFIED_DIFF_MAX_INPUT_BYTES: usize = 64 * 1024;
pub const UNIFIED_DIFF_MAX_INPUT_LINES: usize = 256;
pub const UNIFIED_DIFF_MAX_OUTPUT_BYTES: usize = 32 * 1024;

const DIFF_OUTPUT_TRUNCATED: &str =
    "\n... diff output truncated; remaining compared edits omitted ...\n";

/// Small std-only line diff for fixture failures. Hostile compiler output is
/// bounded before the dynamic-programming LCS and while rendering, keeping
/// memory and failure-message size fixed. Every discarded input or edit is
/// named explicitly in the output.
pub fn unified_diff(
    expected_name: &str,
    actual_name: &str,
    expected: &str,
    actual: &str,
) -> String {
    let expected_lines = bounded_diff_lines(expected);
    let actual_lines = bounded_diff_lines(actual);
    let columns = actual_lines.lines.len() + 1;
    let mut lcs = vec![0usize; (expected_lines.lines.len() + 1) * columns];
    for i in (0..expected_lines.len()).rev() {
        for j in (0..actual_lines.lines.len()).rev() {
            let here = i * columns + j;
            lcs[here] = if expected_lines.lines[i] == actual_lines.lines[j] {
                lcs[(i + 1) * columns + j + 1] + 1
            } else {
                lcs[(i + 1) * columns + j].max(lcs[i * columns + j + 1])
            };
        }
    }

    let mut out = BoundedDiffOutput::new();
    out.push(&format!("--- {expected_name}\n+++ {actual_name}\n"));
    push_input_truncation(&mut out, "expected", expected, &expected_lines);
    push_input_truncation(&mut out, "actual", actual, &actual_lines);
    out.push(&format!(
        "@@ -1,{} +1,{} @@\n",
        expected_lines.lines.len(),
        actual_lines.lines.len()
    ));
    let (mut i, mut j) = (0, 0);
    while i < expected_lines.lines.len() || j < actual_lines.lines.len() {
        if i < expected_lines.lines.len()
            && j < actual_lines.lines.len()
            && expected_lines.lines[i] == actual_lines.lines[j]
        {
            push_diff_line(
                &mut out,
                ' ',
                expected_lines.lines[i],
                expected_lines.cut_mid_line && i + 1 == expected_lines.lines.len(),
            );
            i += 1;
            j += 1;
        } else if j < actual_lines.lines.len()
            && (i == expected_lines.lines.len()
                || lcs[i * columns + j + 1] >= lcs[(i + 1) * columns + j])
        {
            push_diff_line(
                &mut out,
                '+',
                actual_lines.lines[j],
                actual_lines.cut_mid_line && j + 1 == actual_lines.lines.len(),
            );
            j += 1;
        } else {
            push_diff_line(
                &mut out,
                '-',
                expected_lines.lines[i],
                expected_lines.cut_mid_line && i + 1 == expected_lines.lines.len(),
            );
            i += 1;
        }
    }
    out.finish()
}

struct BoundedDiffLines<'a> {
    lines: Vec<&'a str>,
    compared_bytes: usize,
    cut_mid_line: bool,
}

impl BoundedDiffLines<'_> {
    fn len(&self) -> usize {
        self.lines.len()
    }
}

fn bounded_diff_lines(input: &str) -> BoundedDiffLines<'_> {
    let mut byte_end = input.len().min(UNIFIED_DIFF_MAX_INPUT_BYTES);
    while !input.is_char_boundary(byte_end) {
        byte_end -= 1;
    }
    let prefix = &input[..byte_end];
    let mut lines = Vec::new();
    let mut compared_bytes = 0;
    for line in prefix
        .split_inclusive('\n')
        .take(UNIFIED_DIFF_MAX_INPUT_LINES)
    {
        compared_bytes += line.len();
        lines.push(line);
    }
    BoundedDiffLines {
        lines,
        compared_bytes,
        cut_mid_line: compared_bytes < input.len()
            && compared_bytes > 0
            && input.as_bytes()[compared_bytes - 1] != b'\n',
    }
}

fn push_input_truncation(
    out: &mut BoundedDiffOutput,
    side: &str,
    input: &str,
    bounded: &BoundedDiffLines<'_>,
) {
    if bounded.compared_bytes < input.len() {
        out.push(&format!(
            "# diff input truncated: {side} compares first {} of {} bytes across {} lines; limits are {} bytes/{} lines\n",
            bounded.compared_bytes,
            input.len(),
            bounded.lines.len(),
            UNIFIED_DIFF_MAX_INPUT_BYTES,
            UNIFIED_DIFF_MAX_INPUT_LINES,
        ));
    }
}

struct BoundedDiffOutput {
    text: String,
    truncated: bool,
}

impl BoundedDiffOutput {
    fn new() -> Self {
        Self {
            text: String::new(),
            truncated: false,
        }
    }

    fn push(&mut self, text: &str) {
        if self.truncated {
            return;
        }
        let content_limit = UNIFIED_DIFF_MAX_OUTPUT_BYTES - DIFF_OUTPUT_TRUNCATED.len();
        let remaining = content_limit.saturating_sub(self.text.len());
        if text.len() <= remaining {
            self.text.push_str(text);
            return;
        }
        let mut take = remaining.min(text.len());
        while !text.is_char_boundary(take) {
            take -= 1;
        }
        self.text.push_str(&text[..take]);
        self.truncated = true;
    }

    fn finish(mut self) -> String {
        if self.truncated {
            self.text.push_str(DIFF_OUTPUT_TRUNCATED);
        }
        debug_assert!(self.text.len() <= UNIFIED_DIFF_MAX_OUTPUT_BYTES);
        self.text
    }
}

fn push_diff_line(out: &mut BoundedDiffOutput, marker: char, line: &str, cut_mid_line: bool) {
    let mut marker_bytes = [0; 4];
    out.push(marker.encode_utf8(&mut marker_bytes));
    out.push(line);
    if !line.ends_with('\n') && !cut_mid_line {
        out.push("\n\\ No newline at end of file\n");
    }
}

/// Cross-process advisory lock serializing access to Jet's hidden global FFI
/// bridge cache (`~/.cache/jet/ffi/<key>/`, keyed by a hash of the `extern
/// rust`/`core.regex`/`core.archive`/etc. signature — see
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
    build_and_run_with_cwd(prefix, name, src, false)
}

/// `build_and_run`, with the generated binary's working directory isolated to
/// its unique scratch directory.
pub fn build_and_run_in_scratch(prefix: &str, name: &str, src: &str) -> (i32, String, String) {
    build_and_run_with_cwd(prefix, name, src, true)
}

fn build_and_run_with_cwd(
    prefix: &str,
    name: &str,
    src: &str,
    run_in_scratch: bool,
) -> (i32, String, String) {
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
    let mut rustc_cmd = Command::new("rustc");
    add_generated_rust(&mut rustc_cmd, &rs, &out.rust, out.ffi.is_some(), &[]);
    rustc_cmd.arg("-o").arg(&bin);
    if let Some(link) = &out.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let rustc = rustc_cmd.output().unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let mut run_cmd = Command::new(&bin);
    if run_in_scratch {
        run_cmd.current_dir(&dir);
    }
    let run = run_cmd.output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

/// Remove every audited generated-prelude module (jet_mem, jet_txn, the
/// per-platform jet_term/jet_os/jet_atomic shims, jet_gtk, and any
/// user___c_* CFFI overlay module) before checking generated Rust for I1
/// violations. Mirrors `golden.rs::strip_vetted_prelude_modules` — kept as a
/// second, independent implementation so a sema-soundness corpus check does
/// not depend on golden.rs internals.
pub fn strip_vetted_prelude_modules(rust_code: &str) -> String {
    fn strip_mod(src: &str, name: &str) -> String {
        let Some(start) = src.find(&format!("mod {name}")) else {
            return src.to_string();
        };
        let bytes = src.as_bytes();
        let mut depth = 0usize;
        let mut i = start;
        let mut end = src.len();
        let mut seen_brace = false;
        while i < bytes.len() {
            match bytes[i] {
                b'{' => {
                    depth += 1;
                    seen_brace = true;
                }
                b'}' => {
                    depth -= 1;
                    if seen_brace && depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        format!("{}{}", &src[..start], &src[end..])
    }
    let s = strip_mod(rust_code, "jet_uninit_semantics");
    let s = strip_mod(&s, "jet_mem");
    let s = strip_mod(&s, "jet_cell");
    let s = strip_mod(&s, "jet_txn");
    let s = strip_mod(&s, "jet_term_unix");
    let s = strip_mod(&s, "jet_term_windows");
    let s = strip_mod(&s, "jet_process_pty");
    let s = strip_mod(&s, "jet_os_unix");
    let s = strip_mod(&s, "jet_atomic_windows");
    let s = strip_mod(&s, "jet_gtk");
    let s = strip_mod(&s, "jet_crypto_entropy");
    let mut s = strip_scheduler_native(&s);
    s = strip_shared_guard_internals(&s);
    s = strip_vetted_module(&s, "jet_env_windows");
    s = strip_vetted_module(&s, "jet_watch_process_probe");
    s = strip_vetted_module(&s, "jet_atomic_windows");
    s = strip_vetted_module(&s, "jet_ws_upgrade");
    // D-TASKBORROW1=A: scoped taskgroup lifetime erasure (mirrors golden.rs).
    s = strip_vetted_module(&s, "jet_taskgroup_scoped");
    s = strip_vetted_module(&s, "ffi_reporter");
    while s.contains("mod user___c_") {
        let before = s.clone();
        s = strip_mod(&s, "user___c_");
        if s == before {
            break;
        }
    }
    s
}

/// Remove the vetted `jet:scheduler-native` region (raw epoll/kqueue syscalls,
/// the only `unsafe` in the emitted scheduler — Tower #126) before an I1 scan,
/// exactly as `golden.rs::strip_vetted_prelude_modules` does.
pub fn strip_scheduler_native(src: &str) -> String {
    let begin = "// jet:scheduler-native-begin";
    let end = "// jet:scheduler-native-end";
    match (src.find(begin), src.find(end)) {
        (Some(b), Some(e)) if e >= b => {
            let mut s = src[..b].to_string();
            s.push_str(&src[e + end.len()..]);
            s
        }
        _ => src.to_string(),
    }
}

/// D-SHAREDGUARD1: drop every vetted `jet:shared-guard-internal` region. A
/// guard's lease holds the matching lock while its sema-proved projection is
/// dereferenced; those casts are runtime internals, not user-reachable unsafe.
pub fn strip_shared_guard_internals(src: &str) -> String {
    let begin = "// jet:shared-guard-internal-begin";
    let end = "// jet:shared-guard-internal-end";
    let mut out = src.to_string();
    loop {
        let (Some(b), Some(e)) = (out.find(begin), out.find(end)) else {
            return out;
        };
        if e < b {
            return out;
        }
        let mut next = out[..b].to_string();
        next.push_str(&out[e + end.len()..]);
        out = next;
    }
}

/// Remove one audited generated-prelude module before checking user code for
/// I1 violations. Generated runtime internals may contain vetted unsafe FFI;
/// user-authored lowering may not.
pub fn strip_vetted_module(src: &str, name: &str) -> String {
    let begin = format!("// JET_VETTED_UNSAFE_BEGIN: {name}");
    let end = format!("// JET_VETTED_UNSAFE_END: {name}");
    // Markers may be indented inside cfg blocks.
    let Some(start) = src.find(&begin).or_else(|| {
        src.lines().enumerate().find_map(|(i, line)| {
            line.trim_start()
                .starts_with(&begin)
                .then(|| src.lines().take(i).map(|l| l.len() + 1).sum::<usize>())
        })
    }) else {
        return src.to_string();
    };
    let Some(relative_end) = src[start..].find(&end) else {
        return src.to_string();
    };
    let end_offset = start + relative_end + end.len();
    format!("{}{}", &src[..start], &src[end_offset..])
}

#[test]
fn vetted_module_stripping_cannot_swallow_following_user_unsafe() {
    let generated = r##"before
// JET_VETTED_UNSAFE_BEGIN: jet_watch_process_probe
#[cfg(windows)]
mod audited { const TEXT: &str = r#"{ comment-like }"#; unsafe { ffi() } }
// JET_VETTED_UNSAFE_END: jet_watch_process_probe
fn user() { unsafe { user_pointer() } }
"##;
    let stripped = strip_vetted_prelude_modules(generated);
    assert!(!stripped.contains("ffi()"));
    assert!(stripped.contains("unsafe { user_pointer() }"));
}
