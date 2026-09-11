//! Typed bridge from a real `jet_debug` stop to the paused devtools kernel.
//!
//! This adapter owns no debugger process and parses no request JSON.  The
//! debugger backend publishes a bounded [`jet_debug::DebugSnapshot`]; this
//! module turns those facts into the canonical paused-session projections and
//! supplies read-only evaluation results to [`PausedEvaluateHost`].

use jet_foundation::Devtools::JetDevtoolsSourceIdentityFact;
use jet_debug::{DebugAggregateKind, DebugEvaluation, DebugSnapshot, DebugValue};

use crate::PausedEvaluate::{
    AggregateKind, AuthorityGrant, AuthorityRequirement, AuthoritySet, EvaluateRequest,
    EvaluationMode, EvaluationUsage, EvaluatorResult, LexicalScope, PausedEvaluateHost,
    PausedSession, SessionIdentity, SessionState, SourceLocation, TypedValue, ValueProjection,
    DEFAULT_EVALUATION_BUDGET, EVALUATE_AUTHORITY, INSPECT_AUTHORITY, MAX_VALUE_BYTES,
};

const UNKNOWN_TYPE: &str = "Unknown";
const MAX_SOURCE_COMPONENT_BYTES: usize = 256;

/// The one source-owned adapter for a paused Canvas debugger session.
///
/// A clone is cheap and intentional: request handling can take a stable view
/// of the published stop while the owning `DebugSessions` entry is refreshed
/// by a later command boundary.
#[derive(Clone, Debug)]
pub(crate) struct PausedDebugSessionAdapter {
    identity: SessionIdentity,
    source: JetDevtoolsSourceIdentityFact,
    scope_id: String,
    snapshot: DebugSnapshot,
    authority: AuthoritySet,
    state: SessionState,
}

impl PausedDebugSessionAdapter {
    pub(crate) fn new(
        identity: SessionIdentity,
        source: JetDevtoolsSourceIdentityFact,
        scope_id: impl Into<String>,
        snapshot: DebugSnapshot,
        authority: AuthoritySet,
    ) -> Result<Self, String> {
        let adapter = Self {
            identity,
            source,
            scope_id: scope_id.into(),
            snapshot,
            authority,
            state: SessionState::Paused,
        };
        adapter.validate()?;
        Ok(adapter)
    }

    /// Default capabilities for the ordinary read-only debugger view.
    pub(crate) fn debugger_authority(scope_id: &str) -> Result<AuthoritySet, String> {
        Ok(AuthoritySet::new(vec![
            AuthorityGrant::new(INSPECT_AUTHORITY, scope_id)?,
            AuthorityGrant::new(EVALUATE_AUTHORITY, scope_id)?,
        ]))
    }

    pub(crate) fn identity(&self) -> &SessionIdentity {
        &self.identity
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.state == SessionState::Paused
    }

    /// Replace the published stop after a replayed command boundary.
    pub(crate) fn refresh_snapshot(
        &mut self,
        identity: SessionIdentity,
        snapshot: DebugSnapshot,
    ) -> Result<(), String> {
        if identity.session_id != self.identity.session_id
            || identity.source_id != self.identity.source_id
            || identity.source_revision != self.identity.source_revision
        {
            return Err("paused debugger refresh changed session or source identity".to_string());
        }
        self.identity = identity;
        self.snapshot = snapshot;
        self.state = SessionState::Paused;
        self.validate()
    }

    /// Inject the current real stop into the canonical paused host.
    pub(crate) fn inject_into(&self, host: &mut PausedEvaluateHost) -> Result<(), String> {
        if self.state != SessionState::Paused {
            host.clear_session();
            return Ok(());
        }
        let location = self.source_location()?;
        let mut bindings = Vec::with_capacity(self.snapshot.locals.len());
        for (index, local) in self.snapshot.locals.iter().enumerate() {
            let id = self.value_id(index, &local.name);
            let type_name = normalized_type_name(&local.type_name);
            let value = jet_debug::evaluate_snapshot(&self.snapshot, &local.name)
                .ok()
                .and_then(|evaluation| self.projection_value(&evaluation, &id, &location).ok());
            let mut projection = match value {
                Some(value) => value,
                None => ValueProjection::redacted(
                    id,
                    bounded_name(&local.name),
                    type_name,
                    location.clone(),
                ),
            };
            if projection.type_name.is_empty() {
                projection.type_name = UNKNOWN_TYPE.to_string();
            }
            bindings.push(projection);
        }
        let scope = LexicalScope::new(
            self.scope_id.clone(),
            bounded_scope_name(&self.snapshot.function),
            None,
            location,
            bindings,
        )?;
        let session = PausedSession::paused(
            self.identity.clone(),
            vec![scope],
            self.authority.clone(),
            DEFAULT_EVALUATION_BUDGET,
        )?;
        host.clear_evaluator_result();
        host.set_session(session);
        Ok(())
    }

    /// Compute one evaluator fact from the published snapshot.  This is
    /// deliberately read-only: there is no mutation operation in the
    /// snapshot API and no fabricated observation is emitted.
    pub(crate) fn evaluate(&self, request: &EvaluateRequest) -> EvaluatorResult {
        let usage = EvaluationUsage::new(0, 0, 0, 0);
        if request.identity != self.identity || request.scope_id != self.scope_id {
            return EvaluatorResult::failed(
                request.identity.clone(),
                request.scope_id.clone(),
                "paused debugger request does not match the published stop",
                usage,
            );
        }
        if request.cancellation.requested || self.state == SessionState::Cancelled {
            return EvaluatorResult::cancelled(
                request.identity.clone(),
                request.scope_id.clone(),
                usage,
            );
        }
        if self.state != SessionState::Paused {
            return EvaluatorResult::failed(
                request.identity.clone(),
                request.scope_id.clone(),
                "paused debugger is no longer stopped",
                usage,
            );
        }
        if request.mode == EvaluationMode::Mutate {
            return EvaluatorResult::failed(
                request.identity.clone(),
                request.scope_id.clone(),
                "paused debugger evaluation is read-only; mutation is unsupported",
                usage,
            );
        }
        let requirement = match AuthorityRequirement::new(EVALUATE_AUTHORITY, &self.scope_id) {
            Ok(requirement) => requirement,
            Err(error) => {
                return EvaluatorResult::failed(
                    request.identity.clone(),
                    request.scope_id.clone(),
                    error,
                    usage,
                )
            }
        };
        if !self.authority.allows(&requirement)
            || request
                .authority
                .iter()
                .any(|requirement| !self.authority.allows(requirement))
        {
            return EvaluatorResult::failed(
                request.identity.clone(),
                request.scope_id.clone(),
                "paused debugger lacks read-only evaluation authority",
                usage,
            );
        }
        let evaluation = match jet_debug::evaluate_snapshot(&self.snapshot, &request.expression) {
            Ok(evaluation) => evaluation,
            Err(error) => {
                return EvaluatorResult::failed(
                    request.identity.clone(),
                    request.scope_id.clone(),
                    error,
                    usage,
                )
            }
        };
        let usage = EvaluationUsage::new(
            evaluation.usage.depth,
            evaluation.usage.nodes,
            evaluation.usage.bytes.min(MAX_VALUE_BYTES),
            evaluation.usage.steps,
        );
        let location = match self.source_location() {
            Ok(location) => location,
            Err(error) => {
                return EvaluatorResult::failed(
                    request.identity.clone(),
                    request.scope_id.clone(),
                    error,
                    usage,
                )
            }
        };
        let index = self
            .snapshot
            .locals
            .iter()
            .position(|local| local.name == evaluation.name)
            .unwrap_or(0);
        let id = self.value_id(index, &evaluation.name);
        let value = match self.projection_value(&evaluation, &id, &location) {
            Ok(value) => value,
            Err(error) => {
                return EvaluatorResult::failed(
                    request.identity.clone(),
                    request.scope_id.clone(),
                    error,
                    usage,
                )
            }
        };
        EvaluatorResult::completed(
            request.identity.clone(),
            request.scope_id.clone(),
            Some(value),
            usage,
        )
    }

    /// Resume, cancel, or exit only the host session belonging to this
    /// adapter.  A different active session in the shared host is untouched.
    pub(crate) fn resume(&mut self, host: &mut PausedEvaluateHost) {
        self.state = SessionState::Running;
        self.clear_if_current(host);
    }

    pub(crate) fn cancel(
        &mut self,
        host: &mut PausedEvaluateHost,
        reason: impl Into<String>,
    ) -> Result<(), String> {
        self.state = SessionState::Cancelled;
        if self.host_is_current(host) {
            host.cancel_session(reason).map_err(|error| error.to_string())?;
            host.clear_evaluator_result();
        }
        Ok(())
    }

    pub(crate) fn exit(&mut self, host: &mut PausedEvaluateHost) {
        self.state = SessionState::Finished;
        self.clear_if_current(host);
    }

    pub(crate) fn stale(&mut self, host: &mut PausedEvaluateHost) {
        self.state = SessionState::Stale;
        self.clear_if_current(host);
    }

    fn validate(&self) -> Result<(), String> {
        if self.source.source_id.as_deref() != Some(self.identity.source_id.as_str()) {
            return Err("paused debugger source identity does not match the session".to_string());
        }
        if self.source.revision.as_deref() != Some(self.identity.source_revision.as_str()) {
            return Err("paused debugger revision identity does not match the session".to_string());
        }
        for (value, label) in [
            (self.source.build_id.as_deref(), "build identity"),
            (self.source.world_id.as_deref(), "world identity"),
        ] {
            if let Some(value) = value {
                if value.is_empty()
                    || value.len() > MAX_SOURCE_COMPONENT_BYTES
                    || value.chars().any(char::is_control)
                {
                    return Err(format!("{label} is outside the bounded identity contract"));
                }
            }
        }
        if self.scope_id.is_empty()
            || self.scope_id.len() > MAX_SOURCE_COMPONENT_BYTES
            || self.scope_id.chars().any(char::is_control)
        {
            return Err("paused debugger scope identity is outside the bounded contract".to_string());
        }
        if self.snapshot.function.is_empty()
            || self.snapshot.line == 0
            || self.snapshot.locals.len() > 4_096
            || self.snapshot.call_stack.len() > 256
        {
            return Err("paused debugger snapshot is outside the bounded fact contract".to_string());
        }
        Ok(())
    }

    fn source_location(&self) -> Result<SourceLocation, String> {
        let line = self.snapshot.line as u64;
        SourceLocation::new(
            self.identity.source_id.clone(),
            self.identity.source_revision.clone(),
            line,
            line,
        )
    }

    fn value_id(&self, index: usize, name: &str) -> String {
        let candidate = format!("{}-value-{}-{}", self.identity.session_id, index, bounded_name(name));
        if candidate.len() <= MAX_SOURCE_COMPONENT_BYTES && !candidate.chars().any(char::is_control) {
            candidate
        } else {
            let digest = jet_foundation::SHA256::sha256_hex(
                format!("{}:{}:{}", self.identity.session_id, index, name).as_bytes(),
            );
            format!("value-{}", &digest[..32])
        }
    }

    fn projection_value(
        &self,
        evaluation: &DebugEvaluation,
        id: &str,
        location: &SourceLocation,
    ) -> Result<ValueProjection, String> {
        let mut projection = ValueProjection::published(
            id.to_string(),
            bounded_name(&evaluation.name),
            normalized_type_name(&evaluation.type_name),
            location.clone(),
            typed_value(&evaluation.value)?,
        );
        if let Some(TypedValue::Text(text)) = projection.value.as_mut() {
            let (bounded, truncated) = bounded_text(text);
            *text = bounded;
            projection.truncated = truncated;
        }
        Ok(projection)
    }

    fn host_is_current(&self, host: &PausedEvaluateHost) -> bool {
        host.session()
            .is_some_and(|session| session.identity == self.identity)
    }

    fn clear_if_current(&self, host: &mut PausedEvaluateHost) {
        if self.host_is_current(host) {
            host.clear_session();
        }
    }
}

fn normalized_type_name(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        UNKNOWN_TYPE.to_string()
    } else {
        bounded_name(value)
    }
}

fn bounded_name(value: &str) -> String {
    let mut name = String::new();
    for ch in value.chars().filter(|ch| !ch.is_control()) {
        if name.len().saturating_add(ch.len_utf8()) > MAX_SOURCE_COMPONENT_BYTES {
            break;
        }
        name.push(ch);
    }
    if name.is_empty() {
        UNKNOWN_TYPE.to_string()
    } else {
        name
    }
}

fn bounded_scope_name(value: &str) -> String {
    bounded_name(value)
}

fn bounded_text(value: &str) -> (String, bool) {
    let mut result = String::new();
    let mut truncated = false;
    for ch in value.chars() {
        if ch.is_control() {
            match ch {
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                _ => result.push('\u{fffd}'),
            }
        } else {
            result.push(ch);
        }
        if result.len() > MAX_VALUE_BYTES as usize {
            while result.len() > MAX_VALUE_BYTES as usize {
                result.pop();
            }
            truncated = true;
            break;
        }
    }
    (result, truncated)
}

fn typed_value(value: &DebugValue) -> Result<TypedValue, String> {
    Ok(match value {
        DebugValue::Unit => TypedValue::Unit,
        DebugValue::Bool(value) => TypedValue::Bool(*value),
        DebugValue::Int(value) => TypedValue::Int(*value),
        DebugValue::FloatBits(bits) => TypedValue::FloatBits(*bits),
        DebugValue::Text(value) => TypedValue::Text(value.clone()),
        DebugValue::Bytes { length, sha256 } => TypedValue::Bytes {
            length: *length,
            sha256: sha256.clone(),
        },
        DebugValue::Aggregate { kind, length } => TypedValue::Aggregate {
            kind: match kind {
                DebugAggregateKind::Record => AggregateKind::Record,
                DebugAggregateKind::List => AggregateKind::List,
                DebugAggregateKind::Map => AggregateKind::Map,
                DebugAggregateKind::Set => AggregateKind::Set,
                DebugAggregateKind::Tuple => AggregateKind::Tuple,
                DebugAggregateKind::Variant => AggregateKind::Variant,
            },
            length: *length,
        },
    })
}
