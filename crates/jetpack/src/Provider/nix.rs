use super::*;
use crate::Store::{admit_nix_closure_with_progress, plan_nix_downloads, NixOutputRequest};

/// The Nix compatibility provider. It owns Nix authority, cache identity,
/// closure planning, and both indexed and fixture-backed realization.
pub(crate) struct NixProvider;

impl Provider for NixProvider {
    fn validate_cache_authority(
        &self,
        _spec: &RefSpec,
        _table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<(), ProviderError> {
        if let Some(project) = ctx.project_dir.filter(|path| path.is_dir()) {
            crate::Lock::load_strict(project).map_err(ProviderError::BadOutput)?;
        }
        Ok(())
    }

    fn cache_expectation(
        &self,
        spec: &RefSpec,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Option<crate::Store::CacheExpectation> {
        if let Some(project) = ctx.project_dir {
            if crate::Lock::load_strict(project).is_err() {
                return None;
            }
            if let Some((closure, env)) = crate::Lock::nix_realization(project, &spec.raw) {
                let platform = if env.platform.is_empty() {
                    closure.system.clone()
                } else {
                    env.platform.clone()
                };
                if env.output_hash.is_empty()
                    || closure.output != env.output_hash
                    || platform != closure.system
                {
                    return None;
                }
                let roots = ctx.nix_roots?;
                return Some(crate::Store::CacheExpectation {
                    identity: crate::Store::CacheIdentity {
                        source_fingerprint: env.output_hash.clone(),
                        recipe_fingerprint: crate::SHA256::sha256_hex(NIX_RECIPE_ID.as_bytes()),
                        policy_fingerprint: closure.cache_key.clone(),
                        platform,
                    },
                    owned_output: Some(
                        roots.hangar_dir().join("objects").join(&closure.output),
                    ),
                    allow_unsigned_local: true,
                });
            }
        }
        if let Some(project) = ctx.project_dir {
            if let Some((output, _, repository, authority, envelope)) =
                crate::Lock::registry_realization(project, "jetpackage", &spec.raw)
            {
                if (repository.starts_with("github:NixOS/nixpkgs#")
                    || authority.contains("jetpack-nix-fallback-v1"))
                    && !output.is_empty()
                    && envelope.output_hash == output
                    && !envelope.platform.is_empty()
                {
                    let platform = envelope.platform;
                    let roots = ctx.nix_roots?;
                    return Some(crate::Store::CacheExpectation {
                        identity: nix_cache_identity(&output, &platform, spec, table, ctx),
                        owned_output: Some(roots.hangar_dir().join("objects").join(&output)),
                        allow_unsigned_local: true,
                    });
                }
            }
        }
        if let Some(index) = ctx.nix_index {
            if let Ok(Some(recipe)) = index.resolve_native_recipe(&spec.package) {
                return Some(native::catalog_cache_expectation(spec, &recipe, ctx));
            }
        }
        None
    }

    fn plan_downloads(
        &self,
        specs: &[RefSpec],
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<DownloadPlan, ProviderError> {
        let index = ctx.nix_index.ok_or_else(|| {
            ProviderError::Unsupported(format!(
                "download planning needs a signed index for `{}`",
                specs
                    .first()
                    .map(|spec| spec.raw.as_str())
                    .unwrap_or("Nix package")
            ))
        })?;
        let mut plan = DownloadPlan::default();
        let mut nix_paths = Vec::new();

        for spec in specs {
            if let Some(recipe) = index
                .resolve_native_recipe(&spec.package)
                .map_err(ProviderError::NixIndex)?
            {
                plan.add_item(PlanItem {
                    package: spec.raw.clone(),
                    state: PlanState::New,
                    download_bytes: native::catalog_download_size(&recipe)?,
                    disk_bytes: None,
                });
                continue;
            }
            let host_system = host_nix_system().ok_or_else(|| {
                ProviderError::Unsupported(
                    "the host system is not supported by the signed nixpkgs index".into(),
                )
            })?;
            let key = locked_nix_index_key(spec, table, ctx, host_system)?;
            let verified = match index.resolve(&key) {
                Ok(verified) => verified,
                Err(NixIndexError::NotIndexed { .. }) => {
                    // The fallback policy and executable are checked during
                    // realization. Planning keeps this item unknown.
                    plan.add_item(PlanItem {
                        package: spec.raw.clone(),
                        state: PlanState::New,
                        download_bytes: None,
                        disk_bytes: None,
                    });
                    continue;
                }
                Err(error) => return Err(ProviderError::NixIndex(error)),
            };
            nix_paths.extend(verified.record.outputs.values().cloned());
        }

        if !nix_paths.is_empty() {
            let roots = ctx.nix_roots.ok_or_else(|| {
                ProviderError::BadOutput(
                    "download planning has no Hangar roots for closure admission".into(),
                )
            })?;
            let nix = plan_nix_downloads(roots, &nix_paths, ctx.offline, current_progress())
                .map_err(|error| nix_cache_error(roots, error))?;
            plan.add_nix(nix);
        }
        Ok(plan)
    }

    fn can_repair(&self, spec: &RefSpec, table: &SourceTable, ctx: &Ctx) -> bool {
        ctx.fixtures.is_none()
            && locked_nix_index_key(spec, table, ctx, host_nix_system().unwrap_or_default())
                .is_ok()
    }

    fn realize(
        &self,
        spec: &RefSpec,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Realized, ProviderError> {
        self.validate_cache_authority(spec, table, ctx)?;
        if let Some(project) = ctx.project_dir.filter(|path| path.is_dir()) {
            if crate::Lock::locked_nix_package(project, &spec.raw).is_some() {
                return Err(ProviderError::BadOutput(
                    "fully locked Nix realization must replay its project CAS bundle before catalog discovery"
                        .into(),
                ));
            }
        }
        if let Some(index) = ctx.nix_index {
            if let Some(recipe) = index
                .resolve_native_recipe(&spec.package)
                .map_err(ProviderError::NixIndex)?
            {
                return native::realize_catalog_recipe(spec, &recipe, ctx);
            }
        }
        if let Some(dir) = ctx.fixtures {
            let path = dir.join(fixture_name(spec));
            let stdout =
                std::fs::read_to_string(&path).map_err(|_| ProviderError::FixtureMissing(path))?;
            let mut realized = parse_realization(spec, &stdout)?;
            realized
                .producer
                .facts
                .insert(NIX_NATIVE_FORMAT.to_string(), "json".to_string());
            realized
                .producer
                .facts
                .insert(NIX_NATIVE_DOCUMENT.to_string(), stdout);
            return finalize_nix_realization(spec, table, ctx, realized);
        }

        let index = ctx.nix_index.ok_or_else(|| {
            if ctx.offline {
                ProviderError::Offline(format!(
                    "`{}` is not in the hangar and --offline forbids fetching provider output",
                    spec.raw
                ))
            } else {
                ProviderError::Unsupported(format!(
                    "Nix package realization needs an exact signed-index record or an exact-lock local fallback policy for `{}`; ambient nixpkgs is never accepted",
                    spec.raw
                ))
            }
        })?;
        let host_system = host_nix_system().ok_or_else(|| {
            ProviderError::Unsupported(
                "the host system is not supported by the signed nixpkgs index".into(),
            )
        })?;
        let key = locked_nix_index_key(spec, table, ctx, host_system)?;
        let verified = match index.resolve(&key) {
            Ok(verified) => verified,
            Err(NixIndexError::NotIndexed { .. })
                if crate::NixFallbackPolicy::allowed_from_environment(ctx.offline) =>
            {
                return realize_from_local_nix(spec, table, ctx, &key);
            }
            Err(error) => return Err(ProviderError::NixIndex(error)),
        };
        let roots = ctx.nix_roots.ok_or_else(|| {
            ProviderError::BadOutput(
                "index-backed Nix realization has no Hangar roots for closure admission".into(),
            )
        })?;
        let requests = verified
            .record
            .outputs
            .iter()
            .map(|(name, store_path)| NixOutputRequest {
                name: name.clone(),
                store_path: store_path.clone(),
            })
            .collect::<Vec<_>>();
        let admitted = admit_nix_closure_with_progress(
            roots,
            &requests,
            ctx.offline,
            current_progress(),
        )
        .map_err(|error| nix_cache_error(roots, error))?;
        let realized = realization_from_index(spec, &key, verified, admitted)?;
        finalize_nix_realization(spec, table, ctx, realized)
    }
}
