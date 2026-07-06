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

/// `jet devtools grammars` — D-HL1 generated lexical base for editor grammars.
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
        Some(other) => {
            eprintln!("error: unknown `devtools` subcommand `{}`", other);
            eprintln!("usage: {} devtools grammars", jet::Syntax::BINARY_NAME);
            exit(ExitCodes::USAGE);
        }
        None => {
            eprintln!("usage: {} devtools grammars", jet::Syntax::BINARY_NAME);
            exit(ExitCodes::USAGE);
        }
    }
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
