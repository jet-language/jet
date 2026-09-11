//! Backend-neutral facts for one live source edit.
//!
//! Sema owns the compatibility verdict and state-retention facts. Execution
//! adapters consume this record; they must not compare source or type shapes a
//! second time.

use crate::Game::JetGameChangeFact;
use crate::SchemaMigration::SchemaMigrationPlan;
/// Whether the changed module can be redefined in the resident process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HotSwapCompatibility {
    /// The module's runtime type surface is unchanged.
    Compatible,
    /// The module's runtime type surface changed and needs a clean restart.
    Incompatible { reason: String },
}

impl HotSwapCompatibility {
    /// Return whether a resident code replacement is permitted.
    pub const fn is_compatible(&self) -> bool {
        matches!(self, Self::Compatible)
    }

    /// Return the semantic restart reason, if the edit is incompatible.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Compatible => None,
            Self::Incompatible { reason } => Some(reason),
        }
    }
}

/// The semantic action an adapter must take for one persistent state key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateRetentionDecision {
    /// Keep the existing value associated with the key.
    Preserve,
    /// Do not reuse the existing value; seed the key from the new bundle.
    Fresh { reason: String },
}

/// One state-key retention fact emitted by sema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateRetentionFact {
    /// Canonical `module.alias::binding` identity used by `PersistStore`.
    pub key: String,
    pub decision: StateRetentionDecision,
}

impl StateRetentionFact {
    /// Build a fact that keeps the value for `key`.
    pub fn preserve(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            decision: StateRetentionDecision::Preserve,
        }
    }

    /// Build a fact that seeds `key` from the new declaration.
    pub fn fresh(key: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            decision: StateRetentionDecision::Fresh {
                reason: reason.into(),
            },
        }
    }

    /// Return whether this fact keeps the existing state value.
    pub const fn is_preserved(&self) -> bool {
        matches!(self.decision, StateRetentionDecision::Preserve)
    }
}

/// Complete semantic verdict for one changed module.
///
/// `changed` explains the compatibility verdict. `changed_functions` carries
/// canonical sema identities (`module.display::fn:name` and the matching
/// method forms) for every changed callable. `rechecked_items` is filled by the
/// incremental sema entry after the existing dependency/cache walk; adapters
/// may display it but never use it to make a second invalidation decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HotSwapDecision {
    /// Stable module display/alias selected by the caller.
    pub module: String,
    pub compatibility: HotSwapCompatibility,
    pub changed: Vec<String>,
    pub changed_functions: Vec<String>,
    pub state: Vec<StateRetentionFact>,
    /// Canonical `#PublishedSchema` migration plans supplied by sema. The
    /// resident adapter applies these plans; it never reconstructs migration
    /// operations from source or a second schema table.
    pub schema_migrations: Vec<SchemaMigrationPlan>,
    pub rechecked_items: Vec<String>,
    /// Checked game change facts supplied by the watcher/build/package seam.
    /// Adapters publish these facts but never infer them from paths.
    pub change_facts: Vec<JetGameChangeFact>,
}

impl HotSwapDecision {
    /// Return whether the resident adapter may redefine code in place.
    pub fn is_compatible(&self) -> bool {
        self.compatibility.is_compatible()
    }

    /// Return whether the adapter must perform a clean restart.
    pub fn requires_restart(&self) -> bool {
        !self.is_compatible()
    }

    /// Attach the measured incremental semantic cone to this verdict.
    pub fn with_rechecked_items(mut self, mut items: Vec<String>) -> Self {
        items.sort();
        items.dedup();
        self.rechecked_items = items;
        self
    }

    /// Attach checked game change facts from the watcher/build/package seam.
    pub fn with_change_facts(mut self, mut facts: Vec<JetGameChangeFact>) -> Self {
        facts.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.kind.cmp(&right.kind))
        });
        self.change_facts = facts;
        self
    }

    /// Attach canonical published-schema migration plans from sema.
    pub fn with_schema_migrations(mut self, mut plans: Vec<SchemaMigrationPlan>) -> Self {
        plans.sort_by(|left, right| left.type_name.cmp(&right.type_name));
        self.schema_migrations = plans;
        self
    }

    /// Borrow canonical migration plans without exposing adapter storage.
    pub fn schema_migrations(&self) -> &[SchemaMigrationPlan] {
        &self.schema_migrations
    }

    /// Borrow checked game change facts without exposing adapter storage.
    pub fn change_facts(&self) -> &[JetGameChangeFact] {
        &self.change_facts
    }

    /// Borrow state-retention facts without exposing adapter-specific storage.
    pub fn state_facts(&self) -> &[StateRetentionFact] {
        &self.state
    }

    /// Borrow the measured semantic recheck set.
    pub fn rechecked(&self) -> &[String] {
        &self.rechecked_items
    }
}
