//! Shared helpers for the integration-test suites (`mod common;`).
//!
//! Each tests/*.rs binary compiles its own copy of this module, so not every
//! suite uses every item — hence the file-level `allow(dead_code)`.

#![allow(dead_code)]

use std::fs;
use std::io::Write as _;
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
/// One-shot: only the first tripper prints. A re-entrant trip (e.g. an
/// allocation made while formatting the message below) or a concurrent
/// tripper on another thread skips straight to `abort()` instead of
/// recursing into a second print.
static GUARD_ABORTING: AtomicBool = AtomicBool::new(false);

/// A cap this large is already indistinguishable from "unbounded" for any
/// real machine; it exists only so a garbage env value (typo, dropped digit)
/// can't overflow the `* 1 GiB` multiply below and wrap into a negative or
/// tiny cap. Real caps are single-digit GB.
const GUARD_CAP_CEILING_GB: i64 = 1 << 20; // ~1 PB

/// Parse + clamp the raw `JET_TEST_ALLOC_CAP_GB` value. Pure and
/// env-independent so it's directly unit-testable — see
/// `garbage_alloc_cap_env_value_clamps_to_a_sane_cap` below.
fn parsed_cap_gb(raw: Option<&str>) -> i64 {
    raw.and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(10)
        .clamp(1, GUARD_CAP_CEILING_GB)
}

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
            let gb = parsed_cap_gb(std::env::var("JET_TEST_ALLOC_CAP_GB").ok().as_deref());
            // Both factors are positive and gb is clamped well below any
            // overflow threshold, but saturating_mul is the boring choice
            // over "trust the clamp" — it can never produce a negative or
            // wrapped cap, which would otherwise silently disarm the guard
            // (see the `cap < 0` branch above).
            GUARD_CAP_BYTES.store(gb.saturating_mul(1 << 30), Ordering::Relaxed);
        }
        return;
    }
    let total = GUARD_LIVE_BYTES.fetch_add(size, Ordering::Relaxed) + size;
    if total > cap {
        guard_abort(|out| {
            let _ = writeln!(
                out,
                "jet test guard: allocation cap {} GB exceeded — aborting; raise JET_TEST_ALLOC_CAP_GB if legitimate",
                cap / (1 << 30)
            );
        });
    }
    guard_start_watchdog();
}

/// Print (first tripper only) then abort. `print` is a closure so each call
/// site can format its own message without heap-allocating a `String` first
/// — `write!` to `Stderr` formats straight into the underlying `write()`
/// syscall.
fn guard_abort(print: impl FnOnce(&mut std::io::Stderr)) -> ! {
    if !GUARD_ABORTING.swap(true, Ordering::AcqRel) {
        print(&mut std::io::stderr());
    }
    std::process::abort();
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

/// The wall-clock budget for one test binary when nothing names it. #677: this
/// number does not move. A suite that outruns it is normally defective — split
/// it or speed it up — and the rare suite that genuinely cannot fit (a
/// measurement suite whose sample count is ratified policy) earns a NAMED row
/// in the table below instead of a bigger number here.
const DEFAULT_SUITE_BUDGET_SECS: u64 = 900;

/// The committed exemption table, embedded rather than read at run time: the
/// guard must not depend on a cwd, and a deleted table becomes a compile error
/// instead of a silently restored default. `tests/suite_membership.rs` reads
/// this same text through this module — never a second copy — and pins how many
/// rows may exceed the default (AGENTS.md I8).
pub const SUITE_BUDGET_LEDGER: &str = include_str!("../suite_budgets.txt");

/// Named once so the abort message, the ledger checks and the runner all cite
/// the same path.
pub const SUITE_BUDGET_LEDGER_PATH: &str = "tests/suite_budgets.txt";

/// One table row: the test target name (`--test <name>`), its budget in
/// seconds, and the one-line reason it is not the default. Borrowed from the
/// ledger text, so resolving a budget allocates nothing but the message.
#[derive(Debug)]
pub struct SuiteBudgetRow<'a> {
    pub suite: &'a str,
    pub secs: u64,
    pub reason: &'a str,
}

/// Parse the table. Strict on purpose: a malformed row is an error naming the
/// line, never a skipped row, because a row nobody parses is an exemption
/// nobody voted for. The guard below treats an unparseable table as "no rows"
/// and so falls back to the strict default; `tests/suite_membership.rs` fails
/// on the same input, which is where the drift gets reported.
pub fn parse_suite_budgets(text: &str) -> Result<Vec<SuiteBudgetRow<'_>>, String> {
    let mut rows = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let number = index + 1;
        let mut fields = line.splitn(3, char::is_whitespace);
        let suite = fields.next().unwrap_or_default();
        let secs = fields.next().unwrap_or_default();
        let reason = fields.next().unwrap_or_default().trim();
        let secs: u64 = secs.parse().map_err(|_| {
            format!(
                "{SUITE_BUDGET_LEDGER_PATH}:{number}: `{secs}` is not a budget in whole seconds. \
                 Row shape: <test target name> <seconds> <reason, one line>"
            )
        })?;
        if reason.is_empty() {
            return Err(format!(
                "{SUITE_BUDGET_LEDGER_PATH}:{number}: the `{suite}` row states no reason. An \
                 exemption without a reason is one nobody can review or retire (#677)"
            ));
        }
        rows.push(SuiteBudgetRow {
            suite,
            secs,
            reason,
        });
    }
    Ok(rows)
}

/// The suite a running test binary belongs to. Cargo names the binary
/// `<target>-<metadata hash>`, so the stem minus that hash is the target name —
/// exactly the spelling `tests/suites.txt` and the budget table use.
pub fn suite_name_from_exe_stem(stem: &str) -> &str {
    match stem.rsplit_once('-') {
        Some((head, hash))
            if !head.is_empty()
                && hash.len() >= 8
                && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            head
        }
        _ => stem,
    }
}

/// Resolve one suite's budget and say where it came from, so the abort can name
/// the rule that bit the reader instead of only the number (#677).
///
/// `JET_TEST_DEADLINE_SECS` may only TIGHTEN: an env var that could loosen is a
/// deadline handed to every target at once by whoever exported it, which is the
/// opposite of a committed, named, reviewable minority. Pure, so the unit tests
/// below drive it with synthetic input instead of the real environment.
fn resolve_suite_budget(suite: &str, ledger: &str, env_raw: Option<&str>) -> (u64, String) {
    let row = parse_suite_budgets(ledger).ok().and_then(|rows| {
        rows.into_iter()
            .find(|row| row.suite == suite)
            .map(|row| (row.secs, row.reason.to_string()))
    });
    let (mut secs, mut source) = match row {
        Some((secs, reason)) => (
            secs,
            format!("the committed `{suite}` row in {SUITE_BUDGET_LEDGER_PATH} — {reason}"),
        ),
        None => (
            DEFAULT_SUITE_BUDGET_SECS,
            format!(
                "the {DEFAULT_SUITE_BUDGET_SECS}s default (no `{suite}` row in \
                 {SUITE_BUDGET_LEDGER_PATH})"
            ),
        ),
    };
    // 1..secs, both ends meaningful: 0 would abort instantly (garbage, not a
    // tightening) and anything at or above the resolved budget is the loosening
    // this law refuses.
    if let Some(requested) = env_raw
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|requested| (1..secs).contains(requested))
    {
        source = format!("JET_TEST_DEADLINE_SECS={requested}, tightening {source}");
        secs = requested;
    }
    (secs, source)
}

fn guard_watchdog_main() {
    // Backstop, not an accommodation: a suite that hits this is itself
    // defective — split or speed it up. The one exception is a measurement
    // suite whose sample count is ratified policy, and that exception is a
    // committed row in the table, naming itself and its reason (#677).
    let stem = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let suite = suite_name_from_exe_stem(&stem);
    let (secs, source) = resolve_suite_budget(
        suite,
        SUITE_BUDGET_LEDGER,
        std::env::var("JET_TEST_DEADLINE_SECS").ok().as_deref(),
    );
    let started = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_secs(secs));
    let elapsed = started.elapsed().as_secs();
    guard_abort(|out| {
        let _ = writeln!(
            out,
            "jet test guard: suite `{suite}` exceeded its {secs}s budget after {elapsed}s — \
             aborting. Budget source: {source}. Split the suite or speed it up; a longer deadline \
             is a committed row in {SUITE_BUDGET_LEDGER_PATH} with its reason, never a raised \
             default (#677)."
        );
    });
}

#[global_allocator]
static JET_TEST_GUARD: GuardedAlloc = GuardedAlloc;

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Reject a test path on the system temp filesystem. On this machine `/tmp` is
/// RAM-backed, so crate scratch and Cargo artifacts there can OOM the agent.
pub fn assert_test_path_on_disk(path: &Path, label: &str) {
    let absolute = |path: &Path| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap().join(path)
        }
    };
    let path = absolute(path);
    let temp = absolute(&std::env::temp_dir());
    let path = fs::canonicalize(&path).unwrap_or(path);
    let temp = fs::canonicalize(&temp).unwrap_or(temp);
    assert!(
        !path.starts_with(&temp),
        "jet test harness refuses {label}={} on RAM-backed temp storage; use a disk path",
        path.display()
    );
}

/// Guard process-wide artifact locations before a battery writes.
pub fn assert_test_environment_is_safe() {
    for (name, value) in [
        ("CARGO_TARGET_DIR", std::env::var_os("CARGO_TARGET_DIR")),
        (
            "JET_TEST_SCRATCH_DIR",
            std::env::var_os("JET_TEST_SCRATCH_DIR"),
        ),
        (
            "JET_DEV_ORACLE_CACHE_DIR",
            std::env::var_os("JET_DEV_ORACLE_CACHE_DIR"),
        ),
    ] {
        if let Some(value) = value {
            assert_test_path_on_disk(&PathBuf::from(value), name);
        }
    }
}

/// Persistent, gitignored test scratch root. `JET_TEST_SCRATCH_DIR` is an
/// explicit override for CI or a local measurement, but it must stay off `/tmp`.
pub fn test_scratch_root(scope: &str) -> PathBuf {
    assert_test_environment_is_safe();
    let root = std::env::var_os("JET_TEST_SCRATCH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".tmp/jet-test-scratch"));
    assert_test_path_on_disk(&root, "JET_TEST_SCRATCH_DIR");
    let path = root.join(scope);
    fs::create_dir_all(&path)
        .unwrap_or_else(|error| panic!("create test scratch root `{}`: {error}", path.display()));
    path
}

/// Collision-safe throwaway dir under `std::env::temp_dir()`: prefix + pid +
/// per-process counter, so concurrent tests in one binary never share a dir.
pub fn unique_tmp(prefix: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), n))
}

// --- interactive example stdin ----------------------------------------------
//
// An interactive example's checked-in golden is only reproducible against the
// answers it was recorded with, and five suites feed those answers
// (`tests/golden.rs`, `tests/terminal.rs`, `tests/jit_run.rs`,
// `tests/corelib_parts/http_data.rs`, and the `tests/dev*.rs` batteries). They
// live here once. A second copy is how a golden and a harness drift apart while
// neither one looks wrong on its own — and a harness that feeds nothing at all
// silently compares a no-input run against a fed-input golden.
//
// #2017 recorded the mechanism as well as the bytes: a harness feeds a CHILD
// process, never an injected reader. The in-process entry points read the test
// binary's own fd 0, shared by every thread of a parallel suite, and an
// injected reader would exercise an input path no user has.

/// The answers one example needs on stdin.
pub struct ExampleStdin {
    /// Answers for a run whose stdin is a pipe or a file. This is what the
    /// checked-in `examples/features/expected/<stem>.out` golden was recorded
    /// against, so it is also what any harness comparing against that golden
    /// owes the program.
    pub piped: &'static str,
    /// The extra answers a run on a real terminal reaches and a piped run does
    /// not. Empty when a terminal run reads exactly what a piped run reads.
    pub tty_only: &'static str,
}

impl ExampleStdin {
    /// Answers for a run on a real terminal.
    pub fn tty(&self) -> String {
        format!("{}{}", self.piped, self.tty_only)
    }
}

/// `io/terminal_parity` answers, in program order:
///   `io.confirm`      — an empty line takes the `[y/N]` default, so `false`;
///   `io.choose`       — `not-a-number` and `3` are both rejected (the golden's
///                       two `Enter a number from 1 to 2.` lines), then `2`
///                       selects `production`;
///   `io.input_secret` — reached only on a terminal. Off a terminal it answers
///                       `Err(InvalidInput)` without consuming a line, which is
///                       the golden's `secret: non-tty`; on a terminal it reads
///                       six characters, which is `secret length: 6`.
const TERMINAL_PARITY_STDIN: ExampleStdin = ExampleStdin {
    piped: "\nnot-a-number\n3\n2\n",
    tty_only: "secret\n",
};

/// `io/terminal` answers: one keystroke for the single `term.read_key()` call
/// inside the example's `live { … }` block.
///
/// Exactly one key, with no trailing newline, on purpose. `read_key` takes up
/// to six bytes per call, so a piped script of several keys would be split by
/// pipe buffering instead of by keystroke and the transcript would depend on
/// the writer's chunking. One key is the same on a pipe and on a terminal, and
/// the example never reads a second time, so a terminal run cannot block on an
/// answer a piped run does not need — `tty_only` is therefore empty.
const TERMINAL_STDIN: ExampleStdin = ExampleStdin {
    piped: "h",
    tty_only: "",
};

/// The stdin an example needs, or `None` for the examples that read nothing.
///
/// Keyed by example stem (`<topic>/<name>`) so a harness that already walks
/// stems asks for the answers instead of naming the interactive examples
/// again. Only an example whose checked-in golden was recorded WITH these
/// answers belongs here: a harness that walks every stem uses this table to
/// decide what to feed, so an entry whose golden is a no-input transcript
/// would make that harness feed the wrong thing.
pub fn example_stdin(stem: &str) -> Option<&'static ExampleStdin> {
    match stem {
        "io/terminal_parity" => Some(&TERMINAL_PARITY_STDIN),
        "io/terminal" => Some(&TERMINAL_STDIN),
        _ => None,
    }
}

/// Run the built `jet` binary with `answers` on stdin and collect its output.
///
/// A program that reads stdin needs a child process to read from. The
/// in-process entry points (`dev_iteration`, `CraneliftBackend::run`) read the
/// test binary's own fd 0, which every other test on every other thread
/// shares, so there is nothing per-run to redirect there.
pub fn jet_cli_output_with_stdin(args: &[&str], answers: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("jet CLI must start");
    child
        .stdin
        .take()
        .expect("piped jet stdin")
        .write_all(answers.as_bytes())
        .expect("write jet stdin answers");
    child.wait_with_output().expect("collect jet CLI output")
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
    // #2004 — DECISION: a raw `rustc --test` build of generated Jet output is
    // NOT a supported mode, so this helper refuses it by name instead of
    // letting it fail 10,000 lines deep in someone else's fragment.
    //
    // `--test` asks rustc to synthesize a harness from every `#[cfg(test)]`
    // module in the crate. The generated crate is one flat splice of the
    // Prelude, so that switch activates the COMPILER's private fragment unit
    // tests inside a user program — a set that is not the program's tests, is
    // not shipped, and whose only shared namespace is the generated crate root.
    // The supported way to run Jet tests is `jet test`, which builds its own
    // harness (TEST_PRELUDE / `jet_test_print`). A probe that needs to observe
    // generated internals hooks the generated `main` instead — see
    // `tests/dev.rs::io_style_raw_nonunicode_no_color_uses_presence_semantics`.
    assert!(
        !rustc_flags.contains(&"--test"),
        "`rustc --test` over generated Jet output is not a supported build mode: it \
         synthesizes a harness from the Prelude's own `#[cfg(test)]` modules inside a \
         user program. Run Jet tests with `jet test`, or hook the generated `main` for \
         an inspection probe."
    );
    let prepared = if has_rust_ffi {
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
        .arg("--crate-name")
        .arg(jet::Syntax::sanitize_crate_name(
            path.file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("out"),
        ))
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
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
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

/// Text that only appears when a Rust control transfer killed a process instead
/// of becoming a Jet report (R13 / D-JITUNWIND1, #1995 / #1997).
///
/// One list, because the two checks that need it must not drift: the JIT
/// boundary suite (`tests/jit_no_unwind_boundary.rs`) asserts a cancelled task
/// never aborts, and the example-corpus gate refuses to *classify* an abort at
/// all. `streams/generators` is why the second one exists — it died with
/// `panic in a destructor during cleanup` and the gate filed it as the benign
/// row `AOT exit 1`, so an abort became a shrink-only ledger entry instead of a
/// failure.
///
/// The four families, which are four different defects:
///
/// * `failed to initiate panic` is `_URC_END_OF_STACK`: phase 1 walked off the
///   top of the stack without finding a handler, because `cranelift-jit`
///   registers no unwind information for the code it emits.
/// * `non-unwinding panic` / `panic in a function that cannot unwind` is an
///   `extern "C"` body's own abort-on-unwind shim.
/// * `panic in a destructor during cleanup` is a second raise from drop glue
///   while an unwind is already in flight (#2007).
/// * `fatal runtime error: stack overflow` is real stack exhaustion — the only
///   one `jet_foundation::CompilerStack::COMPILER_STACK_SIZE` can answer for.
///   Listed so a failure names which abort it saw rather than guessing.
pub const ABORT_MARKERS: &[&str] = &[
    "failed to initiate panic",
    "non-unwinding panic",
    "panic in a function that cannot unwind",
    "panic in a destructor during cleanup",
    "core::panicking",
    "fatal runtime error: stack overflow",
];

/// The abort marker `stderr` carries, if any.
pub fn abort_marker(stderr: &str) -> Option<&'static str> {
    ABORT_MARKERS
        .iter()
        .copied()
        .find(|marker| stderr.contains(marker))
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

/// #2075: the bridge cache key in an FFI artifact's file name
/// (`jet-crypto-helper-<key>`, `libjet_ffi_<key>.rlib`, `jet_ffi_<key>.sha256`).
///
/// Every key's outputs now live in ONE Cargo target dir shared per (toolchain,
/// target, profile), so "this bridge's files" is a name filter, not a directory
/// listing.
pub fn ffi_bridge_key(artifact: &Path) -> String {
    let name = artifact
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    name.split(|c: char| !c.is_ascii_hexdigit())
        .find(|part| part.len() == 16)
        .unwrap_or_else(|| panic!("no 16-hex FFI bridge key in `{name}`"))
        .to_string()
}

/// The per-key input dir (`<ffi cache>/<key>/`, holding the generated
/// `Cargo.toml` and `src/`) of the bridge that produced `artifact`.
pub fn ffi_bridge_cache_root(artifact: &Path) -> PathBuf {
    artifact
        .ancestors()
        .find(|dir| dir.ends_with("ffi"))
        .unwrap_or_else(|| panic!("`{}` is not under an FFI cache root", artifact.display()))
        .join(ffi_bridge_key(artifact))
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
/// __jet___c_* CFFI overlay module) before checking generated Rust for I1
/// violations. This shared structural helper is used by golden and sema/TIR
/// checks so every I1 scan removes the same vetted source.
pub fn strip_vetted_prelude_modules(rust_code: &str) -> String {
    fn strip_named_mod(src: &str, matches_name: impl Fn(&str) -> bool) -> String {
        fn ident_continue(byte: u8) -> bool {
            byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
        }

        fn raw_string_start(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
            let mut i = at;
            if bytes.get(i) == Some(&b'b') {
                if bytes.get(i + 1) != Some(&b'r') {
                    return None;
                }
                i += 1;
            } else if bytes.get(i) != Some(&b'r') {
                return None;
            }
            i += 1;
            let mut hashes = 0;
            while bytes.get(i) == Some(&b'#') {
                hashes += 1;
                i += 1;
            }
            (bytes.get(i) == Some(&b'"')).then_some((i + 1, hashes))
        }

        fn starts_char_literal(bytes: &[u8], at: usize) -> bool {
            let Some(next) = bytes.get(at + 1).copied() else {
                return false;
            };
            next == b'\\' || !ident_continue(next) || bytes.get(at + 2) == Some(&b'\'')
        }

        fn matching_brace_end(bytes: &[u8], opening: usize) -> Option<usize> {
            #[derive(Clone, Copy)]
            enum State {
                Normal,
                LineComment,
                BlockComment(usize),
                String,
                Char,
                Raw(usize),
            }

            let mut state = State::Normal;
            let mut depth = 0usize;
            let mut i = opening;
            while i < bytes.len() {
                match state {
                    State::Normal => match bytes[i] {
                        b'/' if bytes.get(i + 1) == Some(&b'/') => {
                            state = State::LineComment;
                            i += 2;
                        }
                        b'/' if bytes.get(i + 1) == Some(&b'*') => {
                            state = State::BlockComment(1);
                            i += 2;
                        }
                        _ if raw_string_start(bytes, i).is_some() => {
                            let (after_open, hashes) = raw_string_start(bytes, i).unwrap();
                            state = State::Raw(hashes);
                            i = after_open;
                        }
                        b'"' => {
                            state = State::String;
                            i += 1;
                        }
                        b'\'' if starts_char_literal(bytes, i) => {
                            state = State::Char;
                            i += 1;
                        }
                        b'{' => {
                            depth += 1;
                            i += 1;
                        }
                        b'}' => {
                            depth = depth.checked_sub(1)?;
                            i += 1;
                            if depth == 0 {
                                return Some(i);
                            }
                        }
                        _ => i += 1,
                    },
                    State::LineComment => {
                        if bytes[i] == b'\n' {
                            state = State::Normal;
                        }
                        i += 1;
                    }
                    State::BlockComment(mut nested) => {
                        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                            nested += 1;
                            state = State::BlockComment(nested);
                            i += 2;
                        } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                            nested -= 1;
                            i += 2;
                            if nested == 0 {
                                state = State::Normal;
                            } else {
                                state = State::BlockComment(nested);
                            }
                        } else {
                            i += 1;
                        }
                    }
                    State::String | State::Char => {
                        if bytes[i] == b'\\' {
                            i = (i + 2).min(bytes.len());
                        } else {
                            let closing = matches!(state, State::String) && bytes[i] == b'"'
                                || matches!(state, State::Char) && bytes[i] == b'\'';
                            i += 1;
                            if closing {
                                state = State::Normal;
                            }
                        }
                    }
                    State::Raw(hashes) => {
                        if bytes[i] == b'"'
                            && bytes
                                .get(i + 1..i + 1 + hashes)
                                .is_some_and(|tail| tail.iter().all(|&byte| byte == b'#'))
                        {
                            i += hashes + 1;
                            state = State::Normal;
                        } else {
                            i += 1;
                        }
                    }
                }
            }
            None
        }

        fn item_start(src: &str, module_start: usize) -> usize {
            let line_start = |at: usize| src[..at].rfind('\n').map_or(0, |pos| pos + 1);
            let current_line = line_start(module_start);
            let prefix = src[current_line..module_start].trim_start();
            let mut start =
                if prefix.is_empty() || prefix.starts_with("pub") || prefix.starts_with("#[") {
                    current_line
                } else {
                    module_start
                };
            while start > 0 {
                let previous_line = line_start(start.saturating_sub(1));
                let previous = src[previous_line..start].trim();
                if previous.starts_with("#[")
                    || previous.starts_with("///")
                    || previous.starts_with("//!")
                {
                    start = previous_line;
                } else if previous.ends_with(']') {
                    // An outer attribute may span several lines. Walk back to
                    // its `#[` line before deciding where the item starts.
                    let mut candidate_end = start;
                    let mut attribute_start = None;
                    while candidate_end > 0 {
                        let candidate_line = line_start(candidate_end.saturating_sub(1));
                        let text = src[candidate_line..candidate_end].trim();
                        if text.starts_with("#[") {
                            attribute_start = Some(candidate_line);
                            break;
                        }
                        if text.is_empty() {
                            break;
                        }
                        candidate_end = candidate_line;
                    }
                    if let Some(attribute_start) = attribute_start {
                        start = attribute_start;
                    } else {
                        break;
                    }
                } else if previous.ends_with("*/") {
                    // Preserve a preceding block documentation comment with
                    // the item it documents.
                    let mut candidate_end = start;
                    let mut doc_start = None;
                    while candidate_end > 0 {
                        let candidate_line = line_start(candidate_end.saturating_sub(1));
                        let text = src[candidate_line..candidate_end].trim();
                        if text.starts_with("/**") || text.starts_with("/*!") {
                            doc_start = Some(candidate_line);
                            break;
                        }
                        if text.is_empty() {
                            break;
                        }
                        candidate_end = candidate_line;
                    }
                    if let Some(doc_start) = doc_start {
                        start = doc_start;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            start
        }

        fn skip_trivia(bytes: &[u8], mut at: usize) -> usize {
            loop {
                while bytes.get(at).is_some_and(|byte| byte.is_ascii_whitespace()) {
                    at += 1;
                }
                if bytes.get(at) == Some(&b'/') && bytes.get(at + 1) == Some(&b'/') {
                    at += 2;
                    while bytes.get(at).is_some_and(|byte| *byte != b'\n') {
                        at += 1;
                    }
                    continue;
                }
                if bytes.get(at) == Some(&b'/') && bytes.get(at + 1) == Some(&b'*') {
                    let mut nested = 1usize;
                    at += 2;
                    while at < bytes.len() && nested > 0 {
                        if bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'*') {
                            nested += 1;
                            at += 2;
                        } else if bytes[at] == b'*' && bytes.get(at + 1) == Some(&b'/') {
                            nested -= 1;
                            at += 2;
                        } else {
                            at += 1;
                        }
                    }
                    continue;
                }
                return at;
            }
        }

        #[derive(Clone, Copy)]
        enum State {
            Normal,
            LineComment,
            BlockComment(usize),
            String,
            Char,
            Raw(usize),
        }

        let bytes = src.as_bytes();
        let mut state = State::Normal;
        let mut i = 0usize;
        let mut brace_depth = 0usize;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut module = None;
        while i < bytes.len() {
            match state {
                State::Normal => match bytes[i] {
                    b'/' if bytes.get(i + 1) == Some(&b'/') => {
                        state = State::LineComment;
                        i += 2;
                    }
                    b'/' if bytes.get(i + 1) == Some(&b'*') => {
                        state = State::BlockComment(1);
                        i += 2;
                    }
                    _ if raw_string_start(bytes, i).is_some() => {
                        let (after_open, hashes) = raw_string_start(bytes, i).unwrap();
                        state = State::Raw(hashes);
                        i = after_open;
                    }
                    b'"' => {
                        state = State::String;
                        i += 1;
                    }
                    b'\'' if starts_char_literal(bytes, i) => {
                        state = State::Char;
                        i += 1;
                    }
                    b'{' => {
                        brace_depth += 1;
                        i += 1;
                    }
                    b'}' => {
                        brace_depth = brace_depth.saturating_sub(1);
                        i += 1;
                    }
                    b'(' => {
                        paren_depth += 1;
                        i += 1;
                    }
                    b')' => {
                        paren_depth = paren_depth.saturating_sub(1);
                        i += 1;
                    }
                    b'[' => {
                        bracket_depth += 1;
                        i += 1;
                    }
                    b']' => {
                        bracket_depth = bracket_depth.saturating_sub(1);
                        i += 1;
                    }
                    b'm' if brace_depth == 0
                        && paren_depth == 0
                        && bracket_depth == 0
                        && bytes.get(i + 1..i + 3) == Some(b"od")
                        && (i == 0 || !ident_continue(bytes[i - 1]))
                        && bytes.get(i + 3).is_none_or(|byte| !ident_continue(*byte)) =>
                    {
                        let mut j = i + 3;
                        while bytes.get(j).is_some_and(|byte| byte.is_ascii_whitespace()) {
                            j += 1;
                        }
                        let name_start = j;
                        while bytes.get(j).is_some_and(|byte| ident_continue(*byte)) {
                            j += 1;
                        }
                        if matches_name(&src[name_start..j]) {
                            j = skip_trivia(bytes, j);
                            if bytes.get(j) == Some(&b'{') {
                                module = Some((item_start(src, i), j, true));
                                break;
                            }
                            if bytes.get(j) == Some(&b';') {
                                module = Some((item_start(src, i), j + 1, false));
                                break;
                            }
                        }
                        i += 1;
                    }
                    _ => i += 1,
                },
                State::LineComment => {
                    if bytes[i] == b'\n' {
                        state = State::Normal;
                    }
                    i += 1;
                }
                State::BlockComment(mut nested) => {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        nested += 1;
                        state = State::BlockComment(nested);
                        i += 2;
                    } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        nested -= 1;
                        i += 2;
                        if nested == 0 {
                            state = State::Normal;
                        } else {
                            state = State::BlockComment(nested);
                        }
                    } else {
                        i += 1;
                    }
                }
                State::String | State::Char => {
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(bytes.len());
                    } else {
                        let closing = matches!(state, State::String) && bytes[i] == b'"'
                            || matches!(state, State::Char) && bytes[i] == b'\'';
                        i += 1;
                        if closing {
                            state = State::Normal;
                        }
                    }
                }
                State::Raw(hashes) => {
                    if bytes[i] == b'"'
                        && bytes
                            .get(i + 1..i + 1 + hashes)
                            .is_some_and(|tail| tail.iter().all(|&byte| byte == b'#'))
                    {
                        i += hashes + 1;
                        state = State::Normal;
                    } else {
                        i += 1;
                    }
                }
            }
        }
        let Some((start, end_or_opening, has_body)) = module else {
            return src.to_string();
        };
        let end = if has_body {
            let Some(end) = matching_brace_end(bytes, end_or_opening) else {
                return src.to_string();
            };
            end
        } else {
            end_or_opening
        };
        format!("{}{}", &src[..start], &src[end..])
    }

    fn strip_mod(src: &str, name: &str) -> String {
        strip_named_mod(src, |candidate| candidate == name)
    }

    fn strip_mod_prefix(src: &str, prefix: &str) -> String {
        strip_named_mod(src, |candidate| candidate.starts_with(prefix))
    }
    let s = strip_mod(rust_code, "jet_uninit_semantics");
    let s = strip_mod(&s, "jet_mem");
    let s = strip_vetted_module(&s, "jet_cell");
    let s = strip_mod(&s, "jet_txn");
    let s = strip_mod(&s, "jet_term_unix");
    let s = strip_mod(&s, "jet_term_windows");
    let mut s = strip_mod(&s, "jet_term_mode");
    loop {
        let next = strip_mod(&s, "jet_term_mode");
        if next == s {
            break;
        }
        s = next;
    }
    let s = strip_mod(&s, "jet_process_pty");
    let s = strip_mod(&s, "jet_os_unix");
    let s = strip_mod(&s, "jet_atomic_windows");
    let s = strip_mod(&s, "jet_gtk");
    let s = strip_mod(&s, "jet_crypto_entropy");
    let mut s = strip_scheduler_native(&s);
    s = strip_shared_guard_internals(&s);
    s = strip_vetted_module(&s, "jet_os_extra");
    s = strip_vetted_module(&s, "jet_env_windows");
    s = strip_vetted_module(&s, "jet_watch_process_probe");
    s = strip_vetted_module(&s, "jet_atomic_windows");
    s = strip_vetted_module(&s, "jet_ws_upgrade");
    // D-TASKBORROW1=A: canonical task-group lifetime erasure (mirrors golden.rs).
    s = strip_vetted_module(&s, "jet_taskgroup_borrowed_spawn");
    s = strip_vetted_module(&s, "jet_compute_cpu_simd");
    s = strip_vetted_module(&s, "jet_regex_cpu_simd_dispatch");
    s = strip_vetted_module(&s, "jet_regex_cpu_simd");
    s = strip_vetted_module(&s, "ffi_reporter");
    s = strip_vetted_module(&s, "jet_program_allocator");
    s = strip_vetted_module(&s, "jet_mod_native");
    loop {
        let next = strip_vetted_module(&s, "jet_mod_native");
        if next == s {
            break;
        }
        s = next;
    }
    s = strip_vetted_module(&s, "jet_os_interrupt_ffi");
    s = strip_mod(&s, "jet_os_interrupt");
    s = strip_raylib_bridge(&s);
    while s.contains("mod __jet___c_") {
        let before = s.clone();
        s = strip_mod_prefix(&s, "__jet___c_");
        if s == before {
            break;
        }
    }
    s
}

/// Byte columns of every real `unsafe` KEYWORD on one line of generated Rust.
///
/// #2025: the I1 scans used to ask `line.contains("unsafe")`, which reads DATA
/// as CODE. Generated Rust embeds the program's own source path and its string
/// literals verbatim — `crate::jet_stack_enter("examples/features/memory/
/// unsafe_sentries.jet", …)`, `jet_mem::jet_sentry_scope(true, "…/
/// unsafe_obligations.jet", …)`, `print("unsafe gate")` — so four examples were
/// permanently red for owning the word `unsafe` in their FILE NAME, and
/// `tests/sema_soundness_parts/provenance.rs` had to keep its scratch-dir prefix
/// free of the substring to avoid false-tripping every case. The property being
/// checked is about the `unsafe` keyword, so scan for the keyword: skip `//`
/// comments, string and char literals, and identifiers that merely contain the
/// word (`unsafe_sentries`, `jet_unsafe_probe`).
///
/// Line-scoped, like the checks it serves: a generated statement is one line.
pub fn unsafe_keyword_columns(line: &str) -> Vec<usize> {
    let bytes = line.as_bytes();
    let ident_byte = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80;
    let mut columns = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => return columns,
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            // A char literal, not a lifetime (`'a`) and not an apostrophe inside
            // one: `'x'`, `'\n'`. Anything else advances one byte.
            b'\'' if bytes.get(i + 2) == Some(&b'\'') || bytes.get(i + 1) == Some(&b'\\') => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'\'' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'u' if bytes[i..].starts_with(b"unsafe")
                && (i == 0 || !ident_byte(bytes[i - 1]))
                && bytes.get(i + 6).is_none_or(|byte| !ident_byte(*byte)) =>
            {
                columns.push(i);
                i += 6;
            }
            _ => i += 1,
        }
    }
    columns
}

/// #2025 negative control: the keyword scan tells generated CODE from generated
/// DATA. Every string below is a real line from `jet emit --rust` output.
#[test]
fn unsafe_keyword_scan_reads_code_not_data() {
    let path_argument = "    let __jet_stack_frame = crate::jet_stack_enter(\"examples/features/memory/unsafe_sentries.jet\", 5, \"run\", \"fn run() {\");";
    assert!(
        unsafe_keyword_columns(path_argument).is_empty(),
        "an example's own path in a stack-frame argument is data, not an `unsafe` block"
    );
    let sentry = "        let _jet_sentry = jet_mem::jet_sentry_scope(true, \"examples/features/lowlevel/unsafe_obligations.jet\", 5, \"cell stays live\");";
    assert!(
        unsafe_keyword_columns(sentry).is_empty(),
        "sentry reason strings are data"
    );
    let user_string = "        { let _ = jet_term_write_stdout_line(&((\"unsafe gate\".to_string()).jet_show()), false); };";
    assert!(
        unsafe_keyword_columns(user_string).is_empty(),
        "a printed string is data"
    );
    assert!(unsafe_keyword_columns("// unsafe { … } in a comment").is_empty());
    assert!(
        unsafe_keyword_columns("    let unsafe_flag = 1;").is_empty(),
        "identifier, not keyword"
    );

    assert_eq!(
        unsafe_keyword_columns("    unsafe {").len(),
        1,
        "the gated block form must count"
    );
    assert_eq!(
        unsafe_keyword_columns("unsafe extern \"C\" fn InitWindow() {}").len(),
        1,
        "an `unsafe extern` item must count even though the line also holds a string"
    );
    assert_eq!(
        unsafe_keyword_columns("    let p = unsafe { *raw }; // unsafe read").len(),
        1,
        "code counts once; the trailing comment does not add a second"
    );
}

/// Remove the vetted `jet:scheduler-native` region (raw epoll/kqueue syscalls,
/// the only `unsafe` in the emitted scheduler — Tower #126) before an I1 scan,
/// exactly as `golden.rs::strip_vetted_prelude_modules` does.
pub fn strip_scheduler_native(src: &str) -> String {
    let begin = "// jet:scheduler-native-begin";
    let end = "// jet:scheduler-native-end";
    let mut src = src.to_string();
    loop {
        match (src.find(begin), src.find(end)) {
            (Some(b), Some(e)) if e >= b => {
                let mut s = src[..b].to_string();
                s.push_str(&src[e + end.len()..]);
                src = s;
            }
            _ => return src,
        }
    }
}

/// D-RAYLIB1: drop the always-emitted kernel raylib FFI region. Bridge unsafe
/// lives between `jet:raylib-begin` / `jet:raylib-end`; user functions stay
/// in the scan.
pub fn strip_raylib_bridge(src: &str) -> String {
    let begin = "// jet:raylib-begin";
    let end = "// jet:raylib-end";
    match (src.find(begin), src.find(end)) {
        (Some(b), Some(e)) if e >= b => {
            let mut out = src[..b].to_string();
            out.push_str(&src[e + end.len()..]);
            out
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
    let mut src = src.to_string();
    loop {
        // Markers may be indented inside cfg blocks.
        let Some(start) = src.find(&begin).or_else(|| {
            src.lines().enumerate().find_map(|(i, line)| {
                line.trim_start()
                    .starts_with(&begin)
                    .then(|| src.lines().take(i).map(|l| l.len() + 1).sum::<usize>())
            })
        }) else {
            return src;
        };
        let Some(relative_end) = src[start..].find(&end) else {
            return src;
        };
        let end_offset = start + relative_end + end.len();
        src = format!("{}{}", &src[..start], &src[end_offset..]);
    }
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

#[test]
fn vetted_module_stripping_tracks_nested_block_comments() {
    let generated = r#"
mod jet_mem {
    /* outer comment /* nested comment with } */ still outer */
    const VALUE: i32 = 1;
}
unsafe { user_pointer() }
"#;
    let stripped = strip_vetted_prelude_modules(generated);
    assert!(!stripped.contains("mod jet_mem"));
    assert!(stripped.contains("unsafe { user_pointer() }"));
}

#[test]
fn term_mode_stripping_removes_all_platform_variants_without_hiding_user_unsafe() {
    let generated = r#"
#[cfg(unix)]
mod jet_term_mode { unsafe { unix_ffi() } }
#[cfg(windows)]
mod jet_term_mode { unsafe { windows_ffi() } }
#[cfg(not(any(unix, windows)))]
mod jet_term_mode { unsafe { other_ffi() } }
unsafe { user_pointer() }
"#;
    let stripped = strip_vetted_prelude_modules(generated);
    assert!(!stripped.contains("unix_ffi()"));
    assert!(!stripped.contains("windows_ffi()"));
    assert!(!stripped.contains("other_ffi()"));
    assert!(stripped.contains("unsafe { user_pointer() }"));
}

#[test]
fn raylib_bridge_stripping_cannot_swallow_following_user_unsafe() {
    let generated = "// jet:raylib-begin\nunsafe extern \"C\" fn InitWindow() {}\n// jet:raylib-end\nunsafe { user_pointer() }";
    let stripped = strip_vetted_prelude_modules(generated);
    assert!(!stripped.contains("InitWindow"));
    assert!(stripped.contains("unsafe { user_pointer() }"));
}

#[test]
fn garbage_alloc_cap_env_value_clamps_to_a_sane_cap() {
    // Before the fix: `gb * (1 << 30)` on a raw value this large overflows
    // i64 (panics in debug, wraps toward negative in release) and the
    // resulting negative cap satisfies `cap < 0` forever, silently disarming
    // accounting for the rest of the process. It must clamp instead.
    let gb = parsed_cap_gb(Some("999999999999999999"));
    assert!(
        (1..=GUARD_CAP_CEILING_GB).contains(&gb),
        "gb={gb} not clamped"
    );
    let bytes = gb.saturating_mul(1 << 30);
    assert!(bytes > 0, "cap bytes must stay positive, got {bytes}");

    // A missing/unparseable value still gets the documented default.
    assert_eq!(parsed_cap_gb(None), 10);
    assert_eq!(parsed_cap_gb(Some("not a number")), 10);
    assert_eq!(
        parsed_cap_gb(Some("0")),
        1,
        "0 GB must clamp up to the 1 GB floor"
    );
}

#[test]
fn suite_budget_defaults_to_900s_and_env_can_only_tighten() {
    // #677: the table is the only way up. Everything unlisted keeps the flat
    // default, and the abort names which of the two rules applied.
    let ledger = "# comment\nslow_measurement 5400 ratified sample policy\n";

    let (secs, source) = resolve_suite_budget("some_unit_suite", ledger, None);
    assert_eq!(secs, 900, "an unlisted suite keeps the 900s default");
    assert!(source.contains("900s default"), "source: {source}");
    assert!(
        source.contains("no `some_unit_suite` row"),
        "source: {source}"
    );

    let (secs, source) = resolve_suite_budget("slow_measurement", ledger, None);
    assert_eq!(secs, 5400, "a named row is the suite's budget");
    assert!(
        source.contains("committed `slow_measurement` row"),
        "source: {source}"
    );
    assert!(
        source.contains("ratified sample policy"),
        "the reason travels: {source}"
    );

    // Tightening is the env var's whole job — the guard self-test runs on it.
    let (secs, source) = resolve_suite_budget("some_unit_suite", ledger, Some("3"));
    assert_eq!(secs, 3);
    assert!(
        source.starts_with("JET_TEST_DEADLINE_SECS=3, tightening"),
        "source: {source}"
    );

    // Loosening is not. An exported deadline is granted to every target at
    // once, which is exactly the drift the committed table exists to stop.
    assert_eq!(
        resolve_suite_budget("some_unit_suite", ledger, Some("36000")).0,
        900
    );
    assert_eq!(
        resolve_suite_budget("slow_measurement", ledger, Some("36000")).0,
        5400
    );
    assert_eq!(
        resolve_suite_budget("some_unit_suite", ledger, Some("0")).0,
        900,
        "a zero deadline would abort instantly; it is garbage, not a tightening"
    );

    // A malformed table falls back to the strict default rather than to the
    // last row that happened to parse. tests/suite_membership.rs is what turns
    // that fallback into a reported failure.
    assert_eq!(
        resolve_suite_budget("slow_measurement", "slow_measurement lots", None).0,
        900
    );
    assert_eq!(
        resolve_suite_budget("slow_measurement", "slow_measurement 5400", None).0,
        900
    );
}

#[test]
fn suite_name_comes_from_the_test_binary_cargo_built() {
    // Cargo runs `target/debug/deps/<target>-<metadata hash>`; the budget table
    // and tests/suites.txt both spell the target name, so the hash comes off.
    assert_eq!(
        suite_name_from_exe_stem("cli_compile_latency-1a2b3c4d5e6f7a8b"),
        "cli_compile_latency"
    );
    // Run by hand, or by a runner that copied the binary: no hash to strip.
    assert_eq!(
        suite_name_from_exe_stem("cli_compile_latency"),
        "cli_compile_latency"
    );
    // A trailing word that is not a metadata hash is part of the name.
    assert_eq!(suite_name_from_exe_stem("cli-runtime"), "cli-runtime");
    assert_eq!(
        suite_name_from_exe_stem("-1a2b3c4d5e6f7a8b"),
        "-1a2b3c4d5e6f7a8b"
    );
}

#[test]
fn the_committed_suite_budget_table_parses() {
    // The guard reads this table in a watchdog thread and falls back to the
    // default when it cannot parse it, so a typo would otherwise cost a suite
    // its exemption silently. Every suite binary carries this check.
    let rows = parse_suite_budgets(SUITE_BUDGET_LEDGER)
        .unwrap_or_else(|err| panic!("{SUITE_BUDGET_LEDGER_PATH} does not parse: {err}"));
    for row in &rows {
        assert!(
            row.secs > DEFAULT_SUITE_BUDGET_SECS,
            "{SUITE_BUDGET_LEDGER_PATH}: `{}` asks for {}s, at or under the \
             {DEFAULT_SUITE_BUDGET_SECS}s default. A row that buys nothing is a row to delete.",
            row.suite,
            row.secs
        );
    }
}
