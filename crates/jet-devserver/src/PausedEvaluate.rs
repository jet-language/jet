//! Typed paused-session inspection and evaluation boundary.
//!
//! This module is deliberately an adapter, not an evaluator.  A paused
//! session supplies source-owned lexical facts and an injected evaluator
//! supplies a checked result.  The adapter accepts neither a closure nor a
//! command to run: it only checks identity, authority, cancellation, and
//! budgets before projecting the supplied result.

use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::{json_escape, json_int, parse_json_with_limit};
use jet_foundation::SHA256::sha256_hex;
use std::collections::BTreeSet;
use std::fmt;

pub const PAUSED_EVALUATE_PROTOCOL: &str = "jet.devtools.v1";
pub const PAUSED_EVALUATE_SCHEMA_VERSION: u32 = 1;
pub const INSPECT_AUTHORITY: &str = "devtools.inspect";
pub const EVALUATE_AUTHORITY: &str = "devtools.evaluate";
pub const MUTATE_AUTHORITY: &str = "devtools.mutate";

pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_EXPRESSION_BYTES: usize = 16 * 1024;
pub const MAX_SCOPES: usize = 256;
pub const MAX_VALUE_DEPTH: u32 = 64;
pub const MAX_VALUE_NODES: u64 = 4_096;
pub const MAX_VALUE_BYTES: u64 = 1024 * 1024;
pub const MAX_EVALUATION_STEPS: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    Running,
    Paused,
    Finished,
    Stale,
    Cancelled,
}

impl SessionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Finished => "finished",
            Self::Stale => "stale",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluationMode {
    ReadOnly,
    Mutate,
}

impl EvaluationMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Mutate => "mutate",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Publication {
    Published,
    Redacted,
    Absent,
}

impl Publication {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Redacted => "redacted",
            Self::Absent => "absent",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AggregateKind {
    Record,
    List,
    Map,
    Set,
    Tuple,
    Variant,
}

impl AggregateKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Record => "record",
            Self::List => "list",
            Self::Map => "map",
            Self::Set => "set",
            Self::Tuple => "tuple",
            Self::Variant => "variant",
        }
    }
}

/// A typed leaf or aggregate header.  Aggregate children live in
/// [`ValueProjection::children`], so truncation never requires serializing a
/// large value into display text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypedValue {
    Unit,
    Bool(bool),
    Int(i64),
    FloatBits(u64),
    Text(String),
    Bytes { length: u64, sha256: String },
    Aggregate { kind: AggregateKind, length: u64 },
}

impl TypedValue {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Text(value) => valid_text(value, "typed text", MAX_VALUE_BYTES as usize),
            Self::Bytes { sha256, .. } => {
                if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err("typed bytes must carry a 64-character SHA-256 digest".to_string());
                }
                Ok(())
            }
            Self::Unit
            | Self::Bool(_)
            | Self::Int(_)
            | Self::FloatBits(_)
            | Self::Aggregate { .. } => Ok(()),
        }
    }

    fn json(&self) -> String {
        match self {
            Self::Unit => "{\"kind\":\"unit\"}".to_string(),
            Self::Bool(value) => format!("{{\"kind\":\"bool\",\"value\":{value}}}"),
            Self::Int(value) => format!("{{\"kind\":\"int\",\"value\":{value}}}"),
            Self::FloatBits(bits) => {
                format!("{{\"kind\":\"float_bits\",\"bits\":{bits}}}")
            }
            Self::Text(value) => format!(
                "{{\"kind\":\"text\",\"value\":\"{}\"}}",
                json_escape(value)
            ),
            Self::Bytes { length, sha256 } => format!(
                "{{\"kind\":\"bytes\",\"length\":{length},\"sha256\":\"{}\"}}",
                json_escape(sha256)
            ),
            Self::Aggregate { kind, length } => format!(
                "{{\"kind\":\"{}\",\"length\":{length}}}",
                kind.as_str()
            ),
        }
    }

    fn bytes(&self) -> u64 {
        match self {
            Self::Unit => 1,
            Self::Bool(_) => 1,
            Self::Int(_) | Self::FloatBits(_) => 8,
            Self::Text(value) => value.len() as u64,
            Self::Bytes { .. } => 32,
            Self::Aggregate { .. } => 8,
        }
    }

    fn is_aggregate(&self) -> bool {
        matches!(self, Self::Aggregate { .. })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceLocation {
    pub source_id: String,
    pub source_revision: String,
    pub start: u64,
    pub end: u64,
}

impl SourceLocation {
    pub fn new(
        source_id: impl Into<String>,
        source_revision: impl Into<String>,
        start: u64,
        end: u64,
    ) -> Result<Self, String> {
        let location = Self {
            source_id: source_id.into(),
            source_revision: source_revision.into(),
            start,
            end,
        };
        location.validate()?;
        Ok(location)
    }

    fn validate(&self) -> Result<(), String> {
        valid_id(&self.source_id, "source id")?;
        valid_id(&self.source_revision, "source revision")?;
        if self.end < self.start {
            return Err("source location ends before it starts".to_string());
        }
        Ok(())
    }

    fn json(&self) -> String {
        format!(
            "{{\"source_id\":\"{}\",\"source_revision\":\"{}\",\"start\":{},\"end\":{}}}",
            json_escape(&self.source_id),
            json_escape(&self.source_revision),
            self.start,
            self.end
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionIdentity {
    pub session_id: String,
    pub source_id: String,
    pub source_revision: String,
    pub pause_id: String,
    pub frame_id: String,
}

impl SessionIdentity {
    pub fn new(
        session_id: impl Into<String>,
        source_id: impl Into<String>,
        source_revision: impl Into<String>,
        pause_id: impl Into<String>,
        frame_id: impl Into<String>,
    ) -> Result<Self, String> {
        let identity = Self {
            session_id: session_id.into(),
            source_id: source_id.into(),
            source_revision: source_revision.into(),
            pause_id: pause_id.into(),
            frame_id: frame_id.into(),
        };
        identity.validate()?;
        Ok(identity)
    }

    fn validate(&self) -> Result<(), String> {
        valid_id(&self.session_id, "session id")?;
        valid_id(&self.source_id, "source id")?;
        valid_id(&self.source_revision, "source revision")?;
        valid_id(&self.pause_id, "pause id")?;
        valid_id(&self.frame_id, "frame id")
    }

    fn source_matches(&self, location: &SourceLocation) -> bool {
        self.source_id == location.source_id && self.source_revision == location.source_revision
    }

    fn json(&self) -> String {
        format!(
            "{{\"session_id\":\"{}\",\"source_id\":\"{}\",\"source_revision\":\"{}\",\"pause_id\":\"{}\",\"frame_id\":\"{}\"}}",
            json_escape(&self.session_id),
            json_escape(&self.source_id),
            json_escape(&self.source_revision),
            json_escape(&self.pause_id),
            json_escape(&self.frame_id)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct AuthorityRequirement {
    pub capability: String,
    pub scope_id: String,
}

impl AuthorityRequirement {
    pub fn new(capability: impl Into<String>, scope_id: impl Into<String>) -> Result<Self, String> {
        let requirement = Self {
            capability: capability.into(),
            scope_id: scope_id.into(),
        };
        requirement.validate()?;
        Ok(requirement)
    }

    fn validate(&self) -> Result<(), String> {
        valid_id(&self.capability, "authority capability")?;
        valid_id(&self.scope_id, "authority scope")
    }

    fn json(&self) -> String {
        format!(
            "{{\"capability\":\"{}\",\"scope_id\":\"{}\"}}",
            json_escape(&self.capability),
            json_escape(&self.scope_id)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct AuthorityGrant {
    pub capability: String,
    pub scope_id: String,
}

impl AuthorityGrant {
    pub fn new(capability: impl Into<String>, scope_id: impl Into<String>) -> Result<Self, String> {
        let grant = Self {
            capability: capability.into(),
            scope_id: scope_id.into(),
        };
        grant.validate()?;
        Ok(grant)
    }

    fn validate(&self) -> Result<(), String> {
        valid_id(&self.capability, "authority capability")?;
        valid_id(&self.scope_id, "authority scope")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthoritySet {
    pub grants: Vec<AuthorityGrant>,
}

impl AuthoritySet {
    pub fn new(mut grants: Vec<AuthorityGrant>) -> Self {
        grants.sort();
        grants.dedup();
        Self { grants }
    }

    pub fn allows(&self, requirement: &AuthorityRequirement) -> bool {
        self.grants.iter().any(|grant| {
            grant.capability == requirement.capability && grant.scope_id == requirement.scope_id
        })
    }

    fn validate(&self) -> Result<(), String> {
        for grant in &self.grants {
            grant.validate()?;
        }
        Ok(())
    }

}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvaluationBudget {
    pub max_depth: u32,
    pub max_nodes: u64,
    pub max_bytes: u64,
    pub max_steps: u64,
}

pub const DEFAULT_EVALUATION_BUDGET: EvaluationBudget = EvaluationBudget {
    max_depth: 32,
    max_nodes: 1_024,
    max_bytes: 256 * 1024,
    max_steps: 100_000,
};

impl EvaluationBudget {
    pub fn new(
        max_depth: u32,
        max_nodes: u64,
        max_bytes: u64,
        max_steps: u64,
    ) -> Result<Self, String> {
        let budget = Self {
            max_depth,
            max_nodes,
            max_bytes,
            max_steps,
        };
        budget.validate()?;
        Ok(budget)
    }

    fn validate(&self) -> Result<(), String> {
        if self.max_depth == 0
            || self.max_depth > MAX_VALUE_DEPTH
            || self.max_nodes == 0
            || self.max_nodes > MAX_VALUE_NODES
            || self.max_bytes == 0
            || self.max_bytes > MAX_VALUE_BYTES
            || self.max_steps == 0
            || self.max_steps > MAX_EVALUATION_STEPS
        {
            return Err("paused evaluation budget is outside its bounded range".to_string());
        }
        Ok(())
    }

    fn contains(&self, other: &Self) -> bool {
        other.max_depth <= self.max_depth
            && other.max_nodes <= self.max_nodes
            && other.max_bytes <= self.max_bytes
            && other.max_steps <= self.max_steps
    }

    fn fits(&self, usage: EvaluationUsage) -> bool {
        usage.depth <= self.max_depth
            && usage.nodes <= self.max_nodes
            && usage.bytes <= self.max_bytes
            && usage.steps <= self.max_steps
    }

    fn json(&self) -> String {
        format!(
            "{{\"max_depth\":{},\"max_nodes\":{},\"max_bytes\":{},\"max_steps\":{}}}",
            self.max_depth, self.max_nodes, self.max_bytes, self.max_steps
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EvaluationUsage {
    pub depth: u32,
    pub nodes: u64,
    pub bytes: u64,
    pub steps: u64,
}

impl EvaluationUsage {
    pub const fn new(depth: u32, nodes: u64, bytes: u64, steps: u64) -> Self {
        Self {
            depth,
            nodes,
            bytes,
            steps,
        }
    }

    fn json(self) -> String {
        format!(
            "{{\"depth\":{},\"nodes\":{},\"bytes\":{},\"steps\":{}}}",
            self.depth, self.nodes, self.bytes, self.steps
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Cancellation {
    pub requested: bool,
    pub reason: Option<String>,
}

impl Cancellation {
    pub const fn none() -> Self {
        Self {
            requested: false,
            reason: None,
        }
    }

    pub fn requested(reason: impl Into<String>) -> Result<Self, String> {
        let reason = reason.into();
        valid_text(&reason, "cancellation reason", 256)?;
        Ok(Self {
            requested: true,
            reason: Some(reason),
        })
    }

    fn validate(&self) -> Result<(), String> {
        if let Some(reason) = &self.reason {
            valid_text(reason, "cancellation reason", 256)?;
        }
        if !self.requested && self.reason.is_some() {
            return Err("a non-requested cancellation cannot carry a reason".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueProjection {
    pub id: String,
    pub name: String,
    pub type_name: String,
    pub source: SourceLocation,
    pub publication: Publication,
    pub value: Option<TypedValue>,
    pub children: Vec<ValueProjection>,
    pub truncated: bool,
}

impl ValueProjection {
    pub fn published(
        id: impl Into<String>,
        name: impl Into<String>,
        type_name: impl Into<String>,
        source: SourceLocation,
        value: TypedValue,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            type_name: type_name.into(),
            source,
            publication: Publication::Published,
            value: Some(value),
            children: Vec::new(),
            truncated: false,
        }
    }

    pub fn redacted(
        id: impl Into<String>,
        name: impl Into<String>,
        type_name: impl Into<String>,
        source: SourceLocation,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            type_name: type_name.into(),
            source,
            publication: Publication::Redacted,
            value: None,
            children: Vec::new(),
            truncated: false,
        }
    }

    pub fn absent(
        id: impl Into<String>,
        name: impl Into<String>,
        type_name: impl Into<String>,
        source: SourceLocation,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            type_name: type_name.into(),
            source,
            publication: Publication::Absent,
            value: None,
            children: Vec::new(),
            truncated: false,
        }
    }

    pub fn with_children(mut self, children: Vec<ValueProjection>) -> Self {
        self.children = children;
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut ids = BTreeSet::new();
        let mut nodes = 0u64;
        self.validate_inner(None, 1, &mut ids, &mut nodes)
    }

    fn validate_for_session(&self, identity: &SessionIdentity) -> Result<(), String> {
        let mut ids = BTreeSet::new();
        let mut nodes = 0u64;
        self.validate_inner(Some(identity), 1, &mut ids, &mut nodes)
    }

    fn validate_inner(
        &self,
        identity: Option<&SessionIdentity>,
        depth: u32,
        ids: &mut BTreeSet<String>,
        nodes: &mut u64,
    ) -> Result<(), String> {
        if depth > MAX_VALUE_DEPTH {
            return Err("paused value exceeds the maximum projection depth".to_string());
        }
        *nodes = nodes.saturating_add(1);
        if *nodes > MAX_VALUE_NODES {
            return Err("paused value exceeds the maximum projection node count".to_string());
        }
        valid_id(&self.id, "value id")?;
        valid_text(&self.name, "value name", 256)?;
        valid_id(&self.type_name, "value type")?;
        self.source.validate()?;
        if !ids.insert(self.id.clone()) {
            return Err("paused value ids must be unique within a projection".to_string());
        }
        if let Some(identity) = identity {
            if !identity.source_matches(&self.source) {
                return Err("paused value source identity does not match the session".to_string());
            }
        }
        match self.publication {
            Publication::Published => {
                let value = self
                    .value
                    .as_ref()
                    .ok_or_else(|| "published value is missing its typed payload".to_string())?;
                value.validate()?;
                if !value.is_aggregate() && !self.children.is_empty() {
                    return Err("only aggregate values may carry projected children".to_string());
                }
            }
            Publication::Redacted | Publication::Absent => {
                if self.value.is_some() || !self.children.is_empty() {
                    return Err("redacted or absent values cannot carry a payload".to_string());
                }
            }
        }
        for child in &self.children {
            child.validate_inner(identity, depth.saturating_add(1), ids, nodes)?;
        }
        Ok(())
    }

    fn metadata_bytes(&self) -> u64 {
        self.id
            .len()
            .saturating_add(self.name.len())
            .saturating_add(self.type_name.len())
            .saturating_add(self.source.source_id.len())
            .saturating_add(self.source.source_revision.len())
            .saturating_add(48) as u64
    }

    fn measure_inner(&self, depth: u32, usage: &mut EvaluationUsage) {
        if self.publication == Publication::Absent {
            return;
        }
        usage.depth = usage.depth.max(depth);
        usage.nodes = usage.nodes.saturating_add(1);
        usage.bytes = usage
            .bytes
            .saturating_add(self.metadata_bytes())
            .saturating_add(self.value.as_ref().map_or(0, TypedValue::bytes));
        for child in &self.children {
            child.measure_inner(depth.saturating_add(1), usage);
        }
    }

    fn measure(&self) -> EvaluationUsage {
        let mut usage = EvaluationUsage::default();
        self.measure_inner(1, &mut usage);
        usage
    }

    fn json(&self) -> Option<String> {
        if self.publication == Publication::Absent {
            return None;
        }
        let value = self
            .value
            .as_ref()
            .map(TypedValue::json)
            .map(|value| format!(",\"value\":{value}"))
            .unwrap_or_default();
        let children = array_json(self.children.iter().filter_map(ValueProjection::json));
        Some(format!(
            "{{\"id\":\"{}\",\"name\":\"{}\",\"type\":\"{}\",\"source\":{},\"publication\":\"{}\"{} ,\"children\":{},\"truncated\":{}}}",
            json_escape(&self.id),
            json_escape(&self.name),
            json_escape(&self.type_name),
            self.source.json(),
            self.publication.as_str(),
            value,
            children,
            self.truncated
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LexicalScope {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub source: SourceLocation,
    pub bindings: Vec<ValueProjection>,
}

impl LexicalScope {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        parent_id: Option<String>,
        source: SourceLocation,
        bindings: Vec<ValueProjection>,
    ) -> Result<Self, String> {
        let scope = Self {
            id: id.into(),
            name: name.into(),
            parent_id,
            source,
            bindings,
        };
        scope.validate(None)?;
        Ok(scope)
    }

    fn validate(&self, identity: Option<&SessionIdentity>) -> Result<(), String> {
        valid_id(&self.id, "scope id")?;
        valid_text(&self.name, "scope name", 256)?;
        if let Some(parent) = &self.parent_id {
            valid_id(parent, "parent scope id")?;
        }
        self.source.validate()?;
        if let Some(identity) = identity {
            if !identity.source_matches(&self.source) {
                return Err("lexical scope source identity does not match the session".to_string());
            }
        }
        let mut ids = BTreeSet::new();
        let mut nodes = 0u64;
        for binding in &self.bindings {
            binding.validate_inner(identity, 1, &mut ids, &mut nodes)?;
        }
        Ok(())
    }

    fn visible_bindings(&self) -> impl Iterator<Item = &ValueProjection> {
        self.bindings
            .iter()
            .filter(|binding| binding.publication != Publication::Absent)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeProjection {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub source: SourceLocation,
    pub bindings: Vec<ValueProjection>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ProjectionStats {
    usage: EvaluationUsage,
    truncated: bool,
    published: u64,
    redacted: u64,
}

impl ScopeProjection {
    fn project(
        scope: &LexicalScope,
        identity: &SessionIdentity,
        budget: EvaluationBudget,
        selected_value: Option<&str>,
    ) -> Result<(Self, ProjectionStats), String> {
        let mut stats = ProjectionStats::default();
        let mut bindings = scope
            .visible_bindings()
            .filter(|binding| selected_value.is_none_or(|id| id == binding.id))
            .collect::<Vec<_>>();
        bindings.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        let mut projected = Vec::with_capacity(bindings.len());
        for binding in bindings {
            if binding.publication == Publication::Published {
                stats.published = stats.published.saturating_add(1);
            } else {
                stats.redacted = stats.redacted.saturating_add(1);
            }
            let value = project_root(binding, identity, budget, &mut stats)?;
            projected.push(value);
        }
        Ok((
            Self {
                id: scope.id.clone(),
                name: scope.name.clone(),
                parent_id: scope.parent_id.clone(),
                source: scope.source.clone(),
                bindings: projected,
            },
            stats,
        ))
    }

    pub fn json(&self) -> String {
        let bindings = array_json(self.bindings.iter().filter_map(ValueProjection::json));
        format!(
            "{{\"id\":\"{}\",\"name\":\"{}\",\"parent_id\":{},\"source\":{},\"bindings\":{}}}",
            json_escape(&self.id),
            json_escape(&self.name),
            self.parent_id
                .as_deref()
                .map(|id| format!("\"{}\"", json_escape(id)))
                .unwrap_or_else(|| "null".to_string()),
            self.source.json(),
            bindings
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PausedSession {
    pub identity: SessionIdentity,
    pub state: SessionState,
    pub scopes: Vec<LexicalScope>,
    pub authority: AuthoritySet,
    pub budget: EvaluationBudget,
    pub cancellation: Cancellation,
}

impl PausedSession {
    pub fn new(
        identity: SessionIdentity,
        state: SessionState,
        scopes: Vec<LexicalScope>,
        authority: AuthoritySet,
        budget: EvaluationBudget,
    ) -> Result<Self, String> {
        identity.validate()?;
        budget.validate()?;
        authority.validate()?;
        if scopes.len() > MAX_SCOPES {
            return Err("paused session exceeds the lexical scope limit".to_string());
        }
        let mut ids = BTreeSet::new();
        for scope in &scopes {
            scope.validate(Some(&identity))?;
            if !ids.insert(scope.id.clone()) {
                return Err("paused session scope ids must be unique".to_string());
            }
        }
        for scope in &scopes {
            if let Some(parent) = &scope.parent_id {
                if !ids.contains(parent) {
                    return Err("paused lexical scope refers to a missing parent".to_string());
                }
            }
        }
        Ok(Self {
            identity,
            state,
            scopes,
            authority,
            budget,
            cancellation: Cancellation::none(),
        })
    }

    pub fn paused(
        identity: SessionIdentity,
        scopes: Vec<LexicalScope>,
        authority: AuthoritySet,
        budget: EvaluationBudget,
    ) -> Result<Self, String> {
        Self::new(identity, SessionState::Paused, scopes, authority, budget)
    }

    pub fn cancel(&mut self, reason: impl Into<String>) -> Result<(), String> {
        self.cancellation = Cancellation::requested(reason)?;
        self.state = SessionState::Cancelled;
        Ok(())
    }

    fn scope(&self, id: &str) -> Option<&LexicalScope> {
        self.scopes.iter().find(|scope| scope.id == id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectRequest {
    pub request_id: String,
    pub identity: SessionIdentity,
    pub scope_id: String,
    pub value_id: Option<String>,
    pub budget: EvaluationBudget,
    pub authority: Vec<AuthorityRequirement>,
    pub cancellation: Cancellation,
}

impl InspectRequest {
    pub fn new(
        request_id: impl Into<String>,
        identity: SessionIdentity,
        scope_id: impl Into<String>,
        budget: EvaluationBudget,
        authority: Vec<AuthorityRequirement>,
    ) -> Result<Self, String> {
        let request = Self {
            request_id: request_id.into(),
            identity,
            scope_id: scope_id.into(),
            value_id: None,
            budget,
            authority,
            cancellation: Cancellation::none(),
        };
        request.validate_shape()?;
        Ok(request)
    }

    fn validate_shape(&self) -> Result<(), String> {
        valid_id(&self.request_id, "request id")?;
        self.identity.validate()?;
        valid_id(&self.scope_id, "scope id")?;
        if let Some(value_id) = &self.value_id {
            valid_id(value_id, "value id")?;
        }
        self.budget.validate()?;
        self.cancellation.validate()?;
        for requirement in &self.authority {
            requirement.validate()?;
        }
        Ok(())
    }

    pub fn from_json(raw: &str) -> Result<Self, String> {
        parse_inspect_request(raw)
    }

    pub fn json(&self) -> String {
        format!(
            "{{\"protocol\":\"{}\",\"schema_version\":{},\"kind\":\"paused.inspect\",\"request_id\":\"{}\",\"identity\":{},\"scope_id\":\"{}\",\"value_id\":{},\"budget\":{},\"authority\":{},\"cancelled\":{}}}",
            PAUSED_EVALUATE_PROTOCOL,
            PAUSED_EVALUATE_SCHEMA_VERSION,
            json_escape(&self.request_id),
            self.identity.json(),
            json_escape(&self.scope_id),
            self.value_id
                .as_deref()
                .map(|id| format!("\"{}\"", json_escape(id)))
                .unwrap_or_else(|| "null".to_string()),
            self.budget.json(),
            array_json(self.authority.iter().map(AuthorityRequirement::json)),
            self.cancellation.requested
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluateRequest {
    pub request_id: String,
    pub identity: SessionIdentity,
    pub scope_id: String,
    pub expression: String,
    pub mode: EvaluationMode,
    pub effects: Vec<String>,
    pub authority: Vec<AuthorityRequirement>,
    pub budget: EvaluationBudget,
    pub cancellation: Cancellation,
}

impl EvaluateRequest {
    pub fn new(
        request_id: impl Into<String>,
        identity: SessionIdentity,
        scope_id: impl Into<String>,
        expression: impl Into<String>,
        mode: EvaluationMode,
        effects: Vec<String>,
        authority: Vec<AuthorityRequirement>,
        budget: EvaluationBudget,
    ) -> Result<Self, String> {
        let request = Self {
            request_id: request_id.into(),
            identity,
            scope_id: scope_id.into(),
            expression: expression.into(),
            mode,
            effects,
            authority,
            budget,
            cancellation: Cancellation::none(),
        };
        request.validate_shape()?;
        Ok(request)
    }

    fn validate_shape(&self) -> Result<(), String> {
        valid_id(&self.request_id, "request id")?;
        self.identity.validate()?;
        valid_id(&self.scope_id, "scope id")?;
        valid_text(&self.expression, "evaluation expression", MAX_EXPRESSION_BYTES)?;
        if self.mode == EvaluationMode::Mutate && self.effects.is_empty() {
            return Err("mutating evaluation requires at least one checked effect".to_string());
        }
        for effect in &self.effects {
            valid_id(effect, "evaluation effect")?;
        }
        self.budget.validate()?;
        self.cancellation.validate()?;
        for requirement in &self.authority {
            requirement.validate()?;
        }
        Ok(())
    }

    pub fn from_json(raw: &str) -> Result<Self, String> {
        parse_evaluate_request(raw)
    }

    pub fn json(&self) -> String {
        format!(
            "{{\"protocol\":\"{}\",\"schema_version\":{},\"kind\":\"paused.evaluate\",\"request_id\":\"{}\",\"identity\":{},\"scope_id\":\"{}\",\"expression\":\"{}\",\"mode\":\"{}\",\"effects\":{},\"authority\":{},\"budget\":{},\"cancelled\":{}}}",
            PAUSED_EVALUATE_PROTOCOL,
            PAUSED_EVALUATE_SCHEMA_VERSION,
            json_escape(&self.request_id),
            self.identity.json(),
            json_escape(&self.scope_id),
            json_escape(&self.expression),
            self.mode.as_str(),
            array_json(self.effects.iter().map(|effect| {
                format!("\"{}\"", json_escape(effect))
            })),
            array_json(self.authority.iter().map(AuthorityRequirement::json)),
            self.budget.json(),
            self.cancellation.requested
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluatorStatus {
    Completed,
    Failed,
    Cancelled,
}

impl EvaluatorStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationFact {
    pub event_id: String,
    pub kind: String,
    pub session_id: String,
    pub source_id: String,
    pub source_revision: String,
    pub scope_id: String,
    pub target_id: String,
    pub effect: String,
}

impl ObservationFact {
    pub fn new(
        event_id: impl Into<String>,
        kind: impl Into<String>,
        session_id: impl Into<String>,
        source_id: impl Into<String>,
        source_revision: impl Into<String>,
        scope_id: impl Into<String>,
        target_id: impl Into<String>,
        effect: impl Into<String>,
    ) -> Result<Self, String> {
        let fact = Self {
            event_id: event_id.into(),
            kind: kind.into(),
            session_id: session_id.into(),
            source_id: source_id.into(),
            source_revision: source_revision.into(),
            scope_id: scope_id.into(),
            target_id: target_id.into(),
            effect: effect.into(),
        };
        fact.validate_shape()?;
        Ok(fact)
    }

    fn validate_shape(&self) -> Result<(), String> {
        valid_id(&self.event_id, "observation id")?;
        valid_id(&self.kind, "observation kind")?;
        valid_id(&self.session_id, "observation session id")?;
        valid_id(&self.source_id, "observation source id")?;
        valid_id(&self.source_revision, "observation source revision")?;
        valid_id(&self.scope_id, "observation scope id")?;
        valid_id(&self.target_id, "observation target id")?;
        valid_id(&self.effect, "observation effect")
    }

    fn matches(&self, session: &PausedSession, request: &EvaluateRequest) -> bool {
        self.session_id == session.identity.session_id
            && self.source_id == session.identity.source_id
            && self.source_revision == session.identity.source_revision
            && self.scope_id == request.scope_id
            && request.effects.iter().any(|effect| effect == &self.effect)
    }

    fn json(&self) -> String {
        format!(
            "{{\"event_id\":\"{}\",\"kind\":\"{}\",\"session_id\":\"{}\",\"source_id\":\"{}\",\"source_revision\":\"{}\",\"scope_id\":\"{}\",\"target_id\":\"{}\",\"effect\":\"{}\"}}",
            json_escape(&self.event_id),
            json_escape(&self.kind),
            json_escape(&self.session_id),
            json_escape(&self.source_id),
            json_escape(&self.source_revision),
            json_escape(&self.scope_id),
            json_escape(&self.target_id),
            json_escape(&self.effect)
        )
    }
}

/// Result supplied by the real paused evaluator.  `evaluate` consumes this
/// fact; it never invokes a closure, interpreter, process, or host command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluatorResult {
    pub identity: SessionIdentity,
    pub scope_id: String,
    pub status: EvaluatorStatus,
    pub value: Option<ValueProjection>,
    pub usage: EvaluationUsage,
    pub changed: bool,
    pub observation: Option<ObservationFact>,
    pub error: Option<String>,
}

impl EvaluatorResult {
    pub fn completed(
        identity: SessionIdentity,
        scope_id: impl Into<String>,
        value: Option<ValueProjection>,
        usage: EvaluationUsage,
    ) -> Self {
        Self {
            identity,
            scope_id: scope_id.into(),
            status: EvaluatorStatus::Completed,
            value,
            usage,
            changed: false,
            observation: None,
            error: None,
        }
    }

    pub fn failed(
        identity: SessionIdentity,
        scope_id: impl Into<String>,
        message: impl Into<String>,
        usage: EvaluationUsage,
    ) -> Self {
        Self {
            identity,
            scope_id: scope_id.into(),
            status: EvaluatorStatus::Failed,
            value: None,
            usage,
            changed: false,
            observation: None,
            error: Some(message.into()),
        }
    }

    pub fn cancelled(
        identity: SessionIdentity,
        scope_id: impl Into<String>,
        usage: EvaluationUsage,
    ) -> Self {
        Self {
            identity,
            scope_id: scope_id.into(),
            status: EvaluatorStatus::Cancelled,
            value: None,
            usage,
            changed: false,
            observation: None,
            error: None,
        }
    }

    pub fn mutated(
        identity: SessionIdentity,
        scope_id: impl Into<String>,
        value: Option<ValueProjection>,
        usage: EvaluationUsage,
        observation: ObservationFact,
    ) -> Self {
        Self {
            identity,
            scope_id: scope_id.into(),
            status: EvaluatorStatus::Completed,
            value,
            usage,
            changed: true,
            observation: Some(observation),
            error: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditOutcome {
    Accepted,
    Rejected,
}

impl AuditOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectReason {
    InvalidRequest,
    SessionNotPaused,
    StaleSession,
    SessionIdentityMismatch,
    SourceIdentityMismatch,
    PauseIdentityMismatch,
    FrameIdentityMismatch,
    ScopeNotFound,
    ValueNotFound,
    ValueNotPublished,
    InsufficientAuthority,
    BudgetExceeded,
    Cancelled,
    EvaluatorFailed,
    MutationNotAllowed,
    ObservationMissing,
    InvalidEvaluatorResult,
}

impl RejectReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::SessionNotPaused => "session_not_paused",
            Self::StaleSession => "stale_session",
            Self::SessionIdentityMismatch => "session_identity_mismatch",
            Self::SourceIdentityMismatch => "source_identity_mismatch",
            Self::PauseIdentityMismatch => "pause_identity_mismatch",
            Self::FrameIdentityMismatch => "frame_identity_mismatch",
            Self::ScopeNotFound => "scope_not_found",
            Self::ValueNotFound => "value_not_found",
            Self::ValueNotPublished => "value_not_published",
            Self::InsufficientAuthority => "insufficient_authority",
            Self::BudgetExceeded => "budget_exceeded",
            Self::Cancelled => "cancelled",
            Self::EvaluatorFailed => "evaluator_failed",
            Self::MutationNotAllowed => "mutation_not_allowed",
            Self::ObservationMissing => "observation_missing",
            Self::InvalidEvaluatorResult => "invalid_evaluator_result",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditReceipt {
    pub receipt_id: String,
    pub operation: String,
    pub request_id: String,
    pub outcome: AuditOutcome,
    pub reason: Option<RejectReason>,
    pub identity: SessionIdentity,
    pub scope_id: String,
    pub authority: Vec<AuthorityRequirement>,
    pub usage: EvaluationUsage,
    pub truncated: bool,
    pub published_values: u64,
    pub redacted_values: u64,
    pub observation_id: Option<String>,
}

impl AuditReceipt {
    fn new(
        operation: &str,
        request_id: &str,
        identity: &SessionIdentity,
        scope_id: &str,
        authority: &[AuthorityRequirement],
        outcome: AuditOutcome,
        reason: Option<RejectReason>,
        usage: EvaluationUsage,
        truncated: bool,
        published_values: u64,
        redacted_values: u64,
        observation_id: Option<&str>,
    ) -> Self {
        let mut authority = authority.to_vec();
        authority.sort();
        authority.dedup();
        let authority_key = authority
            .iter()
            .map(|requirement| format!("{}@{}", requirement.capability, requirement.scope_id))
            .collect::<Vec<_>>()
            .join(",");
        let observation_id = observation_id.map(str::to_string);
        let canonical = format!(
            "paused-audit-v1|{operation}|{request_id}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            identity.session_id,
            identity.source_id,
            identity.source_revision,
            identity.pause_id,
            identity.frame_id,
            scope_id,
            outcome.as_str(),
            reason.map(RejectReason::as_str).unwrap_or(""),
            authority_key,
            usage.depth,
            usage.nodes,
            usage.bytes,
            usage.steps,
            truncated,
            published_values,
            redacted_values,
            observation_id.as_deref().unwrap_or("")
        );
        let digest = sha256_hex(canonical.as_bytes());
        Self {
            receipt_id: format!("paused-audit-{}", &digest[..24]),
            operation: operation.to_string(),
            request_id: request_id.to_string(),
            outcome,
            reason,
            identity: identity.clone(),
            scope_id: scope_id.to_string(),
            authority,
            usage,
            truncated,
            published_values,
            redacted_values,
            observation_id,
        }
    }

    pub fn json(&self) -> String {
        let reason = self
            .reason
            .map(|reason| format!("\"{}\"", reason.as_str()))
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"receipt_id\":\"{}\",\"operation\":\"{}\",\"request_id\":\"{}\",\"outcome\":\"{}\",\"reason\":{},\"identity\":{},\"scope_id\":\"{}\",\"authority\":{},\"usage\":{},\"truncated\":{},\"published_values\":{},\"redacted_values\":{},\"observation_id\":{}}}",
            json_escape(&self.receipt_id),
            json_escape(&self.operation),
            json_escape(&self.request_id),
            self.outcome.as_str(),
            reason,
            self.identity.json(),
            json_escape(&self.scope_id),
            array_json(self.authority.iter().map(AuthorityRequirement::json)),
            self.usage.json(),
            self.truncated,
            self.published_values,
            self.redacted_values,
            self.observation_id
                .as_deref()
                .map(|id| format!("\"{}\"", json_escape(id)))
                .unwrap_or_else(|| "null".to_string())
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestError {
    pub reason: RejectReason,
    pub audit: AuditReceipt,
}

impl RequestError {
    pub fn json(&self) -> String {
        format!(
            "{{\"protocol\":\"{}\",\"schema_version\":{},\"ok\":false,\"error\":\"{}\",\"audit\":{}}}",
            PAUSED_EVALUATE_PROTOCOL,
            PAUSED_EVALUATE_SCHEMA_VERSION,
            self.reason.as_str(),
            self.audit.json()
        )
    }
}

impl fmt::Display for RequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.reason.as_str())
    }
}

impl std::error::Error for RequestError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectResponse {
    pub identity: SessionIdentity,
    pub scope: ScopeProjection,
    pub selected_value_id: Option<String>,
    pub truncated: bool,
    pub audit: AuditReceipt,
}

impl InspectResponse {
    pub fn json(&self) -> String {
        format!(
            "{{\"protocol\":\"{}\",\"schema_version\":{},\"ok\":true,\"kind\":\"paused.inspect\",\"identity\":{},\"scope\":{},\"selected_value_id\":{},\"truncated\":{},\"audit\":{}}}",
            PAUSED_EVALUATE_PROTOCOL,
            PAUSED_EVALUATE_SCHEMA_VERSION,
            self.identity.json(),
            self.scope.json(),
            self.selected_value_id
                .as_deref()
                .map(|id| format!("\"{}\"", json_escape(id)))
                .unwrap_or_else(|| "null".to_string()),
            self.truncated,
            self.audit.json()
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluateResponse {
    pub identity: SessionIdentity,
    pub scope_id: String,
    pub expression_hash: String,
    pub mode: EvaluationMode,
    pub value: Option<ValueProjection>,
    pub changed: bool,
    pub observation: Option<ObservationFact>,
    pub truncated: bool,
    pub audit: AuditReceipt,
}

impl EvaluateResponse {
    pub fn json(&self) -> String {
        let value = self
            .value
            .as_ref()
            .and_then(ValueProjection::json)
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"protocol\":\"{}\",\"schema_version\":{},\"ok\":true,\"kind\":\"paused.evaluate\",\"identity\":{},\"scope_id\":\"{}\",\"expression_hash\":\"{}\",\"mode\":\"{}\",\"value\":{},\"changed\":{},\"observation\":{},\"truncated\":{},\"audit\":{}}}",
            PAUSED_EVALUATE_PROTOCOL,
            PAUSED_EVALUATE_SCHEMA_VERSION,
            self.identity.json(),
            json_escape(&self.scope_id),
            json_escape(&self.expression_hash),
            self.mode.as_str(),
            value,
            self.changed,
            self.observation
                .as_ref()
                .map(ObservationFact::json)
                .unwrap_or_else(|| "null".to_string()),
            self.truncated,
            self.audit.json()
        )
    }
}

/// Inspect one paused lexical scope.  The source session owns the facts; this
/// function only projects published and redacted values within the request
/// budget.
pub fn inspect(
    session: &PausedSession,
    request: &InspectRequest,
) -> Result<InspectResponse, RequestError> {
    let authority = inspect_authority(request);
    if let Err(reason) = validate_common(
        session,
        &request.identity,
        &request.request_id,
        &request.scope_id,
        &request.budget,
        &request.cancellation,
        &authority,
    ) {
        return Err(reject(
            "paused.inspect",
            request.request_id.as_str(),
            &request.identity,
            &request.scope_id,
            &authority,
            reason,
            EvaluationUsage::default(),
        ));
    }
    let Some(scope) = session.scope(&request.scope_id) else {
        return Err(reject(
            "paused.inspect",
            request.request_id.as_str(),
            &request.identity,
            &request.scope_id,
            &authority,
            RejectReason::ScopeNotFound,
            EvaluationUsage::default(),
        ));
    };
    if let Some(value_id) = request.value_id.as_deref() {
        if !scope.bindings.iter().any(|binding| binding.id == value_id) {
            return Err(reject(
                "paused.inspect",
                request.request_id.as_str(),
                &request.identity,
                &request.scope_id,
                &authority,
                RejectReason::ValueNotFound,
                EvaluationUsage::default(),
            ));
        }
        if scope
            .bindings
            .iter()
            .find(|binding| binding.id == value_id)
            .is_some_and(|binding| binding.publication == Publication::Absent)
        {
            return Err(reject(
                "paused.inspect",
                request.request_id.as_str(),
                &request.identity,
                &request.scope_id,
                &authority,
                RejectReason::ValueNotPublished,
                EvaluationUsage::default(),
            ));
        }
    }
    let Ok((projection, stats)) = ScopeProjection::project(
        scope,
        &session.identity,
        request.budget,
        request.value_id.as_deref(),
    ) else {
        return Err(reject(
            "paused.inspect",
            request.request_id.as_str(),
            &request.identity,
            &request.scope_id,
            &authority,
            RejectReason::InvalidEvaluatorResult,
            EvaluationUsage::default(),
        ));
    };
    let audit = AuditReceipt::new(
        "paused.inspect",
        &request.request_id,
        &request.identity,
        &request.scope_id,
        &authority,
        AuditOutcome::Accepted,
        None,
        stats.usage,
        stats.truncated,
        stats.published,
        stats.redacted,
        None,
    );
    Ok(InspectResponse {
        identity: request.identity.clone(),
        scope: projection,
        selected_value_id: request.value_id.clone(),
        truncated: stats.truncated,
        audit,
    })
}

/// Accept an injected paused-evaluator result.  This function does not execute
/// the requested expression or resume the target process.
pub fn evaluate(
    session: &PausedSession,
    request: &EvaluateRequest,
    result: EvaluatorResult,
) -> Result<EvaluateResponse, RequestError> {
    let authority = evaluate_authority(request);
    if let Err(reason) = validate_common(
        session,
        &request.identity,
        &request.request_id,
        &request.scope_id,
        &request.budget,
        &request.cancellation,
        &authority,
    ) {
        return Err(reject(
            "paused.evaluate",
            request.request_id.as_str(),
            &request.identity,
            &request.scope_id,
            &authority,
            reason,
            result.usage,
        ));
    }
    if result.identity != request.identity || result.scope_id != request.scope_id {
        return Err(reject(
            "paused.evaluate",
            request.request_id.as_str(),
            &request.identity,
            &request.scope_id,
            &authority,
            RejectReason::InvalidEvaluatorResult,
            result.usage,
        ));
    }
    let invalid_result_shape = match result.status {
        EvaluatorStatus::Completed => result.error.is_some(),
        EvaluatorStatus::Failed => {
            result.value.is_some()
                || result.changed
                || result.observation.is_some()
                || result.error.as_ref().is_none_or(|message| {
                    message.is_empty() || valid_text(message, "evaluator error", 4 * 1024).is_err()
                })
        }
        EvaluatorStatus::Cancelled => {
            result.value.is_some()
                || result.changed
                || result.observation.is_some()
                || result.error.is_some()
        }
    };
    if invalid_result_shape {
        return Err(reject(
            "paused.evaluate",
            request.request_id.as_str(),
            &request.identity,
            &request.scope_id,
            &authority,
            RejectReason::InvalidEvaluatorResult,
            result.usage,
        ));
    }
    if result.status == EvaluatorStatus::Failed {
        return Err(reject(
            "paused.evaluate",
            request.request_id.as_str(),
            &request.identity,
            &request.scope_id,
            &authority,
            RejectReason::EvaluatorFailed,
            result.usage,
        ));
    }
    if result.status == EvaluatorStatus::Cancelled {
        return Err(reject(
            "paused.evaluate",
            request.request_id.as_str(),
            &request.identity,
            &request.scope_id,
            &authority,
            RejectReason::Cancelled,
            result.usage,
        ));
    }
    if !request.budget.fits(result.usage) {
        return Err(reject(
            "paused.evaluate",
            request.request_id.as_str(),
            &request.identity,
            &request.scope_id,
            &authority,
            RejectReason::BudgetExceeded,
            result.usage,
        ));
    }
    if result.changed || result.observation.is_some() {
        if request.mode != EvaluationMode::Mutate {
            return Err(reject(
                "paused.evaluate",
                request.request_id.as_str(),
                &request.identity,
                &request.scope_id,
                &authority,
                RejectReason::MutationNotAllowed,
                result.usage,
            ));
        }
        if result.changed && result.observation.is_none() {
            return Err(reject(
                "paused.evaluate",
                request.request_id.as_str(),
                &request.identity,
                &request.scope_id,
                &authority,
                RejectReason::ObservationMissing,
                result.usage,
            ));
        }
        if !result.changed && result.observation.is_some() {
            return Err(reject(
                "paused.evaluate",
                request.request_id.as_str(),
                &request.identity,
                &request.scope_id,
                &authority,
                RejectReason::InvalidEvaluatorResult,
                result.usage,
            ));
        }
    }
    if let Some(observation) = result.observation.as_ref() {
        if !observation.matches(session, request) {
            return Err(reject(
                "paused.evaluate",
                request.request_id.as_str(),
                &request.identity,
                &request.scope_id,
                &authority,
                RejectReason::InvalidEvaluatorResult,
                result.usage,
            ));
        }
    }
    let mut stats = ProjectionStats::default();
    let value = match result.value.as_ref() {
        Some(value) => {
            if value.validate_for_session(&session.identity).is_err() {
                return Err(reject(
                    "paused.evaluate",
                    request.request_id.as_str(),
                    &request.identity,
                    &request.scope_id,
                    &authority,
                    RejectReason::InvalidEvaluatorResult,
                    result.usage,
                ));
            }
            if value.publication == Publication::Absent {
                None
            } else {
                if value.publication == Publication::Published {
                    stats.published = 1;
                } else {
                    stats.redacted = 1;
                }
                let projected =
                    project_root(value, &session.identity, request.budget, &mut stats)
                        .map_err(|_| RejectReason::InvalidEvaluatorResult)
                        .map_err(|reason| {
                            reject(
                                "paused.evaluate",
                                request.request_id.as_str(),
                                &request.identity,
                                &request.scope_id,
                                &authority,
                                reason,
                                result.usage,
                            )
                        })?;
                Some(projected)
            }
        }
        None => None,
    };
    let observation_id = result.observation.as_ref().map(|observation| observation.event_id.as_str());
    let audit = AuditReceipt::new(
        "paused.evaluate",
        &request.request_id,
        &request.identity,
        &request.scope_id,
        &authority,
        AuditOutcome::Accepted,
        None,
        result.usage,
        stats.truncated,
        stats.published,
        stats.redacted,
        observation_id,
    );
    Ok(EvaluateResponse {
        identity: request.identity.clone(),
        scope_id: request.scope_id.clone(),
        expression_hash: format!("sha256-{}", sha256_hex(request.expression.as_bytes())),
        mode: request.mode,
        value,
        changed: result.changed,
        observation: result.observation,
        truncated: stats.truncated,
        audit,
    })
}

/// Explicit failure to serve a `/paused/inspect` or `/paused/evaluate`
/// request from [`PausedEvaluateHost`]'s currently injected facts.  This is
/// distinct from [`RequestError`]: it covers the cases the kernel cannot
/// even be called for, because no real session or evaluator fact has been
/// injected yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PausedEvaluateHostError {
    /// No paused session has been injected, or it was cleared (resume,
    /// detach, process exit).  There is nothing to inspect or evaluate.
    SessionUnavailable,
    /// A session is injected, but the real evaluator has not supplied a
    /// result fact for this evaluate call.
    EvaluatorUnavailable,
    /// The request body was not valid JSON for the expected request shape.
    InvalidRequest(String),
    /// The kernel rejected the request against the injected facts.
    Rejected(RequestError),
}

impl PausedEvaluateHostError {
    /// Explicit bounded JSON error body; never a fabricated success value.
    pub fn json(&self) -> String {
        match self {
            Self::SessionUnavailable => format!(
                "{{\"protocol\":\"{}\",\"schema_version\":{},\"ok\":false,\"error\":\"session_unavailable\"}}",
                PAUSED_EVALUATE_PROTOCOL, PAUSED_EVALUATE_SCHEMA_VERSION
            ),
            Self::EvaluatorUnavailable => format!(
                "{{\"protocol\":\"{}\",\"schema_version\":{},\"ok\":false,\"error\":\"evaluator_unavailable\"}}",
                PAUSED_EVALUATE_PROTOCOL, PAUSED_EVALUATE_SCHEMA_VERSION
            ),
            Self::InvalidRequest(detail) => format!(
                "{{\"protocol\":\"{}\",\"schema_version\":{},\"ok\":false,\"error\":\"invalid_request\",\"detail\":\"{}\"}}",
                PAUSED_EVALUATE_PROTOCOL,
                PAUSED_EVALUATE_SCHEMA_VERSION,
                json_escape(detail)
            ),
            Self::Rejected(err) => err.json(),
        }
    }
}

impl fmt::Display for PausedEvaluateHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionUnavailable => formatter.write_str("session_unavailable"),
            Self::EvaluatorUnavailable => formatter.write_str("evaluator_unavailable"),
            Self::InvalidRequest(detail) => write!(formatter, "invalid_request: {detail}"),
            Self::Rejected(err) => fmt::Display::fmt(err, formatter),
        }
    }
}

impl std::error::Error for PausedEvaluateHostError {}

impl From<RequestError> for PausedEvaluateHostError {
    fn from(err: RequestError) -> Self {
        Self::Rejected(err)
    }
}

/// Host-owned state wrapper around the paused inspect/evaluate kernel.
///
/// A host stores exactly one instance per devtools connection and injects
/// the real paused session and the real evaluator's result as those facts
/// become available; it never runs an evaluator, never decides pause state
/// on its own, and never invents a value.  An absent session or an absent
/// evaluator result is reported as an explicit [`PausedEvaluateHostError`],
/// never silently substituted.  `handle_inspect` and `handle_evaluate` are
/// the only entry points a host needs to serve `/paused/inspect` and
/// `/paused/evaluate`: they parse the typed request, delegate to the
/// kernel, and return a bounded JSON body with no semantic logic left for
/// the caller.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PausedEvaluateHost {
    session: Option<PausedSession>,
    evaluator_result: Option<EvaluatorResult>,
}

impl PausedEvaluateHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inject the real paused session.  Replaces any session injected
    /// earlier; does not touch a separately staged evaluator result.
    pub fn set_session(&mut self, session: PausedSession) {
        self.session = Some(session);
    }

    /// Clear the paused session (resume, detach, or process exit).  Also
    /// drops any evaluator result staged for a request the session can no
    /// longer serve.
    pub fn clear_session(&mut self) {
        self.session = None;
        self.evaluator_result = None;
    }

    /// Mark the injected session cancelled in place, e.g. the host is
    /// stopping the debug target while a paused inspect/evaluate call may
    /// still be in flight.  Kept distinct from `clear_session`: the session
    /// facts (identity, scopes) stay retained and visible for one last
    /// `handle_inspect`, but the kernel now fails every further request
    /// with `RejectReason::Cancelled` per [`PausedSession::cancel`].
    /// Returns an explicit error if no session is injected, or if the
    /// reason text fails the kernel's own bounded-text validation.
    pub fn cancel_session(
        &mut self,
        reason: impl Into<String>,
    ) -> Result<(), PausedEvaluateHostError> {
        let Some(session) = self.session.as_mut() else {
            return Err(PausedEvaluateHostError::SessionUnavailable);
        };
        session.cancel(reason).map_err(PausedEvaluateHostError::InvalidRequest)
    }

    /// The currently injected session, if any.
    pub fn session(&self) -> Option<&PausedSession> {
        self.session.as_ref()
    }

    /// Whether the injected session is actually paused right now.
    pub fn is_paused(&self) -> bool {
        self.session.as_ref().is_some_and(|session| session.state == SessionState::Paused)
    }

    /// Inject the real evaluator's result fact ahead of the evaluate call it
    /// answers.  The evaluator computes this out of band; this wrapper
    /// never runs one itself.
    pub fn set_evaluator_result(&mut self, result: EvaluatorResult) {
        self.evaluator_result = Some(result);
    }

    /// Clear a staged evaluator result without consuming it, e.g. because
    /// the request it would have answered was cancelled.
    pub fn clear_evaluator_result(&mut self) {
        self.evaluator_result = None;
    }

    /// Whether an evaluator result is currently staged for the next
    /// evaluate call.
    pub fn has_evaluator_result(&self) -> bool {
        self.evaluator_result.is_some()
    }

    /// Handle one `/paused/inspect` request body against the injected
    /// session.  Returns the bounded JSON response on success, or an
    /// explicit JSON error; the body is a `String` either way so a host can
    /// write it back without further branching on success shape.
    pub fn handle_inspect(&self, raw: &str) -> Result<String, PausedEvaluateHostError> {
        self.inspect_request(raw).map(|response| response.json())
    }

    /// Handle one `/paused/inspect` request body and return the typed
    /// response for a host that needs the structured fields rather than
    /// serialized JSON.
    pub fn inspect_request(&self, raw: &str) -> Result<InspectResponse, PausedEvaluateHostError> {
        let Some(session) = self.session.as_ref() else {
            return Err(PausedEvaluateHostError::SessionUnavailable);
        };
        let request =
            InspectRequest::from_json(raw).map_err(PausedEvaluateHostError::InvalidRequest)?;
        Ok(inspect(session, &request)?)
    }

    /// Handle one `/paused/evaluate` request body against the injected
    /// session and the currently staged evaluator result, consuming the
    /// staged result.  Returns the bounded JSON response on success, or an
    /// explicit JSON error.
    pub fn handle_evaluate(&mut self, raw: &str) -> Result<String, PausedEvaluateHostError> {
        self.evaluate_request(raw).map(|response| response.json())
    }

    /// Handle one `/paused/evaluate` request body and return the typed
    /// response for a host that needs the structured fields rather than
    /// serialized JSON.
    pub fn evaluate_request(
        &mut self,
        raw: &str,
    ) -> Result<EvaluateResponse, PausedEvaluateHostError> {
        let Some(session) = self.session.as_ref() else {
            return Err(PausedEvaluateHostError::SessionUnavailable);
        };
        let request =
            EvaluateRequest::from_json(raw).map_err(PausedEvaluateHostError::InvalidRequest)?;
        let Some(result) = self.evaluator_result.take() else {
            return Err(PausedEvaluateHostError::EvaluatorUnavailable);
        };
        Ok(evaluate(session, &request, result)?)
    }
}

fn inspect_authority(request: &InspectRequest) -> Vec<AuthorityRequirement> {
    let mut authority = request.authority.clone();
    authority.push(AuthorityRequirement {
        capability: INSPECT_AUTHORITY.to_string(),
        scope_id: request.scope_id.clone(),
    });
    authority
}

fn evaluate_authority(request: &EvaluateRequest) -> Vec<AuthorityRequirement> {
    let mut authority = request.authority.clone();
    authority.push(AuthorityRequirement {
        capability: EVALUATE_AUTHORITY.to_string(),
        scope_id: request.scope_id.clone(),
    });
    if request.mode == EvaluationMode::Mutate {
        authority.push(AuthorityRequirement {
            capability: MUTATE_AUTHORITY.to_string(),
            scope_id: request.scope_id.clone(),
        });
        authority.extend(request.effects.iter().map(|effect| AuthorityRequirement {
            capability: effect.clone(),
            scope_id: request.scope_id.clone(),
        }));
    }
    authority.sort();
    authority.dedup();
    authority
}

fn validate_common(
    session: &PausedSession,
    identity: &SessionIdentity,
    request_id: &str,
    scope_id: &str,
    budget: &EvaluationBudget,
    cancellation: &Cancellation,
    authority: &[AuthorityRequirement],
) -> Result<(), RejectReason> {
    if session.state == SessionState::Stale {
        return Err(RejectReason::StaleSession);
    }
    if session.state != SessionState::Paused {
        return Err(if session.state == SessionState::Cancelled || cancellation.requested {
            RejectReason::Cancelled
        } else {
            RejectReason::SessionNotPaused
        });
    }
    if identity.validate().is_err()
        || valid_id(request_id, "request id").is_err()
        || valid_id(scope_id, "scope id").is_err()
        || budget.validate().is_err()
        || cancellation.validate().is_err()
        || authority.iter().any(|requirement| requirement.validate().is_err())
    {
        return Err(RejectReason::InvalidRequest);
    }
    if identity.session_id != session.identity.session_id {
        return Err(RejectReason::SessionIdentityMismatch);
    }
    if identity.source_id != session.identity.source_id
        || identity.source_revision != session.identity.source_revision
    {
        return Err(RejectReason::SourceIdentityMismatch);
    }
    if identity.pause_id != session.identity.pause_id {
        return Err(RejectReason::PauseIdentityMismatch);
    }
    if identity.frame_id != session.identity.frame_id {
        return Err(RejectReason::FrameIdentityMismatch);
    }
    if session.cancellation.requested || cancellation.requested {
        return Err(RejectReason::Cancelled);
    }
    if !session.budget.contains(budget) {
        return Err(RejectReason::BudgetExceeded);
    }
    if authority
        .iter()
        .any(|requirement| !session.authority.allows(requirement))
    {
        return Err(RejectReason::InsufficientAuthority);
    }
    Ok(())
}

fn reject(
    operation: &str,
    request_id: &str,
    identity: &SessionIdentity,
    scope_id: &str,
    authority: &[AuthorityRequirement],
    reason: RejectReason,
    usage: EvaluationUsage,
) -> RequestError {
    RequestError {
        reason,
        audit: AuditReceipt::new(
            operation,
            request_id,
            identity,
            scope_id,
            authority,
            AuditOutcome::Rejected,
            Some(reason),
            usage,
            false,
            0,
            0,
            None,
        ),
    }
}

fn project_root(
    value: &ValueProjection,
    identity: &SessionIdentity,
    budget: EvaluationBudget,
    stats: &mut ProjectionStats,
) -> Result<ValueProjection, String> {
    let mut nodes = stats.usage.nodes;
    let projected = project_node(value, identity, budget, 1, true, &mut nodes, stats)
        .ok_or_else(|| "paused value root could not be retained".to_string())?;
    stats.usage.nodes = nodes;
    Ok(projected)
}

fn project_node(
    value: &ValueProjection,
    _identity: &SessionIdentity,
    budget: EvaluationBudget,
    depth: u32,
    root: bool,
    nodes: &mut u64,
    stats: &mut ProjectionStats,
) -> Option<ValueProjection> {
    if value.publication == Publication::Absent {
        return None;
    }
    if !root && *nodes >= budget.max_nodes {
        stats.truncated = true;
        return None;
    }
    if *nodes < budget.max_nodes {
        *nodes = nodes.saturating_add(1);
    } else {
        // Keep a root node visible even when earlier siblings used the node
        // budget; the omitted children are represented by truncation.
        stats.truncated = true;
    }
    stats.usage.nodes = *nodes;
    stats.usage.depth = stats.usage.depth.max(depth);
    let metadata = value.metadata_bytes();
    let available = budget.max_bytes.saturating_sub(stats.usage.bytes);
    let metadata_fits = metadata <= available;
    if metadata_fits {
        stats.usage.bytes = stats.usage.bytes.saturating_add(metadata);
    } else {
        stats.usage.bytes = budget.max_bytes;
        stats.truncated = true;
    }
    let mut projected = value.clone();
    projected.value = None;
    projected.children.clear();
    projected.truncated = value.truncated || !metadata_fits;
    if value.publication == Publication::Published {
        if let Some(typed) = value.value.as_ref() {
            let payload = typed.bytes();
            let available = budget.max_bytes.saturating_sub(stats.usage.bytes);
            if metadata_fits && payload <= available {
                stats.usage.bytes = stats.usage.bytes.saturating_add(payload);
                projected.value = Some(typed.clone());
            } else {
                stats.truncated = true;
                projected.truncated = true;
            }
        }
    }
    if !value.children.is_empty() {
        if depth >= budget.max_depth {
            stats.truncated = true;
            projected.truncated = true;
        } else if !metadata_fits {
            stats.truncated = true;
            projected.truncated = true;
        } else {
            for child in &value.children {
                if let Some(child) = project_node(
                    child,
                    _identity,
                    budget,
                    depth.saturating_add(1),
                    false,
                    nodes,
                    stats,
                ) {
                    projected.children.push(child);
                } else {
                    stats.truncated = true;
                    projected.truncated = true;
                }
            }
        }
    }
    if value.publication == Publication::Published {
        let measured = value.measure();
        stats.usage.depth = stats.usage.depth.max(measured.depth.min(budget.max_depth));
    }
    Some(projected)
}

fn valid_id(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(format!("{label} is empty, too long, or contains control text"));
    }
    Ok(())
}

fn valid_text(value: &str, label: &str, max_bytes: usize) -> Result<(), String> {
    if value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(format!("{label} exceeds its bounded text contract"));
    }
    Ok(())
}

fn array_json(values: impl Iterator<Item = String>) -> String {
    format!("[{}]", values.collect::<Vec<_>>().join(","))
}

fn object(raw: &str) -> Result<std::collections::BTreeMap<String, DataTree>, String> {
    let root = parse_json_with_limit(raw, MAX_REQUEST_BYTES)
        .map_err(|_| "paused request is not valid bounded JSON".to_string())?;
    let DataTree::Object(fields) = root else {
        return Err("paused request must be a JSON object".to_string());
    };
    Ok(fields.into_iter().collect())
}

fn required_string(
    fields: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<String, String> {
    let value = fields
        .get(key)
        .and_then(|value| match value {
            DataTree::Text(value) => Some(value.clone()),
            _ => None,
        })
        .ok_or_else(|| format!("paused request requires string `{key}`"))?;
    valid_text(&value, key, 256)?;
    Ok(value)
}

fn optional_string(
    fields: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<Option<String>, String> {
    match fields.get(key) {
        None | Some(DataTree::Null) => Ok(None),
        Some(DataTree::Text(value)) => {
            valid_text(value, key, 256)?;
            Ok(Some(value.clone()))
        }
        Some(_) => Err(format!("paused request field `{key}` must be a string or null")),
    }
}

fn required_u64(
    fields: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<u64, String> {
    let value = fields
        .get(key)
        .and_then(json_int)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| format!("paused request requires non-negative integer `{key}`"))?;
    Ok(value)
}

fn required_u32(
    fields: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<u32, String> {
    let value = required_u64(fields, key)?;
    u32::try_from(value).map_err(|_| format!("paused request integer `{key}` is too large"))
}

fn optional_bool(
    fields: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<bool, String> {
    match fields.get(key) {
        None => Ok(false),
        Some(DataTree::Bool(value)) => Ok(*value),
        Some(_) => Err(format!("paused request field `{key}` must be boolean")),
    }
}

fn parse_identity(
    fields: &std::collections::BTreeMap<String, DataTree>,
) -> Result<SessionIdentity, String> {
    let identity: std::collections::BTreeMap<String, DataTree> = match fields.get("identity") {
        Some(DataTree::Object(identity)) => identity.iter().cloned().collect(),
        Some(_) => return Err("paused request `identity` must be an object".to_string()),
        None => fields.clone(),
    };
    SessionIdentity::new(
        required_string(&identity, "session_id")?,
        required_string(&identity, "source_id")?,
        required_string(&identity, "source_revision")?,
        required_string(&identity, "pause_id")?,
        required_string(&identity, "frame_id")?,
    )
}

fn parse_budget(
    fields: &std::collections::BTreeMap<String, DataTree>,
 ) -> Result<EvaluationBudget, String> {
    let Some(DataTree::Object(budget)) = fields.get("budget") else {
        return Ok(DEFAULT_EVALUATION_BUDGET);
    };
    let budget: std::collections::BTreeMap<String, DataTree> =
        budget.iter().cloned().collect();
    EvaluationBudget::new(
        required_u32(&budget, "max_depth")?,
        required_u64(&budget, "max_nodes")?,
        required_u64(&budget, "max_bytes")?,
        required_u64(&budget, "max_steps")?,
    )
}

fn parse_authority(
    fields: &std::collections::BTreeMap<String, DataTree>,
) -> Result<Vec<AuthorityRequirement>, String> {
    let Some(value) = fields.get("authority") else {
        return Ok(Vec::new());
    };
    let DataTree::Array(values) = value else {
        return Err("paused request `authority` must be an array".to_string());
    };
    values
        .iter()
        .map(|value| {
            let DataTree::Object(fields) = value else {
                return Err("paused authority requirement must be an object".to_string());
            };
            let fields: std::collections::BTreeMap<String, DataTree> =
                fields.iter().cloned().collect();
            AuthorityRequirement::new(
                required_string(&fields, "capability")?,
                required_string(&fields, "scope_id")?,
            )
        })
        .collect()
}

fn validate_header(
    fields: &std::collections::BTreeMap<String, DataTree>,
    kind: &str,
) -> Result<(), String> {
    if required_string(fields, "protocol")? != PAUSED_EVALUATE_PROTOCOL {
        return Err("paused request protocol does not match jet.devtools.v1".to_string());
    }
    let schema = fields
        .get("schema_version")
        .and_then(json_int)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| "paused request schema_version is missing or invalid".to_string())?;
    if schema != PAUSED_EVALUATE_SCHEMA_VERSION {
        return Err("paused request schema_version is unsupported".to_string());
    }
    if required_string(fields, "kind")? != kind {
        return Err(format!("paused request kind must be `{kind}`"));
    }
    Ok(())
}

pub fn parse_inspect_request(raw: &str) -> Result<InspectRequest, String> {
    let fields = object(raw)?;
    validate_header(&fields, "paused.inspect")?;
    let mut request = InspectRequest::new(
        required_string(&fields, "request_id")?,
        parse_identity(&fields)?,
        required_string(&fields, "scope_id")?,
        parse_budget(&fields)?,
        parse_authority(&fields)?,
    )?;
    request.value_id = optional_string(&fields, "value_id")?;
    request.cancellation = if optional_bool(&fields, "cancelled")? {
        Cancellation::requested("request cancelled")?
    } else {
        Cancellation::none()
    };
    request.validate_shape()?;
    Ok(request)
}

pub fn parse_evaluate_request(raw: &str) -> Result<EvaluateRequest, String> {
    let fields = object(raw)?;
    validate_header(&fields, "paused.evaluate")?;
    let mode = match required_string(&fields, "mode")?.as_str() {
        "read_only" => EvaluationMode::ReadOnly,
        "mutate" => EvaluationMode::Mutate,
        _ => return Err("paused evaluation mode is unsupported".to_string()),
    };
    let effects = match fields.get("effects") {
        None => Vec::new(),
        Some(DataTree::Array(values)) => values
            .iter()
            .map(|value| match value {
                DataTree::Text(value) => {
                    valid_id(value, "evaluation effect")?;
                    Ok(value.clone())
                }
                _ => Err("paused evaluation effects must be strings".to_string()),
            })
            .collect::<Result<Vec<_>, String>>()?,
        Some(_) => return Err("paused evaluation `effects` must be an array".to_string()),
    };
    let mut request = EvaluateRequest::new(
        required_string(&fields, "request_id")?,
        parse_identity(&fields)?,
        required_string(&fields, "scope_id")?,
        required_string(&fields, "expression")?,
        mode,
        effects,
        parse_authority(&fields)?,
        parse_budget(&fields)?,
    )?;
    request.cancellation = if optional_bool(&fields, "cancelled")? {
        Cancellation::requested("request cancelled")?
    } else {
        Cancellation::none()
    };
    request.validate_shape()?;
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> SessionIdentity {
        SessionIdentity::new("session-1", "src/main.jet", "rev-1", "pause-1", "frame-1")
            .unwrap()
    }

    fn source() -> SourceLocation {
        SourceLocation::new("src/main.jet", "rev-1", 0, 10).unwrap()
    }

    fn scope() -> LexicalScope {
        let source = source();
        LexicalScope::new(
            "scope-1",
            "run",
            None,
            source.clone(),
            vec![
                ValueProjection::published(
                    "value-public",
                    "count",
                    "Int",
                    source.clone(),
                    TypedValue::Int(7),
                ),
                ValueProjection::redacted(
                    "value-secret",
                    "token",
                    "String",
                    source.clone(),
                ),
                ValueProjection::absent(
                    "value-hidden",
                    "password",
                    "String",
                    source,
                ),
            ],
        )
        .unwrap()
    }

    fn authority() -> AuthoritySet {
        AuthoritySet::new(vec![
            AuthorityGrant::new(INSPECT_AUTHORITY, "scope-1").unwrap(),
            AuthorityGrant::new(EVALUATE_AUTHORITY, "scope-1").unwrap(),
            AuthorityGrant::new(MUTATE_AUTHORITY, "scope-1").unwrap(),
            AuthorityGrant::new("fs.write", "scope-1").unwrap(),
        ])
    }

    fn session() -> PausedSession {
        PausedSession::paused(
            identity(),
            vec![scope()],
            authority(),
            DEFAULT_EVALUATION_BUDGET,
        )
        .unwrap()
    }

    #[test]
    fn inspect_is_typed_and_omits_unpublished_values() {
        let session = session();
        let request = InspectRequest::new(
            "request-1",
            identity(),
            "scope-1",
            DEFAULT_EVALUATION_BUDGET,
            Vec::new(),
        )
        .unwrap();
        let response = inspect(&session, &request).unwrap();
        let json = response.json();
        assert!(json.contains("\"kind\":\"int\""));
        assert!(json.contains("\"publication\":\"redacted\""));
        assert!(!json.contains("password"));
        assert!(json.contains("\"name\":\"token\""));
        assert_eq!(response.audit.outcome, AuditOutcome::Accepted);
    }

    #[test]
    fn stale_running_and_wrong_source_fail_closed() {
        let mut stale = session();
        stale.state = SessionState::Stale;
        let request = InspectRequest::new(
            "request-2",
            identity(),
            "scope-1",
            DEFAULT_EVALUATION_BUDGET,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(inspect(&stale, &request).unwrap_err().reason, RejectReason::StaleSession);

        let mut running = session();
        running.state = SessionState::Running;
        assert_eq!(inspect(&running, &request).unwrap_err().reason, RejectReason::SessionNotPaused);

        let wrong = SessionIdentity::new("session-1", "src/other.jet", "rev-1", "pause-1", "frame-1")
            .unwrap();
        let request = InspectRequest::new(
            "request-3",
            wrong,
            "scope-1",
            DEFAULT_EVALUATION_BUDGET,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(inspect(&session(), &request).unwrap_err().reason, RejectReason::SourceIdentityMismatch);
    }

    #[test]
    fn evaluator_result_is_injected_and_mutation_needs_authority_and_observation() {
        let session = session();
        let request = EvaluateRequest::new(
            "request-4",
            identity(),
            "scope-1",
            "count + 1",
            EvaluationMode::ReadOnly,
            Vec::new(),
            Vec::new(),
            DEFAULT_EVALUATION_BUDGET,
        )
        .unwrap();
        let source = source();
        let value = ValueProjection::published(
            "eval-value",
            "result",
            "Int",
            source,
            TypedValue::Int(8),
        );
        let response = evaluate(
            &session,
            &request,
            EvaluatorResult::completed(
                identity(),
                "scope-1",
                Some(value),
                EvaluationUsage::new(1, 1, 64, 2),
            ),
        )
        .unwrap();
        assert_eq!(response.value.unwrap().value, Some(TypedValue::Int(8)));

        let request = EvaluateRequest::new(
            "request-5",
            identity(),
            "scope-1",
            "count = 8",
            EvaluationMode::Mutate,
            vec!["fs.write".to_string()],
            Vec::new(),
            DEFAULT_EVALUATION_BUDGET,
        )
        .unwrap();
        let observation = ObservationFact::new(
            "event-1",
            "paused.update",
            "session-1",
            "src/main.jet",
            "rev-1",
            "scope-1",
            "value-public",
            "fs.write",
        )
        .unwrap();
        let result = EvaluatorResult::mutated(
            identity(),
            "scope-1",
            None,
            EvaluationUsage::new(1, 1, 32, 2),
            observation,
        );
        assert!(evaluate(&session, &request, result).is_ok());

        let mut denied = request.clone();
        denied.authority.push(AuthorityRequirement::new("fs.read", "scope-1").unwrap());
        assert_eq!(evaluate(&session, &denied, EvaluatorResult::completed(identity(), "scope-1", None, EvaluationUsage::new(1, 0, 0, 1))).unwrap_err().reason, RejectReason::InsufficientAuthority);
    }

    #[test]
    fn audit_receipts_are_deterministic() {
        let session = session();
        let request = InspectRequest::new(
            "request-6",
            identity(),
            "scope-1",
            DEFAULT_EVALUATION_BUDGET,
            Vec::new(),
        )
        .unwrap();
        let first = inspect(&session, &request).unwrap();
        let second = inspect(&session, &request).unwrap();
        assert_eq!(first.audit, second.audit);
        assert_eq!(first.audit.receipt_id, "paused-audit-".to_string() + &first.audit.receipt_id[13..]);
    }

    #[test]
    fn cancellation_budget_and_absent_result_are_fail_closed() {
        let session = session();
        let mut inspect_request = InspectRequest::new(
            "request-cancelled",
            identity(),
            "scope-1",
            DEFAULT_EVALUATION_BUDGET,
            Vec::new(),
        )
        .unwrap();
        inspect_request.cancellation = Cancellation::requested("user stopped").unwrap();
        assert_eq!(
            inspect(&session, &inspect_request).unwrap_err().reason,
            RejectReason::Cancelled
        );

        let budget = EvaluationBudget::new(4, 16, 4096, 1).unwrap();
        let evaluate_request = EvaluateRequest::new(
            "request-budget",
            identity(),
            "scope-1",
            "count",
            EvaluationMode::ReadOnly,
            Vec::new(),
            Vec::new(),
            budget,
        )
        .unwrap();
        let over_budget = EvaluatorResult::completed(
            identity(),
            "scope-1",
            None,
            EvaluationUsage::new(1, 1, 1, 2),
        );
        assert_eq!(
            evaluate(&session, &evaluate_request, over_budget)
                .unwrap_err()
                .reason,
            RejectReason::BudgetExceeded
        );

        let absent = ValueProjection::absent(
            "eval-hidden",
            "secret",
            "String",
            source(),
        );
        let response = evaluate(
            &session,
            &evaluate_request,
            EvaluatorResult::completed(
                identity(),
                "scope-1",
                Some(absent),
                EvaluationUsage::new(1, 1, 1, 1),
            ),
        )
        .unwrap();
        assert!(response.value.is_none());
        assert!(!response.json().contains("secret"));
    }

    #[test]
    fn request_json_round_trips() {
        let request = EvaluateRequest::new(
            "request-7",
            identity(),
            "scope-1",
            "count",
            EvaluationMode::ReadOnly,
            Vec::new(),
            Vec::new(),
            DEFAULT_EVALUATION_BUDGET,
        )
        .unwrap();
        let parsed = EvaluateRequest::from_json(&request.json()).unwrap();
        assert_eq!(parsed, request);
    }
}
