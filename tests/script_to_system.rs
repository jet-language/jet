//! Card #1415: one source grows through the supported Jet project forms.
//!
//! The test copies only `run.jet` first. Every later artifact is made by a
//! documented command or by the one package metadata edit recorded in the
//! journey document. The source bytes and dependency lock are checked across
//! the structural transition.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

mod common;
use common::{jetpack_bin, Scratch};

const EXPECTED_STDOUT: &[u8] =
    include_bytes!("../examples/continuity/script_to_system/expected.out");
const RECEIPT_NAME: &str = "script-to-system.tsv";
const RECEIPT_VERSION: u8 = 2;

fn receipt_path(cache: &Path) -> PathBuf {
    cache.join(RECEIPT_NAME)
}

fn start_receipt(cache: &Path) {
    fs::write(
        receipt_path(cache),
        format!(
            "version={RECEIPT_VERSION}\thost={}\tarch={}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    )
    .expect("create the script-to-system receipt");
}

fn receipt_operation(label: &str) -> &'static str {
    if label == "fold-rollback" || label.starts_with("rollback-") {
        "rollback"
    } else if label.starts_with("split-") {
        "split"
    } else if label.starts_with("fold-") {
        "fold"
    } else if label.contains("install") {
        "install"
    } else if label.contains("library")
        || label.starts_with("export-")
        || label.contains("aot")
        || label.contains("rebuild")
    {
        "export"
    } else {
        "journey"
    }
}

fn bytes_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", jet::SHA256::sha256_hex(bytes))
}

fn file_digest(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    bytes_digest(&bytes)
}

fn record_receipt(
    cache: &Path,
    event: &str,
    label: &str,
    elapsed: Duration,
    status: &str,
    detail: &str,
) {
    let detail = detail.replace('\t', " ").replace('\n', " ");
    let line = format!(
        "event={event}\toperation={}\tlabel={label}\telapsed_ms={}\tstatus={status}\tdetail={detail}\n",
        receipt_operation(label),
        elapsed.as_millis()
    );
    OpenOptions::new()
        .append(true)
        .open(receipt_path(cache))
        .expect("open the script-to-system receipt")
        .write_all(line.as_bytes())
        .expect("write the script-to-system receipt");
    eprint!("script-to-system receipt {line}");
}

fn assert_lock_preserved(
    cache: &Path,
    root: &Path,
    expected: &[u8],
    expected_digest: &str,
    label: &str,
) {
    let lock = root.join(".jet/lock");
    let actual = fs::read(&lock)
        .unwrap_or_else(|error| panic!("read {} after {label}: {error}", lock.display()));
    assert_eq!(
        actual.as_slice(),
        expected,
        "{label} changed authoritative lock bytes"
    );
    let actual_digest = bytes_digest(&actual);
    assert_eq!(
        actual_digest, expected_digest,
        "{label} changed authoritative lock digest"
    );
    record_receipt(
        cache,
        "proof",
        label,
        Duration::ZERO,
        "ok",
        &format!(
            "lock_bytes={} lock_sha256={expected_digest} lock_unchanged=true",
            expected.len()
        ),
    );
}

fn assert_source_preserved(
    cache: &Path,
    root: &Path,
    expected: &[u8],
    expected_digest: &str,
    label: &str,
) {
    let source = root.join("run.jet");
    let actual = fs::read(&source)
        .unwrap_or_else(|error| panic!("read {} after {label}: {error}", source.display()));
    assert_eq!(actual.as_slice(), expected, "{label} changed run.jet");
    assert_eq!(
        bytes_digest(&actual),
        expected_digest,
        "{label} changed run.jet digest"
    );
    record_receipt(
        cache,
        "proof",
        label,
        Duration::ZERO,
        "ok",
        &format!("path=run.jet source_sha256={expected_digest} source_unchanged=true"),
    );
}

fn edit_file(cache: &Path, root: &Path, label: &str, path: &Path, contents: &str) {
    let started = Instant::now();
    fs::write(path, contents).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    let relative = path.strip_prefix(root).unwrap_or(path);
    record_receipt(
        cache,
        "edit",
        label,
        started.elapsed(),
        "ok",
        &format!("path={}", relative.display()),
    );
}

fn record_artifact(cache: &Path, root: &Path, path: &Path) {
    let started = Instant::now();
    let bytes = fs::read(path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let metadata =
        fs::metadata(path).unwrap_or_else(|error| panic!("stat {}: {error}", path.display()));
    let relative = path.strip_prefix(root).unwrap_or(path);
    record_receipt(
        cache,
        "artifact",
        "final-output",
        started.elapsed(),
        "ok",
        &format!(
            "path={} bytes={} sha256={}",
            relative.display(),
            metadata.len(),
            bytes_digest(&bytes)
        ),
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectGraphTruth {
    fingerprint: String,
    dependencies: String,
    dependency_sha256: String,
}

fn recompute_project_graph(root: &Path) -> ProjectGraphTruth {
    // Do not read the transition's stated fingerprint. Reload checked facts
    // from the files on disk after each apply and derive the graph from those
    // facts, so preflight and journal metadata remain comparison evidence.
    let package = match jet::Package::PackageFacts::load(root) {
        Some(Ok(package)) => package,
        Some(Err(error)) => panic!("load package facts from {}: {error}", root.display()),
        None => panic!("no package facts in {}", root.display()),
    };
    let graph = format!(
        "name={:?}\nversion={:?}\njet={:?}\nsource={:?}\ndeps={:?}\nservices={:?}\noutputs={:?}\nenvironments={:?}\ndefaults={:?}\n",
        package.name,
        package.version,
        package.jet,
        package.source,
        package.deps,
        package.services,
        package.outputs,
        package.environments,
        package.defaults
    );
    let dependencies = format!("{:?}", package.deps);
    ProjectGraphTruth {
        fingerprint: bytes_digest(graph.as_bytes()),
        dependency_sha256: bytes_digest(dependencies.as_bytes()),
        dependencies,
    }
}

fn assert_graph_preserved(before: &ProjectGraphTruth, after: &ProjectGraphTruth, label: &str) {
    assert_eq!(
        after.fingerprint, before.fingerprint,
        "{label} changed the checked package graph"
    );
    assert_eq!(
        after.dependencies, before.dependencies,
        "{label} changed package dependencies"
    );
    assert_eq!(
        after.dependency_sha256, before.dependency_sha256,
        "{label} changed the dependency identity"
    );
}

fn transition_fingerprints(output: &Output) -> (String, String) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(fingerprint) = stdout
        .lines()
        .find_map(|line| line.strip_prefix("package graph unchanged: "))
    {
        return (fingerprint.to_owned(), fingerprint.to_owned());
    }
    let before = stdout
        .lines()
        .find_map(|line| line.strip_prefix("package graph before: "));
    let after = stdout
        .lines()
        .find_map(|line| line.strip_prefix("package graph after: "));
    match (before, after) {
        (Some(before), Some(after)) => (before.to_owned(), after.to_owned()),
        _ => panic!("transition output lacks graph identity:\n{stdout}"),
    }
}

/// One graph identity for a transition that must not change the package graph.
/// Split and fold are pure source reorganizations, so the before and after
/// fingerprints have to agree; disagreement is the defect this proves absent.
fn transition_graph(output: &Output) -> String {
    let (before, after) = transition_fingerprints(output);
    assert_eq!(
        before,
        after,
        "transition changed the package graph: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    before
}

fn transition_journal(output: &Output) -> PathBuf {
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("Transition journal: "))
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("transition output lacks journal path:\n{stdout}"))
}

fn journal_value(contents: &str, field: &str, path: &Path) -> String {
    let prefix = format!("{field}=");
    contents
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("journal {} lacks {field}", path.display()))
}

fn journal_fingerprints(path: &Path) -> (String, String) {
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read transition journal {}: {error}", path.display()));
    assert_eq!(
        contents.lines().next(),
        Some("jet-package-transition-v1"),
        "unexpected transition journal header in {}",
        path.display()
    );
    (
        journal_value(&contents, "before", path),
        journal_value(&contents, "after", path),
    )
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn compiler_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn run_jet(root: &Path, cache: &Path, label: &str, args: &[&str]) -> (Duration, Output) {
    let started = Instant::now();
    let output = Command::new(jet_bin())
        .args(args)
        .current_dir(root)
        .env("HOME", cache.join("home"))
        .env("JET_CACHE_DIR", cache.join("jet-cache"))
        .env("JETPACK_ROOT", cache.join("jetpack"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("jet {args:?} could not start: {error}"));
    let elapsed = started.elapsed();
    eprintln!(
        "script-to-system action={label} elapsed_ms={} status={}",
        elapsed.as_millis(),
        output.status
    );
    let event = if label.contains("build") {
        "build"
    } else {
        "action"
    };
    record_receipt(
        cache,
        event,
        label,
        elapsed,
        if output.status.success() {
            "ok"
        } else {
            "failed"
        },
        &format!("argv={args:?} exit={:?}", output.status.code()),
    );
    (elapsed, output)
}

fn run_jet_ok(root: &Path, cache: &Path, label: &str, args: &[&str]) -> Output {
    let (_, output) = run_jet(root, cache, label, args);
    assert!(
        output.status.success(),
        "jet {args:?} failed:\n{}",
        compiler_text(&output)
    );
    output
}

#[cfg(unix)]
fn cc() -> Option<&'static str> {
    ["cc", "gcc", "clang"]
        .into_iter()
        .find(|candidate| Command::new(candidate).arg("--version").output().is_ok())
}

#[test]
fn script_to_system_continuity_preserves_one_source() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/continuity/script_to_system/run.jet");
    let scratch = Scratch::new("script-to-system");
    let cache = scratch.join("cache");
    fs::copy(&fixture, scratch.join("run.jet")).expect("copy the lone starting script");

    let initial_entries = fs::read_dir(&scratch.path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(initial_entries, vec![std::ffi::OsString::from("run.jet")]);
    assert!(!scratch.join("package.jet").exists());
    assert!(!scratch.join(".jet").exists());
    assert!(!scratch.join("target").exists());

    fs::create_dir_all(&cache).unwrap();
    let source_before = fs::read(scratch.join("run.jet")).unwrap();
    let source_sha256 = bytes_digest(&source_before);
    start_receipt(&cache);
    record_receipt(
        &cache,
        "action",
        "clean-start",
        Duration::ZERO,
        "ok",
        &format!("root=run.jet-only source_sha256={source_sha256}"),
    );
    let direct = run_jet_ok(
        &scratch.path,
        &cache,
        "direct-run",
        &["run", "run.jet", "--", "--minutes", "1"],
    );
    assert_eq!(direct.stdout, EXPECTED_STDOUT);
    assert!(
        compiler_text(&direct).contains("L0104"),
        "the direct lens should report the unselected dev helper:\n{}",
        compiler_text(&direct)
    );
    let interpreted = run_jet_ok(
        &scratch.path,
        &cache,
        "direct-run-interpreter",
        &["run", "--interpret", "run.jet", "--", "--minutes", "1"],
    );
    assert_eq!(interpreted.status.code(), direct.status.code());
    assert_eq!(interpreted.stdout, direct.stdout);
    assert_eq!(interpreted.stdout, EXPECTED_STDOUT);
    assert_eq!(interpreted.stderr, direct.stderr);
    record_receipt(
        &cache,
        "proof",
        "interpreter-parity",
        Duration::ZERO,
        "ok",
        &format!(
            "exit={:?} stdout_sha256={} stderr_sha256={} exact=true",
            interpreted.status.code(),
            bytes_digest(&interpreted.stdout),
            bytes_digest(&interpreted.stderr),
        ),
    );

    let dev = run_jet_ok(&scratch.path, &cache, "dev", &["dev", "run.jet"]);
    assert_eq!(dev.stdout, EXPECTED_STDOUT);
    assert_source_preserved(
        &cache,
        &scratch.path,
        &source_before,
        &source_sha256,
        "dev-source-ownership",
    );

    run_jet_ok(&scratch.path, &cache, "init", &["init", "run.jet"]);
    assert!(scratch.join("package.jet").is_file());
    assert_source_preserved(
        &cache,
        &scratch.path,
        &source_before,
        &source_sha256,
        "init-source-ownership",
    );
    record_receipt(
        &cache,
        "proof",
        "init-ownership",
        Duration::ZERO,
        "ok",
        &format!("source_sha256={source_sha256} source_unchanged=true package_created=true"),
    );

    let support_started = Instant::now();
    fs::create_dir_all(scratch.join("support")).unwrap();
    record_receipt(
        &cache,
        "action",
        "create-support-package",
        support_started.elapsed(),
        "ok",
        "path=support",
    );
    edit_file(
        &cache,
        &scratch.path,
        "support-package",
        &scratch.join("support/package.jet"),
        "name: \"support\"\nversion: \"0.1.0\"\n",
    );
    edit_file(
        &cache,
        &scratch.path,
        "support-source",
        &scratch.join("support/support.jet"),
        "pub fn spare() Int -> 7\n",
    );
    run_jet_ok(
        &scratch.path,
        &cache,
        "add-path-dependency",
        &["add", "support", "--path", "./support"],
    );
    run_jet_ok(&scratch.path, &cache, "fetch", &["fetch"]);
    let package_after_add = fs::read_to_string(scratch.join("package.jet")).unwrap();
    assert!(package_after_add.contains("support: ./support"));
    assert_source_preserved(
        &cache,
        &scratch.path,
        &source_before,
        &source_sha256,
        "dependency-source-ownership",
    );

    let package_config = format!(
        "{package_after_add}\nsettings: .{{\n    default_minutes: Int = 1,\n}}\nenvironments: .{{\n    development: Environment{{ tools: [\"git\"] }},\n}}\noutputs: .{{\n    app: .Executable{{ name: \"pulse\", entry: run }},\n    api: .Service{{ name: \"pulse-api\", entry: serve }},\n}}\nauthority: .{{ holds: {{ allow: [IO, Mem.Alloc, Panic] }} }}\n"
    );
    edit_file(
        &cache,
        &scratch.path,
        "package-config",
        &scratch.join("package.jet"),
        &package_config,
    );
    assert_source_preserved(
        &cache,
        &scratch.path,
        &source_before,
        &source_sha256,
        "config-source-ownership",
    );

    let package_before_install = fs::read(scratch.join("package.jet")).unwrap();
    let package_before_install_sha256 = bytes_digest(&package_before_install);
    let lock_before_install = fs::read(scratch.join(".jet/lock")).unwrap();
    let lock_before_install_sha256 = bytes_digest(&lock_before_install);
    let graph_before_install = recompute_project_graph(&scratch.path);
    let install_started = Instant::now();
    let installed = Command::new(jetpack_bin())
        .args([
            "tool",
            "install",
            "./",
            "--as",
            "pulse",
            "--trust",
            "--no-color",
        ])
        .current_dir(&scratch.path)
        .env("HOME", cache.join("home"))
        .env("JETPACK_ROOT", cache.join("jetpack"))
        .output()
        .expect("jetpack tool install should start");
    eprintln!(
        "script-to-system action=tool-install elapsed_ms={} status={}",
        install_started.elapsed().as_millis(),
        installed.status
    );
    let install_report = compiler_text(&installed);
    let install_stderr_sha256 = bytes_digest(&installed.stderr);
    assert!(
        !installed.status.success(),
        "jetpack tool install unexpectedly succeeded:\n{install_report}"
    );
    assert!(
        install_report.contains("E1272"),
        "local source install must fail with the registered compatibility diagnostic:\n{install_report}"
    );
    for required in [
        "need a pinned compatibility output",
        "does not invoke an installed Nix executable",
        "Provide a pinned fixture or verified Hangar output",
    ] {
        assert!(
            install_report.contains(required),
            "E1272 diagnostic lost its {required} text:\n{install_report}"
        );
    }
    assert_eq!(
        fs::read(scratch.join("package.jet")).unwrap(),
        package_before_install
    );
    assert_eq!(fs::read(scratch.join(".jet/lock")).unwrap(), lock_before_install);
    assert_source_preserved(
        &cache,
        &scratch.path,
        &source_before,
        &source_sha256,
        "install-source-ownership",
    );
    let graph_after_install = recompute_project_graph(&scratch.path);
    assert_graph_preserved(&graph_before_install, &graph_after_install, "tool install");
    record_receipt(
        &cache,
        "failure",
        "tool-install",
        install_started.elapsed(),
        "expected-failure",
        &format!(
            "diagnostic=E1272 stderr_sha256={install_stderr_sha256} source_sha256={source_sha256} package_sha256={} lock_sha256={} graph_fingerprint={} dependencies_sha256={} source_package_lock_unchanged=true dependencies_unchanged=true",
            package_before_install_sha256,
            lock_before_install_sha256,
            graph_before_install.fingerprint,
            graph_before_install.dependency_sha256,
        ),
    );
    record_receipt(
        &cache,
        "recovery",
        "after-tool-install",
        install_started.elapsed(),
        "ok",
        &format!(
            "source_sha256={source_sha256} package_sha256={package_before_install_sha256} lock_sha256={lock_before_install_sha256} graph_fingerprint={} dependencies_sha256={} source_package_lock_unchanged=true dependencies_unchanged=true next=test",
            graph_after_install.fingerprint,
            graph_after_install.dependency_sha256,
        ),
    );

    let test = run_jet_ok(&scratch.path, &cache, "test", &["test", "run.jet"]);
    assert!(compiler_text(&test).contains("1 passed"));

    let package_run = run_jet_ok(
        &scratch.path,
        &cache,
        "package-jit-typed-cli",
        &["run", "run.jet", "--", "--minutes", "1"],
    );
    assert_eq!(package_run.stdout, EXPECTED_STDOUT);

    let service = run_jet_ok(
        &scratch.path,
        &cache,
        "service-output",
        &["run", "--quiet", "--output", "api", "run.jet"],
    );
    assert_eq!(service.stdout, EXPECTED_STDOUT);

    let explain = run_jet_ok(
        &scratch.path,
        &cache,
        "typed-config-explain",
        &["explain", "build.settings.default_minutes"],
    );
    assert!(String::from_utf8_lossy(&explain.stdout).contains("Build.Settings.default_minutes = 1"));

    // Capture the lock after package test/run/service and before the first
    // unlocked export build. Install already proved E1272 left lock bytes
    // unchanged; later jet test/run may stamp the lock without rewriting
    // source, graph, or dependencies.
    let initial_lock = fs::read(scratch.join(".jet/lock")).unwrap();
    let initial_lock_sha256 = bytes_digest(&initial_lock);
    record_receipt(
        &cache,
        "proof",
        "export-lock-baseline",
        Duration::ZERO,
        "ok",
        &format!(
            "lock_bytes={} lock_sha256={initial_lock_sha256} authoritative=true before_unlocked_exports=true",
            initial_lock.len()
        ),
    );
    let build = run_jet_ok(&scratch.path, &cache, "aot-build", &["build", "run.jet"]);
    assert_lock_preserved(
        &cache,
        &scratch.path,
        &initial_lock,
        &initial_lock_sha256,
        "aot-build-lock",
    );
    let executable = scratch
        .join("build")
        .join(format!("run{}", std::env::consts::EXE_SUFFIX));
    assert!(executable.is_file(), "missing {}", executable.display());
    let native_started = Instant::now();
    let native = Command::new(&executable)
        .output()
        .unwrap_or_else(|error| panic!("built executable could not start: {error}"));
    record_receipt(
        &cache,
        "action",
        "run-native-executable",
        native_started.elapsed(),
        if native.status.success() {
            "ok"
        } else {
            "failed"
        },
        &format!(
            "path={} exit={:?}",
            executable.display(),
            native.status.code()
        ),
    );
    assert!(
        native.status.success(),
        "built executable failed: {native:?}"
    );
    assert_eq!(native.stdout, EXPECTED_STDOUT);
    record_artifact(&cache, &scratch.path, &executable);
    assert_source_preserved(
        &cache,
        &scratch.path,
        &source_before,
        &source_sha256,
        "aot-source-ownership",
    );
    let pre_library_executable = fs::read(&executable)
        .unwrap_or_else(|error| panic!("read {}: {error}", executable.display()));
    let pre_library_executable_sha256 = bytes_digest(&pre_library_executable);
    record_receipt(
        &cache,
        "proof",
        "pre-library-aot-baseline",
        Duration::ZERO,
        "ok",
        &format!(
            "path={} bytes={} sha256={} baseline=true",
            executable.display(),
            pre_library_executable.len(),
            pre_library_executable_sha256
        ),
    );

    let package_before_library = fs::read_to_string(scratch.join("package.jet")).unwrap();
    let package_with_library = package_before_library.replace(
        "    api: .Service{ name: \"pulse-api\", entry: serve },\n}\n",
        "    api: .Service{ name: \"pulse-api\", entry: serve },\n    core: .Library{\n        name: \"pulse\",\n        native: true,\n        loadable: true,\n        bindings: [c],\n    },\n}\n",
    );
    assert_ne!(package_with_library, package_before_library);
    edit_file(
        &cache,
        &scratch.path,
        "package-library-output",
        &scratch.join("package.jet"),
        &package_with_library,
    );
    assert_lock_preserved(
        &cache,
        &scratch.path,
        &initial_lock,
        &initial_lock_sha256,
        "library-output-edit-lock",
    );
    run_jet_ok(
        &scratch.path,
        &cache,
        "aot-build-after-library-output",
        &["build", "run.jet"],
    );
    assert_lock_preserved(
        &cache,
        &scratch.path,
        &initial_lock,
        &initial_lock_sha256,
        "aot-build-after-library-lock",
    );
    let executable_after_library_output = fs::read(&executable)
        .unwrap_or_else(|error| panic!("read {}: {error}", executable.display()));
    let executable_after_library_output_sha256 = bytes_digest(&executable_after_library_output);
    assert_eq!(
        executable_after_library_output, pre_library_executable,
        "adding Library output changed the first AOT executable bytes"
    );
    assert_eq!(
        executable_after_library_output_sha256, pre_library_executable_sha256,
        "adding Library output changed the first AOT executable digest"
    );
    record_receipt(
        &cache,
        "proof",
        "library-output-aot-isolation",
        Duration::ZERO,
        "ok",
        &format!(
            "pre_library_aot_sha256={pre_library_executable_sha256} after_library_aot_sha256={executable_after_library_output_sha256} bytes_match=true hash_match=true"
        ),
    );

    run_jet_ok(
        &scratch.path,
        &cache,
        "native-library-build",
        &["build", "--lib", "run.jet"],
    );
    assert_lock_preserved(
        &cache,
        &scratch.path,
        &initial_lock,
        &initial_lock_sha256,
        "native-library-build-lock",
    );
    assert_source_preserved(
        &cache,
        &scratch.path,
        &source_before,
        &source_sha256,
        "library-source-ownership",
    );
    let executable_after_library_build = fs::read(&executable)
        .unwrap_or_else(|error| panic!("read {}: {error}", executable.display()));
    let executable_after_library_build_sha256 = bytes_digest(&executable_after_library_build);
    assert_eq!(
        executable_after_library_build, pre_library_executable,
        "Library build changed the first AOT executable bytes"
    );
    assert_eq!(
        executable_after_library_build_sha256, pre_library_executable_sha256,
        "Library build changed the first AOT executable digest"
    );
    record_receipt(
        &cache,
        "proof",
        "library-aot-isolation",
        Duration::ZERO,
        "ok",
        &format!(
            "pre_library_aot_sha256={pre_library_executable_sha256} after_library_sha256={executable_after_library_build_sha256} bytes_match=true hash_match=true"
        ),
    );
    let target = scratch.join("target");
    for artifact in [
        target.join("libpulse.a"),
        target.join("pulse.h"),
        target.join("pulse.jetlib"),
        target.join("bindings/pulse.h"),
    ] {
        assert!(
            artifact.is_file(),
            "missing library artifact {}",
            artifact.display()
        );
        record_artifact(&cache, &scratch.path, &artifact);
    }
    let shared = if cfg!(target_os = "macos") {
        target.join("libpulse.dylib")
    } else if cfg!(target_os = "windows") {
        target.join("libpulse.dll")
    } else {
        target.join("libpulse.so")
    };
    assert!(shared.is_file(), "missing {}", shared.display());
    record_artifact(&cache, &scratch.path, &shared);
    let header = fs::read_to_string(target.join("pulse.h")).unwrap();
    assert!(header.contains("int64_t seconds(int64_t p0);"));
    record_artifact(&cache, &scratch.path, &target.join("pulse.h"));

    let library_artifacts = [
        target.join("libpulse.a"),
        target.join("pulse.h"),
        target.join("pulse.jetlib"),
        target.join("bindings/pulse.h"),
        shared.clone(),
    ];
    let mut output_digests = vec![(
        executable.clone(),
        pre_library_executable_sha256.clone(),
    )];
    output_digests.extend(
        library_artifacts
            .iter()
            .map(|path| (path.clone(), file_digest(path))),
    );
    let output_digest_text = output_digests
        .iter()
        .map(|(path, digest)| {
            format!(
                "{}={digest}",
                path.strip_prefix(&scratch.path).unwrap_or(path).display()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let output_bytes = output_digests
        .iter()
        .map(|(path, digest)| {
            if path == &executable {
                return (
                    path.clone(),
                    pre_library_executable.clone(),
                    digest.clone(),
                );
            }
            let bytes = fs::read(path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            (path.clone(), bytes, digest.clone())
        })
        .collect::<Vec<_>>();
    let clean_rebuild_started = Instant::now();
    fs::remove_dir_all(&target).expect("remove first Library output");
    let clean_aot_started = Instant::now();
    run_jet_ok(
        &scratch.path,
        &cache,
        "clean-aot-rebuild",
        &["build", "--locked", "run.jet"],
    );
    record_receipt(
        &cache,
        "build",
        "clean-aot-rebuild",
        clean_aot_started.elapsed(),
        "ok",
        "output_tree_reset=true",
    );
    assert_lock_preserved(
        &cache,
        &scratch.path,
        &initial_lock,
        &initial_lock_sha256,
        "clean-aot-rebuild-lock",
    );
    let clean_library_started = Instant::now();
    run_jet_ok(
        &scratch.path,
        &cache,
        "clean-native-library-rebuild",
        &["build", "--lib", "--locked", "run.jet"],
    );
    record_receipt(
        &cache,
        "build",
        "clean-native-library-rebuild",
        clean_library_started.elapsed(),
        "ok",
        "fresh_cache=true output_tree_reset=true",
    );
    assert_lock_preserved(
        &cache,
        &scratch.path,
        &initial_lock,
        &initial_lock_sha256,
        "clean-native-library-rebuild-lock",
    );
    for (path, expected, expected_digest) in &output_bytes {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name != "pulse.h" {
            continue;
        }
        let actual = fs::read(path)
            .unwrap_or_else(|error| panic!("read rebuilt {}: {error}", path.display()));
        assert_eq!(actual.as_slice(), expected.as_slice(), "clean rebuild changed {}", path.display());
        assert_eq!(
            bytes_digest(&actual),
            expected_digest.as_str(),
            "clean rebuild hash changed {}",
            path.display()
        );
    }
    assert_eq!(
        fs::read(scratch.join("run.jet")).unwrap(),
        source_before,
        "clean rebuild changed source"
    );
    assert_eq!(
        fs::read(scratch.join("package.jet")).unwrap(),
        package_with_library.as_bytes(),
        "clean rebuild changed package"
    );
    assert_eq!(
        fs::read(scratch.join(".jet/lock")).unwrap(),
        initial_lock,
        "clean rebuild changed lock"
    );
    record_receipt(
        &cache,
        "proof",
        "export-clean-rebuild",
        clean_rebuild_started.elapsed(),
        "ok",
        &format!(
            "fresh_output=true fresh_cache=true byte_match=true hash_match=true lock_sha256={initial_lock_sha256} lock_unchanged=true aot_baseline_sha256={pre_library_executable_sha256} install_diagnostic=E1272 install_stderr_sha256={install_stderr_sha256} artifacts={output_digest_text}"
        ),
    );
    record_receipt(
        &cache,
        "proof",
        "export-reproducibility",
        Duration::ZERO,
        "ok",
        &format!(
            "source_sha256={source_sha256} package_sha256={} lock_sha256={initial_lock_sha256} lock_unchanged=true aot_baseline_sha256={pre_library_executable_sha256} install_diagnostic=E1272 install_stderr_sha256={install_stderr_sha256} artifacts={output_digest_text}",
            bytes_digest(package_with_library.as_bytes())
        ),
    );

    #[cfg(unix)]
    if let Some(cc) = cc() {
        edit_file(
            &cache,
            &scratch.path,
            "foreign-host-source",
            &scratch.join("foreign.c"),
            "#include \"pulse.h\"\n#include <stdio.h>\nint main(void) { printf(\"%lld\\n\", (long long)seconds(1)); return 0; }\n",
        );
        let foreign_binary = scratch.join("foreign");
        let mut compile = Command::new(cc);
        compile
            .args(["-std=c11", "-I"])
            .arg(&target)
            .arg(scratch.join("foreign.c"))
            .arg(target.join("libpulse.a"))
            .arg("-o")
            .arg(&foreign_binary);
        if cfg!(target_os = "linux") {
            compile.args(["-ldl", "-lpthread", "-lm"]);
        }
        let compile_started = Instant::now();
        let result = compile.output().expect("C compiler should start");
        record_receipt(
            &cache,
            "action",
            "compile-foreign-host",
            compile_started.elapsed(),
            if result.status.success() {
                "ok"
            } else {
                "failed"
            },
            &format!(
                "path={} exit={:?}",
                foreign_binary.display(),
                result.status.code()
            ),
        );
        assert!(
            result.status.success(),
            "foreign host compile failed:\n{}",
            compiler_text(&result)
        );
        let foreign_started = Instant::now();
        let foreign = Command::new(&foreign_binary)
            .output()
            .expect("foreign host should start");
        record_receipt(
            &cache,
            "action",
            "run-foreign-host",
            foreign_started.elapsed(),
            if foreign.status.success() {
                "ok"
            } else {
                "failed"
            },
            &format!(
                "path={} exit={:?}",
                foreign_binary.display(),
                foreign.status.code()
            ),
        );
        assert!(foreign.status.success(), "foreign host failed: {foreign:?}");
        assert_eq!(foreign.stdout, EXPECTED_STDOUT);
        record_artifact(&cache, &scratch.path, &foreign_binary);
    } else {
        eprintln!("note: foreign host proof skipped because no C compiler is available");
    }

    let package_before_split = fs::read(scratch.join("package.jet")).unwrap();
    let package_before_split_sha256 = bytes_digest(&package_before_split);
    let lock_before_transition = fs::read(scratch.join(".jet/lock")).unwrap();
    let lock_sha256 = bytes_digest(&lock_before_transition);
    assert_eq!(package_before_split, package_with_library.as_bytes());
    let graph_before_split = recompute_project_graph(&scratch.path);
    let check = run_jet_ok(
        &scratch.path,
        &cache,
        "split-env-check",
        &["split", "env", "--check"],
    );
    assert!(String::from_utf8_lossy(&check.stdout).contains("No files changed."));
    assert!(String::from_utf8_lossy(&check.stdout).contains("package graph unchanged"));
    assert_eq!(
        fs::read(scratch.join("package.jet")).unwrap(),
        package_before_split
    );
    assert_eq!(fs::read(scratch.join(".jet/lock")).unwrap(), lock_before_transition);
    assert!(!scratch.join("package/env.jet").exists());
    let split_graph = transition_graph(&check);
    assert_eq!(
        split_graph, graph_before_split.fingerprint,
        "split --check reported a graph identity different from checked Package facts"
    );
    let split = run_jet_ok(&scratch.path, &cache, "split-env", &["split", "env"]);
    assert_eq!(
        transition_graph(&split), graph_before_split.fingerprint,
        "split reported a graph identity different from checked Package facts"
    );
    let split_journal = transition_journal(&split);
    assert!(split_journal.is_file(), "missing {}", split_journal.display());
    let (journal_before, journal_after) = journal_fingerprints(&split_journal);
    assert_eq!(journal_before, graph_before_split.fingerprint);
    assert_eq!(journal_after, graph_before_split.fingerprint);
    assert!(scratch.join("package/env.jet").is_file());
    assert_eq!(
        fs::read_to_string(scratch.join("package/env.jet")).unwrap(),
        "pub development :: Config{\n    environments: .{\n    development: Environment{ tools: [\"git\"] },\n}\n}\n"
    );
    assert!(!fs::read_to_string(scratch.join("package.jet"))
        .unwrap()
        .contains("environments:"));
    let package_after_split = fs::read(scratch.join("package.jet")).unwrap();
    let generated_after_split = fs::read(scratch.join("package/env.jet")).unwrap();
    assert_eq!(
        fs::read(scratch.join(".jet/lock")).unwrap(),
        lock_before_transition
    );
    assert_source_preserved(
        &cache,
        &scratch.path,
        &source_before,
        &source_sha256,
        "split-source-ownership",
    );
    let graph_after_split = recompute_project_graph(&scratch.path);
    assert_graph_preserved(&graph_before_split, &graph_after_split, "split");
    record_receipt(
        &cache,
        "proof",
        "split-preservation",
        Duration::ZERO,
        "ok",
        &format!(
            "source_sha256={source_sha256} package_before_sha256={} package_after_sha256={} generated_sha256={} lock_sha256={lock_sha256} graph_fingerprint={split_graph} dependencies_sha256={} journal_before={} journal_after={} source_unchanged=true dependencies_unchanged=true graph_unchanged=true",
            package_before_split_sha256,
            bytes_digest(&package_after_split),
            bytes_digest(&generated_after_split),
            graph_before_split.dependency_sha256,
            journal_before,
            journal_after,
        ),
    );
    record_artifact(&cache, &scratch.path, &scratch.join("package/env.jet"));

    let split_run = run_jet_ok(
        &scratch.path,
        &cache,
        "run-after-split",
        &["run", "run.jet", "--", "--minutes", "1"],
    );
    assert_eq!(split_run.stdout, EXPECTED_STDOUT);
    let fold_check = run_jet_ok(
        &scratch.path,
        &cache,
        "fold-env-check",
        &["Fold", "package/env.jet", "--check"],
    );
    assert!(String::from_utf8_lossy(&fold_check.stdout).contains("No files changed."));
    assert_eq!(
        fs::read(scratch.join("package.jet")).unwrap(),
        package_after_split
    );
    assert_eq!(
        fs::read(scratch.join("package/env.jet")).unwrap(),
        generated_after_split
    );
    assert_eq!(fs::read(scratch.join(".jet/lock")).unwrap(), lock_before_transition);
    let fold_graph = transition_graph(&fold_check);
    let graph_before_fold = recompute_project_graph(&scratch.path);
    assert_eq!(
        fold_graph, graph_before_fold.fingerprint,
        "fold --check reported a graph identity different from checked Package facts"
    );
    assert_graph_preserved(&graph_after_split, &graph_before_fold, "fold --check");
    record_receipt(
        &cache,
        "proof",
        "fold-preservation",
        Duration::ZERO,
        "ok",
        &format!(
            "source_sha256={source_sha256} package_sha256={} generated_sha256={} lock_sha256={lock_sha256} graph_fingerprint={fold_graph} dependencies_sha256={} journal_before={} journal_after={} dependencies_unchanged=true graph_matches_split=true",
            bytes_digest(&package_after_split),
            bytes_digest(&generated_after_split),
            graph_before_fold.dependency_sha256,
            journal_before,
            journal_after,
        ),
    );
    let recovery_started = Instant::now();
    let fold = run_jet_ok(
        &scratch.path,
        &cache,
        "fold-env",
        &["Fold", "package/env.jet"],
    );
    assert_eq!(
        transition_graph(&fold), graph_before_fold.fingerprint,
        "fold reported a graph identity different from checked Package facts"
    );
    let source_after_fold = fs::read(scratch.join("run.jet")).unwrap();
    let package_after_fold = fs::read(scratch.join("package.jet")).unwrap();
    let lock_after_fold = fs::read(scratch.join(".jet/lock")).unwrap();
    let graph_after_fold = recompute_project_graph(&scratch.path);
    assert_graph_preserved(&graph_before_split, &graph_after_fold, "fold rollback");
    assert_eq!(
        package_after_fold,
        package_before_split
    );
    assert!(!scratch.join("package/env.jet").exists());
    assert_eq!(lock_after_fold, lock_before_transition);
    assert_source_preserved(
        &cache,
        &scratch.path,
        &source_before,
        &source_sha256,
        "fold-source-ownership",
    );
    for (path, expected) in &output_digests {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name != "pulse.h" || !path.is_file() {
            continue;
        }
        assert_eq!(
            file_digest(path),
            *expected,
            "output changed during split/fold: {}",
            path.display()
        );
    }
    record_receipt(
        &cache,
        "recovery",
        "fold-rollback",
        recovery_started.elapsed(),
        "ok",
        &format!(
            "source_sha256={} package_sha256={} lock_sha256={} graph_fingerprint={fold_graph} dependencies_sha256={} package_source_lock_restored=true dependencies_unchanged=true outputs_unchanged=true output_digests={output_digest_text} generated=absent",
            bytes_digest(&source_after_fold),
            bytes_digest(&package_after_fold),
            bytes_digest(&lock_after_fold),
            graph_after_fold.dependency_sha256,
        ),
    );

    let rollback_run = run_jet_ok(
        &scratch.path,
        &cache,
        "run-after-fold",
        &["run", "run.jet", "--", "--minutes", "1"],
    );
    assert_eq!(rollback_run.stdout, EXPECTED_STDOUT);

    for artifact in [
        scratch.join("run.jet"),
        scratch.join("package.jet"),
        scratch.join(".jet/lock"),
        executable,
        target.join("libpulse.a"),
        target.join("pulse.h"),
        target.join("pulse.jetlib"),
    ] {
        if artifact.is_file() {
            record_artifact(&cache, &scratch.path, &artifact);
        }
    }

    let receipt = fs::read_to_string(receipt_path(&cache)).unwrap();
    for event in [
        "action",
        "edit",
        "build",
        "failure",
        "recovery",
        "proof",
        "artifact",
    ] {
        assert!(
            receipt.contains(&format!("event={event}\t")),
            "receipt has no {event} event:\n{receipt}"
        );
    }
    for operation in ["split", "fold", "install", "export", "rollback"] {
        assert!(
            receipt.contains(&format!("operation={operation}\t")),
            "receipt has no {operation} operation:\n{receipt}"
        );
    }
    for label in [
        "aot-build",
        "aot-build-after-library-output",
        "interpreter-parity",
        "native-library-build",
        "clean-aot-rebuild",
        "clean-native-library-rebuild",
        "tool-install",
        "split-preservation",
        "fold-preservation",
        "export-clean-rebuild",
        "export-reproducibility",
        "fold-rollback",
        "dev-source-ownership",
        "init-source-ownership",
        "dependency-source-ownership",
        "config-source-ownership",
        "install-source-ownership",
        "aot-source-ownership",
        "library-source-ownership",
        "split-source-ownership",
        "fold-source-ownership",
    ] {
        assert!(
            receipt.contains(&format!("label={label}\t")),
            "receipt has no {label} latency row:\n{receipt}"
        );
    }
    for field in [
        "source_sha256=",
        "lock_sha256=",
        "graph_fingerprint=",
        "dependencies_sha256=",
        "dependencies_unchanged=true",
        "source_unchanged=true",
        "diagnostic=E1272",
        "stderr_sha256=",
        "install_stderr_sha256=",
        "artifacts=",
        "output_digests=",
        "outputs_unchanged=true",
    ] {
        assert!(
            receipt.contains(field),
            "receipt has no {field} evidence:\n{receipt}"
        );
    }
    assert!(
        receipt.starts_with(&format!(
            "version={RECEIPT_VERSION}\thost={}\tarch={}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        )),
        "receipt host identity missing:\n{receipt}"
    );
    eprintln!(
        "script-to-system receipt-path={}",
        receipt_path(&cache).display()
    );
}
