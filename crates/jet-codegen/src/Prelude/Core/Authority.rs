/// D-AUTHORITY-NAME1=A: the one named rights value that crosses boundaries.
/// The rights remain ordinary data; later boundary adapters narrow this set
/// without giving engines a second authority representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetAuthority {
    rights: std::collections::BTreeSet<String>,
}

impl JetAuthority {
    /// D-AUTHORITY-NAME1=A: the workspace authority starts with the workspace
    /// rights set. Boundary operations may narrow it, but never widen it.
    pub fn workspace() -> Self {
        Self {
            rights: std::collections::BTreeSet::new(),
        }
    }
}
