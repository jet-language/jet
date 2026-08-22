//! Reproducibility certification at the Hangar closure boundary.
//!
//! The registration gate is the one place every locally realized object must
//! cross before it becomes a closure fact. This module compares only entries
//! with the same action identity, records immutable divergence evidence, and
//! leaves the trusted closure/cache graph unchanged on failure.

use super::{entry_action_key, list_unlocked, Ingest, ProducerRecord, Roots, StoreEntry};
use crate::{Envelope, SHA256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const REPORT_DIR: &str = "unreproducible";
const REPORT_STATUS: &str = "unreproducible";
const REPORT_SCHEMA: &str = "jet-reproducibility-report-v1";
const CERTIFICATION_STAGE_DIR: &str = "reproducibility-staging";

static REPORT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static CERTIFICATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn recover_certification_staging_unlocked(roots: &Roots) -> io::Result<usize> {
    let parent = roots.hangar_dir().join(CERTIFICATION_STAGE_DIR);
    let metadata = match fs::symlink_metadata(&parent) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid("reproducibility staging is not a real directory"));
    }
    let mut swept = 0;
    for entry in fs::read_dir(&parent)? {
        let path = entry?.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("reproducibility staging contains a non-directory"));
        }
        super::make_tree_writable_for_removal(&path)?;
        fs::remove_dir_all(path)?;
        swept += 1;
    }
    Ok(swept)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FirstDifference {
    path: String,
    kind: String,
    left: String,
    right: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeObservation {
    kind: &'static str,
    mode: u32,
    digest: Option<String>,
    target: Option<String>,
    hardlink: Option<String>,
    xattrs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OutputSnapshot {
    digest: String,
    nodes: BTreeMap<Vec<u8>, NodeObservation>,
}

/// A provider result that has not crossed the Hangar registration boundary.
/// The workspace guards stay alive until the caller has promoted `realized`.
pub(crate) struct PreparedRealization {
    pub realized: super::super::Provider::Realized,
    pub action_key: Option<String>,
    pub attestation: Option<String>,
    _workspaces: Vec<RootWorkspace>,
}

struct RootWorkspace {
    path: PathBuf,
    roots: Roots,
}

impl RootWorkspace {
    fn new(roots: &Roots, side: &str, attempt: usize) -> io::Result<Self> {
        let hangar = roots.hangar_dir();
        Ingest::ensure_real_directory(&hangar, "Hangar root")?;
        let parent = hangar.join(CERTIFICATION_STAGE_DIR);
        Ingest::ensure_real_directory(&parent, "reproducibility staging")?;
        let path = loop {
            let sequence = CERTIFICATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                "{}-{}-{}-{}",
                std::process::id(),
                side,
                attempt,
                sequence
            ));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        let roots = Roots::at(path.clone());
        if let Err(error) =
            Ingest::ensure_real_directory(&roots.hangar_dir(), "independent Hangar root")
        {
            let _ = super::make_tree_writable_for_removal(&path);
            let _ = fs::remove_dir_all(&path);
            return Err(error);
        }
        Ok(Self { path, roots })
    }
}

impl Drop for RootWorkspace {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path).is_ok() {
            let _ = super::make_tree_writable_for_removal(&self.path);
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Build through the real provider boundary without consulting a shared
/// cache. This is also the seam used by the normal realization path when a
/// writable shared cache would otherwise receive the result.
pub(crate) fn realize_uncached(
    roots: &Roots,
    ctx: &super::super::Provider::Ctx<'_>,
    request: &super::RealizeRequest<'_>,
) -> Result<super::super::Provider::Realized, super::RealizeError> {
    match request {
        super::RealizeRequest::Package { spec, table } => {
            super::super::Provider::realize(spec, table, ctx).map_err(super::RealizeError::Provider)
        }
        super::RealizeRequest::Adapter {
            plan,
            table,
            expectation,
        } => {
            let (tools, _dependency_leases) =
                super::realize_adapter_tools(roots, ctx, plan, table)?;
            let mut realized =
                super::super::Provider::realize_adapter(plan, ctx, expectation, &tools, table)
                    .map_err(super::RealizeError::Provider)?;
            super::bind_adapter_hook_identity(&mut realized, plan, table, expectation, ctx)
                .map_err(super::RealizeError::Store)?;
            Ok(realized)
        }
    }
}

/// Build one or two fresh roots. A non-source provider is returned once for
/// the ordinary path; the explicit certification API rejects it.
pub(crate) fn build_for_cache(
    roots: &Roots,
    ctx: &super::super::Provider::Ctx<'_>,
    request: &super::RealizeRequest<'_>,
    options: &super::IndependentRootOptions<'_>,
    require_built: bool,
) -> Result<PreparedRealization, super::RealizeError> {
    let attempts = options.retries.saturating_add(1);
    for attempt in 0..attempts {
        check_cancelled(options)?;
        let left_workspace =
            RootWorkspace::new(roots, "left", attempt).map_err(super::RealizeError::Store)?;
        let left_store = left_workspace.roots.hangar_dir();
        let left_ctx = super::super::Provider::Ctx {
            fixtures: ctx.fixtures,
            store_dir: &left_store,
            offline: ctx.offline,
            project_dir: ctx.project_dir,
        };
        let left = match realize_uncached(&left_workspace.roots, &left_ctx, request) {
            Ok(realized) => realized,
            Err(_error) if attempt + 1 < attempts => continue,
            Err(error) => {
                let _ = super::super::BuildDebug::promote_failed_attempt(
                    &left_workspace.roots.hangar_dir(),
                    &roots.hangar_dir(),
                    request_package(request),
                );
                return Err(error);
            }
        };
        if left.source_state != super::super::Provider::SourceState::Built {
            if require_built {
                return Err(super::RealizeError::Provider(
                    super::super::Provider::ProviderError::BadOutput(
                        "independent certification accepts only a fresh source build; substituted, unsigned, or replayed results are rejected".into(),
                    ),
                ));
            }
            return Ok(PreparedRealization {
                realized: left,
                action_key: None,
                attestation: None,
                _workspaces: vec![left_workspace],
            });
        }

        check_cancelled(options)?;
        let right_workspace =
            RootWorkspace::new(roots, "right", attempt).map_err(super::RealizeError::Store)?;
        let right_store = right_workspace.roots.hangar_dir();
        let right_ctx = super::super::Provider::Ctx {
            fixtures: ctx.fixtures,
            store_dir: &right_store,
            offline: ctx.offline,
            project_dir: ctx.project_dir,
        };
        let right = match realize_uncached(&right_workspace.roots, &right_ctx, request) {
            Ok(realized) => realized,
            Err(_error) if attempt + 1 < attempts => continue,
            Err(error) => {
                let _ = super::super::BuildDebug::promote_failed_attempt(
                    &right_workspace.roots.hangar_dir(),
                    &roots.hangar_dir(),
                    request_package(request),
                );
                return Err(error);
            }
        };
        let left_entry = entry_from_realized(&left_workspace.roots, &left)
            .map_err(super::RealizeError::Store)?;
        let right_entry = entry_from_realized(&right_workspace.roots, &right)
            .map_err(super::RealizeError::Store)?;
        let left_action = super::entry_action_key(&left_entry);
        let right_action = super::entry_action_key(&right_entry);
        let difference = if left_action != right_action {
            Some(FirstDifference {
                path: ".".into(),
                kind: "action-identity".into(),
                left: left_action.clone(),
                right: right_action.clone(),
            })
        } else {
            compare_entries(
                &left_workspace.roots,
                &left_entry,
                &right_workspace.roots,
                &right_entry,
            )
            .map_err(super::RealizeError::Store)?
        };
        if let Some(difference) = difference {
            if attempt + 1 < attempts {
                continue;
            }
            let left_producer = decode_producer(&left_entry).map_err(super::RealizeError::Store)?;
            let right_producer =
                decode_producer(&right_entry).map_err(super::RealizeError::Store)?;
            let report = report_json(
                &left_action,
                &left_action,
                &right_action,
                &left_entry,
                &right_entry,
                &left_producer,
                &right_producer,
                &difference,
            );
            let report_path =
                persist_report(roots, &left_action, &report).map_err(super::RealizeError::Store)?;
            return Err(super::RealizeError::Store(unreproducible_error(
                &left_action,
                &format!(
                    "conflicting independent roots at `{}`; report `{}`",
                    difference.path,
                    report_path.display()
                ),
            )));
        }
        check_cancelled(options)?;
        let attestation = independent_attestation(&left_action, &left.envelope.output_hash);
        return Ok(PreparedRealization {
            realized: left,
            action_key: Some(left_action),
            attestation: Some(attestation),
            _workspaces: vec![left_workspace, right_workspace],
        });
    }
    Err(super::RealizeError::Store(io::Error::other(
        "independent reproducibility runner exhausted its attempts",
    )))
}

fn request_package<'a>(request: &'a super::RealizeRequest<'a>) -> &'a str {
    match request {
        super::RealizeRequest::Package { spec, .. } => &spec.package,
        super::RealizeRequest::Adapter { plan, .. } => &plan.name,
    }
}

fn check_cancelled(options: &super::IndependentRootOptions<'_>) -> Result<(), super::RealizeError> {
    if options.cancelled.is_some_and(|cancelled| cancelled()) {
        return Err(super::RealizeError::Store(io::Error::new(
            io::ErrorKind::Interrupted,
            "independent reproducibility certification cancelled",
        )));
    }
    Ok(())
}

fn independent_attestation(action_key: &str, output_hash: &str) -> String {
    let input = format!("jet-independent-cert-v1\n{action_key}\n{output_hash}\n");
    format!(
        "independent-agreeing-v1:sha256-{}",
        SHA256::sha256_hex(input.as_bytes())
    )
}

fn entry_from_realized(
    roots: &Roots,
    realized: &super::super::Provider::Realized,
) -> io::Result<StoreEntry> {
    if realized.source_state != super::super::Provider::SourceState::Built {
        return Err(invalid("independent root result was not built from source"));
    }
    let hangar = roots.hangar_dir();
    let canonical_hangar = fs::canonicalize(&hangar).unwrap_or(hangar.clone());
    let canonical_out = fs::canonicalize(&realized.out)
        .map_err(|error| invalid(&format!("independent root output is unavailable: {error}")))?;
    if !canonical_out.starts_with(&canonical_hangar) {
        return Err(invalid(
            "independent root result points outside its private Hangar",
        ));
    }
    for (label, path) in [
        ("out", realized.out.as_str()),
        ("bin", realized.bin.as_str()),
        ("rlib", realized.rlib.as_str()),
    ] {
        if path.is_empty() {
            continue;
        }
        let canonical = fs::canonicalize(path).map_err(|error| {
            invalid(&format!(
                "independent root `{label}` output is unavailable: {error}"
            ))
        })?;
        if !canonical.starts_with(&canonical_out) {
            return Err(invalid(&format!(
                "independent root `{label}` output escapes the primary output"
            )));
        }
    }
    if realized.envelope.output_hash.is_empty()
        || realized.reference.trim().is_empty()
        || realized.envelope.provenance.trim().is_empty()
        || realized.cache_identity.source_fingerprint.is_empty()
        || realized.cache_identity.recipe_fingerprint.is_empty()
        || realized.cache_identity.policy_fingerprint.is_empty()
        || realized.cache_identity.platform.is_empty()
        || realized.envelope.platform != realized.cache_identity.platform
    {
        return Err(invalid(
            "independent root result has incomplete or mismatched build identity",
        ));
    }
    let mut producer = realized.producer.clone();
    producer.bind_cache_provenance(
        &realized.reference,
        &realized.envelope.output_hash,
        &realized.cache_identity,
        &realized.references,
    );
    super::super::Provider::refresh_provider_facts(&mut producer, &realized.reference)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let producer = ProducerRecord::decode(&producer.encode()).map_err(io::Error::other)?;
    let actual = Ingest::try_entry_output_hash(
        roots,
        &StoreEntry {
            id: String::new(),
            name: realized.name.clone(),
            version: realized.version.clone(),
            reference: realized.reference.clone(),
            out: realized.out.clone(),
            bin: realized.bin.clone(),
            rlib: realized.rlib.clone(),
            envelope: realized.envelope.clone(),
            cache_identity: realized.cache_identity.clone(),
            references: realized.references.clone(),
            named_outputs: BTreeMap::new(),
            platform_artifact_kind: String::new(),
            producer_record: producer.encode(),
            receipt: String::new(),
            realized_at: 0,
            last_used_at: 0,
        },
    )
    .map_err(io::Error::other)?;
    if actual != realized.envelope.output_hash {
        return Err(invalid(&format!(
            "independent root output re-hashed as `{actual}`, expected `{}`",
            realized.envelope.output_hash
        )));
    }
    let mut named_outputs = BTreeMap::new();
    for (name, path) in &realized.named_outputs {
        let canonical = fs::canonicalize(path)?;
        if !canonical.starts_with(&canonical_out) {
            return Err(invalid(&format!(
                "independent named output `{name}` escapes the primary output"
            )));
        }
        if name == "out" && canonical != canonical_out {
            return Err(invalid(
                "independent named output `out` disagrees with the primary output",
            ));
        }
        let digest = if name == "out" {
            realized.envelope.output_hash.clone()
        } else {
            Envelope::try_output_hash_of_in_hangar(path, &canonical_hangar, false)
                .map_err(io::Error::other)?
        };
        named_outputs.insert(name.clone(), digest);
    }
    named_outputs.insert("out".into(), realized.envelope.output_hash.clone());
    let id = super::entry_id(
        &realized.name,
        &realized.version,
        &realized.reference,
        &realized.out,
    );
    Ok(StoreEntry {
        id,
        name: realized.name.clone(),
        version: realized.version.clone(),
        reference: realized.reference.clone(),
        out: realized.out.clone(),
        bin: realized.bin.clone(),
        rlib: realized.rlib.clone(),
        envelope: realized.envelope.clone(),
        cache_identity: realized.cache_identity.clone(),
        references: realized.references.clone(),
        named_outputs,
        platform_artifact_kind: String::new(),
        producer_record: producer.encode(),
        receipt: String::new(),
        realized_at: 0,
        last_used_at: 0,
    })
}

/// Check a candidate before the closure transaction can publish it.
/// `additional` contains entries in the same not-yet-committed batch.
pub(crate) fn certify_registration_unlocked(
    roots: &Roots,
    entry: &StoreEntry,
    additional: &[StoreEntry],
) -> io::Result<()> {
    certify_registration_unlocked_mode(roots, entry, additional, None)
}

pub(crate) fn certify_registration_unlocked_with_fresh_agreement(
    roots: &Roots,
    entry: &StoreEntry,
    additional: &[StoreEntry],
    action_key: &str,
) -> io::Result<()> {
    certify_registration_unlocked_mode(roots, entry, additional, Some(action_key))
}

fn certify_registration_unlocked_mode(
    roots: &Roots,
    entry: &StoreEntry,
    additional: &[StoreEntry],
    fresh_action_key: Option<&str>,
) -> io::Result<()> {
    let action_key = entry_action_key(entry);
    let producer = decode_producer(entry)?;
    let fresh_agreement = fresh_action_key == Some(action_key.as_str())
        && producer
            .facts
            .get("cache.reproducibility")
            .is_some_and(|value| value.starts_with("independent-agreeing-v1:"));
    if reproducibility_blocked(roots, &action_key)? && !fresh_agreement {
        return Err(unreproducible_error(
            &action_key,
            "this action already has durable unreproducible evidence",
        ));
    }
    let mut candidates = BTreeMap::new();
    for candidate in list_unlocked(roots)?
        .into_iter()
        .chain(additional.iter().cloned())
    {
        if candidate.id != entry.id && entry_action_key(&candidate) == action_key {
            candidates.insert(candidate.id.clone(), candidate);
        }
    }

    for candidate in candidates.into_values() {
        let candidate_producer = decode_producer(&candidate)?;
        verify_existing_output(roots, &candidate)?;
        let (left, right, left_producer, right_producer) =
            if entry_sort_key(entry) <= entry_sort_key(&candidate) {
                (entry, &candidate, &producer, &candidate_producer)
            } else {
                (&candidate, entry, &candidate_producer, &producer)
            };
        let difference =
            compare_registration_entries(roots, left, right, left_producer, right_producer)?;
        if let Some(difference) = difference {
            let report = report_json(
                &action_key,
                &action_key,
                &action_key,
                left,
                right,
                left_producer,
                right_producer,
                &difference,
            );
            let report_path = persist_report(roots, &action_key, &report)?;
            return Err(unreproducible_error(
                &action_key,
                &format!(
                    "conflicting bytes or provenance at `{}`; report `{}`",
                    difference.path,
                    report_path.display()
                ),
            ));
        }
    }
    Ok(())
}

/// Nix records may arrive one named output at a time while they are being
/// folded into one derivation action. Compare only the shared output names at
/// this gate; the closure graph performs the complete disjoint-output merge
/// after certification. A primary-payload comparison here would reject valid
/// `out` + `dev` records before that merge can happen.
fn compare_registration_entries(
    left_roots: &Roots,
    left: &StoreEntry,
    right: &StoreEntry,
    left_producer: &ProducerRecord,
    right_producer: &ProducerRecord,
) -> io::Result<Option<FirstDifference>> {
    if left_producer.provider != "nix" || right_producer.provider != "nix" {
        return compare_entries(left_roots, left, left_roots, right);
    }

    if left.references != right.references {
        return Ok(Some(FirstDifference {
            path: "references".to_string(),
            kind: "references".to_string(),
            left: format!("{:?}", left.references),
            right: format!("{:?}", right.references),
        }));
    }
    for (kind, left_value, right_value) in [
        (
            "recipe-fingerprint",
            left.cache_identity.recipe_fingerprint.clone(),
            right.cache_identity.recipe_fingerprint.clone(),
        ),
        (
            "policy-fingerprint",
            left.cache_identity.policy_fingerprint.clone(),
            right.cache_identity.policy_fingerprint.clone(),
        ),
        (
            "platform",
            left.cache_identity.platform.clone(),
            right.cache_identity.platform.clone(),
        ),
        (
            "platform-artifact-kind",
            left.platform_artifact_kind.clone(),
            right.platform_artifact_kind.clone(),
        ),
    ] {
        if left_value != right_value {
            return Ok(Some(FirstDifference {
                path: ".".to_string(),
                kind: kind.to_string(),
                left: left_value,
                right: right_value,
            }));
        }
    }

    let left_outputs = nix_action_outputs(left, left_producer);
    let right_outputs = nix_action_outputs(right, right_producer);
    for name in left_outputs
        .keys()
        .chain(right_outputs.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
    {
        let (Some((left_digest, left_path)), Some((right_digest, right_path))) =
            (left_outputs.get(&name), right_outputs.get(&name))
        else {
            continue;
        };
        if left_digest != right_digest {
            return Ok(Some(FirstDifference {
                path: format!("output/{name}"),
                kind: "named-output".to_string(),
                left: left_digest.clone(),
                right: right_digest.clone(),
            }));
        }
        if left_path != right_path {
            return Ok(Some(FirstDifference {
                path: format!("output-path/{name}"),
                kind: "named-output-path".to_string(),
                left: left_path.clone(),
                right: right_path.clone(),
            }));
        }
    }

    let left_provenance = producer_provenance_facts(left_producer);
    let right_provenance = producer_provenance_facts(right_producer);
    if left_provenance != right_provenance {
        return Ok(Some(FirstDifference {
            path: ".".to_string(),
            kind: "producer-provenance".to_string(),
            left: json_map(&left_provenance),
            right: json_map(&right_provenance),
        }));
    }
    Ok(None)
}

fn nix_action_outputs(
    entry: &StoreEntry,
    producer: &ProducerRecord,
) -> BTreeMap<String, (String, String)> {
    producer
        .facts
        .iter()
        .filter_map(|(key, path)| {
            let name = key.strip_prefix("nix.output.")?;
            let digest = entry
                .named_outputs
                .get(name)
                .cloned()
                .or_else(|| (name == "out").then(|| entry.envelope.output_hash.clone()))?;
            Some((name.to_string(), (digest, path.clone())))
        })
        .collect()
}

/// A durable report blocks trusted cache use for the action until a fresh,
/// independently agreeing certification has replaced the action explicitly.
pub(crate) fn reproducibility_blocked(roots: &Roots, action_key: &str) -> io::Result<bool> {
    let Some(directory) = report_directory(roots, false)? else {
        return Ok(false);
    };
    let path = report_path(&directory, action_key);
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(invalid("reproducibility evidence is not a regular file"))
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Clear divergence evidence only after the complete promotion boundary has
/// succeeded. Keeping the report until then makes a killed worker or a later
/// publication failure remain fail-closed.
pub(crate) fn clear_reproducibility_report(roots: &Roots, action_key: &str) -> io::Result<()> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        remove_report(roots, action_key)
    })
}

fn remove_report(roots: &Roots, action_key: &str) -> io::Result<()> {
    let Some(directory) = report_directory(roots, false)? else {
        return Ok(());
    };
    let path = report_path(&directory, action_key);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid("reproducibility evidence is not a regular file"));
    }
    fs::remove_file(&path)?;
    super::sync_store_directory(&directory)?;
    Ok(())
}

fn compare_entries(
    left_roots: &Roots,
    left: &StoreEntry,
    right_roots: &Roots,
    right: &StoreEntry,
) -> io::Result<Option<FirstDifference>> {
    verify_existing_output(left_roots, left)?;
    verify_existing_output(right_roots, right)?;
    let left_snapshot = snapshot_for_entry(left_roots, left)?;
    let right_snapshot = snapshot_for_entry(right_roots, right)?;
    if left_snapshot.digest != right_snapshot.digest {
        return Ok(Some(
            first_node_difference(&left_snapshot, &right_snapshot).unwrap_or(FirstDifference {
                path: ".".to_string(),
                kind: "canonical-output".to_string(),
                left: left_snapshot.digest,
                right: right_snapshot.digest,
            }),
        ));
    }
    Ok(metadata_difference(left, right))
}

fn snapshot_for_entry(roots: &Roots, entry: &StoreEntry) -> io::Result<OutputSnapshot> {
    let path = Path::new(&entry.out);
    let hangar = roots.hangar_dir();
    let canonical_hangar = fs::canonicalize(&hangar).unwrap_or(hangar);
    let hangar_root = path
        .starts_with(&canonical_hangar)
        .then_some(canonical_hangar);
    snapshot_output(
        path,
        hangar_root.as_deref(),
        !entry.platform_artifact_kind.is_empty(),
    )
}

fn snapshot_output(
    path: &Path,
    hangar_root: Option<&Path>,
    allow_semantic_xattrs: bool,
) -> io::Result<OutputSnapshot> {
    let canonical = fs::canonicalize(path)?;
    let mut observed = Vec::new();
    let mut hook = |node: &Path, event: &'static str| {
        if event == "node" {
            observed.push(node.to_path_buf());
        }
    };
    let digest = match hangar_root {
        Some(hangar_root) => Envelope::try_output_hash_of_in_hangar_with_policy(
            &path.to_string_lossy(),
            hangar_root,
            allow_semantic_xattrs,
            &mut hook,
        ),
        None => Envelope::try_output_hash_of_with_policy(
            &path.to_string_lossy(),
            allow_semantic_xattrs,
            &mut hook,
        ),
    }
    .map_err(io::Error::other)?;

    let mut nodes = BTreeMap::new();
    let mut hardlink_first = BTreeMap::new();
    for node in observed {
        let relative = node
            .strip_prefix(&canonical)
            .map_err(|_| invalid("reproducibility observation escaped its output root"))?;
        let key = path_bytes(relative);
        let observation = observe_node(&node, &key, &mut hardlink_first)?;
        if nodes.insert(key, observation).is_some() {
            return Err(invalid("reproducibility observed one output node twice"));
        }
    }

    let confirmed = match hangar_root {
        Some(hangar_root) => Envelope::try_output_hash_of_in_hangar(
            &path.to_string_lossy(),
            hangar_root,
            allow_semantic_xattrs,
        ),
        None => Envelope::try_output_hash_of(&path.to_string_lossy()),
    }
    .map_err(io::Error::other)?;
    if confirmed != digest {
        return Err(invalid(
            "output changed while collecting reproducibility evidence",
        ));
    }
    Ok(OutputSnapshot { digest, nodes })
}

fn observe_node(
    path: &Path,
    relative: &[u8],
    hardlink_first: &mut BTreeMap<(u64, u64), Vec<u8>>,
) -> io::Result<NodeObservation> {
    let metadata = fs::symlink_metadata(path)?;
    let kind = metadata.file_type();
    let xattrs = Envelope::list_xattr_names(path)
        .map_err(io::Error::other)?
        .into_iter()
        .filter(|name| !Envelope::is_excluded_xattr(name))
        .collect::<Vec<_>>();
    let mode = mode_of(&metadata);
    if kind.is_dir() {
        return Ok(NodeObservation {
            kind: "directory",
            mode,
            digest: None,
            target: None,
            hardlink: None,
            xattrs,
        });
    }
    if kind.is_symlink() {
        let target = fs::read_link(path)?;
        return Ok(NodeObservation {
            kind: "symlink",
            mode,
            digest: None,
            target: Some(path_display(&path_bytes(&target))),
            hardlink: None,
            xattrs,
        });
    }
    if !kind.is_file() {
        return Err(invalid(
            "reproducibility observed an unsupported special file",
        ));
    }
    let hardlink = file_identity(&metadata).and_then(|identity| {
        hardlink_first
            .insert(identity, relative.to_vec())
            .map(|first| path_display(&first))
    });
    let digest = read_file_digest(path)?;
    Ok(NodeObservation {
        kind: "file",
        mode,
        digest: Some(digest),
        target: None,
        hardlink,
        xattrs,
    })
}

fn read_file_digest(path: &Path) -> io::Result<String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(Envelope::nofollow_open_flag().map_err(io::Error::other)?);
    }
    let mut file = options.open(path)?;
    let before = metadata_fingerprint(&file.metadata()?);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let after = metadata_fingerprint(&file.metadata()?);
    if before != after {
        return Err(invalid(
            "file changed while collecting reproducibility evidence",
        ));
    }
    Ok(format!("sha256-{}", SHA256::sha256_hex(&bytes)))
}

fn first_node_difference(left: &OutputSnapshot, right: &OutputSnapshot) -> Option<FirstDifference> {
    let paths = left
        .nodes
        .keys()
        .chain(right.nodes.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in paths {
        match (left.nodes.get(&path), right.nodes.get(&path)) {
            (Some(left), Some(right)) if left != right => {
                return Some(node_difference(&path, left, right));
            }
            (Some(left), None) => {
                return Some(FirstDifference {
                    path: path_display(&path),
                    kind: "missing-right".to_string(),
                    left: observation_value(left),
                    right: "<missing>".to_string(),
                });
            }
            (None, Some(right)) => {
                return Some(FirstDifference {
                    path: path_display(&path),
                    kind: "missing-left".to_string(),
                    left: "<missing>".to_string(),
                    right: observation_value(right),
                });
            }
            (Some(_), Some(_)) => {}
            (None, None) => {}
        }
    }
    None
}

fn node_difference(
    path: &[u8],
    left: &NodeObservation,
    right: &NodeObservation,
) -> FirstDifference {
    let kind = if left.kind != right.kind {
        "node-kind"
    } else if left.mode != right.mode {
        "mode"
    } else if left.digest != right.digest {
        "bytes"
    } else if left.target != right.target {
        "symlink-target"
    } else if left.hardlink != right.hardlink {
        "hardlink"
    } else {
        "xattrs"
    };
    FirstDifference {
        path: path_display(path),
        kind: kind.to_string(),
        left: observation_value(left),
        right: observation_value(right),
    }
}

fn metadata_difference(left: &StoreEntry, right: &StoreEntry) -> Option<FirstDifference> {
    for (kind, left, right) in [
        (
            "provenance",
            left.envelope.provenance.clone(),
            right.envelope.provenance.clone(),
        ),
        (
            "source-fingerprint",
            left.cache_identity.source_fingerprint.clone(),
            right.cache_identity.source_fingerprint.clone(),
        ),
        (
            "recipe-fingerprint",
            left.cache_identity.recipe_fingerprint.clone(),
            right.cache_identity.recipe_fingerprint.clone(),
        ),
        (
            "policy-fingerprint",
            left.cache_identity.policy_fingerprint.clone(),
            right.cache_identity.policy_fingerprint.clone(),
        ),
        (
            "platform",
            left.cache_identity.platform.clone(),
            right.cache_identity.platform.clone(),
        ),
        (
            "platform-artifact-kind",
            left.platform_artifact_kind.clone(),
            right.platform_artifact_kind.clone(),
        ),
    ] {
        if left != right {
            return Some(FirstDifference {
                path: ".".to_string(),
                kind: kind.to_string(),
                left,
                right,
            });
        }
    }
    let named_outputs = left
        .named_outputs
        .keys()
        .chain(right.named_outputs.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for name in named_outputs {
        let left_digest = left.named_outputs.get(&name).cloned();
        let right_digest = right.named_outputs.get(&name).cloned();
        if left_digest != right_digest {
            return Some(FirstDifference {
                path: format!("output/{name}"),
                kind: "named-output".to_string(),
                left: left_digest.unwrap_or_else(|| "<missing>".to_string()),
                right: right_digest.unwrap_or_else(|| "<missing>".to_string()),
            });
        }
    }
    let left_provenance = ProducerRecord::decode(&left.producer_record)
        .ok()
        .map(|producer| producer_provenance_facts(&producer));
    let right_provenance = ProducerRecord::decode(&right.producer_record)
        .ok()
        .map(|producer| producer_provenance_facts(&producer));
    if left_provenance != right_provenance {
        return Some(FirstDifference {
            path: ".".to_string(),
            kind: "producer-provenance".to_string(),
            left: json_map(&left_provenance.unwrap_or_default()),
            right: json_map(&right_provenance.unwrap_or_default()),
        });
    }
    None
}

fn producer_provenance_facts(producer: &ProducerRecord) -> BTreeMap<String, String> {
    let mut facts = BTreeMap::from([
        ("provider".to_string(), producer.provider.clone()),
        (
            "immutable_source".to_string(),
            producer.immutable_source.clone(),
        ),
        ("source_digest".to_string(), producer.source_digest.clone()),
        (
            "toolchain_facts".to_string(),
            producer.toolchain_facts.clone(),
        ),
        ("policy_facts".to_string(), producer.policy_facts.clone()),
    ]);
    for (key, value) in &producer.facts {
        if !is_action_ignored_fact(&producer.provider, key)
            && key != "provider-facts"
            && key != "provider-facts-digest"
        {
            facts.insert(format!("fact.{key}"), value.clone());
        }
    }
    for (key, value) in producer.plan.facts() {
        if !is_action_ignored_fact(&producer.provider, key) {
            facts.insert(format!("plan.{key}"), value.clone());
        }
    }
    facts
}

fn is_output_fact(key: &str) -> bool {
    key.starts_with("output.") || key.starts_with("nix.output.") || key == "cache.output"
}

fn is_action_ignored_fact(provider: &str, key: &str) -> bool {
    is_output_fact(key)
        || key == "cache.reproducibility"
        || (provider == "nix" && key == "nix.reference")
}

fn report_json(
    action_key: &str,
    left_action_key: &str,
    right_action_key: &str,
    left: &StoreEntry,
    right: &StoreEntry,
    left_producer: &ProducerRecord,
    right_producer: &ProducerRecord,
    difference: &FirstDifference,
) -> String {
    let mut fields = BTreeMap::new();
    fields.insert("action_key".to_string(), crate::JSON::quote(action_key));
    fields.insert(
        "first_difference".to_string(),
        json_object(BTreeMap::from([
            ("kind".to_string(), crate::JSON::quote(&difference.kind)),
            ("left".to_string(), crate::JSON::quote(&difference.left)),
            ("path".to_string(), crate::JSON::quote(&difference.path)),
            ("right".to_string(), crate::JSON::quote(&difference.right)),
        ])),
    );
    fields.insert(
        "left".to_string(),
        side_json(left, left_producer, left_action_key),
    );
    fields.insert(
        "producer_action".to_string(),
        json_object(BTreeMap::from([
            (
                "left_action_key".to_string(),
                crate::JSON::quote(left_action_key),
            ),
            (
                "left_provider".to_string(),
                crate::JSON::quote(&left_producer.provider),
            ),
            (
                "left_source_digest".to_string(),
                crate::JSON::quote(&left_producer.source_digest),
            ),
            (
                "left_source".to_string(),
                crate::JSON::quote(&left_producer.immutable_source),
            ),
            (
                "right_action_key".to_string(),
                crate::JSON::quote(right_action_key),
            ),
            (
                "right_provider".to_string(),
                crate::JSON::quote(&right_producer.provider),
            ),
            (
                "right_source_digest".to_string(),
                crate::JSON::quote(&right_producer.source_digest),
            ),
            (
                "right_source".to_string(),
                crate::JSON::quote(&right_producer.immutable_source),
            ),
        ])),
    );
    fields.insert(
        "right".to_string(),
        side_json(right, right_producer, right_action_key),
    );
    fields.insert("schema".to_string(), crate::JSON::quote(REPORT_SCHEMA));
    fields.insert("status".to_string(), crate::JSON::quote(REPORT_STATUS));
    format!("{}\n", json_object(fields))
}

fn side_json(entry: &StoreEntry, producer: &ProducerRecord, action_key: &str) -> String {
    json_object(BTreeMap::from([
        ("action_key".to_string(), crate::JSON::quote(action_key)),
        (
            "capabilities".to_string(),
            crate::JSON::quote(
                producer
                    .facts
                    .get("build.capabilities")
                    .map(String::as_str)
                    .unwrap_or(""),
            ),
        ),
        (
            "entry".to_string(),
            crate::JSON::quote(&report_entry_id(entry, action_key)),
        ),
        (
            "output_hash".to_string(),
            crate::JSON::quote(&entry.envelope.output_hash),
        ),
        (
            "platform".to_string(),
            crate::JSON::quote(&entry.envelope.platform),
        ),
        ("producer_facts".to_string(), json_map(&producer.facts)),
        (
            "provenance".to_string(),
            crate::JSON::quote(&entry.envelope.provenance),
        ),
        ("replay_facts".to_string(), json_map(producer.plan.facts())),
        (
            "source_fingerprint".to_string(),
            crate::JSON::quote(&entry.cache_identity.source_fingerprint),
        ),
        (
            "recipe_fingerprint".to_string(),
            crate::JSON::quote(&entry.cache_identity.recipe_fingerprint),
        ),
        (
            "policy_fingerprint".to_string(),
            crate::JSON::quote(&entry.cache_identity.policy_fingerprint),
        ),
    ]))
}

fn report_entry_id(entry: &StoreEntry, action_key: &str) -> String {
    let mut identity = b"jet-reproducibility-candidate-v1\0".to_vec();
    for value in [action_key, entry.envelope.output_hash.as_str()] {
        identity.extend_from_slice(&(value.len() as u64).to_be_bytes());
        identity.extend_from_slice(value.as_bytes());
    }
    format!("sha256-{}", SHA256::sha256_hex(&identity))
}

fn persist_report(roots: &Roots, action_key: &str, report: &str) -> io::Result<PathBuf> {
    let directory = report_directory(roots, true)?
        .ok_or_else(|| invalid("cannot create the private reproducibility evidence directory"))?;
    let destination = report_path(&directory, action_key);
    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid(
                "reproducibility evidence path is not a regular file",
            ));
        }
        return Err(unreproducible_error(
            action_key,
            "this action already has durable unreproducible evidence",
        ));
    }
    let sequence = REPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let partial = directory.join(format!(
        ".{}-partial-{}-{}",
        report_file_stem(action_key),
        std::process::id(),
        sequence
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
        file.write_all(report.as_bytes())?;
        file.sync_all()?;
        set_private_file_mode(&partial)?;
        fs::hard_link(&partial, &destination)?;
        fs::remove_file(&partial)?;
        super::sync_store_node(&directory, true)?;
        Ok::<(), io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result.map(|()| destination)
}

fn report_directory(roots: &Roots, create: bool) -> io::Result<Option<PathBuf>> {
    let root = &roots.root;
    let root_metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound && !create => return Ok(None),
        Err(error) => return Err(error),
    };
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(invalid("Jetpack root is not a real directory"));
    }
    let private = root.join("private");
    ensure_directory(&private, create)?;
    let Some(private) = private_if_present(&private, create)? else {
        return Ok(None);
    };
    let directory = private.join(REPORT_DIR);
    ensure_directory(&directory, create)?;
    let Some(directory) = private_if_present(&directory, create)? else {
        return Ok(None);
    };
    set_private_directory_mode(&directory)?;
    Ok(Some(directory))
}

fn ensure_directory(path: &Path, create: bool) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(invalid("reproducibility evidence directory is not real"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound && create => fs::create_dir(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn private_if_present(path: &Path, create: bool) -> io::Result<Option<PathBuf>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(invalid("reproducibility evidence directory is not real"))
        }
        Ok(_) => Ok(Some(path.to_path_buf())),
        Err(error) if error.kind() == io::ErrorKind::NotFound && !create => Ok(None),
        Err(error) => Err(error),
    }
}

fn report_path(directory: &Path, action_key: &str) -> PathBuf {
    directory.join(format!("{}.json", report_file_stem(action_key)))
}

fn report_file_stem(action_key: &str) -> String {
    let valid = action_key.len() == 71
        && action_key.starts_with("sha256-")
        && action_key[7..].bytes().all(|byte| byte.is_ascii_hexdigit());
    if valid {
        action_key.to_string()
    } else {
        format!("sha256-{}", SHA256::sha256_hex(action_key.as_bytes()))
    }
}

fn verify_existing_output(roots: &Roots, entry: &StoreEntry) -> io::Result<()> {
    let actual = Ingest::try_entry_output_hash(roots, entry).map_err(io::Error::other)?;
    if actual != entry.envelope.output_hash {
        return Err(invalid(&format!(
            "existing action output `{}` is corrupt: expected `{}`, got `{actual}`",
            entry.id, entry.envelope.output_hash
        )));
    }
    Ok(())
}

fn decode_producer(entry: &StoreEntry) -> io::Result<ProducerRecord> {
    ProducerRecord::decode(&entry.producer_record).map_err(|error| {
        invalid(&format!(
            "reproducibility action `{}` has invalid producer provenance: {error}",
            entry.id
        ))
    })
}

fn entry_sort_key(entry: &StoreEntry) -> (String, String) {
    (entry.envelope.output_hash.clone(), entry.id.clone())
}

fn unreproducible_error(action_key: &str, detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("unreproducible action `{action_key}`: {detail}"),
    )
}

fn invalid(detail: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, detail)
}

fn json_object(fields: BTreeMap<String, String>) -> String {
    let mut out = String::from("{");
    for (index, (key, value)) in fields.into_iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&crate::JSON::quote(&key));
        out.push(':');
        out.push_str(&value);
    }
    out.push('}');
    out
}

fn json_map(map: &BTreeMap<String, String>) -> String {
    json_object(
        map.iter()
            .map(|(key, value)| (key.clone(), crate::JSON::quote(value)))
            .collect(),
    )
}

fn observation_value(observation: &NodeObservation) -> String {
    format!(
        "kind={};mode={};digest={};target={};hardlink={};xattrs={}",
        observation.kind,
        observation.mode,
        observation.digest.as_deref().unwrap_or("<none>"),
        observation.target.as_deref().unwrap_or("<none>"),
        observation.hardlink.as_deref().unwrap_or("<none>"),
        observation.xattrs.join(",")
    )
}

fn path_display(path: &[u8]) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    String::from_utf8(path.to_vec()).unwrap_or_else(|_| format!("hex:{}", hex(path)))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}

fn path_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        return path.as_os_str().as_bytes().to_vec();
    }
    #[cfg(not(unix))]
    path.to_string_lossy().as_bytes().to_vec()
}

fn mode_of(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        return metadata.permissions().mode();
    }
    #[cfg(not(unix))]
    u32::from(metadata.permissions().readonly())
}

fn file_identity(metadata: &fs::Metadata) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        return (metadata.nlink() > 1).then(|| (metadata.dev(), metadata.ino()));
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

fn metadata_fingerprint(metadata: &fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        return format!(
            "{}:{}:{}:{}:{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.len(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec(),
            metadata.mode()
        );
    }
    #[cfg(not(unix))]
    format!(
        "{}:{}:{}",
        metadata.len(),
        metadata.permissions().readonly(),
        metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos())
    )
}

fn set_private_directory_mode(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let _ = path;
    Ok(())
}

fn set_private_file_mode(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_file_stem_is_path_safe_and_stable() {
        let action = format!("sha256-{}", "a".repeat(64));
        assert_eq!(report_file_stem(&action), action);
        assert!(report_file_stem("../action").starts_with("sha256-"));
    }
}
