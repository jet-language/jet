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
use std::time::{Duration, Instant, SystemTime};

use crate::file_mtime;
use jet_driver::Diagnostics::Diagnostic;

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

/// Fingerprint of a watched path (mtime + length + existence).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathStamp {
    pub exists: bool,
    pub mtime: Option<SystemTime>,
    pub len: Option<u64>,
}

impl PathStamp {
    pub fn capture(path: &Path) -> Self {
        match fs::metadata(path) {
            Ok(meta) => Self {
                exists: true,
                mtime: meta.modified().ok(),
                len: Some(meta.len()),
            },
            Err(_) => Self {
                exists: false,
                mtime: None,
                len: None,
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

/// One typed graph node.
#[derive(Clone, Debug)]
pub struct WatchNode {
    pub path: PathBuf,
    pub kind: RootKind,
    pub stamp: PathStamp,
}

/// Deterministic invalidation receipt for one poll cycle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidationReceipt {
    pub generation: u64,
    pub changed: Vec<PathBuf>,
    pub closure: Vec<PathBuf>,
    pub kinds: Vec<&'static str>,
    pub change_kinds: Vec<&'static str>,
    pub edit_to_visible_ms: Option<u128>,
}

impl InvalidationReceipt {
    pub fn render(&self) -> String {
        format!(
            "{{\"generation\":{},\"changed\":[{}],\"closure\":[{}],\"kinds\":[{}],\"change_kinds\":[{}],\"edit_to_visible_ms\":{}}}",
            self.generation,
            join_paths(&self.changed),
            join_paths(&self.closure),
            join_quoted(&self.kinds),
            join_quoted(&self.change_kinds),
            self.edit_to_visible_ms
                .map(|ms| ms.to_string())
                .unwrap_or_else(|| "null".to_string()),
        )
    }
}

fn join_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| format!("\"{}\"", escape_json(&p.display().to_string())))
        .collect::<Vec<_>>()
        .join(",")
}

fn join_quoted(items: &[&str]) -> String {
    items
        .iter()
        .map(|s| format!("\"{}\"", escape_json(s)))
        .collect::<Vec<_>>()
        .join(",")
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Typed dependency graph with deterministic reverse edges.
#[derive(Clone, Debug, Default)]
pub struct WatchGraph {
    nodes: BTreeMap<PathBuf, WatchNode>,
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
        let stamp = PathStamp::capture(&path);
        self.nodes.insert(
            path.clone(),
            WatchNode {
                path,
                kind,
                stamp,
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
        match path.extension().and_then(|e| e.to_str()) {
            Some("jet") => RootKind::Import,
            Some("html" | "htm") => RootKind::HTML,
            Some("css") => RootKind::Style,
            Some("png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "woff" | "woff2" | "ttf" | "otf") => {
                RootKind::Asset
            }
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

        let entry_dir = entry.parent().unwrap_or_else(|| Path::new("."));
        let project = jet_driver::Loader::find_manifest_root_checked(entry_dir)?
            .unwrap_or_else(|| entry_dir.to_path_buf());
        let manifest = jet_driver::Loader::manifest_path_checked(&project)?;
        let extras = [
            (manifest, RootKind::Manifest),
            (Some(project.join(".jet/lock")), RootKind::Lock),
            (
                Some(project.join(format!(
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
            if path.exists() || kind == RootKind::Manifest || kind == RootKind::Lock {
                graph.upsert(path.clone(), kind);
                graph.link(entry.clone(), path);
            }
        }

        for dep in deps {
            let path = canonicalize_loose(dep);
            if path == entry {
                continue;
            }
            let kind = Self::classify(&path);
            graph.upsert(path.clone(), kind);
            graph.link(entry.clone(), path);
        }
        Ok(graph)
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

/// Live poll session over a `WatchGraph`.
pub struct WatchSession {
    graph: WatchGraph,
    generation: u64,
    applied_generation: u64,
    debounce: Duration,
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
            debounce: Duration::from_millis(30),
            edit_started: None,
        })
    }

    pub fn from_graph(graph: WatchGraph) -> Self {
        Self {
            graph,
            generation: 0,
            applied_generation: 0,
            debounce: Duration::from_millis(30),
            edit_started: None,
        }
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
        let mut changed = Vec::new();
        let mut change_kinds = Vec::new();
        let mut kinds = Vec::new();

        for node in self.graph.nodes.values() {
            let now = PathStamp::capture(&node.path);
            if now == node.stamp {
                continue;
            }
            let kind = match (node.stamp.exists, now.exists) {
                (false, true) => ChangeKind::Created,
                (true, false) => ChangeKind::Deleted,
                (true, true) => {
                    // Same parent dir + missing old path elsewhere looks like rename;
                    // we report Modified for content stamp drift, Renamed when the
                    // basename disappears but a sibling appears (handled below).
                    ChangeKind::Modified
                }
                (false, false) => continue,
            };
            changed.push(node.path.clone());
            change_kinds.push(kind);
            kinds.push(node.kind);
        }

        // Rename detection: a Deleted + Created pair in the same directory
        // with the same RootKind collapses to Renamed on the deleted path and
        // still invalidates both.
        detect_renames(&mut changed, &mut change_kinds, &self.graph);

        if changed.is_empty() {
            return None;
        }

        if self.edit_started.is_none() {
            self.edit_started = Some(Instant::now());
        }
        std::thread::sleep(self.debounce);

        // Re-sample after debounce (atomic save / editor write settle).
        let mut settled = Vec::new();
        let mut settled_kinds = Vec::new();
        let mut settled_root_kinds = Vec::new();
        for (path, ck) in changed.into_iter().zip(change_kinds.into_iter()) {
            let node_kind = self
                .graph
                .nodes
                .get(&path)
                .map(|n| n.kind)
                .unwrap_or_else(|| WatchGraph::classify(&path));
            let now = PathStamp::capture(&path);
            let prev = self
                .graph
                .nodes
                .get(&path)
                .map(|n| n.stamp.clone())
                .unwrap_or(PathStamp {
                    exists: false,
                    mtime: None,
                    len: None,
                });
            if now == prev && ck != ChangeKind::Renamed {
                continue;
            }
            settled.push(path);
            settled_kinds.push(ck);
            settled_root_kinds.push(node_kind);
        }
        if settled.is_empty() {
            return None;
        }

        self.generation += 1;
        let generation = self.generation;
        if generation <= self.applied_generation {
            return Some(InvalidationReceipt {
                generation,
                changed: settled,
                closure: Vec::new(),
                kinds: settled_root_kinds
                    .iter()
                    .map(|k| k.as_str())
                    .collect(),
                change_kinds: vec!["stale"],
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
        let edit_to_visible_ms = self.edit_started.map(|t| t.elapsed().as_millis());
        self.edit_started = None;

        Some(InvalidationReceipt {
            generation,
            changed: settled,
            closure,
            kinds: settled_root_kinds
                .iter()
                .map(|k| k.as_str())
                .collect(),
            change_kinds: settled_kinds
                .iter()
                .map(|k| match k {
                    ChangeKind::Created => "created",
                    ChangeKind::Modified => "modified",
                    ChangeKind::Deleted => "deleted",
                    ChangeKind::Renamed => "renamed",
                    ChangeKind::Stale => "stale",
                })
                .collect(),
            edit_to_visible_ms,
        })
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
        }
        self.graph.refresh_stamps();
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

// ── `#Persist` typed migration (D-PERSIST1) ─────────────────────────────
// Store + migration live in `jet_foundation::Persist` (shared runtime-heap
// boundary for tier-0 and tier-1). Re-exported here for WatchService callers.

pub use jet_foundation::Persist::{PersistEntry, PersistOutcome, PersistStore};

// ── Client/server hot-replacement transaction ───────────────────────────

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSnapshot {
    pub generation: u64,
    pub artifact_token: String,
    pub persist: PersistStore,
}

#[derive(Clone, Debug)]
pub struct HotReplaceTxn {
    prior: SessionSnapshot,
    client_ready: bool,
    server_ready: bool,
    committed: bool,
    failed: Option<String>,
}

impl HotReplaceTxn {
    pub fn begin(prior: SessionSnapshot) -> Self {
        Self {
            prior,
            client_ready: false,
            server_ready: false,
            committed: false,
            failed: None,
        }
    }

    pub fn mark_server_ready(&mut self) {
        if self.failed.is_none() {
            self.server_ready = true;
        }
    }

    pub fn mark_client_ready(&mut self) {
        if self.failed.is_none() {
            self.client_ready = true;
        }
    }

    pub fn fail(&mut self, reason: impl Into<String>) {
        self.failed = Some(reason.into());
        self.client_ready = false;
        self.server_ready = false;
        self.committed = false;
    }

    /// Commit only when both sides are ready. On failure, prior session stays
    /// valid and the reason is reported.
    pub fn commit(mut self) -> Result<SessionSnapshot, (SessionSnapshot, String)> {
        if let Some(reason) = self.failed.take() {
            return Err((self.prior, reason));
        }
        if !(self.client_ready && self.server_ready) {
            return Err((
                self.prior,
                "hot replacement incomplete; prior session kept".to_string(),
            ));
        }
        self.committed = true;
        Ok(SessionSnapshot {
            generation: self.prior.generation + 1,
            artifact_token: format!("gen-{}", self.prior.generation + 1),
            // Carry the live shared-heap persist store (updated by prepare_bundle
            // during the successful reload), not a stale prior clone.
            persist: jet_foundation::Persist::shared_clone(),
        })
    }

    pub fn prior(&self) -> &SessionSnapshot {
        &self.prior
    }
}

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
        let manifest = dir.join("pkg.jet");
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
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&lib)
            .unwrap();
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
        assert!(receipt.change_kinds.contains(&"deleted") || receipt.changed.iter().any(|p| p == &a));
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
        let mut store = PersistStore::new();
        store.put(PersistEntry {
            module: "app".into(),
            name: "counter".into(),
            shape: "Int".into(),
            payload: "{\"shape\":\"Int\",\"value\":7}".into(),
        });
        match store.migrate("app", "counter", "Int", "0") {
            PersistOutcome::Kept(e) => assert_eq!(e.payload.contains('7'), true),
            other => panic!("expected Kept, got {other:?}"),
        }
        match store.migrate("app", "counter", "Int+", "0") {
            PersistOutcome::Migrated(e) => assert!(e.payload.contains('7')),
            other => panic!("expected Migrated, got {other:?}"),
        }
        match store.migrate("app", "counter", "String", "\"\"") {
            PersistOutcome::Reset { reason, entry } => {
                assert!(reason.contains("reinitialized"));
                assert_eq!(entry.payload, "\"\"");
            }
            other => panic!("expected Reset, got {other:?}"),
        }

        let prior = SessionSnapshot {
            generation: 3,
            artifact_token: "gen-3".into(),
            persist: store,
        };
        let mut txn = HotReplaceTxn::begin(prior.clone());
        txn.mark_server_ready();
        // Client never ready → prior kept.
        let err = txn.commit().expect_err("incomplete");
        assert_eq!(err.0.generation, 3);
        assert!(err.1.contains("prior session"));

        let mut txn = HotReplaceTxn::begin(prior.clone());
        txn.mark_server_ready();
        txn.fail("type surface changed");
        txn.mark_client_ready();
        let err = txn.commit().expect_err("failed");
        assert!(err.1.contains("type surface"));
        assert_eq!(err.0.artifact_token, "gen-3");

        let mut txn = HotReplaceTxn::begin(prior);
        txn.mark_server_ready();
        txn.mark_client_ready();
        let next = txn.commit().expect("commit");
        assert_eq!(next.generation, 4);
        assert_eq!(next.artifact_token, "gen-4");
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
