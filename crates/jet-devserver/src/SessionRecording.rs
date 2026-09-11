//! Bounded replay state for the shared `jet.devtools.v1` observation stream.
//!
//! The recorder only understands the envelope's transport facts: sequence,
//! monotonic timestamp, source, kind, entity, and opaque JSON objects. It does
//! not assign meaning to an event family or manufacture a payload. Replay
//! cursors operate on an immutable recording generation and fail closed when
//! the source/build identity or retained window changes.

use crate::Session::JET_DEVTOOLS_PROTOCOL;
use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::{json_escape, parse_json_with_limit};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;

/// The largest event ring admitted by the shared devtools policy.
pub const MAX_RECORDING_EVENTS: usize = 256;
/// The largest byte budget admitted by the shared devtools policy.
pub const MAX_RECORDING_BYTES: usize = 1024 * 1024;
/// Maximum size of one identity or event text label.
pub const MAX_RECORDING_TEXT_BYTES: usize = 16 * 1024;
/// Maximum size of an identity component. IDs are opaque stable strings.
pub const MAX_ID_BYTES: usize = 1024;

/// Explicit source/build identity attached to one recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionIdentity {
    pub source_id: String,
    pub build_id: String,
}

impl SessionIdentity {
    /// Construct and validate an identity without consulting ambient state.
    pub fn new(source_id: impl Into<String>, build_id: impl Into<String>) -> Result<Self, RecordingError> {
        let identity = Self {
            source_id: source_id.into(),
            build_id: build_id.into(),
        };
        validate_id(&identity.source_id, "source_id")?;
        validate_id(&identity.build_id, "build_id")?;
        Ok(identity)
    }

    /// Return whether both identity components match exactly.
    pub fn matches(&self, other: &Self) -> bool {
        self == other
    }
}

/// Hard limits for one in-memory session recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionRecordingLimits {
    pub max_events: usize,
    pub max_bytes: usize,
}

impl SessionRecordingLimits {
    /// Declare a bounded policy. Zero and over-policy values are rejected when
    /// the policy is attached to a recording.
    pub const fn new(max_events: usize, max_bytes: usize) -> Self {
        Self {
            max_events,
            max_bytes,
        }
    }

    fn validate(self) -> Result<(), RecordingError> {
        if self.max_events == 0 || self.max_events > MAX_RECORDING_EVENTS {
            return Err(RecordingError::InvalidLimits {
                max_events: self.max_events,
                max_bytes: self.max_bytes,
            });
        }
        if self.max_bytes == 0 || self.max_bytes > MAX_RECORDING_BYTES {
            return Err(RecordingError::InvalidLimits {
                max_events: self.max_events,
                max_bytes: self.max_bytes,
            });
        }
        Ok(())
    }
}

impl Default for SessionRecordingLimits {
    fn default() -> Self {
        Self::new(MAX_RECORDING_EVENTS, MAX_RECORDING_BYTES)
    }
}

/// One opaque event fact copied from a devtools envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionEvent {
    sequence: u64,
    timestamp_ms: u64,
    source: String,
    kind: String,
    entity: String,
    fields_json: String,
    payload_json: Option<String>,
}

impl SessionEvent {
    /// Construct one event from envelope facts.
    ///
    /// `fields_json` and an explicitly supplied `payload_json` must be JSON
    /// objects. A `None` payload remains absent; no default value is added.
    pub fn new(
        sequence: u64,
        timestamp_ms: u64,
        source: impl Into<String>,
        kind: impl Into<String>,
        entity: impl Into<String>,
        fields_json: impl Into<String>,
        payload_json: Option<String>,
    ) -> Result<Self, RecordingError> {
        let source = source.into();
        let kind = kind.into();
        let entity = entity.into();
        validate_text(&source, "event source")?;
        validate_text(&kind, "event kind")?;
        validate_text(&entity, "event entity")?;
        let fields_json = canonical_object(&fields_json.into(), "event fields")?;
        let payload_json = match payload_json {
            None => None,
            Some(payload) => {
                let value = parse_json_value(&payload, "event payload")?;
                match value {
                    DataTree::Null => None,
                    value @ DataTree::Object(_) => Some(canonical_json(&value)),
                    _ => {
                        return Err(RecordingError::InvalidEvent(
                            "event payload must be an object or null".to_string(),
                        ));
                    }
                }
            }
        };
        let event = Self {
            sequence,
            timestamp_ms,
            source,
            kind,
            entity,
            fields_json,
            payload_json,
        };
        if event.wire_len() > MAX_RECORDING_BYTES {
            return Err(RecordingError::EventTooLarge {
                bytes: event.wire_len(),
                limit: MAX_RECORDING_BYTES,
            });
        }
        Ok(event)
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn timestamp_ms(&self) -> u64 {
        self.timestamp_ms
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn entity(&self) -> &str {
        &self.entity
    }

    /// Return the canonical opaque metadata object.
    pub fn fields_json(&self) -> &str {
        &self.fields_json
    }

    /// Return a payload only when its producer explicitly supplied one.
    pub fn payload_json(&self) -> Option<&str> {
        self.payload_json.as_deref()
    }

    fn wire_len(&self) -> usize {
        let payload = self.payload_json.as_deref().unwrap_or("null");
        format!(
            "{{\"sequence\":{},\"timestamp_ms\":{},\"source\":{},\"kind\":{},\"entity\":{},\"fields\":{},\"payload\":{}}}",
            self.sequence,
            self.timestamp_ms,
            quoted(&self.source),
            quoted(&self.kind),
            quoted(&self.entity),
            self.fields_json,
            payload,
        )
        .len()
    }
}

/// Why and how much of a recording was evicted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TruncationFacts {
    pub event_limit: usize,
    pub byte_limit: usize,
    pub retained_events: usize,
    pub retained_bytes: usize,
    pub dropped_events: u64,
    pub dropped_bytes: u64,
    pub event_limit_reached: bool,
    pub byte_limit_reached: bool,
    pub first_dropped_sequence: Option<u64>,
    pub last_dropped_sequence: Option<u64>,
    pub first_retained_sequence: Option<u64>,
    pub last_retained_sequence: Option<u64>,
}

impl TruncationFacts {
    pub fn truncated(&self) -> bool {
        self.dropped_events != 0
    }
}

/// Errors are typed so identity, ordering, and bound failures cannot be
/// confused with a domain event's opaque payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordingError {
    InvalidIdentity(String),
    InvalidLimits { max_events: usize, max_bytes: usize },
    InvalidEnvelope(String),
    InvalidEvent(String),
    EnvelopeTooLarge { bytes: usize, limit: usize },
    EventTooLarge { bytes: usize, limit: usize },
    NonMonotonicSequence { previous: u64, next: u64 },
    NonMonotonicTime { previous: u64, next: u64 },
    IdentityMismatch {
        expected: SessionIdentity,
        found: SessionIdentity,
    },
    RecordingChanged,
    CheckpointUnavailable,
    CursorUnavailable { sequence: u64 },
    ReplayAtBeginning,
    ReplayAtEnd,
}

impl fmt::Display for RecordingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity(message) => write!(formatter, "invalid session identity: {message}"),
            Self::InvalidLimits {
                max_events,
                max_bytes,
            } => write!(
                formatter,
                "session recording limits are outside the bounded policy: events={max_events}, bytes={max_bytes}"
            ),
            Self::InvalidEnvelope(message) => write!(formatter, "invalid jet.devtools.v1 envelope: {message}"),
            Self::InvalidEvent(message) => write!(formatter, "invalid jet.devtools.v1 event: {message}"),
            Self::EnvelopeTooLarge { bytes, limit } => {
                write!(formatter, "devtools envelope is {bytes} bytes; limit is {limit}")
            }
            Self::EventTooLarge { bytes, limit } => {
                write!(formatter, "devtools event is {bytes} bytes; limit is {limit}")
            }
            Self::NonMonotonicSequence { previous, next } => write!(
                formatter,
                "devtools event sequence moved backwards or repeated: previous={previous}, next={next}"
            ),
            Self::NonMonotonicTime { previous, next } => write!(
                formatter,
                "devtools event timestamp moved backwards: previous={previous}, next={next}"
            ),
            Self::IdentityMismatch { expected, found } => write!(
                formatter,
                "replay identity mismatch: expected source={} build={}, found source={} build={}",
                expected.source_id, expected.build_id, found.source_id, found.build_id
            ),
            Self::RecordingChanged => write!(formatter, "recording changed after replay cursor creation"),
            Self::CheckpointUnavailable => write!(formatter, "checkpoint does not belong to the retained recording"),
            Self::CursorUnavailable { sequence } => {
                write!(formatter, "replay sequence {sequence} is outside the retained window")
            }
            Self::ReplayAtBeginning => write!(formatter, "replay cursor is already at the beginning"),
            Self::ReplayAtEnd => write!(formatter, "replay cursor is already at the end"),
        }
    }
}

impl std::error::Error for RecordingError {}

/// Direction for a one-event deterministic replay step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayStep {
    Forward,
    Backward,
}

/// An immutable position bound to one recording generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayCursor {
    identity: SessionIdentity,
    generation: u64,
    position: usize,
    sequence: Option<u64>,
}

impl ReplayCursor {
    pub fn identity(&self) -> &SessionIdentity {
        &self.identity
    }

    /// Number of retained events consumed by this cursor.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Sequence at the current position, or `None` before the first event.
    pub fn sequence(&self) -> Option<u64> {
        self.sequence
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn seek(
        &mut self,
        recording: &SessionRecording,
        sequence: u64,
    ) -> Result<ReplayProjection, RecordingError> {
        self.check(recording)?;
        let Some(index) = recording
            .events
            .iter()
            .position(|event| event.sequence == sequence)
        else {
            return Err(RecordingError::CursorUnavailable { sequence });
        };
        self.position = index + 1;
        self.sequence = Some(sequence);
        recording.projection(self)
    }

    /// Seek to the newest event at or before an exact monotonic timestamp.
    /// Before the first retained timestamp, the cursor becomes the explicit
    /// before-start position rather than inventing an event.
    pub fn seek_time(
        &mut self,
        recording: &SessionRecording,
        timestamp_ms: u64,
    ) -> Result<ReplayProjection, RecordingError> {
        self.check(recording)?;
        if let Some(first) = recording.events.front() {
            if timestamp_ms < first.timestamp_ms && recording.dropped_events != 0 {
                return Err(RecordingError::CursorUnavailable {
                    sequence: first.sequence,
                });
            }
        }
        let position = recording
            .events
            .iter()
            .enumerate()
            .filter(|(_, event)| event.timestamp_ms <= timestamp_ms)
            .map(|(index, _)| index + 1)
            .last()
            .unwrap_or(0);
        self.position = position;
        self.sequence = position
            .checked_sub(1)
            .and_then(|index| recording.events.get(index))
            .map(|event| event.sequence);
        recording.projection(self)
    }

    pub fn step(
        &mut self,
        recording: &SessionRecording,
        direction: ReplayStep,
    ) -> Result<ReplayProjection, RecordingError> {
        match direction {
            ReplayStep::Forward => self.step_forward(recording),
            ReplayStep::Backward => self.step_backward(recording),
        }
    }

    pub fn step_forward(
        &mut self,
        recording: &SessionRecording,
    ) -> Result<ReplayProjection, RecordingError> {
        self.check(recording)?;
        let Some(event) = recording.events.get(self.position) else {
            return Err(RecordingError::ReplayAtEnd);
        };
        self.position += 1;
        self.sequence = Some(event.sequence);
        recording.projection(self)
    }

    pub fn step_backward(
        &mut self,
        recording: &SessionRecording,
    ) -> Result<ReplayProjection, RecordingError> {
        self.check(recording)?;
        if self.position == 0 {
            return Err(RecordingError::ReplayAtBeginning);
        }
        self.position -= 1;
        self.sequence = self
            .position
            .checked_sub(1)
            .and_then(|index| recording.events.get(index))
            .map(|event| event.sequence);
        recording.projection(self)
    }

    fn check(&self, recording: &SessionRecording) -> Result<(), RecordingError> {
        if !self.identity.matches(&recording.identity) {
            return Err(RecordingError::IdentityMismatch {
                expected: self.identity.clone(),
                found: recording.identity.clone(),
            });
        }
        if self.generation != recording.generation {
            return Err(RecordingError::RecordingChanged);
        }
        if self.position > recording.events.len() {
            return Err(RecordingError::CheckpointUnavailable);
        }
        Ok(())
    }
}

/// A replay restore point. It is bound to identity, generation, and retained
/// window so restoring against a different source/build fails closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionCheckpoint {
    identity: SessionIdentity,
    generation: u64,
    position: usize,
    sequence: Option<u64>,
    timestamp_ms: Option<u64>,
    first_retained_sequence: Option<u64>,
    last_retained_sequence: Option<u64>,
}

impl SessionCheckpoint {
    pub fn identity(&self) -> &SessionIdentity {
        &self.identity
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn sequence(&self) -> Option<u64> {
        self.sequence
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn timestamp_ms(&self) -> Option<u64> {
        self.timestamp_ms
    }
}

/// A read-only projection at one replay position. It carries the generic
/// event fact plus visible ring bounds and truncation evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayProjection {
    identity: SessionIdentity,
    position: usize,
    sequence: Option<u64>,
    timestamp_ms: Option<u64>,
    event: Option<SessionEvent>,
    previous_sequence: Option<u64>,
    next_sequence: Option<u64>,
    limits: SessionRecordingLimits,
    truncation: TruncationFacts,
}

impl ReplayProjection {
    pub fn identity(&self) -> &SessionIdentity {
        &self.identity
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn sequence(&self) -> Option<u64> {
        self.sequence
    }

    pub fn timestamp_ms(&self) -> Option<u64> {
        self.timestamp_ms
    }

    pub fn event(&self) -> Option<&SessionEvent> {
        self.event.as_ref()
    }

    pub fn previous_sequence(&self) -> Option<u64> {
        self.previous_sequence
    }

    pub fn next_sequence(&self) -> Option<u64> {
        self.next_sequence
    }

    pub fn limits(&self) -> SessionRecordingLimits {
        self.limits
    }

    pub fn truncation(&self) -> &TruncationFacts {
        &self.truncation
    }
}

/// Mutable capture boundary whose replay views are immutable by generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRecording {
    identity: SessionIdentity,
    limits: SessionRecordingLimits,
    events: VecDeque<SessionEvent>,
    retained_bytes: usize,
    dropped_events: u64,
    dropped_bytes: u64,
    event_limit_reached: bool,
    byte_limit_reached: bool,
    first_dropped_sequence: Option<u64>,
    last_dropped_sequence: Option<u64>,
    last_sequence: Option<u64>,
    last_timestamp_ms: Option<u64>,
    generation: u64,
}

impl SessionRecording {
    pub fn new(identity: SessionIdentity) -> Result<Self, RecordingError> {
        Self::with_limits(identity, SessionRecordingLimits::default())
    }

    pub fn with_limits(
        identity: SessionIdentity,
        limits: SessionRecordingLimits,
    ) -> Result<Self, RecordingError> {
        validate_id(&identity.source_id, "source_id")?;
        validate_id(&identity.build_id, "build_id")?;
        limits.validate()?;
        Ok(Self {
            identity,
            limits,
            events: VecDeque::new(),
            retained_bytes: 0,
            dropped_events: 0,
            dropped_bytes: 0,
            event_limit_reached: false,
            byte_limit_reached: false,
            first_dropped_sequence: None,
            last_dropped_sequence: None,
            last_sequence: None,
            last_timestamp_ms: None,
            generation: 0,
        })
    }

    pub fn identity(&self) -> &SessionIdentity {
        &self.identity
    }

    pub fn limits(&self) -> SessionRecordingLimits {
        self.limits
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn events(&self) -> impl Iterator<Item = &SessionEvent> {
        self.events.iter()
    }

    /// Return explicit retained-window and eviction facts.
    pub fn truncation(&self) -> TruncationFacts {
        TruncationFacts {
            event_limit: self.limits.max_events,
            byte_limit: self.limits.max_bytes,
            retained_events: self.events.len(),
            retained_bytes: self.retained_bytes,
            dropped_events: self.dropped_events,
            dropped_bytes: self.dropped_bytes,
            event_limit_reached: self.event_limit_reached,
            byte_limit_reached: self.byte_limit_reached,
            first_dropped_sequence: self.first_dropped_sequence,
            last_dropped_sequence: self.last_dropped_sequence,
            first_retained_sequence: self.events.front().map(|event| event.sequence),
            last_retained_sequence: self.events.back().map(|event| event.sequence),
        }
    }

    /// Append one already-validated protocol event while preserving its exact
    /// sequence and timestamp facts.
    pub fn append_event(&mut self, event: SessionEvent) -> Result<u64, RecordingError> {
        self.validate_next(&event)?;
        let sequence = event.sequence;
        self.append_event_unchecked(event);
        Ok(sequence)
    }

    /// Parse and append one complete `jet.devtools.v1` envelope. The whole
    /// batch is preflighted before the first event is retained.
    pub fn record_envelope(&mut self, bytes: &[u8]) -> Result<usize, RecordingError> {
        if bytes.len() > MAX_RECORDING_BYTES {
            return Err(RecordingError::EnvelopeTooLarge {
                bytes: bytes.len(),
                limit: MAX_RECORDING_BYTES,
            });
        }
        let events = parse_envelope(bytes, &self.identity)?;
        self.preflight(&events)?;
        let count = events.len();
        for event in events {
            self.append_event_unchecked(event);
        }
        Ok(count)
    }

    /// Open a cursor at the deterministic before-start position.
    pub fn replay(&self) -> ReplayCursor {
        ReplayCursor {
            identity: self.identity.clone(),
            generation: self.generation,
            position: 0,
            sequence: None,
        }
    }

    /// Open a replay only when the caller presents the exact source/build
    /// identity expected by this recording.
    pub fn replay_for(&self, identity: &SessionIdentity) -> Result<ReplayCursor, RecordingError> {
        validate_id(&identity.source_id, "source_id")?;
        validate_id(&identity.build_id, "build_id")?;
        if !self.identity.matches(identity) {
            return Err(RecordingError::IdentityMismatch {
                expected: self.identity.clone(),
                found: identity.clone(),
            });
        }
        Ok(self.replay())
    }

    pub fn projection(&self, cursor: &ReplayCursor) -> Result<ReplayProjection, RecordingError> {
        cursor.check(self)?;
        Ok(self.make_projection(cursor))
    }

    pub fn checkpoint(&self, cursor: &ReplayCursor) -> Result<SessionCheckpoint, RecordingError> {
        cursor.check(self)?;
        let timestamp_ms = cursor
            .position
            .checked_sub(1)
            .and_then(|index| self.events.get(index))
            .map(|event| event.timestamp_ms);
        Ok(SessionCheckpoint {
            identity: self.identity.clone(),
            generation: self.generation,
            position: cursor.position,
            sequence: cursor.sequence,
            timestamp_ms,
            first_retained_sequence: self.events.front().map(|event| event.sequence),
            last_retained_sequence: self.events.back().map(|event| event.sequence),
        })
    }

    pub fn restore_checkpoint(
        &self,
        checkpoint: &SessionCheckpoint,
    ) -> Result<ReplayCursor, RecordingError> {
        if !self.identity.matches(&checkpoint.identity) {
            return Err(RecordingError::IdentityMismatch {
                expected: self.identity.clone(),
                found: checkpoint.identity.clone(),
            });
        }
        if checkpoint.generation != self.generation
            || checkpoint.first_retained_sequence
                != self.events.front().map(|event| event.sequence)
            || checkpoint.last_retained_sequence != self.events.back().map(|event| event.sequence)
            || checkpoint.position > self.events.len()
        {
            return Err(RecordingError::CheckpointUnavailable);
        }
        let actual_sequence = checkpoint
            .position
            .checked_sub(1)
            .and_then(|index| self.events.get(index))
            .map(|event| event.sequence);
        if actual_sequence != checkpoint.sequence {
            return Err(RecordingError::CheckpointUnavailable);
        }
        Ok(ReplayCursor {
            identity: self.identity.clone(),
            generation: self.generation,
            position: checkpoint.position,
            sequence: checkpoint.sequence,
        })
    }

    fn preflight(&self, events: &[SessionEvent]) -> Result<(), RecordingError> {
        let mut previous_sequence = self.last_sequence;
        let mut previous_time = self.last_timestamp_ms;
        for event in events {
            let bytes = event.wire_len();
            if bytes > self.limits.max_bytes {
                return Err(RecordingError::EventTooLarge {
                    bytes,
                    limit: self.limits.max_bytes,
                });
            }
            if let Some(previous) = previous_sequence {
                if event.sequence <= previous {
                    return Err(RecordingError::NonMonotonicSequence {
                        previous,
                        next: event.sequence,
                    });
                }
            }
            if let Some(previous) = previous_time {
                if event.timestamp_ms < previous {
                    return Err(RecordingError::NonMonotonicTime {
                        previous,
                        next: event.timestamp_ms,
                    });
                }
            }
            previous_sequence = Some(event.sequence);
            previous_time = Some(event.timestamp_ms);
        }
        Ok(())
    }

    fn validate_next(&self, event: &SessionEvent) -> Result<(), RecordingError> {
        let bytes = event.wire_len();
        if bytes > self.limits.max_bytes {
            return Err(RecordingError::EventTooLarge {
                bytes,
                limit: self.limits.max_bytes,
            });
        }
        if let Some(previous) = self.last_sequence {
            if event.sequence <= previous {
                return Err(RecordingError::NonMonotonicSequence {
                    previous,
                    next: event.sequence,
                });
            }
        }
        if let Some(previous) = self.last_timestamp_ms {
            if event.timestamp_ms < previous {
                return Err(RecordingError::NonMonotonicTime {
                    previous,
                    next: event.timestamp_ms,
                });
            }
        }
        Ok(())
    }

    fn append_event_unchecked(&mut self, event: SessionEvent) {
        let bytes = event.wire_len();
        self.last_sequence = Some(event.sequence);
        self.last_timestamp_ms = Some(event.timestamp_ms);
        self.events.push_back(event);
        self.retained_bytes = self.retained_bytes.saturating_add(bytes);
        if self.events.len() > self.limits.max_events {
            self.event_limit_reached = true;
        }
        while self.events.len() > self.limits.max_events
            || self.retained_bytes > self.limits.max_bytes
        {
            if self.events.len() > self.limits.max_events {
                self.event_limit_reached = true;
            }
            if self.retained_bytes > self.limits.max_bytes {
                self.byte_limit_reached = true;
            }
            let Some(evicted) = self.events.pop_front() else {
                break;
            };
            let evicted_bytes = evicted.wire_len();
            self.retained_bytes = self.retained_bytes.saturating_sub(evicted_bytes);
            self.dropped_events = self.dropped_events.saturating_add(1);
            self.dropped_bytes = self
                .dropped_bytes
                .saturating_add(evicted_bytes as u64);
            if self.first_dropped_sequence.is_none() {
                self.first_dropped_sequence = Some(evicted.sequence);
            }
            self.last_dropped_sequence = Some(evicted.sequence);
        }
        self.generation = self.generation.wrapping_add(1);
    }

    fn make_projection(&self, cursor: &ReplayCursor) -> ReplayProjection {
        let current_index = cursor.position.checked_sub(1);
        let event = current_index.and_then(|index| self.events.get(index)).cloned();
        let previous_sequence = cursor
            .position
            .checked_sub(2)
            .and_then(|index| self.events.get(index))
            .map(|event| event.sequence);
        let next_sequence = self
            .events
            .get(cursor.position)
            .map(|event| event.sequence);
        ReplayProjection {
            identity: self.identity.clone(),
            position: cursor.position,
            sequence: cursor.sequence,
            timestamp_ms: event.as_ref().map(|event| event.timestamp_ms),
            event,
            previous_sequence,
            next_sequence,
            limits: self.limits,
            truncation: self.truncation(),
        }
    }
}

fn parse_envelope(
    bytes: &[u8],
    expected_identity: &SessionIdentity,
) -> Result<Vec<SessionEvent>, RecordingError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| RecordingError::InvalidEnvelope("envelope is not UTF-8".to_string()))?;
    let root = parse_json_value(text, "devtools envelope")?;
    let object = match root {
        DataTree::Object(object) => object.into_iter().collect::<BTreeMap<_, _>>(),
        _ => {
            return Err(RecordingError::InvalidEnvelope(
                "envelope must be an object".to_string(),
            ));
        }
    };
    let protocol = object
        .get("protocol")
        .and_then(|value| match value {
            DataTree::Text(protocol) => Some(protocol.as_str()),
            _ => None,
        })
        .ok_or_else(|| RecordingError::InvalidEnvelope("missing string `protocol`".to_string()))?;
    if protocol != JET_DEVTOOLS_PROTOCOL {
        return Err(RecordingError::InvalidEnvelope(format!(
            "protocol is `{protocol}`, expected `{JET_DEVTOOLS_PROTOCOL}`"
        )));
    }
    validate_optional_envelope_identity(&object, expected_identity)?;
    let values = object
        .get("events")
        .ok_or_else(|| RecordingError::InvalidEnvelope("missing `events` array".to_string()))?;
    let values = match values {
        DataTree::Array(values) => values,
        _ => {
            return Err(RecordingError::InvalidEnvelope(
                "envelope `events` must be an array".to_string(),
            ));
        }
    };
    values.iter().map(parse_event).collect()
}

fn parse_event(value: &DataTree) -> Result<SessionEvent, RecordingError> {
    let object = match value {
        DataTree::Object(object) => object.iter().cloned().collect::<BTreeMap<_, _>>(),
        _ => {
            return Err(RecordingError::InvalidEvent(
                "event must be an object".to_string(),
            ));
        }
    };
    let sequence = required_u64(&object, "sequence")?;
    let timestamp_ms = required_u64(&object, "timestamp_ms")?;
    let source = required_string(&object, "source")?;
    let kind = required_string(&object, "kind")?;
    let entity = required_string(&object, "entity")?;
    let fields = object
        .get("fields")
        .ok_or_else(|| RecordingError::InvalidEvent("missing event `fields`".to_string()))?;
    let fields_json = match fields {
        DataTree::Object(_) => canonical_json(fields),
        _ => {
            return Err(RecordingError::InvalidEvent(
                "event fields must be an object".to_string(),
            ));
        }
    };
    let payload_json = match object.get("payload") {
        None | Some(DataTree::Null) => None,
        Some(value @ DataTree::Object(_)) => Some(canonical_json(value)),
        Some(_) => {
            return Err(RecordingError::InvalidEvent(
                "event payload must be an object or null".to_string(),
            ));
        }
    };
    SessionEvent::new(
        sequence,
        timestamp_ms,
        source,
        kind,
        entity,
        fields_json,
        payload_json,
    )
}

fn parse_json_value(text: &str, label: &str) -> Result<DataTree, RecordingError> {
    if text.len() > MAX_RECORDING_BYTES {
        return Err(RecordingError::InvalidEvent(format!(
            "{label} exceeds the {}-byte limit",
            MAX_RECORDING_BYTES
        )));
    }
    parse_json_with_limit(text, MAX_RECORDING_BYTES).map_err(|_| {
        if label == "devtools envelope" {
            RecordingError::InvalidEnvelope(format!("{label} is not valid bounded JSON"))
        } else {
            RecordingError::InvalidEvent(format!("{label} is not valid bounded JSON"))
        }
    })
}

fn canonical_object(text: &str, label: &str) -> Result<String, RecordingError> {
    if text.len() > MAX_RECORDING_TEXT_BYTES {
        return Err(RecordingError::InvalidEvent(format!(
            "{label} exceeds the {}-byte JSON object limit",
            MAX_RECORDING_TEXT_BYTES
        )));
    }
    let value = parse_json_value(text, label)?;
    if !matches!(value, DataTree::Object(_)) {
        return Err(RecordingError::InvalidEvent(format!("{label} must be an object")));
    }
    Ok(canonical_json(&value))
}

fn required_string(object: &BTreeMap<String, DataTree>, key: &str) -> Result<String, RecordingError> {
    let value = object
        .get(key)
        .and_then(|value| match value {
            DataTree::Text(value) => Some(value.clone()),
            _ => None,
        })
        .ok_or_else(|| RecordingError::InvalidEvent(format!("missing string `{key}`")))?;
    validate_text(&value, &format!("event {key}"))?;
    Ok(value)
}

fn required_u64(object: &BTreeMap<String, DataTree>, key: &str) -> Result<u64, RecordingError> {
    match object.get(key) {
        Some(DataTree::Int(value)) => u64::try_from(*value).map_err(|_| {
            RecordingError::InvalidEvent(format!("event `{key}` must be a non-negative integer"))
        }),
        Some(_) => Err(RecordingError::InvalidEvent(format!(
            "event `{key}` must be an exact integer"
        ))),
        None => Err(RecordingError::InvalidEvent(format!("missing integer `{key}`"))),
    }
}

fn validate_optional_envelope_identity(
    object: &BTreeMap<String, DataTree>,
    expected: &SessionIdentity,
) -> Result<(), RecordingError> {
    let source = object.get("source_id");
    let build = object.get("build_id");
    if source.is_none() && build.is_none() {
        return Ok(());
    }
    let source = source
        .and_then(|value| match value {
            DataTree::Text(value) => Some(value.clone()),
            _ => None,
        })
        .ok_or_else(|| {
            RecordingError::InvalidEnvelope("envelope `source_id` must be a string".to_string())
        })?;
    let build = build
        .and_then(|value| match value {
            DataTree::Text(value) => Some(value.clone()),
            _ => None,
        })
        .ok_or_else(|| {
            RecordingError::InvalidEnvelope("envelope `build_id` must be a string".to_string())
        })?;
    let found = SessionIdentity::new(source, build).map_err(|error| {
        RecordingError::InvalidEnvelope(format!("invalid envelope identity: {error}"))
    })?;
    if !expected.matches(&found) {
        return Err(RecordingError::IdentityMismatch {
            expected: expected.clone(),
            found,
        });
    }
    Ok(())
}

fn validate_id(value: &str, label: &str) -> Result<(), RecordingError> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(RecordingError::InvalidIdentity(format!(
            "{label} must contain 1..={} bytes and no control characters",
            MAX_ID_BYTES
        )));
    }
    Ok(())
}

 
fn validate_text(value: &str, label: &str) -> Result<(), RecordingError> {
    if value.len() > MAX_RECORDING_TEXT_BYTES {
        return Err(RecordingError::InvalidEvent(format!(
            "{label} exceeds the {}-byte text limit",
            MAX_RECORDING_TEXT_BYTES
        )));
    }
    Ok(())
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
}

fn canonical_json(value: &DataTree) -> String {
    match value {
        DataTree::Null => "null".to_string(),
        DataTree::Bool(value) => value.to_string(),
        DataTree::Int(value) => value.to_string(),
        DataTree::Float(value) => value.to_string(),
        DataTree::Number(value) => value.clone(),
        DataTree::TypedText(value) | DataTree::Text(value) => quoted(value),
        DataTree::Bytes(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
        DataTree::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        DataTree::Object(values) => format!(

            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!("{}:{}", quoted(key), canonical_json(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(source_id: &str, build_id: &str) -> SessionIdentity {
        SessionIdentity::new(source_id, build_id).unwrap()
    }

    fn event(sequence: u64, timestamp_ms: u64) -> SessionEvent {
        SessionEvent::new(
            sequence,
            timestamp_ms,
            "runtime",
            "Trace",
            "request-1",
            "{\"status\":\"ok\"}",
            None,
        )
        .unwrap()
    }

    #[test]
    fn replay_steps_and_checkpoint_restore_are_deterministic() {
        let mut recording = SessionRecording::with_limits(
            identity("src-a", "build-a"),
            SessionRecordingLimits::new(8, 64 * 1024),
        )
        .unwrap();
        recording.append_event(event(1, 10)).unwrap();
        recording.append_event(event(2, 20)).unwrap();
        recording.append_event(event(3, 30)).unwrap();

        let mut cursor = recording.replay();
        assert_eq!(cursor.step_forward(&recording).unwrap().sequence(), Some(1));
        let checkpoint = recording.checkpoint(&cursor).unwrap();
        assert_eq!(cursor.step_forward(&recording).unwrap().sequence(), Some(2));
        assert_eq!(cursor.step_forward(&recording).unwrap().sequence(), Some(3));
        assert_eq!(cursor.step_backward(&recording).unwrap().sequence(), Some(2));

        let restored = recording.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(restored.sequence(), Some(1));
        assert_eq!(recording.projection(&restored).unwrap().next_sequence(), Some(2));
        assert_eq!(recording.projection(&restored).unwrap().event().unwrap().sequence(), 1);
    }

    #[test]
    fn truncation_facts_expose_count_and_window() {
        let mut recording = SessionRecording::with_limits(
            identity("src-a", "build-a"),
            SessionRecordingLimits::new(2, 64 * 1024),
        )
        .unwrap();
        recording.append_event(event(1, 10)).unwrap();
        recording.append_event(event(2, 20)).unwrap();
        recording.append_event(event(3, 30)).unwrap();
        let facts = recording.truncation();
        assert_eq!(facts.retained_events, 2);
        assert_eq!(facts.first_retained_sequence, Some(2));
        assert_eq!(facts.last_retained_sequence, Some(3));
        assert_eq!(facts.dropped_events, 1);
        assert_eq!(facts.first_dropped_sequence, Some(1));
        assert!(facts.event_limit_reached);
        assert!(facts.truncated());
    }

    #[test]
    fn incompatible_identity_fails_closed_and_payload_stays_absent() {
        let recording = SessionRecording::new(identity("src-a", "build-a")).unwrap();
        let error = recording
            .replay_for(&identity("src-b", "build-a"))
            .unwrap_err();
        assert!(matches!(error, RecordingError::IdentityMismatch { .. }));

        let event = event(1, 1);
        assert_eq!(event.payload_json(), None);
    }

    #[test]
    fn envelope_preserves_explicit_payload_and_rejects_time_regressions() {
        let mut recording = SessionRecording::new(identity("src-a", "build-a")).unwrap();
        let envelope = format!(
            "{{\"protocol\":\"{}\",\"events\":[{{\"sequence\":1,\"timestamp_ms\":7,\"source\":\"host\",\"kind\":\"Custom\",\"entity\":\"item\",\"fields\":{{\"state\":\"ready\"}},\"payload\":{{\"published\":true}}}}]}}",
            JET_DEVTOOLS_PROTOCOL
        );
        assert_eq!(recording.record_envelope(envelope.as_bytes()).unwrap(), 1);
        assert_eq!(
            recording.events().next().unwrap().payload_json(),
            Some("{\"published\":true}")
        );

        let error = recording.append_event(event(2, 6)).unwrap_err();
        assert!(matches!(error, RecordingError::NonMonotonicTime { .. }));
    }
}
