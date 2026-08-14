//! D-FACT-LAW1=B / D-FACT-WORD1=A: one sema boundary for knowledge facts.
//!
//! Tightening is implicit. A loosening boundary is accepted only when the
//! existing source spelling is recognized as one of the ratified gate words.
//! The fact is compile-time-only; TIR and every execution adapter receive the
//! already-erased value.

use crate::Diagnostics::{Diagnostic, Span};

/// A written operation that may move a value away from one knowledge plane.
///
/// This is sema policy only. The source expression and the one `GateLedger`
/// remain the record of what the user wrote; this enum only answers whether a
/// boundary is already spelled while the checker is walking it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KnowledgeGate {
    BoundedArithmetic,
    Approximation,
    RoundedConversion,
    RawProjection,
    StateTransition,
    ClassificationScrub,
}

impl KnowledgeGate {
    pub(crate) const fn spelling(self) -> &'static str {
        match self {
            Self::BoundedArithmetic => crate::Syntax::BUILTIN_WRAPPING,
            Self::Approximation => crate::Syntax::BUILTIN_APPROX,
            Self::RoundedConversion => "rounded",
            Self::RawProjection => crate::Syntax::METHOD_DISTINCT_RAW,
            Self::StateTransition => crate::Syntax::KW_TRANSITION,
            Self::ClassificationScrub => crate::Syntax::KW_SCRUB,
        }
    }

    pub(crate) const fn permits(self, plane: KnowledgePlane) -> bool {
        match (plane, self) {
            (KnowledgePlane::Range, Self::BoundedArithmetic | Self::RawProjection)
            | (
                KnowledgePlane::Exactness,
                Self::Approximation | Self::RoundedConversion | Self::RawProjection,
            )
            | (KnowledgePlane::Unit, Self::RoundedConversion | Self::RawProjection)
            | (KnowledgePlane::State, Self::StateTransition | Self::RawProjection)
            | (
                KnowledgePlane::Classification,
                Self::ClassificationScrub | Self::RawProjection,
            ) => true,
            _ => false,
        }
    }
}

/// The value facts named by the one-way law. Existing plane-specific checks
/// keep their ratified diagnostics; the law decides the boundary once for all
/// five planes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KnowledgePlane {
    Range,
    Exactness,
    Unit,
    State,
    Classification,
}

impl KnowledgePlane {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Range => "range",
            Self::Exactness => "exactness",
            Self::Unit => "unit",
            Self::State => "state",
            Self::Classification => "classification",
        }
    }
}

/// True when this boundary has no written operation that permits the move.
///
/// `None` is the ordinary implicit path. A caller supplies `Some(gate)` only
/// after it has recognized the existing source spelling in its own AST rule.
pub(crate) const fn requires_gate(
    plane: KnowledgePlane,
    gate: Option<KnowledgeGate>,
) -> bool {
    !matches!(gate, Some(gate) if gate.permits(plane))
}

pub(crate) const fn allows_gate(plane: KnowledgePlane, gate: KnowledgeGate) -> bool {
    gate.permits(plane)
}

pub(crate) fn diagnostic(plane: KnowledgePlane, gate: &str, span: Span) -> Diagnostic {
    Diagnostic::from_row(
        "E0156",
        &[("plane", plane.name()), ("gate", gate)],
        Some(span),
    )
}

#[cfg(test)]
mod tests {
    use super::{allows_gate, requires_gate, KnowledgeGate, KnowledgePlane};

    #[test]
    fn one_law_covers_all_knowledge_planes() {
        let cases = [
            (KnowledgePlane::Range, KnowledgeGate::BoundedArithmetic),
            (KnowledgePlane::Exactness, KnowledgeGate::Approximation),
            (KnowledgePlane::Unit, KnowledgeGate::RoundedConversion),
            (KnowledgePlane::State, KnowledgeGate::StateTransition),
            (
                KnowledgePlane::Classification,
                KnowledgeGate::ClassificationScrub,
            ),
        ];
        for (plane, gate) in cases {
            assert!(requires_gate(plane, None));
            assert!(!requires_gate(plane, Some(gate)));
            assert!(allows_gate(plane, gate));
        }
    }

    #[test]
    fn raw_projection_is_the_explicit_escape_for_each_plane() {
        for plane in [
            KnowledgePlane::Range,
            KnowledgePlane::Exactness,
            KnowledgePlane::Unit,
            KnowledgePlane::State,
            KnowledgePlane::Classification,
        ] {
            assert!(!requires_gate(
                plane,
                Some(KnowledgeGate::RawProjection)
            ));
        }
    }
}
