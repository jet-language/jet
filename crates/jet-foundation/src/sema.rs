//! Backend-neutral rights rows and the one semantic walker.
//!
//! D-RIGHTS1=C keeps the surface spellings but gives them one typed row. The
//! row is compile-time metadata: every checker supplies the effects it found,
//! and this module answers the same allow/deny question without knowing a
//! backend, MIR, or execution tier.

use crate::Authority::{self, Holds, Scope, Verdict};

/// The source spelling or semantic preset that produced one rights row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RightsRowKind {
    Callable,
    Pure,
    Replayable,
    Comptime,
    Invocation,
    Transaction,
}

impl RightsRowKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Callable => "callable",
            Self::Pure => "pure",
            Self::Replayable => "replayable",
            Self::Comptime => "comptime",
            Self::Invocation => "invocation",
            Self::Transaction => "transaction",
        }
    }
}

/// Why a row exists. The chain is kept with the row instead of reconstructed
/// by a diagnostic or an execution tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RightsProvenance {
    pub source: String,
    pub reason: String,
}

impl RightsProvenance {
    pub fn new(source: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            reason: reason.into(),
        }
    }
}

/// One callable rights row. `allow == None` is an unbounded row; `Some(empty)`
/// is the explicit pure-empty row. Denials always win over grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RightsRow {
    pub kind: RightsRowKind,
    pub allow: Option<Holds>,
    pub deny: Holds,
    pub provenance: RightsProvenance,
}

impl RightsRow {
    pub fn new(
        kind: RightsRowKind,
        allow: Option<Holds>,
        deny: Holds,
        provenance: RightsProvenance,
    ) -> Self {
        Self {
            kind,
            allow: allow.map(|rights| normalize_holds(&rights)),
            deny: normalize_holds(&deny),
            provenance,
        }
    }

    /// The ordinary inferred callable row: no bound, no prohibition.
    pub fn callable(provenance: RightsProvenance) -> Self {
        Self::new(RightsRowKind::Callable, None, Holds::new(), provenance)
    }

    /// `-[]>`: the explicit empty row. Every reached effect is missing.
    pub fn pure(provenance: RightsProvenance) -> Self {
        Self::new(
            RightsRowKind::Pure,
            Some(Holds::new()),
            Holds::new(),
            provenance,
        )
    }

    /// `#Replayable`: an unbounded row with the four ambient nondeterministic
    /// roots denied. A denied root covers every qualified descendant.
    pub fn replayable(provenance: RightsProvenance) -> Self {
        Self::new(
            RightsRowKind::Replayable,
            None,
            Holds::from([
                "Time".to_string(),
                "Rand".to_string(),
                "Net".to_string(),
                "IO".to_string(),
            ]),
            provenance,
        )
    }

    /// A compile-time block may use only the memory row.
    pub fn comptime(provenance: RightsProvenance) -> Self {
        Self::new(
            RightsRowKind::Comptime,
            Some(Holds::from(["Mem".to_string()])),
            Holds::new(),
            provenance,
        )
    }

    /// Invocation grants and denials use the manifest's roots or leaves. Keep
    /// the original root as well as its known leaves so open effect trees do
    /// not become narrower merely because a leaf is not in the Prelude list.
    pub fn invocation<I, A, J, D>(
        allow: I,
        deny: J,
        provenance: RightsProvenance,
    ) -> Self
    where
        I: IntoIterator<Item = A>,
        A: AsRef<str>,
        J: IntoIterator<Item = D>,
        D: AsRef<str>,
    {
        let allow = expand_leaves(allow);
        let deny = expand_leaves(deny);
        Self::new(RightsRowKind::Invocation, Some(allow), deny, provenance)
    }

    /// The transaction checker retains its undo-duty logic, but its
    /// irreversibility vocabulary comes from the declared Prelude facts.
    pub fn transaction(provenance: RightsProvenance) -> Self {
        Self::new(
            RightsRowKind::Transaction,
            None,
            irreversible_effects(),
            provenance,
        )
    }

    /// Lower a named preset to the same row consumed by [`walk`].
    pub fn preset(kind: RightsRowKind, provenance: RightsProvenance) -> Self {
        match kind {
            RightsRowKind::Callable => Self::callable(provenance),
            RightsRowKind::Pure => Self::pure(provenance),
            RightsRowKind::Replayable => Self::replayable(provenance),
            RightsRowKind::Comptime => Self::comptime(provenance),
            RightsRowKind::Invocation => Self::invocation(
                std::iter::empty::<&str>(),
                std::iter::empty::<&str>(),
                provenance,
            ),
            RightsRowKind::Transaction => Self::transaction(provenance),
        }
    }

    pub fn decide(&self, right: &str) -> Verdict {
        if Authority::covers_any(&self.deny, right) {
            Verdict::Denied
        } else if self
            .allow
            .as_ref()
            .is_none_or(|allow| Authority::covers_any(allow, right))
        {
            Verdict::Allowed
        } else {
            Verdict::Missing
        }
    }

    /// Walk every reached right. The result retains every denial rather than
    /// collapsing different failures into one score.
    pub fn walk(
        &self,
        effects: &Holds,
        call_chain: &[String],
        scope_chain: &[ScopeFrame],
    ) -> RightsWalk {
        let denials = effects
            .iter()
            .filter_map(|right| {
                let verdict = self.decide(right);
                (verdict != Verdict::Allowed).then(|| RightsDenial {
                    right: right.clone(),
                    verdict,
                    row: self.provenance.clone(),
                    chain: DenialChain::new(right, call_chain, scope_chain),
                })
            })
            .collect::<Vec<_>>();
        let verdict = if denials
            .iter()
            .any(|denial| denial.verdict == Verdict::Denied)
        {
            Verdict::Denied
        } else if denials
            .iter()
            .any(|denial| denial.verdict == Verdict::Missing)
        {
            Verdict::Missing
        } else {
            Verdict::Allowed
        };
        RightsWalk {
            kind: self.kind,
            verdict,
            denials,
        }
    }

    /// Compare a legacy answer one right at a time. This is the Phase A seam:
    /// callers may instrument old walkers while retaining their verdict until
    /// the deferred differential matrix proves equality.
    pub fn observe<F>(
        &self,
        callable: impl Into<String>,
        effects: &Holds,
        call_chain: &[String],
        scope_chain: &[ScopeFrame],
        legacy: F,
    ) -> Result<RightsWalk, RightsMismatch>
    where
        F: Fn(&str) -> Verdict,
    {
        let walk = self.walk(effects, call_chain, scope_chain);
        let differences = effects
            .iter()
            .filter_map(|right| {
                let canonical = self.decide(right);
                let observed = legacy(right);
                (canonical != observed).then(|| RightsDifference {
                    right: right.clone(),
                    canonical,
                    legacy: observed,
                })
            })
            .collect::<Vec<_>>();
        if differences.is_empty() {
            Ok(walk)
        } else {
            Err(RightsMismatch {
                callable: callable.into(),
                kind: self.kind,
                row: self.provenance.clone(),
                differences,
            })
        }
    }
}

/// One scope in the outer-to-inner authority chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeFrame {
    pub scope: Scope,
    pub name: String,
    pub grants: Holds,
    pub provenance: RightsProvenance,
}

impl ScopeFrame {
    pub fn new(
        scope: Scope,
        name: impl Into<String>,
        grants: Holds,
        provenance: RightsProvenance,
    ) -> Self {
        Self {
            scope,
            name: name.into(),
            grants: normalize_holds(&grants),
            provenance,
        }
    }
}

/// One typed denial chain for every refused right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenialChain {
    pub right: String,
    pub call_chain: Vec<String>,
    pub scope_chain: Vec<ScopeFrame>,
    pub nearest_granting_scope: Option<ScopeFrame>,
}

impl DenialChain {
    pub fn new(right: &str, call_chain: &[String], scope_chain: &[ScopeFrame]) -> Self {
        let nearest_granting_scope = scope_chain
            .iter()
            .rev()
            .find(|frame| Authority::covers_any(&frame.grants, right))
            .cloned();
        Self {
            right: right.to_string(),
            call_chain: call_chain.to_vec(),
            scope_chain: scope_chain.to_vec(),
            nearest_granting_scope,
        }
    }
}

/// One refusal produced by the canonical walker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RightsDenial {
    pub right: String,
    pub verdict: Verdict,
    pub row: RightsProvenance,
    pub chain: DenialChain,
}

/// Result of one row walk. `denials` contains all failing rights in stable
/// order, so callers never average an allowed and a refused fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RightsWalk {
    pub kind: RightsRowKind,
    pub verdict: Verdict,
    pub denials: Vec<RightsDenial>,
}

impl RightsWalk {
    pub const fn allowed(&self) -> bool {
        matches!(self.verdict, Verdict::Allowed)
    }
}

/// One per-right difference between an old checker and the canonical row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RightsDifference {
    pub right: String,
    pub canonical: Verdict,
    pub legacy: Verdict,
}

/// Explicit differential failure. No aggregate pass/fail score hides which
/// fact differed or which row provenance produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RightsMismatch {
    pub callable: String,
    pub kind: RightsRowKind,
    pub row: RightsProvenance,
    pub differences: Vec<RightsDifference>,
}

/// The one canonical walker entry point.
pub fn walk(
    row: &RightsRow,
    effects: &Holds,
    call_chain: &[String],
    scope_chain: &[ScopeFrame],
) -> RightsWalk {
    row.walk(effects, call_chain, scope_chain)
}

/// Expand roots into the known Prelude leaves while retaining the root itself
/// for open, user-defined descendants.
pub fn expand_leaves<I, S>(rights: I) -> Holds
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let known_leaves = crate::Syntax::BUILTIN_EFFECT_LEAVES
        .iter()
        .copied()
        .chain(
            Authority::effect_declarations()
                .iter()
                .filter(|declaration| declaration.name.contains('.'))
                .map(|declaration| declaration.name),
        )
        .collect::<Vec<_>>();
    let mut expanded = Holds::new();
    for right in rights {
        let canonical = Authority::parse_right(right.as_ref())
            .unwrap_or_else(|| right.as_ref().trim().to_string());
        expanded.insert(canonical.clone());
        for leaf in &known_leaves {
            if Authority::covers(&canonical, leaf) {
                expanded.insert((*leaf).to_string());
            }
        }
    }
    expanded
}

/// Source-declared irreversible roots/leaves, including the `FFI` root.
pub fn irreversible_effects() -> Holds {
    Authority::effect_declarations()
        .iter()
        .filter(|declaration| declaration.irreversible)
        .map(|declaration| declaration.name.to_string())
        .collect()
}

pub fn is_irreversible(right: &str) -> bool {
    Authority::is_declared_irreversible(right)
}

fn normalize_holds(rights: &Holds) -> Holds {
    rights
        .iter()
        .map(|right| {
            Authority::parse_right(right).unwrap_or_else(|| right.trim().to_string())
        })
        .collect()
}
