//! jet CLI: check / build / run / test / new / fmt / try / lsp +
//!          add / remove / fetch / update (M12.1 package manager).
//!
//! The driver owns invariant I2: rustc's voice never reaches the user as
//! if it were their fault. A rustc failure on generated code is reported
//! as an internal compiler error in jet.

// Source files/modules use PascalCase names (owner decision).
#![allow(non_snake_case)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use jet::Diagnostics::{Diagnostic, ReportPath};
use jet::ExitCodes;
pub(crate) use jet::{Diagnostics, Syntax, SHA256};
use jet_foundation::BuildEffect;
use jet_foundation::Report::{
    render_status, render_status_with_reports, StatusFields, StatusValue,
};

// D-ALLOC-PROGRAM1=A: the CLI executable installs the resident instance once.
// JIT/interpreter entry points only marshal the checked package fact into the
// same Prelude kernel; they never implement allocation policy.
#[global_allocator]
static JET_HOST_ALLOCATOR: jet::program_allocator::JetHostProgramAllocator =
    jet::program_allocator::JetHostProgramAllocator;

mod CmdBackendScaffold;
mod CmdBudget;
mod CmdCodemod;
mod CmdCompile;
mod CmdConsoleHost;
mod CmdDb;
mod CmdDevTools;
mod CmdDoc;
mod CmdDossier;
mod CmdExec;
mod CmdExpand;
mod CmdFill;
mod CmdFlash;
mod CmdGame;
mod CmdGates;
mod CmdGc;
mod CmdImpact;
mod CmdImport;
mod CmdInspect;
mod CmdLearn;
mod CmdLocalApp;
mod CmdMemory;
mod CmdMigration;
mod CmdNotebook;
mod CmdPackage;
mod CmdPerf;
mod CmdPkg;
mod CmdProve;
mod CmdRemote;
mod CmdReview;
mod CmdSchema;
mod CmdSemIndex;
mod CmdStatus;
mod CmdStructuralMerge;
mod CmdStructure;
mod CmdSupply;
mod CmdTest;
mod CmdTry;
mod EngineDispatch;
mod NativeLinker;
mod OutputAdapter;
mod ProductionReceipt;
mod ProveReplay;
mod ProveSolver;
#[allow(dead_code)]
mod Store;
use OutputAdapter::{
    active_profile, write_machine, write_mode_diagnostic, write_mode_machine,
    write_mode_renderable, write_mode_status, write_renderable, write_status,
};
pub(crate) use OutputAdapter::{OutputAdapter as OutputAdapterHost, OutputMode};

use CmdBackendScaffold::run_new_backend;
use CmdCodemod::run_generate;
use CmdCompile::{
    prepare_project_environment, project_environment_requirement, require_project_environment,
    resolve_named_profile, run_build_query, run_compiler_api, run_debug_native, run_dev_entry,
    run_dev_web, run_external_fmt, run_fix, run_fmt, run_fuzz, run_native_execution,
    run_native_source_from_stdin, run_new, run_test_opts, run_web_app_dev_entry,
    scaffold_browser_tests, validate_target, FuzzRunOpts, NativeExecutionRequest, TestRunOpts,
};
use CmdConsoleHost::ConsoleHost;
use CmdDb::run_db;
use CmdDevTools::{
    run_bind, run_completions, run_dev, run_devtools, run_doctor, run_emit_rust, run_eval,
    run_eval_expression, run_explain, run_explain_cost, run_explain_marker, run_explain_web_graph,
    run_lint_a11y, run_lint_complexity, run_lint_cost, run_repl, watch_policy_from, WatchPolicy,
};
use CmdDoc::run_doc;
use CmdDossier::run_module_explain;
use CmdExec::run_exec;
use CmdExpand::run_expand;
use CmdFill::run_fill;
use CmdFlash::run_flash;
use CmdImpact::run_impact;
use CmdInspect::{run_digest, run_env, run_inspect, run_output, run_provenance};
use CmdLearn::run as run_learn;
use CmdPackage::run_package;
use CmdPkg::{run_add, run_fetch, run_remove, run_update};
use CmdProve::run_prove;
use CmdRemote::run_remote;
use CmdReview::run_review;
use CmdSchema::run_schema;
use CmdSemIndex::{run_find, run_semindex};
use CmdStructuralMerge::{run_diff, run_merge, structural_help};
use CmdSupply::{
    run_audit, run_copy_audit, run_key_backup, run_keygen, run_publish, run_sbom, run_vendor,
    run_yank,
};
use CmdTry::run_try;

/// Render one spanless driver failure through Jet's shared diagnostic frame.
fn cli_diagnostic_copy(code: &str) -> (&'static str, &'static str) {
    match code {
        "E1202" => (
            "the lockfile must contain the exact dependency graph before a locked or supply-chain command can run",
            "run `jet fetch`, then run the command again",
        ),
        "E1235" => (
            "the registry endpoint contains credentials or URL parameters, or its git operation failed because of network, authentication, or local-clone state; rendered endpoint details are redacted",
            "remove URL credentials and parameters, configure the host Git credential provider for the registry path, then check network access",
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

pub(crate) fn emit_cli_report(code: &str, what: String, why: String, fix: String, json: bool) {
    emit_cli_report_for_action("cli", code, what, why, fix, json);
}

/// Render a command-owned diagnostic with the command action in the shared
/// status envelope. Callers that are not one of the named command producers
/// keep the `cli` action through `emit_cli_report`.
pub(crate) fn emit_cli_report_for_action(
    action: &str,
    code: &str,
    what: String,
    why: String,
    fix: String,
    json: bool,
) {
    let diagnostic = jet::Diagnostics::Diagnostic::error(code, what, why, fix, None);
    emit_cli_value_for_action(diagnostic, json, action);
}

fn emit_cli_value(diagnostic: Diagnostic, json: bool) {
    emit_cli_value_for_action(diagnostic, json, "cli");
}

fn emit_cli_value_for_action(diagnostic: Diagnostic, json: bool, action: &str) {
    let profile = active_profile();
    if json || profile.is_some_and(|profile| profile.machine_enabled()) {
        let report_file = ReportPath::from_process("");
        let report = diagnostic.to_report(&report_file, "");
        let rendered =
            render_status_with_reports(action, false, std::iter::once(report), StatusFields::new());
        let rendered = format!("{rendered}\n");
        if let Some(profile) = profile.filter(|profile| profile.machine_enabled()) {
            write_machine(profile, &rendered);
        } else {
            write_mode_machine(
                OutputMode {
                    json: true,
                    color: jet::Diagnostics::ColorChoice::Never,
                    quiet: false,
                },
                &rendered,
            );
        }
    } else {
        let color = profile.is_some_and(|profile| profile.ansi_enabled());
        let rendered = jet::render_all_colored("", "", &[diagnostic], color);
        if let Some(profile) = profile {
            write_status(profile, &rendered);
        } else {
            write_mode_diagnostic(
                OutputMode {
                    json: false,
                    color: jet::Diagnostics::ColorChoice::Never,
                    quiet: false,
                },
                &rendered,
            );
        }
    }
}

fn emit_cli_diagnostics(file: &str, source: &str, diagnostics: &[Diagnostic]) {
    let profile = active_profile();
    if profile.is_some_and(|profile| profile.machine_enabled()) {
        let machine_file = machine_report_path_for_process(file);
        let clears = jet::Diagnostics::report_clear_counts(diagnostics);
        let reports = diagnostics.iter().zip(clears).map(|(diagnostic, clears)| {
            diagnostic.to_report_with_clears(&machine_file, source, clears)
        });
        let rendered =
            render_status_with_reports("diagnostics", false, reports, StatusFields::new());
        let rendered = format!("{rendered}\n");
        if let Some(profile) = profile {
            write_machine(profile, &rendered);
        } else {
            write_mode_machine(
                OutputMode {
                    json: true,
                    color: jet::Diagnostics::ColorChoice::Never,
                    quiet: false,
                },
                &rendered,
            );
        }
    } else {
        let color = profile.is_some_and(|profile| profile.ansi_enabled());
        let rendered = jet::render_all_colored(file, source, diagnostics, color);
        if let Some(profile) = profile {
            write_status(profile, &rendered);
        } else {
            write_mode_diagnostic(
                OutputMode {
                    json: false,
                    color: jet::Diagnostics::ColorChoice::Never,
                    quiet: false,
                },
                &rendered,
            );
        }
    }
}

pub(crate) fn emit_cli_row(code: &str, holes: &[(&str, &str)], json: bool) {
    let diagnostic = jet::Diagnostics::Diagnostic::from_row(code, holes, None);
    emit_cli_value(diagnostic, json);
}

pub(crate) fn emit_cli_row_with_detail(
    code: &str,
    holes: &[(&str, &str)],
    detail: String,
    json: bool,
) {
    let diagnostic = jet::Diagnostics::Diagnostic::from_row(code, holes, None).with_detail(detail);
    emit_cli_value(diagnostic, json);
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

/// Offer the existing Jetpack environment boundary when an interactive
/// `jet dev` would otherwise be refused. The child inherits stdio, so its
/// resolver progress and any realization diagnostic stay in this terminal.
fn offer_dev_environment(raw: &[String], file: &str, output: &OutputAdapterHost) {
    let Some((environment, import)) = project_environment_requirement(Path::new(file)) else {
        return;
    };
    if output.profile().machine_enabled()
        || !output.stdin_is_terminal()
        || !output.stderr_is_terminal()
    {
        return;
    }

    write_mode_diagnostic(
        output.mode(),
        &format!("jet dev needs {environment} for `use {import}`.\nRealize it now? [Y/n]\n"),
    );
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    let Ok(read) = std::io::stdin().read_line(&mut answer) else {
        return;
    };
    if read == 0
        || (!answer.trim().is_empty()
            && !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
    {
        return;
    }

    let Ok(jet_binary) = std::env::current_exe() else {
        return;
    };
    let mut forwarded = Vec::with_capacity(raw.len() + 2);
    forwarded.push("env".to_string());
    forwarded.push("--".to_string());
    forwarded.push(jet_binary.to_string_lossy().into_owned());
    forwarded.extend(raw.iter().cloned());
    exit(EngineDispatch::dispatch(
        jet::Syntax::JETPACK_BINARY_NAME,
        "env",
        &forwarded,
    ));
}

/// D-BUILDPROFILE1 (ratified 2026-06-25): the optimization level carried by a
/// named build profile. Three levels mapping directly to rustc opt-level values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptimizeLevel {
    /// `optimize: none` — rustc opt-level=0; fast compile, no optimization.
    None,
    /// `optimize: basic` — rustc opt-level=2; the driver default.
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
    pub codegen_units: Option<u16>,
    pub small: bool,
    pub panic_abort: bool,
    pub inspect: jet::Package::Blocks::ReleaseInspect,
    pub settings: BTreeMap<String, String>,
}

impl ProfileConfig {
    pub(crate) fn release() -> Self {
        Self {
            optimize: OptimizeLevel::Full,
            debug_info: false,
            codegen_units: None,
            small: false,
            panic_abort: false,
            settings: BTreeMap::new(),
            inspect: Default::default(),
        }
    }

    pub(crate) fn debug() -> Self {
        Self {
            optimize: OptimizeLevel::None,
            debug_info: true,
            codegen_units: Some(256),
            small: false,
            panic_abort: false,
            settings: BTreeMap::new(),
            inspect: Default::default(),
        }
    }

    pub(crate) fn ci() -> Self {
        Self {
            optimize: OptimizeLevel::Basic,
            debug_info: true,
            codegen_units: None,
            small: false,
            panic_abort: false,
            settings: BTreeMap::new(),
            inspect: Default::default(),
        }
    }

    pub(crate) fn from_def(def: &jet::Package::BuildProfileDef) -> Self {
        use jet::Package::BuildPanic;
        Self {
            optimize: OptimizeLevel::from(def.optimize),
            debug_info: def.debug_info,
            codegen_units: None,
            small: def.small,
            panic_abort: matches!(def.panic, Some(BuildPanic::Abort)),
            settings: def.settings.clone(),
            inspect: def.inspect,
        }
    }

    /// Cache-key suffix for user-defined profiles — encodes every flag that
    /// affects the emitted binary.
    pub(crate) fn settings_tag(&self) -> String {
        let mut parts = vec![self.optimize.cache_tag().to_string()];
        parts.push(format!("inspect:{}", self.inspect.cfg_value()));
        if self.debug_info {
            parts.push("dbg".into());
        }
        if self.small {
            parts.push("small".into());
        }
        if self.panic_abort {
            parts.push("panic=abort".into());
        }
        if let Some(units) = self.codegen_units {
            parts.push(format!("codegen-units={units}"));
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

    #[cfg(test)]
    pub(crate) fn rustc_args(&self, ffi: bool) -> Vec<String> {
        self.rustc_args_for_target(ffi, false)
    }

    /// D-SIMD3=B: native AOT builds opt into the host CPU so LLVM can use its
    /// available vector instructions. Cross-target builds keep the portable
    /// baseline; `#Scalar` is the per-function opt-out boundary.
    pub(crate) fn rustc_args_for_target(&self, ffi: bool, native: bool) -> Vec<String> {
        let mut args = Vec::new();
        if self.small {
            args.extend(
                [
                    "-C",
                    "opt-level=z",
                    "-C",
                    "panic=abort",
                    "-C",
                    "strip=symbols",
                ]
                .into_iter()
                .map(str::to_string),
            );
            if !ffi {
                args.extend(["-C".to_string(), "lto=fat".to_string()]);
            }
            if native {
                args.extend(["-C".to_string(), "target-cpu=native".to_string()]);
            }
            return args;
        }
        if let Some(units) = self.codegen_units {
            args.extend(["-C".to_string(), format!("codegen-units={units}")]);
        }
        match self.optimize {
            OptimizeLevel::None => {
                args.extend([
                    "-C".to_string(),
                    "opt-level=0".to_string(),
                    "-C".to_string(),
                    "lto=off".to_string(),
                ]);
            }
            OptimizeLevel::Basic => {
                args.extend(["-C".to_string(), "opt-level=2".to_string()]);
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
        if native {
            args.extend(["-C".to_string(), "target-cpu=native".to_string()]);
        }
        args
    }
}

#[derive(Clone)]
pub(crate) enum BuildProfile {
    /// Default `jet build` profile: optimized (opt-level=2, thin LTO).
    Default,
    /// D-BUILD-DEFAULT1: fast `jet run`/`jet dev` profile.
    Fast,
    /// D-BUILDPROFILE1: `--release` / `--profile=release`. Full optimization.
    Release,
    /// D-MEM-SENTRY1: release optimization with runtime sentries enabled at
    /// every audited memory boundary.
    Hardened,
    /// D-BUILDPROFILE1: `--profile=debug`. No optimization.
    Debug,
    /// D-BUILDPROFILE1: `--profile=ci`. Optimized with debug symbols for CI.
    Ci,
    /// D-BUILDPROFILE1: user-defined or manifest-overridden profile settings.
    Named { name: String, config: ProfileConfig },
    /// S15: size-oriented (`opt-level=z`, fat LTO, `panic=abort`).
    Small,
    /// E2-M15: typed no-OS / embedded profile — only the Core layer; `panic=abort`.
    NoOs,
}

impl BuildProfile {
    /// D-BUILD-DEFAULT1: command defaults are explicit, never ambient.
    pub(crate) fn default_for_command(command: &str) -> Self {
        match command {
            "run" | "dev" => BuildProfile::Fast,
            _ => BuildProfile::Default,
        }
    }

    pub(crate) fn is_release(&self) -> bool {
        match self {
            BuildProfile::Release | BuildProfile::Hardened => true,
            BuildProfile::Named { name, .. } => name == Syntax::BUILD_PROFILE_RELEASE,
            _ => false,
        }
    }

    pub(crate) fn release_inspect(&self) -> Option<jet::Package::Blocks::ReleaseInspect> {
        self.is_release().then(|| self.config().inspect)
    }

    /// Ratified performance-budget applicability name. Default builds retain
    /// the existing `dev` profile identity; named profiles use their declared
    /// name, never their cache-settings suffix.
    pub(crate) fn budget_name(&self) -> &str {
        match self {
            BuildProfile::Default => "dev",
            BuildProfile::Fast => "dev",
            BuildProfile::Release => "release",
            BuildProfile::Hardened => "hardened",
            BuildProfile::Debug => "debug",
            BuildProfile::Ci => "ci",
            BuildProfile::Named { name, .. } => name,
            BuildProfile::Small => "small",
            BuildProfile::NoOs => "no-os",
        }
    }

    pub(crate) fn cache_tag(&self) -> String {
        match self {
            BuildProfile::Default => "default".to_string(),
            BuildProfile::Fast => "fast".to_string(),
            BuildProfile::Release => "release".to_string(),
            BuildProfile::Hardened => "hardened".to_string(),
            BuildProfile::Debug => "debug".to_string(),
            BuildProfile::Ci => "ci".to_string(),
            BuildProfile::Named { name, config } => {
                format!("profile:{name};{}", config.settings_tag())
            }
            BuildProfile::Small => "small".to_string(),
            BuildProfile::NoOs => "no-os".to_string(),
        }
    }

    pub(crate) fn config(&self) -> ProfileConfig {
        match self {
            BuildProfile::Default => ProfileConfig {
                optimize: OptimizeLevel::Basic,
                debug_info: false,
                codegen_units: None,
                small: false,
                panic_abort: false,
                settings: BTreeMap::new(),
                inspect: Default::default(),
            },
            BuildProfile::Fast => ProfileConfig {
                optimize: OptimizeLevel::None,
                debug_info: false,
                codegen_units: Some(256),
                small: false,
                panic_abort: false,
                settings: BTreeMap::new(),
                inspect: Default::default(),
            },
            BuildProfile::Release => ProfileConfig::release(),
            BuildProfile::Hardened => ProfileConfig::release(),
            BuildProfile::Debug => ProfileConfig::debug(),
            BuildProfile::Ci => ProfileConfig::ci(),
            BuildProfile::Named { config, .. } => config.clone(),
            BuildProfile::Small => ProfileConfig {
                optimize: OptimizeLevel::Basic,
                debug_info: false,
                codegen_units: None,
                small: true,
                panic_abort: true,
                settings: BTreeMap::new(),
                inspect: Default::default(),
            },
            BuildProfile::NoOs => ProfileConfig {
                optimize: OptimizeLevel::Basic,
                debug_info: false,
                codegen_units: None,
                small: true,
                panic_abort: true,
                settings: BTreeMap::new(),
                inspect: Default::default(),
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
    // (`os`, `env`, or `db`) — exhaustive groups are handled by normalization.
    if let Some(group) = jet::CLI::command_group(cmd) {
        let mut output = format!(
            "{bin} {cmd} — {}\n\n{}",
            group.summary,
            jet::CLI::command_group_usage(cmd)
        );
        if cmd == "db" {
            output.push_str("\nFlags:\n");
            for (long, help) in jet::CLI::flags_for_command(cmd) {
                output.push_str(&format!("  {long:<28} {help}\n"));
            }
        }
        return output;
    }
    // A normalized nested-action dispatch word (`publish`, `graph`, …).
    if let Some((group, action)) = jet::CLI::moved_command(cmd) {
        let mut lines = String::new();
        for usage_line in action.usage.lines() {
            lines.push_str(&format!("  {bin} {} {}\n", group.name, usage_line));
        }
        return format!(
            "{bin} {} {cmd} — {}\n\n{lines}",
            group.name,
            jet::CLI::action_help_summary(group.name, action)
        );
    }
    // A canonical flat top-level command: summary, usage, and flags all come
    // from the live registry. No parsing of a second usage string.
    let Some(command) = jet::CLI::COMMANDS
        .iter()
        .find(|command| command.name == cmd)
    else {
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
/// to `jetpack use tool@nixpkgs -- tool`, preserving the offline fixture flags
/// used by the same provider path and forwarding user args after `--`.
fn dispatch_nixpkgs_run(raw: &[String], target: &str, sep: Option<usize>) -> Option<i32> {
    let (package, source) = target.rsplit_once(jet::Syntax::REF_PROVIDER_AT)?;
    if source != jet::Syntax::REF_SOURCE_NIXPKGS || package.is_empty() {
        return None;
    }

    let before_sep = sep.map_or(raw, |i| &raw[..i]);
    let mut fwd = vec!["use".to_string(), target.to_string()];
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
                emit_cli_report(
                    "E2102",
                    format!("`{s}` isn't a flag `jet run …@nixpkgs` understands"),
                    "this form forwards only package-run flags before `--`; tool arguments go after `--`".to_string(),
                    format!("write `jet run {package}@nixpkgs -- {s}` to pass it to the tool."),
                    false,
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
        "use",
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
// E2101 has the same argv-only boundary as E2102: no Jet source span exists for
// a command token, so there is no `fix_edits` entry for `jet fix` to apply.
fn unknown_subcommand(cmd: &str) -> ! {
    let bin = jet::Syntax::BINARY_NAME;
    let fix = match jet::CLI::closest_command(cmd) {
        Some(close) => format!("did you mean `{bin} {close}`? Run `{bin} help` to see them all."),
        None => format!("run `{bin} help` to see every command."),
    };
    emit_cli_report(
        "E2101",
        format!("`{cmd}` isn't a {bin} command."),
        format!("every {bin} run starts with a command like `run`, `check`, or `new`."),
        fix,
        false,
    );
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
        if matches!(
            arg.as_str(),
            "-p" | "--output" | "--gate" | "--scope" | "--kind" | "--target"
        ) {
            skip_next = true;
            continue;
        }
        if arg.starts_with("--output=")
            || arg.starts_with("--scope=")
            || arg.starts_with("--kind=")
            || arg.starts_with("--target=")
        {
            continue;
        }
        if arg == "-" || !arg.starts_with('-') {
            return Some(arg);
        }
    }
    None
}

fn normalize_compiler_alias(raw: &mut Vec<String>, argv0: &str) {
    let Some(name) = Path::new(argv0).file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let Some(verb) = (match name {
        "jet-cc" => Some("cc"),
        "jet-cxx" | "jet-c++" => Some("c++"),
        _ => None,
    }) else {
        return;
    };
    raw.insert(0, verb.to_string());
}

fn normalize_frequency_ring_argv(
    raw: &mut Vec<String>,
    mode: OutputMode,
    profile: jet_cli::OutputProfile::OutputProfile,
) {
    if let Some(retired) = jet::CLI::retired_command(raw) {
        let category = retired.category;
        let rewrite = retired.rewrite;
        if category == jet::CLI::RetirementCategory::Semantic {
            teach_retired(retired, raw, mode.json);
        }
        let rewrite = rewrite.expect("rename retirement needs a rewrite rule");
        let replacement = (retired.fix)(raw);
        *raw = rewrite(raw);
        if !mode.json && !mode.quiet {
            write_status(
                profile,
                &format!("Notice: `{}` is now `{replacement}`.\n", retired.spelling),
            );
        }
    }
    let Some(first) = raw.first().map(String::as_str) else {
        return;
    };
    if let Some((group_spec, _)) = jet::CLI::moved_command(first) {
        let group = group_spec.name;
        let verb = first;
        let replacement = format!("jet {group} {}", raw.join(" "));
        emit_cli_report(
            "E2101",
            format!("`{verb}` moved under `jet {group}`."),
            "infrequent commands live in a named area so daily Jet commands stay easy to scan."
                .to_string(),
            format!("run `{replacement}`."),
            mode.json,
        );
        exit(ExitCodes::USAGE);
    }
    let Some(group) = raw.first().cloned() else {
        return;
    };
    // D-CLI-SURFACE3=B: `os` is not exhaustive — jetos's own native verbs
    // (`check`/`build`/`switch`/…, D-JPK-OSVERB1) stay opaque to this
    // registry, so bare `jet os` / `jet os help` fall through unchanged to
    // the real `jet os` dispatcher instead of being hijacked by this
    // group's (partial) action list.
    let exhaustive = jet::CLI::command_group(&group)
        .map(|spec| spec.exhaustive)
        .unwrap_or(false);
    if let Some(spec) = jet::CLI::command_group(&group) {
        // #1659 criterion 2: `--help`/`-h` are real help requests here, not
        // an unmodeled subword — retiring the E2101 that used to fire for
        // an exhaustive group help request.
        let asks_help = if group == "env" {
            matches!(raw.get(1).map(String::as_str), Some("help"))
                || raw.get(1).is_some_and(|a| jet::CLI::is_help_flag(a))
        } else {
            raw.len() == 1
                || matches!(raw.get(1).map(String::as_str), Some("help"))
                || raw.get(1).is_some_and(|a| jet::CLI::is_help_flag(a))
        };
        if (exhaustive || group == "env") && asks_help {
            let help = format!(
                "jet {group} — {}\n{}",
                spec.summary,
                jet::CLI::command_group_usage(&group)
            );
            write_renderable(profile, &help);
            exit(ExitCodes::OK);
        }
    }
    let Some(sub) = raw.get(1).cloned() else {
        return;
    };
    if exhaustive && jet::CLI::nested_command(&group, &sub).is_none() {
        emit_cli_report(
            "E2101",
            format!("`{sub}` isn't a jet {group} command."),
            format!("jet {group} accepts only commands in its named area."),
            format!("run `jet {group} help`."),
            mode.json,
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

fn run_project_parts(raw: &[String], profile: jet_cli::OutputProfile::OutputProfile) -> ! {
    let skipped_only = raw.iter().any(|arg| arg == "--skipped");
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (report, failures) = jet::ProjectParts::scan_with_diagnostics(&root, &[]);
    if let Some(failure) = failures.iter().find(|failure| failure.authority) {
        emit_cli_value(failure.problem.clone(), profile.machine_enabled());
        exit(ExitCodes::USER_ERROR);
    }
    let parts = report
        .parts
        .iter()
        .filter(|part| !skipped_only || part.state == jet::ProjectParts::ProjectPartState::Skipped)
        .collect::<Vec<_>>();
    let source_files = if skipped_only {
        Vec::new()
    } else {
        report.source_files.iter().collect::<Vec<_>>()
    };
    let relative = |path: &Path| {
        path.strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let source_name = |path: &Path| {
        let mut name = relative(path);
        let extension = format!(".{}", jet::Syntax::FILE_EXT);
        if name.ends_with(&extension) {
            name.truncate(name.len() - extension.len());
        }
        format!(
            "{}{}",
            jet::Syntax::PROJECT_IMPORT_PREFIX,
            name.replace('/', ".")
        )
    };
    if profile.machine_enabled() {
        let mut part_values = Vec::with_capacity(parts.len() + source_files.len());
        part_values.extend(parts.iter().map(|part| {
            StatusValue::object(
                StatusFields::new()
                    .with("name", part.canonical_name())
                    .with("path", relative(&part.path))
                    .with("state", part.state.name()),
            )
        }));
        part_values.extend(source_files.iter().map(|path| {
            StatusValue::object(
                StatusFields::new()
                    .with("name", source_name(path))
                    .with("path", relative(path))
                    .with("state", "automatic"),
            )
        }));
        let conflict_values = report.conflicts.iter().map(|conflict| {
            StatusValue::object(
                StatusFields::new()
                    .with("name", conflict.canonical_name())
                    .with(
                        "paths",
                        StatusValue::array(
                            conflict
                                .paths
                                .iter()
                                .map(|path| StatusValue::from(relative(path))),
                        ),
                    ),
            )
        });
        let payload = render_status(
            "inspect.parts",
            true,
            StatusFields::new()
                .with("parts", StatusValue::array(part_values))
                .with("conflicts", StatusValue::array(conflict_values)),
        );
        write_machine(profile, &format!("{payload}\n"));
    } else if profile.unicode_enabled() {
        let columns = vec![
            jet_cli::AdaptiveTable::TableColumn::new(
                "state",
                jet_cli::AdaptiveTable::TableCellKind::Text,
            ),
            jet_cli::AdaptiveTable::TableColumn::new(
                "name",
                jet_cli::AdaptiveTable::TableCellKind::Text,
            ),
            jet_cli::AdaptiveTable::TableColumn::new(
                "path",
                jet_cli::AdaptiveTable::TableCellKind::Text,
            ),
        ];
        let mut rows = Vec::with_capacity(parts.len() + source_files.len());
        rows.extend(parts.iter().map(|part| {
            jet_cli::AdaptiveTable::TableRow::new(vec![
                jet_cli::AdaptiveTable::TableCell::text(part.state.name()),
                jet_cli::AdaptiveTable::TableCell::text(part.canonical_name()),
                jet_cli::AdaptiveTable::TableCell::text(relative(&part.path)),
            ])
        }));
        rows.extend(source_files.iter().map(|path| {
            jet_cli::AdaptiveTable::TableRow::new(vec![
                jet_cli::AdaptiveTable::TableCell::text("automatic"),
                jet_cli::AdaptiveTable::TableCell::text(source_name(path)),
                jet_cli::AdaptiveTable::TableCell::text(relative(path)),
            ])
        }));
        let table = jet_cli::AdaptiveTable::AdaptiveTable::new(columns, rows);
        if let Some(rendered) = table.layout(&profile).render() {
            write_renderable(profile, &format!("{rendered}\n"));
        }
        for conflict in &report.conflicts {
            let rendered = jet::render_diagnostics("", "", &[conflict.diagnostic(&root, None)]);
            write_status(profile, &rendered);
        }
    } else {
        let mut rendered = String::new();
        for part in parts {
            rendered.push_str(&format!(
                "{:<9} {:<24} {}\n",
                part.state.name(),
                part.canonical_name(),
                relative(&part.path)
            ));
        }
        for path in source_files {
            rendered.push_str(&format!(
                "{:<9} {:<24} {}\n",
                "automatic",
                source_name(path),
                relative(path)
            ));
        }
        write_renderable(profile, &rendered);
        for conflict in &report.conflicts {
            let rendered = jet::render_diagnostics("", "", &[conflict.diagnostic(&root, None)]);
            write_status(profile, &rendered);
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
    let external_fmt = subcmd == "fmt"
        && raw
            .iter()
            .any(|arg| arg == jet::Syntax::FMT_FLAG_LANG || arg.starts_with("--lang="));
    // E2102 is an argv diagnostic, not a Jet-source diagnostic: there is no
    // source file and byte span for `jet fix` to apply, so its flag suggestion
    // remains prose-only until the CLI report gains an argv edit surface.
    for a in raw {
        if !a.starts_with("--") || a == "--" {
            continue;
        }
        if jet::CLI::is_known_flag(a) {
            continue;
        }
        let head = a.split('=').next().unwrap_or(a);
        if external_fmt
            && matches!(
                head,
                "--trust"
                    | "--offline"
                    | "--online"
                    | "--no-color"
                    | "--json"
                    | "--flake"
                    | "--pure"
                    | "--yes"
                    | "--fixtures"
                    | "--preset"
                    | "--env"
            )
        {
            continue;
        }
        if head == "--emit-rust" {
            emit_cli_report(
                "E2102",
                format!("`--emit-rust` isn't a flag {bin} understands"),
                "generated output belongs to the `emit` command".to_string(),
                format!("run `{bin} emit --rust <file.{}>`", jet::Syntax::FILE_EXT),
                false,
            );
            exit(ExitCodes::USAGE);
        }
        let fix = match jet::CLI::closest_flag(head) {
            Some(close) if matches!(subcmd, "run" | "test") => format!(
                "did you mean `{close}`? Or use `{bin} {subcmd} <file> -- {head}` to pass it to your program"
            ),
            Some(close) => format!("did you mean `{close}`? (run `{bin} help` for the flags)"),
            None if matches!(subcmd, "run" | "test") => format!(
                "use `{bin} {subcmd} <file> -- {head}` to pass this flag to your program"
            ),
            None => format!("drop the flag, or run `{bin} help` to see the flags"),
        };
        emit_cli_report(
            "E2102",
            format!("`{head}` isn't a flag {bin} understands"),
            format!("flags before `--` belong to {bin}; everything after `--` is forwarded to your program"),
            fix,
            false,
        );
        exit(ExitCodes::USAGE);
    }
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
        "the old boolean changed which audited escapes were checked and is no longer accepted"
            .to_string(),
        format!("use `{}`", retirement.canonical),
        json,
    );
    exit(ExitCodes::USAGE);
}
/// D-RIGHTS-CLI1=B: one invocation spelling for authority. The values are
/// canonical effect roots or leaves; old per-effect flags are retired rather
/// than silently interpreted as a second policy language.
fn reject_retired_authority_flags(argv: &[String], json: bool) {
    for argument in argv {
        let head = argument.split('=').next().unwrap_or(argument);
        let Some((kind, effect)) = BuildEffect::ALL.into_iter().find_map(|effect| {
            let allow = format!("--allow-{}", effect.flag());
            let deny = format!("--deny-{}", effect.flag());
            if head == allow {
                Some(("allow", effect))
            } else if head == deny {
                Some(("deny", effect))
            } else {
                None
            }
        }) else {
            continue;
        };
        emit_cli_report(
            "E2102",
            format!("`{head}` is retired"),
            "authority grants and denials use one comma-separated rights surface".to_string(),
            format!("use `--{kind}={}`", effect.name()),
            json,
        );
        exit(ExitCodes::USAGE);
    }
}

/// Parse the canonical invocation authority rows once at the host boundary.
/// Runtime REPLs keep roots or leaves; native build policy receives the same
/// names and projects only the ten build-capability roots below.
fn parse_authority_flags(argv: &[String], json: bool) -> (Vec<String>, Vec<String>) {
    let mut allow = Vec::new();
    let mut deny = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        let argument = argv[index].as_str();
        let (kind, inline) = if let Some(value) = argument.strip_prefix("--allow=") {
            (Some("allow"), Some(value))
        } else if let Some(value) = argument.strip_prefix("--deny=") {
            (Some("deny"), Some(value))
        } else if argument == "--allow" {
            (Some("allow"), None)
        } else if argument == "--deny" {
            (Some("deny"), None)
        } else {
            (None, None)
        };
        let Some(kind) = kind else {
            index += 1;
            continue;
        };
        let value = match inline {
            Some(value) => value.to_string(),
            None => {
                let Some(value) = argv.get(index + 1).filter(|value| !value.starts_with('-'))
                else {
                    emit_cli_report(
                        "E2104",
                        format!("`--{kind}` needs a comma-separated rights value"),
                        "authority is read before the program's own arguments".to_string(),
                        format!("use `--{kind}=FS.Read,Time`"),
                        json,
                    );
                    exit(ExitCodes::USAGE);
                };
                index += 1;
                value.clone()
            }
        };
        if value.trim().is_empty() {
            emit_cli_report(
                "E2104",
                format!("`--{kind}` needs a non-empty rights value"),
                "authority names one or more effect roots or leaves".to_string(),
                format!("use `--{kind}=FS.Read,Time`"),
                json,
            );
            exit(ExitCodes::USAGE);
        }
        for raw_right in value.split(',') {
            let right = raw_right.trim();
            let Some(canonical) = jet_foundation::Authority::parse_right(right) else {
                emit_cli_report(
                    "E2104",
                    format!("unknown authority right `{right}`"),
                    "authority names match the canonical effect roots and leaves".to_string(),
                    format!("use `--{kind}=FS.Read,Time`"),
                    json,
                );
                exit(ExitCodes::USAGE);
            };
            let rights = if kind == "allow" {
                &mut allow
            } else {
                &mut deny
            };
            if !rights.iter().any(|existing| existing == &canonical) {
                rights.push(canonical);
            }
        }
        index += 1;
    }
    if let Some(right) = allow
        .iter()
        .find(|right| deny.iter().any(|denied| denied == *right))
    {
        emit_cli_report(
            "E2102",
            format!("authority right `{right}` is both allowed and denied"),
            "the same exact effective right cannot be granted and denied in one invocation"
                .to_string(),
            format!("remove one of `--allow={right}` or `--deny={right}`"),
            json,
        );
        exit(ExitCodes::USAGE);
    }
    (allow, deny)
}

fn build_grants_from_authority(allow: &[String]) -> Vec<String> {
    let mut grants = BTreeSet::new();
    for right in allow {
        let root = jet_foundation::Authority::root(right);
        if let Some(effect) = BuildEffect::parse(root) {
            grants.insert(effect.flag().to_string());
        }
    }
    grants.into_iter().collect()
}

fn named_record_for_command(argv: &[String], command: &str, json: bool) -> Option<String> {
    let mut name = None;
    for arg in argv.iter().skip(1) {
        if arg == "--record" {
            crate::cli_error!(
                @fix "E2104",
                "`--record` needs a closed `=NAME` value",
                "write `--record=NAME` on `run`, `dev`, `test`, or `debug`"
            );
            exit(ExitCodes::USAGE);
        }
        let Some(parsed) = crate::ProveReplay::parse_record_flag(arg) else {
            continue;
        };
        if !matches!(command, "run" | "dev" | "test" | "debug") {
            crate::cli_error!(
                @fix "E2102",
                format!("`--record` is not valid with `jet {command}`"),
                "use `--record=NAME` on `run`, `dev`, `test`, or `debug`"
            );
            exit(ExitCodes::USAGE);
        }
        match parsed {
            Ok(value) => name = Some(value),
            Err(message) => {
                crate::ProveReplay::emit_diag(
                    "E2104",
                    "invalid replay name",
                    &message,
                    "write `--record=NAME` with letters, digits, `-`, or `_`",
                    json,
                );
                exit(ExitCodes::USAGE);
            }
        }
    }
    name
}

fn named_debug_replay(argv: &[String], command: &str, json: bool) -> Option<String> {
    if command != "debug" {
        return None;
    }
    let mut path = None;
    for arg in argv.iter().skip(1) {
        if arg == "--replay" {
            crate::cli_error!(
                @fix "E2104",
                "`--replay` needs a closed `=NAME` value",
                "write `--replay=NAME` on `jet debug`"
            );
            exit(ExitCodes::USAGE);
        }
        let Some(parsed) = crate::ProveReplay::parse_closed_replay_flag(arg) else {
            continue;
        };
        match parsed {
            Ok(value) => path = Some(value),
            Err(message) => {
                crate::ProveReplay::emit_diag(
                    "E2104",
                    "invalid replay name",
                    &message,
                    "write `--replay=NAME` with letters, digits, `-`, or `_`",
                    json,
                );
                exit(ExitCodes::USAGE);
            }
        }
    }
    path
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
                        "a typed package setting override names one declared key and its value"
                            .to_string(),
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
        if settings
            .insert(key.to_string(), value.trim().to_string())
            .is_some()
        {
            emit_cli_report(
                "E2104",
                format!("setting `{key}` is assigned more than once"),
                "one invocation must contribute one unambiguous value for each declared setting"
                    .to_string(),
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
    // Keep the derived executable name to one safe path component. Otherwise
    // separators in an unknown command can escape the PATH entry below.
    if cmd.is_empty()
        || matches!(cmd, "." | "..")
        || cmd
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | ':'))
    {
        return None;
    }
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
/// for `<query>` (args), printed once and exited. Named recording/replay
/// requests deliberately use the headless reducer, so their artifacts and
/// output never depend on a live terminal.
fn run_question_mark(args: &[String], output: &OutputAdapterHost) -> ! {
    let profile = output.profile();
    let color = profile.ansi_enabled();
    let mut query_args = Vec::new();
    let mut record_name = None;
    let mut replay_name = None;
    let mut skip_value = false;
    let store = jet_cli::Recording::RecordingStore::default_store();
    for arg in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        if arg == "--color" {
            skip_value = true;
            continue;
        }
        if arg == "--record" {
            question_mark_recording_error(
                ExitCodes::USAGE,
                "E2104",
                "`--record` needs a closed `=NAME` value",
                "use `jet ? --record=NAME`",
                profile.machine_enabled(),
            );
        }
        if let Some(name) = arg.strip_prefix("--record=") {
            if record_name.is_some() {
                question_mark_recording_error(
                    ExitCodes::USAGE,
                    "E2104",
                    "`jet ?` accepts only one `--record=NAME` request",
                    "remove the duplicate recording flag",
                    profile.machine_enabled(),
                );
            }
            if let Err(error) = store.path_for_name(name) {
                question_mark_recording_error(
                    ExitCodes::USAGE,
                    "E2104",
                    format!("invalid recording name: {error}"),
                    "use letters, digits, `-`, or `_` in NAME",
                    profile.machine_enabled(),
                );
            }
            record_name = Some(name.to_string());
            continue;
        }
        if arg == "--replay" {
            question_mark_recording_error(
                ExitCodes::USAGE,
                "E2104",
                "`--replay` needs a closed `=NAME` value",
                "use `jet ? --replay=NAME`",
                profile.machine_enabled(),
            );
        }
        if let Some(name) = arg.strip_prefix("--replay=") {
            if replay_name.is_some() {
                question_mark_recording_error(
                    ExitCodes::USAGE,
                    "E2104",
                    "`jet ?` accepts only one `--replay=NAME` request",
                    "remove the duplicate replay flag",
                    profile.machine_enabled(),
                );
            }
            if let Err(error) = store.path_for_name(name) {
                question_mark_recording_error(
                    ExitCodes::USAGE,
                    "E2104",
                    format!("invalid replay name: {error}"),
                    "use letters, digits, `-`, or `_` in NAME",
                    profile.machine_enabled(),
                );
            }
            replay_name = Some(name.to_string());
            continue;
        }
        if !arg.starts_with("--color=") && arg != "--json" && arg != "--quiet" {
            query_args.push(arg.as_str());
        }
    }
    if record_name.is_some() && replay_name.is_some() {
        question_mark_recording_error(
            ExitCodes::USAGE,
            "E2104",
            "`jet ?` cannot record and replay in the same request",
            "choose either `--record=NAME` or `--replay=NAME`",
            profile.machine_enabled(),
        );
    }
    if (record_name.is_some() || replay_name.is_some()) && !query_args.is_empty() {
        question_mark_recording_error(
            ExitCodes::USAGE,
            "E2104",
            "a `jet ?` recording request cannot include a help query",
            "remove the query, or run `jet ? <query>` without recording",
            profile.machine_enabled(),
        );
    }
    if let Some(name) = record_name {
        run_question_mark_recording(&store, &name, profile);
    }
    if let Some(name) = replay_name {
        run_question_mark_replay(&store, &name, profile);
    }
    if !query_args.is_empty() {
        let query = query_args.join(" ");
        let rendered = jet::Help::run_query(&query, color);
        write_renderable(profile, &rendered);
        exit(ExitCodes::OK);
    }
    if output.help_interactive() && profile.human_enabled() {
        jet::Help::Interactive::run(color).ok();
        exit(ExitCodes::OK);
    }
    let rendered = jet::Help::Render::render_categorized(
        &jet::Help::build_index(),
        0,
        false,
        None,
        profile.width(),
        color,
        None,
    );
    write_renderable(profile, &format!("{rendered}\n"));
    exit(ExitCodes::OK);
}

const QUESTION_MARK_RECORDING_HEIGHT: usize = 24;

fn question_mark_recording_error(
    status: i32,
    code: &str,
    what: impl Into<String>,
    fix: impl Into<String>,
    json: bool,
) -> ! {
    emit_cli_report(
        code,
        what.into(),
        "the requested help recording cannot be used".to_string(),
        fix.into(),
        json,
    );
    exit(status);
}

fn question_mark_recording_identity(
    profile: jet_cli::OutputProfile::OutputProfile,
) -> Result<jet_cli::Recording::RecordingIdentity, String> {
    let width = u16::try_from(profile.width())
        .map_err(|_| format!("output width {} exceeds recording limits", profile.width()))?;
    jet_cli::Recording::RecordingIdentity::new(
        "?",
        width,
        u16::try_from(QUESTION_MARK_RECORDING_HEIGHT)
            .expect("question-mark recording height fits in u16"),
        profile.ansi_enabled(),
    )
    .map_err(|error| error.to_string())
}

fn run_question_mark_recording(
    store: &jet_cli::Recording::RecordingStore,
    name: &str,
    profile: jet_cli::OutputProfile::OutputProfile,
) -> ! {
    let identity = match question_mark_recording_identity(profile) {
        Ok(identity) => identity,
        Err(error) => question_mark_recording_error(
            ExitCodes::USAGE,
            "E2104",
            format!("cannot create help recording identity: {error}"),
            "use a terminal width within the recording limits",
            profile.machine_enabled(),
        ),
    };
    let mut replay = match jet_cli::Headless::HeadlessReplay::new(identity) {
        Ok(replay) => replay,
        Err(error) => question_mark_recording_error(
            ExitCodes::USER_ERROR,
            "E3629",
            format!("could not initialize help recording: {error}"),
            "retry the recording with the same terminal profile",
            profile.machine_enabled(),
        ),
    };
    let output = match replay.capture_output() {
        Ok(output) => output,
        Err(error) => question_mark_recording_error(
            ExitCodes::USER_ERROR,
            "E3629",
            format!("could not capture help recording: {error}"),
            "retry the recording",
            profile.machine_enabled(),
        ),
    };
    let recording = replay.recording();
    let path = match store.write(name, recording) {
        Ok(path) => path,
        Err(error) => question_mark_recording_error(
            ExitCodes::USER_ERROR,
            "E3629",
            format!("could not publish help recording: {error}"),
            "choose a new NAME or remove the conflicting artifact",
            profile.machine_enabled(),
        ),
    };
    let artifact_id = match recording.artifact_id() {
        Ok(artifact_id) => artifact_id,
        Err(error) => question_mark_recording_error(
            ExitCodes::USER_ERROR,
            "E3629",
            format!("could not identify help recording: {error}"),
            "retry the recording",
            profile.machine_enabled(),
        ),
    };
    if profile.machine_enabled() {
        let payload = render_status(
            "help.record",
            true,
            StatusFields::new()
                .with("artifact", path.display().to_string())
                .with("artifact_id", artifact_id)
                .with("frames", recording.events().len()),
        );
        write_machine(profile, &format!("{payload}\n"));
    } else {
        write_renderable(profile, &format!("{output}\n"));
        write_status(profile, &format!("recording: {}\n", path.display()));
    }
    exit(ExitCodes::OK);
}

fn run_question_mark_replay(
    store: &jet_cli::Recording::RecordingStore,
    name: &str,
    profile: jet_cli::OutputProfile::OutputProfile,
) -> ! {
    let recording = match store.read(name) {
        Ok(recording) => recording,
        Err(error) => question_mark_recording_error(
            ExitCodes::USER_ERROR,
            "E3622",
            format!("could not read help replay `{name}`: {error}"),
            "pass an intact `.jetproof-replay` TUI recording",
            profile.machine_enabled(),
        ),
    };
    if recording.identity().command != "?" {
        question_mark_recording_error(
            ExitCodes::USER_ERROR,
            "E3621",
            format!(
                "help replay `{name}` targets `{}` instead of `jet ?`",
                recording.identity().command
            ),
            "record the help palette with `jet ? --record=NAME`",
            profile.machine_enabled(),
        );
    }
    let expected_frame = recording
        .events()
        .iter()
        .rev()
        .find_map(|event| match event {
            jet_cli::Recording::RecordingEvent::Frame { text, .. } => {
                Some(jet_cli::Tape::normalize_terminal_text(text))
            }
            _ => None,
        });
    let tape = match recording.interaction_tape() {
        Ok(tape) => tape,
        Err(error) => question_mark_recording_error(
            ExitCodes::USER_ERROR,
            "E3622",
            format!("help replay `{name}` has unsupported input: {error}"),
            "record the help palette through the headless TUI seam",
            profile.machine_enabled(),
        ),
    };
    let mut replay = match jet_cli::Headless::HeadlessReplay::new(recording.identity().clone()) {
        Ok(replay) => replay,
        Err(error) => question_mark_recording_error(
            ExitCodes::USER_ERROR,
            "E3622",
            format!("could not initialize help replay `{name}`: {error}"),
            "record the help palette with a valid terminal identity",
            profile.machine_enabled(),
        ),
    };
    let run = match replay.replay(&tape) {
        Ok(run) => run,
        Err(error) => question_mark_recording_error(
            ExitCodes::USER_ERROR,
            "E3623",
            format!("help replay `{name}` diverged: {error}"),
            "recapture the help palette with `jet ? --record=NAME`",
            profile.machine_enabled(),
        ),
    };
    let output = jet_cli::Tape::normalize_terminal_text(&replay.driver().render().text);
    if expected_frame.as_deref() != Some(output.as_str()) {
        question_mark_recording_error(
            ExitCodes::USER_ERROR,
            "E3623",
            format!("help replay `{name}` final frame differs from its recording"),
            "recapture the help palette with `jet ? --record=NAME`",
            profile.machine_enabled(),
        );
    }
    let path = match store.path_for_name(name) {
        Ok(path) => path,
        Err(error) => question_mark_recording_error(
            ExitCodes::USAGE,
            "E2104",
            format!("invalid replay name: {error}"),
            "use letters, digits, `-`, or `_` in NAME",
            profile.machine_enabled(),
        ),
    };
    let artifact_id = match recording.artifact_id() {
        Ok(artifact_id) => artifact_id,
        Err(error) => question_mark_recording_error(
            ExitCodes::USER_ERROR,
            "E3622",
            format!("could not identify help replay `{name}`: {error}"),
            "pass an intact `.jetproof-replay` TUI recording",
            profile.machine_enabled(),
        ),
    };
    if profile.machine_enabled() {
        let payload = render_status(
            "help.replay",
            true,
            StatusFields::new()
                .with("artifact", path.display().to_string())
                .with("artifact_id", artifact_id)
                .with("steps", run.steps_executed),
        );
        write_machine(profile, &format!("{payload}\n"));
    } else {
        write_renderable(profile, &format!("{output}\n"));
    }
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

fn canvas_flag_value<'a>(argv: &'a [String], name: &str) -> Option<&'a str> {
    argv.iter()
        .find_map(|arg| arg.strip_prefix(&format!("{name}=")))
        .or_else(|| flag_value(argv, name))
}

fn canvas_options_from_args(
    argv: &[String],
    output: Option<&str>,
    target: Option<&str>,
) -> jet_devserver::WebHost::CanvasHostOptions {
    let mut options = jet_devserver::WebHost::CanvasHostOptions::default();
    if let Some(host) = canvas_flag_value(argv, "--canvas-host") {
        options.host = host.to_string();
    }
    if let Some(port) = canvas_flag_value(argv, "--canvas-port") {
        options.port = Some(port.parse::<u16>().unwrap_or_else(|_| {
            crate::cli_error!(
                @fix "E2104",
                format!("`--canvas-port={port}` isn't a valid port number"),
                "use a number from 1 to 65535, e.g. `--canvas-port=3000`"
            );
            exit(ExitCodes::USAGE);
        }));
        if options.port == Some(0) {
            crate::cli_error!(
                @fix "E2104",
                "`--canvas-port=0` isn't a valid port number",
                "use a number from 1 to 65535, or omit `--canvas-port` for automatic selection"
            );
            exit(ExitCodes::USAGE);
        }
    }
    if let Some(transport) = canvas_flag_value(argv, "--canvas-transport") {
        options.transport = transport.to_string();
    }
    if let Some(authority) = canvas_flag_value(argv, "--canvas-authority") {
        options.authority = authority.to_string();
    }
    options.output = output.map(str::to_string);
    options.target = target.map(str::to_string);
    options.audit = argv.iter().any(|arg| arg == "--canvas-audit");
    options
}

fn run_explain_reload(
    file: Option<&str>,
    mode: OutputMode,
    profile: jet_cli::OutputProfile::OutputProfile,
) {
    let Some(file) = file else {
        crate::cli_error!(
            @fix "E2104",
            "`jet explain --reload` needs one source or project path",
            "run `jet explain --reload <file.jet|dir>`"
        );
        exit(ExitCodes::USAGE);
    };

    let resolved = resolve_source_path(file);
    let canonical = match fs::canonicalize(&resolved) {
        Ok(path) if path.is_file() => path,
        Ok(_) => {
            crate::cli_error!(
                @fix "E2104",
                format!("`{file}` is not a source file"),
                "pass a `.jet` file or a project directory with a source entry"
            );
            exit(ExitCodes::USAGE);
        }
        Err(error) => {
            crate::cli_error!(
                @fix "E2105",
                format!("couldn't read `{file}`: {error}"),
                "check the source path and run the command again"
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    let canonical_text = canonical.to_string_lossy().into_owned();
    let source_id = canonical_text.clone();
    let emit = |explanation: jet::DevServer::NativeSwap::NativeSwapExplanation| {
        if mode.json {
            let payload = render_status(
                "explain.reload",
                true,
                StatusFields::new()
                    .with("source_id", explanation.source_id.as_str())
                    .with("disposition", explanation.disposition.to_string())
                    .with("blocking_fact", explanation.blocking_fact.as_str())
                    .with("detail", explanation.detail.as_str()),
            );
            write_machine(profile, &format!("{payload}\n"));
        } else {
            write_renderable(profile, &format!("{}\n", explanation.render()));
        }
    };

    let baseline = match git_head_source(&canonical) {
        Ok(source) => source,
        Err(detail) => {
            emit(jet::DevServer::NativeSwap::NativeSwapExplanation::new(
                source_id,
                jet::DevServer::NativeSwap::NativeSwapDisposition::Rejected,
                jet::DevServer::NativeSwap::NativeSwapBlockingFact::CapabilityUnavailable,
                detail,
            ));
            return;
        }
    };
    let checked = jet::run_compiler_work(|| {
        let mut cache = jet::Sema::IncrementalSemaCache::new();
        let old = jet::Driver::check_file_with_effect_facts_incremental(
            &canonical_text,
            Some((&canonical, baseline.as_str())),
            false,
            &mut cache,
        );
        let new = jet::Driver::check_file_with_effect_facts_incremental(
            &canonical_text,
            None,
            false,
            &mut cache,
        );
        (old, new)
    });
    let (old_diagnostics, old_bundle, _) = checked.0;
    let (new_diagnostics, new_bundle, _) = checked.1;
    let (old_bundle, new_bundle) = match (old_bundle, new_bundle) {
        (Some(old_bundle), Some(new_bundle)) => (old_bundle, new_bundle),
        (old_bundle, new_bundle) => {
            let (phase, diagnostics) = if old_bundle.is_none() {
                ("HEAD", old_diagnostics)
            } else {
                ("current", new_diagnostics)
            };
            let detail = if diagnostics.is_empty() {
                format!("{phase} source did not produce a checked program")
            } else {
                format!(
                    "{phase} source check failed: {}",
                    diagnostics
                        .iter()
                        .map(|diagnostic| format!("{} {}", diagnostic.code, diagnostic.what))
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            };
            emit(jet::DevServer::NativeSwap::NativeSwapExplanation::new(
                source_id,
                jet::DevServer::NativeSwap::NativeSwapDisposition::Rejected,
                jet::DevServer::NativeSwap::NativeSwapBlockingFact::CompileFailed,
                detail,
            ));
            return;
        }
    };
    let decision =
        match jet::Sema::HotSwap::type_stable_decision(&old_bundle, &new_bundle, &source_id) {
            Ok(decision) => decision,
            Err(diagnostics) => {
                let detail = diagnostics
                    .iter()
                    .map(|diagnostic| format!("{} {}", diagnostic.code, diagnostic.what))
                    .collect::<Vec<_>>()
                    .join("; ");
                emit(jet::DevServer::NativeSwap::NativeSwapExplanation::new(
                    source_id,
                    jet::DevServer::NativeSwap::NativeSwapDisposition::Rejected,
                    jet::DevServer::NativeSwap::NativeSwapBlockingFact::CompileFailed,
                    if detail.is_empty() {
                        "the checked compiler could not produce a reload verdict".to_string()
                    } else {
                        detail
                    },
                ));
                return;
            }
        };
    if decision.is_compatible() {
        emit(jet::DevServer::NativeSwap::NativeSwapExplanation::compatible(source_id));
    } else {
        emit(jet::DevServer::NativeSwap::NativeSwapExplanation::new(
            source_id,
            jet::DevServer::NativeSwap::NativeSwapDisposition::Restart,
            jet::DevServer::NativeSwap::NativeSwapBlockingFact::IncompatibleType,
            decision
                .compatibility
                .reason()
                .unwrap_or("the checked type surface requires a clean restart"),
        ));
    }
}

fn git_head_source(path: &Path) -> Result<String, String> {
    let root = Command::new("git")
        .current_dir(path.parent().unwrap_or(Path::new(".")))
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|error| format!("could not locate the project baseline: {error}"))?;
    if !root.status.success() {
        return Err(
            "no committed project baseline is available for reload explanation".to_string(),
        );
    }
    let root = String::from_utf8(root.stdout)
        .map_err(|_| "git returned a non-text project root".to_string())?;
    let root = PathBuf::from(root.trim());
    let relative = path
        .strip_prefix(&root)
        .map_err(|_| "source path is outside the project baseline".to_string())?;
    let relative = relative
        .to_str()
        .ok_or_else(|| "source path is not valid UTF-8".to_string())?
        .replace('\\', "/");
    let spec = format!("HEAD:{relative}");
    let source = Command::new("git")
        .current_dir(&root)
        .args(["show", &spec])
        .output()
        .map_err(|error| format!("could not read the project baseline: {error}"))?;
    if !source.status.success() {
        return Err("source has no committed baseline for reload explanation".to_string());
    }
    String::from_utf8(source.stdout)
        .map_err(|_| "the committed source baseline is not valid UTF-8".to_string())
}

fn console_requested(argv: &[String]) -> bool {
    argv.iter().any(|argument| {
        matches!(
            argument.as_str(),
            "--console" | "--sandbox" | "--console-ttl"
        ) || argument.starts_with("--sandbox=")
            || argument.starts_with("--console-ttl=")
    })
}

fn invalid_repl_console_flag(argv: &[String]) -> Option<String> {
    argv.iter()
        .find(|argument| {
            let argument = argument.as_str();
            (argument.starts_with("--console")
                && argument != "--console"
                && argument != "--console-ttl"
                && !argument.starts_with("--console-ttl="))
                || (argument.starts_with("--sandbox")
                    && argument != "--sandbox"
                    && !argument.starts_with("--sandbox="))
        })
        .cloned()
}
fn console_flag_value<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.iter()
        .find_map(|argument| {
            argument
                .strip_prefix(flag)
                .and_then(|rest| rest.strip_prefix('='))
        })
        .or_else(|| flag_value(argv, flag))
}

fn console_sandbox(argv: &[String]) -> Result<bool, String> {
    let mut sandbox = false;
    let mut index = 0;
    while index < argv.len() {
        let argument = argv[index].as_str();
        let value = if argument == "--sandbox" {
            index += 1;
            argv.get(index)
                .map(String::as_str)
                .ok_or_else(|| "`--sandbox` needs `data`".to_string())?
        } else if let Some(value) = argument.strip_prefix("--sandbox=") {
            value
        } else {
            index += 1;
            continue;
        };
        if value != "data" {
            return Err(format!(
                "unknown console sandbox `{value}`; use `--sandbox data`"
            ));
        }
        sandbox = true;
        index += 1;
    }
    Ok(sandbox)
}

fn console_rights(
    sandbox: bool,
    authority_allow: &[String],
    authority_deny: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut allow = vec!["IO".to_string(), "Mem.Alloc".to_string()];
    if sandbox {
        allow.push("DB.Write".to_string());
    }
    allow.extend(authority_allow.iter().cloned());
    (allow, authority_deny.to_vec())
}

fn console_output_kind(kind: jet::REPL::ConsoleOutputKind) -> &'static str {
    match kind {
        jet::REPL::ConsoleOutputKind::Startup => "startup",
        jet::REPL::ConsoleOutputKind::Evaluation => "evaluation",
        jet::REPL::ConsoleOutputKind::Request => "request",
        jet::REPL::ConsoleOutputKind::Data => "data",
        jet::REPL::ConsoleOutputKind::Grant => "grant",
        jet::REPL::ConsoleOutputKind::Capabilities => "capabilities",
        jet::REPL::ConsoleOutputKind::Bindings => "bindings",
        jet::REPL::ConsoleOutputKind::Audit => "audit",
        jet::REPL::ConsoleOutputKind::History => "history",
        jet::REPL::ConsoleOutputKind::Sandbox => "sandbox",
        jet::REPL::ConsoleOutputKind::Commit => "commit",
        jet::REPL::ConsoleOutputKind::Rollback => "rollback",
        jet::REPL::ConsoleOutputKind::Cancelled => "cancelled",
        jet::REPL::ConsoleOutputKind::Error => "error",
        jet::REPL::ConsoleOutputKind::Goodbye => "goodbye",
    }
}

fn console_output(mode: OutputMode, output: &jet::REPL::ConsoleOutput) {
    if mode.json {
        let payload = jet_foundation::JSON::parse(&output.payload)
            .and_then(|value| StatusValue::from_data_tree(&value))
            .unwrap_or_else(|_| StatusValue::String(output.payload.clone()));
        let console = StatusValue::object(
            StatusFields::new()
                .with("protocol", "jet.console.v1")
                .with("sequence", output.sequence)
                .with("session_id", output.session_id.as_str())
                .with("kind", console_output_kind(output.kind))
                .with("ok", output.ok)
                .with("message", output.message.as_str())
                .with("payload", payload)
                .with(
                    "request_id",
                    job_status_optional_string(output.request_id.as_deref()),
                )
                .with(
                    "transaction_id",
                    job_status_optional_string(output.transaction_id.as_deref()),
                )
                .with("truncated", output.truncated),
        );
        let payload = render_status(
            "repl",
            output.ok,
            StatusFields::new().with("console", console),
        );
        write_mode_machine(mode, &format!("{payload}\n"));
    } else {
        write_mode_renderable(mode, &format!("{}\n{}\n", output.message, output.payload));
    }
}

fn console_error_code(error: &jet::REPL::ConsoleError) -> &'static str {
    match error {
        jet::REPL::ConsoleError::ReleaseUnavailable => "E2105",
        jet::REPL::ConsoleError::Denied { .. }
        | jet::REPL::ConsoleError::Expired
        | jet::REPL::ConsoleError::NonLoopbackDenied => "E1803",
        jet::REPL::ConsoleError::ProjectUnavailable(_)
        | jet::REPL::ConsoleError::InvalidInput(_)
        | jet::REPL::ConsoleError::InvalidRequest(_)
        | jet::REPL::ConsoleError::UnknownCommand(_)
        | jet::REPL::ConsoleError::ConfirmationRequired(_) => "E2104",
        _ => "E2105",
    }
}

fn run_console(
    project_dir: Option<&str>,
    argv: &[String],
    mode: OutputMode,
    authority_allow: &[String],
    authority_deny: &[String],
    gates: jet::Policy::GateSet,
    profile: Option<&str>,
    setting_overrides: &BTreeMap<String, String>,
) -> i32 {
    let sandbox = match console_sandbox(argv) {
        Ok(sandbox) => sandbox,
        Err(message) => {
            emit_cli_diagnostic("E2104", message);
            return ExitCodes::USAGE;
        }
    };
    let root = PathBuf::from(project_dir.unwrap_or("."));
    let member = flag_value(argv, "-p");
    let entry = match resolve_bare_entry("run", &root, member, mode, true) {
        Some(entry) => entry,
        None => {
            missing_bare_entry("run", &root, mode);
        }
    };
    let file = entry.path.to_string_lossy().into_owned();
    let entry_fn = entry.callable;
    let snapshot = match crate::Store::read_authority_file(&entry.path) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            emit_cli_diagnostic(
                "E2105",
                format!("can't securely read console entry `{file}`: {error}"),
            );
            return ExitCodes::USER_ERROR;
        }
    };
    let source = match snapshot.text() {
        Ok(source) => source.to_owned(),
        Err(error) => {
            emit_cli_diagnostic(
                "E2105",
                format!("can't read console entry `{file}`: {error}"),
            );
            return ExitCodes::USER_ERROR;
        }
    };
    let source_identity = snapshot.path().to_path_buf();
    let source_for_closure = source.clone();
    let file_for_closure = file.clone();
    let source_closure = match jet_jit::on_compiler_stack(move || {
        jet::Driver::load_immutable_source_closure(
            &file_for_closure,
            &[(source_identity.as_path(), source_for_closure.as_str())],
        )
    }) {
        Ok(source_closure) => source_closure,
        Err(diagnostics) => {
            emit_cli_diagnostics(&file, &source, &diagnostics);
            return ExitCodes::USER_ERROR;
        }
    };
    let mut project = match jet::REPL::ConsoleProject::from_root(root.clone()) {
        Ok(project) => project,
        Err(error) => {
            emit_cli_diagnostic(console_error_code(&error), error.to_string());
            return ExitCodes::USER_ERROR;
        }
    };
    let package_facts = match jet_jit::on_compiler_stack({
        let root = root.clone();
        move || jet::Loader::package_facts_for_root(&root)
    }) {
        Ok(package_facts) => package_facts,
        Err(diagnostics) => {
            emit_cli_diagnostics(&file, &source, &diagnostics);
            return ExitCodes::USER_ERROR;
        }
    };
    if let Some(facts) = package_facts {
        for (name, fact) in facts.services {
            let state = if fact.enable { "enabled" } else { "disabled" };
            let identity = format!("{}:service:{name}", project.identity.project_id);
            let handle = match jet::REPL::ConsoleServiceHandle::new(name, identity, state) {
                Ok(handle) => handle,
                Err(error) => {
                    emit_cli_diagnostic(console_error_code(&error), error.to_string());
                    return ExitCodes::USER_ERROR;
                }
            };
            if let Err(error) = project.add_service(handle) {
                emit_cli_diagnostic(console_error_code(&error), error.to_string());
                return ExitCodes::USER_ERROR;
            }
        }
    }
    let (allow, deny) = console_rights(sandbox, authority_allow, authority_deny);
    let application_authority =
        (!authority_allow.is_empty() || !authority_deny.is_empty()).then(|| {
            jet_foundation::Authority::ApplicationAuthority::from_policy(
                Some(authority_allow),
                Some(authority_deny),
                "console invocation",
            )
        });
    let profile = profile.unwrap_or("dev").to_owned();
    let settings = setting_overrides.clone();
    let boot = match jet::Interpreter::boot_console_with_source_closure(
        &file,
        &source_closure,
        gates,
        &profile,
        &settings,
        application_authority.as_ref(),
        entry_fn.as_deref(),
    ) {
        Ok(boot) => boot,
        Err(diagnostics) => {
            emit_cli_diagnostics(&file, &source, &diagnostics);
            return ExitCodes::USER_ERROR;
        }
    };
    CmdDevTools::render_lints(&file, mode, &boot.lints);
    let ttl_ms = match console_flag_value(argv, "--console-ttl") {
        Some(value) => match value.parse::<u64>() {
            Ok(ttl_ms) => Some(ttl_ms),
            Err(_) => {
                emit_cli_diagnostic(
                    "E2104",
                    "`--console-ttl` needs an integer number of milliseconds".to_string(),
                );
                return ExitCodes::USAGE;
            }
        },
        None => None,
    };
    jet_jit::on_compiler_stack(move || {
        let mut lease = boot.lease;
        if !lease.startup_stdout().is_empty() {
            write_mode_renderable(mode, lease.startup_stdout());
        }
        if !lease.startup_stderr().is_empty() {
            write_mode_diagnostic(mode, lease.startup_stderr());
        }
        let mut host = ConsoleHost::new().with_router(lease.router());
        for resource in lease.take_database_resources() {
            host = match host.with_database_resource(resource) {
                Ok(host) => host,
                Err(error) => {
                    emit_cli_diagnostic(console_error_code(&error), error.to_string());
                    return ExitCodes::USER_ERROR;
                }
            };
        }
        for binding in lease.take_service_bindings() {
            host = match host.with_service_binding(binding) {
                Ok(host) => host,
                Err(error) => {
                    emit_cli_diagnostic(console_error_code(&error), error.to_string());
                    return ExitCodes::USER_ERROR;
                }
            };
        }
        let project = match host.attach_project(project) {
            Ok(project) => project,
            Err(error) => {
                emit_cli_diagnostic(console_error_code(&error), error.to_string());
                return ExitCodes::USER_ERROR;
            }
        };
        let mut options = jet::REPL::ConsoleOptions::new(project)
            .with_mode(if sandbox {
                jet::REPL::ConsoleMode::SandboxData
            } else {
                jet::REPL::ConsoleMode::ReadOnly
            })
            .with_rights(allow)
            .with_denied(deny)
            .release_build(cfg!(not(debug_assertions)));
        if let Some(ttl_ms) = ttl_ms {
            options = options.with_ttl_ms(ttl_ms);
        }
        let mut session = match host.open(options) {
            Ok(session) => session,
            Err(error) => {
                emit_cli_diagnostic(console_error_code(&error), error.to_string());
                return ExitCodes::USER_ERROR;
            }
        };
        let stdin = io::stdin();
        let mut input = stdin.lock();
        let mut line = String::new();
        let mut code = ExitCodes::OK;
        loop {
            line.clear();
            let bytes = match input.read_line(&mut line) {
                Ok(bytes) => bytes,
                Err(error) => {
                    emit_cli_diagnostic("E2105", format!("console input read failed: {error}"));
                    code = ExitCodes::USER_ERROR;
                    break;
                }
            };
            if bytes == 0 {
                break;
            }
            match session.run_line(line.trim_end_matches(['\r', '\n'])) {
                Ok(output) => {
                    console_output(mode, &output);
                    if matches!(
                        output.kind,
                        jet::REPL::ConsoleOutputKind::Goodbye
                            | jet::REPL::ConsoleOutputKind::Cancelled
                    ) {
                        break;
                    }
                }
                Err(error) => {
                    emit_cli_diagnostic(console_error_code(&error), error.to_string());
                    code = ExitCodes::USER_ERROR;
                    if matches!(
                        error,
                        jet::REPL::ConsoleError::Closed | jet::REPL::ConsoleError::Cancelled
                    ) {
                        break;
                    }
                }
            }
        }
        if !session.is_closed() {
            if let Ok(output) = session.close() {
                console_output(mode, &output);
            }
        }
        code
    })
}

fn main() {
    // I2: install first, before any other work, so every uncaught panic
    // (including one triggered before argv parsing) renders the branded
    // ICE report instead of raw Rust panic text.
    jet::Diagnostics::install_ice_panic_hook();

    // Windows cannot replace an executable while it is mapped. The updater
    // launches a separate helper image, then exits before normal boot so the
    // helper can complete the verified handoff.
    if jetpack::ToolchainUpdate::windows_update_helper_requested() {
        exit(jetpack::ToolchainUpdate::run_windows_update_helper());
    }

    // Process-wide: any derive/comptime path may hit MirBridge before Loader.
    jet::boot_mir_eval();

    let mut argv = std::env::args();
    let argv0 = argv.next().unwrap_or_default();
    let mut raw: Vec<String> = argv.collect();
    normalize_compiler_alias(&mut raw, &argv0);

    // Resolve every output capability at the host boundary. The standalone
    // separator belongs to the child program and cannot change this profile.
    let output_arg_end = raw
        .iter()
        .position(|argument| argument == "--")
        .unwrap_or(raw.len());
    let output = match OutputAdapterHost::select(&raw[..output_arg_end]) {
        Ok(output) => output,
        Err(error) => {
            write_mode_diagnostic(
                OutputMode {
                    json: false,
                    color: jet::Diagnostics::ColorChoice::Never,
                    quiet: false,
                },
                &format!("Error [E2104]: {error}\n"),
            );
            exit(ExitCodes::USAGE);
        }
    };
    output.apply_machine_mode();
    output.activate();
    let profile = output.profile();
    let mode = output.mode();

    // Private REPL child mode: the parent sends the authoritative session
    // snapshot on stdin, so this path never resolves a source pathname.
    if raw.len() == 1 && raw.first().map(String::as_str) == Some("__jet_repl_run_stdin") {
        run_native_source_from_stdin();
    }

    // c6vz465: bare `jet` starts the REPL (D-REPL4); `jet ?` is help sugar.
    if raw.is_empty() {
        run_repl(None, None, &[], &[], mode.color);
        return;
    }
    if raw[0] == "?" {
        run_question_mark(&raw[1..], &output);
    }

    normalize_frequency_ring_argv(&mut raw, mode, profile);
    // D-CLI-ONE1=A: the host parser has no second inventory. Check the
    // registry-derived command/flag table before dispatch so a stale parser
    // seam fails loudly instead of silently diverging from help/completions.
    // D-CLI-ONE1=A: the dispatch arms and their inventories are one set of
    // macro expansions. The parser guard consumes the exact route words from
    // every dispatch phase; no hand-maintained host route list can drift.
    macro_rules! define_dispatch_routes {
        (
            $(
                $name:literal $(if $guard:expr)? => $body:block $(,)?
            )*
            _ => $fallback:block $(,)?
        ) => {
            macro_rules! dispatch_route_words {
                () => {
                    &[$($name),*]
                };
            }
            macro_rules! dispatch_command {
                ($command:expr) => {
                    match $command {
                        $(
                            $name $(if $guard)? => $body,
                        )*
                        _ => $fallback
                    }
                };
            }
        };
    }
    macro_rules! define_bare_dispatch_routes {
        (
            $($name:literal)|+ => $body:block $(,)?
            _ => $fallback:block $(,)?
        ) => {
            macro_rules! bare_dispatch_route_words {
                () => {
                    &[$($name),*]
                };
            }
            macro_rules! bare_dispatch {
                ($command:expr) => {
                    match $command {
                        $($name)|+ => $body,
                        _ => $fallback
                    }
                };
            }
        };
    }
    macro_rules! define_late_dispatch_routes {
        (
            $(
                $name:literal $(if $guard:expr)? => $body:block $(,)?
            )*
            _ [$($fallback_name:literal)|+] => $fallback:block $(,)?
        ) => {
            macro_rules! late_dispatch_route_words {
                () => {
                    &[$($name,)* $($fallback_name),*]
                };
            }
            macro_rules! late_dispatch {
                ($command:expr) => {
                    match $command {
                        $(
                            $name $(if $guard)? => $body,
                        )*
                        $($fallback_name)|+ => $fallback,
                        _ => $fallback
                    }
                };
            }
        };
    }
    macro_rules! define_direct_dispatch {
        (
            $words:ident,
            $dispatch:ident;
            $($name:literal)|+ => $condition:expr => $body:expr
        ) => {
            macro_rules! $words {
                () => {
                    &[$($name),*]
                };
            }
            macro_rules! $dispatch {
                () => {
                    if $condition {
                        $body
                    }
                };
            }
        };
    }
    define_direct_dispatch!(
        alias_dispatch_route_words,
        dispatch_alias;
        "cc" | "c++" => matches!(raw.first().map(String::as_str), Some("cc") | Some("c++")) => {
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                raw[0].as_str(),
                &raw,
            ));
        }
    );
    define_direct_dispatch!(
        perf_dispatch_route_words,
        dispatch_perf;
        "perf" => raw.first().map(String::as_str) == Some("perf") => {
            match CmdPerf::run(&raw) {
                CmdPerf::Outcome::Exit(code) => exit(code),
            }
        }
    );

    // D-ADOPT-CCSPELL1=A: compiler aliases preserve the raw driver argv and
    // re-enter the canonical Jet subcommand. Keep this ahead of Jet's global
    // `--version` and flag parser so compiler options remain compiler-owned.
    dispatch_alias!();

    // D-PERFSESSION1=D: `jet perf` owns trace sessions for the run and test
    // intents and spawns the exact base-intent driver that writes .jettrace.
    dispatch_perf!();

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
    // `--version` is global only before any recognized command word. A package
    // or backend scaffold may own a later `--version` value.
    let global_version = jet_argv
        .iter()
        .position(|argument| argument == "--version")
        .map_or(false, |version| {
            !jet_argv[..version]
                .iter()
                .any(|argument| jet::CLI::is_builtin(argument))
        });
    if global_version {
        run_version(profile);
        return;
    }
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
    let fmt_simplify = jet_argv.iter().any(|a| a == "--simplify");
    let dry_run = jet_argv.iter().any(|a| a == jet::CLI::DRY_RUN_FLAG);
    let json = profile.machine_enabled();
    reject_retired_gate_flags(jet_argv, json);
    reject_retired_authority_flags(jet_argv, json);
    let (authority_allow, authority_deny) = parse_authority_flags(jet_argv, json);
    let invocation_authority = if authority_allow.is_empty() && authority_deny.is_empty() {
        None
    } else {
        Some(
            jet_foundation::Authority::ApplicationAuthority::from_policy(
                Some(&authority_allow),
                Some(&authority_deny),
                "CLI invocation",
            ),
        )
    };
    let small = jet_argv.iter().any(|a| a == "--small");
    let interpret = jet_argv.iter().any(|a| a == "--interpret");
    let library_flag = jet_argv.iter().any(|a| a == "--lib");
    let gates = parse_gate_flags(jet_argv, json);
    let build_grants = build_grants_from_authority(&authority_allow);
    let locked = jet_argv.iter().any(|a| a == "--locked");
    let annotated = jet_argv.iter().any(|a| a == "--annotated");
    let verbose = output.flags().verbose;
    // D-A11YGATE1=B (c134 Phase 6): `jet lint --a11y` — opt-in, never blocking.
    let a11y = jet_argv.iter().any(|a| a == "--a11y");
    let complexity = jet_argv.iter().any(|a| a == "--complexity");
    let cost = jet_argv.iter().any(|a| a == "--cost");
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
    let web_scaffold = requested_target.as_deref() == Some(jet::Syntax::BUILD_TARGET_WEB);
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
    // A named no-OS machine carries the no-OS fact; there is no standalone
    // profile switch or compatibility alias.
    let no_os = selected_machine
        .as_ref()
        .is_some_and(|machine| machine.no_os);
    let remote_builder: Option<String> = jet_argv.iter().enumerate().find_map(|(index, arg)| {
        arg.strip_prefix("--builder=")
            .map(str::to_string)
            .or_else(|| {
                (arg == "--builder")
                    .then(|| jet_argv.get(index + 1))
                    .flatten()
                    .cloned()
            })
    });
    // c134 Phase 7: `jet dev <file> --target=web --port=<N>` picks the app
    // preview port explicitly instead of scanning from 8080.
    let dev_port: Option<u16> = jet_argv
        .iter()
        .find_map(|a| a.strip_prefix("--port=").map(str::to_string))
        .map(|s| {
            let port = s.parse::<u16>().unwrap_or_else(|_| {
                crate::cli_error!(@fix "E2104", format!("`--port={}` isn't a valid port number", s), "use a number from 1 to 65535, e.g. `--port=3000`");
                exit(ExitCodes::USAGE);
            });
            if port == 0 {
                crate::cli_error!(@fix "E2104", "`--port=0` isn't a valid port number", "use a number from 1 to 65535, e.g. `--port=3000`");
                exit(ExitCodes::USAGE);
            }
            port
        });
    let explain_partition = jet_argv.iter().any(|a| a == "--explain-partition");
    // D-BUILDPROFILE1: `--release` is sugar for `--profile=release`.
    // `--profile=<name>` selects a named profile. Resolved against package.jet
    // in the native execution workflow; only the name is collected here.
    let release_flag = jet_argv.iter().any(|a| a == "--release");
    let profile_flag: Option<String> = jet_argv.iter().enumerate().find_map(|(index, arg)| {
        arg.strip_prefix("--profile=")
            .map(str::to_string)
            .or_else(|| {
                (arg == "--profile")
                    .then(|| jet_argv.get(index + 1))
                    .flatten()
                    .filter(|value| !value.starts_with('-'))
                    .cloned()
            })
    });
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
    // #1659 criterion 3: one spelling, parsed once, threaded everywhere.
    // OutputMode is the immutable command projection of the host profile.
    let setting_overrides = parse_setting_overrides(jet_argv, json);
    // Positional args only. Keep bare `-` (stdin for `jet fmt -`); drop every
    // other dash-flag including short forms like `-u` / `-v` so they never become
    // the file target (D-TOOL4). D-CLI-BARE1=A: `-p <member>` and the
    // environment/package selectors also swallow their values — those values
    // are never positional file/program args.
    // `--output <name>` swallows its value the same way.
    let args: Vec<&String> = {
        let mut out = Vec::new();
        let mut skip_next = false;
        for a in jet_argv.iter() {
            if skip_next {
                skip_next = false;
                continue;
            }
            // `--project <dir>` swallows its value too (#2038): a project
            // directory is never the positional file/program arg.
            if a == "-p"
                || a == "--fixtures"
                || a == "--env"
                || a == "--preset"
                || a == "--set"
                || a == "--builder"
                || a == "--output"
                || a == "--profile"
                || a == "--target"
                || a == "--endpoint"
                || a == "--channel"
                || a == "--platform"
                || a == "--trust-key"
                || a == "--gate"
                || a == "--allow"
                || a == "--deny"
                || a == "--scope"
                || a == "--live"
                || a == "--replay"
                || a == "--project"
                || a == "--console-ttl"
                || a == "--app"
                || a == "--share"
                || a == "--token"
                || a == "--base-receipt"
                || a == "--receipt"
                || a == "--head-receipt"
                || a == "--after-receipt"
                || a == "--canvas-host"
                || a == "--canvas-port"
                || a == "--canvas-transport"
                || a == "--canvas-authority"
                || a == "--where"
                || a == "--capture"
                || a == "--browser"
                || a == "--browser-retries"
                || a == "--browser-reporter"
                || a == "--filter"
                || a == "--shuffle"
                || a == "--verify"
            {
                skip_next = true;
                continue;
            }
            if a.starts_with("--verify=") {
                continue;
            }
            if a.starts_with("--output=") {
                continue;
            }
            if a.starts_with("--where=") || a.starts_with("--capture=") {
                continue;
            }
            if a.starts_with("--endpoint=")
                || a.starts_with("--channel=")
                || a.starts_with("--platform=")
                || a.starts_with("--trust-key=")
            {
                continue;
            }
            if a.starts_with("--set=") {
                continue;
            }
            if a.starts_with("--scope=")
                || a.starts_with("--kind=")
                || a.starts_with("--live=")
                || a.starts_with("--replay=")
            {
                continue;
            }
            if a.starts_with("--target=")
                || a.starts_with("--app=")
                || a.starts_with("--share=")
                || a.starts_with("--token=")
            {
                continue;
            }
            if a.starts_with("--canvas-host=")
                || a.starts_with("--canvas-port=")
                || a.starts_with("--canvas-transport=")
                || a.starts_with("--canvas-authority=")
            {
                continue;
            }
            // The canonical arrow is also a valid `jet explain` query; keep
            // it as a positional token even though it begins with `-`.
            let explain_arrow_query = jet_argv.first().is_some_and(|arg| arg == "explain")
                && a == jet::Syntax::OP_UNIFIED_ARROW;
            if a.as_str() == "-" || !a.starts_with('-') || explain_arrow_query {
                out.push(a);
            }
        }
        out
    };
    define_direct_dispatch!(
        lsp_dispatch_route_words,
        dispatch_lsp;
        "lsp" => args.first().map(|s| s.as_str()) == Some("lsp") => {
            // #1659 c2 (round 2): `jet self lsp --help`/`-h` must print help, not
            // start the language server on stdio.
            if jet_argv.iter().any(|a| jet::CLI::is_help_flag(a)) {
                write_renderable(profile, &command_help("lsp"));
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
    );
    dispatch_lsp!();

    let cmd = match args.first() {
        Some(c) => c.as_str(),
        None => {
            // #1659 criterion 2: `jet --help`/`jet -h` are real requests for
            // the full command table, not the short orientation greeting.
            if jet_argv.iter().any(|a| jet::CLI::is_help_flag(a)) {
                write_renderable(profile, &usage());
                exit(ExitCodes::OK);
            }
            // No-args: a friendly greeting that orients, NOT a usage error.
            write_renderable(profile, &greeting());
            exit(ExitCodes::OK);
        }
    };
    let canvas_requested = jet_argv.iter().any(|arg| arg == jet::CLI::CANVAS_FLAG);
    let record_name = named_record_for_command(jet_argv, cmd, json);
    let no_capture = cmd == "dev" && jet_argv.iter().any(|arg| arg == "--no-capture");
    let debug_replay = named_debug_replay(jet_argv, cmd, json);
    if library_flag && cmd != "build" {
        crate::cli_error!(@fix "E2104", "`--lib` is only valid with `jet build`", "run `jet build --lib <file.jet>` to emit the native Library artifacts");
        exit(ExitCodes::USAGE);
    }
    if library_flag && cross_target.is_some() {
        crate::cli_error!(@fix "E2102", "`--lib` cannot be combined with `--target`", "remove `--target`; the native Library is built for the host ABI");
        exit(ExitCodes::USAGE);
    }
    if let Some(output) = output_name.as_deref() {
        let output_allowed = cmd == "run"
            || (cmd == "dev" && canvas_requested)
            || (cmd == "build" && library_flag)
            || cmd == "package";
        if !output_allowed || output.is_empty() {
            crate::cli_error!(@fix "E2104", "`--output` needs a runnable Output address or `jet build --lib`", format!("write `jet run --output <address> <file.{}>`, or `jet build --lib --output <name> <file.{}>`", jet::Syntax::FILE_EXT, jet::Syntax::FILE_EXT));
            exit(ExitCodes::USAGE);
        }
    }

    // D-OBSERVE-LIVE1=A: dev sessions expose bounded scheduler facts by
    // default; other executions opt in explicitly. Generated programs derive
    // their own PID and publish no payloads or secrets.
    if cmd == "dev" || raw.iter().any(|arg| arg == "--observe") {
        std::env::set_var("JET_OBSERVE", "1");
    }
    // Card #1895: every executed runtime tier inherits one project-scoped,
    // cross-run witness ledger path. Recording stays observational.
    CmdMemory::configure_ledger();
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
            let resolved = resolve_command_target(
                "run",
                cmd,
                flag_value(jet_argv, "-p"),
                mode,
                !raw.iter().any(|arg| arg == "--show-default"),
            );
            if jet_argv.iter().any(|arg| arg == "--show-default") && !mode.quiet {
                write_status(profile, "jet run: using stock default\n");
            }
            let program_args: Vec<&String> = if passthrough_sep.is_some() {
                passthrough.clone()
            } else {
                args.iter().skip(1).copied().collect()
            };
            let resolved_path = resolved.path.to_string_lossy().into_owned();
            let effective = effective_target("run", &resolved_path, cross_target.as_deref());
            reject_native_web_run("run", &resolved_path, effective.as_deref(), mode);
            let effective = native_run_target("run", &resolved_path, effective);
            run_native_execution(NativeExecutionRequest {
                command: "run",
                file: &resolved_path,
                emit_rust,
                emit_generated,
                library: library_flag,
                small,
                no_os,
                gates,
                build_grants: &build_grants,
                invocation_authority: invocation_authority.as_ref(),
                remote_builder: remote_builder.as_deref(),
                locked,
                target: effective.as_deref(),
                target_machine: selected_machine.as_ref(),
                explain_partition,
                verbose,
                sbom,
                release: release_flag,
                profile: named_profile.as_deref(),
                setting_overrides: &setting_overrides,
                output: output_name.as_deref(),
                program_args: &program_args,
                mode,
                output_profile: Some(&profile),
                record: record_name.as_deref(),
                interpret,
                entry_fn: resolved.callable.as_deref(),
                check_project_scope: false,
                package_scope: true,
                build_override: true,
                source_overlay: None,
            });
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
    // (`prove`/`budget`/`report`/`clean`/`update`/`image`/`trust`/
    // `devtools`/…) previously either error-taught E2102 or, worse, executed
    // for real — this is checked before the pinned-toolchain re-exec so a
    // help request never pays for one.
    const BESPOKE_DEEP_HELP: &[&str] = &["bind", "diff", "merge", "perf"];
    let owns_flags = jet::CLI::owns_flag_vocabulary(cmd);
    let wants_help = jet_argv.iter().any(|a| jet::CLI::is_help_flag(a));
    if wants_help && !(owns_flags && BESPOKE_DEEP_HELP.contains(&cmd)) {
        write_renderable(profile, &command_help(cmd));
        exit(ExitCodes::OK);
    }

    // Validate flags against the registry; an unknown/half-typed flag is E2102.
    // Skipped for commands that own a bespoke flag vocabulary or forward flags
    // downstream (so their flags aren't measured against the global set).
    if !owns_flags {
        check_flags(jet_argv, cmd);
    }
    if cmd == "build" {
        let verify_artifact = parse_build_verify(jet_argv).unwrap_or_else(|message| {
            crate::cli_error!(
                @fix "E2104",
                message,
                "run `jet build --verify <receipt-id>`"
            );
            exit(ExitCodes::USAGE);
        });
        if let Some(artifact_id) = verify_artifact.as_deref() {
            CmdCompile::run_build_verify(artifact_id, mode);
        }
    }
    if cmd == "build" && CmdCompile::foreign_build_import_requested(jet_argv) {
        CmdCompile::run_foreign_build_import(jet_argv, mode);
    }
    if cmd != "dev" && jet_argv.iter().any(|arg| arg == jet::CLI::CANVAS_FLAG) {
        crate::cli_error!(@fix "E2102", "`--canvas` is only valid with `jet dev`", "run `jet dev <file.jet> --canvas` to open the Canvas IDE");
        exit(ExitCodes::USAGE);
    }
    if let Some(status) = jet::ReceiptStore::run_if_needed(&raw) {
        exit(status);
    }
    // D-JPK-TOOLCHAIN1=A (#179): a version-pinned project hands off to its
    // pinned `jet` toolchain before any manifest-driven verb runs. A running
    // `jet` in the pinned channel runs natively; a genuine version mismatch
    // realizes the pinned prebuilt (never a source build) and re-execs into it.
    if matches!(cmd, "run" | "build" | "test" | "check" | "fill" | "jobs") {
        maybe_dispatch_pinned_toolchain(&raw, mode);
    }
    if cmd == "build"
        && args.get(1).is_none()
        && !jet_argv.iter().any(|argument| argument == "--show-default")
    {
        if let Some(status) = CmdCompile::run_selected_foreign_build(mode) {
            exit(status);
        }
    }
    // Commands with no required positional target.

    // D-CLI-BARE1=A: `-p <member>` picks a workspace member for the bare-entry
    // resolver below; declared here so its borrow outlives `target`.
    let bare_member_flag = flag_value(jet_argv, "-p");
    let named_build_entry = match args.get(1) {
        Some(f)
            if cmd == "build"
                && !Path::new(f.as_str()).is_dir()
                && checked_explicit_file(Path::new(f.as_str())).is_none() =>
        {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            match resolve_named_build_member(&cwd, f) {
                Ok(entry) => entry,
                Err(error) => report_build_resolution_error(error),
            }
        }
        _ => None,
    };
    let named_build_path = named_build_entry
        .as_ref()
        .map(|entry| entry.path.to_string_lossy().into_owned());
    let no_target = args.get(1).is_none();
    let target = match args.get(1) {
        Some(f) if cmd == "build" => named_build_path.as_deref().unwrap_or(f.as_str()),
        Some(f) => f.as_str(),
        None => "",
    };
    define_dispatch_routes! {
        "inspect" if args.get(1).map(|arg| arg.as_str()) == Some("env") => {
            let env_args = raw.iter().skip(2).cloned().collect::<Vec<_>>();
            run_env(&env_args, mode.json);
            return;
        }
        "codemod" => {
            let codemod_args = raw.iter().skip(1).cloned().collect::<Vec<_>>();
            CmdCodemod::run_codemod(&codemod_args);
            return;
        }
        "graph" => {
            let query_args = args.iter().skip(1).copied().collect::<Vec<_>>();
            run_build_query("graph", &query_args, mode);
            return;
        }
        "query" => {
            let query_args = args.iter().skip(1).copied().collect::<Vec<_>>();
            run_build_query("query", &query_args, mode);
            return;
        }
        "explain-build" => {
            let query_args = args.iter().skip(1).copied().collect::<Vec<_>>();
            run_build_query("explain-build", &query_args, mode);
            return;
        }
        "compiler" => {
            let operation = args.get(1).map(|arg| arg.as_str()).unwrap_or_default();
            let file = args.get(2).map(|arg| arg.as_str()).unwrap_or_default();
            run_compiler_api(operation, file, mode);
            return;
        }
        "impact" => {
            let impact_args = raw.iter().skip(1).cloned().collect::<Vec<_>>();
            run_impact(&impact_args, mode.json);
            return;
        }
        "provenance" => {
            let provenance_args = raw.iter().skip(1).cloned().collect::<Vec<_>>();
            run_provenance(&provenance_args, mode.json);
            return;
        }
        "digest" => {
            let digest_args = raw.iter().skip(1).cloned().collect::<Vec<_>>();
            run_digest(&digest_args, mode.json);
            return;
        }
        "inspect" => {
            let request = jet::CLI::parse_inspect_args(&raw).unwrap_or_else(|message| {
                crate::cli_error!(
                    @fix "E2104",
                    message,
                    "run `jet inspect <types|rights|claims|shapes|accel|decisions|structure|build|gates> [TARGET] [--live PID | --replay ARTIFACT]`"
                );
                exit(ExitCodes::USAGE);
            });
            run_inspect(
                &request,
                mode,
                gates,
                named_profile.as_deref().unwrap_or("debug"),
                &setting_overrides,
                no_os,
            );
            return;
        }
        "import" => {
            exit(CmdImport::run(&raw, mode.json));
        }
        "fix" if args.get(1).map(|arg| arg.as_str()) == Some("memory") => {
            CmdMemory::fix(&raw.iter().skip(2).cloned().collect::<Vec<_>>(), mode);
            return;
        }
        "budget" => {
            exit(CmdBudget::run(&raw));
        }
        // D-DEVR-STATUS1=A: project truth has one read-only home. It consumes
        // receipts; it never runs a producer to manufacture a green answer.
        "status" => {
            let status_args = raw.iter().skip(1).cloned().collect::<Vec<_>>();
            exit(CmdStatus::run_status(&status_args, mode.json));
        }
        "parts" => {
            run_project_parts(&raw, profile);
        }
        "reserved" => {
            if profile.machine_enabled() {
                write_machine(profile, &format!("{}\n", jet::CLI::reserved_report_json()));
            } else {
                write_renderable(profile, &jet::CLI::reserved_report_text());
            }
            return;
        }
        "prove" => {
            let prove_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_prove(&prove_args, mode.json);
            return;
        }
        "fill" => {
            let target = args.get(1).map(|arg| arg.as_str()).unwrap_or_else(|| {
                write_status(profile, &command_help("fill"));
                exit(ExitCodes::USAGE);
            });
            run_fill(target, mode);
            return;
        }
        "diff" => {
            run_diff(&raw);
            return;
        }
        "merge" => {
            run_merge(&raw);
            return;
        }
        "review" => {
            run_review(&raw, mode.json);
            return;
        }
        "remote" => {
            run_remote(&raw, mode);
        }
        "help" => {
            // `jet help <cmd>` renders the SAME per-command screen as
            // `jet <cmd> --help` (#2072): one renderer, so the two spellings
            // can never drift. `diff`/`merge` keep their bespoke deep help,
            // exactly as the `--help` path above does. Bare `jet help` is the
            // full inventory. An unknown word falls through `command_help`'s
            // teaching line rather than dumping the ~200-line global screen.
            if let Some(command) = raw.get(1) {
                if let Some(help) = structural_help(command) {
                    write_renderable(profile, &help);
                    exit(ExitCodes::OK);
                }
                write_renderable(profile, &command_help(command));
                exit(ExitCodes::OK);
            }
            write_renderable(profile, &usage());
            exit(ExitCodes::OK);
        }
        "learn" => {
            run_learn(&raw[1..], mode);
        }
        "doctor" => {
            let online = raw.iter().any(|a| a == "--online");
            let apply = raw.iter().any(|a| a == "--fix");
            run_doctor(online, apply, mode, cross_target.as_deref());
            return;
        }
        "exec" => {
            run_exec(&raw, mode);
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
            write_renderable(
                profile,
                &jet::CLI::man_page(env!("CARGO_PKG_VERSION")),
            );
            return;
        }
        "version" => {
            run_version(profile);
            return;
        }
        "self-update" => {
            run_self_update(&raw, mode);
        }
        "explain" => {
            if jet_argv.iter().any(|a| a == "--reload") {
                if jet_argv.iter().any(|a| a == "--web-graph") {
                    crate::cli_error!(
                        @fix "E2104",
                        "`--reload` and `--web-graph` are separate explain views",
                        "run one of `jet explain --reload <file.jet|dir>` or `jet explain --web-graph <file.jet>`"
                    );
                    exit(ExitCodes::USAGE);
                }
                if args.len() != 2 {
                    crate::cli_error!(
                        @fix "E2104",
                        "`jet explain --reload` accepts one source or project path",
                        "run `jet explain --reload <file.jet|dir>`"
                    );
                    exit(ExitCodes::USAGE);
                }
                run_explain_reload(args.get(1).map(|value| value.as_str()), mode, profile);
                return;
            }
            if jet_argv.iter().any(|a| a == "--web-graph") {
                run_explain_web_graph(&jet_argv[1..], mode);
                return;
            }
            if cost {
                run_explain_cost(
                    args.get(1).map(|value| value.as_str()),
                    mode,
                    named_profile.as_deref().unwrap_or("dev"),
                    &setting_overrides,
                );
                return;
            }
            if let (Some(subject), Some(file)) = (args.get(1), args.get(2)) {
                if subject.contains('.')
                    && file.ends_with(jet::Syntax::FILE_EXT)
                    && !jet::Explain::is_build_fact_query(subject)
                {
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
                        || jet::Explain::is_build_fact_query(value)
                        || jet::Explain::is_syntax_query(value)
                        || jet::Explain::lookup(value).is_some()
                })
                .unwrap_or(true)
            {
                run_explain(
                    code,
                    args.get(2).map(|s| s.as_str()),
                    mode,
                    named_profile.as_deref().unwrap_or("dev"),
                    &setting_overrides,
                );
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
            // D-ECO12=A: external language formatting belongs to the same
            // environment realization/trust path as `jet env`; native `.jet`
            // formatting stays in the compiler driver below.
            if jet_argv
                .iter()
                .any(|a| a == jet::Syntax::FMT_FLAG_LANG || a.starts_with("--lang="))
            {
                exit(run_external_fmt(&raw, mode));
            }
            // D-FMTPROJECT1=D: project-level formatter. No positional target
            // required — defaults to discovering the workspace/project root.
            let path_args: Vec<String> = args[1..].iter().map(|s| s.as_str().to_string()).collect();
            let stdin_mode = path_args.iter().any(|p| p == "-");
            let stdin_path: Option<String> = jet_argv
                .iter()
                .find_map(|a| a.strip_prefix("--stdin-path=").map(str::to_string));
            let show_diff = jet_argv.iter().any(|a| a == "--diff") || dry_run;
            let changed_only = jet_argv.iter().any(|a| a == "--changed");
            let explicit_copies = jet_argv.iter().any(|a| a == "--explicit-copies");
            let explicit_paths: Vec<String> = path_args.into_iter().filter(|p| p != "-").collect();
            run_fmt(
                &explicit_paths,
                stdin_mode,
                stdin_path.as_deref(),
                fmt_check || dry_run,
                show_diff,
                changed_only,
                explicit_copies,
                fmt_simplify,
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
            run_fetch(locked || jet_argv.iter().any(|a| a == "--offline"));
            return;
        }
        "update" => {
            // D-JPK-TOOLCHAIN1=A (#179): `jet update jet [<channel>]` moves the
            // toolchain pin; anything else refreshes moving dependency selectors.
            if args.get(1).map(|s| s.as_str()) == Some("jet") {
                run_update_jet(args.get(2).map(|s| s.as_str()), mode);
            }
            let dep = args.get(1).map(|s| s.as_str());
            run_update(dep);
            return;
        }
        "toolchain" => {
            run_toolchain(mode);
        }
        // U11 (D-JPK-SCRIPTDEP1=A): `jet init <script.jet>` lifts that
        // script's inline `use pkg#version;` deps into the freshly written
        // `package.jet`; bare `jet init` is unchanged.
        "init" => {
            run_init(args.get(1).map(|s| s.as_str()), &raw, mode);
        }
        "split" => {
            run_split(&args, &raw, mode);
        }
        "Fold" => {
            run_fold(&args, &raw, mode);
        }
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
                        "`jet clean` is the sole package-store cleanup entry (D-CLI-STORE2=A)"
                            .to_string(),
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
        },
        "publish" => {
            let force = raw.iter().any(|a| a == "--force");
            // c146 (D-PKGSIGN1): sign by default; --no-sign opts out.
            let no_sign = raw.iter().any(|a| a == "--no-sign");
            let foreign_registry = flag_value(&raw, "--to");
            run_publish(force, no_sign, foreign_registry, mode);
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
                    // No typed edit: this is an argv subcommand, not Jet source.
                    crate::cli_error!("E2101", "unknown `jet registry key` subcommand `{}` — did you mean `jet registry key backup`?", other);
                    exit(ExitCodes::USER_ERROR);
                }
                None => {
                    crate::cli_error!(
                        "E2104",
                        "`jet registry key` needs a subcommand — try `jet registry key backup`."
                    );
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
        },
        "db" => {
            let db_args: Vec<String> = jet_argv.iter().skip(1).cloned().collect();
            exit(run_db(&db_args, mode));
        }
        "semindex" => {
            // D-SEMINDEX1: stable semantic-index JSON smoke surface.
            let semindex_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_semindex(&semindex_args, mode.json);
            return;
        }
        "output" => {
            let output_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_output(&output_args, mode.json);
            return;
        }
        "find" => {
            let find_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_find(&find_args, mode.json);
            return;
        }
        "jobs" => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let member_flag = flag_value(jet_argv, "-p");
            let entry = resolve_bare_entry("run", &cwd, member_flag, mode, true)
                .unwrap_or_else(|| missing_bare_entry("run", &cwd, mode));
            let registry = job_registry_for_entry(&entry.path, mode);
            validate_job_registry(&entry.path, &registry, mode);
            let job_options = jet_argv;
            let graph_requested = job_options.iter().any(|argument| argument == "--graph");
            let status_requested = job_options.iter().any(|argument| argument == "--status");
            let explain_requested = job_options.iter().any(|argument| argument == "--explain");
            let watch_requested = run_wants_watch(job_options);
            let inspection_requested =
                graph_requested || status_requested || explain_requested;
            if inspection_requested {
                let requested = args.get(1).map(|value| value.as_str());
                render_job_inspection(
                    &entry.path,
                    &registry,
                    profile,
                    requested,
                    graph_requested,
                    status_requested,
                    explain_requested,
                );
                return;
            }
            let job_program_args = if passthrough_sep.is_some() {
                passthrough.iter().map(|arg| (*arg).clone()).collect::<Vec<_>>()
            } else {
                args.iter().skip(2).map(|arg| (*arg).clone()).collect::<Vec<_>>()
            };
            if watch_requested {
                validate_job_working_directories(&entry.path, &registry, mode);
            }
            if watch_requested {
                let requested = args.get(1).map(|value| value.as_str());
                run_job_watch(
                    &entry.path,
                    &registry,
                    requested,
                    job_options,
                    &job_program_args,
                    mode,
                    profile,
                );
            }
            if let Some(name) = args.get(1).map(|value| value.as_str()) {
                if registry.find_visible(name).is_none() {
                    let declared = registry.completion_words();
                    let fix = if declared.is_empty() {
                        "mark a function `#Job`, or check the spelling".to_string()
                    } else {
                        format!(
                            "check the spelling; declared jobs: {}",
                            declared.join(", ")
                        )
                    };
                    crate::emit_cli_report(
                        "E1294",
                        format!("No job named `{name}`."),
                        "`jet jobs` only invokes visible checked `#Job fn`s in the selected entry"
                            .to_string(),
                        fix,
                        mode.json,
                    );
                    exit(ExitCodes::USER_ERROR);
                }
                validate_job_working_directories(&entry.path, &registry, mode);
                let name = name.to_string();
                let mut job_args = vec![name];
                job_args.extend(job_program_args);
                let program_args = job_args.iter().collect::<Vec<_>>();
                let entry_str = entry.path.to_string_lossy().into_owned();
                let effective = effective_target("run", &entry_str, cross_target.as_deref());
                reject_native_web_run("run", &entry_str, effective.as_deref(), mode);
                let effective = native_run_target("run", &entry_str, effective);
                run_native_execution(NativeExecutionRequest {
                    command: "run",
                    file: &entry_str,
                    emit_rust,
                    emit_generated,
                    library: library_flag,
                    small,
                    no_os,
                    gates,
                    build_grants: &build_grants,
                    invocation_authority: invocation_authority.as_ref(),
                    remote_builder: remote_builder.as_deref(),
                    locked,
                    target: effective.as_deref(),
                    target_machine: selected_machine.as_ref(),
                    explain_partition,
                    verbose,
                    sbom,
                    release: release_flag,
                    profile: named_profile.as_deref(),
                    setting_overrides: &setting_overrides,
                    output: output_name.as_deref(),
                    program_args: &program_args,
                    mode,
                    output_profile: Some(&profile),
                    record: record_name.as_deref(),
                    interpret,
                    entry_fn: None,
                    check_project_scope: false,
                    package_scope: true,
                    build_override: true,
                    source_overlay: None,
                });
            } else {
                render_job_registry(&registry, profile);
            }
            return;
        }
        "generate" => {
            // D-DX-GENERATE1=A: source generation is an explicit command; it
            // never runs as a build/check/test side effect.
            let generate_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_generate(&generate_args, mode.json);
            return;
        }
        "expand" => {
            // D-EXPANDCLI1=A: `jet inspect expand --facts <lens> <file>` / bare
            // `jet inspect expand <file>` — the transparency command (card #183).
            let expand_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_expand(&expand_args, mode.json);
            return;
        }
        "audit" => {
            if raw.get(1).map(String::as_str) == Some("memory") {
                CmdMemory::audit(&raw[2..], mode);
                return;
            }
            if raw.get(1).map(String::as_str) == Some("copies") {
                run_copy_audit(&raw[2..], mode.json);
                return;
            }
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
            // env`, forwarding flags and any trailing `-- cmd`.
            // D-JPK-DISPATCH1=B (A1): execs the `jetpack` engine binary by
            // name — never linked in-process — so the compiler binary stays
            // standalone-checkable (deleting `jetpack` still leaves `jet
            // build`/`jet run` fully working).
            let mut fwd = raw.clone();
            if let Some(pos) = fwd.iter().position(|a| a == "env") {
                fwd.remove(pos);
            }
            fwd.insert(0, "env".to_string());
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
            // U16 (card c9jetpackgates): `jet os bridge flake` translates a
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
            // U12 (card c9jetpackgates): `jet services up/down/health/logs/wait`
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
            if let Some(argument) = invalid_repl_console_flag(jet_argv) {
                crate::cli_error!(
                    @fix "E2102",
                    format!("unknown `jet repl` console option `{argument}`"),
                    "use `--console`, `--sandbox data`, or `--console-ttl <milliseconds>"
                );
                exit(ExitCodes::USAGE);
            }
            let project = jet_argv
                .iter()
                .find_map(|a| a.strip_prefix("--project=").map(str::to_string))
                .or_else(|| flag_value(jet_argv, "--project").map(str::to_string));
            if console_requested(jet_argv) {
                exit(run_console(
                    project.as_deref(),
                    jet_argv,
                    mode,
                    &authority_allow,
                    &authority_deny,
                    gates,
                    named_profile.as_deref(),
                    &setting_overrides,
                ));
            }
            let allow = authority_allow.clone();
            let deny = authority_deny.clone();
            // #2038: the first positional is a session preload file. `args`
            // (not `raw`) is the positional-only view, so project and authority
            // option values never land here.
            let preload = args.get(1).map(|file| file.as_str());
            run_repl(project.as_deref(), preload, &allow, &deny, mode.color);
            return;
        }
        "notebook" => {
            // D-NOTEBOOK-SURFACE1=D: shared REPL session + .jetnb / Jupyter.
            CmdNotebook::run_notebook(&raw);
            return;
        }
        "package" => {
            exit(run_package(jet_argv, mode));
        }
        "flash" => {
            exit(run_flash(&raw, mode));
        }
        // Teaching error: E0043 `jet install` -> `jet fetch`
        "install" => {
            emit_cli_row("E0043", &[], mode.json);
            exit(ExitCodes::USER_ERROR);
        }
        "dev" => {
            // D-RUN-LAW1=A: every dev verb runs one program; `jet dev` uses
            // the file-scoped watcher below. Project-level development
            // remains a Jet operation.
            // E2-M4 (D-DEV4): re-check and re-run the entry file on every save,
            // streaming output for sub-200ms feedback. The interpreter is a dev
            // convenience only — `jet build`/`jet run` never touch it (I2/I3).
            let try_anyway = raw.iter().any(|a| a == "--try-anyway");
            let canvas_options = canvas_requested.then(|| {
                canvas_options_from_args(
                    jet_argv,
                    output_name.as_deref(),
                    requested_target.as_deref(),
                )
            });
            // c139 (D-JIT2=A): --interpret forces tier-0 interpreter; otherwise
            // CraneliftBackend wraps it (M0 delegates, M1+ JIT-compiles).
            let use_interpreter = raw.iter().any(|a| a == "--interpret");
            // c77 (D-DEVMODE1=A): default auto-detect; experts force a mode with
            // --restart / --swap / --watch=off.
            let policy = watch_policy_from(&raw, WatchPolicy::Auto);
            let bare_member = flag_value(jet_argv, "-p");
            let resolved = match args.get(1) {
                Some(f) => resolve_command_target(
                    "dev",
                    f,
                    bare_member,
                    mode,
                    !jet_argv.iter().any(|arg| arg == "--show-default"),
                ),
                None => {
                    let entry = match resolve_bare_entry(
                        "dev",
                        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                        bare_member,
                        mode,
                        true,
                    ) {
                        Some(entry) => entry,
                        None => {
                            crate::cli_error!(
                                "E2104",
                                "`jet dev` needs a file to watch: {} dev <file.{}>",
                                jet::Syntax::BINARY_NAME,
                                jet::Syntax::FILE_EXT
                            );
                            exit(ExitCodes::USAGE);
                        }
                    };
                    entry
                }
            };
            let file = resolved.path.to_string_lossy().into_owned();
            if CmdLocalApp::app_requested(&raw) {
                let function = CmdLocalApp::app_function(&raw).unwrap_or_else(|| {
                    crate::cli_error!(
                        @fix "E2104",
                        "`jet dev --app` needs a function name",
                        "run `jet dev <file.jet> --app <function>`"
                    );
                    exit(ExitCodes::USAGE);
                });
                CmdLocalApp::run_local_app(
                    Path::new(&file),
                    function,
                    CmdLocalApp::app_share(&raw),
                    flag_value(&raw, "--token"),
                    mode.json,
                );
            }
            // E2-M15: `jet dev` has the same target validation contract as
            // build/run, even when its execution tier is the native watcher.
            // A target flag must never disappear merely because dev selects a
            // different runner branch below.
            if let Some(target) = cross_target.as_deref() {
                validate_target(target, mode);
            }
            offer_dev_environment(&raw, &file, &output);
            require_project_environment("dev", Path::new(&file), mode);
            if jet_argv.iter().any(|arg| arg == "--show-default") && !mode.quiet {
                write_status(profile, "jet dev: using stock default\n");
            }
            let dev_profile = if no_os {
                BuildProfile::NoOs
            } else if small {
                BuildProfile::Small
            } else if let Some(profile) = named_profile.as_deref() {
                resolve_named_profile(profile, &file, mode)
            } else {
                BuildProfile::default_for_command("dev")
            };
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
            if has_dev_entry_fn(&file) && !jet_argv.iter().any(|arg| arg == "--show-default") {
                run_dev_entry(
                    &file,
                    dev_profile,
                    mode,
                    &setting_overrides,
                    &passthrough,
                    record_name.as_deref(),
                );
                return;
            }
            if entry_returns_app(&file) {
                if use_interpreter {
                    std::env::set_var("JET_APP_DEV", "1");
                    let dev_file = std::fs::canonicalize(&file)
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|_| file.clone());
                    std::env::set_var("JET_DEV_FILE", dev_file);
                    if let Some(port) = dev_port {
                        std::env::set_var("JET_APP_PORT", port.to_string());
                    }
                    run_dev(
                        &file,
                        resolved.callable.as_deref(),
                        try_anyway,
                        policy,
                        gates,
                        mode,
                        true,
                        named_profile.as_deref().unwrap_or("dev"),
                        &setting_overrides,
                        &passthrough,
                        record_name.as_deref(),
                        no_capture,
                        canvas_requested,
                        canvas_options.clone(),
                    );
                }
                run_web_app_dev_entry(
                    &file,
                    mode,
                    dev_port,
                    named_profile.as_deref(),
                    &setting_overrides,
                    record_name.as_deref(),
                    &passthrough,
                );
                return;
            }
            // c134 Phase 7: `jet dev <file> --target=web` compiles to JS/WASM
            // and serves `build/` with browser live-reload — a completely
            // different execution model from the native interpret/hot-swap
            // loop above, so it's a separate function, not a new branch
            // inside `run_dev`'s interpreter machinery.
            // D-WEBDEFAULT1 (ratified 2026-07-01, c134): no explicit --target= falls back to the
            // file's own `#Target(Web)` marker, if any.
            if effective_target("dev", &file, cross_target.as_deref()).as_deref()
                == Some(jet::Syntax::BUILD_TARGET_WEB)
            {
                run_dev_web(
                    &file,
                    &dev_profile,
                    mode,
                    verbose,
                    dev_port,
                    canvas_requested,
                    canvas_options,
                    &setting_overrides,
                );
                return;
            }
            run_dev(
                &file,
                resolved.callable.as_deref(),
                try_anyway,
                policy,
                gates,
                mode,
                use_interpreter,
                named_profile.as_deref().unwrap_or("dev"),
                &setting_overrides,
                &passthrough,
                record_name.as_deref(),
                no_capture,
                canvas_requested,
                canvas_options,
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
            // entry the same way run/build/check/dev do; outside a package
            // the usage error is unchanged.
            let file: String = match args.get(1) {
                Some(f) => resolve_command_target("debug", f, flag_value(jet_argv, "-p"), mode, false)
                    .path
                    .to_string_lossy()
                    .into_owned(),
                None => {
                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    let member_flag = flag_value(jet_argv, "-p");
                    match resolve_bare_entry("debug", &cwd, member_flag, mode, false) {
                        Some(entry) => entry.path.to_string_lossy().into_owned(),
                        None => {
                            crate::cli_error!(
                                "E2104",
                                "`jet debug` needs a file to debug: {} debug <file.{}>",
                                jet::Syntax::BINARY_NAME,
                                jet::Syntax::FILE_EXT
                            );
                            exit(ExitCodes::USAGE);
                        }
                    }
                }
            };
            let resolved = resolve_source_path(&file);
            if record_name.is_some() && debug_replay.is_some() {
                crate::ProveReplay::emit_diag(
                    "E2104",
                    "debug accepts either recording or replay",
                    "`--record` and `--replay` cannot be used in the same debug session",
                    "choose one of `--record=NAME` or `--replay=NAME`",
                    mode.json,
                );
                exit(ExitCodes::USAGE);
            }
            if dap && debug_replay.is_some() {
                crate::ProveReplay::emit_diag(
                    "E2203",
                    "replay debugging is unavailable for native sessions",
                    "the native LLDB/DAP backend cannot browse an interpreter recording",
                    "replay an interpreter receipt without `--dap` or native-only source features",
                    mode.json,
                );
                exit(ExitCodes::USER_ERROR);
            }
            // Open and validate a replay before selecting a live backend. The
            // receipt carries authoritative source history, so native-only
            // constructs in the current source must not turn a validated
            // no-execution replay into a live native session. `open_named_replay`
            // still performs the strict source/MIR identity checks.
            let replay = debug_replay.as_deref().map(|path| {
                crate::ProveReplay::open_named_replay(
                    &resolved,
                    path,
                    named_profile.as_deref().unwrap_or("dev"),
                    &setting_overrides,
                    mode.json,
                )
                .unwrap_or_else(|status| exit(status))
            });
            let use_native =
                dap || (replay.is_none() && jet::Debug::needs_native(&resolved).unwrap_or(false));
            if use_native {
                if record_name.is_some() {
                    crate::ProveReplay::emit_diag(
                        "E2203",
                        "recorded reverse debugging is unavailable for native sessions",
                        "the native LLDB/DAP backend has no recorded Jet source history",
                        "remove `--record`, or debug a source program within the interpreter boundary",
                        mode.json,
                    );
                    exit(ExitCodes::USER_ERROR);
                }
                if debug_replay.is_some() {
                    crate::ProveReplay::emit_diag(
                        "E2203",
                        "replay debugging is unavailable for native sessions",
                        "the native LLDB/DAP backend cannot browse an interpreter recording",
                        "replay an interpreter receipt without `--dap` or native-only source features",
                        mode.json,
                    );
                    exit(ExitCodes::USER_ERROR);
                }
                exit(run_debug_native(&resolved, raw_frames, dap, mode));
            }
            if let Some(name) = record_name.as_deref() {
                let capture = crate::ProveReplay::begin_named_capture(
                    &resolved,
                    name,
                    named_profile.as_deref().unwrap_or("dev"),
                    &setting_overrides,
                    mode.json,
                )
                .unwrap_or_else(|status| exit(status));
                let execution = jet::Debug::run_debug_recorded(&resolved);
                if let Err(status) =
                    crate::ProveReplay::finish_named_capture_with_run(
                        &capture,
                        execution.exit_code,
                        mode.json,
                        &execution.run,
                    )
                {
                    exit(status);
                }
                exit(execution.exit_code);
            }
            if let Some(replay) = replay {
                exit(jet::Debug::run_debug_with_recording(
                    &resolved,
                    replay.recorded_run,
                ));
            }
            exit(jet::Debug::run_debug(&resolved));
        }
        // D-JPK-CACHECONFIG1=D: cache status, pruning, and host limits are
        // artifact-store CLI operations owned by CmdStatus.
        "cache" => {
            exit(CmdStatus::run_cache(&raw[1..], mode.json));
        }
        "shared-store" => {
            exit(EngineDispatch::dispatch(
                jet::Syntax::JETPACK_BINARY_NAME,
                "shared-store",
                &raw,
            ));
        }
        "eval" => {
            // S60 / D-PURE1 (E2-M16): deterministic evaluation of pure Jet.
            let pure_flag = raw.iter().any(|a| a == "--pure");
            let argument = match args.get(1) {
                Some(f) => f.as_str(),
                None => {
                    crate::cli_error!(
                        "E2104",
                        "`jet eval` needs a file or an expression: {} eval <file.{} | expression>",
                        jet::Syntax::BINARY_NAME,
                        jet::Syntax::FILE_EXT
                    );
                    exit(ExitCodes::USAGE);
                }
            };
            // #2068: one argument, two shapes. `looks_like_jet_source` is the
            // same predicate the bare-entry sugar uses, so a file, a project
            // directory, or a bare stem all keep the file form — including a
            // `.jet` name that is NOT there, which stays a file-not-found
            // error rather than being re-read as an expression. Anything else
            // is an expression, which the registry summary has always
            // promised ("Evaluate pure Jet and print JSON").
            if looks_like_jet_source(argument) {
                run_eval(&resolve_source_path(argument), pure_flag, mode);
                return;
            }
            // A `:meta` word is REPL vocabulary, not an expression: say so
            // instead of handing it to the parser as broken source.
            if argument.starts_with(':') {
                crate::cli_error!(@fix "E2104", format!("`{}` is a {} command, not an expression", argument, jet::Syntax::BINARY_NAME), format!("run `{} repl` for an interactive session, or pass an expression such as `1 + 2`", jet::Syntax::BINARY_NAME));
                exit(ExitCodes::USAGE);
            }
            run_eval_expression(argument, mode);
            return;
        }
        _ => {}
    }
    define_bare_dispatch_routes! {
        "run" | "build" | "test" | "check" | "dev" | "doc" => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            if let Some(entry) = resolve_bare_entry(
                cmd,
                &cwd,
                bare_member_flag,
                mode,
                !jet_argv.iter().any(|arg| arg == "--show-default"),
            ) {
                let entry_str = entry.path.to_string_lossy().to_string();
                match cmd {
                    "doc" => {
                        run_doc(
                            &entry_str,
                            mode,
                            jet_argv.iter().any(|arg| arg == "--check"),
                        );
                        return;
                    }
                    "test" => {
                        // Spec S43: bare `jet test` collects every
                        // `#Test` in the package, so the target is the
                        // package the resolved entry belongs to — the
                        // same project shape as the package test resolver
                        // below, with member selection unchanged.
                        let entry_dir = entry
                            .path
                            .parent()
                            .filter(|path| !path.as_os_str().is_empty())
                            .unwrap_or_else(|| Path::new("."));
                        let root = jet::Loader::find_manifest_root(entry_dir)
                            .unwrap_or_else(|| entry_dir.to_path_buf());
                        let test_opts =
                            TestRunOpts::parse(jet_argv, mode, &setting_overrides);
                        if test_opts.show_default && !mode.quiet {
                            write_status(profile, "jet test: using stock default\n");
                        }
                        run_test_opts(
                            &root.to_string_lossy(),
                            test_opts,
                            mode,
                        );
                        return;
                    }
                    "dev" => {
                        let try_anyway = jet_argv.iter().any(|a| a == "--try-anyway");
                        let use_interpreter = jet_argv.iter().any(|a| a == "--interpret");
                        let policy = watch_policy_from(&raw, WatchPolicy::Auto);
                        let canvas_options = canvas_requested.then(|| {
                            canvas_options_from_args(
                                jet_argv,
                                output_name.as_deref(),
                                requested_target.as_deref(),
                            )
                        });
                        run_dev(
                            &entry_str,
                            entry.callable.as_deref(),
                            try_anyway,
                            policy,
                            gates,
                            mode,
                            use_interpreter,
                            named_profile.as_deref().unwrap_or("dev"),
                            &setting_overrides,
                            &passthrough,
                            record_name.as_deref(),
                            no_capture,
                            canvas_requested,
                            canvas_options,
                        );
                        return;
                    }
                    _ => {
                        if jet_argv.iter().any(|arg| arg == "--show-default")
                            && !mode.quiet
                        {
                            write_status(profile, &format!("jet {cmd}: using stock default\n"));
                        }
                        // D-CLI1: use passthrough slice if `--` was present;
                        // otherwise fall back to positional words after the subcommand.
                        let program_args: Vec<&String> = if passthrough_sep.is_some() {
                            passthrough.clone()
                        } else {
                            args.iter().skip(1).copied().collect()
                        };
                        if cmd == "run" && run_wants_watch(&raw) {
                            prepare_project_environment("run", Path::new(&entry_str), mode);
                            let try_anyway = raw.iter().any(|a| a == "--try-anyway");
                            let use_interpreter = raw.iter().any(|a| a == "--interpret");
                            run_dev(
                                &entry_str,
                                entry.callable.as_deref(),
                                try_anyway,
                                WatchPolicy::Restart,
                                gates,
                                mode,
                                use_interpreter,
                                named_profile.as_deref().unwrap_or("dev"),
                                &setting_overrides,
                                &program_args,
                                record_name.as_deref(),
                                false,
                                false,
                                None,
                            );
                            return;
                        }
                        let effective =
                            effective_target(cmd, &entry_str, cross_target.as_deref());
                        reject_native_web_run(cmd, &entry_str, effective.as_deref(), mode);
                        let effective = native_run_target(cmd, &entry_str, effective);
                        run_native_execution(NativeExecutionRequest {
                            command: cmd,
                            file: &entry_str,
                            emit_rust,
                            emit_generated,
                            library: library_flag,
                            small,
                            no_os,
                            gates,
                            build_grants: &build_grants,
                            invocation_authority: invocation_authority.as_ref(),
                            remote_builder: remote_builder.as_deref(),
                            locked,
                            target: effective.as_deref(),
                            target_machine: selected_machine.as_ref(),
                            explain_partition,
                            verbose,
                            sbom,
                            release: release_flag,
                            profile: named_profile.as_deref(),
                            setting_overrides: &setting_overrides,
                            output: output_name.as_deref(),
                            program_args: &program_args,
                            mode,
                            output_profile: Some(&profile),
                            record: record_name.as_deref(),
                            interpret,
                            entry_fn: entry.callable.as_deref(),
                            check_project_scope: cmd == "check",
                            package_scope: cmd != "build"
                                || !jet_argv.iter().any(|arg| arg == "--show-default"),
                            build_override: cmd != "build"
                                || !jet_argv.iter().any(|arg| arg == "--show-default"),
                            source_overlay: None,
                        });
                        return;
                    }
                }
            } else {
                missing_bare_entry(cmd, &cwd, mode);
            }
        }
        _ => {
            // #2072: a target-requiring verb invoked bare (`jet fix`,
            // `jet lint`, `jet fuzz`, …) wants its OWN usage, not the
            // whole command inventory. Same registry renderer as
            // `jet <cmd> --help`; stderr + exit 2 because this is a
            // usage error, not a help request.
            write_status(profile, &command_help(cmd));
            exit(ExitCodes::USAGE);
        }
    }
    define_late_dispatch_routes! {
        "try" => {
            let keep = jet_argv.iter().any(|arg| arg == "--keep");
            run_try(target, keep, mode.json);
        }
        "fix" => {
            let edition = jet_argv
                .iter()
                .find_map(|a| a.strip_prefix("--edition=").map(str::to_string));
            let all = jet_argv.iter().any(|a| a == "--all");
            run_fix(target, dry_run, edition.as_deref(), all, mode);
        }
        "new" if matches!(
            args.get(1).map(|arg| arg.as_str()),
            Some("service" | "route" | "job" | "migration")
        ) =>
        {
            let kind = args.get(1).map(|arg| arg.as_str()).unwrap_or_default();
            let name = args.get(2).copied().unwrap_or_else(|| {
                crate::cli_error!(
                    @fix "E2104",
                    format!("`jet new {kind}` needs a name"),
                    format!("run `jet new {kind} <name> --preview`")
                );
                exit(ExitCodes::USAGE);
            });
            run_new_backend(kind, name, jet_argv, mode);
        }
        "new" if args.get(1).map(|arg| arg.as_str()) == Some("game") => {
            let name = args.get(2).copied().unwrap_or_else(|| {
                crate::cli_error!(
                    @fix "E2104",
                    "`jet new game` needs a project name",
                    "run `jet new game my_game`"
                );
                exit(ExitCodes::USAGE);
            });
            if args.len() > 3 {
                crate::cli_error!(
                    @fix "E2104",
                    "`jet new game` accepts one project name",
                    "run `jet new game my_game`"
                );
                exit(ExitCodes::USAGE);
            }
            CmdGame::run_new_game(name, mode);
        }
        "new" => {
            let name = args.get(1).map(|arg| arg.as_str()).unwrap_or_else(|| {
                crate::cli_error!(
                    @fix "E2104",
                    "`jet new` needs a project name",
                    "run `jet new my_app`"
                );
                exit(ExitCodes::USAGE);
            });
            run_new(name, annotated, web_scaffold, mode);
        }
        "test-compare" => {
            CmdTest::run_test_compare(target, jet_argv, mode);
        }
        "test" => {
            if let Some(name) = test_option_value(jet_argv, "--browser-scaffold") {
                let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                match scaffold_browser_tests(&root, &name) {
                    Ok(project) => {
                        write_mode_status(
                            mode,
                            &format!("created browser test project `{}`\n", project.display()),
                        );
                        exit(ExitCodes::OK);
                    }
                    Err(error) => {
                        crate::cli_error!(
                            @fix "E2104",
                            error,
                            "use `jet test --browser-scaffold=<new-project-name>`"
                        );
                        exit(ExitCodes::USER_ERROR);
                    }
                }
            }
            let test_opts = TestRunOpts::parse(jet_argv, mode, &setting_overrides);
            if test_opts.show_default && !mode.quiet {
                write_status(profile, "jet test: using stock default\n");
            }
            // Keep directory targets intact so package tests/checks are
            // collected together instead of resolving to one run entry.
            let target_path = Path::new(target);
            let resolved = if target_path.is_dir() {
                target.to_string()
            } else {
                resolve_source_path(target)
            };
            run_test_opts(&resolved, test_opts, mode);
        }
        "add" => {
            run_add(&raw);
        }
        "remove" => {
            run_remove(target);
        }
        // D-TOOL3 (E2-M11): `jet emit --rust` — print the generated Rust source.
        "emit" => {
            let rust_flag = jet_argv.iter().any(|a| a == "--rust");
            if !rust_flag {
                emit_cli_report(
                    "E2102",
                    format!(
                        "usage: {} emit --rust <file.{}>",
                        jet::Syntax::BINARY_NAME,
                        jet::Syntax::FILE_EXT
                    ),
                    "`emit` needs a mode flag; today only `--rust` is supported".to_string(),
                    format!(
                        "run `{} emit --rust <file.{}>` to print the generated Rust",
                        jet::Syntax::BINARY_NAME,
                        jet::Syntax::FILE_EXT
                    ),
                    false,
                );
                exit(ExitCodes::USAGE);
            }
            let emit_metadata = jet_argv.iter().any(|a| a == "--metadata");
            run_emit_rust(target, mode, emit_metadata);
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
        "doc" => {
            run_doc(target, mode, jet_argv.iter().any(|arg| arg == "--check"));
        }
        // Expert-only lint categories. Ordinary build/check never run these
        // opt-in policy views.
        "lint" => {
            let categories = usize::from(a11y) + usize::from(complexity) + usize::from(cost);
            if categories != 1 {
                emit_cli_report(
                    "E2102",
                    format!(
                        "usage: {} lint --a11y|--complexity|--cost <file.{}>",
                        jet::Syntax::BINARY_NAME,
                        jet::Syntax::FILE_EXT
                    ),
                    "`lint` needs exactly one category flag".to_string(),
                    format!(
                        "run `{} lint --a11y <file.{}>`, `{} lint --complexity <file.{}>`, or `{} lint --cost <file.{}>`",
                        jet::Syntax::BINARY_NAME,
                        jet::Syntax::FILE_EXT,
                        jet::Syntax::BINARY_NAME,
                        jet::Syntax::FILE_EXT,
                        jet::Syntax::BINARY_NAME,
                        jet::Syntax::FILE_EXT,
                    ),
                    false,
                );
                exit(ExitCodes::USAGE);
            }
            let resolved = resolve_source_path(target);
            if complexity {
                let max_budget = jet_argv.iter().find_map(|arg| arg.strip_prefix("--max="));
                let max_budget = max_budget.map(|value| match value.parse::<u32>() {
                    Ok(value) => value,
                    Err(_) => {
                        crate::cli_error!(
                            @fix "E2104",
                            format!("`--max={value}` isn't a non-negative integer"),
                            "use `--max=<n>` with a non-negative integer"
                        );
                        exit(ExitCodes::USAGE);
                    }
                });
                run_lint_complexity(&resolved, mode, max_budget);
            } else if cost {
                run_lint_cost(&resolved, mode);
            } else {
                run_lint_a11y(&resolved, mode);
            }
        }
        _ ["run" | "build" | "check"] => {
            // D-CLI1: use passthrough slice if `--` was present; otherwise fall
            // back to positional words after the file (args[0]=cmd, args[1]=file).
            let program_args: Vec<&String> = if passthrough_sep.is_some() {
                passthrough.clone()
            } else {
                args.iter().skip(2).copied().collect()
            };
            // Ext-optional CLI: `jet run examples/test` resolves to `examples/test.jet`
            // for the path-accepting compile commands.
            let named_build_target = named_build_entry.is_some();
            let resolved = if cmd == "build" {
                named_build_entry.unwrap_or_else(|| {
                    resolve_command_target(
                        cmd,
                        target,
                        bare_member_flag,
                        mode,
                        !jet_argv.iter().any(|arg| arg == "--show-default"),
                    )
                })
            } else if matches!(cmd, "run" | "check") {
                resolve_command_target(
                    cmd,
                    target,
                    bare_member_flag,
                    mode,
                    !jet_argv.iter().any(|arg| arg == "--show-default"),
                )
            } else {
                ResolvedEntry::file(PathBuf::from(target))
            };
            let resolved_path = resolved.path.to_string_lossy().into_owned();
            if cmd == "run" {
                // #439 / E3-UL6: `jet run --watch` uses the shared dependency-
                // aware engine; `jet dev` keeps the richer swap/overlay surface.
                if run_wants_watch(&raw) {
                    prepare_project_environment("run", Path::new(&resolved_path), mode);
                    let try_anyway = raw.iter().any(|a| a == "--try-anyway");
                    let use_interpreter = raw.iter().any(|a| a == "--interpret");
                    run_dev(
                        &resolved_path,
                        resolved.callable.as_deref(),
                        try_anyway,
                        WatchPolicy::Restart,
                        gates,
                        mode,
                        use_interpreter,
                        named_profile.as_deref().unwrap_or("dev"),
                        &setting_overrides,
                        &program_args,
                        record_name.as_deref(),
                        false,
                        false,
                        None,
                    );
                    return;
                }
            }
            if jet_argv.iter().any(|arg| arg == "--show-default") && !mode.quiet {
                write_status(profile, &format!("jet {cmd}: using stock default\n"));
            }
            let effective = effective_target(cmd, &resolved_path, cross_target.as_deref());
            reject_native_web_run(cmd, &resolved_path, effective.as_deref(), mode);
            let effective = native_run_target(cmd, &resolved_path, effective);
            run_native_execution(NativeExecutionRequest {
                command: cmd,
                file: &resolved_path,
                emit_rust,
                emit_generated,
                library: library_flag,
                small,
                no_os,
                gates,
                build_grants: &build_grants,
                invocation_authority: invocation_authority.as_ref(),
                remote_builder: remote_builder.as_deref(),
                locked,
                target: effective.as_deref(),
                target_machine: selected_machine.as_ref(),
                explain_partition,
                verbose,
                sbom,
                release: release_flag,
                profile: named_profile.as_deref(),
                setting_overrides: &setting_overrides,
                output: output_name.as_deref(),
                program_args: &program_args,
                mode,
                output_profile: Some(&profile),
                record: record_name.as_deref(),
                interpret,
                entry_fn: resolved.callable.as_deref(),
                check_project_scope: cmd == "check" && Path::new(target).is_dir(),
                package_scope: cmd != "build"
                    || named_build_target
                    || (Path::new(target).is_dir()
                        && !jet_argv.iter().any(|arg| arg == "--show-default")),
                build_override: cmd != "build"
                    || !jet_argv.iter().any(|arg| arg == "--show-default"),
                source_overlay: None,
            });
        }
    }

    let host_dispatch_words: &[&[&str]] = &[
        dispatch_route_words!(),
        bare_dispatch_route_words!(),
        late_dispatch_route_words!(),
        alias_dispatch_route_words!(),
        perf_dispatch_route_words!(),
        lsp_dispatch_route_words!(),
    ];
    if let Some(violation) =
        jet::CLI::parser_inventory_violations_against_slices(Some(&host_dispatch_words))
            .into_iter()
            .next()
    {
        emit_cli_report(
            "E2105",
            format!("CLI parser registry drift: {violation}"),
            "the command parser and its registered surface must remain one table".to_string(),
            "repair the CLI registry drift before invoking Jet".to_string(),
            mode.json,
        );
        exit(ExitCodes::ICE);
    }
    dispatch_command!(cmd);
    if no_target {
        bare_dispatch!(cmd);
    }
    late_dispatch!(cmd);
}

fn parse_build_verify(args: &[String]) -> Result<Option<String>, String> {
    let mut artifact = None;
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        let value = if let Some(value) = argument.strip_prefix("--verify=") {
            if value.is_empty() {
                return Err("`--verify` needs a receipt id".to_string());
            }
            Some(value)
        } else if argument == "--verify" {
            let value = args
                .get(index + 1)
                .filter(|value| !value.is_empty() && !value.starts_with('-'))
                .ok_or_else(|| "`--verify` needs a receipt id".to_string())?;
            index += 1;
            Some(value.as_str())
        } else {
            None
        };
        if let Some(value) = value {
            if artifact.is_some() {
                return Err("`--verify` may be specified only once".to_string());
            }
            artifact = Some(value.to_string());
        }
        index += 1;
    }
    Ok(artifact)
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
/// `file`. Precedence: an explicit CLI flag always wins; else the canonical Package
/// context's `target: "web"` (inline in the entry or a managed package's package.jet);
/// else a lightweight parse of `file` for a top-level `#Target(Web)` marker (a loose
/// file's own default, for standalone examples with no manifest at all).
/// Reparses the file/manifest — wasteful compared to threading the fact
/// through the real compile pipeline, but `jet` recompiles from scratch on
/// every invocation anyway (no incremental compilation), so a couple of
/// extra cheap lex+parse passes are negligible, and it keeps this CLI-only
/// concern out of `ProgramBundle`/codegen entirely.
///
/// `jet run` also resolves the package/file web default so the execution
/// boundary can reject it with a teaching diagnostic before attempting a
/// native build. `jet dev` and `jet build` keep the same default resolution.
fn effective_target(_cmd: &str, file: &str, explicit: Option<&str>) -> Option<String> {
    if explicit.is_some() {
        return explicit.map(str::to_string);
    }
    if let Some(target) = manifest_default_target(file) {
        return Some(target);
    }
    let src = fs::read_to_string(file).ok()?;
    let src = jet::Package::mask_inline_package_source(&src).ok()?.0;
    let (toks, lex_diags) = jet::Lexer::lex(&src);
    if !lex_diags.is_empty() {
        return None;
    }
    let prog = jet::Parser::parse_with_source(&toks, &src).ok()?;
    prog.default_target
}

/// D-WEBRUN1=A: native `jet run` has no web execution backend for ordinary
/// web programs. An App-returning entry is the documented runtime-edge
/// exception: `jet run` serves its App, and `jet dev` reaches this same path
/// through its child `jet run`. Preserve E-WEB-RUN for non-App web programs.
fn reject_native_web_run(command: &str, file: &str, target: Option<&str>, mode: OutputMode) {
    if command == "run" && target == Some(jet::Syntax::BUILD_TARGET_WEB) && !entry_returns_app(file)
    {
        emit_cli_row("E-WEB-RUN", &[], mode.json);
        exit(ExitCodes::USER_ERROR);
    }
}

/// An explicit host target is a native execution choice, even when it
/// overrides a file's web default. An App-returning entry is also served by
/// the native runtime edge, regardless of its web target marker; genuinely
/// web-targeted non-App programs remain rejected above.
fn native_run_target(command: &str, file: &str, target: Option<String>) -> Option<String> {
    let host = jet_foundation::Layout::TargetLayout::host_triple();
    if command == "run"
        && (target.as_deref() == Some(host.as_str())
            || (target.as_deref() == Some(jet::Syntax::BUILD_TARGET_WEB)
                && entry_returns_app(file)))
    {
        None
    } else {
        target
    }
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
    let src = match jet::Package::mask_inline_package_source(&src) {
        Ok((masked, _)) => masked,
        Err(_) => return false,
    };
    let (toks, lex_diags) = jet::Lexer::lex(&src);
    if !lex_diags.is_empty() {
        return false;
    }
    let prog = match jet::Parser::parse_with_source(&toks, &src) {
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
    let source = match jet::Package::mask_inline_package_source(&source) {
        Ok((masked, _)) => masked,
        Err(_) => return false,
    };
    let (tokens, diagnostics) = jet::Lexer::lex(&source);
    if !diagnostics.is_empty() {
        return false;
    }
    let program = match jet::Parser::parse_with_source(&tokens, &source) {
        Ok(program) => program,
        Err(_) => return false,
    };
    jet::AST::app_entry_run_fn(&program.items).is_some()
}

/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): a Package `targets` selection
/// named `web` chooses one of the canonical web target profiles.
fn manifest_default_target(file: &str) -> Option<String> {
    let path = Path::new(file);
    if !path.is_file() {
        return None;
    }
    let manifest = match jet::Loader::package_facts_for_entry(path) {
        Ok(Some(manifest)) => manifest,
        Ok(None) => return None,
        Err(diagnostics) => report_entry_diagnostics(path, &diagnostics),
    };
    let profile = manifest.target_profile("web")?;
    matches!(
        profile,
        jet::Package::Blocks::TargetProfileIdentity::WebBrowser
            | jet::Package::Blocks::TargetProfileIdentity::WebWasiServer
            | jet::Package::Blocks::TargetProfileIdentity::WebNoOs
    )
    .then(|| jet::Syntax::BUILD_TARGET_WEB.to_string())
}

/// Report a package-scope command collision through the registered diagnostic
/// row. The package authority owns discovery; the CLI only supplies the
/// command name and the checked source locations.
fn job_registry_for_entry(
    entry: &Path,
    mode: OutputMode,
) -> jet_foundation::CLISchema::JobRegistry {
    let display = entry.to_string_lossy().into_owned();
    let source = fs::read_to_string(entry).unwrap_or_default();
    let (diagnostics, bundle, _facts) =
        jet::Driver::check_file_with_effect_facts_for_run(&display, "dev", &BTreeMap::new());
    let errors = diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        report_problems(mode, &display, &source, &errors);
        exit(ExitCodes::USER_ERROR);
    }
    let Some(bundle) = bundle else {
        report_problems(mode, &display, &source, &[]);
        exit(ExitCodes::USER_ERROR);
    };
    jet_foundation::CLISchema::JobRegistry::for_entry(&bundle)
}

fn reject_job_graph(entry: &Path, reason: String, mode: OutputMode) -> ! {
    crate::emit_cli_report(
        "E1331",
        format!("Invalid job graph in `{}`: {reason}", entry.display()),
        "checked #Job dependencies and execution bounds must form one valid graph".to_string(),
        "remove unknown, ambiguous, or cyclic dependencies and use a positive `parallel` bound"
            .to_string(),
        mode.json,
    );
    exit(ExitCodes::USER_ERROR);
}

fn validate_job_registry(
    entry: &Path,
    registry: &jet_foundation::CLISchema::JobRegistry,
    mode: OutputMode,
) {
    for job in registry.jobs() {
        let Some(cwd) = job.cwd.as_deref() else {
            continue;
        };
        let cwd_path = Path::new(cwd);
        if cwd_path.is_absolute()
            || cwd_path
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        {
            crate::emit_cli_report(
                "E1330",
                format!("job `{}` has an unsafe cwd", job.name),
                "checked #Job dependencies and execution bounds must form one valid graph"
                    .to_string(),
                "use a project-relative path without `..`".to_string(),
                mode.json,
            );
            exit(ExitCodes::USER_ERROR);
        }
        let resolved = job_base_directory(entry, job);
        if resolved.exists() {
            let project = job_project_directory(entry);
            let root = project.canonicalize().unwrap_or_else(|_| project.clone());
            let resolved = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
            if !resolved.starts_with(&root) {
                crate::emit_cli_report(
                    "E1330",
                    format!("job `{}` has an unsafe cwd", job.name),
                    "checked #Job dependencies and execution bounds must form one valid graph"
                        .to_string(),
                    "use an existing project-relative directory without symlink escapes"
                        .to_string(),
                    mode.json,
                );
                exit(ExitCodes::USER_ERROR);
            }
        }
    }
    if let Err(reason) = registry.validate_graph() {
        reject_job_graph(entry, reason, mode);
    }
}

fn validate_job_working_directories(
    entry: &Path,
    registry: &jet_foundation::CLISchema::JobRegistry,
    mode: OutputMode,
) {
    for job in registry.jobs() {
        if job.cwd.is_some() {
            let resolved = job_base_directory(entry, job);
            if !resolved.is_dir() {
                crate::emit_cli_report(
                    "E1330",
                    format!("job `{}` has a non-directory cwd", job.name),
                    format!("job cwd `{}` is not a directory", resolved.display()),
                    "use an existing project-relative directory for `cwd`".to_string(),
                    mode.json,
                );
                exit(ExitCodes::USER_ERROR);
            }
        }
    }
}

fn job_project_directory(entry: &Path) -> PathBuf {
    let entry_dir = entry.parent().unwrap_or_else(|| Path::new("."));
    jet::Loader::find_manifest_root(entry_dir).unwrap_or_else(|| entry_dir.to_path_buf())
}
fn job_base_directory(entry: &Path, job: &jet_foundation::CLISchema::JobFact) -> PathBuf {
    let project = job_project_directory(entry);
    job.cwd
        .as_deref()
        .map_or(project.clone(), |cwd| project.join(cwd))
}

fn job_status_string_array(values: &[String]) -> StatusValue {
    StatusValue::array(
        values
            .iter()
            .map(|value| StatusValue::String(value.clone())),
    )
}

fn job_status_optional_string(value: Option<&str>) -> StatusValue {
    value
        .map(|value| StatusValue::String(value.to_string()))
        .unwrap_or(StatusValue::Null)
}

fn job_status_skip(skip: Option<&jet::AST::JobSkip>) -> StatusValue {
    match skip {
        None => StatusValue::Null,
        Some(jet::AST::JobSkip::Always(reason)) => StatusValue::object(
            StatusFields::new()
                .with("kind", "always")
                .with("reason", reason.as_str()),
        ),
        Some(jet::AST::JobSkip::UnlessPlatform { platform }) => StatusValue::object(
            StatusFields::new()
                .with("kind", "unless-platform")
                .with("platform", platform.as_str()),
        ),
    }
}

fn job_status_limits(limits: &BTreeMap<String, String>) -> StatusValue {
    let mut fields = StatusFields::new();
    for (name, value) in limits {
        fields = fields.with(name.as_str(), value.as_str());
    }
    StatusValue::object(fields)
}

fn job_status_argument(argument: &jet_foundation::CLISchema::JobArgumentSchema) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("name", argument.name.as_str())
            .with("label", argument.label.as_str())
            .with("type", argument.ty.as_str())
            .with("required", argument.required)
            .with(
                "default",
                job_status_optional_string(argument.default.as_deref()),
            )
            .with("variadic", argument.variadic)
            .with("zone", format!("{:?}", argument.zone)),
    )
}

fn job_cache_name(cache: jet::AST::JobCachePolicy) -> &'static str {
    match cache {
        jet::AST::JobCachePolicy::Uncached => "uncached",
        jet::AST::JobCachePolicy::Local => "local",
        jet::AST::JobCachePolicy::Shared => "shared",
    }
}

fn job_skip_label(skip: &jet::AST::JobSkip) -> String {
    match skip {
        jet::AST::JobSkip::Always(reason) => format!("always ({reason})"),
        jet::AST::JobSkip::UnlessPlatform { platform } => {
            format!("unless platform is {platform}")
        }
    }
}

fn render_job_registry(
    registry: &jet_foundation::CLISchema::JobRegistry,
    profile: jet_cli::OutputProfile::OutputProfile,
) {
    let jobs = registry.visible_jobs();
    if profile.machine_enabled() {
        let jobs_value = StatusValue::array(jobs.iter().map(|job| {
            StatusValue::object(
                StatusFields::new()
                    .with("name", job.name.as_str())
                    .with("scope", job.scope_name())
                    .with("doc", job_status_optional_string(job.doc.as_deref()))
                    .with(
                        "arguments",
                        StatusValue::array(job.arguments.iter().map(job_status_argument)),
                    )
                    .with(
                        "schedule",
                        job_status_optional_string(job.schedule.as_deref()),
                    )
                    .with("after", job_status_string_array(&job.after))
                    .with("inputs", job_status_string_array(&job.inputs))
                    .with("outputs", job_status_string_array(&job.outputs))
                    .with("packages", job_status_string_array(&job.packages))
                    .with("cwd", job_status_optional_string(job.cwd.as_deref()))
                    .with("skip", job_status_skip(job.skip.as_ref()))
                    .with("cache", job_cache_name(job.cache))
                    .with("limits", job_status_limits(&job.limits))
                    .with("parallel", job.parallel),
            )
        }));
        let payload = render_status("jobs", true, StatusFields::new().with("jobs", jobs_value));
        write_machine(profile, &format!("{payload}\n"));
        return;
    }
    if jobs.is_empty() {
        write_renderable(profile, "No jobs declared.\n");
        return;
    }
    let columns = vec![
        jet_cli::AdaptiveTable::TableColumn::new(
            "Name",
            jet_cli::AdaptiveTable::TableCellKind::Text,
        ),
        jet_cli::AdaptiveTable::TableColumn::new(
            "Scope",
            jet_cli::AdaptiveTable::TableCellKind::Text,
        ),
        jet_cli::AdaptiveTable::TableColumn::new(
            "Details",
            jet_cli::AdaptiveTable::TableCellKind::Text,
        ),
    ];
    let rows = jobs
        .iter()
        .map(|job| {
            let arguments = job
                .arguments
                .iter()
                .map(jet_foundation::CLISchema::JobArgumentSchema::display)
                .collect::<Vec<_>>()
                .join(", ");
            let mut detail = if arguments.is_empty() {
                String::new()
            } else {
                format!("({arguments})")
            };
            if let Some(doc) = job.doc.as_deref().filter(|doc| !doc.is_empty()) {
                if !detail.is_empty() {
                    detail.push(' ');
                }
                detail.push_str(&doc.replace('\n', " "));
            }
            if let Some(schedule) = job.schedule.as_deref() {
                if !detail.is_empty() {
                    detail.push(' ');
                }
                detail.push_str(&format!("(every {schedule})"));
            }
            if !job.after.is_empty() {
                if !detail.is_empty() {
                    detail.push(' ');
                }
                detail.push_str(&format!("(after {})", job.after.join(", ")));
            }
            if !job.inputs.is_empty() {
                if !detail.is_empty() {
                    detail.push(' ');
                }
                detail.push_str(&format!("(inputs {})", job.inputs.join(", ")));
            }
            if !job.outputs.is_empty() {
                if !detail.is_empty() {
                    detail.push(' ');
                }
                detail.push_str(&format!("(outputs {})", job.outputs.join(", ")));
            }
            if !job.packages.is_empty() {
                if !detail.is_empty() {
                    detail.push(' ');
                }
                detail.push_str(&format!("(packages {})", job.packages.join(", ")));
            }
            if let Some(cwd) = job.cwd.as_deref() {
                if !detail.is_empty() {
                    detail.push(' ');
                }
                detail.push_str(&format!("(cwd {cwd})"));
            }
            if let Some(skip) = job.skip.as_ref() {
                if !detail.is_empty() {
                    detail.push(' ');
                }
                detail.push_str(&format!("(skip {})", job_skip_label(skip)));
            }
            if job.cache != jet::AST::JobCachePolicy::Uncached {
                if !detail.is_empty() {
                    detail.push(' ');
                }
                detail.push_str(&format!("(cache {})", job_cache_name(job.cache)));
            }
            if !job.limits.is_empty() {
                detail.push(' ');
                let limits = job
                    .limits
                    .iter()
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                detail.push_str(&format!("(limits {limits})"));
            }
            if !detail.is_empty() {
                detail.push(' ');
            }
            detail.push_str(&format!("(parallel {})", job.parallel));
            jet_cli::AdaptiveTable::TableRow::new(vec![
                jet_cli::AdaptiveTable::TableCell::text(job.name.clone()),
                jet_cli::AdaptiveTable::TableCell::text(job.scope_name()),
                jet_cli::AdaptiveTable::TableCell::text(detail),
            ])
        })
        .collect();
    let table = jet_cli::AdaptiveTable::AdaptiveTable::new(columns, rows);
    if let Some(rendered) = table.layout(&profile).render() {
        write_renderable(profile, &format!("{rendered}\n"));
    }
}
fn render_job_inspection(
    entry: &Path,
    registry: &jet_foundation::CLISchema::JobRegistry,
    profile: jet_cli::OutputProfile::OutputProfile,
    requested: Option<&str>,
    graph: bool,
    status: bool,
    explain: bool,
) {
    let jobs = registry.visible_jobs();
    let selected = match requested {
        Some(name) => {
            let Some(job) = registry.find_visible(name) else {
                crate::emit_cli_report(
                    "E1294",
                    format!("No job named `{name}`."),
                    "`jet jobs` inspection uses the same visible checked job namespace as invocation"
                        .to_string(),
                    format!(
                        "check the spelling; declared jobs: {}",
                        registry.completion_words().join(", ")
                    ),
                    profile.machine_enabled(),
                );
                exit(ExitCodes::USER_ERROR);
            };
            vec![job]
        }
        None => jobs,
    };
    if profile.machine_enabled() {
        let jobs_value = StatusValue::array(selected.iter().map(|job| {
            let (fresh, reason) = job_graph_freshness(entry, registry, job);
            StatusValue::object(
                StatusFields::new()
                    .with("name", job.name.as_str())
                    .with("scope", job.scope_name())
                    .with("after", job_status_string_array(&job.after))
                    .with("inputs", job_status_string_array(&job.inputs))
                    .with("outputs", job_status_string_array(&job.outputs))
                    .with("packages", job_status_string_array(&job.packages))
                    .with("cwd", job_status_optional_string(job.cwd.as_deref()))
                    .with("skip", job_status_skip(job.skip.as_ref()))
                    .with("cache", job_cache_name(job.cache))
                    .with("limits", job_status_limits(&job.limits))
                    .with("parallel", job.parallel)
                    .with("fresh", fresh)
                    .with("reason", reason),
            )
        }));
        let view = if graph {
            "graph"
        } else if status {
            "status"
        } else if explain {
            "explain"
        } else {
            "inspect"
        };
        let payload = render_status(
            format!("jobs.{view}"),
            true,
            StatusFields::new()
                .with("entry", entry.display().to_string())
                .with("jobs", jobs_value),
        );
        write_machine(profile, &format!("{payload}\n"));
        return;
    }
    if selected.is_empty() {
        write_renderable(profile, "No jobs declared.\n");
        return;
    }
    let mut rendered = String::new();
    if graph {
        rendered.push_str("Job graph\n");
        for job in &selected {
            let predecessors = if job.after.is_empty() {
                "—".to_string()
            } else {
                job.after.join(", ")
            };
            rendered.push_str(&format!("  {} <- {}\n", job.name, predecessors));
        }
    } else if status {
        rendered.push_str("Job status\n");
        for job in &selected {
            let (fresh, reason) = job_graph_freshness(entry, registry, job);
            let state = if fresh { "fresh" } else { "stale" };
            rendered.push_str(&format!("  {:<24} {:<6} {}\n", job.name, state, reason));
        }
    } else {
        rendered.push_str("Job details\n");
        for job in &selected {
            let (fresh, reason) = job_graph_freshness(entry, registry, job);
            rendered.push_str(&format!("  {}\n", job.name));
            rendered.push_str(&format!("    scope: {}\n", job.scope_name()));
            rendered.push_str(&format!(
                "    after: {}\n",
                if job.after.is_empty() {
                    "—".to_string()
                } else {
                    job.after.join(", ")
                }
            ));
            rendered.push_str(&format!(
                "    inputs: {}\n",
                if job.inputs.is_empty() {
                    "—".to_string()
                } else {
                    job.inputs.join(", ")
                }
            ));
            rendered.push_str(&format!(
                "    outputs: {}\n",
                if job.outputs.is_empty() {
                    "—".to_string()
                } else {
                    job.outputs.join(", ")
                }
            ));
            rendered.push_str(&format!(
                "    packages: {}\n",
                if job.packages.is_empty() {
                    "—".to_string()
                } else {
                    job.packages.join(", ")
                }
            ));
            rendered.push_str(&format!(
                "    cwd: {}\n",
                job.cwd.as_deref().unwrap_or("project root")
            ));
            rendered.push_str(&format!(
                "    skip: {}\n",
                job.skip
                    .as_ref()
                    .map(job_skip_label)
                    .unwrap_or_else(|| "never".to_string())
            ));
            rendered.push_str(&format!("    cache: {}\n", job_cache_name(job.cache)));
            rendered.push_str(&format!(
                "    limits: {}\n",
                if job.limits.is_empty() {
                    "—".to_string()
                } else {
                    job.limits
                        .iter()
                        .map(|(name, value)| format!("{name}={value}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ));
            rendered.push_str(&format!("    parallel: {}\n", job.parallel));
            rendered.push_str(&format!(
                "    freshness: {} ({reason})\n",
                if fresh { "fresh" } else { "stale" }
            ));
        }
    }
    write_renderable(profile, &rendered);
}

fn job_freshness(entry: &Path, job: &jet_foundation::CLISchema::JobFact) -> (bool, String) {
    if matches!(job.cache, jet::AST::JobCachePolicy::Uncached) {
        return (false, "cache policy is uncached".to_string());
    }
    let base = job_base_directory(entry, job);
    let mut missing_input = None;
    let mut missing_output = None;
    let mut newest_input: Option<std::time::SystemTime> = None;
    let mut newest_output: Option<std::time::SystemTime> = None;
    for pattern in &job.inputs {
        let paths = job_declared_paths(&base, pattern);
        if paths.is_empty() {
            missing_input = Some(pattern.clone());
        }
        for path in paths {
            if !path.exists() {
                missing_input = Some(pattern.clone());
            }
            if let Some(modified) = job_latest_modified(&path) {
                newest_input = Some(newest_input.map_or(modified, |current| current.max(modified)));
            }
        }
    }
    for pattern in &job.outputs {
        let paths = job_declared_paths(&base, pattern);
        if paths.is_empty() {
            missing_output = Some(pattern.clone());
        }
        for path in paths {
            if !path.exists() {
                missing_output = Some(pattern.clone());
            }
            if let Some(modified) = job_latest_modified(&path) {
                newest_output =
                    Some(newest_output.map_or(modified, |current| current.max(modified)));
            }
        }
    }
    if job.outputs.is_empty() {
        return (false, "no declared outputs".to_string());
    }
    if let Some(path) = missing_output {
        return (false, format!("output `{path}` is missing"));
    }
    if let Some(path) = missing_input {
        return (false, format!("input `{path}` is missing"));
    }
    if newest_input > newest_output {
        return (
            false,
            "an input is newer than the declared outputs".to_string(),
        );
    }
    (
        true,
        "all declared outputs are newer than declared inputs".to_string(),
    )
}

fn job_graph_freshness(
    entry: &Path,
    registry: &jet_foundation::CLISchema::JobRegistry,
    job: &jet_foundation::CLISchema::JobFact,
) -> (bool, String) {
    let own = job_freshness(entry, job);
    if !own.0 {
        return own;
    }
    let mut seen = BTreeSet::new();
    if job_graph_is_stale(entry, registry, job, &mut seen) {
        return (false, "a declared dependency is stale".to_string());
    }
    own
}

fn job_glob_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut row = vec![false; value.len() + 1];
    row[0] = true;
    for &token in pattern {
        let mut next = vec![false; value.len() + 1];
        if token == b'*' {
            next[0] = row[0];
            for index in 1..=value.len() {
                next[index] = row[index] || next[index - 1];
            }
        } else {
            for index in 1..=value.len() {
                if token == b'?' || token == value[index - 1] {
                    next[index] = row[index - 1];
                }
            }
        }
        row = next;
    }
    row[value.len()]
}

fn job_declared_paths(base: &Path, pattern: &str) -> Vec<PathBuf> {
    let normalized = pattern.replace('\\', "/");
    if !normalized.contains('*') && !normalized.contains('?') {
        return vec![base.join(pattern)];
    }
    let mut pending = vec![base.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                pending.push(path.clone());
            }
            let Ok(relative) = path.strip_prefix(base) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            if job_glob_match(&normalized, &relative) {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn job_latest_modified(path: &Path) -> Option<std::time::SystemTime> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let mut latest = metadata.modified().ok();
    if metadata.is_dir() {
        let mut entries = fs::read_dir(path)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        entries.sort();
        for child in entries {
            if let Some(modified) = job_latest_modified(&child) {
                latest = Some(latest.map_or(modified, |current| current.max(modified)));
            }
        }
    }
    latest
}

fn collect_job_graph_jobs<'a>(
    registry: &'a jet_foundation::CLISchema::JobRegistry,
    job: &'a jet_foundation::CLISchema::JobFact,
    out: &mut Vec<&'a jet_foundation::CLISchema::JobFact>,
    seen: &mut BTreeSet<String>,
) -> Result<(), String> {
    if !seen.insert(job.name.clone()) {
        return Ok(());
    }
    for dependency in &job.after {
        let dependency_job = registry
            .find(dependency)
            .ok_or_else(|| format!("job graph references unknown job `{dependency}`"))?;
        collect_job_graph_jobs(registry, dependency_job, out, seen)?;
    }
    out.push(job);
    Ok(())
}

fn job_graph_is_stale(
    entry: &Path,
    registry: &jet_foundation::CLISchema::JobRegistry,
    job: &jet_foundation::CLISchema::JobFact,
    seen: &mut BTreeSet<String>,
) -> bool {
    if !seen.insert(job.name.clone()) {
        return false;
    }
    !job_freshness(entry, job).0
        || job.after.iter().any(|dependency| {
            registry.find(dependency).is_some_and(|dependency_job| {
                job_graph_is_stale(entry, registry, dependency_job, seen)
            })
        })
}

fn job_depends_on(
    registry: &jet_foundation::CLISchema::JobRegistry,
    job: &jet_foundation::CLISchema::JobFact,
    target: &str,
    seen: &mut BTreeSet<String>,
) -> bool {
    if !seen.insert(job.name.clone()) {
        return false;
    }
    job.after.iter().any(|dependency| {
        dependency == target
            || registry.find(dependency).is_some_and(|dependency_job| {
                job_depends_on(registry, dependency_job, target, seen)
            })
    })
}

fn retain_outermost_stale_jobs(
    registry: &jet_foundation::CLISchema::JobRegistry,
    jobs: &mut Vec<&jet_foundation::CLISchema::JobFact>,
) {
    let names = jobs
        .iter()
        .map(|job| job.name.clone())
        .collect::<BTreeSet<_>>();
    jobs.retain(|job| {
        !names.iter().any(|candidate| {
            candidate != &job.name
                && registry.find(candidate).is_some_and(|candidate_job| {
                    job_depends_on(registry, candidate_job, &job.name, &mut BTreeSet::new())
                })
        })
    });
}

fn run_job_watch(
    entry: &Path,
    registry: &jet_foundation::CLISchema::JobRegistry,
    requested: Option<&str>,
    raw: &[String],
    job_args: &[String],
    mode: OutputMode,
    profile: jet_cli::OutputProfile::OutputProfile,
) -> ! {
    let jobs = registry.visible_jobs();
    let selected = match requested {
        Some(name) => {
            let Some(job) = registry.find_visible(name) else {
                crate::emit_cli_report(
                    "E1294",
                    format!("No job named `{name}`."),
                    "`jet jobs --watch` uses the same visible checked job namespace as invocation"
                        .to_string(),
                    format!(
                        "check the spelling; declared jobs: {}",
                        registry.completion_words().join(", ")
                    ),
                    mode.json,
                );
                exit(ExitCodes::USER_ERROR);
            };
            vec![job]
        }
        None => jobs,
    };
    if selected.is_empty() {
        write_renderable(profile, "No jobs declared.\n");
        exit(ExitCodes::OK);
    }
    let mut watch_jobs = Vec::new();
    let mut seen = BTreeSet::new();
    for job in &selected {
        if let Err(reason) = collect_job_graph_jobs(registry, job, &mut watch_jobs, &mut seen) {
            reject_job_graph(entry, reason, mode);
        }
    }
    let mut graph = jet_devserver::WatchGraph::new();
    graph.set_entry(entry.to_path_buf());
    for job in &watch_jobs {
        let job_base = job_base_directory(entry, job);
        for pattern in &job.inputs {
            for path in job_declared_paths(&job_base, pattern) {
                graph.upsert(path, jet_devserver::WatchService::RootKind::BuildInput);
            }
            if pattern.contains('*') || pattern.contains('?') {
                let prefix = pattern
                    .split(|character| character == '*' || character == '?')
                    .next()
                    .unwrap_or("")
                    .rsplit_once('/')
                    .map(|(parent, _)| parent)
                    .unwrap_or("");
                graph.upsert(
                    job_base.join(prefix),
                    jet_devserver::WatchService::RootKind::BuildInput,
                );
            }
        }
    }
    let mut watch = jet_devserver::WatchSession::from_graph(graph);
    write_mode_status(mode, "watching declared job inputs … (Ctrl-C to stop)\n");
    run_stale_jobs(entry, registry, &selected, raw, job_args, mode);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(
            jet_devserver::WATCH_POLL_INTERVAL_MS,
        ));
        if watch.poll().is_some() {
            run_stale_jobs(entry, registry, &selected, raw, job_args, mode);
        }
    }
}

fn job_option_takes_value(argument: &str) -> bool {
    matches!(
        argument,
        "--profile"
            | "--target"
            | "--allow"
            | "--deny"
            | "--gate"
            | "--set"
            | "--builder"
            | "--record"
    )
}

fn append_job_run_options(command: &mut Command, raw: &[String]) {
    let mut index = 0;
    while index < raw.len() {
        let argument = raw[index].as_str();
        if argument == "--" {
            break;
        }
        if argument == "jobs" || !argument.starts_with('-') {
            index += 1;
            continue;
        }
        if argument == "-p" {
            index += 1;
            if raw.get(index).is_some_and(|value| !value.starts_with('-')) {
                index += 1;
            }
            continue;
        }
        if argument == "--json"
            || argument == "--watch"
            || argument.starts_with("--watch=")
            || matches!(argument, "--graph" | "--status" | "--explain")
        {
            index += 1;
            continue;
        }
        command.arg(argument);
        if job_option_takes_value(argument) && !argument.contains('=') {
            if let Some(value) = raw.get(index + 1).filter(|value| !value.starts_with('-')) {
                command.arg(value);
                index += 1;
            }
        }
        index += 1;
    }
}

fn run_stale_jobs(
    entry: &Path,
    registry: &jet_foundation::CLISchema::JobRegistry,
    jobs: &[&jet_foundation::CLISchema::JobFact],
    raw: &[String],
    job_args: &[String],
    mode: OutputMode,
) {
    let mut stale = jobs
        .iter()
        .copied()
        .filter(|job| {
            let mut seen = BTreeSet::new();
            job_graph_is_stale(entry, registry, job, &mut seen)
        })
        .collect::<Vec<_>>();
    stale.sort_by(|left, right| left.name.cmp(&right.name));
    retain_outermost_stale_jobs(registry, &mut stale);
    for job in stale {
        let mut command = Command::new(
            std::env::current_exe().unwrap_or_else(|_| PathBuf::from(jet::Syntax::BINARY_NAME)),
        );
        if mode.json {
            command.arg("--json");
        }
        append_job_run_options(&mut command, raw);
        command.arg("run").arg(entry).arg("--").arg(&job.name);
        command.args(job_args);
        let status = command.status();
        if mode.quiet {
            continue;
        }
        match status {
            Ok(status) if status.success() => {
                write_mode_status(mode, &format!("job {} completed\n", job.name));
            }
            Ok(status) => {
                write_mode_status(mode, &format!("job {} exited with {}\n", job.name, status));
            }
            Err(error) => {
                write_mode_status(
                    mode,
                    &format!("job {} could not start: {}\n", job.name, error),
                );
            }
        }
    }
}

fn report_command_resolution_error(command: &str, error: &str, mode: OutputMode) -> ! {
    if error.contains("command overrides for the package") {
        let prefix = format!("two `{command}` command overrides for the package: ");
        let locations = error.strip_prefix(&prefix).unwrap_or(error);
        crate::emit_cli_row_with_detail(
            "E3540",
            &[("command", command), ("locations", locations)],
            format!(
                "pin the chosen `{command}` entry with `jet fix`, or remove every other override"
            ),
            mode.json,
        );
    } else {
        crate::cli_error!(
            @full "E2105",
            error,
            "command resolution uses the checked package source tree",
            "repair the package source and run the command again"
        );
    }
    exit(ExitCodes::USER_ERROR);
}

/// Resolve one package-scoped command-function override. A package without a
/// manifest still has a single-file/package root for command discovery; the
/// checked Package resolver owns role, root, and `src/` precedence.
pub(crate) fn resolve_package_command_override(
    root: &Path,
    command: &str,
    mode: OutputMode,
) -> Option<PathBuf> {
    let resolver = match jet::Authority::AuthorityResolver::open(root) {
        Ok(resolver) => resolver,
        Err(error) => report_entry_authority_error(error),
    };
    let package = match jet::Loader::package_facts_for_root(root) {
        Ok(Some(package)) => package,
        Ok(None) => jet::Package::PackageFacts::default(),
        Err(diagnostics) => {
            let rendered = jet::render_diagnostics(&root.display().to_string(), "", &diagnostics);
            write_mode_diagnostic(mode, &rendered);
            exit(ExitCodes::USER_ERROR);
        }
    };
    match package.resolve_command_entry_checked(&resolver, command) {
        Ok(Some(entry)) => Some(entry.path),
        Ok(None) => None,
        Err(error) => report_command_resolution_error(command, &error, mode),
    }
}

/// Resolve a command target through the shared bare-entry rule when it names a
/// project directory. Explicit files keep the ordinary path resolver, so a
/// directory target and a bare command have one workspace/member decision.
fn resolve_command_target(
    cmd: &str,
    raw: &str,
    member_flag: Option<&str>,
    mode: OutputMode,
    use_override: bool,
) -> ResolvedEntry {
    if Path::new(raw).is_dir() {
        if let Some(entry) =
            resolve_bare_entry(cmd, Path::new(raw), member_flag, mode, use_override)
        {
            if checked_explicit_file(&entry.path).is_some() {
                return entry;
            }
        }
    }
    ResolvedEntry::file(PathBuf::from(resolve_source_path(raw)))
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
    if let Some(checked) = checked_explicit_file(path) {
        return keep_typed_spelling(raw, &checked);
    }
    let with_ext = format!("{}.{}", raw, jet::Syntax::FILE_EXT);
    if let Some(checked) = checked_explicit_file(Path::new(&with_ext)) {
        return keep_typed_spelling(&with_ext, &checked);
    }
    raw.to_string()
}

#[derive(Clone)]
struct ResolvedEntry {
    path: PathBuf,
    callable: Option<String>,
}

impl ResolvedEntry {
    fn file(path: PathBuf) -> Self {
        Self {
            path,
            callable: None,
        }
    }
}

/// Keep the spelling the caller typed when it names the same file the authority
/// check validated.
///
/// The point of `checked_explicit_file` is the authority check; its canonical
/// absolute path is not. Returning that path made every human diagnostic
/// locator carry the checkout location (`--> /home/…/tests/ui/x.jet:6:4`) and
/// disagree with `jet dev`, which never resolves and so renders the argument as
/// typed — two verbs, two locators, one diagnostic. The absolute form stays the
/// *machine* representation, which `machine_report_path_for_process` builds on
/// its own for `--json`.
///
/// A relative entry runs the whole pipeline: `jet dev` already hands its raw
/// argument to the loader, sema, the JIT, the watcher, and the cache, and
/// `find_manifest_root` canonicalizes its start before walking up.
///
/// The canonical-equality guard is what makes this safe rather than merely
/// shorter: it proves the typed spelling and the validated path name the same
/// file. A spelling that reaches the file through a symlink or `..` — or an
/// argument the caller already wrote absolute — keeps the validated path, so
/// nothing downstream is ever pointed at a spelling the resolver did not check.
fn keep_typed_spelling(typed: &str, checked: &Path) -> String {
    let path = Path::new(typed);
    if !path.is_absolute() && fs::canonicalize(path).is_ok_and(|resolved| resolved == checked) {
        return typed.to_string();
    }
    checked.to_string_lossy().into_owned()
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

/// Find the stock entry `.jet` file for a project rooted at `root` (D-ILE1,
/// amended by D-ROLEFILE1). `run.jet` is the ordinary zero-ceremony fallback,
/// followed by `src/run.jet` and `<package>.jet`; command homes are selected by
/// the command resolver before this stock fallback is used.
pub(crate) fn find_project_entry(root: &Path) -> PathBuf {
    find_project_entry_with_callable(root).path
}

fn find_project_entry_with_callable(root: &Path) -> ResolvedEntry {
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
            Ok(Some(entry)) => {
                return ResolvedEntry {
                    path: entry.file.path,
                    callable: Some(entry.callable),
                }
            }
            Ok(None) => {}
            Err(error) => {
                crate::cli_error!(@fix "E2105", error, "repair the typed Package output or point at a `.jet` file directly");
                exit(ExitCodes::USER_ERROR);
            }
        }
    }
    match jet::Loader::find_inline_package_root_checked(root) {
        Ok(Some(inline_root)) if inline_root == resolver.root() => {
            match jet::Loader::package_facts_for_root(&inline_root) {
                Ok(Some(facts)) => match facts.resolve_run_entry_checked(&resolver) {
                    Ok(Some(entry)) => {
                        return ResolvedEntry {
                            path: entry.file.path,
                            callable: Some(entry.callable),
                        };
                    }
                    Ok(None) => {}
                    Err(error) => {
                        crate::cli_error!(
                            @fix "E2105",
                            error,
                            "repair the typed inline Package output or point at a `.jet` file directly"
                        );
                        exit(ExitCodes::USER_ERROR);
                    }
                },
                Ok(None) => {}
                Err(diagnostics) => {
                    let entry = inline_root.display().to_string();
                    emit_cli_diagnostics(&entry, "", &diagnostics);
                    exit(ExitCodes::USER_ERROR);
                }
            }
        }
        Ok(Some(_)) | Ok(None) => {}
        Err(diagnostic) => report_entry_diagnostic(diagnostic),
    }
    match jet::Package::PackageFacts::default().resolve_run_entry_checked(&resolver) {
        Ok(Some(entry)) => {
            return ResolvedEntry {
                path: entry.file.path,
                callable: Some(entry.callable),
            };
        }
        Ok(None) => {}
        Err(error) => {
            crate::cli_error!(
                @fix "E2105",
                error,
                "keep one project entry, using `run.jet`, or point at a `.jet` file directly"
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
    if let Some(manifest) = package.as_ref().map(|package| &package.facts) {
        let named = resolver
            .root()
            .join(format!("{}.{}", manifest.name, jet::Syntax::FILE_EXT));
        if let Ok(relative) = named.strip_prefix(resolver.root()) {
            if let Some(entry) = checked_project_entry(&resolver, relative) {
                return ResolvedEntry::file(entry);
            }
        }
    }
    ResolvedEntry::file(resolver.root().join(jet::Syntax::DEFAULT_ENTRY_FILE))
}

fn command_override_entry(root: &Path, command: &str, mode: OutputMode) -> Option<ResolvedEntry> {
    if !matches!(command, "run" | "dev" | "build" | "test") {
        return None;
    }
    resolve_package_command_override(root, command, mode).map(|path| ResolvedEntry {
        path,
        callable: Some(command.to_string()),
    })
}

fn find_command_entry_with_callable(
    root: &Path,
    command: &str,
    mode: OutputMode,
    use_override: bool,
) -> ResolvedEntry {
    let stock = find_project_entry_with_callable(root);
    if use_override {
        command_override_entry(root, command, mode).unwrap_or(stock)
    } else {
        stock
    }
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
    let diagnostic = error.diagnostic();
    emit_cli_diagnostics(jet::Syntax::PACKAGE_FILE, "", &[diagnostic]);
    exit(ExitCodes::USER_ERROR)
}
fn report_entry_diagnostics(path: &Path, diagnostics: &[Diagnostic]) -> ! {
    let entry = path.display().to_string();
    let source = fs::read_to_string(path).unwrap_or_default();
    emit_cli_diagnostics(&entry, &source, diagnostics);
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
fn resolve_named_build_member(cwd: &Path, wanted: &str) -> Result<Option<ResolvedEntry>, String> {
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

fn resolve_member_build_entry(root: &Path) -> Result<Option<ResolvedEntry>, String> {
    let member_resolver =
        jet::Authority::AuthorityResolver::open(&root).map_err(|error| error.to_string())?;
    let checked = member_resolver
        .checked_manifest(Path::new("."))
        .map_err(|error| error.to_string())?;
    let Some(entry) = checked
        .facts
        .resolve_build_entry_checked(&member_resolver)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    Ok(Some(ResolvedEntry::file(entry.path)))
}

/// D-CLI-BARE1=A: shared bare-entry resolver for `run`/`dev`/`debug`/`check`/
/// `build` inside a package — the one rule all five share instead of
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
fn resolve_bare_entry(
    cmd: &str,
    cwd: &Path,
    member_flag: Option<&str>,
    mode: OutputMode,
    use_override: bool,
) -> Option<ResolvedEntry> {
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
            Some(resolver) => {
                match resolver.checked_file(Path::new(jet::Syntax::UNIFIED_LOCK_FILE)) {
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
                }
            }
            None => false,
        };
        if stale_workspace_lock {
            let lock_path = cwd.join(jet::Syntax::UNIFIED_LOCK_FILE);
            let diagnostic = jetpack::Lock::e1202_workspace(&lock_path.display().to_string());
            emit_cli_diagnostics(jet::Syntax::WORKSPACE_FILE, "", &[diagnostic]);
            exit(ExitCodes::USER_ERROR);
        }
    }
    // D-BUILDSCOPE1: the workspace source is the build authority even when it
    // has no `fn build`; the ordinary batteries still build the root after
    // members. Member selection (`-p`) remains an explicit escape.
    if cmd == "build" && member_flag.is_none() {
        if let Some(Ok(Some(source))) = workspace_source.as_ref() {
            if source.role == jetpack::WorkspaceFile::WorkspaceSourceRole::Index {
                return Some(ResolvedEntry::file(source.path.clone()));
            }
        }
    }
    let workspace = match (workspace_resolver.as_ref(), workspace_source.as_ref()) {
        (Some(resolver), Some(Ok(Some(source))))
            if source.role == jetpack::WorkspaceFile::WorkspaceSourceRole::Index =>
        {
            Some(jetpack::WorkspaceFile::evaluate_checked_source(
                source, resolver,
            ))
        }
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
        emit_cli_diagnostics(jet::Syntax::WORKSPACE_FILE, "", &[diagnostic]);
        exit(ExitCodes::USER_ERROR);
    }
    if let Some(workspace) = workspace {
        let plan = match workspace {
            Ok(plan) => plan,
            Err(diagnostic) => {
                if cmd == "build" && member_flag.is_none() {
                    if let Some(Ok(Some(source))) = workspace_source.as_ref() {
                        if source.role == jetpack::WorkspaceFile::WorkspaceSourceRole::Index {
                            return Some(ResolvedEntry::file(source.path.clone()));
                        }
                    }
                }
                emit_cli_diagnostics(jet::Syntax::WORKSPACE_FILE, "", &[diagnostic]);
                exit(ExitCodes::USER_ERROR);
            }
        };
        let runnable: Vec<(String, ResolvedEntry)> = plan
            .members
            .iter()
            .filter_map(|m| {
                let root = cwd.join(&m.path);
                let stock = if cmd == "build" {
                    match resolve_member_build_entry(&root) {
                        Ok(entry) => entry,
                        Err(error) => report_build_resolution_error(error),
                    }
                } else {
                    let entry = find_project_entry_with_callable(&root);
                    checked_explicit_file(&entry.path).map(|_| entry)
                };
                let entry = if use_override {
                    command_override_entry(&root, cmd, mode).or(stock)
                } else {
                    stock
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
    match jet::Loader::find_package_root_checked(cwd) {
        Ok(Some(root)) => Some(find_command_entry_with_callable(
            &root,
            cmd,
            mode,
            use_override,
        )),
        Ok(None) => None,
        Err(diagnostic) => report_entry_diagnostic(diagnostic),
    }
}

fn report_entry_diagnostic(diagnostic: Diagnostic) -> ! {
    emit_cli_diagnostics(jet::Syntax::WORKSPACE_FILE, "", &[diagnostic]);
    exit(ExitCodes::USER_ERROR)
}

fn missing_bare_entry(cmd: &str, cwd: &Path, mode: OutputMode) -> ! {
    // D-JPK-FILENAME2=B (A2): a retired manifest filename in place of
    // `pkg.jet` gets the E1226 teaching diagnostic.
    if let Some(msg) = jet::Loader::stale_manifest_name_message(cwd) {
        write_mode_diagnostic(mode, &msg);
        exit(ExitCodes::USAGE);
    }
    crate::cli_error!(@fix "E2104", "no file given and no `package.jet` found in this directory or above", format!("run `jet {} <file.{}>` or cd into a project", cmd, jet::Syntax::FILE_EXT));
    exit(ExitCodes::USAGE);
}

fn run_version(profile: jet_cli::OutputProfile::OutputProfile) {
    write_renderable(profile, &jet::Manifest::version_banner());
}

/// Self update verifies the signed channel manifest and selected platform
/// artifact. An explicit `--allow-unofficial` selects only a local keyless
/// fixture; the endpoint is host-owned so local staging and HTTPS use one
/// verifier. Apply is exact-host only; cross-platform selection is dry-run
/// verification.
fn run_self_update(raw: &[String], mode: OutputMode) -> ! {
    let roots = jetpack::Store::resolve();
    let endpoint = self_update_option(raw, "--endpoint")
        .or_else(|| {
            match jetpack::ToolchainUpdate::configured_endpoint(&roots.root) {
                Ok(endpoint) => endpoint,
                Err(error) => {
                    emit_cli_report(
                        "E2105",
                        format!("could not read the Jet toolchain endpoint: {error}"),
                        "self-update needs a readable host-owned endpoint configuration"
                            .to_string(),
                        "fix or remove JETPACK_ROOT/config/toolchain-v1.endpoint, or pass --endpoint <url>"
                            .to_string(),
                        mode.json,
                    );
                    exit(ExitCodes::USER_ERROR);
                }
            }
        })
        .unwrap_or_else(|| jetpack::ToolchainUpdate::DEFAULT_ENDPOINT.to_string());
    let channel = self_update_option(raw, "--channel")
        .unwrap_or_else(|| jetpack::ToolchainUpdate::DEFAULT_CHANNEL.to_string());
    let platform = self_update_option(raw, "--platform")
        .unwrap_or_else(jetpack::ToolchainUpdate::default_target);
    let state_key =
        jet::SHA256::sha256_hex(format!("toolchain-update-v1\n{channel}\n{platform}").as_bytes());
    let state_path = roots
        .root
        .join("config/toolchain-v1")
        .join(format!("{state_key}.state"));
    let trust_key = self_update_option(raw, "--trust-key")
        .map(PathBuf::from)
        .unwrap_or_else(|| roots.root.join("trust/toolchain-v1.ed25519.pub"));
    let dry_run = raw.iter().any(|arg| arg == "--dry-run");
    let apply = raw.iter().any(|arg| arg == "--apply");
    let allow_unofficial = raw.iter().any(|arg| arg == "--allow-unofficial");
    let options = jetpack::ToolchainUpdate::UpdateOptions {
        endpoint,
        channel,
        platform,
        trust_key,
        dry_run,
        apply,
        allow_unofficial,
        running_version: jet::Manifest::COMPILER_VERSION.to_string(),
        state_path,
    };
    let current_exe = apply.then(|| std::env::current_exe().ok()).flatten();
    match jetpack::ToolchainUpdate::run(&options, current_exe.as_deref()) {
        Ok(result) => {
            if mode.json {
                let trust = match result.plan.trust {
                    jetpack::ToolchainUpdate::UpdateTrust::Signed => "signed",
                    jetpack::ToolchainUpdate::UpdateTrust::UnofficialKeyless => {
                        "unofficial-keyless"
                    }
                };
                let key_id = result
                    .plan
                    .key_id
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null);
                let payload = render_status(
                    "self-update",
                    true,
                    StatusFields::new()
                        .with("channel", result.plan.channel.as_str())
                        .with("version", result.plan.version.as_str())
                        .with("platform", result.plan.platform.as_str())
                        .with("artifact", result.plan.artifact_url.as_str())
                        .with("sha256", result.plan.sha256.as_str())
                        .with("size", result.plan.size)
                        .with("key_id", key_id)
                        .with("trust", trust)
                        .with("sequence", result.plan.sequence)
                        .with("published_at", result.plan.published_at)
                        .with("expires_at", result.plan.expires_at)
                        .with("min_version", result.plan.min_version.as_str())
                        .with("applied", result.applied)
                        .with("deferred", result.deferred),
                );
                write_mode_machine(mode, &format!("{payload}\n"));
            } else if result.deferred {
                write_mode_status(
                    mode,
                    &format!(
                        "staged {} {} for {}; Windows will restart it after this process exits{}",
                        jet::Syntax::BINARY_NAME,
                        result.plan.version,
                        result.plan.platform,
                        match result.plan.trust {
                            jetpack::ToolchainUpdate::UpdateTrust::Signed => "",
                            jetpack::ToolchainUpdate::UpdateTrust::UnofficialKeyless => {
                                " [unofficial keyless source]"
                            }
                        }
                    ),
                );
            } else if result.applied {
                write_mode_status(
                    mode,
                    &format!(
                        "updated {} to {} ({}){}",
                        jet::Syntax::BINARY_NAME,
                        result.plan.version,
                        result.plan.platform,
                        match result.plan.trust {
                            jetpack::ToolchainUpdate::UpdateTrust::Signed => "",
                            jetpack::ToolchainUpdate::UpdateTrust::UnofficialKeyless => {
                                " [unofficial keyless source]"
                            }
                        }
                    ),
                );
            } else {
                write_mode_status(
                    mode,
                    &format!(
                        "verified {} {} for {} from {}{}",
                        jet::Syntax::BINARY_NAME,
                        result.plan.version,
                        result.plan.platform,
                        result.plan.artifact_url,
                        match result.plan.trust {
                            jetpack::ToolchainUpdate::UpdateTrust::Signed => "",
                            jetpack::ToolchainUpdate::UpdateTrust::UnofficialKeyless => {
                                " [unofficial keyless source]"
                            }
                        }
                    ),
                );
            }
            exit(ExitCodes::OK);
        }
        Err(error) => {
            emit_cli_report(
                "E2105",
                format!("could not update {}: {error}", jet::Syntax::BINARY_NAME),
                "self-update accepts a signed channel manifest and matching signed artifact; local keyless sources are a separate explicit tier"
                    .to_string(),
                "check the endpoint, public trust key, channel, and platform; use --dry-run to verify without installing, or --allow-unofficial only with a local file:// source"
                    .to_string(),
                mode.json,
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

fn self_update_option(raw: &[String], name: &str) -> Option<String> {
    raw.iter()
        .find_map(|arg| arg.strip_prefix(&format!("{name}=")).map(str::to_string))
        .or_else(|| {
            raw.iter()
                .position(|arg| arg == name)
                .and_then(|index| raw.get(index + 1))
                .filter(|value| !value.starts_with('-'))
                .cloned()
        })
}

fn is_diagnostic_code(s: &str) -> bool {
    let bytes = s.as_bytes();
    (bytes.len() == 5
        && matches!(bytes[0], b'E' | b'L')
        && bytes[1..].iter().all(|byte| byte.is_ascii_digit()))
        || (bytes.len() == 6
            && bytes[..2] == *b"JT"
            && bytes[2..].iter().all(|byte| byte.is_ascii_digit()))
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
        let clears = jet::Diagnostics::report_clear_counts(diags);
        let reports = diags.iter().zip(clears).map(|(diagnostic, clears)| {
            diagnostic.to_report_with_clears(&machine_file, src, clears)
        });
        let rendered =
            render_status_with_reports("diagnostics", false, reports, StatusFields::new());
        write_mode_machine(mode, &format!("{rendered}\n"));
        return;
    }
    let rendered = jet::render_all_linked(
        file,
        src,
        diags,
        mode.color_stderr(),
        mode.hyperlinks_stderr(),
    );
    write_mode_diagnostic(mode, &rendered);
    let n = diags.len();
    write_mode_diagnostic(
        mode,
        &format!("\n{} problem{} found\n", n, if n == 1 { "" } else { "s" }),
    );
    if let Some(first) = diags.first() {
        write_mode_diagnostic(
            mode,
            &format!(
                "{}\n",
                jet::Explain::pointer_line(&first.code, mode.color_stderr())
            ),
        );
    }
}

/// Return the one physical path representation used by CLI machine reports.
///
/// Disk-backed source labels are absolute, so an agent can open them without
/// guessing the package root. Synthetic labels stay labels: they do not name
/// files and must not become host-specific paths in stable output.
pub(crate) fn machine_report_path_for_process(file: &str) -> ReportPath {
    ReportPath::from_process(file)
}

/// Resolve a loader display label against the entry's package/workspace root.
/// Loader displays are intentionally human-friendly and root-relative; JSON
/// reports need the corresponding physical path instead.
pub(crate) fn machine_report_path_for_entry(entry_file: &str, display: &str) -> ReportPath {
    if display.is_empty() || display.starts_with('<') || Path::new(display).is_absolute() {
        return ReportPath::from_process(display);
    }
    let entry = machine_report_path_from_path(Path::new(entry_file));
    let entry = Path::new(entry.as_str());
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
) -> ReportPath {
    if display.is_empty() || display.starts_with('<') || Path::new(display).is_absolute() {
        return ReportPath::from_process(display);
    }
    let path = bundle
        .modules
        .iter()
        .find(|module| module.display == display)
        .map(|module| module.path.clone())
        .unwrap_or_else(|| bundle.project_root.join(display));
    machine_report_path_from_path(&path)
}

fn machine_report_path_from_path(path: &Path) -> ReportPath {
    ReportPath::from_path(path)
}

/// Render a `jet`-owned toolchain diagnostic (E1249–E1252) in the standard
/// teaching voice (docs/spec/diagnostics.md), matching the engine-dispatch
/// diagnostics. These carry no source span, so the full linked renderer isn't
/// used.
fn print_toolchain_diag(d: &jet::Diagnostics::Diagnostic, mode: OutputMode) {
    write_mode_diagnostic(mode, &d.render("", ""));
}

/// D-JPK-TOOLCHAIN1=A (#179): hand off to the project's pinned `jet` toolchain
/// when the running compiler doesn't satisfy the pin. Returns normally when the
/// running `jet` should run the verb itself (unpinned, in-channel, or already
/// the exec'd pinned child).
fn maybe_dispatch_pinned_toolchain(raw: &[String], mode: OutputMode) {
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
            print_toolchain_diag(&d, mode);
            exit(ExitCodes::USER_ERROR);
        }
        PinDecision::ReExec {
            binary,
            channel,
            version,
        } => {
            write_mode_status(
                mode,
                &format!("{}\n", handoff_line(&channel, &version, running)),
            );
            let status = Command::new(&binary)
                .args(raw.iter().map(|s| s.as_str()))
                .env(jet::Syntax::TOOLCHAIN_EXEC_MARKER_ENV, &version)
                .status()
                .unwrap_or_else(|e| {
                    crate::cli_error!(
                        "E2105",
                        "couldn't exec the pinned toolchain `{}`: {}",
                        binary.display(),
                        e
                    );
                    exit(ExitCodes::USER_ERROR);
                });
            exit(status.code().unwrap_or(ExitCodes::USER_ERROR));
        }
    }
}

/// `jet self toolchain` — print the project's pin, locked version, object id, and
/// realized state (read-only, D-JPK-TOOLCHAIN1=A #179).
fn run_toolchain(mode: OutputMode) -> ! {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = require_manifest_root(
        &cwd,
        "error: `jet self toolchain` needs a project — no `package.jet` found here or above",
    );
    write_mode_renderable(mode, &jetpack::JetPin::report_pin(&root));
    exit(ExitCodes::OK);
}

/// `jet init` — write a `package.jet` here, pinning the running toolchain's channel
/// (D-JPK-TOOLCHAIN1=A #179, U11 lift).
/// `jet init [<script.jet>]` — U11 (D-JPK-SCRIPTDEP1=A): when a manifest-less
/// script is named, its inline `use pkg#version;` refs are lifted into the
/// freshly written `package.jet`'s `deps: {}` block (D-JPK-SCRIPTDEP1=A).
/// Lifting is best-effort: a lex/
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
        let changes = StatusValue::array(result.summary.changes.iter().map(|change| {
            StatusValue::object(
                StatusFields::new()
                    .with("path", change.path.to_string_lossy().to_string())
                    .with("action", change.action),
            )
        }));
        let payload = render_status(
            "fold",
            true,
            StatusFields::new()
                .with("operation", result.summary.operation.as_str())
                .with("check", check_only)
                .with("before", result.summary.before_fingerprint.as_str())
                .with("after", result.summary.after_fingerprint.as_str())
                .with(
                    "journal",
                    result.summary.journal.to_string_lossy().to_string(),
                )
                .with("changes", changes),
        );
        write_mode_machine(mode, &format!("{payload}\n"));
    } else {
        for change in &result.summary.changes {
            write_mode_status(
                mode,
                &format!(
                    "{}\n",
                    if check_only {
                        format!("Would {}: {}", change.action, change.path.display())
                    } else {
                        format!("{}d {}.", change.action, change.path.display())
                    }
                ),
            );
        }
        if result.summary.before_fingerprint == result.summary.after_fingerprint {
            write_mode_status(
                mode,
                &format!(
                    "package graph unchanged: {}\n",
                    result.summary.after_fingerprint
                ),
            );
        } else {
            write_mode_status(
                mode,
                &format!(
                    "package graph before: {}\n",
                    result.summary.before_fingerprint
                ),
            );
            write_mode_status(
                mode,
                &format!(
                    "package graph after: {}\n",
                    result.summary.after_fingerprint
                ),
            );
        }
        if check_only {
            write_mode_status(mode, "No files changed.\n");
        } else {
            write_mode_status(
                mode,
                &format!("Transition journal: {}\n", result.summary.journal.display()),
            );
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
    emit_cli_report(
        code,
        message.to_string(),
        why.to_string(),
        fix.to_string(),
        false,
    );
    exit(ExitCodes::USER_ERROR);
}

fn run_init(script: Option<&str>, raw: &[String], mode: OutputMode) -> ! {
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
        write_mode_status(
            mode,
            "No migration-era role files found.\nNo files changed.\n",
        );
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
                lift_inline_deps_into_manifest(&cwd, script, mode);
            }
            write_mode_status(mode, &format!("{msg}\n"));
            exit(ExitCodes::OK);
        }
        Err(d) => {
            print_toolchain_diag(&d, mode);
            exit(ExitCodes::USER_ERROR);
        }
    }
}
/// U11: fold `script`'s inline deps into `<cwd>/package.jet`'s `deps: {}` block
/// (just written by `write_init`), preserving comments/formatting via the
/// same comment-preserving editor `jet add` uses.
fn lift_inline_deps_into_manifest(cwd: &Path, script: &str, mode: OutputMode) {
    let script_path = resolve_source_path(script);
    let Ok(src) = fs::read_to_string(&script_path) else {
        return;
    };
    let source_for_parse = match jet::Package::mask_inline_package_source(&src) {
        Ok((masked, _)) => masked,
        Err(_) => return,
    };
    let (toks, lex_diags) = jet::Lexer::lex(&source_for_parse);
    if !lex_diags.is_empty() {
        return;
    }
    let Ok(prog) = jet::Parser::parse_with_source(&toks, &source_for_parse) else {
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
        write_mode_status(
            mode,
            &format!(
                "lifted {} inline dependenc{} into {}\n",
                deps.len(),
                if deps.len() == 1 { "y" } else { "ies" },
                manifest_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("package.jet")
            ),
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
        crate::cli_error!(
            "E2104",
            "`jet fetch --lock` needs a script path, e.g. `jet fetch --lock stats.jet`"
        );
        exit(ExitCodes::USER_ERROR);
    };
    let file = resolve_source_path(raw_arg);
    let script_path = Path::new(&file);
    let script_dir = script_path.parent().unwrap_or(Path::new("."));

    if jet::Loader::find_manifest_root(script_dir).is_some() {
        crate::cli_error!(
            "E1202",
            "`{file}` belongs to a project with a `{}` — use `jet fetch` to lock its dependencies",
            jet::Syntax::PACKAGE_FILE
        );
        exit(ExitCodes::USER_ERROR);
    }

    let src = match fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) => {
            crate::cli_error!("E2105", "couldn't read `{file}`: {e}");
            exit(ExitCodes::USER_ERROR);
        }
    };
    let source_for_parse = match jet::Package::mask_inline_package_source(&src) {
        Ok((masked, _)) => masked,
        Err(error) => {
            report_problems(mode, &file, &src, &[error.diagnostic()]);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let (toks, lex_diags) = jet::Lexer::lex(&source_for_parse);
    if !lex_diags.is_empty() {
        report_problems(mode, &file, &src, &lex_diags);
        exit(ExitCodes::USER_ERROR);
    }
    let prog = match jet::Parser::parse_with_source(&toks, &source_for_parse) {
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
            write_mode_status(
                mode,
                &format!(
                    "wrote {}\n",
                    jetpack::ScriptLock::sidecar_path(script_path).display()
                ),
            );
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
fn run_update_jet(channel: Option<&str>, mode: OutputMode) -> ! {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = require_manifest_root(
        &cwd,
        "error: `jet update jet` needs a project — no `package.jet` found here or above",
    );
    match jetpack::JetPin::move_pin(&root, channel, env!("CARGO_PKG_VERSION")) {
        Ok(msg) => {
            write_mode_status(mode, &format!("{msg}\n"));
            exit(ExitCodes::OK);
        }
        Err(d) => {
            print_toolchain_diag(&d, mode);
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

fn test_option_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().enumerate().find_map(|(index, arg)| {
        if let Some(value) = arg.strip_prefix(&format!("{flag}=")) {
            Some(value.to_string())
        } else if arg == flag {
            args.get(index + 1).cloned()
        } else {
            None
        }
    })
}

/// Find the manifest root from `cwd`, or exit. D-JPK-FILENAME2=B (A2): when
/// there's no `package.jet` but a retired filename (`pkg.jet`/`pack.jet`/
/// `payload.jet`/`jet.toml`) sits where it belongs, teaches E1226 instead of
/// the generic "no package.jet found" — `fallback_hint` is that generic
/// message's body for
/// commands that genuinely have no manifest at all.
pub(crate) fn require_manifest_root(cwd: &Path, fallback_hint: &str) -> PathBuf {
    match jet::Loader::find_package_root_checked(cwd) {
        Ok(Some(root)) => root,
        Ok(None) => {
            match jet::Loader::stale_manifest_name_message(cwd) {
                Some(msg) => write_mode_diagnostic(
                    OutputMode {
                        json: false,
                        color: jet::Diagnostics::ColorChoice::Never,
                        quiet: false,
                    },
                    &msg,
                ),
                None => write_mode_diagnostic(
                    OutputMode {
                        json: false,
                        color: jet::Diagnostics::ColorChoice::Never,
                        quiet: false,
                    },
                    &format!("{fallback_hint}\n"),
                ),
            }
            exit(ExitCodes::USER_ERROR);
        }
        Err(diagnostic) => {
            emit_cli_diagnostics(jet::Syntax::PACKAGE_FILE, "", &[diagnostic]);
            exit(ExitCodes::USER_ERROR);
        }
    }
}
