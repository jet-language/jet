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

/// Stable CLI exit codes (E2-M3, docs/spec/architecture.md "Exit codes").
/// Scripts depend on these; treat them as a contract.
mod exit_code {
    /// Everything went fine.
    pub const OK: i32 = 0;
    /// The user's program or input had a problem we reported (E-codes, failed
    /// checks, missing files, failed tests).
    pub const USER_ERROR: i32 = 1;
    /// The command line itself was wrong (unknown subcommand/flag, missing
    /// argument). The greeting is NOT this — running `jet` bare is fine.
    pub const USAGE: i32 = 2;
    // 70 — a built program panicked at runtime (S36); produced by the program,
    //      not the driver, and forwarded through by `jet run`.
    // 101 — internal compiler error (I2): rustc rejected generated code. Set in
    //       `build()` when the backend fails.
}

/// Every subcommand the driver dispatches, for `jet` help and the E2101
/// "did you mean" suggester — generated from the single-source command table in
/// `cli_spec` so completions, the man page, and this suggester can never drift.
/// The package-management verbs and the known-but-redirected `install`
/// (E0043 → `jet fetch`) all live in that table, so a typo like `jet fecth`
/// still lands on `fetch`.
fn known_commands() -> Vec<&'static str> {
    jet::cli_spec::command_names()
}

/// Known long flags (with `--`), for the E2102 unknown-flag suggester. Derived
/// from the same `cli_spec` tables (global flags + every per-command flag), so a
/// flag added to a command automatically passes the E2102 gate.
fn known_flags() -> Vec<String> {
    jet::cli_spec::flag_names()
        .iter()
        .map(|f| format!("--{}", f))
        .collect()
}

/// Levenshtein edit distance (same algorithm sema.rs uses for "did you mean").
/// Kept local to the driver because the sema copy is private and the CLI does
/// not render through the `Diagnostic` type.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Closest candidate within edit distance 2 (the diagnostics.md threshold).
fn closest<'a>(word: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let mut best: Option<(&str, usize)> = None;
    for &cand in candidates {
        let d = edit_distance(word, cand);
        if d <= 2 && best.map_or(true, |(_, bd)| d < bd) {
            best = Some((cand, d));
        }
    }
    best.map(|(c, _)| c)
}

/// D-DX5 external subcommand discovery (cargo-style). Given a word that is NOT a
/// built-in, return the path to an executable `jet-<word>` on `PATH`, or `None`.
/// The caller orders this AFTER the built-in check and BEFORE the E2101 typo
/// path, so a genuine typo of a built-in still teaches and only a real binary
/// shadows it.
fn resolve_external(cmd: &str) -> Option<PathBuf> {
    // Reject anything that isn't a plain subcommand word (no path separators,
    // no flags) so `jet ./foo` or `jet --x` can never exec an arbitrary file.
    if cmd.is_empty()
        || cmd.starts_with('-')
        || cmd.contains('/')
        || cmd.contains('\\')
    {
        return None;
    }
    let exe = format!("{}-{}", jet::syntax::BINARY_NAME, cmd);
    find_on_path(&exe)
}

/// Search `PATH` for an executable file named `exe`. std-only (I6); honours the
/// platform separator. Returns the first match.
fn find_on_path(exe: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(exe);
        if is_executable_file(&cand) {
            return Some(cand);
        }
    }
    None
}

/// True if `p` is a regular file the OS would treat as executable. On Unix we
/// check the owner/group/other execute bits; elsewhere existence is enough.
fn is_executable_file(p: &Path) -> bool {
    let meta = match fs::metadata(p) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// When color is allowed (E2-M3 TTY-aware presentation). Precedence:
/// `--color=never`/`NO_COLOR` force off, `--color=always`/`FORCE_COLOR` force
/// on, otherwise color only when the stream is a real terminal. Scripts (piped
/// or in CI) get plain deterministic bytes and never have to parse ANSI.
#[derive(Clone, Copy)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

fn color_choice(raw: &[String]) -> ColorChoice {
    // Last `--color=...` (or bare `--color`) on the line wins.
    let mut choice = None;
    for a in raw {
        if a == "--color" {
            choice = Some(ColorChoice::Auto);
        } else if let Some(v) = a.strip_prefix("--color=") {
            choice = Some(match v {
                "always" => ColorChoice::Always,
                "never" => ColorChoice::Never,
                _ => ColorChoice::Auto,
            });
        }
    }
    choice.unwrap_or(ColorChoice::Auto)
}

fn use_color(choice: ColorChoice, stream_is_tty: bool) -> bool {
    match choice {
        ColorChoice::Never => false,
        ColorChoice::Always => true,
        ColorChoice::Auto => {
            if std::env::var_os("NO_COLOR").is_some() {
                false
            } else if std::env::var_os("FORCE_COLOR").is_some() {
                true
            } else {
                stream_is_tty
            }
        }
    }
}

/// True when the driver may color stderr (where diagnostics and the greeting
/// go). Resolved once from argv + environment + TTY state.
fn stderr_color(raw: &[String]) -> bool {
    use_color(color_choice(raw), std::io::stderr().is_terminal())
}

/// Same as `stderr_color`, re-reading argv so deep command handlers (which do
/// not carry `raw`) can decide whether to attach the dim `jet explain` footer.
/// Honours `--color`, `NO_COLOR`, `FORCE_COLOR`, and the stderr TTY state.
fn stderr_color_now() -> bool {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    stderr_color(&raw)
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
  {bin} dev                         enter the project shell (delegates to `jetpack enter`)
  {bin} dev   -- cmd                run a command in the project shell, then exit
  {bin} fmt   <file.{ext}>          rewrite file to canonical style (S44)
  {bin} fix   <file.{ext}>          apply all auto-fixable diagnostics in place
  {bin} bind  <header.h> --pkg <lib>   generate a C binding cache (S59)
  {bin} lsp                         language server (stdio JSON-RPC)
  {bin} doctor                      diagnose your environment (rustc, cache, PATH, FFI)
  {bin} doctor --fix                same, applying safe auto-fixes
  {bin} completions <shell>         print a completion script (bash|zsh|fish)
  {bin} man [<command>]             print the manual page (roff)
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
",
        bin = jet::syntax::BINARY_NAME,
        lang = jet::syntax::LANG_NAME,
        ver = env!("CARGO_PKG_VERSION"),
        ext = jet::syntax::FILE_EXT,
    )
}

/// E2-M3 examples-first front door: `jet` with no args greets and shows the
/// few commands that matter, then exits 0. Not a usage error — orientation.
fn greeting() -> String {
    format!(
        "\
Welcome to {lang}! (v{ver})

Get started:
  {bin} new   <name>           create a new project
  {bin} run   <file.{ext}>         build and run a file (or a project)
  {bin} check <file.{ext}>         look for problems, build nothing

  {bin} help                   see every command
",
        bin = jet::syntax::BINARY_NAME,
        lang = jet::syntax::LANG_NAME,
        ver = env!("CARGO_PKG_VERSION"),
        ext = jet::syntax::FILE_EXT,
    )
}

/// E2101 — an unknown subcommand, with a "did you mean" when one is close.
/// Renders in the diagnostics.md what/why/fix voice and exits 2 (usage).
fn unknown_subcommand(cmd: &str, color: bool) -> ! {
    let (red, bold, gray, reset) = palette(color);
    let bin = jet::syntax::BINARY_NAME;
    eprintln!(
        "{red}Error{reset} [E2101]: {bold}`{cmd}` isn't a {bin} command.{reset}"
    );
    eprintln!(" Why: every {bin} run starts with a command like `run`, `check`, or `new`.");
    match closest(cmd, &known_commands()) {
        Some(s) => eprintln!(" Fix: did you mean `{bin} {s}`? Run `{bin} help` to see them all."),
        None => eprintln!(" Fix: run `{bin} help` to see every command."),
    }
    let _ = gray;
    exit(exit_code::USAGE);
}

/// E2102 — an unknown (or ambiguous) flag, with a suggestion when one is close.
fn unknown_flag(flag: &str, color: bool) -> ! {
    let (red, bold, gray, reset) = palette(color);
    let bin = jet::syntax::BINARY_NAME;
    let head = flag.split('=').next().unwrap_or(flag);
    eprintln!(
        "{red}Error{reset} [E2102]: {bold}`{flag}` isn't a flag {bin} understands.{reset}"
    );
    eprintln!(" Why: {bin} ignores no flags silently, so a typo can't quietly change a build.");
    let flags = known_flags();
    let flag_refs: Vec<&str> = flags.iter().map(|s| s.as_str()).collect();
    match closest(head, &flag_refs) {
        Some(s) => eprintln!(" Fix: did you mean `{s}`? Run `{bin} help` to see the flags."),
        None => eprintln!(" Fix: run `{bin} help` to see the flags {bin} accepts."),
    }
    let _ = gray;
    exit(exit_code::USAGE);
}

/// Minimal ANSI palette for driver-level diagnostics. Empty strings when color
/// is off, so piped/CI output is plain deterministic bytes (E2-M3).
fn palette(color: bool) -> (&'static str, &'static str, &'static str, &'static str) {
    if color {
        ("\x1b[31m", "\x1b[1m", "\x1b[90m", "\x1b[0m")
    } else {
        ("", "", "", "")
    }
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    // E2-M3: a bare `jet` greets and orients. This is NOT a usage error — it is
    // the front door, so it exits 0. (A genuine usage problem still exits 2.)
    if raw.is_empty() {
        print!("{}", greeting());
        exit(exit_code::OK);
    }

    if raw.iter().any(|a| a == "--version") {
        run_version();
        return;
    }

    // E2102: validate long flags up front so a typo'd flag teaches instead of
    // being silently dropped by the positional filter below. `--color[=...]` is
    // the presentation flag; it is known and consumed here. Commands that own
    // their own flag universe (`dev` → jetpack, `bind`) are exempt: they parse
    // and validate flags themselves further down.
    let first_word = raw
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(|s| s.as_str());
    let forwards_flags = matches!(first_word, Some("dev") | Some("bind"));
    if !forwards_flags {
        let flags = known_flags();
        if let Some(bad) = raw.iter().find(|a| {
            a.starts_with("--")
                && a.as_str() != "--"
                && !flags.iter().any(|k| k == a.split('=').next().unwrap_or(a.as_str()))
        }) {
            unknown_flag(bad, stderr_color(&raw));
        }
    }

    let emit_rust = raw.iter().any(|a| a == "--emit-rust");
    let fmt_check = raw.iter().any(|a| a == "--check");
    let json = raw.iter().any(|a| a == "--json");
    let small = raw.iter().any(|a| a == "--small");
    let locked = raw.iter().any(|a| a == "--locked");
    let annotated = raw.iter().any(|a| a == "--annotated");
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
            exit(1);
        }
        return;
    }

    let cmd = match args.first() {
        Some(c) => c.as_str(),
        None => {
            // Only flags were given (e.g. `jet --color=always`). Greet rather
            // than error — there is no subcommand to be wrong about.
            print!("{}", greeting());
            exit(exit_code::OK);
        }
    };

    // E2101 / D-DX5: a word that isn't a built-in is resolved here, before it is
    // mistaken for a file to compile. `lsp` is dispatched above and never
    // reaches this point. Resolution order (see `resolve_external`):
    //   1. built-in command  → fall through to dispatch below
    //   2. external `jet-<cmd>` on PATH → exec it with the remaining args
    //   3. neither           → E2101 "did you mean" (typos still teach)
    if !known_commands().contains(&cmd) {
        match resolve_external(cmd) {
            Some(prog) => {
                // cargo-style: forward everything after the subcommand word.
                let pos = raw.iter().position(|a| a == cmd).unwrap_or(0);
                let fwd: Vec<&String> = raw.iter().skip(pos + 1).collect();
                let status = Command::new(&prog)
                    .args(fwd.iter().map(|s| s.as_str()))
                    .status()
                    .unwrap_or_else(|e| {
                        eprintln!("error: couldn't run `{}`: {}", prog.display(), e);
                        exit(exit_code::USER_ERROR);
                    });
                exit(status.code().unwrap_or(exit_code::USER_ERROR));
            }
            None => unknown_subcommand(cmd, stderr_color(&raw)),
        }
    }

    // Commands with no required positional target.
    match cmd {
        "help" => {
            eprint!("{}", usage());
            exit(2);
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
            run_explain(code, stderr_color(&raw));
            return;
        }
        "completions" => {
            run_completions(args.get(1).map(|s| s.as_str()), stderr_color(&raw));
            return;
        }
        "man" => {
            // `jet man` (whole surface) or `jet man <subcommand>` (focused page).
            print!("{}", jet::cli_spec::man(args.get(1).map(|s| s.as_str())));
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
        "doctor" => {
            // E2-M3 / D-DX2 / D-BUILD1: environment self-diagnosis. Offline by
            // default; `--online`/`--network` enables the registry probe,
            // `--fix` applies conservative auto-fixes, `--plain` is the
            // deterministic test mode. Color is gated on stdout's TTY since the
            // report goes to stdout (it is a normal command result, not an error).
            let online = raw.iter().any(|a| a == "--online" || a == "--network");
            let do_fix = raw.iter().any(|a| a == "--fix");
            let plain = raw.iter().any(|a| a == "--plain");
            let color = use_color(color_choice(&raw), std::io::stdout().is_terminal());
            let opts = jet::doctor::Options { online, fix: do_fix, plain, color };
            exit(jet::doctor::run(&opts));
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
        "dev" => {
            // Scale-2 front door (U §8): `jet dev` delegates straight to
            // `jetpack enter`, forwarding flags and any trailing `-- cmd`.
            let mut fwd = raw.clone();
            if let Some(pos) = fwd.iter().position(|a| a == "dev") {
                fwd.remove(pos);
            }
            fwd.insert(0, "enter".to_string());
            exit(jet::jetpack::run(fwd));
        }
        "store" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "verify" => run_store_verify(),
                _ => {
                    eprintln!("error: unknown store subcommand `{}`", sub);
                    eprintln!(" fix: try `jet store verify`");
                    exit(2);
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
                                run_test(&entry_str, json);
                                return;
                            }
                            _ => {
                                let program_args: Vec<&String> =
                                    args.iter().skip(1).copied().collect();
                                run_compile_cmd(cmd, &entry_str, emit_rust, small, json, &program_args);
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
                    exit(2);
                }
                _ => {
                    eprint!("{}", usage());
                    exit(2);
                }
            }
        }
    };

    match cmd {
        "fmt" => run_fmt(target, fmt_check),
        "fix" => {
            let write = raw.iter().any(|a| a == "--write" || a == "--apply");
            // The preview goes to stdout (a normal command result), so color is
            // gated on stdout's TTY, matching `doctor`.
            let color = use_color(color_choice(&raw), std::io::stdout().is_terminal());
            run_fix(target, write, color);
        }
        "new" => run_new(target, annotated),
        "test" => run_test(target, json),
        "add" => run_add(&raw),
        "remove" => run_remove(target),
        // Teaching error: E0042 foreign manifest filename, E0043 `jet install`
        "install" => {
            eprintln!("Error [E0043]: `jet install` isn't a Jet command");
            eprintln!(" Why: Jet uses `jet fetch` to download and link dependencies");
            eprintln!(" Fix: run `jet fetch` to install all dependencies listed in payload.jet");
            exit(1);
        }
        _ => {
            let program_args: Vec<&String> = args.iter().skip(2).copied().collect();
            run_compile_cmd(cmd, target, emit_rust, small, json, &program_args);
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
    println!("{}", env!("CARGO_PKG_VERSION"));
}

fn run_upgrade() {
    println!(
        "To upgrade {}, download the latest release from:",
        jet::syntax::BINARY_NAME
    );
    println!("  https://github.com/jet-lang/jet/releases");
}

/// `jet explain <code>` (E2-M3): print an offline essay for a diagnostic code,
/// sourced from docs/spec/diagnostics.md. Unknown/missing codes fail cleanly
/// with the what/why/fix voice and exit 2 (usage) — never a panic.
fn run_explain(code: Option<&str>, color: bool) {
    let bin = jet::syntax::BINARY_NAME;
    let code = match code {
        Some(c) => c,
        None => {
            let (red, bold, _gray, reset) = palette(color);
            eprintln!("{red}Error{reset}: {bold}`{bin} explain` needs a diagnostic code.{reset}");
            eprintln!(" Why: it prints the offline essay for one error or warning code.");
            eprintln!(" Fix: try `{bin} explain E0102` (any code from an error message).");
            exit(exit_code::USAGE);
        }
    };
    match jet::explain::lookup(code) {
        Some(entry) => {
            print!("{}", entry.essay());
        }
        None => {
            let (red, bold, _gray, reset) = palette(color);
            eprintln!("{red}Error{reset}: {bold}`{code}` isn't a diagnostic code {bin} knows.{reset}");
            eprintln!(" Why: every code {bin} reports is one of the `E####`/`L####` codes in its diagnostics reference.");
            // Suggest a close known code, like the subcommand suggester.
            let codes = jet::explain::live_codes();
            let cand: Vec<&str> = codes.iter().map(|s| s.as_str()).collect();
            match closest(&code.to_ascii_uppercase(), &cand) {
                Some(s) => eprintln!(" Fix: did you mean `{bin} explain {s}`?"),
                None => eprintln!(" Fix: copy the `E####` code from an error message and pass it to `{bin} explain`."),
            }
            exit(exit_code::USAGE);
        }
    }
}

/// `jet completions <bash|zsh|fish>` (E2-M3, D-DX4): print a shell completion
/// script generated from the single-source `cli_spec` tables, so it can never
/// drift from the real command/flag surface. An unknown or missing shell teaches
/// the three supported shells and exits 2 — never a panic.
fn run_completions(shell: Option<&str>, color: bool) {
    let bin = jet::syntax::BINARY_NAME;
    let shell = match shell {
        Some(s) => s,
        None => {
            let (red, bold, _gray, reset) = palette(color);
            eprintln!("{red}Error{reset}: {bold}`{bin} completions` needs a shell name.{reset}");
            eprintln!(" Why: the script differs per shell (bash, zsh, fish).");
            eprintln!(" Fix: try `{bin} completions bash` (or zsh, or fish).");
            exit(exit_code::USAGE);
        }
    };
    match jet::cli_spec::completions(shell) {
        Some(script) => print!("{}", script),
        None => {
            let (red, bold, _gray, reset) = palette(color);
            eprintln!("{red}Error{reset}: {bold}`{shell}` isn't a shell {bin} can generate completions for.{reset}");
            eprintln!(" Why: completions are hand-written per shell; only bash, zsh, and fish are supported.");
            eprintln!(" Fix: try `{bin} completions bash`, `{bin} completions zsh`, or `{bin} completions fish`.");
            exit(exit_code::USAGE);
        }
    }
}

fn run_compile_cmd(
    cmd: &str,
    file: &str,
    emit_rust: bool,
    small: bool,
    json: bool,
    program_args: &[&String],
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
            exit(1);
        }
    };

    if cmd == "check" {
        let diags: Vec<_> = jet::check_with_path(file)
            .into_iter()
            .filter(|d| matches!(d.severity, jet::diag::Severity::Error))
            .collect();
        if !diags.is_empty() {
            if json {
                print!("{}", jet::render_diagnostics_json(file, &src, &diags));
            } else {
                eprint!("{}", jet::render_diagnostics_cli(file, &src, &diags, stderr_color_now()));
                let n = diags.len();
                eprintln!("\n{} problem{} found", n, if n == 1 { "" } else { "s" });
            }
            exit(1);
        }
        if !json {
            println!("ok: `{}` has no problems", file);
        }
        return;
    }

    let (rust_code, ffi_link, clinks) = match jet::compile_with_path(&src, file) {
        Ok(out) => {
            if !out.lints.is_empty() {
                if json {
                    print!("{}", jet::render_diagnostics_json(file, &src, &out.lints));
                } else {
                    eprint!("{}", jet::render_diagnostics_cli(file, &src, &out.lints, stderr_color_now()));
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
                    if json {
                        print!("{}", jet::render_diagnostics_json(file, &src, &diags));
                    } else {
                        eprint!("{}", jet::render_diagnostics_cli(file, &src, &diags, stderr_color_now()));
                        let n = diags.len();
                        eprintln!("\n{} problem{} found", n, if n == 1 { "" } else { "s" });
                    }
                    exit(1);
                }
            };
            (out.rust, out.ffi, clinks)
        }
        Err(diags) => {
            if json {
                print!("{}", jet::render_diagnostics_json(file, &src, &diags));
            } else {
                eprint!("{}", jet::render_diagnostics_cli(file, &src, &diags, stderr_color_now()));
                let n = diags.len();
                eprintln!("\n{} problem{} found", n, if n == 1 { "" } else { "s" });
            }
            exit(1);
        }
    };

    if emit_rust {
        print!("{}", rust_code);
    }

    match cmd {
        "build" => {
            build(file, &rust_code, bin_path(file), profile, ffi_link.as_ref(), &clinks);
            println!("built: {}", bin_path(file).display());
        }
        "run" => {
            let out = bin_path(file);
            build(file, &rust_code, out.clone(), profile, ffi_link.as_ref(), &clinks);
            let mut run_cmd = Command::new(&out);
            for arg in program_args {
                run_cmd.arg(arg.as_str());
            }
            let status = run_cmd.status().unwrap_or_else(|e| {
                eprintln!("error: couldn't run the built program: {}", e);
                exit(1);
            });
            exit(status.code().unwrap_or(0));
        }
        other => {
            eprintln!(
                "error: `{}` isn't a {} command",
                other,
                jet::syntax::BINARY_NAME
            );
            eprint!("{}", usage());
            exit(2);
        }
    }
}

/// `jet fix` (E2-M3, D-REL5): the sanctioned code-migration tool. Runs the front
/// end on `file`, collects the machine-applicable `edit`s its diagnostics carry
/// (the S14 teaching autocorrects), and applies them through the unified fix
/// engine (`jet::fixengine`) — the exact same applier the LSP code-action path
/// builds its edits on.
///
/// Safety model: **dry-run by default**. Without `--write`/`--apply` it prints a
/// unified-diff-style preview of what would change and exits 0, touching nothing
/// on disk. With `write`, it writes the rewritten file back. It only ever
/// touches the file it was handed; it never reaches into jetpack manifests.
fn run_fix(file: &str, write: bool, color: bool) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            eprintln!(" fix: check the spelling");
            exit(1);
        }
    };
    let diags = jet::lsp::check_document(file, &src);
    let edits: Vec<_> = diags.iter().filter_map(|d| d.edit.clone()).collect();
    if edits.is_empty() {
        println!("{}: no auto-fixable problems found", file);
        return;
    }
    let fixed = match jet::fixengine::apply_edits(&src, &edits) {
        Ok(f) => f,
        Err(jet::fixengine::ApplyError::Overlap { first, second }) => {
            // Two fixes contend for the same bytes; refuse rather than guess.
            eprintln!(
                "{}: two auto-fixes overlap (bytes {}..{} and {}..{})",
                file, first.0, first.1, second.0, second.1
            );
            eprintln!(" why: applying both would be ambiguous, so `jet fix` won't.");
            eprintln!(" fix: fix one problem by hand, then re-run `jet fix`.");
            exit(1);
        }
    };
    if fixed == src {
        println!("{}: no changes made", file);
        return;
    }

    let n = edits.len();
    let plural = if n == 1 { "" } else { "es" };

    if write {
        fs::write(file, &fixed).unwrap_or_else(|e| {
            eprintln!("error: couldn't write {}: {}", file, e);
            exit(1);
        });
        println!("{}: applied {} fix{}", file, n, plural);
        println!("{} fix{} in 1 file", n, plural);
    } else {
        // Dry-run: show a unified-diff-style preview and change nothing.
        print!("{}", unified_diff(file, &src, &fixed, color));
        println!(
            "{}: {} fix{} available — run `jet fix --write {}` to apply",
            file, n, plural, file
        );
        println!("{} fix{} in 1 file (preview; nothing written)", n, plural);
    }
}

/// Render a minimal unified-diff-style preview of a whole-file rewrite. Every
/// changed line is shown as a `-`/`+` pair; unchanged lines are elided. This is
/// presentation only (the engine already produced `new`), so it is free to be
/// line-granular rather than byte-exact. Color is TTY-gated by the caller.
fn unified_diff(file: &str, old: &str, new: &str, color: bool) -> String {
    let (red, _bold, gray, reset) = palette(color);
    let green = if color { "\x1b[32m" } else { "" };
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut out = String::new();
    out.push_str(&format!("{gray}--- {file}{reset}\n"));
    out.push_str(&format!("{gray}+++ {file} (fixed){reset}\n"));
    let max = old_lines.len().max(new_lines.len());
    for i in 0..max {
        let o = old_lines.get(i).copied();
        let nw = new_lines.get(i).copied();
        if o == nw {
            continue;
        }
        if let Some(o) = o {
            out.push_str(&format!("{red}-{:>4} | {}{reset}\n", i + 1, o));
        }
        if let Some(nw) = nw {
            out.push_str(&format!("{green}+{:>4} | {}{reset}\n", i + 1, nw));
        }
    }
    out
}

fn run_new(name: &str, annotated: bool) {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        eprintln!("error: project name must be a simple folder name");
        eprintln!(" fix: try: {} new my_app", jet::syntax::BINARY_NAME);
        exit(1);
    }
    let dir = Path::new(name);
    if dir.exists() {
        eprintln!("error: `{}` already exists", name);
        exit(1);
    }
    // Create: <name>/payload.jet, <name>/.jet/main.jet, <name>/.gitignore
    let jet_dir = dir.join(".jet");
    fs::create_dir_all(&jet_dir).unwrap_or_else(|e| {
        eprintln!("error: couldn't create `{}`/.jet: {}", name, e);
        exit(1);
    });
    let manifest_text = jet::manifest::new_template(name, annotated);
    fs::write(dir.join(jet::syntax::PAYLOAD_FILE), manifest_text).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(1);
    });
    let main_src = "fn main() {\n    print(\"hello, world\");\n}\n";
    fs::write(jet_dir.join("main.jet"), main_src).unwrap_or_else(|e| {
        eprintln!("error: couldn't write .jet/main.jet: {}", e);
        exit(1);
    });
    fs::write(dir.join(".gitignore"), "build/\n.jet-build/\n").unwrap_or_else(|e| {
        eprintln!("error: couldn't write .gitignore: {}", e);
        exit(1);
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
        exit(1);
    });

    // Parse: jet add <dep-name> --path <dir> | --git <url> [--tag <t>|--branch <b>|--rev <r>]
    let non_flag: Vec<&String> = raw_args.iter().filter(|a| !a.starts_with("--")).collect();
    let dep_name = match non_flag.get(1) {
        Some(n) => n.as_str(),
        None => {
            eprintln!("error: `jet add` needs a dependency name");
            eprintln!(" fix: try `jet add mylib --path ../mylib`");
            exit(1);
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
            exit(1);
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
        exit(1);
    };

    // Load the manifest, add the dep, write back.
    let pack_path = root.join(jet::syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(1);
    });
    let updated = jet::manifest::add_dependency(&raw, dep_name, &spec);
    fs::write(&pack_path, updated).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(1);
    });
    println!("added `{}` to {}", dep_name, jet::syntax::PAYLOAD_FILE);

    // Auto-fetch.
    do_fetch(&root, false);
}

fn run_remove(dep_name: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `payload.jet` found");
        exit(1);
    });

    let pack_path = root.join(jet::syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(1);
    });
    let updated = jet::manifest::remove_dependency(&raw, dep_name);
    fs::write(&pack_path, updated).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(1);
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
                exit(2);
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
    exit(1);
}

fn run_fetch(locked: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `payload.jet` found — run `jet fetch` inside a project");
        exit(1);
    });
    do_fetch(&root, locked);
}

fn run_update(dep: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `payload.jet` found");
        exit(1);
    });

    let pack_path = root.join(jet::syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(1);
    });
    let mf = jet::manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprintln!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(1);
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
            exit(1);
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
        exit(1);
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
        exit(1);
    });
    let mf = jet::manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprint!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(1);
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
            exit(1);
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

fn run_test(path: &str, json: bool) {
    let p = Path::new(path);
    if !p.exists() {
        eprintln!("error: can't find `{}`", path);
        exit(1);
    }
    if p.is_dir() {
        let ext = jet::syntax::FILE_EXT;
        let mut files: Vec<PathBuf> = fs::read_dir(p)
            .unwrap_or_else(|e| {
                eprintln!("error: couldn't read `{}`: {}", path, e);
                exit(1);
            })
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|f| f.extension().and_then(|e| e.to_str()) == Some(ext))
            .collect();
        files.sort();
        if files.is_empty() {
            eprintln!("error: no .{} files in `{}`", ext, path);
            exit(1);
        }
        let mut any_fail = false;
        for f in files {
            if !run_test_file(&f, json) {
                any_fail = true;
            }
        }
        exit(if any_fail { 1 } else { 0 });
    }
    exit(if run_test_file(p, json) { 0 } else { 1 });
}

fn run_test_file(path: &Path, json: bool) -> bool {
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
            if json {
                print!("{}", jet::render_diagnostics_json(&shown, &src, &diags));
            } else {
                eprint!("{}", jet::render_diagnostics(&shown, &src, &diags));
            }
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
    );
    let out = Command::new(&bin).output().unwrap_or_else(|e| {
        eprintln!("error: couldn't run tests in `{}`: {}", shown, e);
        exit(1);
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
            exit(1);
        }
    };
    let formatted = match jet::format_source(&src) {
        Ok(s) => s,
        Err(diags) => {
            eprint!("{}", jet::render_diagnostics(file, &src, &diags));
            exit(1);
        }
    };
    if formatted == src {
        return;
    }
    if check_only {
        print!("{}", jet::fmt::unified_diff(file, &src, &formatted));
        exit(1);
    }
    fs::write(file, &formatted).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", file, e);
        exit(1);
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
) {
    fs::create_dir_all("build").unwrap_or_else(|e| {
        eprintln!("error: couldn't create the build/ folder: {}", e);
        exit(1);
    });
    let rs_path = PathBuf::from("build").join(format!("{}.rs", stem(file)));
    fs::write(&rs_path, rust_code).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", rs_path.display(), e);
        exit(1);
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
            return;
        }
    }

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
            exit(1);
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
        exit(101);
    }

    if let Some(key) = cache_key {
        jet::build_cache::store_cached(&key, &bin);
    }
}
