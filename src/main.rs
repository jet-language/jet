//! jet CLI: check / build / run / test / new / fmt / lsp +
//!          add / remove / fetch / update / store (M12.1 package manager).
//!
//! The driver owns invariant I2: rustc's voice never reaches the user as
//! if it were their fault. A rustc failure on generated code is reported
//! as an internal compiler error in jet.

use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use jet::diag::ColorChoice;
use jet::exit_codes;

/// How diagnostics should be presented this run, resolved once from flags +
/// environment and threaded through the diagnostic-printing helpers.
#[derive(Clone, Copy)]
struct OutputMode {
    /// Emit machine-readable `--json` diagnostics instead of human text.
    json: bool,
    /// User's `--color` choice (resolved against TTY-ness at print time).
    color: ColorChoice,
}

impl OutputMode {
    /// Should stderr (where human diagnostics go) be colored?
    fn color_stderr(&self) -> bool {
        self.color.resolve(std::io::stderr().is_terminal())
    }

    /// Resolve the color decision against an explicit TTY-ness (e.g. stdout for
    /// commands that print their report to stdout).
    fn color_stderr_for(&self, is_tty: bool) -> bool {
        self.color.resolve(is_tty)
    }

    /// Should OSC 8 hyperlinks be emitted on stderr? Only on a real TTY with
    /// color resolved on — never when piped/redirected/CI (D-DX6), so existing
    /// snapshots stay byte-identical.
    fn hyperlinks_stderr(&self) -> bool {
        std::io::stderr().is_terminal() && self.color_stderr()
    }
}

/// Parse `--color=auto|always|never` from raw argv (last one wins).
fn parse_color(raw: &[String]) -> ColorChoice {
    let mut choice = ColorChoice::Auto;
    for a in raw {
        if let Some(v) = a.strip_prefix("--color=") {
            choice = ColorChoice::parse(v);
        } else if a == "--color" {
            // bare `--color` means always-on, like ripgrep/cargo.
            choice = ColorChoice::Always;
        }
    }
    choice
}


#[derive(Clone, Copy)]
enum BuildProfile {
    /// Default: speed-oriented (`-O`, thin LTO).
    Default,
    /// S15: size-oriented (`opt-level=z`, fat LTO, `panic=abort`).
    Small,
}

fn usage() -> String {
    format!(
        "\
Welcome to {lang}! (v{ver})

usage:
  {bin} check <file.{ext}>          look for problems, build nothing
  {bin} build <file.{ext}>          compile to a native binary in ./build/
  {bin} run   <file.{ext}>          build, then run (or `jet run` inside a project)
  {bin} run   <file.{ext}> a b      extra words become program arguments
  {bin} test  <file|dir>            compile and run top-level test blocks
  {bin} new   <name>                create a new project folder with payload.jet
  {bin} new   <name> --annotated    same, with commented example deps
  {bin} env                         enter the project dev shell (delegates to `jetpack enter`)
  {bin} env   -- cmd                run a command in the project dev shell, then exit
  {bin} fmt   <file.{ext}>          rewrite file to canonical style (S44)
  {bin} fix   <file.{ext}>          apply all auto-fixable diagnostics in place
  {bin} fix   <file.{ext}> --dry-run   show the fixes as a diff, write nothing
  {bin} doctor                      diagnose the toolchain and offer fixes
  {bin} doctor --fix                apply the auto-fixable problems
  {bin} completions <bash|zsh|fish> print a shell completion script
  {bin} man                         print the jet man page (roff)
  {bin} bind  <header.h> --pkg <lib>   generate a C binding cache (S59)
  {bin} lsp                         language server (stdio JSON-RPC)
  {bin} lsp doctor                  health-check the language server
  {bin} lsp --bench                 latency benchmark (CI: must pass in <200ms/round)
  {bin} version                     print compiler version
  {bin} help                        print this help text
  {bin} upgrade                     how to download a newer release

package management (M12.1):
  {bin} add   <dep> --path <dir>    add a path dependency and fetch
  {bin} add   <dep> --git <url> --tag <tag>   add a git dependency
  {bin} remove <dep>                remove a dependency
  {bin} fetch                       download and link all dependencies
  {bin} fetch --locked              verify lock only, no network
  {bin} update                      refresh @latest / branch selectors
  {bin} update <dep>                update one moving selector
  {bin} store verify                re-check all store entry hashes
  {bin} gc                          remove unreferenced store entries

flags:
  --emit-rust                  also print the generated Rust code
  --check                      with fmt: exit 1 if file would change (CI)
  --small                      with build/run: smallest binary (S15)
  --locked                     with fetch: verify only, refuse network
  --verbose, -v                with build: print the bridge steps
  --json                       emit machine-readable diagnostics
  --color=auto|always|never    control color (auto: only on a terminal)
",
        bin = jet::syntax::BINARY_NAME,
        lang = jet::syntax::LANG_NAME,
        ver = env!("CARGO_PKG_VERSION"),
        ext = jet::syntax::FILE_EXT,
    )
}

/// The bare-`jet` greeting (D-DX): friendly, exit 0, shows the three commands
/// that matter, points at help — not a usage error.
fn greeting() -> String {
    let bin = jet::syntax::BINARY_NAME;
    let ext = jet::syntax::FILE_EXT;
    let mut out = format!(
        "Welcome to {lang}! (v{ver})\n\n",
        lang = jet::syntax::LANG_NAME,
        ver = env!("CARGO_PKG_VERSION"),
    );
    out.push_str("the three commands to know:\n");
    for c in jet::cli::COMMANDS.iter().filter(|c| c.headline) {
        out.push_str(&format!(
            "  {bin} {name:<6} {summary}\n",
            bin = bin,
            name = c.name,
            summary = c.summary,
        ));
    }
    out.push_str(&format!(
        "\ntry `{bin} run hello.{ext}`, or `{bin} help` to see everything.\n",
        bin = bin,
        ext = ext,
    ));
    out
}

/// Teach E2101 for an unknown subcommand, with a "did you mean" when one is
/// close (reusing the edit-distance muscle behind S14 teaching errors).
fn unknown_subcommand(cmd: &str) -> ! {
    let bin = jet::syntax::BINARY_NAME;
    eprintln!("Error [E2101]: `{}` isn't a {} command", cmd, bin);
    eprintln!(" Why: the first word after `{}` must be a known command or an installed `{}-<name>` plugin on your PATH", bin, bin);
    match jet::cli::closest_command(cmd) {
        Some(close) => eprintln!(" Fix: did you mean `{} {}`? (run `{} help` for all commands)", bin, close, bin),
        None => eprintln!(" Fix: run `{} help` to see the commands", bin),
    }
    eprintln!("{}", jet::explain::pointer_line("E2101", std::io::stderr().is_terminal()));
    exit(exit_codes::USAGE);
}

/// Validate every `--flag` in argv against the registry. The first unknown flag
/// teaches E2102 (with a suggestion) and exits usage.
fn check_flags(raw: &[String]) {
    let bin = jet::syntax::BINARY_NAME;
    for a in raw {
        if !a.starts_with("--") || a == "--" {
            continue;
        }
        if jet::cli::is_known_flag(a) {
            continue;
        }
        let head = a.split('=').next().unwrap_or(a);
        eprintln!("Error [E2102]: `{}` isn't a flag {} understands", head, bin);
        eprintln!(" Why: each command accepts a fixed set of flags; an unknown one is usually a typo");
        match jet::cli::closest_flag(head) {
            Some(close) => eprintln!(" Fix: did you mean `{}`? (run `{} help` for the flags)", close, bin),
            None => eprintln!(" Fix: drop the flag, or run `{} help` to see the flags", bin),
        }
        eprintln!("{}", jet::explain::pointer_line("E2102", std::io::stderr().is_terminal()));
        exit(exit_codes::USAGE);
    }
}

/// Find an external `jet-<cmd>` executable on PATH (D-DX5).
fn find_external(cmd: &str) -> Option<PathBuf> {
    let exe = format!("{}-{}", jet::syntax::BINARY_NAME, cmd);
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(&exe);
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    if raw.iter().any(|a| a == "--version") {
        run_version();
        return;
    }

    let emit_rust = raw.iter().any(|a| a == "--emit-rust");
    let fmt_check = raw.iter().any(|a| a == "--check");
    let small = raw.iter().any(|a| a == "--small");
    let locked = raw.iter().any(|a| a == "--locked");
    let annotated = raw.iter().any(|a| a == "--annotated");
    let verbose = raw.iter().any(|a| a == "--verbose" || a == "-v");
    let json = raw.iter().any(|a| a == "--json");
    let mode = OutputMode {
        json,
        color: parse_color(&raw),
    };
    let args: Vec<&String> = raw.iter().filter(|a| !a.starts_with("--")).collect();

    if args.first().map(|s| s.as_str()) == Some("lsp") {
        let sub = args.get(1).map(|s| s.as_str());
        let bench_flag = raw.iter().any(|a| a == "--bench");
        match (sub, bench_flag) {
            (Some("doctor"), _) => {
                jet::lsp::run_doctor();
                return;
            }
            (_, true) | (Some("--bench"), _) => {
                // jet lsp --bench: run latency benchmark on a small program
                let src = include_str!("../examples/features/16_wordcount.jet");
                jet::lsp::run_bench(src, 10, 200);
                return;
            }
            _ => {}
        }
        if let Err(e) = jet::lsp::run_stdio() {
            eprintln!("error: language server failed: {}", e);
            exit(exit_codes::USER_ERROR);
        }
        return;
    }

    let cmd = match args.first() {
        Some(c) => c.as_str(),
        None => {
            // No-args: a friendly greeting that orients, NOT a usage error.
            print!("{}", greeting());
            exit(exit_codes::OK);
        }
    };

    // If the first word is neither a built-in nor a recognized package/pkg
    // command, try an external `jet-<cmd>` on PATH (D-DX5, cargo/git style),
    // else teach E2101 with a "did you mean".
    let known = jet::cli::is_builtin(cmd)
        || matches!(cmd, "lsp" | "install" | "doctor" | "completions" | "man" | "dev");
    if !known {
        if let Some(bin) = find_external(cmd) {
            // Forward every argument after the subcommand name verbatim.
            let fwd: Vec<&String> = raw
                .iter()
                .skip_while(|a| a.as_str() != cmd)
                .skip(1)
                .collect();
            let status = Command::new(&bin)
                .args(fwd.iter().map(|s| s.as_str()))
                .status()
                .unwrap_or_else(|e| {
                    eprintln!("error: couldn't run `{}`: {}", bin.display(), e);
                    exit(exit_codes::USER_ERROR);
                });
            exit(status.code().unwrap_or(exit_codes::OK));
        }
        unknown_subcommand(cmd);
    }

    // Validate flags against the registry; an unknown/half-typed flag is E2102.
    // Skipped for commands that own a bespoke flag vocabulary or forward flags
    // downstream (so their flags aren't measured against the global set).
    let owns_flags = matches!(
        cmd,
        "env" | "dev" | "add" | "remove" | "bind" | "lsp" | "store" | "update" | "fetch"
    );
    if !owns_flags {
        check_flags(&raw);
    }

    // Commands with no required positional target.
    match cmd {
        "help" => {
            print!("{}", usage());
            exit(exit_codes::OK);
        }
        "doctor" => {
            let online = raw.iter().any(|a| a == "--online");
            let apply = raw.iter().any(|a| a == "--fix");
            run_doctor(online, apply, mode);
            return;
        }
        "completions" => {
            run_completions(args.get(1).map(|s| s.as_str()));
            return;
        }
        "man" => {
            print!("{}", jet::cli::man_page(env!("CARGO_PKG_VERSION")));
            return;
        }
        "version" => {
            run_version();
            return;
        }
        "upgrade" => {
            run_upgrade();
            return;
        }
        "explain" => {
            let code = args.get(1).map(|s| s.as_str());
            run_explain(code, mode);
            return;
        }
        "fetch" => {
            run_fetch(locked);
            return;
        }
        "update" => {
            let dep = args.get(1).map(|s| s.as_str());
            run_update(dep);
            return;
        }
        "gc" => {
            run_gc();
            return;
        }
        "bind" => {
            // S59 / E2-M14 Phase 4 (D-CBIND2): generate (or refresh) a C binding
            // cache from a header. Shares the bind backend with compile-time
            // auto-bind. The backend (D-CBIND3 bindgen helper) is not wired in
            // this build, so this reports E3208 honestly rather than faking a
            // translation.
            // Use the unfiltered argv: `bind` takes `--pkg`/`-o` flags that the
            // global `args` filter would otherwise strip.
            let bind_args: Vec<&String> = raw.iter().skip(1).collect();
            run_bind(&bind_args);
            return;
        }
        "env" => {
            // Scale-2 front door (U §8, D-DEV4): `jet env` delegates straight to
            // `jetpack enter`, forwarding flags and any trailing `-- cmd`.
            let mut fwd = raw.clone();
            if let Some(pos) = fwd.iter().position(|a| a == "env") {
                fwd.remove(pos);
            }
            fwd.insert(0, "enter".to_string());
            exit(jet::jetpack::run(fwd));
        }
        "dev" => {
            // E2-M4 (D-DEV4): the watch/interpret loop. Re-check and re-run the
            // entry file on every save, streaming output, for sub-200ms
            // feedback. The interpreter is a dev convenience only — `jet build`/
            // `jet run` never touch it (I2/I3).
            let try_anyway = raw.iter().any(|a| a == "--try-anyway");
            let file = match args.get(1) {
                Some(f) => f.as_str(),
                None => {
                    eprintln!(
                        "error: `jet dev` needs a file to watch: {} dev <file.{}>",
                        jet::syntax::BINARY_NAME,
                        jet::syntax::FILE_EXT
                    );
                    exit(exit_codes::USAGE);
                }
            };
            run_dev(file, try_anyway, mode);
            return;
        }
        "store" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "verify" => run_store_verify(),
                _ => {
                    eprintln!("error: unknown store subcommand `{}`", sub);
                    eprintln!(" fix: try `jet store verify`");
                    exit(exit_codes::USAGE);
                }
            }
            return;
        }
        _ => {}
    }

    let target = match args.get(1) {
        Some(f) => f.as_str(),
        None => {
            // No target: try project-root mode for run/build/test.
            match cmd {
                "run" | "build" | "test" | "check" => {
                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    if let Some(root) = jet::loader::find_manifest_root(&cwd) {
                        let entry = find_project_entry(&root);
                        let entry_str = entry.to_string_lossy().to_string();
                        match cmd {
                            "test" => {
                                run_test(&entry_str, mode);
                                return;
                            }
                            _ => {
                                let program_args: Vec<&String> =
                                    args.iter().skip(1).copied().collect();
                                run_compile_cmd(
                                    cmd,
                                    &entry_str,
                                    emit_rust,
                                    small,
                                    verbose,
                                    &program_args,
                                    mode,
                                );
                                return;
                            }
                        }
                    }
                    eprintln!(
                        "error: no file given and no `payload.jet` found in this directory or above"
                    );
                    eprintln!(
                        " fix: run `jet {} <file.{}>` or cd into a project",
                        cmd,
                        jet::syntax::FILE_EXT
                    );
                    exit(exit_codes::USAGE);
                }
                _ => {
                    eprint!("{}", usage());
                    exit(exit_codes::USAGE);
                }
            }
        }
    };

    match cmd {
        "fmt" => run_fmt(target, fmt_check),
        "fix" => run_fix(target, raw.iter().any(|a| a == "--dry-run")),
        "new" => run_new(target, annotated),
        "test" => run_test(target, mode),
        "add" => run_add(&raw),
        "remove" => run_remove(target),
        // Teaching error: E0042 foreign manifest filename, E0043 `jet install`
        "install" => {
            eprintln!("Error [E0043]: `jet install` isn't a Jet command");
            eprintln!(" Why: Jet uses `jet fetch` to download and link dependencies");
            eprintln!(" Fix: run `jet fetch` to install all dependencies listed in payload.jet");
            exit(exit_codes::USER_ERROR);
        }
        _ => {
            let program_args: Vec<&String> = args.iter().skip(2).copied().collect();
            run_compile_cmd(cmd, target, emit_rust, small, verbose, &program_args, mode);
        }
    }
}

/// Find the entry .jet file for a project (`.jet/main.jet` if exists, else `main.jet`).
fn find_project_entry(root: &Path) -> PathBuf {
    let dot_jet = root
        .join(".jet")
        .join(format!("main.{}", jet::syntax::FILE_EXT));
    if dot_jet.is_file() {
        return dot_jet;
    }
    root.join(format!("main.{}", jet::syntax::FILE_EXT))
}

fn run_version() {
    print!("{}", jet::manifest::version_banner());
}

fn run_upgrade() {
    println!(
        "To upgrade {}, download the latest release from:",
        jet::syntax::BINARY_NAME
    );
    println!("  https://github.com/jet-lang/jet/releases");
}

fn run_compile_cmd(
    cmd: &str,
    file: &str,
    emit_rust: bool,
    small: bool,
    verbose: bool,
    program_args: &[&String],
    mode: OutputMode,
) {
    let profile = if small {
        BuildProfile::Small
    } else {
        BuildProfile::Default
    };

    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            eprintln!(
                " fix: check the spelling, or run {} from the folder that contains it",
                jet::syntax::BINARY_NAME
            );
            exit(exit_codes::USER_ERROR);
        }
    };

    if cmd == "check" {
        let diags: Vec<_> = jet::check_with_path(file)
            .into_iter()
            .filter(|d| matches!(d.severity, jet::diag::Severity::Error))
            .collect();
        if !diags.is_empty() {
            report_problems(mode, file, &src, &diags);
            exit(exit_codes::USER_ERROR);
        }
        if mode.json {
            println!("{}", jet::render_all_json(file, &src, &[]).trim_end());
        } else {
            println!("ok: `{}` has no problems", file);
        }
        return;
    }

    let (rust_code, ffi_link, clinks) = match jet::compile_with_path(&src, file) {
        Ok(out) => {
            if !out.lints.is_empty() {
                if mode.json {
                    eprint!("{}", jet::render_all_json(file, &src, &out.lints));
                } else {
                    eprint!(
                        "{}",
                        jet::render_all_colored(file, &src, &out.lints, mode.color_stderr())
                    );
                    let n = out.lints.len();
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
                    exit(exit_codes::USER_ERROR);
                }
            };
            (out.rust, out.ffi, clinks)
        }
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(exit_codes::USER_ERROR);
        }
    };

    if emit_rust {
        print!("{}", rust_code);
    }

    match cmd {
        "build" => {
            build(file, &rust_code, bin_path(file), profile, ffi_link.as_ref(), &clinks, verbose);
            println!("built: {}", bin_path(file).display());
        }
        "run" => {
            let out = bin_path(file);
            build(file, &rust_code, out.clone(), profile, ffi_link.as_ref(), &clinks, verbose);
            let mut run_cmd = Command::new(&out);
            for arg in program_args {
                run_cmd.arg(arg.as_str());
            }
            let status = run_cmd.status().unwrap_or_else(|e| {
                eprintln!("error: couldn't run the built program: {}", e);
                exit(exit_codes::USER_ERROR);
            });
            exit(status.code().unwrap_or(exit_codes::OK));
        }
        other => {
            eprintln!(
                "error: `{}` isn't a {} command",
                other,
                jet::syntax::BINARY_NAME
            );
            eprint!("{}", usage());
            exit(exit_codes::USAGE);
        }
    }
}

/// `jet dev <file>` — the E2-M4 watch/interpret loop (D-DEV4). Re-checks and
/// re-runs the entry file on every save, streaming output. The per-iteration
/// work lives in `jet::interp::dev_iteration` (so it can be golden-tested);
/// this is the thin std-only watcher around it (I6: no `notify` crate — we
/// poll the file's mtime in a loop).
fn run_dev(file: &str, try_anyway: bool, mode: OutputMode) {
    let path = Path::new(file);
    if !path.exists() {
        eprintln!("error: can't find the file `{}`", file);
        eprintln!(
            " fix: check the spelling, or run {} from the folder that contains it",
            jet::syntax::BINARY_NAME
        );
        exit(exit_codes::USER_ERROR);
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
    let outcome = jet::interp::dev_iteration(file, try_anyway);
    let elapsed = started.elapsed();
    match outcome {
        jet::interp::RunOutcome::Ran { stdout, stderr } => {
            print!("{}", stdout);
            if !stderr.is_empty() {
                eprint!("{}", stderr);
            }
            println!("✓ ran in {} ms", elapsed.as_millis());
        }
        jet::interp::RunOutcome::Problems(diags) => {
            let src = fs::read_to_string(file).unwrap_or_default();
            report_problems(mode, file, &src, &diags);
        }
    }
}

/// Print front-end problems in the active output mode, with the trailing
/// "N problems found" count and (in human mode) one quiet `jet explain`
/// pointer naming the first code. Suppressed entirely in `--json`, where the
/// code is already structured.
fn report_problems(mode: OutputMode, file: &str, src: &str, diags: &[jet::diag::Diagnostic]) {
    if mode.json {
        eprint!("{}", jet::render_all_json(file, src, diags));
        return;
    }
    eprint!(
        "{}",
        jet::render_all_linked(file, src, diags, mode.color_stderr(), mode.hyperlinks_stderr())
    );
    let n = diags.len();
    eprintln!("\n{} problem{} found", n, if n == 1 { "" } else { "s" });
    if let Some(first) = diags.first() {
        eprintln!(
            "{}",
            jet::explain::pointer_line(first.code, mode.color_stderr())
        );
    }
}

/// `jet completions <shell>` — print a shell completion script (D-DX4).
fn run_completions(shell: Option<&str>) {
    let out = match shell {
        Some("bash") => jet::cli::completions_bash(),
        Some("zsh") => jet::cli::completions_zsh(),
        Some("fish") => jet::cli::completions_fish(),
        other => {
            eprintln!(
                "error: completions need a shell: {} completions <bash|zsh|fish>",
                jet::syntax::BINARY_NAME
            );
            if let Some(s) = other {
                eprintln!(" why: `{}` isn't a shell I generate completions for", s);
            }
            exit(exit_codes::USAGE);
        }
    };
    print!("{}", out);
}

/// `jet doctor` — environment self-diagnosis with actionable fixes (D-DX2,
/// D-BUILD1). Offline by default; `--online` enables network checks; `--fix`
/// applies the auto-fixable problems. The advisory code for rustc/cache/PATH
/// problems is L2101.
fn run_doctor(online: bool, apply: bool, mode: OutputMode) {
    let checks = jet::doctor::run(jet::doctor::Options { online });
    let color = mode.color_stderr_for(std::io::stdout().is_terminal());

    if apply {
        let fixed = jet::doctor::apply_fixes(&checks);
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

    use jet::doctor::Health;
    let bold = |s: &str| if color { format!("\x1b[1m{}\x1b[0m", s) } else { s.to_string() };
    let green = |s: &str| if color { format!("\x1b[32m{}\x1b[0m", s) } else { s.to_string() };
    let yellow = |s: &str| if color { format!("\x1b[33m{}\x1b[0m", s) } else { s.to_string() };

    println!("{}", bold(&format!("{} doctor", jet::syntax::BINARY_NAME)));
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
                println!("        (auto-fixable: run `{} doctor --fix`)", jet::syntax::BINARY_NAME);
            }
        }
    }
    println!();
    if jet::doctor::has_problem(&checks) {
        // Advisory L2101 pointer, without making the report noisy.
        println!(
            "some checks need attention. {}",
            jet::explain::pointer_line("L2101", color)
        );
        exit(exit_codes::USER_ERROR);
    } else {
        println!("everything looks good.");
    }
}

/// `jet explain <CODE>` — print the offline essay for a diagnostic code.
fn run_explain(code: Option<&str>, mode: OutputMode) {
    let code = match code {
        Some(c) => c,
        None => {
            eprintln!(
                "usage: {} explain <CODE>   (e.g. {} explain E0102)",
                jet::syntax::BINARY_NAME,
                jet::syntax::BINARY_NAME
            );
            exit(exit_codes::USAGE);
        }
    };
    match jet::explain::lookup(code) {
        Some(ex) => {
            let color = ColorChoice::resolve(mode.color, std::io::stdout().is_terminal());
            print!("{}", jet::explain::render(&ex, color));
        }
        None => {
            eprintln!("error: no diagnostic code `{}` exists", code);
            eprintln!(
                " fix: run a command that reports an error to see its code, e.g. `{} check file.{}`",
                jet::syntax::BINARY_NAME,
                jet::syntax::FILE_EXT
            );
            exit(exit_codes::USER_ERROR);
        }
    }
}

/// Apply all auto-fixable diagnostics in a source file in place (D-LSP7 / M13).
/// Goes through `jet::lsp::collect_fixes` / `apply_all` — the SAME unified fix
/// engine the LSP code-action layer uses — so a fix on the command line and a
/// fix in the editor are byte-identical. `--dry-run` shows the diff without
/// writing.
fn run_fix(file: &str, dry_run: bool) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            eprintln!(" fix: check the spelling");
            exit(exit_codes::USER_ERROR);
        }
    };
    let fixes = jet::lsp::collect_fixes(file, &src);
    if fixes.is_empty() {
        println!("{}: no auto-fixable problems found", file);
        return;
    }
    let fixed = jet::lsp::apply_all(&src, &fixes);
    if fixed == src {
        println!("{}: no changes made", file);
        return;
    }
    let n = fixes.len();
    if dry_run {
        print!("{}", jet::fmt::unified_diff(file, &src, &fixed));
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
        exit(exit_codes::USER_ERROR);
    });
    println!("{}: applied {} fix{}", file, n, if n == 1 { "" } else { "es" });
}

fn run_new(name: &str, annotated: bool) {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        eprintln!("error: project name must be a simple folder name");
        eprintln!(" fix: try: {} new my_app", jet::syntax::BINARY_NAME);
        exit(exit_codes::USER_ERROR);
    }
    let dir = Path::new(name);
    if dir.exists() {
        eprintln!("error: `{}` already exists", name);
        exit(exit_codes::USER_ERROR);
    }
    // Create: <name>/payload.jet, <name>/.jet/main.jet, <name>/.gitignore
    let jet_dir = dir.join(".jet");
    fs::create_dir_all(&jet_dir).unwrap_or_else(|e| {
        eprintln!("error: couldn't create `{}`/.jet: {}", name, e);
        exit(exit_codes::USER_ERROR);
    });
    let manifest_text = jet::manifest::new_template(name, annotated);
    fs::write(dir.join(jet::syntax::PAYLOAD_FILE), manifest_text).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    let main_src = "fn main() {\n    print(\"hello, world\");\n}\n";
    fs::write(jet_dir.join("main.jet"), main_src).unwrap_or_else(|e| {
        eprintln!("error: couldn't write .jet/main.jet: {}", e);
        exit(exit_codes::USER_ERROR);
    });
    fs::write(dir.join(".gitignore"), "build/\n.jet-build/\n").unwrap_or_else(|e| {
        eprintln!("error: couldn't write .gitignore: {}", e);
        exit(exit_codes::USER_ERROR);
    });
    println!("created {}/", name);
    println!("  {}", jet::syntax::PAYLOAD_FILE);
    println!("  .jet/main.jet");
    println!("  .gitignore");
    println!("next: cd {} && {} run", name, jet::syntax::BINARY_NAME);
}

// ──────────────────────────────────────────────
// Package management commands (M12.1)
// ──────────────────────────────────────────────

fn run_add(raw_args: &[String]) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `payload.jet` found — run `jet add` inside a project");
        eprintln!(" fix: run `jet new <name>` to create a project first");
        exit(exit_codes::USER_ERROR);
    });

    // Parse: jet add <dep-name> --path <dir> | --git <url> [--tag <t>|--branch <b>|--rev <r>]
    let non_flag: Vec<&String> = raw_args.iter().filter(|a| !a.starts_with("--")).collect();
    let dep_name = match non_flag.get(1) {
        Some(n) => n.as_str(),
        None => {
            eprintln!("error: `jet add` needs a dependency name");
            eprintln!(" fix: try `jet add mylib --path ../mylib`");
            exit(exit_codes::USER_ERROR);
        }
    };

    let path_val = flag_value(raw_args, "--path");
    let git_val = flag_value(raw_args, "--git");
    let tag_val = flag_value(raw_args, "--tag");
    let branch_val = flag_value(raw_args, "--branch");
    let rev_val = flag_value(raw_args, "--rev");

    let spec = if let Some(p) = path_val {
        jet::manifest::DepSpec::Path {
            path: p.to_string(),
        }
    } else if let Some(url) = git_val {
        let selector = if let Some(t) = tag_val {
            jet::manifest::GitSelector::Tag(t.to_string())
        } else if let Some(b) = branch_val {
            jet::manifest::GitSelector::Branch(b.to_string())
        } else if let Some(r) = rev_val {
            jet::manifest::GitSelector::Rev(r.to_string())
        } else {
            eprintln!(
                "error: git dependency `{}` needs one of: --tag, --branch, --rev",
                dep_name
            );
            exit(exit_codes::USER_ERROR);
        };
        jet::manifest::DepSpec::Git {
            url: url.to_string(),
            selector,
        }
    } else {
        eprintln!("error: `jet add {}` needs --path or --git", dep_name);
        eprintln!(
            " fix: try `jet add {} --path ../{}` or `jet add {} --git <url> --tag <tag>`",
            dep_name, dep_name, dep_name
        );
        exit(exit_codes::USER_ERROR);
    };

    // Load the manifest, add the dep, write back.
    let pack_path = root.join(jet::syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    let updated = jet::manifest::add_dependency(&raw, dep_name, &spec);
    fs::write(&pack_path, updated).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    println!("added `{}` to {}", dep_name, jet::syntax::PAYLOAD_FILE);

    // Auto-fetch.
    do_fetch(&root, false);
}

fn run_remove(dep_name: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `payload.jet` found");
        exit(exit_codes::USER_ERROR);
    });

    let pack_path = root.join(jet::syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    let updated = jet::manifest::remove_dependency(&raw, dep_name);
    fs::write(&pack_path, updated).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    println!("removed `{}` from {}", dep_name, jet::syntax::PAYLOAD_FILE);

    // Re-fetch to update lock.
    do_fetch(&root, false);
}

/// `jet bind <header.h> [--pkg <lib>] [-o <out.jet>]` (S59 / E2-M14 Phase 4).
///
/// Generates a `@bindgen module c.<lib>.__bindgen__` cache from a C header,
/// the same backend the compiler invokes on a cache miss. The header→Jet
/// translator (D-CBIND3 bindgen helper) is not built into this binary yet, so
/// this surfaces **E3208** with the workaround (hand-write `@extern module`).
fn run_bind(args: &[&String]) {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        eprintln!("usage: {} bind <header.h> [--pkg <lib>] [-o <out.jet>]", jet::syntax::BINARY_NAME);
        eprintln!();
        eprintln!("Generate a C binding cache from a header (S59). The output is");
        eprintln!("an `@bindgen module c.<lib>.__bindgen__` file, by default written");
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
                eprintln!("usage: {} bind <header.h> [--pkg <lib>] [-o <out.jet>]", jet::syntax::BINARY_NAME);
                exit(exit_codes::USAGE);
            }
        }
    }

    // Link key: --pkg if given, else the header basename (header→lib rule).
    let lib = pkg.unwrap_or_else(|| {
        let base = header.rsplit('/').next().unwrap_or(header);
        base.strip_suffix(".h").unwrap_or(base).to_string()
    });
    let _ = out;

    // The bind backend (header parse + translate) is not wired in this build.
    // Report E3208 honestly with the documented workaround.
    eprintln!("Error [E3208]: Could not generate bindings from `{}`.", header);
    eprintln!(" Why: the header-to-Jet bind backend is not built into this `{}`; the bindgen helper (D-CBIND3) ships in a later milestone.", jet::syntax::BINARY_NAME);
    eprintln!(
        " Fix: hand-write `@extern module c.{} {{ … }}`, or place a generated cache at .jet/bindings/c/{}.{}.",
        lib, lib, jet::syntax::FILE_EXT
    );
    exit(exit_codes::USER_ERROR);
}

fn run_fetch(locked: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `payload.jet` found — run `jet fetch` inside a project");
        exit(exit_codes::USER_ERROR);
    });
    do_fetch(&root, locked);
}

fn run_update(dep: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `payload.jet` found");
        exit(exit_codes::USER_ERROR);
    });

    let pack_path = root.join(jet::syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    let mf = jet::manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprintln!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(exit_codes::USER_ERROR);
    });
    let existing_lock = jet::lock::load(&root);
    let opts = jet::fetch::FetchOptions {
        locked: false,
        update: true,
        update_dep: dep.map(str::to_string),
    };
    match jet::fetch::fetch(&root, &mf, existing_lock.as_ref(), &opts) {
        Ok(_) => {
            if let Some(d) = dep {
                println!("updated `{}`", d);
            } else {
                println!("updated all moving selectors");
            }
        }
        Err(diags) => {
            let src = String::new();
            eprint!("{}", jet::render_diagnostics(jet::syntax::PAYLOAD_FILE, &src, &diags));
            exit(exit_codes::USER_ERROR);
        }
    }
}

fn run_store_verify() {
    let store_dir = jet::store::store_dir();
    let entries = jet::store::list_entries();
    if entries.is_empty() {
        println!("store is empty ({})", store_dir.display());
        return;
    }
    println!("verifying {} store entries...", entries.len());
    // Without lockfile context we can only verify tree hashes against themselves.
    // Full verification requires the lock file; this checks for obvious corruption.
    let mut ok = 0;
    let mut bad = 0;
    for entry in &entries {
        let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let th = jet::sha256::tree_hash(entry);
        if th.starts_with("sha256-") {
            ok += 1;
        } else {
            eprintln!("  bad: {}", name);
            bad += 1;
        }
    }
    println!("{} ok, {} bad", ok, bad);
    if bad > 0 {
        exit(exit_codes::USER_ERROR);
    }
}

fn run_gc() {
    // Without a global registry of in-use locks, we print a stub message.
    // Full gc would walk all .jet/lock files; M12.1 ships the infrastructure.
    let entries = jet::store::list_entries();
    println!(
        "store has {} entries; use `jet store verify` to check hashes",
        entries.len()
    );
    println!("(gc: removing unreferenced entries requires a future registry — coming in M12.2)");
}

fn do_fetch(root: &Path, locked: bool) {
    let pack_path = root.join(jet::syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    let mf = jet::manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprint!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(exit_codes::USER_ERROR);
    });
    let existing_lock = jet::lock::load(root);
    let opts = jet::fetch::FetchOptions {
        locked,
        update: false,
        update_dep: None,
    };
    match jet::fetch::fetch(root, &mf, existing_lock.as_ref(), &opts) {
        Ok(_) => {
            if locked {
                println!("lock verified");
            } else {
                println!("fetched all dependencies");
            }
        }
        Err(diags) => {
            eprint!("{}", jet::render_diagnostics(jet::syntax::PAYLOAD_FILE, &raw, &diags));
            exit(exit_codes::USER_ERROR);
        }
    }
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == flag {
            return iter.next().map(String::as_str);
        }
    }
    None
}

fn run_test(path: &str, mode: OutputMode) {
    let p = Path::new(path);
    if !p.exists() {
        eprintln!("error: can't find `{}`", path);
        exit(exit_codes::USER_ERROR);
    }
    if p.is_dir() {
        let ext = jet::syntax::FILE_EXT;
        let mut files: Vec<PathBuf> = fs::read_dir(p)
            .unwrap_or_else(|e| {
                eprintln!("error: couldn't read `{}`: {}", path, e);
                exit(exit_codes::USER_ERROR);
            })
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|f| f.extension().and_then(|e| e.to_str()) == Some(ext))
            .collect();
        files.sort();
        if files.is_empty() {
            eprintln!("error: no .{} files in `{}`", ext, path);
            exit(exit_codes::USER_ERROR);
        }
        let mut any_fail = false;
        for f in files {
            if !run_test_file(&f, mode) {
                any_fail = true;
            }
        }
        exit(if any_fail {
            exit_codes::USER_ERROR
        } else {
            exit_codes::OK
        });
    }
    exit(if run_test_file(p, mode) {
        exit_codes::OK
    } else {
        exit_codes::USER_ERROR
    });
}

fn run_test_file(path: &Path, mode: OutputMode) -> bool {
    let shown = path.to_string_lossy();
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: couldn't read `{}`: {}", shown, e);
            return false;
        }
    };
    let (rust_code, ffi_link) = match jet::compile_tests_with_path(&src, &shown) {
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
    );
    let out = Command::new(&bin).output().unwrap_or_else(|e| {
        eprintln!("error: couldn't run tests in `{}`: {}", shown, e);
        exit(exit_codes::USER_ERROR);
    });
    print!("{}", String::from_utf8_lossy(&out.stdout));
    if !out.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
    }
    out.status.success()
}

fn run_fmt(file: &str, check_only: bool) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            exit(exit_codes::USER_ERROR);
        }
    };
    let formatted = match jet::format_source(&src) {
        Ok(s) => s,
        Err(diags) => {
            eprint!("{}", jet::render_diagnostics(file, &src, &diags));
            exit(exit_codes::USER_ERROR);
        }
    };
    if formatted == src {
        return;
    }
    if check_only {
        print!("{}", jet::fmt::unified_diff(file, &src, &formatted));
        exit(exit_codes::USER_ERROR);
    }
    fs::write(file, &formatted).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", file, e);
        exit(exit_codes::USER_ERROR);
    });
}

fn stem(file: &str) -> String {
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

fn build(
    file: &str,
    rust_code: &str,
    bin: PathBuf,
    profile: BuildProfile,
    ffi: Option<&jet::ffi::FfiLink>,
    clinks: &[String],
    verbose: bool,
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
        exit(exit_codes::USER_ERROR);
    });
    let rs_path = PathBuf::from("build").join(format!("{}.rs", stem(file)));
    step(format!("emit Rust  -> {}", rs_path.display()));
    fs::write(&rs_path, rust_code).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", rs_path.display(), e);
        exit(exit_codes::USER_ERROR);
    });

    let small = matches!(profile, BuildProfile::Small);
    // C-linked builds depend on system/hangar link paths not captured by the
    // source hash, so they bypass the binary cache (S59).
    let use_cache = ffi.is_none() && clinks.is_empty();
    let cache_key = if use_cache {
        Some(jet::build_cache::cache_key(rust_code, small))
    } else {
        None
    };
    if let Some(ref key) = cache_key {
        if jet::build_cache::try_copy_cached(key, &bin) {
            step("cache hit -> reused cached binary".to_string());
            return;
        }
    }
    if verbose {
        if cache_key.is_some() {
            step("cache miss -> compiling".to_string());
        } else {
            step("cache bypassed (C-linked build)".to_string());
        }
    }

    step(format!("rustc      {} -> {}", rs_path.display(), bin.display()));
    let mut cmd = Command::new("rustc");
    cmd.arg("--edition").arg("2021");
    match profile {
        BuildProfile::Default => {
            cmd.arg("-O").arg("-C").arg("strip=symbols");
            // FFI rlibs come from a separate cargo build without LTO bitcode.
            if ffi.is_none() {
                cmd.arg("-C").arg("lto=thin");
            }
        }
        BuildProfile::Small => {
            cmd.arg("-C")
                .arg("opt-level=z")
                .arg("-C")
                .arg("panic=abort")
                .arg("-C")
                .arg("strip=symbols");
            if ffi.is_none() {
                cmd.arg("-C").arg("lto=fat");
            }
        }
    }
    cmd.arg(&rs_path).arg("-o").arg(&bin);
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
            exit(exit_codes::USER_ERROR);
        }
    };

    if !out.status.success() {
        eprintln!("internal compiler error: the generated Rust did not compile.");
        eprintln!(
            "This is a bug in {}, NOT in your program. Please report it,",
            jet::syntax::BINARY_NAME
        );
        eprintln!("attaching your source file and the generated file below.");
        eprintln!("  generated: {}", rs_path.display());
        eprintln!("--- rustc said ---");
        eprintln!("{}", String::from_utf8_lossy(&out.stderr));
        exit(exit_codes::ICE);
    }

    step(format!("link       -> {}", bin.display()));

    if let Some(key) = cache_key {
        jet::build_cache::store_cached(&key, &bin);
        step("cache store -> saved binary for next time".to_string());
    }
}
