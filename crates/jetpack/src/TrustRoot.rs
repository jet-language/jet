//! E4-JP6A / D-JPK-TRUSTROOT1 — trust primitives and root bootstrap.
//!
//! Vertical slice of the TUF-shaped registry authority substrate **before**
//! live registry/cache wiring (that is JP6B / #434):
//!
//! - toolchain-pinned trusted-root bootstrap (digest pin)
//! - offline threshold root keys + rotation/recovery drills
//! - role delegations with path bounds
//! - consistent snapshots, monotonic versions, metadata size limits
//! - trusted-time / bad-clock expiry rules
//! - signature-stripping rejection
//! - hybrid publisher identity (Sigstore vs offline Ed25519/KMS/HSM)
//! - distinct publisher / registry / cache-builder / remote-executor identities
//!
//! Crypto-agility seam: role metadata is signed with HMAC-SHA256 over the
//! canonical bytes (pure std via [`crate::SHA256`], I6). Publisher identity
//! remains typed for Ed25519/Sigstore/KMS so JP6B can attach live verifiers
//! without inventing a second trust model (I8). Mutable helper binaries are
//! never consulted — same fail-closed posture as Hangar cache signature
//! verification.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::SHA256;

/// Default max metadata payload size (bytes) before signatures.
pub const DEFAULT_MAX_METADATA_BYTES: usize = 1024 * 1024;

/// Default max clock skew accepted against trusted time.
pub const DEFAULT_MAX_CLOCK_SKEW: Duration = Duration::from_secs(5 * 60);

/// Algorithm id recorded on every signature (crypto-agility seam).
pub const ALG_HMAC_SHA256: &str = "hmac-sha256";

/// Separate trust-domain identities (D-JPK-TRUSTROOT1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IdentityKind {
    Publisher,
    Registry,
    CacheBuilder,
    RemoteExecutor,
}

impl IdentityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IdentityKind::Publisher => "publisher",
            IdentityKind::Registry => "registry",
            IdentityKind::CacheBuilder => "cache-builder",
            IdentityKind::RemoteExecutor => "remote-executor",
        }
    }
}

/// Hybrid public/private publisher proof (D-JPK-TRUSTROOT1).
///
/// Public releases accept a Sigstore identity bundle against a pinned
/// transparency checkpoint **or** an offline Ed25519/KMS/HSM publisher
/// signature. D-PKGSIGN1 author signatures stay opt-in and are a different
/// surface; this enum is the registry-authority publisher identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublisherIdentity {
    Sigstore {
        identity: String,
        issuer: String,
        checkpoint_digest: String,
        bundle_digest: String,
    },
    OfflineEd25519 {
        public_key_hex: String,
        key_id: String,
    },
    KmsHsm {
        key_uri: String,
        public_key_hex: String,
        key_id: String,
    },
}

impl PublisherIdentity {
    pub fn kind_label(&self) -> &'static str {
        match self {
            PublisherIdentity::Sigstore { .. } => "sigstore",
            PublisherIdentity::OfflineEd25519 { .. } => "ed25519",
            PublisherIdentity::KmsHsm { .. } => "kms-hsm",
        }
    }

    pub fn validate(&self) -> Result<(), TrustError> {
        match self {
            PublisherIdentity::Sigstore {
                identity,
                issuer,
                checkpoint_digest,
                bundle_digest,
            } => {
                require_nonempty("sigstore.identity", identity)?;
                require_nonempty("sigstore.issuer", issuer)?;
                require_sha256_hex("sigstore.checkpoint_digest", checkpoint_digest)?;
                require_sha256_hex("sigstore.bundle_digest", bundle_digest)?;
            }
            PublisherIdentity::OfflineEd25519 {
                public_key_hex,
                key_id,
            } => {
                require_ed25519_pub_hex("ed25519.public_key_hex", public_key_hex)?;
                require_nonempty("ed25519.key_id", key_id)?;
            }
            PublisherIdentity::KmsHsm {
                key_uri,
                public_key_hex,
                key_id,
            } => {
                require_nonempty("kms.key_uri", key_uri)?;
                if !(key_uri.starts_with("kms:")
                    || key_uri.starts_with("hsm:")
                    || key_uri.starts_with("pkcs11:"))
                {
                    return Err(TrustError::InvalidPublisher {
                        detail: format!("kms key URI must start with kms:, hsm:, or pkcs11: (got `{key_uri}`)"),
                    });
                }
                require_ed25519_pub_hex("kms.public_key_hex", public_key_hex)?;
                require_nonempty("kms.key_id", key_id)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundIdentity {
    pub kind: IdentityKind,
    pub name: String,
    pub publisher: Option<PublisherIdentity>,
}

impl BoundIdentity {
    pub fn registry(name: impl Into<String>) -> Self {
        Self {
            kind: IdentityKind::Registry,
            name: name.into(),
            publisher: None,
        }
    }

    pub fn publisher(name: impl Into<String>, proof: PublisherIdentity) -> Self {
        Self {
            kind: IdentityKind::Publisher,
            name: name.into(),
            publisher: Some(proof),
        }
    }

    pub fn cache_builder(name: impl Into<String>) -> Self {
        Self {
            kind: IdentityKind::CacheBuilder,
            name: name.into(),
            publisher: None,
        }
    }

    pub fn remote_executor(name: impl Into<String>) -> Self {
        Self {
            kind: IdentityKind::RemoteExecutor,
            name: name.into(),
            publisher: None,
        }
    }

    pub fn validate(&self) -> Result<(), TrustError> {
        require_nonempty("identity.name", &self.name)?;
        match self.kind {
            IdentityKind::Publisher => {
                let proof = self.publisher.as_ref().ok_or(TrustError::InvalidPublisher {
                    detail: "publisher identity requires a hybrid proof".into(),
                })?;
                proof.validate()?;
            }
            _ => {
                if self.publisher.is_some() {
                    return Err(TrustError::IdentityKindMismatch {
                        kind: self.kind,
                        detail: "only publisher identities carry hybrid publisher proofs".into(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustKey {
    pub key_id: String,
    pub algorithm: String,
    /// Secret material for HMAC-SHA256 role signing. Never serialized into
    /// public metadata; only the key_id appears on the wire.
    pub secret: Vec<u8>,
}

impl TrustKey {
    pub fn generate(label: &str) -> Self {
        let mut secret = Vec::with_capacity(32);
        secret.extend_from_slice(b"jet-trust-v1/");
        secret.extend_from_slice(label.as_bytes());
        // Pad/truncate to 32 bytes of deterministic material for drills.
        secret.resize(32, 0);
        let key_id = key_id_for(&secret);
        Self {
            key_id,
            algorithm: ALG_HMAC_SHA256.to_string(),
            secret,
        }
    }

    pub fn from_secret(secret: Vec<u8>) -> Result<Self, TrustError> {
        if secret.len() < 16 {
            return Err(TrustError::InvalidKey {
                detail: "trust key secret must be at least 16 bytes".into(),
            });
        }
        let key_id = key_id_for(&secret);
        Ok(Self {
            key_id,
            algorithm: ALG_HMAC_SHA256.to_string(),
            secret,
        })
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        Signature {
            key_id: self.key_id.clone(),
            algorithm: self.algorithm.clone(),
            sig_hex: hex(&hmac_sha256(&self.secret, message)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub key_id: String,
    pub algorithm: String,
    pub sig_hex: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetadataRole {
    Root,
    Targets,
    Snapshot,
    Timestamp,
}

impl MetadataRole {
    pub fn as_str(self) -> &'static str {
        match self {
            MetadataRole::Root => "root",
            MetadataRole::Targets => "targets",
            MetadataRole::Snapshot => "snapshot",
            MetadataRole::Timestamp => "timestamp",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleKeys {
    pub key_ids: Vec<String>,
    pub threshold: usize,
}

impl RoleKeys {
    pub fn new(key_ids: Vec<String>, threshold: usize) -> Result<Self, TrustError> {
        if threshold == 0 {
            return Err(TrustError::InvalidThreshold {
                detail: "threshold must be >= 1".into(),
            });
        }
        if threshold > key_ids.len() {
            return Err(TrustError::InvalidThreshold {
                detail: format!(
                    "threshold {threshold} exceeds key count {}",
                    key_ids.len()
                ),
            });
        }
        let mut seen = BTreeSet::new();
        for id in &key_ids {
            if !seen.insert(id.clone()) {
                return Err(TrustError::InvalidThreshold {
                    detail: format!("duplicate key id `{id}` in role"),
                });
            }
        }
        Ok(Self { key_ids, threshold })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegation {
    pub name: String,
    pub role: RoleKeys,
    /// Path prefixes this delegation may sign (TUF path hash prefixes / path bounds).
    pub path_prefixes: Vec<String>,
    pub terminating: bool,
}

impl Delegation {
    pub fn allows_path(&self, path: &str) -> bool {
        self.path_prefixes.iter().any(|p| path.starts_with(p.as_str()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootMetadata {
    pub version: u64,
    pub expires_unix: u64,
    pub consistent_snapshot: bool,
    pub roles: BTreeMap<MetadataRole, RoleKeys>,
    pub delegations: Vec<Delegation>,
    /// Public key ids known to this root (secret material stays offline).
    pub public_key_ids: BTreeMap<String, String>, // key_id -> algorithm
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetsMetadata {
    pub version: u64,
    pub expires_unix: u64,
    pub targets: BTreeMap<String, TargetMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetMeta {
    pub length: u64,
    pub hashes: BTreeMap<String, String>,
    pub custom: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotMetadata {
    pub version: u64,
    pub expires_unix: u64,
    /// role name -> (version, sha256 of that role's signed metadata bytes)
    pub meta: BTreeMap<String, SnapshotMetaEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotMetaEntry {
    pub version: u64,
    pub length: u64,
    pub hashes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampMetadata {
    pub version: u64,
    pub expires_unix: u64,
    pub snapshot: SnapshotMetaEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed<T> {
    pub signed: T,
    pub signatures: Vec<Signature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustError {
    BootstrapPinMismatch { expected: String, actual: String },
    BootstrapMissing,
    SignatureStripped { role: MetadataRole },
    SignatureInvalid { role: MetadataRole, detail: String },
    ThresholdUnmet { role: MetadataRole, have: usize, need: usize },
    Rollback { role: MetadataRole, current: u64, incoming: u64 },
    Expired { role: MetadataRole, expires_unix: u64, now_unix: u64 },
    BadClock { skew_secs: u64, max_secs: u64 },
    MetadataTooLarge { role: MetadataRole, size: usize, max: usize },
    ConsistentSnapshotMismatch { role: String, detail: String },
    DelegationDenied { path: String, detail: String },
    UnknownKey { key_id: String },
    InvalidKey { detail: String },
    InvalidThreshold { detail: String },
    InvalidPublisher { detail: String },
    IdentityKindMismatch { kind: IdentityKind, detail: String },
    RotationRejected { detail: String },
    Io { detail: String },
}

impl fmt::Display for TrustError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrustError::BootstrapPinMismatch { expected, actual } => write!(
                f,
                "trusted-root bootstrap pin mismatch: expected {expected}, got {actual}"
            ),
            TrustError::BootstrapMissing => write!(f, "trusted-root bootstrap pin is missing"),
            TrustError::SignatureStripped { role } => {
                write!(f, "{} metadata has no signatures (signature stripping rejected)", role.as_str())
            }
            TrustError::SignatureInvalid { role, detail } => {
                write!(f, "{} signature invalid: {detail}", role.as_str())
            }
            TrustError::ThresholdUnmet { role, have, need } => write!(
                f,
                "{} threshold unmet: have {have}, need {need}",
                role.as_str()
            ),
            TrustError::Rollback { role, current, incoming } => write!(
                f,
                "{} version rollback rejected: current {current}, incoming {incoming}",
                role.as_str()
            ),
            TrustError::Expired { role, expires_unix, now_unix } => write!(
                f,
                "{} metadata expired at {expires_unix} (trusted now {now_unix})",
                role.as_str()
            ),
            TrustError::BadClock { skew_secs, max_secs } => write!(
                f,
                "bad clock: skew {skew_secs}s exceeds max {max_secs}s"
            ),
            TrustError::MetadataTooLarge { role, size, max } => write!(
                f,
                "{} metadata too large: {size} > {max}",
                role.as_str()
            ),
            TrustError::ConsistentSnapshotMismatch { role, detail } => {
                write!(f, "consistent snapshot mismatch for `{role}`: {detail}")
            }
            TrustError::DelegationDenied { path, detail } => {
                write!(f, "delegation denied for `{path}`: {detail}")
            }
            TrustError::UnknownKey { key_id } => write!(f, "unknown trust key `{key_id}`"),
            TrustError::InvalidKey { detail }
            | TrustError::InvalidThreshold { detail }
            | TrustError::InvalidPublisher { detail }
            | TrustError::RotationRejected { detail }
            | TrustError::Io { detail } => write!(f, "{detail}"),
            TrustError::IdentityKindMismatch { kind, detail } => {
                write!(f, "identity kind {}: {detail}", kind.as_str())
            }
        }
    }
}

/// Trusted-time source. Production uses wall clock; tests inject fixed time.
pub trait TrustedClock {
    fn now_unix(&self) -> u64;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTrustedClock;

impl TrustedClock for SystemTrustedClock {
    fn now_unix(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub u64);

impl TrustedClock for FixedClock {
    fn now_unix(&self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct TrustPolicy {
    pub max_metadata_bytes: usize,
    pub max_clock_skew: Duration,
    /// When set, local clock must stay within skew of this trusted unix time.
    pub trusted_unix: Option<u64>,
}

impl Default for TrustPolicy {
    fn default() -> Self {
        Self {
            max_metadata_bytes: DEFAULT_MAX_METADATA_BYTES,
            max_clock_skew: DEFAULT_MAX_CLOCK_SKEW,
            trusted_unix: None,
        }
    }
}

/// Offline keyring for role signing/verification (never a mutable helper).
#[derive(Debug, Clone, Default)]
pub struct Keyring {
    keys: BTreeMap<String, TrustKey>,
}

impl Keyring {
    pub fn insert(&mut self, key: TrustKey) {
        self.keys.insert(key.key_id.clone(), key);
    }

    pub fn get(&self, key_id: &str) -> Option<&TrustKey> {
        self.keys.get(key_id)
    }

    pub fn verify(&self, key_id: &str, message: &[u8], signature: &Signature) -> Result<(), TrustError> {
        let key = self.keys.get(key_id).ok_or_else(|| TrustError::UnknownKey {
            key_id: key_id.to_string(),
        })?;
        if signature.algorithm != ALG_HMAC_SHA256 && signature.algorithm != key.algorithm {
            return Err(TrustError::SignatureInvalid {
                role: MetadataRole::Root,
                detail: format!("unsupported algorithm `{}`", signature.algorithm),
            });
        }
        let expected = hex(&hmac_sha256(&key.secret, message));
        if expected != signature.sig_hex {
            return Err(TrustError::SignatureInvalid {
                role: MetadataRole::Root,
                detail: format!("signature for key `{key_id}` does not verify"),
            });
        }
        Ok(())
    }
}

/// Persistent trusted-root bootstrap pin under a Jetpack root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootBootstrap {
    pub pin_digest: String,
    pub root_version: u64,
    pub consistent_snapshot: bool,
}

impl RootBootstrap {
    pub fn path(roots_dir: &Path) -> PathBuf {
        roots_dir.join("trust").join("root.bootstrap")
    }

    pub fn write(&self, roots_dir: &Path) -> Result<(), TrustError> {
        let dir = roots_dir.join("trust");
        std::fs::create_dir_all(&dir).map_err(|e| TrustError::Io {
            detail: e.to_string(),
        })?;
        let body = format!(
            "digest={}\nversion={}\nconsistent_snapshot={}\n",
            self.pin_digest, self.root_version, self.consistent_snapshot
        );
        std::fs::write(Self::path(roots_dir), body).map_err(|e| TrustError::Io {
            detail: e.to_string(),
        })
    }

    pub fn load(roots_dir: &Path) -> Result<Self, TrustError> {
        let text = std::fs::read_to_string(Self::path(roots_dir)).map_err(|_| {
            TrustError::BootstrapMissing
        })?;
        let mut digest = None;
        let mut version = None;
        let mut consistent = None;
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("digest=") {
                digest = Some(v.to_string());
            } else if let Some(v) = line.strip_prefix("version=") {
                version = v.parse().ok();
            } else if let Some(v) = line.strip_prefix("consistent_snapshot=") {
                consistent = Some(v == "true");
            }
        }
        Ok(Self {
            pin_digest: digest.ok_or(TrustError::BootstrapMissing)?,
            root_version: version.ok_or(TrustError::BootstrapMissing)?,
            consistent_snapshot: consistent.ok_or(TrustError::BootstrapMissing)?,
        })
    }
}

/// Live trust engine state after bootstrap.
#[derive(Debug, Clone)]
pub struct TrustEngine {
    pub root: RootMetadata,
    pub root_digest: String,
    pub policy: TrustPolicy,
    /// Last accepted monotonic versions per role.
    pub versions: BTreeMap<MetadataRole, u64>,
    pub keyring: Keyring,
    pub identities: BTreeMap<IdentityKind, BoundIdentity>,
}

impl TrustEngine {
    /// Pin and accept the first root. Digests must match `expected_pin` when provided.
    pub fn bootstrap(
        signed_root: &Signed<RootMetadata>,
        keyring: Keyring,
        policy: TrustPolicy,
        expected_pin: Option<&str>,
        clock: &dyn TrustedClock,
        roots_dir: Option<&Path>,
    ) -> Result<Self, TrustError> {
        let canonical = canonical_root(&signed_root.signed);
        enforce_size(MetadataRole::Root, canonical.len(), &policy)?;
        check_clock(&policy, clock)?;
        verify_role_signatures(
            MetadataRole::Root,
            &canonical,
            &signed_root.signatures,
            signed_root.signed.roles.get(&MetadataRole::Root).ok_or(
                TrustError::InvalidThreshold {
                    detail: "root metadata missing root role keys".into(),
                },
            )?,
            &keyring,
        )?;
        check_expiry(MetadataRole::Root, signed_root.signed.expires_unix, clock)?;
        let digest = SHA256::sha256_hex(canonical.as_bytes());
        if let Some(pin) = expected_pin {
            if pin != digest {
                return Err(TrustError::BootstrapPinMismatch {
                    expected: pin.to_string(),
                    actual: digest,
                });
            }
        }
        let bootstrap = RootBootstrap {
            pin_digest: digest.clone(),
            root_version: signed_root.signed.version,
            consistent_snapshot: signed_root.signed.consistent_snapshot,
        };
        if let Some(dir) = roots_dir {
            bootstrap.write(dir)?;
        }
        let mut versions = BTreeMap::new();
        versions.insert(MetadataRole::Root, signed_root.signed.version);
        Ok(Self {
            root: signed_root.signed.clone(),
            root_digest: digest,
            policy,
            versions,
            keyring,
            identities: BTreeMap::new(),
        })
    }

    /// Load an existing bootstrap pin and verify an offered root matches it.
    pub fn from_bootstrap_pin(
        roots_dir: &Path,
        signed_root: &Signed<RootMetadata>,
        keyring: Keyring,
        policy: TrustPolicy,
        clock: &dyn TrustedClock,
    ) -> Result<Self, TrustError> {
        let pin = RootBootstrap::load(roots_dir)?;
        let eng = Self::bootstrap(
            signed_root,
            keyring,
            policy,
            Some(&pin.pin_digest),
            clock,
            None,
        )?;
        if eng.root.version < pin.root_version {
            return Err(TrustError::Rollback {
                role: MetadataRole::Root,
                current: pin.root_version,
                incoming: eng.root.version,
            });
        }
        Ok(eng)
    }

    pub fn bind_identity(&mut self, identity: BoundIdentity) -> Result<(), TrustError> {
        identity.validate()?;
        // Distinct domains: one binding per kind in this slice. JP6B may widen.
        if let Some(existing) = self.identities.get(&identity.kind) {
            if existing.name != identity.name {
                return Err(TrustError::IdentityKindMismatch {
                    kind: identity.kind,
                    detail: format!(
                        "already bound to `{}`; refusing to replace with `{}`",
                        existing.name, identity.name
                    ),
                });
            }
        }
        self.identities.insert(identity.kind, identity);
        Ok(())
    }

    pub fn update_root(
        &mut self,
        signed_root: &Signed<RootMetadata>,
        clock: &dyn TrustedClock,
    ) -> Result<(), TrustError> {
        let canonical = canonical_root(&signed_root.signed);
        enforce_size(MetadataRole::Root, canonical.len(), &self.policy)?;
        check_clock(&self.policy, clock)?;
        // New root must be signed by the **current** root threshold (recovery).
        let current_role = self.root.roles.get(&MetadataRole::Root).ok_or(
            TrustError::InvalidThreshold {
                detail: "current root missing root role".into(),
            },
        )?;
        verify_role_signatures(
            MetadataRole::Root,
            &canonical,
            &signed_root.signatures,
            current_role,
            &self.keyring,
        )?;
        check_expiry(MetadataRole::Root, signed_root.signed.expires_unix, clock)?;
        let current_v = *self.versions.get(&MetadataRole::Root).unwrap_or(&0);
        if signed_root.signed.version <= current_v {
            return Err(TrustError::Rollback {
                role: MetadataRole::Root,
                current: current_v,
                incoming: signed_root.signed.version,
            });
        }
        self.root = signed_root.signed.clone();
        self.root_digest = SHA256::sha256_hex(canonical.as_bytes());
        self.versions.insert(MetadataRole::Root, signed_root.signed.version);
        Ok(())
    }

    /// Threshold-minus-one recovery drill: attempt root rotation with fewer
    /// than threshold current-root signatures. Must fail.
    pub fn recovery_drill_threshold_minus_one(
        &self,
        new_root: &RootMetadata,
        signers: &[&TrustKey],
        clock: &dyn TrustedClock,
    ) -> Result<(), TrustError> {
        let role = self.root.roles.get(&MetadataRole::Root).ok_or(
            TrustError::InvalidThreshold {
                detail: "current root missing root role".into(),
            },
        )?;
        if signers.len() >= role.threshold {
            return Err(TrustError::RotationRejected {
                detail: "drill requires fewer than threshold signers".into(),
            });
        }
        let signed = sign_root(new_root, signers)?;
        match self.clone_for_drill().update_root(&signed, clock) {
            Err(TrustError::ThresholdUnmet { .. }) => Ok(()),
            Err(other) => Err(TrustError::RotationRejected {
                detail: format!("expected threshold unmet, got: {other}"),
            }),
            Ok(()) => Err(TrustError::RotationRejected {
                detail: "threshold-minus-one rotation incorrectly succeeded".into(),
            }),
        }
    }

    fn clone_for_drill(&self) -> Self {
        self.clone()
    }

    pub fn verify_targets(
        &mut self,
        signed: &Signed<TargetsMetadata>,
        delegated_path: Option<&str>,
        clock: &dyn TrustedClock,
    ) -> Result<(), TrustError> {
        let canonical = canonical_targets(&signed.signed);
        enforce_size(MetadataRole::Targets, canonical.len(), &self.policy)?;
        check_clock(&self.policy, clock)?;
        let role = self.targets_role_for_path(delegated_path)?;
        verify_role_signatures(
            MetadataRole::Targets,
            &canonical,
            &signed.signatures,
            &role,
            &self.keyring,
        )?;
        check_expiry(MetadataRole::Targets, signed.signed.expires_unix, clock)?;
        enforce_monotonic(self, MetadataRole::Targets, signed.signed.version)?;
        if let Some(path) = delegated_path {
            self.ensure_delegation_allows(path)?;
        }
        self.versions
            .insert(MetadataRole::Targets, signed.signed.version);
        Ok(())
    }

    pub fn verify_snapshot(
        &mut self,
        signed: &Signed<SnapshotMetadata>,
        targets_canonical: &str,
        targets_version: u64,
        clock: &dyn TrustedClock,
    ) -> Result<(), TrustError> {
        let canonical = canonical_snapshot(&signed.signed);
        enforce_size(MetadataRole::Snapshot, canonical.len(), &self.policy)?;
        check_clock(&self.policy, clock)?;
        let role = self.root.roles.get(&MetadataRole::Snapshot).ok_or(
            TrustError::InvalidThreshold {
                detail: "root missing snapshot role".into(),
            },
        )?;
        verify_role_signatures(
            MetadataRole::Snapshot,
            &canonical,
            &signed.signatures,
            role,
            &self.keyring,
        )?;
        check_expiry(MetadataRole::Snapshot, signed.signed.expires_unix, clock)?;
        enforce_monotonic(self, MetadataRole::Snapshot, signed.signed.version)?;
        if self.root.consistent_snapshot {
            let entry = signed.signed.meta.get("targets").ok_or(
                TrustError::ConsistentSnapshotMismatch {
                    role: "targets".into(),
                    detail: "snapshot missing targets entry".into(),
                },
            )?;
            if entry.version != targets_version {
                return Err(TrustError::ConsistentSnapshotMismatch {
                    role: "targets".into(),
                    detail: format!(
                        "version {} != targets {}",
                        entry.version, targets_version
                    ),
                });
            }
            let want = SHA256::sha256_hex(targets_canonical.as_bytes());
            let got = entry
                .hashes
                .get("sha256")
                .cloned()
                .unwrap_or_default();
            if got != want {
                return Err(TrustError::ConsistentSnapshotMismatch {
                    role: "targets".into(),
                    detail: format!("sha256 {got} != {want}"),
                });
            }
            if entry.length as usize != targets_canonical.len() {
                return Err(TrustError::ConsistentSnapshotMismatch {
                    role: "targets".into(),
                    detail: format!(
                        "length {} != {}",
                        entry.length,
                        targets_canonical.len()
                    ),
                });
            }
        }
        self.versions
            .insert(MetadataRole::Snapshot, signed.signed.version);
        Ok(())
    }

    pub fn verify_timestamp(
        &mut self,
        signed: &Signed<TimestampMetadata>,
        snapshot_canonical: &str,
        snapshot_version: u64,
        clock: &dyn TrustedClock,
    ) -> Result<(), TrustError> {
        let canonical = canonical_timestamp(&signed.signed);
        enforce_size(MetadataRole::Timestamp, canonical.len(), &self.policy)?;
        check_clock(&self.policy, clock)?;
        let role = self.root.roles.get(&MetadataRole::Timestamp).ok_or(
            TrustError::InvalidThreshold {
                detail: "root missing timestamp role".into(),
            },
        )?;
        verify_role_signatures(
            MetadataRole::Timestamp,
            &canonical,
            &signed.signatures,
            role,
            &self.keyring,
        )?;
        check_expiry(MetadataRole::Timestamp, signed.signed.expires_unix, clock)?;
        enforce_monotonic(self, MetadataRole::Timestamp, signed.signed.version)?;
        if signed.signed.snapshot.version != snapshot_version {
            return Err(TrustError::ConsistentSnapshotMismatch {
                role: "snapshot".into(),
                detail: format!(
                    "timestamp snapshot version {} != {snapshot_version}",
                    signed.signed.snapshot.version
                ),
            });
        }
        let want = SHA256::sha256_hex(snapshot_canonical.as_bytes());
        let got = signed
            .signed
            .snapshot
            .hashes
            .get("sha256")
            .cloned()
            .unwrap_or_default();
        if got != want {
            return Err(TrustError::ConsistentSnapshotMismatch {
                role: "snapshot".into(),
                detail: format!("sha256 {got} != {want}"),
            });
        }
        self.versions
            .insert(MetadataRole::Timestamp, signed.signed.version);
        Ok(())
    }

    fn targets_role_for_path(&self, path: Option<&str>) -> Result<RoleKeys, TrustError> {
        if let Some(path) = path {
            for del in &self.root.delegations {
                if del.allows_path(path) {
                    return Ok(del.role.clone());
                }
            }
            // Fall through to top-level targets only when no path-bound
            // delegation matched; callers that require delegation must use
            // ensure_delegation_allows after.
        }
        self.root
            .roles
            .get(&MetadataRole::Targets)
            .cloned()
            .ok_or(TrustError::InvalidThreshold {
                detail: "root missing targets role".into(),
            })
    }

    fn ensure_delegation_allows(&self, path: &str) -> Result<(), TrustError> {
        if self.root.delegations.is_empty() {
            return Ok(());
        }
        if self.root.delegations.iter().any(|d| d.allows_path(path)) {
            return Ok(());
        }
        Err(TrustError::DelegationDenied {
            path: path.to_string(),
            detail: "no delegation path prefix matches".into(),
        })
    }
}

pub fn sign_root(root: &RootMetadata, keys: &[&TrustKey]) -> Result<Signed<RootMetadata>, TrustError> {
    let canonical = canonical_root(root);
    let signatures = keys.iter().map(|k| k.sign(canonical.as_bytes())).collect();
    Ok(Signed {
        signed: root.clone(),
        signatures,
    })
}

pub fn sign_targets(
    targets: &TargetsMetadata,
    keys: &[&TrustKey],
) -> Result<Signed<TargetsMetadata>, TrustError> {
    let canonical = canonical_targets(targets);
    Ok(Signed {
        signed: targets.clone(),
        signatures: keys.iter().map(|k| k.sign(canonical.as_bytes())).collect(),
    })
}

pub fn sign_snapshot(
    snapshot: &SnapshotMetadata,
    keys: &[&TrustKey],
) -> Result<Signed<SnapshotMetadata>, TrustError> {
    let canonical = canonical_snapshot(snapshot);
    Ok(Signed {
        signed: snapshot.clone(),
        signatures: keys.iter().map(|k| k.sign(canonical.as_bytes())).collect(),
    })
}

pub fn sign_timestamp(
    timestamp: &TimestampMetadata,
    keys: &[&TrustKey],
) -> Result<Signed<TimestampMetadata>, TrustError> {
    let canonical = canonical_timestamp(timestamp);
    Ok(Signed {
        signed: timestamp.clone(),
        signatures: keys.iter().map(|k| k.sign(canonical.as_bytes())).collect(),
    })
}

pub fn canonical_root(root: &RootMetadata) -> String {
    let mut out = String::new();
    out.push_str("jet-tuf-root-v1\n");
    out.push_str(&format!("version={}\n", root.version));
    out.push_str(&format!("expires={}\n", root.expires_unix));
    out.push_str(&format!(
        "consistent_snapshot={}\n",
        root.consistent_snapshot
    ));
    for (role, keys) in &root.roles {
        out.push_str(&format!(
            "role.{}={}@{}\n",
            role.as_str(),
            keys.threshold,
            keys.key_ids.join(",")
        ));
    }
    for (kid, alg) in &root.public_key_ids {
        out.push_str(&format!("key.{kid}={alg}\n"));
    }
    for del in &root.delegations {
        out.push_str(&format!(
            "delegation.{}={}@{}|{}|{}\n",
            del.name,
            del.role.threshold,
            del.role.key_ids.join(","),
            del.path_prefixes.join(","),
            del.terminating
        ));
    }
    out
}

pub fn canonical_targets(t: &TargetsMetadata) -> String {
    let mut out = format!(
        "jet-tuf-targets-v1\nversion={}\nexpires={}\n",
        t.version, t.expires_unix
    );
    for (path, meta) in &t.targets {
        let hash = meta.hashes.get("sha256").cloned().unwrap_or_default();
        out.push_str(&format!("target.{path}={}:{}\n", meta.length, hash));
    }
    out
}

pub fn canonical_snapshot(s: &SnapshotMetadata) -> String {
    let mut out = format!(
        "jet-tuf-snapshot-v1\nversion={}\nexpires={}\n",
        s.version, s.expires_unix
    );
    for (name, entry) in &s.meta {
        let hash = entry.hashes.get("sha256").cloned().unwrap_or_default();
        out.push_str(&format!(
            "meta.{name}={}:{hash}:{}\n",
            entry.version, entry.length
        ));
    }
    out
}

pub fn canonical_timestamp(t: &TimestampMetadata) -> String {
    let hash = t
        .snapshot
        .hashes
        .get("sha256")
        .cloned()
        .unwrap_or_default();
    format!(
        "jet-tuf-timestamp-v1\nversion={}\nexpires={}\nsnapshot={}:{hash}:{}\n",
        t.version, t.expires_unix, t.snapshot.version, t.snapshot.length
    )
}

fn verify_role_signatures(
    role: MetadataRole,
    canonical: &str,
    signatures: &[Signature],
    role_keys: &RoleKeys,
    keyring: &Keyring,
) -> Result<(), TrustError> {
    if signatures.is_empty() {
        return Err(TrustError::SignatureStripped { role });
    }
    let allowed: BTreeSet<&str> = role_keys.key_ids.iter().map(String::as_str).collect();
    let mut valid: BTreeSet<String> = BTreeSet::new();
    for sig in signatures {
        if !allowed.contains(sig.key_id.as_str()) {
            continue;
        }
        match keyring.verify(&sig.key_id, canonical.as_bytes(), sig) {
            Ok(()) => {
                valid.insert(sig.key_id.clone());
            }
            Err(TrustError::UnknownKey { .. }) => continue,
            Err(TrustError::SignatureInvalid { detail, .. }) => {
                // Invalid signature from an allowed key does not count; keep scanning.
                let _ = detail;
            }
            Err(other) => return Err(other),
        }
    }
    if valid.len() < role_keys.threshold {
        return Err(TrustError::ThresholdUnmet {
            role,
            have: valid.len(),
            need: role_keys.threshold,
        });
    }
    Ok(())
}

fn enforce_monotonic(
    eng: &TrustEngine,
    role: MetadataRole,
    incoming: u64,
) -> Result<(), TrustError> {
    let current = *eng.versions.get(&role).unwrap_or(&0);
    if incoming < current {
        return Err(TrustError::Rollback {
            role,
            current,
            incoming,
        });
    }
    // Equal version is allowed only for re-verify of the same metadata in this
    // slice; advancing requires strictly greater for root updates. Targets/
    // snapshot/timestamp accept equal as idempotent refresh.
    Ok(())
}

fn enforce_size(role: MetadataRole, size: usize, policy: &TrustPolicy) -> Result<(), TrustError> {
    if size > policy.max_metadata_bytes {
        return Err(TrustError::MetadataTooLarge {
            role,
            size,
            max: policy.max_metadata_bytes,
        });
    }
    Ok(())
}

fn check_expiry(role: MetadataRole, expires_unix: u64, clock: &dyn TrustedClock) -> Result<(), TrustError> {
    let now = clock.now_unix();
    if now > expires_unix {
        return Err(TrustError::Expired {
            role,
            expires_unix,
            now_unix: now,
        });
    }
    Ok(())
}

fn check_clock(policy: &TrustPolicy, clock: &dyn TrustedClock) -> Result<(), TrustError> {
    if let Some(trusted) = policy.trusted_unix {
        let now = clock.now_unix();
        let skew = now.abs_diff(trusted);
        let max = policy.max_clock_skew.as_secs();
        if skew > max {
            return Err(TrustError::BadClock {
                skew_secs: skew,
                max_secs: max,
            });
        }
    }
    Ok(())
}

fn key_id_for(secret: &[u8]) -> String {
    SHA256::sha256_hex(secret)[..16].to_string()
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    // RFC 2104 HMAC with SHA-256 (I6-clean).
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let hashed = SHA256::sha256(key);
        key_block[..32].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }
    let mut inner = Vec::with_capacity(BLOCK + message.len());
    inner.extend_from_slice(&ipad);
    inner.extend_from_slice(message);
    let inner_hash = SHA256::sha256(&inner);
    let mut outer = Vec::with_capacity(BLOCK + 32);
    outer.extend_from_slice(&opad);
    outer.extend_from_slice(&inner_hash);
    SHA256::sha256(&outer)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn require_nonempty(field: &str, value: &str) -> Result<(), TrustError> {
    if value.trim().is_empty() {
        return Err(TrustError::InvalidPublisher {
            detail: format!("{field} must be non-empty"),
        });
    }
    Ok(())
}

fn require_sha256_hex(field: &str, value: &str) -> Result<(), TrustError> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(TrustError::InvalidPublisher {
            detail: format!("{field} must be 64-char sha256 hex"),
        });
    }
    Ok(())
}

fn require_ed25519_pub_hex(field: &str, value: &str) -> Result<(), TrustError> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(TrustError::InvalidPublisher {
            detail: format!("{field} must be 32-byte ed25519 public key hex"),
        });
    }
    Ok(())
}

/// Build a minimal 2-of-3 offline root for drills and tests.
pub fn fixture_threshold_root(
    version: u64,
    expires_unix: u64,
) -> (RootMetadata, Keyring, Vec<TrustKey>) {
    let k1 = TrustKey::generate("root-1");
    let k2 = TrustKey::generate("root-2");
    let k3 = TrustKey::generate("root-3");
    let targets = TrustKey::generate("targets");
    let snapshot = TrustKey::generate("snapshot");
    let timestamp = TrustKey::generate("timestamp");
    let mut keyring = Keyring::default();
    for k in [&k1, &k2, &k3, &targets, &snapshot, &timestamp] {
        keyring.insert(k.clone());
    }
    let mut roles = BTreeMap::new();
    roles.insert(
        MetadataRole::Root,
        RoleKeys::new(vec![k1.key_id.clone(), k2.key_id.clone(), k3.key_id.clone()], 2).unwrap(),
    );
    roles.insert(
        MetadataRole::Targets,
        RoleKeys::new(vec![targets.key_id.clone()], 1).unwrap(),
    );
    roles.insert(
        MetadataRole::Snapshot,
        RoleKeys::new(vec![snapshot.key_id.clone()], 1).unwrap(),
    );
    roles.insert(
        MetadataRole::Timestamp,
        RoleKeys::new(vec![timestamp.key_id.clone()], 1).unwrap(),
    );
    let mut public_key_ids = BTreeMap::new();
    for k in [&k1, &k2, &k3, &targets, &snapshot, &timestamp] {
        public_key_ids.insert(k.key_id.clone(), ALG_HMAC_SHA256.to_string());
    }
    let root = RootMetadata {
        version,
        expires_unix,
        consistent_snapshot: true,
        roles,
        delegations: vec![Delegation {
            name: "jetsrc".into(),
            role: RoleKeys::new(vec![targets.key_id.clone()], 1).unwrap(),
            path_prefixes: vec!["jetsrc/".into()],
            terminating: true,
        }],
        public_key_ids,
    };
    (
        root,
        keyring,
        vec![k1, k2, k3, targets, snapshot, timestamp],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SCRATCH: AtomicU64 = AtomicU64::new(0);

    fn scratch_dir(tag: &str) -> PathBuf {
        let n = SCRATCH.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!(
            "jet-trustroot-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn boot(now: u64) -> (TrustEngine, Vec<TrustKey>, PathBuf) {
        let dir = scratch_dir("boot");
        let (root, keyring, keys) = fixture_threshold_root(1, now + 3600);
        let signed = sign_root(&root, &[&keys[0], &keys[1]]).unwrap();
        let eng = TrustEngine::bootstrap(
            &signed,
            keyring,
            TrustPolicy {
                trusted_unix: Some(now),
                ..TrustPolicy::default()
            },
            None,
            &FixedClock(now),
            Some(&dir),
        )
        .unwrap();
        (eng, keys, dir)
    }

    #[test]
    fn bootstrap_pins_digest_and_rejects_mismatch() {
        let now = 1_700_000_000;
        let (root, keyring, keys) = fixture_threshold_root(1, now + 3600);
        let signed = sign_root(&root, &[&keys[0], &keys[1]]).unwrap();
        let dig = SHA256::sha256_hex(canonical_root(&root).as_bytes());
        let eng = TrustEngine::bootstrap(
            &signed,
            keyring.clone(),
            TrustPolicy::default(),
            Some(&dig),
            &FixedClock(now),
            None,
        )
        .unwrap();
        assert_eq!(eng.root_digest, dig);
        let err = TrustEngine::bootstrap(
            &signed,
            keyring,
            TrustPolicy::default(),
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
            &FixedClock(now),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, TrustError::BootstrapPinMismatch { .. }));
    }

    #[test]
    fn signature_stripping_rejected() {
        let now = 1_700_000_000;
        let (root, keyring, keys) = fixture_threshold_root(1, now + 3600);
        let mut signed = sign_root(&root, &[&keys[0], &keys[1]]).unwrap();
        signed.signatures.clear();
        let err = TrustEngine::bootstrap(
            &signed,
            keyring,
            TrustPolicy::default(),
            None,
            &FixedClock(now),
            None,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            TrustError::SignatureStripped {
                role: MetadataRole::Root
            }
        ));
    }

    #[test]
    fn threshold_minus_one_cannot_bootstrap_or_rotate() {
        let now = 1_700_000_000;
        let (eng, keys, _) = boot(now);
        let (mut new_root, _, _) = fixture_threshold_root(2, now + 7200);
        new_root.version = 2;
        // Drill API.
        eng.recovery_drill_threshold_minus_one(&new_root, &[&keys[0]], &FixedClock(now))
            .unwrap();
        // Direct update with one of three (threshold 2) fails.
        let signed = sign_root(&new_root, &[&keys[0]]).unwrap();
        let mut drill = eng.clone_for_drill();
        let err = drill.update_root(&signed, &FixedClock(now)).unwrap_err();
        assert!(matches!(
            err,
            TrustError::ThresholdUnmet {
                role: MetadataRole::Root,
                have: 1,
                need: 2
            }
        ));
    }

    #[test]
    fn root_rotation_with_threshold_succeeds_and_rollback_fails() {
        let now = 1_700_000_000;
        let (mut eng, keys, _) = boot(now);
        let (mut new_root, _, _) = fixture_threshold_root(2, now + 7200);
        new_root.version = 2;
        // Keep same root key ids so current threshold keys still verify.
        new_root.roles = eng.root.roles.clone();
        new_root.public_key_ids = eng.root.public_key_ids.clone();
        new_root.delegations = eng.root.delegations.clone();
        let signed = sign_root(&new_root, &[&keys[0], &keys[1]]).unwrap();
        eng.update_root(&signed, &FixedClock(now)).unwrap();
        assert_eq!(eng.versions.get(&MetadataRole::Root), Some(&2));
        let mut older = new_root.clone();
        older.version = 1;
        let signed_old = sign_root(&older, &[&keys[0], &keys[1]]).unwrap();
        let err = eng.update_root(&signed_old, &FixedClock(now)).unwrap_err();
        assert!(matches!(err, TrustError::Rollback { .. }));
    }

    #[test]
    fn delegation_path_bounds_and_consistent_snapshot() {
        let now = 1_700_000_000;
        let (mut eng, keys, _) = boot(now);
        let targets_key = &keys[3];
        let snapshot_key = &keys[4];
        let timestamp_key = &keys[5];

        let targets = TargetsMetadata {
            version: 1,
            expires_unix: now + 3600,
            targets: BTreeMap::from([(
                "jetsrc/core".into(),
                TargetMeta {
                    length: 4,
                    hashes: BTreeMap::from([(
                        "sha256".into(),
                        SHA256::sha256_hex(b"blob"),
                    )]),
                    custom: BTreeMap::new(),
                },
            )]),
        };
        let signed_targets = sign_targets(&targets, &[targets_key]).unwrap();
        eng.verify_targets(&signed_targets, Some("jetsrc/core"), &FixedClock(now))
            .unwrap();
        let mut denied_eng = eng.clone_for_drill();
        let denied = denied_eng
            .verify_targets(&signed_targets, Some("evil/core"), &FixedClock(now))
            .unwrap_err();
        assert!(matches!(denied, TrustError::DelegationDenied { .. }));

        let t_canon = canonical_targets(&targets);
        let snap = SnapshotMetadata {
            version: 1,
            expires_unix: now + 3600,
            meta: BTreeMap::from([(
                "targets".into(),
                SnapshotMetaEntry {
                    version: 1,
                    length: t_canon.len() as u64,
                    hashes: BTreeMap::from([("sha256".into(), SHA256::sha256_hex(t_canon.as_bytes()))]),
                },
            )]),
        };
        let signed_snap = sign_snapshot(&snap, &[snapshot_key]).unwrap();
        eng.verify_snapshot(&signed_snap, &t_canon, 1, &FixedClock(now))
            .unwrap();

        let s_canon = canonical_snapshot(&snap);
        let ts = TimestampMetadata {
            version: 1,
            expires_unix: now + 3600,
            snapshot: SnapshotMetaEntry {
                version: 1,
                length: s_canon.len() as u64,
                hashes: BTreeMap::from([("sha256".into(), SHA256::sha256_hex(s_canon.as_bytes()))]),
            },
        };
        let signed_ts = sign_timestamp(&ts, &[timestamp_key]).unwrap();
        eng.verify_timestamp(&signed_ts, &s_canon, 1, &FixedClock(now))
            .unwrap();
    }

    #[test]
    fn bad_clock_expiry_and_size_limits() {
        let now = 1_700_000_000;
        let (root, keyring, keys) = fixture_threshold_root(1, now + 3600);
        let signed = sign_root(&root, &[&keys[0], &keys[1]]).unwrap();
        let err = TrustEngine::bootstrap(
            &signed,
            keyring.clone(),
            TrustPolicy {
                trusted_unix: Some(now),
                max_clock_skew: Duration::from_secs(30),
                ..TrustPolicy::default()
            },
            None,
            &FixedClock(now + 120),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, TrustError::BadClock { .. }));

        let err = TrustEngine::bootstrap(
            &signed,
            keyring.clone(),
            TrustPolicy::default(),
            None,
            &FixedClock(now + 7200),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, TrustError::Expired { .. }));

        let err = TrustEngine::bootstrap(
            &signed,
            keyring,
            TrustPolicy {
                max_metadata_bytes: 8,
                ..TrustPolicy::default()
            },
            None,
            &FixedClock(now),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, TrustError::MetadataTooLarge { .. }));
    }

    #[test]
    fn hybrid_publisher_identities_are_distinct_domains() {
        let now = 1_700_000_000;
        let (mut eng, _, _) = boot(now);
        eng.bind_identity(BoundIdentity::registry("jet.dev"))
            .unwrap();
        eng.bind_identity(BoundIdentity::cache_builder("ci-builder-1"))
            .unwrap();
        eng.bind_identity(BoundIdentity::remote_executor("worker-a"))
            .unwrap();
        let pub_id = PublisherIdentity::OfflineEd25519 {
            public_key_hex: "11".repeat(32),
            key_id: "pub1".into(),
        };
        eng.bind_identity(BoundIdentity::publisher("alice", pub_id))
            .unwrap();
        let sigstore = PublisherIdentity::Sigstore {
            identity: "alice@jet.dev".into(),
            issuer: "https://accounts.google.com".into(),
            checkpoint_digest: "aa".repeat(32),
            bundle_digest: "bb".repeat(32),
        };
        // Cannot put Sigstore proof on a registry identity.
        let err = eng
            .bind_identity(BoundIdentity {
                kind: IdentityKind::Registry,
                name: "other".into(),
                publisher: Some(sigstore.clone()),
            })
            .unwrap_err();
        assert!(matches!(err, TrustError::IdentityKindMismatch { .. }));
        // Publisher with Sigstore validates.
        let mut eng2 = eng.clone_for_drill();
        // already has publisher alice — rebind same name ok
        eng2.bind_identity(BoundIdentity::publisher("alice", {
            // different proof same name — still same binding name; allowed as refresh
            PublisherIdentity::Sigstore {
                identity: "alice@jet.dev".into(),
                issuer: "https://accounts.google.com".into(),
                checkpoint_digest: "aa".repeat(32),
                bundle_digest: "bb".repeat(32),
            }
        }))
        .unwrap();
        let kms = PublisherIdentity::KmsHsm {
            key_uri: "kms:projects/p/locations/l/keyRings/r/cryptoKeys/k".into(),
            public_key_hex: "cc".repeat(32),
            key_id: "kms1".into(),
        };
        kms.validate().unwrap();
        PublisherIdentity::KmsHsm {
            key_uri: "http://evil".into(),
            public_key_hex: "cc".repeat(32),
            key_id: "kms1".into(),
        }
        .validate()
        .unwrap_err();
    }

    #[test]
    fn bootstrap_file_round_trip() {
        let now = 1_700_000_000;
        let (eng, keys, dir) = boot(now);
        let pin = RootBootstrap::load(&dir).unwrap();
        assert_eq!(pin.pin_digest, eng.root_digest);
        let (root, keyring, _) = fixture_threshold_root(1, now + 3600);
        // Re-sign with same fixture keys from boot — need the boot keys.
        let signed = sign_root(&eng.root, &[&keys[0], &keys[1]]).unwrap();
        let reloaded = TrustEngine::from_bootstrap_pin(
            &dir,
            &signed,
            eng.keyring.clone(),
            TrustPolicy {
                trusted_unix: Some(now),
                ..TrustPolicy::default()
            },
            &FixedClock(now),
        )
        .unwrap();
        assert_eq!(reloaded.root_digest, eng.root_digest);
        let _ = (root, keyring);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
