//! Snapshot tests for every user-facing diagnostic (invariant I4): errors
//! (tests/ui/*.jet + .stderr) and ownership lints (tests/ui_lint/*.jet +
//! .warn), both driven by the same `UPDATE_EXPECT` snapshot harness.
//!
//! Each flat tests/ui/NAME.jet has a sibling NAME.stderr holding the exact
//! rendered output. A directory fixture uses `run.jet` or legacy `main.jet`
//! plus `stderr`, or
//! `workspace.jet` + tests/ui/NAME.stderr. Each tests/ui_lint/NAME.jet has a
//! sibling NAME.warn.
//! To update after an INTENTIONAL wording change:
//!
//!     JET_UI_FILTER=tests/ui/NAME.jet UPDATE_EXPECT=tests/ui/NAME.jet \
//!       cargo test --test diagnostic_snapshots ui_snapshots
//!
//! `UPDATE_EXPECT=1` remains the explicit bless-all mode for a reviewed sweep.
//!
//! Never bless a snapshot you haven't read against the typed diagnostic row.
//! These files are the product: the error messages ARE the language's UX.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

mod common;
use common::{
    fixture_filter, fixture_matches, jetpack_bin, normalize_fixture_selector, unified_diff,
    unique_tmp,
};

/// Serialize `JET_COMPILER_EXTENSION` mutations across UI/lint snapshot
/// threads (same race the driver unit tests already lock).
fn compiler_extension_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct CompilerExtensionEnvRestore(Option<std::ffi::OsString>);

impl Drop for CompilerExtensionEnvRestore {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var("JET_COMPILER_EXTENSION", value),
            None => std::env::remove_var("JET_COMPILER_EXTENSION"),
        }
    }
}

fn compiler_extension_env(
    compiler_extension: Option<&str>,
) -> (std::sync::MutexGuard<'static, ()>, CompilerExtensionEnvRestore) {
    let guard = compiler_extension_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let restore = CompilerExtensionEnvRestore(std::env::var_os("JET_COMPILER_EXTENSION"));
    match compiler_extension {
        Some(rel) => {
            let wasm = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
            std::env::set_var("JET_COMPILER_EXTENSION", wasm.to_str().expect("utf-8 wasm path"));
        }
        None => std::env::remove_var("JET_COMPILER_EXTENSION"),
    }
    (guard, restore)
}

#[test]
fn compiler_extension_env_lock_covers_plain_compile_regions() {
    let held = compiler_extension_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (attempting_tx, attempting_rx) = std::sync::mpsc::channel();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        attempting_tx.send(()).unwrap();
        let _scope = compiler_extension_env(None);
        entered_tx.send(()).unwrap();
    });
    attempting_rx.recv().unwrap();
    assert!(
        entered_rx.recv_timeout(std::time::Duration::from_millis(50)).is_err(),
        "plain compile region entered while compiler-extension env lock was held"
    );
    drop(held);
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("plain compile region should enter after env lock release");
    worker.join().unwrap();
}

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
            let entry = ["run", "main"]
                .into_iter()
                .map(|name| path.join(format!("{name}.{ext}")))
                .find(|candidate| candidate.is_file());
            if let Some(entry) = entry {
                let entry_name = entry.file_name().unwrap().to_string_lossy().into_owned();
                let rel = format!(
                    "tests/ui/{}/{}",
                    path.file_name().unwrap().to_string_lossy(),
                    entry_name
                );
                entries.push((entry, rel));
            } else {
                let workspace = path.join(jet::Syntax::WORKSPACE_FILE);
                if workspace.is_file() {
                    let rel = format!(
                        "tests/ui/{}/{}",
                        path.file_name().unwrap().to_string_lossy(),
                        jet::Syntax::WORKSPACE_FILE
                    );
                    entries.push((workspace, rel));
                }
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
        let render_path = if path.file_name().and_then(|name| name.to_str())
            == Some(jet::Syntax::WORKSPACE_FILE)
        {
            jet::Syntax::WORKSPACE_FILE.to_string()
        } else {
            shown_path.clone()
        };
        // E2-M15: files marked with `// @freestanding` are compiled with
        // the freestanding profile (E3301 checks enabled).
        let freestanding = src.lines().any(|l| l.trim() == "// @freestanding");
        // I4: `// @all_diags` and workspace fixtures run `check_with_path`.
        let all_diags = src.lines().any(|l| l.trim() == "// @all_diags")
            || path.file_name().and_then(|name| name.to_str())
                == Some(jet::Syntax::WORKSPACE_FILE);
        // D-ONCE-GATE1=A: files marked with the invocation gate exercise the
        // same audited policy path as the CLI.
        let gates = src.lines().any(|l| l.trim() == "// @gate impure=allow");
        let repl_deny = src.lines().any(|l| l.trim() == "// @repl_deny");
        // Runtime/interpreter diagnostics still use the same exact snapshot
        // product contract as front-end diagnostics.
        let dev_interpreter = src
            .lines()
            .any(|l| l.trim() == "// @dev_interpreter");
        // D-CANCELMODEL1: parent-control cancellation is produced by a live
        // task wait, so this fixture renders the shared Prelude diagnostic at
        // the representative wait expression without inventing user syntax.
        let parent_control_cancel = src
            .lines()
            .any(|l| l.trim() == "// @parent_control_cancel");
        // Compiler/tool-generated Jet may use the reserved `__name` lane.
        // Ordinary fixtures never take this path.
        let generated_source = src.lines().any(|l| l.trim() == "// @generated_source")
            && path.parent() == Some(dir.as_path())
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("generated_cffi_"));
        // D-PLUGIN1=B (c81): files marked `// @plugin_target` compile via
        // `jet build --target=sandbox`'s front end so sandbox-only diagnostics
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
        // D-ENV-FACET1 / E1337: environment module selection is an
        // environment-model diagnostic, so this fixture drives that same
        // evaluator while retaining the ordinary UI snapshot contract.
        let env_facet_missing = src
            .lines()
            .any(|line| line.trim() == "// @env_facet_missing");
        // D-ENVFLAG1 / E1342: the retired compound selector is a teaching
        // diagnostic from the real Jetpack command surface.
        let jetpack_retired_environment_flag = src
            .lines()
            .any(|line| line.trim() == "// @jetpack_retired_environment_flag");
        let retired_gate_flag = src
            .lines()
            .any(|line| line.trim() == "// @retired_gate_flag");
        let cli_e0043 = src.lines().any(|line| line.trim() == "// @cli_e0043");
        let cli_e1219 = src.lines().any(|line| line.trim() == "// @cli_e1219");
        let typed_settings_cli = src
            .lines()
            .any(|line| line.trim() == "// @typed_settings_cli");
        // Card #1748: this flat fixture carries a manifest sample in a test
        // directive so the intentional retired spelling is not a live
        // `package.jet` counted by the migration ratchet.
        let lint_policy_config = src.lines().find_map(|line| {
            line.trim()
                .strip_prefix("// @lint_policy_config ")
                .map(str::to_owned)
        });
        // D-WORKSPACELOCK1 / E1202: persisted workspace identity failures
        // use the same registered diagnostic in tooling and CLI paths.
        let workspace_lock_e1202 = src
            .lines()
            .any(|line| line.trim() == "// @workspace_lock_e1202");
        // Card #1421 c2 / D-LIB-REUSE1=B / E1338: a `.jetlib` artifact's
        // compiler-identity stamp is checked before mapping. This fixture
        // drives the shared stamp check directly against a fixture stamp.
        let jetlib_version_mismatch = src
            .lines()
            .any(|line| line.trim() == "// @jetlib_version_mismatch");
        // Card #1421 c3 / D-LIB-DYNTRUST1=A / E1339: a `.jetlib` artifact's
        // declared effects are checked against the load site's grant before
        // mapping. This fixture drives the shared grant check directly.
        let jetlib_effect_refused = src
            .lines()
            .any(|line| line.trim() == "// @jetlib_effect_refused");
        // Card #1421 / E1341: the Library resolver must reject an unknown
        // binding instead of silently dropping it during code generation.
        let library_invalid_binding = src
            .lines()
            .any(|line| line.trim() == "// @library_invalid_binding");
        // D-DX5-HOOK1 / Tower #549: `// @compiler_extension <repo-relative.wasm>`
        // sets JET_COMPILER_EXTENSION for this fixture only (no user syntax).
        let compiler_extension = src.lines().find_map(|line| {
            line.trim()
                .strip_prefix("// @compiler_extension ")
                .map(|p| p.trim().to_string())
        });
        let (cex_lock, cex_restore) = compiler_extension_env(compiler_extension.as_deref());
        let actual = if let Some(manifest_source) = lint_policy_config.as_deref() {
            let diagnostic = jet::Manifest::parse(Path::new("package.jet"), manifest_source)
                .expect_err("the lint-policy code fixture must be refused");
            jet::render_diagnostics(&shown_path, &src, &[diagnostic])
        } else if typed_settings_cli {
            run_typed_settings_cli_snapshot(&file_arg)
        } else if cli_e0043 {
            run_cli_e0043_snapshot()
        } else if cli_e1219 {
            run_cli_e1219_snapshot(&file_arg)
        } else if jetpack_hangar_digest_mismatch {
            run_jetpack_hangar_digest_mismatch_snapshot()
        } else if jetpack_retired_environment_flag {
            run_jetpack_retired_environment_flag_snapshot()
        } else if retired_gate_flag {
            run_retired_gate_flag_snapshot(&file_arg)
        } else if env_facet_missing {
            let diagnostic = jet_env_model::ModuleEval::evaluate_env_with_environment(
                &src,
                path.parent().expect("environment fixture parent"),
                Some("missing"),
            )
            .expect_err("missing environment module fixture must fail");
            jet::render_diagnostics(&shown_path, &src, &[diagnostic])
        } else if workspace_lock_e1202 {
            let lock_path = ".jet/lock";
            let diagnostics = [
                jetpack::Lock::e1202_workspace(lock_path),
                jetpack::Lock::e1202_workspace_write(lock_path, "permission denied"),
            ];
            jet::render_diagnostics(&shown_path, &src, &diagnostics)
        } else if jetlib_version_mismatch {
            let stamp = jetpack::JetLib::JetLibStamp {
                compiler_version: "0.0.1-old".to_string(),
                declared_effects: Default::default(),
            };
            let diagnostic = jetpack::JetLib::check_compiler_identity(&stamp)
                .expect_err("mismatched compiler identity must be refused before mapping");
            jet::render_diagnostics(&shown_path, &src, &[diagnostic])
        } else if jetlib_effect_refused {
            let declared: jetpack::Sema::EffectSet =
                ["Net".to_string()].into_iter().collect();
            let stamp = jetpack::JetLib::JetLibStamp::for_this_compiler(declared);
            let grant: jetpack::Sema::EffectSet = ["FS".to_string()].into_iter().collect();
            let diagnostics = jetpack::JetLib::check_effect_grant("skyhawk", &stamp, &grant)
                .expect_err("an effect outside the grant must be refused before mapping");
            jet::render_diagnostics(&shown_path, &src, &diagnostics)
        } else if library_invalid_binding {
            match jet::compile_library(&file_arg, None) {
                Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
                Ok(_) => "(no errors)\n".to_string(),
            }
        } else if programmable_build {
            let result = if build_locked {
                jet::compile_programmable_build_opts(
                    &file_arg,
                    &build_grants,
                    false,
                    jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
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
        } else if dev_interpreter {
            match jet::Interpreter::dev_iteration(&file_arg, false, true) {
                jet::Interpreter::RunOutcome::Problems(diags) => {
                    jet::render_diagnostics(&shown_path, &src, &diags)
                }
                jet::Interpreter::RunOutcome::Ran {
                    stderr,
                    exit_code,
                    ..
                } if exit_code != 0 => stderr,
                jet::Interpreter::RunOutcome::Ran { .. } => "(no errors)\n".to_string(),
            }
        } else if parent_control_cancel {
            let cancellation = jet::Codegen::task_group::jet_task_cancellation();
            let wait_start = src
                .find("time.sleep")
                .expect("parent-control fixture must name its wait expression");
            let diagnostic = jet::Diagnostics::Diagnostic::error(
                cancellation.code,
                cancellation.what.to_string(),
                cancellation.why.to_string(),
                cancellation.fix.to_string(),
                Some(jet::Diagnostics::Span::new(wait_start, wait_start + "time.sleep".len())),
            );
            jet::render_diagnostics(&shown_path, &src, &[diagnostic])
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
                .env("JET_REPL_HISTORY", "off")
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
                jet::render_diagnostics(&render_path, &src, &diags)
            }
        } else if freestanding {
            match jet::compile_freestanding(&file_arg) {
                Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
                Ok(_) => "(no errors)\n".to_string(),
            }
        } else if gates {
            match jet::compile_with_gates(
                &file_arg,
                jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
            ) {
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
        drop(cex_restore);
        drop(cex_lock);
        let actual = normalize_volatile_ui_snapshot(&shown_path, actual);

        let is_directory_entry = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "main.jet" || name == "run.jet");
        let expect_path = if is_directory_entry {
            path.parent().unwrap().join("stderr")
        } else if path.file_name().and_then(|name| name.to_str())
            == Some(jet::Syntax::WORKSPACE_FILE)
        {
            path.parent()
                .unwrap()
                .parent()
                .unwrap()
                .join(format!(
                    "{}.stderr",
                    path.parent()
                        .unwrap()
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                ))
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

fn run_retired_gate_flag_snapshot(file: &str) -> String {
    let retirement = jet::Syntax::retirement("allow-impure").expect("retired gate row");
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", file])
        .arg(retirement.retired)
        .env("NO_COLOR", "1")
        .output()
        .expect("run retired gate fixture");
    assert!(!output.status.success(), "retired gate flag must fail");
    let mut rendered = String::from_utf8(output.stdout).expect("retired gate stdout is UTF-8");
    rendered.push_str(
        &String::from_utf8(output.stderr).expect("retired gate stderr is UTF-8"),
    );
    rendered
}

fn run_cli_e0043_snapshot() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["install", "--color=never"])
        .env("NO_COLOR", "1")
        .output()
        .expect("run E0043 CLI diagnostic fixture");
    assert!(!output.status.success(), "E0043 command must fail");
    let mut rendered = String::from_utf8(output.stdout).expect("E0043 stdout is UTF-8");
    rendered.push_str(&String::from_utf8(output.stderr).expect("E0043 stderr is UTF-8"));
    rendered
}

fn run_cli_e1219_snapshot(file: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["build", file, "--profile=turbo", "--color=never"])
        .env("NO_COLOR", "1")
        .output()
        .expect("run E1219 CLI diagnostic fixture");
    assert!(!output.status.success(), "E1219 command must fail");
    let mut rendered = String::from_utf8(output.stdout).expect("E1219 stdout is UTF-8");
    rendered.push_str(&String::from_utf8(output.stderr).expect("E1219 stderr is UTF-8"));
    rendered
}

fn run_typed_settings_cli_snapshot(file: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", file, "--set", "cli_only=true", "--color=never"])
        .env("NO_COLOR", "1")
        .output()
        .expect("run typed-settings CLI diagnostic fixture");
    assert!(
        !output.status.success(),
        "undeclared typed setting must fail through the CLI"
    );
    let mut rendered = String::from_utf8(output.stdout).expect("typed-settings stdout is UTF-8");
    rendered.push_str(&String::from_utf8(output.stderr).expect("typed-settings stderr is UTF-8"));
    rendered
}

fn run_jetpack_retired_environment_flag_snapshot() -> String {
    let scratch = unique_tmp("jet_ui_retired_environment_flag");
    let root = scratch.join("root");
    let project = scratch.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("env.jet"),
        "module env.dev { packages: [nixpkgs.ripgrep] }\nmodule env.full { packages: [nixpkgs.fd] }\n",
    )
    .unwrap();
    let output = Command::new(jetpack_bin())
        .args(["enter", "info", "--env-profile", "full", "--no-color"])
        .current_dir(&project)
        .env("JETPACK_ROOT", &root)
        .env("NO_COLOR", "1")
        .output()
        .expect("run real jetpack retired environment flag diagnostic fixture");
    let _ = fs::remove_dir_all(&scratch);
    assert_eq!(
        output.status.code(),
        Some(2),
        "retired environment selector must fail: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stderr).expect("Jetpack diagnostic is UTF-8")
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
        .find("Error [E1315]:")
        .expect("real hangar verify must emit E1315");
    stderr[start..].to_string()
}

fn normalize_volatile_ui_snapshot(shown_path: &str, actual: String) -> String {
    // A snapshot must not depend on where the repository is checked out. A
    // diagnostic that names a file inside its message text (E0921's memory-fact
    // provenance, for one) renders an absolute path; make it repo-relative.
    let root = format!("{}/", env!("CARGO_MANIFEST_DIR"));
    let actual = actual.replace(&root, "");
    // Workspace evaluator tests use `/tmp` as their stable base directory.
    let actual = if let Some(fixture_root) = shown_path.strip_suffix("/workspace.jet") {
        actual.replace(fixture_root, "/tmp")
    } else {
        actual
    };
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
        let (cex_lock, cex_restore) = compiler_extension_env(compiler_extension.as_deref());

        let out = jet::compile_with_path(&src, &path.to_string_lossy()).unwrap_or_else(|diags| {
            panic!(
                "lint fixture {} must compile:\n{}",
                name,
                jet::render_diagnostics(&shown_path, &src, &diags)
            );
        });
        drop(cex_restore);
        drop(cex_lock);
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
