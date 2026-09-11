//! Canonical frame-resource scheduling facts shared by sema and every runtime.
//!
//! A schedule is derived from checked operation contracts.  This module never
//! guesses a device, an alias, or an asynchronous completion event.  The
//! serial reference plan keeps the source order and records only the edges and
//! conservative decisions needed by an optimizer or an inspector.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex};

/// Access mode proved by a checked call signature.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetResourceAccessMode {
    Read,
    Write,
    Move,
}

impl JetResourceAccessMode {
    pub const fn is_write(self) -> bool {
        matches!(self, Self::Write | Self::Move)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Move => "move",
        }
    }
}

/// Resource identity after sema has resolved an argument place.
///
/// `Unknown` is an explicit checked fact.  It is never treated as disjoint
/// from a named resource, so a serial plan remains safe without fabricating an
/// alias proof.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetResourceIdentity {
    Named(String),
    Unknown,
}

impl JetResourceIdentity {
    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(name.into())
    }

    pub const fn unknown() -> Self {
        Self::Unknown
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Named(name) => name.as_str(),
            Self::Unknown => "unknown resource",
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Named(left), Self::Named(right)) => left == right,
            _ => true,
        }
    }
}

/// Proven resource region.  A whole/unknown region overlaps every region.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetResourceRegion {
    Whole,
    Bytes { start: u64, length: u64 },
    Unknown,
}

impl JetResourceRegion {
    pub const fn whole() -> Self {
        Self::Whole
    }

    pub const fn unknown() -> Self {
        Self::Unknown
    }

    pub const fn bytes(start: u64, length: u64) -> Self {
        Self::Bytes { start, length }
    }

    fn validate(&self, operation: &str, resource: &str) -> Result<(), JetFrameScheduleError> {
        if let Self::Bytes { start, length } = self {
            if *length == 0 || start.checked_add(*length).is_none() {
                return Err(JetFrameScheduleError::InvalidAccess {
                    operation: operation.to_string(),
                    resource: resource.to_string(),
                    reason: "byte region must be non-empty and fit its checked range".to_string(),
                });
            }
        }
        Ok(())
    }

    fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bytes { start: left, length: left_len },
             Self::Bytes { start: right, length: right_len }) => {
                let left_end = left.saturating_add(*left_len);
                let right_end = right.saturating_add(*right_len);
                *left < right_end && *right < left_end
            }
            _ => true,
        }
    }
}

/// Layout/device facts required before temporary storage may be reused.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetResourceLayout {
    pub size: u64,
    pub align: u64,
    pub layout: String,
    /// Empty means that placement is unknown and must not be selected here.
    pub device: String,
}

impl JetResourceLayout {
    pub fn new(
        size: u64,
        align: u64,
        layout: impl Into<String>,
        device: impl Into<String>,
    ) -> Self {
        Self {
            size,
            align,
            layout: layout.into(),
            device: device.into(),
        }
    }

    fn validate(&self, operation: &str, resource: &str) -> Result<(), JetFrameScheduleError> {
        if self.size == 0 || self.align == 0 || !self.align.is_power_of_two() {
            return Err(JetFrameScheduleError::InvalidAccess {
                operation: operation.to_string(),
                resource: resource.to_string(),
                reason: "layout size must be positive and alignment must be a non-zero power of two"
                    .to_string(),
            });
        }
        if self.layout.trim().is_empty() {
            return Err(JetFrameScheduleError::InvalidAccess {
                operation: operation.to_string(),
                resource: resource.to_string(),
                reason: "layout identity must not be empty".to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetResourceAccess {
    pub resource: JetResourceIdentity,
    pub mode: JetResourceAccessMode,
    pub region: JetResourceRegion,
    pub layout: Option<JetResourceLayout>,
    pub temporary: bool,
}

impl JetResourceAccess {
    pub fn named(resource: impl Into<String>, mode: JetResourceAccessMode) -> Self {
        Self {
            resource: JetResourceIdentity::named(resource),
            mode,
            region: JetResourceRegion::Whole,
            layout: None,
            temporary: false,
        }
    }

    pub fn unknown(mode: JetResourceAccessMode) -> Self {
        Self {
            resource: JetResourceIdentity::Unknown,
            mode,
            region: JetResourceRegion::Unknown,
            layout: None,
            temporary: false,
        }
    }

    pub fn with_region(mut self, region: JetResourceRegion) -> Self {
        self.region = region;
        self
    }

    pub fn with_layout(mut self, layout: JetResourceLayout) -> Self {
        self.layout = Some(layout);
        self
    }

    pub const fn temporary(mut self) -> Self {
        self.temporary = true;
        self
    }

    fn label(&self) -> &str {
        self.resource.as_str()
    }
}

/// Completion contract for one checked operation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetFrameCompletion {
    /// All accesses finish before the operation returns.
    Synchronous,
    /// Accesses continue until a real driver event with this token occurs.
    Pending { token: String },
    /// This operation is the checked driver event for a pending token.
    Event { token: String },
}

impl Default for JetFrameCompletion {
    fn default() -> Self {
        Self::Synchronous
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetFrameOperation {
    pub name: String,
    pub source_index: usize,
    pub accesses: Vec<JetResourceAccess>,
    pub completion: JetFrameCompletion,
    /// Canonical provider identity for a real completion boundary. `None`
    /// means the operation is synchronous or has no runtime adapter.
    pub completion_provider: Option<String>,
    pub external_effect: bool,
}

impl JetFrameOperation {
    pub fn new(name: impl Into<String>, source_index: usize) -> Self {
        Self {
            name: name.into(),
            source_index,
            accesses: Vec::new(),
            completion: JetFrameCompletion::Synchronous,
            completion_provider: None,
            external_effect: false,
        }
    }

    pub fn with_access(mut self, access: JetResourceAccess) -> Self {
        self.accesses.push(access);
        self
    }

    pub fn with_accesses(mut self, accesses: impl IntoIterator<Item = JetResourceAccess>) -> Self {
        self.accesses.extend(accesses);
        self
    }

    pub fn pending(mut self, token: impl Into<String>) -> Self {
        self.completion = JetFrameCompletion::Pending {
            token: token.into(),
        };
        self
    }

    pub fn completion_event(mut self, token: impl Into<String>) -> Self {
        self.completion = JetFrameCompletion::Event {
            token: token.into(),
        };
        self
    }

    pub fn with_completion_provider(mut self, provider: impl Into<String>) -> Self {
        self.completion_provider = Some(provider.into());
        self
    }

    pub const fn external_effect(mut self) -> Self {
        self.external_effect = true;
        self
    }
}


/// Whether a caller explicitly requests operations that may overlap.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JetFrameScheduleOptions {
    pub permit_parallel: bool,
}

impl JetFrameScheduleOptions {
    pub const fn serial() -> Self {
        Self {
            permit_parallel: false,
        }
    }

    pub const fn parallel() -> Self {
        Self {
            permit_parallel: true,
        }
    }
}

/// One source-order dependency in the derived schedule.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetFrameDependency {
    pub before: usize,
    pub after: usize,
    pub resource: String,
    pub reason: String,
}

/// One checked device transfer; the scheduler never chooses either device.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetFrameTransfer {
    pub before: usize,
    pub after: usize,
    pub resource: String,
    pub from_device: String,
    pub to_device: String,
}

/// A temporary-storage reuse decision, including conservative refusals.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetFrameReuse {
    pub previous_resource: String,
    pub next_resource: String,
    pub previous_last_use: usize,
    pub next_first_use: usize,
    pub legal: bool,
    pub reason: String,
}

/// Ownership retained across asynchronous device work.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetFrameRetention {
    pub resource: String,
    pub operation: usize,
    pub owner: String,
    pub completion_token: Option<String>,
    pub completed: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetFrameSchedule {
    pub operations: Vec<JetFrameOperation>,
    pub dependencies: Vec<JetFrameDependency>,
    pub transfers: Vec<JetFrameTransfer>,
    pub reuse: Vec<JetFrameReuse>,
    pub retentions: Vec<JetFrameRetention>,
    pub conservative: Vec<String>,
}

/// Runtime state for the completion tokens already proved by a frame
/// schedule. It is deliberately a consumer of the canonical rows: it never
/// derives dependencies, reuse, or retention decisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetFrameRetentionGuard {
    resource: String,
    owner: String,
    /// The guard is an owned lifetime carrier, not a second resource-name
    /// index. Runtime adapters clone it into the in-flight owner they submit.
    lifetime: Arc<()>,
}

impl JetFrameRetentionGuard {
    fn new(resource: impl Into<String>, owner: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            owner: owner.into(),
            lifetime: Arc::new(()),
        }
    }
    fn is_live(&self) -> bool {
        Arc::strong_count(&self.lifetime) > 0
    }


    pub fn resource(&self) -> &str {
        &self.resource
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }
}

/// Exact checked completion identity captured when a provider submits work.
/// The handle carries the owning retention guards until its provider signals
/// the matching event; provider-wide sweeps cannot complete another submit.
#[derive(Clone, Debug)]
pub struct JetFrameCompletionHandle {
    state: Arc<Mutex<JetFrameCompletionState>>,
    token: String,
    event_provider: String,
    generation: u64,
    guards: Vec<JetFrameRetentionGuard>,
}

impl JetFrameCompletionHandle {
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn event_provider(&self) -> &str {
        &self.event_provider
    }

    pub fn signal(&self) -> Result<(), String> {
        signal_frame_completion_binding_in(
            &self.state,
            &self.event_provider,
            &self.token,
            self.generation,
        )
    }

    pub fn retention_guards(&self) -> &[JetFrameRetentionGuard] {
        &self.guards
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetFrameCompletionState {
    schedule: Option<Arc<JetFrameSchedule>>,
    generation: u64,
    completed_tokens: BTreeSet<String>,
    retained_resources: BTreeSet<String>,
    retention_guards: BTreeMap<String, Vec<JetFrameRetentionGuard>>,
    bound_tokens: BTreeSet<String>,
}
fn frame_retention_guards(
    schedule: &JetFrameSchedule,
) -> (
    BTreeMap<String, Vec<JetFrameRetentionGuard>>,
    BTreeSet<String>,
) {
    let mut guards = BTreeMap::<String, Vec<JetFrameRetentionGuard>>::new();
    for retention in &schedule.retentions {
        if let Some(token) = &retention.completion_token {
            guards
                .entry(token.clone())
                .or_default()
                .push(JetFrameRetentionGuard::new(
                    retention.resource.clone(),
                    retention.owner.clone(),
                ));
        }
    }
    let resources = guards
        .values()
        .flatten()
        .filter(|guard| guard.is_live())
        .map(|guard| guard.resource.clone())
        .collect();
    (guards, resources)
}


impl Default for JetFrameCompletionState {
    fn default() -> Self {
        Self {
            schedule: None,
            generation: 0,
            completed_tokens: BTreeSet::new(),
            retained_resources: BTreeSet::new(),
            retention_guards: BTreeMap::new(),
            bound_tokens: BTreeSet::new(),
        }
    }
}

impl JetFrameCompletionState {
    pub fn new(schedule: &JetFrameSchedule) -> Self {
        let (retention_guards, retained_resources) = frame_retention_guards(schedule);
        Self {
            schedule: Some(Arc::new(schedule.clone())),
            generation: 0,
            completed_tokens: BTreeSet::new(),
            retained_resources,
            retention_guards,
            bound_tokens: BTreeSet::new(),
        }
    }

    pub fn begin_frame(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.completed_tokens.clear();
        self.bound_tokens.clear();
        if let Some(schedule) = self.schedule.clone() {
            let (retention_guards, retained_resources) = frame_retention_guards(&schedule);
            self.retention_guards = retention_guards;
            self.retained_resources = retained_resources;
        } else {
            self.retention_guards.clear();
            self.retained_resources.clear();
        }
    }

    /// Consume one checked completion event and release only the resources
    /// owned by that token. Unknown, duplicate, or stale tokens are rejected.
    pub fn complete(
        &mut self,
        schedule: &JetFrameSchedule,
        token: &str,
    ) -> Result<(), String> {
        if !schedule.operations.iter().any(|operation| {
            matches!(
                &operation.completion,
                JetFrameCompletion::Event { token: event } if event == token
            )
        }) {
            return Err(format!("completion token `{token}` is not in the checked schedule"));
        }
        if !self.completed_tokens.insert(token.to_string()) {
            return Err(format!("completion token `{token}` was already completed"));
        }
        let released = self.retention_guards.remove(token).unwrap_or_default();
        for guard in released {
            if !self
                .retention_guards
                .values()
                .flatten()
                .any(|other| other.resource == guard.resource)
            {
                self.retained_resources.remove(&guard.resource);
            }
        }
        Ok(())
    }

    /// Signal one event from its provider. The provider claims exactly one
    /// unbound event in source order; it never sweeps every matching token.
    pub fn complete_from_provider(&mut self, provider: &str) -> Result<(), String> {
        let Some(schedule) = self.schedule.clone() else {
            return Err("completion state has no checked frame schedule".to_string());
        };
        let token = schedule.operations.iter().find_map(|operation| {
            let JetFrameCompletion::Event { token } = &operation.completion else {
                return None;
            };
            (operation.completion_provider.as_deref() == Some(provider)
                && !self.bound_tokens.contains(token)
                && !self.completed_tokens.contains(token))
                .then(|| token.clone())
        });
        if let Some(token) = token {
            self.bound_tokens.insert(token.clone());
            return self.complete(schedule.as_ref(), &token);
        }
        let has_provider = schedule
            .operations
            .iter()
            .any(|operation| operation.completion_provider.as_deref() == Some(provider));
        if !has_provider {
            return Err(format!(
                "completion provider `{provider}` is not in the checked schedule"
            ));
        }
        let has_event = schedule.operations.iter().any(|operation| {
            matches!(operation.completion, JetFrameCompletion::Event { .. })
                && operation.completion_provider.as_deref() == Some(provider)
        });
        if has_event {
            return Err(format!(
                "completion provider `{provider}` has no unbound checked event"
            ));
        }
        // Synchronous providers may be called for their real completion
        // boundary, but have no event token to consume.
        Ok(())
    }

    pub fn is_completed(&self, token: &str) -> bool {
        self.completed_tokens.contains(token)
    }

    pub fn retained_resources(&self) -> impl Iterator<Item = &str> {
        self.retained_resources.iter().map(String::as_str)
    }

    /// Verify that every legal canonical reuse row has crossed its checked
    /// completion boundary. A runtime cannot silently reuse a retained
    /// resource merely because the frame callback returned.
    pub fn assert_reuse_ready(&self, schedule: &JetFrameSchedule) -> Result<(), String> {
        for reuse in schedule.reuse.iter().filter(|reuse| reuse.legal) {
            for operation in schedule.operations.iter().take(reuse.next_first_use) {
                for access in &operation.accesses {
                    if !matches!(
                        &access.resource,
                        JetResourceIdentity::Named(resource)
                            if resource == &reuse.previous_resource
                    ) {
                        continue;
                    }
                    if let JetFrameCompletion::Pending { token } = &operation.completion {
                        if !self.is_completed(token) {
                            return Err(format!(
                                "resource `{}` cannot be reused before completion token `{token}`",
                                reuse.previous_resource
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
/// Install the checked completion state while one runtime callback invokes
/// providers.  Core adapters signal only after their real wait/event boundary;
/// this scope is not a frame-end completion shortcut.
pub fn with_frame_completion_scope<T>(
    state: Arc<Mutex<JetFrameCompletionState>>,
    body: impl FnOnce() -> T,
) -> T {
    FRAME_COMPLETION_STATE.with(|active| {
        state
            .lock()
            .expect("checked frame completion state is poisoned")
            .begin_frame();
        let previous = active.replace(Some(state));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
        active.replace(previous);
        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    })
}

/// Return the callback's checked completion handle so an asynchronous provider
/// can retain the exact frame state until its later driver event.
pub fn current_frame_completion_state(
) -> Option<Arc<Mutex<JetFrameCompletionState>>> {
    FRAME_COMPLETION_STATE.with(|active| active.borrow().as_ref().cloned())
}

/// Bind one pending submission to its unique checked event token. The returned
/// handle must travel with the submitted owner until the provider signals.
pub fn current_frame_completion_handle(
    provider: &str,
) -> Result<Option<JetFrameCompletionHandle>, String> {
    let Some(state) = current_frame_completion_state() else {
        return Ok(None);
    };
    bind_frame_completion_in(&state, provider).map(Some)
}

pub fn bind_frame_completion_in(
    state: &Arc<Mutex<JetFrameCompletionState>>,
    provider: &str,
) -> Result<JetFrameCompletionHandle, String> {
    let mut state_guard = state
        .lock()
        .map_err(|_| "checked frame completion state is poisoned".to_string())?;
    let Some(schedule) = state_guard.schedule.clone() else {
        return Err("completion state has no checked frame schedule".to_string());
    };
    let Some((token, event_provider)) = schedule.operations.iter().find_map(|operation| {
        let JetFrameCompletion::Pending { token } = &operation.completion else {
            return None;
        };
        if operation.completion_provider.as_deref() != Some(provider)
            || state_guard.bound_tokens.contains(token)
        {
            return None;
        }
        let event_provider = schedule.operations.iter().find_map(|event| {
            matches!(
                &event.completion,
                JetFrameCompletion::Event { token: event_token } if event_token == token
            )
            .then(|| event.completion_provider.as_deref())
            .flatten()
        })?;
        (event_provider == provider).then(|| (token.clone(), event_provider.to_string()))
    }) else {
        return Err(format!(
            "completion provider `{provider}` has no unbound checked submission"
        ));
    };
    state_guard.bound_tokens.insert(token.clone());
    let guards = state_guard
        .retention_guards
        .get(&token)
        .cloned()
        .unwrap_or_default();
    Ok(JetFrameCompletionHandle {
        state: Arc::clone(state),
        token,
        event_provider,
        generation: state_guard.generation,
        guards,
    })
}

/// Signal a checked completion from the provider that actually waited for or
/// observed it. Calls outside a checked callback fail closed. One call claims
/// one event; it never completes every event sharing a provider.
pub fn signal_frame_completion(provider: &str) -> Result<(), String> {
    let Some(state) = current_frame_completion_state() else {
        return Err(format!(
            "completion provider `{provider}` signalled without an active checked frame"
        ));
    };
    signal_frame_completion_in(&state, provider)
}

pub fn signal_frame_completion_in(
    state: &Arc<Mutex<JetFrameCompletionState>>,
    provider: &str,
) -> Result<(), String> {
    let mut state = state
        .lock()
        .map_err(|_| "checked frame completion state is poisoned".to_string())?;
    state.complete_from_provider(provider)
}

pub fn signal_frame_completion_binding_in(
    state: &Arc<Mutex<JetFrameCompletionState>>,
    provider: &str,
    token: &str,
    generation: u64,
) -> Result<(), String> {
    let mut state = state
        .lock()
        .map_err(|_| "checked frame completion state is poisoned".to_string())?;
    if state.generation != generation {
        return Err(format!(
            "completion token `{token}` belongs to stale frame generation {generation}"
        ));
    }
    let Some(schedule) = state.schedule.clone() else {
        return Err("completion state has no checked frame schedule".to_string());
    };
    let Some(event) = schedule.operations.iter().find(|operation| {
        matches!(
            &operation.completion,
            JetFrameCompletion::Event { token: event_token } if event_token == token
        )
    }) else {
        return Err(format!("completion token `{token}` is not in the checked schedule"));
    };
    if event.completion_provider.as_deref() != Some(provider) {
        return Err(format!(
            "completion token `{token}` belongs to provider `{}` not `{provider}`",
            event.completion_provider.as_deref().unwrap_or("<none>")
        ));
    }
    if !state.bound_tokens.contains(token) {
        return Err(format!(
            "completion token `{token}` was signalled without a checked submission"
        ));
    }
    state.complete(schedule.as_ref(), token)
}

thread_local! {
    static FRAME_COMPLETION_STATE: RefCell<Option<Arc<Mutex<JetFrameCompletionState>>>> =
        const { RefCell::new(None) };
}




impl JetFrameSchedule {
    /// Stable human-readable inspection text.  This is a projection of the
    /// adopted schedule facts; no renderer rebuilds a second dependency graph.
    pub fn explain(&self) -> String {
        let mut lines = vec!["Why did these operations run in order?".to_string()];
        for operation in &self.operations {
            let accesses = if operation.accesses.is_empty() {
                "no resource access".to_string()
            } else {
                operation
                    .accesses
                    .iter()
                    .map(|access| format!("{} {}", access.mode.as_str(), access.label()))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            lines.push(format!("  {} {}", operation.name, accesses));
        }
        for dependency in &self.dependencies {
            lines.push(format!(
                "  dependency: {} -> {} ({})",
                self.operations[dependency.before].name,
                self.operations[dependency.after].name,
                dependency.reason
            ));
        }
        for transfer in &self.transfers {
            lines.push(format!(
                "  transfer: {} {} -> {}",
                transfer.resource, transfer.from_device, transfer.to_device
            ));
        }
        for reuse in &self.reuse {
            let verb = if reuse.legal { "reuse" } else { "reuse refused" };
            lines.push(format!(
                "  {verb}: {} -> {} ({})",
                reuse.previous_resource, reuse.next_resource, reuse.reason
            ));
        }
        for retention in &self.retentions {
            let state = if retention.completed {
                "completed"
            } else {
                "still retained"
            };
            let token = retention.completion_token.as_deref().unwrap_or("return");
            lines.push(format!(
                "  retention: {} owner {} until {} ({state})",
                retention.resource, retention.owner, token
            ));
        }
        for reason in &self.conservative {
            lines.push(format!("  conservative: {reason}"));
        }
        if self.dependencies.is_empty() {
            lines.push("  no conflicting resource accesses".to_string());
        }
        lines.join("\n")
    }
    /// Serialize the complete checked schedule into one deterministic wire
    /// record.  This is the canonical payload stored beside its derivation;
    /// `explain()` remains a human projection and is intentionally not used
    /// for persistence.
    pub fn canonical_json(&self) -> String {
        let operations = self
            .operations
            .iter()
            .map(encode_operation)
            .collect::<Vec<_>>()
            .join(",");
        let dependencies = self
            .dependencies
            .iter()
            .map(encode_dependency)
            .collect::<Vec<_>>()
            .join(",");
        let transfers = self
            .transfers
            .iter()
            .map(encode_transfer)
            .collect::<Vec<_>>()
            .join(",");
        let reuse = self
            .reuse
            .iter()
            .map(encode_reuse)
            .collect::<Vec<_>>()
            .join(",");
        let retentions = self
            .retentions
            .iter()
            .map(encode_retention)
            .collect::<Vec<_>>()
            .join(",");
        let conservative = self
            .conservative
            .iter()
            .map(|reason| json_string(reason))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema\":\"jet.frame_schedule/v1\",\"operations\":[{operations}],\"dependencies\":[{dependencies}],\"transfers\":[{transfers}],\"reuse\":[{reuse}],\"retentions\":[{retentions}],\"conservative\":[{conservative}]}}"
        )
    }

    /// Decode one complete canonical schedule payload without deriving a new
    /// graph.  The decoder preserves every checked identity and decision,
    /// including conservative/refused reuse rows.
    pub fn from_canonical_json(input: &str) -> Result<Self, String> {
        let value = crate::EncodingJson::parse_json(input, true)
            .map_err(|error| format!("invalid frame schedule JSON: {}", error.message))?;
        let root = json_object(&value, "frame schedule")?;
        let schema = json_text(json_field(root, "schema")?, "schema")?;
        if schema != "jet.frame_schedule/v1" {
            return Err(format!("unsupported frame schedule schema `{schema}`"));
        }
        let operations = json_array(json_field(root, "operations")?, "operations")?
            .iter()
            .map(parse_operation)
            .collect::<Result<Vec<_>, _>>()?;
        let dependencies = json_array(json_field(root, "dependencies")?, "dependencies")?
            .iter()
            .map(parse_dependency)
            .collect::<Result<Vec<_>, _>>()?;
        let transfers = json_array(json_field(root, "transfers")?, "transfers")?
            .iter()
            .map(parse_transfer)
            .collect::<Result<Vec<_>, _>>()?;
        let reuse = json_array(json_field(root, "reuse")?, "reuse")?
            .iter()
            .map(parse_reuse)
            .collect::<Result<Vec<_>, _>>()?;
        let retentions = json_array(json_field(root, "retentions")?, "retentions")?
            .iter()
            .map(parse_retention)
            .collect::<Result<Vec<_>, _>>()?;
        let conservative = json_array(json_field(root, "conservative")?, "conservative")?
            .iter()
            .map(|value| json_text(value, "conservative reason"))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            operations,
            dependencies,
            transfers,
            reuse,
            retentions,
            conservative,
        })
    }

    /// Byte-oriented alias for persistence layers that already carry opaque
    /// records.  The bytes are UTF-8 canonical JSON, not an explanation.
    pub fn encode_canonical(&self) -> Vec<u8> {
        self.canonical_json().into_bytes()
    }

    pub fn decode_canonical(bytes: &[u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(bytes)
            .map_err(|error| format!("frame schedule payload is not UTF-8: {error}"))?;
        Self::from_canonical_json(text)
    }

}

fn json_string(value: &str) -> String {
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
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", character as u32);
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

fn json_number(value: u64) -> String {
    json_string(&value.to_string())
}

fn encode_identity(identity: &JetResourceIdentity) -> String {
    match identity {
        JetResourceIdentity::Named(resource) => format!(
            "{{\"kind\":\"named\",\"value\":{}}}",
            json_string(resource)
        ),
        JetResourceIdentity::Unknown => "{\"kind\":\"unknown\"}".to_string(),
    }
}

fn encode_region(region: &JetResourceRegion) -> String {
    match region {
        JetResourceRegion::Whole => "{\"kind\":\"whole\"}".to_string(),
        JetResourceRegion::Unknown => "{\"kind\":\"unknown\"}".to_string(),
        JetResourceRegion::Bytes { start, length } => format!(
            "{{\"kind\":\"bytes\",\"start\":{},\"length\":{}}}",
            json_number(*start),
            json_number(*length)
        ),
    }
}

fn encode_layout(layout: &Option<JetResourceLayout>) -> String {
    match layout {
        None => "null".to_string(),
        Some(layout) => format!(
            "{{\"size\":{},\"align\":{},\"layout\":{},\"device\":{}}}",
            json_number(layout.size),
            json_number(layout.align),
            json_string(&layout.layout),
            json_string(&layout.device)
        ),
    }
}

fn encode_access(access: &JetResourceAccess) -> String {
    format!(
        "{{\"resource\":{},\"mode\":{},\"region\":{},\"layout\":{},\"temporary\":{}}}",
        encode_identity(&access.resource),
        json_string(access.mode.as_str()),
        encode_region(&access.region),
        encode_layout(&access.layout),
        access.temporary
    )
}

fn encode_completion(completion: &JetFrameCompletion) -> String {
    match completion {
        JetFrameCompletion::Synchronous => "{\"kind\":\"synchronous\"}".to_string(),
        JetFrameCompletion::Pending { token } => format!(
            "{{\"kind\":\"pending\",\"token\":{}}}",
            json_string(token)
        ),
        JetFrameCompletion::Event { token } => format!(
            "{{\"kind\":\"event\",\"token\":{}}}",
            json_string(token)
        ),
    }
}

fn encode_operation(operation: &JetFrameOperation) -> String {
    let accesses = operation
        .accesses
        .iter()
        .map(encode_access)
        .collect::<Vec<_>>()
        .join(",");
    let provider = operation
        .completion_provider
        .as_ref()
        .map(|provider| format!(",\"completion_provider\":{}", json_string(provider)))
        .unwrap_or_default();
    format!(
        "{{\"name\":{},\"source_index\":{},\"accesses\":[{}],\"completion\":{},\"external_effect\":{}{}}}",
        json_string(&operation.name),
        json_number(operation.source_index as u64),
        accesses,
        encode_completion(&operation.completion),
        operation.external_effect,
        provider
    )
}

fn encode_dependency(dependency: &JetFrameDependency) -> String {
    format!(
        "{{\"before\":{},\"after\":{},\"resource\":{},\"reason\":{}}}",
        json_number(dependency.before as u64),
        json_number(dependency.after as u64),
        json_string(&dependency.resource),
        json_string(&dependency.reason)
    )
}

fn encode_transfer(transfer: &JetFrameTransfer) -> String {
    format!(
        "{{\"before\":{},\"after\":{},\"resource\":{},\"from_device\":{},\"to_device\":{}}}",
        json_number(transfer.before as u64),
        json_number(transfer.after as u64),
        json_string(&transfer.resource),
        json_string(&transfer.from_device),
        json_string(&transfer.to_device)
    )
}

fn encode_reuse(reuse: &JetFrameReuse) -> String {
    format!(
        "{{\"previous_resource\":{},\"next_resource\":{},\"previous_last_use\":{},\"next_first_use\":{},\"legal\":{},\"reason\":{}}}",
        json_string(&reuse.previous_resource),
        json_string(&reuse.next_resource),
        json_number(reuse.previous_last_use as u64),
        json_number(reuse.next_first_use as u64),
        reuse.legal,
        json_string(&reuse.reason)
    )
}

fn encode_retention(retention: &JetFrameRetention) -> String {
    let completion = retention
        .completion_token
        .as_deref()
        .map(json_string)
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"resource\":{},\"operation\":{},\"owner\":{},\"completion_token\":{},\"completed\":{}}}",
        json_string(&retention.resource),
        json_number(retention.operation as u64),
        json_string(&retention.owner),
        completion,
        retention.completed
    )
}

fn json_object<'a>(
    value: &'a crate::DataTree::DataTree,
    what: &str,
) -> Result<&'a [(String, crate::DataTree::DataTree)], String> {
    value
        .as_object()
        .map(Vec::as_slice)
        .map_err(|_| format!("expected {what} object"))
}

fn json_array<'a>(
    value: &'a crate::DataTree::DataTree,
    what: &str,
) -> Result<&'a [crate::DataTree::DataTree], String> {
    value
        .as_array()
        .map(Vec::as_slice)
        .map_err(|_| format!("expected {what} array"))
}

fn json_field<'a>(
    object: &'a [(String, crate::DataTree::DataTree)],
    name: &str,
) -> Result<&'a crate::DataTree::DataTree, String> {
    object
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value))
        .ok_or_else(|| format!("frame schedule is missing `{name}`"))
}

fn json_text(value: &crate::DataTree::DataTree, what: &str) -> Result<String, String> {
    value
        .as_str()
        .map(str::to_string)
        .map_err(|_| format!("expected {what} text"))
}

fn json_bool(value: &crate::DataTree::DataTree, what: &str) -> Result<bool, String> {
    match value {
        crate::DataTree::DataTree::Bool(value) => Ok(*value),
        _ => Err(format!("expected {what} boolean")),
    }
}

fn json_u64(value: &crate::DataTree::DataTree, what: &str) -> Result<u64, String> {
    let text = match value {
        crate::DataTree::DataTree::Int(value) if *value >= 0 => value.to_string(),
        crate::DataTree::DataTree::Number(value)
        | crate::DataTree::DataTree::Text(value)
        | crate::DataTree::DataTree::TypedText(value) => value.clone(),
        _ => return Err(format!("expected {what} unsigned integer")),
    };
    text.parse::<u64>()
        .map_err(|_| format!("invalid {what} unsigned integer `{text}`"))
}

fn json_usize(value: &crate::DataTree::DataTree, what: &str) -> Result<usize, String> {
    let value = json_u64(value, what)?;
    usize::try_from(value).map_err(|_| format!("{what} does not fit in usize"))
}

fn json_optional_text(
    value: &crate::DataTree::DataTree,
    what: &str,
) -> Result<Option<String>, String> {
    match value {
        crate::DataTree::DataTree::Null => Ok(None),
        _ => json_text(value, what).map(Some),
    }
}

fn parse_identity(value: &crate::DataTree::DataTree) -> Result<JetResourceIdentity, String> {
    let object = json_object(value, "resource identity")?;
    match json_text(json_field(object, "kind")?, "resource identity kind")?.as_str() {
        "named" => Ok(JetResourceIdentity::Named(json_text(
            json_field(object, "value")?,
            "resource identity value",
        )?)),
        "unknown" => Ok(JetResourceIdentity::Unknown),
        kind => Err(format!("unsupported resource identity kind `{kind}`")),
    }
}

fn parse_mode(value: &crate::DataTree::DataTree) -> Result<JetResourceAccessMode, String> {
    match json_text(value, "resource access mode")?.as_str() {
        "read" => Ok(JetResourceAccessMode::Read),
        "write" => Ok(JetResourceAccessMode::Write),
        "move" => Ok(JetResourceAccessMode::Move),
        mode => Err(format!("unsupported resource access mode `{mode}`")),
    }
}

fn parse_region(value: &crate::DataTree::DataTree) -> Result<JetResourceRegion, String> {
    let object = json_object(value, "resource region")?;
    match json_text(json_field(object, "kind")?, "resource region kind")?.as_str() {
        "whole" => Ok(JetResourceRegion::Whole),
        "unknown" => Ok(JetResourceRegion::Unknown),
        "bytes" => Ok(JetResourceRegion::Bytes {
            start: json_u64(json_field(object, "start")?, "region start")?,
            length: json_u64(json_field(object, "length")?, "region length")?,
        }),
        kind => Err(format!("unsupported resource region kind `{kind}`")),
    }
}

fn parse_layout(
    value: &crate::DataTree::DataTree,
) -> Result<Option<JetResourceLayout>, String> {
    if matches!(value, crate::DataTree::DataTree::Null) {
        return Ok(None);
    }
    let object = json_object(value, "resource layout")?;
    Ok(Some(JetResourceLayout {
        size: json_u64(json_field(object, "size")?, "layout size")?,
        align: json_u64(json_field(object, "align")?, "layout alignment")?,
        layout: json_text(json_field(object, "layout")?, "layout identity")?,
        device: json_text(json_field(object, "device")?, "layout device")?,
    }))
}

fn parse_access(value: &crate::DataTree::DataTree) -> Result<JetResourceAccess, String> {
    let object = json_object(value, "resource access")?;
    Ok(JetResourceAccess {
        resource: parse_identity(json_field(object, "resource")?)?,
        mode: parse_mode(json_field(object, "mode")?)?,
        region: parse_region(json_field(object, "region")?)?,
        layout: parse_layout(json_field(object, "layout")?)?,
        temporary: json_bool(json_field(object, "temporary")?, "temporary")?,
    })
}

fn parse_completion(
    value: &crate::DataTree::DataTree,
) -> Result<JetFrameCompletion, String> {
    let object = json_object(value, "completion")?;
    let kind = json_text(json_field(object, "kind")?, "completion kind")?;
    match kind.as_str() {
        "synchronous" => Ok(JetFrameCompletion::Synchronous),
        "pending" => Ok(JetFrameCompletion::Pending {
            token: json_text(json_field(object, "token")?, "completion token")?,
        }),
        "event" => Ok(JetFrameCompletion::Event {
            token: json_text(json_field(object, "token")?, "completion token")?,
        }),
        _ => Err(format!("unsupported completion kind `{kind}`")),
    }
}

fn parse_operation(value: &crate::DataTree::DataTree) -> Result<JetFrameOperation, String> {
    let object = json_object(value, "frame operation")?;
    let accesses = json_array(json_field(object, "accesses")?, "operation accesses")?
        .iter()
        .map(parse_access)
        .collect::<Result<Vec<_>, _>>()?;
    let completion_provider = object
        .iter()
        .find_map(|(key, value)| (key == "completion_provider").then_some(value))
        .map(|value| json_optional_text(value, "completion provider"))
        .transpose()?
        .flatten();
    Ok(JetFrameOperation {
        name: json_text(json_field(object, "name")?, "operation name")?,
        source_index: json_usize(json_field(object, "source_index")?, "source index")?,
        accesses,
        completion: parse_completion(json_field(object, "completion")?)?,
        completion_provider,
        external_effect: json_bool(
            json_field(object, "external_effect")?,
            "external effect",
        )?,
    })
}

fn parse_dependency(value: &crate::DataTree::DataTree) -> Result<JetFrameDependency, String> {
    let object = json_object(value, "frame dependency")?;
    Ok(JetFrameDependency {
        before: json_usize(json_field(object, "before")?, "dependency before")?,
        after: json_usize(json_field(object, "after")?, "dependency after")?,
        resource: json_text(json_field(object, "resource")?, "dependency resource")?,
        reason: json_text(json_field(object, "reason")?, "dependency reason")?,
    })
}

fn parse_transfer(value: &crate::DataTree::DataTree) -> Result<JetFrameTransfer, String> {
    let object = json_object(value, "frame transfer")?;
    Ok(JetFrameTransfer {
        before: json_usize(json_field(object, "before")?, "transfer before")?,
        after: json_usize(json_field(object, "after")?, "transfer after")?,
        resource: json_text(json_field(object, "resource")?, "transfer resource")?,
        from_device: json_text(json_field(object, "from_device")?, "source device")?,
        to_device: json_text(json_field(object, "to_device")?, "destination device")?,
    })
}

fn parse_reuse(value: &crate::DataTree::DataTree) -> Result<JetFrameReuse, String> {
    let object = json_object(value, "frame reuse")?;
    Ok(JetFrameReuse {
        previous_resource: json_text(
            json_field(object, "previous_resource")?,
            "previous resource",
        )?,
        next_resource: json_text(json_field(object, "next_resource")?, "next resource")?,
        previous_last_use: json_usize(
            json_field(object, "previous_last_use")?,
            "previous last use",
        )?,
        next_first_use: json_usize(json_field(object, "next_first_use")?, "next first use")?,
        legal: json_bool(json_field(object, "legal")?, "reuse legality")?,
        reason: json_text(json_field(object, "reason")?, "reuse reason")?,
    })
}

fn parse_retention(value: &crate::DataTree::DataTree) -> Result<JetFrameRetention, String> {
    let object = json_object(value, "frame retention")?;
    Ok(JetFrameRetention {
        resource: json_text(json_field(object, "resource")?, "retained resource")?,
        operation: json_usize(json_field(object, "operation")?, "retention operation")?,
        owner: json_text(json_field(object, "owner")?, "retention owner")?,
        completion_token: json_optional_text(
            json_field(object, "completion_token")?,
            "retention completion token",
        )?,
        completed: json_bool(json_field(object, "completed")?, "retention completion")?,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetFrameScheduleError {
    InvalidOperation {
        index: usize,
        reason: String,
    },
    InvalidAccess {
        operation: String,
        resource: String,
        reason: String,
    },
    NonMonotonicSourceOrder {
        index: usize,
        previous: usize,
        current: usize,
    },
    ConflictingAccess {
        before: String,
        after: String,
        resource: String,
        before_mode: JetResourceAccessMode,
        after_mode: JetResourceAccessMode,
    },
    UnknownAlias {
        before: String,
        after: String,
    },
    MissingCompletion {
        operation: String,
        resource: String,
        token: String,
    },
}

impl fmt::Display for JetFrameScheduleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOperation { index, reason } => {
                write!(formatter, "operation {index} is invalid: {reason}")
            }
            Self::InvalidAccess {
                operation,
                resource,
                reason,
            } => write!(
                formatter,
                "operation `{operation}` has invalid access to `{resource}`: {reason}"
            ),
            Self::NonMonotonicSourceOrder {
                index,
                previous,
                current,
            } => write!(
                formatter,
                "operation {index} source index {current} follows {previous} out of order"
            ),
            Self::ConflictingAccess {
                before,
                after,
                resource,
                before_mode,
                after_mode,
            } => write!(
                formatter,
                "parallel frame refused: `{before}` {} `{resource}` before `{after}` {} it",
                before_mode.as_str(),
                after_mode.as_str()
            ),
            Self::UnknownAlias { before, after } => write!(
                formatter,
                "parallel frame refused: `{before}` and `{after}` have unresolved aliasing"
            ),
            Self::MissingCompletion {
                operation,
                resource,
                token,
            } => write!(
                formatter,
                "operation `{operation}` accesses `{resource}` before completion event `{token}`"
            ),
        }
    }
}

/// Derive the conservative source-order reference schedule.
pub fn derive_frame_schedule(
    operations: impl AsRef<[JetFrameOperation]>,
) -> Result<JetFrameSchedule, JetFrameScheduleError> {
    derive_frame_schedule_with_options(operations, JetFrameScheduleOptions::serial())
}

/// Derive a schedule with an explicit parallel demand.  The normal frame path
/// calls [`derive_frame_schedule`]; this option is for a checked optimizer or
/// an expert placement request and refuses unresolved hazards.
pub fn derive_frame_schedule_with_options(
    operations: impl AsRef<[JetFrameOperation]>,
    options: JetFrameScheduleOptions,
) -> Result<JetFrameSchedule, JetFrameScheduleError> {
    let operations = operations.as_ref().to_vec();
    validate_operations(&operations)?;

    let completion_events = completion_events(&operations)?;
    let mut dependencies = Vec::new();
    let mut dependency_keys = BTreeSet::new();
    let mut transfers = Vec::new();
    let mut transfer_keys = BTreeSet::new();
    let mut conservative = Vec::new();

    for (left_index, left) in operations.iter().enumerate() {
        for (right_index, right) in operations.iter().enumerate().skip(left_index + 1) {

            for left_access in &left.accesses {
                for right_access in &right.accesses {
                    if !left_access
                        .resource
                        .overlaps(&right_access.resource)
                        || !left_access.region.overlaps(&right_access.region)
                    {
                        continue;
                    }

                    let unknown_alias = matches!(
                        (&left_access.resource, &right_access.resource),
                        (JetResourceIdentity::Unknown, _) | (_, JetResourceIdentity::Unknown)
                    );
                    let conflict = left_access.mode.is_write() || right_access.mode.is_write();
                    if unknown_alias {
                        if options.permit_parallel && conflict {
                            return Err(JetFrameScheduleError::UnknownAlias {
                                before: left.name.clone(),
                                after: right.name.clone(),
                            });
                        }
                        conservative.push(format!(
                            "{} and {} keep source order because aliasing is unknown",
                            left.name, right.name
                        ));
                        if !conflict {
                            push_dependency(
                                &mut dependencies,
                                &mut dependency_keys,
                                left_index,
                                right_index,
                                "unknown resource".to_string(),
                                "unknown aliasing".to_string(),
                            );
                        }
                    }
                    if conflict {
                        if options.permit_parallel {
                            return Err(JetFrameScheduleError::ConflictingAccess {
                                before: left.name.clone(),
                                after: right.name.clone(),
                                resource: left_access.label().to_string(),
                                before_mode: left_access.mode,
                                after_mode: right_access.mode,
                            });
                        }
                        let resource = if unknown_alias {
                            "unknown resource".to_string()
                        } else {
                            left_access.label().to_string()
                        };
                        push_dependency(
                            &mut dependencies,
                            &mut dependency_keys,
                            left_index,
                            right_index,
                            resource,
                            format!(
                                "{} {} -> {} {}",
                                left.name,
                                left_access.mode.as_str(),
                                right.name,
                                right_access.mode.as_str()
                            ),
                        );
                    }

                    if let (
                        JetResourceIdentity::Named(resource),
                        Some(left_layout),
                        Some(right_layout),
                    ) = (&left_access.resource, &left_access.layout, &right_access.layout)
                    {
                        if !left_layout.device.is_empty()
                            && !right_layout.device.is_empty()
                            && left_layout.device != right_layout.device
                            && transfer_keys.insert((left_index, right_index, resource.clone()))
                        {
                            transfers.push(JetFrameTransfer {
                                before: left_index,
                                after: right_index,
                                resource: resource.clone(),
                                from_device: left_layout.device.clone(),
                                to_device: right_layout.device.clone(),
                            });
                        }
                    }
                }
            }
            if left.external_effect || right.external_effect {
                push_dependency(
                    &mut dependencies,
                    &mut dependency_keys,
                    left_index,
                    right_index,
                    "external effect".to_string(),
                    "visible external effects preserve source order".to_string(),
                );
            }
        }
    }

    let retentions = derive_retentions(&operations, &completion_events);
    ensure_async_uses_complete(
        &operations,
        &completion_events,
        &mut dependencies,
        &mut dependency_keys,
    )?;
    let reuse = derive_reuse(&operations, &completion_events, &mut conservative);

    dependencies.sort_by_key(|dependency| {
        (
            dependency.before,
            dependency.after,
            dependency.resource.clone(),
            dependency.reason.clone(),
        )
    });
    transfers.sort_by_key(|transfer| {
        (
            transfer.before,
            transfer.after,
            transfer.resource.clone(),
            transfer.from_device.clone(),
            transfer.to_device.clone(),
        )
    });
    conservative.sort();
    conservative.dedup();

    Ok(JetFrameSchedule {
        operations,
        dependencies,
        transfers,
        reuse,
        retentions,
        conservative,
    })
}

fn validate_operations(operations: &[JetFrameOperation]) -> Result<(), JetFrameScheduleError> {
    let mut previous_source = None;
    for (index, operation) in operations.iter().enumerate() {
        if operation.name.trim().is_empty() {
            return Err(JetFrameScheduleError::InvalidOperation {
                index,
                reason: "checked operation name must not be empty".to_string(),
            });
        }
        if let Some(previous) = previous_source {
            if operation.source_index < previous {
                return Err(JetFrameScheduleError::NonMonotonicSourceOrder {
                    index,
                    previous,
                    current: operation.source_index,
                });
            }
        }
        previous_source = Some(operation.source_index);
        match &operation.completion {
            JetFrameCompletion::Pending { token } | JetFrameCompletion::Event { token }
                if token.trim().is_empty() =>
            {
                return Err(JetFrameScheduleError::InvalidOperation {
                    index,
                    reason: "completion token must not be empty".to_string(),
                });
            }
            _ => {}
        }
        for access in &operation.accesses {
            if let JetResourceIdentity::Named(resource) = &access.resource {
                if resource.trim().is_empty() {
                    return Err(JetFrameScheduleError::InvalidAccess {
                        operation: operation.name.clone(),
                        resource: resource.clone(),
                        reason: "resource identity must not be empty".to_string(),
                    });
                }
            }
            access
                .region
                .validate(&operation.name, access.label())?;
            if let Some(layout) = &access.layout {
                layout.validate(&operation.name, access.label())?;
            }
        }
    }
    Ok(())
}

fn completion_events(
    operations: &[JetFrameOperation],
) -> Result<BTreeMap<String, usize>, JetFrameScheduleError> {
    let mut events = BTreeMap::new();
    for (index, operation) in operations.iter().enumerate() {
        if let JetFrameCompletion::Event { token } = &operation.completion {
            if let Some(previous) = events.insert(token.clone(), index) {
                return Err(JetFrameScheduleError::InvalidOperation {
                    index,
                    reason: format!(
                        "completion token `{token}` has duplicate events at {previous} and {index}"
                    ),
                });
            }
        }
    }
    for (index, operation) in operations.iter().enumerate() {
        let JetFrameCompletion::Pending { token } = &operation.completion else {
            continue;
        };
        let Some(&event_index) = events.get(token) else {
            let resource = operation
                .accesses
                .first()
                .map(JetResourceAccess::label)
                .unwrap_or("unknown resource");
            return Err(JetFrameScheduleError::MissingCompletion {
                operation: operation.name.clone(),
                resource: resource.to_string(),
                token: token.clone(),
            });
        };
        if event_index <= index {
            let resource = operation
                .accesses
                .first()
                .map(JetResourceAccess::label)
                .unwrap_or("unknown resource");
            return Err(JetFrameScheduleError::MissingCompletion {
                operation: operation.name.clone(),
                resource: resource.to_string(),
                token: token.clone(),
            });
        }
    }
    Ok(events)
}

fn push_dependency(
    dependencies: &mut Vec<JetFrameDependency>,
    keys: &mut BTreeSet<(usize, usize, String)>,
    before: usize,
    after: usize,
    resource: String,
    reason: String,
) {
    if keys.insert((before, after, resource.clone())) {
        dependencies.push(JetFrameDependency {
            before,
            after,
            resource,
            reason,
        });
    }
}

fn ensure_async_uses_complete(
    operations: &[JetFrameOperation],
    events: &BTreeMap<String, usize>,
    dependencies: &mut Vec<JetFrameDependency>,
    keys: &mut BTreeSet<(usize, usize, String)>,
) -> Result<(), JetFrameScheduleError> {
    for (index, operation) in operations.iter().enumerate() {
        let JetFrameCompletion::Pending { token } = &operation.completion else {
            continue;
        };
        let Some(&event_index) = events.get(token) else {
            let resource = operation
                .accesses
                .first()
                .map(JetResourceAccess::label)
                .unwrap_or("unknown resource");
            return Err(JetFrameScheduleError::MissingCompletion {
                operation: operation.name.clone(),
                resource: resource.to_string(),
                token: token.clone(),
            });
        };
        if event_index <= index {
            continue;
        }
        push_dependency(
            dependencies,
            keys,
            index,
            event_index,
            "completion".to_string(),
            format!("{} completes at event {token}", operation.name),
        );
        for later in operations.iter().enumerate().skip(event_index + 1) {
            let (later_index, later_operation) = later;
            for access in &later_operation.accesses {
                if operation.accesses.iter().any(|submitted| {
                    submitted.resource.overlaps(&access.resource)
                        && submitted.region.overlaps(&access.region)
                }) {
                    push_dependency(
                        dependencies,
                        keys,
                        event_index,
                        later_index,
                        "completion".to_string(),
                        format!("event {token} releases submitted owners"),
                    );
                }
            }
        }
    }

    for (index, operation) in operations.iter().enumerate() {
        for access in &operation.accesses {
            for (pending_index, pending) in operations.iter().enumerate().take(index) {
                let JetFrameCompletion::Pending { token } = &pending.completion else {
                    continue;
                };
                let overlaps = pending.accesses.iter().any(|submitted| {
                    submitted.resource.overlaps(&access.resource)
                        && submitted.region.overlaps(&access.region)
                });
                if !overlaps {
                    continue;
                }
                match events.get(token).copied() {
                    Some(event_index) if event_index < index => {}
                    _ => {
                        return Err(JetFrameScheduleError::MissingCompletion {
                            operation: operation.name.clone(),
                            resource: access.label().to_string(),
                            token: token.clone(),
                        });
                    }
                }
                let _ = pending_index;
            }
        }
    }
    Ok(())
}

fn derive_retentions(
    operations: &[JetFrameOperation],
    events: &BTreeMap<String, usize>,
) -> Vec<JetFrameRetention> {
    let mut retentions = Vec::new();
    for (index, operation) in operations.iter().enumerate() {
        let JetFrameCompletion::Pending { token } = &operation.completion else {
            continue;
        };
        let completed = events.get(token).is_some_and(|event| *event > index);
        for access in &operation.accesses {
            retentions.push(JetFrameRetention {
                resource: access.label().to_string(),
                operation: index,
                owner: operation.name.clone(),
                completion_token: Some(token.clone()),
                completed,
            });
        }
    }
    retentions
}

#[derive(Clone)]
struct ResourceUse {
    resource: String,
    first: usize,
    last: usize,
    temporary: bool,
    layouts: Vec<Option<JetResourceLayout>>,
}

fn derive_reuse(
    operations: &[JetFrameOperation],
    events: &BTreeMap<String, usize>,
    conservative: &mut Vec<String>,
) -> Vec<JetFrameReuse> {
    let mut uses = BTreeMap::<String, ResourceUse>::new();
    for (index, operation) in operations.iter().enumerate() {
        for access in &operation.accesses {
            let JetResourceIdentity::Named(resource) = &access.resource else {
                continue;
            };
            let entry = uses.entry(resource.clone()).or_insert_with(|| ResourceUse {
                resource: resource.clone(),
                first: index,
                last: index,
                temporary: access.temporary,
                layouts: Vec::new(),
            });
            entry.first = entry.first.min(index);
            entry.last = entry.last.max(index);
            // Reuse is legal only when every checked use permits temporary
            // storage.  A single persistent use keeps the resource alive.
            entry.temporary &= access.temporary;
            entry.layouts.push(access.layout.clone());
        }
    }

    let candidates = uses
        .into_values()
        .filter(|usage| usage.temporary)
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    for right in &candidates {
        let Some(left) = candidates
            .iter()
            .filter(|left| left.resource != right.resource && left.last < right.first)
            .max_by_key(|left| left.last)
        else {
            continue;
        };
        let reason = matching_layout_reason(left, right);
        let mut legal = reason.is_none();
        let mut reason_text = reason.unwrap_or_else(|| "lifetimes do not overlap".to_string());
        if legal {
            let pending_index = operations
                .iter()
                .enumerate()
                .filter(|(_, operation)| {
                    matches!(&operation.completion, JetFrameCompletion::Pending { .. })
                        && operation.accesses.iter().any(|access| {
                            matches!(&access.resource, JetResourceIdentity::Named(resource) if resource == &left.resource)
                        })
                })
                .filter(|(index, _)| *index <= left.last)
                .max_by_key(|(index, _)| *index)
                .map(|(index, _)| index);
            if let Some(pending_index) = pending_index {
                if let JetFrameCompletion::Pending { token } = &operations[pending_index].completion {
                    let completed_at = events.get(token).copied();
                    if completed_at.is_none_or(|event| event >= right.first) {
                        legal = false;
                        reason_text = format!("completion event {token} has not occurred");
                    }
                }
            }
        }
        if !legal {
            conservative.push(format!(
                "{} was not reused for {}: {}",
                left.resource, right.resource, reason_text
            ));
        }
        result.push(JetFrameReuse {
            previous_resource: left.resource.clone(),
            next_resource: right.resource.clone(),
            previous_last_use: left.last,
            next_first_use: right.first,
            legal,
            reason: reason_text,
        });
    }
    result.sort_by_key(|reuse| {
        (
            reuse.previous_last_use,
            reuse.next_first_use,
            reuse.previous_resource.clone(),
            reuse.next_resource.clone(),
        )
    });
    result
}

fn matching_layout_reason(left: &ResourceUse, right: &ResourceUse) -> Option<String> {
    let Some(left_layout) = left.layouts.iter().flatten().next() else {
        return Some("layout/size/alignment/device facts are incomplete".to_string());
    };
    let Some(right_layout) = right.layouts.iter().flatten().next() else {
        return Some("layout/size/alignment/device facts are incomplete".to_string());
    };
    if left
        .layouts
        .iter()
        .any(|layout| layout.as_ref() != Some(left_layout))
        || right
            .layouts
            .iter()
            .any(|layout| layout.as_ref() != Some(right_layout))
    {
        return Some("every temporary use needs complete matching layout facts".to_string());
    }
    if left_layout.device.is_empty() || right_layout.device.is_empty() {
        return Some("placement/device facts are incomplete".to_string());
    }
    if left_layout != right_layout {
        return Some("size, alignment, layout, or device differs".to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(name: &str, index: usize, resource: &str, mode: JetResourceAccessMode) -> JetFrameOperation {
        JetFrameOperation::new(name, index).with_access(JetResourceAccess::named(resource, mode))
    }

    fn temp(name: &str, index: usize, resource: &str) -> JetFrameOperation {
        JetFrameOperation::new(name, index).with_access(
            JetResourceAccess::named(resource, JetResourceAccessMode::Write)
                .with_layout(JetResourceLayout::new(64, 16, "rgba8", "gpu0"))
                .temporary(),
        )
    }

    #[test]
    fn serial_plan_records_read_write_dependency_and_explanation() {
        let schedule = derive_frame_schedule([
            op("simulate", 0, "positions", JetResourceAccessMode::Write),
            op("draw", 1, "positions", JetResourceAccessMode::Read),
        ])
        .expect("serial schedule");
        assert_eq!(schedule.dependencies.len(), 1);
        assert!(schedule.explain().contains("dependency: simulate -> draw"));
    }

    #[test]
    fn read_read_is_legal_without_dependency() {
        let schedule = derive_frame_schedule([
            op("a", 0, "image", JetResourceAccessMode::Read),
            op("b", 1, "image", JetResourceAccessMode::Read),
        ])
        .expect("read/read schedule");
        assert!(schedule.dependencies.is_empty());
    }

    #[test]
    fn overlapping_regions_preserve_source_order() {
        let left = JetResourceAccess::named("image", JetResourceAccessMode::Write)
            .with_region(JetResourceRegion::bytes(0, 8));
        let right = JetResourceAccess::named("image", JetResourceAccessMode::Read)
            .with_region(JetResourceRegion::bytes(4, 4));
        let schedule = derive_frame_schedule([
            JetFrameOperation::new("write", 0).with_access(left),
            JetFrameOperation::new("read", 1).with_access(right),
        ])
        .expect("overlapping regions need a serial dependency");
        assert_eq!(schedule.dependencies.len(), 1);
        assert_eq!(schedule.dependencies[0].resource, "image");
    }

    #[test]
    fn unknown_alias_refuses_parallel_write() {
        let result = derive_frame_schedule_with_options(
            [
                JetFrameOperation::new("a", 0)
                    .with_access(JetResourceAccess::unknown(JetResourceAccessMode::Write)),
                op("b", 1, "image", JetResourceAccessMode::Read),
            ],
            JetFrameScheduleOptions::parallel(),
        );
        assert!(matches!(result, Err(JetFrameScheduleError::UnknownAlias { .. })));
    }

    #[test]
    fn overlapping_writes_refuse_explicit_parallel_demand() {
        let result = derive_frame_schedule_with_options(
            [
                op("a", 0, "image", JetResourceAccessMode::Write),
                op("b", 1, "image", JetResourceAccessMode::Write),
            ],
            JetFrameScheduleOptions::parallel(),
        );
        assert!(matches!(result, Err(JetFrameScheduleError::ConflictingAccess { .. })));
    }

    #[test]
    fn disjoint_regions_can_run_in_parallel() {
        let left = JetResourceAccess::named("image", JetResourceAccessMode::Write)
            .with_region(JetResourceRegion::bytes(0, 4));
        let right = JetResourceAccess::named("image", JetResourceAccessMode::Write)
            .with_region(JetResourceRegion::bytes(4, 4));
        let schedule = derive_frame_schedule_with_options(
            [
                JetFrameOperation::new("a", 0).with_access(left),
                JetFrameOperation::new("b", 1).with_access(right),
            ],
            JetFrameScheduleOptions::parallel(),
        )
        .expect("disjoint schedule");
        assert!(schedule.dependencies.is_empty());
    }

    #[test]
    fn async_owner_is_retained_until_actual_event() {
        let schedule = derive_frame_schedule([
            JetFrameOperation::new("submit", 0)
                .with_access(JetResourceAccess::named("image", JetResourceAccessMode::Read))
                .pending("flight"),
            JetFrameOperation::new("finish", 1).completion_event("flight"),
            op("consume", 2, "image", JetResourceAccessMode::Read),
        ])
        .expect("completion schedule");
        assert_eq!(schedule.retentions[0].completion_token.as_deref(), Some("flight"));
        assert!(schedule
            .dependencies
            .iter()
            .any(|dependency| dependency.resource == "completion"));
    }

    #[test]
    fn completion_event_before_submit_is_refused() {
        let result = derive_frame_schedule([
            JetFrameOperation::new("finish", 0).completion_event("flight"),
            JetFrameOperation::new("submit", 1)
                .with_access(JetResourceAccess::named("image", JetResourceAccessMode::Read))
                .pending("flight"),
        ]);
        assert!(matches!(
            result,
            Err(JetFrameScheduleError::MissingCompletion { .. })
        ));
    }

    #[test]
    fn missing_async_completion_is_refused() {
        let result = derive_frame_schedule([
            JetFrameOperation::new("submit", 0)
                .with_access(
                    JetResourceAccess::named("a", JetResourceAccessMode::Write)
                        .with_layout(JetResourceLayout::new(64, 16, "rgba8", "gpu0"))
                        .temporary(),
                )
                .pending("flight"),
            temp("next", 1, "b"),
        ]);
        assert!(matches!(
            result,
            Err(JetFrameScheduleError::MissingCompletion { .. })
        ));
    }
    #[test]
    fn canonical_codec_round_trips_complete_schedule() {
        let schedule = derive_frame_schedule([
            JetFrameOperation::new("submit", 0)
                .with_access(
                    JetResourceAccess::named("image", JetResourceAccessMode::Write)
                        .with_region(JetResourceRegion::bytes(4, 8))
                        .with_layout(JetResourceLayout::new(64, 16, "rgba8", "gpu0"))
                        .temporary(),
                )
                .pending("flight"),
            JetFrameOperation::new("finish", 1).completion_event("flight"),
            temp("next", 2, "other"),
        ])
        .expect("schedule");
        let encoded = schedule.canonical_json();
        let decoded = JetFrameSchedule::from_canonical_json(&encoded).expect("canonical schedule");
        assert_eq!(decoded, schedule);
        assert_eq!(
            JetFrameSchedule::decode_canonical(&schedule.encode_canonical()).expect("wire schedule"),
            schedule
        );
    }
    #[test]
    fn persistent_use_blocks_temporary_reuse() {
        let first = temp("first", 0, "a");
        let persistent = op("keep", 1, "a", JetResourceAccessMode::Read);
        let next = temp("next", 2, "b");
        let schedule = derive_frame_schedule([first, persistent, next])
            .expect("persistent resource schedule");
        assert!(schedule.reuse.is_empty());
    }

    #[test]
    fn matching_temporary_layout_allows_reuse_and_explains_transfer() {
        let schedule = derive_frame_schedule([
            temp("first", 0, "a"),
            temp("second", 1, "b"),
        ])
        .expect("temporary schedule");
        assert_eq!(schedule.reuse.len(), 1);
        assert!(schedule.reuse[0].legal);
        assert!(schedule.explain().contains("reuse: a -> b"));

        let transferred = derive_frame_schedule([
            JetFrameOperation::new("cpu", 0).with_access(
                JetResourceAccess::named("image", JetResourceAccessMode::Read)
                    .with_layout(JetResourceLayout::new(64, 16, "rgba8", "cpu")),
            ),
            JetFrameOperation::new("gpu", 1).with_access(
                JetResourceAccess::named("image", JetResourceAccessMode::Read)
                    .with_layout(JetResourceLayout::new(64, 16, "rgba8", "gpu")),
            ),
        ])
        .expect("transfer schedule");
        assert_eq!(transferred.transfers.len(), 1);
        assert!(transferred.explain().contains("transfer: image cpu -> gpu"));
    }
}
