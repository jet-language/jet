use super::*;
use std::cell::Cell;

pub fn find_by_reference(roots: &Roots, reference: &str) -> Option<StoreEntry> {
    list(roots)
        .into_iter()
        .filter(|e| e.reference == reference)
        .max_by_key(|e| e.last_used_at)
}

/// Return a cache candidate without taking the checked Store path. Prompt
/// planning may inspect identity metadata, but must not replay or migrate the
/// closure graph; `realize_verified` remains the integrity authority.
pub(crate) fn find_by_reference_read_only(roots: &Roots, reference: &str) -> Option<StoreEntry> {
    list_read_only(roots)
        .into_iter()
        .filter(|e| e.reference == reference)
        .max_by_key(|e| e.last_used_at)
}

/// Cheap provider-routing preflight for an exact cache identity. This is only
/// a candidate check: `realize_verified` remains the authority that validates
/// the output and its complete closure before reuse.
pub(crate) fn cache_candidate_matches(
    roots: &Roots,
    reference: &str,
    expectation: &CacheExpectation,
) -> bool {
    find_by_reference_read_only(roots, reference)
        .is_some_and(|entry| entry.cache_identity == expectation.identity)
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
    let graph = Closure::closure_graph_structure(roots).ok();
    verify_cache_entry_with_graph(
        roots,
        entry,
        expected_reference,
        expectation,
        graph.as_ref(),
    )
}

pub(crate) fn verify_cache_entry_with_graph(
    roots: &Roots,
    entry: &StoreEntry,
    expected_reference: &str,
    expectation: &CacheExpectation,
    graph: Option<&Closure::ClosureGraph>,
) -> CacheVerification {
    let out = Path::new(&entry.out);
    // Admitted objects are directory, regular file, or symlink roots.
    let output_exists = fs::symlink_metadata(out).is_ok();
    let output_digest = output_exists
        && !entry.envelope.output_hash.is_empty()
        && Ingest::try_entry_output_hash(roots, entry)
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
        && entry.cache_identity.policy_fingerprint == expectation.identity.policy_fingerprint
        && producer_authority_verified(roots, entry, expectation);
    let signature_verified = !entry.envelope.signature.is_empty()
        && verify_configured_signature(roots, entry, expectation);
    let canonical_local = Path::new(&entry.out)
        == roots
            .hangar_dir()
            .join(OBJECTS_DIR)
            .join(&entry.envelope.output_hash);
    let unsigned_local_allowed = entry.envelope.signature.is_empty()
        && expectation.allow_unsigned_local
        && (canonical_local
            || expectation
                .owned_output
                .as_ref()
                .is_some_and(|path| path == Path::new(&entry.out)));
    let closure = output_exists
        && closure_is_reachable(roots, entry)
        && graph.is_some_and(|graph| Closure::entry_closure_store_proof(roots, graph, entry));
    let verification = CacheVerification {
        output_exists,
        output_digest,
        source,
        recipe,
        platform,
        policy,
        signature_verified,
        unsigned_local_allowed,
        closure,
    };
    if !verification.trusted() && std::env::var_os("JETPACK_VERIFY_TRACE").is_some() {
        eprintln!(
            "VERIFY-TRACE ref={} id={} legs={verification:?} hash_probe={:?}",
            entry.reference,
            entry.id,
            Ingest::try_entry_output_hash(roots, entry)
        );
    }
    verification
}

fn producer_authority_verified(
    roots: &Roots,
    entry: &StoreEntry,
    expectation: &CacheExpectation,
) -> bool {
    if reproducibility_blocked(roots, &entry_action_key(entry)).unwrap_or(true) {
        return false;
    }
    let Ok(producer) = ProducerRecord::decode(&entry.producer_record) else {
        return false;
    };
    if producer.provider.trim().is_empty()
        || producer.immutable_source.trim().is_empty()
        || producer.source_digest.trim().is_empty()
        || producer.facts.get("action.recipe").map(String::as_str)
            != Some(expectation.identity.recipe_fingerprint.as_str())
        || producer.policy_facts
            != format!(
                "policy={}\nplatform={}",
                expectation.identity.policy_fingerprint, expectation.identity.platform
            )
    {
        return false;
    }
    let Some(attestation) = producer.facts.get("cache.reproducibility") else {
        return false;
    };
    if !(attestation == "attested-v1" || attestation.starts_with("independent-agreeing-v1:")) {
        return false;
    }
    let provenance = crate::TrustRoot::CacheProvenance {
        reference: producer
            .facts
            .get("cache.reference")
            .cloned()
            .unwrap_or_default(),
        source: producer
            .facts
            .get("cache.source")
            .cloned()
            .unwrap_or_default(),
        builder: producer
            .facts
            .get("cache.builder")
            .cloned()
            .unwrap_or_default(),
        action: producer
            .facts
            .get("cache.action")
            .cloned()
            .unwrap_or_default(),
        output: producer
            .facts
            .get("cache.output")
            .cloned()
            .unwrap_or_default(),
        platform: producer
            .facts
            .get("cache.platform")
            .cloned()
            .unwrap_or_default(),
        sandbox: producer
            .facts
            .get("cache.sandbox")
            .cloned()
            .unwrap_or_default(),
        policy: producer
            .facts
            .get("cache.policy")
            .cloned()
            .unwrap_or_default(),
    };
    let builder = cache_builder_identity(
        &producer.provider,
        &producer.immutable_source,
        &producer.source_digest,
    );
    provenance.validate().is_ok()
        && provenance.reference == entry.reference
        && provenance.source == producer.immutable_source
        && provenance.builder == builder
        && provenance.action
            == cache_action_identity(
                &producer,
                &entry.reference,
                &entry.cache_identity,
                &entry.references,
            )
        && provenance.output == entry.envelope.output_hash
        && provenance.platform == expectation.identity.platform
        && provenance.sandbox == "sandbox:policy-bound"
        && provenance.policy == expectation.identity.policy_fingerprint
        && !is_cache_builder_revoked(&roots.root, &builder).unwrap_or(true)
}

pub(crate) fn enforce_manifest_provenance_floor(
    project: Option<&Path>,
    package: &str,
) -> Result<(), RealizeError> {
    let Some(project) = project else {
        return Ok(());
    };
    let Some(manifest) = crate::Package::PackageFacts::load(project) else {
        return Ok(());
    };
    let manifest = match manifest {
        Ok(manifest) => manifest,
        Err(error) => {
            return Err(RealizeError::Store(std::io::Error::other(format!(
                "package trust manifest could not be loaded: {error:?}"
            ))));
        }
    };
    let Some(requirement) = manifest
        .authority
        .trust
        .as_ref()
        .and_then(|trust| trust.require)
    else {
        return Ok(());
    };
    let Some(lock) = crate::Lock::load(project) else {
        return Err(RealizeError::Integrity(IntegrityFailure {
            package: package.to_string(),
            version: String::new(),
            expected: format!("{} provenance", requirement.label()),
            actual: format!("{} is missing", lock_path(project).display()),
            reason: format!("the package trust policy requires {} provenance", requirement.label()),
            disposition: "Jetpack rejected the package before provider, cache, or remote bytes became usable."
                .to_string(),
            fix: "Refresh `.jet/lock` with the required provenance, then retry.".to_string(),
        }));
    };
    if let Err(detail) = crate::Lock::enforce_provenance_requirement(&lock, requirement) {
        return Err(RealizeError::Integrity(IntegrityFailure {
            package: package.to_string(),
            version: String::new(),
            expected: format!("{} provenance", requirement.label()),
            actual: detail,
            reason: format!("the package trust policy requires {} provenance", requirement.label()),
            disposition: "Jetpack rejected the package before provider, cache, or remote bytes became usable."
                .to_string(),
            fix: "Refresh `.jet/lock` with the required provenance, then retry.".to_string(),
        }));
    }
    Ok(())
}

pub struct VerifiedCacheHit {
    pub entry: StoreEntry,
    pub lease: CacheLease,
}

pub struct CacheLease {
    files: Vec<(PathBuf, fs::File)>,
    executables: Vec<(std::ffi::OsString, fs::File)>,
    out: PathBuf,
    lease_root: PathBuf,
    lease_lock_path: PathBuf,
    live_lock: Option<crate::RuntimePolicy::FileLock>,
    handed_off: Cell<bool>,
    protocol_lease_id: String,
    protocol_generation: u64,
    protocol_owner_scope: String,
    protocol_owner_lock: Option<crate::RuntimePolicy::FileLock>,
    pub(crate) snapshot_root: PathBuf,
    snapshot_dir_handle: Option<fs::File>,
    bin_relative: Option<PathBuf>,
    expected_digest: String,
    direct_cas: bool,
    package: String,
    version: String,
    reference: String,
    store_root: PathBuf,
    status: ConsumptionStatus,
    wrapper_root: Option<PathBuf>,
    /// Logical `/nix/store/<name>` paths mapped to a verified lease root or
    /// canonical Hangar object. Shell consumers use this only inside a rootless
    /// namespace.
    nix_store_projection: Vec<(String, PathBuf)>,
    /// Verified output roots accepted by `stable_path`. Linux Nix substitutions
    /// lease the immutable CAS object by open directory handle; other outputs
    /// use a private snapshot.
    leased_output_roots: Vec<(PathBuf, PathBuf)>,
    bin_output_root: Option<PathBuf>,
    projected_bin_root: Option<PathBuf>,
    #[cfg(target_os = "linux")]
    nix_projection_bindings: Vec<NixProjectionBinding>,
    _wrapper_dir_handle: Option<fs::File>,
}

#[cfg(target_os = "linux")]
struct NixProjectionBinding {
    logical: String,
    digest: String,
    primary: bool,
    source: PathBuf,
    /// `None` for symlink-root objects: a symlink cannot be opened
    /// `O_NOFOLLOW`; its stability is proven by re-reading the target.
    handle: Option<fs::File>,
    symlink_target: Option<PathBuf>,
}

pub(crate) struct NixStoreProjection {
    pub(crate) logical: String,
    digest: String,
    primary: bool,
    pub(crate) source: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumptionStatus {
    Consumable,
    NonConsumable { reason: String },
}

impl CacheLease {
    pub(crate) fn mark_process_handoff(&self) {
        self.handed_off.set(true);
    }

    pub fn status(&self) -> &ConsumptionStatus {
        &self.status
    }

    pub fn wrapper_dir(&self) -> Option<&Path> {
        self.wrapper_root.as_deref()
    }

    /// Return the lease-owned bin directory for PATH projection. On Linux the
    /// protected wrapper directory is the only path form that may cross the
    /// child boundary; the private snapshot path remains an internal source
    /// for validation and fd-backed handoff.
    pub(crate) fn projected_bin_dir(&self) -> Option<PathBuf> {
        self.validate().ok()?;
        let bin = self.bin_relative.as_ref()?;
        #[cfg(target_os = "linux")]
        if let Some(wrapper) = &self.wrapper_root {
            let _ = bin;
            return Some(wrapper.clone());
        }
        #[cfg(unix)]
        if self.projected_bin_root.as_ref() == Some(&self.snapshot_root) {
            if let Some(directory) = &self.snapshot_dir_handle {
                use std::os::fd::AsRawFd as _;
                let prefix = if cfg!(target_os = "linux") {
                    "/proc/self/fd"
                } else {
                    "/dev/fd"
                };
                return Some(
                    PathBuf::from(format!("{prefix}/{}", directory.as_raw_fd())).join(bin),
                );
            }
        }
        self.projected_bin_root.as_ref().map(|root| root.join(bin))
    }

    pub(crate) fn nix_store_projection(&self) -> &[(String, PathBuf)] {
        &self.nix_store_projection
    }

    pub fn original_output(&self) -> &Path {
        &self.out
    }

    pub fn original_reference(&self) -> &str {
        &self.reference
    }

    pub(crate) fn profile_install_receipt(&self) -> std::io::Result<ProfileInstallReceipt> {
        self.validate()?;
        let mut executable_members = self
            .executables
            .iter()
            .map(|(name, _)| {
                name.to_str()
                    .map(str::to_string)
                    .ok_or_else(|| std::io::Error::other("tool executable name is not UTF-8"))
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        executable_members.sort();
        if executable_members.is_empty() {
            return Err(std::io::Error::other(
                "verified tool lease has no executables",
            ));
        }
        Ok(ProfileInstallReceipt {
            store_root: self.store_root.clone(),
            package: self.package.clone(),
            version: self.version.clone(),
            reference: self.reference.clone(),
            output_hash: self.expected_digest.clone(),
            executable_members,
        })
    }

    pub(crate) fn copy_profile_executable(
        &self,
        member: &str,
        destination: &Path,
    ) -> std::io::Result<ProfileExecutableProof> {
        self.validate()?;
        let (_, source) = self
            .executables
            .iter()
            .find(|(name, _)| name == member)
            .ok_or_else(|| std::io::Error::other("verified receipt lost executable member"))?;
        copy_open_profile_file(source, destination)
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
        let (member, file) = self.executable_file(name)?;
        Some(self.executable_file_path(member, file))
    }

    /// Resolve a caller-supplied executable only when it is either an exact
    /// lease member or the exact raw/projected path for that member. This is
    /// the confinement boundary: a path that merely shares an output prefix
    /// never gets converted into a trusted executable handle.
    pub(crate) fn executable_for(&self, requested: &str) -> Option<PathBuf> {
        self.require_consumable().ok()?;
        let (member, file) = self.executable_file(requested)?;
        Some(self.executable_file_path(member, file))
    }

    pub(crate) fn executable_for_command(
        &self,
        requested: &str,
    ) -> std::io::Result<Option<PathBuf>> {
        let caller_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        self.executable_for_command_at(requested, &caller_dir)
    }

    pub(crate) fn executable_for_command_at(
        &self,
        requested: &str,
        caller_dir: &Path,
    ) -> std::io::Result<Option<PathBuf>> {
        self.require_consumable()?;
        let requested_path = Path::new(requested);
        let resolved_path = if requested_path.is_absolute() {
            requested_path.to_path_buf()
        } else {
            caller_dir.join(requested_path)
        };
        if let Some((member, file)) = self.executable_file_at(requested, &resolved_path) {
            return Ok(Some(self.executable_file_path(member, file)));
        }
        let resolved_paths = path_variants(&resolved_path);
        let path_is_lease_owned = [
            Some(self.out.as_path()),
            Some(self.lease_root.as_path()),
            self.bin_output_root.as_deref(),
            self.projected_bin_root.as_deref(),
            self.wrapper_root.as_deref(),
        ]
        .into_iter()
        .flatten()
        .chain(
            self.leased_output_roots
                .iter()
                .map(|(root, _)| root.as_path()),
        )
        .any(|root| {
            let roots = path_variants(root);
            roots.iter().any(|root| {
                resolved_paths
                    .iter()
                    .any(|resolved| resolved.starts_with(root))
            })
        });
        if path_is_lease_owned {
            return Err(std::io::Error::other(
                "caller requested a path inside an executable lease that is not a recorded member",
            ));
        }
        Ok(None)
    }

    fn executable_file(&self, requested: &str) -> Option<(&std::ffi::OsString, &fs::File)> {
        self.executable_file_at(requested, Path::new(requested))
    }

    fn executable_file_at(
        &self,
        requested: &str,
        requested_path: &Path,
    ) -> Option<(&std::ffi::OsString, &fs::File)> {
        if requested.is_empty() || requested.contains('/') || requested.contains('\\') {
            let member = requested_path.file_name()?;
            let bin = self.bin_relative.as_ref()?;
            let matches_root = |root: Option<&Path>| {
                root.map(|root| root.join(bin).join(member) == requested_path)
                    .unwrap_or(false)
            };
            let matches_wrapper = self
                .wrapper_root
                .as_deref()
                .map(|root| root.join(member) == requested_path)
                .unwrap_or(false);
            if !matches_root(self.bin_output_root.as_deref())
                && !matches_root(self.projected_bin_root.as_deref())
                && !matches_wrapper
            {
                return None;
            }
            return self
                .executables
                .iter()
                .find(|(name, _)| name.as_os_str() == member)
                .map(|(name, file)| (name, file));
        }
        self.executables
            .iter()
            .find(|(member, _)| member == requested)
            .map(|(member, file)| (member, file))
    }

    fn executable_file_path(&self, member: &std::ffi::OsString, file: &fs::File) -> PathBuf {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd as _;
            let _ = member;
            let prefix = if cfg!(target_os = "linux") {
                "/proc/self/fd"
            } else {
                "/dev/fd"
            };
            PathBuf::from(format!("{prefix}/{}", file.as_raw_fd()))
        }
        #[cfg(not(unix))]
        {
            let _ = file;
            let bin = self
                .bin_relative
                .as_ref()
                .expect("executable member has a bin-relative path");
            self.projected_bin_root
                .as_ref()
                .expect("executable member has a projected bin root")
                .join(bin)
                .join(member)
        }
    }

    pub(crate) fn projected_executable(&self, name: &str) -> Option<PathBuf> {
        self.require_consumable().ok()?;
        if name.contains(std::path::MAIN_SEPARATOR) {
            return None;
        }
        self.executables
            .iter()
            .any(|(member, _)| member == name)
            .then(|| {
                self.projected_bin_root
                    .as_ref()
                    .and_then(|root| self.bin_relative.as_ref().map(|bin| root.join(bin)))
                    .map(|bin| bin.join(name))
            })?
    }

    pub fn stable_path(&self, path: &str) -> std::io::Result<PathBuf> {
        self.validate()?;
        let path = Path::new(path);
        let (output_root, lease_root, relative) = self
            .leased_output_roots
            .iter()
            .find_map(|(output_root, lease_root)| {
                path.strip_prefix(output_root)
                    .ok()
                    .map(|relative| (output_root, lease_root, relative))
            })
            .ok_or_else(|| {
                std::io::Error::other(format!(
                    "consumer path `{}` escapes leased output `{}`",
                    path.display(),
                    self.out.display()
                ))
            })?;
        if relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(std::io::Error::other(
                "leased consumer path contains parent traversal",
            ));
        }
        if output_root == &self.out {
            if let Some((_, file)) = self.files.iter().find(|(member, _)| member == relative) {
                #[cfg(unix)]
                {
                    use std::os::fd::AsRawFd as _;
                    let prefix = if cfg!(target_os = "linux") {
                        "/proc/self/fd"
                    } else {
                        "/dev/fd"
                    };
                    return Ok(PathBuf::from(format!("{prefix}/{}", file.as_raw_fd())));
                }
                #[cfg(not(unix))]
                let _ = file;
            }
        }
        if self.bin_output_root.as_ref() == Some(output_root)
            && relative.parent() == self.bin_relative.as_deref()
        {
            if let Some((_, file)) = self
                .executables
                .iter()
                .find(|(name, _)| Some(name.as_os_str()) == relative.file_name())
            {
                #[cfg(unix)]
                {
                    use std::os::fd::AsRawFd as _;
                    let prefix = if cfg!(target_os = "linux") {
                        "/proc/self/fd"
                    } else {
                        "/dev/fd"
                    };
                    return Ok(PathBuf::from(format!("{prefix}/{}", file.as_raw_fd())));
                }
                #[cfg(not(unix))]
                let _ = file;
            }
        }
        #[cfg(unix)]
        if output_root == &self.out {
            if let Some(directory) = &self.snapshot_dir_handle {
                use std::os::fd::AsRawFd as _;
                let prefix = if cfg!(target_os = "linux") {
                    "/proc/self/fd"
                } else {
                    "/dev/fd"
                };
                return Ok(
                    PathBuf::from(format!("{prefix}/{}", directory.as_raw_fd())).join(relative)
                );
            }
        }
        Ok(lease_root.join(relative))
    }

    /// Revalidate immediately before handing paths to a child consumer. The
    /// archive reader uses no-follow handles and rejects concurrent mutation.
    pub fn validate(&self) -> std::io::Result<()> {
        self.require_consumable()?;
        if self.direct_cas {
            let expected = self
                .store_root
                .join("hangar")
                .join(OBJECTS_DIR)
                .join(&self.expected_digest);
            if self.snapshot_root != expected {
                return Err(std::io::Error::other(
                    "direct CAS lease does not name its digest path",
                ));
            }
            let path_metadata = fs::symlink_metadata(&self.snapshot_root)?;
            if path_metadata.file_type().is_symlink() || !path_metadata.is_dir() {
                return Err(std::io::Error::other(
                    "direct CAS lease root is not a real directory",
                ));
            }
            let opened_metadata = self
                .snapshot_dir_handle
                .as_ref()
                .ok_or_else(|| std::io::Error::other("direct CAS lease lost its directory handle"))?
                .metadata()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt as _;
                if path_metadata.dev() != opened_metadata.dev()
                    || path_metadata.ino() != opened_metadata.ino()
                {
                    return Err(std::io::Error::other(
                        "direct CAS lease root changed while leased",
                    ));
                }
            }
        } else {
            if !self.protocol_lease_id.is_empty() {
                crate::RuntimePolicy::ExecutableLeaseProtocol::open(&self.store_root)?
                    .validate_snapshot(
                        &self.protocol_lease_id,
                        self.protocol_generation,
                        &self.protocol_owner_scope,
                        &self.snapshot_root,
                        &self.expected_digest,
                    )?;
            }
            let actual = crate::Envelope::try_output_hash_of(&self.snapshot_root.to_string_lossy())
                .map_err(std::io::Error::other)?;
            if actual != self.expected_digest {
                return Err(std::io::Error::other(format!(
                    "leased output changed: expected {}, got {actual}",
                    self.expected_digest
                )));
            }
        }
        #[cfg(target_os = "linux")]
        for binding in &self.nix_projection_bindings {
            let path_metadata = fs::symlink_metadata(&binding.source)?;
            if !path_metadata.file_type().is_symlink()
                && !path_metadata.is_dir()
                && !path_metadata.is_file()
            {
                return Err(std::io::Error::other(format!(
                    "Nix projection source `{}` is not a regular node",
                    binding.source.display()
                )));
            }
            match (&binding.handle, &binding.symlink_target) {
                (Some(handle), _) => {
                    if path_metadata.file_type().is_symlink() {
                        return Err(std::io::Error::other(format!(
                            "Nix projection source `{}` changed type",
                            binding.source.display()
                        )));
                    }
                    let opened_metadata = handle.metadata()?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt as _;
                        if path_metadata.dev() != opened_metadata.dev()
                            || path_metadata.ino() != opened_metadata.ino()
                        {
                            return Err(std::io::Error::other(format!(
                                "Nix projection source `{}` changed while leased",
                                binding.source.display()
                            )));
                        }
                    }
                    if path_metadata.is_dir() != opened_metadata.is_dir()
                        || path_metadata.is_file() != opened_metadata.is_file()
                    {
                        return Err(std::io::Error::other(format!(
                            "Nix projection source `{}` changed type",
                            binding.source.display()
                        )));
                    }
                }
                (None, Some(expected_target)) => {
                    if !path_metadata.file_type().is_symlink()
                        || &fs::read_link(&binding.source)? != expected_target
                    {
                        return Err(std::io::Error::other(format!(
                            "Nix projection source `{}` changed while leased",
                            binding.source.display()
                        )));
                    }
                }
                (None, None) => {
                    return Err(std::io::Error::other(format!(
                        "Nix projection source `{}` has no stability witness",
                        binding.source.display()
                    )));
                }
            }
            if binding.primary {
                if binding.digest != self.expected_digest {
                    return Err(std::io::Error::other(format!(
                        "Nix primary projection `{}` has the wrong digest",
                        binding.logical
                    )));
                }
                if binding.source != self.snapshot_root {
                    return Err(std::io::Error::other(format!(
                        "Nix primary projection `{}` is not the private lease snapshot",
                        binding.logical
                    )));
                }
            } else {
                let expected = self
                    .store_root
                    .join("hangar")
                    .join(OBJECTS_DIR)
                    .join(&binding.digest);
                if binding.source != expected {
                    return Err(std::io::Error::other(format!(
                        "Nix projection `{}` does not use its canonical Hangar object",
                        binding.logical
                    )));
                }
                if !self.direct_cas {
                    let actual = Ingest::verified_output_hash_persistent(
                        &binding.source,
                        Some(&self.store_root.join("hangar")),
                        false,
                    )
                    .map_err(std::io::Error::other)?;
                    if actual != binding.digest {
                        return Err(std::io::Error::other(format!(
                            "Nix projection `{}` changed: expected {}, got {actual}",
                            binding.logical, binding.digest
                        )));
                    }
                }
            }
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
                    (
                        name.clone(),
                        PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())),
                    )
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
            fix: "Re-run `jet store fetch` after `jet clean`. If the problem persists, audit the source before rebuilding."
                .to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileInstallReceipt {
    pub(crate) store_root: PathBuf,
    pub(crate) package: String,
    pub(crate) version: String,
    pub(crate) reference: String,
    pub(crate) output_hash: String,
    pub(crate) executable_members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileExecutableProof {
    pub(crate) digest: String,
    pub(crate) mode: u32,
}

fn copy_open_profile_file(
    source: &fs::File,
    destination: &Path,
) -> std::io::Result<ProfileExecutableProof> {
    use std::io::{Read as _, Seek as _, Write as _};

    let mut input = source.try_clone()?;
    input.seek(std::io::SeekFrom::Start(0))?;
    let metadata = input.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::other(
            "profile executable is not a regular file",
        ));
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut hasher = SHA256::StreamingSha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        output.write_all(&buffer[..count])?;
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = metadata.permissions().mode() & 0o777;
        fs::set_permissions(destination, fs::Permissions::from_mode(mode))?;
        mode
    };
    #[cfg(not(unix))]
    let mode = 0;
    output.sync_all()?;
    let digest = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if SHA256::sha256_file_hex(destination)? != digest {
        return Err(std::io::Error::other("copied profile executable changed"));
    }
    Ok(ProfileExecutableProof {
        digest: format!("sha256-{digest}"),
        mode,
    })
}

pub(crate) fn profile_file_proof(path: &Path) -> std::io::Result<ProfileExecutableProof> {
    let path_metadata = fs::symlink_metadata(path)?;
    if !path_metadata.is_file() || path_metadata.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "profile projection is not a no-follow file",
        ));
    }
    let opened = fs::File::open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let metadata = opened.metadata()?;
        if path_metadata.dev() != metadata.dev() || path_metadata.ino() != metadata.ino() {
            return Err(std::io::Error::other(
                "profile projection changed while opening",
            ));
        }
        return Ok(ProfileExecutableProof {
            digest: format!("sha256-{}", SHA256::sha256_file_hex(path)?),
            mode: metadata.permissions().mode() & 0o777,
        });
    }
    #[cfg(not(unix))]
    Ok(ProfileExecutableProof {
        digest: format!("sha256-{}", SHA256::sha256_file_hex(path)?),
        mode: 0,
    })
}

/// Return path spellings that can identify the same caller target. The
/// lexical form catches missing nodes and `..` traversal; the canonical form
/// catches caller-created symlink aliases into a protected lease.
fn path_variants(path: &Path) -> Vec<PathBuf> {
    let lexical = lexical_normalize(path);
    let mut variants = vec![lexical];
    if let Ok(canonical) = fs::canonicalize(path) {
        if !variants.iter().any(|variant| variant == &canonical) {
            variants.push(canonical);
        }
    }
    variants
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let absolute = path.is_absolute();
    let mut normalized = PathBuf::new();
    let mut normal_components = 0usize;
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if normal_components > 0 {
                    normalized.pop();
                    normal_components -= 1;
                } else if !absolute {
                    normalized.push(component.as_os_str());
                }
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                normalized.push(component.as_os_str());
            }
            std::path::Component::Normal(value) => {
                normalized.push(value);
                normal_components += 1;
            }
        }
    }
    normalized
}

impl Drop for CacheLease {
    fn drop(&mut self) {
        // The local copy must close before probing. A launched descendant may
        // still hold the inherited lease handle, in which case recovery owns
        // cleanup and this process must leave the whole container intact.
        drop(self.live_lock.take());
        let lock_held = |path: &Path| match crate::RuntimePolicy::lock_state(path) {
            Ok(crate::RuntimePolicy::LockState::Held) => true,
            Ok(crate::RuntimePolicy::LockState::Idle)
            | Ok(crate::RuntimePolicy::LockState::Absent) => false,
            Err(_) => true,
        };
        // The owner lock is still held by this CacheLease while Drop runs. It
        // authenticates publication, but cannot prove a descendant exists;
        // only the inherited container lock is the process-tree lifetime fact.
        let keep_for_descendant = self.handed_off.get() && lock_held(&self.lease_lock_path);
        if !keep_for_descendant {
            if remove_snapshot_node(&self.lease_root).is_ok()
                && !self.protocol_lease_id.is_empty()
                && self.protocol_owner_lock.is_some()
            {
                if let Ok(protocol) =
                    crate::RuntimePolicy::ExecutableLeaseProtocol::open(&self.store_root)
                {
                    let released = protocol
                        .release(
                            &self.protocol_lease_id,
                            self.protocol_generation,
                            &self.protocol_owner_scope,
                        )
                        .is_ok();
                    if released {
                        drop(self.protocol_owner_lock.take());
                        let _ = protocol.reap_empty_record(&self.protocol_lease_id);
                    }
                }
            }
        }
    }
}

pub fn find_verified_by_reference(
    roots: &Roots,
    reference: &str,
    expectation: &CacheExpectation,
) -> std::io::Result<Option<VerifiedCacheHit>> {
    let phase_started = std::time::Instant::now();
    let result = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        let phase_started = std::time::Instant::now();
        let graph = Closure::closure_graph_structure_unlocked(roots)?;
        super::timing(
            &format!("verify {reference} closure-structure"),
            phase_started.elapsed(),
        );
        let phase_started = std::time::Instant::now();
        let entry = list_unlocked(roots)?
            .into_iter()
            .filter(|entry| entry.reference == reference)
            .filter(|entry| {
                verify_cache_entry_with_graph(roots, entry, reference, expectation, Some(&graph))
                    .trusted()
            })
            .max_by_key(|entry| entry.last_used_at);
        super::timing(
            &format!("verify {reference} entry-check"),
            phase_started.elapsed(),
        );
        let Some(entry) = entry else {
            return Ok(None);
        };
        let phase_started = std::time::Instant::now();
        let lease = snapshot_lease_unlocked(roots, &entry)?;
        super::timing(
            &format!("verify {reference} lease"),
            phase_started.elapsed(),
        );
        Ok(Some(VerifiedCacheHit { entry, lease }))
    });
    super::timing(
        &format!("verify {reference} total"),
        phase_started.elapsed(),
    );
    result
}

/// Reuse the exact user-profile realization for a package ref. User-profile
/// refs have no project lock from which to derive a fresh expectation, so the
/// recorded Hangar identity is the preparation witness; the normal closure,
/// output, provenance, and reproducibility checks still certify the hit.
pub(crate) fn find_verified_user_profile_by_reference(
    roots: &Roots,
    reference: &str,
) -> std::io::Result<Option<VerifiedRealization>> {
    let Some(candidate) = find_by_reference(roots, reference) else {
        return Ok(None);
    };
    let expectation = CacheExpectation {
        identity: candidate.cache_identity.clone(),
        owned_output: None,
        allow_unsigned_local: true,
    };
    let Some(hit) = find_verified_by_reference(roots, reference, &expectation)? else {
        return Ok(None);
    };
    Ok(Some(VerifiedRealization {
        entry: hit.entry,
        source_state: crate::Provider::SourceState::Cached,
        lease: hit.lease,
    }))
}

pub(crate) fn snapshot_lease(roots: &Roots, entry: &StoreEntry) -> std::io::Result<CacheLease> {
    crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        snapshot_lease_unlocked(roots, entry)
    })
}

#[cfg(target_os = "linux")]
fn direct_cas_entry(roots: &Roots, entry: &StoreEntry) -> bool {
    let Ok(producer) = ProducerRecord::decode(&entry.producer_record) else {
        return false;
    };
    let expected = roots
        .hangar_dir()
        .join(OBJECTS_DIR)
        .join(&entry.envelope.output_hash);
    let Ok(metadata) = fs::symlink_metadata(&expected) else {
        return false;
    };
    producer.provider == "nix"
        && Path::new(&entry.out) == expected
        && !entry.envelope.output_hash.is_empty()
        && metadata.is_dir()
        && !metadata.file_type().is_symlink()
}

#[cfg(not(target_os = "linux"))]
fn direct_cas_entry(_roots: &Roots, _entry: &StoreEntry) -> bool {
    false
}

fn snapshot_lease_unlocked(roots: &Roots, entry: &StoreEntry) -> std::io::Result<CacheLease> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    use std::sync::atomic::Ordering;

    let leases = roots.root.join("leases");
    Ingest::ensure_real_directory(&leases, "Hangar lease directory")?;
    let mut components = Path::new(&entry.id).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(std::io::Error::other(
            "cache lease entry identity is not one path component",
        ));
    }
    let lease_name = format!(
        "{}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed),
        entry.id
    );
    if lease_name.len() > super::LEASE_NAME_MAX {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "cache lease identity exceeds the recovery bound",
        ));
    }
    if let Ok(producer) = ProducerRecord::decode(&entry.producer_record) {
        crate::Provider::validate_nix_build_facts(&producer)?;
    }
    let lease_root = leases.join(&lease_name);
    fs::create_dir(&lease_root)?;
    let lease_lock_path = lease_root.join(".locks").join("live.lock");
    let mut live_lock = match crate::RuntimePolicy::acquire_lease_lock(&lease_root, "live") {
        Ok(lock) => Some(lock),
        Err(error) => {
            let _ = fs::remove_dir_all(&lease_root);
            return Err(error);
        }
    };
    let direct_cas = direct_cas_entry(roots, entry);
    let lease_snapshot_root = lease_root.join("snapshot");
    let snapshot_root = if direct_cas {
        PathBuf::from(&entry.out)
    } else {
        lease_snapshot_root.clone()
    };
    let phase_started = std::time::Instant::now();
    let result = (|| {
        if !Path::new(&entry.out).exists() {
            Ingest::ensure_real_directory(&snapshot_root, "cache lease snapshot")?;
            return Ok(CacheLease {
                files: Vec::new(),
                executables: Vec::new(),
                out: PathBuf::from(&entry.out),
                lease_root: lease_root.clone(),
                lease_lock_path: lease_lock_path.clone(),
                live_lock: live_lock.take(),
                handed_off: Cell::new(false),
                protocol_lease_id: String::new(),
                protocol_generation: 0,
                protocol_owner_scope: String::new(),
                protocol_owner_lock: None,
                snapshot_root,
                snapshot_dir_handle: None,
                bin_relative: None,
                expected_digest: String::new(),
                direct_cas: false,
                package: entry.name.clone(),
                version: entry.version.clone(),
                reference: entry.reference.clone(),
                store_root: roots.root.clone(),
                status: ConsumptionStatus::NonConsumable {
                    reason: "realization has no canonical consumable output".to_string(),
                },
                wrapper_root: None,
                nix_store_projection: Vec::new(),
                leased_output_roots: Vec::new(),
                bin_output_root: None,
                projected_bin_root: None,
                #[cfg(target_os = "linux")]
                nix_projection_bindings: Vec::new(),
                _wrapper_dir_handle: None,
            });
        }
        let (sealed_digest, snapshot_dir_handle, files) = if direct_cas {
            let snapshot_dir_handle = fs::File::open(&snapshot_root).and_then(|file| {
                clear_close_on_exec(&file)?;
                Ok(file)
            })?;
            (
                entry.envelope.output_hash.clone(),
                snapshot_dir_handle,
                Vec::new(),
            )
        } else {
            let mut hardlinks = BTreeMap::new();
            copy_snapshot_node(Path::new(&entry.out), &snapshot_root, &mut hardlinks)?;
            let digest = crate::Envelope::try_output_hash_of(&snapshot_root.to_string_lossy())
                .map_err(std::io::Error::other)?;
            if digest != entry.envelope.output_hash {
                remove_snapshot_node(&snapshot_root)?;
                return Err(std::io::Error::other(format!(
                    "private lease snapshot mismatch: expected {}, got {digest}",
                    entry.envelope.output_hash
                )));
            }
            seal_local_output(&snapshot_root)?;
            fsync_tree(&lease_root)?;
            let sealed_digest =
                crate::Envelope::try_output_hash_of(&snapshot_root.to_string_lossy())
                    .map_err(std::io::Error::other)?;
            let snapshot_dir_handle = fs::File::open(&snapshot_root).and_then(|file| {
                clear_close_on_exec(&file)?;
                Ok(file)
            })?;
            let mut files = Vec::new();
            open_snapshot_files(&snapshot_root, &snapshot_root, &mut files)?;
            (sealed_digest, snapshot_dir_handle, files)
        };
        super::timing(
            &format!("lease {} snapshot", entry.reference),
            phase_started.elapsed(),
        );
        let phase_started = std::time::Instant::now();
        let projections = nix_store_projection_for_entry(roots, entry, &snapshot_root)?;
        super::timing(
            &format!("lease {} projections", entry.reference),
            phase_started.elapsed(),
        );
        let mut output_sources = vec![(PathBuf::from(&entry.out), snapshot_root.clone())];
        let mut seen_output_digests = BTreeSet::from([entry.envelope.output_hash.clone()]);
        for digest in entry.named_outputs.values() {
            if !seen_output_digests.insert(digest.clone()) {
                continue;
            }
            let source = projections
                .iter()
                .find(|projection| projection.digest == *digest)
                .map(|projection| projection.source.clone())
                .unwrap_or(hangar_projection_object(roots, digest, "named output")?);
            output_sources.push((roots.hangar_dir().join(OBJECTS_DIR).join(digest), source));
        }
        #[cfg(target_os = "linux")]
        let (nix_store_projection, nix_projection_bindings) =
            open_nix_projection_sources(projections)?;
        #[cfg(not(target_os = "linux"))]
        let nix_store_projection = projections
            .into_iter()
            .map(|projection| (projection.logical, projection.source))
            .collect();
        let leased_output_roots = output_sources
            .iter()
            .map(|(output_root, source)| {
                let lease_root = if source == &snapshot_root {
                    snapshot_root.clone()
                } else {
                    #[cfg(target_os = "linux")]
                    {
                        use std::os::fd::AsRawFd as _;
                        let binding = nix_projection_bindings
                            .iter()
                            .find(|binding| binding.source == *source)
                            .ok_or_else(|| {
                                std::io::Error::other(format!(
                                    "named output `{}` has no stable lease handle",
                                    output_root.display()
                                ))
                            })?;
                        match &binding.handle {
                            Some(handle) => {
                                PathBuf::from(format!("/proc/self/fd/{}", handle.as_raw_fd()))
                            }
                            // Symlink-root objects have no fd; the canonical
                            // source path is the lease root and its target is
                            // witnessed at commit.
                            None => source.clone(),
                        }
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        source.clone()
                    }
                };
                Ok((output_root.clone(), lease_root))
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        let (bin_output_root, bin_relative, projected_bin_root) = if entry.bin.is_empty() {
            (None, None, None)
        } else {
            let bin = Path::new(&entry.bin);
            output_sources
                .iter()
                .find_map(|(output_root, source)| {
                    bin.strip_prefix(output_root).ok().map(|relative| {
                        (
                            Some(output_root.clone()),
                            Some(relative.to_path_buf()),
                            Some(source.clone()),
                        )
                    })
                })
                .unwrap_or((None, None, None))
        };
        let executables = open_snapshot_executables(
            projected_bin_root.as_deref().unwrap_or(&snapshot_root),
            bin_relative.as_deref(),
        )?;
        super::timing(
            &format!("lease {} executable-open", entry.reference),
            phase_started.elapsed(),
        );
        let wrappers = create_exec_wrappers(&lease_snapshot_root, &executables)?;
        let (protocol_lease_id, protocol_generation, protocol_owner_scope, protocol_owner_lock) =
            if direct_cas {
                (String::new(), 0, String::new(), None)
            } else {
                let protocol = crate::RuntimePolicy::ExecutableLeaseProtocol::open(&roots.root)?;
                let protocol_lease_id = protocol.new_lease_id()?;
                let protocol_owner_scope =
                    crate::RuntimePolicy::ExecutableLeaseProtocol::owner_scope(&protocol_lease_id)?;
                let protocol_members = executable_lease_members(&executables)?;
                let (protocol_request, protocol_owner_lock) = protocol.prepare_snapshot(
                    &protocol_lease_id,
                    &protocol_owner_scope,
                    &snapshot_root,
                    &entry.name,
                    &entry.version,
                    &entry.reference,
                    &sealed_digest,
                    &protocol_members,
                )?;
                let protocol_frame = protocol.encode_request(&protocol_request)?;
                let protocol_receipt =
                    protocol.accept_snapshot(&protocol_frame, &sealed_digest, &snapshot_root)?;
                (
                    protocol_lease_id,
                    protocol_receipt.generation,
                    protocol_owner_scope,
                    Some(protocol_owner_lock),
                )
            };
        Ok(CacheLease {
            files,
            executables,
            out: PathBuf::from(&entry.out),
            lease_root: lease_root.clone(),
            lease_lock_path: lease_lock_path.clone(),
            live_lock: live_lock.take(),
            handed_off: Cell::new(false),
            protocol_lease_id,
            protocol_generation,
            protocol_owner_scope,
            protocol_owner_lock,
            snapshot_root,
            snapshot_dir_handle: Some(snapshot_dir_handle),
            bin_relative,
            expected_digest: sealed_digest,
            direct_cas,
            package: entry.name.clone(),
            version: entry.version.clone(),
            reference: entry.reference.clone(),
            store_root: roots.root.clone(),
            status: ConsumptionStatus::Consumable,
            wrapper_root: wrappers.as_ref().map(|wrapper| wrapper.root.clone()),
            nix_store_projection,
            leased_output_roots,
            bin_output_root,
            projected_bin_root,
            #[cfg(target_os = "linux")]
            nix_projection_bindings,
            _wrapper_dir_handle: wrappers.map(|wrapper| wrapper.directory),
        })
    })();
    if result.is_err() {
        drop(live_lock.take());
        let _ = make_tree_writable_for_removal(&lease_root);
        let _ = fs::remove_dir_all(&lease_root);
    }
    // Product copy: a raw OS error out of the lease names nothing. Every
    // lease failure is about materializing this entry's snapshot or its
    // recorded Hangar objects, so say so once here; already-specific
    // messages pass through unchanged.
    result.map_err(|error| {
        if error.to_string().contains("Hangar") {
            error
        } else {
            std::io::Error::other(format!(
                "cache lease for `{}`: a Hangar object or snapshot resource is unavailable: {error}",
                entry.id
            ))
        }
    })
}

#[cfg(target_os = "linux")]
fn open_nix_projection_sources(
    projections: Vec<NixStoreProjection>,
) -> std::io::Result<(Vec<(String, PathBuf)>, Vec<NixProjectionBinding>)> {
    use std::os::fd::AsRawFd as _;
    use std::os::unix::fs::MetadataExt as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let mut stable = Vec::with_capacity(projections.len());
    let mut bindings = Vec::with_capacity(projections.len());
    for projection in projections {
        let path_metadata = fs::symlink_metadata(&projection.source).map_err(|error| {
            std::io::Error::other(format!(
                "Nix output `{}` Hangar object `{}` is unavailable: {error}",
                projection.logical, projection.digest
            ))
        })?;
        if !path_metadata.file_type().is_symlink()
            && !path_metadata.is_dir()
            && !path_metadata.is_file()
        {
            return Err(std::io::Error::other(format!(
                "Nix projection source `{}` is not a regular node",
                projection.source.display()
            )));
        }
        if path_metadata.file_type().is_symlink() {
            // Symlink-root objects project as the symlink itself; there is no
            // fd to pin, so the lease re-reads the target for stability.
            let target = fs::read_link(&projection.source).map_err(|error| {
                std::io::Error::other(format!(
                    "Nix output `{}` Hangar object `{}` is unavailable: {error}",
                    projection.logical, projection.digest
                ))
            })?;
            stable.push((projection.logical.clone(), projection.source.clone()));
            bindings.push(NixProjectionBinding {
                logical: projection.logical,
                digest: projection.digest,
                primary: projection.primary,
                source: projection.source,
                handle: None,
                symlink_target: Some(target),
            });
            continue;
        }
        let mut options = fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(crate::Envelope::nofollow_open_flag().map_err(std::io::Error::other)?);
        let handle = options.open(&projection.source).map_err(|error| {
            std::io::Error::other(format!(
                "Nix output `{}` Hangar object `{}` is unavailable: {error}",
                projection.logical, projection.digest
            ))
        })?;
        let opened_metadata = handle.metadata()?;
        if path_metadata.dev() != opened_metadata.dev()
            || path_metadata.ino() != opened_metadata.ino()
        {
            return Err(std::io::Error::other(format!(
                "Nix projection source `{}` changed while opening",
                projection.source.display()
            )));
        }
        clear_close_on_exec(&handle)?;
        let stable_path = PathBuf::from(format!("/proc/self/fd/{}", handle.as_raw_fd()));
        stable.push((projection.logical.clone(), stable_path));
        bindings.push(NixProjectionBinding {
            logical: projection.logical,
            digest: projection.digest,
            primary: projection.primary,
            source: projection.source,
            handle: Some(handle),
            symlink_target: None,
        });
    }
    Ok((stable, bindings))
}

pub(crate) fn nix_store_projection_for_entry(
    roots: &Roots,
    entry: &StoreEntry,
    snapshot_root: &Path,
) -> std::io::Result<Vec<NixStoreProjection>> {
    if entry.producer_record.is_empty() {
        return Ok(Vec::new());
    }
    let producer = ProducerRecord::decode(&entry.producer_record).map_err(|error| {
        std::io::Error::other(format!(
            "cache entry `{}` has an invalid producer record: {error}",
            entry.id
        ))
    })?;
    if producer.provider != "nix" {
        return Ok(Vec::new());
    }

    // The logical store names remain producer facts, but every runtime source
    // below is either the verified lease snapshot or a Hangar CAS object. A
    // Nix closure record is the authority for transitive runtime objects.
    let mut logical_digests = BTreeMap::new();
    for key in producer.facts.keys() {
        let Some(name) = key.strip_prefix("nix.output.") else {
            continue;
        };
        if name.contains('.') {
            continue;
        }
        let digest = if name == "out" {
            entry.envelope.output_hash.clone()
        } else {
            entry.named_outputs.get(name).cloned().ok_or_else(|| {
                std::io::Error::other(format!(
                    "Nix output `{name}` has no verified named-output digest"
                ))
            })?
        };
        let path = canonical_nix_output_path(&producer, name)?;
        add_nix_projection_path(&mut logical_digests, path, &digest)?;
    }
    if logical_digests.is_empty() {
        return Err(std::io::Error::other(format!(
            "Nix cache entry `{}` has no canonical output path for projection",
            entry.id
        )));
    }

    let graph = Closure::closure_graph_structure_unlocked(roots).map_err(|error| {
        std::io::Error::other(format!(
            "Nix cache entry `{}`: a Hangar object in its recorded closure is unavailable: {error}",
            entry.id
        ))
    })?;
    let mut closure_digests = BTreeSet::new();
    if let Some(record) = graph.records.get(&entry.id) {
        let mut expected_outputs = entry.named_outputs.clone();
        expected_outputs.insert("out".to_string(), entry.envelope.output_hash.clone());
        if record.primary != entry.envelope.output_hash
            || record.outputs != expected_outputs
            || record.references != entry.references.iter().cloned().collect()
        {
            return Err(std::io::Error::other(format!(
                "Nix closure record `{}` disagrees with the verified entry",
                entry.id
            )));
        }
        // Every named root may have references not shared by `out`.
        for digest in expected_outputs.values() {
            closure_digests.extend(graph.closure(digest));
        }
        for digest in &closure_digests {
            let object = graph.objects.get(digest).ok_or_else(|| {
                std::io::Error::other(format!(
                    "Nix closure object `{digest}` is missing from the Hangar graph"
                ))
            })?;
            let expected_path = roots.hangar_dir().join(OBJECTS_DIR).join(digest);
            if object.external || Path::new(&object.path) != expected_path {
                return Err(std::io::Error::other(format!(
                    "Nix closure object `{digest}` is not a canonical Hangar object"
                )));
            }
            // Admitted Nix objects are directory, regular file, or symlink
            // roots (gcc-wrapper-info and pkg-config-man are symlink roots);
            // only other node kinds are corrupt here.
            let metadata = fs::symlink_metadata(&expected_path)?;
            if !metadata.file_type().is_symlink() && !metadata.is_dir() && !metadata.is_file() {
                return Err(std::io::Error::other(format!(
                    "Nix closure object `{digest}` is not a regular Hangar node"
                )));
            }

            let mut paths = BTreeSet::new();
            for owner in graph.records.values() {
                for (name, owner_digest) in &owner.outputs {
                    if owner_digest != digest {
                        continue;
                    }
                    let producer =
                        ProducerRecord::decode(&owner.producer_record).map_err(|error| {
                            std::io::Error::other(format!(
                                "Nix closure record `{}` has invalid producer facts: {error}",
                                owner.id
                            ))
                        })?;
                    if producer.provider != "nix" {
                        return Err(std::io::Error::other(format!(
                            "Nix closure object `{digest}` is owned by `{}`",
                            producer.provider
                        )));
                    }
                    let path = canonical_nix_output_path(&producer, name).map_err(|error| {
                        std::io::Error::other(format!(
                            "Nix closure object `{digest}` has no canonical `{name}` output fact: {error}"
                        ))
                    })?;
                    paths.insert(path.to_string());
                }
            }
            if paths.len() != 1 {
                return Err(std::io::Error::other(format!(
                    "Nix closure object `{digest}` has {} canonical store paths",
                    paths.len()
                )));
            }
            add_nix_projection_path(
                &mut logical_digests,
                paths.first().expect("one path checked above"),
                digest,
            )?;
        }
    } else if !entry.references.is_empty()
        || producer.facts.contains_key("nix.closure.receipt")
        || producer
            .facts
            .contains_key("nix.cache.closure.receipt.sha256")
    {
        return Err(std::io::Error::other(format!(
            "Nix closure record `{}` is missing from the Hangar graph",
            entry.id
        )));
    }

    let primary_logical = ["out", "bin"]
        .into_iter()
        .find_map(|name| canonical_nix_output_path(&producer, name).ok());
    let mut projection = Vec::new();
    for (logical, digest) in logical_digests {
        let primary = primary_logical == Some(logical.as_str());
        let source = if primary {
            snapshot_root.to_path_buf()
        } else {
            hangar_projection_object(roots, &digest, logical.as_str())?
        };
        projection.push(NixStoreProjection {
            logical,
            digest,
            primary,
            source,
        });
    }
    Ok(projection)
}

fn canonical_nix_output_path<'a>(
    producer: &'a ProducerRecord,
    name: &str,
) -> std::io::Result<&'a str> {
    let output = producer
        .facts
        .get(&format!("nix.output.{name}"))
        .filter(|path| path.starts_with("/nix/store/"));
    let store_path = (name == "out")
        .then(|| producer.facts.get("nix.store-path"))
        .flatten()
        .filter(|path| path.starts_with("/nix/store/"));
    if let (Some(output), Some(store_path)) = (output, store_path) {
        if output != store_path {
            return Err(std::io::Error::other(format!(
                "Nix output `{name}` disagrees with its canonical store path"
            )));
        }
    }
    output.or(store_path).map(String::as_str).ok_or_else(|| {
        std::io::Error::other(format!(
            "Nix output `{name}` has no canonical `/nix/store` path"
        ))
    })
}

fn add_nix_projection_path(
    logical_digests: &mut BTreeMap<String, String>,
    logical: &str,
    digest: &str,
) -> std::io::Result<()> {
    let name = logical.strip_prefix("/nix/store/").ok_or_else(|| {
        std::io::Error::other(format!("invalid canonical Nix output path `{logical}`"))
    })?;
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err(std::io::Error::other(format!(
            "invalid canonical Nix output path `{logical}`"
        )));
    }
    let mut digest_components = Path::new(digest).components();
    if digest.is_empty()
        || !matches!(
            digest_components.next(),
            Some(std::path::Component::Normal(_))
        )
        || digest_components.next().is_some()
    {
        return Err(std::io::Error::other(format!(
            "invalid Hangar digest `{digest}` for `{logical}`"
        )));
    }
    if let Some(existing) = logical_digests.insert(logical.to_string(), digest.to_string()) {
        if existing != digest {
            return Err(std::io::Error::other(format!(
                "conflicting canonical Nix output path `{logical}`"
            )));
        }
    }
    Ok(())
}

fn hangar_projection_object(
    roots: &Roots,
    digest: &str,
    logical: &str,
) -> std::io::Result<PathBuf> {
    let object = roots.hangar_dir().join(OBJECTS_DIR).join(digest);
    let metadata = fs::symlink_metadata(&object).map_err(|error| {
        std::io::Error::other(format!(
            "Nix output `{logical}` Hangar object `{digest}` is unavailable: {error}"
        ))
    })?;
    // Admitted Nix objects are directory, regular file, or symlink roots
    // (gcc-wrapper-info projects a symlink); only other kinds are corrupt.
    if !metadata.file_type().is_symlink() && !metadata.is_dir() && !metadata.is_file() {
        return Err(std::io::Error::other(format!(
            "Nix output `{logical}` Hangar object `{digest}` is not a regular node"
        )));
    }
    Ok(object)
}

struct ExecWrappers {
    root: PathBuf,
    directory: fs::File,
}

pub(crate) fn required_child_pipe<T>(pipe: Option<T>, message: &'static str) -> std::io::Result<T> {
    pipe.ok_or_else(|| std::io::Error::other(message))
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

fn executable_lease_members(
    executables: &[(std::ffi::OsString, fs::File)],
) -> std::io::Result<Vec<crate::RuntimePolicy::LeaseMember>> {
    use std::io::{Read as _, Seek as _};

    let mut members = Vec::with_capacity(executables.len());
    for (name, file) in executables {
        let name = name
            .to_str()
            .ok_or_else(|| std::io::Error::other("lease executable name is not UTF-8"))?;
        let mut input = file.try_clone()?;
        input.seek(std::io::SeekFrom::Start(0))?;
        let mut hasher = SHA256::StreamingSha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        let digest = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        members.push(crate::RuntimePolicy::LeaseMember {
            name: name.to_string(),
            digest: format!("sha256-{digest}"),
        });
    }
    Ok(members)
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
        .args([
            "--user",
            "--map-root-user",
            "--mount",
            "--propagation",
            "private",
        ])
        .arg(shell)
        .args(["-c", script, "jetpack-lease-keeper"])
        .arg(mount)
        .arg(&mountpoint)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut input = required_child_pipe(child.stdin.take(), "piped lease keeper stdin")?;
    let output = required_child_pipe(child.stdout.take(), "piped lease keeper stdout")?;
    let mut output = std::io::BufReader::new(output);
    let mut line = String::new();
    output.read_line(&mut line)?;
    if line.trim() != "ready" {
        return Err(wrapper_keeper_error(
            child,
            "creating private executable mount",
        ));
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
        return Err(wrapper_keeper_error(
            child,
            "sealing private executable mount",
        ));
    }
    let directory = fs::File::open(&root)?;
    clear_close_on_exec(&directory)?;
    let root = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    drop(input);
    let status = child.wait()?;
    if !status.success() {
        return Err(std::io::Error::other(
            "private executable mount keeper failed",
        ));
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
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("required system tool `{name}` not found"),
            )
        })
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
    executables: &[(std::ffi::OsString, fs::File)],
) -> std::io::Result<Option<ExecWrappers>> {
    if executables.is_empty() {
        return Ok(None);
    }
    // A private snapshot is not a caller-unwritable handoff on these tiers.
    // Refuse the child before it can receive a raw snapshot path until the
    // installer-provided protected lease service accepts the same protocol.
    let _ = executables;
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "protected executable lease service is unavailable",
    ))
}

pub(crate) fn copy_snapshot_node(
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
        {
            let _ = target;
            return Err(std::io::Error::other(
                "cache symlink snapshots need platform support",
            ));
        }
    } else {
        return Err(std::io::Error::other("special file in cache snapshot"));
    }
    Ok(())
}

fn remove_snapshot_node(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    make_tree_writable_for_removal(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
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

pub(crate) fn open_snapshot_files(
    root: &Path,
    path: &Path,
    files: &mut Vec<(PathBuf, fs::File)>,
) -> std::io::Result<()> {
    let relative = path.strip_prefix(root).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "snapshot path `{}` is outside snapshot root `{}`",
                path.display(),
                root.display()
            ),
        )
    })?;
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
    files.push((relative.to_path_buf(), file));
    Ok(())
}

#[cfg(unix)]
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

#[cfg(not(unix))]
fn clear_close_on_exec(_file: &fs::File) -> std::io::Result<()> {
    Ok(())
}
