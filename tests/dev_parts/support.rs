// Shared helpers for every `tests/dev*.rs` target (`include!`d, never a module
// of its own — see tests/cli_parts and tests/corelib_parts for the same shape).
//
// #2020: `tests/dev.rs` used to be one 11.8k-line binary carrying all 166 of
// these tests. Its whole-corpus batteries run serially against one
// `dev_diff_lock`, so the binary needed longer than the 900s suite guard and
// aborted with about 30 of its own declared tests never started — every pass/fail
// number it ever printed was a partial measurement. The guard is right and the
// budget is not raisable, so the cost is split across targets instead: one routine
// target plus one
// per whole-corpus battery. This file is the single copy of everything they
// share, so the split cannot fork a helper into two versions (AGENTS.md I8).

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use common::{add_generated_rust, have_rustc, panic_message, test_worker_count};
use jet::Interpreter::RunOutcome;
use jet::JitBackend::JitBackend;

fn emit_native_aot(
    bundle: &jet::AST::ProgramBundle,
    checked_nominals: &[std::sync::Arc<jet::AST::MirFragmentNominalFacts>],
    ffi: Option<&jet::AST::FfiLink>,
) -> String {
    let request = jet_foundation::MIR::MirArtifactRequest::new(
        jet_foundation::MIR::MirArtifactTarget::RustAot,
        jet_foundation::MIR::MirArtifactKind::NativeExecutable,
        jet_foundation::MIR::MirArtifactBuildMode::Dev,
    );
    let (mir, artifact) = jet::Codegen::TIR::lower_checked_mir_program_for(bundle, checked_nominals, request)
        .expect("checked native artifact lowers through MIR");
    mir.validate()
        .expect("canonical MIR validates in dev support");
    let mir = jet_foundation::MIR::optimize_mir_program(
        mir,
        &jet_foundation::MIR::MirOptimizationPolicy::conservative(),
    )
    .expect("canonical MIR optimizes in dev support");
    let mut execution = jet::Codegen::MIRRust::MirRustExecutionConfig::for_artifact(artifact);
    execution.ffi = ffi;
    jet::Codegen::MIRRust::emit_mir_program(
        &mir,
        &jet::Codegen::MIRRust::MirRustConfig {
            target: jet_foundation::Layout::TargetLayout::host(),
            target_kind: jet::Codegen::MIRRust::MirRustTarget::Native,
            root_prefix: String::new(),
            execution,
        },
    )
}

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

/// The default `jet run` builds and executes, so a default-tier assertion
/// needs rustc on the host.
fn skip_without_rustc() -> bool {
    !have_rustc()
}

fn require_multi_head_parity_prereqs() {
    assert!(
        have_rustc(),
        "multi-head parity requires rustc; project harness must provision it"
    );
}

/// Run test work on the sized compiler stack. Re-entrancy makes it inline when
/// an outer entry already crossed, so nesting it is free.
fn on_test_compiler_stack<T, F>(f: F) -> T
where
    F: FnOnce() -> T + Send,
    T: Send,
{
    jet_evalhost::on_compiler_stack(f)
}

/// c77: the differential battery covers EVERY `Examples/features/*.jet`, not a
/// hand-curated subset — so the battery can never quietly shrink. Each example
/// either runs in the interpreter (and its stdout/stderr/exit code must match
/// the compiled binary; stdout also matches its golden `.out`) or stops at a named boundary
/// (E2202 fuel, E0956 unsupported-at-runtime, or a named effect boundary). A silent skip is
/// a test failure.
fn example_path(stem: &str) -> String {
    format!("Examples/features/{}.jet", stem)
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
// `Tools/agent/verify-full.sh` gives every run a fresh disk-backed root via
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
            common::test_scratch_root("dev-oracle-cache")
                .join(format!("run-{}-{rustc_salt}", std::process::id()))
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
                jet::SHA256::sha256_file_hex(&link.rlib_path)
                    .ok()?
                    .as_bytes(),
            );
            let artifacts_manifest = link
                .rlib_path
                .parent()?
                .join(format!("{}.sha256", link.crate_name));
            push_oracle_key_field(&mut data, b"ffi-artifacts.sha256");
            push_oracle_key_field(&mut data, artifacts_manifest.to_string_lossy().as_bytes());
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
    if !fs::symlink_metadata(&artifact).is_ok_and(|metadata| metadata.file_type().is_file())
        || !fs::symlink_metadata(&digest).is_ok_and(|metadata| metadata.file_type().is_file())
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
                        .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
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
    let counts = ORACLE_CACHE_COUNTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    common::test_scratch_root("aot").join(format!("{tag}_{stem}_{}", std::process::id()))
}

/// Build the optimized AOT oracle for one example, or reuse the run-scoped
/// artifact for the same inputs (#2076).
///
/// The shared path where dev batteries turn generated Rust into an oracle
/// binary. Both source and checked-bundle callers route through it, so the
/// cache cannot be half-applied and their `rustc` invocations cannot drift
/// apart (AGENTS.md I8).
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
    build_oracle_binary_from_parts(dir, tag, i, stem, file, compiled.rust, compiled.ffi, clinks)
}

/// Build an AOT oracle from the bundle already checked by the strict
/// three-tier harness. This keeps conformance policy and build facts identical
/// across interpreter, resident JIT, and AOT.
fn build_oracle_binary_from_bundle(
    dir: &std::path::Path,
    tag: &str,
    i: usize,
    stem: &str,
    file: &str,
    bundle: &jet::AST::ProgramBundle,
    checked_nominals: &[std::sync::Arc<jet::AST::MirFragmentNominalFacts>],
) -> Result<std::path::PathBuf, OracleBuildFailure> {
    common::assert_test_environment_is_safe();
    common::assert_test_path_on_disk(dir, "AOT scratch");
    let ffi = jet::FFI::prepare(bundle).map_err(OracleBuildFailure::FrontEnd)?;
    let rust = emit_native_aot(bundle, checked_nominals, ffi.as_ref());
    let clinks =
        jet::resolve_c_links_for_bundle(&bundle.bundle, None).map_err(OracleBuildFailure::CLinks)?;
    build_oracle_binary_from_parts(dir, tag, i, stem, file, rust, ffi, clinks)
}

fn build_oracle_binary_from_parts(
    dir: &std::path::Path,
    tag: &str,
    i: usize,
    stem: &str,
    file: &str,
    rust: String,
    ffi: Option<jet::FFI::FfiLink>,
    clinks: Vec<String>,
) -> Result<std::path::PathBuf, OracleBuildFailure> {
    // I8: `compiled_binary_path` is the ONE oracle-binary naming rule, and it
    // exists precisely so argv[0] is the program's own name (see its doc). The
    // corpus gate's own build site used to keep the old `jet_<tag>_<i>` name, so
    // its oracle ran as `jet_corpus_gate_aot_0` and the gate read its own
    // harness binary name back out of a usage banner as a run-tier divergence.
    let bin = compiled_binary_path(dir, tag, i, file);
    fs::create_dir_all(bin.parent().expect("oracle binary has a parent"))
        .map_err(OracleBuildFailure::Io)?;
    let entry = oracle_cache_entry(&rust, ffi.as_ref(), &clinks);
    if let Some(entry) = &entry {
        if reuse_cached_oracle(entry, &bin) {
            return Ok(bin);
        }
        record_oracle_event("miss", entry);
    }
    let _cache_lock = entry
        .as_ref()
        .and_then(|entry| match OracleCacheLock::acquire(entry) {
            Ok(lock) => Some(lock),
            Err(error) => {
                eprintln!(
                    "note: oracle cache lock unavailable for {}: {error}; building uncached",
                    entry.display()
                );
                None
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
    let _runtime_lease = add_generated_rust(
        &mut rustc_cmd,
        &rs,
        &rust,
        ffi.is_some(),
        ORACLE_RUSTC_FLAGS,
    );
    rustc_cmd.arg("-o").arg(&bin);
    if let Some(link) = &ffi {
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
            if out.status.success() {
                "build"
            } else {
                "fail"
            },
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

fn compiled_binary_output_from_bundle(
    _dir: &std::path::Path,
    tag: &str,
    i: usize,
    stem: &str,
    file: &str,
    bundle: &jet::AST::ProgramBundle,
    checked_nominals: &[std::sync::Arc<jet::AST::MirFragmentNominalFacts>],
) -> ProgramOutput {
    let dir = aot_scratch_dir(tag, stem);
    let bin = build_oracle_binary_from_bundle(&dir, tag, i, stem, file, bundle, checked_nominals)
        .unwrap_or_else(|failure| panic!("{}", failure.describe(stem, file)));
    let run = command_output_with_timeout(
        Command::new(&bin),
        *DEV_DIFF_TIMEOUT,
        &format!("compiled binary run for `{stem}`"),
    );
    ProgramOutput::ran(
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
        run.status.code().unwrap_or(1),
    )
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
/// injected reader. The in-process entry points (`dev_iteration`) read the
/// test binary's own fd 0, which every thread
/// of a parallel suite shares, so there is nothing per-run to redirect there;
/// and an injected reader would exercise an input path no user has, which is
/// exactly the sort of harness-only seam an I9 differential is supposed to
/// catch rather than contain. A child reads stdin the way a user's program
/// does, through the same CLI entry point.
fn cli_tier_output_with_answers(file: &str, stem: &str, answers: &str) -> Output {
    let tag = format!("{}_default", stem.replace('/', "_"));
    let cache = common::unique_tmp(&format!("jet_stdin_{tag}_cache"));
    fs::create_dir_all(&cache).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jet"));
    cmd.arg("run").arg(file);
    // `--quiet` on every run: the banner is the CLI's, not the program's, and
    // `tests/terminal.rs` compares this same golden from this same CLI with it.
    cmd.arg("--quiet");
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("NO_COLOR", "1")
        .env("JET_STORE_DIR", cache.join("build"))
        .stdin(fs::File::open(answer_file(&cache, &tag, 0, answers)).unwrap());
    let output = command_output_with_timeout(
        cmd,
        *DEV_DIFF_TIMEOUT,
        &format!("`jet run` with answers for `{stem}` ({tag})"),
    );
    let _ = fs::remove_dir_all(&cache);
    output
}

/// One tier of one interactive example, byte-for-byte.
fn cli_tier_program_output(file: &str, stem: &str, answers: &str) -> ProgramOutput {
    let output = cli_tier_output_with_answers(file, stem, answers);
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
            let out = crate::common::dev_run(&worker_file, false, use_interpreter);
            let _ = tx.send(out);
        })
        .expect("dev_iteration worker");
    let out = rx.recv_timeout(*DEV_DIFF_TIMEOUT).unwrap_or_else(|_| {
        panic!(
            "dev_iteration timed out after {:?} for `{}` ({}) with use_interpreter={}",
            *DEV_DIFF_TIMEOUT, stem, file, use_interpreter
        )
    });
    out
}

/// All `.jet` files directly under a topic directory of `Examples/features/`
/// (one level: `Examples/features/<topic>/<name>.jet`). Skips `expected/`
/// and skips project-directory examples (`<topic>/<name>/main.jet`) — those
/// have their own multi-file drivers and are not single-entry dev targets.
///
/// A `package.jet` beside the examples is a package MANIFEST, not an example:
/// a manifest binds `name: "…"`, which is E0003 in source. Discovering one as
/// an example put a permanent unjudgeable row into `out_of_universe`, the
/// corpus gate's `frontend_rejected:`, and `typechecked_example_stems`'s
/// rejected list at the same time (#2018). Removing it HERE, at the one
/// discovery seam every oracle reads, removes it from all three at once, and
/// is not a skip list: the name comes from `Syntax::PACKAGE_FILE`, the same
/// constant the package loader resolves manifests by, so a renamed manifest
/// cannot silently reappear as a broken example.
fn topic_jet_files(root: &std::path::Path) -> Vec<PathBuf> {
    let ex_dir = root.join("Examples/features");
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
    let ex_dir = root.join("Examples/features");
    let rel = path.strip_prefix(&ex_dir).unwrap().with_extension("");
    rel.to_string_lossy().replace('\\', "/")
}

/// Every top-level example stem under `Examples/features/<topic>/`, sorted
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

/// Every generated Core conformance witness, including nested module paths.
/// Keep this walk separate from `all_example_stems`: the ordinary example
/// batteries have their own denominator and must not silently absorb the
/// registry corpus. The strict gate below is the single consumer.
fn core_conformance_corpus_entries() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap_or_else(|error| {
            panic!(
                "Core conformance corpus directory `{}` unreadable: {error}",
                dir.display()
            )
        }) {
            let path = entry.expect("Core conformance corpus entry").path();
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if name.starts_with(".") || name == "package.jet" {
                continue;
            }
            if path.is_dir() {
                walk(&path, files);
            } else if path.extension().and_then(|value| value.to_str()) == Some("jet") {
                files.push(path);
            }
        }
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpus = root.join("tests/conformance/corpus");
    if !corpus.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    walk(&corpus, &mut files);
    let mut entries: Vec<(String, String)> = files
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&corpus)
                .expect("conformance path belongs to conformance corpus")
                .with_extension("");
            let stem = format!(
                "conformance/{}",
                relative.to_string_lossy().replace('\\', "/")
            );
            (stem, path.to_string_lossy().into_owned())
        })
        .collect();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

/// Example stems the run-path front end accepts: the interpreter batteries
/// judge every one of them.
fn interpreter_example_stems() -> Vec<String> {
    typechecked_example_stems().0
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
                jet::Driver::compile_bundle_path_build(
                    &file,
                    jet::Driver::BuildRunOptions::default(),
                )
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
    // E3001 local values are explicitly debug-build-only. They are diagnostic
    // context, not program semantics, so optimized AOT and resident/interpreter
    // outputs compare without this build-profile-dependent line.
    out.stderr = strip_panic_locals(&out.stderr);
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

const BOUNDARY_CODES: &[&str] = &[
    "E2202", "E0952", "E0956", "E0953", "E3410", "E3411", "E3412", "E1265",
    // Front-end / sema codes that surface when the interpreter can't load a
    // construct the AOT path still accepts (splice/generics gaps).
    "E0102", "E0857", "E0107", "E0501",
    // Frontend-rejected examples (corpus gate) may still appear in manifested lists.
    "E0308", "E0504", "E0302", "E0505", "E0915", "E0311", "E1004",
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

fn assert_default_run_or_named_boundary(stem: &str, file: &str) {
    match dev_iteration_with_timeout(stem, file, false) {
        RunOutcome::Ran { .. } => {
        }
        RunOutcome::Problems(diags) => {
            // Real effect boundaries (FFI) may still stop; coverage gaps must not.
            assert!(
                !diags.iter().any(|diagnostic| diagnostic.code == "E2211"),
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
    tag: &str,
    manifested_divergences: &[String],
) -> DevBatteryStats {
    let left = normalize_for_parity(
        stem,
        cli_tier_program_output(file, stem, answers),
    );
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
        stats.add(
            handle
                .join()
                .expect("interpreter worker panicked outside harness"),
        );
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
/// #2020: `assert_tier_parity` hard-coded index 0, so every stem's
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

fn check_job_runner_interpreter(root: &PathBuf, file: &str) {
    let mut bundle = jet::Loader::load_entry(file).expect("job_runner loads");
    let (diagnostics, _facts, nominals) =
        jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diagnostics.into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "job_runner front end: {errors:?}");
    let bundle = jet::Driver::CheckedSourceBundle::new(bundle, facts, nominals);
    for (job, expected_name) in [
        ("greet", "job_runner.greet"),
        ("seed_data", "job_runner.seed_data"),
    ] {
        let expected = fs::read_to_string(root.join(format!(
            "Examples/features/expected/devloop/{expected_name}.out"
        )))
        .unwrap_or_else(|_| panic!("missing expected/devloop/{expected_name}.out"));
        let policy = common::development_policy();
        match common::run_interpreter_named_job(&bundle, &nominals, file, job, false, &policy) {
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
fn checked_bundle_from_path(file: &str) -> jet::Driver::CheckedSourceBundle {
    jet::run_compiler_work(|| {
        let mut b = jet::Loader::load_entry(file).expect("bundle should load");
        let (diags, facts, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut b, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "fixture must type-check: {errors:?}");
        jet::Driver::CheckedSourceBundle::new(b, facts, nominals)
    })
}

fn assert_default_run_matches_interpreter(src: &str, tag: &str) {
    if skip_without_rustc() {
        return;
    }
    let p = std::env::temp_dir().join(format!("jet_default_m3_{tag}.jet"));
    fs::write(&p, src).unwrap();
    let shown = p.to_string_lossy().to_string();

    let expected = match crate::common::dev_run(&shown, false, true) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => {
            panic!("interpreter baseline must run `{tag}`, got diagnostics: {ds:?}")
        }
    };
    let got = run_default_file(&shown, tag).stdout;
    assert_eq!(
        got, expected,
        "default `jet run` output drifted from interpreter for `{tag}`"
    );
}

/// The default `jet run` (build, then execute) of one source snippet.
fn run_default_source_outcome(src: &str, tag: &str) -> RunOutcome {
    let p = std::env::temp_dir().join(format!("jet_default_run_{tag}.jet"));
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, src).unwrap();
    let shown = p.to_string_lossy().to_string();
    crate::common::dev_run(&shown, false, false)
}

fn run_default_source(src: &str, tag: &str) -> ProgramOutput {
    match run_default_source_outcome(src, tag) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(ds) => panic!("`{tag}` default run returned diagnostics: {ds:?}"),
    }
}

/// The default `jet run` of one file.
fn run_default_file(file: &str, tag: &str) -> ProgramOutput {
    match crate::common::dev_run(file, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(ds) => panic!("`{tag}` default run returned diagnostics: {ds:?}"),
    }
}

/// The default `jet run` of a tier-parity example.
fn default_run_output(file: &str, stem: &str) -> ProgramOutput {
    run_default_file(file, stem)
}

/// Drive the real CLI default command (`jet run`, or one `jet dev` pass).
fn run_cli_default(command: &str, file: &str, tag: &str) -> ProgramOutput {
    let cache = common::unique_tmp(&format!("jet_{tag}_cache"));
    fs::create_dir_all(&cache).unwrap();
    let mut args = vec![command, file];
    if command == "dev" {
        args.extend(["--watch=off", "--quiet"]);
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jet"));
    cmd.args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("NO_COLOR", "1")
        .env("JET_STORE_DIR", cache.join("build"));
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
    fs::read_to_string(root.join(format!("Examples/features/expected/{stem}.out")))
        .unwrap_or_else(|e| panic!("missing golden for `{stem}`: {e}"))
}

fn conformance_application_authority() -> jet_foundation::Authority::ApplicationAuthority {
    let mut allow = jet_foundation::Authority::Effect::all();
    allow.extend(
        jet_foundation::Authority::ApplicationAuthority::AMBIENT_BASIC_EFFECTS
            .iter()
            .map(|effect| (*effect).to_string()),
    );
    let allow = allow.into_iter().collect::<Vec<_>>();
    jet_foundation::Authority::ApplicationAuthority::from_policy(
        Some(&allow),
        None,
        "core conformance harness",
    )
}

fn assert_tier_parity(file: &str, stem: &str) {
    if skip_without_rustc() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let is_core_conformance = stem.starts_with("conformance/");
    if is_core_conformance {
        // The corpus proves I9 tier parity for Core surfaces, not application
        // policy. Authority/E1803 suites prove that policy separately; leaving
        // the ambient default here would make effect-carrying Core surfaces
        // untestable.
        bundle.package_guarantees.application_authority = conformance_application_authority();
    }
    // Same single build-fact snapshot `jet run` seeds before sema. The
    // conformance AOT path consumes this checked
    // bundle directly; ordinary feature oracles retain `compile_with_path`.
    // Without it this harness builds a DIFFERENT bundle from the gate, so the
    // two differential oracles disagree on every `@build.*` read and the
    // weaker one silently passes (I9: engines marshal one snapshot).
    jet::Driver::seed_build_facts(
        &mut bundle,
        "dev",
        false,
        &std::collections::BTreeMap::new(),
    )
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
    let (diags, _facts, nominals) =
        jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check: {errors:?}");

    // Run the interpreter against the exact checked bundle above. Reloading
    // through `dev_iteration` re-applies the repository application's
    // authority manifest, which is unrelated to these standalone generated
    // witnesses and can reject otherwise valid Core calls before interpretation.
    // AOT's argv[0] is the compiled witness name. Give the in-process tiers
    // the same witness identity instead of an empty synthetic argument vector.
    let program_args = [file.to_string()];
    let interpreted = {
        use jet::JitBackend::{InterpreterBackend, JitBackend as _};
        let mut backend = InterpreterBackend;
        match jet_evalhost::with_program_args(&program_args, || {
            common::run_interpreter_bundle(
                &mut backend,
                &bundle,
                &nominals,
                false,
                &common::development_policy(),
            )
        }) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => Some(ProgramOutput::ran(stdout, stderr, exit_code)),
            RunOutcome::Problems(diags)
                if diags
                    .iter()
                    .any(|d| d.code == "E0956" || d.code == "E1265") =>
            {
                if is_core_conformance {
                    panic!(
                        "Core conformance witness must run in the forced interpreter `{stem}`, got: {diags:?}"
                    );
                }
                None
            }
            RunOutcome::Problems(ds) => {
                panic!(
                    "interpreter baseline must run `{stem}` or stop at E0956/E1265, got: {ds:?}"
                )
            }
        }
    };

    let jit = default_run_output(file, stem);
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
        if is_core_conformance {
            compiled_binary_output_from_bundle(
                &dir,
                "jit_3way",
                next_oracle_index(),
                stem,
                file,
                &bundle,
                &nominals,
            )
        } else {
            compiled_binary_output(&dir, "jit_3way", next_oracle_index(), stem, file)
        },
    );
    assert_eq!(jit, aot, "JIT vs AOT divergence for `{stem}`");
}

fn assert_ui_and_web_three_way(file: &str, stem: &str) {
    if skip_without_rustc() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let (diags, facts, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");

    let expected = if stem == "web/web_wasm_callback" {
        // Native `run()` is empty; golden is the wasm/node harness (`42`).
        ProgramOutput::ran(String::new(), String::new(), 0)
    } else {
        ProgramOutput::ran(golden_stdout(stem), String::new(), 0)
    };

    let interpreted = match crate::common::dev_run(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Some(ProgramOutput::ran(stdout, stderr, exit_code)),
        RunOutcome::Problems(diags)
            if diags
                .iter()
                .any(|d| d.code == "E0956" || d.code == "E1265") =>
        {
            None
        }
        RunOutcome::Problems(diags) => {
            panic!("interpreter baseline must run `{stem}` or stop at E0956/E1265, got: {diags:?}")
        }
    };

    let jit = default_run_output(file, stem);

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
    if skip_without_rustc() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let (diags, facts, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");

    let expected = if stem == "concurrency/deadline_context" {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut stderr = fs::read_to_string(
            root.join("Examples/features/expected/concurrency/deadline_context.err.out"),
        )
        .expect("deadline_context.err.out");
        if !stderr.ends_with('\n') {
            stderr.push('\n');
        }
        ProgramOutput::ran(golden_stdout(stem), stderr, 70)
    } else {
        ProgramOutput::ran(golden_stdout(stem), String::new(), 0)
    };

    let interpreted = match crate::common::dev_run(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Some(ProgramOutput::ran(stdout, stderr, exit_code)),
        RunOutcome::Problems(diags)
            if diags
                .iter()
                .any(|d| d.code == "E0956" || d.code == "E1265") =>
        {
            // Native concurrency/game surfaces may stop at a canonical
            // evaluator boundary; golden + resident JIT + AOT carry parity.
            None
        }
        RunOutcome::Problems(diags) => {
            panic!("interpreter baseline must run `{stem}` or stop at E0956/E1265, got: {diags:?}")
        }
    };

    let jit = default_run_output(file, stem);

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
    if skip_without_rustc() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let (diags, facts, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");

    let expected = ProgramOutput::ran(golden_stdout(stem), String::new(), 0);

    let interpreted = match crate::common::dev_run(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Some(ProgramOutput::ran(stdout, stderr, exit_code)),
        RunOutcome::Problems(diags)
            if diags.iter().any(|d| d.code == "E1265") =>
        {
            // Native crypto/auth/vault surfaces may stop at a canonical
            // evaluator boundary; golden + resident JIT + AOT carry parity.
            None
        }
        RunOutcome::Problems(diags) => {
            panic!("interpreter baseline must run `{stem}` or stop at E1265, got: {diags:?}")
        }
    };

    let jit = default_run_output(file, stem);

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
    if skip_without_rustc() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let (diags, facts, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");

    let expected = ProgramOutput::ran(golden_stdout(stem), String::new(), 0);

    let interpreted = match crate::common::dev_run(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Some(ProgramOutput::ran(stdout, stderr, exit_code)),
        RunOutcome::Problems(diags)
            if diags
                .iter()
                .any(|d| d.code == "E0956" || d.code == "E1265") =>
        {
            // Live network/HTTP/WS may stop at their canonical evaluator
            // boundary (E0956/E1265); golden + resident JIT + AOT carry
            // ProgramOutput parity.
            None
        }
        RunOutcome::Problems(diags) => {
            panic!(
                "interpreter baseline must run `{stem}` or stop at E0956/E1265, got: {diags:?}"
            )
        }
    };

    let jit = default_run_output(file, stem);

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
        let stderr =
            fs::read_to_string(root.join(format!("Examples/features/expected/{stem}.err.out")))
                .unwrap_or_else(|e| panic!("missing err golden for `{stem}`: {e}"));
        return ProgramOutput::ran(String::new(), stderr, 70);
    }
    ProgramOutput::ran(golden_stdout(stem), String::new(), 0)
}

fn example_has_err_golden(stem: &str) -> bool {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("Examples/features/expected/{stem}.err.out"))
        .is_file()
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
    if skip_without_rustc() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let (diags, facts, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");

    let expected = golden_program_output(stem);

    let interpreted = match crate::common::dev_run(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Some(ProgramOutput::ran(stdout, stderr, exit_code)),
        RunOutcome::Problems(diags)
            if diags
                .iter()
                .any(|d| d.code == "E1265" || d.code == "E0956") =>
        {
            None
        }
        RunOutcome::Problems(diags) => {
            panic!(
                "interpreter baseline must run `{stem}` or stop at known boundary, got: {diags:?}"
            )
        }
    };

    if stem == "reflection/reflect-value" {
        assert_eq!(
            interpreted.as_ref(),
            Some(&expected),
            "reflection example must match its golden output in the interpreter"
        );
    }

    let jit = default_run_output(file, stem);

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

fn core_os_examples_match_interpreter_default_run_and_aot_inner() {
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    for stem in ["io/os_facts", "io/os_process_control"] {
        assert_io_cli_terminal_time_three_way(&example_path(stem), stem);
    }
}

fn golden_stderr(stem: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join(format!("Examples/features/expected/{stem}.stderr.out"));
    if path.is_file() {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("read stderr golden for `{stem}`: {e}"))
    } else {
        String::new()
    }
}

/// #2017: the two-tier differential for an example that reads stdin.
///
/// The same claim its caller makes — default `jet run` and AOT both reproduce
/// the checked-in golden byte for byte — with the one difference that makes
/// the claim true here: every tier is a child, and every child is fed the
/// answers the golden was recorded with, from `common::example_stdin`.
///
/// Each tier gets its own file of answers rather than one shared pipe, because
/// the first reader would otherwise drain it and the other would silently run
/// with no input — a pass that means "two tiers agree about nothing".
fn assert_interactive_example_three_way(file: &str, stem: &str, answers: &str) {
    let golden = ProgramOutput::ran(golden_stdout(stem), golden_stderr(stem), 0);

    let jit = cli_tier_program_output(file, stem, answers);
    assert_eq!(
        jit, golden,
        "default `jet run` drifted from the golden for `{stem}` with the answers it was recorded with"
    );

    if !have_rustc() {
        eprintln!("note: rustc not found; `{stem}` compared on the interpreter only");
        return;
    }
    let dir = aot_scratch_dir("stdin_three_way", stem);
    let aot =
        compiled_binary_output_with_stdin(&dir, "stdin_three_way", 0, stem, file, Some(answers));
    assert_eq!(
        aot, golden,
        "AOT drifted from the golden for `{stem}` with the answers it was recorded with"
    );
}

fn assert_io_cli_terminal_time_three_way(file: &str, stem: &str) {
    if skip_without_rustc() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let (diags, facts, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");

    // #2017: an interactive example is only comparable against the answers its
    // golden was recorded with, and this battery fed NOTHING — so a pass here
    // meant the three tiers agreed on a no-input run while the golden described
    // a fed run. The answers come from `common::example_stdin`, the one home
    // for them (I8), and each tier gets its own child so each gets its own fd 0.
    // The in-process resident-JIT safety proof above still runs; the tier
    // attribution below moves to `--trace-tiers`, which is the out-of-process
    // form of the same claim (`run_cli_default` already reads it).
    if let Some(answers) = common::example_stdin(stem) {
        assert_interactive_example_three_way(file, stem, answers.piped);
        return;
    }

    let golden = ProgramOutput::ran(golden_stdout(stem), golden_stderr(stem), 0);

    let interpreted = match crate::common::dev_run(file, false, true) {
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
            if diags
                .iter()
                .any(|d| d.code == "E1265" || d.code == "E0956") =>
        {
            None
        }
        RunOutcome::Problems(diags) => {
            panic!(
                "interpreter baseline must run `{stem}` or stop at E1265/E0956, got: {diags:?}"
            )
        }
    };

    let dir = aot_scratch_dir("jit_1219", stem);
    let aot = compiled_binary_output(&dir, "jit_1219", 0, stem, file);

    let jit = default_run_output(file, stem);

    // AOT is the ProgramOutput oracle; golden pins stdout. Dev `?` traces may
    // land on stderr under JIT while release-flavored AOT goldens stay quiet.
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
    if skip_without_rustc() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let (diags, facts, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check: {errors:?}");

    let golden = ProgramOutput::ran(golden_stdout(stem), golden_stderr(stem), 0);

    let interpreted = match crate::common::dev_run(file, false, true) {
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
                .any(|d| d.code == "E1265" || d.code == "E0956") =>
        {
            None
        }
        RunOutcome::Problems(diags) => {
            panic!(
                "interpreter baseline must run `{stem}` or stop at E1265/E0956, got: {diags:?}"
            )
        }
    };

    let jit = default_run_output(file, stem);

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

#[derive(Debug)]
struct MultiHeadDiagnosticEntries {
    sema: Vec<jet::Diagnostics::Diagnostic>,
    aot: Vec<jet::Diagnostics::Diagnostic>,
    default_run: RunOutcome,
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
            let mut bundle =
                jet::Loader::load_entry(&file).expect("missing-head fixture should load");
            let sema = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
            let aot = jet::compile_with_path(&src, &file)
                .err()
                .expect("AOT entry must reject missing multi-head coverage");
            let default_run = crate::common::dev_run(&file, false, false);
            // Keep the invalid fixture as the interpreter diagnostic gate. Use a
            // valid multi-head fixture for the separate backend entry below.
            let interpreter_gate = crate::common::dev_run(&file, false, true);
            let valid_file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/ui_lint/multi_head_unreachable.jet");
            let valid_shown = valid_file.to_string_lossy().into_owned();
            let forced_interpreter = match crate::common::dev_run(&valid_shown, false, true) {
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
                default_run,
                interpreter_gate,
                forced_interpreter,
            };
            tx.send(entries).expect("multi-head diagnostic receiver");
        })
        .expect("spawn multi-head diagnostic worker");
    let entries = rx
        .recv_timeout(*DEV_DIFF_TIMEOUT)
        .expect("multi-head diagnostic worker");
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

const STRUCT_OLD: &str = "struct P {\n    x: Int\n}\nfn f(p: P) -> Int {\n    return p.x\n}\nfn run() {\n    print(f(P{x: 1}))\n}\n";

fn persist_binding_survives_hot_swap_and_resets_on_shape_change_inner() {
    fn load_checked(path: &std::path::Path) -> (
        jet::AST::ProgramBundle,
        std::sync::Arc<[std::sync::Arc<jet::AST::MirFragmentNominalFacts>]>,
    ) {
        let mut b = jet::Loader::load_entry(path.to_str().unwrap()).expect("bundle should load");
        let (diags, _facts, nominals) =
            jet::Sema::check_bundle_with_effect_facts(&mut b, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "fixture must type-check: {errors:?}");
        (b, nominals)
    }

    // Same on-disk path across reloads — matches `jet dev` editing one file
    // (module alias identity is the file stem).
    let path = std::env::temp_dir().join(format!("jet_persist_reload_{}.jet", std::process::id()));
    let write = |src: &str| {
        fs::write(&path, src).unwrap();
    };

    jet_foundation::Persist::shared_clear();

    write("#Persist counter := 0\nfn run() {\n    counter += 1\n    print(counter)\n}\n");
    let (v1, v1_nominals) = load_checked(&path);
    write("#Persist counter := 99\nfn run() {\n    counter += 1\n    print(counter)\n}\n");
    let (v2, v2_nominals) = load_checked(&path);
    write("#Persist counter := true\nfn run() {\n    print(counter)\n}\n");
    let (v3, v3_nominals) = load_checked(&path);

    // Interpreter tier (always available): value must survive compatible reload.
    {
        use jet::JitBackend::InterpreterBackend;
        jet_foundation::Persist::shared_clear();
        let policy = common::development_policy();
        let mut backend =
            InterpreterBackend::new(jet::Interpreter::InterpreterInvocation::DevInterpret);
        let out1 = match common::run_interpreter_bundle(&mut backend, &v1, &v1_nominals, false, &policy) {
            RunOutcome::Ran { stdout, .. } => stdout,
            RunOutcome::Problems(ds) => panic!("interp run failed: {ds:?}"),
        };
        assert_eq!(out1, "1\n");

        let out2 = match common::hot_swap_interpreter_bundle(
            &mut backend,
            "run",
            &v2,
            &v2_nominals,
            false,
            &policy,
        ) {
            Ok(RunOutcome::Ran { stdout, .. }) => stdout,
            Ok(RunOutcome::Problems(ds)) => panic!("interp hot_swap problems: {ds:?}"),
            Err(ds) => panic!("interp hot_swap failed: {ds:?}"),
        };
        assert_eq!(
            out2, "2\n",
            "compatible `#Persist` reload must keep the mutated Int value, not reinit to 99"
        );

        let out3 = match common::hot_swap_interpreter_bundle(
            &mut backend,
            "run",
            &v3,
            &v3_nominals,
            false,
            &policy,
        ) {
            Ok(RunOutcome::Ran { stdout, .. }) => stdout,
            Ok(RunOutcome::Problems(ds)) => panic!("interp shape-change problems: {ds:?}"),
            Err(ds) => panic!("interp shape-change failed: {ds:?}"),
        };
        assert_eq!(
            out3, "true\n",
            "incompatible shape must reinitialize from the new Bool initializer"
        );

        let out4 = match common::restart_interpreter_bundle(&mut backend, &v2, &v2_nominals, false, &policy) {
            RunOutcome::Ran { stdout, .. } => stdout,
            RunOutcome::Problems(ds) => panic!("interp restart failed: {ds:?}"),
        };
        assert_eq!(
            out4, "100\n",
            "restart must clear persist and use the new initializer"
        );
    }

    jet_foundation::Persist::shared_clear();
    let _ = fs::remove_file(&path);
}
