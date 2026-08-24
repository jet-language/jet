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

fn receipt_path(cache: &Path) -> PathBuf {
    cache.join(RECEIPT_NAME)
}

fn start_receipt(cache: &Path) {
    fs::write(
        receipt_path(cache),
        format!(
            "version=1\thost={}\tarch={}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    )
    .expect("create the script-to-system receipt");
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
        "event={event}\tlabel={label}\telapsed_ms={}\tstatus={status}\tdetail={detail}\n",
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
    let metadata =
        fs::metadata(path).unwrap_or_else(|error| panic!("stat {}: {error}", path.display()));
    let relative = path.strip_prefix(root).unwrap_or(path);
    record_receipt(
        cache,
        "artifact",
        "final-output",
        started.elapsed(),
        "ok",
        &format!("path={} bytes={}", relative.display(), metadata.len()),
    );
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
    start_receipt(&cache);
    record_receipt(
        &cache,
        "action",
        "clean-start",
        Duration::ZERO,
        "ok",
        "root=run.jet-only",
    );
    let source_before = fs::read(scratch.join("run.jet")).unwrap();
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

    let dev = run_jet_ok(&scratch.path, &cache, "dev", &["dev", "run.jet"]);
    assert_eq!(dev.stdout, EXPECTED_STDOUT);

    run_jet_ok(&scratch.path, &cache, "init", &["init", "run.jet"]);
    assert!(scratch.join("package.jet").is_file());
    assert_eq!(fs::read(scratch.join("run.jet")).unwrap(), source_before);

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

    let package_config = format!(
        "{package_after_add}\nsettings: .{{\n    default_minutes: Int = 1,\n}}\nenvironments: .{{\n    development: Environment{{ tools: [\"git\"] }},\n}}\noutputs: .{{\n    app: .Executable{{ name: \"pulse\", entry: run }},\n    api: .Service{{ name: \"pulse-api\", entry: serve }},\n}}\n"
    );
    edit_file(
        &cache,
        &scratch.path,
        "package-config",
        &scratch.join("package.jet"),
        &package_config,
    );

    let package_before_install = fs::read(scratch.join("package.jet")).unwrap();
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
    record_receipt(
        &cache,
        "failure",
        "tool-install",
        install_started.elapsed(),
        "expected-failure",
        "code=E1272 package-unchanged=true",
    );
    assert!(
        !installed.status.success(),
        "jetpack tool install unexpectedly succeeded:\n{}",
        compiler_text(&installed)
    );
    assert!(
        compiler_text(&installed).contains("E1272"),
        "local source install must fail with the registered compatibility diagnostic:\n{}",
        compiler_text(&installed)
    );
    assert_eq!(
        fs::read(scratch.join("package.jet")).unwrap(),
        package_before_install
    );
    record_receipt(
        &cache,
        "recovery",
        "after-tool-install",
        install_started.elapsed(),
        "ok",
        "package-unchanged=true next=test",
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

    let build = run_jet_ok(&scratch.path, &cache, "aot-build", &["build", "run.jet"]);
    assert!(compiler_text(&build).contains("built:"));
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

    run_jet_ok(
        &scratch.path,
        &cache,
        "native-library-build",
        &["build", "--lib", "run.jet"],
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
    let lock_before_transition = fs::read(scratch.join(".jet/lock")).unwrap();
    let check = run_jet_ok(
        &scratch.path,
        &cache,
        "split-env-check",
        &["split", "env", "--check"],
    );
    assert!(String::from_utf8_lossy(&check.stdout).contains("No files changed."));
    assert!(String::from_utf8_lossy(&check.stdout).contains("package graph unchanged"));
    run_jet_ok(&scratch.path, &cache, "split-env", &["split", "env"]);
    assert!(scratch.join("package/env.jet").is_file());
    assert_eq!(
        fs::read_to_string(scratch.join("package/env.jet")).unwrap(),
        "pub development :: Config{\n    environments: .{\n    development: Environment{ tools: [\"git\"] },\n}\n}\n"
    );
    assert!(!fs::read_to_string(scratch.join("package.jet"))
        .unwrap()
        .contains("environments:"));
    assert_eq!(
        fs::read(scratch.join(".jet/lock")).unwrap(),
        lock_before_transition
    );
    record_artifact(&cache, &scratch.path, &scratch.join("package/env.jet"));

    let split_run = run_jet_ok(
        &scratch.path,
        &cache,
        "run-after-split",
        &["run", "run.jet", "--", "--minutes", "1"],
    );
    assert_eq!(split_run.stdout, EXPECTED_STDOUT);
    run_jet_ok(
        &scratch.path,
        &cache,
        "fold-env",
        &["fold", "package/env.jet"],
    );
    let recovery_started = Instant::now();
    assert_eq!(
        fs::read(scratch.join("package.jet")).unwrap(),
        package_before_split
    );
    assert!(!scratch.join("package/env.jet").exists());
    assert_eq!(
        fs::read(scratch.join(".jet/lock")).unwrap(),
        lock_before_transition
    );
    assert_eq!(fs::read(scratch.join("run.jet")).unwrap(), source_before);
    record_receipt(
        &cache,
        "recovery",
        "fold-rollback",
        recovery_started.elapsed(),
        "ok",
        "package-source-lock-restored=true",
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
        record_artifact(&cache, &scratch.path, &artifact);
    }

    let receipt = fs::read_to_string(receipt_path(&cache)).unwrap();
    for event in ["action", "edit", "build", "failure", "recovery", "artifact"] {
        assert!(
            receipt.contains(&format!("event={event}\t")),
            "receipt has no {event} event:\n{receipt}"
        );
    }
    for label in ["aot-build", "native-library-build"] {
        assert!(
            receipt.contains(&format!("label={label}\t")),
            "receipt has no {label} latency row:\n{receipt}"
        );
    }
    assert!(
        receipt.starts_with(&format!(
            "version=1\thost={}\tarch={}\n",
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
