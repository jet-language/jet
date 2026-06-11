//! Golden tests: every example program must pass the front end, and (when
//! rustc is available) build and print exactly its expected output.
//! Examples are the executable spec (invariant I5).
//!
//! Also enforces:
//!   I1 — generated code never contains `unsafe`
//!   I2 — rustc accepting the generated code; a rejection here is a
//!        front-end soundness bug, reported loudly

use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn examples_compile_and_run() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ex_dir = root.join("examples");
    let ext = jet::syntax::FILE_EXT;
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; checking codegen only, skipping build+run");
    }

    let mut entries: Vec<_> = fs::read_dir(&ex_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();

    let mut checked = 0;
    for path in entries {
        if path.extension().and_then(|e| e.to_str()) != Some(ext) {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(&path).unwrap();

        let rust_code = match jet::compile(&src) {
            Ok(c) => c.rust,
            Err(diags) => panic!(
                "example {} failed the front end:\n{}",
                stem,
                jet::render_diagnostics(&format!("examples/{}.{}", stem, ext), &src, &diags)
            ),
        };

        // I1: memory safety is never traded away.
        assert!(
            !rust_code.contains("unsafe"),
            "generated Rust for {} contains `unsafe`",
            stem
        );
        assert!(
            rust_code.contains("fn main()"),
            "generated Rust for {} has no fn main",
            stem
        );

        if have_rustc {
            let dir = std::env::temp_dir();
            let rs = dir.join(format!("jet_golden_{}.rs", stem));
            let bin = dir.join(format!("jet_golden_{}", stem));
            fs::write(&rs, &rust_code).unwrap();
            let out = Command::new("rustc")
                .args(["--edition", "2021"])
                .arg(&rs)
                .arg("-o")
                .arg(&bin)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "I2 violated: rustc rejected generated code for {} — this is a jet bug:\n{}",
                stem,
                String::from_utf8_lossy(&out.stderr)
            );
            let run = Command::new(&bin).output().unwrap();
            let expected = fs::read_to_string(ex_dir.join("expected").join(format!("{}.out", stem)))
                .unwrap_or_else(|_| panic!("missing examples/expected/{}.out", stem));
            assert_eq!(
                String::from_utf8_lossy(&run.stdout),
                expected,
                "output mismatch for example {}",
                stem
            );
        }
        checked += 1;
    }
    assert!(checked >= 2, "expected at least 2 examples, found {}", checked);
}
