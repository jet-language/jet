// Mapped-file authority is shared by every filesystem adapter.  A writer must
// consult this registry before truncating or replacing a path while a mapped
// read owner is live.
static JET_MAPPED_PATHS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::BTreeMap<String, usize>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::BTreeMap::new()));

pub(crate) fn jet_mapped_path_key(path: &str) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| std::path::PathBuf::from(path))
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn jet_mapped_register(key: &str) {
    let mut paths = JET_MAPPED_PATHS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *paths.entry(key.to_owned()).or_insert(0) += 1;
}

pub(crate) fn jet_mapped_unregister(key: &str) {
    let mut paths = JET_MAPPED_PATHS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match paths.get_mut(key) {
        Some(count) if *count > 1 => *count -= 1,
        Some(_) => {
            paths.remove(key);
        }
        None => {}
    }
}

pub(crate) fn jet_mapped_path_is_live(path: &str) -> bool {
    let key = jet_mapped_path_key(path);
    let paths = JET_MAPPED_PATHS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    paths.get(&key).copied().unwrap_or(0) != 0
}

/// Shared writer-policy fact returned before any adapter constructs its
/// tier-specific IOError carrier.
pub(crate) struct JetMappedWriterRefusal {
    pub(crate) path: String,
    pub(crate) reason: &'static str,
}

pub(crate) fn jet_mapped_writer_refusal(path: &str) -> Option<JetMappedWriterRefusal> {
    jet_mapped_path_is_live(path).then(|| JetMappedWriterRefusal {
        path: path.to_owned(),
        reason: "file is mapped read-only",
    })
}

/// D-ABILITY-NAME2=A: the one named Authority value that crosses boundaries.
/// Authority remains ordinary data; every engine calls
/// the helpers below instead of keeping a second policy implementation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetAuthority {
    rights: std::collections::BTreeSet<String>,
}

impl JetAuthority {
    /// Build the one runtime carrier for a checked named `#FX` scope.
    /// The caller has already resolved the names; this method only stores the
    /// rights set, so every execution tier shares the same relation.
    pub fn from_rights(rights: Vec<String>) -> Self {
        Self {
            rights: jet_authority_rights_from_strings(rights),
        }
    }

    /// Generated TIR uses the prefixed constructor name to keep the runtime
    /// helper distinct from a user-defined static method. Both spellings use
    /// this one authority constructor.
    pub fn __jet_from_rights(rights: Vec<String>) -> Self {
        Self::from_rights(rights)
    }

    /// D-AUTHORITY-NAME1=A / D-AGENT-EXEC1: the workspace value names the
    /// default resource boundary explicitly. The executor treats omitted
    /// resources as denied; these entries are the only default grants.
    pub fn workspace() -> Self {
        Self {
            rights: jet_authority_workspace_rights(),
        }
    }
}

pub(crate) fn jet_authority_from_rights(rights: Vec<String>) -> JetAuthority {
    JetAuthority::from_rights(rights)
}
/// D-FILES-SCOPE1: a file scope is an owned attenuation of an Authority.
/// It keeps only explicit resource-qualified read roots; a bare `FS.Read`
/// grant never becomes an ambient filesystem handle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetFileScope {
    roots: Vec<JetFileScopeRoot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetFileScopeRoot {
    path: std::path::PathBuf,
    allow_absolute: bool,
}

impl JetFileScope {
    pub(crate) fn from_authority(authority: &JetAuthority) -> Self {
        let roots = authority
            .rights
            .iter()
            .filter_map(|right| {
                let root = right.strip_prefix("FS.Read:")?;
                if root.is_empty() {
                    return None;
                }
                let path = if root == "repo" {
                    std::path::PathBuf::from(".")
                } else {
                    let path = std::path::PathBuf::from(root);
                    if path
                        .components()
                        .any(|component| matches!(component, std::path::Component::ParentDir))
                    {
                        return None;
                    }
                    path
                };
                Some(JetFileScopeRoot {
                    allow_absolute: root != "repo" && path.is_absolute(),
                    path,
                })
            })
            .collect();
        Self { roots }
    }

    pub(crate) fn roots(&self) -> &[JetFileScopeRoot] {
        &self.roots
    }
}

impl JetFileScopeRoot {
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub(crate) fn allow_absolute(&self) -> bool {
        self.allow_absolute
    }
}

pub(crate) fn jet_authority_rights_from_strings(
    rights: Vec<String>,
) -> std::collections::BTreeSet<String> {
    rights.into_iter().collect()
}

/// Marshal the ordinary Authority carrier across the sandboxed plugin bridge.
/// The plugin runtime stores this wire value; it never turns it into host
/// imports or a second permission policy.
pub(crate) fn jet_authority_to_wire(authority: &JetAuthority) -> String {
    authority.rights.iter().cloned().collect::<Vec<_>>().join("\n")
}

fn jet_authority_covers(held: &str, requested: &str) -> bool {
    held == requested
        || requested
            .strip_prefix(held)
            .is_some_and(|tail| tail.starts_with('.') || tail.starts_with(':') || tail.starts_with('/'))
}

/// D-AGENT-EXEC1: the default workspace process scope is a repository read
/// plus a private build write. Network, home, secrets, devices, inherited
/// handles, and every other resource stay absent and therefore denied.
pub(crate) fn jet_authority_workspace_rights() -> std::collections::BTreeSet<String> {
    [
        "FS.Read:repo".to_string(),
        "FS.Write:.jet/build".to_string(),
    ]
    .into_iter()
    .collect()
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
) -> JetAuthority {
    match jet_authority_with_right(&authority.rights, requested) {
        Ok(rights) => JetAuthority { rights },
        Err(message) => panic!("{message}"),
    }
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
