use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read evaluator source directory") {
        let path = entry.expect("read evaluator source entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn evaluator_boundary_has_no_process_or_external_engine_escape() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("crates/jetpack/src/NixEval");
    let mut files = Vec::new();
    rust_files(&dir, &mut files);
    assert!(!files.is_empty(), "native evaluator boundary must exist");

    let forbidden = [
        "std::process",
        "process::Command",
        "Command::new",
        "extern crate",
        "use tvix",
        "tvix_",
        "libnix",
        "nix-instantiate",
    ];
    for path in files {
        let source = fs::read_to_string(&path).expect("read evaluator source");
        for needle in forbidden {
            assert!(
                !source.contains(needle),
                "{} crosses native evaluator stop-line with `{needle}`",
                path.display()
            );
        }
    }

    let lib = fs::read_to_string(root.join("crates/jetpack/src/lib.rs")).expect("read jetpack lib");
    assert!(lib.contains("pub(crate) mod NixEval;"));
    assert!(!lib.contains("pub mod NixEval;"));
}

#[test]
fn oracle_pin_is_independent_from_mutable_root_flake_lock() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let oracle = fs::read_to_string(root.join("tests/fixtures/nix-compat/oracle.json"))
        .expect("committed oracle manifest");
    assert!(oracle.contains("\"version\": \"2.34.8\""));
    assert!(oracle.contains("b6769c588f60b3e762f73d3a8cf60294df078ccd"));
    assert!(oracle.contains("f3f1c3c5b8ad91850e0f7c590cf177f7ab022024"));
    assert!(oracle.contains("b5aa0fbd538984f6e3d201be0005b4463d8b09f8"));
    assert!(oracle.contains("\"last_modified\": 1782723713"));
    assert!(oracle.contains("sha256-oPXCU/SSUokcGaJREHibG1CBX3+s/W7orDWQOZDsEeQ="));
    assert_eq!(oracle.matches("\"build_nar_hash\": null").count(), 4);
    assert_eq!(oracle.matches("\"executable_nar_hash\": null").count(), 4);
}
