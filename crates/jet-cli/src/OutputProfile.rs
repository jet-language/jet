//! Deterministic CLI output capabilities and channel routing.
//!
//! The host collects process facts (terminal state and environment policy) and
//! passes them here.  This module deliberately performs no environment or
//! terminal reads, so selection is deterministic and testable.

/// The color policy requested by the caller.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ColorRequest {
    /// Enable color when the resolved precedence permits it.
    #[default]
    Auto,
    /// Enable ANSI styling unless machine mode forbids it.
    Always,
    /// Never emit ANSI styling.
    Never,
}

impl ColorRequest {
    /// Parse the exact command-line spellings accepted by the output policy.
    pub fn parse(value: &str) -> Result<Self, OutputProfileError> {
        match value {
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            "never" => Ok(Self::Never),
            _ => Err(OutputProfileError::InvalidColorRequest(value.to_owned())),
        }
    }
}

impl std::str::FromStr for ColorRequest {
    type Err = OutputProfileError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl std::fmt::Display for ColorRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        })
    }
}

/// Facts supplied by the process boundary to [`OutputProfile::select`].
///
/// Every field is explicit.  In particular, `term`, `no_color`, and
/// `force_color` are values already read by the caller; selecting a profile
/// never reads the process environment or probes a stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputFacts {
    /// Whether the primary human output stream is a terminal.
    pub tty: bool,
    /// The `TERM` value, if one was supplied by the host.
    pub term: Option<String>,
    /// Whether the `NO_COLOR` presence policy was observed.
    pub no_color: bool,
    /// Whether the `FORCE_COLOR` presence policy was observed.
    pub force_color: bool,
    /// The explicit color request (`auto`, `always`, or `never`).
    pub requested_color: ColorRequest,
    /// Whether the command requested structured machine output.
    pub json: bool,
    /// An optional terminal column budget.  Omitted means the canonical 80
    /// column default.
    pub width: Option<usize>,
    /// Whether the output encoding supports Unicode glyphs.
    pub unicode: bool,
}

impl Default for OutputFacts {
    fn default() -> Self {
        Self {
            tty: false,
            term: None,
            no_color: false,
            force_color: false,
            requested_color: ColorRequest::Auto,
            json: false,
            width: None,
            unicode: true,
        }
    }
}

impl OutputFacts {
    pub fn new(
        tty: bool,
        term: Option<&str>,
        no_color: bool,
        force_color: bool,
        requested_color: ColorRequest,
        json: bool,
        width: Option<usize>,
        unicode: bool,
    ) -> Self {
        Self {
            tty,
            term: term.map(str::to_owned),
            no_color,
            force_color,
            requested_color,
            json,
            width,
            unicode,
        }
    }
}

/// The semantic output stream selected for one output kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputChannel {
    /// Human-readable primary content on stdout.
    HumanStdout,
    /// Human-readable status, log, or progress content on stderr.
    HumanStderr,
    /// Structured machine records on stdout.
    MachineStdout,
    /// This output kind is not emitted for the selected profile.
    Hidden,
}

impl OutputChannel {
    pub const fn is_human(self) -> bool {
        matches!(self, Self::HumanStdout | Self::HumanStderr)
    }

    pub const fn is_machine(self) -> bool {
        matches!(self, Self::MachineStdout)
    }

    pub const fn is_hidden(self) -> bool {
        matches!(self, Self::Hidden)
    }
}

/// The semantic output kind whose channel is being requested.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputKind {
    /// Tables, explanations, and other primary human content.
    Renderable,
    /// A human status line or status block.
    Status,
    /// A human diagnostic or informational log line.
    Log,
    /// Ephemeral progress output.
    Progress,
    /// A structured machine record.
    Machine,
}

/// Why explicit output facts could not form a profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputProfileError {
    /// The color request was not one of `auto`, `always`, or `never`.
    InvalidColorRequest(String),
    /// `TERM` was empty or contained a control character.
    InvalidTerm(String),
    /// A supplied width was zero.
    InvalidWidth(usize),
}

impl std::fmt::Display for OutputProfileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidColorRequest(value) => write!(
                formatter,
                "invalid color request {:?}; expected auto, always, or never",
                value
            ),
            Self::InvalidTerm(value) => {
                write!(formatter, "invalid TERM value {:?}", value)
            }
            Self::InvalidWidth(width) => {
                write!(formatter, "output width must be greater than zero: {width}")
            }
        }
    }
}

impl std::error::Error for OutputProfileError {}

const DEFAULT_WIDTH: usize = 80;

/// The one resolved output policy for a command invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputProfile {
    width: usize,
    ansi_enabled: bool,
    unicode_enabled: bool,
    progress_enabled: bool,
    human_enabled: bool,
    machine_enabled: bool,
    renderable: OutputChannel,
    status: OutputChannel,
    log: OutputChannel,
    progress: OutputChannel,
    machine: OutputChannel,
}

impl OutputProfile {
    /// Resolve all output capabilities and channels from explicit facts.
    pub fn select(facts: OutputFacts) -> Result<Self, OutputProfileError> {
        if let Some(term) = facts.term.as_deref() {
            if term.is_empty() || term.chars().any(char::is_control) {
                return Err(OutputProfileError::InvalidTerm(term.to_owned()));
            }
        }
        let width = match facts.width {
            Some(0) => return Err(OutputProfileError::InvalidWidth(0)),
            Some(width) => width,
            None => DEFAULT_WIDTH,
        };
        let term_is_dumb = facts.term.as_deref() == Some("dumb");
        let human_enabled = !facts.json;
        let machine_enabled = facts.json;
        // Explicit choice > NO_COLOR > FORCE_COLOR > TTY.  `TERM=dumb`
        // constrains decorations and progress, not the canonical color
        // precedence; an explicit force remains explicit.
        let ansi_enabled = human_enabled
            && match facts.requested_color {
                ColorRequest::Always => true,
                ColorRequest::Never => false,
                ColorRequest::Auto => {
                    if facts.no_color {
                        false
                    } else if facts.force_color {
                        true
                    } else {
                        facts.tty
                    }
                }
            };
        let unicode_enabled = human_enabled && facts.tty && facts.unicode && !term_is_dumb;
        let progress_enabled = human_enabled && facts.tty && !term_is_dumb;
        let (renderable, status, log, progress, machine) = if machine_enabled {
            (
                OutputChannel::Hidden,
                OutputChannel::Hidden,
                OutputChannel::Hidden,
                OutputChannel::Hidden,
                OutputChannel::MachineStdout,
            )
        } else {
            (
                OutputChannel::HumanStdout,
                OutputChannel::HumanStderr,
                OutputChannel::HumanStderr,
                OutputChannel::HumanStderr,
                OutputChannel::Hidden,
            )
        };
        Ok(Self {
            width,
            ansi_enabled,
            unicode_enabled,
            progress_enabled,
            human_enabled,
            machine_enabled,
            renderable,
            status,
            log,
            progress,
            machine,
        })
    }

    /// Return the resolved terminal column budget.
    pub const fn width(self) -> usize {
        self.width
    }

    /// Return whether human ANSI styling is allowed.
    pub const fn ansi_enabled(self) -> bool {
        self.ansi_enabled
    }

    /// Return whether Unicode decorations are allowed.
    pub const fn unicode_enabled(self) -> bool {
        self.unicode_enabled
    }

    /// Return whether ephemeral progress output is allowed.
    pub const fn progress_enabled(self) -> bool {
        self.progress_enabled
    }

    /// Return whether human output is selected.
    pub const fn human_enabled(self) -> bool {
        self.human_enabled
    }

    /// Return whether structured machine output is selected.
    pub const fn machine_enabled(self) -> bool {
        self.machine_enabled
    }

    /// Return the selected channel for one semantic output kind.
    pub const fn channel(self, kind: OutputKind) -> OutputChannel {
        match kind {
            OutputKind::Renderable => self.renderable,
            OutputKind::Status => self.status,
            OutputKind::Log => self.log,
            OutputKind::Progress => self.progress,
            OutputKind::Machine => self.machine,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorRequest, OutputChannel, OutputFacts, OutputKind, OutputProfile, OutputProfileError};

    fn facts() -> OutputFacts {
        OutputFacts::new(
            true,
            Some("xterm-256color"),
            false,
            false,
            ColorRequest::Auto,
            false,
            Some(100),
            true,
        )
    }

    #[test]
    fn human_terminal_selects_styled_unicode_progress_channels() {
        let profile = OutputProfile::select(facts()).unwrap();
        assert_eq!(profile.width(), 100);
        assert!(profile.ansi_enabled());
        assert!(profile.unicode_enabled());
        assert!(profile.progress_enabled());
        assert!(profile.human_enabled());
        assert!(!profile.machine_enabled());
        assert_eq!(profile.channel(OutputKind::Renderable), OutputChannel::HumanStdout);
        for kind in [OutputKind::Status, OutputKind::Log, OutputKind::Progress] {
            assert_eq!(profile.channel(kind), OutputChannel::HumanStderr);
        }
        assert_eq!(profile.channel(OutputKind::Machine), OutputChannel::Hidden);
    }

    #[test]
    fn redirected_auto_output_is_plain_and_non_progress() {
        let mut facts = facts();
        facts.tty = false;
        let profile = OutputProfile::select(facts).unwrap();
        assert!(!profile.ansi_enabled());
        assert!(!profile.unicode_enabled());
        assert!(!profile.progress_enabled());
        assert_eq!(profile.channel(OutputKind::Renderable), OutputChannel::HumanStdout);
    }
    #[test]
    fn explicit_choice_wins_over_dumb_terminal() {
        let mut facts = facts();
        facts.term = Some("dumb".to_owned());
        facts.requested_color = ColorRequest::Always;
        facts.unicode = true;
        let profile = OutputProfile::select(facts).unwrap();
        assert!(profile.ansi_enabled());
        assert!(!profile.unicode_enabled());
        assert!(!profile.progress_enabled());
    }

    #[test]
    fn auto_color_uses_no_color_then_force_color_then_tty() {
        let mut facts = facts();
        facts.tty = false;
        facts.force_color = true;
        assert!(OutputProfile::select(facts.clone()).unwrap().ansi_enabled());
        facts.no_color = true;
        assert!(!OutputProfile::select(facts.clone()).unwrap().ansi_enabled());
        facts.no_color = false;
        facts.force_color = false;
        assert!(!OutputProfile::select(facts.clone()).unwrap().ansi_enabled());
        facts.tty = true;
        assert!(OutputProfile::select(facts).unwrap().ansi_enabled());
    }

    #[test]
    fn json_is_machine_only_and_never_ansi_or_progress() {
        let mut facts = facts();
        facts.json = true;
        facts.requested_color = ColorRequest::Always;
        let profile = OutputProfile::select(facts).unwrap();
        assert!(!profile.ansi_enabled());
        assert!(!profile.progress_enabled());
        assert!(!profile.human_enabled());
        assert!(profile.machine_enabled());
        for kind in [
            OutputKind::Renderable,
            OutputKind::Status,
            OutputKind::Log,
            OutputKind::Progress,
        ] {
            assert_eq!(profile.channel(kind), OutputChannel::Hidden);
        }
        assert_eq!(profile.channel(OutputKind::Machine), OutputChannel::MachineStdout);
    }

    #[test]
    fn invalid_facts_are_typed_errors() {
        assert_eq!(ColorRequest::parse("sometimes"), Err(OutputProfileError::InvalidColorRequest("sometimes".to_owned())));
        let mut width = facts();
        width.width = Some(0);
        assert_eq!(OutputProfile::select(width), Err(OutputProfileError::InvalidWidth(0)));
        let mut term = facts();
        term.term = Some("x\nterm".to_owned());
        assert_eq!(
            OutputProfile::select(term),
            Err(OutputProfileError::InvalidTerm("x\nterm".to_owned()))
        );
    }

    #[test]
    fn selection_is_deterministic() {
        let first = OutputProfile::select(facts()).unwrap();
        let second = OutputProfile::select(facts()).unwrap();
        assert_eq!(first, second);
    }
}
