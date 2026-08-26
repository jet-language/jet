// Shared helpers for every `tests/dev*.rs` target (`include!`d, never a module
// of its own — see tests/cli_parts and tests/corelib_parts for the same shape).
//
// #2020: `tests/dev.rs` used to be one 11.8k-line binary carrying all 166 of
// these tests. Its whole-corpus batteries run serially against one
// `dev_diff_lock`, so the binary needed longer than the 900s suite guard and
// aborted with about 30 of its own declared tests never started — every pass/fail
// number it ever printed was a partial measurement. The guard is right and the
// budget is not raisable (`scripts/agent/time-suites.sh` refuses anything over
// 900), so the cost is split across targets instead: one routine target plus one
// per whole-corpus battery. This file is the single copy of everything they
// share, so the split cannot fork a helper into two versions (AGENTS.md I8).

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use common::{add_generated_rust, have_rustc, panic_message, test_worker_count};
use jet::Interpreter::{dev_iteration, dev_run_bundle, run_jit_once, run_named_job, RunOutcome};
use jet::JitBackend::JitBackend;
use jet_jit::CraneliftBackend;

// A stem that has never been built pays a cold AOT compile, and this battery
// runs eight of them at once. Thirty seconds was a warm-cache number, so a
// cold or freshly pruned runtime cache turned real passes into timeouts.
// JET_TIER_PARITY_TIMEOUT_SECS raises it without hiding a genuine hang.
static DEV_DIFF_TIMEOUT: std::sync::LazyLock<Duration> = std::sync::LazyLock::new(|| {
    Duration::from_secs(
        std::env::var("JET_TIER_PARITY_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30),
    )
});

fn dev_diff_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Acquire a harness mutex whose guarded value CANNOT be left half-written by an
/// unwind, and say out loud when the lock was already poisoned.
///
/// Every dev test serializes on `dev_diff_lock`, so one test that unwinds while
/// holding it poisons the lock for the whole binary. With `.lock().unwrap()`
/// each later test then dies with a bare `PoisonError { .. }` reported at *its
/// own* line, so a single real defect manufactures an unbounded run of false
/// failures — the same slander the old abort-on-first-failure behavior produced.
///
/// `PoisonError::into_inner()` is sound at these sites because the guarded value
/// has no multi-step invariant to break: `dev_diff_lock` guards `()`, the work
/// queues are only ever `pop_front`ed under the guard, and the report vectors are
/// only ever `push`ed. `VecDeque::pop_front` and `Vec::push` either complete or
/// do nothing, so no torn state can outlive an unwind.
///
/// The poison flag is deliberately NOT cleared: every later acquisition keeps
/// tagging its test as post-cascade. This changes attribution only — the calling
/// test still stands or falls on its own assertions.
fn lock_recovered<'a, T>(lock: &'a Mutex<T>, what: &str) -> MutexGuard<'a, T> {
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!(
                "note: cascade, not a fresh defect — `{what}` was poisoned by an EARLIER panic \
                 in this test binary. The guard holds no partially written state, so it is \
                 recovered and this test keeps its own verdict; fix the first panic of the run."
            );
            poisoned.into_inner()
        }
    }
}

/// Read a worker-collected report the caller is about to assert on.
///
/// `into_inner()` is WRONG here. Poison means a worker thread unwound while
/// holding the report, so the row that is missing is exactly the row that would
/// have failed the test, and `assert!(report.is_empty())` over a recovered
/// report can go GREEN across a real defect. Fail instead, name the cascade, and
/// print the rows that survived so the first panic stays visible.
fn judged_report<'a>(lock: &'a Mutex<Vec<String>>, what: &str) -> MutexGuard<'a, Vec<String>> {
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            let partial = poisoned.into_inner();
            panic!(
                "cascade, not a fresh defect: `{what}` was poisoned — a worker of this test \
                 unwound while holding the report, so it is incomplete and cannot be judged \
                 (an absent row reads as success). {} row(s) survived:\n{}",
                partial.len(),
                partial.join("\n\n")
            )
        }
    }
}

fn skip_if_cranelift_host_unsupported() -> bool {
    if jet_jit::cranelift_host_supported() {
        false
    } else if std::env::var("JET_REQUIRE_CRANELIFT_HOST").as_deref() == Ok("1") {
        // c730: CI parity must never green-skip on an unsupported host.
        panic!(
            "cranelift-jit host path unsupported on this architecture \
             (JET_REQUIRE_CRANELIFT_HOST=1); remove the host from the parity \
             matrix or restore native JIT support"
        );
    } else {
        eprintln!("note: cranelift-jit host path unsupported on this architecture; skipping resident JIT assertion");
        true
    }
}

fn require_multi_head_parity_prereqs() {
    assert!(
        jet_jit::cranelift_host_supported(),
        "multi-head parity requires a supported Cranelift host; project harness must provision one"
    );
    assert!(
        have_rustc(),
        "multi-head parity requires rustc; project harness must provision it"
    );
}

/// Keep JIT trace state scoped to each test operation.
///
/// `jet_jit::on_compiler_stack` is the product boundary: it installs the sized
/// compiler stack and carries the trace flags, tier rows, `struct_new` tally
/// and tier artifact back to this thread. Re-entrancy makes it inline when an
/// outer entry already crossed, so nesting it is free.
fn with_jit_test_scope<T, F>(f: F) -> T
where
    F: FnOnce() -> T + Send,
    T: Send,
{
    jet_jit::reset_jit_trace_for_test();
    jet_jit::on_compiler_stack(f)
}

/// c77: the differential battery covers EVERY `examples/features/*.jet`, not a
/// hand-curated subset — so the battery can never quietly shrink. Each example
/// either runs in the interpreter (and its stdout/stderr/exit code must match
/// the compiled binary; stdout also matches its golden `.out`) or stops at a named boundary
/// (E2201 pre-scan, E2202 fuel, E0956 unsupported-at-runtime). A silent skip is
/// a test failure.
fn example_path(stem: &str) -> String {
    format!("examples/features/{}.jet", stem)
}

fn host_expected_stdout(stem: &str) -> Option<&'static str> {
    match stem {
        "lowlevel/os_target_gating" if cfg!(target_os = "macos") => Some("macos: appkit\n"),
        "lowlevel/os_target_gating" if cfg!(target_os = "windows") => Some("windows: win32\n"),
        _ => None,
    }
}

fn uses_ffi_bridge(stem: &str) -> bool {
    matches!(
        stem,
        "lowlevel/ffi"
            | "lowlevel/inline_c"
            | "lowlevel/inline_asm"
            | "io/archive"
            | "io/db"
            | "crypto/crypto_envelope"
            | "crypto/crypto_sign"
            | "crypto/crypto_migration"
            | "crypto/crypto_suite"
            | "crypto/vault_secret"
            | "crypto/auth_tokens"
            | "io/compress_gzip"
            | "io/compress_zstd"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProgramOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

impl ProgramOutput {
    fn ran(stdout: String, stderr: String, exit_code: i32) -> Self {
        Self {
            stdout,
            stderr,
            exit_code,
        }
    }
}

/// The oracle binary path for one `compiled_binary_output` invocation.
///
/// A generated CLI's usage banner names argv[0], on purpose: it flows from the
/// one renderer at `Prelude/CoreLib/Top/Args.rs::JetArgsSpec::help` through
/// `Prelude/Job.rs::jet_args_source_program_name`, which takes the basename and
/// strips a trailing `.jet`. The resident JIT and the interpreter are handed
/// the source path (`positionals.jet` -> `positionals`), and a real `jet build`
/// names the binary after the source stem — `tests/cli_parts/surface.rs::
/// derived_help_uses_program_basename_for_compiled_and_jet_run_paths` pins
/// `build/typed` -> `Usage: typed` — so a user's tiers always agree. Only this
/// oracle disagreed, because it built `jet_<tag>_<i>` and that harness-specific
/// name leaked into argv[0].
///
/// So give the oracle the real program name instead of teaching a comparison to
/// scrub it: `tests/golden.rs` already has to rewrite `Usage: <binary>` back to
/// the source stem for exactly this reason, and a second copy of that
/// workaround would spread it. `jet_<tag>_<i>` becomes the containing
/// directory, preserving the per-invocation uniqueness the old file name
/// carried. A caller that needs the built program's identity (argv[0] for a
/// re-exec) asks here rather than restating the layout.
fn compiled_binary_path(
    dir: &std::path::Path,
    tag: &str,
    i: usize,
    file: &str,
) -> std::path::PathBuf {
    let program_name = std::path::Path::new(file)
        .file_stem()
        .and_then(|name| name.to_str())
        .expect("oracle source name is utf8");
    dir.join(format!("jet_{tag}_{i}_bin")).join(program_name)
}

/// #2017: the one place a dev-side harness turns shared answers into something
/// a child can read.
///
/// The answers arrive as bytes from `common::example_stdin`, and each tier gets
/// its OWN file so three children can each read the whole sequence — a single
/// pipe would be drained by whichever tier ran first, which is a silent
/// no-input run for the other two, the exact defect this card is about.
fn answer_file(dir: &std::path::Path, tag: &str, i: usize, answers: &str) -> std::path::PathBuf {
    let path = dir.join(format!("jet_{tag}_{i}.stdin"));
    fs::create_dir_all(dir).unwrap_or_else(|e| panic!("create answer dir: {e}"));
    fs::write(&path, answers).unwrap_or_else(|e| panic!("write answers for `{tag}`: {e}"));
    path
}

// ── #2076: the run-scoped oracle-binary cache ───────────────────────────────
//
// Every dev battery pays an optimized (`-O`) rustc build per example, and the
// six battery targets walk overlapping slices of the same corpus, so one full
// dev run compiled the same binary up to ~958 times. That build is a pure
// function of its inputs — the generated Rust, the toolchain, the flags, the
// link inputs — so the second caller of one key can reuse the first caller's
// artifact instead of paying for it again.
//
// `scripts/agent/verify-full.sh` gives every run a fresh disk-backed root via
// `JET_DEV_ORACLE_CACHE_DIR`, shared by all six battery processes and removed
// at run end. Ad-hoc runs get a process-scoped disk root below the same
// gitignored scratch area; no developer cache crosses runs.
//
// The key covers everything that decides the bytes rustc emits, and every hit
// is digest-verified. Everything else — a changed key input, a truncated
// artifact, an unwritable root — is a MISS, and a miss is the old build path
// unchanged.
// Nothing here can turn a failing battery green: the cache sits strictly
// between "generated Rust ready" and "binary on disk", and a failed build is
// never published.

/// The oracle's rustc flags. ONE list, because it goes into the command AND
/// into the cache key: a flag cannot enter the build without entering the key.
const ORACLE_RUSTC_FLAGS: &[&str] = &["-O"];

/// What stopped an oracle build. The strict caller renders each case as the
/// panic it always did, the `try_` caller reports every case as `None`, and
/// neither one carries its own copy of the build (AGENTS.md I8).
enum OracleBuildFailure {
    Io(std::io::Error),
    FrontEnd(Vec<jet::Diagnostics::Diagnostic>),
    CLinks(Vec<jet::Diagnostics::Diagnostic>),
    Rustc(Output),
}

impl OracleBuildFailure {
    /// The panic text each failure has always carried, in one place, so the two
    /// callers cannot disagree about which failure is which (AGENTS.md I8).
    fn describe(&self, stem: &str, file: &str) -> String {
        let rendered = |diags: &[jet::Diagnostics::Diagnostic]| {
            let src = fs::read_to_string(file).unwrap_or_default();
            jet::render_diagnostics(file, &src, diags)
        };
        match self {
            OracleBuildFailure::Io(error) => format!(
                "`{stem}` ran in dev but its oracle I/O failed (source read or scratch dir): {error}"
            ),
            OracleBuildFailure::FrontEnd(diags) => format!(
                "`{}` ran in dev but failed the front end:\n{}",
                stem,
                rendered(diags.as_slice())
            ),
            OracleBuildFailure::CLinks(diags) => format!(
                "`{}` ran in dev but AOT C-link resolution failed:\n{}",
                stem,
                rendered(diags.as_slice())
            ),
            OracleBuildFailure::Rustc(out) => format!(
                "`{}` ran in dev but generated Rust failed to build (status: {}):\nstdout:\n{}\nstderr:\n{}",
                stem,
                out.status,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        }
    }
}

/// The current run's oracle artifact directory, or `None` when it cannot be
/// created — then every caller builds exactly as it did before #2076.
///
/// An explicit run root is used as-is. The ad-hoc fallback is salted with the
/// rustc identity as well as the process, while the key below still carries
/// `rustc -vV` so the inputs remain self-describing.
static ORACLE_CACHE_ROOT: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    common::assert_test_environment_is_safe();
    let root = match std::env::var_os("JET_DEV_ORACLE_CACHE_DIR") {
        Some(dir) => {
            let root = PathBuf::from(dir);
            common::assert_test_path_on_disk(&root, "JET_DEV_ORACLE_CACHE_DIR");
            root
        }
        None => {
            let rustc_salt = ORACLE_RUSTC_IDENTITY
                .as_deref()
                .map(|identity| jet::SHA256::sha256_hex(identity.as_bytes()))
                .unwrap_or_else(|| "unknown".to_string());
            common::test_scratch_root("dev-oracle-cache").join(format!(
                "run-{}-{rustc_salt}",
                std::process::id()
            ))
        }
    };
    fs::create_dir_all(&root).ok().map(|()| root)
});

/// The identity of the oracle toolchain: `rustc -vV` names the release, the
/// commit hash AND the `host:` triple — which is the oracle's target, because
/// no `--target` is ever passed here. One probe per process.
///
/// `None` disables the cache instead of failing a battery: a harness that
/// cannot ask rustc who it is has no business claiming two builds match.
static ORACLE_RUSTC_IDENTITY: LazyLock<Option<String>> = LazyLock::new(|| {
    let out = Command::new("rustc").arg("-vV").output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
});

/// The cache entry for one oracle build, or `None` when there is no usable
/// cache.
///
/// The key is a length-prefixed digest — the shape
/// `Source/RuntimeCache.rs::cache_key` uses, so no two field lists can collide
/// — over every input that decides the bytes rustc emits:
///
///   1. the generated Rust, which already carries the example's own bytes and
///      the compiler that lowered them,
///   2. `rustc -vV`: toolchain release, commit hash, host/target triple,
///   3. the rustc flags, from the one `ORACLE_RUSTC_FLAGS` list,
///   4. the FFI bridge link inputs when the example has one — crate name, rlib
///      path (itself content-keyed by the bridge signature), the rlib's OWN
///      digest, its `artifacts.sha256` manifest digest because that bridge is
///      rebuilt in place under the same path, and every dependency search dir,
///   5. the resolved C link flags.
fn oracle_cache_entry(
    rust: &str,
    ffi: Option<&jet::FFI::FfiLink>,
    clinks: &[String],
) -> Option<PathBuf> {
    let root = ORACLE_CACHE_ROOT.as_ref()?;
    let mut data = Vec::new();
    push_oracle_key_field(&mut data, b"jet-dev-oracle-bin-v1");
    push_oracle_key_field(&mut data, rust.as_bytes());
    push_oracle_key_field(&mut data, ORACLE_RUSTC_IDENTITY.as_deref()?.as_bytes());
    for flag in ORACLE_RUSTC_FLAGS {
        push_oracle_key_field(&mut data, flag.as_bytes());
    }
    match ffi {
        None => push_oracle_key_field(&mut data, b"no-ffi"),
        Some(link) => {
            push_oracle_key_field(&mut data, link.crate_name.as_bytes());
            push_oracle_key_field(&mut data, link.rlib_path.to_string_lossy().as_bytes());
            push_oracle_key_field(
                &mut data,
                jet::SHA256::sha256_file_hex(&link.rlib_path).ok()?.as_bytes(),
            );
            let artifacts_manifest = link
                .rlib_path
                .parent()?
                .join(format!("{}.sha256", link.crate_name));
            push_oracle_key_field(&mut data, b"ffi-artifacts.sha256");
            push_oracle_key_field(
                &mut data,
                artifacts_manifest.to_string_lossy().as_bytes(),
            );
            push_oracle_key_field(
                &mut data,
                jet::SHA256::sha256_file_hex(&artifacts_manifest)
                    .ok()?
                    .as_bytes(),
            );
            for deps_dir in link.dependency_dirs() {
                push_oracle_key_field(&mut data, deps_dir.to_string_lossy().as_bytes());
            }
        }
    }
    for arg in clinks {
        push_oracle_key_field(&mut data, arg.as_bytes());
    }
    Some(root.join(jet::SHA256::sha256_hex(&data)))
}

/// Length-prefix every key field, so two different field lists cannot hash to
/// the same bytes (`Source/RuntimeCache.rs::push_bytes`).
fn push_oracle_key_field(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}

/// Put this entry's artifact at `bin`, or say the entry cannot be trusted.
///
/// Verified on every hit: a recorded digest that does not match the bytes on
/// disk is a miss, never a reused artifact — `Source/RuntimeCache.rs` verifies
/// its rlibs the same way. The link lands at the caller's own
/// `compiled_binary_path`, so argv[0] stays the program's own name and a
/// generated CLI's usage banner reads the same as it did when this call built
/// its own binary.
fn reuse_cached_oracle(entry: &std::path::Path, bin: &std::path::Path) -> bool {
    let ready = entry.join("ready");
    let artifact = ready.join("artifact");
    let digest = ready.join("artifact.sha256");
    let Ok(recorded) = fs::read_to_string(&digest) else {
        return false;
    };
    if !fs::symlink_metadata(&artifact)
        .is_ok_and(|metadata| metadata.file_type().is_file())
        || !fs::symlink_metadata(&digest)
            .is_ok_and(|metadata| metadata.file_type().is_file())
    {
        return false;
    }
    match jet::SHA256::sha256_file_hex(&artifact) {
        Ok(actual) if actual == recorded.trim() => {}
        _ => return false,
    }
    let _ = fs::remove_file(bin);
    // A hard link keeps reuse O(1); `copy` covers a caller whose scratch is on
    // another filesystem than the cache root.
    if fs::hard_link(&artifact, bin).is_err() && fs::copy(&artifact, bin).is_err() {
        return false;
    }
    record_oracle_event("hit", entry);
    true
}

/// Publish the freshly built `bin` as this entry's artifact, best effort.
///
/// The artifact and its digest are staged in a private directory and the whole
/// directory is renamed into place, so a reader can never meet an artifact whose
/// digest has not landed yet. `ready` already existing means another process
/// published this key first: same key, same inputs, nothing to do.
///
/// A hard link, not a copy: the run then holds exactly ONE copy of the bytes per
/// distinct oracle, whoever built it and however many batteries reuse it. The
/// run-scoped root keeps that copy available to every battery in the run. A
/// later build that writes over that same scratch path in place can change the
/// published bytes — and that is caught rather than served, because a hit
/// re-hashes the artifact against its recorded digest and a mismatch is a miss.
fn publish_cached_oracle(entry: &std::path::Path, bin: &std::path::Path) {
    let ready = entry.join("ready");
    if ready.is_dir() {
        return;
    }
    let staged = entry.join(format!(
        "staged.{}.{}",
        std::process::id(),
        next_oracle_index()
    ));
    let published = (|| -> std::io::Result<()> {
        fs::create_dir_all(&staged)?;
        if fs::hard_link(bin, staged.join("artifact")).is_err() {
            fs::copy(bin, staged.join("artifact"))?;
        }
        fs::write(
            staged.join("artifact.sha256"),
            jet::SHA256::sha256_file_hex(&staged.join("artifact"))?,
        )?;
        fs::rename(&staged, &ready)
    })();
    if published.is_err() {
        let _ = fs::remove_dir_all(&staged);
    }
}

/// Same-key oracle builds race across the six test processes. The atomic
/// publish prevents torn hits; this lock prevents duplicate cold rustc work.
struct OracleCacheLock {
    dir: PathBuf,
}

impl OracleCacheLock {
    fn acquire(entry: &std::path::Path) -> std::io::Result<Self> {
        fs::create_dir_all(entry)?;
        let dir = entry.join(".build-lock");
        loop {
            match fs::create_dir(&dir) {
                Ok(()) => return Ok(Self { dir }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(&dir)
                        .and_then(|meta| meta.modified())
                        .and_then(|modified| {
                            modified.elapsed().map_err(std::io::Error::other)
                        })
                        .is_ok_and(|age| age > Duration::from_secs(120));
                    if stale {
                        let _ = fs::remove_dir(&dir);
                    } else {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for OracleCacheLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.dir);
    }
}

#[derive(Default)]
struct OracleCacheCounts {
    hits: usize,
    misses: usize,
    builds: usize,
}

static ORACLE_CACHE_COUNTS: LazyLock<Mutex<OracleCacheCounts>> =
    LazyLock::new(|| Mutex::new(OracleCacheCounts::default()));

fn report_oracle_cache_counts() {
    let counts = ORACLE_CACHE_COUNTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if counts.hits + counts.misses + counts.builds > 0 {
        eprintln!(
            "oracle cache: {} hits, {} misses, {} builds",
            counts.hits, counts.misses, counts.builds
        );
    }
}

/// One line per oracle event in the shared cache root — the #2076 ledger.
///
/// Six battery processes share the root, so a per-process counter cannot state
/// a combined count and an appended line can. The run file is cumulative; total
/// builds against the number of DISTINCT keys built is the duplicate-compile
/// count the card gates on:
///
/// ```text
/// events="$JET_DEV_ORACLE_CACHE_DIR/events"
/// builds=$(grep -c '^build ' "$events")
/// distinct=$(awk '$1=="build"{print $2}' "$events" | sort -u | wc -l)
/// hits=$(grep -c '^hit ' "$events")
/// echo "duplicate optimized oracle compiles: $((builds - distinct)) (was $((hits + builds - distinct)))"
/// ```
///
/// Every `hit` is a compile the current or an earlier run did not pay and the
/// old harness did, so the cumulative ledger still exposes reuse without a
/// cache-off run.
///
/// `miss` records a caller that needed the lock, `hit` a reuse, `build` a paid
/// compile, and `fail` a compile rustc rejected. A rejection is deliberately
/// NOT published, so the few examples whose oracle is known broken
/// (`AOT_BROKEN_HELD_OUT`) still pay their compile in every battery that reaches
/// them, and the ledger says so by name instead of hiding them in the duplicate
/// count.
///
/// Best effort by construction: this ledger measures the cache and never gates
/// a battery, so a failed append costs a number, never a verdict.
fn record_oracle_event(event: &str, entry: &std::path::Path) {
    if let Ok(mut counts) = ORACLE_CACHE_COUNTS.lock() {
        match event {
            "hit" => counts.hits += 1,
            "miss" => counts.misses += 1,
            "build" => counts.builds += 1,
            _ => {}
        }
    }
    let Some(root) = ORACLE_CACHE_ROOT.as_ref() else {
        return;
    };
    let Some(key) = entry.file_name() else {
        return;
    };
    let key = key.to_string_lossy();
    if let Ok(mut log) = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(root.join("events"))
    {
        use std::io::Write;
        let _ = log.write_all(format!("{event} {key}\n").as_bytes());
    }
}

/// Per-stem AOT scratch on the gitignored disk root. The process id keeps two
/// simultaneous batteries from writing the same generated source; the oracle
/// cache above supplies same-run reuse.
fn aot_scratch_dir(tag: &str, stem: &str) -> PathBuf {
    let stem = stem.replace('/', "_");
    common::test_scratch_root("aot").join(format!(
        "{tag}_{stem}_{}",
        std::process::id()
    ))
}

/// Build the optimized AOT oracle for one example, or reuse the run-scoped
/// artifact for the same inputs (#2076).
///
/// The ONE place a dev battery turns generated Rust into an oracle binary. Both
/// callers below route through it, so the cache cannot be half-applied and the
/// two `rustc` invocations they used to carry cannot drift apart (AGENTS.md I8).
fn build_oracle_binary(
    dir: &std::path::Path,
    tag: &str,
    i: usize,
    stem: &str,
    file: &str,
) -> Result<std::path::PathBuf, OracleBuildFailure> {
    common::assert_test_environment_is_safe();
    common::assert_test_path_on_disk(dir, "AOT scratch");
    let src = fs::read_to_string(file).map_err(OracleBuildFailure::Io)?;
    let compiled = jet::compile_with_path(&src, file).map_err(OracleBuildFailure::FrontEnd)?;
    let clinks = jet::resolve_c_links(file).map_err(OracleBuildFailure::CLinks)?;
    // I8: `compiled_binary_path` is the ONE oracle-binary naming rule, and it
    // exists precisely so argv[0] is the program's own name (see its doc). The
    // corpus gate's own build site used to keep the old `jet_<tag>_<i>` name, so
    // its oracle ran as `jet_corpus_gate_aot_0` and the gate read its own
    // harness binary name back out of a usage banner as a run-tier divergence.
    let bin = compiled_binary_path(dir, tag, i, file);
    fs::create_dir_all(bin.parent().expect("oracle binary has a parent"))
        .map_err(OracleBuildFailure::Io)?;
    let entry = oracle_cache_entry(&compiled.rust, compiled.ffi.as_ref(), &clinks);
    if let Some(entry) = &entry {
        if reuse_cached_oracle(entry, &bin) {
            return Ok(bin);
        }
        record_oracle_event("miss", entry);
    }
    let _cache_lock = entry.as_ref().and_then(|entry| {
        match OracleCacheLock::acquire(entry) {
            Ok(lock) => Some(lock),
            Err(error) => {
                eprintln!(
                    "note: oracle cache lock unavailable for {}: {error}; building uncached",
                    entry.display()
                );
                None
            }
        }
    });
    if let Some(entry) = &entry {
        if reuse_cached_oracle(entry, &bin) {
            return Ok(bin);
        }
        if _cache_lock.is_some() {
            let _ = fs::remove_dir_all(entry.join("ready"));
        }
    }
    let rs = dir.join(format!("jet_{tag}_{i}.rs"));
    let mut rustc_cmd = Command::new("rustc");
    // Match default optimized AOT behavior. `add_generated_rust` caches the
    // runtime dependency; the entry above is what keeps the USER program from
    // being compiled and linked again for every battery that walks this stem.
    add_generated_rust(
        &mut rustc_cmd,
        &rs,
        &compiled.rust,
        compiled.ffi.is_some(),
        ORACLE_RUSTC_FLAGS,
    );
    rustc_cmd.arg("-o").arg(&bin);
    if let Some(link) = &compiled.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    for arg in &clinks {
        rustc_cmd.arg(arg);
    }
    let out = command_output_with_timeout(
        rustc_cmd,
        *DEV_DIFF_TIMEOUT,
        &format!("rustc build for `{stem}`"),
    );
    if let Some(entry) = &entry {
        record_oracle_event(
            if out.status.success() { "build" } else { "fail" },
            entry,
        );
    }
    if !out.status.success() {
        return Err(OracleBuildFailure::Rustc(out));
    }
    if let Some(entry) = &entry {
        publish_cached_oracle(entry, &bin);
    }
    Ok(bin)
}

/// `dir` is the caller's fixture directory; AOT output is always redirected to
/// `aot_scratch_dir` so one shared helper owns the storage policy.
fn compiled_binary_output(
    dir: &std::path::Path,
    tag: &str,
    i: usize,
    stem: &str,
    file: &str,
) -> ProgramOutput {
    compiled_binary_output_with_stdin(dir, tag, i, stem, file, None)
}

/// #2017: every harness states the ANSWERS, never a path. The three suites that
/// feed an interactive example used to disagree on the type of the input —
/// inline bytes in `tests/golden.rs`, an `Option<&Path>` here — so a call site
/// had to convert before it could share a fixture. `common::example_stdin` is
/// the one home for the answers themselves (I8); materializing them is this
/// function's private business, because `command_output_with_timeout` owns the
/// spawn and never services a pipe, so a child needs a real fd 0.
fn compiled_binary_output_with_stdin(
    _dir: &std::path::Path,
    tag: &str,
    i: usize,
    stem: &str,
    file: &str,
    answers: Option<&str>,
) -> ProgramOutput {
    let dir = aot_scratch_dir(tag, stem);
    let bin = build_oracle_binary(&dir, tag, i, stem, file)
        .unwrap_or_else(|failure| panic!("{}", failure.describe(stem, file)));
    let mut run_cmd = Command::new(&bin);
    if let Some(answers) = answers {
        run_cmd.stdin(fs::File::open(answer_file(&dir, tag, i, answers)).unwrap());
    }
    // Match golden / ui_and_web three-way: GTK `present` opens a real window
    // unless headless — AOT would hang the 30s timeout otherwise.
    if stem == "ui/ui_native_linux" {
        run_cmd.env("JET_UI_HEADLESS", "1");
    }
    // `os.sync` flushes the shared build filesystem and can exceed the normal
    // short example timeout when the workspace is busy.
    let runtime_timeout = if stem == "io/os_process_control" {
        Duration::from_secs(120)
    } else {
        *DEV_DIFF_TIMEOUT
    };
    let run = command_output_with_timeout(
        run_cmd,
        runtime_timeout,
        &format!("compiled binary run for `{stem}`"),
    );
    ProgramOutput::ran(
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
        run.status.code().unwrap_or(1),
    )
}

/// Builds the AOT binary and stops. #2016: a service entry serves until it is
/// stopped, so the gate cannot RUN it, but its AOT compile is still the proof
/// that the stem is real. Splitting build from run is what lets the gate keep
/// that proof while leaving the stem out of the run universe.
fn try_compiled_binary_build(
    _dir: &std::path::Path,
    tag: &str,
    i: usize,
    stem: &str,
    file: &str,
) -> Option<std::path::PathBuf> {
    let dir = aot_scratch_dir(tag, stem);
    build_oracle_binary(&dir, tag, i, stem, file).ok()
}

fn try_compiled_binary_output(
    dir: &std::path::Path,
    tag: &str,
    i: usize,
    stem: &str,
    file: &str,
) -> Option<ProgramOutput> {
    let bin = try_compiled_binary_build(dir, tag, i, stem, file)?;
    let run = command_output_with_timeout(
        Command::new(&bin),
        *DEV_DIFF_TIMEOUT,
        &format!("compiled binary run for `{stem}`"),
    );
    Some(ProgramOutput::ran(
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
        run.status.code().unwrap_or(1),
    ))
}

fn command_output_with_timeout(mut cmd: Command, timeout: Duration, label: &str) -> Output {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base =
        std::env::temp_dir().join(format!("jet_dev_timeout_{}_{}", std::process::id(), stamp));
    let stdout_path = base.with_extension("stdout");
    let stderr_path = base.with_extension("stderr");
    let stdout_file = fs::File::create(&stdout_path)
        .unwrap_or_else(|e| panic!("{label}: failed to create stdout temp file: {e}"));
    let stderr_file = fs::File::create(&stderr_path)
        .unwrap_or_else(|e| panic!("{label}: failed to create stderr temp file: {e}"));
    let mut child = cmd
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .unwrap_or_else(|e| panic!("{label}: failed to spawn: {e}"));
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let status = child
                    .wait()
                    .unwrap_or_else(|e| panic!("{label}: failed to collect status: {e}"));
                let stdout = fs::read(&stdout_path).unwrap_or_default();
                let stderr = fs::read(&stderr_path).unwrap_or_default();
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                return Output {
                    status,
                    stdout,
                    stderr,
                };
            }
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = fs::read(&stdout_path).unwrap_or_default();
                let stderr = fs::read(&stderr_path).unwrap_or_default();
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                panic!(
                    "{label}: timed out after {:?}\nstdout:\n{}\nstderr:\n{}",
                    timeout,
                    String::from_utf8_lossy(&stdout),
                    String::from_utf8_lossy(&stderr)
                );
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => panic!("{label}: failed to poll child: {e}"),
        }
    }
}

/// #2017: run one tier of one example in its own child, with the answers the
/// golden was recorded with on its own fd 0.
///
/// The choice, recorded here because the card asks for it: a SUBPROCESS, not an
/// injected reader. The in-process entry points (`dev_iteration`,
/// `CraneliftBackend::run`) read the test binary's own fd 0, which every thread
/// of a parallel suite shares, so there is nothing per-run to redirect there;
/// and an injected reader would exercise an input path no user has, which is
/// exactly the sort of harness-only seam an I9 differential is supposed to
/// catch rather than contain. A child reads stdin the way a user's program
/// does, through the same CLI entry point.
///
/// `trace` asks the run for `--trace-tiers`, which puts the tier attribution on
/// stderr and therefore makes stderr unusable as a byte comparison — so a
/// caller that needs both takes two runs: one to learn WHICH tier answered, one
/// to collect what it printed. `tests/terminal.rs` splits the same way.
fn cli_tier_output_with_answers(
    file: &str,
    stem: &str,
    answers: &str,
    interpret: bool,
    trace: bool,
) -> Output {
    let tag = format!(
        "{}_{}",
        stem.replace('/', "_"),
        if interpret { "interp" } else { "default" }
    );
    let cache = common::unique_tmp(&format!("jet_stdin_{tag}_cache"));
    fs::create_dir_all(&cache).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jet"));
    cmd.arg("run").arg(file);
    if interpret {
        cmd.arg("--interpret");
    }
    // `--quiet` on every run: the banner is the CLI's, not the program's, and
    // `tests/terminal.rs` compares this same golden from this same CLI with it.
    cmd.arg("--quiet");
    if trace {
        cmd.arg("--trace-tiers");
    }
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("NO_COLOR", "1")
        .env("JET_RUN_CACHE_DIR", &cache)
        .env("JET_CACHE_DIR", cache.join("build"))
        .stdin(fs::File::open(answer_file(&cache, &tag, 0, answers)).unwrap());
    let output = command_output_with_timeout(
        cmd,
        *DEV_DIFF_TIMEOUT,
        &format!("`jet run` with answers for `{stem}` ({tag})"),
    );
    let _ = fs::remove_dir_all(&cache);
    output
}

/// The tier that answered, proved from the trace of a run fed the same answers.
///
/// Stated as a positive AND a negative, because a differential that only checks
/// "the tier I wanted appears" passes when BOTH tiers ran and one deopted.
fn assert_cli_tier_answered(file: &str, stem: &str, answers: &str, interpret: bool) {
    let traced = cli_tier_output_with_answers(file, stem, answers, interpret, true);
    assert!(
        traced.status.success(),
        "traced `jet run{}` failed for `{stem}`:\nstdout:\n{}\nstderr:\n{}",
        if interpret { " --interpret" } else { "" },
        String::from_utf8_lossy(&traced.stdout),
        String::from_utf8_lossy(&traced.stderr)
    );
    let trace = String::from_utf8_lossy(&traced.stderr);
    let (wanted, forbidden) = if interpret {
        ("tier0 interp", "tier1 native")
    } else {
        ("tier1 native", "tier0 interp")
    };
    assert!(
        trace.contains(wanted),
        "`{stem}` did not report `{wanted}` when fed its answers:\n{trace}"
    );
    assert!(
        !trace.contains(forbidden),
        "`{stem}` reached `{forbidden}` when it had to stay on `{wanted}`:\n{trace}"
    );
}

/// One tier of one interactive example, byte-for-byte, with no trace on stderr.
fn cli_tier_program_output(
    file: &str,
    stem: &str,
    answers: &str,
    interpret: bool,
) -> ProgramOutput {
    let output = cli_tier_output_with_answers(file, stem, answers, interpret, false);
    ProgramOutput::ran(
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(1),
    )
}

fn dev_iteration_with_timeout(stem: &str, file: &str, use_interpreter: bool) -> RunOutcome {
    let stem = stem.to_string();
    let file = file.to_string();
    let worker_file = file.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(format!("dev-iter-{stem}"))
        .spawn(move || {
            jet_jit::reset_jit_trace_for_test();
            let out = dev_iteration(&worker_file, false, use_interpreter);
            let flags = jet_jit::jit_trace_flags_for_test();
            let _ = tx.send((out, flags));
        })
        .expect("dev_iteration worker");
    let (out, flags) = rx.recv_timeout(*DEV_DIFF_TIMEOUT).unwrap_or_else(|_| {
        panic!(
            "dev_iteration timed out after {:?} for `{}` ({}) with use_interpreter={}",
            *DEV_DIFF_TIMEOUT, stem, file, use_interpreter
        )
    });
    jet_jit::merge_jit_trace_flags_for_test(flags);
    out
}

/// All `.jet` files directly under a topic directory of `examples/features/`
/// (one level: `examples/features/<topic>/<name>.jet`). Skips `expected/`
/// and skips project-directory examples (`<topic>/<name>/main.jet`) — those
/// have their own multi-file drivers and are not single-entry dev targets.
///
/// A `package.jet` beside the examples is that topic's MANIFEST, not an
/// example. `examples/features/modules/package.jet` declares the
/// `settings: .{ cache_slots: … }` and `build:` profiles that
/// `modules/fact_value_arguments.jet` reads through `@build.settings.*`, so it
/// is load-bearing on disk and can never parse as a program — a manifest binds
/// `name: "…"`, which is E0003 in source. Discovering it as an example put one
/// permanent unjudgeable row into `out_of_universe`, the corpus gate's
/// `frontend_rejected:`, and `typechecked_example_stems`'s rejected list at the
/// same time (#2018). Removing it HERE, at the one discovery seam every oracle
/// reads, removes it from all three at once, and is not a skip list: the name
/// comes from `Syntax::PACKAGE_FILE`, the same constant the package loader
/// resolves manifests by, so a renamed manifest cannot silently reappear as a
/// broken example.
fn topic_jet_files(root: &std::path::Path) -> Vec<PathBuf> {
    let ex_dir = root.join("examples/features");
    let mut files = Vec::new();
    for topic_entry in fs::read_dir(&ex_dir).unwrap().flatten() {
        let topic_path = topic_entry.path();
        if !topic_path.is_dir() {
            continue;
        }
        let topic_name = topic_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if topic_name == "expected" {
            continue;
        }
        for e in fs::read_dir(&topic_path).unwrap().flatten() {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jet") {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some(jet::Syntax::PACKAGE_FILE) {
                continue;
            }
            files.push(path);
        }
    }
    files
}

/// The "topic/name" stem for a `.jet` file found via `topic_jet_files`.
fn stem_of(root: &std::path::Path, path: &std::path::Path) -> String {
    let ex_dir = root.join("examples/features");
    let rel = path.strip_prefix(&ex_dir).unwrap().with_extension("");
    rel.to_string_lossy().replace('\\', "/")
}

/// Every top-level example stem under `examples/features/<topic>/`, sorted
/// for determinism. (Subdirectory examples — imports, modules, packages —
/// have their own multi-file drivers and are not single-entry dev targets.)
fn all_example_stems() -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut stems: Vec<String> = topic_jet_files(&root)
        .iter()
        .map(|p| stem_of(&root, p))
        .collect();
    stems.sort();
    stems
}

/// The same weighted LPT shard used by `tools/ci/test-shards.sh`, applied to
/// corpus stems when the gate is split for a bounded run.
fn corpus_gate_shard_config() -> Option<(usize, usize)> {
    let index = std::env::var("JET_CORPUS_GATE_SHARD_INDEX").ok();
    let count = std::env::var("JET_CORPUS_GATE_SHARD_COUNT").ok();
    match (index, count) {
        (None, None) => None,
        (Some(index), Some(count)) => {
            let index = index.parse::<usize>().unwrap_or_else(|_| {
                panic!(
                    "JET_CORPUS_GATE_SHARD_INDEX must be a non-negative integer, got `{index}`"
                )
            });
            let count = count.parse::<usize>().unwrap_or_else(|_| {
                panic!(
                    "JET_CORPUS_GATE_SHARD_COUNT must be a positive integer, got `{count}`"
                )
            });
            assert!(
                count > 0,
                "JET_CORPUS_GATE_SHARD_COUNT must be >= 1, got {count}"
            );
            assert!(
                index < count,
                "JET_CORPUS_GATE_SHARD_INDEX ({index}) must be < JET_CORPUS_GATE_SHARD_COUNT ({count})"
            );
            Some((index, count))
        }
        (Some(_), None) | (None, Some(_)) => panic!(
            "JET_CORPUS_GATE_SHARD_INDEX and JET_CORPUS_GATE_SHARD_COUNT must be set together"
        ),
    }
}

/// Static work estimate for one corpus stem. The current ledger class is the
/// cheapest honest cost signal available before a run: frontend/held-out rows
/// stop before the tier differential, AOT-broken rows reach the oracle, and
/// resident/deopt rows run all tiers. New stems take the expensive default so
/// they cannot silently skew a shard.
fn corpus_gate_stem_weight(class: Option<&CorpusGateClass>) -> u64 {
    match class {
        Some(
            CorpusGateClass::FrontendRejected
            | CorpusGateClass::GateExcluded
            | CorpusGateClass::NonRunnable
            | CorpusGateClass::OracleUnavailable,
        ) => 1,
        Some(CorpusGateClass::ExpectedExit) => 2,
        Some(CorpusGateClass::AotBroken | CorpusGateClass::RunTierBroken) => 3,
        Some(CorpusGateClass::ResidentJit | CorpusGateClass::DeoptInterp | CorpusGateClass::TierDivergent)
        | None => 4,
    }
}

fn corpus_gate_shard_stems(stems: Vec<String>) -> Vec<String> {
    let Some((index, count)) = corpus_gate_shard_config() else {
        return stems;
    };

    let weights: std::collections::BTreeMap<String, CorpusGateClass> =
        parse_corpus_gate_manifest()
            .into_iter()
            .map(|record| (record.stem, record.class))
            .collect();
    let mut input = String::new();
    for stem in &stems {
        input.push_str(&corpus_gate_stem_weight(weights.get(stem)).to_string());
        input.push('\t');
        input.push_str(stem);
        input.push('\n');
    }

    let mut child = Command::new("bash")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/ci/weighted-shards.sh"))
        .args([index.to_string(), count.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn shared weighted shard engine");
    child
        .stdin
        .as_mut()
        .expect("shared weighted shard stdin")
        .write_all(input.as_bytes())
        .expect("write corpus stems to shared weighted shard engine");
    let output = child
        .wait_with_output()
        .expect("wait for shared weighted shard engine");
    assert!(
        output.status.success(),
        "shared weighted shard engine failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let known: std::collections::BTreeSet<&str> = stems.iter().map(String::as_str).collect();
    let mut selected: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|stem| !stem.is_empty())
        .map(str::to_string)
        .collect();
    assert!(
        selected.iter().all(|stem| known.contains(stem.as_str())),
        "shared weighted shard engine emitted a stem outside its input: {selected:?}"
    );
    selected.sort();
    assert!(
        selected.windows(2).all(|pair| pair[0] != pair[1]),
        "shared weighted shard engine emitted a duplicate stem: {selected:?}"
    );
    selected
}

fn corpus_gate_selected_stems() -> Vec<String> {
    let mut stems = all_example_stems();
    if let Ok(filter) = std::env::var("JET_CORPUS_GATE_FILTER") {
        stems.retain(|stem| stem.contains(&filter));
    }
    corpus_gate_shard_stems(stems)
}

/// Stems the corpus gate already classifies as frontend-rejected (compile errors).
/// Interpreter parity batteries skip them — they cannot run or hit an interpreter boundary.
fn frontend_rejected_stems() -> std::collections::HashSet<String> {
    parse_corpus_gate_manifest()
        .into_iter()
        .filter(|r| r.class == CorpusGateClass::FrontendRejected)
        .map(|r| r.stem)
        .collect()
}

fn interpreter_example_stems() -> Vec<String> {
    let rejected = frontend_rejected_stems();
    all_example_stems()
        .into_iter()
        .filter(|stem| !rejected.contains(stem))
        .collect()
}

/// How many shipped examples the default `jet dev` battery is still allowed to
/// lose to a run-path sema error (#2020).
///
/// Shrink-only, exactly like `OUT_OF_UNIVERSE_CEILING` and for the same reason.
/// `typechecked_example_stems` used to `filter` a sema-failing stem out and say
/// nothing, so a stem could leave this battery's universe by BREAKING: on
/// 2026-08-14 a mechanical `x ?? panic(…)` -> `x?` rewrite across 39 files was
/// made against a conversion rail that had been removed eight days earlier, 28
/// net/serde examples stopped type-checking, and this battery still reported
/// green because the broken stems were simply no longer in it. That is the hole
/// #1998 closed in `observe_jit_coverage` and #2012 closed in the retired
/// `jit_covered_example_stems`, reproduced in a third oracle.
///
/// Every row is a defect to fix — fix the example, or the rail it trips — never
/// a stem to write out of the battery. The rows are DERIVED from the compiler's
/// own diagnostic at battery time, so this is a counted ceiling and not an
/// allowlist: no stem can be excused by name.
///
/// Provenance: 28, the count traced on this branch while #2020 was open. It is a
/// recorded observation, not a measurement of this assertion — correct it to the
/// first count this battery observes, in the same diff as that observation, and
/// only downward afterwards.
/// 54 is the first OBSERVED count, taken at 76bb19ba9 once this assertion
/// existed to measure it. The 28 it replaced was a hand-traced estimate from a
/// different oracle (the JIT coverage universe, which walks a slightly
/// different set) and was never a measurement of this battery. Correcting an
/// estimate to its measurement is not raising a guard: the number may only fall
/// from here, and card #2018 owns driving it down.
///
/// 1 is the count #2018 drove it to. The 11 it replaces was never measured by
/// anything: it was written in the same uncommitted snapshot that also renamed
/// the modules examples, so the number moved without evidence while the ledger
/// beside it still carried 40 rows. The four #2018 recovered, each by its own
/// root cause and not by editing the ceiling:
///   * the E2402 core-error family, by the ratified `impl <CoreError> => Err`
///     rail (D-FAIL-CONV2=A) — 36 net/serde stems, every cascading E0311/E0302/
///     E0107/E0114 with them;
///   * `crypto/vault_secret`, by dropping the D-CRYPTOENV1 `#Unsafe` demand from
///     `core.crypto.vault.get`, whose gate is the `Secret` effect;
///   * `memory/shared_guard_queue`, by putting `Condition` on the shared-handle
///     rail it was always Arc-backed for;
///   * `modules/package`, which was never an example — it is that topic's
///     manifest, now excluded at `topic_jet_files`, the one discovery seam.
///
/// Build entries use the full staged build front end, so compiler-only calls
/// and generated runtime declarations are checked in their real context.
const SEMA_REJECTED_CEILING: usize = 0;

/// The stems the run-path front end accepts, and the ones it rejects together
/// with the reason the compiler gave (#2020). Nothing is discarded.
fn typechecked_example_stems() -> (Vec<String>, Vec<String>) {
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for stem in all_example_stems() {
        let file = example_path(&stem);
        let diags = if let Ok(source) = fs::read_to_string(&file) {
            if jet::Driver::selects_build_entry(&source, None) {
                jet::Driver::compile_bundle_path_build(&file, jet::Driver::BuildRunOptions::default())
                    .map(|_| Vec::new())
                    .unwrap_or_else(|diags| diags)
            } else {
                jet::check_with_path(&file)
            }
        } else {
            jet::check_with_path(&file)
        };
        if diags
            .iter()
            .any(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        {
            rejected.push(format!("{stem}: {}", first_diagnostic_summary(&diags)));
        } else {
            accepted.push(stem);
        }
    }
    (accepted, rejected)
}

/// The one-line `CODE: message` reason a stem carries when the compile oracle
/// could not judge it.
///
/// #1998: the reason is DERIVED from the compiler's own diagnostic, never
/// declared here, so the out-of-universe list can never become an allowlist. A
/// stem leaves the oracle only for as long as the compiler still says why.
fn first_diagnostic_summary(diagnostics: &[jet::Diagnostics::Diagnostic]) -> String {
    let first = diagnostics
        .iter()
        .find(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .or_else(|| diagnostics.first());
    match first {
        Some(d) => format!("{}: {}", d.code, d.what.replace('\n', " ")),
        None => "failed without a diagnostic".to_string(),
    }
}

/// What the in-process compile oracle can say about one example stem.
///
/// #1998: there used to be a fourth outcome with no name. A stem whose
/// in-process `Loader` or `Sema` pass failed fell out of the loop on a bare
/// `continue`, so it joined neither set, and the audit then reported `gaps: 0`
/// over a denominator it never stated. The fourth outcome is named here and the
/// match on it is exhaustive, so a stem can no longer leave the audit silently.
///
/// #2012: `Covered` carries the resident gate too. The three-way differential
/// battery needs a stricter fact — the lowerer accepted the bundle AND the
/// resident JIT will run it — and it used to answer that from a second walk of
/// the same corpus (`jit_covered_example_stems`) carrying the same two silent
/// `continue`s this enum exists to remove. One oracle now answers both
/// questions, so the two can no longer disagree (AGENTS.md I8).
enum JitCompileVerdict {
    /// The JIT lowerer accepted the bundle. `resident_refusal` is `None` when
    /// the resident JIT will also RUN it, or the
    /// `resident_jit_safe_bundle_detail` reason it will not.
    Covered { resident_refusal: Option<String> },
    /// The lowerer rejected the bundle: a live I9 compile gap, and its reason.
    Gap(String),
    /// The harness never reached the lowerer, so this run judged nothing about
    /// the stem. Carries the diagnostic that stopped it.
    OutOfUniverse(String),
}

/// How many example stems the compile oracle must still see (#1998).
///
/// A floor, because examples get added: deleting one, or making
/// `topic_jet_files` stop seeing one, must lower a reviewed constant. It lives
/// here, outside `examples/features/`, so deleting an example cannot silently
/// shrink the measured universe.
///
/// #2012: every reader of `collect_jit_coverage` asserts this, not just the
/// coverage audit, so no battery scoped to a slice of this universe can report
/// success while the universe itself shrank.
///
/// #2018 lowered it 498 -> 497 in the same diff as the deletion the rule above
/// demands: `topic_jet_files` no longer discovers `examples/features/modules/
/// package.jet`, because that file is the `modules` topic's MANIFEST and never
/// was an example. It stays on disk — `modules/fact_value_arguments.jet` reads
/// the `cache_slots` setting it declares — so this is one fewer stem measured,
/// not one fewer example shipped. `examples/features/expected/` corroborates it:
/// 19 stems carry no golden, and every one of the other 18 is a deliberate
/// panic/trap/abort example whose whole point is the failure path.
/// `modules/package` is the only one that is not a program at all.
///
/// 497 is the last REVIEWED value minus that one deletion, deliberately not the
/// count on disk. `topic_jet_files` saw 500 files while this card was open, so
/// the honest observed floor was 499 — but the extra stems are examples other
/// work added concurrently and this card does not own them. Leaving the floor at
/// the reviewed-minus-one value keeps a real deletion loud without making this
/// diff fail when a sibling change is integrated in a different order. Raise it
/// to the observed count in the same diff as the next observed run.
const EXAMPLE_CORPUS_FLOOR: usize = 497;

/// How many stems the compile oracle is still allowed to be blind to (#1998).
///
/// Shrink-only. Every row is a stem this oracle cannot judge, so every row is a
/// defect to fix — build the
/// context the harness is missing, or fix the frontend the stem trips — never a
/// skip to accept. The rows are derived from the compiler's own diagnostics at
/// audit time, so this is a counted ceiling and not an allowlist: no stem can be
/// written out of the universe by hand.
///
/// #2018 drove it to 1. It was 53 when the card opened and 11 when the card was
/// picked up, and that 11 was never measured — it was written in an uncommitted
/// snapshot while `tests/jit_corpus_gate.txt` beside it still carried 40
/// `frontend_rejected:` rows, so the exclusion set had been tightened without
/// evidence. What actually recovered the stems is recorded on
/// `SEMA_REJECTED_CEILING` above, plus one gap closed HERE: `classify_jit_compile`
/// now seeds the build-fact snapshot before sema, exactly as
/// `classify_corpus_stem` does, so the two in-process front ends can no longer
/// judge the same `@build.*`-reading stem in two different contexts.
///
/// Build entries now use the same staged runtime bundle as `jet run`, so
/// generated declarations enter this oracle instead of being reported outside
/// it.
const OUT_OF_UNIVERSE_CEILING: usize = 0;

/// How many compile-covered stems the resident JIT must still be willing to run
/// — the universe `cranelift_three_way_differential_battery` is scoped to
/// (#2012).
///
/// A floor for the same reason `EXAMPLE_CORPUS_FLOOR` is one: the corpus grows,
/// and a stem that quietly stops being resident-safe silently leaves the
/// battery's denominator, which is exactly how that battery could run a
/// shrinking slice of the corpus and still pass.
///
/// Last recorded observation: `344 ran / 353 resident-safe`, from the #1251
/// verification at 0f5505640 (2026-07-28), when the corpus was smaller than the
/// `EXAMPLE_CORPUS_FLOOR` above. It is therefore a conservative floor, to raise
/// to the observed count in the same diff as the next observed run.
const RESIDENT_SAFE_FLOOR: usize = 353;

/// How many stems the three-way differential battery must still execute (#2012).
///
/// `RESIDENT_SAFE_FLOOR` pins the universe; this pins how much of it the battery
/// actually ran, so deleting a golden — which moves a stem out of the run set
/// without moving it out of the universe — cannot pass quietly either. Same
/// provenance as `RESIDENT_SAFE_FLOOR`: `344 ran` at 0f5505640. It replaces the
/// original `ran >= 9` M3 seed floor, which stopped saying anything once the
/// battery reached the hundreds.
const THREE_WAY_RAN_FLOOR: usize = 344;

/// The audit's whole answer, together with the denominator it was measured over.
///
/// `covered`, `gaps` and `out_of_universe` partition `corpus` exactly, and
/// `resident_safe` and `resident_refused` partition `covered` exactly;
/// `observe_jit_coverage` asserts both.
#[derive(Clone)]
struct JitCoverage {
    /// Every stem `topic_jet_files` yielded — the audit's universe.
    corpus: usize,
    covered: Vec<String>,
    /// #2012: the subset of `covered` the resident JIT will also RUN — the
    /// universe `cranelift_three_way_differential_battery` is scoped to.
    resident_safe: Vec<String>,
    /// `stem: reason`, one row per compile-covered stem the resident gate turns
    /// away. The battery cannot run these, so it states how many it held back
    /// and why, instead of quietly narrowing to the ones it can.
    resident_refused: Vec<String>,
    gaps: Vec<String>,
    /// `stem: where: CODE: message`, one row per stem the oracle could not
    /// judge. Shrink-only: each row is a defect to fix, never a skip to accept.
    out_of_universe: Vec<String>,
}

/// Observe which examples `try_compile_bundle` accepts, once per test binary.
///
/// The answer is a pure function of the example corpus and the compiler, and
/// producing it compiles the whole corpus (~80s in CI). Both ratchet entry
/// points want the same answer, so pay for it once.
fn collect_jit_coverage() -> JitCoverage {
    static COVERAGE: LazyLock<JitCoverage> = LazyLock::new(observe_jit_coverage);
    (*COVERAGE).clone()
}

fn observe_jit_coverage() -> JitCoverage {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // `topic_jet_files` yields `read_dir` order, which is the host filesystem's
    // and not the corpus's. This walk compiles every bundle into one process, so
    // an order-dependent verdict would make the ledger a property of the machine
    // that dumped it. Sort the walk, exactly as `all_example_stems` already does.
    let mut paths = topic_jet_files(&root);
    paths.sort();
    let corpus = paths.len();
    let mut covered = Vec::new();
    let mut resident_safe = Vec::new();
    let mut resident_refused = Vec::new();
    let mut gaps = Vec::new();
    let mut out_of_universe = Vec::new();
    for path in paths {
        let stem = stem_of(&root, &path);
        match classify_jit_compile(&path) {
            JitCompileVerdict::Covered { resident_refusal } => {
                // #2012: the resident split is recorded on the same pass, so the
                // three-way battery reads this observation instead of walking the
                // corpus a second time with an oracle that could disagree.
                match resident_refusal {
                    None => resident_safe.push(stem.clone()),
                    Some(reason) => resident_refused.push(format!("{stem}: {reason}")),
                }
                covered.push(stem);
            }
            JitCompileVerdict::Gap(reason) => gaps.push(format!("{stem}: {reason}")),
            JitCompileVerdict::OutOfUniverse(reason) => {
                out_of_universe.push(format!("{stem}: {reason}"));
            }
        }
    }
    covered.sort();
    resident_safe.sort();
    resident_refused.sort();
    gaps.sort();
    out_of_universe.sort();
    assert_eq!(
        covered.len() + gaps.len() + out_of_universe.len(),
        corpus,
        "the compile oracle lost a stem: {corpus} discovered, {} covered, {} gap(s), {} \
         outside the universe. Every discovered stem lands in exactly one bucket (#1998)",
        covered.len(),
        gaps.len(),
        out_of_universe.len()
    );
    // #2012: and the resident split partitions `covered` exactly, so the
    // battery's universe cannot lose a stem without this arithmetic failing.
    assert_eq!(
        resident_safe.len() + resident_refused.len(),
        covered.len(),
        "the resident gate lost a stem: {} compile-covered, {} resident-safe, {} refused. \
         Every compile-covered stem lands in exactly one of the two (#2012)",
        covered.len(),
        resident_safe.len(),
        resident_refused.len()
    );
    JitCoverage {
        corpus,
        covered,
        resident_safe,
        resident_refused,
        gaps,
        out_of_universe,
    }
}

/// Put one example in front of the JIT lowerer, or say what stopped it.
fn classify_jit_compile(path: &std::path::Path) -> JitCompileVerdict {
    let file = path.to_string_lossy();
    if fs::read_to_string(path)
        .ok()
        .is_some_and(|source| jet::Driver::selects_build_entry(&source, None))
    {
        return match jet::Driver::compile_bundle_path_build(
            &file,
            jet::Driver::BuildRunOptions::default(),
        ) {
            Ok(output) => match output.runtime {
                Some(bundle) => classify_jit_bundle(bundle),
                None => JitCompileVerdict::OutOfUniverse(
                    "build front end returned no runtime bundle".to_string(),
                ),
            },
            Err(diagnostics) => JitCompileVerdict::OutOfUniverse(format!(
                "build: {}",
                first_diagnostic_summary(&diagnostics)
            )),
        };
    }
    // #1998: this arm was `Err(_) => continue`, which dropped the stem.
    let mut bundle = match jet::Loader::load_entry(&file) {
        Ok(bundle) => bundle,
        Err(diagnostics) => {
            return JitCompileVerdict::OutOfUniverse(format!(
                "loader: {}",
                first_diagnostic_summary(&diagnostics)
            ));
        }
    };
    // #2018: seed the ONE build-fact snapshot before sema, exactly as
    // `classify_corpus_stem` does. This was the only difference between the two
    // in-process front ends, and it is the whole of the card's (a) class: an
    // example reading `@build.settings.*` was judged by this oracle against the
    // harness's defaults while the corpus gate judged it against the manifest's
    // locked stamp, so the two oracles could disagree about the same stem and
    // neither reader could tell which context produced the verdict (AGENTS.md
    // I8; I9: engines marshal one snapshot, they do not each invent one).
    if let Err(diagnostics) = jet::Driver::seed_build_facts(
        &mut bundle,
        "dev",
        false,
        &std::collections::BTreeMap::new(),
    ) {
        // Named for what it is. #1998's whole point is that the reason a stem
        // left the universe is the compiler's own, so a seeding failure must not
        // be reported as a sema rejection.
        return JitCompileVerdict::OutOfUniverse(format!(
            "build facts: {}",
            first_diagnostic_summary(&diagnostics)
        ));
    }
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    // #1998: this arm was `if !errors.is_empty() { continue }`, same drop.
    if !errors.is_empty() {
        return JitCompileVerdict::OutOfUniverse(format!(
            "sema: {}",
            first_diagnostic_summary(&errors)
        ));
    }
    classify_jit_bundle(bundle)
}

fn classify_jit_bundle(bundle: jet::AST::ProgramBundle) -> JitCompileVerdict {
    match jet_jit::try_compile_bundle(&bundle) {
        // #2012: one more question about a bundle the lowerer already accepted:
        // will the resident JIT run it? `resident_jit_safe_bundle_detail` is a
        // pure analysis of the same bundle, so ask it here rather than in a
        // second corpus walk.
        Ok(()) => {
            let refusal = jet_jit::resident_jit_safe_bundle_detail(&bundle);
            JitCompileVerdict::Covered {
                resident_refusal: (!refusal.is_empty()).then_some(refusal),
            }
        }
        Err(reason) => JitCompileVerdict::Gap(reason),
    }
}

fn is_manifested_parity_divergence(stem: &str, entries: &[String]) -> bool {
    entries.iter().any(|entry| {
        entry
            .strip_prefix(stem)
            .is_some_and(|rest| rest.starts_with(':'))
    })
}

fn normalize_for_parity(stem: &str, mut out: ProgramOutput) -> ProgramOutput {
    if matches!(stem, "io/log" | "io/log_structured" | "serde/json_coerce") {
        out.stderr = normalize_json_log_timestamps(&out.stderr);
    }
    if stem == "io/log_human" {
        out.stderr = normalize_text_log_timestamps(&out.stderr);
    }
    if stem == "ui/ui_native_linux" {
        out.stderr = normalize_gtk_loader_path(&out.stderr);
    }
    // AOT prints `?` propagation trails on uncaught Err; the golden and the
    // interpreter keep stdout-only. Strip the trail so parity compares the
    // program result, not the reporting envelope.
    if stem == "errors/typed_error_families" {
        out.stderr = out
            .stderr
            .lines()
            .filter(|line| {
                let hop = line.strip_prefix("  ").is_some_and(|rest| {
                    rest.split_once(". ")
                        .is_some_and(|(index, _)| index.parse::<u32>().is_ok())
                });
                !line.starts_with(" Trail [E3002] (") && !hop
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !out.stderr.is_empty() && !out.stderr.ends_with('\n') {
            out.stderr.push('\n');
        }
    }
    out
}

fn normalize_gtk_loader_path(s: &str) -> String {
    let marker = ": symbol lookup error: ";
    if !s.contains("libgtk-4.so.1") || !s.contains("undefined symbol") {
        return s.to_string();
    }
    if let Some((_, rest)) = s.split_once(marker) {
        format!("<jet-ui-binary>{marker}{rest}")
    } else {
        s.to_string()
    }
}

fn normalize_json_log_timestamps(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find("\"ts\":") {
        out.push_str(&rest[..pos]);
        out.push_str("\"ts\":<ts>");
        let mut tail = &rest[pos + "\"ts\":".len()..];
        // Optional whitespace between colon and digits (emitters may vary).
        let ws = tail.bytes().take_while(|b| b.is_ascii_whitespace()).count();
        tail = &tail[ws..];
        let digits = tail.bytes().take_while(|b| b.is_ascii_digit()).count();
        tail = &tail[digits..];
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn normalize_text_log_timestamps(s: &str) -> String {
    s.lines()
        .map(|line| {
            let Some((level, rest)) = line.split_once(' ') else {
                return line.to_string();
            };
            let Some((_, msg)) = rest.split_once(" | ") else {
                return line.to_string();
            };
            format!("{level} <ts> | {msg}")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if s.ends_with('\n') { "\n" } else { "" }
}

fn print_jit_op_report() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut ops: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
    for path in topic_jet_files(&root) {
        let file = path.to_string_lossy();
        let mut bundle = match jet::Loader::load_entry(&file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        if diags
            .iter()
            .any(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        {
            continue;
        }
        let covered = jet_jit::try_compile_bundle(&bundle).is_ok();
        for tag in jet_jit::jit_dump_main_ops(&bundle) {
            let entry = ops.entry(tag).or_default();
            if covered {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }
    }
    eprintln!("jit observed TIR op compile coverage:");
    for (op, (covered, gaps)) in ops {
        eprintln!("  {op}: compile_covered_examples={covered} gap_examples={gaps}");
    }
}

/// The recognized honest-boundary / terminal codes the interpreter may stop at
/// instead of producing run-to-completion stdout (c77 / D-DEV1):
///   - E2201: a feature the dev interpreter doesn't cover (pre-scan boundary),
///   - E2202 / E0952: the step/fuel budget was exhausted,
///   - E0956: a construct not yet supported at comptime (hit during execution),
///   - E0953: a deliberate user-authored panic (`assert(false, …)`), which is
///     the program legitimately failing, not a silent skip.
///   - E3410 / E3411: a D-CTEFFECT1 Tier-2 comptime effect (`core.files`/`core.term`/
///     `core.sys`/…) reached with no `#Impure` gate, or a gate present but
///     `--gate impure=allow` not passed — an honest, named boundary (the golden
///     corpus runs with neither), not a silent skip.
///   - E1265 (U13, D-JPK-SECRETCRYPTO1): `core.crypto.vault.get` reached through the
///     same comptime/interpreter evaluation path — unconditionally denied
///     (no `#Impure` escape hatch), so an example exercising it always stops
///     here under the interpreter/JIT tiers even though the AOT-compiled
///     binary runs it fine (it never goes through this evaluator).
const BOUNDARY_CODES: &[&str] = &[
    "E2201", "E2202", "E0952", "E0956", "E0953", "E3410", "E3411", "E3412", "E1265",
    // Front-end / sema codes that surface when the interpreter can't load a
    // construct the AOT path still accepts (splice/generics gaps).
    "E0102", "E0857", "E0107", "E0501",
    // Frontend-rejected examples (corpus gate) may still appear in manifested lists.
    "E0308", "E0504", "E0302", "E0505", "E0915", "E0311", "E1004",
];

const DEFAULT_BACKEND_EXPECTED_BOUNDARIES: &[&str] = &[
    "collections/list_bounds",
];

#[derive(Default, Clone)]
struct DevBatteryStats {
    ran: usize,
    boundary: usize,
    deopt: usize,
    manifested: usize,
    boundary_stems: Vec<String>,
}

impl DevBatteryStats {
    fn add(&mut self, mut other: DevBatteryStats) {
        self.ran += other.ran;
        self.boundary += other.boundary;
        self.deopt += other.deopt;
        self.manifested += other.manifested;
        self.boundary_stems.append(&mut other.boundary_stems);
    }
}

fn assert_default_dev_jit_gap(stem: &str, file: &str) {
    jet_jit::reset_jit_trace_for_test();
    match dev_iteration_with_timeout(stem, file, false) {
        RunOutcome::Ran { .. } => {
            assert!(
                jet_jit::deopt_invoked_for_test() || jet_jit::jit_executed_for_test(),
                "`{stem}` must run via tiered JIT or interpreter deopt"
            );
        }
        RunOutcome::Problems(diags) => {
            // Real boundaries (E2201/FFI) may still stop; coverage gaps must not.
            assert!(
                !jet_jit::is_e2211(&diags),
                "`{stem}` must not emit retired E2211: {diags:?}"
            );
            assert!(
                is_named_dev_boundary(stem, &diags),
                "`{stem}` unexpected Problems under tiered run: {diags:?}"
            );
        }
    }
}

fn is_named_dev_boundary(stem: &str, diags: &[jet::Diagnostics::Diagnostic]) -> bool {
    diags.iter().any(|d| BOUNDARY_CODES.contains(&d.code.as_str()))
        // D-DBDRIVER1/D-DBMIGRATE1: the checked-SQL DB example is an AOT-backed
        // core.db surface today; the dev default tier stops before execution.
        || (stem == "io/db_checked_sql" && diags.iter().all(|d| d.code == "E1004"))
}

/// #2017: corpus batteries compared no-input runs when a stem had answers.
/// `dev_iteration_with_timeout` cannot take a per-run fd 0, so those stems
/// leave the in-process path and compare a child CLI against AOT, both fed
/// `common::example_stdin`.
fn check_interactive_stem_cli_vs_aot(
    i: usize,
    stem: &str,
    dir: &std::path::Path,
    file: &str,
    answers: &str,
    interpret: bool,
    tag: &str,
    manifested_divergences: &[String],
) -> DevBatteryStats {
    let left = normalize_for_parity(stem, cli_tier_program_output(file, stem, answers, interpret));
    let compiled = normalize_for_parity(
        stem,
        compiled_binary_output_with_stdin(dir, tag, i, stem, file, Some(answers)),
    );
    if left != compiled {
        if is_manifested_parity_divergence(stem, manifested_divergences) {
            eprintln!("manifested parity divergence: {stem}");
            return DevBatteryStats {
                ran: 1,
                manifested: 1,
                ..DevBatteryStats::default()
            };
        }
        assert_eq!(
            left, compiled,
            "DIVERGENCE for `{stem}` with the answers its golden was recorded with: CLI and compiled binary disagree"
        );
    }
    DevBatteryStats {
        ran: 1,
        ..DevBatteryStats::default()
    }
}

fn check_dev_default_stem(
    i: usize,
    stem: &str,
    dir: &std::path::Path,
    manifested_divergences: &[String],
) -> DevBatteryStats {
    let file = example_path(stem);
    eprintln!("dev-default checking {stem}");
    if let Some(answers) = common::example_stdin(stem) {
        return check_interactive_stem_cli_vs_aot(
            i,
            stem,
            dir,
            &file,
            answers.piped,
            false,
            "dev_default_diff",
            manifested_divergences,
        );
    }
    jet_jit::reset_jit_trace_for_test();
    let interpreted = match dev_iteration_with_timeout(stem, &file, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            // Coverage gaps deopt; only named boundaries remain.
            assert!(
                !jet_jit::is_e2211(&diags),
                "`{stem}` must not emit retired E2211: {diags:?}"
            );
            eprintln!(
                "default boundary: {stem}: {}",
                diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>().join(",")
            );
            assert!(
                is_named_dev_boundary(stem, &diags),
                "`{}` neither ran nor stopped at a named boundary {:?} under the default \
                 jet dev backend; codes were {:?}",
                stem,
                BOUNDARY_CODES,
                diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
            );
            return DevBatteryStats {
                boundary: 1,
                boundary_stems: vec![stem.to_string()],
                ..DevBatteryStats::default()
            };
        }
    };

    // Host AOT may fail (missing -lbz2, rawptr cast gaps) even when tiered
    // deopt ran. Skip parity when rustc can't build the oracle — not a P0
    // tier bug.
    let Some(compiled) = try_compiled_binary_output(dir, "dev_default_diff", i, stem, &file) else {
        eprintln!("aot unavailable (skip parity): {stem}");
        return DevBatteryStats {
            ran: 1,
            deopt: usize::from(jet_jit::deopt_invoked_for_test()),
            ..DevBatteryStats::default()
        };
    };
    let interpreted = normalize_for_parity(stem, interpreted);
    let compiled = normalize_for_parity(stem, compiled);
    if interpreted != compiled {
        if is_manifested_parity_divergence(stem, manifested_divergences) {
            eprintln!("manifested parity divergence: {stem}");
            return DevBatteryStats {
                ran: 1,
                manifested: 1,
                ..DevBatteryStats::default()
            };
        }
        assert_eq!(
            interpreted, compiled,
            "DIVERGENCE for `{}` under the default jet dev backend: JIT/fallback and compiled \
             binary disagree on stdout/stderr/exit code — this is a P0 miscompile",
            stem
        );
    }
    DevBatteryStats {
        ran: 1,
        deopt: usize::from(jet_jit::deopt_invoked_for_test()),
        ..DevBatteryStats::default()
    }
}

fn run_dev_default_battery_parallel(
    stems: Vec<String>,
    dir: PathBuf,
    manifested_divergences: Vec<String>,
) -> DevBatteryStats {
    let jobs = Arc::new(Mutex::new(
        stems
            .into_iter()
            .enumerate()
            .collect::<std::collections::VecDeque<_>>(),
    ));
    let dir = Arc::new(dir);
    let manifested_divergences = Arc::new(manifested_divergences);
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let worker_count = test_worker_count(16);
    let mut handles = Vec::new();
    for _ in 0..worker_count {
        let jobs = Arc::clone(&jobs);
        let dir = Arc::clone(&dir);
        let manifested_divergences = Arc::clone(&manifested_divergences);
        let failures = Arc::clone(&failures);
        handles.push(
            std::thread::Builder::new()
                .spawn(move || {
                    let mut stats = DevBatteryStats::default();
                    loop {
                        let Some((i, stem)) =
                            lock_recovered(&jobs, "dev default work queue").pop_front()
                        else {
                            break;
                        };
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            check_dev_default_stem(i, &stem, &dir, &manifested_divergences)
                        }));
                        match result {
                            Ok(next) => stats.add(next),
                            Err(payload) => lock_recovered(&failures, "dev default failure report")
                                .push(format!("{stem}: {}", panic_message(payload))),
                        }
                    }
                    stats
                })
                .expect("dev default worker"),
        );
    }

    let mut stats = DevBatteryStats::default();
    for handle in handles {
        stats.add(handle.join().expect("dev default worker panicked outside harness"));
    }
    let failures = judged_report(&failures, "dev default failure report");
    assert!(
        failures.is_empty(),
        "dev default parity failures:\n{}",
        failures.join("\n\n")
    );
    report_oracle_cache_counts();
    stats
}

fn check_interpreter_stem(
    i: usize,
    stem: &str,
    dir: &std::path::Path,
    manifested_divergences: &[String],
) -> DevBatteryStats {
    if uses_ffi_bridge(stem) {
        return DevBatteryStats::default();
    }
    let file = example_path(stem);
    eprintln!("interpreter checking {stem}");
    if let Some(answers) = common::example_stdin(stem) {
        return check_interactive_stem_cli_vs_aot(
            i,
            stem,
            dir,
            &file,
            answers.piped,
            true,
            "dev_diff",
            manifested_divergences,
        );
    }
    let interpreted = match dev_iteration_with_timeout(stem, &file, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            assert!(
                is_named_dev_boundary(stem, &diags),
                "`{}` neither ran nor stopped at a named boundary {:?}; codes were {:?}",
                stem,
                BOUNDARY_CODES,
                diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
            );
            return DevBatteryStats {
                boundary: 1,
                ..DevBatteryStats::default()
            };
        }
    };

    let Some(compiled) = try_compiled_binary_output(dir, "dev_diff", i, stem, &file) else {
        eprintln!("aot unavailable (skip parity): {stem}");
        return DevBatteryStats {
            ran: 1,
            ..DevBatteryStats::default()
        };
    };
    let interpreted = normalize_for_parity(stem, interpreted);
    let compiled = normalize_for_parity(stem, compiled);
    if interpreted != compiled {
        if is_manifested_parity_divergence(stem, manifested_divergences) {
            eprintln!("manifested parity divergence: {stem}");
            return DevBatteryStats {
                ran: 1,
                manifested: 1,
                ..DevBatteryStats::default()
            };
        }
        assert_eq!(
            interpreted, compiled,
            "DIVERGENCE for `{}`: interpreter and compiled binary disagree on stdout/stderr/exit code — this is a P0 miscompile",
            stem
        );
    }
    DevBatteryStats {
        ran: 1,
        ..DevBatteryStats::default()
    }
}

fn run_interpreter_battery_parallel(
    stems: Vec<String>,
    dir: PathBuf,
    manifested_divergences: Vec<String>,
) -> DevBatteryStats {
    let jobs = Arc::new(Mutex::new(
        stems
            .into_iter()
            .enumerate()
            .collect::<std::collections::VecDeque<_>>(),
    ));
    let dir = Arc::new(dir);
    let manifested_divergences = Arc::new(manifested_divergences);
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let worker_count = test_worker_count(16);
    let mut handles = Vec::new();
    for _ in 0..worker_count {
        let jobs = Arc::clone(&jobs);
        let dir = Arc::clone(&dir);
        let manifested_divergences = Arc::clone(&manifested_divergences);
        let failures = Arc::clone(&failures);
        handles.push(
            std::thread::Builder::new()
                .spawn(move || {
                    let mut stats = DevBatteryStats::default();
                    loop {
                        let Some((i, stem)) =
                            lock_recovered(&jobs, "interpreter work queue").pop_front()
                        else {
                            break;
                        };
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            check_interpreter_stem(i, &stem, &dir, &manifested_divergences)
                        }));
                        match result {
                            Ok(next) => stats.add(next),
                            Err(payload) => lock_recovered(&failures, "interpreter failure report")
                                .push(format!("{stem}: {}", panic_message(payload))),
                        }
                    }
                    stats
                })
                .expect("interpreter worker"),
        );
    }

    let mut stats = DevBatteryStats::default();
    for handle in handles {
        stats.add(handle.join().expect("interpreter worker panicked outside harness"));
    }
    let failures = judged_report(&failures, "interpreter failure report");
    assert!(
        failures.is_empty(),
        "interpreter parity failures:\n{}",
        failures.join("\n\n")
    );
    report_oracle_cache_counts();
    stats
}

/// A scratch index no other in-flight oracle build can be using.
///
/// #2020: `assert_cranelift_three_way` hard-coded index 0, so every stem's
/// generated `.rs` landed on the same file name. That was invisible while the
/// battery judged one stem at a time; two stems judged at once would overwrite
/// each other's source. One counter, so every oracle build names its own
/// scratch no matter which thread asked for it.
fn next_oracle_index() -> usize {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Judge one curated stem list, each stem in its own child run of `test_name`.
///
/// #2020: ten tests carried a private copy of this loop and every copy spawned
/// its children strictly one at a time. That put ~160 optimized `rustc` builds
/// in series inside `--test dev`, which is most of why the 900s suite guard
/// aborted the binary before ~30 of its declared tests ever started. The
/// children were already isolated — own process, own pid-keyed scratch, own
/// thread-local JIT state — so the serialization bought nothing.
///
/// The FFI bridge is the one build artifact the children share, and the product
/// locks that cache per key across processes (#2075), so a bridge stem needs no
/// suite-level serialization. Ten copies of one loop is also why their timeouts
/// had already drifted apart with nothing naming the difference (AGENTS.md I8);
/// the timeout is a parameter now.
fn run_child_stem_battery(
    test_name: &str,
    child_env: &str,
    label: &str,
    timeout: Duration,
    stems: &[&str],
    extra_env: &[(&str, &str)],
) {
    let jobs = Arc::new(Mutex::new(
        stems
            .iter()
            .map(|stem| (*stem).to_string())
            .collect::<std::collections::VecDeque<_>>(),
    ));
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut handles = Vec::new();
    for worker in 0..test_worker_count(8) {
        let jobs = Arc::clone(&jobs);
        let failures = Arc::clone(&failures);
        let test_name = test_name.to_string();
        let child_env = child_env.to_string();
        let label = label.to_string();
        let extra_env: Vec<(String, String)> = extra_env
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        handles.push(
            std::thread::Builder::new()
                .name(format!("child-stem-battery-{worker}"))
                .spawn(move || loop {
                    let Some(stem) = lock_recovered(&jobs, "child stem battery queue").pop_front()
                    else {
                        break;
                    };
                    // Every sibling battery in this file already does this
                    // (`dev default`, `interpreter`, `three-way`, `corpus
                    // gate`): a per-stem panic becomes THIS stem's failure
                    // line, so the run keeps going and the report names the
                    // stem and its real message. Letting the panic ride the
                    // thread out instead cost the batch twice — `join()`
                    // re-panicked with a `Box<dyn Any>` that `expect` renders
                    // as `Any { .. }`, and every stem still queued behind the
                    // first casualty was never judged at all.
                    let judged = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut command =
                            Command::new(std::env::current_exe().expect("current dev test binary"));
                        command
                            .args(["--exact", test_name.as_str(), "--nocapture"])
                            .env(&child_env, &stem)
                            .env("NO_COLOR", "1");
                        for (key, value) in &extra_env {
                            command.env(key, value);
                        }
                        command_output_with_timeout(command, timeout, &format!("{label} `{stem}`"))
                    }));
                    match judged {
                        Ok(output) if output.status.success() => {}
                        Ok(output) => {
                            lock_recovered(&failures, "child stem battery failures").push(format!(
                                "{stem}: status={:?}\nstdout:\n{}\nstderr:\n{}",
                                output.status,
                                String::from_utf8_lossy(&output.stdout),
                                String::from_utf8_lossy(&output.stderr)
                            ));
                        }
                        Err(payload) => lock_recovered(&failures, "child stem battery failures")
                            .push(format!("{stem}: {}", panic_message(payload))),
                    }
                })
                .expect("child stem battery worker"),
        );
    }
    for handle in handles {
        handle
            .join()
            .expect("child stem battery worker panicked outside the per-stem guard");
    }
    let failures = judged_report(&failures, "child stem battery failures");
    assert!(
        failures.is_empty(),
        "{label} failures:\n{}",
        failures.join("\n")
    );
}

/// Judge the three-way universe in a bounded pool instead of one stem at a time.
///
/// #2020: this battery pays an optimized `rustc` build per stem over ~350 stems,
/// so on its own it outlasted the whole 900s suite budget and the binary never
/// reached its own summary. Every leg is per-thread state — `jet_jit` keeps the
/// trace flags, the program args and the resident module in thread-locals — and
/// `collect_corpus_gate_records` already judges the same corpus with
/// `test_worker_count(8)` workers doing the same three legs, so serial execution
/// was never what kept the legs honest.
///
/// Each worker enters `with_jit_test_scope` per stem: that is the product
/// boundary that installs the sized compiler stack on THIS thread, which a
/// default 2 MiB worker stack does not have.
///
/// A failing stem is named and the rest are still judged, exactly as
/// `run_interpreter_battery_parallel` does. The old walk stopped at the first
/// divergence, so one failure hid every other one.
fn run_three_way_battery_parallel(stems: Vec<String>) -> usize {
    let total = stems.len();
    let jobs = Arc::new(Mutex::new(std::collections::VecDeque::from(stems)));
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut handles = Vec::new();
    for worker in 0..test_worker_count(8) {
        let jobs = Arc::clone(&jobs);
        let failures = Arc::clone(&failures);
        let ran = Arc::clone(&ran);
        handles.push(
            std::thread::Builder::new()
                .name(format!("three-way-{worker}"))
                .spawn(move || loop {
                    let Some(stem) = lock_recovered(&jobs, "three-way work queue").pop_front()
                    else {
                        break;
                    };
                    eprintln!("three-way battery: checking `{stem}`");
                    let judged = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        with_jit_test_scope(|| {
                            assert_cranelift_three_way(&example_path(&stem), &stem)
                        })
                    }));
                    match judged {
                        Ok(()) => {
                            ran.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        Err(payload) => lock_recovered(&failures, "three-way failure report")
                            .push(format!("{stem}: {}", panic_message(payload))),
                    }
                })
                .expect("three-way battery worker"),
        );
    }
    for handle in handles {
        handle.join().expect("three-way battery worker panicked");
    }
    let failures = judged_report(&failures, "three-way failure report");
    assert!(
        failures.is_empty(),
        "three-way differential failures ({} of {total} stem(s)):\n{}",
        failures.len(),
        failures.join("\n\n")
    );
    ran.load(std::sync::atomic::Ordering::Relaxed)
}

fn check_job_runner_interpreter(root: &PathBuf, file: &str) {
    let mut bundle = jet::Loader::load_entry(file).expect("job_runner loads");
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "job_runner front end: {errors:?}");
    for (job, expected_name) in [
        ("greet", "job_runner.greet"),
        ("seed_data", "job_runner.seed_data"),
    ] {
        let expected = fs::read_to_string(
            root.join(format!("examples/features/expected/devloop/{expected_name}.out")),
        )
        .unwrap_or_else(|_| panic!("missing expected/devloop/{expected_name}.out"));
        match run_named_job(&bundle, job, false) {
            RunOutcome::Ran { stdout, .. } => assert_eq!(
                stdout, expected,
                "interpreter job `{job}` differs from golden"
            ),
            RunOutcome::Problems(diags) => panic!(
                "interpreter job `{job}` did not run: {:?}",
                diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
            ),
        }
    }
}

/// Load + check a fixture on the canonical compiler worker.
///
/// `Loader::load_entry` and `Sema::check_bundle` are the same unbounded-depth
/// recursive descent every compile entry runs, and reaching them directly --
/// as this helper does, and as any embedder holding its own bundle would --
/// skips the driver's own boundary. Without it a 2 MiB libtest worker aborts
/// the whole binary and reports every other in-flight test as failed.
fn checked_bundle_from_path(file: &str) -> jet::AST::ProgramBundle {
    jet::run_compiler_work(|| {
        let mut b = jet::Loader::load_entry(file).expect("bundle should load");
        let diags = jet::Sema::check_bundle(&mut b, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "fixture must type-check: {errors:?}");
        b
    })
}

fn assert_cranelift_matches_interpreter(src: &str, tag: &str) {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let p = std::env::temp_dir().join(format!("jet_jit_m3_{tag}.jet"));
    fs::write(&p, src).unwrap();
    let shown = p.to_string_lossy().to_string();

    let expected = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => {
            panic!("interpreter baseline must run `{tag}`, got diagnostics: {ds:?}")
        }
    };

    let bundle = checked_bundle_from_path(&shown);
    let mut backend = CraneliftBackend::new();
    let got = match backend.run(&bundle, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("cranelift backend did not run `{tag}`: {ds:?}"),
    };
    assert_eq!(
        got, expected,
        "cranelift output drifted from interpreter for `{tag}`"
    );
}

fn run_cranelift_outcome(src: &str, tag: &str) -> ProgramOutput {
    let p = std::env::temp_dir().join(format!("jet_jit_result_{tag}.jet"));
    fs::write(&p, src).unwrap();
    let shown = p.to_string_lossy().to_string();
    let bundle = checked_bundle_from_path(&shown);
    // #778: tiered Cranelift + silent deopt (E2211 retired).
    let mut backend = CraneliftBackend::new();
    match backend.run(&bundle, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(ds) => panic!("`{tag}` JIT returned diagnostics: {ds:?}"),
    }
}

fn run_cranelift_outcome_without_fallback(src: &str, tag: &str) -> RunOutcome {
    let p = std::env::temp_dir().join(format!("jet_jit_no_fallback_{tag}.jet"));
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, src).unwrap();
    let shown = p.to_string_lossy().to_string();
    let bundle = checked_bundle_from_path(&shown);
    // #778: coverage gaps silent-deopt; preserve the shared helper's existing
    // tiered behavior for unrelated dev tests.
    let mut backend = CraneliftBackend::new();
    backend.run(&bundle, false)
}

fn run_cranelift_without_fallback(src: &str, tag: &str) -> ProgramOutput {
    match run_cranelift_outcome_without_fallback(src, tag) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(ds) => panic!("`{tag}` JIT returned diagnostics: {ds:?}"),
    }
}

fn run_cranelift_resident_file_outcome(file: &str, tag: &str) -> RunOutcome {
    let bundle = checked_bundle_from_path(file);
    jet_jit::run_resident_strict_for_test(&bundle)
        .unwrap_or_else(|reason| panic!("`{tag}` resident JIT failed: {reason}"))
}

/// Run one resident-JIT fixture on the JIT test state scope. A successful
/// interpreter result is not resident-JIT evidence.
fn run_cranelift_resident(src: &str, tag: &str) -> ProgramOutput {
    let p = std::env::temp_dir().join(format!("jet_jit_resident_{tag}.jet"));
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, src).unwrap();
    let shown = p.to_string_lossy().to_string();
    run_cranelift_resident_file(&shown, tag)
}

fn run_cranelift_resident_file(file: &str, tag: &str) -> ProgramOutput {
    let file = file.to_owned();
    let tag = tag.to_owned();
    with_jit_test_scope(move || {
        jet_jit::reset_jit_trace_for_test();
        let outcome = run_cranelift_resident_file_outcome(&file, &tag);
        assert!(
            jet_jit::jit_executed_for_test(),
            "`{tag}` must execute resident Cranelift"
        );
        assert!(
            !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
            "`{tag}` must not deopt to the interpreter or use fallback"
        );
        match outcome {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(ds) => {
                panic!("`{tag}` resident JIT returned diagnostics: {ds:?}")
            }
        }
    })
}

fn run_default_dev_resident(file: &str, tag: &str) -> ProgramOutput {
    jet_jit::reset_jit_trace_for_test();
    let outcome = dev_iteration_with_timeout(tag, file, false);
    assert!(
        jet_jit::jit_executed_for_test(),
        "default dev `{tag}` must execute resident Cranelift"
    );
    if jet_jit::deopt_invoked_for_test() || jet_jit::fallback_invoked_for_test() {
        let bundle = checked_bundle_from_path(file);
        panic!(
            "default dev `{tag}` must not deopt to the interpreter or use fallback; tier plan: {:?}",
            jet_jit::plan_bundle_tiers(&bundle)
        );
    }
    match outcome {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(ds) => panic!("default dev `{tag}` returned diagnostics: {ds:?}"),
    }
}

/// Drive the real CLI default backend. `--trace-tiers` is the observable
/// proof that this child process stayed in resident Cranelift.
fn run_cli_default_resident(command: &str, file: &str, tag: &str) -> ProgramOutput {
    let cache = common::unique_tmp(&format!("jet_{tag}_cache"));
    fs::create_dir_all(&cache).unwrap();
    let mut args = vec![command, file, "--trace-tiers"];
    if command == "dev" {
        args.extend(["--watch=off", "--quiet"]);
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jet"));
    cmd.args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("NO_COLOR", "1")
        .env("JET_RUN_CACHE_DIR", &cache)
        .env("JET_CACHE_DIR", cache.join("build"));
    let output = command_output_with_timeout(
        cmd,
        *DEV_DIFF_TIMEOUT,
        &format!("default CLI {command} for `{tag}`"),
    );
    assert!(
        output.status.success(),
        "default CLI {command} failed for `{tag}`:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let trace = String::from_utf8_lossy(&output.stderr);
    assert!(
        trace.contains("tier1 native"),
        "default CLI {command} did not report native Cranelift for `{tag}`:\n{trace}"
    );
    assert!(
        !trace.contains("tier0 interp"),
        "default CLI {command} deopted to the interpreter for `{tag}`:\n{trace}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stdout = match stdout.find("✓ ran in ") {
        Some(end) => stdout[..end].to_string(),
        None => stdout,
    };
    let _ = fs::remove_dir_all(&cache);
    ProgramOutput::ran(stdout, String::new(), output.status.code().unwrap_or(1))
}

fn assert_cli_diagnostic_snapshot(command: &str, fixture: &str, snapshot: &str) {
    let mut args = vec![command, fixture];
    if command == "dev" {
        args.extend(["--watch=off", "--quiet"]);
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jet"));
    cmd.args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("NO_COLOR", "1");
    let output = command_output_with_timeout(
        cmd,
        *DEV_DIFF_TIMEOUT,
        &format!("default CLI {command} diagnostic"),
    );
    assert!(
        !output.status.success(),
        "default CLI {command} unexpectedly accepted `{fixture}`"
    );
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        rendered.starts_with(snapshot),
        "default CLI {command} diagnostic drifted from UI snapshot:\n{rendered}"
    );
}

fn golden_stdout(stem: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(format!("examples/features/expected/{stem}.out")))
        .unwrap_or_else(|e| panic!("missing golden for `{stem}`: {e}"))
}

fn assert_cranelift_three_way(file: &str, stem: &str) {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    // Same single build-fact snapshot the corpus gate seeds before sema (see
    // `corpus_gate_record`) and that `compile_with_path` gives the AOT oracle.
    // Without it this harness builds a DIFFERENT bundle from the gate, so the
    // two differential oracles disagree on every `@build.*` read and the
    // weaker one silently passes (I9: engines marshal one snapshot).
    jet::Driver::seed_build_facts(&mut bundle, "dev", false, &std::collections::BTreeMap::new())
        .unwrap_or_else(|diags| {
            panic!(
                "`{stem}` build facts should seed: {}",
                diags
                    .iter()
                    .map(|d| format!("{}: {}", d.code, d.what))
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        });
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");
    let safety_detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
    let compile = jet_jit::try_compile_bundle(&bundle);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle) && compile.is_ok(),
        "`{stem}` must be resident-JIT safe for three-way differential: safety={safety_detail:?}, compile={compile:?}"
    );

    // Honest pre-scan / TIR boundaries (typed CLI, env, etc.) skip the
    // interpreter leg — same contract as assert_ui_and_web_three_way. JIT and
    // AOT must still agree with no deopt/fallback.
    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Some(ProgramOutput::ran(stdout, stderr, exit_code)),
        RunOutcome::Problems(diags)
            if diags
                .iter()
                .any(|d| d.code == "E2201" || d.code == "E0956" || d.code == "E1265") =>
        {
            None
        }
        RunOutcome::Problems(ds) => {
            panic!("interpreter baseline must run `{stem}` or stop at E2201/E0956/E1265, got: {ds:?}")
        }
    };

    jet_jit::reset_jit_trace_for_test();
    let mut backend = CraneliftBackend::new();
    let jit = jet_jit::with_program_args(&[file.to_string()], || {
        match backend.run(&bundle, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(ds) => panic!("cranelift backend did not run `{stem}`: {ds:?}"),
        }
    });
    assert!(
        jet_jit::jit_executed_for_test(),
        "`{stem}` did not execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "`{stem}` used deopt or fallback"
    );
    let jit = normalize_for_parity(stem, jit);
    if let Some(interpreted) = interpreted {
        let interpreted = normalize_for_parity(stem, interpreted);
        assert_eq!(
            jit, interpreted,
            "JIT vs interpreter divergence for `{stem}`"
        );
    }

    let dir = aot_scratch_dir("jit_3way", stem);
    let aot = normalize_for_parity(
        stem,
        compiled_binary_output(&dir, "jit_3way", next_oracle_index(), stem, file),
    );
    assert_eq!(jit, aot, "JIT vs AOT divergence for `{stem}`");
}

fn assert_ui_and_web_three_way(file: &str, stem: &str) {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");
    let safety_detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
    let compile = jet_jit::try_compile_bundle(&bundle);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle) && compile.is_ok(),
        "`{stem}` must be resident-JIT safe for three-way differential: safety={safety_detail:?}, compile={compile:?}"
    );

    let expected = if stem == "web/web_wasm_callback" {
        // Native `run()` is empty; golden is the wasm/node harness (`42`).
        ProgramOutput::ran(String::new(), String::new(), 0)
    } else {
        ProgramOutput::ran(golden_stdout(stem), String::new(), 0)
    };

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Some(ProgramOutput::ran(stdout, stderr, exit_code)),
        RunOutcome::Problems(diags)
            if diags
                .iter()
                .any(|d| d.code == "E2201" || d.code == "E0956" || d.code == "E1265") =>
        {
            None
        }
        RunOutcome::Problems(diags) => {
            panic!("interpreter baseline must run `{stem}` or stop at E2201/E0956/E1265, got: {diags:?}")
        }
    };

    jet_jit::reset_jit_trace_for_test();
    let mut backend = CraneliftBackend::new();
    let jit = jet_jit::with_program_args(&[file.to_string()], || {
        match backend.run(&bundle, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(ds) => panic!("cranelift backend did not run `{stem}`: {ds:?}"),
        }
    });
    assert!(
        jet_jit::jit_executed_for_test(),
        "`{stem}` did not execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "`{stem}` used deopt or fallback"
    );

    let dir = aot_scratch_dir("jit_1225", stem);
    let aot = compiled_binary_output(&dir, "jit_1225", 0, stem, file);

    if let Some(interpreted) = interpreted {
        assert_eq!(
            interpreted, expected,
            "interpreter drifted from golden for `{stem}`"
        );
        assert_eq!(
            jit, interpreted,
            "JIT vs interpreter divergence for `{stem}`"
        );
    } else {
        assert_eq!(jit, expected, "JIT drifted from golden for `{stem}`");
    }
    assert_eq!(jit, aot, "JIT vs AOT divergence for `{stem}`");
}

fn assert_concurrency_and_game_three_way(file: &str, stem: &str) {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");
    let safety_detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
    let compile = jet_jit::try_compile_bundle(&bundle);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle) && compile.is_ok(),
        "`{stem}` must be resident-JIT safe for three-way differential: safety={safety_detail:?}, compile={compile:?}"
    );

    let expected = if stem == "concurrency/deadline_context" {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut stderr = fs::read_to_string(
            root.join("examples/features/expected/concurrency/deadline_context.err.out"),
        )
        .expect("deadline_context.err.out");
        if !stderr.ends_with('\n') {
            stderr.push('\n');
        }
        ProgramOutput::ran(golden_stdout(stem), stderr, 70)
    } else {
        ProgramOutput::ran(golden_stdout(stem), String::new(), 0)
    };

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Some(ProgramOutput::ran(stdout, stderr, exit_code)),
        RunOutcome::Problems(diags)
            if diags
                .iter()
                .any(|d| d.code == "E2201" || d.code == "E0956" || d.code == "E1265") =>
        {
            // Native concurrency/game surfaces stop at the honest interpreter
            // boundary; golden + resident JIT + AOT carry ProgramOutput parity.
            None
        }
        RunOutcome::Problems(diags) => {
            panic!("interpreter baseline must run `{stem}` or stop at E2201/E0956/E1265, got: {diags:?}")
        }
    };

    jet_jit::reset_jit_trace_for_test();
    let mut backend = CraneliftBackend::new();
    let jit = jet_jit::with_program_args(&[file.to_string()], || {
        match backend.run(&bundle, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(ds) => panic!("cranelift backend did not run `{stem}`: {ds:?}"),
        }
    });
    assert!(
        jet_jit::jit_executed_for_test(),
        "`{stem}` did not execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "`{stem}` used deopt or fallback"
    );

    let dir = aot_scratch_dir("jit_1218", stem);
    let aot = compiled_binary_output(&dir, "jit_1218", 0, stem, file);

    if let Some(interpreted) = interpreted {
        assert_eq!(
            interpreted, expected,
            "interpreter drifted from golden for `{stem}`"
        );
        assert_eq!(
            jit, interpreted,
            "JIT vs interpreter divergence for `{stem}`"
        );
    } else {
        assert_eq!(jit, expected, "JIT drifted from golden for `{stem}`");
    }
    assert_eq!(jit, aot, "JIT vs AOT divergence for `{stem}`");
}

fn assert_crypto_auth_vault_three_way(file: &str, stem: &str) {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");
    let safety_detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
    let compile = jet_jit::try_compile_bundle(&bundle);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle) && compile.is_ok(),
        "`{stem}` must be resident-JIT safe for three-way differential: safety={safety_detail:?}, compile={compile:?}"
    );

    let expected = ProgramOutput::ran(golden_stdout(stem), String::new(), 0);

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Some(ProgramOutput::ran(stdout, stderr, exit_code)),
        RunOutcome::Problems(diags)
            if diags.iter().any(|d| d.code == "E2201" || d.code == "E1265") =>
        {
            // Native crypto/auth/vault surfaces stop at the honest interpreter
            // boundary; golden + resident JIT + AOT carry ProgramOutput parity.
            None
        }
        RunOutcome::Problems(diags) => {
            panic!("interpreter baseline must run `{stem}` or stop at E2201/E1265, got: {diags:?}")
        }
    };

    jet_jit::reset_jit_trace_for_test();
    let mut backend = CraneliftBackend::new();
    let jit = jet_jit::with_program_args(&[file.to_string()], || {
        match backend.run(&bundle, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(ds) => panic!("cranelift backend did not run `{stem}`: {ds:?}"),
        }
    });
    assert!(
        jet_jit::jit_executed_for_test(),
        "`{stem}` did not execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "`{stem}` used deopt or fallback"
    );

    let dir = aot_scratch_dir("jit_1222", stem);
    let aot = compiled_binary_output(&dir, "jit_1222", 0, stem, file);

    if let Some(interpreted) = interpreted {
        assert_eq!(
            interpreted, expected,
            "interpreter drifted from golden for `{stem}`"
        );
        assert_eq!(
            jit, interpreted,
            "JIT vs interpreter divergence for `{stem}`"
        );
    } else {
        assert_eq!(jit, expected, "JIT drifted from golden for `{stem}`");
    }
    assert_eq!(jit, aot, "JIT vs AOT divergence for `{stem}`");
}

fn assert_network_http_browser_three_way(file: &str, stem: &str) {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");
    let safety_detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
    let compile = jet_jit::try_compile_bundle(&bundle);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle) && compile.is_ok(),
        "`{stem}` must be resident-JIT safe for three-way differential: safety={safety_detail:?}, compile={compile:?}"
    );

    let expected = ProgramOutput::ran(golden_stdout(stem), String::new(), 0);

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Some(ProgramOutput::ran(stdout, stderr, exit_code)),
        RunOutcome::Problems(diags)
            if diags
                .iter()
                .any(|d| d.code == "E2201" || d.code == "E0956" || d.code == "E1265") =>
        {
            // Live network/HTTP/WS stop at the honest interpreter boundary
            // (E3412 comptime voice is rewritten to E2201 in Source/Interpreter);
            // golden + resident JIT + AOT carry ProgramOutput parity.
            None
        }
        RunOutcome::Problems(diags) => {
            panic!(
                "interpreter baseline must run `{stem}` or stop at E2201/E0956/E1265, got: {diags:?}"
            )
        }
    };

    jet_jit::reset_jit_trace_for_test();
    let mut backend = CraneliftBackend::new();
    let jit = jet_jit::with_program_args(&[file.to_string()], || {
        match backend.run(&bundle, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(ds) => panic!("cranelift backend did not run `{stem}`: {ds:?}"),
        }
    });
    assert!(
        jet_jit::jit_executed_for_test(),
        "`{stem}` did not execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "`{stem}` used deopt or fallback"
    );

    let dir = aot_scratch_dir("jit_1221", stem);
    let aot = compiled_binary_output(&dir, "jit_1221", 0, stem, file);

    if let Some(interpreted) = interpreted {
        assert_eq!(
            interpreted, expected,
            "interpreter drifted from golden for `{stem}`"
        );
        assert_eq!(
            jit, interpreted,
            "JIT vs interpreter divergence for `{stem}`"
        );
    } else {
        assert_eq!(jit, expected, "JIT drifted from golden for `{stem}`");
    }
    assert_eq!(jit, aot, "JIT vs AOT divergence for `{stem}`");
}

fn golden_program_output(stem: &str) -> ProgramOutput {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if example_has_err_golden(stem) {
        let stderr = fs::read_to_string(
            root.join(format!("examples/features/expected/{stem}.err.out")),
        )
        .unwrap_or_else(|e| panic!("missing err golden for `{stem}`: {e}"));
        return ProgramOutput::ran(String::new(), stderr, 70);
    }
    ProgramOutput::ran(golden_stdout(stem), String::new(), 0)
}

fn strip_panic_locals(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|line| !line.starts_with("locals:"))
        .collect::<Vec<_>>()
        .join("\n")
        + if stderr.ends_with('\n') { "\n" } else { "" }
}

fn assert_data_pipelines_parsing_three_way(file: &str, stem: &str) {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");
    let safety_detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
    let compile = jet_jit::try_compile_bundle(&bundle);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle) && compile.is_ok(),
        "`{stem}` must be resident-JIT safe for three-way differential: safety={safety_detail:?}, compile={compile:?}"
    );

    let expected = golden_program_output(stem);

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Some(ProgramOutput::ran(stdout, stderr, exit_code)),
        RunOutcome::Problems(diags)
            if diags.iter().any(|d| d.code == "E2201" || d.code == "E1265" || d.code == "E0956") =>
        {
            None
        }
        RunOutcome::Problems(diags) => {
            panic!("interpreter baseline must run `{stem}` or stop at known boundary, got: {diags:?}")
        }
    };

    if stem == "reflection/reflect-value" {
        assert_eq!(
            interpreted.as_ref(),
            Some(&expected),
            "reflection example must match its golden output in the interpreter"
        );
    }

    jet_jit::reset_jit_trace_for_test();
    let mut backend = CraneliftBackend::new();
    let jit = jet_jit::with_program_args(&[file.to_string()], || {
        match backend.run(&bundle, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(ds) => panic!("cranelift backend did not run `{stem}`: {ds:?}"),
        }
    });
    assert!(
        jet_jit::jit_executed_for_test(),
        "`{stem}` did not execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "`{stem}` used deopt or fallback"
    );

    let dir = aot_scratch_dir("jit_1223", stem);
    let aot = compiled_binary_output(&dir, "jit_1223", 0, stem, file);

    // Golden + resident JIT + AOT own ProgramOutput. Interpreter is compared when
    // it already matches the golden (DataError Display / panic locals may differ).
    assert_eq!(jit, expected, "JIT drifted from golden for `{stem}`");
    if stem == "tooling/panic_report" {
        let body = |out: &ProgramOutput| {
            ProgramOutput::ran(
                out.stdout.clone(),
                strip_panic_locals(&out.stderr),
                out.exit_code,
            )
        };
        assert_eq!(
            body(&jit),
            body(&aot),
            "JIT vs AOT panic body divergence for `{stem}`"
        );
        if let Some(interpreted) = &interpreted {
            assert_eq!(
                body(interpreted),
                body(&expected),
                "interpreter panic body drifted from golden for `{stem}`"
            );
        }
    } else {
        assert_eq!(jit, aot, "JIT vs AOT divergence for `{stem}`");
        if let Some(interpreted) = &interpreted {
            if interpreted == &expected {
                assert_eq!(
                    jit, *interpreted,
                    "JIT vs interpreter divergence for `{stem}`"
                );
            }
        }
    }
}

fn core_os_examples_match_interpreter_jit_and_aot_inner() {
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    for stem in ["io/os_facts", "io/os_process_control"] {
        assert_io_cli_terminal_time_three_way(&example_path(stem), stem);
    }
}

fn golden_stderr(stem: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join(format!("examples/features/expected/{stem}.stderr.out"));
    if path.is_file() {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("read stderr golden for `{stem}`: {e}"))
    } else {
        String::new()
    }
}

/// #2017: the three-tier differential for an example that reads stdin.
///
/// The same claim its caller makes — interpreter, resident JIT and AOT all
/// reproduce the checked-in golden byte for byte — with the one difference that
/// makes the claim true here: every tier is a child, and every child is fed the
/// answers the golden was recorded with, from `common::example_stdin`.
///
/// Each tier gets its own file of answers rather than one shared pipe, because
/// the first reader would otherwise drain it and the other two would silently
/// run with no input — a pass that means "three tiers agree about nothing".
fn assert_interactive_example_three_way(file: &str, stem: &str, answers: &str) {
    let golden = ProgramOutput::ran(golden_stdout(stem), golden_stderr(stem), 0);

    // Which tier answered, then what it printed. Two runs, because the trace
    // that proves the tier lands on the stderr the golden pins.
    assert_cli_tier_answered(file, stem, answers, true);
    let interpreted = cli_tier_program_output(file, stem, answers, true);
    assert_eq!(
        interpreted, golden,
        "forced interpreter drifted from the golden for `{stem}` with the answers it was \
         recorded with"
    );

    assert_cli_tier_answered(file, stem, answers, false);
    let jit = cli_tier_program_output(file, stem, answers, false);
    assert_eq!(
        jit, golden,
        "resident JIT drifted from the golden for `{stem}` with the answers it was recorded with"
    );

    if !have_rustc() {
        eprintln!(
            "note: rustc not found; `{stem}` compared on interpreter and resident JIT only"
        );
        return;
    }
    let dir = aot_scratch_dir("stdin_three_way", stem);
    let aot = compiled_binary_output_with_stdin(
        &dir,
        "stdin_three_way",
        0,
        stem,
        file,
        Some(answers),
    );
    assert_eq!(
        aot, golden,
        "AOT drifted from the golden for `{stem}` with the answers it was recorded with"
    );
}

fn assert_io_cli_terminal_time_three_way(file: &str, stem: &str) {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");
    let safety_detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
    let compile = jet_jit::try_compile_bundle(&bundle);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle) && compile.is_ok(),
        "`{stem}` must be resident-JIT safe for three-way differential: safety={safety_detail:?}, compile={compile:?}"
    );

    // #2017: an interactive example is only comparable against the answers its
    // golden was recorded with, and this battery fed NOTHING — so a pass here
    // meant the three tiers agreed on a no-input run while the golden described
    // a fed run. The answers come from `common::example_stdin`, the one home
    // for them (I8), and each tier gets its own child so each gets its own fd 0.
    // The in-process resident-JIT safety proof above still runs; the tier
    // attribution below moves to `--trace-tiers`, which is the out-of-process
    // form of the same claim (`run_cli_default_resident` already reads it).
    if let Some(answers) = common::example_stdin(stem) {
        assert_interactive_example_three_way(file, stem, answers.piped);
        return;
    }

    let golden = ProgramOutput::ran(golden_stdout(stem), golden_stderr(stem), 0);

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            let out = ProgramOutput::ran(stdout, stderr, exit_code);
            if out == golden {
                Some(out)
            } else {
                None
            }
        }
        RunOutcome::Problems(diags)
            if diags.iter().any(|d| d.code == "E2201" || d.code == "E1265" || d.code == "E0956") =>
        {
            None
        }
        RunOutcome::Problems(diags) => {
            panic!(
                "interpreter baseline must run `{stem}` or stop at E2201/E1265/E0956, got: {diags:?}"
            )
        }
    };

    let dir = aot_scratch_dir("jit_1219", stem);
    let aot = compiled_binary_output(&dir, "jit_1219", 0, stem, file);
    let aot_bin = compiled_binary_path(
        &aot_scratch_dir("jit_1219", stem),
        "jit_1219",
        0,
        file,
    );

    // Watcher re-execs `os.executable() --watch-child`. Under resident JIT,
    // point argv[0] at the AOT binary so the child is the same program identity
    // AOT uses (not a `.jet` → `jet run` round-trip).
    let jit_argv0 = if stem == "io/watcher" {
        aot_bin.to_string_lossy().into_owned()
    } else {
        file.to_string()
    };

    jet_jit::reset_jit_trace_for_test();
    let mut backend = CraneliftBackend::new();
    let jit = jet_jit::with_program_args(&[jit_argv0], || {
        match backend.run(&bundle, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(ds) => panic!("cranelift backend did not run `{stem}`: {ds:?}"),
        }
    });
    assert!(
        jet_jit::jit_executed_for_test(),
        "`{stem}` did not execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "`{stem}` used deopt or fallback"
    );

    // AOT is the ProgramOutput oracle; golden pins stdout. Dev `?` traces may
    // land on stderr under JIT while release-flavored AOT goldens stay quiet.
    assert_eq!(
        aot.stdout, golden.stdout,
        "AOT stdout drifted from golden for `{stem}`"
    );
    assert_eq!(jit.stdout, aot.stdout, "JIT vs AOT stdout divergence for `{stem}`");
    assert_eq!(
        jit.exit_code, aot.exit_code,
        "JIT vs AOT exit divergence for `{stem}`"
    );
    if let Some(interpreted) = interpreted {
        assert_eq!(
            interpreted.stdout, golden.stdout,
            "interpreter stdout drifted from golden for `{stem}`"
        );
    }
    // Prefer exact stderr when golden declares it; otherwise accept AOT stderr.
    let expected_stderr = if !golden.stderr.is_empty() {
        golden.stderr.clone()
    } else {
        aot.stderr.clone()
    };
    assert_eq!(
        jit.stderr, expected_stderr,
        "JIT stderr divergence for `{stem}`"
    );
    assert_eq!(
        aot.stderr, expected_stderr,
        "AOT stderr divergence for `{stem}`"
    );
}

fn assert_lowlevel_and_safety_three_way(file: &str, stem: &str) {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check: {errors:?}");
    let safety_detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
    let compile = jet_jit::try_compile_bundle(&bundle);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle) && compile.is_ok(),
        "`{stem}` must be resident-JIT safe for three-way differential: safety={safety_detail:?}, compile={compile:?}"
    );

    let golden = ProgramOutput::ran(golden_stdout(stem), golden_stderr(stem), 0);

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            let out = ProgramOutput::ran(stdout, stderr, exit_code);
            if out.stdout == golden.stdout {
                Some(out)
            } else {
                // Interpreter may still stop short of release-flavored golden
                // precision (F32 display); golden + JIT + AOT carry the oracle.
                None
            }
        }
        RunOutcome::Problems(diags)
            if diags
                .iter()
                .any(|d| d.code == "E2201" || d.code == "E1265" || d.code == "E0956") =>
        {
            None
        }
        RunOutcome::Problems(diags) => {
            panic!(
                "interpreter baseline must run `{stem}` or stop at E2201/E1265/E0956, got: {diags:?}"
            )
        }
    };

    jet_jit::reset_jit_trace_for_test();
    let mut backend = CraneliftBackend::new();
    let jit = jet_jit::with_program_args(&[file.to_string()], || {
        match backend.run(&bundle, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(ds) => panic!("cranelift backend did not run `{stem}`: {ds:?}"),
        }
    });
    assert!(
        jet_jit::jit_executed_for_test(),
        "`{stem}` did not execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "`{stem}` used deopt or fallback"
    );

    let dir = aot_scratch_dir("jit_1220", stem);
    let aot = compiled_binary_output(&dir, "jit_1220", 0, stem, file);

    if let Some(interpreted) = interpreted {
        assert_eq!(
            interpreted.stdout, golden.stdout,
            "interpreter stdout drifted from golden for `{stem}`"
        );
        assert_eq!(
            jit.stdout, interpreted.stdout,
            "JIT vs interpreter stdout divergence for `{stem}`"
        );
    } else {
        assert_eq!(
            jit.stdout, golden.stdout,
            "JIT stdout drifted from golden for `{stem}`"
        );
    }
    assert_eq!(
        aot.stdout, golden.stdout,
        "AOT stdout drifted from golden for `{stem}`"
    );
    assert_eq!(
        jit.stdout, aot.stdout,
        "JIT vs AOT stdout divergence for `{stem}`"
    );
    assert_eq!(
        jit.exit_code, aot.exit_code,
        "JIT vs AOT exit divergence for `{stem}`"
    );
}

/// Pin the CLIF Jet emits at a site that once produced verifier-invalid code
/// (#1989, #1990, #1991).
///
/// This failure class is INVISIBLE from stdout alone. Cranelift's verifier
/// rejects the function, the tier silently deopts to the canonical
/// interpreter, and the program still prints the right answer — which is
/// exactly how the nine census members survived. Correct output is therefore
/// necessary but nowhere near sufficient; four facts have to hold together:
///
///   1. the repaired site is actually inside the emitted program, so a green
///      run is evidence about *that* site (`jit_program_func_names`),
///   2. every emitted function passes the verifier — `try_compile_bundle`
///      hands back the verifier's own text, so a regression names its own
///      instruction and both disagreeing types instead of returning silently,
///   3. the tier plan binds the whole program to resident Cranelift, and
///   4. the run really executed native code: no deopt, no fallback.
///
/// Invalid CLIF that Jet itself emitted is a compiler bug of I2's severity, so
/// every one of these is a hard failure rather than a recorded gap.
fn assert_resident_clif_shape(
    tag: &str,
    source: &str,
    require_funcs: &[&str],
    expected_stdout: &str,
) {
    let dir = std::env::temp_dir().join(format!("jet_clif_shape_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{tag}.jet"));
    fs::write(&file, source).unwrap();
    let shown = file.to_string_lossy().to_string();
    let bundle = checked_bundle_from_path(&shown);

    let funcs = jet_jit::jit_program_func_names(&bundle);
    for want in require_funcs {
        assert!(
            funcs.iter().any(|name| name.as_str() == *want),
            "`{tag}` no longer emits `{want}`, so it cannot pin that site; emitted: {funcs:?}"
        );
    }

    jet_jit::try_compile_bundle(&bundle).unwrap_or_else(|reason| {
        panic!("`{tag}` emitted CLIF that Cranelift's own verifier rejects: {reason}")
    });

    let plan = jet_jit::plan_bundle_tiers(&bundle);
    assert!(
        !plan.whole_interp && plan.deopt.is_empty(),
        "`{tag}` must plan entirely on the resident tier or its output proves nothing \
         about emitted CLIF: whole_interp={}, deopt={:?}, safety={:?}",
        plan.whole_interp,
        plan.deopt,
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );

    jet_jit::reset_jit_trace_for_test();
    let resident = run_cranelift_without_fallback(source, tag);
    assert!(
        jet_jit::jit_executed_for_test(),
        "`{tag}` did not execute resident Cranelift"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "`{tag}` used deopt or fallback, so its output is not resident-JIT evidence"
    );
    assert_eq!(
        resident.stdout, expected_stdout,
        "`{tag}` resident JIT output drifted (stderr: {:?})",
        resident.stderr
    );
    assert_eq!(
        resident.exit_code, 0,
        "`{tag}` resident JIT exited {} (stderr: {:?})",
        resident.exit_code, resident.stderr
    );
    let _ = fs::remove_dir_all(&dir);
}

/// State the universe the compile audit measured, every time it reports.
///
/// #1998: `gaps: 0` is only ever a claim about the stems the in-process harness
/// could put in front of the lowerer. Printing how many stems it could NOT, in
/// the same breath, is what stops the zero from being quoted on its own.
fn report_jit_universe(
    corpus: usize,
    covered: &[String],
    gaps: &[String],
    out_of_universe: &[String],
) {
    eprintln!(
        "jit compile-coverage universe: {corpus} example stem(s) = {} compile-covered + {} \
         compile gap(s) + {} outside this oracle",
        covered.len(),
        gaps.len(),
        out_of_universe.len()
    );
    if out_of_universe.is_empty() {
        eprintln!("jit stems outside this oracle: none — the audit judged the whole corpus");
        return;
    }
    eprintln!(
        "jit stems outside this oracle ({}): the {} gap row(s) are scoped to the {} stem(s) \
         this oracle could judge, NOT to the {corpus}-stem corpus (#1998)",
        out_of_universe.len(),
        gaps.len(),
        covered.len() + gaps.len()
    );
    for row in out_of_universe {
        eprintln!("  {row}");
    }
}

fn jit_coverage_audit_inner() {
    let JitCoverage {
        corpus,
        covered,
        gaps,
        out_of_universe,
        // #2012: the resident split of `covered` belongs to
        // `cranelift_three_way_differential_battery`, which states and pins it
        // there. This ratchet is about compile coverage only.
        ..
    } = collect_jit_coverage();
    let run_gaps = corpus_gate_run_gaps(&parse_corpus_gate_manifest());
    eprintln!("jit coverage: gaps: {}, run_gaps: {}", gaps.len(), run_gaps.len());
    report_jit_universe(corpus, &covered, &gaps, &out_of_universe);
    eprintln!("jit compile-covered ({}):", covered.len());
    for s in &covered {
        eprintln!("  {s}");
    }
    // #1998: never print a bare `gaps: N`. The count is over the stems this
    // oracle could judge, and the line says so wherever it is quoted.
    eprintln!(
        "jit gaps ({} of {} stem(s) judged; {} stem(s) unjudged):",
        gaps.len(),
        covered.len() + gaps.len(),
        out_of_universe.len()
    );
    for g in &gaps {
        eprintln!("  {g}");
    }
    print_jit_op_report();

    // #1998: the universe pins the corpus and the compile oracle, so coverage
    // cannot be greened by a stem quietly leaving the denominator.
    assert!(
        corpus >= EXAMPLE_CORPUS_FLOOR,
        "the example corpus shrank to {corpus} stem(s) (floor {EXAMPLE_CORPUS_FLOOR}). A stem \
         leaving `examples/features/<topic>/` shrinks every claim this audit makes: restore \
         it, or lower the floor in the same diff as the deletion."
    );

    // Every out-of-universe row is a defect to fix; the count is derived, never
    // an allowlist.
    assert!(
        out_of_universe.len() <= OUT_OF_UNIVERSE_CEILING,
        "{} stem(s) sit outside the compile oracle (ceiling {OUT_OF_UNIVERSE_CEILING}); this \
         count may only fall:\n{}",
        out_of_universe.len(),
        out_of_universe.join("\n")
    );
    assert!(
        !out_of_universe.is_empty() || OUT_OF_UNIVERSE_CEILING == 0,
        "the oracle now judges the whole corpus: set OUT_OF_UNIVERSE_CEILING to 0 in the same \
         diff, so the universe hole cannot reopen unnoticed"
    );
    assert!(
        gaps.is_empty(),
        "JIT compile coverage has {} gap(s); fix the lowering path before changing the corpus \
         claim:\n{}",
        gaps.len(),
        gaps.join("\n")
    );
    assert!(
        run_gaps.is_empty(),
        "JIT run-tier parity has {} gap(s); fix the shared semantics:\n{}",
        run_gaps.len(),
        run_gaps.join("\n")
    );
}

fn cranelift_three_way_differential_battery_inner() {
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping three-way JIT differential battery");
        return;
    }

    // Focused stem: `JET_THREE_WAY_STEM=io/files_depth cargo test --test dev cranelift_three_way_differential_battery`
    if let Ok(stem) = std::env::var("JET_THREE_WAY_STEM") {
        assert_cranelift_three_way(&example_path(&stem), &stem);
        eprintln!(
            "three-way battery: focused stem `{stem}` ok — this run is scoped to that ONE stem \
             and is NOT the battery"
        );
        return;
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // #2012: one oracle, one universe. This battery is scoped to the
    // `resident_safe` bucket of the shared `collect_jit_coverage` walk — the same
    // observation `jit_coverage_audit` ratchets.
    //
    // It used to walk the corpus itself, in `jit_covered_example_stems`, with the
    // two silent drops #1998 removed from the audit: `Err(_) => continue` on a
    // loader failure and a bare `continue` on a sema error. A stem this battery
    // could not even load was therefore never reported as untested — it left the
    // denominator without a word and the battery still passed. Reading the shared
    // observation makes that impossible twice over: the walk is total, and a
    // second oracle that could disagree with the first no longer exists.
    let JitCoverage {
        corpus,
        covered,
        resident_safe,
        resident_refused,
        gaps,
        out_of_universe,
    } = collect_jit_coverage();

    // State the universe before spending an hour of AOT builds inside it, and
    // pin it before trusting anything measured over it. These are the audit's own
    // pins, asserted here because this battery inherits the audit's denominator:
    // a stem cannot leave this battery by leaving the corpus, by becoming
    // unjudgeable by the compile oracle, or by quietly failing the resident gate.
    report_jit_universe(corpus, &covered, &gaps, &out_of_universe);
    assert!(
        corpus >= EXAMPLE_CORPUS_FLOOR,
        "the example corpus shrank to {corpus} stem(s) (floor {EXAMPLE_CORPUS_FLOOR}); this \
         battery is scoped to a subset of it, so a smaller corpus silently shrinks the battery"
    );
    assert!(
        out_of_universe.len() <= OUT_OF_UNIVERSE_CEILING,
        "{} stem(s) sit outside the compile oracle (ceiling {OUT_OF_UNIVERSE_CEILING}), so this \
         battery never judged them; the count may only fall:\n{}",
        out_of_universe.len(),
        out_of_universe.join("\n")
    );
    assert!(
        resident_safe.len() >= RESIDENT_SAFE_FLOOR,
        "the resident-safe universe shrank to {} stem(s) of {} compile-covered (floor \
         {RESIDENT_SAFE_FLOOR}). A stem leaving the resident gate leaves this battery: fix the \
         gate or the lowering it refuses, and only lower the floor in a reviewed diff. Refused \
         now:\n{}",
        resident_safe.len(),
        covered.len(),
        resident_refused.join("\n")
    );

    // Say it before running, not only after: a differential failure inside the
    // loop must not be the reason nobody learns what the battery was scoped to.
    eprintln!(
        "three-way battery scope: {} resident-safe stem(s) of {} compile-covered in a \
         {corpus}-stem corpus; {} compile-covered stem(s) refused by the resident gate, {} \
         compile gap(s) and {} stem(s) outside the compile oracle are NOT tested here",
        resident_safe.len(),
        covered.len(),
        resident_refused.len(),
        gaps.len(),
        out_of_universe.len()
    );

    // Every stem of the stated universe lands in exactly one of these three, so
    // no stem can drop out of the battery without a counted reason.
    let mut to_run: Vec<String> = Vec::new();
    let mut no_golden = Vec::new();
    let mut held_back = Vec::new();
    for stem in &resident_safe {
        if !root
            .join(format!("examples/features/expected/{stem}.out"))
            .exists()
        {
            no_golden.push(stem.clone());
            continue;
        }
        // #2016: a service entry serves until it is stopped, so running it here
        // spends the battery's timeout and, for the three `App` examples, races
        // a fixed port. Held back by the DERIVED predicate rather than by name,
        // so a fourth service example is classified instead of being discovered
        // by a timeout. Still counted with a reason, per the #1998 rule.
        {
            let file = example_path(stem);
            if let Ok(mut bundle) = jet::Loader::load_entry(&file) {
                let _ = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
                if jet::AST::bundle_serves_until_stopped(&bundle) {
                    held_back.push(format!("{stem}: service entry serves until stopped"));
                    continue;
                }
            }
        }
        // JIT lowers only `tir_covers` entry-module funcs; AOT still walks every
        // top-level item. `web/app_hello` keeps unused web.page/app helpers that
        // miss the TIR gate (ICE on `home`) while `run` stays resident-JIT safe.
        if stem == "web/app_hello" {
            held_back.push(format!("{stem}: AOT TIR gate miss on unused web helpers"));
            continue;
        }
        to_run.push(stem.clone());
    }
    // #2020: the classification above stays sequential and unchanged, so the
    // three buckets still partition the universe exactly; only the expensive
    // judging is spread across workers.
    let ran = run_three_way_battery_parallel(to_run);

    // #2012: totality. The three buckets partition the universe exactly, so the
    // battery cannot run fewer stems than it speaks for.
    assert_eq!(
        ran + no_golden.len() + held_back.len(),
        resident_safe.len(),
        "the three-way battery lost a stem: {} resident-safe, {ran} ran, {} without a golden, {} \
         held back. Every stem of the universe lands in exactly one bucket (#2012)",
        resident_safe.len(),
        no_golden.len(),
        held_back.len()
    );

    // #2012: never print a bare `N ran`. This battery speaks for the
    // resident-safe stems only, and the line that reports it says so, together
    // with everything it is NOT a claim about.
    eprintln!(
        "three-way battery universe: {} resident-safe stem(s) of {} compile-covered, out of a \
         {corpus}-stem corpus = {ran} ran + {} with no golden + {} held back at a named AOT gate. \
         This battery says NOTHING about the {} compile-covered stem(s) the resident gate \
         refuses, the {} compile gap(s), or the {} stem(s) outside the compile oracle.",
        resident_safe.len(),
        covered.len(),
        no_golden.len(),
        held_back.len(),
        resident_refused.len(),
        gaps.len(),
        out_of_universe.len()
    );
    for row in &resident_refused {
        eprintln!("  resident gate refused: {row}");
    }
    for stem in &no_golden {
        eprintln!("  no golden: {stem}");
    }
    for row in &held_back {
        eprintln!("  held back: {row}");
    }

    // Raised from the original `ran >= 9` M3 seed floor, which stopped saying
    // anything once the battery reached the hundreds: a deleted golden moves a
    // stem from `ran` to `no_golden` without moving it out of the universe, and
    // only this floor notices.
    assert!(
        ran >= THREE_WAY_RAN_FLOOR,
        "the three-way battery ran {ran} of its {} resident-safe stem(s) (floor \
         {THREE_WAY_RAN_FLOOR}); it may only grow. A fallen count means goldens or resident-safe \
         stems left the battery: restore them, or lower the floor in the same reviewed diff.",
        resident_safe.len()
    );
    report_oracle_cache_counts();
}

// ── c727: differential example-corpus gate (D-LENS-RUN1 / #688 C1) ─────────

/// Stable exclusion for examples outside the native JIT↔AOT oracle lens.
fn corpus_gate_exclusion(stem: &str) -> Option<&'static str> {
    match stem {
        "game/raylib_window" => Some("interactive display required"),
        "lowlevel/cross" => Some("cross-target demo"),
        "net/http_server" | "net/http_server_lifecycle" | "net/http_server_middleware"
        | "net/http_server_tasks" | "net/http_server_trailers" | "net/socket_echo" => {
            Some("network service")
        }
        "ui/ui_native_linux" => Some("native GTK shell"),
        _ => None,
    }
}

fn example_has_err_golden(stem: &str) -> bool {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("examples/features/expected/{stem}.err.out"))
        .is_file()
}

fn example_has_out_golden(stem: &str) -> bool {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("examples/features/expected/{stem}.out"))
        .is_file()
}

/// Unique diagnostic codes in encounter order — run_tier_broken records codes only
/// so concurrent E0956 wording churn does not thrash the manifest.
fn corpus_gate_unique_codes<'a>(codes: impl IntoIterator<Item = &'a str>) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for code in codes {
        if seen.insert(code) {
            out.push(code);
        }
    }
    out.join("; ")
}

fn corpus_gate_interpreter_refusal_detail<'a>(
    codes: impl IntoIterator<Item = &'a str>,
) -> String {
    format!(
        "interpreter_refused: {}",
        corpus_gate_unique_codes(codes)
    )
}

/// R13 (#1997): an abort is never an outcome, so it is never a ledger row
/// either.
///
/// Every other classification below records a real observable — a diagnostic, an
/// exit code, a diverging stream. An abort records none of those: the process
/// died before the report boundary could run, and the exit code it leaves behind
/// describes the corpse rather than the program. Filing one is worse than
/// missing it, because every ratcheted section here is shrink-only, so the row
/// then *protects* the abort from ever failing this suite again.
///
/// That is not hypothetical. `streams/generators` cancelled a generator at a
/// wait point inside drop glue, raised a second time, and died with `panic in a
/// destructor during cleanup`; the classifier saw only `exit_code != 0` and
/// filed the benign-looking `aot_broken` row `AOT exit 1`. So the check lives
/// where the bytes are first seen, on every tier, rather than in the manifest
/// where the abort has already been turned into a number.
fn corpus_gate_refuse_abort(stem: &str, tier: &str, out: &ProgramOutput) {
    if let Some(marker) = common::abort_marker(&out.stderr) {
        panic!(
            "`{stem}`: the {tier} run carries the abort marker `{marker}`, so a Rust \
             control transfer killed the process instead of becoming a report. An \
             abort is never an outcome and never a gate row either — fix the \
             boundary, do not file the stem (docs/spec/architecture.md R13, \
             crates/jet-jit/src/host_seam.rs).\nexit: {}\nstdout:\n{}\nstderr:\n{}",
            out.exit_code, out.stdout, out.stderr
        );
    }
}

/// Which observable stream(s) diverged, as stable tokens.
///
/// The old detail for a divergence was the single word `parity`, which named the
/// CHECK rather than the finding: a stem could sit in `run_tier_broken` reading
/// `parity` while the default run had exited 0 with correct-looking output. These
/// tokens are the finding, and they stay free of program text so a divergence
/// cannot thrash the ledger with its own payload.
fn corpus_gate_divergent_streams(jit: &ProgramOutput, aot: &ProgramOutput) -> String {
    let mut streams = Vec::new();
    if jit.stdout != aot.stdout {
        streams.push("stdout");
    }
    if jit.stderr != aot.stderr {
        streams.push("stderr");
    }
    if jit.exit_code != aot.exit_code {
        streams.push("exit");
    }
    streams.join("+")
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum CorpusGateClass {
    FrontendRejected,
    GateExcluded,
    NonRunnable,
    /// The HOST has no rustc, so no AOT oracle could be built for ANY stem. This
    /// states a fact about the machine, never about the program: membership is
    /// all-or-nothing and flips when a toolchain is installed. That is exactly
    /// why it is neither `expected_exit` nor `aot_broken` — a shrink-only
    /// ratchet over a set the host decides cannot be held honestly, and a stem
    /// whose oracle was never built is no evidence that anything is broken or
    /// repaired (#2016). Detail names the missing tool.
    OracleUnavailable,
    /// The GOLDEN expects a non-zero exit (an `.err` golden is present), so the
    /// stem has no exit-0 oracle to differentiate against. Exactly ONE fact.
    ///
    /// It used to carry four: this one, `rustc unavailable`, `AOT compile or run
    /// failed`, and `AOT exit N`. A broken oracle therefore sat in a
    /// benign-sounding section and was dropped from the differential entirely,
    /// so a cross-tier divergence in any of those stems was permanently
    /// unobservable — the same shape as the `parity` rows that hid in
    /// `run_tier_broken` (#2016).
    ExpectedExit,
    /// The AOT ORACLE ITSELF is broken: the optimized AOT build failed, or the
    /// binary it produced exited non-zero for a stem whose golden expects exit
    /// 0. No tier comparison runs for such a stem, so this is a HOLE in the
    /// differential, not an outcome the gate accepts. Shrink-only against
    /// `AOT_BROKEN_HELD_OUT`.
    ///
    /// A build failure here is also where an I2 violation surfaces: rustc
    /// rejecting GENERATED code is an internal compiler error, never a
    /// user-facing outcome. Detail is `AOT compile or run failed` or `AOT exit
    /// N`.
    AotBroken,
    ResidentJit,
    DeoptInterp,
    /// AOT-green (oracle exit 0) but the default tiered run REFUSES to run the
    /// program: sema rejects it on the run path, or the tiered backend answers
    /// `Problems`. Shrink-only burndown (D-VERDICT-1254-1 / D-LENS-RUN1). Detail
    /// is diagnostic codes only — a record with any other detail is not this
    /// class, which is how `parity` rows hid eight non-refusals here.
    RunTierBroken,
    /// AOT-green and the default tiered run ALSO ran to completion, but their
    /// normalized stdout/stderr/exit differ. That is a tier-semantics divergence,
    /// not a run-tier refusal: the program runs under `jet run` and prints the
    /// wrong thing (or the oracle does). Kept separate because `run_tier_broken`
    /// asserts "fails under default `jet run`", and a stem that ran did not fail
    /// (D-ONECORE1=A, the same law `jit_coverage_audit` states).
    /// Detail names the diverging stream(s): `stdout`, `stderr`, `exit`.
    TierDivergent,
}

/// The stems whose default `jet run` is KNOWN to refuse an AOT-green program,
/// with the codes it stops on and the card that owes the fix. Stem-sorted, and
/// the gate compares its observed `run_tier_broken` class against this list
/// EXACTLY — so a new refusal fails, and a repaired one fails until its row is
/// deleted here.
///
/// It lives in source, not in `tests/jit_corpus_gate.txt`: the ledger is
/// regenerated from an observed run, so a row there cannot hold a law that
/// regeneration is meant to be unable to silence (#2013).
///
/// Empty since #2016: the one row here, `lowlevel/layout_columnar`, cannot be
/// observed in this class at all while its AOT oracle is broken, because an
/// exit-0 oracle is the precondition for reaching the run tier. The stem is
/// held out in `AOT_BROKEN_HELD_OUT` instead, which names #1988 too, and the
/// diff that repairs the AOT build moves the row back here — one section owns a
/// stem, so the two lists never double-count it.
const RUN_TIER_BROKEN_HELD_OUT: &[(&str, &str, &str)] = &[];

/// The stems whose AOT ORACLE is KNOWN to be broken, with the observed detail
/// and the card that owes the fix. Stem-sorted, compared EXACTLY against the
/// observed `aot_broken` class for the same reason `RUN_TIER_BROKEN_HELD_OUT`
/// is: a new breakage fails, and a repaired stem fails until its row is deleted
/// here.
///
/// Every row is a stem the three-tier differential does not cover, so the list
/// is the honest size of the hole. It may only SHRINK. Raising it is not a fix
/// — repair the oracle (#2016).
const AOT_BROKEN_HELD_OUT: &[(&str, &str, &str)] = &[
    (
        "devloop/schedule_every",
        "AOT compile or run failed",
        "#2016: the optimized AOT build or run of the example fails, so the gate \
         has no oracle to compare the tiers against",
    ),
    (
        "operators/spaceship",
        "AOT compile or run failed",
        "#2016: the optimized AOT build or run of the example fails, so the gate \
         has no oracle to compare the tiers against",
    ),
    (
        "reflection/reflect-value",
        "AOT exit 70",
        "#2016: the AOT binary exits 70 for a stem whose golden expects exit 0, \
         so the oracle disagrees with the golden and cannot arbitrate the tiers",
    ),
    (
        "serde/hand_codec",
        "AOT exit 1",
        "#2016: the AOT binary exits 1 for a stem whose golden expects exit 0, \
         so the oracle disagrees with the golden and cannot arbitrate the tiers",
    ),
    (
        "types/typed_literal_forms",
        "AOT compile or run failed",
        "#2016: the optimized AOT build or run of the example fails, so the gate \
         has no oracle to compare the tiers against",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CorpusGateRecord {
    stem: String,
    class: CorpusGateClass,
    detail: String,
}

fn classify_corpus_stem(stem: &str, have_rustc: bool) -> CorpusGateRecord {
    let file = example_path(stem);
    if let Some(reason) = corpus_gate_exclusion(stem) {
        return CorpusGateRecord {
            stem: stem.to_string(),
            class: CorpusGateClass::GateExcluded,
            detail: reason.to_string(),
        };
    }

    let build_entry = fs::read_to_string(&file)
        .ok()
        .is_some_and(|source| jet::Driver::selects_build_entry(&source, None));
    let mut bundle = if build_entry {
        match jet::Driver::compile_bundle_path_build(
            &file,
            jet::Driver::BuildRunOptions::default(),
        ) {
            Ok(output) => match output.runtime {
                Some(bundle) => bundle,
                None => {
                    return CorpusGateRecord {
                        stem: stem.to_string(),
                        class: CorpusGateClass::FrontendRejected,
                        detail: "build front end returned no runtime bundle".to_string(),
                    }
                }
            },
            Err(diags) => {
                let detail = diags
                    .first()
                    .map(|d| format!("{}: {}", d.code, d.what))
                    .unwrap_or_else(|| "build failed".to_string());
                return CorpusGateRecord {
                    stem: stem.to_string(),
                    class: CorpusGateClass::FrontendRejected,
                    detail,
                };
            }
        }
    } else {
        match jet::Loader::load_entry(&file) {
            Ok(b) => b,
            Err(diags) => {
                let detail = diags
                    .first()
                    .map(|d| format!("{}: {}", d.code, d.what))
                    .unwrap_or_else(|| "load failed".to_string());
                return CorpusGateRecord {
                    stem: stem.to_string(),
                    class: CorpusGateClass::FrontendRejected,
                    detail,
                };
            }
        }
    };
    // The run tier's bundle is the one `jet run` builds, not a load+check
    // lookalike. `Interpreter::checked_bundle_with_entry` seeds the single
    // build-fact snapshot before sema, and the AOT oracle below gets it too
    // (`compile_with_path` -> `Driver::seed_build_facts_from_stamp`). Skipping it
    // here left every `@build.*` read folding to a default on the tiered side
    // only, so `comptime/build_stamp` printed the harness's defaults against the
    // oracle's locked `.jet/lock` stamp and the gate blamed the run tier for a
    // snapshot the gate itself never took (I9: engines marshal one snapshot).
    let errors: Vec<_> = if build_entry {
        Vec::new()
    } else {
        match jet::Driver::seed_build_facts(
            &mut bundle,
            "dev",
            false,
            &std::collections::BTreeMap::new(),
        ) {
            // Seeding answers with errors only, so they are already the run-path
            // rejection the block below classifies.
            Err(diags) => diags,
            Ok(()) => jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
                .into_iter()
                .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
                .collect(),
        }
    };
    if !errors.is_empty() {
        let detail = errors
            .iter()
            .map(|d| format!("{}: {}", d.code, d.what))
            .collect::<Vec<_>>()
            .join("; ");
        // Run-path frontend rejection of an AOT/golden-green example is a
        // run-tier parity hole, not a true frontend reject (D-VERDICT-1254-1).
        if have_rustc && example_has_out_golden(stem) && !example_has_err_golden(stem) {
            let aot_dir = aot_scratch_dir("corpus_gate", stem);
            if let Some(aot) =
                try_compiled_binary_output(&aot_dir, "corpus_gate_aot", 0, stem, &file)
            {
                if aot.exit_code == 0 {
                    return CorpusGateRecord {
                        stem: stem.to_string(),
                        class: CorpusGateClass::RunTierBroken,
                        detail: corpus_gate_unique_codes(errors.iter().map(|d| d.code.as_str())),
                    };
                }
            }
        }
        return CorpusGateRecord {
            stem: stem.to_string(),
            class: CorpusGateClass::FrontendRejected,
            detail,
        };
    }

    // A HOST fact, kept out of every ratcheted section: without rustc there is
    // no oracle for ANY stem, so nothing here can be read as a property of the
    // program (#2016).
    if !have_rustc {
        return CorpusGateRecord {
            stem: stem.to_string(),
            class: CorpusGateClass::OracleUnavailable,
            detail: "rustc unavailable; oracle skipped".to_string(),
        };
    }

    // #2016: a service entry serves until it is stopped, so it has no
    // terminating observable for a three-tier differential to compare, and
    // running it costs the gate its whole timeout. It leaves the RUN universe
    // and stays in the COMPILE universe: the AOT build below is the only proof
    // left that the stem is real, so a build failure is asserted, not skipped.
    //
    // The reason is DERIVED from the program, never a stem list. Before this,
    // "entry returns App" lived as four private copies with no shared
    // predicate, which is why nothing forced the gate to notice when three
    // examples became services.
    if jet::AST::bundle_serves_until_stopped(&bundle) {
        let aot_dir = aot_scratch_dir("corpus_gate_service", stem);
        assert!(
            try_compiled_binary_build(&aot_dir, "corpus_gate_service", 0, stem, &file).is_some(),
            "`{stem}` is a service entry outside the gate's run universe, so its AOT compile is \
             the only proof left: that compile failed"
        );
        return CorpusGateRecord {
            stem: stem.to_string(),
            class: CorpusGateClass::GateExcluded,
            detail: "service entry serves until stopped".to_string(),
        };
    }

    if example_has_err_golden(stem) {
        return CorpusGateRecord {
            stem: stem.to_string(),
            class: CorpusGateClass::ExpectedExit,
            detail: "golden expects non-zero exit".to_string(),
        };
    }

    let aot_dir = aot_scratch_dir("corpus_gate", stem);
    let aot = try_compiled_binary_output(&aot_dir, "corpus_gate_aot", 0, stem, &file);
    let aot = match aot {
        Some(out) => out,
        None => {
            return CorpusGateRecord {
                stem: stem.to_string(),
                class: CorpusGateClass::AotBroken,
                detail: "AOT compile or run failed".to_string(),
            };
        }
    };
    corpus_gate_refuse_abort(stem, "AOT oracle", &aot);
    if aot.exit_code != 0 {
        return CorpusGateRecord {
            stem: stem.to_string(),
            class: CorpusGateClass::AotBroken,
            detail: format!("AOT exit {}", aot.exit_code),
        };
    }

    jet_jit::reset_jit_trace_for_test();
    let jit =
        jet_jit::with_program_args(&[file.clone()], || dev_run_bundle(&bundle, false, false));
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "`{stem}` must not invoke forbidden interpreter/AOT fallback under tiered Cranelift"
    );

    match jit {
        RunOutcome::Problems(diags) => {
            assert!(
                !jet_jit::is_e2211(&diags),
                "`{stem}` must not emit retired E2211: {diags:?}"
            );
            // AOT already green above — Problems under default tiered run is a
            // shrink-only run_tier_broken entry, never an accepted deopt class.
            CorpusGateRecord {
                stem: stem.to_string(),
                class: CorpusGateClass::RunTierBroken,
                detail: corpus_gate_unique_codes(diags.iter().map(|d| d.code.as_str())),
            }
        }
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            let jit_raw = ProgramOutput::ran(stdout, stderr, exit_code);
            corpus_gate_refuse_abort(stem, "default `jet run`", &jit_raw);
            let jit_out = normalize_for_parity(stem, jit_raw);
            let aot_out = normalize_for_parity(stem, aot);
            // The program RAN under the default tier. If its normalized output
            // differs from the oracle's, that is a tier-semantics divergence, not
            // the run-tier refusal `run_tier_broken` asserts — recording it there
            // made that section's own message ("fail under default `jet run`")
            // false for every `parity` row it held.
            if jit_out != aot_out {
                return CorpusGateRecord {
                    stem: stem.to_string(),
                    class: CorpusGateClass::TierDivergent,
                    detail: corpus_gate_divergent_streams(&jit_out, &aot_out),
                };
            }
            // Criterion #5: tiered must match AOT (above). The forced
            // interpreter must match when the TIR evaluator covers the program.
            // A known E2201/E0956 boundary is recorded on the backend row; it
            // is not an interpreter comparison.
            let interpreter_detail = match jet::Interpreter::run_checked(&bundle, true) {
                RunOutcome::Ran {
                    stdout,
                    stderr,
                    exit_code,
                } => {
                    let interp_raw = ProgramOutput::ran(stdout, stderr, exit_code);
                    corpus_gate_refuse_abort(stem, "forced interpreter", &interp_raw);
                    let interp_out = normalize_for_parity(stem, interp_raw);
                    assert_eq!(
                        interp_out, aot_out,
                        "`{stem}` pure-interpreter must match AOT stdout/stderr/exit"
                    );
                    String::new()
                }
                RunOutcome::Problems(diags) => {
                    assert!(
                        diags.iter().any(|d| d.code == "E2201" || d.code == "E0956"),
                        "`{stem}` pure-interpreter failed without TIR coverage boundary: {diags:?}"
                    );
                    corpus_gate_interpreter_refusal_detail(
                        diags.iter().map(|diagnostic| diagnostic.code.as_str()),
                    )
                }
            };

            if jet_jit::deopt_invoked_for_test()
                || !jet_jit::resident_jit_safe_bundle(&bundle)
            {
                CorpusGateRecord {
                    stem: stem.to_string(),
                    class: CorpusGateClass::DeoptInterp,
                    detail: interpreter_detail,
                }
            } else {
                assert!(
                    jet_jit::jit_executed_for_test(),
                    "`{stem}` must execute resident Cranelift when AOT succeeds without deopt"
                );
                CorpusGateRecord {
                    stem: stem.to_string(),
                    class: CorpusGateClass::ResidentJit,
                    detail: interpreter_detail,
                }
            }
        }
    }
}

fn collect_corpus_gate_records() -> Vec<CorpusGateRecord> {
    common::assert_test_environment_is_safe();
    let _ = common::test_scratch_root("aot");
    let have_rustc = have_rustc();
    let stems = corpus_gate_selected_stems();
    let jobs = Arc::new(Mutex::new(std::collections::VecDeque::from(stems)));
    let records = Arc::new(Mutex::new(Vec::<CorpusGateRecord>::new()));
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut handles = Vec::new();
    for worker in 0..test_worker_count(8) {
        let jobs = Arc::clone(&jobs);
        let records = Arc::clone(&records);
        let failures = Arc::clone(&failures);
        handles.push(
            std::thread::Builder::new()
                .name(format!("corpus-gate-{worker}"))
                .spawn(move || loop {
                    let Some(stem) = lock_recovered(&jobs, "corpus gate work queue").pop_front()
                    else {
                        break;
                    };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        classify_corpus_stem(&stem, have_rustc)
                    }));
                    match result {
                        Ok(record) => lock_recovered(&records, "corpus gate record set")
                            .push(record),
                        Err(payload) => lock_recovered(&failures, "corpus gate failure report")
                            .push(format!("{stem}: {}", panic_message(payload))),
                    }
                })
                .expect("corpus gate worker"),
        );
    }
    for handle in handles {
        handle.join().expect("corpus gate worker panicked");
    }
    let failures = judged_report(&failures, "corpus gate failure report");
    assert!(
        failures.is_empty(),
        "corpus gate classification failures:\n{}",
        failures.join("\n")
    );
    // NOT `into_inner()`: poison here means a worker unwound while holding the
    // record set, so the classification is missing stems. A partial corpus
    // would read as agreement. Fail with the cause named.
    let mut records = match records.lock() {
        Ok(records) => records.clone(),
        Err(poisoned) => panic!(
            "cascade, not a fresh defect: the corpus gate record set was poisoned — a worker \
             unwound while holding it, so the classification is incomplete ({} stem(s) \
             recorded) and no ledger cross-check may be judged against it; fix the first \
             panic of the run",
            poisoned.into_inner().len()
        ),
    };
    records.sort_by(|left, right| left.stem.cmp(&right.stem));

    // #2016 + #1998 rule: an excluded stem carries a named reason AND is
    // counted, so the number can only shrink. A bare skip list is what let
    // stems leave a gate's universe unnoticed; a ceiling makes leaving cost a
    // reviewed edit. Raising this is not a fix — drive the stem instead.
    //
    // #2013: the constant moved to module scope because the LEDGER needs the
    // same ceiling as this live observation, and two copies of one ceiling is
    // how a ledger drifts away from what was observed (AGENTS.md I8).
    let excluded = records
        .iter()
        .filter(|record| record.class == CorpusGateClass::GateExcluded)
        .count();
    assert!(
        excluded <= CORPUS_GATE_EXCLUDED_CEILING,
        "the corpus gate excludes {excluded} stem(s) but the ceiling is \
         {CORPUS_GATE_EXCLUDED_CEILING}; exclusions may only SHRINK. Drive the stem instead of \
         excluding it, or lower the ceiling in the same reviewed diff"
    );
    report_oracle_cache_counts();
    records
}

/// Live run-tier parity failures from the observed corpus gate.
///
/// The retired hand ledger called these `run_gaps:` rows. Keep the name in the
/// audit output, but derive it from the one run-tier observation instead of
/// maintaining a second list.
fn corpus_gate_run_gaps(records: &[CorpusGateRecord]) -> Vec<String> {
    let mut gaps: Vec<String> = records
        .iter()
        .filter(|record| {
            matches!(
                record.class,
                CorpusGateClass::RunTierBroken | CorpusGateClass::TierDivergent
            ) || (record.class == CorpusGateClass::DeoptInterp
                && !record.detail.is_empty()
                && !record.detail.starts_with("interpreter_refused: "))
        })
        .map(|record| {
            if record.detail.is_empty() {
                record.stem.clone()
            } else {
                format!("{}: {}", record.stem, record.detail)
            }
        })
        .collect();
    gaps.sort();
    gaps
}

fn corpus_gate_manifest_from_records(records: &[CorpusGateRecord]) -> String {
    let mut out = String::from(
        "# c727: differential example-corpus gate manifest.\n\
         # Every top-level examples/features/<topic>/*.jet appears in exactly one section.\n\
         # Update only for intentional ratchet moves.\n\
         # D-VERDICT-1254-1 / D-LENS-RUN1: run_tier_broken may only shrink — AOT-green\n\
         # examples whose default jet run REFUSES to run. Record stem + diagnostic code only.\n\
         # tier_divergent is the other half: it ran under both tiers and the observables\n\
         # differ. Record stem + the diverging stream(s) only.\n\
         # aot_broken may only shrink too (#2016): the AOT oracle failed to build or exited\n\
         # non-zero, so NO tier comparison ran for the stem. expected_exit means one thing\n\
         # and one thing only — the golden itself expects a non-zero exit. oracle_unavailable\n\
         # is a HOST fact (no rustc), so it carries no ratchet and says nothing about a stem.\n\
         # A resident_jit or deopt_interp row may carry interpreter_refused: CODE.\n\
         # That row has no interpreter output comparison. A resident_jit or\n\
         # deopt_interp row without that detail does.\n\
         #\n\
         # That invariant is enforced, not merely stated (#2013). CORPUS_GATE_ROW_FLOOR\n\
         # and CORPUS_GATE_UNCLASSIFIED_CEILING in tests/dev_parts/support.rs pin how many\n\
         # stems this file classifies and how many it still says nothing about; both live\n\
         # outside this file so a hand-edit cannot green the check by deleting the row that\n\
         # fails. Rows may only GROW, unclassified stems may only SHRINK, and\n\
         # tests/dev_corpus_gate.rs::corpus_gate_manifest_accounts_for_every_example checks\n\
         # both with no run at all.\n\
         #\n\
         # Never hand-write a classification: a row states an OBSERVED tier. Regenerate with\n\
         #   JET_CORPUS_GATE_REPORT_DIR=jit-aot-parity-report JET_WRITE_CORPUS_GATE=1 \\\n\
         #     bash tools/ci/jit-aot-parity.sh\n\n",
    );
    for class in CORPUS_GATE_SECTION_ORDER {
        let section = corpus_gate_section_name(&class);
        let mut section_records: Vec<_> = records
            .iter()
            .filter(|record| record.class == class)
            .collect();
        section_records.sort_by(|left, right| left.stem.cmp(&right.stem));
        out.push_str(section);
        out.push_str(":\n");
        for record in section_records {
            if record.detail.is_empty() {
                out.push_str("  ");
                out.push_str(&record.stem);
                out.push('\n');
            } else {
                out.push_str("  ");
                out.push_str(&record.stem);
                out.push_str(": ");
                out.push_str(&record.detail);
                out.push('\n');
            }
        }
        out.push('\n');
    }
    out
}

/// The one section order. The writer and the `JET_DUMP_CORPUS_GATE` printer used
/// to carry a private copy of this list each, so a new class could reach the
/// ledger and never be printed (AGENTS.md I8).
const CORPUS_GATE_SECTION_ORDER: [CorpusGateClass; 10] = [
    CorpusGateClass::FrontendRejected,
    CorpusGateClass::GateExcluded,
    CorpusGateClass::NonRunnable,
    CorpusGateClass::OracleUnavailable,
    CorpusGateClass::ExpectedExit,
    CorpusGateClass::AotBroken,
    CorpusGateClass::ResidentJit,
    CorpusGateClass::DeoptInterp,
    CorpusGateClass::RunTierBroken,
    CorpusGateClass::TierDivergent,
];

fn corpus_gate_section_name(class: &CorpusGateClass) -> &'static str {
    match class {
        CorpusGateClass::FrontendRejected => "frontend_rejected",
        CorpusGateClass::GateExcluded => "gate_excluded",
        CorpusGateClass::NonRunnable => "non_runnable",
        CorpusGateClass::OracleUnavailable => "oracle_unavailable",
        CorpusGateClass::ExpectedExit => "expected_exit",
        CorpusGateClass::AotBroken => "aot_broken",
        CorpusGateClass::ResidentJit => "resident_jit",
        CorpusGateClass::DeoptInterp => "deopt_interp",
        CorpusGateClass::RunTierBroken => "run_tier_broken",
        CorpusGateClass::TierDivergent => "tier_divergent",
    }
}

fn parse_corpus_gate_manifest() -> Vec<CorpusGateRecord> {
    // The section names come from `corpus_gate_section_name`, not from a private
    // copy of the list: a header this parser did not know used to be silently
    // impossible to add (AGENTS.md I8).
    let mut section: Option<CorpusGateClass> = None;
    let mut records = Vec::new();
    for raw in include_str!("../jit_corpus_gate.txt").lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(name) = trimmed.strip_suffix(':') {
            // `e2211:` is a retired section that folded into deopt_interp.
            section = Some(if name == "e2211" {
                CorpusGateClass::DeoptInterp
            } else {
                CORPUS_GATE_SECTION_ORDER
                    .into_iter()
                    .find(|class| corpus_gate_section_name(class) == name)
                    .unwrap_or_else(|| panic!("unknown manifest section: {trimmed}"))
            });
            continue;
        }
        let (stem, detail) = match trimmed.split_once(": ") {
            Some((stem, detail)) => (stem.to_string(), detail.to_string()),
            None => (trimmed.to_string(), String::new()),
        };
        let class = section
            .clone()
            .unwrap_or_else(|| panic!("manifest entry outside a section: {trimmed}"));
        records.push(CorpusGateRecord {
            stem,
            class,
            detail,
        });
    }
    records.sort_by(|left, right| left.stem.cmp(&right.stem));
    records
}

/// How many stems `tests/jit_corpus_gate.txt` must classify (#2013).
///
/// A floor: rows may only GROW. The file states its own invariant — "Every
/// top-level examples/features/<topic>/*.jet appears in exactly one section" —
/// and until #2013 nothing outside the expensive gate checked it. On 2026-08-16
/// the file held 374 rows against a 496-stem corpus: 122 stems had no row in ANY
/// section, `tooling/data_plot` among them, while open cards cited its
/// `resident_jit:` rows as current evidence about which tier runs a stem.
///
/// This lives here, outside the file it guards, for the same reason
/// its row floor does: a hand-edit cannot green the ledger by deleting
/// the row that fails. A row that LEAVES is either a deleted example — lower this
/// in the same diff as the deletion — or the defect this pins.
const CORPUS_GATE_ROW_FLOOR: usize = 496;

/// How many examples the gate ledger is still allowed to say nothing about (#2013).
///
/// Shrink-only, exactly like `OUT_OF_UNIVERSE_CEILING` and for the same reason:
/// every stem counted here is a stem no section names, so no reader can tell
/// whether it runs resident, deopts, or fails. The count is DERIVED from the
/// corpus walk on every run and never an allowlist, so a new example without a
/// row raises it and fails. Burn it down with a regeneration run; raising it is
/// not a fix.
const CORPUS_GATE_UNCLASSIFIED_CEILING: usize = 0;

/// How many stems the corpus gate may exclude outright (#2016 / #1998 / #2013).
///
/// One constant for two readers: `collect_corpus_gate_records` applies it to the
/// live classification, `assert_corpus_gate_manifest_covers_corpus` applies it to
/// the checked-in ledger. Exclusions may only SHRINK.
const CORPUS_GATE_EXCLUDED_CEILING: usize = 12;

/// The gate ledger's own stated invariant, asserted without an observed run (#2013).
///
/// The gate that owns `tests/jit_corpus_gate.txt` needs a Cranelift host and an
/// AOT build per stem, and it returns green early where the host is unsupported,
/// so the completeness of that file was only ever checked behind ~500
/// classifications. This check reads the ledger and walks
/// `examples/features/<topic>/`, nothing more, so a stem that falls out of every
/// section fails on any host in milliseconds.
///
/// State the polarity plainly, because both directions are bugs:
///
/// - `CORPUS_GATE_ROW_FLOOR` may only grow — a vanished row is a stem that left
///   the ledger.
/// - `CORPUS_GATE_UNCLASSIFIED_CEILING` may only shrink — a stem with no row is
///   a claim nobody made.
///
/// Nothing here decides which section a stem belongs in. This check cannot and
/// must not classify: a classification needs an observed run, and the rows this
/// an earlier hand ledger was falsified three times by exactly the hand-edit
/// that guessing here would invite. Regenerate the rows
/// from an observation:
///
/// ```text
/// JET_CORPUS_GATE_REPORT_DIR=jit-aot-parity-report JET_WRITE_CORPUS_GATE=1 \
///   bash tools/ci/jit-aot-parity.sh
/// ```
fn assert_corpus_gate_manifest_covers_corpus() {
    let manifest = parse_corpus_gate_manifest();
    let corpus = all_example_stems();
    let audit = audit_corpus_gate_ledger(&manifest, &corpus);

    // Printed on every run, pass or fail, for the same reason `jit_coverage_audit`
    // prints its unjudged stems: the denominator is stated, never implied.
    eprintln!(
        "corpus gate ledger: {} of {} stem(s) classified; {} stem(s) in no section:",
        audit.classified,
        corpus.len(),
        audit.unclassified.len()
    );
    for stem in &audit.unclassified {
        eprintln!("  {stem}");
    }

    // Named apart, and named first: the gate's blob compare reports the first
    // diverging record, so a row naming a file that does not exist reads exactly
    // like a classification that changed. That is how the nonexistent stem
    // `tooling/data_line` held a battery down for ten days.
    assert!(
        audit.ghosts.is_empty(),
        "tests/jit_corpus_gate.txt names {} stem(s) with no \
         examples/features/<topic>/<name>.jet file: {:?}. A nonexistent stem is a stale row to \
         delete, never a classification that failed.",
        audit.ghosts.len(),
        audit.ghosts
    );
    assert!(
        audit.duplicated.is_empty(),
        "{} stem(s) appear in more than one section of tests/jit_corpus_gate.txt, breaking that \
         file's own invariant that every top-level example appears in exactly one:\n{}",
        audit.duplicated.len(),
        audit.duplicated.join("\n")
    );

    // #2016 + #1998 rule applied to the ledger, not only to the live
    // observation: a stem held out of the run tier carries a named reason AND is
    // counted. `classify_corpus_stem` never emits one of those classes with an
    // empty detail, so a bare stem in one of those sections is a hand-edit.
    assert!(
        audit.reasonless.is_empty(),
        "{} row(s) in tests/jit_corpus_gate.txt hold a stem out of the run tier without saying \
         why:\n{}\nA stem held out states its reason, or it is not held out — it is hidden.",
        audit.reasonless.len(),
        audit.reasonless.join("\n")
    );

    // The pin is an identity against the corpus, not a bare row count. With no
    // ghosts and no duplicates, classified + unclassified IS the corpus, so
    // neither pin below can be satisfied by moving a stem out of the accounting.
    assert_eq!(
        audit.classified + audit.unclassified.len(),
        corpus.len(),
        "gate ledger accounting is broken: {} classified + {} unclassified != {} discovered \
         stem(s). The pins below only mean something while this identity holds.",
        audit.classified,
        audit.unclassified.len(),
        corpus.len()
    );
    assert!(
        corpus.len() >= EXAMPLE_CORPUS_FLOOR,
        "the example corpus shrank to {} stem(s) (floor {EXAMPLE_CORPUS_FLOOR}). A stem leaving \
         `examples/features/<topic>/` shrinks every claim this ledger makes: restore it, or \
         lower the floor in the same diff as the deletion.",
        corpus.len()
    );
    assert!(
        audit.classified >= CORPUS_GATE_ROW_FLOOR,
        "tests/jit_corpus_gate.txt classifies {} stem(s) but the ratchet floor is \
         {CORPUS_GATE_ROW_FLOOR}; rows may only GROW. A stem that lost its row did not change \
         class — it left the ledger, and every card citing this file then cites a silence. \
         Regenerate from an observed run; lower the floor only in the same diff as a deleted \
         example.",
        audit.classified
    );
    assert!(
        audit.unclassified.len() <= CORPUS_GATE_UNCLASSIFIED_CEILING,
        "{} example(s) appear in NO section of tests/jit_corpus_gate.txt (ceiling \
         {CORPUS_GATE_UNCLASSIFIED_CEILING}), breaking that file's own invariant that every \
         top-level example appears in exactly one:\n  {}\nThis count may only FALL. Regenerate \
         the manifest from an observed run; a missing row is not a class change, and guessing \
         one by hand is how the sibling ledger was falsified three times.",
        audit.unclassified.len(),
        audit.unclassified.join("\n  ")
    );
    assert!(
        !audit.unclassified.is_empty() || CORPUS_GATE_UNCLASSIFIED_CEILING == 0,
        "every example now carries a row: set CORPUS_GATE_UNCLASSIFIED_CEILING to 0 in the same \
         diff, so the hole cannot reopen unnoticed"
    );
    assert!(
        audit.excluded <= CORPUS_GATE_EXCLUDED_CEILING,
        "tests/jit_corpus_gate.txt excludes {} stem(s) but the ceiling is \
         {CORPUS_GATE_EXCLUDED_CEILING}; exclusions may only SHRINK. Drive the stem instead of \
         excluding it, or lower the ceiling in the same reviewed diff",
        audit.excluded
    );
}

/// What the ledger accounts for, and what it does not (#2013).
struct CorpusGateLedgerAudit {
    /// Distinct stems the ledger names, each of which exists on disk.
    classified: usize,
    /// Rows in `gate_excluded:` — the same ceiling the live observation applies.
    excluded: usize,
    /// Rows naming a file that is not there.
    ghosts: Vec<String>,
    /// Stems named by more than one section, with the sections that claim them.
    duplicated: Vec<String>,
    /// Discovered examples no section names at all.
    unclassified: Vec<String>,
    /// Held-out rows with an empty reason.
    reasonless: Vec<String>,
}

/// The whole law as a pure function, so a negative control can prove it fires.
///
/// `assert_corpus_gate_manifest_covers_corpus` is the only caller that reads the
/// real ledger; this half takes both sides as arguments so
/// `corpus_gate_ledger_audit_fires_on_a_missing_row` can hand it a corpus with a
/// stem the manifest forgot. Without that, "the ledger is complete" would rest on
/// an assertion nothing ever watched fail — which is exactly how the sibling
/// ledger stayed green through three falsifications (#1509 c4 keeps the same
/// negative control).
fn audit_corpus_gate_ledger(
    manifest: &[CorpusGateRecord],
    corpus: &[String],
) -> CorpusGateLedgerAudit {
    let discovered: std::collections::HashSet<&str> =
        corpus.iter().map(String::as_str).collect();

    // "exactly one section" has two halves, and nothing checked this one, so a
    // regeneration that appended instead of replacing would read as agreement
    // for whichever copy sorted first.
    let mut sections: std::collections::BTreeMap<&str, Vec<&'static str>> =
        std::collections::BTreeMap::new();
    for record in manifest {
        sections
            .entry(record.stem.as_str())
            .or_default()
            .push(corpus_gate_section_name(&record.class));
    }

    let ghosts: Vec<String> = sections
        .keys()
        .filter(|stem| !discovered.contains(**stem))
        .map(|stem| (*stem).to_string())
        .collect();
    let duplicated: Vec<String> = sections
        .iter()
        .filter(|(_, listed)| listed.len() > 1)
        .map(|(stem, listed)| format!("  {stem}: {}", listed.join(", ")))
        .collect();
    let unclassified: Vec<String> = corpus
        .iter()
        .filter(|stem| !sections.contains_key(stem.as_str()))
        .cloned()
        .collect();
    let reasonless: Vec<String> = manifest
        .iter()
        .filter(|record| {
            matches!(
                record.class,
                CorpusGateClass::FrontendRejected
                    | CorpusGateClass::GateExcluded
                    | CorpusGateClass::NonRunnable
                    | CorpusGateClass::OracleUnavailable
                    | CorpusGateClass::ExpectedExit
                    | CorpusGateClass::AotBroken
                    | CorpusGateClass::RunTierBroken
                    | CorpusGateClass::TierDivergent
            ) && record.detail.is_empty()
        })
        .map(|record| {
            format!(
                "  {}: in `{}:` with no reason",
                record.stem,
                corpus_gate_section_name(&record.class)
            )
        })
        .collect();

    CorpusGateLedgerAudit {
        classified: sections.len() - ghosts.len(),
        excluded: manifest
            .iter()
            .filter(|record| record.class == CorpusGateClass::GateExcluded)
            .count(),
        ghosts,
        duplicated,
        unclassified,
        reasonless,
    }
}

fn print_corpus_gate_manifest(records: &[CorpusGateRecord]) {
    for class in CORPUS_GATE_SECTION_ORDER {
        let section = corpus_gate_section_name(&class);
        let mut section_records: Vec<_> = records
            .iter()
            .filter(|record| record.class == class)
            .collect();
        section_records.sort_by(|left, right| left.stem.cmp(&right.stem));
        if section_records.is_empty() {
            continue;
        }
        eprintln!("{section}:");
        for record in section_records {
            if record.detail.is_empty() {
                eprintln!("  {}", record.stem);
            } else {
                eprintln!("  {}: {}", record.stem, record.detail);
            }
        }
        eprintln!();
    }
}

/// c730 report bundle: case list + backend attribution + timing for CI upload.
fn write_corpus_gate_report(records: &[CorpusGateRecord], elapsed: std::time::Duration) {
    let Ok(dir) = std::env::var("JET_CORPUS_GATE_REPORT_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    fs::create_dir_all(&dir).expect("create JET_CORPUS_GATE_REPORT_DIR");
    fs::write(
        dir.join("cases.txt"),
        corpus_gate_manifest_from_records(records),
    )
    .expect("write cases.txt");
    let mut backend_trace = String::from(
        "# backend attribution (c727/c730)\n\
         # resident_jit = native Cranelift; deopt_interp = tiered deopt to tier-0\n\
         # run_tier_broken = AOT-green but the default jet run refuses (D-VERDICT-1254-1)\n\
         # tier_divergent = both tiers ran and the named stream(s) disagree\n\
         # parity = pure-interpreter + default tiered + optimized AOT identity\n\
         # only when the row has no interpreter_refused: CODE detail\n",
    );
    for record in records {
        let backend = corpus_gate_section_name(&record.class);
        if record.detail.is_empty() {
            backend_trace.push_str(&format!("{}\t{}\n", record.stem, backend));
        } else {
            backend_trace.push_str(&format!(
                "{}\t{}\t{}\n",
                record.stem, backend, record.detail
            ));
        }
    }
    fs::write(dir.join("backend_trace.txt"), backend_trace).expect("write backend_trace.txt");
    fs::write(
        dir.join("timing.txt"),
        format!(
            "shard_index={}\nshard_count={}\nelapsed_ms={}\nelapsed_s={:.3}\n",
            corpus_gate_shard_config().map_or(-1_i32, |(index, _)| index as i32),
            corpus_gate_shard_config().map_or(1, |(_, count)| count),
            elapsed.as_millis(),
            elapsed.as_secs_f64()
        ),
    )
    .expect("write timing.txt");
    fs::write(dir.join("result.txt"), "ok\n").expect("write result.txt");
    // Empty on success; failures leave cargo assert diffs in gate.log instead.
    fs::write(dir.join("output_diff.txt"), "").expect("write output_diff.txt");
}

#[derive(Debug)]
struct MultiHeadDiagnosticEntries {
    sema: Vec<jet::Diagnostics::Diagnostic>,
    aot: Vec<jet::Diagnostics::Diagnostic>,
    jit: RunOutcome,
    default_dev: RunOutcome,
    interpreter_gate: RunOutcome,
    forced_interpreter: ProgramOutput,
}

fn multi_head_diagnostic_entries(file: &str, src: &str) -> MultiHeadDiagnosticEntries {
    let file = file.to_owned();
    let src = src.to_owned();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("multi-head-diagnostic".into())
        .spawn(move || {
            let mut bundle = jet::Loader::load_entry(&file)
                .expect("missing-head fixture should load");
            let sema = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
            let aot = jet::compile_with_path(&src, &file)
                .err()
                .expect("AOT entry must reject missing multi-head coverage");
            let jit = run_jit_once(&file);
            let default_dev = dev_iteration(&file, false, false);
            // Keep the invalid fixture as the interpreter diagnostic gate. Use a
            // valid multi-head fixture for the separate backend entry below.
            let interpreter_gate = dev_iteration(&file, false, true);
            let valid_file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/ui_lint/multi_head_unreachable.jet");
            let valid_shown = valid_file.to_string_lossy().into_owned();
            let forced_interpreter = match dev_iteration(&valid_shown, false, true) {
                RunOutcome::Ran {
                    stdout,
                    stderr,
                    exit_code,
                } => ProgramOutput::ran(stdout, stderr, exit_code),
                RunOutcome::Problems(diags) => {
                    panic!("valid multi-head entry must reach forced interpreter: {diags:?}")
                }
            };
            let entries = MultiHeadDiagnosticEntries {
                sema,
                aot,
                jit,
                default_dev,
                interpreter_gate,
                forced_interpreter,
            };
            tx.send(entries).expect("multi-head diagnostic receiver");
        })
        .expect("spawn multi-head diagnostic worker");
    let entries = rx.recv_timeout(*DEV_DIFF_TIMEOUT).expect("multi-head diagnostic worker");
    worker
        .join()
        .expect("multi-head diagnostic worker panicked");
    entries
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiagnosticShape {
    severity: jet::Diagnostics::Severity,
    code: String,
    what: String,
    why: String,
    fix: String,
    span: Option<(usize, usize)>,
}

fn diagnostic_shapes(diags: &[jet::Diagnostics::Diagnostic]) -> Vec<DiagnosticShape> {
    diags
        .iter()
        .map(|diagnostic| DiagnosticShape {
            severity: diagnostic.severity,
            code: diagnostic.code.clone(),
            what: diagnostic.what.clone(),
            why: diagnostic.why.clone(),
            fix: diagnostic.fix.clone(),
            span: diagnostic.span.map(|span| (span.start, span.end)),
        })
        .collect()
}

fn cranelift_hot_swap_preserves_live_state_inner() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    fn checked_bundle(src: &str, tag: &str) -> jet::AST::ProgramBundle {
        let mut b = bundle_of(src, tag);
        let diags = jet::Sema::check_bundle(&mut b, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "fixture must type-check: {errors:?}");
        b
    }

    let v1 = checked_bundle("fn run() {\n    print(\"v1\")\n}\n", "jit_swap_v1");
    let v2 = checked_bundle("fn run() {\n    print(\"v2\")\n}\n", "jit_swap_v2");

    let mut backend = CraneliftBackend::new();
    let out1 = match backend.run(&v1, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("first run failed: {ds:?}"),
    };
    assert_eq!(out1, "v1\n");
    assert_eq!(jet_jit::resident_invocations_for_test(), 1);

    let out2 = match backend.hot_swap("run", &v2, false) {
        Ok(RunOutcome::Ran { stdout, .. }) => stdout,
        Ok(RunOutcome::Problems(ds)) => panic!("hot_swap produced diagnostics: {ds:?}"),
        Err(ds) => panic!("hot_swap failed: {ds:?}"),
    };
    assert_eq!(out2, "v2\n");
    assert_eq!(
        jet_jit::resident_invocations_for_test(),
        2,
        "hot_swap must preserve resident invocation count (live state)"
    );

    let mut backend2 = CraneliftBackend::new();
    let out3 = match backend2.hot_swap("run", &v1, false) {
        Ok(RunOutcome::Ran { stdout, .. }) => stdout,
        Ok(RunOutcome::Problems(ds)) => panic!("second backend hot_swap failed: {ds:?}"),
        Err(ds) => panic!("second backend hot_swap errored: {ds:?}"),
    };
    assert_eq!(out3, "v1\n");
    assert_eq!(jet_jit::resident_invocations_for_test(), 3);

    let out4 = match backend.restart(&v2, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("restart failed: {ds:?}"),
    };
    assert_eq!(out4, "v2\n");
    assert_eq!(
        jet_jit::resident_invocations_for_test(),
        1,
        "restart must reset resident live state"
    );
}

fn cranelift_trap_then_hot_swap_continues_inner() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    fn checked_bundle(src: &str, tag: &str) -> jet::AST::ProgramBundle {
        let mut b = bundle_of(src, tag);
        let diags = jet::Sema::check_bundle(&mut b, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "fixture must type-check: {errors:?}");
        b
    }

    let panics = checked_bundle(
        "fn run() {\n    xs :: [Int]{ 1, 2, 3 }\n    print(xs[99])\n}\n",
        "jit_trap_v1",
    );
    let recovers = checked_bundle("fn run() {\n    print(\"recovered\")\n}\n", "jit_trap_v2");

    assert!(
        jet_jit::resident_jit_safe_bundle(&panics),
        "trap fixture must be resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&panics)
    );

    let mut backend = CraneliftBackend::new();
    match backend.run(&panics, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(
                exit_code, 70,
                "a list index OOB is a runtime stop: out={stdout} err={stderr}"
            );
            // Empty stdout is the proof the trap actually fired: the only way
            // to skip the `print` is `emit_trap_check` cutting the run at the
            // indexing. A missed trap would have printed the host's `0`.
            assert!(
                stdout.is_empty(),
                "the trap must cut the run before `print`, got stdout {stdout:?}"
            );
            assert!(
                stderr.contains("Stop [E3010]"),
                "expected the registered E3010 bounds stop, got {stderr:?}"
            );
            assert!(
                !stderr.contains("E0953") && !stderr.contains("comptime"),
                "a live-program trap must not wear the comptime voice (#1483): {stderr}"
            );
        }
        RunOutcome::Problems(diags) => panic!(
            "a live-program trap is a runtime stop, not a build diagnostic (#1483): {:?}",
            diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
        ),
    }

    // Same resident process (thread-local module/runtime), next hot-reload
    // iteration: must run to completion, not carry over the trapped flag or
    // the crashed run's partial heap.
    let out = match backend.hot_swap("run", &recovers, false) {
        Ok(RunOutcome::Ran { stdout, .. }) => stdout,
        Ok(RunOutcome::Problems(ds)) => {
            panic!("hot_swap after a trap produced diagnostics: {ds:?}")
        }
        Err(ds) => panic!("hot_swap after a trap errored: {ds:?}"),
    };
    assert_eq!(out, "recovered\n");
}

// ── c77: hot-swap type-surface stability (D-HOTSWAP1 / E2210) ──────────

/// Parse `src` to a bundle via a temp file (the only loader entry point).
///
/// Runs on the canonical compiler worker. `Loader::load_entry` reaches the same
/// unbounded-depth recursive descent every compile does, so a caller that skips
/// the driver's own entry seams -- as this helper does, and as any embedder
/// calling the loader directly would -- must install the sized stack itself.
/// Without it a 2 MiB libtest worker aborts the whole binary and reports every
/// other in-flight test in this file as failed.
fn bundle_of(src: &str, tag: &str) -> jet::AST::ProgramBundle {
    let p = std::env::temp_dir().join(format!("jet_hotswap_{tag}.jet"));
    fs::write(&p, src).unwrap();
    let path = p.to_str().expect("temp path is utf-8").to_string();
    jet::run_compiler_work(move || jet::Loader::load_entry(&path)).expect("bundle should load")
}

const STRUCT_OLD: &str = "struct P {\n    x: Int\n}\nfn f(p: P) Int {\n    return p.x\n}\nfn run() {\n    print(f(P{x: 1}))\n}\n";

fn persist_binding_survives_hot_swap_and_resets_on_shape_change_inner() {
    fn load_checked(path: &std::path::Path) -> jet::AST::ProgramBundle {
        let mut b = jet::Loader::load_entry(path.to_str().unwrap()).expect("bundle should load");
        let diags = jet::Sema::check_bundle(&mut b, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "fixture must type-check: {errors:?}");
        b
    }

    // Same on-disk path across reloads — matches `jet dev` editing one file
    // (module alias identity is the file stem).
    let path = std::env::temp_dir().join(format!(
        "jet_persist_reload_{}.jet",
        std::process::id()
    ));
    let write = |src: &str| {
        fs::write(&path, src).unwrap();
    };

    jet_foundation::Persist::shared_clear();

    write("#Persist counter := 0\nfn run() {\n    counter += 1\n    print(counter)\n}\n");
    let v1 = load_checked(&path);
    write("#Persist counter := 99\nfn run() {\n    counter += 1\n    print(counter)\n}\n");
    let v2 = load_checked(&path);
    write("#Persist counter := true\nfn run() {\n    print(counter)\n}\n");
    let v3 = load_checked(&path);

    // Interpreter tier (always available): value must survive compatible reload.
    {
        use jet::JitBackend::{InterpreterBackend, JitBackend};
        jet_foundation::Persist::shared_clear();
        let mut backend = InterpreterBackend::new();
        let out1 = match backend.run(&v1, false) {
            RunOutcome::Ran { stdout, .. } => stdout,
            RunOutcome::Problems(ds) => panic!("interp run failed: {ds:?}"),
        };
        assert_eq!(out1, "1\n");

        let out2 = match backend.hot_swap("run", &v2, false) {
            Ok(RunOutcome::Ran { stdout, .. }) => stdout,
            Ok(RunOutcome::Problems(ds)) => panic!("interp hot_swap problems: {ds:?}"),
            Err(ds) => panic!("interp hot_swap failed: {ds:?}"),
        };
        assert_eq!(
            out2, "2\n",
            "compatible `#Persist` reload must keep the mutated Int value, not reinit to 99"
        );

        let out3 = match backend.hot_swap("run", &v3, false) {
            Ok(RunOutcome::Ran { stdout, .. }) => stdout,
            Ok(RunOutcome::Problems(ds)) => panic!("interp shape-change problems: {ds:?}"),
            Err(ds) => panic!("interp shape-change failed: {ds:?}"),
        };
        assert_eq!(
            out3, "true\n",
            "incompatible shape must reinitialize from the new Bool initializer"
        );

        let out4 = match backend.restart(&v2, false) {
            RunOutcome::Ran { stdout, .. } => stdout,
            RunOutcome::Problems(ds) => panic!("interp restart failed: {ds:?}"),
        };
        assert_eq!(
            out4, "100\n",
            "restart must clear persist and use the new initializer"
        );
    }

    if skip_if_cranelift_host_unsupported() {
        return;
    }

    // Cranelift tier: same contract at the shared-heap boundary.
    {
        use jet::JitBackend::JitBackend;
        jet_foundation::Persist::shared_clear();
        let mut backend = CraneliftBackend::new();
        let out1 = match backend.run(&v1, false) {
            RunOutcome::Ran { stdout, .. } => stdout,
            RunOutcome::Problems(ds) => panic!("jit run failed: {ds:?}"),
        };
        assert_eq!(out1, "1\n");

        let out2 = match backend.hot_swap("run", &v2, false) {
            Ok(RunOutcome::Ran { stdout, .. }) => stdout,
            Ok(RunOutcome::Problems(ds)) => panic!("jit hot_swap problems: {ds:?}"),
            Err(ds) => panic!("jit hot_swap failed: {ds:?}"),
        };
        assert_eq!(
            out2, "2\n",
            "JIT hot_swap must keep the mutated `#Persist` Int across compatible reload"
        );

        let out3 = match backend.hot_swap("run", &v3, false) {
            Ok(RunOutcome::Ran { stdout, .. }) => stdout,
            Ok(RunOutcome::Problems(ds)) => panic!("jit shape-change problems: {ds:?}"),
            Err(ds) => panic!("jit shape-change failed: {ds:?}"),
        };
        assert_eq!(out3, "true\n");

        let out4 = match backend.restart(&v2, false) {
            RunOutcome::Ran { stdout, .. } => stdout,
            RunOutcome::Problems(ds) => panic!("jit restart failed: {ds:?}"),
        };
        assert_eq!(out4, "100\n");
    }

    jet_foundation::Persist::shared_clear();
    let _ = fs::remove_file(&path);
}
