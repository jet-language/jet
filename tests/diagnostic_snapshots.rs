//! Snapshot tests for every user-facing diagnostic (invariant I4): errors
//! (tests/ui/*.jet + .stderr) and ownership lints (tests/ui_lint/*.jet +
//! .warn), both driven by the same `UPDATE_EXPECT` snapshot harness.
//!
//! Each tests/ui/NAME.jet has a sibling NAME.stderr holding the exact
//! rendered output; each tests/ui_lint/NAME.jet has a sibling NAME.warn.
//! To update after an INTENTIONAL wording change:
//!
//!     JET_UI_FILTER=tests/ui/NAME.jet UPDATE_EXPECT=tests/ui/NAME.jet \
//!       cargo test --test diagnostic_snapshots ui_snapshots
//!
//! `UPDATE_EXPECT=1` remains the explicit bless-all mode for a reviewed sweep.
//!
//! Never bless a snapshot you haven't read against docs/spec/diagnostics.md.
//! These files are the product: the error messages ARE the language's UX.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

mod common;
use common::{
    fixture_filter, fixture_matches, jetpack_bin, normalize_fixture_selector, unified_diff,
    unique_tmp,
};

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

    let filter = fixture_filter("JET_UI_FILTER");
    entries.retain(|(_, shown)| fixture_matches(filter.as_deref(), shown));
    assert!(
        !entries.is_empty(),
        "JET_UI_FILTER matched no error fixtures: {}",
        filter.as_deref().unwrap_or("<unfiltered>")
    );
    let update = update_selector();
    validate_scoped_update(&update, entries.iter().map(|(_, shown)| shown.as_str()));

    let mut checked = 0;
    let mut failures = Vec::new();
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
        let repl_deny = src.lines().any(|l| l.trim() == "// @repl_deny");
        // Compiler/tool-generated Jet may use the reserved `__name` lane.
        // Ordinary fixtures never take this path.
        let generated_source = src.lines().any(|l| l.trim() == "// @generated_source")
            && path.parent() == Some(dir.as_path())
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("generated_cffi_"));
        // D-PLUGIN1=B (c81): files marked `// @plugin_target` compile via
        // `jet build --target=plugin`'s front end so plugin-only diagnostics
        // (E1257-E1260) can exercise the gate.
        let plugin_target = src.lines().any(|l| l.trim() == "// @plugin_target");
        // D-WEBTIR1=A: files marked `// @web_target` compile through the web
        // preflight so web-only executable-body diagnostics get UI snapshots.
        let web_target = src.lines().any(|l| l.trim() == "// @web_target");
        // D-BUILDENTRY1: selected-root build diagnostics need programmable
        // staging, not ordinary runtime compilation.
        let programmable_build = src.lines().any(|l| l.trim() == "// @programmable_build");
        let build_grants = src
            .lines()
            .find_map(|line| line.trim().strip_prefix("// @build_grants "))
            .map(|list| list.split(',').map(|item| item.trim().to_string()).collect::<Vec<_>>())
            .unwrap_or_default();
        let build_locked = src.lines().any(|line| line.trim() == "// @build_locked");
        // Hangar diagnostics originate in the real Jetpack command surface,
        // not Jet source compilation. This directive runs that command against
        // an isolated root so its exact user-facing output remains I4-pinned.
        let jetpack_hangar_digest_mismatch = src
            .lines()
            .any(|line| line.trim() == "// @jetpack_hangar_digest_mismatch");
        // D-DX5-HOOK1 / Tower #549: `// @compiler_extension <repo-relative.wasm>`
        // sets JET_COMPILER_EXTENSION for this fixture only (no user syntax).
        let compiler_extension = src.lines().find_map(|line| {
            line.trim()
                .strip_prefix("// @compiler_extension ")
                .map(|p| p.trim().to_string())
        });
        if let Some(ref rel) = compiler_extension {
            let wasm = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
            std::env::set_var(
                "JET_COMPILER_EXTENSION",
                wasm.to_str().expect("utf-8 wasm path"),
            );
        } else {
            std::env::remove_var("JET_COMPILER_EXTENSION");
        }
        let actual = if jetpack_hangar_digest_mismatch {
            run_jetpack_hangar_digest_mismatch_snapshot()
        } else if programmable_build {
            let result = if build_locked {
                jet::compile_programmable_build_opts(
                    &file_arg,
                    &build_grants,
                    false,
                    true,
                    true,
                    false,
                    false,
                    None,
                )
            } else {
                jet::compile_programmable_build(&file_arg, &build_grants)
            };
            match result {
                Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
                Ok(_) => "(no errors)\n".to_string(),
            }
        } else if repl_deny {
            let input = src
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .filter(|line| !line.trim().is_empty())
                .chain(std::iter::once(":quit"))
                .collect::<Vec<_>>()
                .join("\n");
            let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
                .args(["repl", "--deny-fs"])
                .env("NO_COLOR", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .expect("start real REPL diagnostic fixture");
            child
                .stdin
                .as_mut()
                .expect("REPL stdin")
                .write_all(input.as_bytes())
                .expect("write REPL diagnostic fixture");
            let output = child.wait_with_output().expect("finish REPL diagnostic fixture");
            assert!(output.status.success(), "REPL diagnostic fixture failed to exit");
            String::from_utf8(output.stderr).expect("REPL diagnostic stderr is UTF-8")
        } else if generated_source {
            match jet::Driver::compile_generated_src(
                &src,
                &file_arg,
                jet::Sema::CompileMode::Check,
            ) {
                Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
                Ok(_) => "(no errors)\n".to_string(),
            }
        } else if all_diags {
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
        } else if web_target {
            match jet::compile_web(&file_arg) {
                Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
                Ok(_) => "(no errors)\n".to_string(),
            }
        } else {
            match jet::compile_with_path(&src, &file_arg) {
                Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
                Ok(_) => "(no errors)\n".to_string(),
            }
        };
        if compiler_extension.is_some() {
            std::env::remove_var("JET_COMPILER_EXTENSION");
        }
        let actual = normalize_volatile_ui_snapshot(&shown_path, actual);

        let expect_path = if path.file_name().unwrap() == "main.jet" {
            path.parent().unwrap().join("stderr")
        } else {
            path.with_extension("stderr")
        };
        if should_update(&update, &shown_path) {
            fs::write(&expect_path, &actual).unwrap();
        } else {
            let expected = fs::read_to_string(&expect_path).unwrap_or_default();
            if actual != expected {
                failures.push(format!(
                    "ui snapshot mismatch for {shown_path}\n{}",
                    unified_diff(
                        &expect_path.to_string_lossy(),
                        &format!("{shown_path} (actual)"),
                        &expected,
                        &actual,
                    )
                ));
            }
        }
        checked += 1;
    }
    assert!(
        failures.is_empty(),
        "snapshot mismatches:\n\n{}\n\nTo update one reviewed fixture, use UPDATE_EXPECT=<canonical-relative-name> with JET_UI_FILTER=<same-name>.",
        failures.join("\n\n")
    );
    assert!(
        filter.is_some() || checked >= 7,
        "expected the ui suite to contain tests, found {}",
        checked
    );
}

fn run_jetpack_hangar_digest_mismatch_snapshot() -> String {
    let scratch = unique_tmp("jet_ui_hangar_digest_mismatch");
    let root = scratch.join("root");
    let project = scratch.join("project");
    let dependency = scratch.join("dependency");
    let source = scratch.join("source");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&dependency).unwrap();
    fs::create_dir_all(&source).unwrap();
    fs::write(dependency.join("payload"), "dependency bytes\n").unwrap();
    fs::write(source.join("payload"), "trusted bytes\n").unwrap();

    let roots = jetpack::Store::Roots {
        root: root.clone(),
        dev_mode: false,
    };
    let ingest = |name: &str, output: PathBuf, references| {
        jetpack::Store::ingest_tree(
            &roots,
            &jetpack::Store::IngestRequest {
                name: name.to_string(),
                version: String::new(),
                reference: format!("path:{}", output.display()),
                cache_identity: jetpack::Store::CacheIdentity::default(),
                references,
                outputs: std::collections::BTreeMap::from([("out".to_string(), output)]),
                signature: String::new(),
                provenance: String::new(),
                platform_artifact_kind: String::new(),
            },
        )
        .expect("seed valid Hangar closure for diagnostic fixture")
    };
    let dependency = ingest("ui-e1315-dependency", dependency, Vec::new());
    let ingested = ingest(
        "ui-e1315",
        source,
        vec![dependency.entry.envelope.output_hash],
    );
    let digest = ingested.entry.envelope.output_hash;

    let object = root.join("hangar/objects").join(&digest);
    let payload = object.join("payload");
    for path in [&object, &payload] {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).unwrap();
    }
    fs::write(&payload, "tampered bytes\n").unwrap();

    let output = Command::new(jetpack_bin())
        .args(["hangar", "verify", &digest, "--no-color"])
        .current_dir(&project)
        .env("JETPACK_ROOT", &root)
        .output()
        .expect("run real jetpack hangar verify diagnostic fixture");
    let _ = fs::remove_dir_all(&scratch);
    assert_eq!(
        output.status.code(),
        Some(2),
        "tampered Hangar object must fail verification: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).expect("Jetpack diagnostic is UTF-8");
    let start = stderr
        .find("\n  error[E1315]")
        .expect("real hangar verify must emit E1315");
    stderr[start..].to_string()
}

fn normalize_volatile_ui_snapshot(shown_path: &str, actual: String) -> String {
    let actual = actual
        .lines()
        .filter(|line| !line.contains("Blocking waiting for file lock on package cache"))
        .map(|line| {
            let mut line = line.to_string();
            line.push('\n');
            line
        })
        .collect::<String>();
    if shown_path.ends_with("ffi_fetch_failed.jet")
        && actual.contains("Error [E0704]")
        && actual.contains("not-a-real-crate-xyz@9.9.9")
        && cargo_index_unavailable(&actual)
    {
        return fs::read_to_string("tests/ui/ffi_fetch_failed.stderr")
            .expect("ffi_fetch_failed.stderr must exist");
    }
    actual
}

fn cargo_index_unavailable(actual: &str) -> bool {
    actual.contains("download of config.json failed")
        || actual.contains("failed to download from `https://index.crates.io/config.json`")
        || actual.contains("spurious network error")
        || actual.contains("Could not resolve host: index.crates.io")
        || actual.contains("Resolving timed out")
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

    let filter = fixture_filter("JET_UI_FILTER");
    entries.retain(|path| {
        let shown = format!(
            "tests/ui_lint/{}",
            path.file_name().unwrap().to_string_lossy()
        );
        fixture_matches(filter.as_deref(), &shown)
    });
    assert!(
        !entries.is_empty(),
        "JET_UI_FILTER matched no lint fixtures: {}",
        filter.as_deref().unwrap_or("<unfiltered>")
    );
    let update = update_selector();
    let shown_entries: Vec<String> = entries
        .iter()
        .map(|path| {
            format!(
                "tests/ui_lint/{}",
                path.file_name().unwrap().to_string_lossy()
            )
        })
        .collect();
    validate_scoped_update(&update, shown_entries.iter().map(String::as_str));

    let mut checked = 0;
    let mut failures = Vec::new();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(&path).unwrap();
        let shown_path = format!("tests/ui_lint/{}", name);

        // D-DX5-HOOK1: optional `// @compiler_extension <repo-relative.wasm>`.
        let compiler_extension = src.lines().find_map(|line| {
            line.trim()
                .strip_prefix("// @compiler_extension ")
                .map(|p| p.trim().to_string())
        });
        if let Some(ref rel) = compiler_extension {
            let wasm = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
            std::env::set_var(
                "JET_COMPILER_EXTENSION",
                wasm.to_str().expect("utf-8 wasm path"),
            );
        } else {
            std::env::remove_var("JET_COMPILER_EXTENSION");
        }

        let out = jet::compile_with_path(&src, &path.to_string_lossy()).unwrap_or_else(|diags| {
            if compiler_extension.is_some() {
                std::env::remove_var("JET_COMPILER_EXTENSION");
            }
            panic!(
                "lint fixture {} must compile:\n{}",
                name,
                jet::render_diagnostics(&shown_path, &src, &diags)
            );
        });
        if compiler_extension.is_some() {
            std::env::remove_var("JET_COMPILER_EXTENSION");
        }
        assert!(
            !out.lints.is_empty(),
            "lint fixture {} should emit at least one lint",
            name
        );

        let actual = jet::render_diagnostics(&shown_path, &src, &out.lints);
        let expect_path = path.with_extension("warn");
        if should_update(&update, &shown_path) {
            fs::write(&expect_path, &actual).unwrap();
        } else {
            let expected = fs::read_to_string(&expect_path).unwrap_or_default();
            if actual != expected {
                failures.push(format!(
                    "lint snapshot mismatch for {shown_path}\n{}",
                    unified_diff(
                        &expect_path.to_string_lossy(),
                        &format!("{shown_path} (actual)"),
                        &expected,
                        &actual,
                    )
                ));
            }
        }
        checked += 1;
    }
    assert!(
        failures.is_empty(),
        "snapshot mismatches:\n\n{}\n\nTo update one reviewed fixture, use UPDATE_EXPECT=<canonical-relative-name> with JET_UI_FILTER=<same-name>.",
        failures.join("\n\n")
    );
    assert!(
        filter.is_some() || checked >= 2,
        "expected lint fixtures, found {}",
        checked
    );
}

#[derive(Clone)]
enum UpdateSelector {
    None,
    All,
    One(String),
}

fn update_selector() -> UpdateSelector {
    match std::env::var("UPDATE_EXPECT") {
        Err(_) => UpdateSelector::None,
        Ok(value) if value == "1" => UpdateSelector::All,
        Ok(value) => UpdateSelector::One(normalize_fixture_selector("UPDATE_EXPECT", &value)),
    }
}

fn validate_scoped_update<'a>(update: &UpdateSelector, paths: impl Iterator<Item = &'a str>) {
    let UpdateSelector::One(selector) = update else {
        return;
    };
    let matches: Vec<&str> = paths.filter(|path| path.contains(selector)).collect();
    assert_eq!(
        matches.len(),
        1,
        "UPDATE_EXPECT={selector} must match exactly one fixture; matched: {matches:?}"
    );
}

fn should_update(update: &UpdateSelector, shown_path: &str) -> bool {
    match update {
        UpdateSelector::None => false,
        UpdateSelector::All => true,
        UpdateSelector::One(selector) => shown_path.contains(selector),
    }
}
