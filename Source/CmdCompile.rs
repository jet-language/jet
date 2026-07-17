//! check / build / run / test / new / fmt / fix subcommand handlers + the
//! rustc bridge.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use jet::ExitCodes;
use jet_foundation::JSON::json_escape;

use crate::{report_problems, usage, BuildProfile, OutputMode, ProfileConfig};

pub(crate) fn run_build_query(command: &str, args: &[&String], mode: OutputMode) {
    let (subject, file) = match command {
        "graph" => (None, args.first().map(|s| s.as_str())),
        "query" if args.first().map(|s| s.as_str()) == Some("build") => {
            (None, args.get(1).map(|s| s.as_str()))
        }
        "explain-build" => (
            args.first().map(|s| s.as_str()),
            args.get(1).map(|s| s.as_str()),
        ),
        _ => (None, None),
    };
    let Some(file) = file else {
        eprintln!("usage: jet {command} {}<file.jet>", if command == "explain-build" { "<target|action|file> " } else { "" });
        exit(ExitCodes::USAGE);
    };
    let src = fs::read_to_string(file).unwrap_or_default();
    let plan = match if command == "query" {
        jet::Driver::evaluate_build_query(file, jet::Driver::BuildQueryExpression::Build)
    } else {
        jet::Driver::query_build_plan(file)
    } {
        Ok(plan) => plan,
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let Some(plan) = plan else {
        if mode.json { println!("{{\"schema_version\":1,\"build\":null}}"); }
        else { println!("default pipeline: no root fn build"); }
        return;
    };
    if let Some(subject) = subject {
        if let Some(explanation) = plan.explain_target_named(subject) {
            print_build_explanation(&explanation, mode.json);
            return;
        }
        if let Some(mut explanation) = plan.explain_action_named(subject) {
            let source_parent = std::path::Path::new(file)
                .parent()
                .unwrap_or(std::path::Path::new("."));
            let project_root = jet::Loader::find_manifest_root(source_parent)
                .unwrap_or_else(|| source_parent.to_path_buf());
            if let Ok(Some(rebuild)) = plan.last_rebuild_explanation(&project_root, subject) {
                explanation.provenance.push(format!("rebuild={}", rebuild.reason));
            }
            print_build_explanation(&explanation, mode.json);
            return;
        }
        print_build_explanation(&plan.explain_file(subject), mode.json);
        return;
    }
    let graph = plan.graph();
    if mode.json {
        println!("{}", jet::Driver::build_plan_json(&plan));
    } else {
        for target in graph.targets { println!("target\t{}\t{:?}", target.name, target.kind); }
        for action in graph.actions { println!("action\t{}\t{}", action.name, action.outputs.join(",")); }
    }
}

fn print_build_explanation(explanation: &jet::Comptime::Build::BuildExplanation, json: bool) {
    if json {
        println!("{{\"schema_version\":1,\"label\":\"{}\",\"provenance\":{}}}", json_escape(&explanation.label), json_strings(&explanation.provenance));
    } else {
        println!("{}", explanation.label);
        for fact in &explanation.provenance { println!("  {fact}"); }
    }
}

fn json_strings(values: &[String]) -> String {
    format!("[{}]", values.iter().map(|value| format!("\"{}\"", json_escape(value))).collect::<Vec<_>>().join(","))
}

/// D-BUILDPROFILE1: load `pkg.jet` build profiles from the project root of `source_file`.
fn load_pkg_profiles(
    source_file: &str,
) -> Option<Vec<jet::PackageManifest::BuildProfileDef>> {
    let src_path = std::path::Path::new(source_file);
    let search_from = src_path.parent().unwrap_or(std::path::Path::new("."));
    let root = jet::Loader::find_manifest_root(search_from)?;
    let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).ok()?;
    jet::PackageManifest::parse(&raw)
        .ok()
        .map(|mf| mf.build_profiles)
}

/// D-BUILDPROFILE1: resolve `--profile=<name>` (or `--release` → `"release"`)
/// to a `BuildProfile`. Manifest entries override blessed defaults for
/// `release`/`debug`/`ci`; unknown names emit E1219 and exit.
fn resolve_named_profile(name: &str, source_file: &str, mode: OutputMode) -> BuildProfile {
    if let Some(profiles) = load_pkg_profiles(source_file) {
        if let Some(def) = profiles.iter().find(|p| p.name == name) {
            return BuildProfile::Named {
                name: name.to_string(),
                config: ProfileConfig::from_def(def),
            };
        }
    }
    match name {
        n if n == jet::Syntax::BUILD_PROFILE_RELEASE => BuildProfile::Release,
        n if n == jet::Syntax::BUILD_PROFILE_DEBUG => BuildProfile::Debug,
        n if n == jet::Syntax::BUILD_PROFILE_CI => BuildProfile::Ci,
        _ => {
            let defined = load_pkg_profiles(source_file)
                .map(|profiles| profiles.into_iter().map(|p| p.name).collect::<Vec<_>>())
                .unwrap_or_default();
            let diag = jet::Manifest::e1219(name, &defined);
            if mode.json {
                eprint!("{}", jet::render_all_json("<cli>", "", &[diag]));
            } else {
                eprint!(
                    "{}",
                    jet::render_all_colored("<cli>", "", &[diag], mode.color_stderr())
                );
            }
            std::process::exit(jet::ExitCodes::USER_ERROR);
        }
    }
}

pub(crate) fn run_compile_cmd(
    cmd: &str,
    file: &str,
    emit_rust: bool,
    small: bool,
    freestanding: bool,
    allow_impure: bool,
    build_grants: &[String],
    locked: bool,
    cross_target: Option<&str>,
    explain_partition: bool,
    verbose: bool,
    capabilities_json: bool,
    sbom: bool,
    named_profile: Option<&str>,
    program_args: &[&String],
    mode: OutputMode,
) {
    // D-BUILDPROFILE1: profile selection. Precedence: --freestanding > --small >
    // --release/--profile=<name> > default. Named profiles are resolved against
    // pkg.jet's `build {}` block; unknown names emit E1219 and exit.
    let profile = if freestanding {
        BuildProfile::Freestanding
    } else if small {
        BuildProfile::Small
    } else if let Some(name) = named_profile {
        resolve_named_profile(name, file, mode)
    } else {
        BuildProfile::Default
    };

    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            eprintln!(
                " fix: check the spelling, or run {} from the folder that contains it",
                jet::Syntax::BINARY_NAME
            );
            exit(ExitCodes::USER_ERROR);
        }
    };

    if cmd == "check" {
        let diags: Vec<_> = jet::check_with_path(file)
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        if !diags.is_empty() {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
        if mode.json {
            println!("{}", jet::render_all_json(file, &src, &[]).trim_end());
        } else {
            println!("ok: `{}` has no problems", file);
        }
        return;
    }

    // E2-M15: validate cross-compilation target before invoking rustc.
    if let Some(triple) = cross_target {
        validate_target(triple, mode);
    }

    let is_web = cross_target == Some(jet::Syntax::BUILD_TARGET_WEB);
    // D-PLUGIN1=B / D-DEP-WASM1=A (c81): `--target=plugin` routes to the
    // sandboxed WASM Component Model guest build instead of a native binary.
    let is_plugin = cross_target == Some(jet::Syntax::TARGET_PLUGIN);
    if explain_partition && !is_web {
        let diag = jet::Diagnostics::Diagnostic::error(
            "E2102",
            "`--explain-partition` requires `--target=web`".to_string(),
            "the partition report is only meaningful for the web backend (D-WASM1)".to_string(),
            format!(
                "run `jet build --target={} --explain-partition <file>`",
                jet::Syntax::BUILD_TARGET_WEB
            ),
            None,
        );
        report_problems(mode, file, &src, &[diag]);
        exit(ExitCodes::USAGE);
    }
    // D-BUILDNORM1=A (Tower #85): compute the content-cache key from the
    // program's canonical *pre-sema* AST, up front. `mode_tag` keeps the three
    // native codegen shapes (plain, `--freestanding`, `--allow-impure`) in
    // separate key spaces. `None` for web/cross builds (they never cache) or an
    // `embed_file` build (external bytes not in the AST) or a parse failure.
    let profile_tag = profile.cache_tag();
    let mode_tag = if freestanding {
        "freestanding"
    } else if allow_impure {
        "impure"
    } else {
        "run"
    };
    let native_key = if !is_web && cross_target.is_none() && (cmd == "build" || cmd == "run") {
        native_cache_key(file, &profile_tag, mode_tag)
    } else {
        None
    };

    // `jet run` short-circuits the whole front end (only the parse inside the
    // key computation ran) when this exact program is already in the content
    // cache. A hit means this program was type-checked, codegen'd, compiled and
    // run once before under this toolchain + profile + manifest — replaying its
    // binary replays a validated result, it never bypasses a check (I2/I3). The
    // toolchain-version + `pkg.jet` salts guarantee a compiler or policy change
    // invalidates the entry, so effect-budget enforcement can't be masked.
    // `jet build` deliberately stays on the full path below so its effect +
    // capability summaries always print; it still skips rustc via `native_key`.
    if cmd == "run" && mode_tag == "run" && !emit_rust {
        if let Some(ref key) = native_key {
            let out = bin_path(file);
            if jet::BuildCache::try_copy_cached(key, &out) {
                if verbose {
                    eprintln!("[build] cache hit -> reused cached binary (front end skipped)");
                }
                let mut run_cmd = Command::new(&out);
                for arg in program_args {
                    run_cmd.arg(arg.as_str());
                }
                let status = run_cmd.status().unwrap_or_else(|e| {
                    eprintln!("error: couldn't run the built program: {}", e);
                    exit(ExitCodes::USER_ERROR);
                });
                exit(status.code().unwrap_or(ExitCodes::OK));
            }
        }
    }

    // D-LINTPOLICY1=A (the override law): visible lints from this compile,
    // captured here (out of the `match` arm's scope) so the `policy.lints`
    // deny-path enforcement below can see them alongside the already-loaded
    // `pkg.jet` manifest.
    #[allow(unused_assignments)]
    let mut visible_lints: Vec<jet::Diagnostics::Diagnostic> = Vec::new();

    let compile_result = if cmd == "build" {
        jet::compile_programmable_build_opts(
            file,
            build_grants,
            freestanding,
            allow_impure || !build_grants.is_empty(),
            locked,
            is_web,
            is_plugin,
            cross_target,
        )
    } else if is_web {
        jet::compile_web(file)
    } else if is_plugin {
        jet::compile_plugin(file)
    } else if freestanding {
        jet::compile_freestanding(file)
    } else if allow_impure {
        jet::compile_allow_impure(file)
    } else {
        // D-OSTARGET1=A: thread the real `--target=<triple>` through so
        // codegen only emits/links `@Target(Os.*)`-gated impls for the OS
        // that triple builds for (host OS when the flag is absent).
        jet::compile_with_target(&src, file, cross_target)
    };
    let (rust_code, ffi_link, clinks, capabilities, web_out, web_partition_report, plugin_out) =
        match compile_result {
            Ok(out) => {
                // D-A11YGATE1=B (c134 Phase 6): a11y lints (E2930/E2931) are opt-in
                // via `jet lint --a11y`; ordinary build/run never surfaces them.
                let lints = crate::CmdDevTools::visible_lints(&out.lints);
                visible_lints = lints.clone();
                if !lints.is_empty() {
                    if mode.json {
                        eprint!("{}", jet::render_all_json(file, &src, &lints));
                    } else {
                        eprint!(
                            "{}",
                            jet::render_all_colored(file, &src, &lints, mode.color_stderr())
                        );
                        let n = lints.len();
                        eprintln!(
                            "\n{} warning{} emitted (compilation continues)",
                            n,
                            if n == 1 { "" } else { "s" }
                        );
                    }
                }
                // S59 (E2-M14): resolve native C link flags at build time; E3201
                // (unresolved C lib) surfaces here, not during front-end checking.
                let clinks = match jet::resolve_c_links(file) {
                    Ok(args) => args,
                    Err(diags) => {
                        report_problems(mode, file, &src, &diags);
                        exit(ExitCodes::USER_ERROR);
                    }
                };
                (
                    out.rust,
                    out.ffi,
                    clinks,
                    out.capabilities,
                    out.web,
                    out.web_partition_report,
                    out.plugin,
                )
            }
            Err(diags) => {
                report_problems(mode, file, &src, &diags);
                exit(ExitCodes::USER_ERROR);
            }
        };

    if emit_rust {
        print!("{}", rust_code);
    }

    // D-EFFBUDGET1: zero-config, always-on effect summary on every build/run,
    // plus opt-in whole-graph enforcement when `pkg.jet` declares `effects:`.
    // The front-end compile above already succeeded; this reruns the
    // check-only pass to pull the whole-program effect fixpoint
    // (`Sema::solve`) that ordinary compilation doesn't need to return.
    if cmd == "build" || cmd == "run" {
        let (_effect_diags, effect_bundle, effect_facts) =
            jet::Driver::check_file_with_effect_facts(file, None, false);
        if let Some(bundle) = &effect_bundle {
            let entries =
                jet::EffectBudget::compute_package_effects(bundle, &effect_facts.solved);
            // D-PLUGIN1=B (c81): a plugin is deny-by-default — the wasmtime
            // host registers zero host imports, so any effect used by the
            // plugin's own code would fail to instantiate at load time. Catch
            // it here, at build time, with a clean diagnostic naming the
            // effect, instead of deferring to that runtime failure (E1258).
            if is_plugin {
                if let Some(root) = entries.iter().find(|p| p.name == "root") {
                    if !root.effects.is_empty() {
                        let diag = jet::Manifest::e1258(&jet::Sema::show_set(&root.effects));
                        report_problems(mode, file, &src, &[diag]);
                        exit(ExitCodes::USER_ERROR);
                    }
                }
            }
            // Program stdout stays the program's (U7 / D-DEVMODE1); tool
            // chatter goes to stderr.
            eprintln!("{}", jet::EffectBudget::summary_line(&entries));
            let search_from = Path::new(file).parent().unwrap_or(Path::new("."));
            if let Some(root) = jet::Loader::find_manifest_root(search_from) {
                let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
                if let Some(manifest) = fs::read_to_string(&pack_path)
                    .ok()
                    .and_then(|raw| jet::PackageManifest::parse(&raw).ok())
                {
                    let violations = jet::EffectBudget::enforce(&entries, &manifest);
                    if !violations.is_empty() {
                        report_problems(mode, file, &src, &violations);
                        exit(ExitCodes::USER_ERROR);
                    }
                    // D-LINTPOLICY1=A (the override law): a team's
                    // `policy.lints.deny` promotes named lints from warnings
                    // to a build failure (E1293); absent entirely, every
                    // lint above already printed as a warning and nothing
                    // blocks (I1/D-LINTPOLICY1 default).
                    let lint_violations =
                        jet::LintPolicy::enforce(&visible_lints, &manifest);
                    if !lint_violations.is_empty() {
                        report_problems(mode, file, &src, &lint_violations);
                        exit(ExitCodes::USER_ERROR);
                    }
                    // Record per-dependency effect provenance + grants in the
                    // lockfile, when one already exists (`jet fetch` owns
                    // creating it).
                    if let Some(mut lock) = jet::Lock::load(&root) {
                        jet::EffectBudget::update_lock_provenance(
                            &mut lock, &entries, &manifest,
                        );
                        let _ = fs::write(
                            jet::PkgStore::lock_path(&root),
                            jet::Lock::write(&lock),
                        );
                    }
                }
            }
        }
    }

    match cmd {
        "build" => {
            let artifact_path=bin_path(file);
            let budget_profile=profile.budget_name().to_string();
            build(
                file,
                &rust_code,
                artifact_path.clone(),
                profile,
                ffi_link.as_ref(),
                &clinks,
                verbose,
                cross_target,
                web_out.as_ref(),
                plugin_out.as_ref(),
                mode,
                native_key.clone(),
            );
            // D-PERFBUDGET-INTEGRATION1: every build enforces applicable
            // deterministic Fail budgets through CmdBudget's one canonical
            // evaluator/report path. Cross backends use their semantic target
            // class; native remains the default current target.
            let budget_target=if is_web{"web"}else if is_plugin{"plugin"}else{"native"};
            if crate::CmdBudget::run_build_gates(file,&artifact_path,budget_target,&budget_profile)!=0{
                exit(ExitCodes::USER_ERROR);
            }
            if is_web {
                println!("built: build/app.wasm + build/app.js");
            } else if is_plugin {
                println!("built: build/{}.wasm (sandboxed plugin)", stem(file));
            } else {
                println!("built: {}", bin_path(file).display());
            }
            if explain_partition {
                if let Some(report) = &web_partition_report {
                    println!("{report}");
                }
            }
            if let Some(triple) = cross_target {
                println!("target: {}", triple);
            }
            // D-SUPPLY1: `--sbom` writes an SPDX SBOM next to the binary.
            if sbom {
                write_sbom_for_build(file, &bin_path(file));
            }
            // D-TOOL5 (E2-M11): print capability summary after a successful build.
            if capabilities_json {
                println!("{}", capabilities.to_json());
            } else {
                println!("{}", capabilities.summary());
            }
        }
        "run" => {
            let out = bin_path(file);
            build(
                file,
                &rust_code,
                out.clone(),
                profile,
                ffi_link.as_ref(),
                &clinks,
                verbose,
                cross_target,
                web_out.as_ref(),
                plugin_out.as_ref(),
                mode,
                native_key.clone(),
            );
            if cross_target.is_some() {
                eprintln!("note: cross-compiled binary cannot run on this host — use emulation (see docs/embedded.md)");
                exit(ExitCodes::OK);
            }
            let mut run_cmd = Command::new(&out);
            for arg in program_args {
                run_cmd.arg(arg.as_str());
            }
            let status = run_cmd.status().unwrap_or_else(|e| {
                eprintln!("error: couldn't run the built program: {}", e);
                exit(ExitCodes::USER_ERROR);
            });
            exit(status.code().unwrap_or(ExitCodes::OK));
        }
        other => {
            eprintln!(
                "error: `{}` isn't a {} command",
                other,
                jet::Syntax::BINARY_NAME
            );
            eprint!("{}", usage());
            exit(ExitCodes::USAGE);
        }
    }
}

/// c-devserver (owner-directed 2026-07-01): `jet dev <file>` when `file`
/// defines a top-level `fn dev()` — compile NATIVELY with `dev()` swapped in
/// as the program's real entry point (`jet::compile_with_entry`), then run the
/// resulting binary exactly like `jet run` does. `dev()`'s own body decides
/// what happens next — normally configuring and starting a `core.web.devserver`
/// value, but it's just an ordinary function; this call site owns none of
/// that behavior (I3: codegen/the driver stay dumb about what `dev()` does).
pub(crate) fn run_dev_entry(file: &str, mode: OutputMode) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            eprintln!(
                " fix: check the spelling, or run {} from the folder that contains it",
                jet::Syntax::BINARY_NAME
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    let out = match jet::compile_with_entry(file, "dev") {
        Ok(out) => out,
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let clinks = match jet::resolve_c_links(file) {
        Ok(args) => args,
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let bin = bin_path(file);
    build(
        file,
        &out.rust,
        bin.clone(),
        BuildProfile::Default,
        out.ffi.as_ref(),
        &clinks,
        false,
        None,
        None,
        None,
        mode,
        // `jet dev` is an interactive live-reload loop (entry-swapped codegen);
        // not worth a content-cache entry. Still race-safe via `build`'s
        // per-process temp path.
        None,
    );
    // `devserver.app()` support: hand the running `dev()` program the
    // canonical absolute path of the file `jet dev` was pointed at, so "the
    // file being run is the file to watch" needs no path spelled out in the
    // Jet source — and works from any invocation directory (a relative
    // string literal in `for_app(...)` only resolves from one cwd).
    let dev_file = fs::canonicalize(file)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| file.to_string());
    // JET_BIN: the devserver's rebuild subprocess must use THIS `jet`, not
    // whatever a bare PATH lookup finds — a different `jet` on PATH could be
    // a different version, and cwd-sensitive wrappers (the repo's own nix
    // devshell `jet` resolves target/debug/jet relative to cwd) break
    // outright when the rebuild runs from a staging directory.
    let jet_bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| jet::Syntax::BINARY_NAME.to_string());
    let status = Command::new(&bin)
        .env("JET_DEV_FILE", &dev_file)
        .env("JET_BIN", &jet_bin)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("error: couldn't run the built program: {}", e);
            exit(ExitCodes::USER_ERROR);
        });
    exit(status.code().unwrap_or(ExitCodes::OK));
}

/// D-JPK-TASKRUN1 (card #476): `jet run --task <name> <file>` — compile with
/// the named `@Task fn` as the entry via a synthetic `fn run { task(…) }`
/// wrapper (same `compile_with_entry` path `fn dev()` uses; the task keeps
/// its source name so plain-call deps stay resolvable), then run the binary
/// with `program_args` (typed CLI args via D-CLIFLAG1 ride for free).
pub(crate) fn run_task_entry(
    file: &str,
    task: &str,
    program_args: &[&String],
    mode: OutputMode,
) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            eprintln!(
                " fix: check the spelling, or run {} from the folder that contains it",
                jet::Syntax::BINARY_NAME
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    let declared = list_task_names(&src);
    let is_marked = match task_is_marked(&src, task) {
        Some(v) => v,
        None => {
            // Parse failed — let compile_with_entry surface real diagnostics.
            true
        }
    };
    if !is_marked {
        let list = if declared.is_empty() {
            "(none)".to_string()
        } else {
            declared.join(", ")
        };
        let diag = jet::Diagnostics::Diagnostic::error(
            "E1294",
            format!("no task named `{task}`"),
            format!("`jet run --task` / `jetpack run` only invoke functions marked `@Task` (D-JPK-TASKRUN1)."),
            "mark a function `@Task` to make it runnable, or check the spelling.".to_string(),
            None,
        )
        .with_detail(format!("declared tasks: {list}\n"));
        report_problems(mode, file, &src, &[diag]);
        exit(ExitCodes::USER_ERROR);
    }
    let out = match jet::compile_with_entry(file, task) {
        Ok(out) => out,
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let clinks = match jet::resolve_c_links(file) {
        Ok(args) => args,
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let bin = bin_path(file);
    build(
        file,
        &out.rust,
        bin.clone(),
        BuildProfile::Default,
        out.ffi.as_ref(),
        &clinks,
        false,
        None,
        None,
        None,
        mode,
        // Task entry-swap is a one-shot; skip content-cache (same as `run_dev_entry`).
        None,
    );
    let mut run_cmd = Command::new(&bin);
    for arg in program_args {
        run_cmd.arg(arg.as_str());
    }
    let status = run_cmd.status().unwrap_or_else(|e| {
        eprintln!("error: couldn't run the built program: {}", e);
        exit(ExitCodes::USER_ERROR);
    });
    exit(status.code().unwrap_or(ExitCodes::OK));
}

/// Cheap lex+parse: names of top-level `@Task fn`s in `src`.
fn list_task_names(src: &str) -> Vec<String> {
    let (toks, lex_diags) = jet::Lexer::lex(src);
    if !lex_diags.is_empty() {
        return Vec::new();
    }
    let Ok(prog) = jet::Parser::parse(&toks) else {
        return Vec::new();
    };
    prog.items
        .iter()
        .filter_map(|i| match i {
            jet::AST::Item::Func(f) if f.is_task => Some(f.name.clone()),
            _ => None,
        })
        .collect()
}

/// `Some(true)` if `name` is a `@Task fn`; `Some(false)` if the file parsed
/// but has no such task; `None` on lex/parse failure.
fn task_is_marked(src: &str, name: &str) -> Option<bool> {
    let (toks, lex_diags) = jet::Lexer::lex(src);
    if !lex_diags.is_empty() {
        return None;
    }
    let prog = jet::Parser::parse(&toks).ok()?;
    Some(prog.items.iter().any(
        |i| matches!(i, jet::AST::Item::Func(f) if f.is_task && f.name == name),
    ))
}

/// D-SUPPLY1 — write an SPDX SBOM next to the freshly built binary.
///
/// Best-effort: an SBOM describes the *dependency* graph, so a single-file
/// program with no project is emitted with just the root component. When a
/// `pkg.jet` and lockfile exist, the SBOM lists every locked dependency with
/// its tree-hash checksum.
fn write_sbom_for_build(file: &str, bin: &Path) {
    let file_path = Path::new(file);
    let search_from = file_path.parent().unwrap_or(Path::new("."));

    // Resolve a name/version + lockfile from the enclosing project, if any.
    let (name, version, lock) = match jet::Loader::find_manifest_root(search_from) {
        Some(root) => {
            let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
            let (n, v) = match fs::read_to_string(&pack_path)
                .ok()
                .and_then(|raw| jet::Manifest::parse(&pack_path, &raw).ok())
            {
                Some(mf) => (mf.package.name, mf.package.version),
                None => (stem(file), "0.0.0".to_string()),
            };
            (n, v, jet::Lock::load(&root))
        }
        None => (stem(file), "0.0.0".to_string(), None),
    };

    let lock = lock.unwrap_or_else(|| jet::Lock::LockFile {
        version: 1,
        packages: Vec::new(),
        root_dependencies: Vec::new(),
        workspace_members: Vec::new(),
        comptime_inputs: Vec::new(),
        toolchains: Vec::new(),
        source_channels: Vec::new(),
    });

    let sbom = jet::Publish::emit_spdx(&lock, &name, &version);
    let out = bin.with_extension("spdx");
    match fs::write(&out, sbom) {
        Ok(()) => println!("sbom: {}", out.display()),
        Err(e) => eprintln!("warning: couldn't write SBOM to {}: {}", out.display(), e),
    }
}

/// Apply all auto-fixable diagnostics in a source file in place (D-LSP7 / M13).
/// Goes through `jet::LSP::collect_fixes` / `apply_all` — the SAME unified fix
/// engine the LSP code-action layer uses — so a fix on the command line and a
/// fix in the editor are byte-identical. `--dry-run` shows the diff without
/// writing.
pub(crate) fn run_fix(file: &str, dry_run: bool) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            eprintln!(" fix: check the spelling");
            exit(ExitCodes::USER_ERROR);
        }
    };
    let fixes = jet::LSP::collect_fixes(file, &src);
    if fixes.is_empty() {
        println!("{}: no auto-fixable problems found", file);
        return;
    }
    let fixed = jet::LSP::apply_all(&src, &fixes);
    if fixed == src {
        println!("{}: no changes made", file);
        return;
    }
    let n = fixes.len();
    if dry_run {
        print!("{}", jet::Formatter::unified_diff(file, &src, &fixed));
        println!(
            "{}: would apply {} fix{} (dry run; nothing written)",
            file,
            n,
            if n == 1 { "" } else { "es" }
        );
        return;
    }
    fs::write(file, &fixed).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", file, e);
        exit(ExitCodes::USER_ERROR);
    });
    println!(
        "{}: applied {} fix{}",
        file,
        n,
        if n == 1 { "" } else { "es" }
    );
}

pub(crate) fn run_new(name: &str, annotated: bool) {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        eprintln!("error: project name must be a simple folder name");
        eprintln!(" fix: try: {} new my_app", jet::Syntax::BINARY_NAME);
        exit(ExitCodes::USER_ERROR);
    }
    let dir = Path::new(name);
    if dir.exists() {
        eprintln!("error: `{}` already exists", name);
        exit(ExitCodes::USER_ERROR);
    }
    // Create: <name>/pkg.jet, <name>/run.jet, <name>/.gitignore
    let jet_dir = dir.join(".jet");
    fs::create_dir_all(&jet_dir).unwrap_or_else(|e| {
        eprintln!("error: couldn't create `{}`/.jet: {}", name, e);
        exit(ExitCodes::USER_ERROR);
    });
    let manifest_text = jet::Manifest::new_template(name, annotated);
    fs::write(dir.join(jet::Syntax::PAYLOAD_FILE), manifest_text).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::Syntax::PAYLOAD_FILE, e);
        exit(ExitCodes::USER_ERROR);
    });
    let run_src = "fn run() {\n    print(\"hello, world\");\n}\n";
    fs::write(dir.join(jet::Syntax::DEFAULT_ENTRY_FILE), run_src).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::Syntax::DEFAULT_ENTRY_FILE, e);
        exit(ExitCodes::USER_ERROR);
    });
    fs::write(
        dir.join(".gitignore"),
        "build/\n.jet-build/\n.jet/lock\n.jet/cache/\n",
    )
    .unwrap_or_else(|e| {
        eprintln!("error: couldn't write .gitignore: {}", e);
        exit(ExitCodes::USER_ERROR);
    });
    println!("created {}/", name);
    println!("  {}", jet::Syntax::PAYLOAD_FILE);
    println!("  {}", jet::Syntax::DEFAULT_ENTRY_FILE);
    println!("  .gitignore");
    println!("next: cd {} && {} run", name, jet::Syntax::BINARY_NAME);
}

/// `jet test` flags beyond the file/dir target (D-TESTKIT1=A gaps #2-#4).
/// Grouped so new flags don't keep growing every `run_test*` signature.
#[derive(Clone, Default)]
pub(crate) struct TestRunOpts {
    pub(crate) update_snapshots: bool,
    pub(crate) coverage: bool,
    /// `--filter=<substr>`: only run tests whose name contains it.
    pub(crate) filter: Option<String>,
    /// `--shuffle` / `--shuffle=<seed>`: reorder tests before running (order-
    /// dependence detection). `None` = source order (the default).
    pub(crate) shuffle_seed: Option<u64>,
    /// `--serial`: run one test at a time instead of the parallel default.
    pub(crate) serial: bool,
}

pub(crate) fn run_test(path: &str, update_snapshots: bool, mode: OutputMode) {
    run_test_opts(
        path,
        TestRunOpts {
            update_snapshots,
            ..Default::default()
        },
        mode,
    )
}

/// `jet test [--coverage] [--filter=<substr>] [--shuffle[=<seed>]] [--serial]`.
/// With `coverage`, the harness is built with line/function probes (D-COV1) and
/// a per-function coverage report prints after the test results. A directory
/// target recurses into every subdirectory (D-TESTKIT1=A gap #2), running every
/// `.jet` file found, in sorted path order.
pub(crate) fn run_test_opts(path: &str, opts: TestRunOpts, mode: OutputMode) {
    let p = Path::new(path);
    if !p.exists() {
        eprintln!("error: can't find `{}`", path);
        exit(ExitCodes::USER_ERROR);
    }
    if p.is_dir() {
        let ext = jet::Syntax::FILE_EXT;
        let mut files: Vec<PathBuf> = Vec::new();
        collect_test_files_recursive(p, ext, &mut files);
        files.sort();
        if files.is_empty() {
            eprintln!("error: no .{} files in `{}` (searched subdirectories too)", ext, path);
            exit(ExitCodes::USER_ERROR);
        }
        let mut any_fail = false;
        for f in files {
            if !run_test_file(&f, &opts, mode) {
                any_fail = true;
            }
        }
        exit(if any_fail {
            ExitCodes::USER_ERROR
        } else {
            ExitCodes::OK
        });
    }
    exit(if run_test_file(p, &opts, mode) {
        ExitCodes::OK
    } else {
        ExitCodes::USER_ERROR
    });
}

/// D-TESTKIT1=A gap #2: walk every subdirectory under `dir`, collecting `.ext`
/// files. `build/` and dotdirs (`.git`, `.jet`'s own cache, etc.) are skipped —
/// a project's build output and VCS metadata are never test sources.
fn collect_test_files_recursive(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "build" || name.starts_with('.') {
                continue;
            }
            collect_test_files_recursive(&path, ext, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
}

fn run_test_file(path: &Path, opts: &TestRunOpts, mode: OutputMode) -> bool {
    let update_snapshots = opts.update_snapshots;
    let coverage = opts.coverage;
    let shown = path.to_string_lossy();
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: couldn't read `{}`: {}", shown, e);
            return false;
        }
    };
    // D-TEST4: discover and run any `///` doctest examples first. They are
    // independent of `@Test` blocks, so a file with only doctests is testable.
    let has_doctests = !jet::Doctest::discover(&src).is_empty();
    let doctests_ok = run_doctests(path, &shown, &src, update_snapshots, mode);

    // A file with doctests but no `@Test` blocks is testable on its doctests
    // alone — skip the test harness (which would otherwise error E0601 "no @Test
    // blocks"). A file with NEITHER falls through so the harness reports E0601.
    if has_doctests && !jet::has_test_blocks(&shown) {
        return doctests_ok;
    }

    let (rust_code, ffi_link) = match jet::compile_tests_with_path_cov(&src, &shown, coverage) {
        Ok(r) => r,
        Err(diags) => {
            report_problems(mode, &shown, &src, &diags);
            return false;
        }
    };
    // Test harnesses are one-shot process-private executables. Concurrent
    // `jet test` invocations may target the same file; sharing
    // `build/test_<stem>` lets one process replace an executable while another
    // is launching it (ETXTBSY on Linux, sharing violations on Windows).
    let bin = test_bin_path(path);
    build(
        &shown,
        &rust_code,
        bin.clone(),
        BuildProfile::Default,
        ffi_link.as_ref(),
        &[],
        false,
        None,
        None,
        None,
        mode,
        // `jet test` caches on the same canonical-AST key, but in its own mode
        // space (the test-harness binary must never be served for a `jet run`);
        // `--coverage` instrumentation is a distinct binary again.
        native_cache_key(
            shown.as_ref(),
            &BuildProfile::Default.cache_tag(),
            if coverage { "testcov" } else { "test" },
        ),
    );
    // D-COV1: run with `JET_COV_OUT` pointing at a temp file; the harness writes
    // the executed-line set there, which we diff against the coverable functions.
    let cov_out = if coverage {
        Some(bin.with_extension("cov"))
    } else {
        None
    };
    let mut cmd = Command::new(&bin);
    // D-TOOL4: `-u`/`--update-snapshots` must reach the harness. Both
    // `expect(…).snapshot()` (any value) and `testing.snap` (`=1`) honor this.
    if update_snapshots {
        cmd.env("JET_UPDATE_SNAPSHOTS", "1");
    }
    if let Some(co) = &cov_out {
        let _ = fs::remove_file(co);
        cmd.env("JET_COV_OUT", co);
    }
    // D-TESTKIT1=A gaps #3/#4: filter/shuffle/serial reach the harness the same
    // way `--coverage`/`-u` do — an env var the generated `main` reads (see
    // `emit_test_main_cov` in jet-codegen).
    if let Some(filter) = &opts.filter {
        cmd.env("JET_TEST_FILTER", filter);
    }
    if let Some(seed) = opts.shuffle_seed {
        cmd.env("JET_TEST_SHUFFLE_SEED", seed.to_string());
    }
    if opts.serial {
        cmd.env("JET_TEST_SERIAL", "1");
    }
    let out = match cmd.output() {
        Ok(out) => out,
        Err(e) => {
            let _ = fs::remove_file(&bin);
            if let Some(co) = &cov_out {
                let _ = fs::remove_file(co);
            }
            eprintln!("error: couldn't run tests in `{}`: {}", shown, e);
            exit(ExitCodes::USER_ERROR);
        }
    };
    print!("{}", String::from_utf8_lossy(&out.stdout));
    if !out.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
    }
    if let Some(co) = &cov_out {
        report_coverage(&shown, co);
        let _ = fs::remove_file(co);
    }
    let _ = fs::remove_file(&bin);
    out.status.success() && doctests_ok
}

/// D-TEST4: compile and run each ```` ```jet ```` doctest block found in the file's
/// `///` doc comments, comparing each `// =>` line to the produced value. A
/// mismatch fires E2901 (or rewrites the claimed output when `update_snapshots`).
/// Returns true when every block (if any) passed. A file with no doctests passes
/// trivially and prints nothing.
fn run_doctests(
    path: &Path,
    shown: &str,
    src: &str,
    update_snapshots: bool,
    mode: OutputMode,
) -> bool {
    let blocks = jet::Doctest::discover(src);
    if blocks.is_empty() {
        return true;
    }
    let mut all_ok = true;
    let mut rewritten = src.to_string();
    let mut did_rewrite = false;
    for (n, block) in blocks.iter().enumerate() {
        let label = format!("doctest at {}:{}", shown, block.fence_line);
        let program = jet::Doctest::synth_program(block);
        // Write the synthetic program to a temp file next to the build dir so the
        // normal compile+build pipeline can consume it.
        let _ = fs::create_dir_all("build");
        let tmp = PathBuf::from("build").join(format!(
            "{}__doctest_{}.{}.jet",
            stem(shown),
            n,
            std::process::id()
        ));
        if fs::write(&tmp, &program).is_err() {
            eprintln!("error: couldn't stage doctest from `{}`", shown);
            all_ok = false;
            write_doctest_proof_record(&label, shown, block.fence_line, false, "producer_start_failed");
            continue;
        }
        let tmp_shown = tmp.to_string_lossy().into_owned();
        let compiled = jet::compile_with_path(&program, &tmp_shown);
        let (rust_code, ffi_link) = match compiled {
            Ok(out) => (out.rust, out.ffi),
            Err(diags) => {
                // The doctest source is wrong; surface its diagnostics against the
                // synthetic program so the author sees the exact problem.
                println!("{}: FAIL (does not compile)", label);
                report_problems(mode, &tmp_shown, &program, &diags);
                all_ok = false;
                write_doctest_proof_record(&label, shown, block.fence_line, false, "does not compile");
                let _ = fs::remove_file(&tmp);
                continue;
            }
        };
        let bin = tmp.with_extension("");
        let generated_rs = PathBuf::from("build").join(format!("{}.rs", stem(&tmp_shown)));
        build(
            &tmp_shown,
            &rust_code,
            bin.clone(),
            BuildProfile::Default,
            ffi_link.as_ref(),
            &[],
            false,
            None,
            None,
            None,
            mode,
            // Doctest binaries are one-shot synthetic programs; not cached.
            None,
        );
        let out = match Command::new(&bin).output() {
            Ok(o) => o,
            Err(e) => {
                eprintln!("error: couldn't run {}: {}", label, e);
                all_ok = false;
                write_doctest_proof_record(&label, shown, block.fence_line, false, "producer_start_failed");
                let _ = fs::remove_file(&tmp);
                let _ = fs::remove_file(&bin);
                let _ = fs::remove_file(&generated_rs);
                continue;
            }
        };
        let _ = fs::remove_file(&tmp);
        let _ = fs::remove_file(&bin);
        let _ = fs::remove_file(&generated_rs);
        if !out.status.success() {
            println!("{}: FAIL (runtime error)", label);
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
            all_ok = false;
            write_doctest_proof_record(&label, shown, block.fence_line, false, "runtime error");
            continue;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let produced: Vec<&str> = stdout.lines().collect();
        let mut block_ok = true;
        for (i, e) in block.expects.iter().enumerate() {
            let actual = produced.get(i).copied().unwrap_or("");
            if actual == e.expected {
                continue;
            }
            if update_snapshots {
                // D-TOOL4 / E2901 fix: rewrite the claimed `// =>` value in place.
                if rewrite_doctest_expect(&mut rewritten, e.line, actual) {
                    did_rewrite = true;
                    continue;
                }
            }
            block_ok = false;
            all_ok = false;
            let span = doc_line_span(src, e.line);
            let diag = jet::Doctest::mismatch_diag(shown, e, actual, span);
            // Render against the original file so the line points at the doc
            // comment's producing line.
            eprint!("{}", jet::render_diagnostics(shown, src, &[diag]));
        }
        println!("{}: {}", label, if block_ok { "pass" } else { "FAIL" });
        write_doctest_proof_record(&label, shown, block.fence_line, block_ok, if block_ok { "" } else { "output mismatch" });
    }
    if did_rewrite {
        if let Err(e) = fs::write(path, &rewritten) {
            eprintln!("error: couldn't update doctest snapshots in `{}`: {}", shown, e);
            return false;
        }
    }
    all_ok
}

fn write_doctest_proof_record(name: &str, file: &str, line: usize, passed: bool, message: &str) {
    let Ok(path) = std::env::var("JET_TEST_PROOF_REPORT") else { return };
    let Ok(mut report) = fs::OpenOptions::new().create(true).append(true).open(path) else { return };
    use std::io::Write as _;
    if report.metadata().map(|m| m.len() == 0).unwrap_or(false) {
        let _ = report.write_all(b"JETTEST2");
    }
    let _ = report.write_all(&[4, if passed { 0 } else { 1 }]);
    let _ = report.write_all(&(line as u64).to_be_bytes());
    for bytes in [name.as_bytes(), message.as_bytes(), file.as_bytes()] {
        let _ = report.write_all(&(bytes.len() as u64).to_be_bytes());
        let _ = report.write_all(bytes);
    }
    let _ = report.flush();
}

/// Replace the `// => …` claim on 1-based `line` with `actual`. Returns false when
/// the line has no expect marker (caller falls through to E2901).
fn rewrite_doctest_expect(src: &mut String, line: usize, actual: &str) -> bool {
    let mut out = String::with_capacity(src.len() + actual.len());
    let mut found = false;
    for (i, l) in src.split_inclusive('\n').enumerate() {
        if i + 1 != line {
            out.push_str(l);
            continue;
        }
        let (nl, body) = if let Some(b) = l.strip_suffix('\n') {
            ("\n", b)
        } else {
            ("", l)
        };
        let body = body.strip_suffix('\r').unwrap_or(body);
        let trimmed = body.trim_start();
        if !trimmed.starts_with("///") {
            out.push_str(l);
            continue;
        }
        let indent_len = body.len() - trimmed.len();
        let indent = &body[..indent_len];
        let after_slashes = &trimmed[3..];
        let doc_space = if after_slashes.starts_with(' ') { " " } else { "" };
        let inner = after_slashes.strip_prefix(' ').unwrap_or(after_slashes);
        let Some(idx) = find_doctest_expect_marker(inner) else {
            out.push_str(l);
            continue;
        };
        let expr = inner[..idx].trim_end();
        out.push_str(indent);
        out.push_str("///");
        out.push_str(doc_space);
        out.push_str(expr);
        if !expr.is_empty() {
            out.push(' ');
        }
        out.push_str("// => ");
        out.push_str(actual);
        out.push_str(nl);
        found = true;
    }
    if found {
        *src = out;
    }
    found
}

/// Same marker scan as `Doctest::find_expect_marker` — keep local so CmdCompile
/// does not depend on a private helper.
fn find_doctest_expect_marker(s: &str) -> Option<usize> {
    const MARKER: &str = "// =>";
    let bytes = s.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i + MARKER.len() <= bytes.len() {
        let c = bytes[i];
        if c == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_str = !in_str;
        }
        if !in_str && &s[i..i + MARKER.len()] == MARKER {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// D-TEST4: the byte span of the (1-based) `line` in `src`, for an E2901 report.
fn doc_line_span(src: &str, line: usize) -> Option<jet::Diagnostics::Span> {
    let mut start = 0usize;
    for (i, l) in src.split_inclusive('\n').enumerate() {
        if i + 1 == line {
            let trimmed_len = l.trim_end_matches(['\n', '\r']).len();
            return Some(jet::Diagnostics::Span::new(start, start + trimmed_len));
        }
        start += l.len();
    }
    None
}

/// D-COV1: read the executed-line set and print a per-function coverage table
/// plus an overall line-coverage figure. Output format is an implementation
/// choice (the spec scopes coverage as tooling); this prints a stdout summary.
fn report_coverage(file: &str, cov_out: &Path) {
    let hits: std::collections::BTreeSet<usize> = fs::read_to_string(cov_out)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse::<usize>().ok())
        .collect();
    let _ = fs::remove_file(cov_out);
    let funcs = jet::coverable_functions(file);
    if funcs.is_empty() {
        println!("\ncoverage: no functions to measure");
        return;
    }
    let mut covered = 0usize;
    println!("\ncoverage for {}", file);
    let mut by_line = funcs.clone();
    by_line.sort_by_key(|(_, l)| *l);
    for (name, line) in &by_line {
        let hit = hits.contains(line);
        if hit {
            covered += 1;
        }
        println!(
            "  {:4}  {}  {}:{}",
            if hit { "HIT " } else { "MISS" },
            name,
            file,
            line
        );
    }
    let total = funcs.len();
    let pct = (covered as f64 / total as f64) * 100.0;
    println!("  {}/{} functions covered ({:.0}%)", covered, total, pct);
}

// ─── D-FMTPROJECT1=D: project-level formatter ───────────────────────────────

/// Directories skipped during recursive discovery. Explicit file paths and
/// stdin are NEVER subject to these ignore rules.
const IGNORED_DIRS: &[&str] = &[
    "vendor", "target", "build", ".git", "node_modules", ".jet",
];

/// Recursively collect `.jet` files under `dir`, skipping IGNORED_DIRS.
/// Entries are sorted deterministically.
fn walk_jet_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !IGNORED_DIRS.contains(&name) {
                walk_jet_files(&path, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some(jet::Syntax::FILE_EXT) {
            out.push(path);
        }
    }
}

/// Discover the project/workspace root via `pkg.jet` and collect all `.jet`
/// files under it. Falls back to cwd when no manifest is found above.
fn discover_project_files() -> Vec<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::Loader::find_manifest_root(&cwd).unwrap_or(cwd);
    let mut files = Vec::new();
    walk_jet_files(&root, &mut files);
    files
}

/// Collect `.jet` files from an explicit list of paths. Directories are walked
/// recursively (IGNORED_DIRS still apply to directory traversal); individual
/// file paths are included as-is (no ignore — explicit path = intentional).
/// Non-existent paths are pushed so the read phase produces a proper I/O
/// error that gets collected by the preflight loop.
fn collect_explicit_files(paths: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for raw in paths {
        let p = PathBuf::from(raw);
        if p.is_dir() {
            walk_jet_files(&p, &mut files);
        } else {
            files.push(p);
        }
    }
    files.sort();
    files.dedup();
    files
}

/// Collect VCS-changed `.jet` files using git. Exits with USAGE (exit 2) when
/// not inside a git repository, with a diagnostic naming the fix.
fn collect_changed_files() -> Vec<PathBuf> {
    let is_git = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !is_git {
        eprintln!("error: `--changed` requires a git repository");
        eprintln!(" why: `jet fmt --changed` uses git to find modified .jet files");
        eprintln!(" fix: run from inside a git repository, or format specific files with `jet fmt <path>`");
        exit(ExitCodes::USAGE);
    }

    let ext = jet::Syntax::FILE_EXT;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut seen: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();

    // Modified tracked files vs HEAD (covers staged + unstaged changes to tracked files).
    // Also grab staged-only diffs (new files added to index but not yet committed).
    for git_args in [
        &["diff", "--name-only", "HEAD"][..],
        &["diff", "--name-only", "--cached"][..],
    ] {
        if let Ok(out) = Command::new("git").args(git_args).output() {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    if line.ends_with(&format!(".{}", ext)) {
                        let p = cwd.join(line);
                        if p.is_file() {
                            seen.insert(p);
                        }
                    }
                }
            }
        }
    }
    seen.into_iter().collect()
}

/// JSON-escape a string (backslash, double-quote, newlines).
/// `json_escape` at line 87 covers the same set — this reuses that function.

/// Emit a JSON "ok" result for `--json --check` or successful format with no changes.
fn fmt_json_ok() -> String {
    "{\"schema_version\":1,\"command\":\"fmt\",\"status\":\"ok\"}".to_string()
}

/// Emit a JSON "dirty" result (--check found changes, no --diff).
fn fmt_json_dirty_paths(paths: &[&str]) -> String {
    let list: String = paths
        .iter()
        .map(|p| format!("\"{}\"", json_escape(p)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema_version\":1,\"command\":\"fmt\",\"status\":\"dirty\",\"files\":[{}]}}",
        list
    )
}

/// Emit a JSON "dirty" result with unified diffs (--check --diff).
fn fmt_json_dirty_diffs(entries: &[(&str, &str)]) -> String {
    let list: String = entries
        .iter()
        .map(|(p, d)| {
            format!(
                "{{\"path\":\"{}\",\"diff\":\"{}\"}}",
                json_escape(p),
                json_escape(d)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema_version\":1,\"command\":\"fmt\",\"status\":\"dirty\",\"files\":[{}]}}",
        list
    )
}

/// `jet fmt - [--stdin-path=<label>]`: read from stdin, format, write to stdout.
/// Ignore rules do NOT apply. Exit 2 on parse error.
fn run_fmt_stdin(stdin_path: Option<&str>, mode: OutputMode) {
    use std::io::Read;
    let mut src = String::new();
    if std::io::stdin().read_to_string(&mut src).is_err() {
        eprintln!("error: failed to read from stdin");
        exit(ExitCodes::USAGE);
    }
    let label = stdin_path.unwrap_or("<stdin>");
    match jet::format_source(&src) {
        Ok(formatted) => {
            print!("{}", formatted);
        }
        Err(diags) => {
            if mode.json {
                eprint!("{}", jet::render_all_json(label, &src, &diags));
            } else {
                eprint!("{}", jet::render_diagnostics(label, &src, &diags));
            }
            exit(ExitCodes::USAGE);
        }
    }
}

/// D-FMTPROJECT1=D: the full project formatter.
///
/// `explicit_paths` — zero or more file/directory paths given on the CLI.
/// `stdin_mode`     — true when `-` was among the path arguments.
/// `stdin_path`     — optional `--stdin-path=<label>` for diagnostics.
/// `check_only`     — `--check`: exit 1 if any file would change, list paths.
/// `show_diff`      — `--diff`: with `--check`, also print unified diffs.
/// `changed_only`   — `--changed`: limit to VCS-changed `.jet` files (git).
/// `mode`           — output mode (`--json`, color).
pub(crate) fn run_fmt(
    explicit_paths: &[String],
    stdin_mode: bool,
    stdin_path: Option<&str>,
    check_only: bool,
    show_diff: bool,
    changed_only: bool,
    mode: OutputMode,
) {
    if stdin_mode {
        run_fmt_stdin(stdin_path, mode);
        return;
    }

    // Collect the set of files to operate on.
    let files: Vec<PathBuf> = if changed_only {
        collect_changed_files()
    } else if explicit_paths.is_empty() {
        discover_project_files()
    } else {
        collect_explicit_files(explicit_paths)
    };

    if files.is_empty() {
        if mode.json {
            println!("{}", fmt_json_ok());
        }
        return;
    }

    // Preflight: format ALL files before writing ANY. Collect results so that
    // a single parse or I/O failure aborts the whole batch with no writes.
    struct FileResult {
        path: PathBuf,
        original: String,
        formatted: String,
        changed: bool,
        io_error: Option<String>,
        parse_diags: Vec<jet::Diagnostics::Diagnostic>,
    }

    let mut results: Vec<FileResult> = Vec::with_capacity(files.len());
    for path in &files {
        let src = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                results.push(FileResult {
                    path: path.clone(),
                    original: String::new(),
                    formatted: String::new(),
                    changed: false,
                    io_error: Some(format!("can't read `{}`: {}", path.display(), e)),
                    parse_diags: Vec::new(),
                });
                continue;
            }
        };
        match jet::format_source(&src) {
            Ok(formatted) => {
                let changed = formatted != src;
                results.push(FileResult {
                    path: path.clone(),
                    original: src,
                    formatted,
                    changed,
                    io_error: None,
                    parse_diags: Vec::new(),
                });
            }
            Err(diags) => {
                results.push(FileResult {
                    path: path.clone(),
                    original: src,
                    formatted: String::new(),
                    changed: false,
                    io_error: None,
                    parse_diags: diags,
                });
            }
        }
    }

    // If ANY file failed, report every failure and exit 2 — nothing is written.
    let has_errors = results
        .iter()
        .any(|r| r.io_error.is_some() || !r.parse_diags.is_empty());
    if has_errors {
        for r in &results {
            if let Some(ref io_err) = r.io_error {
                if mode.json {
                    let path_s = r.path.to_str().unwrap_or("?");
                    eprintln!(
                        "{{\"schema_version\":1,\"command\":\"fmt\",\"status\":\"error\",\"errors\":[{{\"path\":\"{}\",\"message\":\"{}\"}}]}}",
                        json_escape(path_s),
                        json_escape(io_err)
                    );
                } else {
                    eprintln!("error: {}", io_err);
                }
            }
            if !r.parse_diags.is_empty() {
                let path_s = r.path.to_str().unwrap_or("?");
                if mode.json {
                    eprint!(
                        "{}",
                        jet::render_all_json(path_s, &r.original, &r.parse_diags)
                    );
                } else {
                    eprint!(
                        "{}",
                        jet::render_diagnostics(path_s, &r.original, &r.parse_diags)
                    );
                }
            }
        }
        exit(ExitCodes::USAGE);
    }

    // Root for root-relative path display in --check output.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let display_root = jet::Loader::find_manifest_root(&cwd).unwrap_or_else(|| cwd.clone());
    let make_rel = |p: &Path| -> String {
        p.strip_prefix(&display_root)
            .map(|r| r.display().to_string())
            .unwrap_or_else(|_| p.display().to_string())
    };

    if check_only {
        // --check: report which files would change (sorted root-relative), exit 1 if any.
        let mut dirty: Vec<&FileResult> = results.iter().filter(|r| r.changed).collect();
        dirty.sort_by(|a, b| make_rel(&a.path).cmp(&make_rel(&b.path)));

        if dirty.is_empty() {
            if mode.json {
                println!("{}", fmt_json_ok());
            }
            return; // exit 0
        }

        if mode.json {
            if show_diff {
                let entries: Vec<(String, String)> = dirty
                    .iter()
                    .map(|r| {
                        let rel = make_rel(&r.path);
                        let diff =
                            jet::Formatter::unified_diff(&rel, &r.original, &r.formatted);
                        (rel, diff)
                    })
                    .collect();
                let refs: Vec<(&str, &str)> =
                    entries.iter().map(|(p, d)| (p.as_str(), d.as_str())).collect();
                println!("{}", fmt_json_dirty_diffs(&refs));
            } else {
                let paths: Vec<&str> =
                    dirty.iter().map(|r| r.path.to_str().unwrap_or("?")).collect();
                println!("{}", fmt_json_dirty_paths(&paths));
            }
        } else {
            for r in &dirty {
                println!("{}", make_rel(&r.path));
                if show_diff {
                    let rel = make_rel(&r.path);
                    print!(
                        "{}",
                        jet::Formatter::unified_diff(&rel, &r.original, &r.formatted)
                    );
                }
            }
        }
        exit(ExitCodes::USER_ERROR); // exit 1 — --check found changes
    }

    // Format mode: write all changed files (preflight passed, so all are valid).
    for r in results.iter().filter(|r| r.changed) {
        if let Err(e) = fs::write(&r.path, &r.formatted) {
            eprintln!("error: couldn't write `{}`: {}", r.path.display(), e);
            exit(ExitCodes::USAGE);
        }
    }
    // exit 0 implicitly
}

pub(crate) fn stem(file: &str) -> String {
    Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".to_string())
        .replace('.', "_")
}

fn rustc_crate_name(file: &str) -> String {
    let mut name: String = stem(file)
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if name.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        name.insert(0, '_');
    }
    name
}

fn bin_path(file: &str) -> PathBuf {
    PathBuf::from("build").join(stem(file))
}

fn test_bin_path(path: &Path) -> PathBuf {
    PathBuf::from("build").join(format!(
        ".test_{}.{}",
        stem(&path.to_string_lossy()),
        std::process::id()
    ))
}

fn fuzz_bin_path(path: &Path, test_name: Option<&str>) -> PathBuf {
    let suffix = test_name.map(|n| format!("_{}", stem(n))).unwrap_or_default();
    PathBuf::from("build").join(format!(
        ".fuzz_{}{}.{}",
        stem(&path.to_string_lossy()),
        suffix,
        std::process::id()
    ))
}

/// `jet fuzz` flags beyond the file/test-name target (D-TESTKIT1=A gap #1).
#[derive(Clone, Default)]
pub(crate) struct FuzzRunOpts {
    /// Case budget (`--iterations=<n>`); the harness default (1000) applies
    /// when `None`.
    pub(crate) iterations: Option<u64>,
    /// Wall-clock budget in milliseconds (`--time=<n>` seconds, converted).
    pub(crate) time_budget_ms: Option<u64>,
    /// Base PRNG seed (`--seed=<n>`); the harness's fixed default applies
    /// when `None`, so a bare `jet fuzz` run is still reproducible.
    pub(crate) seed: Option<u64>,
    /// Corpus directory (`--corpus=<dir>`); defaults to
    /// `.jet/fuzz/<file-stem>[/<test-name>]`.
    pub(crate) corpus: Option<String>,
}

/// D-TESTKIT1=A (c308 pass 2, gap #1): `jet fuzz <file> [<name>]` — fuzz a
/// parameterized `@Test fn` (D-TEST1's property-test form) with generated
/// inputs: corpus dir persistence (failing seeds saved, replayed first next
/// run), minimization (the same greedy shrink `jet test` uses), a deterministic
/// seeded PRNG (`JetRng`, std-only, I6 — the same splitmix64 generator D-TEST1
/// already ships), and iteration/time budget flags. Exit 0 = clean, exit 1 =
/// found a failure (the repro is printed as a `jet test`-shaped invocation).
pub(crate) fn run_fuzz(file: &str, test_name: Option<&str>, opts: FuzzRunOpts, mode: OutputMode) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let (rust_code, ffi_link) = match jet::compile_fuzz_with_path(file, test_name) {
        Ok(r) => r,
        Err(jet::FuzzCompileError::Diagnostics(diags)) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
        Err(jet::FuzzCompileError::Target(msg)) => {
            eprintln!("error: {}", msg);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let path = Path::new(file);
    let bin = fuzz_bin_path(path, test_name);
    build(
        file,
        &rust_code,
        bin.clone(),
        BuildProfile::Default,
        ffi_link.as_ref(),
        &[],
        false,
        None,
        None,
        None,
        mode,
        // Fuzz harness build; not content-cached — target selection (an
        // implicit "the file's only property test") can change without the
        // file's bytes changing (e.g. a sibling test gains params), and a
        // fuzz run's whole point is a fresh, honest compile of this driver.
        None,
    );
    let corpus = opts.corpus.clone().unwrap_or_else(|| {
        let mut p = format!(".jet/fuzz/{}", stem(file));
        if let Some(n) = test_name {
            p.push('/');
            p.push_str(&stem(n));
        }
        p
    });
    let mut cmd = Command::new(&bin);
    if let Some(n) = opts.iterations {
        cmd.env("JET_FUZZ_ITERATIONS", n.to_string());
    }
    if let Some(ms) = opts.time_budget_ms {
        cmd.env("JET_FUZZ_TIME_MS", ms.to_string());
    }
    if let Some(seed) = opts.seed {
        cmd.env("JET_FUZZ_SEED", seed.to_string());
    }
    cmd.env("JET_FUZZ_CORPUS", &corpus);
    let status = match cmd.status() {
        Ok(status) => status,
        Err(e) => {
            let _ = fs::remove_file(&bin);
            eprintln!("error: couldn't run the fuzz harness for `{}`: {}", file, e);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let _ = fs::remove_file(&bin);
    exit(status.code().unwrap_or(ExitCodes::USER_ERROR));
}

/// D-BUILDNORM1=A (Tower #85): SHA-256 of the enclosing `pkg.jet`'s bytes, or
/// empty when the file has no project manifest. Folded into the cache-key salt
/// so a manifest edit — including a tightened `effects:` budget or a changed
/// build profile — invalidates old cache entries. That is what keeps skipping
/// the pipeline on a cache hit sound: a policy the manifest enforces (effect
/// budgets, D-EFFBUDGET1) can never be masked by an unchanged program AST,
/// because changing the manifest changes the key.
fn manifest_fingerprint(file: &str) -> String {
    let search_from = Path::new(file).parent().unwrap_or(Path::new("."));
    if let Some(root) = jet::Loader::find_manifest_root(search_from) {
        let pack = root.join(jet::Syntax::PAYLOAD_FILE);
        if let Ok(raw) = fs::read_to_string(&pack) {
            return jet::SHA256::sha256_hex(raw.as_bytes());
        }
    }
    String::new()
}

fn native_cache_salt(
    toolchain: &str,
    dependency_fingerprint: &str,
    mode: &str,
    target: &str,
    instance_fingerprints: &[String],
) -> String {
    let mut instances = instance_fingerprints.to_vec();
    instances.sort();
    format!("{toolchain}\u{1}{dependency_fingerprint}\u{1}{mode}\u{1}{target}\u{1}{}", instances.join(","))
}

const NATIVE_CACHE_COMPILER_ABI: &str = "jet.native-cache-abi.v2";

fn command_identity(program: &str, args: &[&str]) -> String {
    match Command::new(program).args(args).output() {
        Ok(output) => {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&(output.status.code().unwrap_or(-1) as i64).to_be_bytes());
            bytes.extend_from_slice(&output.stdout);
            bytes.extend_from_slice(&output.stderr);
            jet::SHA256::sha256_hex(&bytes)
        }
        Err(error) => format!("unavailable:{:?}", error.kind()),
    }
}

fn rustc_identity() -> (String, String) {
    match Command::new("rustc").arg("-vV").output() {
        Ok(output) => {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&(output.status.code().unwrap_or(-1) as i64).to_be_bytes());
            bytes.extend_from_slice(&output.stdout);
            bytes.extend_from_slice(&output.stderr);
            let verbose = String::from_utf8_lossy(&output.stdout);
            let backend = verbose.lines().find(|line| line.starts_with("LLVM version:"))
                .unwrap_or("LLVM version: unavailable").to_string();
            (jet::SHA256::sha256_hex(&bytes), backend)
        }
        Err(error) => {
            let unavailable = format!("unavailable:{:?}", error.kind());
            (unavailable.clone(), unavailable)
        }
    }
}

/// Identity of every executable/backend that can change emitted native code.
/// Compiler build bytes make local/dev compilers distinct even when package
/// SemVer is unchanged; rustc `-vV` includes its LLVM backend revision.
fn native_toolchain_identity() -> &'static str {
    static IDENTITY: OnceLock<String> = OnceLock::new();
    IDENTITY.get_or_init(|| {
        let compiler_build = std::env::current_exe().ok()
            .and_then(|path| fs::read(path).ok())
            .map(|bytes| jet::SHA256::sha256_hex(&bytes))
            .unwrap_or_else(|| "unavailable".into());
        let (rustc, backend) = rustc_identity();
        let linker_name = Command::new("rustc").args(["--print", "linker"])
            .output().ok().filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
            .unwrap_or_else(|| "cc".into());
        let linker = command_identity(&linker_name, &["--version"]);
        format!(
            "abi={NATIVE_CACHE_COMPILER_ABI}\u{1}build={compiler_build}\u{1}version={}\u{1}semindex={}\u{1}rustc={rustc}\u{1}backend={backend}\u{1}linker-name={linker_name}\u{1}linker={linker}",
            jet::Manifest::COMPILER_VERSION,
            jet_semindex::SCHEMA_VERSION,
        )
    })
}

fn dependency_interface_fingerprint(bundle: &jet::AST::ProgramBundle) -> String {
    let mut interfaces = Vec::new();
    for (dependency, root) in &bundle.dep_roots {
        for module in &bundle.modules {
            if !module.path.starts_with(root) { continue; }
            for item in &module.items {
                let public = match item {
                    jet::AST::Item::Func(def) => def.is_pub || def.is_package_pub,
                    jet::AST::Item::Struct(def) => def.is_pub || def.is_package_pub,
                    jet::AST::Item::Enum(def) => def.is_pub || def.is_package_pub,
                    jet::AST::Item::Trait(def) => def.is_pub || def.is_package_pub,
                    jet::AST::Item::Tag(def) => def.is_pub || def.is_package_pub,
                    jet::AST::Item::CodeModule(def) => def.is_pub || def.is_package_pub,
                    _ => false,
                };
                if public {
                    interfaces.push((dependency.clone(), module.display.clone(), jet::CanonicalAST::canonical_fragment(item)));
                }
            }
        }
    }
    interfaces.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
    let mut bytes = Vec::new();
    for (dependency, module, interface) in interfaces {
        for value in [dependency.as_bytes(), module.as_bytes(), interface.as_slice()] {
            bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
            bytes.extend_from_slice(value);
        }
    }
    jet::SHA256::sha256_hex(&bytes)
}

/// D-BUILDNORM1=A (Tower #85): the content-cache key for building `file` under
/// `mode_tag` (`"run"`, `"test"`, `"testcov"`, `"bench"`, `"dev"`, …), computed
/// from the *pre-sema* canonical AST of the whole program (entry + every module
/// the loader resolved). Returns `None` when:
///
/// - the program can't be loaded/parsed — the caller falls through to the
///   normal compile, which reports the real diagnostic; or
/// - the program uses `embed_file`/`embed_bytes` — its output depends on
///   external file bytes not captured by the AST, so it must never be served
///   from (or stored into) a content cache keyed on the AST alone. Detected by a
///   conservative source-substring scan: a false positive only forgoes caching
///   for that build (a safe perf cost), never serves a stale binary.
///
/// `mode_tag` keeps binaries built from the same AST under different pipelines in
/// separate key spaces (a `jet test` harness binary can never be served for a
/// `jet run`). The toolchain version and `pkg.jet` fingerprint ride the salt.
fn native_cache_key(file: &str, profile_tag: &str, mode_tag: &str) -> Option<String> {
    native_cache_key_with_toolchain(file, profile_tag, mode_tag, native_toolchain_identity())
}

fn native_cache_key_with_toolchain(
    file: &str,
    profile_tag: &str,
    mode_tag: &str,
    toolchain_identity: &str,
) -> Option<String> {
    // `jet prove` consumes a compiler-private structured harness protocol. A
    // dirty/development compiler must never receive an older cached harness
    // that predates or mismatches that protocol.
    if std::env::var_os("JET_PROVE_FRESH_TEST").is_some() {
        return None;
    }
    let mut bundle = jet::Loader::load_entry_with_overlay(file, None, false).ok()?;
    let uses_embed = bundle.modules.iter().any(|m| {
        m.source.contains(jet::Syntax::BUILTIN_EMBED_FILE)
            || m.source.contains(jet::Syntax::BUILTIN_EMBED_BYTES)
    });
    if uses_embed {
        return None;
    }
    // #91: instance identity is a sema product. Run the front end before a
    // cache lookup so a hit is keyed by resolved template identity rather than
    // consumer spelling. A hit still skips codegen/rustc, never validation.
    if jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check)
        .iter().any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
    {
        return None;
    }
    let instances: Vec<String> = bundle.modules.iter().flat_map(|module| module.items.iter().filter_map(|item| {
        let jet::AST::Item::CodeModule(cm) = item else { return None };
        cm.instance_identity.as_ref().map(|identity| identity.fingerprint.clone())
    })).collect();
    let dependency_interfaces = dependency_interface_fingerprint(&bundle);
    let salt = native_cache_salt(
        toolchain_identity,
        &format!("{}:{dependency_interfaces}", manifest_fingerprint(file)),
        mode_tag,
        &format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        &instances,
    );
    Some(jet::CanonicalAST::ast_cache_key(
        &bundle,
        profile_tag,
        &salt,
    ))
}

/// E2-M15 / E3302: check that rustc knows the requested cross-compilation target.
/// Runs `rustc --print target-list` and exits with E3302 if the triple is absent.
/// D-WEBKIND1=A (c123): `web` is a Jet backend target, not a rustc triple — accepted here.
fn validate_target(triple: &str, mode: OutputMode) {
    // D-WEBKIND1=A: Jet backend target, not a rustc triple.
    if triple == "web" || triple == jet::Syntax::BUILD_TARGET_WEB {
        return;
    }
    // D-PLUGIN1=B (c81): another Jet backend target, not a rustc triple — the
    // guest build resolves its own `wasm32-unknown-unknown` triple internally.
    if triple == jet::Syntax::TARGET_PLUGIN {
        return;
    }
    // rustc --print target-list gives the full list; if the output contains
    // the triple exactly (one per line), the target is known.
    let out = Command::new("rustc")
        .arg("--print")
        .arg("target-list")
        .output();
    let known = match out {
        Ok(o) if o.status.success() => {
            let list = String::from_utf8_lossy(&o.stdout);
            list.lines().any(|l| l.trim() == triple)
        }
        _ => false, // rustc not found or failed; will fail later during compile
    };
    if !known {
        let diag = jet::Sema::e3302(triple);
        let src = format!("// cross-build for {}", triple);
        report_problems(mode, "<target>", &src, &[diag]);
        exit(ExitCodes::USER_ERROR);
    }
    // Check that the std library is installed for this target.
    // `rustc --print sysroot` + check for lib/<triple>/ directory.
    let sysroot = Command::new("rustc").arg("--print").arg("sysroot").output();
    if let Ok(o) = sysroot {
        let root = String::from_utf8_lossy(&o.stdout).trim().to_string();
        let target_lib = PathBuf::from(&root)
            .join("lib")
            .join("rustlib")
            .join(triple);
        if !target_lib.exists() {
            let diag = jet::Sema::e3302(triple);
            let src = format!("// cross-build for {}", triple);
            report_problems(mode, "<target>", &src, &[diag]);
            eprintln!(
                " why: `rustup target add {}` to install the standard library for this target",
                triple
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// `jet dev <file>.jet --target=web`: the root retains only the R5 build
/// executor and process watch loop. HTTP, Canvas routes, terminal/browser
/// status, client leases, and last-good swapping live in `jet-devserver`.
pub(crate) fn run_dev_web(
    file: &str,
    mode: OutputMode,
    verbose: bool,
    port: Option<u16>,
) {
    let path = Path::new(file);
    if !path.exists() {
        eprintln!("error: can't find the file `{}`", file);
        eprintln!(
            " fix: check the spelling, or run {} from the folder that contains it",
            jet::Syntax::BINARY_NAME
        );
        exit(ExitCodes::USER_ERROR);
    }

    let host = match jet_devserver::WebHost::WebHost::bind(file, verbose, port) {
        Ok(host) => host,
        Err(message) => {
            eprintln!("{message}");
            exit(ExitCodes::USER_ERROR);
        }
    };
    if !rebuild_dev_web(file, mode, verbose, false, &host) {
        exit(ExitCodes::USER_ERROR);
    }
    host.start();

    let mut last_mtime = jet_devserver::file_mtime(path);
    loop {
        thread::sleep(Duration::from_millis(120));
        if let Some(code) = host.exit_code() {
            exit(code);
        }
        let now = jet_devserver::file_mtime(path);
        if now != last_mtime {
            last_mtime = now;
            thread::sleep(Duration::from_millis(30));
            rebuild_dev_web(file, mode, verbose, true, &host);
        }
    }
}

fn rebuild_dev_web(
    file: &str,
    mode: OutputMode,
    verbose: bool,
    is_rebuild: bool,
    host: &jet_devserver::WebHost::WebHost,
) -> bool {
    let started = Instant::now();
    host.mark_building();
    let src = fs::read_to_string(file).unwrap_or_default();
    let out = match jet::compile_web(file) {
        Ok(out) => out,
        Err(diags) => {
            if !is_rebuild {
                report_problems(mode, file, &src, &diags);
            }
            let code = diags
                .first()
                .map(|diagnostic| diagnostic.code.to_string())
                .unwrap_or_default();
            host.mark_error(
                code,
                jet::render_diagnostics(file, &src, &diags),
                is_rebuild,
            );
            return false;
        }
    };
    let Some(web) = &out.web else {
        let message = "internal compiler error: missing web codegen output".to_string();
        eprintln!("error: {message}");
        host.mark_error("ICE".to_string(), message, is_rebuild);
        return false;
    };

    let staging = PathBuf::from("build").join(".jet-dev-staging");
    if let Err(message) = write_web_artifacts(file, web, verbose, &staging) {
        eprintln!("{message}");
        host.mark_error("ICE".to_string(), message, is_rebuild);
        return false;
    }
    if let Err(error) = jet_devserver::WebHost::stage_and_swap(&staging, Path::new("build")) {
        let message = format!("couldn't finalize web build: {error}");
        eprintln!("error: {message}");
        host.mark_error("ICE".to_string(), message, is_rebuild);
        return false;
    }

    host.mark_ready(started.elapsed().as_millis(), is_rebuild);
    true
}

/// Where `write_web_artifacts` put each `build/*` file it wrote — returned so
/// a caller can print/report the exact locations without recomputing the
/// path-join logic a second time (I8: the join logic lives in exactly one
/// place).
pub(crate) struct WebBuildPaths {
    pub(crate) manifest: PathBuf,
    pub(crate) dom: PathBuf,
    pub(crate) js: PathBuf,
    pub(crate) wasm: PathBuf,
    pub(crate) html: PathBuf,
}

/// D-WEBKIND1=A (c123 M2), extended by c134 Phase 7 (`jet dev --target=web`):
/// the ONE place that knows how to turn a compiled `WebArtifacts` bundle into
/// files on disk under `out_dir` (I8 — `jet build --target=web` and `jet dev
/// --target=web`'s rebuild-on-save loop both call this, never duplicating the
/// write/rustc-invoke logic).
///
/// `file` is the `.jet` source path — used only to look for a companion
/// `<stem>.html` next to it, which wins over the generic `index_html` codegen
/// emits (an example wiring a button to an exported `#Js` function ships its
/// own page; see `Codegen::Web::emit_web`'s `index_html` doc comment).
///
/// Returns the paths written on success. On failure to run/pass rustc for the
/// wasm half, returns `Err` with an already-formatted message instead of
/// exiting — I2: rustc rejecting generated code is always an internal
/// compiler error, but only the caller knows whether that should abort the
/// process (`jet build`) or just be reported while the previous good build
/// keeps serving (`jet dev --target=web`).
pub(crate) fn write_web_artifacts(
    file: &str,
    web: &jet::Codegen::WebArtifacts,
    verbose: bool,
    out_dir: &Path,
) -> Result<WebBuildPaths, String> {
    let step = |msg: String| {
        if verbose {
            eprintln!("[build] {}", msg);
        }
    };

    fs::create_dir_all(out_dir).map_err(|e| {
        format!(
            "error: couldn't create the {} folder: {}",
            out_dir.display(),
            e
        )
    })?;

    let manifest_path = out_dir.join("web.manifest.json");
    let dom_path = out_dir.join("jet_dom_runtime.js");
    let js_path = out_dir.join("app.js");
    let wasm_rs_path = out_dir.join("app_wasm.rs");
    let wasm_path = out_dir.join("app.wasm");
    let html_path = out_dir.join("index.html");
    // D-HTMLPAIR1 (ratified 2026-07-01, c134): precedence for the served HTML source —
    // (1) an explicit `@Html("path.html")` marker, relative to the source
    //     file's own directory; a path that doesn't resolve is a hard error
    //     naming the missing file, never a silent fallback;
    // (2) the legacy `<stem>.html` sibling-filename convention, kept for
    //     backward-compat with existing examples that predate the marker;
    // (3) the generic `jet_main()`-only page from `emit_index_html`.
    let html_contents = if let Some(rel) = &web.explicit_html_path {
        let source_dir = Path::new(file).parent().unwrap_or(Path::new("."));
        let explicit_path = source_dir.join(rel);
        fs::read_to_string(&explicit_path).map_err(|e| {
            format!(
                "error: `@Html(\"{}\")` names a file that doesn't exist: {} ({})",
                rel,
                explicit_path.display(),
                e
            )
        })?
    } else {
        let sibling_html = PathBuf::from(file).with_extension("html");
        fs::read_to_string(&sibling_html).unwrap_or_else(|_| web.index_html.clone())
    };

    fs::write(&manifest_path, &web.manifest_json)
        .map_err(|e| format!("error: couldn't write {}: {}", manifest_path.display(), e))?;
    fs::write(&dom_path, &web.dom_runtime)
        .map_err(|e| format!("error: couldn't write {}: {}", dom_path.display(), e))?;
    fs::write(&js_path, &web.js_app)
        .map_err(|e| format!("error: couldn't write {}: {}", js_path.display(), e))?;
    fs::write(&wasm_rs_path, &web.wasm_rust)
        .map_err(|e| format!("error: couldn't write {}: {}", wasm_rs_path.display(), e))?;
    fs::write(&html_path, &html_contents)
        .map_err(|e| format!("error: couldn't write {}: {}", html_path.display(), e))?;

    step(format!("web manifest -> {}", manifest_path.display()));
    step(format!("dom shim    -> {}", dom_path.display()));
    step(format!("js entry    -> {}", js_path.display()));
    step(format!("wasm emit   -> {}", wasm_rs_path.display()));
    step(format!("index.html  -> {}", html_path.display()));
    step(format!(
        "rustc wasm  {} -> {}",
        wasm_rs_path.display(),
        wasm_path.display()
    ));

    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
            "-O",
            wasm_rs_path.to_str().unwrap(),
            "-o",
            wasm_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| format!("error: couldn't run rustc for wasm: {}", e))?;

    if !rustc.status.success() {
        return Err(format!(
            "error: rustc rejected generated wasm module (internal compiler error)\n{}",
            String::from_utf8_lossy(&rustc.stderr)
        ));
    }

    let mut wasm = fs::read(&wasm_path)
        .map_err(|e| format!("error: couldn't read {}: {}", wasm_path.display(), e))?;
    jet_foundation::CliSchema::embed_wasm_record(&mut wasm, &web.command_record)
        .map_err(|e| format!("error: couldn't embed JetCommandSchema metadata: {e}"))?;
    fs::write(&wasm_path, wasm)
        .map_err(|e| format!("error: couldn't write {}: {}", wasm_path.display(), e))?;

    Ok(WebBuildPaths {
        manifest: manifest_path,
        dom: dom_path,
        js: js_path,
        wasm: wasm_path,
        html: html_path,
    })
}

/// Where `write_plugin_artifacts` put the final `.wasm` Component (and the
/// intermediate `.wit`/guest-Rust files, kept for `-v` reporting).
pub(crate) struct PluginBuildPaths {
    pub(crate) wit: PathBuf,
    pub(crate) guest_rust: PathBuf,
    pub(crate) core_wasm: PathBuf,
    pub(crate) component_wasm: PathBuf,
}

/// D-PLUGIN1=B / D-DEP-WASM1=A (c81): whether a plugin build step failed
/// because a required external tool is missing/failed to run (a clean E1259,
/// never an internal-compiler-error exit — I2 reserves ICE for our own
/// generated code being rejected) or because `rustc` rejected the
/// jet-generated guest Rust (a genuine internal compiler error — sema should
/// have caught this before codegen ever ran).
pub(crate) enum PluginBuildError {
    ToolFailure(String),
    GeneratedCodeRejected(String),
}

/// The ONE place that turns a compiled `PluginArtifacts` bundle into an actual
/// `.wasm` Component Model file on disk (I8, mirrors `write_web_artifacts`):
/// `rustc --target wasm32-unknown-unknown --crate-type cdylib` builds the core
/// module from the guest Rust, then `wasm-tools component embed` + `component
/// new` lift it into a typed component using the generated `.wit` world — no
/// wit-bindgen crate, no adapter shims; v1's Int/Float-only scalar exports are
/// already canonical-ABI-compatible at the core-wasm level (see
/// `Codegen::Plugin` module doc).
pub(crate) fn write_plugin_artifacts(
    file: &str,
    plugin: &jet::Codegen::PluginArtifacts,
    verbose: bool,
    out_dir: &Path,
) -> Result<PluginBuildPaths, PluginBuildError> {
    let step = |msg: String| {
        if verbose {
            eprintln!("[build] {}", msg);
        }
    };
    fs::create_dir_all(out_dir).map_err(|e| {
        PluginBuildError::ToolFailure(format!(
            "couldn't create the {} folder: {}",
            out_dir.display(),
            e
        ))
    })?;
    let name = stem(file);
    let wit_dir = out_dir.join(format!("{name}_wit"));
    fs::create_dir_all(&wit_dir).map_err(|e| {
        PluginBuildError::ToolFailure(format!("couldn't create {}: {}", wit_dir.display(), e))
    })?;
    let wit_path = wit_dir.join("world.wit");
    let guest_rust_path = out_dir.join(format!("{name}_plugin.rs"));
    let core_wasm_path = out_dir.join(format!("{name}_core.wasm"));
    let embedded_wasm_path = out_dir.join(format!("{name}_embedded.wasm"));
    let component_wasm_path = out_dir.join(format!("{name}.wasm"));

    fs::write(&wit_path, &plugin.wit).map_err(|e| {
        PluginBuildError::ToolFailure(format!("couldn't write {}: {}", wit_path.display(), e))
    })?;
    fs::write(&guest_rust_path, &plugin.guest_rust).map_err(|e| {
        PluginBuildError::ToolFailure(format!(
            "couldn't write {}: {}",
            guest_rust_path.display(),
            e
        ))
    })?;

    step(format!("wit emit    -> {}", wit_path.display()));
    step(format!("guest rust  -> {}", guest_rust_path.display()));
    step(format!(
        "rustc wasm  {} -> {}",
        guest_rust_path.display(),
        core_wasm_path.display()
    ));
    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
            "-O",
            guest_rust_path.to_str().unwrap(),
            "-o",
            core_wasm_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| {
            PluginBuildError::ToolFailure(format!(
                "couldn't run `rustc --target wasm32-unknown-unknown` ({e}) — is the wasm32-unknown-unknown target installed?"
            ))
        })?;
    if !rustc.status.success() {
        return Err(PluginBuildError::GeneratedCodeRejected(format!(
            "rustc rejected the generated plugin guest module\n{}",
            String::from_utf8_lossy(&rustc.stderr)
        )));
    }

    step(format!(
        "wit embed   {} -> {}",
        core_wasm_path.display(),
        embedded_wasm_path.display()
    ));
    let embed = Command::new("wasm-tools")
        .args([
            "component",
            "embed",
            wit_dir.to_str().unwrap(),
            "--world",
            &plugin.world_name,
            core_wasm_path.to_str().unwrap(),
            "-o",
            embedded_wasm_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| {
            PluginBuildError::ToolFailure(format!(
                "couldn't run `wasm-tools` ({e}) — install it (ships in the project's `nix develop` shell) or add it to PATH"
            ))
        })?;
    if !embed.status.success() {
        return Err(PluginBuildError::ToolFailure(format!(
            "`wasm-tools component embed` failed\n{}",
            String::from_utf8_lossy(&embed.stderr)
        )));
    }

    step(format!(
        "wit lift    {} -> {}",
        embedded_wasm_path.display(),
        component_wasm_path.display()
    ));
    let new = Command::new("wasm-tools")
        .args([
            "component",
            "new",
            embedded_wasm_path.to_str().unwrap(),
            "-o",
            component_wasm_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| PluginBuildError::ToolFailure(format!("couldn't run `wasm-tools` ({e})")))?;
    if !new.status.success() {
        return Err(PluginBuildError::ToolFailure(format!(
            "`wasm-tools component new` failed\n{}",
            String::from_utf8_lossy(&new.stderr)
        )));
    }

    Ok(PluginBuildPaths {
        wit: wit_path,
        guest_rust: guest_rust_path,
        core_wasm: core_wasm_path,
        component_wasm: component_wasm_path,
    })
}

pub(crate) fn build(
    file: &str,
    rust_code: &str,
    bin: PathBuf,
    profile: BuildProfile,
    ffi: Option<&jet::FFI::FfiLink>,
    clinks: &[String],
    verbose: bool,
    cross_target: Option<&str>,
    web: Option<&jet::Codegen::WebArtifacts>,
    plugin: Option<&jet::Codegen::PluginArtifacts>,
    mode: OutputMode,
    // D-BUILDNORM1=A (Tower #85): the content-addressed cache key, computed by
    // the caller from the *pre-sema* canonical AST bytes (+ profile + toolchain
    // salt). `None` when this build must not be cached (e.g. an `embed_file`
    // build, whose output depends on external file bytes not captured by the
    // AST, or a caller that couldn't parse a bundle). `build` still applies its
    // own `use_cache` gate (FFI/C-link/cross builds never cache) on top of this.
    cache_key: Option<String>,
) {
    // D-BUILD2: `jet build -v` makes the hidden Jet→Rust→native bridge honest.
    // Step labels are deterministic so they can be golden-tested.
    let step = |msg: String| {
        if verbose {
            eprintln!("[build] {}", msg);
        }
    };

    fs::create_dir_all("build").unwrap_or_else(|e| {
        eprintln!("error: couldn't create the build/ folder: {}", e);
        exit(ExitCodes::USER_ERROR);
    });
    let rs_path = PathBuf::from("build").join(format!("{}.rs", stem(file)));
    step(format!("emit Rust  -> {}", rs_path.display()));
    fs::write(&rs_path, rust_code).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", rs_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    // D-WEBKIND1=A (c123 M2): `web` is a Jet backend target — emit WASM + JS.
    if cross_target == Some(jet::Syntax::BUILD_TARGET_WEB) {
        let web = web.unwrap_or_else(|| {
            eprintln!("error: internal compiler error: missing web codegen output");
            exit(ExitCodes::ICE);
        });
        let paths = match write_web_artifacts(file, web, verbose, Path::new("build")) {
            Ok(p) => p,
            Err(msg) => {
                eprintln!("{}", msg);
                exit(ExitCodes::ICE);
            }
        };
        let _ = rust_code;
        let _ = bin;
        let _ = profile;
        let _ = ffi;
        let _ = clinks;
        if !mode.json {
            eprintln!(
                "note: `--target=web` wrote `{}`, `{}`, `{}`, `{}`, `{}`",
                paths.manifest.display(),
                paths.dom.display(),
                paths.js.display(),
                paths.wasm.display(),
                paths.html.display(),
            );
        }
        return;
    }

    // D-PLUGIN1=B (c81): `plugin` is another Jet backend target — build the
    // sandboxed wasm32 Component Model module instead of a native binary.
    if cross_target == Some(jet::Syntax::TARGET_PLUGIN) {
        let plugin = plugin.unwrap_or_else(|| {
            eprintln!("error: internal compiler error: missing plugin codegen output");
            exit(ExitCodes::ICE);
        });
        let paths = match write_plugin_artifacts(file, plugin, verbose, Path::new("build")) {
            Ok(p) => p,
            Err(PluginBuildError::GeneratedCodeRejected(msg)) => {
                eprintln!(
                    "error: rustc rejected generated code (internal compiler error)\n{}",
                    msg
                );
                exit(ExitCodes::ICE);
            }
            Err(PluginBuildError::ToolFailure(msg)) => {
                let diag = jet::Manifest::e1259(&msg);
                let src = format!("// plugin build for {}", file);
                report_problems(mode, "<plugin>", &src, &[diag]);
                exit(ExitCodes::USER_ERROR);
            }
        };
        let _ = rust_code;
        let _ = bin;
        let _ = profile;
        let _ = ffi;
        let _ = clinks;
        if !mode.json {
            eprintln!(
                "note: `--target=plugin` wrote `{}`, `{}`, `{}`, `{}`",
                paths.wit.display(),
                paths.guest_rust.display(),
                paths.core_wasm.display(),
                paths.component_wasm.display(),
            );
        }
        return;
    }

    // Cross-compiled or freestanding builds bypass the host binary cache
    // (the binary is not executable on this host, and the target triple
    // affects codegen choices that aren't captured by the source hash), as do
    // FFI- and C-linked builds (link inputs aren't in the AST key) and builds
    // the caller marked un-cacheable by passing `cache_key = None` (embed_file).
    let use_cache =
        ffi.is_none() && clinks.is_empty() && cross_target.is_none() && cache_key.is_some();
    // D-BUILDNORM1=A: the key is the caller's pre-sema canonical-AST key
    // (D-BUILDPROFILE1's profile tag is already folded into it). Kept only when
    // this build is cacheable.
    let cache_key = if use_cache { cache_key } else { None };
    if let Some(ref key) = cache_key {
        if jet::BuildCache::try_copy_cached(key, &bin) {
            step("cache hit -> reused cached binary".to_string());
            // c121: still report size on a cache hit so the dashboard always
            // has a binary-size data point.
            if jet::PhaseTiming::enabled() {
                if let Ok(meta) = std::fs::metadata(&bin) {
                    eprintln!("jet-timing binary_bytes={}", meta.len());
                }
            }
            return;
        }
    }
    if verbose {
        if cache_key.is_some() {
            step("cache miss -> compiling".to_string());
        } else if cross_target.is_some() {
            step("cache bypassed (cross-compiled build)".to_string());
        } else {
            step("cache bypassed (C-linked build)".to_string());
        }
    }

    step(format!(
        "rustc      {} -> {}",
        rs_path.display(),
        bin.display()
    ));
    let mut cmd = Command::new("rustc");
    cmd.arg("--edition").arg("2021");
    // E2-M15: cross-compilation target triple.
    if let Some(triple) = cross_target {
        cmd.arg("--target").arg(triple);
    }
    let ffi_present = ffi.is_some();
    let config = profile.config();
    config.apply_env(&mut cmd);
    if matches!(profile, BuildProfile::Release) {
        cmd.arg("--cfg").arg("jet_release");
    }
    config.apply_rustc(&mut cmd, ffi_present);
    // Cache-integrity fix (Tower #85 §0): compile to a *private per-process*
    // path, never straight onto the shared `build/<stem>` display path. Two
    // concurrent `jet` processes compiling different source that happens to
    // share a file stem would otherwise race — process A could `store_cached`
    // its hash against process B's freshly-overwritten `build/<stem>`, mapping
    // A's key to B's binary in the shared content cache. `process::id()`
    // disambiguates the processes; we `store_cached` from this private path
    // (safe — only ever racing another process computing the *same* key, i.e.
    // the same content) and only then rename into the shared display path.
    let bin_name = bin
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("out")
        .to_string();
    // Private per-process working directory: rustc writes the output binary,
    // the generated `.rs`, AND all its intermediate codegen-unit object files
    // (`*.rcgu.o`) here. Two concurrent builds that share a file stem would
    // otherwise collide in the shared `build/` dir — on the binary, on the
    // source file mid-compile, and on the crate-name-derived intermediates —
    // corrupting each other's compile and (per §0) the shared content cache.
    // The results are published to the shared display paths only after a clean
    // compile; `store_cached` reads from this private path.
    let work = PathBuf::from("build").join(format!(".work.{}.{}", bin_name, std::process::id()));
    if let Err(e) = fs::create_dir_all(&work) {
        eprintln!("error: couldn't create the build work dir: {}", e);
        exit(ExitCodes::USER_ERROR);
    }
    let tmp_bin = work.join(&bin_name);
    let tmp_rs = work.join(format!("{}.rs", stem(file)));
    if let Err(e) = fs::write(&tmp_rs, rust_code) {
        eprintln!("error: couldn't write {}: {}", tmp_rs.display(), e);
        exit(ExitCodes::USER_ERROR);
    }
    // Pin the crate name to the file stem — the name rustc used to infer from
    // `build/<stem>.rs` — so the private working-dir source name doesn't leak
    // into codegen.
    cmd.arg("--crate-name").arg(rustc_crate_name(file));
    cmd.arg(&tmp_rs).arg("-o").arg(&tmp_bin);
    if let Some(link) = ffi {
        cmd.arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        if link.deps_dir.is_dir() {
            cmd.arg("-L")
                .arg(format!("dependency={}", link.deps_dir.display()));
        }
    }
    // S59 (E2-M14): native C library link flags (`-L native=…`, `-l <name>`).
    for arg in clinks {
        cmd.arg(arg);
    }

    let out = match cmd.output() {
        Ok(o) => o,
        Err(_) => {
            eprintln!("error: couldn't find `rustc` on this machine");
            eprintln!(
                " why: v1 of this language uses Rust as its backend (docs/spec/architecture.md)"
            );
            eprintln!(" fix: install Rust from https://rustup.rs, then try again");
            exit(ExitCodes::USER_ERROR);
        }
    };

    if !out.status.success() {
        // Preserve the shared `build/<stem>.rs` artifact (written above) for the
        // ICE bug report, but drop this process's private working dir.
        let _ = fs::remove_dir_all(&work);
        let stderr = String::from_utf8_lossy(&out.stderr);
        // I2: a *missing C library* is a user/system problem, not generated-code
        // rejection. Detect a linker "cannot find -l<name>" and print a clean
        // E3209 diagnostic naming the lib + fix — NOT the bug-in-jet banner. The
        // ICE banner stays only for genuine codegen-rejected-by-rustc.
        if let Some(missing) = missing_c_lib(&stderr) {
            let diag = jet::CFFI::e3209(&missing);
            report_problems(mode, file, "", &[diag]);
            exit(ExitCodes::USER_ERROR);
        }
        if let Some(linker) = missing_linker(&stderr) {
            eprintln!("Error [L2101]: rustc could not find linker `{}`.", linker);
            eprintln!(
                " Why: Jet uses rustc as its backend, and rustc needs a C linker to produce a native binary."
            );
            eprintln!(
                " Fix: run from `nix develop`, or install a C toolchain (`gcc`/`clang`; on Debian/Ubuntu: `build-essential`, on Arch: `base-devel`)."
            );
            exit(ExitCodes::USER_ERROR);
        }
        eprintln!("internal compiler error: the generated Rust did not compile.");
        eprintln!(
            "This is a bug in {}, NOT in your program. Please report it,",
            jet::Syntax::BINARY_NAME
        );
        eprintln!("attaching your source file and the generated file below.");
        eprintln!("  generated: {}", rs_path.display());
        eprintln!("--- rustc said ---");
        eprintln!("{}", stderr);
        exit(ExitCodes::ICE);
    }

    step(format!("link       -> {}", bin.display()));

    // Store into the content cache *from the private path* first, so the cache
    // entry is written from exactly the binary this process just built — never
    // a copy that a racing process may have already overwritten on the shared
    // display path (Tower #85 §0). `store_cached` is itself write-tmp-then-rename.
    if let Some(key) = cache_key {
        jet::BuildCache::store_cached(&key, &tmp_bin);
        step("cache store -> saved binary for next time".to_string());
    }
    // Then publish the private binary onto the shared, human-readable display
    // path (`build/<stem>`) that `jet run`/`jet build` hand back. A same-dir
    // rename is atomic; last writer wins the convenience slot, which was never
    // a content identity. Fall back to copy if rename crosses a filesystem.
    if fs::rename(&tmp_bin, &bin).is_err() {
        if let Err(e) = fs::copy(&tmp_bin, &bin) {
            let _ = fs::remove_file(&tmp_bin);
            eprintln!("error: couldn't finish writing {}: {}", bin.display(), e);
            exit(ExitCodes::USER_ERROR);
        }
        let _ = fs::remove_file(&tmp_bin);
    }
    // Drop the private working dir (generated `.rs` + rustc intermediates).
    let _ = fs::remove_dir_all(&work);

    // c121: report final binary size when timing is requested. The dashboard
    // tool captures this `binary_bytes=` line from build stderr.
    if jet::PhaseTiming::enabled() {
        if let Ok(meta) = std::fs::metadata(&bin) {
            eprintln!("jet-timing binary_bytes={}", meta.len());
        }
    }
}

/// D-DBG3 step 2 (dap-debugger): build + launch the native lldb-backed `jet
/// debug` backend — a debug-profile build (full debuginfo) whose generated Rust
/// carries the `// jet:line N` table (`emit_bundle_dbg` via
/// `jet::compile_for_debug`), then either the `(jet)` terminal session or the
/// DAP server (`--dap`) drives it through `crates/jet-debug/src/Inferior.rs`. Returns
/// the process exit code.
pub(crate) fn run_debug_native(file: &str, raw_frames: bool, dap: bool, mode: OutputMode) -> i32 {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            return ExitCodes::USER_ERROR;
        }
    };
    let out = match jet::compile_for_debug(file) {
        Ok(o) => o,
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            return ExitCodes::USER_ERROR;
        }
    };
    let clinks = match jet::resolve_c_links(file) {
        Ok(a) => a,
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            return ExitCodes::USER_ERROR;
        }
    };
    let bin = PathBuf::from("build").join(format!("{}_dbg", stem(file)));
    build(
        file,
        &out.rust,
        bin.clone(),
        BuildProfile::Debug,
        out.ffi.as_ref(),
        &clinks,
        false,
        None,
        None,
        None,
        mode,
        // `jet debug` builds carry a line-map and launch interactively; not cached.
        None,
    );
    // `build()` always writes the generated Rust to `build/<stem>.rs` (the
    // debug binary path is the only caller-chosen path) — lldb's `-f` flag
    // matches by basename, so this is what `Inferior::set_breakpoint` needs.
    let rust_file = format!("{}.rs", stem(file));
    if dap {
        jet::Debug::run_dap(&bin, &rust_file, &out.rust, file, &src)
    } else {
        jet::Debug::run_native(&bin, &rust_file, &out.rust, file, &src, raw_frames)
    }
}

/// Scan a failed rustc/linker stderr for a missing C library and return its
/// link name. Matches the GNU ld / lld phrasing `cannot find -l<name>` (and the
/// `-l<name>` form some linkers print). Used to keep a missing *system library*
/// off the I2 ICE path — it's a user/system problem, not generated-code being
/// rejected.
fn missing_c_lib(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        // e.g. "cannot find -lraylib" / "cannot find -lraylib: No such file"
        if let Some(rest) = line.split("cannot find -l").nth(1) {
            let name: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ':' && *c != '\'' && *c != '"')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Scan a failed rustc stderr for a missing system linker. This is an
/// environment/toolchain problem (L2101), not a generated-Rust ICE.
fn missing_linker(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        if let Some(rest) = line.split("linker `").nth(1) {
            if let Some((name, tail)) = rest.split_once('`') {
                if !name.is_empty() && tail.contains("not found") {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod missing_c_lib_tests {
    use super::{missing_c_lib, missing_linker, native_cache_key, native_cache_key_with_toolchain, native_cache_salt, rustc_crate_name};

    struct ScratchProject(std::path::PathBuf);

    impl ScratchProject {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "jet_genmod_cache_{}_{}",
                std::process::id(),
                nonce
            ));
            std::fs::create_dir_all(&root).expect("create generic-module cache fixture");
            Self(root)
        }

        fn write(&self, name: &str, source: &str) {
            std::fs::write(self.0.join(name), source).expect("write generic-module cache fixture");
        }

        fn main(&self) -> String {
            self.0.join("main.jet").to_string_lossy().into_owned()
        }
    }

    impl Drop for ScratchProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn rustc_crate_name_sanitizes_user_facing_file_stems() {
        assert_eq!(rustc_crate_name("renderable-varargs.jet"), "renderable_varargs");
        assert_eq!(rustc_crate_name("3d.demo.jet"), "_3d_demo");
    }

    #[test]
    fn generic_instance_cache_salt_tracks_every_downstream_input() {
        let instances = vec!["instance-a".to_string(), "instance-b".to_string()];
        let base = native_cache_salt("tool-a", "deps-a", "run", "linux-x86_64", &instances);
        assert_ne!(base, native_cache_salt("tool-b", "deps-a", "run", "linux-x86_64", &instances));
        assert_ne!(base, native_cache_salt("tool-a", "deps-b", "run", "linux-x86_64", &instances));
        assert_ne!(base, native_cache_salt("tool-a", "deps-a", "test", "linux-x86_64", &instances));
        assert_ne!(base, native_cache_salt("tool-a", "deps-a", "run", "macos-aarch64", &instances));
        assert_ne!(base, native_cache_salt("tool-a", "deps-a", "run", "linux-x86_64", &["instance-c".into()]));
        assert_eq!(base, native_cache_salt("tool-a", "deps-a", "run", "linux-x86_64", &["instance-b".into(), "instance-a".into()]));
    }

    #[test]
    fn generic_instance_native_cache_key_invalidates_on_program_dependency_arg_package_and_profile_edits() {
        let project = ScratchProject::new();
        let main = "use defs.Box\nmodule defs\n\nmodule Selected = Box<Int, 3>\nfn run() { print(Selected.value()) }\n";
        let dependency = "pub module Box<T, n: Int> { pub fn value() -> Int { return n } }\n";
        let manifest_v1 = "payload: { name: \"cache-proof\", version: \"1.0.0\" }\n";
        project.write("main.jet", main);
        project.write("defs.jet", dependency);
        project.write("pkg.jet", manifest_v1);

        let base = native_cache_key(&project.main(), "default", "run").expect("base cache key");
        let toolchain_a = native_cache_key_with_toolchain(&project.main(), "default", "run", "compiler-build-a/rustc-a/linker-a/backend-a").expect("toolchain A cache key");
        let toolchain_b = native_cache_key_with_toolchain(&project.main(), "default", "run", "compiler-build-b/rustc-a/linker-a/backend-a").expect("toolchain B cache key");
        assert_ne!(toolchain_a, toolchain_b, "production native-cache key seam must include compiler/toolchain identity");

        project.write(
            "main.jet",
            "use defs.Box\nmodule defs\n\nmodule Selected = Box<Int, 3>\nfn run() { print(Selected.value() + 1) }\n",
        );
        let program_body = native_cache_key(&project.main(), "default", "run").expect("program body cache key");
        assert_ne!(base, program_body, "entry-body edit must invalidate native cache");

        project.write("main.jet", main);
        project.write(
            "defs.jet",
            "pub module Box<T, n: Int> { pub fn value() -> Int { return n + 1 } }\n",
        );
        let dependency_body = native_cache_key(&project.main(), "default", "run").expect("dependency cache key");
        assert_ne!(base, dependency_body, "imported template-body edit must invalidate native cache");

        project.write("defs.jet", dependency);
        project.write(
            "main.jet",
            "use defs.Box\nmodule defs\n\nmodule Selected = Box<Int, 4>\nfn run() { print(Selected.value()) }\n",
        );
        let argument = native_cache_key(&project.main(), "default", "run").expect("argument cache key");
        assert_ne!(base, argument, "normalized instance-argument edit must invalidate native cache");

        project.write("main.jet", main);
        project.write(
            "pkg.jet",
            "payload: { name: \"cache-proof\", version: \"2.0.0\" }\n",
        );
        let package = native_cache_key(&project.main(), "default", "run").expect("package cache key");
        assert_ne!(base, package, "package manifest edit must invalidate native cache");

        project.write("pkg.jet", manifest_v1);
        let profile = native_cache_key(&project.main(), "small", "run").expect("profile cache key");
        assert_ne!(base, profile, "build-profile edit must invalidate native cache");
    }

    #[test]
    fn detects_ld_cannot_find() {
        // GNU ld / lld phrasing — must be routed to E3209, not the I2 ICE banner.
        let stderr = "  = note: /usr/bin/ld: cannot find -lraylib: No such file or directory\n  collect2: error: ld returned 1 exit status\n";
        assert_eq!(missing_c_lib(stderr).as_deref(), Some("raylib"));
    }

    #[test]
    fn detects_bare_form() {
        assert_eq!(
            missing_c_lib("ld: cannot find -lsqlite3\n").as_deref(),
            Some("sqlite3")
        );
    }

    #[test]
    fn genuine_codegen_error_is_not_a_missing_lib() {
        // A real rustc type error must keep the ICE path (returns None here).
        let stderr = "error[E0308]: mismatched types\n --> build/main.rs:3:5\n";
        assert_eq!(missing_c_lib(stderr), None);
    }

    #[test]
    fn detects_missing_system_linker() {
        let stderr =
            "error: linker `cc` not found\n  |\n  = note: No such file or directory (os error 2)\n";
        assert_eq!(missing_linker(stderr).as_deref(), Some("cc"));
    }

    #[test]
    fn genuine_codegen_error_is_not_a_missing_linker() {
        let stderr = "error[E0425]: cannot find type `RaylibWindow` in module `jet_std`\n";
        assert_eq!(missing_linker(stderr), None);
    }
}
