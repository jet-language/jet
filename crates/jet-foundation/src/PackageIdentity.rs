//! #2517 stage S1: package identity, the package graph, and the
//! fingerprints that key persisted package checks.
//!
//! The package is the unit of incremental checking (owner ruling of
//! 2026-09-30). A package is a directory with `package.jet` (its member files
//! share one namespace, D-MOD-CYCLE1=A), the embedded Core library, or a
//! single file outside any package. Identities are stable semantic labels,
//! never checkout paths or loader aliases, so two checkouts of one tree
//! produce equal keys and digests.
//!
//! Keys use one field framing that JetFoundation mirrors byte for byte: each
//! field is a UTF-8 string written as a LEB128 byte length followed by its
//! bytes, and a key is the lowercase SHA-256 hex of the concatenation.
//! Digests enter as 64-character lowercase hex and counts as decimal text.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use crate::AST::{Func, ImportDecl, Item, LoadedModule, ProgramBundle};
use crate::RecordCodec::{digest_hex, record_digest, record_map, RecordSection, RecordValue};
use crate::SHA256::sha256_hex;

/// Package identity of the embedded Core library.
pub const CORE_PACKAGE: &str = "core";

pub const SOURCE_DIGEST_SCHEMA: &str = "jet.pkg-source/v1";
pub const TARGET_FACTS_SCHEMA: &str = "jet.target-facts/v1";
pub const CHECK_KEY_SCHEMA: &str = "jet.pkg-check-key/v1";
pub const INTERFACE_SCHEMA: &str = "jet.iface/v1";

/// Interface record sections that enter the interface digest.
pub const IFACE_SECTION_HEADER: u64 = 1;
pub const IFACE_SECTION_DECLARATIONS: u64 = 2;
pub const IFACE_SECTION_REEXPORTS: u64 = 3;
pub const IFACE_SECTION_SUMMARIES: u64 = 4;

/// The shared key framing.
pub struct KeyHasher {
    bytes: Vec<u8>,
}

impl KeyHasher {
    pub fn new(schema: &str) -> Self {
        let mut hasher = Self { bytes: Vec::new() };
        hasher.field(schema);
        hasher
    }

    pub fn field(&mut self, value: &str) -> &mut Self {
        let mut len = value.len() as u64;
        loop {
            let byte = (len & 0x7f) as u8;
            len >>= 7;
            if len == 0 {
                self.bytes.push(byte);
                break;
            }
            self.bytes.push(byte | 0x80);
        }
        self.bytes.extend_from_slice(value.as_bytes());
        self
    }

    pub fn count(&mut self, count: usize) -> &mut Self {
        self.field(&count.to_string())
    }

    pub fn finish(&self) -> String {
        sha256_hex(&self.bytes)
    }
}

/// Source digest of one package: its member files as `(package-relative
/// path, bytes)` in any order, and its manifest bytes when it has one.
pub fn source_digest(files: &[(String, &[u8])], manifest: Option<&[u8]>) -> String {
    let mut files = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), sha256_hex(bytes)))
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let mut hasher = KeyHasher::new(SOURCE_DIGEST_SCHEMA);
    hasher.field("files").count(files.len());
    for (path, digest) in &files {
        hasher.field(path).field(digest);
    }
    hasher
        .field("manifest")
        .field(&manifest.map(sha256_hex).unwrap_or_default());
    hasher.finish()
}

/// Digest of the target facts sema reads, as `(name, value)` pairs.
pub fn target_facts_digest(pairs: &[(String, String)]) -> String {
    let mut pairs = pairs.iter().collect::<Vec<_>>();
    pairs.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let mut hasher = KeyHasher::new(TARGET_FACTS_SCHEMA);
    hasher.count(pairs.len());
    for (name, value) in pairs {
        hasher.field(name).field(value);
    }
    hasher.finish()
}

/// The package check key: the Go action ID for one package check.
/// `dependencies` holds each direct dependency's identity and the interface
/// digest it produced in this run.
pub fn package_check_key(
    compiler_identity: &str,
    target_facts_digest: &str,
    package_identity: &str,
    source_digest: &str,
    dependencies: &[(String, String)],
) -> String {
    let mut dependencies = dependencies.iter().collect::<Vec<_>>();
    dependencies.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let mut hasher = KeyHasher::new(CHECK_KEY_SCHEMA);
    hasher
        .field(compiler_identity)
        .field(target_facts_digest)
        .field(package_identity)
        .field(source_digest)
        .count(dependencies.len());
    for (identity, interface) in dependencies {
        hasher.field(identity).field(interface);
    }
    hasher.finish()
}

/// The target facts the Rust checker reads from a bundle. The build clock is
/// excluded: it changes on every unlocked run and never changes checking.
/// Contribution chains and setting provenance are excluded too: they name
/// the declaring files (checkout paths) for `jet explain`, while checking
/// reads only the folded values, which stay in.
pub fn target_fact_pairs(bundle: &ProgramBundle) -> Vec<(String, String)> {
    let mut build_facts = bundle.build_facts.clone();
    build_facts.stamp.at.clear();
    build_facts.contributions.clear();
    build_facts.setting_provenance.clear();
    vec![
        ("os".to_string(), format!("{:?}", bundle.active_os)),
        ("edition".to_string(), bundle.edition.clone()),
        (
            "web_partition".to_string(),
            bundle.web_partition_enforced.to_string(),
        ),
        (
            "layer_ceiling".to_string(),
            format!("{:?}", bundle.layer_ceiling),
        ),
        (
            "build_facts".to_string(),
            sha256_hex(&crate::CanonicalAST::canonical_fragment(&build_facts)),
        ),
    ]
}

// ------------------------------------------------------------ package graph

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PackageKind {
    /// The embedded Core library.
    Core,
    /// A directory package with `package.jet`.
    Manifest,
    /// One file outside any directory package (a loose file, a one-file
    /// package with a `package { }` header, or a synthetic module).
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageNode {
    /// Stable identity: `core`, `pkg:<dir>`, `dep:<name>/<dir>`, or
    /// `file:<path>`, relative to the project root or dependency root.
    pub identity: String,
    pub kind: PackageKind,
    /// The package directory for a manifest package.
    pub root: Option<PathBuf>,
    /// Member module indexes into `ProgramBundle::modules`, ascending.
    pub modules: Vec<usize>,
    /// Direct dependency package indexes, ascending.
    pub dependencies: Vec<usize>,
}

/// The package partition of a loaded bundle and its dependency graph.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackageGraph {
    /// Packages ordered by identity.
    pub packages: Vec<PackageNode>,
    module_package: Vec<usize>,
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn slash(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.is_empty() {
        ".".to_string()
    } else {
        text
    }
}

/// Stable label of a path: relative to the project root, else relative to the
/// dependency root that holds it, else the normalized path itself.
fn stable_label(bundle: &ProgramBundle, path: &Path) -> String {
    let path = normalize(path);
    if path.is_relative() {
        return slash(&path);
    }
    let root = normalize(&bundle.project_root);
    if let Ok(relative) = path.strip_prefix(&root) {
        return slash(relative);
    }
    let mut dependencies = bundle.dep_roots.iter().collect::<Vec<_>>();
    dependencies.sort();
    for (name, dependency_root) in dependencies {
        if let Ok(relative) = path.strip_prefix(normalize(dependency_root)) {
            return format!("dep:{name}/{}", slash(relative));
        }
    }
    slash(&path)
}

impl PackageGraph {
    /// Partition the bundle's modules into packages. Membership comes from
    /// the loader's namespace facts in the name ledger (the same rule that
    /// decides which files share a namespace); edges come from the loader's
    /// resolved file imports. Every non-Core package depends on Core when
    /// Core modules are loaded, because the prelude is implicit.
    pub fn from_bundle(bundle: &ProgramBundle) -> Self {
        Self::from_ledger(bundle, &bundle.name_ledger)
    }

    /// `from_bundle` with the name ledger passed separately, for sema, which
    /// holds the ledger outside the bundle while it checks.
    pub fn from_ledger(bundle: &ProgramBundle, ledger: &crate::Names::NameLedger) -> Self {
        let mut groups: BTreeMap<String, (PackageKind, Option<PathBuf>, Vec<usize>)> =
            BTreeMap::new();
        let mut module_identity = Vec::with_capacity(bundle.modules.len());
        for (index, module) in bundle.modules.iter().enumerate() {
            let (identity, kind, root) = if module.is_core_source() {
                (CORE_PACKAGE.to_string(), PackageKind::Core, None)
            } else if let Some(namespace) = ledger.module_namespace(index) {
                let root = PathBuf::from(namespace);
                let label = stable_label(bundle, &root);
                let identity = if label.starts_with("dep:") {
                    label
                } else {
                    format!("pkg:{label}")
                };
                (identity, PackageKind::Manifest, Some(root))
            } else {
                (
                    format!("file:{}", stable_label(bundle, &module.path)),
                    PackageKind::File,
                    None,
                )
            };
            groups
                .entry(identity.clone())
                .or_insert_with(|| (kind, root, Vec::new()))
                .2
                .push(index);
            module_identity.push(identity);
        }
        let index_of = groups
            .keys()
            .enumerate()
            .map(|(index, identity)| (identity.clone(), index))
            .collect::<BTreeMap<_, _>>();
        let module_package = module_identity
            .iter()
            .map(|identity| index_of[identity])
            .collect::<Vec<_>>();
        let core = index_of.get(CORE_PACKAGE).copied();
        let mut packages = groups
            .into_iter()
            .map(|(identity, (kind, root, modules))| PackageNode {
                identity,
                kind,
                root,
                modules,
                dependencies: Vec::new(),
            })
            .collect::<Vec<_>>();
        for (from, to) in ledger.import_edges() {
            let (Some(&from), Some(&to)) = (module_package.get(from), module_package.get(to)) else {
                continue;
            };
            if from != to {
                packages[from].dependencies.push(to);
            }
        }
        for (index, package) in packages.iter_mut().enumerate() {
            if let Some(core) = core.filter(|core| *core != index) {
                package.dependencies.push(core);
            }
            package.dependencies.sort_unstable();
            package.dependencies.dedup();
        }
        Self {
            packages,
            module_package,
        }
    }

    /// The package index that owns `module`.
    pub fn package_of(&self, module: usize) -> Option<usize> {
        self.module_package.get(module).copied()
    }

    /// Strongly connected components in dependency order: every component
    /// comes after the components it depends on. A component with more than
    /// one package is an import cycle across packages.
    pub fn components(&self) -> Vec<Vec<usize>> {
        // Tarjan's algorithm emits components in reverse topological order of
        // the "depends on" edges, which is dependencies first.
        struct State<'a> {
            graph: &'a PackageGraph,
            next: usize,
            index: Vec<Option<usize>>,
            low: Vec<usize>,
            stack: Vec<usize>,
            on_stack: Vec<bool>,
            components: Vec<Vec<usize>>,
        }
        fn visit(state: &mut State<'_>, node: usize) {
            state.index[node] = Some(state.next);
            state.low[node] = state.next;
            state.next += 1;
            state.stack.push(node);
            state.on_stack[node] = true;
            let graph = state.graph;
            for &dependency in &graph.packages[node].dependencies {
                match state.index[dependency] {
                    None => {
                        visit(state, dependency);
                        state.low[node] = state.low[node].min(state.low[dependency]);
                    }
                    Some(index) if state.on_stack[dependency] => {
                        state.low[node] = state.low[node].min(index);
                    }
                    Some(_) => {}
                }
            }
            if Some(state.low[node]) == state.index[node] {
                let mut component = Vec::new();
                while let Some(member) = state.stack.pop() {
                    state.on_stack[member] = false;
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                component.sort_unstable();
                state.components.push(component);
            }
        }
        let count = self.packages.len();
        let mut state = State {
            graph: self,
            next: 0,
            index: vec![None; count],
            low: vec![0; count],
            stack: Vec::new(),
            on_stack: vec![false; count],
            components: Vec::new(),
        };
        for node in 0..count {
            if state.index[node].is_none() {
                visit(&mut state, node);
            }
        }
        state.components
    }

    pub fn is_acyclic(&self) -> bool {
        self.components().iter().all(|component| component.len() == 1)
    }

    /// Package-relative label of a member module.
    pub fn member_label(&self, bundle: &ProgramBundle, module: usize) -> String {
        let loaded = &bundle.modules[module];
        let package = self.package_of(module).map(|index| &self.packages[index]);
        match package.and_then(|package| package.root.as_ref()) {
            Some(root) => normalize(&loaded.path)
                .strip_prefix(normalize(root))
                .map(slash)
                .unwrap_or_else(|_| stable_label(bundle, &loaded.path)),
            None if loaded.is_core_source() => slash(&loaded.path),
            None => stable_label(bundle, &loaded.path),
        }
    }

    /// Source digest of one package from the bytes the loader read.
    /// `manifest` is the package manifest's bytes, read through the audited
    /// reader by the caller.
    pub fn source_digest(&self, bundle: &ProgramBundle, package: usize, manifest: Option<&[u8]>) -> String {
        let files = self.packages[package]
            .modules
            .iter()
            .map(|&module| {
                (
                    self.member_label(bundle, module),
                    bundle.modules[module].source.as_bytes(),
                )
            })
            .collect::<Vec<_>>();
        source_digest(&files, manifest)
    }

    /// Interface digest of one package's declarations. `summaries` holds the
    /// inferred facts importers depend on, keyed by semantic identity (for
    /// example effect rows and failure sets); the checker supplies them.
    pub fn interface_digest(
        &self,
        bundle: &ProgramBundle,
        package: usize,
        summaries: &BTreeMap<String, String>,
    ) -> String {
        let node = &self.packages[package];
        let mut declarations = Vec::new();
        let mut reexports = Vec::new();
        for &module in &node.modules {
            let loaded = &bundle.modules[module];
            let label = self.member_label(bundle, module);
            for (name, fingerprint) in module_interface(loaded) {
                declarations.push((label.clone(), name, fingerprint));
            }
            for import in loaded.imports.iter().filter(|import| import.is_pub || import.is_package_pub) {
                reexports.push((label.clone(), import_fingerprint(import)));
            }
        }
        declarations.sort();
        reexports.sort();
        let sections = [
            RecordSection::new(
                IFACE_SECTION_HEADER,
                vec![record_map([("package", RecordValue::str(node.identity.as_str()))])],
            ),
            RecordSection::new(
                IFACE_SECTION_DECLARATIONS,
                declarations
                    .into_iter()
                    .map(|(module, name, fingerprint)| {
                        record_map([
                            ("module", RecordValue::Str(module)),
                            ("name", RecordValue::Str(name)),
                            ("fingerprint", RecordValue::Str(fingerprint)),
                        ])
                    })
                    .collect(),
            ),
            RecordSection::new(
                IFACE_SECTION_REEXPORTS,
                reexports
                    .into_iter()
                    .map(|(module, fingerprint)| {
                        record_map([
                            ("module", RecordValue::Str(module)),
                            ("fingerprint", RecordValue::Str(fingerprint)),
                        ])
                    })
                    .collect(),
            ),
            RecordSection::new(
                IFACE_SECTION_SUMMARIES,
                summaries
                    .iter()
                    .map(|(identity, value)| {
                        record_map([
                            ("identity", RecordValue::str(identity.as_str())),
                            ("value", RecordValue::str(value.as_str())),
                        ])
                    })
                    .collect(),
            ),
        ];
        let digest = record_digest(INTERFACE_SCHEMA, &sections)
            .expect("interface sections have distinct tags");
        digest_hex(&digest)
    }
}

// ------------------------------------------------------------ fingerprints

/// Fingerprint of a whole item, bodies included. Whitespace, comments, and
/// source positions never change it; any edit to the item's syntax does.
/// This is the input fingerprint of the item's own check.
pub fn item_source_fingerprint(item: &Item) -> String {
    sha256_hex(&crate::CanonicalAST::canonical_fragment(item))
}

/// Fingerprint of what importers see of an item, or `None` when the item is
/// not part of the package interface. Bodies are erased except the ones
/// importers consume: generic and `#Inline(Always)` functions and trait
/// default methods. Private inherent methods are dropped. A body edit that
/// changes an inferred fact reaches importers through the checker's
/// summaries, not through this fingerprint.
pub fn item_interface_fingerprint(item: &Item, pub_file: bool) -> Option<String> {
    let erased = interface_item(item, pub_file)?;
    Some(sha256_hex(&crate::CanonicalAST::canonical_fragment(&erased)))
}

fn exported(is_pub: bool, is_package_pub: bool, pub_file: bool) -> bool {
    is_pub || is_package_pub || pub_file
}

fn keeps_body(func: &Func) -> bool {
    !func.type_params.is_empty() || func.is_inline_always
}

fn erase_func(func: &mut Func) {
    if !keeps_body(func) {
        func.body.clear();
    }
}

fn erase_inherent_methods(methods: &mut Vec<Func>, pub_file: bool) {
    methods.retain(|method| exported(method.is_pub, method.is_package_pub, pub_file));
    methods.iter_mut().for_each(erase_func);
}

fn erase_trait_impls(blocks: &mut [crate::AST::TraitImplBlock]) {
    for block in blocks {
        block.methods.iter_mut().for_each(erase_func);
    }
}

fn interface_item(item: &Item, pub_file: bool) -> Option<Item> {
    let mut item = item.clone();
    let keep = match &mut item {
        Item::Func(func) => {
            erase_func(func);
            exported(func.is_pub, func.is_package_pub, pub_file)
        }
        Item::Struct(def) => {
            erase_inherent_methods(&mut def.methods, pub_file);
            erase_trait_impls(&mut def.trait_impls);
            exported(def.is_pub, def.is_package_pub, pub_file)
        }
        Item::Enum(def) => {
            erase_inherent_methods(&mut def.methods, pub_file);
            erase_trait_impls(&mut def.trait_impls);
            exported(def.is_pub, def.is_package_pub, pub_file)
        }
        Item::Impl(def) => {
            if def.trait_name.is_some() {
                def.methods.iter_mut().for_each(erase_func);
            } else {
                erase_inherent_methods(&mut def.methods, pub_file);
            }
            true
        }
        Item::CodeModule(module) => {
            if let Some(body) = &mut module.body {
                *body = body
                    .iter()
                    .filter_map(|item| interface_item(item, pub_file))
                    .collect();
            }
            exported(module.is_pub, module.is_package_pub, pub_file)
        }
        Item::Distinct(def) => exported(def.is_pub, def.is_package_pub, pub_file),
        Item::TypeAlias(def) => exported(def.is_pub, def.is_package_pub, pub_file),
        Item::UnitFamily(def) => exported(def.is_pub, def.is_package_pub, pub_file),
        Item::Trait(def) => exported(def.is_pub, def.is_package_pub, pub_file),
        Item::Tag(def) => exported(def.is_pub, def.is_package_pub, pub_file),
        Item::Const(def) => exported(def.is_pub, def.is_package_pub, pub_file),
        Item::ProtocolDecl(def) => exported(def.is_pub, def.is_package_pub, pub_file),
        Item::GenericModule(def) => exported(def.is_pub, def.is_package_pub, pub_file),
        Item::ModuleAlias(def) => exported(def.is_pub, def.is_package_pub, pub_file),
        Item::Test(_) => false,
        // Declarations without a visibility flag register package-wide facts
        // (effects, markers, facts, derives, migrations, conversions, foreign
        // bindings); they stay in the interface whole.
        Item::EffectDecl(_)
        | Item::ExternRust(_)
        | Item::Module(_)
        | Item::CModule(_)
        | Item::ErrorConv(_)
        | Item::Migration(_)
        | Item::UserDerive(_)
        | Item::TemplateLoop(_)
        | Item::MarkerDecl(_)
        | Item::FactDecl(_) => true,
    };
    keep.then_some(item)
}

fn item_name(item: &Item) -> String {
    match item {
        Item::Func(func) => format!("fn {}", func.name),
        Item::Struct(def) => format!("struct {}", def.name),
        Item::Enum(def) => format!("enum {}", def.name),
        Item::Distinct(def) => format!("distinct {}", def.name),
        Item::TypeAlias(def) => format!("alias {}", def.name),
        Item::Trait(def) => format!("trait {}", def.name),
        Item::Tag(def) => format!("tag {}", def.name),
        Item::Const(def) => format!("const {}", def.name),
        Item::ProtocolDecl(def) => format!("protocol {}", def.name),
        Item::CodeModule(def) => format!("module {}", def.name),
        Item::GenericModule(def) => format!("module {}", def.name),
        Item::ModuleAlias(def) => format!("module {}", def.name),
        Item::EffectDecl(def) => format!("effect {}", def.name),
        Item::MarkerDecl(def) => format!("marker {}", def.name),
        Item::FactDecl(def) => format!("fact {}", def.name),
        Item::Impl(def) => match &def.trait_name {
            Some(trait_name) => format!("impl {}: {trait_name}", def.type_name),
            None => format!("impl {}", def.type_name),
        },
        // Unnamed declarations are identified by their content.
        other => format!("item {}", item_source_fingerprint(other)),
    }
}

/// `(name, interface fingerprint)` for every item of one module that is part
/// of the package interface.
pub fn module_interface(module: &LoadedModule) -> Vec<(String, String)> {
    module
        .items
        .iter()
        .filter_map(|item| {
            item_interface_fingerprint(item, module.pub_file).map(|fingerprint| (item_name(item), fingerprint))
        })
        .collect()
}

fn import_fingerprint(import: &ImportDecl) -> String {
    sha256_hex(&crate::CanonicalAST::canonical_fragment(import))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_framing_is_pinned() {
        // uvarint(3) "abc" uvarint(0)
        let mut hasher = KeyHasher::new("abc");
        hasher.field("");
        assert_eq!(hasher.finish(), sha256_hex(&[3, b'a', b'b', b'c', 0]));
        assert_eq!(
            source_digest(&[("b.jet".into(), &b"2"[..]), ("a.jet".into(), &b"1"[..])], None),
            source_digest(&[("a.jet".into(), &b"1"[..]), ("b.jet".into(), &b"2"[..])], None),
        );
        assert_ne!(
            source_digest(&[("a.jet".into(), &b"1"[..])], None),
            source_digest(&[("a.jet".into(), &b"1"[..])], Some(&b""[..])),
        );
        let key = |deps: &[(String, String)]| package_check_key("c", "t", "pkg:.", "s", deps);
        let a = ("pkg:a".to_string(), "1".repeat(64));
        let b = ("pkg:b".to_string(), "2".repeat(64));
        assert_eq!(key(&[a.clone(), b.clone()]), key(&[b.clone(), a.clone()]));
        assert_ne!(key(&[a.clone()]), key(&[a, b]));
    }
}
