/// D-AUTHORITY-NAME1=A: the one named rights value that crosses boundaries.
/// Authority remains ordinary data; every engine calls
/// the helpers below instead of keeping a second policy implementation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetAuthority {
    rights: std::collections::BTreeSet<String>,
}

impl JetAuthority {
    /// D-AUTHORITY-NAME1=A: the workspace value starts with the safe workspace
    /// read right. Boundary operations may narrow it, but never widen it.
    pub fn workspace() -> Self {
        Self {
            rights: jet_authority_workspace_rights(),
        }
    }
}

fn jet_authority_covers(held: &str, requested: &str) -> bool {
    held == requested
        || requested
            .strip_prefix(held)
            .is_some_and(|tail| tail.starts_with('.'))
}

/// D-AGENT-EXEC1: the default workspace process scope is read-only.
pub(crate) fn jet_authority_workspace_rights() -> std::collections::BTreeSet<String> {
    ["FS.Read".to_string()].into_iter().collect()
}

/// D-AUTHORITY-NAME1=A: `with` can only attenuate a right already held.
pub(crate) fn jet_authority_with_right(
    held: &std::collections::BTreeSet<String>,
    requested: &str,
) -> Result<std::collections::BTreeSet<String>, String> {
    if held.iter().any(|bound| jet_authority_covers(bound, requested)) {
        Ok([requested.to_string()].into_iter().collect())
    } else {
        Err(format!(
            "E0712: authority cannot narrow to `{requested}` outside its held rights"
        ))
    }
}

/// D-AUTHORITY-NAME1=A: `without` removes a right subtree and never widens.
pub(crate) fn jet_authority_without_right(
    held: &std::collections::BTreeSet<String>,
    requested: &str,
) -> std::collections::BTreeSet<String> {
    held.iter()
        .filter(|held_right| !jet_authority_covers(requested, held_right))
        .cloned()
        .collect()
}

/// Shared AOT/Wasm operation for the `with` family member.
pub(crate) fn jet_authority_with(
    authority: &JetAuthority,
    requested: &str,
) -> Result<JetAuthority, String> {
    jet_authority_with_right(&authority.rights, requested)
        .map(|rights| JetAuthority { rights })
}

/// Shared AOT/Wasm operation for the `without` family member.
pub(crate) fn jet_authority_without(
    authority: &JetAuthority,
    requested: &str,
) -> JetAuthority {
    JetAuthority {
        rights: jet_authority_without_right(&authority.rights, requested),
    }
}
