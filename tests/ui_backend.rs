//! D-RENDERTGT2=A (c133 M1): null backend measure→layout→paint conformance.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn build_and_run(dir: &PathBuf, name: &str, src: &str) -> (i32, String, String) {
    let path = dir.join(name);
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            rs.to_str().unwrap(),
            "-o",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

#[test]
fn null_backend_measure_layout_paint_roundtrip() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping ui backend test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_ui_backend_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "ui_null_backend",
        include_str!("../examples/features/ui/ui_null_backend.jet"),
    );
    assert_eq!(code, 0, "ui backend roundtrip failed: {stderr}");
    let expected = include_str!("../examples/features/expected/ui/ui_null_backend.out");
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn tui_backend_reactive_render_loop() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping ui backend test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_ui_tui_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "ui_tui_reactive",
        include_str!("../examples/features/ui/ui_tui_reactive.jet"),
    );
    assert_eq!(code, 0, "ui tui reactive render failed: {stderr}");
    let expected = include_str!("../examples/features/expected/ui/ui_tui_reactive.out");
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}
