//! Shared compile-time fact model (D-FACTMODEL1=A).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::sync::LazyLock;

/// D-CONF-STAMP1=B: the provenance carried by the build fact plane. `at` is
/// deliberately a lock-history value; it is empty until a lock-writing path
/// supplies the first stamp and is never refreshed while a lock already has
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildStamp {
    pub git: Option<String>,
    pub dirty: bool,
    pub toolchain: String,
    pub at: String,
}

impl Default for BuildStamp {
    fn default() -> Self {
        Self {
            git: None,
            dirty: false,
            toolchain: env!("CARGO_PKG_VERSION").to_string(),
            at: String::new(),
        }
    }
}

/// The one typed snapshot consumed by every front-end fact reader. Engines do
/// not discover any of these values; they only receive the folded literals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildFactSnapshot {
    pub package_name: String,
    pub package_version: String,
    pub os: crate::OSTarget::OSTarget,
    pub profile: String,
    pub stamp: BuildStamp,
    /// Resolved contribution chains used by `jet explain`; fact readers still
    /// consume the folded fields above.
    pub contributions: BTreeMap<String, crate::Policy::EffectiveFact>,
    /// D-CONF-MODULE1=A: effective declared settings for the selected build
    /// profile. The map is a folded input, not an engine-visible lookup.
    pub settings: BTreeMap<String, BuildFactValue>,
    /// The contribution chain for each effective setting, kept beside the
    /// value so semantic tools can explain a specialized argument.
    pub setting_provenance: BTreeMap<String, Vec<String>>,
}

impl Default for BuildFactSnapshot {
    fn default() -> Self {
        Self {
            package_name: "script".to_string(),
            package_version: "0.0.0".to_string(),
            os: crate::OSTarget::OSTarget::host(),
            profile: "dev".to_string(),
            stamp: BuildStamp::default(),
            contributions: BTreeMap::new(),
            settings: BTreeMap::new(),
            setting_provenance: BTreeMap::new(),
        }
    }
}

impl BuildFactSnapshot {
    /// D-CONF-READ1=A: the manifest-less rung uses the entry filename as its
    /// package identity and `0.0.0` as its version.
    pub fn script(file: &Path, os: crate::OSTarget::OSTarget, profile: &str) -> Self {
        let package_name = file
            .file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("script")
            .to_string();
        Self {
            package_name,
            package_version: "0.0.0".to_string(),
            os,
            profile: profile.to_string(),
            stamp: BuildStamp::default(),
            contributions: BTreeMap::new(),
            settings: BTreeMap::new(),
            setting_provenance: BTreeMap::new(),
        }
    }

    pub fn contribution(&self, name: &str) -> Option<&crate::Policy::EffectiveFact> {
        self.contributions.get(name)
    }

    /// Resolve one declared setting from the already-seeded build snapshot.
    pub fn setting_value(&self, name: &str) -> Option<BuildFactValue> {
        self.settings.get(name).cloned()
    }

    /// Resolve a registered build leaf into a value shape. The registry owns
    /// the path-to-row mapping; this snapshot owns only the values.
    pub fn value(&self, read: crate::Registry::FactRead) -> Option<BuildFactValue> {
        match read {
            crate::Registry::FactRead::BuildPackageName => {
                Some(BuildFactValue::Text(self.package_name.clone()))
            }
            crate::Registry::FactRead::BuildPackageVersion => {
                Some(BuildFactValue::Text(self.package_version.clone()))
            }
            crate::Registry::FactRead::BuildOS => {
                Some(BuildFactValue::Text(self.os.name().to_string()))
            }
            crate::Registry::FactRead::BuildProfile => {
                Some(BuildFactValue::Text(self.profile.clone()))
            }
            crate::Registry::FactRead::BuildStampGit => {
                Some(BuildFactValue::OptionalText(self.stamp.git.clone()))
            }
            crate::Registry::FactRead::BuildStampDirty => {
                Some(BuildFactValue::Bool(self.stamp.dirty))
            }
            crate::Registry::FactRead::BuildStampToolchain => {
                Some(BuildFactValue::Text(self.stamp.toolchain.clone()))
            }
            crate::Registry::FactRead::BuildStampAt => {
                Some(BuildFactValue::Text(self.stamp.at.clone()))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildFactValue {
    Text(String),
    OptionalText(Option<String>),
    Bool(bool),
    Int(i64),
    Char(char),
    Enum { type_name: String, variant: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FactKind {
    Effect,
    State,
    Tag,
}

impl FactKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Effect => "Effect",
            Self::State => "State",
            Self::Tag => "Tag",
        }
    }
}

/// D-META-ONE1=A: the effect roots are written as `effect Name` declarations
/// in `Prelude/Effects.jet`, so this file keeps no copy of the vocabulary.
pub const EFFECT_SOURCE: &str = include_str!("../../jet-codegen/src/Prelude/Effects.jet");

/// Canonical closed effect roots, read from `EFFECT_SOURCE`. Effect parsing,
/// rule arguments, fact registration, diagnostics, and reflection all consume
/// this list.
pub static EFFECT_ROOTS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    EFFECT_SOURCE
        .lines()
        .filter_map(|line| line.trim().strip_prefix("effect "))
        .map(str::trim)
        .collect()
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactDeclaration {
    pub kind: FactKind,
    pub name: String,
    pub members: BTreeSet<String>,
    /// Destinations rejected by this fact. Used by tag facts; empty for other
    /// families.
    pub deny: BTreeSet<String>,
    /// Sources that introduce this fact. Used by tag facts; empty for other
    /// families.
    pub from: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FactRegistry {
    declarations: BTreeMap<(FactKind, String), FactDeclaration>,
}

impl FactRegistry {
    pub fn declare(
        &mut self,
        kind: FactKind,
        name: impl Into<String>,
        members: impl IntoIterator<Item = String>,
    ) {
        let name = name.into();
        self.declare_with_rules(kind, name, members, std::iter::empty(), std::iter::empty());
    }

    pub fn declare_with_rules(
        &mut self,
        kind: FactKind,
        name: impl Into<String>,
        members: impl IntoIterator<Item = String>,
        deny: impl IntoIterator<Item = String>,
        from: impl IntoIterator<Item = String>,
    ) {
        let name = name.into();
        self.declarations.insert(
            (kind, name.clone()),
            FactDeclaration {
                kind,
                name,
                members: members.into_iter().collect(),
                deny: deny.into_iter().collect(),
                from: from.into_iter().collect(),
            },
        );
    }

    pub fn get(&self, kind: FactKind, name: &str) -> Option<&FactDeclaration> {
        self.declarations.get(&(kind, name.to_string()))
    }

    pub fn declare_member(
        &mut self,
        kind: FactKind,
        name: impl Into<String>,
        member: impl Into<String>,
    ) {
        let name = name.into();
        let member = member.into();
        self.declarations
            .entry((kind, name.clone()))
            .or_insert_with(|| FactDeclaration {
                kind,
                name,
                members: BTreeSet::new(),
                deny: BTreeSet::new(),
                from: BTreeSet::new(),
            })
            .members
            .insert(member);
    }

    pub fn contains(&self, kind: FactKind, name: &str) -> bool {
        self.declarations.contains_key(&(kind, name.to_string()))
    }

    pub fn iter(&self) -> impl Iterator<Item = &FactDeclaration> {
        self.declarations.values()
    }

    pub fn iter_kind(&self, kind: FactKind) -> impl Iterator<Item = &FactDeclaration> {
        self.declarations.values().filter(move |declaration| declaration.kind == kind)
    }

    pub fn member(&self, kind: FactKind, declaration: &str, member: &str) -> bool {
        self.get(kind, declaration)
            .is_some_and(|fact| fact.members.contains(member))
    }
}

/// One subsumption rule for every hierarchical fact family.
pub fn fact_covers(bound: &str, fact: &str) -> bool {
    fact == bound
        || fact
            .strip_prefix(bound)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

/// Values propagated by one call-graph fact row. Each row uses the finite-set
/// lattice: facts only join by union, so cycles converge without a recursive
/// walker.
pub type ReachabilityValues = BTreeMap<String, BTreeSet<String>>;

/// One property projection over a shared call graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachabilityRow {
    pub name: String,
    pub seeds: ReachabilityValues,
}

impl ReachabilityRow {
    pub fn new(name: impl Into<String>, seeds: ReachabilityValues) -> Self {
        Self {
            name: name.into(),
            seeds,
        }
    }
}

/// All fact rows projected by one call-graph traversal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReachabilityResult {
    rows: BTreeMap<String, ReachabilityValues>,
    proofs: BTreeMap<(String, String, String), Vec<String>>,
}

impl ReachabilityResult {
    pub fn row(&self, name: &str) -> Option<&ReachabilityValues> {
        self.rows.get(name)
    }

    pub fn path(&self, row: &str, node: &str, fact: &str) -> Option<&[String]> {
        self.proofs
            .get(&(row.to_string(), node.to_string(), fact.to_string()))
            .map(Vec::as_slice)
    }

    pub fn nodes_with(&self, row: &str, fact: &str) -> BTreeSet<String> {
        self.row(row)
            .into_iter()
            .flat_map(|values| values.iter())
            .filter(|(_, facts)| facts.contains(fact))
            .map(|(node, _)| node.clone())
            .collect()
    }

    /// Copy solved facts onto a unique short-name alias after qualification.
    pub fn copy_node(&mut self, alias: &str, target: &str) {
        let row_names: Vec<String> = self.rows.keys().cloned().collect();
        for row_name in row_names {
            let Some(facts) = self
                .rows
                .get(&row_name)
                .and_then(|values| values.get(target))
                .cloned()
            else {
                continue;
            };
            if let Some(values) = self.rows.get_mut(&row_name) {
                values.insert(alias.to_string(), facts.clone());
            }
            for fact in facts {
                if let Some(path) = self
                    .proofs
                    .get(&(row_name.clone(), target.to_string(), fact.clone()))
                    .cloned()
                {
                    let mut alias_path = path;
                    if alias_path.first().is_some_and(|node| node.as_str() == target) {
                        alias_path[0] = alias.to_string();
                    }
                    self.proofs.insert(
                        (row_name.clone(), alias.to_string(), fact),
                        alias_path,
                    );
                }
            }
        }
    }
}

/// Project every reachability property from one edge set.
pub fn project_reachability(
    edges: &BTreeMap<String, BTreeSet<String>>,
    rows: impl IntoIterator<Item = ReachabilityRow>,
) -> ReachabilityResult {
    let rows: Vec<ReachabilityRow> = rows.into_iter().collect();
    let mut nodes = BTreeSet::new();
    for (caller, callees) in edges {
        nodes.insert(caller.clone());
        nodes.extend(callees.iter().cloned());
    }
    for row in &rows {
        nodes.extend(row.seeds.keys().cloned());
    }

    let mut values = BTreeMap::new();
    let mut proofs = BTreeMap::new();
    for row in &rows {
        let mut row_values = BTreeMap::new();
        for node in &nodes {
            row_values.insert(node.clone(), BTreeSet::new());
        }
        for (node, facts) in &row.seeds {
            let destination = row_values.entry(node.clone()).or_default();
            for fact in facts {
                if destination.insert(fact.clone()) {
                    proofs.insert(
                        (row.name.clone(), node.clone(), fact.clone()),
                        vec![node.clone()],
                    );
                }
            }
        }
        values.insert(row.name.clone(), row_values);
    }

    let mut predecessors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (caller, callees) in edges {
        for callee in callees {
            predecessors
                .entry(callee.clone())
                .or_default()
                .insert(caller.clone());
        }
    }

    let mut queue = VecDeque::new();
    let mut queued = BTreeSet::new();
    for row in &rows {
        for node in row.seeds.keys() {
            for predecessor in predecessors.get(node).into_iter().flatten().cloned() {
                if queued.insert(predecessor.clone()) {
                    queue.push_back(predecessor);
                }
            }
        }
    }
    while let Some(caller) = queue.pop_front() {
        queued.remove(&caller);
        let Some(callees) = edges.get(&caller) else {
            continue;
        };
        for callee in callees {
            for row in &rows {
                let facts = values
                    .get(&row.name)
                    .and_then(|row_values| row_values.get(callee))
                    .cloned()
                    .unwrap_or_default();
                for fact in facts {
                    let changed = values
                        .get_mut(&row.name)
                        .and_then(|row_values| row_values.get_mut(&caller))
                        .is_some_and(|destination| destination.insert(fact.clone()));
                    if !changed {
                        continue;
                    }
                    let mut path = proofs
                        .get(&(row.name.clone(), callee.clone(), fact.clone()))
                        .cloned()
                        .unwrap_or_else(|| vec![callee.clone()]);
                    path.insert(0, caller.clone());
                    proofs.insert((row.name.clone(), caller.clone(), fact), path);
                    for predecessor in predecessors
                        .get(&caller)
                        .into_iter()
                        .flatten()
                        .cloned()
                    {
                        if queued.insert(predecessor.clone()) {
                            queue.push_back(predecessor);
                        }
                    }
                }
            }
        }
    }

    ReachabilityResult { rows: values, proofs }
}

#[cfg(test)]
mod tests {
    use super::{fact_covers, project_reachability, FactKind, FactRegistry, ReachabilityRow};
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn subsumption_uses_segment_boundaries() {
        assert!(fact_covers("FS", "FS"));
        assert!(fact_covers("FS", "FS.Read"));
        assert!(!fact_covers("FS", "FStore"));
        assert!(!fact_covers("FS.Read", "FS"));
    }

    #[test]
    fn fact_families_have_distinct_namespaces() {
        let mut facts = FactRegistry::default();
        facts.declare(FactKind::Effect, "Secret", std::iter::empty());
        facts.declare_with_rules(
            FactKind::Tag,
            "Secret",
            std::iter::empty(),
            ["Log".to_string()],
            std::iter::empty(),
        );

        assert_eq!(facts.get(FactKind::Effect, "Secret").unwrap().kind, FactKind::Effect);
        assert_eq!(facts.get(FactKind::Tag, "Secret").unwrap().deny.len(), 1);
        assert_eq!(facts.iter().count(), 2);
    }

    #[test]
    fn one_traversal_projects_new_fact_row_with_existing_rows() {
        let edges = BTreeMap::from([
            ("root".to_string(), BTreeSet::from(["mid".to_string()])),
            ("mid".to_string(), BTreeSet::from(["leaf".to_string()])),
            ("leaf".to_string(), BTreeSet::from(["root".to_string()])),
        ]);
        let result = project_reachability(
            &edges,
            [
                ReachabilityRow::new(
                    "effects",
                    BTreeMap::from([("leaf".to_string(), BTreeSet::from(["FS".to_string()]))]),
                ),
                ReachabilityRow::new(
                    "panic",
                    BTreeMap::from([(
                        "leaf".to_string(),
                        BTreeSet::from(["panic".to_string()]),
                    )]),
                ),
                ReachabilityRow::new(
                    "calls-exec",
                    BTreeMap::from([(
                        "leaf".to_string(),
                        BTreeSet::from(["Exec".to_string()]),
                    )]),
                ),
            ],
        );

        assert!(result.nodes_with("effects", "FS").contains("root"));
        assert!(result.nodes_with("panic", "panic").contains("root"));
        assert!(result.nodes_with("calls-exec", "Exec").contains("root"));
        assert_eq!(
            result.path("effects", "root", "FS").unwrap(),
            &vec!["root".to_string(), "mid".to_string(), "leaf".to_string()]
        );
    }
}
