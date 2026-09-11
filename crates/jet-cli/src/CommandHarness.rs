//! In-process command execution and deterministic result capture.
//!
//! `JetCommand` does not spawn a child or own a terminal.  A caller supplies
//! the normal command dispatcher, which receives explicit arguments, explicit
//! environment overrides, and a capture sink.  This keeps command tests at the
//! process boundary while making stdout, stderr, diagnostics, and exit status
//! reviewable and serializable.

use jet_foundation::PerformanceBudget::CanonicalJson;
use std::collections::BTreeMap;
use std::fmt;

/// Maximum captured stdout or stderr bytes for one command.
pub const MAX_STREAM_BYTES: usize = 16 * 1024 * 1024;
/// Maximum structured diagnostics captured for one command.
pub const MAX_DIAGNOSTICS: usize = 100_000;

/// Severity carried by a structured command diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticSeverity {
    /// Informational report.
    Info,
    /// Warning or lint report.
    Warning,
    /// Command failure report.
    Error,
}

impl DiagnosticSeverity {
    /// Return the stable lowercase wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// One structured command diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandDiagnostic {
    /// Stable diagnostic code, such as `E2102`.
    pub code: String,
    /// Diagnostic severity.
    pub severity: DiagnosticSeverity,
    /// What happened.
    pub what: String,
    /// Why the command rejected or reported it.
    pub why: String,
    /// Concrete next step.
    pub fix: String,
}

impl CommandDiagnostic {
    /// Construct an error diagnostic.
    pub fn error(
        code: impl Into<String>,
        what: impl Into<String>,
        why: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: DiagnosticSeverity::Error,
            what: what.into(),
            why: why.into(),
            fix: fix.into(),
        }
    }

    /// Construct a diagnostic with an explicit severity.
    pub fn new(
        code: impl Into<String>,
        severity: DiagnosticSeverity,
        what: impl Into<String>,
        why: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            what: what.into(),
            why: why.into(),
            fix: fix.into(),
        }
    }
    /// Convert a foundation diagnostic without losing its stable report fields.
    pub fn from_foundation(diagnostic: &jet_foundation::Diagnostics::Diagnostic) -> Self {
        let severity = match diagnostic.severity {
            jet_foundation::Diagnostics::Severity::Error => DiagnosticSeverity::Error,
            jet_foundation::Diagnostics::Severity::Lint => DiagnosticSeverity::Warning,
        };
        Self::new(
            diagnostic.code.clone(),
            severity,
            diagnostic.what.clone(),
            diagnostic.why.clone(),
            diagnostic.fix.clone(),
        )
    }


    /// Encode this diagnostic as canonical JSON.
    pub fn to_json(&self) -> CanonicalJson {
        CanonicalJson::Object(BTreeMap::from([
            (
                "code".into(),
                CanonicalJson::String(self.code.clone()),
            ),
            (
                "fix".into(),
                CanonicalJson::String(self.fix.clone()),
            ),
            (
                "severity".into(),
                CanonicalJson::String(self.severity.as_str().into()),
            ),
            (
                "what".into(),
                CanonicalJson::String(self.what.clone()),
            ),
            (
                "why".into(),
                CanonicalJson::String(self.why.clone()),
            ),
        ]))
    }
}

/// Captured result of one in-process command invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandResult {
    /// Exact bytes written to stdout, represented as UTF-8 text.
    pub stdout: String,
    /// Exact bytes written to stderr, represented as UTF-8 text.
    pub stderr: String,
    /// Structured reports emitted by the command.
    pub diagnostics: Vec<CommandDiagnostic>,
    /// Process-style exit status returned by the dispatcher.
    pub exit_code: i32,
}

impl CommandResult {
    /// Return true when the command returned status zero and no error report.
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
            && !self
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
    }

    /// Return a copy with ANSI and carriage-return variance removed.
    pub fn normalized(&self) -> Self {
        Self {
            stdout: normalize_terminal_text(&self.stdout),
            stderr: normalize_terminal_text(&self.stderr),
            diagnostics: self
                .diagnostics
                .iter()
                .map(|diagnostic| CommandDiagnostic {
                    code: diagnostic.code.clone(),
                    severity: diagnostic.severity,
                    what: normalize_terminal_text(&diagnostic.what),
                    why: normalize_terminal_text(&diagnostic.why),
                    fix: normalize_terminal_text(&diagnostic.fix),
                })
                .collect(),
            exit_code: self.exit_code,
        }
    }

    /// Return canonical JSON for byte-stable recording and review.
    pub fn to_json(&self) -> CanonicalJson {
        CanonicalJson::Object(BTreeMap::from([
            (
                "diagnostics".into(),
                CanonicalJson::Array(
                    self.diagnostics
                        .iter()
                        .map(CommandDiagnostic::to_json)
                        .collect(),
                ),
            ),
            (
                "exit_code".into(),
                CanonicalJson::Integer(self.exit_code.to_string()),
            ),
            (
                "stderr".into(),
                CanonicalJson::String(self.stderr.clone()),
            ),
            (
                "stdout".into(),
                CanonicalJson::String(self.stdout.clone()),
            ),
        ]))
    }

    /// Return canonical JSON bytes for this command result.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        self.to_json().bytes()
    }

    /// Return a stable digest of the normalized result.
    pub fn digest(&self) -> String {
        self.normalized().to_json().sha256()
    }
}

/// Mutable output sink passed to an in-process command dispatcher.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandCapture {
    stdout: String,
    stderr: String,
    diagnostics: Vec<CommandDiagnostic>,
}

impl CommandCapture {
    /// Create an empty capture sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append text to stdout.
    pub fn stdout(&mut self, text: impl AsRef<str>) {
        self.stdout.push_str(text.as_ref());
    }

    /// Append text to stderr.
    pub fn stderr(&mut self, text: impl AsRef<str>) {
        self.stderr.push_str(text.as_ref());
    }

    /// Append one structured diagnostic.
    pub fn diagnostic(&mut self, diagnostic: CommandDiagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Return the currently captured stdout.
    pub fn stdout_text(&self) -> &str {
        &self.stdout
    }

    /// Return the currently captured stderr.
    pub fn stderr_text(&self) -> &str {
        &self.stderr
    }

    /// Return diagnostics in emission order.
    pub fn diagnostics(&self) -> &[CommandDiagnostic] {
        &self.diagnostics
    }

    fn finish(self, exit_code: i32) -> Result<CommandResult, HarnessError> {
        if self.stdout.len() > MAX_STREAM_BYTES {
            return Err(HarnessError::Limit {
                what: "stdout bytes",
                limit: MAX_STREAM_BYTES,
            });
        }
        if self.stderr.len() > MAX_STREAM_BYTES {
            return Err(HarnessError::Limit {
                what: "stderr bytes",
                limit: MAX_STREAM_BYTES,
            });
        }
        if self.diagnostics.len() > MAX_DIAGNOSTICS {
            return Err(HarnessError::Limit {
                what: "structured diagnostic count",
                limit: MAX_DIAGNOSTICS,
            });
        }
        Ok(CommandResult {
            stdout: self.stdout,
            stderr: self.stderr,
            diagnostics: self.diagnostics,
            exit_code,
        })
    }
}

/// An explicit in-process command invocation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JetCommand {
    args: Vec<String>,
    environment: BTreeMap<String, String>,
}

impl JetCommand {
    /// Construct a command from argv words.
    pub fn new<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            environment: BTreeMap::new(),
        }
    }

    /// Add one argv word.
    pub fn arg(mut self, argument: impl Into<String>) -> Self {
        self.args.push(argument.into());
        self
    }

    /// Add one explicit environment override.
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Result<Self, HarnessError> {
        let name = name.into();
        if !valid_env_name(&name) {
            return Err(HarnessError::InvalidEnvironmentName(name));
        }
        self.environment.insert(name, value.into());
        Ok(self)
    }

    /// Return argv words in order.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Return explicit environment overrides in lexical key order.
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    /// Execute through a closure without spawning a process.
    pub fn run<F>(&self, dispatch: F) -> Result<CommandResult, HarnessError>
    where
        F: FnOnce(&[String], &BTreeMap<String, String>, &mut CommandCapture) -> i32,
    {
        let mut capture = CommandCapture::new();
        let exit_code = dispatch(&self.args, &self.environment, &mut capture);
        capture.finish(exit_code)
    }

    /// Execute through a reusable in-process dispatcher.
    pub fn run_with<E: CommandExecutor>(&self, executor: &mut E) -> Result<CommandResult, HarnessError> {
        let mut capture = CommandCapture::new();
        let exit_code = executor.execute(self, &mut capture);
        capture.finish(exit_code)
    }
}

/// Reusable command dispatch seam for tests and the CLI host.
pub trait CommandExecutor {
    /// Run one command and return its process-style status.
    fn execute(&mut self, command: &JetCommand, capture: &mut CommandCapture) -> i32;
}

impl<F> CommandExecutor for F
where
    F: FnMut(&JetCommand, &mut CommandCapture) -> i32,
{
    fn execute(&mut self, command: &JetCommand, capture: &mut CommandCapture) -> i32 {
        self(command, capture)
    }
}
/// Adapt a raw-argv dispatcher to the reusable command harness.
///
/// The adapter keeps argument and environment ownership in [`JetCommand`]
/// while routing all output through the existing [`CommandCapture`] sink.
pub struct CommandExecutorAdapter<F> {
    dispatch: F,
}

impl<F> CommandExecutorAdapter<F> {
    /// Wrap one in-process dispatcher.
    pub fn new(dispatch: F) -> Self {
        Self { dispatch }
    }

    /// Recover the wrapped dispatcher.
    pub fn into_inner(self) -> F {
        self.dispatch
    }
}

impl<F> CommandExecutor for CommandExecutorAdapter<F>
where
    F: FnMut(&[String], &BTreeMap<String, String>, &mut CommandCapture) -> i32,
{
    fn execute(&mut self, command: &JetCommand, capture: &mut CommandCapture) -> i32 {
        (self.dispatch)(command.args(), command.environment(), capture)
    }
}


/// Stable harness failures that occur before a command result exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HarnessError {
    /// A captured stream or diagnostic collection exceeded its bound.
    Limit { what: &'static str, limit: usize },
    /// An environment key is not a portable explicit variable name.
    InvalidEnvironmentName(String),
}

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit { what, limit } => write!(formatter, "{what} exceeds the {limit} item limit"),
            Self::InvalidEnvironmentName(name) => {
                write!(formatter, "invalid command environment name `{name}`")
            }
        }
    }
}

impl std::error::Error for HarnessError {}

/// Remove terminal control sequences and normalize line endings.
pub fn normalize_terminal_text(value: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Normal,
        Escape,
        Csi,
        Osc,
        OscEscape,
    }

    let mut state = State::Normal;
    let mut previous_cr = false;
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match state {
            State::Normal => match character {
                '\u{1b}' => {
                    previous_cr = false;
                    state = State::Escape;
                }
                '\r' => {
                    out.push('\n');
                    previous_cr = true;
                }
                '\n' if previous_cr => previous_cr = false,
                '\u{7}' => previous_cr = false,
                _ => {
                    previous_cr = false;
                    out.push(character);
                }
            },
            State::Escape => match character {
                '[' => state = State::Csi,
                ']' => state = State::Osc,
                _ => state = State::Normal,
            },
            State::Csi => {
                if ('@'..='~').contains(&character) {
                    state = State::Normal;
                }
            }
            State::Osc => match character {
                '\u{7}' => state = State::Normal,
                '\u{1b}' => state = State::OscEscape,
                _ => {}
            },
            State::OscEscape => {
                state = if character == '\\' {
                    State::Normal
                } else {
                    State::Osc
                };
            }
        }
    }
    out
}

fn valid_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
