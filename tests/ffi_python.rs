//! Focused closeout checks for the unified Python sidecar and capability report.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn test_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("jet_{name}_{}", std::process::id()))
}

#[test]
fn ffi_capability_report_is_golden_and_cli_backed() {
    let expected = include_str!("fixtures/ffi_capability_report.txt");
    assert_eq!(jet::Foreign::capability_report_text(), expected);

    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "dossier", "ffi"])
        .env("NO_COLOR", "1")
        .output()
        .expect("jet inspect dossier ffi");
    assert!(
        output.status.success(),
        "capability report failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[cfg(unix)]
#[test]
fn python_sidecar_binds_runs_and_launders_failure() {
    if Command::new("python3").arg("--version").output().is_err()
        || Command::new("cc").arg("--version").output().is_err()
        || Command::new("ar").arg("--version").output().is_err()
    {
        eprintln!("note: Python sidecar toolchain unavailable; skipping production-path check");
        return;
    }
    let root = test_root("python_sidecar");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("ops.py"),
        "def add(left: int, right: int) -> int:\n    return left + right\n\ndef negate(value: bool) -> bool:\n    return not value\n\ndef fail(value: int) -> int:\n    raise RuntimeError('secret foreign detail')\n\ndef wrong_type(value: int) -> int:\n    return 1.5\n",
    )
    .unwrap();
    let bind = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "bind", "py", "ops.py", "--pkg", "ops"])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "Python binding failed: {}",
        String::from_utf8_lossy(&bind.stderr)
    );
    let generated = fs::read_to_string(root.join(".jet/bindings/py/ops.jet")).unwrap();
    assert!(generated.contains("jet-ffi-descriptor="));
    assert!(generated.contains("-[FFI.Py]>"));
    assert!(!generated.contains("=>"));
    assert!(root.join(".jet/bindings/py/libjet_py_ops.a").is_file());
    let provenance = fs::read_to_string(root.join(".jet/bindings/py/ops.provenance")).unwrap();
    assert!(provenance.contains("descriptor="));
    assert!(provenance.contains("layout=AdapterTyped"));
    assert!(provenance.contains("ownership=AdapterOwned"));
    assert!(provenance.contains("errors=AdapterResult"));
    assert!(provenance.contains("callbacks=AdapterMarshalled"));
    assert!(provenance.contains("async=AdapterMarshalled"));
    assert!(provenance.contains("tasks=AdapterMarshalled"));
    assert!(provenance.contains("safety=GeneratedWrapper"));
    assert!(provenance.contains("provider=PyPi"));
    assert!(provenance.contains("artifact.libjet_py_ops.a="));
    assert!(provenance.contains("runtime-toolchain="));
    assert!(provenance.contains("source-sha256="));
    let first_identity = provenance
        .lines()
        .find_map(|line| line.strip_prefix("identity="))
        .unwrap()
        .to_string();
    let first_store = root.join(".jet/bindings/py/.bridges").join(&first_identity);
    assert!(first_store.join("libjet_py_ops.a").is_file());
    fs::remove_file(root.join(".jet/bindings/py/libjet_py_ops.a")).unwrap();
    let cached = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "bind", "py", "ops.py", "--pkg", "ops"])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        cached.status.success(),
        "identical Python binding did not reuse its bridge: {}",
        String::from_utf8_lossy(&cached.stderr)
    );
    let cached_provenance =
        fs::read_to_string(root.join(".jet/bindings/py/ops.provenance")).unwrap();
    assert_eq!(
        cached_provenance
            .lines()
            .find_map(|line| line.strip_prefix("identity=")),
        Some(first_identity.as_str())
    );
    assert!(root.join(".jet/bindings/py/libjet_py_ops.a").is_file());

    fs::write(
        root.join("main.jet"),
        "use py.ops as ops\nfn run() -[FFI.Py, IO]> {\n    print(ops.add(2, 3) ?? panic(\"add failed\"))\n    print(ops.negate(true) ?? true)\n    print(ops.fail(7) ?? 99)\n    print(ops.wrong_type(7) ?? 123)\n}\n",
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
        "Python sidecar run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "5\nfalse\n99\n123\n");

    fs::write(
        root.join("ops.py"),
        "def add(left: int, right: int) -> int:\n    return left + right + 1\n\ndef fail(value: int) -> int:\n    raise RuntimeError('secret foreign detail')\n",
    )
    .unwrap();
    let changed = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "bind", "py", "ops.py", "--pkg", "ops"])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        changed.status.success(),
        "changed Python source did not rebuild its bridge: {}",
        String::from_utf8_lossy(&changed.stderr)
    );
    let changed_provenance =
        fs::read_to_string(root.join(".jet/bindings/py/ops.provenance")).unwrap();
    let changed_identity = changed_provenance
        .lines()
        .find_map(|line| line.strip_prefix("identity="))
        .unwrap();
    assert_ne!(changed_identity, first_identity);
    assert!(root
        .join(".jet/bindings/py/.bridges")
        .join(changed_identity)
        .join("libjet_py_ops.a")
        .is_file());

    fs::write(root.join("bad.py"), "def bad(value):\n    return value\n").unwrap();
    let bad = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "bind", "py", "bad.py", "--pkg", "bad"])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!bad.status.success());
    let stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(stderr.contains("Error [E3208]:"), "{stderr}");
    assert!(stderr.contains(" Why:"), "{stderr}");
    assert!(stderr.contains(" Fix:"), "{stderr}");
    assert!(!root.join(".jet/bindings/py/bad.jet").exists());
    let _ = fs::remove_dir_all(root);
}
