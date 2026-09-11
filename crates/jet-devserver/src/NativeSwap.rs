//! Backend-neutral facts and transaction state for a resident native swap.
//!
//! This module deliberately stops before a linker, socket, scheduler, or
//! operating-system adapter.  A compiler/sema producer supplies the source,
//! revision, compatibility, relocation, and link facts.  A native host can
//! then consume the checked plan without re-deciding what an edit means.
//!
//! A swap is a small compare-and-commit transaction:
//!
//! 1. [`NativeSwapState::pause`] validates every fact while the old executable
//!    is still live and records a checkpoint.
//! 2. [`NativeSwapState::apply`] installs only the already validated candidate
//!    and only the listener/connection handles named by the plan.
//! 3. [`NativeSwapState::commit`] drops the checkpoint and publishes a receipt.
//!    [`NativeSwapState::rollback`] restores the checkpoint exactly.
//!
//! No method waits for quiescence or performs a native operation.  Quiescence,
//! relocation, and link strategy are facts that an adapter must prove before it
//! hands a plan to this boundary.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use jet_foundation::HotSwap::{HotSwapDecision, StateRetentionDecision};

/// Protocol label for a future native devtools/reload projection.
pub const NATIVE_SWAP_PROTOCOL: &str = "jet.native.swap.v1";
/// Maximum bytes accepted for one stable source, handle, symbol, or revision identity.
pub const MAX_NATIVE_SWAP_ID_BYTES: usize = 256;
/// Maximum bytes retained for one human-readable explanation.
pub const MAX_NATIVE_SWAP_DETAIL_BYTES: usize = 1024;
/// Maximum number of typed handle or changed-item facts in one plan.
pub const MAX_NATIVE_SWAP_FACTS: usize = 1024;

/// A source/module identity.  It is kept separate from code and data revisions:
/// a new revision of the same source can be swapped, while a different source
/// cannot borrow its resident executable or handles.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapSourceIdentity {
    /// Stable source identity, normally a project-relative path or sema id.
    pub source_id: String,
    /// Stable module identity within that source.  [`Self::new`] uses the
    /// source id for both fields, which is useful for single-module programs.
    pub module: String,
}

impl NativeSwapSourceIdentity {
    /// Construct a source identity whose source and module have one identity.
    pub fn new(source_id: impl Into<String>) -> Result<Self, NativeSwapError> {
        let source_id = source_id.into();
        Self::new_with_module(source_id.clone(), source_id)
    }

    /// Construct a source identity with separate source and module names.
    pub fn new_with_module(
        source_id: impl Into<String>,
        module: impl Into<String>,
    ) -> Result<Self, NativeSwapError> {
        let source_id = source_id.into();
        let module = module.into();
        if let Err(detail) = validate_identity(&source_id, "source identity") {
            return Err(NativeSwapError::input(source_id, NativeSwapBlockingFact::InvalidIdentity, detail));
        }
        if let Err(detail) = validate_identity(&module, "module identity") {
            return Err(NativeSwapError::input(source_id, NativeSwapBlockingFact::InvalidIdentity, detail));
        }
        Ok(Self { source_id, module })
    }

    /// Return the source identity used in explanations and receipts.
    pub fn canonical(&self) -> String {
        if self.source_id == self.module {
            self.source_id.clone()
        } else {
            format!("{}::{}", self.source_id, self.module)
        }
    }

    /// Return whether two source/module identities match exactly.
    pub fn matches(&self, other: &Self) -> bool {
        self == other
    }
}

impl fmt::Display for NativeSwapSourceIdentity {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(&self.canonical())
    }
}

/// Content/build identity for the code side of a native resident image.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapCodeRevision(String);

impl NativeSwapCodeRevision {
    /// Construct a checked code revision identity.
    pub fn new(value: impl Into<String>) -> Result<Self, NativeSwapError> {
        let value = value.into();
        validate_revision(&value, "code revision")
            .map(|_| Self(value.clone()))
            .map_err(|detail| NativeSwapError::input(value, NativeSwapBlockingFact::InvalidRevision, detail))
    }

    /// Borrow the opaque revision identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NativeSwapCodeRevision {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(&self.0)
    }
}

/// Content/build identity for the data/layout side of a native resident image.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapDataRevision(String);

impl NativeSwapDataRevision {
    /// Construct a checked data revision identity.
    pub fn new(value: impl Into<String>) -> Result<Self, NativeSwapError> {
        let value = value.into();
        validate_revision(&value, "data revision")
            .map(|_| Self(value.clone()))
            .map_err(|detail| NativeSwapError::input(value, NativeSwapBlockingFact::InvalidRevision, detail))
    }

    /// Borrow the opaque revision identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NativeSwapDataRevision {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(&self.0)
    }
}

/// The two revision facts that identify one executable/data image.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapRevision {
    /// Code/TIR/native-symbol revision.
    pub code: NativeSwapCodeRevision,
    /// Data/layout/retained-value revision.
    pub data: NativeSwapDataRevision,
}

impl NativeSwapRevision {
    /// Construct a checked pair of code and data revisions.
    pub fn new(
        code: impl Into<String>,
        data: impl Into<String>,
    ) -> Result<Self, NativeSwapError> {
        Ok(Self {
            code: NativeSwapCodeRevision::new(code)?,
            data: NativeSwapDataRevision::new(data)?,
        })
    }

    /// Construct revisions from numeric generations without making the
    /// generation itself an authority or a native address.
    pub fn from_generations(code: u64, data: u64) -> Result<Self, NativeSwapError> {
        Self::new(code.to_string(), data.to_string())
    }

    /// Borrow the code revision as an opaque string.
    pub fn code_revision(&self) -> &str {
        self.code.as_str()
    }

    /// Borrow the data revision as an opaque string.
    pub fn data_revision(&self) -> &str {
        self.data.as_str()
    }

    /// Return whether the code fact is unchanged.
    pub fn code_unchanged(&self, other: &Self) -> bool {
        self.code == other.code
    }

    /// Return whether the data fact is unchanged.
    pub fn data_unchanged(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

/// An executable image identity held by a resident state.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapExecutable {
    /// Source/module owning this image.
    pub source: NativeSwapSourceIdentity,
    /// Code and data facts for this image.
    pub revision: NativeSwapRevision,
    /// Monotonic resident generation, distinct from source revisions.
    pub generation: u64,
}

impl NativeSwapExecutable {
    /// Construct one executable identity.
    pub fn new(
        source: NativeSwapSourceIdentity,
        revision: NativeSwapRevision,
        generation: u64,
    ) -> Result<Self, NativeSwapError> {
        if let Err(detail) = validate_source_identity(&source, "executable") {
            return Err(NativeSwapError::input(
                source.canonical(),
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        Ok(Self {
            source,
            revision,
            generation,
        })
    }

    /// Return the source identity used by this image.
    pub fn source_id(&self) -> String {
        self.source.canonical()
    }
}

/// Whether a code revision can be installed in place.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeSwapCodePolicy {
    /// Replace the resident code after all other facts pass.
    Replace,
    /// Never replace code in place; the caller must perform a clean restart.
    Restart,
    /// Reject the edit instead of offering a restart path.
    Reject,
}

impl NativeSwapCodePolicy {
    /// Stable report spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Restart => "restart",
            Self::Reject => "reject",
        }
    }
}

/// Whether a data/layout revision can be installed in place.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeSwapDataPolicy {
    /// Keep existing resident data; a changed data revision is not accepted.
    Preserve,
    /// Replace data through an already checked adapter operation.
    Replace,
    /// Apply an already checked migration at the adapter boundary.
    Migrate,
    /// Require a clean restart for changed data.
    Restart,
    /// Reject changed data without a restart path.
    Reject,
}

impl NativeSwapDataPolicy {
    /// Stable report spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Replace => "replace",
            Self::Migrate => "migrate",
            Self::Restart => "restart",
            Self::Reject => "reject",
        }
    }
}

/// Policy for preserving one class of native handles.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeSwapHandlePolicy {
    /// No handle may cross the boundary.
    Never,
    /// A handle crosses only when explicitly named and marked compatible.
    ExplicitCompatible,
}

impl NativeSwapHandlePolicy {
    /// Stable report spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::ExplicitCompatible => "explicit_compatible",
        }
    }
}

/// Whether the caller must prove no work is executing at the swap boundary.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeSwapQuiescencePolicy {
    /// The plan must carry a reached quiescence fact.
    Required,
    /// The capability does not require a quiescence fact for this boundary.
    NotRequired,
}

impl NativeSwapQuiescencePolicy {
    /// Stable report spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::NotRequired => "not_required",
        }
    }
}

/// Abstract relocation choices.  These are facts for a later native adapter,
/// not calls into a linker or address allocator.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeSwapRelocationStrategy {
    /// Existing identities remain valid without moving the resident handles.
    StableIdentity,
    /// New code/data is rebound by its checked semantic identity.
    RebindByIdentity,
    /// Relocation is only valid as part of a clean restart.
    Restart,
    /// The capability cannot account for the relocation.
    Reject,
}

impl NativeSwapRelocationStrategy {
    /// Stable report spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StableIdentity => "stable_identity",
            Self::RebindByIdentity => "rebind_by_identity",
            Self::Restart => "restart",
            Self::Reject => "reject",
        }
    }

    fn permits_in_place(self) -> bool {
        matches!(self, Self::StableIdentity | Self::RebindByIdentity)
    }
}

/// Abstract link choices.  This records what a native adapter must do later;
/// this module never invokes a linker or loads a symbol.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeSwapLinkStrategy {
    /// Existing resident link identity remains valid.
    ReuseResident,
    /// A checked adapter may link the new image before activation.
    Relink,
    /// Linking is only valid across a clean restart.
    Restart,
    /// No compatible link strategy is available.
    Reject,
}

impl NativeSwapLinkStrategy {
    /// Stable report spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReuseResident => "reuse_resident",
            Self::Relink => "relink",
            Self::Restart => "restart",
            Self::Reject => "reject",
        }
    }

    fn permits_in_place(self) -> bool {
        matches!(self, Self::ReuseResident | Self::Relink)
    }
}
impl fmt::Display for NativeSwapRelocationStrategy {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

impl fmt::Display for NativeSwapLinkStrategy {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}


/// Relocation identity facts supplied by the checked producer.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapRelocationFacts {
    /// Chosen relocation strategy.
    pub strategy: NativeSwapRelocationStrategy,
    /// Changed code identities whose relocation was considered.
    pub code_symbols: Vec<String>,
    /// Changed data identities whose relocation was considered.
    pub data_slots: Vec<String>,
}

impl NativeSwapRelocationFacts {
    /// Construct relocation facts with no symbol list.
    pub fn new(strategy: NativeSwapRelocationStrategy) -> Self {
        Self {
            strategy,
            code_symbols: Vec::new(),
            data_slots: Vec::new(),
        }
    }

    /// Attach canonical code identities in deterministic order.
    pub fn with_code_symbols(mut self, symbols: Vec<String>) -> Self {
        self.code_symbols = normalized_facts(symbols);
        self
    }

    /// Attach canonical data identities in deterministic order.
    pub fn with_data_slots(mut self, slots: Vec<String>) -> Self {
        self.data_slots = normalized_facts(slots);
        self
    }
}

impl Default for NativeSwapRelocationFacts {
    fn default() -> Self {
        Self::new(NativeSwapRelocationStrategy::StableIdentity)
    }
}

/// Link identity facts supplied by the checked producer.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapLinkFacts {
    /// Chosen link strategy.
    pub strategy: NativeSwapLinkStrategy,
    /// Opaque resident link identity, not a pointer or OS handle.
    pub resident_identity: String,
    /// Opaque candidate link identity.
    pub candidate_identity: String,
}

/// Descriptive alias matching the capability/plan vocabulary.
pub type NativeSwapLinkStrategyFacts = NativeSwapLinkFacts;

impl NativeSwapLinkFacts {
    /// Construct checked link facts.
    pub fn new(
        strategy: NativeSwapLinkStrategy,
        resident_identity: impl Into<String>,
        candidate_identity: impl Into<String>,
    ) -> Result<Self, NativeSwapError> {
        let resident_identity = resident_identity.into();
        let candidate_identity = candidate_identity.into();
        if let Err(detail) = validate_identity(&resident_identity, "resident link identity") {
            return Err(NativeSwapError::input(
                resident_identity,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if let Err(detail) = validate_identity(&candidate_identity, "candidate link identity") {
            return Err(NativeSwapError::input(
                candidate_identity,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        Ok(Self {
            strategy,
            resident_identity,
            candidate_identity,
        })
    }

    /// Reuse one resident link identity for an unchanged link boundary.
    pub fn resident(identity: impl Into<String>) -> Result<Self, NativeSwapError> {
        let identity = identity.into();
        Self::new(
            NativeSwapLinkStrategy::ReuseResident,
            identity.clone(),
            identity,
        )
    }
}

impl Default for NativeSwapLinkFacts {
    fn default() -> Self {
        // The default only carries opaque labels.  It is valid and does not
        // claim that any linker has run.
        Self {
            strategy: NativeSwapLinkStrategy::ReuseResident,
            resident_identity: "resident".to_string(),
            candidate_identity: "candidate".to_string(),
        }
    }
}

/// Quiescence facts observed before a plan crosses the native boundary.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapQuiescence {
    /// Whether this plan asks for a quiescent boundary.
    pub required: bool,
    /// Number of active/in-flight operations observed by the producer.
    pub active_operations: u64,
    /// Whether the producer has reached the required boundary.
    pub reached: bool,
}

impl NativeSwapQuiescence {
    /// A reached boundary with no active operations.
    pub const fn ready() -> Self {
        Self {
            required: true,
            active_operations: 0,
            reached: true,
        }
    }

    /// A boundary that still has active work and cannot be applied.
    pub const fn pending(active_operations: u64) -> Self {
        Self {
            required: true,
            active_operations,
            reached: false,
        }
    }

    /// A plan for which no quiescence proof is needed.
    pub const fn not_required() -> Self {
        Self {
            required: false,
            active_operations: 0,
            reached: true,
        }
    }

    /// Return whether this fact is sufficient for a required pause.
    pub const fn is_ready(self) -> bool {
        self.reached && self.active_operations == 0
    }
}

/// Why a plan cannot be applied as an in-place native swap.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeSwapBlockingFact {
    /// No fact blocks the in-place operation.
    None,
    /// A source/module identity is malformed or does not match.
    InvalidIdentity,
    /// A code or data revision is malformed or stale.
    InvalidRevision,
    /// Source identity changed between the resident and candidate images.
    SourceIdentityChanged,
    /// Candidate code revision is not accepted by the capability.
    CodeRevisionUnsupported,
    /// Candidate data/layout revision is not accepted by the capability.
    DataRevisionUnsupported,
    /// The checked semantic type surface changed.
    IncompatibleType,
    /// A connection handle belongs to another source or generation.
    ConnectionHandleStale,
    LayoutChanged,
    /// An unsafe edit was offered to the resident boundary.
    UnsafeEdit,
    /// A restart boundary was explicitly required.
    RestartRequired,
    /// A listener handle was not explicitly supplied.
    ListenerHandleMissing,
    /// A listener handle was supplied but marked incompatible.
    ListenerHandleIncompatible,
    /// A listener handle belongs to another source or generation.
    ListenerHandleStale,
    /// A connection handle was not explicitly supplied.
    ConnectionHandleMissing,
    /// A connection handle was supplied but marked incompatible.
    ConnectionHandleIncompatible,
    /// A connection's listener was not explicitly preserved.
    ConnectionListenerNotPreserved,
    /// The plan has duplicate handle or changed-item identity.
    DuplicateIdentity,
    /// The plan has too many typed facts.
    FactLimit,
    /// The boundary was not quiescent.
    QuiescenceRequired,
    /// Relocation facts do not permit in-place activation.
    RelocationUnsupported,
    /// Link facts do not permit in-place activation.
    LinkStrategyUnsupported,
    /// A removed definition would leave an old reference callable.
    StaleReference,
    /// The plan no longer describes the current executable.
    StalePlan,
    /// The transaction method was called in the wrong phase.
    InvalidPhase,
    /// Candidate compilation failed before activation.
    CompileFailed,
    /// Candidate data migration failed before activation.
    MigrationFailed,
    /// No checkpoint remains to restore.
    RollbackUnavailable,
    /// A capability does not offer the requested operation.
    CapabilityUnavailable,
}

impl NativeSwapBlockingFact {
    /// Stable report spelling for `jet explain --reload` and receipts.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::InvalidIdentity => "invalid_identity",
            Self::InvalidRevision => "invalid_revision",
            Self::SourceIdentityChanged => "source_identity_changed",
            Self::CodeRevisionUnsupported => "code_revision_unsupported",
            Self::DataRevisionUnsupported => "data_revision_unsupported",
            Self::IncompatibleType => "incompatible_type",
            Self::ConnectionHandleStale => "connection_handle_stale",
            Self::LayoutChanged => "layout_changed",
            Self::UnsafeEdit => "unsafe_edit",
            Self::RestartRequired => "restart_required",
            Self::ListenerHandleMissing => "listener_handle_missing",
            Self::ListenerHandleIncompatible => "listener_handle_incompatible",
            Self::ListenerHandleStale => "listener_handle_stale",
            Self::ConnectionHandleMissing => "connection_handle_missing",
            Self::ConnectionHandleIncompatible => "connection_handle_incompatible",
            Self::ConnectionListenerNotPreserved => "connection_listener_not_preserved",
            Self::DuplicateIdentity => "duplicate_identity",
            Self::FactLimit => "fact_limit",
            Self::QuiescenceRequired => "quiescence_required",
            Self::RelocationUnsupported => "relocation_unsupported",
            Self::LinkStrategyUnsupported => "link_strategy_unsupported",
            Self::StaleReference => "stale_reference",
            Self::StalePlan => "stale_plan",
            Self::InvalidPhase => "invalid_phase",
            Self::CompileFailed => "compile_failed",
            Self::MigrationFailed => "migration_failed",
            Self::RollbackUnavailable => "rollback_unavailable",
            Self::CapabilityUnavailable => "capability_unavailable",
        }
    }
    /// Return whether a blocking fact can be handled by a clean restart.
    ///
    /// Unsafe edits, stale references, malformed facts, and operational
    /// failures remain hard rejections even when a host supports restarting.
    pub const fn is_restartable(self) -> bool {
        matches!(
            self,
            Self::CodeRevisionUnsupported
                | Self::DataRevisionUnsupported
                | Self::IncompatibleType
                | Self::LayoutChanged
                | Self::RestartRequired
                | Self::RelocationUnsupported
                | Self::LinkStrategyUnsupported
        )
    }

}

impl fmt::Display for NativeSwapBlockingFact {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

/// Whether `jet explain --reload` recommends an in-place swap or a restart.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeSwapDisposition {
    /// The checked plan may be applied in place.
    Swap,
    /// The plan is valid evidence for a clean restart, but this module will
    /// not perform that restart.
    Restart,
    /// The plan is unsafe or malformed and must be rejected.
    Rejected,
}

impl NativeSwapDisposition {
    /// Stable report spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Swap => "swap",
            Self::Restart => "restart",
            Self::Rejected => "rejected",
        }
    }
}
impl fmt::Display for NativeSwapDisposition {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}


/// Exact explanation for one reload decision or failure.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapExplanation {
    /// Source/module identity that owns the decision.
    pub source_id: String,
    /// Whether this is an in-place swap, restart recommendation, or rejection.
    pub disposition: NativeSwapDisposition,
    /// One closed blocking fact; [`NativeSwapBlockingFact::None`] means the
    /// plan is compatible.
    pub blocking_fact: NativeSwapBlockingFact,
    /// Human-readable detail tied to the blocking fact.
    pub detail: String,
}

impl NativeSwapExplanation {
    /// Construct an exact bounded explanation.
    pub fn new(
        source_id: impl Into<String>,
        disposition: NativeSwapDisposition,
        blocking_fact: NativeSwapBlockingFact,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            source_id: bounded_text(&source_id.into(), MAX_NATIVE_SWAP_ID_BYTES),
            disposition,
            blocking_fact,
            detail: bounded_text(&detail.into(), MAX_NATIVE_SWAP_DETAIL_BYTES),
        }
    }

    /// Construct the positive explanation for a compatible plan.
    pub fn compatible(source_id: impl Into<String>) -> Self {
        Self::new(
            source_id,
            NativeSwapDisposition::Swap,
            NativeSwapBlockingFact::None,
            "code/data and explicit resident handles are compatible",
        )
    }

    /// Return a deterministic one-line explanation suitable for a CLI report.
    pub fn render(&self) -> String {
        format!(
            "reload disposition={} source={} blocking_fact={} detail={}",
            self.disposition, self.source_id, self.blocking_fact, self.detail
        )
    }
}

impl fmt::Display for NativeSwapExplanation {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(&self.render())
    }
}

/// Error returned before a native swap mutates resident state.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapError {
    /// Machine-readable explanation of the refusal.
    pub explanation: NativeSwapExplanation,
}

impl NativeSwapError {
    fn input(
        source_id: impl Into<String>,
        blocking_fact: NativeSwapBlockingFact,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            explanation: NativeSwapExplanation::new(
                source_id,
                NativeSwapDisposition::Rejected,
                blocking_fact,
                detail,
            ),
        }
    }

    fn restart(
        source_id: impl Into<String>,
        blocking_fact: NativeSwapBlockingFact,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            explanation: NativeSwapExplanation::new(
                source_id,
                NativeSwapDisposition::Restart,
                blocking_fact,
                detail,
            ),
        }
    }

    /// Borrow the exact explanation.
    pub fn explanation(&self) -> &NativeSwapExplanation {
        &self.explanation
    }

    /// Source identity named by this error.
    pub fn source_id(&self) -> &str {
        &self.explanation.source_id
    }

    /// Blocking fact named by this error.
    pub const fn blocking_fact(&self) -> NativeSwapBlockingFact {
        self.explanation.blocking_fact
    }

    /// Deterministic one-line report.
    pub fn render(&self) -> String {
        self.explanation.render()
    }
}

impl fmt::Display for NativeSwapError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(&self.render())
    }
}

impl std::error::Error for NativeSwapError {}

/// The explicit compatibility fact attached to a listener or connection.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeSwapHandleCompatibility {
    /// This handle may cross a compatible swap when explicitly named.
    Compatible,
    /// This handle must not cross the boundary.
    Incompatible,
}

impl NativeSwapHandleCompatibility {
    /// Stable report spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::Incompatible => "incompatible",
        }
    }

    /// Return whether the handle carries the explicit compatible fact.
    pub const fn is_compatible(self) -> bool {
        matches!(self, Self::Compatible)
    }
}

/// Explicit resident listener identity.  It is only a fact; no socket or
/// listener object is stored here.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapListenerHandle {
    /// Stable listener identity.
    pub id: String,
    /// Source/module owning this listener.
    pub source: NativeSwapSourceIdentity,
    /// Resident generation in which the handle was observed.
    pub generation: u64,
    /// Explicit compatibility fact from the host/producer.
    pub compatibility: NativeSwapHandleCompatibility,
}

impl NativeSwapListenerHandle {
    /// Construct a listener handle with an explicit compatibility fact.
    pub fn new(
        id: impl Into<String>,
        source: NativeSwapSourceIdentity,
        generation: u64,
        compatibility: NativeSwapHandleCompatibility,
    ) -> Result<Self, NativeSwapError> {
        let id = id.into();
        if let Err(detail) = validate_identity(&id, "listener handle identity") {
            return Err(NativeSwapError::input(
                source.canonical(),
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if let Err(detail) = validate_source_identity(&source, "listener") {
            return Err(NativeSwapError::input(
                source.canonical(),
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        Ok(Self {
            id,
            source,
            generation,
            compatibility,
        })
    }

    /// Construct an explicitly compatible listener handle.
    pub fn compatible(
        id: impl Into<String>,
        source: NativeSwapSourceIdentity,
        generation: u64,
    ) -> Result<Self, NativeSwapError> {
        Self::new(id, source, generation, NativeSwapHandleCompatibility::Compatible)
    }

    /// Construct a listener handle that must be rejected if preservation is requested.
    pub fn incompatible(
        id: impl Into<String>,
        source: NativeSwapSourceIdentity,
        generation: u64,
    ) -> Result<Self, NativeSwapError> {
        Self::new(id, source, generation, NativeSwapHandleCompatibility::Incompatible)
    }

    /// Return whether this handle can cross a swap boundary.
    pub const fn is_compatible(&self) -> bool {
        self.compatibility.is_compatible()
    }
}

/// Explicit resident connection identity.  It is only a fact; no stream or
/// operating-system connection is stored here.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapConnectionHandle {
    /// Stable connection identity.
    pub id: String,
    /// Listener identity that accepted this connection.
    pub listener_id: String,
    /// Source/module owning this connection.
    pub source: NativeSwapSourceIdentity,
    /// Resident generation in which the handle was observed.
    pub generation: u64,
    /// Explicit compatibility fact from the host/producer.
    pub compatibility: NativeSwapHandleCompatibility,
}

impl NativeSwapConnectionHandle {
    /// Construct a connection handle with an explicit compatibility fact.
    pub fn new(
        id: impl Into<String>,
        listener_id: impl Into<String>,
        source: NativeSwapSourceIdentity,
        generation: u64,
        compatibility: NativeSwapHandleCompatibility,
    ) -> Result<Self, NativeSwapError> {
        let id = id.into();
        let listener_id = listener_id.into();
        if let Err(detail) = validate_identity(&id, "connection handle identity") {
            return Err(NativeSwapError::input(
                source.canonical(),
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if let Err(detail) = validate_identity(&listener_id, "connection listener identity") {
            return Err(NativeSwapError::input(
                source.canonical(),
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if let Err(detail) = validate_source_identity(&source, "connection") {
            return Err(NativeSwapError::input(
                source.canonical(),
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        Ok(Self {
            id,
            listener_id,
            source,
            generation,
            compatibility,
        })
    }

    /// Construct an explicitly compatible connection handle.
    pub fn compatible(
        id: impl Into<String>,
        listener_id: impl Into<String>,
        source: NativeSwapSourceIdentity,
        generation: u64,
    ) -> Result<Self, NativeSwapError> {
        Self::new(
            id,
            listener_id,
            source,
            generation,
            NativeSwapHandleCompatibility::Compatible,
        )
    }

    /// Construct a connection handle that must be rejected if preservation is requested.
    pub fn incompatible(
        id: impl Into<String>,
        listener_id: impl Into<String>,
        source: NativeSwapSourceIdentity,
        generation: u64,
    ) -> Result<Self, NativeSwapError> {
        Self::new(
            id,
            listener_id,
            source,
            generation,
            NativeSwapHandleCompatibility::Incompatible,
        )
    }

    /// Return whether this handle can cross a swap boundary.
    pub const fn is_compatible(&self) -> bool {
        self.compatibility.is_compatible()
    }
}

/// Host capabilities accepted by a native swap plan.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapCapability {
    /// In-place code policy.
    pub code: NativeSwapCodePolicy,
    /// In-place data policy.
    pub data: NativeSwapDataPolicy,
    /// Listener preservation policy.
    pub listeners: NativeSwapHandlePolicy,
    /// Connection preservation policy.
    pub connections: NativeSwapHandlePolicy,
    /// Whether a clean restart can be recommended for unsupported edits.
    pub restart: bool,
    /// Quiescence policy.
    pub quiescence: NativeSwapQuiescencePolicy,
    /// Capability-level relocation strategy.
    pub relocation: NativeSwapRelocationStrategy,
    /// Capability-level link strategy.
    pub link: NativeSwapLinkStrategy,
}

impl NativeSwapCapability {
    /// Resident native default: code replacement, explicit handle retention,
    /// and a required quiescent boundary.  Data changes remain conservative
    /// until the producer explicitly chooses replacement or migration.
    pub const fn resident() -> Self {
        Self {
            code: NativeSwapCodePolicy::Replace,
            data: NativeSwapDataPolicy::Preserve,
            listeners: NativeSwapHandlePolicy::ExplicitCompatible,
            connections: NativeSwapHandlePolicy::ExplicitCompatible,
            restart: true,
            quiescence: NativeSwapQuiescencePolicy::Required,
            relocation: NativeSwapRelocationStrategy::StableIdentity,
            link: NativeSwapLinkStrategy::ReuseResident,
        }
    }

    /// Capability with no in-place operation.  It can still report restart
    /// explanations without mutating a resident state.
    pub const fn restart_only() -> Self {
        Self {
            code: NativeSwapCodePolicy::Restart,
            data: NativeSwapDataPolicy::Restart,
            listeners: NativeSwapHandlePolicy::Never,
            connections: NativeSwapHandlePolicy::Never,
            restart: true,
            quiescence: NativeSwapQuiescencePolicy::NotRequired,
            relocation: NativeSwapRelocationStrategy::Restart,
            link: NativeSwapLinkStrategy::Restart,
        }
    }

    /// Capability that allows checked replacement of code and data facts.
    pub const fn resident_with_data_replace() -> Self {
        let mut capability = Self::resident();
        capability.data = NativeSwapDataPolicy::Replace;
        capability
    }

    /// Set the code policy.
    pub const fn with_code_policy(mut self, policy: NativeSwapCodePolicy) -> Self {
        self.code = policy;
        self
    }

    /// Set the data policy.
    pub const fn with_data_policy(mut self, policy: NativeSwapDataPolicy) -> Self {
        self.data = policy;
        self
    }

    /// Set listener preservation policy.
    pub const fn with_listener_policy(mut self, policy: NativeSwapHandlePolicy) -> Self {
        self.listeners = policy;
        self
    }

    /// Set connection preservation policy.
    pub const fn with_connection_policy(mut self, policy: NativeSwapHandlePolicy) -> Self {
        self.connections = policy;
        self
    }

    /// Set the quiescence policy.
    pub const fn with_quiescence_policy(mut self, policy: NativeSwapQuiescencePolicy) -> Self {
        self.quiescence = policy;
        self
    }

    /// Set the relocation strategy.
    pub const fn with_relocation(mut self, strategy: NativeSwapRelocationStrategy) -> Self {
        self.relocation = strategy;
        self
    }

    /// Set the link strategy.
    pub const fn with_link(mut self, strategy: NativeSwapLinkStrategy) -> Self {
        self.link = strategy;
        self
    }

    /// Return whether this capability can recommend a restart.
    pub const fn allows_restart(self) -> bool {
        self.restart
    }
}

impl Default for NativeSwapCapability {
    fn default() -> Self {
        Self::resident()
    }
}

/// A clean restart boundary fact.  The native adapter owns the actual restart;
/// this record says why it is needed and what state it may keep/reset.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapRestartBoundary {
    /// Whether the candidate must cross a clean restart boundary.
    pub required: bool,
    /// Machine-readable reason for the boundary.
    pub reason: Option<NativeSwapBlockingFact>,
    /// Source identities affected by the boundary.
    pub affected: Vec<String>,
    /// State names that an eventual restart may keep.
    pub kept: Vec<String>,
    /// State names that an eventual restart must reset.
    pub reset: Vec<String>,
}

impl NativeSwapRestartBoundary {
    /// An in-place boundary with no restart requirement.
    pub fn in_place() -> Self {
        Self {
            required: false,
            reason: None,
            affected: Vec::new(),
            kept: Vec::new(),
            reset: Vec::new(),
        }
    }

    /// A restart boundary for one exact blocking fact.
    pub fn required(reason: NativeSwapBlockingFact, affected: Vec<String>) -> Self {
        Self {
            required: true,
            reason: Some(reason),
            affected: normalized_facts(affected),
            kept: Vec::new(),
            reset: Vec::new(),
        }
    }

    /// Attach state facts to a restart explanation.
    pub fn with_state(mut self, kept: Vec<String>, reset: Vec<String>) -> Self {
        self.kept = normalized_facts(kept);
        self.reset = normalized_facts(reset);
        self
    }

    fn validate(&self, source_id: &str) -> Result<(), NativeSwapError> {
        validate_fact_list(source_id, &self.affected)?;
        validate_fact_list(source_id, &self.kept)?;
        validate_fact_list(source_id, &self.reset)?;
        if self.required && self.reason.is_none() {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::RestartRequired,
                "restart boundary is required but has no blocking fact",
            ));
        }
        if !self.required && self.reason.is_some() {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::RestartRequired,
                "in-place boundary carries a restart reason",
            ));
        }
        if let Some(reason) = self.reason {
            if !reason.is_restartable() {
                return Err(NativeSwapError::input(
                    source_id,
                    reason,
                    format!("restart boundary reason `{reason}` cannot be handled by a clean restart"),
                ));
            }
        }
        if let Some(value) = self
            .reset
            .iter()
            .find(|value| self.kept.iter().any(|kept| kept == *value))
        {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::DuplicateIdentity,
                format!("restart state `{value}` is both kept and reset"),
            ));
        }
        Ok(())
    }
}

impl Default for NativeSwapRestartBoundary {
    fn default() -> Self {
        Self::in_place()
    }
}

/// Code/data and host facts for one candidate resident swap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeSwapPlan {
    /// Source/module whose resident image is being changed.
    pub source: NativeSwapSourceIdentity,
    /// Executable identity expected before activation.
    pub from: NativeSwapExecutable,
    /// Executable identity expected after activation.
    pub to: NativeSwapExecutable,
    /// Sema-owned compatibility and canonical `#Persist` retention verdict.
    pub decision: HotSwapDecision,
    /// Host capabilities used for preflight.
    pub capability: NativeSwapCapability,
    /// Explicit listener handles allowed to cross the boundary.
    pub preserve_listeners: Vec<NativeSwapListenerHandle>,
    /// Explicit connection handles allowed to cross the boundary.
    pub preserve_connections: Vec<NativeSwapConnectionHandle>,
    /// Restart explanation if the edit is not in-place compatible.
    pub restart_boundary: NativeSwapRestartBoundary,
    /// Quiescence fact supplied by the producer.
    pub quiescence: NativeSwapQuiescence,
    /// Relocation facts supplied by the producer.
    pub relocation: NativeSwapRelocationFacts,
    /// Link facts supplied by the producer.
    pub link: NativeSwapLinkFacts,
    /// Changed semantic identities, in deterministic order.
    pub changed_items: Vec<String>,
    /// Removed definitions that must invalidate old references.
    pub removed_definitions: Vec<String>,
    /// An explicit checked blocking fact, when the producer knows the edit is
    /// unsafe/incompatible before native preflight.
    pub blocking_fact: Option<NativeSwapBlockingFact>,
}

impl NativeSwapPlan {
    /// Construct a plan from the sema-owned compatibility and retention
    /// verdict. Native policy and handle facts are only applied after this
    /// semantic input has been accepted.
    pub fn new(
        source: NativeSwapSourceIdentity,
        from: NativeSwapExecutable,
        to: NativeSwapExecutable,
        capability: NativeSwapCapability,
        decision: HotSwapDecision,
    ) -> Result<Self, NativeSwapError> {
        let source_id = source.canonical();
        validate_hot_swap_decision(&source_id, &source, &decision)?;
        if !from.source.matches(&source) || !to.source.matches(&source) {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::SourceIdentityChanged,
                "plan source does not match both executable identities",
            ));
        }
        if to.generation <= from.generation {
            return Err(NativeSwapError::input(
                source.canonical(),
                NativeSwapBlockingFact::StalePlan,
                "candidate generation must advance beyond the resident generation",
            ));
        }
        let restart_boundary = if decision.is_compatible() {
            NativeSwapRestartBoundary::in_place()
        } else {
            decision_restart_boundary(&decision)
        };
        let changed_items = normalized_facts(decision.changed_functions.clone());
        Ok(Self {
            source,
            from,
            to,
            decision,
            capability,
            preserve_listeners: Vec::new(),
            preserve_connections: Vec::new(),
            restart_boundary,
            quiescence: NativeSwapQuiescence::ready(),
            relocation: NativeSwapRelocationFacts::default(),
            link: NativeSwapLinkFacts::default(),
            changed_items,
            removed_definitions: Vec::new(),
            blocking_fact: None,
        })
    }

    /// Construct a plan directly from source and revision pairs plus the
    /// sema-owned hot-swap verdict.
    pub fn for_revisions(
        source: NativeSwapSourceIdentity,
        from: NativeSwapRevision,
        to: NativeSwapRevision,
        capability: NativeSwapCapability,
        decision: HotSwapDecision,
    ) -> Result<Self, NativeSwapError> {
        let from_image = NativeSwapExecutable::new(source.clone(), from, 0)?;
        let to_image = NativeSwapExecutable::new(source.clone(), to, 1)?;
        Self::new(source, from_image, to_image, capability, decision)
    }

    /// Add a changed semantic identity in deterministic order.
    pub fn with_changed_items(mut self, items: Vec<String>) -> Self {
        self.changed_items = normalized_facts(items);
        self
    }

    /// Add one removed definition.  Validation turns it into a stale-reference
    /// refusal rather than allowing an old implementation to run.
    pub fn with_removed_definition(mut self, item: impl Into<String>) -> Self {
        self.removed_definitions.push(item.into());
        self.removed_definitions = normalized_facts(std::mem::take(&mut self.removed_definitions));
        self
    }

    /// Mark the candidate with an explicit checked blocking fact.
    pub fn with_blocking_fact(mut self, fact: NativeSwapBlockingFact) -> Self {
        self.blocking_fact = Some(fact);
        self
    }

    /// Mark an unsafe edit with the standard blocking fact.
    pub fn mark_unsafe(self) -> Self {
        self.with_blocking_fact(NativeSwapBlockingFact::UnsafeEdit)
    }

    /// Mark an incompatible type edit and its restart boundary.
    pub fn mark_incompatible_type(mut self, affected: Vec<String>) -> Self {
        self.blocking_fact = Some(NativeSwapBlockingFact::IncompatibleType);
        self.restart_boundary = NativeSwapRestartBoundary::required(
            NativeSwapBlockingFact::IncompatibleType,
            affected,
        );
        self
    }

    /// Mark a layout edit and its restart boundary.
    pub fn mark_layout_changed(mut self, affected: Vec<String>) -> Self {
        self.blocking_fact = Some(NativeSwapBlockingFact::LayoutChanged);
        self.restart_boundary = NativeSwapRestartBoundary::required(
            NativeSwapBlockingFact::LayoutChanged,
            affected,
        );
        self
    }

    /// Attach an explicit restart boundary and state explanation.
    pub fn with_restart_boundary(mut self, boundary: NativeSwapRestartBoundary) -> Self {
        self.restart_boundary = boundary;
        self
    }

    /// Attach quiescence facts.
    pub fn with_quiescence(mut self, quiescence: NativeSwapQuiescence) -> Self {
        self.quiescence = quiescence;
        self
    }

    /// Attach relocation facts.
    pub fn with_relocation(mut self, relocation: NativeSwapRelocationFacts) -> Self {
        self.relocation = relocation;
        self
    }

    /// Attach link facts.
    pub fn with_link(mut self, link: NativeSwapLinkFacts) -> Self {
        self.link = link;
        self
    }

    /// Add one explicitly preserved listener handle.
    pub fn preserve_listener(mut self, handle: NativeSwapListenerHandle) -> Self {
        self.preserve_listeners.push(handle);
        self.preserve_listeners.sort();
        self
    }

    /// Add one explicitly preserved connection handle.
    pub fn preserve_connection(mut self, handle: NativeSwapConnectionHandle) -> Self {
        self.preserve_connections.push(handle);
        self.preserve_connections.sort();
        self
    }

    /// Replace the explicit listener preservation set.
    pub fn with_preserved_listeners(mut self, mut handles: Vec<NativeSwapListenerHandle>) -> Self {
        handles.sort();
        self.preserve_listeners = handles;
        self
    }

    /// Replace the explicit connection preservation set.
    pub fn with_preserved_connections(mut self, mut handles: Vec<NativeSwapConnectionHandle>) -> Self {
        handles.sort();
        self.preserve_connections = handles;
        self
    }

    /// Return whether code changed between the two revision facts.
    pub fn code_changed(&self) -> bool {
        !self.from.revision.code_unchanged(&self.to.revision)
    }

    /// Return whether data/layout changed between the two revision facts.
    pub fn data_changed(&self) -> bool {
        !self.from.revision.data_unchanged(&self.to.revision)
    }

    /// Return whether this plan carries any explicit restart/incompatibility fact.
    pub fn requires_restart(&self) -> bool {
        self.decision.requires_restart()
            || self.restart_boundary.required
            || self
                .blocking_fact
                .is_some_and(NativeSwapBlockingFact::is_restartable)
    }

    /// Return a positive or blocking explanation without consulting a resident
    /// host state.  The temporary state supplies the plan's explicit handles
    /// as its resident baseline so capability and policy failures use the same
    /// validation path as a live reload.
    pub fn explanation(&self) -> NativeSwapExplanation {
        if !self.decision.is_compatible() {
            let disposition = if self.capability.restart {
                NativeSwapDisposition::Restart
            } else {
                NativeSwapDisposition::Rejected
            };
            return NativeSwapExplanation::new(
                self.source.canonical(),
                disposition,
                NativeSwapBlockingFact::IncompatibleType,
                self.decision
                    .compatibility
                    .reason()
                    .unwrap_or("semantic hot-swap verdict requires a clean restart"),
            );
        }
        match NativeSwapState::new(
            self.from.clone(),
            self.preserve_listeners.clone(),
            self.preserve_connections.clone(),
        ) {
            Ok(state) => state.explain_reload(self),
            Err(error) => error.explanation.clone(),
        }
    }

    fn validate_shape(&self) -> Result<(), NativeSwapError> {
        let source_id = self.source.canonical();
        for (label, identity) in [
            ("plan", &self.source),
            ("resident executable", &self.from.source),
            ("candidate executable", &self.to.source),
        ] {
            if let Err(detail) = validate_source_identity(identity, label) {
                return Err(NativeSwapError::input(
                    source_id.clone(),
                    NativeSwapBlockingFact::InvalidIdentity,
                    detail,
                ));
            }
        }
        if !self.from.source.matches(&self.source) || !self.to.source.matches(&self.source) {
            return Err(NativeSwapError::input(
                source_id.clone(),
                NativeSwapBlockingFact::SourceIdentityChanged,
                "plan source does not match both executable identities",
            ));
        }
        if self.to.generation <= self.from.generation {
            return Err(NativeSwapError::input(
                source_id.clone(),
                NativeSwapBlockingFact::StalePlan,
                "candidate generation must advance beyond the resident generation",
            ));
        }
        validate_hot_swap_decision(&source_id, &self.source, &self.decision)?;
        self.restart_boundary.validate(&source_id)?;
        validate_fact_list(&source_id, &self.changed_items)?;
        validate_fact_list(&source_id, &self.removed_definitions)?;
        validate_fact_list(&source_id, &self.relocation.code_symbols)?;
        validate_fact_list(&source_id, &self.relocation.data_slots)?;
        validate_listener_plan_facts(&source_id, &self.source, &self.preserve_listeners)?;
        validate_connection_plan_facts(&source_id, &self.source, &self.preserve_connections)?;
        for (label, identity) in [
            ("resident link identity", &self.link.resident_identity),
            ("candidate link identity", &self.link.candidate_identity),
        ] {
            if let Err(detail) = validate_identity(identity, label) {
                return Err(NativeSwapError::input(
                    source_id.clone(),
                    NativeSwapBlockingFact::InvalidIdentity,
                    detail,
                ));
            }
        }
        Ok(())
    }
}

/// Result of a committed, rolled-back, or rejected native swap attempt.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NativeSwapReceipt {
    /// Source/module identity for the operation.
    pub source: NativeSwapSourceIdentity,
    /// Operation outcome.
    pub outcome: NativeSwapOutcome,
    /// Revision before the attempted operation.
    pub before: NativeSwapRevision,
    /// Revision after the attempted operation (the prior revision on rollback/rejection).
    pub after: NativeSwapRevision,
    /// Resident generation before the attempted operation.
    pub before_generation: u64,
    /// Resident generation after the attempted operation.
    pub after_generation: u64,
    /// Listener handles retained by explicit compatible facts.
    pub preserved_listeners: Vec<String>,
    /// Listener handles reset/dropped because they were not explicitly retained.
    pub reset_listeners: Vec<String>,
    /// Connection handles retained by explicit compatible facts.
    pub preserved_connections: Vec<String>,
    /// Connection handles reset/dropped because they were not explicitly retained.
    pub reset_connections: Vec<String>,
    /// Restart boundary fact, if the operation was rejected for restart.
    pub restart_boundary: NativeSwapRestartBoundary,
    /// Exact explanation for a rejection or rollback.
    pub explanation: Option<NativeSwapExplanation>,
}

/// Outcome of one native swap transaction.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeSwapOutcome {
    /// Candidate is now the resident executable.
    Committed,
    /// Candidate was installed transiently then restored to the checkpoint.
    RolledBack,
    /// Candidate was rejected before activation.
    Rejected,
    /// Candidate was not activated because a clean restart is required.
    RestartRequired,
}

impl NativeSwapOutcome {
    /// Stable report spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::RolledBack => "rolled_back",
            Self::Rejected => "rejected",
            Self::RestartRequired => "restart_required",
        }
    }
}

impl NativeSwapReceipt {
    /// Return whether the candidate is the committed resident image.
    pub const fn committed(&self) -> bool {
        matches!(self.outcome, NativeSwapOutcome::Committed)
    }

    /// Return whether this receipt restored the prior checkpoint.
    pub const fn rolled_back(&self) -> bool {
        matches!(self.outcome, NativeSwapOutcome::RolledBack)
    }

    /// Return whether the operation did not activate the candidate.
    pub const fn rejected(&self) -> bool {
        matches!(
            self.outcome,
            NativeSwapOutcome::Rejected | NativeSwapOutcome::RestartRequired
        )
    }

    /// Return a deterministic one-line report with all preservation and
    /// explanation facts visible.
    pub fn render(&self) -> String {
        let explanation = self
            .explanation
            .as_ref()
            .map(NativeSwapExplanation::render)
            .unwrap_or_else(|| "none".to_string());
        format!(
            "native-swap outcome={} source={} before=code:{}+data:{}@{} after=code:{}+data:{}@{} preserved_listeners=[{}] reset_listeners=[{}] preserved_connections=[{}] reset_connections=[{}] restart={} explanation={}",
            self.outcome,
            self.source,
            self.before.code,
            self.before.data,
            self.before_generation,
            self.after.code,
            self.after.data,
            self.after_generation,
            self.preserved_listeners.join(","),
            self.reset_listeners.join(","),
            self.preserved_connections.join(","),
            self.reset_connections.join(","),
            self.restart_boundary.required,
            explanation,
        )
    }
}

impl fmt::Display for NativeSwapOutcome {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

/// Observable phase of a resident swap transaction.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeSwapPhase {
    /// No transaction is open.
    Ready,
    /// Old executable is paused and a checkpoint is retained.
    Paused,
    /// Candidate has been applied but not committed.
    Applied,
}

impl NativeSwapPhase {
    /// Stable report spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Paused => "paused",
            Self::Applied => "applied",
        }
    }
}

impl fmt::Display for NativeSwapPhase {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeSwapCheckpoint {
    executable: NativeSwapExecutable,
    listeners: BTreeMap<String, NativeSwapListenerHandle>,
    connections: BTreeMap<String, NativeSwapConnectionHandle>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeSwapWorkingReceipt {
    source: NativeSwapSourceIdentity,
    before: NativeSwapExecutable,
    after: NativeSwapExecutable,
    preserved_listeners: Vec<String>,
    reset_listeners: Vec<String>,
    preserved_connections: Vec<String>,
    reset_connections: Vec<String>,
    restart_boundary: NativeSwapRestartBoundary,
}

/// Resident executable and explicit handle state for one native dev session.
///
/// Handles are retained only through the plan's explicit compatible lists.  A
/// successful swap that omits an old listener/connection resets it in the
/// receipt; this prevents accidental preservation by merely copying a map.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeSwapState {
    executable: NativeSwapExecutable,
    listeners: BTreeMap<String, NativeSwapListenerHandle>,
    connections: BTreeMap<String, NativeSwapConnectionHandle>,
    phase: NativeSwapPhase,
    checkpoint: Option<NativeSwapCheckpoint>,
    pending_plan: Option<NativeSwapPlan>,
    working_receipt: Option<NativeSwapWorkingReceipt>,
}

impl NativeSwapState {
    /// Construct resident state from one executable and explicit live handles.
    pub fn new(
        executable: NativeSwapExecutable,
        listeners: Vec<NativeSwapListenerHandle>,
        connections: Vec<NativeSwapConnectionHandle>,
    ) -> Result<Self, NativeSwapError> {
        let source_id = executable.source.canonical();
        if let Err(detail) = validate_source_identity(&executable.source, "executable") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        let listener_map = listener_map(&source_id, &executable, listeners)?;
        let connection_map = connection_map(&source_id, &executable, connections, &listener_map)?;
        Ok(Self {
            executable,
            listeners: listener_map,
            connections: connection_map,
            phase: NativeSwapPhase::Ready,
            checkpoint: None,
            pending_plan: None,
            working_receipt: None,
        })
    }

    /// Construct state with no listener or connection handles.
    pub fn empty(executable: NativeSwapExecutable) -> Result<Self, NativeSwapError> {
        Self::new(executable, Vec::new(), Vec::new())
    }

    /// Borrow the current executable identity.
    pub fn executable(&self) -> &NativeSwapExecutable {
        &self.executable
    }

    /// Borrow the current source identity.
    pub fn source(&self) -> &NativeSwapSourceIdentity {
        &self.executable.source
    }

    /// Borrow the current revisions.
    pub fn revision(&self) -> &NativeSwapRevision {
        &self.executable.revision
    }

    /// Current resident generation.
    pub const fn generation(&self) -> u64 {
        self.executable.generation
    }

    /// Current transaction phase.
    pub const fn phase(&self) -> NativeSwapPhase {
        self.phase
    }

    /// Return whether a transaction is paused before apply.
    pub const fn is_paused(&self) -> bool {
        matches!(self.phase, NativeSwapPhase::Paused)
    }

    /// Return whether a candidate is applied but not committed.
    pub const fn is_applied(&self) -> bool {
        matches!(self.phase, NativeSwapPhase::Applied)
    }

    /// Return listener handles in deterministic identity order.
    pub fn listeners(&self) -> Vec<NativeSwapListenerHandle> {
        self.listeners.values().cloned().collect()
    }

    /// Return connection handles in deterministic identity order.
    pub fn connections(&self) -> Vec<NativeSwapConnectionHandle> {
        self.connections.values().cloned().collect()
    }

    /// Find one resident listener by identity.
    pub fn listener(&self, id: &str) -> Option<&NativeSwapListenerHandle> {
        self.listeners.get(id)
    }

    /// Find one resident connection by identity.
    pub fn connection(&self, id: &str) -> Option<&NativeSwapConnectionHandle> {
        self.connections.get(id)
    }

    /// Number of resident listeners.
    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }

    /// Number of resident connections.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Validate a plan without changing phase, checkpoint, executable, or handles.
    pub fn preflight(&self, plan: &NativeSwapPlan) -> Result<(), NativeSwapError> {
        self.validate_plan(plan)
    }

    /// Explain a plan in the exact vocabulary used by receipts and CLI reports.
    pub fn explain_reload(&self, plan: &NativeSwapPlan) -> NativeSwapExplanation {
        match self.validate_plan(plan) {
            Ok(()) => NativeSwapExplanation::compatible(plan.source.canonical()),
            Err(error) => error.explanation.clone(),
        }
    }

    /// Pause a resident image after complete preflight and retain a rollback
    /// checkpoint.  Any failure leaves the state byte-for-byte equivalent to
    /// its prior ready state.
    pub fn pause(&mut self, plan: &NativeSwapPlan) -> Result<(), NativeSwapError> {
        if self.phase != NativeSwapPhase::Ready {
            return Err(self.phase_error("pause requires a ready transaction"));
        }
        self.validate_plan(plan)?;
        let checkpoint = NativeSwapCheckpoint {
            executable: self.executable.clone(),
            listeners: self.listeners.clone(),
            connections: self.connections.clone(),
        };
        self.checkpoint = Some(checkpoint);
        self.pending_plan = Some(plan.clone());
        self.working_receipt = None;
        self.phase = NativeSwapPhase::Paused;
        Ok(())
    }

    /// Apply a previously paused plan.  All checks run before the first state
    /// assignment, so incompatible/unsafe edits cannot partially mutate state.
    pub fn apply(&mut self, plan: &NativeSwapPlan) -> Result<(), NativeSwapError> {
        if self.phase != NativeSwapPhase::Paused {
            return Err(self.phase_error("apply requires a paused transaction"));
        }
        if self.pending_plan.as_ref() != Some(plan) {
            return Err(NativeSwapError::input(
                self.source().canonical(),
                NativeSwapBlockingFact::StalePlan,
                "apply plan does not match the paused plan",
            ));
        }
        self.validate_plan(plan)?;
        let checkpoint = self
            .checkpoint
            .as_ref()
            .ok_or_else(|| self.error(NativeSwapBlockingFact::RollbackUnavailable, "apply checkpoint is missing"))?;
        let preserved_listener_ids: BTreeSet<String> = plan
            .preserve_listeners
            .iter()
            .map(|handle| handle.id.clone())
            .collect();
        let preserved_connection_ids: BTreeSet<String> = plan
            .preserve_connections
            .iter()
            .map(|handle| handle.id.clone())
            .collect();

        let mut next_listeners = BTreeMap::new();
        for handle in &plan.preserve_listeners {
            let mut preserved = handle.clone();
            preserved.generation = plan.to.generation;
            next_listeners.insert(preserved.id.clone(), preserved);
        }
        let mut next_connections = BTreeMap::new();
        for handle in &plan.preserve_connections {
            let mut preserved = handle.clone();
            preserved.generation = plan.to.generation;
            next_connections.insert(preserved.id.clone(), preserved);
        }
        let reset_listeners = checkpoint
            .listeners
            .keys()
            .filter(|id| !preserved_listener_ids.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        let reset_connections = checkpoint
            .connections
            .keys()
            .filter(|id| !preserved_connection_ids.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        let working = NativeSwapWorkingReceipt {
            source: plan.source.clone(),
            before: checkpoint.executable.clone(),
            after: plan.to.clone(),
            preserved_listeners: preserved_listener_ids.iter().cloned().collect(),
            reset_listeners,
            preserved_connections: preserved_connection_ids.iter().cloned().collect(),
            reset_connections,
            restart_boundary: plan.restart_boundary.clone(),
        };

        // Every fallible check and all receipt facts are complete above.  These
        // assignments are the first mutation of the resident image/handles.
        self.executable = plan.to.clone();
        self.listeners = next_listeners;
        self.connections = next_connections;
        self.working_receipt = Some(working);
        self.phase = NativeSwapPhase::Applied;
        Ok(())
    }

    /// Commit an applied candidate and drop the rollback checkpoint.
    pub fn commit(&mut self) -> Result<NativeSwapReceipt, NativeSwapError> {
        if self.phase != NativeSwapPhase::Applied {
            return Err(self.phase_error("commit requires an applied transaction"));
        }
        let working = self.working_receipt.take().ok_or_else(|| {
            self.error(
                NativeSwapBlockingFact::InvalidPhase,
                "applied transaction has no working receipt",
            )
        })?;
        let receipt = receipt_from_working(&working, NativeSwapOutcome::Committed, None);
        self.checkpoint = None;
        self.pending_plan = None;
        self.phase = NativeSwapPhase::Ready;
        Ok(receipt)
    }

    /// Restore the exact executable and explicit handles captured by pause.
    pub fn rollback(&mut self) -> Result<NativeSwapReceipt, NativeSwapError> {
        if !matches!(self.phase, NativeSwapPhase::Paused | NativeSwapPhase::Applied) {
            return Err(self.phase_error("rollback requires a paused or applied transaction"));
        }
        let checkpoint = self.checkpoint.take().ok_or_else(|| {
            self.error(
                NativeSwapBlockingFact::RollbackUnavailable,
                "rollback checkpoint is missing",
            )
        })?;
        let before = self.executable.clone();
        let source = checkpoint.executable.source.clone();
        let receipt = NativeSwapReceipt {
            source,
            outcome: NativeSwapOutcome::RolledBack,
            before: before.revision.clone(),
            after: checkpoint.executable.revision.clone(),
            before_generation: before.generation,
            after_generation: checkpoint.executable.generation,
            preserved_listeners: checkpoint.listeners.keys().cloned().collect(),
            reset_listeners: self
                .listeners
                .keys()
                .filter(|id| !checkpoint.listeners.contains_key(*id))
                .cloned()
                .collect(),
            preserved_connections: checkpoint.connections.keys().cloned().collect(),
            reset_connections: self
                .connections
                .keys()
                .filter(|id| !checkpoint.connections.contains_key(*id))
                .cloned()
                .collect(),
            restart_boundary: NativeSwapRestartBoundary::in_place(),
            explanation: Some(NativeSwapExplanation::new(
                checkpoint.executable.source.canonical(),
                NativeSwapDisposition::Rejected,
                NativeSwapBlockingFact::UnsafeEdit,
                "rollback restored the prior executable and explicit handles",
            )),
        };
        self.executable = checkpoint.executable;
        self.listeners = checkpoint.listeners;
        self.connections = checkpoint.connections;
        self.pending_plan = None;
        self.working_receipt = None;
        self.phase = NativeSwapPhase::Ready;
        Ok(receipt)
    }

    /// Reject a candidate without opening a transaction, retaining current
    /// executable and handles.  This is useful when compile/migration failed
    /// before pause could begin.
    pub fn reject(
        &self,
        plan: &NativeSwapPlan,
        explanation: NativeSwapExplanation,
    ) -> NativeSwapReceipt {
        NativeSwapReceipt {
            source: self.executable.source.clone(),
            outcome: if explanation.disposition == NativeSwapDisposition::Restart {
                NativeSwapOutcome::RestartRequired
            } else {
                NativeSwapOutcome::Rejected
            },
            before: self.executable.revision.clone(),
            after: plan.to.revision.clone(),
            before_generation: self.executable.generation,
            after_generation: self.executable.generation,
            preserved_listeners: self.listeners.keys().cloned().collect(),
            reset_listeners: Vec::new(),
            preserved_connections: self.connections.keys().cloned().collect(),
            reset_connections: Vec::new(),
            restart_boundary: plan.restart_boundary.clone(),
            explanation: Some(explanation),
        }
    }

    /// Roll back and attach an exact compile/migration failure explanation.
    pub fn fail(
        &mut self,
        explanation: NativeSwapExplanation,
    ) -> Result<NativeSwapReceipt, NativeSwapError> {
        let mut receipt = self.rollback()?;
        receipt.outcome = if explanation.disposition == NativeSwapDisposition::Restart {
            NativeSwapOutcome::RestartRequired
        } else {
            NativeSwapOutcome::Rejected
        };
        receipt.explanation = Some(explanation);
        Ok(receipt)
    }

    fn validate_plan(&self, plan: &NativeSwapPlan) -> Result<(), NativeSwapError> {
        plan.validate_shape()?;
        let source_id = self.source().canonical();
        if !plan.source.matches(self.source()) {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::SourceIdentityChanged,
                format!(
                    "plan source `{}` does not match resident source",
                    plan.source.canonical()
                ),
            ));
        }
        if plan.from != self.executable {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::StalePlan,
                "plan `from` executable does not match resident executable",
            ));
        }
        if plan.to.source != plan.source {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::SourceIdentityChanged,
                "candidate executable belongs to another source",
            ));
        }
        if plan.to.generation <= self.executable.generation {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::StalePlan,
                "candidate generation does not advance the resident generation",
            ));
        }
        if plan.decision.requires_restart() {
            return Err(restart_or_reject(
                &source_id,
                plan.capability.restart,
                NativeSwapBlockingFact::IncompatibleType,
                plan.decision
                    .compatibility
                    .reason()
                    .unwrap_or("semantic hot-swap verdict requires a clean restart"),
            ));
        }
        if let Some(fact) = plan.blocking_fact {
            let disposition = if plan.capability.restart && fact.is_restartable() {
                NativeSwapDisposition::Restart
            } else {
                NativeSwapDisposition::Rejected
            };
            return Err(NativeSwapError {
                explanation: NativeSwapExplanation::new(
                    source_id,
                    disposition,
                    fact,
                    "checked producer marked this plan unsafe or incompatible",
                ),
            });
        }
        if !plan.removed_definitions.is_empty() {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::StaleReference,
                format!(
                    "removed definitions would leave stale references: {}",
                    plan.removed_definitions.join(",")
                ),
            ));
        }
        if plan.restart_boundary.required {
            let fact = plan
                .restart_boundary
                .reason
                .unwrap_or(NativeSwapBlockingFact::RestartRequired);
            return Err(restart_or_reject(
                &source_id,
                plan.capability.restart,
                fact,
                "candidate requires the announced clean restart boundary",
            ));
        }
        if plan.capability.quiescence == NativeSwapQuiescencePolicy::Required
            && (!plan.quiescence.required || !plan.quiescence.is_ready())
        {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::QuiescenceRequired,
                format!(
                    "swap requires quiescence; active_operations={} reached={}",
                    plan.quiescence.active_operations, plan.quiescence.reached
                ),
            ));
        }
        if plan.code_changed() {
            match plan.capability.code {
                NativeSwapCodePolicy::Replace => {}
                NativeSwapCodePolicy::Restart => {
                    return Err(restart_or_reject(
                        &source_id,
                        plan.capability.restart,
                        NativeSwapBlockingFact::CodeRevisionUnsupported,
                        "code revision changed but capability permits replacement only at restart",
                    ));
                }
                NativeSwapCodePolicy::Reject => {
                    return Err(NativeSwapError::input(
                        source_id,
                        NativeSwapBlockingFact::CodeRevisionUnsupported,
                        "code revision changed and capability rejects in-place replacement",
                    ));
                }
            }
        }
        if plan.data_changed() {
            match plan.capability.data {
                NativeSwapDataPolicy::Preserve => {
                    return Err(restart_or_reject(
                        &source_id,
                        plan.capability.restart,
                        NativeSwapBlockingFact::DataRevisionUnsupported,
                        "data revision changed but resident data policy is preserve",
                    ));
                }
                NativeSwapDataPolicy::Replace | NativeSwapDataPolicy::Migrate => {}
                NativeSwapDataPolicy::Restart => {
                    return Err(restart_or_reject(
                        &source_id,
                        plan.capability.restart,
                        NativeSwapBlockingFact::LayoutChanged,
                        "data revision changed and capability requires a clean restart",
                    ));
                }
                NativeSwapDataPolicy::Reject => {
                    return Err(NativeSwapError::input(
                        source_id,
                        NativeSwapBlockingFact::DataRevisionUnsupported,
                        "data revision changed and capability rejects in-place replacement",
                    ));
                }
            }
        }
        let relocation_supported = plan.relocation.strategy.permits_in_place()
            && plan.capability.relocation.permits_in_place();
        if (plan.code_changed() || plan.data_changed()) && !relocation_supported {
            return Err(restart_or_reject(
                &source_id,
                plan.capability.restart,
                NativeSwapBlockingFact::RelocationUnsupported,
                format!(
                    "relocation strategy `{}` with capability `{}` does not permit in-place activation",
                    plan.relocation.strategy, plan.capability.relocation
                ),
            ));
        }
        let link_supported =
            plan.link.strategy.permits_in_place() && plan.capability.link.permits_in_place();
        if (plan.code_changed() || plan.data_changed()) && !link_supported {
            return Err(restart_or_reject(
                &source_id,
                plan.capability.restart,
                NativeSwapBlockingFact::LinkStrategyUnsupported,
                format!(
                    "link strategy `{}` with capability `{}` does not permit in-place activation",
                    plan.link.strategy, plan.capability.link
                ),
            ));
        }
        self.validate_listener_handles(plan)?;
        self.validate_connection_handles(plan)?;
        Ok(())
    }

    fn validate_listener_handles(&self, plan: &NativeSwapPlan) -> Result<(), NativeSwapError> {
        let source_id = self.source().canonical();
        if !plan.preserve_listeners.is_empty()
            && plan.capability.listeners == NativeSwapHandlePolicy::Never
        {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::CapabilityUnavailable,
                "listener preservation was requested but capability forbids it",
            ));
        }
        for handle in &plan.preserve_listeners {
            if !handle.source.matches(self.source()) {
                return Err(NativeSwapError::input(
                    source_id,
                    NativeSwapBlockingFact::ListenerHandleStale,
                    format!("listener handle `{}` belongs to another source", handle.id),
                ));
            }
            let Some(current) = self.listeners.get(&handle.id) else {
                return Err(NativeSwapError::input(
                    source_id,
                    NativeSwapBlockingFact::ListenerHandleMissing,
                    format!("listener handle `{}` is not resident", handle.id),
                ));
            };
            if !handle.is_compatible() {
                return Err(NativeSwapError::input(
                    source_id,
                    NativeSwapBlockingFact::ListenerHandleIncompatible,
                    format!("listener handle `{}` is explicitly incompatible", handle.id),
                ));
            }
            if current.generation != handle.generation || !current.is_compatible() {
                return Err(NativeSwapError::input(
                    source_id,
                    NativeSwapBlockingFact::ListenerHandleStale,
                    format!("listener handle `{}` does not match the resident generation", handle.id),
                ));
            }
        }
        Ok(())
    }

    fn validate_connection_handles(&self, plan: &NativeSwapPlan) -> Result<(), NativeSwapError> {
        let source_id = self.source().canonical();
        if !plan.preserve_connections.is_empty()
            && plan.capability.connections == NativeSwapHandlePolicy::Never
        {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::CapabilityUnavailable,
                "connection preservation was requested but capability forbids it",
            ));
        }
        let listener_ids: BTreeSet<&str> = plan
            .preserve_listeners
            .iter()
            .map(|handle| handle.id.as_str())
            .collect();
        for handle in &plan.preserve_connections {
            if !handle.source.matches(self.source()) {
                return Err(NativeSwapError::input(
                    source_id,
                    NativeSwapBlockingFact::ConnectionHandleStale,
                    format!("connection handle `{}` belongs to another source", handle.id),
                ));
            }
            let Some(current) = self.connections.get(&handle.id) else {
                return Err(NativeSwapError::input(
                    source_id,
                    NativeSwapBlockingFact::ConnectionHandleMissing,
                    format!("connection handle `{}` is not resident", handle.id),
                ));
            };
            if !handle.is_compatible() {
                return Err(NativeSwapError::input(
                    source_id,
                    NativeSwapBlockingFact::ConnectionHandleIncompatible,
                    format!("connection handle `{}` is explicitly incompatible", handle.id),
                ));
            }
            if current.generation != handle.generation || !current.is_compatible() {
                return Err(NativeSwapError::input(
                    source_id,
                    NativeSwapBlockingFact::ConnectionHandleStale,
                    format!("connection handle `{}` does not match the resident generation", handle.id),
                ));
            }
            if !listener_ids.contains(handle.listener_id.as_str()) {
                return Err(NativeSwapError::input(
                    source_id,
                    NativeSwapBlockingFact::ConnectionListenerNotPreserved,
                    format!(
                        "connection `{}` names listener `{}` without an explicit compatible listener handle",
                        handle.id, handle.listener_id
                    ),
                ));
            }
        }
        Ok(())
    }

    fn error(&self, fact: NativeSwapBlockingFact, detail: impl Into<String>) -> NativeSwapError {
        NativeSwapError::input(self.source().canonical(), fact, detail)
    }

    fn phase_error(&self, detail: impl Into<String>) -> NativeSwapError {
        let detail = detail.into();
        self.error(
            NativeSwapBlockingFact::InvalidPhase,
            format!("{detail}; current phase={}", self.phase),
        )
    }
}

/// Compatibility aliases for callers that use unqualified domain terms.
pub type CodeRevision = NativeSwapCodeRevision;
/// Compatibility alias for the data revision fact.
pub type DataRevision = NativeSwapDataRevision;
/// Compatibility alias for the resident native state.
pub type NativeSwapResidentState = NativeSwapState;
/// Compatibility alias for the swap failure.
pub type NativeSwapFailure = NativeSwapError;

fn receipt_from_working(
    working: &NativeSwapWorkingReceipt,
    outcome: NativeSwapOutcome,
    explanation: Option<NativeSwapExplanation>,
) -> NativeSwapReceipt {
    NativeSwapReceipt {
        source: working.source.clone(),
        outcome,
        before: working.before.revision.clone(),
        after: working.after.revision.clone(),
        before_generation: working.before.generation,
        after_generation: working.after.generation,
        preserved_listeners: working.preserved_listeners.clone(),
        reset_listeners: working.reset_listeners.clone(),
        preserved_connections: working.preserved_connections.clone(),
        reset_connections: working.reset_connections.clone(),
        restart_boundary: working.restart_boundary.clone(),
        explanation,
    }
}

fn listener_map(
    source_id: &str,
    executable: &NativeSwapExecutable,
    listeners: Vec<NativeSwapListenerHandle>,
) -> Result<BTreeMap<String, NativeSwapListenerHandle>, NativeSwapError> {
    if listeners.len() > MAX_NATIVE_SWAP_FACTS {
        return Err(NativeSwapError::input(
            source_id,
            NativeSwapBlockingFact::FactLimit,
            "resident listener count exceeds its bounded fact limit",
        ));
    }
    let mut map = BTreeMap::new();
    for listener in listeners {
        if let Err(detail) = validate_identity(&listener.id, "listener handle identity") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if let Err(detail) = validate_source_identity(&listener.source, "listener handle") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if !listener.source.matches(&executable.source) {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::ListenerHandleStale,
                format!("listener `{}` belongs to another source", listener.id),
            ));
        }
        if listener.generation != executable.generation {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::ListenerHandleStale,
                format!("listener `{}` does not match resident generation", listener.id),
            ));
        }
        if map.insert(listener.id.clone(), listener).is_some() {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::DuplicateIdentity,
                "resident listener identities must be unique",
            ));
        }
    }
    Ok(map)
}

fn connection_map(
    source_id: &str,
    executable: &NativeSwapExecutable,
    connections: Vec<NativeSwapConnectionHandle>,
    listeners: &BTreeMap<String, NativeSwapListenerHandle>,
) -> Result<BTreeMap<String, NativeSwapConnectionHandle>, NativeSwapError> {
    if connections.len() > MAX_NATIVE_SWAP_FACTS {
        return Err(NativeSwapError::input(
            source_id,
            NativeSwapBlockingFact::FactLimit,
            "resident connection count exceeds its bounded fact limit",
        ));
    }
    let mut map = BTreeMap::new();
    for connection in connections {
        if let Err(detail) = validate_identity(&connection.id, "connection handle identity") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if let Err(detail) = validate_identity(&connection.listener_id, "connection listener identity") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if let Err(detail) = validate_source_identity(&connection.source, "connection handle") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if !connection.source.matches(&executable.source) {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::ConnectionHandleStale,
                format!("connection `{}` belongs to another source", connection.id),
            ));
        }
        if connection.generation != executable.generation {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::ConnectionHandleStale,
                format!("connection `{}` does not match resident generation", connection.id),
            ));
        }
        if !listeners.contains_key(&connection.listener_id) {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::ConnectionListenerNotPreserved,
                format!(
                    "connection `{}` names unknown listener `{}`",
                    connection.id, connection.listener_id
                ),
            ));
        }
        if map.insert(connection.id.clone(), connection).is_some() {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::DuplicateIdentity,
                "resident connection identities must be unique",
            ));
        }
    }
    Ok(map)
}

fn validate_identity(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if value.len() > MAX_NATIVE_SWAP_ID_BYTES {
        return Err(format!(
            "{label} exceeds {} bytes",
            MAX_NATIVE_SWAP_ID_BYTES
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} contains a control character"));
    }
    Ok(())
}

fn validate_source_identity(
    identity: &NativeSwapSourceIdentity,
    label: &str,
) -> Result<(), String> {
    validate_identity(&identity.source_id, &format!("{label} source"))?;
    validate_identity(&identity.module, &format!("{label} module"))
}
fn validate_hot_swap_decision(
    source_id: &str,
    source: &NativeSwapSourceIdentity,
    decision: &HotSwapDecision,
) -> Result<(), NativeSwapError> {
    if let Err(detail) = validate_identity(&decision.module, "hot-swap decision module") {
        return Err(NativeSwapError::input(
            source_id,
            NativeSwapBlockingFact::InvalidIdentity,
            detail,
        ));
    }
    if decision.module != source.module {
        return Err(NativeSwapError::input(
            source_id,
            NativeSwapBlockingFact::SourceIdentityChanged,
            format!(
                "hot-swap decision module `{}` does not match source module `{}`",
                decision.module, source.module
            ),
        ));
    }
    if decision.state_facts().len() > MAX_NATIVE_SWAP_FACTS {
        return Err(NativeSwapError::input(
            source_id,
            NativeSwapBlockingFact::FactLimit,
            "hot-swap decision exceeds its retention fact limit",
        ));
    }
    let mut seen = BTreeSet::new();
    for fact in decision.state_facts() {
        if let Err(detail) = validate_identity(&fact.key, "hot-swap retention key") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if !seen.insert(fact.key.as_str()) {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::DuplicateIdentity,
                format!("hot-swap retention key `{}` appears more than once", fact.key),
            ));
        }
    }
    Ok(())
}

fn decision_restart_boundary(decision: &HotSwapDecision) -> NativeSwapRestartBoundary {
    let mut affected = normalized_facts(decision.changed_functions.clone());
    if affected.is_empty() {
        affected.push(decision.module.clone());
    }
    let mut kept = Vec::new();
    let mut reset = Vec::new();
    for fact in decision.state_facts() {
        match &fact.decision {
            StateRetentionDecision::Preserve => kept.push(fact.key.clone()),
            StateRetentionDecision::Fresh { .. } => reset.push(fact.key.clone()),
        }
    }
    NativeSwapRestartBoundary::required(NativeSwapBlockingFact::IncompatibleType, affected)
        .with_state(kept, reset)
}

fn validate_listener_plan_facts(
    source_id: &str,
    expected_source: &NativeSwapSourceIdentity,
    handles: &[NativeSwapListenerHandle],
) -> Result<(), NativeSwapError> {
    if handles.len() > MAX_NATIVE_SWAP_FACTS {
        return Err(NativeSwapError::input(
            source_id,
            NativeSwapBlockingFact::FactLimit,
            "native swap plan exceeds its listener fact limit",
        ));
    }
    let mut seen = BTreeSet::new();
    for handle in handles {
        if let Err(detail) = validate_identity(&handle.id, "listener handle identity") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if let Err(detail) = validate_source_identity(&handle.source, "listener handle") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if !handle.source.matches(expected_source) {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::ListenerHandleStale,
                format!("listener handle `{}` belongs to another source", handle.id),
            ));
        }
        if !seen.insert(&handle.id) {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::DuplicateIdentity,
                format!("listener handle `{}` appears more than once", handle.id),
            ));
        }
    }
    Ok(())
}

fn validate_connection_plan_facts(
    source_id: &str,
    expected_source: &NativeSwapSourceIdentity,
    handles: &[NativeSwapConnectionHandle],
) -> Result<(), NativeSwapError> {
    if handles.len() > MAX_NATIVE_SWAP_FACTS {
        return Err(NativeSwapError::input(
            source_id,
            NativeSwapBlockingFact::FactLimit,
            "native swap plan exceeds its connection fact limit",
        ));
    }
    let mut seen = BTreeSet::new();
    for handle in handles {
        if let Err(detail) = validate_identity(&handle.id, "connection handle identity") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if let Err(detail) = validate_identity(&handle.listener_id, "connection listener identity") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if let Err(detail) = validate_source_identity(&handle.source, "connection handle") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
        if !handle.source.matches(expected_source) {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::ConnectionHandleStale,
                format!("connection handle `{}` belongs to another source", handle.id),
            ));
        }
        if !seen.insert(&handle.id) {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::DuplicateIdentity,
                format!("connection handle `{}` appears more than once", handle.id),
            ));
        }
    }
    Ok(())
}

fn validate_revision(value: &str, label: &str) -> Result<(), String> {
    validate_identity(value, label)
}

fn validate_fact_list(source_id: &str, facts: &[String]) -> Result<(), NativeSwapError> {
    if facts.len() > MAX_NATIVE_SWAP_FACTS {
        return Err(NativeSwapError::input(
            source_id,
            NativeSwapBlockingFact::FactLimit,
            "native swap fact list exceeds its bounded fact limit",
        ));
    }
    for fact in facts {
        if let Err(detail) = validate_identity(fact, "native swap fact") {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::InvalidIdentity,
                detail,
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for fact in facts {
        if !seen.insert(fact) {
            return Err(NativeSwapError::input(
                source_id,
                NativeSwapBlockingFact::DuplicateIdentity,
                format!("native swap fact `{fact}` appears more than once"),
            ));
        }
    }
    Ok(())
}

fn normalized_facts(mut facts: Vec<String>) -> Vec<String> {
    facts.sort();
    facts
}

fn bounded_text(value: &str, limit: usize) -> String {
    let mut output = String::new();
    for character in value.chars() {
        if character.is_control() {
            continue;
        }
        if output.len() + character.len_utf8() > limit {
            break;
        }
        output.push(character);
    }
    output
}

fn restart_or_reject(
    source_id: &str,
    allows_restart: bool,
    fact: NativeSwapBlockingFact,
    detail: impl Into<String>,
) -> NativeSwapError {
    let detail = detail.into();
    if allows_restart && fact.is_restartable() {
        NativeSwapError::restart(source_id.to_string(), fact, detail)
    } else {
        let reason = if fact.is_restartable() {
            "capability does not allow a clean restart"
        } else {
            "blocking fact cannot be handled by a clean restart"
        };
        NativeSwapError::input(source_id.to_string(), fact, format!("{detail}; {reason}"))
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use jet_foundation::HotSwap::HotSwapCompatibility;

    fn fixture() -> (
        NativeSwapSourceIdentity,
        NativeSwapExecutable,
        NativeSwapListenerHandle,
        NativeSwapConnectionHandle,
    ) {
        let source = NativeSwapSourceIdentity::new("server.jet").unwrap();
        let revision = NativeSwapRevision::new("code-1", "data-1").unwrap();
        let executable = NativeSwapExecutable::new(source.clone(), revision, 1).unwrap();
        let listener = NativeSwapListenerHandle::compatible("http", source.clone(), 1).unwrap();
        let connection = NativeSwapConnectionHandle::compatible("conn-7", "http", source.clone(), 1).unwrap();
        (source, executable, listener, connection)
    }

    fn compatible_decision() -> HotSwapDecision {
        HotSwapDecision {
            module: "server.jet".to_string(),
            compatibility: HotSwapCompatibility::Compatible,
            changed: Vec::new(),
            changed_functions: vec!["server.jet::fn:main".to_string()],
            state: Vec::new(),
            schema_migrations: Vec::new(),
            rechecked_items: Vec::new(),
            change_facts: Vec::new(),
        }
    }

    fn compatible_plan(
        source: NativeSwapSourceIdentity,
        executable: NativeSwapExecutable,
        listener: NativeSwapListenerHandle,
        connection: NativeSwapConnectionHandle,
    ) -> NativeSwapPlan {
        let next = NativeSwapExecutable::new(
            source.clone(),
            NativeSwapRevision::new("code-2", "data-1").unwrap(),
            2,
        )
        .unwrap();
        NativeSwapPlan::new(
            source,
            executable,
            next,
            NativeSwapCapability::resident(),
            compatible_decision(),
        )
            .unwrap()
            .preserve_listener(listener)
            .preserve_connection(connection)
    }

    #[test]
    fn compatible_swap_keeps_only_explicit_compatible_handles() {
        let (source, executable, listener, connection) = fixture();
        let other_listener = NativeSwapListenerHandle::compatible("metrics", source.clone(), 1).unwrap();
        let mut state = NativeSwapState::new(
            executable.clone(),
            vec![listener.clone(), other_listener],
            vec![connection.clone()],
        )
        .unwrap();
        let plan = compatible_plan(source, executable, listener, connection);
        state.pause(&plan).unwrap();
        state.apply(&plan).unwrap();
        let receipt = state.commit().unwrap();
        assert!(receipt.committed());
        assert_eq!(state.generation(), 2);
        assert_eq!(state.listener_count(), 1);
        assert_eq!(state.connection_count(), 1);
        assert_eq!(receipt.preserved_listeners, vec!["http"]);
        assert_eq!(receipt.reset_listeners, vec!["metrics"]);
        assert_eq!(receipt.preserved_connections, vec!["conn-7"]);
    }

    #[test]
    fn incompatible_plan_is_rejected_before_pause_mutates_state() {
        let (source, executable, listener, connection) = fixture();
        let mut state = NativeSwapState::new(executable.clone(), vec![listener.clone()], vec![connection.clone()]).unwrap();
        let plan = compatible_plan(source, executable.clone(), listener, connection).mark_layout_changed(vec!["Order".to_string()]);
        let before = state.clone();
        let error = state.pause(&plan).unwrap_err();
        assert_eq!(error.blocking_fact(), NativeSwapBlockingFact::LayoutChanged);
        assert_eq!(state, before);
        assert!(error.render().contains("source=server.jet"));
        assert!(error.render().contains("blocking_fact=layout_changed"));
    }

    #[test]
    fn rollback_restores_prior_executable_and_handles() {
        let (source, executable, listener, connection) = fixture();
        let mut state = NativeSwapState::new(executable.clone(), vec![listener.clone()], vec![connection.clone()]).unwrap();
        let plan = compatible_plan(source, executable.clone(), listener, connection);
        state.pause(&plan).unwrap();
        state.apply(&plan).unwrap();
        assert_eq!(state.generation(), 2);
        let receipt = state.rollback().unwrap();
        assert!(receipt.rolled_back());
        assert_eq!(state.executable(), &executable);
        assert_eq!(state.listener_count(), 1);
        assert_eq!(state.connection_count(), 1);
        assert_eq!(state.phase(), NativeSwapPhase::Ready);
    }

    #[test]
    fn quiescence_and_strategy_failures_name_exact_facts() {
        let (source, executable, listener, connection) = fixture();
        let mut state = NativeSwapState::new(executable.clone(), vec![listener.clone()], vec![connection.clone()]).unwrap();
        let plan = compatible_plan(source.clone(), executable, listener, connection)
            .with_quiescence(NativeSwapQuiescence::pending(2));
        let error = state.pause(&plan).unwrap_err();
        assert_eq!(error.blocking_fact(), NativeSwapBlockingFact::QuiescenceRequired);
        assert_eq!(error.source_id(), "server.jet");

        let executable = state.executable().clone();
        let listener = state.listener("http").unwrap().clone();
        let connection = state.connection("conn-7").unwrap().clone();
        let plan = compatible_plan(source, executable, listener, connection)
            .with_relocation(NativeSwapRelocationFacts::new(NativeSwapRelocationStrategy::Reject));
        let error = state.pause(&plan).unwrap_err();
        assert_eq!(error.blocking_fact(), NativeSwapBlockingFact::RelocationUnsupported);
    }

    #[test]
    fn receipt_order_and_explanation_are_deterministic() {
        let (source, executable, listener, connection) = fixture();
        let mut state = NativeSwapState::new(executable.clone(), vec![listener.clone()], vec![connection.clone()]).unwrap();
        let plan = compatible_plan(source, executable, listener, connection);
        assert_eq!(state.explain_reload(&plan).blocking_fact, NativeSwapBlockingFact::None);
        state.pause(&plan).unwrap();
        state.apply(&plan).unwrap();
        let receipt = state.commit().unwrap();
        let rendered = receipt.render();
        assert!(rendered.contains("outcome=committed"));
        assert!(rendered.contains("preserved_listeners=[http]"));
        assert!(rendered.contains("preserved_connections=[conn-7]"));
    }
}
