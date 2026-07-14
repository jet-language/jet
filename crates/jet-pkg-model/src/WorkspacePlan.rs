//! `WorkspacePlan` and `WorkspaceMember` — the data types produced by
//! evaluating `workspace.jet` (D-WORKSPACE1=B, D-WORKSPACE2=A).
//!
//! These types live here (L1 data model) so both the evaluator
//! (`jet-env-model::WorkspaceFile`) and the lock reader
//! (`jet-pkg-model::WorkspaceLock`) share the same definition without
//! either depending on the other.

use crate::AST::ComptimeInput;
use crate::Overlay::OverlayPolicy;

/// The result of evaluating `workspace.jet`.
#[derive(Debug, Clone, Default)]
pub struct WorkspacePlan {
    /// Member packages in source order (the order `members:` produced them).
    pub members: Vec<WorkspaceMember>,
    /// D-CTEFFECT1 Tier-1: content-addressed inputs (`@embed`, `fetch(url,
    /// sha256:)`) that a `members:` expression pulled in during evaluation.
    /// Recorded into `.jet/lock` so the index is reproducible — a changed
    /// input invalidates the lock the same way it does for any other Tier-1
    /// call site.
    pub comptime_inputs: Vec<ComptimeInput>,
    /// D-JPK-OVERLAY1=A: reviewed package overlay/override policy from
    /// `workspace.jet`; CLI commands may draft this source but never create
    /// hidden override state.
    pub overlay_policy: OverlayPolicy,
}

/// One workspace member package.
#[derive(Debug, Clone)]
pub struct WorkspaceMember {
    /// Package name read from the member's `pkg.jet` (or derived from path).
    pub name: String,
    /// Path to the package directory, relative to the workspace root.
    pub path: String,
}
