//! Snapshot tests for every user-facing diagnostic (invariant I4).
//!
//! Each tests/ui/NAME.jet has a sibling NAME.stderr holding the exact
//! rendered output. To update after an INTENTIONAL wording change:
//!
//!     UPDATE_EXPECT=1 cargo test
//!
//! Never bless a snapshot you haven't read against docs/spec/diagnostics.md.
//! These files are the product: the error messages ARE the language's UX.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn ui_snapshots() {
    let have_cargo = Command::new("cargo").arg("--version").output().is_ok();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui");
    let ext = jet::Syntax::FILE_EXT;
    let mut entries: Vec<(PathBuf, String)> = Vec::new();
    for e in fs::read_dir(&dir).unwrap().flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) == Some(ext) {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if !name.contains(".fixed.") {
                entries.push((path, format!("tests/ui/{}", name)));
            }
        } else if path.is_dir() {
            let main = path.join(format!("main.{}", ext));
            if main.is_file() {
                let rel = format!(
                    "tests/ui/{}/main.{}",
                    path.file_name().unwrap().to_string_lossy(),
                    ext
                );
                entries.push((main, rel));
            }
        }
    }
    entries.sort_by(|a, b| a.1.cmp(&b.1));

    let mut checked = 0;
    for (path, shown_path) in entries {
        if shown_path.contains("ffi_bad_path") && !have_cargo {
            eprintln!(
                "note: skipping {} ui snapshot (need cargo for E0705)",
                shown_path
            );
            checked += 1;
            continue;
        }
        if shown_path.contains("ffi_fetch_failed") && !have_cargo {
            eprintln!(
                "note: skipping {} ui snapshot (need cargo for E0704)",
                shown_path
            );
            checked += 1;
            continue;
        }
        if shown_path.contains("ffi_no_cargo") && have_cargo {
            eprintln!(
                "note: skipping {} ui snapshot (need no cargo for E0703)",
                shown_path
            );
            checked += 1;
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();

        // D-MIGRATE1: if a sibling NAME.published.snapshot exists, install it in a temp
        // dir and set JET_SCHEMA_CACHE_DIR so the schema diff pass can find the prior snapshot.
        let snap_path = path.with_extension("published.snapshot");
        if snap_path.is_file() {
            let snap_text = fs::read_to_string(&snap_path).unwrap();
            // Extract type name from the snapshot.
            let type_name = snap_text
                .lines()
                .find(|l| l.starts_with("type = "))
                .and_then(|l| l.strip_prefix("type = "))
                .unwrap_or("Unknown")
                .trim()
                .to_string();
            let tmp = std::env::temp_dir().join(format!("jet_schema_ui_{}", std::process::id()));
            fs::create_dir_all(&tmp).ok();
            fs::write(tmp.join(format!("{}.snapshot", type_name)), &snap_text).ok();
            std::env::set_var("JET_SCHEMA_CACHE_DIR", &tmp);
        } else {
            std::env::remove_var("JET_SCHEMA_CACHE_DIR");
        }

        // c129: if a sibling NAME.frozen.api exists, install it in a temp dir and set
        // JET_API_CACHE_DIR so the capability-freeze drift pass (E0912) finds the
        // prior contract.
        let api_path = path.with_extension("frozen.api");
        if api_path.is_file() {
            let api_text = fs::read_to_string(&api_path).unwrap();
            let package = api_text
                .lines()
                .find(|l| l.starts_with("package = "))
                .and_then(|l| l.strip_prefix("package = "))
                .unwrap_or("pkg")
                .trim()
                .to_string();
            let tmp = std::env::temp_dir().join(format!("jet_api_ui_{}", std::process::id()));
            fs::create_dir_all(&tmp).ok();
            fs::write(tmp.join(format!("{}.api", package)), &api_text).ok();
            std::env::set_var("JET_API_CACHE_DIR", &tmp);
        } else {
            std::env::remove_var("JET_API_CACHE_DIR");
        }

        let file_arg = path.to_string_lossy();
        // E2-M15: files marked with `// @freestanding` are compiled with
        // the freestanding profile (E3301 checks enabled).
        let freestanding = src.lines().any(|l| l.trim() == "// @freestanding");
        // I4: files marked with `// @all_diags` run `check_with_path` and include
        // ALL diagnostics (errors + lints) so lint codes can have UI snapshots.
        let all_diags = src.lines().any(|l| l.trim() == "// @all_diags");
        // D-CTEFFECT1: files marked `// @allow_impure` compile with the
        // allow-impure flag so E3411/E3412 snapshots can exercise the gate.
        let allow_impure = src.lines().any(|l| l.trim() == "// @allow_impure");
        let actual = if all_diags {
            let diags = jet::check_with_path(&file_arg);
            if diags.is_empty() {
                "(no errors)\n".to_string()
            } else {
                jet::render_diagnostics(&shown_path, &src, &diags)
            }
        } else if freestanding {
            match jet::compile_freestanding(&file_arg) {
                Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
                Ok(_) => "(no errors)\n".to_string(),
            }
        } else if allow_impure {
            match jet::compile_allow_impure(&file_arg) {
                Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
                Ok(_) => "(no errors)\n".to_string(),
            }
        } else {
            match jet::compile_with_path(&src, &file_arg) {
                Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
                Ok(_) => "(no errors)\n".to_string(),
            }
        };

        let expect_path = if path.file_name().unwrap() == "main.jet" {
            path.parent().unwrap().join("stderr")
        } else {
            path.with_extension("stderr")
        };
        if std::env::var("UPDATE_EXPECT").is_ok() {
            fs::write(&expect_path, &actual).unwrap();
        } else {
            let expected = fs::read_to_string(&expect_path).unwrap_or_default();
            assert_eq!(
                actual, expected,
                "\nui snapshot mismatch for {}\n(if the new output is intentional and matches docs/spec/diagnostics.md, run: UPDATE_EXPECT=1 cargo test)\n",
                shown_path
            );
        }
        checked += 1;
    }
    assert!(
        checked >= 7,
        "expected the ui suite to contain tests, found {}",
        checked
    );
}
