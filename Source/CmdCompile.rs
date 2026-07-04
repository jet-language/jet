//! check / build / run / test / new / fmt / fix subcommand handlers + the
//! rustc bridge.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use jet::ExitCodes;

use crate::{report_problems, usage, BuildProfile, OutputMode, ProfileConfig};

/// D-BUILDPROFILE1: load `pkg.jet` build profiles from the project root of `source_file`.
fn load_pkg_profiles(
    source_file: &str,
) -> Option<Vec<jet::Jetpack::PackageManifest::BuildProfileDef>> {
    let src_path = std::path::Path::new(source_file);
    let search_from = src_path.parent().unwrap_or(std::path::Path::new("."));
    let root = jet::Loader::find_manifest_root(search_from)?;
    let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).ok()?;
    jet::Jetpack::PackageManifest::parse(&raw)
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

    let compile_result = if is_web {
        jet::compile_web(file)
    } else if is_plugin {
        jet::compile_plugin(file)
    } else if freestanding {
        jet::compile_freestanding(file)
    } else if allow_impure {
        jet::compile_allow_impure(file)
    } else {
        // D-OSTARGET1=A: thread the real `--target=<triple>` through so
        // codegen only emits/links `#Target(Os.*)`-gated impls for the OS
        // that triple builds for (host OS when the flag is absent).
        jet::compile_with_target(&src, file, cross_target)
    };
    let (rust_code, ffi_link, clinks, capabilities, web_out, web_partition_report, plugin_out) =
        match compile_result {
            Ok(out) => {
                // D-A11YGATE1=B (c134 Phase 6): a11y lints (E2930/E2931) are opt-in
                // via `jet lint --a11y`; ordinary build/run never surfaces them.
                let lints = crate::CmdDevTools::visible_lints(&out.lints);
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
                jet::Jetpack::EffectBudget::compute_package_effects(bundle, &effect_facts.solved);
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
            eprintln!("{}", jet::Jetpack::EffectBudget::summary_line(&entries));
            let search_from = Path::new(file).parent().unwrap_or(Path::new("."));
            if let Some(root) = jet::Loader::find_manifest_root(search_from) {
                let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
                if let Some(manifest) = fs::read_to_string(&pack_path)
                    .ok()
                    .and_then(|raw| jet::Jetpack::PackageManifest::parse(&raw).ok())
                {
                    let violations = jet::Jetpack::EffectBudget::enforce(&entries, &manifest);
                    if !violations.is_empty() {
                        report_problems(mode, file, &src, &violations);
                        exit(ExitCodes::USER_ERROR);
                    }
                    // Record per-dependency effect provenance + grants in the
                    // lockfile, when one already exists (`jet fetch` owns
                    // creating it).
                    if let Some(mut lock) = jet::Lock::load(&root) {
                        jet::Jetpack::EffectBudget::update_lock_provenance(
                            &mut lock, &entries, &manifest,
                        );
                        let _ = fs::write(
                            jet::Jetpack::Store::lock_path(&root),
                            jet::Lock::write(&lock),
                        );
                    }
                }
            }
        }
    }

    match cmd {
        "build" => {
            build(
                file,
                &rust_code,
                bin_path(file),
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
/// what happens next — normally configuring and starting a `core.devserver`
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
        toolchains: Vec::new(),    });

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
    // Create: <name>/pkg.jet, <name>/.jet/main.jet, <name>/.gitignore
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
    let main_src = "fn run() {\n    print(\"hello, world\");\n}\n";
    fs::write(jet_dir.join("main.jet"), main_src).unwrap_or_else(|e| {
        eprintln!("error: couldn't write .jet/main.jet: {}", e);
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
    println!("  .jet/main.jet");
    println!("  .gitignore");
    println!("next: cd {} && {} run", name, jet::Syntax::BINARY_NAME);
}

pub(crate) fn run_test(path: &str, update_snapshots: bool, mode: OutputMode) {
    run_test_cov(path, update_snapshots, false, mode)
}

/// `jet test [--coverage]`. With `coverage`, the harness is built with line/
/// function probes (D-COV1) and a per-function coverage report prints after the
/// test results.
pub(crate) fn run_test_cov(path: &str, _update_snapshots: bool, coverage: bool, mode: OutputMode) {
    let p = Path::new(path);
    if !p.exists() {
        eprintln!("error: can't find `{}`", path);
        exit(ExitCodes::USER_ERROR);
    }
    if p.is_dir() {
        let ext = jet::Syntax::FILE_EXT;
        let mut files: Vec<PathBuf> = fs::read_dir(p)
            .unwrap_or_else(|e| {
                eprintln!("error: couldn't read `{}`: {}", path, e);
                exit(ExitCodes::USER_ERROR);
            })
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|f| f.extension().and_then(|e| e.to_str()) == Some(ext))
            .collect();
        files.sort();
        if files.is_empty() {
            eprintln!("error: no .{} files in `{}`", ext, path);
            exit(ExitCodes::USER_ERROR);
        }
        let mut any_fail = false;
        for f in files {
            if !run_test_file(&f, coverage, mode) {
                any_fail = true;
            }
        }
        exit(if any_fail {
            ExitCodes::USER_ERROR
        } else {
            ExitCodes::OK
        });
    }
    exit(if run_test_file(p, coverage, mode) {
        ExitCodes::OK
    } else {
        ExitCodes::USER_ERROR
    });
}

fn run_test_file(path: &Path, coverage: bool, mode: OutputMode) -> bool {
    let shown = path.to_string_lossy();
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: couldn't read `{}`: {}", shown, e);
            return false;
        }
    };
    // D-TEST4: discover and run any `///` doctest examples first. They are
    // independent of `#Test` blocks, so a file with only doctests is testable.
    let has_doctests = !jet::Doctest::discover(&src).is_empty();
    let doctests_ok = run_doctests(&shown, &src, mode);

    // A file with doctests but no `#Test` blocks is testable on its doctests
    // alone — skip the test harness (which would otherwise error E0601 "no #Test
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
    if let Some(co) = &cov_out {
        let _ = fs::remove_file(co);
        cmd.env("JET_COV_OUT", co);
    }
    let out = cmd.output().unwrap_or_else(|e| {
        eprintln!("error: couldn't run tests in `{}`: {}", shown, e);
        exit(ExitCodes::USER_ERROR);
    });
    print!("{}", String::from_utf8_lossy(&out.stdout));
    if !out.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
    }
    if let Some(co) = &cov_out {
        report_coverage(&shown, co);
    }
    out.status.success() && doctests_ok
}

/// D-TEST4: compile and run each ```` ```jet ```` doctest block found in the file's
/// `///` doc comments, comparing each `// =>` line to the produced value. A
/// mismatch fires E2901; a block that fails to compile is reported with its own
/// diagnostics. Returns true when every block (if any) passed. A file with no
/// doctests passes trivially and prints nothing.
fn run_doctests(shown: &str, src: &str, mode: OutputMode) -> bool {
    let blocks = jet::Doctest::discover(src);
    if blocks.is_empty() {
        return true;
    }
    let mut all_ok = true;
    for (n, block) in blocks.iter().enumerate() {
        let label = format!("doctest at {}:{}", shown, block.fence_line);
        let program = jet::Doctest::synth_program(block);
        // Write the synthetic program to a temp file next to the build dir so the
        // normal compile+build pipeline can consume it.
        let _ = fs::create_dir_all("build");
        let tmp = PathBuf::from("build").join(format!("{}__doctest_{}.jet", stem(shown), n));
        if fs::write(&tmp, &program).is_err() {
            eprintln!("error: couldn't stage doctest from `{}`", shown);
            all_ok = false;
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
                let _ = fs::remove_file(&tmp);
                continue;
            }
        };
        let bin = tmp.with_extension("");
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
                let _ = fs::remove_file(&tmp);
                continue;
            }
        };
        let _ = fs::remove_file(&tmp);
        if !out.status.success() {
            println!("{}: FAIL (runtime error)", label);
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
            all_ok = false;
            continue;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let produced: Vec<&str> = stdout.lines().collect();
        let mut block_ok = true;
        for (i, e) in block.expects.iter().enumerate() {
            let actual = produced.get(i).copied().unwrap_or("");
            if actual != e.expected {
                block_ok = false;
                all_ok = false;
                let span = doc_line_span(src, e.line);
                let diag = jet::Doctest::mismatch_diag(shown, e, actual, span);
                // Render against the original file so the line points at the doc
                // comment's producing line.
                eprint!("{}", jet::render_diagnostics(shown, src, &[diag]));
            }
        }
        println!("{}: {}", label, if block_ok { "pass" } else { "FAIL" });
    }
    all_ok
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

pub(crate) fn run_fmt(file: &str, check_only: bool) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let formatted = match jet::format_source(&src) {
        Ok(s) => s,
        Err(diags) => {
            eprint!("{}", jet::render_diagnostics(file, &src, &diags));
            exit(ExitCodes::USER_ERROR);
        }
    };
    if formatted == src {
        return;
    }
    if check_only {
        print!("{}", jet::Formatter::unified_diff(file, &src, &formatted));
        exit(ExitCodes::USER_ERROR);
    }
    fs::write(file, &formatted).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", file, e);
        exit(ExitCodes::USER_ERROR);
    });
}

pub(crate) fn stem(file: &str) -> String {
    Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".to_string())
        .replace('.', "_")
}

fn bin_path(file: &str) -> PathBuf {
    PathBuf::from("build").join(stem(file))
}

fn test_bin_path(path: &Path) -> PathBuf {
    PathBuf::from("build").join(format!("test_{}", stem(&path.to_string_lossy())))
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
    let bundle = jet::Loader::load_entry_with_overlay(file, None, false).ok()?;
    let uses_embed = bundle.modules.iter().any(|m| {
        m.source.contains(jet::Syntax::BUILTIN_EMBED_FILE)
            || m.source.contains(jet::Syntax::BUILTIN_EMBED_BYTES)
    });
    if uses_embed {
        return None;
    }
    let salt = format!(
        "{}\u{1}{}\u{1}{}",
        jet::Manifest::COMPILER_VERSION,
        manifest_fingerprint(file),
        mode_tag,
    );
    Some(jet::CanonicalAST::ast_cache_key(&bundle, profile_tag, &salt))
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
    // (1) an explicit `#Html("path.html")` marker, relative to the source
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
                "error: `#Html(\"{}\")` names a file that doesn't exist: {} ({})",
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
        .map_err(|e| {
            PluginBuildError::ToolFailure(format!("couldn't run `wasm-tools` ({e})"))
        })?;
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
    cmd.arg("--crate-name").arg(stem(file));
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
/// DAP server (`--dap`) drives it through `Source/Debug/Inferior.rs`. Returns
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

#[cfg(test)]
mod missing_c_lib_tests {
    use super::missing_c_lib;

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
}
