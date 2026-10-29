//! D-AUTHORITY-MODEL1=A: one rights tree and one holds relation, as the
//! `core.plugin` host checks them.
//!
//! The compiler-side authority model is Jet (Compiler/JetFoundation
//! Policy/Authority.jet). This is the plugin host's projection of the same
//! laws: canonical roots come from `Compiler/Prelude/Effects.jet`, so the host
//! never keeps a second root table.

use std::collections::BTreeSet;
use std::sync::LazyLock;

const EFFECT_SOURCE: &str = include_str!("../../../../../../../../Compiler/Prelude/Effects.jet");

/// The grantable roots: every undotted `effect` declaration except the
/// deny-only `Panic` and `Mem`.
static EFFECT_ROOTS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    EFFECT_SOURCE
        .lines()
        .filter_map(|line| line.strip_prefix("effect "))
        .filter_map(|rest| rest.split_whitespace().next())
        .filter(|name| !name.contains('.') && *name != "Panic" && *name != "Mem")
        .collect()
});

/// Root segment of a dotted right (`FS.Read` → `FS`).
pub fn root(right: &str) -> &str {
    right.split('.').next().unwrap_or(right)
}

/// Resolve one root using the canonical table. Dotted input is accepted only
/// for its root; leaf declaration remains sema's job.
pub fn parse_root(right: &str) -> Option<&'static str> {
    let root = root(right);
    if root.eq_ignore_ascii_case("Panic") {
        return Some("Panic");
    }
    if root.eq_ignore_ascii_case("Mem") {
        return Some("Mem");
    }
    EFFECT_ROOTS
        .iter()
        .copied()
        .find(|known| known.eq_ignore_ascii_case(root))
}

/// Preserve a right's leaf spelling while normalizing its canonical root.
pub fn parse_right(right: &str) -> Option<String> {
    let right = right.trim();
    let root = parse_root(right)?;
    if root == "Panic" && right != "Panic" {
        return None;
    }
    Some(match right.split_once('.') {
        Some((_, leaf)) => format!("{root}.{leaf}"),
        None => root.to_string(),
    })
}

/// D-EFFTREE1: a bound covers itself and every descendant in the rights tree.
///
/// Resource-qualified rights use `:` for hosts and paths (for example,
/// `FS.Read:/data` or `Net.Connect:api.example.com`). A qualified descendant
/// must cross a separator boundary, so `/data` does not accidentally cover
/// `/database`.
pub fn covers(bound: &str, right: &str) -> bool {
    let bound = parse_right(bound).unwrap_or_else(|| bound.trim().to_string());
    let right = parse_right(right).unwrap_or_else(|| right.trim().to_string());
    let qualified = bound.contains(':');
    right == bound
        || right
            .strip_prefix(&bound)
            .is_some_and(|suffix| {
                suffix.as_bytes().first().is_some_and(|byte| {
                    if qualified {
                        *byte == b'/'
                    } else {
                        matches!(byte, b'.' | b':' | b'/')
                    }
                })
            })
}

/// One rights carrier for every authority checkpoint.
pub type Holds = BTreeSet<String>;

pub fn covers_any(bounds: &Holds, right: &str) -> bool {
    bounds.iter().any(|bound| covers(bound, right))
}

/// The canonical authority value a plugin handle carries. Sandbox wrappers
/// must not maintain a second grant set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Authority {
    holds: Holds,
}

impl Authority {
    /// Construct an authority from canonical or resource-qualified rights.
    /// Unknown spellings are retained for the caller's diagnostic; policy
    /// checks still fail closed because they are never covered by a known hold.
    pub fn from_rights<I, S>(rights: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let holds = rights
            .into_iter()
            .map(Into::into)
            .map(|right| parse_right(&right).unwrap_or(right))
            .collect();
        Self { holds }
    }

    pub fn rights(&self) -> impl Iterator<Item = &str> {
        self.holds.iter().map(String::as_str)
    }

    pub fn allows(&self, right: &str) -> bool {
        covers_any(&self.holds, right)
    }

    /// True when this authority can satisfy a host operation whose declared
    /// right is `right`.  Unlike `allows`, this also accepts a resource
    /// qualified hold (`FS.Read:/data`) for the unqualified operation
    /// (`FS.Read`).  The operation is still bounded by the held descendant;
    /// callers must apply that resource to their path/host argument.
    pub fn allows_operation(&self, right: &str) -> bool {
        self.allows(right) || self.holds.iter().any(|held| covers(right, held))
    }

    /// Check one typed host import against this authority without letting an
    /// adapter reinterpret the right or keep a parallel grant table.
    pub fn decide_import(&self, import: &HostImportFact) -> Verdict {
        if self.allows_operation(import.required_grant()) {
            Verdict::Allowed
        } else {
            Verdict::Missing
        }
    }
}

/// Stable, typed result of checking one host import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostImportDecision {
    pub import: HostImportFact,
    pub verdict: Verdict,
}

impl HostImportDecision {
    pub fn check(authority: &Authority, import: HostImportFact) -> Self {
        let verdict = authority.decide_import(&import);
        Self { import, verdict }
    }

    pub fn is_allowed(&self) -> bool {
        self.verdict == Verdict::Allowed
    }
}

/// Backend-neutral description of one host-provided operation. Payloads are
/// copied at the boundary and refer to the existing interface snapshot type
/// identities rather than introducing a second wire type registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostImportFact {
    pub id: String,
    pub operation: String,
    pub required_right: String,
    pub parameter_type_ids: Vec<String>,
    pub result_type_id: Option<String>,
}

impl HostImportFact {
    pub fn new(
        id: impl Into<String>,
        operation: impl Into<String>,
        required_right: impl Into<String>,
        parameter_type_ids: impl IntoIterator<Item = String>,
        result_type_id: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            operation: operation.into(),
            required_right: required_right.into(),
            parameter_type_ids: parameter_type_ids.into_iter().collect(),
            result_type_id,
        }
    }

    pub fn decision(&self, authority: &Authority) -> HostImportDecision {
        HostImportDecision::check(authority, self.clone())
    }

    pub fn required_grant(&self) -> &str {
        &self.required_right
    }
}

/// The authority fact shared by the plugin loader and every call on the
/// resulting handle, so adapters cannot invent operation-specific grant names.
pub fn plugin_authority_import_fact() -> HostImportFact {
    HostImportFact::new(
        "core.plugin.authority",
        "core.plugin.host-import",
        "FS.Read",
        ["String".to_string(), "Authority".to_string()],
        Some("Plugin".to_string()),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Missing,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_roots_are_the_thirteen_grantable_roots() {
        assert_eq!(
            EFFECT_ROOTS.as_slice(),
            &[
                "Net", "FS", "IO", "DB", "Time", "Rand", "Env", "Exec", "Log", "GPU", "FFI",
                "Browser", "Secret",
            ]
        );
        assert_eq!(parse_right("Panic").as_deref(), Some("Panic"));
        assert_eq!(parse_right("Mem.Alloc").as_deref(), Some("Mem.Alloc"));
        assert_eq!(parse_right("Panic.Now"), None);
    }

    #[test]
    fn rights_parse_to_their_canonical_root() {
        assert_eq!(parse_right("fs.Read").as_deref(), Some("FS.Read"));
        assert_eq!(parse_right("fs.read").as_deref(), Some("FS.read"));
        assert_eq!(parse_right("Nope"), None);
    }

    #[test]
    fn a_bound_covers_only_its_descendants() {
        assert!(covers("FS", "FS.Read"));
        assert!(covers("FS.Read", "fs.Read"));
        assert!(!covers("FS.Read", "FS"));
        assert!(covers("FS.Read:/data", "FS.Read:/data/x"));
        assert!(!covers("FS.Read:/data", "FS.Read:/database"));
    }

    #[test]
    fn plugin_imports_are_decided_by_the_one_holds_relation() {
        let fact = plugin_authority_import_fact();
        assert!(fact.decision(&Authority::from_rights(["fs"])).is_allowed());
        assert!(fact.decision(&Authority::from_rights(["FS.Read:/plugins"])).is_allowed());
        assert_eq!(Authority::from_rights(["Net"]).decide_import(&fact), Verdict::Missing);
        assert_eq!(Authority::from_rights(["FS.Read"]).rights().collect::<Vec<_>>(), ["FS.Read"]);
    }
}
