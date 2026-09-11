use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Mutex, OnceLock};

/// One bounded fact history is enough for the frame debugger and its host
/// projections.  The bound is deliberately shared with the game-dev profile
/// protocol rather than being a host-local cache setting.
pub const JET_GAME_FRAME_PROFILER_MAX_HISTORY: usize = 256;
pub const JET_GAME_FRAME_PROFILER_MAX_RECEIPTS: usize = 256;
pub const JET_GAME_FRAME_PROFILER_MAX_BOTTLENECKS: usize = 128;

pub const JET_GAME_FRAME_PROFILER_MAX_CALL_GRAPH: usize = 64;
pub const JET_GAME_FRAME_PROFILER_MAX_CRASH_STACKS: usize = 32;
pub const JET_GAME_FRAME_PROFILER_MAX_CRASH_LOGS: usize = 256;
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct JetGameTraceIdentity {
    pub build: String,
    pub revision: String,
    pub trace_id: String,
}

impl JetGameTraceIdentity {
    pub fn new(
        build: impl Into<String>,
        revision: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        Self {
            build: build.into(),
            revision: revision.into(),
            trace_id: trace_id.into(),
        }
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        jet_game_frame_profiler_require_text(&self.build, "build")?;
        jet_game_frame_profiler_require_text(&self.revision, "revision")?;
        jet_game_frame_profiler_require_text(&self.trace_id, "trace id")
    }
}
/// Capture context is persisted with every trace. A frame identity alone
/// cannot explain which game session, renderer, viewport, or settings
/// produced a sample.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct JetGameTraceContext {
    pub session_id: String,
    pub scene: String,
    pub target: String,
    pub backend: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub settings: String,
}

impl JetGameTraceContext {
    pub fn new(
        session_id: impl Into<String>,
        scene: impl Into<String>,
        target: impl Into<String>,
        backend: impl Into<String>,
        viewport_width: u32,
        viewport_height: u32,
        settings: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            scene: scene.into(),
            target: target.into(),
            backend: backend.into(),
            viewport_width,
            viewport_height,
            settings: settings.into(),
        }
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        jet_game_frame_profiler_require_text(&self.session_id, "session id")?;
        jet_game_frame_profiler_require_text(&self.scene, "trace scene")?;
        jet_game_frame_profiler_require_text(&self.target, "trace target")?;
        jet_game_frame_profiler_require_text(&self.backend, "trace backend")?;
        jet_game_frame_profiler_require_text(&self.settings, "trace settings")?;
        if self.viewport_width == 0 || self.viewport_height == 0 {
            return Err(JetGameFrameProfilerError::InvalidTraceContext {
                reason: "trace viewport must be non-zero",
            });
        }
        Ok(())
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"session_id\":{},\"scene\":{},\"target\":{},\"backend\":{},\
\"viewport\":{{\"width\":{},\"height\":{}}},\"settings\":{}}}",
            jet_game_frame_profiler_json_string(&self.session_id),
            jet_game_frame_profiler_json_string(&self.scene),
            jet_game_frame_profiler_json_string(&self.target),
            jet_game_frame_profiler_json_string(&self.backend),
            self.viewport_width,
            self.viewport_height,
            jet_game_frame_profiler_json_string(&self.settings),
        )
    }
}


#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct JetGameSourceSpan {
    pub source_id: String,
    pub file: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl JetGameSourceSpan {
    pub fn new(
        source_id: impl Into<String>,
        file: impl Into<String>,
        start_line: u32,
        start_column: u32,
        end_line: u32,
        end_column: u32,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            file: file.into(),
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        jet_game_frame_profiler_require_text(&self.source_id, "source id")?;
        jet_game_frame_profiler_require_text(&self.file, "source file")?;
        if self.start_line == 0 || self.start_column == 0 {
            return Err(JetGameFrameProfilerError::InvalidSourceSpan {
                reason: "source span starts at zero",
            });
        }
        if self.end_line == 0 || self.end_column == 0 {
            return Err(JetGameFrameProfilerError::InvalidSourceSpan {
                reason: "source span ends at zero",
            });
        }
        if (self.end_line, self.end_column) < (self.start_line, self.start_column) {
            return Err(JetGameFrameProfilerError::InvalidSourceSpan {
                reason: "source span ends before it starts",
            });
        }
        Ok(())
    }
}

/// Function identity is inseparable from its source mapping.  A display name
/// alone is not enough to join a hot path to a source location.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct JetGameFunctionIdentity {
    pub function_id: String,
    pub name: String,
    pub source: JetGameSourceSpan,
}

impl JetGameFunctionIdentity {
    pub fn new(
        function_id: impl Into<String>,
        name: impl Into<String>,
        source: JetGameSourceSpan,
    ) -> Self {
        Self {
            function_id: function_id.into(),
            name: name.into(),
            source,
        }
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        jet_game_frame_profiler_require_text(&self.function_id, "function id")?;
        jet_game_frame_profiler_require_text(&self.name, "function name")?;
        self.source.validate()
    }
}

/// A frame id is deterministic for one scene, build, source revision, and
/// frame index.  The trace id remains capture provenance and is not part of
/// the frame position, so two captures can refer to the same frame position.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct JetGameFrameIdentity {
    pub frame_id: u64,
    pub scene: String,
    pub frame_index: u64,
    pub build: String,
    pub revision: String,
    pub trace_id: String,
}

impl JetGameFrameIdentity {
    pub fn new(
        scene: impl Into<String>,
        frame_index: u64,
        build: impl Into<String>,
        revision: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        let scene = scene.into();
        let build = build.into();
        let revision = revision.into();
        let trace_id = trace_id.into();
        let frame_id = jet_game_frame_profiler_frame_id(&scene, frame_index, &build, &revision);
        Self {
            frame_id,
            scene,
            frame_index,
            build,
            revision,
            trace_id,
        }
    }

    pub fn trace_identity(&self) -> JetGameTraceIdentity {
        JetGameTraceIdentity::new(&self.build, &self.revision, &self.trace_id)
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        jet_game_frame_profiler_require_text(&self.scene, "scene")?;
        self.trace_identity().validate()?;
        let expected = jet_game_frame_profiler_frame_id(
            &self.scene,
            self.frame_index,
            &self.build,
            &self.revision,
        );
        if self.frame_id != expected {
            return Err(JetGameFrameProfilerError::InvalidFrameIdentity {
                expected,
                actual: self.frame_id,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetGameTimeDomain {
    Cpu,
    Gpu,
}

impl JetGameTimeDomain {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetGameFramePhase {
    Input,
    Update,
    Physics,
    Animation,
    Script,
    Render,
    Draw,
    Present,
    Audio,
    Ui,
    Network,
    Asset,
    Cook,
    Memory,
    Custom(String),
}

impl JetGameFramePhase {
    pub const fn fixed_as_str(&self) -> Option<&'static str> {
        match self {
            Self::Input => Some("input"),
            Self::Update => Some("update"),
            Self::Physics => Some("physics"),
            Self::Animation => Some("animation"),
            Self::Script => Some("script"),
            Self::Render => Some("render"),
            Self::Draw => Some("draw"),
            Self::Present => Some("present"),
            Self::Audio => Some("audio"),
            Self::Ui => Some("ui"),
            Self::Network => Some("network"),
            Self::Asset => Some("asset"),
            Self::Cook => Some("cook"),
            Self::Memory => Some("memory"),
            Self::Custom(_) => None,
        }
    }

    pub fn as_str(&self) -> &str {
        self.fixed_as_str()
            .or_else(|| match self {
                Self::Custom(value) => Some(value.as_str()),
                _ => None,
            })
            .unwrap_or("")
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        if let Self::Custom(value) = self {
            jet_game_frame_profiler_require_text(value, "custom phase")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetGameSampleKind {
    Frame,
    Phase,
    Function,
    Draw,
}

impl JetGameSampleKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Frame => "frame",
            Self::Phase => "phase",
            Self::Function => "function",
            Self::Draw => "draw",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameFrameSample {
    pub sequence: u64,
    pub frame: JetGameFrameIdentity,
    pub start_ns: u64,
    pub cpu_ns: u64,
    pub gpu_ns: Option<u64>,
    pub responsible_function: Option<JetGameFunctionIdentity>,
}

impl JetGameFrameSample {
    pub fn new(
        frame: JetGameFrameIdentity,
        start_ns: u64,
        cpu_ns: u64,
        gpu_ns: Option<u64>,
    ) -> Self {
        Self {
            sequence: 0,
            frame,
            start_ns,
            cpu_ns,
            gpu_ns,
            responsible_function: None,
        }
    }

    pub fn with_responsible_function(mut self, function: JetGameFunctionIdentity) -> Self {
        self.responsible_function = Some(function);
        self
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        self.frame.validate()?;
        if let Some(function) = &self.responsible_function {
            function.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGamePhaseSample {
    pub sequence: u64,
    pub frame: JetGameFrameIdentity,
    pub phase: JetGameFramePhase,
    pub domain: JetGameTimeDomain,
    pub start_ns: u64,
    pub duration_ns: u64,
}

impl JetGamePhaseSample {
    pub fn new(
        frame: JetGameFrameIdentity,
        phase: JetGameFramePhase,
        domain: JetGameTimeDomain,
        start_ns: u64,
        duration_ns: u64,
    ) -> Self {
        Self {
            sequence: 0,
            frame,
            phase,
            domain,
            start_ns,
            duration_ns,
        }
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        self.frame.validate()?;
        self.phase.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameFunctionSample {
    pub sequence: u64,
    pub frame: JetGameFrameIdentity,
    pub function: JetGameFunctionIdentity,
    pub phase: JetGameFramePhase,
    pub domain: JetGameTimeDomain,
    pub start_ns: u64,
    pub duration_ns: u64,
    pub callers: Vec<JetGameFunctionIdentity>,
    pub callees: Vec<JetGameFunctionIdentity>,
}

impl JetGameFunctionSample {
    pub fn new(
        frame: JetGameFrameIdentity,
        function: JetGameFunctionIdentity,
        phase: JetGameFramePhase,
        domain: JetGameTimeDomain,
        start_ns: u64,
        duration_ns: u64,
    ) -> Self {
        Self {
            sequence: 0,
            frame,
            function,
            phase,
            domain,
            start_ns,
            duration_ns,
            callers: Vec::new(),
            callees: Vec::new(),
        }
    }

    pub fn with_call_graph(
        mut self,
        callers: Vec<JetGameFunctionIdentity>,
        callees: Vec<JetGameFunctionIdentity>,
    ) -> Self {
        self.callers = callers;
        self.callees = callees;
        self
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        self.frame.validate()?;
        self.function.validate()?;
        self.phase.validate()?;
        if self.callers.len() > JET_GAME_FRAME_PROFILER_MAX_CALL_GRAPH
            || self.callees.len() > JET_GAME_FRAME_PROFILER_MAX_CALL_GRAPH
        {
            return Err(JetGameFrameProfilerError::CallGraphTooLarge {
                max: JET_GAME_FRAME_PROFILER_MAX_CALL_GRAPH,
            });
        }
        for function in self.callers.iter().chain(self.callees.iter()) {
            function.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameDrawEvent {
    pub sequence: u64,
    pub frame: JetGameFrameIdentity,
    pub event_index: u64,
    pub event_id: u64,
    pub function: JetGameFunctionIdentity,
    pub phase: JetGameFramePhase,
    pub domain: JetGameTimeDomain,
    pub start_ns: u64,
    pub duration_ns: u64,
    pub object_id: Option<String>,
    pub resource_id: Option<String>,
}

impl JetGameDrawEvent {
    pub fn new(
        frame: JetGameFrameIdentity,
        event_index: u64,
        function: JetGameFunctionIdentity,
        phase: JetGameFramePhase,
        domain: JetGameTimeDomain,
        start_ns: u64,
        duration_ns: u64,
    ) -> Self {
        let event_id = jet_game_frame_profiler_draw_id(frame.frame_id, event_index);
        Self {
            sequence: 0,
            frame,
            event_index,
            event_id,
            function,
            phase,
            domain,
            start_ns,
            duration_ns,
            object_id: None,
            resource_id: None,
        }
    }

    pub fn with_links(
        mut self,
        object_id: Option<impl Into<String>>,
        resource_id: Option<impl Into<String>>,
    ) -> Self {
        self.object_id = object_id.map(Into::into);
        self.resource_id = resource_id.map(Into::into);
        self
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        self.frame.validate()?;
        self.function.validate()?;
        self.phase.validate()?;
        if let Some(object_id) = &self.object_id {
            jet_game_frame_profiler_require_text(object_id, "draw object id")?;
        }
        if let Some(resource_id) = &self.resource_id {
            jet_game_frame_profiler_require_text(resource_id, "draw resource id")?;
        }
        let expected = jet_game_frame_profiler_draw_id(self.frame.frame_id, self.event_index);
        if self.event_id != expected {
            return Err(JetGameFrameProfilerError::InvalidDrawIdentity {
                expected,
                actual: self.event_id,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetGameSampleFact {
    Frame(JetGameFrameSample),
    Phase(JetGamePhaseSample),
    Function(JetGameFunctionSample),
    Draw(JetGameDrawEvent),
}

impl JetGameSampleFact {
    pub fn kind(&self) -> JetGameSampleKind {
        match self {
            Self::Frame(_) => JetGameSampleKind::Frame,
            Self::Phase(_) => JetGameSampleKind::Phase,
            Self::Function(_) => JetGameSampleKind::Function,
            Self::Draw(_) => JetGameSampleKind::Draw,
        }
    }

    pub fn frame_identity(&self) -> &JetGameFrameIdentity {
        match self {
            Self::Frame(sample) => &sample.frame,
            Self::Phase(sample) => &sample.frame,
            Self::Function(sample) => &sample.frame,
            Self::Draw(event) => &event.frame,
        }
    }

    pub fn sequence(&self) -> u64 {
        match self {
            Self::Frame(sample) => sample.sequence,
            Self::Phase(sample) => sample.sequence,
            Self::Function(sample) => sample.sequence,
            Self::Draw(event) => event.sequence,
        }
    }

    pub fn timestamp_ns(&self) -> u64 {
        match self {
            Self::Frame(sample) => sample.start_ns,
            Self::Phase(sample) => sample.start_ns,
            Self::Function(sample) => sample.start_ns,
            Self::Draw(event) => event.start_ns,
        }
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        match self {
            Self::Frame(sample) => sample.validate(),
            Self::Phase(sample) => sample.validate(),
            Self::Function(sample) => sample.validate(),
            Self::Draw(event) => event.validate(),
        }
    }

    fn with_sequence(self, sequence: u64) -> Self {
        match self {
            Self::Frame(mut sample) => {
                sample.sequence = sequence;
                Self::Frame(sample)
            }
            Self::Phase(mut sample) => {
                sample.sequence = sequence;
                Self::Phase(sample)
            }
            Self::Function(mut sample) => {
                sample.sequence = sequence;
                Self::Function(sample)
            }
            Self::Draw(mut event) => {
                event.sequence = sequence;
                Self::Draw(event)
            }
        }
    }
}

impl From<JetGameFrameSample> for JetGameSampleFact {
    fn from(sample: JetGameFrameSample) -> Self {
        Self::Frame(sample)
    }
}

impl From<JetGamePhaseSample> for JetGameSampleFact {
    fn from(sample: JetGamePhaseSample) -> Self {
        Self::Phase(sample)
    }
}

impl From<JetGameFunctionSample> for JetGameSampleFact {
    fn from(sample: JetGameFunctionSample) -> Self {
        Self::Function(sample)
    }
}

impl From<JetGameDrawEvent> for JetGameSampleFact {
    fn from(event: JetGameDrawEvent) -> Self {
        Self::Draw(event)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetGameFrameProfilerError {
    EmptyIdentity { field: &'static str },
    InvalidSourceSpan { reason: &'static str },
    InvalidTraceContext { reason: &'static str },
    InvalidFrameIdentity { expected: u64, actual: u64 },
    CallGraphTooLarge { max: usize },
    CrashBundleTooLarge { field: &'static str, max: usize },
    InvalidDrawIdentity { expected: u64, actual: u64 },
    InvalidSampleSequence { actual: u64 },
    BuildMismatch { expected: String, actual: String },
    RevisionMismatch { expected: String, actual: String },
    TraceMismatch { expected: String, actual: String },
    FrameMismatch {
        expected: JetGameFrameIdentity,
        actual: JetGameFrameIdentity,
    },
    NonMonotonicTimestamp { previous: u64, actual: u64 },
    NonMonotonicFrame { previous: u64, actual: u64 },
    DuplicateDrawEvent { frame_id: u64, event_index: u64 },
    DrawEventsOutOfOrder { previous: u64, actual: u64 },
    CursorPositionOutOfBounds { position: usize, len: usize },
    SequenceExhausted { kind: &'static str },
    ProfilerPaused,
    ProfilerNotPaused,
    FrameNotRetained { frame_id: u64 },
    InvalidErrorPause { field: &'static str },
}

impl JetGameFrameProfilerError {
    pub fn message(&self) -> String {
        match self {
            Self::EmptyIdentity { field } => {
                format!("game frame profiler {field} must not be empty")
            }
            Self::InvalidSourceSpan { reason } => {
                format!("game frame profiler source span {reason}")
            }
            Self::InvalidTraceContext { reason } => {
                format!("game frame profiler trace context {reason}")
            }
            Self::InvalidFrameIdentity { expected, actual } => {
                format!("game frame profiler frame id {actual} does not match stable id {expected}")
            }
            Self::CallGraphTooLarge { max } => {
                format!("game frame profiler call graph exceeds {max} functions")
            }
            Self::CrashBundleTooLarge { field, max } => {
                format!("game frame profiler crash bundle {field} exceeds {max} entries")
            }
            Self::InvalidDrawIdentity { expected, actual } => {
                format!("game frame profiler draw id {actual} does not match stable id {expected}")
            }
            Self::InvalidSampleSequence { actual } => {
                format!("game frame profiler sample sequence {actual} is not allocator-owned")
            }
            Self::BuildMismatch { expected, actual } => {
                format!("game frame profiler build changed from {expected} to {actual}")
            }
            Self::RevisionMismatch { expected, actual } => {
                format!("game frame profiler source revision changed from {expected} to {actual}")
            }
            Self::TraceMismatch { expected, actual } => {
                format!("game frame profiler trace changed from {expected} to {actual}")
            }
            Self::FrameMismatch { expected, actual } => format!(
                "game frame profiler cursor expected frame {} but received {}",
                expected.frame_id, actual.frame_id
            ),
            Self::NonMonotonicTimestamp { previous, actual } => format!(
                "game frame profiler timestamp moved backward from {previous} to {actual}"
            ),
            Self::NonMonotonicFrame { previous, actual } => format!(
                "game frame profiler frame index moved backward from {previous} to {actual}"
            ),
            Self::DuplicateDrawEvent {
                frame_id,
                event_index,
            } => format!(
                "game frame profiler frame {frame_id} repeats draw event {event_index}"
            ),
            Self::DrawEventsOutOfOrder { previous, actual } => format!(
                "game frame profiler draw event {actual} follows event {previous} out of order"
            ),
            Self::CursorPositionOutOfBounds { position, len } => format!(
                "game frame profiler cursor position {position} exceeds {len} draw events"
            ),
            Self::SequenceExhausted { kind } => {
                format!("game frame profiler {kind} sequence is exhausted")
            }
            Self::ProfilerPaused => "game frame profiler is paused".to_string(),
            Self::ProfilerNotPaused => {
                "game frame profiler draw cursor requires a paused frame".to_string()
            }
            Self::FrameNotRetained { frame_id } => {
                format!("game frame profiler frame {frame_id} is not retained")
            }
            Self::InvalidErrorPause { field } => {
                format!("game frame profiler error pause {field} is invalid")
            }
        }
    }
}

impl std::fmt::Display for JetGameFrameProfilerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for JetGameFrameProfilerError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameFrameHistory {
    trace_identity: JetGameTraceIdentity,
    capacity: usize,
    facts: VecDeque<JetGameSampleFact>,
    next_sequence: u64,
    dropped_samples: u64,
    last_timestamp_ns: Option<u64>,
    last_frame_index: Option<u64>,
}

impl JetGameFrameHistory {
    pub fn new(trace_identity: JetGameTraceIdentity) -> Self {
        Self::with_capacity(trace_identity, JET_GAME_FRAME_PROFILER_MAX_HISTORY)
    }

    pub fn with_capacity(trace_identity: JetGameTraceIdentity, capacity: usize) -> Self {
        Self {
            trace_identity,
            capacity: capacity.clamp(1, JET_GAME_FRAME_PROFILER_MAX_HISTORY),
            facts: VecDeque::new(),
            next_sequence: 1,
            dropped_samples: 0,
            last_timestamp_ns: None,
            last_frame_index: None,
        }
    }

    pub fn trace_identity(&self) -> &JetGameTraceIdentity {
        &self.trace_identity
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.facts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    pub fn dropped_samples(&self) -> u64 {
        self.dropped_samples
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn facts(&self) -> impl Iterator<Item = &JetGameSampleFact> {
        self.facts.iter()
    }

    pub fn latest(&self) -> Option<&JetGameSampleFact> {
        self.facts.back()
    }

    pub fn append(&mut self, fact: JetGameSampleFact) -> Result<u64, JetGameFrameProfilerError> {
        fact.validate()?;
        if fact.sequence() != 0 {
            return Err(JetGameFrameProfilerError::InvalidSampleSequence {
                actual: fact.sequence(),
            });
        }
        let frame = fact.frame_identity().clone();
        jet_game_frame_profiler_check_trace(&self.trace_identity, &frame.trace_identity())?;
        if let Some(previous) = self.last_timestamp_ns {
            if fact.timestamp_ns() < previous {
                return Err(JetGameFrameProfilerError::NonMonotonicTimestamp {
                    previous,
                    actual: fact.timestamp_ns(),
                });
            }
        }
        if let Some(previous) = self.last_frame_index {
            if frame.frame_index < previous {
                return Err(JetGameFrameProfilerError::NonMonotonicFrame {
                    previous,
                    actual: frame.frame_index,
                });
            }
        }
        if let JetGameSampleFact::Draw(event) = &fact {
            if self.facts.iter().any(|retained| {
                matches!(
                    retained,
                    JetGameSampleFact::Draw(previous)
                        if previous.frame == event.frame && previous.event_index == event.event_index
                )
            }) {
                return Err(JetGameFrameProfilerError::DuplicateDrawEvent {
                    frame_id: event.frame.frame_id,
                    event_index: event.event_index,
                });
            }
        }
        if self.next_sequence == u64::MAX {
            return Err(JetGameFrameProfilerError::SequenceExhausted {
                kind: "sample",
            });
        }
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        let fact = fact.with_sequence(sequence);
        if self.facts.len() == self.capacity {
            self.facts.pop_front();
            self.dropped_samples = self.dropped_samples.saturating_add(1);
        }
        self.last_timestamp_ns = Some(fact.timestamp_ns());
        self.last_frame_index = Some(frame.frame_index);
        self.facts.push_back(fact);
        Ok(sequence)
    }

    pub fn frame_facts(
        &self,
        frame: &JetGameFrameIdentity,
    ) -> Result<Vec<JetGameSampleFact>, JetGameFrameProfilerError> {
        frame.validate()?;
        jet_game_frame_profiler_check_trace(&self.trace_identity, &frame.trace_identity())?;
        Ok(self
            .facts
            .iter()
            .filter(|fact| fact.frame_identity() == frame)
            .cloned()
            .collect())
    }

    pub fn draw_events_for(
        &self,
        frame: &JetGameFrameIdentity,
    ) -> Result<Vec<JetGameDrawEvent>, JetGameFrameProfilerError> {
        let facts = self.frame_facts(frame)?;
        let mut events = facts
            .into_iter()
            .filter_map(|fact| match fact {
                JetGameSampleFact::Draw(event) => Some(event),
                _ => None,
            })
            .collect::<Vec<_>>();
        events.sort_by(|left, right| {
            left.event_index
                .cmp(&right.event_index)
                .then_with(|| left.sequence.cmp(&right.sequence))
        });
        Ok(events)
    }

    pub fn hot_paths(&self) -> Vec<JetGameHotPath> {
        let mut accumulators = BTreeMap::<
            (JetGameTimeDomain, JetGameFunctionIdentity),
            JetGameHotPathAccumulator,
        >::new();
        for fact in &self.facts {
            let JetGameSampleFact::Function(sample) = fact else {
                continue;
            };
            let key = (sample.domain, sample.function.clone());
            let accumulator = accumulators
                .entry(key)
                .or_insert_with(|| JetGameHotPathAccumulator {
                    function: sample.function.clone(),
                    domain: sample.domain,
                    sample_count: 0,
                    total_ns: 0,
                    max_ns: 0,
                    frames: BTreeSet::new(),
                });
            accumulator.sample_count = accumulator.sample_count.saturating_add(1);
            accumulator.total_ns = accumulator.total_ns.saturating_add(sample.duration_ns);
            accumulator.max_ns = accumulator.max_ns.max(sample.duration_ns);
            accumulator.frames.insert(sample.frame.frame_id);
        }
        let mut paths = accumulators
            .into_values()
            .map(JetGameHotPathAccumulator::finish)
            .collect::<Vec<_>>();
        paths.sort_by(|left, right| {
            right
                .total_ns
                .cmp(&left.total_ns)
                .then_with(|| right.max_ns.cmp(&left.max_ns))
                .then_with(|| right.sample_count.cmp(&left.sample_count))
                .then_with(|| left.function.function_id.cmp(&right.function.function_id))
                .then_with(|| left.domain.cmp(&right.domain))
        });
        paths
    }

    pub fn bottleneck(&self, limit: usize) -> JetGameBottleneckProjection {
        let mut hot_paths = self.hot_paths();
        let total_function_ns = hot_paths.iter().fold(0u64, |total, path| {
            total.saturating_add(path.total_ns)
        });
        hot_paths.truncate(limit.min(JET_GAME_FRAME_PROFILER_MAX_BOTTLENECKS));
        JetGameBottleneckProjection {
            trace_identity: self.trace_identity.clone(),
            retained_samples: self.facts.len(),
            capacity: self.capacity,
            dropped_samples: self.dropped_samples,
            total_function_ns,
            hot_paths,
        }
    }
}

#[derive(Clone, Debug)]
struct JetGameHotPathAccumulator {
    function: JetGameFunctionIdentity,
    domain: JetGameTimeDomain,
    sample_count: u64,
    total_ns: u64,
    max_ns: u64,
    frames: BTreeSet<u64>,
}

impl JetGameHotPathAccumulator {
    fn finish(self) -> JetGameHotPath {
        JetGameHotPath {
            function: self.function,
            domain: self.domain,
            sample_count: self.sample_count,
            frame_count: self.frames.len() as u64,
            total_ns: self.total_ns,
            max_ns: self.max_ns,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameHotPath {
    pub function: JetGameFunctionIdentity,
    pub domain: JetGameTimeDomain,
    pub sample_count: u64,
    pub frame_count: u64,
    pub total_ns: u64,
    pub max_ns: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameBottleneckProjection {
    pub trace_identity: JetGameTraceIdentity,
    pub retained_samples: usize,
    pub capacity: usize,
    pub dropped_samples: u64,
    pub total_function_ns: u64,
    pub hot_paths: Vec<JetGameHotPath>,
}

/// A cursor is bound to one exact frame identity.  It can only move through a
/// caller-provided event slice after every event has passed the identity and
/// ordering checks; a different frame or source revision is an error, not EOF.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameDrawCursor {
    frame: JetGameFrameIdentity,
    position: usize,
}

impl JetGameDrawCursor {
    pub fn new(frame: JetGameFrameIdentity) -> Self {
        Self { frame, position: 0 }
    }

    pub fn frame_identity(&self) -> &JetGameFrameIdentity {
        &self.frame
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn at_start(&self) -> bool {
        self.position == 0
    }

    pub fn at_end(
        &self,
        events: &[JetGameDrawEvent],
    ) -> Result<bool, JetGameFrameProfilerError> {
        self.validate_events(events)?;
        if self.position > events.len() {
            return Err(JetGameFrameProfilerError::CursorPositionOutOfBounds {
                position: self.position,
                len: events.len(),
            });
        }
        Ok(self.position == events.len())
    }

    pub fn current(
        &self,
        events: &[JetGameDrawEvent],
    ) -> Result<Option<JetGameDrawEvent>, JetGameFrameProfilerError> {
        self.validate_events(events)?;
        if self.position > events.len() {
            return Err(JetGameFrameProfilerError::CursorPositionOutOfBounds {
                position: self.position,
                len: events.len(),
            });
        }
        Ok(events.get(self.position).cloned())
    }

    pub fn step_forward(
        &mut self,
        events: &[JetGameDrawEvent],
    ) -> Result<Option<JetGameDrawEvent>, JetGameFrameProfilerError> {
        self.validate_events(events)?;
        if self.position > events.len() {
            return Err(JetGameFrameProfilerError::CursorPositionOutOfBounds {
                position: self.position,
                len: events.len(),
            });
        }
        let Some(event) = events.get(self.position).cloned() else {
            return Ok(None);
        };
        self.position += 1;
        Ok(Some(event))
    }

    pub fn step_backward(
        &mut self,
        events: &[JetGameDrawEvent],
    ) -> Result<Option<JetGameDrawEvent>, JetGameFrameProfilerError> {
        self.validate_events(events)?;
        if self.position > events.len() {
            return Err(JetGameFrameProfilerError::CursorPositionOutOfBounds {
                position: self.position,
                len: events.len(),
            });
        }
        if self.position == 0 {
            return Ok(None);
        }
        self.position -= 1;
        Ok(events.get(self.position).cloned())
    }

    fn validate_events(&self, events: &[JetGameDrawEvent]) -> Result<(), JetGameFrameProfilerError> {
        self.frame.validate()?;
        let mut previous = None;
        for event in events {
            event.validate()?;
            jet_game_frame_profiler_check_frame(&self.frame, &event.frame)?;
            if let Some(previous_index) = previous {
                if event.event_index <= previous_index {
                    return Err(JetGameFrameProfilerError::DrawEventsOutOfOrder {
                        previous: previous_index,
                        actual: event.event_index,
                    });
                }
            }
            previous = Some(event.event_index);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetGameFrameProfilerState {
    Running,
    Paused,
    ErrorPaused,
}

impl JetGameFrameProfilerState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::ErrorPaused => "error_paused",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetGameFrameReceiptKind {
    Frame,
    Phase,
    Function,
    Draw,
    Paused,
    Resumed,
    ErrorPaused,
}

impl JetGameFrameReceiptKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Frame => "frame",
            Self::Phase => "phase",
            Self::Function => "function",
            Self::Draw => "draw",
            Self::Paused => "paused",
            Self::Resumed => "resumed",
            Self::ErrorPaused => "error_paused",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetGameReloadStatus {
    Kept,
    Reset,
    Unsupported,
}

impl JetGameReloadStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Kept => "kept",
            Self::Reset => "reset",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameReloadFact {
    pub frame: JetGameFrameIdentity,
    pub domain: String,
    pub status: JetGameReloadStatus,
    pub reason: String,
}

impl JetGameReloadFact {
    pub fn new(
        frame: JetGameFrameIdentity,
        domain: impl Into<String>,
        status: JetGameReloadStatus,
        reason: impl Into<String>,
    ) -> Result<Self, JetGameFrameProfilerError> {
        let fact = Self {
            frame,
            domain: domain.into(),
            status,
            reason: reason.into(),
        };
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        self.frame.validate()?;
        jet_game_frame_profiler_require_error_pause_text(&self.domain, "reload domain")?;
        jet_game_frame_profiler_require_error_pause_text(&self.reason, "reload reason")
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"frame\":{},\"domain\":{},\"status\":{},\"reason\":{}}}",
            jet_game_trace_frame_json(&self.frame),
            jet_game_frame_profiler_json_string(&self.domain),
            jet_game_frame_profiler_json_string(self.status.as_str()),
            jet_game_frame_profiler_json_string(&self.reason),
        )
    }
}


#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameErrorPause {
    pub source: JetGameSourceSpan,
    pub code: String,
    pub message: String,
}

impl JetGameErrorPause {
    pub fn new(
        source: JetGameSourceSpan,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<Self, JetGameFrameProfilerError> {
        let pause = Self {
            source,
            code: code.into(),
            message: message.into(),
        };
        pause.validate()?;
        Ok(pause)
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        self.source.validate()?;
        jet_game_frame_profiler_require_error_pause_text(&self.code, "code")?;
        jet_game_frame_profiler_require_error_pause_text(&self.message, "message")
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetGameCrashUploadPolicy {
    Never,
    LoopbackOnly,
    Explicit(String),
}

impl JetGameCrashUploadPolicy {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Never => "never",
            Self::LoopbackOnly => "loopback-only",
            Self::Explicit(_) => "explicit",
        }
    }

    pub fn render_json(&self) -> String {
        match self {
            Self::Never | Self::LoopbackOnly => {
                jet_game_frame_profiler_json_string(self.as_str())
            }
            Self::Explicit(endpoint) => format!(
                "{{\"mode\":{},\"endpoint\":{}}}",
                jet_game_frame_profiler_json_string(self.as_str()),
                jet_game_frame_profiler_json_string(endpoint),
            ),
        }
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        if let Self::Explicit(endpoint) = self {
            jet_game_frame_profiler_require_error_pause_text(endpoint, "crash upload endpoint")?;
        }
        Ok(())
    }
}

/// A bounded crash bundle keeps enough context for local diagnosis without
/// turning a game session into an unbounded log sink. Upload remains explicit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameCrashBundle {
    pub frame: JetGameFrameIdentity,
    pub context: Option<JetGameTraceContext>,
    pub reason: String,
    pub target: String,
    pub stacks: Vec<String>,
    pub logs: Vec<String>,
    pub upload_policy: JetGameCrashUploadPolicy,
}

impl JetGameCrashBundle {
    pub fn new(
        frame: JetGameFrameIdentity,
        reason: impl Into<String>,
        target: impl Into<String>,
        upload_policy: JetGameCrashUploadPolicy,
    ) -> Result<Self, JetGameFrameProfilerError> {
        let bundle = Self {
            frame,
            context: None,
            reason: reason.into(),
            target: target.into(),
            stacks: Vec::new(),
            logs: Vec::new(),
            upload_policy,
        };
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn with_context(mut self, context: JetGameTraceContext) -> Result<Self, JetGameFrameProfilerError> {
        context.validate()?;
        self.context = Some(context);
        self.validate()?;
        Ok(self)
    }

    pub fn add_stack(&mut self, stack: impl Into<String>) -> Result<(), JetGameFrameProfilerError> {
        if self.stacks.len() >= JET_GAME_FRAME_PROFILER_MAX_CRASH_STACKS {
            return Err(JetGameFrameProfilerError::CrashBundleTooLarge {
                field: "stacks",
                max: JET_GAME_FRAME_PROFILER_MAX_CRASH_STACKS,
            });
        }
        let stack = stack.into();
        jet_game_frame_profiler_require_error_pause_text(&stack, "crash stack")?;
        self.stacks.push(stack);
        Ok(())
    }

    pub fn add_log(&mut self, log: impl Into<String>) -> Result<(), JetGameFrameProfilerError> {
        if self.logs.len() >= JET_GAME_FRAME_PROFILER_MAX_CRASH_LOGS {
            return Err(JetGameFrameProfilerError::CrashBundleTooLarge {
                field: "logs",
                max: JET_GAME_FRAME_PROFILER_MAX_CRASH_LOGS,
            });
        }
        let log = log.into();
        jet_game_frame_profiler_require_error_pause_text(&log, "crash log")?;
        self.logs.push(log);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        self.frame.validate()?;
        if let Some(context) = &self.context {
            context.validate()?;
            if context.scene != self.frame.scene {
                return Err(JetGameFrameProfilerError::InvalidTraceContext {
                    reason: "crash context scene does not match frame",
                });
            }
        }
        jet_game_frame_profiler_require_error_pause_text(&self.reason, "crash reason")?;
        jet_game_frame_profiler_require_error_pause_text(&self.target, "crash target")?;
        if self.stacks.len() > JET_GAME_FRAME_PROFILER_MAX_CRASH_STACKS
            || self.logs.len() > JET_GAME_FRAME_PROFILER_MAX_CRASH_LOGS
        {
            return Err(JetGameFrameProfilerError::CrashBundleTooLarge {
                field: "contents",
                max: JET_GAME_FRAME_PROFILER_MAX_CRASH_LOGS,
            });
        }
        self.upload_policy.validate()?;
        Ok(())
    }

    pub fn render_json(&self) -> String {
        let stacks = self
            .stacks
            .iter()
            .map(|stack| jet_game_frame_profiler_json_string(stack))
            .collect::<Vec<_>>()
            .join(",");
        let logs = self
            .logs
            .iter()
            .map(|log| jet_game_frame_profiler_json_string(log))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"frame\":{},\"context\":{},\"reason\":{},\"target\":{},\
\"stacks\":[{}],\"logs\":[{}],\"upload_policy\":{}}}",
            jet_game_trace_frame_json(&self.frame),
            self.context
                .as_ref()
                .map(JetGameTraceContext::render_json)
                .unwrap_or_else(|| "null".to_string()),
            jet_game_frame_profiler_json_string(&self.reason),
            jet_game_frame_profiler_json_string(&self.target),
            stacks,
            logs,
            self.upload_policy.render_json(),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameFrameReceipt {
    pub sequence: u64,
    pub kind: JetGameFrameReceiptKind,
    pub before: JetGameFrameProfilerState,
    pub after: JetGameFrameProfilerState,
    pub trace_identity: JetGameTraceIdentity,
    pub frame: Option<JetGameFrameIdentity>,
    pub sample_sequence: Option<u64>,
    pub error_pause: Option<JetGameErrorPause>,
    pub dropped_samples: u64,
}

#[derive(Clone, Debug)]
pub struct JetGameFrameProfiler {
    trace_identity: JetGameTraceIdentity,
    context: Option<JetGameTraceContext>,
    history: JetGameFrameHistory,
    state: JetGameFrameProfilerState,
    last_frame: Option<JetGameFrameIdentity>,
    error_pause: Option<JetGameErrorPause>,
    receipts: VecDeque<JetGameFrameReceipt>,
    next_receipt_sequence: u64,
}

impl JetGameFrameProfiler {
    pub fn new(identity: impl Into<JetGameTraceIdentity>) -> Self {
        Self::with_capacity(identity, JET_GAME_FRAME_PROFILER_MAX_HISTORY)
    }

    pub fn with_capacity(
        identity: impl Into<JetGameTraceIdentity>,
        history_capacity: usize,
    ) -> Self {
        let trace_identity = identity.into();
        Self {
            history: JetGameFrameHistory::with_capacity(trace_identity.clone(), history_capacity),
            trace_identity,
            context: None,
            state: JetGameFrameProfilerState::Running,
            last_frame: None,
            error_pause: None,
            receipts: VecDeque::new(),
            next_receipt_sequence: 1,
        }
    }

    pub fn trace_identity(&self) -> &JetGameTraceIdentity {
        &self.trace_identity
    }
    pub fn context(&self) -> Option<&JetGameTraceContext> {
        self.context.as_ref()
    }

    pub fn set_context(
        &mut self,
        context: JetGameTraceContext,
    ) -> Result<(), JetGameFrameProfilerError> {
        context.validate()?;
        if let Some(frame) = &self.last_frame {
            if frame.scene != context.scene {
                return Err(JetGameFrameProfilerError::InvalidTraceContext {
                    reason: "trace scene does not match the profiler frame stream",
                });
            }
        }
        self.context = Some(context);
        Ok(())
    }

    pub fn state(&self) -> JetGameFrameProfilerState {
        self.state
    }

    pub fn history(&self) -> &JetGameFrameHistory {
        &self.history
    }

    pub fn receipts(&self) -> impl Iterator<Item = &JetGameFrameReceipt> {
        self.receipts.iter()
    }
    /// A frame is debugger-authoritative only after the profiler has paused.
    /// Running sessions intentionally expose no paused-frame authority.
    pub fn paused_frame(&self) -> Option<&JetGameFrameIdentity> {
        if self.state == JetGameFrameProfilerState::Running {
            None
        } else {
            self.last_frame.as_ref()
        }
    }

    pub fn last_frame(&self) -> Option<&JetGameFrameIdentity> {
        self.last_frame.as_ref()
    }

    pub fn active_error_pause(&self) -> Option<&JetGameErrorPause> {
        self.error_pause.as_ref()
    }

    pub fn record(&mut self, fact: JetGameSampleFact) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        if self.state != JetGameFrameProfilerState::Running {
            return Err(JetGameFrameProfilerError::ProfilerPaused);
        }
        let kind = match fact.kind() {
            JetGameSampleKind::Frame => JetGameFrameReceiptKind::Frame,
            JetGameSampleKind::Phase => JetGameFrameReceiptKind::Phase,
            JetGameSampleKind::Function => JetGameFrameReceiptKind::Function,
            JetGameSampleKind::Draw => JetGameFrameReceiptKind::Draw,
        };
        let frame = fact.frame_identity().clone();
        let sample_sequence = self.history.append(fact)?;
        self.last_frame = Some(frame.clone());
        let state = self.state;
        Ok(self.push_receipt(
            kind,
            state,
            state,
            Some(frame),
            Some(sample_sequence),
            self.error_pause.clone(),
        ))
    }

    pub fn record_frame(
        &mut self,
        sample: JetGameFrameSample,
    ) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        self.record(sample.into())
    }

    pub fn record_phase(
        &mut self,
        sample: JetGamePhaseSample,
    ) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        self.record(sample.into())
    }

    pub fn record_function(
        &mut self,
        sample: JetGameFunctionSample,
    ) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        self.record(sample.into())
    }

    pub fn record_draw(
        &mut self,
        event: JetGameDrawEvent,
    ) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        self.record(event.into())
    }

    pub fn pause(&mut self) -> JetGameFrameReceipt {
        let before = self.state;
        if self.state == JetGameFrameProfilerState::Running {
            self.state = JetGameFrameProfilerState::Paused;
        }
        self.push_receipt(
            JetGameFrameReceiptKind::Paused,
            before,
            self.state,
            self.last_frame.clone(),
            None,
            self.error_pause.clone(),
        )
    }

    pub fn resume(&mut self) -> JetGameFrameReceipt {
        let before = self.state;
        self.state = JetGameFrameProfilerState::Running;
        self.error_pause = None;
        self.push_receipt(
            JetGameFrameReceiptKind::Resumed,
            before,
            self.state,
            self.last_frame.clone(),
            None,
            None,
        )
    }

    pub fn error_pause(
        &mut self,
        source: JetGameSourceSpan,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        let pause = JetGameErrorPause::new(source, code, message)?;
        let before = self.state;
        self.state = JetGameFrameProfilerState::ErrorPaused;
        self.error_pause = Some(pause.clone());
        Ok(self.push_receipt(
            JetGameFrameReceiptKind::ErrorPaused,
            before,
            self.state,
            self.last_frame.clone(),
            None,
            Some(pause),
        ))
    }

    pub fn draw_cursor(
        &self,
        frame: JetGameFrameIdentity,
    ) -> Result<JetGameDrawCursor, JetGameFrameProfilerError> {
        if self.state == JetGameFrameProfilerState::Running {
            return Err(JetGameFrameProfilerError::ProfilerNotPaused);
        }
        frame.validate()?;
        jet_game_frame_profiler_check_trace(&self.trace_identity, &frame.trace_identity())?;
        if self.history.frame_facts(&frame)?.is_empty() {
            return Err(JetGameFrameProfilerError::FrameNotRetained {
                frame_id: frame.frame_id,
            });
        }
        Ok(JetGameDrawCursor::new(frame))
    }

    pub fn bottleneck(&self, limit: usize) -> JetGameBottleneckProjection {
        self.history.bottleneck(limit)
    }


    fn push_receipt(
        &mut self,
        kind: JetGameFrameReceiptKind,
        before: JetGameFrameProfilerState,
        after: JetGameFrameProfilerState,
        frame: Option<JetGameFrameIdentity>,
        sample_sequence: Option<u64>,
        error_pause: Option<JetGameErrorPause>,
    ) -> JetGameFrameReceipt {
        let sequence = self.next_receipt_sequence;
        self.next_receipt_sequence = self.next_receipt_sequence.saturating_add(1);
        let receipt = JetGameFrameReceipt {
            sequence,
            kind,
            before,
            after,
            trace_identity: self.trace_identity.clone(),
            frame,
            sample_sequence,
            error_pause,
            dropped_samples: self.history.dropped_samples(),
        };
        if self.receipts.len() == JET_GAME_FRAME_PROFILER_MAX_RECEIPTS {
            self.receipts.pop_front();
        }
        self.receipts.push_back(receipt.clone());
        receipt
    }
}

impl From<JetGameFrameIdentity> for JetGameTraceIdentity {
    fn from(frame: JetGameFrameIdentity) -> Self {
        frame.trace_identity()
    }
}

impl From<&JetGameFrameIdentity> for JetGameTraceIdentity {
    fn from(frame: &JetGameFrameIdentity) -> Self {
        frame.trace_identity()
    }
}

fn jet_game_frame_profiler_require_text(
    value: &str,
    field: &'static str,
) -> Result<(), JetGameFrameProfilerError> {
    if value.is_empty() {
        Err(JetGameFrameProfilerError::EmptyIdentity { field })
    } else {
        Ok(())
    }
}

fn jet_game_frame_profiler_require_error_pause_text(
    value: &str,
    field: &'static str,
) -> Result<(), JetGameFrameProfilerError> {
    if value.is_empty() {
        Err(JetGameFrameProfilerError::InvalidErrorPause { field })
    } else {
        Ok(())
    }
}

fn jet_game_frame_profiler_check_trace(
    expected: &JetGameTraceIdentity,
    actual: &JetGameTraceIdentity,
) -> Result<(), JetGameFrameProfilerError> {
    if expected.build != actual.build {
        return Err(JetGameFrameProfilerError::BuildMismatch {
            expected: expected.build.clone(),
            actual: actual.build.clone(),
        });
    }
    if expected.revision != actual.revision {
        return Err(JetGameFrameProfilerError::RevisionMismatch {
            expected: expected.revision.clone(),
            actual: actual.revision.clone(),
        });
    }
    if expected.trace_id != actual.trace_id {
        return Err(JetGameFrameProfilerError::TraceMismatch {
            expected: expected.trace_id.clone(),
            actual: actual.trace_id.clone(),
        });
    }
    Ok(())
}

fn jet_game_frame_profiler_check_frame(
    expected: &JetGameFrameIdentity,
    actual: &JetGameFrameIdentity,
) -> Result<(), JetGameFrameProfilerError> {
    jet_game_frame_profiler_check_trace(&expected.trace_identity(), &actual.trace_identity())?;
    if expected != actual {
        return Err(JetGameFrameProfilerError::FrameMismatch {
            expected: expected.clone(),
            actual: actual.clone(),
        });
    }
    Ok(())
}

fn jet_game_frame_profiler_frame_id(
    scene: &str,
    frame_index: u64,
    build: &str,
    revision: &str,
) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in b"jet.game.frame.profiler.v1\0" {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    for value in [scene, build, revision] {
        for byte in (value.len() as u64).to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        for byte in value.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    for byte in frame_index.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn jet_game_frame_profiler_draw_id(frame_id: u64, event_index: u64) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in b"jet.game.draw.v1\0" {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    for byte in frame_id.to_le_bytes().into_iter().chain(event_index.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}


fn jet_game_trace_frame_json(frame: &JetGameFrameIdentity) -> String {
    format!(
        "{{\"frame_id\":{},\"scene\":{},\"frame_index\":{},\"build\":{},\
\"revision\":{},\"trace_id\":{}}}",
        frame.frame_id,
        jet_game_frame_profiler_json_string(&frame.scene),
        frame.frame_index,
        jet_game_frame_profiler_json_string(&frame.build),
        jet_game_frame_profiler_json_string(&frame.revision),
        jet_game_frame_profiler_json_string(&frame.trace_id),
    )
}



fn jet_game_frame_profiler_json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}


/// Stable marker carried by every generated game binary that has the package
/// crash-reporter bootstrap.  The package adapter checks this marker before it
/// claims an active reporter is installed.
pub const JET_GAME_CRASH_REPORTER_ABI: &str = "jet-game-crash-reporter-v1";
const JET_GAME_CRASH_REPORTER_MAX_PERSISTED: usize = 1;
const JET_GAME_CRASH_REPORTER_MAX_TEXT: usize = 4096;

/// The installed game crash policy.  This is read from the packaged
/// `jet-game.json`; it is not inferred from the host environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameCrashReporterConfig {
    pub reporter: String,
    pub consent: String,
    pub target: String,
    pub build: String,
    pub revision: String,
    pub development_stripping: bool,
    pub upload_policy: JetGameCrashUploadPolicy,
    pub retention_mode: String,
    pub retention_path: String,
    pub retention_limit: usize,
}

impl JetGameCrashReporterConfig {
    pub fn new(
        reporter: impl Into<String>,
        consent: impl Into<String>,
        target: impl Into<String>,
        build: impl Into<String>,
        revision: impl Into<String>,
        development_stripping: bool,
        upload_policy: JetGameCrashUploadPolicy,
        retention_mode: impl Into<String>,
        retention_path: impl Into<String>,
        retention_limit: usize,
    ) -> Self {
        Self {
            reporter: reporter.into(),
            consent: consent.into(),
            target: target.into(),
            build: build.into(),
            revision: revision.into(),
            development_stripping,
            upload_policy,
            retention_mode: retention_mode.into(),
            retention_path: retention_path.into(),
            retention_limit,
        }
    }

    pub fn active(&self) -> bool {
        self.reporter != "off" && self.consent == "granted"
    }

    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.reporter.as_str(), "off" | "on" | "opt-in") {
            return Err(format!("unknown game crash reporter `{}`", self.reporter));
        }
        if !matches!(
            self.consent.as_str(),
            "not-requested" | "granted" | "denied"
        ) {
            return Err(format!("unknown game crash consent `{}`", self.consent));
        }
        if self.reporter == "on" && self.consent != "granted" {
            return Err("game crash reporter `on` requires granted consent".into());
        }
        if self.reporter == "off" && self.consent == "granted" {
            return Err("game crash consent cannot be granted when the reporter is off".into());
        }
        self.upload_policy
            .validate()
            .map_err(|error| error.message())?;
        jet_game_frame_profiler_require_text(&self.target, "crash target")
            .map_err(|error| error.message())?;
        jet_game_frame_profiler_require_text(&self.build, "crash build")
            .map_err(|error| error.message())?;
        jet_game_frame_profiler_require_text(&self.revision, "crash revision")
            .map_err(|error| error.message())?;
        if !self.active() {
            return Ok(());
        }
        if !matches!(self.upload_policy, JetGameCrashUploadPolicy::Never) {
            return Err(
                "game crash upload requires an explicit transport adapter; package runtime stays local"
                    .into(),
            );
        }
        if self.retention_mode != "bounded-persisted" {
            return Err("active game crash reporter requires bounded persisted retention".into());
        }
        if self.retention_limit != JET_GAME_CRASH_REPORTER_MAX_PERSISTED {
            return Err(format!(
                "game crash reporter retention must keep exactly {} report",
                JET_GAME_CRASH_REPORTER_MAX_PERSISTED
            ));
        }
        jet_game_crash_reporter_validate_relative_path(&self.retention_path)?;
        Ok(())
    }
}

static JET_GAME_CRASH_REPORTER_STATE: OnceLock<Mutex<JetGameCrashReporterConfig>> = OnceLock::new();
static JET_GAME_CRASH_REPORTER_HOOK: OnceLock<()> = OnceLock::new();

/// Install the one process-wide panic hook for an active, consented package
/// reporter.  `off`, denied, and not-requested configurations are accepted as
/// inert configurations and do not install a hook or write a report.
pub fn jet_game_crash_reporter_install(
    config: JetGameCrashReporterConfig,
) -> Result<(), String> {
    config.validate()?;
    if !config.active() {
        return Ok(());
    }
    let state = JET_GAME_CRASH_REPORTER_STATE.get_or_init(|| Mutex::new(config.clone()));
    {
        let existing = state
            .lock()
            .map_err(|_| "game crash reporter state is poisoned".to_string())?;
        if *existing != config {
            return Err("game crash reporter was already installed with different package facts".into());
        }
    }
    if JET_GAME_CRASH_REPORTER_HOOK.set(()).is_ok() {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                jet_game_crash_reporter_capture(info);
            }));
            previous(info);
        }));
    }
    Ok(())
}

/// Resolve and install the checked reporter configuration from the package
/// manifest beside the generated executable.  Missing manifests are a no-op,
/// which keeps ordinary `jet run` game binaries reporter-free.
pub fn jet_game_crash_reporter_install_from_manifest() {
    for path in jet_game_crash_manifest_paths() {
        let Ok(manifest) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(config) = jet_game_crash_reporter_config_from_manifest(&manifest) else {
            continue;
        };
        let _ = jet_game_crash_reporter_install(config);
        return;
    }
}

fn jet_game_crash_reporter_config_from_manifest(
    manifest: &str,
) -> Option<JetGameCrashReporterConfig> {
    let runtime = manifest.split_once("\"crash_runtime\"")?.1;
    if jet_game_crash_manifest_string(runtime, "abi")?.as_str() != JET_GAME_CRASH_REPORTER_ABI {
        return None;
    }
    if jet_game_crash_manifest_string(runtime, "installation")?.as_str() != "installed" {
        return None;
    }
    let reporter = jet_game_crash_manifest_string(manifest, "crash_reporter")?;
    let consent = jet_game_crash_manifest_string(manifest, "crash_consent")?;
    if jet_game_crash_manifest_string(runtime, "routing")?.as_str() != "local" {
        return None;
    }
    let target = jet_game_crash_manifest_string(manifest, "target")?;
    let build = jet_game_crash_manifest_string(manifest, "build_identity")?;
    let revision = jet_game_crash_manifest_string(manifest, "source_revision")?;
    let development_stripping = jet_game_crash_manifest_bool(manifest, "development_stripping")?;
    let policy = match jet_game_crash_manifest_string(runtime, "upload_policy")?.as_str() {
        "never" => JetGameCrashUploadPolicy::Never,
        "loopback-only" => JetGameCrashUploadPolicy::LoopbackOnly,
        "explicit" => JetGameCrashUploadPolicy::Explicit(
            jet_game_crash_manifest_string(runtime, "upload_endpoint")?,
        ),
        _ => return None,
    };
    let retention = runtime.split_once("\"retention\"")?.1;
    let retention_mode = jet_game_crash_manifest_string(retention, "mode")?;
    let retention_path = jet_game_crash_manifest_string(retention, "path")?;
    let retention_limit = jet_game_crash_manifest_integer(retention, "max_reports")?;
    Some(JetGameCrashReporterConfig::new(
        reporter,
        consent,
        target,
        build,
        revision,
        development_stripping,
        policy,
        retention_mode,
        retention_path,
        retention_limit,
    ))
}

fn jet_game_crash_reporter_capture(info: &std::panic::PanicHookInfo<'_>) {
    let Some(state) = JET_GAME_CRASH_REPORTER_STATE.get() else {
        return;
    };
    let config = match state.lock() {
        Ok(config) => config.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let frame = JetGameFrameIdentity::new(
        "runtime",
        0,
        &config.build,
        &config.revision,
        "crash-runtime",
    );
    let Ok(mut bundle) = JetGameCrashBundle::new(
        frame,
        "panic",
        &config.target,
        config.upload_policy.clone(),
    ) else {
        return;
    };
    let panic_text = info
        .payload()
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
        .unwrap_or("panic payload unavailable");
    let panic_text =
        jet_game_crash_reporter_truncate(panic_text, JET_GAME_CRASH_REPORTER_MAX_TEXT);
    let location = info
        .location()
        .map(|location| format!("panic at line {}, column {}", location.line(), location.column()))
        .unwrap_or_else(|| "panic location unavailable".into());
    let panic_log = format!("{location}: {panic_text}");
    let _ = bundle.add_log(jet_game_crash_reporter_truncate(
        &panic_log,
        JET_GAME_CRASH_REPORTER_MAX_TEXT,
    ));
    if !config.development_stripping {
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        if !backtrace.is_empty() {
            let _ = bundle.add_stack(jet_game_crash_reporter_truncate(
                &backtrace,
                JET_GAME_CRASH_REPORTER_MAX_TEXT,
            ));
        }
    }
    let rendered = bundle.render_json();
    let persisted = format!(
        "{{\"runtime\":{},\"reporter\":{},\"consent\":{},\"target\":{},\
\"memory\":{{\"max_stacks\":{},\"max_logs\":{}}},\
\"retention\":{{\"mode\":{},\"path\":{},\"max_reports\":{}}},\
\"upload\":{{\"policy\":{},\"routing\":{}}},\"bundle\":{}}}",
        jet_game_frame_profiler_json_string(JET_GAME_CRASH_REPORTER_ABI),
        jet_game_frame_profiler_json_string(&config.reporter),
        jet_game_frame_profiler_json_string(&config.consent),
        jet_game_frame_profiler_json_string(&config.target),
        JET_GAME_FRAME_PROFILER_MAX_CRASH_STACKS,
        JET_GAME_FRAME_PROFILER_MAX_CRASH_LOGS,
        jet_game_frame_profiler_json_string(&config.retention_mode),
        jet_game_frame_profiler_json_string(&config.retention_path),
        config.retention_limit,
        config.upload_policy.render_json(),
        jet_game_frame_profiler_json_string(jet_game_crash_reporter_route(
            &config.upload_policy,
        )),
        rendered,
    );
    jet_game_crash_reporter_persist(&config, persisted.as_bytes());
}

fn jet_game_crash_reporter_persist(
    config: &JetGameCrashReporterConfig,
    bytes: &[u8],
) {
    if config.retention_mode != "bounded-persisted"
        || config.retention_limit != JET_GAME_CRASH_REPORTER_MAX_PERSISTED
    {
        return;
    }
    let base = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok());
    let Some(base) = base else {
        return;
    };
    let directory = base.join(&config.retention_path);
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let destination = directory.join("latest.json");
    let temporary = directory.join(format!(".latest-{}.tmp", std::process::id()));
    if std::fs::write(&temporary, bytes).is_ok() {
        let _ = std::fs::rename(&temporary, destination);
    }
}

fn jet_game_crash_reporter_route(policy: &JetGameCrashUploadPolicy) -> &'static str {
    match policy {
        JetGameCrashUploadPolicy::Never => "local",
        JetGameCrashUploadPolicy::LoopbackOnly => "loopback",
        JetGameCrashUploadPolicy::Explicit(_) => "explicit",
    }
}

fn jet_game_crash_reporter_validate_relative_path(path: &str) -> Result<(), String> {
    let path_view = std::path::Path::new(path);
    if path.is_empty()
        || path_view.is_absolute()
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("game crash retention path `{path}` is not a safe relative path"));
    }
    Ok(())
}

fn jet_game_crash_reporter_truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn jet_game_crash_manifest_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    let mut push = |path: std::path::PathBuf| {
        if !paths.contains(&path) {
            paths.push(path);
        }
    };
    if let Some(appdir) = std::env::var_os("APPDIR") {
        let appdir = std::path::PathBuf::from(appdir);
        if let Ok(entries) = std::fs::read_dir(appdir.join("usr").join("share")) {
            for entry in entries.flatten().take(32) {
                push(entry.path().join("jet-game.json"));
            }
        }
        push(appdir.join("jet").join("jet-game.json"));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            push(parent.join("jet-game.json"));
            push(parent.join("jet").join("jet-game.json"));
            push(parent.join("../Resources/jet-game.json"));
            if let Some(usr) = parent.parent() {
                if let Ok(entries) = std::fs::read_dir(usr.join("share")) {
                    for entry in entries.flatten().take(32) {
                        push(entry.path().join("jet-game.json"));
                    }
                }
            }
        }
    }
    if let Ok(current) = std::env::current_dir() {
        push(current.join("jet-game.json"));
    }
    paths
}

fn jet_game_crash_manifest_string(source: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = source.find(&needle)? + needle.len();
    let value = source[start..].trim_start().strip_prefix(':')?.trim_start();
    let mut chars = value.strip_prefix('"')?.chars();
    let mut output = String::new();
    let mut escaped = false;
    while let Some(character) = chars.next() {
        if escaped {
            output.push(match character {
                '"' => '"',
                '\\' => '\\',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return Some(output);
        } else {
            output.push(character);
        }
    }
    None
}

fn jet_game_crash_manifest_integer(source: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let start = source.find(&needle)? + needle.len();
    let value = source[start..].trim_start().strip_prefix(':')?.trim_start();
    let digits = value
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

fn jet_game_crash_manifest_bool(source: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{key}\"");
    let start = source.find(&needle)? + needle.len();
    let value = source[start..].trim_start().strip_prefix(':')?.trim_start();
    if value.starts_with("true") {
        Some(true)
    } else if value.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod jet_game_frame_profiler_tests {
    use super::*;

    fn trace() -> JetGameTraceIdentity {
        JetGameTraceIdentity::new("build-1", "revision-1", "trace-1")
    }

    fn frame(index: u64) -> JetGameFrameIdentity {
        JetGameFrameIdentity::new("scene", index, "build-1", "revision-1", "trace-1")
    }

    fn function() -> JetGameFunctionIdentity {
        JetGameFunctionIdentity::new(
            "fn/player/update",
            "player.update",
            JetGameSourceSpan::new("src/player.jet:4", "src/player.jet", 4, 1, 8, 1),
        )
    }

    #[test]
    fn identities_are_stable_and_hot_paths_use_integer_ns() {
        let first = frame(7);
        let second = frame(7);
        assert_eq!(first, second);
        assert_eq!(first.frame_id, second.frame_id);

        let mut history = JetGameFrameHistory::with_capacity(trace(), 8);
        history
            .append(
                JetGameFunctionSample::new(
                    first.clone(),
                    function(),
                    JetGameFramePhase::Update,
                    JetGameTimeDomain::Cpu,
                    10,
                    40,
                )
                .into(),
            )
            .expect("function sample");
        history
            .append(
                JetGameFunctionSample::new(
                    first,
                    function(),
                    JetGameFramePhase::Update,
                    JetGameTimeDomain::Cpu,
                    50,
                    60,
                )
                .into(),
            )
            .expect("function sample");
        let paths = history.hot_paths();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].total_ns, 100);
        assert_eq!(paths[0].sample_count, 2);
        assert_eq!(history.bottleneck(1).total_function_ns, 100);
    }

    #[test]
    fn history_reports_drops_and_cursor_refuses_other_revision() {
        let first = frame(0);
        let function = function();
        let mut history = JetGameFrameHistory::with_capacity(trace(), 2);
        history
            .append(
                JetGameFrameSample::new(first.clone(), 0, 100, Some(80)).into(),
            )
            .expect("frame sample");
        history
            .append(
                JetGameDrawEvent::new(
                    first.clone(),
                    0,
                    function.clone(),
                    JetGameFramePhase::Draw,
                    JetGameTimeDomain::Gpu,
                    100,
                    10,
                )
                .into(),
            )
            .expect("draw event");
        history
            .append(
                JetGamePhaseSample::new(
                    first.clone(),
                    JetGameFramePhase::Present,
                    JetGameTimeDomain::Cpu,
                    110,
                    5,
                )
                .into(),
            )
            .expect("phase sample");
        assert_eq!(history.len(), 2);
        assert_eq!(history.dropped_samples(), 1);

        let events = history.draw_events_for(&first).expect("draw events");
        let mut cursor = JetGameDrawCursor::new(first.clone());
        assert_eq!(cursor.step_forward(&events).unwrap().unwrap().event_index, 0);
        assert!(cursor.step_forward(&events).unwrap().is_none());
        assert!(cursor.step_backward(&events).unwrap().is_some());

        let other_revision = JetGameFrameIdentity::new(
            "scene",
            0,
            "build-1",
            "revision-2",
            "trace-1",
        );
        let other_event = JetGameDrawEvent::new(
            other_revision,
            0,
            function,
            JetGameFramePhase::Draw,
            JetGameTimeDomain::Gpu,
            100,
            10,
        );
        let error = cursor.step_forward(&[other_event]).unwrap_err();
        assert!(matches!(error, JetGameFrameProfilerError::RevisionMismatch { .. }));
    }

    #[test]
    fn profiler_receipts_preserve_pause_and_error_identity() {
        let mut profiler = JetGameFrameProfiler::with_capacity(trace(), 4);
        profiler
            .record_frame(JetGameFrameSample::new(frame(0), 0, 10, None))
            .expect("frame sample");
        let paused = profiler.pause();
        assert_eq!(paused.before, JetGameFrameProfilerState::Running);
        assert_eq!(paused.after, JetGameFrameProfilerState::Paused);
        assert!(matches!(
            profiler.record_frame(JetGameFrameSample::new(frame(1), 20, 10, None)),
            Err(JetGameFrameProfilerError::ProfilerPaused)
        ));
        let source = JetGameSourceSpan::new("src/player.jet:4", "src/player.jet", 4, 1, 4, 8);
        let error_pause = profiler
            .error_pause(source, "E3001", "player update failed")
            .expect("error pause");
        assert_eq!(error_pause.after, JetGameFrameProfilerState::ErrorPaused);
        assert_eq!(error_pause.trace_identity, *profiler.trace_identity());
        assert!(profiler.active_error_pause().is_some());
        let resumed = profiler.resume();
        assert_eq!(resumed.after, JetGameFrameProfilerState::Running);
        assert!(profiler.active_error_pause().is_none());
    }
}
