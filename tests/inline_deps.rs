//! U11 (D-JPK-SCRIPTDEP1=A) — inline script dependencies.
//!
//! A manifest-less `.jet` script may open with `use pkg#version;` instead of
//! shipping a `package.jet`. These tests drive the real `jet` binary end to end:
//! `jet run` resolves + locks by file-content hash, `jet fetch --lock` writes a
//! `<script>.lock` sidecar, and `jet init <script>` lifts the inline refs into
//! a generated `package.jet`. Resolution is offline-only today (no external
//! network/registry fetch — see `crates/jet-driver/src/Jetpack/ScriptDeps.rs`):
//! a local `.jet/inline-deps/<name>/<version>/` copy stands in as the
//! resolvable "provider" fixture, mirroring Jetpack's own committed-fixture
//! test convention.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn tmp_dir(label: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "jet_inline_deps_{}_{}_{}",
        label,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    fs::create_dir_all(&base).unwrap();
    base
}

fn write(dir: &Path, rel: &str, content: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn have_cargo() -> bool {
    Command::new("cargo").arg("--version").output().is_ok()
}

/// Run the `jet` binary with its own private `JET_CACHE_DIR` (D-BUILDNORM1=A):
/// the native build cache is keyed on the canonical pre-sema AST, so two
/// tests writing byte-identical script content (as several here deliberately
/// do, to keep fixtures simple) would otherwise share a cache entry across
/// processes and skip re-running sema — silently dropping the L0203 lint a
/// cached rebuild never re-emits.
fn jet_cmd(args: &[&str], cwd: &Path) -> std::process::Output {
    Command::new(jet_bin())
        .args(args)
        .current_dir(cwd)
        .env("JET_CACHE_DIR", cwd.join(".jet-test-cache"))
        .output()
        .expect("jet binary should run")
}

/// Committed offline fixture: a tiny `textkit` "library" at 1.4.2, staged the
/// same way a `jet fetch --lock`-populated local cache would look.
fn write_textkit_fixture(project: &Path) {
    write(
        project,
        ".jet/inline-deps/textkit/1.4.2/textkit.jet",
        "pub fn shout(s: String) => String {\n    return ~s;\n}\n",
    );
}

// ─────────────────────────────────────────────
// script_inline_dep_resolves
// ─────────────────────────────────────────────

#[test]
fn script_inline_dep_resolves() {
    if !jet_bin().is_file() || !have_cargo() {
        eprintln!("note: skipping script_inline_dep_resolves (need the jet binary + cargo)");
        return;
    }
    let dir = tmp_dir("resolves");
    write_textkit_fixture(&dir);
    write(
        &dir,
        "stats.jet",
        "use textkit#1.4.2;\n\nfn run() {\n    print(textkit.shout(\"hi\"))\n}\n",
    );

    let out = jet_cmd(&["run", "stats.jet"], &dir);
    assert!(
        out.status.success(),
        "jet run should resolve the inline dep and run\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hi"),
        "expected the script's output, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn script_inline_dep_unresolved_is_e1253() {
    if !jet_bin().is_file() {
        return;
    }
    let dir = tmp_dir("unresolved");
    write(
        &dir,
        "stats.jet",
        "use ghostpkg#1.0.0;\n\nfn run() {\n    print(\"never\")\n}\n",
    );

    let out = jet_cmd(&["run", "stats.jet"], &dir);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1253"),
        "expected E1253 in stderr, got: {stderr}"
    );
}

#[test]
fn script_inline_dep_unpinned_is_l0203() {
    if !jet_bin().is_file() || !have_cargo() {
        return;
    }
    let dir = tmp_dir("unpinned");
    write_textkit_fixture(&dir);
    write(
        &dir,
        "stats.jet",
        "use textkit#1.4;\n\nfn run() {\n    print(textkit.shout(\"hi\"))\n}\n",
    );

    let out = jet_cmd(&["run", "stats.jet"], &dir);
    assert!(
        out.status.success(),
        "a loose selector still resolves and runs\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("L0203"),
        "expected L0203 warning in stderr, got: {stderr}"
    );
}

// ─────────────────────────────────────────────
// jet_lock_writes_sidecar
// ─────────────────────────────────────────────

#[test]
fn jet_lock_writes_sidecar() {
    if !jet_bin().is_file() {
        return;
    }
    let dir = tmp_dir("lock");
    write_textkit_fixture(&dir);
    write(
        &dir,
        "stats.jet",
        "use textkit#1.4;\n\nfn run() {\n    print(textkit.shout(\"hi\"))\n}\n",
    );

    let out = jet_cmd(&["fetch", "--lock", "stats.jet"], &dir);
    assert!(
        out.status.success(),
        "jet fetch --lock should succeed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let sidecar = dir.join("stats.jet.lock");
    assert!(sidecar.is_file(), "expected {} to exist", sidecar.display());
    let contents = fs::read_to_string(&sidecar).unwrap();
    assert!(contents.contains("version = 1"));
    assert!(contents.contains("script_hash = \"sha256-"));
    assert!(contents.contains("[[dep]]"));
    assert!(contents.contains("name = \"textkit\""));
    assert!(contents.contains("selector = \"1.4\""));
    assert!(contents.contains("resolved = \"1.4.2\""));
    assert!(contents.contains("content_hash = \"sha256-"));

    // Locking again is stable (same script, same resolved shape).
    let out2 = jet_cmd(&["fetch", "--lock", "stats.jet"], &dir);
    assert!(out2.status.success());
    let contents2 = fs::read_to_string(&sidecar).unwrap();
    assert_eq!(contents, contents2);
}

#[test]
fn jet_lock_unresolved_dep_is_e1253() {
    if !jet_bin().is_file() {
        return;
    }
    let dir = tmp_dir("lock_unresolved");
    write(
        &dir,
        "stats.jet",
        "use ghostpkg#1.0.0;\n\nfn run() {\n    print(\"never\")\n}\n",
    );
    let out = jet_cmd(&["fetch", "--lock", "stats.jet"], &dir);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("E1253"));
    assert!(!dir.join("stats.jet.lock").exists());
}

// ─────────────────────────────────────────────
// jet_init_lifts_uses_into_package_jet
// ─────────────────────────────────────────────

#[test]
fn jet_init_lifts_uses_into_package_jet() {
    if !jet_bin().is_file() {
        return;
    }
    let dir = tmp_dir("init_lift");
    write_textkit_fixture(&dir);
    write(
        &dir,
        "stats.jet",
        "use textkit#1.4;\n\nfn run() {\n    print(textkit.shout(\"hi\"))\n}\n",
    );

    let out = jet_cmd(&["init", "stats.jet"], &dir);
    assert!(
        out.status.success(),
        "jet init should succeed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let manifest_path = dir.join("package.jet");
    assert!(manifest_path.is_file(), "jet init should write package.jet");
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    assert!(
        manifest.contains("textkit"),
        "expected the inline dep lifted into package.jet, got:\n{manifest}"
    );
    assert!(
        manifest.contains("\"1.4\""),
        "expected the selector preserved in the lifted dep, got:\n{manifest}"
    );
}

/// `jet init` with no script argument keeps its original bare-cwd behavior
/// (no lift, since there's nothing to lift).
#[test]
fn jet_init_without_script_is_unchanged() {
    if !jet_bin().is_file() {
        return;
    }
    let dir = tmp_dir("init_bare");
    let out = jet_cmd(&["init"], &dir);
    assert!(out.status.success());
    let manifest = fs::read_to_string(dir.join("package.jet")).unwrap();
    assert!(manifest.contains("name:"));
    assert!(!manifest.contains("textkit"));
}
