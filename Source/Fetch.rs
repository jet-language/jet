//! Package fetch operations (M12.1, D-PM4).
//!
//! Network access is entirely via git subprocess — no HTTP in the compiler.
//! Path dependencies are resolved in-place (no fetch needed).
//! Path and git results retain the legacy source store; verified registry
//! results are installed in the canonical Jetpack Hangar.

use crate::Diagnostics::Diagnostic;
use crate::Lock::{self, LockFile, LockSource, LockedPackage, LockedRevision};
use crate::Manifest::{check_toolchain, DepSpec, GitSelector, Manifest};
use crate::Package::PackagePolicyException;
use crate::Publish::SemVer::SemVer;
use crate::Publish::{self, ResolveMode, SolverCandidate, VersionConstraint, VersionReq};
use crate::Store;
use crate::Syntax;
use crate::SHA256::tree_hash;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

// ──────────────────────────────────────────────
// Main entry points
// ──────────────────────────────────────────────

/// Options for a fetch operation.
pub struct FetchOptions {
    /// If true, refuse network; verify lock only (--locked CI mode).
    pub locked: bool,
    /// If true, re-resolve moving selectors (@latest, branches).
    pub update: bool,
    /// If Some, only update this specific dep name.
    pub update_dep: Option<String>,
    /// How registry versions are selected when no exact lock is reused.
    pub resolution: ResolveMode,
}

/// Resolve, fetch, and install all dependencies from a manifest.
/// Returns the written lock file and a map of dep name → source directory.
pub fn fetch(
    project_root: &Path,
    manifest: &Manifest,
    existing_lock: Option<&LockFile>,
    opts: &FetchOptions,
) -> Result<(LockFile, HashMap<String, PathBuf>), Vec<Diagnostic>> {
    // Validate toolchain constraint.
    let manifest_path = crate::Manifest::manifest_path_in(project_root)
        .display()
        .to_string();
    if let Err(d) = check_toolchain(manifest, &manifest_path) {
        return Err(vec![d]);
    }

    // --locked: verify lock matches manifest, then stop.
    if opts.locked {
        let lock = existing_lock.ok_or_else(|| {
            vec![Lock::e1202(
                &project_root
                    .join(Syntax::UNIFIED_LOCK_FILE)
                    .display()
                    .to_string(),
            )]
        })?;
        if let Err(d) = Lock::verify_lock_matches_manifest(
            lock,
            manifest,
            &project_root
                .join(Syntax::UNIFIED_LOCK_FILE)
                .display()
                .to_string(),
        ) {
            return Err(vec![d]);
        }
        // D-SUPPLY1 Step 2: every manifest dep must resolve to a pinned version.
        if let Err(d) = Lock::verify_all_manifest_deps_locked(manifest, lock) {
            return Err(vec![d]);
        }
        if let Err(d) = enforce_provenance_policy(lock, manifest) {
            return Err(vec![d]);
        }
        let dep_dirs = build_dep_dirs_from_lock(lock, project_root, manifest)?;
        return Ok((lock.clone(), dep_dirs));
    }

    // Resolve the full dependency graph. Load an offline advisory feed only
    // when a new registry candidate needs authorization; an exact existing
    // lock remains the explicit freshness escape.
    let mut resolver = Resolver::new(project_root, existing_lock, opts, &manifest.policy);
    let (mut new_lock, dep_dirs) = resolver.resolve_manifest(manifest)?;
    if let Err(d) = enforce_provenance_policy(&new_lock, manifest) {
        return Err(vec![d]);
    }
    Lock::ensure_build_stamp(project_root, &mut new_lock);

    let semantic_update = if opts.update
        || manifest.policy.licenses.is_some()
        || !manifest.policy.exceptions.is_empty()
        || new_lock
            .packages
            .iter()
            .any(|package| matches!(&package.source, LockSource::Registry { .. }))
        || semantic_policy_needs_update(project_root, manifest)
    {
        Some(
            registry_update_rationales(project_root, &new_lock, manifest, opts)
                .map_err(|diagnostic| vec![diagnostic])?,
        )
    } else {
        None
    };

    // Write the lock file, inside the project's `.jet/` managed folder (U2).
    let lock_path = project_root.join(Syntax::UNIFIED_LOCK_FILE);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            vec![Diagnostic::error(
                "E1206",
                format!("couldn't create {}", parent.display()),
                "the lock file lives inside the project's `.jet/` managed folder".to_string(),
                format!("check write permissions: {}", e),
                None,
            )]
        })?;
    }
    let lock_str = Lock::write(&new_lock);
    let lock_bytes = match semantic_update.as_ref() {
        Some(semantic)
            if !semantic.records.is_empty()
                || !semantic.inputs.is_empty()
                || !semantic.source_maps.is_empty() =>
        {
            format!(
                "{}\n\n{}",
                lock_str.trim_end(),
                jetpack::SemanticLock::write(semantic)
            )
            .into_bytes()
        }
        _ => lock_str.into_bytes(),
    };
    write_lock_atomically(&lock_path, &lock_bytes).map_err(|e| {
        vec![Diagnostic::error(
            "E1206",
            format!("couldn't write {}", Syntax::UNIFIED_LOCK_FILE),
            "the lock file records exact package versions".to_string(),
            format!("check write permissions: {}", e),
            None,
        )]
    })?;

    Ok((new_lock, dep_dirs))
}

fn write_lock_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "lock path is not a regular file",
            ));
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "lock has no parent"))?;
    let parent_metadata = std::fs::symlink_metadata(parent)?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "lock parent is not a real directory",
        ));
    }
    let partial = parent.join(format!(
        ".{}.partial-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("lock"),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0),
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&partial, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

fn enforce_provenance_policy(
    lock: &LockFile,
    manifest: &Manifest,
) -> Result<(), Diagnostic> {
    let requirement = manifest
        .authority
        .trust
        .as_ref()
        .and_then(|trust| trust.require)
        .unwrap_or(crate::Package::ProvenanceRequirement::None);
    Lock::enforce_provenance_requirement(lock, requirement).map_err(|error| {
        Diagnostic::error(
            "E1207",
            error,
            "authority.trust.require is an explicit provenance floor; Jet will not silently downgrade it".to_string(),
            "record the required transparency or build evidence, or set `authority: { trust: { require: none } }`".to_string(),
            None,
        )
    })
}

// ──────────────────────────────────────────────
// Resolver
// ──────────────────────────────────────────────

struct Resolver<'a> {
    project_root: &'a Path,
    existing_lock: Option<&'a LockFile>,
    opts: &'a FetchOptions,
    policy: &'a crate::Package::PackagePolicy,
    advisory_policy: Option<Result<Option<Publish::AdvisoryPolicy>, Diagnostic>>,
    /// name → (version, source_dir, fingerprint, content hash, deps, provenance)
    resolved: BTreeMap<String, ResolvedPkg>,
    /// name → Vec<chain> — for E1201 blame chains.
    version_seen: HashMap<String, (String, Vec<String>)>,
    /// Selected registry entries from the graph solver.
    registry_plan: BTreeMap<String, Publish::IndexEntry>,
    /// Packages allowed to move during `jet update <package>`.
    update_scope: BTreeSet<String>,
}

struct ResolvedPkg {
    version: String,
    source: Lock::LockSource,
    locked: Option<LockedRevision>,
    fingerprint: String,
    content_hash: Option<String>,
    deps: Vec<String>,
    source_dir: PathBuf,
    provenance: Option<Lock::DependencyProvenance>,
}

impl<'a> Resolver<'a> {
    fn new(
        project_root: &'a Path,
        existing_lock: Option<&'a LockFile>,
        opts: &'a FetchOptions,
        policy: &'a crate::Package::PackagePolicy,
    ) -> Self {
        Resolver {
            project_root,
            existing_lock,
            opts,
            policy,
            advisory_policy: None,
            resolved: BTreeMap::new(),
            version_seen: HashMap::new(),
            registry_plan: BTreeMap::new(),
            update_scope: compute_update_scope(existing_lock, opts),
        }
    }

    fn resolve_manifest(
        &mut self,
        manifest: &Manifest,
    ) -> Result<(LockFile, HashMap<String, PathBuf>), Vec<Diagnostic>> {
        self.registry_plan = self.prepare_registry_plan(manifest)?;
        let root_deps: Vec<String> = manifest.dependencies.keys().cloned().collect();

        // Resolve each direct dep recursively.
        for (dep_name, spec) in &manifest.dependencies {
            let chain = vec![
                format!("{} (root)", manifest.package.name),
                dep_name.clone(),
            ];
            self.resolve_dep(dep_name, spec, self.project_root, &chain)?;
        }

        // Build the lock file.
        let mut packages: Vec<LockedPackage> = Vec::new();

        // Root package.
        packages.push(LockedPackage {
            name: manifest.package.name.clone(),
            version: manifest.package.version.clone(),
            source: LockSource::Root,
            locked: None,
            fingerprint: String::new(),
            content_hash: None,
            dependencies: root_deps.clone(),
            layer: Lock::layer_from_manifest(manifest),
            inferred_layer: None,
            // D-EFFBUDGET1: effect provenance is filled in after `jet build`
            // computes it (fetch/resolve runs before sema); see
            // `EffectBudget::update_lock_provenance`.
            effects: Vec::new(),
            effect_grants: Vec::new(),
            envelope: None,
            receipt: Default::default(),
            provenance: None,
        });

        // Dependency packages in stable order.
        for (name, pkg) in &self.resolved {
            packages.push(LockedPackage {
                name: name.clone(),
                version: pkg.version.clone(),
                source: pkg.source.clone(),
                locked: pkg.locked.clone(),
                fingerprint: pkg.fingerprint.clone(),
                content_hash: pkg.content_hash.clone(),
                dependencies: pkg.deps.clone(),
                layer: None,
                inferred_layer: None,
                effects: Vec::new(),
                effect_grants: Vec::new(),
                envelope: None,
                receipt: Default::default(),
                provenance: pkg.provenance.clone(),
            });
        }

        let mut new_lock = LockFile {
            version: Lock::LOCK_VERSION,
            packages,
            root_dependencies: root_deps,
            authority: (manifest.authority != Default::default())
                .then(|| manifest.authority.clone()),
            workspace_members: self
                .existing_lock
                .map(|lock| lock.workspace_members.clone())
                .unwrap_or_default(),
            workspace_source_digest: self
                .existing_lock
                .and_then(|lock| lock.workspace_source_digest.clone()),
            workspace_overlay_policy: self
                .existing_lock
                .map(|lock| lock.workspace_overlay_policy.clone())
                .unwrap_or_default(),
            comptime_inputs: Vec::new(),
            toolchains: Vec::new(),
            browsers: self
                .existing_lock
                .map(|lock| lock.browsers.clone())
                .unwrap_or_default(),
            source_channels: self
                .existing_lock
                .map(|lock| lock.source_channels.clone())
                .unwrap_or_default(),
            build_stamp: self
                .existing_lock
                .and_then(|lock| lock.build_stamp.clone()),
            build_contributions: self
                .existing_lock
                .map(|lock| lock.build_contributions.clone())
                .unwrap_or_default(),
        };
        self.preserve_unrelated_records(&mut new_lock);

        // Build dep_dirs map.
        let mut dep_dirs = HashMap::new();
        for (name, pkg) in &self.resolved {
            dep_dirs.insert(name.clone(), pkg.source_dir.clone());
        }

        Ok((new_lock, dep_dirs))
    }

    fn prepare_registry_plan(
        &self,
        manifest: &Manifest,
    ) -> Result<BTreeMap<String, Publish::IndexEntry>, Vec<Diagnostic>> {
        let mut roots = Vec::new();
        let mut direct = BTreeSet::new();
        let mut pending = BTreeSet::new();
        let mut owners: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (name, spec) in &manifest.dependencies {
            let DepSpec::Registry(requirement) = spec else {
                continue;
            };
            let req = VersionReq::parse(requirement).ok_or_else(|| {
                vec![registry_diagnostic(
                    name,
                    &format!("invalid version requirement `{requirement}`"),
                    "use a SemVer requirement such as `1.2.0`, `^1.2`, or `>=1.0 <2.0`",
                )]
            })?;
            roots.push(VersionConstraint {
                package: name.clone(),
                req,
                from: format!("{} (root)", manifest.package.name),
            });
            direct.insert(name.clone());
            pending.insert(name.clone());
            owners
                .entry(name.clone())
                .or_default()
                .insert(manifest.package.name.clone());
        }
        if roots.is_empty() {
            return Ok(BTreeMap::new());
        }

        let mut visited = BTreeSet::new();
        let mut candidate_map: BTreeMap<String, Vec<SolverCandidate>> = BTreeMap::new();
        let mut entry_map: BTreeMap<(String, String), Publish::IndexEntry> = BTreeMap::new();
        while let Some(name) = pending.pop_first() {
            if !visited.insert(name.clone()) {
                continue;
            }
            let update_requested = self.registry_update_requested(&name);
            let registry = if update_requested {
                Publish::resolve_publish_registry()
            } else {
                self.locked_registry_config(&name)
                    .unwrap_or_else(Publish::resolve_publish_registry)
            };
            let (all, _warnings) = Publish::resolve_and_verify_all(&registry, &name)
                .map_err(|diagnostic| {
                    vec![registry_diagnostic(
                        &name,
                        &diagnostic.what,
                        &diagnostic.fix,
                    )]
                })?;
            let locked_version = (!update_requested)
                .then(|| self.find_locked_registry_version(&name))
                .flatten();
            let mut solver_candidates = Vec::new();
            for entry in all {
                if entry.yanked && locked_version.as_deref() != Some(entry.version.as_str()) {
                    continue;
                }
                let version = SemVer::parse(&entry.version).ok_or_else(|| {
                    vec![registry_diagnostic(
                        &name,
                        &format!("registry entry has invalid SemVer `{}`", entry.version),
                        "publish a new immutable version with a valid SemVer identity",
                    )]
                })?;
                let repo = Publish::index_repo_path(&registry);
                let artifact = Publish::verify_artifact(&repo, &entry).map_err(|error| {
                    vec![registry_diagnostic(
                        &name,
                        &format!("published artifact is unavailable or corrupt: {error}"),
                        "refresh the registry mirror or publish the immutable source artifact",
                    )]
                })?;
                let dep_manifest = self.load_dep_manifest(&artifact, &name)?;
                if dep_manifest.package.name != name || dep_manifest.package.version != entry.version {
                    return Err(vec![registry_diagnostic(
                        &name,
                        "published source metadata disagrees with its registry index entry",
                        "republish a new immutable version with matching payload identity",
                    )]);
                }
                let registry_metadata = self.load_registry_metadata(
                    &artifact,
                    &name,
                    &entry.version,
                )?;
                let registry_dependencies = registry_dependency_edges(
                    &dep_manifest,
                    registry_metadata.as_ref(),
                    &name,
                )?;
                Publish::authorize_package_candidate(
                    self.policy,
                    &name,
                    &entry.version,
                    dep_manifest.package.license.as_deref(),
                    &registry.name,
                )
                .map_err(|error| {
                    let owner = owners
                        .get(&name)
                        .map(|owners| owners.iter().cloned().collect::<Vec<_>>().join(" | "))
                        .unwrap_or_else(|| manifest.package.name.clone());
                    let edge = format!("{owner} -> {}#{}", name, entry.version);
                    vec![Publish::package_policy_edge_diagnostic(
                        &owner,
                        &edge,
                        &registry.name,
                        &error,
                    )]
                })?;
                let mut dependencies = Vec::new();
                let mut rejected = BTreeMap::new();
                let mut preferred = BTreeMap::new();
                let mut strict = BTreeSet::new();
                for dependency in registry_dependencies {
                    for requirement in &dependency.requirements {
                        let req = VersionReq::parse(requirement).ok_or_else(|| {
                            vec![registry_diagnostic(
                                &dependency.name,
                                &format!(
                                    "invalid version requirement `{requirement}` in {name} {entry}",
                                    entry = entry.version
                                ),
                                "publish a package with a valid SemVer dependency requirement",
                            )]
                        })?;
                        let roles = dependency.roles.iter().cloned().collect::<Vec<_>>().join(",");
                        dependencies.push(VersionConstraint {
                            package: dependency.name.clone(),
                            req,
                            from: format!("{} {} ({roles})", name, entry.version),
                        });
                    }
                    for requirement in dependency.prefer {
                        let req = VersionReq::parse(&requirement).ok_or_else(|| {
                            vec![registry_diagnostic(
                                &dependency.name,
                                "registry dependency has an invalid prefer constraint",
                                "publish a valid SemVer requirement in registry.json",
                            )]
                        })?;
                        preferred
                            .entry(dependency.name.clone())
                            .or_insert_with(Vec::new)
                            .push(req);
                    }
                    let mut rejected_versions = BTreeSet::new();
                    for version in dependency.reject {
                        let version = SemVer::parse(&version).ok_or_else(|| {
                            vec![registry_diagnostic(
                                &dependency.name,
                                "registry dependency has an invalid reject version",
                                "publish exact SemVer reject values in registry.json",
                            )]
                        })?;
                        rejected_versions.insert(version);
                    }
                    if !rejected_versions.is_empty() {
                        rejected
                            .entry(dependency.name.clone())
                            .or_insert_with(BTreeSet::new)
                            .extend(rejected_versions);
                    }
                    if dependency.strict {
                        strict.insert(dependency.name.clone());
                    }
                    owners
                        .entry(dependency.name.clone())
                        .or_default()
                        .insert(format!("{}#{}", name, entry.version));
                    pending.insert(dependency.name);
                }
                entry_map.insert((name.clone(), entry.version.clone()), entry);
                solver_candidates.push(SolverCandidate {
                    version,
                    dependencies,
                    rejected,
                    preferred,
                    strict,
                });
            }
            candidate_map.insert(name, solver_candidates);
        }

        let locked = self
            .existing_lock
            .into_iter()
            .flat_map(|lock| lock.packages.iter())
            .filter_map(|package| {
                matches!(&package.source, LockSource::Registry { .. })
                    .then(|| (package.name.clone(), package.version.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let selected = Publish::solve_registry(
            &roots,
            &candidate_map,
            &locked,
            &self.update_scope,
            self.opts.resolution,
            &direct,
        )
        .map_err(|diagnostic| vec![diagnostic])?;
        let mut plan = BTreeMap::new();
        for (name, version) in selected {
            let entry = entry_map
                .iter()
                .find(|((candidate_name, _), candidate)| {
                    candidate_name == &name
                        && SemVer::parse(&candidate.version)
                            .is_some_and(|candidate_version| candidate_version == version)
                })
                .map(|(_, entry)| entry.clone())
                .ok_or_else(|| {
                    vec![registry_diagnostic(
                        &name,
                        "resolver selected a registry candidate with no immutable index entry",
                        "refresh the registry checkpoint and retry resolution",
                    )]
                })?;
            plan.insert(name, entry);
        }
        Ok(plan)
    }

    fn preserve_unrelated_records(&self, lock: &mut LockFile) {
        let Some(target) = self.opts.update_dep.as_deref() else {
            return;
        };
        let Some(previous) = self.existing_lock else {
            return;
        };
        let mut related = lock_closure(previous, target);
        related.extend(lock_closure(lock, target));
        for package in &mut lock.packages {
            if related.contains(&package.name) {
                continue;
            }
            if let Some(previous_package) = previous
                .packages
                .iter()
                .find(|candidate| candidate.name == package.name)
            {
                *package = previous_package.clone();
            }
        }
    }

    fn resolve_dep(
        &mut self,
        dep_name: &str,
        spec: &DepSpec,
        parent_dir: &Path,
        chain: &[String],
    ) -> Result<(), Vec<Diagnostic>> {
        if self.resolved.contains_key(dep_name) {
            // Already resolved — check for version conflict.
            let existing = &self.resolved[dep_name];
            let existing_version = existing.version.clone();
            if let Some((prev_version, prev_chain)) = self.version_seen.get(dep_name).cloned() {
                if prev_version != existing_version {
                    return Err(vec![Lock::e1201(
                        dep_name,
                        &prev_version,
                        &prev_chain,
                        &existing_version,
                        chain,
                    )]);
                }
            }
            return Ok(());
        }

        match spec {
            DepSpec::Path { path } => {
                let abs_path = if Path::new(path).is_absolute() {
                    PathBuf::from(path)
                } else {
                    normalize_path(&parent_dir.join(path))
                };

                // Load the dep's manifest.
                let dep_manifest = self.load_dep_manifest(&abs_path, dep_name)?;

                let dep_version = dep_manifest.package.version.clone();
                let dep_name_in_manifest = dep_manifest.package.name.clone();

                // Check for version conflict with another dep that has the same package name.
                if let Some((prev_ver, prev_chain)) =
                    self.version_seen.get(&dep_name_in_manifest).cloned()
                {
                    if prev_ver != dep_version {
                        return Err(vec![Lock::e1201(
                            &dep_name_in_manifest,
                            &prev_ver,
                            &prev_chain,
                            &dep_version,
                            chain,
                        )]);
                    }
                } else {
                    self.version_seen.insert(
                        dep_name_in_manifest.clone(),
                        (dep_version.clone(), chain.to_vec()),
                    );
                }

                // Compute tree hash (for fingerprint and store).
                let th = tree_hash(&abs_path);

                // Resolve transitive deps.
                let mut trans_deps = Vec::new();
                for (trans_name, trans_spec) in &dep_manifest.dependencies {
                    let mut child_chain = chain.to_vec();
                    child_chain.push(trans_name.clone());
                    self.resolve_dep(trans_name, trans_spec, &abs_path, &child_chain)?;
                    trans_deps.push(trans_name.clone());
                }

                // Compute fingerprint.
                let dep_fps: Vec<&str> = trans_deps
                    .iter()
                    .filter_map(|d| self.resolved.get(d).map(|r| r.fingerprint.as_str()))
                    .collect();
                // c129: fold the dep's frozen capability contract into its pin.
                let cap_digest = crate::Publish::ApiFreeze::project_capability_digest(&abs_path);
                let fp = Lock::compute_fingerprint(&th, &dep_fps, &cap_digest);

                // Store the path dep (copy to store for inode sharing).
                // D-CASTORE1=A: returns (path, content_hash) for lock recording.
                let (store_path, content_hash) =
                    Store::ensure_path_dep(dep_name, &dep_version, &fp, &abs_path)
                        .map_err(|d| vec![d])?;

                // Integrity floor (D-PKGSIGN1): the store entry must match its
                // recorded content hash before it is linked into the build.
                Store::verify_entry(dep_name, &store_path, &th).map_err(|d| vec![d])?;

                // Link into project build dir.
                let link_dir = self
                    .project_root
                    .join(".jet-build")
                    .join("deps")
                    .join(dep_name);
                Store::link_into_project(&store_path, &link_dir).map_err(|d| vec![d])?;
                let provenance = self.existing_provenance(
                    dep_name,
                    &dep_version,
                    &content_hash,
                );

                self.resolved.insert(
                    dep_name.to_string(),
                    ResolvedPkg {
                        version: dep_version,
                        source: LockSource::Path(path.clone()),
                        locked: None,
                        fingerprint: fp,
                        content_hash: Some(content_hash),
                        deps: trans_deps,
                        source_dir: abs_path,
                        provenance,
                    },
                );
            }

            DepSpec::Git { url, selector } => {
                // Check if git is available.
                if !git_available() {
                    return Err(vec![Lock::e1203()]);
                }

                // Determine what rev to fetch.
                let rev_to_fetch = self.resolve_git_rev(dep_name, url, selector)?;
                let clone_dir = git_cache_dir(url, &rev_to_fetch);

                // Clone/fetch if not already cached.
                if !clone_dir.is_dir() {
                    git_clone(url, &rev_to_fetch, &clone_dir)?;
                }

                // Load the dep's manifest from the cloned dir.
                let dep_manifest = self.load_dep_manifest(&clone_dir, dep_name)?;
                let dep_version = dep_manifest.package.version.clone();

                // Content hash of the checked-out source tree. This MUST use the
                // same algorithm as path deps (`SHA256::tree_hash`), because the
                // integrity floor (c122) re-hashes the *store* entry with that same
                // function and compares — git's own `HEAD^{tree}` object id is a
                // different hash space and would never match, falsely tripping
                // E1204 on every git dep. Git identity is already recorded by the
                // locked `rev`; this field is the content fingerprint.
                let git_tree_hash = tree_hash(&clone_dir);

                // Check for version conflicts.
                let dep_name_in_manifest = dep_manifest.package.name.clone();
                if let Some((prev_ver, prev_chain)) =
                    self.version_seen.get(&dep_name_in_manifest).cloned()
                {
                    if prev_ver != dep_version {
                        return Err(vec![Lock::e1201(
                            &dep_name_in_manifest,
                            &prev_ver,
                            &prev_chain,
                            &dep_version,
                            chain,
                        )]);
                    }
                } else {
                    self.version_seen.insert(
                        dep_name_in_manifest.clone(),
                        (dep_version.clone(), chain.to_vec()),
                    );
                }

                // Resolve transitive deps.
                let mut trans_deps = Vec::new();
                for (trans_name, trans_spec) in &dep_manifest.dependencies {
                    let mut child_chain = chain.to_vec();
                    child_chain.push(trans_name.clone());
                    self.resolve_dep(trans_name, trans_spec, &clone_dir, &child_chain)?;
                    trans_deps.push(trans_name.clone());
                }

                // Compute fingerprint.
                let dep_fps: Vec<&str> = trans_deps
                    .iter()
                    .filter_map(|d| self.resolved.get(d).map(|r| r.fingerprint.as_str()))
                    .collect();
                // c129: fold the dep's frozen capability contract into its pin.
                let cap_digest = crate::Publish::ApiFreeze::project_capability_digest(&clone_dir);
                let fp = Lock::compute_fingerprint(&git_tree_hash, &dep_fps, &cap_digest);

                // Store.
                // D-CASTORE1=A: returns (path, content_hash).
                let (store_path, _content_hash) =
                    Store::ensure_git_dep(dep_name, &dep_version, &fp, &clone_dir)
                        .map_err(|d| vec![d])?;

                // Integrity floor (D-PKGSIGN1): the store entry must match its
                // recorded content hash before it is linked into the build.
                Store::verify_entry(dep_name, &store_path, &git_tree_hash).map_err(|d| vec![d])?;

                // Link into project build dir.
                let link_dir = self
                    .project_root
                    .join(".jet-build")
                    .join("deps")
                    .join(dep_name);
                Store::link_into_project(&store_path, &link_dir).map_err(|d| vec![d])?;

                let locked = LockedRevision {
                    rev: rev_to_fetch.clone(),
                    tree_hash: git_tree_hash.clone(),
                    last_modified: unix_now(),
                };
                let provenance = self.existing_provenance(
                    dep_name,
                    &dep_version,
                    &git_tree_hash,
                );

                self.resolved.insert(
                    dep_name.to_string(),
                    ResolvedPkg {
                        version: dep_version,
                        source: LockSource::Git {
                            url: url.clone(),
                            selector: Lock::git_selector_str(selector),
                        },
                        locked: Some(locked),
                        fingerprint: fp,
                        content_hash: Some(git_tree_hash.clone()),
                        deps: trans_deps,
                        source_dir: clone_dir,
                        provenance,
                    },
                );
            }

            DepSpec::Registry(version_req) => {
                let update_requested = self.registry_update_requested(dep_name);
                let registry = if update_requested {
                    Publish::resolve_publish_registry()
                } else {
                    self.locked_registry_config(dep_name)
                        .unwrap_or_else(Publish::resolve_publish_registry)
                };
                let (available, _warnings) = Publish::resolve_and_verify_all(&registry, dep_name)
                    .map_err(|diagnostic| {
                        vec![registry_diagnostic(
                            dep_name,
                            &diagnostic.what,
                            &diagnostic.fix,
                        )]
                    })?;
                let requirement = VersionReq::parse(version_req).ok_or_else(|| {
                    vec![registry_diagnostic(
                        dep_name,
                        &format!("invalid version requirement `{version_req}`"),
                        "use a SemVer requirement such as `1.2.0`, `^1.2`, or `>=1.0 <2.0`",
                    )]
                })?;
                let locked_version = (!update_requested
                    && self.opts.resolution == ResolveMode::Conservative)
                    .then(|| self.find_locked_registry_version(dep_name))
                    .flatten();
                let locked_candidate = locked_version
                    .as_deref()
                    .filter(|version| {
                        SemVer::parse(version)
                            .is_some_and(|parsed| requirement.matches(&parsed))
                    })
                    .and_then(|version| available.iter().find(|entry| entry.version == version))
                    .cloned();
                let planned = self
                    .registry_plan
                    .get(dep_name)
                    .filter(|entry| {
                        SemVer::parse(&entry.version)
                            .is_some_and(|version| requirement.matches(&version))
                    })
                    .cloned();
                let selected = planned.or(locked_candidate).or_else(|| {
                        let mut candidates: Vec<(SemVer, crate::Publish::IndexEntry)> = available
                            .into_iter()
                            .filter(|entry| !entry.yanked)
                            .filter_map(|entry| {
                                let version = SemVer::parse(&entry.version)?;
                                requirement.matches(&version).then_some((version, entry))
                            })
                            .collect();
                        let versions: Vec<SemVer> =
                            candidates.iter().map(|(version, _)| version.clone()).collect();
                        let constraint = crate::Publish::VersionConstraint {
                            package: dep_name.to_string(),
                            req: requirement.clone(),
                            from: chain.first().cloned().unwrap_or_default(),
                        };
                        let mode = match self.opts.resolution {
                            ResolveMode::LowestDirect if chain.len() > 2 => ResolveMode::Latest,
                            ResolveMode::Conservative => ResolveMode::Latest,
                            mode => mode,
                        };
                        let selected_version = Publish::select_compatible(
                            dep_name,
                            &[&constraint],
                            &versions,
                            mode,
                        )
                        .ok()
                        .cloned()?;
                        let selected_entry = candidates
                            .drain(..)
                            .find(|(version, _)| *version == selected_version)
                            .map(|(_, entry)| entry);
                        selected_entry
                    });
                let Some(selected) = selected else {
                    return Err(vec![registry_diagnostic(
                        dep_name,
                        &format!("no published version satisfies `{version_req}`"),
                        "the configured registry has no compatible non-yanked artifact",
                    )]);
                };
                let reused_exact_lock = !update_requested
                    && locked_version.as_deref() == Some(selected.version.as_str());
                let source_exception = self
                    .policy
                    .exceptions
                    .iter()
                    .find(|exception| exception.matches(dep_name, &selected.version))
                    .cloned();
                if !reused_exact_lock {
                    if let Some(policy) = self.load_advisory_policy()? {
                        Publish::authorize_registry_candidate_with_source_exception(
                            policy,
                            dep_name,
                            &selected.version,
                            source_exception.as_ref(),
                        )
                        .map_err(|diagnostic| {
                            vec![contextualize_dependency_diagnostic(
                                diagnostic,
                                chain,
                                dep_name,
                                &selected.version,
                            )]
                        })?;
                    }
                }
                let registry_repo = Publish::index_repo_path(&registry);
                let artifact = Publish::verify_artifact(&registry_repo, &selected).map_err(|error| {
                    vec![registry_diagnostic(
                        dep_name,
                        &format!("published artifact is unavailable or corrupt: {error}"),
                        "refresh the registry mirror or publish the immutable source artifact",
                    )]
                })?;
                let dep_manifest = self.load_dep_manifest(&artifact, dep_name)?;
                if dep_manifest.package.name != dep_name || dep_manifest.package.version != selected.version {
                    return Err(vec![registry_diagnostic(
                        dep_name,
                        "published source metadata disagrees with its registry index entry",
                        "republish a new immutable version with matching payload identity",
                    )]);
                }
                let registry_metadata = self.load_registry_metadata(
                    &artifact,
                    dep_name,
                    &selected.version,
                )?;
                let registry_dependencies = registry_dependency_edges(
                    &dep_manifest,
                    registry_metadata.as_ref(),
                    dep_name,
                )?;
                let policy_receipt = Publish::authorize_package_candidate(
                    self.policy,
                    dep_name,
                    &selected.version,
                    dep_manifest.package.license.as_deref(),
                    &registry.name,
                )
                .map_err(|error| {
                    vec![Publish::package_policy_edge_diagnostic(
                        &dependency_owner(chain),
                        &dependency_edge(chain, dep_name, &selected.version),
                        &registry.name,
                        &error,
                    )]
                })?;
                if selected.content_hash.is_empty() || selected.fingerprint.is_empty() {
                    return Err(vec![registry_diagnostic(
                        dep_name,
                        "published registry metadata has no complete source identity",
                        "republish the package with its source hash and plan fingerprint",
                    )]);
                }
                let dep_version = selected.version.clone();
                let content_hash = selected.content_hash.clone();
                let publisher = (!selected.public_key.is_empty() && !selected.signature.is_empty())
                    .then(|| format!("ed25519:{}", selected.public_key));
                if let Some((prev_ver, prev_chain)) =
                    self.version_seen.get(dep_name).cloned()
                {
                    if prev_ver != dep_version {
                        return Err(vec![Lock::e1201(
                            dep_name,
                            &prev_ver,
                            &prev_chain,
                            &dep_version,
                            chain,
                        )]);
                    }
                } else {
                    self.version_seen.insert(
                        dep_name.to_string(),
                        (dep_version.clone(), chain.to_vec()),
                    );
                }

                let mut trans_specs = Vec::new();
                for (trans_name, trans_spec) in &dep_manifest.dependencies {
                    if let DepSpec::Registry(requirement) = trans_spec {
                        if let Some(metadata) = registry_metadata.as_ref() {
                            if let Some(dependency) = registry_dependencies
                                .iter()
                                .find(|dependency| dependency.name == *trans_name)
                            {
                                trans_specs.push((
                                    trans_name.clone(),
                                    DepSpec::Registry(
                                        dependency
                                            .requirements
                                            .first()
                                            .cloned()
                                            .unwrap_or_else(|| "*".to_string()),
                                    ),
                                ));
                            } else if metadata.contains_dependency(trans_name) {
                                // A dev/test-only edge is retained in metadata
                                // but is outside a production install closure.
                            } else {
                                trans_specs.push((
                                    trans_name.clone(),
                                    DepSpec::Registry(requirement.clone()),
                                ));
                            }
                        } else {
                            trans_specs.push((
                                trans_name.clone(),
                                DepSpec::Registry(requirement.clone()),
                            ));
                        }
                    } else {
                        trans_specs.push((trans_name.clone(), trans_spec.clone()));
                    }
                }
                for dependency in &registry_dependencies {
                    if !dep_manifest.dependencies.contains_key(&dependency.name) {
                        trans_specs.push((
                            dependency.name.clone(),
                            DepSpec::Registry(
                                dependency
                                    .requirements
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| "*".to_string()),
                            ),
                        ));
                    }
                }

                let mut trans_deps = Vec::new();
                for (trans_name, trans_spec) in trans_specs {
                    let mut child_chain = chain.to_vec();
                    child_chain.push(trans_name.clone());
                    self.resolve_dep(&trans_name, &trans_spec, &artifact, &child_chain)?;
                    if !trans_deps.contains(&trans_name) {
                        trans_deps.push(trans_name);
                    }
                }
                let dep_fps: Vec<&str> = trans_deps
                    .iter()
                    .filter_map(|dep| self.resolved.get(dep).map(|pkg| pkg.fingerprint.as_str()))
                    .collect();
                let hangar_references = trans_deps
                    .iter()
                    .filter_map(|dep| {
                        let pkg = self.resolved.get(dep)?;
                        if !matches!(&pkg.source, LockSource::Registry { .. }) {
                            return None;
                        }
                        jetpack::Envelope::try_output_hash_of(&pkg.source_dir.to_string_lossy())
                            .ok()
                    })
                    .collect::<Vec<_>>();
                let cap_digest = crate::Publish::ApiFreeze::project_capability_digest(&artifact);
                let fp = Lock::compute_fingerprint(&content_hash, &dep_fps, &cap_digest);
                let advisory_receipt = if reused_exact_lock {
                    None
                } else {
                    self.advisory_policy
                        .as_ref()
                        .and_then(|result| result.as_ref().ok())
                        .and_then(Option::as_ref)
                        .map(|policy| &policy.receipt)
                };
                let store_path = ingest_registry_artifact(
                    &registry,
                    &selected,
                    &artifact,
                    &hangar_references,
                    advisory_receipt,
                    Some(&policy_receipt),
                    source_exception.as_ref(),
                )
                .map_err(|error| {
                    vec![registry_diagnostic(
                        dep_name,
                        &error,
                        "repair the canonical Jetpack Hangar or retry the verified registry ingest",
                    )]
                })?;
                Store::verify_entry(dep_name, &store_path, &content_hash)
                    .map_err(|diagnostic| vec![diagnostic])?;
                let link_dir = self
                    .project_root
                    .join(".jet-build")
                    .join("deps")
                    .join(dep_name);
                Store::copy_into_project(&store_path, &link_dir)
                    .map_err(|diagnostic| vec![diagnostic])?;
                let mut provenance = self
                    .existing_provenance(dep_name, &dep_version, &content_hash)
                    .unwrap_or_default();
                if let Some(publisher) = publisher {
                    provenance.publisher = Some(publisher);
                }
                let provenance = (provenance.transparency.is_some()
                    || provenance.publisher.is_some()
                    || provenance.build.is_some())
                .then_some(provenance);
                self.resolved.insert(
                    dep_name.to_string(),
                    ResolvedPkg {
                        version: dep_version,
                        source: LockSource::Registry {
                            registry: registry.name,
                            reference: format!("{}#{}", selected.name, selected.version),
                            output: store_path.to_string_lossy().into_owned(),
                            source_hash: content_hash.clone(),
                            repository: registry.url,
                            authority: "jet-registry-index".to_string(),
                            tier: selected.tier.label().to_string(),
                            gate_status: selected.gate_status.summary(),
                        },
                        locked: None,
                        fingerprint: fp,
                        content_hash: Some(content_hash),
                        deps: trans_deps,
                        source_dir: store_path,
                        provenance,
                    },
                );
            }
        }

        Ok(())
    }

    fn load_dep_manifest(&self, dir: &Path, dep_name: &str) -> Result<Manifest, Vec<Diagnostic>> {
        load_dep_manifest(dir, dep_name)
    }

    fn load_registry_metadata(
        &self,
        artifact: &Path,
        dep_name: &str,
        version: &str,
    ) -> Result<Option<Publish::RegistryPackageMetadata>, Vec<Diagnostic>> {
        Publish::read_registry_package_metadata(artifact, dep_name, version).map_err(|error| {
            vec![registry_diagnostic(
                dep_name,
                &format!("registry dependency metadata is invalid: {error}"),
                "republish the immutable artifact with a valid registry.json record",
            )]
        })
    }
}

fn registry_dependency_edges(
    manifest: &Manifest,
    metadata: Option<&Publish::RegistryPackageMetadata>,
    package: &str,
) -> Result<Vec<Publish::RegistryDependency>, Vec<Diagnostic>> {
    if let Some(metadata) = metadata {
        for (name, spec) in &manifest.dependencies {
            if matches!(spec, DepSpec::Registry(_)) && !metadata.contains_dependency(name) {
                return Err(vec![registry_diagnostic(
                    package,
                    &format!("registry.json omits declared dependency `{name}`"),
                    "publish registry.json with every registry dependency in package.jet",
                )]);
            }
        }
        return Ok(metadata.active_dependencies());
    }

    Ok(manifest
        .dependencies
        .iter()
        .filter_map(|(name, spec)| match spec {
            DepSpec::Registry(requirement) => Some(Publish::RegistryDependency {
                name: name.clone(),
                requirements: vec![requirement.clone()],
                roles: BTreeSet::from(["normal".to_string()]),
                prefer: Vec::new(),
                reject: BTreeSet::new(),
                strict: false,
                enabled_by_default: true,
            }),
            _ => None,
        })
        .collect())
}

fn load_dep_manifest(dir: &Path, dep_name: &str) -> Result<Manifest, Vec<Diagnostic>> {
    match crate::Manifest::load(dir) {
        None => Err(vec![Diagnostic::error(
            "E1206",
            format!(
                "dependency `{}` has no `{}`",
                dep_name,
                crate::Syntax::PACKAGE_FILE
            ),
            format!(
                "every Jet package must have a `{}` manifest",
                crate::Syntax::PACKAGE_FILE
            ),
            format!(
                "add a `{}` to `{}`",
                crate::Syntax::PACKAGE_FILE,
                dir.display()
            ),
            None,
        )]),
        Some(Err(d)) => Err(vec![d]),
        Some(Ok(m)) => Ok(m),
    }
}

impl<'a> Resolver<'a> {
    fn resolve_git_rev(
        &self,
        dep_name: &str,
        url: &str,
        selector: &GitSelector,
    ) -> Result<String, Vec<Diagnostic>> {
        match selector {
            GitSelector::Rev(r) => Ok(r.clone()),
            GitSelector::Tag(t) if t != "@latest" => {
                // If we have an existing lock and not updating, use locked rev.
                if !self.dependency_update_requested(dep_name) {
                    if let Some(locked_rev) = self.find_locked_rev(dep_name) {
                        return Ok(locked_rev);
                    }
                }
                // Resolve the tag to a specific commit via ls-remote.
                git_resolve_ref(url, t).map_err(|d| vec![d])
            }
            GitSelector::Branch(b) | GitSelector::Tag(b) => {
                // Moving selector: always re-resolve if updating.
                if !self.dependency_update_requested(dep_name) {
                    if let Some(locked_rev) = self.find_locked_rev(dep_name) {
                        return Ok(locked_rev);
                    }
                }
                git_resolve_ref(url, b).map_err(|d| vec![d])
            }
        }
    }

    fn dependency_update_requested(&self, dep_name: &str) -> bool {
        self.opts.update
            && (self.opts.update_dep.is_none() || self.update_scope.contains(dep_name))
    }

    fn registry_update_requested(&self, dep_name: &str) -> bool {
        self.dependency_update_requested(dep_name)
    }

    fn load_advisory_policy(
        &mut self,
    ) -> Result<Option<&Publish::AdvisoryPolicy>, Vec<Diagnostic>> {
        if self.advisory_policy.is_none() {
            self.advisory_policy = Some(Publish::load_advisory_policy(self.project_root));
        }
        match self.advisory_policy.as_ref().expect("advisory policy is loaded") {
            Ok(policy) => Ok(policy.as_ref()),
            Err(diagnostic) => Err(vec![diagnostic.clone()]),
        }
    }

    fn locked_registry_config(&self, dep_name: &str) -> Option<Publish::RegistryConfig> {
        let package = self.existing_lock?.packages.iter().find(|package| {
            package.name == dep_name && matches!(&package.source, LockSource::Registry { .. })
        })?;
        let LockSource::Registry {
            registry,
            repository,
            tier,
            ..
        } = &package.source
        else {
            return None;
        };
        let tier = Publish::RegistryTier::parse(tier).unwrap_or(Publish::RegistryTier::Core);
        Some(Publish::RegistryConfig {
            name: registry.clone(),
            url: repository.clone(),
            mirror: false,
            require_signed: tier == Publish::RegistryTier::Community,
            tier,
        })
    }

    fn find_locked_registry_version(&self, dep_name: &str) -> Option<String> {
        self.existing_lock?
            .packages
            .iter()
            .find(|package| {
                package.name == dep_name && matches!(&package.source, LockSource::Registry { .. })
            })
            .map(|package| package.version.clone())
    }

    fn existing_provenance(
        &self,
        dep_name: &str,
        version: &str,
        content_hash: &str,
    ) -> Option<Lock::DependencyProvenance> {
        self.existing_lock?
            .packages
            .iter()
            .find(|package| {
                package.name == dep_name
                    && package.version == version
                    && package.provenance_report().integrity.value == content_hash
            })
            .and_then(|package| package.provenance.clone())
    }

    fn find_locked_rev(&self, dep_name: &str) -> Option<String> {
        let lock = self.existing_lock?;
        lock.packages
            .iter()
            .find(|p| p.name == dep_name)
            .and_then(|p| p.locked.as_ref())
            .map(|l| l.rev.clone())
    }
}

fn compute_update_scope(
    existing_lock: Option<&LockFile>,
    opts: &FetchOptions,
) -> BTreeSet<String> {
    if !opts.update {
        return BTreeSet::new();
    }
    match opts.update_dep.as_deref() {
        None => existing_lock
            .into_iter()
            .flat_map(|lock| lock.packages.iter().map(|package| package.name.clone()))
            .collect(),
        Some(target) => {
            let mut scope = existing_lock
                .map(|lock| lock_closure(lock, target))
                .unwrap_or_default();
            scope.insert(target.to_string());
            scope
        }
    }
}

fn lock_closure(lock: &LockFile, target: &str) -> BTreeSet<String> {
    let mut scope = BTreeSet::new();
    let mut pending = vec![target.to_string()];
    while let Some(name) = pending.pop() {
        if !scope.insert(name.clone()) {
            continue;
        }
        if let Some(package) = lock.packages.iter().find(|package| package.name == name) {
            pending.extend(package.dependencies.iter().cloned());
        }
    }
    scope
}

fn registry_update_rationales(
    project_root: &Path,
    lock: &LockFile,
    manifest: &Manifest,
    opts: &FetchOptions,
) -> Result<jetpack::SemanticLock::SemanticLockFile, Diagnostic> {
    let mut packages = lock
        .packages
        .iter()
        .filter(|package| matches!(&package.source, LockSource::Registry { .. }))
        .collect::<Vec<_>>();
    let mut semantic = jetpack::SemanticLock::load(project_root).unwrap_or_default();
    semantic.source_maps = manifest
        .policy
        .source_maps
        .iter()
        .map(|(pattern, sources)| jetpack::SemanticLock::SourceMapEntry {
            pattern: pattern.clone(),
            sources: sources.clone(),
        })
        .collect();
    if packages.is_empty() {
        return Ok(semantic);
    }
    if let Some(target) = opts.update_dep.as_deref() {
        let scope = lock_closure(lock, target);
        packages.retain(|package| scope.contains(&package.name));
    }
    if packages.is_empty() {
        return Ok(semantic);
    }

    for package in packages {
        let LockSource::Registry {
            registry,
            reference,
            output,
            source_hash,
            repository,
            ..
        } = &package.source
        else {
            continue;
        };
        let dep_manifest = load_dep_manifest(Path::new(output), &package.name)
            .map_err(|mut diagnostics| {
                diagnostics.pop().unwrap_or_else(|| {
                    Diagnostic::error(
                        "E1207",
                        format!(
                            "registry dependency `{}` has no readable installed manifest",
                            package.name
                        ),
                        "registry package policy evidence must remain explainable after ingest"
                            .to_string(),
                        "repair the canonical Hangar object or rerun `jet fetch` from the trusted registry"
                            .to_string(),
                        None,
                    )
                })
            })?;
        let policy_receipt = Publish::authorize_package_candidate(
            &manifest.policy,
            &package.name,
            &package.version,
            dep_manifest.package.license.as_deref(),
            registry,
        )
        .map_err(|error| registry_diagnostic(&package.name, &error.detail, &error.fix))?;
        let registry_metadata = Publish::read_registry_package_metadata(
            Path::new(output),
            &package.name,
            &package.version,
        )
        .map_err(|error| {
            registry_diagnostic(
                &package.name,
                &format!("registry dependency metadata is invalid: {error}"),
                "repair the canonical Hangar object or republish registry.json",
            )
        })?;
        let key = format!("registry:{registry}:{}", package.name);
        let mut record = jetpack::SemanticLock::SemanticRecord::new(
            jetpack::SemanticLock::LockIdentity {
                kind: jetpack::SemanticLock::LockRecordKind::Package,
                key,
                exact: reference.clone(),
                hash: source_hash.clone(),
                platform: jetpack::Envelope::host_platform(),
            },
            jetpack::SemanticLock::LockRationale {
                owner_package: manifest.package.name.clone(),
                reason: format!(
                    "selected `{}` with {} resolution; registry policy record; package-policy={}{}",
                    package.version,
                    opts.resolution.label(),
                    policy_receipt.summary(),
                    manifest
                        .policy
                        .exceptions
                        .iter()
                        .find(|exception| exception.matches(&package.name, &package.version))
                        .map(|exception| {
                            format!("; source-policy-exception={}", exception.summary())
                        })
                        .unwrap_or_default()
                ),
                source_ref: format!(
                    "registry:{registry};repository={}",
                    Publish::redact_registry_url(repository)
                ),
                provider: "jet-registry".to_string(),
                channel_input: opts.resolution.label().to_string(),
                exact_output: output.clone(),
                policy_fingerprint: Publish::policy_fingerprint(&manifest.policy),
                recipe_id: String::new(),
                adapter_id: "registry.pubgrub".to_string(),
                signature: String::new(),
                cache_provenance: "verified-registry".to_string(),
                update_command: opts
                    .update_dep
                    .as_deref()
                    .map(|name| format!("jet update {name}"))
                    .unwrap_or_else(|| "jet update".to_string()),
            },
        );
        if let Some(metadata) = registry_metadata {
            record.future_fields.insert(
                "dependency-metadata".to_string(),
                metadata.canonical().to_string(),
            );
        }
        jetpack::SemanticLock::selective_update(&mut semantic, record);
    }
    jetpack::SemanticLock::revalidate(&semantic).map_err(|issues| {
        Diagnostic::error(
            "E1206",
            "couldn't record the registry resolution rationale".to_string(),
            "the package lock was resolved, but its update explanation failed semantic lock validation".to_string(),
            issues
                .iter()
                .map(jetpack::SemanticLock::ValidationIssue::message)
                .collect::<Vec<_>>()
                .join("; "),
            None,
        )
    })?;
    Ok(semantic)
}

fn semantic_policy_needs_update(project_root: &Path, manifest: &Manifest) -> bool {
    let mut expected_maps = manifest
        .policy
        .source_maps
        .iter()
        .map(|(pattern, sources)| jetpack::SemanticLock::SourceMapEntry {
            pattern: pattern.clone(),
            sources: sources.clone(),
        })
        .collect::<Vec<_>>();
    expected_maps.sort_by(|left, right| left.pattern.cmp(&right.pattern));
    let semantic = jetpack::SemanticLock::load(project_root).unwrap_or_default();
    if semantic.source_maps != expected_maps {
        return true;
    }
    let fingerprint = Publish::policy_fingerprint(&manifest.policy);
    semantic.records.iter().any(|record| {
        record.identity.kind == jetpack::SemanticLock::LockRecordKind::Package
            && record.identity.key.starts_with("registry:")
            && record
                .rationales
                .first()
                .map(|rationale| rationale.policy_fingerprint.as_str())
                != Some(fingerprint.as_str())
    })
}

fn dependency_owner(chain: &[String]) -> String {
    if chain.len() > 1 {
        chain[..chain.len() - 1].join(" -> ")
    } else {
        "package root".to_string()
    }
}

fn dependency_edge(chain: &[String], package: &str, version: &str) -> String {
    format!("{} -> {package}#{version}", dependency_owner(chain))
}

fn contextualize_dependency_diagnostic(
    mut diagnostic: Diagnostic,
    chain: &[String],
    package: &str,
    version: &str,
) -> Diagnostic {
    let edge = dependency_edge(chain, package, version);
    diagnostic.what = format!("dependency edge `{edge}` was rejected: {}", diagnostic.what);
    diagnostic.why = format!(
        "package `{}` owns this source decision; {}",
        dependency_owner(chain),
        diagnostic.why
    );
    diagnostic
}

fn registry_diagnostic(name: &str, what: &str, fix: &str) -> Diagnostic {
    Diagnostic::error(
        "E1207",
        format!("registry dependency `{name}` cannot be resolved: {what}"),
        "registry package identity is source-backed and must be verified before it enters the lock".to_string(),
        fix.to_string(),
        None,
    )
}

/// Move a verified registry source tree into the canonical Jetpack Hangar.
/// Registry verification happens before this call; Hangar then supplies the
/// durable, content-addressed project input used by linking and locked fetch.
fn ingest_registry_artifact(
    registry: &Publish::RegistryConfig,
    entry: &Publish::IndexEntry,
    artifact: &Path,
    references: &[String],
    advisory_receipt: Option<&Publish::AdvisoryReceipt>,
    package_policy: Option<&Publish::PackagePolicyReceipt>,
    source_exception: Option<&PackagePolicyException>,
) -> Result<PathBuf, String> {
    let roots = jetpack::Store::resolve();
    let policy = format!(
        "registry={};tier={};gate-status={}",
        registry.name,
        entry.tier.label(),
        entry.gate_status.summary(),
    );
    let provenance = format!(
        "registry={};repository={};package={}#{};source-hash={};fingerprint={};publisher={};index-signature={}",
        registry.name,
        Publish::redact_registry_url(&registry.url),
        entry.name,
        entry.version,
        entry.content_hash,
        entry.fingerprint,
        entry.public_key,
        entry.signature,
    );
    let referrer_receipt = format!(
        "subject={};sbom=verified;signature-binding=verified;provenance=verified;reproducibility=verified",
        entry.content_hash
    );
    let policy = format!("{policy};oci-referrers={referrer_receipt}");
    let provenance = format!("{provenance};oci-referrers={referrer_receipt}");
    let advisory = advisory_receipt.map(|receipt| {
        format!(
            "sequence={},digest={},key={},maturity={}s",
            receipt.sequence, receipt.digest, receipt.key_id, receipt.maturity_seconds
        )
    });
    let package_policy = package_policy.map(Publish::PackagePolicyReceipt::summary);
    let source_exception = source_exception.map(PackagePolicyException::summary);
    let policy = package_policy
        .as_deref()
        .map(|receipt| format!("{policy};package-policy={receipt}"))
        .unwrap_or(policy);
    let provenance = package_policy
        .as_deref()
        .map(|receipt| format!("{provenance};package-policy={receipt}"))
        .unwrap_or(provenance);
    let policy = source_exception
        .as_deref()
        .map(|exception| format!("{policy};source-policy-exception={exception}"))
        .unwrap_or(policy);
    let provenance = source_exception
        .as_deref()
        .map(|exception| format!("{provenance};source-policy-exception={exception}"))
        .unwrap_or(provenance);
    let policy = advisory
        .as_deref()
        .map(|receipt| format!("{policy};advisory-feed={receipt}"))
        .unwrap_or(policy);
    let provenance = advisory
        .as_deref()
        .map(|receipt| format!("{provenance};advisory-feed={receipt}"))
        .unwrap_or(provenance);
    let reference = format!("registry:{}:{}#{}", registry.name, entry.name, entry.version);
    let existing = jetpack::Store::list(&roots)
        .into_iter()
        .find(|existing| {
            existing.name.as_str() == entry.name.as_str()
                && existing.version.as_str() == entry.version.as_str()
                && existing.reference.as_str() == reference.as_str()
                && existing.cache_identity.source_fingerprint.as_str()
                    == entry.content_hash.as_str()
        });
    let provenance = if advisory_receipt.is_none() {
        existing
            .as_ref()
            .and_then(|existing| {
                existing
                    .envelope
                    .provenance
                    .split(';')
                    .find(|field| field.starts_with("advisory-feed="))
            })
            .map(|receipt| format!("{provenance};{receipt}"))
            .unwrap_or(provenance)
    } else {
        provenance
    };
    let references = existing
        .as_ref()
        .map(|existing| existing.references.clone())
        .unwrap_or_else(|| references.to_vec());
    let policy_fingerprint = existing
        .as_ref()
        .filter(|_| advisory_receipt.is_none() && package_policy.is_none())
        .map(|existing| existing.cache_identity.policy_fingerprint.clone())
        .unwrap_or_else(|| format!("sha256-{}", crate::SHA256::sha256_hex(policy.as_bytes())));
    let provenance = existing
        .as_ref()
        .filter(|_| advisory_receipt.is_none() && package_policy.is_none())
        .map(|existing| existing.envelope.provenance.clone())
        .unwrap_or(provenance);
    let provenance = if provenance.contains("oci-referrers=") {
        provenance
    } else {
        format!("{provenance};oci-referrers={referrer_receipt}")
    };
    let request = jetpack::Store::IngestRequest {
        name: entry.name.clone(),
        version: entry.version.clone(),
        reference,
        cache_identity: jetpack::Store::CacheIdentity {
            source_fingerprint: entry.content_hash.clone(),
            recipe_fingerprint: entry.fingerprint.clone(),
            policy_fingerprint,
            platform: jetpack::Envelope::host_platform(),
        },
        references,
        outputs: BTreeMap::from([(String::from("out"), artifact.to_path_buf())]),
        signature: String::new(),
        provenance,
        platform_artifact_kind: String::new(),
    };
    let ingested = jetpack::Store::ingest_tree(&roots, &request)
        .map_err(|error| format!("{} ({})", error.what(), error.code()))?;
    Ok(PathBuf::from(ingested.entry.out))
}

// ──────────────────────────────────────────────
// Build dep_dirs from existing lock (--locked mode)
// ──────────────────────────────────────────────

fn build_dep_dirs_from_lock(
    lock: &LockFile,
    project_root: &Path,
    manifest: &Manifest,
) -> Result<HashMap<String, PathBuf>, Vec<Diagnostic>> {
    let mut dep_dirs = HashMap::new();
    for (dep_name, spec) in &manifest.dependencies {
        let source_dir = match spec {
            DepSpec::Path { path } => normalize_path(&project_root.join(path)),
            DepSpec::Git { .. } => {
                // Find the locked rev and use the git cache dir.
                let locked_pkg = lock.packages.iter().find(|p| p.name == *dep_name);
                if let Some(pkg) = locked_pkg {
                    if let Some(rev) = pkg.locked.as_ref().map(|l| &l.rev) {
                        if let LockSource::Git { url, .. } = &pkg.source {
                            git_cache_dir(url, rev)
                        } else {
                            project_root.to_path_buf()
                        }
                    } else {
                        project_root.to_path_buf()
                    }
                } else {
                    project_root.to_path_buf()
                }
            }
            DepSpec::Registry(version_req) => {
                let requirement = VersionReq::parse(version_req).ok_or_else(|| {
                    vec![registry_diagnostic(
                        dep_name,
                        &format!("invalid locked version requirement `{version_req}`"),
                        "use a valid SemVer requirement before creating a lock",
                    )]
                })?;
                let Some(locked) = lock.packages.iter().find(|package| {
                    package.name == *dep_name
                        && SemVer::parse(&package.version)
                            .is_some_and(|version| requirement.matches(&version))
                        && matches!(&package.source, LockSource::Registry { .. })
                }) else {
                    return Err(vec![registry_diagnostic(
                        dep_name,
                        "locked registry dependency has no exact package record",
                        "run `jet fetch` online to select and record an immutable registry version",
                    )]);
                };
                let LockSource::Registry {
                    registry,
                    reference,
                    repository,
                    source_hash,
                    tier,
                    gate_status,
                    ..
                } = &locked.source else {
                    unreachable!("registry package predicate guarantees registry source")
                };
                if crate::Publish::redact_registry_url(repository) != *repository {
                    return Err(vec![registry_diagnostic(
                        dep_name,
                        "locked registry repository contains embedded credentials",
                        "regenerate the lock from a credential-free registry endpoint; credentials never belong in `.jet/lock`",
                    )]);
                }
                let config = crate::Publish::RegistryConfig {
                    name: registry.clone(),
                    url: repository.clone(),
                    mirror: false,
                    require_signed: false,
                    tier: crate::Publish::RegistryTier::parse(tier)
                        .unwrap_or(crate::Publish::RegistryTier::Core),
                };
                let repo = crate::Publish::ensure_local_index_clone(&config)
                    .map_err(|diagnostic| vec![diagnostic])?;
                let entry = crate::Publish::Index::find_entry(&repo, dep_name, &locked.version)
                    .map_err(|error| {
                        vec![registry_diagnostic(
                            dep_name,
                            &format!("locked registry index could not be read: {error}"),
                            "restore the pinned registry mirror; locked mode never downloads a new index",
                        )]
                    })?
                    .ok_or_else(|| {
                        vec![registry_diagnostic(
                            dep_name,
                            "locked registry index has no pinned version",
                            "restore the exact offline registry checkpoint or run `jet fetch` online",
                        )]
                    })?;
                let expected_reference = format!("{}#{}", dep_name, locked.version);
                if reference != &expected_reference {
                    return Err(vec![registry_diagnostic(
                        dep_name,
                        "locked registry reference does not name the locked package version",
                        "regenerate the lock from the trusted registry checkpoint",
                    )]);
                }
                if source_hash.is_empty() {
                    return Err(vec![registry_diagnostic(
                        dep_name,
                        "locked registry package has no source hash",
                        "regenerate the lock from a registry entry with immutable source identity",
                    )]);
                }
                if entry.content_hash != *source_hash {
                    return Err(vec![registry_diagnostic(
                        dep_name,
                        "locked registry content hash disagrees with the local index",
                        "refresh the lock from a trusted registry checkpoint",
                    )]);
                }
                if entry.tier.label() != tier || entry.gate_status.summary() != *gate_status {
                    return Err(vec![registry_diagnostic(
                        dep_name,
                        "locked registry tier or gate status disagrees with the live index",
                        "regenerate the lock from the trusted registry checkpoint",
                    )]);
                }
                let all_entries = crate::Publish::verify_registry_package(
                    &repo,
                    &config.name,
                    dep_name,
                )
                .map_err(|diagnostic| vec![diagnostic])?;
                crate::Publish::verify_entry_tier(&entry)
                    .map_err(|diagnostic| vec![diagnostic])?;
                crate::Publish::verify_index_entry(
                    &all_entries,
                    &entry,
                    config.require_signed || entry.tier == crate::Publish::RegistryTier::Community,
                    &config.name,
                )
                .map_err(|diagnostic| vec![diagnostic])?;
                let artifact = crate::Publish::verify_artifact(&repo, &entry).map_err(|error| {
                    vec![registry_diagnostic(
                        dep_name,
                        &format!("locked registry artifact is unavailable: {error}"),
                        "restore the artifact in the local registry mirror; locked mode stays offline",
                    )]
                })?;
                let dep_manifest = load_dep_manifest(&artifact, dep_name)?;
                if dep_manifest.package.name != *dep_name
                    || dep_manifest.package.version != locked.version
                {
                    return Err(vec![registry_diagnostic(
                        dep_name,
                        "locked source metadata disagrees with its registry identity",
                        "refresh the lock from a trusted immutable registry artifact",
                    )]);
                }
                let policy_receipt = crate::Publish::authorize_package_candidate(
                    &manifest.policy,
                    dep_name,
                    &locked.version,
                    dep_manifest.package.license.as_deref(),
                    &config.name,
                )
                .map_err(|error| {
                    let owner = manifest.package.name.clone();
                    let edge = format!("{owner} -> {}#{}", dep_name, locked.version);
                    vec![crate::Publish::package_policy_edge_diagnostic(
                        &owner,
                        &edge,
                        &config.name,
                        &error,
                    )]
                })?;
                let source_exception = manifest
                    .policy
                    .exceptions
                    .iter()
                    .find(|exception| exception.matches(dep_name, &locked.version));
                ingest_registry_artifact(
                    &config,
                    &entry,
                    &artifact,
                    &[],
                    None,
                    Some(&policy_receipt),
                    source_exception,
                )
                .map_err(|error| {
                    vec![registry_diagnostic(
                        dep_name,
                        &error,
                        "repair the canonical Jetpack Hangar or retry the verified registry ingest",
                    )]
                })?
            }
        };
        dep_dirs.insert(dep_name.clone(), source_dir);
    }
    Ok(dep_dirs)
}

// ──────────────────────────────────────────────
// Git helpers
// ──────────────────────────────────────────────

pub fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn git_cache_dir(url: &str, rev: &str) -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let url_hash = crate::SHA256::sha256_hex(url.as_bytes());
    PathBuf::from(home)
        .join(".jet")
        .join("git-cache")
        .join(&url_hash[..16])
        .join(&rev[..16.min(rev.len())])
}

fn git_resolve_ref(url: &str, refname: &str) -> Result<String, Diagnostic> {
    let out = Command::new("git")
        .args(["ls-remote", "--exit-code", url, refname])
        .output()
        .map_err(|_| Lock::e1203())?;
    if !out.status.success() {
        return Err(Diagnostic::error(
            "E1203",
            format!("couldn't resolve git ref `{}` at `{}`", refname, url),
            "the git ref may not exist or the URL may be unreachable".to_string(),
            format!(
                "check the URL and ref name in {}",
                crate::Syntax::PACKAGE_FILE
            ),
            None,
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let rev = stdout.split_whitespace().next().unwrap_or("").to_string();
    if rev.is_empty() {
        return Err(Diagnostic::error(
            "E1203",
            format!("git ref `{}` not found at `{}`", refname, url),
            "the tag or branch name must exist in the remote repository".to_string(),
            format!("check the ref spelling in {}", crate::Syntax::PACKAGE_FILE),
            None,
        ));
    }
    Ok(rev)
}

fn git_clone(url: &str, rev: &str, dest: &Path) -> Result<(), Vec<Diagnostic>> {
    std::fs::create_dir_all(dest).map_err(|e| {
        vec![Diagnostic::error(
            "E1206",
            format!("couldn't create git cache directory: {}", e),
            "the git cache lives in ~/.jet/git-cache/".to_string(),
            "check disk space and permissions".to_string(),
            None,
        )]
    })?;

    // Clone the repository and check out the specific revision.
    let tmp = dest.with_extension("_tmp");
    let _ = std::fs::remove_dir_all(&tmp);

    let clone_ok = Command::new("git")
        .args(["clone", "--quiet", url, tmp.to_str().unwrap_or(".")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !clone_ok {
        return Err(vec![Diagnostic::error(
            "E1203",
            format!("failed to clone `{}`", url),
            "git clone returned an error".to_string(),
            "check the git URL and your network connection".to_string(),
            None,
        )]);
    }

    let checkout_ok = Command::new("git")
        .args([
            "-C",
            tmp.to_str().unwrap_or("."),
            "checkout",
            "--quiet",
            rev,
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !checkout_ok {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(vec![Diagnostic::error(
            "E1203",
            format!("couldn't check out revision `{}` from `{}`", rev, url),
            "git checkout returned an error".to_string(),
            "check that the revision exists in the repository".to_string(),
            None,
        )]);
    }

    std::fs::rename(&tmp, dest).map_err(|e| {
        vec![Diagnostic::error(
            "E1206",
            format!("couldn't move cloned repo into place: {}", e),
            "filesystem rename failed".to_string(),
            "check disk space and permissions".to_string(),
            None,
        )]
    })?;

    Ok(())
}

fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
