//! Signed, immutable nixpkgs channel indexes.
//!
//! The producer side joins off-device Nix/Hydra evidence.  The client side
//! only consumes a signed, content-addressed index and never invokes Nix.

use crate::Store::Roots;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use jet_foundation::base_encoding_strict::decode_base64;
use jet_foundation::EncodingJson::{parse_json_exact_numbers, Value};
use jet_foundation::JSON::json_escape;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const MAX_COMPRESSED_BYTES: usize = 33_554_432;
pub(crate) const MAX_DECODED_BYTES: usize = 268_435_456;
pub(crate) const MAX_RECORDS: usize = 400_000;
const MAX_SIGNATURE_BYTES: u64 = 16 * 1024;
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const INDEX_SCHEMA: u64 = 1;
const INDEX_DOMAIN: &[u8] = b"jet-nixpkgs-index-v1\n";
const MANIFEST_DOMAIN: &[u8] = b"jet-nixpkgs-channel-manifest-v1\n";
const INDEX_ROOT: &str = "hangar/nix-index/v1";
const DEFAULT_ENDPOINT: &str = "https://index.jet.dev/nixpkgs";
// RFC 8032 test public key.  The operational key is an owner gate; this
// embedded value is deliberately not used as publication evidence.
const DEFAULT_KEY_ID: &str = "jet-nixpkgs-index-v1-unratified";
const DEFAULT_PUBLIC_KEY: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];
const CHANNELS: &[&str] = &["nixpkgs-unstable", "nixos-unstable"];
const SYSTEMS: &[&str] = &[
    "x86_64-linux",
    "aarch64-linux",
    "x86_64-darwin",
    "aarch64-darwin",
];
const COVERAGE_REASONS: &[&str] = &[
    "evaluation-failed",
    "unsupported-value",
    "unsupported-system",
    "no-channel-build",
    "missing-narinfo",
    "missing-version",
    "missing-primary-output",
    "policy-excluded",
];
const NIX32: &[u8] = b"0123456789abcdfghijklmnpqrsvwxyz";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexKey {
    pub channel: String,
    pub revision: String,
    pub system: String,
    pub attrpath: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexRecord {
    pub attrpath: Vec<String>,
    pub version: String,
    pub drv_path: String,
    pub outputs: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexProof {
    pub schema: u64,
    pub channel: String,
    pub revision: String,
    pub system: String,
    pub attrpath: Vec<String>,
    pub manifest_generation: u64,
    pub manifest_sha256: String,
    pub index_sha256: String,
    pub record_sha256: String,
    pub jet_key_id: String,
    pub jet_signature: String,
}

impl IndexRecord {
    pub(crate) fn canonical_json(&self) -> String {
        String::from_utf8(canonical_record_bytes(self)).unwrap_or_default()
    }
}

impl IndexProof {
    pub(crate) fn canonical_json(&self) -> String {
        let mut output = String::new();
        output.push_str("{\"schema\":");
        output.push_str(&self.schema.to_string());
        output.push_str(",\"channel\":\"");
        output.push_str(&json_escape(&self.channel));
        output.push_str("\",\"revision\":\"");
        output.push_str(&json_escape(&self.revision));
        output.push_str("\",\"system\":\"");
        output.push_str(&json_escape(&self.system));
        output.push_str("\",\"attrpath\":");
        encode_attrpath(&mut output, &self.attrpath);
        output.push_str(",\"manifest_generation\":");
        output.push_str(&self.manifest_generation.to_string());
        output.push_str(",\"manifest_sha256\":\"");
        output.push_str(&json_escape(&self.manifest_sha256));
        output.push_str("\",\"index_sha256\":\"");
        output.push_str(&json_escape(&self.index_sha256));
        output.push_str("\",\"record_sha256\":\"");
        output.push_str(&json_escape(&self.record_sha256));
        output.push_str("\",\"jet_key_id\":\"");
        output.push_str(&json_escape(&self.jet_key_id));
        output.push_str("\",\"jet_signature\":\"");
        output.push_str(&json_escape(&self.jet_signature));
        output.push_str("\"}");
        output
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VerifiedIndexRecord {
    pub record: IndexRecord,
    pub proof: IndexProof,
}

pub(crate) trait IndexTransport {
    fn get_bounded(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, NixIndexError>;
}

impl<T: IndexTransport + ?Sized> IndexTransport for &T {
    fn get_bounded(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, NixIndexError> {
        (**self).get_bounded(url, max_bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NixIndexError {
    Invalid(String),
    NotIndexed {
        attrpath: Vec<String>,
        channel: String,
        revision: String,
        system: String,
        reason: String,
    },
    Offline(String),
    Transport(String),
}

impl NixIndexError {
    pub(crate) fn code(&self) -> u32 {
        match self {
            Self::NotIndexed { .. } => 1349,
            Self::Offline(_) => 1276,
            Self::Invalid(_) | Self::Transport(_) => 1348,
        }
    }

    fn invalid(detail: impl Into<String>) -> Self {
        Self::Invalid(detail.into())
    }

    fn offline(detail: impl Into<String>) -> Self {
        Self::Offline(detail.into())
    }
}

impl fmt::Display for NixIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(f, "E1348: {detail}"),
            Self::NotIndexed {
                attrpath,
                channel,
                revision,
                system,
                reason,
            } => write!(
                f,
                "E1349: `{}` is not covered by {channel}@{revision} for {system}: {reason}",
                attrpath.join(".")
            ),
            Self::Offline(detail) => write!(f, "E1276: {detail}"),
            Self::Transport(detail) => write!(f, "E1348: {detail}"),
        }
    }
}

impl std::error::Error for NixIndexError {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OutputWire {
    name: String,
    store_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RecordWire {
    attrpath: Vec<String>,
    version: String,
    drv_path: String,
    outputs: Vec<OutputWire>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CoverageMiss {
    attrpath: Vec<String>,
    reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Coverage {
    indexed: Vec<Vec<String>>,
    not_indexed: Vec<CoverageMiss>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IndexDocument {
    schema: u64,
    channel: String,
    revision: String,
    system: String,
    released_unix: u64,
    records: Vec<RecordWire>,
    coverage: Coverage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IndexSignature {
    schema: u64,
    key_id: String,
    algorithm: String,
    signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManifestTarget {
    revision: String,
    system: String,
    url: String,
    signature_url: String,
    sha256: String,
    compressed_length: u64,
    decoded_length: u64,
    record_count: u64,
    index_signature_sha256: String,
    discoverable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChannelManifest {
    schema: u64,
    channel: String,
    generation: u64,
    issued_unix: u64,
    expires_unix: u64,
    targets: Vec<ManifestTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CachedManifest {
    manifest: ChannelManifest,
    bytes: Vec<u8>,
    signature_bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OracleRecord {
    pub(crate) record: IndexRecord,
    pub(crate) cache_admitted: bool,
}

/// Native HTTP seam used by `from_roots`.  HTTPS is intentionally left to the
/// #2156 native transport handoff; loopback HTTP remains useful for isolated
/// tests and is accepted only through the paired test configuration files.
struct NativeIndexTransport;

impl IndexTransport for NativeIndexTransport {
    fn get_bounded(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, NixIndexError> {
        let rest = url.strip_prefix("http://").ok_or_else(|| {
            NixIndexError::Transport(
                "the native index transport requires the #2156 HTTPS seam for production URLs"
                    .to_string(),
            )
        })?;
        let (authority, path) = match rest.split_once('/') {
            Some((authority, path)) => (authority, format!("/{path}")),
            None => (rest, "/".to_string()),
        };
        if authority.is_empty() || !is_loopback_host(authority) {
            return Err(NixIndexError::Transport(
                "plain HTTP index transport is restricted to loopback".to_string(),
            ));
        }
        let mut addresses = authority.to_socket_addrs().map_err(|error| {
            NixIndexError::Transport(format!("resolve index endpoint: {error}"))
        })?;
        let address = addresses
            .next()
            .ok_or_else(|| NixIndexError::Transport("index endpoint has no address".to_string()))?;
        let mut stream = TcpStream::connect(address).map_err(|error| {
            NixIndexError::Transport(format!("connect index endpoint: {error}"))
        })?;
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .map_err(|error| {
                NixIndexError::Transport(format!("request index endpoint: {error}"))
            })?;
        let limit = usize::try_from(max_bytes)
            .map_err(|_| NixIndexError::invalid("index response bound is too large"))?;
        let mut response = Vec::new();
        stream
            .take((limit as u64).saturating_add(64 * 1024))
            .read_to_end(&mut response)
            .map_err(|error| NixIndexError::Transport(format!("read index endpoint: {error}")))?;
        let header_end = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| {
                NixIndexError::Transport("index response has no HTTP header".to_string())
            })?;
        let header = std::str::from_utf8(&response[..header_end]).map_err(|_| {
            NixIndexError::Transport("index response header is not UTF-8".to_string())
        })?;
        if !header.starts_with("HTTP/1.1 200 ") && !header.starts_with("HTTP/1.0 200 ") {
            return Err(NixIndexError::Transport(format!(
                "index endpoint returned {}",
                header.lines().next().unwrap_or("unknown status")
            )));
        }
        let body = &response[header_end + 4..];
        if body.len() > limit {
            return Err(NixIndexError::invalid("index response exceeds its bound"));
        }
        if let Some(length) = header.lines().find_map(|line| {
            line.strip_prefix("Content-Length:")
                .and_then(|value| value.trim().parse::<usize>().ok())
        }) {
            if length != body.len() {
                return Err(NixIndexError::Transport(
                    "index response Content-Length disagrees".to_string(),
                ));
            }
        }
        Ok(body.to_vec())
    }
}

trait IndexClock {
    fn now_unix(&self) -> u64;
}

impl<T: IndexClock + ?Sized> IndexClock for &T {
    fn now_unix(&self) -> u64 {
        (**self).now_unix()
    }
}

struct SystemIndexClock;

impl IndexClock for SystemIndexClock {
    fn now_unix(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0)
    }
}

pub(crate) struct NixIndexClient<'a> {
    endpoint: String,
    root: PathBuf,
    key_id: String,
    public_key: VerifyingKey,
    offline: bool,
    clock: Box<dyn IndexClock + 'a>,
    transport: Box<dyn IndexTransport + 'a>,
}

impl NixIndexClient<'static> {
    pub(crate) fn from_roots(roots: &Roots) -> Result<Self, NixIndexError> {
        Self::from_roots_with_mode(roots, false)
    }

    pub(crate) fn from_roots_with_mode(
        roots: &Roots,
        offline: bool,
    ) -> Result<Self, NixIndexError> {
        ensure_real_directory(&roots.root)?;
        let trust_path = roots.root.join("trust/nix-index-v1.ed25519.pub");
        let endpoint_path = roots.root.join("config/nix-index-v1.endpoint");
        for parent in [trust_path.parent(), endpoint_path.parent()]
            .into_iter()
            .flatten()
        {
            if path_exists(parent)? {
                ensure_real_directory(parent)?;
            }
        }
        let trust_exists = path_exists(&trust_path)?;
        let endpoint_exists = path_exists(&endpoint_path)?;
        let (key_id, public_key, endpoint) = match (trust_exists, endpoint_exists) {
            (true, true) => {
                let trust = read_regular(&trust_path, 4096)?;
                let endpoint = read_regular(&endpoint_path, 4096)?;
                let text = std::str::from_utf8(&trust)
                    .map_err(|_| NixIndexError::invalid("nix index public key is not UTF-8"))?;
                let (key_id, encoded) = text.trim().split_once(':').ok_or_else(|| {
                    NixIndexError::invalid("nix index public key must be key-id:base64-public-key")
                })?;
                let key_id = nonempty_text(key_id, "nix index key id")?.to_string();
                let key_bytes = decode_base64(encoded, false, false)
                    .map_err(|_| NixIndexError::invalid("nix index public key is not base64"))?;
                let key_bytes: [u8; 32] = key_bytes
                    .try_into()
                    .map_err(|_| NixIndexError::invalid("nix index public key must be 32 bytes"))?;
                let endpoint = parse_endpoint(
                    std::str::from_utf8(&endpoint)
                        .map_err(|_| NixIndexError::invalid("nix index endpoint is not UTF-8"))?
                        .trim(),
                )?;
                (key_id, key_bytes, endpoint)
            }
            (false, false) => (
                DEFAULT_KEY_ID.to_string(),
                DEFAULT_PUBLIC_KEY,
                DEFAULT_ENDPOINT.to_string(),
            ),
            _ => {
                return Err(NixIndexError::invalid(
                    "nix index endpoint and public-key overrides must be installed together",
                ));
            }
        };
        let public_key = VerifyingKey::from_bytes(&public_key)
            .map_err(|_| NixIndexError::invalid("nix index public key is invalid"))?;
        Ok(Self {
            endpoint,
            root: roots.root.clone(),
            key_id,
            public_key,
            offline,
            clock: Box::new(SystemIndexClock),
            transport: Box::new(NativeIndexTransport),
        })
    }
}

impl<'a> NixIndexClient<'a> {
    fn for_test(
        root: PathBuf,
        endpoint: String,
        key_id: String,
        public_key: [u8; 32],
        transport: &'a dyn IndexTransport,
        clock: &'a dyn IndexClock,
        offline: bool,
    ) -> Result<Self, NixIndexError> {
        Ok(Self {
            endpoint: parse_endpoint(&endpoint)?,
            root,
            key_id,
            public_key: VerifyingKey::from_bytes(&public_key)
                .map_err(|_| NixIndexError::invalid("test index public key is invalid"))?,
            offline,
            clock: Box::new(clock),
            transport: Box::new(transport),
        })
    }

    pub(crate) fn resolve(&self, key: &IndexKey) -> Result<VerifiedIndexRecord, NixIndexError> {
        validate_key(key)?;
        let now = self.clock.now_unix();
        let cached = self.load_manifest(&key.channel);
        let manifest = match cached {
            Ok(Some(manifest)) if manifest.manifest.expires_unix > now => manifest,
            Ok(Some(_)) if self.is_offline() => {
                return Err(NixIndexError::offline(
                    "signed nixpkgs channel manifest expired; refresh required",
                ));
            }
            Ok(Some(_)) | Ok(None) if self.is_offline() => {
                return Err(NixIndexError::offline(
                    "signed nixpkgs channel manifest is not cached",
                ));
            }
            Ok(Some(_)) | Ok(None) => self.fetch_manifest(&key.channel)?,
            Err(error) if self.is_offline() => return Err(error),
            Err(_) => self.fetch_manifest(&key.channel)?,
        };
        if manifest.manifest.expires_unix <= now {
            return Err(NixIndexError::invalid(
                "signed nixpkgs channel manifest is expired",
            ));
        }
        let target = manifest
            .manifest
            .targets
            .iter()
            .find(|target| target.revision == key.revision && target.system == key.system)
            .cloned()
            .ok_or_else(|| {
                NixIndexError::invalid(format!(
                    "manifest has no target for {}@{} on {}",
                    key.channel, key.revision, key.system
                ))
            })?;
        let (compressed, signature_bytes, cached_target) = self.load_target(&key, &target)?;
        let (compressed, signature_bytes) =
            if let (Some(compressed), Some(signature_bytes)) = (compressed, signature_bytes) {
                (compressed, signature_bytes)
            } else {
                if self.is_offline() {
                    return Err(NixIndexError::offline(format!(
                        "signed nixpkgs index target is not cached for {} on {}",
                        key.revision, key.system
                    )));
                }
                let compressed = self
                    .transport
                    .get_bounded(&target.url, target.compressed_length)?;
                verify_digest_and_length(
                    &compressed,
                    target.compressed_length,
                    &target.sha256,
                    "nix index target",
                )?;
                let signature_bytes = self
                    .transport
                    .get_bounded(&target.signature_url, MAX_SIGNATURE_BYTES)?;
                if sha256_hex(&signature_bytes) != target.index_signature_sha256 {
                    return Err(NixIndexError::invalid(
                        "nix index signature sidecar digest disagrees with manifest",
                    ));
                }
                (compressed, signature_bytes)
            };
        let decoded = zstd_decode_bounded(&compressed, MAX_DECODED_BYTES)?;
        if decoded.len() as u64 != target.decoded_length {
            return Err(NixIndexError::invalid(
                "nix index decoded length disagrees with manifest",
            ));
        }
        let signature = parse_signature_strict(&signature_bytes)?;
        verify_index_signature(&self.public_key, &self.key_id, &signature, &decoded)?;
        let document = parse_index_strict(&decoded)?;
        validate_document(&document)?;
        if document.channel != key.channel
            || document.revision != key.revision
            || document.system != key.system
        {
            return Err(NixIndexError::invalid(
                "signed nixpkgs index target disagrees with its requested key",
            ));
        }
        if document.records.len() as u64 != target.record_count {
            return Err(NixIndexError::invalid(
                "nix index record count disagrees with manifest",
            ));
        }
        let record = document
            .records
            .iter()
            .find(|record| record.attrpath == key.attrpath)
            .map(record_from_wire);
        let record = match record {
            Some(record) => record,
            None => {
                let reason = document
                    .coverage
                    .not_indexed
                    .iter()
                    .find(|miss| miss.attrpath == key.attrpath)
                    .map(|miss| miss.reason.clone())
                    .unwrap_or_else(|| "not-listed".to_string());
                return Err(NixIndexError::NotIndexed {
                    attrpath: key.attrpath.clone(),
                    channel: key.channel.clone(),
                    revision: key.revision.clone(),
                    system: key.system.clone(),
                    reason,
                });
            }
        };
        if !cached_target {
            self.cache_target_atomically(key, &target, &compressed, &signature_bytes)?;
        }
        self.cache_manifest_atomically(&key.channel, &manifest)?;
        let index_sha256 = sha256_hex(&compressed);
        let proof = IndexProof {
            schema: INDEX_SCHEMA,
            channel: key.channel.clone(),
            revision: key.revision.clone(),
            system: key.system.clone(),
            attrpath: key.attrpath.clone(),
            manifest_generation: manifest.manifest.generation,
            manifest_sha256: sha256_hex(&manifest.bytes),
            index_sha256,
            record_sha256: sha256_hex(&canonical_record_bytes(&record)),
            jet_key_id: self.key_id.clone(),
            jet_signature: signature.signature,
        };
        Ok(VerifiedIndexRecord { record, proof })
    }

    fn is_offline(&self) -> bool {
        self.offline
    }

    fn load_manifest(&self, channel: &str) -> Result<Option<CachedManifest>, NixIndexError> {
        let directory = self.manifest_dir(channel)?;
        let bytes_path = directory.join("current.json");
        let signature_path = directory.join("current.sig.json");
        let bytes_exists = path_exists(&bytes_path)?;
        let signature_exists = path_exists(&signature_path)?;
        if !bytes_exists && !signature_exists {
            return Ok(None);
        }
        if !bytes_exists || !signature_exists {
            return Err(NixIndexError::invalid(
                "cached nix index manifest is missing its signature pair",
            ));
        }
        let bytes = read_regular(&bytes_path, MAX_MANIFEST_BYTES)?;
        let signature_bytes = read_regular(&signature_path, MAX_SIGNATURE_BYTES)?;
        let manifest = parse_manifest_strict(&bytes)?;
        validate_manifest(&manifest)?;
        verify_manifest_signature(&self.public_key, &self.key_id, &signature_bytes, &bytes)?;
        self.check_generation(channel, &manifest, &bytes)?;
        Ok(Some(CachedManifest {
            manifest,
            bytes,
            signature_bytes,
        }))
    }

    fn fetch_manifest(&self, channel: &str) -> Result<CachedManifest, NixIndexError> {
        let base = format!(
            "{}/v1/{channel}/manifest.json",
            self.endpoint.trim_end_matches('/')
        );
        let bytes = self.transport.get_bounded(&base, MAX_MANIFEST_BYTES)?;
        let signature_url = format!("{base}.sig.json");
        let signature_bytes = self
            .transport
            .get_bounded(&signature_url, MAX_SIGNATURE_BYTES)?;
        let manifest = parse_manifest_strict(&bytes)?;
        validate_manifest(&manifest)?;
        if manifest.channel != channel {
            return Err(NixIndexError::invalid(
                "signed nixpkgs channel manifest disagrees with requested channel",
            ));
        }
        verify_manifest_signature(&self.public_key, &self.key_id, &signature_bytes, &bytes)?;
        self.check_generation(channel, &manifest, &bytes)?;
        Ok(CachedManifest {
            manifest,
            bytes,
            signature_bytes,
        })
    }

    fn check_generation(
        &self,
        channel: &str,
        manifest: &ChannelManifest,
        bytes: &[u8],
    ) -> Result<(), NixIndexError> {
        let Some((generation, digest)) = self.load_highest_generation(channel)? else {
            return Ok(());
        };
        let current_digest = sha256_hex(bytes);
        if manifest.generation < generation
            || (manifest.generation == generation && current_digest != digest)
        {
            return Err(NixIndexError::invalid(
                "signed nixpkgs channel manifest rolled back or forked",
            ));
        }
        Ok(())
    }

    fn load_target(
        &self,
        key: &IndexKey,
        target: &ManifestTarget,
    ) -> Result<(Option<Vec<u8>>, Option<Vec<u8>>, bool), NixIndexError> {
        let bytes_path = self.target_path(key, &target.sha256)?;
        let signature_path = PathBuf::from(format!("{}.sig.json", bytes_path.display()));
        let bytes_exists = path_exists(&bytes_path)?;
        let signature_exists = path_exists(&signature_path)?;
        if !bytes_exists && !signature_exists {
            return Ok((None, None, false));
        }
        if !bytes_exists || !signature_exists {
            return Err(NixIndexError::invalid(
                "cached nix index target is missing its signature pair",
            ));
        }
        let bytes = read_regular(&bytes_path, MAX_COMPRESSED_BYTES as u64)?;
        verify_digest_and_length(
            &bytes,
            target.compressed_length,
            &target.sha256,
            "cached nix index target",
        )?;
        let signature = read_regular(&signature_path, MAX_SIGNATURE_BYTES)?;
        if sha256_hex(&signature) != target.index_signature_sha256 {
            return Err(NixIndexError::invalid(
                "cached nix index signature sidecar digest disagrees with manifest",
            ));
        }
        Ok((Some(bytes), Some(signature), true))
    }

    fn cache_manifest_atomically(
        &self,
        channel: &str,
        manifest: &CachedManifest,
    ) -> Result<(), NixIndexError> {
        let directory = self.manifest_dir(channel)?;
        ensure_real_directory(&directory)?;
        let immutable = directory.join(format!("{}.json", manifest.manifest.generation));
        let immutable_signature =
            directory.join(format!("{}.sig.json", manifest.manifest.generation));
        write_atomic(&immutable, &manifest.bytes, false)?;
        write_atomic(&immutable_signature, &manifest.signature_bytes, false)?;
        write_atomic(&directory.join("current.json"), &manifest.bytes, true)?;
        write_atomic(
            &directory.join("current.sig.json"),
            &manifest.signature_bytes,
            true,
        )?;
        self.record_highest_generation_atomically(channel, &manifest.manifest, &manifest.bytes)
    }

    fn cache_target_atomically(
        &self,
        key: &IndexKey,
        target: &ManifestTarget,
        bytes: &[u8],
        signature: &[u8],
    ) -> Result<(), NixIndexError> {
        let path = self.target_path(key, &target.sha256)?;
        let signature_path = PathBuf::from(format!("{}.sig.json", path.display()));
        ensure_real_directory(
            path.parent()
                .ok_or_else(|| NixIndexError::invalid("nix index target has no parent"))?,
        )?;
        write_atomic(&path, bytes, false)?;
        write_atomic(&signature_path, signature, false)
    }

    fn record_highest_generation_atomically(
        &self,
        channel: &str,
        manifest: &ChannelManifest,
        bytes: &[u8],
    ) -> Result<(), NixIndexError> {
        let directory = self.generation_dir(channel)?;
        ensure_real_directory(&directory)?;
        let state = format!("{}\n{}\n", manifest.generation, sha256_hex(bytes));
        write_atomic(&directory.join("highest"), state.as_bytes(), true)
    }

    fn load_highest_generation(
        &self,
        channel: &str,
    ) -> Result<Option<(u64, String)>, NixIndexError> {
        let path = self.generation_dir(channel)?.join("highest");
        if !path_exists(&path)? {
            return Ok(None);
        }
        let bytes = read_regular(&path, 256)?;
        let mut lines = std::str::from_utf8(&bytes)
            .map_err(|_| NixIndexError::invalid("nix index rollback state is not UTF-8"))?
            .lines();
        let generation = lines
            .next()
            .and_then(|line| line.parse::<u64>().ok())
            .ok_or_else(|| NixIndexError::invalid("nix index rollback generation is malformed"))?;
        let digest = lines
            .next()
            .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| NixIndexError::invalid("nix index rollback digest is malformed"))?
            .to_string();
        if lines.next().is_some() {
            return Err(NixIndexError::invalid(
                "nix index rollback state has trailing data",
            ));
        }
        Ok(Some((generation, digest)))
    }

    fn manifest_dir(&self, channel: &str) -> Result<PathBuf, NixIndexError> {
        validate_channel(channel)?;
        Ok(self.root.join(INDEX_ROOT).join("manifests").join(channel))
    }

    fn generation_dir(&self, channel: &str) -> Result<PathBuf, NixIndexError> {
        validate_channel(channel)?;
        Ok(self.root.join(INDEX_ROOT).join("generations").join(channel))
    }

    fn target_path(&self, key: &IndexKey, digest: &str) -> Result<PathBuf, NixIndexError> {
        validate_key(key)?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(NixIndexError::invalid(
                "nix index target digest is malformed",
            ));
        }
        Ok(self
            .root
            .join(INDEX_ROOT)
            .join("targets")
            .join(&key.revision)
            .join(&key.system)
            .join(format!("{digest}.json.zst")))
    }
}

fn path_exists(path: &Path) -> Result<bool, NixIndexError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(NixIndexError::Transport(format!(
            "inspect {}: {error}",
            path.display()
        ))),
    }
}

fn ensure_real_directory(path: &Path) -> Result<(), NixIndexError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(NixIndexError::invalid(format!(
                        "nix index state path `{}` is not a real directory",
                        current.display()
                    )));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|create_error| {
                    NixIndexError::Transport(format!(
                        "create {}: {create_error}",
                        current.display()
                    ))
                })?;
            }
            Err(error) => {
                return Err(NixIndexError::Transport(format!(
                    "inspect {}: {error}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, NixIndexError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| NixIndexError::Transport(format!("read {}: {error}", path.display())))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(NixIndexError::invalid(format!(
            "nix index path `{}` is not a regular file",
            path.display()
        )));
    }
    if metadata.len() > limit {
        return Err(NixIndexError::invalid(format!(
            "nix index file `{}` exceeds its bound",
            path.display()
        )));
    }
    fs::read(path)
        .map_err(|error| NixIndexError::Transport(format!("read {}: {error}", path.display())))
}

fn write_atomic(path: &Path, bytes: &[u8], replace: bool) -> Result<(), NixIndexError> {
    let parent = path
        .parent()
        .ok_or_else(|| NixIndexError::invalid("nix index state file has no parent"))?;
    ensure_real_directory(parent)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(NixIndexError::invalid(format!(
                "nix index state file `{}` is not regular",
                path.display()
            )));
        }
        if !replace {
            let current = fs::read(path).map_err(|error| {
                NixIndexError::Transport(format!("read existing {}: {error}", path.display()))
            })?;
            if current == bytes {
                return Ok(());
            }
            return Err(NixIndexError::invalid(format!(
                "immutable nix index object `{}` changed",
                path.display()
            )));
        }
    }
    let serial = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{}.partial-{}-{serial}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("index"),
        std::process::id()
    ));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        #[cfg(windows)]
        if replace && fs::symlink_metadata(path).is_ok() {
            fs::remove_file(path)?;
        }
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| NixIndexError::Transport(format!("write {}: {error}", path.display())))
}

fn validate_channel(channel: &str) -> Result<(), NixIndexError> {
    if CHANNELS.contains(&channel) {
        Ok(())
    } else {
        Err(NixIndexError::invalid(format!(
            "unsupported nixpkgs channel `{channel}`"
        )))
    }
}

fn validate_system(system: &str) -> Result<(), NixIndexError> {
    if SYSTEMS.contains(&system) {
        Ok(())
    } else {
        Err(NixIndexError::invalid(format!(
            "unsupported nixpkgs system `{system}`"
        )))
    }
}

fn validate_revision(revision: &str) -> Result<(), NixIndexError> {
    if revision.len() == 40
        && revision
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(NixIndexError::invalid(
            "nixpkgs revision must be exactly 40 lowercase hexadecimal characters",
        ))
    }
}

fn validate_key(key: &IndexKey) -> Result<(), NixIndexError> {
    validate_channel(&key.channel)?;
    validate_revision(&key.revision)?;
    validate_system(&key.system)?;
    validate_attrpath(&key.attrpath)
}

fn validate_attrpath(attrpath: &[String]) -> Result<(), NixIndexError> {
    if attrpath.is_empty() {
        return Err(NixIndexError::invalid("nix index attrpath cannot be empty"));
    }
    if attrpath.iter().any(|segment| {
        segment.is_empty()
            || segment.chars().any(|character| character.is_control())
            || segment.contains('\0')
    }) {
        return Err(NixIndexError::invalid(
            "nix index attrpath contains an invalid segment",
        ));
    }
    Ok(())
}

fn validate_document(document: &IndexDocument) -> Result<(), NixIndexError> {
    if document.schema != INDEX_SCHEMA {
        return Err(NixIndexError::invalid("unsupported nix index schema"));
    }
    validate_channel(&document.channel)?;
    validate_revision(&document.revision)?;
    validate_system(&document.system)?;
    if document.records.len() > MAX_RECORDS {
        return Err(NixIndexError::invalid("nix index has too many records"));
    }
    let mut record_keys = BTreeSet::new();
    let mut records = BTreeSet::new();
    for record in &document.records {
        validate_attrpath(&record.attrpath)?;
        if !record_keys.insert(attrpath_key(&record.attrpath)) {
            return Err(NixIndexError::invalid(
                "nix index has duplicate record keys",
            ));
        }
        if record.version.is_empty()
            || record
                .version
                .chars()
                .any(|character| character.is_control())
        {
            return Err(NixIndexError::invalid(
                "nix index record has an empty or invalid version",
            ));
        }
        validate_store_path(&record.drv_path, true)?;
        let mut output_names = BTreeSet::new();
        let mut output_paths = BTreeSet::new();
        if record.outputs.is_empty() {
            return Err(NixIndexError::invalid("nix index record has no outputs"));
        }
        for output in &record.outputs {
            if output.name.is_empty() || !output_names.insert(output.name.clone()) {
                return Err(NixIndexError::invalid(
                    "nix index record has duplicate output names",
                ));
            }
            validate_store_path(&output.store_path, false)?;
            if !output_paths.insert(output.store_path.clone()) {
                return Err(NixIndexError::invalid(
                    "nix index assigns one store path to multiple output names",
                ));
            }
        }
        if !output_names.contains("out") && !output_names.contains("bin") {
            return Err(NixIndexError::invalid(
                "nix index record has neither a primary `out` nor `bin` output",
            ));
        }
        records.insert(attrpath_key(&record.attrpath));
    }
    let mut indexed = BTreeSet::new();
    for attrpath in &document.coverage.indexed {
        validate_attrpath(attrpath)?;
        if !indexed.insert(attrpath_key(attrpath)) {
            return Err(NixIndexError::invalid(
                "nix index coverage repeats an indexed attrpath",
            ));
        }
    }
    if indexed != records {
        return Err(NixIndexError::invalid(
            "nix index coverage does not exactly match indexed records",
        ));
    }
    let mut misses = BTreeSet::new();
    for miss in &document.coverage.not_indexed {
        validate_attrpath(&miss.attrpath)?;
        if !COVERAGE_REASONS.contains(&miss.reason.as_str()) {
            return Err(NixIndexError::invalid(
                "nix index has an unknown coverage reason",
            ));
        }
        if !misses.insert(attrpath_key(&miss.attrpath)) {
            return Err(NixIndexError::invalid(
                "nix index coverage repeats a missing attrpath",
            ));
        }
        if records.contains(&attrpath_key(&miss.attrpath)) {
            return Err(NixIndexError::invalid(
                "nix index lists one attrpath as indexed and not-indexed",
            ));
        }
    }
    Ok(())
}

fn validate_store_path(path: &str, derivation: bool) -> Result<(), NixIndexError> {
    let rest = path
        .strip_prefix("/nix/store/")
        .ok_or_else(|| NixIndexError::invalid(format!("malformed Nix store path `{path}`")))?;
    let (hash, name) = rest
        .split_once('-')
        .ok_or_else(|| NixIndexError::invalid(format!("malformed Nix store path `{path}`")))?;
    if hash.len() != 32
        || !hash.bytes().all(|byte| NIX32.contains(&byte))
        || name.is_empty()
        || name.contains('/')
        || name.chars().any(|character| character.is_control())
        || (derivation && !name.ends_with(".drv"))
    {
        return Err(NixIndexError::invalid(format!(
            "malformed Nix store path `{path}`"
        )));
    }
    Ok(())
}

fn attrpath_key(attrpath: &[String]) -> Vec<Vec<u8>> {
    attrpath
        .iter()
        .map(|segment| segment.as_bytes().to_vec())
        .collect()
}

fn compare_attrpath(left: &[String], right: &[String]) -> std::cmp::Ordering {
    for (a, b) in left.iter().zip(right) {
        match a.as_bytes().cmp(b.as_bytes()) {
            std::cmp::Ordering::Equal => {}
            order => return order,
        }
    }
    left.len().cmp(&right.len())
}

fn record_from_wire(record: &RecordWire) -> IndexRecord {
    IndexRecord {
        attrpath: record.attrpath.clone(),
        version: record.version.clone(),
        drv_path: record.drv_path.clone(),
        outputs: record
            .outputs
            .iter()
            .map(|output| (output.name.clone(), output.store_path.clone()))
            .collect(),
    }
}

fn wire_from_record(record: &IndexRecord) -> RecordWire {
    RecordWire {
        attrpath: record.attrpath.clone(),
        version: record.version.clone(),
        drv_path: record.drv_path.clone(),
        outputs: record
            .outputs
            .iter()
            .map(|(name, store_path)| OutputWire {
                name: name.clone(),
                store_path: store_path.clone(),
            })
            .collect(),
    }
}

fn canonical_index_bytes(document: &IndexDocument) -> Result<Vec<u8>, NixIndexError> {
    let mut document = document.clone();
    validate_document(&document)?;
    document
        .records
        .sort_by(|left, right| compare_attrpath(&left.attrpath, &right.attrpath));
    for record in &mut document.records {
        record
            .outputs
            .sort_by(|left, right| left.name.as_bytes().cmp(right.name.as_bytes()));
    }
    document
        .coverage
        .indexed
        .sort_by(|left, right| compare_attrpath(left, right));
    document.coverage.not_indexed.sort_by(|left, right| {
        compare_attrpath(&left.attrpath, &right.attrpath)
            .then_with(|| left.reason.as_bytes().cmp(right.reason.as_bytes()))
    });
    let mut output = String::new();
    output.push_str("{\"schema\":");
    output.push_str(&document.schema.to_string());
    output.push_str(",\"channel\":\"");
    output.push_str(&json_escape(&document.channel));
    output.push_str("\",\"revision\":\"");
    output.push_str(&json_escape(&document.revision));
    output.push_str("\",\"system\":\"");
    output.push_str(&json_escape(&document.system));
    output.push_str("\",\"released_unix\":");
    output.push_str(&document.released_unix.to_string());
    output.push_str(",\"records\":[");
    for (index, record) in document.records.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        encode_record(&mut output, record);
    }
    output.push_str("],\"coverage\":{\"indexed\":[");
    for (index, attrpath) in document.coverage.indexed.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        encode_attrpath(&mut output, attrpath);
    }
    output.push_str("],\"notIndexed\":[");
    for (index, miss) in document.coverage.not_indexed.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str("{\"attrpath\":");
        encode_attrpath(&mut output, &miss.attrpath);
        output.push_str(",\"reason\":\"");
        output.push_str(&json_escape(&miss.reason));
        output.push_str("\"}");
    }
    output.push_str("]}}");
    Ok(output.into_bytes())
}

fn canonical_record_bytes(record: &IndexRecord) -> Vec<u8> {
    let mut output = String::new();
    encode_record(&mut output, &wire_from_record(record));
    output.into_bytes()
}

fn encode_record(output: &mut String, record: &RecordWire) {
    output.push_str("{\"attrpath\":");
    encode_attrpath(output, &record.attrpath);
    output.push_str(",\"version\":\"");
    output.push_str(&json_escape(&record.version));
    output.push_str("\",\"drvPath\":\"");
    output.push_str(&json_escape(&record.drv_path));
    output.push_str("\",\"outputs\":[");
    for (index, output_wire) in record.outputs.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str("{\"name\":\"");
        output.push_str(&json_escape(&output_wire.name));
        output.push_str("\",\"storePath\":\"");
        output.push_str(&json_escape(&output_wire.store_path));
        output.push_str("\"}");
    }
    output.push_str("]}");
}

fn encode_attrpath(output: &mut String, attrpath: &[String]) {
    output.push('[');
    for (index, segment) in attrpath.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push('"');
        output.push_str(&json_escape(segment));
        output.push('"');
    }
    output.push(']');
}

fn canonical_manifest_bytes(manifest: &ChannelManifest) -> Result<Vec<u8>, NixIndexError> {
    validate_manifest(manifest)?;
    let mut targets = manifest.targets.clone();
    targets.sort_by(|left, right| {
        left.revision
            .as_bytes()
            .cmp(right.revision.as_bytes())
            .then_with(|| left.system.as_bytes().cmp(right.system.as_bytes()))
    });
    let mut output = String::new();
    output.push_str("{\"schema\":");
    output.push_str(&manifest.schema.to_string());
    output.push_str(",\"channel\":\"");
    output.push_str(&json_escape(&manifest.channel));
    output.push_str("\",\"generation\":");
    output.push_str(&manifest.generation.to_string());
    output.push_str(",\"issued_unix\":");
    output.push_str(&manifest.issued_unix.to_string());
    output.push_str(",\"expires_unix\":");
    output.push_str(&manifest.expires_unix.to_string());
    output.push_str(",\"targets\":[");
    for (index, target) in targets.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str("{\"revision\":\"");
        output.push_str(&json_escape(&target.revision));
        output.push_str("\",\"system\":\"");
        output.push_str(&json_escape(&target.system));
        output.push_str("\",\"url\":\"");
        output.push_str(&json_escape(&target.url));
        output.push_str("\",\"signature_url\":\"");
        output.push_str(&json_escape(&target.signature_url));
        output.push_str("\",\"sha256\":\"");
        output.push_str(&json_escape(&target.sha256));
        output.push_str("\",\"compressed_length\":");
        output.push_str(&target.compressed_length.to_string());
        output.push_str(",\"decoded_length\":");
        output.push_str(&target.decoded_length.to_string());
        output.push_str(",\"record_count\":");
        output.push_str(&target.record_count.to_string());
        output.push_str(",\"index_signature_sha256\":\"");
        output.push_str(&json_escape(&target.index_signature_sha256));
        output.push_str("\",\"discoverable\":");
        output.push_str(if target.discoverable { "true" } else { "false" });
        output.push('}');
    }
    output.push_str("]}");
    Ok(output.into_bytes())
}

fn canonical_signature_bytes(signature: &IndexSignature) -> Vec<u8> {
    format!(
        "{{\"schema\":{},\"key_id\":\"{}\",\"algorithm\":\"{}\",\"signature\":\"{}\"}}",
        signature.schema,
        json_escape(&signature.key_id),
        json_escape(&signature.algorithm),
        json_escape(&signature.signature)
    )
    .into_bytes()
}

fn parse_index_strict(bytes: &[u8]) -> Result<IndexDocument, NixIndexError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| NixIndexError::invalid("nix index is not UTF-8"))?;
    let value = parse_json_exact_numbers(text, true).map_err(|error| {
        NixIndexError::invalid(format!(
            "parse nix index JSON at line {}: {}",
            error.line, error.message
        ))
    })?;
    let map = object(value, "nix index")?;
    reject_unknown(
        &map,
        &[
            "schema",
            "channel",
            "revision",
            "system",
            "released_unix",
            "records",
            "coverage",
        ],
    )?;
    let document = IndexDocument {
        schema: u64_field(&map, "schema")?,
        channel: string_field(&map, "channel")?.to_string(),
        revision: string_field(&map, "revision")?.to_string(),
        system: string_field(&map, "system")?.to_string(),
        released_unix: u64_field(&map, "released_unix")?,
        records: parse_records(field(&map, "records")?)?,
        coverage: parse_coverage(field(&map, "coverage")?)?,
    };
    validate_document(&document)?;
    if canonical_index_bytes(&document)? != bytes {
        return Err(NixIndexError::invalid("nix index bytes are not canonical"));
    }
    Ok(document)
}

fn parse_manifest_strict(bytes: &[u8]) -> Result<ChannelManifest, NixIndexError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| NixIndexError::invalid("nix index manifest is not UTF-8"))?;
    let value = parse_json_exact_numbers(text, true).map_err(|error| {
        NixIndexError::invalid(format!(
            "parse nix index manifest JSON at line {}: {}",
            error.line, error.message
        ))
    })?;
    let map = object(value, "nix index manifest")?;
    reject_unknown(
        &map,
        &[
            "schema",
            "channel",
            "generation",
            "issued_unix",
            "expires_unix",
            "targets",
        ],
    )?;
    let targets = array_field(&map, "targets")?
        .iter()
        .map(parse_manifest_target)
        .collect::<Result<Vec<_>, _>>()?;
    let manifest = ChannelManifest {
        schema: u64_field(&map, "schema")?,
        channel: string_field(&map, "channel")?.to_string(),
        generation: u64_field(&map, "generation")?,
        issued_unix: u64_field(&map, "issued_unix")?,
        expires_unix: u64_field(&map, "expires_unix")?,
        targets,
    };
    validate_manifest(&manifest)?;
    if canonical_manifest_bytes(&manifest)? != bytes {
        return Err(NixIndexError::invalid(
            "nix index manifest bytes are not canonical",
        ));
    }
    Ok(manifest)
}

fn parse_signature_strict(bytes: &[u8]) -> Result<IndexSignature, NixIndexError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| NixIndexError::invalid("nix index signature is not UTF-8"))?;
    let value = parse_json_exact_numbers(text, true).map_err(|error| {
        NixIndexError::invalid(format!("parse nix index signature: {}", error.message))
    })?;
    let map = object(value, "nix index signature")?;
    reject_unknown(&map, &["schema", "key_id", "algorithm", "signature"])?;
    let signature = IndexSignature {
        schema: u64_field(&map, "schema")?,
        key_id: string_field(&map, "key_id")?.to_string(),
        algorithm: string_field(&map, "algorithm")?.to_string(),
        signature: string_field(&map, "signature")?.to_string(),
    };
    if canonical_signature_bytes(&signature) != bytes {
        return Err(NixIndexError::invalid(
            "nix index signature bytes are not canonical",
        ));
    }
    Ok(signature)
}

fn parse_records(value: &Value) -> Result<Vec<RecordWire>, NixIndexError> {
    let values = value_array(value, "records")?;
    if values.len() > MAX_RECORDS {
        return Err(NixIndexError::invalid("nix index has too many records"));
    }
    values.iter().map(parse_record).collect()
}

fn parse_record(value: &Value) -> Result<RecordWire, NixIndexError> {
    let map = object(value.clone(), "nix index record")?;
    reject_unknown(&map, &["attrpath", "version", "drvPath", "outputs"])?;
    let outputs = array_field(&map, "outputs")?
        .iter()
        .map(parse_output)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RecordWire {
        attrpath: parse_attrpath(field(&map, "attrpath")?)?,
        version: string_field(&map, "version")?.to_string(),
        drv_path: string_field(&map, "drvPath")?.to_string(),
        outputs,
    })
}

fn parse_output(value: &Value) -> Result<OutputWire, NixIndexError> {
    let map = object(value.clone(), "nix index output")?;
    reject_unknown(&map, &["name", "storePath"])?;
    Ok(OutputWire {
        name: string_field(&map, "name")?.to_string(),
        store_path: string_field(&map, "storePath")?.to_string(),
    })
}

fn parse_coverage(value: &Value) -> Result<Coverage, NixIndexError> {
    let map = object(value.clone(), "nix index coverage")?;
    reject_unknown(&map, &["indexed", "notIndexed"])?;
    let indexed = array_field(&map, "indexed")?
        .iter()
        .map(parse_attrpath)
        .collect::<Result<Vec<_>, _>>()?;
    let not_indexed = array_field(&map, "notIndexed")?
        .iter()
        .map(|value| {
            let map = object(value.clone(), "nix index coverage miss")?;
            reject_unknown(&map, &["attrpath", "reason"])?;
            Ok(CoverageMiss {
                attrpath: parse_attrpath(field(&map, "attrpath")?)?,
                reason: string_field(&map, "reason")?.to_string(),
            })
        })
        .collect::<Result<Vec<_>, NixIndexError>>()?;
    Ok(Coverage {
        indexed,
        not_indexed,
    })
}

fn parse_attrpath(value: &Value) -> Result<Vec<String>, NixIndexError> {
    value_array(value, "attrpath")?
        .iter()
        .map(|value| Ok(string_value(value, "attrpath segment")?.to_string()))
        .collect()
}

fn parse_manifest_target(value: &Value) -> Result<ManifestTarget, NixIndexError> {
    let map = object(value.clone(), "nix index manifest target")?;
    reject_unknown(
        &map,
        &[
            "revision",
            "system",
            "url",
            "signature_url",
            "sha256",
            "compressed_length",
            "decoded_length",
            "record_count",
            "index_signature_sha256",
            "discoverable",
        ],
    )?;
    Ok(ManifestTarget {
        revision: string_field(&map, "revision")?.to_string(),
        system: string_field(&map, "system")?.to_string(),
        url: string_field(&map, "url")?.to_string(),
        signature_url: string_field(&map, "signature_url")?.to_string(),
        sha256: string_field(&map, "sha256")?.to_string(),
        compressed_length: u64_field(&map, "compressed_length")?,
        decoded_length: u64_field(&map, "decoded_length")?,
        record_count: u64_field(&map, "record_count")?,
        index_signature_sha256: string_field(&map, "index_signature_sha256")?.to_string(),
        discoverable: bool_field(&map, "discoverable")?,
    })
}

fn validate_manifest(manifest: &ChannelManifest) -> Result<(), NixIndexError> {
    if manifest.schema != INDEX_SCHEMA {
        return Err(NixIndexError::invalid(
            "unsupported nix index manifest schema",
        ));
    }
    validate_channel(&manifest.channel)?;
    if manifest.expires_unix <= manifest.issued_unix {
        return Err(NixIndexError::invalid(
            "nix index manifest expiry is not after issue time",
        ));
    }
    let mut keys = BTreeSet::new();
    for target in &manifest.targets {
        validate_revision(&target.revision)?;
        validate_system(&target.system)?;
        if !keys.insert((target.revision.clone(), target.system.clone())) {
            return Err(NixIndexError::invalid(
                "nix index manifest has duplicate targets",
            ));
        }
        if target.compressed_length == 0 || target.compressed_length as usize > MAX_COMPRESSED_BYTES
        {
            return Err(NixIndexError::invalid(
                "nix index target exceeds compressed bound",
            ));
        }
        if target.decoded_length == 0 || target.decoded_length as usize > MAX_DECODED_BYTES {
            return Err(NixIndexError::invalid(
                "nix index target exceeds decoded bound",
            ));
        }
        if target.record_count as usize > MAX_RECORDS {
            return Err(NixIndexError::invalid(
                "nix index target exceeds record bound",
            ));
        }
        for (label, digest) in [
            ("nix index target", &target.sha256),
            ("nix index signature", &target.index_signature_sha256),
        ] {
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                return Err(NixIndexError::invalid(format!(
                    "{label} digest is malformed"
                )));
            }
        }
        validate_url(&target.url)?;
        validate_url(&target.signature_url)?;
    }
    Ok(())
}

fn validate_url(url: &str) -> Result<(), NixIndexError> {
    if (url.starts_with("https://") || url.starts_with("http://"))
        && !url
            .chars()
            .any(|character| character.is_control() || character == ' ')
    {
        Ok(())
    } else {
        Err(NixIndexError::invalid(
            "nix index manifest contains an invalid URL",
        ))
    }
}

fn verify_index_signature(
    public_key: &VerifyingKey,
    expected_key_id: &str,
    signature: &IndexSignature,
    bytes: &[u8],
) -> Result<(), NixIndexError> {
    if signature.schema != INDEX_SCHEMA
        || signature.key_id != expected_key_id
        || signature.algorithm != "ed25519"
    {
        return Err(NixIndexError::invalid(
            "nix index signature key or algorithm is not trusted",
        ));
    }
    let signature = decode_signature(&signature.signature)?;
    let mut message = Vec::with_capacity(INDEX_DOMAIN.len() + bytes.len());
    message.extend_from_slice(INDEX_DOMAIN);
    message.extend_from_slice(bytes);
    public_key
        .verify(&message, &Signature::from_bytes(&signature))
        .map_err(|_| NixIndexError::invalid("nix index signature verification failed"))
}

fn verify_manifest_signature(
    public_key: &VerifyingKey,
    expected_key_id: &str,
    signature_bytes: &[u8],
    bytes: &[u8],
) -> Result<(), NixIndexError> {
    let signature = parse_signature_strict(signature_bytes)?;
    if signature.key_id != expected_key_id || signature.algorithm != "ed25519" {
        return Err(NixIndexError::invalid(
            "nix index manifest signature key or algorithm is not trusted",
        ));
    }
    let signature = decode_signature(&signature.signature)?;
    let mut message = Vec::with_capacity(MANIFEST_DOMAIN.len() + bytes.len());
    message.extend_from_slice(MANIFEST_DOMAIN);
    message.extend_from_slice(bytes);
    public_key
        .verify(&message, &Signature::from_bytes(&signature))
        .map_err(|_| NixIndexError::invalid("nix index manifest signature verification failed"))
}

fn decode_signature(encoded: &str) -> Result<[u8; 64], NixIndexError> {
    let bytes = decode_base64(encoded, false, false)
        .map_err(|_| NixIndexError::invalid("nix index signature is not base64"))?;
    bytes
        .try_into()
        .map_err(|_| NixIndexError::invalid("nix index signature must be 64 bytes"))
}

fn signature_message(domain: &[u8], bytes: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(domain.len() + bytes.len());
    message.extend_from_slice(domain);
    message.extend_from_slice(bytes);
    message
}

fn sha256_hex(bytes: &[u8]) -> String {
    crate::SHA256::sha256_hex(bytes)
}

fn verify_digest_and_length(
    bytes: &[u8],
    expected_length: u64,
    expected_digest: &str,
    label: &str,
) -> Result<(), NixIndexError> {
    if bytes.len() as u64 != expected_length || sha256_hex(bytes) != expected_digest {
        return Err(NixIndexError::invalid(format!(
            "{label} digest or length mismatch"
        )));
    }
    Ok(())
}

fn object(value: Value, label: &str) -> Result<Vec<(String, Value)>, NixIndexError> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(NixIndexError::invalid(format!("{label} must be an object"))),
    }
}

fn field<'a>(map: &'a [(String, Value)], name: &str) -> Result<&'a Value, NixIndexError> {
    map.iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .ok_or_else(|| NixIndexError::invalid(format!("nix index is missing `{name}`")))
}

fn reject_unknown(map: &[(String, Value)], known: &[&str]) -> Result<(), NixIndexError> {
    if let Some((key, _)) = map.iter().find(|(key, _)| !known.contains(&key.as_str())) {
        return Err(NixIndexError::invalid(format!(
            "nix index has unknown field `{key}`"
        )));
    }
    Ok(())
}

fn string_value<'a>(value: &'a Value, label: &str) -> Result<&'a str, NixIndexError> {
    match value {
        Value::Text(value) => Ok(value),
        _ => Err(NixIndexError::invalid(format!("{label} must be a string"))),
    }
}

fn string_field<'a>(map: &'a [(String, Value)], name: &str) -> Result<&'a str, NixIndexError> {
    string_value(field(map, name)?, name)
}

fn u64_value(value: &Value, label: &str) -> Result<u64, NixIndexError> {
    let text = match value {
        Value::Number(text) => text,
        Value::Int(value) if *value >= 0 => return Ok(*value as u64),
        _ => {
            return Err(NixIndexError::invalid(format!(
                "{label} must be an integer"
            )))
        }
    };
    if text.starts_with('-') || text.contains('.') || text.contains('e') || text.contains('E') {
        return Err(NixIndexError::invalid(format!(
            "{label} must be an unsigned integer"
        )));
    }
    text.parse::<u64>()
        .map_err(|_| NixIndexError::invalid(format!("{label} is out of range")))
}

fn u64_field(map: &[(String, Value)], name: &str) -> Result<u64, NixIndexError> {
    u64_value(field(map, name)?, name)
}

fn bool_field(map: &[(String, Value)], name: &str) -> Result<bool, NixIndexError> {
    match field(map, name)? {
        Value::Bool(value) => Ok(*value),
        _ => Err(NixIndexError::invalid(format!("{name} must be a boolean"))),
    }
}

fn value_array<'a>(value: &'a Value, label: &str) -> Result<&'a [Value], NixIndexError> {
    match value {
        Value::Array(values) => Ok(values),
        _ => Err(NixIndexError::invalid(format!("{label} must be an array"))),
    }
}

fn array_field<'a>(map: &'a [(String, Value)], name: &str) -> Result<&'a [Value], NixIndexError> {
    value_array(field(map, name)?, name)
}

fn nonempty_text<'a>(text: &'a str, label: &str) -> Result<&'a str, NixIndexError> {
    if text.is_empty()
        || text
            .chars()
            .any(|character| character.is_control() || character == ':')
    {
        Err(NixIndexError::invalid(format!("{label} is malformed")))
    } else {
        Ok(text)
    }
}

fn parse_endpoint(endpoint: &str) -> Result<String, NixIndexError> {
    validate_url(endpoint)?;
    if endpoint.starts_with("http://") {
        let authority = endpoint
            .strip_prefix("http://")
            .and_then(|value| value.split('/').next())
            .unwrap_or_default();
        if !is_loopback_host(authority) {
            return Err(NixIndexError::invalid(
                "plain HTTP nix index endpoints must use loopback",
            ));
        }
    }
    Ok(endpoint.trim_end_matches('/').to_string())
}

fn is_loopback_host(authority: &str) -> bool {
    let host = authority
        .strip_prefix('[')
        .and_then(|value| value.split(']').next())
        .unwrap_or_else(|| authority.split(':').next().unwrap_or(authority));
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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
                ALPHABET[((second & 0x0f) << 2 | chunk.get(2).copied().unwrap_or(0) >> 6) as usize]
                    as char,
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

// ponytail: raw Zstandard blocks preserve the required deterministic frame
// and avoid a new jetpack dependency; replace with #2156's native level-19
// one-thread seam when that handoff lands.
fn zstd_encode(bytes: &[u8]) -> Vec<u8> {
    const BLOCK: usize = 128 * 1024;
    let mut output = Vec::with_capacity(bytes.len() + bytes.len() / BLOCK * 3 + 32);
    output.extend_from_slice(&0xfd2f_b528u32.to_le_bytes());
    let descriptor = if bytes.len() <= 255 {
        0x24
    } else if bytes.len() <= 65_791 {
        0x64
    } else {
        0xa4
    };
    output.push(descriptor);
    match descriptor {
        0x24 => output.push(bytes.len() as u8),
        0x64 => output.extend_from_slice(&((bytes.len() - 256) as u16).to_le_bytes()),
        _ => output.extend_from_slice(&(bytes.len() as u32).to_le_bytes()),
    }
    if bytes.is_empty() {
        output.extend_from_slice(&1u32.to_le_bytes()[..3]);
    } else {
        let mut blocks = bytes.chunks(BLOCK).peekable();
        while let Some(block) = blocks.next() {
            let header = ((block.len() as u32) << 3) | u32::from(blocks.peek().is_none());
            output.extend_from_slice(&header.to_le_bytes()[..3]);
            output.extend_from_slice(block);
        }
    }
    output.extend_from_slice(&(xxh64(bytes) as u32).to_le_bytes());
    output
}

fn zstd_decode_bounded(bytes: &[u8], limit: usize) -> Result<Vec<u8>, NixIndexError> {
    let mut input = 0usize;
    let mut output = Vec::new();
    while input < bytes.len() {
        if bytes.get(input..input + 4) != Some(&0xfd2f_b528u32.to_le_bytes()) {
            return Err(NixIndexError::invalid(
                "nix index target is not a Zstandard frame",
            ));
        }
        input += 4;
        let descriptor = *bytes
            .get(input)
            .ok_or_else(|| NixIndexError::invalid("truncated nix index Zstandard header"))?;
        input += 1;
        if descriptor & 0x1b != 0 || descriptor & 0x20 == 0 || descriptor & 0x04 == 0 {
            return Err(NixIndexError::invalid(
                "nix index Zstandard frame must be single-segment and checksummed",
            ));
        }
        let fcs_size = match descriptor >> 6 {
            0 => 1,
            1 => 2,
            2 => 4,
            _ => 8,
        };
        let expected = read_le(bytes, input, fcs_size)
            .ok_or_else(|| NixIndexError::invalid("truncated nix index Zstandard content size"))?;
        input += fcs_size;
        let expected = if fcs_size == 2 {
            expected + 256
        } else {
            expected
        };
        if expected > limit as u64 {
            return Err(NixIndexError::invalid(
                "nix index decoded bytes exceed bound",
            ));
        }
        let frame_start = output.len();
        loop {
            let header = read_le(bytes, input, 3)
                .ok_or_else(|| NixIndexError::invalid("truncated nix index Zstandard block"))?
                as u32;
            input += 3;
            let last = header & 1 != 0;
            let kind = (header >> 1) & 3;
            let size = (header >> 3) as usize;
            if kind > 1 || size > 128 * 1024 {
                return Err(NixIndexError::invalid(
                    "nix index uses an unsupported Zstandard block",
                ));
            }
            if output.len().saturating_add(size) > limit {
                return Err(NixIndexError::invalid(
                    "nix index decoded bytes exceed bound",
                ));
            }
            match kind {
                0 => {
                    let end = input
                        .checked_add(size)
                        .filter(|end| *end <= bytes.len())
                        .ok_or_else(|| {
                            NixIndexError::invalid("truncated nix index Zstandard block")
                        })?;
                    output.extend_from_slice(&bytes[input..end]);
                    input = end;
                }
                1 => {
                    let value = *bytes
                        .get(input)
                        .ok_or_else(|| NixIndexError::invalid("truncated nix index RLE block"))?;
                    input += 1;
                    output.resize(output.len() + size, value);
                }
                _ => unreachable!(),
            }
            if last {
                break;
            }
        }
        if output.len() - frame_start != expected as usize {
            return Err(NixIndexError::invalid(
                "nix index Zstandard content size disagrees",
            ));
        }
        let checksum = read_le(bytes, input, 4)
            .ok_or_else(|| NixIndexError::invalid("truncated nix index Zstandard checksum"))?;
        input += 4;
        if checksum as u32 != xxh64(&output[frame_start..]) as u32 {
            return Err(NixIndexError::invalid(
                "nix index Zstandard checksum failed",
            ));
        }
    }
    if output.is_empty() {
        return Err(NixIndexError::invalid(
            "nix index Zstandard stream is empty",
        ));
    }
    Ok(output)
}

fn read_le(bytes: &[u8], offset: usize, size: usize) -> Option<u64> {
    let bytes = bytes.get(offset..offset.checked_add(size)?)?;
    (size <= 8).then(|| {
        bytes.iter().enumerate().fold(0u64, |value, (shift, byte)| {
            value | (u64::from(*byte) << (shift * 8))
        })
    })
}

fn xxh64(data: &[u8]) -> u64 {
    const P1: u64 = 11_400_714_785_074_694_791;
    const P2: u64 = 14_029_467_366_897_019_727;
    const P3: u64 = 1_609_587_929_392_839_161;
    const P4: u64 = 9_650_029_242_287_828_579;
    const P5: u64 = 2_870_177_450_012_600_261;
    let round = |acc: u64, lane: u64| {
        acc.wrapping_add(lane.wrapping_mul(P2))
            .rotate_left(31)
            .wrapping_mul(P1)
    };
    let mut offset = 0usize;
    let mut hash = if data.len() >= 32 {
        let mut lanes = [P1.wrapping_add(P2), P2, 0, 0u64.wrapping_sub(P1)];
        while offset + 32 <= data.len() {
            for lane in &mut lanes {
                *lane = round(
                    *lane,
                    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()),
                );
                offset += 8;
            }
        }
        let mut hash = lanes[0]
            .rotate_left(1)
            .wrapping_add(lanes[1].rotate_left(7))
            .wrapping_add(lanes[2].rotate_left(12))
            .wrapping_add(lanes[3].rotate_left(18));
        for lane in lanes {
            hash ^= round(0, lane);
            hash = hash.wrapping_mul(P1).wrapping_add(P4);
        }
        hash
    } else {
        P5
    };
    hash = hash.wrapping_add(data.len() as u64);
    while offset + 8 <= data.len() {
        let lane = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
        hash ^= round(0, lane);
        hash = hash.rotate_left(27).wrapping_mul(P1).wrapping_add(P4);
        offset += 8;
    }
    if offset + 4 <= data.len() {
        hash ^= u64::from(u32::from_le_bytes(
            data[offset..offset + 4].try_into().unwrap(),
        )) * P1;
        hash = hash.rotate_left(23).wrapping_mul(P2).wrapping_add(P3);
        offset += 4;
    }
    while offset < data.len() {
        hash ^= u64::from(data[offset]) * P5;
        hash = hash.rotate_left(11).wrapping_mul(P1);
        offset += 1;
    }
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(P2);
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(P3);
    hash ^ (hash >> 32)
}

fn generated_document(
    channel: String,
    revision: String,
    system: String,
    released_unix: u64,
    mut records: Vec<IndexRecord>,
    mut not_indexed: Vec<CoverageMiss>,
) -> Result<IndexDocument, NixIndexError> {
    records.sort_by(|left, right| compare_attrpath(&left.attrpath, &right.attrpath));
    not_indexed.sort_by(|left, right| {
        compare_attrpath(&left.attrpath, &right.attrpath)
            .then_with(|| left.reason.as_bytes().cmp(right.reason.as_bytes()))
    });
    let document = IndexDocument {
        schema: INDEX_SCHEMA,
        channel,
        revision,
        system,
        released_unix,
        coverage: Coverage {
            indexed: records
                .iter()
                .map(|record| record.attrpath.clone())
                .collect(),
            not_indexed,
        },
        records: records.iter().map(wire_from_record).collect(),
    };
    validate_document(&document)?;
    Ok(document)
}

fn parse_oracle(bytes: &[u8], system: &str) -> Result<Vec<OracleRecord>, NixIndexError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| NixIndexError::invalid("oracle output is not UTF-8"))?;
    let value = parse_json_exact_numbers(text, true)
        .map_err(|error| NixIndexError::invalid(format!("parse oracle JSON: {}", error.message)))?;
    let records_value = match &value {
        Value::Array(_) => value,
        Value::Object(_) => {
            let map = object(value.clone(), "oracle")?;
            reject_unknown(&map, &["system", "records"])?;
            if let Some((_, value)) = map.iter().find(|(key, _)| key == "system") {
                if string_value(value, "oracle system")? != system {
                    return Err(NixIndexError::invalid("oracle has the wrong system"));
                }
            }
            field(&map, "records")?.clone()
        }
        _ => {
            return Err(NixIndexError::invalid(
                "oracle output must be an array or object",
            ))
        }
    };
    let records = value_array(&records_value, "oracle records")?;
    records
        .iter()
        .map(|value| {
            let map = object(value.clone(), "oracle record")?;
            reject_unknown(
                &map,
                &[
                    "system", "attrpath", "version", "drvPath", "outputs", "cache",
                ],
            )?;
            let record_system = match map.iter().find(|(key, _)| key == "system") {
                Some((_, value)) => string_value(value, "oracle record system")?,
                None => system,
            };
            if record_system != system {
                return Err(NixIndexError::invalid("oracle record has the wrong system"));
            }
            let outputs = array_field(&map, "outputs")?
                .iter()
                .map(parse_output)
                .collect::<Result<Vec<_>, _>>()?;
            let mut output_names = BTreeSet::new();
            let mut output_paths = BTreeSet::new();
            for output in &outputs {
                if !output_names.insert(output.name.clone()) {
                    return Err(NixIndexError::invalid(
                        "oracle record repeats an output name",
                    ));
                }
                if !output_paths.insert(output.store_path.clone()) {
                    return Err(NixIndexError::invalid(
                        "oracle record repeats an output path",
                    ));
                }
            }
            let record = IndexRecord {
                attrpath: parse_attrpath(field(&map, "attrpath")?)?,
                version: string_field(&map, "version")?.to_string(),
                drv_path: string_field(&map, "drvPath")?.to_string(),
                outputs: outputs
                    .into_iter()
                    .map(|output| (output.name, output.store_path))
                    .collect(),
            };
            validate_oracle_record(&record)?;
            Ok(OracleRecord {
                record,
                cache_admitted: match map
                    .iter()
                    .find(|(key, _)| key == "cache")
                    .map(|(_, value)| value)
                {
                    None => false,
                    Some(Value::Bool(value)) => *value,
                    Some(_) => {
                        return Err(NixIndexError::invalid("oracle cache field must be boolean"))
                    }
                },
            })
        })
        .collect()
}

fn validate_oracle_record(record: &IndexRecord) -> Result<(), NixIndexError> {
    validate_attrpath(&record.attrpath)?;
    if record.version.chars().any(|character| character.is_control()) {
        return Err(NixIndexError::invalid(
            "oracle record version contains a control character",
        ));
    }
    validate_store_path(&record.drv_path, true)?;
    if record.outputs.is_empty() {
        return Err(NixIndexError::invalid("oracle record has no outputs"));
    }
    let mut output_paths = BTreeSet::new();
    for (name, path) in &record.outputs {
        if name.is_empty() {
            return Err(NixIndexError::invalid("oracle record has an empty output name"));
        }
        validate_store_path(path, false)?;
        if !output_paths.insert(path) {
            return Err(NixIndexError::invalid(
                "oracle record repeats an output path",
            ));
        }
    }
    Ok(())
}

pub(crate) fn producer_signature_request(index_bytes: &[u8]) -> Vec<u8> {
    signature_message(INDEX_DOMAIN, index_bytes)
}

pub(crate) fn manifest_signature_request(manifest_bytes: &[u8]) -> Vec<u8> {
    signature_message(MANIFEST_DOMAIN, manifest_bytes)
}

pub(crate) fn canonical_test_index(
    channel: &str,
    revision: &str,
    system: &str,
    released_unix: u64,
    records: Vec<IndexRecord>,
    not_indexed: Vec<(Vec<String>, String)>,
) -> Result<(Vec<u8>, Vec<u8>), NixIndexError> {
    let misses = not_indexed
        .into_iter()
        .map(|(attrpath, reason)| CoverageMiss { attrpath, reason })
        .collect();
    let document = generated_document(
        channel.to_string(),
        revision.to_string(),
        system.to_string(),
        released_unix,
        records,
        misses,
    )?;
    let bytes = canonical_index_bytes(&document)?;
    Ok((bytes.clone(), zstd_encode(&bytes)))
}

pub(crate) fn canonical_manifest_for_test(
    channel: &str,
    generation: u64,
    issued_unix: u64,
    expires_unix: u64,
    targets: Vec<ManifestTarget>,
) -> Result<Vec<u8>, NixIndexError> {
    canonical_manifest_bytes(&ChannelManifest {
        schema: INDEX_SCHEMA,
        channel: channel.to_string(),
        generation,
        issued_unix,
        expires_unix,
        targets,
    })
}

pub(crate) fn signature_sidecar_for_test(key_id: &str, signature: &[u8]) -> Vec<u8> {
    canonical_signature_bytes(&IndexSignature {
        schema: INDEX_SCHEMA,
        key_id: key_id.to_string(),
        algorithm: "ed25519".to_string(),
        signature: base64_encode(signature),
    })
}

pub(crate) fn index_target_for_test(
    revision: &str,
    system: &str,
    endpoint: &str,
    compressed: &[u8],
    decoded: &[u8],
    signature_bytes: &[u8],
    record_count: u64,
) -> Result<ManifestTarget, NixIndexError> {
    validate_revision(revision)?;
    validate_system(system)?;
    let digest = sha256_hex(compressed);
    let url = format!("{endpoint}/index-v1/{revision}/{system}/{digest}.json.zst");
    Ok(ManifestTarget {
        revision: revision.to_string(),
        system: system.to_string(),
        url: url.clone(),
        signature_url: format!("{url}.sig.json"),
        sha256: digest,
        compressed_length: compressed.len() as u64,
        decoded_length: decoded.len() as u64,
        record_count,
        index_signature_sha256: sha256_hex(signature_bytes),
        discoverable: true,
    })
}

pub(crate) fn producer_generate(
    channel: &str,
    system: &str,
    revision: &str,
    released_unix: u64,
    oracle: &[OracleRecord],
    store_paths: &BTreeSet<String>,
) -> Result<(Vec<u8>, Vec<u8>, String), NixIndexError> {
    producer_generate_with_hydra_paths(
        channel,
        system,
        revision,
        released_unix,
        oracle,
        store_paths,
        &BTreeSet::new(),
    )
}

pub(crate) fn producer_generate_with_hydra_paths(
    channel: &str,
    system: &str,
    revision: &str,
    released_unix: u64,
    oracle: &[OracleRecord],
    store_paths: &BTreeSet<String>,
    hydra_output_paths: &BTreeSet<String>,
) -> Result<(Vec<u8>, Vec<u8>, String), NixIndexError> {
    validate_channel(channel)?;
    validate_system(system)?;
    validate_revision(revision)?;
    let mut records = Vec::new();
    let mut not_indexed = Vec::new();
    for candidate in oracle {
        let missing_path = candidate
            .record
            .outputs
            .values()
            .find(|path| !store_paths.contains(*path));
        let hydra_missing_path = if hydra_output_paths.is_empty() {
            None
        } else {
            candidate
                .record
                .outputs
                .values()
                .find(|path| !hydra_output_paths.contains(*path))
        };
        let reason = if missing_path.is_some() || hydra_missing_path.is_some() {
            Some("no-channel-build")
        } else if !candidate.cache_admitted {
            Some("missing-narinfo")
        } else if candidate.record.version.is_empty() {
            Some("missing-version")
        } else if !candidate.record.outputs.contains_key("out")
            && !candidate.record.outputs.contains_key("bin")
        {
            Some("missing-primary-output")
        } else {
            None
        };
        if let Some(reason) = reason {
            not_indexed.push(CoverageMiss {
                attrpath: candidate.record.attrpath.clone(),
                reason: reason.to_string(),
            });
        } else {
            records.push(candidate.record.clone());
        }
    }
    let document = generated_document(
        channel.to_string(),
        revision.to_string(),
        system.to_string(),
        released_unix,
        records,
        not_indexed,
    )?;
    let decoded = canonical_index_bytes(&document)?;
    let compressed = zstd_encode(&decoded);
    if compressed.len() > MAX_COMPRESSED_BYTES || decoded.len() > MAX_DECODED_BYTES {
        return Err(NixIndexError::invalid(
            "generated nix index exceeds format bounds",
        ));
    }
    let report = format_generation_report(&decoded, &compressed, &document);
    Ok((decoded, compressed, report))
}

fn format_generation_report(decoded: &[u8], compressed: &[u8], document: &IndexDocument) -> String {
    let output_count: usize = document
        .records
        .iter()
        .map(|record| record.outputs.len())
        .sum();
    let indexed = document.coverage.indexed.len();
    let not_indexed = document.coverage.not_indexed.len();
    format!(
        "{{\"schema\":1,\"channel\":\"{}\",\"revision\":\"{}\",\"system\":\"{}\",\"compressed_bytes\":{},\"decoded_bytes\":{},\"record_count\":{},\"output_count\":{},\"indexed_count\":{},\"not_indexed_count\":{}}}",
        json_escape(&document.channel),
        json_escape(&document.revision),
        json_escape(&document.system),
        compressed.len(),
        decoded.len(),
        document.records.len(),
        output_count,
        indexed,
        not_indexed
    )
}

pub(crate) fn parse_oracle_for_producer(
    bytes: &[u8],
    system: &str,
) -> Result<Vec<OracleRecord>, NixIndexError> {
    parse_oracle(bytes, system)
}

pub(crate) fn producer_coverage_report(decoded: &[u8]) -> Result<String, NixIndexError> {
    let document = parse_index_strict(decoded)?;
    let mut output = String::new();
    output.push_str("{\"schema\":1,\"channel\":\"");
    output.push_str(&json_escape(&document.channel));
    output.push_str("\",\"revision\":\"");
    output.push_str(&json_escape(&document.revision));
    output.push_str("\",\"system\":\"");
    output.push_str(&json_escape(&document.system));
    output.push_str("\",\"indexed\":[");
    for (index, attrpath) in document.coverage.indexed.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        encode_attrpath(&mut output, attrpath);
    }
    output.push_str("],\"notIndexed\":[");
    for (index, miss) in document.coverage.not_indexed.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str("{\"attrpath\":");
        encode_attrpath(&mut output, &miss.attrpath);
        output.push_str(",\"reason\":\"");
        output.push_str(&json_escape(&miss.reason));
        output.push_str("\"}");
    }
    output.push_str("],\"scope\":\"off-device packages-info.nix inventory only; overlays, overrides, user Nix config, custom flakes, custom package sets, local inputs, and impure evaluation are outside the denominator\"}");
    Ok(output)
}

pub(crate) fn decode_index_records_for_producer(
    bytes: &[u8],
) -> Result<Vec<IndexRecord>, NixIndexError> {
    let decoded = if bytes.starts_with(&0xfd2f_b528u32.to_le_bytes()) {
        zstd_decode_bounded(bytes, MAX_DECODED_BYTES)?
    } else {
        bytes.to_vec()
    };
    Ok(parse_index_strict(&decoded)?
        .records
        .iter()
        .map(record_from_wire)
        .collect())
}

pub(crate) fn producer_target_measurements(bytes: &[u8]) -> Result<(u64, u64), NixIndexError> {
    let decoded = if bytes.starts_with(&0xfd2f_b528u32.to_le_bytes()) {
        zstd_decode_bounded(bytes, MAX_DECODED_BYTES)?
    } else {
        bytes.to_vec()
    };
    let document = parse_index_strict(&decoded)?;
    Ok((decoded.len() as u64, document.records.len() as u64))
}

pub(crate) fn producer_manifest_bytes(
    channel: &str,
    generation: u64,
    issued_unix: u64,
    expires_unix: u64,
    targets: Vec<(
        String,
        String,
        String,
        String,
        String,
        u64,
        u64,
        u64,
        String,
        bool,
    )>,
) -> Result<Vec<u8>, NixIndexError> {
    let targets = targets
        .into_iter()
        .map(
            |(
                revision,
                system,
                url,
                signature_url,
                sha256,
                compressed_length,
                decoded_length,
                record_count,
                index_signature_sha256,
                discoverable,
            )| ManifestTarget {
                revision,
                system,
                url,
                signature_url,
                sha256,
                compressed_length,
                decoded_length,
                record_count,
                index_signature_sha256,
                discoverable,
            },
        )
        .collect();
    canonical_manifest_bytes(&ChannelManifest {
        schema: INDEX_SCHEMA,
        channel: channel.to_string(),
        generation,
        issued_unix,
        expires_unix,
        targets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::sync::Mutex;

    const REVISION: &str = "c8f90650c15282fa8656a041bfbbd2403997a9a7";
    const RIPGREP_DRV: &str = "/nix/store/har68bdn10m05zxkvn680q37bdq0bpdx-ripgrep-15.2.0.drv";
    const RIPGREP_OUT: &str = "/nix/store/axp6zlky4x2v3jwcbq24a2cz25hzlw9b-ripgrep-15.2.0";
    const AUX_DRV: &str = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-a-1.drv";
    const AUX_OUT: &str = "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-a-1";

    struct FixedClock(u64);
    impl IndexClock for FixedClock {
        fn now_unix(&self) -> u64 {
            self.0
        }
    }

    struct MutableClock(AtomicU64);
    impl IndexClock for MutableClock {
        fn now_unix(&self) -> u64 {
            self.0.load(Ordering::Relaxed)
        }
    }

    struct MapTransport {
        values: Mutex<BTreeMap<String, Vec<u8>>>,
    }
    impl IndexTransport for MapTransport {
        fn get_bounded(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, NixIndexError> {
            let value = self
                .values
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| NixIndexError::Transport(format!("missing test URL {url}")))?;
            if value.len() as u64 > max_bytes {
                return Err(NixIndexError::invalid("test response exceeded bound"));
            }
            Ok(value)
        }
    }

    fn ripgrep_record() -> IndexRecord {
        IndexRecord {
            attrpath: vec!["ripgrep".to_string()],
            version: "15.2.0".to_string(),
            drv_path: RIPGREP_DRV.to_string(),
            outputs: [("out".to_string(), RIPGREP_OUT.to_string())]
                .into_iter()
                .collect(),
        }
    }

    fn auxiliary_record() -> IndexRecord {
        IndexRecord {
            attrpath: vec!["a".to_string(), "nested".to_string()],
            version: "1.0.0".to_string(),
            drv_path: AUX_DRV.to_string(),
            outputs: [("out".to_string(), AUX_OUT.to_string())]
                .into_iter()
                .collect(),
        }
    }

    fn signed_fixture() -> (PathBuf, MapTransport, FixedClock, SigningKey, IndexKey) {
        let root = std::env::temp_dir().join(format!("jet-nix-index-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let key = SigningKey::from_bytes(&[7; 32]);
        let (decoded, compressed) = canonical_test_index(
            "nixpkgs-unstable",
            REVISION,
            "x86_64-linux",
            1_724_000_000,
            vec![ripgrep_record()],
            Vec::new(),
        )
        .unwrap();
        let index_signature = key.sign(&producer_signature_request(&decoded));
        let sidecar = signature_sidecar_for_test("test-key", &index_signature.to_bytes());
        let endpoint = "http://127.0.0.1:9999";
        let target = index_target_for_test(
            REVISION,
            "x86_64-linux",
            endpoint,
            &compressed,
            &decoded,
            &sidecar,
            1,
        )
        .unwrap();
        let manifest = canonical_manifest_for_test(
            "nixpkgs-unstable",
            1,
            1_724_000_000,
            1_724_000_000 + 604_800,
            vec![target.clone()],
        )
        .unwrap();
        let manifest_signature = key.sign(&manifest_signature_request(&manifest));
        let manifest_sidecar =
            signature_sidecar_for_test("test-key", &manifest_signature.to_bytes());
        let target_url = target.url.clone();
        let signature_url = target.signature_url.clone();
        let manifest_url = format!("{endpoint}/v1/nixpkgs-unstable/manifest.json");
        let transport = MapTransport {
            values: Mutex::new(
                [
                    (manifest_url, manifest),
                    (
                        format!("{endpoint}/v1/nixpkgs-unstable/manifest.json.sig.json"),
                        manifest_sidecar,
                    ),
                    (target_url, compressed),
                    (signature_url, sidecar),
                ]
                .into_iter()
                .collect(),
            ),
        };
        (
            root,
            transport,
            FixedClock(1_724_000_100),
            key,
            IndexKey {
                channel: "nixpkgs-unstable".to_string(),
                revision: REVISION.to_string(),
                system: "x86_64-linux".to_string(),
                attrpath: vec!["ripgrep".to_string()],
            },
        )
    }

    #[test]
    fn nix_index_resolves_exact_ripgrep_record() {
        let (root, transport, clock, key, index_key) = signed_fixture();
        let client = NixIndexClient::for_test(
            root.clone(),
            "http://127.0.0.1:9999".to_string(),
            "test-key".to_string(),
            key.verifying_key().to_bytes(),
            &transport,
            &clock,
            false,
        )
        .unwrap();
        let resolved = client.resolve(&index_key).unwrap();
        assert_eq!(resolved.record, ripgrep_record());
        assert_eq!(resolved.proof.revision, REVISION);
        assert_eq!(resolved.proof.system, "x86_64-linux");
        assert_eq!(resolved.proof.manifest_generation, 1);
        assert_eq!(resolved.proof.jet_key_id, "test-key");
        assert!(root.join(INDEX_ROOT).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn nix_index_reordered_input_is_byte_identical() {
        let first_records = vec![ripgrep_record(), auxiliary_record()];
        let mut second_records = first_records.clone();
        second_records.reverse();
        let first = canonical_test_index(
            "nixpkgs-unstable",
            REVISION,
            "x86_64-linux",
            1,
            first_records,
            Vec::new(),
        )
        .unwrap();
        let second = canonical_test_index(
            "nixpkgs-unstable",
            REVISION,
            "x86_64-linux",
            1,
            second_records,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(first.0, second.0);
        assert_eq!(first.1, second.1);
        assert_eq!(sha256_hex(&first.1), sha256_hex(&second.1));
    }

    #[test]
    fn nix_index_coverage_report_partitions_indexed_and_not_indexed_attrs() {
        let excluded = auxiliary_record();
        let oracle = vec![
            OracleRecord {
                record: ripgrep_record(),
                cache_admitted: true,
            },
            OracleRecord {
                record: excluded.clone(),
                cache_admitted: false,
            },
        ];
        let store_paths = [RIPGREP_OUT.to_string(), AUX_OUT.to_string()]
            .into_iter()
            .collect();
        let (decoded, _, _) = producer_generate(
            "nixpkgs-unstable",
            "x86_64-linux",
            REVISION,
            1,
            &oracle,
            &store_paths,
        )
        .unwrap();
        let report = producer_coverage_report(&decoded).unwrap();
        let parsed = parse_json_exact_numbers(&report, true).unwrap();
        assert!(report.contains("\"system\":\"x86_64-linux\""));
        assert!(report.contains("\"indexed\":[[\"ripgrep\"]]"));
        assert!(report.contains("\"notIndexed\":[{"));
        assert!(report.contains("missing-narinfo"));
        assert!(matches!(parsed, Value::Object(_)));
    }

    #[test]
    fn nix_index_rejects_forged_cross_system_duplicate_and_ambiguous_input() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let (decoded, _) = canonical_test_index(
            "nixpkgs-unstable",
            REVISION,
            "x86_64-linux",
            1,
            vec![ripgrep_record()],
            Vec::new(),
        )
        .unwrap();
        let forged = {
            let mut changed = decoded.clone();
            let position = changed.len() - 2;
            changed[position] ^= 1;
            changed
        };
        let signature = IndexSignature {
            schema: 1,
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
            signature: base64_encode(&key.sign(&producer_signature_request(&decoded)).to_bytes()),
        };
        assert!(
            verify_index_signature(&key.verifying_key(), "test-key", &signature, &forged).is_err()
        );
        let duplicate = br#"{"schema":1,"schema":1}"#;
        assert!(parse_index_strict(duplicate).is_err());
        let wrong_system = decoded
            .windows(b"x86_64-linux".len())
            .position(|window| window == b"x86_64-linux")
            .map(|position| {
                let mut bytes = decoded.clone();
                bytes.splice(
                    position..position + b"x86_64-linux".len(),
                    b"aarch64-linux".iter().copied(),
                );
                bytes
            });
        assert!(wrong_system.is_some());
        assert!(validate_store_path("/nix/store/not-a-store-path", false).is_err());

        let (root, transport, clock, key, mut cross_system_key) = signed_fixture();
        cross_system_key.system = "aarch64-linux".to_string();
        let client = NixIndexClient::for_test(
            root.clone(),
            "http://127.0.0.1:9999".to_string(),
            "test-key".to_string(),
            key.verifying_key().to_bytes(),
            &transport,
            &clock,
            false,
        )
        .unwrap();
        assert!(matches!(
            client.resolve(&cross_system_key),
            Err(NixIndexError::Invalid(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn nix_index_whole_refresh_retains_targets_and_rejects_rollback() {
        let (root, transport, clock, key, index_key) = signed_fixture();
        let client = NixIndexClient::for_test(
            root.clone(),
            "http://127.0.0.1:9999".to_string(),
            "test-key".to_string(),
            key.verifying_key().to_bytes(),
            &transport,
            &clock,
            false,
        )
        .unwrap();
        client.resolve(&index_key).unwrap();
        let state = client.load_highest_generation("nixpkgs-unstable").unwrap();
        assert_eq!(state.map(|(generation, _)| generation), Some(1));
        let target_files = fs::read_dir(
            root.join(INDEX_ROOT)
                .join("targets")
                .join(REVISION)
                .join("x86_64-linux"),
        )
        .unwrap()
        .count();
        assert_eq!(target_files, 2);

        assert!(client
            .check_generation(
            "nixpkgs-unstable",
            &ChannelManifest {
                schema: INDEX_SCHEMA,
                channel: "nixpkgs-unstable".to_string(),
                generation: 0,
                issued_unix: 1_723_000_000,
                expires_unix: 1_723_604_800,
                targets: Vec::new(),
            },
            b"stale",
        )
        .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn nix_index_offline_rejects_expired_cached_manifest() {
        let (root, transport, _, key, index_key) = signed_fixture();
        let clock = MutableClock(AtomicU64::new(1_724_000_100));
        let client = NixIndexClient::for_test(
            root.clone(),
            "http://127.0.0.1:9999".to_string(),
            "test-key".to_string(),
            key.verifying_key().to_bytes(),
            &transport,
            &clock,
            false,
        )
        .unwrap();
        client.resolve(&index_key).unwrap();
        drop(client);
        clock.0.store(1_724_604_800, Ordering::Relaxed);
        let offline = NixIndexClient::for_test(
            root.clone(),
            "http://127.0.0.1:9999".to_string(),
            "test-key".to_string(),
            key.verifying_key().to_bytes(),
            &transport,
            &clock,
            true,
        )
        .unwrap();
        let error = offline.resolve(&index_key).unwrap_err();
        assert_eq!(error.code(), 1276);
        assert!(matches!(error, NixIndexError::Offline(_)));
        let _ = fs::remove_dir_all(root);
    }
}
