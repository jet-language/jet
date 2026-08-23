//! Focused production-path conformance for the typed JavaScript sidecar.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn test_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("jet_{name}_{}", std::process::id()))
}

#[cfg(unix)]
#[test]
fn javascript_sidecar_binds_runs_and_rejects_dynamic_types() {
    if Command::new("node").arg("--version").output().is_err()
        || Command::new("cc").arg("--version").output().is_err()
        || Command::new("ar").arg("--version").output().is_err()
    {
        eprintln!("note: JavaScript sidecar toolchain unavailable; skipping production-path check");
        return;
    }
    let root = test_root("javascript_sidecar");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("ops.d.ts"),
        "export function add(left: bigint, right: bigint): bigint;\nexport function fail(value: bigint): bigint;\nexport function wrong_type(value: bigint): bigint;\n",
    )
    .unwrap();
    fs::write(
        root.join("ops.mjs"),
        "export function add(left, right) { return left + right; }\nexport function fail(value) { throw new Error('secret foreign detail'); }\nexport function wrong_type(value) { return Number(value); }\n",
    )
    .unwrap();
    let bind = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "inspect",
            "bind",
            "js",
            "ops.d.ts",
            "--runtime",
            "ops.mjs",
            "--pkg",
            "ops",
        ])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "JavaScript binding failed: {}",
        String::from_utf8_lossy(&bind.stderr)
    );
    let generated = fs::read_to_string(root.join(".jet/bindings/js/ops.jet")).unwrap();
    assert!(generated.contains("jet-ffi-descriptor="));
    assert!(generated.contains("Int String! -[FFI]>"));
    assert!(!generated.contains("=>"));
    assert!(root.join(".jet/bindings/js/libjet_js_ops.a").is_file());
    assert!(root.join(".jet/bindings/js/ops.d.ts").is_file());
    let provenance = fs::read_to_string(root.join(".jet/bindings/js/ops.provenance")).unwrap();
    assert!(provenance.contains("descriptor="));
    assert!(provenance.contains("artifact.libjet_js_ops.a="));

    fs::write(
        root.join("main.jet"),
        "use js.ops as ops\nfn run() -[FFI, IO]> {\n    print(ops.add(2, 3) ?? panic(\"add failed\"))\n    print(ops.fail(7) ?? 99)\n    print(ops.wrong_type(7) ?? 123)\n}\n",
    )
    .unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "main.jet"])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "JavaScript sidecar run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "5\n99\n123\n");

    fs::write(
        root.join("bad.d.ts"),
        "export function dynamic(value?: number): number;\n",
    )
    .unwrap();
    let bad = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "inspect",
            "bind",
            "js",
            "bad.d.ts",
            "--runtime",
            "ops.mjs",
            "--pkg",
            "bad",
        ])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!bad.status.success());
    let stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(stderr.contains("Error [E3208]:"), "{stderr}");
    assert!(stderr.contains(" Why:"), "{stderr}");
    assert!(stderr.contains(" Fix:"), "{stderr}");
    assert!(!root.join(".jet/bindings/js/bad.jet").exists());
    let _ = fs::remove_dir_all(root);
}
