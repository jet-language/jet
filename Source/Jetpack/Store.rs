//! Jetpack state + store roots (D-JPK12; hangar per unified ecosystem U2).
//!
//! End-state roots are system-scoped: `/etc/jet/` for config/state and the one
//! global, content-addressed store — the **hangar** — at `/etc/jet/hangar/`
//! (`Syntax::HANGAR_DIR`). Jetpack *owns* the lifecycle even when the Nix
//! provider realizes bytes into `/nix/store` — a Jetpack hangar entry is a small
//! metadata record under our root that points at the realized output.
//!
//! A project also has a project-local **`.jet/` managed folder**
//! (`Syntax::SOURCE_ROOT_DIR`) holding the single lockfile (`.jet/lock`),
//! caches, and GC roots — never the realized packages, which live in the shared
//! hangar.
//!
//! Permissions: `/etc/jet` is usually not writable by a normal user, so the
//! root resolves with a dev-mode fallback. `JETPACK_ROOT` overrides everything
//! (tests set it to a tempdir).

use super::JSON::{self, Json};
use crate::SHA256;
use std::path::{Path, PathBuf};
use std::fs;

/// The subdir of the resolved root that holds the content-addressed store.
/// Mirrors the trailing segment of `Syntax::HANGAR_DIR` (`/etc/jet/hangar`).
const HANGAR_SUBDIR: &str = "hangar";

/// The resolved root, plus whether we fell back out of `/etc/jet`.
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

const SYSTEM_ROOT: &str = "/etc/jet";

/// Resolve the Jetpack root with a dev-mode fallback.
///
/// 1. `JETPACK_ROOT` if set (tests, custom installs).
/// 2. `/etc/jet` if we can create/write it (the ratified system root).
/// 3. `$XDG_STATE_HOME/jet` (or `~/.local/state/jet`) in dev mode otherwise.
pub fn resolve() -> Roots {
    if let Some(dir) = std::env::var_os("JETPACK_ROOT") {
        return Roots {
            root: PathBuf::from(dir),
            dev_mode: false,
        };
    }
    let system = PathBuf::from(SYSTEM_ROOT);
    if can_write(&system) {
        return Roots {
            root: system,
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

/// True if `dir` exists and is writable, or can be created.
fn can_write(dir: &std::path::Path) -> bool {
    if dir.exists() {
        // Probe by trying to create the store subdir.
        fs::create_dir_all(dir.join(HANGAR_SUBDIR)).is_ok()
    } else {
        fs::create_dir_all(dir.join(HANGAR_SUBDIR)).is_ok()
    }
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
}

impl StoreEntry {
    fn meta_json(&self) -> String {
        JSON::object_of(&[
            ("name", &self.name),
            ("version", &self.version),
            ("ref", &self.reference),
            ("out", &self.out),
            ("bin", &self.bin),
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
) -> std::io::Result<StoreEntry> {
    let id = entry_id(name, version, reference, out);
    let entry = StoreEntry {
        id: id.clone(),
        name: name.to_string(),
        version: version.to_string(),
        reference: reference.to_string(),
        out: out.to_string(),
        bin: bin.to_string(),
    };
    let dir = roots.hangar_dir().join(&id);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("meta.json"), entry.meta_json())?;
    Ok(entry)
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
        let Ok(j) = JSON::parse(&text) else { continue };
        let get = |k: &str| j.get(k).and_then(Json::as_str).map(str::to_string).ok();
        if let (Some(name), Some(reference), Some(o), Some(b)) =
            (get("name"), get("ref"), get("out"), get("bin"))
        {
            out.push(StoreEntry {
                id: ent.file_name().to_string_lossy().into_owned(),
                name,
                // Older records predate the version field; treat as unknown.
                version: get("version").unwrap_or_default(),
                reference,
                out: o,
                bin: b,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Remove every recorded store entry; returns how many were removed.
///
/// Phase 1 temporary environments leave no GC roots, so "unused" means every
/// recorded entry. The realized Nix outputs themselves live in `/nix/store`
/// and are reclaimed by `nix store gc`; Jetpack only drops its own records.
pub fn clean(roots: &Roots) -> std::io::Result<usize> {
    let store = roots.hangar_dir();
    let mut removed = 0;
    if let Ok(rd) = fs::read_dir(&store) {
        for ent in rd.flatten() {
            if ent.path().is_dir() {
                fs::remove_dir_all(ent.path())?;
                removed += 1;
            }
        }
    }
    Ok(removed)
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
        )
        .unwrap();
        // Name-and-version first, fingerprint last (D-PM1).
        assert!(e.id.starts_with("fastfetch-2.1.0-"));
        let listed = list(&roots);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], e);
    }

    #[test]
    fn clean_removes_entries() {
        let (roots, _g) = temp_roots();
        record(&roots, "a", "1.0", "nixpkgs:a", "/nix/store/a", "/nix/store/a/bin").unwrap();
        record(&roots, "b", "1.0", "nixpkgs:b", "/nix/store/b", "/nix/store/b/bin").unwrap();
        assert_eq!(clean(&roots).unwrap(), 2);
        assert!(list(&roots).is_empty());
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
