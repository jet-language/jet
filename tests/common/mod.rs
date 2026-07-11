//! Shared helpers for the integration-test suites (`mod common;`).
//!
//! Each tests/*.rs binary compiles its own copy of this module, so not every
//! suite uses every item — hence the file-level `allow(dead_code)`.

#![allow(dead_code)]

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

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
    let present = Command::new("rustc").arg("--version").output().is_ok();
    if !present && std::env::var("JET_REQUIRE_RUSTC").as_deref() == Ok("1") {
        panic!(
            "JET_REQUIRE_RUSTC=1 but rustc not found on PATH — refusing to \
             silently skip I2 (rustc-must-accept) coverage. Fix the CI \
             environment; do not unset JET_REQUIRE_RUSTC to paper over this."
        );
    }
    present
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
    assert!(!path.is_absolute(), "{var} must be repository-relative: {raw}");
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
    for line in prefix.split_inclusive('\n').take(UNIFIED_DIFF_MAX_INPUT_LINES) {
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

fn push_diff_line(
    out: &mut BoundedDiffOutput,
    marker: char,
    line: &str,
    cut_mid_line: bool,
) {
    let mut marker_bytes = [0; 4];
    out.push(marker.encode_utf8(&mut marker_bytes));
    out.push(line);
    if !line.ends_with('\n') && !cut_mid_line {
        out.push("\n\\ No newline at end of file\n");
    }
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
    let s = strip_mod(rust_code, "jet_mem");
    let s = strip_mod(&s, "jet_txn");
    let s = strip_mod(&s, "jet_term_unix");
    let s = strip_mod(&s, "jet_term_windows");
    let s = strip_mod(&s, "jet_os_unix");
    let s = strip_mod(&s, "jet_atomic_windows");
    let s = strip_mod(&s, "jet_gtk");
    let mut s = strip_scheduler_native(&s);
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

/// Remove one audited generated-prelude module before checking user code for
/// I1 violations. Generated runtime internals may contain vetted unsafe FFI;
/// user-authored lowering may not.
pub fn strip_vetted_module(src: &str, name: &str) -> String {
    let begin = format!("// JET_VETTED_UNSAFE_BEGIN: {name}");
    let end = format!("// JET_VETTED_UNSAFE_END: {name}");
    let Some(start) = src.find(&begin) else {
        return src.to_string();
    };
    let Some(relative_end) = src[start + begin.len()..].find(&end) else {
        return src.to_string();
    };
    let end_offset = start + begin.len() + relative_end + end.len();
    format!("{}{}", &src[..start], &src[end_offset..])
}

#[test]
fn vetted_module_stripping_cannot_swallow_following_user_unsafe() {
    let generated = r##"before
// JET_VETTED_UNSAFE_BEGIN: audited
#[cfg(windows)]
mod audited { const TEXT: &str = r#"{ comment-like }"#; unsafe { ffi() } }
// JET_VETTED_UNSAFE_END: audited
fn user() { unsafe { user_pointer() } }
"##;
    let stripped = strip_vetted_module(generated, "audited");
    assert!(!stripped.contains("ffi()"));
    assert!(stripped.contains("unsafe { user_pointer() }"));
}
