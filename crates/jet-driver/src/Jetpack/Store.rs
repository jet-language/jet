//! Jetpack state + store roots (D-JPK12; hangar per unified ecosystem U2).
//!
//! End-state roots are user-owned by default: `$XDG_STATE_HOME/jet` (or
//! `~/.local/state/jet`) holds the content-addressed store — the **hangar**.
//! Jetpack *owns* the lifecycle even when the Nix provider realizes bytes into
//! `/nix/store` — a Jetpack hangar entry is a small metadata record under our
//! root that points at the realized output.
//!
//! A project also has a project-local **`.jet/` managed folder**
//! (`Syntax::SOURCE_ROOT_DIR`) holding the single lockfile (`.jet/lock`),
//! caches, and GC roots — never the realized packages, which live in the shared
//! hangar.
//!
//! U28 / D-JPK-NODAEMON1=A: no root-owned default path. `JETPACK_ROOT`
//! overrides everything (tests set it to a tempdir), but the ordinary path is
//! per-user and coordinated with file locks.

use super::JSON::{self, Json};
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The subdir of the resolved root that holds the content-addressed store.
/// Mirrors the trailing segment of the historical `Syntax::HANGAR_DIR`.
const HANGAR_SUBDIR: &str = "hangar";
const BUILD_SCRATCH_DIR: &str = "build-scratch";
const ACTIVE_TMP_MARKER: &str = ".active";
const AUTO_CLEAN_STAMP: &str = ".last-auto-clean";
const STALE_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const AUTO_CLEAN_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// The resolved root, plus whether we are using the default user-owned root.
pub struct Roots {
    pub root: PathBuf,
    pub dev_mode: bool,
}

impl Roots {
    /// The global content-addressed store (hangar) under this root.
    pub fn hangar_dir(&self) -> PathBuf {
        self.root.join(HANGAR_SUBDIR)
    }
}

/// The project-local `.jet/` managed folder for `project` (lockfile, caches,
/// GC roots). Never holds realized packages — those live in the shared hangar.
pub fn managed_dir(project: &Path) -> PathBuf {
    project.join(crate::Syntax::SOURCE_ROOT_DIR)
}

/// The single unified lockfile path for `project` (`.jet/lock`, U2).
pub fn lock_path(project: &Path) -> PathBuf {
    managed_dir(project).join("lock")
}

/// Resolve the Jetpack root with a dev-mode fallback.
///
/// 1. `JETPACK_ROOT` if set (tests, custom installs).
/// 2. `$XDG_STATE_HOME/jet` (or `~/.local/state/jet`) in dev mode otherwise.
pub fn resolve() -> Roots {
    if let Some(dir) = std::env::var_os("JETPACK_ROOT") {
        return Roots {
            root: PathBuf::from(dir),
            dev_mode: false,
        };
    }
    Roots {
        root: dev_root(),
        dev_mode: true,
    }
}

fn dev_root() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(x).join("jet");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local").join("state").join("jet")
}

/// A realized package recorded under the Jetpack store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreEntry {
    /// Directory name under `store/`, e.g. `fastfetch-2.1.0-<fp>` (D-PM1: name
    /// and version first, fingerprint last — never Nix's hash-first layout).
    pub id: String,
    pub name: String,
    /// Package version, or empty when the provider can't determine one.
    pub version: String,
    pub reference: String,
    /// The realized output root (often a `/nix/store/...` path).
    pub out: String,
    /// The `bin` directory to add to PATH.
    pub bin: String,
    /// Path to the built Rust rlib artifact, when a library package was compiled
    /// from its Cargo.toml by the core provider (D-BFS1). Empty for executable
    /// packages and for library packages that are consumed as staged source only.
    pub rlib: String,
    /// D-JPK-CACHE1=A: the A4 envelope — output hash, platform, signature slot,
    /// provenance. Empty for records written before the envelope existed.
    pub envelope: super::Envelope::Envelope,
    /// Unix seconds when this hangar object was first realized.
    pub realized_at: u64,
    /// Unix seconds when Jetpack last reused/refreshed this object.
    pub last_used_at: u64,
}

impl StoreEntry {
    fn meta_json(&self) -> String {
        let realized_at = self.realized_at.to_string();
        let last_used_at = self.last_used_at.to_string();
        JSON::object_of(&[
            ("name", &self.name),
            ("version", &self.version),
            ("ref", &self.reference),
            ("out", &self.out),
            ("bin", &self.bin),
            ("rlib", &self.rlib),
            ("output_hash", &self.envelope.output_hash),
            ("platform", &self.envelope.platform),
            ("signature", &self.envelope.signature),
            ("provenance", &self.envelope.provenance),
            ("realized_at", &realized_at),
            ("last_used_at", &last_used_at),
        ])
    }
}

/// Build the store id for a realization — human-readable `<name>-<version>`
/// first, then a short fingerprint of (ref, out) so two realizations of the
/// same name+version from different refs don't collide (D-PM1). The version
/// segment is dropped when unknown, leaving `<name>-<fp>`. Never hash-first:
/// identity for correctness is the lockfile, the dir name is for humans.
pub fn entry_id(name: &str, version: &str, reference: &str, out: &str) -> String {
    let fp = SHA256::sha256_hex(format!("{reference}\n{out}").as_bytes());
    let short = &fp[..12];
    if version.is_empty() {
        format!("{name}-{short}")
    } else {
        format!("{name}-{version}-{short}")
    }
}

/// Record (or refresh) a store entry; returns the entry with its id filled in.
pub fn record(
    roots: &Roots,
    name: &str,
    version: &str,
    reference: &str,
    out: &str,
    bin: &str,
    rlib: &str,
    envelope: &super::Envelope::Envelope,
) -> std::io::Result<StoreEntry> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar-record", || {
        let id = entry_id(name, version, reference, out);
        let dir = roots.hangar_dir().join(&id);
        let now = now_secs();
        let realized_at = read_meta(&dir).and_then(|m| m.realized_at).unwrap_or(now);
        let entry = StoreEntry {
            id: id.clone(),
            name: name.to_string(),
            version: version.to_string(),
            reference: reference.to_string(),
            out: out.to_string(),
            bin: bin.to_string(),
            rlib: rlib.to_string(),
            envelope: envelope.clone(),
            realized_at,
            last_used_at: now,
        };
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("meta.json"), entry.meta_json())?;
        Ok(entry)
    })
}

/// Read all recorded store entries (skipping malformed ones quietly).
pub fn list(roots: &Roots) -> Vec<StoreEntry> {
    let mut out = Vec::new();
    let store = roots.hangar_dir();
    let Ok(rd) = fs::read_dir(&store) else {
        return out;
    };
    for ent in rd.flatten() {
        let meta = ent.path().join("meta.json");
        let Ok(text) = fs::read_to_string(&meta) else {
            continue;
        };
        if let Some(parsed) = parse_meta(&text) {
            out.push(StoreEntry {
                id: ent.file_name().to_string_lossy().into_owned(),
                name: parsed.name,
                version: parsed.version,
                reference: parsed.reference,
                out: parsed.out,
                bin: parsed.bin,
                rlib: parsed.rlib,
                envelope: parsed.envelope,
                realized_at: parsed.realized_at.unwrap_or(0),
                last_used_at: parsed.last_used_at.unwrap_or(0),
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Return the newest recorded object for an exact ref, if one is already in
/// the hangar. U29 uses this before provider dispatch so a lock-satisfied
/// offline run never asks Nix/git for metadata.
pub fn find_by_reference(roots: &Roots, reference: &str) -> Option<StoreEntry> {
    list(roots)
        .into_iter()
        .filter(|e| e.reference == reference)
        .max_by_key(|e| e.last_used_at)
}

/// One line of `jet hangar du` output: a realized object, its on-disk size, and
/// whether it was built from source (vs substituted/nix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuEntry {
    pub id: String,
    pub name: String,
    pub bytes: u64,
    /// True when the A4 provenance shows a first-party source build.
    pub source_built: bool,
}

/// D-JPK-GC1 / U22: honest per-object disk usage. Sizes each realized object's
/// output tree (source-built objects live under the hangar; nix outputs live in
/// `/nix/store` and size 0 here since Jetpack doesn't own those bytes). A
/// source-built object is counted honestly, envelope and all.
pub fn du(roots: &Roots) -> Vec<DuEntry> {
    list(roots)
        .into_iter()
        .map(|e| {
            let bytes = dir_size(std::path::Path::new(&e.out));
            let source_built = e.envelope.provenance.contains("core-");
            DuEntry {
                id: e.id,
                name: e.name,
                bytes,
                source_built,
            }
        })
        .collect()
}

/// Total bytes on disk of a directory tree (0 if it isn't a local dir, e.g. a
/// `/nix/store` path Jetpack references but does not own).
fn dir_size(path: &std::path::Path) -> u64 {
    if !path.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    if let Ok(rd) = fs::read_dir(path) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(meta) = p.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CleanReport {
    pub removed_objects: usize,
    pub removed_bytes: u64,
    pub swept_tmp: usize,
    pub swept_tmp_bytes: u64,
    pub optimized_files: usize,
    pub optimized_bytes: u64,
}

impl CleanReport {
    pub fn is_empty(&self) -> bool {
        self.removed_objects == 0
            && self.removed_bytes == 0
            && self.swept_tmp == 0
            && self.swept_tmp_bytes == 0
            && self.optimized_files == 0
            && self.optimized_bytes == 0
    }
}

/// D-JPK-GC1=B / U22: collect only unreferenced stale hangar objects, sweep
/// orphan build scratch, then optimize duplicate Jet-owned files. Lockfile
/// reachable entries and unknown legacy records are retained.
pub fn clean(roots: &Roots) -> std::io::Result<CleanReport> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar-clean", || clean_unlocked(roots))
}

fn clean_unlocked(roots: &Roots) -> std::io::Result<CleanReport> {
    let store = roots.hangar_dir();
    fs::create_dir_all(&store)?;
    let live = current_lock_roots();
    let mut report = sweep_build_scratch(&store)?;
    let now = now_secs();

    for ent in object_dirs(&store)? {
        let path = ent.path();
        let id = ent.file_name().to_string_lossy().into_owned();
        let Some(meta) = read_meta(&path) else {
            continue;
        };
        if is_live(&id, &meta, &live) || meta.last_used_at.is_none() {
            continue;
        }
        let last_used = meta.last_used_at.unwrap_or(now);
        if now.saturating_sub(last_used) < STALE_AFTER.as_secs() {
            continue;
        }
        let bytes = dir_size(&path);
        fs::remove_dir_all(&path)?;
        report.removed_objects += 1;
        report.removed_bytes += bytes;
    }

    let opt = optimize_hangar(&store)?;
    report.optimized_files += opt.optimized_files;
    report.optimized_bytes += opt.optimized_bytes;
    Ok(report)
}

pub fn maybe_auto_clean(roots: &Roots) -> std::io::Result<Option<CleanReport>> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar-clean", || {
        let hangar = roots.hangar_dir();
        fs::create_dir_all(&hangar)?;
        let stamp = hangar.join(AUTO_CLEAN_STAMP);
        let now = SystemTime::now();
        if std::env::var_os("JETPACK_AUTO_CLEAN_ALWAYS").is_none() {
            if let Ok(meta) = fs::metadata(&stamp) {
                if let Ok(modified) = meta.modified() {
                    if now.duration_since(modified).unwrap_or_default() < AUTO_CLEAN_AFTER {
                        return Ok(None);
                    }
                }
            }
        }
        let report = clean_unlocked(roots)?;
        let _ = fs::write(stamp, now_secs().to_string());
        Ok(Some(report))
    })
}

fn sweep_build_scratch(hangar: &Path) -> std::io::Result<CleanReport> {
    let root = hangar.join(BUILD_SCRATCH_DIR);
    let mut report = CleanReport::default();
    let Ok(rd) = fs::read_dir(&root) else {
        return Ok(report);
    };
    for ent in rd.flatten() {
        let path = ent.path();
        if path.join(ACTIVE_TMP_MARKER).exists() {
            continue;
        }
        let bytes = dir_size(&path);
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
        report.swept_tmp += 1;
        report.swept_tmp_bytes += bytes;
    }
    Ok(report)
}

fn optimize_hangar(hangar: &Path) -> std::io::Result<CleanReport> {
    let mut seen: BTreeMap<(u64, String), PathBuf> = BTreeMap::new();
    let mut report = CleanReport::default();
    for obj in object_dirs(hangar)? {
        for file in files_under(&obj.path()) {
            if file.file_name().and_then(|n| n.to_str()) == Some("meta.json") {
                continue;
            }
            let Ok(meta) = fs::metadata(&file) else {
                continue;
            };
            if !meta.is_file() || meta.len() == 0 {
                continue;
            }
            let Ok(bytes) = fs::read(&file) else { continue };
            let key = (meta.len(), SHA256::sha256_hex(&bytes));
            if let Some(first) = seen.get(&key) {
                if hardlink_replace(first, &file).is_ok() {
                    report.optimized_files += 1;
                    report.optimized_bytes += meta.len();
                }
            } else {
                seen.insert(key, file);
            }
        }
    }
    Ok(report)
}

fn hardlink_replace(first: &Path, file: &Path) -> std::io::Result<()> {
    if first == file {
        return Ok(());
    }
    let tmp = file.with_extension(format!("jet-dedup-{}", std::process::id()));
    fs::rename(file, &tmp)?;
    match fs::hard_link(first, file) {
        Ok(()) => {
            let _ = fs::remove_file(&tmp);
            Ok(())
        }
        Err(e) => {
            let _ = fs::rename(&tmp, file);
            Err(e)
        }
    }
}

fn object_dirs(hangar: &Path) -> std::io::Result<Vec<fs::DirEntry>> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(hangar) {
        for ent in rd.flatten() {
            let path = ent.path();
            let name = ent.file_name().to_string_lossy().into_owned();
            if path.is_dir() && name != BUILD_SCRATCH_DIR {
                out.push(ent);
            }
        }
    }
    out.sort_by_key(|e| e.file_name());
    Ok(out)
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(root) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                out.extend(files_under(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
struct ParsedMeta {
    name: String,
    version: String,
    reference: String,
    out: String,
    bin: String,
    rlib: String,
    envelope: super::Envelope::Envelope,
    realized_at: Option<u64>,
    last_used_at: Option<u64>,
}

fn read_meta(dir: &Path) -> Option<ParsedMeta> {
    let text = fs::read_to_string(dir.join("meta.json")).ok()?;
    parse_meta(&text)
}

fn parse_meta(text: &str) -> Option<ParsedMeta> {
    let j = JSON::parse(text).ok()?;
    let get = |k: &str| j.get(k).and_then(Json::as_str).map(str::to_string).ok();
    let name = get("name")?;
    let reference = get("ref")?;
    let out = get("out")?;
    let bin = get("bin")?;
    Some(ParsedMeta {
        name,
        version: get("version").unwrap_or_default(),
        reference,
        out,
        bin,
        rlib: get("rlib").unwrap_or_default(),
        envelope: super::Envelope::Envelope {
            output_hash: get("output_hash").unwrap_or_default(),
            platform: get("platform").unwrap_or_default(),
            signature: get("signature").unwrap_or_default(),
            provenance: get("provenance").unwrap_or_default(),
        },
        realized_at: get("realized_at").and_then(|s| s.parse().ok()),
        last_used_at: get("last_used_at").and_then(|s| s.parse().ok()),
    })
}

#[derive(Default)]
struct LiveRoots {
    ids: BTreeSet<String>,
    output_hashes: BTreeSet<String>,
    name_versions: BTreeSet<(String, String)>,
}

fn current_lock_roots() -> LiveRoots {
    let Ok(cwd) = std::env::current_dir() else {
        return LiveRoots::default();
    };
    let Some(lock_path) = nearest_lock_path(&cwd) else {
        return LiveRoots::default();
    };
    let Ok(raw) = fs::read_to_string(lock_path) else {
        return LiveRoots::default();
    };
    let Ok(lock) = crate::Lock::parse(&raw) else {
        return LiveRoots::default();
    };
    let mut roots = LiveRoots::default();
    for pkg in lock.packages {
        roots.name_versions.insert((pkg.name, pkg.version));
        if let Some(env) = pkg.envelope {
            if !env.output_hash.is_empty() {
                roots.output_hashes.insert(env.output_hash);
            }
        }
    }
    for toolchain in lock.toolchains {
        roots.ids.insert(toolchain.id);
        if !toolchain.envelope.output_hash.is_empty() {
            roots.output_hashes.insert(toolchain.envelope.output_hash);
        }
    }
    roots
}

fn nearest_lock_path(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        let lock = lock_path(current);
        if lock.is_file() {
            return Some(lock);
        }
        dir = current.parent();
    }
    None
}

fn is_live(id: &str, meta: &ParsedMeta, roots: &LiveRoots) -> bool {
    roots.ids.contains(id)
        || (!meta.envelope.output_hash.is_empty()
            && roots.output_hashes.contains(&meta.envelope.output_hash))
        || (meta.envelope.output_hash.is_empty()
            && roots
                .name_versions
                .contains(&(meta.name.clone(), meta.version.clone())))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_roots() -> (Roots, tempdir::Guard) {
        let g = tempdir::Guard::new("jpk-store");
        let roots = Roots {
            root: g.path.clone(),
            dev_mode: true,
        };
        (roots, g)
    }

    #[test]
    fn record_and_list_roundtrip() {
        let (roots, _g) = temp_roots();
        let e = record(
            &roots,
            "fastfetch",
            "2.1.0",
            "nixpkgs:fastfetch",
            "/nix/store/x",
            "/nix/store/x/bin",
            "",
            &super::super::Envelope::Envelope::default(),
        )
        .unwrap();
        // Name-and-version first, fingerprint last (D-PM1).
        assert!(e.id.starts_with("fastfetch-2.1.0-"));
        let listed = list(&roots);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], e);
    }

    #[test]
    fn clean_keeps_fresh_entries() {
        let (roots, _g) = temp_roots();
        record(
            &roots,
            "a",
            "1.0",
            "nixpkgs:a",
            "/nix/store/a",
            "/nix/store/a/bin",
            "",
            &super::super::Envelope::Envelope::default(),
        )
        .unwrap();
        record(
            &roots,
            "b",
            "1.0",
            "nixpkgs:b",
            "/nix/store/b",
            "/nix/store/b/bin",
            "",
            &super::super::Envelope::Envelope::default(),
        )
        .unwrap();
        let report = clean(&roots).unwrap();
        assert_eq!(report.removed_objects, 0);
        assert_eq!(list(&roots).len(), 2);
    }

    #[test]
    fn ids_differ_by_ref() {
        let a = entry_id("x", "1.0", "nixpkgs:x", "/o");
        let b = entry_id("x", "1.0", "github:o/x", "/o");
        assert_ne!(a, b);
    }

    #[test]
    fn id_omits_empty_version() {
        // Unknown version falls back to `<name>-<fp>`, no dangling segment.
        let id = entry_id("x", "", "nixpkgs:x", "/o");
        assert!(id.starts_with("x-"));
        assert!(!id.starts_with("x--"));
    }
}

/// Minimal scoped tempdir for tests (std-only; auto-removes on drop).
#[cfg(test)]
mod tempdir {
    use std::path::PathBuf;

    pub struct Guard {
        pub path: PathBuf,
    }

    impl Guard {
        pub fn new(tag: &str) -> Guard {
            let mut path = std::env::temp_dir();
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            path.push(format!("{tag}-{nanos}-{:?}", std::thread::current().id()));
            std::fs::create_dir_all(&path).unwrap();
            Guard { path }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
