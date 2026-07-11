//! #353: deterministic accepts-invalid and miscompile adversary corpus.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::{build_and_run, fixture_matches, normalize_fixture_selector, strip_vetted_prelude_modules};
use jet::Interpreter::{dev_iteration, RunOutcome};

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
    "memory/uninit_buffer",
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

fn replay(path: &Path) {
    eprintln!(
        "replay: SEMA_SOUNDNESS_CASE={} cargo test --test sema_soundness -- --nocapture",
        relative(path)
    );
}

fn require_rustc() {
    let out = Command::new("rustc")
        .arg("--version")
        .output()
        .unwrap_or_else(|e| panic!("sema soundness requires rustc; refusing to skip: {e}"));
    assert!(out.status.success(), "rustc unavailable; refusing to skip sema soundness");
}

fn expected_code(path: &Path) -> &str {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.rsplit_once('.'))
        .map(|(_, code)| code)
        .unwrap_or_else(|| panic!("invalid fixture must end in .E####.jet: {}", path.display()))
}

#[test]
fn selector_routes_exactly_one_fixture_category() {
    let filter = case_filter();
    let counts: Vec<_> = ["valid", "invalid", "differential"]
        .into_iter()
        .map(|kind| (kind, selected_cases(kind, filter.as_deref()).len()))
        .collect();
    if let Some(filter) = filter {
        assert_eq!(
            counts.iter().map(|(_, count)| count).sum::<usize>(),
            1,
            "SEMA_SOUNDNESS_CASE must select exactly one fixture: {filter}; routes: {counts:?}"
        );
        assert_eq!(
            counts.iter().filter(|(_, count)| *count == 1).count(),
            1,
            "selector must route to exactly one category: {counts:?}"
        );
    } else {
        assert!(
            counts.iter().all(|(_, count)| *count > 0),
            "full corpus must keep every category non-vacuous: {counts:?}"
        );
    }
}

#[test]
fn exact_invalid_corpus_rejects_in_jet() {
    let filter = case_filter();
    let selected = selected_cases("invalid", filter.as_deref());
    if filter.is_some() && selected.is_empty() { return; }
    assert!(!selected.is_empty(), "full invalid corpus must not be empty");
    // Collect every mismatch instead of aborting on the first one: a
    // minimized-fixture corpus this size needs one full pass per edit, not
    // one panic-and-rerun cycle per broken case.
    let mut failures = Vec::new();
    for path in selected {
        replay(&path);
        let expected = expected_code(&path);
        let fixture = fs::read_to_string(&path).unwrap();
        let src = fixture.replace("__NUL__", "\0");
        let materialized = common::unique_tmp("jet_sema_sound_invalid").with_extension("jet");
        fs::write(&materialized, &src).unwrap();
        match jet::compile_with_path(&src, &materialized.to_string_lossy()) {
            Ok(_) => failures.push(format!(
                "{}: sema accepted known-invalid fixture (expected {expected})",
                relative(&path)
            )),
            Err(diags) => {
                if !diags.iter().any(|d| d.code == expected) {
                    failures.push(format!(
                        "{}: expected {expected}, got {:?}",
                        relative(&path),
                        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} invalid-corpus mismatch(es), each replayable via SEMA_SOUNDNESS_CASE=<path>:\n{}",
        failures.len(),
        failures.join("\n")
    );
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

#[test]
fn valid_corpus_reaches_rustc() {
    let filter = case_filter();
    let selected = selected_cases("valid", filter.as_deref());
    if filter.is_some() && selected.is_empty() { return; }
    assert!(!selected.is_empty(), "full valid corpus must not be empty");
    require_rustc();
    let failures = run_all_collecting_failures(selected, |path| {
        replay(path);
        let src = fs::read_to_string(path).unwrap();
        let name = path.file_stem().unwrap().to_string_lossy();
        let (code, _, stderr) = build_and_run("jet_sema_sound_valid", &name, &src);
        if code == 0 {
            Ok(())
        } else {
            Err(format!("{}: failed (exit {code}):\n{stderr}", relative(path)))
        }
    });
    assert!(
        failures.is_empty(),
        "{} valid-corpus failure(s), each replayable via SEMA_SOUNDNESS_CASE=<path>:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn executable_corpus_matches_aot_and_default_dev() {
    let filter = case_filter();
    let selected = selected_cases("differential", filter.as_deref());
    if filter.is_some() && selected.is_empty() { return; }
    assert!(!selected.is_empty(), "full differential corpus must not be empty");
    require_rustc();
    let failures = run_all_collecting_failures(selected, |path| {
        replay(path);
        let src = fs::read_to_string(path).unwrap();
        let name = path.file_stem().unwrap().to_string_lossy();
        let expected = fs::read_to_string(path.with_extension("out"))
            .unwrap_or_else(|e| panic!("{} needs .out: {e}", relative(path)));
        let aot = build_and_run("jet_sema_sound_diff", &name, &src);
        let dev = match dev_iteration(&path.to_string_lossy(), false, false) {
            RunOutcome::Ran { stdout, stderr, exit_code } => (exit_code, stdout, stderr),
            RunOutcome::Problems(diags) => {
                return Err(format!("{}: default dev refused fixture: {diags:?}", relative(path)))
            }
        };
        if aot != dev {
            return Err(format!("{}: AOT/default-dev divergence: {aot:?} vs {dev:?}", relative(path)));
        }
        let want = (0, expected, String::new());
        if aot != want {
            return Err(format!("{}: output drift: got {aot:?}, want {want:?}", relative(path)));
        }
        Ok(())
    });
    assert!(
        failures.is_empty(),
        "{} differential-corpus failure(s), each replayable via SEMA_SOUNDNESS_CASE=<path>:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Crit #2: the required CI run is at least 250 fixed cases, zero silent
/// skips. Only meaningful for a full (unfiltered) run — a single-fixture
/// replay via `SEMA_SOUNDNESS_CASE` is exempt.
#[test]
fn full_corpus_meets_minimum_case_count() {
    if case_filter().is_some() {
        return;
    }
    let total: usize = ["valid", "invalid", "differential"]
        .iter()
        .map(|kind| cases(kind).len())
        .sum();
    assert!(
        total >= 250,
        "sema soundness corpus has {total} fixed cases across valid+invalid+differential; \
         crit #2 requires at least 250"
    );
}

/// Crit #3: safe generated user bodies contain no unaudited `unsafe`,
/// classified structurally by provenance. Every `valid`/`differential`
/// fixture's generated Rust must be free of bare `unsafe` once the vetted
/// prelude modules (FFI/mem/term/os/atomic/gtk bridges) are stripped —
/// except the handful of `examples/features/{lowlevel,memory,effects,
/// crypto}` stems that intentionally exercise the audited `#Unsafe` gate
/// (`GATED_UNSAFE_STEMS`), which may contain only *gated* `unsafe { … }` /
/// `unsafe fn` forms, never an ungated one. Mirrors golden.rs's per-example
/// I1 check, applied across the whole soundness corpus.
#[test]
fn generated_rust_has_no_unaudited_unsafe() {
    let filter = case_filter();
    let mut selected = selected_cases("valid", filter.as_deref());
    selected.extend(selected_cases("differential", filter.as_deref()));
    if filter.is_some() && selected.is_empty() {
        return;
    }
    assert!(!selected.is_empty(), "full valid+differential corpus must not be empty");
    let failures = run_all_collecting_failures(selected, |path| {
        replay(path);
        let src = fs::read_to_string(path).unwrap();
        // NB: keep this prefix free of the substring "unsafe" — codegen
        // embeds the source path in a `jet:source-map` comment, and a tmp
        // dir name containing "unsafe" would false-positive every check
        // below.
        let materialized = common::unique_tmp("jet_sema_sound_prov").with_extension("jet");
        fs::write(&materialized, &src).unwrap();
        let compiled = match jet::compile_with_path(&src, &materialized.to_string_lossy()) {
            Ok(c) => c,
            Err(diags) => {
                return Err(format!("{}: front end rejected: {diags:?}", relative(path)))
            }
        };
        let user_code = strip_vetted_prelude_modules(&compiled.rust);
        let stem = path.file_stem().unwrap().to_string_lossy();
        let gated = original_example_stem(&stem)
            .map(|orig| GATED_UNSAFE_STEMS.contains(&orig.as_str()))
            .unwrap_or(false);
        if gated {
            for (i, line) in user_code.lines().enumerate() {
                if let Some(col) = line.find("unsafe") {
                    let after = line[col..].trim_start_matches("unsafe").trim_start();
                    if !(after.starts_with('{') || after.starts_with("fn ")) {
                        return Err(format!(
                            "{}: ungated `unsafe` at line {}: {}",
                            relative(path),
                            i + 1,
                            line.trim()
                        ));
                    }
                }
            }
        } else if user_code.contains("unsafe") {
            return Err(format!(
                "{}: generated Rust contains `unsafe` outside the vetted prelude/gated #Unsafe tier",
                relative(path)
            ));
        }
        Ok(())
    });
    assert!(
        failures.is_empty(),
        "{} unsafe-provenance failure(s), each replayable via SEMA_SOUNDNESS_CASE=<path>:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
