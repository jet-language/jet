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
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_ENDPOINT: &str = "https://dl.jet-lang.dev";
pub const DEFAULT_CHANNEL: &str = "stable";
pub const ENDPOINT_ENV: &str = "JET_TOOLCHAIN_ENDPOINT";
pub const MANIFEST_DOMAIN: &[u8] = b"jet-toolchain-channel-v1\n";
pub const ARTIFACT_DOMAIN: &[u8] = b"jet-toolchain-artifact-v1\n";

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_SIGNATURE_BYTES: u64 = 16 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOptions {
    pub endpoint: String,
    pub channel: String,
    pub platform: String,
    pub trust_key: PathBuf,
    pub dry_run: bool,
    pub apply: bool,
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
    pub key_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateResult {
    pub plan: UpdatePlan,
    pub applied: bool,
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
    Http(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelManifest {
    schema: u64,
    channel: String,
    version: String,
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

/// Verify the configured channel and, when requested, install the verified
/// artifact. Without apply, this is a complete dry-run verification.
pub fn run(
    options: &UpdateOptions,
    current_exe: Option<&Path>,
) -> Result<UpdateResult, UpdateError> {
    if options.apply && options.dry_run {
        return Err(UpdateError::new("apply and dry-run cannot be combined"));
    }
    let (plan, artifact) = resolve(options)?;
    if options.apply {
        let current_exe = current_exe.ok_or_else(|| {
            UpdateError::new("self-update installation needs the current executable path")
        })?;
        install_verified(current_exe, &artifact)?;
    }
    Ok(UpdateResult {
        plan,
        applied: options.apply,
    })
}

fn resolve(options: &UpdateOptions) -> Result<(UpdatePlan, Vec<u8>), UpdateError> {
    validate_component(&options.channel, "channel")?;
    validate_component(&options.platform, "platform")?;
    let endpoint = parse_endpoint(&options.endpoint)?;
    let (key_id, public_key) = read_public_key(&options.trust_key)?;
    let manifest_path = format!("v1/{}/manifest.json", options.channel);
    let manifest_bytes = fetch_bytes(
        &endpoint,
        &manifest_path,
        MAX_MANIFEST_BYTES,
        "toolchain channel manifest",
    )?;
    let manifest_signature_path = format!("{manifest_path}.sig.json");
    let manifest_signature = fetch_bytes(
        &endpoint,
        &manifest_signature_path,
        MAX_SIGNATURE_BYTES,
        "toolchain channel manifest signature",
    )?;
    verify_signature(
        &public_key,
        &key_id,
        MANIFEST_DOMAIN,
        &manifest_bytes,
        &manifest_signature,
        "toolchain channel manifest",
    )?;
    let manifest = parse_manifest(&manifest_bytes, &options.channel)?;
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
    let artifact_signature = fetch_bytes(
        &endpoint,
        &artifact.signature,
        MAX_SIGNATURE_BYTES,
        "toolchain artifact signature",
    )?;
    verify_signature(
        &public_key,
        &key_id,
        ARTIFACT_DOMAIN,
        &artifact_bytes,
        &artifact_signature,
        "toolchain artifact",
    )?;
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
            key_id,
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
    reject_unknown(fields, &["schema", "channel", "version", "artifacts"])?;
    let manifest = ChannelManifest {
        schema: integer(field(fields, "schema")?, "manifest schema")?,
        channel: text_field(field(fields, "channel")?, "manifest channel")?.to_string(),
        version: text_field(field(fields, "version")?, "manifest version")?.to_string(),
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
    validate_component(&manifest.version, "toolchain version")?;
    if manifest.artifacts.is_empty() {
        return Err(UpdateError::new(
            "toolchain channel manifest has no artifacts",
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
    let base = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))
        .ok_or_else(|| UpdateError::new("toolchain endpoint must use https:// or file://"))?;
    let is_http = raw.starts_with("http://");
    let authority = base.split('/').next().unwrap_or_default();
    if authority.is_empty() || (is_http && !is_loopback_host(authority)) {
        return Err(UpdateError::new(
            "plain HTTP toolchain endpoints must use loopback",
        ));
    }
    Ok(Endpoint::Http(raw.trim_end_matches('/').to_string()))
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
        Endpoint::Http(base) => {
            let url = format!("{base}/{relative}");
            let response = jet_net::get_stream(&url, Duration::from_secs(120))
                .map_err(|error| UpdateError::new(format!("fetch {label}: {error}")))?;
            if response.status() != 200 {
                return Err(UpdateError::new(format!(
                    "{label} endpoint returned HTTP {}",
                    response.status()
                )));
            }
            let content_length = response.content_length();
            if content_length.is_some_and(|length| length > limit) {
                return Err(UpdateError::new(format!("{label} exceeds its bound")));
            }
            let max = usize::try_from(limit)
                .map_err(|_| UpdateError::new(format!("{label} bound is too large")))?;
            let mut bytes = Vec::new();
            response
                .take((max as u64).saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(|error| UpdateError::new(format!("read {label}: {error}")))?;
            if bytes.len() > max {
                return Err(UpdateError::new(format!("{label} exceeds its bound")));
            }
            if content_length.is_some_and(|length| length != bytes.len() as u64)
            {
                return Err(UpdateError::new(format!("{label} Content-Length disagrees")));
            }
            Ok(bytes)
        }
    }
}

fn endpoint_url(endpoint: &Endpoint, relative: &str) -> String {
    match endpoint {
        Endpoint::File(root) => format!("file://{}", root.join(relative).display()),
        Endpoint::Http(base) => format!("{base}/{relative}"),
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

fn validate_local_root(path: &Path) -> Result<(), UpdateError> {
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

fn is_loopback_host(authority: &str) -> bool {
    let host = authority
        .strip_prefix('[')
        .and_then(|value| value.split(']').next())
        .unwrap_or_else(|| authority.split(':').next().unwrap_or(authority));
    matches!(host, "127.0.0.1" | "localhost" | "::1")
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
        "{{\"schema\":{},\"channel\":{},\"version\":{},\"artifacts\":[{}]}}",
        manifest.schema,
        quote(&manifest.channel),
        quote(&manifest.version),
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

fn install_verified(current_exe: &Path, bytes: &[u8]) -> Result<(), UpdateError> {
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
    let staged = parent.join(format!(".jet-update-{}.new", std::process::id()));
    let backup = parent.join(format!(".jet-update-{}.old", std::process::id()));
    if fs::symlink_metadata(&staged).is_ok() || fs::symlink_metadata(&backup).is_ok() {
        return Err(UpdateError::new(
            "a previous self-update staging file is still present",
        ));
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)
        .map_err(|error| UpdateError::new(format!("stage Jet executable: {error}")))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| UpdateError::new(format!("write staged Jet executable: {error}")))?;
    #[cfg(unix)]
    {
        let permissions = metadata.permissions();
        fs::set_permissions(&staged, permissions).map_err(|error| {
            UpdateError::new(format!("set staged Jet executable permissions: {error}"))
        })?;
    }
    drop(file);
    fs::rename(current_exe, &backup)
        .map_err(|error| UpdateError::new(format!("stage current Jet executable: {error}")))?;
    match fs::rename(&staged, current_exe) {
        Ok(()) => {
            fs::remove_file(&backup).map_err(|error| {
                UpdateError::new(format!("remove previous Jet executable: {error}"))
            })?;
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(&backup, current_exe);
            let _ = fs::remove_file(&staged);
            Err(UpdateError::new(format!(
                "activate staged Jet executable: {error}"
            )))
        }
    }
}
