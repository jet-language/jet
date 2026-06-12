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
    let have_cargo = Command::new("cargo").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; checking codegen only, skipping build+run");
    }

    let mut entries: Vec<(PathBuf, String, String)> = Vec::new();
    for e in fs::read_dir(&ex_dir).unwrap().flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) == Some(ext) {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            entries.push((
                path.clone(),
                stem.clone(),
                format!("examples/{}.{}", stem, ext),
            ));
        } else if path.is_dir() {
            let main = path.join(format!("main.{}", ext));
            if main.is_file() {
                let stem = path.file_name().unwrap().to_string_lossy().into_owned();
                entries.push((
                    main.clone(),
                    stem,
                    format!("examples/{}/main.{}", path.file_name().unwrap().to_string_lossy(), ext),
                ));
            }
        }
    }
    entries.sort_by(|a, b| a.1.cmp(&b.1));

    let mut checked = 0;
    for (path, stem, shown) in entries {
        let src = fs::read_to_string(&path).unwrap();

        if stem == "22_ffi" && !have_cargo {
            eprintln!("note: skipping examples/22_ffi.jet golden (need cargo for FFI bridge)");
            checked += 1;
            continue;
        }

        let compiled = match jet::compile_with_path(&src, &shown) {
            Ok(c) => c,
            Err(diags) => panic!(
                "example {} failed the front end:\n{}",
                stem,
                jet::render_diagnostics(&format!("examples/{}.{}", stem, ext), &src, &diags)
            ),
        };
        let rust_code = compiled.rust;
        let ffi_link = compiled.ffi;

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
            let mut rustc_cmd = Command::new("rustc");
            rustc_cmd
                .args(["--edition", "2021"])
                .arg(&rs)
                .arg("-o")
                .arg(&bin);
            if let Some(link) = &ffi_link {
                rustc_cmd
                    .arg("--extern")
                    .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
                if link.deps_dir.is_dir() {
                    rustc_cmd
                        .arg("-L")
                        .arg(format!("dependency={}", link.deps_dir.display()));
                }
            }
            let out = rustc_cmd.output().unwrap();
            assert!(
                out.status.success(),
                "I2 violated: rustc rejected generated code for {} — this is a jet bug:\n{}",
                stem,
                String::from_utf8_lossy(&out.stderr)
            );
            let run = Command::new(&bin).output().unwrap();
            let err_path = ex_dir.join("expected").join(format!("{}.err.out", stem));
            if err_path.exists() {
                let expected_err =
                    fs::read_to_string(&err_path).unwrap_or_else(|_| {
                        panic!("missing examples/expected/{}.err.out", stem)
                    });
                assert_eq!(
                    run.status.code(),
                    Some(70),
                    "exit code mismatch for example {}",
                    stem
                );
                assert_eq!(
                    String::from_utf8_lossy(&run.stderr),
                    expected_err,
                    "stderr mismatch for example {}",
                    stem
                );
            } else {
                assert!(
                    run.status.success(),
                    "example {} failed at runtime:\nstdout: {}\nstderr: {}",
                    stem,
                    String::from_utf8_lossy(&run.stdout),
                    String::from_utf8_lossy(&run.stderr)
                );
                let expected = fs::read_to_string(
                    ex_dir.join("expected").join(format!("{}.out", stem)),
                )
                .unwrap_or_else(|_| panic!("missing examples/expected/{}.out", stem));
                assert_eq!(
                    String::from_utf8_lossy(&run.stdout),
                    expected,
                    "output mismatch for example {}",
                    stem
                );
            }
        }
        checked += 1;
    }
    assert!(checked >= 2, "expected at least 2 examples, found {}", checked);
}
