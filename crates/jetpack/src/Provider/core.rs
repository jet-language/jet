use super::*;

/// The first-party Jet package provider (R2/U10). Realizes a Jet package with
/// no Nix at all: it discovers the package's `module <name>` in the source repo
/// (Chunk 3), reads the repo's `pkg.jet` `packages:` index for the package's
/// kind (Chunk 4), and materializes that source tree into the Jetpack store —
/// staging a `bin/` for an `executable`, source-only for a `library`. R2
/// supports local and git-backed remote source repos.
pub(crate) struct CoreProvider;

impl Provider for CoreProvider {
    fn requires_acquisition(&self, spec: &RefSpec, table: &SourceTable) -> bool {
        table
            .upstream(spec.source.label())
            .is_none_or(|upstream| !upstream.starts_with("path:"))
    }

    fn cache_expectation(
        &self,
        spec: &RefSpec,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Option<crate::Store::CacheExpectation> {
        core_cache_expectation(spec, table, ctx)
    }

    fn approval_facts(
        &self,
        spec: &RefSpec,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Option<String>, String> {
        core_approval_facts(spec, table, ctx)
    }

    fn plan_downloads(
        &self,
        specs: &[RefSpec],
        table: &SourceTable,
        _ctx: &Ctx,
    ) -> Result<DownloadPlan, ProviderError> {
        let mut plan = DownloadPlan::default();
        for spec in specs {
            if self.requires_acquisition(spec, table) {
                plan.add_item(PlanItem {
                    package: spec.raw.clone(),
                    state: PlanState::New,
                    download_bytes: None,
                    disk_bytes: None,
                });
            }
        }
        Ok(plan)
    }

    fn realize(
        &self,
        spec: &RefSpec,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Realized, ProviderError> {
        let source_name = spec.source.label();
        let upstream = table.upstream(source_name).ok_or_else(|| {
            ProviderError::CoreBuild(format!("source `{source_name}` has no upstream"))
        })?;
        let repo = source_repo(upstream, &spec.package, ctx)?;
        let canonical_package =
            find_canonical_package(&repo, &spec.package).map_err(ProviderError::CoreBuild)?;
        let (src_dir, canonical, canonical_kind, canonical_version) = if let Some((
            package_root,
            facts,
        )) = canonical_package
        {
            let source = facts.source.as_deref().unwrap_or(".");
            let source_path = Path::new(source);
            if source_path.is_absolute()
                || source_path
                    .components()
                    .any(|component| component == std::path::Component::ParentDir)
            {
                return Err(ProviderError::CoreBuild(format!(
                    "canonical Package source `{source}` escapes {}",
                    package_root.display()
                )));
            }
            let source_dir = package_root.join(source_path);
            let kind = canonical_package_kind(&facts, &spec.package)
                .unwrap_or_else(|| infer_package_kind(&source_dir));
            (
                source_dir,
                Some(facts.clone()),
                Some(kind),
                facts.version.clone().unwrap_or_default(),
            )
        } else {
            let source_dir = Package::discover_module_in(&repo, &spec.package)
                    .map_err(|e| match e {
                        Package::DiscoveryError::NotFound { name } => {
                            ProviderError::CoreBuild(format!(
                                "source repo at {} has no `module {name}` — add a .{} file declaring it",
                                repo.display(),
                                crate::Syntax::FILE_EXT,
                            ))
                        }
                        Package::DiscoveryError::Ambiguous { name, paths } => {
                            let list = paths
                                .iter()
                                .map(|p| p.display().to_string())
                                .collect::<Vec<_>>()
                                .join(", ");
                            ProviderError::CoreBuild(format!(
                                "source repo at {} has `module {name}` in multiple files: {list}",
                                repo.display(),
                            ))
                        }
                    })?;
            (source_dir, None, None, String::new())
        };
        if !src_dir.is_dir() {
            return Err(ProviderError::CoreBuild(format!(
                "package source {} does not exist",
                src_dir.display()
            )));
        }
        validate_core_source_tree(&src_dir).map_err(ProviderError::CoreBuild)?;
        // Content-address the materialized package so identical sources share a
        // store entry and changes get a fresh one.
        let fp = core_tree_fingerprint(&src_dir).map_err(ProviderError::CoreBuild)?;
        let source_fingerprint = fp.clone();
        let toolchain = crate::Toolchain::Toolchain::resolve_for_core(ctx.offline);
        let out_dir = ctx
            .store_dir
            .join(format!("{}-{}", spec.package, &fp[..12]));
        // Reuse is owned by Store verification. Reaching the provider with an
        // existing object means no verified record leased it.
        if std::fs::symlink_metadata(&out_dir).is_ok() {
            return Err(ProviderError::CoreBuild(format!(
                "unverified existing output {}; run `jet clean` before rebuilding",
                out_dir.display()
            )));
        }
        copy_tree(&src_dir, &out_dir)
            .map_err(|e| ProviderError::CoreBuild(format!("could not place package: {e}")))?;
        // U10 Chunk 4: the repo's `pkg.jet` `packages:` index decides what
        // goes on PATH. `executable` stages the prebuilt `bin/` (the devshell
        // case); `library` stages module source for import and contributes no
        // PATH entry (an empty `bin`). With no manifest entry — a bare `core`
        // source declared by marker, no `pkg.jet` — we default to
        // `executable`, today's behavior.
        let manifest = match if canonical.is_some() {
            None
        } else {
            Package::PackageFacts::load(&repo)
        } {
            None => None,
            Some(Ok(manifest)) => Some(manifest),
            Some(Err(error)) => {
                return Err(ProviderError::CoreBuild(format!(
                    "package manifest {} is invalid: {error:?}",
                    crate::Manifest::manifest_path_in(&repo).display()
                )));
            }
        };
        // D-ILE1: `kind` is inferred when `pkg.jet` omits it (or there is no
        // `pkg.jet`): a top-level `fn run` in the package source means
        // executable, otherwise library. An explicit `library`/`executable`
        // always wins.
        let kind = canonical_kind
            .or_else(|| {
                manifest
                    .as_ref()
                    .and_then(|pm| pm.package_kind(&spec.package))
            })
            .unwrap_or_else(|| infer_package_kind(&out_dir));
        // `pkg.jet` carries the real version for core packages (U10).
        let version = if canonical.is_some() {
            canonical_version
        } else {
            manifest
                .as_ref()
                .and_then(|pm| pm.version.clone())
                .unwrap_or_default()
        };
        let (bin, rlib, recipe_id, sandbox_class, sandbox_policy) = match kind {
            Package::PackageKind::Executable => (
                out_dir.join("bin").to_string_lossy().into_owned(),
                String::new(),
                "core-source",
                "non-executing".to_string(),
                "no child launched".to_string(),
            ),
            Package::PackageKind::Library => {
                // D-BFS1: if the package ships a Cargo.toml, compile it to an
                // rlib now. The rlib lands *inside* the hangar object (`out_dir`)
                // so the object is self-contained and content-addressed; the
                // cargo target dir is a hangar-scoped scratch swept after the
                // build (D-JPK-GC1: build scratch is hangar-scoped, swept on
                // crash), never a sibling of the store root.
                let cargo_toml = out_dir.join("Cargo.toml");
                if cargo_toml.is_file() {
                    // D-JPK-BUILDTOOL1=A: compile through the resolved toolchain.
                    // Offline Core is resolved with `resolve_pinned`, so a
                    // missing fixture is a hard miss rather than a host-Cargo
                    // fallback. Online development may use the explicit host
                    // dev toolchain.
                    let toolchain = toolchain.as_ref().ok_or_else(|| {
                        ProviderError::CoreBuild(
                            "core library carries Cargo.toml but no permitted Jet toolchain is available"
                                .to_string(),
                        )
                    })?;
                    if ctx.offline && !toolchain.pinned {
                        return Err(ProviderError::CoreBuild(
                            "offline Core package delivery requires a realized pinned Jet toolchain; refusing the host toolchain"
                                .to_string(),
                        ));
                    }
                    let cargo_build =
                        build_rlib_from_cargo_mode(&out_dir, ctx.store_dir, toolchain, ctx.offline)
                            .map_err(|error| match error {
                                CargoBuildError::SandboxUnavailable(reason) => {
                                    ProviderError::SandboxUnavailable(reason)
                                }
                                CargoBuildError::Failed(reason) => ProviderError::CoreBuild(reason),
                            })?;
                    (
                        String::new(),
                        cargo_build.rlib,
                        "core-cargo-rlib",
                        cargo_build.sandbox_class,
                        cargo_build.sandbox_policy,
                    )
                } else {
                    (
                        String::new(),
                        String::new(),
                        "core-source",
                        "non-executing".to_string(),
                        "no child launched".to_string(),
                    )
                }
            }
        };
        crate::Store::seal_local_output(&out_dir).map_err(|error| {
            ProviderError::CoreBuild(format!("could not seal package output: {error}"))
        })?;
        let out = out_dir.to_string_lossy().into_owned();
        let envelope = crate::Envelope::Envelope::for_output(&out, &spec.raw, recipe_id);
        let recipe_identity = core_recipe_identity(
            &src_dir,
            &spec.package,
            manifest.as_ref(),
            kind,
            canonical.as_ref(),
            toolchain.as_ref(),
        )
        .map_err(ProviderError::CoreBuild)?;
        let identity = cache_identity(&source_fingerprint, &recipe_identity, ctx);
        let private_untrusted = recipe_id == "core-cargo-rlib";
        let mut plan_facts = BTreeMap::from([
            ("action.kind".into(), "core-build".into()),
            ("action.recipe".into(), recipe_identity.clone()),
            ("build.sandbox".into(), sandbox_class.clone()),
            ("build.sandbox_policy".into(), sandbox_policy.clone()),
        ]);
        let mut producer_facts = BTreeMap::from([
            ("source.kind".into(), "core-package-tree".into()),
            (
                "source.tree_schema".into(),
                "jet-core-source-tree-v2".into(),
            ),
            ("source.tree_fingerprint".into(), fp.clone()),
            ("artifact.kind".into(), recipe_id.to_string()),
            (
                "execution.platform".into(),
                crate::Envelope::host_platform(),
            ),
            ("build.sandbox".into(), sandbox_class),
            ("build.sandbox_policy".into(), sandbox_policy),
        ]);
        if private_untrusted {
            // D-JPK-SANDBOX2: an exact local build grant permits the action,
            // but its output is private and cannot become shared cache input.
            plan_facts.insert("build.trust".into(), "private-untrusted".into());
            producer_facts.insert("build.trust".into(), "private-untrusted".into());
        }
        let producer = producer_record(
            "core",
            &format!("cas:{source_fingerprint}"),
            &source_fingerprint,
            plan_facts,
            &toolchain_facts(toolchain.as_ref()),
            &identity,
            producer_facts,
        )?;
        Ok(Realized {
            name: spec.package.clone(),
            version,
            reference: spec.raw.clone(),
            out,
            bin,
            rlib,
            envelope,
            cache_identity: identity,
            source_state: SourceState::Built,
            named_outputs: BTreeMap::from([("out".into(), out_dir.to_string_lossy().into_owned())]),
            references: Vec::new(),
            producer,
        })
    }
}

fn core_cache_expectation(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
) -> Option<crate::Store::CacheExpectation> {
    let upstream = table.upstream(spec.source.label())?;
    let repo = source_repo(upstream, &spec.package, ctx).ok()?;
    let canonical_package = match find_canonical_package(&repo, &spec.package) {
        Ok(package) => package,
        Err(_) => return None,
    };
    let canonical = canonical_package.as_ref().map(|(_, facts)| facts);
    let src_dir = canonical_package
        .as_ref()
        .and_then(|(root, facts)| canonical_source_dir(root, facts))
        .or_else(|| Package::discover_module_in(&repo, &spec.package).ok())?;
    validate_core_source_tree(&src_dir).ok()?;
    let toolchain = crate::Toolchain::Toolchain::resolve_for_core(ctx.offline);
    if ctx.offline
        && src_dir.join("Cargo.toml").is_file()
        && !toolchain.as_ref().is_some_and(|toolchain| toolchain.pinned)
    {
        return None;
    }
    let source_fingerprint = core_tree_fingerprint(&src_dir).ok()?;
    let (manifest, canonical) = if canonical.is_some() {
        (None, canonical)
    } else {
        let manifest = match Package::PackageFacts::load(&repo) {
            None => None,
            Some(Ok(manifest)) => Some(manifest),
            Some(Err(_)) => return None,
        };
        (manifest, None)
    };
    let kind = canonical
        .and_then(|facts| canonical_package_kind(facts, &spec.package))
        .or_else(|| {
            manifest
                .as_ref()
                .and_then(|manifest| manifest.package_kind(&spec.package))
        })
        .unwrap_or_else(|| infer_package_kind(&src_dir));
    let recipe = core_recipe_identity(
        &src_dir,
        &spec.package,
        manifest.as_ref(),
        kind,
        canonical,
        toolchain.as_ref(),
    )
    .ok()?;
    Some(crate::Store::CacheExpectation {
        identity: cache_identity(&source_fingerprint, &recipe, ctx),
        owned_output: Some(ctx.store_dir.join(format!(
            "{}-{}",
            spec.package,
            &source_fingerprint[..12]
        ))),
        allow_unsigned_local: true,
    })
}

fn core_approval_facts(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
) -> Result<Option<String>, String> {
    let upstream = table.upstream(spec.source.label()).ok_or_else(|| {
        format!(
            "Core source `{}` has no resolved upstream",
            spec.source.label()
        )
    })?;
    let repo = source_repo(upstream, &spec.package, ctx)
        .map_err(|error| format!("could not resolve Core source: {error:?}"))?;
    let canonical_package = find_canonical_package(&repo, &spec.package)?;
    let canonical_facts = canonical_package.as_ref().map(|(_, facts)| facts);
    let src_dir = if let Some(source) = canonical_package
        .as_ref()
        .and_then(|(root, facts)| canonical_source_dir(root, facts))
    {
        source
    } else {
        Package::discover_module_in(&repo, &spec.package)
            .map_err(|error| format!("could not identify Core source: {error:?}"))?
    };
    validate_core_source_tree(&src_dir)?;
    let source_digest = core_tree_fingerprint(&src_dir)?;
    let (manifest, canonical) = if canonical_facts.is_some() {
        (None, canonical_facts)
    } else {
        let manifest = match Package::PackageFacts::load(&repo) {
            None => None,
            Some(Ok(manifest)) => Some(manifest),
            Some(Err(error)) => return Err(format!("Core package manifest is invalid: {error:?}")),
        };
        (manifest, None)
    };
    let kind = canonical
        .and_then(|facts| canonical_package_kind(facts, &spec.package))
        .or_else(|| {
            manifest
                .as_ref()
                .and_then(|manifest| manifest.package_kind(&spec.package))
        })
        .unwrap_or_else(|| infer_package_kind(&src_dir));
    if kind != Package::PackageKind::Library || !src_dir.join("Cargo.toml").is_file() {
        return Ok(None);
    }
    let toolchain = crate::Toolchain::Toolchain::resolve_for_core(ctx.offline);
    if ctx.offline && !toolchain.as_ref().is_some_and(|toolchain| toolchain.pinned) {
        return Ok(None);
    }
    let recipe = core_recipe_identity(
        &src_dir,
        &spec.package,
        manifest.as_ref(),
        kind,
        canonical,
        toolchain.as_ref(),
    )?;
    let platform = crate::Envelope::host_platform();
    let authority = format!(
        "jet-core-build-hook.v1\npackage={}\nprovider={}\nsource={}\nsource_digest={}\nplatform={}\nrecipe={}\ncapabilities=exec:cargo\n",
        spec.package, upstream, spec.raw, source_digest, platform, recipe
    );
    Ok(Some(format!(
        "build-sha256:{}",
        SHA256::sha256_hex(authority.as_bytes())
    )))
}

/// The hangar-scoped subdir that holds transient build scratch (cargo target
/// dirs). D-JPK-GC1: build scratch is hangar-scoped and swept on crash, never a
/// sibling of the store root.
pub const BUILD_SCRATCH_DIR: &str = "build-scratch";
pub const ACTIVE_TMP_MARKER: &str = ".active";
static NEXT_BUILD_SCRATCH: AtomicU64 = AtomicU64::new(0);

/// Return whether a scratch marker belongs to a process that can still be
/// using the directory. A bare marker from an older build is stale and may be
/// reclaimed; a live marker protects an in-flight build from GC.
pub(crate) fn active_tmp_marker_is_live(path: &Path) -> bool {
    let marker = path.join(ACTIVE_TMP_MARKER);
    let Ok(contents) = std::fs::read_to_string(marker) else {
        return false;
    };
    // Older Jetpack versions used an empty marker as a conservative lock. Keep
    // that meaning: cleanup must never delete a directory whose owner only
    // wrote the legacy marker before crashing or being interrupted.
    if contents.trim().is_empty() {
        return true;
    }
    let mut pid = None;
    let mut started = None;
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("pid=") {
            pid = value.parse::<u32>().ok();
        } else if let Some(value) = line.strip_prefix("started=") {
            started = value.parse::<u64>().ok();
        }
    }
    let Some(pid) = pid else {
        return false;
    };
    if pid == std::process::id() {
        return true;
    }
    #[cfg(unix)]
    if Path::new("/proc").join(pid.to_string()).exists() {
        return true;
    }
    // Platforms without a process table still get a conservative grace
    // period. A malformed or very old marker is safe to reclaim.
    let Some(started) = started else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(started);
    now.saturating_sub(started) < 24 * 60 * 60
}

/// Remove every transient build-scratch dir under the hangar. Idempotent; used
/// to sweep scratch left behind by a crashed build (D-JPK-GC1). Returns the
/// number of scratch entries removed.
pub fn sweep_build_scratch(hangar_dir: &Path) -> usize {
    let root = hangar_dir.join(BUILD_SCRATCH_DIR);
    let mut removed = 0;
    if let Ok(rd) = std::fs::read_dir(&root) {
        for ent in rd.flatten() {
            if active_tmp_marker_is_live(&ent.path()) {
                continue;
            }
            if std::fs::remove_dir_all(ent.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// A hangar-scoped scratch dir that removes itself on drop — so a panic or an
/// early return between build start and finish never leaks a cargo target dir
/// into the hangar (D-JPK-GC1 crash-clean).
struct BuildScratch {
    path: PathBuf,
}

impl BuildScratch {
    fn new(hangar_dir: &Path, key: &str) -> Result<BuildScratch, String> {
        if key.is_empty() || key.contains(std::path::MAIN_SEPARATOR) || key == "." || key == ".." {
            return Err("cargo scratch key is not a safe single path component".to_string());
        }
        let root = hangar_dir.join(BUILD_SCRATCH_DIR);
        match std::fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(format!(
                    "build scratch root is not a directory: {}",
                    root.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir_all(&root)
                    .map_err(|error| format!("could not create build scratch root: {error}"))?;
            }
            Err(error) => return Err(format!("could not inspect build scratch root: {error}")),
        }
        let nonce = NEXT_BUILD_SCRATCH.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!("{key}-{}-{nonce}", std::process::id()));
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!(
                    "build scratch path is not a directory: {}",
                    path.display()
                ));
            }
            if active_tmp_marker_is_live(&path) {
                return Err(format!(
                    "build scratch path is already active: {}",
                    path.display()
                ));
            }
            std::fs::remove_dir_all(&path)
                .map_err(|error| format!("could not clear build scratch: {error}"))?;
        }
        std::fs::create_dir(&path)
            .map_err(|error| format!("could not create build scratch: {error}"))?;
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        std::fs::write(
            path.join(ACTIVE_TMP_MARKER),
            format!("pid={}\nstarted={started}\n", std::process::id()),
        )
        .map_err(|error| format!("could not mark build scratch active: {error}"))?;
        Ok(BuildScratch { path })
    }
}

impl Drop for BuildScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// D-BFS1: compile a library package's `Cargo.toml` to an rlib artifact.
///
/// The rlib is placed *inside* the hangar object (`pkg_dir`, the object root) so
/// the object is self-contained and content-addressed. The cargo target dir is
/// a hangar-scoped scratch (`<hangar>/build-scratch/<key>`) swept immediately
/// after the build and on crash (D-JPK-GC1). A prior realize of the same
/// content-addressed object leaves the rlib in place, so the rebuild is skipped
/// (cache hit). Returns the rlib path plus the actual sandbox receipt, or an
/// error. Every failure is returned to the caller. A missing rlib is not a valid
/// library realization: silently returning an empty artifact would make the
/// package appear built while leaving the eventual failure to an unrelated
/// linker or importer.
///
/// `toolchain` is the resolved pinned/realized build toolchain
/// (D-JPK-BUILDTOOL1=A): the build execs *its* `cargo`, so a bridge's output
/// hash does not depend on whatever host `cargo` happens to be on PATH when the
/// toolchain is a pinned object.
struct CargoBuildReceipt {
    rlib: String,
    sandbox_class: String,
    sandbox_policy: String,
}

enum CargoBuildError {
    SandboxUnavailable(String),
    Failed(String),
}

#[cfg(test)]
pub(crate) fn build_rlib_from_cargo(
    pkg_dir: &Path,
    hangar_dir: &Path,
    toolchain: &crate::Toolchain::Toolchain,
) -> Result<String, String> {
    build_rlib_from_cargo_mode(pkg_dir, hangar_dir, toolchain, false)
        .map(|build| build.rlib)
        .map_err(|error| match error {
            CargoBuildError::SandboxUnavailable(reason) | CargoBuildError::Failed(reason) => reason,
        })
}

fn build_rlib_from_cargo_mode(
    pkg_dir: &Path,
    hangar_dir: &Path,
    toolchain: &crate::Toolchain::Toolchain,
    offline: bool,
) -> Result<CargoBuildReceipt, CargoBuildError> {
    if offline && !toolchain.pinned {
        return Err(CargoBuildError::Failed(
            "offline Core builds require a pinned realized toolchain".to_string(),
        ));
    }
    if offline
        && !std::fs::symlink_metadata(pkg_dir.join("Cargo.lock"))
            .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            .unwrap_or(false)
    {
        return Err(CargoBuildError::Failed(
            "offline Core builds require a regular Cargo.lock".to_string(),
        ));
    }
    if toolchain.pinned {
        let hangar = std::fs::canonicalize(hangar_dir).map_err(|error| {
            CargoBuildError::Failed(format!(
                "could not resolve the immutable tool closure: {error}"
            ))
        })?;
        let cargo = std::fs::canonicalize(&toolchain.cargo).map_err(|error| {
            CargoBuildError::Failed(format!(
                "could not resolve pinned cargo in the immutable tool closure: {error}"
            ))
        })?;
        if !cargo.starts_with(&hangar) {
            return Err(CargoBuildError::Failed(
                "pinned cargo is outside the immutable tool closure".to_string(),
            ));
        }
    }
    // Cache hit: a previously realized object already carries its rlib.
    if let Some(existing) = find_rlib_in(pkg_dir) {
        return Ok(CargoBuildReceipt {
            rlib: existing,
            sandbox_class: "non-executing".to_string(),
            sandbox_policy: "no child launched (rlib already present)".to_string(),
        });
    }
    let cache_key = pkg_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "pkg".to_string());
    let scratch = BuildScratch::new(hangar_dir, &cache_key).map_err(CargoBuildError::Failed)?;
    let mut args = vec![
        "build".to_string(),
        "--lib".to_string(),
        "--release".to_string(),
        "--manifest-path".to_string(),
        "/work/source/Cargo.toml".to_string(),
        "--locked".to_string(),
    ];
    if offline {
        args.push("--offline".to_string());
    }
    let home = scratch.path.join("home");
    let cargo_home = scratch.path.join("cargo-home");
    std::fs::create_dir_all(&home).map_err(|error| {
        CargoBuildError::Failed(format!("could not create Cargo home: {error}"))
    })?;
    std::fs::create_dir_all(&cargo_home).map_err(|error| {
        CargoBuildError::Failed(format!("could not create Cargo cache home: {error}"))
    })?;
    let mut env = BTreeMap::new();
    env.insert("CARGO_TARGET_DIR".to_string(), "/work/output".to_string());
    env.insert(
        "CARGO_HOME".to_string(),
        "/work/output/cargo-home".to_string(),
    );
    env.insert("HOME".to_string(), "/work/output/home".to_string());
    env.insert("SOURCE_DATE_EPOCH".to_string(), "0".to_string());
    let sandboxed = jet_comptime::Comptime::Build::run_native_sandboxed(
        &toolchain.cargo,
        &args,
        pkg_dir,
        Some(&scratch.path),
        &env,
        false,
    )
    .map_err(|error| {
        CargoBuildError::SandboxUnavailable(format!(
            "Core Cargo action could not enter the native sandbox: {error:?}"
        ))
    })?;
    let out = sandboxed.output;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(CargoBuildError::Failed(format!(
            "pinned cargo failed with {}{}{}",
            out.status,
            if stderr.is_empty() { "" } else { ": " },
            if stderr.is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            }
        )));
    }
    // Find the rlib in the scratch `release/` dir and copy it into the object.
    let release = scratch.path.join("release");
    let built = std::fs::read_dir(&release)
        .map_err(|error| {
            CargoBuildError::Failed(format!(
                "sandboxed cargo produced no release directory: {error}"
            ))
        })?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("rlib")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("lib"))
                    .unwrap_or(false)
        })
        .ok_or_else(|| {
            CargoBuildError::Failed("sandboxed cargo produced no lib*.rlib artifact".to_string())
        })?;
    let file_name = built.file_name().ok_or_else(|| {
        CargoBuildError::Failed("sandboxed cargo rlib has no file name".to_string())
    })?;
    let dest = pkg_dir.join(file_name);
    std::fs::copy(&built, &dest).map_err(|error| {
        CargoBuildError::Failed(format!("could not copy rlib into package object: {error}"))
    })?;
    Ok(CargoBuildReceipt {
        rlib: dest.to_string_lossy().into_owned(),
        sandbox_class: sandboxed.mechanism,
        sandbox_policy: sandboxed.policy,
    })
    // `scratch` drops here → the cargo target dir is swept.
}

/// Find a `lib*.rlib` already sitting in an object root (a cache hit).
fn find_rlib_in(pkg_dir: &Path) -> Option<String> {
    std::fs::read_dir(pkg_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("rlib")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("lib"))
                    .unwrap_or(false)
        })
        .map(|p| p.to_string_lossy().into_owned())
}
