//! Canonical Hangar archive operations.
//!
//! One archive format backs export/import, dump/restore, copy, sign, and
//! repair.  The format is deliberately small and deterministic: it contains
//! typed package metadata, content-addressed output trees, and one detached
//! HMAC signature over every byte before the signature trailer.  No archive
//! operation publishes an object before the complete archive has been decoded,
//! authenticated, and re-hashed in quarantine.

use super::Closure;
use super::{list_checked, parse_meta, Roots, StoreEntry};
use crate::RuntimePolicy;
use crate::TrustRoot::{Signature as TrustSignature, TrustKey};
use crate::{Envelope, JSON, SHA256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAGIC: &[u8] = b"jet-hangar-archive-v1\0";
pub const MAX_ARCHIVE_BYTES: usize = 1024 * 1024 * 1024;
const MAX_OBJECTS: usize = 16_384;
const MAX_NODES_PER_OBJECT: usize = 1_000_000;
const MAX_FIELD_BYTES: usize = 16 * 1024 * 1024;
const MAX_NODE_BYTES: u64 = 512 * 1024 * 1024;
const ARCHIVE_STAGE: &str = ".archive-stage";
const ARCHIVE_KEY: &str = "hangar.key";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveReport {
    pub objects: usize,
    pub bytes: u64,
    pub signed: bool,
    pub root: String,
}

#[derive(Debug, Clone)]
struct Archive {
    root_id: String,
    objects: Vec<ArchiveObject>,
    signature: Option<ArchiveSignature>,
}

#[derive(Debug, Clone)]
struct ArchiveObject {
    id: String,
    digest: String,
    meta: String,
    root_mode: u32,
    nodes: Vec<ArchiveNode>,
}

#[derive(Debug, Clone)]
struct ArchiveNode {
    path: String,
    kind: ArchiveNodeKind,
    mode: u32,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveNodeKind {
    Directory,
    File,
    Symlink,
    Hardlink,
}

#[derive(Debug, Clone)]
struct ArchiveSignature {
    key_id: String,
    algorithm: String,
    sig_hex: String,
}

/// Export a package closure into the canonical signed archive.
pub fn export_archive(
    roots: &Roots,
    target: &str,
    include_closure: bool,
    key: Option<&str>,
) -> io::Result<(Vec<u8>, ArchiveReport)> {
    let archive = build_archive(roots, target, include_closure)?;
    let key = signing_key(roots, key, true)?;
    let payload = archive.encode_unsigned()?;
    let signed = Archive {
        root_id: archive.root_id,
        objects: archive.objects,
        signature: Some(ArchiveSignature::from(key.sign(&payload))),
    };
    let bytes = signed.encode()?;
    let root = target.to_string();
    let report = ArchiveReport {
        objects: signed.objects.len(),
        bytes: bytes.len() as u64,
        signed: true,
        root,
    };
    Ok((bytes, report))
}

/// Import a complete archive.  Objects are decoded and hashed in a private
/// staging directory before the closure WAL and package projections are
/// touched.  Existing identical objects are reused; conflicting objects fail
/// closed.
pub fn import_archive(
    roots: &Roots,
    bytes: &[u8],
    key: Option<&str>,
    allow_unsigned: bool,
) -> io::Result<ArchiveReport> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(invalid("archive exceeds the 1 GiB limit"));
    }
    let archive = Archive::decode(bytes)?;
    archive.verify_signature(roots, key, allow_unsigned)?;
    let report = archive.report();
    RuntimePolicy::with_lock(&roots.root, "hangar", || {
        import_archive_unlocked(roots, archive)
    })?;
    Ok(report)
}

/// Read a bounded archive file for a CLI or connector caller.
pub fn read_archive_file(path: &Path) -> io::Result<Vec<u8>> {
    read_bounded(path)
}

/// Atomically write a canonical archive file.
pub fn write_archive_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic(path, bytes)
}

/// Verify an archive or one live Hangar entry without changing state.
pub fn verify_archive(
    roots: &Roots,
    target: &str,
    key: Option<&str>,
) -> io::Result<ArchiveReport> {
    if Path::new(target).is_file() {
        let bytes = read_bounded(Path::new(target))?;
        let archive = Archive::decode(&bytes)?;
        archive.verify_signature(roots, key, false)?;
        verify_archive_contents(roots, &archive)?;
        return Ok(archive.report());
    }
    let entry = select_entry(roots, target)?;
    verify_live_entry(roots, &entry)?;
    let sidecar = signature_path(roots, &entry);
    if sidecar.is_file() {
        let bytes = read_bounded(&sidecar)?;
        let archive = Archive::decode(&bytes)?;
        archive.verify_signature(roots, key, false)?;
        verify_archive_contents(roots, &archive)?;
        if !archive.objects.iter().any(|object| object.id == entry.id) {
            return Err(invalid("the signed archive does not name the requested entry"));
        }
        return Ok(archive.report());
    }
    Ok(ArchiveReport {
        objects: 1,
        bytes: 0,
        signed: false,
        root: entry.id,
    })
}

/// Sign an archive in place, or create a signed single-object archive for a
/// live entry.  The latter is stored as a hidden sidecar under the entry so
/// signing never mutates immutable package metadata.
pub fn sign_archive(
    roots: &Roots,
    target: &str,
    destination: Option<&Path>,
    key: Option<&str>,
) -> io::Result<ArchiveReport> {
    let key = signing_key(roots, key, true)?;
    if Path::new(target).is_file() {
        let bytes = read_bounded(Path::new(target))?;
        let archive = Archive::decode(&bytes)?;
        let signed = sign_decoded(archive, &key)?;
        let out = destination.unwrap_or_else(|| Path::new(target));
        write_atomic(out, &signed.encode()?)?;
        return Ok(signed.report());
    }
    let entry = select_entry(roots, target)?;
    let archive = build_archive(roots, &entry.id, false)?;
    let signed = sign_decoded(archive, &key)?;
    let sidecar = destination
        .map(PathBuf::from)
        .unwrap_or_else(|| signature_path(roots, &entry));
    write_atomic(&sidecar, &signed.encode()?)?;
    Ok(signed.report())
}

/// Copy a closure to another local Jetpack root through the same archive path
/// used by export/import.  Network endpoints are rejected until a transport
/// adapter can preserve the same verified archive contract.
pub fn copy_archive(
    roots: &Roots,
    target: &str,
    destination: &Path,
    key: Option<&str>,
) -> io::Result<ArchiveReport> {
    if destination.to_string_lossy().starts_with("ssh://")
        || destination.to_string_lossy().starts_with("https://")
    {
        return Err(invalid(
            "remote Hangar copy needs a configured transport adapter; export the signed archive and import it on the target",
        ));
    }
    let (bytes, _) = export_archive(roots, target, true, key)?;
    let destination_root = Roots {
        root: destination.to_path_buf(),
        dev_mode: false,
    };
    import_archive(&destination_root, &bytes, key, false)
}

/// Repair a corrupt live object from a signed archive.  No source archive is
/// guessed: a missing repair input is a safe, actionable error.  The damaged
/// object is quarantined before import and restored if any import step fails.
pub fn repair_archive(
    roots: &Roots,
    target: &str,
    source_archive: Option<&Path>,
    key: Option<&str>,
) -> io::Result<ArchiveReport> {
    let entry = select_entry(roots, target)?;
    match verify_live_entry(roots, &entry) {
        Ok(()) => {
            return Ok(ArchiveReport {
                objects: 1,
                bytes: 0,
                signed: signature_path(roots, &entry).is_file(),
                root: entry.id,
            })
        }
        Err(error) if source_archive.is_none() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Hangar entry `{}` is corrupt: {error}; supply `--from <signed.hangar>` to repair it",
                    entry.id
                ),
            ))
        }
        Err(_) => {}
    }
    let source = source_archive.expect("checked above");
    let bytes = read_bounded(source)?;
    let archive = Archive::decode(&bytes)?;
    archive.verify_signature(roots, key, false)?;
    if !archive.objects.iter().any(|object| object.id == entry.id) {
        return Err(invalid("repair archive does not contain the requested entry"));
    }
    let object_path = PathBuf::from(&entry.out);
    let hangar = roots.hangar_dir();
    if !is_under(&object_path, &hangar) {
        return Err(invalid(
            "external provider outputs cannot be repaired by copying a Hangar archive",
        ));
    }
    let quarantine = hangar.join("quarantine");
    fs::create_dir_all(&quarantine)?;
    let backup = quarantine.join(format!(
        "repair-{}-{}",
        entry.envelope.output_hash,
        unique_suffix()
    ));
    fs::rename(&object_path, &backup)?;
    let imported = import_archive(roots, &bytes, key, false);
    match imported {
        Ok(report) => {
            remove_tree(&backup)?;
            Ok(report)
        }
        Err(error) => {
            if object_path.exists() {
                remove_tree(&object_path)?;
            }
            fs::rename(&backup, &object_path)?;
            Err(error)
        }
    }
}

fn build_archive(roots: &Roots, target: &str, include_closure: bool) -> io::Result<Archive> {
    let entry = select_entry(roots, target)?;
    verify_live_entry(roots, &entry)?;
    let graph = Closure::closure_graph_structure(roots)?;
    let root_digest = entry.envelope.output_hash.clone();
    let mut digests = if include_closure {
        graph.closure(&root_digest)
    } else {
        vec![root_digest.clone()]
    };
    if digests.is_empty() {
        digests.push(root_digest.clone());
    }
    digests.sort();
    digests.dedup();

    let entries = list_checked(roots)?;
    let mut selected_entries = BTreeMap::new();
    selected_entries.insert(entry.id.clone(), entry.clone());
    for candidate in entries {
        let owns = candidate
            .named_outputs
            .values()
            .chain(std::iter::once(&candidate.envelope.output_hash))
            .any(|digest| digests.iter().any(|wanted| wanted == digest));
        if owns {
            selected_entries.insert(candidate.id.clone(), candidate);
        }
    }

    let mut objects = Vec::new();
    for digest in &digests {
        let object = graph.objects.get(digest).ok_or_else(|| {
            invalid(&format!("closure object `{digest}` is absent from the Hangar graph"))
        })?;
        let path = PathBuf::from(&object.path);
        let canonical_objects = fs::canonicalize(roots.hangar_dir().join("objects"))
            .unwrap_or_else(|_| roots.hangar_dir().join("objects"));
        if !is_under(&path, &canonical_objects) {
            return Err(invalid(&format!(
                "closure object `{digest}` is outside the local Hangar object pool"
            )));
        }
        objects.push(ArchiveObject {
            id: digest.clone(),
            digest: digest.clone(),
            meta: String::new(),
            root_mode: mode_of(&fs::symlink_metadata(&path)?),
            nodes: collect_nodes(&path)?,
        });
    }

    let mut records = Vec::new();
    for (_, candidate) in selected_entries {
        let primary = candidate.envelope.output_hash.clone();
        if !digests.iter().any(|digest| digest == &primary) {
            continue;
        }
        records.push(ArchiveObject {
            id: candidate.id.clone(),
            digest: primary,
            meta: candidate.meta_json(),
            root_mode: 0,
            nodes: Vec::new(),
        });
    }
    objects.extend(records);
    objects.sort_by(|a, b| a.id.cmp(&b.id).then(a.digest.cmp(&b.digest)));
    let mut output_ids = BTreeSet::new();
    let mut package_ids = BTreeSet::new();
    for object in &objects {
        if object.meta.is_empty() {
            if !output_ids.insert(object.digest.clone()) {
                return Err(invalid("archive contains duplicate output records"));
            }
        } else if !package_ids.insert(object.id.clone()) {
            return Err(invalid("archive contains duplicate package records"));
        }
    }
    Ok(Archive {
        root_id: entry.id,
        objects,
        signature: None,
    })
}

fn import_archive_unlocked(roots: &Roots, archive: Archive) -> io::Result<usize> {
    let stage = roots
        .hangar_dir()
        .join(ARCHIVE_STAGE)
        .join(format!("{}-{}", std::process::id(), unique_suffix()));
    let stage_objects = stage.join("objects");
    fs::create_dir_all(&stage_objects)?;
    let result = (|| {
        let mut package_records = Vec::new();
        let mut seen_digests = BTreeSet::new();
        for object in &archive.objects {
            if !object.meta.is_empty() {
                let meta = parse_meta(&object.meta).ok_or_else(|| {
                    invalid(&format!("package record `{}` has malformed meta.json", object.id))
                })?;
                let entry = portable_entry(roots, object, &meta)?;
                if entry.envelope.output_hash != object.digest {
                    return Err(invalid(&format!(
                        "package record `{}` names `{}` but its primary output is `{}`",
                        entry.id, object.digest, entry.envelope.output_hash
                    )));
                }
                package_records.push(entry);
                continue;
            }
            if !seen_digests.insert(object.digest.clone()) {
                return Err(invalid(&format!(
                    "archive contains duplicate output `{}`",
                    object.digest
                )));
            }
            validate_digest(&object.digest)?;
            let output = stage_objects.join(&object.digest);
            write_nodes(&output, &object.nodes, object.root_mode)?;
            let actual = Envelope::try_output_hash_of(&output.to_string_lossy())
                .map_err(io::Error::other)?;
            if actual != object.digest {
                return Err(invalid(&format!(
                    "archive output `{}` re-hashes as `{actual}`",
                    object.digest
                )));
            }
        }

        for entry in &package_records {
            if !seen_digests.contains(&entry.envelope.output_hash) {
                return Err(invalid(&format!(
                    "package record `{}` has no archived primary output",
                    entry.id
                )));
            }
            for reference in &entry.references {
                if !seen_digests.contains(reference) {
                    return Err(invalid(&format!(
                        "package record `{}` references missing output `{reference}`",
                        entry.id
                    )));
                }
            }
        }

        validate_import_destinations(roots, &package_records, &seen_digests)?;

        let objects_dir = roots.hangar_dir().join("objects");
        fs::create_dir_all(&objects_dir)?;
        let mut moved = Vec::new();
        for digest in &seen_digests {
            let staged = stage_objects.join(digest);
            let destination = objects_dir.join(digest);
            let metadata = fs::symlink_metadata(&destination);
            if metadata.is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                rollback_import_moves(&mut moved)?;
                return Err(invalid(&format!(
                    "existing Hangar object `{digest}` is a symlink"
                )));
            }
            if metadata.is_ok() {
                let actual = Envelope::try_output_hash_of_in_hangar(
                    &destination.to_string_lossy(),
                    &roots.hangar_dir(),
                    false,
                )
                .map_err(io::Error::other)?;
                if actual != *digest {
                    return Err(invalid(&format!(
                        "existing Hangar object `{digest}` has a conflicting digest"
                    )));
                }
                remove_tree(&staged)?;
            } else {
                fs::rename(&staged, &destination)?;
                seal_tree(&destination)?;
                moved.push((destination, staged));
            }
        }

        if let Err(error) = super::Closure::register_entries_unlocked(roots, &package_records) {
            rollback_import_moves(&mut moved)?;
            return Err(error);
        }
        Ok(package_records.len())
    })();
    let _ = remove_tree(&stage);
    result
}

fn portable_entry(
    roots: &Roots,
    object: &ArchiveObject,
    meta: &super::ParsedMeta,
) -> io::Result<StoreEntry> {
    validate_id(&object.id)?;
    let destination = roots.hangar_dir().join("objects").join(&object.digest);
    let source_out = Path::new(&meta.out);
    let out = destination.to_string_lossy().into_owned();
    let bin = map_member_path(source_out, &meta.bin, &destination)?;
    let rlib = map_member_path(source_out, &meta.rlib, &destination)?;
    Ok(StoreEntry {
        id: object.id.clone(),
        name: meta.name.clone(),
        version: meta.version.clone(),
        reference: meta.reference.clone(),
        out,
        bin,
        rlib,
        envelope: meta.envelope.clone(),
        cache_identity: meta.cache_identity.clone(),
        references: meta.references.clone(),
        named_outputs: meta.named_outputs.clone(),
        platform_artifact_kind: meta.platform_artifact_kind.clone(),
        producer_record: meta.producer_record.clone(),
        realized_at: meta.realized_at.unwrap_or_else(now_secs),
        last_used_at: meta.last_used_at.unwrap_or_else(now_secs),
    })
}

fn map_member_path(source_out: &Path, member: &str, destination: &Path) -> io::Result<String> {
    if member.is_empty() {
        return Ok(String::new());
    }
    if source_out.as_os_str().is_empty()
        || source_out
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(invalid("archive package metadata has an unsafe primary output path"));
    }
    let member = Path::new(member);
    let suffix = member.strip_prefix(source_out).map_err(|_| {
        invalid("archive package metadata points outside its primary output")
    })?;
    if suffix.components().any(|component| {
        matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
    }) {
        return Err(invalid("archive package metadata points outside its primary output"));
    }
    Ok(destination.join(suffix).to_string_lossy().into_owned())
}

fn verify_live_entry(roots: &Roots, entry: &StoreEntry) -> io::Result<()> {
    super::verify_hangar_object(roots, entry).map_err(|error| io::Error::other(error.to_string()))
}

fn verify_archive_object(
    roots: &Roots,
    object: &ArchiveObject,
    staged: Option<&Path>,
) -> io::Result<()> {
    let path = staged
        .map(|root| root.join(&object.digest))
        .unwrap_or_else(|| roots.hangar_dir().join("objects").join(&object.digest));
    let actual = Envelope::try_output_hash_of(&path.to_string_lossy()).map_err(io::Error::other)?;
    if actual != object.digest {
        return Err(invalid(&format!(
            "output `{}` re-hashes as `{actual}`",
            object.digest
        )));
    }
    Ok(())
}

fn verify_archive_contents(roots: &Roots, archive: &Archive) -> io::Result<()> {
    let stage = roots
        .hangar_dir()
        .join(ARCHIVE_STAGE)
        .join(format!("verify-{}-{}", std::process::id(), unique_suffix()));
    let result = (|| {
        fs::create_dir_all(&stage)?;
        let mut outputs = BTreeSet::new();
        for object in archive.objects.iter().filter(|object| object.meta.is_empty()) {
            let output = stage.join(&object.digest);
            write_nodes(&output, &object.nodes, object.root_mode)?;
            verify_archive_object(roots, object, Some(&stage))?;
            outputs.insert(object.digest.clone());
        }
        for object in archive.objects.iter().filter(|object| !object.meta.is_empty()) {
            let meta = parse_meta(&object.meta)
                .ok_or_else(|| invalid("archive contains malformed package metadata"))?;
            if meta.envelope.output_hash != object.digest || !outputs.contains(&object.digest) {
                return Err(invalid("archive package metadata has no matching output"));
            }
            if meta.references.iter().any(|reference| !outputs.contains(reference)) {
                return Err(invalid("archive package metadata references a missing output"));
            }
        }
        Ok(())
    })();
    let _ = remove_tree(&stage);
    result
}

fn select_entry(roots: &Roots, target: &str) -> io::Result<StoreEntry> {
    let entries = list_checked(roots)?;
    entries
        .iter()
        .find(|entry| {
            entry.id == target
                || entry.reference == target
                || entry.envelope.output_hash == target
                || format!("{}@{}", entry.name, entry.version) == target
        })
        .cloned()
        .ok_or_else(|| invalid(&format!("no Hangar entry matches `{target}`")))
}

fn collect_nodes(root: &Path) -> io::Result<Vec<ArchiveNode>> {
    if !root.exists() {
        return Err(invalid(&format!("output `{}` is missing", root.display())));
    }
    let mut nodes = Vec::new();
    let canonical_root = fs::canonicalize(root)?;
    let mut hardlinks = BTreeMap::new();
    collect_nodes_at(
        root,
        Path::new(""),
        &canonical_root,
        &mut hardlinks,
        &mut nodes,
    )?;
    nodes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(nodes)
}

fn collect_nodes_at(
    root: &Path,
    relative: &Path,
    canonical_root: &Path,
    hardlinks: &mut BTreeMap<(u64, u64), String>,
    out: &mut Vec<ArchiveNode>,
) -> io::Result<()> {
    if out.len() >= MAX_NODES_PER_OBJECT {
        return Err(invalid("output tree contains too many nodes"));
    }
    let metadata = fs::symlink_metadata(root)?;
    let file_type = metadata.file_type();
    if metadata.is_dir() {
        if !relative.as_os_str().is_empty() {
            out.push(ArchiveNode {
                path: portable_path(relative)?,
                kind: ArchiveNodeKind::Directory,
                mode: mode_of(&metadata),
                bytes: Vec::new(),
            });
        }
        let mut children = fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let child_rel = relative.join(child.file_name());
            collect_nodes_at(&child.path(), &child_rel, canonical_root, hardlinks, out)?;
        }
    } else if metadata.is_file() {
        if metadata.len() > MAX_NODE_BYTES {
            return Err(invalid("archive file exceeds the 512 MiB limit"));
        }
        let path = portable_path(relative)?;
        let link_target = file_identity(&metadata).and_then(|key| {
            if hardlink_count(&metadata) > 1 {
                hardlinks.get(&key).cloned()
            } else {
                None
            }
        });
        if let Some(target) = link_target {
            out.push(ArchiveNode {
                path,
                kind: ArchiveNodeKind::Hardlink,
                mode: mode_of(&metadata),
                bytes: target.into_bytes(),
            });
            return Ok(());
        }
        if let Some(key) = file_identity(&metadata) {
            if hardlink_count(&metadata) > 1 {
                hardlinks.insert(key, path.clone());
            }
        }
        out.push(ArchiveNode {
            path,
            kind: ArchiveNodeKind::File,
            mode: mode_of(&metadata),
            bytes: fs::read(root)?,
        });
    } else if metadata.file_type().is_symlink() {
        let target = fs::read_link(root)?;
        if target.is_absolute() {
            return Err(invalid("absolute symlinks are not portable Hangar archive nodes"));
        }
        let resolved = fs::canonicalize(root).map_err(|error| {
            invalid(&format!("symlink `{}` is dangling or cyclic: {error}", root.display()))
        })?;
        if !resolved.starts_with(canonical_root) {
            return Err(invalid("symlink target escapes the Hangar output root"));
        }
        let target = target
            .to_str()
            .ok_or_else(|| invalid("symlink target is not UTF-8"))?;
        if target.is_empty() || target.contains('\\') || target.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(invalid("symlink target is not portable"));
        }
        out.push(ArchiveNode {
            path: portable_path(relative)?,
            kind: ArchiveNodeKind::Symlink,
            mode: mode_of(&metadata),
            bytes: target.replace(std::path::MAIN_SEPARATOR, "/").into_bytes(),
        });
    } else {
        return Err(invalid("special files are not portable Hangar archive nodes"));
    }
    Ok(())
}

fn write_nodes(root: &Path, nodes: &[ArchiveNode], root_mode: u32) -> io::Result<()> {
    validate_no_path_collisions(nodes)?;
    fs::create_dir_all(root)?;

    // Create the complete directory skeleton before applying archived modes.
    // A read-only parent must not prevent a later child from being restored.
    for node in nodes
        .iter()
        .filter(|node| matches!(node.kind, ArchiveNodeKind::Directory))
    {
        let path = root.join(&node.path);
        if !is_under(&path, root) {
            return Err(invalid("archive node escapes its output root"));
        }
        fs::create_dir_all(&path)?;
    }
    for node in nodes.iter().filter(|node| matches!(node.kind, ArchiveNodeKind::File)) {
        let path = root.join(&node.path);
        if !is_under(&path, root) {
            return Err(invalid("archive node escapes its output root"));
        }
        if node.bytes.len() as u64 > MAX_NODE_BYTES {
            return Err(invalid("archive file exceeds the 512 MiB limit"));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_atomic(&path, &node.bytes)?;
        set_mode(&path, node.mode)?;
    }
    for node in nodes.iter().filter(|node| matches!(node.kind, ArchiveNodeKind::Hardlink)) {
        let path = root.join(&node.path);
        let target = hardlink_target(root, &node.bytes)?;
        if !target.is_file() {
            return Err(invalid("archive hardlink target is not a regular file"));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::hard_link(&target, &path)?;
    }
    for node in nodes.iter().filter(|node| matches!(node.kind, ArchiveNodeKind::Symlink)) {
        let path = root.join(&node.path);
        let target = std::str::from_utf8(&node.bytes)
            .map_err(|_| invalid("archive symlink target is not UTF-8"))?;
        let _ = symlink_or_hardlink_target(root, &path, &node.bytes)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        create_relative_symlink(target, &path)?;
    }
    // Apply directory modes after all children exist, deepest first.
    let mut directories: Vec<&ArchiveNode> = nodes
        .iter()
        .filter(|node| matches!(node.kind, ArchiveNodeKind::Directory))
        .collect();
    directories.sort_by_key(|node| std::cmp::Reverse(node.path.matches('/').count()));
    for node in directories {
        set_mode(&root.join(&node.path), node.mode)?;
    }
    if root_mode != 0 {
        set_mode(root, root_mode)?;
    }
    Ok(())
}

fn symlink_or_hardlink_target(root: &Path, link: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let target = std::str::from_utf8(bytes)
        .map_err(|_| invalid("archive link target is not UTF-8"))?;
    if target.is_empty() || target.contains('\\') || target.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(invalid("archive link target is not portable"));
    }
    let relative = Path::new(target);
    if relative.is_absolute() {
        return Err(invalid("archive link target is absolute"));
    }
    let parent = link.parent().unwrap_or(root);
    let candidate = parent.join(relative);
    let relative_candidate = candidate
        .strip_prefix(root)
        .map_err(|_| invalid("archive link target escapes its output root"))?;
    let mut normalized = PathBuf::new();
    for component in relative_candidate.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => normalized.push(value),
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(invalid("archive link target escapes its output root"));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(invalid("archive link target is not relative"));
            }
        }
    }
    Ok(root.join(normalized))
}

fn hardlink_target(root: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let target = std::str::from_utf8(bytes)
        .map_err(|_| invalid("archive hardlink target is not UTF-8"))?;
    validate_relative(target)?;
    Ok(root.join(target))
}

fn create_relative_symlink(target: &str, link: &Path) -> io::Result<()> {
    let target_path = Path::new(target);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target_path, link)
    }
    #[cfg(windows)]
    {
        let resolved = link.parent().unwrap_or_else(|| Path::new(".")).join(target_path);
        if resolved.is_dir() {
            std::os::windows::fs::symlink_dir(target_path, link)
        } else {
            std::os::windows::fs::symlink_file(target_path, link)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target_path, link);
        Err(invalid("portable Hangar symlinks are unsupported on this host"))
    }
}

fn validate_no_path_collisions(nodes: &[ArchiveNode]) -> io::Result<()> {
    let mut paths = BTreeMap::new();
    for node in nodes {
        validate_relative(&node.path)?;
        if paths.insert(node.path.clone(), node.kind).is_some() {
            return Err(invalid("archive contains duplicate output paths"));
        }
    }
    for path in paths.keys() {
        let components = path.split('/').collect::<Vec<_>>();
        for index in 1..components.len() {
            let ancestor = components[..index].join("/");
            if paths
                .get(&ancestor)
                .is_some_and(|kind| *kind != ArchiveNodeKind::Directory)
            {
                return Err(invalid(
                    "archive contains a file/link path collision",
                ));
            }
        }
    }
    Ok(())
}

fn validate_import_destinations(
    roots: &Roots,
    entries: &[StoreEntry],
    digests: &BTreeSet<String>,
) -> io::Result<()> {
    let objects_dir = roots.hangar_dir().join("objects");
    for digest in digests {
        let destination = objects_dir.join(digest);
        if let Ok(metadata) = fs::symlink_metadata(&destination) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(invalid(&format!(
                    "existing Hangar object `{digest}` is not a directory"
                )));
            }
        }
    }
    for entry in entries {
        let destination = roots.hangar_dir().join(&entry.id);
        match fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(invalid(&format!(
                    "existing Hangar record `{}` is not a directory",
                    entry.id
                )))
            }
            Ok(_) => {
                let meta = fs::read_to_string(destination.join("meta.json"))?;
                if meta != entry.meta_json() {
                    return Err(invalid(&format!(
                        "existing Hangar record `{}` conflicts with the archive",
                        entry.id
                    )));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn rollback_import_moves(moved: &mut Vec<(PathBuf, PathBuf)>) -> io::Result<()> {
    for (destination, staged) in moved.drain(..).rev() {
        if fs::symlink_metadata(&destination).is_ok() {
            make_tree_writable(&destination)?;
            fs::rename(destination, staged)?;
        }
    }
    Ok(())
}

fn sign_decoded(mut archive: Archive, key: &TrustKey) -> io::Result<Archive> {
    archive.signature = None;
    let payload = archive.encode_unsigned()?;
    archive.signature = Some(ArchiveSignature::from(key.sign(&payload)));
    Ok(archive)
}

fn signing_key(roots: &Roots, requested: Option<&str>, create: bool) -> io::Result<TrustKey> {
    let path = requested
        .map(|value| key_path(roots, value))
        .unwrap_or_else(|| roots.root.join("trust").join(ARCHIVE_KEY));
    if !path.is_file() {
        if !create {
            return Err(invalid(&format!(
                "archive signer key `{}` is unavailable; pass `--key <file>`",
                path.display()
            )));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let secret = entropy(roots);
        write_atomic(&path, &secret)?;
        set_mode(&path, 0o600)?;
    }
    let bytes = fs::read(&path)?;
    let secret = decode_secret(&bytes)?;
    TrustKey::from_secret(secret).map_err(|error| invalid(&format!("invalid archive signer key: {error}")))
}

fn key_path(roots: &Roots, requested: &str) -> PathBuf {
    let path = PathBuf::from(requested);
    if path.components().count() > 1 || path.is_absolute() || path.is_file() {
        path
    } else {
        roots.root.join("trust").join(format!("{requested}.key"))
    }
}

fn decode_secret(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let trimmed = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    if trimmed.len() >= 32
        && trimmed.len() % 2 == 0
        && trimmed.iter().all(|byte| byte.is_ascii_hexdigit())
    {
        let mut out = Vec::with_capacity(trimmed.len() / 2);
        for pair in trimmed.chunks_exact(2) {
            let high = hex_value(pair[0]).ok_or_else(|| invalid("archive key has invalid hex"))?;
            let low = hex_value(pair[1]).ok_or_else(|| invalid("archive key has invalid hex"))?;
            out.push((high << 4) | low);
        }
        return Ok(out);
    }
    Ok(trimmed.to_vec())
}

fn entropy(roots: &Roots) -> Vec<u8> {
    #[cfg(unix)]
    if let Ok(bytes) = fs::read("/dev/urandom") {
        if bytes.len() >= 32 {
            return bytes[..32].to_vec();
        }
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let material = format!(
        "jet-hangar-key-v1\n{}\n{}\n{}",
        roots.root.display(),
        std::process::id(),
        now
    );
    let digest = SHA256::sha256_hex(material.as_bytes());
    let mut out = Vec::new();
    for pair in digest.as_bytes().chunks_exact(2) {
        let high = hex_value(pair[0]).unwrap_or(0);
        let low = hex_value(pair[1]).unwrap_or(0);
        out.push((high << 4) | low);
    }
    out
}

impl ArchiveSignature {
    fn from(signature: TrustSignature) -> Self {
        Self {
            key_id: signature.key_id,
            algorithm: signature.algorithm,
            sig_hex: signature.sig_hex,
        }
    }
}

impl Archive {
    fn report(&self) -> ArchiveReport {
        let bytes = self
            .objects
            .iter()
            .flat_map(|object| object.nodes.iter())
            .map(|node| node.bytes.len() as u64)
            .sum();
        ArchiveReport {
            objects: self
                .objects
                .iter()
                .filter(|object| object.meta.is_empty())
                .count(),
            bytes,
            signed: self.signature.is_some(),
            root: self.root_id.clone(),
        }
    }

    fn verify_signature(&self, roots: &Roots, requested: Option<&str>, allow_unsigned: bool) -> io::Result<()> {
        let Some(signature) = &self.signature else {
            if allow_unsigned {
                return Ok(());
            }
            return Err(invalid("unsigned Hangar archives are refused by default"));
        };
        let key = signing_key(roots, requested, false)?;
        if key.key_id != signature.key_id {
            return Err(invalid(&format!(
                "archive signer `{}` is not the configured key `{}`",
                signature.key_id, key.key_id
            )));
        }
        if key.algorithm != signature.algorithm {
            return Err(invalid(&format!(
                "archive uses unsupported signature algorithm `{}`",
                signature.algorithm
            )));
        }
        let mut unsigned = self.clone();
        unsigned.signature = None;
        let expected = key.sign(&unsigned.encode_unsigned()?).sig_hex;
        if expected != signature.sig_hex {
            return Err(invalid("Hangar archive signature verification failed"));
        }
        Ok(())
    }

    fn encode_unsigned(&self) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        put_string(&mut out, &self.root_id)?;
        put_u32(&mut out, self.objects.len())?;
        for object in &self.objects {
            put_string(&mut out, &object.id)?;
            put_string(&mut out, &object.digest)?;
            put_string(&mut out, &object.meta)?;
            put_raw_u32(&mut out, object.root_mode);
            put_u32(&mut out, object.nodes.len())?;
            for node in &object.nodes {
                let kind = match node.kind {
                    ArchiveNodeKind::Directory => 0,
                    ArchiveNodeKind::File => 1,
                    ArchiveNodeKind::Symlink => 2,
                    ArchiveNodeKind::Hardlink => 3,
                };
                out.push(kind);
                put_string(&mut out, &node.path)?;
                put_raw_u32(&mut out, node.mode);
                put_u64(&mut out, node.bytes.len() as u64)?;
                out.extend_from_slice(&node.bytes);
            }
        }
        Ok(out)
    }

    fn encode(&self) -> io::Result<Vec<u8>> {
        let mut out = self.encode_unsigned()?;
        match &self.signature {
            Some(signature) => {
                out.push(1);
                put_string(&mut out, &signature.key_id)?;
                put_string(&mut out, &signature.algorithm)?;
                put_string(&mut out, &signature.sig_hex)?;
            }
            None => out.push(0),
        }
        Ok(out)
    }

    fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > MAX_ARCHIVE_BYTES {
            return Err(invalid("archive exceeds the 1 GiB limit"));
        }
        let mut reader = Reader::new(bytes);
        if reader.take(MAGIC.len())? != MAGIC {
            return Err(invalid("archive has an unknown format"));
        }
        let root_id = reader.string()?;
        validate_id(&root_id)?;
        let count = reader.u32()? as usize;
        if count > MAX_OBJECTS {
            return Err(invalid("archive contains too many records"));
        }
        let mut objects = Vec::with_capacity(count);
        let mut output_digests = BTreeSet::new();
        let mut package_ids = BTreeSet::new();
        for _ in 0..count {
            let id = reader.string()?;
            let digest = reader.string()?;
            let meta = reader.string()?;
            validate_id(&id)?;
            validate_digest(&digest)?;
            if !meta.is_empty() {
                parse_meta(&meta).ok_or_else(|| invalid("archive contains malformed package metadata"))?;
                if !package_ids.insert(id.clone()) {
                    return Err(invalid("archive contains duplicate package records"));
                }
            } else {
                if id != digest || !output_digests.insert(digest.clone()) {
                    return Err(invalid("archive contains duplicate or mismatched output records"));
                }
            }
            let root_mode = reader.u32()?;
            let node_count = reader.u32()? as usize;
            if node_count > MAX_NODES_PER_OBJECT {
                return Err(invalid("archive output has too many nodes"));
            }
            let mut nodes = Vec::with_capacity(node_count);
            for _ in 0..node_count {
                let kind = reader.byte()?;
                let path = reader.string()?;
                validate_relative(&path)?;
                let mode = reader.u32()?;
                let size = reader.u64()?;
                if size > MAX_NODE_BYTES {
                    return Err(invalid("archive node exceeds the 512 MiB limit"));
                }
                let payload = match kind {
                    0 if size == 0 => Vec::new(),
                    1..=3 => reader.bytes(size as usize)?,
                    0 => return Err(invalid("archive directory node has a payload")),
                    _ => return Err(invalid("archive contains an unknown node kind")),
                };
                let node_kind = match kind {
                    0 => ArchiveNodeKind::Directory,
                    1 => ArchiveNodeKind::File,
                    2 => ArchiveNodeKind::Symlink,
                    3 => ArchiveNodeKind::Hardlink,
                    _ => unreachable!(),
                };
                nodes.push(ArchiveNode {
                    path,
                    kind: node_kind,
                    mode,
                    bytes: payload,
                });
            }
            validate_no_path_collisions(&nodes)?;
            objects.push(ArchiveObject {
                id,
                digest,
                meta,
                root_mode,
                nodes,
            });
        }
        let signature = match reader.byte()? {
            0 => None,
            1 => Some(ArchiveSignature {
                key_id: reader.string()?,
                algorithm: reader.string()?,
                sig_hex: reader.string()?,
            }),
            _ => return Err(invalid("archive has an unknown signature trailer")),
        };
        if reader.remaining() != 0 {
            return Err(invalid("archive has trailing bytes"));
        }
        if !objects.iter().any(|object| object.id == root_id) {
            return Err(invalid("archive root does not name an archived record or output"));
        }
        Ok(Self {
            root_id,
            objects,
            signature,
        })
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, len: usize) -> io::Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(len)
            .ok_or_else(|| invalid("archive length overflows"))?;
        let bytes = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| invalid("archive is truncated"))?;
        self.at = end;
        Ok(bytes)
    }

    fn byte(&mut self) -> io::Result<u8> {
        Ok(*self.take(1)?.first().expect("one byte was requested"))
    }

    fn u32(&mut self) -> io::Result<u32> {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> io::Result<u64> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(bytes))
    }

    fn string(&mut self) -> io::Result<String> {
        let len = self.u32()? as usize;
        if len > MAX_FIELD_BYTES {
            return Err(invalid("archive field is too large"));
        }
        String::from_utf8(self.take(len)?.to_vec())
            .map_err(|_| invalid("archive field is not UTF-8"))
    }

    fn bytes(&mut self, len: usize) -> io::Result<Vec<u8>> {
        Ok(self.take(len)?.to_vec())
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }
}

fn put_u32(out: &mut Vec<u8>, value: usize) -> io::Result<()> {
    let value = u32::try_from(value).map_err(|_| invalid("archive field is too large"))?;
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn put_raw_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) -> io::Result<()> {
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn put_string(out: &mut Vec<u8>, value: &str) -> io::Result<()> {
    if value.len() > MAX_FIELD_BYTES {
        return Err(invalid("archive field is too large"));
    }
    put_u32(out, value.len())?;
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn validate_id(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > 4096
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(invalid("archive identity is not one safe path component"));
    }
    Ok(())
}

fn validate_digest(value: &str) -> io::Result<()> {
    if !value.starts_with("sha256-") || value.len() != 71 || !value[7..].bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid(&format!("invalid output digest `{value}`")));
    }
    Ok(())
}

fn validate_relative(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.contains('\\')
        || value.contains("//")
        || value.ends_with('/')
        || value.bytes().any(|byte| byte == 0)
    {
        return Err(invalid("archive path is empty or uses a foreign separator"));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
                    | Component::CurDir
            )
        })
    {
        return Err(invalid(&format!("archive path `{value}` escapes its root")));
    }
    Ok(())
}

fn portable_path(path: &Path) -> io::Result<String> {
    let value = path.to_str().ok_or_else(|| invalid("archive path is not UTF-8"))?;
    if value.is_empty() {
        return Err(invalid("archive path is empty"));
    }
    let portable = value.replace(std::path::MAIN_SEPARATOR, "/");
    validate_relative(&portable)?;
    Ok(portable)
}

fn is_under(path: &Path, root: &Path) -> bool {
    path == root || path.strip_prefix(root).is_ok()
}

fn mode_of(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode()
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o644
    }
}

#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn file_identity(_metadata: &fs::Metadata) -> Option<(u64, u64)> {
    None
}

#[cfg(unix)]
fn hardlink_count(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink()
}

#[cfg(not(unix))]
fn hardlink_count(_metadata: &fs::Metadata) -> u64 {
    1
}

fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(mode & 0o7777);
    }
    #[cfg(not(unix))]
    {
        permissions.set_readonly(mode & 0o200 == 0);
    }
    fs::set_permissions(path, permissions)
}

fn seal_tree(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        let children = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        for child in children {
            seal_tree(&child.path())?;
        }
    }
    let mut permissions = metadata.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() & !0o222);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions)
}

fn signature_path(roots: &Roots, entry: &StoreEntry) -> PathBuf {
    roots.hangar_dir().join(&entry.id).join(".hangar")
}

fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(invalid("archive input may not be a symlink"));
    }
    if !metadata.is_file() || metadata.len() > MAX_ARCHIVE_BYTES as u64 {
        return Err(invalid("archive input is not a regular file within the size limit"));
    }
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(invalid("archive destination may not be a symlink"));
        }
    }
    let partial = path.with_extension(format!(
        "jet-partial-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&partial, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result?;
    if let Some(parent) = path.parent() {
        fsync_directory(parent)?;
    }
    Ok(())
}

fn fsync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        return fs::File::open(path)?.sync_all();
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn remove_tree(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        fs::remove_file(path)
    } else if metadata.is_dir() {
        make_tree_writable(path)?;
        fs::remove_dir_all(path)
    } else {
        set_mode(path, 0o600)?;
        fs::remove_file(path)
    }
}

fn make_tree_writable(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        for child in fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()? {
            make_tree_writable(&child.path())?;
        }
    }
    let mut permissions = metadata.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions)
}

fn unique_suffix() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{}", std::process::id(), now)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// Stable machine output used by the Hangar CLI.  It intentionally does not
/// include host paths or timestamps.
pub fn report_json(action: &str, report: &ArchiveReport) -> String {
    format!(
        "{{\"action\":{},\"bytes\":{},\"objects\":{},\"root\":{},\"signed\":{}}}",
        JSON::quote(action),
        report.bytes,
        report.objects,
        JSON::quote(&report.root),
        report.signed
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_rejects_parent_paths() {
        assert!(validate_relative("../outside").is_err());
        assert!(validate_relative("a/../../outside").is_err());
        assert!(validate_relative("a\\outside").is_err());
    }

    #[test]
    fn archive_roundtrip_preserves_binary_nodes() {
        let archive = Archive {
            root_id: "sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            objects: vec![ArchiveObject {
                id: "sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                digest: "sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                meta: String::new(),
                root_mode: 0o755,
                nodes: vec![ArchiveNode {
                    path: "bin/tool".into(),
                    kind: ArchiveNodeKind::File,
                    mode: 0o755,
                    bytes: vec![0, 1, 2, 255],
                }],
            }],
            signature: None,
        };
        let bytes = archive.encode().unwrap();
        let decoded = Archive::decode(&bytes).unwrap();
        assert_eq!(decoded.objects[0].nodes[0].bytes, vec![0, 1, 2, 255]);
        assert_eq!(decoded.objects[0].nodes[0].mode, 0o755);
    }

    #[test]
    fn archive_signature_covers_unsigned_payload() {
        let key = TrustKey::from_secret(vec![7; 32]).unwrap();
        let digest = "sha256-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let archive = Archive {
            root_id: digest.into(),
            objects: vec![ArchiveObject {
                id: digest.into(),
                digest: digest.into(),
                meta: String::new(),
                root_mode: 0o755,
                nodes: Vec::new(),
            }],
            signature: None,
        };
        let signed = sign_decoded(archive, &key).unwrap();
        let bytes = signed.encode().unwrap();
        let decoded = Archive::decode(&bytes).unwrap();
        let root = Roots {
            root: std::env::temp_dir().join("jet-archive-signature-test"),
            dev_mode: false,
        };
        let key_path = root.root.join("trust/test.key");
        fs::create_dir_all(key_path.parent().unwrap()).unwrap();
        fs::write(&key_path, vec![7; 32]).unwrap();
        decoded
            .verify_signature(&root, Some(key_path.to_str().unwrap()), false)
            .unwrap();
        let _ = remove_tree(&root.root);
    }
}
