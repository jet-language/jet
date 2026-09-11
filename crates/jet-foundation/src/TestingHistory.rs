//! Typed operation-history generation, dependency-aware shrinking, and replay artifacts.
//!
//! A history is a bounded sequence of checked operations, not a byte-fuzzing
//! corpus.  The operation metadata keeps resource handles, task/event
//! dependencies, and schedule choices visible while the runner delegates the
//! verdict to [`crate::TestingComparison`].  The model and candidate are
//! supplied by the caller; this module never derives an oracle from either one.

use crate::JSON::quote;
use crate::SHA256;
use crate::TestingComparison::{
    compare_samples, ComparisonIdentity, ComparisonObservation, ComparisonRecord,
    ComparisonSample, ComparisonStatus, ObservationRelation,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::Path;

// Compiler-private weak metadata indexes. Capture storage stays in the native
// callable environment, including its ordinary checked Rust references. Only
// the index erases a reader lifetime; it never owns or projects a referent.
//
// SAFETY CONTRACT: the native registry may call a reader only while borrowing
// the exact registered callable. That callable owns the typed reader and its
// allocation ticket. Dead tickets cannot match a reused allocation address.
macro_rules! history_callback_reader {
    ($name:ident, $owner:ident, $weak:ident $(, $bound:ident)*) => {
        #[derive(Clone)]
        pub struct $name<R: 'static>($weak<dyn Fn() -> R $(+ $bound)*>);
        impl<R: 'static> $name<R> {
            /// The registered callable must own this reader for its entire
            /// lifetime, and every read must borrow that same callable.
            pub unsafe fn new<'a>(reader: &$owner<dyn Fn() -> R $(+ $bound)* + 'a>) -> Self {
                let weak = <$owner<_>>::downgrade(reader);
                // SAFETY: Weak owns no capture. Its lifetime is restored by
                // the caller's live-callable requirement on read().
                Self(unsafe { std::mem::transmute::<
                    $weak<dyn Fn() -> R $(+ $bound)* + 'a>,
                    $weak<dyn Fn() -> R $(+ $bound)* + 'static>,
                >(weak) })
            }
            /// The exact callable used at registration must remain borrowed
            /// until this call returns, keeping its typed reader alive.
            pub unsafe fn read(&self) -> Option<R> {
                self.0.upgrade().map(|reader| reader())
            }
        }
    };
}
use std::rc::{Rc as HistoryReaderRc, Weak as HistoryReaderWeak};
use std::sync::{Arc as HistoryReaderArc, Weak as HistorySendReaderWeak};
history_callback_reader!(HistoryCallbackReader, HistoryReaderRc, HistoryReaderWeak);
history_callback_reader!(HistorySendCallbackReader, HistoryReaderArc, HistorySendReaderWeak, Send, Sync);

pub const HISTORY_SCHEMA: &str = "jet.testing.history";
pub const HISTORY_VERSION: u16 = 1;
pub const HISTORY_ENGINE: &str = "typed-history-v1";
const MAX_CASES: usize = 100_000;
const MAX_STEPS: usize = 4_096;
const MAX_RESOURCES: usize = 4_096;
const MAX_DISTRIBUTIONS: usize = 256;
const MAX_SHRINK_ATTEMPTS: usize = 1_000_000;
const MAX_STRING_BYTES: usize = 4_096;

/// Errors returned before a history can be claimed as evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistoryError {
    InvalidConfig(String),
    InvalidCase(String),
    InvalidOracle(String),
    NoFailure,
    Io(String),
}

impl fmt::Display for HistoryError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(output, "invalid history config: {message}"),
            Self::InvalidCase(message) => write!(output, "invalid history case: {message}"),
            Self::InvalidOracle(message) => write!(output, "invalid history oracle: {message}"),
            Self::NoFailure => output.write_str("history did not contain a mismatch"),
            Self::Io(message) => write!(output, "history artifact I/O failed: {message}"),
        }
    }
}

impl std::error::Error for HistoryError {}

/// A typed resource identity retained by operations that create, consume, or
/// inspect a resource.  Numeric identities are intentionally local to one
/// case; they are not secrets or process handles.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct HandleId {
    pub value: u32,
}

/// A logical task identity used by the schedule and precondition metadata.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct TaskId {
    pub value: u32,
}

/// A logical external event identity used by the schedule and precondition
/// metadata.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct EventId {
    pub value: u32,
}
/// Non-negative source-level count values lower to the host `usize` only
/// after the compiler has checked their range.
pub type Count = usize;
/// Compiler-private provenance for one selected history execution.
///
/// `source`, `tool`, and `target` are already canonical digests supplied by
/// the checked program/artifact boundary.  [`Self::bind_callbacks`] derives a
/// new source digest that also commits to the selected callbacks while leaving
/// the toolchain and target identities intact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryProvenance {
    pub source: String,
    pub tool: String,
    pub target: String,
}

/// Hash checked callable metadata after its owner has losslessly encoded live
/// captures. Capture bytes never enter a replay artifact.
pub fn history_callback_value_fingerprint(
    function_identity: &str,
    canonical_capture_bytes: &[u8],
) -> Result<String, HistoryError> {
    if function_identity.is_empty() {
        return Err(HistoryError::InvalidConfig(
            "history callback has no checked function identity".to_string(),
        ));
    }
    let mut payload = Vec::new();
    for bytes in [
        b"jet.history.callback.value.v1".as_slice(),
        function_identity.as_bytes(),
        canonical_capture_bytes,
    ] {
        payload.extend_from_slice(bytes.len().to_string().as_bytes());
        payload.push(b':');
        payload.extend_from_slice(bytes);
    }
    Ok(SHA256::sha256_hex(&payload))
}

impl HistoryProvenance {
    pub fn bind_callbacks(
        &self,
        callbacks: &[(&str, &str)],
    ) -> Result<Self, HistoryError> {
        if self.source.is_empty() || self.tool.is_empty() || self.target.is_empty() {
            return Err(HistoryError::InvalidConfig(
                "history provenance is incomplete".to_string(),
            ));
        }
        let mut callbacks = callbacks
            .iter()
            .map(|(role, identity)| (*role, *identity))
            .collect::<Vec<_>>();
        if callbacks.iter().any(|(role, identity)| {
            role.is_empty()
                || identity.is_empty()
                || role.len() > MAX_STRING_BYTES
                || identity.len() > MAX_STRING_BYTES
        }) {
            return Err(HistoryError::InvalidConfig(
                "history callback provenance is incomplete".to_string(),
            ));
        }
        callbacks.sort_unstable_by(|left, right| left.0.cmp(right.0));
        if callbacks
            .windows(2)
            .any(|pair| pair[0].0 == pair[1].0 || pair[0].1 == pair[1].1)
        {
            return Err(HistoryError::InvalidConfig(
                "history callback provenance contains a duplicate identity".to_string(),
            ));
        }
        let mut payload = Vec::new();
        fn frame(payload: &mut Vec<u8>, value: &str) {
            payload.extend_from_slice(value.len().to_string().as_bytes());
            payload.push(b':');
            payload.extend_from_slice(value.as_bytes());
        }
        frame(&mut payload, "jet.history.provenance.v1");
        frame(&mut payload, &self.source);
        for (role, identity) in callbacks {
            frame(&mut payload, role);
            frame(&mut payload, identity);
        }
        Ok(Self {
            source: SHA256::sha256_hex(&payload),
            tool: self.tool.clone(),
            target: self.target.clone(),
        })
    }
}

/// Values carried by an operation.  Callers must use [`Self::redacted`] for a
/// secret; the original secret is never retained by this type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistoryValue {
    Integer(i64),
    Boolean(bool),
    /// Text explicitly declared safe for a public replay artifact.
    Text(String),
    Handle(HandleId),
    Redacted(String),
}

impl HistoryValue {
    pub fn public_text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    /// Discard a sensitive value before it enters the history.  Only the
    /// caller-provided non-sensitive label is persisted.
    pub fn redacted(label: impl Into<String>) -> Self {
        Self::Redacted(label.into())
    }

    /// Accept a sensitive value only to replace it immediately with a label.
    /// This makes accidental artifact serialization of the value impossible.
    pub fn secret(_value: impl AsRef<str>, label: impl Into<String>) -> Self {
        Self::redacted(label)
    }

    fn handles(&self) -> Option<HandleId> {
        match self {
            Self::Handle(handle) => Some(*handle),
            _ => None,
        }
    }

    fn json(&self) -> String {
        match self {
            Self::Integer(value) => value.to_string(),
            Self::Boolean(value) => value.to_string(),
            Self::Text(value) => quote(value),
            Self::Handle(handle) => handle.value.to_string(),
            Self::Redacted(label) => format!("{{\"redacted\":{}}}", quote(label)),
        }
    }

    fn shrinks(&self) -> Vec<Self> {
        match self {
            Self::Integer(value) => {
                let mut values = vec![Self::Integer(0)];
                if *value != 0 {
                    values.push(Self::Integer(*value / 2));
                }
                values.dedup();
                values
            }
            Self::Boolean(value) if *value => vec![Self::Boolean(false)],
            Self::Boolean(_) | Self::Handle(_) | Self::Redacted(_) => Vec::new(),
            Self::Text(value) if !value.is_empty() => {
                let half = value.chars().take(value.chars().count() / 2).collect();
                vec![Self::Text(String::new()), Self::Text(half)]
            }
            Self::Text(_) => Vec::new(),
        }
    }
}

/// A precondition that must hold immediately before an operation runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistoryPrecondition {
    HandleLive(HandleId),
    HandleState { handle: HandleId, state: String },
    TaskCompleted(TaskId),
    EventAvailable(EventId),
}

/// One operation in a case.  `depends_on` captures explicit task/event order;
/// handle references in `arguments`, `creates`, and `consumes` capture object
/// lifetimes for the shrinker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryOperation {
    pub index: u32,
    pub name: String,
    pub arguments: Vec<HistoryValue>,
    pub creates: Vec<HandleId>,
    pub consumes: Vec<HandleId>,
    pub preconditions: Vec<HistoryPrecondition>,
    pub depends_on: Vec<u32>,
    pub task: Option<TaskId>,
    pub event: Option<EventId>,
}

impl HistoryOperation {
    pub fn new(index: u32, name: impl Into<String>) -> Self {
        Self {
            index,
            name: name.into(),
            arguments: Vec::new(),
            creates: Vec::new(),
            consumes: Vec::new(),
            preconditions: Vec::new(),
            depends_on: Vec::new(),
            task: None,
            event: None,
        }
    }

    pub fn with_argument(mut self, value: HistoryValue) -> Self {
        self.arguments.push(value);
        self
    }

    pub fn creates(mut self, handle: HandleId) -> Self {
        self.creates.push(handle);
        self
    }

    pub fn consumes(mut self, handle: HandleId) -> Self {
        self.consumes.push(handle);
        self
    }

    pub fn requires(mut self, precondition: HistoryPrecondition) -> Self {
        self.preconditions.push(precondition);
        self
    }

    pub fn depends_on(mut self, operation: u32) -> Self {
        self.depends_on.push(operation);
        self
    }

    pub fn scheduled_on(mut self, task: TaskId, event: EventId) -> Self {
        self.task = Some(task);
        self.event = Some(event);
        self
    }

    fn references_any_handle(&self, handles: &BTreeSet<HandleId>) -> bool {
        self.creates.iter().any(|handle| handles.contains(handle))
            || self.consumes.iter().any(|handle| handles.contains(handle))
            || self
                .arguments
                .iter()
                .filter_map(HistoryValue::handles)
                .any(|handle| handles.contains(&handle))
            || self.preconditions.iter().any(|precondition| match precondition {
                HistoryPrecondition::HandleLive(handle)
                | HistoryPrecondition::HandleState { handle, .. } => handles.contains(handle),
                HistoryPrecondition::TaskCompleted(_) | HistoryPrecondition::EventAvailable(_) => {
                    false
                }
            })
    }

    fn json(&self) -> String {
        let arguments = self
            .arguments
            .iter()
            .map(HistoryValue::json)
            .collect::<Vec<_>>()
            .join(",");
        let creates = self
            .creates
            .iter()
            .map(|handle| handle.value.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let consumes = self
            .consumes
            .iter()
            .map(|handle| handle.value.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let preconditions = self
            .preconditions
            .iter()
            .map(precondition_json)
            .collect::<Vec<_>>()
            .join(",");
        let dependencies = self
            .depends_on
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"arguments\":[{arguments}],\"consumes\":[{consumes}],\"creates\":[{creates}],\"dependsOn\":[{dependencies}],\"event\":{},\"index\":{},\"name\":{},\"preconditions\":[{preconditions}],\"task\":{}}}",
            optional_u32(self.event.map(|event| event.value)),
            self.index,
            quote(&self.name),
            optional_u32(self.task.map(|task| task.value)),
        )
    }
}

fn optional_u32(value: Option<u32>) -> String {
    value.map_or_else(|| "null".to_string(), |value| value.to_string())
}

fn precondition_json(precondition: &HistoryPrecondition) -> String {
    match precondition {
        HistoryPrecondition::HandleLive(handle) => {
            format!("{{\"kind\":\"handle_live\",\"handle\":{}}}", handle.value)
        }
        HistoryPrecondition::HandleState { handle, state } => format!(
            "{{\"handle\":{},\"kind\":\"handle_state\",\"state\":{}}}",
            handle.value,
            quote(state)
        ),
        HistoryPrecondition::TaskCompleted(task) => {
            format!("{{\"kind\":\"task_completed\",\"task\":{}}}", task.value)
        }
        HistoryPrecondition::EventAvailable(event) => format!(
            "{{\"event\":{},\"kind\":\"event_available\"}}",
            event.value
        ),
    }
}

/// One scheduler choice attached to an operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryScheduleChoice {
    pub operation: u32,
    pub task: Option<TaskId>,
    pub event: Option<EventId>,
    pub choice: String,
}

impl HistoryScheduleChoice {
    pub fn new(operation: u32, choice: impl Into<String>) -> Self {
        Self {
            operation,
            task: None,
            event: None,
            choice: choice.into(),
        }
    }

    pub fn for_task_event(mut self, task: TaskId, event: EventId) -> Self {
        self.task = Some(task);
        self.event = Some(event);
        self
    }

    fn json(&self) -> String {
        format!(
            "{{\"choice\":{},\"event\":{},\"operation\":{},\"task\":{}}}",
            quote(&self.choice),
            optional_u32(self.event.map(|event| event.value)),
            self.operation,
            optional_u32(self.task.map(|task| task.value)),
        )
    }
}

/// A complete concrete history case.  The case identity, derived seed, typed
/// operations, and scheduler choices travel together so replay and shrinking
/// never fall back to a seed-only or otherwise synthetic input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryCase {
    pub case_id: String,
    pub seed: u64,
    pub operations: Vec<HistoryOperation>,
    pub schedule: Vec<HistoryScheduleChoice>,
}


impl HistoryCase {
    pub fn new(case_id: impl Into<String>, seed: u64) -> Self {
        Self {
            case_id: case_id.into(),
            seed,
            operations: Vec::new(),
            schedule: Vec::new(),
        }
    }

    pub fn push(&mut self, operation: HistoryOperation) {
        self.operations.push(operation);
    }

    pub fn schedule(&mut self, choice: HistoryScheduleChoice) {
        self.schedule.push(choice);
    }

    /// Validate generic handle and task/event dependencies.  Domain state
    /// preconditions are checked by the scenario callback passed to the runner.
    pub fn validate(&self) -> Result<(), HistoryError> {
        if self.case_id.is_empty() || self.case_id.len() > MAX_STRING_BYTES {
            return Err(HistoryError::InvalidCase(
                "case identity must be non-empty and bounded".to_string(),
            ));
        }
        if self.operations.is_empty() {
            return Err(HistoryError::InvalidCase(
                "case must contain at least one operation".to_string(),
            ));
        }
        if self.operations.len() > MAX_STEPS {
            return Err(HistoryError::InvalidCase(
                "operation count exceeds the history bound".to_string(),
            ));
        }
        let mut live = BTreeSet::new();
        let mut known = BTreeSet::new();
        let mut completed_tasks = BTreeSet::new();
        let mut available_events = BTreeSet::new();
        for (position, operation) in self.operations.iter().enumerate() {
            let expected_index = u32::try_from(position).map_err(|_| {
                HistoryError::InvalidCase("operation index exceeds u32".to_string())
            })?;
            if operation.index != expected_index {
                return Err(HistoryError::InvalidCase(format!(
                    "operation {} has index {}, expected {}",
                    position, operation.index, expected_index
                )));
            }
            if operation.name.is_empty() || operation.name.len() > MAX_STRING_BYTES {
                return Err(HistoryError::InvalidCase(format!(
                    "operation {} has an invalid name",
                    operation.index
                )));
            }
            if operation.arguments.len() > 64
                || operation.creates.len() > MAX_RESOURCES
                || operation.consumes.len() > MAX_RESOURCES
                || operation.preconditions.len() > 64
                || operation.depends_on.len() > 64
            {
                return Err(HistoryError::InvalidCase(format!(
                    "operation {} exceeds its metadata bound",
                    operation.index
                )));
            }
            for argument in &operation.arguments {
                match argument {
                    HistoryValue::Text(value) if value.len() > MAX_STRING_BYTES => {
                        return Err(HistoryError::InvalidCase(format!(
                            "operation {} has an oversized argument",
                            operation.index
                        )));
                    }
                    HistoryValue::Redacted(value)
                        if value.is_empty() || value.len() > MAX_STRING_BYTES =>
                    {
                        return Err(HistoryError::InvalidCase(format!(
                            "operation {} has an invalid redacted argument",
                            operation.index
                        )));
                    }
                    HistoryValue::Handle(handle) if !live.contains(handle) => {
                        return Err(HistoryError::InvalidCase(format!(
                            "operation {} requires non-live argument handle {}",
                            operation.index, handle.value
                        )));
                    }
                    HistoryValue::Integer(_)
                    | HistoryValue::Boolean(_)
                    | HistoryValue::Text(_)
                    | HistoryValue::Handle(_)
                    | HistoryValue::Redacted(_) => {}
                }
            }
            let mut dependencies = BTreeSet::new();
            for dependency in &operation.depends_on {
                if *dependency >= operation.index || !dependencies.insert(*dependency) {
                    return Err(HistoryError::InvalidCase(format!(
                        "operation {} has a duplicate or forward dependency",
                        operation.index
                    )));
                }
            }
            for precondition in &operation.preconditions {
                match precondition {
                    HistoryPrecondition::HandleLive(handle) => {
                        if !live.contains(handle) {
                            return Err(HistoryError::InvalidCase(format!(
                                "operation {} requires non-live handle {}",
                                operation.index, handle.value
                            )));
                        }
                    }
                    HistoryPrecondition::HandleState { handle, state } => {
                        if state.is_empty() || state.len() > MAX_STRING_BYTES {
                            return Err(HistoryError::InvalidCase(format!(
                                "operation {} has an invalid handle state",
                                operation.index
                            )));
                        }
                        if !live.contains(handle) {
                            return Err(HistoryError::InvalidCase(format!(
                                "operation {} requires non-live handle {}",
                                operation.index, handle.value
                            )));
                        }
                    }
                    HistoryPrecondition::TaskCompleted(task) => {
                        if !completed_tasks.contains(task) {
                            return Err(HistoryError::InvalidCase(format!(
                                "operation {} requires incomplete task {}",
                                operation.index, task.value
                            )));
                        }
                    }
                    HistoryPrecondition::EventAvailable(event) => {
                        if !available_events.contains(event) {
                            return Err(HistoryError::InvalidCase(format!(
                                "operation {} requires unavailable event {}",
                                operation.index, event.value
                            )));
                        }
                    }
                }
            }
            let creates = operation.creates.iter().copied().collect::<BTreeSet<_>>();
            if creates.len() != operation.creates.len() {
                return Err(HistoryError::InvalidCase(format!(
                    "operation {} creates a handle more than once",
                    operation.index
                )));
            }
            let consumes = operation.consumes.iter().copied().collect::<BTreeSet<_>>();
            if consumes.len() != operation.consumes.len() {
                return Err(HistoryError::InvalidCase(format!(
                    "operation {} consumes a handle more than once",
                    operation.index
                )));
            }
            if creates.intersection(&consumes).next().is_some() {
                return Err(HistoryError::InvalidCase(format!(
                    "operation {} creates and consumes one handle",
                    operation.index
                )));
            }
            for handle in &operation.consumes {
                if !live.remove(handle) {
                    return Err(HistoryError::InvalidCase(format!(
                        "operation {} consumes non-live handle {}",
                        operation.index, handle.value
                    )));
                }
            }
            for handle in &operation.creates {
                if !known.insert(*handle) {
                    return Err(HistoryError::InvalidCase(format!(
                        "operation {} recreates handle {}",
                        operation.index, handle.value
                    )));
                }
                live.insert(*handle);
            }
            if let Some(task) = operation.task {
                completed_tasks.insert(task);
            }
            if let Some(event) = operation.event {
                available_events.insert(event);
            }
        }
        if known.len() > MAX_RESOURCES {
            return Err(HistoryError::InvalidCase(
                "resource count exceeds the history bound".to_string(),
            ));
        }
        for choice in &self.schedule {
            let Some(operation) = self.operations.get(choice.operation as usize) else {
                return Err(HistoryError::InvalidCase(format!(
                    "schedule references missing operation {}",
                    choice.operation
                )));
            };
            if let Some(task) = choice.task {
                if operation.task != Some(task) {
                    return Err(HistoryError::InvalidCase(format!(
                        "schedule task does not match operation {}",
                        choice.operation
                    )));
                }
            }
            if let Some(event) = choice.event {
                if operation.event != Some(event) {
                    return Err(HistoryError::InvalidCase(format!(
                        "schedule event does not match operation {}",
                        choice.operation
                    )));
                }
            }
            if choice.choice.is_empty() || choice.choice.len() > MAX_STRING_BYTES {
                return Err(HistoryError::InvalidCase(
                    "schedule choice must be non-empty and bounded".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Stable public input identity.  It includes every operation and schedule
    /// choice, so a seed cannot accidentally replay a changed case.
    pub fn input_id(&self) -> String {
        SHA256::sha256_hex(self.canonical_json().as_bytes())
    }

    /// Canonical case JSON is the callback input for every execution tier.
    /// It contains the typed operations and schedule, never only the seed.
    pub fn json(&self) -> String {
        self.canonical_json()
    }

    fn canonical_json(&self) -> String {
        let operations = self
            .operations
            .iter()
            .map(HistoryOperation::json)
            .collect::<Vec<_>>()
            .join(",");
        let schedule = self
            .schedule
            .iter()
            .map(HistoryScheduleChoice::json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"caseId\":{},\"operations\":[{}],\"schedule\":[{}],\"seed\":{}}}",
            quote(&self.case_id),
            operations,
            schedule,
            self.seed,
        )
    }

    fn without_indices(&self, requested: &BTreeSet<u32>) -> Option<Self> {
        if requested.is_empty() || requested.len() >= self.operations.len() {
            return None;
        }
        let mut removed = requested.clone();
        loop {
            let handles = removed
                .iter()
                .filter_map(|index| self.operations.get(*index as usize))
                .flat_map(|operation| operation.creates.iter().copied())
                .collect::<BTreeSet<_>>();
            let mut changed = false;
            for operation in &self.operations {
                if removed.contains(&operation.index) {
                    continue;
                }
                if operation.depends_on.iter().any(|dependency| removed.contains(dependency))
                    || operation.references_any_handle(&handles)
                {
                    changed = removed.insert(operation.index) || changed;
                }
            }
            if !changed {
                break;
            }
        }
        if removed.len() >= self.operations.len() {
            return None;
        }
        let mut remap = BTreeMap::new();
        let mut operations = Vec::new();
        for operation in &self.operations {
            if removed.contains(&operation.index) {
                continue;
            }
            let next = u32::try_from(operations.len()).ok()?;
            remap.insert(operation.index, next);
            operations.push(operation.clone());
        }
        for operation in &mut operations {
            operation.index = *remap.get(&operation.index)?;
            operation.depends_on = operation
                .depends_on
                .iter()
                .filter_map(|dependency| remap.get(dependency).copied())
                .collect();
        }
        let schedule = self
            .schedule
            .iter()
            .filter_map(|choice| {
                let operation = remap.get(&choice.operation).copied()?;
                let mut choice = choice.clone();
                choice.operation = operation;
                Some(choice)
            })
            .collect();
        Some(Self {
            case_id: self.case_id.clone(),
            seed: self.seed,
            operations,
            schedule,
        })
    }

    fn with_argument(
        &self,
        operation_index: usize,
        argument_index: usize,
        value: HistoryValue,
    ) -> Option<Self> {
        let mut candidate = self.clone();
        let operation = candidate.operations.get_mut(operation_index)?;
        *operation.arguments.get_mut(argument_index)? = value;
        Some(candidate)
    }
}

/// A generated case retains its typed commands beside the canonical metadata.
/// The metadata is what crosses replay boundaries; the command vector keeps
/// generation and domain validation tied to the declared operation type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedHistoryCase<Command> {
    pub case: HistoryCase,
    pub commands: Vec<Command>,
}

impl<Command> TypedHistoryCase<Command> {
    pub fn new(case: HistoryCase, commands: Vec<Command>) -> Option<Self> {
        (case.operations.len() == commands.len()).then_some(Self { case, commands })
    }
}

/// Marker implemented by each operation type admitted to the generic history
/// runner. Each checked command type names its derived strategy as an
/// associated type. Explicit [`HistoryStrategy`] values never consult that
/// derived strategy: the call boundary chooses one or the other.
pub trait HistoryCommand: Clone + fmt::Debug + 'static {
    type Strategy: HistoryStrategyBehavior<Self>;

    fn history_strategy() -> Self::Strategy;

    /// Stable source-level identity retained in replay artifacts.
    fn history_command_type() -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Compiler-supplied lossless command codec used by host bridges.
    fn history_commands_to_data_tree(
        _commands: &[Self],
    ) -> Option<crate::DataTree::DataTree> {
        None
    }

    /// Checked program/artifact provenance for generated strategy execution.
    fn history_provenance() -> Option<HistoryProvenance> {
        None
    }
}

/// An explicit per-test strategy. It is an immutable value: all generation,
/// rebuilding, validation, bounds, and distribution policy is carried by the
/// five required fields, with no hidden merge with a derived strategy.
pub struct HistoryStrategy<Command> {
    pub generate: Box<
        dyn Fn(&mut HistoryRng, Count, Count) -> Option<TypedHistoryCase<Command>>,
    >,
    pub rebuild:
        Box<dyn Fn(&HistoryCase) -> Option<TypedHistoryCase<Command>>>,
    pub valid: Box<dyn Fn(&TypedHistoryCase<Command>) -> bool>,
    pub bounds: HistoryBounds,
    pub distributions: Vec<HistoryDistribution>,
}

impl<Command> fmt::Debug for HistoryStrategy<Command> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output
            .debug_struct("HistoryStrategy")
            .field("bounds", &self.bounds)
            .field("distributions", &self.distributions)
            .finish_non_exhaustive()
    }
}

impl<Command> HistoryStrategy<Command> {
    pub fn new<G, R, V>(
        generate: G,
        rebuild: R,
        valid: V,
        bounds: HistoryBounds,
        distributions: Vec<HistoryDistribution>,
    ) -> Self
    where
        G: Fn(&mut HistoryRng, Count, Count) -> Option<TypedHistoryCase<Command>>
            + 'static,
        R: Fn(&HistoryCase) -> Option<TypedHistoryCase<Command>> + 'static,
        V: Fn(&TypedHistoryCase<Command>) -> bool + 'static,
    {
        Self {
            generate: Box::new(generate),
            rebuild: Box::new(rebuild),
            valid: Box::new(valid),
            bounds,
            distributions,
        }
    }
}

/// Shared behavior implemented by compiler-derived and explicit strategies.
/// The public value above is the only user-facing strategy shape.
pub trait HistoryStrategyBehavior<Command: HistoryCommand> {
    fn command_type(&self) -> &str;
    fn bounds(&self) -> HistoryBounds {
        HistoryBounds::default()
    }
    fn distributions(&self) -> Vec<HistoryDistribution>;
    /// Explain why an automatically derived strategy cannot execute.
    fn unsupported_reason(&self) -> Option<&str> {
        None
    }
    fn generate(
        &self,
        rng: &mut HistoryRng,
        case_index: Count,
        max_steps: Count,
    ) -> Option<TypedHistoryCase<Command>>;
    fn rebuild(&self, case: &HistoryCase) -> Option<TypedHistoryCase<Command>>;
    fn valid(&self, case: &TypedHistoryCase<Command>) -> bool;

    /// Project generated typed commands into the portable callback carrier.
    /// Tiers that execute callbacks natively can keep the commands typed; a
    /// host bridge uses this only at its boundary.
    fn commands_to_data_tree(
        &self,
        commands: &[Command],
    ) -> Option<crate::DataTree::DataTree> {
        Command::history_commands_to_data_tree(commands)
    }
}

impl<Command> HistoryStrategyBehavior<Command> for HistoryStrategy<Command>
where
    Command: HistoryCommand,
{
    fn command_type(&self) -> &str {
        Command::history_command_type()
    }

    fn bounds(&self) -> HistoryBounds {
        self.bounds
    }

    fn distributions(&self) -> Vec<HistoryDistribution> {
        self.distributions.clone()
    }

    fn generate(
        &self,
        rng: &mut HistoryRng,
        case_index: Count,
        max_steps: Count,
    ) -> Option<TypedHistoryCase<Command>> {
        (self.generate)(rng, case_index, max_steps)
    }

    fn rebuild(&self, case: &HistoryCase) -> Option<TypedHistoryCase<Command>> {
        (self.rebuild)(case)
    }

    fn valid(&self, case: &TypedHistoryCase<Command>) -> bool {
        (self.valid)(case)
    }
}

/// One typed operation produced alongside the metadata for a generated step.
/// The operation is the exact input that enters the case; the command is the
/// typed value that the command strategy and callback boundary consume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryGeneratedStep<Command> {
    pub operation: HistoryOperation,
    pub command: Command,
}


/// One checked operation constructor for an automatically generated command
/// strategy. The compiler emits these rows from the command enum declaration;
/// no adapter invents operation names or payloads.
pub struct HistoryGeneratedVariant<Command> {
    pub operation: &'static str,
    pub weight: u32,
    pub generate: fn(&mut HistoryRng, u32) -> Option<HistoryGeneratedStep<Command>>,
    pub rebuild: fn(&HistoryOperation) -> Option<Command>,
    pub valid: fn(&HistoryOperation, &Command) -> bool,
}
/// Strategy shared by all compiler-emitted command types.
pub struct HistoryGeneratedStrategy<Command: 'static> {
    command_type: &'static str,
    variants: &'static [HistoryGeneratedVariant<Command>],
    commands_to_data_tree: fn(&[Command]) -> crate::DataTree::DataTree,
    unsupported_reason: Option<&'static str>,
}

impl<Command: 'static> HistoryGeneratedStrategy<Command> {
    pub const fn new(
        command_type: &'static str,
        variants: &'static [HistoryGeneratedVariant<Command>],
        commands_to_data_tree: fn(&[Command]) -> crate::DataTree::DataTree,
    ) -> Self {
        Self {
            command_type,
            variants,
            commands_to_data_tree,
            unsupported_reason: None,
        }
    }

    /// Construct an automatically derived strategy that is explicitly
    /// unavailable because its full enum declaration is not executable.
    pub const fn new_with_unsupported(
        command_type: &'static str,
        variants: &'static [HistoryGeneratedVariant<Command>],
        commands_to_data_tree: fn(&[Command]) -> crate::DataTree::DataTree,
        unsupported_reason: &'static str,
    ) -> Self {
        Self {
            command_type,
            variants,
            commands_to_data_tree,
            unsupported_reason: Some(unsupported_reason),
        }
    }

    fn variant(&self, operation: &str) -> Option<&HistoryGeneratedVariant<Command>> {
        self.variants
            .iter()
            .find(|variant| variant.operation == operation)
    }
}

impl<Command: 'static> HistoryStrategyBehavior<Command> for HistoryGeneratedStrategy<Command>
where
    Command: HistoryCommand,
{
    fn command_type(&self) -> &str {
        self.command_type
    }

    fn distributions(&self) -> Vec<HistoryDistribution> {
        self.variants
            .iter()
            .filter(|variant| variant.weight > 0)
            .map(|variant| HistoryDistribution {
                operation: variant.operation.to_string(),
                weight: variant.weight as Count,
            })
            .collect()
    }

    fn unsupported_reason(&self) -> Option<&str> {
        self.unsupported_reason
    }

    fn generate(
        &self,
        rng: &mut HistoryRng,
        case_index: usize,
        max_steps: usize,
    ) -> Option<TypedHistoryCase<Command>> {
        if self.unsupported_reason.is_some() {
            return None;
        }
        let distributions = self.distributions();
        if max_steps == 0 || distributions.is_empty() {
            return None;
        }
        let case_seed = rng.next_u64();
        let mut local = HistoryRng::new(case_seed);
        let step_count = 1 + local.below(max_steps.min(8) as u64) as usize;
        let mut case = HistoryCase::new(format!("{}-{case_index}", self.command_type), case_seed);
        let mut commands = Vec::with_capacity(step_count);
        for index in 0..step_count {
            let choice = local
                .choose_distribution(&distributions)
                .unwrap_or(index % distributions.len());
            let variant = self.variant(&distributions[choice].operation)?;
            let generated = (variant.generate)(&mut local, index as u32)?;
            let mut operation = generated.operation;
            if operation.name != variant.operation || operation.index != index as u32 {
                return None;
            }
            operation = operation.scheduled_on(
                TaskId {
                    value: index as u32 % 2,
                },
                EventId {
                    value: index as u32 + 1,
                },
            );
            if index > 0 {
                operation = operation
                    .depends_on(index as u32 - 1)
                    .requires(HistoryPrecondition::TaskCompleted(TaskId {
                        value: (index as u32 - 1) % 2,
                    }))
                    .requires(HistoryPrecondition::EventAvailable(EventId {
                        value: index as u32,
                    }));
            }
            case.push(operation);
            commands.push(generated.command);
        }
        TypedHistoryCase::new(case, commands)
    }

    fn rebuild(&self, case: &HistoryCase) -> Option<TypedHistoryCase<Command>> {
        if self.unsupported_reason.is_some() {
            return None;
        }
        let commands = case
            .operations
            .iter()
            .map(|operation| {
                self.variant(&operation.name)
                    .and_then(|variant| (variant.rebuild)(operation))
            })
            .collect::<Option<Vec<_>>>()?;
        TypedHistoryCase::new(case.clone(), commands)
    }

    fn valid(&self, case: &TypedHistoryCase<Command>) -> bool {
        self.unsupported_reason.is_none()
            && case.case.validate().is_ok()
            && case
                .case
                .operations
                .iter()
                .zip(&case.commands)
                .all(|(operation, command)| {
                    self.variant(&operation.name)
                        .is_some_and(|variant| (variant.valid)(operation, command))
                })
    }

    fn commands_to_data_tree(
        &self,
        commands: &[Command],
    ) -> Option<crate::DataTree::DataTree> {
        Some((self.commands_to_data_tree)(commands))
    }
}

impl HistoryCase {
    /// Count operations after the first one that carry no dependency or
    /// precondition.  Such rows are legal, but reporting them prevents a
    /// campaign from hiding vacuous stateful coverage behind a case count.
    pub fn vacuous_preconditions(&self) -> usize {
        self.operations
            .iter()
            .skip(1)
            .filter(|operation| {
                operation.preconditions.is_empty() && operation.depends_on.is_empty()
            })
            .count()
    }
}

/// Reusable non-booking operation type used by the public adapter and by the
/// second fixture.  Its presence keeps the generic surface from silently
/// specializing on the booking example.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CounterCommand {
    Add(i64),
    Sub(i64),
    Reset,
}

impl HistoryCommand for CounterCommand {
    type Strategy = CounterHistoryStrategy;

    fn history_strategy() -> Self::Strategy {
        CounterHistoryStrategy::new()
    }
}
const COUNTER_DISTRIBUTIONS: [(&str, Count); 3] = [("add", 3), ("sub", 3), ("reset", 1)];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CounterHistoryStrategy;

impl CounterHistoryStrategy {
    pub fn new() -> Self {
        Self
    }

    pub fn distributions() -> Vec<HistoryDistribution> {
        COUNTER_DISTRIBUTIONS
            .into_iter()
            .map(|(operation, weight)| HistoryDistribution {
                operation: operation.to_string(),
                weight,
            })
            .collect()
    }

    fn operation(
        index: u32,
        name: &str,
        argument: Option<i64>,
    ) -> HistoryOperation {
        let task = TaskId { value: index % 2 };
        let event = EventId { value: index + 1 };
        let mut operation = HistoryOperation::new(index, name).scheduled_on(task, event);
        if index > 0 {
            operation = operation
                .depends_on(index - 1)
                .requires(HistoryPrecondition::TaskCompleted(TaskId {
                    value: (index - 1) % 2,
                }))
                .requires(HistoryPrecondition::EventAvailable(EventId {
                    value: index,
                }));
        }
        if let Some(argument) = argument {
            operation = operation.with_argument(HistoryValue::Integer(argument));
        }
        operation
    }

    fn command_for(operation: &HistoryOperation) -> Option<CounterCommand> {
        match operation.name.as_str() {
            "add" => match operation.arguments.as_slice() {
                [HistoryValue::Integer(value)] => Some(CounterCommand::Add(*value)),
                _ => None,
            },
            "sub" => match operation.arguments.as_slice() {
                [HistoryValue::Integer(value)] => Some(CounterCommand::Sub(*value)),
                _ => None,
            },
            "reset" if operation.arguments.is_empty() => Some(CounterCommand::Reset),
            _ => None,
        }
    }
}

impl HistoryStrategyBehavior<CounterCommand> for CounterHistoryStrategy {
    fn command_type(&self) -> &str {
        "CounterCommand"
    }

    fn distributions(&self) -> Vec<HistoryDistribution> {
        Self::distributions()
    }

    fn generate(
        &self,
        rng: &mut HistoryRng,
        case_index: usize,
        max_steps: usize,
    ) -> Option<TypedHistoryCase<CounterCommand>> {
        if max_steps == 0 {
            return None;
        }
        let case_seed = rng.next_u64();
        let mut local = HistoryRng::new(case_seed);
        let step_count = 1 + local.below(max_steps.min(8) as u64) as usize;
        let distributions = Self::distributions();
        let mut case = HistoryCase::new(format!("counter-{case_index}"), case_seed);
        let mut commands = Vec::with_capacity(step_count);
        for index in 0..step_count {
            let choice = local
                .choose_distribution(&distributions)
                .unwrap_or(index % distributions.len());
            let (name, argument, command) = match distributions[choice].operation.as_str() {
                "add" => {
                    let value = local.below(17) as i64 - 8;
                    ("add", Some(value), CounterCommand::Add(value))
                }
                "sub" => {
                    let value = local.below(17) as i64 - 8;
                    ("sub", Some(value), CounterCommand::Sub(value))
                }
                "reset" => ("reset", None, CounterCommand::Reset),
                _ => return None,
            };
            let operation = Self::operation(index as u32, name, argument);
            case.push(operation);
            case.schedule(
                HistoryScheduleChoice::new(index as u32, "fifo")
                    .for_task_event(
                        TaskId {
                            value: index as u32 % 2,
                        },
                        EventId {
                            value: index as u32 + 1,
                        },
                    ),
            );
            commands.push(command);
        }
        TypedHistoryCase::new(case, commands)
    }

    fn rebuild(&self, case: &HistoryCase) -> Option<TypedHistoryCase<CounterCommand>> {
        let commands = case
            .operations
            .iter()
            .map(Self::command_for)
            .collect::<Option<Vec<_>>>()?;
        TypedHistoryCase::new(case.clone(), commands)
    }

    fn valid(&self, case: &TypedHistoryCase<CounterCommand>) -> bool {
        case.case.validate().is_ok()
            && case
                .case
                .operations
                .iter()
                .zip(&case.commands)
                .all(|(operation, command)| match (operation.name.as_str(), command) {
                    ("add", CounterCommand::Add(value))
                    | ("sub", CounterCommand::Sub(value)) => {
                        operation.arguments.as_slice() == [HistoryValue::Integer(*value)]
                    }
                    ("reset", CounterCommand::Reset) => operation.arguments.is_empty(),
                    _ => false,
                })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BookingCommand {
    Reserve { creates: HandleId },
    Cancel { handle: HandleId },
    Timeout { handle: HandleId },
    Retry { handle: HandleId, creates: HandleId },
}

impl HistoryCommand for BookingCommand {
    type Strategy = BookingHistoryStrategy;

    fn history_strategy() -> Self::Strategy {
        BookingHistoryStrategy::new()
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BookingHistoryStrategy;

impl BookingHistoryStrategy {
    pub fn new() -> Self {
        Self
    }

    pub fn distributions() -> Vec<HistoryDistribution> {
        [
            ("reserve", 3),
            ("cancel", 2),
            ("timeout", 2),
            ("retry", 1),
        ]
        .into_iter()
        .map(|(operation, weight)| HistoryDistribution {
            operation: operation.to_string(),
            weight,
        })
        .collect()
    }

    fn handle_argument(operation: &HistoryOperation) -> Option<HandleId> {
        let mut handles = operation.arguments.iter().filter_map(HistoryValue::handles);
        let handle = handles.next()?;
        handles.next().is_none().then_some(handle)
    }

    fn command_for(operation: &HistoryOperation) -> Option<BookingCommand> {
        match operation.name.as_str() {
            "reserve" if operation.arguments.is_empty() && operation.creates.len() == 1 => {
                Some(BookingCommand::Reserve {
                    creates: operation.creates[0],
                })
            }
            "cancel" if operation.creates.is_empty() && operation.consumes.is_empty() => {
                Some(BookingCommand::Cancel {
                    handle: Self::handle_argument(operation)?,
                })
            }
            "timeout" if operation.creates.is_empty() && operation.consumes.is_empty() => {
                Some(BookingCommand::Timeout {
                    handle: Self::handle_argument(operation)?,
                })
            }
            "retry" if operation.creates.len() == 1 && operation.consumes.len() == 1 => {
                Some(BookingCommand::Retry {
                    handle: Self::handle_argument(operation)?,
                    creates: operation.creates[0],
                })
            }
            _ => None,
        }
    }
}

impl HistoryStrategyBehavior<BookingCommand> for BookingHistoryStrategy {
    fn command_type(&self) -> &str {
        "BookingCommand"
    }

    fn distributions(&self) -> Vec<HistoryDistribution> {
        Self::distributions()
    }

    fn generate(
        &self,
        rng: &mut HistoryRng,
        case_index: usize,
        max_steps: usize,
    ) -> Option<TypedHistoryCase<BookingCommand>> {
        generate_booking_history_case(rng, case_index, max_steps)
            .and_then(|case| self.rebuild(&case))
    }

    fn rebuild(&self, case: &HistoryCase) -> Option<TypedHistoryCase<BookingCommand>> {
        let commands = case
            .operations
            .iter()
            .map(Self::command_for)
            .collect::<Option<Vec<_>>>()?;
        TypedHistoryCase::new(case.clone(), commands)
    }

    fn valid(&self, case: &TypedHistoryCase<BookingCommand>) -> bool {
        booking_case_is_valid(&case.case)
            && case
                .case
                .operations
                .iter()
                .zip(&case.commands)
                .all(|(operation, command)| {
                    Self::command_for(operation).as_ref() == Some(command)
                })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistorySchemaCommand {
    pub operation: String,
    pub arguments: Vec<HistoryValue>,
    pub creates: Vec<HandleId>,
    pub consumes: Vec<HandleId>,
}
/// A scalar field admitted by the shared schema strategy.  Compiler tiers use
/// this small checked vocabulary for ordinary enum commands; resource-bearing
/// operations still require a domain strategy that can name their handles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistorySchemaField {
    Integer { lo: i64, hi: i64 },
    Boolean,
    Text,
    Char,
}

impl HistorySchemaField {
    fn generate(&self, rng: &mut HistoryRng, case_index: usize, step_index: usize) -> Option<HistoryValue> {
        match self {
            Self::Integer { lo, hi } if lo <= hi => {
                let width = (*hi as i128 - *lo as i128 + 1) as u128;
                let value = if width <= u64::MAX as u128 {
                    *lo as i128 + rng.below(width as u64) as i128
                } else {
                    *lo as i128
                };
                Some(HistoryValue::Integer(value as i64))
            }
            Self::Integer { .. } => None,
            Self::Boolean => Some(HistoryValue::Boolean(rng.coin())),
            Self::Text => Some(HistoryValue::Text(format!(
                "history-{case_index}-{step_index}"
            ))),
            Self::Char => {
                let value = (b'a' + rng.below(26) as u8) as char;
                Some(HistoryValue::Text(value.to_string()))
            }
        }
    }

    fn valid(&self, value: &HistoryValue) -> bool {
        match (self, value) {
            (Self::Integer { lo, hi }, HistoryValue::Integer(value)) => lo <= value && value <= hi,
            (Self::Boolean, HistoryValue::Boolean(_)) => true,
            (Self::Text, HistoryValue::Text(_)) => true,
            (Self::Char, HistoryValue::Text(value)) => {
                value.chars().count() == 1
            }
            _ => false,
        }
    }
}
/// Canonical reason used when automatic derivation cannot cover a declared
/// enum variant without inventing a payload codec.
pub fn history_unsupported_variant_reason(command_type: &str, variant: &str) -> String {
    format!(
        "core.testing.histories command type `{command_type}` has unsupported variant `{variant}`: no type-valid scalar generator/codec"
    )
}
/// Canonical reason used when automatic derivation cannot find a checked enum
/// schema for the requested command type.
pub fn history_unsupported_type_reason(command_type: &str) -> String {
    format!(
        "core.testing.histories command type `{command_type}` has no checked enum schema"
    )
}

/// One ordinary enum operation accepted by [`HistorySchemaStrategy`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistorySchemaVariant {
    pub operation: String,
    pub variant: String,
    pub fields: Vec<HistorySchemaField>,
}

impl HistorySchemaVariant {
    pub fn new(
        operation: impl Into<String>,
        variant: impl Into<String>,
        fields: Vec<HistorySchemaField>,
    ) -> Option<Self> {
        let operation = operation.into();
        let variant = variant.into();
        (!operation.is_empty() && !variant.is_empty() && operation.len() <= MAX_STRING_BYTES)
            .then_some(Self {
                operation,
                variant,
                fields,
            })
    }
}


#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistorySchemaStrategy {
    command_type: String,
    distributions: Vec<HistoryDistribution>,
    variants: Option<Vec<HistorySchemaVariant>>,
}

impl HistorySchemaStrategy {
    pub fn new(command_type: impl Into<String>) -> Self {
        Self {
            command_type: command_type.into(),
            distributions: vec![HistoryDistribution {
                operation: "command".to_string(),
                weight: 1,
            }],
            variants: None,
        }
    }

    /// Build the shared ordinary-enum strategy from checked variant rows.
    /// Every admitted variant is visible in the distribution table; unsupported
    /// payloads are omitted by the caller before this constructor.
    pub fn with_variants(
        command_type: impl Into<String>,
        variants: Vec<HistorySchemaVariant>,
    ) -> Option<Self> {
        if variants.is_empty()
            || variants
                .iter()
                .enumerate()
                .any(|(index, variant)| {
                    variant.operation.is_empty()
                        || variant.operation.len() > MAX_STRING_BYTES
                        || variants[..index]
                            .iter()
                            .any(|previous| previous.operation == variant.operation)
                })
        {
            return None;
        }
        let distributions = variants
            .iter()
            .map(|variant| HistoryDistribution {
                operation: variant.operation.clone(),
                weight: 1,
            })
            .collect();
        Some(Self {
            command_type: command_type.into(),
            distributions,
            variants: Some(variants),
        })
    }

    pub fn command_type(&self) -> &str {
        &self.command_type
    }

    pub fn distributions(&self) -> Vec<HistoryDistribution> {
        self.distributions.clone()
    }

    pub fn with_distributions(
        command_type: impl Into<String>,
        distributions: Vec<HistoryDistribution>,
    ) -> Option<Self> {
        (!distributions.is_empty()).then_some(Self {
            command_type: command_type.into(),
            distributions,
            variants: None,
        })
    }

    fn command_for(operation: &HistoryOperation) -> HistorySchemaCommand {
        HistorySchemaCommand {
            operation: operation.name.clone(),
            arguments: operation.arguments.clone(),
            creates: operation.creates.clone(),
            consumes: operation.consumes.clone(),
        }
    }

    fn variant_for(&self, operation: &HistoryOperation) -> Option<&HistorySchemaVariant> {
        self.variants
            .as_ref()?
            .iter()
            .find(|variant| variant.operation == operation.name)
    }
}


impl HistoryCommand for HistorySchemaCommand {
    type Strategy = HistorySchemaStrategy;

    fn history_strategy() -> Self::Strategy {
        HistorySchemaStrategy::new(std::any::type_name::<Self>())
    }
}

impl HistoryStrategyBehavior<HistorySchemaCommand> for HistorySchemaStrategy {
    fn command_type(&self) -> &str {
        &self.command_type
    }

    fn distributions(&self) -> Vec<HistoryDistribution> {
        self.distributions.clone()
    }

    fn generate(
        &self,
        rng: &mut HistoryRng,
        case_index: usize,
        max_steps: usize,
    ) -> Option<TypedHistoryCase<HistorySchemaCommand>> {
        if max_steps == 0 || self.distributions.is_empty() {
            return None;
        }
        let case_seed = rng.next_u64();
        let mut local = HistoryRng::new(case_seed);
        let step_count = 1 + local.below(max_steps.min(8) as u64) as usize;
        let mut case = HistoryCase::new(format!("{}-{case_index}", self.command_type), case_seed);
        for index in 0..step_count {
            let choice = local.choose_distribution(&self.distributions)?;
            let mut operation =
                HistoryOperation::new(index as u32, self.distributions[choice].operation.clone())
                    .scheduled_on(
                        TaskId {
                            value: index as u32 % 2,
                        },
                        EventId {
                            value: index as u32 + 1,
                        },
                    );
            if self.variants.is_some() {
                let variant = self.variant_for(&operation)?;
                for (field_index, field) in variant.fields.iter().enumerate() {
                    operation = operation.with_argument(field.generate(
                        &mut local,
                        case_index,
                        index * variant.fields.len() + field_index,
                    )?);
                }
            }
            if index > 0 {
                operation = operation
                    .depends_on(index as u32 - 1)
                    .requires(HistoryPrecondition::TaskCompleted(TaskId {
                        value: (index as u32 - 1) % 2,
                    }))
                    .requires(HistoryPrecondition::EventAvailable(EventId {
                        value: index as u32,
                    }));
            }
            case.push(operation);
        }
        let commands = case
            .operations
            .iter()
            .map(Self::command_for)
            .collect();
        TypedHistoryCase::new(case, commands)
    }

    fn rebuild(
        &self,
        case: &HistoryCase,
    ) -> Option<TypedHistoryCase<HistorySchemaCommand>> {
        let commands = case
            .operations
            .iter()
            .map(Self::command_for)
            .collect();
        TypedHistoryCase::new(case.clone(), commands)
    }

    fn valid(&self, case: &TypedHistoryCase<HistorySchemaCommand>) -> bool {
        let schema_valid = self.variants.as_ref().is_none_or(|variants| {
            case.case.operations.iter().all(|operation| {
                let Some(variant) = variants
                    .iter()
                    .find(|variant| variant.operation == operation.name)
                else {
                    return false;
                };
                operation.creates.is_empty()
                    && operation.consumes.is_empty()
                    && operation.arguments.len() == variant.fields.len()
                    && operation
                        .arguments
                        .iter()
                        .zip(&variant.fields)
                        .all(|(value, field)| field.valid(value))
            })
        });
        case.case.validate().is_ok()
            && schema_valid
            && case
                .case
                .operations
                .iter()
                .zip(&case.commands)
                .all(|(operation, command)| {
                    command.operation == operation.name
                        && command.arguments == operation.arguments
                        && command.creates == operation.creates
                        && command.consumes == operation.consumes
                })
    }
}

fn history_value_data_tree(value: &HistoryValue) -> crate::DataTree::DataTree {
    match value {
        HistoryValue::Integer(value) => crate::DataTree::DataTree::Int(*value),
        HistoryValue::Boolean(value) => crate::DataTree::DataTree::Bool(*value),
        HistoryValue::Text(value) => crate::DataTree::DataTree::Text(value.clone()),
        HistoryValue::Handle(handle) => crate::DataTree::DataTree::Int(handle.value as i64),
        HistoryValue::Redacted(label) => crate::DataTree::DataTree::Object(vec![
            (
                "redacted".to_string(),
                crate::DataTree::DataTree::Text(label.clone()),
            ),
        ]),
    }
}

fn history_schema_command_data_tree(
    command: &HistorySchemaCommand,
) -> crate::DataTree::DataTree {
    crate::DataTree::DataTree::Object(vec![
        (
            "operation".to_string(),
            crate::DataTree::DataTree::Text(command.operation.clone()),
        ),
        (
            "arguments".to_string(),
            crate::DataTree::DataTree::Array(
                command
                    .arguments
                    .iter()
                    .map(history_value_data_tree)
                    .collect(),
            ),
        ),
        (
            "creates".to_string(),
            crate::DataTree::DataTree::Array(
                command
                    .creates
                    .iter()
                    .map(|handle| crate::DataTree::DataTree::Int(handle.value as i64))
                    .collect(),
            ),
        ),
        (
            "consumes".to_string(),
            crate::DataTree::DataTree::Array(
                command
                    .consumes
                    .iter()
                    .map(|handle| crate::DataTree::DataTree::Int(handle.value as i64))
                    .collect(),
            ),
        ),
    ])
}

impl HistoryCommand for crate::DataTree::DataTree {
    type Strategy = HistorySchemaStrategy;

    fn history_strategy() -> Self::Strategy {
        HistorySchemaStrategy::new("DataTree")
    }
}

impl HistoryStrategyBehavior<crate::DataTree::DataTree> for HistorySchemaStrategy {
    fn command_type(&self) -> &str {
        &self.command_type
    }

    fn distributions(&self) -> Vec<HistoryDistribution> {
        self.distributions.clone()
    }

    fn generate(
        &self,
        rng: &mut HistoryRng,
        case_index: usize,
        max_steps: usize,
    ) -> Option<TypedHistoryCase<crate::DataTree::DataTree>> {
        let generated =
            <Self as HistoryStrategyBehavior<HistorySchemaCommand>>::generate(
                self,
                rng,
                case_index,
                max_steps,
            )?;
        let commands = generated
            .case
            .operations
            .iter()
            .map(|operation| {
                history_schema_command_data_tree(&Self::command_for(operation))
            })
            .collect();
        TypedHistoryCase::new(generated.case, commands)
    }

    fn rebuild(
        &self,
        case: &HistoryCase,
    ) -> Option<TypedHistoryCase<crate::DataTree::DataTree>> {
        let commands = case
            .operations
            .iter()
            .map(|operation| {
                history_schema_command_data_tree(&Self::command_for(operation))
            })
            .collect();
        TypedHistoryCase::new(case.clone(), commands)
    }

    fn valid(&self, case: &TypedHistoryCase<crate::DataTree::DataTree>) -> bool {
        case.case.validate().is_ok() && case.commands.len() == case.case.operations.len()
    }

    fn commands_to_data_tree(
        &self,
        commands: &[crate::DataTree::DataTree],
    ) -> Option<crate::DataTree::DataTree> {
        Some(crate::DataTree::DataTree::Array(commands.to_vec()))
    }
}



/// Bounds are recorded as evidence, rather than hidden runner constants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistoryBounds {
    pub max_steps: usize,
    pub max_resources: usize,
    pub max_shrink_attempts: usize,
    pub max_discarded_cases: usize,
}

impl Default for HistoryBounds {
    fn default() -> Self {
        Self {
            max_steps: 64,
            max_resources: 64,
            max_shrink_attempts: 10_000,
            max_discarded_cases: 100,
        }
    }
}

impl HistoryBounds {
    pub fn validate(self) -> Result<(), HistoryError> {
        if self.max_steps == 0 || self.max_steps > MAX_STEPS {
            return Err(HistoryError::InvalidConfig(
                "max_steps must be between 1 and the history limit".to_string(),
            ));
        }
        if self.max_resources == 0 || self.max_resources > MAX_RESOURCES {
            return Err(HistoryError::InvalidConfig(
                "max_resources must be between 1 and the history limit".to_string(),
            ));
        }
        if self.max_shrink_attempts == 0 || self.max_shrink_attempts > MAX_SHRINK_ATTEMPTS {
            return Err(HistoryError::InvalidConfig(
                "max_shrink_attempts must be between 1 and the history limit".to_string(),
            ));
        }
        if self.max_discarded_cases > MAX_CASES {
            return Err(HistoryError::InvalidConfig(
                "max_discarded_cases exceeds the history limit".to_string(),
            ));
        }
        Ok(())
    }
}

/// A visible weighted operation distribution.  The generator still owns the
/// domain semantics; these rows only document its bounded choices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryDistribution {
    pub operation: String,
    pub weight: Count,
}

impl HistoryDistribution {
    pub fn new(operation: impl Into<String>, weight: Count) -> Result<Self, HistoryError> {
        let operation = operation.into();
        if operation.is_empty() || operation.len() > MAX_STRING_BYTES || weight == 0 {
            return Err(HistoryError::InvalidConfig(
                "distribution operation and positive weight are required".to_string(),
            ));
        }
        Ok(Self { operation, weight })
    }

    fn json(&self) -> String {
        format!(
            "{{\"operation\":{},\"weight\":{}}}",
            quote(&self.operation),
            self.weight
        )
    }
}

/// Dependency labels let the runner reject direct and transitive production
/// calls before a mismatch becomes evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OracleDependencyKind {
    Independent,
    ProductionBehavior,
    Opaque,
    Unknown,
}

impl OracleDependencyKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Independent => "independent",
            Self::ProductionBehavior => "production_behavior",
            Self::Opaque => "opaque",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleDependency {
    pub from: String,
    pub to: String,
    pub kind: OracleDependencyKind,
}

impl OracleDependency {
    pub fn new(
        from: impl Into<String>,
        to: impl Into<String>,
        kind: OracleDependencyKind,
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            kind,
        }
    }
}

/// An explicit oracle provenance declaration.  No oracle is independent by
/// assertion alone: every reachable dependency must be classified.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleDeclaration {
    pub root: String,
    pub dependencies: Vec<OracleDependency>,
}

impl OracleDeclaration {
    pub fn independent(root: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            dependencies: Vec::new(),
        }
    }

    pub fn with_dependency(mut self, dependency: OracleDependency) -> Self {
        self.dependencies.push(dependency);
        self
    }

    pub fn validate_independence(&self) -> Result<(), HistoryError> {
        if self.root.is_empty() || self.root.len() > MAX_STRING_BYTES {
            return Err(HistoryError::InvalidOracle(
                "oracle root must be non-empty and bounded".to_string(),
            ));
        }
        let mut reachable = vec![self.root.clone()];
        let mut visited = BTreeSet::new();
        while let Some(node) = reachable.pop() {
            if !visited.insert(node.clone()) {
                continue;
            }
            for dependency in self.dependencies.iter().filter(|edge| edge.from == node) {
                if dependency.from == dependency.to {
                    return Err(HistoryError::InvalidOracle(format!(
                        "oracle dependency `{}` is self-confirming",
                        dependency.from
                    )));
                }
                match dependency.kind {
                    OracleDependencyKind::Independent => reachable.push(dependency.to.clone()),
                    OracleDependencyKind::ProductionBehavior => {
                        return Err(HistoryError::InvalidOracle(format!(
                            "oracle `{}` reaches production behavior `{}`",
                            dependency.from, dependency.to
                        )))
                    }
                    OracleDependencyKind::Opaque | OracleDependencyKind::Unknown => {
                        return Err(HistoryError::InvalidOracle(format!(
                            "oracle dependency `{}` -> `{}` is not certifiably independent ({})",
                            dependency.from,
                            dependency.to,
                            dependency.kind.as_str()
                        )))
                    }
                }
            }
        }
        Ok(())
    }
}

/// A complete, deterministic campaign declaration.
#[derive(Clone, Debug)]
pub struct HistoryConfig {
    pub seed: u64,
    /// Number of generated cases. This remains a call-level bound rather than
    /// part of the per-strategy resource/step bounds.
    pub cases: usize,
    pub bounds: HistoryBounds,
    pub distributions: Vec<HistoryDistribution>,
    pub source: String,
    pub tool: String,
    pub target: String,
    pub relation: ObservationRelation,
    pub oracle: OracleDeclaration,
    /// The source-level operation type carried by this campaign. Keeping it
    /// in the envelope makes a changed generic command fail closed on replay.
    pub command_type: String,
}

impl HistoryConfig {
    pub fn new(
        seed: u64,
        cases: usize,
        bounds: HistoryBounds,
        source: impl Into<String>,
        tool: impl Into<String>,
        target: impl Into<String>,
        relation: ObservationRelation,
        oracle: OracleDeclaration,
    ) -> Self {
        Self {
            seed,
            cases,
            bounds,
            distributions: Vec::new(),
            source: source.into(),
            tool: tool.into(),
            target: target.into(),
            relation,
            oracle,
            command_type: "unspecified".to_string(),
        }
    }
    pub fn with_provenance(mut self, provenance: HistoryProvenance) -> Self {
        self.source = provenance.source;
        self.tool = provenance.tool;
        self.target = provenance.target;
        self
    }

    pub fn with_command_type(mut self, command_type: impl Into<String>) -> Self {
        self.command_type = command_type.into();
        self
    }

    pub fn with_distribution(mut self, distribution: HistoryDistribution) -> Self {
        self.distributions.push(distribution);
        self
    }

    pub fn validate(&self) -> Result<(), HistoryError> {
        if self.cases == 0 || self.cases > MAX_CASES {
            return Err(HistoryError::InvalidConfig(
                "cases must be between 1 and the history limit".to_string(),
            ));
        }
        self.bounds.validate()?;
        for field in [
            (&self.source, "source"),
            (&self.tool, "tool"),
            (&self.target, "target"),
            (&self.command_type, "command type"),
        ] {
            if field.0.is_empty() || field.0.len() > MAX_STRING_BYTES {
                return Err(HistoryError::InvalidConfig(format!(
                    "{} must be non-empty and bounded",
                    field.1
                )));
            }
        }
        if self.distributions.len() > MAX_DISTRIBUTIONS {
            return Err(HistoryError::InvalidConfig(
                "distribution count exceeds the history limit".to_string(),
            ));
        }
        for (index, distribution) in self.distributions.iter().enumerate() {
            if distribution.operation.is_empty()
                || distribution.operation.len() > MAX_STRING_BYTES
                || distribution.weight == 0
            {
                return Err(HistoryError::InvalidConfig(format!(
                    "distribution {} has an invalid operation or weight",
                    index
                )));
            }
        }
        self.oracle.validate_independence()?;
        Ok(())
    }
}


/// Deterministic splitmix64 choices.  A case records its derived seed, so a
/// replay does not depend on how many earlier cases were discarded.
#[derive(Clone, Debug)]
pub struct HistoryRng {
    state: u64,
}

impl HistoryRng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn state(&self) -> u64 {
        self.state
    }

    pub fn replace_state(&mut self, state: u64) {
        self.state = state;
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }

    pub fn coin(&mut self) -> bool {
        self.below(2) == 0
    }
    /// Select one weighted operation without introducing platform-dependent
    /// randomness.  Invalid/empty distributions are rejected by returning
    /// `None`; callers can then discard the case rather than silently using a
    /// different operation.
    pub fn choose_distribution(&mut self, distributions: &[HistoryDistribution]) -> Option<usize> {
        let total = distributions.iter().try_fold(0u64, |total, distribution| {
            total.checked_add(u64::try_from(distribution.weight).ok()?)
        })?;
        if total == 0 {
            return None;
        }
        let mut choice = self.below(total);
        for (index, distribution) in distributions.iter().enumerate() {
            let weight = u64::try_from(distribution.weight).ok()?;
            if choice < weight {
                return Some(index);
            }
            choice -= weight;
        }
        None
    }
}

/// Result of one bounded campaign.  A missing failure is not a proof of
/// equivalence; explored, discarded, and vacuous-precondition counts remain
/// visible to the caller.
#[derive(Clone, Debug)]
pub struct HistoryRun {
    pub seed: u64,
    pub cases: usize,
    pub bounds: HistoryBounds,
    pub distributions: Vec<HistoryDistribution>,
    pub explored_cases: usize,
    pub discarded_cases: usize,
    pub vacuous_preconditions: usize,
    pub failure: Option<HistoryArtifact>,
}

impl HistoryRun {
    pub fn has_failure(&self) -> bool {
        self.failure.is_some()
    }

    pub fn discarded_rate(&self) -> f64 {
        let total = self.explored_cases.saturating_add(self.discarded_cases);
        if total == 0 {
            return 0.0;
        }
        self.discarded_cases as f64 / total as f64
    }
}

/// Persisted counterexample.  The comparison record is the canonical verdict;
/// this wrapper adds the command type, typed operation and schedule needed to
/// replay it.
#[derive(Clone, Debug)]
pub struct HistoryArtifact {
    pub artifact_id: String,
    pub identity: ComparisonIdentity,
    pub case: HistoryCase,
    pub cases: usize,
    pub bounds: HistoryBounds,
    pub distributions: Vec<HistoryDistribution>,
    pub discarded_cases: usize,
    pub vacuous_preconditions: usize,
    pub command_type: String,
    pub oracle: OracleDeclaration,
    pub comparison: ComparisonRecord,
}

impl HistoryArtifact {
    fn new(
        config: &HistoryConfig,
        case: HistoryCase,
        comparison: ComparisonRecord,
        discarded_cases: usize,
    ) -> Result<Self, HistoryError> {
        let Some(sample) = comparison.first_mismatch() else {
            return Err(HistoryError::NoFailure);
        };
        let mut identity = sample.identity.clone();
        identity.seed = Some(case.seed);
        let vacuous_preconditions = case.vacuous_preconditions();
        let mut artifact = Self {
            artifact_id: String::new(),
            identity,
            case,
            cases: config.cases,
            bounds: config.bounds,
            distributions: config.distributions.clone(),
            discarded_cases,
            vacuous_preconditions,
            command_type: config.command_type.clone(),
            oracle: config.oracle.clone(),
            comparison,
        };
        artifact.artifact_id = SHA256::sha256_hex(artifact.payload_json().as_bytes());
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), HistoryError> {
        self.case.validate()?;
        if !case_within_bounds(&self.case, self.bounds) {
            return Err(HistoryError::InvalidCase(
                "artifact case exceeds its recorded history bounds".to_string(),
            ));
        }
        self.oracle.validate_independence()?;
        if !self.identity.is_complete() {
            return Err(HistoryError::InvalidCase(
                "artifact identity is incomplete".to_string(),
            ));
        }
        if self.identity.seed != Some(self.case.seed) {
            return Err(HistoryError::InvalidCase(
                "artifact identity seed does not match the replay case".to_string(),
            ));
        }
        if self.command_type.is_empty() || self.command_type.len() > MAX_STRING_BYTES {
            return Err(HistoryError::InvalidCase(
                "artifact command type must be non-empty and bounded".to_string(),
            ));
        }
        if self.vacuous_preconditions != self.case.vacuous_preconditions() {
            return Err(HistoryError::InvalidCase(
                "artifact vacuous-precondition count does not match its case".to_string(),
            ));
        }
        if self.comparison.status != ComparisonStatus::Mismatch {
            return Err(HistoryError::InvalidCase(
                "history artifact must retain a comparison mismatch".to_string(),
            ));
        }
        if self.comparison.universal_proof {
            return Err(HistoryError::InvalidCase(
                "finite history cannot be marked as universal proof".to_string(),
            ));
        }
        if self.identity.case_id != self.case.case_id {
            return Err(HistoryError::InvalidCase(
                "artifact identity case does not match the replay case".to_string(),
            ));
        }
        if self.identity.input_id != self.case.input_id() {
            return Err(HistoryError::InvalidCase(
                "artifact identity input does not match the replay case".to_string(),
            ));
        }
        let Some(sample) = self.comparison.first_mismatch() else {
            return Err(HistoryError::InvalidCase(
                "history artifact has no mismatch sample".to_string(),
            ));
        };
        if sample.identity != self.identity {
            return Err(HistoryError::InvalidCase(
                "artifact identity does not match the mismatch sample".to_string(),
            ));
        }
        let expected_id = SHA256::sha256_hex(self.payload_json().as_bytes());
        if self.artifact_id != expected_id {
            return Err(HistoryError::InvalidCase(
                "artifact identity hash does not match its payload".to_string(),
            ));
        }
        Ok(())
    }

    /// Canonical JSON envelope.  Secret values can only enter through
    /// `HistoryValue::secret`, which stores a redaction label instead.
    pub fn json(&self) -> String {
        // `payload_json` is an object with stable key ordering.  Insert the
        // identity as the first field without reparsing a second schema.
        let payload = self.payload_json();
        let body = payload.strip_prefix('{').unwrap_or(payload.as_str());
        format!(
            "{{\"artifactId\":{},{}",
            quote(&self.artifact_id),
            body
        )
    }

    pub fn write_json(&self, path: &Path) -> Result<(), HistoryError> {
        self.validate()?;
        let contents = format!("{}\n", self.json());
        if let Ok(metadata) = fs::symlink_metadata(path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(HistoryError::Io(format!(
                    "artifact path is not a regular file: {}",
                    path.display()
                )));
            }
            let existing = fs::read(path).map_err(|error| HistoryError::Io(error.to_string()))?;
            if existing == contents.as_bytes() {
                return Ok(());
            }
            return Err(HistoryError::Io(format!(
                "refusing to overwrite differing artifact: {}",
                path.display()
            )));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| HistoryError::Io(error.to_string()))?;
        }
        let temporary = path.with_extension(format!(
            "json.tmp.{}.{}",
            std::process::id(),
            &SHA256::sha256_hex(contents.as_bytes())[..8]
        ));
        let write_result = (|| -> Result<(), HistoryError> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| HistoryError::Io(error.to_string()))?;
            file.write_all(contents.as_bytes())
                .map_err(|error| HistoryError::Io(error.to_string()))?;
            file.sync_all()
                .map_err(|error| HistoryError::Io(error.to_string()))?;
            Ok(())
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        fs::rename(&temporary, path).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            HistoryError::Io(error.to_string())
        })?;
        if let Some(parent) = path.parent() {
            fs::File::open(parent)
                .map_err(|error| HistoryError::Io(error.to_string()))?
                .sync_all()
                .map_err(|error| HistoryError::Io(error.to_string()))?;
        }
        Ok(())
    }

    fn payload_json(&self) -> String {
        let operations = self
            .case
            .operations
            .iter()
            .map(HistoryOperation::json)
            .collect::<Vec<_>>()
            .join(",");
        let schedule = self
            .case
            .schedule
            .iter()
            .map(HistoryScheduleChoice::json)
            .collect::<Vec<_>>()
            .join(",");
        let distributions = self
            .distributions
            .iter()
            .map(HistoryDistribution::json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"bounds\":{{\"maxDiscardedCases\":{},\"maxResources\":{},\"maxSteps\":{},\"maxShrinkAttempts\":{}}},\"case\":{},\"cases\":{},\"commandType\":{},\"comparison\":{},\"discardedCases\":{},\"distributions\":[{}],\"identity\":{},\"operations\":[{}],\"oracle\":{},\"schedule\":[{}],\"schema\":{},\"seed\":{},\"vacuousPreconditions\":{},\"version\":{}}}",
            self.bounds.max_discarded_cases,
            self.bounds.max_resources,
            self.bounds.max_steps,
            self.bounds.max_shrink_attempts,
            quote(&self.case.case_id),
            self.cases,
            quote(&self.command_type),
            comparison_json(&self.comparison),
            self.discarded_cases,
            distributions,
            identity_json(&self.identity),
            operations,
            oracle_json(&self.oracle),
            schedule,
            quote(HISTORY_SCHEMA),
            self.case.seed,
            self.vacuous_preconditions,
            HISTORY_VERSION,
        )
    }
}

fn identity_json(identity: &ComparisonIdentity) -> String {
    format!(
        "{{\"caseId\":{},\"inputId\":{},\"seed\":{},\"source\":{},\"target\":{},\"tool\":{}}}",
        quote(&identity.case_id),
        quote(&identity.input_id),
        identity.seed.map_or_else(|| "null".to_string(), |seed| seed.to_string()),
        quote(&identity.source),
        quote(&identity.target),
        quote(&identity.tool),
    )
}

fn oracle_json(oracle: &OracleDeclaration) -> String {
    let dependencies = oracle
        .dependencies
        .iter()
        .map(|dependency| {
            format!(
                "{{\"from\":{},\"kind\":{},\"to\":{}}}",
                quote(&dependency.from),
                quote(dependency.kind.as_str()),
                quote(&dependency.to),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"dependencies\":[{}],\"root\":{}}}",
        dependencies,
        quote(&oracle.root)
    )
}

fn comparison_json(comparison: &ComparisonRecord) -> String {
    let samples = comparison
        .samples
        .iter()
        .map(sample_json)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"contamination\":{},\"discardedCases\":{},\"firstDifference\":{},\"reason\":{},\"relation\":{},\"samples\":[{}],\"status\":{},\"universalProof\":{}}}",
        comparison
            .contamination
            .as_ref()
            .map_or_else(|| "null".to_string(), |value| quote(value)),
        comparison.discarded_cases,
        comparison
            .first_difference
            .map_or_else(|| "null".to_string(), |value| value.to_string()),
        comparison
            .reason
            .as_ref()
            .map_or_else(|| "null".to_string(), |value| quote(value)),
        quote(comparison.relation.as_str()),
        samples,
        quote(comparison.status.as_str()),
        comparison.universal_proof,
    )
}

fn sample_json(sample: &ComparisonSample) -> String {
    let candidate_replay = sample
        .candidate_replay
        .as_ref()
        .map_or_else(|| "null".to_string(), observation_json);
    let reference_replay = sample
        .reference_replay
        .as_ref()
        .map_or_else(|| "null".to_string(), observation_json);
    format!(
        "{{\"candidate\":{},\"candidateReplay\":{},\"identity\":{},\"reference\":{},\"referenceReplay\":{}}}",
        observation_json(&sample.candidate),
        candidate_replay,
        identity_json(&sample.identity),
        observation_json(&sample.reference),
        reference_replay,
    )
}

fn observation_json(observation: &ComparisonObservation) -> String {
    let effects = observation
        .ordered_effects
        .iter()
        .map(|effect| quote(effect))
        .collect::<Vec<_>>()
        .join(",");
    let cleanup = observation
        .cleanup
        .iter()
        .map(|effect| quote(effect))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"cleanup\":[{}],\"mutation\":{},\"orderedEffects\":[{}],\"raw\":{},\"typedFailure\":{}}}",
        cleanup,
        observation
            .mutation
            .as_ref()
            .map_or_else(|| "null".to_string(), |value| quote(value)),
        effects,
        quote(&observation.raw),
        observation
            .typed_failure
            .as_ref()
            .map_or_else(|| "null".to_string(), |value| quote(value)),
    )
}

/// Reduce a mismatch while preserving generic typed dependencies and the
/// canonical comparison's failure class.
pub fn shrink_history<F>(
    original: &HistoryCase,
    bounds: HistoryBounds,
    observe: F,
) -> Result<(HistoryCase, ComparisonRecord), HistoryError>
where
    F: FnMut(&HistoryCase) -> ComparisonRecord,
{
    shrink_history_with_validity(original, bounds, |_| true, observe)
}

/// Reduce a mismatch while also applying domain-specific state validation.
/// The validator is where a booking/session model enforces state transitions;
/// generic handle validation remains in [`HistoryCase::validate`].
fn case_within_bounds(case: &HistoryCase, bounds: HistoryBounds) -> bool {
    if case.operations.len() > bounds.max_steps {
        return false;
    }
    let resources = case
        .operations
        .iter()
        .flat_map(|operation| operation.creates.iter().copied())
        .collect::<BTreeSet<_>>();
    resources.len() <= bounds.max_resources
}

pub fn shrink_history_with_validity<V, F>(
    original: &HistoryCase,
    bounds: HistoryBounds,
    mut valid: V,
    mut observe: F,

) -> Result<(HistoryCase, ComparisonRecord), HistoryError>
where
    V: FnMut(&HistoryCase) -> bool,
    F: FnMut(&HistoryCase) -> ComparisonRecord,
{
    bounds.validate()?;
    original.validate()?;
    if !case_within_bounds(original, bounds) {
        return Err(HistoryError::InvalidCase(
            "starting case exceeds configured history bounds".to_string(),
        ));
    }
    if !valid(original) {
        return Err(HistoryError::InvalidCase(
            "cannot shrink an invalid starting case".to_string(),
        ));
    }
    let initial = observe(original);
    match initial.status {
        ComparisonStatus::Mismatch => {}
        ComparisonStatus::InvalidOracle | ComparisonStatus::Contaminated => {
            return Err(HistoryError::InvalidOracle(
                initial
                    .reason
                    .clone()
                    .unwrap_or_else(|| "comparison rejected the oracle".to_string()),
            ));
        }
        _ => return Err(HistoryError::NoFailure),
    }
    let mut current = original.clone();
    let mut current_record = initial.clone();
    let mut attempts = 0usize;
    loop {
        let mut changed = false;
        for index in 0..current.operations.len() {
            if attempts >= bounds.max_shrink_attempts {
                break;
            }
            let mut remove = BTreeSet::new();
            remove.insert(index as u32);
            let Some(candidate) = current.without_indices(&remove) else {
                continue;
            };
            attempts += 1;
            if candidate.validate().is_err()
                || !case_within_bounds(&candidate, bounds)
                || !valid(&candidate)
            {
                continue;
            }
            let candidate_record = observe(&candidate);
            if preserves_failure(&current_record, &candidate_record) {
                current = candidate;
                current_record = candidate_record;
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        'values: for operation_index in 0..current.operations.len() {
            let argument_count = current.operations[operation_index].arguments.len();
            for argument_index in 0..argument_count {
                let values = current.operations[operation_index].arguments[argument_index].shrinks();
                for value in values {
                    if attempts >= bounds.max_shrink_attempts {
                        break 'values;
                    }
                    let Some(candidate) =
                        current.with_argument(operation_index, argument_index, value)
                    else {
                        continue;
                    };
                    attempts += 1;
                    if candidate.validate().is_err()
                        || !case_within_bounds(&candidate, bounds)
                        || !valid(&candidate)
                    {
                        continue;
                    }
                    let candidate_record = observe(&candidate);
                    if preserves_failure(&current_record, &candidate_record) {
                        current = candidate;
                        current_record = candidate_record;
                        changed = true;
                        break 'values;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    Ok((current, current_record))
}

fn preserves_failure(previous: &ComparisonRecord, candidate: &ComparisonRecord) -> bool {
    if candidate.status != ComparisonStatus::Mismatch || previous.relation != candidate.relation {
        return false;
    }
    let Some(previous_sample) = previous.first_mismatch() else {
        return false;
    };
    let Some(candidate_sample) = candidate.first_mismatch() else {
        return false;
    };
    previous_sample.reference.typed_failure == candidate_sample.reference.typed_failure
        && previous_sample.candidate.typed_failure == candidate_sample.candidate.typed_failure
        && previous_sample.reference.mutation == candidate_sample.reference.mutation
        && previous_sample.candidate.mutation == candidate_sample.candidate.mutation
        && previous_sample.reference.cleanup == candidate_sample.reference.cleanup
        && previous_sample.candidate.cleanup == candidate_sample.candidate.cleanup
}

/// Run a bounded campaign.  Invalid, unavailable, and unsupported cases count
/// as discarded; oracle contamination is a hard error and never a pass.
pub struct HistoryRunner {
    config: HistoryConfig,
}

impl HistoryRunner {
    pub fn new(config: HistoryConfig) -> Result<Self, HistoryError> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn run<G, F>(&self, generate: G, observe: F) -> Result<HistoryRun, HistoryError>
    where
        G: FnMut(&mut HistoryRng, usize) -> Option<HistoryCase>,
        F: FnMut(&HistoryCase) -> ComparisonRecord,
    {
        self.run_with_validity(generate, |_| true, observe)
    }

    pub fn run_with_validity<G, V, F>(
        &self,
        mut generate: G,
        mut valid: V,
        mut observe: F,
    ) -> Result<HistoryRun, HistoryError>
    where
        G: FnMut(&mut HistoryRng, usize) -> Option<HistoryCase>,
        V: FnMut(&HistoryCase) -> bool,
        F: FnMut(&HistoryCase) -> ComparisonRecord,
    {
        let mut rng = HistoryRng::new(self.config.seed);
        let mut explored_cases = 0usize;
        let mut discarded_cases = 0usize;
        let mut vacuous_preconditions = 0usize;
        for case_index in 0..self.config.cases {
            let Some(case) = generate(&mut rng, case_index) else {
                discarded_cases += 1;
                if discarded_cases > self.config.bounds.max_discarded_cases {
                    return Err(HistoryError::InvalidConfig(
                        "discarded cases exceeded the configured bound".to_string(),
                    ));
                }
                continue;
            };
            if case.validate().is_err()
                || !case_within_bounds(&case, self.config.bounds)
                || !valid(&case)
            {
                discarded_cases += 1;
                if discarded_cases > self.config.bounds.max_discarded_cases {
                    return Err(HistoryError::InvalidConfig(
                        "discarded cases exceeded the configured bound".to_string(),
                    ));
                }
                continue;
            }
            vacuous_preconditions += case.vacuous_preconditions();
            let record = observe(&case);
            match record.status {
                ComparisonStatus::InvalidOracle | ComparisonStatus::Contaminated => {
                    return Err(HistoryError::InvalidOracle(
                        record
                            .reason
                            .unwrap_or_else(|| "comparison rejected the oracle".to_string()),
                    ));
                }
                ComparisonStatus::Mismatch => {
                    let (reduced_case, reduced_record) = shrink_history_with_validity(
                        &case,
                        self.config.bounds,
                        &mut valid,
                        &mut observe,
                    )?;
                    let artifact = HistoryArtifact::new(
                        &self.config,
                        reduced_case,
                        reduced_record,
                        discarded_cases,
                    )?;
                    return Ok(HistoryRun {
                        seed: self.config.seed,
                        cases: self.config.cases,
                        bounds: self.config.bounds,
                        distributions: self.config.distributions.clone(),
                        explored_cases: explored_cases + 1,
                        discarded_cases,
                        vacuous_preconditions,
                        failure: Some(artifact),
                    });
                }
                ComparisonStatus::Matched => {
                    explored_cases += 1;
                }
                ComparisonStatus::Empty
                | ComparisonStatus::Unsupported
                | ComparisonStatus::Unavailable
                | ComparisonStatus::Timeout
                | ComparisonStatus::Cancelled => {
                    explored_cases += 1;
                    discarded_cases += 1;
                    if discarded_cases > self.config.bounds.max_discarded_cases {
                        return Err(HistoryError::InvalidConfig(
                            "discarded cases exceeded the configured bound".to_string(),
                        ));
                    }
                }
            }
        }
        Ok(HistoryRun {
            seed: self.config.seed,
            cases: self.config.cases,
            bounds: self.config.bounds,
            distributions: self.config.distributions.clone(),
            explored_cases,
            discarded_cases,
            vacuous_preconditions,
            failure: None,
        })
    }
    /// Run a campaign through a typed command strategy.  Shrinking rebuilds
    /// the command vector from each candidate case before domain validation,
    /// so a reduced metadata row cannot silently become an untyped operation.
    pub fn run_strategy<Command, Strategy, F>(
        &self,
        strategy: &Strategy,
        observe: F,
    ) -> Result<HistoryRun, HistoryError>
    where
        Command: HistoryCommand,
        Strategy: HistoryStrategyBehavior<Command>,
        F: FnMut(&TypedHistoryCase<Command>) -> ComparisonRecord,
    {
        if self.config.command_type != strategy.command_type() {
            return Err(HistoryError::InvalidConfig(format!(
                "history command type `{}` does not match strategy `{}`",
                self.config.command_type,
                strategy.command_type()
            )));
        }
        if let Some(reason) = strategy.unsupported_reason() {
            return Err(HistoryError::InvalidConfig(reason.to_string()));
        }
        let expected_distributions = strategy.distributions();
        if self.config.distributions != expected_distributions {
            return Err(HistoryError::InvalidConfig(
                "history distributions do not match the command strategy".to_string(),
            ));
        }
        let mut observe = observe;
        let relation = self.config.relation.clone();
        self.run_with_validity(
            |rng, index| strategy.generate(rng, index, self.config.bounds.max_steps).map(|typed| typed.case),
            |case| {
                strategy
                    .rebuild(case)
                    .as_ref()
                    .is_some_and(|typed| strategy.valid(typed))
            },
            |case| match strategy.rebuild(case) {
                Some(typed) => observe(&typed),
                None => ComparisonRecord::terminal(
                    relation.clone(),
                    ComparisonStatus::Unavailable,
                    "history strategy could not rebuild a typed operation".to_string(),
                ),
            },
        )
    }
}

fn push_booking_operation(
    case: &mut HistoryCase,
    name: &str,
    argument: Option<HandleId>,
    creates: Option<HandleId>,
    consumes: Option<HandleId>,
    state: Option<&str>,
) {
    let index = case.operations.len() as u32;
    let task = TaskId { value: index % 2 };
    let event = EventId { value: index + 1 };
    let mut operation = HistoryOperation::new(index, name).scheduled_on(task, event);
    if index > 0 {
        let previous = index - 1;
        operation = operation
            .depends_on(previous)
            .requires(HistoryPrecondition::TaskCompleted(TaskId {
                value: previous % 2,
            }))
            .requires(HistoryPrecondition::EventAvailable(EventId {
                value: previous + 1,
            }));
    }
    if let Some(argument) = argument {
        operation = operation.with_argument(HistoryValue::Handle(argument));
    }
    if let Some(creates) = creates {
        operation = operation.creates(creates);
    }
    if let Some(consumes) = consumes {
        operation = operation.consumes(consumes);
    }
    if let Some(state) = state {
        if let Some(argument) = argument {
            operation = operation.requires(HistoryPrecondition::HandleState {
                handle: argument,
                state: state.to_string(),
            });
        }
    }
    case.push(operation);
    case.schedule(
        HistoryScheduleChoice::new(index, if name == "timeout" { "deliver-late" } else { "fifo" })
            .for_task_event(task, event),
    );
}

/// A small domain fixture used by the executable documentation and by focused
/// tests.  It deliberately injects a retry-after-timeout defect into only the
/// candidate side; the reference transition table is independent.
///
/// The public generator below keeps the fixture useful to callers that want a
/// deterministic counterexample without adopting a private booking model.
pub fn generate_booking_history_case(
    rng: &mut HistoryRng,
    case_index: usize,
    max_steps: usize,
) -> Option<HistoryCase> {
    if max_steps < 3 {
        return None;
    }
    let case_seed = rng.next_u64();
    let mut local = HistoryRng::new(case_seed);
    let mut case = HistoryCase::new(format!("booking-{case_index}"), case_seed);
    let mut next_handle = 1u32;
    let mut active = Vec::new();
    let mut terminal = Vec::new();

    let first = HandleId { value: next_handle };
    next_handle += 1;
    push_booking_operation(&mut case, "reserve", None, Some(first), None, None);
    push_booking_operation(
        &mut case,
        "timeout",
        Some(first),
        None,
        None,
        Some("active"),
    );
    terminal.push((first, "timed_out"));

    let second = HandleId { value: next_handle };
    next_handle += 1;
    push_booking_operation(
        &mut case,
        "retry",
        Some(first),
        Some(second),
        Some(first),
        Some("timed_out"),
    );
    active.push(second);

    while case.operations.len() < max_steps {
        let choice = local.below(8);
        if choice < 3 || (active.is_empty() && terminal.is_empty()) {
            let handle = HandleId { value: next_handle };
            next_handle += 1;
            push_booking_operation(&mut case, "reserve", None, Some(handle), None, None);
            active.push(handle);
        } else if choice < 5 && !active.is_empty() {
            let index = local.below(active.len() as u64) as usize;
            let handle = active[index];
            push_booking_operation(
                &mut case,
                "cancel",
                Some(handle),
                None,
                None,
                Some("active"),
            );
            active.swap_remove(index);
            terminal.push((handle, "cancelled"));
        } else if choice < 7 && !active.is_empty() {
            let index = local.below(active.len() as u64) as usize;
            let handle = active[index];
            push_booking_operation(
                &mut case,
                "timeout",
                Some(handle),
                None,
                None,
                Some("active"),
            );
            active.swap_remove(index);
            terminal.push((handle, "timed_out"));
        } else if !terminal.is_empty() {
            let index = local.below(terminal.len() as u64) as usize;
            let (handle, retry_state) = terminal.swap_remove(index);
            let replacement = HandleId { value: next_handle };
            next_handle += 1;
            push_booking_operation(
                &mut case,
                "retry",
                Some(handle),
                Some(replacement),
                Some(handle),
                Some(retry_state),
            );
            active.push(replacement);
        } else {
            let handle = HandleId { value: next_handle };
            next_handle += 1;
            push_booking_operation(&mut case, "reserve", None, Some(handle), None, None);
            active.push(handle);
        }
    }
    Some(case)
}

fn booking_case_is_valid(case: &HistoryCase) -> bool {
    if case.validate().is_err() || case.schedule.len() != case.operations.len() {
        return false;
    }
    let mut states = BTreeMap::<HandleId, &'static str>::new();
    for (position, operation) in case.operations.iter().enumerate() {
        let index = position as u32;
        if operation.task != Some(TaskId { value: index % 2 })
            || operation.event != Some(EventId { value: index + 1 })
            || (position > 0
                && (operation.depends_on.as_slice() != [index - 1]
                    || !operation.preconditions.iter().any(|precondition| {
                        matches!(
                            precondition,
                            HistoryPrecondition::TaskCompleted(task)
                                if *task == (TaskId {
                                    value: (index - 1) % 2,
                                })
                        )
                    })
                    || !operation.preconditions.iter().any(|precondition| {
                        matches!(
                            precondition,
                            HistoryPrecondition::EventAvailable(event)
                            if *event == (EventId {
                                value: index,
                            })
                        )
                    })))
        {
            return false;
        }
        let choice = &case.schedule[position];
        if choice.operation != index
            || choice.task != operation.task
            || choice.event != operation.event
            || choice.choice
                != if operation.name == "timeout" {
                    "deliver-late"
                } else {
                    "fifo"
                }
        {
            return false;
        }
        let handle = operation
            .arguments
            .iter()
            .find_map(HistoryValue::handles);
        match operation.name.as_str() {
            "reserve" => {
                if operation.creates.len() != 1
                    || handle.is_some()
                    || !operation.consumes.is_empty()
                {
                    return false;
                }
                states.insert(operation.creates[0], "active");
            }
            "cancel" | "timeout" => {
                let Some(handle) = handle else { return false };
                let expected_state = if operation.name == "cancel" {
                    "active"
                } else {
                    "active"
                };
                if states.get(&handle) != Some(&expected_state)
                    || !operation.creates.is_empty()
                    || !operation.consumes.is_empty()
                    || !operation.preconditions.iter().any(|precondition| {
                        matches!(
                            precondition,
                            HistoryPrecondition::HandleState {
                                handle: precondition_handle,
                                state
                            } if *precondition_handle == handle && state == expected_state
                        )
                    })
                {
                    return false;
                }
                let next_state = if operation.name == "cancel" {
                    "cancelled"
                } else {
                    "timed_out"
                };
                states.insert(handle, next_state);
            }
            "retry" => {
                let Some(handle) = handle else { return false };
                if !matches!(
                    states.get(&handle),
                    Some(&"cancelled") | Some(&"timed_out")
                ) || operation.creates.len() != 1
                    || operation.consumes.as_slice() != [handle]
                    || !operation.preconditions.iter().any(|precondition| {
                        matches!(
                            precondition,
                            HistoryPrecondition::HandleState {
                                handle: precondition_handle,
                                state
                            } if *precondition_handle == handle
                                && (state == "cancelled" || state == "timed_out")
                        )
                    })
                {
                    return false;
                }
                states.remove(&handle);
                states.insert(operation.creates[0], "active");
            }
            _ => return false,
        }
    }
    true
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BookingState {
    Active,
    Cancelled,
    TimedOut,
}

#[derive(Clone, Debug, Default)]
struct BookingWorld {
    states: BTreeMap<HandleId, BookingState>,
    effects: Vec<String>,
}

impl BookingWorld {
    fn apply_reference(&mut self, operation: &HistoryOperation) {
        let handle = operation
            .arguments
            .iter()
            .find_map(HistoryValue::handles);
        match operation.name.as_str() {
            "reserve" => {
                if let Some(created) = operation.creates.first().copied() {
                    self.states.insert(created, BookingState::Active);
                    self.effects.push(format!("reserve:{}", created.value));
                }
            }
            "cancel" => {
                if let Some(handle) = handle {
                    self.states.insert(handle, BookingState::Cancelled);
                    self.effects.push(format!("cancel:{}", handle.value));
                }
            }
            "timeout" => {
                if let Some(handle) = handle {
                    self.states.insert(handle, BookingState::TimedOut);
                    self.effects.push(format!("timeout:{}", handle.value));
                }
            }
            "retry" => {
                if let (Some(handle), Some(created)) = (handle, operation.creates.first().copied()) {
                    self.states.remove(&handle);
                    self.states.insert(created, BookingState::Active);
                    self.effects.push(format!("retry:{}->{}", handle.value, created.value));
                }
            }
            _ => {}
        }
    }

    fn apply_candidate(&mut self, operation: &HistoryOperation, injected_defect: bool) {
        let handle = operation
            .arguments
            .iter()
            .find_map(HistoryValue::handles);
        match operation.name.as_str() {
            "reserve" => {
                if let Some(created) = operation.creates.first().copied() {
                    self.states.insert(created, BookingState::Active);
                    self.effects.push(format!("reserve:{}", created.value));
                }
            }
            "cancel" => {
                if let Some(handle) = handle {
                    self.states.insert(handle, BookingState::Cancelled);
                    self.effects.push(format!("cancel:{}", handle.value));
                }
            }
            "timeout" => {
                if let Some(handle) = handle {
                    self.states.insert(handle, BookingState::TimedOut);
                    self.effects.push(format!("timeout:{}", handle.value));
                }
            }
            "retry" => {
                if let (Some(handle), Some(created)) = (handle, operation.creates.first().copied()) {
                    let timed_out = self.states.get(&handle) == Some(&BookingState::TimedOut);
                    if !(injected_defect && timed_out) {
                        self.states.remove(&handle);
                    }
                    self.states.insert(created, BookingState::Active);
                    self.effects.push(format!("retry:{}->{}", handle.value, created.value));
                }
            }
            _ => {}
        }
    }

    fn observation(&self) -> ComparisonObservation {
        let raw = self
            .states
            .iter()
            .map(|(handle, state)| {
                let state = match state {
                    BookingState::Active => "active",
                    BookingState::Cancelled => "cancelled",
                    BookingState::TimedOut => "timed_out",
                };
                format!("{}={state}", handle.value)
            })
            .collect::<Vec<_>>()
            .join(",");
        ComparisonObservation::failure(raw, "booking_state")
            .with_mutation("booking lifecycle")
            .with_effects(self.effects.clone())
            .with_cleanup(["booking handles released"])
    }
}

/// Compare one booking case through the canonical comparison engine.
pub fn compare_booking_case(
    case: &HistoryCase,
    injected_defect: bool,
) -> Result<ComparisonRecord, HistoryError> {
    if !booking_case_is_valid(case) {
        return Err(HistoryError::InvalidCase(
            "booking case violates a state precondition".to_string(),
        ));
    }
    let mut reference = BookingWorld::default();
    let mut candidate = BookingWorld::default();
    for operation in &case.operations {
        reference.apply_reference(operation);
        candidate.apply_candidate(operation, injected_defect);
    }
    let mut identity = ComparisonIdentity::new(
        case.case_id.clone(),
        case.input_id(),
        "examples/features/tooling/test_history/run.jet",
        HISTORY_ENGINE,
        "booking",
    );
    identity.seed = Some(case.seed);
    Ok(compare_samples(
        ObservationRelation::OrderedEffects,
        [ComparisonSample::new(
            identity,
            reference.observation(),
            candidate.observation(),
        )],
    ))
}

/// Deterministic executable fixture for the history example and focused
/// integration checks.  The injected candidate defect must yield a reduced
/// artifact; without it the bounded campaign is matched.
pub fn run_booking_history_campaign(
    seed: u64,
    cases: usize,
    injected_defect: bool,
) -> Result<HistoryRun, HistoryError> {
    let bounds = HistoryBounds {
        max_steps: 8,
        max_resources: 32,
        max_shrink_attempts: 10_000,
        max_discarded_cases: cases,
    };
    let strategy = BookingHistoryStrategy::new();
    let config = HistoryConfig::new(
        seed,
        cases,
        bounds,
        "examples/features/tooling/test_history/run.jet",
        HISTORY_ENGINE,
        "booking",
        ObservationRelation::OrderedEffects,
        OracleDeclaration::independent("booking.reference"),
    )
    .with_command_type(strategy.command_type());
    let config = <BookingHistoryStrategy as HistoryStrategyBehavior<BookingCommand>>::distributions(&strategy)
        .into_iter()
        .fold(config, |config, distribution| config.with_distribution(distribution));
    let runner = HistoryRunner::new(config)?;
    runner.run_strategy::<BookingCommand, _, _>(&strategy, |typed| {
        let case = &typed.case;
        compare_booking_case(case, injected_defect).unwrap_or_else(|error| {
            let identity = ComparisonIdentity::new(
                case.case_id.clone(),
                case.input_id(),
                "examples/features/tooling/test_history/run.jet",
                HISTORY_ENGINE,
                "booking",
            );
            ComparisonRecord {
                schema_version: crate::TestingComparison::TEST_COMPARISON_SCHEMA_VERSION,
                status: ComparisonStatus::InvalidOracle,
                relation: ObservationRelation::OrderedEffects,
                samples: vec![ComparisonSample::new(
                    identity,
                    ComparisonObservation::failure("invalid", "history_error"),
                    ComparisonObservation::failure(error.to_string(), "history_error"),
                )],
                discarded_cases: 0,
                first_difference: None,
                reduced_counterexample: None,
                contamination: None,
                reason: Some(error.to_string()),
                universal_proof: false,
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injected_booking_defect_reduces_to_valid_replay() {
        let run = run_booking_history_campaign(41, 4, true).expect("campaign");
        let artifact = run.failure.expect("injected defect must mismatch");
        assert!(artifact.case.operations.len() <= 3);
        assert!(booking_case_is_valid(&artifact.case));
        assert_eq!(artifact.comparison.status, ComparisonStatus::Mismatch);
        assert_eq!(artifact.identity.input_id, artifact.case.input_id());
        assert!(artifact.json().contains("\"schedule\""));
    }

    #[test]
    fn matched_booking_campaign_is_not_a_universal_proof() {
        let run = run_booking_history_campaign(41, 4, false).expect("campaign");
        assert!(!run.has_failure());
    }

    #[test]
    fn shrinker_preserves_handles_and_domain_dependencies() {
        let run = run_booking_history_campaign(41, 4, true).expect("campaign");
        let artifact = run.failure.expect("mismatch");
        artifact.case.validate().expect("generic dependencies");
        let names = artifact
            .case
            .operations
            .iter()
            .map(|operation| operation.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["reserve", "timeout", "retry"]);
        assert_eq!(
            artifact.case.operations[2].consumes,
            [HandleId { value: 1 }]
        );
        assert_eq!(
            artifact.case.operations[2].creates,
            [HandleId { value: 2 }]
        );
    }

    #[test]
    fn generated_histories_cover_operations_and_dependencies() {
        let mut seen = BTreeSet::new();
        for seed in 0..256 {
            let mut rng = HistoryRng::new(seed);
            let case = generate_booking_history_case(&mut rng, seed as usize, 8)
                .expect("booking generator produces a bounded case");
            for (position, operation) in case.operations.iter().enumerate() {
                seen.insert(operation.name.clone());
                assert_eq!(
                    operation.task,
                    Some(TaskId {
                        value: (position as u32) % 2,
                    })
                );
                assert_eq!(
                    operation.event,
                    Some(EventId {
                        value: position as u32 + 1,
                    })
                );
                if position > 0 {
                    assert_eq!(operation.depends_on, [position as u32 - 1]);
                    assert!(operation
                        .preconditions
                        .iter()
                        .any(|precondition| matches!(
                            precondition,
                            HistoryPrecondition::TaskCompleted(task)
                                if *task
                                    == (TaskId {
                                        value: (position as u32 - 1) % 2,
                                    })
                        )));
                    assert!(operation
                        .preconditions
                        .iter()
                        .any(|precondition| matches!(
                            precondition,
                            HistoryPrecondition::EventAvailable(event)
                                if *event
                                    == (EventId {
                                        value: position as u32,
                                    })
                        )));
                }
            }
            if ["reserve", "cancel", "timeout", "retry"]
                .iter()
                .all(|name| seen.contains(*name))
            {
                break;
            }
        }
        assert!(["reserve", "cancel", "timeout", "retry"]
            .iter()
            .all(|name| seen.contains(*name)));
    }

    #[test]
    fn history_artifact_repeats_and_replays_same_failure() {
        let first = run_booking_history_campaign(41, 4, true)
            .expect("campaign")
            .failure
            .expect("injected defect");
        let second = run_booking_history_campaign(41, 4, true)
            .expect("campaign")
            .failure
            .expect("injected defect");
        assert_eq!(first.json(), second.json());
        let replay = compare_booking_case(&first.case, true).expect("replay");
        assert_eq!(replay.status, ComparisonStatus::Mismatch);
        let original = first.comparison.first_mismatch().expect("artifact mismatch");
        let replayed = replay.first_mismatch().expect("replay mismatch");
        assert_eq!(replayed.reference.typed_failure, original.reference.typed_failure);
        assert_eq!(replayed.candidate.typed_failure, original.candidate.typed_failure);
        assert_eq!(replayed.reference.mutation, original.reference.mutation);
        assert_eq!(replayed.candidate.mutation, original.candidate.mutation);
    }

    #[test]
    fn discarded_cases_stop_at_the_declared_bound() {
        let config = HistoryConfig::new(
            41,
            1,
            HistoryBounds {
                max_steps: 1,
                max_resources: 1,
                max_shrink_attempts: 1,
                max_discarded_cases: 0,
            },
            "test",
            "typed-history-v1",
            "unit",
            ObservationRelation::TypedEquality,
            OracleDeclaration::independent("model"),
        );
        let runner = HistoryRunner::new(config).expect("valid config");
        let error = runner
            .run(|_, _| None, |_| unreachable!("discarded cases are not observed"))
            .expect_err("discard bound must be enforced");
        assert_eq!(
            error,
            HistoryError::InvalidConfig("discarded cases exceeded the configured bound".to_string())
        );
    }

    #[test]
    fn oracle_validation_rejects_direct_transitive_and_opaque_edges() {
        let direct = OracleDeclaration::independent("model")
            .with_dependency(OracleDependency::new(
                "model",
                "production",
                OracleDependencyKind::ProductionBehavior,
            ));
        assert!(matches!(
            direct.validate_independence(),
            Err(HistoryError::InvalidOracle(_))
        ));
        let transitive = OracleDeclaration::independent("model")
            .with_dependency(OracleDependency::new(
                "model",
                "helper",
                OracleDependencyKind::Independent,
            ))
            .with_dependency(OracleDependency::new(
                "helper",
                "production",
                OracleDependencyKind::ProductionBehavior,
            ));
        assert!(transitive.validate_independence().is_err());
        let opaque = OracleDeclaration::independent("model").with_dependency(
            OracleDependency::new("model", "opaque", OracleDependencyKind::Opaque),
        );
        assert!(opaque.validate_independence().is_err());
    }

    #[test]
    fn secret_constructor_never_retains_secret_value() {
        assert_eq!(
            HistoryValue::secret("password=not-persisted", "credential"),
            HistoryValue::Redacted("credential".to_string())
        );
    }
}
