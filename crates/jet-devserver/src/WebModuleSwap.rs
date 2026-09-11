//! Backend-neutral web module swap facts and transaction state.
//!
//! This module deliberately stops at typed state. It does not know about a
//! browser, JavaScript, DOM rendering, or a network. A later `Session` or
//! `WebHost` adapter can turn a committed receipt into those effects without
//! re-deciding compatibility or retention.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use jet_foundation::HotSwap::{HotSwapDecision, StateRetentionDecision};

/// Stable identity for the page/module that owns one web swap stream.
///
/// Source and build revisions do not belong to this identity: changing either
/// revision is the normal input to a swap. They live on
/// [`WebModuleSwapSnapshot`] and are checked as transaction revisions.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WebModuleSwapIdentity {
    pub module: String,
    pub page: String,
}

impl WebModuleSwapIdentity {
    pub fn new(module: impl Into<String>, page: impl Into<String>) -> Self {
        Self {
            module: module.into(),
            page: page.into(),
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.module.is_empty() && !self.page.is_empty()
    }
}

/// The non-rendering page artifact retained by a swap state.
///
/// `artifact` is an opaque compiler/publication identity, not HTML. Keeping it
/// opaque makes rollback auditable without making this module a renderer.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WebPageFact {
    pub identity: String,
    pub artifact: String,
}

impl WebPageFact {
    pub fn new(identity: impl Into<String>, artifact: impl Into<String>) -> Self {
        Self {
            identity: identity.into(),
            artifact: artifact.into(),
        }
    }
}

/// One module in a web artifact.
///
/// A body fingerprint may change during a compatible module swap. An interface
/// fingerprint change is not safe for an in-place replacement.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WebModuleFact {
    pub identity: String,
    pub interface_fingerprint: String,
    pub body_fingerprint: String,
}

impl WebModuleFact {
    pub fn new(
        identity: impl Into<String>,
        interface_fingerprint: impl Into<String>,
        body_fingerprint: impl Into<String>,
    ) -> Self {
        Self {
            identity: identity.into(),
            interface_fingerprint: interface_fingerprint.into(),
            body_fingerprint: body_fingerprint.into(),
        }
    }
}

/// Backend-neutral values that may cross a web swap boundary.
///
/// There is intentionally no JSON/object/map escape hatch. A producer supplies
/// a type fingerprint on each state fact, while this small carrier keeps the
/// value representation explicit and cloneable for a transactional apply.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum WebStateValue {
    Text(String),
    Integer(i64),
    Unsigned(u64),
    Boolean(bool),
    Bytes(Vec<u8>),
}

impl WebStateValue {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Integer(_) => "integer",
            Self::Unsigned(_) => "unsigned",
            Self::Boolean(_) => "boolean",
            Self::Bytes(_) => "bytes",
        }
    }
}

impl From<&str> for WebStateValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<String> for WebStateValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<bool> for WebStateValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<i64> for WebStateValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<i32> for WebStateValue {
    fn from(value: i32) -> Self {
        Self::Integer(i64::from(value))
    }
}

impl From<u64> for WebStateValue {
    fn from(value: u64) -> Self {
        Self::Unsigned(value)
    }
}

impl From<u32> for WebStateValue {
    fn from(value: u32) -> Self {
        Self::Unsigned(u64::from(value))
    }
}

impl From<Vec<u8>> for WebStateValue {
    fn from(value: Vec<u8>) -> Self {
        Self::Bytes(value)
    }
}

/// The seven state families whose values can be audited across a web swap.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WebStateKind {
    Signal,
    Store,
    QueryCache,
    FormDraft,
    Scroll,
    Focus,
    Island,
}

impl WebStateKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Signal => "signal",
            Self::Store => "store",
            Self::QueryCache => "query_cache",
            Self::FormDraft => "form_draft",
            Self::Scroll => "scroll",
            Self::Focus => "focus",
            Self::Island => "island",
        }
    }
}

macro_rules! keyed_web_fact {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub struct $name {
            pub key: String,
            pub type_fingerprint: String,
            pub value: WebStateValue,
        }

        impl $name {
            pub fn new(
                key: impl Into<String>,
                type_fingerprint: impl Into<String>,
                value: impl Into<WebStateValue>,
            ) -> Self {
                Self {
                    key: key.into(),
                    type_fingerprint: type_fingerprint.into(),
                    value: value.into(),
                }
            }
        }
    };
}

// Compiler-derived signal retention input.
keyed_web_fact!(WebSignalFact);
// Compiler-derived store retention input.
keyed_web_fact!(WebStoreFact);
// Compiler-derived query-cache retention input.
keyed_web_fact!(WebQueryCacheFact);
// Compiler-derived form-draft retention input.
keyed_web_fact!(WebFormDraftFact);
// Compiler-derived scroll retention input, keyed by stable scroll target.
keyed_web_fact!(WebScrollFact);
// Compiler-derived focus retention input, keyed by stable focus target.
keyed_web_fact!(WebFocusFact);

/// Compiler-derived island retention input.
///
/// `key` identifies the route/slot. `identity` identifies the resumable island
/// instance. A slot can remain present while its island identity changes; that
/// change is therefore visible as a reset reason instead of being hidden as a
/// removed-and-added key pair.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WebIslandFact {
    pub key: String,
    pub identity: String,
    pub type_fingerprint: String,
    pub value: WebStateValue,
}

impl WebIslandFact {
    pub fn new(
        key: impl Into<String>,
        identity: impl Into<String>,
        type_fingerprint: impl Into<String>,
        value: impl Into<WebStateValue>,
    ) -> Self {
        Self {
            key: key.into(),
            identity: identity.into(),
            type_fingerprint: type_fingerprint.into(),
            value: value.into(),
        }
    }
}

/// One typed state fact in a snapshot or receipt.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum WebModuleSwapStateFact {
    Signal(WebSignalFact),
    Store(WebStoreFact),
    QueryCache(WebQueryCacheFact),
    FormDraft(WebFormDraftFact),
    Scroll(WebScrollFact),
    Focus(WebFocusFact),
    Island(WebIslandFact),
}

impl WebModuleSwapStateFact {
    pub fn kind(&self) -> WebStateKind {
        match self {
            Self::Signal(_) => WebStateKind::Signal,
            Self::Store(_) => WebStateKind::Store,
            Self::QueryCache(_) => WebStateKind::QueryCache,
            Self::FormDraft(_) => WebStateKind::FormDraft,
            Self::Scroll(_) => WebStateKind::Scroll,
            Self::Focus(_) => WebStateKind::Focus,
            Self::Island(_) => WebStateKind::Island,
        }
    }

    pub fn key(&self) -> &str {
        match self {
            Self::Signal(fact) => &fact.key,
            Self::Store(fact) => &fact.key,
            Self::QueryCache(fact) => &fact.key,
            Self::FormDraft(fact) => &fact.key,
            Self::Scroll(fact) => &fact.key,
            Self::Focus(fact) => &fact.key,
            Self::Island(fact) => &fact.key,
        }
    }

    pub fn identity(&self) -> &str {
        match self {
            Self::Island(fact) => &fact.identity,
            _ => self.key(),
        }
    }

    pub fn type_fingerprint(&self) -> &str {
        match self {
            Self::Signal(fact) => &fact.type_fingerprint,
            Self::Store(fact) => &fact.type_fingerprint,
            Self::QueryCache(fact) => &fact.type_fingerprint,
            Self::FormDraft(fact) => &fact.type_fingerprint,
            Self::Scroll(fact) => &fact.type_fingerprint,
            Self::Focus(fact) => &fact.type_fingerprint,
            Self::Island(fact) => &fact.type_fingerprint,
        }
    }

    pub fn value(&self) -> &WebStateValue {
        match self {
            Self::Signal(fact) => &fact.value,
            Self::Store(fact) => &fact.value,
            Self::QueryCache(fact) => &fact.value,
            Self::FormDraft(fact) => &fact.value,
            Self::Scroll(fact) => &fact.value,
            Self::Focus(fact) => &fact.value,
            Self::Island(fact) => &fact.value,
        }
    }

    fn with_value(&self, value: WebStateValue) -> Self {
        match self {
            Self::Signal(fact) => Self::Signal(WebSignalFact {
                key: fact.key.clone(),
                type_fingerprint: fact.type_fingerprint.clone(),
                value,
            }),
            Self::Store(fact) => Self::Store(WebStoreFact {
                key: fact.key.clone(),
                type_fingerprint: fact.type_fingerprint.clone(),
                value,
            }),
            Self::QueryCache(fact) => Self::QueryCache(WebQueryCacheFact {
                key: fact.key.clone(),
                type_fingerprint: fact.type_fingerprint.clone(),
                value,
            }),
            Self::FormDraft(fact) => Self::FormDraft(WebFormDraftFact {
                key: fact.key.clone(),
                type_fingerprint: fact.type_fingerprint.clone(),
                value,
            }),
            Self::Scroll(fact) => Self::Scroll(WebScrollFact {
                key: fact.key.clone(),
                type_fingerprint: fact.type_fingerprint.clone(),
                value,
            }),
            Self::Focus(fact) => Self::Focus(WebFocusFact {
                key: fact.key.clone(),
                type_fingerprint: fact.type_fingerprint.clone(),
                value,
            }),
            Self::Island(fact) => Self::Island(WebIslandFact {
                key: fact.key.clone(),
                identity: fact.identity.clone(),
                type_fingerprint: fact.type_fingerprint.clone(),
                value,
            }),
        }
    }
}

/// Source/build/page/module/state facts for one committed or candidate app.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebModuleSwapSnapshot {
    pub identity: WebModuleSwapIdentity,
    pub source_revision: String,
    pub build_revision: String,
    pub page: WebPageFact,
    pub modules: Vec<WebModuleFact>,
    pub state: Vec<WebModuleSwapStateFact>,
}

impl WebModuleSwapSnapshot {
    pub fn new(
        identity: WebModuleSwapIdentity,
        source_revision: impl Into<String>,
        build_revision: impl Into<String>,
        page: WebPageFact,
        modules: Vec<WebModuleFact>,
        state: Vec<WebModuleSwapStateFact>,
    ) -> Result<Self, WebModuleSwapError> {
        let mut snapshot = Self {
            identity,
            source_revision: source_revision.into(),
            build_revision: build_revision.into(),
            page,
            modules,
            state,
        };
        snapshot.normalize()?;
        Ok(snapshot)
    }

    pub fn normalize(&mut self) -> Result<(), WebModuleSwapError> {
        if !self.identity.is_valid() {
            return Err(WebModuleSwapError::InvalidSnapshot {
                detail: "module and page identity must be non-empty".to_string(),
            });
        }
        if self.source_revision.is_empty() || self.build_revision.is_empty() {
            return Err(WebModuleSwapError::InvalidSnapshot {
                detail: "source and build revisions must be non-empty".to_string(),
            });
        }
        if self.page.identity.is_empty() {
            return Err(WebModuleSwapError::InvalidSnapshot {
                detail: "page identity must be non-empty".to_string(),
            });
        }
        if self.page.identity != self.identity.page {
            return Err(WebModuleSwapError::InvalidSnapshot {
                detail: "page fact identity does not match swap identity".to_string(),
            });
        }

        self.modules.sort_by(|left, right| left.identity.cmp(&right.identity));
        for module in &self.modules {
            if module.identity.is_empty()
                || module.interface_fingerprint.is_empty()
                || module.body_fingerprint.is_empty()
            {
                return Err(WebModuleSwapError::InvalidSnapshot {
                    detail: "module identity and fingerprints must be non-empty".to_string(),
                });
            }
        }
        if self
            .modules
            .windows(2)
            .any(|pair| pair[0].identity == pair[1].identity)
        {
            return Err(WebModuleSwapError::InvalidSnapshot {
                detail: "module identities must be unique".to_string(),
            });
        }

        self.state.sort_by(|left, right| {
            (left.kind(), left.key()).cmp(&(right.kind(), right.key()))
        });
        for fact in &self.state {
            if fact.key().is_empty() || fact.type_fingerprint().is_empty() {
                return Err(WebModuleSwapError::InvalidSnapshot {
                    detail: "state keys and type fingerprints must be non-empty".to_string(),
                });
            }
            if matches!(fact, WebModuleSwapStateFact::Island(_)) && fact.identity().is_empty() {
                return Err(WebModuleSwapError::InvalidSnapshot {
                    detail: "island identities must be non-empty".to_string(),
                });
            }
        }
        if self.state.windows(2).any(|pair| {
            pair[0].kind() == pair[1].kind() && pair[0].key() == pair[1].key()
        }) {
            return Err(WebModuleSwapError::InvalidSnapshot {
                detail: "state keys must be unique within a state family".to_string(),
            });
        }
        Ok(())
    }

    pub fn module(&self, identity: &str) -> Option<&WebModuleFact> {
        self.modules
            .binary_search_by(|module| module.identity.as_str().cmp(identity))
            .ok()
            .map(|index| &self.modules[index])
    }

    pub fn state_fact(&self, kind: WebStateKind, key: &str) -> Option<&WebModuleSwapStateFact> {
        self.state
            .binary_search_by(|fact| (fact.kind(), fact.key()).cmp(&(kind, key)))
            .ok()
            .map(|index| &self.state[index])
    }
}

/// Why a module swap or state retention decision has a particular outcome.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WebModuleSwapReasonCode {
    NoChange,
    SourceRevisionChanged,
    BuildRevisionChanged,
    ModuleBodyChanged,
    ModuleAdded,
    ModuleRemoved,
    ModuleIdentityChanged,
    ModuleInterfaceChanged,
    PageIdentityChanged,
    StateAdded,
    StateRemoved,
    StateRetentionFresh,
    SignalTypeChanged,
    StoreTypeChanged,
    QueryCacheTypeChanged,
    FormDraftTypeChanged,
    ScrollTypeChanged,
    FocusTypeChanged,
    IslandTypeChanged,
    IslandIdentityChanged,
    IdentityMismatch,
    SemanticIncompatible,
    StalePlan,
    NothingToRollback,
}

impl WebModuleSwapReasonCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoChange => "no_change",
            Self::SourceRevisionChanged => "source_revision_changed",
            Self::BuildRevisionChanged => "build_revision_changed",
            Self::ModuleBodyChanged => "module_body_changed",
            Self::ModuleAdded => "module_added",
            Self::ModuleRemoved => "module_removed",
            Self::ModuleIdentityChanged => "module_identity_changed",
            Self::ModuleInterfaceChanged => "module_interface_changed",
            Self::PageIdentityChanged => "page_identity_changed",
            Self::StateAdded => "state_added",
            Self::StateRemoved => "state_removed",
            Self::StateRetentionFresh => "state_retention_fresh",
            Self::SignalTypeChanged => "signal_type_changed",
            Self::StoreTypeChanged => "store_type_changed",
            Self::QueryCacheTypeChanged => "query_cache_type_changed",
            Self::FormDraftTypeChanged => "form_draft_type_changed",
            Self::ScrollTypeChanged => "scroll_type_changed",
            Self::FocusTypeChanged => "focus_type_changed",
            Self::IslandTypeChanged => "island_type_changed",
            Self::IslandIdentityChanged => "island_identity_changed",
            Self::IdentityMismatch => "identity_mismatch",
            Self::SemanticIncompatible => "semantic_incompatible",
            Self::StalePlan => "stale_plan",
            Self::NothingToRollback => "nothing_to_rollback",
        }
    }
}

impl fmt::Display for WebModuleSwapReasonCode {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

/// Whether a candidate is safe for an in-place web module replacement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebModuleSwapCompatibility {
    Compatible,
    Incompatible {
        reason_codes: Vec<WebModuleSwapReasonCode>,
    },
}

impl WebModuleSwapCompatibility {
    pub fn is_compatible(&self) -> bool {
        matches!(self, Self::Compatible)
    }

    pub fn reason_codes(&self) -> &[WebModuleSwapReasonCode] {
        match self {
            Self::Compatible => &[],
            Self::Incompatible { reason_codes } => reason_codes,
        }
    }
}

/// One state fact that will not be reused by the candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebModuleSwapResetFact {
    pub state: WebModuleSwapStateFact,
    pub prior_identity: Option<String>,
    pub prior_type_fingerprint: Option<String>,
    pub reason: WebModuleSwapReasonCode,
}

/// Immutable semantic transaction prepared from two snapshots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebModuleSwapPlan {
    pub base: WebModuleSwapSnapshot,
    pub candidate: WebModuleSwapSnapshot,
    /// The sema-owned compatibility and retention verdict consumed by this
    /// adapter. This plan never re-derives either decision from snapshots.
    pub decision: HotSwapDecision,
    pub compatibility: WebModuleSwapCompatibility,
    pub changed_modules: Vec<String>,
    pub preserved: Vec<WebModuleSwapStateFact>,
    pub reset: Vec<WebModuleSwapResetFact>,
    pub reason_codes: Vec<WebModuleSwapReasonCode>,
}

impl WebModuleSwapPlan {
    /// Project one sema-owned verdict and its canonical state facts into the
    /// typed web transaction. Snapshot facts are marshalled here; they never
    /// decide whether a value is retained.
    pub fn from_hot_swap_decision(
        base: &WebModuleSwapSnapshot,
        candidate: &WebModuleSwapSnapshot,
        decision: &HotSwapDecision,
    ) -> Result<Self, WebModuleSwapError> {
        let mut base = base.clone();
        let mut candidate = candidate.clone();
        base.normalize()?;
        candidate.normalize()?;

        if decision.module.is_empty() || decision.module != base.identity.module {
            return Err(WebModuleSwapError::DecisionModuleMismatch {
                expected: base.identity.module.clone(),
                found: decision.module.clone(),
            });
        }

        let mut retention = BTreeMap::new();
        for fact in decision.state_facts() {
            if fact.key.is_empty() {
                return Err(WebModuleSwapError::InvalidSnapshot {
                    detail: "hot-swap retention keys must be non-empty".to_string(),
                });
            }
            if retention.insert(fact.key.as_str(), fact).is_some() {
                return Err(WebModuleSwapError::DuplicateRetentionKey {
                    key: fact.key.clone(),
                });
            }
        }

        let mut old_state = BTreeMap::new();
        for fact in &base.state {
            if old_state.insert(fact.key(), fact).is_some() {
                return Err(WebModuleSwapError::DuplicateRetentionKey {
                    key: fact.key().to_string(),
                });
            }
        }
        let mut new_state = BTreeMap::new();
        for fact in &candidate.state {
            if new_state.insert(fact.key(), fact).is_some() {
                return Err(WebModuleSwapError::DuplicateRetentionKey {
                    key: fact.key().to_string(),
                });
            }
        }
        for key in old_state.keys().chain(new_state.keys()) {
            if !retention.contains_key(key) {
                let fact = new_state.get(key).or_else(|| old_state.get(key)).expect("state key exists");
                return Err(WebModuleSwapError::MissingRetentionFact {
                    kind: fact.kind(),
                    key: (*key).to_string(),
                });
            }
        }
        for key in retention.keys() {
            if !old_state.contains_key(key) && !new_state.contains_key(key) {
                return Err(WebModuleSwapError::UnusedRetentionFact {
                    key: (*key).to_string(),
                });
            }
        }

        let mut reasons = BTreeSet::new();
        if base.source_revision != candidate.source_revision {
            reasons.insert(WebModuleSwapReasonCode::SourceRevisionChanged);
        }
        if base.build_revision != candidate.build_revision {
            reasons.insert(WebModuleSwapReasonCode::BuildRevisionChanged);
        }
        let changed = !decision.changed.is_empty() || !decision.changed_functions.is_empty();
        let changed_modules = if changed || !decision.is_compatible() {
            vec![decision.module.clone()]
        } else {
            Vec::new()
        };
        if !decision.changed_functions.is_empty() && decision.is_compatible() {
            reasons.insert(WebModuleSwapReasonCode::ModuleBodyChanged);
        }
        if !decision.is_compatible() {
            reasons.insert(WebModuleSwapReasonCode::SemanticIncompatible);
        }

        let mut preserved = Vec::new();
        let mut reset = Vec::new();
        for (key, new_fact) in &new_state {
            let retention_fact = retention.get(key).expect("checked web retention fact");
            match &retention_fact.decision {
                StateRetentionDecision::Preserve => {
                    let old_fact = old_state.get(key).ok_or_else(|| {
                        WebModuleSwapError::InvalidSnapshot {
                            detail: format!(
                                "retention fact `{key}` preserves state with no resident value"
                            ),
                        }
                    })?;
                    preserved.push(new_fact.with_value(old_fact.value().clone()));
                }
                StateRetentionDecision::Fresh { .. } => {
                    reasons.insert(WebModuleSwapReasonCode::StateRetentionFresh);
                    let (prior_identity, prior_type_fingerprint) = old_state
                        .get(key)
                        .map(|old_fact| {
                            (
                                Some(old_fact.identity().to_string()),
                                Some(old_fact.type_fingerprint().to_string()),
                            )
                        })
                        .unwrap_or((None, None));
                    reset.push(WebModuleSwapResetFact {
                        state: (*new_fact).clone(),
                        prior_identity,
                        prior_type_fingerprint,
                        reason: if old_state.contains_key(key) {
                            WebModuleSwapReasonCode::StateRetentionFresh
                        } else {
                            WebModuleSwapReasonCode::StateAdded
                        },
                    });
                }
            }
        }
        for (key, old_fact) in &old_state {
            if new_state.contains_key(key) {
                continue;
            }
            let retention_fact = retention.get(key).expect("checked web retention fact");
            if matches!(&retention_fact.decision, StateRetentionDecision::Preserve) {
                return Err(WebModuleSwapError::InvalidSnapshot {
                    detail: format!("retention fact `{key}` preserves removed state"),
                });
            }
            reasons.insert(WebModuleSwapReasonCode::StateRemoved);
            reset.push(WebModuleSwapResetFact {
                state: (*old_fact).clone(),
                prior_identity: Some(old_fact.identity().to_string()),
                prior_type_fingerprint: Some(old_fact.type_fingerprint().to_string()),
                reason: WebModuleSwapReasonCode::StateRemoved,
            });
        }

        if reasons.is_empty() {
            reasons.insert(WebModuleSwapReasonCode::NoChange);
        }
        preserved.sort_by(|left, right| {
            (left.kind(), left.key()).cmp(&(right.kind(), right.key()))
        });
        reset.sort_by(|left, right| {
            (left.state.kind(), left.state.key()).cmp(&(right.state.kind(), right.state.key()))
        });
        let reason_codes = reasons.into_iter().collect::<Vec<_>>();
        let compatibility = if decision.is_compatible() {
            WebModuleSwapCompatibility::Compatible
        } else {
            WebModuleSwapCompatibility::Incompatible {
                reason_codes: vec![WebModuleSwapReasonCode::SemanticIncompatible],
            }
        };
        Ok(Self {
            base,
            candidate,
            decision: decision.clone(),
            compatibility,
            changed_modules,
            preserved,
            reset,
            reason_codes,
        })
    }

    pub fn decision(&self) -> &HotSwapDecision {
        &self.decision
    }

    pub fn is_compatible(&self) -> bool {
        self.decision.is_compatible()
    }

    pub fn changed_module_set(&self) -> &[String] {
        &self.changed_modules
    }

    pub fn applied_snapshot(&self) -> WebModuleSwapSnapshot {
        let mut snapshot = self.candidate.clone();
        for preserved in &self.preserved {
            if let Some(slot) = snapshot.state.iter_mut().find(|fact| {
                fact.kind() == preserved.kind() && fact.key() == preserved.key()
            }) {
                *slot = preserved.clone();
            }
        }
        snapshot
    }
}


/// Transaction operation carried by every successful receipt.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WebModuleSwapOperation {
    Pause,
    Apply,
    Commit,
    Rollback,
    Restart,
}

impl WebModuleSwapOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Apply => "apply",
            Self::Commit => "commit",
            Self::Rollback => "rollback",
            Self::Restart => "restart",
        }
    }
}

impl fmt::Display for WebModuleSwapOperation {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

/// Transaction phase after a successful operation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WebModuleSwapPhase {
    Committed,
    Paused,
    Applied,
}

impl WebModuleSwapPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::Paused => "paused",
            Self::Applied => "applied",
        }
    }
}

impl fmt::Display for WebModuleSwapPhase {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

/// Receipt status for a completed transaction step.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WebModuleSwapReceiptStatus {
    Paused,
    Applied,
    Committed,
    RolledBack,
    Restarted,
}

impl WebModuleSwapReceiptStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::Applied => "applied",
            Self::Committed => "committed",
            Self::RolledBack => "rolled_back",
            Self::Restarted => "restarted",
        }
    }
}

/// Auditable outcome of one swap transaction step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebModuleSwapReceipt {
    pub operation: WebModuleSwapOperation,
    pub status: WebModuleSwapReceiptStatus,
    pub phase: WebModuleSwapPhase,
    pub identity: WebModuleSwapIdentity,
    pub candidate_identity: WebModuleSwapIdentity,
    pub source_revision: String,
    pub build_revision: String,
    pub changed_modules: Vec<String>,
    pub reason_codes: Vec<WebModuleSwapReasonCode>,
    pub preserved: Vec<WebModuleSwapStateFact>,
    pub reset: Vec<WebModuleSwapResetFact>,
    pub committed_page: WebPageFact,
    pub active_page: WebPageFact,
}

impl WebModuleSwapReceipt {
    pub fn changed_module_set(&self) -> &[String] {
        &self.changed_modules
    }

    pub fn preserved_facts(&self) -> &[WebModuleSwapStateFact] {
        &self.preserved
    }

    pub fn reset_facts(&self) -> &[WebModuleSwapResetFact] {
        &self.reset
    }

    /// Stable, line-oriented audit text. It is deliberately not a wire format.
    pub fn audit(&self) -> String {
        let changed = self.changed_modules.join(",");
        let reasons = self
            .reason_codes
            .iter()
            .map(|reason| reason.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let preserved = self
            .preserved
            .iter()
            .map(|fact| format!("{}:{}", fact.kind().as_str(), fact.key()))
            .collect::<Vec<_>>()
            .join(",");
        let reset = self
            .reset
            .iter()
            .map(|fact| {
                format!(
                    "{}:{}({})",
                    fact.state.kind().as_str(),
                    fact.state.key(),
                    fact.reason.as_str()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "operation={} status={} phase={} identity={}/{} candidate={}/{} source_revision={} build_revision={} changed_modules=[{}] reasons=[{}] preserved=[{}] reset=[{}] committed_page={} active_page={}",
            self.operation,
            self.status.as_str(),
            self.phase,
            self.identity.module,
            self.identity.page,
            self.candidate_identity.module,
            self.candidate_identity.page,
            self.source_revision,
            self.build_revision,
            changed,
            reasons,
            preserved,
            reset,
            self.committed_page.artifact,
            self.active_page.artifact,
        )
    }
}

/// Typed errors for planning and transaction transitions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebModuleSwapError {
    InvalidSnapshot { detail: String },
    IdentityMismatch {
        expected: WebModuleSwapIdentity,
        found: WebModuleSwapIdentity,
    },
    DecisionModuleMismatch {
        expected: String,
        found: String,
    },
    MissingRetentionFact {
        kind: WebStateKind,
        key: String,
    },
    DuplicateRetentionKey {
        key: String,
    },
    UnusedRetentionFact {
        key: String,
    },
    StalePlan {
        expected_source_revision: String,
        found_source_revision: String,
        expected_build_revision: String,
        found_build_revision: String,
    },
    Incompatible {
        reason_codes: Vec<WebModuleSwapReasonCode>,
    },
    InvalidTransition {
        operation: WebModuleSwapOperation,
        phase: WebModuleSwapPhase,
    },
    NoPendingPlan,
    NoAppliedPlan,
    NoCommittedPage,
}

impl fmt::Display for WebModuleSwapError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSnapshot { detail } => write!(output, "invalid web swap snapshot: {detail}"),
            Self::IdentityMismatch { expected, found } => write!(
                output,
                "web swap identity mismatch: expected {}/{} but found {}/{}",
                expected.module, expected.page, found.module, found.page
            ),
            Self::DecisionModuleMismatch { expected, found } => write!(
                output,
                "web swap decision module mismatch: expected `{expected}` but found `{found}`"
            ),
            Self::MissingRetentionFact { kind, key } => write!(
                output,
                "web swap has no canonical retention fact for {} `{key}`",
                kind.as_str()
            ),
            Self::DuplicateRetentionKey { key } => {
                write!(output, "web swap retention key `{key}` is not unique")
            }
            Self::UnusedRetentionFact { key } => {
                write!(output, "web swap retention fact `{key}` has no typed state")
            }
            Self::StalePlan {
                expected_source_revision,
                found_source_revision,
                expected_build_revision,
                found_build_revision,
            } => write!(
                output,
                "stale web swap plan: expected source/build {expected_source_revision}/{expected_build_revision} but state is {found_source_revision}/{found_build_revision}"
            ),
            Self::Incompatible { reason_codes } => write!(
                output,
                "web module swap is incompatible: {}",
                reason_codes
                    .iter()
                    .map(|reason| reason.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::InvalidTransition { operation, phase } => write!(
                output,
                "web swap cannot {} while phase is {}",
                operation, phase
            ),
            Self::NoPendingPlan => output.write_str("web swap has no pending plan"),
            Self::NoAppliedPlan => output.write_str("web swap has no applied plan"),
            Self::NoCommittedPage => output.write_str("web swap has no committed page"),
        }
    }
}

impl Error for WebModuleSwapError {}

/// Mutable owner of the last committed page and one in-flight transaction.
///
/// The committed snapshot is never replaced by `pause` or `apply`. A failed
/// candidate therefore leaves the last committed page available for rollback;
/// `commit` is the only operation that advances it.
#[derive(Debug)]
pub struct WebModuleSwapState {
    committed: WebModuleSwapSnapshot,
    pending: Option<WebModuleSwapPlan>,
    active: WebModuleSwapSnapshot,
    phase: WebModuleSwapPhase,
}

impl WebModuleSwapState {
    pub fn new(snapshot: WebModuleSwapSnapshot) -> Result<Self, WebModuleSwapError> {
        let mut snapshot = snapshot;
        snapshot.normalize()?;
        Ok(Self {
            active: snapshot.clone(),
            committed: snapshot,
            pending: None,
            phase: WebModuleSwapPhase::Committed,
        })
    }

    pub fn committed(&self) -> &WebModuleSwapSnapshot {
        &self.committed
    }

    pub fn active(&self) -> &WebModuleSwapSnapshot {
        &self.active
    }

    pub fn pending(&self) -> Option<&WebModuleSwapPlan> {
        self.pending.as_ref()
    }

    pub fn phase(&self) -> WebModuleSwapPhase {
        self.phase
    }

    /// Stage a compatible plan without changing the active or committed page.
    pub fn pause(
        &mut self,
        plan: WebModuleSwapPlan,
    ) -> Result<WebModuleSwapReceipt, WebModuleSwapError> {
        if self.phase != WebModuleSwapPhase::Committed || self.pending.is_some() {
            return Err(WebModuleSwapError::InvalidTransition {
                operation: WebModuleSwapOperation::Pause,
                phase: self.phase,
            });
        }
        self.validate_base(&plan)?;
        if plan.base.identity != plan.candidate.identity {
            return Err(WebModuleSwapError::IdentityMismatch {
                expected: plan.base.identity.clone(),
                found: plan.candidate.identity.clone(),
            });
        }
        if !plan.is_compatible() {
            return Err(WebModuleSwapError::Incompatible {
                reason_codes: plan.compatibility.reason_codes().to_vec(),
            });
        }
        self.pending = Some(plan);
        self.phase = WebModuleSwapPhase::Paused;
        Ok(self.receipt(
            WebModuleSwapOperation::Pause,
            WebModuleSwapReceiptStatus::Paused,
        ))
    }

    /// Apply the candidate to the active snapshot while keeping commit undoable.
    pub fn apply(&mut self) -> Result<WebModuleSwapReceipt, WebModuleSwapError> {
        if self.phase != WebModuleSwapPhase::Paused {
            return Err(WebModuleSwapError::InvalidTransition {
                operation: WebModuleSwapOperation::Apply,
                phase: self.phase,
            });
        }
        let plan = self.pending.as_ref().ok_or(WebModuleSwapError::NoPendingPlan)?;
        self.validate_base(plan)?;
        if !plan.is_compatible() {
            return Err(WebModuleSwapError::Incompatible {
                reason_codes: plan.compatibility.reason_codes().to_vec(),
            });
        }
        let applied = plan.applied_snapshot();
        self.active = applied;
        self.phase = WebModuleSwapPhase::Applied;
        Ok(self.receipt(
            WebModuleSwapOperation::Apply,
            WebModuleSwapReceiptStatus::Applied,
        ))
    }

    /// Make the applied candidate the new last-good snapshot.
    pub fn commit(&mut self) -> Result<WebModuleSwapReceipt, WebModuleSwapError> {
        if self.phase != WebModuleSwapPhase::Applied {
            return Err(WebModuleSwapError::InvalidTransition {
                operation: WebModuleSwapOperation::Commit,
                phase: self.phase,
            });
        }
        let plan = self.pending.as_ref().ok_or(WebModuleSwapError::NoAppliedPlan)?;
        self.validate_base(plan)?;
        if self.active.identity != plan.candidate.identity {
            return Err(WebModuleSwapError::IdentityMismatch {
                expected: plan.candidate.identity.clone(),
                found: self.active.identity.clone(),
            });
        }
        let plan = self.pending.take().expect("applied swap has a pending plan");
        self.committed = self.active.clone();
        self.phase = WebModuleSwapPhase::Committed;
        Ok(self.receipt_for_plan(
            &plan,
            WebModuleSwapOperation::Commit,
            WebModuleSwapReceiptStatus::Committed,
        ))
    }

    /// Drop an in-flight candidate and restore the last committed page.
    pub fn rollback(&mut self) -> Result<WebModuleSwapReceipt, WebModuleSwapError> {
        let had_pending = self.pending.is_some();
        let receipt_data = self.pending.as_ref().map(|plan| {
            (
                plan.candidate.identity.clone(),
                plan.candidate.source_revision.clone(),
                plan.candidate.build_revision.clone(),
                plan.changed_modules.clone(),
                plan.reason_codes.clone(),
                plan.preserved.clone(),
                plan.reset.clone(),
            )
        });
        self.pending = None;
        self.active = self.committed.clone();
        self.phase = WebModuleSwapPhase::Committed;
        if had_pending {
            let (candidate_identity, source_revision, build_revision, changed_modules, reason_codes, preserved, reset) =
                receipt_data.expect("pending plan was present");
            return Ok(WebModuleSwapReceipt {
                operation: WebModuleSwapOperation::Rollback,
                status: WebModuleSwapReceiptStatus::RolledBack,
                phase: self.phase,
                identity: self.committed.identity.clone(),
                candidate_identity,
                source_revision,
                build_revision,
                changed_modules,
                reason_codes,
                preserved,
                reset,
                committed_page: self.committed.page.clone(),
                active_page: self.active.page.clone(),
            });
        }
        Ok(WebModuleSwapReceipt {
            operation: WebModuleSwapOperation::Rollback,
            status: WebModuleSwapReceiptStatus::RolledBack,
            phase: self.phase,
            identity: self.committed.identity.clone(),
            candidate_identity: self.committed.identity.clone(),
            source_revision: self.committed.source_revision.clone(),
            build_revision: self.committed.build_revision.clone(),
            changed_modules: Vec::new(),
            reason_codes: vec![WebModuleSwapReasonCode::NothingToRollback],
            preserved: Vec::new(),
            reset: Vec::new(),
            committed_page: self.committed.page.clone(),
            active_page: self.active.page.clone(),
        })
    }
    /// Apply the canonical verdict without letting a host choose between
    /// in-place activation and a clean restart.
    pub fn apply_decision(
        &mut self,
        plan: WebModuleSwapPlan,
    ) -> Result<WebModuleSwapReceipt, WebModuleSwapError> {
        if !plan.is_compatible() {
            return self.restart(plan);
        }
        self.pause(plan)?;
        if let Err(error) = self.apply() {
            let _ = self.rollback();
            return Err(error);
        }
        match self.commit() {
            Ok(receipt) => Ok(receipt),
            Err(error) => {
                let _ = self.rollback();
                Err(error)
            }
        }
    }

    /// Publish an incompatible candidate as an explicit clean-restart
    /// transaction. The candidate becomes last-good with every canonical
    /// fresh-state fact visible in the receipt; this is not an in-place swap.
    pub fn restart(
        &mut self,
        plan: WebModuleSwapPlan,
    ) -> Result<WebModuleSwapReceipt, WebModuleSwapError> {
        if self.phase != WebModuleSwapPhase::Committed || self.pending.is_some() {
            return Err(WebModuleSwapError::InvalidTransition {
                operation: WebModuleSwapOperation::Restart,
                phase: self.phase,
            });
        }
        self.validate_base(&plan)?;
        if plan.base.identity != plan.candidate.identity {
            return Err(WebModuleSwapError::IdentityMismatch {
                expected: plan.base.identity.clone(),
                found: plan.candidate.identity.clone(),
            });
        }
        if plan.is_compatible() {
            return Err(WebModuleSwapError::Incompatible {
                reason_codes: vec![WebModuleSwapReasonCode::SemanticIncompatible],
            });
        }
        self.active = plan.candidate.clone();
        self.committed = self.active.clone();
        self.phase = WebModuleSwapPhase::Committed;
        Ok(self.receipt_for_plan(
            &plan,
            WebModuleSwapOperation::Restart,
            WebModuleSwapReceiptStatus::Restarted,
        ))
    }


    fn validate_base(&self, plan: &WebModuleSwapPlan) -> Result<(), WebModuleSwapError> {
        if self.committed.identity != plan.base.identity {
            return Err(WebModuleSwapError::IdentityMismatch {
                expected: self.committed.identity.clone(),
                found: plan.base.identity.clone(),
            });
        }
        if self.committed.source_revision != plan.base.source_revision
            || self.committed.build_revision != plan.base.build_revision
        {
            return Err(WebModuleSwapError::StalePlan {
                expected_source_revision: plan.base.source_revision.clone(),
                found_source_revision: self.committed.source_revision.clone(),
                expected_build_revision: plan.base.build_revision.clone(),
                found_build_revision: self.committed.build_revision.clone(),
            });
        }
        Ok(())
    }

    fn receipt(
        &self,
        operation: WebModuleSwapOperation,
        status: WebModuleSwapReceiptStatus,
    ) -> WebModuleSwapReceipt {
        let plan = self
            .pending
            .as_ref()
            .expect("successful swap operation has a pending plan");
        self.receipt_for_plan(plan, operation, status)
    }

    fn receipt_for_plan(
        &self,
        plan: &WebModuleSwapPlan,
        operation: WebModuleSwapOperation,
        status: WebModuleSwapReceiptStatus,
    ) -> WebModuleSwapReceipt {
        WebModuleSwapReceipt {
            operation,
            status,
            phase: self.phase,
            identity: self.committed.identity.clone(),
            candidate_identity: plan.candidate.identity.clone(),
            source_revision: plan.candidate.source_revision.clone(),
            build_revision: plan.candidate.build_revision.clone(),
            changed_modules: plan.changed_modules.clone(),
            reason_codes: plan.reason_codes.clone(),
            preserved: plan.preserved.clone(),
            reset: plan.reset.clone(),
            committed_page: self.committed.page.clone(),
            active_page: self.active.page.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jet_foundation::HotSwap::{HotSwapCompatibility, StateRetentionFact};

    fn snapshot(
        source_revision: &str,
        build_revision: &str,
        body: &str,
        signal_value: &str,
        island_identity: &str,
    ) -> WebModuleSwapSnapshot {
        WebModuleSwapSnapshot::new(
            WebModuleSwapIdentity::new("app", "/"),
            source_revision,
            build_revision,
            WebPageFact::new("/", format!("page-{build_revision}")),
            vec![WebModuleFact::new("app", "iface-1", body)],
            vec![
                WebModuleSwapStateFact::Signal(WebSignalFact::new(
                    "count",
                    "Int",
                    signal_value,
                )),
                WebModuleSwapStateFact::Store(WebStoreFact::new(
                    "session",
                    "Session-v1",
                    "store-value",
                )),
                WebModuleSwapStateFact::QueryCache(WebQueryCacheFact::new(
                    "todos",
                    "TodoList-v1",
                    "cached",
                )),
                WebModuleSwapStateFact::FormDraft(WebFormDraftFact::new(
                    "profile",
                    "ProfileForm-v1",
                    "dirty-draft",
                )),
                WebModuleSwapStateFact::Scroll(WebScrollFact::new(
                    "main",
                    "Scroll-v1",
                    42i64,
                )),
                WebModuleSwapStateFact::Focus(WebFocusFact::new(
                    "email",
                    "Focus-v1",
                    true,
                )),
                WebModuleSwapStateFact::Island(WebIslandFact::new(
                    "home",
                    island_identity,
                    "Island-v1",
                    "island-state",
                )),
            ],
        )
        .unwrap()
    }

    fn decision_for(
        before: &WebModuleSwapSnapshot,
        candidate: &WebModuleSwapSnapshot,
        compatible: bool,
        changed_functions: Vec<String>,
    ) -> HotSwapDecision {
        let mut keys = BTreeSet::new();
        keys.extend(before.state.iter().map(|fact| fact.key().to_string()));
        keys.extend(candidate.state.iter().map(|fact| fact.key().to_string()));
        HotSwapDecision {
            module: "app".to_string(),
            compatibility: if compatible {
                HotSwapCompatibility::Compatible
            } else {
                HotSwapCompatibility::Incompatible {
                    reason: "checked semantic surface changed".to_string(),
                }
            },
            changed: if compatible {
                Vec::new()
            } else {
                vec!["checked semantic surface changed".to_string()]
            },
            changed_functions,
            state: keys
                .into_iter()
                .map(|key| {
                    if compatible {
                        StateRetentionFact::preserve(key)
                    } else {
                        StateRetentionFact::fresh(key, "checked state shape changed")
                    }
                })
                .collect(),
            schema_migrations: Vec::new(),
            rechecked_items: Vec::new(),
            change_facts: Vec::new(),
        }
    }

    #[test]
    fn compatible_body_swap_preserves_every_supported_state_family() {
        let before = snapshot("source-1", "build-1", "body-1", "7", "island-a");
        let after = snapshot("source-2", "build-2", "body-2", "0", "island-a");
        let decision = decision_for(&before, &after, true, vec!["app::fn:main".to_string()]);
        let plan = WebModuleSwapPlan::from_hot_swap_decision(&before, &after, &decision).unwrap();

        assert!(plan.is_compatible());
        assert_eq!(plan.changed_module_set(), &["app".to_string()]);
        assert_eq!(plan.preserved.len(), 7);
        assert!(plan.reset.is_empty());
        assert!(plan
            .reason_codes
            .contains(&WebModuleSwapReasonCode::SourceRevisionChanged));
        assert!(plan
            .reason_codes
            .contains(&WebModuleSwapReasonCode::BuildRevisionChanged));

        let applied = plan.applied_snapshot();
        assert_eq!(applied.state[0].value(), &WebStateValue::Text("7".to_string()));
        assert_eq!(applied.state[1].value(), &WebStateValue::Text("store-value".to_string()));
        assert_eq!(applied.state[2].value(), &WebStateValue::Text("cached".to_string()));
        assert_eq!(applied.state[3].value(), &WebStateValue::Text("dirty-draft".to_string()));
        assert_eq!(applied.state[4].value(), &WebStateValue::Integer(42));
        assert_eq!(applied.state[5].value(), &WebStateValue::Boolean(true));
        assert_eq!(applied.state[6].value(), &WebStateValue::Text("island-state".to_string()));
    }

    #[test]
    fn identity_mismatch_is_rejected_before_state_mutation() {
        let before = snapshot("source-1", "build-1", "body-1", "7", "island-a");
        let candidate = WebModuleSwapSnapshot::new(
            WebModuleSwapIdentity::new("other-app", "/"),
            "source-2",
            "build-2",
            WebPageFact::new("/", "page-build-2"),
            vec![WebModuleFact::new("other-app", "iface-1", "body-2")],
            Vec::new(),
        )
        .unwrap();
        let decision = decision_for(&before, &candidate, true, Vec::new());
        let plan =
            WebModuleSwapPlan::from_hot_swap_decision(&before, &candidate, &decision).unwrap();
        let mut state = WebModuleSwapState::new(before.clone()).unwrap();

        let error = state.pause(plan).unwrap_err();
        assert_eq!(state.phase(), WebModuleSwapPhase::Committed);
        assert_eq!(state.committed(), &before);
        assert!(matches!(error, WebModuleSwapError::IdentityMismatch { .. }));
    }

    #[test]
    fn rollback_keeps_last_committed_page_after_apply() {
        let before = snapshot("source-1", "build-1", "body-1", "7", "island-a");
        let after = snapshot("source-2", "build-2", "body-2", "0", "island-a");
        let decision = decision_for(&before, &after, true, vec!["app::fn:main".to_string()]);
        let plan = WebModuleSwapPlan::from_hot_swap_decision(&before, &after, &decision).unwrap();
        let mut state = WebModuleSwapState::new(before.clone()).unwrap();

        let paused = state.pause(plan).unwrap();
        assert_eq!(paused.status, WebModuleSwapReceiptStatus::Paused);
        let applied = state.apply().unwrap();
        assert_eq!(applied.active_page.artifact, "page-build-2");
        assert_eq!(applied.committed_page.artifact, "page-build-1");
        let rolled_back = state.rollback().unwrap();
        assert_eq!(rolled_back.status, WebModuleSwapReceiptStatus::RolledBack);
        assert_eq!(rolled_back.committed_page.artifact, "page-build-1");
        assert_eq!(rolled_back.active_page.artifact, "page-build-1");
        assert_eq!(state.active(), &before);
        assert_eq!(state.committed(), &before);
    }

    #[test]
    fn incompatible_interface_reports_changed_modules_and_resets() {
        let before = snapshot("source-1", "build-1", "body-1", "7", "island-a");
        let mut after = snapshot("source-2", "build-2", "body-2", "0", "island-b");
        after.modules[0].interface_fingerprint = "iface-2".to_string();
        after.state[0] = WebModuleSwapStateFact::Signal(WebSignalFact::new(
            "count",
            "Int-v2",
            "0",
        ));
        let decision = decision_for(&before, &after, false, vec!["app::fn:main".to_string()]);
        let plan = WebModuleSwapPlan::from_hot_swap_decision(&before, &after, &decision).unwrap();

        assert!(!plan.is_compatible());
        assert_eq!(plan.changed_module_set(), &["app".to_string()]);
        assert!(plan
            .compatibility
            .reason_codes()
            .contains(&WebModuleSwapReasonCode::SemanticIncompatible));
        assert!(plan
            .reason_codes
            .contains(&WebModuleSwapReasonCode::StateRetentionFresh));
        assert!(plan.reset.iter().any(|fact| {
            fact.state.kind() == WebStateKind::Signal
                && fact.reason == WebModuleSwapReasonCode::StateRetentionFresh
        }));

        let mut state = WebModuleSwapState::new(before.clone()).unwrap();
        assert!(state.pause(plan).is_err());
        assert_eq!(state.committed(), &before);
    }

    #[test]
    fn receipt_audit_has_stable_order_and_all_preserved_facts() {
        let before = snapshot("source-1", "build-1", "body-1", "7", "island-a");
        let after = snapshot("source-2", "build-2", "body-2", "0", "island-a");
        let decision = decision_for(&before, &after, true, vec!["app::fn:main".to_string()]);
        let plan = WebModuleSwapPlan::from_hot_swap_decision(&before, &after, &decision).unwrap();
        let mut state = WebModuleSwapState::new(before).unwrap();
        let receipt = state.pause(plan).unwrap();
        let audit = receipt.audit();

        assert!(audit.contains("preserved=[signal:count,store:session,query_cache:todos,form_draft:profile,scroll:main,focus:email,island:home]"));
        assert!(audit.contains("changed_modules=[app]"));
        assert!(audit.contains("source_revision_changed"));
        assert!(audit.contains("build_revision_changed"));
    }
}
