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
    let ex_dir = root.join("examples/features");
    let ext = jet::Syntax::FILE_EXT;
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
                format!("examples/features/{}.{}", stem, ext),
            ));
        } else if path.is_dir() {
            let main = path.join(format!("main.{}", ext));
            if main.is_file() {
                let stem = path.file_name().unwrap().to_string_lossy().into_owned();
                entries.push((
                    main.clone(),
                    stem,
                    format!(
                        "examples/features/{}/main.{}",
                        path.file_name().unwrap().to_string_lossy(),
                        ext
                    ),
                ));
            }
        }
    }
    entries.sort_by(|a, b| a.1.cmp(&b.1));

    let mut checked = 0;
    for (path, stem, shown) in entries {
        let src = fs::read_to_string(&path).unwrap();

        if stem == "22_ffi" && !have_cargo {
            eprintln!("note: skipping examples/features/22_ffi.jet golden (need cargo for FFI bridge)");
            checked += 1;
            continue;
        }

        let compiled = match jet::compile_with_path(&src, &shown) {
            Ok(c) => c,
            Err(diags) => panic!(
                "example {} failed the front end:\n{}",
                stem,
                jet::render_diagnostics(&format!("examples/features/{}.{}", stem, ext), &src, &diags)
            ),
        };
        let rust_code = compiled.rust;
        let ffi_link = compiled.ffi;

        // I1 (amended by D-LL1, E2-M13): memory safety is never traded away in
        // ordinary Jet. Generated `unsafe` appears ONLY inside the gated
        // low-level tier (`use core.mem` + `#Unsafe`) and the vetted Core `mem`
        // arena helper (`mod jet_mem`, D-ALLOC2 — the one lifetime-extension
        // `unsafe`, always emitted as part of the prelude). Either way every
        // `unsafe` must be a *gated* form (`unsafe {` or `unsafe fn`) — never a
        // bare `unsafe` leaking memory safety. We check the gated-form rule on
        // the user code; the fixed `jet_mem` prelude block is excluded since it
        // is the audited helper, not example output.
        let user_code: String = {
            // Drop the `mod jet_mem { … }` block (brace-matched) before scanning.
            if let Some(start) = rust_code.find("mod jet_mem") {
                let bytes = rust_code.as_bytes();
                let mut depth = 0usize;
                let mut i = start;
                let mut end = rust_code.len();
                let mut seen_brace = false;
                while i < bytes.len() {
                    match bytes[i] {
                        b'{' => { depth += 1; seen_brace = true; }
                        b'}' => {
                            depth -= 1;
                            if seen_brace && depth == 0 { end = i + 1; break; }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                let mut s = rust_code[..start].to_string();
                s.push_str(&rust_code[end..]);
                s
            } else {
                rust_code.clone()
            }
        };
        if stem == "48_lowlevel" {
            assert!(
                user_code.contains("unsafe"),
                "the low-level example should exercise the gated `unsafe` tier"
            );
            // Even in the audited example, every `unsafe` is a gated form.
            for (i, line) in user_code.lines().enumerate() {
                if let Some(col) = line.find("unsafe") {
                    let after = line[col..].trim_start_matches("unsafe");
                    let after = after.trim_start();
                    assert!(
                        after.starts_with('{') || after.starts_with("fn "),
                        "48_lowlevel emits an ungated `unsafe` at line {}: {}",
                        i + 1,
                        line.trim()
                    );
                }
            }
        } else {
            // Every other example's user code is fully safe — the only `unsafe`
            // in the file is the vetted `jet_mem` arena helper, already excluded.
            assert!(
                !user_code.contains("unsafe"),
                "generated Rust for {} contains `unsafe` outside the vetted `jet_mem` helper",
                stem
            );
        }
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
                rustc_cmd.arg("--extern").arg(format!(
                    "{}={}",
                    link.crate_name,
                    link.rlib_path.display()
                ));
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
                let expected_err = fs::read_to_string(&err_path)
                    .unwrap_or_else(|_| panic!("missing examples/features/expected/{}.err.out", stem));
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
                let expected =
                    fs::read_to_string(ex_dir.join("expected").join(format!("{}.out", stem)))
                        .unwrap_or_else(|_| panic!("missing examples/features/expected/{}.out", stem));
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
    assert!(
        checked >= 2,
        "expected at least 2 examples, found {}",
        checked
    );
}
