// D-DX-DEVTOOLS1 / #2454: one bounded, typed topology fact set for devtools.
//
// This module is included beside Prelude/Devtools.rs.  Its bounded facts and
// projections cross the same `jet.devtools.v1` envelope; hosts do not redefine
// topology meaning here. Facts contain no process environment, argv, authority
// proof, credentials, or opaque handles.
// or other secret-bearing payload.  Endpoint authority is represented only by
// a closed status and its display is built from safe address parts.

pub const JET_DEVTOOLS_TOPOLOGY_MAX_FACTS: usize = 256;
pub const JET_DEVTOOLS_TOPOLOGY_MAX_RESTARTS: usize = 64;
const JET_DEVTOOLS_TOPOLOGY_MAX_TEXT_BYTES: usize = 256;
const JET_DEVTOOLS_TOPOLOGY_MAX_RESTART_BUDGET: u32 = 1_000;
const JET_DEVTOOLS_TOPOLOGY_MAX_RESTART_WINDOW_MS: u64 = 86_400_000;
const JET_DEVTOOLS_TOPOLOGY_MAX_BYTES: u64 = 1 << 40;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetDevtoolsTopologyError {
    TooManyFacts,
    TooManyRestarts { id: String },
    DuplicateIdentity { id: String },
    InvalidText { field: &'static str },
    InvalidEndpointAddress,
    InvalidRestartHistory { id: String },
    InvalidRestartPolicy { id: String },
    InvalidByteCount { id: String },
    Cycle { id: String },
    MissingParent { child_id: String, parent_id: String },
    UnknownRoot { id: String },
}

impl std::fmt::Display for JetDevtoolsTopologyError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyFacts => write!(output, "topology fact limit exceeded"),
            Self::TooManyRestarts { id } => {
                write!(output, "restart history limit exceeded for `{id}`")
            }
            Self::DuplicateIdentity { id } => write!(output, "duplicate topology identity `{id}`"),
            Self::InvalidText { field } => write!(output, "invalid topology {field}"),
            Self::InvalidEndpointAddress => write!(output, "invalid topology endpoint address"),
            Self::InvalidRestartHistory { id } => {
                write!(output, "invalid restart history for `{id}`")
            }
            Self::InvalidRestartPolicy { id } => {
                write!(output, "invalid restart policy for `{id}`")
            }
            Self::InvalidByteCount { id } => {
                write!(output, "invalid endpoint byte count for `{id}`")
            }
            Self::Cycle { id } => write!(output, "topology cycle at `{id}`"),
            Self::MissingParent {
                child_id,
                parent_id,
            } => write!(output, "topology parent `{parent_id}` for `{child_id}` is missing"),
            Self::UnknownRoot { id } => write!(output, "unknown topology root `{id}`"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologySupervisorState {
    Starting,
    Running,
    Failed,
    Restarting,
    Stopping,
    Stopped,
    Escalated,
}

impl JetDevtoolsTopologySupervisorState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Failed => "failed",
            Self::Restarting => "restarting",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Escalated => "escalated",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyProcessState {
    Starting,
    Running,
    Failed,
    Restarting,
    Stopping,
    Stopped,
    Exited,
}

impl JetDevtoolsTopologyProcessState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Failed => "failed",
            Self::Restarting => "restarting",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Exited => "exited",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyTaskState {
    Created,
    Runnable,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

impl JetDevtoolsTopologyTaskState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Runnable => "runnable",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyReadinessState {
    Unknown,
    Checking,
    Ready,
    NotReady,
    Failed,
}

impl JetDevtoolsTopologyReadinessState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Checking => "checking",
            Self::Ready => "ready",
            Self::NotReady => "not_ready",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyReadinessReason {
    NotChecked,
    Starting,
    WaitingForParent,
    WaitingForProcess,
    WaitingForEndpoint,
    ProbePassed,
    ProbeFailed,
    TimedOut,
    AuthorityRevoked,
    Stopped,
}

impl JetDevtoolsTopologyReadinessReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotChecked => "not_checked",
            Self::Starting => "starting",
            Self::WaitingForParent => "waiting_for_parent",
            Self::WaitingForProcess => "waiting_for_process",
            Self::WaitingForEndpoint => "waiting_for_endpoint",
            Self::ProbePassed => "probe_passed",
            Self::ProbeFailed => "probe_failed",
            Self::TimedOut => "timed_out",
            Self::AuthorityRevoked => "authority_revoked",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyRestartStrategy {
    OneForOne,
    OneForAll,
    RestForOne,
}

impl JetDevtoolsTopologyRestartStrategy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OneForOne => "one_for_one",
            Self::OneForAll => "one_for_all",
            Self::RestForOne => "rest_for_one",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevtoolsTopologyRestartPolicy {
    pub strategy: JetDevtoolsTopologyRestartStrategy,
    pub max_restarts: u32,
    pub window_ms: u64,
}

impl JetDevtoolsTopologyRestartPolicy {
    pub fn new(
        strategy: JetDevtoolsTopologyRestartStrategy,
        max_restarts: u32,
        window_ms: u64,
    ) -> Result<Self, JetDevtoolsTopologyError> {
        let policy = Self {
            strategy,
            max_restarts,
            window_ms,
        };
        jet_devtools_topology_validate_restart_policy(&policy, "<policy>")?;
        Ok(policy)
    }

    fn render_json(&self) -> String {
        format!(
            "{{\"strategy\":{},\"max_restarts\":{},\"window_ms\":{}}}",
            jet_devtools_topology_json_string(self.strategy.as_str()),
            self.max_restarts,
            self.window_ms,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyFailureKind {
    Failed,
    Cancelled,
    TimedOut,
    WorkerLost,
    Escalated,
}

impl JetDevtoolsTopologyFailureKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::WorkerLost => "worker_lost",
            Self::Escalated => "escalated",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyEndpointConnectionState {
    Unknown,
    Connecting,
    Connected,
    Draining,
    Closed,
    Failed,
}

impl JetDevtoolsTopologyEndpointConnectionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Draining => "draining",
            Self::Closed => "closed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyRestartReason {
    Failed,
    Exited,
    HealthCheck,
    Manual,
    Policy,
}

impl JetDevtoolsTopologyRestartReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Exited => "exited",
            Self::HealthCheck => "health_check",
            Self::Manual => "manual",
            Self::Policy => "policy",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevtoolsTopologyRestartFact {
    pub at_ms: u64,
    pub duration_ms: u64,
    pub reason: JetDevtoolsTopologyRestartReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyFreshnessState {
    Fresh,
    Stale,
    Unknown,
}

impl JetDevtoolsTopologyFreshnessState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JetDevtoolsTopologyFreshness {
    pub state: JetDevtoolsTopologyFreshnessState,
    pub observed_at_ms: u64,
}

impl JetDevtoolsTopologyFreshness {
    pub const fn fresh(observed_at_ms: u64) -> Self {
        Self {
            state: JetDevtoolsTopologyFreshnessState::Fresh,
            observed_at_ms,
        }
    }

    pub const fn stale(observed_at_ms: u64) -> Self {
        Self {
            state: JetDevtoolsTopologyFreshnessState::Stale,
            observed_at_ms,
        }
    }

    pub const fn unknown(observed_at_ms: u64) -> Self {
        Self {
            state: JetDevtoolsTopologyFreshnessState::Unknown,
            observed_at_ms,
        }
    }
}

impl Default for JetDevtoolsTopologyFreshness {
    fn default() -> Self {
        Self::unknown(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyEndpointScheme {
    Tcp,
    Http,
    Https,
    Unix,
}

impl JetDevtoolsTopologyEndpointScheme {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Http => "http",
            Self::Https => "https",
            Self::Unix => "unix",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyEndpointAuthorityState {
    Unknown,
    Unverified,
    Verified,
    Revoked,
}

impl JetDevtoolsTopologyEndpointAuthorityState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Unverified => "unverified",
            Self::Verified => "verified",
            Self::Revoked => "revoked",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyNodeKind {
    Supervisor,
    Process,
    Task,
    Endpoint,
    Readiness,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevtoolsTopologySupervisorFact {
    pub id: String,
    pub source: String,
    pub parent_id: Option<String>,
    pub state: JetDevtoolsTopologySupervisorState,
    pub restart_history: Vec<JetDevtoolsTopologyRestartFact>,
    pub owner: Option<String>,
    pub restart_policy: Option<JetDevtoolsTopologyRestartPolicy>,
    pub failure: Option<JetDevtoolsTopologyFailureKind>,
    pub duration_ms: Option<u64>,
    pub freshness: JetDevtoolsTopologyFreshness,
}

impl JetDevtoolsTopologySupervisorFact {
    pub fn new(
        id: impl Into<String>,
        source: impl Into<String>,
        parent_id: Option<String>,
        state: JetDevtoolsTopologySupervisorState,
        restart_history: Vec<JetDevtoolsTopologyRestartFact>,
        freshness: JetDevtoolsTopologyFreshness,
    ) -> Result<Self, JetDevtoolsTopologyError> {
        let fact = Self {
            id: id.into(),
            source: source.into(),
            parent_id,
            state,
            restart_history,
            owner: None,
            restart_policy: None,
            failure: None,
            duration_ms: None,
            freshness,
        };
        jet_devtools_topology_validate_supervisor(&fact)?;
        Ok(fact)
    }

    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = Some(owner.into());
        self
    }

    pub fn with_restart_policy(
        mut self,
        restart_policy: JetDevtoolsTopologyRestartPolicy,
    ) -> Self {
        self.restart_policy = Some(restart_policy);
        self
    }

    pub fn with_failure(mut self, failure: JetDevtoolsTopologyFailureKind) -> Self {
        self.failure = Some(failure);
        self
    }

    pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevtoolsTopologyProcessFact {
    pub id: String,
    pub source: String,
    pub parent_id: Option<String>,
    pub state: JetDevtoolsTopologyProcessState,
    pub pid: Option<u64>,
    pub restart_history: Vec<JetDevtoolsTopologyRestartFact>,
    pub owner: Option<String>,
    pub restart_policy: Option<JetDevtoolsTopologyRestartPolicy>,
    pub freshness: JetDevtoolsTopologyFreshness,
}

impl JetDevtoolsTopologyProcessFact {
    pub fn new(
        id: impl Into<String>,
        source: impl Into<String>,
        parent_id: Option<String>,
        state: JetDevtoolsTopologyProcessState,
        pid: Option<u64>,
        restart_history: Vec<JetDevtoolsTopologyRestartFact>,
        freshness: JetDevtoolsTopologyFreshness,
    ) -> Result<Self, JetDevtoolsTopologyError> {
        let fact = Self {
            id: id.into(),
            source: source.into(),
            parent_id,
            state,
            pid,
            restart_history,
            owner: None,
            restart_policy: None,
            freshness,
        };
        jet_devtools_topology_validate_process(&fact)?;
        Ok(fact)
    }
    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = Some(owner.into());
        self
    }

    pub fn with_restart_policy(
        mut self,
        restart_policy: JetDevtoolsTopologyRestartPolicy,
    ) -> Self {
        self.restart_policy = Some(restart_policy);
        self
    }

}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevtoolsTopologyTaskFact {
    pub id: String,
    pub source: String,
    pub parent_id: Option<String>,
    pub state: JetDevtoolsTopologyTaskState,
    pub restart_history: Vec<JetDevtoolsTopologyRestartFact>,
    pub owner: Option<String>,
    pub wait_target: Option<String>,
    pub duration_ms: Option<u64>,
    pub failure: Option<JetDevtoolsTopologyFailureKind>,
    pub restart_policy: Option<JetDevtoolsTopologyRestartPolicy>,
    pub freshness: JetDevtoolsTopologyFreshness,
}

impl JetDevtoolsTopologyTaskFact {
    pub fn new(
        id: impl Into<String>,
        source: impl Into<String>,
        parent_id: Option<String>,
        state: JetDevtoolsTopologyTaskState,
        restart_history: Vec<JetDevtoolsTopologyRestartFact>,
        freshness: JetDevtoolsTopologyFreshness,
    ) -> Result<Self, JetDevtoolsTopologyError> {
        let fact = Self {
            id: id.into(),
            source: source.into(),
            parent_id,
            state,
            restart_history,
            owner: None,
            wait_target: None,
            duration_ms: None,
            failure: None,
            restart_policy: None,
            freshness,
        };
        jet_devtools_topology_validate_task(&fact)?;
        Ok(fact)
    }
    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = Some(owner.into());
        self
    }

    pub fn with_wait_target(mut self, wait_target: impl Into<String>) -> Self {
        self.wait_target = Some(wait_target.into());
        self
    }

    pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    pub fn with_failure(mut self, failure: JetDevtoolsTopologyFailureKind) -> Self {
        self.failure = Some(failure);
        self
    }

    pub fn with_restart_policy(
        mut self,
        restart_policy: JetDevtoolsTopologyRestartPolicy,
    ) -> Self {
        self.restart_policy = Some(restart_policy);
        self
    }

}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevtoolsTopologyEndpointFact {
    pub id: String,
    pub source: String,
    pub parent_id: Option<String>,
    pub scheme: JetDevtoolsTopologyEndpointScheme,
    pub host: String,
    pub port: Option<u16>,
    pub authority_state: JetDevtoolsTopologyEndpointAuthorityState,
    pub owner: Option<String>,
    pub connection: JetDevtoolsTopologyEndpointConnectionState,
    pub bytes_in: Option<u64>,
    pub bytes_out: Option<u64>,
    pub restart_policy: Option<JetDevtoolsTopologyRestartPolicy>,
    pub freshness: JetDevtoolsTopologyFreshness,
}

impl JetDevtoolsTopologyEndpointFact {
    pub fn new(
        id: impl Into<String>,
        source: impl Into<String>,
        parent_id: Option<String>,
        scheme: JetDevtoolsTopologyEndpointScheme,
        host: impl Into<String>,
        port: Option<u16>,
        authority_state: JetDevtoolsTopologyEndpointAuthorityState,
        freshness: JetDevtoolsTopologyFreshness,
    ) -> Result<Self, JetDevtoolsTopologyError> {
        let fact = Self {
            id: id.into(),
            source: source.into(),
            parent_id,
            scheme,
            host: host.into(),
            port,
            authority_state,
            owner: None,
            connection: JetDevtoolsTopologyEndpointConnectionState::Unknown,
            bytes_in: None,
            bytes_out: None,
            restart_policy: None,
            freshness,
        };
        jet_devtools_topology_validate_endpoint(&fact)?;
        Ok(fact)
    }

    pub fn display(&self) -> JetDevtoolsTopologyEndpointDisplay {
        JetDevtoolsTopologyEndpointDisplay {
            scheme: self.scheme,
            host: self.host.clone(),
            port: self.port,
            authority_state: self.authority_state,
        }
    }
    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = Some(owner.into());
        self
    }

    pub fn with_connection(
        mut self,
        connection: JetDevtoolsTopologyEndpointConnectionState,
    ) -> Self {
        self.connection = connection;
        self
    }

    pub fn with_bytes(mut self, bytes_in: u64, bytes_out: u64) -> Self {
        self.bytes_in = Some(bytes_in);
        self.bytes_out = Some(bytes_out);
        self
    }

    pub fn with_restart_policy(
        mut self,
        restart_policy: JetDevtoolsTopologyRestartPolicy,
    ) -> Self {
        self.restart_policy = Some(restart_policy);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevtoolsTopologyReadinessFact {
    pub id: String,
    pub source: String,
    pub parent_id: Option<String>,
    pub state: JetDevtoolsTopologyReadinessState,
    pub reason: JetDevtoolsTopologyReadinessReason,
    pub freshness: JetDevtoolsTopologyFreshness,
}

impl JetDevtoolsTopologyReadinessFact {
    pub fn new(
        id: impl Into<String>,
        source: impl Into<String>,
        parent_id: Option<String>,
        state: JetDevtoolsTopologyReadinessState,
        reason: JetDevtoolsTopologyReadinessReason,
        freshness: JetDevtoolsTopologyFreshness,
    ) -> Result<Self, JetDevtoolsTopologyError> {
        let fact = Self {
            id: id.into(),
            source: source.into(),
            parent_id,
            state,
            reason,
            freshness,
        };
        jet_devtools_topology_validate_readiness(&fact)?;
        Ok(fact)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetDevtoolsTopologyFact {
    Supervisor(JetDevtoolsTopologySupervisorFact),
    Process(JetDevtoolsTopologyProcessFact),
    Task(JetDevtoolsTopologyTaskFact),
    Endpoint(JetDevtoolsTopologyEndpointFact),
    Readiness(JetDevtoolsTopologyReadinessFact),
}

impl From<JetDevtoolsTopologySupervisorFact> for JetDevtoolsTopologyFact {
    fn from(fact: JetDevtoolsTopologySupervisorFact) -> Self {
        Self::Supervisor(fact)
    }
}

impl From<JetDevtoolsTopologyProcessFact> for JetDevtoolsTopologyFact {
    fn from(fact: JetDevtoolsTopologyProcessFact) -> Self {
        Self::Process(fact)
    }
}

impl From<JetDevtoolsTopologyTaskFact> for JetDevtoolsTopologyFact {
    fn from(fact: JetDevtoolsTopologyTaskFact) -> Self {
        Self::Task(fact)
    }
}

impl From<JetDevtoolsTopologyEndpointFact> for JetDevtoolsTopologyFact {
    fn from(fact: JetDevtoolsTopologyEndpointFact) -> Self {
        Self::Endpoint(fact)
    }
}

impl From<JetDevtoolsTopologyReadinessFact> for JetDevtoolsTopologyFact {
    fn from(fact: JetDevtoolsTopologyReadinessFact) -> Self {
        Self::Readiness(fact)
    }
}

impl JetDevtoolsTopologyFact {
    pub fn id(&self) -> &str {
        match self {
            Self::Supervisor(fact) => &fact.id,
            Self::Process(fact) => &fact.id,
            Self::Task(fact) => &fact.id,
            Self::Endpoint(fact) => &fact.id,
            Self::Readiness(fact) => &fact.id,
        }
    }

    pub fn source(&self) -> &str {
        match self {
            Self::Supervisor(fact) => &fact.source,
            Self::Process(fact) => &fact.source,
            Self::Task(fact) => &fact.source,
            Self::Endpoint(fact) => &fact.source,
            Self::Readiness(fact) => &fact.source,
        }
    }

    pub fn parent_id(&self) -> Option<&str> {
        match self {
            Self::Supervisor(fact) => fact.parent_id.as_deref(),
            Self::Process(fact) => fact.parent_id.as_deref(),
            Self::Task(fact) => fact.parent_id.as_deref(),
            Self::Endpoint(fact) => fact.parent_id.as_deref(),
            Self::Readiness(fact) => fact.parent_id.as_deref(),
        }
    }

    pub fn kind(&self) -> JetDevtoolsTopologyNodeKind {
        match self {
            Self::Supervisor(_) => JetDevtoolsTopologyNodeKind::Supervisor,
            Self::Process(_) => JetDevtoolsTopologyNodeKind::Process,
            Self::Task(_) => JetDevtoolsTopologyNodeKind::Task,
            Self::Endpoint(_) => JetDevtoolsTopologyNodeKind::Endpoint,
            Self::Readiness(_) => JetDevtoolsTopologyNodeKind::Readiness,
        }
    }

    fn freshness(&self) -> JetDevtoolsTopologyFreshness {
        match self {
            Self::Supervisor(fact) => fact.freshness,
            Self::Process(fact) => fact.freshness,
            Self::Task(fact) => fact.freshness,
            Self::Endpoint(fact) => fact.freshness,
            Self::Readiness(fact) => fact.freshness,
        }
    }
}

impl JetDevtoolsTopologySupervisorFact {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_topology_validate_supervisor(self).map_err(|error| error.to_string())?;
        JetDevtoolsEvent::from_parts(
            self.freshness.observed_at_ms,
            source,
            "Supervisor",
            self.id.clone(),
            jet_devtools_topology_supervisor_fields(self),
        )
    }
}

impl JetDevtoolsTopologyProcessFact {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_topology_validate_process(self).map_err(|error| error.to_string())?;
        JetDevtoolsEvent::from_parts(
            self.freshness.observed_at_ms,
            source,
            "Process",
            self.id.clone(),
            jet_devtools_topology_process_fields(self),
        )
    }
}

impl JetDevtoolsTopologyTaskFact {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_topology_validate_task(self).map_err(|error| error.to_string())?;
        JetDevtoolsEvent::from_parts(
            self.freshness.observed_at_ms,
            source,
            "Task",
            self.id.clone(),
            jet_devtools_topology_task_fields(self),
        )
    }
}

impl JetDevtoolsTopologyEndpointFact {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_topology_validate_endpoint(self).map_err(|error| error.to_string())?;
        JetDevtoolsEvent::from_parts(
            self.freshness.observed_at_ms,
            source,
            "Endpoint",
            self.id.clone(),
            jet_devtools_topology_endpoint_fields(self),
        )
    }
}

impl JetDevtoolsTopologyReadinessFact {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_topology_validate_readiness(self).map_err(|error| error.to_string())?;
        JetDevtoolsEvent::from_parts(
            self.freshness.observed_at_ms,
            source,
            "Readiness",
            self.id.clone(),
            jet_devtools_topology_readiness_fields(self),
        )
    }
}

impl JetDevtoolsTopologyFact {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        match self {
            Self::Supervisor(fact) => fact.to_protocol_event(source),
            Self::Process(fact) => fact.to_protocol_event(source),
            Self::Task(fact) => fact.to_protocol_event(source),
            Self::Endpoint(fact) => fact.to_protocol_event(source),
            Self::Readiness(fact) => fact.to_protocol_event(source),
        }
    }
}

fn jet_devtools_topology_supervisor_fields(
    fact: &JetDevtoolsTopologySupervisorFact,
) -> String {
    let mut fields = jet_devtools_topology_common_fields(
        &fact.id,
        &fact.source,
        fact.parent_id.as_deref(),
        fact.freshness,
    );
    fields.push(format!(
        "\"state\":{}",
        jet_devtools_topology_json_string(fact.state.as_str())
    ));
    fields.push(format!(
        "\"restart_history\":{}",
        jet_devtools_topology_restart_history(&fact.restart_history)
    ));
    fields.push(jet_devtools_topology_optional_text_field(
        "owner",
        fact.owner.as_deref(),
    ));
    fields.push(jet_devtools_topology_optional_policy_field(
        "restart_policy",
        fact.restart_policy.as_ref(),
    ));
    fields.push(jet_devtools_topology_optional_failure_field(
        "failure",
        fact.failure,
    ));
    fields.push(jet_devtools_topology_optional_u64_field(
        "duration_ms",
        fact.duration_ms,
    ));
    jet_devtools_topology_object(fields)
}

fn jet_devtools_topology_process_fields(
    fact: &JetDevtoolsTopologyProcessFact,
) -> String {
    let mut fields = jet_devtools_topology_common_fields(
        &fact.id,
        &fact.source,
        fact.parent_id.as_deref(),
        fact.freshness,
    );
    fields.push(format!(
        "\"state\":{}",
        jet_devtools_topology_json_string(fact.state.as_str())
    ));
    fields.push(jet_devtools_topology_optional_u64_field("pid", fact.pid));
    fields.push(format!(
        "\"restart_history\":{}",
        jet_devtools_topology_restart_history(&fact.restart_history)
    ));
    fields.push(jet_devtools_topology_optional_text_field(
        "owner",
        fact.owner.as_deref(),
    ));
    fields.push(jet_devtools_topology_optional_policy_field(
        "restart_policy",
        fact.restart_policy.as_ref(),
    ));
    jet_devtools_topology_object(fields)
}

fn jet_devtools_topology_task_fields(fact: &JetDevtoolsTopologyTaskFact) -> String {
    let mut fields = jet_devtools_topology_common_fields(
        &fact.id,
        &fact.source,
        fact.parent_id.as_deref(),
        fact.freshness,
    );
    fields.push(format!(
        "\"state\":{}",
        jet_devtools_topology_json_string(fact.state.as_str())
    ));
    fields.push(format!(
        "\"restart_history\":{}",
        jet_devtools_topology_restart_history(&fact.restart_history)
    ));
    fields.push(jet_devtools_topology_optional_text_field(
        "owner",
        fact.owner.as_deref(),
    ));
    fields.push(jet_devtools_topology_optional_text_field(
        "wait_target",
        fact.wait_target.as_deref(),
    ));
    fields.push(jet_devtools_topology_optional_u64_field(
        "duration_ms",
        fact.duration_ms,
    ));
    fields.push(jet_devtools_topology_optional_failure_field(
        "failure",
        fact.failure,
    ));
    fields.push(jet_devtools_topology_optional_policy_field(
        "restart_policy",
        fact.restart_policy.as_ref(),
    ));
    jet_devtools_topology_object(fields)
}

fn jet_devtools_topology_endpoint_fields(
    fact: &JetDevtoolsTopologyEndpointFact,
) -> String {
    let mut fields = jet_devtools_topology_common_fields(
        &fact.id,
        &fact.source,
        fact.parent_id.as_deref(),
        fact.freshness,
    );
    fields.push(format!(
        "\"scheme\":{}",
        jet_devtools_topology_json_string(fact.scheme.as_str())
    ));
    fields.push(format!(
        "\"host\":{}",
        jet_devtools_topology_json_string(&fact.host)
    ));
    fields.push(jet_devtools_topology_optional_u64_field(
        "port",
        fact.port.map(u64::from),
    ));
    fields.push(format!(
        "\"authority_state\":{}",
        jet_devtools_topology_json_string(fact.authority_state.as_str())
    ));
    fields.push(jet_devtools_topology_optional_text_field(
        "owner",
        fact.owner.as_deref(),
    ));
    fields.push(format!(
        "\"connection\":{}",
        jet_devtools_topology_json_string(fact.connection.as_str())
    ));
    fields.push(jet_devtools_topology_optional_u64_field("bytes_in", fact.bytes_in));
    fields.push(jet_devtools_topology_optional_u64_field("bytes_out", fact.bytes_out));
    fields.push(jet_devtools_topology_optional_policy_field(
        "restart_policy",
        fact.restart_policy.as_ref(),
    ));
    jet_devtools_topology_object(fields)
}

fn jet_devtools_topology_readiness_fields(
    fact: &JetDevtoolsTopologyReadinessFact,
) -> String {
    let mut fields = jet_devtools_topology_common_fields(
        &fact.id,
        &fact.source,
        fact.parent_id.as_deref(),
        fact.freshness,
    );
    fields.push(format!(
        "\"state\":{}",
        jet_devtools_topology_json_string(fact.state.as_str())
    ));
    fields.push(format!(
        "\"reason\":{}",
        jet_devtools_topology_json_string(fact.reason.as_str())
    ));
    jet_devtools_topology_object(fields)
}

fn jet_devtools_topology_common_fields(
    id: &str,
    source: &str,
    parent_id: Option<&str>,
    freshness: JetDevtoolsTopologyFreshness,
) -> Vec<String> {
    vec![
        jet_devtools_topology_text_field("id", id),
        jet_devtools_topology_text_field("source", source),
        jet_devtools_topology_optional_text_field("parent_id", parent_id),
        jet_devtools_topology_text_field("freshness", freshness.state.as_str()),
        format!("\"observed_at_ms\":{}", freshness.observed_at_ms),
    ]
}

fn jet_devtools_topology_restart_history(
    history: &[JetDevtoolsTopologyRestartFact],
) -> String {
    let values = history
        .iter()
        .map(|restart| {
            format!(
                "{{\"at_ms\":{},\"duration_ms\":{},\"reason\":{}}}",
                restart.at_ms,
                restart.duration_ms,
                jet_devtools_topology_json_string(restart.reason.as_str())
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}

fn jet_devtools_topology_text_field(key: &str, value: &str) -> String {
    format!(
        "{}:{}",
        jet_devtools_topology_json_string(key),
        jet_devtools_topology_json_string(value)
    )
}

fn jet_devtools_topology_optional_text_field(
    key: &str,
    value: Option<&str>,
) -> String {
    format!(
        "{}:{}",
        jet_devtools_topology_json_string(key),
        value.map_or_else(|| "null".to_string(), jet_devtools_topology_json_string)
    )
}

fn jet_devtools_topology_optional_u64_field(key: &str, value: Option<u64>) -> String {
    format!(
        "\"{key}\":{}",
        value.map_or_else(|| "null".to_string(), |value| value.to_string())
    )
}
fn jet_devtools_topology_optional_failure_field(
    key: &str,
    value: Option<JetDevtoolsTopologyFailureKind>,
) -> String {
    format!(
        "\"{key}\":{}",
        value.map_or_else(
            || "null".to_string(),
            |value| jet_devtools_topology_json_string(value.as_str()),
        )
    )
}

fn jet_devtools_topology_optional_policy_field(
    key: &str,
    value: Option<&JetDevtoolsTopologyRestartPolicy>,
) -> String {
    format!(
        "\"{key}\":{}",
        value.map_or_else(
            || "null".to_string(),
            JetDevtoolsTopologyRestartPolicy::render_json,
        )
    )
}


fn jet_devtools_topology_object(fields: Vec<String>) -> String {
    format!("{{{}}}", fields.join(","))
}

fn jet_devtools_topology_json_string(value: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(out, "\\u{:04x}", character as u32);
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevtoolsTopologyEndpointDisplay {
    pub scheme: JetDevtoolsTopologyEndpointScheme,
    pub host: String,
    pub port: Option<u16>,
    pub authority_state: JetDevtoolsTopologyEndpointAuthorityState,
}

impl JetDevtoolsTopologyEndpointDisplay {
    pub fn render(&self) -> String {
        let scheme = match self.scheme {
            JetDevtoolsTopologyEndpointScheme::Tcp => "tcp",
            JetDevtoolsTopologyEndpointScheme::Http => "http",
            JetDevtoolsTopologyEndpointScheme::Https => "https",
            JetDevtoolsTopologyEndpointScheme::Unix => "unix",
        };
        if self.scheme == JetDevtoolsTopologyEndpointScheme::Unix {
            return format!("{scheme}://{}", self.host);
        }
        match self.port {
            Some(port) => format!("{scheme}://{}:{port}", self.host),
            None => format!("{scheme}://{}", self.host),
        }
    }
}

impl std::fmt::Display for JetDevtoolsTopologyEndpointDisplay {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str(&self.render())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JetDevtoolsTopologyRowState {
    Supervisor(JetDevtoolsTopologySupervisorState),
    Process(JetDevtoolsTopologyProcessState),
    Task(JetDevtoolsTopologyTaskState),
    Endpoint(JetDevtoolsTopologyEndpointAuthorityState),
    Readiness(JetDevtoolsTopologyReadinessState),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevtoolsTopologyRow {
    pub id: String,
    pub source: String,
    pub parent_id: Option<String>,
    pub kind: JetDevtoolsTopologyNodeKind,
    pub state: JetDevtoolsTopologyRowState,
    pub restart_history: Vec<JetDevtoolsTopologyRestartFact>,
    pub process_id: Option<u64>,
    pub endpoint: Option<JetDevtoolsTopologyEndpointDisplay>,
    pub readiness_reason: Option<JetDevtoolsTopologyReadinessReason>,
    pub owner: Option<String>,
    pub wait_target: Option<String>,
    pub duration_ms: Option<u64>,
    pub failure: Option<JetDevtoolsTopologyFailureKind>,
    pub restart_policy: Option<JetDevtoolsTopologyRestartPolicy>,
    pub connection: Option<JetDevtoolsTopologyEndpointConnectionState>,
    pub bytes_in: Option<u64>,
    pub bytes_out: Option<u64>,
    pub freshness: JetDevtoolsTopologyFreshness,
}

impl JetDevtoolsTopologyRow {
    pub fn restart_count(&self) -> usize {
        self.restart_history.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevtoolsTopologyTreeNode {
    pub row: JetDevtoolsTopologyRow,
    pub children: Vec<JetDevtoolsTopologyTreeNode>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JetDevtoolsTopologyTree {
    pub roots: Vec<JetDevtoolsTopologyTreeNode>,
}

impl JetDevtoolsTopologyTree {
    pub fn roots(&self) -> &[JetDevtoolsTopologyTreeNode] {
        &self.roots
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JetDevtoolsTopologyState {
    facts: std::collections::BTreeMap<String, JetDevtoolsTopologyFact>,
}

impl JetDevtoolsTopologyState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.facts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    pub fn ingest(
        &mut self,
        fact: JetDevtoolsTopologyFact,
    ) -> Result<(), JetDevtoolsTopologyError> {
        jet_devtools_topology_validate_fact(&fact)?;
        let id = fact.id().to_string();
        if self.facts.contains_key(&id) {
            return Err(JetDevtoolsTopologyError::DuplicateIdentity { id });
        }
        if self.facts.len() >= JET_DEVTOOLS_TOPOLOGY_MAX_FACTS {
            return Err(JetDevtoolsTopologyError::TooManyFacts);
        }
        if self.would_create_cycle(&id, fact.parent_id()) {
            return Err(JetDevtoolsTopologyError::Cycle { id });
        }
        self.facts.insert(id, fact);
        Ok(())
    }

    pub fn rows(&self) -> Vec<JetDevtoolsTopologyRow> {
        self.facts
            .values()
            .map(jet_devtools_topology_project_row)
            .collect()
    }

    pub fn tree(
        &self,
        root: Option<&str>,
    ) -> Result<JetDevtoolsTopologyTree, JetDevtoolsTopologyError> {
        self.validate_graph()?;
        if let Some(root_id) = root {
            if !self.facts.contains_key(root_id) {
                return Err(JetDevtoolsTopologyError::UnknownRoot {
                    id: root_id.to_string(),
                });
            }
        }

        let rows = self
            .facts
            .iter()
            .map(|(id, fact)| (id.clone(), jet_devtools_topology_project_row(fact)))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut children = std::collections::BTreeMap::<String, Vec<String>>::new();
        let mut roots = Vec::new();
        for (id, fact) in &self.facts {
            if let Some(parent_id) = fact.parent_id() {
                children
                    .entry(parent_id.to_string())
                    .or_default()
                    .push(id.clone());
            } else {
                roots.push(id.clone());
            }
        }
        for child_ids in children.values_mut() {
            child_ids.sort();
        }
        roots.sort();
        let root_ids = match root {
            Some(root_id) => vec![root_id.to_string()],
            None => roots,
        };
        let tree_roots = root_ids
            .iter()
            .map(|id| jet_devtools_topology_tree_node(id, &rows, &children))
            .collect();
        Ok(JetDevtoolsTopologyTree { roots: tree_roots })
    }

    fn would_create_cycle(&self, child_id: &str, parent_id: Option<&str>) -> bool {
        let mut cursor = parent_id;
        let mut visited = std::collections::BTreeSet::new();
        while let Some(id) = cursor {
            if id == child_id {
                return true;
            }
            if !visited.insert(id.to_string()) {
                return true;
            }
            cursor = self.facts.get(id).and_then(JetDevtoolsTopologyFact::parent_id);
        }
        false
    }

    fn validate_graph(&self) -> Result<(), JetDevtoolsTopologyError> {
        for id in self.facts.keys() {
            let mut visited = std::collections::BTreeSet::new();
            let mut cursor = id.as_str();
            loop {
                if !visited.insert(cursor.to_string()) {
                    return Err(JetDevtoolsTopologyError::Cycle {
                        id: cursor.to_string(),
                    });
                }
                let Some(fact) = self.facts.get(cursor) else {
                    return Err(JetDevtoolsTopologyError::MissingParent {
                        child_id: id.clone(),
                        parent_id: cursor.to_string(),
                    });
                };
                let Some(parent_id) = fact.parent_id() else {
                    break;
                };
                cursor = parent_id;
            }
        }
        Ok(())
    }
    /// Marshal the validated topology graph into the shared Prelude stream.
    /// BTreeMap order keeps reconnects deterministic and graph validation
    /// prevents a partial projection from crossing the protocol boundary.
    pub fn to_protocol_events(
        &self,
        source: impl Into<String>,
    ) -> Result<Vec<JetDevtoolsEvent>, String> {
        self.validate_graph().map_err(|error| error.to_string())?;
        let source = source.into();
        self.facts
            .values()
            .map(|fact| fact.to_protocol_event(source.clone()))
            .collect()
    }

    pub fn protocol_events(
        &self,
        source: impl Into<String>,
    ) -> Result<Vec<JetDevtoolsEvent>, String> {
        self.to_protocol_events(source)
    }
}

fn jet_devtools_topology_tree_node(
    id: &str,
    rows: &std::collections::BTreeMap<String, JetDevtoolsTopologyRow>,
    children: &std::collections::BTreeMap<String, Vec<String>>,
) -> JetDevtoolsTopologyTreeNode {
    let row = rows
        .get(id)
        .expect("validated topology tree identity must exist")
        .clone();
    let children = children
        .get(id)
        .into_iter()
        .flat_map(|ids| ids.iter())
        .map(|child_id| jet_devtools_topology_tree_node(child_id, rows, children))
        .collect();
    JetDevtoolsTopologyTreeNode { row, children }
}

fn jet_devtools_topology_project_row(fact: &JetDevtoolsTopologyFact) -> JetDevtoolsTopologyRow {
    let (
        state,
        restart_history,
        process_id,
        endpoint,
        readiness_reason,
        owner,
        wait_target,
        duration_ms,
        failure,
        restart_policy,
        connection,
        bytes_in,
        bytes_out,
    ) = match fact {
        JetDevtoolsTopologyFact::Supervisor(fact) => (
            JetDevtoolsTopologyRowState::Supervisor(fact.state),
            fact.restart_history.clone(),
            None,
            None,
            None,
            fact.owner.clone(),
            None,
            fact.duration_ms,
            fact.failure,
            fact.restart_policy.clone(),
            None,
            None,
            None,
        ),
        JetDevtoolsTopologyFact::Process(fact) => (
            JetDevtoolsTopologyRowState::Process(fact.state),
            fact.restart_history.clone(),
            fact.pid,
            None,
            None,
            fact.owner.clone(),
            None,
            None,
            None,
            fact.restart_policy.clone(),
            None,
            None,
            None,
        ),
        JetDevtoolsTopologyFact::Task(fact) => (
            JetDevtoolsTopologyRowState::Task(fact.state),
            fact.restart_history.clone(),
            None,
            None,
            None,
            fact.owner.clone(),
            fact.wait_target.clone(),
            fact.duration_ms,
            fact.failure,
            fact.restart_policy.clone(),
            None,
            None,
            None,
        ),
        JetDevtoolsTopologyFact::Endpoint(fact) => (
            JetDevtoolsTopologyRowState::Endpoint(fact.authority_state),
            Vec::new(),
            None,
            Some(fact.display()),
            None,
            fact.owner.clone(),
            None,
            None,
            None,
            fact.restart_policy.clone(),
            Some(fact.connection),
            fact.bytes_in,
            fact.bytes_out,
        ),
        JetDevtoolsTopologyFact::Readiness(fact) => (
            JetDevtoolsTopologyRowState::Readiness(fact.state),
            Vec::new(),
            None,
            None,
            Some(fact.reason),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ),
    };
    JetDevtoolsTopologyRow {
        id: fact.id().to_string(),
        source: fact.source().to_string(),
        parent_id: fact.parent_id().map(str::to_string),
        kind: fact.kind(),
        state,
        restart_history,
        process_id,
        endpoint,
        readiness_reason,
        owner,
        wait_target,
        duration_ms,
        failure,
        restart_policy,
        connection,
        bytes_in,
        bytes_out,
        freshness: fact.freshness(),
    }
}

fn jet_devtools_topology_validate_fact(
    fact: &JetDevtoolsTopologyFact,
) -> Result<(), JetDevtoolsTopologyError> {
    match fact {
        JetDevtoolsTopologyFact::Supervisor(fact) => {
            jet_devtools_topology_validate_supervisor(fact)
        }
        JetDevtoolsTopologyFact::Process(fact) => jet_devtools_topology_validate_process(fact),
        JetDevtoolsTopologyFact::Task(fact) => jet_devtools_topology_validate_task(fact),
        JetDevtoolsTopologyFact::Endpoint(fact) => jet_devtools_topology_validate_endpoint(fact),
        JetDevtoolsTopologyFact::Readiness(fact) => {
            jet_devtools_topology_validate_readiness(fact)
        }
    }
}

fn jet_devtools_topology_validate_common(
    id: &str,
    source: &str,
    parent_id: Option<&str>,
) -> Result<(), JetDevtoolsTopologyError> {
    jet_devtools_topology_validate_text(id, "identity")?;
    jet_devtools_topology_validate_text(source, "source")?;
    if let Some(parent_id) = parent_id {
        jet_devtools_topology_validate_text(parent_id, "parent identity")?;
    }
    Ok(())
}

fn jet_devtools_topology_validate_text(
    text: &str,
    field: &'static str,
) -> Result<(), JetDevtoolsTopologyError> {
    if text.is_empty()
        || text.len() > JET_DEVTOOLS_TOPOLOGY_MAX_TEXT_BYTES
        || text.chars().any(char::is_control)
    {
        return Err(JetDevtoolsTopologyError::InvalidText { field });
    }
    Ok(())
}
fn jet_devtools_topology_validate_optional_text(
    text: Option<&str>,
    field: &'static str,
) -> Result<(), JetDevtoolsTopologyError> {
    if let Some(text) = text {
        jet_devtools_topology_validate_text(text, field)?;
    }
    Ok(())
}

fn jet_devtools_topology_validate_restart_policy(
    policy: &JetDevtoolsTopologyRestartPolicy,
    id: &str,
) -> Result<(), JetDevtoolsTopologyError> {
    if policy.max_restarts == 0
        || policy.max_restarts > JET_DEVTOOLS_TOPOLOGY_MAX_RESTART_BUDGET
        || policy.window_ms == 0
        || policy.window_ms > JET_DEVTOOLS_TOPOLOGY_MAX_RESTART_WINDOW_MS
    {
        return Err(JetDevtoolsTopologyError::InvalidRestartPolicy {
            id: id.to_string(),
        });
    }
    Ok(())
}

fn jet_devtools_topology_validate_optional_policy(
    policy: Option<&JetDevtoolsTopologyRestartPolicy>,
    id: &str,
) -> Result<(), JetDevtoolsTopologyError> {
    if let Some(policy) = policy {
        jet_devtools_topology_validate_restart_policy(policy, id)?;
    }
    Ok(())
}

fn jet_devtools_topology_validate_bytes(
    bytes: Option<u64>,
    id: &str,
) -> Result<(), JetDevtoolsTopologyError> {
    if bytes.is_some_and(|bytes| bytes > JET_DEVTOOLS_TOPOLOGY_MAX_BYTES) {
        return Err(JetDevtoolsTopologyError::InvalidByteCount {
            id: id.to_string(),
        });
    }
    Ok(())
}

fn jet_devtools_topology_validate_restart_history(
    id: &str,
    history: &[JetDevtoolsTopologyRestartFact],
) -> Result<(), JetDevtoolsTopologyError> {
    if history.len() > JET_DEVTOOLS_TOPOLOGY_MAX_RESTARTS {
        return Err(JetDevtoolsTopologyError::InvalidRestartHistory {
            id: id.to_string(),
        });
    }
    Ok(())
}


fn jet_devtools_topology_validate_supervisor(
    fact: &JetDevtoolsTopologySupervisorFact,
) -> Result<(), JetDevtoolsTopologyError> {
    jet_devtools_topology_validate_common(&fact.id, &fact.source, fact.parent_id.as_deref())?;
    jet_devtools_topology_validate_optional_text(fact.owner.as_deref(), "owner")?;
    jet_devtools_topology_validate_optional_policy(fact.restart_policy.as_ref(), &fact.id)?;
    jet_devtools_topology_validate_restart_history(&fact.id, &fact.restart_history)
}

fn jet_devtools_topology_validate_process(
    fact: &JetDevtoolsTopologyProcessFact,
) -> Result<(), JetDevtoolsTopologyError> {
    jet_devtools_topology_validate_common(&fact.id, &fact.source, fact.parent_id.as_deref())?;
    jet_devtools_topology_validate_optional_text(fact.owner.as_deref(), "owner")?;
    jet_devtools_topology_validate_optional_policy(fact.restart_policy.as_ref(), &fact.id)?;
    jet_devtools_topology_validate_restart_history(&fact.id, &fact.restart_history)
}

fn jet_devtools_topology_validate_task(
    fact: &JetDevtoolsTopologyTaskFact,
) -> Result<(), JetDevtoolsTopologyError> {
    jet_devtools_topology_validate_common(&fact.id, &fact.source, fact.parent_id.as_deref())?;
    jet_devtools_topology_validate_optional_text(fact.owner.as_deref(), "owner")?;
    jet_devtools_topology_validate_optional_text(fact.wait_target.as_deref(), "wait target")?;
    jet_devtools_topology_validate_optional_policy(fact.restart_policy.as_ref(), &fact.id)?;
    jet_devtools_topology_validate_restart_history(&fact.id, &fact.restart_history)
}


fn jet_devtools_topology_validate_endpoint(
    fact: &JetDevtoolsTopologyEndpointFact,
) -> Result<(), JetDevtoolsTopologyError> {
    jet_devtools_topology_validate_common(&fact.id, &fact.source, fact.parent_id.as_deref())?;
    jet_devtools_topology_validate_optional_text(fact.owner.as_deref(), "owner")?;
    jet_devtools_topology_validate_optional_policy(fact.restart_policy.as_ref(), &fact.id)?;
    jet_devtools_topology_validate_bytes(fact.bytes_in, &fact.id)?;
    jet_devtools_topology_validate_bytes(fact.bytes_out, &fact.id)?;
    if fact.host.is_empty()
        || fact.host.len() > JET_DEVTOOLS_TOPOLOGY_MAX_TEXT_BYTES
        || fact.host.chars().any(char::is_control)
        || fact.host.chars().any(|character| {
            character.is_whitespace() || matches!(character, '@' | '?' | '#')
        })
        || (fact.scheme != JetDevtoolsTopologyEndpointScheme::Unix
            && (fact.host.contains('/') || fact.host.contains('\\')))
    {
        return Err(JetDevtoolsTopologyError::InvalidEndpointAddress);
    }
    if fact.scheme == JetDevtoolsTopologyEndpointScheme::Unix && fact.port.is_some() {
        return Err(JetDevtoolsTopologyError::InvalidEndpointAddress);
    }
    Ok(())
}

fn jet_devtools_topology_validate_readiness(
    fact: &JetDevtoolsTopologyReadinessFact,
) -> Result<(), JetDevtoolsTopologyError> {
    jet_devtools_topology_validate_common(&fact.id, &fact.source, fact.parent_id.as_deref())
}

#[cfg(test)]
mod jet_devtools_topology_tests {
    use super::*;

    fn supervisor(id: &str, parent_id: Option<&str>) -> JetDevtoolsTopologyFact {
        JetDevtoolsTopologySupervisorFact::new(
            id,
            "test",
            parent_id.map(str::to_string),
            JetDevtoolsTopologySupervisorState::Running,
            Vec::new(),
            JetDevtoolsTopologyFreshness::fresh(10),
        )
        .unwrap()
        .into()
    }

    #[test]
    fn duplicate_identity_is_rejected_across_fact_kinds() {
        let mut state = JetDevtoolsTopologyState::new();
        state.ingest(supervisor("root", None)).unwrap();
        let duplicate = JetDevtoolsTopologyProcessFact::new(
            "root",
            "test",
            None,
            JetDevtoolsTopologyProcessState::Running,
            Some(7),
            Vec::new(),
            JetDevtoolsTopologyFreshness::fresh(10),
        )
        .unwrap();
        assert_eq!(
            state.ingest(duplicate.into()),
            Err(JetDevtoolsTopologyError::DuplicateIdentity {
                id: "root".to_string()
            })
        );
    }

    #[test]
    fn cycle_is_rejected_deterministically() {
        let mut state = JetDevtoolsTopologyState::new();
        state.ingest(supervisor("a", Some("b"))).unwrap();
        let result = state.ingest(supervisor("b", Some("a")));
        assert_eq!(
            result,
            Err(JetDevtoolsTopologyError::Cycle {
                id: "b".to_string()
            })
        );
    }

    #[test]
    fn tree_and_rows_project_the_same_sorted_fact_set() {
        let mut state = JetDevtoolsTopologyState::new();
        state.ingest(supervisor("root", None)).unwrap();
        state
            .ingest(
                JetDevtoolsTopologyTaskFact::new(
                    "task",
                    "test",
                    Some("root".to_string()),
                    JetDevtoolsTopologyTaskState::Running,
                    Vec::new(),
                    JetDevtoolsTopologyFreshness::fresh(11),
                )
                .unwrap()
                .into(),
            )
            .unwrap();
        let rows = state.rows();
        let tree = state.tree(None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(tree.roots.len(), 1);
        assert_eq!(tree.roots[0].row.id, "root");
        assert_eq!(tree.roots[0].children[0].row.id, "task");
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            vec!["root", "task"]
        );
    }

    #[test]
    fn endpoint_display_has_no_authority_proof() {
        let endpoint = JetDevtoolsTopologyEndpointFact::new(
            "api",
            "test",
            None,
            JetDevtoolsTopologyEndpointScheme::Https,
            "api.example.test",
            Some(443),
            JetDevtoolsTopologyEndpointAuthorityState::Verified,
            JetDevtoolsTopologyFreshness::fresh(12),
        )
        .unwrap();
        let display = endpoint.display();
        assert_eq!(display.render(), "https://api.example.test:443");
        assert_eq!(display.authority_state, JetDevtoolsTopologyEndpointAuthorityState::Verified);
    }

    #[test]
    fn invalid_endpoint_userinfo_is_rejected() {
        let result = JetDevtoolsTopologyEndpointFact::new(
            "api",
            "test",
            None,
            JetDevtoolsTopologyEndpointScheme::Https,
            "token@api.example.test",
            Some(443),
            JetDevtoolsTopologyEndpointAuthorityState::Verified,
            JetDevtoolsTopologyFreshness::fresh(12),
        );
        assert_eq!(result, Err(JetDevtoolsTopologyError::InvalidEndpointAddress));
    }
}
