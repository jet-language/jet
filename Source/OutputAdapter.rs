//! Host-side output selection.
//!
//! The process boundary reads terminal and environment facts once.  Commands
//! receive the resulting profile or its policy-free `OutputMode` projection;
//! they do not perform another output-policy lookup.

use std::env;
use std::sync::OnceLock;
use std::io::IsTerminal;

use jet::Diagnostics::ColorChoice;
use jet_cli::CLI::OutputFlags;
use jet_cli::OutputProfile::{
    OutputChannel, OutputFacts, OutputKind, OutputProfile, OutputProfileError,
};

const MACHINE_OUTPUT_ENV: &str = "JET_MACHINE_OUTPUT";

/// The command ABI projection of the resolved output profile.
///
/// Existing command modules still consume these three fields.  They are
/// immutable facts projected from `OutputProfile`; the methods below never
/// consult the process environment or probe a stream.
#[derive(Clone, Copy)]
pub(crate) struct OutputMode {
    /// Emit machine-readable output on stdout.
    pub(crate) json: bool,
    /// A resolved color choice, never `Auto` after host selection.
    pub(crate) color: ColorChoice,
    /// Suppress non-error status and progress output.
    pub(crate) quiet: bool,
}

impl OutputMode {
    pub(crate) fn from_profile(profile: OutputProfile, flags: OutputFlags) -> Self {
        Self {
            json: profile.machine_enabled(),
            color: if profile.ansi_enabled() {
                ColorChoice::Always
            } else {
                ColorChoice::Never
            },
            quiet: flags.quiet,
        }
    }

    /// Whether the resolved profile permits ANSI diagnostics.
    pub(crate) fn color_stderr(&self) -> bool {
        matches!(self.color, ColorChoice::Always)
    }
    /// Return the resolved decision for a renderer that carries an explicit
    /// stream fact. Host selection already accounted for stream safety.
    pub(crate) fn color_stderr_for(&self, _is_tty: bool) -> bool {
        self.color_stderr()
    }

    /// Hyperlinks follow the same resolved ANSI decision as diagnostics.
    pub(crate) fn hyperlinks_stderr(&self) -> bool {
        self.color_stderr()
    }
}

/// The command identity that controls ordinary advisory-lint presentation.
///
/// This is deliberately separate from [`OutputMode`]: the latter is a public
/// stream projection consumed by many command helpers, while this policy is a
/// host-owned presentation fact. Semantic checking and lint-policy enforcement
/// continue to receive the complete checked lint set.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CommandIdentity {
    Check,
    Build,
    Run,
    Dev,
    Test,
    Other,
}

impl CommandIdentity {
    fn from_name(name: &str) -> Self {
        match name {
            "check" => Self::Check,
            "build" => Self::Build,
            "run" => Self::Run,
            "dev" => Self::Dev,
            "test" => Self::Test,
            _ => Self::Other,
        }
    }

    fn advisory_lints_visible(self, verbose: bool) -> bool {
        verbose || !matches!(self, Self::Run | Self::Dev | Self::Test)
    }
}

/// One startup snapshot and its resolved output profile.
#[derive(Clone, Copy)]
pub(crate) struct OutputAdapter {
    profile: OutputProfile,
    flags: OutputFlags,
    stdin_terminal: bool,
    stdout_terminal: bool,
    stderr_terminal: bool,
    shell_prefill: bool,
}

impl OutputAdapter {
    /// Read host facts once and select the one profile for this invocation.
    pub(crate) fn select(args: &[String]) -> Result<Self, OutputProfileError> {
        let flags = OutputFlags::parse(args)?;
        let stdin_terminal = std::io::stdin().is_terminal();
        let stdout_terminal = std::io::stdout().is_terminal();
        let stderr_terminal = std::io::stderr().is_terminal();
        let shell_prefill = env::var_os("JET_HELP_SHELL_PREFILL").is_some();
        let term = env::var_os("TERM")
            .and_then(|value| value.into_string().ok())
            .filter(|value| !value.is_empty());
        let no_color = env::var_os("NO_COLOR").is_some();
        let force_color = env::var_os("FORCE_COLOR").is_some();
        let width = env::var_os("COLUMNS")
            .and_then(|value| value.into_string().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0);
        let unicode = locale_supports_unicode();

        // A single profile must be safe for both human streams.  A redirected
        // stdout or stderr therefore disables automatic styling/progress for
        // the whole invocation.  The canonical auto ladder remains
        // explicit choice > NO_COLOR > FORCE_COLOR > TTY.
        let tty = stdout_terminal && stderr_terminal;
        let facts = OutputFacts::new(
            tty,
            term.as_deref(),
            no_color,
            force_color,
            flags.color,
            flags.json,
            width,
            unicode,
        );
        let profile = OutputProfile::select(facts)?;
        Ok(Self {
            profile,
            flags,
            stdin_terminal,
            stdout_terminal,
            stderr_terminal,
            shell_prefill,
        })
    }

    pub(crate) const fn profile(self) -> OutputProfile {
        self.profile
    }

    pub(crate) const fn flags(self) -> OutputFlags {
        self.flags
    }

    pub(crate) fn mode(self) -> OutputMode {
        OutputMode::from_profile(self.profile, self.flags)
    }
    /// Propagate the resolved machine-output fact into every in-process
    /// execution tier. The generated Prelude reads this same environment key
    /// dynamically, so the selection made at the process boundary is visible
    /// before a child or resident engine emits its first progress frame.
    pub(crate) fn apply_machine_mode(self) {
        if self.profile.machine_enabled() {
            env::set_var(MACHINE_OUTPUT_ENV, "1");
        } else {
            env::remove_var(MACHINE_OUTPUT_ENV);
        }
    }
    /// Publish this invocation's resolved profile for diagnostics emitted by
    /// legacy command helpers that cannot receive a command argument. This is
    /// a fact handoff, not a second environment or terminal-policy lookup.
    pub(crate) fn activate(self) {
        let _ = ACTIVE_PROFILE.set(self.profile);
        let _ = ACTIVE_VERBOSE.set(self.flags.verbose);
    }

    /// Bind the already-parsed command identity to this invocation's output
    /// policy. The command is set after the host has split Jet arguments from
    /// the program tail, so child arguments cannot change lint visibility.
    pub(crate) fn activate_command(command: &str) {
        let _ = ACTIVE_COMMAND.set(CommandIdentity::from_name(command));
    }


    /// Preserve the shell-widget exception used by `jet ?`: the palette may
    /// use stderr while stdout is captured for the selected command line.
    pub(crate) const fn help_interactive(self) -> bool {
        self.stdin_terminal
            && (self.stdout_terminal
                || (self.shell_prefill && self.stderr_terminal))
    }

    pub(crate) const fn stdin_is_terminal(self) -> bool {
        self.stdin_terminal
    }

    pub(crate) const fn stderr_is_terminal(self) -> bool {
        self.stderr_terminal
    }
}

/// Write human primary content only to the profile-selected human channel.
pub(crate) fn write_renderable(profile: OutputProfile, text: &str) {
    match profile.channel(OutputKind::Renderable) {
        OutputChannel::HumanStdout => print!("{text}"),
        OutputChannel::HumanStderr => eprint!("{text}"),
        OutputChannel::MachineStdout | OutputChannel::Hidden => {}
    }
}

/// Write a human status line only when the profile permits status output.
pub(crate) fn write_status(profile: OutputProfile, text: &str) {
    if profile.channel(OutputKind::Status) == OutputChannel::HumanStderr {
        eprint!("{text}");
    }
}

/// Write structured records only to machine stdout.
pub(crate) fn write_machine(profile: OutputProfile, text: &str) {
    if profile.channel(OutputKind::Machine) == OutputChannel::MachineStdout {
        print!("{text}");
    }
}
/// Write primary human content using the already-resolved command mode.
pub(crate) fn write_mode_renderable(mode: OutputMode, text: &str) {
    if !mode.json {
        print!("{text}");
    }
}

/// Write a non-error human status without contaminating machine stdout.
pub(crate) fn write_mode_status(mode: OutputMode, text: &str) {
    if !mode.json && !mode.quiet {
        eprint!("{text}");
    }
}

/// Write a diagnostic to stderr in either output mode. Machine mode reserves
/// stdout for structured/program bytes; diagnostics remain a separate stream.
pub(crate) fn write_mode_diagnostic(_mode: OutputMode, text: &str) {
    eprint!("{text}");
}
/// Write ephemeral progress using the command mode's resolved human channel.
pub(crate) fn write_mode_progress(mode: OutputMode, text: &str) {
    write_mode_status(mode, text);
}

/// Write a structured record only in machine mode.
pub(crate) fn write_mode_machine(mode: OutputMode, text: &str) {
    if mode.json {
        print!("{text}");
    }
}


fn locale_supports_unicode() -> bool {
    let locale = env::var_os("LC_ALL")
        .or_else(|| env::var_os("LC_CTYPE"))
        .or_else(|| env::var_os("LANG"))
        .and_then(|value| value.into_string().ok());
    locale.map_or(true, |value| {
        let upper = value.to_ascii_uppercase();
        upper != "C" && upper != "POSIX" && !upper.contains("ASCII")
    })
}

static ACTIVE_PROFILE: OnceLock<OutputProfile> = OnceLock::new();
static ACTIVE_VERBOSE: OnceLock<bool> = OnceLock::new();
static ACTIVE_COMMAND: OnceLock<CommandIdentity> = OnceLock::new();

pub(crate) fn active_profile() -> Option<OutputProfile> {
    ACTIVE_PROFILE.get().copied()
}

/// Whether ordinary advisory lints belong in the current command's diagnostic
/// presentation. This does not change checking or policy enforcement; it only
/// controls the shared CLI report boundary.
pub(crate) fn advisory_lints_visible() -> bool {
    let command = ACTIVE_COMMAND
        .get()
        .copied()
        .unwrap_or(CommandIdentity::Other);
    let verbose = ACTIVE_VERBOSE.get().copied().unwrap_or(false);
    command.advisory_lints_visible(verbose)
}
