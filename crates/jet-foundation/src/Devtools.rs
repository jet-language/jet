// D-DX-DEVTOOLS1 / #2429: one typed observation protocol for every
// devtools host. Host adapters marshal facts into these values; they do not
// define another envelope, event vocabulary, or time-cursor rule. This part
// is std-only because AOT, JIT, interpreter, and web runtimes embed it.

/// Stable protocol identity and its single version fact.
pub const JET_DEVTOOLS_PROTOCOL: &str = "jet.devtools.v1";
pub const JET_DEVTOOLS_VERSION: u32 = 1;

/// Devtools is a development observation surface, not an application data
/// channel. These defaults are policy, not host hints: a host may marshal the
/// same envelope, but must not widen the limits or privacy defaults silently.
pub const JET_DEVTOOLS_LOOPBACK_ONLY: bool = true;
pub const JET_DEVTOOLS_PAYLOAD_FREE_BY_DEFAULT: bool = true;
pub const JET_DEVTOOLS_MAX_EVENTS: usize = 256;
pub const JET_DEVTOOLS_MAX_HISTORY: usize = 256;
pub const JET_DEVTOOLS_MAX_TEXT_BYTES: usize = 16 * 1024;
pub const JET_DEVTOOLS_MAX_ENVELOPE_BYTES: usize = 1024 * 1024;

pub const fn jet_devtools_protocol() -> &'static str {
    JET_DEVTOOLS_PROTOCOL
}

pub const fn jet_devtools_version() -> u32 {
    JET_DEVTOOLS_VERSION
}
/// Lifecycle states are shared by every devtools host.  `Stopped` is distinct
/// from `Unavailable`: the former is an intentional session shutdown while
/// the latter means no observation source is available.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsLifecycleState {
    Starting,
    Building,
    Ready,
    Error,
    Unavailable,
    Stopped,
}

impl JetDevtoolsLifecycleState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Building => "building",
            Self::Ready => "ready",
            Self::Error => "error",
            Self::Unavailable => "unavailable",
            Self::Stopped => "stopped",
        }
    }
}

/// Freshness is a fact on the canonical envelope, not a host-local timestamp
/// heuristic.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsFreshnessState {
    Fresh,
    Stale,
    Unknown,
}

impl JetDevtoolsFreshnessState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetDevtoolsFreshnessFact {
    pub state: JetDevtoolsFreshnessState,
    pub observed_at_ms: u64,
}

impl JetDevtoolsFreshnessFact {
    pub const fn fresh(observed_at_ms: u64) -> Self {
        Self {
            state: JetDevtoolsFreshnessState::Fresh,
            observed_at_ms,
        }
    }

    pub const fn stale(observed_at_ms: u64) -> Self {
        Self {
            state: JetDevtoolsFreshnessState::Stale,
            observed_at_ms,
        }
    }

    pub const fn unknown(observed_at_ms: u64) -> Self {
        Self {
            state: JetDevtoolsFreshnessState::Unknown,
            observed_at_ms,
        }
    }
}

impl Default for JetDevtoolsFreshnessFact {
    fn default() -> Self {
        Self::unknown(0)
    }
}

/// Privacy policy is carried with the envelope so hosts cannot silently widen
/// the observation surface while reconnecting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetDevtoolsPrivacy {
    pub loopback_only: bool,
    pub payload_free_by_default: bool,
    pub published_payloads: bool,
}

impl JetDevtoolsPrivacy {
    pub const fn from_policy(policy: JetDevtoolsPolicy) -> Self {
        Self {
            loopback_only: policy.loopback_only,
            payload_free_by_default: policy.payload_free_by_default,
            published_payloads: false,
        }
    }
}

impl Default for JetDevtoolsPrivacy {
    fn default() -> Self {
        Self::from_policy(JetDevtoolsPolicy::development())
    }
}

/// Source/build identity is retained independently from event text.  Empty
/// optional components mean that the producer has not published that fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsSourceIdentityFact {
    pub source_id: Option<String>,
    pub build_id: Option<String>,
    pub revision: Option<String>,
    pub world_id: Option<String>,
}

impl JetDevtoolsSourceIdentityFact {
    pub fn new(
        source_id: Option<String>,
        build_id: Option<String>,
        revision: Option<String>,
        world_id: Option<String>,
    ) -> Self {
        Self {
            source_id,
            build_id,
            revision,
            world_id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for (value, field) in [
            (self.source_id.as_deref(), "source_id"),
            (self.build_id.as_deref(), "build_id"),
            (self.revision.as_deref(), "revision"),
            (self.world_id.as_deref(), "world_id"),
        ] {
            if let Some(value) = value {
                validate_text(value, field)?;
            }
        }
        Ok(())
    }
}

impl Default for JetDevtoolsSourceIdentityFact {
    fn default() -> Self {
        Self::new(None, None, None, None)
    }
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetDevtoolsPolicy {
    pub loopback_only: bool,
    pub payload_free_by_default: bool,
    pub max_events: usize,
    pub max_history: usize,
    pub max_text_bytes: usize,
    pub max_envelope_bytes: usize,
}

impl JetDevtoolsPolicy {
    pub const fn development() -> Self {
        Self {
            loopback_only: JET_DEVTOOLS_LOOPBACK_ONLY,
            payload_free_by_default: JET_DEVTOOLS_PAYLOAD_FREE_BY_DEFAULT,
            max_events: JET_DEVTOOLS_MAX_EVENTS,
            max_history: JET_DEVTOOLS_MAX_HISTORY,
            max_text_bytes: JET_DEVTOOLS_MAX_TEXT_BYTES,
            max_envelope_bytes: JET_DEVTOOLS_MAX_ENVELOPE_BYTES,
        }
    }
}

impl Default for JetDevtoolsPolicy {
    fn default() -> Self {
        Self::development()
    }
}
pub const JET_DEVTOOLS_MAX_LIVE_VALUES: usize = 4096;

/// One checked, bounded live value update.  The producer supplies only the
/// canonical slot key, type identity, and optional display text; this row is
/// the Foundation-owned disposition/redaction result sent to host sinks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetLiveValueDecision {
    pub value_id: String,
    pub type_identity: String,
    pub disposition: String,
    pub reason: String,
    pub rendered_value: Option<String>,
}

pub type JetLiveValueUpdateSink = fn(JetLiveValueDecision);

struct JetLiveValueSinkRegistration {
    id: u64,
    sink: JetLiveValueUpdateSink,
}

struct JetLiveValueSinkRegistry {
    next_id: u64,
    sinks: Vec<JetLiveValueSinkRegistration>,
}

impl Default for JetLiveValueSinkRegistry {
    fn default() -> Self {
        Self {
            next_id: 1,
            sinks: Vec::new(),
        }
    }
}

pub struct JetLiveValueSinkGuard {
    id: u64,
}

impl Drop for JetLiveValueSinkGuard {
    fn drop(&mut self) {
        if self.id == 0 {
            return;
        }
        if let Ok(mut registry) = JET_LIVE_VALUE_SINKS.lock() {
            registry.sinks.retain(|registration| registration.id != self.id);
        }
    }
}

static JET_LIVE_VALUE_SINKS: std::sync::LazyLock<
    std::sync::Mutex<JetLiveValueSinkRegistry>,
> = std::sync::LazyLock::new(|| {
    std::sync::Mutex::new(JetLiveValueSinkRegistry::default())
});

pub fn jet_devtools_install_live_value_sink(
    sink: JetLiveValueUpdateSink,
) -> JetLiveValueSinkGuard {
    let mut registry = JET_LIVE_VALUE_SINKS
        .lock()
        .expect("live value sink registry poisoned");
    let id = registry.next_id;
    registry.next_id = registry.next_id.saturating_add(1).max(1);
    registry
        .sinks
        .push(JetLiveValueSinkRegistration { id, sink });
    JetLiveValueSinkGuard { id }
}

fn jet_live_value_text_valid(value: &str) -> bool {
    value.len() <= JET_DEVTOOLS_MAX_TEXT_BYTES
        && !value.chars().any(char::is_control)
}

fn jet_live_value_identity_valid(value: &str) -> bool {
    !value.is_empty() && jet_live_value_text_valid(value)
}

fn jet_live_value_sensitive(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("credential") || value.contains("secret") || value.contains("pii")
}

/// Apply the one Foundation live-value policy and fan the resulting typed
/// decision out to the installed host callbacks.  Producers must not construct
/// a disposition, reason, or payload of their own.
pub fn jet_observe_live_value_update(
    key: &str,
    type_identity: &str,
    rendered: Option<&str>,
) {
    if !jet_live_value_identity_valid(key) || !jet_live_value_identity_valid(type_identity) {
        return;
    }
    let (rendered_value, reason) = match rendered {
        None => (None, "runtime assignment"),
        Some(value) if !jet_live_value_text_valid(value) => {
            (None, "rendered value rejected by policy")
        }
        Some(_)
            if jet_live_value_sensitive(key) || jet_live_value_sensitive(type_identity) =>
        {
            (None, "rendered value redacted by policy")
        }
        Some(value) => (Some(value.to_string()), "runtime assignment"),
    };
    let decision = JetLiveValueDecision {
        value_id: key.to_string(),
        type_identity: type_identity.to_string(),
        disposition: "kept".to_string(),
        reason: reason.to_string(),
        rendered_value,
    };
    let sinks = {
        let Ok(registry) = JET_LIVE_VALUE_SINKS.lock() else {
            return;
        };
        registry
            .sinks
            .iter()
            .map(|registration| registration.sink)
            .collect::<Vec<_>>()
    };
    for sink in sinks {
        sink(decision.clone());
    }
}


#[derive(Clone, Debug, PartialEq)]
pub enum JetDevtoolsValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

impl JetDevtoolsValue {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Float(value) if !value.is_finite() => {
                Err("jet.devtools.v1 payload float must be finite".to_string())
            }
            Self::Text(value) if value.len() > JET_DEVTOOLS_MAX_TEXT_BYTES => Err(
                "jet.devtools.v1 payload text exceeds the Prelude limit".to_string(),
            ),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsField {
    pub key: String,
    pub value: JetDevtoolsValue,
}

impl JetDevtoolsField {
    pub fn new(key: impl Into<String>, value: JetDevtoolsValue) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }
}

/// Values are absent unless an observation site explicitly calls `publish` on
/// an event. Metadata in event bodies remains typed and payload-free.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsPayload {
    fields: Vec<JetDevtoolsField>,
}

impl JetDevtoolsPayload {
    pub fn new(fields: Vec<JetDevtoolsField>) -> Self {
        Self { fields }
    }

    pub fn one(key: impl Into<String>, value: JetDevtoolsValue) -> Self {
        Self::new(vec![JetDevtoolsField::new(key, value)])
    }

    pub fn fields(&self) -> &[JetDevtoolsField] {
        &self.fields
    }

    fn validate(&self) -> Result<(), String> {
        if self.fields.len() > JET_DEVTOOLS_MAX_HISTORY {
            return Err("jet.devtools.v1 payload has too many fields".to_string());
        }
        for (index, field) in self.fields.iter().enumerate() {
            if field.key.is_empty() {
                return Err(format!(
                    "jet.devtools.v1 payload field {index} has an empty key"
                ));
            }
            if field.key.len() > JET_DEVTOOLS_MAX_TEXT_BYTES {
                return Err("jet.devtools.v1 payload field key exceeds the Prelude limit".to_string());
            }
            field.value.validate()?;
            if self.fields[index + 1..]
                .iter()
                .any(|other| other.key == field.key)
            {
                return Err(format!(
                    "jet.devtools.v1 payload contains duplicate field `{}`",
                    field.key
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDevtoolsQueryState {
    Fetching,
    Fresh,
    Stale,
    Invalidated,
}

impl JetDevtoolsQueryState {
    fn wire(self) -> &'static str {
        match self {
            Self::Fetching => "fetching",
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Invalidated => "invalidated",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDevtoolsMutationLifecycle {
    Started,
    Committed,
    Failed,
    RolledBack,
}

impl JetDevtoolsMutationLifecycle {
    fn wire(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Committed => "committed",
            Self::Failed => "failed",
            Self::RolledBack => "rolled_back",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDevtoolsFormState {
    Pristine,
    Dirty,
    Valid,
    Invalid,
}

impl JetDevtoolsFormState {
    fn wire(self) -> &'static str {
        match self {
            Self::Pristine => "pristine",
            Self::Dirty => "dirty",
            Self::Valid => "valid",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDevtoolsTraceKind {
    Request,
    Render,
    Job,
    Reload,
    Frame,
}

impl JetDevtoolsTraceKind {
    fn wire(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Render => "render",
            Self::Job => "job",
            Self::Reload => "reload",
            Self::Frame => "frame",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsSourceSpan {
    pub source_id: String,
    pub file: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl JetDevtoolsSourceSpan {
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

    pub fn validate(&self) -> Result<(), String> {
        for (value, label) in [
            (&self.source_id, "source id"),
            (&self.file, "source file"),
        ] {
            validate_text(value, label)?;
        }
        if self.start_line == 0
            || self.start_column == 0
            || self.end_line == 0
            || self.end_column == 0
        {
            return Err("jet.devtools.v1 source span starts or ends at zero".to_string());
        }
        if (self.end_line, self.end_column) < (self.start_line, self.start_column) {
            return Err("jet.devtools.v1 source span ends before it starts".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsFunctionIdentity {
    pub function_id: String,
    pub name: String,
    pub source: JetDevtoolsSourceSpan,
}

impl JetDevtoolsFunctionIdentity {
    pub fn new(
        function_id: impl Into<String>,
        name: impl Into<String>,
        source: JetDevtoolsSourceSpan,
    ) -> Self {
        Self {
            function_id: function_id.into(),
            name: name.into(),
            source,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_text(&self.function_id, "function id")?;
        validate_text(&self.name, "function name")?;
        self.source.validate()
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsGameLaunchPhase {
    pub phase: String,
    pub status: String,
    pub detail: String,
}

impl JetDevtoolsGameLaunchPhase {
    pub fn new(
        phase: impl Into<String>,
        status: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            phase: phase.into(),
            status: status.into(),
            detail: detail.into(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        validate_text(&self.phase, "game launch phase")?;
        validate_text(&self.status, "game launch phase status")?;
        validate_text(&self.detail, "game launch phase detail")
    }
}

/// Closed first-party event bodies. Package-specific event payloads use the
/// `from_parts_with_payload` marshalling seam instead of widening this core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetDevtoolsEventBody {
    Build {
        status: String,
        tier: String,
        diagnostics: Vec<String>,
        kept: u64,
        reset: u64,
    },
    Route {
        path: String,
        matched: bool,
        render_mode: String,
        reason: String,
    },
    Query {
        name: String,
        state: JetDevtoolsQueryState,
        footprint: u64,
    },
    Mutation {
        name: String,
        lifecycle: JetDevtoolsMutationLifecycle,
        rollback: bool,
    },
    Form {
        name: String,
        field: String,
        state: JetDevtoolsFormState,
        validation: String,
    },
    Table {
        name: String,
        viewport: u64,
        sort: String,
        filter: String,
    },
    Store {
        name: String,
        transaction: String,
        diff: String,
        cursor: u64,
    },
    Trace {
        name: String,
        kind: JetDevtoolsTraceKind,
        request: String,
        duration_ms: u64,
        status: String,
    },
    Cost {
        name: String,
        rule: String,
        bytes: u64,
        allocations: u64,
    },
    Gate {
        name: String,
        status: String,
        detail: String,
    },
    Structure {
        name: String,
        kind: String,
        status: String,
    },
    Test {
        name: String,
        status: String,
        duration_ms: u64,
        diagnostics: String,
    },
    Job {
        name: String,
        status: String,
        duration_ms: u64,
        diagnostics: String,
    },
    Frame {
        name: String,
        status: String,
        duration_ms: u64,
    },
    GameFrameSample {
        sequence: u64,
        frame_id: u64,
        scene: String,
        frame_index: u64,
        build: String,
        revision: String,
        trace_id: String,
        start_ns: u64,
        cpu_ns: u64,
        gpu_ns: Option<u64>,
        responsible_function: Option<JetDevtoolsFunctionIdentity>,
    },
    GameDrawEvent {
        sequence: u64,
        frame_id: u64,
        scene: String,
        frame_index: u64,
        build: String,
        revision: String,
        trace_id: String,
        event_index: u64,
        event_id: u64,
        function: JetDevtoolsFunctionIdentity,
        phase: String,
        domain: String,
        start_ns: u64,
        duration_ns: u64,
        object_id: Option<String>,
        resource_id: Option<String>,
    },
    GameLaunchProfile {
        target: String,
        run_mode: String,
        headless_compatible: bool,
        phases: Vec<JetDevtoolsGameLaunchPhase>,
    },
    GameSwap {
        sequence: u64,
        path: String,
        kind: String,
        status: String,
        old_schema_id: Option<String>,
        new_schema_id: Option<String>,
        migration: String,
        reason: String,
    },
    Request {
        id: String,
        method: String,
        path: String,
        status: u16,
    },
    Response {
        id: String,
        status: u16,
        bytes: u64,
    },
}

impl JetDevtoolsEventBody {
    fn family(&self) -> &'static str {
        match self {
            Self::Build { .. } => "Build",
            Self::Route { .. } => "Route",
            Self::Query { .. } => "Query",
            Self::Mutation { .. } => "Mutation",
            Self::Form { .. } => "Form",
            Self::Table { .. } => "Table",
            Self::Store { .. } => "Store",
            Self::Trace { .. } => "Trace",
            Self::Cost { .. } => "Cost",
            Self::Gate { .. } => "Gate",
            Self::Structure { .. } => "Structure",
            Self::Test { .. } => "Test",
            Self::Job { .. } => "Job",
            Self::Frame { .. } => "Frame",
            Self::GameFrameSample { .. } => "GameFrameSample",
            Self::GameDrawEvent { .. } => "GameDrawEvent",
            Self::GameLaunchProfile { .. } => "GameLaunchProfile",
            Self::GameSwap { .. } => "GameSwap",
            Self::Request { .. } => "Request",
            Self::Response { .. } => "Response",
        }
    }

    fn entity(&self) -> &str {
        match self {
            Self::Build { .. } => "build",
            Self::Route { path, .. } => path,
            Self::Query { name, .. }
            | Self::Mutation { name, .. }
            | Self::Table { name, .. }
            | Self::Trace { name, .. }
            | Self::Cost { name, .. }
            | Self::Gate { name, .. }
            | Self::Structure { name, .. }
            | Self::Test { name, .. }
            | Self::Job { name, .. }
            | Self::Frame { name, .. } => name,
            Self::GameFrameSample { scene, .. } | Self::GameDrawEvent { scene, .. } => scene,
            Self::GameLaunchProfile { target, .. } => target,
            Self::GameSwap { path, .. } => path,
            Self::Form { name, .. } => name,
            Self::Store { name, .. } => name,
            Self::Request { id, .. } | Self::Response { id, .. } => id,
        }
    }
}

/// One canonical event record. The wire shape is deliberately flat so every
/// host can project the same `kind`/`entity`/`fields`/`payload` record while
/// producers and in-process consumers retain the typed body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevtoolsEvent {
    sequence: u64,
    pub timestamp_ms: u64,
    pub source: String,
    pub kind: String,
    pub entity: String,
    fields: String,
    payload: Option<String>,
    body: Option<JetDevtoolsEventBody>,
}

impl JetDevtoolsEvent {
    pub fn try_new(
        timestamp_ms: u64,
        source: impl Into<String>,
        body: JetDevtoolsEventBody,
    ) -> Result<Self, String> {
        let kind = body.family().to_string();
        let entity = body.entity().to_string();
        let fields = render_body_fields(&body)?;
        let mut event = Self::from_parts(timestamp_ms, source, kind, entity, fields)?;
        event.body = Some(body);
        Ok(event)
    }

    /// Reconstruct a typed event from its flat wire projection.
    ///
    /// The supplied fields must be the canonical rendering of `body`; this
    /// keeps wire compatibility while rejecting a body/identity mismatch.
    pub fn from_wire(
        sequence: u64,
        timestamp_ms: u64,
        source: impl Into<String>,
        body: JetDevtoolsEventBody,
        fields_json: impl Into<String>,
        payload_json: Option<String>,
    ) -> Result<Self, String> {
        if sequence == 0 {
            return Err("jet.devtools.v1 event sequence must be positive".to_string());
        }
        let fields = fields_json.into();
        let mut event = Self::try_new(timestamp_ms, source, body)?;
        validate_json_object(&fields, "fields")?;
        event.fields = fields;
        if let Some(payload) = payload_json.as_deref() {
            validate_json_object(payload, "payload")?;
        }
        event.sequence = sequence;
        event.payload = payload_json;
        Ok(event)
    }


    /// Construct a host-marshalled event. `fields_json` must be one JSON
    /// object; values are attached only through the typed `publish` gate.
    pub fn from_parts(
        timestamp_ms: u64,
        source: impl Into<String>,
        kind: impl Into<String>,
        entity: impl Into<String>,
        fields_json: impl Into<String>,
    ) -> Result<Self, String> {
        Self::from_parts_with_payload(
            timestamp_ms,
            source,
            kind,
            entity,
            fields_json,
            None,
        )
    }

    /// Game and package adapters use this boundary for their already-typed
    /// payload object. The shared serializer validates the object framing and
    /// keeps protocol/version outside the package payload.
    pub fn from_parts_with_payload(
        timestamp_ms: u64,
        source: impl Into<String>,
        kind: impl Into<String>,
        entity: impl Into<String>,
        fields_json: impl Into<String>,
        payload_json: Option<String>,
    ) -> Result<Self, String> {
        let source = source.into();
        let kind = kind.into();
        let entity = entity.into();
        let fields = fields_json.into();
        validate_text(&source, "source")?;
        validate_text(&kind, "kind")?;
        validate_text(&entity, "entity")?;
        validate_json_object(&fields, "fields")?;
        if let Some(payload) = payload_json.as_deref() {
            validate_json_object(payload, "payload")?;
        }
        Ok(Self {
            sequence: 0,
            timestamp_ms,
            source,
            kind,
            entity,
            fields,
            payload: payload_json,
            body: None,
        })
    }

    /// Explicit publication is the only path that attaches values to an
    /// event. Ordinary event constructors never capture locals or data.
    pub fn publish(mut self, payload: JetDevtoolsPayload) -> Result<Self, String> {
        let mut rendered = String::new();
        render_payload(&payload, &mut rendered)?;
        self.payload = Some(rendered);
        Ok(self)
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn fields_json(&self) -> &str {
        &self.fields
    }

    pub fn payload_json(&self) -> Option<&str> {
        self.payload.as_deref()
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
    pub fn body(&self) -> Option<&JetDevtoolsEventBody> {
        self.body.as_ref()
    }

}
/// A generic, host-independent sink for every canonical devtools event.
///
/// Producers publish the same typed event regardless of whether a resident
/// session, a test harness, or another embedder is observing it.  The sink
/// does not depend on a devserver or panel-specific queue.
pub trait JetDevtoolsEventSink: Send + Sync {
    fn publish(&self, event: JetDevtoolsEvent);
}

pub type JetDevtoolsEventSinkHandle = std::sync::Arc<dyn JetDevtoolsEventSink>;

struct JetDevtoolsEventSinkRegistration {
    id: u64,
    sink: JetDevtoolsEventSinkHandle,
}

struct JetDevtoolsEventSinkRegistry {
    next_id: u64,
    sinks: Vec<JetDevtoolsEventSinkRegistration>,
}

impl Default for JetDevtoolsEventSinkRegistry {
    fn default() -> Self {
        Self {
            next_id: 1,
            sinks: Vec::new(),
        }
    }
}

/// A scoped registration for one devtools event sink.
///
/// Registrations fan out instead of replacing one another, so concurrent
/// resident sessions cannot silently steal events. Dropping the guard removes
/// only its own registration.
pub struct JetDevtoolsEventSinkGuard {
    id: u64,
}

impl Drop for JetDevtoolsEventSinkGuard {
    #[inline(always)]
    fn drop(&mut self) {
        if self.id == 0 {
            return;
        }
        let mut registry = JET_DEVTOOLS_EVENT_SINKS
            .lock()
            .expect("devtools event sink registry poisoned");
        registry.sinks.retain(|registration| registration.id != self.id);
    }
}

static JET_DEVTOOLS_EVENT_SINKS: std::sync::LazyLock<
    std::sync::Mutex<JetDevtoolsEventSinkRegistry>,
> = std::sync::LazyLock::new(|| {
    std::sync::Mutex::new(JetDevtoolsEventSinkRegistry::default())
});

#[inline(always)]
pub fn jet_devtools_install_event_sink(
    sink: JetDevtoolsEventSinkHandle,
) -> JetDevtoolsEventSinkGuard {
    let mut registry = JET_DEVTOOLS_EVENT_SINKS
        .lock()
        .expect("devtools event sink registry poisoned");
    let id = registry.next_id;
    registry.next_id = registry.next_id.saturating_add(1).max(1);
    registry
        .sinks
        .push(JetDevtoolsEventSinkRegistration { id, sink });
    JetDevtoolsEventSinkGuard { id }
}

/// Optional installation convenience for session owners. `None` performs no
/// installation; an existing guard must be dropped to uninstall its sink.
#[inline(always)]
pub fn jet_devtools_set_event_sink(
    sink: Option<JetDevtoolsEventSinkHandle>,
) -> Option<JetDevtoolsEventSinkGuard> {
    sink.map(jet_devtools_install_event_sink)
}

/// Explicitly end a scoped installation before its guard leaves scope.
#[inline(always)]
pub fn jet_devtools_uninstall_event_sink(guard: JetDevtoolsEventSinkGuard) {
    drop(guard);
}

#[inline(always)]
pub fn jet_devtools_publish_event(event: JetDevtoolsEvent) {
    let sinks = {
        let registry = JET_DEVTOOLS_EVENT_SINKS
            .lock()
            .expect("devtools event sink registry poisoned");
        registry
            .sinks
            .iter()
            .map(|registration| registration.sink.clone())
            .collect::<Vec<_>>()
    };
    for sink in sinks {
        sink.publish(event.clone());
    }
}


pub type JetDevtoolsTimeCursor = u64;


#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct JetDevtoolsSelection {
    pub panel_id: String,
    pub item_key: Option<String>,
}

impl JetDevtoolsSelection {
    pub fn new(panel_id: impl Into<String>, item_key: Option<String>) -> Self {
        Self {
            panel_id: panel_id.into(),
            item_key,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct JetDevtoolsViewState {
    /// The selected devtools panel and optional item within that panel.
    pub selection: Option<JetDevtoolsSelection>,
    /// Event sequence selected by a host. This is not a wall-clock cursor.
    pub cursor: Option<JetDevtoolsTimeCursor>,
}

impl JetDevtoolsViewState {
    pub fn new(
        selection: Option<JetDevtoolsSelection>,
        cursor: Option<JetDevtoolsTimeCursor>,
    ) -> Self {
        Self { selection, cursor }
    }

    pub fn selection(&self) -> Option<&JetDevtoolsSelection> {
        self.selection.as_ref()
    }

    pub fn cursor(&self) -> Option<JetDevtoolsTimeCursor> {
        self.cursor
    }

    pub fn set_selection(&mut self, selection: Option<JetDevtoolsSelection>) {
        self.selection = selection;
    }

    pub fn set_cursor(&mut self, cursor: Option<JetDevtoolsTimeCursor>) {
        self.cursor = cursor;
    }
}


#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsStateEntry {
    pub key: String,
    pub state: String,
    pub revision: u64,
    /// `None` is the normal payload-free state. A value is retained only when
    /// the state publication site explicitly supplies one.
    pub published: Option<JetDevtoolsValue>,
}

impl JetDevtoolsStateEntry {
    pub fn new(key: impl Into<String>, state: impl Into<String>, revision: u64) -> Self {
        Self {
            key: key.into(),
            state: state.into(),
            revision,
            published: None,
        }
    }

    pub fn publish(mut self, value: JetDevtoolsValue) -> Self {
        self.published = Some(value);
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsStateSnapshot {
    pub cursor: JetDevtoolsTimeCursor,
    pub store: Vec<JetDevtoolsStateEntry>,
    pub query_cache: Vec<JetDevtoolsStateEntry>,
}

impl JetDevtoolsStateSnapshot {
    pub fn new(
        cursor: JetDevtoolsTimeCursor,
        store: Vec<JetDevtoolsStateEntry>,
        query_cache: Vec<JetDevtoolsStateEntry>,
    ) -> Self {
        Self {
            cursor,
            store,
            query_cache,
        }
    }

    fn validate(&self) -> Result<(), String> {
        validate_state_entries(&self.store, "store")?;
        validate_state_entries(&self.query_cache, "query_cache")?;
        Ok(())
    }
}

fn validate_state_entries(entries: &[JetDevtoolsStateEntry], label: &str) -> Result<(), String> {
    if entries.len() > JET_DEVTOOLS_MAX_HISTORY {
        return Err(format!(
            "jet.devtools.v1 {label} snapshot exceeds the Prelude limit"
        ));
    }
    for (index, entry) in entries.iter().enumerate() {
        validate_text(&entry.key, "state key")?;
        validate_text(&entry.state, "state name")?;
        if entry.key.is_empty() {
            return Err(format!(
                "jet.devtools.v1 {label} entry {index} has an empty key"
            ));
        }
        if let Some(value) = &entry.published {
            value.validate()?;
        }
        if entries[index + 1..]
            .iter()
            .any(|other| other.key == entry.key)
        {
            return Err(format!(
                "jet.devtools.v1 {label} snapshot contains duplicate key `{}`",
                entry.key
            ));
        }
    }
    Ok(())
}

/// Bounded, monotonic state history. A time cursor returns the newest
/// snapshot at or before that time; asking before the retained window returns
/// `None`.
#[derive(Clone, Debug)]
pub struct JetDevtoolsStateRing {
    capacity: usize,
    snapshots: std::collections::VecDeque<JetDevtoolsStateSnapshot>,
}

impl JetDevtoolsStateRing {
    pub fn new() -> Self {
        Self::with_capacity(JET_DEVTOOLS_MAX_HISTORY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.clamp(1, JET_DEVTOOLS_MAX_HISTORY),
            snapshots: std::collections::VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    pub fn push(&mut self, snapshot: JetDevtoolsStateSnapshot) -> Result<(), String> {
        snapshot.validate()?;
        if let Some(previous) = self.snapshots.back() {
            if snapshot.cursor < previous.cursor {
                return Err("jet.devtools.v1 state cursor must be monotonic".to_string());
            }
        }
        if self.snapshots.len() == self.capacity {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(snapshot);
        Ok(())
    }

    pub fn latest(&self) -> Option<&JetDevtoolsStateSnapshot> {
        self.snapshots.back()
    }

    pub fn at(&self, cursor: JetDevtoolsTimeCursor) -> Option<&JetDevtoolsStateSnapshot> {
        self.snapshots
            .iter()
            .rev()
            .find(|snapshot| snapshot.cursor <= cursor)
    }

    pub fn store_at(&self, cursor: JetDevtoolsTimeCursor) -> Option<&[JetDevtoolsStateEntry]> {
        self.at(cursor).map(|snapshot| snapshot.store.as_slice())
    }

    pub fn query_cache_at(
        &self,
        cursor: JetDevtoolsTimeCursor,
    ) -> Option<&[JetDevtoolsStateEntry]> {
        self.at(cursor)
            .map(|snapshot| snapshot.query_cache.as_slice())
    }
}

#[derive(Clone, Debug)]
pub struct JetDevtoolsEnvelope {
    pub session_id: String,
    pub started_at_ms: u64,
    pub view: JetDevtoolsViewState,
    pub lifecycle: JetDevtoolsLifecycleState,
    pub freshness: JetDevtoolsFreshnessFact,
    pub privacy: JetDevtoolsPrivacy,
    pub source_identity: Option<JetDevtoolsSourceIdentityFact>,
    pub reset: bool,
    pub truncated: bool,
    events: std::collections::VecDeque<JetDevtoolsEvent>,
    next_sequence: u64,
    max_events: usize,
}

impl JetDevtoolsEnvelope {
    pub fn new(session_id: impl Into<String>, started_at_ms: u64) -> Self {
        Self::with_policy(session_id, started_at_ms, JetDevtoolsPolicy::development())
    }

    pub fn with_policy(
        session_id: impl Into<String>,
        started_at_ms: u64,
        policy: JetDevtoolsPolicy,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            started_at_ms,
            view: JetDevtoolsViewState::default(),
            lifecycle: JetDevtoolsLifecycleState::Starting,
            freshness: JetDevtoolsFreshnessFact::default(),
            privacy: JetDevtoolsPrivacy::from_policy(policy),
            source_identity: None,
            reset: false,
            truncated: false,
            events: std::collections::VecDeque::new(),
            next_sequence: 1,
            max_events: policy.max_events.clamp(1, JET_DEVTOOLS_MAX_EVENTS),
        }
    }

    /// Append one event and return its assigned sequence. The event sequence
    pub fn push(&mut self, mut event: JetDevtoolsEvent) -> u64 {
        event.sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        if event.payload.is_some() {
            self.privacy.published_payloads = true;
        }
        let sequence = event.sequence;
        if self.events.len() == self.max_events {
            self.events.pop_front();
            self.truncated = true;
        }
        self.events.push_back(event);
        self.view.set_cursor(Some(sequence));
        sequence
    }


    /// Ingest a canonical envelope without interpreting event fields. The
    /// destination assigns its own monotonic sequence while retaining the
    /// producer's typed event record and metadata.
    pub fn ingest(&mut self, incoming: &JetDevtoolsEnvelope) -> Result<usize, String> {
        if incoming.session_id != self.session_id {
            return Err("jet.devtools.v1 envelope session identity mismatch".to_string());
        }
        incoming
            .source_identity
            .as_ref()
            .map_or(Ok(()), JetDevtoolsSourceIdentityFact::validate)?;
        let mut count = 0usize;
        for event in incoming.events() {
            self.push(event.clone());
            count = count.saturating_add(1);
        }
        if incoming.view.selection.is_some() {
            self.view.set_selection(incoming.view.selection.clone());
        }
        if incoming.view.cursor.is_some() {
            self.view.set_cursor(incoming.view.cursor);
        }
        self.lifecycle = incoming.lifecycle;
        self.freshness = incoming.freshness;
        self.privacy.loopback_only = incoming.privacy.loopback_only;
        self.privacy.payload_free_by_default = incoming.privacy.payload_free_by_default;
        self.privacy.published_payloads |= incoming.privacy.published_payloads;
        self.source_identity = incoming.source_identity.clone();
        self.reset |= incoming.reset;
        self.truncated |= incoming.truncated;
        Ok(count)
    }

    pub fn events_since(
        &self,
        cursor: Option<JetDevtoolsTimeCursor>,
    ) -> impl Iterator<Item = &JetDevtoolsEvent> {
        self.events
            .iter()
            .filter(move |event| cursor.map_or(true, |value| event.sequence() > value))
    }

    pub fn oldest_sequence(&self) -> Option<u64> {
        self.events.front().map(JetDevtoolsEvent::sequence)
    }

    pub fn latest_sequence(&self) -> Option<u64> {
        self.events.back().map(JetDevtoolsEvent::sequence)
    }

    pub fn events(&self) -> impl Iterator<Item = &JetDevtoolsEvent> {
        self.events.iter()
    }

    /// Return typed game trace bodies retained by the canonical envelope.
    /// Wire kind strings and payload text are never parsed here.
    pub fn game_trace_bodies(&self) -> Vec<JetDevtoolsEventBody> {
        self.events
            .iter()
            .filter_map(|event| {
                let body = event.body()?;
                match body {
                    JetDevtoolsEventBody::GameFrameSample { .. }
                    | JetDevtoolsEventBody::GameDrawEvent { .. } => Some(body.clone()),
                    _ => None,
                }
            })
            .collect()
    }


    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn view(&self) -> &JetDevtoolsViewState {
        &self.view
    }

    pub fn selection(&self) -> Option<&JetDevtoolsSelection> {
        self.view.selection()
    }

    pub fn cursor(&self) -> Option<JetDevtoolsTimeCursor> {
        self.view.cursor()
    }


    pub fn set_selection(&mut self, selection: Option<JetDevtoolsSelection>) {
        self.view.set_selection(selection);
    }

    pub fn set_cursor(&mut self, cursor: Option<JetDevtoolsTimeCursor>) {
        self.view.set_cursor(cursor);
    }

    pub fn set_lifecycle(&mut self, lifecycle: JetDevtoolsLifecycleState) {
        self.lifecycle = lifecycle;
    }

    pub fn set_freshness(&mut self, freshness: JetDevtoolsFreshnessFact) {
        self.freshness = freshness;
    }

    pub fn set_source_identity(
        &mut self,
        identity: Option<JetDevtoolsSourceIdentityFact>,
    ) -> Result<(), String> {
        if let Some(identity) = &identity {
            identity.validate()?;
        }
        self.source_identity = identity;
        Ok(())
    }

    pub fn mark_reset(&mut self) {
        self.reset = true;
        self.truncated = false;
    }

    pub fn serialize(&self) -> Result<String, String> {
        jet_devtools_serialize(self)
    }
}



pub fn jet_devtools_serialize(envelope: &JetDevtoolsEnvelope) -> Result<String, String> {
    validate_text(&envelope.session_id, "session_id")?;
    if let Some(identity) = &envelope.source_identity {
        identity.validate()?;
    }
    let mut out = String::new();
    out.push_str("{\"protocol\":");
    render_text(JET_DEVTOOLS_PROTOCOL, &mut out)?;
    out.push_str(",\"session_id\":");
    render_text(&envelope.session_id, &mut out)?;
    out.push_str(",\"started_at_ms\":");
    out.push_str(&envelope.started_at_ms.to_string());
    out.push_str(",\"events\":[");
    render_event_ring(envelope.events(), &mut out)?;
    out.push_str("],\"selection\":");
    match &envelope.view.selection {
        Some(selection) => render_selection(selection, &mut out)?,
        None => out.push_str("null"),
    }
    out.push_str(",\"cursor\":");
    match envelope.view.cursor {
        Some(cursor) => out.push_str(&cursor.to_string()),
        None => out.push_str("null"),
    }
    let has_metadata = envelope.lifecycle != JetDevtoolsLifecycleState::Starting
        || envelope.freshness != JetDevtoolsFreshnessFact::default()
        || envelope.privacy.published_payloads
        || envelope.source_identity.is_some()
        || envelope.reset
        || envelope.truncated;
    if has_metadata {
        out.push_str(",\"lifecycle\":");
        render_text(envelope.lifecycle.as_str(), &mut out)?;
        out.push_str(",\"freshness\":{\"state\":");
        render_text(envelope.freshness.state.as_str(), &mut out)?;
        out.push_str(",\"observed_at_ms\":");
        out.push_str(&envelope.freshness.observed_at_ms.to_string());
        out.push_str("},\"privacy\":{\"loopback_only\":");
        out.push_str(if envelope.privacy.loopback_only { "true" } else { "false" });
        out.push_str(",\"payload_free_by_default\":");
        out.push_str(if envelope.privacy.payload_free_by_default {
            "true"
        } else {
            "false"
        });
        out.push_str(",\"published_payloads\":");
        out.push_str(if envelope.privacy.published_payloads {
            "true"
        } else {
            "false"
        });
        out.push_str("},\"reset\":");
        out.push_str(if envelope.reset { "true" } else { "false" });
        out.push_str(",\"truncated\":");
        out.push_str(if envelope.truncated { "true" } else { "false" });
        out.push_str(",\"source_identity\":");
        match &envelope.source_identity {
            Some(identity) => render_source_identity(identity, &mut out)?,
            None => out.push_str("null"),
        }
    }
    out.push('}');
    if out.len() > JET_DEVTOOLS_MAX_ENVELOPE_BYTES {
        return Err("jet.devtools.v1 envelope exceeds the Prelude byte limit".to_string());
    }
    Ok(out)
}

pub fn jet_devtools_event_serialize(event: &JetDevtoolsEvent) -> Result<String, String> {
    let mut out = String::new();
    render_event(event, &mut out)?;
    Ok(out)
}

fn render_event_ring<'a>(
    events: impl Iterator<Item = &'a JetDevtoolsEvent>,
    out: &mut String,
) -> Result<(), String> {
    for (index, event) in events.enumerate() {
        if index != 0 {
            out.push(',');
        }
        render_event(event, out)?;
    }
    Ok(())
}

fn render_event(event: &JetDevtoolsEvent, out: &mut String) -> Result<(), String> {
    if event.sequence == 0 {
        return Err("jet.devtools.v1 event has no assigned sequence".to_string());
    }
    validate_text(&event.source, "source")?;
    validate_text(&event.kind, "kind")?;
    validate_text(&event.entity, "entity")?;
    validate_json_object(&event.fields, "fields")?;
    if let Some(payload) = event.payload.as_deref() {
        validate_json_object(payload, "payload")?;
    }
    out.push_str("{\"sequence\":");
    out.push_str(&event.sequence.to_string());
    out.push_str(",\"timestamp_ms\":");
    out.push_str(&event.timestamp_ms.to_string());
    out.push_str(",\"source\":");
    render_text(&event.source, out)?;
    out.push_str(",\"kind\":");
    render_text(&event.kind, out)?;
    out.push_str(",\"entity\":");
    render_text(&event.entity, out)?;
    out.push_str(",\"fields\":");
    out.push_str(&event.fields);
    out.push_str(",\"payload\":");
    match event.payload.as_deref() {
        Some(payload) => out.push_str(payload),
        None => out.push_str("null"),
    }
    out.push('}');
    Ok(())
}

fn render_selection(
    selection: &JetDevtoolsSelection,
    out: &mut String,
) -> Result<(), String> {
    validate_text(&selection.panel_id, "selection.panel_id")?;
    if let Some(item_key) = &selection.item_key {
        validate_text(item_key, "selection.item_key")?;
    }
    out.push_str("{\"panel_id\":");
    render_text(&selection.panel_id, out)?;
    out.push_str(",\"item_key\":");
    match &selection.item_key {
        Some(item_key) => render_text(item_key, out)?,
        None => out.push_str("null"),
    }
    out.push('}');
    Ok(())
}
fn render_source_identity(
    identity: &JetDevtoolsSourceIdentityFact,
    out: &mut String,
) -> Result<(), String> {
    identity.validate()?;
    out.push('{');
    let mut first = true;
    for (key, value) in [
        ("source_id", identity.source_id.as_deref()),
        ("build_id", identity.build_id.as_deref()),
        ("revision", identity.revision.as_deref()),
        ("world_id", identity.world_id.as_deref()),
    ] {
        if let Some(value) = value {
            if !first {
                out.push(',');
            }
            first = false;
            render_text(key, out)?;
            out.push(':');
            render_text(value, out)?;
        }
    }
    out.push('}');
    Ok(())
}


fn render_body_fields(body: &JetDevtoolsEventBody) -> Result<String, String> {
    let mut out = String::new();
    out.push('{');
    match body {
        JetDevtoolsEventBody::Build {
            status,
            tier,
            diagnostics,
            kept,
            reset,
        } => {
            render_text_field("status", status, &mut out)?;
            render_text_field("tier", tier, &mut out)?;
            if diagnostics.len() > JET_DEVTOOLS_MAX_HISTORY {
                return Err("jet.devtools.v1 build has too many diagnostics".to_string());
            }
            out.push_str(",\"diagnostics\":[");
            for (index, diagnostic) in diagnostics.iter().enumerate() {
                if index != 0 {
                    out.push(',');
                }
                render_text(diagnostic, &mut out)?;
            }
            out.push(']');
            render_u64_field("kept", *kept, &mut out);
            render_u64_field("reset", *reset, &mut out);
        }
        JetDevtoolsEventBody::Route {
            path,
            matched,
            render_mode,
            reason,
        } => {
            render_text_field("path", path, &mut out)?;
            render_bool_field("matched", *matched, &mut out);
            render_text_field("render_mode", render_mode, &mut out)?;
            render_text_field("reason", reason, &mut out)?;
        }
        JetDevtoolsEventBody::Query {
            name,
            state,
            footprint,
        } => {
            render_text_field("name", name, &mut out)?;
            render_text_field("state", state.wire(), &mut out)?;
            render_u64_field("footprint", *footprint, &mut out);
        }
        JetDevtoolsEventBody::Mutation {
            name,
            lifecycle,
            rollback,
        } => {
            render_text_field("name", name, &mut out)?;
            render_text_field("lifecycle", lifecycle.wire(), &mut out)?;
            render_bool_field("rollback", *rollback, &mut out);
        }
        JetDevtoolsEventBody::Form {
            name,
            field,
            state,
            validation,
        } => {
            render_text_field("name", name, &mut out)?;
            render_text_field("field", field, &mut out)?;
            render_text_field("state", state.wire(), &mut out)?;
            render_text_field("validation", validation, &mut out)?;
        }
        JetDevtoolsEventBody::Table {
            name,
            viewport,
            sort,
            filter,
        } => {
            render_text_field("name", name, &mut out)?;
            render_u64_field("viewport", *viewport, &mut out);
            render_text_field("sort", sort, &mut out)?;
            render_text_field("filter", filter, &mut out)?;
        }
        JetDevtoolsEventBody::Store {
            name,
            transaction,
            diff,
            cursor,
        } => {
            render_text_field("name", name, &mut out)?;
            render_text_field("transaction", transaction, &mut out)?;
            render_text_field("diff", diff, &mut out)?;
            render_u64_field("cursor", *cursor, &mut out);
        }
        JetDevtoolsEventBody::Trace {
            name,
            kind,
            request,
            duration_ms,
            status,
        } => {
            render_text_field("name", name, &mut out)?;
            render_text_field("kind", kind.wire(), &mut out)?;
            render_text_field("request", request, &mut out)?;
            render_u64_field("duration_ms", *duration_ms, &mut out);
            render_text_field("status", status, &mut out)?;
        }
        JetDevtoolsEventBody::Cost {
            name,
            rule,
            bytes,
            allocations,
        } => {
            render_text_field("name", name, &mut out)?;
            render_text_field("rule", rule, &mut out)?;
            render_u64_field("bytes", *bytes, &mut out);
            render_u64_field("allocations", *allocations, &mut out);
        }
        JetDevtoolsEventBody::Gate {
            name,
            status,
            detail,
        } => {
            render_text_field("name", name, &mut out)?;
            render_text_field("status", status, &mut out)?;
            render_text_field("detail", detail, &mut out)?;
        }
        JetDevtoolsEventBody::Structure {
            name,
            kind,
            status,
        } => {
            render_text_field("name", name, &mut out)?;
            render_text_field("kind", kind, &mut out)?;
            render_text_field("status", status, &mut out)?;
        }
        JetDevtoolsEventBody::Test {
            name,
            status,
            duration_ms,
            diagnostics,
        } => {
            render_text_field("name", name, &mut out)?;
            render_text_field("status", status, &mut out)?;
            render_u64_field("duration_ms", *duration_ms, &mut out);
            render_text_field("diagnostics", diagnostics, &mut out)?;
        }
        JetDevtoolsEventBody::Job {
            name,
            status,
            duration_ms,
            diagnostics,
        } => {
            render_text_field("name", name, &mut out)?;
            render_text_field("status", status, &mut out)?;
            render_u64_field("duration_ms", *duration_ms, &mut out);
            render_text_field("diagnostics", diagnostics, &mut out)?;
        }
        JetDevtoolsEventBody::Frame {
            name,
            status,
            duration_ms,
        } => {
            render_text_field("name", name, &mut out)?;
            render_text_field("status", status, &mut out)?;
            render_u64_field("duration_ms", *duration_ms, &mut out);
        }
        JetDevtoolsEventBody::GameFrameSample {
            sequence,
            frame_id,
            scene,
            frame_index,
            build,
            revision,
            trace_id,
            start_ns,
            cpu_ns,
            gpu_ns,
            responsible_function,
        } => {
            render_u64_field("sequence", *sequence, &mut out);
            render_u64_field("frame_id", *frame_id, &mut out);
            render_text_field("scene", scene, &mut out)?;
            render_u64_field("frame_index", *frame_index, &mut out);
            render_text_field("build", build, &mut out)?;
            render_text_field("revision", revision, &mut out)?;
            render_text_field("trace_id", trace_id, &mut out)?;
            render_u64_field("start_ns", *start_ns, &mut out);
            render_u64_field("cpu_ns", *cpu_ns, &mut out);
            match gpu_ns {
                Some(value) => render_u64_field("gpu_ns", *value, &mut out),
                None => render_null_field("gpu_ns", &mut out),
            }
            match responsible_function {
                Some(function) => render_function_field("responsible_function", function, &mut out)?,
                None => render_null_field("responsible_function", &mut out),
            }
        }
        JetDevtoolsEventBody::GameDrawEvent {
            sequence,
            frame_id,
            scene,
            frame_index,
            build,
            revision,
            trace_id,
            event_index,
            event_id,
            function,
            phase,
            domain,
            start_ns,
            duration_ns,
            object_id,
            resource_id,
        } => {
            render_u64_field("sequence", *sequence, &mut out);
            render_u64_field("frame_id", *frame_id, &mut out);
            render_text_field("scene", scene, &mut out)?;
            render_u64_field("frame_index", *frame_index, &mut out);
            render_text_field("build", build, &mut out)?;
            render_text_field("revision", revision, &mut out)?;
            render_text_field("trace_id", trace_id, &mut out)?;
            render_u64_field("event_index", *event_index, &mut out);
            render_u64_field("event_id", *event_id, &mut out);
            render_function_field("function", function, &mut out)?;
            render_text_field("phase", phase, &mut out)?;
            render_text_field("domain", domain, &mut out)?;
            render_u64_field("start_ns", *start_ns, &mut out);
            render_u64_field("duration_ns", *duration_ns, &mut out);
            render_optional_text_field("object_id", object_id.as_deref(), &mut out)?;
            render_optional_text_field("resource_id", resource_id.as_deref(), &mut out)?;
        }
        JetDevtoolsEventBody::GameLaunchProfile {
            target,
            run_mode,
            headless_compatible,
            phases,
        } => {
            if phases.len() > 256 {
                return Err("jet.devtools.v1 game launch profile exceeds phase limit".to_string());
            }
            render_text_field("target", target, &mut out)?;
            render_text_field("run_mode", run_mode, &mut out)?;
            render_bool_field("headless_compatible", *headless_compatible, &mut out);
            render_field_prefix("phases", &mut out);
            out.push('[');
            for (index, phase) in phases.iter().enumerate() {
                if index != 0 {
                    out.push(',');
                }
                render_launch_phase(phase, &mut out)?;
            }
            out.push(']');
        }
        JetDevtoolsEventBody::GameSwap {
            sequence,
            path,
            kind,
            status,
            old_schema_id,
            new_schema_id,
            migration,
            reason,
        } => {
            render_u64_field("sequence", *sequence, &mut out);
            render_text_field("path", path, &mut out)?;
            render_text_field("kind", kind, &mut out)?;
            render_text_field("status", status, &mut out)?;
            render_optional_text_field("old_schema_id", old_schema_id.as_deref(), &mut out)?;
            render_optional_text_field("new_schema_id", new_schema_id.as_deref(), &mut out)?;
            render_text_field("migration", migration, &mut out)?;
            render_text_field("reason", reason, &mut out)?;
        }
        JetDevtoolsEventBody::Request {
            id,
            method,
            path,
            status,
        } => {
            render_text_field("id", id, &mut out)?;
            render_text_field("method", method, &mut out)?;
            render_text_field("path", path, &mut out)?;
            render_u64_field("status", u64::from(*status), &mut out);
        }
        JetDevtoolsEventBody::Response { id, status, bytes } => {
            render_text_field("id", id, &mut out)?;
            render_u64_field("status", u64::from(*status), &mut out);
            render_u64_field("bytes", *bytes, &mut out);
        }
    }
    out.push('}');
    Ok(out)
}

fn render_payload(payload: &JetDevtoolsPayload, out: &mut String) -> Result<(), String> {
    payload.validate()?;
    let mut ordered: Vec<&JetDevtoolsField> = payload.fields.iter().collect();
    ordered.sort_by(|left, right| left.key.cmp(&right.key));
    out.push_str("{\"fields\":{");
    for (index, field) in ordered.into_iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        render_text(&field.key, out)?;
        out.push(':');
        render_value(&field.value, out)?;
    }
    out.push_str("}}");
    Ok(())
}

pub fn render_value(value: &JetDevtoolsValue, out: &mut String) -> Result<(), String> {
    value.validate()?;
    match value {
        JetDevtoolsValue::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        JetDevtoolsValue::Int(value) => out.push_str(&value.to_string()),
        JetDevtoolsValue::Float(value) => out.push_str(&value.to_string()),
        JetDevtoolsValue::Text(value) => render_text(value, out)?,
    }
    Ok(())
}

fn render_text_field(key: &str, value: &str, out: &mut String) -> Result<(), String> {
    render_field_prefix(key, out);
    render_text(value, out)
}

fn render_u64_field(key: &str, value: u64, out: &mut String) {
    render_field_prefix(key, out);
    out.push_str(&value.to_string());
}
fn render_null_field(key: &str, out: &mut String) {
    render_field_prefix(key, out);
    out.push_str("null");
}

fn render_optional_text_field(
    key: &str,
    value: Option<&str>,
    out: &mut String,
) -> Result<(), String> {
    render_field_prefix(key, out);
    match value {
        Some(value) => render_text(value, out),
        None => {
            out.push_str("null");
            Ok(())
        }
    }
}

fn render_launch_phase(
    phase: &JetDevtoolsGameLaunchPhase,
    out: &mut String,
) -> Result<(), String> {
    phase.validate()?;
    out.push('{');
    render_text_field("phase", &phase.phase, out)?;
    render_text_field("status", &phase.status, out)?;
    render_text_field("detail", &phase.detail, out)?;
    out.push('}');
    Ok(())
}
fn render_function_field(
    key: &str,
    function: &JetDevtoolsFunctionIdentity,
    out: &mut String,
) -> Result<(), String> {
    render_field_prefix(key, out);
    render_function_identity(function, out)
}

fn render_function_identity(
    function: &JetDevtoolsFunctionIdentity,
    out: &mut String,
) -> Result<(), String> {
    function.validate()?;
    out.push('{');
    render_text_field("function_id", &function.function_id, out)?;
    render_text_field("name", &function.name, out)?;
    render_field_prefix("source", out);
    render_source_span(&function.source, out)?;
    out.push('}');
    Ok(())
}

fn render_source_span(span: &JetDevtoolsSourceSpan, out: &mut String) -> Result<(), String> {
    span.validate()?;
    out.push('{');
    render_text_field("source_id", &span.source_id, out)?;
    render_text_field("file", &span.file, out)?;
    render_u64_field("start_line", u64::from(span.start_line), out);
    render_u64_field("start_column", u64::from(span.start_column), out);
    render_u64_field("end_line", u64::from(span.end_line), out);
    render_u64_field("end_column", u64::from(span.end_column), out);
    out.push('}');
    Ok(())
}

fn render_bool_field(key: &str, value: bool, out: &mut String) {
    render_field_prefix(key, out);
    out.push_str(if value { "true" } else { "false" });
}

fn render_field_prefix(key: &str, out: &mut String) {
    if !out.ends_with('{') {
        out.push(',');
    }
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
}

fn validate_text(value: &str, field: &str) -> Result<(), String> {
    if value.len() > JET_DEVTOOLS_MAX_TEXT_BYTES {
        return Err(format!(
            "jet.devtools.v1 {field} exceeds the Prelude text limit"
        ));
    }
    Ok(())
}

/// This is deliberately a framing check, not a second JSON implementation.
/// Adapters own typed payload construction; the shared boundary only prevents
/// malformed fragments and control bytes from crossing into the envelope.
fn validate_json_object(value: &str, field: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.len() > JET_DEVTOOLS_MAX_TEXT_BYTES
        || !trimmed.starts_with('{')
        || !trimmed.ends_with('}')
    {
        return Err(format!(
            "jet.devtools.v1 {field} must be a bounded JSON object"
        ));
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for character in trimmed.chars() {
        if character.is_control() {
            return Err(format!(
                "jet.devtools.v1 {field} contains a control character"
            ));
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth = depth.saturating_add(1),
            '}' => {
                if depth == 0 {
                    return Err(format!("jet.devtools.v1 {field} has unbalanced braces"));
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    if in_string || escaped || depth != 0 {
        return Err(format!("jet.devtools.v1 {field} has unbalanced JSON"));
    }
    Ok(())
}

pub fn render_text(value: &str, out: &mut String) -> Result<(), String> {
    validate_text(value, "text")?;
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str("\\u00");
                let code = character as u32;
                out.push(hex_digit((code >> 4) as u8));
                out.push(hex_digit(code as u8));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    Ok(())
}

pub fn hex_digit(value: u8) -> char {
    match value & 0x0f {
        0..=9 => (b'0' + (value & 0x0f)) as char,
        value => (b'a' + value - 10) as char,
    }
}

#[cfg(test)]
mod jet_devtools_protocol_tests {
    use super::*;

    #[test]
    fn envelope_uses_one_canonical_wire_shape_and_payload_is_opt_in() {
        let mut envelope = JetDevtoolsEnvelope::new("session", 10);
        envelope.push(
            JetDevtoolsEvent::try_new(
                11,
                "test",
                JetDevtoolsEventBody::Query {
                    name: "orders".into(),
                    state: JetDevtoolsQueryState::Fresh,
                    footprint: 3,
                },
            )
            .unwrap(),
        );
        envelope.set_selection(Some(JetDevtoolsSelection::new(
            "Queries",
            Some("orders".into()),
        )));
        let event = envelope.events().next().expect("event");
        assert_eq!(event.sequence(), 1);
        assert!(event.payload_json().is_none());
        assert_eq!(
            envelope.serialize().unwrap(),
            "{\"protocol\":\"jet.devtools.v1\",\"session_id\":\"session\",\"started_at_ms\":10,\"events\":[{\"sequence\":1,\"timestamp_ms\":11,\"source\":\"test\",\"kind\":\"Query\",\"entity\":\"orders\",\"fields\":{\"name\":\"orders\",\"state\":\"fresh\",\"footprint\":3},\"payload\":null}],\"selection\":{\"panel_id\":\"Queries\",\"item_key\":\"orders\"},\"cursor\":1}"
        );
    }

    #[test]
    fn typed_publication_attaches_opt_in_payload_without_reframing_envelope() {
        let mut envelope = JetDevtoolsEnvelope::new("game", 1);
        let event = JetDevtoolsEvent::try_new(
            2,
            "game",
            JetDevtoolsEventBody::Frame {
                name: "scene".into(),
                status: "ready".into(),
                duration_ms: 1,
            },
        )
        .unwrap()
        .publish(JetDevtoolsPayload::one(
            "index",
            JetDevtoolsValue::Int(1),
        ))
        .unwrap();
        envelope.push(event);
        let json = envelope.serialize().unwrap();
        assert!(json.contains("\"protocol\":\"jet.devtools.v1\""));
        assert!(json.contains("\"payload\":{\"fields\":{\"index\":1}}"));
    }

    #[test]
    fn state_ring_returns_the_newest_past_cursor_and_drops_oldest() {
        let mut ring = JetDevtoolsStateRing::with_capacity(2);
        for cursor in [10, 20, 30] {
            ring.push(JetDevtoolsStateSnapshot::new(
                cursor,
                vec![JetDevtoolsStateEntry::new("cart", "ready", cursor)],
                vec![JetDevtoolsStateEntry::new("orders", "fresh", cursor)],
            ))
            .unwrap();
        }
        assert!(ring.at(10).is_none());
        assert_eq!(ring.at(25).unwrap().cursor, 20);
        assert_eq!(ring.store_at(30).unwrap()[0].revision, 30);
        assert_eq!(ring.query_cache_at(30).unwrap()[0].key, "orders");
    }
}
// JET_HOST_DEVTOOLS_NATIVE_BEGIN
// Host-crate projection of the native devtools registry. Compiled consumers
// (`jet_foundation::Devtools::*`) reach the registry through this include and
// these always-on wrappers. Codegen strips this whole region when it splices
// `Prelude/Devtools.rs` (a symlink to this file) into a generated program:
// the program receives `DevtoolsNative.rs` as its own Prelude part exactly
// once and wrappers gated on `JET_DEVTOOLS_RUNTIME_ENABLED` instead.
include!("DevtoolsNative.rs");

#[inline(always)]
pub fn jet_devtools_install_native_host(
    session_id: impl Into<String>,
    host: JetDevtoolsNativeHostHandle,
) -> Result<JetDevtoolsNativeHostGuard, String> {
    jet_devtools_install_native_host_unchecked(session_id, host)
}

#[inline(always)]
pub fn jet_devtools_native_bind_session(session_id: &str) -> Result<(), String> {
    jet_devtools_native_bind_session_unchecked(session_id)
}

#[inline(always)]
pub fn jet_devtools_native_clear_session(session_id: &str) {
    jet_devtools_native_clear_session_unchecked(session_id);
}

#[inline(always)]
pub fn jet_devtools_publish_event_for_session(session_id: &str, event: JetDevtoolsEvent) {
    jet_devtools_publish_event_for_session_unchecked(session_id, event);
}

#[inline(always)]
pub fn jet_devtools_native_window_open(width: u32, height: u32) {
    jet_devtools_native_window_open_unchecked(width, height);
}

#[inline(always)]
pub fn jet_devtools_native_window_close() {
    jet_devtools_native_window_close_unchecked();
}

#[inline(always)]
pub fn jet_devtools_native_input(input: JetDevtoolsNativeInput) -> bool {
    jet_devtools_native_input_unchecked(input)
}

#[inline(always)]
pub fn jet_devtools_native_frame_begin(
    frame_index: u64,
    width: u32,
    height: u32,
) -> Vec<JetDevtoolsNativeDrawCommand> {
    jet_devtools_native_frame_begin_unchecked(frame_index, width, height)
}

#[inline(always)]
pub fn jet_devtools_native_draw(command: JetDevtoolsNativeDrawCommand) {
    jet_devtools_native_draw_unchecked(command);
}

#[inline(always)]
pub fn jet_devtools_native_frame_end() {
    jet_devtools_native_frame_end_unchecked();
}
// JET_HOST_DEVTOOLS_NATIVE_END
