//! Read-only slice of the Jetpack hangar store (D-JPK12; U2) — root
//! resolution and listing of already-recorded entries only.
//!
//! Card #367 / D-PRODUCT-SPLIT1=C: this is the "store" data half of the
//! model/engine split. Realization, cache verification/leasing, GC, and
//! quarantine stay in `jetpack::Store` (they need the provider/network
//! engine); `jetpack::Store` re-exports every item here so its own internal
//! callers are unaffected. `jet-driver`'s module loader consumes `resolve`
//! and `list` directly from this crate to resolve `use <pkg>` imports
//! against packages already realized into the hangar, without depending on
//! Jetpack's realization engine.

use crate::JSON::{self, JSONValue};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// The subdir of the resolved root that holds the content-addressed store.
/// D-ECO-HANGARPATH1 uses the platform's native data-directory spelling.
#[cfg(any(target_os = "macos", windows))]
const HANGAR_SUBDIR: &str = "Hangar";
#[cfg(not(any(target_os = "macos", windows)))]
const HANGAR_SUBDIR: &str = "hangar";
const LEGACY_HANGAR_SUBDIR: &str = "hangar";
const SHARED_CAS_ENV: &str = "JETPACK_SHARED_CAS";
const MAX_STORE_OBJECTS: usize = 1_000_000;
const MAX_STORE_META_BYTES: u64 = 1 << 20;

/// The resolved root, plus whether we are using the default user-owned root.
pub struct Roots {
    pub root: PathBuf,
    pub dev_mode: bool,
}

impl Roots {
    /// Construct an explicit non-default root for an isolated operation.
    ///
    /// Independent build roots use the same model as the user store, but must
    /// never inherit the native user-data spelling from `resolve`.
    pub fn at(root: PathBuf) -> Self {
        Self {
            root,
            dev_mode: false,
        }
    }

    /// The global content-addressed store (hangar) under this root.
    pub fn hangar_dir(&self) -> PathBuf {
        self.root.join(if self.dev_mode {
            HANGAR_SUBDIR
        } else {
            // Explicit JETPACK_ROOT and administrator-owned broker roots keep
            // their historical lower-case child for test/custom-root parity.
            LEGACY_HANGAR_SUBDIR
        })
    }

    /// The immutable payload pool this root shares.
    ///
    /// The default user root uses the machine-wide pool, so concurrent agents
    /// do not each retain a full copy. A root pointed somewhere else by
    /// `JETPACK_ROOT` owns its own pool instead: that root was chosen for
    /// isolation, and pooling its bytes into the user's store would both break
    /// that isolation and leave inode peers outside the hangar being verified.
    pub fn shared_cas_dir(&self) -> PathBuf {
        if self.root == user_data_root() {
            shared_cas_dir()
        } else {
            self.hangar_dir().join("cas")
        }
    }

    /// Discover user and custom Hangar roots visible from this machine.
    ///
    /// Custom roots are commonly sibling directories under an agent cache or
    /// an XDG data directory. Discovery stays bounded to those known parents;
    /// it never walks an arbitrary filesystem tree.
    pub fn machine_roots() -> Vec<Self> {
        let resolved = resolve();
        let default = Self {
            root: user_data_root(),
            dev_mode: true,
        };
        let legacy = Self::at(legacy_user_root());
        let mut roots = Vec::new();
        let mut parents = Vec::new();

        for candidate in [resolved, default, legacy] {
            add_machine_root(&mut roots, candidate);
        }
        for root in &roots {
            if let Some(parent) = root.root.parent() {
                parents.push(parent.to_path_buf());
            }
        }
        for parent in [
            environment_path("XDG_DATA_HOME"),
            environment_path("XDG_STATE_HOME"),
            environment_path("LOCALAPPDATA"),
            Some(environment_path("XDG_CACHE_HOME").unwrap_or_else(|| home_dir().join(".cache"))),
        ]
        .into_iter()
        .flatten()
        {
            parents.push(parent);
        }

        parents.sort();
        parents.dedup();
        for parent in parents {
            let Ok(entries) = fs::read_dir(parent) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(metadata) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    continue;
                }
                let native_hangar = path.join(HANGAR_SUBDIR);
                let legacy_hangar = path.join(LEGACY_HANGAR_SUBDIR);
                if real_directory(&native_hangar) {
                    add_machine_root(
                        &mut roots,
                        Self {
                            root: path,
                            dev_mode: true,
                        },
                    );
                } else if real_directory(&legacy_hangar) {
                    add_machine_root(&mut roots, Self::at(path));
                }
            }
        }
        roots
    }
}

/// Resolve the machine/user shared immutable payload pool.
pub fn shared_cas_dir() -> PathBuf {
    if let Some(path) = environment_path(SHARED_CAS_ENV) {
        return path;
    }
    user_data_root().join(HANGAR_SUBDIR).join("cas")
}

fn add_machine_root(roots: &mut Vec<Roots>, candidate: Roots) {
    if !roots
        .iter()
        .any(|root| root.root == candidate.root && root.hangar_dir() == candidate.hangar_dir())
    {
        roots.push(candidate);
    }
}

fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| !metadata.file_type().is_symlink() && metadata.is_dir())
        .unwrap_or(false)
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

/// Resolve the Jetpack root with a user-owned default.
///
/// 1. `JETPACK_ROOT` if set (tests, custom installs).
/// 2. The platform data directory from D-ECO-HANGARPATH1 otherwise.
pub fn resolve() -> Roots {
    if let Some(dir) = environment_path("JETPACK_ROOT") {
        return Roots {
            root: PathBuf::from(dir),
            dev_mode: false,
        };
    }
    Roots {
        root: user_data_root(),
        dev_mode: true,
    }
}

fn home_dir() -> PathBuf {
    #[cfg(windows)]
    {
        return environment_path("USERPROFILE")
            .or_else(|| environment_path("HOME"))
            .unwrap_or_else(|| PathBuf::from("."));
    }
    #[cfg(not(windows))]
    environment_path("HOME").unwrap_or_else(|| PathBuf::from("."))
}

fn environment_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn user_data_root() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        return home_dir()
            .join("Library")
            .join("Application Support")
            .join("Jet");
    }
    #[cfg(windows)]
    {
        return environment_path("LOCALAPPDATA")
            .unwrap_or_else(|| home_dir().join("AppData").join("Local"))
            .join("Jet");
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        environment_path("XDG_DATA_HOME")
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home_dir().join(".local").join("share"))
            .join("jet")
    }
}

/// The pre-D-ECO-HANGARPATH1 user root. It is kept as a migration source only;
/// new resolution never selects it.
pub fn legacy_user_root() -> PathBuf {
    environment_path("XDG_STATE_HOME")
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home_dir().join(".local").join("state"))
        .join("jet")
}

/// The old Hangar path used by the pre-D-ECO-HANGARPATH1 resolver.
pub fn legacy_user_hangar_dir() -> PathBuf {
    legacy_user_root().join(LEGACY_HANGAR_SUBDIR)
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
    pub envelope: crate::Envelope::Envelope,
    /// JP0 cache identity. Legacy records have empty fields and can never hit.
    pub cache_identity: CacheIdentity,
    /// Content digests this object references (runtime/build closure edges).
    pub references: Vec<String>,
    /// Named outputs → content digests (`out` is the primary).
    pub named_outputs: BTreeMap<String, String>,
    /// Explicit platform artifact kind that permits semantic xattrs on ingest.
    /// Empty = semantic xattrs rejected (default).
    pub platform_artifact_kind: String,
    /// Versioned canonical producer/action/source replay record. Empty only for
    /// pre-JP4 metadata; engine migration must validate or reject it.
    pub producer_record: String,
    /// D-ECO-RECEIPT2 / D-ECO-RECEIPTSTORE1: digest of the immutable Hangar
    /// receipt connected to this realized entry. Empty only for pre-receipt
    /// metadata that the closure migration must upgrade before reuse.
    pub receipt: String,
    /// Unix seconds when this hangar object was first realized.
    pub realized_at: u64,
    /// Unix seconds when Jetpack last reused/refreshed this object.
    pub last_used_at: u64,
}

impl StoreEntry {
    /// Public so the `jetpack` engine (a different crate) can write it into
    /// `meta.json` when recording/refreshing an entry.
    pub fn meta_json(&self) -> String {
        let realized_at = self.realized_at.to_string();
        let last_used_at = self.last_used_at.to_string();
        let references = json_string_array(&self.references);
        let named_outputs = json_string_object(&self.named_outputs);
        let mut out = String::from("{\n");
        let mut field = |key: &str, value: &str, raw: bool| {
            out.push_str("  ");
            out.push_str(&JSON::quote(key));
            out.push_str(": ");
            if raw {
                out.push_str(value);
            } else {
                out.push_str(&JSON::quote(value));
            }
            out.push_str(",\n");
        };
        field("name", &self.name, false);
        field("version", &self.version, false);
        field("ref", &self.reference, false);
        field("out", &self.out, false);
        field("bin", &self.bin, false);
        field("rlib", &self.rlib, false);
        field("output_hash", &self.envelope.output_hash, false);
        field("platform", &self.envelope.platform, false);
        field("signature", &self.envelope.signature, false);
        field("provenance", &self.envelope.provenance, false);
        field(
            "source_fingerprint",
            &self.cache_identity.source_fingerprint,
            false,
        );
        field(
            "recipe_fingerprint",
            &self.cache_identity.recipe_fingerprint,
            false,
        );
        field(
            "policy_fingerprint",
            &self.cache_identity.policy_fingerprint,
            false,
        );
        field("identity_platform", &self.cache_identity.platform, false);
        field("references", &references, true);
        field("named_outputs", &named_outputs, true);
        field(
            "platform_artifact_kind",
            &self.platform_artifact_kind,
            false,
        );
        field("producer_record", &self.producer_record, false);
        field("receipt", &self.receipt, false);
        field("realized_at", &realized_at, false);
        // last field — no trailing comma
        out.push_str("  ");
        out.push_str(&JSON::quote("last_used_at"));
        out.push_str(": ");
        out.push_str(&JSON::quote(&last_used_at));
        out.push('\n');
        out.push('}');
        out
    }
}

fn json_string_array(items: &[String]) -> String {
    let mut out = String::from("[");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&JSON::quote(item));
    }
    out.push(']');
    out
}

fn json_string_object(map: &BTreeMap<String, String>) -> String {
    let mut out = String::from("{");
    for (i, (k, v)) in map.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&JSON::quote(k));
        out.push_str(": ");
        out.push_str(&JSON::quote(v));
    }
    out.push('}');
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CacheIdentity {
    pub source_fingerprint: String,
    pub recipe_fingerprint: String,
    pub policy_fingerprint: String,
    pub platform: String,
}

/// Read all recorded store entries (skipping malformed ones quietly).
pub fn list(roots: &Roots) -> Vec<StoreEntry> {
    list_impl(roots, false).unwrap_or_default()
}

/// Read all recorded store entries and fail closed on a malformed or
/// over-budget projection. The engine uses this boundary for integrity-
/// sensitive operations; the infallible `list` above remains a read-only
/// compatibility view for the driver.
pub fn list_checked(roots: &Roots) -> io::Result<Vec<StoreEntry>> {
    list_impl(roots, true)
}

fn list_impl(roots: &Roots, strict: bool) -> io::Result<Vec<StoreEntry>> {
    let mut out = Vec::new();
    let store = roots.hangar_dir();
    let rd = match fs::read_dir(&store) {
        Ok(rd) => rd,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(out),
        Err(_error) if !strict => return Ok(out),
        Err(error) => return Err(error),
    };
    let mut object_count = 0usize;
    for ent in rd {
        let ent = match ent {
            Ok(ent) => ent,
            Err(_error) if !strict => continue,
            Err(error) => return Err(error),
        };
        let entry_type = match ent.file_type() {
            Ok(entry_type) => entry_type,
            Err(_error) if !strict => continue,
            Err(error) => return Err(error),
        };
        if !entry_type.is_dir() {
            continue;
        }
        let meta = ent.path().join("meta.json");
        let metadata = match fs::symlink_metadata(&meta) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_error) if !strict => continue,
            Err(error) => return Err(error),
        };
        if !metadata.file_type().is_file() {
            if strict {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Hangar metadata `{}` is not a regular file", meta.display()),
                ));
            }
            continue;
        }
        object_count = object_count.saturating_add(1);
        if object_count > MAX_STORE_OBJECTS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Hangar contains more than {MAX_STORE_OBJECTS} store objects"),
            ));
        }
        let text = match read_bounded_text(&meta, MAX_STORE_META_BYTES) {
            Ok(text) => text,
            Err(_error) if !strict => continue,
            Err(error) => return Err(error),
        };
        let parsed = match parse_meta(&text) {
            Some(parsed) => parsed,
            None if !strict => continue,
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid Hangar metadata `{}`", meta.display()),
                ));
            }
        };
        {
            out.push(StoreEntry {
                id: ent.file_name().to_string_lossy().into_owned(),
                name: parsed.name,
                version: parsed.version,
                reference: parsed.reference,
                out: parsed.out,
                bin: parsed.bin,
                rlib: parsed.rlib,
                envelope: parsed.envelope,
                cache_identity: parsed.cache_identity,
                references: parsed.references,
                named_outputs: parsed.named_outputs,
                platform_artifact_kind: parsed.platform_artifact_kind,
                producer_record: parsed.producer_record,
                receipt: parsed.receipt,
                realized_at: parsed.realized_at.unwrap_or(0),
                last_used_at: parsed.last_used_at.unwrap_or(0),
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn read_bounded_text(path: &Path, limit: u64) -> io::Result<String> {
    if fs::metadata(path)?.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Hangar metadata `{}` exceeds {limit} bytes", path.display()),
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Hangar metadata `{}` exceeds {limit} bytes", path.display()),
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Hangar metadata `{}` is not UTF-8", path.display()),
        )
    })
}

/// A `meta.json` record parsed back into typed fields. Public so the
/// `jetpack` engine's `read_meta` (used when refreshing an entry) can share
/// this parser instead of re-implementing it.
#[derive(Debug, Clone)]
pub struct ParsedMeta {
    pub name: String,
    pub version: String,
    pub reference: String,
    pub out: String,
    pub bin: String,
    pub rlib: String,
    pub envelope: crate::Envelope::Envelope,
    pub cache_identity: CacheIdentity,
    pub references: Vec<String>,
    pub named_outputs: BTreeMap<String, String>,
    pub platform_artifact_kind: String,
    pub producer_record: String,
    pub receipt: String,
    pub realized_at: Option<u64>,
    pub last_used_at: Option<u64>,
}

pub fn parse_meta(text: &str) -> Option<ParsedMeta> {
    let j = JSON::parse(text).ok()?;
    let get = |k: &str| {
        j.get(k)
            .and_then(JSONValue::as_str)
            .map(str::to_string)
            .ok()
    };
    let name = get("name")?;
    let reference = get("ref")?;
    let out = get("out")?;
    let bin = get("bin")?;
    let references = j
        .get("references")
        .ok()
        .and_then(|v| v.as_array().ok())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().ok().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let named_outputs = j
        .get("named_outputs")
        .ok()
        .and_then(|v| v.as_object().ok())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().ok().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    Some(ParsedMeta {
        name,
        version: get("version").unwrap_or_default(),
        reference,
        out,
        bin,
        rlib: get("rlib").unwrap_or_default(),
        envelope: crate::Envelope::Envelope {
            output_hash: get("output_hash").unwrap_or_default(),
            platform: get("platform").unwrap_or_default(),
            signature: get("signature").unwrap_or_default(),
            provenance: get("provenance").unwrap_or_default(),
        },
        cache_identity: CacheIdentity {
            source_fingerprint: get("source_fingerprint").unwrap_or_default(),
            recipe_fingerprint: get("recipe_fingerprint").unwrap_or_default(),
            policy_fingerprint: get("policy_fingerprint").unwrap_or_default(),
            platform: get("identity_platform").unwrap_or_default(),
        },
        references,
        named_outputs,
        platform_artifact_kind: get("platform_artifact_kind").unwrap_or_default(),
        producer_record: get("producer_record").unwrap_or_default(),
        receipt: get("receipt").unwrap_or_default(),
        realized_at: get("realized_at").and_then(|s| s.parse().ok()),
        last_used_at: get("last_used_at").and_then(|s| s.parse().ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::Roots;
    use std::path::PathBuf;

    #[test]
    fn hangar_dir_uses_the_native_leaf() {
        let roots = Roots {
            root: PathBuf::from("/tmp/jet-root"),
            dev_mode: true,
        };
        let expected_leaf = if cfg!(any(target_os = "macos", windows)) {
            "Hangar"
        } else {
            "hangar"
        };
        assert_eq!(
            roots.hangar_dir(),
            PathBuf::from("/tmp/jet-root").join(expected_leaf)
        );
    }

    #[test]
    fn explicit_root_keeps_the_custom_hangar_child() {
        let roots = Roots {
            root: PathBuf::from("/tmp/jet-root"),
            dev_mode: false,
        };
        assert_eq!(roots.hangar_dir(), PathBuf::from("/tmp/jet-root/hangar"));
    }

    #[test]
    fn isolated_root_requires_explicit_construction() {
        let roots = Roots::at(PathBuf::from("/explicit/jet-test-root"));
        assert_eq!(roots.root, PathBuf::from("/explicit/jet-test-root"));
        assert!(!roots.dev_mode);
        assert_eq!(
            roots.hangar_dir(),
            PathBuf::from("/explicit/jet-test-root/hangar")
        );
    }
}
