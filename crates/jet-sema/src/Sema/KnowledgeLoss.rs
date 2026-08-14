use crate::Diagnostics::{Diagnostic, Span};

/// The spelled operation that is currently checking a bounded arithmetic
/// expression. The operation remains the source of truth; this marker only
/// lets the sema checker recognize its already-written boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KnowledgeGate {
    BoundedArithmetic,
}

impl KnowledgeGate {
    pub(crate) const fn allows_range_loss(self) -> bool {
        matches!(self, Self::BoundedArithmetic)
    }
}

/// The value facts named by the one-way law. Existing plane-specific checks
/// keep their ratified diagnostics; this row is the shared diagnostic for a
/// newly discovered silent loss at a sema boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KnowledgePlane {
    Range,
}

impl KnowledgePlane {
    const fn name(self) -> &'static str {
        match self {
            Self::Range => "range",
        }
    }
}

pub(crate) fn diagnostic(plane: KnowledgePlane, gate: &str, span: Span) -> Diagnostic {
    Diagnostic::from_row(
        "E0156",
        &[("plane", plane.name()), ("gate", gate)],
        Some(span),
    )
}
