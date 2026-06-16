//! M7 FFI integration — gated on `cargo` like rustc-gated goldens.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn ffi_example_compiles_and_runs() {
    let have_cargo = Command::new("cargo").arg("--version").output().is_ok();
    if !have_cargo {
        eprintln!("note: cargo not found; skipping FFI integration test");
        return;
    }
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping FFI integration test");
        return;
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("examples/features/22_ffi.jet");
    let src = fs::read_to_string(&path).unwrap();
    let shown = "examples/features/22_ffi.jet";

    let out = jet::compile_with_path(&src, shown).unwrap_or_else(|diags| {
        panic!(
            "22_ffi.jet failed the front end:\n{}",
            jet::render_diagnostics(shown, &src, &diags)
        );
    });
    assert!(out.ffi.is_some(), "expected an FFI bridge for 22_ffi.jet");
    assert!(
        !out.rust.contains("unsafe"),
        "I1: FFI output must not use unsafe"
    );

    let dir = std::env::temp_dir();
    let rs = dir.join("jet_ffi_test.rs");
    let bin = dir.join("jet_ffi_test_bin");
    fs::write(&rs, &out.rust).unwrap();

    let link = out.ffi.as_ref().unwrap();
    let status = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .arg("--extern")
        .arg(format!("{}={}", link.crate_name, link.rlib_path.display()))
        .arg("-L")
        .arg(format!("dependency={}", link.deps_dir.display()))
        .status()
        .unwrap();
    assert!(status.success(), "rustc rejected FFI-linked output (I2)");

    let run = Command::new(&bin).output().unwrap();
    assert!(run.status.success(), "22_ffi runtime failed");
    let expected = fs::read_to_string(root.join("examples/features/expected/22_ffi.out")).unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
}

#[test]
fn inline_ffi_pin_works_inside_manifest_project() {
    let have_cargo = Command::new("cargo").arg("--version").output().is_ok();
    if !have_cargo {
        eprintln!("note: cargo not found; skipping manifest FFI integration test");
        return;
    }

    let root = std::env::temp_dir().join(format!(
        "jet_manifest_ffi_{}_{}",
        std::process::id(),
        "inline"
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("pack.jet"),
        "package: {\n    name: \"ffi_app\",\n    version: \"0.1.0\",\n}\n",
    )
    .unwrap();
    let path = root.join("main.jet");
    let src = "extern rust \"base64@0.22\" {\n    fn b64encode(s: String) -> String = \"base64::encode\";\n}\nfn main() { print(b64encode(\"hi\")); }\n";
    fs::write(&path, src).unwrap();

    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "inline FFI pin should work even when pack.jet exists:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        );
    });
    assert!(out.ffi.is_some(), "expected an FFI bridge");
}
