//! Native standard Nix binary-cache admission.
//!
//! The cache wire format and Jet's local HMAC cache format are separate trust
//! protocols. This module verifies standard narinfo metadata, streams the
//! compressed NAR path, follows references, and publishes one Hangar batch.

use super::{
    entry_id, CacheIdentity, Closure, NixCompression, NixNarInfo, NixPublicKey, ProducerRecord,
    ProgressHandle, Roots, StoreEntry,
};
use crate::{Envelope, RuntimePolicy, SHA256};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
#[cfg(test)]
use std::io::Write;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_ENDPOINT: &str = "https://cache.nixos.org";
const DEFAULT_PUBLIC_KEY: &str = "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=";
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_PARALLEL_FETCHES: usize = 8;
const MAX_PARALLEL_METADATA_FETCHES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NixCacheErrorKind {
    Metadata,
    WrongKey,
    Signature,
    Transport,
    UnsupportedCompression,
    CompressedCorruption,
    NarCorruption,
    MissingReference,
    PathTraversal,
    DuplicateEntry,
    Admission,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NixCacheError {
    kind: NixCacheErrorKind,
    detail: String,
}

pub(crate) type StoreError = NixCacheError;

impl NixCacheError {
    fn new(kind: NixCacheErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    #[cfg(test)]
    pub(crate) fn kind(&self) -> NixCacheErrorKind {
        self.kind
    }

    pub(crate) fn code(&self) -> &'static str {
        "E1350"
    }

    pub(crate) fn what(&self) -> String {
        format!(
            "native Nix cache admission failed during {}: {}",
            self.stage(),
            self.detail
        )
    }

    pub(crate) fn why(&self) -> String {
        format!(
            "the signed Nix cache closure was not admitted because its {} check failed",
            self.stage()
        )
    }

    pub(crate) fn fix(&self) -> &'static str {
        "repair the cache metadata or network response, then retry the admission"
    }

    #[cfg(test)]
    pub(crate) fn diagnostic(&self) -> crate::Diagnostics::Diagnostic {
        crate::Diagnostics::Diagnostic::from_row(self.code(), &[("kind", self.stage())], None)
    }

    fn stage(&self) -> &'static str {
        match self.kind {
            NixCacheErrorKind::Metadata => "metadata validation",
            NixCacheErrorKind::WrongKey => "trusted-key selection",
            NixCacheErrorKind::Signature => "signature verification",
            NixCacheErrorKind::Transport => "native transport",
            NixCacheErrorKind::UnsupportedCompression => "compression selection",
            NixCacheErrorKind::CompressedCorruption => "compressed-byte verification",
            NixCacheErrorKind::NarCorruption => "NAR decoding",
            NixCacheErrorKind::MissingReference => "closure reference discovery",
            NixCacheErrorKind::PathTraversal => "path validation",
            NixCacheErrorKind::DuplicateEntry => "duplicate-entry validation",
            NixCacheErrorKind::Admission => "atomic Hangar admission",
        }
    }
}

impl std::fmt::Display for NixCacheError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.what())
    }
}

impl std::error::Error for NixCacheError {}

#[derive(Debug, Clone)]
pub(crate) struct NixOutputRequest {
    pub name: String,
    pub store_path: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AdmittedNixObject {
    pub store_path: String,
    pub hangar_path: PathBuf,
    pub hangar_digest: String,
    pub direct_reference_digests: Vec<String>,
    pub upstream_proof_sha256: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AdmittedNixClosure {
    pub outputs: BTreeMap<String, AdmittedNixObject>,
    pub objects: BTreeMap<String, AdmittedNixObject>,
    pub closure_receipt_sha256: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct NixDownloadPlan {
    pub packages: usize,
    pub bytes: u64,
}

#[allow(dead_code)]
pub(crate) fn admit_nix_closure(
    roots: &Roots,
    outputs: &[NixOutputRequest],
    offline: bool,
) -> Result<AdmittedNixClosure, StoreError> {
    admit_nix_closure_with_progress(roots, outputs, offline, None)
}

pub(crate) fn admit_nix_closure_with_progress(
    roots: &Roots,
    outputs: &[NixOutputRequest],
    offline: bool,
    progress: Option<ProgressHandle>,
) -> Result<AdmittedNixClosure, StoreError> {
    let mut transaction = AdmissionTransaction::new(roots)?;
    let result = transaction.admit(outputs, offline, progress);
    if result.is_err() {
        transaction.rollback();
    }
    result
}

/// Resolve and verify closure metadata without downloading or publishing any
/// NAR. Admission reuses this process-local signed metadata to open the whole
/// dependency frontier immediately; a rejected plan still acquires no payload.
pub(crate) fn plan_nix_downloads(
    roots: &Roots,
    store_paths: &[String],
    offline: bool,
    progress: Option<ProgressHandle>,
) -> Result<NixDownloadPlan, StoreError> {
    let endpoint = CacheEndpoint::from_roots(roots)?;
    let cache_info = if offline {
        None
    } else {
        Some(endpoint.cache_info()?)
    };
    let store_dir = cache_info
        .as_ref()
        .map(|info| info.store_dir.as_str())
        .unwrap_or("/nix/store");
    let mut queue = BTreeSet::new();
    for store_path in store_paths {
        validate_store_path(store_path, store_dir)?;
        queue.insert(store_path.clone());
    }
    let mut scheduled = queue.clone();
    let mut checked = 0usize;
    let mut packages = 0usize;
    let mut bytes = 0u64;
    let mut planned_infos = BTreeMap::new();
    if let Some(progress) = progress.as_ref() {
        progress.phase("Planning");
        progress.object_progress(0, scheduled.len());
    }

    while !queue.is_empty() {
        let wave = queue
            .iter()
            .take(MAX_PARALLEL_METADATA_FETCHES)
            .cloned()
            .collect::<Vec<_>>();
        for store_path in &wave {
            queue.remove(store_path);
        }

        let mut pending = Vec::new();
        for store_path in wave {
            if let Some(existing) = existing_object(roots, &store_path, store_dir)? {
                for reference in existing.references {
                    if scheduled.insert(reference.clone()) {
                        queue.insert(reference);
                    }
                }
                checked = checked.saturating_add(1);
                if let Some(progress) = progress.as_ref() {
                    progress.object_progress(checked, scheduled.len());
                }
            } else {
                pending.push(store_path);
            }
        }
        if pending.is_empty() {
            continue;
        }
        if offline {
            return Err(NixCacheError::new(
                NixCacheErrorKind::MissingReference,
                "offline admission found no complete verified Hangar object",
            ));
        }

        let results = std::thread::scope(|scope| {
            let handles = pending
                .iter()
                .map(|store_path| {
                    scope.spawn(|| {
                        endpoint
                            .narinfo(store_path, store_dir)?
                            .ok_or_else(|| {
                                NixCacheError::new(
                                    NixCacheErrorKind::MissingReference,
                                    "a requested Nix reference returned no narinfo",
                                )
                            })
                            .map(|info| (store_path.clone(), info))
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().unwrap_or_else(|_| {
                        Err(NixCacheError::new(
                            NixCacheErrorKind::Metadata,
                            "Nix cache worker panicked during closure planning",
                        ))
                    })
                })
                .collect::<Vec<_>>()
        });
        let mut results = results;
        results.sort_by(|left, right| match (left, right) {
            (Ok((left, _)), Ok((right, _))) => left.cmp(right),
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            (Err(_), Err(_)) => std::cmp::Ordering::Equal,
        });
        for result in results {
            let (store_path, info) = result?;
            planned_infos.insert(store_path, info.clone());
            let file_size = info.info.file_size.ok_or_else(|| {
                NixCacheError::new(
                    NixCacheErrorKind::Metadata,
                    "Nix narinfo has no signed FileSize for download planning",
                )
            })?;
            bytes = bytes.checked_add(file_size).ok_or_else(|| {
                NixCacheError::new(
                    NixCacheErrorKind::Metadata,
                    "Nix closure download size overflowed",
                )
            })?;
            packages = packages.checked_add(1).ok_or_else(|| {
                NixCacheError::new(
                    NixCacheErrorKind::Metadata,
                    "Nix closure package count overflowed",
                )
            })?;
            for reference in info.info.references {
                let reference = format!("{store_dir}/{reference}");
                validate_store_path(&reference, store_dir)?;
                if scheduled.insert(reference.clone()) {
                    queue.insert(reference);
                }
            }
            checked = checked.saturating_add(1);
            if let Some(progress) = progress.as_ref() {
                progress.object_progress(checked, scheduled.len());
            }
        }
    }
    remember_planned_narinfos(roots, &endpoint, planned_infos);

    Ok(NixDownloadPlan { packages, bytes })
}

/// Encode the producer's canonical zstd payload with stable single-threaded
/// settings. The zstd crate's default build has no `zstdmt` feature, so the
/// encoder remains single-threaded.
#[cfg(test)]
pub(crate) fn encode_zstd_deterministic(input: &[u8]) -> Result<Vec<u8>, NixCacheError> {
    let mut encoder = zstd::stream::Encoder::new(Vec::new(), 19)
        .map_err(|error| NixCacheError::new(NixCacheErrorKind::Admission, error.to_string()))?;
    encoder
        .include_contentsize(true)
        .and_then(|_| encoder.include_checksum(true))
        .and_then(|_| encoder.include_dictid(false))
        .and_then(|_| encoder.set_pledged_src_size(Some(input.len() as u64)))
        .map_err(|error| NixCacheError::new(NixCacheErrorKind::Admission, error.to_string()))?;
    encoder
        .write_all(input)
        .map_err(|error| NixCacheError::new(NixCacheErrorKind::Admission, error.to_string()))?;
    encoder
        .finish()
        .map_err(|error| NixCacheError::new(NixCacheErrorKind::Admission, error.to_string()))
}

struct AdmissionTransaction<'a> {
    roots: &'a Roots,
    stage: PathBuf,
    created_objects: Vec<PathBuf>,
    receipt: Option<PathBuf>,
    committed: bool,
}

impl<'a> AdmissionTransaction<'a> {
    fn new(roots: &'a Roots) -> Result<Self, NixCacheError> {
        ensure_dir(&roots.hangar_dir(), NixCacheErrorKind::Admission)?;
        let stage_parent = roots.hangar_dir().join("stage");
        ensure_dir(&stage_parent, NixCacheErrorKind::Admission)?;
        let stage = stage_parent.join(format!("nix-cache-{}", unique_suffix()));
        fs::create_dir(&stage).map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
        Ok(Self {
            roots,
            stage,
            created_objects: Vec::new(),
            receipt: None,
            committed: false,
        })
    }

    fn admit(
        &mut self,
        requested: &[NixOutputRequest],
        offline: bool,
        progress: Option<ProgressHandle>,
    ) -> Result<AdmittedNixClosure, NixCacheError> {
        let requests = validate_requests(requested)?;
        let endpoint = CacheEndpoint::from_roots(self.roots)?;
        let cache_info = if offline {
            None
        } else {
            Some(endpoint.cache_info()?)
        };
        let store_dir = cache_info
            .as_ref()
            .map(|info| info.store_dir.as_str())
            .unwrap_or("/nix/store");
        let mut queue = requests
            .values()
            .map(|request| request.store_path.clone())
            .collect::<BTreeSet<_>>();
        let mut scheduled = queue.clone();
        let planned_infos = planned_narinfos(self.roots, &endpoint);
        expand_planned_closure(&mut queue, &mut scheduled, &planned_infos, store_dir)?;
        let mut fetched = BTreeMap::new();
        if let Some(progress) = progress.as_ref() {
            progress.phase("Downloading");
            progress.object_progress(0, scheduled.len());
        }

        while !queue.is_empty() {
            let wave = queue
                .iter()
                .take(MAX_PARALLEL_FETCHES)
                .cloned()
                .collect::<Vec<_>>();
            for store_path in &wave {
                queue.remove(store_path);
            }

            let mut pending = Vec::new();
            for store_path in wave {
                if fetched.contains_key(&store_path) {
                    continue;
                }
                if let Some(existing) = existing_object(self.roots, &store_path, store_dir)? {
                    for reference in &existing.references {
                        if scheduled.insert(reference.clone()) {
                            queue.insert(reference.clone());
                        }
                    }
                    fetched.insert(store_path, existing);
                    if let Some(progress) = progress.as_ref() {
                        progress.object_progress(fetched.len(), scheduled.len());
                    }
                } else {
                    pending.push(store_path);
                }
            }
            if pending.is_empty() {
                continue;
            }
            if offline {
                return Err(NixCacheError::new(
                    NixCacheErrorKind::MissingReference,
                    "offline admission found no complete verified Hangar object",
                ));
            }

            if let Some(progress) = progress.as_ref() {
                progress.phase("Downloading");
            }
            let results = std::thread::scope(|scope| {
                let handles = pending
                    .iter()
                    .map(|store_path| {
                        scope.spawn(|| {
                            let info = match planned_infos.get(store_path) {
                                Some(info) => info.clone(),
                                None => {
                                    endpoint.narinfo(store_path, store_dir)?.ok_or_else(|| {
                                        NixCacheError::new(
                                            NixCacheErrorKind::MissingReference,
                                            "a requested Nix reference returned no narinfo",
                                        )
                                    })?
                                }
                            };
                            let object =
                                self.fetch_object(&endpoint, &info, store_dir, progress.clone())?;
                            Ok::<_, NixCacheError>((store_path.clone(), object))
                        })
                    })
                    .collect::<Vec<_>>();
                handles
                    .into_iter()
                    .map(|handle| {
                        handle.join().unwrap_or_else(|_| {
                            Err(NixCacheError::new(
                                NixCacheErrorKind::Admission,
                                "Nix cache worker panicked during closure fetch",
                            ))
                        })
                    })
                    .collect::<Vec<_>>()
            });
            let mut results = results;
            results.sort_by(|left, right| match (left, right) {
                (Ok((left, _)), Ok((right, _))) => left.cmp(right),
                (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                (Err(_), Err(_)) => std::cmp::Ordering::Equal,
            });
            for result in results {
                let (store_path, object) = result?;
                for reference in &object.references {
                    if scheduled.insert(reference.clone()) {
                        queue.insert(reference.clone());
                    }
                }
                fetched.insert(store_path, object);
                if let Some(progress) = progress.as_ref() {
                    progress.object_progress(fetched.len(), scheduled.len());
                }
            }
        }

        if let Some(progress) = progress.as_ref() {
            progress.phase("Installing");
            progress.object_progress(0, fetched.len());
        }
        let closure_receipt = self.publish(&mut fetched, &requests, store_dir, progress)?;
        let mut objects = BTreeMap::new();
        for (store_path, object) in &fetched {
            objects.insert(
                store_path.clone(),
                AdmittedNixObject {
                    store_path: store_path.clone(),
                    hangar_path: object.hangar_path.clone(),
                    hangar_digest: object.hangar_digest.clone(),
                    direct_reference_digests: object.direct_reference_digests.clone(),
                    upstream_proof_sha256: object.proof.clone(),
                },
            );
        }
        let mut selected = BTreeMap::new();
        for (name, request) in requests {
            let object = objects.get(&request.store_path).ok_or_else(|| {
                NixCacheError::new(
                    NixCacheErrorKind::Admission,
                    "selected output is missing from the admitted closure",
                )
            })?;
            selected.insert(name, object.clone());
        }
        self.committed = true;
        Ok(AdmittedNixClosure {
            outputs: selected,
            objects,
            closure_receipt_sha256: closure_receipt,
        })
    }

    fn fetch_object(
        &self,
        endpoint: &CacheEndpoint,
        info: &FetchedInfo,
        store_dir: &str,
        progress: Option<ProgressHandle>,
    ) -> Result<FetchedObject, NixCacheError> {
        if matches!(
            info.info.compression,
            NixCompression::Xz | NixCompression::Bzip2
        ) {
            return Err(NixCacheError::new(
                NixCacheErrorKind::UnsupportedCompression,
                "Nix cache compression is not enabled for this admission path",
            ));
        }
        let url = endpoint.object_url(&info.info.url)?;
        let response = jet_net::get_stream(&url, Duration::from_secs(120))
            .map_err(|error| NixCacheError::new(NixCacheErrorKind::Transport, error.to_string()))?;
        if response.status() == 404 || response.status() == 410 {
            return Err(NixCacheError::new(
                NixCacheErrorKind::MissingReference,
                "Nix cache object is absent",
            ));
        }
        if response.status() != 200 {
            return Err(NixCacheError::new(
                NixCacheErrorKind::Transport,
                "Nix cache object returned a non-success status",
            ));
        }
        let compressed_limit = match (info.info.file_size, response.content_length()) {
            (Some(expected), Some(actual)) if expected != actual => {
                return Err(NixCacheError::new(
                    NixCacheErrorKind::CompressedCorruption,
                    "Nix cache Content-Length disagrees with signed FileSize",
                ));
            }
            (Some(expected), _) => expected,
            (None, Some(actual)) => actual,
            (None, None) => {
                return Err(NixCacheError::new(
                    NixCacheErrorKind::CompressedCorruption,
                    "Nix cache object has neither FileSize nor Content-Length",
                ));
            }
        };
        if let Some(progress) = progress.as_ref() {
            progress.discovered_bytes(compressed_limit);
        }
        let tree = self
            .stage
            .join("trees")
            .join(store_basename(&info.info.store_path)?);
        ensure_dir(
            tree.parent().ok_or_else(|| {
                NixCacheError::new(
                    NixCacheErrorKind::Admission,
                    "Nix staging path has no parent",
                )
            })?,
            NixCacheErrorKind::Admission,
        )?;
        let mut body = HashingReader::new(response, compressed_limit, progress.clone());
        let stats = match info.info.compression {
            NixCompression::None => super::read_nar_stream(&mut body, &tree, info.info.nar_size),
            NixCompression::Zstd => {
                let mut decoder = zstd::stream::read::Decoder::new(&mut body).map_err(|error| {
                    NixCacheError::new(
                        NixCacheErrorKind::CompressedCorruption,
                        format!("could not initialize zstd decoder: {error}"),
                    )
                })?;
                let result = super::read_nar_stream(&mut decoder, &tree, info.info.nar_size);
                if result.is_ok() {
                    let mut trailing = [0u8; 64 * 1024];
                    if decoder.read(&mut trailing).map_err(|error| {
                        NixCacheError::new(
                            NixCacheErrorKind::CompressedCorruption,
                            format!("could not finish zstd decoder: {error}"),
                        )
                    })? != 0
                    {
                        return Err(NixCacheError::new(
                            NixCacheErrorKind::CompressedCorruption,
                            "zstd stream contains bytes after the canonical NAR",
                        ));
                    }
                }
                result
            }
            NixCompression::Xz | NixCompression::Bzip2 => unreachable!(),
        }
        .map_err(|error| {
            NixCacheError::new(
                classify_nar_error(&error),
                "Nix cache object is not a valid canonical NAR",
            )
        })?;
        if body.count != compressed_limit {
            return Err(NixCacheError::new(
                NixCacheErrorKind::CompressedCorruption,
                "Nix cache compressed byte count disagrees with its signed size",
            ));
        }
        let compressed_hash = format!("sha256:{}", bytes_to_hex(&body.hasher.finalize()));
        if let Some(expected) = &info.info.file_hash {
            if super::normalize_nar_hash(expected).map_err(|_| {
                NixCacheError::new(
                    NixCacheErrorKind::CompressedCorruption,
                    "Nix cache FileHash is invalid",
                )
            })? != super::normalize_nar_hash(&compressed_hash).map_err(|_| {
                NixCacheError::new(
                    NixCacheErrorKind::CompressedCorruption,
                    "Nix cache compressed hash is invalid",
                )
            })? {
                return Err(NixCacheError::new(
                    NixCacheErrorKind::CompressedCorruption,
                    "Nix cache compressed bytes do not match signed FileHash",
                ));
            }
        }
        if super::normalize_nar_hash(&stats.digest).map_err(|_| {
            NixCacheError::new(NixCacheErrorKind::NarCorruption, "Nix NAR hash is invalid")
        })? != super::normalize_nar_hash(&info.info.nar_hash).map_err(|_| {
            NixCacheError::new(NixCacheErrorKind::NarCorruption, "Nix NarHash is invalid")
        })? {
            return Err(NixCacheError::new(
                NixCacheErrorKind::NarCorruption,
                "Nix NAR bytes do not match signed NarHash",
            ));
        }
        super::seal_node(&tree).map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
        let hangar_digest = Envelope::try_output_hash_of_in_hangar(
            &tree.to_string_lossy(),
            &self.roots.hangar_dir(),
            false,
        )
        .map_err(|error| {
            NixCacheError::new(
                NixCacheErrorKind::NarCorruption,
                format!(
                    "Nix staged object {} could not be hashed: {error}",
                    info.info.store_path
                ),
            )
        })?;
        let mut references = info
            .info
            .references
            .iter()
            .map(|reference| format!("{store_dir}/{reference}"))
            .collect::<Vec<_>>();
        references.sort();
        let proof = proof_digest(
            endpoint.endpoint.as_str(),
            &info.info,
            &info.fingerprint,
            &info.key_id,
            &info.signature,
            &references,
        );
        Ok(FetchedObject {
            store_path: info.info.store_path.clone(),
            stage: Some(tree),
            hangar_path: PathBuf::new(),
            hangar_digest,
            references,
            direct_reference_digests: Vec::new(),
            unpacked_bytes: info.info.nar_size,
            proof,
        })
    }

    fn publish(
        &mut self,
        fetched: &mut BTreeMap<String, FetchedObject>,
        requests: &BTreeMap<String, NixOutputRequest>,
        store_dir: &str,
        progress: Option<ProgressHandle>,
    ) -> Result<String, NixCacheError> {
        let lock_root = self.roots.root.clone();
        Ok(RuntimePolicy::with_lock(&lock_root, "hangar", || {
            self.publish_locked(fetched, requests, store_dir, progress)
                .map_err(|error| std::io::Error::other(error.detail))
        })
        .map_err(|error| io_error(NixCacheErrorKind::Admission, error))?)
    }

    fn publish_locked(
        &mut self,
        fetched: &mut BTreeMap<String, FetchedObject>,
        requests: &BTreeMap<String, NixOutputRequest>,
        _store_dir: &str,
        progress: Option<ProgressHandle>,
    ) -> Result<String, NixCacheError> {
        let objects_dir = self.roots.hangar_dir().join("objects");
        ensure_dir(&objects_dir, NixCacheErrorKind::Admission)?;
        let mut incoming_bytes = 0u64;
        for object in fetched.values() {
            if object.stage.is_some() && !objects_dir.join(&object.hangar_digest).is_dir() {
                incoming_bytes = incoming_bytes
                    .checked_add(object.unpacked_bytes)
                    .ok_or_else(|| {
                        NixCacheError::new(
                            NixCacheErrorKind::Admission,
                            "Nix cache admission size overflowed",
                        )
                    })?;
            }
        }
        super::ensure_hangar_capacity(
            self.roots,
            super::admission_reservation(incoming_bytes),
            Some(&self.stage),
        )
        .map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
        let hangar_digests = fetched
            .iter()
            .map(|(store_path, object)| (store_path.clone(), object.hangar_digest.clone()))
            .collect::<BTreeMap<_, _>>();
        let total_objects = fetched.len();
        let mut admitted_objects = 0usize;
        for (store_path, object) in fetched.iter_mut() {
            let final_path = objects_dir.join(&object.hangar_digest);
            if let Some(stage) = &object.stage {
                if let Ok(metadata) = fs::symlink_metadata(&final_path) {
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return Err(NixCacheError::new(
                            NixCacheErrorKind::Admission,
                            "existing Hangar object is not a real directory",
                        ));
                    }
                    let actual = Envelope::try_output_hash_of_in_hangar(
                        &final_path.to_string_lossy(),
                        &self.roots.hangar_dir(),
                        false,
                    )
                    .map_err(|_| {
                        NixCacheError::new(
                            NixCacheErrorKind::Admission,
                            "existing Hangar object could not be verified",
                        )
                    })?;
                    if actual != object.hangar_digest {
                        return Err(NixCacheError::new(
                            NixCacheErrorKind::Admission,
                            "existing Hangar object conflicts with verified bytes",
                        ));
                    }
                    remove_tree(stage);
                } else {
                    make_tree_root_writable(stage)
                        .map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
                    fs::rename(stage, &final_path)
                        .map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
                    self.created_objects.push(final_path.clone());
                    super::seal_node(&final_path)
                        .map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
                }
            }
            object.hangar_path = final_path;
            let mut refs = Vec::new();
            for reference in &object.references {
                let target = hangar_digests.get(reference).ok_or_else(|| {
                    NixCacheError::new(
                        NixCacheErrorKind::MissingReference,
                        "Nix closure edge has no fetched target",
                    )
                })?;
                refs.push(target.clone());
            }
            refs.sort();
            object.direct_reference_digests = refs;
            let _ = store_path;
            admitted_objects += 1;
            if let Some(progress) = progress.as_ref() {
                progress.object_progress(admitted_objects, total_objects);
            }
        }
        if !self.created_objects.is_empty() {
            super::sync_store_directory(&objects_dir)
                .map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
        }

        let closure_receipt = closure_receipt_digest(fetched, requests);
        let receipt_path = self
            .roots
            .hangar_dir()
            .join("receipts")
            .join(&closure_receipt.0);
        let receipt_existed = fs::symlink_metadata(&receipt_path).is_ok();
        let receipt_path = self.write_closure_receipt(&closure_receipt.0, &closure_receipt.1)?;
        if !receipt_existed {
            self.receipt = Some(receipt_path);
        }

        let mut entries = Vec::new();
        let now = now_secs();
        for object in fetched.values() {
            let name = store_name(&object.store_path)?;
            let reference = format!("{name}@nixpkgs");
            let identity = CacheIdentity {
                source_fingerprint: object.proof.clone(),
                recipe_fingerprint: SHA256::sha256_hex(b"nix-binary-cache-substitution-v1"),
                policy_fingerprint: RuntimePolicy::cache_policy_fingerprint(false),
                platform: Envelope::host_platform(),
            };
            let mut facts = BTreeMap::from([
                ("nix.store-path".into(), object.store_path.clone()),
                ("nix.references".into(), object.references.join(",")),
                ("nix.closure.receipt".into(), closure_receipt.0.clone()),
                ("nix.proof".into(), object.proof.clone()),
            ]);
            facts.insert("nix.output.out".into(), object.store_path.clone());
            let producer_bytes = super::canonical_producer(
                "nix",
                &object.store_path,
                &object.proof,
                &identity,
                std::mem::take(&mut facts),
            )
            .map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
            let mut producer = ProducerRecord::decode(&producer_bytes).map_err(|error| {
                NixCacheError::new(NixCacheErrorKind::Admission, error.to_string())
            })?;
            producer.bind_cache_provenance(
                &reference,
                &object.hangar_digest,
                &identity,
                &object.direct_reference_digests,
            );
            let bin = object.hangar_path.join("bin");
            entries.push(StoreEntry {
                id: entry_id(
                    &name,
                    "",
                    &reference,
                    &object.hangar_path.display().to_string(),
                ),
                name,
                version: String::new(),
                reference,
                out: object.hangar_path.display().to_string(),
                bin: bin
                    .is_dir()
                    .then(|| bin.display().to_string())
                    .unwrap_or_default(),
                rlib: String::new(),
                envelope: Envelope::Envelope {
                    output_hash: object.hangar_digest.clone(),
                    platform: identity.platform.clone(),
                    signature: String::new(),
                    provenance: format!("{} via nix-cache", object.store_path),
                },
                cache_identity: identity,
                references: object.direct_reference_digests.clone(),
                named_outputs: BTreeMap::from([("out".into(), object.hangar_digest.clone())]),
                platform_artifact_kind: String::new(),
                producer_record: producer.encode(),
                receipt: String::new(),
                realized_at: now,
                last_used_at: now,
            });
        }
        Closure::register_entries_unlocked(self.roots, &entries)
            .map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
        Ok(closure_receipt.0)
    }

    fn write_closure_receipt(&self, digest: &str, bytes: &[u8]) -> Result<PathBuf, NixCacheError> {
        let receipts = self.roots.hangar_dir().join("receipts");
        ensure_dir(&receipts, NixCacheErrorKind::Admission)?;
        let path = receipts.join(digest);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(NixCacheError::new(
                    NixCacheErrorKind::Admission,
                    "closure receipt path is not a regular file",
                ));
            }
            let existing =
                fs::read(&path).map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
            if existing != bytes {
                return Err(NixCacheError::new(
                    NixCacheErrorKind::Admission,
                    "closure receipt digest is occupied by different bytes",
                ));
            }
            return Ok(path);
        }
        let partial = receipts.join(format!(".{digest}.partial-{}", unique_suffix()));
        fs::write(&partial, bytes)
            .map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
        fs::File::open(&partial)
            .and_then(|file| file.sync_all())
            .map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
        fs::rename(&partial, &path)
            .map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
        Ok(path)
    }

    fn rollback(&mut self) {
        if self.committed {
            return;
        }
        remove_tree(&self.stage);
        for path in self.created_objects.drain(..) {
            remove_tree(&path);
        }
        if let Some(path) = self.receipt.take() {
            let _ = fs::remove_file(path);
        }
    }
}

impl Drop for AdmissionTransaction<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.rollback();
        }
    }
}

#[derive(Debug, Clone)]
struct FetchedObject {
    store_path: String,
    stage: Option<PathBuf>,
    hangar_path: PathBuf,
    hangar_digest: String,
    references: Vec<String>,
    direct_reference_digests: Vec<String>,
    unpacked_bytes: u64,
    proof: String,
}

#[derive(Debug, Clone)]
struct FetchedInfo {
    info: NixNarInfo,
    fingerprint: Vec<u8>,
    key_id: String,
    signature: [u8; 64],
}

#[derive(Debug, Clone)]
struct CacheInfo {
    store_dir: String,
}

#[derive(Debug, Clone)]
struct CacheEndpoint {
    endpoint: String,
    trusted_keys: Vec<NixPublicKey>,
}

thread_local! {
    static PLANNED_NARINFOS: RefCell<
        BTreeMap<(PathBuf, String), Arc<BTreeMap<String, FetchedInfo>>>
    > = RefCell::new(BTreeMap::new());
}

fn planned_key(roots: &Roots, endpoint: &CacheEndpoint) -> (PathBuf, String) {
    (roots.root.clone(), endpoint.endpoint.clone())
}

fn remember_planned_narinfos(
    roots: &Roots,
    endpoint: &CacheEndpoint,
    infos: BTreeMap<String, FetchedInfo>,
) {
    PLANNED_NARINFOS.with(|planned| {
        planned
            .borrow_mut()
            .insert(planned_key(roots, endpoint), Arc::new(infos));
    });
}

fn planned_narinfos(roots: &Roots, endpoint: &CacheEndpoint) -> Arc<BTreeMap<String, FetchedInfo>> {
    PLANNED_NARINFOS.with(|planned| {
        planned
            .borrow()
            .get(&planned_key(roots, endpoint))
            .cloned()
            .unwrap_or_else(|| Arc::new(BTreeMap::new()))
    })
}

fn expand_planned_closure(
    queue: &mut BTreeSet<String>,
    scheduled: &mut BTreeSet<String>,
    infos: &BTreeMap<String, FetchedInfo>,
    store_dir: &str,
) -> Result<(), NixCacheError> {
    let mut frontier = queue.clone();
    while let Some(store_path) = frontier.iter().next().cloned() {
        frontier.remove(&store_path);
        let Some(info) = infos.get(&store_path) else {
            continue;
        };
        for reference in &info.info.references {
            let reference = format!("{store_dir}/{reference}");
            validate_store_path(&reference, store_dir)?;
            if scheduled.insert(reference.clone()) {
                queue.insert(reference.clone());
                frontier.insert(reference);
            }
        }
    }
    Ok(())
}

impl CacheEndpoint {
    fn from_roots(roots: &Roots) -> Result<Self, NixCacheError> {
        let endpoint_path = roots.root.join("config/nix-cache-v1.endpoint");
        let key_path = roots.root.join("trust/nix-cache-v1.ed25519.pub");
        let endpoint_present = fs::symlink_metadata(&endpoint_path).is_ok();
        let key_present = fs::symlink_metadata(&key_path).is_ok();
        if endpoint_present != key_present {
            return Err(NixCacheError::new(
                NixCacheErrorKind::Metadata,
                "Nix cache endpoint and trust key must be configured together",
            ));
        }
        let endpoint = if endpoint_present {
            let text = read_config(&endpoint_path)?;
            text.trim().to_string()
        } else {
            DEFAULT_ENDPOINT.to_string()
        };
        validate_endpoint(&endpoint)?;
        let trusted_keys = if key_present {
            vec![NixPublicKey::parse(&read_config(&key_path)?).map_err(|_| {
                NixCacheError::new(
                    NixCacheErrorKind::Metadata,
                    "Nix cache trust key is malformed",
                )
            })?]
        } else {
            vec![NixPublicKey::parse(DEFAULT_PUBLIC_KEY).map_err(|_| {
                NixCacheError::new(
                    NixCacheErrorKind::Metadata,
                    "embedded Nix cache trust key is malformed",
                )
            })?]
        };
        Ok(Self {
            endpoint,
            trusted_keys,
        })
    }

    fn cache_info(&self) -> Result<CacheInfo, NixCacheError> {
        let url = self.url("nix-cache-info")?;
        let bytes = self.get(&url, MAX_METADATA_BYTES)?.ok_or_else(|| {
            NixCacheError::new(
                NixCacheErrorKind::Transport,
                "Nix cache-info endpoint is absent",
            )
        })?;
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            NixCacheError::new(NixCacheErrorKind::Metadata, "Nix cache-info is not UTF-8")
        })?;
        let mut store_dir = None;
        for line in text.lines() {
            let Some((key, value)) = line.split_once(": ") else {
                return Err(NixCacheError::new(
                    NixCacheErrorKind::Metadata,
                    "Nix cache-info has a malformed line",
                ));
            };
            if key == "StoreDir" {
                if store_dir.replace(value.to_string()).is_some() {
                    return Err(NixCacheError::new(
                        NixCacheErrorKind::Metadata,
                        "Nix cache-info repeats StoreDir",
                    ));
                }
            }
        }
        let store_dir = store_dir.ok_or_else(|| {
            NixCacheError::new(
                NixCacheErrorKind::Metadata,
                "Nix cache-info has no StoreDir",
            )
        })?;
        validate_store_dir(&store_dir)?;
        Ok(CacheInfo { store_dir })
    }

    fn narinfo(
        &self,
        store_path: &str,
        store_dir: &str,
    ) -> Result<Option<FetchedInfo>, NixCacheError> {
        validate_store_path(store_path, store_dir)?;
        let basename = store_basename(store_path)?;
        let key = &basename[..32];
        let url = self.url(&format!("{key}.narinfo"))?;
        let Some(bytes) = self.get(&url, MAX_METADATA_BYTES)? else {
            return Ok(None);
        };
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            NixCacheError::new(NixCacheErrorKind::Metadata, "Nix narinfo is not UTF-8")
        })?;
        let info = NixNarInfo::parse(text).map_err(|error| {
            NixCacheError::new(classify_narinfo_error(&error), "Nix narinfo is malformed")
        })?;
        if info.store_path != store_path {
            return Err(NixCacheError::new(
                NixCacheErrorKind::Signature,
                "Nix narinfo StorePath disagrees with the requested path",
            ));
        }
        let fingerprint = info.fingerprint(store_dir).map_err(|_| {
            NixCacheError::new(
                NixCacheErrorKind::Signature,
                "Nix narinfo fingerprint is invalid",
            )
        })?;
        let key_matches = info.signatures.iter().any(|signature| {
            self.trusted_keys
                .iter()
                .any(|key| key.key_id == signature.key_id)
        });
        if !key_matches {
            return Err(NixCacheError::new(
                NixCacheErrorKind::WrongKey,
                "Nix narinfo has no signature under a configured key name",
            ));
        }
        let (key_id, signature) = info
            .verified_signature(store_dir, &self.trusted_keys)
            .map_err(|_| {
                NixCacheError::new(
                    NixCacheErrorKind::Signature,
                    "Nix narinfo signature does not verify",
                )
            })?;
        Ok(Some(FetchedInfo {
            info,
            fingerprint,
            key_id,
            signature,
        }))
    }

    fn object_url(&self, relative: &str) -> Result<String, NixCacheError> {
        validate_relative_endpoint_url(relative)?;
        Ok(format!(
            "{}/{}",
            self.endpoint.trim_end_matches('/'),
            relative
        ))
    }

    fn url(&self, relative: &str) -> Result<String, NixCacheError> {
        self.object_url(relative)
    }

    fn get(&self, url: &str, limit: u64) -> Result<Option<Vec<u8>>, NixCacheError> {
        let response = jet_net::get_stream(url, Duration::from_secs(120))
            .map_err(|error| NixCacheError::new(NixCacheErrorKind::Transport, error.to_string()))?;
        if response.status() == 404 || response.status() == 410 {
            return Ok(None);
        }
        if response.status() != 200 {
            return Err(NixCacheError::new(
                NixCacheErrorKind::Transport,
                "Nix cache returned a non-success status",
            ));
        }
        let mut bytes = Vec::new();
        response
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| NixCacheError::new(NixCacheErrorKind::Transport, error.to_string()))?;
        if bytes.len() as u64 > limit {
            return Err(NixCacheError::new(
                NixCacheErrorKind::Metadata,
                "Nix cache response exceeds its bound",
            ));
        }
        Ok(Some(bytes))
    }
}

struct HashingReader<R> {
    reader: R,
    limit: u64,
    count: u64,
    hasher: SHA256::StreamingSha256,
    progress: Option<ProgressHandle>,
}

impl<R: Read> HashingReader<R> {
    fn new(reader: R, limit: u64, progress: Option<ProgressHandle>) -> Self {
        Self {
            reader,
            limit,
            count: 0,
            hasher: SHA256::StreamingSha256::new(),
            progress,
        }
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.count >= self.limit {
            let mut extra = [0u8; 1];
            return match self.reader.read(&mut extra)? {
                0 => Ok(0),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Nix cache response exceeded its signed byte limit",
                )),
            };
        }
        let remaining = (self.limit - self.count).min(buffer.len() as u64) as usize;
        let count = self.reader.read(&mut buffer[..remaining])?;
        if count != 0 {
            self.count = self.count.checked_add(count as u64).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Nix cache byte count overflow")
            })?;
            self.hasher.update(&buffer[..count]);
            if let Some(progress) = self.progress.as_ref() {
                progress.transferred_bytes(count as u64);
            }
        }
        Ok(count)
    }
}

fn validate_requests(
    requested: &[NixOutputRequest],
) -> Result<BTreeMap<String, NixOutputRequest>, NixCacheError> {
    if requested.is_empty() {
        return Err(NixCacheError::new(
            NixCacheErrorKind::DuplicateEntry,
            "Nix closure admission has no outputs",
        ));
    }
    let mut result = BTreeMap::new();
    for request in requested {
        if request.name.is_empty()
            || request.name.contains('/')
            || request.name == "."
            || request.name == ".."
        {
            return Err(NixCacheError::new(
                NixCacheErrorKind::PathTraversal,
                "Nix output name is not one safe component",
            ));
        }
        validate_store_path(&request.store_path, "/nix/store")?;
        if result
            .insert(request.name.clone(), request.clone())
            .is_some()
        {
            return Err(NixCacheError::new(
                NixCacheErrorKind::DuplicateEntry,
                "Nix closure admission repeats an output name",
            ));
        }
    }
    Ok(result)
}

fn existing_object(
    roots: &Roots,
    store_path: &str,
    store_dir: &str,
) -> Result<Option<FetchedObject>, NixCacheError> {
    let entries = super::list_checked(roots)
        .map_err(|error| io_error(NixCacheErrorKind::Admission, error))?;
    let Some(entry) = entries.into_iter().find(|entry| {
        ProducerRecord::decode(&entry.producer_record)
            .ok()
            .and_then(|producer| producer.facts.get("nix.store-path").cloned())
            .as_deref()
            == Some(store_path)
    }) else {
        return Ok(None);
    };
    let output = Path::new(&entry.out);
    let output_meta = match fs::symlink_metadata(output) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => metadata,
        _ => return Ok(None),
    };
    if !output.starts_with(roots.hangar_dir()) || !output_meta.is_dir() {
        return Ok(None);
    }
    let digest = Envelope::try_output_hash_of_in_hangar(&entry.out, &roots.hangar_dir(), false)
        .map_err(|_| {
            NixCacheError::new(
                NixCacheErrorKind::Admission,
                "existing Nix object failed re-hash",
            )
        })?;
    if digest != entry.envelope.output_hash {
        return Ok(None);
    }
    let producer = ProducerRecord::decode(&entry.producer_record)
        .map_err(|error| NixCacheError::new(NixCacheErrorKind::Admission, error.to_string()))?;
    let references: Vec<String> = producer
        .facts
        .get("nix.references")
        .map(|value| {
            value
                .split(',')
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if producer.facts.get("nix.proof").is_none_or(String::is_empty)
        || references
            .iter()
            .any(|reference| validate_store_path(reference, store_dir).is_err())
        || references.len() != entry.references.len()
    {
        return Ok(None);
    }
    Ok(Some(FetchedObject {
        store_path: store_path.to_string(),
        stage: None,
        hangar_path: output.to_path_buf(),
        hangar_digest: digest,
        references,
        direct_reference_digests: entry.references,
        unpacked_bytes: 0,
        proof: producer.facts.get("nix.proof").cloned().unwrap_or_default(),
    }))
}

fn closure_receipt_digest(
    objects: &BTreeMap<String, FetchedObject>,
    outputs: &BTreeMap<String, NixOutputRequest>,
) -> (String, Vec<u8>) {
    let mut bytes = b"nix-cache-closure-v1\n".to_vec();
    push_u64(&mut bytes, outputs.len() as u64);
    for (name, request) in outputs {
        push_field(&mut bytes, name);
        push_field(&mut bytes, &request.store_path);
        push_field(&mut bytes, &objects[&request.store_path].hangar_digest);
    }
    push_u64(&mut bytes, objects.len() as u64);
    for (store_path, object) in objects {
        push_field(&mut bytes, store_path);
        push_field(&mut bytes, &object.hangar_digest);
        push_field(&mut bytes, &object.proof);
    }
    let edge_count = objects
        .values()
        .map(|object| object.references.len())
        .sum::<usize>();
    push_u64(&mut bytes, edge_count as u64);
    for (from, object) in objects {
        for (index, to) in object.references.iter().enumerate() {
            push_field(&mut bytes, from);
            push_field(&mut bytes, to);
            push_field(&mut bytes, &object.direct_reference_digests[index]);
        }
    }
    (format!("sha256-{}", SHA256::sha256_hex(&bytes)), bytes)
}

fn proof_digest(
    endpoint: &str,
    info: &NixNarInfo,
    fingerprint: &[u8],
    key_id: &str,
    signature: &[u8; 64],
    references: &[String],
) -> String {
    let mut bytes = b"nix-narinfo-proof-v1\n".to_vec();
    for value in [
        endpoint.as_bytes(),
        info.store_path.as_bytes(),
        fingerprint,
        key_id.as_bytes(),
        signature,
        info.url.as_bytes(),
        info.compression.as_str().as_bytes(),
    ] {
        push_bytes(&mut bytes, value);
    }
    match &info.file_hash {
        Some(value) => {
            bytes.push(1);
            push_field(&mut bytes, value);
        }
        None => bytes.push(0),
    }
    match info.file_size {
        Some(value) => {
            bytes.push(1);
            push_u64(&mut bytes, value);
        }
        None => bytes.push(0),
    }
    push_field(&mut bytes, &info.nar_hash);
    push_u64(&mut bytes, info.nar_size);
    for reference in references {
        push_field(&mut bytes, reference);
    }
    format!("sha256-{}", SHA256::sha256_hex(&bytes))
}

fn push_field(output: &mut Vec<u8>, value: &str) {
    push_bytes(output, value.as_bytes());
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value);
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn validate_endpoint(endpoint: &str) -> Result<(), NixCacheError> {
    if endpoint.is_empty()
        || endpoint
            .bytes()
            .any(|byte| matches!(byte, b'?' | b'#' | b'@' | b'\\'))
        || endpoint.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return Err(NixCacheError::new(
            NixCacheErrorKind::PathTraversal,
            "Nix cache endpoint is malformed",
        ));
    }
    let (scheme, rest) = endpoint.split_once("://").ok_or_else(|| {
        NixCacheError::new(
            NixCacheErrorKind::PathTraversal,
            "Nix cache endpoint has no supported scheme",
        )
    })?;
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err(NixCacheError::new(
            NixCacheErrorKind::PathTraversal,
            "Nix cache endpoint has no host",
        ));
    }
    let path = &rest[authority_end..];
    if path
        .split('/')
        .any(|component| matches!(component, "." | ".."))
        || path.contains("//")
        || path.bytes().any(|byte| byte == b'%')
    {
        return Err(NixCacheError::new(
            NixCacheErrorKind::PathTraversal,
            "Nix cache endpoint path is unsafe",
        ));
    }
    if scheme == "http" {
        let host = authority
            .strip_prefix('[')
            .and_then(|value| value.split(']').next())
            .unwrap_or_else(|| authority.split(':').next().unwrap_or(authority));
        if matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1") {
            return Ok(());
        }
    }
    if scheme == "https" {
        return Ok(());
    }
    Err(NixCacheError::new(
        NixCacheErrorKind::PathTraversal,
        "Nix cache endpoint must be HTTPS or loopback HTTP",
    ))
}

fn validate_relative_endpoint_url(relative: &str) -> Result<(), NixCacheError> {
    let (path, query) = relative.split_once('?').unwrap_or((relative, ""));
    if path.is_empty()
        || path.starts_with('/')
        || path.contains("//")
        || path.contains('\\')
        || path
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control() || byte == b'%' || byte == b'#')
        || path
            .split('/')
            .any(|component| matches!(component, "." | ".."))
        || query.len() > MAX_METADATA_BYTES as usize
        || query
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control() || byte == b'#')
    {
        return Err(NixCacheError::new(
            NixCacheErrorKind::PathTraversal,
            "Nix narinfo URL is not relative to the cache endpoint",
        ));
    }
    Ok(())
}

fn validate_store_dir(value: &str) -> Result<(), NixCacheError> {
    let path = Path::new(value);
    if value.is_empty()
        || !path.is_absolute()
        || value.ends_with('/')
        || value.contains('\\')
        || path.components().any(|component| {
            matches!(
                component,
                Component::CurDir | Component::ParentDir | Component::Prefix(_)
            )
        })
    {
        return Err(NixCacheError::new(
            NixCacheErrorKind::PathTraversal,
            "Nix StoreDir is unsafe",
        ));
    }
    Ok(())
}

fn validate_store_path(value: &str, store_dir: &str) -> Result<(), NixCacheError> {
    let prefix = format!("{store_dir}/");
    let basename = value.strip_prefix(&prefix).ok_or_else(|| {
        NixCacheError::new(
            NixCacheErrorKind::PathTraversal,
            "Nix StorePath is outside StoreDir",
        )
    })?;
    if basename.contains('/')
        || basename.len() < 34
        || basename.as_bytes().get(32) != Some(&b'-')
        || basename[33..].is_empty()
        || basename
            .bytes()
            .any(|byte| byte == b'\\' || byte == 0 || byte.is_ascii_control())
    {
        return Err(NixCacheError::new(
            NixCacheErrorKind::PathTraversal,
            "Nix StorePath is not one direct safe child",
        ));
    }
    if !basename[..32]
        .bytes()
        .all(|byte| b"0123456789abcdfghijklmnpqrsvwxyz".contains(&byte))
    {
        return Err(NixCacheError::new(
            NixCacheErrorKind::PathTraversal,
            "Nix StorePath hash is malformed",
        ));
    }
    Ok(())
}

fn store_basename(value: &str) -> Result<String, NixCacheError> {
    let basename = value.rsplit('/').next().ok_or_else(|| {
        NixCacheError::new(
            NixCacheErrorKind::PathTraversal,
            "Nix StorePath has no basename",
        )
    })?;
    if basename.len() < 34 || basename.as_bytes().get(32) != Some(&b'-') {
        return Err(NixCacheError::new(
            NixCacheErrorKind::PathTraversal,
            "Nix StorePath basename is malformed",
        ));
    }
    Ok(basename.to_string())
}

fn store_name(value: &str) -> Result<String, NixCacheError> {
    Ok(store_basename(value)?[33..].to_string())
}

fn read_config(path: &Path) -> Result<String, NixCacheError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| io_error(NixCacheErrorKind::Metadata, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 4096 {
        return Err(NixCacheError::new(
            NixCacheErrorKind::Metadata,
            "Nix cache configuration is not a bounded regular file",
        ));
    }
    fs::read_to_string(path)
        .map_err(|error| io_error(NixCacheErrorKind::Metadata, error))
        .and_then(|value| {
            if value.lines().count() != 1 || value.trim().is_empty() {
                Err(NixCacheError::new(
                    NixCacheErrorKind::Metadata,
                    "Nix cache configuration has multiple or empty lines",
                ))
            } else {
                Ok(value.trim().to_string())
            }
        })
}

fn ensure_dir(path: &Path, kind: NixCacheErrorKind) -> Result<(), NixCacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            NixCacheError::new(kind, "Nix cache directory is not a real directory"),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|error| io_error(kind, error))
        }
        Err(error) => Err(io_error(kind, error)),
    }
}

fn io_error(kind: NixCacheErrorKind, error: io::Error) -> NixCacheError {
    NixCacheError::new(kind, error.to_string())
}

fn classify_narinfo_error(error: &io::Error) -> NixCacheErrorKind {
    let detail = error.to_string();
    if detail.contains("duplicate") {
        NixCacheErrorKind::DuplicateEntry
    } else if detail.contains("URL")
        || detail.contains("StorePath")
        || detail.contains("store path")
        || detail.contains("store reference")
    {
        NixCacheErrorKind::PathTraversal
    } else if detail.contains("compression") {
        NixCacheErrorKind::UnsupportedCompression
    } else {
        NixCacheErrorKind::Metadata
    }
}

fn classify_nar_error(error: &io::Error) -> NixCacheErrorKind {
    let detail = error.to_string().to_ascii_lowercase();
    if detail.contains("duplicate") {
        NixCacheErrorKind::DuplicateEntry
    } else if detail.contains("signed byte limit")
        || detail.contains("compressed")
        || detail.contains("zstd")
        || detail.contains("checksum")
        || detail.contains("frame")
        || detail.contains("data corruption")
    {
        NixCacheErrorKind::CompressedCorruption
    } else if detail.contains("path") || detail.contains("name") || detail.contains("symlink") {
        NixCacheErrorKind::PathTraversal
    } else {
        NixCacheErrorKind::NarCorruption
    }
}

fn remove_tree(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        let _ = fs::remove_file(path);
    } else if metadata.is_dir() {
        let _ = super::make_tree_writable_for_removal(path);
        let _ = fs::remove_dir_all(path);
    }
}

fn make_tree_root_writable(path: &Path) -> io::Result<()> {
    let mut permissions = fs::symlink_metadata(path)?.permissions();
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
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}", std::process::id())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod Tests;
