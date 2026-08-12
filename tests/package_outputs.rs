//! D-CONF-PLANE1/D-CONF-NAME1 (card #1517): a package.jet `outputs:` block
//! drives entry resolution for both `jet run` (JIT) and `jet build` (AOT) —
//! I9 tier parity for the manifest-driven build path.
//!
//! `examples/features/packages/outputs_build/` has no `main.jet`/`run.jet`
//! convention file. Its `package.jet` uses a *dotted* entry —
//! `outputs: .{ demo: .Executable.{ entry: service.run } }` — which follows
//! `entry.jet`'s `use service` import into `service/module.jet`. That's
//! deliberate: a single-segment `entry: run` resolves through the existing
//! root-level source lookup, so it can never prove `outputs:` is doing
//! anything.
//! This fixture has no canonical entry and can only resolve through
//! `outputs:`. `entry_resolution_requires_the_outputs_block` below proves it:
//! the same fixture with `outputs:` deleted fails to resolve at all.
//!
//! `golden.rs`'s directory scan (which requires a `main.<ext>`) never
//! discovers this fixture and always compiles a single file directly,
//! bypassing package/output resolution — so this is a dedicated test, not an
//! addition to that harness.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

mod common;

fn example_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/packages/outputs_build")
}

fn expected_output() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/expected/packages/outputs_build.out");
    fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing {}", path.display()))
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

#[test]
fn outputs_build_example_has_no_convention_entry_filename() {
    // The whole point of this fixture: no `main.jet`/`run.jet` file exists,
    // and the entry function lives behind an import the migration-era
    // fallback cannot follow.
    let dir = example_dir();
    assert!(!dir.join("main.jet").is_file());
    assert!(!dir.join("run.jet").is_file());
    assert!(dir.join("entry.jet").is_file());
    assert!(dir.join("service/module.jet").is_file());
    let manifest = fs::read_to_string(dir.join("package.jet")).unwrap();
    assert!(manifest.contains("outputs:"), "{manifest}");
    assert!(manifest.contains("entry: service.run"), "{manifest}");
}

#[test]
fn outputs_block_drives_jet_run_jit() {
    let dir = example_dir();
    let out = Command::new(jet_bin())
        .arg("run")
        .current_dir(&dir)
        .output()
        .expect("jet run should execute");
    assert!(
        out.status.success(),
        "jet run failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected_output());
}

#[test]
fn outputs_block_drives_jet_build_aot() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping outputs_block_drives_jet_build_aot (need rustc)");
        return;
    }
    let dir = example_dir();
    let build_dir = dir.join("build");
    let _ = fs::remove_dir_all(&build_dir);

    let build = Command::new(jet_bin())
        .arg("build")
        .current_dir(&dir)
        .output()
        .expect("jet build should execute");
    assert!(
        build.status.success(),
        "jet build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    // `jet build` names the binary after the entry file it resolved
    // (`module`, from `service/module.jet`) — proof the AOT lens took the
    // same `outputs:` path as the JIT lens above, not a `main`/`run`
    // fallback (which would never find a file two directories away from a
    // bare filename convention).
    let binary = build_dir.join("module");
    assert!(
        binary.is_file(),
        "jet build did not produce build/module (resolved via outputs:); found: {:?}",
        fs::read_dir(&build_dir)
            .map(|entries| entries.flatten().map(|e| e.path()).collect::<Vec<_>>())
            .unwrap_or_default()
    );

    let run = Command::new(&binary).output().expect("built binary should run");
    assert!(
        run.status.success(),
        "built binary failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected_output());

    let _ = fs::remove_dir_all(&build_dir);
}

/// The negative half of the proof: copy the fixture but drop `outputs:` from
/// `package.jet`, keeping the same `entry.jet` + `service/module.jet` layout.
/// There is no canonical `run.jet`, so entry resolution must fail. If this
/// ever starts resolving without `outputs:`, the fixture above would stop
/// proving anything.
#[test]
fn entry_resolution_requires_the_outputs_block() {
    let dir = std::env::temp_dir().join(format!(
        "jet-outputs-build-negative-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("service")).unwrap();
    fs::write(dir.join("package.jet"), "name: \"outputs_build_demo\"\nversion: \"0.1.0\"\n").unwrap();
    fs::copy(example_dir().join("entry.jet"), dir.join("entry.jet")).unwrap();
    fs::copy(
        example_dir().join("service/module.jet"),
        dir.join("service/module.jet"),
    )
    .unwrap();

    let out = Command::new(jet_bin())
        .arg("run")
        .current_dir(&dir)
        .output()
        .expect("jet run should execute");
    assert!(
        !out.status.success(),
        "jet run unexpectedly succeeded without outputs: driving the entry"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("run.jet"),
        "expected a missing-entry error naming run.jet, got:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&dir);
}
