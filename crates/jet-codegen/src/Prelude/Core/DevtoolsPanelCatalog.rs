// D-DX-DEVTOOLS1 / card #2492: one canonical, typed first-party panel
// catalog and projection map.  The catalog owns panel vocabulary and
// availability; the Prelude envelope owns bounded events and metadata.
// Hosts render these projections and never reinterpret event JSON.

/// Stable identity for every first-party devtools panel.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetDevtoolsPanelId {
    Build,
    Routes,
    Queries,
    Mutations,
    Forms,
    Table,
    Store,
    Traces,
    Cost,
    Gates,
    Structure,
    Tests,
    Jobs,
    UiTree,
    Database,
    Topology,
    Game,
    World,
    Profiler,
}

impl JetDevtoolsPanelId {
    pub const COUNT: usize = 19;

    /// The canonical panel order.  This is protocol vocabulary, not a host
    /// rendering preference.
    pub const ALL: [Self; Self::COUNT] = [
        Self::Build,
        Self::Routes,
        Self::Queries,
        Self::Mutations,
        Self::Forms,
        Self::Table,
        Self::Store,
        Self::Traces,
        Self::Cost,
        Self::Gates,
        Self::Structure,
        Self::Tests,
        Self::Jobs,
        Self::UiTree,
        Self::Database,
        Self::Topology,
        Self::Game,
        Self::World,
        Self::Profiler,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Routes => "routes",
            Self::Queries => "queries",
            Self::Mutations => "mutations",
            Self::Forms => "forms",
            Self::Table => "table",
            Self::Store => "store",
            Self::Traces => "traces",
            Self::Cost => "cost",
            Self::Gates => "gates",
            Self::Structure => "structure",
            Self::Tests => "tests",
            Self::Jobs => "jobs",
            Self::UiTree => "ui-tree",
            Self::Database => "database",
            Self::Topology => "topology",
            Self::Game => "game",
            Self::World => "world",
            Self::Profiler => "profiler",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Build => "Build",
            Self::Routes => "Routes",
            Self::Queries => "Queries",
            Self::Mutations => "Mutations",
            Self::Forms => "Forms",
            Self::Table => "Table",
            Self::Store => "Store",
            Self::Traces => "Traces",
            Self::Cost => "Cost",
            Self::Gates => "Gates",
            Self::Structure => "Structure",
            Self::Tests => "Tests",
            Self::Jobs => "Jobs",
            Self::UiTree => "UI tree",
            Self::Database => "Database",
            Self::Topology => "Topology",
            Self::Game => "Game",
            Self::World => "World",
            Self::Profiler => "Profiler",
        }
    }

    pub const fn canonical_order(self) -> u16 {
        match self {
            Self::Build => 1,
            Self::Routes => 2,
            Self::Queries => 3,
            Self::Mutations => 4,
            Self::Forms => 5,
            Self::Table => 6,
            Self::Store => 7,
            Self::Traces => 8,
            Self::Cost => 9,
            Self::Gates => 10,
            Self::Structure => 11,
            Self::Tests => 12,
            Self::Jobs => 13,
            Self::UiTree => 14,
            Self::Database => 15,
            Self::Topology => 16,
            Self::Game => 17,
            Self::World => 18,
            Self::Profiler => 19,
        }
    }
}

/// Host projections accepted by the shared devtools surface.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetDevtoolsHostKind {
    #[default]
    BrowserInApp,
    BrowserWorkbench,
    Terminal,
    Editor,
    NativeOverlay,
    Headless,
}

impl JetDevtoolsHostKind {
    pub const COUNT: usize = 6;

    pub const ALL: [Self; Self::COUNT] = [
        Self::BrowserInApp,
        Self::BrowserWorkbench,
        Self::Terminal,
        Self::Editor,
        Self::NativeOverlay,
        Self::Headless,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BrowserInApp => "browser-in-app",
            Self::BrowserWorkbench => "browser-workbench",
            Self::Terminal => "terminal",
            Self::Editor => "editor",
            Self::NativeOverlay => "native-overlay",
            Self::Headless => "headless",
        }
    }
}

/// Closed authority vocabulary for first-party panel reads and controls.
///
/// A grant is an authority fact, not a string supplied by a host.  The
/// canonical spelling is always `Devtools.<variant>`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetDevtoolsPanelCapability {
    BuildRead,
    SourceRead,
    RequestRead,
    QueryRead,
    MutationObserve,
    FormInspect,
    TableInspect,
    StateInspect,
    TraceRead,
    TelemetryRead,
    CostRead,
    GateRead,
    StructureRead,
    TestRead,
    JobRead,
    UiInspect,
    AccessibilityInspect,
    DatabaseStatsRead,
    DatabaseExplain,
    TopologyRead,
    GameInspect,
    GameControl,
}

impl JetDevtoolsPanelCapability {
    pub const COUNT: usize = 22;

    pub const ALL: [Self; Self::COUNT] = [
        Self::BuildRead,
        Self::SourceRead,
        Self::RequestRead,
        Self::QueryRead,
        Self::MutationObserve,
        Self::FormInspect,
        Self::TableInspect,
        Self::StateInspect,
        Self::TraceRead,
        Self::TelemetryRead,
        Self::CostRead,
        Self::GateRead,
        Self::StructureRead,
        Self::TestRead,
        Self::JobRead,
        Self::UiInspect,
        Self::AccessibilityInspect,
        Self::DatabaseStatsRead,
        Self::DatabaseExplain,
        Self::TopologyRead,
        Self::GameInspect,
        Self::GameControl,
    ];

    /// Return the one canonical authority grant for this capability.
    pub const fn grant(self) -> &'static str {
        match self {
            Self::BuildRead => "Devtools.BuildRead",
            Self::SourceRead => "Devtools.SourceRead",
            Self::RequestRead => "Devtools.RequestRead",
            Self::QueryRead => "Devtools.QueryRead",
            Self::MutationObserve => "Devtools.MutationObserve",
            Self::FormInspect => "Devtools.FormInspect",
            Self::TableInspect => "Devtools.TableInspect",
            Self::StateInspect => "Devtools.StateInspect",
            Self::TraceRead => "Devtools.TraceRead",
            Self::TelemetryRead => "Devtools.TelemetryRead",
            Self::CostRead => "Devtools.CostRead",
            Self::GateRead => "Devtools.GateRead",
            Self::StructureRead => "Devtools.StructureRead",
            Self::TestRead => "Devtools.TestRead",
            Self::JobRead => "Devtools.JobRead",
            Self::UiInspect => "Devtools.UiInspect",
            Self::AccessibilityInspect => "Devtools.AccessibilityInspect",
            Self::DatabaseStatsRead => "Devtools.DatabaseStatsRead",
            Self::DatabaseExplain => "Devtools.DatabaseExplain",
            Self::TopologyRead => "Devtools.TopologyRead",
            Self::GameInspect => "Devtools.GameInspect",
            Self::GameControl => "Devtools.GameControl",
        }
    }

    pub const fn as_str(self) -> &'static str {
        self.grant()
    }
}

/// How a panel decides that its checked facts need a new projection.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetDevtoolsPanelFreshnessPolicy {
    EventDriven,
    TimeCursor,
    Snapshot,
    Frame,
    OnDemand,
}

impl JetDevtoolsPanelFreshnessPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EventDriven => "event-driven",
            Self::TimeCursor => "time-cursor",
            Self::Snapshot => "snapshot",
            Self::Frame => "frame",
            Self::OnDemand => "on-demand",
        }
    }
}

/// One immutable, backend-neutral panel declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetDevtoolsPanelDescriptor {
    pub id: JetDevtoolsPanelId,
    pub title: &'static str,
    pub order: u16,
    pub required_capabilities: &'static [JetDevtoolsPanelCapability],
    /// The primary authority for the panel.  Additional authorities remain in
    /// `required_capabilities` and are checked as a conjunction.
    pub authority: JetDevtoolsPanelCapability,
    pub supported_hosts: &'static [JetDevtoolsHostKind],
    pub freshness: JetDevtoolsPanelFreshnessPolicy,
}

impl JetDevtoolsPanelDescriptor {
    pub const fn new(
        id: JetDevtoolsPanelId,
        title: &'static str,
        order: u16,
        required_capabilities: &'static [JetDevtoolsPanelCapability],
        authority: JetDevtoolsPanelCapability,
        supported_hosts: &'static [JetDevtoolsHostKind],
        freshness: JetDevtoolsPanelFreshnessPolicy,
    ) -> Self {
        Self {
            id,
            title,
            order,
            required_capabilities,
            authority,
            supported_hosts,
            freshness,
        }
    }

    pub fn supports_host(&self, host: JetDevtoolsHostKind) -> bool {
        self.supported_hosts.contains(&host)
    }

    pub fn requires(&self, capability: JetDevtoolsPanelCapability) -> bool {
        self.required_capabilities.contains(&capability)
    }
}

/// A reason a descriptor cannot be projected by one host under one grant set.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetDevtoolsPanelUnavailableReason {
    UnsupportedHost(JetDevtoolsHostKind),
    MissingCapability(JetDevtoolsPanelCapability),
}

impl JetDevtoolsPanelUnavailableReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedHost(_) => "unsupported-host",
            Self::MissingCapability(_) => "missing-capability",
        }
    }

    pub const fn host(self) -> Option<JetDevtoolsHostKind> {
        match self {
            Self::UnsupportedHost(host) => Some(host),
            Self::MissingCapability(_) => None,
        }
    }

    pub const fn capability(self) -> Option<JetDevtoolsPanelCapability> {
        match self {
            Self::UnsupportedHost(_) => None,
            Self::MissingCapability(capability) => Some(capability),
        }
    }
}

/// The closed state of one panel projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetDevtoolsPanelAvailabilityState {
    Available,
    Unavailable(Vec<JetDevtoolsPanelUnavailableReason>),
}

/// One complete projection row.  Unavailable rows are retained instead of
/// being dropped, so a host can explain every missing panel deterministically.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsPanelAvailability {
    pub descriptor: JetDevtoolsPanelDescriptor,
    pub state: JetDevtoolsPanelAvailabilityState,
}

impl JetDevtoolsPanelAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self.state, JetDevtoolsPanelAvailabilityState::Available)
    }

    pub fn unavailable_reasons(&self) -> &[JetDevtoolsPanelUnavailableReason] {
        match &self.state {
            JetDevtoolsPanelAvailabilityState::Available => &[],
            JetDevtoolsPanelAvailabilityState::Unavailable(reasons) => reasons,
        }
    }
}
/// A panel projection is a view over the canonical envelope.  It carries the
/// same event records and cursor metadata for every host; adapters only render
/// this typed row.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsPanelProjection {
    pub availability: JetDevtoolsPanelAvailability,
    pub events: Vec<JetDevtoolsEvent>,
    pub selected: bool,
    pub cursor: Option<JetDevtoolsTimeCursor>,
    pub freshness: JetDevtoolsFreshnessFact,
    pub reset: bool,
    pub truncated: bool,
}

impl JetDevtoolsPanelId {
    fn accepts_event(self, event: &JetDevtoolsEvent) -> bool {
        match self {
            Self::Build => event.kind() == "Build",
            Self::Routes => matches!(
                event.kind(),
                "Route" | "Request" | "Response" | "Access" | "Exception" | "LogLink"
            ),
            Self::Queries => matches!(event.kind(), "Query" | "DatabaseQuery"),
            Self::Mutations => event.kind() == "Mutation",
            Self::Forms => event.kind() == "Form",
            Self::Table => event.kind() == "Table",
            Self::Store => matches!(event.kind(), "Store" | "State"),
            Self::Traces => matches!(
                event.kind(),
                "Trace" | "Frame" | "TraceLink" | "LogLink"
            ),
            Self::Cost => event.kind() == "Cost",
            Self::Gates => event.kind() == "Gate",
            Self::Structure => event.kind() == "Structure",
            Self::Tests => event.kind() == "Test",
            Self::Jobs => event.kind() == "Job",
            Self::UiTree => matches!(event.kind(), "Ui" | "UiTree"),
            Self::Database => matches!(
                event.kind(),
                "Database"
                    | "DatabasePool"
                    | "DatabaseQuery"
                    | "DatabaseExplainRequest"
                    | "DatabaseExplainResult"
            ),
            Self::Topology => matches!(
                event.kind(),
                "Topology" | "Supervisor" | "Process" | "Task" | "Readiness" | "Endpoint"
            ),
            Self::Game => event.kind() == "Game",
            Self::World => event.kind() == "World",
            Self::Profiler => matches!(
                event.kind(),
                "Profile" | "Metric" | "Log" | "Telemetry" | "SlowWork"
            ),
        }
    }
}


/// Structural failures in a catalog are reported before any host projection.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetDevtoolsPanelCatalogError {
    Empty,
    DuplicateId(JetDevtoolsPanelId),
    DuplicateOrder(u16),
    ZeroOrder(JetDevtoolsPanelId),
    EmptyTitle(JetDevtoolsPanelId),
    NoCapabilities(JetDevtoolsPanelId),
    NoSupportedHosts(JetDevtoolsPanelId),
    AuthorityNotRequired {
        id: JetDevtoolsPanelId,
        authority: JetDevtoolsPanelCapability,
    },
    DuplicateCapability {
        id: JetDevtoolsPanelId,
        capability: JetDevtoolsPanelCapability,
    },
    DuplicateHost {
        id: JetDevtoolsPanelId,
        host: JetDevtoolsHostKind,
    },
}

impl std::fmt::Display for JetDevtoolsPanelCatalogError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => output.write_str("devtools panel catalog is empty"),
            Self::DuplicateId(id) => write!(output, "devtools panel id `{}` is duplicated", id.as_str()),
            Self::DuplicateOrder(order) => {
                write!(output, "devtools panel order `{order}` is duplicated")
            }
            Self::ZeroOrder(id) => {
                write!(output, "devtools panel `{}` has zero order", id.as_str())
            }
            Self::EmptyTitle(id) => {
                write!(output, "devtools panel `{}` has an empty title", id.as_str())
            }
            Self::NoCapabilities(id) => {
                write!(output, "devtools panel `{}` has no required authority", id.as_str())
            }
            Self::NoSupportedHosts(id) => {
                write!(output, "devtools panel `{}` has no supported host", id.as_str())
            }
            Self::AuthorityNotRequired { id, authority } => write!(
                output,
                "devtools panel `{}` primary authority `{}` is not required",
                id.as_str(),
                authority.grant()
            ),
            Self::DuplicateCapability { id, capability } => write!(
                output,
                "devtools panel `{}` repeats authority `{}`",
                id.as_str(),
                capability.grant()
            ),
            Self::DuplicateHost { id, host } => write!(
                output,
                "devtools panel `{}` repeats host `{}`",
                id.as_str(),
                host.as_str()
            ),
        }
    }
}

impl std::error::Error for JetDevtoolsPanelCatalogError {}

/// One validated, deterministically ordered first-party panel catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsPanelCatalog {
    descriptors: Vec<JetDevtoolsPanelDescriptor>,
}

impl JetDevtoolsPanelCatalog {
    /// Validate and retain descriptors in ascending stable order.
    pub fn new<I>(descriptors: I) -> Result<Self, JetDevtoolsPanelCatalogError>
    where
        I: IntoIterator<Item = JetDevtoolsPanelDescriptor>,
    {
        let mut descriptors = descriptors.into_iter().collect::<Vec<_>>();
        if descriptors.is_empty() {
            return Err(JetDevtoolsPanelCatalogError::Empty);
        }

        for descriptor in &descriptors {
            if descriptor.order == 0 {
                return Err(JetDevtoolsPanelCatalogError::ZeroOrder(descriptor.id));
            }
            if descriptor.title.is_empty() {
                return Err(JetDevtoolsPanelCatalogError::EmptyTitle(descriptor.id));
            }
            if descriptor.required_capabilities.is_empty() {
                return Err(JetDevtoolsPanelCatalogError::NoCapabilities(descriptor.id));
            }
            if descriptor.supported_hosts.is_empty() {
                return Err(JetDevtoolsPanelCatalogError::NoSupportedHosts(descriptor.id));
            }
            if !descriptor.requires(descriptor.authority) {
                return Err(JetDevtoolsPanelCatalogError::AuthorityNotRequired {
                    id: descriptor.id,
                    authority: descriptor.authority,
                });
            }
            for (index, capability) in descriptor.required_capabilities.iter().enumerate() {
                if descriptor.required_capabilities[index + 1..].contains(capability) {
                    return Err(JetDevtoolsPanelCatalogError::DuplicateCapability {
                        id: descriptor.id,
                        capability: *capability,
                    });
                }
            }
            for (index, host) in descriptor.supported_hosts.iter().enumerate() {
                if descriptor.supported_hosts[index + 1..].contains(host) {
                    return Err(JetDevtoolsPanelCatalogError::DuplicateHost {
                        id: descriptor.id,
                        host: *host,
                    });
                }
            }
        }

        for (index, descriptor) in descriptors.iter().enumerate() {
            if descriptors[index + 1..].iter().any(|other| other.id == descriptor.id) {
                return Err(JetDevtoolsPanelCatalogError::DuplicateId(descriptor.id));
            }
            if descriptors[index + 1..]
                .iter()
                .any(|other| other.order == descriptor.order)
            {
                return Err(JetDevtoolsPanelCatalogError::DuplicateOrder(descriptor.order));
            }
        }

        descriptors.sort_by_key(|descriptor| (descriptor.order, descriptor.id));
        Ok(Self { descriptors })
    }

    /// Build the validated owned view used by availability and projection.
    /// Static host consumers should use `catalog::descriptors()` instead.
    pub fn canonical() -> Result<Self, JetDevtoolsPanelCatalogError> {
        Self::new(catalog::descriptors().iter().copied())
    }

    pub fn descriptors(&self) -> &[JetDevtoolsPanelDescriptor] {
        &self.descriptors
    }

    pub fn get(&self, id: JetDevtoolsPanelId) -> Option<&JetDevtoolsPanelDescriptor> {
        self.descriptors.iter().find(|descriptor| descriptor.id == id)
    }
    /// Resolve a protocol event to its canonical panel without exposing event
    /// family matching to a host adapter.
    pub fn panel_for_event(&self, event: &JetDevtoolsEvent) -> Option<JetDevtoolsPanelId> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.id.accepts_event(event))
            .map(|descriptor| descriptor.id)
    }


    /// Return every descriptor in stable order.  Unauthorized and unsupported
    /// rows remain present with a typed reason; no panel disappears silently.
    pub fn available_for(
        &self,
        host: JetDevtoolsHostKind,
        grants: &[JetDevtoolsPanelCapability],
    ) -> Vec<JetDevtoolsPanelAvailability> {
        self.descriptors
            .iter()
            .map(|descriptor| {
                let mut unavailable = Vec::new();
                if !descriptor.supports_host(host) {
                    unavailable.push(JetDevtoolsPanelUnavailableReason::UnsupportedHost(host));
                }
                unavailable.extend(
                    descriptor
                        .required_capabilities
                        .iter()
                        .filter(|capability| !grants.contains(capability))
                        .copied()
                        .map(JetDevtoolsPanelUnavailableReason::MissingCapability),
                );
                let state = if unavailable.is_empty() {
                    JetDevtoolsPanelAvailabilityState::Available
                } else {
                    JetDevtoolsPanelAvailabilityState::Unavailable(unavailable)
                };
                JetDevtoolsPanelAvailability {
                    descriptor: *descriptor,
                    state,
                }
            })
            .collect()
    }
    /// Project all first-party panels from one envelope.  The event-to-panel
    /// mapping is defined here so hosts never rediscover semantic families
    /// from JSON field names.
    pub fn project(
        &self,
        envelope: &JetDevtoolsEnvelope,
        host: JetDevtoolsHostKind,
        grants: &[JetDevtoolsPanelCapability],
    ) -> Vec<JetDevtoolsPanelProjection> {
        self.available_for(host, grants)
            .into_iter()
            .map(|availability| {
                let id = availability.descriptor.id;
                let events = envelope
                    .events()
                    .filter(|event| id.accepts_event(event))
                    .cloned()
                    .collect();
                let selected = envelope
                    .view()
                    .selection()
                    .is_some_and(|selection| selection.panel_id == id.as_str());
                JetDevtoolsPanelProjection {
                    availability,
                    events,
                    selected,
                    cursor: envelope.view().cursor(),
                    freshness: envelope.freshness,
                    reset: envelope.reset,
                    truncated: envelope.truncated,
                }
            })
            .collect()
    }

}

/// The public module-level entry point used by host assembly.
pub mod catalog {
    /// Return the canonical descriptor slice without constructing a second
    /// catalog or copying host-local panel metadata.
    pub const fn descriptors() -> &'static [super::JetDevtoolsPanelDescriptor] {
        super::JET_DEVTOOLS_PANEL_DESCRIPTORS
    }
    /// Resolve one event with no allocation for host adapters.
    pub fn panel_for_event(
        event: &super::JetDevtoolsEvent,
    ) -> Option<super::JetDevtoolsPanelId> {
        descriptors()
            .iter()
            .find(|descriptor| descriptor.id.accepts_event(event))
            .map(|descriptor| descriptor.id)
    }


    pub fn canonical() -> Result<
        super::JetDevtoolsPanelCatalog,
        super::JetDevtoolsPanelCatalogError,
    > {
        super::JetDevtoolsPanelCatalog::canonical()
    }

    pub fn available_for(
        host: super::JetDevtoolsHostKind,
        grants: &[super::JetDevtoolsPanelCapability],
    ) -> Result<
        Vec<super::JetDevtoolsPanelAvailability>,
        super::JetDevtoolsPanelCatalogError,
    > {
        Ok(canonical()?.available_for(host, grants))
    }
}

const ALL_HOSTS: &[JetDevtoolsHostKind] = &JetDevtoolsHostKind::ALL;

const BUILD_CAPABILITIES: &[JetDevtoolsPanelCapability] =
    &[JetDevtoolsPanelCapability::BuildRead];
const ROUTES_CAPABILITIES: &[JetDevtoolsPanelCapability] =
    &[JetDevtoolsPanelCapability::RequestRead, JetDevtoolsPanelCapability::SourceRead];
const QUERIES_CAPABILITIES: &[JetDevtoolsPanelCapability] =
    &[JetDevtoolsPanelCapability::QueryRead];
const MUTATIONS_CAPABILITIES: &[JetDevtoolsPanelCapability] =
    &[JetDevtoolsPanelCapability::MutationObserve];
const FORMS_CAPABILITIES: &[JetDevtoolsPanelCapability] =
    &[JetDevtoolsPanelCapability::FormInspect];
const TABLE_CAPABILITIES: &[JetDevtoolsPanelCapability] =
    &[JetDevtoolsPanelCapability::TableInspect];
const STORE_CAPABILITIES: &[JetDevtoolsPanelCapability] =
    &[JetDevtoolsPanelCapability::StateInspect];
const TRACES_CAPABILITIES: &[JetDevtoolsPanelCapability] =
    &[JetDevtoolsPanelCapability::TraceRead, JetDevtoolsPanelCapability::TelemetryRead];
const COST_CAPABILITIES: &[JetDevtoolsPanelCapability] = &[JetDevtoolsPanelCapability::CostRead];
const GATES_CAPABILITIES: &[JetDevtoolsPanelCapability] = &[JetDevtoolsPanelCapability::GateRead];
const STRUCTURE_CAPABILITIES: &[JetDevtoolsPanelCapability] =
    &[JetDevtoolsPanelCapability::StructureRead, JetDevtoolsPanelCapability::SourceRead];
const TESTS_CAPABILITIES: &[JetDevtoolsPanelCapability] = &[JetDevtoolsPanelCapability::TestRead];
const JOBS_CAPABILITIES: &[JetDevtoolsPanelCapability] = &[JetDevtoolsPanelCapability::JobRead];
const UI_TREE_CAPABILITIES: &[JetDevtoolsPanelCapability] = &[
    JetDevtoolsPanelCapability::UiInspect,
    JetDevtoolsPanelCapability::AccessibilityInspect,
    JetDevtoolsPanelCapability::SourceRead,
];
const DATABASE_CAPABILITIES: &[JetDevtoolsPanelCapability] = &[
    JetDevtoolsPanelCapability::DatabaseStatsRead,
    JetDevtoolsPanelCapability::DatabaseExplain,
];
const TOPOLOGY_CAPABILITIES: &[JetDevtoolsPanelCapability] =
    &[JetDevtoolsPanelCapability::TopologyRead];
const GAME_CAPABILITIES: &[JetDevtoolsPanelCapability] = &[
    JetDevtoolsPanelCapability::GameInspect,
    JetDevtoolsPanelCapability::GameControl,
];
const WORLD_CAPABILITIES: &[JetDevtoolsPanelCapability] = &[JetDevtoolsPanelCapability::GameInspect];
const PROFILER_CAPABILITIES: &[JetDevtoolsPanelCapability] = &[
    JetDevtoolsPanelCapability::TelemetryRead,
    JetDevtoolsPanelCapability::TraceRead,
];

/// Stable first-party metadata consumed by the canonical projection map.  It
/// carries no host implementation or payload storage.
pub const JET_DEVTOOLS_PANEL_DESCRIPTORS: &[JetDevtoolsPanelDescriptor] = &[
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Build,
        "Build",
        1,
        BUILD_CAPABILITIES,
        JetDevtoolsPanelCapability::BuildRead,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::EventDriven,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Routes,
        "Routes",
        2,
        ROUTES_CAPABILITIES,
        JetDevtoolsPanelCapability::RequestRead,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::EventDriven,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Queries,
        "Queries",
        3,
        QUERIES_CAPABILITIES,
        JetDevtoolsPanelCapability::QueryRead,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::TimeCursor,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Mutations,
        "Mutations",
        4,
        MUTATIONS_CAPABILITIES,
        JetDevtoolsPanelCapability::MutationObserve,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::TimeCursor,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Forms,
        "Forms",
        5,
        FORMS_CAPABILITIES,
        JetDevtoolsPanelCapability::FormInspect,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::Snapshot,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Table,
        "Table",
        6,
        TABLE_CAPABILITIES,
        JetDevtoolsPanelCapability::TableInspect,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::Snapshot,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Store,
        "Store",
        7,
        STORE_CAPABILITIES,
        JetDevtoolsPanelCapability::StateInspect,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::TimeCursor,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Traces,
        "Traces",
        8,
        TRACES_CAPABILITIES,
        JetDevtoolsPanelCapability::TraceRead,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::TimeCursor,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Cost,
        "Cost",
        9,
        COST_CAPABILITIES,
        JetDevtoolsPanelCapability::CostRead,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::TimeCursor,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Gates,
        "Gates",
        10,
        GATES_CAPABILITIES,
        JetDevtoolsPanelCapability::GateRead,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::EventDriven,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Structure,
        "Structure",
        11,
        STRUCTURE_CAPABILITIES,
        JetDevtoolsPanelCapability::StructureRead,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::Snapshot,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Tests,
        "Tests",
        12,
        TESTS_CAPABILITIES,
        JetDevtoolsPanelCapability::TestRead,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::EventDriven,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Jobs,
        "Jobs",
        13,
        JOBS_CAPABILITIES,
        JetDevtoolsPanelCapability::JobRead,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::TimeCursor,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::UiTree,
        "UI tree",
        14,
        UI_TREE_CAPABILITIES,
        JetDevtoolsPanelCapability::UiInspect,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::Snapshot,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Database,
        "Database",
        15,
        DATABASE_CAPABILITIES,
        JetDevtoolsPanelCapability::DatabaseStatsRead,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::OnDemand,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Topology,
        "Topology",
        16,
        TOPOLOGY_CAPABILITIES,
        JetDevtoolsPanelCapability::TopologyRead,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::Snapshot,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Game,
        "Game",
        17,
        GAME_CAPABILITIES,
        JetDevtoolsPanelCapability::GameInspect,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::Frame,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::World,
        "World",
        18,
        WORLD_CAPABILITIES,
        JetDevtoolsPanelCapability::GameInspect,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::Frame,
    ),
    JetDevtoolsPanelDescriptor::new(
        JetDevtoolsPanelId::Profiler,
        "Profiler",
        19,
        PROFILER_CAPABILITIES,
        JetDevtoolsPanelCapability::TelemetryRead,
        ALL_HOSTS,
        JetDevtoolsPanelFreshnessPolicy::Frame,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_catalog_is_ordered_and_unique() {
        let catalog = catalog::canonical().expect("canonical panel catalog");
        assert_eq!(catalog.descriptors().len(), JetDevtoolsPanelId::COUNT);
        assert_eq!(
            catalog
                .descriptors()
                .iter()
                .map(|descriptor| descriptor.id)
                .collect::<Vec<_>>(),
            JetDevtoolsPanelId::ALL
        );
        assert!(catalog
            .descriptors()
            .windows(2)
            .all(|pair| pair[0].order < pair[1].order));
    }

    #[test]
    fn duplicate_ids_and_orders_are_rejected() {
        let descriptor = JET_DEVTOOLS_PANEL_DESCRIPTORS[0];
        assert_eq!(
            JetDevtoolsPanelCatalog::new([descriptor, descriptor]),
            Err(JetDevtoolsPanelCatalogError::DuplicateId(
                JetDevtoolsPanelId::Build
            ))
        );

        let duplicate_order = JetDevtoolsPanelDescriptor::new(
            JetDevtoolsPanelId::Routes,
            "Routes",
            descriptor.order,
            ROUTES_CAPABILITIES,
            JetDevtoolsPanelCapability::RequestRead,
            ALL_HOSTS,
            JetDevtoolsPanelFreshnessPolicy::EventDriven,
        );
        assert_eq!(
            JetDevtoolsPanelCatalog::new([descriptor, duplicate_order]),
            Err(JetDevtoolsPanelCatalogError::DuplicateOrder(1))
        );
    }

    #[test]
    fn availability_keeps_unauthorized_rows() {
        let catalog = catalog::canonical().expect("canonical panel catalog");
        let rows = catalog.available_for(
            JetDevtoolsHostKind::Terminal,
            &[JetDevtoolsPanelCapability::BuildRead],
        );
        assert_eq!(rows.len(), JetDevtoolsPanelId::COUNT);
        assert!(rows[0].is_available());
        assert_eq!(
            rows[1].unavailable_reasons(),
            &[
                JetDevtoolsPanelUnavailableReason::MissingCapability(
                    JetDevtoolsPanelCapability::RequestRead
                ),
                JetDevtoolsPanelUnavailableReason::MissingCapability(
                    JetDevtoolsPanelCapability::SourceRead
                ),
            ]
        );
        assert!(rows
            .iter()
            .any(|row| row.descriptor.id == JetDevtoolsPanelId::Database));
    }

    #[test]
    fn unsupported_host_is_an_explicit_reason() {
        let descriptor = JetDevtoolsPanelDescriptor::new(
            JetDevtoolsPanelId::Build,
            "Build",
            1,
            BUILD_CAPABILITIES,
            JetDevtoolsPanelCapability::BuildRead,
            &[JetDevtoolsHostKind::Headless],
            JetDevtoolsPanelFreshnessPolicy::EventDriven,
        );
        let catalog = JetDevtoolsPanelCatalog::new([descriptor]).expect("catalog");
        let row = catalog
            .available_for(
                JetDevtoolsHostKind::Terminal,
                &[JetDevtoolsPanelCapability::BuildRead],
            )
            .pop()
            .expect("availability row");
        assert_eq!(
            row.unavailable_reasons(),
            &[JetDevtoolsPanelUnavailableReason::UnsupportedHost(
                JetDevtoolsHostKind::Terminal
            )]
        );
    }
}
