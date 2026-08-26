use super::*;

/// U20: realize an inline `Pkg.adapt(...)` plan into the same `Realized`
/// boundary as provider-backed packages.
pub(crate) fn realize_adapter(
    plan: &AdapterPlan,
    ctx: &Ctx,
    expected: &crate::Store::CacheExpectation,
    tools: &HashMap<String, PathBuf>,
    table: &SourceTable,
) -> Result<Realized, ProviderError> {
    let source_ref = crate::RefSpec::classify_provider_ref(&plan.source).map_err(|_| {
        ProviderError::Adapter(format!(
            "adapter source `{}` is not a provider ref",
            plan.source
        ))
    })?;
    let staged = stage_adapter_source(&source_ref, ctx)?;
    let recipe = adapter_recipe_to_build(&plan.recipe);
    let recipe_hash = recipe.recipe_hash();
    let source_hash = tree_fingerprint(&staged).map_err(ProviderError::Adapter)?;
    let source_fingerprint = crate::Envelope::try_output_hash_of(&staged.to_string_lossy())
        .map_err(ProviderError::Adapter)?;
    let identity_source = if matches!(&plan.recipe, AdapterRecipe::Build(_)) {
        &source_fingerprint
    } else {
        &source_hash
    };
    let build_identity = adapter_action_identity(
        plan,
        &recipe,
        identity_source,
        &crate::Envelope::host_platform(),
        table,
    );
    let identity = adapter_cache_identity(&source_fingerprint, &build_identity, ctx);
    if identity != expected.identity {
        return Err(ProviderError::Adapter(
            "adapter source or build identity changed after approval".to_string(),
        ));
    }
    let id_input = format!(
        "u20-adapter-v1\nname={}\nsource={}\nsource_hash={}\nidentity={}\n",
        plan.name, plan.source, source_hash, build_identity
    );
    let fp = SHA256::sha256_hex(id_input.as_bytes());
    let out_dir = ctx
        .store_dir
        .join(format!("{}-adapter-{}", plan.name, &fp[..12]));
    if out_dir.exists() {
        return Err(ProviderError::Adapter(format!(
            "unverified existing output {}; run `jet clean` before rebuilding",
            out_dir.display()
        )));
    }
    let fetch_cache = ctx.store_dir.join("fetch-cache");
    let build_ctx = BuildContext {
        source_dir: &staged,
        output_root: &out_dir,
        tools: tools.clone(),
        fetch_cache: &fetch_cache,
        offline: ctx.offline,
    };
    let mut attempt = crate::BuildDebug::Attempt::new(
        &plan.name,
        &format!("adapt:{}:{}", plan.name, plan.source),
        "adapter",
        &recipe_hash,
        &source_hash,
    );
    let run_report = match Recipe::run_logged(&recipe, &build_ctx, None, &mut attempt) {
        Ok(report) => report,
        Err(d) => {
            if attempt.steps.is_empty() {
                attempt.push_step(crate::BuildDebug::StepLog {
                    index: 0,
                    total: 0,
                    name: "recipe validation".into(),
                    command: d.what.clone(),
                    cwd: staged.to_string_lossy().into_owned(),
                    status: "failed".into(),
                    stdout: String::new(),
                    stderr: format!("{}: {}\n", d.code, d.why),
                });
            }
            attempt.mark_failed();
            let scratch_error = attempt
                .preserve_scratch(ctx.store_dir, &staged, &out_dir)
                .err()
                .map(|error| format!("; preserved scratch unavailable: {error}"))
                .unwrap_or_default();
            let _ = attempt.persist(ctx.store_dir);
            if d.code == "E1275" {
                return Err(ProviderError::SandboxUnavailable(format!(
                    "adapter `{}` refused before its executable action launched: {}",
                    plan.name, d.why
                )));
            }
            let message = format!(
                "adapter `{}` failed at step {} of {}: {} — full log: `jet logs {}`; rerun with `--shell-on-fail` to debug inside {}{}",
                plan.name,
                attempt.failed_step,
                attempt.steps.len(),
                d.what,
                plan.name,
                attempt.scratch_dir,
                scratch_error
            );
            return Err(ProviderError::BuildDebug(message));
        }
    };
    let _ = attempt.persist(ctx.store_dir);
    crate::Store::seal_local_output(&out_dir).map_err(|error| {
        ProviderError::Adapter(format!("could not seal adapter output: {error}"))
    })?;
    let out = out_dir.to_string_lossy().into_owned();
    let bin_dir = out_dir.join("bin");
    let bin = if bin_dir.is_dir() {
        bin_dir.to_string_lossy().into_owned()
    } else {
        String::new()
    };
    let envelope = crate::Envelope::Envelope::for_output(
        &out,
        &format!("adapt:{}:{}", plan.name, plan.source),
        &format!("adapter:{build_identity}"),
    );
    let declared_dependencies = adapter_dependency_refs(plan).join(",");
    let declared_capabilities = recipe.declared_capabilities().join(",");
    let declared_authority = table.trust_lines().join("\n");
    let sandbox_class = run_report.sandbox_class.clone();
    let sandbox_policy = run_report.sandbox_policy.clone();
    let private_untrusted = matches!(&plan.recipe, AdapterRecipe::Build(_));
    let replay = Recipe::lower_to_plan(&recipe, &plan.name, &build_ctx.tools)
        .map_err(|d| ProviderError::Adapter(d.what))?
        .replay_record()
        .map_err(ProviderError::Adapter)?;
    let mut replay_facts = replay.facts().clone();
    replay_facts.insert(
        "adapter.build.dependencies".to_string(),
        declared_dependencies.clone(),
    );
    replay_facts.insert("adapter.build.identity".to_string(), build_identity.clone());
    replay_facts.insert(
        "adapter.build.authority".to_string(),
        declared_authority.clone(),
    );
    replay_facts.insert("adapter.build.sandbox".to_string(), sandbox_class.clone());
    replay_facts.insert(
        "adapter.build.sandbox_policy".to_string(),
        sandbox_policy.clone(),
    );
    if private_untrusted {
        replay_facts.insert(
            "adapter.build.trust".to_string(),
            "private-untrusted".to_string(),
        );
    }
    let replay = crate::Comptime::Build::BuildPlanReplay::from_facts(replay_facts)
        .map_err(ProviderError::Adapter)?;
    let mut producer_facts = BTreeMap::from([
        ("adapter.source".into(), plan.source.clone()),
        ("build.identity".into(), build_identity.clone()),
        ("build.capabilities".into(), declared_capabilities.clone()),
        ("build.dependencies".into(), declared_dependencies.clone()),
        ("build.sandbox".into(), sandbox_class.clone()),
        ("build.sandbox_policy".into(), sandbox_policy.clone()),
    ]);
    if private_untrusted {
        producer_facts.insert("build.trust".into(), "private-untrusted".into());
    }
    let producer = crate::Store::ProducerRecord::new(
        "adapter",
        format!("cas:{source_fingerprint}"),
        &source_fingerprint,
        replay,
        format!(
            "declared-tools={}\nbuild-identity={build_identity}\ncapabilities={}\ndependencies={}\nauthority={}\nsandbox={}\nsandbox-policy={}",
            declared_dependencies,
            declared_capabilities,
            declared_dependencies,
            declared_authority,
            sandbox_class,
            sandbox_policy,
        ),
        format!("policy={}\nplatform={}", identity.policy_fingerprint, identity.platform),
        producer_facts,
    )
    .map_err(ProviderError::Adapter)?;
    let mut realized = Realized {
        name: plan.name.clone(),
        version: String::new(),
        reference: format!("adapt:{}:{}", plan.name, plan.source),
        out,
        bin,
        rlib: String::new(),
        envelope,
        cache_identity: identity,
        source_state: SourceState::Built,
        named_outputs: BTreeMap::from([("out".into(), out_dir.to_string_lossy().into_owned())]),
        references: Vec::new(),
        producer,
    };
    refresh_provider_facts(&mut realized.producer, &realized.reference)?;
    Ok(realized)
}

pub(crate) fn adapter_recipe_to_build(recipe: &AdapterRecipe) -> BuildRecipe {
    match recipe {
        AdapterRecipe::Copy => BuildRecipe {
            steps: vec![BuildStep::InstallTree {
                src: ".".to_string(),
                dest: ".".to_string(),
            }],
        },
        AdapterRecipe::Prebuilt { bin, as_name } => BuildRecipe {
            steps: vec![BuildStep::Install {
                src: bin.clone(),
                dest: format!("bin/{as_name}"),
            }],
        },
        AdapterRecipe::Build(recipe) => recipe.clone(),
    }
}

pub(crate) fn stage_adapter_source(
    source: &crate::RefSpec::ProviderRef,
    ctx: &Ctx,
) -> Result<PathBuf, ProviderError> {
    match source.provider {
        Source::Path => {
            let (target, _) = crate::RefSpec::split_channel_ref(&source.target);
            let path = PathBuf::from(target);
            let path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir().unwrap_or_default().join(path)
            };
            if path.is_dir() {
                Ok(path)
            } else {
                Err(ProviderError::Adapter(format!(
                    "adapter source `{}` is not a directory",
                    path.display()
                )))
            }
        }
        Source::Github => {
            let remote = parse_remote_source(&format!("github:{}", source.target))?;
            fetch_remote_repo(&remote, ctx)
        }
        Source::Jetpack | Source::Nixpkgs => Err(ProviderError::Adapter(
            "`...@jetpack` is an index source, not source bytes; use `jetpack add <ref> --adapt` to draft a concrete adapter.".to_string(),
        )),
        Source::Cran => Err(ProviderError::Adapter(
            "CRAN packages must be realized before they can be adapter source bytes.".to_string(),
        )),
        Source::LuaRocks => Err(ProviderError::Adapter(
            "LuaRocks packages must be realized before they can be adapter source bytes."
                .to_string(),
        )),
        Source::RubyGems | Source::Cpan | Source::Packagist => Err(ProviderError::Adapter(
            "scripting-registry packages must be realized before they can be adapter source bytes."
                .to_string(),
        )),
        Source::JetRegistry
        | Source::Npm
        | Source::Cargo
        | Source::PyPI
        | Source::SwiftPM
        | Source::Releases => Err(ProviderError::Adapter(
            "Jet registry, npm, and Cargo packages must be realized before they can be adapter source bytes."
                .to_string(),
        )),
        Source::Named(_) => Err(ProviderError::Adapter(
            "adapter source must be a source ref such as `owner/repo@github` or a bare path such as `./vendor/tool`.".to_string(),
        )),
    }
}
