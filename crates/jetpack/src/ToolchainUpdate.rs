//! Signed Jet toolchain channel reads and safe installation.
//!
//! The channel is a static-file protocol. The manifest and artifact are
//! verified before an update is considered, and installation stages beside
//! the current executable before an atomic rename. A file endpoint supports
//! local publication staging and tests; production endpoints use HTTPS.

use crate::TrustRoot::PublicTrustKey;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use jet_foundation::base_encoding_strict::decode_base64;
use jet_foundation::EncodingJson::{parse_json_exact_numbers, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use std::time::Duration;

/// D-DOMAIN-LAYOUT1=A (#2199/#2201): toolchain releases live at the owned
/// download subdomain; production key publication remains owner-controlled.
pub const DEFAULT_ENDPOINT: &str = "https://dl.jet-lang.dev";
pub const DEFAULT_CHANNEL: &str = "stable";
pub const ENDPOINT_ENV: &str = "JET_TOOLCHAIN_ENDPOINT";
pub const MANIFEST_DOMAIN: &[u8] = b"jet-toolchain-channel-v1\n";
pub const ARTIFACT_DOMAIN: &[u8] = b"jet-toolchain-artifact-v1\n";

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_SIGNATURE_BYTES: u64 = 16 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_STATE_BYTES: u64 = 16 * 1024;
const MAX_ARTIFACTS: usize = 64;
const MAX_CLOCK_SKEW_SECONDS: u64 = 5 * 60;
const MAX_MANIFEST_AGE_SECONDS: u64 = 30 * 24 * 60 * 60;
#[cfg(windows)]
const WINDOWS_HANDOFF_ATTEMPTS: usize = 600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOptions {
    pub endpoint: String,
    pub channel: String,
    pub platform: String,
    pub trust_key: PathBuf,
    pub dry_run: bool,
    pub apply: bool,
    /// Permit an explicitly selected local file endpoint without signature
    /// sidecars. This never applies to HTTPS endpoints.
    pub allow_unofficial: bool,
    /// The version of the running client. A channel cannot raise this floor,
    /// and a toolchain cannot move backwards without an owner-ratified rule.
    pub running_version: String,
    /// Per-channel/platform monotonic state. This is written only after a
    /// successful activation (or by the Windows helper after its activation).
    pub state_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateTrust {
    Signed,
    UnofficialKeyless,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePlan {
    pub endpoint: String,
    pub channel: String,
    pub version: String,
    pub platform: String,
    pub artifact_path: String,
    pub artifact_url: String,
    pub sha256: String,
    pub size: u64,
    pub key_id: Option<String>,
    pub trust: UpdateTrust,
    pub sequence: u64,
    pub published_at: u64,
    pub expires_at: u64,
    pub min_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateResult {
    pub plan: UpdatePlan,
    pub applied: bool,
    pub deferred: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateError {
    detail: String,
}

impl UpdateError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for UpdateError {}

/// Resolve the host-owned endpoint override. CLI selection belongs to the
/// front door; this helper supplies the environment and Jetpack config layers.
pub fn configured_endpoint(root: &Path) -> Result<Option<String>, UpdateError> {
    if let Some(value) = std::env::var_os(ENDPOINT_ENV).filter(|value| !value.is_empty()) {
        return value
            .into_string()
            .map(Some)
            .map_err(|_| UpdateError::new("toolchain endpoint environment value is not UTF-8"));
    }
    let path = root.join("config/toolchain-v1.endpoint");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(UpdateError::new(format!(
                "inspect toolchain endpoint config: {error}"
            )))
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 4096 {
        return Err(UpdateError::new(
            "toolchain endpoint config is not a regular file",
        ));
    }
    let value = String::from_utf8(
        fs::read(&path)
            .map_err(|error| UpdateError::new(format!("read toolchain endpoint config: {error}")))?,
    )
    .map_err(|_| UpdateError::new("toolchain endpoint config is not UTF-8"))?;
    Ok(Some(value.trim().to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Endpoint {
    File(PathBuf),
    Http(HttpEndpoint),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpEndpoint {
    base: String,
    origin: HttpOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpOrigin {
    scheme: String,
    host: HttpHost,
    port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HttpHost {
    Name(String),
    Address(IpAddr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelManifest {
    schema: u64,
    channel: String,
    version: String,
    sequence: u64,
    published_at: u64,
    expires_at: u64,
    min_version: String,
    artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Artifact {
    target: String,
    path: String,
    sha256: String,
    size: u64,
    signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SignatureSidecar {
    schema: u64,
    key_id: String,
    algorithm: String,
    signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateState {
    schema: u64,
    channel: String,
    platform: String,
    sequence: u64,
    version: String,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticVersion {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<PreIdentifier>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreIdentifier {
    Numeric(u64),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallOutcome {
    Applied,
    #[cfg(windows)]
    Deferred,
}

/// Verify the configured channel and, when requested, install the verified
/// artifact. Without apply, this is a complete dry-run verification.
pub fn run(
    options: &UpdateOptions,
    current_exe: Option<&Path>,
) -> Result<UpdateResult, UpdateError> {
    run_at(options, current_exe, SystemTime::now())
}

fn run_at(
    options: &UpdateOptions,
    current_exe: Option<&Path>,
    now: SystemTime,
) -> Result<UpdateResult, UpdateError> {
    if options.apply && options.dry_run {
        return Err(UpdateError::new("apply and dry-run cannot be combined"));
    }
    if options.apply && options.platform != default_target() {
        return Err(UpdateError::new(format!(
            "apply requires the exact host platform {}; selected {} is dry-run only",
            default_target(), options.platform
        )));
    }
    let (plan, artifact) = resolve(options, now)?;
    let (applied, deferred) = if options.apply {
        let current_exe = current_exe.ok_or_else(|| {
            UpdateError::new("self-update installation needs the current executable path")
        })?;
        match install_verified(
            current_exe,
            &artifact,
            &plan,
            &options.state_path,
            &options.running_version,
        )? {
            InstallOutcome::Applied => (true, false),
            #[cfg(windows)]
            InstallOutcome::Deferred => (false, true),
        }
    } else {
        (false, false)
    };
    Ok(UpdateResult {
        plan,
        applied,
        deferred,
    })
}

fn resolve(
    options: &UpdateOptions,
    now: SystemTime,
) -> Result<(UpdatePlan, Vec<u8>), UpdateError> {
    validate_component(&options.channel, "channel")?;
    validate_component(&options.platform, "platform")?;
    parse_semantic_version(&options.running_version, "running Jet version")?;
    validate_no_symlink_ancestors(&options.state_path, "toolchain update state")?;
    let endpoint = parse_endpoint(&options.endpoint)?;
    if options.allow_unofficial && !matches!(&endpoint, Endpoint::File(_)) {
        return Err(UpdateError::new(
            "unofficial keyless toolchain sources must use an explicit file:// endpoint",
        ));
    }
    let trusted_key = (!options.allow_unofficial)
        .then(|| read_public_key(&options.trust_key))
        .transpose()?;
    let manifest_path = format!("v1/{}/manifest.json", options.channel);
    let manifest_bytes = fetch_bytes(
        &endpoint,
        &manifest_path,
        MAX_MANIFEST_BYTES,
        "toolchain channel manifest",
    )?;
    if let Some((key_id, public_key)) = trusted_key.as_ref() {
        let manifest_signature_path = format!("{manifest_path}.sig.json");
        let manifest_signature = fetch_bytes(
            &endpoint,
            &manifest_signature_path,
            MAX_SIGNATURE_BYTES,
            "toolchain channel manifest signature",
        )?;
        verify_signature(
            public_key,
            key_id,
            MANIFEST_DOMAIN,
            &manifest_bytes,
            &manifest_signature,
            "toolchain channel manifest",
        )?;
    }
    let manifest = parse_manifest(&manifest_bytes, &options.channel)?;
    let now = unix_seconds(now)?;
    validate_manifest_policy(&manifest, options, now)?;
    let artifact = manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.target == options.platform)
        .ok_or_else(|| {
            UpdateError::new(format!(
                "toolchain channel has no artifact for {}",
                options.platform
            ))
        })?;
    let artifact_bytes = fetch_bytes(
        &endpoint,
        &artifact.path,
        artifact.size.min(MAX_ARTIFACT_BYTES),
        "toolchain artifact",
    )?;
    if artifact.size > MAX_ARTIFACT_BYTES || artifact_bytes.len() as u64 != artifact.size {
        return Err(UpdateError::new(
            "toolchain artifact size disagrees with the channel manifest",
        ));
    }
    if crate::SHA256::sha256_hex(&artifact_bytes) != artifact.sha256 {
        return Err(UpdateError::new(
            "toolchain artifact digest disagrees with the channel manifest",
        ));
    }
    if let Some((key_id, public_key)) = trusted_key.as_ref() {
        let artifact_signature = fetch_bytes(
            &endpoint,
            &artifact.signature,
            MAX_SIGNATURE_BYTES,
            "toolchain artifact signature",
        )?;
        verify_signature(
            public_key,
            key_id,
            ARTIFACT_DOMAIN,
            &artifact_bytes,
            &artifact_signature,
            "toolchain artifact",
        )?;
    }
    let artifact_url = endpoint_url(&endpoint, &artifact.path);
    Ok((
        UpdatePlan {
            endpoint: options.endpoint.clone(),
            channel: options.channel.clone(),
            version: manifest.version,
            platform: artifact.target.clone(),
            artifact_path: artifact.path.clone(),
            artifact_url,
            sha256: artifact.sha256.clone(),
            size: artifact.size,
            key_id: trusted_key.as_ref().map(|(key_id, _)| key_id.clone()),
            trust: if options.allow_unofficial {
                UpdateTrust::UnofficialKeyless
            } else {
                UpdateTrust::Signed
            },
            sequence: manifest.sequence,
            published_at: manifest.published_at,
            expires_at: manifest.expires_at,
            min_version: manifest.min_version.clone(),
        },
        artifact_bytes,
    ))
}

fn parse_manifest(bytes: &[u8], expected_channel: &str) -> Result<ChannelManifest, UpdateError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| UpdateError::new("toolchain channel manifest is not UTF-8"))?;
    let value = parse_json_exact_numbers(text, true).map_err(|error| {
        UpdateError::new(format!(
            "parse toolchain channel manifest: {}",
            error.message
        ))
    })?;
    let fields = object(&value, "toolchain channel manifest")?;
    reject_unknown(
        fields,
        &[
            "schema",
            "channel",
            "version",
            "sequence",
            "published_at",
            "expires_at",
            "min_version",
            "artifacts",
        ],
    )?;
    let manifest = ChannelManifest {
        schema: integer(field(fields, "schema")?, "manifest schema")?,
        channel: text_field(field(fields, "channel")?, "manifest channel")?.to_string(),
        version: text_field(field(fields, "version")?, "manifest version")?.to_string(),
        sequence: integer(field(fields, "sequence")?, "manifest sequence")?,
        published_at: integer(field(fields, "published_at")?, "manifest publication time")?,
        expires_at: integer(field(fields, "expires_at")?, "manifest expiry time")?,
        min_version: text_field(field(fields, "min_version")?, "manifest minimum version")?
            .to_string(),
        artifacts: array(field(fields, "artifacts")?, "manifest artifacts")?
            .iter()
            .map(parse_artifact)
            .collect::<Result<Vec<_>, _>>()?,
    };
    if manifest.schema != 1 {
        return Err(UpdateError::new("unsupported toolchain channel schema"));
    }
    if manifest.channel != expected_channel {
        return Err(UpdateError::new(
            "toolchain channel manifest disagrees with the requested channel",
        ));
    }
    parse_semantic_version(&manifest.version, "toolchain version")?;
    parse_semantic_version(&manifest.min_version, "manifest minimum version")?;
    if manifest.sequence == 0 {
        return Err(UpdateError::new(
            "toolchain channel manifest sequence must be positive",
        ));
    }
    if manifest.artifacts.is_empty() {
        return Err(UpdateError::new(
            "toolchain channel manifest has no artifacts",
        ));
    }
    if manifest.artifacts.len() > MAX_ARTIFACTS {
        return Err(UpdateError::new(
            "toolchain channel manifest has too many artifacts",
        ));
    }
    if manifest
        .artifacts
        .windows(2)
        .any(|pair| pair[0].target >= pair[1].target)
    {
        return Err(UpdateError::new(
            "toolchain channel artifacts are not sorted by target",
        ));
    }
    let mut targets = std::collections::BTreeSet::new();
    for artifact in &manifest.artifacts {
        validate_component(&artifact.target, "artifact target")?;
        if !targets.insert(artifact.target.clone()) {
            return Err(UpdateError::new(
                "toolchain channel manifest repeats an artifact target",
            ));
        }
        let expected_path = format!(
            "v1/{}/{}/jet-{}-{}",
            manifest.channel, manifest.version, manifest.version, artifact.target
        );
        if artifact.path != expected_path
            || artifact.signature != format!("{expected_path}.sig.json")
        {
            return Err(UpdateError::new(
                "toolchain artifact naming does not match the channel contract",
            ));
        }
        validate_relative_path(&artifact.path)?;
        validate_relative_path(&artifact.signature)?;
        validate_digest(&artifact.sha256, "toolchain artifact digest")?;
        if artifact.size == 0 || artifact.size > MAX_ARTIFACT_BYTES {
            return Err(UpdateError::new(
                "toolchain artifact size is outside the channel bound",
            ));
        }
    }
    if canonical_manifest(&manifest).as_bytes() != bytes {
        return Err(UpdateError::new(
            "toolchain channel manifest bytes are not canonical",
        ));
    }
    Ok(manifest)
}

fn parse_artifact(value: &Value) -> Result<Artifact, UpdateError> {
    let fields = object(value, "toolchain artifact")?;
    reject_unknown(fields, &["target", "path", "sha256", "size", "signature"])?;
    Ok(Artifact {
        target: text_field(field(fields, "target")?, "artifact target")?.to_string(),
        path: text_field(field(fields, "path")?, "artifact path")?.to_string(),
        sha256: text_field(field(fields, "sha256")?, "artifact digest")?.to_string(),
        size: integer(field(fields, "size")?, "artifact size")?,
        signature: text_field(field(fields, "signature")?, "artifact signature path")?.to_string(),
    })
}

fn validate_manifest_policy(
    manifest: &ChannelManifest,
    options: &UpdateOptions,
    now: u64,
) -> Result<(), UpdateError> {
    validate_release_policy(
        &manifest.version,
        &manifest.min_version,
        manifest.published_at,
        manifest.expires_at,
        &options.running_version,
        now,
    )?;
    if let Some(state) = read_state(&options.state_path)? {
        validate_state_transition(
            Some(&state),
            &options.channel,
            &options.platform,
            manifest.sequence,
            &manifest.version,
        )?;
    }
    Ok(())
}

fn validate_release_policy(
    version: &str,
    min_version: &str,
    published_at: u64,
    expires_at: u64,
    running_version: &str,
    now: u64,
) -> Result<(), UpdateError> {
    let running = parse_semantic_version(running_version, "running Jet version")?;
    let minimum = parse_semantic_version(min_version, "manifest minimum version")?;
    if running < minimum {
        return Err(UpdateError::new(format!(
            "running Jet {} is older than this channel's minimum supported version {}",
            running_version, min_version
        )));
    }
    let candidate = parse_semantic_version(version, "toolchain version")?;
    if candidate < running {
        return Err(UpdateError::new(format!(
            "refusing toolchain downgrade from {} to {}; no downgrade override is authorized",
            running_version, version
        )));
    }
    if expires_at <= published_at {
        return Err(UpdateError::new(
            "toolchain channel manifest expiry must be after publication",
        ));
    }
    if published_at == 0 || expires_at == 0 {
        return Err(UpdateError::new(
            "toolchain channel manifest timestamps must be positive",
        ));
    }
    if expires_at - published_at > MAX_MANIFEST_AGE_SECONDS {
        return Err(UpdateError::new(
            "toolchain channel manifest validity exceeds the freshness window",
        ));
    }
    if published_at > now.saturating_add(MAX_CLOCK_SKEW_SECONDS) {
        return Err(UpdateError::new(
            "toolchain channel manifest publication time is too far in the future",
        ));
    }
    if expires_at <= now {
        return Err(UpdateError::new("toolchain channel manifest has expired"));
    }
    if now.saturating_sub(published_at) > MAX_MANIFEST_AGE_SECONDS {
        return Err(UpdateError::new(
            "toolchain channel manifest is older than the freshness window",
        ));
    }
    Ok(())
}

fn unix_seconds(now: SystemTime) -> Result<u64, UpdateError> {
    now.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| UpdateError::new("system clock is before the Unix epoch"))
}

fn parse_semantic_version(value: &str, label: &str) -> Result<SemanticVersion, UpdateError> {
    let (without_build, build) = if let Some((without_build, build)) = value.split_once('+') {
        (without_build, Some(build))
    } else {
        (value, None)
    };
    if let Some(build) = build {
        validate_version_identifiers(build, label, false)?;
    }
    let (core, pre) = if let Some((core, pre)) = without_build.split_once('-') {
        (core, Some(pre))
    } else {
        (without_build, None)
    };
    let mut core_parts = core.split('.');
    let major = version_number(core_parts.next(), label)?;
    let minor = version_number(core_parts.next(), label)?;
    let patch = version_number(core_parts.next(), label)?;
    if core_parts.next().is_some() {
        return Err(UpdateError::new(format!("{label} must use major.minor.patch")));
    }
    let pre = pre
        .map(|value| parse_pre_identifiers(value, label))
        .transpose()?
        .unwrap_or_default();
    Ok(SemanticVersion {
        major,
        minor,
        patch,
        pre,
    })
}

fn version_number(value: Option<&str>, label: &str) -> Result<u64, UpdateError> {
    let value = value.ok_or_else(|| UpdateError::new(format!("{label} must use major.minor.patch")))?;
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(UpdateError::new(format!("{label} has an invalid numeric component")));
    }
    value
        .parse::<u64>()
        .map_err(|_| UpdateError::new(format!("{label} has an invalid numeric component")))
}

fn parse_pre_identifiers(value: &str, label: &str) -> Result<Vec<PreIdentifier>, UpdateError> {
    if value.is_empty() {
        return Err(UpdateError::new(format!("{label} has an empty prerelease")));
    }
    value
        .split('.')
        .map(|identifier| {
            if identifier.is_empty()
                || !identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return Err(UpdateError::new(format!("{label} has an invalid prerelease")));
            }
            if identifier.bytes().all(|byte| byte.is_ascii_digit()) {
                if identifier.len() > 1 && identifier.starts_with('0') {
                    return Err(UpdateError::new(format!(
                        "{label} has a leading-zero prerelease number"
                    )));
                }
                return identifier
                    .parse::<u64>()
                    .map(PreIdentifier::Numeric)
                    .map_err(|_| UpdateError::new(format!("{label} has an invalid prerelease")));
            }
            Ok(PreIdentifier::Text(identifier.to_string()))
        })
        .collect()
}

fn validate_version_identifiers(
    value: &str,
    label: &str,
    numeric_leading_zero_is_invalid: bool,
) -> Result<(), UpdateError> {
    if value.is_empty() {
        return Err(UpdateError::new(format!("{label} has an empty build")));
    }
    for identifier in value.split('.') {
        if identifier.is_empty()
            || !identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || (numeric_leading_zero_is_invalid
                && identifier.bytes().all(|byte| byte.is_ascii_digit())
                && identifier.len() > 1
                && identifier.starts_with('0'))
        {
            return Err(UpdateError::new(format!("{label} has an invalid build")));
        }
    }
    Ok(())
}

impl Ord for SemanticVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => self
                    .pre
                    .iter()
                    .zip(&other.pre)
                    .map(|(left, right)| match (left, right) {
                        (PreIdentifier::Numeric(left), PreIdentifier::Numeric(right)) => {
                            left.cmp(right)
                        }
                        (PreIdentifier::Numeric(_), PreIdentifier::Text(_)) => {
                            std::cmp::Ordering::Less
                        }
                        (PreIdentifier::Text(_), PreIdentifier::Numeric(_)) => {
                            std::cmp::Ordering::Greater
                        }
                        (PreIdentifier::Text(left), PreIdentifier::Text(right)) => {
                            left.cmp(right)
                        }
                    })
                    .find(|ordering| *ordering != std::cmp::Ordering::Equal)
                    .unwrap_or_else(|| self.pre.len().cmp(&other.pre.len())),
            })
    }
}

impl PartialOrd for SemanticVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_signature(bytes: &[u8], label: &str) -> Result<SignatureSidecar, UpdateError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| UpdateError::new(format!("{label} signature is not UTF-8")))?;
    let value = parse_json_exact_numbers(text, true).map_err(|error| {
        UpdateError::new(format!("parse {label} signature: {}", error.message))
    })?;
    let fields = object(&value, &format!("{label} signature"))?;
    reject_unknown(fields, &["schema", "key_id", "algorithm", "signature"])?;
    let sidecar = SignatureSidecar {
        schema: integer(field(fields, "schema")?, "signature schema")?,
        key_id: text_field(field(fields, "key_id")?, "signature key id")?.to_string(),
        algorithm: text_field(field(fields, "algorithm")?, "signature algorithm")?.to_string(),
        signature: text_field(field(fields, "signature")?, "signature bytes")?.to_string(),
    };
    if canonical_signature(&sidecar).as_bytes() != bytes {
        return Err(UpdateError::new(format!(
            "{label} signature bytes are not canonical"
        )));
    }
    Ok(sidecar)
}

fn verify_signature(
    public_key: &VerifyingKey,
    expected_key_id: &str,
    domain: &[u8],
    bytes: &[u8],
    sidecar_bytes: &[u8],
    label: &str,
) -> Result<(), UpdateError> {
    let sidecar = parse_signature(sidecar_bytes, label)?;
    if sidecar.schema != 1
        || sidecar.key_id != expected_key_id
        || sidecar.algorithm != "ed25519"
    {
        return Err(UpdateError::new(format!(
            "{label} signature key or algorithm is not trusted"
        )));
    }
    let signature = decode_base64(&sidecar.signature, false, false)
        .map_err(|_| UpdateError::new(format!("{label} signature is not base64")))?;
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|_| UpdateError::new(format!("{label} signature must be 64 bytes")))?;
    let mut message = Vec::with_capacity(domain.len() + bytes.len());
    message.extend_from_slice(domain);
    message.extend_from_slice(bytes);
    public_key
        .verify(&message, &Signature::from_bytes(&signature))
        .map_err(|_| UpdateError::new(format!("{label} signature verification failed")))
}

fn read_public_key(path: &Path) -> Result<(String, VerifyingKey), UpdateError> {
    let bytes = read_local_file(path, 4096, "toolchain public key")?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| UpdateError::new("toolchain public key is not UTF-8"))?;
    let key = PublicTrustKey::from_nix_line("toolchain", text.trim())
        .map_err(|error| UpdateError::new(format!("toolchain public key is invalid: {error}")))?;
    let decoded = decode_base64(&key.public_key, false, false)
        .map_err(|_| UpdateError::new("toolchain public key is not base64"))?;
    let decoded: [u8; 32] = decoded
        .try_into()
        .map_err(|_| UpdateError::new("toolchain public key must be 32 bytes"))?;
    let public_key = VerifyingKey::from_bytes(&decoded)
        .map_err(|_| UpdateError::new("toolchain public key is invalid Ed25519"))?;
    Ok((key.key_id, public_key))
}

fn parse_endpoint(raw: &str) -> Result<Endpoint, UpdateError> {
    if raw.is_empty()
        || raw.trim() != raw
        || raw
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        || raw.contains('?')
        || raw.contains('#')
    {
        return Err(UpdateError::new("toolchain endpoint is malformed"));
    }
    if let Some(path) = raw.strip_prefix("file://") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(UpdateError::new(
                "file toolchain endpoint must name an absolute directory",
            ));
        }
        validate_local_root(&path)?;
        return Ok(Endpoint::File(path));
    }
    let origin = parse_http_url(raw).map_err(UpdateError::new)?;
    if origin.scheme == "http" && !origin.is_loopback() {
        return Err(UpdateError::new(
            "plain HTTP toolchain endpoints must use loopback",
        ));
    }
    Ok(Endpoint::Http(HttpEndpoint {
        base: raw.trim_end_matches('/').to_string(),
        origin,
    }))
}

fn parse_http_url(raw: &str) -> Result<HttpOrigin, String> {
    let (scheme, rest) = raw
        .split_once("://")
        .ok_or_else(|| "toolchain URL must use http:// or https://".to_string())?;
    let default_port = match scheme {
        "http" => 80,
        "https" => 443,
        _ => return Err("toolchain URL must use http:// or https://".to_string()),
    };
    if rest.is_empty()
        || raw.contains('\\')
        || raw.contains('#')
        || raw
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("toolchain URL is malformed".to_string());
    }
    let authority_end = rest
        .find(|character| matches!(character, '/' | '?' | '#'))
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let suffix = &rest[authority_end..];
    if authority.is_empty()
        || (!suffix.is_empty() && !suffix.starts_with('/') && !suffix.starts_with('?'))
    {
        return Err("toolchain URL has an invalid authority or path".to_string());
    }
    let (host, port) = parse_http_authority(authority, default_port)?;
    Ok(HttpOrigin {
        scheme: scheme.to_string(),
        host,
        port,
    })
}

fn parse_http_authority(authority: &str, default_port: u16) -> Result<(HttpHost, u16), String> {
    if authority
        .chars()
        .any(|character| matches!(character, '@' | '%'))
    {
        return Err("toolchain URL has an invalid authority".to_string());
    }

    if authority.starts_with('[') {
        let closing = authority
            .find(']')
            .ok_or_else(|| "toolchain URL has an invalid IPv6 authority".to_string())?;
        let address = authority[1..closing]
            .parse::<Ipv6Addr>()
            .map(IpAddr::V6)
            .map_err(|_| "toolchain URL has an invalid IPv6 authority".to_string())?;
        let suffix = &authority[closing + 1..];
        let port = if suffix.is_empty() {
            default_port
        } else if let Some(port) = suffix.strip_prefix(':') {
            parse_http_port(port)?
        } else {
            return Err("toolchain URL has an invalid IPv6 authority".to_string());
        };
        return Ok((HttpHost::Address(address), port));
    }

    if authority
        .chars()
        .any(|character| matches!(character, '[' | ']'))
    {
        return Err("toolchain URL has an invalid authority".to_string());
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, parse_http_port(port)?),
        Some(_) => return Err("toolchain URL must bracket an IPv6 address".to_string()),
        None => (authority, default_port),
    };
    if host.is_empty() {
        return Err("toolchain URL has an empty host".to_string());
    }
    if let Ok(address) = host.parse::<IpAddr>() {
        return Ok((HttpHost::Address(address), port));
    }
    if host.len() > 253
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|character| character.is_ascii_alphanumeric() || character == b'-')
        })
    {
        return Err("toolchain URL has an invalid host".to_string());
    }
    Ok((HttpHost::Name(host.to_ascii_lowercase()), port))
}

fn parse_http_port(raw: &str) -> Result<u16, String> {
    if raw.is_empty() || !raw.bytes().all(|character| character.is_ascii_digit()) {
        return Err("toolchain URL has an invalid port".to_string());
    }
    raw.parse::<u16>()
        .map_err(|_| "toolchain URL has an invalid port".to_string())
}

impl HttpOrigin {
    fn is_loopback(&self) -> bool {
        match &self.host {
            HttpHost::Name(host) => host == "localhost",
            HttpHost::Address(address) => address.is_loopback(),
        }
    }

    fn authority(&self) -> String {
        let host = match &self.host {
            HttpHost::Name(host) => host.clone(),
            HttpHost::Address(IpAddr::V4(address)) => address.to_string(),
            HttpHost::Address(IpAddr::V6(address)) => format!("[{address}]"),
        };
        format!("{host}:{}", self.port)
    }
}

impl HttpEndpoint {
    fn url(&self, relative: &str) -> String {
        format!("{}/{relative}", self.base)
    }
}

fn resolve_toolchain_redirect(
    allowed: &HttpOrigin,
    current: &str,
    location: &str,
) -> Result<String, String> {
    let current_origin = parse_http_url(current)?;
    if current_origin != *allowed {
        return Err("toolchain redirect changed scheme or origin".to_string());
    }
    let next = if location.starts_with("https://") || location.starts_with("http://") {
        location.to_string()
    } else if location.starts_with("//") {
        format!("{}:{location}", current_origin.scheme)
    } else if location.starts_with('/') {
        format!(
            "{}://{}{}",
            current_origin.scheme,
            current_origin.authority(),
            location
        )
    } else {
        return Err(
            "toolchain redirect Location must be an absolute or root-relative URL".to_string(),
        );
    };
    if parse_http_url(&next)? != *allowed {
        return Err("toolchain redirect changed scheme or origin".to_string());
    }
    Ok(next)
}

fn fetch_bytes(
    endpoint: &Endpoint,
    relative: &str,
    limit: u64,
    label: &str,
) -> Result<Vec<u8>, UpdateError> {
    validate_relative_path(relative)?;
    match endpoint {
        Endpoint::File(root) => read_local_path(root, relative, limit, label),
        Endpoint::Http(http) => {
            // Keep the updater's offline gate while sharing jet-net transport.
            if std::env::var_os("JETPACK_DENY_NETWORK").is_some_and(|value| !value.is_empty()) {
                return Err(UpdateError::new(format!(
                    "network disabled by JETPACK_DENY_NETWORK while trying to {label}"
                )));
            }
            let url = http.url(relative);
            jet_net::fetch_bounded_with_redirect_policy(
                &url,
                Duration::from_secs(120),
                5,
                limit,
                |current, location| resolve_toolchain_redirect(&http.origin, current, location),
            )
            .map_err(|error| UpdateError::new(format!("could not fetch {label}: {error}")))
        }
    }
}

fn endpoint_url(endpoint: &Endpoint, relative: &str) -> String {
    match endpoint {
        Endpoint::File(root) => format!("file://{}", root.join(relative).display()),
        Endpoint::Http(http) => http.url(relative),
    }
}

fn read_local_path(
    root: &Path,
    relative: &str,
    limit: u64,
    label: &str,
) -> Result<Vec<u8>, UpdateError> {
    let mut path = root.to_path_buf();
    for component in relative.split('/') {
        path.push(component);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| UpdateError::new(format!("read {label}: {error}")))?;
        if metadata.file_type().is_symlink() {
            return Err(UpdateError::new(format!("{label} path traverses a symlink")));
        }
    }
    read_local_file(&path, limit, label)
}

fn read_local_file(path: &Path, limit: u64, label: &str) -> Result<Vec<u8>, UpdateError> {
    validate_no_symlink_ancestors(path, label)?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| UpdateError::new(format!("read {label}: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(UpdateError::new(format!("{label} is not a regular file")));
    }
    if metadata.len() > limit {
        return Err(UpdateError::new(format!("{label} exceeds its bound")));
    }
    let mut file = fs::File::open(path)
        .map_err(|error| UpdateError::new(format!("open {label}: {error}")))?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| UpdateError::new(format!("read {label}: {error}")))?;
    if bytes.len() as u64 > limit {
        return Err(UpdateError::new(format!("{label} exceeds its bound")));
    }
    Ok(bytes)
}

fn read_state(path: &Path) -> Result<Option<UpdateState>, UpdateError> {
    if path.as_os_str().is_empty() {
        return Err(UpdateError::new("toolchain update state path is empty"));
    }
    validate_no_symlink_ancestors(path, "toolchain update state")?;
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(UpdateError::new(
                    "toolchain update state is not a regular file",
                ));
            }
            let bytes = read_local_file(path, MAX_STATE_BYTES, "toolchain update state")?;
            parse_state(&bytes).map(Some)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(UpdateError::new(format!(
            "inspect toolchain update state: {error}"
        ))),
    }
}

fn parse_state(bytes: &[u8]) -> Result<UpdateState, UpdateError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| UpdateError::new("toolchain update state is not UTF-8"))?;
    let value = parse_json_exact_numbers(text, true).map_err(|error| {
        UpdateError::new(format!("parse toolchain update state: {}", error.message))
    })?;
    let fields = object(&value, "toolchain update state")?;
    reject_unknown(
        fields,
        &["schema", "channel", "platform", "sequence", "version", "sha256"],
    )?;
    let state = UpdateState {
        schema: integer(field(fields, "schema")?, "state schema")?,
        channel: text_field(field(fields, "channel")?, "state channel")?.to_string(),
        platform: text_field(field(fields, "platform")?, "state platform")?.to_string(),
        sequence: integer(field(fields, "sequence")?, "state sequence")?,
        version: text_field(field(fields, "version")?, "state version")?.to_string(),
        sha256: text_field(field(fields, "sha256")?, "state digest")?.to_string(),
    };
    if state.schema != 1 || state.sequence == 0 {
        return Err(UpdateError::new("toolchain update state schema is unsupported"));
    }
    validate_component(&state.channel, "state channel")?;
    validate_component(&state.platform, "state platform")?;
    parse_semantic_version(&state.version, "state version")?;
    validate_digest(&state.sha256, "state digest")?;
    if canonical_state(&state).as_bytes() != bytes {
        return Err(UpdateError::new("toolchain update state is not canonical"));
    }
    Ok(state)
}

fn write_state(path: &Path, state: &UpdateState) -> Result<(), UpdateError> {
    if path.as_os_str().is_empty() {
        return Err(UpdateError::new("toolchain update state path is empty"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| UpdateError::new("toolchain update state has no parent"))?;
    validate_no_symlink_ancestors(parent, "toolchain update state directory")?;
    fs::create_dir_all(parent)
        .map_err(|error| UpdateError::new(format!("create toolchain state directory: {error}")))?;
    validate_no_symlink_ancestors(path, "toolchain update state")?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(UpdateError::new(
                "toolchain update state is not a regular file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(UpdateError::new(format!(
                "inspect toolchain update state: {error}"
            )))
        }
    }
    let partial = parent.join(format!(
        ".{}.partial-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("toolchain-state"),
        std::process::id()
    ));
    match fs::symlink_metadata(&partial) {
        Ok(_) => {
            return Err(UpdateError::new(
                "a previous toolchain update state write is still present",
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(UpdateError::new(format!(
                "inspect partial toolchain update state: {error}"
            )))
        }
    }
    let mut created = false;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
            .map_err(|error| UpdateError::new(format!("stage toolchain update state: {error}")))?;
        created = true;
        file.write_all(canonical_state(state).as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|error| UpdateError::new(format!("write toolchain update state: {error}")))?;
        drop(file);
        Ok(())
    })();
    if let Err(error) = result {
        if created {
            let _ = fs::remove_file(&partial);
        }
        return Err(error);
    }
    #[cfg(windows)]
    {
        if let Err(error) = windows_helper::replace(&partial, path) {
            let _ = fs::remove_file(&partial);
            return Err(UpdateError::new(format!(
                "activate toolchain update state: {error}"
            )));
        }
        return sync_parent(parent);
    }
    #[cfg(not(windows))]
    {
        fs::rename(&partial, path).map_err(|error| {
            let _ = fs::remove_file(&partial);
            UpdateError::new(format!("activate toolchain update state: {error}"))
        })?;
        sync_parent(parent)
    }
}

fn canonical_state(state: &UpdateState) -> String {
    format!(
        "{{\"schema\":{},\"channel\":{},\"platform\":{},\"sequence\":{},\"version\":{},\"sha256\":{}}}\n",
        state.schema,
        quote(&state.channel),
        quote(&state.platform),
        state.sequence,
        quote(&state.version),
        quote(&state.sha256),
    )
}

fn validate_no_symlink_ancestors(path: &Path, label: &str) -> Result<(), UpdateError> {
    if path.as_os_str().is_empty() {
        return Err(UpdateError::new(format!("{label} path is empty")));
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(UpdateError::new(format!("{label} path is unsafe")));
    }
    let mut probe = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| UpdateError::new(format!("inspect {label}: {error}")))?
            .join(path)
    };
    let mut descendant_requires_directory = false;
    loop {
        match fs::symlink_metadata(&probe) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(UpdateError::new(format!("{label} path traverses a symlink")));
                }
                if descendant_requires_directory && !metadata.is_dir() {
                    return Err(UpdateError::new(format!(
                        "{label} path has a non-directory ancestor"
                    )));
                }
                descendant_requires_directory = true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                descendant_requires_directory = true;
            }
            Err(error) => {
                return Err(UpdateError::new(format!("inspect {label}: {error}")))
            }
        }
        let Some(parent) = probe.parent() else {
            break;
        };
        if parent == probe.as_path() || parent.as_os_str().is_empty() {
            break;
        }
        probe = parent.to_path_buf();
    }
    Ok(())
}

fn validate_local_root(path: &Path) -> Result<(), UpdateError> {
    validate_no_symlink_ancestors(path, "toolchain endpoint")?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| UpdateError::new(format!("inspect toolchain endpoint: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(UpdateError::new(
            "file toolchain endpoint is not a real directory",
        ));
    }
    Ok(())
}

fn validate_component(value: &str, label: &str) -> Result<(), UpdateError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || b"._+-".contains(&byte)))
    {
        return Err(UpdateError::new(format!("{label} contains unsafe characters")));
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<(), UpdateError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('%')
        || path.chars().any(|character| character.is_control())
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(UpdateError::new("toolchain path is unsafe"));
    }
    Ok(())
}

fn validate_digest(value: &str, label: &str) -> Result<(), UpdateError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(UpdateError::new(format!("{label} is malformed")));
    }
    Ok(())
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a [(String, Value)], UpdateError> {
    match value {
        Value::Object(fields) => Ok(fields),
        _ => Err(UpdateError::new(format!("{label} must be an object"))),
    }
}

fn array<'a>(value: &'a Value, label: &str) -> Result<&'a [Value], UpdateError> {
    match value {
        Value::Array(values) => Ok(values),
        _ => Err(UpdateError::new(format!("{label} must be an array"))),
    }
}

fn field<'a>(fields: &'a [(String, Value)], name: &str) -> Result<&'a Value, UpdateError> {
    fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .ok_or_else(|| UpdateError::new(format!("missing toolchain field {name}")))
}

fn reject_unknown(fields: &[(String, Value)], allowed: &[&str]) -> Result<(), UpdateError> {
    if let Some((name, _)) = fields
        .iter()
        .find(|(name, _)| !allowed.iter().any(|allowed| *allowed == name))
    {
        return Err(UpdateError::new(format!("unknown toolchain field {name}")));
    }
    Ok(())
}

fn text_field<'a>(value: &'a Value, label: &str) -> Result<&'a str, UpdateError> {
    match value {
        Value::Text(value) => Ok(value),
        _ => Err(UpdateError::new(format!("{label} must be text"))),
    }
}

fn integer(value: &Value, label: &str) -> Result<u64, UpdateError> {
    let value = match value {
        Value::Number(value) => value,
        Value::Int(value) => {
            return u64::try_from(*value).map_err(|_| {
                UpdateError::new(format!("{label} must be a non-negative integer"))
            })
        }
        _ => return Err(UpdateError::new(format!("{label} must be an integer"))),
    };
    value
        .parse::<u64>()
        .map_err(|_| UpdateError::new(format!("{label} must be a non-negative integer")))
}

fn canonical_manifest(manifest: &ChannelManifest) -> String {
    let artifacts = manifest
        .artifacts
        .iter()
        .map(|artifact| {
            format!(
                "{{\"target\":{},\"path\":{},\"sha256\":{},\"size\":{},\"signature\":{}}}",
                quote(&artifact.target),
                quote(&artifact.path),
                quote(&artifact.sha256),
                artifact.size,
                quote(&artifact.signature),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":{},\"channel\":{},\"version\":{},\"sequence\":{},\"published_at\":{},\"expires_at\":{},\"min_version\":{},\"artifacts\":[{}]}}\n",
        manifest.schema,
        quote(&manifest.channel),
        quote(&manifest.version),
        manifest.sequence,
        manifest.published_at,
        manifest.expires_at,
        quote(&manifest.min_version),
        artifacts,
    )
}

fn canonical_signature(signature: &SignatureSidecar) -> String {
    format!(
        "{{\"schema\":{},\"key_id\":{},\"algorithm\":{},\"signature\":{}}}",
        signature.schema,
        quote(&signature.key_id),
        quote(&signature.algorithm),
        quote(&signature.signature),
    )
}

fn quote(value: &str) -> String {
    jet_foundation::JSON::quote(value)
}

/// Return the target spelling used by the default release channel on this host.
pub fn default_target() -> String {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu".to_string(),
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu".to_string(),
        ("x86_64", "macos") => "x86_64-apple-darwin".to_string(),
        ("aarch64", "macos") => "aarch64-apple-darwin".to_string(),
        ("x86_64", "windows") => "x86_64-pc-windows-msvc".to_string(),
        ("aarch64", "windows") => "aarch64-pc-windows-msvc".to_string(),
        (arch, os) => format!("{arch}-unknown-{os}"),
    }
}

fn install_verified(
    current_exe: &Path,
    bytes: &[u8],
    plan: &UpdatePlan,
    state_path: &Path,
    running_version: &str,
) -> Result<InstallOutcome, UpdateError> {
    if plan.size == 0
        || plan.size > MAX_ARTIFACT_BYTES
        || bytes.len() as u64 != plan.size
        || crate::SHA256::sha256_hex(bytes) != plan.sha256
    {
        return Err(UpdateError::new(
            "verified toolchain artifact changed before installation",
        ));
    }
    validate_release_policy(
        &plan.version,
        &plan.min_version,
        plan.published_at,
        plan.expires_at,
        running_version,
        unix_seconds(SystemTime::now())?,
    )?;
    let metadata = fs::symlink_metadata(current_exe)
        .map_err(|error| UpdateError::new(format!("inspect current Jet executable: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(UpdateError::new(
            "current Jet executable is not a regular file",
        ));
    }
    let parent = current_exe
        .parent()
        .ok_or_else(|| UpdateError::new("current Jet executable has no parent"))?;
    validate_no_symlink_ancestors(current_exe, "current Jet executable")?;
    validate_no_symlink_ancestors(parent, "current Jet executable directory")?;
    validate_no_symlink_ancestors(state_path, "toolchain update state")?;
    let lock_root = state_path
        .parent()
        .ok_or_else(|| UpdateError::new("toolchain update state has no lock parent"))?;
    let _update_lock = crate::RuntimePolicy::acquire_lock(lock_root, "toolchain-update")
        .map_err(|error| UpdateError::new(format!("lock toolchain update: {error}")))?;
    #[cfg(unix)]
    {
        let state_before = read_state(state_path)?;
        install_unix(
            current_exe,
            parent,
            metadata.permissions(),
            bytes,
            plan,
            state_path,
            state_before,
            running_version,
        )
    }
    #[cfg(windows)]
    {
        install_windows_deferred(
            current_exe,
            parent,
            bytes,
            plan,
            state_path,
            running_version,
        )
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (current_exe, parent, metadata, bytes, plan, state_path);
        Err(UpdateError::new(
            "self-update installation is unsupported on this platform",
        ))
    }
}

fn reject_existing(path: &Path, label: &str) -> Result<(), UpdateError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(UpdateError::new(format!(
            "a previous {label} is still present"
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(UpdateError::new(format!("inspect {label}: {error}"))),
    }
}

#[cfg(unix)]
fn stage_executable(
    staged: &Path,
    bytes: &[u8],
    permissions: fs::Permissions,
) -> Result<(), UpdateError> {
    reject_existing(staged, "self-update staging file")?;
    validate_no_symlink_ancestors(staged, "self-update staging file")?;
    let mut created = false;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(staged)
            .map_err(|error| UpdateError::new(format!("stage Jet executable: {error}")))?;
        created = true;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| UpdateError::new(format!("write staged Jet executable: {error}")))?;
        fs::set_permissions(staged, permissions).map_err(|error| {
            UpdateError::new(format!("set staged Jet executable permissions: {error}"))
        })?;
        Ok(())
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(staged);
    }
    result
}

#[cfg(unix)]
fn install_unix(
    current_exe: &Path,
    parent: &Path,
    permissions: fs::Permissions,
    bytes: &[u8],
    plan: &UpdatePlan,
    state_path: &Path,
    state_before: Option<UpdateState>,
    running_version: &str,
) -> Result<InstallOutcome, UpdateError> {
    let staged = parent.join(format!(".jet-update-{}.new", std::process::id()));
    let rollback = parent.join(format!(".jet-update-{}.old", std::process::id()));
    reject_existing(&staged, "self-update staging file")?;
    reject_existing(&rollback, "self-update rollback file")?;
    validate_state_transition(
        state_before.as_ref(),
        &plan.channel,
        &plan.platform,
        plan.sequence,
        &plan.version,
    )?;
    stage_executable(&staged, bytes, permissions)?;
    let result = (|| {
        validate_release_policy(
            &plan.version,
            &plan.min_version,
            plan.published_at,
            plan.expires_at,
            running_version,
            unix_seconds(SystemTime::now())?,
        )?;
        health_check(&staged)?;
        fs::hard_link(current_exe, &rollback).map_err(|error| {
            UpdateError::new(format!("preserve current Jet executable for rollback: {error}"))
        })?;
        if let Err(error) = fs::rename(&staged, current_exe) {
            let _ = fs::remove_file(&rollback);
            return Err(UpdateError::new(format!(
                "activate staged Jet executable: {error}"
            )));
        }
        if let Err(error) = sync_parent(parent) {
            return Err(rollback_unix(
                current_exe,
                &rollback,
                parent,
                state_path,
                state_before.as_ref(),
                error,
            ));
        }
        if let Err(error) = health_check(current_exe) {
            return Err(rollback_unix(
                current_exe,
                &rollback,
                parent,
                state_path,
                state_before.as_ref(),
                error,
            ));
        }
        let state = state_from_plan(plan);
        if let Err(error) = write_state(state_path, &state) {
            return Err(rollback_unix(
                current_exe,
                &rollback,
                parent,
                state_path,
                state_before.as_ref(),
                error,
            ));
        }
        // The new image and state are committed. A cleanup failure keeps the
        // rollback copy as a recoverable safety net and must not report a
        // failed update after the new path is already active.
        let _ = fs::remove_file(&rollback);
        let _ = sync_parent(parent);
        Ok(InstallOutcome::Applied)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result
}

#[cfg(unix)]
fn rollback_unix(
    current_exe: &Path,
    rollback: &Path,
    parent: &Path,
    state_path: &Path,
    state_before: Option<&UpdateState>,
    cause: UpdateError,
) -> UpdateError {
    let rollback_result = (|| {
        let metadata = fs::symlink_metadata(current_exe).map_err(|error| {
            UpdateError::new(format!("inspect activated Jet executable during rollback: {error}"))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(UpdateError::new(
                "activated Jet executable became unsafe during rollback",
            ));
        }
        fs::remove_file(current_exe)
            .map_err(|error| UpdateError::new(format!("remove failed Jet executable: {error}")))?;
        fs::rename(rollback, current_exe)
            .map_err(|error| UpdateError::new(format!("restore previous Jet executable: {error}")))?;
        sync_parent(parent)?;
        restore_state(state_path, state_before)
    })();
    match rollback_result {
        Ok(()) => UpdateError::new(format!("{cause}; previous Jet executable restored")),
        Err(error) => UpdateError::new(format!("{cause}; rollback failed: {error}")),
    }
}

fn state_from_plan(plan: &UpdatePlan) -> UpdateState {
    UpdateState {
        schema: 1,
        channel: plan.channel.clone(),
        platform: plan.platform.clone(),
        sequence: plan.sequence,
        version: plan.version.clone(),
        sha256: plan.sha256.clone(),
    }
}

fn validate_state_transition(
    previous: Option<&UpdateState>,
    channel: &str,
    platform: &str,
    sequence: u64,
    version: &str,
) -> Result<(), UpdateError> {
    if let Some(previous) = previous {
        if previous.channel != channel || previous.platform != platform {
            return Err(UpdateError::new(
                "toolchain update state does not match the requested channel and platform",
            ));
        }
        if sequence <= previous.sequence {
            return Err(UpdateError::new(format!(
                "toolchain channel sequence {} is not newer than accepted sequence {}",
                sequence, previous.sequence
            )));
        }
        let previous_version =
            parse_semantic_version(&previous.version, "accepted toolchain version")?;
        let candidate_version = parse_semantic_version(version, "toolchain version")?;
        if candidate_version < previous_version {
            return Err(UpdateError::new(format!(
                "refusing toolchain downgrade from accepted {} to {}; no downgrade override is authorized",
                previous.version, version
            )));
        }
    }
    Ok(())
}

fn restore_state(path: &Path, previous: Option<&UpdateState>) -> Result<(), UpdateError> {
    match previous {
        Some(previous) => write_state(path, previous),
        None => {
            match fs::symlink_metadata(path) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.is_file() {
                        return Err(UpdateError::new(
                            "toolchain update state became unsafe during rollback",
                        ));
                    }
                    fs::remove_file(path).map_err(|error| {
                        UpdateError::new(format!("remove new toolchain update state: {error}"))
                    })?;
                    if let Some(parent) = path.parent() {
                        sync_parent(parent)?;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(UpdateError::new(format!(
                        "inspect toolchain update state during rollback: {error}"
                    )))
                }
            }
            Ok(())
        }
    }
}

fn health_check(path: &Path) -> Result<(), UpdateError> {
    let status = Command::new(path)
        .arg("--version")
        .env_remove(WINDOWS_HELPER_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| UpdateError::new(format!("health-check staged Jet executable: {error}")))?;
    if !status.success() {
        return Err(UpdateError::new(format!(
            "health-check staged Jet executable exited with {status}"
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<(), UpdateError> {
    let directory = fs::File::open(parent)
        .map_err(|error| UpdateError::new(format!("open executable parent for fsync: {error}")))?;
    directory
        .sync_all()
        .map_err(|error| UpdateError::new(format!("fsync executable parent: {error}")))
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<(), UpdateError> {
    Ok(())
}

const WINDOWS_HELPER_ENV: &str = "JET_TOOLCHAIN_UPDATE_HELPER";

pub fn windows_update_helper_requested() -> bool {
    cfg!(windows) && std::env::var_os(WINDOWS_HELPER_ENV).is_some()
}

pub fn run_windows_update_helper() -> i32 {
    #[cfg(windows)]
    {
        match windows_helper::run() {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("jet self-update helper: {error}");
                1
            }
        }
    }
    #[cfg(not(windows))]
    {
        1
    }
}

#[cfg(windows)]
fn install_windows_deferred(
    current_exe: &Path,
    parent: &Path,
    bytes: &[u8],
    plan: &UpdatePlan,
    state_path: &Path,
    running_version: &str,
) -> Result<InstallOutcome, UpdateError> {
    let staged = parent.join(format!(".jet-update-{}.new.exe", std::process::id()));
    let rollback = parent.join(format!(".jet-update-{}.old.exe", std::process::id()));
    let helper = parent.join(format!(".jet-update-{}.helper.exe", std::process::id()));
    reject_existing(&staged, "self-update staging file")?;
    reject_existing(&rollback, "self-update rollback file")?;
    reject_existing(&helper, "self-update helper file")?;
    validate_no_symlink_ancestors(&staged, "self-update staging file")?;
    validate_no_symlink_ancestors(&rollback, "self-update rollback file")?;
    validate_no_symlink_ancestors(&helper, "self-update helper file")?;
    if let Err(error) = copy_regular_file(current_exe, &rollback, "self-update rollback file") {
        let _ = fs::remove_file(&rollback);
        return Err(error);
    }
    if let Err(error) = copy_regular_file(current_exe, &helper, "self-update helper file") {
        let _ = fs::remove_file(&rollback);
        return Err(error);
    }
    if let Err(error) = stage_windows_file(&staged, bytes) {
        let _ = fs::remove_file(&rollback);
        let _ = fs::remove_file(&helper);
        return Err(error);
    }
    let mut command = Command::new(&helper);
    command
        .env(WINDOWS_HELPER_ENV, "1")
        .env("JET_TOOLCHAIN_UPDATE_CURRENT", current_exe)
        .env("JET_TOOLCHAIN_UPDATE_STAGED", &staged)
        .env("JET_TOOLCHAIN_UPDATE_ROLLBACK", &rollback)
        .env("JET_TOOLCHAIN_UPDATE_STATE", state_path)
        .env("JET_TOOLCHAIN_UPDATE_CHANNEL", &plan.channel)
        .env("JET_TOOLCHAIN_UPDATE_PLATFORM", &plan.platform)
        .env("JET_TOOLCHAIN_UPDATE_SEQUENCE", plan.sequence.to_string())
        .env("JET_TOOLCHAIN_UPDATE_VERSION", &plan.version)
        .env(
            "JET_TOOLCHAIN_UPDATE_PUBLISHED_AT",
            plan.published_at.to_string(),
        )
        .env("JET_TOOLCHAIN_UPDATE_EXPIRES_AT", plan.expires_at.to_string())
        .env("JET_TOOLCHAIN_UPDATE_MIN_VERSION", &plan.min_version)
        .env("JET_TOOLCHAIN_UPDATE_RUNNING_VERSION", running_version)
        .env("JET_TOOLCHAIN_UPDATE_SIZE", plan.size.to_string())
        .env("JET_TOOLCHAIN_UPDATE_SHA256", &plan.sha256)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Err(error) = command.spawn() {
        let _ = fs::remove_file(&staged);
        let _ = fs::remove_file(&rollback);
        let _ = fs::remove_file(&helper);
        return Err(UpdateError::new(format!(
            "start deferred Windows self-update helper: {error}"
        )));
    }
    Ok(InstallOutcome::Deferred)
}

#[cfg(windows)]
fn stage_windows_file(staged: &Path, bytes: &[u8]) -> Result<(), UpdateError> {
    reject_existing(staged, "self-update staging file")?;
    let mut created = false;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(staged)
            .map_err(|error| UpdateError::new(format!("stage Jet executable: {error}")))?;
        created = true;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| UpdateError::new(format!("write staged Jet executable: {error}")))
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(staged);
    }
    result
}

#[cfg(windows)]
fn copy_regular_file(source: &Path, destination: &Path, label: &str) -> Result<(), UpdateError> {
    validate_no_symlink_ancestors(source, label)?;
    validate_no_symlink_ancestors(destination, label)?;
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| UpdateError::new(format!("inspect {label}: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(UpdateError::new(format!("{label} source is not a regular file")));
    }
    reject_existing(destination, label)?;
    fs::copy(source, destination)
        .map_err(|error| UpdateError::new(format!("write {label}: {error}")))?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(destination)
        .map_err(|error| UpdateError::new(format!("open {label}: {error}")))?;
    file.sync_all()
        .map_err(|error| UpdateError::new(format!("flush {label}: {error}")))
}

#[cfg(windows)]
mod windows_helper {
    use super::*;
    use std::ffi::OsStr;
    use std::io;
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
    const MOVEFILE_DELAY_UNTIL_REBOOT: u32 = 0x0000_0004;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(source: *const u16, destination: *const u16, flags: u32) -> i32;
    }

    fn wide(path: &Path) -> Vec<u16> {
        OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub(super) fn replace(source: &Path, destination: &Path) -> io::Result<()> {
        let source = wide(source);
        let destination = wide(destination);
        // SAFETY: both vectors are NUL-terminated UTF-16 paths and remain
        // alive for the duration of the synchronous Win32 call.
        let result = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn schedule_delete(path: &Path) -> io::Result<()> {
        let path = wide(path);
        // SAFETY: the source vector is a NUL-terminated UTF-16 path and the
        // null destination requests the documented delete-on-reboot action.
        let result = unsafe {
            MoveFileExW(
                path.as_ptr(),
                std::ptr::null(),
                MOVEFILE_DELAY_UNTIL_REBOOT,
            )
        };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn env_path(name: &str) -> Result<PathBuf, UpdateError> {
        let value = std::env::var_os(name)
            .ok_or_else(|| UpdateError::new(format!("Windows helper is missing {name}")))?;
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(UpdateError::new(format!("Windows helper path {name} is not absolute")));
        }
        Ok(path)
    }

    fn env_text(name: &str) -> Result<String, UpdateError> {
        std::env::var(name)
            .map_err(|_| UpdateError::new(format!("Windows helper is missing {name}")))
    }

    fn env_sequence() -> Result<u64, UpdateError> {
        env_text("JET_TOOLCHAIN_UPDATE_SEQUENCE")?
            .parse::<u64>()
            .map_err(|_| UpdateError::new("Windows helper sequence is invalid"))
    }

    fn env_size() -> Result<u64, UpdateError> {
        let size = env_text("JET_TOOLCHAIN_UPDATE_SIZE")?
            .parse::<u64>()
            .map_err(|_| UpdateError::new("Windows helper artifact size is invalid"))?;
        if size == 0 || size > MAX_ARTIFACT_BYTES {
            return Err(UpdateError::new(
                "Windows helper artifact size is outside the channel bound",
            ));
        }
        Ok(size)
    }

    fn env_timestamp(name: &str) -> Result<u64, UpdateError> {
        let timestamp = env_text(name)?
            .parse::<u64>()
            .map_err(|_| UpdateError::new(format!("Windows helper timestamp {name} is invalid")))?;
        if timestamp == 0 {
            return Err(UpdateError::new(format!(
                "Windows helper timestamp {name} must be positive"
            )));
        }
        Ok(timestamp)
    }

    fn regular(path: &Path, label: &str) -> Result<fs::Metadata, UpdateError> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| UpdateError::new(format!("inspect {label}: {error}")))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(UpdateError::new(format!("{label} is not a regular file")));
        }
        Ok(metadata)
    }

    pub(super) fn run() -> Result<(), UpdateError> {
        let current = env_path("JET_TOOLCHAIN_UPDATE_CURRENT")?;
        let staged = env_path("JET_TOOLCHAIN_UPDATE_STAGED")?;
        let rollback = env_path("JET_TOOLCHAIN_UPDATE_ROLLBACK")?;
        let state_path = env_path("JET_TOOLCHAIN_UPDATE_STATE")?;
        let channel = env_text("JET_TOOLCHAIN_UPDATE_CHANNEL")?;
        let platform = env_text("JET_TOOLCHAIN_UPDATE_PLATFORM")?;
        let version = env_text("JET_TOOLCHAIN_UPDATE_VERSION")?;
        let published_at = env_timestamp("JET_TOOLCHAIN_UPDATE_PUBLISHED_AT")?;
        let expires_at = env_timestamp("JET_TOOLCHAIN_UPDATE_EXPIRES_AT")?;
        let min_version = env_text("JET_TOOLCHAIN_UPDATE_MIN_VERSION")?;
        let running_version = env_text("JET_TOOLCHAIN_UPDATE_RUNNING_VERSION")?;
        let size = env_size()?;
        let sha256 = env_text("JET_TOOLCHAIN_UPDATE_SHA256")?;
        let state = UpdateState {
            schema: 1,
            channel,
            platform,
            sequence: env_sequence()?,
            version,
            sha256,
        };
        validate_component(&state.channel, "Windows self-update channel")?;
        validate_component(&state.platform, "Windows self-update platform")?;
        parse_semantic_version(&state.version, "Windows self-update version")?;
        validate_digest(&state.sha256, "Windows self-update digest")?;
        if state.sequence == 0 {
            return Err(UpdateError::new(
                "Windows self-update sequence must be positive",
            ));
        }
        validate_release_policy(
            &state.version,
            &min_version,
            published_at,
            expires_at,
            &running_version,
            unix_seconds(SystemTime::now())?,
        )?;
        validate_no_symlink_ancestors(&current, "Windows self-update current executable")?;
        validate_no_symlink_ancestors(&staged, "Windows self-update staging file")?;
        validate_no_symlink_ancestors(&rollback, "Windows self-update rollback file")?;
        validate_no_symlink_ancestors(&state_path, "Windows self-update state")?;
        let lock_root = state_path
            .parent()
            .ok_or_else(|| UpdateError::new("Windows self-update state has no lock parent"))?;
        let _update_lock = crate::RuntimePolicy::acquire_lock(lock_root, "toolchain-update")
            .map_err(|error| UpdateError::new(format!("lock Windows self-update: {error}")))?;
        regular(&current, "Windows self-update current executable")?;
        let staged_metadata = regular(&staged, "Windows self-update staging file")?;
        if staged_metadata.len() != size {
            return Err(UpdateError::new(
                "Windows self-update staging file size disagrees with the verified artifact",
            ));
        }
        let staged_bytes = read_local_file(&staged, size, "Windows self-update staging file")?;
        if staged_bytes.len() as u64 != size
            || crate::SHA256::sha256_hex(&staged_bytes) != state.sha256
        {
            return Err(UpdateError::new(
                "Windows self-update staging file digest disagrees with the verified artifact",
            ));
        }
        regular(&rollback, "Windows self-update rollback file")?;
        let previous_state = read_state(&state_path)?;
        validate_state_transition(
            previous_state.as_ref(),
            &state.channel,
            &state.platform,
            state.sequence,
            &state.version,
        )?;
        let mut activated = false;
        let mut last_error = None;
        for _ in 0..WINDOWS_HANDOFF_ATTEMPTS {
            regular(&current, "Windows self-update current executable")?;
            match replace(&staged, &current) {
                Ok(()) => {
                    activated = true;
                    break;
                }
                Err(error) => {
                    last_error = Some(error);
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
        if !activated {
            return Err(UpdateError::new(format!(
                "Windows self-update could not acquire the old executable: {}",
                last_error
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "unknown error".to_string())
            )));
        }
        let result = (|| {
            regular(&current, "Windows self-update activated executable")?;
            health_check(&current)?;
            write_state(&state_path, &state)
        })();
        if let Err(error) = result {
            let rollback_error = (|| {
                fs::remove_file(&current).map_err(|rollback_error| {
                    UpdateError::new(format!(
                        "remove failed Windows Jet executable: {rollback_error}"
                    ))
                })?;
                replace(&rollback, &current).map_err(|rollback_error| {
                    UpdateError::new(format!(
                        "restore previous Windows Jet executable: {rollback_error}"
                    ))
                })?;
                restore_state(&state_path, previous_state.as_ref())
            })();
            return match rollback_error {
                Ok(()) => Err(UpdateError::new(format!(
                    "{error}; previous Windows Jet executable restored"
                ))),
                Err(rollback_error) => Err(UpdateError::new(format!(
                    "{error}; Windows rollback failed: {rollback_error}"
                ))),
            };
        }
        let _ = fs::remove_file(&rollback);
        let helper = std::env::current_exe().map_err(|error| {
            UpdateError::new(format!("locate Windows self-update helper: {error}"))
        })?;
        let _ = schedule_delete(&helper);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::sync::atomic::{AtomicU64, Ordering};

    const TARGET: &str = "x86_64-unknown-linux-gnu";
    const KEY_ID: &str = "local-channel-v1";
    static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

    struct TestTree(PathBuf);

    impl TestTree {
        fn new(label: &str) -> Self {
            let serial = NEXT_TREE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "jet-toolchain-update-{label}-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create toolchain update test tree");
            Self(path)
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn base64_encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let first = chunk[0];
            output.push(ALPHABET[(first >> 2) as usize] as char);
            output.push(
                ALPHABET[((first & 0x03) << 4 | chunk.get(1).copied().unwrap_or(0) >> 4) as usize]
                    as char,
            );
            if let Some(second) = chunk.get(1) {
                output.push(
                    ALPHABET[((second & 0x0f) << 2
                        | chunk.get(2).copied().unwrap_or(0) >> 6)
                        as usize] as char,
                );
            } else {
                output.push('=');
            }
            if let Some(third) = chunk.get(2) {
                output.push(ALPHABET[(third & 0x3f) as usize] as char);
            } else {
                output.push('=');
            }
        }
        output
    }

    fn sidecar(domain: &[u8], bytes: &[u8], signing_key: &SigningKey) -> Vec<u8> {
        let mut message = Vec::with_capacity(domain.len() + bytes.len());
        message.extend_from_slice(domain);
        message.extend_from_slice(bytes);
        canonical_signature(&SignatureSidecar {
            schema: 1,
            key_id: KEY_ID.into(),
            algorithm: "ed25519".into(),
            signature: base64_encode(&signing_key.sign(&message).to_bytes()),
        })
        .into_bytes()
    }

    fn write_publication(
        root: &Path,
        artifact_bytes: &[u8],
        signing_key: Option<&SigningKey>,
    ) -> PathBuf {
        let artifact_path = "v1/stable/1.2.3/jet-1.2.3-x86_64-unknown-linux-gnu";
        let artifact = root.join(artifact_path);
        fs::create_dir_all(artifact.parent().expect("artifact parent")).expect("create channel");
        fs::write(&artifact, artifact_bytes).expect("write artifact");
        let manifest = ChannelManifest {
            schema: 1,
            channel: "stable".into(),
            version: "1.2.3".into(),
            sequence: 1,
            published_at: unix_seconds(SystemTime::now()).expect("current test time") - 1,
            expires_at: unix_seconds(SystemTime::now()).expect("current test time") + 86_400,
            min_version: "1.0.0".into(),
            artifacts: vec![Artifact {
                target: TARGET.into(),
                path: artifact_path.into(),
                sha256: crate::SHA256::sha256_hex(artifact_bytes),
                size: artifact_bytes.len() as u64,
                signature: format!("{artifact_path}.sig.json"),
            }],
        };
        let manifest_path = root.join("v1/stable/manifest.json");
        let manifest_bytes = canonical_manifest(&manifest).into_bytes();
        fs::write(&manifest_path, &manifest_bytes).expect("write manifest");
        if let Some(signing_key) = signing_key {
            fs::write(
                manifest_path.with_file_name("manifest.json.sig.json"),
                sidecar(MANIFEST_DOMAIN, &manifest_bytes, signing_key),
            )
            .expect("write manifest signature");
            fs::write(
                artifact.with_file_name(
                    "jet-1.2.3-x86_64-unknown-linux-gnu.sig.json",
                ),
                sidecar(ARTIFACT_DOMAIN, artifact_bytes, signing_key),
            )
            .expect("write artifact signature");
        }
        artifact
    }

    fn options(
        root: &Path,
        trust_key: PathBuf,
        allow_unofficial: bool,
        apply: bool,
    ) -> UpdateOptions {
        UpdateOptions {
            endpoint: format!("file://{}", root.display()),
            channel: "stable".into(),
            platform: TARGET.into(),
            trust_key,
            dry_run: !apply,
            apply,
            allow_unofficial,
            running_version: "1.0.0".into(),
            state_path: root.join("state/toolchain.state"),
        }
    }

    fn install_plan() -> UpdatePlan {
        let now = unix_seconds(SystemTime::now()).expect("current test time");
        UpdatePlan {
            endpoint: "file://local".into(),
            channel: "stable".into(),
            version: "1.2.3".into(),
            platform: TARGET.into(),
            artifact_path: "v1/stable/1.2.3/jet-1.2.3-x86_64-unknown-linux-gnu".into(),
            artifact_url: "file://local/artifact".into(),
            sha256: crate::SHA256::sha256_hex(b"#!/bin/sh\nexit 0\n"),
            size: b"#!/bin/sh\nexit 0\n".len() as u64,
            key_id: None,
            trust: UpdateTrust::Signed,
            sequence: 1,
            published_at: now - 1,
            expires_at: now + 86_400,
            min_version: "1.0.0".into(),
        }
    }

    #[test]
    fn signed_channel_and_keyless_channel_are_distinct_explicit_tiers() {
        let tree = TestTree::new("trust");
        let signing_key = SigningKey::from_bytes(&[7; 32]);
        let trust_key = tree.0.join("toolchain.pub");
        fs::write(
            &trust_key,
            format!(
                "{KEY_ID}:{}\n",
                base64_encode(&signing_key.verifying_key().to_bytes())
            ),
        )
        .expect("write trust key");
        let artifact = write_publication(&tree.0, b"signed toolchain", Some(&signing_key));

        let signed = run(&options(&tree.0, trust_key.clone(), false, false), None)
            .expect("verify signed channel");
        assert_eq!(signed.plan.trust, UpdateTrust::Signed);
        assert_eq!(signed.plan.key_id.as_deref(), Some(KEY_ID));

        fs::remove_file(tree.0.join("v1/stable/manifest.json.sig.json"))
            .expect("remove manifest signature");
        fs::remove_file(format!("{}.sig.json", artifact.display())).expect("remove artifact signature");
        let refused = run(&options(&tree.0, trust_key.clone(), false, false), None)
            .expect_err("unsigned channel must be refused by default");
        assert!(refused.detail.contains("signature"));

        let unofficial = run(&options(&tree.0, trust_key, true, false), None)
            .expect("verify explicitly selected keyless channel");
        assert_eq!(unofficial.plan.trust, UpdateTrust::UnofficialKeyless);
        assert_eq!(unofficial.plan.key_id, None);
    }

    #[test]
    fn keyless_override_is_local_only() {
        let tree = TestTree::new("endpoint");
        let options = UpdateOptions {
            endpoint: DEFAULT_ENDPOINT.into(),
            channel: "stable".into(),
            platform: TARGET.into(),
            trust_key: tree.0.join("unused.pub"),
            dry_run: true,
            apply: false,
            allow_unofficial: true,
            running_version: "1.0.0".into(),
            state_path: tree.0.join("state/toolchain.state"),
        };
        let error = run(&options, None).expect_err("HTTPS keyless source must be refused");
        assert!(error.detail.contains("file://"));

        let mut loopback = options;
        loopback.endpoint = "http://localhost:3210".into();
        let error = run(&loopback, None).expect_err("HTTP keyless source must be refused");
        assert!(error.detail.contains("file://"));
    }

    #[test]
    fn endpoint_authority_is_structural_and_loopback_http_is_exact() {
        for endpoint in [
            "http://localhost@evil.example",
            "http://localhost:80@evil.example",
            "https://localhost:443@evil.example",
            "http://[::1]@evil.example",
            "http://localhost%40evil.example",
            "https://localhost%40evil.example",
        ] {
            assert!(
                parse_endpoint(endpoint).is_err(),
                "userinfo or encoded authority must not be accepted: {endpoint}"
            );
        }

        for endpoint in [
            "http://localhost:3210",
            "http://127.0.0.1:3210",
            "http://[::1]:3210",
        ] {
            assert!(
                matches!(parse_endpoint(endpoint), Ok(Endpoint::Http(_))),
                "valid loopback endpoint must be accepted: {endpoint}"
            );
        }
        assert!(parse_endpoint("http://2001:db8::1:3210").is_err());
        assert!(parse_endpoint("https://[2001:db8::1]:443").is_ok());
    }

    #[test]
    fn official_redirect_policy_preserves_https_origin_across_every_hop() {
        let Endpoint::Http(endpoint) =
            parse_endpoint("https://dl.jet-lang.dev").expect("endpoint")
        else {
            panic!("HTTPS endpoint must be HTTP endpoint");
        };
        let first = resolve_toolchain_redirect(
            &endpoint.origin,
            "https://dl.jet-lang.dev/v1/stable/manifest.json",
            "/v1/stable/manifest.json.sig.json",
        )
        .expect("same-origin redirect");
        let second = resolve_toolchain_redirect(
            &endpoint.origin,
            &first,
            "https://dl.jet-lang.dev/v1/stable/artifact",
        )
        .expect("second same-origin redirect");
        assert_eq!(second, "https://dl.jet-lang.dev/v1/stable/artifact");

        for location in [
            "http://dl.jet-lang.dev/v1/stable/artifact",
            "https://evil.example/v1/stable/artifact",
            "//evil.example/v1/stable/artifact",
            "https://dl.jet-lang.dev:444/v1/stable/artifact",
        ] {
            assert!(
                resolve_toolchain_redirect(&endpoint.origin, &second, location).is_err(),
                "unsafe redirect must be rejected: {location}"
            );
        }
        assert!(
            resolve_toolchain_redirect(&endpoint.origin, "https://evil.example/start", "/safe")
                .is_err()
        );
    }

    #[test]
    fn digest_failure_leaves_current_executable_unchanged() {
        let tree = TestTree::new("rollback");
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let trust_key = tree.0.join("toolchain.pub");
        fs::write(
            &trust_key,
            format!(
                "{KEY_ID}:{}\n",
                base64_encode(&signing_key.verifying_key().to_bytes())
            ),
        )
        .expect("write trust key");
        let artifact = write_publication(&tree.0, b"verified bytes", Some(&signing_key));
        fs::write(&artifact, b"corrupt! bytes").expect("corrupt artifact");
        let current = tree.0.join("jet");
        fs::write(&current, b"current toolchain").expect("write current executable");

        let error = run(&options(&tree.0, trust_key, false, true), Some(&current))
            .expect_err("digest mismatch must abort before install");
        assert!(error.detail.contains("digest"));
        assert_eq!(fs::read(&current).expect("read current executable"), b"current toolchain");
        assert!(!tree.0.join(format!(".jet-update-{}.new", std::process::id())).exists());
    }

    #[cfg(unix)]
    #[test]
    fn install_replaces_current_path_without_backup_or_partial_file() {
        let tree = TestTree::new("install");
        let current = tree.0.join("jet");
        fs::write(&current, b"#!/bin/sh\nexit 0\n").expect("write current executable");
        let mut permissions = fs::metadata(&current)
            .expect("read current executable metadata")
            .permissions();
        #[cfg(unix)]
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        fs::set_permissions(&current, permissions).expect("make current executable runnable");
        let replacement = b"#!/bin/sh\nexit 0\n";
        let plan = install_plan();
        let state_path = tree.0.join("state/toolchain.state");

        install_verified(&current, replacement, &plan, &state_path, "1.0.0")
            .expect("install verified executable");

        assert_eq!(fs::read(&current).expect("read installed executable"), replacement);
        assert!(!tree.0.join(format!(".jet-update-{}.new", std::process::id())).exists());
        assert!(!tree.0.join(format!(".jet-update-{}.old", std::process::id())).exists());
        assert!(state_path.exists());
    }

    #[test]
    fn preexisting_staging_file_is_not_removed() {
        let tree = TestTree::new("staging-collision");
        let current = tree.0.join("jet");
        let staged = tree.0.join(format!(".jet-update-{}.new", std::process::id()));
        fs::write(&current, b"current toolchain").expect("write current executable");
        fs::write(&staged, b"unrelated staged bytes").expect("write collision file");

        let error = install_verified(
            &current,
            b"#!/bin/sh\nexit 0\n",
            &install_plan(),
            &tree.0.join("state/toolchain.state"),
            "1.0.0",
        )
            .expect_err("an existing staging path must stop the update");
        assert!(error.detail.contains("staging file"));
        assert_eq!(fs::read(&current).expect("read current executable"), b"current toolchain");
        assert_eq!(fs::read(&staged).expect("read collision file"), b"unrelated staged bytes");
    }

    #[test]
    fn apply_rejects_a_cross_platform_selection_before_resolution() {
        let tree = TestTree::new("cross-platform");
        let mut options = options(&tree.0, tree.0.join("unused.pub"), true, true);
        options.platform = if default_target() == TARGET {
            "aarch64-unknown-linux-gnu".into()
        } else {
            TARGET.into()
        };
        let error = run_at(&options, None, SystemTime::now())
            .expect_err("cross-platform apply must be rejected");
        assert!(error.detail.contains("exact host platform"));
    }

    #[test]
    fn manifest_policy_rejects_expired_future_stale_and_downgrade() {
        let tree = TestTree::new("policy");
        let options = options(&tree.0, tree.0.join("unused.pub"), true, false);
        let now = MAX_MANIFEST_AGE_SECONDS + 1_000_000;
        let base = ChannelManifest {
            schema: 1,
            channel: "stable".into(),
            version: "1.2.3".into(),
            sequence: 1,
            published_at: now - 1,
            expires_at: now + 100,
            min_version: "1.0.0".into(),
            artifacts: Vec::new(),
        };
        validate_manifest_policy(&base, &options, now).expect("fresh manifest policy");

        let mut expired = base.clone();
        expired.expires_at = now;
        assert!(validate_manifest_policy(&expired, &options, now)
            .expect_err("expired manifest must fail")
            .detail
            .contains("expired"));

        let mut future = base.clone();
        future.published_at = now + MAX_CLOCK_SKEW_SECONDS + 1;
        future.expires_at = future.published_at + 100;
        assert!(validate_manifest_policy(&future, &options, now)
            .expect_err("future manifest must fail")
            .detail
            .contains("future"));

        let mut stale = base.clone();
        stale.published_at = now - MAX_MANIFEST_AGE_SECONDS - 1;
        assert!(validate_manifest_policy(&stale, &options, now)
            .expect_err("stale manifest must fail")
            .detail
            .contains("freshness"));

        let mut downgrade = base;
        downgrade.version = "0.9.0".into();
        assert!(validate_manifest_policy(&downgrade, &options, now)
            .expect_err("downgrade must fail")
            .detail
            .contains("downgrade"));

        let mut too_new_minimum = ChannelManifest {
            version: "1.2.3".into(),
            min_version: "1.1.0".into(),
            ..downgrade
        };
        too_new_minimum.published_at = now - 1;
        too_new_minimum.expires_at = now + 100;
        assert!(validate_manifest_policy(&too_new_minimum, &options, now)
            .expect_err("minimum version floor must fail")
            .detail
            .contains("minimum supported"));
    }

    #[test]
    fn monotonic_state_rejects_a_replayed_sequence() {
        let tree = TestTree::new("replay");
        let options = options(&tree.0, tree.0.join("unused.pub"), true, false);
        let state = UpdateState {
            schema: 1,
            channel: "stable".into(),
            platform: TARGET.into(),
            sequence: 7,
            version: "1.2.0".into(),
            sha256: "a".repeat(64),
        };
        write_state(&options.state_path, &state).expect("write accepted state");
        let manifest = ChannelManifest {
            schema: 1,
            channel: "stable".into(),
            version: "1.2.3".into(),
            sequence: 7,
            published_at: 999_999,
            expires_at: 1_000_100,
            min_version: "1.0.0".into(),
            artifacts: Vec::new(),
        };
        let error = validate_manifest_policy(&manifest, &options, 1_000_000)
            .expect_err("replayed sequence must fail");
        assert!(error.detail.contains("not newer"));

        let mut lower_version = manifest;
        lower_version.sequence = 8;
        lower_version.version = "1.1.9".into();
        let error = validate_manifest_policy(&lower_version, &options, 1_000_000)
            .expect_err("a newer sequence must not carry an older version");
        assert!(error.detail.contains("downgrade"));
    }
}
