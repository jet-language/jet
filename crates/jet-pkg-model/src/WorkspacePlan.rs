//! `WorkspacePlan`, workspace-source authority, and `WorkspaceMember` — the
//! data types produced by evaluating a workspace declaration
//! (D-WORKSPACE1=B, D-WORKSPACE2=A).
//!
//! These types live here (L1 data model) so both the evaluator
//! (`jet-env-model::WorkspaceFile`) and the lock reader
//! (`jet-pkg-model::WorkspaceLock`) share the same definition without
//! either depending on the other.

use crate::AST::ComptimeInput;
use crate::Diagnostics::Diagnostic;
use crate::Overlay::OverlayPolicy;
pub use crate::Authority::{
    AuthorityError, AuthorityKind, AuthorityResolver, CheckedDirectory, CheckedFile,
    CheckedManifest, CheckedMember, CheckedPackage, FileIdentity,
};
use std::path::{Path, PathBuf};

/// The role of a declaration-resolved workspace source.
///
/// `workspace.jet` is the D-WORKSPACE2 index. Other top-level declarations
/// establish an authority boundary, but are not workspace indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceSourceRole {
    Index,
    Authority,
}

/// The one declaration-resolved workspace source used by every authority
/// lookup.
#[derive(Debug, Clone)]
pub struct WorkspaceSource {
    pub path: PathBuf,
    pub source: String,
    pub role: WorkspaceSourceRole,
    /// The opened source and its identity. Consumers must revalidate this
    /// snapshot before using the source for authority-sensitive work.
    pub checked: CheckedFile,
}

/// The result of evaluating a workspace declaration.
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
    /// D-JPK-OVERLAY1=A: reviewed package overlay/override policy from the
    /// workspace declaration; CLI commands may draft this source but never
    /// create hidden override state.
    pub overlay_policy: OverlayPolicy,
    /// Digest of the workspace source that produced this plan. Locks may be
    /// reused only when the source bytes still have this identity.
    pub source_digest: String,
}

/// One workspace member package.
#[derive(Debug, Clone)]
pub struct WorkspaceMember {
    /// Package name read from the member's `package.jet` (or derived from path).
    pub name: String,
    /// Path to the package directory, relative to the workspace root.
    pub path: String,
    /// Checkout-relative canonical directory identity used by the workspace
    /// lock. Relative identity keeps a committed lock portable after a
    /// checkout moves.
    pub canonical_path: String,
}

/// Resolve the one top-level `.jet` source that declares `module workspace`.
///
/// The filename and declaration jointly select the role. A declared
/// `workspace.jet` is the strict D-WORKSPACE2 index. A declared workspace
/// module in any other top-level `.jet` file is an authority boundary and is
/// evaluated only for policy. A malformed canonical file still fails closed,
/// even when an arbitrary authority declaration is present beside it.
///
/// Every candidate is inspected in deterministic filename order. More than
/// one usable declaration is E1239. Any source-discovery I/O failure is an
/// error rather than an absent workspace so callers cannot fall through to an
/// outer authority or stale lock.
pub fn resolve_workspace_source(dir: &Path) -> Option<Result<WorkspaceSource, Diagnostic>> {
    let resolver = match AuthorityResolver::open(dir) {
        Ok(resolver) => resolver,
        Err(error) if error.is_missing() => return None,
        Err(error) => return Some(Err(error.workspace_diagnostic())),
    };
    match resolver.resolve_workspace_source() {
        Ok(Some(source)) => Some(Ok(source)),
        Ok(None) => None,
        Err(error) => Some(Err(error.workspace_diagnostic())),
    }
}

/// E0995: the canonical workspace source is present but has no workspace
/// declaration. Keep this diagnostic in the shared model so discovery and
/// evaluation cannot drift.
pub fn e0995_no_workspace_module() -> Diagnostic {
    Diagnostic::error(
        "E0995",
        format!(
            "`{}` must declare `module {} {{ … }}`",
            crate::Syntax::WORKSPACE_FILE,
            crate::Syntax::NS_WORKSPACE
        ),
        format!(
            "`{}` is the monorepo workspace index (D-WORKSPACE2=A); it must contain exactly one `module {} {{ members: … }}` body",
            crate::Syntax::WORKSPACE_FILE,
            crate::Syntax::NS_WORKSPACE
        ),
        format!(
            "write `module {} {{ members: find(\"./packages\") }}` in `{}`",
            crate::Syntax::NS_WORKSPACE,
            crate::Syntax::WORKSPACE_FILE
        ),
        None,
    )
}

/// Cheap token probe for a top-level `module workspace` candidate.
/// Full parsing stays in `jet-env-model::WorkspaceFile::evaluate`.
pub(crate) fn declares_workspace_module(src: &str) -> bool {
    let (tokens, _lex_diags) = crate::Lexer::lex(src);
    let tokens = crate::Lexer::without_comments(&tokens);
    let mut brace_depth = 0i32;
    let mut index = 0;
    while index + 1 < tokens.len() {
        match &tokens[index].kind {
            crate::Lexer::TokKind::LBrace => {
                brace_depth += 1;
                index += 1;
            }
            crate::Lexer::TokKind::RBrace => {
                brace_depth -= 1;
                index += 1;
            }
            crate::Lexer::TokKind::KwModule if brace_depth == 0 => {
                match &tokens[index + 1].kind {
                    crate::Lexer::TokKind::Ident(name)
                        if name == crate::Syntax::NS_WORKSPACE
                            && !name.starts_with(crate::Syntax::MODULE_INTERNAL_PREFIX) =>
                    {
                        return true;
                    }
                    _ => index += 1,
                }
            }
            _ => index += 1,
        }
    }
    false
}

/// E1239: the workspace authority must have one declaration source.
pub(crate) fn e1239_ambiguous_workspace(paths: &[&Path]) -> Diagnostic {
    let list = paths
        .iter()
        .map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string())
        })
        .collect::<Vec<_>>()
        .join("`, `");
    Diagnostic::error(
        "E1239",
        format!("`module workspace` is declared in more than one file: `{list}`"),
        "the workspace authority is discovered by declaration, so exactly one file may declare `module workspace { … }`".to_string(),
        "keep one declaration and delete the others".to_string(),
        None,
    )
}
