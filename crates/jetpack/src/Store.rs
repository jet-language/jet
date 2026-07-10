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
use std::process::{Child, Command, Stdio};
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

/// Validate then seal a locally produced output before it becomes reusable.
/// Files keep executable bits but lose all write bits; directories are sealed
/// bottom-up. Canonical archive validation rejects external hardlinks first.
pub fn seal_local_output(path: &Path) -> std::io::Result<()> {
    super::Envelope::try_output_hash_of(&path.to_string_lossy())
        .map_err(std::io::Error::other)?;
    seal_node(path)
}

fn seal_node(path: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            seal_node(&entry?.path())?;
        }
    }
    let mut permissions = meta.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        permissions.set_mode(permissions.mode() & !0o222);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions)
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
    /// JP0 cache identity. Legacy records have empty fields and can never hit.
    pub cache_identity: CacheIdentity,
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
            ("source_fingerprint", &self.cache_identity.source_fingerprint),
            ("recipe_fingerprint", &self.cache_identity.recipe_fingerprint),
            ("policy_fingerprint", &self.cache_identity.policy_fingerprint),
            ("identity_platform", &self.cache_identity.platform),
            ("realized_at", &realized_at),
            ("last_used_at", &last_used_at),
        ])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CacheIdentity {
    pub source_fingerprint: String,
    pub recipe_fingerprint: String,
    pub policy_fingerprint: String,
    pub platform: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheExpectation {
    pub identity: CacheIdentity,
    pub owned_output: Option<PathBuf>,
    pub allow_unsigned_local: bool,
}

pub enum RealizeRequest<'a> {
    Package {
        spec: &'a super::RefSpec::RefSpec,
        table: &'a super::RefSpec::SourceTable,
    },
    Adapter(&'a super::ModuleEval::AdapterPlan),
}

pub struct VerifiedRealization {
    entry: StoreEntry,
    source_state: super::Provider::SourceState,
    lease: CacheLease,
}

impl VerifiedRealization {
    pub fn metadata(&self) -> &StoreEntry {
        &self.entry
    }

    pub fn source_state(&self) -> super::Provider::SourceState {
        self.source_state
    }

    pub fn consumption_status(&self) -> &ConsumptionStatus {
        self.lease.status()
    }

    pub fn original_output(&self) -> &Path {
        self.lease.original_output()
    }

    pub fn original_reference(&self) -> &str {
        self.lease.original_reference()
    }

    pub(crate) fn into_parts(self) -> (StoreEntry, super::Provider::SourceState, CacheLease) {
        (self.entry, self.source_state, self.lease)
    }
}

#[derive(Debug)]
pub enum RealizeError {
    Provider(super::Provider::ProviderError),
    Store(std::io::Error),
    Integrity(IntegrityFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityFailure {
    pub package: String,
    pub version: String,
    pub expected: String,
    pub actual: String,
    pub reason: String,
    pub disposition: String,
    pub fix: String,
}

impl IntegrityFailure {
    pub fn what(&self) -> String {
        format!(
            "Integrity check failed for `{}` `{}` — expected `{}`, got `{}`.",
            self.package, self.version, self.expected, self.actual
        )
    }

    pub fn why(&self) -> String {
        format!(
            "The cached artifact failed {}. {}",
            self.reason, self.disposition
        )
    }

    pub fn fix(&self) -> &str {
        &self.fix
    }
}

pub fn report_integrity(theme: &super::Output::Theme, failure: &IntegrityFailure) {
    theme.error_coded(
        "E2604",
        &failure.what(),
        &failure.why(),
        failure.fix(),
    );
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
    record_verified(
        roots,
        name,
        version,
        reference,
        out,
        bin,
        rlib,
        envelope,
        &CacheIdentity::default(),
    )
}

pub fn record_verified(
    roots: &Roots,
    name: &str,
    version: &str,
    reference: &str,
    out: &str,
    bin: &str,
    rlib: &str,
    envelope: &super::Envelope::Envelope,
    cache_identity: &CacheIdentity,
) -> std::io::Result<StoreEntry> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
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
            cache_identity: cache_identity.clone(),
            realized_at,
            last_used_at: now,
        };
        fs::create_dir_all(&dir)?;
        pin_nix_gc_root(&dir, out)?;
        fs::write(dir.join("meta.json"), entry.meta_json())?;
        Ok(entry)
    })
}

const NIX_GC_ROOT: &str = "nix-gc-root";

/// Keep every live Nix compatibility output reachable until JP11 imports its
/// closure into Hangar. A root on the top-level output protects its transitive
/// Nix closure. Missing fixture paths are not roots and remain readable only as
/// metadata.
fn pin_nix_gc_root(entry_dir: &Path, out: &str) -> std::io::Result<()> {
    let out_path = Path::new(out);
    if !out_path.starts_with("/nix/store") || !out_path.exists() {
        return Ok(());
    }
    pin_nix_gc_root_with(entry_dir, out_path, Path::new("nix-store"))
}

/// Startup migration for records written before JP0. Every real Nix output is
/// rooted before any command may consume or clean Hangar state.
pub fn migrate_nix_gc_roots(roots: &Roots) -> std::io::Result<usize> {
    migrate_nix_gc_roots_with(roots, Path::new("/nix/store"), Path::new("nix-store"))
}

fn migrate_nix_gc_roots_with(
    roots: &Roots,
    store_prefix: &Path,
    nix_store: &Path,
) -> std::io::Result<usize> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        let mut rooted = 0;
        for entry in list(roots) {
            let out = Path::new(&entry.out);
            if !out.starts_with(store_prefix) || !out.exists() {
                continue;
            }
            let entry_dir = roots.hangar_dir().join(&entry.id);
            let root = entry_dir.join(NIX_GC_ROOT);
            if root.exists()
                && fs::canonicalize(&root).ok() == fs::canonicalize(out).ok()
            {
                continue;
            }
            if fs::symlink_metadata(&root).is_ok() {
                fs::remove_file(&root)?;
            }
            pin_nix_gc_root_with(&entry_dir, out, nix_store)?;
            rooted += 1;
        }
        Ok(rooted)
    })
}

fn pin_nix_gc_root_with(entry_dir: &Path, out: &Path, nix_store: &Path) -> std::io::Result<()> {
    let root = entry_dir.join(NIX_GC_ROOT);
    let output = Command::new(nix_store)
        .arg("--add-root")
        .arg(&root)
        .arg("--indirect")
        .arg("--realise")
        .arg(out)
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "could not create durable Nix GC root for `{}`: {}",
            out.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    if !root.exists() {
        return Err(std::io::Error::other(format!(
            "nix-store reported success but did not create GC root `{}`",
            root.display()
        )));
    }
    Ok(())
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

/// Proof attached to a cache reuse decision. Every field must pass; callers
/// must treat a partial proof as a miss. An unsigned artifact is accepted only
/// when the provider independently derives its exact Hangar-owned output. A
/// signed artifact must verify against the configured cache public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheVerification {
    pub output_exists: bool,
    pub output_digest: bool,
    pub source: bool,
    pub recipe: bool,
    pub platform: bool,
    pub policy: bool,
    pub signature_verified: bool,
    pub unsigned_local_allowed: bool,
    pub closure: bool,
}

impl CacheVerification {
    pub fn trusted(self) -> bool {
        self.output_exists
            && self.output_digest
            && self.source
            && self.recipe
            && self.platform
            && self.policy
            && (self.signature_verified || self.unsigned_local_allowed)
            && self.closure
    }
}

pub fn verify_cache_entry(
    roots: &Roots,
    entry: &StoreEntry,
    expected_reference: &str,
    expectation: &CacheExpectation,
) -> CacheVerification {
    let out = Path::new(&entry.out);
    let output_exists = out.exists();
    let output_digest = output_exists
        && !entry.envelope.output_hash.is_empty()
        && super::Envelope::try_output_hash_of(&entry.out)
            .is_ok_and(|hash| hash == entry.envelope.output_hash);
    let source = !expectation.identity.source_fingerprint.is_empty()
        && entry.cache_identity.source_fingerprint == expectation.identity.source_fingerprint;
    let recipe = !expectation.identity.recipe_fingerprint.is_empty()
        && entry.cache_identity.recipe_fingerprint == expectation.identity.recipe_fingerprint;
    let platform = entry.envelope.platform == expectation.identity.platform
        && entry.cache_identity.platform == expectation.identity.platform;
    let policy = entry.reference == expected_reference
        && !entry.envelope.provenance.is_empty()
        && !expectation.identity.policy_fingerprint.is_empty()
        && entry.cache_identity.policy_fingerprint == expectation.identity.policy_fingerprint;
    let signature_verified = !entry.envelope.signature.is_empty()
        && verify_configured_signature(roots, entry, expectation);
    let unsigned_local_allowed = entry.envelope.signature.is_empty()
        && expectation.allow_unsigned_local
        && expectation
            .owned_output
            .as_ref()
            .is_some_and(|path| path == Path::new(&entry.out));
    let closure = output_exists && closure_is_reachable(roots, entry);
    CacheVerification {
        output_exists,
        output_digest,
        source,
        recipe,
        platform,
        policy,
        signature_verified,
        unsigned_local_allowed,
        closure,
    }
}

pub struct VerifiedCacheHit {
    pub entry: StoreEntry,
    pub lease: CacheLease,
}

pub struct CacheLease {
    files: Vec<(PathBuf, fs::File)>,
    executables: Vec<(std::ffi::OsString, fs::File)>,
    out: PathBuf,
    snapshot_root: PathBuf,
    bin_relative: Option<PathBuf>,
    expected_digest: String,
    package: String,
    version: String,
    reference: String,
    status: ConsumptionStatus,
    wrapper_root: Option<PathBuf>,
    _wrapper_dir_handle: Option<fs::File>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumptionStatus {
    Consumable,
    NonConsumable { reason: String },
}

impl CacheLease {
    pub fn status(&self) -> &ConsumptionStatus {
        &self.status
    }

    pub fn wrapper_dir(&self) -> Option<&Path> {
        self.wrapper_root.as_deref()
    }

    pub fn original_output(&self) -> &Path {
        &self.out
    }

    pub fn original_reference(&self) -> &str {
        &self.reference
    }

    fn require_consumable(&self) -> std::io::Result<()> {
        match &self.status {
            ConsumptionStatus::Consumable => Ok(()),
            ConsumptionStatus::NonConsumable { reason } => {
                Err(std::io::Error::other(reason.clone()))
            }
        }
    }

    pub fn executable(&self, name: &str) -> Option<PathBuf> {
        self.require_consumable().ok()?;
        if name.contains(std::path::MAIN_SEPARATOR) {
            return None;
        }
        self.executables
            .iter()
            .find(|(member, _)| member == name)
            .map(|(_, file)| {
                #[cfg(target_os = "linux")]
                {
                    use std::os::fd::AsRawFd as _;
                    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let bin = self.bin_relative.as_ref()?;
                    self.snapshot_root.join(bin).join(name)
                }
            })
    }

    pub fn stable_path(&self, path: &str) -> std::io::Result<PathBuf> {
        self.require_consumable()?;
        let relative = Path::new(path).strip_prefix(&self.out).map_err(|_| {
            std::io::Error::other(format!(
                "consumer path `{path}` escapes leased output `{}`",
                self.out.display()
            ))
        })?;
        if relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(std::io::Error::other("leased consumer path contains parent traversal"));
        }
        if let Some((_, file)) = self.files.iter().find(|(member, _)| member == relative) {
            #[cfg(target_os = "linux")]
            {
                use std::os::fd::AsRawFd as _;
                return Ok(PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())));
            }
        }
        if relative.parent() == self.bin_relative.as_deref() {
            if let Some((_, file)) = self
                .executables
                .iter()
                .find(|(name, _)| Some(name.as_os_str()) == relative.file_name())
            {
                #[cfg(target_os = "linux")]
                {
                    use std::os::fd::AsRawFd as _;
                    return Ok(PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())));
                }
            }
        }
        Ok(self.snapshot_root.join(relative))
    }

    /// Revalidate immediately before handing paths to a child consumer. The
    /// archive reader uses no-follow handles and rejects concurrent mutation.
    pub fn validate(&self) -> std::io::Result<()> {
        self.require_consumable()?;
        let actual = super::Envelope::try_output_hash_of(&self.snapshot_root.to_string_lossy())
            .map_err(std::io::Error::other)?;
        if actual != self.expected_digest {
            return Err(std::io::Error::other(format!(
                "leased output changed: expected {}, got {actual}",
                self.expected_digest
            )));
        }
        #[cfg(target_os = "linux")]
        if let Some(wrapper) = &self.wrapper_root {
            let mut actual = fs::read_dir(wrapper)?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|entry| {
                    let target = fs::read_link(entry.path())?;
                    Ok((entry.file_name(), target))
                })
                .collect::<std::io::Result<Vec<_>>>()?;
            actual.sort_by(|left, right| left.0.cmp(&right.0));
            let mut expected = self
                .executables
                .iter()
                .map(|(name, file)| {
                    use std::os::fd::AsRawFd as _;
                    (name.clone(), PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())))
                })
                .collect::<Vec<_>>();
            expected.sort_by(|left, right| left.0.cmp(&right.0));
            if actual != expected {
                return Err(std::io::Error::other("lease executable wrappers changed"));
            }
        }
        Ok(())
    }

    pub fn integrity_failure(&self) -> Option<IntegrityFailure> {
        self.validate()
            .err()
            .map(|error| self.consumption_failure(&error))
    }

    pub(crate) fn consumption_failure(&self, error: &std::io::Error) -> IntegrityFailure {
        IntegrityFailure {
            package: self.package.clone(),
            version: self.version.clone(),
            expected: self.expected_digest.clone(),
            actual: error.to_string(),
            reason: "race-safe pre-consumer revalidation".to_string(),
            disposition: "Jetpack rejected it before handing any path to the consumer."
                .to_string(),
            fix: "Re-run `jet fetch` after `jet clean`. If the problem persists, audit the source before rebuilding."
                .to_string(),
        }
    }
}

impl Drop for CacheLease {
    fn drop(&mut self) {
        let _ = make_tree_writable_for_removal(&self.snapshot_root);
        let _ = fs::remove_dir_all(&self.snapshot_root);
    }
}

pub fn find_verified_by_reference(
    roots: &Roots,
    reference: &str,
    expectation: &CacheExpectation,
) -> std::io::Result<Option<VerifiedCacheHit>> {
    let _global = super::RuntimePolicy::acquire_lock(&roots.root, "hangar")?;
    let entry = list(roots)
        .into_iter()
        .filter(|entry| entry.reference == reference)
        .filter(|entry| verify_cache_entry(roots, entry, reference, expectation).trusted())
        .max_by_key(|entry| entry.last_used_at);
    let Some(entry) = entry else {
        return Ok(None);
    };
    let lease = snapshot_lease(roots, &entry)?;
    Ok(Some(VerifiedCacheHit { entry, lease }))
}

fn snapshot_lease(roots: &Roots, entry: &StoreEntry) -> std::io::Result<CacheLease> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    use std::sync::atomic::Ordering;

    let leases = roots.root.join("leases");
    fs::create_dir_all(&leases)?;
    let snapshot_root = leases.join(format!(
        "{}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed),
        entry.id
    ));
    if snapshot_root.exists() {
        make_tree_writable_for_removal(&snapshot_root)?;
        fs::remove_dir_all(&snapshot_root)?;
    }
    if !Path::new(&entry.out).exists() {
        fs::create_dir_all(&snapshot_root)?;
        return Ok(CacheLease {
            files: Vec::new(),
            executables: Vec::new(),
            out: PathBuf::from(&entry.out),
            snapshot_root,
            bin_relative: None,
            expected_digest: String::new(),
            package: entry.name.clone(),
            version: entry.version.clone(),
            reference: entry.reference.clone(),
            status: ConsumptionStatus::NonConsumable {
                reason: "realization has no canonical consumable output".to_string(),
            },
            wrapper_root: None,
            _wrapper_dir_handle: None,
        });
    }
    let mut hardlinks = BTreeMap::new();
    copy_snapshot_node(Path::new(&entry.out), &snapshot_root, &mut hardlinks)?;
    let digest = super::Envelope::try_output_hash_of(&snapshot_root.to_string_lossy())
        .map_err(std::io::Error::other)?;
    if digest != entry.envelope.output_hash {
        make_tree_writable_for_removal(&snapshot_root)?;
        fs::remove_dir_all(&snapshot_root)?;
        return Err(std::io::Error::other(format!(
            "private lease snapshot mismatch: expected {}, got {digest}",
            entry.envelope.output_hash
        )));
    }
    seal_local_output(&snapshot_root)?;
    let sealed_digest = super::Envelope::try_output_hash_of(&snapshot_root.to_string_lossy())
        .map_err(std::io::Error::other)?;
    let mut files = Vec::new();
    open_snapshot_files(&snapshot_root, &snapshot_root, &mut files)?;
    let bin_relative = (!entry.bin.is_empty())
        .then(|| Path::new(&entry.bin).strip_prefix(&entry.out).ok().map(PathBuf::from))
        .flatten();
    let executables = open_snapshot_executables(&snapshot_root, bin_relative.as_deref())?;
    let wrappers = create_exec_wrappers(&snapshot_root, &executables)?;
    Ok(CacheLease {
        files,
        executables,
        out: PathBuf::from(&entry.out),
        snapshot_root,
        bin_relative,
        expected_digest: sealed_digest,
        package: entry.name.clone(),
        version: entry.version.clone(),
        reference: entry.reference.clone(),
        status: ConsumptionStatus::Consumable,
        wrapper_root: wrappers.as_ref().map(|wrapper| wrapper.root.clone()),
        _wrapper_dir_handle: wrappers.map(|wrapper| wrapper.directory),
    })
}

struct ExecWrappers {
    root: PathBuf,
    directory: fs::File,
}

fn open_snapshot_executables(
    snapshot_root: &Path,
    bin: Option<&Path>,
) -> std::io::Result<Vec<(std::ffi::OsString, fs::File)>> {
    let Some(bin) = bin else {
        return Ok(Vec::new());
    };
    let path = snapshot_root.join(bin);
    let mut out = Vec::new();
    if !path.is_dir() {
        return Ok(out);
    }
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file = fs::File::open(entry.path())?;
        if !file.metadata()?.is_file() {
            continue;
        }
        clear_close_on_exec(&file)?;
        out.push((entry.file_name(), file));
    }
    Ok(out)
}

#[cfg(target_os = "linux")]
fn create_exec_wrappers(
    snapshot_root: &Path,
    executables: &[(std::ffi::OsString, fs::File)],
) -> std::io::Result<Option<ExecWrappers>> {
    use std::io::{BufRead as _, Write as _};
    use std::os::fd::AsRawFd as _;

    if executables.is_empty() {
        return Ok(None);
    }
    let mountpoint = snapshot_root.with_extension("exec-mount");
    fs::create_dir_all(&mountpoint)?;
    let unshare = find_system_tool("unshare")?;
    let mount = find_system_tool("mount")?;
    let shell = find_system_tool("sh")?;
    let script = r#"set -eu
"$1" -t tmpfs -o size=1m,mode=0755 jetpack-exec "$2"
printf 'ready\n'
IFS= read -r _
"$1" -o remount,bind,ro "$2"
printf 'sealed\n'
IFS= read -r _ || true
"#;
    let mut child = Command::new(unshare)
        .args(["--user", "--map-root-user", "--mount", "--propagation", "private"])
        .arg(shell)
        .args(["-c", script, "jetpack-lease-keeper"])
        .arg(mount)
        .arg(&mountpoint)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut input = child.stdin.take().expect("piped lease keeper stdin");
    let output = child.stdout.take().expect("piped lease keeper stdout");
    let mut output = std::io::BufReader::new(output);
    let mut line = String::new();
    output.read_line(&mut line)?;
    if line.trim() != "ready" {
        return Err(wrapper_keeper_error(child, "creating private executable mount"));
    }
    let root = PathBuf::from(format!("/proc/{}/root{}", child.id(), mountpoint.display()));
    for (name, file) in executables {
        std::os::unix::fs::symlink(
            format!("/proc/self/fd/{}", file.as_raw_fd()),
            root.join(name),
        )?;
    }
    input.write_all(b"seal\n")?;
    input.flush()?;
    line.clear();
    output.read_line(&mut line)?;
    if line.trim() != "sealed" {
        return Err(wrapper_keeper_error(child, "sealing private executable mount"));
    }
    let directory = fs::File::open(&root)?;
    clear_close_on_exec(&directory)?;
    let root = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    drop(input);
    let status = child.wait()?;
    if !status.success() {
        return Err(std::io::Error::other("private executable mount keeper failed"));
    }
    fs::remove_dir(&mountpoint)?;
    Ok(Some(ExecWrappers { root, directory }))
}

#[cfg(target_os = "linux")]
fn find_system_tool(name: &str) -> std::io::Result<PathBuf> {
    ["/run/current-system/sw/bin", "/usr/bin", "/bin"]
        .into_iter()
        .map(PathBuf::from)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, format!("required system tool `{name}` not found")))
}

#[cfg(target_os = "linux")]
fn wrapper_keeper_error(mut child: Child, action: &str) -> std::io::Error {
    use std::io::Read as _;
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    let _ = child.wait();
    std::io::Error::other(format!("{action} failed: {}", stderr.trim()))
}

#[cfg(not(target_os = "linux"))]
fn create_exec_wrappers(
    _snapshot_root: &Path,
    _executables: &[(std::ffi::OsString, fs::File)],
) -> std::io::Result<Option<ExecWrappers>> {
    Err(std::io::Error::other(
        "immutable executable PATH handoff is unavailable on this platform",
    ))
}

fn copy_snapshot_node(
    src: &Path,
    dst: &Path,
    hardlinks: &mut BTreeMap<(u64, u64), PathBuf>,
) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.is_dir() {
        fs::create_dir_all(dst)?;
        let mut entries = fs::read_dir(src)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_snapshot_node(&entry.path(), &dst.join(entry.file_name()), hardlinks)?;
        }
        fs::set_permissions(dst, meta.permissions())?;
    } else if meta.is_file() {
        if let Some(key) = snapshot_file_identity(&meta) {
            if let Some(first) = hardlinks.get(&key) {
                fs::hard_link(first, dst)?;
            } else {
                fs::copy(src, dst)?;
                hardlinks.insert(key, dst.to_path_buf());
            }
        } else {
            fs::copy(src, dst)?;
        }
        fs::set_permissions(dst, meta.permissions())?;
    } else if meta.file_type().is_symlink() {
        let target = fs::read_link(src)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, dst)?;
        #[cfg(not(unix))]
        return Err(std::io::Error::other("cache symlink snapshots need platform support"));
    } else {
        return Err(std::io::Error::other("special file in cache snapshot"));
    }
    Ok(())
}

#[cfg(unix)]
fn snapshot_file_identity(meta: &fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    (meta.nlink() > 1).then(|| (meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
fn snapshot_file_identity(_meta: &fs::Metadata) -> Option<(u64, u64)> {
    None
}

fn open_snapshot_files(
    root: &Path,
    path: &Path,
    files: &mut Vec<(PathBuf, fs::File)>,
) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            open_snapshot_files(root, &entry?.path(), files)?;
        }
        return Ok(());
    }
    let file = fs::File::open(path)?;
    clear_close_on_exec(&file)?;
    files.push((path.strip_prefix(root).unwrap().to_path_buf(), file));
    Ok(())
}

#[cfg(target_os = "linux")]
fn clear_close_on_exec(file: &fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd as _;
    const F_SETFD: i32 = 2;
    // Must match studio_transactions.rs: variadic fcntl (clashing_extern_declarations).
    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    if unsafe { fcntl(file.as_raw_fd(), F_SETFD, 0) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn clear_close_on_exec(_file: &fs::File) -> std::io::Result<()> {
    Ok(())
}

fn make_tree_writable_for_removal(path: &Path) -> std::io::Result<()> {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if meta.is_dir() {
        let mut permissions = meta.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
        for entry in fs::read_dir(path)? {
            make_tree_writable_for_removal(&entry?.path())?;
        }
    }
    Ok(())
}

/// Single realization boundary for every product consumer. Cache reuse,
/// quarantine, provider execution, and recording cannot be bypassed by CLI or
/// JetOS callers.
pub fn realize_verified(
    roots: &Roots,
    ctx: &super::Provider::Ctx<'_>,
    request: RealizeRequest<'_>,
) -> Result<VerifiedRealization, RealizeError> {
    let (reference, expectation) = match request {
        RealizeRequest::Package { spec, table } => (
            spec.raw.clone(),
            super::Provider::cache_expectation(spec, table, ctx),
        ),
        RealizeRequest::Adapter(plan) => (
            format!("adapt:{}:{}", plan.name, plan.source),
            Some(
                super::Provider::adapter_cache_expectation(plan, ctx)
                    .map_err(RealizeError::Provider)?,
            ),
        ),
    };

    if let (Some(candidate), Some(expectation)) =
        (find_by_reference(roots, &reference), expectation.as_ref())
    {
        match find_verified_by_reference(roots, &reference, expectation)
            .map_err(RealizeError::Store)?
        {
            Some(hit) => {
                return Ok(VerifiedRealization {
                    entry: hit.entry,
                    source_state: super::Provider::SourceState::Cached,
                    lease: hit.lease,
                });
            }
            None => {
                let proof = verify_cache_entry(roots, &candidate, &reference, expectation);
                let mut failure = integrity_failure(&candidate, expectation, proof);
                if let Err(error) = quarantine_invalid_entry(roots, &candidate, expectation) {
                    failure.actual = format!("{}; quarantine failed: {error}", failure.actual);
                }
                return Err(RealizeError::Integrity(failure));
            }
        }
    }

    let realized = match request {
        RealizeRequest::Package { spec, table } => {
            super::Provider::realize(spec, table, ctx).map_err(RealizeError::Provider)?
        }
        RealizeRequest::Adapter(plan) => {
            super::Provider::realize_adapter(plan, ctx).map_err(RealizeError::Provider)?
        }
    };
    let entry = record_verified(
        roots,
        &realized.name,
        &realized.version,
        &realized.reference,
        &realized.out,
        &realized.bin,
        &realized.rlib,
        &realized.envelope,
        &realized.cache_identity,
    )
    .map_err(RealizeError::Store)?;
    let lease = snapshot_lease(roots, &entry).map_err(RealizeError::Store)?;
    Ok(VerifiedRealization {
        entry,
        source_state: realized.source_state,
        lease,
    })
}

fn integrity_failure(
    entry: &StoreEntry,
    expectation: &CacheExpectation,
    proof: CacheVerification,
) -> IntegrityFailure {
    let (reason, expected, actual) = if !proof.output_exists {
        (
            "output existence",
            entry.envelope.output_hash.clone(),
            "missing output".to_string(),
        )
    } else if !proof.output_digest {
        (
            "content digest verification",
            entry.envelope.output_hash.clone(),
            super::Envelope::try_output_hash_of(&entry.out)
                .unwrap_or_else(|error| format!("unreadable output: {error}")),
        )
    } else if !proof.source {
        (
            "source identity verification",
            expectation.identity.source_fingerprint.clone(),
            entry.cache_identity.source_fingerprint.clone(),
        )
    } else if !proof.recipe {
        (
            "recipe identity verification",
            expectation.identity.recipe_fingerprint.clone(),
            entry.cache_identity.recipe_fingerprint.clone(),
        )
    } else if !proof.platform {
        (
            "platform verification",
            expectation.identity.platform.clone(),
            entry.cache_identity.platform.clone(),
        )
    } else if !proof.policy {
        (
            "policy identity verification",
            expectation.identity.policy_fingerprint.clone(),
            entry.cache_identity.policy_fingerprint.clone(),
        )
    } else if !proof.signature_verified && !proof.unsigned_local_allowed {
        (
            "signature verification",
            "a trusted cache signature or an exact Hangar-owned local output".to_string(),
            if entry.envelope.signature.is_empty() {
                "unsigned non-local artifact".to_string()
            } else {
                "untrusted signature".to_string()
            },
        )
    } else {
        (
            "closure containment verification",
            "canonical closure members strictly below the output root".to_string(),
            "closure member escaped or disappeared".to_string(),
        )
    };
    IntegrityFailure {
        package: entry.name.clone(),
        version: entry.version.clone(),
        expected,
        actual,
        reason: reason.to_string(),
        disposition: "Jetpack quarantined it instead of using or silently repairing it."
            .to_string(),
        fix: "Re-run `jet fetch` after `jet clean`. If the problem persists, audit the source before rebuilding."
            .to_string(),
    }
}

fn verify_configured_signature(
    _roots: &Roots,
    _entry: &StoreEntry,
    _expectation: &CacheExpectation,
) -> bool {
    // The generated crypto bridge lives in a user-writable build cache and is
    // not an immutable trust root. Until Jetpack ships an in-process vetted
    // verifier, signed cache imports fail closed. Never execute a mutable
    // helper to decide whether mutable bytes are trusted.
    false
}

#[cfg(test)]
fn verify_configured_signature_with(
    roots: &Roots,
    entry: &StoreEntry,
    expectation: &CacheExpectation,
    verifier: impl FnOnce(&str, &str, &str) -> bool,
) -> bool {
    let Ok(public_key) = fs::read_to_string(roots.root.join("trust/cache.ed25519.pub")) else {
        return false;
    };
    let signature = entry
        .envelope
        .signature
        .strip_prefix("ed25519:")
        .unwrap_or(&entry.envelope.signature);
    verifier(
        public_key.trim(),
        &cache_signature_message(entry, expectation),
        signature,
    )
}

#[cfg(test)]
fn cache_signature_message(entry: &StoreEntry, expectation: &CacheExpectation) -> String {
    format!(
        "jet-cache-v1\nreference={}\nsource={}\nrecipe={}\npolicy={}\nplatform={}\noutput={}\n",
        entry.reference,
        expectation.identity.source_fingerprint,
        expectation.identity.recipe_fingerprint,
        expectation.identity.policy_fingerprint,
        expectation.identity.platform,
        entry.envelope.output_hash,
    )
}

fn closure_is_reachable(roots: &Roots, entry: &StoreEntry) -> bool {
    let out = Path::new(&entry.out);
    if out.starts_with("/nix/store") {
        let root = roots.hangar_dir().join(&entry.id).join(NIX_GC_ROOT);
        return root.exists()
            && fs::canonicalize(&root).ok() == fs::canonicalize(out).ok();
    }
    let Ok(canonical_out) = fs::canonicalize(out) else {
        return false;
    };
    [&entry.bin, &entry.rlib].into_iter().all(|member| {
        if member.is_empty() {
            return true;
        }
        let member = Path::new(member);
        let Ok(canonical_member) = fs::canonicalize(member) else {
            return false;
        };
        canonical_member != canonical_out && canonical_member.starts_with(&canonical_out)
    })
}

/// Remove an invalid local cache candidate so provider realization cannot
/// mistake the same tampered directory for a fresh hit. Never removes external
/// outputs such as `/nix/store`; their provider must realize them again.
pub fn quarantine_invalid_entry(
    roots: &Roots,
    entry: &StoreEntry,
    expectation: &CacheExpectation,
) -> std::io::Result<()> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        let expected_id = entry_id(
            &entry.name,
            &entry.version,
            &entry.reference,
            &entry.out,
        );
        if entry.id != expected_id || Path::new(&entry.id).components().count() != 1 {
            return Err(std::io::Error::other("invalid cache record identity"));
        }
        let hangar = roots.hangar_dir();
        let quarantine = hangar.join("quarantine");
        fs::create_dir_all(&quarantine)?;
        let stamp = now_secs();
        let record = hangar.join(&entry.id);
        if fs::symlink_metadata(&record).is_ok() {
            fs::rename(&record, quarantine.join(format!("record-{}-{stamp}", entry.id)))?;
        }
        if let Some(owned) = &expectation.owned_output {
            if fs::symlink_metadata(owned).is_ok() {
                let canonical_hangar = fs::canonicalize(&hangar)?;
                let canonical_owned = fs::canonicalize(owned)?;
                if !owned.starts_with(&hangar) || !canonical_owned.starts_with(&canonical_hangar) {
                    return Err(std::io::Error::other(
                        "derived cache output escapes canonical Hangar root",
                    ));
                }
                let name = owned
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| std::io::Error::other("invalid owned output name"))?;
                fs::rename(owned, quarantine.join(format!("output-{name}-{stamp}")))?;
            }
        }
        Ok(())
    })
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
pub fn clean_plan(roots: &Roots) -> std::io::Result<CleanReport> {
    let store = roots.hangar_dir();
    if !store.exists() {
        return Ok(CleanReport::default());
    }
    let live = current_lock_roots();
    let mut report = sweep_build_scratch_plan(&store)?;
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
        report.removed_objects += 1;
        report.removed_bytes += dir_size(&path);
    }

    let opt = optimize_hangar_plan(&store)?;
    report.optimized_files += opt.optimized_files;
    report.optimized_bytes += opt.optimized_bytes;
    Ok(report)
}

pub fn clean(roots: &Roots) -> std::io::Result<CleanReport> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || clean_unlocked(roots))
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
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
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

fn sweep_build_scratch_plan(hangar: &Path) -> std::io::Result<CleanReport> {
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
        report.swept_tmp += 1;
        report.swept_tmp_bytes += dir_size(&path);
    }
    Ok(report)
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

fn optimize_hangar_plan(hangar: &Path) -> std::io::Result<CleanReport> {
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
            if seen.contains_key(&key) {
                report.optimized_files += 1;
                report.optimized_bytes += meta.len();
            } else {
                seen.insert(key, file);
            }
        }
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
    cache_identity: CacheIdentity,
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
        cache_identity: CacheIdentity {
            source_fingerprint: get("source_fingerprint").unwrap_or_default(),
            recipe_fingerprint: get("recipe_fingerprint").unwrap_or_default(),
            policy_fingerprint: get("policy_fingerprint").unwrap_or_default(),
            platform: get("identity_platform").unwrap_or_default(),
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

    fn test_identity() -> CacheIdentity {
        CacheIdentity {
            source_fingerprint: "source-v1".to_string(),
            recipe_fingerprint: "recipe-v1".to_string(),
            policy_fingerprint: "policy-v1".to_string(),
            platform: super::super::Envelope::host_platform(),
        }
    }

    fn test_expectation(out: &Path) -> CacheExpectation {
        CacheExpectation {
            identity: test_identity(),
            owned_output: Some(out.to_path_buf()),
            allow_unsigned_local: true,
        }
    }

    fn verified(roots: &Roots, reference: &str, expectation: &CacheExpectation) -> bool {
        find_verified_by_reference(roots, reference, expectation)
            .unwrap()
            .is_some()
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

    #[test]
    fn verified_cache_rejects_deleted_and_tampered_outputs() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("owned-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let envelope = super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "mine:demo",
            "core-source",
        );
        record_verified(
            &roots,
            "demo",
            "1.0",
            "mine:demo",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let expectation = test_expectation(&out);
        let entry = find_by_reference(&roots, "mine:demo").unwrap();
        let proof = verify_cache_entry(&roots, &entry, "mine:demo", &expectation);
        assert!(!proof.signature_verified);
        assert!(proof.unsigned_local_allowed);
        assert!(verified(&roots, "mine:demo", &expectation));

        fs::write(out.join("payload"), "tampered").unwrap();
        assert!(!verified(&roots, "mine:demo", &expectation));

        fs::remove_dir_all(&out).unwrap();
        assert!(!verified(&roots, "mine:demo", &expectation));
    }

    #[test]
    fn verified_cache_rejects_wrong_platform_and_incomplete_proof() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("owned-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let mut envelope = super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "mine:demo",
            "core-source",
        );
        envelope.platform = "not-this-host".to_string();
        record_verified(
            &roots,
            "demo",
            "1.0",
            "mine:demo",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let mut expectation = test_expectation(&out);
        assert!(!verified(&roots, "mine:demo", &expectation));

        envelope.platform = super::super::Envelope::host_platform();
        envelope.provenance.clear();
        record_verified(
            &roots,
            "demo",
            "1.0",
            "mine:demo",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        assert!(!verified(&roots, "mine:demo", &expectation));

        envelope.provenance = "mine:demo via core-source".to_string();
        envelope.signature = "unverified-signature-text".to_string();
        record_verified(
            &roots,
            "demo",
            "1.0",
            "mine:demo",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        assert!(!verified(&roots, "mine:demo", &expectation));

        envelope.signature.clear();
        record_verified(
            &roots,
            "demo",
            "1.0",
            "mine:demo",
            &out.to_string_lossy(),
            &out.join("missing-bin").to_string_lossy(),
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        expectation.owned_output = Some(out.clone());
        assert!(!verified(&roots, "mine:demo", &expectation));
    }

    #[test]
    fn closure_rejects_parent_traversal_to_sibling() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("owned-output");
        let sibling = roots.root.join("other");
        fs::create_dir_all(&out).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        fs::write(sibling.join("tool"), "outside").unwrap();
        let envelope = super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "mine:escape",
            "core-source",
        );
        let mut entry = record_verified(
            &roots,
            "escape",
            "1",
            "mine:escape",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        entry.bin = out.join("../other").to_string_lossy().into_owned();
        let proof = verify_cache_entry(
            &roots,
            &entry,
            "mine:escape",
            &test_expectation(&out),
        );
        assert!(!proof.closure);
        assert!(!proof.trusted());
    }

    #[cfg(unix)]
    #[test]
    fn nix_compat_output_gets_durable_gc_root() {
        use std::os::unix::fs::PermissionsExt as _;

        let (roots, _g) = temp_roots();
        let entry = roots.root.join("entry");
        let out = roots.root.join("fake-nix-output");
        let helper = roots.root.join("fake-nix-store");
        fs::create_dir_all(&entry).unwrap();
        fs::create_dir_all(&out).unwrap();
        fs::write(&helper, "#!/bin/sh\nln -s \"$5\" \"$2\"\n").unwrap();
        let mut perms = fs::metadata(&helper).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&helper, perms).unwrap();

        pin_nix_gc_root_with(&entry, &out, &helper).unwrap();
        assert_eq!(fs::read_link(entry.join(NIX_GC_ROOT)).unwrap(), out);
    }

    #[cfg(unix)]
    #[test]
    fn startup_migration_roots_existing_real_paths() {
        use std::os::unix::fs::PermissionsExt as _;

        let (roots, _g) = temp_roots();
        let prefix = roots.root.join("nix/store");
        let out = prefix.join("abc-demo");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "demo").unwrap();
        let envelope = super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "nixpkgs:demo",
            "nix",
        );
        let entry = record(
            &roots,
            "demo",
            "1.0",
            "nixpkgs:demo",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
        )
        .unwrap();
        let helper = roots.root.join("fake-nix-store-migrate");
        fs::write(&helper, "#!/bin/sh\nln -s \"$5\" \"$2\"\n").unwrap();
        let mut perms = fs::metadata(&helper).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&helper, perms).unwrap();

        assert_eq!(migrate_nix_gc_roots_with(&roots, &prefix, &helper).unwrap(), 1);
        let root = roots.hangar_dir().join(entry.id).join(NIX_GC_ROOT);
        assert_eq!(fs::canonicalize(root).unwrap(), fs::canonicalize(out).unwrap());
    }

    #[test]
    fn hostile_out_pointer_never_quarantines_another_object() {
        let (roots, _g) = temp_roots();
        let survivor = roots.hangar_dir().join("survivor-output");
        let expected = roots.hangar_dir().join("expected-output");
        fs::create_dir_all(&survivor).unwrap();
        fs::create_dir_all(&expected).unwrap();
        fs::write(survivor.join("keep"), "survivor").unwrap();
        fs::write(expected.join("bad"), "candidate").unwrap();
        let envelope = super::super::Envelope::Envelope::for_output(
            &survivor.to_string_lossy(),
            "mine:hostile",
            "core-source",
        );
        let entry = record_verified(
            &roots,
            "hostile",
            "1.0",
            "mine:hostile",
            &survivor.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let expectation = CacheExpectation {
            identity: test_identity(),
            owned_output: Some(expected.clone()),
            allow_unsigned_local: true,
        };

        quarantine_invalid_entry(&roots, &entry, &expectation).unwrap();
        assert_eq!(fs::read_to_string(survivor.join("keep")).unwrap(), "survivor");
        assert!(!expected.exists());
    }

    #[test]
    fn cache_lease_is_private_snapshot_without_long_object_lock() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("leased-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let envelope = super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "mine:leased",
            "core-source",
        );
        record_verified(
            &roots,
            "leased",
            "1.0",
            "mine:leased",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let hit = find_verified_by_reference(
            &roots,
            "mine:leased",
            &test_expectation(&out),
        )
        .unwrap()
        .unwrap();
        fs::write(out.join("payload"), "mutated outside cooperative lock").unwrap();
        hit.lease.validate().unwrap();
        let stable = hit
            .lease
            .stable_path(&out.join("payload").to_string_lossy())
            .unwrap();
        assert_eq!(fs::read_to_string(stable).unwrap(), "trusted");
    }

    #[test]
    fn realization_type_distinguishes_fresh_cached_and_missing_outputs() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("typed-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let envelope = super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "mine:typed",
            "core-source",
        );
        let entry = record_verified(
            &roots,
            "typed",
            "1",
            "mine:typed",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();

        let fresh = VerifiedRealization {
            lease: snapshot_lease(&roots, &entry).unwrap(),
            entry: entry.clone(),
            source_state: super::super::Provider::SourceState::Built,
        };
        assert_eq!(fresh.consumption_status(), &ConsumptionStatus::Consumable);
        assert!(fresh
            .lease
            .stable_path(&out.join("payload").to_string_lossy())
            .is_ok());

        let hit = find_verified_by_reference(&roots, "mine:typed", &test_expectation(&out))
            .unwrap()
            .unwrap();
        let cached = VerifiedRealization {
            entry: hit.entry,
            source_state: super::super::Provider::SourceState::Cached,
            lease: hit.lease,
        };
        assert_eq!(cached.consumption_status(), &ConsumptionStatus::Consumable);

        let missing_out = roots.root.join("missing-output");
        let missing_entry = StoreEntry {
            id: "missing-1-test".to_string(),
            name: "missing".to_string(),
            version: "1".to_string(),
            reference: "mine:missing".to_string(),
            out: missing_out.to_string_lossy().into_owned(),
            bin: String::new(),
            rlib: String::new(),
            envelope: super::super::Envelope::Envelope::default(),
            cache_identity: test_identity(),
            realized_at: 0,
            last_used_at: 0,
        };
        let missing = VerifiedRealization {
            lease: snapshot_lease(&roots, &missing_entry).unwrap(),
            entry: missing_entry,
            source_state: super::super::Provider::SourceState::Built,
        };
        assert!(matches!(
            missing.consumption_status(),
            ConsumptionStatus::NonConsumable { .. }
        ));
        assert!(missing.lease.stable_path(&missing_out.to_string_lossy()).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn leased_fd_view_executes_original_after_rename_symlink_swap() {
        use std::os::unix::fs::{symlink, PermissionsExt as _};

        let (roots, _g) = temp_roots();
        let out = roots.root.join("fd-view-output");
        let tool = out.join("bin/tool");
        fs::create_dir_all(tool.parent().unwrap()).unwrap();
        fs::write(out.join("bin/tool-real"), "#!/bin/sh\nprintf trusted").unwrap();
        fs::set_permissions(
            out.join("bin/tool-real"),
            fs::Permissions::from_mode(0o555),
        )
        .unwrap();
        symlink("tool-real", &tool).unwrap();
        let envelope = super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "mine:fd-view",
            "core-source",
        );
        record_verified(
            &roots,
            "fd-view",
            "1",
            "mine:fd-view",
            &out.to_string_lossy(),
            &out.join("bin").to_string_lossy(),
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let hit = find_verified_by_reference(
            &roots,
            "mine:fd-view",
            &test_expectation(&out),
        )
        .unwrap()
        .unwrap();
        let stable_tool = hit.lease.stable_path(&tool.to_string_lossy()).unwrap();

        let moved = roots.root.join("fd-view-original");
        let attacker = roots.root.join("fd-view-attacker");
        fs::rename(&out, &moved).unwrap();
        fs::create_dir_all(attacker.join("bin")).unwrap();
        fs::write(attacker.join("bin/tool"), "#!/bin/sh\nprintf attacker").unwrap();
        fs::set_permissions(
            attacker.join("bin/tool"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        symlink(&attacker, &out).unwrap();

        hit.lease.validate().unwrap();
        let wrapper = hit.lease.wrapper_dir().unwrap();
        assert!(fs::set_permissions(wrapper, fs::Permissions::from_mode(0o755)).is_err());
        assert!(fs::remove_file(wrapper.join("tool")).is_err());
        hit.lease.validate().unwrap();
        make_tree_writable_for_removal(&hit.lease.snapshot_root).unwrap();
        let snapshot_tool = hit.lease.snapshot_root.join("bin/tool");
        fs::remove_file(&snapshot_tool).unwrap();
        fs::write(&snapshot_tool, "#!/bin/sh\nprintf snapshot-attacker").unwrap();
        fs::set_permissions(&snapshot_tool, fs::Permissions::from_mode(0o755)).unwrap();

        let output = Command::new(&stable_tool).output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "trusted");
        let nested = Command::new("/bin/sh")
            .args(["-c", "exec tool"])
            .env(
                "PATH",
                format!(
                    "{}:/usr/bin:/bin",
                    hit.lease.wrapper_dir().unwrap().display()
                ),
            )
            .output()
            .unwrap();
        assert!(nested.status.success());
        assert_eq!(String::from_utf8(nested.stdout).unwrap(), "trusted");
        assert_eq!(fs::read_to_string(out.join("bin/tool")).unwrap(), "#!/bin/sh\nprintf attacker");
    }

    #[test]
    fn configured_key_is_required_before_signature_verifier_runs() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("signed-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "signed").unwrap();
        let mut envelope = super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "cache:demo",
            "remote-cache",
        );
        envelope.signature = "ed25519:abcd".to_string();
        let entry = StoreEntry {
            id: entry_id("demo", "1", "cache:demo", &out.to_string_lossy()),
            name: "demo".to_string(),
            version: "1".to_string(),
            reference: "cache:demo".to_string(),
            out: out.to_string_lossy().into_owned(),
            bin: String::new(),
            rlib: String::new(),
            envelope,
            cache_identity: test_identity(),
            realized_at: 0,
            last_used_at: 0,
        };
        let expectation = CacheExpectation {
            identity: test_identity(),
            owned_output: None,
            allow_unsigned_local: false,
        };
        let mut called = false;
        assert!(!verify_configured_signature_with(
            &roots,
            &entry,
            &expectation,
            |_, _, _| {
                called = true;
                true
            }
        ));
        assert!(!called);

        fs::create_dir_all(roots.root.join("trust")).unwrap();
        fs::write(roots.root.join("trust/cache.ed25519.pub"), "public-key").unwrap();
        assert!(verify_configured_signature_with(
            &roots,
            &entry,
            &expectation,
            |key, message, signature| {
                key == "public-key"
                    && message.contains("source=source-v1")
                    && signature == "abcd"
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn mutable_replaced_crypto_helper_is_never_a_trust_root() {
        use std::os::unix::fs::PermissionsExt as _;

        let (roots, _g) = temp_roots();
        let out = roots.root.join("signed-output-hostile");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "signed").unwrap();
        fs::create_dir_all(roots.root.join("trust")).unwrap();
        fs::write(roots.root.join("trust/cache.ed25519.pub"), "attacker-key").unwrap();
        let marker = roots.root.join("helper-ran");
        let helper = roots.root.join("trust/jet-crypto-helper");
        fs::write(
            &helper,
            format!("#!/bin/sh\ntouch '{}'\nexit 0\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
        let mut envelope = super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "cache:hostile",
            "remote-cache",
        );
        envelope.signature = "ed25519:forged".to_string();
        let entry = StoreEntry {
            id: entry_id("hostile", "1", "cache:hostile", &out.to_string_lossy()),
            name: "hostile".to_string(),
            version: "1".to_string(),
            reference: "cache:hostile".to_string(),
            out: out.to_string_lossy().into_owned(),
            bin: String::new(),
            rlib: String::new(),
            envelope,
            cache_identity: test_identity(),
            realized_at: 0,
            last_used_at: 0,
        };
        let expectation = CacheExpectation {
            identity: test_identity(),
            owned_output: None,
            allow_unsigned_local: false,
        };
        let proof = verify_cache_entry(&roots, &entry, "cache:hostile", &expectation);
        assert!(!proof.signature_verified);
        assert!(!proof.trusted());
        assert!(!marker.exists(), "mutable helper must never execute");
    }

    #[test]
    fn e2604_reason_snapshot_is_exact() {
        let failure = IntegrityFailure {
            package: "demo".to_string(),
            version: "1.2.3".to_string(),
            expected: "sha256-good".to_string(),
            actual: "sha256-bad".to_string(),
            reason: "content digest verification".to_string(),
            disposition: "Jetpack quarantined it instead of using or silently repairing it."
                .to_string(),
            fix: "Re-run `jet fetch` after `jet clean`. If the problem persists, audit the source before rebuilding."
                .to_string(),
        };
        assert_eq!(
            failure.what(),
            "Integrity check failed for `demo` `1.2.3` — expected `sha256-good`, got `sha256-bad`."
        );
        assert_eq!(
            failure.why(),
            "The cached artifact failed content digest verification. Jetpack quarantined it instead of using or silently repairing it."
        );
        assert_eq!(
            failure.fix(),
            "Re-run `jet fetch` after `jet clean`. If the problem persists, audit the source before rebuilding."
        );
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
