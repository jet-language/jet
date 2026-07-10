//! dev / repl / doctor / explain / completions / bind / eval / emit / bench
//! developer-tooling subcommand handlers.

use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use jet::Diagnostics::ColorChoice;
use jet::ExitCodes;

use crate::CmdCompile::{build, stem};
use crate::{report_problems, BuildProfile, OutputMode};

/// c77 (D-DEVMODE1=A): how the watch loop reacts to a save. The default is
/// auto-detection (`detect_dev_mode`); the three expert overrides force a mode.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WatchPolicy {
    /// Auto-detect run-to-completion vs resident on each save (the default).
    Auto,
    /// `--restart`: always rerun from scratch (force run-to-completion).
    Restart,
    /// `--swap`/`jet serve`: always take the swap-or-announced-restart path.
    Swap,
    /// `--watch=off`: run once and exit, no loop.
    Once,
}

/// Parse the c77 dev/serve flags out of the raw argv. Unknown flags are left
/// for the caller; this only recognizes `--restart`, `--swap`, `--watch=off`.
pub(crate) fn watch_policy_from(raw: &[String], default: WatchPolicy) -> WatchPolicy {
    let mut policy = default;
    for a in raw {
        match a.as_str() {
            "--restart" => policy = WatchPolicy::Restart,
            "--swap" => policy = WatchPolicy::Swap,
            "--watch=off" => policy = WatchPolicy::Once,
            _ => {}
        }
    }
    policy
}

/// `jet dev <file>` — the E2-M4 watch/interpret loop (D-DEV4), extended by c77
/// with three-mode routing (D-DEVMODE1=A) and hot-swap/restart (D-HOTSWAP1=B).
/// Re-checks and re-runs the entry file on every save, streaming output. The
/// per-iteration work lives in `jet::Interpreter::dev_iteration` (so it can be
/// golden-tested); this is the thin std-only watcher around it (I6: no `notify`
/// crate — we poll the file's mtime in a loop).
pub(crate) fn run_dev(
    file: &str,
    try_anyway: bool,
    policy: WatchPolicy,
    mode: OutputMode,
    use_interpreter: bool,
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

    // `--watch=off`: run once and exit (no loop).
    if policy == WatchPolicy::Once {
        render_dev_iteration(file, try_anyway, mode, use_interpreter);
        return;
    }

    println!("watching {} … (Ctrl-C to stop)", file);

    // The bundle from the last successful load, kept so a resident edit can be
    // diffed against it for type stability (D-HOTSWAP1).
    let mut prev_bundle = render_dev_iteration(file, try_anyway, mode, use_interpreter);
    let mut last_mtime = file_mtime(path);

    loop {
        std::thread::sleep(std::time::Duration::from_millis(120));
        let now = file_mtime(path);
        if now != last_mtime {
            last_mtime = now;
            // A debounce sleep lets editors finish writing before we read.
            std::thread::sleep(std::time::Duration::from_millis(30));
            prev_bundle = render_dev_change(
                file,
                try_anyway,
                policy,
                prev_bundle.as_ref(),
                mode,
                use_interpreter,
            );
        }
    }
}

/// `jet serve <entry>` (c77) — `jet dev <entry> --swap`: force the resident/
/// swap path, so a type-stable edit hot-swaps and a type-changing edit announces
/// a clean restart. Shares the whole watch loop with `jet dev`. `policy`
/// defaults to `Swap` but `--watch=off` (run once) and the other overrides
/// still apply.
///
/// `use_interpreter` — D-JIT2=A opt-out flag (`--interpret`): forces tier-0
/// interpreter; otherwise `CraneliftBackend` is used (M0 still delegates,
/// M1+ will JIT-compile the covered subset).
pub(crate) fn run_serve(
    file: &str,
    try_anyway: bool,
    policy: WatchPolicy,
    mode: OutputMode,
    use_interpreter: bool,
) {
    run_dev(file, try_anyway, policy, mode, use_interpreter);
}

/// Handle one detected file change: pick swap vs rerun vs restart and render.
/// Returns the freshly loaded bundle (or `None` if it failed to load) for the
/// next diff.
fn render_dev_change(
    file: &str,
    try_anyway: bool,
    policy: WatchPolicy,
    prev: Option<&jet::AST::ProgramBundle>,
    mode: OutputMode,
    use_interpreter: bool,
) -> Option<jet::AST::ProgramBundle> {
    // Load+check the new bundle so we can both diff its type surface and run it.
    let new_bundle = match jet::Loader::load_entry(file) {
        Ok(mut b) => {
            let diags = jet::Sema::check_bundle(&mut b, jet::Sema::CompileMode::Run);
            let errs: Vec<_> = diags
                .into_iter()
                .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
                .collect();
            if !errs.is_empty() {
                let src = fs::read_to_string(file).unwrap_or_default();
                println!("\n— {} changed —", file);
                report_problems(mode, file, &src, &errs);
                // Keep the previous bundle as the swap baseline; the bad edit
                // never became the running version.
                return None;
            }
            b
        }
        Err(diags) => {
            let src = fs::read_to_string(file).unwrap_or_default();
            println!("\n— {} changed —", file);
            report_problems(mode, file, &src, &diags);
            return None;
        }
    };

    // Decide whether this save uses the swap path.
    let resident = match policy {
        WatchPolicy::Swap => true,
        WatchPolicy::Restart => false,
        WatchPolicy::Once => false, // unreachable here (handled in run_dev)
        WatchPolicy::Auto => {
            jet::Interpreter::detect_dev_mode(&new_bundle) == jet::Interpreter::DevMode::Resident
        }
    };

    if resident {
        // The hot-reload unit is the entry module (D-HOTSWAP1).
        let module_name = new_bundle
            .modules
            .get(new_bundle.entry)
            .map(|m| m.display.clone())
            .unwrap_or_else(|| file.to_string());

        match prev {
            Some(old) => {
                match jet::Sema::HotSwap::type_stable_check(old, &new_bundle, &module_name) {
                    Ok(()) => {
                        println!(
                            "\n[hot-swap] {} — types stable, code re-applied",
                            module_name
                        );
                        run_resident_swap(
                            &new_bundle,
                            try_anyway,
                            &module_name,
                            file,
                            mode,
                            use_interpreter,
                        );
                    }
                    Err(diags) => {
                        // E2210 names what changed; surface it on the restart line.
                        let what = diags.first().map(|d| d.what.clone()).unwrap_or_default();
                        println!("\n[restart] {} — {}", module_name, what);
                        run_resident_restart(&new_bundle, try_anyway, file, mode, use_interpreter);
                    }
                }
            }
            None => {
                // No baseline yet (first run after an error): a clean restart.
                println!("\n[restart] {} — first run", module_name);
                run_resident_restart(&new_bundle, try_anyway, file, mode, use_interpreter);
            }
        }
    } else {
        // Run-to-completion (default / `--restart`): plain rerun.
        println!("\n— {} changed, re-running —", file);
        render_outcome(
            jet::Interpreter::dev_iteration(file, try_anyway, use_interpreter),
            file,
            mode,
        );
    }

    Some(new_bundle)
}

/// Hot-swap via the JitBackend seam. `use_interpreter` forces tier-0;
/// otherwise `CraneliftBackend` uses the same transparent AOT fallback ladder
/// as `dev_iteration` before reaching the interpreter boundary.
fn run_resident_swap(
    bundle: &jet::AST::ProgramBundle,
    try_anyway: bool,
    module_name: &str,
    file: &str,
    mode: OutputMode,
    use_interpreter: bool,
) {
    use jet::JitBackend::{AotFallbackBackend, InterpreterBackend, JitBackend};
    use jet_jit::CraneliftBackend;
    let outcome = if use_interpreter {
        let mut b = InterpreterBackend::new();
        b.hot_swap(module_name, bundle, try_anyway)
    } else {
        let mut b = CraneliftBackend::new(AotFallbackBackend::new(InterpreterBackend::new()));
        b.hot_swap(module_name, bundle, try_anyway)
    };
    match outcome {
        Ok(o) => render_outcome(o, file, mode),
        Err(diags) => {
            let src = fs::read_to_string(file).unwrap_or_default();
            report_problems(mode, file, &src, &diags);
        }
    }
}

/// Clean restart via the JitBackend seam.
fn run_resident_restart(
    bundle: &jet::AST::ProgramBundle,
    try_anyway: bool,
    file: &str,
    mode: OutputMode,
    use_interpreter: bool,
) {
    use jet::JitBackend::{AotFallbackBackend, InterpreterBackend, JitBackend};
    use jet_jit::CraneliftBackend;
    let outcome = if use_interpreter {
        let mut b = InterpreterBackend::new();
        b.restart(bundle, try_anyway)
    } else {
        let mut b = CraneliftBackend::new(AotFallbackBackend::new(InterpreterBackend::new()));
        b.restart(bundle, try_anyway)
    };
    render_outcome(outcome, file, mode);
}

/// `jet repl` — interactive REPL session (E2-M18, D-REPL3=A).
/// `project_dir` sets the base for `:load` paths and (eventually) import
/// context (D-REPL10=A sandbox; `--project <dir>` enables project mode).
pub(crate) fn run_repl(project_dir: Option<&str>) {
    let code = jet::REPL::run(project_dir);
    exit(code);
}

/// Last-modified time of a path, or `None` if it can't be read (treated as a
/// distinct state so a transient unlink/rewrite still triggers a re-run).
/// `pub(crate)`: also reused verbatim by `CmdDevWeb::run_dev_web` (c134
/// Phase 7) — same mtime-poll pattern, no `notify` crate (I6).
pub(crate) fn file_mtime(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Run one dev iteration and render its outcome to the terminal in the active
/// output mode. Diagnostics use the SAME renderer as batch compilation
/// (D-DEV), so a problem looks identical whether seen via `jet check` or
/// `jet dev`.
fn render_dev_iteration(
    file: &str,
    try_anyway: bool,
    mode: OutputMode,
    use_interpreter: bool,
) -> Option<jet::AST::ProgramBundle> {
    let started = std::time::Instant::now();
    let outcome = jet::Interpreter::dev_iteration(file, try_anyway, use_interpreter);
    let elapsed = started.elapsed();
    let ran_ok = matches!(outcome, jet::Interpreter::RunOutcome::Ran { .. });
    render_outcome_timed(outcome, file, Some(elapsed), mode);
    // Load the checked bundle once more as the swap baseline — only when the
    // program actually ran (a broken file has no running version to diff).
    if ran_ok {
        jet::Loader::load_entry(file).ok().map(|mut b| {
            let _ = jet::Sema::check_bundle(&mut b, jet::Sema::CompileMode::Run);
            b
        })
    } else {
        None
    }
}

/// Render a run outcome with no timing line (used for re-runs/swaps).
fn render_outcome(outcome: jet::Interpreter::RunOutcome, file: &str, mode: OutputMode) {
    render_outcome_timed(outcome, file, None, mode);
}

fn render_outcome_timed(
    outcome: jet::Interpreter::RunOutcome,
    file: &str,
    elapsed: Option<std::time::Duration>,
    mode: OutputMode,
) {
    match outcome {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, .. } => {
            print!("{}", stdout);
            if !stderr.is_empty() {
                eprint!("{}", stderr);
            }
            if let Some(e) = elapsed {
                println!("✓ ran in {} ms", e.as_millis());
            }
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            let src = fs::read_to_string(file).unwrap_or_default();
            report_problems(mode, file, &src, &diags);
        }
    }
}

/// `jet completions <shell>` — print a shell completion script (D-DX4).
pub(crate) fn run_completions(shell: Option<&str>) {
    let out = match shell {
        Some("bash") => jet::CLI::completions_bash(),
        Some("zsh") => jet::CLI::completions_zsh(),
        Some("fish") => jet::CLI::completions_fish(),
        other => {
            eprintln!(
                "error: completions need a shell: {} completions <bash|zsh|fish>",
                jet::Syntax::BINARY_NAME
            );
            if let Some(s) = other {
                eprintln!(" why: `{}` isn't a shell I generate completions for", s);
            }
            exit(ExitCodes::USAGE);
        }
    };
    print!("{}", out);
}

/// Every `jet devtools` subcommand name, for the usage line and typo errors.
const DEVTOOLS_SUBCOMMANDS: &str =
    "grammars | reduce | ice-report | new-example | new-ui | check-fixture-paths | bless";

/// `jet devtools grammars` — D-HL1 generated lexical base for editor grammars.
/// c450 (D-DEVTOOLS1=A): extended with maintainer-facing minimizer/scaffolding
/// tools, all under this same hidden namespace (never top-level commands).
pub(crate) fn run_devtools(args: &[&String]) {
    match args.first().map(|s| s.as_str()) {
        Some("grammars") => {
            write_generated_section(
                "editors/vscode/syntaxes/jet.tmLanguage.json",
                &jet::Syntax::render_vscode_generated_highlights(),
            );
            write_generated_section(
                "editors/tree-sitter/grammar.js",
                &jet::Syntax::render_tree_sitter_generated_highlights(),
            );
            write_generated_section(
                "editors/zed/languages/jet/highlights.scm",
                &jet::Syntax::render_zed_generated_highlights(),
            );
            println!("regenerated editor grammar sections");
        }
        Some("reduce") => run_devtools_reduce(&args[1..]),
        Some("ice-report") => run_devtools_ice_report(&args[1..]),
        Some("new-example") => run_devtools_new_example(&args[1..]),
        Some("new-ui") => run_devtools_new_ui(&args[1..]),
        Some("check-fixture-paths") => run_devtools_check_fixture_paths(),
        Some("bless") => run_devtools_bless(&args[1..]),
        Some(other) => {
            eprintln!("error: unknown `devtools` subcommand `{}`", other);
            eprintln!("usage: {} devtools <{}>", jet::Syntax::BINARY_NAME, DEVTOOLS_SUBCOMMANDS);
            exit(ExitCodes::USAGE);
        }
        None => {
            eprintln!("usage: {} devtools <{}>", jet::Syntax::BINARY_NAME, DEVTOOLS_SUBCOMMANDS);
            exit(ExitCodes::USAGE);
        }
    }
}

// ──────────────────────────────────────────────
// c450: `jet devtools reduce` — delta-debugging minimizer.
// ──────────────────────────────────────────────

/// `jet devtools reduce <file.jet> [--code EXXXX]`.
///
/// Oracle (what makes a candidate "still interesting"):
///   - default: the front end accepts the file AND rustc rejects the
///     generated Rust (an I2 repro — invariant I2 says this must never
///     happen in a shipped compiler, so a minimal repro is worth having);
///   - `--code EXXXX`: the front end emits diagnostic `EXXXX` (from either
///     an error result or a lint on a successful compile).
///
/// Shrinks by removing line-chunks (a simplified Zeller ddmin: try removing
/// progressively smaller contiguous chunks, re-trying the whole file at a
/// finer granularity whenever a whole pass makes no progress) and writes the
/// smallest failing case to `<file>.reduced.<ext>`.
pub(crate) fn run_devtools_reduce(args: &[&String]) {
    if args.is_empty() {
        eprintln!(
            "usage: {} devtools reduce <file.{}> [--code EXXXX]",
            jet::Syntax::BINARY_NAME,
            jet::Syntax::FILE_EXT
        );
        exit(ExitCodes::USAGE);
    }
    let file = args[0].as_str();
    let mut code_filter: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--code" => {
                code_filter = args.get(i + 1).map(|s| s.to_string());
                i += 2;
            }
            other => {
                eprintln!("error: unknown `reduce` flag `{}`", other);
                exit(ExitCodes::USAGE);
            }
        }
    }

    let src = fs::read_to_string(file).unwrap_or_else(|e| {
        eprintln!("error: couldn't read `{}`: {}", file, e);
        exit(ExitCodes::USER_ERROR);
    });

    let interesting = |text: &str| reduce_oracle(text, code_filter.as_deref());

    if !interesting(&src) {
        eprintln!("error: `{}` doesn't reproduce the target oracle as given", file);
        match &code_filter {
            Some(c) => eprintln!(" why: the front end never emits `{}` for this file", c),
            None => eprintln!(
                " why: either the front end already rejects it, or rustc accepts the generated Rust"
            ),
        }
        eprintln!(" fix: confirm the case fails the way you expect, then reduce it");
        exit(ExitCodes::USER_ERROR);
    }

    let lines: Vec<String> = src.lines().map(|s| s.to_string()).collect();
    println!("reduce: starting at {} line(s)", lines.len());
    let reduced = ddmin(lines, &interesting);
    println!("reduce: finished at {} line(s)", reduced.len());

    let mut out_text = reduced.join("\n");
    if !out_text.is_empty() {
        out_text.push('\n');
    }
    let out_path = reduced_path(file);
    fs::write(&out_path, &out_text).unwrap_or_else(|e| {
        eprintln!("error: couldn't write `{}`: {}", out_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    println!("wrote {}", out_path.display());
}

/// Whether `src` still triggers the target oracle. `code`: `None` = default
/// I2 oracle (front end accepts, rustc rejects); `Some(code)` = the front end
/// emits that diagnostic code (error or lint).
fn reduce_oracle(src: &str, code: Option<&str>) -> bool {
    match jet::compile_with_path(src, "reduce_candidate.jet") {
        Ok(out) => match code {
            Some(c) => out.lints.iter().any(|d| d.code == c),
            None => rustc_rejects(&out.rust),
        },
        Err(diags) => match code {
            Some(c) => diags.iter().any(|d| d.code == c),
            None => false, // default oracle needs the front end to accept
        },
    }
}

/// Simplified Zeller ddmin over line-chunks: repeatedly try removing a
/// contiguous chunk of the current granularity; on any removal that keeps the
/// oracle interesting, keep the shrunk version and tighten the granularity;
/// on a full pass with no progress, double the chunk count (finer chunks)
/// until we're at single-line granularity, then stop.
fn ddmin(lines: Vec<String>, interesting: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut current = lines;
    let mut n: usize = 2;
    loop {
        if current.len() < 2 {
            break;
        }
        let chunk_size = (current.len() + n - 1) / n;
        if chunk_size == 0 {
            break;
        }
        let mut made_progress = false;
        let mut start = 0;
        while start < current.len() {
            let end = (start + chunk_size).min(current.len());
            let mut candidate = current.clone();
            candidate.drain(start..end);
            if !candidate.is_empty() && interesting(&candidate.join("\n")) {
                println!(
                    "reduce: dropped lines {}..{} -> {} line(s) left",
                    start + 1,
                    end,
                    candidate.len()
                );
                current = candidate;
                n = if n > 2 { n - 1 } else { 2 };
                made_progress = true;
                break;
            }
            start = end;
        }
        if !made_progress {
            if n >= current.len() {
                break;
            }
            n = (n * 2).min(current.len());
        }
    }
    current
}

/// `<file>.reduced.<ext>` sibling path for the reducer's output.
fn reduced_path(file: &str) -> PathBuf {
    let p = Path::new(file);
    let stem_name = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("reduced");
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or(jet::Syntax::FILE_EXT);
    let name = format!("{}.reduced.{}", stem_name, ext);
    match p.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

/// Run rustc over generated Rust source with no linking (`--emit=metadata`,
/// I2's oracle only cares whether rustc accepts the code, not whether it
/// links). Returns `(accepted, stderr)`; a rustc that can't even be invoked
/// counts as "accepted" so a missing toolchain never manufactures a false
/// I2 repro.
fn rustc_probe(rust_code: &str) -> (bool, String) {
    let tmp_dir = std::env::temp_dir().join(format!("jet_devtools_rustc_{}", std::process::id()));
    if fs::create_dir_all(&tmp_dir).is_err() {
        return (true, String::new());
    }
    let rs_path = tmp_dir.join("check.rs");
    if fs::write(&rs_path, rust_code).is_err() {
        let _ = fs::remove_dir_all(&tmp_dir);
        return (true, String::new());
    }
    let meta_path = tmp_dir.join("check.rmeta");
    let result = Command::new("rustc")
        .arg("--edition")
        .arg("2021")
        .arg("--crate-name")
        .arg("jet_devtools_check")
        .arg("--emit=metadata")
        .arg(&rs_path)
        .arg("-o")
        .arg(&meta_path)
        .output();
    let outcome = match result {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(_) => (true, String::new()),
    };
    let _ = fs::remove_dir_all(&tmp_dir);
    outcome
}

fn rustc_rejects(rust_code: &str) -> bool {
    !rustc_probe(rust_code).0
}

// ──────────────────────────────────────────────
// c450: `jet devtools ice-report` — bundle an I2 repro for a bug report.
// ──────────────────────────────────────────────

/// `jet devtools ice-report <file.jet>` — bundles the source, generated Rust,
/// rustc's stderr, and both tool versions into one directory under
/// `.jet/ice-report/<stem>-<unix-time>/` so a bug report has everything
/// attached in one place. Prints the bundle path.
pub(crate) fn run_devtools_ice_report(args: &[&String]) {
    if args.is_empty() {
        eprintln!(
            "usage: {} devtools ice-report <file.{}>",
            jet::Syntax::BINARY_NAME,
            jet::Syntax::FILE_EXT
        );
        exit(ExitCodes::USAGE);
    }
    let file = args[0].as_str();
    let src = fs::read_to_string(file).unwrap_or_else(|e| {
        eprintln!("error: couldn't read `{}`: {}", file, e);
        exit(ExitCodes::USER_ERROR);
    });

    let out = match jet::compile_with_path(&src, file) {
        Ok(o) => o,
        Err(diags) => {
            eprintln!(
                "error: `{}` doesn't reach codegen — the front end already rejects it",
                file
            );
            eprintln!(" why: ice-report bundles a case that compiles to Rust (an I2 repro)");
            eprintln!(
                " fix: fix the front-end errors first, or use `{} devtools reduce --code <CODE>` \
                 to shrink a front-end diagnostic instead",
                jet::Syntax::BINARY_NAME
            );
            eprint!("{}", jet::render_diagnostics(file, &src, &diags));
            exit(ExitCodes::USER_ERROR);
        }
    };

    let (accepted, rustc_stderr) = rustc_probe(&out.rust);
    if accepted {
        println!(
            "note: rustc accepted the generated Rust for `{}` — bundling anyway, but this isn't an I2 repro",
            file
        );
    }

    let rustc_version = Command::new("rustc")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "rustc: not found".to_string());
    let jet_version = env!("CARGO_PKG_VERSION");

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let bundle_dir = PathBuf::from(".jet")
        .join("ice-report")
        .join(format!("{}-{}", stem(file), ts));
    fs::create_dir_all(&bundle_dir).unwrap_or_else(|e| {
        eprintln!("error: couldn't create `{}`: {}", bundle_dir.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    let write = |name: &str, content: &str| {
        fs::write(bundle_dir.join(name), content).unwrap_or_else(|e| {
            eprintln!("error: couldn't write `{}`: {}", name, e);
            exit(ExitCodes::USER_ERROR);
        });
    };
    write(&format!("source.{}", jet::Syntax::FILE_EXT), &src);
    write("generated.rs", &out.rust);
    write("rustc.stderr", &rustc_stderr);
    write(
        "versions.txt",
        &format!("jet {}\n{}\n", jet_version, rustc_version),
    );

    println!("wrote ICE report bundle to {}", bundle_dir.display());
}

// ──────────────────────────────────────────────
// c450: `jet devtools new-example` / `new-ui` — scaffold I5/I4 fixtures.
// ──────────────────────────────────────────────

/// `jet devtools new-example <topic>/<name>` — scaffolds
/// `examples/features/<topic>/<name>.jet` and
/// `examples/features/expected/<topic>/<name>.out`, matching the layout
/// `tests/golden.rs` walks exactly. The stub is a real, passing example (I5:
/// no example ships broken) that the author edits to demonstrate the feature.
pub(crate) fn run_devtools_new_example(args: &[&String]) {
    if args.is_empty() {
        eprintln!(
            "usage: {} devtools new-example <topic>/<name>",
            jet::Syntax::BINARY_NAME
        );
        exit(ExitCodes::USAGE);
    }
    let spec = args[0].as_str();
    let (topic, name) = match spec.split_once('/') {
        Some((t, n)) if !t.is_empty() && !n.is_empty() => (t, n),
        _ => {
            eprintln!("error: expected `<topic>/<name>`, got `{}`", spec);
            exit(ExitCodes::USER_ERROR);
        }
    };

    let ext = jet::Syntax::FILE_EXT;
    let example_dir = PathBuf::from("examples/features").join(topic);
    let expected_dir = PathBuf::from("examples/features/expected").join(topic);
    let example_path = example_dir.join(format!("{}.{}", name, ext));
    let expected_path = expected_dir.join(format!("{}.out", name));

    if example_path.exists() {
        eprintln!("error: `{}` already exists", example_path.display());
        exit(ExitCodes::USER_ERROR);
    }

    fs::create_dir_all(&example_dir).unwrap_or_else(|e| {
        eprintln!("error: couldn't create `{}`: {}", example_dir.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    fs::create_dir_all(&expected_dir).unwrap_or_else(|e| {
        eprintln!("error: couldn't create `{}`: {}", expected_dir.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    let greeting = format!("scaffold: {}/{}", topic, name);
    let src = format!(
        "// TODO: describe examples/features/{}/{}.{}\nfn run() {{\n    print(\"{}\")\n}}\n",
        topic, name, ext, greeting
    );
    fs::write(&example_path, &src).unwrap_or_else(|e| {
        eprintln!("error: couldn't write `{}`: {}", example_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    fs::write(&expected_path, format!("{}\n", greeting)).unwrap_or_else(|e| {
        eprintln!("error: couldn't write `{}`: {}", expected_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    println!("wrote {}", example_path.display());
    println!("wrote {}", expected_path.display());
}

/// `jet devtools new-ui <name>` — scaffolds `tests/ui/<name>.jet` and its
/// `<name>.stderr` snapshot, matching the layout `tests/diagnostic_snapshots.rs`
/// walks exactly. The stub triggers a real (if generic) diagnostic and its
/// `.stderr` is computed with the SAME calls the harness uses, so the pair is
/// valid the moment it's written — edit the `.jet` to demonstrate the real
/// diagnostic, then re-bless with `jet devtools bless diagnostic_snapshots`.
pub(crate) fn run_devtools_new_ui(args: &[&String]) {
    if args.is_empty() {
        eprintln!("usage: {} devtools new-ui <name>", jet::Syntax::BINARY_NAME);
        exit(ExitCodes::USAGE);
    }
    let name = args[0].as_str();
    if name.is_empty() || name.contains('/') || name.contains('.') {
        eprintln!(
            "error: `new-ui` takes a bare fixture name, no path or extension: got `{}`",
            name
        );
        exit(ExitCodes::USER_ERROR);
    }

    let ext = jet::Syntax::FILE_EXT;
    let dir = PathBuf::from("tests/ui");
    let jet_path = dir.join(format!("{}.{}", name, ext));
    let stderr_path = dir.join(format!("{}.stderr", name));

    if jet_path.exists() {
        eprintln!("error: `{}` already exists", jet_path.display());
        exit(ExitCodes::USER_ERROR);
    }
    fs::create_dir_all(&dir).unwrap_or_else(|e| {
        eprintln!("error: couldn't create `{}`: {}", dir.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    let src = "// TODO: describe what this diagnostic demonstrates.\n\
fn run() {\n    print(definitely_undefined_scaffold_symbol)\n}\n"
        .to_string();
    fs::write(&jet_path, &src).unwrap_or_else(|e| {
        eprintln!("error: couldn't write `{}`: {}", jet_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    // Mirror `tests/diagnostic_snapshots.rs`'s `ui_snapshots` exactly, so the
    // pair this writes is already a valid harness fixture.
    let shown_path = format!("tests/ui/{}.{}", name, ext);
    let actual = match jet::compile_with_path(&src, &shown_path) {
        Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
        Ok(_) => "(no errors)\n".to_string(),
    };
    fs::write(&stderr_path, &actual).unwrap_or_else(|e| {
        eprintln!("error: couldn't write `{}`: {}", stderr_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    println!("wrote {}", jet_path.display());
    println!("wrote {}", stderr_path.display());
    if actual == "(no errors)\n" {
        println!(
            "note: scaffold compiled cleanly — edit `{}` to trigger the diagnostic you want, \
             then `{} devtools bless diagnostic_snapshots`",
            jet_path.display(),
            jet::Syntax::BINARY_NAME
        );
    }
}

// ──────────────────────────────────────────────
// c450: `jet devtools check-fixture-paths` — validate hardcoded path fixtures.
// ──────────────────────────────────────────────

/// `jet devtools check-fixture-paths` — greps every `tests/**/*.rs` file for
/// hardcoded fixture path literals (`examples/features/...`, `docs/spec/...`,
/// `tests/ui/...`, etc.) and confirms each one exists on disk relative to the
/// current directory (run from the repo root). Path-embedding fixtures rot
/// silently when an example moves; this is the check that catches it.
pub(crate) fn run_devtools_check_fixture_paths() {
    let tests_dir = PathBuf::from("tests");
    if !tests_dir.is_dir() {
        eprintln!("error: no `tests/` directory here");
        eprintln!(" fix: run from the repo root");
        exit(ExitCodes::USER_ERROR);
    }

    let mut rs_files = Vec::new();
    collect_rs_files(&tests_dir, &mut rs_files);

    let mut checked = 0usize;
    let mut missing: Vec<(PathBuf, String)> = Vec::new();
    for rs in &rs_files {
        let text = fs::read_to_string(rs).unwrap_or_default();
        for candidate in extract_hardcoded_paths(&text) {
            checked += 1;
            if !Path::new(&candidate).exists() {
                missing.push((rs.clone(), candidate));
            }
        }
    }

    if missing.is_empty() {
        println!(
            "check-fixture-paths: {} embedded path(s) across {} file(s), all present",
            checked,
            rs_files.len()
        );
    } else {
        eprintln!("check-fixture-paths: {} missing path(s):", missing.len());
        for (file, path) in &missing {
            eprintln!("  {} -> `{}` does not exist", file.display(), path);
        }
        exit(ExitCodes::USER_ERROR);
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// Pull out string-literal path fixtures from Rust source text. Deliberately
/// conservative: only whole quoted literals (never `format!` templates with a
/// `{`) whose prefix marks them as a fixture path and whose suffix is a known
/// fixture extension, so dynamically-joined paths (already covered by their
/// own `read_to_string`/`unwrap_or_else` panics at test time) are left alone.
fn extract_hardcoded_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'"' {
                if bytes[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            if j <= text.len() {
                let lit = &text[start..j.min(text.len())];
                // `render_diagnostics`/`render_all_*` take a "shown" display
                // label as their file-name argument, not a path that must
                // exist (tests/net_tls.rs feeds one a made-up name purely so
                // the rendered banner looks like a real fixture path).
                let mut before_start = start.saturating_sub(40);
                while before_start < i && !text.is_char_boundary(before_start) {
                    before_start += 1;
                }
                let before = &text[before_start..i];
                let is_display_label = before.contains("render_diagnostics(")
                    || before.contains("render_all_colored(")
                    || before.contains("render_all_json(");
                if !is_display_label && is_hardcoded_fixture_path(lit) {
                    out.push(lit.to_string());
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn is_hardcoded_fixture_path(lit: &str) -> bool {
    let known_prefix = lit.starts_with("examples/features/")
        || lit.starts_with("docs/spec/")
        || lit.starts_with("tests/ui/")
        || lit.starts_with("tests/ui_lint/")
        || lit.starts_with("tests/cli/")
        || lit.starts_with("tests/release/");
    if !known_prefix || lit.contains('{') || lit.contains('}') {
        return false;
    }
    let ext = format!(".{}", jet::Syntax::FILE_EXT);
    lit.ends_with(&ext)
        || lit.ends_with(".md")
        || lit.ends_with(".out")
        || lit.ends_with(".stderr")
        || lit.ends_with(".warn")
        || lit.ends_with(".txt")
        || lit.ends_with(".snapshot")
}

// ──────────────────────────────────────────────
// c450: `jet devtools bless` — wrapper over the UPDATE_EXPECT re-bless convention.
// ──────────────────────────────────────────────

/// Every test binary that owns `UPDATE_EXPECT`-blessable snapshots (I4). Kept
/// as one list so `bless` never drifts from what actually re-blesses on
/// `UPDATE_EXPECT=1`.
pub(crate) const BLESS_TARGETS: &[&str] = &[
    "cli",
    "cross",
    "diagnostic_snapshots",
    "diagnostics_coverage",
    "release_gates",
];

/// `jet devtools bless [target...] [--dry-run]` — runs
/// `UPDATE_EXPECT=1 cargo test --test <target>` for each named target (all of
/// `BLESS_TARGETS` when none is given). This is the one re-bless mechanism in
/// the repo (see each test file's own "bless with UPDATE_EXPECT=1" doc
/// comment) — `bless` just names and runs it so nobody has to remember the
/// env var or the target list. `--dry-run` prints the commands without
/// running them (never mutates a snapshot file).
pub(crate) fn run_devtools_bless(args: &[&String]) {
    let mut dry_run = false;
    let mut requested: Vec<String> = Vec::new();
    for a in args {
        if a.as_str() == "--dry-run" {
            dry_run = true;
        } else {
            requested.push(a.to_string());
        }
    }

    let targets = match resolve_bless_targets(&requested) {
        Ok(t) => t,
        Err(unknown) => {
            eprintln!("error: unknown bless target(s): {}", unknown);
            eprintln!(
                "usage: {} devtools bless [target...] [--dry-run]",
                jet::Syntax::BINARY_NAME
            );
            eprintln!("known targets: {}", BLESS_TARGETS.join(", "));
            exit(ExitCodes::USAGE);
        }
    };

    if dry_run {
        for t in &targets {
            println!("would run: UPDATE_EXPECT=1 cargo test --test {}", t);
        }
        return;
    }

    let mut any_failed = false;
    for t in &targets {
        println!("bless: UPDATE_EXPECT=1 cargo test --test {}", t);
        match bless_command(t).status() {
            Ok(s) if s.success() => {}
            Ok(s) => {
                any_failed = true;
                eprintln!("bless: `{}` exited with {}", t, s);
            }
            Err(e) => {
                any_failed = true;
                eprintln!("bless: couldn't run `cargo test --test {}`: {}", t, e);
            }
        }
    }
    if any_failed {
        exit(ExitCodes::USER_ERROR);
    }
    println!(
        "bless: done ({} target{})",
        targets.len(),
        if targets.len() == 1 { "" } else { "s" }
    );
}

/// Resolve requested target names against `BLESS_TARGETS`; empty `requested`
/// means "all of them". `Err` carries the comma-joined unknown names.
pub(crate) fn resolve_bless_targets(requested: &[String]) -> Result<Vec<&'static str>, String> {
    if requested.is_empty() {
        return Ok(BLESS_TARGETS.to_vec());
    }
    let mut out = Vec::new();
    let mut unknown = Vec::new();
    for r in requested {
        match BLESS_TARGETS.iter().find(|k| **k == r.as_str()) {
            Some(t) => out.push(*t),
            None => unknown.push(r.clone()),
        }
    }
    if !unknown.is_empty() {
        return Err(unknown.join(", "));
    }
    Ok(out)
}

/// Build (never spawns) the `UPDATE_EXPECT=1 cargo test --test <target>`
/// command for one bless target.
pub(crate) fn bless_command(target: &str) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.env("UPDATE_EXPECT", "1");
    cmd.args(["test", "--test", target]);
    cmd
}

fn write_generated_section(path: &str, fresh: &str) {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read `{}`: {}", path, e);
        exit(ExitCodes::USER_ERROR);
    });
    let start = text
        .find(jet::Syntax::HIGHLIGHT_GENERATED_START)
        .unwrap_or_else(|| {
            eprintln!(
                "error: `{}` has no `{}` marker",
                path,
                jet::Syntax::HIGHLIGHT_GENERATED_START
            );
            exit(ExitCodes::USER_ERROR);
        });
    let prefix_start = text[..start].rfind('\n').map_or(0, |idx| idx + 1);
    let after_start = &text[start..];
    let end_rel = after_start
        .find(jet::Syntax::HIGHLIGHT_GENERATED_END)
        .unwrap_or_else(|| {
            eprintln!(
                "error: `{}` has no `{}` marker",
                path,
                jet::Syntax::HIGHLIGHT_GENERATED_END
            );
            exit(ExitCodes::USER_ERROR);
        });
    let end_marker = start + end_rel + jet::Syntax::HIGHLIGHT_GENERATED_END.len();
    let suffix_start = text[end_marker..]
        .find('\n')
        .map_or(text.len(), |idx| end_marker + idx + 1);

    let mut out = String::new();
    out.push_str(&text[..prefix_start]);
    out.push_str(fresh.trim_end());
    out.push('\n');
    out.push_str(&text[suffix_start..]);
    fs::write(path, out).unwrap_or_else(|e| {
        eprintln!("error: couldn't write `{}`: {}", path, e);
        exit(ExitCodes::USER_ERROR);
    });
    println!("wrote {}", path);
}

/// `jet doctor` — environment self-diagnosis with actionable fixes (D-DX2,
/// D-BUILD1). Offline by default; `--online` enables network checks; `--fix`
/// applies the auto-fixable problems. The advisory code for rustc/cache/PATH
/// problems is L2101.
pub(crate) fn run_doctor(online: bool, apply: bool, mode: OutputMode) {
    // E2-M15: `jet doctor --target=<triple>` checks cross-compilation readiness.
    let cross_target =
        std::env::args().find_map(|a| a.strip_prefix("--target=").map(str::to_string));
    let checks = jet::Doctor::run(jet::Doctor::Options {
        online,
        cross_target,
    });
    let color = mode.color_stderr_for(std::io::stdout().is_terminal());

    if apply {
        let fixed = jet::Doctor::apply_fixes(&checks);
        if fixed.is_empty() {
            println!("doctor: nothing to auto-fix");
        } else {
            for f in &fixed {
                println!("fixed: {}", f);
            }
        }
        // Re-run so the report reflects the world after fixes.
        return run_doctor(online, false, mode);
    }

    use jet::Doctor::Health;
    let bold = |s: &str| {
        if color {
            format!("\x1b[1m{}\x1b[0m", s)
        } else {
            s.to_string()
        }
    };
    let green = |s: &str| {
        if color {
            format!("\x1b[32m{}\x1b[0m", s)
        } else {
            s.to_string()
        }
    };
    let yellow = |s: &str| {
        if color {
            format!("\x1b[33m{}\x1b[0m", s)
        } else {
            s.to_string()
        }
    };

    println!("{}", bold(&format!("{} doctor", jet::Syntax::BINARY_NAME)));
    let mut last_section = "";
    for c in &checks {
        if c.section != last_section {
            println!();
            println!("{}", bold(c.section));
            last_section = c.section;
        }
        let (mark, label) = match c.health {
            Health::Ok => (green("ok  "), c.label.clone()),
            Health::Note => ("note".to_string(), c.label.clone()),
            Health::Problem => (yellow("warn"), c.label.clone()),
        };
        println!("  [{}] {}: {}", mark, label, c.detail);
        if let Some(fix) = &c.fix {
            println!("        Fix: {}", fix);
            if c.auto_fixable {
                println!(
                    "        (auto-fixable: run `{} doctor --fix`)",
                    jet::Syntax::BINARY_NAME
                );
            }
        }
    }
    println!();
    if jet::Doctor::has_problem(&checks) {
        // Advisory L2101 pointer, without making the report noisy.
        println!(
            "some checks need attention. {}",
            jet::Explain::pointer_line("L2101", color)
        );
        exit(ExitCodes::USER_ERROR);
    } else {
        println!("everything looks good.");
    }
}

/// `jet explain <CODE>` — print the offline essay for a diagnostic code.
pub(crate) fn run_explain(code: Option<&str>, mode: OutputMode) {
    let code = match code {
        Some(c) => c,
        None => {
            eprintln!(
                "usage: {} explain <CODE>   (e.g. {} explain E0102)",
                jet::Syntax::BINARY_NAME,
                jet::Syntax::BINARY_NAME
            );
            exit(ExitCodes::USAGE);
        }
    };
    match jet::Explain::lookup(code) {
        Some(ex) => {
            let color = ColorChoice::resolve(mode.color, std::io::stdout().is_terminal());
            print!("{}", jet::Explain::render(&ex, color));
        }
        None => {
            eprintln!("error: no diagnostic code `{}` exists", code);
            eprintln!(
                " fix: run a command that reports an error to see its code, e.g. `{} check file.{}`",
                jet::Syntax::BINARY_NAME,
                jet::Syntax::FILE_EXT
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// `jet bind <header.h> [--pkg <lib>] [-o <out.jet>]` (S59 / E2-M14 Phase 4).
///
/// Generates a `#Bindgen module c.<lib>.__bindgen__` cache from a C header,
/// using the same native std-only backend the compiler invokes on a cache miss
/// (owner 2026-06-18, supersedes D-CBIND3=B). Parses C function prototypes
/// over the bindable type subset; skips and reports what it cannot map (I3).
/// **E3208** fires only when the header is unreadable or has no bindable
/// prototypes — use `#Extern module c.<lib>` for those declarations.
pub(crate) fn run_bind(args: &[&String]) {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        eprintln!(
            "usage: {} bind <header.h> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        );
        eprintln!();
        eprintln!("Generate a C binding cache from a header (S59). The output is");
        eprintln!("a `#Bindgen module c.<lib>.__bindgen__` file, by default written");
        eprintln!("to .jet/bindings/c/<lib>.jet. The compiler also runs this");
        eprintln!("automatically on a cache miss; `bind` is the manual refresh.");
        exit(if args.is_empty() { 2 } else { 0 });
    }

    let header = args[0].as_str();
    let mut pkg: Option<String> = None;
    let mut out: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|s| s.to_string());
                i += 2;
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|s| s.to_string());
                i += 2;
            }
            other => {
                eprintln!("error: unknown `bind` flag `{}`", other);
                eprintln!(
                    "usage: {} bind <header.h> [--pkg <lib>] [-o <out.jet>]",
                    jet::Syntax::BINARY_NAME
                );
                exit(ExitCodes::USAGE);
            }
        }
    }

    // Link key: --pkg if given, else the header basename (header→lib rule).
    let lib = pkg.unwrap_or_else(|| {
        let base = header.rsplit('/').next().unwrap_or(header);
        base.strip_suffix(".h").unwrap_or(base).to_string()
    });

    let header_src = match std::fs::read_to_string(header) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "Error [E3208]: Could not generate bindings from `{}`.",
                header
            );
            eprintln!(" Why: the header file could not be read ({}).", e);
            eprintln!(" Fix: check the path, or install the library's dev headers.");
            exit(ExitCodes::USER_ERROR);
        }
    };

    // E2-M14 (owner 2026-06-18, supersedes D-CBIND3=B): native std-only backend.
    let result = match jet::CBind::generate(&header_src, &lib) {
        Ok(r) => r,
        Err(why) => {
            eprintln!(
                "Error [E3208]: Could not generate bindings from `{}`.",
                header
            );
            eprintln!(" Why: {}.", why);
            eprintln!(
                " Fix: hand-write `#Extern module c.{} {{ … }}` for the symbols you need.",
                lib
            );
            exit(ExitCodes::USER_ERROR);
        }
    };

    // Default cache path follows D-CBIND7: .jet/bindings/c/<lib>.jet.
    let out_path =
        out.unwrap_or_else(|| format!(".jet/bindings/c/{}.{}", lib, jet::Syntax::FILE_EXT));
    if let Some(parent) = std::path::Path::new(&out_path).parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("error: could not create `{}`: {}", parent.display(), e);
            exit(ExitCodes::USER_ERROR);
        }
    }
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        eprintln!("error: could not write `{}`: {}", out_path, e);
        exit(ExitCodes::USER_ERROR);
    }

    // Phase 3 (D-CBIND2): write a hash sidecar alongside the cache so the
    // compiler can detect stale caches on the next build (hash invalidation).
    // cflags are not yet threaded through `jet bind`; pass "" for now.
    let _ = jet::CBind::write_bind_hash(std::path::Path::new(&out_path), &header_src, "");

    println!(
        "bound {} function{} from `{}` → {}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" },
        header,
        out_path
    );
    if !result.skipped.is_empty() {
        println!(
            "skipped {} declaration{} outside the bindable subset (hand-write `#Extern` for these):",
            result.skipped.len(),
            if result.skipped.len() == 1 { "" } else { "s" }
        );
        for (name, why) in &result.skipped {
            println!("  - {} — {}", name, why);
        }
    }
}

/// S60 / D-PURE1 (E2-M16): evaluate a `pure fn run()` program.
/// D-EVAL1=A: pretty output by default; `--json` for stable machine JSON.
///
/// When `--pure` is given, the entire call graph from `run` is checked for
/// purity violations (E3401 with the full transitive chain, not just the
/// direct callee). This replaces the old hand-rolled `impure_fns` check.
pub(crate) fn run_eval(file: &str, pure_required: bool, mode: OutputMode) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: couldn't read `{}`: {}", file, e);
            exit(ExitCodes::USER_ERROR);
        }
    };

    // Lex + parse (needed for purity walk and for eval).
    let (toks, lex_diags) = jet::Lexer::lex(&src);
    if !lex_diags.is_empty() {
        eprint!(
            "{}",
            jet::render_all_colored(file, &src, &lex_diags, mode.color_stderr())
        );
        exit(ExitCodes::USER_ERROR);
    }
    let prog = match jet::Parser::parse(&toks) {
        Ok(p) => p,
        Err(ds) => {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &ds, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    };

    // Transitive purity check via the real sema root-walker (E3401 with full
    // call chain). Replaces the old hand-rolled top-level `is_pure` scan.
    if pure_required {
        // Build a FuncSig map from the parsed program for purity flags.
        use std::collections::HashMap;
        let mut funcs_sig: HashMap<String, jet::Sema::FuncSig> = HashMap::new();
        let mut ast_funcs: HashMap<String, &jet::AST::Func> = HashMap::new();
        for item in &prog.items {
            if let jet::AST::Item::Func(f) = item {
                funcs_sig.insert(
                    f.name.clone(),
                    jet::Sema::FuncSig {
                        params: f
                            .params
                            .iter()
                            .map(|p| (p.convention.clone(), p.ty.clone()))
                            .collect(),
                        return_type: f.return_type.clone(),
                        is_extern: false,
                        is_unsafe: f.is_unsafe,
                        is_pure: f.is_pure,
                        is_sanitizer: f.is_sanitizer,
                        is_must_use: f.is_must_use,
                        param_info: f
                            .params
                            .iter()
                            .map(|p| (p.name.clone(), p.default.is_some()))
                            .collect(),
                        defaults: f
                            .params
                            .iter()
                            .map(|p| p.default.as_ref().map(|d| *d.clone()))
                            .collect(),
                        param_variadic: f.params.iter().map(|p| p.variadic).collect(),
                        variadic_bounds: f
                            .params
                            .last()
                            .and_then(|p| p.variadic_bound_list.clone()),
                    },
                );
                ast_funcs.insert(f.name.clone(), f);
            }
        }
        let diags = jet::check_pure_program_root("run", &funcs_sig, &ast_funcs);
        if !diags.is_empty() {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &diags, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    }

    // Full sema type-check with CompileMode::Eval — runs all type/ownership
    // checks but relaxes E0122 (run return type) so `pure fn run() -> T`
    // is accepted. This ensures type errors (e.g. `"string" + 5`) surface with
    // their precise diagnostics rather than falling through to E0956.
    {
        let type_diags = jet::check_for_eval(&src, file);
        if !type_diags.is_empty() {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &type_diags, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    }

    // Evaluate via comptime and render. D-EVAL1=A: pretty by default, JSON with --json.
    match jet::eval_pure_program_value(&src, file) {
        Ok(value) => {
            if mode.json {
                println!("{}", value.to_json());
            } else {
                println!("{}", value.render_pretty());
            }
        }
        Err(diags) => {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &diags, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// D-A11YGATE1=B (c134 Phase 6): the a11y lint codes (E2930/E2931). These are
/// always computed during sema (same as any other `Severity::Lint`), but per
/// the ratified decision they only *surface* under `jet lint --a11y` — never
/// as ordinary build/run/emit warnings. `visible_lints` is the one filter
/// every normal compile-flavored command applies before printing `out.lints`.
const A11Y_LINT_CODES: [&str; 2] = ["E2930", "E2931"];

pub(crate) fn visible_lints(
    lints: &[jet::Diagnostics::Diagnostic],
) -> Vec<jet::Diagnostics::Diagnostic> {
    lints
        .iter()
        .filter(|d| !A11Y_LINT_CODES.contains(&d.code))
        .cloned()
        .collect()
}

/// D-TOOL3 (E2-M11): `jet emit --rust` — print the generated Rust source for a
/// Jet file. This is the expert-window view: the hidden Jet→Rust translation
/// without compiling to native. Useful for debugging or learning what codegen
/// produces.
pub(crate) fn run_emit_rust(file: &str, mode: OutputMode) {
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
    match jet::compile_with_path(&src, file) {
        Ok(out) => {
            let lints = visible_lints(&out.lints);
            if !lints.is_empty() {
                eprint!(
                    "{}",
                    jet::render_all_colored(file, &src, &lints, mode.color_stderr())
                );
            }
            print!("{}", out.rust);
        }
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// D-A11YGATE1=B (c134 Phase 6): `jet lint --a11y <file>` — the opt-in
/// surface for accessibility lints (E2930 unlabeled control, E2931 duplicate
/// label). Never runs during `jet build`/`jet run`/`jet check`; exits nonzero
/// when it finds something so a project can gate CI on "zero a11y warnings"
/// without those warnings ever blocking ordinary compilation.
pub(crate) fn run_lint_a11y(file: &str, mode: OutputMode) {
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
    let diags = jet::check_with_path(file);
    let errors: Vec<jet::Diagnostics::Diagnostic> = diags
        .iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .cloned()
        .collect();
    if !errors.is_empty() {
        report_problems(mode, file, &src, &errors);
        exit(ExitCodes::USER_ERROR);
    }
    let a11y_lints: Vec<jet::Diagnostics::Diagnostic> = diags
        .into_iter()
        .filter(|d| A11Y_LINT_CODES.contains(&d.code))
        .collect();
    if a11y_lints.is_empty() {
        if mode.json {
            println!("{}", jet::render_all_json(file, &src, &[]).trim_end());
        } else {
            println!("ok: `{}` has no accessibility problems", file);
        }
        return;
    }
    if mode.json {
        eprint!("{}", jet::render_all_json(file, &src, &a11y_lints));
    } else {
        eprint!(
            "{}",
            jet::render_all_colored(file, &src, &a11y_lints, mode.color_stderr())
        );
        let n = a11y_lints.len();
        eprintln!(
            "\n{} accessibility warning{} found",
            n,
            if n == 1 { "" } else { "s" }
        );
    }
    exit(ExitCodes::USER_ERROR);
}

/// D-TEST1 / D-TOOL5 (E2-M11): `jet bench` — benchmark a Jet program.
/// Builds the program, runs it with warmups and repeated trials, and reports
/// statistically honest output: mean, stddev, and trial count.
pub(crate) fn run_bench(file: &str, mode: OutputMode) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            exit(ExitCodes::USER_ERROR);
        }
    };

    // D-BENCH1: when the file declares `#Bench` blocks, time each region via
    // the bench harness (its generated `main` reports ns/iter + ops/sec).
    // Otherwise fall through to whole-program timing (the original behaviour).
    if jet::has_bench_blocks(file) {
        run_bench_regions(file, &src, mode);
        return;
    }

    let (rust_code, ffi_link, _capabilities) = match jet::compile_with_path(&src, file) {
        Ok(out) => (out.rust, out.ffi, out.capabilities),
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let bin = PathBuf::from("build").join(format!("bench_{}", stem(file)));
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
        // Benchmark build; not content-cached (race-safe via `build`'s temp path).
        None,
    );

    let warmups = 5u32;
    let trials = 20u32;

    // Warmup runs.
    for _ in 0..warmups {
        Command::new(&bin).output().ok();
    }

    // Timed trials.
    let mut times_ms: Vec<f64> = Vec::new();
    for _ in 0..trials {
        let t0 = std::time::Instant::now();
        let status = Command::new(&bin).status();
        let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
        match status {
            Ok(s) if s.success() => times_ms.push(elapsed),
            Ok(_) => {
                eprintln!("bench: program exited with non-zero status during trial");
                exit(ExitCodes::USER_ERROR);
            }
            Err(e) => {
                eprintln!("bench: couldn't run `{}`: {}", bin.display(), e);
                exit(ExitCodes::USER_ERROR);
            }
        }
    }

    let n = times_ms.len() as f64;
    let mean = times_ms.iter().sum::<f64>() / n;
    let variance = times_ms.iter().map(|t| (t - mean).powi(2)).sum::<f64>() / n;
    let stddev = variance.sqrt();
    let stem_name = stem(file);

    if mode.json {
        println!(
            "{{\"name\":\"{}\",\"mean_ms\":{:.3},\"stddev_ms\":{:.3},\"trials\":{},\"warmups\":{}}}",
            stem_name, mean, stddev, trials, warmups
        );
    } else {
        println!(
            "{}  {:.2} ms ±{:.2}  ({} runs, {} warmup)",
            stem_name, mean, stddev, trials, warmups
        );
    }
}

/// D-BENCH1: build and run the per-region bench harness. The harness binary's
/// `main` warms up, auto-scales, times each `#Bench` region, and prints a line
/// per region (ns/iter + ops/sec), so this just compiles, runs it once, and
/// relays its output.
fn run_bench_regions(file: &str, src: &str, mode: OutputMode) {
    let (rust_code, ffi_link) = match jet::compile_benches_with_path(file) {
        Ok(r) => r,
        Err(diags) => {
            report_problems(mode, file, src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let bin = PathBuf::from("build").join(format!("bench_{}", stem(file)));
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
        // Benchmark build; not content-cached (race-safe via `build`'s temp path).
        None,
    );
    let out = Command::new(&bin).output().unwrap_or_else(|e| {
        eprintln!("bench: couldn't run `{}`: {}", bin.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    print!("{}", String::from_utf8_lossy(&out.stdout));
    if !out.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
    }
    if !out.status.success() {
        exit(ExitCodes::USER_ERROR);
    }
}
