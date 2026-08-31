//! Shared compile-time fact model (D-FACTMODEL1=A).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

pub use crate::Authority::{EFFECT_ROOTS, EFFECT_SOURCE};

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
/// Stable target inputs that distinguish one emitted Prelude artifact from
/// another. The target triple remains the adjacent `BuildFactSnapshot` fact;
/// these fields capture the selected runtime layer, provider, and source
/// closure that the triple alone cannot identify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDossier {
    /// The selected runtime ring for the reachable Prelude closure.
    pub layer: crate::RingLayer::RuntimeLayer,
    /// An opaque, stable provider identity. Providers should include their
    /// content digest in this value when their implementation can change.
    pub provider_identity: String,
    /// An opaque, stable identity for the canonical Prelude source closure.
    pub closure_identity: String,
}

impl Default for TargetDossier {
    fn default() -> Self {
        Self {
            layer: crate::RingLayer::RuntimeLayer::Std,
            provider_identity: "hosted-default".to_string(),
            closure_identity: "prelude-hosted-v1".to_string(),
        }
    }
}

impl TargetDossier {
    pub fn new(
        layer: crate::RingLayer::RuntimeLayer,
        provider_identity: impl Into<String>,
        closure_identity: impl Into<String>,
    ) -> Self {
        Self {
            layer,
            provider_identity: provider_identity.into(),
            closure_identity: closure_identity.into(),
        }
    }

    /// Canonical bytes for the target dossier portion of an artifact key.
    ///
    /// Length framing is intentional: it keeps field boundaries unambiguous,
    /// so identities such as `("ab", "c")` cannot collide with `("a", "bc")`.
    pub fn cache_bytes(&self, target_triple: &str) -> Vec<u8> {
        let mut bytes = b"jet-target-dossier-v1\0".to_vec();
        for value in [
            target_triple,
            self.layer.as_str(),
            self.provider_identity.as_str(),
            self.closure_identity.as_str(),
        ] {
            append_cache_frame(&mut bytes, value.as_bytes());
        }
        bytes
    }
}

fn append_cache_frame(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value);
}

/// The one typed snapshot consumed by every front-end fact reader. Engines do
/// not discover any of these values; they only receive the folded literals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildFactSnapshot {
    pub package_name: String,
    pub package_version: String,
    pub os: crate::OSTarget::OSTarget,
    /// Target identity used by target-aware compiler facts. This is an
    /// internal build input, not a user-declared fact row.
    pub target_triple: String,
    /// Stable identity of the selected runtime layer, provider, and Prelude
    /// source closure. This is folded into artifact/cache keys.
    pub target_dossier: TargetDossier,
    pub profile: String,
    pub stamp: BuildStamp,
    /// Resolved contribution chains used by `jet explain`; fact readers still
    /// consume the folded fields above.
    pub contributions: BTreeMap<String, crate::Policy::EffectiveFact>,
    /// D-CONF-MODULE1=A: effective declared settings for the selected build
    /// profile. The map is a folded input, not an engine-visible lookup.
    pub settings: BTreeMap<String, BuildSettingFact>,
    /// The contribution chain for each effective setting, kept beside the
    /// value so semantic tools can explain a specialized argument.
    pub setting_provenance: BTreeMap<String, Vec<String>>,
}

/// One declared setting after the contribution ladder has been resolved.
/// The declaration type stays beside the value so later readers do not need
/// to rediscover manifest syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSettingFact {
    pub ty: String,
    pub value: BuildFactValue,
}

impl Default for BuildFactSnapshot {
    fn default() -> Self {
        Self {
            package_name: "script".to_string(),
            package_version: "0.0.0".to_string(),
            os: crate::OSTarget::OSTarget::host(),
            target_triple: crate::Layout::TargetLayout::host_triple(),
            target_dossier: TargetDossier::default(),
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
            target_triple: crate::Layout::TargetLayout::host_triple(),
            target_dossier: TargetDossier::default(),
            profile: profile.to_string(),
            stamp: BuildStamp::default(),
            contributions: BTreeMap::new(),
            settings: BTreeMap::new(),
            setting_provenance: BTreeMap::new(),
        }
    }

    /// Set the target dossier while retaining the rest of the folded facts.
    pub fn with_target_dossier(mut self, target_dossier: TargetDossier) -> Self {
        self.target_dossier = target_dossier;
        self
    }

    /// Canonical target identity bytes for artifact and runtime cache keys.
    pub fn artifact_identity_bytes(&self) -> Vec<u8> {
        self.target_dossier.cache_bytes(&self.target_triple)
    }

    pub fn contribution(&self, name: &str) -> Option<&crate::Policy::EffectiveFact> {
        self.contributions.get(name)
    }

    /// Resolve one declared setting from the already-seeded build snapshot.
    pub fn setting_value(&self, name: &str) -> Option<BuildFactValue> {
        self.settings.get(name).map(|setting| setting.value.clone())
    }

    pub fn setting(&self, key: &str) -> Option<&BuildSettingFact> {
        self.settings.get(key)
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

/// Checked typestate facts. The graph is compile-time metadata only; engines
/// never receive a state discriminator or transition policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateGraph {
    pub states: Vec<StateNode>,
    pub transitions: Vec<StateTransition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateNode {
    pub name: String,
    pub terminal: bool,
    /// `None` means that the declaration has no entry transition, so
    /// reachability from an initial state is not defined.
    pub reachable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateTransition {
    pub operation: String,
    pub from: Option<String>,
    pub to: String,
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
    /// State labels retain declaration order for typed reflection. The set in
    /// `FactDeclaration` remains the membership/validation view.
    state_member_order: BTreeMap<String, Vec<String>>,
    state_graphs: BTreeMap<String, StateGraph>,
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
        let members = members.into_iter().collect::<Vec<_>>();
        if kind == FactKind::State {
            self.state_member_order
                .insert(name.clone(), members.clone());
        }
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

    /// Register the erased state plane owned by one nominal type.
    pub fn declare_state(
        &mut self,
        type_name: impl Into<String>,
        members: impl IntoIterator<Item = String>,
    ) {
        self.declare(
            FactKind::State,
            format!("{}.State", type_name.into()),
            members,
        );
    }

    /// Build the state portion of the erased registry from nested struct
    /// sections. Sema remains responsible for marker validation and graphs;
    /// this constructor gives early comptime folds the same fact row.
    pub fn from_state_items(items: &[crate::AST::Item]) -> Self {
        let mut registry = Self::default();
        for item in items {
            let crate::AST::Item::Struct(structure) = item else {
                continue;
            };
            let Some(state) = &structure.state else {
                continue;
            };
            registry.declare_state(
                structure.name.clone(),
                state.states.iter().map(|(name, _)| name.clone()),
            );
        }
        registry
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
                name: name.clone(),
                members: BTreeSet::new(),
                deny: BTreeSet::new(),
                from: BTreeSet::new(),
            })
            .members
            .insert(member.clone());
        if kind == FactKind::State {
            let ordered = self.state_member_order.entry(name).or_default();
            if !ordered.contains(&member) {
                ordered.push(member);
            }
        }
    }

    /// Attach the checked graph to its erased state fact row.
    pub fn set_state_graph(&mut self, name: impl Into<String>, graph: StateGraph) {
        self.state_graphs.insert(name.into(), graph);
    }

    pub fn state_graph(&self, name: &str) -> Option<&StateGraph> {
        self.state_graphs.get(name)
    }

    /// Return state labels in the order written in the owning struct section.
    pub fn state_members(&self, type_name: &str) -> Option<&[String]> {
        self.state_member_order
            .get(&format!("{type_name}.State"))
            .map(Vec::as_slice)
    }

    pub fn contains(&self, kind: FactKind, name: &str) -> bool {
        self.declarations.contains_key(&(kind, name.to_string()))
    }

    pub fn iter(&self) -> impl Iterator<Item = &FactDeclaration> {
        self.declarations.values()
    }

    pub fn iter_kind(&self, kind: FactKind) -> impl Iterator<Item = &FactDeclaration> {
        self.declarations
            .values()
            .filter(move |declaration| declaration.kind == kind)
    }

    pub fn member(&self, kind: FactKind, declaration: &str, member: &str) -> bool {
        self.get(kind, declaration)
            .is_some_and(|fact| fact.members.contains(member))
    }
}

/// One subsumption rule for every hierarchical fact family.
pub fn fact_covers(bound: &str, fact: &str) -> bool {
    crate::Authority::covers(bound, fact)
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
                    if alias_path
                        .first()
                        .is_some_and(|node| node.as_str() == target)
                    {
                        alias_path[0] = alias.to_string();
                    }
                    self.proofs
                        .insert((row_name.clone(), alias.to_string(), fact), alias_path);
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
                    for predecessor in predecessors.get(&caller).into_iter().flatten().cloned() {
                        if queued.insert(predecessor.clone()) {
                            queue.push_back(predecessor);
                        }
                    }
                }
            }
        }
    }

    ReachabilityResult {
        rows: values,
        proofs,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        fact_covers, project_reachability, BuildFactSnapshot, FactKind, FactRegistry,
        ReachabilityRow, TargetDossier,
    };
    use crate::RingLayer::RuntimeLayer;
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

        assert_eq!(
            facts.get(FactKind::Effect, "Secret").unwrap().kind,
            FactKind::Effect
        );
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
                    BTreeMap::from([("leaf".to_string(), BTreeSet::from(["panic".to_string()]))]),
                ),
                ReachabilityRow::new(
                    "calls-exec",
                    BTreeMap::from([("leaf".to_string(), BTreeSet::from(["Exec".to_string()]))]),
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

    #[test]
    fn target_dossier_identity_is_framed_and_profile_sensitive() {
        let hosted = BuildFactSnapshot::default();
        let freestanding = hosted.clone().with_target_dossier(TargetDossier::new(
            RuntimeLayer::Core,
            "board.uart",
            "prelude-core-v1",
        ));

        assert_ne!(
            hosted.artifact_identity_bytes(),
            freestanding.artifact_identity_bytes()
        );

        let left = TargetDossier::new(RuntimeLayer::Core, "ab", "c");
        let right = TargetDossier::new(RuntimeLayer::Core, "a", "bc");
        assert_ne!(
            left.cache_bytes("test-target"),
            right.cache_bytes("test-target")
        );
    }
}
