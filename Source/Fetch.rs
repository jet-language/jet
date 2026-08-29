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
use std::net::{IpAddr, ToSocketAddrs};
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
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "lock has no parent")
    })?;
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

fn enforce_provenance_policy(lock: &LockFile, manifest: &Manifest) -> Result<(), Diagnostic> {
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

fn foreign_provider_diagnostic(error: &jetpack::Provider::ProviderError) -> Diagnostic {
    Diagnostic::error(
        error.code().unwrap_or("E1256"),
        "couldn't realize the foreign package provider".to_string(),
        format!("the verified provider projection failed: {error:?}"),
        "provide a pinned, hash-verified provider artifact with its generated binding, then run `jet fetch` again".to_string(),
        None,
    )
}

fn foreign_dependency_diagnostic(dep_name: &str, detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1256",
        format!("foreign dependency `{dep_name}` has no valid provider projection"),
        detail.to_string(),
        "run `jet fetch` with the pinned provider artifact available; Jet will not invent or bypass a binding".to_string(),
        None,
    )
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
    /// Verified package-provider projections for foreign namespace deps.
    foreign: BTreeMap<String, jetpack::Foreign::Realization>,
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
    envelope: Option<Lock::LockEnvelope>,
    receipt: Option<String>,
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
            foreign: BTreeMap::new(),
        }
    }

    fn resolve_manifest(
        &mut self,
        manifest: &Manifest,
    ) -> Result<(LockFile, HashMap<String, PathBuf>), Vec<Diagnostic>> {
        self.realize_foreign_dependencies(manifest)?;
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
            required_effects: Vec::new(),
            granted_effects: Vec::new(),
            denied_effects: Vec::new(),
            effect_authority: None,
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
                required_effects: Vec::new(),
                granted_effects: Vec::new(),
                denied_effects: Vec::new(),
                effect_authority: None,
                provenance: pkg.provenance.clone(),
                envelope: pkg.envelope.clone(),
                receipt: pkg.receipt.clone(),
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
            build_stamp: self.existing_lock.and_then(|lock| lock.build_stamp.clone()),
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

    fn realize_foreign_dependencies(&mut self, manifest: &Manifest) -> Result<(), Vec<Diagnostic>> {
        if !manifest
            .dependencies
            .values()
            .any(|spec| matches!(spec, DepSpec::Foreign { .. }))
        {
            return Ok(());
        }

        let roots = jetpack::Store::resolve();
        let store_dir = roots.hangar_dir();
        let fixtures = jetpack::Provider::fixtures_from_env(None);
        let context = jetpack::Provider::Ctx {
            fixtures: fixtures.as_deref(),
            store_dir: &store_dir,
            offline: false,
            project_dir: Some(self.project_root),
            nix_index: None,
            nix_roots: None,
        };
        let realized = jetpack::Foreign::realize_manifest_dependencies(
            &roots,
            self.project_root,
            manifest,
            &context,
        )
        .map_err(|error| vec![foreign_provider_diagnostic(&error)])?;
        self.foreign = realized
            .into_iter()
            .map(|item| (item.name.clone(), item))
            .collect();
        Ok(())
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
            let (all, _warnings) =
                Publish::resolve_and_verify_all(&registry, &name).map_err(|diagnostic| {
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
                if dep_manifest.package.name != name
                    || dep_manifest.package.version != entry.version
                {
                    return Err(vec![registry_diagnostic(
                        &name,
                        "published source metadata disagrees with its registry index entry",
                        "republish a new immutable version with matching payload identity",
                    )]);
                }
                let registry_metadata =
                    self.load_registry_metadata(&artifact, &name, &entry.version)?;
                let registry_dependencies =
                    registry_dependency_edges(&dep_manifest, registry_metadata.as_ref(), &name)?;
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
                        let roles = dependency
                            .roles
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(",");
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

                // A fetched package controls its own transitive path strings,
                // but it must not turn that control into a read of an
                // unrelated host directory. Root package paths retain their
                // existing sibling-path behavior; paths declared by a
                // dependency stay below that dependency's source root.
                if parent_dir != self.project_root {
                    let lexical_escape =
                        Path::new(path).is_absolute() || !abs_path.starts_with(parent_dir);
                    let canonical_escape = std::fs::canonicalize(parent_dir)
                        .and_then(|parent_root| {
                            std::fs::canonicalize(&abs_path)
                                .map(|resolved| !resolved.starts_with(parent_root))
                        })
                        .unwrap_or(true);
                    if lexical_escape || canonical_escape {
                        return Err(vec![path_dependency_escape_diagnostic(
                            dep_name,
                            path,
                            parent_dir,
                        )]);
                    }
                }

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
                let provenance = self.existing_provenance(dep_name, &dep_version, &content_hash);

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
                        envelope: None,
                        receipt: None,
                    },
                );
            }

            DepSpec::Git { url, selector } => {
                // Check if git is available.
                if !git_available() {
                    return Err(vec![Lock::e1203()]);
                }
                if let Err(reason) = validate_git_transport_url(url, self.project_root) {
                    return Err(vec![git_transport_diagnostic(url, &reason)]);
                }

                // Determine what rev to fetch.
                let rev_to_fetch = self.resolve_git_rev(dep_name, url, selector)?;
                let clone_dir = git_cache_dir(url, &rev_to_fetch).map_err(|d| vec![d])?;

                // Clone/fetch if not already cached.
                if !is_real_directory(&clone_dir) {
                    git_clone(url, &rev_to_fetch, &clone_dir, self.project_root)?;
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
                let provenance = self.existing_provenance(dep_name, &dep_version, &git_tree_hash);

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
                        envelope: None,
                        receipt: None,
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
                        SemVer::parse(version).is_some_and(|parsed| requirement.matches(&parsed))
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
                    let versions: Vec<SemVer> = candidates
                        .iter()
                        .map(|(version, _)| version.clone())
                        .collect();
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
                    let selected_version =
                        Publish::select_compatible(dep_name, &[&constraint], &versions, mode)
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
                let source_exception = Publish::active_source_exception(
                    &self.policy.exceptions,
                    dep_name,
                    &selected.version,
                )
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
                let artifact =
                    Publish::verify_artifact(&registry_repo, &selected).map_err(|error| {
                        vec![registry_diagnostic(
                            dep_name,
                            &format!("published artifact is unavailable or corrupt: {error}"),
                            "refresh the registry mirror or publish the immutable source artifact",
                        )]
                    })?;
                let dep_manifest = self.load_dep_manifest(&artifact, dep_name)?;
                if dep_manifest.package.name != dep_name
                    || dep_manifest.package.version != selected.version
                {
                    return Err(vec![registry_diagnostic(
                        dep_name,
                        "published source metadata disagrees with its registry index entry",
                        "republish a new immutable version with matching payload identity",
                    )]);
                }
                let registry_metadata =
                    self.load_registry_metadata(&artifact, dep_name, &selected.version)?;
                let registry_dependencies =
                    registry_dependency_edges(&dep_manifest, registry_metadata.as_ref(), dep_name)?;
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
                if let Some((prev_ver, prev_chain)) = self.version_seen.get(dep_name).cloned() {
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
                    self.version_seen
                        .insert(dep_name.to_string(), (dep_version.clone(), chain.to_vec()));
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
                            trans_specs
                                .push((trans_name.clone(), DepSpec::Registry(requirement.clone())));
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
                verify_registry_hangar_entry(dep_name, &store_path, &content_hash)?;
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
                        envelope: None,
                        receipt: None,
                    },
                );
            }

            DepSpec::Foreign {
                language,
                reference,
            } => {
                let Some(realization) = self.foreign.get(dep_name).cloned() else {
                    return Err(vec![foreign_dependency_diagnostic(
                        dep_name,
                        "the provider projection was not produced for this dependency",
                    )]);
                };
                if realization.language != *language || realization.reference != *reference {
                    return Err(vec![foreign_dependency_diagnostic(
                        dep_name,
                        "the provider projection identity disagrees with package.jet",
                    )]);
                }
                let dep_version = realization.entry.version.clone();
                if let Some((previous, previous_chain)) = self.version_seen.get(dep_name).cloned() {
                    if previous != dep_version {
                        return Err(vec![Lock::e1201(
                            dep_name,
                            &previous,
                            &previous_chain,
                            &dep_version,
                            chain,
                        )]);
                    }
                } else {
                    self.version_seen
                        .insert(dep_name.to_string(), (dep_version.clone(), chain.to_vec()));
                }
                self.resolved.insert(
                    dep_name.to_string(),
                    ResolvedPkg {
                        version: dep_version,
                        source: LockSource::Foreign {
                            language: *language,
                            reference: reference.clone(),
                            output: realization.entry.out.clone(),
                        },
                        locked: None,
                        fingerprint: realization.entry.envelope.output_hash.clone(),
                        content_hash: Some(realization.entry.envelope.output_hash.clone()),
                        deps: Vec::new(),
                        source_dir: PathBuf::from(&realization.entry.out),
                        provenance: None,
                        envelope: Some(Lock::LockEnvelope {
                            output_hash: realization.entry.envelope.output_hash.clone(),
                            platform: realization.entry.envelope.platform.clone(),
                            signature: realization.entry.envelope.signature.clone(),
                            provenance: realization.entry.envelope.provenance.clone(),
                            catalog_tier: String::new(),
                            catalog_trust: String::new(),
                        }),
                        receipt: (!realization.entry.receipt.is_empty())
                            .then(|| realization.entry.receipt.clone()),
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
            GitSelector::Rev(r) => validate_cached_revision(r)
                .map(|_| r.clone())
                .map_err(|reason| vec![git_revision_diagnostic(r, &reason)]),
            GitSelector::Tag(t) if t != "@latest" => {
                // If we have an existing lock and not updating, use locked rev.
                if !self.dependency_update_requested(dep_name) {
                    if let Some(locked_rev) = self.find_locked_rev(dep_name) {
                        return validate_cached_revision(&locked_rev)
                            .map(|_| locked_rev)
                            .map_err(|reason| {
                                vec![git_revision_diagnostic("locked revision", &reason)]
                            });
                    }
                }
                // Resolve the tag to a specific commit via ls-remote.
                git_resolve_ref(url, t, self.project_root).map_err(|d| vec![d])
            }
            GitSelector::Branch(b) | GitSelector::Tag(b) => {
                // Moving selector: always re-resolve if updating.
                if !self.dependency_update_requested(dep_name) {
                    if let Some(locked_rev) = self.find_locked_rev(dep_name) {
                        return validate_cached_revision(&locked_rev)
                            .map(|_| locked_rev)
                            .map_err(|reason| {
                                vec![git_revision_diagnostic("locked revision", &reason)]
                            });
                    }
                }
                git_resolve_ref(url, b, self.project_root).map_err(|d| vec![d])
            }
        }
    }

    fn dependency_update_requested(&self, dep_name: &str) -> bool {
        self.opts.update && (self.opts.update_dep.is_none() || self.update_scope.contains(dep_name))
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
        match self
            .advisory_policy
            .as_ref()
            .expect("advisory policy is loaded")
        {
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

fn compute_update_scope(existing_lock: Option<&LockFile>, opts: &FetchOptions) -> BTreeSet<String> {
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
        .map_err(|error| {
            let edge = format!(
                "{} -> {}#{}",
                manifest.package.name, package.name, package.version
            );
            Publish::package_policy_edge_diagnostic(&manifest.package.name, &edge, registry, &error)
        })?;
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
                    Publish::active_source_exception(
                        &manifest.policy.exceptions,
                        &package.name,
                        &package.version,
                    )
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
        "registry package identity is source-backed and must be verified before it enters the lock"
            .to_string(),
        fix.to_string(),
        None,
    )
}

fn verify_registry_hangar_entry(
    package: &str,
    store_entry: &Path,
    expected_hash: &str,
) -> Result<(), Vec<Diagnostic>> {
    let actual = Publish::registry_artifact_hash(store_entry).map_err(|error| {
        vec![registry_diagnostic(
            package,
            &format!("stored registry artifact cannot be hashed: {error}"),
            "repair the canonical Jetpack Hangar or retry the verified registry ingest",
        )]
    })?;
    if actual != expected_hash {
        return Err(vec![registry_diagnostic(
            package,
            "stored registry artifact failed its content hash",
            "repair the canonical Jetpack Hangar or retry the verified registry ingest",
        )]);
    }
    Ok(())
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
    let package_policy_fingerprint = package_policy.map(|receipt| receipt.fingerprint.clone());
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
    let reference = format!(
        "registry:{}:{}#{}",
        registry.name, entry.name, entry.version
    );
    let existing = jetpack::Store::list(&roots).into_iter().find(|existing| {
        existing.name.as_str() == entry.name.as_str()
            && existing.version.as_str() == entry.version.as_str()
            && existing.reference.as_str() == reference.as_str()
            && existing.cache_identity.source_fingerprint.as_str() == entry.content_hash.as_str()
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
    let policy_fingerprint = package_policy_fingerprint
        .or_else(|| {
            existing
                .as_ref()
                .filter(|_| advisory_receipt.is_none())
                .map(|existing| existing.cache_identity.policy_fingerprint.clone())
        })
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
            DepSpec::Path { path } => {
                let source_dir = normalize_path(&project_root.join(path));
                let Some(package) = lock.packages.iter().find(|package| {
                    package.name == *dep_name
                        && matches!(&package.source, LockSource::Path(_))
                }) else {
                    return Err(vec![locked_source_diagnostic(
                        dep_name,
                        "the lock has no matching path source identity",
                    )]);
                };
                let LockSource::Path(locked_path) = &package.source else {
                    unreachable!("path package predicate guarantees a path source")
                };
                if normalize_path(&project_root.join(locked_path)) != source_dir {
                    return Err(vec![locked_source_diagnostic(
                        dep_name,
                        "the locked path source disagrees with the manifest",
                    )]);
                }
                Store::verify_entry(
                    dep_name,
                    &source_dir,
                    package.content_hash.as_deref().unwrap_or(""),
                )
                .map_err(|diagnostic| vec![diagnostic])?;
                source_dir
            }
            DepSpec::Git { url, selector } => {
                let Some(package) = lock.packages.iter().find(|package| {
                    package.name == *dep_name
                        && matches!(&package.source, LockSource::Git { .. })
                }) else {
                    return Err(vec![locked_source_diagnostic(
                        dep_name,
                        "the lock has no matching git source identity",
                    )]);
                };
                let LockSource::Git {
                    url: locked_url,
                    selector: locked_selector,
                } = &package.source
                else {
                    unreachable!("git package predicate guarantees a git source")
                };
                let expected_selector = Lock::git_selector_str(selector);
                if locked_url != url || locked_selector != &expected_selector {
                    return Err(vec![locked_source_diagnostic(
                        dep_name,
                        "the locked git source disagrees with the manifest",
                    )]);
                }
                let Some(revision) = package.locked.as_ref() else {
                    return Err(vec![locked_source_diagnostic(
                        dep_name,
                        "the lock has no pinned git revision",
                    )]);
                };
                let source_dir = git_cache_dir(url, &revision.rev).map_err(|diagnostic| vec![diagnostic])?;
                let expected_hash = package
                    .content_hash
                    .as_deref()
                    .unwrap_or(&revision.tree_hash);
                Store::verify_entry(dep_name, &source_dir, expected_hash)
                    .map_err(|diagnostic| vec![diagnostic])?;
                source_dir
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
                } = &locked.source
                else {
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
                let all_entries =
                    crate::Publish::verify_registry_package(&repo, &config.name, dep_name)
                        .map_err(|diagnostic| vec![diagnostic])?;
                crate::Publish::verify_entry_tier(&entry).map_err(|diagnostic| vec![diagnostic])?;
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
                let registry_metadata = crate::Publish::read_registry_package_metadata(
                    &artifact,
                    dep_name,
                    &locked.version,
                )
                .map_err(|error| {
                    vec![registry_diagnostic(
                        dep_name,
                        &format!("locked registry dependency metadata is invalid: {error}"),
                        "refresh the lock from a trusted immutable registry artifact",
                    )]
                })?;
                let registry_dependencies =
                    registry_dependency_edges(&dep_manifest, registry_metadata.as_ref(), dep_name)?;
                let expected_dependencies = registry_dependencies
                    .iter()
                    .map(|dependency| dependency.name.clone())
                    .chain(dep_manifest.dependencies.iter().filter_map(|(name, spec)| {
                        (!matches!(spec, DepSpec::Registry(_))).then_some(name.clone())
                    }))
                    .collect::<BTreeSet<_>>();
                let locked_dependencies =
                    locked.dependencies.iter().cloned().collect::<BTreeSet<_>>();
                if locked_dependencies != expected_dependencies {
                    return Err(vec![registry_diagnostic(
                        dep_name,
                        "locked dependency roles, features, or constraints disagree with the artifact metadata",
                        "refresh the lock from the trusted immutable registry artifact",
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
                let source_exception = crate::Publish::active_source_exception(
                    &manifest.policy.exceptions,
                    dep_name,
                    &locked.version,
                );
                let store_path = ingest_registry_artifact(
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
                })?;
                verify_registry_hangar_entry(dep_name, &store_path, source_hash)?;
                store_path
            }
            DepSpec::Foreign {
                language,
                reference,
            } => validate_locked_foreign_dependency(
                lock,
                project_root,
                dep_name,
                *language,
                reference,
            )?,
        };
        dep_dirs.insert(dep_name.clone(), source_dir);
    }
    Ok(dep_dirs)
}

fn validate_locked_foreign_dependency(
    lock: &LockFile,
    project_root: &Path,
    dep_name: &str,
    language: crate::AST::ForeignLanguage,
    reference: &str,
) -> Result<PathBuf, Vec<Diagnostic>> {
    let Some(package) = lock.packages.iter().find(|package| {
        package.name == dep_name
            && matches!(
                &package.source,
                LockSource::Foreign {
                    language: locked_language,
                    reference: locked_reference,
                    ..
                } if *locked_language == language && locked_reference == reference
            )
    }) else {
        return Err(vec![foreign_locked_diagnostic(
            dep_name,
            "the lock has no matching language-qualified provider identity",
        )]);
    };
    let LockSource::Foreign { output, .. } = &package.source else {
        unreachable!("foreign package predicate guarantees a foreign lock source")
    };
    let output = PathBuf::from(output);
    let output_metadata = std::fs::symlink_metadata(&output).map_err(|error| {
        vec![foreign_locked_diagnostic(
            dep_name,
            &format!("locked Hangar output is unavailable: {error}"),
        )]
    })?;
    if output_metadata.file_type().is_symlink() || !output_metadata.is_dir() {
        return Err(vec![foreign_locked_diagnostic(
            dep_name,
            "locked Hangar output is not a real directory",
        )]);
    }
    let Some(envelope) = package.envelope.as_ref() else {
        return Err(vec![foreign_locked_diagnostic(
            dep_name,
            "the foreign lock record has no verified output envelope",
        )]);
    };
    if envelope.output_hash.is_empty() {
        return Err(vec![foreign_locked_diagnostic(
            dep_name,
            "the foreign lock record has no output hash",
        )]);
    }
    let roots = jetpack::Store::resolve();
    let actual = jetpack::Envelope::try_output_hash_of_in_hangar(
        &output.to_string_lossy(),
        &roots.hangar_dir(),
        false,
    )
    .map_err(|error| {
        vec![foreign_locked_diagnostic(
            dep_name,
            &format!("locked foreign output failed integrity verification: {error}"),
        )]
    })?;
    if actual != envelope.output_hash {
        return Err(vec![foreign_locked_diagnostic(
            dep_name,
            "locked foreign output hash disagrees with its envelope",
        )]);
    }

    let stored_binding = output
        .join(Syntax::SOURCE_ROOT_DIR)
        .join(language.bindings_subdir())
        .join(format!("{dep_name}.{}", Syntax::FILE_EXT));
    let project_binding = jetpack::Foreign::project_binding_path(project_root, language, dep_name);
    let stored_metadata = std::fs::symlink_metadata(&stored_binding).map_err(|error| {
        vec![foreign_locked_diagnostic(
            dep_name,
            &format!("locked generated binding is unavailable: {error}"),
        )]
    })?;
    let project_metadata = std::fs::symlink_metadata(&project_binding).map_err(|error| {
        vec![foreign_locked_diagnostic(
            dep_name,
            &format!("project binding cache is unavailable: {error}"),
        )]
    })?;
    if stored_metadata.file_type().is_symlink()
        || !stored_metadata.is_file()
        || project_metadata.file_type().is_symlink()
        || !project_metadata.is_file()
    {
        return Err(vec![foreign_locked_diagnostic(
            dep_name,
            "the locked generated binding is not a regular file",
        )]);
    }
    let stored_bytes = std::fs::read(&stored_binding).map_err(|error| {
        vec![foreign_locked_diagnostic(
            dep_name,
            &format!("could not read locked generated binding: {error}"),
        )]
    })?;
    let project_bytes = std::fs::read(&project_binding).map_err(|error| {
        vec![foreign_locked_diagnostic(
            dep_name,
            &format!("could not read project generated binding: {error}"),
        )]
    })?;
    if stored_bytes != project_bytes {
        return Err(vec![foreign_locked_diagnostic(
            dep_name,
            "project binding cache disagrees with the locked Hangar binding",
        )]);
    }
    Ok(output)
}

fn foreign_locked_diagnostic(dep_name: &str, detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1204",
        format!("locked foreign dependency `{dep_name}` failed verification"),
        detail.to_string(),
        "run `jet fetch` to recreate the verified provider projection, then commit the resulting `.jet/lock`".to_string(),
        None,
    )
}

fn locked_source_diagnostic(dep_name: &str, detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1204",
        format!("locked dependency `{dep_name}` failed source verification"),
        detail.to_string(),
        "run `jet fetch` to recreate the lock from the verified dependency source".to_string(),
        None,
    )
}

// ──────────────────────────────────────────────
// Git helpers
// ──────────────────────────────────────────────

pub fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn git_cache_dir(url: &str, rev: &str) -> Result<PathBuf, Diagnostic> {
    if let Err(reason) = validate_cached_revision(rev) {
        return Err(git_revision_diagnostic(rev, &reason));
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let url_hash = crate::SHA256::sha256_hex(url.as_bytes());
    let rev_prefix: String = rev
        .char_indices()
        .take_while(|(index, _)| *index < 16)
        .map(|(_, character)| character)
        .collect();
    let path = PathBuf::from(home)
        .join(".jet")
        .join("git-cache")
        .join(&url_hash[..16])
        .join(rev_prefix);
    validate_git_cache_path(&path).map_err(|reason| git_cache_diagnostic(&path, &reason))?;
    Ok(path)
}

fn git_resolve_ref(url: &str, refname: &str, project_root: &Path) -> Result<String, Diagnostic> {
    if let Err(reason) = validate_git_transport_url(url, project_root) {
        return Err(git_transport_diagnostic(url, &reason));
    }
    if let Err(reason) = validate_git_revision(refname) {
        return Err(git_revision_diagnostic(refname, &reason));
    }
    let out = hardened_git_command()
        .args(["ls-remote", "--exit-code", "--", url, refname])
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
    if let Err(reason) = validate_cached_revision(&rev) {
        return Err(git_revision_diagnostic(&rev, &reason));
    }
    Ok(rev)
}

fn git_clone(
    url: &str,
    rev: &str,
    dest: &Path,
    project_root: &Path,
) -> Result<(), Vec<Diagnostic>> {
    if let Err(reason) = validate_git_transport_url(url, project_root) {
        return Err(vec![git_transport_diagnostic(url, &reason)]);
    }
    if let Err(reason) = validate_git_revision(rev) {
        return Err(vec![git_revision_diagnostic(rev, &reason)]);
    }
    if let Err(reason) = validate_cached_revision(rev) {
        return Err(vec![git_revision_diagnostic(rev, &reason)]);
    }
    if let Err(reason) = validate_git_cache_path(dest) {
        return Err(vec![git_cache_diagnostic(dest, &reason)]);
    }
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
    if let Err(reason) = validate_git_cache_path(&tmp) {
        return Err(vec![git_cache_diagnostic(&tmp, &reason)]);
    }
    if let Ok(metadata) = std::fs::symlink_metadata(&tmp) {
        let result = if metadata.is_dir() {
            std::fs::remove_dir_all(&tmp)
        } else {
            std::fs::remove_file(&tmp)
        };
        if let Err(error) = result {
            return Err(vec![git_cache_diagnostic(
                &tmp,
                &format!("could not clear the temporary checkout: {error}"),
            )]);
        }
    }

    let clone_ok = hardened_git_command()
        .args(["clone", "--quiet", "--", url, tmp.to_str().unwrap_or(".")])
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

    let checkout_ok = hardened_git_command()
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

fn path_dependency_escape_diagnostic(
    dep_name: &str,
    path: &str,
    parent_dir: &Path,
) -> Diagnostic {
    Diagnostic::error(
        "E1206",
        format!("path dependency `{dep_name}` escapes its parent package"),
        format!(
            "the path `{path}` resolves outside the declaring package directory `{}`",
            parent_dir.display()
        ),
        "use a relative path below the declaring package directory".to_string(),
        None,
    )
}

fn git_transport_diagnostic(url: &str, reason: &str) -> Diagnostic {
    Diagnostic::error(
        "E1203",
        "git dependency URL is not allowed".to_string(),
        format!("git transport policy rejected `{url}`: {reason}"),
        "use a public HTTPS, SSH, or Git URL, or a local file URL".to_string(),
        None,
    )
}

fn git_revision_diagnostic(revision: &str, reason: &str) -> Diagnostic {
    Diagnostic::error(
        "E1203",
        "git revision is not allowed".to_string(),
        format!("git revision policy rejected `{revision}`: {reason}"),
        format!(
            "use a branch, tag, or commit name without leading `-` in {}",
            crate::Syntax::PACKAGE_FILE
        ),
        None,
    )
}

fn git_cache_diagnostic(path: &Path, reason: &str) -> Diagnostic {
    Diagnostic::error(
        "E1206",
        format!("git cache path `{}` is not allowed", path.display()),
        reason.to_string(),
        "remove the cache symlink or use a safe pinned revision, then run `jet fetch` again"
            .to_string(),
        None,
    )
}

fn validate_git_revision(revision: &str) -> Result<(), String> {
    if revision.is_empty() || revision.chars().any(char::is_control) || revision.starts_with('-') {
        return Err("the revision is empty or contains unsafe characters".to_string());
    }
    Ok(())
}

fn validate_cached_revision(revision: &str) -> Result<(), String> {
    if revision.is_empty()
        || revision == "."
        || revision == ".."
        || revision.starts_with('-')
        || revision.contains(['/', '\\', ':'])
        || revision.chars().any(char::is_control)
        || !matches!(
            Path::new(revision).components().next(),
            Some(std::path::Component::Normal(_))
        )
        || Path::new(revision).components().nth(1).is_some()
    {
        return Err("the revision must be one safe cache path component".to_string());
    }
    Ok(())
}

fn validate_git_cache_path(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "cache path component `{}` is a symlink",
                    current.display()
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!(
                    "cache path component `{}` is not a directory",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn is_real_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn hardened_git_command() -> Command {
    let mut command = Command::new("git");
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", git_null_device())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", git_null_device())
        .env("GIT_SSH_COMMAND", hardened_ssh_command())
        // Command-line config wins over a repository-local credential helper.
        .args([
            "-c",
            "credential.helper=",
            "-c",
            "protocol.ext.allow=never",
            "-c",
            "protocol.file.allow=always",
        ]);
    command
}

fn git_null_device() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

fn hardened_ssh_command() -> &'static str {
    if cfg!(windows) {
        "ssh -oBatchMode=yes -oIdentitiesOnly=yes -oIdentityAgent=none -oIdentityFile=none -F NUL"
    } else {
        "ssh -oBatchMode=yes -oIdentitiesOnly=yes -oIdentityAgent=none -oIdentityFile=none -F /dev/null"
    }
}

fn validate_git_transport_url(url: &str, project_root: &Path) -> Result<(), String> {
    if url.is_empty() || url.chars().any(char::is_control) || url.starts_with('-') {
        return Err("the URL is empty or contains unsafe characters".to_string());
    }
    if let Some(path) = url.strip_prefix("file://") {
        if path.is_empty()
            || path.contains(['?', '#', '\\'])
            || !path.starts_with('/')
        {
            return Err("file URLs must name a local absolute path".to_string());
        }
        return validate_local_git_path(Path::new(path), project_root);
    }
    if !url.contains("://") && !looks_like_scp_url(url) {
        return Err("local git paths must use file:// inside the project root".to_string());
    }
    let (scheme, authority) = if let Some((scheme, rest)) = url.split_once("://") {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        (scheme.to_ascii_lowercase(), authority)
    } else {
        (
            "ssh".to_string(),
            url.split_once(':').map(|(host, _)| host).unwrap_or(""),
        )
    };
    if !matches!(scheme.as_str(), "git" | "http" | "https" | "ssh") {
        return Err(format!("the `{scheme}` transport is not allowed"));
    }
    if let Some((user, _)) = authority.rsplit_once('@') {
        if scheme != "ssh"
            || user.is_empty()
            || user.contains('@')
            || user.contains(':')
            || user.chars().any(char::is_whitespace)
        {
            return Err("embedded credentials are not allowed in Git URLs".to_string());
        }
    }
    let host = host_from_git_authority(authority)?;
    let port = if let Some(port) = port_from_git_authority(authority)? {
        port
    } else if scheme == "https" {
        443
    } else if scheme == "http" {
        80
    } else {
        22
    };
    let endpoint = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let addresses = endpoint
        .to_socket_addrs()
        .map_err(|error| format!("could not resolve the destination: {error}"))?
        .collect::<Vec<_>>();
    if addresses.is_empty() || addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err("the destination resolves to a non-public address".to_string());
    }
    Ok(())
}

fn validate_local_git_path(path: &Path, project_root: &Path) -> Result<(), String> {
    let root = std::fs::canonicalize(project_root)
        .map_err(|error| format!("could not resolve the project root: {error}"))?;
    let candidate = std::fs::canonicalize(path)
        .map_err(|error| format!("could not resolve the local Git path: {error}"))?;
    if !candidate.starts_with(&root) {
        return Err("the local Git path resolves outside the project root".to_string());
    }
    reject_git_path_symlinks(path)?;
    if !is_real_directory(&candidate) {
        return Err("the local Git path must be a real directory".to_string());
    }
    Ok(())
}

fn reject_git_path_symlinks(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = std::fs::symlink_metadata(&current).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "the local Git path contains a symlink component `{}`",
                current.display()
            ));
        }
    }
    Ok(())
}

fn looks_like_scp_url(url: &str) -> bool {
    url.split_once(':')
        .map(|(host, path)| {
            !host.is_empty()
                && !path.is_empty()
                && !host.contains(['/', '?', '#'])
                && !host.ends_with('\\')
        })
        .unwrap_or(false)
}

fn host_from_git_authority(authority: &str) -> Result<String, String> {
    let authority = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    if authority.is_empty() || authority.contains(['@', '\\']) {
        return Err("the URL has no valid host".to_string());
    }
    if let Some(host) = authority.strip_prefix('[') {
        let (host, suffix) = host
            .split_once(']')
            .ok_or_else(|| "the URL has an invalid IPv6 host".to_string())?;
        if host.is_empty() || (!suffix.is_empty() && !suffix.starts_with(':')) {
            return Err("the URL has an invalid host".to_string());
        }
        return Ok(host.to_string());
    }
    let host = authority.rsplit_once(':').map(|(host, _)| host).unwrap_or(authority);
    if host.is_empty() || host.contains(':') || host.chars().any(char::is_whitespace) {
        return Err("the URL has an invalid host".to_string());
    }
    Ok(host.to_string())
}

fn port_from_git_authority(authority: &str) -> Result<Option<u16>, String> {
    let authority = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    let raw_port = if let Some(host) = authority.strip_prefix('[') {
        host.split_once(']')
            .and_then(|(_, suffix)| suffix.strip_prefix(':'))
    } else {
        authority.rsplit_once(':').map(|(_, port)| port)
    };
    raw_port
        .map(|port| {
            let port = port
                .parse::<u16>()
                .map_err(|_| "the URL has an invalid port".to_string())?;
            if port == 0 {
                return Err("the URL has an invalid port".to_string());
            }
            Ok(port)
        })
        .transpose()
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 100 && (b & 0b1100_0000) == 0b0100_0000
                || a == 127
                || a == 169 && b == 254
                || a == 172 && (16..=31).contains(&b)
                || a == 192 && b == 0 && c == 0
                || a == 192 && b == 0 && c == 2
                || a == 192 && b == 168
                || a == 198 && (18..=19).contains(&b)
                || a == 198 && b == 51 && c == 100
                || a == 203 && b == 0 && c == 113
                || a >= 224)
        }
        IpAddr::V6(ip) => {
            if let Some(ipv4) = ip.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(ipv4));
            }
            let [first, second, ..] = ip.segments();
            (first & 0xe000) == 0x2000
                && !ip.is_unspecified()
                && !ip.is_loopback()
                && !ip.is_multicast()
                && (first & 0xfe00) != 0xfc00
                && (first & 0xffc0) != 0xfe80
                && !(first == 0x2001 && second == 0x0db8)
        }
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn git_revision_allowlist_rejects_option_and_control_injection() {
        assert!(super::validate_git_revision("main").is_ok());
        assert!(super::validate_git_revision("--upload-pack=touch pwned").is_err());
        assert!(super::validate_git_revision("main\n--upload-pack=touch").is_err());
    }
}
