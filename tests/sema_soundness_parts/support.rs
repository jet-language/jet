use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use common::{build_and_run, fixture_matches, normalize_fixture_selector, strip_vetted_prelude_modules};

const DEFAULT_DEV_CASE_DEADLINE: Duration = Duration::from_secs(120);

/// Fixtures allowed to emit a *gated* `unsafe { … }` / `unsafe fn` block —
/// the same stems golden.rs treats as intentionally exercising the audited
/// `#Unsafe` expert tier (I1). Everything else must generate zero bare
/// `unsafe` in user-authored lowering. Corpus files copied from
/// `examples/features/<dir>/<name>.jet` are named `ex_<dir>_<name>.jet`
/// (see `original_example_stem`); every other naming scheme (the `ui_*`
/// reuse from tests/ui, and the handwritten seeds) is never gated.
const GATED_UNSAFE_STEMS: &[&str] = &[
    "lowlevel/lowlevel",
    "lowlevel/pointer_cast_deref",
    "memory/rawptr",
    "effects/single_use_discard",
    "memory/uninit",
    "crypto/crypto_migration",
];

/// Recover the `examples/features/<dir>/<name>` stem from an `ex_`-prefixed
/// corpus filename (`ex_<dir>_<name>` — every topic dir under
/// `examples/features` is a single path segment with no underscore, so
/// splitting on the first `_` is exact). Returns `None` for any other
/// naming scheme (never gated).
fn original_example_stem(file_stem: &str) -> Option<String> {
    let rest = file_stem.strip_prefix("ex_")?;
    let (dir, name) = rest.split_once('_')?;
    Some(format!("{dir}/{name}"))
}

const CORPUS: &str = "tests/fuzz/sema";

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CORPUS)
}

fn cases(kind: &str) -> Vec<PathBuf> {
    let dir = corpus_root().join(kind);
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("jet"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "empty sema soundness corpus: {}", dir.display());
    files
}

fn relative(path: &Path) -> String {
    path.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

fn case_filter() -> Option<String> {
    std::env::var("SEMA_SOUNDNESS_CASE")
        .ok()
        .map(|s| normalize_fixture_selector("SEMA_SOUNDNESS_CASE", &s))
}

fn selected_cases(kind: &str, filter: Option<&str>) -> Vec<PathBuf> {
    cases(kind)
        .into_iter()
        .filter(|path| fixture_matches(filter, &relative(path)))
        .collect()
}

fn require_lane_selection(lane: &str, filter: Option<&str>, selected: usize) {
    match filter {
        Some(filter) => assert_eq!(
            selected, 1,
            "SEMA_SOUNDNESS_CASE must select exactly one {lane} fixture: {filter}"
        ),
        None => assert!(selected > 0, "full {lane} corpus must not be empty"),
    }
}

fn replay(path: &Path, test: &str) {
    eprintln!(
        "replay: SEMA_SOUNDNESS_CASE={} cargo test --test {} {} -- --exact --nocapture",
        relative(path),
        SUITE,
        test,
    );
}

fn require_rustc() {
    assert!(
        common::have_rustc(),
        "rustc unavailable; refusing to skip sema soundness"
    );
}

fn expected_code(path: &Path) -> &str {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.rsplit_once('.'))
        .map(|(_, code)| code)
        .unwrap_or_else(|| panic!("invalid fixture must end in .E####.jet: {}", path.display()))
}

fn default_dev_with_deadline(path: &Path) -> Result<(i32, String, String), String> {
    let shown = path.to_string_lossy().into_owned();
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", &shown])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start default dev: {error}"))?;
    let deadline = Instant::now() + DEFAULT_DEV_CASE_DEADLINE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait_with_output();
                return Err(format!(
                    "default dev exceeded {} seconds and was terminated",
                    DEFAULT_DEV_CASE_DEADLINE.as_secs()
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait_with_output();
                return Err(format!("could not supervise default dev: {error}"));
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("could not collect default dev output: {error}"))?;
    Ok((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}
/// Run `body` for every selected fixture, catching panics (the `build_and_run`
/// / front-end helpers panic on the first internal-compiler-error-shaped
/// failure) so one full pass reports every broken fixture instead of the
/// first. The default panic hook is silenced for the duration so per-case
/// panics don't spam stderr; it is always restored before returning.
fn run_all_collecting_failures(
    paths: Vec<PathBuf>,
    mut body: impl FnMut(&Path) -> Result<(), String>,
) -> Vec<String> {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut failures = Vec::new();
    for path in &paths {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(path)));
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => failures.push(msg),
            Err(payload) => failures.push(format!(
                "{}: panicked: {}",
                relative(path),
                common::panic_message(payload)
            )),
        }
    }
    std::panic::set_hook(previous_hook);
    failures
}
