//! Deterministic, line-oriented interaction tapes.
//!
//! A tape is deliberately a small command language rather than a terminal
//! implementation.  The runner supplies the observed terminal/command seam,
//! so replay can use the same renderer and input path as an interactive run.

use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::SHA256;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::time::Duration;

/// The stable text-tape format identifier.
pub const FORMAT: &str = "jet.tape";
/// The current tape format version.
pub const VERSION: u16 = 1;
/// Maximum accepted tape source size.
pub const MAX_TAPE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum number of commands in one tape.
pub const MAX_TAPE_STEPS: usize = 100_000;
/// Maximum text payload carried by one tape command.
pub const MAX_STEP_TEXT_BYTES: usize = 1024 * 1024;
/// Maximum delay accepted by `Sleep`.
pub const MAX_SLEEP: Duration = Duration::from_secs(60 * 60);

/// One deterministic interaction command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TapeStep {
    /// Type text through the normal input path.
    Type(String),
    /// Submit the current input.
    Enter,
    /// Advance the runner's clock by a bounded duration.
    Sleep(Duration),
    /// Wait for an observed readiness marker.
    Wait(String),
    /// Require a named executable before replay starts.
    Require(String),
    /// Set one explicit environment value for the command under test.
    Set { name: String, value: String },
    /// Compare the next observed output after terminal normalization.
    Output(String),
}

/// A parsed and validated interaction tape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tape {
    steps: Vec<TapeStep>,
}

impl Tape {
    /// Build a tape from already parsed steps.
    pub fn new(steps: Vec<TapeStep>) -> Result<Self, TapeError> {
        if steps.len() > MAX_TAPE_STEPS {
            return Err(TapeError::Limit {
                what: "tape step count",
                limit: MAX_TAPE_STEPS,
            });
        }
        for (index, step) in steps.iter().enumerate() {
            validate_step(step).map_err(|message| TapeError::Step {
                step: index + 1,
                message,
            })?;
        }
        Ok(Self { steps })
    }

    /// Parse the stable line-oriented tape representation.
    pub fn parse(source: &str) -> Result<Self, TapeError> {
        if source.len() > MAX_TAPE_BYTES {
            return Err(TapeError::Limit {
                what: "tape source bytes",
                limit: MAX_TAPE_BYTES,
            });
        }
        if source.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]) {
            return Err(TapeError::Parse {
                line: 1,
                message: "tape must not contain a UTF-8 BOM".into(),
            });
        }
        if source.contains('\0') {
            return Err(TapeError::Parse {
                line: 1,
                message: "tape must not contain NUL bytes".into(),
            });
        }

        let mut steps = Vec::new();
        for (line_index, line) in source.lines().enumerate() {
            let line_number = line_index + 1;
            if let Some(step) = parse_line(line, line_number)? {
                steps.push(step);
                if steps.len() > MAX_TAPE_STEPS {
                    return Err(TapeError::Limit {
                        what: "tape step count",
                        limit: MAX_TAPE_STEPS,
                    });
                }
            }
        }
        Self::new(steps)
    }

    /// Return the commands in source order.
    pub fn steps(&self) -> &[TapeStep] {
        &self.steps
    }

    /// Return one command by zero-based index.
    pub fn step(&self, index: usize) -> Option<&TapeStep> {
        self.steps.get(index)
    }

    /// Render the canonical text representation.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for step in &self.steps {
            match step {
                TapeStep::Type(text) => {
                    out.push_str("Type ");
                    out.push_str(&quote(text));
                }
                TapeStep::Enter => out.push_str("Enter"),
                TapeStep::Sleep(duration) => {
                    out.push_str("Sleep ");
                    out.push_str(&duration.as_millis().to_string());
                    out.push_str("ms");
                }
                TapeStep::Wait(readiness) => {
                    out.push_str("Wait ");
                    out.push_str(&quote(readiness));
                }
                TapeStep::Require(program) => {
                    out.push_str("Require ");
                    out.push_str(&quote(program));
                }
                TapeStep::Set { name, value } => {
                    out.push_str("Set ");
                    out.push_str(name);
                    out.push(' ');
                    out.push_str(&quote(value));
                }
                TapeStep::Output(expected) => {
                    out.push_str("Output ");
                    out.push_str(&quote(expected));
                }
            }
            out.push('\n');
        }
        out
    }

    /// Return canonical UTF-8 bytes with exactly one LF per command.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.to_text().into_bytes()
    }

    /// Return the SHA-256 identity of the canonical tape text.
    pub fn digest(&self) -> String {
        SHA256::sha256_hex(&self.to_bytes())
    }

    /// Return unique required programs in lexical order.
    pub fn required_programs(&self) -> Vec<String> {
        self.steps
            .iter()
            .filter_map(|step| match step {
                TapeStep::Require(program) => Some(program.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Return required programs that cannot be resolved from the host PATH.
    pub fn missing_requirements(&self) -> Vec<String> {
        self.required_programs()
            .into_iter()
            .filter(|program| !program_available(program))
            .collect()
    }

    /// Fail before any interaction when a required executable is absent.
    pub fn preflight(&self) -> Result<(), TapeError> {
        if let Some(program) = self.missing_requirements().into_iter().next() {
            return Err(TapeError::MissingRequirement { program });
        }
        Ok(())
    }

    /// Parse and replay one canonical tape source through an existing seam.
    pub fn replay_source<E: TapeExecutor>(
        source: &str,
        executor: &mut E,
    ) -> Result<TapeRun, TapeError> {
        Self::parse(source)?.replay(executor)
    }

    /// Replay through the caller's existing input, wait, and output seams.
    pub fn replay<E: TapeExecutor>(&self, executor: &mut E) -> Result<TapeRun, TapeError> {
        self.preflight()?;
        let mut outputs = Vec::new();
        for (index, step) in self.steps.iter().enumerate() {
            let step_number = index + 1;
            if let TapeStep::Output(expected) = step {
                let actual = executor
                    .observe_output()
                    .map_err(|message| TapeError::Execution {
                        step: step_number,
                        action: "Output",
                        message,
                    })?;
                let expected = normalize_terminal_text(expected);
                let actual = normalize_terminal_text(&actual);
                if expected != actual {
                    return Err(TapeError::OutputMismatch {
                        step: step_number,
                        expected,
                        actual,
                    });
                }
                outputs.push(actual);
                continue;
            }
            let result = match step {
                TapeStep::Type(text) => executor.type_text(text),
                TapeStep::Enter => executor.enter(),
                TapeStep::Sleep(duration) => executor.sleep(*duration),
                TapeStep::Wait(readiness) => executor.wait(readiness),
                TapeStep::Require(program) => executor.require(program),
                TapeStep::Set { name, value } => executor.set(name, value),
                TapeStep::Output(_) => unreachable!("Output is handled above"),
            };
            result.map_err(|message| TapeError::Execution {
                step: step_number,
                action: step_name(step),
                message,
            })?;
        }
        Ok(TapeRun {
            steps_executed: self.steps.len(),
            outputs,
        })
    }
}

impl Default for Tape {
    fn default() -> Self {
        Self { steps: Vec::new() }
    }
}

/// Output and progress returned by one successful tape replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TapeRun {
    /// Number of commands consumed.
    pub steps_executed: usize,
    /// Normalized outputs checked by `Output` commands, in order.
    pub outputs: Vec<String>,
}

/// The terminal/command seam used by tape replay.
///
/// Implementations should route these calls through the production input and
/// renderer paths.  `sleep` is explicit so tests and headless runs can use a
/// virtual clock instead of making replay wall-clock dependent.
pub trait TapeExecutor {
    /// Type text through the normal input path.
    fn type_text(&mut self, text: &str) -> Result<(), String>;
    /// Submit the current input.
    fn enter(&mut self) -> Result<(), String>;
    /// Advance or observe the runner's clock.
    fn sleep(&mut self, duration: Duration) -> Result<(), String>;
    /// Wait for a readiness marker observed by the runner.
    fn wait(&mut self, readiness: &str) -> Result<(), String>;
    /// Resolve one required executable.
    fn require(&mut self, program: &str) -> Result<(), String>;
    /// Set an explicit command environment value.
    fn set(&mut self, name: &str, value: &str) -> Result<(), String>;
    /// Return the next normalized-or-raw terminal output observation.
    fn observe_output(&mut self) -> Result<String, String>;
}

/// Stable error returned while parsing or replaying a tape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TapeError {
    /// A source line has invalid command syntax.
    Parse { line: usize, message: String },
    /// A programmatic step violates a tape bound or invariant.
    Step { step: usize, message: String },
    /// A source or collection bound was exceeded.
    Limit { what: &'static str, limit: usize },
    /// A required executable is unavailable before replay starts.
    MissingRequirement { program: String },
    /// The executor rejected one command.
    Execution {
        step: usize,
        action: &'static str,
        message: String,
    },
    /// Observed output differs after normalization.
    OutputMismatch {
        step: usize,
        expected: String,
        actual: String,
    },
}

impl fmt::Display for TapeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse { line, message } => write!(formatter, "tape line {line}: {message}"),
            Self::Step { step, message } => write!(formatter, "tape step {step}: {message}"),
            Self::Limit { what, limit } => write!(formatter, "{what} exceeds the {limit} item limit"),
            Self::MissingRequirement { program } => {
                write!(formatter, "required program `{program}` is unavailable")
            }
            Self::Execution {
                step,
                action,
                message,
            } => write!(formatter, "tape step {step} ({action}) failed: {message}"),
            Self::OutputMismatch {
                step,
                expected,
                actual,
            } => write!(
                formatter,
                "tape step {step} output mismatch: expected {expected:?}, observed {actual:?}"
            ),
        }
    }
}

impl std::error::Error for TapeError {}

fn parse_line(line: &str, line_number: usize) -> Result<Option<TapeStep>, TapeError> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Ok(None);
    }
    let mut words = trimmed.splitn(2, |character: char| character.is_ascii_whitespace());
    let command = words.next().unwrap_or_default();
    let rest = words.next().unwrap_or_default().trim_start();
    let parse = |message: String| TapeError::Parse {
        line: line_number,
        message,
    };
    let step = match command {
        "Type" => TapeStep::Type(parse_value(rest).map_err(parse)?),
        "Enter" => {
            if !rest.trim().is_empty() {
                return Err(parse("Enter does not accept arguments".into()));
            }
            TapeStep::Enter
        }
        "Sleep" => TapeStep::Sleep(parse_duration(rest).map_err(parse)?),
        "Wait" => {
            let readiness = parse_value(rest).map_err(parse)?;
            if readiness.is_empty() {
                return Err(parse("Wait needs a readiness marker".into()));
            }
            TapeStep::Wait(readiness)
        }
        "Require" => {
            let program = parse_value(rest).map_err(parse)?;
            if program.is_empty() {
                return Err(parse("Require needs a program name".into()));
            }
            TapeStep::Require(program)
        }
        "Set" => parse_set(rest).map_err(parse)?,
        "Output" => TapeStep::Output(parse_value(rest).map_err(parse)?),
        _ => return Err(parse(format!("unknown tape command `{command}`"))),
    };
    validate_step(&step).map_err(parse)?;
    Ok(Some(step))
}

fn parse_set(rest: &str) -> Result<TapeStep, String> {
    let rest = rest.trim();
    let (name, value) = if let Some((name, value)) = rest.split_once('=') {
        (name.trim(), value.trim_start())
    } else {
        let mut fields = rest.splitn(2, |character: char| character.is_ascii_whitespace());
        let name = fields.next().unwrap_or_default();
        let value = fields.next().unwrap_or_default().trim_start();
        (name, value)
    };
    if !valid_env_name(name) {
        return Err(format!("invalid environment name `{name}`"));
    }
    Ok(TapeStep::Set {
        name: name.to_string(),
        value: parse_value(value)?,
    })
}

fn parse_value(value: &str) -> Result<String, String> {
    let value = value.trim_end();
    if value.starts_with('"') {
        let json = format!("{value}\n");
        match CanonicalJson::parse_canonical(json.as_bytes())? {
            CanonicalJson::String(value) => Ok(value),
            _ => Err("quoted tape value must be a JSON string".into()),
        }
    } else {
        Ok(value.to_string())
    }
}

fn parse_duration(value: &str) -> Result<Duration, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("Sleep needs a duration".into());
    }
    let (digits, multiplier) = if let Some(value) = value.strip_suffix("ms") {
        (value, 1u64)
    } else if let Some(value) = value.strip_suffix("us") {
        (value, 0u64)
    } else if let Some(value) = value.strip_suffix("ns") {
        (value, 0u64)
    } else if let Some(value) = value.strip_suffix('s') {
        (value, 1_000u64)
    } else {
        (value, 1u64)
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("invalid duration `{value}`"));
    }
    let number = digits
        .parse::<u64>()
        .map_err(|_| format!("duration `{value}` is too large"))?;
    let millis = if multiplier == 0 {
        if value.ends_with("us") {
            number / 1_000
        } else {
            number / 1_000_000
        }
    } else {
        number
            .checked_mul(multiplier)
            .ok_or_else(|| format!("duration `{value}` is too large"))?
    };
    let duration = Duration::from_millis(millis);
    if duration > MAX_SLEEP {
        return Err(format!("duration `{value}` exceeds the one-hour limit"));
    }
    Ok(duration)
}

fn validate_step(step: &TapeStep) -> Result<(), String> {
    match step {
        TapeStep::Type(text) | TapeStep::Output(text) => {
            if text.len() > MAX_STEP_TEXT_BYTES {
                return Err(format!("text payload exceeds the {MAX_STEP_TEXT_BYTES}-byte limit"));
            }
        }
        TapeStep::Wait(readiness) => {
            if readiness.is_empty() {
                return Err("Wait needs a readiness marker".into());
            }
            if readiness.len() > MAX_STEP_TEXT_BYTES {
                return Err(format!("readiness exceeds the {MAX_STEP_TEXT_BYTES}-byte limit"));
            }
        }
        TapeStep::Require(program) => {
            if program.is_empty() || program.contains('\0') {
                return Err("Require needs a non-empty program name".into());
            }
            if program.len() > 4096 {
                return Err("required program name exceeds the 4096-byte limit".into());
            }
        }
        TapeStep::Set { name, value } => {
            if !valid_env_name(name) {
                return Err(format!("invalid environment name `{name}`"));
            }
            if value.len() > MAX_STEP_TEXT_BYTES {
                return Err(format!("environment value exceeds the {MAX_STEP_TEXT_BYTES}-byte limit"));
            }
        }
        TapeStep::Enter => {}
        TapeStep::Sleep(duration) => {
            if *duration > MAX_SLEEP {
                return Err("Sleep duration exceeds the one-hour limit".into());
            }
        }
    }
    Ok(())
}

fn step_name(step: &TapeStep) -> &'static str {
    match step {
        TapeStep::Type(_) => "Type",
        TapeStep::Enter => "Enter",
        TapeStep::Sleep(_) => "Sleep",
        TapeStep::Wait(_) => "Wait",
        TapeStep::Require(_) => "Require",
        TapeStep::Set { .. } => "Set",
        TapeStep::Output(_) => "Output",
    }
}

fn quote(value: &str) -> String {
    let mut bytes = CanonicalJson::String(value.to_string()).bytes();
    let _ = bytes.pop();
    String::from_utf8(bytes).expect("CanonicalJson emits UTF-8")
}

fn valid_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn program_available(program: &str) -> bool {
    if program.is_empty() || program.contains('\0') {
        return false;
    }
    let path = Path::new(program);
    if path.is_absolute() || program.contains(std::path::MAIN_SEPARATOR) {
        return executable_file(path);
    }
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|directory| executable_file(&directory.join(program)))
}

#[cfg(unix)]
fn executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn executable_file(path: &Path) -> bool {
    path.is_file()
}

/// Normalize terminal output before comparing or storing it.
///
/// CSI, OSC, and other ANSI control sequences are removed.  CRLF and bare
/// carriage returns become LF, making captures independent of terminal mode.
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
                '\u{7}' => {
                    previous_cr = false;
                }
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
