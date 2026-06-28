//! E2-M15 cross-compilation and freestanding profile tests.
//! Tests E3301 (std API in freestanding build).

use std::fs;
use std::path::PathBuf;

/// Write `src` to a temp file and compile in freestanding mode.
/// Returns the rendered diagnostic output (or "(no errors)\n").
fn check_freestanding_src(src: &str, label: &str) -> String {
    let dir = std::env::temp_dir().join(format!("jet_cross_test_{}_{}", std::process::id(), label));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.jet");
    fs::write(&path, src).unwrap();
    let file_arg = path.to_string_lossy().into_owned();
    match jet::compile_freestanding(&file_arg) {
        Ok(_) => "(no errors)\n".to_string(),
        Err(diags) => jet::render_diagnostics(&file_arg, src, &diags),
    }
}

// ── E3301: OS-dependent API used in freestanding build ──────────────────────

#[test]
fn e3301_fs_read_in_freestanding() {
    let src = r#"use core.fs as fs

fn main() {
    _ @= fs.read("config.txt")
}
"#;
    let out = check_freestanding_src(src, "fs_read");
    assert!(
        out.contains("E3301"),
        "expected E3301 for fs.read in freestanding mode; got:\n{}",
        out
    );
    assert!(
        out.contains("freestanding"),
        "expected 'freestanding' in error; got:\n{}",
        out
    );
}

#[test]
fn e3301_http_in_freestanding() {
    let src = r#"use core.http as http

fn main() {
    _ @= http.get("http://example.com")
}
"#;
    let out = check_freestanding_src(src, "http");
    assert!(
        out.contains("E3301"),
        "expected E3301 for http.get in freestanding mode; got:\n{}",
        out
    );
}

#[test]
fn e3301_tasks_in_freestanding() {
    let src = r#"use core.tasks as tasks

fn main() {
    t @= tasks.spawn(() => 42)
    t.join()
}
"#;
    let out = check_freestanding_src(src, "tasks");
    assert!(
        out.contains("E3301"),
        "expected E3301 for tasks.spawn in freestanding mode; got:\n{}",
        out
    );
}

#[test]
fn freestanding_allows_core_math() {
    // core.math is not OS-dependent; must not trigger E3301.
    let src = r#"use core.math as math

fn main() {
    x @= math.sqrt(4.0)
    print(x)
}
"#;
    let out = check_freestanding_src(src, "core_math");
    assert!(
        !out.contains("E3301"),
        "core.math should be allowed in freestanding mode; got:\n{}",
        out
    );
}

#[test]
fn freestanding_allows_core_json() {
    // core.encoding.json does not need an OS.
    let src = r#"use core.encoding.json as json

fn main() {
    s @= json.to_string("hello")
    print(s)
}
"#;
    let out = check_freestanding_src(src, "core_json");
    assert!(
        !out.contains("E3301"),
        "core.encoding.json should be allowed in freestanding mode; got:\n{}",
        out
    );
}

// ── E3301 UI snapshot ────────────────────────────────────────────────────────

/// Pin the exact rendered output for E3301 so it matches docs/spec/diagnostics.md.
#[test]
fn e3301_snapshot() {
    let src_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui/freestanding_e3301.jet");
    let snap_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui/freestanding_e3301.stderr");

    if !src_path.exists() {
        panic!("missing tests/ui/freestanding_e3301.jet (I4 requires a snapshot)");
    }
    let src = fs::read_to_string(&src_path).unwrap();
    let shown = "tests/ui/freestanding_e3301.jet";
    let actual = match jet::compile_freestanding(&src_path.to_string_lossy()) {
        Ok(_) => "(no errors)\n".to_string(),
        Err(diags) => jet::render_diagnostics(shown, &src, &diags),
    };

    if std::env::var("UPDATE_EXPECT").is_ok() {
        fs::write(&snap_path, &actual).unwrap();
    } else {
        let expected = fs::read_to_string(&snap_path).unwrap_or_default();
        assert_eq!(
            actual, expected,
            "\nE3301 snapshot mismatch (run UPDATE_EXPECT=1 cargo test to bless)\n"
        );
    }
}
