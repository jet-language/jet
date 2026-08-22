//! Canonical NAR codec and local binary-cache admission.
//!
//! NAR is the portable wire representation used by Nix-compatible binary
//! caches.  This module owns the bytes and trust boundary only: callers still
//! decide which store identity and action produced an object.  The codec is
//! deterministic, bounded, rejects traversal and duplicate names, and stages
//! substitutions before publication.

use crate::TrustRoot::{Signature as TrustSignature, TrustKey};
use crate::SHA256;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const NAR_MAGIC: &str = "nix-archive-1";
pub const MAX_NAR_BYTES: usize = 1024 * 1024 * 1024;
const MAX_NODE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_NODES: usize = 1_000_000;
const MAX_DEPTH: usize = 256;
const MAX_NAME_BYTES: usize = 4096;
const MAX_INFO_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarStats {
    pub digest: String,
    pub bytes: u64,
    pub nodes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarSignature {
    pub key_id: String,
    pub algorithm: String,
    pub sig_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarInfo {
    pub store_path: String,
    pub url: String,
    pub compression: String,
    pub file_size: u64,
    pub nar_size: u64,
    pub nar_hash: String,
    pub references: Vec<String>,
    pub deriver: Option<String>,
    /// Optional Nix content-addressing descriptor. It is signed with the
    /// remaining narinfo fields and retained across endpoint round-trips.
    pub ca: Option<String>,
    pub signatures: Vec<NarSignature>,
}

/// Serialize a store tree into canonical NAR bytes.
pub fn write_nar(root: &Path) -> io::Result<(Vec<u8>, NarStats)> {
    let metadata = fs::symlink_metadata(root)?;
    if metadata.file_type().is_symlink() {
        return Err(invalid("NAR source root cannot be a symlink"));
    }
    let root = root
        .canonicalize()
        .map_err(|error| invalid(&format!("cannot resolve NAR source: {error}")))?;
    let mut output = Vec::new();
    put_string(&mut output, NAR_MAGIC)?;
    put_string(&mut output, "(")?;
    let mut state = EncodeState::default();
    encode_node(&root, &root, &mut output, &mut state, 0)?;
    put_string(&mut output, ")")?;
    let stats = NarStats {
        digest: digest_for(&output),
        bytes: output.len() as u64,
        nodes: state.nodes,
    };
    Ok((output, stats))
}

/// Serialize a tree and write it atomically to a regular file.
pub fn write_nar_file(root: &Path, destination: &Path) -> io::Result<NarStats> {
    let (bytes, stats) = write_nar(root)?;
    write_atomic(destination, &bytes)?;
    Ok(stats)
}

/// Read a NAR, fully validate it, and publish the decoded tree at `destination`.
/// The destination must not already exist.  A caller that wants to reuse an
/// object must compare its independently computed digest before calling this.
pub fn read_nar(bytes: &[u8], destination: &Path) -> io::Result<NarStats> {
    let (node, stats) = decode_nar(bytes)?;
    publish_decoded_node(&node, destination)?;
    Ok(stats)
}

/// Validate a NAR byte stream without materializing it. Cache lookup uses this
/// before it treats a signed narinfo as a usable hit.
pub fn validate_nar(bytes: &[u8]) -> io::Result<NarStats> {
    decode_nar(bytes).map(|(_, stats)| stats)
}

fn decode_nar(bytes: &[u8]) -> io::Result<(NarNode, NarStats)> {
    if bytes.len() > MAX_NAR_BYTES {
        return Err(invalid("NAR exceeds the 1 GiB limit"));
    }
    let mut reader = NarReader::new(bytes);
    if reader.string()? != NAR_MAGIC {
        return Err(invalid("NAR has an unknown magic"));
    }
    if reader.string()? != "(" {
        return Err(invalid("NAR root is not a node"));
    }
    let mut state = DecodeState::default();
    let node = decode_node(&mut reader, &mut state, 0, 0)?;
    if reader.string()? != ")" || reader.remaining() != 0 {
        return Err(invalid("NAR has trailing or unbalanced data"));
    }
    let stats = NarStats {
        digest: digest_for(bytes),
        bytes: bytes.len() as u64,
        nodes: state.nodes,
    };
    Ok((node, stats))
}

/// Read and stage a NAR file before atomically publishing its decoded tree.
pub fn read_nar_file(source: &Path, destination: &Path) -> io::Result<NarStats> {
    let bytes = read_bounded(source, MAX_NAR_BYTES)?;
    read_nar(&bytes, destination)
}

/// Compute the canonical digest of a NAR byte stream.
pub fn nar_digest(bytes: &[u8]) -> String {
    digest_for(bytes)
}

impl NarInfo {
    pub fn validate(&self) -> io::Result<()> {
        validate_store_path(&self.store_path)?;
        validate_relative_url(&self.url)?;
        if self.compression != "none" {
            return Err(invalid(
                "only uncompressed NARs are supported by this cache",
            ));
        }
        validate_digest(&self.nar_hash)?;
        if self.file_size > MAX_NAR_BYTES as u64 || self.nar_size > MAX_NAR_BYTES as u64 {
            return Err(invalid("narinfo size exceeds the NAR limit"));
        }
        if self.file_size != self.nar_size {
            return Err(invalid("uncompressed narinfo file and NAR sizes disagree"));
        }
        let mut references = BTreeSet::new();
        for reference in &self.references {
            validate_store_reference(reference)?;
            if !references.insert(reference) {
                return Err(invalid("narinfo contains duplicate references"));
            }
        }
        if let Some(deriver) = &self.deriver {
            if !deriver.is_empty() && deriver != "unknown-deriver" {
                validate_store_reference(deriver)?;
            }
        }
        if let Some(ca) = &self.ca {
            if ca.is_empty()
                || ca.len() > MAX_NAME_BYTES * 4
                || ca.bytes().any(|byte| byte == 0 || byte.is_ascii_control())
            {
                return Err(invalid("narinfo CA field is invalid"));
            }
        }
        let mut signatures = BTreeSet::new();
        for signature in &self.signatures {
            if signature.key_id.is_empty()
                || signature.algorithm.is_empty()
                || signature.sig_hex.is_empty()
                || signature.algorithm != crate::TrustRoot::ALG_HMAC_SHA256
                || signature.sig_hex.len() != 64
                || !signature
                    .sig_hex
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || !signatures.insert(signature.key_id.clone())
            {
                return Err(invalid("narinfo has an invalid or duplicate signature"));
            }
        }
        Ok(())
    }

    pub fn unsigned_text(&self) -> io::Result<String> {
        self.validate()?;
        let mut references = self.references.clone();
        references.sort();
        let mut out = String::new();
        line(&mut out, "StorePath", &self.store_path)?;
        line(&mut out, "URL", &self.url)?;
        line(&mut out, "Compression", &self.compression)?;
        line(&mut out, "FileHash", &self.nar_hash)?;
        line(&mut out, "FileSize", &self.file_size.to_string())?;
        line(&mut out, "NarHash", &self.nar_hash)?;
        line(&mut out, "NarSize", &self.nar_size.to_string())?;
        line(&mut out, "References", &references.join(" "))?;
        if let Some(deriver) = &self.deriver {
            if !deriver.is_empty() {
                line(&mut out, "Deriver", deriver)?;
            }
        }
        if let Some(ca) = &self.ca {
            line(&mut out, "CA", ca)?;
        }
        Ok(out)
    }

    pub fn to_text(&self) -> io::Result<String> {
        let mut out = self.unsigned_text()?;
        let mut signatures = self.signatures.clone();
        signatures.sort_by(|a, b| a.key_id.cmp(&b.key_id));
        for signature in signatures {
            let value = format!(
                "{}:{}:{}",
                signature.key_id, signature.algorithm, signature.sig_hex
            );
            line(&mut out, "Sig", &value)?;
        }
        Ok(out)
    }

    pub fn parse(text: &str) -> io::Result<Self> {
        if text.len() > MAX_INFO_BYTES {
            return Err(invalid("narinfo is too large"));
        }
        let mut info = NarInfo {
            store_path: String::new(),
            url: String::new(),
            compression: String::new(),
            file_size: 0,
            nar_size: 0,
            nar_hash: String::new(),
            references: Vec::new(),
            deriver: None,
            ca: None,
            signatures: Vec::new(),
        };
        let mut file_hash = None;
        let mut seen = BTreeSet::new();
        for raw in text.lines() {
            if raw.is_empty() {
                continue;
            }
            let (key, value) = raw
                .split_once(':')
                .ok_or_else(|| invalid("narinfo contains a malformed line"))?;
            if value.starts_with(' ') {
                // Canonical narinfo uses exactly one separator space.
                let value = &value[1..];
                if !seen.insert(key.to_string()) && key != "Sig" {
                    return Err(invalid("narinfo contains duplicate fields"));
                }
                match key {
                    "StorePath" => info.store_path = value.to_string(),
                    "URL" => info.url = value.to_string(),
                    "Compression" => info.compression = value.to_string(),
                    "FileHash" => file_hash = Some(value.to_string()),
                    "FileSize" => info.file_size = parse_u64(value, "FileSize")?,
                    "NarHash" => info.nar_hash = value.to_string(),
                    "NarSize" => info.nar_size = parse_u64(value, "NarSize")?,
                    "References" => {
                        info.references = if value.is_empty() {
                            Vec::new()
                        } else {
                            value.split_whitespace().map(str::to_string).collect()
                        }
                    }
                    "Deriver" => info.deriver = Some(value.to_string()),
                    "CA" => info.ca = Some(value.to_string()),
                    "Sig" => info.signatures.push(parse_signature(value)?),
                    _ => return Err(invalid("narinfo contains an unknown field")),
                }
            } else {
                return Err(invalid("narinfo field is missing its separator"));
            }
        }
        info.validate()?;
        let file_hash = file_hash.ok_or_else(|| invalid("narinfo has no FileHash field"))?;
        if !same_digest(&file_hash, &info.nar_hash) {
            return Err(invalid("narinfo FileHash and NarHash disagree"));
        }
        Ok(info)
    }

    pub fn signed(self, key: &TrustKey) -> io::Result<Self> {
        let mut signed = self;
        signed.signatures.clear();
        let signature = key.sign(signed.unsigned_text()?.as_bytes());
        signed.signatures.push(NarSignature::from(signature));
        Ok(signed)
    }

    pub fn verify(&self, key: &TrustKey) -> io::Result<()> {
        self.validate()?;
        let expected = key.sign(self.unsigned_text()?.as_bytes());
        if !self.signatures.iter().any(|signature| {
            signature.key_id == expected.key_id
                && signature.algorithm == expected.algorithm
                && signature.sig_hex == expected.sig_hex
        }) {
            return Err(invalid("narinfo signature verification failed"));
        }
        Ok(())
    }
}

impl From<TrustSignature> for NarSignature {
    fn from(signature: TrustSignature) -> Self {
        Self {
            key_id: signature.key_id,
            algorithm: signature.algorithm,
            sig_hex: signature.sig_hex,
        }
    }
}

/// Publish a verified, signed NAR and narinfo into a local cache directory.
/// `info.url` is the relative NAR location; the narinfo is stored beside it
/// with a `.narinfo` suffix. Existing bytes must match exactly.
pub fn publish_local(
    cache: &Path,
    info: NarInfo,
    nar: &[u8],
    key: &TrustKey,
) -> io::Result<(PathBuf, PathBuf)> {
    let signed = info.signed(key)?;
    verify_nar_info_bytes(&signed, nar)?;
    let nar_path = cache.join(&signed.url);
    let info_path = cache.join(narinfo_name(&signed.store_path)?);
    write_or_match(&nar_path, nar)?;
    write_or_match(&info_path, signed.to_text()?.as_bytes())?;
    Ok((nar_path, info_path))
}

/// Substitute a local NAR only after narinfo trust and byte identity checks.
/// A corrupt or conflicting destination is never overwritten.
pub fn substitute_local(
    nar_path: &Path,
    info_path: &Path,
    destination: &Path,
    key: &TrustKey,
) -> io::Result<NarStats> {
    let info_text = String::from_utf8(read_bounded(info_path, MAX_INFO_BYTES)?)
        .map_err(|_| invalid("narinfo is not UTF-8"))?;
    let info = NarInfo::parse(&info_text)?;
    info.verify(key)?;
    let nar = read_bounded(nar_path, MAX_NAR_BYTES)?;
    verify_nar_info_bytes(&info, &nar)?;
    read_nar(&nar, destination)
}

fn verify_nar_info_bytes(info: &NarInfo, nar: &[u8]) -> io::Result<()> {
    if info.nar_size != nar.len() as u64 || !hash_matches(&info.nar_hash, nar) {
        return Err(invalid("NAR bytes do not match signed narinfo"));
    }
    Ok(())
}

#[derive(Default)]
struct EncodeState {
    nodes: usize,
}

fn encode_node(
    path: &Path,
    root: &Path,
    out: &mut Vec<u8>,
    state: &mut EncodeState,
    depth: usize,
) -> io::Result<()> {
    if depth > MAX_DEPTH {
        return Err(invalid("NAR tree is too deep"));
    }
    state.nodes = state
        .nodes
        .checked_add(1)
        .ok_or_else(|| invalid("NAR node count overflow"))?;
    if state.nodes > MAX_NODES {
        return Err(invalid("NAR contains too many nodes"));
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        put_string(out, "(")?;
        put_string(out, "type")?;
        put_string(out, "symlink")?;
        let target = fs::read_link(path)?;
        let target = target
            .to_str()
            .ok_or_else(|| invalid("NAR symlink target is not UTF-8"))?;
        validate_symlink_target(target)?;
        if target.starts_with('/') {
            validate_absolute_symlink_target(target)?;
        } else {
            validate_relative_symlink_target(path, root, target)?;
        }
        put_string(out, "target")?;
        put_string(out, target)?;
        put_string(out, ")")?;
    } else if metadata.is_dir() {
        put_string(out, "(")?;
        put_string(out, "type")?;
        put_string(out, "directory")?;
        let mut children = fs::read_dir(path)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|child| {
                let name = child
                    .file_name()
                    .to_str()
                    .ok_or_else(|| invalid("NAR filename is not UTF-8"))?
                    .to_string();
                validate_name(&name)?;
                Ok((name, child))
            })
            .collect::<io::Result<Vec<_>>>()?;
        children.sort_by(|(left, _), (right, _)| left.cmp(right));
        let mut names = BTreeSet::new();
        for (name, child) in children {
            if !names.insert(name.clone()) {
                return Err(invalid("NAR directory contains duplicate names"));
            }
            put_string(out, "entry")?;
            put_string(out, "(")?;
            put_string(out, "name")?;
            put_string(out, &name)?;
            put_string(out, "node")?;
            encode_node(&child.path(), root, out, state, depth + 1)?;
            put_string(out, ")")?;
        }
        put_string(out, ")")?;
    } else if metadata.is_file() {
        if metadata.len() > MAX_NODE_BYTES {
            return Err(invalid("NAR file exceeds the 512 MiB limit"));
        }
        let bytes = read_bounded(path, MAX_NODE_BYTES as usize)?;
        put_string(out, "(")?;
        put_string(out, "type")?;
        put_string(out, "regular")?;
        if executable(&metadata) {
            put_string(out, "executable")?;
            put_string(out, "")?;
        }
        put_string(out, "contents")?;
        put_bytes(out, &bytes)?;
        put_string(out, ")")?;
    } else {
        return Err(invalid("NAR cannot encode a special file"));
    }
    Ok(())
}

#[derive(Default)]
struct DecodeState {
    nodes: usize,
    bytes: u64,
}

#[derive(Debug)]
enum NarNode {
    Directory(Vec<(String, NarNode)>),
    Regular { executable: bool, bytes: Vec<u8> },
    Symlink(String),
}

fn decode_node(
    reader: &mut NarReader<'_>,
    state: &mut DecodeState,
    depth: usize,
    relative_depth: usize,
) -> io::Result<NarNode> {
    if depth > MAX_DEPTH {
        return Err(invalid("NAR tree is too deep"));
    }
    if reader.string()? != "(" || reader.string()? != "type" {
        return Err(invalid("NAR node has an invalid header"));
    }
    state.nodes += 1;
    if state.nodes > MAX_NODES {
        return Err(invalid("NAR contains too many nodes"));
    }
    let kind = reader.string()?;
    let node = match kind.as_str() {
        "directory" => {
            let mut entries = Vec::new();
            let mut names = BTreeSet::new();
            let mut previous_name = None;
            loop {
                match reader.string()?.as_str() {
                    ")" => break NarNode::Directory(entries),
                    "entry" => {
                        if reader.string()? != "(" || reader.string()? != "name" {
                            return Err(invalid("NAR directory entry is malformed"));
                        }
                        let name = reader.string()?;
                        validate_name(&name)?;
                        if !names.insert(name.clone()) {
                            return Err(invalid("NAR directory contains duplicate names"));
                        }
                        if previous_name
                            .as_deref()
                            .is_some_and(|previous| previous >= name.as_str())
                        {
                            return Err(invalid(
                                "NAR directory entries are not in canonical name order",
                            ));
                        }
                        previous_name = Some(name.clone());
                        if reader.string()? != "node" {
                            return Err(invalid("NAR directory entry has no node"));
                        }
                        let child = decode_node(reader, state, depth + 1, relative_depth + 1)?;
                        if reader.string()? != ")" {
                            return Err(invalid("NAR directory entry is unbalanced"));
                        }
                        entries.push((name, child));
                    }
                    _ => return Err(invalid("NAR directory has an unknown field")),
                }
            }
        }
        "regular" => {
            let mut field = reader.string()?;
            let executable = if field == "executable" {
                if reader.string()? != "" {
                    return Err(invalid("NAR regular executable marker is malformed"));
                }
                field = reader.string()?;
                true
            } else {
                false
            };
            if field != "contents" {
                return Err(invalid("NAR regular file has no contents"));
            }
            let bytes = reader.bytes()?;
            state.bytes = state
                .bytes
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| invalid("NAR byte count overflow"))?;
            if state.bytes > MAX_NAR_BYTES as u64 {
                return Err(invalid("NAR contents exceed the 1 GiB limit"));
            }
            if reader.string()? != ")" {
                return Err(invalid("NAR regular file is unbalanced"));
            }
            NarNode::Regular { executable, bytes }
        }
        "symlink" => {
            if reader.string()? != "target" {
                return Err(invalid("NAR symlink has no target"));
            }
            let target = reader.string()?;
            validate_symlink_target(&target)?;
            if target.starts_with('/') {
                validate_absolute_symlink_target(&target)?;
            } else {
                if relative_depth == 0 {
                    return Err(invalid("NAR root relative symlink has no safe parent"));
                }
                validate_relative_symlink_depth(&target, relative_depth - 1)?;
            }
            if reader.string()? != ")" {
                return Err(invalid("NAR symlink is unbalanced"));
            }
            NarNode::Symlink(target)
        }
        _ => return Err(invalid("NAR contains an unknown node type")),
    };
    Ok(node)
}

fn publish_decoded_node(node: &NarNode, destination: &Path) -> io::Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(invalid("NAR substitution destination already exists"));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("NAR substitution destination has no parent"))?;
    validate_destination_parent(parent)?;
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(
        ".nar-stage-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    if staging.exists() {
        remove_tree(&staging)?;
    }
    let result = (|| {
        write_decoded_node(node, &staging, &staging)?;
        fs::rename(&staging, destination)
    })();
    if result.is_err() {
        let _ = remove_tree(&staging);
    }
    result
}

fn write_decoded_node(node: &NarNode, path: &Path, root: &Path) -> io::Result<()> {
    match node {
        NarNode::Directory(entries) => {
            fs::create_dir(path)?;
            for (name, child) in entries {
                write_decoded_node(child, &path.join(name), root)?;
            }
        }
        NarNode::Regular { executable, bytes } => {
            write_atomic(path, bytes)?;
            set_mode(path, if *executable { 0o755 } else { 0o644 })?;
        }
        NarNode::Symlink(target) => create_symlink(target, path, root)?,
    }
    Ok(())
}

fn create_symlink(target: &str, path: &Path, root: &Path) -> io::Result<()> {
    if target.starts_with('/') {
        validate_absolute_symlink_target(target)?;
    } else {
        let parent = path.parent().unwrap_or(root);
        let candidate = parent.join(target);
        let relative = candidate
            .strip_prefix(root)
            .map_err(|_| invalid("NAR relative symlink escapes its output root"))?;
        let mut normalized = PathBuf::new();
        for component in relative.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(value) => normalized.push(value),
                Component::ParentDir => {
                    if !normalized.pop() {
                        return Err(invalid("NAR relative symlink escapes its output root"));
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(invalid("NAR relative symlink is not relative"));
                }
            }
        }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, path)
    }
    #[cfg(windows)]
    {
        if target.starts_with('/') {
            std::os::windows::fs::symlink_dir(target, path)
        } else {
            std::os::windows::fs::symlink_file(target, path)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, path);
        Err(invalid("this host cannot materialize NAR symlinks"))
    }
}

fn executable(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

fn put_string(out: &mut Vec<u8>, value: &str) -> io::Result<()> {
    put_bytes(out, value.as_bytes())
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_NODE_BYTES as usize {
        return Err(invalid("NAR field exceeds the 512 MiB limit"));
    }
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
    let padding = (8 - (bytes.len() % 8)) % 8;
    out.extend(std::iter::repeat(0).take(padding));
    if out.len() > MAX_NAR_BYTES {
        return Err(invalid("NAR exceeds the 1 GiB limit"));
    }
    Ok(())
}

struct NarReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> NarReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, length: usize) -> io::Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(length)
            .ok_or_else(|| invalid("NAR length overflows"))?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| invalid("NAR is truncated"))?;
        self.at = end;
        Ok(value)
    }

    fn string(&mut self) -> io::Result<String> {
        let bytes = self.bytes()?;
        String::from_utf8(bytes).map_err(|_| invalid("NAR string is not UTF-8"))
    }

    fn bytes(&mut self) -> io::Result<Vec<u8>> {
        let mut length = [0u8; 8];
        length.copy_from_slice(self.take(8)?);
        let length = u64::from_le_bytes(length);
        if length > MAX_NODE_BYTES {
            return Err(invalid("NAR field exceeds the 512 MiB limit"));
        }
        let length = usize::try_from(length).map_err(|_| invalid("NAR field is too large"))?;
        let value = self.take(length)?.to_vec();
        let padding = (8 - (length % 8)) % 8;
        let padding_bytes = self.take(padding)?;
        if padding_bytes.iter().any(|byte| *byte != 0) {
            return Err(invalid("NAR padding is not canonical"));
        }
        Ok(value)
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }
}

fn validate_name(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > MAX_NAME_BYTES
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(invalid("NAR name is not one safe path component"));
    }
    Ok(())
}

fn validate_symlink_target(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > MAX_NAME_BYTES * 4
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(invalid("NAR symlink target is invalid"));
    }
    Ok(())
}

fn validate_absolute_symlink_target(value: &str) -> io::Result<()> {
    if !value.starts_with("/nix/store/") {
        return Err(invalid("NAR absolute symlink must point into /nix/store"));
    }
    validate_store_path(value)
}

fn validate_relative_symlink_target(path: &Path, root: &Path, target: &str) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("NAR symlink has no parent"))?;
    let relative_parent = parent
        .strip_prefix(root)
        .map_err(|_| invalid("NAR symlink is outside its output root"))?;
    let depth = relative_parent
        .components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .count();
    validate_relative_symlink_depth(target, depth)
}

fn validate_relative_symlink_depth(target: &str, mut depth: usize) -> io::Result<()> {
    for component in Path::new(target).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => depth = depth.saturating_add(1),
            Component::ParentDir if depth > 0 => depth -= 1,
            Component::ParentDir => {
                return Err(invalid("NAR relative symlink escapes its output root"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(invalid("NAR relative symlink is not relative"));
            }
        }
    }
    Ok(())
}

fn validate_store_path(value: &str) -> io::Result<()> {
    let mut parts = value.split('/');
    let safe = value.starts_with('/')
        && value != "/"
        && parts.next() == Some("")
        && parts.all(|part| !part.is_empty() && part != "." && part != "..")
        && !value.contains('\\')
        && !value.contains('\n')
        && !value.contains('\r')
        && !value.contains('\0');
    if !safe {
        return Err(invalid("narinfo has an unsafe store path"));
    }
    Ok(())
}

fn validate_store_reference(value: &str) -> io::Result<()> {
    if value.starts_with('/') {
        validate_store_path(value)
    } else if value.is_empty()
        || value == "."
        || value == ".."
        || value == "unknown-deriver"
        || value.len() > MAX_NAME_BYTES * 4
        || value.contains('/')
        || value.contains('\\')
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        Err(invalid("narinfo has an unsafe store reference"))
    } else {
        Ok(())
    }
}

fn validate_relative_url(value: &str) -> io::Result<()> {
    let path = Path::new(value);
    if value.is_empty()
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control() || matches!(byte, b'?' | b'#' | b'%'))
        || value.contains("//")
        || value.ends_with('/')
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir
                    | Component::CurDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return Err(invalid("narinfo URL is not a safe relative path"));
    }
    Ok(())
}

fn validate_digest(value: &str) -> io::Result<()> {
    if decode_sha256(value).is_some() {
        Ok(())
    } else {
        Err(invalid("narinfo hash is not a SHA-256 digest"))
    }
}

fn digest_for(bytes: &[u8]) -> String {
    format!("sha256:{}", SHA256::sha256_hex(bytes))
}

pub(crate) fn nar_hash_matches(expected: &str, bytes: &[u8]) -> bool {
    hash_matches(expected, bytes)
}

pub(crate) fn normalize_nar_hash(value: &str) -> io::Result<String> {
    let bytes =
        decode_sha256(value).ok_or_else(|| invalid("Nix NarHash is not a SHA-256 digest"))?;
    Ok(format!("sha256:{}", bytes_to_hex(&bytes)))
}

fn hash_matches(expected: &str, bytes: &[u8]) -> bool {
    decode_sha256(expected).is_some_and(|hash| hash == SHA256::sha256(bytes))
}

fn same_digest(left: &str, right: &str) -> bool {
    match (decode_sha256(left), decode_sha256(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if let Some(hex) = value.strip_prefix("sha256:") {
        if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            let mut bytes = [0u8; 32];
            for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
                bytes[index] = (hex_value(pair[0])? << 4) | hex_value(pair[1])?;
            }
            return Some(bytes);
        }
        return decode_nix_base32(hex);
    }
    let encoded = value.strip_prefix("sha256-")?;
    let bytes = jet_foundation::base_encoding_strict::decode_base64(encoded, false, false).ok()?;
    bytes.try_into().ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_nix_base32(value: &str) -> Option<[u8; 32]> {
    const ALPHABET: &[u8] = b"0123456789abcdfghijklmnpqrsvwxyz";
    if value.len() != 52 {
        return None;
    }
    let mut output = [0u8; 32];
    for (index, byte) in value.bytes().enumerate() {
        let digit = ALPHABET.iter().position(|candidate| *candidate == byte)? as u8;
        let bit = (51 - index) * 5;
        for offset in 0..5 {
            if digit & (1 << offset) != 0 {
                let target = bit + offset;
                if target >= 256 {
                    return None;
                }
                output[target / 8] |= 1 << (target % 8);
            }
        }
    }
    Some(output)
}

fn line(output: &mut String, key: &str, value: &str) -> io::Result<()> {
    if key.is_empty() || value.contains('\n') || value.contains('\r') || value.contains('\0') {
        return Err(invalid("narinfo field contains a line break"));
    }
    output.push_str(key);
    output.push_str(": ");
    output.push_str(value);
    output.push('\n');
    Ok(())
}

fn parse_u64(value: &str, field: &str) -> io::Result<u64> {
    value
        .parse()
        .map_err(|_| invalid(&format!("narinfo {field} is not an integer")))
}

fn parse_signature(value: &str) -> io::Result<NarSignature> {
    let mut parts = value.splitn(3, ':');
    let key_id = parts.next().unwrap_or_default();
    let algorithm = parts.next().unwrap_or_default();
    let sig_hex = parts.next().unwrap_or_default();
    if key_id.is_empty() || algorithm.is_empty() || sig_hex.is_empty() {
        return Err(invalid("narinfo signature is malformed"));
    }
    Ok(NarSignature {
        key_id: key_id.to_string(),
        algorithm: algorithm.to_string(),
        sig_hex: sig_hex.to_string(),
    })
}

fn narinfo_name(store_path: &str) -> io::Result<String> {
    let name = store_path
        .rsplit('/')
        .next()
        .ok_or_else(|| invalid("narinfo store path has no name"))?;
    validate_name(name)?;
    Ok(format!("{name}.narinfo"))
}

fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit as u64 {
        return Err(invalid(
            "binary-cache input is not a regular file within its limit",
        ));
    }
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid(
            "binary-cache input exceeded its limit while being read",
        ));
    }
    Ok(bytes)
}

fn write_or_match(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(invalid("binary-cache destination cannot be a symlink"));
        }
    }
    if path.is_file() {
        if read_bounded(path, bytes.len())? == bytes {
            return Ok(());
        }
        return Err(invalid(
            "binary-cache destination already has conflicting bytes",
        ));
    }
    if path.exists() {
        return Err(invalid("binary-cache destination is not a regular file"));
    }
    write_atomic(path, bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::symlink_metadata(path).is_ok() {
        return Err(invalid("binary-cache destination already exists"));
    }
    let partial = path.with_extension(format!(
        "nar-partial-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    match fs::hard_link(&partial, path) {
        Ok(()) => {
            let _ = fs::remove_file(&partial);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let same = read_bounded(path, bytes.len())
                .map(|existing| existing == bytes)
                .unwrap_or(false);
            let _ = fs::remove_file(&partial);
            if same {
                Ok(())
            } else {
                Err(invalid(
                    "binary-cache destination was concurrently published with different bytes",
                ))
            }
        }
        Err(error) => {
            let _ = fs::remove_file(&partial);
            Err(error)
        }
    }
}

fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(mode & 0o7777);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(mode & 0o200 == 0);
    fs::set_permissions(path, permissions)
}

fn remove_tree(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        fs::remove_file(path)
    } else {
        fs::remove_dir_all(path)
    }
}

fn validate_destination_parent(path: &Path) -> io::Result<()> {
    let mut current = if path.is_absolute() {
        PathBuf::from(std::path::MAIN_SEPARATOR_STR)
    } else {
        std::env::current_dir()?
    };
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => {}
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(invalid("NAR destination contains a parent component"));
            }
            Component::Normal(value) => {
                current.push(value);
                if let Ok(metadata) = fs::symlink_metadata(&current) {
                    if metadata.file_type().is_symlink() {
                        return Err(invalid("NAR destination traverses a symlink"));
                    }
                    if !metadata.is_dir() {
                        return Err(invalid("NAR destination parent is not a directory"));
                    }
                }
            }
        }
    }
    Ok(())
}

fn unique_suffix() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{now}", std::process::id())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nar_roundtrip_is_deterministic() {
        let root = std::env::temp_dir().join(format!("jet-nar-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("bin/tool"), [0, 1, 2, 255]).unwrap();
        let (first, stats) = write_nar(&root).unwrap();
        let (second, second_stats) = write_nar(&root).unwrap();
        assert_eq!(first, second);
        assert_eq!(stats, second_stats);
        let destination = root.with_extension("out");
        read_nar(&first, &destination).unwrap();
        assert_eq!(
            fs::read(destination.join("bin/tool")).unwrap(),
            [0, 1, 2, 255]
        );
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(destination);
    }

    #[test]
    fn narinfo_signature_covers_canonical_fields() {
        let key = TrustKey::from_secret(vec![7; 32]).unwrap();
        let info = NarInfo {
            store_path: "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-tool".into(),
            url: "nar/tool.nar".into(),
            compression: "none".into(),
            file_size: 4,
            nar_size: 4,
            nar_hash: digest_for(b"test"),
            references: Vec::new(),
            deriver: None,
            ca: None,
            signatures: Vec::new(),
        };
        let signed = info.signed(&key).unwrap();
        signed.verify(&key).unwrap();
        let text = signed.to_text().unwrap();
        NarInfo::parse(&text).unwrap().verify(&key).unwrap();
    }
}
