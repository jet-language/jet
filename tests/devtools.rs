//! c450 (D-DEVTOOLS1=A) — the maintainer-facing `jet self devtools` subcommands:
//! `reduce`, `ice-report`, `new-example`, `new-ui`, `check-fixture-paths`,
//! `bless`. All hidden behind the existing `devtools` namespace (never a
//! top-level command); one test per tool.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

mod common;

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A private cwd for a subprocess, so parallel tests never race on shared
/// output paths (same reasoning as `tests/cli.rs`'s `isolated_cwd`).
fn isolated_cwd(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_devtools_cwd_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `new-example`/`new-ui` scaffold into fixed, real repo paths by design (no
/// override flag — that's the point of the command), so their tests can't
/// redirect output to a tempdir; they clean up the files they created
/// instead. A bare best-effort remove at the end of the test body only runs
/// on the success path — any assertion panic in between (front end
/// rejects the scaffold, rustc rejects it, output mismatch, …) leaves the
/// scaffold committed to disk under `examples/` or `tests/ui/`, where a
/// later broad `git add -A` can commit it permanently. This guard removes
/// its paths on drop, panicking or not.
struct ScaffoldCleanup(Vec<PathBuf>);

impl Drop for ScaffoldCleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

// ── reduce ─────────────────────────────────────────────────────────

/// `--code EXXXX` oracle: shrink a file with unrelated helper functions down
/// to the minimal case that still emits E0107 (undefined name).
#[test]
fn devtools_reduce_shrinks_to_minimal_e0107_repro() {
    let dir = isolated_cwd("reduce");
    let src_path = dir.join("repro.jet");
    fs::write(
        &src_path,
        "fn helper_a() {\n    print(\"a\")\n}\n\n\
         fn helper_b() {\n    print(\"b\")\n}\n\n\
         fn run() {\n    helper_a()\n    helper_b()\n    \
         print(nonexistent_symbol_for_reduce_test)\n}\n",
    )
    .unwrap();

    let out = Command::new(jet())
        .args(["self", "devtools", "reduce"])
        .arg(&src_path)
        .args(["--code", "E0107"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "reduce should succeed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let reduced_path = dir.join("repro.reduced.jet");
    let reduced = fs::read_to_string(&reduced_path).unwrap_or_else(|_| {
        panic!(
            "missing {}; reduce stdout was:\n{}",
            reduced_path.display(),
            String::from_utf8_lossy(&out.stdout)
        )
    });

    // Shrunk away both unused helper functions; kept the line that trips E0107.
    assert!(
        !reduced.contains("helper_a") && !reduced.contains("helper_b"),
        "reduce didn't shrink away the unused helpers:\n{}",
        reduced
    );
    assert!(
        reduced.contains("nonexistent_symbol_for_reduce_test"),
        "reduce shrunk away the line that reproduces E0107:\n{}",
        reduced
    );
    assert!(
        reduced.lines().count() <= 3,
        "expected reduce to shrink to <= 3 lines, got:\n{}",
        reduced
    );

    // The shrunk case must still actually reproduce the oracle.
    let recheck = Command::new(jet())
        .arg("check")
        .arg(&reduced_path)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&recheck.stderr);
    assert!(
        stderr.contains("E0107"),
        "reduced case no longer reproduces E0107:\n{}",
        stderr
    );

    let _ = fs::remove_dir_all(&dir);
}

/// An input that doesn't reproduce the requested oracle is a usage error, not
/// a silent no-op.
#[test]
fn devtools_reduce_rejects_non_reproducing_input() {
    let dir = isolated_cwd("reduce_clean");
    let src_path = dir.join("clean.jet");
    fs::write(&src_path, "fn run() {\n    print(\"hello\")\n}\n").unwrap();

    let out = Command::new(jet())
        .args(["self", "devtools", "reduce"])
        .arg(&src_path)
        .args(["--code", "E0107"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "reduce should refuse a file that never reproduces the oracle"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("doesn't reproduce"),
        "expected an oracle-mismatch message, got:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let _ = fs::remove_dir_all(&dir);
}

// ── ice-report ─────────────────────────────────────────────────────

#[test]
fn devtools_ice_report_bundles_source_rust_and_versions() {
    let dir = isolated_cwd("ice_report");
    let src_path = dir.join("prog.jet");
    fs::write(&src_path, "fn run() {\n    print(\"hi\")\n}\n").unwrap();

    let out = Command::new(jet())
        .current_dir(&dir)
        .args(["self", "devtools", "ice-report"])
        .arg(&src_path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "ice-report should succeed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let bundle_line = stdout
        .lines()
        .find_map(|l| l.strip_prefix("wrote ICE report bundle to "))
        .unwrap_or_else(|| panic!("no bundle-path line in stdout:\n{}", stdout));
    // The command printed a path relative to its own cwd (`dir`).
    let bundle_dir = dir.join(bundle_line);
    assert!(
        bundle_dir.is_dir(),
        "bundle dir `{}` (from `{}`) doesn't exist",
        bundle_dir.display(),
        bundle_line
    );

    for name in ["source.jet", "generated.rs", "rustc.stderr", "versions.txt"] {
        assert!(
            bundle_dir.join(name).is_file(),
            "ice-report bundle missing `{}` in {}",
            name,
            bundle_dir.display()
        );
    }
    let versions = fs::read_to_string(bundle_dir.join("versions.txt")).unwrap();
    assert!(versions.contains("jet "), "versions.txt: {}", versions);
    let source = fs::read_to_string(bundle_dir.join("source.jet")).unwrap();
    assert!(source.contains("fn run()"));

    let _ = fs::remove_dir_all(&dir);
}

// ── new-example ────────────────────────────────────────────────────

/// Scaffolds a real example + expected pair matching `tests/golden.rs`'s
/// layout exactly: `examples/features/<topic>/<name>.jet` +
/// `examples/features/expected/<topic>/<name>.out`. `new-example` writes into
/// fixed repo-relative paths (no override flag), so this test runs at the
/// real repo root and removes what it created afterward.
#[test]
fn devtools_new_example_scaffolds_a_passing_golden_pair() {
    let root = repo_root();
    let topic = "tooling";
    let name = format!(
        "devtools_test_scaffold_{}_{}",
        std::process::id(),
        line!()
    );
    let example_path = root
        .join("examples/features")
        .join(topic)
        .join(format!("{}.jet", name));
    let expected_path = root
        .join("examples/features/expected")
        .join(topic)
        .join(format!("{}.out", name));
    let _ = fs::remove_file(&example_path);
    let _ = fs::remove_file(&expected_path);
    let _cleanup = ScaffoldCleanup(vec![example_path.clone(), expected_path.clone()]);

    let out = Command::new(jet())
        .current_dir(&root)
        .args(["self", "devtools", "new-example"])
        .arg(format!("{}/{}", topic, name))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "new-example should succeed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(example_path.is_file(), "missing {}", example_path.display());
    assert!(
        expected_path.is_file(),
        "missing {}",
        expected_path.display()
    );

    // It must be a real, passing example (I5): front end accepts it, and
    // (when rustc is available) it builds and its stdout matches the .out
    // fixture byte for byte.
    let src = fs::read_to_string(&example_path).unwrap();
    let shown = format!("examples/features/{}/{}.jet", topic, name);
    let compiled = jet::compile_with_path(&src, &example_path.to_string_lossy())
        .unwrap_or_else(|diags| {
            panic!(
                "scaffolded example failed the front end:\n{}",
                jet::render_diagnostics(&shown, &src, &diags)
            )
        });

    if common::have_rustc() {
        let rs = dir_join_unique("devtools_new_example");
        fs::write(&rs, &compiled.rust).unwrap();
        let bin = rs.with_extension("");
        let rustc = Command::new("rustc")
            .args(["--edition", "2021"])
            .arg(&rs)
            .arg("-o")
            .arg(&bin)
            .output()
            .unwrap();
        assert!(
            rustc.status.success(),
            "I2 violated by scaffold: rustc rejected generated code:\n{}",
            String::from_utf8_lossy(&rustc.stderr)
        );
        let run = Command::new(&bin).output().unwrap();
        assert!(run.status.success());
        let expected = fs::read_to_string(&expected_path).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "scaffold stdout must match its own .out fixture"
        );
        let _ = fs::remove_file(&rs);
        let _ = fs::remove_file(&bin);
    }

    let _ = fs::remove_file(&example_path);
    let _ = fs::remove_file(&expected_path);
}

fn dir_join_unique(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("jet_{}_{}.rs", tag, std::process::id()))
}

// ── new-ui ─────────────────────────────────────────────────────────

/// Scaffolds a `tests/ui/<name>.jet` + `<name>.stderr` pair that's already
/// self-consistent with `tests/diagnostic_snapshots.rs`'s `ui_snapshots`
/// (same compile + render calls), matching that harness's layout exactly.
#[test]
fn devtools_new_ui_scaffolds_a_self_consistent_snapshot_pair() {
    let root = repo_root();
    let name = format!("devtools_test_scaffold_{}_{}", std::process::id(), line!());
    let jet_path = root.join("tests/ui").join(format!("{}.jet", name));
    let stderr_path = root.join("tests/ui").join(format!("{}.stderr", name));
    let _ = fs::remove_file(&jet_path);
    let _ = fs::remove_file(&stderr_path);
    let _cleanup = ScaffoldCleanup(vec![jet_path.clone(), stderr_path.clone()]);

    let out = Command::new(jet())
        .current_dir(&root)
        .args(["self", "devtools", "new-ui"])
        .arg(&name)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "new-ui should succeed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(jet_path.is_file(), "missing {}", jet_path.display());
    assert!(stderr_path.is_file(), "missing {}", stderr_path.display());

    // Recompute exactly what `ui_snapshots` computes and confirm it matches
    // the `.stderr` this just wrote — i.e. the pair is valid the moment it's
    // created, with no manual bless step needed.
    let src = fs::read_to_string(&jet_path).unwrap();
    let shown_path = format!("tests/ui/{}.jet", name);
    let actual = match jet::compile_with_path(&src, &shown_path) {
        Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
        Ok(_) => "(no errors)\n".to_string(),
    };
    let expected = fs::read_to_string(&stderr_path).unwrap();
    assert_eq!(actual, expected, "scaffolded pair isn't self-consistent");
    assert!(
        actual.contains("Error ["),
        "expected the scaffold to trigger a real diagnostic, got:\n{}",
        actual
    );

    let _ = fs::remove_file(&jet_path);
    let _ = fs::remove_file(&stderr_path);
}

// ── check-fixture-paths ────────────────────────────────────────────

#[test]
fn devtools_check_fixture_paths_runs_clean_on_this_repo() {
    let root = repo_root();
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["self", "devtools", "check-fixture-paths"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "check-fixture-paths should run clean on this repo:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("all present"),
        "expected an all-present summary line"
    );
}

// ── bless ──────────────────────────────────────────────────────────

/// `--dry-run` prints the `UPDATE_EXPECT=1 cargo test --test <target>`
/// commands without running them (never mutates a snapshot file) — the safe
/// path to test here; the real invocation is a thin `Command` wrapper over
/// the same convention every `UPDATE_EXPECT` test file already documents.
#[test]
fn devtools_bless_dry_run_lists_every_known_target() {
    let out = Command::new(jet())
        .args(["self", "devtools", "bless", "--dry-run"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for target in [
        "cli",
        "cross",
        "diagnostic_snapshots",
        "diagnostics_coverage",
        "release_gates",
    ] {
        assert!(
            stdout.contains(&format!("cargo test --test {}", target)),
            "bless --dry-run missing target `{}`:\n{}",
            target,
            stdout
        );
        assert!(
            stdout.contains("UPDATE_EXPECT=1"),
            "bless --dry-run should show the UPDATE_EXPECT wiring:\n{}",
            stdout
        );
    }
}

#[test]
fn devtools_bless_rejects_unknown_target() {
    let out = Command::new(jet())
        .args(["self", "devtools", "bless", "not_a_real_test_target", "--dry-run"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "bless should reject an unknown target name"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("unknown bless target"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
