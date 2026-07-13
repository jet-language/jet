//! Flagship vertical-slice harness for `examples/apps/*`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

mod common;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn jet_command_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn run_jet(args: &[&str]) -> std::process::Output {
    let _guard = jet_command_lock().lock().unwrap();
    Command::new(jet_bin())
        .env("JETPLAY_HEADLESS", "1")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run jet {:?}: {e}", args))
}

fn run_jet_in(cwd: &Path, args: &[&str]) -> std::process::Output {
    let _guard = jet_command_lock().lock().unwrap();
    Command::new(jet_bin())
        .env("JETPLAY_HEADLESS", "1")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("failed to run jet {:?} in {}: {e}", args, cwd.display()))
}

fn assert_success(args: &[&str]) -> String {
    let out = run_jet(args);
    assert!(
        out.status.success(),
        "jet {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn assert_success_in(cwd: &Path, args: &[&str]) -> String {
    let out = run_jet_in(cwd, args);
    assert!(
        out.status.success(),
        "jet {:?} failed in {}\nstdout:\n{}\nstderr:\n{}",
        args,
        cwd.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn assert_expected(app: &str) {
    let main = format!("examples/apps/{app}/main.jet");
    let expected = fs::read_to_string(format!("examples/apps/{app}/expected/run.out")).unwrap();
    let actual = assert_success(&["run", &main]);
    assert_eq!(actual, expected, "{app} stdout drifted");
    let test_out = assert_success(&["test", &main]);
    assert!(
        test_out.contains("passed") || test_out.contains("ok"),
        "{app} test output should report success, got:\n{test_out}"
    );
}

fn assert_failure(args: &[&str], needle: &str) {
    let out = run_jet(args);
    assert!(
        !out.status.success(),
        "jet {:?} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains(needle),
        "jet {:?} missing `{needle}`\noutput:\n{combined}",
        args
    );
}

fn assert_manifest(app: &str) {
    let dir = Path::new("examples/apps").join(app);
    assert!(dir.join("pkg.jet").is_file(), "{app} missing pkg.jet");
    assert!(dir.join("README.md").is_file(), "{app} missing README.md");
    assert!(
        dir.join("expected/run.out").is_file(),
        "{app} missing golden"
    );
}

fn app_names() -> [&'static str; 5] {
    ["jetgrep", "jetpaste", "metal", "jettasks", "jetfighter"]
}

fn slice_app_names() -> [&'static str; 4] {
    ["jetgrep", "jetpaste", "metal", "jettasks"]
}

fn copy_dir_all(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir_all(&src, &dst);
        } else {
            fs::copy(&src, &dst).unwrap();
        }
    }
}

#[test]
fn app_slices_run_tests_and_match_goldens() {
    for app in app_names() {
        assert_manifest(app);
        assert_expected(app);
    }
}

#[test]
fn app_capstone_claims_are_gated() {
    let proof = fs::read_to_string("examples/apps/capstone-proof.md")
        .expect("examples/apps/capstone-proof.md missing");
    for gate in [
        "Standalone run path",
        "Real workflow",
        "Headless tests",
        "UI/browser tests",
        "Packaging/deploy story",
        "Perf budget",
        "LOC comparison",
        "No facade",
    ] {
        assert!(proof.contains(gate), "capstone proof gate missing `{gate}`");
    }
    for app in slice_app_names() {
        let row = format!("| `{app}` | slice |");
        assert!(
            proof.contains(&row),
            "capstone matrix missing slice row for {app}"
        );
        let readme = fs::read_to_string(format!("examples/apps/{app}/README.md"))
            .unwrap_or_else(|e| panic!("{app} README missing: {e}"));
        assert!(
            readme.contains("slice"),
            "{app} README must honestly classify the current app as a slice"
        );
        assert!(
            !readme.to_ascii_lowercase().contains("capstone"),
            "{app} README must not claim capstone status before proof gate passes"
        );
    }

    assert!(
        proof.contains("| `jetfighter` / `JetPlay` | capstone |"),
        "JetPlay capstone row missing"
    );
    let readme =
        fs::read_to_string("examples/apps/jetfighter/README.md").expect("jetfighter README");
    assert!(
        readme.contains("Product capstone")
            && readme.contains("source-backed editor")
            && readme.contains("workbench_ui.jet"),
        "jetfighter README must name capstone, source-backed editor, and UI proof"
    );
}

#[test]
fn jetplay_capstone_source_editor_package_and_ui_proof() {
    assert_manifest("jetfighter");
    for path in [
        "examples/apps/jetfighter/level.jet",
        "examples/apps/jetfighter/workbench.jet",
        "examples/apps/jetfighter/workbench_ui.jet",
        "examples/apps/jetfighter/proof/perf-baseline.md",
        "examples/apps/jetfighter/proof/loc-comparison.md",
    ] {
        assert!(
            Path::new(path).is_file(),
            "missing JetPlay proof file {path}"
        );
    }

    let native = run_jet(&["build", "examples/apps/jetfighter/main.jet"]);
    assert!(
        native.status.success(),
        "JetPlay native build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&native.stdout),
        String::from_utf8_lossy(&native.stderr)
    );

    let web = run_jet(&[
        "build",
        "--target=web",
        "examples/apps/jetfighter/workbench_ui.jet",
    ]);
    assert!(
        web.status.success(),
        "JetPlay web editor build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&web.stdout),
        String::from_utf8_lossy(&web.stderr)
    );

    let tmp = common::unique_tmp("jetplay_capstone");
    let app_root = tmp.join("jetfighter");
    copy_dir_all(Path::new("examples/apps/jetfighter"), &app_root);
    let app_root_str = app_root.to_string_lossy().into_owned();

    let editor = assert_success(&[
        "run",
        "examples/apps/jetfighter/workbench.jet",
        "--",
        &app_root_str,
        "2",
        "0",
    ]);
    assert!(editor.contains("workflow source-backed"));
    assert!(editor.contains("ui fill("));
    assert!(editor.contains("ui text("));

    let edited_level = fs::read_to_string(app_root.join("level.jet")).unwrap();
    assert!(edited_level.contains("nebula-edited"));
    assert!(edited_level.contains("return 0"));

    let edited_main = app_root.join("main.jet").to_string_lossy().into_owned();
    let rerun = assert_success_in(&tmp, &["run", &edited_main]);
    assert!(rerun.contains("level nebula-edited asteroid=(2,0)"));
    assert!(rerun.contains("frame 1 ship=(2,0) score=250"));
    assert!(rerun.contains("render ship=(2,1) asteroid=(2,0)"));

    let perf = fs::read_to_string("examples/apps/jetfighter/proof/perf-baseline.md").unwrap();
    assert!(perf.contains("16 ms") && perf.contains("128 MB") && perf.contains("Draw calls"));

    let loc = fs::read_to_string("examples/apps/jetfighter/proof/loc-comparison.md").unwrap();
    for competitor in [
        "Godot",
        "Bevy",
        "Love2D",
        "Unity",
        "Raylib C",
        "Raylib Zig",
        "Odin + raylib",
    ] {
        assert!(loc.contains(competitor), "LOC proof missing {competitor}");
    }
    for proof in [
        "Product Proof Matrix",
        "Clarity",
        "Safety",
        "Deterministic tests",
        "Packaging/deploy",
        "Perf proof",
        "jet build main.jet",
        "jet build --target=web workbench_ui.jet",
    ] {
        assert!(loc.contains(proof), "JetPlay proof missing {proof}");
    }
}

#[test]
fn jetgrep_reports_cli_errors() {
    assert_failure(
        &[
            "run",
            "examples/apps/jetgrep/main.jet",
            "[",
            "examples/apps/jetgrep/fixtures/api.log",
        ],
        "jetgrep: invalid regex [",
    );
    assert_failure(
        &[
            "run",
            "examples/apps/jetgrep/main.jet",
            "error",
            "examples/apps/jetgrep/fixtures/missing.txt",
        ],
        "jetgrep: missing file examples/apps/jetgrep/fixtures/missing.txt",
    );
}

#[test]
fn jetgrep_cli_modes_are_pinned() {
    let count = assert_success(&[
        "run",
        "examples/apps/jetgrep/main.jet",
        "--",
        "--count",
        "warning",
        "examples/apps/jetgrep/fixtures",
    ]);
    assert_eq!(
        count,
        concat!(
            "jetgrep pattern=warning\n",
            "examples/apps/jetgrep/fixtures/api.log:1\n",
            "examples/apps/jetgrep/fixtures/notes.txt:1\n",
            "matches=2\n",
        )
    );

    let files = assert_success(&[
        "run",
        "examples/apps/jetgrep/main.jet",
        "--",
        "--files",
        "TODO",
        "examples/apps/jetgrep/fixtures",
    ]);
    assert_eq!(
        files,
        concat!(
            "jetgrep pattern=TODO\n",
            "examples/apps/jetgrep/fixtures/nested/deploy.log\n",
            "examples/apps/jetgrep/fixtures/notes.txt\n",
            "matches=2\n",
        )
    );

    let ignored = assert_success(&[
        "run",
        "examples/apps/jetgrep/main.jet",
        "--",
        "--ignore",
        "nested",
        "TODO",
        "examples/apps/jetgrep/fixtures",
    ]);
    assert_eq!(
        ignored,
        concat!(
            "jetgrep pattern=TODO\n",
            "examples/apps/jetgrep/fixtures/notes.txt:2: TODO add benchmark corpus once owner greenlights docs/build gates\n",
            "matches=1\n",
        )
    );
}

#[test]
fn metal_freestanding_builds() {
    let out = run_jet(&[
        "build",
        "--freestanding",
        "examples/apps/metal/main.jet",
    ]);
    assert!(
        out.status.success(),
        "metal freestanding build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn jettasks_web_builds() {
    let out = run_jet(&["build", "--target=web", "examples/apps/jettasks/main.jet"]);
    assert!(
        out.status.success(),
        "jettasks web build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
