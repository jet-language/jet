//! Unified dependency-aware watch / invalidation / hot-replacement engine
//! (Tower #439 / E3-UL6).
//!
//! One typed graph feeds `jet run --watch` and `jet dev`. Nodes carry a
//! `RootKind`; reverse edges give exact closure invalidation. Receipts are
//! deterministic. `#Persist` migration and client/server replacement commit
//! as one transaction or leave the prior session valid.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use crate::file_mtime;
use jet_driver::Diagnostics::Diagnostic;
use jet_foundation::JSON::json_escape;
use jet_foundation::Game::JetGameChangeKind;

/// What kind of watch root a path is.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RootKind {
    Import,
    Asset,
    HTML,
    Style,
    Manifest,
    Lock,
    Generated,
    BuildInput,
    TargetFact,
}

impl RootKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Asset => "asset",
            Self::HTML => "html",
            Self::Style => "style",
            Self::Manifest => "manifest",
            Self::Lock => "lock",
            Self::Generated => "generated",
            Self::BuildInput => "build_input",
            Self::TargetFact => "target_fact",
        }
    }
}

/// Fingerprint of a watched path (mtime + length + existence + content digest).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathStamp {
    pub exists: bool,
    pub mtime: Option<SystemTime>,
    pub len: Option<u64>,
    pub digest: Option<String>,
}

impl PathStamp {
    pub fn capture(path: &Path) -> Self {
        match fs::metadata(path) {
            Ok(meta) => {
                let digest = meta
                    .is_file()
                    .then(|| jet_foundation::SHA256::sha256_file_hex(path).ok())
                    .flatten()
                    .map(|digest| format!("sha256-{digest}"));
                Self {
                    exists: true,
                    mtime: meta.modified().ok(),
                    len: Some(meta.len()),
                    digest,
                }
            }
            Err(_) => Self {
                exists: false,
                mtime: None,
                len: None,
                digest: None,
            },
        }
    }
}

/// How a watched path changed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
    Renamed,
    /// Event arrived after a newer generation already applied — ignore.
    Stale,
}

/// Deterministic invalidation receipt for one poll cycle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidationReceipt {
    pub generation: u64,
    pub changed: Vec<PathBuf>,
    pub closure: Vec<PathBuf>,
    pub kinds: Vec<&'static str>,
    pub change_kinds: Vec<&'static str>,
    /// Content digests captured for each `changed` path after the save settled.
    /// `None` means the path is absent or could not be hashed safely.
    pub content_digests: Vec<Option<String>>,
    pub dev_entries: Vec<DevWatchEntry>,
    pub edit_to_visible_ms: Option<u128>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameAssetWatchChange {
    pub path: PathBuf,
    pub change_kind: &'static str,
    pub digest: Option<String>,
}

/// Checked game change metadata projected from the dependency watcher.
///
/// `old_digest`/`new_digest` are filesystem observations. Schema ids remain
/// optional because only a package/build manifest can author them; the
/// watcher never treats a content digest as a schema id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevWatchEntry {
    pub path: PathBuf,
    pub change_kind: &'static str,
    pub game_kind: JetGameChangeKind,
    pub old_digest: Option<String>,
    pub new_digest: Option<String>,
    pub old_schema_id: Option<String>,
    pub new_schema_id: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchNode {
    pub path: PathBuf,
    pub kind: RootKind,
    pub stamp: PathStamp,
    /// Asset roots contain directories as watch sentinels. Keep their shape
    /// so directory churn can discover files without becoming an import.
    pub is_dir: bool,
}


impl InvalidationReceipt {
    pub fn render(&self) -> String {
        format!(
            "{{\"generation\":{},\"changed\":[{}],\"closure\":[{}],\"kinds\":[{}],\"change_kinds\":[{}],\"content_digests\":[{}],\"dev_entries\":[{}],\"edit_to_visible_ms\":{}}}",
            self.generation,
            join_paths(&self.changed),
            join_paths(&self.closure),
            join_quoted(&self.kinds),
            join_quoted(&self.change_kinds),
            join_optional_quoted(&self.content_digests),
            join_dev_entries(&self.dev_entries),
            self.edit_to_visible_ms
                .map(|ms| ms.to_string())
                .unwrap_or_else(|| "null".to_string()),
        )
    }

    /// Return only game asset paths from this generic invalidation receipt.
    /// The watcher keeps ownership of filesystem facts; the game asset
    /// pipeline consumes this projection and performs its own typed import
    /// transaction.
    pub fn game_asset_paths(&self) -> Vec<PathBuf> {
        self.changed
            .iter()
            .zip(self.kinds.iter())
            .filter(|(_, kind)| **kind == RootKind::Asset.as_str())
            .map(|(path, _)| path.clone())
            .collect()
    }
    pub fn game_asset_changes(&self) -> Vec<GameAssetWatchChange> {
        self.changed
            .iter()
            .zip(self.change_kinds.iter())
            .zip(self.content_digests.iter())
            .zip(self.kinds.iter())
            .filter(|(((_, _), _), kind)| **kind == RootKind::Asset.as_str())
            .map(|(((path, change), digest), _)| GameAssetWatchChange {
                path: path.clone(),
                change_kind: *change,
                digest: digest.clone(),
            })
            .collect()
    }
    pub fn game_dev_entries(&self) -> &[DevWatchEntry] {
        &self.dev_entries
    }
}

fn join_optional_quoted(items: &[Option<String>]) -> String {
    items
        .iter()
        .map(|item| {
            item.as_deref()
                .map(|value| format!("\"{}\"", json_escape(value)))
                .unwrap_or_else(|| "null".to_string())
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn join_quoted(items: &[&'static str]) -> String {
    items
        .iter()
        .map(|item| format!("\"{}\"", json_escape(item)))
        .collect::<Vec<_>>()
        .join(",")
}

fn join_dev_entries(entries: &[DevWatchEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            let optional = |value: Option<&String>| {
                value
                    .map(|value| format!("\"{}\"", json_escape(value)))
                    .unwrap_or_else(|| "null".to_string())
            };
            format!(
                "{{\"path\":\"{}\",\"change_kind\":\"{}\",\"game_kind\":\"{}\",\"old_digest\":{},\"new_digest\":{},\"old_schema_id\":{},\"new_schema_id\":{}}}",
                json_escape(&entry.path.display().to_string()),
                entry.change_kind,
                entry.game_kind.as_str(),
                optional(entry.old_digest.as_ref()),
                optional(entry.new_digest.as_ref()),
                optional(entry.old_schema_id.as_ref()),
                optional(entry.new_schema_id.as_ref()),
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}


fn join_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| format!("\"{}\"", json_escape(&p.display().to_string())))
        .collect::<Vec<_>>()
        .join(",")
}

/// Typed dependency graph with deterministic reverse edges.
#[derive(Clone, Debug, Default)]
pub struct WatchGraph {
    nodes: BTreeMap<PathBuf, WatchNode>,
    /// Declared game asset roots. This is discovery scope, never importer
    /// authority; files outside these roots are not promoted to Asset nodes.
    asset_roots: BTreeSet<PathBuf>,
    /// Directory roots supplied by runtime file/testing APIs. Their sentinels
    /// stay watched even when absent so a later directory create is visible.
    runtime_input_roots: BTreeSet<PathBuf>,
    /// Explicit game path kinds supplied by the package/build owner. The
    /// watcher never infers script/world meaning from a filename.
    game_paths: BTreeMap<PathBuf, JetGameChangeKind>,
    game_schema_ids: BTreeMap<PathBuf, (Option<String>, Option<String>)>,
    /// `dep -> dependents` (who must rebuild when `dep` changes).
    reverse: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    /// Forward edges kept for rebuild/debug.
    forward: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    entry: Option<PathBuf>,
}

impl WatchGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entry(&self) -> Option<&Path> {
        self.entry.as_deref()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &WatchNode> {
        self.nodes.values()
    }

    pub fn reverse_edges(&self) -> &BTreeMap<PathBuf, BTreeSet<PathBuf>> {
        &self.reverse
    }

    pub fn upsert(&mut self, path: PathBuf, kind: RootKind) {
        self.upsert_with_dir_hint(path, kind, false);
    }

    fn upsert_with_dir_hint(&mut self, path: PathBuf, kind: RootKind, dir_hint: bool) {
        let is_dir = dir_hint || path.is_dir();
        let stamp = PathStamp::capture(&path);
        self.nodes.insert(
            path.clone(),
            WatchNode {
                path,
                kind,
                stamp,
                is_dir,
            },
        );
    }

    pub fn link(&mut self, from: PathBuf, to: PathBuf) {
        self.forward
            .entry(from.clone())
            .or_default()
            .insert(to.clone());
        self.reverse.entry(to).or_default().insert(from);
    }

    pub fn set_entry(&mut self, entry: PathBuf) {
        self.entry = Some(entry.clone());
        self.upsert(entry, RootKind::Import);
    }
    /// Register a declared game asset root and its current descendants.
    /// Every discovered path stays in the Asset domain; no importer or
    /// dependency inference is performed here.
    pub fn register_asset_root(&mut self, path: PathBuf) {
        let path = canonicalize_loose(&path);
        self.asset_roots.insert(path.clone());
        let mut visited = BTreeSet::new();
        self.add_asset_tree(&path, &mut visited);
    }

    pub fn asset_roots(&self) -> &BTreeSet<PathBuf> {
        &self.asset_roots
    }

    /// Register one exact game path with an owner-provided semantic kind.
    /// Registration is the only authority for script/world meaning.
    pub fn register_game_path(&mut self, path: PathBuf, kind: JetGameChangeKind) {
        let path = canonicalize_loose(&path);
        self.game_paths.insert(path.clone(), kind);
        if !self.nodes.contains_key(&path) {
            self.upsert(path, RootKind::BuildInput);
        }
    }

    /// Register exact old/new schema ids when a package/build manifest has
    /// checked them. Filesystem content digests remain separate observations.
    pub fn register_game_path_with_schema_ids(
        &mut self,
        path: PathBuf,
        kind: JetGameChangeKind,
        old_schema_id: Option<String>,
        new_schema_id: Option<String>,
    ) {
        let path = canonicalize_loose(&path);
        self.register_game_path(path.clone(), kind);
        self.game_schema_ids
            .insert(path, (old_schema_id, new_schema_id));
    }

    fn game_kind_for(&self, path: &Path) -> Option<JetGameChangeKind> {
        self.game_paths.get(path).copied().or_else(|| {
            self.nodes.get(path).and_then(|node| match node.kind {
                // Import/asset classification is already a checked WatchGraph
                // fact; do not infer game meaning from the path spelling.
                RootKind::Import => Some(JetGameChangeKind::Script),
                RootKind::Asset => Some(JetGameChangeKind::Asset),
                _ => None,
            })
        })
    }

    fn game_schema_ids_for(&self, path: &Path) -> (Option<String>, Option<String>) {
        self.game_schema_ids
            .get(path)
            .cloned()
            .unwrap_or((None, None))
    }

    /// Discover files created below runtime file/testing directory inputs
    /// without resetting stamps for existing nodes. New nodes start absent so
    /// the next poll reports a typed `created` transition.
    fn discover_runtime_input_trees(&mut self) {
        let roots = self.runtime_input_roots.iter().cloned().collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        for root in roots {
            self.add_new_runtime_tree(&root, &mut visited);
        }
    }

    fn add_new_runtime_tree(&mut self, path: &Path, visited: &mut BTreeSet<PathBuf>) {
        let path = canonicalize_loose(path);
        if !visited.insert(path.clone()) {
            return;
        }
        let exists = PathStamp::capture(&path).exists;
        let expected_dir = !exists
            && self
                .nodes
                .get(&path)
                .is_some_and(|node| node.is_dir && !node.stamp.exists);
        let is_dir = path.is_dir() || expected_dir;
        if let Some(node) = self.nodes.get_mut(&path) {
            if exists {
                node.is_dir = is_dir;
                if is_dir {
                    node.kind = RootKind::Asset;
                }
            }
        } else {
            self.nodes.insert(
                path.clone(),
                WatchNode {
                    path: path.clone(),
                    kind: if is_dir {
                        RootKind::Asset
                    } else {
                        Self::classify(&path)
                    },
                    stamp: PathStamp {
                        exists: false,
                        mtime: None,
                        len: None,
                        digest: None,
                    },
                    is_dir,
                },
            );
        }
        if let Some(entry) = &self.entry {
            self.link(entry.clone(), path.clone());
        }
        if !is_dir {
            return;
        }
        let mut children = fs::read_dir(&path)
            .into_iter()
            .flatten()
            .filter_map(|item| item.ok().map(|item| item.path()))
            .collect::<Vec<_>>();
        children.sort();
        for child in children {
            self.add_new_runtime_tree(&child, visited);
        }
    }

    /// Discover files created below declared asset roots without resetting
    /// stamps for existing nodes.
    fn discover_asset_tree(&mut self) {
        let roots = self.asset_roots.iter().cloned().collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        for root in roots {
            self.add_new_asset_tree(&root, &mut visited);
        }
    }

    fn add_new_asset_tree(&mut self, path: &Path, visited: &mut BTreeSet<PathBuf>) {
        let path = canonicalize_loose(path);
        if !visited.insert(path.clone()) {
            return;
        }
        let is_dir = path.is_dir();
        let exists = PathStamp::capture(&path).exists;
        if let Some(node) = self.nodes.get_mut(&path) {
            if exists {
                node.is_dir = is_dir;
            }
        } else {
            self.nodes.insert(
                path.clone(),
                WatchNode {
                    path: path.clone(),
                    kind: RootKind::Asset,
                    stamp: PathStamp {
                        exists: false,
                        mtime: None,
                        len: None,
                        digest: None,
                    },
                    is_dir,
                },
            );
        }
        if !is_dir {
            return;
        }
        let mut children = fs::read_dir(&path)
            .into_iter()
            .flatten()
            .filter_map(|item| item.ok().map(|item| item.path()))
            .collect::<Vec<_>>();
        children.sort();
        for child in children {
            self.add_new_asset_tree(&child, visited);
        }
    }

    fn add_asset_tree(&mut self, path: &Path, visited: &mut BTreeSet<PathBuf>) {
        let path = canonicalize_loose(path);
        if !visited.insert(path.clone()) {
            return;
        }
        self.upsert(path.clone(), RootKind::Asset);
        if !path.is_dir() {
            return;
        }
        let mut children = fs::read_dir(&path)
            .into_iter()
            .flatten()
            .filter_map(|item| item.ok().map(|item| item.path()))
            .collect::<Vec<_>>();
        children.sort();
        for child in children {
            self.add_asset_tree(&child, visited);
        }
    }

    /// Exact reverse-dependent closure of `seeds`, sorted.
    pub fn closure_of(&self, seeds: &[PathBuf]) -> Vec<PathBuf> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        for seed in seeds {
            if seen.insert(seed.clone()) {
                queue.push_back(seed.clone());
            }
        }
        while let Some(path) = queue.pop_front() {
            if let Some(deps) = self.reverse.get(&path) {
                for dep in deps {
                    if seen.insert(dep.clone()) {
                        queue.push_back(dep.clone());
                    }
                }
            }
            // A changed path always invalidates itself.
            seen.insert(path);
        }
        seen.into_iter().collect()
    }

    pub fn watched_paths(&self) -> Vec<PathBuf> {
        self.nodes.keys().cloned().collect()
    }

    pub fn refresh_stamps(&mut self) {
        for node in self.nodes.values_mut() {
            node.stamp = PathStamp::capture(&node.path);
        }
    }

    /// Classify a path into a root kind from extension / name.
    pub fn classify(path: &Path) -> RootKind {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name == jet_driver::Syntax::PACKAGE_FILE
            || name == jet_driver::Syntax::PAYLOAD_FILE
            || name.ends_with(".manifest")
            || name == "web.manifest.json"
        {
            return RootKind::Manifest;
        }
        if name == "lock" || name.ends_with(".lock") || path.ends_with(".jet/lock") {
            return RootKind::Lock;
        }
        if let Some(parent) = path.parent() {
            if parent
                .components()
                .any(|c| c.as_os_str() == "generated" || c.as_os_str() == ".jet-gen")
            {
                return RootKind::Generated;
            }
        }
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        match extension.as_deref() {
            Some("jet") => RootKind::Import,
            Some("html" | "htm") => RootKind::HTML,
            Some("css") => RootKind::Style,
            Some(
                "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "woff" | "woff2" | "ttf" | "otf",
            ) => RootKind::Asset,
            Some("toml" | "json" | "yaml" | "yml") => RootKind::BuildInput,
            _ => {
                if name.starts_with("target.") || name.contains("target_fact") {
                    RootKind::TargetFact
                } else {
                    RootKind::BuildInput
                }
            }
        }
    }

    /// Build a graph from an entry file plus known dependency paths.
    pub fn from_entry(entry: &Path, deps: &[PathBuf]) -> Result<Self, Diagnostic> {
        let mut graph = Self::new();
        let entry = canonicalize_loose(entry);
        graph.set_entry(entry.clone());
        let mut html_paths = Vec::new();

        let entry_dir = entry.parent().unwrap_or_else(|| Path::new("."));
        let project = jet_driver::Loader::find_manifest_root_checked(entry_dir)?
            .unwrap_or_else(|| entry_dir.to_path_buf());
        let manifest = jet_driver::Loader::manifest_path_checked(&project)?;
        let extras = [
            (manifest, RootKind::Manifest),
            (Some(project.join(".jet/lock")), RootKind::Lock),
            (
                Some(entry_dir.join(format!(
                    "{}.html",
                    entry.file_stem().and_then(|s| s.to_str()).unwrap_or("app")
                ))),
                RootKind::HTML,
            ),
            (
                Some(project.join(format!(
                    "{}.css",
                    entry.file_stem().and_then(|s| s.to_str()).unwrap_or("app")
                ))),
                RootKind::Style,
            ),
            (Some(project.join("target.fact")), RootKind::TargetFact),
        ];
        for (path, kind) in extras {
            let Some(path) = path else {
                continue;
            };
            if path.exists()
                || kind == RootKind::Manifest
                || kind == RootKind::Lock
                || kind == RootKind::HTML
            {
                graph.upsert(path.clone(), kind);
                graph.link(entry.clone(), path.clone());
                if kind == RootKind::HTML {
                    html_paths.push(path);
                }
            }
        }

        for dep in deps {
            let path = canonicalize_loose(dep);
            if path == entry {
                continue;
            }
            let kind = Self::classify(&path);
            graph.upsert(path.clone(), kind);
            graph.link(entry.clone(), path.clone());
            if kind == RootKind::HTML {
                html_paths.push(path);
            }
        }
        graph.scan_html_references(&html_paths);
        graph.scan_runtime_inputs();
        Ok(graph)
    }

    /// Track literal paths passed to `core.files` and `core.testing`.
    /// The runtime still owns all file semantics; this watcher only discovers
    /// paths whose changes can invalidate the source that consumed them.
    pub fn scan_runtime_inputs(&mut self) {
        let sources: Vec<PathBuf> = self
            .nodes
            .values()
            .filter(|node| {
                node.kind == RootKind::Import
                    || node
                        .path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("jet"))
            })
            .map(|node| node.path.clone())
            .collect();
        let base = self
            .entry
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        let mut inputs = BTreeMap::<PathBuf, bool>::new();
        for source in sources {
            let Ok(text) = fs::read_to_string(source) else {
                continue;
            };
            for input in runtime_file_inputs(&text) {
                let path = Path::new(&input.path);
                let path = if path.is_absolute() {
                    canonicalize_loose(path)
                } else {
                    canonicalize_loose(&base.join(path))
                };
                inputs
                    .entry(path)
                    .and_modify(|is_dir| *is_dir |= input.is_dir)
                    .or_insert(input.is_dir);
            }
        }
        let mut visited = BTreeSet::new();
        for (input, is_dir) in inputs {
            self.add_runtime_input_tree(&input, &mut visited, is_dir);
        }
    }

    fn add_runtime_input_tree(
        &mut self,
        path: &Path,
        visited: &mut BTreeSet<PathBuf>,
        dir_hint: bool,
    ) {
        self.add_runtime_input_node(path, visited, dir_hint, true);
    }

    fn add_runtime_input_node(
        &mut self,
        path: &Path,
        visited: &mut BTreeSet<PathBuf>,
        dir_hint: bool,
        register_root: bool,
    ) {
        let path = canonicalize_loose(path);
        if !visited.insert(path.clone()) {
            return;
        }
        let exists = PathStamp::capture(&path).exists;
        let is_dir = path.is_dir() || (dir_hint && !exists);
        let kind = if is_dir {
            RootKind::Asset
        } else {
            Self::classify(&path)
        };
        self.upsert_with_dir_hint(path.clone(), kind, dir_hint && !exists);
        if register_root && dir_hint {
            self.runtime_input_roots.insert(path.clone());
        }
        if let Some(entry) = &self.entry {
            self.link(entry.clone(), path.clone());
        }
        if !is_dir {
            return;
        }
        let mut children: Vec<PathBuf> = fs::read_dir(&path)
            .into_iter()
            .flatten()
            .filter_map(|item| item.ok().map(|item| item.path()))
            .collect();
        children.sort();
        for child in children {
            self.add_runtime_input_node(&child, visited, false, false);
        }
    }


    fn scan_html_references(&mut self, roots: &[PathBuf]) {
        let mut visited = BTreeSet::new();
        for root in roots {
            self.add_reference_tree(root, &mut visited);
        }
    }

    fn scan_existing_html_references(&mut self) {
        let roots: Vec<PathBuf> = self
            .nodes
            .values()
            .filter(|node| node.kind == RootKind::HTML)
            .map(|node| node.path.clone())
            .collect();
        self.scan_html_references(&roots);
    }

    fn add_reference_tree(&mut self, path: &Path, visited: &mut BTreeSet<PathBuf>) {
        let path = canonicalize_loose(path);
        if !visited.insert(path.clone()) {
            return;
        }
        let kind = Self::classify(&path);
        self.upsert(path.clone(), kind);
        if let Some(entry) = &self.entry {
            self.link(entry.clone(), path.clone());
        }
        let Ok(source) = fs::read_to_string(&path) else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        for reference in referenced_paths(&path, &source) {
            let Some(child) = local_reference_path(parent, &reference) else {
                continue;
            };
            self.add_reference_tree(&child, visited);
        }
    }

    /// Rebuild from disk using the compiler loader's dependency list.
    pub fn discover(entry: &Path) -> Result<Self, Diagnostic> {
        let entry_str = entry.to_string_lossy();
        let deps = match jet_driver::Loader::load_entry_with_overlays_and_dependencies(
            entry_str.as_ref(),
            &[],
            false,
        ) {
            (Ok(bundle), deps) => {
                let mut paths = deps;
                for module in &bundle.modules {
                    paths.push(module.path.clone());
                }
                if let Some(module) = bundle.modules.get(bundle.entry) {
                    if let (Some(parent), Some(html)) = (module.path.parent(), &module.html_path) {
                        if let Some(path) = local_reference_path(parent, html) {
                            paths.push(path);
                        }
                    }
                }
                for input in &bundle.comptime_inputs {
                    paths.push(bundle.project_root.join(&input.path));
                }
                paths.sort();
                paths.dedup();
                paths
            }
            (Err(_), deps) => deps,
        };
        Self::from_entry(entry, &deps)
    }
}

fn canonicalize_loose(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        }
    })
}

#[derive(Clone, Debug)]
struct RuntimeFileInput {
    path: String,
    is_dir: bool,
}

const RUNTIME_FILE_INPUT_METHODS: [(&str, bool); 14] = [
    ("copy", false),
    ("copy_dir", true),
    ("exists", false),
    ("is_dir", true),
    ("list_dir", true),
    ("read", false),
    ("read_bytes", false),
    ("walk", true),
    ("walk_parallel", true),
    ("glob", false),
    ("stat", false),
    ("fixture", false),
    ("golden", false),
    ("corpus", true),
];

fn runtime_file_inputs(source: &str) -> Vec<RuntimeFileInput> {
    let mut inputs = BTreeMap::<String, bool>::new();
    for &(method, is_dir) in &RUNTIME_FILE_INPUT_METHODS {
        let marker = format!(".{method}");
        let mut offset = 0;
        while let Some(relative) = source[offset..].find(&marker) {
            let start = offset + relative;
            let mut cursor = start + marker.len();
            if source
                .as_bytes()
                .get(cursor)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                offset = cursor;
                continue;
            }
            while source
                .as_bytes()
                .get(cursor)
                .is_some_and(|byte| byte.is_ascii_whitespace())
            {
                cursor += 1;
            }
            if source.as_bytes().get(cursor) != Some(&b'(') {
                offset = cursor;
                continue;
            }
            cursor += 1;
            while source
                .as_bytes()
                .get(cursor)
                .is_some_and(|byte| byte.is_ascii_whitespace())
            {
                cursor += 1;
            }
            if let Some((value, end)) = quoted_literal(source, cursor) {
                if !value.is_empty()
                    && !value.contains('{')
                    && !value.contains('}')
                    && !value.contains("://")
                {
                    inputs
                        .entry(value.to_string())
                        .and_modify(|seen_dir| *seen_dir |= is_dir)
                        .or_insert(is_dir);
                }
                offset = end.saturating_add(1);
            } else {
                offset = cursor.saturating_add(1);
            }
        }
    }
    inputs
        .into_iter()
        .map(|(path, is_dir)| RuntimeFileInput { path, is_dir })
        .collect()
}

fn quoted_literal(source: &str, start: usize) -> Option<(&str, usize)> {
    let bytes = source.as_bytes();
    let quote = *bytes.get(start)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let value_start = start + 1;
    let mut cursor = value_start;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor = cursor.saturating_add(2),
            byte if byte == quote => return Some((&source[value_start..cursor], cursor)),
            _ => cursor += 1,
        }
    }
    None
}

fn local_reference_path(base: &Path, raw: &str) -> Option<PathBuf> {
    let reference = raw
        .trim()
        .split(|ch| ch == '?' || ch == '#')
        .next()?
        .trim();
    if reference.is_empty()
        || reference.starts_with("//")
        || reference.starts_with("data:")
        || reference.starts_with("blob:")
        || reference.starts_with("http:")
        || reference.starts_with("https:")
        || reference.starts_with("mailto:")
        || reference.starts_with("javascript:")
        || reference.contains('\\')
    {
        return None;
    }
    let reference = reference.trim_start_matches('/');
    let path = Path::new(reference);
    if path.is_absolute() {
        return None;
    }
    Some(canonicalize_loose(&base.join(path)))
}

fn referenced_paths(path: &Path, source: &str) -> Vec<String> {
    let mut references = Vec::new();
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if matches!(extension.as_deref(), Some("html" | "htm")) {
        for attribute in ["src", "href", "poster", "srcset"] {
            extract_values_after(source, attribute, Some(b'='), false, attribute == "srcset", &mut references);
        }
    }
    if matches!(extension.as_deref(), Some("html" | "htm" | "css")) {
        extract_values_after(source, "url", Some(b'('), false, false, &mut references);
    }
    if matches!(extension.as_deref(), Some("js" | "mjs")) {
        extract_values_after(source, "from", None, false, false, &mut references);
        extract_values_after(source, "import", None, true, false, &mut references);
    }
    references
}

fn is_attribute_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn extract_values_after(
    source: &str,
    keyword: &str,
    delimiter: Option<u8>,
    optional_parentheses: bool,
    split_srcset: bool,
    out: &mut Vec<String>,
) {
    let lower = source.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut offset = 0;
    while let Some(relative) = lower[offset..].find(keyword) {
        let start = offset + relative;
        let before = start.checked_sub(1).and_then(|index| bytes.get(index));
        let after = bytes.get(start + keyword.len());
        if before.is_some_and(|byte| is_attribute_name_byte(*byte))
            || after.is_some_and(|byte| is_attribute_name_byte(*byte))
        {
            offset = start + keyword.len();
            continue;
        }

        let mut cursor = start + keyword.len();
        while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_whitespace()) {
            cursor += 1;
        }
        match delimiter {
            Some(b'=') if bytes.get(cursor) == Some(&b'=') => {
                cursor += 1;
                while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_whitespace()) {
                    cursor += 1;
                }
            }
            Some(b'=') => {
                offset = start + keyword.len();
                continue;
            }
            Some(b'(') if bytes.get(cursor) == Some(&b'(') => {
                cursor += 1;
                while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_whitespace()) {
                    cursor += 1;
                }
            }
            Some(b'(') => {
                offset = start + keyword.len();
                continue;
            }
            None if optional_parentheses && bytes.get(cursor) == Some(&b'(') => {
                cursor += 1;
                while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_whitespace()) {
                    cursor += 1;
                }
            }
            None => {}
            _ => unreachable!(),
        }

        let quote = bytes
            .get(cursor)
            .copied()
            .filter(|byte| *byte == b'\'' || *byte == b'"');
        if quote.is_some() {
            cursor += 1;
        }
        if quote.is_none() && delimiter.is_none() {
            offset = start + keyword.len();
            continue;
        }

        let value_start = cursor;
        let value_end = if let Some(quote) = quote {
            source[cursor..]
                .find(char::from(quote))
                .map(|index| cursor + index)
                .unwrap_or(source.len())
        } else {
            source[cursor..]
                .find(|ch: char| {
                    ch.is_ascii_whitespace()
                        || ch == '>'
                        || (delimiter == Some(b'(') && ch == ')')
                })
                .map(|index| cursor + index)
                .unwrap_or(source.len())
        };

        if value_start < value_end {
            let value = &source[value_start..value_end];
            if split_srcset {
                for candidate in value.split(',') {
                    if let Some(path) = candidate.split_whitespace().next() {
                        out.push(path.to_string());
                    }
                }
            } else {
                out.push(value.to_string());
            }
        }
        offset = value_end.saturating_add(1);
    }
}

/// Live poll session over a `WatchGraph`.
pub struct WatchSession {
    graph: WatchGraph,
    generation: u64,
    applied_generation: u64,
    coalesce: Duration,
    edit_started: Option<Instant>,
}

impl WatchSession {
    pub fn open(entry: &Path) -> Result<Self, Diagnostic> {
        let mut graph = WatchGraph::discover(entry)?;
        graph.refresh_stamps();
        Ok(Self {
            graph,
            generation: 0,
            applied_generation: 0,
            coalesce: Duration::from_millis(WATCH_COALESCE_MS),
            edit_started: None,
        })
    }

    pub fn from_graph(graph: WatchGraph) -> Self {
        Self {
            graph,
            generation: 0,
            applied_generation: 0,
            coalesce: Duration::from_millis(WATCH_COALESCE_MS),
            edit_started: None,
        }
    }
    pub fn register_asset_root(&mut self, path: PathBuf) {
        self.graph.register_asset_root(path);
    }

    pub fn register_game_path(&mut self, path: PathBuf, kind: JetGameChangeKind) {
        self.graph.register_game_path(path, kind);
    }

    pub fn register_game_path_with_schema_ids(
        &mut self,
        path: PathBuf,
        kind: JetGameChangeKind,
        old_schema_id: Option<String>,
        new_schema_id: Option<String>,
    ) {
        self.graph
            .register_game_path_with_schema_ids(path, kind, old_schema_id, new_schema_id);
    }

    pub fn graph(&self) -> &WatchGraph {
        &self.graph
    }

    pub fn graph_mut(&mut self) -> &mut WatchGraph {
        &mut self.graph
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn mark_edit_started(&mut self) {
        self.edit_started = Some(Instant::now());
    }

    /// Poll once. `None` = quiet. Handles rename/delete/create/modify and
    /// drops stale events whose generation was already superseded.
    pub fn poll(&mut self) -> Option<InvalidationReceipt> {
        // A directory mtime only tells us that its contents changed. Discover
        // new descendants first, preserving their absent baseline so creates
        // become observable on this poll.
        self.graph.discover_asset_tree();
        self.graph.discover_runtime_input_trees();

        let mut changed = Vec::new();
        let mut change_kinds = Vec::new();
        for node in self.graph.nodes.values() {
            let now = PathStamp::capture(&node.path);
            if now == node.stamp {
                continue;
            }
            let change_kind = match (node.stamp.exists, now.exists) {
                (false, true) => ChangeKind::Created,
                (true, false) => ChangeKind::Deleted,
                (true, true) => {
                    // Same parent dir + missing old path elsewhere looks like
                    // rename; report Modified for stamp drift and let rename
                    // detection below promote matching pairs.
                    ChangeKind::Modified
                }
                (false, false) => continue,
            };
            changed.push(node.path.clone());
            change_kinds.push(change_kind);
        }

        // A Deleted + Created pair in the same directory with the same
        // RootKind collapses to Renamed on the deleted path and still
        // invalidates both.
        detect_renames(&mut changed, &mut change_kinds, &self.graph);
        if changed.is_empty() {
            return None;
        }

        if self.edit_started.is_none() {
            self.edit_started = Some(Instant::now());
        }
        // Coalesce one editor save burst without making every quiet poll pay a
        // fixed delay. Callers use WATCH_POLL_INTERVAL_MS while idle; this is
        // the only bounded wait added after a change is observed.
        std::thread::sleep(self.coalesce);

        // Re-sample after coalescing (atomic save / editor write settle).
        let mut settled = Vec::new();
        let mut settled_kinds = Vec::new();
        let mut settled_root_kinds = Vec::new();
        let mut settled_old_digests = Vec::new();
        let mut settled_digests = Vec::new();
        for (path, change_kind) in changed.into_iter().zip(change_kinds.into_iter()) {
            let (root_kind, is_dir) = self
                .graph
                .nodes
                .get(&path)
                .map(|node| (node.kind, node.is_dir))
                .unwrap_or_else(|| (WatchGraph::classify(&path), path.is_dir()));
            let now = PathStamp::capture(&path);
            let previous = self
                .graph
                .nodes
                .get(&path)
                .map(|node| node.stamp.clone())
                .unwrap_or(PathStamp {
                    exists: false,
                    mtime: None,
                    len: None,
                    digest: None,
                });
            if now == previous && change_kind != ChangeKind::Renamed {
                continue;
            }
            if root_kind == RootKind::Asset && is_dir {
                let runtime_root = self.graph.runtime_input_roots.contains(&path);
                let report_transition = runtime_root
                    && matches!(change_kind, ChangeKind::Created | ChangeKind::Deleted);
                if !report_transition {
                    // Directory nodes are discovery sentinels, not importable
                    // resources. Updating their stamp avoids repeating the same
                    // directory churn while child files carry the real changes.
                    if let Some(node) = self.graph.nodes.get_mut(&path) {
                        node.stamp = now;
                    }
                    continue;
                }
            }
            settled.push(path);
            settled_kinds.push(change_kind);
            settled_root_kinds.push(root_kind);
            settled_old_digests.push(previous.digest.clone());
            settled_digests.push(now.digest);
        }
        if settled.is_empty() {
            return None;
        }

        self.generation += 1;
        let generation = self.generation;
        if generation <= self.applied_generation {
            let stale_count = settled.len();
            let stale_digests = vec![None; stale_count];
            let dev_entries = make_dev_entries(
                &self.graph,
                &settled,
                &settled_kinds,
                &settled_root_kinds,
                &stale_digests,
                &stale_digests,
            );
            return Some(InvalidationReceipt {
                generation,
                changed: settled,
                closure: Vec::new(),
                kinds: settled_root_kinds.iter().map(|kind| kind.as_str()).collect(),
                change_kinds: vec!["stale"; stale_count],
                content_digests: stale_digests,
                dev_entries,
                edit_to_visible_ms: None,
            });
        }

        for path in &settled {
            if let Some(node) = self.graph.nodes.get_mut(path) {
                node.stamp = PathStamp::capture(path);
            } else {
                let kind = WatchGraph::classify(path);
                self.graph.upsert(path.clone(), kind);
            }
        }

        let closure = self.graph.closure_of(&settled);
        let edit_to_visible_ms = self
            .edit_started
            .map(|started| started.elapsed().as_millis());
        self.edit_started = None;
        let dev_entries = make_dev_entries(
            &self.graph,
            &settled,
            &settled_kinds,
            &settled_root_kinds,
            &settled_old_digests,
            &settled_digests,
        );

        Some(InvalidationReceipt {
            generation,
            changed: settled,
            closure,
            kinds: settled_root_kinds.iter().map(|kind| kind.as_str()).collect(),
            change_kinds: settled_kinds
                .iter()
                .map(|kind| match kind {
                    ChangeKind::Created => "created",
                    ChangeKind::Modified => "modified",
                    ChangeKind::Deleted => "deleted",
                    ChangeKind::Renamed => "renamed",
                    ChangeKind::Stale => "stale",
                })
                .collect(),
            content_digests: settled_digests,
            dev_entries,
            edit_to_visible_ms,
        })
    }

    /// Poll the shared watcher once and route only cycles containing a
    /// `RootKind::Asset` change to the game asset producer. The returned
    /// receipt remains the canonical invalidation transaction input.
    pub fn poll_game_assets(&mut self) -> Option<InvalidationReceipt> {
        let receipt = self.poll()?;
        if receipt.game_asset_paths().is_empty() {
            None
        } else {
            Some(receipt)
        }
    }

    /// Mark a receipt applied. Later polls with older generations are stale.
    /// Stamps refresh in place; newly discovered imports merge in without
    /// dropping previously tracked roots (assets, manual links, etc.).
    pub fn acknowledge(&mut self, receipt: &InvalidationReceipt) -> Result<(), Diagnostic> {
        if receipt.generation > self.applied_generation {
            self.applied_generation = receipt.generation;
        }
        if let Some(entry) = self.graph.entry().map(|p| p.to_path_buf()) {
            let discovered = WatchGraph::discover(&entry)?;
            for node in discovered.nodes() {
                if !self.graph.nodes.contains_key(&node.path) {
                    self.graph.upsert(node.path.clone(), node.kind);
                }
            }
            for (to, froms) in discovered.reverse_edges() {
                for from in froms {
                    self.graph.link(from.clone(), to.clone());
                }
            }
            self.graph
                .runtime_input_roots
                .extend(discovered.runtime_input_roots.iter().cloned());
        }
        self.graph.refresh_stamps();
        self.graph.scan_existing_html_references();
        self.graph.scan_runtime_inputs();
        Ok(())
    }

    /// Crash/reconnect recovery: rebuild stamps from disk without invalidating.
    pub fn recover(&mut self) {
        self.graph.refresh_stamps();
        self.edit_started = None;
    }
}

fn detect_renames(
    changed: &mut Vec<PathBuf>,
    change_kinds: &mut Vec<ChangeKind>,
    graph: &WatchGraph,
) {
    let deleted: Vec<(usize, PathBuf)> = changed
        .iter()
        .enumerate()
        .filter(|(i, _)| change_kinds[*i] == ChangeKind::Deleted)
        .map(|(i, p)| (i, p.clone()))
        .collect();
    let created: Vec<(usize, PathBuf)> = changed
        .iter()
        .enumerate()
        .filter(|(i, _)| change_kinds[*i] == ChangeKind::Created)
        .map(|(i, p)| (i, p.clone()))
        .collect();
    for (_di, dpath) in &deleted {
        let dparent = dpath.parent();
        let dkind = graph.nodes.get(dpath).map(|n| n.kind);
        for (_ci, cpath) in &created {
            if cpath.parent() == dparent
                && graph.nodes.get(cpath).map(|n| n.kind) == dkind
                && dpath != cpath
            {
                if let Some(idx) = changed.iter().position(|p| p == dpath) {
                    change_kinds[idx] = ChangeKind::Renamed;
                }
            }
        }
    }
}

fn make_dev_entries(
    graph: &WatchGraph,
    paths: &[PathBuf],
    change_kinds: &[ChangeKind],
    root_kinds: &[RootKind],
    old_digests: &[Option<String>],
    new_digests: &[Option<String>],
) -> Vec<DevWatchEntry> {
    paths
        .iter()
        .enumerate()
        .filter_map(|(index, path)| {
            let game_kind = graph.game_kind_for(path).or_else(|| {
                (root_kinds.get(index) == Some(&RootKind::Asset))
                    .then_some(JetGameChangeKind::Asset)
            })?;
            let (old_schema_id, new_schema_id) = graph.game_schema_ids_for(path);
            Some(DevWatchEntry {
                path: path.clone(),
                change_kind: change_kinds
                    .get(index)
                    .copied()
                    .map(change_kind_name)
                    .unwrap_or("modified"),
                game_kind,
                old_digest: old_digests.get(index).cloned().flatten(),
                new_digest: new_digests.get(index).cloned().flatten(),
                old_schema_id,
                new_schema_id,
            })
        })
        .collect()
}

fn change_kind_name(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Created => "created",
        ChangeKind::Modified => "modified",
        ChangeKind::Deleted => "deleted",
        ChangeKind::Renamed => "renamed",
        ChangeKind::Stale => "stale",
    }
}

// ── `#Persist` typed migration (D-PERSIST1) ─────────────────────────────
// Store + migration live in `jet_foundation::Persist` (shared runtime-heap
// boundary for tier-0 and tier-1). Re-exported here for WatchService callers.

pub use jet_foundation::Persist::{
    PersistEntry, PersistIdentity, PersistOutcome, PersistRejectReason, PersistStore,
};

// ── Client/server hot-replacement transaction ───────────────────────────

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSnapshot {
    pub generation: u64,
    pub artifact_token: String,
    /// Stable source identity.  This is not a display label.
    pub source_id: String,
    /// Compiler/build identity for this candidate.
    pub build_id: String,
    /// Source revision identity (normally a content digest).
    pub revision: String,
    /// Stable world identity.  A world change is never an implicit swap.
    pub world_id: String,
    pub persist: PersistStore,
}

impl SessionSnapshot {
    pub fn new(generation: u64, artifact_token: impl Into<String>, persist: PersistStore) -> Self {
        Self {
            generation,
            artifact_token: artifact_token.into(),
            source_id: String::new(),
            build_id: String::new(),
            revision: String::new(),
            world_id: String::new(),
            persist,
        }
    }

    pub fn with_lineage(
        mut self,
        source_id: impl Into<String>,
        build_id: impl Into<String>,
        revision: impl Into<String>,
        world_id: impl Into<String>,
    ) -> Self {
        self.source_id = source_id.into();
        self.build_id = build_id.into();
        self.revision = revision.into();
        self.world_id = world_id.into();
        self
    }

    pub fn with_persist(mut self, persist: PersistStore) -> Self {
        self.persist = persist;
        self
    }

    pub fn lineage_valid(&self) -> bool {
        [self.source_id.as_str(), self.build_id.as_str(), self.revision.as_str(), self.world_id.as_str()]
            .iter()
            .all(|value| !value.is_empty() && !value.chars().any(char::is_control))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HotReplacePolicy {
    #[default]
    Swap,
    Restart,
    Pin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotReplacePhase {
    Begun,
    Paused,
    Preflighted,
    Staged,
    Committed,
    RolledBack,
}

#[derive(Clone, Debug)]
pub struct HotReplaceTxn {
    prior: SessionSnapshot,
    shared_prior: PersistStore,
    candidate: Option<SessionSnapshot>,
    decisions: Vec<PersistOutcome>,
    policy: HotReplacePolicy,
    phase: HotReplacePhase,
    failure: Option<String>,
}

impl HotReplaceTxn {
    pub fn begin(prior: SessionSnapshot) -> Self {
        Self::begin_with_policy(prior, HotReplacePolicy::Swap)
    }

    pub fn begin_with_policy(prior: SessionSnapshot, policy: HotReplacePolicy) -> Self {
        Self {
            shared_prior: jet_foundation::Persist::shared_clone(),
            prior,
            candidate: None,
            decisions: Vec::new(),
            policy,
            phase: HotReplacePhase::Begun,
            failure: None,
        }
    }

    pub fn set_policy(&mut self, policy: HotReplacePolicy) -> Result<(), String> {
        if self.phase != HotReplacePhase::Begun {
            return Err("hot replacement policy must be selected before pause".to_string());
        }
        self.policy = policy;
        Ok(())
    }

    pub fn policy(&self) -> HotReplacePolicy {
        self.policy
    }

    /// Pause the old world before any candidate state is inspected or staged.
    pub fn pause(&mut self) -> Result<(), String> {
        if self.phase != HotReplacePhase::Begun {
            return Err("hot replacement pause is out of order".to_string());
        }
        self.phase = HotReplacePhase::Paused;
        Ok(())
    }

    /// Validate lineage and compute every value disposition without mutating
    /// the shared store.  A layout change is either explicitly migratable or
    /// rejected so the caller can restart; it is never silently reset.
    pub fn preflight(&mut self, candidate: &SessionSnapshot) -> Result<(), String> {
        if self.phase != HotReplacePhase::Paused {
            return Err("hot replacement preflight requires a paused world".to_string());
        }
        if self.policy == HotReplacePolicy::Restart {
            return self.reject("explicit restart requested; swap was not attempted");
        }
        if !candidate.lineage_valid() {
            return self.reject("candidate lineage is incomplete; live identity is unavailable");
        }
        if !self.prior.lineage_valid() {
            return self.reject("prior lineage is incomplete; live identity is unavailable");
        }
        if candidate.source_id != self.prior.source_id {
            return self.reject("source identity changed; restart required");
        }
        if candidate.world_id != self.prior.world_id {
            return self.reject("world identity changed; restart required");
        }

        self.decisions.clear();
        let mut rejected = None;
        for entry in candidate.persist.entries() {
            let outcome = self.prior.persist.plan_migrate(
                &entry.module,
                &entry.name,
                &entry.shape,
                &entry.payload,
            );
            if let PersistOutcome::Rejected { reason, prior } = &outcome {
                rejected = Some(format!(
                    "`{}`: {}",
                    prior.key(),
                    reason.message()
                ));
            }
            self.decisions.push(outcome);
        }
        for old in self.prior.persist.entries() {
            if candidate.persist.get(&old.module, &old.name).is_none() {
                self.decisions.push(PersistOutcome::Reset {
                    reason: format!(
                        "binding `{}` disappeared; prior value reset because its name is no longer present",
                        old.stable_key()
                    ),
                    entry: old.clone(),
                });
            }
        }
        if let Some(reason) = rejected {
            return self.reject(format!("hot replacement preflight rejected: {reason}"));
        }
        self.candidate = Some(candidate.clone());
        self.phase = HotReplacePhase::Preflighted;
        Ok(())
    }

    pub fn stage(&mut self) -> Result<(), String> {
        if self.phase != HotReplacePhase::Preflighted || self.candidate.is_none() {
            return Err("hot replacement stage requires successful preflight".to_string());
        }
        self.phase = HotReplacePhase::Staged;
        Ok(())
    }

    /// Atomically publish the staged candidate.  Any incomplete or rejected
    /// transaction restores the exact shared store captured at `begin`.
    pub fn commit(mut self) -> Result<SessionSnapshot, (SessionSnapshot, String)> {
        if self.phase != HotReplacePhase::Staged {
            let reason = self
                .failure
                .take()
                .unwrap_or_else(|| "hot replacement was not staged; prior session kept".to_string());
            jet_foundation::Persist::shared_replace(self.shared_prior);
            return Err((self.prior, reason));
        }
        let Some(candidate) = self.candidate.take() else {
            jet_foundation::Persist::shared_replace(self.shared_prior);
            return Err((self.prior, "hot replacement candidate disappeared".to_string()));
        };
        let generation = self.prior.generation + 1;
        let next = SessionSnapshot {
            generation,
            artifact_token: format!("gen-{generation}"),
            ..candidate
        };
        jet_foundation::Persist::shared_replace(next.persist.clone());
        self.phase = HotReplacePhase::Committed;
        Ok(next)
    }

    /// Restore the pre-transaction heap and leave the prior snapshot usable.
    pub fn rollback(&mut self, reason: impl Into<String>) -> SessionSnapshot {
        self.failure = Some(reason.into());
        jet_foundation::Persist::shared_replace(self.shared_prior.clone());
        self.phase = HotReplacePhase::RolledBack;
        self.prior.clone()
    }

    fn reject(&mut self, reason: impl Into<String>) -> Result<(), String> {
        let reason = reason.into();
        self.failure = Some(reason.clone());
        Err(reason)
    }

    pub fn phase(&self) -> HotReplacePhase {
        self.phase
    }

    pub fn decisions(&self) -> &[PersistOutcome] {
        &self.decisions
    }

    pub fn candidate(&self) -> Option<&SessionSnapshot> {
        self.candidate.as_ref()
    }

    pub fn prior(&self) -> &SessionSnapshot {
        &self.prior
    }
}

/// Idle watch cadence for the std-only watcher. A sleeping poll avoids a busy
/// loop while keeping the save-to-reload path below the live-reload budget.
pub const WATCH_POLL_INTERVAL_MS: u64 = 5;

/// Bounded save coalescing window. Keep this below the old fixed 30 ms delay;
/// the outer loops provide the idle cadence above.
pub const WATCH_COALESCE_MS: u64 = 4;

/// Shared edit-to-visible budget used by browser and native matrices (ms).
pub const EDIT_TO_VISIBLE_BUDGET_MS: u128 = 2000;

/// Did this receipt meet the edit-to-visible budget?
pub fn within_budget(receipt: &InvalidationReceipt) -> bool {
    match receipt.edit_to_visible_ms {
        Some(ms) => ms <= EDIT_TO_VISIBLE_BUDGET_MS,
        None => true,
    }
}

/// Convenience: one-shot mtime check used by thin callers that only need to
/// know whether *any* watched path drifted (without building a receipt).
pub fn any_stamp_changed(graph: &WatchGraph) -> bool {
    graph.nodes.values().any(|node| {
        let now = PathStamp::capture(&node.path);
        now != node.stamp
    })
}

/// Re-export for callers that previously only had `file_mtime`.
pub fn path_mtime(path: &Path) -> Option<SystemTime> {
    file_mtime(path)
}

/// The session-owned source boundary. Canvas and project transactions use one
/// gate so a client cannot publish a stale result after a newer one.
#[derive(Default)]
pub struct SessionBroker {
    source_transactions: Mutex<()>,
}

impl SessionBroker {
    pub(crate) fn lock_source_transaction(&self) -> MutexGuard<'_, ()> {
        self.source_transactions.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jet_watch_{name}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn typed_graph_records_kinds_reverse_edges_and_receipts() {
        let dir = tmp_dir("graph");
        let entry = dir.join("app.jet");
        let lib = dir.join("lib.jet");
        let css = dir.join("app.css");
        let html = dir.join("app.html");
        let manifest = dir.join("package.jet");
        let lock = dir.join(".jet");
        fs::create_dir_all(&lock).unwrap();
        let lock_file = lock.join("lock");
        for (path, body) in [
            (&entry, "use lib\nfn run() {}\n"),
            (&lib, "fn helper() {}\n"),
            (&css, "body{}\n"),
            (&html, "<html></html>\n"),
            (&manifest, "name: \"app\"\n"),
            (&lock_file, "lock = 1\n"),
        ] {
            fs::write(path, body).unwrap();
        }
        let mut graph = WatchGraph::from_entry(&entry, &[lib.clone()]).unwrap();
        graph.upsert(css.clone(), RootKind::Style);
        graph.link(entry.clone(), css.clone());
        graph.upsert(html.clone(), RootKind::HTML);
        graph.link(entry.clone(), html.clone());
        graph.upsert(dir.join("logo.png"), RootKind::Asset);
        graph.upsert(dir.join("generated/out.jet"), RootKind::Generated);
        graph.upsert(dir.join("input.json"), RootKind::BuildInput);
        graph.upsert(dir.join("target.fact"), RootKind::TargetFact);

        assert!(graph.node_count() >= 8);
        let kinds: BTreeSet<_> = graph.nodes().map(|n| n.kind).collect();
        assert!(kinds.contains(&RootKind::Import));
        assert!(kinds.contains(&RootKind::Manifest));
        assert!(kinds.contains(&RootKind::Lock));
        assert!(kinds.contains(&RootKind::Style));
        assert!(kinds.contains(&RootKind::HTML));
        assert!(kinds.contains(&RootKind::Asset));
        assert!(kinds.contains(&RootKind::Generated));
        assert!(kinds.contains(&RootKind::BuildInput));
        assert!(kinds.contains(&RootKind::TargetFact));

        let closure = graph.closure_of(&[lib.clone()]);
        assert!(closure.contains(&lib));
        assert!(closure.contains(&canonicalize_loose(&entry)));

        let mut session = WatchSession::from_graph(graph);
        std::thread::sleep(Duration::from_millis(20));
        let mut f = fs::OpenOptions::new().append(true).open(&lib).unwrap();
        writeln!(f, "// touch").unwrap();
        drop(f);
        session.mark_edit_started();
        let receipt = session.poll().expect("lib change");
        assert!(!receipt.closure.is_empty());
        assert!(receipt.render().contains("\"generation\":"));
        assert!(within_budget(&receipt));
        session.acknowledge(&receipt).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn html_shell_tracks_local_assets_and_future_creates() {
        let dir = tmp_dir("html_assets");
        let entry = dir.join("app.jet");
        let shell = dir.join("shell.html");
        let stylesheet = dir.join("theme.css");
        let image = dir.join("logo.png");
        fs::write(&entry, "fn run() {}\n").unwrap();
        fs::write(
            &shell,
            "<html><head><link href=\"theme.css\"></head><body><img src=\"logo.png\"></body></html>",
        )
        .unwrap();
        fs::write(&stylesheet, "body { color: white; }\n").unwrap();

        let graph = WatchGraph::from_entry(&entry, &[shell.clone()]).unwrap();
        let shell = canonicalize_loose(&shell);
        let stylesheet = canonicalize_loose(&stylesheet);
        let image = canonicalize_loose(&image);
        let entry = canonicalize_loose(&entry);
        assert!(graph.nodes.contains_key(&shell));
        assert!(graph.nodes.contains_key(&stylesheet));
        assert!(graph.nodes.contains_key(&image));
        assert!(!graph.nodes.get(&image).unwrap().stamp.exists);
        assert!(graph.reverse_edges().get(&stylesheet).unwrap().contains(&entry));
        assert!(graph.reverse_edges().get(&image).unwrap().contains(&entry));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn runtime_file_inputs_track_generator_directories_and_files() {
        let dir = tmp_dir("runtime_inputs");
        let entry = dir.join("generate.jet");
        let assets = dir.join("assets");
        let image = assets.join("logo.png");
        fs::create_dir_all(&assets).unwrap();
        fs::write(&image, "asset").unwrap();
        fs::write(
            &entry,
            "use core.files as fs\nfn run() {\n    fs.copy_dir(\"assets\", \"dist/assets\")\n}\n",
        )
        .unwrap();

        let graph = WatchGraph::from_entry(&entry, &[]).unwrap();
        let entry = canonicalize_loose(&entry);
        let assets = canonicalize_loose(&assets);
        let image = canonicalize_loose(&image);
        assert!(graph.nodes.contains_key(&assets));
        assert!(graph.nodes.contains_key(&image));
        assert!(graph.reverse_edges().get(&assets).unwrap().contains(&entry));
        assert!(graph.reverse_edges().get(&image).unwrap().contains(&entry));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_delete_atomic_and_stale_events() {
        let dir = tmp_dir("events");
        let entry = dir.join("main.jet");
        let a = dir.join("a.jet");
        let b = dir.join("b.jet");
        fs::write(&entry, "fn run() {}\n").unwrap();
        fs::write(&a, "fn a() {}\n").unwrap();
        let mut graph = WatchGraph::from_entry(&entry, &[a.clone()]).unwrap();
        graph.upsert(a.clone(), RootKind::Import);
        graph.link(canonicalize_loose(&entry), a.clone());
        let mut session = WatchSession::from_graph(graph);

        // Atomic save: write temp then rename over target.
        std::thread::sleep(Duration::from_millis(20));
        let tmp = dir.join("a.jet.tmp");
        fs::write(&tmp, "fn a() { /* new */ }\n").unwrap();
        fs::rename(&tmp, &a).unwrap();
        let receipt = session.poll().expect("atomic save");
        assert!(receipt
            .change_kinds
            .iter()
            .any(|k| *k == "modified" || *k == "created"));
        session.acknowledge(&receipt).unwrap();

        // Re-attach `a` after rediscover (entry does not import it).
        session.graph_mut().upsert(a.clone(), RootKind::Import);
        session
            .graph_mut()
            .link(canonicalize_loose(&entry), a.clone());
        session.graph_mut().refresh_stamps();

        // Delete.
        std::thread::sleep(Duration::from_millis(20));
        fs::remove_file(&a).unwrap();
        let receipt = session.poll().expect("delete");
        assert!(
            receipt.change_kinds.contains(&"deleted") || receipt.changed.iter().any(|p| p == &a)
        );
        session.acknowledge(&receipt).unwrap();

        // Rename a→b (create b, delete a already gone — create b).
        fs::write(&b, "fn b() {}\n").unwrap();
        session.graph_mut().upsert(b.clone(), RootKind::Import);
        session
            .graph_mut()
            .link(canonicalize_loose(&entry), b.clone());
        session.graph_mut().refresh_stamps();
        std::thread::sleep(Duration::from_millis(20));
        let c = dir.join("c.jet");
        fs::rename(&b, &c).unwrap();
        session.graph_mut().upsert(c.clone(), RootKind::Import);
        // Force stamps: b deleted, c created.
        if let Some(node) = session.graph_mut().nodes.get_mut(&b) {
            node.stamp.exists = true; // pretend still tracked as present
        }
        let receipt = session.poll();
        assert!(receipt.is_some());
        if let Some(r) = receipt {
            session.acknowledge(&r).unwrap();
        }

        // Stale: acknowledge future gen then craft older receipt path.
        session.applied_generation = session.generation + 10;
        std::thread::sleep(Duration::from_millis(20));
        fs::write(&entry, "fn run() { /* x */ }\n").unwrap();
        if let Some(r) = session.poll() {
            assert!(r.change_kinds.contains(&"stale") || r.closure.is_empty());
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn persist_migration_and_hot_replace_transaction() {
        jet_foundation::Persist::shared_clear();
        let mut store = PersistStore::new();
        store.put(PersistEntry {
            module: "app".into(),
            name: "counter".into(),
            shape: "Int".into(),
            payload: "{\"shape\":\"Int\",\"value\":7}".into(),
        });
        match store.migrate("app", "counter", "Int", "0") {
            PersistOutcome::Kept(e) => assert!(e.payload.contains('7')),
            other => panic!("expected Kept, got {other:?}"),
        }
        match store.migrate("app", "counter", "Int+", "0") {
            PersistOutcome::Migrated(e) => assert!(e.payload.contains('7')),
            other => panic!("expected Migrated, got {other:?}"),
        }
        match store.migrate("app", "counter", "String", "\"\"") {
            PersistOutcome::Rejected { reason, prior } => {
                assert!(reason.message().contains("migration unavailable"));
                assert_eq!(prior.key(), "app::counter");
            }
            other => panic!("expected Rejected, got {other:?}"),
        }

        let prior = SessionSnapshot::new(3, "gen-3", store).with_lineage(
            "app",
            "build-3",
            "rev-3",
            "world-1",
        );
        let incomplete = HotReplaceTxn::begin(prior.clone());
        let err = incomplete.commit().expect_err("incomplete");
        assert_eq!(err.0.generation, 3);
        assert!(err.1.contains("not staged"));

        let candidate = prior
            .clone()
            .with_lineage("app", "build-4", "rev-4", "world-1");
        let mut rejected = HotReplaceTxn::begin(prior.clone());
        rejected.pause().unwrap();
        let changed = candidate.clone().with_persist({
            let mut replacement = PersistStore::new();
            replacement.put(PersistEntry {
                module: "app".into(),
                name: "counter".into(),
                shape: "String".into(),
                payload: "\"\"".into(),
            });
            replacement
        });
        assert!(rejected.preflight(&changed).is_err());
        assert_eq!(rejected.rollback("type surface changed").generation, 3);

        let mut txn = HotReplaceTxn::begin(prior);
        txn.pause().unwrap();
        txn.preflight(&candidate).unwrap();
        txn.stage().unwrap();
        let next = txn.commit().expect("commit");
        assert_eq!(next.generation, 4);
        assert_eq!(next.artifact_token, "gen-4");
        assert_eq!(next.revision, "rev-4");
    }

    #[test]
    fn crash_reconnect_recovers_stamps() {
        let dir = tmp_dir("recover");
        let entry = dir.join("app.jet");
        fs::write(&entry, "fn run() {}\n").unwrap();
        let mut session = WatchSession::open(&entry).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        fs::write(&entry, "fn run() { /* edited offline */ }\n").unwrap();
        // Simulate crash: recover without applying the pending edit as a
        // double-fire — stamps refresh so the next real edit is clean.
        session.recover();
        assert!(session.poll().is_none());
        std::thread::sleep(Duration::from_millis(20));
        fs::write(&entry, "fn run() { /* again */ }\n").unwrap();
        session.mark_edit_started();
        let receipt = session.poll().expect("post-reconnect edit");
        assert!(within_budget(&receipt));
        let _ = fs::remove_dir_all(&dir);
    }
}
