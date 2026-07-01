//! Single source of truth for the `jet` command surface (E2-M3, D-DX4).
//!
//! Shell completions (`jet completions <shell>`), man pages (`jet man`), and the
//! E2101 "did you mean" suggester all derive from the tables here, so they can
//! never drift from one another. The driver's `KNOWN_COMMANDS`/`KNOWN_FLAGS`
//! lists are themselves generated from this module (see `command_names` /
//! `flag_names`), keeping one authority for "what commands and flags exist".
//!
//! Everything is std-only (I6): the completion scripts and the roff man page are
//! hand-written generators over these structs — no clap, no clap_complete, no
//! roff crate. The plan flags any completion/line-handling crate as owner-gated,
//! and we do not have approval, so we emit the scripts ourselves.

use crate::Syntax::{BINARY_NAME, FILE_EXT};

/// What kind of positional argument a command takes, used to decide whether a
/// completion should offer file paths after the subcommand.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Arg {
    /// No positional argument (e.g. `fetch`, `doctor`, `version`).
    None,
    /// A single `.jet` source file (or a project dir): `run`, `check`, `build`.
    File,
    /// A path to a file or directory: `test`, `fmt`, `fix`, `bind`.
    Path,
    /// A free-form name, not a path: `new`, `add`, `remove`, `explain`.
    Name,
}

/// One subcommand: its name, one-line summary (shared by `--help` and the man
/// page), the long flags it accepts, and what it completes positionally.
pub struct CommandSpec {
    pub name: &'static str,
    pub summary: &'static str,
    /// Long flags specific to this command (without the leading `--`). Global
    /// flags like `--color`/`--json` are listed once in `GLOBAL_FLAGS`.
    pub flags: &'static [&'static str],
    pub arg: Arg,
}

/// One long flag and its one-line meaning, for completions and the man page.
pub struct FlagSpec {
    /// Flag name WITHOUT the leading `--`.
    pub name: &'static str,
    pub summary: &'static str,
}

/// Flags accepted regardless of subcommand (presentation + machine output).
pub const GLOBAL_FLAGS: &[FlagSpec] = &[
    FlagSpec { name: "color", summary: "when to colorize: auto|always|never" },
    FlagSpec { name: "json", summary: "emit machine-readable JSON diagnostics" },
    FlagSpec { name: "version", summary: "print the compiler version and exit" },
];

/// One-line meaning for every per-command flag, so the man page and completions
/// can describe `--small`, `--git`, etc. (not just the global flags).
const FLAG_HELP: &[(&str, &str)] = &[
    ("small", "build the smallest possible binary (S15)"),
    ("emit-rust", "also print the generated Rust code"),
    ("capabilities-json", "emit capability summary as JSON (D-TOOL5)"),
    ("update-snapshots", "update snapshot golden files (D-TOOL4)"),
    ("u", "short form of --update-snapshots"),
    ("rust", "with `emit`: print the generated Rust source (D-TOOL3)"),
    ("annotated", "with `new`: include commented example deps"),
    ("check", "with `fmt`: exit 1 if the file would change (CI)"),
    ("bench", "with `lsp`: run the latency benchmark"),
    ("fix", "with `doctor`: apply safe auto-fixes"),
    ("write", "with `fix`: write the fixes to disk (default is a dry-run preview)"),
    ("apply", "with `fix`: alias for --write"),
    ("online", "with `doctor`: allow the registry probe"),
    ("network", "with `doctor`: allow the registry probe"),
    ("plain", "with `doctor`: deterministic ASCII output"),
    ("path", "add a path dependency from a local directory"),
    ("git", "add a git dependency from a URL"),
    ("tag", "with `--git`: pin to a tag"),
    ("branch", "with `--git`: track a branch"),
    ("rev", "with `--git`: pin to a commit"),
    ("locked", "with `fetch`: verify the lock only, no network"),
    ("pkg", "with `bind`: the C library link key"),
    ("out", "with `bind`: the output cache path"),
    // D-BUILDPROFILE1 (ratified 2026-06-25): named build profiles.
    ("release", "with `build`/`run`: use the release profile (D-BUILDPROFILE1)"),
    ("profile", "with `build`/`run`: named build profile --profile=<name> (D-BUILDPROFILE1)"),
    ("target", "with `build`: cross-compile for a rustc target triple or `web` (E2-M15)"),
    (
        "explain-partition",
        "with `build --target=web`: print the JS/WASM partition report (D-WASM1)",
    ),
    // D-DBG2/D-DBG3 step 2 (dap-debugger): the native lldb-backed backend.
    ("raw-frames", "with `debug`: show raw Rust frames/lines instead of Jet terms (D-DBG2 expert opt-in)"),
    ("dap", "with `debug`: speak the Debug Adapter Protocol on stdio instead of the `(jet)` terminal prompt (editor wiring)"),
];

/// Human description for a flag (global or per-command), for man/completions.
pub fn flag_help(name: &str) -> &'static str {
    if let Some(g) = GLOBAL_FLAGS.iter().find(|g| g.name == name) {
        return g.summary;
    }
    FLAG_HELP
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| *s)
        .unwrap_or("(see the manual)")
}

/// Every built-in subcommand. This is THE list; `KNOWN_COMMANDS` in the driver
/// is generated from it (see `command_names`).
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec { name: "check", summary: "look for problems, build nothing", flags: &["json"], arg: Arg::File },
    CommandSpec { name: "build", summary: "compile to a native binary in ./build/", flags: &["small", "emit-rust", "json", "capabilities-json", "release", "profile", "target", "explain-partition"], arg: Arg::File },
    CommandSpec { name: "run", summary: "build, then run (or run a project)", flags: &["small", "emit-rust", "json", "release", "profile"], arg: Arg::File },
    CommandSpec { name: "test", summary: "compile and run top-level test blocks", flags: &["json", "update-snapshots", "u", "coverage"], arg: Arg::Path },
    CommandSpec { name: "emit", summary: "emit the generated Rust source (D-TOOL3)", flags: &["rust"], arg: Arg::File },
    CommandSpec { name: "bench", summary: "benchmark a Jet program (D-TOOL5)", flags: &["json"], arg: Arg::File },
    CommandSpec { name: "new", summary: "create a new project folder", flags: &["annotated"], arg: Arg::Name },
    CommandSpec { name: "dev", summary: "enter the project shell (jetpack enter)", flags: &[], arg: Arg::None },
    CommandSpec { name: "debug", summary: "step through a program at the Jet source level (D-DBG3)", flags: &["raw-frames", "dap"], arg: Arg::File },
    CommandSpec { name: "fmt", summary: "rewrite a file to canonical style", flags: &["check"], arg: Arg::Path },
    CommandSpec { name: "fix", summary: "preview auto-fixable diagnostics (use --write to apply)", flags: &["write", "apply"], arg: Arg::Path },
    CommandSpec { name: "bind", summary: "generate a C binding cache from a header", flags: &["pkg", "out"], arg: Arg::Path },
    CommandSpec { name: "lsp", summary: "language server (stdio JSON-RPC)", flags: &["bench"], arg: Arg::None },
    CommandSpec { name: "explain", summary: "print the offline essay for a diagnostic code", flags: &[], arg: Arg::Name },
    CommandSpec { name: "doctor", summary: "diagnose your environment (rustc, cache, PATH, FFI)", flags: &["fix", "online", "network", "plain"], arg: Arg::None },
    CommandSpec { name: "completions", summary: "print a shell completion script (bash|zsh|fish)", flags: &[], arg: Arg::Name },
    CommandSpec { name: "man", summary: "print the manual page (roff)", flags: &[], arg: Arg::Name },
    CommandSpec { name: "version", summary: "print the compiler version", flags: &[], arg: Arg::None },
    CommandSpec { name: "help", summary: "print the help text", flags: &[], arg: Arg::None },
    CommandSpec { name: "upgrade", summary: "how to download a newer release", flags: &[], arg: Arg::None },
    CommandSpec { name: "add", summary: "add a dependency and fetch it", flags: &["path", "git", "tag", "branch", "rev"], arg: Arg::Name },
    CommandSpec { name: "remove", summary: "remove a dependency", flags: &[], arg: Arg::Name },
    CommandSpec { name: "fetch", summary: "download and link all dependencies", flags: &["locked"], arg: Arg::None },
    CommandSpec { name: "update", summary: "refresh @latest / branch selectors", flags: &[], arg: Arg::Name },
    CommandSpec { name: "store", summary: "inspect the content-addressed store", flags: &[], arg: Arg::Name },
    CommandSpec { name: "gc", summary: "remove unreferenced store entries", flags: &[], arg: Arg::None },
    CommandSpec { name: "install", summary: "(redirected) use `jet fetch` instead", flags: &[], arg: Arg::None },
];

/// Every built-in subcommand name, in declaration order. The driver's
/// `KNOWN_COMMANDS` is exactly this (the E2101 suggester's candidate set).
pub fn command_names() -> Vec<&'static str> {
    COMMANDS.iter().map(|c| c.name).collect()
}

/// Look up a command by name.
pub fn command(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// All long flag names (without `--`) the driver should accept: every global
/// flag plus every per-command flag, de-duplicated. The driver's `KNOWN_FLAGS`
/// is generated from this so a flag added to a command's table also passes the
/// E2102 gate automatically.
pub fn flag_names() -> Vec<String> {
    let mut out: Vec<String> = GLOBAL_FLAGS.iter().map(|f| f.name.to_string()).collect();
    for c in COMMANDS {
        for &f in c.flags {
            if !out.iter().any(|x| x == f) {
                out.push(f.to_string());
            }
        }
    }
    out
}

// ── Completion script generators (D-DX4) ────────────────────────────────────

/// Subcommand names joined by a single space (for shell word lists).
fn names_joined() -> String {
    command_names().join(" ")
}

/// All long flags rendered with the `--` prefix, space-joined.
fn flags_joined() -> String {
    flag_names()
        .iter()
        .map(|f| format!("--{}", f))
        .collect::<Vec<_>>()
        .join(" ")
}

/// True if a command completes file paths positionally.
fn takes_file(c: &CommandSpec) -> bool {
    matches!(c.arg, Arg::File | Arg::Path)
}

/// Generate a bash completion script driven entirely by the spec tables.
pub fn completions_bash() -> String {
    let bin = BINARY_NAME;
    let cmds = names_joined();
    let flags = flags_joined();
    // Commands that should offer file completion for their first argument.
    let file_cmds: Vec<&str> = COMMANDS.iter().filter(|c| takes_file(c)).map(|c| c.name).collect();
    let file_cmds_j = file_cmds.join(" ");
    format!(
        "# {bin} bash completion (generated by `{bin} completions bash`)\n\
# Source from your bashrc:  source <({bin} completions bash)\n\
_{bin}() {{\n\
\x20   local cur prev words cword\n\
\x20   _init_completion 2>/dev/null || {{ cur=\"${{COMP_WORDS[COMP_CWORD]}}\"; prev=\"${{COMP_WORDS[COMP_CWORD-1]}}\"; }}\n\
\x20   local subcommands=\"{cmds}\"\n\
\x20   local globalflags=\"{flags}\"\n\
\x20   local filecmds=\"{file_cmds_j}\"\n\
\x20   # The subcommand is the first non-flag word after `{bin}`.\n\
\x20   local sub=\"\"\n\
\x20   local i\n\
\x20   for ((i=1; i < COMP_CWORD; i++)); do\n\
\x20       case \"${{COMP_WORDS[i]}}\" in\n\
\x20           --*) ;;\n\
\x20           *) sub=\"${{COMP_WORDS[i]}}\"; break ;;\n\
\x20       esac\n\
\x20   done\n\
\x20   if [[ \"$cur\" == --* ]]; then\n\
\x20       COMPREPLY=( $(compgen -W \"$globalflags\" -- \"$cur\") )\n\
\x20       return 0\n\
\x20   fi\n\
\x20   if [[ -z \"$sub\" ]]; then\n\
\x20       COMPREPLY=( $(compgen -W \"$subcommands\" -- \"$cur\") )\n\
\x20       return 0\n\
\x20   fi\n\
\x20   case \" $filecmds \" in\n\
\x20       *\" $sub \"*) COMPREPLY=( $(compgen -f -- \"$cur\") ); return 0 ;;\n\
\x20   esac\n\
\x20   COMPREPLY=( $(compgen -W \"$globalflags\" -- \"$cur\") )\n\
\x20   return 0\n\
}}\n\
complete -F _{bin} {bin}\n",
    )
}

/// Generate a zsh completion script driven by the spec tables. Each subcommand
/// carries its summary so zsh shows it in the completion menu.
pub fn completions_zsh() -> String {
    let bin = BINARY_NAME;
    let mut subs = String::new();
    for c in COMMANDS {
        // zsh `_describe` "name:description" pairs; escape colons in summaries.
        let summary = c.summary.replace(':', "\\:");
        subs.push_str(&format!("        '{}:{}'\n", c.name, summary));
    }
    let mut fdesc = String::new();
    for f in GLOBAL_FLAGS {
        let summary = f.summary.replace(':', "\\:");
        fdesc.push_str(&format!("        '--{}[{}]'\n", f.name, summary));
    }
    let file_cmds: Vec<&str> = COMMANDS.iter().filter(|c| takes_file(c)).map(|c| c.name).collect();
    let file_cmds_j = file_cmds.join(" ");
    format!(
        "#compdef {bin}\n\
# {bin} zsh completion (generated by `{bin} completions zsh`)\n\
# Put on your fpath, or:  {bin} completions zsh > ~/.zfunc/_{bin}\n\
_{bin}() {{\n\
\x20   local -a subcommands\n\
\x20   subcommands=(\n{subs}    )\n\
\x20   local -a globalflags\n\
\x20   globalflags=(\n{fdesc}    )\n\
\x20   local filecmds=\"{file_cmds_j}\"\n\
\x20   if (( CURRENT == 2 )); then\n\
\x20       _describe -t commands '{bin} command' subcommands\n\
\x20       return\n\
\x20   fi\n\
\x20   local sub=\"${{words[2]}}\"\n\
\x20   if [[ \" $filecmds \" == *\" $sub \"* ]]; then\n\
\x20       _files\n\
\x20       return\n\
\x20   fi\n\
\x20   _describe -t flags 'flag' globalflags\n\
}}\n\
_{bin} \"$@\"\n",
    )
}

/// Generate a fish completion script driven by the spec tables.
pub fn completions_fish() -> String {
    let bin = BINARY_NAME;
    let mut out = String::new();
    out.push_str(&format!(
        "# {bin} fish completion (generated by `{bin} completions fish`)\n\
# Save to ~/.config/fish/completions/{bin}.fish\n",
    ));
    // Helper: complete subcommands only when no subcommand word is present yet.
    out.push_str(&format!(
        "function __{bin}_no_subcommand\n\
\x20   set -l cmd (commandline -opc)\n\
\x20   set -e cmd[1]\n\
\x20   for w in $cmd\n\
\x20       switch $w\n\
\x20           case '-*'\n\
\x20           case '*'\n\
\x20               return 1\n\
\x20       end\n\
\x20   end\n\
\x20   return 0\n\
end\n",
    ));
    // One completion line per subcommand, gated on no subcommand seen yet.
    for c in COMMANDS {
        out.push_str(&format!(
            "complete -c {bin} -n '__{bin}_no_subcommand' -f -a '{name}' -d '{desc}'\n",
            name = c.name,
            desc = c.summary.replace('\'', "\\'"),
        ));
    }
    // Global flags, available everywhere.
    for f in GLOBAL_FLAGS {
        out.push_str(&format!(
            "complete -c {bin} -l {name} -d '{desc}'\n",
            name = f.name,
            desc = f.summary.replace('\'', "\\'"),
        ));
    }
    // File completion for file/path commands once that subcommand is chosen.
    for c in COMMANDS.iter().filter(|c| takes_file(c)) {
        out.push_str(&format!(
            "complete -c {bin} -n '__fish_seen_subcommand_from {name}' -F\n",
            name = c.name,
        ));
    }
    out
}

/// Dispatch to the requested shell's generator. Returns `None` for an unknown
/// shell so the driver can teach (exit 2) instead of guessing.
pub fn completions(shell: &str) -> Option<String> {
    match shell {
        "bash" => Some(completions_bash()),
        "zsh" => Some(completions_zsh()),
        "fish" => Some(completions_fish()),
        _ => None,
    }
}

// ── Man page generator (D-DX4) ──────────────────────────────────────────────

/// Escape a string for roff body text: a leading `.` or `'` would be read as a
/// macro, and `\` is the roff escape char.
fn roff_escape(s: &str) -> String {
    let s = s.replace('\\', "\\\\");
    // Lines beginning with control chars are handled by callers via `\&`.
    s
}

/// Generate a roff/man-page (section 1) for `jet` from the spec tables. With a
/// subcommand name, focuses the SYNOPSIS/DESCRIPTION on that command; otherwise
/// documents the whole surface. Same summaries as `--help`, one source.
pub fn man(sub: Option<&str>) -> String {
    let bin = BINARY_NAME;
    let ext = FILE_EXT;
    let upper = bin.to_ascii_uppercase();
    let mut out = String::new();
    // `.TH` title line: name, section, version. Date omitted to stay stable.
    out.push_str(&format!(
        ".TH {upper} 1 \"\" \"{bin} {ver}\" \"{bin} Manual\"\n",
        ver = env!("CARGO_PKG_VERSION"),
    ));
    out.push_str(".SH NAME\n");
    out.push_str(&format!("{bin} \\- the Jet compiler and project tool\n"));

    if let Some(name) = sub {
        if let Some(c) = command(name) {
            out.push_str(".SH SYNOPSIS\n");
            out.push_str(&format!(".B {bin} {name}\n"));
            if takes_file(c) {
                out.push_str(&format!(".I file.{ext}\n"));
            }
            out.push_str(".SH DESCRIPTION\n");
            out.push_str(&format!("{}\n", roff_escape(c.summary)));
            if !c.flags.is_empty() {
                out.push_str(".SH OPTIONS\n");
                for &f in c.flags {
                    out.push_str(&format!(".TP\n\\fB\\-\\-{}\\fR\n", f));
                    out.push_str(&format!("{}\n", roff_escape(flag_help(f))));
                }
            }
            return out;
        }
        // Unknown subcommand: fall through to the full page (caller validates).
    }

    out.push_str(".SH SYNOPSIS\n");
    out.push_str(&format!(".B {bin}\n.I command\n[\\fIargs\\fR...]\n"));
    out.push_str(".SH DESCRIPTION\n");
    out.push_str(&format!(
        "{bin} is the command-line tool for the Jet language: it checks, builds, runs, tests, formats, and packages Jet programs.\n",
    ));
    out.push_str(".SH COMMANDS\n");
    for c in COMMANDS {
        out.push_str(&format!(".TP\n\\fB{}\\fR\n{}\n", c.name, roff_escape(c.summary)));
    }
    out.push_str(".SH GLOBAL OPTIONS\n");
    for f in GLOBAL_FLAGS {
        out.push_str(&format!(".TP\n\\fB\\-\\-{}\\fR\n{}\n", f.name, roff_escape(f.summary)));
    }
    out.push_str(".SH SEE ALSO\n");
    out.push_str(&format!("Run \\fB{bin} explain <code>\\fR for an essay on any diagnostic code.\n"));
    out
}
