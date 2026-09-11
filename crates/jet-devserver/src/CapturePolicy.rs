//! Backend-neutral `jet dev` capture policy and retention facts.
//!
//! This module stops at typed decisions.  Session/CmdDevTools own the terminal
//! prompt and run lifecycle; RecordIndex owns artifact bytes, links, and
//! publication.  No method here reads or writes a filesystem or parses CLI
//! arguments.

use jet_foundation::SHA256::sha256_hex;
use std::fmt;

/// Default project capture budget: 256 MiB across at most 200 indexed records.
pub const DEFAULT_CAPTURE_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
/// Default project capture record count.
pub const DEFAULT_CAPTURE_BUDGET_RECORDS: usize = 200;
/// The phrase a terminal-facing adapter must turn into typed consent.
pub const SENSITIVE_CAPTURE_CONSENT_PHRASE: &str = "capture sensitive";
/// The explicit one-session opt-out spelling owned by the CLI adapter.
pub const NO_CAPTURE_FLAG: &str = "--no-capture";

/// Execution mode used by capture policy.  Development is the safe-capture
/// default; release is deliberately unrecorded unless a later explicit
/// command-level capture path opts in outside this policy.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CaptureMode {
    #[default]
    Development,
    Release,
}

impl CaptureMode {
    pub const fn is_release(self) -> bool {
        matches!(self, Self::Release)
    }

    pub const fn default_capture_enabled(self) -> bool {
        matches!(self, Self::Development)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Development => "dev",
            Self::Release => "release",
        }
    }

    pub const fn development() -> Self {
        Self::Development
    }

    pub const fn release() -> Self {
        Self::Release
    }
}

impl fmt::Display for CaptureMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Source classes relevant to the ratified safe-clock replay split.
///
/// `Clock` is the only source recorded by the safe form.  The other three
/// classes consume ambient values and require one typed sensitive consent for
/// the exact session/source/build and scope.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CaptureSource {
    Clock,
    Randomness,
    RawInput,
    Network,
}

impl CaptureSource {
    /// Exact short reason labels used by the dev status line.
    pub const fn reason_code(self) -> &'static str {
        match self {
            Self::Clock => "Time",
            Self::Randomness => "Rand",
            Self::RawInput => "Input",
            Self::Network => "Net",
        }
    }

    pub const fn is_safe_clock(self) -> bool {
        matches!(self, Self::Clock)
    }

    pub const fn requires_sensitive_consent(self) -> bool {
        !self.is_safe_clock()
    }

    pub const fn as_str(self) -> &'static str {
        self.reason_code()
    }

    pub const fn clock() -> Self {
        Self::Clock
    }

    pub const fn randomness() -> Self {
        Self::Randomness
    }

    pub const fn raw_input() -> Self {
        Self::RawInput
    }

    pub const fn network() -> Self {
        Self::Network
    }
}

impl fmt::Display for CaptureSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.reason_code())
    }
}

/// Project-relative source location attached to an unsafe source fact.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CaptureLocation {
    pub source_id: String,
    pub line: u32,
    pub column: Option<u32>,
}

impl CaptureLocation {
    pub fn new(source_id: impl Into<String>, line: u32) -> Self {
        Self {
            source_id: source_id.into(),
            line,
            column: None,
        }
    }

    pub fn with_column(mut self, column: u32) -> Self {
        self.column = Some(column);
        self
    }

    pub fn render(&self) -> String {
        match self.column {
            Some(column) => format!("{}:{}:{}", self.source_id, self.line, column),
            None => format!("{}:{}", self.source_id, self.line),
        }
    }
}

impl fmt::Display for CaptureLocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render())
    }
}

/// One effect/source fact supplied by sema or the runtime preflight.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CaptureSourceFact {
    pub source: CaptureSource,
    pub location: Option<CaptureLocation>,
}

impl CaptureSourceFact {
    pub fn new(source: CaptureSource, location: Option<CaptureLocation>) -> Self {
        Self { source, location }
    }

    pub fn at(source: CaptureSource, source_id: impl Into<String>, line: u32) -> Self {
        Self::new(source, Some(CaptureLocation::new(source_id, line)))
    }

    /// The exact reason body used in `capture: skipped (...)`.
    pub fn reason_text(&self) -> String {
        match &self.location {
            Some(location) => format!("{} at {}", self.source.reason_code(), location),
            None => self.source.reason_code().to_string(),
        }
    }
}

/// Which replay form an admitted capture uses.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CaptureKind {
    SafeClock,
    Sensitive,
}

impl CaptureKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SafeClock => "safe",
            Self::Sensitive => "sensitive",
        }
    }

    pub const fn is_sensitive(self) -> bool {
        matches!(self, Self::Sensitive)
    }
}

impl fmt::Display for CaptureKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Package-level capture setting from `dev: .{ records: .{ capture: ... } }`.
/// `Default` keeps the mode defaults; `On` explicitly permits dev capture;
/// `Off` disables capture for the package.  Release remains off by default.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PackageCaptureSetting {
    #[default]
    Default,
    On,
    Off,
}

impl PackageCaptureSetting {
    pub const fn allows_development_capture(self) -> bool {
        !matches!(self, Self::Off)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::On => "on",
            Self::Off => "off",
        }
    }
}

/// Typed package records policy.  Parsing `package.jet` belongs to the
/// package/CLI layer; this is the checked value passed into devserver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackageCapturePolicy {
    pub capture: PackageCaptureSetting,
    pub budget: CaptureBudget,
}

impl Default for PackageCapturePolicy {
    fn default() -> Self {
        Self {
            capture: PackageCaptureSetting::Default,
            budget: CaptureBudget::default(),
        }
    }
}

impl PackageCapturePolicy {
    pub const fn new(capture: PackageCaptureSetting, budget: CaptureBudget) -> Self {
        Self { capture, budget }
    }

    pub const fn off() -> Self {
        Self {
            capture: PackageCaptureSetting::Off,
            budget: CaptureBudget::new_unchecked(
                DEFAULT_CAPTURE_BUDGET_BYTES,
                DEFAULT_CAPTURE_BUDGET_RECORDS,
            ),
        }
    }

    pub const fn allows_development_capture(self) -> bool {
        self.capture.allows_development_capture()
    }

    pub fn with_capture(mut self, capture: PackageCaptureSetting) -> Self {
        self.capture = capture;
        self
    }

    pub fn with_budget(mut self, budget: CaptureBudget) -> Self {
        self.budget = budget;
        self
    }
}

/// The fact produced by the CLI adapter for one invocation of `--no-capture`.
/// It is deliberately not a parser or a process-global switch.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NoCaptureOverrideFact {
    pub active: bool,
}

impl NoCaptureOverrideFact {
    pub const fn none() -> Self {
        Self { active: false }
    }

    pub const fn explicit() -> Self {
        Self { active: true }
    }

    pub const fn is_active(self) -> bool {
        self.active
    }

    pub const fn flag(self) -> &'static str {
        NO_CAPTURE_FLAG
    }
}

/// Retention budget applied to record artifacts, not the small index file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureBudget {
    pub max_bytes: u64,
    pub max_records: usize,
}

impl Default for CaptureBudget {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_CAPTURE_BUDGET_BYTES,
            max_records: DEFAULT_CAPTURE_BUDGET_RECORDS,
        }
    }
}

impl CaptureBudget {
    pub const fn new_unchecked(max_bytes: u64, max_records: usize) -> Self {
        Self {
            max_bytes,
            max_records,
        }
    }

    pub fn new(max_bytes: u64, max_records: usize) -> Result<Self, CaptureBudgetError> {
        let budget = Self {
            max_bytes,
            max_records,
        };
        budget.validate()?;
        Ok(budget)
    }

    pub fn validate(self) -> Result<(), CaptureBudgetError> {
        if self.max_bytes == 0 {
            return Err(CaptureBudgetError::ZeroByteLimit);
        }
        if self.max_records == 0 {
            return Err(CaptureBudgetError::ZeroRecordLimit);
        }
        Ok(())
    }

    pub fn plan_eviction<I: RecordIndexRetention>(
        self,
        records: &[I],
        incoming: &I,
    ) -> Result<CaptureEvictionPlan, CaptureBudgetError> {
        plan_eviction(self, records, incoming)
    }

    pub fn plan_current<I: RecordIndexRetention>(
        self,
        records: &[I],
    ) -> Result<CaptureEvictionPlan, CaptureBudgetError> {
        plan_current_eviction(self, records)
    }
}

/// Typed retention failure.  A failed plan never returns a partial plan that
/// could ask RecordIndex to exceed its limits or remove a saved record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptureBudgetError {
    ZeroByteLimit,
    ZeroRecordLimit,
    ArithmeticOverflow,
    EmptyArtifactId,
    DuplicateArtifactId(String),
    IncomingArtifactTooLarge { bytes: u64, limit: u64 },
    NoEvictableRecords {
        retained_bytes: u64,
        retained_records: usize,
        budget: CaptureBudget,
    },
}

impl fmt::Display for CaptureBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroByteLimit => formatter.write_str("capture budget byte limit must be greater than zero"),
            Self::ZeroRecordLimit => formatter.write_str("capture budget record limit must be greater than zero"),
            Self::ArithmeticOverflow => formatter.write_str("capture budget arithmetic overflowed"),
            Self::EmptyArtifactId => formatter.write_str("record artifact id cannot be empty"),
            Self::DuplicateArtifactId(id) => write!(formatter, "record artifact id `{id}` is duplicated"),
            Self::IncomingArtifactTooLarge { bytes, limit } => {
                write!(formatter, "incoming record is {bytes} bytes, over capture budget {limit}")
            }
            Self::NoEvictableRecords {
                retained_bytes,
                retained_records,
                budget,
            } => write!(
                formatter,
                "capture budget cannot retain {retained_records} records/{retained_bytes} bytes within {}/{}",
                budget.max_records, budget.max_bytes
            ),
        }
    }
}

impl std::error::Error for CaptureBudgetError {}

/// The minimum RecordIndex view needed by the policy's deterministic planner.
///
/// The root `RecordIndexEntry` adapter should return its canonical `id`,
/// `recorded_sequence`, `size`, and `saved` fields here.  `evictable` can
/// additionally preserve index-owned rules such as retaining referenced rows;
/// the planner always applies `!saved()` independently.
pub trait RecordIndexRetention {
    fn artifact_id(&self) -> &str;
    fn recorded_sequence(&self) -> u64;
    fn size_bytes(&self) -> u64;
    fn saved(&self) -> bool;

    fn evictable(&self) -> bool {
        !self.saved()
    }
}

/// One deterministic removal fact.  It is a plan, not a mutation or a record
/// replacement; RecordIndex decides whether and when to apply it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureEviction {
    pub artifact_id: String,
    pub recorded_sequence: u64,
    pub size_bytes: u64,
}

/// A complete retention decision that is guaranteed to fit its budget.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureEvictionPlan {
    pub budget: CaptureBudget,
    pub evicted: Vec<CaptureEviction>,
    pub retained_bytes: u64,
    pub retained_records: usize,
}

impl CaptureEvictionPlan {
    pub const fn fits(&self) -> bool {
        self.retained_bytes <= self.budget.max_bytes
            && self.retained_records <= self.budget.max_records
    }

    pub fn evicted_ids(&self) -> impl Iterator<Item = &str> {
        self.evicted.iter().map(|item| item.artifact_id.as_str())
    }
}

/// Plan oldest-first eviction for an existing index plus one candidate row.
/// Ordering is `(recorded_sequence, artifact_id)`, so equal sequence values
/// remain deterministic independent of filesystem or map iteration order.
pub fn plan_eviction<I: RecordIndexRetention>(
    budget: CaptureBudget,
    records: &[I],
    incoming: &I,
) -> Result<CaptureEvictionPlan, CaptureBudgetError> {
    plan_eviction_inner(budget, records, Some(incoming))
}

/// Plan eviction for already-indexed rows without adding a candidate.
pub fn plan_current_eviction<I: RecordIndexRetention>(
    budget: CaptureBudget,
    records: &[I],
) -> Result<CaptureEvictionPlan, CaptureBudgetError> {
    plan_eviction_inner(budget, records, None)
}

fn plan_eviction_inner<I: RecordIndexRetention>(
    budget: CaptureBudget,
    records: &[I],
    incoming: Option<&I>,
) -> Result<CaptureEvictionPlan, CaptureBudgetError> {
    budget.validate()?;
    let mut seen = std::collections::BTreeSet::new();
    let mut retained_bytes = 0u64;
    for record in records {
        let id = record.artifact_id();
        if id.is_empty() {
            return Err(CaptureBudgetError::EmptyArtifactId);
        }
        if !seen.insert(id.to_string()) {
            return Err(CaptureBudgetError::DuplicateArtifactId(id.to_string()));
        }
        retained_bytes = retained_bytes
            .checked_add(record.size_bytes())
            .ok_or(CaptureBudgetError::ArithmeticOverflow)?;
    }

    if let Some(incoming_record) = incoming {
        let id = incoming_record.artifact_id();
        if id.is_empty() {
            return Err(CaptureBudgetError::EmptyArtifactId);
        }
        if !seen.insert(id.to_string()) {
            return Err(CaptureBudgetError::DuplicateArtifactId(id.to_string()));
        }
        let size_bytes = incoming_record.size_bytes();
        if size_bytes > budget.max_bytes {
            return Err(CaptureBudgetError::IncomingArtifactTooLarge {
                bytes: size_bytes,
                limit: budget.max_bytes,
            });
        }
        retained_bytes = retained_bytes
            .checked_add(size_bytes)
            .ok_or(CaptureBudgetError::ArithmeticOverflow)?;
    }

    let incoming_count = if incoming.is_some() { 1 } else { 0 };
    let mut retained_records = records
        .len()
        .checked_add(incoming_count)
        .ok_or(CaptureBudgetError::ArithmeticOverflow)?;
    let mut evictable = records
        .iter()
        .filter(|record| !record.saved() && record.evictable())
        .collect::<Vec<_>>();
    evictable.sort_by(|left, right| {
        left.recorded_sequence()
            .cmp(&right.recorded_sequence())
            .then_with(|| left.artifact_id().cmp(right.artifact_id()))
    });

    let mut evicted = Vec::new();
    for record in evictable {
        if retained_bytes <= budget.max_bytes && retained_records <= budget.max_records {
            break;
        }
        retained_bytes = retained_bytes
            .checked_sub(record.size_bytes())
            .ok_or(CaptureBudgetError::ArithmeticOverflow)?;
        retained_records = retained_records
            .checked_sub(1)
            .ok_or(CaptureBudgetError::ArithmeticOverflow)?;
        evicted.push(CaptureEviction {
            artifact_id: record.artifact_id().to_string(),
            recorded_sequence: record.recorded_sequence(),
            size_bytes: record.size_bytes(),
        });
    }

    if retained_bytes > budget.max_bytes || retained_records > budget.max_records {
        return Err(CaptureBudgetError::NoEvictableRecords {
            retained_bytes,
            retained_records,
            budget,
        });
    }

    let plan = CaptureEvictionPlan {
        budget,
        evicted,
        retained_bytes,
        retained_records,
    };
    debug_assert!(plan.fits());
    Ok(plan)
}

/// The identity a consent and capture receipt are bound to.  A complete
/// identity carries both the live lineage and the record-index key.  The
/// three-field `new` constructor remains a compatibility shorthand for
/// callers that only have a live lineage; adapters creating a receipt should
/// use `exact`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CaptureIdentity {
    pub target_inputs_sha256: String,
    pub tool_version: String,
    pub engine: String,
    pub session_id: String,
    pub source_id: String,
    pub build_id: String,
    pub revision: String,
}

impl CaptureIdentity {
    pub fn new(
        session_id: impl Into<String>,
        source_id: impl Into<String>,
        build_id: impl Into<String>,
    ) -> Self {
        Self {
            target_inputs_sha256: String::new(),
            tool_version: String::new(),
            engine: String::new(),
            session_id: session_id.into(),
            source_id: source_id.into(),
            revision: String::new(),
            build_id: build_id.into(),
        }
    }

    /// Construct the complete identity required by a recorded receipt.
    pub fn exact(
        target_inputs_sha256: impl Into<String>,
        tool_version: impl Into<String>,
        engine: impl Into<String>,
        session_id: impl Into<String>,
        source_id: impl Into<String>,
        build_id: impl Into<String>,
        revision: impl Into<String>,
    ) -> Self {
        Self {
            target_inputs_sha256: target_inputs_sha256.into(),
            tool_version: tool_version.into(),
            engine: engine.into(),
            session_id: session_id.into(),
            source_id: source_id.into(),
            build_id: build_id.into(),
            revision: revision.into(),
        }
    }

    pub fn is_exact(&self) -> bool {
        [
            &self.target_inputs_sha256,
            &self.tool_version,
            &self.engine,
            &self.session_id,
            &self.source_id,
            &self.build_id,
            &self.revision,
        ]
        .iter()
        .all(|value| !value.is_empty() && !value.chars().any(char::is_control))
    }

    pub fn matches(&self, other: &Self) -> bool {
        self == other
    }
}

/// The exact non-identity scope that a sensitive consent covers.  Vectors are
/// canonicalized on construction so an order-only change cannot spuriously
/// invalidate a grant, while any value change does invalidate it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureConsentScope {
    pub authorities: Vec<String>,
    pub hosts: Vec<String>,
    pub streams: Vec<String>,
    pub disclosures: Vec<String>,
    pub destination: String,
    pub byte_limit: u64,
    pub mode: CaptureMode,
    pub source_kinds: Vec<CaptureSource>,
}

impl Default for CaptureConsentScope {
    fn default() -> Self {
        Self {
            authorities: Vec::new(),
            hosts: Vec::new(),
            streams: Vec::new(),
            disclosures: Vec::new(),
            destination: String::new(),
            byte_limit: DEFAULT_CAPTURE_BUDGET_BYTES,
            mode: CaptureMode::Development,
            source_kinds: Vec::new(),
        }
    }
}

impl CaptureConsentScope {
    pub fn new(
        authorities: Vec<String>,
        hosts: Vec<String>,
        streams: Vec<String>,
        disclosures: Vec<String>,
        destination: impl Into<String>,
        byte_limit: u64,
    ) -> Self {
        let mut scope = Self {
            authorities,
            hosts,
            streams,
            disclosures,
            destination: destination.into(),
            byte_limit,
            ..Self::default()
        };
        scope.canonicalize();
        scope
    }

    pub fn for_budget(budget: CaptureBudget) -> Self {
        Self {
            byte_limit: budget.max_bytes,
            ..Self::default()
        }
    }

    fn canonicalize(&mut self) {
        for values in [
            &mut self.authorities,
            &mut self.hosts,
            &mut self.streams,
            &mut self.disclosures,
        ] {
            values.sort();
            values.dedup();
        }
        self.source_kinds.sort();
        self.source_kinds.dedup();
    }

    fn canonical_text(&self) -> String {
        let mut text = String::from("capture-consent-scope-v1\0");
        frame_vec(&mut text, "authorities", &self.authorities);
        frame_vec(&mut text, "hosts", &self.hosts);
        frame_vec(&mut text, "streams", &self.streams);
        frame_vec(&mut text, "disclosures", &self.disclosures);
        frame(&mut text, "destination", &self.destination);
        frame(&mut text, "byte_limit", &self.byte_limit.to_string());
        frame(&mut text, "mode", self.mode.as_str());
        let source_names = self
            .source_kinds
            .iter()
            .map(|source| source.as_str().to_string())
            .collect::<Vec<_>>();
        frame_vec(&mut text, "source_kinds", &source_names);
        text
    }

    /// Stable digest used to bind a typed consent to this exact scope.
    pub fn digest(&self) -> String {
        sha256_hex(self.canonical_text().as_bytes())
    }
}

fn frame(output: &mut String, key: &str, value: &str) {
    output.push_str(key);
    output.push(':');
    output.push_str(&value.len().to_string());
    output.push(':');
    output.push_str(value);
    output.push(';');
}

fn frame_vec(output: &mut String, key: &str, values: &[String]) {
    frame(output, &format!("{key}.count"), &values.len().to_string());
    for (index, value) in values.iter().enumerate() {
        frame(output, &format!("{key}.{index}"), value);
    }
}

/// A typed sensitive grant.  The terminal adapter creates one only after the
/// user enters the ratified phrase; policy checks its full binding on every
/// run.  The private scope prevents accidental mutation after creation.
#[derive(Clone, Eq, PartialEq)]
pub struct SensitiveCaptureConsent {
    identity: CaptureIdentity,
    scope: CaptureConsentScope,
}

impl fmt::Debug for SensitiveCaptureConsent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SensitiveCaptureConsent")
            .field("identity", &self.identity)
            .field("digest", &self.digest())
            .finish()
    }
}

impl SensitiveCaptureConsent {
    pub fn new(identity: CaptureIdentity, mut scope: CaptureConsentScope) -> Self {
        scope.canonicalize();
        Self { identity, scope }
    }

    pub fn for_attempt(attempt: &CaptureAttempt) -> Self {
        Self::new(attempt.identity.clone(), attempt.base_consent_scope())
    }

    pub fn for_policy(policy: &CapturePolicy, attempt: &CaptureAttempt) -> Self {
        Self::new(attempt.identity.clone(), policy.consent_scope(attempt))
    }

    pub fn identity(&self) -> &CaptureIdentity {
        &self.identity
    }

    pub fn scope(&self) -> &CaptureConsentScope {
        &self.scope
    }

    pub fn digest(&self) -> String {
        let mut text = String::from("capture-sensitive-consent-v1\0");
        frame(&mut text, "target_inputs_sha256", &self.identity.target_inputs_sha256);
        frame(&mut text, "tool_version", &self.identity.tool_version);
        frame(&mut text, "engine", &self.identity.engine);
        frame(&mut text, "session_id", &self.identity.session_id);
        frame(&mut text, "source_id", &self.identity.source_id);
        frame(&mut text, "build_id", &self.identity.build_id);
        frame(&mut text, "revision", &self.identity.revision);
        frame(&mut text, "scope", &self.scope.digest());
        sha256_hex(text.as_bytes())
    }

    pub fn matches(&self, identity: &CaptureIdentity, scope: &CaptureConsentScope) -> bool {
        self.identity == *identity && self.scope == *scope
    }
}

/// The per-run facts supplied to policy.  It carries no CLI spelling and no
/// terminal handle; `terminal_available` is a fact supplied by CmdDevTools.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureAttempt {
    pub identity: CaptureIdentity,
    pub sources: Vec<CaptureSourceFact>,
    pub scope: CaptureConsentScope,
    pub consent: Option<SensitiveCaptureConsent>,
    pub terminal_available: bool,
}

impl CaptureAttempt {
    pub fn new(identity: CaptureIdentity) -> Self {
        Self {
            identity,
            sources: Vec::new(),
            scope: CaptureConsentScope::default(),
            consent: None,
            terminal_available: true,
        }
    }

    pub fn safe_clock(identity: CaptureIdentity) -> Self {
        Self::new(identity).with_source(CaptureSourceFact::new(CaptureSource::Clock, None))
    }

    pub fn with_sources(mut self, sources: Vec<CaptureSourceFact>) -> Self {
        self.sources = sources;
        self
    }

    pub fn with_source(mut self, source: CaptureSourceFact) -> Self {
        self.sources.push(source);
        self
    }

    pub fn with_scope(mut self, mut scope: CaptureConsentScope) -> Self {
        scope.canonicalize();
        self.scope = scope;
        self
    }

    pub fn with_consent(mut self, consent: SensitiveCaptureConsent) -> Self {
        self.consent = Some(consent);
        self
    }

    pub fn without_terminal(mut self) -> Self {
        self.terminal_available = false;
        self
    }

    pub fn base_consent_scope(&self) -> CaptureConsentScope {
        let mut scope = self.scope.clone();
        scope.source_kinds = self.sources.iter().map(|fact| fact.source).collect();
        scope.canonicalize();
        scope
    }
}
/// A capture rejection.  Unsafe-source reasons retain the exact source label
/// and optional location needed by the status line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptureSkipReason {
    ReleaseMode,
    PackagePolicy,
    NoCaptureOverride,
    Unavailable(String),
    UnsafeSource(CaptureSourceFact),
    SensitiveConsentScopeChanged(CaptureSourceFact),
}

impl CaptureSkipReason {
    pub fn source(&self) -> Option<&CaptureSourceFact> {
        match self {
            Self::UnsafeSource(source) | Self::SensitiveConsentScopeChanged(source) => Some(source),
            Self::ReleaseMode | Self::PackagePolicy | Self::NoCaptureOverride | Self::Unavailable(_) => {
                None
            }
        }
    }

    pub const fn code(&self) -> &'static str {
        match self {
            Self::ReleaseMode => "release",
            Self::PackagePolicy => "package_policy",
            Self::NoCaptureOverride => "no_capture",
            Self::Unavailable(_) => "unavailable",
            Self::UnsafeSource(source) => source.source.reason_code(),
            Self::SensitiveConsentScopeChanged(_) => "consent_scope_changed",
        }
    }

    pub fn reason_text(&self) -> String {
        match self {
            Self::ReleaseMode => "release mode".to_string(),
            Self::PackagePolicy => "package policy".to_string(),
            Self::NoCaptureOverride => "no-capture override".to_string(),
            Self::Unavailable(reason) => format!("unavailable: {reason}"),
            Self::UnsafeSource(source) | Self::SensitiveConsentScopeChanged(source) => {
                source.reason_text()
            }
        }
    }
}

impl fmt::Display for CaptureSkipReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason_text())
    }
}

/// Full status fact for a skipped capture, including the terminal distinction
/// required for non-TTY dev sessions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureSkip {
    pub reason: CaptureSkipReason,
    pub terminal_available: bool,
}

impl CaptureSkip {
    pub fn requires_sensitive_consent(&self) -> bool {
        matches!(
            self.reason,
            CaptureSkipReason::UnsafeSource(_) | CaptureSkipReason::SensitiveConsentScopeChanged(_)
        )
    }

    pub fn reason_text(&self) -> String {
        self.reason.reason_text()
    }

    /// Exact human status line for Session/CmdDevTools.
    pub fn status_line(&self) -> String {
        match &self.reason {
            CaptureSkipReason::UnsafeSource(source) if !self.terminal_available => {
                format!("capture: skipped ({}); no terminal to consent", source.reason_text())
            }
            CaptureSkipReason::UnsafeSource(source) => format!(
                "capture: skipped ({}); use --capture-sensitive",
                source.reason_text()
            ),
            CaptureSkipReason::SensitiveConsentScopeChanged(source) => format!(
                "capture: skipped ({}); consent scope changed; use --capture-sensitive",
                source.reason_text()
            ),
            reason => format!("capture: skipped ({})", reason.reason_text()),
        }
    }
}

/// Result of one policy preflight.  Safe clock and sensitive captures carry a
/// budget; skipped captures carry a typed, renderable reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptureEligibility {
    SafeClock { budget: CaptureBudget },
    Sensitive { budget: CaptureBudget },
    Skipped(CaptureSkip),
}

impl CaptureEligibility {
    pub const fn is_allowed(&self) -> bool {
        !matches!(self, Self::Skipped(_))
    }

    pub const fn is_sensitive(&self) -> bool {
        matches!(self, Self::Sensitive { .. })
    }

    pub const fn kind(&self) -> Option<CaptureKind> {
        match self {
            Self::SafeClock { .. } => Some(CaptureKind::SafeClock),
            Self::Sensitive { .. } => Some(CaptureKind::Sensitive),
            Self::Skipped(_) => None,
        }
    }

    pub const fn budget(&self) -> Option<CaptureBudget> {
        match self {
            Self::SafeClock { budget } | Self::Sensitive { budget } => Some(*budget),
            Self::Skipped(_) => None,
        }
    }

    pub fn skip(&self) -> Option<&CaptureSkip> {
        match self {
            Self::Skipped(skip) => Some(skip),
            Self::SafeClock { .. } | Self::Sensitive { .. } => None,
        }
    }
}


/// The sole policy decision boundary for dev capture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapturePolicy {
    pub mode: CaptureMode,
    pub package: PackageCapturePolicy,
    pub no_capture: NoCaptureOverrideFact,
    pub explicit_capture: bool,
}

impl Default for CapturePolicy {
    fn default() -> Self {
        Self::development()
    }
}

impl CapturePolicy {
    pub const fn new(
        mode: CaptureMode,
        package: PackageCapturePolicy,
        no_capture: NoCaptureOverrideFact,
    ) -> Self {
        Self {
            mode,
            package,
            no_capture,
            explicit_capture: false,
        }
    }

    pub const fn development() -> Self {
        Self::new(
            CaptureMode::Development,
            PackageCapturePolicy::new(
                PackageCaptureSetting::Default,
                CaptureBudget::new_unchecked(
                    DEFAULT_CAPTURE_BUDGET_BYTES,
                    DEFAULT_CAPTURE_BUDGET_RECORDS,
                ),
            ),
            NoCaptureOverrideFact::none(),
        )
    }

    pub const fn release() -> Self {
        Self::new(
            CaptureMode::Release,
            PackageCapturePolicy::new(
                PackageCaptureSetting::Default,
                CaptureBudget::new_unchecked(
                    DEFAULT_CAPTURE_BUDGET_BYTES,
                    DEFAULT_CAPTURE_BUDGET_RECORDS,
                ),
            ),
            NoCaptureOverrideFact::none(),
        )
    }

    pub const fn budget(self) -> CaptureBudget {
        self.package.budget
    }

    pub const fn capture_enabled_by_default(self) -> bool {
        self.mode.default_capture_enabled()
            && self.package.allows_development_capture()
            && !self.no_capture.active
    }

    pub fn with_no_capture(mut self, fact: NoCaptureOverrideFact) -> Self {
        self.no_capture = fact;
        self
    }

    pub const fn with_explicit_capture(mut self, explicit: bool) -> Self {
        self.explicit_capture = explicit;
        self
    }

    pub fn with_package(mut self, package: PackageCapturePolicy) -> Self {
        self.package = package;
        self
    }

    /// Fill policy-owned consent fields into the attempt's scope.  A consent
    /// created from this exact result is bound to mode, source classes, budget,
    /// authorities, hosts, streams, disclosures, and destination.
    pub fn consent_scope(&self, attempt: &CaptureAttempt) -> CaptureConsentScope {
        let mut scope = attempt.base_consent_scope();
        scope.mode = self.mode;
        scope.byte_limit = self.budget().max_bytes;
        scope.canonicalize();
        scope
    }

    /// Evaluate one run without prompting or touching the index.
    pub fn eligibility(&self, attempt: &CaptureAttempt) -> CaptureEligibility {
        if self.no_capture.active {
            return CaptureEligibility::Skipped(CaptureSkip {
                reason: CaptureSkipReason::NoCaptureOverride,
                terminal_available: attempt.terminal_available,
            });
        }
        if matches!(self.package.capture, PackageCaptureSetting::Off) {
            return CaptureEligibility::Skipped(CaptureSkip {
                reason: CaptureSkipReason::PackagePolicy,
                terminal_available: attempt.terminal_available,
            });
        }
        if self.mode.is_release() && !self.explicit_capture {
            return CaptureEligibility::Skipped(CaptureSkip {
                reason: CaptureSkipReason::ReleaseMode,
                terminal_available: attempt.terminal_available,
            });
        }

        let unsafe_source = attempt
            .sources
            .iter()
            .find(|fact| fact.source.requires_sensitive_consent())
            .cloned();
        let Some(unsafe_source) = unsafe_source else {
            return CaptureEligibility::SafeClock {
                budget: self.budget(),
            };
        };

        if !attempt.terminal_available {
            return CaptureEligibility::Skipped(CaptureSkip {
                reason: CaptureSkipReason::UnsafeSource(unsafe_source),
                terminal_available: false,
            });
        }

        let expected_scope = self.consent_scope(attempt);
        if attempt
            .consent
            .as_ref()
            .is_some_and(|consent| consent.matches(&attempt.identity, &expected_scope))
        {
            CaptureEligibility::Sensitive {
                budget: self.budget(),
            }
        } else {
            let reason = if attempt.consent.is_some() {
                CaptureSkipReason::SensitiveConsentScopeChanged(unsafe_source)
            } else {
                CaptureSkipReason::UnsafeSource(unsafe_source)
            };
            CaptureEligibility::Skipped(CaptureSkip {
                reason,
                terminal_available: true,
            })
        }
    }

    pub fn start_fact(&self, attempt: &CaptureAttempt) -> CaptureStartFact {
        CaptureStartFact::new(*self, attempt)
    }
}

/// Start receipt facts emitted once per dev session/run boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureStartFact {
    pub identity: CaptureIdentity,
    pub mode: CaptureMode,
    pub package: PackageCapturePolicy,
    pub no_capture: NoCaptureOverrideFact,
    pub explicit_capture: bool,
    pub budget: CaptureBudget,
    pub eligibility: CaptureEligibility,
}

impl CaptureStartFact {
    pub fn new(policy: CapturePolicy, attempt: &CaptureAttempt) -> Self {
        Self {
            identity: attempt.identity.clone(),
            mode: policy.mode,
            package: policy.package,
            no_capture: policy.no_capture,
            explicit_capture: policy.explicit_capture,
            budget: policy.budget(),
            eligibility: policy.eligibility(attempt),
        }
    }

    pub fn capture_kind(&self) -> Option<CaptureKind> {
        self.eligibility.kind()
    }

    pub fn fault(
        &self,
        fault_code: impl Into<String>,
        location: Option<CaptureLocation>,
    ) -> CaptureFaultFact {
        CaptureFaultFact::new(self, fault_code, location)
    }
}

/// Fault receipt facts contain only the typed fault anchor and capture outcome;
/// the fault payload itself remains owned by the artifact codec/index layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureFaultFact {
    pub identity: CaptureIdentity,
    pub mode: CaptureMode,
    pub fault_code: String,
    pub location: Option<CaptureLocation>,
    pub eligibility: CaptureEligibility,
}

impl CaptureFaultFact {
    pub fn new(
        start: &CaptureStartFact,
        fault_code: impl Into<String>,
        location: Option<CaptureLocation>,
    ) -> Self {
        Self {
            identity: start.identity.clone(),
            mode: start.mode,
            fault_code: fault_code.into(),
            location,
            eligibility: start.eligibility.clone(),
        }
    }

    pub fn captured(&self) -> bool {
        self.eligibility.is_allowed()
    }

    pub fn capture_kind(&self) -> Option<CaptureKind> {
        self.eligibility.kind()
    }
}

/// Optional sink contract for the Session/CmdDevTools integration.  This
/// trait has no filesystem behavior: a concrete adapter may forward the facts
/// to resident Session state and then construct a RecordIndexEntry.  The
/// RecordIndex side maps `CaptureKind::SafeClock/Sensitive` to its canonical
/// `safe/sensitive` field and maps retention through `RecordIndexRetention`.
pub trait RecordIndexCaptureSink {
    type Error;

    fn record_capture_start(&mut self, fact: &CaptureStartFact) -> Result<(), Self::Error>;
    fn record_capture_fault(&mut self, fact: &CaptureFaultFact) -> Result<(), Self::Error>;
}

