//! Golden tests: every example program must pass the front end, and (when
//! rustc is available) build and print exactly its expected output.
//! Examples are the executable spec (invariant I5).
//!
//! Also enforces:
//!   I1 — generated code never contains `unsafe`
//!   I2 — rustc accepting the generated code; a rejection here is a
//!        front-end soundness bug, reported loudly

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

mod common;
use common::{
    add_generated_rust, example_stdin, fixture_filter, fixture_matches, have_rustc, panic_message,
    strip_vetted_prelude_modules, test_worker_count, unified_diff, unsafe_keyword_columns,
};

#[derive(Clone)]
struct GoldenEntry {
    path: PathBuf,
    stem: String,
    shown: String,
}

struct GoldenEnv {
    ex_dir: PathBuf,
    ext: &'static str,
    have_rustc: bool,
    have_cargo: bool,
    have_gtk: bool,
    update_expected: bool,
}

/// Keep golden's per-run program and FFI-bridge caches private to this test
/// process. Those caches are not content-verified end to end, and parallel test
/// binaries can delete or republish their artifacts while a release example is
/// linking them. Golden examples must prove generated code, not depend on
/// another suite's cache timing.
///
/// The runtime rlib cache is deliberately NOT in here: it is content-addressed
/// and digest-verified, so it is safe to share and expensive to rebuild.
struct GoldenScratch {
    path: PathBuf,
}

impl Drop for GoldenScratch {
    fn drop(&mut self) {
        // The run-private Jetpack hangar realizes batteries read-only.
        common::make_tree_writable(&self.path);
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// One example's generated source and executable. Removed when the example's
/// check passes, so a full run does not leave ~2.5 GB of binaries in the test
/// temp directory; a failing example keeps them for inspection.
struct ProgramFiles([PathBuf; 2]);

impl Drop for ProgramFiles {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

/// Keep the prebuilt runtime directory to this test build's keys: a runtime
/// exported before the binary was linked belongs to an earlier build, and a
/// checkout's long-lived target dir would otherwise gain ~120 MB per stale key
/// on every rebuild. A pruned key that is still current rebuilds once.
fn prune_prebuilt_runtimes_older_than(prebuilt: &Path, exe: &Path) {
    let Ok(built) = fs::metadata(exe).and_then(|meta| meta.modified()) else {
        return;
    };
    let Ok(entries) = fs::read_dir(prebuilt) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .is_ok_and(|modified| modified < built);
        if stale && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

struct FeatureFixtureScratch {
    root: PathBuf,
    entry: PathBuf,
    runtime_cwd: Option<PathBuf>,
}

impl FeatureFixtureScratch {
    fn for_entry(entry: &Path) -> Option<Self> {
        let project_root = entry.parent()?;
        if !contains_fixture_state(project_root) {
            return None;
        }

        let base = std::env::var_os("JET_TEST_SCRATCH_DIR")
            .or_else(|| std::env::var_os("JET_TEST_SCRATCH"))
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").expect("HOME is required for test scratch"))
                    .join(".cache/jet-dev/scratch")
            });
        fs::create_dir_all(&base).expect("create golden fixture scratch root");
        let base = fs::canonicalize(base).expect("canonicalize golden fixture scratch root");
        let repo = fs::canonicalize(env!("CARGO_MANIFEST_DIR")).expect("canonicalize repo root");
        assert!(
            !base.starts_with(&repo) && !base.starts_with("/tmp"),
            "golden fixture scratch must be outside the repository and disk-backed: {}",
            base.display()
        );

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let root = base.join(format!("golden-fixture-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&root).expect("create golden fixture project");
        // This fixture's authority grants and plugin paths are repository-relative.
        // Mirror that layout under scratch and build the real guest there: neither
        // stale checkout artifacts nor a canned component may satisfy the witness.
        let capability_fixture = entry.ends_with("foundations/capabilities/run.jet");
        let project = if capability_fixture {
            root.join("Examples/features/foundations/capabilities")
        } else {
            root.clone()
        };
        fs::create_dir_all(&project).expect("create staged golden project");
        copy_feature_project(project_root, &project);
        let scratch = Self {
            entry: project.join(entry.file_name()?),
            runtime_cwd: capability_fixture.then(|| root.clone()),
            root,
        };
        if capability_fixture {
            let output = Command::new(env!("CARGO_BIN_EXE_jet"))
                .args([
                    "build",
                    "capability_guest.jet",
                    "--target=sandbox",
                    "--color=never",
                ])
                .current_dir(project.join("guest"))
                .output()
                .expect("build capability fixture guest");
            assert!(
                output.status.success(),
                "capability fixture Component build failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                project.join("guest/build/capability_guest.wasm").is_file(),
                "capability fixture build did not publish its real Component"
            );
        }
        Some(scratch)
    }
}

impl Drop for FeatureFixtureScratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn contains_fixture_state(root: &Path) -> bool {
    fs::read_dir(root)
        .unwrap_or_else(|error| panic!("read example project {}: {error}", root.display()))
        .flatten()
        .any(|item| {
            let path = item.path();
            let name = item.file_name();
            if !item.file_type().is_ok_and(|kind| kind.is_dir()) {
                return false;
            }
            if name == "fixture-state" {
                return true;
            }
            name != ".jet" && contains_fixture_state(&path)
        })
}

fn copy_feature_project(source: &Path, destination: &Path) {
    for item in fs::read_dir(source)
        .unwrap_or_else(|error| panic!("read example project {}: {error}", source.display()))
        .flatten()
    {
        let name = item.file_name();
        let kind = item.file_type().expect("read example fixture entry type");
        let from = item.path();
        if kind.is_dir() && name == ".jet" {
            continue;
        }
        if kind.is_dir() && name == "fixture-state" {
            let state = destination.join(".jet");
            fs::create_dir_all(&state).expect("create staged example .jet state");
            copy_feature_state(&from, &state);
        } else if kind.is_dir() {
            let to = destination.join(&name);
            fs::create_dir_all(&to).expect("create copied example directory");
            copy_feature_project(&from, &to);
        } else if kind.is_file() {
            fs::copy(&from, destination.join(&name)).unwrap_or_else(|error| {
                panic!(
                    "copy example fixture {} into {}: {error}",
                    from.display(),
                    destination.display()
                )
            });
        }
    }
}

fn copy_feature_state(source: &Path, destination: &Path) {
    for item in fs::read_dir(source)
        .unwrap_or_else(|error| panic!("read fixture state {}: {error}", source.display()))
        .flatten()
    {
        let from = item.path();
        let to = destination.join(item.file_name());
        let kind = item.file_type().expect("read fixture state entry type");
        if kind.is_dir() {
            fs::create_dir_all(&to).expect("create staged fixture state directory");
            copy_feature_state(&from, &to);
        } else if kind.is_file() {
            fs::copy(&from, &to).unwrap_or_else(|error| {
                panic!(
                    "copy fixture state {} into {}: {error}",
                    from.display(),
                    destination.display()
                )
            });
        }
    }
}

fn gtk_loader_unavailable(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr);
    stderr.contains("symbol lookup error")
        && stderr.contains("libgtk-4.so")
        && stderr.contains("undefined symbol")
}

fn assert_compute_vulkan_webgpu_output(stdout: &str) {
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "unexpected output from tooling/compute_vulkan_webgpu: {stdout:?}"
    );
    assert!(
        matches!(lines[0], "vulkan:accepted" | "vulkan:rejected"),
        "Vulkan must report its real availability: {stdout:?}"
    );
    assert_eq!(
        lines[1], "webgpu:rejected",
        "native WebGPU must fail closed: {stdout:?}"
    );
}

fn nonnegative_host_observation(value: &str) -> i64 {
    assert!(
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()),
        "host observation must be a non-negative integer: {value:?}"
    );
    value.parse().expect("host observation must fit the receipt's Int")
}

fn labelled_host_observation(field: &str, label: &str) -> i64 {
    nonnegative_host_observation(
        field.strip_prefix(label).unwrap_or_else(|| panic!("expected {label:?}: {field:?}")),
    )
}

/// This probe measures the host, not a deterministic scheduling simulation.
/// Check the complete output shape and the two independent receipts' aggregate
/// invariants; callback lateness and control-loop jitter are not the same clock sample.
fn assert_foundations_realtime_output(stdout: &str) {
    let lines: Vec<_> = stdout.split('\n').collect();
    assert_eq!(lines.len(), 4, "unexpected realtime output: {stdout:?}");
    assert_eq!(lines[2], "receipt cancelled:true joined:true");
    assert_eq!(lines[3], "");
    let audio: Vec<_> = lines[0]
        .strip_prefix("audio_callback=cancelled rate:48000 frames:256 ")
        .expect("callback request and cancellation must match the source")
        .split(' ')
        .collect();
    assert_eq!(audio.len(), 4);
    let callbacks = labelled_host_observation(audio[0], "callbacks:");
    let frames = labelled_host_observation(audio[1], "completed_frames:");
    let missed = labelled_host_observation(audio[2], "missed:");
    let lateness = labelled_host_observation(audio[3], "max_lateness_ns:");
    assert!(callbacks > 0, "the callback must have completed real work");
    assert_eq!(i128::from(frames), i128::from(callbacks) * 256);
    // This callback has no contended adapter or blocking operation that can
    // skip it: each miss is a completed callback that began after its deadline.
    assert!(missed <= callbacks);
    assert_eq!(missed == 0, lateness == 0);

    let control: Vec<_> = lines[1]
        .strip_prefix("control_loop=1kHz ticks:10000 ")
        .expect("control-loop rate and tick count must match the source")
        .split(' ')
        .collect();
    assert_eq!(control.len(), 2);
    let missed = labelled_host_observation(control[0], "missed:");
    let jitter: Vec<_> = control[1]
        .strip_prefix("jitter_us=")
        .expect("control-loop jitter receipt")
        .split(',')
        .collect();
    assert_eq!(jitter.len(), 3);
    let min = labelled_host_observation(jitter[0], "min:");
    let max = labelled_host_observation(jitter[1], "max:");
    let sum = labelled_host_observation(jitter[2], "sum:");
    assert!(missed <= 10_000 && min <= max);
    assert!(i128::from(sum) >= i128::from(min) * 10_000);
    assert!(i128::from(sum) <= i128::from(max) * i128::from(missed));
    assert!(sum >= missed);
    assert_eq!(missed == 0, max == 0);
    assert_eq!(missed == 10_000, min > 0);
}

fn assert_foundations_text_output(stdout: &str, expected: &str) {
    let actual: Vec<_> = stdout.split('\n').collect();
    let expected: Vec<_> = expected.split('\n').collect();
    assert_eq!(actual.len(), 9, "unexpected text probe output: {stdout:?}");
    assert_eq!(actual.len(), expected.len());
    for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
        if index == 0 || index == 4 {
            let (actual_values, elapsed) = actual
                .rsplit_once(" elapsed_ms=")
                .expect("measured text operation must report elapsed milliseconds");
            let (expected_values, sample_elapsed) = expected
                .rsplit_once(" elapsed_ms=")
                .expect("golden must retain the real elapsed-milliseconds output shape");
            assert_eq!(actual_values, expected_values);
            nonnegative_host_observation(elapsed);
            nonnegative_host_observation(sample_elapsed);
        } else {
            assert_eq!(actual, expected, "non-timing text output changed at line {}", index + 1);
        }
    }
}

#[test]
fn host_timing_oracles_reject_receipt_and_non_timing_drift() {
    let realtime = "audio_callback=cancelled rate:48000 frames:256 callbacks:2 completed_frames:512 missed:1 max_lateness_ns:10\ncontrol_loop=1kHz ticks:10000 missed:9999 jitter_us=min:0,max:2,sum:9999\nreceipt cancelled:true joined:true\n";
    assert_foundations_realtime_output(realtime);
    for (before, after) in [
        ("frames:256", "frames:128"),
        ("completed_frames:512", "completed_frames:511"),
        ("missed:1 ", "missed:3 "),
        ("max_lateness_ns:10", "max_lateness_ns:0"),
        ("ticks:10000", "ticks:9999"),
        ("min:0", "min:1"),
        ("sum:9999", "sum:9998"),
        ("max:2", "max:-2"),
        ("joined:true", "joined:false"),
    ] {
        let invalid = realtime.replacen(before, after, 1);
        assert!(std::panic::catch_unwind(|| assert_foundations_realtime_output(&invalid)).is_err());
    }
    let text = "bpe bytes=1048576 elapsed_ms=12\nsegments fixed\nnormalized=fixed\ncasefold_equal=true\nscan bytes=100000000 elapsed_ms=34\nshape_arabic=fixed\nshape_devanagari=fixed\ntext_probe_complete\n";
    let timing_changed = text.replace("elapsed_ms=12", "elapsed_ms=99");
    assert_foundations_text_output(&timing_changed, text);
    for invalid in [
        text.replace("elapsed_ms=12", "elapsed_ms=-1"),
        text.replace("bytes=1048576", "bytes=1048575"),
        text.replace("shape_arabic=fixed", "shape_arabic=changed"),
    ] {
        assert!(std::panic::catch_unwind(|| assert_foundations_text_output(&invalid, text)).is_err());
    }
}

fn assert_front_end(entry: &GoldenEntry, src: &str) {
    let diagnostics = jet::check_with_path(entry.path.to_str().expect("example path is utf8"));
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error)),
        "example {} failed the front end:\n{}",
        entry.stem,
        jet::render_diagnostics(&entry.shown, src, &diagnostics)
    );
}

/// Why this example's native binary is built but not run, from facts the
/// program declares — never a stem list. A service serves until stopped
/// (`jet::AST::bundle_serves_until_stopped`, the same fact
/// `tests/dev_parts/support.rs` reads). A browser-only program (`#Target(Web)`
/// on its entry file) has its run proof in `tests/web_build.rs`.
fn example_native_run_skip(path: &Path) -> Option<&'static str> {
    let mut bundle = jet::Loader::load_entry(path.to_str()?).ok()?;
    let _ = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    if jet::AST::bundle_serves_until_stopped(&bundle) {
        return Some("service entry serves until stopped");
    }
    let browser_only = bundle.modules.get(bundle.entry).is_some_and(|module| {
        module.default_target.as_deref() == Some(jet::Syntax::BUILD_TARGET_WEB)
    });
    browser_only.then_some("declared #Target(Web): browser-only, run by tests/web_build.rs")
}

#[test]
fn statement_attributes_codegen_shape() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("Examples/features/tooling/statement_attributes.jet");
    let src = fs::read_to_string(&path).unwrap();
    let out = jet::compile_with_path(&src, "Examples/features/tooling/statement_attributes.jet")
        .unwrap_or_else(|diags| {
            panic!(
                "statement attributes example failed front end:\n{}",
                jet::render_diagnostics(
                    "Examples/features/tooling/statement_attributes.jet",
                    &src,
                    &diags
                )
            )
        });
    let core_end = out
        .rust
        .rfind("// jet:cached-core-end")
        .expect("generated output has cached-core boundary");
    let entry_end = out
        .rust
        .find("// jet-mir-entry")
        .expect("generated output has MIR entry boundary");
    let generated_entry = &out.rust[core_end..entry_end];
    assert!(
        !generated_entry.contains("\"off\"") && !generated_entry.contains("\"off block\""),
        "`#Off` body must not appear in generated entry:\n{}",
        generated_entry
    );
    assert!(
        generated_entry.contains("#[cfg(not(jet_release))]") && generated_entry.contains("debug"),
        "`#DebugOnly` body must be cfg-gated for release:\n{}",
        generated_entry
    );
}

fn feature_stem(path: &Path, ex_dir: &Path) -> String {
    path.strip_prefix(ex_dir)
        .expect("feature path must be below Examples/features")
        .to_string_lossy()
        .replace('\\', "/")
}

fn push_feature_source(entries: &mut Vec<GoldenEntry>, path: PathBuf, ex_dir: &Path, ext: &str) {
    let stem = feature_stem(&path, ex_dir)
        .strip_suffix(&format!(".{ext}"))
        .unwrap_or_else(|| panic!("feature source has unexpected extension: {}", path.display()))
        .to_owned();
    entries.push(GoldenEntry {
        shown: format!("Examples/features/{stem}.{ext}"),
        path,
        stem,
    });
}

fn push_feature_project(entries: &mut Vec<GoldenEntry>, run: PathBuf, ex_dir: &Path, ext: &str) {
    let stem = feature_stem(
        run.parent().expect("run entry must have a project directory"),
        ex_dir,
    );
    entries.push(GoldenEntry {
        shown: format!("Examples/features/{stem}/run.{ext}"),
        path: run,
        stem,
    });
}

fn collect_feature_project_entries(
    dir: &Path,
    ex_dir: &Path,
    ext: &str,
    entries: &mut Vec<GoldenEntry>,
) {
    let Some(name) = dir.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    if name.starts_with('.') || name == "expected" {
        return;
    }

    let run = dir.join(format!("run.{ext}"));
    if run.is_file() {
        push_feature_project(entries, run, ex_dir, ext);
        return;
    }

    // A package/workspace without a canonical run entry owns its descendants;
    // those files are modules, build scripts, or member-package fixtures.
    if dir.join("package.jet").is_file() || dir.join("workspace.jet").is_file() {
        return;
    }

    let Ok(children) = fs::read_dir(dir) else {
        return;
    };
    for child in children.flatten() {
        let path = child.path();
        if path.is_dir() {
            collect_feature_project_entries(&path, ex_dir, ext, entries);
        }
    }
}

fn collect_feature_golden_entries(ex_dir: &Path, ext: &str) -> Vec<GoldenEntry> {
    let mut entries = Vec::new();
    let Ok(topics) = fs::read_dir(ex_dir) else {
        return entries;
    };
    for topic in topics.flatten() {
        let topic_path = topic.path();
        let Some(topic_name) = topic_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !topic_path.is_dir() || topic_name.starts_with('.') || topic_name == "expected" {
            continue;
        }
        let Ok(children) = fs::read_dir(&topic_path) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            if path.is_file()
                && path.extension().and_then(|extension| extension.to_str()) == Some(ext)
                && path.file_name().and_then(|name| name.to_str()) != Some("package.jet")
                && path.file_name().and_then(|name| name.to_str()) != Some("workspace.jet")
                && !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with('.'))
            {
                push_feature_source(&mut entries, path, ex_dir, ext);
            } else if path.is_dir() {
                collect_feature_project_entries(&path, ex_dir, ext, &mut entries);
            }
        }
    }
    entries.sort_by(|a, b| a.stem.cmp(&b.stem));
    entries
}

#[test]
fn feature_entry_discovery_stops_at_project_roots() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let entries = collect_feature_golden_entries(&root.join("Examples/features"), "jet");

    assert!(
        entries.iter().any(|entry| entry.stem == "basics/onboarding"),
        "nested run.jet project was not discovered"
    );
    for module in [
        "foundations/storage/cache",
        "foundations/tooling/load",
        "packages/sandbox_mathkit/sandbox_src",
    ] {
        assert!(
            !entries.iter().any(|entry| entry.stem == module),
            "module/member fixture was incorrectly treated as an executable: {module}"
        );
    }
}

#[test]
fn examples_compile_and_run() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ex_dir = root.join("Examples/features");
    let ext = jet::Syntax::FILE_EXT;
    let have_rustc = have_rustc();
    let have_cargo = Command::new("cargo").arg("--version").output().is_ok();
    // D-UIDEVSHELL1=A (c134 Phase 8): the native GTK4 example links `-lgtk-4`
    // via `pkg-config gtk4`. Only build+run it where gtk4 dev headers exist
    // (the nix dev shell).
    let have_gtk = Command::new("pkg-config")
        .args(["--exists", "gtk4"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let scratch = std::env::temp_dir().join(format!(
        "jet-golden-scratch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_nanos()
    ));
    let ffi_cache = scratch.join("ffi");
    // #2074/#2516: the store is content-addressed and re-verified against the
    // artifact digest on every hit, so sharing it across runs cannot
    // resurrect a stale artifact — a wrong key is a miss, never a reuse.
    // Living under the target dir keeps `cargo clean` as the reset.
    let store = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("jet-golden-store");
    std::env::set_var("JET_STORE_DIR", &store);
    // D-BUILD-PREBUILT1: the runtime/Core closure is compiled once per test
    // build, not once per example process. Every example links the
    // `jet-runtime/<key>` rlib beside this binary in place, the layout a
    // shipped `jet` reads; the first process to need a key builds and exports
    // it while the others wait (`common::add_generated_rust`). Without this,
    // each example process verified and copied a ~120 MB rlib out of its
    // store, and every golden shard compiled every key it met.
    let exe = std::env::current_exe().expect("golden test binary path");
    let prebuilt = exe
        .parent()
        .expect("golden test binary has a directory")
        .join(jet_store::runtime::PREBUILT_DIR_NAME);
    prune_prebuilt_runtimes_older_than(&prebuilt, &exe);
    std::env::set_var(jet_store::runtime::PREBUILD_EXPORT_ENV, &prebuilt);
    std::env::set_var("JET_FFI_CACHE_DIR", &ffi_cache);
    let jetpack_root = scratch.join("jetpack-root");
    let _scratch = GoldenScratch { path: scratch };
    realize_first_party_batteries(&jetpack_root);
    std::env::set_var("JETPACK_ROOT", &jetpack_root);
    if !have_rustc {
        eprintln!("note: rustc not found; checking codegen only, skipping build+run");
    }

    // A feature source is executable when it is a standalone `.jet` file
    // directly under a topic, or the `run.jet` entry of a project directory.
    // Once a package/workspace root is found, its descendants are modules,
    // fixtures, or member packages; do not run them as independent programs.
    // This keeps the corpus census complete without turning implementation
    // files into false golden entries.
    let mut entries = collect_feature_golden_entries(&ex_dir, ext);
    let rust_defects = rust_known_failures(&root, &entries);
    let filter = fixture_filter("JET_GOLDEN_FILTER");
    entries.retain(|entry| fixture_matches(filter.as_deref(), &entry.shown));
    assert!(
        !entries.is_empty(),
        "JET_GOLDEN_FILTER matched no examples: {}",
        filter.as_deref().unwrap_or("<unfiltered>")
    );
    // This harness always compiles with the Rust reference, so a listed stem is
    // reported as SKIP-RUST-DEFECT (never counted as checked); the Jet-built
    // compiler's runners do not read the list and still hold it to the golden.
    let mut skipped_rust_defects = Vec::new();
    entries.retain(|entry| match rust_defects.get(&entry.stem) {
        Some(defect) => {
            skipped_rust_defects.push(format!("{} ({defect})", entry.stem));
            false
        }
        None => true,
    });
    let update_expected = match std::env::var("JET_UPDATE_GOLDEN") {
        Err(_) => false,
        Ok(value) if value == "1" => {
            assert!(
                filter.is_some(),
                "JET_UPDATE_GOLDEN=1 requires JET_GOLDEN_FILTER so updates stay scoped"
            );
            true
        }
        Ok(value) => panic!("JET_UPDATE_GOLDEN must be exactly 1, got {value:?}"),
    };
    let env = Arc::new(GoldenEnv {
        ex_dir,
        ext,
        have_rustc,
        have_cargo,
        have_gtk,
        update_expected,
    });
    let jobs = Arc::new(Mutex::new(std::collections::VecDeque::from(entries)));
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let checked = Arc::new(Mutex::new(0usize));
    let mut handles = Vec::new();
    for _ in 0..test_worker_count(16) {
        let env = Arc::clone(&env);
        let jobs = Arc::clone(&jobs);
        let failures = Arc::clone(&failures);
        let checked = Arc::clone(&checked);
        // Match `Tools/agent/jet-env` / nix RUST_MIN_STACK (~60MiB). Default
        // worker stacks (~2–8MiB) overflow on large `if subject OP { … }` tables
        // (D-IFDIST1 value/statement dispatch) during sema.
        handles.push(
            std::thread::Builder::new()
                .stack_size(64 * 1024 * 1024)
                .spawn(move || loop {
                    let Some(entry) = jobs.lock().unwrap().pop_front() else {
                        break;
                    };
                    let stem = entry.stem.clone();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        check_golden_entry(&entry, &env)
                    }));
                    match result {
                        Ok(()) => *checked.lock().unwrap() += 1,
                        Err(payload) => failures
                            .lock()
                            .unwrap()
                            .push(format!("{stem}: {}", panic_message(payload))),
                    }
                })
                .expect("golden worker thread"),
        );
    }
    for handle in handles {
        handle
            .join()
            .expect("golden worker panicked outside harness");
    }
    // Written straight to the process stderr, past libtest's output capture,
    // so a passing run still shows every polyglot example it could not run.
    let unavailable = POLYGLOT_UNAVAILABLE.lock().unwrap();
    if !unavailable.is_empty() {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr(),
            "golden polyglot UNAVAILABLE: {} — {} (the foreign toolchains come from the Nix integration shell: run under `Tools/agent/jet-env full`)",
            unavailable.len(),
            unavailable.join(", ")
        );
    }
    drop(unavailable);
    if !skipped_rust_defects.is_empty() {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr(),
            "golden SKIP-RUST-DEFECT: {} — {}",
            skipped_rust_defects.len(),
            skipped_rust_defects.join(", ")
        );
    }
    let failures = failures.lock().unwrap();
    assert!(
        failures.is_empty(),
        "golden example failures:\n{}",
        failures.join("\n\n")
    );
    let checked = *checked.lock().unwrap();
    assert!(
        filter.is_some() || checked >= 2,
        "expected at least 2 examples, found {}",
        checked
    );
}

/// `tests/golden-rust-known-failures.tsv`: the transition list of goldens the
/// frozen Rust reference compiler gets wrong (stem, then the defect). Every
/// listed stem must name an existing golden.
fn rust_known_failures(root: &Path, entries: &[GoldenEntry]) -> BTreeMap<String, String> {
    let path = root.join("tests/golden-rust-known-failures.tsv");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let mut defects = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (stem, defect) = line.split_once('\t').unwrap_or_else(|| {
            panic!("{}:{}: expected `stem<TAB>defect`", path.display(), index + 1)
        });
        assert!(
            entries.iter().any(|entry| entry.stem == stem),
            "{}:{}: `{stem}` is not a golden under Examples/features",
            path.display(),
            index + 1
        );
        assert!(
            defects.insert(stem.to_string(), defect.to_string()).is_none(),
            "{}:{}: `{stem}` is listed twice",
            path.display(),
            index + 1
        );
    }
    defects
}

/// Battery examples declare a first-party battery (`deps: { gui: "0.1.0" }`)
/// and consume it with `use gui`. The loader never realizes on demand (E0983),
/// so the harness stages every battery the way `jetpack env --prep` does —
/// through the built-in `batteries` source into a run-private Jetpack root —
/// exactly as `tests/lib_use.rs` stages its library before `use` resolves it.
fn realize_first_party_batteries(jetpack_root: &Path) {
    let roots = jetpack::Store::Roots::at(jetpack_root.to_path_buf());
    let store_dir = roots.hangar_dir();
    fs::create_dir_all(&store_dir).expect("create the golden Jetpack hangar");
    let table = jetpack::Batteries::source_table();
    let ctx = jetpack::Provider::Ctx {
        fixtures: None,
        store_dir: &store_dir,
        offline: true,
        project_dir: None,
        nix_index: None,
        nix_roots: None,
        allow_local_nix_catalog: false,
    };
    for slug in jetpack::Batteries::package_slugs() {
        let reference = format!("{slug}@{}", jetpack::Batteries::SOURCE_NAME);
        let spec = jetpack::RefSpec::classify_in(&reference, &table).unwrap_or_else(|error| {
            panic!("classify first-party battery `{reference}`: {error:?}")
        });
        jetpack::Store::realize_verified(
            &roots,
            &ctx,
            jetpack::Store::RealizeRequest::Package {
                spec: &spec,
                table: &table,
            },
        )
        .unwrap_or_else(|error| {
            // The run-private root is deleted with the scratch directory, so
            // carry any unreproducible-action report in the failure itself.
            let reports = fs::read_dir(jetpack_root.join("private/unreproducible"))
                .map(|dir| {
                    dir.flatten()
                        .filter_map(|item| fs::read_to_string(item.path()).ok())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            panic!("realize first-party battery `{slug}`: {error:?}\n{reports}")
        });
    }
}

/// I1's one exception: examples that teach the audited `#Unsafe` tier.
/// Every approved source region must retain its own audit evidence; unrelated
/// prelude unsafe or another region's sentry cannot satisfy the positive check.
///
/// DERIVED, never hand-typed: the repository already keeps exactly one registry
/// of approved user-written unsafe regions — the `unsafe-ratchet` baseline in
/// `Docs/spec/safety.md`, scanned and enforced by
/// `Tools/agent/check-unsafe-ratchet.mjs` (`tests/unsafe_ratchet.rs`,
/// `Tools/agent/verify-full.sh`). A new `#Unsafe` gate cannot land without
/// refreshing that baseline in the same change, so reading it here makes the two
/// facts one fact.
///
/// A hand copy was a second registry of the same fact, and it rotted twice: the
/// dev-sentry/gate-ladder rows were red from 2026-08-13 with nothing watching
/// `golden`, and `a5bea5f25` added an approved gate at
/// `Examples/features/crypto/random_api_split.jet:28` ("compare the typed and
/// raw HKDF rungs"), refreshed the baseline as the ratchet demands, and left the
/// list behind — so `golden` reported an APPROVED region as an I1 violation.
///
/// Deriving cannot widen I1: a region only enters the baseline through the
/// ratchet, and a stem with no approved region that emits `unsafe` still fails
/// the negative assertion below. Expert crypto gates audit misuse and secret
/// exposure even when the loaded Core Jet implementation needs no Rust unsafe.
///
/// Both directions are still asserted below, and the gated form is scanned with
/// `common::unsafe_keyword_columns` — three audited stems carry the word in
/// their own file name, which a plain substring scan could not tell from code.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AuditRegion {
    line: usize,
    column: usize,
    reason: String,
}

static GATED_UNSAFE_REGIONS: std::sync::LazyLock<
    std::collections::HashMap<String, Vec<AuditRegion>>,
> = std::sync::LazyLock::new(|| {
        let baseline_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Docs/spec/safety.md");
        let baseline = fs::read_to_string(&baseline_path).unwrap_or_else(|err| {
            panic!(
                "cannot read the approved unsafe-region baseline {}: {err}",
                baseline_path.display()
            )
        });
        let data = baseline
            .split_once("<!-- unsafe-ratchet:data")
            .and_then(|(_, rest)| rest.split_once("\n-->"))
            .map(|(data, _)| data)
            .unwrap_or_else(|| {
                panic!(
                    "{} has no `unsafe-ratchet:data` baseline block",
                    baseline_path.display()
                )
            });
        let data = jet_foundation::EncodingJson::parse_json(data, true)
            .expect("unsafe-region baseline must be valid JSON");
        let mut regions = std::collections::HashMap::<String, Vec<AuditRegion>>::new();
        for row in data.get("regions").unwrap().as_array().unwrap() {
            let file = row.get("file").unwrap().as_str().unwrap();
            let Some(stem) = file
                .strip_prefix("Examples/features/")
                .and_then(|file| file.strip_suffix(".jet"))
            else {
                continue;
            };
            let position = |key| match row.get(key).unwrap() {
                jet_foundation::Dyn::Dyn::Int(value) => {
                    value.to_usize().expect("audit position must be nonnegative")
                }
                _ => panic!("audit {key} must be an integer"),
            };
            regions.entry(stem.to_owned()).or_default().push(AuditRegion {
                line: position("line"),
                column: position("column"),
                reason: row.get("reason").unwrap().as_str().unwrap().to_owned(),
            });
        }
        assert!(!regions.is_empty(), "baseline records no audited feature examples");
        regions
    });

/// Use Jet's lexer so strings/comments containing marker spellings do not
/// invent regions, and escapes in reasons have their source-language meaning.
fn source_audit_regions(source: &str) -> Vec<AuditRegion> {
    use jet::Lexer::{StrTokPart, TokKind};
    let (mut tokens, diagnostics) = jet::Lexer::lex(source);
    assert!(diagnostics.is_empty(), "audit source failed lexing: {diagnostics:?}");
    tokens.retain(|token| !jet::Lexer::is_comment(&token.kind));
    let mut regions = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokKind::Hash {
            continue;
        }
        let marker_indices = match tokens.get(index + 1).map(|token| &token.kind) {
            Some(TokKind::Ident(name)) if name == "Unsafe" => index + 1..index + 2,
            Some(TokKind::LBracket) => {
                let mut depth = 1;
                let end = tokens[index + 2..].iter().position(|token| {
                    match token.kind {
                        TokKind::LBracket => depth += 1,
                        TokKind::RBracket => depth -= 1,
                        _ => {}
                    }
                    depth == 0
                }).map(|offset| index + 2 + offset)
                    .expect("checked source has an unfinished marker group");
                index + 2..end
            }
            _ => continue,
        };
        for marker in marker_indices {
            if !matches!(&tokens[marker].kind, TokKind::Ident(name) if name == "Unsafe")
                || tokens[marker - 1].kind == TokKind::Bang
                || tokens.get(marker + 1).map(|token| &token.kind) != Some(&TokKind::LParen)
            {
                continue;
            }
            let reason = match tokens.get(marker + 2).map(|token| &token.kind) {
                Some(TokKind::Str(parts)) => parts.iter().map(|part| match part {
                    StrTokPart::Lit(text) => text.as_str(),
                    _ => panic!("approved audit reason must be literal text"),
                }).collect::<String>(),
                Some(TokKind::RawStr(text)) => text.clone(),
                _ => panic!("approved audit reason must be literal text"),
            };
            let prefix = &source[..token.span.start];
            regions.push(AuditRegion {
                line: prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
                column: prefix.rsplit('\n').next().unwrap().chars().count() + 1,
                reason,
            });
        }
    }
    regions
}

/// True when the source declares a `#Test` block, read from Jet tokens so a
/// string or comment naming the marker does not count.
fn source_declares_tests(source: &str) -> bool {
    use jet::Lexer::TokKind;
    let (mut tokens, _) = jet::Lexer::lex(source);
    tokens.retain(|token| !jet::Lexer::is_comment(&token.kind));
    tokens.windows(2).any(|pair| {
        pair[0].kind == TokKind::Hash
            && matches!(&pair[1].kind, TokKind::Ident(name) if name == "Test")
    })
}

fn audit_region_failures(
    source: &str,
    file: &str,
    approved: &[AuditRegion],
    user_code: &str,
) -> Vec<String> {
    let source_regions = source_audit_regions(source);
    let mut failures = Vec::new();
    if source_regions != approved {
        failures.push(format!(
            "source audit regions differ from the approved baseline:\nsource: {source_regions:?}\napproved: {approved:?}"
        ));
    }
    let basename = Path::new(file).file_name().unwrap().to_str().unwrap();
    let mut required = std::collections::BTreeMap::new();
    for region in &source_regions {
        *required.entry((region.line, &region.reason)).or_insert(0usize) += 1;
    }
    for ((line_number, reason), count) in required {
        let site = format!(", {line_number}, {reason:?})");
        let full_site = format!("{file:?}{site}");
        let short_site = format!("{basename:?}{site}");
        let matches = user_code.lines().filter(|line| {
            (line.contains("jet_mem::jet_sentry_scope(")
                || line.contains("jet_mem::jet_sentry_fenced_scope("))
                && (line.contains(&full_site) || line.contains(&short_site))
        }).count();
        // Generic instantiations may emit a source region more than once.
        // They cannot make up for a missing different source site or reason.
        if matches < count {
            failures.push(format!(
                "missing lowered audit region at {file}:{line_number}: {reason:?} (expected {count}, found {matches})"
            ));
        }
    }
    failures
}

fn check_golden_entry(entry: &GoldenEntry, env: &GoldenEnv) {
    // D-JPK-TASKRUN1 / I5 (card #476): job_runner proves both `#Job` entry
    // paths — leaf `greet` stays callable while sibling `seed_data` calls it.
    if entry.stem == "devloop/job_runner" {
        check_job_runner_jobs(entry, env);
        return;
    }

    if entry.stem.starts_with("lowlevel/polyglot_") {
        check_polyglot_binder_example(entry, env);
        return;
    }
    let src = fs::read_to_string(&entry.path).unwrap();
    // Fixture state has one canonical, non-dot source tree. Materialize it as
    // project `.jet` state only in the disk-backed test scratch directory.
    let staged_fixture = FeatureFixtureScratch::for_entry(&entry.path);
    let compile_path = staged_fixture
        .as_ref()
        .map(|scratch| scratch.entry.to_string_lossy().into_owned())
        .unwrap_or_else(|| entry.shown.clone());


    let stem = entry.stem.as_str();
    let needs_gtk = stem == "ui/ui_native_linux";
    if needs_gtk && !env.have_gtk {
        assert_front_end(entry, &src);
        eprintln!(
            "note: front end checked; skipping Examples/features/{stem}.jet build (need gtk4)"
        );
        return;
    }
    let uses_ffi_bridge = matches!(
        stem,
        "lowlevel/ffi"
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
    );

    if uses_ffi_bridge && !env.have_cargo {
        assert_front_end(entry, &src);
        eprintln!(
            "note: front end checked; skipping Examples/features/{stem}.jet golden (need cargo for FFI bridge)"
        );
        return;
    }

    // Ordinary examples use the repo-relative `entry.shown` path, not the
    // absolute `entry.path`: Loader stores this value as the entry display path
    // and observable source-location examples pin it in their goldens. Projects
    // with canonical `fixture-state` are staged under safe scratch so their
    // required `.jet` state is never created in the checkout; their diagnostics
    // and output goldens still use `entry.shown`.
    let compiled_result = if has_package_build_entry(Path::new(&compile_path), &src) {
        jet::compile_programmable_build(&compile_path, &[])
    } else {
        jet::compile_with_path(&src, &compile_path)
    };
    let compiled = match compiled_result {
        Ok(c) => c,
        Err(diags) => panic!(
            "example {} failed the front end:\n{}",
            stem,
            jet::render_diagnostics(
                &format!("Examples/features/{}.{}", stem, env.ext),
                &src,
                &diags
            )
        ),
    };
    assert!(
        !compiled
            .lints
            .iter()
            .any(|diagnostic| diagnostic.code == "L0507"),
        "feature example {stem} still teaches the discouraged braced/chained branch form:\n{}",
        jet::render_diagnostics(&entry.shown, &src, &compiled.lints)
    );
    let rust_code = compiled.rust;
    let ffi_link = compiled.ffi;
    let user_code = strip_vetted_prelude_modules(&rust_code);

    if let Some(approved) = GATED_UNSAFE_REGIONS.get(stem) {
        // `#Test` bodies (and the regions inside them) exist only in the test
        // harness build, so the evidence is the run build plus that harness.
        let mut audit_code = user_code.clone();
        if source_declares_tests(&src) {
            let (harness, _) = jet::compile_tests_with_path(&src, &compile_path).unwrap_or_else(|diags| {
                panic!(
                    "audited example {stem} failed the test-harness front end:\n{}",
                    jet::render_diagnostics(&entry.shown, &src, &diags)
                )
            });
            audit_code.push('\n');
            audit_code.push_str(&strip_vetted_prelude_modules(&harness));
        }
        let failures = audit_region_failures(&src, &compile_path, approved, &audit_code);
        assert!(
            failures.is_empty(),
            "audited example {stem} lost source-region evidence:\n{}",
            failures.join("\n")
        );
        for (i, line) in user_code.lines().enumerate() {
            for col in unsafe_keyword_columns(line) {
                let after = line[col + "unsafe".len()..].trim_start();
                assert!(
                    after.starts_with('{') || after.starts_with("fn "),
                    "{} emits an ungated `unsafe` at line {}: {}",
                    stem,
                    i + 1,
                    line.trim()
                );
            }
        }
    } else if stem == "game/raylib_window" {
        assert!(
            !user_code.contains("unsafe fn __jet_"),
            "raylib user functions must stay safe; bridge unsafe stays in vetted prelude"
        );
    } else {
        let leftover: Vec<String> = user_code
            .lines()
            .enumerate()
            .filter(|(_, line)| !unsafe_keyword_columns(line).is_empty())
            .map(|(i, line)| format!("  line {}: {}", i + 1, line.trim()))
            .collect();
        assert!(
            leftover.is_empty(),
            "generated Rust for {} contains `unsafe` outside vetted memory helpers:\n{}",
            stem,
            leftover.join("\n")
        );
    }
    assert!(
        rust_code.contains("fn main()"),
        "generated Rust for {} has no fn main",
        stem
    );

    if needs_gtk && !env.have_rustc {
        eprintln!(
            "note: front end checked; skipping Examples/features/{stem}.jet build (need gtk4 + rustc)"
        );
        return;
    }
    let needs_raylib_display = stem == "game/raylib_window";
    if needs_raylib_display && std::env::var("JET_RAYLIB_DISPLAY").as_deref() != Ok("1") {
        eprintln!(
            "note: front end checked; skipping Examples/features/{stem}.jet build (set JET_RAYLIB_DISPLAY=1)"
        );
        return;
    }

    if !env.have_rustc {
        return;
    }
    let flat_stem = stem.replace('/', "_");
    let dir = std::env::temp_dir();
    let rs = dir.join(format!(
        "jet_golden_{}_{}.rs",
        std::process::id(),
        flat_stem
    ));
    let bin = dir.join(format!("jet_golden_{}_{}", std::process::id(), flat_stem));
    let _program_files = ProgramFiles([rs.clone(), bin.clone()]);
    let mut rustc_cmd = Command::new("rustc");
    let _runtime_lease = add_generated_rust(&mut rustc_cmd, &rs, &rust_code, ffi_link.is_some(), &[]);
    rustc_cmd.arg("-o").arg(&bin);
    if let Some(link) = &ffi_link {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    // Link resolution reads the same project state the front end compiled
    // against: a staged fixture's `.jet` exists only in its scratch copy.
    let project_entry = staged_fixture
        .as_ref()
        .map_or(entry.path.as_path(), |scratch| scratch.entry.as_path());
    let clinks = jet::resolve_c_links(project_entry.to_str().unwrap())
        .expect("resolve_c_links should succeed for example C bindings");
    for arg in &clinks {
        rustc_cmd.arg(arg);
    }
    let out = rustc_cmd.output().unwrap();
    assert!(
        out.status.success(),
        "I2 violated: rustc rejected generated code for {} — this is a jet bug:\n{}",
        stem,
        String::from_utf8_lossy(&out.stderr)
    );
    // #2016 / D-WEBAPP-SERVE1=D: a service entry (an `App`-returning `run`, or
    // a declared `Output.Service`) binds a listener and then serves until the
    // process is stopped. Such a program has no terminating stdout and no exit
    // code of its own, so it cannot have a `.out` golden — running it here
    // would hang the suite or race a fixed port. A browser-only program's
    // native run fails closed by design; its run proof is tests/web_build.rs.
    // Both stay in the COMPILE universe: front end, I1, and I2 above all still
    // apply. Held back by declared facts, so a new example is classified
    // instead of discovered as a timeout.
    if let Some(reason) = example_native_run_skip(project_entry) {
        eprintln!("note: built Examples/features/{stem}.jet; not run ({reason})");
        return;
    }

    let mut run_cmd = Command::new(&bin);
    if let Some(cwd) = staged_fixture
        .as_ref()
        .and_then(|scratch| scratch.runtime_cwd.as_ref())
    {
        run_cmd.current_dir(cwd);
    }
    if needs_gtk {
        run_cmd.env("JET_UI_HEADLESS", "1");
    }
    // An interactive example only reproduces its golden against the answers
    // that golden was recorded with. `common::example_stdin` is the one home
    // for those answers (I8); this run asks for them by stem instead of
    // naming the example and restating its bytes.
    let run = if let Some(answers) = example_stdin(stem) {
        use std::io::Write;
        use std::process::Stdio;
        let mut child = run_cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(answers.piped.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    } else {
        run_cmd.output().unwrap()
    };
    if needs_gtk && !run.status.success() && gtk_loader_unavailable(&run.stderr) {
        eprintln!(
            "note: skipping Examples/features/{stem}.jet run (gtk4 runtime loader unavailable)"
        );
        return;
    }
    // Suffix rule (Examples/README.md "Auxiliary golden stream suffix"):
    // `.err.out` = expected non-zero exit (panic/uncaught Err); `.stderr.out`
    // = expected exit 0 with pinned incidental stderr. Never a third suffix.
    let err_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.err.out", stem));
    let success_err_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.stderr.out", stem));
    if err_path.exists() {
        let expected_err = fs::read_to_string(&err_path)
            .unwrap_or_else(|_| panic!("missing Examples/features/expected/{stem}.err.out"));
        let expected_code = expected_failure_exit(stem, &expected_err).unwrap_or_else(|| {
            panic!("ambiguous failure exit oracle for example {stem}: {expected_err:?}")
        });
        let code = run.status.code();
        assert_eq!(code, Some(expected_code), "exit code mismatch for example {stem}");
        let out_path = env.ex_dir.join("expected").join(format!("{stem}.out"));
        if env.update_expected {
            if out_path.exists() || !run.stdout.is_empty() {
                fs::write(&out_path, &run.stdout).unwrap();
            }
        } else {
            let expected_stdout = match fs::read(&out_path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => panic!("could not read {}: {error}", out_path.display()),
            };
            assert_failure_stdout(stem, &expected_stdout, &run.stdout);
        }
        let actual_err = String::from_utf8_lossy(&run.stderr);
        if env.update_expected {
            if out_path.exists() || !run.stdout.is_empty() {
                fs::write(&out_path, &run.stdout).unwrap();
            }
        } else {
            if run.stderr.as_slice() != expected_err.as_bytes() {
                panic!(
                    "stderr mismatch for example {stem}:\n{}",
                    unified_diff(
                        &format!("Examples/features/expected/{stem}.err.out"),
                        &format!("Examples/features/{stem} stderr (actual)"),
                        &expected_err,
                        &actual_err,
                    )
                );
            }
        }
        record_golden_receipt(entry, env, &run, &run.stdout);
    } else {
        assert!(
            run.status.success(),
            "example {} failed at runtime:\nstdout: {}\nstderr: {}",
            stem,
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        let out_path = env.ex_dir.join("expected").join(format!("{}.out", stem));
        let actual_raw = String::from_utf8_lossy(&run.stdout);
        // CLI usage intentionally names argv[0]. The golden binary has a
        // process-specific temporary name, so normalize it to the source
        // basename while keeping every other byte exact.
        let actual_normalized;
        let actual = if stem.starts_with("cli/") {
            let source_name = entry
                .path
                .file_stem()
                .and_then(|name| name.to_str())
                .expect("golden source name is utf8");
            let binary_name = bin
                .file_name()
                .and_then(|name| name.to_str())
                .expect("golden binary name is utf8");
            actual_normalized = actual_raw.replacen(
                &format!("Usage: {binary_name}"),
                &format!("Usage: {source_name}"),
                1,
            );
            actual_normalized.as_str()
        } else {
            actual_raw.as_ref()
        };
        if env.update_expected {
            fs::create_dir_all(out_path.parent().unwrap()).unwrap();
            fs::write(&out_path, actual.as_bytes()).unwrap();
        } else {
            assert!(
                out_path.is_file(),
                "missing Examples/features/expected/{stem}.out; run with JET_UPDATE_GOLDEN=1 and a scoped JET_GOLDEN_FILTER to create it"
            );
            let expected = fs::read_to_string(&out_path).unwrap();
            if stem == "tooling/compute_vulkan_webgpu" {
                // Vulkan availability is host-dependent: the shell may expose
                // the loader while the machine still has no usable device.
                // Validate the portable contract instead of pinning one host's
                // acceptance bit in a cross-host golden.
                assert_compute_vulkan_webgpu_output(actual);
            } else if stem == "foundations/realtime" {
                assert_foundations_realtime_output(&expected);
                assert_foundations_realtime_output(actual);
            } else if stem == "foundations/text" {
                assert_foundations_text_output(actual, &expected);
            } else if actual != expected {
                panic!(
                    "output mismatch for example {stem}:\n{}",
                    unified_diff(
                        &format!("Examples/features/expected/{stem}.out"),
                        &format!("Examples/features/{stem} stdout (actual)"),
                        &expected,
                        &actual,
                    )
                );
            }
        }
        if success_err_path.exists() {
            let actual_err = String::from_utf8_lossy(&run.stderr);
            if env.update_expected {
                fs::write(&success_err_path, actual_err.as_bytes()).unwrap();
            } else {
                let expected_err = fs::read_to_string(&success_err_path).unwrap_or_else(|_| {
                    panic!("missing Examples/features/expected/{}.stderr.out", stem)
                });
                if actual_err != expected_err {
                    panic!(
                        "stderr mismatch for example {stem}:\n{}",
                        unified_diff(
                            &format!("Examples/features/expected/{stem}.stderr.out"),
                            &format!("Examples/features/{stem} stderr (actual)"),
                            &expected_err,
                            &actual_err,
                        )
                    );
                }
            }
        }
        record_golden_receipt(entry, env, &run, actual.as_bytes());
    }
}

/// Keep the expected channel as the executable-spec input, but record the
/// observed result in the same content-addressed store as CLI test results.
/// A missing expected channel produces no claim; an absent input cannot make a
/// status projection look current.
fn record_golden_receipt(
    entry: &GoldenEntry,
    env: &GoldenEnv,
    output: &std::process::Output,
    stdout: &[u8],
) {
    let err_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.err.out", entry.stem));
    let stdout_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.out", entry.stem));
    let stderr_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.stderr.out", entry.stem));
    let mut expected_paths = if err_path.is_file() {
        let mut paths = vec![err_path];
        if stdout_path.is_file() {
            paths.push(stdout_path);
        }
        paths
    } else if stdout_path.is_file() {
        vec![stdout_path]
    } else {
        return;
    };
    if stderr_path.is_file() {
        expected_paths.push(stderr_path);
    }
    record_golden_receipt_at_output(
        entry,
        env,
        output,
        stdout,
        &output.stderr,
        "aot",
        None,
        &expected_paths,
        &[],
    );
}

fn record_golden_receipt_at(
    entry: &GoldenEntry,
    env: &GoldenEnv,
    output: &std::process::Output,
    tier: &str,
    variant: Option<&str>,
    expected_paths: &[PathBuf],
    extra_inputs: &[PathBuf],
) {
    record_golden_receipt_at_output(
        entry,
        env,
        output,
        &output.stdout,
        &output.stderr,
        tier,
        variant,
        expected_paths,
        extra_inputs,
    );
}

fn record_golden_receipt_at_output(
    entry: &GoldenEntry,
    env: &GoldenEnv,
    output: &std::process::Output,
    stdout: &[u8],
    stderr: &[u8],
    tier: &str,
    variant: Option<&str>,
    expected_paths: &[PathBuf],
    extra_inputs: &[PathBuf],
) {
    if expected_paths.iter().any(|path| !path.is_file()) {
        return;
    }
    let mut inputs = vec![entry.path.clone()];
    inputs.extend(extra_inputs.iter().cloned().filter(|path| path.is_file()));
    inputs.extend(expected_paths.iter().cloned());
    let root = env
        .ex_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    let store_root = std::env::var_os("JET_RECEIPT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".jet").join("receipts"));
    let store = jet::ReceiptStore::ReceiptStore::new(store_root);
    let mut argv = vec!["golden".into(), entry.path.to_string_lossy().into_owned(), tier.to_string()];
    if let Some(variant) = variant {
        argv.push(variant.to_string());
    }
    let status = output.status.code().unwrap_or(1);
    store
        .record("golden", &argv, &inputs, status, stdout, stderr)
        .unwrap_or_else(|error| {
            panic!(
                "could not record golden receipt for {}: {error}",
                entry.stem
            )
        });
}

/// Use the production package resolver to decide whether an example needs the
/// programmable-build path. The source-text check remains for source files
/// whose package build entry is declared in their ordinary Jet source.
fn has_package_build_entry(path: &Path, source: &str) -> bool {
    if source.contains("fn build(") {
        return true;
    }
    let Some(root) = path.parent() else {
        return false;
    };
    let Ok(resolver) = jet::Authority::AuthorityResolver::open(root) else {
        return false;
    };
    let facts = match resolver.checked_manifest(Path::new(".")) {
        Ok(manifest) => manifest.facts,
        Err(error) if error.is_missing() => jet::Package::PackageFacts::default(),
        Err(_) => return false,
    };
    facts
        .resolve_build_entry_checked(&resolver)
        .ok()
        .flatten()
        .is_some()
}

/// Polyglot examples this host could not run, as `stem (missing tool)`. The
/// run reports their count as UNAVAILABLE; an unavailable example is never a
/// silent pass.
static POLYGLOT_UNAVAILABLE: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Record one polyglot example this host cannot run. A tool named in
/// `JET_REQUIRE_POLYGLOT` (comma-separated tool names, or `all`) is required
/// on this machine, so its absence fails the example instead.
fn polyglot_unavailable(stem: &str, tool: &str) {
    let required = std::env::var("JET_REQUIRE_POLYGLOT").unwrap_or_default();
    let required = required
        .split(',')
        .map(str::trim)
        .any(|name| name == "all" || name == tool);
    assert!(
        !required,
        "Examples/features/{stem}: required polyglot toolchain `{tool}` is unavailable on this machine (JET_REQUIRE_POLYGLOT)"
    );
    POLYGLOT_UNAVAILABLE
        .lock()
        .unwrap()
        .push(format!("{stem} ({tool})"));
}

/// I5 for card #502: each managed-runtime example binds its checked foreign
/// source through the public CLI, then runs the generated project binding.
/// Generated caches and native archives stay temp-only and host-specific.
/// The front end needs the generated binding cache (E3208), so it is checked
/// after the bind step, never against the bare example directory.
fn check_polyglot_binder_example(entry: &GoldenEntry, env: &GoldenEnv) {
    if entry.stem == "lowlevel/polyglot_com" {
        // COM binds a host-compiled Windows type library and the example
        // claims no Windows execution; `tests/ffi_com.rs` proves its E3260
        // gate on every other host.
        if !cfg!(windows) {
            polyglot_unavailable(&entry.stem, "windows-com");
            return;
        }
        panic!("Examples/features/lowlevel/polyglot_com: the Windows COM golden has no host runner yet");
    }
    let (language, package, foreign_source) = match entry.stem.as_str() {
        "lowlevel/polyglot_go" => ("go", "handles", "handles.go"),
        "lowlevel/polyglot_java" => ("java", "counter", "Counter.java"),
        "lowlevel/polyglot_dotnet" => ("cs", "counter", "Counter.cs"),
        "lowlevel/polyglot_pascal" => ("pascal", "counter", "Counter.pas"),
        "lowlevel/polyglot_ada" => ("ada", "geodesy", "geodesy.ads"),
        "lowlevel/polyglot_fortran" => ("fortran", "matrix", "matrix.f90"),
        "lowlevel/polyglot_tcl" => ("tcl", "eda", "eda.tcl"),
        "lowlevel/polyglot_dart" => ("dart", "callbacks", "callbacks.dart"),
        other => panic!("unknown polyglot golden `{other}`"),
    };
    if !env.have_rustc || !env.have_cargo {
        polyglot_unavailable(&entry.stem, "rustc+cargo");
        return;
    }
    // These examples exercise the real foreign-language inspection path. A
    // host without the language tool reports the example UNAVAILABLE, or
    // fails it when JET_REQUIRE_POLYGLOT names that tool.
    // Each tool's own version query: `go` takes a `version` subcommand and
    // `fpc` prints its version with `-iV`; neither accepts `--version`, so a
    // provisioned toolchain was misreported UNAVAILABLE.
    let (tool, version_arg) = match language {
        "go" => ("go", "version"),
        "fortran" => ("gfortran", "--version"),
        "java" => ("javac", "--version"),
        "cs" => ("dotnet", "--version"),
        "pascal" => ("fpc", "-iV"),
        "ada" => ("gnatmake", "--version"),
        "tcl" => ("tclsh", "--version"),
        "dart" => ("dart", "--version"),
        _ => unreachable!("binder language has no provisioned tool"),
    };
    let have_tool = Command::new(tool)
        .arg(version_arg)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !have_tool {
        polyglot_unavailable(&entry.stem, tool);
        return;
    }

    let flat_stem = entry.stem.replace('/', "_");
    let dir = std::env::temp_dir().join(format!("jet_golden_{}_{}", std::process::id(), flat_stem));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source_dir = entry.path.parent().expect("polyglot example directory");
    fs::copy(&entry.path, dir.join("run.jet")).unwrap();
    fs::copy(source_dir.join(foreign_source), dir.join(foreign_source)).unwrap();
    if language == "ada" {
        fs::copy(source_dir.join("geodesy.adb"), dir.join("geodesy.adb")).unwrap();
    }
    if language == "dart" {
        fs::copy(source_dir.join("host.dart"), dir.join("host.dart")).unwrap();
    }

    let mut bind = Command::new(env!("CARGO_BIN_EXE_jet"));
    bind.current_dir(&dir).env("NO_COLOR", "1");
    bind.args(["inspect", "bind", language, foreign_source]);
    if language == "dart" {
        bind.args(["--jet", "run.jet"]);
    }
    let bind = bind.args(["--pkg", package]).output().unwrap();
    assert!(
        bind.status.success(),
        "{} binder example failed:\n{}",
        language,
        String::from_utf8_lossy(&bind.stderr)
    );
    let check = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", "run.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{} generated binding failed its front end:\n{}",
        language,
        String::from_utf8_lossy(&check.stderr)
    );

    let run = if language == "dart" {
        Command::new("dart")
            .args(["run", "host.dart"])
            .current_dir(&dir)
            .output()
            .unwrap()
    } else {
        Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["run", "run.jet"])
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };
    assert!(
        run.status.success(),
        "{} binder example failed at runtime:\nstdout: {}\nstderr: {}",
        language,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let out_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.out", entry.stem));
    assert!(
        out_path.is_file(),
        "missing Examples/features/expected/{}.out",
        entry.stem
    );
    let mut foreign_inputs = vec![source_dir.join(foreign_source)];
    if language == "ada" {
        foreign_inputs.push(source_dir.join("geodesy.adb"));
    }
    if language == "dart" {
        foreign_inputs.push(source_dir.join("host.dart"));
    }
    record_golden_receipt_at(
        entry,
        env,
        &run,
        "dev",
        None,
        std::slice::from_ref(&out_path),
        &foreign_inputs,
    );
    let actual = String::from_utf8_lossy(&run.stdout);
    if env.update_expected {
        fs::write(&out_path, actual.as_bytes()).unwrap();
    } else {
        let expected = fs::read_to_string(&out_path).unwrap();
        if actual != expected {
            panic!(
                "output mismatch for example {}:\n{}",
                entry.stem,
                unified_diff(
                    &format!("Examples/features/expected/{}.out", entry.stem),
                    &format!("Examples/features/{}/run.jet stdout (actual)", entry.stem),
                    &expected,
                    &actual,
                )
            );
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

/// I5 for `Examples/features/devloop/job_runner.jet`: one generated binary
/// dispatches every named `#Job` entry through argv.
fn check_job_runner_jobs(entry: &GoldenEntry, env: &GoldenEnv) {
    let src = fs::read_to_string(&entry.path).unwrap();
    let path = entry.path.to_str().expect("example path is utf8");
    let compiled = jet::compile_with_path(&src, path).unwrap_or_else(|diags| {
        panic!(
            "job_runner subcommand table failed the front end:\n{}",
            jet::render_diagnostics(&entry.shown, &src, &diags)
        )
    });
    let job_runner_user_code = strip_vetted_prelude_modules(&compiled.rust);
    assert!(
        job_runner_user_code
            .lines()
            .all(|line| unsafe_keyword_columns(line).is_empty()),
        "generated Rust for job_runner contains ungated `unsafe`"
    );
    assert!(
        compiled.rust.contains("fn main()"),
        "job_runner has no fn main"
    );
    if !env.have_rustc {
        return;
    }
    let dir = std::env::temp_dir();
    let rs = dir.join(format!(
        "jet_golden_{}_devloop_job_runner.rs",
        std::process::id()
    ));
    let bin = dir.join(format!(
        "jet_golden_{}_devloop_job_runner",
        std::process::id()
    ));
    let mut rustc = Command::new("rustc");
    let _runtime_lease = add_generated_rust(&mut rustc, &rs, &compiled.rust, compiled.ffi.is_some(), &[]);
    rustc.arg("-o").arg(&bin);
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
    let out = rustc.output().unwrap();
    assert!(
        out.status.success(),
        "I2 violated: rustc rejected job_runner:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for (job, expected_name) in [
        ("greet", "job_runner.greet"),
        ("seed_data", "job_runner.seed_data"),
    ] {
        let run = Command::new(&bin).arg(job).output().unwrap();
        assert!(
            run.status.success(),
            "job_runner subcommand `{job}` failed at runtime:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        let out_path = env
            .ex_dir
            .join("expected")
            .join(format!("devloop/{expected_name}.out"));
        assert!(out_path.is_file(), "missing expected output for `{job}`");
        record_golden_receipt_at(
            entry,
            env,
            &run,
            "aot",
            Some(job),
            std::slice::from_ref(&out_path),
            &[],
        );
        let actual = String::from_utf8_lossy(&run.stdout);
        if env.update_expected {
            fs::write(&out_path, actual.as_bytes()).unwrap();
        } else {
            let expected = fs::read_to_string(&out_path).unwrap();
            if actual != expected {
                panic!(
                    "output mismatch for job_runner subcommand `{job}`:\n{}",
                    unified_diff(
                        &format!("Examples/features/expected/devloop/{expected_name}.out"),
                        &format!("Examples/features/devloop/job_runner subcommand `{job}` stdout"),
                        &expected,
                        &actual,
                    )
                );
            }
        }
    }
    let _ = fs::remove_file(&rs);
    let _ = fs::remove_file(&bin);
}

#[test]
fn watcher_process_probe_is_vetted_without_hiding_user_unsafe() {
    let generated = "// JET_VETTED_UNSAFE_BEGIN: jet_watch_process_probe\nunsafe { ffi() }\n// JET_VETTED_UNSAFE_END: jet_watch_process_probe\nunsafe { user_pointer() }";
    let stripped = strip_vetted_prelude_modules(generated);
    assert!(!stripped.contains("ffi()"));
    assert!(stripped.contains("unsafe { user_pointer() }"));
}

#[test]
fn shared_guard_runtime_is_vetted_without_hiding_user_unsafe() {
    let generated = "// jet:shared-guard-internal-begin\nunsafe { first() }\n// jet:shared-guard-internal-end\n// jet:shared-guard-internal-begin\nunsafe { second() }\n// jet:shared-guard-internal-end\nunsafe { user_pointer() }";
    let stripped = strip_vetted_prelude_modules(generated);
    assert!(!stripped.contains("first()"));
    assert!(!stripped.contains("second()"));
    assert!(stripped.contains("unsafe { user_pointer() }"));
}

#[test]
fn semantic_corpus_policy_runs_with_the_golden_gate() {
    common::corpus_policy::CorpusPolicy::load()
        .expect("corpus manifest")
        .check_gate("golden")
        .expect("golden corpus semantic policy");
}

#[test]
fn audit_region_check_rejects_a_missing_sentry() {
    let source = "fn run() {\n    #Unsafe(\"same reason\") {}\n    #Unsafe(\"same reason\") {}\n}\n";
    let approved = source_audit_regions(source);
    let one_sentry = "let a = jet_mem::jet_sentry_scope(true, \"example.jet\", 2, \"same reason\");\n";
    let failures = audit_region_failures(source, "example.jet", &approved, one_sentry);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("example.jet:3:"), "{failures:?}");
    // An unrelated unsafe block or repeated evidence for the first source
    // region must not make the missing second region pass.
    let false_positive = format!("{one_sentry}{one_sentry}unsafe {{ prelude_fd() }}\n");
    assert_eq!(
        audit_region_failures(source, "example.jet", &approved, &false_positive).len(),
        1
    );
    let complete = format!(
        "{one_sentry}let b = jet_mem::jet_sentry_fenced_scope(false, \"example.jet\", 3, \"same reason\");\n"
    );
    assert!(audit_region_failures(source, "example.jet", &approved, &complete).is_empty());
    let same_line = "fn run() {\n    #Unsafe(\"same reason\") {} #Unsafe(\"same reason\") {}\n}\n";
    let failures = audit_region_failures(
        same_line, "example.jet", &source_audit_regions(same_line), one_sentry,
    );
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("expected 2, found 1"), "{failures:?}");
}

#[test]
fn audit_region_source_scan_uses_jet_strings_and_markers() {
    let source = "// #Unsafe(\"comment\") {}\nfn run() {\n    text :: \"#Unsafe(\\\"string\\\")\"\n    #Unsafe(\"quoted \\\"reason\\\"\") {}\n}\n#[Pure, Unsafe(\"function reason\")]\nfn expert() {}\n";
    let regions = source_audit_regions(source);
    assert_eq!(regions.len(), 2, "{regions:?}");
    assert_eq!(regions[0].reason, "quoted \"reason\"");
    assert_eq!(regions[0].line, 4);
    assert_eq!(regions[1].reason, "function reason");
    assert_eq!(regions[1].line, 6);
}

fn expected_failure_exit(stem: &str, stderr: &str) -> Option<i32> {
    // D-FAIL-EXIT1 / S36: runtime faults are not unhandled entry returns.
    // D-DEADLINE1 explicitly gives an expired join (E3003) exit 70.
    if stderr.starts_with("Stop [") || stderr.starts_with("Error [E3003]:") {
        return Some(70);
    }
    if stderr.starts_with("Runtime fault [R0801]:")
        || stderr.starts_with("Runtime fault [R0802]:")
    {
        // D-MEM-SENTRY1 is a runtime fault. Reject an oracle which itself
        // claims another exit rather than silently accepting its contradiction.
        if stderr.lines().any(|line| {
            line.trim().strip_prefix("exit:").is_some_and(|exit| exit.trim() != "70")
        }) {
            return None;
        }
        return Some(70);
    }
    // io/log_fatal.jet:10-11 suppresses emission before log.fatal exits 1
    // (ledger #1474). Empty stderr alone is not an exit oracle.
    if stderr.is_empty() {
        return (stem == "io/log_fatal").then_some(1);
    }
    // Source-proved entry returns (D-FAIL-EXIT1), not a generic Error-prefix
    // inference. Each source leaves the named Err unhandled at run's edge;
    // fatal_exit instead explicitly calls process.exit(1).
    match stem {
        "errors/propagation_journey"
        | "errors/propagation_journey_direct"
        | "errors/propagation_journey_one_hop"
        | "errors/propagation_journey_core"
        | "errors/error_context"
        | "errors/default_error_conversion"
        | "errors/default_err_edge"
        | "errors/fatal_exit"
        | "serde/decode_error_exit"
        | "crypto/crypto_error_run" => Some(1),
        _ => None,
    }
}

fn assert_failure_stdout(stem: &str, expected: &[u8], actual: &[u8]) {
    if actual != expected {
        panic!(
            "output mismatch for failing example {stem}:\n{}",
            unified_diff(
                &format!("Examples/features/expected/{stem}.out"),
                &format!("Examples/features/{stem} stdout (actual)"),
                &String::from_utf8_lossy(expected),
                &String::from_utf8_lossy(actual),
            )
        );
    }
}

#[test]
fn failure_oracle_requires_exact_exit_and_stdout() {
    assert_eq!(expected_failure_exit("test", "Stop [E3001]: panic\n"), Some(70));
    assert_eq!(expected_failure_exit("test", "Stop [E3012]: Stack overflow\n"), Some(70));
    assert_eq!(expected_failure_exit("test", "Error [E3003]: Deadline exceeded\n"), Some(70));
    assert_eq!(expected_failure_exit("errors/default_err_edge", "Error [RUNFAIL]: bad input\n"), Some(1));
    assert_eq!(expected_failure_exit("errors/fatal_exit", "[fatal] bad input\n"), Some(1));
    assert_eq!(expected_failure_exit("test", "Error [E3999]: unknown fault\n"), None);
    assert_eq!(expected_failure_exit("test", "Error: unknown entry\n"), None);
    assert_eq!(expected_failure_exit("test", "Runtime fault [R0802]: bad storage\nexit: 70\n"), Some(70));
    assert_eq!(expected_failure_exit("test", "Runtime fault [R0802]: bad storage\nexit: 1\n"), None);
    assert_eq!(expected_failure_exit("test", "Runtime fault [R0802]: bad storage\n"), Some(70));
    assert_eq!(expected_failure_exit("serde/decode_error_exit", "[at `name`: expected Text]\n  failed here: run (a.jet:1)\n"), Some(1));
    assert_eq!(expected_failure_exit("io/log_fatal", ""), Some(1));
    assert_eq!(expected_failure_exit("unknown", ""), None);
    assert_eq!(expected_failure_exit("test", "unknown report\n"), None);
    assert_failure_stdout("empty", b"", b"");
    assert_failure_stdout("close", b"closed\n", b"closed\n");
    assert!(std::panic::catch_unwind(|| assert_failure_stdout("close", b"closed\n", b"")).is_err());
    assert!(std::panic::catch_unwind(|| assert_failure_stdout("empty", b"", b"unexpected\n")).is_err());
}
