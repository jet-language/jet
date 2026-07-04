//! Snapshot tests for every user-facing diagnostic (invariant I4): errors
//! (tests/ui/*.jet + .stderr) and ownership lints (tests/ui_lint/*.jet +
//! .warn), both driven by the same `UPDATE_EXPECT` snapshot harness.
//!
//! Each tests/ui/NAME.jet has a sibling NAME.stderr holding the exact
//! rendered output; each tests/ui_lint/NAME.jet has a sibling NAME.warn.
//! To update after an INTENTIONAL wording change:
//!
//!     UPDATE_EXPECT=1 cargo test --test diagnostic_snapshots
//!
//! Never bless a snapshot you haven't read against docs/spec/diagnostics.md.
//! These files are the product: the error messages ARE the language's UX.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

// ============================================================================
// Section: error snapshots, tests/ui/*.stderr (was tests/ui.rs)
// ============================================================================

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
        // D-PLUGIN1=B (c81): files marked `// @plugin_target` compile via
        // `jet build --target=plugin`'s front end so plugin-only diagnostics
        // (E1257-E1260) can exercise the gate.
        let plugin_target = src.lines().any(|l| l.trim() == "// @plugin_target");
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
        } else if plugin_target {
            match jet::compile_plugin(&file_arg) {
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

// ============================================================================
// Section: ownership lint snapshots, tests/ui_lint/*.warn (was tests/lint_snapshots.rs)
// ============================================================================

#[test]
fn lint_snapshots() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui_lint");
    let ext = jet::Syntax::FILE_EXT;
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(ext))
        .collect();
    entries.sort();

    let mut checked = 0;
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(&path).unwrap();
        let shown_path = format!("tests/ui_lint/{}", name);

        let out = jet::compile_with_path(&src, &path.to_string_lossy()).unwrap_or_else(|diags| {
            panic!(
                "lint fixture {} must compile:\n{}",
                name,
                jet::render_diagnostics(&shown_path, &src, &diags)
            );
        });
        assert!(
            !out.lints.is_empty(),
            "lint fixture {} should emit at least one lint",
            name
        );

        let actual = jet::render_diagnostics(&shown_path, &src, &out.lints);
        let expect_path = path.with_extension("warn");
        if std::env::var("UPDATE_EXPECT").is_ok() {
            fs::write(&expect_path, &actual).unwrap();
        } else {
            let expected = fs::read_to_string(&expect_path).unwrap_or_default();
            assert_eq!(
                actual, expected,
                "\nlint snapshot mismatch for {}\n(run: UPDATE_EXPECT=1 cargo test --test diagnostic_snapshots)\n",
                name
            );
        }
        checked += 1;
    }
    assert!(checked >= 2, "expected lint fixtures, found {}", checked);
}
