//! D-JPK-IMAGE1 (=A, ratified 2026-07-01, c9jetpackgates): the native `.Oci`
//! image-layout builder. Builds one deterministic native layer, optionally
//! appended to a validated local OCI base, directly from an already-realized
//! package binary plus optional `expose`/`env_vars`/`files` metadata
//! (`ImagePlan`, `ModuleEval::Types`) — no Docker or second image model.
//!
//! **Deterministic by construction** (this is the hard, budgeted part): every
//! tar entry uses a fixed mtime (Unix epoch 0), uid/gid 0, and no uname/gname;
//! entries are written in a caller-independent sorted-by-path order; no
//! wall-clock timestamp is embedded anywhere in the layer, config, manifest,
//! or index. The same `BuildSpec` therefore always produces byte-identical
//! blobs and the same manifest digest — proven by the `tests` below (build
//! twice, compare; reorder the input files, compare again).
//!
//! **Layer media type is uncompressed** (`application/vnd.oci.image.layer.v1.tar`,
//! not the `+gzip` variant) — both are valid, spec-compliant OCI layer types.
//! `core.compress.gzip`'s flate2 bridge (D-CORE-COMPRESS1) was evaluated for
//! gzip compression here first, but that runtime is emitted only into generated
//! user-program bridge crates, never linked into `jetpack` itself. Linking flate2 into
//! `jet-driver` directly would violate I6 (zero external crates in the
//! compiler proper, which `jetpack`/`jet-driver` are part of). Uncompressed
//! tar sidesteps needing a native DEFLATE implementation to ship a real image
//! today; gzip layers are a follow-up once/if a native (I6-safe) deflate
//! exists.
//!
//! OCI Image Format Spec layout implemented (the minimal uncompressed local
//! subset): `oci-layout`, `index.json`, and
//! `blobs/sha256/<digest>` for the layer tar, the image config, and the
//! manifest.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use super::JSON;
use crate::SHA256;

const ARCHITECTURE: &str = "amd64";
const OS: &str = "linux";
const MEDIA_TYPE_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const MEDIA_TYPE_CONFIG: &str = "application/vnd.oci.image.config.v1+json";
const MEDIA_TYPE_LAYER: &str = "application/vnd.oci.image.layer.v1.tar";
const MAX_REGISTRY_RESPONSE_BYTES: u64 = 64 * 1024;
static COPY_COUNTER: AtomicU64 = AtomicU64::new(1);

/// One file layered into the image, relative to the image root (no leading
/// `/`), e.g. `usr/local/bin/myapp`. `mode` is the tar entry's Unix
/// permission bits (e.g. `0o755` for the package binary, `0o644` for a
/// plain data file).
#[derive(Debug, Clone)]
pub struct LayerFile {
    pub path: String,
    pub data: Vec<u8>,
    pub mode: u32,
}

/// Everything needed to build one `.Oci` image (D-JPK-IMAGE1). Pure data —
/// no filesystem/network access happens until `build` runs. `files` need not
/// be pre-sorted; `build` sorts by path so declaration order never affects
/// the output.
#[derive(Debug, Clone)]
pub struct BuildSpec {
    pub files: Vec<LayerFile>,
    /// The container's `Entrypoint` (D-JPK-IMAGE1: the package binary's path
    /// inside the image, e.g. `["/usr/local/bin/myapp"]`).
    pub entrypoint: Vec<String>,
    /// `env_vars:` — rendered as `KEY=value` strings in the config's `Env`.
    pub env: Vec<(String, String)>,
    /// `expose:` — TCP ports, rendered as `ExposedPorts` keys (`"<port>/tcp"`).
    pub expose: Vec<i64>,
    /// D-ENV-IMAGE1: non-root container user. The default is a stable
    /// unprivileged UID; callers may choose a different explicit UID.
    pub user: u32,
    /// D-ENV-IMAGE1: optional health command rendered as an OCI
    /// `Healthcheck` record.
    pub healthcheck: Option<String>,
}

impl Default for BuildSpec {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            entrypoint: Vec::new(),
            env: Vec::new(),
            expose: Vec::new(),
            user: 10_001,
            healthcheck: None,
        }
    }
}

/// The result of a successful build: where the OCI layout landed and the
/// top-level manifest digest that identifies this exact image.
#[derive(Debug, Clone)]
pub struct BuiltImage {
    pub out_dir: PathBuf,
    /// `sha256:<hex>` — the image manifest's own digest.
    pub manifest_digest: String,
    /// Total bytes across the layer + config + manifest blobs.
    pub total_bytes: u64,
}

/// Result of publishing an already validated local OCI layout to an OCI
/// Distribution registry. Blobs are uploaded by digest before the manifest;
/// a failed upload therefore cannot make a partial image tag visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryPushReport {
    pub reference: String,
    pub manifest_digest: String,
    pub blobs_uploaded: usize,
    pub bytes_uploaded: u64,
}

/// Lossless, secret-free projection ledger written beside every built image.
/// The category names are facts, not prose: consumers can explain what the
/// environment image included, changed, omitted, or rejected without
/// re-evaluating source.
#[derive(Debug, Clone, Default)]
pub struct ProjectionReport {
    pub included: Vec<String>,
    pub changed: Vec<String>,
    pub omitted: Vec<String>,
    pub rejected: Vec<String>,
}

pub fn write_projection_report(
    layout: &Path,
    manifest_digest: &str,
    report: &ProjectionReport,
) -> io::Result<()> {
    let path = layout.join("projection.json");
    let render = |values: &[String]| {
        let mut values = values.to_vec();
        values.sort();
        values.dedup();
        values
            .iter()
            .map(|value| JSON::quote(value))
            .collect::<Vec<_>>()
            .join(",")
    };
    let json = format!(
        "{{\"schema\":\"jet.image.projection\",\"version\":1,\"manifest\":{},\"included\":[{}],\"changed\":[{}],\"omitted\":[{}],\"rejected\":[{}]}}",
        JSON::quote(manifest_digest),
        render(&report.included),
        render(&report.changed),
        render(&report.omitted),
        render(&report.rejected),
    );
    let temporary = path.with_extension(format!("json.tmp-{}", std::process::id()));
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid("image projection is not a regular file"));
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(json.as_bytes())?;
    file.sync_all()?;
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

/// Build the OCI layout into `out_dir` (created if absent). `ref_name` is a
/// human label recorded in `index.json`'s annotations (e.g. the `image.<name>`
/// contribution's name) — cosmetic only, never mixed into any digest.
pub fn build(spec: &BuildSpec, out_dir: &Path, ref_name: &str) -> io::Result<BuiltImage> {
    build_with_base(spec, out_dir, ref_name, None)
}

/// Build an OCI layout and append the new deterministic layer to a local OCI
/// base layout. The base is a directory, not a registry reference; callers
/// must obtain it through a separately verified transport before invoking this
/// function.
pub fn build_with_base(
    spec: &BuildSpec,
    out_dir: &Path,
    ref_name: &str,
    base: Option<&Path>,
) -> io::Result<BuiltImage> {
    ensure_real_layout_directory(out_dir)?;
    let mut files: Vec<&LayerFile> = spec.files.iter().collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    validate_layer_files(&files)?;

    let layer_tar = build_tar(&files);
    let layer_digest = SHA256::sha256_hex(&layer_tar);

    let base = base.map(load_base).transpose()?;
    let base_layers = base
        .as_ref()
        .map(|base| base.layers.clone())
        .unwrap_or_default();
    let mut diff_ids = base
        .as_ref()
        .map(|base| base.diff_ids.clone())
        .unwrap_or_default();
    diff_ids.push(format!("sha256:{layer_digest}"));

    let config_json = build_config_json_with_diff_ids(spec, &diff_ids);
    let config_digest = SHA256::sha256_hex(config_json.as_bytes());

    let mut layers = base_layers;
    layers.push(LayerDescriptor {
        media_type: MEDIA_TYPE_LAYER.to_string(),
        digest: format!("sha256:{layer_digest}"),
        size: layer_tar.len() as u64,
    });
    let manifest_json = build_manifest_json_with_layers(&config_digest, config_json.len(), &layers);
    let manifest_digest = SHA256::sha256_hex(manifest_json.as_bytes());

    fs::create_dir_all(out_dir.join("blobs").join("sha256"))?;
    if let Some(base) = base {
        for layer in &layers[..layers.len().saturating_sub(1)] {
            let source = base.root.join("blobs").join("sha256").join(digest_hex(&layer.digest)?);
            let bytes = read_regular_bounded(&source, layer.size)?;
            if bytes.len() as u64 != layer.size || digest_for(&bytes) != layer.digest {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("base OCI layer `{}` failed its digest or size check", layer.digest),
                ));
            }
            write_blob(out_dir, &layer.digest, &bytes)?;
        }
    }
    write_blob(out_dir, &layer_digest, &layer_tar)?;
    write_blob(out_dir, &config_digest, config_json.as_bytes())?;
    write_blob(out_dir, &manifest_digest, manifest_json.as_bytes())?;
    write_index(out_dir, &manifest_digest, manifest_json.len(), ref_name)?;
    write_layout_file(
        &out_dir.join("oci-layout"),
        b"{\"imageLayoutVersion\":\"1.0.0\"}",
    )?;

    Ok(BuiltImage {
        out_dir: out_dir.to_path_buf(),
        manifest_digest: format!("sha256:{manifest_digest}"),
        total_bytes: (layer_tar.len() + config_json.len() + manifest_json.len()) as u64,
    })
}

/// Copy a validated local OCI layout to another local layout root. Every
/// regular file is compared before reuse; symlinks and special files are not
/// allowed in an image layout. This is the local half of `--push`; registry
/// delivery is handled by `push_registry`.
pub fn copy_layout(source: &Path, destination: &Path) -> io::Result<()> {
    let _ = load_base(source)?;
    if source == destination {
        return Ok(());
    }
    let source = fs::canonicalize(source)?;
    validate_real_path(destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("OCI copy destination has no parent"))?;
    validate_real_path(parent)?;
    if !parent.exists() {
        fs::create_dir_all(parent)?;
    }
    let destination_probe = if destination.exists() {
        let metadata = fs::symlink_metadata(destination)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("OCI copy destination is not a real directory"));
        }
        fs::canonicalize(destination)?
    } else {
        fs::canonicalize(parent)?.join(
            destination
                .file_name()
                .ok_or_else(|| invalid("OCI copy destination has no name"))?,
        )
    };
    if destination_probe == source || destination_probe.starts_with(&source) {
        return Err(invalid("OCI copy destination cannot be inside the source layout"));
    }
    if destination.exists() {
        return if same_layout(&source, &destination_probe)? {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "OCI copy destination is not byte-identical; refusing an in-place merge",
            ))
        };
    }
    let stage = parent.join(format!(
        ".{}-jet-oci-copy-{}-{}",
        destination
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| invalid("OCI copy destination has no UTF-8 name"))?,
        std::process::id(),
        COPY_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&stage)?;
    let result = (|| {
        copy_layout_tree(&source, &stage)?;
        let _ = load_base(&stage)?;
        if fs::symlink_metadata(destination).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "OCI copy destination appeared during staged publication",
            ));
        }
        fs::rename(&stage, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

/// Push a validated local OCI layout through the OCI Distribution HTTP API.
/// The registry reference is `http[s]://host/repository[:tag]` (or a digest
/// reference); credentials stay host-owned through curl's optional netrc
/// lookup and never enter Jet source, argv, logs, or receipts.
pub fn push_registry(layout: &Path, reference: &str) -> io::Result<RegistryPushReport> {
    let registry = parse_registry_reference(reference)?;
    let manifest = load_registry_manifest(layout)?;
    let mut blobs_uploaded = 0usize;
    let mut bytes_uploaded = 0u64;
    for blob in &manifest.blobs {
        let blob_url = format!("{}/blobs/{}", registry.api_root, blob.digest);
        match registry_request(
            "HEAD",
            &blob_url,
            &[],
            None,
            MAX_REGISTRY_RESPONSE_BYTES,
            false,
        )? {
            response if (200..300).contains(&response.status) => continue,
            response if response.status == 404 => {
                upload_registry_blob(&registry, blob)?;
                blobs_uploaded += 1;
                bytes_uploaded = bytes_uploaded.saturating_add(blob.data.len() as u64);
            }
            response => {
                return Err(invalid(&format!(
                    "registry rejected blob probe with HTTP {}: {}",
                    response.status,
                    bounded_text(&response.stderr)
                )))
            }
        }
    }
    let manifest_url = format!("{}/manifests/{}", registry.api_root, registry.reference);
    let response = registry_request(
        "PUT",
        &manifest_url,
        &[format!("Content-Type: {MEDIA_TYPE_MANIFEST}")],
        Some(&manifest.raw),
        MAX_REGISTRY_RESPONSE_BYTES,
        false,
    )?;
    if !(200..300).contains(&response.status) {
        return Err(invalid(&format!(
            "registry rejected image manifest with HTTP {}: {}",
            response.status,
            bounded_text(&response.stderr)
        )));
    }
    Ok(RegistryPushReport {
        reference: reference.to_string(),
        manifest_digest: manifest.digest,
        blobs_uploaded,
        bytes_uploaded,
    })
}

#[derive(Debug, Clone)]
struct RegistryReference {
    origin: String,
    api_root: String,
    reference: String,
}

#[derive(Debug, Clone)]
struct RegistryBlob {
    digest: String,
    data: Vec<u8>,
}

#[derive(Debug, Clone)]
struct RegistryManifest {
    digest: String,
    raw: Vec<u8>,
    blobs: Vec<RegistryBlob>,
}

#[derive(Debug, Clone)]
struct RegistryResponse {
    status: u16,
    headers: Vec<u8>,
    stderr: Vec<u8>,
}

fn parse_registry_reference(reference: &str) -> io::Result<RegistryReference> {
    if reference.is_empty()
        || reference.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'?' | b'#' | b'%')
        })
    {
        return Err(invalid("OCI registry reference contains unsafe characters"));
    }
    let (scheme, rest) = reference
        .split_once("://")
        .ok_or_else(|| invalid("OCI registry reference must start with http:// or https://"))?;
    if scheme != "http" && scheme != "https" {
        return Err(invalid("OCI registry reference must use HTTP or HTTPS"));
    }
    let slash = rest
        .find('/')
        .ok_or_else(|| invalid("OCI registry reference has no repository"))?;
    let authority = &rest[..slash];
    if authority.is_empty()
        || authority.contains('@')
        || authority.starts_with('-')
        || !authority.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'-' | b'[' | b']')
        })
    {
        return Err(invalid("OCI registry reference has an unsafe host"));
    }
    let path = &rest[slash + 1..];
    let mut parts = path.split('/').collect::<Vec<_>>();
    let leaf = parts
        .pop()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("OCI registry reference has no image name"))?;
    if parts.iter().any(|part| part.is_empty()) {
        return Err(invalid("OCI registry repository has an empty path component"));
    }
    let (name, tag_or_digest) = if let Some((name, digest)) = leaf.split_once('@') {
        if name.is_empty() {
            return Err(invalid("OCI registry reference has no image name"));
        }
        validate_oci_digest(digest)?;
        (name, digest.to_string())
    } else if let Some(index) = leaf.rfind(':') {
        let name = &leaf[..index];
        let tag = &leaf[index + 1..];
        if name.is_empty() {
            return Err(invalid("OCI registry reference has no image name"));
        }
        validate_oci_tag(tag)?;
        (name, tag.to_string())
    } else {
        validate_oci_tag("latest")?;
        (leaf, "latest".to_string())
    };
    validate_registry_component(name, "OCI registry image name")?;
    let mut repository_parts = parts;
    repository_parts.push(name);
    for part in &repository_parts {
        validate_registry_component(part, "OCI registry repository component")?;
    }
    let repository = repository_parts.join("/");
    let origin = format!("{scheme}://{authority}");
    Ok(RegistryReference {
        api_root: format!("{origin}/v2/{repository}"),
        origin,
        reference: tag_or_digest,
    })
}

fn validate_registry_component(value: &str, label: &str) -> io::Result<()> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.len() > 255
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid(&format!("{label} is unsafe")));
    }
    Ok(())
}

fn validate_oci_tag(tag: &str) -> io::Result<()> {
    if tag.is_empty()
        || tag.len() > 128
        || !tag.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_alphanumeric() || byte == b'_'
            } else {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
            }
        })
    {
        return Err(invalid("OCI registry tag is unsafe"));
    }
    Ok(())
}

fn validate_oci_digest(digest: &str) -> io::Result<()> {
    if digest.strip_prefix("sha256:").is_none() {
        return Err(invalid("OCI registry digest must be sha256"));
    }
    digest_hex(digest).map(|_| ())
}

fn load_registry_manifest(root: &Path) -> io::Result<RegistryManifest> {
    let _ = load_base(root)?;
    let index = JSON::parse(&read_text(root.join("index.json"))?).map_err(io::Error::other)?;
    let index = object(&index, "OCI index")?;
    let descriptors = array(index.get("manifests"), "OCI index manifests")?;
    let descriptor = object(
        descriptors
            .first()
            .ok_or_else(|| invalid("OCI index has no manifest"))?,
        "OCI manifest descriptor",
    )?;
    let digest = string_field(descriptor, "digest")?.to_string();
    let manifest_size = number_field(descriptor, "size")?;
    let raw = read_blob(root, &digest)?;
    if raw.len() as u64 != manifest_size {
        return Err(invalid("OCI manifest descriptor size mismatch"));
    }
    let computed = digest_for(&raw);
    if computed != digest {
        return Err(invalid("OCI manifest descriptor digest mismatch"));
    }
    let manifest = JSON::parse(
        std::str::from_utf8(&raw).map_err(|_| invalid("OCI manifest is not UTF-8"))?,
    )
    .map_err(io::Error::other)?;
    let manifest = object(&manifest, "OCI manifest")?;
    let config = object(
        manifest
            .get("config")
            .ok_or_else(|| invalid("OCI manifest has no config"))?,
        "OCI config descriptor",
    )?;
    let config_digest = string_field(config, "digest")?.to_string();
    let config_size = number_field(config, "size")?;
    let config_data = read_blob(root, &config_digest)?;
    if config_data.len() as u64 != config_size {
        return Err(invalid("OCI config descriptor size mismatch"));
    }
    let mut blobs = vec![RegistryBlob {
        digest: config_digest,
        data: config_data,
    }];
    for value in array(manifest.get("layers"), "OCI manifest layers")? {
        let descriptor = object(value, "OCI layer descriptor")?;
        let digest = string_field(descriptor, "digest")?.to_string();
        let size = number_field(descriptor, "size")?;
        let data = read_blob(root, &digest)?;
        if data.len() as u64 != size {
            return Err(invalid("OCI layer descriptor size mismatch"));
        }
        blobs.push(RegistryBlob { digest, data });
    }
    Ok(RegistryManifest { digest, raw, blobs })
}

fn upload_registry_blob(registry: &RegistryReference, blob: &RegistryBlob) -> io::Result<()> {
    let uploads = format!("{}/blobs/uploads/", registry.api_root);
    let response = registry_request(
        "POST",
        &uploads,
        &["Content-Length: 0".to_string()],
        None,
        MAX_REGISTRY_RESPONSE_BYTES,
        true,
    )?;
    if !(200..300).contains(&response.status) {
        return Err(invalid(&format!(
            "registry rejected blob upload start with HTTP {}: {}",
            response.status,
            bounded_text(&response.stderr)
        )));
    }
    let location = header_value(&response.headers, "location")
        .ok_or_else(|| invalid("registry upload response has no Location"))?;
    let location = resolve_registry_location(&registry.origin, &location)?;
    let separator = if location.contains('?') { '&' } else { '?' };
    let upload_url = format!("{location}{separator}digest={}", blob.digest);
    let response = registry_request(
        "PUT",
        &upload_url,
        &["Content-Type: application/octet-stream".to_string()],
        Some(&blob.data),
        MAX_REGISTRY_RESPONSE_BYTES,
        false,
    )?;
    if !(200..300).contains(&response.status) {
        return Err(invalid(&format!(
            "registry rejected blob upload with HTTP {}: {}",
            response.status,
            bounded_text(&response.stderr)
        )));
    }
    Ok(())
}

fn registry_request(
    method: &str,
    url: &str,
    headers: &[String],
    body: Option<&[u8]>,
    response_limit: u64,
    include_headers: bool,
) -> io::Result<RegistryResponse> {
    if method.is_empty() || url.is_empty() {
        return Err(invalid("registry request has no method or URL"));
    }
    let mut args = vec![
        "--silent".to_string(),
        "--show-error".to_string(),
        "--max-time".to_string(),
        "120".to_string(),
        "--netrc-optional".to_string(),
        "--request".to_string(),
        method.to_string(),
    ];
    for header in headers {
        args.push("--header".to_string());
        args.push(header.clone());
    }
    if body.is_some() {
        args.extend(["--data-binary".to_string(), "@-".to_string()]);
    }
    if include_headers {
        args.extend(["--dump-header".to_string(), "-".to_string()]);
    } else {
        args.extend(["--output".to_string(), null_device().to_string()]);
    }
    args.extend([
        "--write-out".to_string(),
        "\\n%{http_code}".to_string(),
        url.to_string(),
    ]);
    let mut child = Command::new("curl")
        .args(&args)
        .stdin(if body.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("OCI registry adapter could not start: {error}")))?;
    if let Some(body) = body {
        child
            .stdin
            .take()
            .ok_or_else(|| invalid("OCI registry adapter has no input"))?
            .write_all(body)?;
    }
    let (status, stdout, stderr) =
        run_bounded_output(child, response_limit, "OCI registry response")?;
    if !status.success() && stdout.is_empty() {
        return Ok(RegistryResponse {
            status: 0,
            headers: Vec::new(),
            stderr,
        });
    }
    let separator = stdout
        .iter()
        .rposition(|byte| *byte == b'\n')
        .ok_or_else(|| invalid("OCI registry response has no HTTP status"))?;
    let status_code = std::str::from_utf8(&stdout[separator + 1..])
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .ok_or_else(|| invalid("OCI registry response has an invalid HTTP status"))?;
    Ok(RegistryResponse {
        status: status_code,
        headers: if include_headers {
            stdout[..separator].to_vec()
        } else {
            Vec::new()
        },
        stderr,
    })
}

fn header_value(headers: &[u8], wanted: &str) -> Option<String> {
    headers.split(|byte| *byte == b'\n').find_map(|line| {
        let (name, value) = line.split_once(|byte| *byte == b':')?;
        if std::str::from_utf8(name)
            .ok()?
            .trim()
            .eq_ignore_ascii_case(wanted)
        {
            Some(String::from_utf8_lossy(value).trim().to_string())
        } else {
            None
        }
    })
}

fn resolve_registry_location(origin: &str, location: &str) -> io::Result<String> {
    if location.is_empty()
        || location.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'\'' | b'"' | b'%' | b'#')
        })
    {
        return Err(invalid("registry upload Location is unsafe"));
    }
    if location.starts_with("http://") || location.starts_with("https://") {
        let origin_slash = format!("{origin}/");
        if !location.starts_with(&origin_slash) {
            return Err(invalid(
                "registry upload Location points at a different authority",
            ));
        }
        return Ok(location.to_string());
    }
    if location.starts_with('/') {
        Ok(format!("{origin}{location}"))
    } else {
        Ok(format!("{origin}/{location}"))
    }
}

fn bounded_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(4096)])
        .trim()
        .to_string()
}

fn run_bounded_output(
    mut child: Child,
    limit: u64,
    label: &str,
) -> io::Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>)> {
    let stderr_thread = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                match stderr.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if bytes.len() < 4096 {
                            let keep = read.min(4096 - bytes.len());
                            bytes.extend_from_slice(&buffer[..keep]);
                        }
                    }
                    Err(_) => break,
                }
            }
            bytes
        })
    });
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| invalid(&format!("{label} has no output pipe")))?;
    let max = usize::try_from(limit.saturating_add(1))
        .map_err(|_| invalid(&format!("{label} bound is too large")))?;
    let mut bytes = Vec::new();
    stdout.take(max as u64).read_to_end(&mut bytes)?;
    if bytes.len() >= max {
        let _ = child.kill();
        let _ = child.wait();
        if let Some(thread) = stderr_thread {
            let _ = thread.join();
        }
        return Err(invalid(&format!("{label} exceeded its bound")));
    }
    let status = child.wait()?;
    let stderr = stderr_thread
        .map(|thread| thread.join().unwrap_or_default())
        .unwrap_or_default();
    Ok((status, bytes, stderr))
}

fn null_device() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

fn copy_layout_tree(source: &Path, destination: &Path) -> io::Result<()> {
    let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() && !metadata.is_file() {
            return Err(invalid("OCI layout contains an unsupported filesystem node"));
        }
        if let Ok(destination_metadata) = fs::symlink_metadata(&to) {
            if destination_metadata.file_type().is_symlink() {
                return Err(invalid("OCI copy destination contains a symlink"));
            }
        }
        if metadata.is_dir() {
            if to.exists() && !to.is_dir() {
                return Err(invalid("OCI copy destination has a conflicting node"));
            }
            fs::create_dir_all(&to)?;
            copy_layout_tree(&from, &to)?;
        } else if to.is_file() {
            if read_regular_bounded(&from, 512 * 1024 * 1024)?
                != read_regular_bounded(&to, 512 * 1024 * 1024)?
            {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("OCI copy destination has conflicting `{}`", to.display()),
                ));
            }
        } else if to.exists() {
            return Err(invalid("OCI copy destination has a conflicting node"));
        } else {
            let bytes = read_regular_bounded(&from, 512 * 1024 * 1024)?;
            write_layout_file(&to, &bytes)?;
        }
    }
    Ok(())
}

fn same_layout(left: &Path, right: &Path) -> io::Result<bool> {
    let left_metadata = fs::symlink_metadata(left)?;
    let right_metadata = fs::symlink_metadata(right)?;
    if left_metadata.file_type().is_symlink() || right_metadata.file_type().is_symlink() {
        return Err(invalid("OCI layout contains a symlink"));
    }
    if left_metadata.is_dir() != right_metadata.is_dir()
        || left_metadata.is_file() != right_metadata.is_file()
    {
        return Ok(false);
    }
    if left_metadata.is_file() {
        return Ok(read_regular_bounded(left, 512 * 1024 * 1024)?
            == read_regular_bounded(right, 512 * 1024 * 1024)?);
    }
    if !left_metadata.is_dir() {
        return Err(invalid("OCI layout contains an unsupported filesystem node"));
    }
    let mut left_entries = fs::read_dir(left)?.collect::<Result<Vec<_>, _>>()?;
    let mut right_entries = fs::read_dir(right)?.collect::<Result<Vec<_>, _>>()?;
    left_entries.sort_by_key(|entry| entry.file_name());
    right_entries.sort_by_key(|entry| entry.file_name());
    if left_entries.len() != right_entries.len() {
        return Ok(false);
    }
    for (left_entry, right_entry) in left_entries.iter().zip(right_entries.iter()) {
        if left_entry.file_name() != right_entry.file_name()
            || !same_layout(&left_entry.path(), &right_entry.path())?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(Debug, Clone)]
struct LayerDescriptor {
    media_type: String,
    digest: String,
    size: u64,
}

#[derive(Debug, Clone)]
struct BaseLayout {
    root: PathBuf,
    layers: Vec<LayerDescriptor>,
    diff_ids: Vec<String>,
}

fn write_blob(out_dir: &Path, digest: &str, data: &[u8]) -> io::Result<()> {
    let path = out_dir
        .join("blobs")
        .join("sha256")
        .join(digest_hex(digest)?);
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "OCI blob destination is not a regular file",
            ));
        }
        if read_regular_bounded(&path, data.len() as u64)? == data {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("OCI blob `{digest}` already has conflicting bytes"),
        ));
    }
    let temporary = path.with_extension(format!("partial-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(data)?;
        file.sync_all()?;
        match fs::hard_link(&temporary, &path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if read_regular_bounded(&path, data.len() as u64)? == data {
                    Ok(())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("OCI blob `{digest}` already has conflicting bytes"),
                    ))
                }
            }
            Err(error) => Err(error),
        }
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn write_index(
    out_dir: &Path,
    manifest_digest: &str,
    manifest_size: usize,
    ref_name: &str,
) -> io::Result<()> {
    let json = format!(
        "{{\"schemaVersion\":2,\"manifests\":[{{\"mediaType\":{mt},\"digest\":\"sha256:{d}\",\"size\":{s},\"annotations\":{{\"org.opencontainers.image.ref.name\":{name}}}}}]}}",
        mt = JSON::quote(MEDIA_TYPE_MANIFEST),
        d = manifest_digest,
        s = manifest_size,
        name = JSON::quote(ref_name),
    );
    write_layout_file(&out_dir.join("index.json"), json.as_bytes())
}

/// The OCI image config (minimal subset: `architecture`/`os`/`config`/`rootfs`;
/// no `created`/`history` — an embedded build timestamp is exactly the kind of
/// nondeterminism this builder exists to avoid).
fn build_config_json_with_diff_ids(spec: &BuildSpec, diff_ids: &[String]) -> String {
    let env_arr = sorted_env(&spec.env)
        .iter()
        .map(|(k, v)| JSON::quote(&format!("{k}={v}")))
        .collect::<Vec<_>>()
        .join(",");
    let entrypoint_arr = spec
        .entrypoint
        .iter()
        .map(|s| JSON::quote(s))
        .collect::<Vec<_>>()
        .join(",");
    let mut ports = spec.expose.clone();
    ports.sort_unstable();
    ports.dedup();
    let exposed = ports
        .iter()
        .map(|p| format!("{}:{{}}", JSON::quote(&format!("{p}/tcp"))))
        .collect::<Vec<_>>()
        .join(",");
    let health = spec.healthcheck.as_ref().map_or_else(String::new, |command| {
        format!(
            ",\"Healthcheck\":{{\"Test\":[\"CMD-SHELL\",{}]}}",
            JSON::quote(command)
        )
    });
    let diff_ids = diff_ids
        .iter()
        .map(|digest| JSON::quote(digest))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"architecture\":\"{arch}\",\"os\":\"{os}\",\"config\":{{\"Env\":[{env_arr}],\"ExposedPorts\":{{{exposed}}},\"Entrypoint\":[{entrypoint_arr}],\"User\":\"{user}\"{health}}},\"rootfs\":{{\"type\":\"layers\",\"diff_ids\":[{diff_ids}]}}}}",
        arch = ARCHITECTURE,
        os = OS,
        user = spec.user,
        health = health,
        diff_ids = diff_ids,
    )
}

fn build_manifest_json_with_layers(
    config_digest: &str,
    config_size: usize,
    layers: &[LayerDescriptor],
) -> String {
    let layers = layers
        .iter()
        .map(|layer| {
            format!(
                "{{\"mediaType\":{},\"digest\":{},\"size\":{}}}",
                JSON::quote(&layer.media_type),
                JSON::quote(&layer.digest),
                layer.size
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schemaVersion\":2,\"mediaType\":{mt},\"config\":{{\"mediaType\":{cmt},\"digest\":\"sha256:{config_digest}\",\"size\":{config_size}}},\"layers\":[{layers}]}}",
        mt = JSON::quote(MEDIA_TYPE_MANIFEST),
        cmt = JSON::quote(MEDIA_TYPE_CONFIG),
        layers = layers,
    )
}

fn load_base(root: &Path) -> io::Result<BaseLayout> {
    validate_real_path(root)?;
    let metadata = fs::symlink_metadata(root)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || !root.join("oci-layout").is_file()
        || !root.join("index.json").is_file()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("OCI base `{}` is not a local OCI image layout", root.display()),
        ));
    }
    let layout = JSON::parse(&read_text(root.join("oci-layout"))?).map_err(io::Error::other)?;
    let layout = object(&layout, "OCI layout")?;
    if string_field(layout, "imageLayoutVersion")? != "1.0.0" {
        return Err(invalid("OCI layout has an unsupported version"));
    }
    let index = JSON::parse(&read_text(root.join("index.json"))?).map_err(io::Error::other)?;
    let index = object(&index, "OCI index")?;
    let manifests = array(index.get("manifests"), "OCI index manifests")?;
    let descriptor = object(
        manifests
            .first()
            .ok_or_else(|| invalid("OCI base index has no manifest"))?,
        "OCI manifest descriptor",
    )?;
    if string_field(descriptor, "mediaType")? != MEDIA_TYPE_MANIFEST {
        return Err(invalid("OCI index manifest has an unsupported media type"));
    }
    let manifest_digest = string_field(descriptor, "digest")?;
    let manifest_size = number_field(descriptor, "size")?;
    let manifest_bytes = read_blob(root, manifest_digest)?;
    if manifest_bytes.len() as u64 != manifest_size
        || digest_for(&manifest_bytes) != manifest_digest
    {
        return Err(invalid("OCI manifest descriptor failed its digest or size check"));
    }
    let manifest = JSON::parse(
        std::str::from_utf8(&manifest_bytes)
            .map_err(|_| invalid("OCI manifest is not UTF-8"))?,
    )
    .map_err(io::Error::other)?;
    let manifest = object(&manifest, "OCI manifest")?;
    let layer_values = array(manifest.get("layers"), "OCI manifest layers")?;
    let mut layers = Vec::with_capacity(layer_values.len());
    for value in layer_values {
        let descriptor = object(value, "OCI layer descriptor")?;
        let digest = string_field(descriptor, "digest")?.to_string();
        let media_type = string_field(descriptor, "mediaType")?.to_string();
        let size = number_field(descriptor, "size")?;
        let bytes = read_blob(root, &digest)?;
        if bytes.len() as u64 != size || digest_for(&bytes) != digest {
            return Err(invalid("OCI base layer failed its digest or size check"));
        }
        layers.push(LayerDescriptor {
            media_type,
            digest,
            size,
        });
    }
    let config = object(
        manifest
            .get("config")
            .ok_or_else(|| invalid("OCI manifest has no config descriptor"))?,
        "OCI config descriptor",
    )?;
    if string_field(config, "mediaType")? != MEDIA_TYPE_CONFIG {
        return Err(invalid("OCI config has an unsupported media type"));
    }
    let config_digest = string_field(config, "digest")?;
    let config_size = number_field(config, "size")?;
    let config_bytes = read_blob(root, config_digest)?;
    if config_bytes.len() as u64 != config_size || digest_for(&config_bytes) != config_digest {
        return Err(invalid("OCI config descriptor failed its digest or size check"));
    }
    let config = JSON::parse(
        std::str::from_utf8(&config_bytes)
            .map_err(|_| invalid("OCI config is not UTF-8"))?,
    )
    .map_err(io::Error::other)?;
    let config = object(&config, "OCI config")?;
    let rootfs = object(
        config
            .get("rootfs")
            .ok_or_else(|| invalid("OCI config has no rootfs"))?,
        "OCI config rootfs",
    )?;
    let mut diff_ids = array(rootfs.get("diff_ids"), "OCI config diff_ids")?
        .iter()
        .map(|value| match value {
            JSON::JSONValue::Str(digest) => {
                digest_hex(digest)?;
                Ok(digest.clone())
            }
            _ => Err(invalid("OCI config diff_ids contains a non-string")),
        })
        .collect::<io::Result<Vec<_>>>()?;
    if diff_ids.len() != layers.len() {
        return Err(invalid("OCI config diff_ids do not match manifest layers"));
    }
    if diff_ids.is_empty() && !layers.is_empty() {
        diff_ids = layers.iter().map(|layer| layer.digest.clone()).collect();
    }
    Ok(BaseLayout {
        root: root.canonicalize()?,
        layers,
        diff_ids,
    })
}

fn validate_layer_files(files: &[&LayerFile]) -> io::Result<()> {
    let mut paths = BTreeMap::new();
    for file in files {
        if file.path.is_empty()
            || file.path.len() > 100
            || file.path.starts_with('/')
            || file.path.contains('\\')
            || file.path.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(invalid("OCI layer path is not a safe tar path"));
        }
        let path = Path::new(&file.path);
        if path.components().any(|component| {
            matches!(component, std::path::Component::ParentDir | std::path::Component::RootDir | std::path::Component::Prefix(_))
        }) {
            return Err(invalid("OCI layer path escapes the image root"));
        }
        if file.data.len() > 512 * 1024 * 1024 {
            return Err(invalid("OCI layer file exceeds the 512 MiB limit"));
        }
        if paths.insert(file.path.clone(), ()).is_some() {
            return Err(invalid("OCI layer contains duplicate paths"));
        }
    }
    let paths: Vec<_> = paths.keys().collect();
    for pair in paths.windows(2) {
        if pair[1].starts_with(&format!("{}/", pair[0])) {
            return Err(invalid("OCI layer contains a file/directory path collision"));
        }
    }
    Ok(())
}

fn read_text(path: PathBuf) -> io::Result<String> {
    let bytes = read_regular_bounded(&path, 1024 * 1024)?;
    String::from_utf8(bytes).map_err(|_| invalid("OCI JSON is not UTF-8"))
}

fn read_blob(root: &Path, digest: &str) -> io::Result<Vec<u8>> {
    let root_metadata = fs::symlink_metadata(root)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(invalid("OCI layout root is not a real directory"));
    }
    let digest = digest_hex(digest)?;
    let path = root.join("blobs").join("sha256").join(digest);
    let mut cursor = root.to_path_buf();
    for component in ["blobs", "sha256", digest] {
        cursor.push(component);
        if let Ok(metadata) = fs::symlink_metadata(&cursor) {
            if metadata.file_type().is_symlink() {
                return Err(invalid("OCI layout contains a symlink in its blob path"));
            }
        }
    }
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > 512 * 1024 * 1024
    {
        return Err(invalid("OCI blob is not a regular file within its limit"));
    }
    let bytes = read_regular_bounded(&path, 512 * 1024 * 1024)?;
    if digest_for(&bytes) != digest {
        return Err(invalid("OCI blob digest mismatch"));
    }
    Ok(bytes)
}

fn object<'a>(value: &'a JSON::JSONValue, label: &str) -> io::Result<&'a std::collections::BTreeMap<String, JSON::JSONValue>> {
    match value {
        JSON::JSONValue::Object(object) => Ok(object),
        _ => Err(invalid(&format!("{label} is not an object"))),
    }
}

fn array<'a>(value: Option<&'a JSON::JSONValue>, label: &str) -> io::Result<&'a Vec<JSON::JSONValue>> {
    match value {
        Some(JSON::JSONValue::Array(values)) => Ok(values),
        _ => Err(invalid(&format!("{label} is not an array"))),
    }
}

fn string_field<'a>(
    object: &'a std::collections::BTreeMap<String, JSON::JSONValue>,
    field: &str,
) -> io::Result<&'a str> {
    match object.get(field) {
        Some(JSON::JSONValue::Str(value)) if !value.is_empty() => Ok(value),
        _ => Err(invalid(&format!("OCI object field `{field}` is not a string"))),
    }
}

fn number_field(
    object: &std::collections::BTreeMap<String, JSON::JSONValue>,
    field: &str,
) -> io::Result<u64> {
    match object.get(field) {
        Some(JSON::JSONValue::Num(value))
            if value.is_finite()
                && *value >= 0.0
                && *value <= u64::MAX as f64
                && value.fract() == 0.0 =>
        {
            Ok(*value as u64)
        }
        _ => Err(invalid(&format!("OCI object field `{field}` is not a non-negative integer"))),
    }
}

fn digest_hex(digest: &str) -> io::Result<&str> {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("OCI digest is not a SHA-256 value"));
    }
    Ok(hex)
}

fn digest_for(bytes: &[u8]) -> String {
    format!("sha256:{}", SHA256::sha256_hex(bytes))
}

fn read_regular_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > limit {
        return Err(invalid("OCI input is not a bounded regular file"));
    }
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("OCI input exceeded its bound while being read"));
    }
    Ok(bytes)
}

fn write_layout_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid("OCI layout metadata is not a regular file"));
        }
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn validate_real_path(path: &Path) -> io::Result<()> {
    let mut current = if path.is_absolute() {
        PathBuf::from(std::path::MAIN_SEPARATOR_STR)
    } else {
        std::env::current_dir()?
    };
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                return Err(invalid("OCI path contains a parent component"));
            }
            std::path::Component::Normal(value) => {
                current.push(value);
                if let Ok(metadata) = fs::symlink_metadata(&current) {
                    if metadata.file_type().is_symlink() {
                        return Err(invalid("OCI path cannot traverse a symlink"));
                    }
                }
            }
        }
    }
    Ok(())
}

fn ensure_real_layout_directory(path: &Path) -> io::Result<()> {
    validate_real_path(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(invalid("OCI output path is not a real directory"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::create_dir_all(path)?;
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(invalid("OCI output path is not a real directory"));
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

// ── tar (ustar, uncompressed, deterministic) ────────────────────────────────

/// Build one uncompressed ustar tar archive from `files` (assumed already
/// sorted by path). Every entry: mtime 0 (Unix epoch), uid/gid 0, no
/// uname/gname — nothing that varies run-to-run or machine-to-machine.
fn build_tar(files: &[&LayerFile]) -> Vec<u8> {
    let mut out = Vec::new();
    for f in files {
        write_tar_entry(&mut out, &f.path, &f.data, f.mode);
    }
    // End-of-archive marker: two 512-byte zero blocks.
    out.extend(std::iter::repeat(0u8).take(1024));
    out
}

fn write_tar_entry(out: &mut Vec<u8>, path: &str, data: &[u8], mode: u32) {
    let mut header = [0u8; 512];
    set_bytes(&mut header, 0, 100, path.as_bytes());
    set_octal(&mut header, 100, 8, mode as u64);
    set_octal(&mut header, 108, 8, 0); // uid
    set_octal(&mut header, 116, 8, 0); // gid
    set_octal(&mut header, 124, 12, data.len() as u64); // size
    set_octal(&mut header, 136, 12, 0); // mtime = epoch 0, always
    header[156] = b'0'; // typeflag '0' = regular file
    set_bytes(&mut header, 257, 6, b"ustar\0"); // magic
    set_bytes(&mut header, 263, 2, b"00"); // ustar version

    // Checksum: the field itself reads as ASCII spaces during the sum, then
    // is written as a 6-digit octal + NUL + space (the ustar convention).
    for b in &mut header[148..156] {
        *b = b' ';
    }
    let checksum: u32 = header.iter().map(|&b| b as u32).sum();
    let chksum_field = format!("{checksum:06o}\0 ");
    set_bytes(&mut header, 148, 8, chksum_field.as_bytes());

    out.extend_from_slice(&header);
    out.extend_from_slice(data);
    let pad = (512 - (data.len() % 512)) % 512;
    out.extend(std::iter::repeat(0u8).take(pad));
}

/// Copy `bytes` into `header[offset..offset+len]`, truncating to `len` and
/// zero-padding the rest (tar string fields are NUL-padded).
fn set_bytes(header: &mut [u8; 512], offset: usize, len: usize, bytes: &[u8]) {
    let n = bytes.len().min(len);
    header[offset..offset + n].copy_from_slice(&bytes[..n]);
}

/// Write `value` as a NUL-terminated octal ASCII string right-padded... no —
/// left-padded with `0`s to fill `len - 1` digits, then a trailing NUL, per
/// the classic (pre-GNU-base-256) tar numeric field encoding.
fn set_octal(header: &mut [u8; 512], offset: usize, len: usize, value: u64) {
    let digits = len - 1;
    let s = format!("{value:0>digits$o}\0", digits = digits);
    set_bytes(header, offset, len, s.as_bytes());
}

// ── digest-agnostic helper for callers: sort + dedupe env keys ─────────────

/// D-JPK-IMAGE1: fold `env_vars:` (already key-sorted — `ImagePlan::env_vars`
/// comes from a `CtValue::Map`/`BTreeMap`) into a plain `Vec`, defensively
/// re-sorted so a caller that hand-builds a `BuildSpec` some other way still
/// gets a deterministic `Env` order.
pub fn sorted_env(entries: &[(String, String)]) -> Vec<(String, String)> {
    let map: BTreeMap<&str, &str> = entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    map.into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bin(path: &str, contents: &str) -> LayerFile {
        LayerFile {
            path: path.to_string(),
            data: contents.as_bytes().to_vec(),
            mode: 0o755,
        }
    }

    #[test]
    fn tar_layer_is_byte_identical_across_calls() {
        let files = vec![bin("usr/local/bin/app", "hello binary")];
        let refs: Vec<&LayerFile> = files.iter().collect();
        let a = build_tar(&refs);
        let b = build_tar(&refs);
        assert_eq!(a, b);
    }

    #[test]
    fn tar_header_checksum_is_valid_ustar() {
        let files = vec![bin("usr/local/bin/app", "x")];
        let refs: Vec<&LayerFile> = files.iter().collect();
        let tar = build_tar(&refs);
        // Recompute the checksum the way a real reader would: sum the header
        // bytes with the checksum field blanked to spaces, and it must match
        // what's encoded.
        let mut header = [0u8; 512];
        header.copy_from_slice(&tar[0..512]);
        let encoded = std::str::from_utf8(&header[148..154]).unwrap().to_string();
        let encoded_val = u32::from_str_radix(encoded.trim_end_matches('\0'), 8).unwrap();
        for b in &mut header[148..156] {
            *b = b' ';
        }
        let recomputed: u32 = header.iter().map(|&b| b as u32).sum();
        assert_eq!(encoded_val, recomputed);
    }

    /// I5/reproducibility: building the same `BuildSpec` twice, into two
    /// different output directories, yields byte-identical blobs and the same
    /// manifest digest — no timestamps, no nondeterministic ordering.
    #[test]
    fn build_is_reproducible_across_separate_builds() {
        let spec = BuildSpec {
            files: vec![bin("usr/local/bin/app", "the binary")],
            entrypoint: vec!["/usr/local/bin/app".to_string()],
            env: vec![("RUST_LOG".to_string(), "info".to_string())],
            expose: vec![8080],
            user: 10_001,
            healthcheck: None,
        };
        let dir_a = std::env::temp_dir().join(format!("jet-oci-test-a-{}", std::process::id()));
        let dir_b = std::env::temp_dir().join(format!("jet-oci-test-b-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);

        let a = build(&spec, &dir_a, "myimage").unwrap();
        let b = build(&spec, &dir_b, "myimage").unwrap();
        assert_eq!(a.manifest_digest, b.manifest_digest);
        assert_eq!(a.total_bytes, b.total_bytes);

        // The actual blob bytes on disk must be identical too, not just the
        // digest string (a digest collision would be a much bigger bug, but
        // this proves the builder itself, not just SHA256, is deterministic).
        let blobs_a = dir_a.join("blobs").join("sha256");
        let blobs_b = dir_b.join("blobs").join("sha256");
        let digest_hex = a.manifest_digest.trim_start_matches("sha256:");
        assert_eq!(
            fs::read(blobs_a.join(digest_hex)).unwrap(),
            fs::read(blobs_b.join(digest_hex)).unwrap(),
        );

        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }

    /// Declaration order of `files:` must not affect the digest — `build`
    /// sorts by path before laying out the tar.
    #[test]
    fn file_declaration_order_does_not_affect_digest() {
        let f1 = bin("usr/local/bin/app", "the binary");
        let f2 = LayerFile {
            path: "etc/app.conf".to_string(),
            data: b"config".to_vec(),
            mode: 0o644,
        };
        let spec_a = BuildSpec {
            files: vec![f1.clone(), f2.clone()],
            entrypoint: vec!["/usr/local/bin/app".to_string()],
            env: vec![],
            expose: vec![],
            user: 10_001,
            healthcheck: None,
        };
        let spec_b = BuildSpec {
            files: vec![f2, f1],
            ..spec_a.clone()
        };
        let dir_a =
            std::env::temp_dir().join(format!("jet-oci-test-order-a-{}", std::process::id()));
        let dir_b =
            std::env::temp_dir().join(format!("jet-oci-test-order-b-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);

        let a = build(&spec_a, &dir_a, "myimage").unwrap();
        let b = build(&spec_b, &dir_b, "myimage").unwrap();
        assert_eq!(a.manifest_digest, b.manifest_digest);

        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }

    /// A changed input (different file contents) must change the digest —
    /// the flip side of reproducibility: it isn't a constant.
    #[test]
    fn different_content_yields_different_digest() {
        let spec_a = BuildSpec {
            files: vec![bin("usr/local/bin/app", "version one")],
            entrypoint: vec!["/usr/local/bin/app".to_string()],
            env: vec![],
            expose: vec![],
            user: 10_001,
            healthcheck: None,
        };
        let spec_b = BuildSpec {
            files: vec![bin("usr/local/bin/app", "version two")],
            ..spec_a.clone()
        };
        let dir_a =
            std::env::temp_dir().join(format!("jet-oci-test-diff-a-{}", std::process::id()));
        let dir_b =
            std::env::temp_dir().join(format!("jet-oci-test-diff-b-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);

        let a = build(&spec_a, &dir_a, "myimage").unwrap();
        let b = build(&spec_b, &dir_b, "myimage").unwrap();
        assert_ne!(a.manifest_digest, b.manifest_digest);

        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }

    #[test]
    #[ignore]
    fn manual_dump_for_tar_inspection() {
        let files = vec![
            bin("usr/local/bin/app", "hello binary contents"),
            LayerFile {
                path: "etc/app.conf".to_string(),
                data: b"key=value\n".to_vec(),
                mode: 0o644,
            },
        ];
        let refs: Vec<&LayerFile> = files.iter().collect();
        let tar = build_tar(&refs);
        let path = std::env::temp_dir().join("jet-oci-manual-check.tar");
        fs::write(path, &tar).unwrap();
    }

    #[test]
    fn oci_layout_has_the_required_files() {
        let spec = BuildSpec {
            files: vec![bin("usr/local/bin/app", "bin")],
            entrypoint: vec!["/usr/local/bin/app".to_string()],
            env: vec![],
            expose: vec![],
            user: 10_001,
            healthcheck: None,
        };
        let dir = std::env::temp_dir().join(format!("jet-oci-test-layout-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let built = build(&spec, &dir, "myimage").unwrap();
        assert!(dir.join("oci-layout").is_file());
        assert!(dir.join("index.json").is_file());
        let digest_hex = built.manifest_digest.trim_start_matches("sha256:");
        assert!(dir.join("blobs").join("sha256").join(digest_hex).is_file());
        let _ = fs::remove_dir_all(&dir);
    }
}
