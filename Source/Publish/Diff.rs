use crate::Diagnostics::Diagnostic;
use super::SemVer::BumpKind;
use super::API::ApiItem;

// ──────────────────────────────────────────────
// API diff → E2601
// ──────────────────────────────────────────────

/// Compare old and new public API surfaces and return a list of breaking
/// changes. A change is breaking when an item is removed or its signature
/// changes (any method removed from a trait, any field removed from a pub
/// struct, any function's parameter list or return type changed).
#[derive(Debug, Clone)]
pub struct BreakingChange {
    /// Human-readable description of the broken item.
    pub description: String,
    /// The item name for the diagnostic span label.
    pub item_name: String,
}

pub fn diff_public_api(old: &[ApiItem], new: &[ApiItem]) -> Vec<BreakingChange> {
    let mut changes = Vec::new();
    let mut matched = vec![false; new.len()];
    for old_item in old {
        if let Some((index, _)) = new.iter().enumerate().find(|(index, item)| {
            !matched[*index]
                && item.kind == old_item.kind
                && item.name == old_item.name
                && item.signature == old_item.signature
        }) {
            matched[index] = true;
            continue;
        }
        if let Some((index, new_item)) = new.iter().enumerate().find(|(index, item)| {
            !matched[*index] && item.kind == old_item.kind && item.name == old_item.name
        }) {
            matched[index] = true;
            changes.push(BreakingChange {
                description: format!(
                    "pub {} `{}` changed signature\n   | was: {}\n   | now: {}",
                    old_item.kind, old_item.name, old_item.signature, new_item.signature
                ),
                item_name: old_item.name.clone(),
            });
        } else {
            changes.push(BreakingChange {
                description: format!(
                    "pub {} `{}` was removed\n   | {}\n   | (removed)",
                    old_item.kind, old_item.name, old_item.signature
                ),
                item_name: old_item.name.clone(),
            });
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(name: &str, signature: &str) -> ApiItem {
        ApiItem {
            kind: "fn".to_string(),
            name: name.to_string(),
            signature: signature.to_string(),
        }
    }

    #[test]
    fn duplicate_legacy_names_match_as_signature_multisets() {
        let old = vec![function("report", "fn report()"), function("report", "fn report(x: Int)")];
        let new = vec![function("report", "fn report(x: Int)"), function("report", "fn report()")];
        assert!(diff_public_api(&old, &new).is_empty());
    }
}

/// E2601 — publishing would break SemVer.
pub fn e2601(
    version: &str,
    bump_kind: BumpKind,
    change: &BreakingChange,
    next_major: u64,
) -> Diagnostic {
    let bump_str = match bump_kind {
        BumpKind::Minor => "minor",
        BumpKind::Patch => "patch",
        _ => "non-breaking",
    };
    Diagnostic::error(
        "E2601",
        format!(
            "this release is tagged {} but removes public API",
            version
        ),
        format!(
            "{} is a {} bump, which promises no breaking changes. Callers pinned to ^{}.0 would stop compiling.\n  {}",
            version,
            bump_str,
            // Extract the major from version
            version.split('.').next().unwrap_or("?"),
            change.description,
        ),
        format!(
            "bump to {}.0.0, or restore `{}` (a deprecated shim counts). Use `jet registry publish --force` to override with a warning banner.",
            next_major,
            change.item_name,
        ),
        None,
    )
}

/// E1218 — a breaking public-API change requires a major version bump
/// (D-SUPPLY1, Step 3). Distinct from E2601 (the registry-side SemVer gate):
/// E1218 is the local publish-time gate that compares the current public API
/// against the previously-published API snapshot and refuses a non-major bump
/// that drops or changes public surface.
pub fn e1218(
    old_version: &str,
    new_version: &str,
    bump: BumpKind,
    change: &BreakingChange,
    next_major: u64,
) -> Diagnostic {
    let bump_str = match bump {
        BumpKind::Minor => "minor",
        BumpKind::Patch => "patch",
        BumpKind::Same => "unchanged",
        BumpKind::Major => "major",
    };
    Diagnostic::error(
        "E1218",
        format!(
            "publishing {} after {} is a {} bump but breaks the public API",
            new_version, old_version, bump_str
        ),
        format!(
            "a {} bump promises callers no breaking changes, but the public API changed since {}:\n  {}",
            bump_str, old_version, change.description
        ),
        format!(
            "bump to {}.0.0 (a major release), or restore `{}` (a deprecated shim counts).",
            next_major, change.item_name
        ),
        None,
    )
}

// ──────────────────────────────────────────────
// Pre-publish gate (D-PKGS4 amended)
// ──────────────────────────────────────────────

/// Pre-publish gate outcome.
#[derive(Debug)]
pub struct PrePublishGate {
    pub build_ok: bool,
    pub tests_ok: bool,
    /// API breaking changes found (E2601 candidates).
    pub breaking: Vec<BreakingChange>,
    pub version: String,
    pub bump_kind: BumpKind,
    pub next_major: u64,
}

impl PrePublishGate {
    /// `true` when the publish should be blocked (failing gate, or breaking change
    /// under a non-breaking bump).
    pub fn is_blocked(&self) -> bool {
        !self.build_ok
            || !self.tests_ok
            || (!self.breaking.is_empty()
                && matches!(
                    self.bump_kind,
                    BumpKind::Minor | BumpKind::Patch | BumpKind::Same
                ))
    }

    /// Build E2601 diagnostics for every breaking change.
    pub fn semver_errors(&self) -> Vec<Diagnostic> {
        if matches!(self.bump_kind, BumpKind::Major) {
            return vec![];
        }
        self.breaking
            .iter()
            .map(|c| e2601(&self.version, self.bump_kind, c, self.next_major))
            .collect()
    }
}
