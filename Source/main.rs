//! jet CLI: check / build / run / test / new / fmt / lsp +
//!          add / remove / fetch / update / hangar (M12.1 package manager).
//!
//! The driver owns invariant I2: rustc's voice never reaches the user as
//! if it were their fault. A rustc failure on generated code is reported
//! as an internal compiler error in jet.

// Source files/modules use PascalCase names (owner decision).
#![allow(non_snake_case)]

use std::collections::BTreeMap;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use jet::Diagnostics::{ColorChoice, Diagnostic};
use jet::ExitCodes;
use jet_foundation::BuildEffect;

mod CmdCodemod;
mod CmdBudget;
mod CmdCompile;
mod CmdDevTools;
mod CmdNotebook;
mod CmdDossier;
mod CmdInspect;
mod CmdExpand;
mod CmdGc;
mod CmdImpact;
mod CmdImport;
mod CmdPerf;
mod CmdPkg;
mod CmdProve;
mod ProveReplay;
mod ProveSolver;
mod CmdReport;
mod CmdRemote;
mod CmdSchema;
mod CmdSemIndex;
mod CmdSupply;
mod CmdUnsafe;
mod CmdGates;
mod CmdStructuralMerge;
mod EngineDispatch;

use CmdCodemod::run_codemod;
use CmdCompile::{
    resolve_named_profile, run_build_query, run_compiler_api, run_compile_cmd, run_debug_native, run_dev_entry, run_dev_web,
    run_fix, run_fmt, run_fuzz, run_new, run_jobs, run_test_opts,
    run_web_app_dev_entry, validate_target, FuzzRunOpts, TestRunOpts,
};
use CmdDevTools::{
    run_bench, run_bind, run_completions, run_dev, run_devtools, run_doctor, run_emit_rust,
    run_eval, run_explain, run_explain_marker, run_explain_web_graph, run_lint_a11y, run_repl, watch_policy_from, WatchPolicy,
    BenchRunOpts,
};
use CmdDossier::{run_dossier, run_module_explain};
use CmdExpand::run_expand;
use CmdInspect::{run_guarantees, run_provenance};
use CmdImpact::run_impact;
use CmdPkg::{
    run_add, run_fetch, run_hangar_generations, run_hangar_rollback, run_hangar_verify,
    run_remove, run_update,
};
use CmdProve::run_prove;
use CmdReport::run_report;
use CmdRemote::run_remote;
use CmdSchema::run_schema;
use CmdSemIndex::run_semindex;
use CmdSupply::{
    run_audit, run_key_backup, run_keygen, run_publish, run_sbom, run_vendor, run_yank,
};
use CmdStructuralMerge::{run_diff, run_merge, structural_help};

/// How diagnostics should be presented this run, resolved once from flags +
/// environment and threaded through the diagnostic-printing helpers.
#[derive(Clone, Copy)]
pub(crate) struct OutputMode {
    /// Emit machine-readable `--json` diagnostics instead of human text.
    pub(crate) json: bool,
    /// User's `--color` choice (resolved against TTY-ness at print time).
    pub(crate) color: ColorChoice,
    /// #1659 criterion 3: `--quiet`/`-q` — suppress non-error status/progress
    /// output (watch banners, hot-swap notices, confirmations). Never
    /// suppresses errors (stderr) or requested data (a command's actual
    /// result, `--json` output).
    pub(crate) quiet: bool,
}

impl OutputMode {
    /// Should stderr (where human diagnostics go) be colored?
    pub(crate) fn color_stderr(&self) -> bool {
        self.color.resolve(std::io::stderr().is_terminal())
    }

    /// Resolve the color decision against an explicit TTY-ness (e.g. stdout for
    /// commands that print their report to stdout).
    pub(crate) fn color_stderr_for(&self, is_tty: bool) -> bool {
        self.color.resolve(is_tty)
    }

    /// Should OSC 8 hyperlinks be emitted on stderr? Only on a real TTY with
    /// color resolved on — never when piped/redirected/CI (D-DX6), so existing
    /// snapshots stay byte-identical.
    fn hyperlinks_stderr(&self) -> bool {
        std::io::stderr().is_terminal() && self.color_stderr()
    }
}

/// Render one spanless driver failure through Jet's shared diagnostic frame.
fn cli_diagnostic_copy(code: &str) -> (&'static str, &'static str) {
    match code {
        "E1202" => (
            "the lockfile must contain the exact dependency graph before a locked or supply-chain command can run",
            "run `jet fetch`, then run the command again",
        ),
        "E1235" => (
            "the registry git operation failed because of network, authentication, or local-clone state",
            "check registry network access and credentials, then run the command again",
        ),
        "E2101" => (
            "Jet command groups accept only commands in their named area",
            "run `jet help` or the command group's `help` route",
        ),
        "E2102" => (
            "Jet ignores no flags silently, so a typo cannot quietly change a command",
            "correct the named flag, or run `jet help` to see the flags",
        ),
        "E2104" => (
            "Jet needs valid command input before it can run this command",
            "correct the named argument or input, then run the command again",
        ),
        "E2105" => (
            "Jet could not complete the named file, tool, or operating-system operation",
            "correct the named problem, then run the command again",
        ),
        "E2941" => (
            "jet prove accepts only its registered proof lenses",
            "use `all`, `refinements`, `effects`, `taint`, `contracts`, `tests`, `budgets`, `replay`, or `solver`",
        ),
        _ => (
            "Jet could not complete this command",
            "correct the named problem, then run the command again",
        ),
    }
}

pub(crate) fn emit_cli_diagnostic(code: &str, what: String) {
    let (why, fix) = cli_diagnostic_copy(code);
    emit_cli_report(code, what, why.to_string(), fix.to_string(), false);
}

pub(crate) fn emit_cli_diagnostic_with_fix(code: &str, what: String, fix: String) {
    let (why, _) = cli_diagnostic_copy(code);
    emit_cli_report(code, what, why.to_string(), fix, false);
}

pub(crate) fn emit_cli_report(
    code: &str,
    what: String,
    why: String,
    fix: String,
    json: bool,
) {
    let diagnostic = jet::Diagnostics::Diagnostic::error(code, what, why, fix, None);
    if json {
        print!("{}", jet::render_all_json("", "", &[diagnostic]));
    } else {
        eprint!("{}", jet::render_all_colored("", "", &[diagnostic], false));
    }
}

macro_rules! cli_error {
    (@fix $code:expr, $what:expr, $fix:expr) => {
        crate::emit_cli_diagnostic_with_fix($code, ($what).to_string(), ($fix).to_string())
    };
    (@full $code:expr, $what:expr, $why:expr, $fix:expr) => {
        crate::emit_cli_report(
            $code,
            ($what).to_string(),
            ($why).to_string(),
            ($fix).to_string(),
            false,
        )
    };
    ($code:expr, $($arg:tt)*) => {
        crate::emit_cli_diagnostic($code, format!($($arg)*))
    };
}
pub(crate) use cli_error;

/// Parse `--color=auto|always|never` from raw argv (last one wins).
fn parse_color(raw: &[String]) -> ColorChoice {
    let mut choice = ColorChoice::Auto;
    for a in raw {
        if let Some(v) = a.strip_prefix("--color=") {
            choice = ColorChoice::parse(v);
        } else if a == "--color" {
            choice = ColorChoice::Always;
        }
    }
    choice
}

/// D-BUILDPROFILE1 (ratified 2026-06-25): the optimization level carried by a
/// named build profile. Three levels mapping directly to rustc opt-level values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptimizeLevel {
    /// `optimize: none` — rustc opt-level=0; fast compile, no optimization.
    None,
    /// `optimize: basic` — rustc `-O` (opt-level=2); the driver default.
    Basic,
    /// `optimize: full` — rustc `-C opt-level=3`; maximum throughput.
    Full,
}

impl OptimizeLevel {
    /// Cache-key tag unique per level so different profiles don't share entries.
    pub(crate) fn cache_tag(self) -> &'static str {
        match self {
            OptimizeLevel::None => "opt:none",
            OptimizeLevel::Basic => "opt:basic",
            OptimizeLevel::Full => "opt:full",
        }
    }
}

/// Convert from the manifest `BuildOptimize` enum to the driver `OptimizeLevel`.
impl From<jet::Package::BuildOptimize> for OptimizeLevel {
    fn from(v: jet::Package::BuildOptimize) -> Self {
        match v {
            jet::Package::BuildOptimize::None => OptimizeLevel::None,
            jet::Package::BuildOptimize::Basic => OptimizeLevel::Basic,
            jet::Package::BuildOptimize::Full => OptimizeLevel::Full,
        }
    }
}

/// D-BUILDPROFILE1: rustc flags and typed setting contributions derived from a
/// profile definition.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ProfileConfig {
    pub optimize: OptimizeLevel,
    pub debug_info: bool,
    pub small: bool,
    pub panic_abort: bool,
    pub settings: BTreeMap<String, String>,
}

impl ProfileConfig {
    pub(crate) fn release() -> Self {
        Self {
            optimize: OptimizeLevel::Full,
            debug_info: false,
            small: false,
            panic_abort: false,
            settings: BTreeMap::new(),
        }
    }

    pub(crate) fn debug() -> Self {
        Self {
            optimize: OptimizeLevel::None,
            debug_info: true,
            small: false,
            panic_abort: false,
            settings: BTreeMap::new(),
        }
    }

    pub(crate) fn ci() -> Self {
        Self {
            optimize: OptimizeLevel::Basic,
            debug_info: true,
            small: false,
            panic_abort: false,
            settings: BTreeMap::new(),
        }
    }

    pub(crate) fn from_def(def: &jet::Package::BuildProfileDef) -> Self {
        use jet::Package::BuildPanic;
        Self {
            optimize: OptimizeLevel::from(def.optimize),
            debug_info: def.debug_info,
            small: def.small,
            panic_abort: matches!(def.panic, Some(BuildPanic::Abort)),
            settings: def.settings.clone(),
        }
    }

    /// Cache-key suffix for user-defined profiles — encodes every flag that
    /// affects the emitted binary.
    pub(crate) fn settings_tag(&self) -> String {
        let mut parts = vec![self.optimize.cache_tag().to_string()];
        if self.debug_info {
            parts.push("dbg".into());
        }
        if self.small {
            parts.push("small".into());
        }
        if self.panic_abort {
            parts.push("panic=abort".into());
        }
        if !self.settings.is_empty() {
            parts.push(format!(
                "settings:{}",
                self.settings
                    .iter()
                    .map(|(key, value)| format!("{}:{key}{}:{value}", key.len(), value.len()))
                    .collect::<Vec<_>>()
                    .join("")
            ));
        }
        parts.join(";")
    }

    pub(crate) fn rustc_args(&self, ffi: bool) -> Vec<String> {
        let mut args = Vec::new();
        if self.small {
            args.extend(
                ["-C", "opt-level=z", "-C", "panic=abort", "-C", "strip=symbols"]
                    .into_iter()
                    .map(str::to_string),
            );
            if !ffi {
                args.extend(["-C".to_string(), "lto=fat".to_string()]);
            }
            return args;
        }
        match self.optimize {
            OptimizeLevel::None => {}
            OptimizeLevel::Basic => {
                args.push("-O".to_string());
            }
            OptimizeLevel::Full => {
                args.extend(["-C".to_string(), "opt-level=3".to_string()]);
            }
        }
        if self.debug_info {
            args.extend(["-C".to_string(), "debuginfo=2".to_string()]);
        } else if !matches!(self.optimize, OptimizeLevel::None) {
            args.extend(["-C".to_string(), "strip=symbols".to_string()]);
        }
        if self.panic_abort {
            args.extend(["-C".to_string(), "panic=abort".to_string()]);
        }
        if !ffi && !matches!(self.optimize, OptimizeLevel::None) {
            args.extend(["-C".to_string(), "lto=thin".to_string()]);
        }
        args
    }
}

#[derive(Clone)]
pub(crate) enum BuildProfile {
    /// Default: speed-oriented (`-O`, thin LTO). No `--profile` flag.
    Default,
    /// D-BUILDPROFILE1: `--release` / `--profile=release`. Full optimization.
    Release,
    /// D-BUILDPROFILE1: `--profile=debug`. No optimization.
    Debug,
    /// D-BUILDPROFILE1: `--profile=ci`. Optimized with debug symbols for CI.
    Ci,
    /// D-BUILDPROFILE1: user-defined or manifest-overridden profile settings.
    Named { name: String, config: ProfileConfig },
    /// S15: size-oriented (`opt-level=z`, fat LTO, `panic=abort`).
    Small,
    /// E2-M15: freestanding / embedded — no OS, only core APIs; `panic=abort`.
    Freestanding,
}

impl BuildProfile {
    /// Ratified performance-budget applicability name. Default builds retain
    /// the existing `dev` profile identity; named profiles use their declared
    /// name, never their cache-settings suffix.
    pub(crate) fn budget_name(&self) -> &str {
        match self {
            BuildProfile::Default => "dev",
            BuildProfile::Release => "release",
            BuildProfile::Debug => "debug",
            BuildProfile::Ci => "ci",
            BuildProfile::Named { name, .. } => name,
            BuildProfile::Small => "small",
            BuildProfile::Freestanding => "freestanding",
        }
    }

    pub(crate) fn cache_tag(&self) -> String {
        match self {
            BuildProfile::Default => "default".to_string(),
            BuildProfile::Release => "release".to_string(),
            BuildProfile::Debug => "debug".to_string(),
            BuildProfile::Ci => "ci".to_string(),
            BuildProfile::Named { name, config } => {
                format!("profile:{name};{}", config.settings_tag())
            }
            BuildProfile::Small => "small".to_string(),
            BuildProfile::Freestanding => "freestanding".to_string(),
        }
    }

    pub(crate) fn config(&self) -> ProfileConfig {
        match self {
            BuildProfile::Default => ProfileConfig {
                optimize: OptimizeLevel::Basic,
                debug_info: false,
                small: false,
                panic_abort: false,
                settings: BTreeMap::new(),
            },
            BuildProfile::Release => ProfileConfig::release(),
            BuildProfile::Debug => ProfileConfig::debug(),
            BuildProfile::Ci => ProfileConfig::ci(),
            BuildProfile::Named { config, .. } => config.clone(),
            BuildProfile::Small => ProfileConfig {
                optimize: OptimizeLevel::Basic,
                debug_info: false,
                small: true,
                panic_abort: true,
                settings: BTreeMap::new(),
            },
            BuildProfile::Freestanding => ProfileConfig {
                optimize: OptimizeLevel::Basic,
                debug_info: false,
                small: true,
                panic_abort: true,
                settings: BTreeMap::new(),
            },
        }
    }
}

pub(crate) fn usage() -> String {
    jet::CLI::usage_page(env!("CARGO_PKG_VERSION"))
}

/// #1659 criterion 2: `jet <cmd> --help`/`-h`. Rendered from the same
/// `jet::CLI` tables and `usage()` text as `jet help` and the man page — not
/// a hand-duplicated per-command help string.
fn command_help(cmd: &str) -> String {
    let bin = jet::Syntax::BINARY_NAME;
    // A bare group name reaches here only for a non-exhaustive front door
    // (`os` or `env`) — exhaustive groups are handled by normalization.
    if let Some(group) = jet::CLI::command_group(cmd) {
        return format!(
            "{bin} {cmd} — {}\n\n{}",
            group.summary,
            jet::CLI::command_group_usage(cmd)
        );
    }
    // A normalized nested-action dispatch word (`publish`, `graph`, …).
    if let Some((group, action)) = jet::CLI::moved_command(cmd) {
        let mut lines = String::new();
        for usage_line in action.usage.lines() {
            lines.push_str(&format!("  {bin} {} {}\n", group.name, usage_line));
        }
        return format!(
            "{bin} {} {cmd} — {}\n\n{lines}",
            group.name, action.summary
        );
    }
    // A canonical flat top-level command: summary, usage, and flags all come
    // from the live registry. No parsing of a second usage string.
    let Some(command) = jet::CLI::COMMANDS.iter().find(|command| command.name == cmd) else {
        return format!("{bin} {cmd}\n\nRun `{bin} help` to see every command.\n");
    };
    let mut output = format!(
        "{bin} {cmd} — {}\n\n  {}\n",
        command.summary,
        jet::CLI::command_usage(cmd),
    );
    for (long, help) in jet::CLI::flags_for_command(cmd) {
        output.push_str(&format!("  {long:<28} {help}\n"));
    }
    output
}

/// True when `arg` names a Jet source file or project directory (c6vz465 sugar:
/// `jet <file>` → `jet run <file>`). Unknown bare stems that do not exist are
/// false so E2101 still fires for typos like `buld`.
fn looks_like_jet_source(arg: &str) -> bool {
    let path = Path::new(arg);
    if path.extension().is_some_and(|e| e == jet::Syntax::FILE_EXT) {
        return true;
    }
    if path.exists() {
        return path.is_file() || path.is_dir();
    }
    Path::new(&format!("{}.{}", arg, jet::Syntax::FILE_EXT)).exists()
}

/// U16 (D-JPK-BRIDGE1=A): `nix run nixpkgs#fastfetch` parity.
///
/// Top-level `jet run tool@nixpkgs` is not a Jet source compile; it is the
/// package-engine path, with the ratified `<package>@<source>` CLI spelling. Lower it
/// to `jetpack run tool@nixpkgs -- tool`, preserving the offline fixture flags
/// used by the same provider path and forwarding user args after `--`.
fn dispatch_nixpkgs_run(raw: &[String], target: &str, sep: Option<usize>) -> Option<i32> {
    let (package, source) = target.rsplit_once(jet::Syntax::REF_PROVIDER_AT)?;
    if source != jet::Syntax::REF_SOURCE_NIXPKGS || package.is_empty() {
        return None;
    }

    let before_sep = sep.map_or(raw, |i| &raw[..i]);
    let mut fwd = vec![
        "run".to_string(),
        target.to_string(),
    ];
    let mut command_args = Vec::new();
    let mut saw_run = false;
    let mut saw_target = false;
    let mut i = 0usize;
    while i < before_sep.len() {
        let a = &before_sep[i];
        if !saw_run && a == "run" {
            saw_run = true;
            i += 1;
            continue;
        }
        if !saw_target && a == target {
            saw_target = true;
            i += 1;
            continue;
        }
        match a.as_str() {
            "--no-color" | "--offline" => fwd.push(a.clone()),
            "--fixtures" => {
                fwd.push(a.clone());
                if let Some(value) = before_sep.get(i + 1) {
                    fwd.push(value.clone());
                    i += 1;
                }
            }
            "--color=never" => fwd.push("--no-color".to_string()),
            s if s.starts_with("--") => {
                eprintln!(
                    "Error [E2102]: `{}` isn't a flag `jet run …@nixpkgs` understands",
                    s
                );
                eprintln!(
                    " Why: this form forwards only package-run flags before `--`; tool arguments go after `--`"
                );
                eprintln!(
                    " Fix: write `jet run {}@nixpkgs -- {}` to pass it to the tool.",
                    package, s
                );
                return Some(ExitCodes::USAGE);
            }
            other => command_args.push(other.to_string()),
        }
        i += 1;
    }

    fwd.push("--".to_string());
    fwd.push(package.to_string());
    fwd.extend(command_args);
    if let Some(i) = sep {
        fwd.extend(raw[i + 1..].iter().cloned());
    }
    Some(EngineDispatch::dispatch(
        jet::Syntax::JETPACK_BINARY_NAME,
        "run",
        &fwd,
    ))
}

/// The bare-`jet` greeting (D-DX): friendly, exit 0, not a usage error.
/// Shown when argv is flags-only with no subcommand (not for a bare `jet` —
/// that starts the REPL per c6vz465).
fn greeting() -> String {
    format!(
        "\
Welcome to {lang}! (v{ver})

Get started:
  {bin} new   <name>           create a new project
  {bin} run   <file.{ext}>     build and run a file (or a project)
  {bin} check <file.{ext}>     look for problems, build nothing

  {bin} help                   see every command
",
        bin = jet::Syntax::BINARY_NAME,
        lang = jet::Syntax::LANG_NAME,
        ver = env!("CARGO_PKG_VERSION"),
        ext = jet::Syntax::FILE_EXT,
    )
}

/// Teach E2101 for an unknown subcommand, with a "did you mean" when one is
/// close (reusing the edit-distance muscle behind S14 teaching errors).
fn unknown_subcommand(cmd: &str) -> ! {
    let bin = jet::Syntax::BINARY_NAME;
    eprintln!("Error [E2101]: `{}` isn't a {} command.", cmd, bin);
    eprintln!(
        " Why: every {} run starts with a command like `run`, `check`, or `new`.",
        bin
    );
    match jet::CLI::closest_command(cmd) {
        Some(close) => eprintln!(
            " Fix: did you mean `{} {}`? Run `{} help` to see them all.",
            bin, close, bin
        ),
        None => eprintln!(" Fix: run `{} help` to see every command.", bin),
    }
    exit(ExitCodes::USAGE);
}

/// D-CLI-SURFACE1=B / D-CLI-SURFACE2=A: grouped spelling is canonical.
/// Normalize only after rejecting the retired top-level spelling, so grouped commands
/// reach the existing real handlers without keeping compatibility aliases.
fn first_cli_positional(raw: &[String]) -> Option<&str> {
    let end = raw.iter().position(|arg| arg == "--").unwrap_or(raw.len());
    let mut skip_next = false;
    for arg in &raw[..end] {
        if skip_next {
            skip_next = false;
            continue;
        }
        if matches!(arg.as_str(), "-p" | "--output" | "--gate" | "--scope" | "--kind" | "--target") {
            skip_next = true;
            continue;
        }
        if arg.starts_with("--output=") || arg.starts_with("--scope=") || arg.starts_with("--kind=") || arg.starts_with("--target=") {
            continue;
        }
        if arg == "-" || !arg.starts_with('-') {
            return Some(arg);
        }
    }
    None
}

fn normalize_frequency_ring_argv(raw: &mut Vec<String>) {
    if let Some(retired) = jet::CLI::retired_command(raw) {
        let category = retired.category;
        let rewrite = retired.rewrite;
        if category == jet::CLI::RetirementCategory::Semantic {
            teach_retired(retired, raw, raw.iter().any(|arg| arg == "--json"));
        }
        let rewrite = rewrite.expect("rename retirement needs a rewrite rule");
        let replacement = (retired.fix)(raw);
        *raw = rewrite(raw);
        eprintln!("Notice: `{}` is now `{}`.", retired.spelling, replacement);
    }
    let Some(first) = raw.first().map(String::as_str) else { return };
    if let Some((group_spec, _)) = jet::CLI::moved_command(first) {
        let group = group_spec.name;
        let verb = first;
        let replacement = format!("jet {group} {}", raw.join(" "));
        emit_cli_report(
            "E2101",
            format!("`{verb}` moved under `jet {group}`."),
            "infrequent commands live in a named area so daily Jet commands stay easy to scan.".to_string(),
            format!("run `{replacement}`."),
            raw.iter().any(|arg| arg == "--json"),
        );
        exit(ExitCodes::USAGE);
    }
    if first_cli_positional(raw) == Some("bind") {
        emit_cli_report(
            "E2101",
            "`bind` moved under `jet inspect`.".to_string(),
            "infrequent commands live in a named area so daily Jet commands stay easy to scan.".to_string(),
            format!("run `{} inspect bind`.", jet::Syntax::BINARY_NAME),
            raw.iter().any(|arg| arg == "--json"),
        );
        exit(ExitCodes::USAGE);
    }
    let Some(group) = raw.first().cloned() else { return };
    // D-CLI-SURFACE3=B: `os` is not exhaustive — jetos's own native verbs
    // (`check`/`build`/`switch`/…, D-JPK-OSVERB1) stay opaque to this
    // registry, so bare `jet os` / `jet os help` fall through unchanged to
    // the real `jet os` dispatcher instead of being hijacked by this
    // group's (partial) action list.
    let exhaustive = jet::CLI::command_group(&group).map(|spec| spec.exhaustive).unwrap_or(false);
    if let Some(spec) = jet::CLI::command_group(&group) {
        // #1659 criterion 2: `--help`/`-h` are real help requests here, not
        // an unmodeled subword — retiring the E2101 that used to fire for
        // e.g. `jet hangar --help`.
        let asks_help = if group == "env" {
            matches!(raw.get(1).map(String::as_str), Some("help"))
                || raw.get(1).is_some_and(|a| jet::CLI::is_help_flag(a))
        } else {
            raw.len() == 1
                || matches!(raw.get(1).map(String::as_str), Some("help"))
                || raw.get(1).is_some_and(|a| jet::CLI::is_help_flag(a))
        };
        if (exhaustive || group == "env") && asks_help {
            println!("jet {group} — {}", spec.summary);
            print!("{}", jet::CLI::command_group_usage(&group));
            exit(ExitCodes::OK);
        }
    }
    let Some(sub) = raw.get(1).cloned() else { return };
    if exhaustive && jet::CLI::nested_command(&group, &sub).is_none() {
        emit_cli_report(
            "E2101",
            format!("`{sub}` isn't a jet {group} command."),
            format!("jet {group} accepts only commands in its named area."),
            format!("run `jet {group} help`."),
            raw.iter().any(|arg| arg == "--json"),
        );
        exit(ExitCodes::USAGE);
    }
    if let Some((_, action)) = jet::CLI::nested_command(&group, &sub) {
        if !action.handler.keeps_group() {
            raw[0] = action.handler.dispatch_word().to_string();
            raw.remove(1);
        }
    }
}

fn esc(s: &str) -> String {
    s.chars().flat_map(|c| match c {
        '"' => "\\\"".chars().collect::<Vec<_>>(),
        '\\' => "\\\\".chars().collect(),
        '\n' => "\\n".chars().collect(),
        '\r' => "\\r".chars().collect(),
        '\t' => "\\t".chars().collect(),
        c if c.is_control() => format!("\\u{:04x}", c as u32).chars().collect(),
        c => vec![c],
    }).collect()
}

fn run_project_parts(raw: &[String], mode: OutputMode) -> ! {
    let skipped_only = raw.iter().any(|arg| arg == "--skipped");
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let report = jet::ProjectParts::scan(&root);
    let parts = report
        .parts
        .iter()
        .filter(|part| !skipped_only || part.state == jet::ProjectParts::ProjectPartState::Skipped)
        .collect::<Vec<_>>();
    let relative = |path: &Path| {
        path.strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    if mode.json {
        let parts = parts
            .iter()
            .map(|part| {
                format!(
                    "{{\"name\":\"{}\",\"path\":\"{}\",\"state\":\"{}\"}}",
                    esc(&part.canonical_name()),
                    esc(&relative(&part.path)),
                    part.state.name()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let conflicts = report
            .conflicts
            .iter()
            .map(|conflict| {
                let paths = conflict
                    .paths
                    .iter()
                    .map(|path| format!("\"{}\"", esc(&relative(path))))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"name\":\"{}\",\"paths\":[{}]}}",
                    esc(&conflict.canonical_name()),
                    paths
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"schema_version\":1,\"parts\":[{}],\"conflicts\":[{}]}}",
            parts, conflicts
        );
    } else {
        for part in parts {
            println!(
                "{:<9} {:<24} {}",
                part.state.name(),
                part.canonical_name(),
                relative(&part.path)
            );
        }
        for conflict in &report.conflicts {
            eprint!(
                "{}",
                jet::render_diagnostics("", "", &[conflict.diagnostic(&root, None)])
            );
        }
    }
    exit(if report.conflicts.is_empty() {
        ExitCodes::OK
    } else {
        ExitCodes::USER_ERROR
    });
}

/// D-ONCE-RETIRE1=C: semantic retirement rows hard-error with their registry
/// owned fix. Pure rename rows are rewritten by the caller before dispatch.
fn teach_retired(spec: &jet::CLI::RetiredCommandSpec, raw: &[String], json: bool) -> ! {
    debug_assert_eq!(spec.category, jet::CLI::RetirementCategory::Semantic);
    emit_cli_report(
        spec.error_code,
        format!("`{}` isn't a jet command", spec.spelling),
        spec.why.to_string(),
        format!("run `{}`", (spec.fix)(raw)),
        json,
    );
    exit(ExitCodes::USAGE);
}

/// Validate every `--flag` in argv against the registry. The first unknown flag
/// teaches E2102 (with a suggestion) and exits usage.
///
/// `subcmd` is the subcommand being run (e.g. `"run"`, `"test"`). When the
/// subcommand can forward args to a program, the Fix line also teaches the `--`
/// separator form (D-CLI1=A).
fn check_flags(raw: &[String], subcmd: &str) {
    let bin = jet::Syntax::BINARY_NAME;
    for a in raw {
        if !a.starts_with("--") || a == "--" {
            continue;
        }
        if jet::CLI::is_known_flag(a) {
            continue;
        }
        let head = a.split('=').next().unwrap_or(a);
        if head == "--emit-rust" {
            eprintln!("Error [E2102]: `--emit-rust` isn't a flag {bin} understands");
            eprintln!(" Why: generated output belongs to the `emit` command");
            eprintln!(" Fix: run `{bin} emit --rust <file.{}>`", jet::Syntax::FILE_EXT);
            exit(ExitCodes::USAGE);
        }
        eprintln!("Error [E2102]: `{}` isn't a flag {} understands", head, bin);
        eprintln!(" Why: flags before `--` belong to {}; everything after `--` is forwarded to your program", bin);
        match jet::CLI::closest_flag(head) {
            Some(close) if matches!(subcmd, "run" | "test") => eprintln!(
                " Fix: did you mean `{}`? Or use `{} {} <file> -- {}` to pass it to your program",
                close, bin, subcmd, head
            ),
            Some(close) => eprintln!(
                " Fix: did you mean `{}`? (run `{} help` for the flags)",
                close, bin
            ),
            None if matches!(subcmd, "run" | "test") => eprintln!(
                " Fix: use `{} {} <file> -- {}` to pass this flag to your program",
                bin, subcmd, head
            ),
            None => eprintln!(
                " Fix: drop the flag, or run `{} help` to see the flags",
                bin
            ),
        }
        exit(ExitCodes::USAGE);
    }
}

fn reject_bench_test_flags(argv: &[String]) {
    let Some(flag) = argv.iter().find(|arg| {
        matches!(
            arg.as_str(),
            "-u" | "--serial" | "--coverage" | "--shuffle" | "--update-snapshots"
        )
            || arg.starts_with("--shuffle=")
            || arg.starts_with("--coverage=")
            || arg.starts_with("--update-snapshots=")
    }) else {
        return;
    };
    crate::cli_error!(
        @fix "E2104",
        format!("`{flag}` applies only to `jet test`"),
        format!("run `jet test <file> {flag}`")
    );
    exit(ExitCodes::USAGE);
}

fn reject_retired_gate_flags(argv: &[String], json: bool) {
    let Some(retirement) = jet::Syntax::retirement("allow-impure") else {
        return;
    };
    if !argv.iter().any(|arg| arg == retirement.retired) {
        return;
    }
    emit_cli_report(
        retirement.code.unwrap_or("E1343"),
        format!("`{}` is retired", retirement.retired),
        "the old boolean changed which audited escapes were checked and is no longer accepted".to_string(),
        format!("use `{}`", retirement.canonical),
        json,
    );
    exit(ExitCodes::USAGE);
}

fn parse_gate_flags(argv: &[String], json: bool) -> jet::Policy::GateSet {
    let mut gates = jet::Policy::GateSet::default();
    let mut index = 0;
    while index < argv.len() {
        let argument = &argv[index];
        let spec = if let Some(spec) = argument.strip_prefix("--gate=") {
            Some(spec.to_string())
        } else if argument == "--gate" {
            match argv.get(index + 1) {
                Some(spec) if !spec.starts_with('-') => {
                    index += 1;
                    Some(spec.clone())
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(spec) = spec {
            match jet::Policy::GateSet::parse(&spec) {
                Ok(key) => gates.insert(key),
                Err(detail) => {
                    emit_cli_report(
                        "E2104",
                        "invalid audited gate".to_string(),
                        detail,
                        "use `--gate unsafe=allow`, `--gate impure=allow`, or `--gate nondeterministic=allow`".to_string(),
                        json,
                    );
                    exit(ExitCodes::USAGE);
                }
            }
        } else if argument == "--gate" {
            emit_cli_report(
                "E2104",
                "`--gate` needs a gate assignment".to_string(),
                "an invocation gate names one audited escape and its allow value".to_string(),
                "use `--gate name=allow`".to_string(),
                json,
            );
            exit(ExitCodes::USAGE);
        }
        index += 1;
    }
    gates
}

fn parse_setting_overrides(argv: &[String], json: bool) -> BTreeMap<String, String> {
    let mut settings = BTreeMap::new();
    let mut index = 0;
    while index < argv.len() {
        let argument = &argv[index];
        let raw = if let Some(value) = argument.strip_prefix("--set=") {
            value.to_string()
        } else if argument == "--set" {
            match argv.get(index + 1) {
                Some(value) => {
                    index += 1;
                    value.clone()
                }
                None => {
                    emit_cli_report(
                        "E2104",
                        "`--set` needs a key=value assignment".to_string(),
                        "a typed package setting override names one declared key and its value".to_string(),
                        "use `--set key=value`".to_string(),
                        json,
                    );
                    exit(ExitCodes::USAGE);
                }
            }
        } else {
            index += 1;
            continue;
        };
        let Some((key, value)) = raw.split_once('=') else {
            emit_cli_report(
                "E2104",
                "`--set` needs a key=value assignment".to_string(),
                "a typed package setting override names one declared key and its value".to_string(),
                "use `--set key=value`".to_string(),
                json,
            );
            exit(ExitCodes::USAGE);
        };
        let key = key.trim();
        if key.is_empty() {
            emit_cli_report(
                "E2104",
                "`--set` needs a non-empty key".to_string(),
                "the compiler must be able to resolve one declared setting before it parses the value".to_string(),
                "use `--set key=value` with a declared key".to_string(),
                json,
            );
            exit(ExitCodes::USAGE);
        }
        if settings.insert(key.to_string(), value.trim().to_string()).is_some() {
            emit_cli_report(
                "E2104",
                format!("setting `{key}` is assigned more than once"),
                "one invocation must contribute one unambiguous value for each declared setting".to_string(),
                "remove the duplicate `--set` assignment".to_string(),
                json,
            );
            exit(ExitCodes::USAGE);
        }
        index += 1;
    }
    settings
}
/// Find an external `jet-<cmd>` executable on PATH (D-DX5).
fn find_external(cmd: &str) -> Option<PathBuf> {
    let exe = format!("{}-{}", jet::Syntax::BINARY_NAME, cmd);
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(&exe);
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// `jet ?` dispatch (D-FE-HELP1=D). Bare `jet ?` on a TTY opens the
/// interactive hybrid help app; `jet ? <query>` and any non-TTY use are
/// non-interactive — the static full palette (no args) or the best matches
/// for `<query>` (args), printed once and exited.
fn run_question_mark(args: &[String]) -> ! {
    // #360: shell widgets capture stdout because it carries the selected
    // command. stdin+stderr remain attached to the real palette TTY.
    let shell_prefill = std::env::var_os("JET_HELP_SHELL_PREFILL").is_some();
    let is_tty = std::io::stdin().is_terminal()
        && (std::io::stdout().is_terminal()
            || (shell_prefill && std::io::stderr().is_terminal()));
    let color = parse_color(args).resolve(is_tty);
    let mut query_args = Vec::new();
    let mut skip_value = false;
    for arg in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        if arg == "--color" {
            skip_value = true;
        } else if !arg.starts_with("--color=") {
            query_args.push(arg.as_str());
        }
    }
    if !query_args.is_empty() {
        let query = query_args.join(" ");
        print!("{}", jet::Help::run_query(&query, color));
        exit(ExitCodes::OK);
    }
    if is_tty {
        jet::Help::Interactive::run(color).ok();
        exit(ExitCodes::OK);
    }
    print!("{}\n", jet::Help::Render::render_categorized(&jet::Help::build_index(), 0, false, None, 72, color, None));
    exit(ExitCodes::OK);
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

/// Read `--target` in both canonical CLI forms. A flag that needs a value is
/// never allowed to fall through as a positional file or be ignored.
fn parse_target_flag(argv: &[String]) -> Result<Option<String>, ()> {
    let mut index = 0;
    while index < argv.len() {
        let arg = &argv[index];
        if let Some(value) = arg.strip_prefix("--target=") {
            return if value.is_empty() {
                Err(())
            } else {
                Ok(Some(value.to_string()))
            };
        }
        if arg == "--target" {
            return argv
                .get(index + 1)
                .filter(|value| !value.starts_with('-'))
                .cloned()
                .map(Some)
                .ok_or(());
        }
        index += 1;
    }
    Ok(None)
}

fn main() {
    // I2: install first, before any other work, so every uncaught panic
    // (including one triggered before argv parsing) renders the branded
    // ICE report instead of raw Rust panic text.
    jet::Diagnostics::install_ice_panic_hook();

    // Process-wide: any derive/comptime path may hit TirBridge before Loader.
    jet::boot_tir_eval();

    let mut raw: Vec<String> = std::env::args().skip(1).collect();

    // c6vz465: bare `jet` starts the REPL (D-REPL4); `jet ?` is help sugar.
    if raw.is_empty() {
        run_repl(None, &[], &[], ColorChoice::Auto);
        return;
    }
    if raw[0] == "?" {
        run_question_mark(&raw[1..]);
    }

    normalize_frequency_ring_argv(&mut raw);

    // D-PERFSESSION1=D: `jet perf` owns the full family, including run/test/bench
    // sessions that spawn the exact base-intent driver and write .jettrace.
    if raw.first().map(String::as_str) == Some("perf") {
        match CmdPerf::run(&raw) {
            CmdPerf::Outcome::Exit(code) => exit(code),
        }
    }

    if raw.iter().any(|a| a == "--version") {
        run_version();
        return;
    }

    // D-CLI1 (c11): split at the first standalone `--` separator.
    // Everything before `--` belongs to jet; everything after is forwarded to
    // the program verbatim (including tokens that look like jet flags).
    // `passthrough_sep` is Some(index) when `--` was present.
    let passthrough_sep = raw.iter().position(|a| a == "--");
    // `jet_argv`: the slice jet parses for its own flags and subcommand.
    let jet_argv: &[String] = match passthrough_sep {
        Some(pos) => &raw[..pos],
        None => &raw,
    };
    // `passthrough`: tokens after `--`, forwarded verbatim to the program.
    // When no `--` was given this is empty; the caller site decides whether to
    // fall back to the positional words instead.
    let passthrough: Vec<&String> = match passthrough_sep {
        Some(pos) => raw[pos + 1..].iter().collect(),
        None => Vec::new(),
    };

    // D-CLI-EMIT1=A: generated Rust has one spelling: `jet emit --rust`.
    let emit_rust = false;
    let emit_generated = jet_argv.iter().any(|a| a == "--emit-generated");
    let fmt_check = jet_argv.iter().any(|a| a == "--check");
    let dry_run = jet_argv.iter().any(|a| a == jet::CLI::DRY_RUN_FLAG);
    let json = jet_argv.iter().any(|a| a == "--json");
    reject_retired_gate_flags(jet_argv, json);
    let small = jet_argv.iter().any(|a| a == "--small");
    let freestanding_flag = jet_argv.iter().any(|a| a == "--freestanding");
    let gates = parse_gate_flags(jet_argv, json);
    let build_grants: Vec<String> = BuildEffect::ALL
        .into_iter()
        .filter(|effect| {
            jet_argv
                .iter()
                .any(|arg| arg == &format!("--allow-{}", effect.flag()))
        })
        .map(|effect| effect.flag().to_string())
        .collect();
    let locked = jet_argv.iter().any(|a| a == "--locked");
    let annotated = jet_argv.iter().any(|a| a == "--annotated");
    let verbose = jet_argv.iter().any(|a| a == "--verbose" || a == "-v");
    // D-A11YGATE1=B (c134 Phase 6): `jet lint --a11y` — opt-in, never blocking.
    let a11y = jet_argv.iter().any(|a| a == "--a11y");
    // D-TOOL5 (E2-M11): capability summary flags.
    let capabilities_json = jet_argv.iter().any(|a| a == "--capabilities-json");
    // D-SUPPLY1: `jet build --sbom` writes an SPDX SBOM next to the binary.
    let sbom = jet_argv.iter().any(|a| a == "--sbom");
    // E2-M15 / D-CONF-WORD1=A: the machine axis. `--target` accepts either
    // canonical spelling for a rustc triple or declared machine name; a
    // machine supplies its own triple and brings its no-OS facts with it.
    let requested_target: Option<String> = match parse_target_flag(jet_argv) {
        Ok(target) => target,
        Err(()) => {
            crate::cli_error!(
                @fix "E2104",
                "`--target` needs a value",
                "write `--target=<triple>` or `--target <triple>` before the source file"
            );
            exit(ExitCodes::USAGE);
        }
    };
    let selected_machine = requested_target
        .as_deref()
        .and_then(jet::Driver::target_machine_by_name);
    if let Some(name) = requested_target.as_deref() {
        if selected_machine.is_none() && name.starts_with("board.") {
            crate::cli_error!(@full "E3302", format!("target `{name}` is not available"), "the name is neither a declared target machine nor a recognized target triple", format!("use one of {}, or a rustc target triple", jet::Driver::TARGET_MACHINE_NAMES.join(", ")));
            exit(ExitCodes::USAGE);
        }
    }
    let cross_target: Option<String> = match &selected_machine {
        Some(machine) => Some(machine.triple.clone()),
        None => requested_target.clone(),
    };
    // A no-OS machine carries the freestanding fact, so naming it is enough.
    let freestanding = freestanding_flag
        || selected_machine.as_ref().is_some_and(|machine| machine.no_os);
    let remote_builder: Option<String> = jet_argv.iter().enumerate().find_map(|(index, arg)| {
        arg.strip_prefix("--builder=")
            .map(str::to_string)
            .or_else(|| (arg == "--builder").then(|| jet_argv.get(index + 1)).flatten().cloned())
    });
    // c134 Phase 7: `jet dev <file> --target=web --port=<N>` picks the dev
    // server's port explicitly instead of scanning from 8080.
    let dev_port: Option<u16> = jet_argv
        .iter()
        .find_map(|a| a.strip_prefix("--port=").map(str::to_string))
        .map(|s| {
            s.parse::<u16>().unwrap_or_else(|_| {
                crate::cli_error!(@fix "E2104", format!("`--port={}` isn't a valid port number", s), "use a number from 1 to 65535, e.g. `--port=3000`");
                exit(ExitCodes::USAGE);
            })
        });
    let explain_partition = jet_argv.iter().any(|a| a == "--explain-partition");
    // D-BUILDPROFILE1: `--release` is sugar for `--profile=release`.
    // `--profile=<name>` selects a named profile. Resolved against package.jet
    // in run_compile_cmd; only the name is collected here.
    let release_flag = jet_argv.iter().any(|a| a == "--release");
    let profile_flag: Option<String> = jet_argv
        .iter()
        .find_map(|a| a.strip_prefix("--profile=").map(str::to_string));
    // Effective profile name: --release wins over --profile when both given.
    let named_profile: Option<String> = if release_flag {
        Some(jet::Syntax::BUILD_PROFILE_RELEASE.to_string())
    } else {
        profile_flag
    };
    let output_name: Option<String> = {
        let mut found = None;
        let mut i = 0;
        while i < jet_argv.len() {
            let a = &jet_argv[i];
            if let Some(value) = a.strip_prefix("--output=") {
                found = Some(value.to_string());
                break;
            }
            if a == "--output" {
                found = Some(
                    jet_argv
                        .get(i + 1)
                        .filter(|value| !value.starts_with('-'))
                        .cloned()
                        .unwrap_or_default(),
                );
                break;
            }
            i += 1;
        }
        found
    };
    // #1659 criterion 3: one spelling, parsed once, threaded everywhere
    // OutputMode already reaches (build/run/test/dev/fmt/publish/doctor/…).
    // Criterion 3 says "one spelling" — no `-q` short alias.
    let quiet = jet_argv.iter().any(|a| a == "--quiet");
    let setting_overrides = parse_setting_overrides(jet_argv, json);
    let mode = OutputMode {
        json,
        color: parse_color(jet_argv),
        quiet,
    };
    // Positional args only. Keep bare `-` (stdin for `jet fmt -`); drop every
    // other dash-flag including short forms like `-u` / `-v` so they never become
    // the file target (D-TOOL4). D-CLI-BARE1=A: `-p <member>` also swallows its
    // value — a workspace member name is never a positional file/program arg.
    // `--output <name>` swallows its value the same way.
    let args: Vec<&String> = {
        let mut out = Vec::new();
        let mut skip_next = false;
        for a in jet_argv.iter() {
            if skip_next {
                skip_next = false;
                continue;
            }
            if a == "-p" || a == "--output" || a == "--gate" || a == "--scope" || a == "--kind" || a == "--set" || a == "--target" {
                skip_next = true;
                continue;
            }
            if a.starts_with("--output=") {
                continue;
            }
            if a.starts_with("--set=") {
                continue;
            }
            if a.starts_with("--scope=") || a.starts_with("--kind=") {
                continue;
            }
            if a.starts_with("--target=") {
                continue;
            }
            if a.as_str() == "-" || !a.starts_with('-') {
                out.push(a);
            }
        }
        out
    };
    let bench_filter = jet_argv
        .iter()
        .find_map(|a| a.strip_prefix("--filter=").map(str::to_string));

    if args.first().map(|s| s.as_str()) == Some("lsp") {
        // #1659 c2 (round 2): `jet self lsp --help`/`-h` must print help, not
        // start the language server on stdio.
        if jet_argv.iter().any(|a| jet::CLI::is_help_flag(a)) {
            print!("{}", command_help("lsp"));
            exit(ExitCodes::OK);
        }
        let sub = args.get(1).map(|s| s.as_str());
        let bench_flag = raw.iter().any(|a| a == "--bench");
        match (sub, bench_flag) {
            (Some("doctor"), _) => {
                jet::LSP::run_doctor();
                return;
            }
            (_, true) | (Some("--bench"), _) => {
                // jet self lsp --bench: run latency benchmark on a small program
                let src = include_str!("../examples/features/collections/wordcount.jet");
                jet::LSP::run_bench(src, 10, 200);
                return;
            }
            _ => {}
        }
        if let Err(e) = jet::LSP::run_stdio() {
            crate::cli_error!("E2105", "language server failed: {}", e);
            exit(ExitCodes::USER_ERROR);
        }
        return;
    }

    let cmd = match args.first() {
        Some(c) => c.as_str(),
        None => {
            // #1659 criterion 2: `jet --help`/`jet -h` are real requests for
            // the full command table, not the short orientation greeting.
            if jet_argv.iter().any(|a| jet::CLI::is_help_flag(a)) {
                print!("{}", usage());
                exit(ExitCodes::OK);
            }
            // No-args: a friendly greeting that orients, NOT a usage error.
            print!("{}", greeting());
            exit(ExitCodes::OK);
        }
    };
    if let Some(output) = output_name.as_deref() {
        if cmd != "run" || output.is_empty() {
            crate::cli_error!(@fix "E2104", "`--output` needs a runnable Output address with `jet run`", format!("write `jet run --output <address> <file.{}>`", jet::Syntax::FILE_EXT));
            exit(ExitCodes::USAGE);
        }
    }

    // D-OBSERVE-LIVE1=A: dev sessions expose bounded scheduler facts by
    // default; other executions opt in explicitly. Generated programs derive
    // their own PID and publish no payloads or secrets.
    if cmd == "dev" || raw.iter().any(|arg| arg == "--observe") {
        std::env::set_var("JET_OBSERVE", "1");
    }
    if raw.iter().any(|arg| arg == "--gc-trace") {
        CmdGc::configure_trace();
    }
    if raw.iter().any(|arg| arg == "--trace-tiers") {
        jet_jit::set_trace_tiers(true);
    }

    // If the first word is not in the single CLI registry, try an external
    // `jet-<cmd>` on PATH (D-DX5, cargo/git style), else teach E2101 with a
    // "did you mean".
    if !jet::CLI::is_builtin(cmd) {
        // c6vz465: `jet <file>` → `jet run <file>` when the first word names a
        // source path (not a typo'd subcommand like `buld`).
        if looks_like_jet_source(cmd) {
            let resolved = resolve_source_path(cmd);
            let program_args: Vec<&String> = if passthrough_sep.is_some() {
                passthrough.clone()
            } else {
                args.iter().skip(1).copied().collect()
            };
            run_compile_cmd(
                "run",
                &resolved,
                emit_rust,
                emit_generated,
                small,
                freestanding,
                gates,
                &build_grants,
                remote_builder.as_deref(),
                locked,
                cross_target.as_deref(),
                explain_partition,
                verbose,
                capabilities_json,
                sbom,
                named_profile.as_deref(),
                &setting_overrides,
                output_name.as_deref(),
                &program_args,
                mode,
            );
            return;
        }
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
                    crate::cli_error!("E2105", "couldn't run `{}`: {}", bin.display(), e);
                    exit(ExitCodes::USER_ERROR);
                });
            exit(status.code().unwrap_or(ExitCodes::OK));
        }
        unknown_subcommand(cmd);
    }

    if cmd == "run" {
        if let Some(target) = args.get(1).map(|s| s.as_str()) {
            if let Some(code) = dispatch_nixpkgs_run(&raw, target, passthrough_sep) {
                exit(code);
            }
        }
    }

    // #1659 criterion 2 (round 2): `jet <cmd> --help`/`-h` works for every
    // command, including the ones that own a bespoke flag vocabulary — not
    // just the generic ones. A handful of `owns_flags` commands already
    // render *better*, sub-verb-specific bespoke help deep in their own
    // dispatch and every one of their sub-verbs checks `is_help_flag` itself
    // (`bind`'s per-language usage; `diff`/`merge`'s `wants_help`; `perf`
    // which short-circuits earlier above) — those keep their own renderer
    // instead of being downgraded to the generic table text here.
    // `devtools` does NOT qualify: none of its sub-verbs (`grammars`,
    // `reduce`, `bless`, …) check for `--help` themselves, so without this
    // gate `jet self devtools grammars --help` silently executes (writes
    // files) instead of printing help — the exact bug this criterion exists
    // to close. Every other `owns_flags` command
    // (`prove`/`budget`/`report`/`clean`/`update`/`image`/`trust`/`hangar`/
    // `devtools`/…) previously either error-taught E2102 or, worse, executed
    // for real — this is checked before the pinned-toolchain re-exec so a
    // help request never pays for one.
    const BESPOKE_DEEP_HELP: &[&str] = &["bind", "diff", "merge", "perf"];
    let owns_flags = jet::CLI::owns_flag_vocabulary(cmd);
    let wants_help = jet_argv.iter().any(|a| jet::CLI::is_help_flag(a));
    if wants_help && !(owns_flags && BESPOKE_DEEP_HELP.contains(&cmd)) {
        print!("{}", command_help(cmd));
        exit(ExitCodes::OK);
    }

    // D-JPK-TOOLCHAIN1=A (#179): a version-pinned project hands off to its
    // pinned `jet` toolchain before any manifest-driven verb runs. A running
    // `jet` in the pinned channel runs natively; a genuine version mismatch
    // realizes the pinned prebuilt (never a source build) and re-execs into it.
    if matches!(cmd, "run" | "build" | "test" | "check" | "jobs") {
        maybe_dispatch_pinned_toolchain(&raw);
    }

    // Validate flags against the registry; an unknown/half-typed flag is E2102.
    // Skipped for commands that own a bespoke flag vocabulary or forward flags
    // downstream (so their flags aren't measured against the global set).
    if !owns_flags {
        check_flags(jet_argv, cmd);
    }
    if cmd == "bench" {
        reject_bench_test_flags(jet_argv);
    }

    // Commands with no required positional target.
    match cmd {
        "live" => {
            let pid = args
                .get(1)
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or_else(|| {
                    crate::cli_error!(@fix "E2104", "jet inspect live needs a process id", "run jet inspect live <pid>");
                    exit(ExitCodes::USAGE);
                });
            let once = raw.iter().any(|arg| arg == "--once")
                || mode.json
                || !std::io::stdout().is_terminal();
            loop {
                let snapshot = jet::DevServer::LiveInspect::read(pid).unwrap_or_else(|message| {
                    crate::cli_error!(@fix "E2105", message, "start the program with --observe, or attach to a jet dev process");
                    exit(ExitCodes::USER_ERROR);
                });
                if mode.json {
                    println!("{snapshot}");
                } else {
                    if !once {
                        print!("\x1b[2J\x1b[H");
                    }
                    print!("{}", jet::DevServer::LiveInspect::render(&snapshot));
                }
                if once {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
        "budget" => {
            exit(CmdBudget::run(&raw));
        }
        "parts" => run_project_parts(&raw, mode),
        "reserved" => {
            if mode.json {
                println!("{}", jet::CLI::reserved_report_json());
            } else {
                print!("{}", jet::CLI::reserved_report_text());
            }
            return;
        }
        // D-ONCE-LAW1=A (#1728): read out the one registration table — every
        // registered truth with its home, its renderers, and its guard.
        "facts" => {
            if mode.json {
                println!("{}", jet::Explain::facts_report_json());
            } else {
                print!("{}", jet::Explain::facts_report_text());
            }
            return;
        }
        "prove" => {
            let prove_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_prove(&prove_args, mode.json);
            return;
        }
        "diff" => { run_diff(&raw); return; }
        "merge" => { run_merge(&raw); return; }
        "report" => exit(run_report(&raw[1..])),
        "remote" => run_remote(&raw, mode),
        "help" => {
            if let Some(help) = raw.get(1).and_then(|command| structural_help(command)) {
                print!("{help}");
                exit(ExitCodes::OK);
            }
            print!("{}", usage());
            exit(ExitCodes::OK);
        }
        "jobs" => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let member_flag = flag_value(&raw, "-p");
            let entry = resolve_bare_entry("jobs", &cwd, member_flag)
                .unwrap_or_else(|| missing_bare_entry("run", &cwd));
            run_jobs(&entry.to_string_lossy(), mode);
            return;
        }
        "doctor" => {
            let online = raw.iter().any(|a| a == "--online");
            let apply = raw.iter().any(|a| a == "--fix");
            run_doctor(online, apply, mode);
            return;
        }
        "completions" => {
            run_completions(&raw[1..]);
            return;
        }
        "devtools" => {
            // Use the unfiltered argv: several devtools subcommands take their
            // own `--flags` (reduce's `--code`, bless's `--dry-run`) that the
            // global `args` filter would otherwise strip (same reason `bind`
            // reads from `raw` below).
            let devtool_args: Vec<&String> = raw.iter().skip(1).collect();
            run_devtools(&devtool_args, mode);
            return;
        }
        "man" => {
            print!("{}", jet::CLI::man_page(env!("CARGO_PKG_VERSION")));
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
            if jet_argv.iter().any(|a| a == "--web-graph") {
                run_explain_web_graph(&jet_argv[1..], mode);
                return;
            }
            if let (Some(subject), Some(file)) = (args.get(1), args.get(2)) {
                if subject.contains('.') && file.ends_with(jet::Syntax::FILE_EXT) {
                    if let Some(profile) = named_profile.as_deref() {
                        let _ = resolve_named_profile(profile, file, mode);
                    }
                    let profile = named_profile.as_deref().unwrap_or("dev");
                    run_module_explain(subject, file, profile, mode.json);
                    return;
                }
            }
            if args.get(1).map(|s| s.as_str()) == Some("marker") {
                run_explain_marker(
                    args.get(2).map(|s| s.as_str()),
                    args.get(3).map(|s| s.as_str()),
                    mode,
                );
                return;
            }
            let code = args.get(1).map(|s| s.as_str());
            if code
                .map(|value| {
                    is_diagnostic_code(value)
                        || value.starts_with("build.settings.")
                        || jet::Explain::lookup(value).is_some()
                        || value.eq_ignore_ascii_case("Build.Profile")
                        || value == "@build.profile"
                })
                .unwrap_or(true)
            {
                run_explain(code, args.get(2).map(|s| s.as_str()), mode);
            } else {
                exit(EngineDispatch::dispatch(
                    jet::Syntax::JETPACK_BINARY_NAME,
                    "explain",
                    &raw,
                ));
            }
            return;
        }
        "fmt" => {
            // D-FMTPROJECT1=D: project-level formatter. No positional target
            // required — defaults to discovering the workspace/project root.
            let path_args: Vec<String> =
                args[1..].iter().map(|s| s.as_str().to_string()).collect();
            let stdin_mode = path_args.iter().any(|p| p == "-");
            let stdin_path: Option<String> = jet_argv
                .iter()
                .find_map(|a| a.strip_prefix("--stdin-path=").map(str::to_string));
            let show_diff = jet_argv.iter().any(|a| a == "--diff") || dry_run;
            let changed_only = jet_argv.iter().any(|a| a == "--changed");
            let explicit_paths: Vec<String> =
                path_args.into_iter().filter(|p| p != "-").collect();
            run_fmt(
                &explicit_paths,
                stdin_mode,
                stdin_path.as_deref(),
                fmt_check || dry_run,
                show_diff,
                changed_only,
                mode,
            );
            return;
        }
        "fetch" => {
            // D-CLI-STORE2=A: script locking folds into `fetch --lock
            // <script.jet>` — the old standalone `jet lock` verb is retired.
            if let Some(script) = flag_value(&raw, "--lock") {
                run_lock(Some(script), mode);
                return;
            }
            run_fetch(locked);
            return;
        }
        "update" => {
            // D-JPK-TOOLCHAIN1=A (#179): `jet update jet [<channel>]` moves the
            // toolchain pin; anything else refreshes moving dependency selectors.
            if args.get(1).map(|s| s.as_str()) == Some("jet") {
                run_update_jet(args.get(2).map(|s| s.as_str()));
            }
            let dep = args.get(1).map(|s| s.as_str());
            run_update(dep);
            return;
        }
        "toolchain" => run_toolchain(),
        // U11 (D-JPK-SCRIPTDEP1=A): `jet init <script.jet>` lifts that
        // script's inline `use pkg#version;` deps into the freshly written
        // `package.jet`; bare `jet init` is unchanged.
        "init" => run_init(args.get(1).map(|s| s.as_str()), &raw, mode),
        "split" => run_split(&args, &raw, mode),
        "fold" => run_fold(&args, &raw, mode),
        // D-OPTGC1=A: the grouped report is active; the old bare cleanup alias
        // still teaches `jet clean`.
        "gc" => {
            match args.get(1).map(|word| word.as_str()) {
                Some("report") => {
                    CmdGc::run(&raw.iter().skip(2).cloned().collect::<Vec<_>>(), mode);
                    return;
                }
                None => {
                    emit_cli_report(
                        "E2101",
                        "`gc` isn't a jet command".to_string(),
                        "`jet clean` is the sole package-store cleanup entry (D-CLI-STORE2=A)".to_string(),
                        "run `jet clean`".to_string(),
                        json,
                    );
                    exit(ExitCodes::USAGE);
                }
                Some(other) => {
                    emit_cli_report(
                        "E2101",
                        format!("`{other}` isn't a jet gc command"),
                        "jet gc currently exposes only the automatic-promotion report".to_string(),
                        "run `jet gc report`".to_string(),
                        json,
                    );
                    exit(ExitCodes::USAGE);
                }
            }
        }
        "publish" => {
            let force = raw.iter().any(|a| a == "--force");
            // c146 (D-PKGSIGN1): sign by default; --no-sign opts out.
            let no_sign = raw.iter().any(|a| a == "--no-sign");
            run_publish(force, no_sign, mode);
            return;
        }
        "keygen" => {
            // c146: `jet registry keygen [--registry <name>] [--force]`.
            let registry = flag_value(&raw, "--registry");
            let force = raw.iter().any(|a| a == "--force");
            run_keygen(registry, force);
            return;
        }
        "key" => {
            // c146: `jet registry key backup [<dest>] [--registry <name>]`.
            match args.get(1).map(|s| s.as_str()) {
                Some("backup") => {
                    let registry = flag_value(&raw, "--registry");
                    let dest = args.get(2).map(|s| s.as_str());
                    run_key_backup(dest, registry);
                }
                Some(other) => {
                    crate::cli_error!("E2101", "unknown `jet registry key` subcommand `{}` — did you mean `jet registry key backup`?", other);
                    exit(ExitCodes::USER_ERROR);
                }
                None => {
                    crate::cli_error!("E2104", "`jet registry key` needs a subcommand — try `jet registry key backup`.");
                    exit(ExitCodes::USER_ERROR);
                }
            }
            return;
        }
        "yank" => {
            // D-VERSION1=A: mark a published version as yanked (no delete).
            // `jet registry yank <version> [--message <reason>]`
            let version = args.get(1).map(|s| s.as_str());
            let message = flag_value(&raw, "--message");
            run_yank(version, message);
            return;
        }
        "vendor" => {
            let vendor_dir = flag_value(&raw, "--vendor-dir");
            run_vendor(vendor_dir);
            return;
        }
        "schema" => {
            // D-MIGRATE2C: `jet inspect schema status` / `jet inspect schema squash --before <ver>`.
            // Use the unfiltered argv so `--before` and the verb survive.
            let schema_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_schema(&schema_args);
            return;
        }
        "semindex" => {
            // D-SEMINDEX1: stable semantic-index JSON smoke surface.
            let semindex_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_semindex(&semindex_args, mode.json);
            return;
        }
        "dossier" => {
            // D-WD2/D-DOSSIER1: umbrella explain view over semantic facts.
            let dossier_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_dossier(&dossier_args, mode.json);
            return;
        }
        "guarantees" => {
            let guarantee_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_guarantees(
                &guarantee_args,
                mode.json,
                mode.color_stderr(),
                gates,
                named_profile.as_deref().unwrap_or("dev"),
                freestanding,
            );
            return;
        }
        "provenance" => {
            let provenance_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_provenance(&provenance_args, mode.json);
            return;
        }
        "impact" => {
            // D-IMPACT1: blast-radius queries over the semantic index.
            let impact_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_impact(&impact_args, mode.json);
            return;
        }
        "import" => {
            exit(CmdImport::run(&raw, mode.json));
        }
        "graph" | "query" | "explain-build" => {
            let query_args: Vec<&String> = args.iter().skip(1).copied().collect();
            run_build_query(cmd, &query_args, mode);
            return;
        }
        "compiler" => {
            let operation = args.get(1).map(|value| value.as_str()).unwrap_or("");
            let file = args.get(2).map(|value| value.as_str()).unwrap_or("");
            if file.is_empty() {
                eprintln!(
                    "usage: jet inspect compiler <lex|parse|check|source-map> <file>"
                );
                exit(ExitCodes::USAGE);
            }
            run_compiler_api(operation, file, mode);
            return;
        }
        "codemod" => {
            // D-CODEMOD1: replayable semantic refactors over semindex facts.
            let codemod_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_codemod(&codemod_args);
            return;
        }
        "expand" => {
            // D-EXPANDCLI1=A: `jet inspect expand --facts <lens> <file>` / bare
            // `jet inspect expand <file>` — the transparency command (card #183).
            let expand_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_expand(&expand_args, mode.json);
            return;
        }
        "unsafe" => {
            let unsafe_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            CmdUnsafe::run(&unsafe_args, mode.json, mode.color_stderr(), gates);
            return;
        }
        "gates" => {
            let gate_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            CmdGates::run(&gate_args, mode.json, mode.color_stderr(), gates, false);
            return;
        }
        "authority" => {
            let authority_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            CmdGates::run(&authority_args, mode.json, mode.color_stderr(), gates, true);
            return;
        }
        "audit" => {
            let db_path = flag_value(&raw, "--advisory-db");
            run_audit(db_path);
            return;
        }
        "sbom" => {
            let cyclonedx = raw.iter().any(|a| a == "--cyclonedx");
            run_sbom(cyclonedx);
            return;
        }
        "bind" => {
            // S59 / E2-M14 Phase 4 (D-CBIND2): generate (or refresh) a C binding
            // cache from a header. Shares the bind backend with compile-time
            // auto-bind. Structural failures are rendered through the registered
            // E3208 diagnostic rather than leaking backend output.
            // Use the unfiltered argv: `bind` takes `--pkg`/`-o` flags that the
            // global `args` filter would otherwise strip.
            let bind_args: Vec<&String> = raw.iter().skip(1).collect();
            run_bind(&bind_args);
            return;
        }
        "env" => {
            // Scale-2 front door (U §8, D-DEV4): `jet env` maps to `jetpack
            // enter`, forwarding flags and any trailing `-- cmd`.
            // D-JPK-DISPATCH1=B (A1): execs the `jetpack` engine binary by
            // name — never linked in-process — so the compiler binary stays
            // standalone-checkable (deleting `jetpack` still leaves `jet
            // build`/`jet run` fully working).
            let mut fwd = raw.clone();
            if let Some(pos) = fwd.iter().position(|a| a == "env") {
                fwd.remove(pos);
            }
            fwd.insert(0, "enter".to_string());
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "env",
                &fwd,
            ));
        }
        "push" => {
            // U15: `jet push <fleet>` deploys a fleet. D-JPK-DISPATCH1=B: execs
            // the `jetpack` engine binary, forwarding `push <fleet>` verbatim.
            // Realization is gated (Phase D) — jetpack returns an honest E1243.
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "push",
                &raw,
            ));
        }
        "trust" => {
            // D-JPK-GRANTCMD1=A: public trust/grant graph command. Jetpack owns
            // the trust store; `jet` stays a front door and dispatches.
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "trust",
                &raw,
            ));
        }
        "bridge" => {
            // U16 (card c9jetpackgates): `jet bridge flake` translates a
            // foreign flake.nix's devShell into jetpack's `env.*` form.
            // D-JPK-DISPATCH1=B: dispatched to the jetpack engine exactly
            // like `push`/`config`, never linked in-process.
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "bridge",
                &raw,
            ));
        }
        "services" => {
            // U12 (card c9jetpackgates): `jet services up/down/health/logs`
            // supervises the project's dev `services:` processes. D-JPK-
            // DISPATCH1=B: dispatched to the jetpack engine exactly like
            // `push`/`bridge`/`config`, never linked in-process.
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "services",
                &raw,
            ));
        }
        "image" => {
            // U14 (D-JPK-IMAGE1=A, card c9jetpackgates): `jet image <name>`
            // builds a declared `.Oci` image into a native OCI layout.
            // D-JPK-DISPATCH1=B: dispatched to the jetpack engine exactly
            // like `push`/`bridge`/`services`, never linked in-process.
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "image",
                &raw,
            ));
        }
        "os" => {
            // D-JPK-OSVERB1=A: `jet os ...` is the public jetos front door.
            // The implementation still runs in the Jetpack engine process so
            // the compiler binary stays separate from package/OS realization.
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "os",
                &raw,
            ));
        }
        "config" => {
            // U19: `jet config trust add/list/remove` manages the env/dev
            // trust store. D-JPK-DISPATCH1=B: dispatched to jetpack, which
            // owns the trust store (`~/.jet/trust`) alongside env realization.
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "config",
                &raw,
            ));
        }
        "outdated" => {
            // U21 (D-JPK-CHANNEL1=A): channel freshness is owned by Jetpack's
            // lock/source resolver. `jet outdated` is a read-only front door.
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "outdated",
                &raw,
            ));
        }
        "search" => {
            // U26 (D-JPK-DISCOVER1=A): package discovery is owned by Jetpack's
            // local/offline index. The compiler front door only dispatches.
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "search",
                &raw,
            ));
        }
        "info" => {
            // U26: same local/offline discovery surface as `jet search`.
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "info",
                &raw,
            ));
        }
        "logs" => {
            // U27 (D-JPK-BUILDDBG1=A): persisted build logs live in Jetpack.
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "logs",
                &raw,
            ));
        }
        "clean" => {
            // U22 (D-JPK-GC1=B): hangar disk lifecycle belongs to Jetpack.
            // Top-level `jet clean` is the user-facing spelling; old gc docs
            // were retired with the same decision.
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "clean",
                &raw,
            ));
        }
        "repl" => {
            // E2-M18: interactive REPL (D-REPL1=A, D-REPL3=A).
            let project = raw
                .iter()
                .find_map(|a| a.strip_prefix("--project=").map(str::to_string))
                .or_else(|| flag_value(&raw, "--project").map(str::to_string));
            let allow: Vec<String> = BuildEffect::ALL
                .into_iter()
                .filter(|effect| {
                    raw.iter()
                        .any(|arg| arg == &format!("--allow-{}", effect.flag()))
                })
                .map(|effect| effect.flag().to_string())
                .collect();
            let deny: Vec<String> = BuildEffect::ALL
                .into_iter()
                .filter(|effect| {
                    raw.iter()
                        .any(|arg| arg == &format!("--deny-{}", effect.flag()))
                })
                .map(|effect| effect.flag().to_string())
                .collect();
            run_repl(project.as_deref(), &allow, &deny, mode.color);
            return;
        }
        "notebook" => {
            // D-NOTEBOOK-SURFACE1=D: shared REPL session + .jetnb / Jupyter.
            CmdNotebook::run_notebook(&raw);
            return;
        }
        // Teaching error: E0043 `jet install` -> `jet fetch`
        "install" => {
            eprintln!("Error [E0043]: `jet install` isn't a Jet command");
            eprintln!(" Why: Jet uses `jet fetch` to download and link dependencies");
            eprintln!(" Fix: run `jet fetch` to install all dependencies listed in package.jet");
            exit(ExitCodes::USER_ERROR);
        }
        "dev" => {
            // D-RUN-LAW1=A: every dev verb runs one program; `jet dev` uses
            // the file-scoped watcher below. Project-level `jetpack dev`
            // remains owned by Jetpack.
            // E2-M4 (D-DEV4): re-check and re-run the entry file on every save,
            // streaming output for sub-200ms feedback. The interpreter is a dev
            // convenience only — `jet build`/`jet run` never touch it (I2/I3).
            let try_anyway = raw.iter().any(|a| a == "--try-anyway");
            // c139 (D-JIT2=A): --interpret forces tier-0 interpreter; otherwise
            // CraneliftBackend wraps it (M0 delegates, M1+ JIT-compiles).
            let use_interpreter = raw.iter().any(|a| a == "--interpret");
            // c77 (D-DEVMODE1=A): default auto-detect; experts force a mode with
            // --restart / --swap / --watch=off.
            let policy = watch_policy_from(&raw, WatchPolicy::Auto);
            let file = match args.get(1) {
                Some(f) => f.as_str(),
                None => {
                    crate::cli_error!("E2104", "`jet dev` needs a file to watch: {} dev <file.{}>", jet::Syntax::BINARY_NAME, jet::Syntax::FILE_EXT);
                    exit(ExitCodes::USAGE);
                }
            };
            // E2-M15: `jet dev` has the same target validation contract as
            // build/run, even when its execution tier is the native watcher.
            // A target flag must never disappear merely because dev selects a
            // different runner branch below.
            if let Some(target) = cross_target.as_deref() {
                validate_target(target, mode);
            }
            if let Some(profile) = named_profile.as_deref() {
                let _ = resolve_named_profile(profile, file, mode);
            }
            // c-devserver (owner-directed 2026-07-01): a `.jet` file can define
            // its own `jet dev` behavior as ordinary Jet code — a top-level
            // `fn dev()` becomes the program's real (native) entry point,
            // normally configuring and starting a `core.web.devserver` value.
            // Checked FIRST, ahead of the #Target(Web)-inferred built-in web
            // server below: `fn dev()` is the more specific, user-authored
            // override, so a file that carries BOTH `#Target(Web)` (a build
            // default) and `fn dev()` (an explicit dev-command override) must
            // run the override, not silently fall back to the built-in server
            // because a *different* marker also happened to be present. (This
            // ordering bug was caught during manual verification — the first
            // cut checked #Target(Web) first, which made `fn dev()` totally
            // unreachable on any file that also declared #Target(Web), e.g.
            // ui_web_click.jet, which has both.)
            if has_dev_entry_fn(file) {
                run_dev_entry(file, mode, &setting_overrides);
                return;
            }
            if entry_returns_app(file) {
                if use_interpreter {
                    std::env::set_var("JET_APP_DEV", "1");
                    let dev_file = std::fs::canonicalize(file)
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|_| file.to_string());
                    std::env::set_var("JET_DEV_FILE", dev_file);
                    if let Some(port) = dev_port {
                        std::env::set_var("JET_APP_PORT", port.to_string());
                    }
                    run_dev(
                        file,
                        try_anyway,
                        policy,
                        gates,
                        mode,
                        true,
                        named_profile.as_deref().unwrap_or("dev"),
                        &setting_overrides,
                    );
                }
                run_web_app_dev_entry(file, mode, dev_port, &setting_overrides);
                return;
            }
            // c134 Phase 7: `jet dev <file> --target=web` compiles to JS/WASM
            // and serves `build/` with browser live-reload — a completely
            // different execution model from the native interpret/hot-swap
            // loop above, so it's a separate function, not a new branch
            // inside `run_dev`'s interpreter machinery.
            // D-WEBDEFAULT1 (ratified 2026-07-01, c134): no explicit --target= falls back to the
            // file's own `#Target(Web)` marker, if any.
            if effective_target("dev", file, cross_target.as_deref()).as_deref()
                == Some(jet::Syntax::BUILD_TARGET_WEB)
            {
                run_dev_web(file, mode, verbose, dev_port, &setting_overrides);
                return;
            }
            run_dev(
                file,
                try_anyway,
                policy,
                gates,
                mode,
                use_interpreter,
                named_profile.as_deref().unwrap_or("dev"),
                &setting_overrides,
            );
            return;
        }
        "debug" => {
            // D-DBG1/D-DBG3: `jet debug <file>` — the source-level step
            // debugger. Loads + checks the file, then steps it in the dev
            // interpreter with an interactive `(jet)` prompt. I2: every line,
            // frame, and value shown is in Jet terms (never generated Rust).
            //
            // D-DBG3 step 2 (dap-debugger): `--dap` always uses the native
            // lldb backend (an editor debugs the compiled program, never the
            // interpreter); otherwise auto-detect via the SAME boundary scan
            // the interpreter itself declines on (E2203) — one command, one
            // meaning (I8), the backend choice is never a separate flag.
            let raw_frames = raw.iter().any(|a| a == "--raw-frames"); // D-DBG2
            let dap = raw.iter().any(|a| a == "--dap");
            // D-CLI-BARE1=A: bare `jet debug` inside a package resolves the
            // entry the same way run/build/check/bench do; outside a package
            // the usage error is unchanged.
            let file: String = match args.get(1) {
                Some(f) => f.to_string(),
                None => {
                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    let member_flag = flag_value(&raw, "-p");
                    match resolve_bare_entry("debug", &cwd, member_flag) {
                        Some(entry) => entry.to_string_lossy().into_owned(),
                        None => {
                            crate::cli_error!("E2104", "`jet debug` needs a file to debug: {} debug <file.{}>", jet::Syntax::BINARY_NAME, jet::Syntax::FILE_EXT);
                            exit(ExitCodes::USAGE);
                        }
                    }
                }
            };
            let resolved = resolve_source_path(&file);
            let use_native = dap || jet::Debug::needs_native(&resolved).unwrap_or(false);
            if !use_native {
                exit(jet::Debug::run_debug(&resolved));
            }
            exit(run_debug_native(&resolved, raw_frames, dap, mode));
        }
        // D-CLI-STORE2=A / D-JPK-STORECLI1=D: `jet hangar` owns every physical
        // store verb. `verify`/`rollback`/`generations` reuse the existing
        // real generation-tracking logic (renamed from `store`); `du` is
        // Jetpack's real per-object disk accounting. Archive operations cross
        // the version-checked Jetpack boundary so every transfer verb shares
        // one signed archive and closure implementation.
        "hangar" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "verify" => run_hangar_verify(),
                "rollback" => {
                    let gen_str = args.get(2).map(|s| s.as_str()).unwrap_or("");
                    run_hangar_rollback(gen_str);
                }
                "generations" => run_hangar_generations(),
                "du" => {
                    exit(EngineDispatch::dispatch(
                        jet::Syntax::JETPACK_BINARY_NAME,
                        "hangar",
                        &raw,
                    ));
                }
                "repair" | "copy" | "import" | "export" | "dump" | "restore" | "sign" => {
                    exit(EngineDispatch::dispatch(
                        jet::Syntax::JETPACK_BINARY_NAME,
                        "hangar",
                        &raw,
                    ));
                }
                _ => {
                    crate::cli_error!(@fix "E2101", format!("unknown hangar subcommand `{}`", sub), "run `jet hangar help` to see every hangar command");
                    exit(ExitCodes::USAGE);
                }
            }
            return;
        }
        // D-JPK-CACHECONFIG1=D: cache roles and mirrors are host-owned by
        // Jetpack. The compiler front door forwards the exact argv.
        "cache" => {
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "cache",
                &raw,
            ));
        }
        "shared-store" => {
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "shared-store",
                &raw,
            ));
        }
        "eval" => {
            // S60 / D-PURE1 (E2-M16): deterministic evaluation of a pure program.
            let pure_flag = raw.iter().any(|a| a == "--pure");
            let file = match args.get(1) {
                Some(f) => f.as_str(),
                None => {
                    crate::cli_error!("E2104", "`jet eval` needs a file: {} eval --pure <file.{}>", jet::Syntax::BINARY_NAME, jet::Syntax::FILE_EXT);
                    exit(ExitCodes::USAGE);
                }
            };
            let resolved = resolve_source_path(file);
            run_eval(&resolved, pure_flag, mode);
            return;
        }
        _ => {}
    }

    // D-CLI-BARE1=A: `-p <member>` picks a workspace member for the bare-entry
    // resolver below; declared here so its borrow outlives `target`.
    let bare_member_flag = flag_value(&raw, "-p");
    let named_build_entry = match args.get(1) {
        Some(f) if cmd == "build" && checked_explicit_file(Path::new(f.as_str())).is_none() => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            match resolve_named_build_member(&cwd, f) {
                Ok(entry) => entry.map(|entry| entry.to_string_lossy().into_owned()),
                Err(error) => report_build_resolution_error(error),
            }
        }
        _ => None,
    };
    let target = match args.get(1) {
        Some(f) if cmd == "build" => {
            named_build_entry.as_deref().unwrap_or(f.as_str())
        }
        Some(f) => f.as_str(),
        None => {
            // No target: try project-root mode for run/build/test/bench.
            match cmd {
                "run" | "build" | "test" | "check" | "bench" => {
                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    if let Some(entry) = resolve_bare_entry(cmd, &cwd, bare_member_flag) {
                        let entry_str = entry.to_string_lossy().to_string();
                        match cmd {
                            "test" => {
                                run_test_opts(
                                    &entry_str,
                                    TestRunOpts {
                                        show_default: jet_argv.iter().any(|a| a == "--show-default"),
                                        release: release_flag,
                                        trace_tiers: jet_argv.iter().any(|a| a == "--trace-tiers"),
                                        ..Default::default()
                                    },
                                    mode,
                                );
                                return;
                            }
                            "bench" => {
                                // A bare bench is a project target, not only
                                // the resolved run entry. This keeps its
                                // discovery surface identical to `jet bench
                                // <directory>` while preserving workspace
                                // member selection from the shared resolver.
                                let entry_dir = entry
                                    .parent()
                                    .filter(|path| !path.as_os_str().is_empty())
                                    .unwrap_or_else(|| Path::new("."));
                                let project = jet::Loader::find_manifest_root(entry_dir)
                                    .unwrap_or_else(|| entry_dir.to_path_buf());
                                let project = project.to_string_lossy();
                                run_bench(
                                    &project,
                                    BenchRunOpts {
                                        show_default: jet_argv.iter().any(|a| a == "--show-default"),
                                        filter: bench_filter.clone(),
                                    },
                                    mode,
                                );
                                unreachable!("run_bench exits after the project walk")
                            }
                            _ => {
                                // D-CLI1: use passthrough slice if `--` was present;
                                // otherwise fall back to positional words after the subcommand.
                                let program_args: Vec<&String> = if passthrough_sep.is_some() {
                                    passthrough.clone()
                                } else {
                                    args.iter().skip(1).copied().collect()
                                };
                                if cmd == "run" && run_wants_watch(&raw) {
                                    let try_anyway = raw.iter().any(|a| a == "--try-anyway");
                                    let use_interpreter = raw.iter().any(|a| a == "--interpret");
                                    run_dev(
                                        &entry_str,
                                        try_anyway,
                                        WatchPolicy::Restart,
                                        gates,
                                        mode,
                                        use_interpreter,
                                        named_profile.as_deref().unwrap_or("dev"),
                                        &setting_overrides,
                                    );
                                    return;
                                }
                                let effective =
                                    effective_target(cmd, &entry_str, cross_target.as_deref());
                                run_compile_cmd(
                                    cmd,
                                    &entry_str,
                                    emit_rust,
                                    emit_generated,
                                    small,
                                    freestanding,
                                    gates,
                                    &build_grants,
                                    remote_builder.as_deref(),
                                    locked,
                                    effective.as_deref(),
                                    explain_partition,
                                    verbose,
                                    capabilities_json,
                                    sbom,
                                    named_profile.as_deref(),
                                    &setting_overrides,
                                    output_name.as_deref(),
                                    &program_args,
                                    mode,
                                );
                                return;
                            }
                        }
                    } else {
                        missing_bare_entry(cmd, &cwd);
                    }
                }
                _ => {
                    eprint!("{}", usage());
                    exit(ExitCodes::USAGE);
                }
            }
        }
    };

    match cmd {
        "fix" => {
            let edition = jet_argv
                .iter()
                .find_map(|a| a.strip_prefix("--edition=").map(str::to_string));
            run_fix(target, dry_run, edition.as_deref());
        }
        "new" => run_new(target, annotated, mode),
        "test" => {
            let update_snapshots = jet_argv
                .iter()
                .any(|a| a == "--update-snapshots" || a == "-u");
            // D-COV1: `jet test --coverage` builds an instrumented harness and
            // reports function and branch coverage after the test results.
            let coverage = jet_argv.iter().any(|a| a == "--coverage");
            // D-BUILDPROFILE1: `jet test --release` must compile the harness
            // with the same release AOT profile used by `jet build`/`jet run`.
            let release = jet_argv.iter().any(|a| a == "--release");
            // The child harness owns the observable marker; pass the request
            // across the process boundary instead of relying on parent state.
            let trace_tiers = jet_argv.iter().any(|a| a == "--trace-tiers");
            // D-TESTKIT1=A gap #4: `--filter=<substr>` keeps only test names
            // containing it (harness-side, `JET_TEST_FILTER`).
            let filter = jet_argv
                .iter()
                .find_map(|a| a.strip_prefix("--filter=").map(str::to_string));
            // `--shuffle` (random seed, printed so the run is reproducible after
            // the fact) or `--shuffle=<seed>` (reproduce a specific order).
            let shuffle = jet_argv
                .iter()
                .find_map(|a| a.strip_prefix("--shuffle=").map(str::to_string));
            let shuffle_bare = jet_argv.iter().any(|a| a == "--shuffle");
            let shuffle_seed: Option<u64> = if let Some(s) = shuffle {
                match s.parse::<u64>() {
                    Ok(n) => Some(n),
                    Err(_) => {
                        crate::cli_error!(@fix "E2104", format!("`--shuffle={}` isn't a number", s), "use `--shuffle=<seed>` (e.g. `--shuffle=42`), or bare `--shuffle` for a random seed");
                        exit(ExitCodes::USAGE);
                    }
                }
            } else if shuffle_bare {
                // No seed given: derive one from the clock so each run differs,
                // but the harness always prints the seed it used, so a failure
                // is reproducible with `--shuffle=<printed seed>`.
                Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos() as u64)
                        .unwrap_or(0),
                )
            } else {
                None
            };
            // D-TESTKIT1=A gap #3: parallel by default, `--serial` forces one
            // test at a time (matches `--update-snapshots`/`-u`'s existing style
            // of a plain boolean flag).
            let serial = jet_argv.iter().any(|a| a == "--serial");
            let show_default = jet_argv.iter().any(|a| a == "--show-default");
            // Keep directory targets intact so package tests/checks are
            // collected together instead of resolving to one run entry.
            let target_path = Path::new(target);
            let resolved = if target_path.is_dir() {
                target.to_string()
            } else {
                resolve_source_path(target)
            };
            run_test_opts(
                &resolved,
                TestRunOpts {
                    show_default,
                    update_snapshots,
                    coverage,
                    release,
                    trace_tiers,
                    filter,
                    shuffle_seed,
                    serial,
                },
                mode,
            );
        }
        "add" => run_add(&raw),
        "remove" => run_remove(target),
        // D-TOOL3 (E2-M11): `jet emit --rust` — print the generated Rust source.
        "emit" => {
            let rust_flag = jet_argv.iter().any(|a| a == "--rust");
            if !rust_flag {
                eprintln!(
                    "usage: {} emit --rust <file.{}>",
                    jet::Syntax::BINARY_NAME,
                    jet::Syntax::FILE_EXT
                );
                eprintln!(" Why: `emit` needs a mode flag; today only `--rust` is supported");
                eprintln!(
                    " Fix: run `{} emit --rust <file.{}>` to print the generated Rust",
                    jet::Syntax::BINARY_NAME,
                    jet::Syntax::FILE_EXT
                );
                exit(ExitCodes::USAGE);
            }
            run_emit_rust(target, mode);
        }
        // D-TOOL5 (E2-M11): `jet bench` — benchmark a Jet program.
        "bench" => {
            let target_path = Path::new(target);
            let resolved = if target_path.is_dir() {
                target.to_string()
            } else {
                resolve_source_path(target)
            };
            run_bench(
                &resolved,
                BenchRunOpts {
                    show_default: jet_argv.iter().any(|a| a == "--show-default"),
                    filter: bench_filter,
                },
                mode,
            );
        }
        // D-TESTKIT1=A (c308 pass 2): `jet fuzz <file> [<test-name>]` — fuzz a
        // parameterized `#Test fn` (D-TEST1's property-test form).
        "fuzz" => {
            let test_name = args.get(2).map(|s| s.as_str());
            let iterations = jet_argv
                .iter()
                .find_map(|a| a.strip_prefix("--iterations="))
                .and_then(|v| v.parse::<u64>().ok());
            let time_budget_ms = jet_argv
                .iter()
                .find_map(|a| a.strip_prefix("--time="))
                .and_then(|v| v.parse::<f64>().ok())
                .map(|secs| (secs * 1000.0) as u64);
            let seed = jet_argv
                .iter()
                .find_map(|a| a.strip_prefix("--seed="))
                .and_then(|v| v.parse::<u64>().ok());
            let corpus = jet_argv
                .iter()
                .find_map(|a| a.strip_prefix("--corpus="))
                .map(str::to_string);
            let resolved = resolve_source_path(target);
            run_fuzz(
                &resolved,
                test_name,
                FuzzRunOpts {
                    iterations,
                    time_budget_ms,
                    seed,
                    corpus,
                },
                mode,
            );
        }
        // D-A11YGATE1=B (c134 Phase 6): `jet lint --a11y` — opt-in accessibility
        // lints (E2930/E2931), never blocking `jet build`/`jet run`.
        "lint" => {
            if !a11y {
                eprintln!(
                    "usage: {} lint --a11y <file.{}>",
                    jet::Syntax::BINARY_NAME,
                    jet::Syntax::FILE_EXT
                );
                eprintln!(" Why: `lint` needs a category flag; today only `--a11y` is supported (D-A11YGATE1)");
                eprintln!(
                    " Fix: run `{} lint --a11y <file.{}>` to check accessibility (missing roles, unlabeled controls)",
                    jet::Syntax::BINARY_NAME,
                    jet::Syntax::FILE_EXT
                );
                exit(ExitCodes::USAGE);
            }
            let resolved = resolve_source_path(target);
            run_lint_a11y(&resolved, mode);
        }
        // Teaching error: E0042 foreign manifest filename, E0043 `jet install`
        "install" => {
            eprintln!("Error [E0043]: `jet install` isn't a Jet command");
            eprintln!(" Why: Jet uses `jet fetch` to download and link dependencies");
            eprintln!(" Fix: run `jet fetch` to install all dependencies listed in package.jet");
            exit(ExitCodes::USER_ERROR);
        }
        _ => {
            // D-CLI1: use passthrough slice if `--` was present; otherwise fall
            // back to positional words after the file (args[0]=cmd, args[1]=file).
            let program_args: Vec<&String> = if passthrough_sep.is_some() {
                passthrough.clone()
            } else {
                args.iter().skip(2).copied().collect()
            };
            // Ext-optional CLI: `jet run examples/test` resolves to `examples/test.jet`
            // for the path-accepting compile commands.
            let resolved = if matches!(cmd, "run" | "build" | "check") {
                resolve_source_path(target)
            } else {
                target.to_string()
            };
            if cmd == "run" {
                // #439 / E3-UL6: `jet run --watch` uses the shared dependency-
                // aware engine; `jet dev` keeps the richer swap/overlay surface.
                if run_wants_watch(&raw) {
                    let try_anyway = raw.iter().any(|a| a == "--try-anyway");
                    let use_interpreter = raw.iter().any(|a| a == "--interpret");
                    run_dev(
                        &resolved,
                        try_anyway,
                        WatchPolicy::Restart,
                        gates,
                        mode,
                        use_interpreter,
                        named_profile.as_deref().unwrap_or("dev"),
                        &setting_overrides,
                    );
                    return;
                }
            }
            let effective = effective_target(cmd, &resolved, cross_target.as_deref());
            run_compile_cmd(
                cmd,
                &resolved,
                emit_rust,
                emit_generated,
                small,
                freestanding,
                gates,
                &build_grants,
                remote_builder.as_deref(),
                locked,
                effective.as_deref(),
                explain_partition,
                verbose,
                capabilities_json,
                sbom,
                named_profile.as_deref(),
                &setting_overrides,
                output_name.as_deref(),
                &program_args,
                mode,
            );
        }
    }
}

/// #439 / E3-UL6: `jet run --watch` enters the shared dependency-aware
/// watch engine. `--watch=off` (and a bare one-shot `jet run`) stay one-shot.
fn run_wants_watch(raw: &[String]) -> bool {
    let off = raw.iter().any(|a| a == "--watch=off");
    if off {
        return false;
    }
    raw.iter().any(|a| {
        a == "--watch"
            || a == "--watch=on"
            || a == "--watch=true"
            || a.starts_with("--watch=") && a != "--watch=off"
    })
}

/// Resolve a source-path argument, allowing the `.jet` extension to be omitted
/// (ext-optional CLI). If `raw` exists as-is, use it. Otherwise, if `raw.jet`
/// exists, use that. If neither exists, return `raw` unchanged so the normal
/// file-not-found diagnostic fires with the original name the user typed.
/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): resolve the effective `--target=` value for
/// `file`. Precedence: an explicit CLI flag always wins; else `package.jet`'s
/// `target: "web"` (a managed package's project-level default); else a
/// lightweight parse of `file` for a top-level `#Target(Web)` marker (a loose
/// file's own default, for standalone examples with no manifest at all).
/// Reparses the file/manifest — wasteful compared to threading the fact
/// through the real compile pipeline, but `jet` recompiles from scratch on
/// every invocation anyway (no incremental compilation), so a couple of
/// extra cheap lex+parse passes are negligible, and it keeps this CLI-only
/// concern out of `ProgramBundle`/codegen entirely.
///
/// `cmd == "run"` never infers: "run" means "execute this program and show
/// me console output," a native-execution concept a web build can't satisfy
/// (there's no runtime to run a `.wasm`+`.js` bundle as a console program —
/// it just fails trying to exec the wrong artifact as a native binary). `jet
/// run` on a web-targeted file still requires an explicit `--target=web`
/// (which then hits the normal "can't run a cross-compiled binary" message,
/// same as any other cross target) or, better, `jet dev`/`jet build`.
fn effective_target(cmd: &str, file: &str, explicit: Option<&str>) -> Option<String> {
    if explicit.is_some() {
        return explicit.map(str::to_string);
    }
    if cmd == "run" {
        return None;
    }
    if let Some(target) = manifest_default_target(file) {
        return Some(target);
    }
    let src = fs::read_to_string(file).ok()?;
    let (toks, lex_diags) = jet::Lexer::lex(&src);
    if !lex_diags.is_empty() {
        return None;
    }
    let prog = jet::Parser::parse(&toks).ok()?;
    prog.default_target
}

/// c-devserver (owner-directed 2026-07-01): cheap lex+parse check (same style
/// as `effective_target`) for whether `file`'s entry defines a top-level `fn
/// dev()`. A lex/parse failure here is never fatal — it just means "no"; the
/// real diagnostics surface a moment later when `run_dev_entry`/`run_dev`
/// actually compiles the file.
fn has_dev_entry_fn(file: &str) -> bool {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let (toks, lex_diags) = jet::Lexer::lex(&src);
    if !lex_diags.is_empty() {
        return false;
    }
    let prog = match jet::Parser::parse(&toks) {
        Ok(p) => p,
        Err(_) => return false,
    };
    prog.items
        .iter()
        .any(|i| matches!(i, jet::AST::Item::Func(f) if f.name == "dev"))
}

fn entry_returns_app(file: &str) -> bool {
    let source = match fs::read_to_string(file) {
        Ok(source) => source,
        Err(_) => return false,
    };
    let (tokens, diagnostics) = jet::Lexer::lex(&source);
    if !diagnostics.is_empty() {
        return false;
    }
    let program = match jet::Parser::parse(&tokens) {
        Ok(program) => program,
        Err(_) => return false,
    };
    program.items.iter().any(|item| {
        let jet::AST::Item::Func(function) = item else {
            return false;
        };
        if function.name != "run" {
            return false;
        }
        match function.return_type.as_ref() {
            Some(jet::AST::Type::Named(name)) => name == "App",
            Some(jet::AST::Type::Result { ok, .. }) => {
                matches!(ok.as_ref(), jet::AST::Type::Named(name) if name == "App")
            }
            _ => false,
        }
    })
}

/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): a Package root's `target: "web"`, if `file` sits
/// inside a managed package (found via the same `find_manifest_root` walk
/// `jet run`/`jet build` already use to resolve project-root mode).
fn manifest_default_target(file: &str) -> Option<String> {
    let start = Path::new(file).parent().unwrap_or(Path::new("."));
    let root = match jet::Loader::find_manifest_root_checked(start) {
        Ok(Some(root)) => root,
        Ok(None) => return None,
        Err(diagnostic) => report_entry_diagnostic(diagnostic),
    };
    let manifest = match jet::Package::PackageFacts::load_checked(&root) {
        Ok(Some(manifest)) => manifest,
        Ok(None) => return None,
        Err(error) => report_entry_authority_error(error),
    };
    manifest.target
}

pub(crate) fn resolve_source_path(raw: &str) -> String {
    let path = Path::new(raw);
    // A directory argument is a project root: resolve its canonical entry.
    // `jet run <dir>` just works.
    match jet::Authority::AuthorityResolver::open(path) {
        Ok(resolver) => {
            let entry = find_project_entry(resolver.root());
            if let Ok(relative) = entry.strip_prefix(resolver.root()) {
                match resolver.checked_file(relative) {
                    Ok(file) => {
                        resolver
                            .revalidate_file(&file)
                            .unwrap_or_else(|diagnostic| report_entry_authority_error(diagnostic));
                        return file.path.to_string_lossy().into_owned();
                    }
                    Err(error) if error.is_missing() => {}
                    Err(error) => report_entry_authority_error(error),
                }
            }
            let has_package = match resolver.checked_manifest(Path::new(".")) {
                Ok(_) => true,
                Err(error) if error.is_missing() => false,
                Err(error) => report_entry_authority_error(error),
            };
            if !has_package {
                crate::cli_error!(
                    "E2105",
                    "no `{}` entry in `{}`",
                    jet::Syntax::DEFAULT_ENTRY_FILE,
                    raw
                );
                exit(ExitCodes::USER_ERROR);
            }
            return entry.to_string_lossy().into_owned();
        }
        Err(error) if error.is_missing() => {}
        Err(error) if matches!(error, jet::Authority::AuthorityError::WrongKind { .. }) => {}
        Err(error) => report_entry_authority_error(error),
    }
    if let Some(path) = checked_explicit_file(path) {
        return path.to_string_lossy().into_owned();
    }
    let with_ext = format!("{}.{}", raw, jet::Syntax::FILE_EXT);
    if let Some(path) = checked_explicit_file(Path::new(&with_ext)) {
        return path.to_string_lossy().into_owned();
    }
    raw.to_string()
}

fn checked_explicit_file(path: &Path) -> Option<PathBuf> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let resolver = match jet::Authority::AuthorityResolver::open(parent) {
        Ok(resolver) => resolver,
        Err(error) if error.is_missing() => return None,
        Err(error) => report_entry_authority_error(error),
    };
    let name = path.file_name()?;
    match resolver.checked_file(Path::new(name)) {
        Ok(file) => Some(file.path),
        Err(error) if error.is_missing() => None,
        Err(error) => report_entry_authority_error(error),
    }
}

/// Find the entry `.jet` file for a project rooted at `root` (D-ILE1, owner
/// amendment 2026-07-17). `run.jet` is the zero-ceremony default, followed by
/// `src/run.jet` and `<package>.jet`. Missing-entry errors name `run.jet`.
pub(crate) fn find_project_entry(root: &Path) -> PathBuf {
    let resolver = match jet::Authority::AuthorityResolver::open(root) {
        Ok(resolver) => resolver,
        Err(error) => report_entry_authority_error(error),
    };
    let package = match resolver.checked_package(Path::new(".")) {
        Ok(package) => Some(package),
        Err(error) if error.is_missing() => None,
        Err(error) => report_entry_authority_error(error),
    };
    if let Some(package) = &package {
        match package.facts.resolve_run_entry_checked(&resolver) {
            Ok(Some(entry)) => return entry.path,
            Ok(None) => {}
            Err(error) => {
                crate::cli_error!(@fix "E2105", error, "repair the typed Package output or point at a `.jet` file directly");
                exit(ExitCodes::USER_ERROR);
            }
        }
    }
    if let Some(entry) = checked_project_entry(&resolver, Path::new(jet::Syntax::DEFAULT_ENTRY_FILE)) {
        return entry;
    }
    if let Some(entry) = checked_project_entry(
        &resolver,
        &Path::new("src").join(jet::Syntax::DEFAULT_ENTRY_FILE),
    ) {
        return entry;
    }
    if let Some(manifest) = package.as_ref().map(|package| &package.facts) {
        let named = resolver.root().join(format!(
            "{}.{}",
            manifest.name,
            jet::Syntax::FILE_EXT
        ));
        if let Ok(relative) = named.strip_prefix(resolver.root()) {
            if let Some(entry) = checked_project_entry(&resolver, relative) {
                return entry;
            }
        }
    }
    resolver.root().join(jet::Syntax::DEFAULT_ENTRY_FILE)
}

fn checked_project_entry(
    resolver: &jet::Authority::AuthorityResolver,
    relative: &Path,
) -> Option<PathBuf> {
    match resolver.checked_file(relative) {
        Ok(file) => {
            if let Err(error) = resolver.revalidate_file(&file) {
                report_entry_authority_error(error);
            }
            Some(file.path)
        }
        Err(error) if error.is_missing() => None,
        Err(error) => report_entry_authority_error(error),
    }
}

fn report_entry_authority_error(error: jet::Authority::AuthorityError) -> ! {
    crate::cli_error!(@fix "E1334", error.to_string(), "restore the required regular no-follow authority file or directory");
    exit(ExitCodes::USER_ERROR)
}

fn report_build_resolution_error(error: String) -> ! {
    if error.contains("two build entries for the package:") {
        crate::cli_error!(@full "E3520", error, "one package has exactly one build entry so policy and provenance have one auditable home", "keep one `fn build` and remove every other entry");
    } else {
        crate::cli_error!(@fix "E1334", error, "repair the package source and try the build again");
    }
    exit(ExitCodes::USER_ERROR)
}

/// Resolve positional `jet build <member>` against only declared depth-one
/// workspace members. This chooses the member; the normal PackageFacts/Driver
/// path still chooses and executes its one build entry.
fn resolve_named_build_member(cwd: &Path, wanted: &str) -> Result<Option<PathBuf>, String> {
    let Ok(resolver) = jet::Authority::AuthorityResolver::open(cwd) else {
        return Ok(None);
    };
    let Some(source) = resolver.resolve_workspace_source().ok().flatten() else {
        return Ok(None);
    };
    if source.role != jetpack::WorkspaceFile::WorkspaceSourceRole::Index {
        return Ok(None);
    }
    let Ok(plan) = jetpack::WorkspaceFile::evaluate_checked_source(&source, &resolver) else {
        return Ok(None);
    };
    let Some(member) = plan.members.iter().find(|member| member.name == wanted) else {
        return Ok(None);
    };
    resolve_member_build_entry(&cwd.join(&member.path))
}

fn resolve_member_build_entry(root: &Path) -> Result<Option<PathBuf>, String> {
    let member_resolver = jet::Authority::AuthorityResolver::open(&root)
        .map_err(|error| error.to_string())?;
    let checked = member_resolver
        .checked_manifest(Path::new("."))
        .map_err(|error| error.to_string())?;
    if let Ok(Some(entry)) = checked
        .facts
        .resolve_run_entry_checked(&member_resolver)
    {
        return Ok(Some(entry.path));
    }
    if let Some(entry) = checked
        .facts
        .resolve_build_entry_checked(&member_resolver)
        .map_err(|error| error.to_string())?
    {
        return Ok(Some(entry.path));
    }
    Ok(None)
}

/// D-CLI-BARE1=A: shared bare-entry resolver for `run`/`dev`/`debug`/`bench`/
/// `check`/`build` inside a package — the one rule all six share instead of
/// each hand-rolling its own "no file given" fallback.
///
/// A workspace member list (D-JPK-WORKSPACE, `jetpack::WorkspaceFile`)
/// with more than one runnable member (a member whose own entry resolves to a
/// real file, D-ILE1) is an ambiguity naming every member; `-p <member>`
/// picks one explicitly, or the caller can always name a file directly. A
/// plain project (no workspace, or a workspace with exactly zero or one
/// runnable member) resolves exactly like `find_project_entry` always has —
/// no behavior change for the overwhelmingly common single-package case.
///
/// A declaration-resolved workspace source (D-JPK-WORKSPACE2) is checked at
/// `cwd` directly first —
/// `jetpack::WorkspaceFile::load` never walks upward, matching every
/// other workspace-aware call site — because a monorepo workspace root often
/// carries no `package.jet` of its own (Package facts live entirely in member
/// directories). Only when there's no workspace, or it has zero/one runnable
/// member, does resolution fall back to the ordinary `find_manifest_root` +
/// `find_project_entry` single-package convention (unchanged from before
/// D-CLI-BARE1). Returns `None` outside any package or workspace — the
/// caller keeps today's "no file given" usage error verbatim.
fn resolve_bare_entry(cmd: &str, cwd: &Path, member_flag: Option<&str>) -> Option<PathBuf> {
    let workspace_resolver = match jet::Authority::AuthorityResolver::open(cwd) {
        Ok(resolver) => Some(resolver),
        Err(error) if error.is_missing() => None,
        Err(error) => report_entry_authority_error(error),
    };
    // A workspace lock is a checked member index, not an optional cache. If
    // its source identity is stale while the workspace source is absent, do
    // not fall through to an ordinary package entry and run the wrong file.
    let workspace_source = workspace_resolver.as_ref().and_then(|resolver| {
        match resolver.resolve_workspace_source() {
            Ok(source) => Some(Ok(source)),
            Err(error) => Some(Err(error.workspace_diagnostic())),
        }
    });
    if matches!(workspace_source.as_ref(), None | Some(Ok(None))) {
        let stale_workspace_lock = match workspace_resolver.as_ref() {
            Some(resolver) => match resolver.checked_file(Path::new(jet::Syntax::UNIFIED_LOCK_FILE)) {
                Ok(lock_file) => {
                    let source = match lock_file.text() {
                        Ok(source) => source,
                        Err(error) => report_entry_authority_error(error),
                    };
                    if let Err(error) = resolver.revalidate_file(&lock_file) {
                        report_entry_authority_error(error);
                    }
                    jetpack::WorkspaceLock::load_checked_file(resolver, lock_file).is_none()
                        && jetpack::Lock::looks_like_workspace_lock(&source)
                }
                Err(error) if error.is_missing() => false,
                Err(error) => report_entry_authority_error(error),
            },
            None => false,
        };
        if stale_workspace_lock {
            let lock_path = cwd.join(jet::Syntax::UNIFIED_LOCK_FILE);
            let diagnostic = jetpack::Lock::e1202_workspace(&lock_path.display().to_string());
            eprint!(
                "{}",
                jet::Diagnostics::render_all(
                    jet::Syntax::WORKSPACE_FILE,
                    "",
                    std::slice::from_ref(&diagnostic),
                )
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
    // D-BUILDSCOPE1: the workspace source is the build authority even when it
    // has no `fn build`; the ordinary batteries still build the root after
    // members. Member selection (`-p`) remains an explicit escape.
    if cmd == "build" && member_flag.is_none() {
        if let Some(Ok(Some(source))) = workspace_source.as_ref() {
            if source.role == jetpack::WorkspaceFile::WorkspaceSourceRole::Index
            {
                return Some(source.path.clone());
            }
        }
    }
    let workspace = match (workspace_resolver.as_ref(), workspace_source.as_ref()) {
        (Some(resolver), Some(Ok(Some(source))))
            if source.role == jetpack::WorkspaceFile::WorkspaceSourceRole::Index => Some(
                jetpack::WorkspaceFile::evaluate_checked_source(source, resolver),
            ),
        (_, Some(Err(diagnostic))) => Some(Err(diagnostic.clone())),
        _ => None,
    };
    let canonical_workspace = workspace_source.as_ref().is_some_and(|source| {
        matches!(
            source,
            Ok(Some(source))
                if source.role == jetpack::WorkspaceFile::WorkspaceSourceRole::Index
        )
    });
    if canonical_workspace && workspace.is_none() {
        let diagnostic = Diagnostic::error(
            "E3503",
            format!("workspace source in `{}` disappeared", cwd.display()),
            "workspace authority changed while the entry was being selected".to_string(),
            "restore the workspace declaration before running the command".to_string(),
            None,
        );
        eprint!(
            "{}",
            jet::render_diagnostics(jet::Syntax::WORKSPACE_FILE, "", &[diagnostic])
        );
        exit(ExitCodes::USER_ERROR);
    }
    if let Some(workspace) = workspace {
        let plan = match workspace {
            Ok(plan) => plan,
            Err(diagnostic) => {
                if cmd == "build" && member_flag.is_none() {
                    if let Some(Ok(Some(source))) = workspace_source.as_ref() {
                        if source.role == jetpack::WorkspaceFile::WorkspaceSourceRole::Index {
                            return Some(source.path.clone());
                        }
                    }
                }
                eprint!(
                    "{}",
                    jet::render_diagnostics(
                        jet::Syntax::WORKSPACE_FILE,
                        "",
                        &[diagnostic]
                    )
                );
                exit(ExitCodes::USER_ERROR);
            }
        };
        let runnable: Vec<(String, PathBuf)> = plan
            .members
            .iter()
            .filter_map(|m| {
                let entry = if cmd == "build" {
                    match resolve_member_build_entry(&cwd.join(&m.path)) {
                        Ok(entry) => entry,
                        Err(error) => report_build_resolution_error(error),
                    }
                } else {
                    let entry = find_project_entry(&cwd.join(&m.path));
                    checked_explicit_file(&entry)
                }?;
                Some((m.name.clone(), entry))
            })
            .collect();
        if let Some(want) = member_flag {
            return match runnable.iter().find(|(name, _)| name == want) {
                Some((_, entry)) => Some(entry.clone()),
                None => {
                    let names: Vec<&str> = plan.members.iter().map(|m| m.name.as_str()).collect();
                    if cmd == "jobs" {
                        crate::cli_error!(@fix "E2104", format!("no workspace member named `{want}`"), format!("list jobs for one of: {}", names.join(", ")));
                    } else {
                        crate::cli_error!(@fix "E2104", format!("no workspace member named `{want}`"), format!("pick one of: {}", names.join(", ")));
                    }
                    exit(ExitCodes::USAGE);
                }
            };
        }
        match runnable.len() {
            1 => return Some(runnable.into_iter().next().unwrap().1),
            n if n >= 2 => {
                let names: Vec<&str> = runnable.iter().map(|(n, _)| n.as_str()).collect();
                if cmd == "jobs" {
                    crate::cli_error!(@full "E2104", format!("`jet jobs` is ambiguous — this workspace has {} members with entry files", names.len()), format!("jobs are listed for one member at a time: {}", names.join(", ")), "pick one with `jet jobs -p <member>`");
                    exit(ExitCodes::USAGE);
                }
                crate::cli_error!(@full "E2104", format!("`jet {cmd}` is ambiguous — {} workspace members can run", names.len()), format!("this workspace declares multiple runnable members: {}", names.join(", ")), format!("pick one with `jet {cmd} -p <member>`, or run its file directly"));
                exit(ExitCodes::USAGE);
            }
            _ => {} // no runnable member — fall through to the single-project convention
        }
    }
    match jet::Loader::find_manifest_root_checked(cwd) {
        Ok(Some(root)) => Some(find_project_entry(&root)),
        Ok(None) => None,
        Err(diagnostic) => report_entry_diagnostic(diagnostic),
    }
}

fn report_entry_diagnostic(diagnostic: Diagnostic) -> ! {
    eprint!(
        "{}",
        jet::render_diagnostics(jet::Syntax::WORKSPACE_FILE, "", &[diagnostic])
    );
    exit(ExitCodes::USER_ERROR)
}

fn missing_bare_entry(cmd: &str, cwd: &Path) -> ! {
    // D-JPK-FILENAME2=B (A2): a retired manifest filename in place of
    // `pkg.jet` gets the E1226 teaching diagnostic.
    if let Some(msg) = jet::Loader::stale_manifest_name_message(cwd) {
        eprint!("{}", msg);
        exit(ExitCodes::USAGE);
    }
    crate::cli_error!(@fix "E2104", "no file given and no `package.jet` found in this directory or above", format!("run `jet {} <file.{}>` or cd into a project", cmd, jet::Syntax::FILE_EXT));
    exit(ExitCodes::USAGE);
}

fn run_version() {
    print!("{}", jet::Manifest::version_banner());
}

fn run_upgrade() {
    println!(
        "To upgrade {}, download the latest release from:",
        jet::Syntax::BINARY_NAME
    );
    println!("  https://github.com/jet-lang/jet/releases");
}

fn is_diagnostic_code(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some('E' | 'L'))
        && chars.clone().next().is_some()
        && chars.all(|c| c.is_ascii_digit())
}

/// Print front-end problems in the active output mode, with the trailing
/// "N problems found" count and (in human mode) one quiet `jet explain`
/// pointer naming the first code. Suppressed entirely in `--json`, where the
/// code is already structured.
pub(crate) fn report_problems(
    mode: OutputMode,
    file: &str,
    src: &str,
    diags: &[jet::Diagnostics::Diagnostic],
) {
    if mode.json {
        let machine_file = machine_report_path_for_process(file);
        eprint!("{}", jet::render_all_json(&machine_file, src, diags));
        return;
    }
    eprint!(
        "{}",
        jet::render_all_linked(
            file,
            src,
            diags,
            mode.color_stderr(),
            mode.hyperlinks_stderr()
        )
    );
    let n = diags.len();
    eprintln!("\n{} problem{} found", n, if n == 1 { "" } else { "s" });
    if let Some(first) = diags.first() {
        eprintln!(
            "{}",
            jet::Explain::pointer_line(&first.code, mode.color_stderr())
        );
    }
}

/// Return the one physical path representation used by CLI machine reports.
///
/// Disk-backed source labels are absolute, so an agent can open them without
/// guessing the package root. Synthetic labels stay labels: they do not name
/// files and must not become host-specific paths in stable output.
pub(crate) fn machine_report_path_for_process(file: &str) -> String {
    if file.is_empty() || file.starts_with('<') {
        return file.to_string();
    }
    machine_report_path_from_path(Path::new(file))
}

/// Resolve a loader display label against the entry's package/workspace root.
/// Loader displays are intentionally human-friendly and root-relative; JSON
/// reports need the corresponding physical path instead.
pub(crate) fn machine_report_path_for_entry(entry_file: &str, display: &str) -> String {
    if display.is_empty() || display.starts_with('<') || Path::new(display).is_absolute() {
        return display.to_string();
    }
    let entry = machine_report_path_from_path(Path::new(entry_file));
    let entry = Path::new(&entry);
    let base = entry.parent().unwrap_or_else(|| Path::new("."));
    let root = jet::Loader::find_manifest_root(base).unwrap_or_else(|| base.to_path_buf());
    machine_report_path_from_path(&root.join(display))
}

/// Map a loaded module's display label back to its physical source path.
/// Human renderers keep the display label; only JSON report construction uses
/// this mapping.
pub(crate) fn machine_report_path_for_bundle(
    bundle: &jet::AST::ProgramBundle,
    display: &str,
) -> String {
    if display.is_empty() || display.starts_with('<') || Path::new(display).is_absolute() {
        return display.to_string();
    }
    let path = bundle
        .modules
        .iter()
        .find(|module| module.display == display)
        .map(|module| module.path.clone())
        .unwrap_or_else(|| bundle.project_root.join(display));
    machine_report_path_from_path(&path)
}

fn machine_report_path_from_path(path: &Path) -> String {
    let display = path.to_string_lossy();
    if display.starts_with('<') {
        return display.into_owned();
    }
    if path.is_absolute() {
        display.into_owned()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path).to_string_lossy().into_owned())
            .unwrap_or_else(|_| display.into_owned())
    }
}

/// Render a `jet`-owned toolchain diagnostic (E1249–E1252) in the standard
/// teaching voice (docs/spec/diagnostics.md), matching the engine-dispatch
/// diagnostics. These carry no source span, so the full linked renderer isn't
/// used.
fn print_toolchain_diag(d: &jet::Diagnostics::Diagnostic) {
    eprintln!("Error [{}]: {}", d.code, d.what);
    eprintln!(" Why: {}", d.why);
    eprintln!(" Fix: {}", d.fix);
}

/// D-JPK-TOOLCHAIN1=A (#179): hand off to the project's pinned `jet` toolchain
/// when the running compiler doesn't satisfy the pin. Returns normally when the
/// running `jet` should run the verb itself (unpinned, in-channel, or already
/// the exec'd pinned child).
fn maybe_dispatch_pinned_toolchain(raw: &[String]) {
    use jetpack::JetPin::{decide, handoff_line, PinDecision};
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Some(root) = jet::Loader::find_manifest_root(&cwd) else {
        return;
    };
    // `--offline`/`--locked` forbid resolving an unlocked channel (E1250).
    let offline = raw.iter().any(|a| a == "--offline" || a == "--locked");
    let running = env!("CARGO_PKG_VERSION");
    match decide(&root, running, offline) {
        PinDecision::RunNative => {}
        PinDecision::Report(d) => {
            print_toolchain_diag(&d);
            exit(ExitCodes::USER_ERROR);
        }
        PinDecision::ReExec {
            binary,
            channel,
            version,
        } => {
            eprintln!("{}", handoff_line(&channel, &version, running));
            let status = Command::new(&binary)
                .args(raw.iter().map(|s| s.as_str()))
                .env(jet::Syntax::TOOLCHAIN_EXEC_MARKER_ENV, &version)
                .status()
                .unwrap_or_else(|e| {
                    crate::cli_error!("E2105", "couldn't exec the pinned toolchain `{}`: {}", binary.display(), e);
                    exit(ExitCodes::USER_ERROR);
                });
            exit(status.code().unwrap_or(ExitCodes::USER_ERROR));
        }
    }
}

/// `jet self toolchain` — print the project's pin, locked version, object id, and
/// realized state (read-only, D-JPK-TOOLCHAIN1=A #179).
fn run_toolchain() -> ! {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = require_manifest_root(
        &cwd,
        "error: `jet self toolchain` needs a project — no `package.jet` found here or above",
    );
    print!("{}", jetpack::JetPin::report_pin(&root));
    exit(ExitCodes::OK);
}

/// `jet init` — write a `package.jet` here, pinning the running toolchain's channel
/// (D-JPK-TOOLCHAIN1=A #179, U11 lift).
/// `jet init [<script.jet>]` — U11 (D-JPK-SCRIPTDEP1=A): when a manifest-less
/// script is named, its inline `use pkg#version;` refs are lifted into the
/// freshly written `package.jet`'s `deps: {}` block (rung 0 → rung 1, per
/// docs/plans/epoch-4/vision.md). Lifting is best-effort: a lex/
/// parse problem in the script is silently skipped here (`jet check`/`jet
/// run` on the script itself is where that's diagnosed) so `jet init` never
/// fails just because the *lift* half had nothing to do.
fn run_split(args: &[&String], raw: &[String], mode: OutputMode) -> ! {
    let target = match args.get(1).map(|value| value.as_str()) {
        Some("env") => jetpack::Transition::SplitTarget::Environment,
        Some("package") => {
            let Some(name) = args.get(2) else {
                crate::cli_error!("E2104", "jet split package needs a Package name");
                exit(ExitCodes::USAGE);
            };
            jetpack::Transition::SplitTarget::Package {
                name: (*name).clone(),
            }
        }
        Some("hosts") => {
            let Some(name) = args.get(2) else {
                crate::cli_error!("E2104", "jet split hosts needs a host name");
                exit(ExitCodes::USAGE);
            };
            jetpack::Transition::SplitTarget::Hosts {
                name: (*name).clone(),
            }
        }
        Some(other) => {
            crate::cli_error!(@fix "E2101", format!("unknown split target {other}"), "use jet split env, jet split package <name>, or jet split hosts <name>");
            exit(ExitCodes::USAGE);
        }
        None => {
            crate::cli_error!("E2104", "jet split needs a target");
            exit(ExitCodes::USAGE);
        }
    };
    let check_only = raw.iter().any(|arg| arg == "--check");
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match jetpack::Transition::split(&cwd, target, check_only) {
        Ok(result) => print_transition_result(&result, check_only, mode),
        Err(error) => report_transition_error(&error),
    }
}

fn run_fold(args: &[&String], raw: &[String], mode: OutputMode) -> ! {
    let Some(path) = args.get(1) else {
        crate::cli_error!(@fix "E2104", "jet fold needs a generated transition path", "use jet fold package/env.jet or jet fold packages/name/package.jet");
        exit(ExitCodes::USAGE);
    };
    let check_only = raw.iter().any(|arg| arg == "--check");
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match jetpack::Transition::fold(&cwd, Path::new(path), check_only) {
        Ok(result) => print_transition_result(&result, check_only, mode),
        Err(error) => report_transition_error(&error),
    }
}

fn print_transition_result(
    result: &jetpack::Transition::TransitionResult,
    check_only: bool,
    mode: OutputMode,
) -> ! {
    if mode.json {
        let changes = result
            .summary
            .changes
            .iter()
            .map(|change| {
                format!(
                    "{{\"path\":{},\"action\":{}}}",
                    json_quote(&change.path.to_string_lossy()),
                    json_quote(change.action)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"operation\":{},\"check\":{},\"before\":{},\"after\":{},\"journal\":{},\"changes\":[{}]}}",
            json_quote(&result.summary.operation),
            check_only,
            json_quote(&result.summary.before_fingerprint),
            json_quote(&result.summary.after_fingerprint),
            json_quote(&result.summary.journal.to_string_lossy()),
            changes
        );
    } else if !mode.quiet {
        for change in &result.summary.changes {
            if check_only {
                println!("Would {}: {}", change.action, change.path.display());
            } else {
                println!("{}d {}.", change.action, change.path.display());
            }
        }
        if result.summary.before_fingerprint == result.summary.after_fingerprint {
            println!("package graph unchanged: {}", result.summary.after_fingerprint);
        } else {
            println!("package graph before: {}", result.summary.before_fingerprint);
            println!("package graph after: {}", result.summary.after_fingerprint);
        }
        if check_only {
            println!("No files changed.");
        } else {
            println!("Transition journal: {}", result.summary.journal.display());
        }
    }
    exit(ExitCodes::OK)
}

fn report_transition_error(error: &jetpack::Transition::TransitionError) -> ! {
    let message = error.0.as_str();
    let (code, why, fix) = if message.contains("pkg.jet") || message.contains("retired") {
        (
            "E1226",
            "the package transition accepts only the current package manifest and its recorded migration path.",
            "rename the retired manifest to `package.jet`, or run `jet init --check` in the project root.",
        )
    } else if message.contains("already exists") {
        (
            "E1252",
            "a transition never overwrites an existing package manifest or generated role file.",
            "review the existing file, or run the transition in an empty package root.",
        )
    } else if message.contains("journal") || message.contains("fingerprint") {
        (
            "E1204",
            "a recorded package transition no longer matches the files on disk, so Jet refuses a stale or tampered replay.",
            "restore the recorded files or remove the stale transition journal and make a fresh checked transition.",
        )
    } else {
        (
            "E1206",
            "the package manifest or transition input does not have the required typed shape.",
            "fix the named package or role file, then rerun the transition with `--check` first.",
        )
    };
    eprintln!("Error [{code}]: {message}\n Why: {why}\n Fix: {fix}");
    exit(ExitCodes::USER_ERROR);
}

fn json_quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

fn run_init(
    script: Option<&str>,
    raw: &[String],
    mode: OutputMode,
) -> ! {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if raw.iter().any(|arg| arg == "--restore-role-files") {
        let check_only = raw.iter().any(|arg| arg == "--check");
        match jetpack::Transition::restore_role_files(&cwd, check_only) {
            Ok(result) => print_transition_result(&result, check_only, mode),
            Err(error) => {
                crate::cli_error!("E2105", "{error}");
                exit(ExitCodes::USER_ERROR);
            }
        }
    }
    if raw.iter().any(|arg| arg == "--check") {
        let has_role_file = ["pkg.jet", "env.jet", "workspace.jet", "config.jet"]
            .iter()
            .any(|name| cwd.join(name).is_file());
        if has_role_file {
            match jetpack::Transition::init(&cwd, true) {
                Ok(result) => print_transition_result(&result, true, mode),
                Err(error) => {
                    crate::cli_error!("E2105", "{error}");
                    exit(ExitCodes::USER_ERROR);
                }
            }
        }
        if !mode.quiet {
            println!("No migration-era role files found.\nNo files changed.");
        }
        exit(ExitCodes::OK);
    }
    if script.is_none()
        && ["pkg.jet", "env.jet", "workspace.jet", "config.jet"]
            .iter()
            .any(|name| cwd.join(name).is_file())
        && !cwd.join(jet::Syntax::PACKAGE_FILE).is_file()
    {
            match jetpack::Transition::init(&cwd, false) {
            Ok(result) => print_transition_result(&result, false, mode),
            Err(error) => {
                crate::cli_error!("E2105", "{error}");
                exit(ExitCodes::USER_ERROR);
            }
        }
    }
    let name = cwd
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("app")
        .to_string();
    match jetpack::JetPin::write_init(&cwd, &name, env!("CARGO_PKG_VERSION")) {
        Ok(msg) => {
            if let Some(script) = script {
                lift_inline_deps_into_manifest(&cwd, script);
            }
            if !mode.quiet {
                println!("{msg}");
            }
            exit(ExitCodes::OK);
        }
        Err(d) => {
            print_toolchain_diag(&d);
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// U11: fold `script`'s inline deps into `<cwd>/package.jet`'s `deps: {}` block
/// (just written by `write_init`), preserving comments/formatting via the
/// same comment-preserving editor `jet add` uses.
fn lift_inline_deps_into_manifest(cwd: &Path, script: &str) {
    let script_path = resolve_source_path(script);
    let Ok(src) = fs::read_to_string(&script_path) else {
        return;
    };
    let (toks, lex_diags) = jet::Lexer::lex(&src);
    if !lex_diags.is_empty() {
        return;
    }
    let Ok(prog) = jet::Parser::parse(&toks) else {
        return;
    };
    let deps = jet::ScriptDeps::collect(&prog);
    if deps.is_empty() {
        return;
    }
    let Some(manifest_path) = jet::Loader::manifest_path(cwd) else {
        return;
    };
    let Ok(mut raw) = fs::read_to_string(&manifest_path) else {
        return;
    };
    for dep in &deps {
        raw = jet::Manifest::add_dependency(
            &raw,
            &dep.name,
            &jet::Manifest::DepSpec::Registry(dep.selector.clone()),
        );
    }
    if fs::write(&manifest_path, raw).is_ok() {
        println!(
            "lifted {} inline dependenc{} into {}",
            deps.len(),
            if deps.len() == 1 { "y" } else { "ies" },
            manifest_path.file_name().and_then(|name| name.to_str()).unwrap_or("package.jet")
        );
    }
}

/// `jet fetch --lock <script.jet>` (D-CLI-STORE2=A, was `jet store lock` /
/// `jet lock`) — U11: resolve a manifest-less script's inline
/// `use pkg#version;` deps and write the `<script.jet>.lock` sidecar,
/// keyed by the script's own content hash (edit the script, the lock goes
/// stale — the same "locks by file-content hash" contract `jet run` uses).
fn run_lock(script: Option<&str>, mode: OutputMode) {
    let Some(raw_arg) = script else {
        crate::cli_error!("E2104", "`jet fetch --lock` needs a script path, e.g. `jet fetch --lock stats.jet`");
        exit(ExitCodes::USER_ERROR);
    };
    let file = resolve_source_path(raw_arg);
    let script_path = Path::new(&file);
    let script_dir = script_path.parent().unwrap_or(Path::new("."));

    if jet::Loader::find_manifest_root(script_dir).is_some() {
        crate::cli_error!("E1202", "`{file}` belongs to a project with a `{}` — use `jet fetch` to lock its dependencies", jet::Syntax::PACKAGE_FILE);
        exit(ExitCodes::USER_ERROR);
    }

    let src = match fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) => {
            crate::cli_error!("E2105", "couldn't read `{file}`: {e}");
            exit(ExitCodes::USER_ERROR);
        }
    };
    let (toks, lex_diags) = jet::Lexer::lex(&src);
    if !lex_diags.is_empty() {
        report_problems(mode, &file, &src, &lex_diags);
        exit(ExitCodes::USER_ERROR);
    }
    let prog = match jet::Parser::parse(&toks) {
        Ok(p) => p,
        Err(diags) => {
            report_problems(mode, &file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };

    let deps = jet::ScriptDeps::collect(&prog);
    let mut locked = Vec::new();
    for dep in &deps {
        match jet::ScriptDeps::resolve(dep, script_dir) {
            Ok(r) => locked.push(jetpack::ScriptLock::LockedInlineDep {
                name: r.name,
                selector: r.selector,
                resolved: r.resolved_version,
                content_hash: r.content_hash,
            }),
            Err(reason) => {
                let d = jet::ScriptDeps::e1253(dep, &reason);
                report_problems(mode, &file, &src, &[d]);
                exit(ExitCodes::USER_ERROR);
            }
        }
    }

    let script_hash = jet::ScriptDeps::file_hash(script_path).unwrap_or_default();
    let lock = jetpack::ScriptLock::ScriptLockFile {
        version: jetpack::ScriptLock::SCRIPT_LOCK_VERSION,
        script_hash,
        deps: locked,
    };
    match jetpack::ScriptLock::write(script_path, &lock) {
        Ok(()) => {
            if !mode.quiet {
                println!(
                    "wrote {}",
                    jetpack::ScriptLock::sidecar_path(script_path).display()
                );
            }
            exit(ExitCodes::OK);
        }
        Err(e) => {
            crate::cli_error!("E2105", "couldn't write the lock sidecar: {e}");
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// `jet update jet [<channel>]` — move the toolchain pin (D-JPK-TOOLCHAIN1=A
/// #179). The only place the pin moves.
fn run_update_jet(channel: Option<&str>) -> ! {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = require_manifest_root(
        &cwd,
        "error: `jet update jet` needs a project — no `package.jet` found here or above",
    );
    match jetpack::JetPin::move_pin(&root, channel, env!("CARGO_PKG_VERSION")) {
        Ok(msg) => {
            println!("{msg}");
            exit(ExitCodes::OK);
        }
        Err(d) => {
            print_toolchain_diag(&d);
            exit(ExitCodes::USER_ERROR);
        }
    }
}

pub(crate) fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == flag {
            return iter.next().map(String::as_str);
        }
    }
    None
}

/// Find the manifest root from `cwd`, or exit. D-JPK-FILENAME2=B (A2): when
/// there's no `package.jet` but a retired filename (`pkg.jet`/`pack.jet`/
/// `payload.jet`/`jet.toml`) sits where it belongs, teaches E1226 instead of
/// the generic "no package.jet found" — `fallback_hint` is that generic
/// message's body for
/// commands that genuinely have no manifest at all.
pub(crate) fn require_manifest_root(cwd: &Path, fallback_hint: &str) -> PathBuf {
    jet::Loader::find_manifest_root(cwd).unwrap_or_else(|| {
        match jet::Loader::stale_manifest_name_message(cwd) {
            Some(msg) => eprint!("{}", msg),
            None => eprintln!("{}", fallback_hint),
        }
        exit(ExitCodes::USER_ERROR);
    })
}
