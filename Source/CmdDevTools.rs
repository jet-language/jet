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

/// `jet dev <file>` — the E2-M4 watch/interpret loop (D-DEV4). Re-checks and
/// re-runs the entry file on every save, streaming output. The per-iteration
/// work lives in `jet::Interpreter::dev_iteration` (so it can be golden-tested);
/// this is the thin std-only watcher around it (I6: no `notify` crate — we
/// poll the file's mtime in a loop).
pub(crate) fn run_dev(file: &str, try_anyway: bool, mode: OutputMode) {
    let path = Path::new(file);
    if !path.exists() {
        eprintln!("error: can't find the file `{}`", file);
        eprintln!(
            " fix: check the spelling, or run {} from the folder that contains it",
            jet::Syntax::BINARY_NAME
        );
        exit(ExitCodes::USER_ERROR);
    }

    println!("watching {} … (Ctrl-C to stop)", file);

    // Run once immediately, then re-run whenever the file's mtime changes.
    let mut last_mtime = file_mtime(path);
    render_dev_iteration(file, try_anyway, mode);

    loop {
        std::thread::sleep(std::time::Duration::from_millis(120));
        let now = file_mtime(path);
        if now != last_mtime {
            last_mtime = now;
            // A debounce sleep lets editors finish writing before we read.
            std::thread::sleep(std::time::Duration::from_millis(30));
            println!("\n— {} changed, re-running —", file);
            render_dev_iteration(file, try_anyway, mode);
        }
    }
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
fn file_mtime(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Run one dev iteration and render its outcome to the terminal in the active
/// output mode. Diagnostics use the SAME renderer as batch compilation
/// (D-DEV), so a problem looks identical whether seen via `jet check` or
/// `jet dev`.
fn render_dev_iteration(file: &str, try_anyway: bool, mode: OutputMode) {
    let started = std::time::Instant::now();
    let outcome = jet::Interpreter::dev_iteration(file, try_anyway);
    let elapsed = started.elapsed();
    match outcome {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr } => {
            print!("{}", stdout);
            if !stderr.is_empty() {
                eprint!("{}", stderr);
            }
            println!("✓ ran in {} ms", elapsed.as_millis());
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

/// `jet doctor` — environment self-diagnosis with actionable fixes (D-DX2,
/// D-BUILD1). Offline by default; `--online` enables network checks; `--fix`
/// applies the auto-fixable problems. The advisory code for rustc/cache/PATH
/// problems is L2101.
pub(crate) fn run_doctor(online: bool, apply: bool, mode: OutputMode) {
    // E2-M15: `jet doctor --target=<triple>` checks cross-compilation readiness.
    let cross_target = std::env::args()
        .find_map(|a| a.strip_prefix("--target=").map(str::to_string));
    let checks = jet::Doctor::run(jet::Doctor::Options { online, cross_target });
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
    let bold = |s: &str| if color { format!("\x1b[1m{}\x1b[0m", s) } else { s.to_string() };
    let green = |s: &str| if color { format!("\x1b[32m{}\x1b[0m", s) } else { s.to_string() };
    let yellow = |s: &str| if color { format!("\x1b[33m{}\x1b[0m", s) } else { s.to_string() };

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
                println!("        (auto-fixable: run `{} doctor --fix`)", jet::Syntax::BINARY_NAME);
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
/// Generates a `#bindgen module c.<lib>.__bindgen__` cache from a C header,
/// the same backend the compiler invokes on a cache miss. The header→Jet
/// translator (D-CBIND3 bindgen helper) is not built into this binary yet, so
/// this surfaces **E3208** with the workaround (hand-write `#extern module`).
pub(crate) fn run_bind(args: &[&String]) {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        eprintln!("usage: {} bind <header.h> [--pkg <lib>] [-o <out.jet>]", jet::Syntax::BINARY_NAME);
        eprintln!();
        eprintln!("Generate a C binding cache from a header (S59). The output is");
        eprintln!("a `#bindgen module c.<lib>.__bindgen__` file, by default written");
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
                eprintln!("usage: {} bind <header.h> [--pkg <lib>] [-o <out.jet>]", jet::Syntax::BINARY_NAME);
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
            eprintln!("Error [E3208]: Could not generate bindings from `{}`.", header);
            eprintln!(" Why: the header file could not be read ({}).", e);
            eprintln!(" Fix: check the path, or install the library's dev headers.");
            exit(ExitCodes::USER_ERROR);
        }
    };

    // E2-M14 (owner 2026-06-18, supersedes D-CBIND3=B): native std-only backend.
    let result = match jet::CBind::generate(&header_src, &lib) {
        Ok(r) => r,
        Err(why) => {
            eprintln!("Error [E3208]: Could not generate bindings from `{}`.", header);
            eprintln!(" Why: {}.", why);
            eprintln!(
                " Fix: hand-write `#extern module c.{} {{ … }}` for the symbols you need.",
                lib
            );
            exit(ExitCodes::USER_ERROR);
        }
    };

    // Default cache path follows D-CBIND7: .jet/bindings/c/<lib>.jet.
    let out_path = out.unwrap_or_else(|| {
        format!(".jet/bindings/c/{}.{}", lib, jet::Syntax::FILE_EXT)
    });
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

    println!(
        "bound {} function{} from `{}` → {}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" },
        header,
        out_path
    );
    if !result.skipped.is_empty() {
        println!(
            "skipped {} declaration{} outside the bindable subset (hand-write `#extern` for these):",
            result.skipped.len(),
            if result.skipped.len() == 1 { "" } else { "s" }
        );
        for (name, why) in &result.skipped {
            println!("  - {} — {}", name, why);
        }
    }
}

/// S60 / D-PURE1 (E2-M16): evaluate a `pure fn main()` program.
/// D-EVAL1=A: pretty output by default; `--json` for stable machine JSON.
///
/// When `--pure` is given, the entire call graph from `main` is checked for
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
                funcs_sig.insert(f.name.clone(), jet::Sema::FuncSig {
                    params: f.params.iter().map(|p| (p.convention.clone(), p.ty.clone())).collect(),
                    return_type: f.return_type.clone(),
                    is_view_return: f.is_view_return,
                    is_extern: false,
                    is_unsafe: f.is_unsafe,
                    is_pure: f.is_pure,
                    param_info: f.params.iter().map(|p| (p.name.clone(), p.default.is_some())).collect(),
                    defaults: f.params.iter().map(|p| p.default.as_ref().map(|d| *d.clone())).collect(),
                });
                ast_funcs.insert(f.name.clone(), f);
            }
        }
        let diags = jet::check_pure_program_root("main", &funcs_sig, &ast_funcs);
        if !diags.is_empty() {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &diags, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    }

    // Full sema type-check with CompileMode::Eval — runs all type/ownership
    // checks but relaxes E0122 (main return type) so `pure fn main() -> T`
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
            if !out.lints.is_empty() {
                eprint!(
                    "{}",
                    jet::render_all_colored(file, &src, &out.lints, mode.color_stderr())
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
    let (rust_code, ffi_link, _capabilities) = match jet::compile_with_path(&src, file) {
        Ok(out) => (out.rust, out.ffi, out.capabilities),
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let bin = PathBuf::from("build").join(format!("bench_{}", stem(file)));
    build(file, &rust_code, bin.clone(), BuildProfile::Default, ffi_link.as_ref(), &[], false, None);

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
