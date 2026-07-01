//! jet CLI: check / build / run / test / new / fmt / lsp +
//!          add / remove / fetch / update / store (M12.1 package manager).
//!
//! The driver owns invariant I2: rustc's voice never reaches the user as
//! if it were their fault. A rustc failure on generated code is reported
//! as an internal compiler error in jet.

// Source files/modules use PascalCase names (owner decision).
#![allow(non_snake_case)]

use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use jet::Diagnostics::ColorChoice;
use jet::ExitCodes;

mod CmdCompile;
mod CmdDevTools;
mod CmdDevWeb;
mod CmdPkg;
mod CmdSchema;
mod CmdImpact;
mod CmdSemIndex;
mod CmdSupply;

use CmdCompile::{
    run_compile_cmd, run_debug_native, run_dev_entry, run_fix, run_fmt, run_new, run_test,
    run_test_cov,
};
use CmdDevTools::{
    run_bench, run_bind, run_completions, run_dev, run_doctor, run_emit_rust, run_eval,
    run_explain, run_lint_a11y, run_repl, run_serve, watch_policy_from, WatchPolicy,
};
use CmdDevWeb::run_dev_web;
use CmdPkg::{
    run_add, run_fetch, run_gc, run_remove, run_store_generations, run_store_rollback,
    run_store_verify, run_update,
};
use CmdSchema::run_schema;
use CmdImpact::run_impact;
use CmdSemIndex::run_semindex;
use CmdSupply::{run_audit, run_publish, run_sbom, run_vendor, run_yank};

/// How diagnostics should be presented this run, resolved once from flags +
/// environment and threaded through the diagnostic-printing helpers.
#[derive(Clone, Copy)]
pub(crate) struct OutputMode {
    /// Emit machine-readable `--json` diagnostics instead of human text.
    pub(crate) json: bool,
    /// User's `--color` choice (resolved against TTY-ness at print time).
    pub(crate) color: ColorChoice,
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
impl From<jet::Jetpack::PackageManifest::BuildOptimize> for OptimizeLevel {
    fn from(v: jet::Jetpack::PackageManifest::BuildOptimize) -> Self {
        match v {
            jet::Jetpack::PackageManifest::BuildOptimize::None => OptimizeLevel::None,
            jet::Jetpack::PackageManifest::BuildOptimize::Basic => OptimizeLevel::Basic,
            jet::Jetpack::PackageManifest::BuildOptimize::Full => OptimizeLevel::Full,
        }
    }
}

/// D-BUILDPROFILE1: rustc flags and build env derived from a profile definition.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ProfileConfig {
    pub optimize: OptimizeLevel,
    pub debug_info: bool,
    pub small: bool,
    pub panic_abort: bool,
    pub features: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl ProfileConfig {
    pub(crate) fn release() -> Self {
        Self {
            optimize: OptimizeLevel::Full,
            debug_info: false,
            small: false,
            panic_abort: false,
            features: Vec::new(),
            env: Vec::new(),
        }
    }

    pub(crate) fn debug() -> Self {
        Self {
            optimize: OptimizeLevel::None,
            debug_info: true,
            small: false,
            panic_abort: false,
            features: Vec::new(),
            env: Vec::new(),
        }
    }

    pub(crate) fn ci() -> Self {
        Self {
            optimize: OptimizeLevel::Basic,
            debug_info: true,
            small: false,
            panic_abort: false,
            features: Vec::new(),
            env: Vec::new(),
        }
    }

    pub(crate) fn from_def(def: &jet::Jetpack::PackageManifest::BuildProfileDef) -> Self {
        use jet::Jetpack::PackageManifest::BuildPanic;
        Self {
            optimize: OptimizeLevel::from(def.optimize),
            debug_info: def.debug_info,
            small: def.small,
            panic_abort: matches!(def.panic, Some(BuildPanic::Abort)),
            features: def.features.clone(),
            env: def.env.clone(),
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
        if !self.features.is_empty() {
            parts.push(format!("feat:{}", self.features.join("+")));
        }
        if !self.env.is_empty() {
            let mut env = self.env.clone();
            env.sort_by(|a, b| a.0.cmp(&b.0));
            parts.push(format!(
                "env:{}",
                env.iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        parts.join(";")
    }

    pub(crate) fn apply_rustc(&self, cmd: &mut Command, ffi: bool) {
        if self.small {
            cmd.arg("-C")
                .arg("opt-level=z")
                .arg("-C")
                .arg("panic=abort")
                .arg("-C")
                .arg("strip=symbols");
            if !ffi {
                cmd.arg("-C").arg("lto=fat");
            }
            return;
        }
        match self.optimize {
            OptimizeLevel::None => {}
            OptimizeLevel::Basic => {
                cmd.arg("-O");
            }
            OptimizeLevel::Full => {
                cmd.arg("-C").arg("opt-level=3");
            }
        }
        if self.debug_info {
            cmd.arg("-C").arg("debuginfo=2");
        } else if !matches!(self.optimize, OptimizeLevel::None) {
            cmd.arg("-C").arg("strip=symbols");
        }
        if self.panic_abort {
            cmd.arg("-C").arg("panic=abort");
        }
        if !ffi && !matches!(self.optimize, OptimizeLevel::None) {
            cmd.arg("-C").arg("lto=thin");
        }
        for feat in &self.features {
            cmd.arg("--cfg").arg(format!("feature=\"{feat}\""));
        }
    }

    pub(crate) fn apply_env(&self, cmd: &mut Command) {
        for (key, value) in &self.env {
            cmd.env(key, value);
        }
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
    Named {
        name: String,
        config: ProfileConfig,
    },
    /// S15: size-oriented (`opt-level=z`, fat LTO, `panic=abort`).
    Small,
    /// E2-M15: freestanding / embedded — no OS, only core APIs; `panic=abort`.
    Freestanding,
}

impl BuildProfile {
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
                features: Vec::new(),
                env: Vec::new(),
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
                features: Vec::new(),
                env: Vec::new(),
            },
            BuildProfile::Freestanding => ProfileConfig {
                optimize: OptimizeLevel::Basic,
                debug_info: false,
                small: true,
                panic_abort: true,
                features: Vec::new(),
                env: Vec::new(),
            },
        }
    }
}

pub(crate) fn usage() -> String {
    format!(
        "\
Welcome to {lang}! (v{ver})

usage:
  {bin} check <file.{ext}>          look for problems, build nothing
  {bin} build <file.{ext}>          compile to a native binary in ./build/
  {bin} run   <file.{ext}>          build, then run (or `jet run` inside a project)
  {bin} run   <file.{ext}> a b      extra words become program arguments
  {bin} run   <file.{ext}> -- ...   everything after `--` is forwarded to the program (D-CLI1)
  {bin} test  <file|dir>            compile and run top-level test blocks
  {bin} test  <file|dir>  -- ...    `--` forwards to the test runner
  {bin} new   <name>                create a new project folder with pkg.jet
  {bin} new   <name> --annotated    same, with commented example deps
  {bin} env                         enter the project dev shell (delegates to `jetpack enter`)
  {bin} env   -- cmd                run a command in the project dev shell, then exit
  {bin} dev   <file.{ext}>          watch and re-run on every save (c77 auto-detects mode)
  {bin} serve <file.{ext}>          watch a resident program; hot-swap type-stable edits (c77)
  {bin} debug <file.{ext}>          step through a program at the Jet source level (D-DBG3)
  {bin} repl                         start an interactive session (E2-M18)
  {bin} repl  --project <dir>        same, with access to a project's imports
  {bin} eval  <file.{ext}> --pure   evaluate a pure program to stable JSON (S60)
  {bin} fmt   <file.{ext}>          rewrite file to canonical style (S44)
  {bin} fix   <file.{ext}>          apply all auto-fixable diagnostics in place
  {bin} fix   <file.{ext}> --dry-run   show the fixes as a diff, write nothing
  {bin} doctor                      diagnose the toolchain and offer fixes
  {bin} doctor --fix                apply the auto-fixable problems
  {bin} completions <bash|zsh|fish> print a shell completion script
  {bin} man                         print the jet man page (roff)
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
  {bin} ?                           same as help
  {bin}                             start the interactive REPL (same as repl)
  {bin} <file.{ext}>                run a file (same as run)
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
  {bin} store generations           list recorded store generations (D-PURE3)
  {bin} store rollback <gen>        roll back to a prior generation (D-PURE3)
  {bin} gc                          remove unreferenced store entries

supply chain (E2-M8):
  {bin} publish                     publish the current package to the registry
  {bin} publish --force             publish even if the pre-publish gate warns
  {bin} vendor                      copy all dependencies into vendor/ (offline builds)
  {bin} vendor --vendor-dir <path>  copy them into a chosen directory instead
  {bin} build  --sbom <file>        also write an SPDX SBOM next to the binary
  {bin} audit                       check dependencies against the advisory database
  {bin} audit --advisory-db <path>  use a custom advisory database
  {bin} sbom                        emit an SPDX SBOM from the lockfile
  {bin} sbom --cyclonedx            emit a CycloneDX JSON SBOM instead

flags:
  --emit-rust                  also print the generated Rust code
  --check                      with fmt: exit 1 if file would change (CI)
  --sbom                       with build: write an SPDX SBOM beside the binary
  --vendor-dir <path>          with vendor: directory to copy dependencies into
  --small                      with build/run: smallest binary (S15)
  --freestanding               with build/run: no OS; rejects std-only APIs (E2-M15)
  --target=<triple>            with build: cross-compile for a rustc target triple (E2-M15)
  --explain-partition          with build --target=web: print the JS/WASM partition report (D-WASM1)
  --locked                     with fetch: verify only, refuse network
  --verbose, -v                with build: print the bridge steps
  --json                       emit machine-readable diagnostics
  --color=auto|always|never    control color (auto: only on a terminal)
",
        bin = jet::Syntax::BINARY_NAME,
        lang = jet::Syntax::LANG_NAME,
        ver = env!("CARGO_PKG_VERSION"),
        ext = jet::Syntax::FILE_EXT,
    )
}

/// True when `arg` names a Jet source file or project directory (c6vz465 sugar:
/// `jet <file>` → `jet run <file>`). Unknown bare stems that do not exist are
/// false so E2101 still fires for typos like `buld`.
fn looks_like_jet_source(arg: &str) -> bool {
    let path = Path::new(arg);
    if path
        .extension()
        .is_some_and(|e| e == jet::Syntax::FILE_EXT)
    {
        return true;
    }
    if path.exists() {
        return path.is_file() || path.is_dir();
    }
    Path::new(&format!("{}.{}", arg, jet::Syntax::FILE_EXT)).exists()
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

    // c6vz465: bare `jet` starts the REPL (D-REPL4); `jet ?` is help sugar.
    if raw.is_empty() {
        run_repl(None);
        return;
    }
    if raw.len() == 1 && raw[0] == "?" {
        print!("{}", usage());
        exit(ExitCodes::OK);
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

    let emit_rust = jet_argv.iter().any(|a| a == "--emit-rust");
    let fmt_check = jet_argv.iter().any(|a| a == "--check");
    let json = jet_argv.iter().any(|a| a == "--json");
    let small = jet_argv.iter().any(|a| a == "--small");
    let freestanding = jet_argv.iter().any(|a| a == "--freestanding");
    let allow_impure = jet_argv.iter().any(|a| a == "--allow-impure");
    let locked = jet_argv.iter().any(|a| a == "--locked");
    let annotated = jet_argv.iter().any(|a| a == "--annotated");
    let verbose = jet_argv.iter().any(|a| a == "--verbose" || a == "-v");
    // D-A11YGATE1=B (c134 Phase 6): `jet lint --a11y` — opt-in, never blocking.
    let a11y = jet_argv.iter().any(|a| a == "--a11y");
    // D-TOOL5 (E2-M11): capability summary flags.
    let capabilities_json = jet_argv.iter().any(|a| a == "--capabilities-json");
    // D-SUPPLY1: `jet build --sbom` writes an SPDX SBOM next to the binary.
    let sbom = jet_argv.iter().any(|a| a == "--sbom");
    // E2-M15: cross-compilation target triple (`--target=<triple>`).
    let cross_target: Option<String> = jet_argv
        .iter()
        .find_map(|a| a.strip_prefix("--target=").map(str::to_string));
    // c134 Phase 7: `jet dev <file> --target=web --port=<N>` picks the dev
    // server's port explicitly instead of scanning from 8080.
    let dev_port: Option<u16> = jet_argv
        .iter()
        .find_map(|a| a.strip_prefix("--port=").map(str::to_string))
        .map(|s| {
            s.parse::<u16>().unwrap_or_else(|_| {
                eprintln!("error: `--port={}` isn't a valid port number", s);
                eprintln!(" fix: use a number from 1 to 65535, e.g. `--port=3000`");
                exit(ExitCodes::USAGE);
            })
        });
    let explain_partition = jet_argv.iter().any(|a| a == "--explain-partition");
    // D-BUILDPROFILE1: `--release` is sugar for `--profile=release`.
    // `--profile=<name>` selects a named profile. Resolved against pkg.jet
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
    let mode = OutputMode {
        json,
        color: parse_color(jet_argv),
    };
    let args: Vec<&String> = jet_argv.iter().filter(|a| !a.starts_with("--")).collect();

    if args.first().map(|s| s.as_str()) == Some("lsp") {
        let sub = args.get(1).map(|s| s.as_str());
        let bench_flag = raw.iter().any(|a| a == "--bench");
        match (sub, bench_flag) {
            (Some("doctor"), _) => {
                jet::LSP::run_doctor();
                return;
            }
            (_, true) | (Some("--bench"), _) => {
                // jet lsp --bench: run latency benchmark on a small program
                let src = include_str!("../examples/features/16_wordcount.jet");
                jet::LSP::run_bench(src, 10, 200);
                return;
            }
            _ => {}
        }
        if let Err(e) = jet::LSP::run_stdio() {
            eprintln!("error: language server failed: {}", e);
            exit(ExitCodes::USER_ERROR);
        }
        return;
    }

    let cmd = match args.first() {
        Some(c) => c.as_str(),
        None => {
            // No-args: a friendly greeting that orients, NOT a usage error.
            print!("{}", greeting());
            exit(ExitCodes::OK);
        }
    };

    // If the first word is neither a built-in nor a recognized package/pkg
    // command, try an external `jet-<cmd>` on PATH (D-DX5, cargo/git style),
    // else teach E2101 with a "did you mean".
    let known = jet::CLI::is_builtin(cmd)
        || matches!(
            cmd,
            "lsp"
                | "install"
                | "doctor"
                | "completions"
                | "man"
                | "dev"
                | "serve"
                | "debug"
                | "publish"
                | "yank"
                | "vendor"
                | "audit"
                | "sbom"
                | "emit"
                | "bench"
                | "repl"
                | "schema"
                | "semindex"
                | "impact"
        );
    if !known {
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
                small,
                freestanding,
                allow_impure,
                cross_target.as_deref(),
                explain_partition,
                verbose,
                capabilities_json,
                sbom,
                named_profile.as_deref(),
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
                    eprintln!("error: couldn't run `{}`: {}", bin.display(), e);
                    exit(ExitCodes::USER_ERROR);
                });
            exit(status.code().unwrap_or(ExitCodes::OK));
        }
        unknown_subcommand(cmd);
    }

    // Validate flags against the registry; an unknown/half-typed flag is E2102.
    // Skipped for commands that own a bespoke flag vocabulary or forward flags
    // downstream (so their flags aren't measured against the global set).
    let owns_flags = matches!(
        cmd,
        "env"
            | "dev"
            | "serve"
            | "add"
            | "remove"
            | "bind"
            | "lsp"
            | "store"
            | "update"
            | "fetch"
            | "publish"
            | "yank"
            | "vendor"
            | "audit"
            | "sbom"
            | "repl"
            | "schema"
            | "semindex"
            | "impact"
    );
    if !owns_flags {
        check_flags(jet_argv, cmd);
    }

    // Commands with no required positional target.
    match cmd {
        "help" => {
            print!("{}", usage());
            exit(ExitCodes::OK);
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
        "publish" => {
            let force = raw.iter().any(|a| a == "--force");
            run_publish(force, mode);
            return;
        }
        "yank" => {
            // D-VERSION1=A: mark a published version as yanked (no delete).
            // `jet yank <version> [--message <reason>]`
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
            // D-MIGRATE2C: `jet schema status` / `jet schema squash --before <ver>`.
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
        "impact" => {
            // D-IMPACT1: blast-radius queries over the semantic index.
            let impact_args: Vec<String> = raw.iter().skip(1).cloned().collect();
            run_impact(&impact_args, mode.json);
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
            exit(jet::Jetpack::run(fwd));
        }
        "repl" => {
            // E2-M18: interactive REPL (D-REPL1=A, D-REPL3=A).
            let project = raw
                .iter()
                .find_map(|a| a.strip_prefix("--project=").map(str::to_string))
                .or_else(|| flag_value(&raw, "--project").map(str::to_string));
            run_repl(project.as_deref());
            return;
        }
        // Teaching error: E0043 `jet install` → `jet fetch`
        "install" => {
            eprintln!("Error [E0043]: `jet install` isn't a Jet command");
            eprintln!(" Why: Jet uses `jet fetch` to download and link dependencies");
            eprintln!(" Fix: run `jet fetch` to install all dependencies listed in pkg.jet");
            exit(ExitCodes::USER_ERROR);
        }
        "dev" => {
            // E2-M4 (D-DEV4): the watch/interpret loop. Re-check and re-run the
            // entry file on every save, streaming output, for sub-200ms
            // feedback. The interpreter is a dev convenience only — `jet build`/
            // `jet run` never touch it (I2/I3).
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
                    eprintln!(
                        "error: `jet dev` needs a file to watch: {} dev <file.{}>",
                        jet::Syntax::BINARY_NAME,
                        jet::Syntax::FILE_EXT
                    );
                    exit(ExitCodes::USAGE);
                }
            };
            // c-devserver (owner-directed 2026-07-01): a `.jet` file can define
            // its own `jet dev` behavior as ordinary Jet code — a top-level
            // `fn dev()` becomes the program's real (native) entry point,
            // normally configuring and starting a `core.devserver` value.
            // Checked FIRST, ahead of the #Target(Web)-inferred built-in web
            // server below: `fn dev()` is the more specific, user-authored
            // override, so a file that carries BOTH `#Target(Web)` (a build
            // default) and `fn dev()` (an explicit dev-command override) must
            // run the override, not silently fall back to the built-in server
            // because a *different* marker also happened to be present. (This
            // ordering bug was caught during manual verification — the first
            // cut checked #Target(Web) first, which made `fn dev()` totally
            // unreachable on any file that also declared #Target(Web), e.g.
            // 196_ui_web_click.jet, which has both.)
            if has_dev_entry_fn(file) {
                run_dev_entry(file, mode);
                return;
            }
            // c134 Phase 7: `jet dev <file> --target=web` compiles to JS/WASM
            // and serves `build/` with browser live-reload — a completely
            // different execution model from the native interpret/hot-swap
            // loop above, so it's a separate function, not a new branch
            // inside `run_dev`'s interpreter machinery.
            // D-WEBDEFAULT1 (open): no explicit --target= falls back to the
            // file's own `#Target(Web)` marker, if any.
            if effective_target("dev", file, cross_target.as_deref()).as_deref()
                == Some(jet::Syntax::BUILD_TARGET_WEB)
            {
                run_dev_web(file, mode, verbose, dev_port);
                return;
            }
            run_dev(file, try_anyway, policy, mode, use_interpreter);
            return;
        }
        "serve" => {
            // c77 (D-DEVMODE1=A): `jet serve <entry>` == `jet dev <entry> --swap`.
            // Force the resident/swap path — a type-stable edit hot-swaps, a
            // type-changing edit announces a clean restart (D-HOTSWAP1).
            let try_anyway = raw.iter().any(|a| a == "--try-anyway");
            // c139 (D-JIT2=A): --interpret forces tier-0 interpreter.
            let use_interpreter = raw.iter().any(|a| a == "--interpret");
            let file = match args.get(1) {
                Some(f) => f.as_str(),
                None => {
                    eprintln!(
                        "error: `jet serve` needs a file to serve: {} serve <file.{}>",
                        jet::Syntax::BINARY_NAME,
                        jet::Syntax::FILE_EXT
                    );
                    eprintln!(
                        " note: `jet serve` is `jet dev <file> --swap` — it keeps a resident program up and hot-swaps type-stable edits"
                    );
                    exit(ExitCodes::USAGE);
                }
            };
            // `serve` forces the swap path by default, but `--watch=off` still
            // runs once and exits.
            let policy = watch_policy_from(&raw, WatchPolicy::Swap);
            run_serve(file, try_anyway, policy, mode, use_interpreter);
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
            let file = match args.get(1) {
                Some(f) => f.as_str(),
                None => {
                    eprintln!(
                        "error: `jet debug` needs a file to debug: {} debug <file.{}>",
                        jet::Syntax::BINARY_NAME,
                        jet::Syntax::FILE_EXT
                    );
                    exit(ExitCodes::USAGE);
                }
            };
            let resolved = resolve_source_path(file);
            let use_native = dap || jet::Debug::needs_native(&resolved).unwrap_or(false);
            if !use_native {
                exit(jet::Debug::run_debug(&resolved));
            }
            exit(run_debug_native(&resolved, raw_frames, dap, mode));
        }
        "store" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "verify" => run_store_verify(),
                "rollback" => {
                    // D-PURE3=B (E2-M16): roll back to a prior store generation.
                    let gen_str = args.get(2).map(|s| s.as_str()).unwrap_or("");
                    run_store_rollback(gen_str);
                }
                "generations" => run_store_generations(),
                _ => {
                    eprintln!("error: unknown store subcommand `{}`", sub);
                    eprintln!(" fix: try `jet store verify`, `jet store generations`, or `jet store rollback <gen>`");
                    exit(ExitCodes::USAGE);
                }
            }
            return;
        }
        "eval" => {
            // S60 / D-PURE1 (E2-M16): deterministic evaluation of a pure program.
            let pure_flag = raw.iter().any(|a| a == "--pure");
            let file = match args.get(1) {
                Some(f) => f.as_str(),
                None => {
                    eprintln!(
                        "error: `jet eval` needs a file: {} eval --pure <file.{}>",
                        jet::Syntax::BINARY_NAME,
                        jet::Syntax::FILE_EXT
                    );
                    exit(ExitCodes::USAGE);
                }
            };
            let resolved = resolve_source_path(file);
            run_eval(&resolved, pure_flag, mode);
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
                    if let Some(root) = jet::Loader::find_manifest_root(&cwd) {
                        let entry = find_project_entry(&root);
                        let entry_str = entry.to_string_lossy().to_string();
                        match cmd {
                            "test" => {
                                run_test(&entry_str, false, mode);
                                return;
                            }
                            _ => {
                                // D-CLI1: use passthrough slice if `--` was present;
                                // otherwise fall back to positional words after the subcommand.
                                let program_args: Vec<&String> = if passthrough_sep.is_some() {
                                    passthrough.clone()
                                } else {
                                    args.iter().skip(1).copied().collect()
                                };
                                let effective = effective_target(cmd, &entry_str, cross_target.as_deref());
                                run_compile_cmd(
                                    cmd,
                                    &entry_str,
                                    emit_rust,
                                    small,
                                    freestanding,
                                    allow_impure,
                                    effective.as_deref(),
                                    explain_partition,
                                    verbose,
                                    capabilities_json,
                                    sbom,
                                    named_profile.as_deref(),
                                    &program_args,
                                    mode,
                                );
                                return;
                            }
                        }
                    }
                    eprintln!(
                        "error: no file given and no `pkg.jet` found in this directory or above"
                    );
                    eprintln!(
                        " fix: run `jet {} <file.{}>` or cd into a project",
                        cmd,
                        jet::Syntax::FILE_EXT
                    );
                    exit(ExitCodes::USAGE);
                }
                _ => {
                    eprint!("{}", usage());
                    exit(ExitCodes::USAGE);
                }
            }
        }
    };

    match cmd {
        "fmt" => run_fmt(target, fmt_check),
        "fix" => run_fix(target, jet_argv.iter().any(|a| a == "--dry-run")),
        "new" => run_new(target, annotated),
        "test" => {
            let update_snapshots = jet_argv
                .iter()
                .any(|a| a == "--update-snapshots" || a == "-u");
            // D-COV1: `jet test --coverage` builds an instrumented harness and
            // reports per-function / per-line coverage after the test results.
            let coverage = jet_argv.iter().any(|a| a == "--coverage");
            // A directory target is a project root: resolve its entry.
            let resolved = resolve_source_path(target);
            run_test_cov(&resolved, update_snapshots, coverage, mode);
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
            run_bench(target, mode);
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
            eprintln!(" Fix: run `jet fetch` to install all dependencies listed in pkg.jet");
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
            let effective = effective_target(cmd, &resolved, cross_target.as_deref());
            run_compile_cmd(
                cmd,
                &resolved,
                emit_rust,
                small,
                freestanding,
                allow_impure,
                effective.as_deref(),
                explain_partition,
                verbose,
                capabilities_json,
                sbom,
                named_profile.as_deref(),
                &program_args,
                mode,
            );
        }
    }
}

/// Resolve a source-path argument, allowing the `.jet` extension to be omitted
/// (ext-optional CLI). If `raw` exists as-is, use it. Otherwise, if `raw.jet`
/// exists, use that. If neither exists, return `raw` unchanged so the normal
/// file-not-found diagnostic fires with the original name the user typed.
/// D-WEBDEFAULT1 (open, c134): resolve the effective `--target=` value for
/// `file`. Precedence: an explicit CLI flag always wins; else `pkg.jet`'s
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

/// D-WEBDEFAULT1 (open, c134): `pkg.jet`'s `target: "web"`, if `file` sits
/// inside a managed package (found via the same `find_manifest_root` walk
/// `jet run`/`jet build` already use to resolve project-root mode).
fn manifest_default_target(file: &str) -> Option<String> {
    let start = Path::new(file).parent().unwrap_or(Path::new("."));
    let root = jet::Loader::find_manifest_root(start)?;
    let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).ok()?;
    let manifest = jet::Jetpack::PackageManifest::parse(&raw).ok()?;
    manifest.package.target
}

pub(crate) fn resolve_source_path(raw: &str) -> String {
    let path = Path::new(raw);
    // A directory argument is a project root: resolve its entry
    // (`.jet/main.jet` else `<dir>/main.jet`). `jet run <dir>` just works.
    if path.is_dir() {
        let entry = find_project_entry(path);
        if entry.is_file() {
            return entry.to_string_lossy().into_owned();
        }
        // No entry: if there's no manifest either, surface a clean error; with
        // a manifest but no entry, fall through so the file-not-found path
        // names the missing `main.jet`.
        if !path.join(jet::Syntax::PAYLOAD_FILE).is_file() {
            eprintln!(
                "error: no `main.{ext}` or `.jet/main.{ext}` entry in `{dir}`",
                ext = jet::Syntax::FILE_EXT,
                dir = raw,
            );
            eprintln!(
                " fix: add a `main.{}` to that directory, or point at a `.jet` file directly",
                jet::Syntax::FILE_EXT
            );
            exit(ExitCodes::USER_ERROR);
        }
        return entry.to_string_lossy().into_owned();
    }
    if path.exists() {
        return raw.to_string();
    }
    let with_ext = format!("{}.{}", raw, jet::Syntax::FILE_EXT);
    if Path::new(&with_ext).exists() {
        return with_ext;
    }
    raw.to_string()
}

/// Find the entry .jet file for a project (`.jet/main.jet` if exists, else `main.jet`).
pub(crate) fn find_project_entry(root: &Path) -> PathBuf {
    let dot_jet = root
        .join(".jet")
        .join(format!("main.{}", jet::Syntax::FILE_EXT));
    if dot_jet.is_file() {
        return dot_jet;
    }
    root.join(format!("main.{}", jet::Syntax::FILE_EXT))
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
        eprint!("{}", jet::render_all_json(file, src, diags));
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
            jet::Explain::pointer_line(first.code, mode.color_stderr())
        );
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
