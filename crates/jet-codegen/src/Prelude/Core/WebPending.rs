// D-DX-WEBPENDING1 / card #2478: one backend-neutral kernel for streaming
// boundaries, islands, and resumable hydration.  The module owns typed facts,
// bounded transactions, and deterministic projections only.  It does not
// render HTML, execute JavaScript, or call a host adapter.
//
// A boundary stages chunks separately from its last committed content.  Every
// identity, sequence, lifecycle transition, and hydration timestamp is checked
// before publication.  A rejected update rolls back the staged value and keeps
// the last committed page/world available to both projections.

pub const JET_WEB_PENDING_MAX_CHUNKS: usize = 64;
pub const JET_WEB_PENDING_MAX_BUFFER_BYTES: usize = 1024 * 1024;
pub const JET_WEB_PENDING_MAX_FACTS: usize = 256;
pub const JET_WEB_PENDING_MAX_IDENTITY_BYTES: usize = 1024;
pub const JET_WEB_PENDING_MAX_BOUNDARY_BYTES: usize = 256;
pub const JET_WEB_STREAM_REGISTRY_MAX: usize = 256;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub enum JetWebPendingState {
    #[default]
    Pending,
    Ready,
    Failed,
}

impl JetWebPendingState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    pub const fn is_pending(self) -> bool {
        matches!(self, Self::Pending)
    }

    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    pub const fn is_failed(self) -> bool {
        matches!(self, Self::Failed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum JetWebPendingBackpressureDimension {
    Chunks,
    Bytes,
}

impl JetWebPendingBackpressureDimension {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chunks => "chunks",
            Self::Bytes => "bytes",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum JetWebPendingProjectionTarget {
    Server,
    Client,
}

impl JetWebPendingProjectionTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Client => "client",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum JetWebPendingTransactionKind {
    Begin,
    Chunk,
    Commit,
    Fail,
    Cancel,
    Rollback,
    HydrationStart,
    HydrationComplete,
    Reject,
}

impl JetWebPendingTransactionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Chunk => "chunk",
            Self::Commit => "commit",
            Self::Fail => "fail",
            Self::Cancel => "cancel",
            Self::Rollback => "rollback",
            Self::HydrationStart => "hydration_start",
            Self::HydrationComplete => "hydration_complete",
            Self::Reject => "reject",
        }
    }
}

/// The complete failure vocabulary at the streaming/hydration boundary.
/// Dynamic page data is never hidden in an untyped error string.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetWebPendingError {
    InvalidIdentity { field: &'static str },
    InvalidBoundary { field: &'static str },
    InvalidLimit { field: &'static str, actual: usize, maximum: usize },
    IdentityMismatch {
        expected: JetWebIslandIdentity,
        actual: JetWebIslandIdentity,
    },
    InvalidChunkSequence { expected: u64, actual: u64 },
    Backpressure {
        dimension: JetWebPendingBackpressureDimension,
        limit: usize,
        buffered: usize,
        incoming: usize,
    },
    ContentTooLarge { limit: usize, actual: usize },
    InvalidLifecycle {
        state: JetWebPendingState,
        operation: &'static str,
    },
    MissingFinalChunk,
    InvalidHydrationTiming { started_at_ns: u64, completed_at_ns: u64 },
    HydrationIdentityMismatch {
        expected: JetWebIslandIdentity,
        actual: JetWebIslandIdentity,
    },
    NonMonotonicTimestamp { previous: u64, actual: u64 },
    SequenceExhausted { kind: &'static str },
    StreamIdentityMismatch {
        expected: JetWebStreamId,
        actual: JetWebStreamId,
    },
}

impl JetWebPendingError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidIdentity { .. } => "invalid_identity",
            Self::InvalidBoundary { .. } => "invalid_boundary",
            Self::InvalidLimit { .. } => "invalid_limit",
            Self::IdentityMismatch { .. } => "identity_mismatch",
            Self::InvalidChunkSequence { .. } => "invalid_chunk_sequence",
            Self::Backpressure { .. } => "backpressure",
            Self::ContentTooLarge { .. } => "content_too_large",
            Self::InvalidLifecycle { .. } => "invalid_lifecycle",
            Self::MissingFinalChunk => "missing_final_chunk",
            Self::InvalidHydrationTiming { .. } => "invalid_hydration_timing",
            Self::HydrationIdentityMismatch { .. } => "hydration_identity_mismatch",
            Self::NonMonotonicTimestamp { .. } => "non_monotonic_timestamp",
            Self::SequenceExhausted { .. } => "sequence_exhausted",
            Self::StreamIdentityMismatch { .. } => "stream_identity_mismatch",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidIdentity { field } => {
                format!("web pending {field} identity is empty, oversized, or contains control text")
            }
            Self::InvalidBoundary { field } => {
                format!("web pending boundary {field} is empty, oversized, or contains control text")
            }
            Self::InvalidLimit {
                field,
                actual,
                maximum,
            } => format!("web pending {field} limit {actual} is outside 1..={maximum}"),
            Self::IdentityMismatch { expected, actual } => format!(
                "web pending identity mismatch: expected {}, got {}",
                expected.compact(),
                actual.compact()
            ),
            Self::InvalidChunkSequence { expected, actual } => format!(
                "web pending chunk sequence is out of order: expected {expected}, got {actual}"
            ),
            Self::Backpressure {
                dimension,
                limit,
                buffered,
                incoming,
            } => format!(
                "web pending {} backpressure: limit {limit}, buffered {buffered}, incoming {incoming}",
                dimension.as_str()
            ),
            Self::ContentTooLarge { limit, actual } => {
                format!("web pending content is {actual} bytes; maximum is {limit}")
            }
            Self::InvalidLifecycle { state, operation } => format!(
                "web pending cannot perform {operation} while state is {}",
                state.as_str()
            ),
            Self::MissingFinalChunk => {
                "web pending cannot commit before the final chunk".to_string()
            }
            Self::InvalidHydrationTiming {
                started_at_ns,
                completed_at_ns,
            } => format!(
                "web hydration completed at {completed_at_ns} before it started at {started_at_ns}"
            ),
            Self::HydrationIdentityMismatch { expected, actual } => format!(
                "web hydration identity mismatch: expected {}, got {}",
                expected.compact(),
                actual.compact()
            ),
            Self::NonMonotonicTimestamp { previous, actual } => format!(
                "web pending timestamp moved backward from {previous} to {actual}"
            ),
            Self::SequenceExhausted { kind } => {
                format!("web pending {kind} sequence is exhausted")
            }
            Self::StreamIdentityMismatch { expected, actual } => format!(
                "web stream identity mismatch: expected {}, got {}",
                expected.compact(),
                actual.compact()
            ),
        }
    }
}

impl std::fmt::Display for JetWebPendingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for JetWebPendingError {}

/// Source, build, revision, and island identity travel together on every
/// stream and hydration operation.  Equality is exact: hosts must not accept
/// a chunk from another build merely because its island name is the same.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JetWebIslandIdentity {
    pub island_id: String,
    pub source_id: String,
    pub build_id: String,
    pub revision: String,
}

impl JetWebIslandIdentity {
    pub fn new(
        island_id: impl Into<String>,
        source_id: impl Into<String>,
        build_id: impl Into<String>,
        revision: impl Into<String>,
    ) -> Result<Self, JetWebPendingError> {
        let identity = Self {
            island_id: island_id.into(),
            source_id: source_id.into(),
            build_id: build_id.into(),
            revision: revision.into(),
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), JetWebPendingError> {
        jet_web_pending_validate_identity(&self.island_id, "island_id")?;
        jet_web_pending_validate_identity(&self.source_id, "source_id")?;
        jet_web_pending_validate_identity(&self.build_id, "build_id")?;
        jet_web_pending_validate_identity(&self.revision, "revision")
    }

    pub fn compact(&self) -> String {
        format!(
            "{}@{}:{}:{}",
            self.island_id, self.source_id, self.build_id, self.revision
        )
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"island_id\":{},\"source_id\":{},\"build_id\":{},\"revision\":{}}}",
            jet_web_pending_json_string(&self.island_id),
            jet_web_pending_json_string(&self.source_id),
            jet_web_pending_json_string(&self.build_id),
            jet_web_pending_json_string(&self.revision),
        )
    }
}

/// Opaque bridge identity for one named streaming boundary.
///
/// The type is intentionally not a user-facing `core.web` value.  Generated
/// App code passes it between the lifecycle wrappers so a host cannot
/// accidentally address a boundary with an unvalidated string.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JetWebBoundaryId(String);

impl JetWebBoundaryId {
    pub fn new(value: impl Into<String>) -> Result<Self, JetWebPendingError> {
        let value = value.into();
        jet_web_pending_validate_boundary(&value, "boundary_id")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn compact(&self) -> &str {
        self.as_str()
    }

    pub fn render_json(&self) -> String {
        jet_web_pending_json_string(self.as_str())
    }
}

/// Opaque bridge identity for one stream generation.  Cancellation, chunks,
/// commit, and hydration all require this exact identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JetWebStreamId {
    boundary_id: JetWebBoundaryId,
    generation: u64,
    identity: JetWebIslandIdentity,
}

impl JetWebStreamId {
    fn new(
        boundary_id: JetWebBoundaryId,
        generation: u64,
        identity: JetWebIslandIdentity,
    ) -> Self {
        Self {
            boundary_id,
            generation,
            identity,
        }
    }

    /// Reconstruct an opaque stream ID at a transport boundary after the host
    /// has decoded its typed JSON fields.
    pub fn from_parts(
        boundary_id: JetWebBoundaryId,
        generation: u64,
        identity: JetWebIslandIdentity,
    ) -> Result<Self, JetWebPendingError> {
        identity.validate()?;
        Ok(Self::new(boundary_id, generation, identity))
    }

    pub fn boundary_id(&self) -> &JetWebBoundaryId {
        &self.boundary_id
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn identity(&self) -> &JetWebIslandIdentity {
        &self.identity
    }

    pub fn compact(&self) -> String {
        format!(
            "{}#{}@{}",
            self.boundary_id.compact(),
            self.generation,
            self.identity.compact()
        )
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"boundary_id\":{},\"generation\":{},\"identity\":{}}}",
            self.boundary_id.render_json(),
            self.generation,
            self.identity.render_json(),
        )
    }
}


#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum JetWebHydrationTrigger {
    Immediate,
    Load,
    Idle,
    Visible,
    Manual,
}

impl JetWebHydrationTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Immediate => "immediate",
            Self::Load => "client-load",
            Self::Idle => "client-idle",
            Self::Visible => "client-visible",
            Self::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "immediate" => Some(Self::Immediate),
            "client-load" => Some(Self::Load),
            "client-idle" => Some(Self::Idle),
            "client-visible" => Some(Self::Visible),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

/// Exact integer hydration timing.  No wall-clock conversion or floating
/// duration is performed in this kernel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetWebHydrationTiming {
    pub trigger: JetWebHydrationTrigger,
    pub started_at_ns: u64,
    pub completed_at_ns: Option<u64>,
}

impl JetWebHydrationTiming {
    pub const fn started(started_at_ns: u64) -> Self {
        Self::started_with_trigger(JetWebHydrationTrigger::Manual, started_at_ns)
    }

    pub const fn started_with_trigger(
        trigger: JetWebHydrationTrigger,
        started_at_ns: u64,
    ) -> Self {
        Self {
            trigger,
            started_at_ns,
            completed_at_ns: None,
        }
    }

    pub fn new(
        started_at_ns: u64,
        completed_at_ns: Option<u64>,
    ) -> Result<Self, JetWebPendingError> {
        Self::new_with_trigger(
            JetWebHydrationTrigger::Manual,
            started_at_ns,
            completed_at_ns,
        )
    }

    pub fn new_with_trigger(
        trigger: JetWebHydrationTrigger,
        started_at_ns: u64,
        completed_at_ns: Option<u64>,
    ) -> Result<Self, JetWebPendingError> {
        let timing = Self {
            trigger,
            started_at_ns,
            completed_at_ns,
        };
        timing.validate()?;
        Ok(timing)
    }

    pub fn complete(&mut self, completed_at_ns: u64) -> Result<(), JetWebPendingError> {
        if completed_at_ns < self.started_at_ns {
            return Err(JetWebPendingError::InvalidHydrationTiming {
                started_at_ns: self.started_at_ns,
                completed_at_ns,
            });
        }
        if self.completed_at_ns.is_some() {
            return Err(JetWebPendingError::InvalidLifecycle {
                state: JetWebPendingState::Ready,
                operation: "hydration_complete",
            });
        }
        self.completed_at_ns = Some(completed_at_ns);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), JetWebPendingError> {
        if let Some(completed_at_ns) = self.completed_at_ns {
            if completed_at_ns < self.started_at_ns {
                return Err(JetWebPendingError::InvalidHydrationTiming {
                    started_at_ns: self.started_at_ns,
                    completed_at_ns,
                });
            }
        }
        Ok(())
    }

    pub const fn is_complete(self) -> bool {
        self.completed_at_ns.is_some()
    }

    pub const fn duration_ns(self) -> Option<u64> {
        match self.completed_at_ns {
            Some(completed_at_ns) => Some(completed_at_ns - self.started_at_ns),
            None => None,
        }
    }

    pub fn render_json(&self) -> String {
        let completed = self
            .completed_at_ns
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"started_at_ns\":{},\"completed_at_ns\":{},\"duration_ns\":{},\"trigger\":{}}}",
            self.started_at_ns,
            completed,
            self.duration_ns()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
            jet_web_pending_json_string(self.trigger.as_str()),
        )
    }
}

/// A single typed stream chunk. Sequence zero is the first chunk; the boundary
/// requires every later sequence to be exactly the previous sequence plus one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebPendingChunk {
    pub sequence: u64,
    pub content: String,
    pub is_final: bool,
    pub identity: JetWebIslandIdentity,
}

impl JetWebPendingChunk {
    pub fn new(
        sequence: u64,
        content: impl Into<String>,
        is_final: bool,
        identity: JetWebIslandIdentity,
    ) -> Result<Self, JetWebPendingError> {
        let chunk = Self {
            sequence,
            content: content.into(),
            is_final,
            identity,
        };
        chunk.validate()?;
        Ok(chunk)
    }

    pub fn validate(&self) -> Result<(), JetWebPendingError> {
        self.identity.validate()?;
        if self.content.len() > JET_WEB_PENDING_MAX_BUFFER_BYTES {
            return Err(JetWebPendingError::ContentTooLarge {
                limit: JET_WEB_PENDING_MAX_BUFFER_BYTES,
                actual: self.content.len(),
            });
        }
        Ok(())
    }

    pub fn byte_len(&self) -> usize {
        self.content.len()
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"sequence\":{},\"content\":{},\"is_final\":{},\"identity\":{}}}",
            self.sequence,
            jet_web_pending_json_string(&self.content),
            self.is_final,
            self.identity.render_json(),
        )
    }
}

/// A durable transaction fact.  Chunk content is represented by the separate
/// `JetWebPendingChunk`; this record remains bounded even for large streams.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebPendingTransaction {
    pub sequence: u64,
    pub boundary_id: String,
    pub kind: JetWebPendingTransactionKind,
    pub at_ns: u64,
    pub state: JetWebPendingState,
    pub identity: JetWebIslandIdentity,
    pub chunk_sequence: Option<u64>,
    pub error: Option<JetWebPendingError>,
}

impl JetWebPendingTransaction {
    pub fn render_json(&self) -> String {
        let chunk_sequence = self
            .chunk_sequence
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string());
        let error = self
            .error
            .as_ref()
            .map(JetWebPendingError::render_json)
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"sequence\":{},\"boundary_id\":{},\"kind\":{},\"at_ns\":{},\"state\":{},\"identity\":{},\"chunk_sequence\":{},\"error\":{}}}",
            self.sequence,
            jet_web_pending_json_string(&self.boundary_id),
            jet_web_pending_json_string(self.kind.as_str()),
            self.at_ns,
            jet_web_pending_json_string(self.state.as_str()),
            self.identity.render_json(),
            chunk_sequence,
            error,
        )
    }
}

impl JetWebPendingError {
    pub fn render_json(&self) -> String {
        format!(
            "{{\"code\":{},\"message\":{}}}",
            jet_web_pending_json_string(self.code()),
            jet_web_pending_json_string(&self.message()),
        )
    }
}

/// One bounded observation of a transaction, including buffer policy facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebPendingFact {
    pub transaction: JetWebPendingTransaction,
    pub buffered_chunks: usize,
    pub buffered_bytes: usize,
    pub committed_chunks: usize,
    pub committed_bytes: usize,
    pub expected_chunk_sequence: u64,
    pub final_chunk_seen: bool,
    pub truncated: bool,
    pub backpressure: bool,
    pub cancelled: bool,
    pub rolled_back: bool,
}

impl JetWebPendingFact {
    pub fn render_json(&self) -> String {
        format!(
            "{{\"transaction\":{},\"buffered_chunks\":{},\"buffered_bytes\":{},\"committed_chunks\":{},\"committed_bytes\":{},\"expected_chunk_sequence\":{},\"final_chunk_seen\":{},\"truncated\":{},\"backpressure\":{},\"cancelled\":{},\"rolled_back\":{}}}",
            self.transaction.render_json(),
            self.buffered_chunks,
            self.buffered_bytes,
            self.committed_chunks,
            self.committed_bytes,
            self.expected_chunk_sequence,
            self.final_chunk_seen,
            self.truncated,
            self.backpressure,
            self.cancelled,
            self.rolled_back,
        )
    }
}

/// A receipt is the host-facing result of one transaction.  It is a snapshot,
/// not a live view, so later chunks cannot mutate an earlier observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebPendingReceipt {
    pub transaction_sequence: u64,
    pub boundary_id: String,
    pub operation: JetWebPendingTransactionKind,
    pub state: JetWebPendingState,
    pub at_ns: u64,
    pub identity: JetWebIslandIdentity,
    pub chunk_sequence: Option<u64>,
    pub expected_chunk_sequence: u64,
    pub buffered_chunks: usize,
    pub buffered_bytes: usize,
    pub committed_chunks: usize,
    pub committed_bytes: usize,
    pub final_chunk_seen: bool,
    pub content: String,
    pub stream_content: String,
    pub hydration: Option<JetWebHydrationTiming>,
    pub failure: Option<JetWebPendingError>,
    pub truncated: bool,
    pub backpressure: bool,
    pub cancelled: bool,
    pub rolled_back: bool,
}

impl JetWebPendingReceipt {
    pub const fn is_pending(&self) -> bool {
        self.state.is_pending()
    }

    pub const fn is_ready(&self) -> bool {
        self.state.is_ready()
    }

    pub const fn is_failed(&self) -> bool {
        self.state.is_failed()
    }

    pub const fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    pub const fn was_rolled_back(&self) -> bool {
        self.rolled_back
    }

    pub fn render_json(&self) -> String {
        let chunk_sequence = self
            .chunk_sequence
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string());
        let hydration = self
            .hydration
            .as_ref()
            .map(JetWebHydrationTiming::render_json)
            .unwrap_or_else(|| "null".to_string());
        let failure = self
            .failure
            .as_ref()
            .map(JetWebPendingError::render_json)
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"transaction_sequence\":{},\"boundary_id\":{},\"operation\":{},\"state\":{},\"at_ns\":{},\"identity\":{},\"chunk_sequence\":{},\"expected_chunk_sequence\":{},\"buffered_chunks\":{},\"buffered_bytes\":{},\"committed_chunks\":{},\"committed_bytes\":{},\"final_chunk_seen\":{},\"content\":{},\"stream_content\":{},\"hydration\":{},\"failure\":{},\"truncated\":{},\"backpressure\":{},\"cancelled\":{},\"rolled_back\":{}}}",
            self.transaction_sequence,
            jet_web_pending_json_string(&self.boundary_id),
            jet_web_pending_json_string(self.operation.as_str()),
            jet_web_pending_json_string(self.state.as_str()),
            self.at_ns,
            self.identity.render_json(),
            chunk_sequence,
            self.expected_chunk_sequence,
            self.buffered_chunks,
            self.buffered_bytes,
            self.committed_chunks,
            self.committed_bytes,
            self.final_chunk_seen,
            jet_web_pending_json_string(&self.content),
            jet_web_pending_json_string(&self.stream_content),
            hydration,
            failure,
            self.truncated,
            self.backpressure,
            self.cancelled,
            self.rolled_back,
        )
    }
}

/// Pure server/client projection.  Both targets receive the same committed and
/// staged content and typed identity; `target` only tells a later adapter which
/// side is consuming the facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebPendingProjection {
    pub target: JetWebPendingProjectionTarget,
    pub boundary_id: String,
    pub identity: JetWebIslandIdentity,
    pub state: JetWebPendingState,
    pub committed_content: String,
    pub stream_content: String,
    pub buffered_chunks: usize,
    pub buffered_bytes: usize,
    pub committed_chunks: usize,
    pub committed_bytes: usize,
    pub expected_chunk_sequence: u64,
    pub final_chunk_seen: bool,
    pub hydration: Option<JetWebHydrationTiming>,
    pub failure: Option<JetWebPendingError>,
    pub truncated: bool,
    pub backpressure: bool,
    pub cancelled: bool,
    pub rolled_back: bool,
}

impl JetWebPendingProjection {
    pub fn content(&self) -> &str {
        &self.committed_content
    }

    pub fn pending_content(&self) -> Option<&str> {
        self.state
            .is_pending()
            .then_some(self.stream_content.as_str())
    }

    pub fn render_json(&self) -> String {
        let hydration = self
            .hydration
            .as_ref()
            .map(JetWebHydrationTiming::render_json)
            .unwrap_or_else(|| "null".to_string());
        let failure = self
            .failure
            .as_ref()
            .map(JetWebPendingError::render_json)
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"target\":{},\"boundary_id\":{},\"identity\":{},\"state\":{},\"committed_content\":{},\"stream_content\":{},\"buffered_chunks\":{},\"buffered_bytes\":{},\"committed_chunks\":{},\"committed_bytes\":{},\"expected_chunk_sequence\":{},\"final_chunk_seen\":{},\"hydration\":{},\"failure\":{},\"truncated\":{},\"backpressure\":{},\"cancelled\":{},\"rolled_back\":{}}}",
            jet_web_pending_json_string(self.target.as_str()),
            jet_web_pending_json_string(&self.boundary_id),
            self.identity.render_json(),
            jet_web_pending_json_string(self.state.as_str()),
            jet_web_pending_json_string(&self.committed_content),
            jet_web_pending_json_string(&self.stream_content),
            self.buffered_chunks,
            self.buffered_bytes,
            self.committed_chunks,
            self.committed_bytes,
            self.expected_chunk_sequence,
            self.final_chunk_seen,
            hydration,
            failure,
            self.truncated,
            self.backpressure,
            self.cancelled,
            self.rolled_back,
        )
    }
}

/// Typed streaming boundary and island state.  Staged content is never
/// visible through `content()` until `commit` succeeds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebStreamingBoundary {
    boundary_id: String,
    identity: JetWebIslandIdentity,
    state: JetWebPendingState,
    committed_content: String,
    staged_content: String,
    committed_chunks: usize,
    committed_bytes: usize,
    staged_chunks: usize,
    staged_bytes: usize,
    expected_chunk_sequence: u64,
    final_chunk_seen: bool,
    has_committed_content: bool,
    committed_hydration_identity: Option<JetWebIslandIdentity>,
    committed_hydration: Option<JetWebHydrationTiming>,
    hydration_identity: Option<JetWebIslandIdentity>,
    hydration: Option<JetWebHydrationTiming>,
    last_error: Option<JetWebPendingError>,
    last_operation: JetWebPendingTransactionKind,
    last_at_ns: u64,
    last_cancelled: bool,
    last_rolled_back: bool,
    last_backpressure: bool,
    max_chunks: usize,
    max_buffer_bytes: usize,
    max_facts: usize,
    facts: std::collections::VecDeque<JetWebPendingFact>,
    history_truncated: bool,
    next_transaction_sequence: u64,
}

impl JetWebStreamingBoundary {
    pub fn new(
        boundary_id: impl Into<String>,
        identity: JetWebIslandIdentity,
    ) -> Result<Self, JetWebPendingError> {
        Self::with_limits(
            boundary_id,
            identity,
            JET_WEB_PENDING_MAX_CHUNKS,
            JET_WEB_PENDING_MAX_BUFFER_BYTES,
        )
    }

    pub fn with_limits(
        boundary_id: impl Into<String>,
        identity: JetWebIslandIdentity,
        max_chunks: usize,
        max_buffer_bytes: usize,
    ) -> Result<Self, JetWebPendingError> {
        Self::with_capacities(
            boundary_id,
            identity,
            max_chunks,
            max_buffer_bytes,
            JET_WEB_PENDING_MAX_FACTS,
        )
    }

    pub fn with_capacities(
        boundary_id: impl Into<String>,
        identity: JetWebIslandIdentity,
        max_chunks: usize,
        max_buffer_bytes: usize,
        max_facts: usize,
    ) -> Result<Self, JetWebPendingError> {
        let boundary_id = boundary_id.into();
        jet_web_pending_validate_boundary(&boundary_id, "boundary_id")?;
        identity.validate()?;
        jet_web_pending_validate_limit(
            max_chunks,
            JET_WEB_PENDING_MAX_CHUNKS,
            "chunk",
        )?;
        jet_web_pending_validate_limit(
            max_buffer_bytes,
            JET_WEB_PENDING_MAX_BUFFER_BYTES,
            "buffer byte",
        )?;
        jet_web_pending_validate_limit(max_facts, JET_WEB_PENDING_MAX_FACTS, "fact")?;
        Ok(Self {
            boundary_id,
            identity,
            state: JetWebPendingState::Pending,
            committed_content: String::new(),
            staged_content: String::new(),
            committed_chunks: 0,
            committed_bytes: 0,
            staged_chunks: 0,
            staged_bytes: 0,
            expected_chunk_sequence: 0,
            final_chunk_seen: false,
            has_committed_content: false,
            committed_hydration_identity: None,
            committed_hydration: None,
            hydration_identity: None,
            hydration: None,
            last_error: None,
            last_operation: JetWebPendingTransactionKind::Begin,
            last_at_ns: 0,
            last_cancelled: false,
            last_rolled_back: false,
            last_backpressure: false,
            max_chunks,
            max_buffer_bytes,
            max_facts,
            facts: std::collections::VecDeque::new(),
            history_truncated: false,
            next_transaction_sequence: 1,
        })
    }

    pub fn boundary_id(&self) -> &str {
        &self.boundary_id
    }

    pub fn identity(&self) -> &JetWebIslandIdentity {
        &self.identity
    }

    pub fn state(&self) -> JetWebPendingState {
        self.state
    }

    pub fn is_pending(&self) -> bool {
        self.state.is_pending()
    }

    pub fn is_ready(&self) -> bool {
        self.state.is_ready()
    }

    pub fn is_failed(&self) -> bool {
        self.state.is_failed()
    }

    /// Last committed page/world.  Pending and failed updates never replace it.
    pub fn content(&self) -> &str {
        &self.committed_content
    }

    pub fn committed_content(&self) -> &str {
        &self.committed_content
    }

    pub fn stream_content(&self) -> &str {
        &self.staged_content
    }

    pub fn pending_content(&self) -> Option<&str> {
        self.state
            .is_pending()
            .then_some(self.staged_content.as_str())
    }

    pub fn buffered_chunks(&self) -> usize {
        self.staged_chunks
    }

    pub fn buffered_bytes(&self) -> usize {
        self.staged_bytes
    }

    pub fn committed_chunks(&self) -> usize {
        self.committed_chunks
    }

    pub fn committed_bytes(&self) -> usize {
        self.committed_bytes
    }

    pub fn expected_chunk_sequence(&self) -> u64 {
        self.expected_chunk_sequence
    }

    pub fn final_chunk_seen(&self) -> bool {
        self.final_chunk_seen
    }

    pub fn max_chunks(&self) -> usize {
        self.max_chunks
    }

    pub fn max_buffer_bytes(&self) -> usize {
        self.max_buffer_bytes
    }

    pub fn history_truncated(&self) -> bool {
        self.history_truncated
    }

    pub fn facts(&self) -> Vec<JetWebPendingFact> {
        self.facts.iter().cloned().collect()
    }

    pub fn transactions(&self) -> Vec<JetWebPendingTransaction> {
        self.facts
            .iter()
            .map(|fact| fact.transaction.clone())
            .collect()
    }

    pub fn fact_count(&self) -> usize {
        self.facts.len()
    }

    pub fn last_error(&self) -> Option<&JetWebPendingError> {
        self.last_error.as_ref()
    }

    pub fn hydration(&self) -> Option<JetWebHydrationTiming> {
        self.hydration
    }

    pub fn hydration_trigger(&self) -> Option<JetWebHydrationTrigger> {
        self.hydration.map(|timing| timing.trigger)
    }

    pub fn receipt(&self) -> JetWebPendingReceipt {
        self.snapshot(
            self.last_operation,
            self.last_at_ns,
            None,
            self.last_cancelled,
            self.last_rolled_back,
            self.last_backpressure,
        )
    }

    /// Begin a stream for the boundary's exact source/build/revision identity.
    /// A new stream clears only staged data; the committed page remains intact.
    pub fn begin(
        &mut self,
        identity: JetWebIslandIdentity,
        at_ns: u64,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.check_timestamp(at_ns)?;
        if identity != self.identity {
            let error = JetWebPendingError::IdentityMismatch {
                expected: self.identity.clone(),
                actual: identity,
            };
            return self.reject(error, at_ns);
        }
        if self.state.is_pending() && (self.staged_chunks != 0 || !self.staged_content.is_empty()) {
            let error = JetWebPendingError::InvalidLifecycle {
                state: self.state,
                operation: "begin",
            };
            return self.reject(error, at_ns);
        }
        self.begin_locked(at_ns);
        self.record(
            JetWebPendingTransactionKind::Begin,
            at_ns,
            None,
            None,
            false,
            false,
        )
    }

    /// Begin without repeating identity when the boundary has already been
    /// checked by the caller.  The identity-bearing `begin` remains the trust
    /// boundary used by adapters.
    pub fn begin_at(&mut self, at_ns: u64) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.begin(self.identity.clone(), at_ns)
    }

    fn begin_locked(&mut self, at_ns: u64) {
        self.state = JetWebPendingState::Pending;
        self.staged_content.clear();
        self.staged_chunks = 0;
        self.staged_bytes = 0;
        self.expected_chunk_sequence = 0;
        self.final_chunk_seen = false;
        self.hydration_identity = None;
        self.hydration = None;
        self.last_error = None;
        self.last_cancelled = false;
        self.last_rolled_back = false;
        self.last_backpressure = false;
        self.last_at_ns = at_ns;
    }

    pub fn push_chunk(
        &mut self,
        chunk: JetWebPendingChunk,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.push_chunk_at(chunk, self.last_at_ns)
    }

    pub fn push_chunk_at(
        &mut self,
        chunk: JetWebPendingChunk,
        at_ns: u64,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.check_timestamp(at_ns)?;
        if !self.state.is_pending() {
            return self.reject(
                JetWebPendingError::InvalidLifecycle {
                    state: self.state,
                    operation: "chunk",
                },
                at_ns,
            );
        }
        if chunk.identity != self.identity {
            return self.reject(
                JetWebPendingError::IdentityMismatch {
                    expected: self.identity.clone(),
                    actual: chunk.identity,
                },
                at_ns,
            );
        }
        if let Err(error) = chunk.validate() {
            return self.reject(error, at_ns);
        }
        if chunk.sequence != self.expected_chunk_sequence {
            return self.reject(
                JetWebPendingError::InvalidChunkSequence {
                    expected: self.expected_chunk_sequence,
                    actual: chunk.sequence,
                },
                at_ns,
            );
        }
        if self.final_chunk_seen {
            return self.reject(
                JetWebPendingError::InvalidLifecycle {
                    state: self.state,
                    operation: "chunk_after_final",
                },
                at_ns,
            );
        }
        if self.staged_chunks >= self.max_chunks {
            let error = JetWebPendingError::Backpressure {
                dimension: JetWebPendingBackpressureDimension::Chunks,
                limit: self.max_chunks,
                buffered: self.staged_chunks,
                incoming: 1,
            };
            return self.reject_with_backpressure(error, at_ns);
        }
        if chunk.byte_len() > self.max_buffer_bytes.saturating_sub(self.staged_bytes) {
            let error = JetWebPendingError::Backpressure {
                dimension: JetWebPendingBackpressureDimension::Bytes,
                limit: self.max_buffer_bytes,
                buffered: self.staged_bytes,
                incoming: chunk.byte_len(),
            };
            return self.reject_with_backpressure(error, at_ns);
        }
        self.staged_content.push_str(&chunk.content);
        self.staged_chunks += 1;
        self.staged_bytes += chunk.byte_len();
        self.expected_chunk_sequence = match self.expected_chunk_sequence.checked_add(1) {
            Some(value) => value,
            None => {
                let error = JetWebPendingError::SequenceExhausted { kind: "chunk" };
                return self.reject(error, at_ns);
            }
        };
        self.final_chunk_seen = chunk.is_final;
        self.last_error = None;
        self.last_cancelled = false;
        self.last_rolled_back = false;
        self.last_backpressure = false;
        self.last_at_ns = at_ns;
        self.record(
            JetWebPendingTransactionKind::Chunk,
            at_ns,
            Some(chunk.sequence),
            None,
            false,
            false,
        )
    }

    pub fn append(
        &mut self,
        sequence: u64,
        content: impl Into<String>,
        is_final: bool,
        at_ns: u64,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        let chunk = JetWebPendingChunk::new(sequence, content, is_final, self.identity.clone())?;
        self.push_chunk_at(chunk, at_ns)
    }

    pub fn commit(&mut self, at_ns: u64) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.check_timestamp(at_ns)?;
        if !self.state.is_pending() {
            return self.reject(
                JetWebPendingError::InvalidLifecycle {
                    state: self.state,
                    operation: "commit",
                },
                at_ns,
            );
        }
        if !self.final_chunk_seen {
            return self.reject(JetWebPendingError::MissingFinalChunk, at_ns);
        }
        self.committed_content.clone_from(&self.staged_content);
        self.committed_chunks = self.staged_chunks;
        self.committed_bytes = self.staged_bytes;
        self.has_committed_content = true;
        self.committed_hydration_identity = None;
        self.committed_hydration = None;
        self.state = JetWebPendingState::Ready;
        self.hydration_identity = None;
        self.hydration = None;
        self.last_error = None;
        self.last_cancelled = false;
        self.last_rolled_back = false;
        self.last_backpressure = false;
        self.last_at_ns = at_ns;
        self.record(
            JetWebPendingTransactionKind::Commit,
            at_ns,
            None,
            None,
            false,
            false,
        )
    }

    /// Publish a typed failure and restore the last committed content.
    pub fn fail(
        &mut self,
        error: JetWebPendingError,
        at_ns: u64,
    ) -> JetWebPendingReceipt {
        let at_ns = self.monotonic_timestamp(at_ns);
        self.fail_locked(error.clone(), at_ns, false, false);
        self.record_unchecked(
            JetWebPendingTransactionKind::Fail,
            at_ns,
            None,
            Some(error),
            false,
            true,
        )
    }

    /// Cancel an in-flight stream.  The prior committed state is restored; if
    /// there is no committed page yet, the boundary remains visibly pending.
    pub fn cancel(&mut self, at_ns: u64) -> JetWebPendingReceipt {
        let at_ns = self.monotonic_timestamp(at_ns);
        if !self.state.is_pending() {
            let error = JetWebPendingError::InvalidLifecycle {
                state: self.state,
                operation: "cancel",
            };
            self.fail_locked(error.clone(), at_ns, true, true);
            return self.record_unchecked(
                JetWebPendingTransactionKind::Cancel,
                at_ns,
                None,
                Some(error),
                true,
                true,
            );
        }
        self.rollback_locked();
        self.state = if self.has_committed_content {
            JetWebPendingState::Ready
        } else {
            JetWebPendingState::Pending
        };
        self.last_error = None;
        self.last_cancelled = true;
        self.last_rolled_back = true;
        self.last_backpressure = false;
        self.last_at_ns = at_ns;
        self.record_unchecked(
            JetWebPendingTransactionKind::Cancel,
            at_ns,
            None,
            None,
            true,
            true,
        )
    }

    /// Explicit rollback has the same content guarantee as cancellation but
    /// remains distinguishable in the transaction stream.
    pub fn rollback(&mut self, at_ns: u64) -> JetWebPendingReceipt {
        let at_ns = self.monotonic_timestamp(at_ns);
        if !self.state.is_pending() {
            let error = JetWebPendingError::InvalidLifecycle {
                state: self.state,
                operation: "rollback",
            };
            self.fail_locked(error.clone(), at_ns, false, true);
            return self.record_unchecked(
                JetWebPendingTransactionKind::Rollback,
                at_ns,
                None,
                Some(error),
                false,
                true,
            );
        }
        self.rollback_locked();
        self.state = if self.has_committed_content {
            JetWebPendingState::Ready
        } else {
            JetWebPendingState::Pending
        };
        self.last_error = None;
        self.last_cancelled = false;
        self.last_rolled_back = true;
        self.last_backpressure = false;
        self.last_at_ns = at_ns;
        self.record_unchecked(
            JetWebPendingTransactionKind::Rollback,
            at_ns,
            None,
            None,
            false,
            true,
        )
    }

    pub fn fail_for(
        &mut self,
        identity: JetWebIslandIdentity,
        error: JetWebPendingError,
        at_ns: u64,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.require_identity(identity)?;
        Ok(self.fail(error, at_ns))
    }

    pub fn cancel_for(
        &mut self,
        identity: JetWebIslandIdentity,
        at_ns: u64,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.require_identity(identity)?;
        Ok(self.cancel(at_ns))
    }

    pub fn rollback_for(
        &mut self,
        identity: JetWebIslandIdentity,
        at_ns: u64,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.require_identity(identity)?;
        Ok(self.rollback(at_ns))
    }

    fn require_identity(
        &self,
        identity: JetWebIslandIdentity,
    ) -> Result<(), JetWebPendingError> {
        if identity == self.identity {
            Ok(())
        } else {
            Err(JetWebPendingError::IdentityMismatch {
                expected: self.identity.clone(),
                actual: identity,
            })
        }
    }

    pub fn hydration_start(
        &mut self,
        identity: JetWebIslandIdentity,
        started_at_ns: u64,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.hydration_start_with_trigger(
            identity,
            JetWebHydrationTrigger::Manual,
            started_at_ns,
        )
    }

    pub fn hydration_start_with_trigger(
        &mut self,
        identity: JetWebIslandIdentity,
        trigger: JetWebHydrationTrigger,
        started_at_ns: u64,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.check_timestamp(started_at_ns)?;
        if identity != self.identity {
            return self.reject(
                JetWebPendingError::HydrationIdentityMismatch {
                    expected: self.identity.clone(),
                    actual: identity,
                },
                started_at_ns,
            );
        }
        if !self.state.is_ready() {
            return self.reject(
                JetWebPendingError::InvalidLifecycle {
                    state: self.state,
                    operation: "hydration_start",
                },
                started_at_ns,
            );
        }
        if self.hydration.is_some() {
            return self.reject(
                JetWebPendingError::InvalidLifecycle {
                    state: self.state,
                    operation: "hydration_start_twice",
                },
                started_at_ns,
            );
        }
        self.hydration_identity = Some(identity);
        self.hydration = Some(JetWebHydrationTiming::started_with_trigger(
            trigger,
            started_at_ns,
        ));
        self.last_error = None;
        self.last_cancelled = false;
        self.last_rolled_back = false;
        self.last_backpressure = false;
        self.last_at_ns = started_at_ns;
        self.record(
            JetWebPendingTransactionKind::HydrationStart,
            started_at_ns,
            None,
            None,
            false,
            false,
        )
    }

    pub fn hydration_complete(
        &mut self,
        identity: JetWebIslandIdentity,
        completed_at_ns: u64,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.check_timestamp(completed_at_ns)?;
        if identity != self.identity {
            return self.reject(
                JetWebPendingError::HydrationIdentityMismatch {
                    expected: self.identity.clone(),
                    actual: identity,
                },
                completed_at_ns,
            );
        }
        if !self.state.is_ready() {
            return self.reject(
                JetWebPendingError::InvalidLifecycle {
                    state: self.state,
                    operation: "hydration_complete",
                },
                completed_at_ns,
            );
        }
        if self.hydration_identity.as_ref() != Some(&identity) {
            return self.reject(
                JetWebPendingError::InvalidLifecycle {
                    state: self.state,
                    operation: "hydration_complete_without_start",
                },
                completed_at_ns,
            );
        }
        let Some(mut timing) = self.hydration else {
            return self.reject(
                JetWebPendingError::InvalidLifecycle {
                    state: self.state,
                    operation: "hydration_complete_without_start",
                },
                completed_at_ns,
            );
        };
        if let Err(error) = timing.complete(completed_at_ns) {
            return self.reject(error, completed_at_ns);
        }
        self.hydration = Some(timing);
        self.committed_hydration_identity = Some(identity);
        self.committed_hydration = Some(timing);
        self.last_error = None;
        self.last_cancelled = false;
        self.last_rolled_back = false;
        self.last_backpressure = false;
        self.last_at_ns = completed_at_ns;
        self.record(
            JetWebPendingTransactionKind::HydrationComplete,
            completed_at_ns,
            None,
            None,
            false,
            false,
        )
    }

    pub fn project(&self, target: JetWebPendingProjectionTarget) -> JetWebPendingProjection {
        JetWebPendingProjection {
            target,
            boundary_id: self.boundary_id.clone(),
            identity: self.identity.clone(),
            state: self.state,
            committed_content: self.committed_content.clone(),
            stream_content: self.staged_content.clone(),
            buffered_chunks: self.staged_chunks,
            buffered_bytes: self.staged_bytes,
            committed_chunks: self.committed_chunks,
            committed_bytes: self.committed_bytes,
            expected_chunk_sequence: self.expected_chunk_sequence,
            final_chunk_seen: self.final_chunk_seen,
            hydration: self.hydration,
            failure: self.last_error.clone(),
            truncated: self.history_truncated,
            backpressure: self.last_backpressure,
            cancelled: self.last_cancelled,
            rolled_back: self.last_rolled_back,
        }
    }

    pub fn server_projection(&self) -> JetWebPendingProjection {
        self.project(JetWebPendingProjectionTarget::Server)
    }

    pub fn client_projection(&self) -> JetWebPendingProjection {
        self.project(JetWebPendingProjectionTarget::Client)
    }

    pub fn render_json(&self) -> String {
        let facts = self
            .facts
            .iter()
            .map(JetWebPendingFact::render_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"boundary_id\":{},\"identity\":{},\"state\":{},\"content\":{},\"stream_content\":{},\"buffered_chunks\":{},\"buffered_bytes\":{},\"committed_chunks\":{},\"committed_bytes\":{},\"expected_chunk_sequence\":{},\"final_chunk_seen\":{},\"hydration\":{},\"failure\":{},\"truncated\":{},\"backpressure\":{},\"cancelled\":{},\"rolled_back\":{},\"facts\":[{}]}}",
            jet_web_pending_json_string(&self.boundary_id),
            self.identity.render_json(),
            jet_web_pending_json_string(self.state.as_str()),
            jet_web_pending_json_string(&self.committed_content),
            jet_web_pending_json_string(&self.staged_content),
            self.staged_chunks,
            self.staged_bytes,
            self.committed_chunks,
            self.committed_bytes,
            self.expected_chunk_sequence,
            self.final_chunk_seen,
            self.hydration
                .as_ref()
                .map(JetWebHydrationTiming::render_json)
                .unwrap_or_else(|| "null".to_string()),
            self.last_error
                .as_ref()
                .map(JetWebPendingError::render_json)
                .unwrap_or_else(|| "null".to_string()),
            self.history_truncated,
            self.last_backpressure,
            self.last_cancelled,
            self.last_rolled_back,
            facts,
        )
    }

    fn monotonic_timestamp(&self, at_ns: u64) -> u64 {
        at_ns.max(self.last_at_ns)
    }

    fn check_timestamp(&mut self, at_ns: u64) -> Result<(), JetWebPendingError> {
        if at_ns < self.last_at_ns {
            let error = JetWebPendingError::NonMonotonicTimestamp {
                previous: self.last_at_ns,
                actual: at_ns,
            };
            self.fail_locked(error.clone(), self.last_at_ns, false, true);
            return Err(error);
        }
        Ok(())
    }

    fn rollback_locked(&mut self) {
        self.staged_content.clone_from(&self.committed_content);
        self.staged_chunks = self.committed_chunks;
        self.staged_bytes = self.committed_bytes;
        self.expected_chunk_sequence = self.committed_chunks as u64;
        self.final_chunk_seen = self.has_committed_content;
        self.hydration_identity = self.committed_hydration_identity.clone();
        self.hydration = self.committed_hydration;
    }

    fn fail_locked(
        &mut self,
        error: JetWebPendingError,
        at_ns: u64,
        cancelled: bool,
        rolled_back: bool,
    ) {
        self.rollback_locked();
        self.state = JetWebPendingState::Failed;
        self.last_error = Some(error);
        self.last_cancelled = cancelled;
        self.last_rolled_back = rolled_back;
        self.last_backpressure = false;
        self.last_at_ns = at_ns.max(self.last_at_ns);
    }

    fn reject(
        &mut self,
        error: JetWebPendingError,
        at_ns: u64,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.fail_locked(error.clone(), at_ns, false, true);
        let receipt = self.record_unchecked(
            JetWebPendingTransactionKind::Reject,
            self.last_at_ns,
            None,
            Some(error.clone()),
            false,
            true,
        );
        let _ = receipt;
        Err(error)
    }

    fn reject_with_backpressure(
        &mut self,
        error: JetWebPendingError,
        at_ns: u64,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        self.fail_locked(error.clone(), at_ns, false, true);
        self.last_backpressure = true;
        let receipt = self.record_unchecked(
            JetWebPendingTransactionKind::Reject,
            self.last_at_ns,
            None,
            Some(error.clone()),
            false,
            true,
        );
        let _ = receipt;
        Err(error)
    }

    fn record(
        &mut self,
        kind: JetWebPendingTransactionKind,
        at_ns: u64,
        chunk_sequence: Option<u64>,
        error: Option<JetWebPendingError>,
        cancelled: bool,
        rolled_back: bool,
    ) -> Result<JetWebPendingReceipt, JetWebPendingError> {
        if self.next_transaction_sequence == u64::MAX {
            let exhaustion = JetWebPendingError::SequenceExhausted {
                kind: "transaction",
            };
            self.fail_locked(exhaustion.clone(), at_ns, false, true);
            return Err(exhaustion);
        }
        Ok(self.record_unchecked(
            kind,
            at_ns,
            chunk_sequence,
            error,
            cancelled,
            rolled_back,
        ))
    }

    fn record_unchecked(
        &mut self,
        kind: JetWebPendingTransactionKind,
        at_ns: u64,
        chunk_sequence: Option<u64>,
        error: Option<JetWebPendingError>,
        cancelled: bool,
        rolled_back: bool,
    ) -> JetWebPendingReceipt {
        let at_ns = at_ns.max(self.last_at_ns);
        let sequence = self.next_transaction_sequence;
        self.next_transaction_sequence = self.next_transaction_sequence.saturating_add(1);
        let transaction = JetWebPendingTransaction {
            sequence,
            boundary_id: self.boundary_id.clone(),
            kind,
            at_ns,
            state: self.state,
            identity: self.identity.clone(),
            chunk_sequence,
            error: error.clone(),
        };
        if self.facts.len() == self.max_facts {
            self.facts.pop_front();
            self.history_truncated = true;
        }
        let fact = JetWebPendingFact {
            transaction: transaction.clone(),
            buffered_chunks: self.staged_chunks,
            buffered_bytes: self.staged_bytes,
            committed_chunks: self.committed_chunks,
            committed_bytes: self.committed_bytes,
            expected_chunk_sequence: self.expected_chunk_sequence,
            final_chunk_seen: self.final_chunk_seen,
            truncated: self.history_truncated,
            backpressure: self.last_backpressure,
            cancelled,
            rolled_back,
        };
        self.facts.push_back(fact);
        self.last_operation = kind;
        self.last_at_ns = at_ns.max(self.last_at_ns);
        self.last_cancelled = cancelled;
        self.last_rolled_back = rolled_back;
        self.last_error = error;
        self.snapshot(
            kind,
            self.last_at_ns,
            chunk_sequence,
            cancelled,
            rolled_back,
            self.last_backpressure,
        )
    }

    fn snapshot(
        &self,
        operation: JetWebPendingTransactionKind,
        at_ns: u64,
        chunk_sequence: Option<u64>,
        cancelled: bool,
        rolled_back: bool,
        backpressure: bool,
    ) -> JetWebPendingReceipt {
        JetWebPendingReceipt {
            transaction_sequence: self.next_transaction_sequence.saturating_sub(1),
            boundary_id: self.boundary_id.clone(),
            operation,
            state: self.state,
            at_ns,
            identity: self.identity.clone(),
            chunk_sequence,
            expected_chunk_sequence: self.expected_chunk_sequence,
            buffered_chunks: self.staged_chunks,
            buffered_bytes: self.staged_bytes,
            committed_chunks: self.committed_chunks,
            committed_bytes: self.committed_bytes,
            final_chunk_seen: self.final_chunk_seen,
            content: self.committed_content.clone(),
            stream_content: self.staged_content.clone(),
            hydration: self.hydration,
            failure: self.last_error.clone(),
            truncated: self.history_truncated,
            backpressure,
            cancelled,
            rolled_back,
        }
    }
}

impl Default for JetWebStreamingBoundary {
    fn default() -> Self {
        let identity = JetWebIslandIdentity {
            island_id: "default".to_string(),
            source_id: "default".to_string(),
            build_id: "default".to_string(),
            revision: "default".to_string(),
        };
        Self::new("default", identity).expect("default web pending identity is valid")
    }
}

struct JetWebStreamingRegistryEntry {
    boundary: JetWebStreamingBoundary,
    next_generation: u64,
    active_stream: Option<JetWebStreamId>,
    committed_stream: Option<JetWebStreamId>,
}

impl JetWebStreamingRegistryEntry {
    fn new(boundary: JetWebStreamingBoundary) -> Self {
        Self {
            boundary,
            next_generation: 1,
            active_stream: None,
            committed_stream: None,
        }
    }

    fn reclaimable(&self) -> bool {
        self.active_stream.is_none()
            && (self.boundary.is_failed()
                || self
                    .boundary
                    .hydration()
                    .is_some_and(|timing| timing.is_complete()))
    }
}

type JetWebStreamingRegistry =
    std::collections::BTreeMap<String, JetWebStreamingRegistryEntry>;

static JET_WEB_STREAMING_REGISTRY: std::sync::LazyLock<
    std::sync::Mutex<JetWebStreamingRegistry>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::BTreeMap::new()));

fn jet_web_stream_registry() -> &'static std::sync::Mutex<JetWebStreamingRegistry> {
    &JET_WEB_STREAMING_REGISTRY
}

fn jet_web_stream_entry<'a>(
    registry: &'a mut JetWebStreamingRegistry,
    boundary_id: &JetWebBoundaryId,
) -> Result<&'a mut JetWebStreamingRegistryEntry, JetWebPendingError> {
    registry
        .get_mut(boundary_id.as_str())
        .ok_or(JetWebPendingError::InvalidBoundary {
            field: "registered boundary",
        })
}

fn jet_web_stream_validate_id(
    entry: &JetWebStreamingRegistryEntry,
    stream_id: &JetWebStreamId,
    committed: bool,
    operation: &'static str,
) -> Result<(), JetWebPendingError> {
    let expected = if committed {
        entry.committed_stream.as_ref()
    } else {
        entry.active_stream.as_ref()
    };
    let Some(expected) = expected else {
        return Err(JetWebPendingError::InvalidLifecycle {
            state: entry.boundary.state(),
            operation,
        });
    };
    if expected != stream_id {
        return Err(JetWebPendingError::StreamIdentityMismatch {
            expected: expected.clone(),
            actual: stream_id.clone(),
        });
    }
    Ok(())
}

fn jet_web_stream_cleanup(registry: &mut JetWebStreamingRegistry) {
    registry.retain(|_, entry| !entry.reclaimable());
}

/// Register one compiler-derived boundary for the internal App bridge.
///
/// This is deliberately not exported through `core.web`: generated App code
/// uses the typed ID, while hosts receive only receipt/projection JSON.
pub fn jet_web_stream_register(
    boundary_id: String,
    identity: JetWebIslandIdentity,
) -> Result<JetWebBoundaryId, JetWebPendingError> {
    let boundary_id = JetWebBoundaryId::new(boundary_id)?;
    let mut registry = jet_web_stream_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = registry.get(boundary_id.as_str()) {
        if entry.boundary.identity() != &identity {
            return Err(JetWebPendingError::IdentityMismatch {
                expected: entry.boundary.identity().clone(),
                actual: identity,
            });
        }
        return Ok(boundary_id);
    }
    if registry.len() >= JET_WEB_STREAM_REGISTRY_MAX {
        jet_web_stream_cleanup(&mut registry);
    }
    if registry.len() >= JET_WEB_STREAM_REGISTRY_MAX {
        return Err(JetWebPendingError::InvalidLimit {
            field: "stream registry",
            actual: registry.len(),
            maximum: JET_WEB_STREAM_REGISTRY_MAX,
        });
    }
    let boundary = JetWebStreamingBoundary::new(boundary_id.as_str(), identity)?;
    registry.insert(
        boundary_id.as_str().to_string(),
        JetWebStreamingRegistryEntry::new(boundary),
    );
    Ok(boundary_id)
}

/// Start a new stream generation and return its cancellation-safe identity.
pub fn jet_web_stream_begin(
    boundary_id: &JetWebBoundaryId,
    identity: JetWebIslandIdentity,
    at_ns: u64,
) -> Result<JetWebStreamId, JetWebPendingError> {
    let mut registry = jet_web_stream_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let entry = jet_web_stream_entry(&mut registry, boundary_id)?;
    if entry.next_generation == u64::MAX {
        return Err(JetWebPendingError::SequenceExhausted { kind: "stream" });
    }
    let _ = entry.boundary.begin(identity.clone(), at_ns)?;
    let stream_id = JetWebStreamId::new(
        boundary_id.clone(),
        entry.next_generation,
        identity,
    );
    entry.next_generation += 1;
    entry.active_stream = Some(stream_id.clone());
    Ok(stream_id)
}

/// Append one ordered chunk to the active stream generation.
pub fn jet_web_stream_chunk(
    stream_id: &JetWebStreamId,
    sequence: u64,
    content: String,
    is_final: bool,
    at_ns: u64,
) -> Result<JetWebPendingReceipt, JetWebPendingError> {
    let mut registry = jet_web_stream_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let entry = jet_web_stream_entry(&mut registry, stream_id.boundary_id())?;
    jet_web_stream_validate_id(entry, stream_id, false, "chunk")?;
    let chunk = JetWebPendingChunk::new(
        sequence,
        content,
        is_final,
        stream_id.identity().clone(),
    )?;
    entry.boundary.push_chunk_at(chunk, at_ns)
}

/// Publish the complete stream and retain its ID for hydration.
pub fn jet_web_stream_commit(
    stream_id: &JetWebStreamId,
    at_ns: u64,
) -> Result<JetWebPendingReceipt, JetWebPendingError> {
    let mut registry = jet_web_stream_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let entry = jet_web_stream_entry(&mut registry, stream_id.boundary_id())?;
    jet_web_stream_validate_id(entry, stream_id, false, "commit")?;
    let receipt = entry.boundary.commit(at_ns)?;
    entry.committed_stream = entry.active_stream.take();
    Ok(receipt)
}

/// Publish a typed stream failure and reclaim its active registry entry.
pub fn jet_web_stream_fail(
    stream_id: &JetWebStreamId,
    error: JetWebPendingError,
    at_ns: u64,
) -> Result<JetWebPendingReceipt, JetWebPendingError> {
    let boundary_key = stream_id.boundary_id().as_str().to_string();
    let mut registry = jet_web_stream_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let receipt = {
        let entry = jet_web_stream_entry(&mut registry, stream_id.boundary_id())?;
        jet_web_stream_validate_id(entry, stream_id, false, "fail")?;
        let receipt = entry.boundary.fail_for(stream_id.identity().clone(), error, at_ns)?;
        entry.active_stream = None;
        receipt
    };
    registry.remove(&boundary_key);
    Ok(receipt)
}

/// Cancel only the exact active stream generation; stale work cannot cancel a
/// newer stream with the same boundary name.
pub fn jet_web_stream_cancel(
    stream_id: &JetWebStreamId,
    at_ns: u64,
) -> Result<JetWebPendingReceipt, JetWebPendingError> {
    let boundary_key = stream_id.boundary_id().as_str().to_string();
    let mut registry = jet_web_stream_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let receipt = {
        let entry = jet_web_stream_entry(&mut registry, stream_id.boundary_id())?;
        jet_web_stream_validate_id(entry, stream_id, false, "cancel")?;
        let receipt = entry
            .boundary
            .cancel_for(stream_id.identity().clone(), at_ns)?;
        entry.active_stream = None;
        receipt
    };
    registry.remove(&boundary_key);
    Ok(receipt)
}

/// Roll back only the exact active stream generation.
pub fn jet_web_stream_rollback(
    stream_id: &JetWebStreamId,
    at_ns: u64,
) -> Result<JetWebPendingReceipt, JetWebPendingError> {
    let boundary_key = stream_id.boundary_id().as_str().to_string();
    let mut registry = jet_web_stream_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let receipt = {
        let entry = jet_web_stream_entry(&mut registry, stream_id.boundary_id())?;
        jet_web_stream_validate_id(entry, stream_id, false, "rollback")?;
        let receipt = entry
            .boundary
            .rollback_for(stream_id.identity().clone(), at_ns)?;
        entry.active_stream = None;
        receipt
    };
    registry.remove(&boundary_key);
    Ok(receipt)
}

/// Start hydration only for the stream that published the current HTML.
pub fn jet_web_stream_hydration_start(
    stream_id: &JetWebStreamId,
    trigger: JetWebHydrationTrigger,
    started_at_ns: u64,
) -> Result<JetWebPendingReceipt, JetWebPendingError> {
    let mut registry = jet_web_stream_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let entry = jet_web_stream_entry(&mut registry, stream_id.boundary_id())?;
    jet_web_stream_validate_id(entry, stream_id, true, "hydration_start")?;
    entry.boundary.hydration_start_with_trigger(
        stream_id.identity().clone(),
        trigger,
        started_at_ns,
    )
}

/// Complete hydration and reclaim the now-terminal boundary registry entry.
pub fn jet_web_stream_hydration_complete(
    stream_id: &JetWebStreamId,
    completed_at_ns: u64,
) -> Result<JetWebPendingReceipt, JetWebPendingError> {
    let boundary_key = stream_id.boundary_id().as_str().to_string();
    let mut registry = jet_web_stream_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let receipt = {
        let entry = jet_web_stream_entry(&mut registry, stream_id.boundary_id())?;
        jet_web_stream_validate_id(entry, stream_id, true, "hydration_complete")?;
        entry
            .boundary
            .hydration_complete(stream_id.identity().clone(), completed_at_ns)?
    };
    registry.remove(&boundary_key);
    Ok(receipt)
}

/// Return a deterministic JSON receipt for the host/devtools projection.
pub fn jet_web_stream_receipt_json(
    boundary_id: &JetWebBoundaryId,
) -> Result<String, JetWebPendingError> {
    let mut registry = jet_web_stream_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let entry = jet_web_stream_entry(&mut registry, boundary_id)?;
    Ok(entry.boundary.receipt().render_json())
}

/// Return the shared server/client projection as host-facing JSON.
pub fn jet_web_stream_projection_json(
    boundary_id: &JetWebBoundaryId,
    target: JetWebPendingProjectionTarget,
) -> Result<String, JetWebPendingError> {
    let mut registry = jet_web_stream_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let entry = jet_web_stream_entry(&mut registry, boundary_id)?;
    Ok(entry.boundary.project(target).render_json())
}

pub fn jet_web_stream_id_json(stream_id: &JetWebStreamId) -> String {
    stream_id.render_json()
}

fn jet_web_pending_validate_identity(
    value: &str,
    field: &'static str,
) -> Result<(), JetWebPendingError> {
    if value.is_empty()
        || value.len() > JET_WEB_PENDING_MAX_IDENTITY_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(JetWebPendingError::InvalidIdentity { field });
    }
    Ok(())
}

fn jet_web_pending_validate_boundary(
    value: &str,
    field: &'static str,
) -> Result<(), JetWebPendingError> {
    if value.is_empty()
        || value.len() > JET_WEB_PENDING_MAX_BOUNDARY_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(JetWebPendingError::InvalidBoundary { field });
    }
    Ok(())
}

fn jet_web_pending_validate_limit(
    actual: usize,
    maximum: usize,
    field: &'static str,
) -> Result<(), JetWebPendingError> {
    if actual == 0 || actual > maximum {
        return Err(JetWebPendingError::InvalidLimit {
            field,
            actual,
            maximum,
        });
    }
    Ok(())
}

fn jet_web_pending_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod web_pending_tests {
    use super::*;

    fn identity() -> JetWebIslandIdentity {
        JetWebIslandIdentity::new("orders", "orders.jet", "build-a", "rev-7").unwrap()
    }

    fn boundary() -> JetWebStreamingBoundary {
        JetWebStreamingBoundary::new("orders-boundary", identity()).unwrap()
    }

    #[test]
    fn lifecycle_commits_and_projects_exact_identity_and_time() {
        let mut state = boundary();
        assert!(state.receipt().is_pending());
        state.begin_at(10).unwrap();
        state
            .push_chunk_at(JetWebPendingChunk::new(0, "shell", false, identity()).unwrap(), 20)
            .unwrap();
        state
            .push_chunk_at(JetWebPendingChunk::new(1, "-body", true, identity()).unwrap(), 30)
            .unwrap();
        let ready = state.commit(40).unwrap();
        assert_eq!(ready.state, JetWebPendingState::Ready);
        assert_eq!(ready.content, "shell-body");
        assert_eq!(ready.identity.revision, "rev-7");
        let hydration = state.hydration_start(identity(), 100).unwrap();
        assert_eq!(hydration.hydration.unwrap().started_at_ns, 100);
        let hydrated = state.hydration_complete(identity(), 145).unwrap();
        assert_eq!(hydrated.hydration.unwrap().duration_ns(), Some(45));
        assert_eq!(state.server_projection().content(), state.client_projection().content());
        assert_eq!(state.server_projection().content(), "shell-body");
    }

    #[test]
    fn invalid_sequence_and_identity_fail_closed_to_last_commit() {
        let mut state = boundary();
        state.begin_at(1).unwrap();
        state
            .push_chunk_at(JetWebPendingChunk::new(0, "old", true, identity()).unwrap(), 2)
            .unwrap();
        state.commit(3).unwrap();
        state.begin_at(4).unwrap();
        state
            .push_chunk_at(JetWebPendingChunk::new(0, "new", false, identity()).unwrap(), 5)
            .unwrap();
        let result = state.push_chunk_at(
            JetWebPendingChunk::new(2, "gap", true, identity()).unwrap(),
            6,
        );
        assert!(matches!(
            result,
            Err(JetWebPendingError::InvalidChunkSequence {
                expected: 1,
                actual: 2
            })
        ));
        assert!(state.is_failed());
        assert_eq!(state.content(), "old");
        assert_eq!(state.stream_content(), "old");

        state.begin_at(7).unwrap();
        let mut wrong = identity();
        wrong.revision = "rev-other".to_string();
        let mismatch = state.hydration_start(wrong, 8);
        assert!(matches!(
            mismatch,
            Err(JetWebPendingError::HydrationIdentityMismatch { .. })
        ));
        assert!(state.is_failed());
        assert_eq!(state.content(), "old");
    }

    #[test]
    fn backpressure_and_history_truncation_are_observable() {
        let id = identity();
        let mut state = JetWebStreamingBoundary::with_capacities("b", id.clone(), 1, 4, 2).unwrap();
        state.begin_at(1).unwrap();
        state
            .push_chunk_at(JetWebPendingChunk::new(0, "1234", false, id.clone()).unwrap(), 1)
            .unwrap();
        let result = state.push_chunk_at(JetWebPendingChunk::new(1, "x", true, id).unwrap(), 1);
        assert!(matches!(
            result,
            Err(JetWebPendingError::Backpressure {
                dimension: JetWebPendingBackpressureDimension::Chunks,
                limit: 1,
                ..
            })
        ));
        assert!(state.receipt().backpressure);
        assert!(state.is_failed());
        assert_eq!(state.content(), "");
        assert_eq!(state.fact_count(), 2);
        assert!(state.history_truncated());
        assert!(state.receipt().truncated);
    }

    #[test]
    fn cancellation_restores_ready_content_and_marks_receipt() {
        let id = identity();
        let mut state = boundary();
        state.begin_at(1).unwrap();
        state
            .push_chunk_at(JetWebPendingChunk::new(0, "committed", true, id.clone()).unwrap(), 2)
            .unwrap();
        state.commit(3).unwrap();
        state.begin_at(4).unwrap();
        state
            .push_chunk_at(JetWebPendingChunk::new(0, "replacement", true, id).unwrap(), 5)
            .unwrap();
        let cancelled = state.cancel(6);
        assert!(cancelled.is_cancelled());
        assert!(cancelled.was_rolled_back());
        assert_eq!(cancelled.state, JetWebPendingState::Ready);
        assert_eq!(cancelled.content, "committed");
        assert_eq!(state.content(), "committed");
    }
}
