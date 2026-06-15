//! Jetpack state + store roots (D-JPK12).
//!
//! End-state roots are system-scoped: `/etc/jet/` for config/state and
//! `/etc/jet/store/` for the store. Jetpack *owns* the lifecycle even when the
//! Nix provider realizes bytes into `/nix/store` — a Jetpack store entry is a
//! small metadata record under our root that points at the realized output.
//!
//! Permissions: `/etc/jet` is usually not writable by a normal user, so the
//! root resolves with a dev-mode fallback. `JETPACK_ROOT` overrides everything
//! (tests set it to a tempdir).

use super::json::{self, Json};
use crate::sha256;
use std::fs;
use std::path::PathBuf;

/// The resolved root, plus whether we fell back out of `/etc/jet`.
pub struct Roots {
    pub root: PathBuf,
    pub dev_mode: bool,
}

impl Roots {
    pub fn store_dir(&self) -> PathBuf {
        self.root.join("store")
    }
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
        fs::create_dir_all(dir.join("store")).is_ok()
    } else {
        fs::create_dir_all(dir.join("store")).is_ok()
    }
}

/// A realized package recorded under the Jetpack store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreEntry {
    /// Directory name under `store/`, e.g. `fastfetch-<fp>`.
    pub id: String,
    pub name: String,
    pub reference: String,
    /// The realized output root (often a `/nix/store/...` path).
    pub out: String,
    /// The `bin` directory to add to PATH.
    pub bin: String,
}

impl StoreEntry {
    fn meta_json(&self) -> String {
        json::object_of(&[
            ("name", &self.name),
            ("ref", &self.reference),
            ("out", &self.out),
            ("bin", &self.bin),
        ])
    }
}

/// Build the store id for a (name, ref, out) triple — name plus a short hash so
/// two realizations of the same name from different refs don't collide.
pub fn entry_id(name: &str, reference: &str, out: &str) -> String {
    let fp = sha256::sha256_hex(format!("{reference}\n{out}").as_bytes());
    format!("{name}-{}", &fp[..12])
}

/// Record (or refresh) a store entry; returns the entry with its id filled in.
pub fn record(
    roots: &Roots,
    name: &str,
    reference: &str,
    out: &str,
    bin: &str,
) -> std::io::Result<StoreEntry> {
    let id = entry_id(name, reference, out);
    let entry = StoreEntry {
        id: id.clone(),
        name: name.to_string(),
        reference: reference.to_string(),
        out: out.to_string(),
        bin: bin.to_string(),
    };
    let dir = roots.store_dir().join(&id);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("meta.json"), entry.meta_json())?;
    Ok(entry)
}

/// Read all recorded store entries (skipping malformed ones quietly).
pub fn list(roots: &Roots) -> Vec<StoreEntry> {
    let mut out = Vec::new();
    let store = roots.store_dir();
    let Ok(rd) = fs::read_dir(&store) else {
        return out;
    };
    for ent in rd.flatten() {
        let meta = ent.path().join("meta.json");
        let Ok(text) = fs::read_to_string(&meta) else {
            continue;
        };
        let Ok(j) = json::parse(&text) else { continue };
        let get = |k: &str| j.get(k).and_then(Json::as_str).map(str::to_string).ok();
        if let (Some(name), Some(reference), Some(o), Some(b)) =
            (get("name"), get("ref"), get("out"), get("bin"))
        {
            out.push(StoreEntry {
                id: ent.file_name().to_string_lossy().into_owned(),
                name,
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
    let store = roots.store_dir();
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
            "nixpkgs:fastfetch",
            "/nix/store/x",
            "/nix/store/x/bin",
        )
        .unwrap();
        let listed = list(&roots);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], e);
    }

    #[test]
    fn clean_removes_entries() {
        let (roots, _g) = temp_roots();
        record(&roots, "a", "nixpkgs:a", "/nix/store/a", "/nix/store/a/bin").unwrap();
        record(&roots, "b", "nixpkgs:b", "/nix/store/b", "/nix/store/b/bin").unwrap();
        assert_eq!(clean(&roots).unwrap(), 2);
        assert!(list(&roots).is_empty());
    }

    #[test]
    fn ids_differ_by_ref() {
        let a = entry_id("x", "nixpkgs:x", "/o");
        let b = entry_id("x", "github:o/x", "/o");
        assert_ne!(a, b);
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
