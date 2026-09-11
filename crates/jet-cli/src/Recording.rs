//! Stable interaction recordings backed by the shared replay envelope.
//!
//! Recordings use the `JREPLAY\0` prefix, canonical JSON payloads, hashed
//! frames, and the `JEND` footer already used by proof replay.  The TUI schema
//! has its own frame kinds, so it cannot be mistaken for a proof artifact.
//! Text and frame observations are normalized before they are compared or
//! rendered for review; no wall-clock or ambient machine state is captured.

use crate::CommandHarness::{CommandDiagnostic, CommandResult, DiagnosticSeverity};
use crate::Tape::{normalize_terminal_text, Tape, TapeStep};
use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Shared replay envelope magic.
pub const MAGIC: &[u8; 8] = b"JREPLAY\0";
/// Shared replay envelope footer magic.
pub const FOOTER_MAGIC: &[u8; 4] = b"JEND";
/// TUI recording schema name.
pub const SCHEMA: &str = "jet.tui-replay";
/// TUI recording envelope major version.
pub const VERSION_MAJOR: u16 = 1;
/// TUI recording envelope minor version.
pub const VERSION_MINOR: u16 = 0;
/// Standard extension used by named replay artifacts.
pub const ARTIFACT_EXTENSION: &str = ".jetproof-replay";
/// Maximum complete recording size.
pub const MAX_RECORDING_BYTES: usize = 256 * 1024 * 1024;
/// Maximum number of recording frames.
pub const MAX_RECORDING_FRAMES: usize = 100_000;
/// Maximum payload carried by one recording frame.
pub const MAX_FRAME_PAYLOAD_BYTES: usize = 1024 * 1024;
/// Maximum canonical header size.
pub const MAX_HEADER_BYTES: usize = 4 * 1024 * 1024;

const FRAME_HEADER_BYTES: usize = 15;
const FRAME_HASH_BYTES: usize = 32;
const FOOTER_BYTES: usize = 52;
const ZERO_ARTIFACT_ID: &str = "000000000000000000000000";

const KIND_TYPE: u16 = 0x2001;
const KIND_ENTER: u16 = 0x2002;
const KIND_SLEEP: u16 = 0x2003;
const KIND_WAIT: u16 = 0x2004;
const KIND_REQUIRE: u16 = 0x2005;
const KIND_SET: u16 = 0x2006;
const KIND_OUTPUT: u16 = 0x2007;
const KIND_RESIZE: u16 = 0x2101;
const KIND_COMMAND: u16 = 0x2201;
const KIND_FRAME: u16 = 0x2102;

/// The explicit, non-machine identity attached to a recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingIdentity {
    /// Logical command name supplied by the caller, not ambient argv/cwd.
    pub command: String,
    /// Terminal width used by the headless renderer.
    pub width: u16,
    /// Terminal height used by the headless renderer.
    pub height: u16,
    /// Whether color was deliberately enabled for the session.
    pub color: bool,
}

impl RecordingIdentity {
    /// Construct and validate a recording identity.
    pub fn new(command: impl Into<String>, width: u16, height: u16, color: bool) -> Result<Self, RecordingError> {
        let identity = Self {
            command: command.into(),
            width,
            height,
            color,
        };
        validate_identity(&identity)?;
        Ok(identity)
    }
}

/// One interaction or observed terminal event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordingEvent {
    /// Text sent through the production input path.
    Type(String),
    /// Submit the current input.
    Enter,
    /// Virtual-clock delay in milliseconds.
    Sleep(u64),
    /// Observed readiness marker.
    Wait(String),
    /// Explicit executable dependency.
    Require(String),
    /// Explicit command environment value.
    Set { name: String, value: String },
    /// Expected or observed terminal output.
    Output(String),
    /// Terminal resize event.
    Resize { width: u16, height: u16 },
    /// Normalized headless frame text.
    Frame {
        width: u16,
        height: u16,
        text: String,
    },
    /// Captured in-process command result, including structured diagnostics.
    Command(CommandResult),
}

/// A validated sequence of deterministic recording events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recording {
    identity: RecordingIdentity,
    events: Vec<RecordingEvent>,
}

impl Recording {
    /// Start an empty recording with an explicit logical identity.
    pub fn new(identity: RecordingIdentity) -> Result<Self, RecordingError> {
        validate_identity(&identity)?;
        Ok(Self {
            identity,
            events: Vec::new(),
        })
    }

    /// Build a recording by translating a text tape into event frames.
    pub fn from_tape(identity: RecordingIdentity, tape: &Tape) -> Result<Self, RecordingError> {
        let mut recording = Self::new(identity)?;
        for step in tape.steps() {
            let event = match step {
                TapeStep::Type(text) => RecordingEvent::Type(text.clone()),
                TapeStep::Enter => RecordingEvent::Enter,
                TapeStep::Sleep(duration) => RecordingEvent::Sleep(duration.as_millis() as u64),
                TapeStep::Wait(readiness) => RecordingEvent::Wait(readiness.clone()),
                TapeStep::Require(program) => RecordingEvent::Require(program.clone()),
                TapeStep::Set { name, value } => RecordingEvent::Set {
                    name: name.clone(),
                    value: value.clone(),
                },
                TapeStep::Output(expected) => RecordingEvent::Output(expected.clone()),
            };
            recording.push(event)?;
        }
        Ok(recording)
    }

    /// Convert the interaction events in this recording into the canonical
    /// replay tape. Frame snapshots remain observations owned by the
    /// headless renderer; command results and resize events belong to a
    /// different seam and therefore fail explicitly instead of being
    /// silently dropped.
    pub fn interaction_tape(&self) -> Result<Tape, RecordingError> {
        let mut steps = Vec::new();
        for event in &self.events {
            let step = match event {
                RecordingEvent::Type(text) => TapeStep::Type(text.clone()),
                RecordingEvent::Enter => TapeStep::Enter,
                RecordingEvent::Sleep(milliseconds) => {
                    TapeStep::Sleep(Duration::from_millis(*milliseconds))
                }
                RecordingEvent::Wait(readiness) => TapeStep::Wait(readiness.clone()),
                RecordingEvent::Require(program) => TapeStep::Require(program.clone()),
                RecordingEvent::Set { name, value } => TapeStep::Set {
                    name: name.clone(),
                    value: value.clone(),
                },
                RecordingEvent::Output(expected) => TapeStep::Output(expected.clone()),
                RecordingEvent::Frame { .. } => continue,
                RecordingEvent::Resize { .. } => {
                    return Err(RecordingError::Invalid(
                        "recording resize events cannot be replayed by this headless target"
                            .into(),
                    ));
                }
                RecordingEvent::Command(_) => {
                    return Err(RecordingError::Invalid(
                        "recording command events cannot be replayed by this headless target"
                            .into(),
                    ));
                }
            };
            steps.push(step);
        }
        Tape::new(steps).map_err(|error| {
            RecordingError::Invalid(format!("recording interaction tape is invalid: {error}"))
        })
    }

    /// Append one captured in-process command result.
    pub fn push_command(&mut self, result: &CommandResult) -> Result<usize, RecordingError> {
        self.push(RecordingEvent::Command(result.clone()))
    }
    /// Append one normalized frame captured by a headless terminal driver.
    pub fn push_frame(&mut self, frame: &crate::Headless::Frame) -> Result<usize, RecordingError> {
        let width = u16::try_from(frame.size.width).map_err(|_| {
            RecordingError::Event("headless frame width exceeds recording limits".into())
        })?;
        let height = u16::try_from(frame.size.height).map_err(|_| {
            RecordingError::Event("headless frame height exceeds recording limits".into())
        })?;
        self.push(RecordingEvent::Frame {
            width,
            height,
            text: normalize_terminal_text(&frame.text),
        })
    }

    /// Return the explicit recording identity.
    pub fn identity(&self) -> &RecordingIdentity {
        &self.identity
    }

    /// Return events in sequence order.
    pub fn events(&self) -> &[RecordingEvent] {
        &self.events
    }

    /// Append one event and return its zero-based sequence number.
    pub fn push(&mut self, event: RecordingEvent) -> Result<usize, RecordingError> {
        if self.events.len() >= MAX_RECORDING_FRAMES {
            return Err(RecordingError::Limit {
                what: "recording frame count",
                limit: MAX_RECORDING_FRAMES,
            });
        }
        validate_event(&event)?;
        let index = self.events.len();
        self.events.push(event);
        Ok(index)
    }

    /// Return the canonical normalized text artifact used for CI review.
    pub fn normalized_text(&self) -> String {
        let mut out = String::new();
        for event in &self.events {
            match event {
                RecordingEvent::Type(text) => {
                    out.push_str("Type ");
                    out.push_str(&quote(text));
                }
                RecordingEvent::Enter => out.push_str("Enter"),
                RecordingEvent::Sleep(milliseconds) => {
                    out.push_str("Sleep ");
                    out.push_str(&milliseconds.to_string());
                    out.push_str("ms");
                }
                RecordingEvent::Wait(readiness) => {
                    out.push_str("Wait ");
                    out.push_str(&quote(&normalize_terminal_text(readiness)));
                }
                RecordingEvent::Require(program) => {
                    out.push_str("Require ");
                    out.push_str(&quote(program));
                }
                RecordingEvent::Set { name, value } => {
                    out.push_str("Set ");
                    out.push_str(name);
                    out.push(' ');
                    out.push_str(&quote(value));
                }
                RecordingEvent::Output(text) => {
                    out.push_str("Output ");
                    out.push_str(&quote(&normalize_terminal_text(text)));
                }
                RecordingEvent::Resize { width, height } => {
                    out.push_str("Resize ");
                    out.push_str(&width.to_string());
                    out.push('x');
                    out.push_str(&height.to_string());
                }
                RecordingEvent::Frame {
                    width,
                    height,
                    text,
                } => {
                    out.push_str("Frame ");
                    out.push_str(&width.to_string());
                    out.push('x');
                    out.push_str(&height.to_string());
                    out.push('\n');
                    out.push_str(&normalize_terminal_text(text));
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                RecordingEvent::Command(result) => {
                    out.push_str("Command ");
                    out.push_str(
                        String::from_utf8(result.normalized().canonical_bytes())
                            .expect("canonical command result JSON is UTF-8")
                            .trim_end_matches('\n'),
                    );
                }
            }
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }

    /// Return canonical normalized text bytes.
    pub fn normalized_bytes(&self) -> Vec<u8> {
        self.normalized_text().into_bytes()
    }

    /// Return the SHA-256 identity of the canonical normalized text.
    pub fn digest(&self) -> String {
        SHA256::sha256_hex(&self.normalized_bytes())
    }

    /// Encode the recording using the shared replay envelope.
    pub fn encode(&self) -> Result<Vec<u8>, RecordingError> {
        validate_identity(&self.identity)?;
        if self.events.len() > MAX_RECORDING_FRAMES {
            return Err(RecordingError::Limit {
                what: "recording frame count",
                limit: MAX_RECORDING_FRAMES,
            });
        }
        let mut frames = Vec::with_capacity(self.events.len());
        let mut payload_bytes = 0u64;
        for (sequence, event) in self.events.iter().enumerate() {
            let payload = event_payload(event)?;
            if payload.len() > MAX_FRAME_PAYLOAD_BYTES {
                return Err(RecordingError::Limit {
                    what: "recording frame payload bytes",
                    limit: MAX_FRAME_PAYLOAD_BYTES,
                });
            }
            payload_bytes = payload_bytes
                .checked_add(payload.len() as u64)
                .ok_or_else(|| RecordingError::Invalid("recording payload length overflow".into()))?;
            if payload_bytes > (MAX_RECORDING_BYTES - FOOTER_BYTES) as u64 {
                return Err(RecordingError::Limit {
                    what: "recording payload bytes",
                    limit: MAX_RECORDING_BYTES - FOOTER_BYTES,
                });
            }
            frames.push(encode_frame(
                1,
                event_kind(event),
                sequence as u64,
                &payload,
            ));
        }

        let zero_header = header_json(&self.identity, ZERO_ARTIFACT_ID)?;
        if zero_header.len() > MAX_HEADER_BYTES {
            return Err(RecordingError::Limit {
                what: "recording header bytes",
                limit: MAX_HEADER_BYTES,
            });
        }
        let mut body_zero = Vec::new();
        write_prefix(&mut body_zero, &zero_header);
        for frame in &frames {
            body_zero.extend_from_slice(frame);
        }
        let artifact_id = content_id(&body_zero);
        let header = header_json(&self.identity, &artifact_id)?;
        let mut out = Vec::with_capacity(body_zero.len() + FOOTER_BYTES);
        write_prefix(&mut out, &header);
        for frame in &frames {
            out.extend_from_slice(frame);
        }
        write_footer(&mut out, frames.len() as u64, payload_bytes);
        if out.len() > MAX_RECORDING_BYTES {
            return Err(RecordingError::Limit {
                what: "recording bytes",
                limit: MAX_RECORDING_BYTES,
            });
        }
        Ok(out)
    }

    /// Decode and fully verify a recording envelope.
    pub fn decode(bytes: &[u8]) -> Result<Self, RecordingError> {
        if bytes.len() > MAX_RECORDING_BYTES {
            return Err(RecordingError::Limit {
                what: "recording bytes",
                limit: MAX_RECORDING_BYTES,
            });
        }
        if bytes.len() < 16 + FOOTER_BYTES || &bytes[..8] != MAGIC {
            return Err(RecordingError::Invalid("missing JREPLAY magic".into()));
        }
        let major = u16::from_le_bytes([bytes[8], bytes[9]]);
        let minor = u16::from_le_bytes([bytes[10], bytes[11]]);
        if major != VERSION_MAJOR || minor != VERSION_MINOR {
            return Err(RecordingError::Invalid(format!(
                "unsupported recording schema {major}.{minor}"
            )));
        }
        let header_length = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        if header_length > MAX_HEADER_BYTES {
            return Err(RecordingError::Limit {
                what: "recording header bytes",
                limit: MAX_HEADER_BYTES,
            });
        }
        let header_end = 16usize
            .checked_add(header_length)
            .ok_or_else(|| RecordingError::Invalid("recording header length overflow".into()))?;
        let footer_start = bytes
            .len()
            .checked_sub(FOOTER_BYTES)
            .ok_or_else(|| RecordingError::Invalid("recording footer is truncated".into()))?;
        if header_end > footer_start {
            return Err(RecordingError::Invalid("recording header is truncated".into()));
        }
        let header = CanonicalJson::parse_canonical(&bytes[16..header_end])
            .map_err(RecordingError::Invalid)?;
        let (identity, artifact_id) = parse_header(&header)?;
        if &bytes[footer_start..footer_start + 4] != FOOTER_MAGIC {
            return Err(RecordingError::Invalid("missing JEND footer".into()));
        }
        let footer_end = footer_start
            .checked_add(FOOTER_BYTES)
            .ok_or_else(|| RecordingError::Invalid("recording footer length overflow".into()))?;
        if footer_end != bytes.len() {
            return Err(RecordingError::Invalid("trailing bytes after recording footer".into()));
        }
        let expected_footer_hash = SHA256::sha256(&bytes[..footer_start]);
        if bytes[footer_start + 20..footer_end] != expected_footer_hash {
            return Err(RecordingError::Invalid("recording footer hash mismatch".into()));
        }

        let mut offset = header_end;
        let mut sequence = 0u64;
        let mut frame_count = 0u64;
        let mut payload_bytes = 0u64;
        let mut events = Vec::new();
        while offset < footer_start {
            if frame_count >= MAX_RECORDING_FRAMES as u64 {
                return Err(RecordingError::Limit {
                    what: "recording frame count",
                    limit: MAX_RECORDING_FRAMES,
                });
            }
            let frame_header_end = offset
                .checked_add(FRAME_HEADER_BYTES)
                .ok_or_else(|| RecordingError::Invalid("recording frame length overflow".into()))?;
            if frame_header_end > footer_start {
                return Err(RecordingError::Invalid("truncated recording frame header".into()));
            }
            let flags = bytes[offset];
            let kind = u16::from_le_bytes([bytes[offset + 1], bytes[offset + 2]]);
            let frame_sequence = u64::from_le_bytes(
                bytes[offset + 3..offset + 11]
                    .try_into()
                    .map_err(|_| RecordingError::Invalid("invalid recording sequence".into()))?,
            );
            let payload_length = u32::from_le_bytes([
                bytes[offset + 11],
                bytes[offset + 12],
                bytes[offset + 13],
                bytes[offset + 14],
            ]) as usize;
            if payload_length > MAX_FRAME_PAYLOAD_BYTES {
                return Err(RecordingError::Limit {
                    what: "recording frame payload bytes",
                    limit: MAX_FRAME_PAYLOAD_BYTES,
                });
            }
            let payload_end = frame_header_end
                .checked_add(payload_length)
                .ok_or_else(|| RecordingError::Invalid("recording payload length overflow".into()))?;
            let frame_end = payload_end
                .checked_add(FRAME_HASH_BYTES)
                .ok_or_else(|| RecordingError::Invalid("recording frame length overflow".into()))?;
            if frame_end > footer_start {
                return Err(RecordingError::Invalid("truncated recording frame".into()));
            }
            if flags != 1 || frame_sequence != sequence {
                return Err(RecordingError::Invalid(
                    "recording frame flags or ordering are invalid".into(),
                ));
            }
            let expected_hash = SHA256::sha256(&bytes[offset..payload_end]);
            if bytes[payload_end..frame_end] != expected_hash {
                return Err(RecordingError::Invalid("recording frame hash mismatch".into()));
            }
            let payload = CanonicalJson::parse_canonical(&bytes[frame_header_end..payload_end])
                .map_err(RecordingError::Invalid)?;
            let event = decode_event(kind, &payload)?;
            events.push(event);
            frame_count += 1;
            sequence = sequence
                .checked_add(1)
                .ok_or_else(|| RecordingError::Invalid("recording sequence overflow".into()))?;
            payload_bytes = payload_bytes
                .checked_add(payload_length as u64)
                .ok_or_else(|| RecordingError::Invalid("recording payload length overflow".into()))?;
            if payload_bytes > (MAX_RECORDING_BYTES - FOOTER_BYTES) as u64 {
                return Err(RecordingError::Limit {
                    what: "recording payload bytes",
                    limit: MAX_RECORDING_BYTES - FOOTER_BYTES,
                });
            }
            offset = frame_end;
        }
        if offset != footer_start {
            return Err(RecordingError::Invalid("recording frame boundary is invalid".into()));
        }
        let footer_frames = u64::from_le_bytes(
            bytes[footer_start + 4..footer_start + 12]
                .try_into()
                .map_err(|_| RecordingError::Invalid("invalid recording frame count".into()))?,
        );
        let footer_payload = u64::from_le_bytes(
            bytes[footer_start + 12..footer_start + 20]
                .try_into()
                .map_err(|_| RecordingError::Invalid("invalid recording payload count".into()))?,
        );
        if footer_frames != frame_count || footer_payload != payload_bytes {
            return Err(RecordingError::Invalid(
                "recording footer counts do not match frames".into(),
            ));
        }
        let zero_header = header_json(&identity, ZERO_ARTIFACT_ID)?;
        let mut body_zero = Vec::with_capacity(footer_start);
        write_prefix(&mut body_zero, &zero_header);
        body_zero.extend_from_slice(&bytes[header_end..footer_start]);
        if content_id(&body_zero) != artifact_id {
            return Err(RecordingError::Invalid(
                "recording artifact_id does not match content".into(),
            ));
        }
        Ok(Self { identity, events })
    }
    /// Return the 24-byte content identity embedded in the binary envelope.
    pub fn artifact_id(&self) -> Result<String, RecordingError> {
        let bytes = self.encode()?;
        let header_length =
            u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        let header = CanonicalJson::parse_canonical(&bytes[16..16 + header_length])
            .map_err(RecordingError::Invalid)?;
        parse_header(&header).map(|(_, artifact_id)| artifact_id)
    }


    /// Verify that a replay target has the same explicit command and terminal identity.
    pub fn verify_identity(&self, expected: &RecordingIdentity) -> Result<(), RecordingError> {
        validate_identity(expected)?;
        if &self.identity != expected {
            return Err(RecordingError::IdentityMismatch);
        }
        Ok(())
    }
}

/// A named artifact store rooted at `.jet/replays` by default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingStore {
    root: PathBuf,
}

impl RecordingStore {
    /// Return the standard project-local recording store.
    pub fn default_store() -> Self {
        Self::new(PathBuf::from(".jet/replays"))
    }

    /// Use an explicit artifact root.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Return the store root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a closed `NAME` to its artifact path.
    pub fn path_for_name(&self, name: &str) -> Result<PathBuf, RecordingError> {
        validate_name(name)?;
        Ok(self
            .root
            .join(format!("{name}{ARTIFACT_EXTENSION}")))
    }

    /// Atomically publish a recording without replacing a different artifact.
    pub fn write(&self, name: &str, recording: &Recording) -> Result<PathBuf, RecordingError> {
        let destination = self.path_for_name(name)?;
        let bytes = recording.encode()?;
        fs::create_dir_all(&self.root).map_err(io_error)?;
        let temporary = self
            .root
            .join(format!(".{name}.{}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&temporary).map_err(io_error)?;
        file.write_all(&bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        let result = match fs::hard_link(&temporary, &destination) {
            Ok(()) => Ok(destination.clone()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = fs::read(&destination).map_err(io_error)?;
                if existing == bytes {
                    Ok(destination.clone())
                } else {
                    Err(RecordingError::Conflict(destination.clone()))
                }
            }
            Err(error) => Err(io_error(error)),
        };
        let _ = fs::remove_file(&temporary);
        result
    }

    /// Read and verify a named recording artifact.
    pub fn read(&self, name: &str) -> Result<Recording, RecordingError> {
        let path = self.path_for_name(name)?;
        let bytes = fs::read(&path).map_err(io_error)?;
        Recording::decode(&bytes)
    }
}
impl Default for RecordingStore {
    fn default() -> Self {
        Self::default_store()
    }
}


/// Stable errors from recording construction, encoding, decoding, and storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordingError {
    /// Malformed or inconsistent envelope data.
    Invalid(String),
    /// A bounded collection exceeded its limit.
    Limit { what: &'static str, limit: usize },
    /// Invalid named artifact or identity field.
    Name(String),
    /// Invalid event data.
    Event(String),
    /// Recording identity differs from the replay target.
    IdentityMismatch,
    /// A different artifact already owns the requested name.
    Conflict(PathBuf),
    /// Filesystem failure.
    Io(String),
}

impl fmt::Display for RecordingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => formatter.write_str(message),
            Self::Limit { what, limit } => write!(formatter, "{what} exceeds the {limit} item limit"),
            Self::Name(message) => formatter.write_str(message),
            Self::Event(message) => formatter.write_str(message),
            Self::IdentityMismatch => formatter.write_str("recording identity does not match replay target"),
            Self::Conflict(path) => write!(formatter, "recording already exists with different content: {}", path.display()),
            Self::Io(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RecordingError {}

fn validate_identity(identity: &RecordingIdentity) -> Result<(), RecordingError> {
    if identity.command.is_empty() || identity.command.len() > 4096 {
        return Err(RecordingError::Name(
            "recording command identity must contain 1..=4096 bytes".into(),
        ));
    }
    if identity.command.contains('\0') || identity.command.contains('\n') || identity.command.contains('\r') {
        return Err(RecordingError::Name(
            "recording command identity must not contain control line breaks".into(),
        ));
    }
    if identity.command.contains('\u{1b}') {
        return Err(RecordingError::Name(
            "recording command identity must not contain ANSI escapes".into(),
        ));
    }
    if identity.width == 0 || identity.height == 0 {
        return Err(RecordingError::Name(
            "recording terminal dimensions must be non-zero".into(),
        ));
    }
    Ok(())
}

fn validate_event(event: &RecordingEvent) -> Result<(), RecordingError> {
    match event {
        RecordingEvent::Type(text) | RecordingEvent::Output(text) => validate_text(text)?,
        RecordingEvent::Sleep(milliseconds) => {
            if *milliseconds > Duration::from_secs(60 * 60).as_millis() as u64 {
                return Err(RecordingError::Event("recording Sleep exceeds one hour".into()));
            }
        }
        RecordingEvent::Wait(readiness) => validate_text(readiness)?,
        RecordingEvent::Require(program) => {
            if program.is_empty() || program.contains('\0') || program.len() > 4096 {
                return Err(RecordingError::Event("invalid recording requirement".into()));
            }
        }
        RecordingEvent::Set { name, value } => {
            if !valid_env_name(name) {
                return Err(RecordingError::Event(format!("invalid environment name `{name}`")));
            }
            if sensitive_name(name) {
                return Err(RecordingError::Event(format!(
                    "refusing to record sensitive environment name `{name}`"
                )));
            }
            validate_text(value)?;
        }
        RecordingEvent::Resize { width, height } => {
            if *width == 0 || *height == 0 {
                return Err(RecordingError::Event(
                    "recording dimensions must be non-zero".into(),
                ));
            }
        }
        RecordingEvent::Frame { width, height, text } => {
            if *width == 0 || *height == 0 {
                return Err(RecordingError::Event(
                    "recording dimensions must be non-zero".into(),
                ));
            }
            validate_text(text)?;
        }
        RecordingEvent::Command(result) => {
            if result.stdout.len() > MAX_FRAME_PAYLOAD_BYTES {
                return Err(RecordingError::Limit {
                    what: "recording command stdout bytes",
                    limit: MAX_FRAME_PAYLOAD_BYTES,
                });
            }
            if result.stderr.len() > MAX_FRAME_PAYLOAD_BYTES {
                return Err(RecordingError::Limit {
                    what: "recording command stderr bytes",
                    limit: MAX_FRAME_PAYLOAD_BYTES,
                });
            }
            if result.diagnostics.len() > 100_000 {
                return Err(RecordingError::Limit {
                    what: "recording command diagnostic count",
                    limit: 100_000,
                });
            }
        }
        RecordingEvent::Enter => {}
    }
    Ok(())
}

fn validate_text(text: &str) -> Result<(), RecordingError> {
    if text.len() > MAX_FRAME_PAYLOAD_BYTES {
        return Err(RecordingError::Limit {
            what: "recording text bytes",
            limit: MAX_FRAME_PAYLOAD_BYTES,
        });
    }
    if text.contains('\0') {
        return Err(RecordingError::Event("recording text must not contain NUL".into()));
    }
    Ok(())
}

fn event_kind(event: &RecordingEvent) -> u16 {
    match event {
        RecordingEvent::Type(_) => KIND_TYPE,
        RecordingEvent::Enter => KIND_ENTER,
        RecordingEvent::Sleep(_) => KIND_SLEEP,
        RecordingEvent::Wait(_) => KIND_WAIT,
        RecordingEvent::Require(_) => KIND_REQUIRE,
        RecordingEvent::Set { .. } => KIND_SET,
        RecordingEvent::Output(_) => KIND_OUTPUT,
        RecordingEvent::Resize { .. } => KIND_RESIZE,
        RecordingEvent::Frame { .. } => KIND_FRAME,
        RecordingEvent::Command(_) => KIND_COMMAND,
    }
}

fn event_payload(event: &RecordingEvent) -> Result<Vec<u8>, RecordingError> {
    let value = match event {
        RecordingEvent::Type(text) => object([("text", CanonicalJson::String(text.clone()))])?,
        RecordingEvent::Enter => object([])?,
        RecordingEvent::Sleep(milliseconds) => object([(
            "milliseconds",
            CanonicalJson::Integer(milliseconds.to_string()),
        )])?,
        RecordingEvent::Wait(readiness) => object([(
            "readiness",
            CanonicalJson::String(readiness.clone()),
        )])?,
        RecordingEvent::Require(program) => object([(
            "program",
            CanonicalJson::String(program.clone()),
        )])?,
        RecordingEvent::Set { name, value } => object([
            ("name", CanonicalJson::String(name.clone())),
            ("value", CanonicalJson::String(value.clone())),
        ])?,
        RecordingEvent::Output(text) => object([("text", CanonicalJson::String(text.clone()))])?,
        RecordingEvent::Resize { width, height } => object([
            ("height", CanonicalJson::Integer(height.to_string())),
            ("width", CanonicalJson::Integer(width.to_string())),
        ])?,
        RecordingEvent::Frame {
            width,
            height,
            text,
        } => object([
            ("height", CanonicalJson::Integer(height.to_string())),
            ("text", CanonicalJson::String(text.clone())),
            ("width", CanonicalJson::Integer(width.to_string())),
        ])?,
        RecordingEvent::Command(result) => result.normalized().to_json(),
    };
    Ok(value.bytes())
}

fn decode_event(kind: u16, value: &CanonicalJson) -> Result<RecordingEvent, RecordingError> {
    let fields = object_fields(value)?;
    let event = match kind {
        KIND_TYPE => {
            expect_keys(fields, &["text"])?;
            RecordingEvent::Type(string_field(fields, "text")?)
        }
        KIND_ENTER => {
            expect_keys(fields, &[])?;
            RecordingEvent::Enter
        }
        KIND_SLEEP => {
            expect_keys(fields, &["milliseconds"])?;
            RecordingEvent::Sleep(integer_field(fields, "milliseconds")?)
        }
        KIND_WAIT => {
            expect_keys(fields, &["readiness"])?;
            RecordingEvent::Wait(string_field(fields, "readiness")?)
        }
        KIND_REQUIRE => {
            expect_keys(fields, &["program"])?;
            RecordingEvent::Require(string_field(fields, "program")?)
        }
        KIND_SET => {
            expect_keys(fields, &["name", "value"])?;
            RecordingEvent::Set {
                name: string_field(fields, "name")?,
                value: string_field(fields, "value")?,
            }
        }
        KIND_OUTPUT => {
            expect_keys(fields, &["text"])?;
            RecordingEvent::Output(string_field(fields, "text")?)
        }
        KIND_RESIZE => {
            expect_keys(fields, &["height", "width"])?;
            RecordingEvent::Resize {
                width: u16_field(fields, "width")?,
                height: u16_field(fields, "height")?,
            }
        }
        KIND_FRAME => {
            expect_keys(fields, &["height", "text", "width"])?;
            RecordingEvent::Frame {
                width: u16_field(fields, "width")?,
                height: u16_field(fields, "height")?,
                text: string_field(fields, "text")?,
            }
        }
        KIND_COMMAND => RecordingEvent::Command(decode_command_result(value)?),
        _ => return Err(RecordingError::Invalid(format!("unknown recording frame kind {kind:#06x}"))),
    };
    validate_event(&event)?;
    Ok(event)
}
fn decode_command_result(value: &CanonicalJson) -> Result<CommandResult, RecordingError> {
    let fields = object_fields(value)?;
    expect_keys(fields, &["diagnostics", "exit_code", "stderr", "stdout"])?;
    let diagnostics = match object_field(fields, "diagnostics")? {
        CanonicalJson::Array(values) => values
            .iter()
            .map(|value| {
                let fields = object_fields(value)?;
                expect_keys(fields, &["code", "fix", "severity", "what", "why"])?;
                let severity = match string_field(fields, "severity")?.as_str() {
                    "info" => DiagnosticSeverity::Info,
                    "warning" => DiagnosticSeverity::Warning,
                    "error" => DiagnosticSeverity::Error,
                    other => {
                        return Err(RecordingError::Invalid(format!(
                            "unknown command diagnostic severity `{other}`"
                        )));
                    }
                };
                Ok(CommandDiagnostic::new(
                    string_field(fields, "code")?,
                    severity,
                    string_field(fields, "what")?,
                    string_field(fields, "why")?,
                    string_field(fields, "fix")?,
                ))
            })
            .collect::<Result<Vec<_>, RecordingError>>()?,
        _ => {
            return Err(RecordingError::Invalid(
                "command diagnostics must be an array".into(),
            ));
        }
    };
    Ok(CommandResult {
        stdout: string_field(fields, "stdout")?,
        stderr: string_field(fields, "stderr")?,
        diagnostics,
        exit_code: signed_integer_field(fields, "exit_code")?,
    })
}


fn header_json(identity: &RecordingIdentity, artifact_id: &str) -> Result<Vec<u8>, RecordingError> {
    let value = object([
        (
            "artifact_id",
            CanonicalJson::String(artifact_id.to_string()),
        ),
        (
            "capture",
            object_value([
                ("mode", CanonicalJson::String("tui".into())),
                (
                    "roots",
                    CanonicalJson::Array(vec![
                        CanonicalJson::String("Input".into()),
                        CanonicalJson::String("Output".into()),
                        CanonicalJson::String("Frame".into()),
                    ]),
                ),
            ])?,
        ),
        (
            "identity",
            object_value([
                ("command", CanonicalJson::String(identity.command.clone())),
                (
                    "terminal",
                    object_value([
                        (
                            "color",
                            CanonicalJson::Bool(identity.color),
                        ),
                        (
                            "height",
                            CanonicalJson::Integer(identity.height.to_string()),
                        ),
                        (
                            "width",
                            CanonicalJson::Integer(identity.width.to_string()),
                        ),
                    ])?,
                ),
            ])?,
        ),
        (
            "limits",
            object_value([
                (
                    "frames",
                    CanonicalJson::Integer(MAX_RECORDING_FRAMES.to_string()),
                ),
                (
                    "payload_bytes",
                    CanonicalJson::Integer((MAX_RECORDING_BYTES - FOOTER_BYTES).to_string()),
                ),
            ])?,
        ),
        ("producer", CanonicalJson::String("jet-cli".into())),
        ("schema", CanonicalJson::String(SCHEMA.into())),
        (
            "version",
            object_value([
                ("major", CanonicalJson::Integer(VERSION_MAJOR.to_string())),
                ("minor", CanonicalJson::Integer(VERSION_MINOR.to_string())),
            ])?,
        ),
    ])?;
    Ok(value.bytes())
}

fn parse_header(value: &CanonicalJson) -> Result<(RecordingIdentity, String), RecordingError> {
    let fields = object_fields(value)?;
    expect_keys(
        fields,
        &[
            "artifact_id",
            "capture",
            "identity",
            "limits",
            "producer",
            "schema",
            "version",
        ],
    )?;
    let artifact_id = string_field(fields, "artifact_id")?;
    if artifact_id.len() != 24
        || !artifact_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        || artifact_id.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        return Err(RecordingError::Invalid(
            "recording artifact_id must be 24 lowercase hexadecimal bytes".into(),
        ));
    }
    if string_field(fields, "producer")? != "jet-cli" {
        return Err(RecordingError::Invalid("unexpected recording producer".into()));
    }
    if string_field(fields, "schema")? != SCHEMA {
        return Err(RecordingError::Invalid("unexpected recording schema".into()));
    }
    let version = object_field(fields, "version")?;
    let version_fields = object_fields(version)?;
    expect_keys(version_fields, &["major", "minor"])?;
    if u16_field(version_fields, "major")? != VERSION_MAJOR
        || u16_field(version_fields, "minor")? != VERSION_MINOR
    {
        return Err(RecordingError::Invalid("recording version is incompatible".into()));
    }
    let capture_fields = object_fields(object_field(fields, "capture")?)?;
    expect_keys(capture_fields, &["mode", "roots"])?;
    if string_field(capture_fields, "mode")? != "tui" {
        return Err(RecordingError::Invalid("recording capture mode is not tui".into()));
    }
    let roots = match object_field(capture_fields, "roots")? {
        CanonicalJson::Array(values) => values,
        _ => return Err(RecordingError::Invalid("recording roots must be an array".into())),
    };
    if roots
        != &[
            CanonicalJson::String("Input".into()),
            CanonicalJson::String("Output".into()),
            CanonicalJson::String("Frame".into()),
        ]
    {
        return Err(RecordingError::Invalid("recording roots are incompatible".into()));
    }
    let limits_fields = object_fields(object_field(fields, "limits")?)?;
    expect_keys(limits_fields, &["frames", "payload_bytes"])?;
    if integer_field(limits_fields, "frames")? != MAX_RECORDING_FRAMES as u64
        || integer_field(limits_fields, "payload_bytes")? != (MAX_RECORDING_BYTES - FOOTER_BYTES) as u64
    {
        return Err(RecordingError::Invalid("recording limits are incompatible".into()));
    }
    let identity_fields = object_fields(object_field(fields, "identity")?)?;
    expect_keys(identity_fields, &["command", "terminal"])?;
    let terminal_fields = object_fields(object_field(identity_fields, "terminal")?)?;
    expect_keys(terminal_fields, &["color", "height", "width"])?;
    let identity = RecordingIdentity {
        command: string_field(identity_fields, "command")?,
        width: u16_field(terminal_fields, "width")?,
        height: u16_field(terminal_fields, "height")?,
        color: bool_field(terminal_fields, "color")?,
    };
    validate_identity(&identity)?;
    Ok((identity, artifact_id))
}

fn object<const N: usize>(
    fields: [(&'static str, CanonicalJson); N],
) -> Result<CanonicalJson, RecordingError> {
    object_value(fields.into_iter())
}

fn object_value<I>(fields: I) -> Result<CanonicalJson, RecordingError>
where
    I: IntoIterator<Item = (&'static str, CanonicalJson)>,
{
    CanonicalJson::object(fields.into_iter().map(|(key, value)| (key.to_string(), value)))
        .map_err(RecordingError::Invalid)
}

fn object_fields(value: &CanonicalJson) -> Result<&BTreeMap<String, CanonicalJson>, RecordingError> {
    match value {
        CanonicalJson::Object(fields) => Ok(fields),
        _ => Err(RecordingError::Invalid("recording value must be an object".into())),
    }
}

fn object_field<'a>(
    fields: &'a BTreeMap<String, CanonicalJson>,
    name: &str,
) -> Result<&'a CanonicalJson, RecordingError> {
    fields
        .get(name)
        .ok_or_else(|| RecordingError::Invalid(format!("recording object is missing `{name}`")))
}

fn expect_keys(fields: &BTreeMap<String, CanonicalJson>, expected: &[&str]) -> Result<(), RecordingError> {
    let expected: BTreeSet<&str> = expected.iter().copied().collect();
    let actual: BTreeSet<&str> = fields.keys().map(String::as_str).collect();
    if actual != expected {
        return Err(RecordingError::Invalid(format!(
            "recording object keys differ: expected {expected:?}, observed {actual:?}"
        )));
    }
    Ok(())
}

fn string_field(fields: &BTreeMap<String, CanonicalJson>, name: &str) -> Result<String, RecordingError> {
    match object_field(fields, name)? {
        CanonicalJson::String(value) => Ok(value.clone()),
        _ => Err(RecordingError::Invalid(format!("recording `{name}` must be a string"))),
    }
}

fn bool_field(fields: &BTreeMap<String, CanonicalJson>, name: &str) -> Result<bool, RecordingError> {
    match object_field(fields, name)? {
        CanonicalJson::Bool(value) => Ok(*value),
        _ => Err(RecordingError::Invalid(format!("recording `{name}` must be a boolean"))),
    }
}

fn integer_field(fields: &BTreeMap<String, CanonicalJson>, name: &str) -> Result<u64, RecordingError> {
    match object_field(fields, name)? {
        CanonicalJson::Integer(value) => value
            .parse::<u64>()
            .map_err(|_| RecordingError::Invalid(format!("recording `{name}` must be a non-negative integer"))),
        _ => Err(RecordingError::Invalid(format!("recording `{name}` must be an integer"))),
    }
}
fn signed_integer_field(
    fields: &BTreeMap<String, CanonicalJson>,
    name: &str,
) -> Result<i32, RecordingError> {
    match object_field(fields, name)? {
        CanonicalJson::Integer(value) => {
            let value = value
                .parse::<i64>()
                .map_err(|_| RecordingError::Invalid(format!("recording `{name}` must be an integer")))?;
            i32::try_from(value)
                .map_err(|_| RecordingError::Invalid(format!("recording `{name}` is out of range")))
        }
        _ => Err(RecordingError::Invalid(format!("recording `{name}` must be an integer"))),
    }
}

fn u16_field(fields: &BTreeMap<String, CanonicalJson>, name: &str) -> Result<u16, RecordingError> {
    let value = integer_field(fields, name)?;
    u16::try_from(value)
        .map_err(|_| RecordingError::Invalid(format!("recording `{name}` is out of range")))
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

fn sensitive_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "PRIVATE",
        "CREDENTIAL",
        "AUTH",
        "API_KEY",
        "ACCESS_KEY",
        "COOKIE",
    ]
    .iter()
    .any(|marker| upper.contains(marker))
}

fn validate_name(name: &str) -> Result<(), RecordingError> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(RecordingError::Name(
            "recording NAME must be non-empty and contain only ASCII letters, digits, `-`, or `_`"
                .into(),
        ));
    }
    Ok(())
}

fn encode_frame(flags: u8, kind: u16, sequence: u64, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(FRAME_HEADER_BYTES + payload.len() + FRAME_HASH_BYTES);
    frame.push(flags);
    frame.extend_from_slice(&kind.to_le_bytes());
    frame.extend_from_slice(&sequence.to_le_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&SHA256::sha256(&frame));
    frame
}

fn write_prefix(out: &mut Vec<u8>, header: &[u8]) {
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION_MAJOR.to_le_bytes());
    out.extend_from_slice(&VERSION_MINOR.to_le_bytes());
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(header);
}

fn write_footer(out: &mut Vec<u8>, frames: u64, payload_bytes: u64) {
    let hash = SHA256::sha256(out);
    out.extend_from_slice(FOOTER_MAGIC);
    out.extend_from_slice(&frames.to_le_bytes());
    out.extend_from_slice(&payload_bytes.to_le_bytes());
    out.extend_from_slice(&hash);
}

fn content_id(body_with_zero_id: &[u8]) -> String {
    let digest = SHA256::sha256_hex(body_with_zero_id);
    digest[..24].to_string()
}

fn io_error(error: std::io::Error) -> RecordingError {
    RecordingError::Io(error.to_string())
}
