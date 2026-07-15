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

use crate::JSON::{self, Json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The subdir of the resolved root that holds the content-addressed store.
/// Mirrors the trailing segment of the historical `Syntax::HANGAR_DIR`.
const HANGAR_SUBDIR: &str = "hangar";

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
        field("platform_artifact_kind", &self.platform_artifact_kind, false);
        field("producer_record", &self.producer_record, false);
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
                cache_identity: parsed.cache_identity,
                references: parsed.references,
                named_outputs: parsed.named_outputs,
                platform_artifact_kind: parsed.platform_artifact_kind,
                producer_record: parsed.producer_record,
                realized_at: parsed.realized_at.unwrap_or(0),
                last_used_at: parsed.last_used_at.unwrap_or(0),
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
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
    pub realized_at: Option<u64>,
    pub last_used_at: Option<u64>,
}

pub fn parse_meta(text: &str) -> Option<ParsedMeta> {
    let j = JSON::parse(text).ok()?;
    let get = |k: &str| j.get(k).and_then(Json::as_str).map(str::to_string).ok();
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
        realized_at: get("realized_at").and_then(|s| s.parse().ok()),
        last_used_at: get("last_used_at").and_then(|s| s.parse().ok()),
    })
}
