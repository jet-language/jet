//! The `env.jet` → `EnvPlan` driver: route the typed `module { … }` surface,
//! discover `imports: find(…)` files (U4), lower typed integration imports, and build the merged `(name → upstream)`
//! source table (U5/U6/U9), and fold every module's contributions into the
//! runnable plan.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::AST::{Expr, Item, Namespace, StrPart};

use super::super::Merge;
use super::super::RefSpec::{self, ProviderKind, SourceTable};
use super::Diagnostics::{
    bad_import_directive, bad_source_ref, discovered_module_imports, find_dir_missing,
    fleet_unknown_system, image_from_unknown_system, merge_error_to_diagnostic,
    oci_from_non_executable,
};
use super::Eval::{evaluate_modules, merge_all, parse_program, pkg_ref};
use super::Environment::{
    qualified_call_name, EnvironmentIntegration, EnvironmentLifecycle, IntegrationFactProjection,
    IntegrationKind, LanguagePackCatalog, LanguageSpec, ManagedFile, PackageProfileFact,
    PackageProfilePlan, PackageProfileSet, ProfileSet,
};
use super::Types::{
    AdapterPlan, EnvPlan, FleetPlan, ImageKind, ImagePlan, PromptPathMode, PromptStripMode,
    SystemPlan,
};

/// True when `src` uses the typed `module { … }` surface (U3/U8) rather than
/// the Phase-1 `pkg.*` directive surface. The CLI routes loading on this: a
/// file that declares the `module` keyword stays on this path even when its
/// later syntax is malformed; only the legacy directive surface uses the
/// tolerant fallback scanner.
pub fn is_module_surface(src: &str) -> bool {
    let (toks, diags) = crate::Lexer::lex(src);
    let has_module = toks
        .iter()
        .any(|token| matches!(&token.kind, crate::Lexer::TokKind::KwModule));
    if !diags.is_empty() {
        return has_module;
    }
    match crate::Parser::parse(&toks) {
        Ok(program) => program
            .items
            .iter()
            .any(|item| matches!(item, Item::Module(_))),
        Err(_) => has_module,
    }
}

/// Evaluate a typed `env.jet` (the `module name { sources:/imports:/env.X: }`
/// surface, U3/U6/U8) into an `EnvPlan`. Sources merge across modules by key
/// (U5); package sugar resolves to `package@source` refs; the `prompt`
/// scalar becomes the label. `imports: find(…)` is walked before evaluation;
/// typed integration calls lower into the same source and environment graph.
pub fn evaluate_env(src: &str, base_dir: &Path) -> Result<EnvPlan, Diagnostic> {
    evaluate_env_with_profile(src, base_dir, None)
}

/// Evaluate one source-backed package profile without realizing or mutating
/// the store. This is the production path for `jet profile plan`; callers get
/// provider identity, source provenance, and collision selections from the
/// same graph used by environment evaluation.
pub fn evaluate_package_profile(
    src: &str,
    base_dir: &Path,
    name: &str,
) -> Result<PackageProfilePlan, Diagnostic> {
    let env = evaluate_env(src, base_dir)?;
    let mut profiles = PackageProfileSet::default();
    for profile in env.package_profiles {
        profiles.insert_checked(profile).map_err(|error| {
            Diagnostic::error(
                "E1332",
                format!("package profile composition failed: {error}"),
                "one source-backed package profile cannot silently choose different inheritance, package, or collision facts".to_string(),
                "merge the declarations so they agree, or give the profiles different names".to_string(),
                None,
            )
        })?;
    }
    let resolved = profiles.resolve(name).map_err(|error| {
        Diagnostic::error(
            "E1332",
            format!("package profile `{name}` could not be resolved: {error}"),
            "profile inheritance is resolved parent-first and must remain acyclic".to_string(),
            "fix the profile name, parent reference, or inheritance cycle".to_string(),
            None,
        )
    })?;
    let package_names = resolved
        .packages
        .iter()
        .map(|package| package.raw.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for (path, provider) in &resolved.collisions {
        if !package_names.contains(provider) {
            return Err(Diagnostic::error(
                "E1335",
                format!(
                    "package profile `{name}` selects `{provider}` for `{path}`, but that provider is not in the profile"
                ),
                "a collision selection must name one exact package contender retained by the source-backed profile".to_string(),
                "add the selected package ref to `packages`, or select one of the existing contenders".to_string(),
                None,
            ));
        }
    }
    let mut packages = Vec::new();
    for package in &resolved.packages {
        let spec = classify_profile_ref(&package.raw, &env.table).map_err(|error| {
            Diagnostic::error(
                "E1335",
                format!("package profile `{name}` contains unsupported ref `{}`: {error}", package.raw),
                "profile package facts must retain one lossless package and provider identity".to_string(),
                "use `package@source` with a built-in or declared source".to_string(),
                None,
            )
        })?;
        let (target, channel) = RefSpec::split_channel_ref(&spec.package);
        // Keep the source token from the declaration. `default` is the
        // package-sugar spelling for nixpkgs, but changing it to `nixpkgs`
        // here would erase source identity from the plan and later lock views.
        let source = package
            .raw
            .rsplit_once(Syntax::REF_PROVIDER_AT)
            .map(|(_, source)| source)
            .unwrap_or_else(|| spec.source.label())
            .to_string();
        packages.push(PackageProfileFact {
            raw: package.raw.clone(),
            target: target.to_string(),
            source: source.clone(),
            upstream: env.table.upstream(&source).map(str::to_string),
            provider: env.table.provider(&source).label().to_string(),
            channel: channel.map(|value| value.as_str().to_string()),
            declared_by: package.declared_by.clone(),
        });
    }
    Ok(PackageProfilePlan {
        name: resolved.name,
        selected_profiles: resolved.selected_profiles,
        applied: resolved.applied,
        packages,
        collisions: resolved.collisions,
        sources: resolved.sources,
    })
}

fn classify_profile_ref(
    raw: &str,
    table: &SourceTable,
) -> Result<RefSpec::RefSpec, String> {
    if let Some((package, source)) = raw.rsplit_once(Syntax::REF_PROVIDER_AT) {
        if source == Syntax::DEFAULT_SOURCE {
            return Ok(RefSpec::RefSpec {
                source: RefSpec::Source::Nixpkgs,
                package: package.to_string(),
                raw: raw.to_string(),
            });
        }
    }
    RefSpec::classify_in(raw, table).map_err(|error| format!("{error:?}"))
}

/// Evaluate a typed environment with one authoritative profile selection.
/// An explicit CLI choice is resolved before ambient hostname/user matching,
/// so an unrelated ambient profile cannot reject or augment the requested
/// plan.
pub fn evaluate_env_with_profile(
    src: &str,
    base_dir: &Path,
    requested_profile: Option<&str>,
) -> Result<EnvPlan, Diagnostic> {
    evaluate_env_with_profiles(src, base_dir, requested_profile, None)
}

/// Evaluate an environment while explicitly selecting one `env.<name>`
/// environment profile. The ordinary CLI uses the deterministic default
/// (`dev`, then `default`, then lexical order) when it has no selector.
pub fn evaluate_env_with_environment_profile(
    src: &str,
    base_dir: &Path,
    requested_environment_profile: Option<&str>,
) -> Result<EnvPlan, Diagnostic> {
    evaluate_env_with_profiles(src, base_dir, None, requested_environment_profile)
}

pub fn evaluate_env_with_profiles(
    src: &str,
    base_dir: &Path,
    requested_profile: Option<&str>,
    requested_environment_profile: Option<&str>,
) -> Result<EnvPlan, Diagnostic> {
    let program = parse_program(src)?;
    let environment_root = std::fs::canonicalize(base_dir).map_err(|error| {
        Diagnostic::error(
            "E1331",
            format!("environment root `{}` cannot be resolved: {error}", base_dir.display()),
            "one environment graph must resolve from a real project directory before it follows imports or path-backed facts".to_string(),
            "run the command from an existing project directory".to_string(),
            None,
        )
    })?;

    // The root `env.jet` plus every file reachable through `imports: find(…)`
    // (U4). Each unit owns its source text (spans index into it) and the dir its
    // relative refs / `embed_file` resolve against.
    let mut units = vec![EvalUnit {
        items: program.items,
        src: src.to_string(),
        base_dir: environment_root.clone(),
        source_path: environment_root.join(Syntax::ENV_FILE),
    }];
    let discovered = discover_imports(&units[0], &environment_root)?;
    units.extend(discovered);
    let source_files = units
        .iter()
        .filter_map(|unit| {
            unit.source_path
                .strip_prefix(&environment_root)
                .ok()
                .map(|path| {
                    path.to_string_lossy()
                        .replace(std::path::MAIN_SEPARATOR, "/")
                })
        })
        .collect::<Vec<_>>();

    let table = build_source_table(&units)?;

    // Evaluate every unit's modules (each against its own source + base dir),
    // then merge all contributions through the §6 engine as one pass — so a
    // discovered module's `env.dev` packages combine with the root's, and a
    // cross-file source/scalar conflict still surfaces as E0967.
    let mut modules = Vec::new();
    for unit in &units {
        modules.extend(evaluate_modules(&unit.items, &unit.src, &unit.base_dir)?);
    }

    // U11/U14: collect every captured System/Image across all modules (source
    // order), then cross-check each image's `from:` against the known systems.
    let mut systems: Vec<SystemPlan> = Vec::new();
    let mut images: Vec<ImagePlan> = Vec::new();
    let mut fleets: Vec<FleetPlan> = Vec::new();
    let mut vmtests = Vec::new();
    let mut dev_services: Vec<super::Types::DevServicePlan> = Vec::new();
    let mut secrets: Vec<String> = Vec::new();
    let mut adapters: Vec<AdapterPlan> = Vec::new();
    let mut lifecycle = EnvironmentLifecycle::default();
    let mut profiles = ProfileSet::default();
    let mut package_profiles = PackageProfileSet::default();
    let mut languages = Vec::new();
    let mut files: Vec<ManagedFile> = Vec::new();
    let mut integrations: Vec<EnvironmentIntegration> = Vec::new();
    let mut integration_facts = IntegrationFactProjection::default();
    let mut integration_packages = Vec::new();
    let environment_names: std::collections::BTreeSet<String> = modules
        .iter()
        .flat_map(|module| module.entries.iter())
        .filter_map(|((namespace, name), _)| (*namespace == Namespace::Env).then_some(name.clone()))
        .collect();
    for module in &modules {
        systems.extend(module.systems.iter().cloned());
        images.extend(module.images.iter().cloned());
        fleets.extend(module.fleets.iter().cloned());
        vmtests.extend(module.vmtests.iter().cloned());
        for service in &module.dev_services {
            if dev_services
                .iter()
                .any(|existing: &super::Types::DevServicePlan| existing.name == service.name)
            {
                return Err(Diagnostic::error(
                    "E1262",
                    format!("service `{}` is declared more than once", service.name),
                    "one environment supervisor owns each service name and cannot choose between different process facts".to_string(),
                    "merge the service records or give them distinct names".to_string(),
                    None,
                ));
            }
            dev_services.push(service.clone());
        }
        for secret in &module.secrets {
            push_unique(&mut secrets, secret.clone());
        }
        adapters.extend(module.adapters.iter().cloned());
        for integration in &module.integrations {
            if let Some(existing) = integrations
                .iter()
                .find(|existing| existing.name == integration.name)
            {
                if existing != integration {
                    return Err(Diagnostic::error(
                        "E1335",
                        format!("integration `{}` has conflicting declarations", integration.name),
                        "one environment graph cannot silently choose different SDK, host, credential, or grant facts".to_string(),
                        "merge the integration options so they agree, or keep one declaration".to_string(),
                        None,
                    ));
                }
            } else {
                integrations.push(integration.clone());
            }
            for package in &integration.packages {
                push_unique(&mut integration_packages, package.clone());
            }
            for file in &integration.files {
                if let Some(existing) = files.iter().find(|item| item.destination == file.destination) {
                    if existing != file {
                        return Err(Diagnostic::error(
                            "E1326",
                            format!("managed file `{}` has conflicting declarations", file.destination),
                            "one environment graph cannot apply two different owners to the same destination".to_string(),
                            "merge the file declarations or choose distinct destinations".to_string(),
                            None,
                        ));
                    }
                } else {
                    files.push(file.clone());
                }
            }
            for task in &integration.tasks {
                push_unique(&mut integration_facts.tasks, task.clone());
                let fact = super::Environment::IntegrationTaskFact {
                    name: task.clone(),
                    integration: integration.kind,
                    packages: integration.packages.clone(),
                    secrets: integration.secrets.clone(),
                    providers: integration.providers.clone(),
                    host_checks: integration.host_checks.clone(),
                    grants: integration.grants.clone(),
                };
                if !integration_facts.task_facts.contains(&fact) {
                    integration_facts.task_facts.push(fact);
                }
            }
            for provider in &integration.providers {
                push_unique(&mut integration_facts.providers, provider.clone());
            }
            for host_check in &integration.host_checks {
                push_unique(&mut integration_facts.host_checks, host_check.clone());
            }
            for grant in &integration.grants {
                push_unique(&mut integration_facts.grants, grant.clone());
            }
            for loss in &integration.losses {
                push_unique(&mut integration_facts.losses, loss.clone());
            }
        }
        for dotenv in &module.lifecycle.dotenv {
            if let Some(existing) = lifecycle.dotenv.iter().find(|item| item.file == dotenv.file) {
                if existing != dotenv {
                    return Err(Diagnostic::error(
                        "E1333",
                        format!("dotenv file `{}` has conflicting lifecycle policies", dotenv.file),
                        "one environment graph cannot silently choose different allowlists or secret classifications for the same dotenv file".to_string(),
                        "merge the declarations so their policies agree, or use different files".to_string(),
                        None,
                    ));
                }
            } else {
                lifecycle.dotenv.push(dotenv.clone());
            }
        }
        lifecycle.unset.extend(module.lifecycle.unset.iter().cloned());
        lifecycle.on_enter.extend(module.lifecycle.on_enter.iter().cloned());
        lifecycle.checks.extend(module.lifecycle.checks.iter().cloned());
        if module.lifecycle.reload_explicit {
            if lifecycle.reload_explicit && lifecycle.reload != module.lifecycle.reload {
                return Err(Diagnostic::error(
                    "E1333",
                    format!(
                        "reload policy in module `{}` conflicts with another module",
                        module.name
                    ),
                    "one environment graph cannot silently choose between different reload policies".to_string(),
                    "merge the reload declarations so they agree, or keep one policy owner".to_string(),
                    None,
                ));
            }
            lifecycle.reload = module.lifecycle.reload.clone();
            lifecycle.reload_explicit = true;
        }
        for profile in &module.profiles {
            profiles.insert_checked(profile.clone()).map_err(|error| {
                Diagnostic::error(
                    "E1332",
                    format!("environment profile composition failed: {error}"),
                    "one environment graph cannot silently choose between different facts for the same profile".to_string(),
                    "merge the profile declarations so they are identical, or give them different names".to_string(),
                    None,
                )
            })?;
        }
        for profile in &module.package_profiles {
            package_profiles.insert_checked(profile.clone()).map_err(|error| {
                Diagnostic::error(
                    "E1332",
                    format!("package profile composition failed: {error}"),
                    "one source-backed package profile cannot silently choose different inheritance, package, or collision facts".to_string(),
                    "merge the declarations so they agree, or give the profiles different names".to_string(),
                    None,
                )
            })?;
        }
        for language in &module.languages {
            if let Some(existing) = languages
                .iter()
                .find(|existing: &&LanguageSpec| existing.key() == language.key())
            {
                if !existing.same_selection(language) {
                    return Err(Diagnostic::error(
                        "E1333",
                        format!("language pack `{}` has conflicting selection facts", language.name),
                        "one environment graph cannot silently choose a version, channel, venv, or extra-package selection".to_string(),
                        "merge the language records so their typed facts agree".to_string(),
                        None,
                    ));
                }
            } else {
                languages.push(language.clone());
            }
        }
        for file in &module.files {
            if let Some(existing) = files.iter().find(|item| item.destination == file.destination) {
                if existing != file {
                    return Err(Diagnostic::error(
                        "E1326",
                        format!("managed file `{}` has conflicting declarations", file.destination),
                        "one environment graph cannot apply two different owners to the same destination".to_string(),
                        "merge the file declarations or choose distinct destinations".to_string(),
                        None,
                    ));
                }
            } else {
                files.push(file.clone());
            }
        }
    }
    let system_names: Vec<String> = systems.iter().map(|s| s.name.clone()).collect();
    // D-JPK-IMAGE1: an `.Oci` image's `from: packages.<name>` cross-checks against
    // this project's own `pkg.jet` `packages:` block (E1267) — a different
    // manifest than the `env.jet`/`config.jet` this pass is evaluating, so it's
    // loaded fresh here rather than threaded through as a plan field.
    let manifest = super::super::Package::PackageFacts::load(base_dir).and_then(|r| r.ok());
    for image in &images {
        match image.kind {
            ImageKind::Iso => {
                if !system_names.contains(&image.from) {
                    return Err(image_from_unknown_system(
                        &image.name,
                        &image.from,
                        &system_names,
                    ));
                }
            }
            ImageKind::Oci => {
                if image.from_environment {
                    if !environment_names.contains(&image.from) {
                        return Err(Diagnostic::error(
                            "E1327",
                            format!("the image `{}` names unknown environment `{}`", image.name, image.from),
                            "D-ENV-IMAGE1: an environment image projects one declared `env.<name>` fact graph into a runnable OCI image".to_string(),
                            format!("declare `module env.{} {{ … }}`, or point `from:` at an existing environment", image.from),
                            None,
                        ));
                    }
                    continue;
                }
                let kind = manifest.as_ref().and_then(|m| m.package_kind(&image.from));
                let is_executable = matches!(
                    kind,
                    Some(super::super::Package::PackageKind::Executable)
                );
                if !is_executable {
                    let is_library = matches!(
                        kind,
                        Some(super::super::Package::PackageKind::Library)
                    );
                    return Err(oci_from_non_executable(
                        &image.name,
                        &image.from,
                        is_library,
                    ));
                }
            }
        }
    }
    // U15: every fleet host must reference a known system (E1242).
    for fleet in &fleets {
        for host in &fleet.hosts {
            if !system_names.contains(&host.system) {
                return Err(fleet_unknown_system(
                    &fleet.name,
                    &host.name,
                    &host.system,
                    &system_names,
                ));
            }
        }
    }
    for vmtest in &vmtests {
        for host in &vmtest.hosts {
            if !system_names.contains(&host.system) {
                return Err(fleet_unknown_system(
                    &vmtest.name,
                    &host.name,
                    &host.system,
                    &system_names,
                ));
            }
        }
    }

    let merged = merge_all(&modules).map_err(|e| merge_error_to_diagnostic(&e))?;

    if let Err(error) = integration_facts.validate() {
        return Err(Diagnostic::error(
            "E1335",
            "environment integration lowering was lossy".to_string(),
            error,
            "use named secret references and supported typed integration arguments".to_string(),
            None,
        ));
    }

    // Select exactly one environment profile. The selected name is explicit when a host
    // asks for it; otherwise `dev`, then `default`, then lexical order gives
    // one stable beginner path instead of silently merging sibling profiles.
    let active_environment =
        select_active_environment(&environment_names, requested_environment_profile)?;
    let active_key = active_environment
        .as_ref()
        .map(|name| (Namespace::Env, name.clone()));
    let active_environment_provenance = active_key
        .as_ref()
        .map(|key| {
            modules
                .iter()
                .filter(|module| module.entries.iter().any(|(entry_key, _)| entry_key == key))
                .map(|module| module.name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut package_refs = Vec::new();
    package_refs.extend(integration_packages);
    let mut prompt = None;
    let mut prompt_path = PromptPathMode::default();
    let mut prompt_strip = PromptStripMode::default();
    if let Some(key) = &active_key {
        let entry = &merged[key];
        for pkg in &entry.packages {
            push_unique(&mut package_refs, pkg_ref(pkg));
        }
        if prompt.is_none() {
            if let Some(label) = entry.settings.get(Syntax::ENV_FIELD_PROMPT) {
                prompt = Some(label.clone());
            }
        }
        if let Some(path) = entry.settings.get(Syntax::PROMPT_SETTING_PATH) {
            prompt_path = prompt_path_mode(path);
        }
        if let Some(strip) = entry.settings.get(Syntax::PROMPT_SETTING_STRIP) {
            prompt_strip = prompt_strip_mode(strip);
        }
    }
    let selected_names = requested_profile
        .map(|name| vec![name.to_string()])
        .unwrap_or_else(|| {
            profiles.auto_select_many(
                &std::env::var("HOSTNAME").unwrap_or_default(),
                &std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_default(),
            )
        });
    let selected_profile = (!selected_names.is_empty())
        .then(|| profiles.resolve_many(&selected_names))
        .transpose()
        .map_err(|error| {
            Diagnostic::error(
                "E1332",
                format!("environment profile could not be resolved: {error}"),
                "profile inheritance is resolved parent-first and must remain acyclic".to_string(),
                "fix the profile name, parent reference, or inheritance cycle".to_string(),
                None,
            )
        })?;
    if let Some(profile) = &selected_profile {
        for package in &profile.packages {
            push_unique(&mut package_refs, package.clone());
        }
    }
    let catalog = LanguagePackCatalog::builtin();
    let language_expansion = catalog.expand(&languages).map_err(|error| {
        Diagnostic::error(
            "E1333",
            format!("environment language pack could not be expanded: {error}"),
            "language packs expand through one closed catalog into ordinary package refs".to_string(),
            "choose a language name from the catalog exposed by `jet env info`".to_string(),
            None,
        )
    })?;
    for package in &language_expansion.packages {
        push_unique(&mut package_refs, package.clone());
    }
    let language_packs = language_expansion.packs.clone();
    let language_projections = language_expansion.projections.clone();
    Ok(EnvPlan {
        table,
        source_files,
        package_refs,
        adapters,
        prompt,
        prompt_path,
        prompt_strip,
        systems,
        images,
        fleets,
        vmtests,
        dev_services,
        secrets,
        lifecycle,
        profiles: profiles.profiles.values().cloned().collect(),
        languages,
        selected_profile,
        language_expansion,
        language_packs,
        language_projections,
        files,
        integrations,
        integration_facts,
        package_profiles: package_profiles.profiles.values().cloned().collect(),
        environment_names: environment_names.into_iter().collect(),
        active_environment,
        active_environment_provenance,
    })
}

fn select_active_environment(
    names: &std::collections::BTreeSet<String>,
    requested_environment_profile: Option<&str>,
) -> Result<Option<String>, Diagnostic> {
    if let Some(name) = requested_environment_profile {
        if names.contains(name) {
            return Ok(Some(name.to_string()));
        }
        return Err(Diagnostic::error(
            "E1337",
            format!("environment profile `{name}` is not declared"),
            "one environment plan activates one declared `env.<name>` profile; sibling profiles stay available for explicit selection".to_string(),
            "choose one of the declared environment profile names".to_string(),
            None,
        ));
    }
    if names.contains("dev") {
        return Ok(Some("dev".to_string()));
    }
    if names.contains("default") {
        return Ok(Some("default".to_string()));
    }
    Ok(names.iter().next().cloned())
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn prompt_path_mode(value: &str) -> PromptPathMode {
    match value {
        Syntax::PROMPT_PATH_FULL => PromptPathMode::Full,
        _ => PromptPathMode::Short,
    }
}

fn prompt_strip_mode(value: &str) -> PromptStripMode {
    match value {
        Syntax::PROMPT_STRIP_ON => PromptStripMode::On,
        _ => PromptStripMode::Off,
    }
}

/// One parsed `.jet` file contributing modules: the root `env.jet` and every
/// file discovered through `imports: find(…)` (U4). Spans in `items` index into
/// this unit's own `src`; `base_dir` is the dir the file's relative refs and
/// `embed_file` calls resolve against.
struct EvalUnit {
    items: Vec<Item>,
    src: String,
    base_dir: PathBuf,
    source_path: PathBuf,
}

/// Walk every `imports: find("<dir>")` directive in the root unit's modules,
/// skipping recognized typed integrations, and return one `EvalUnit` per
/// discovered `*.jet` file (U4 import-tree
/// discovery). Discovery is one level deep: a discovered file may not itself
/// import (the liftability law — modules contribute to the merged whole, they
/// don't import each other; violations are E0971).
fn discover_imports(root: &EvalUnit, base_dir: &Path) -> Result<Vec<EvalUnit>, Diagnostic> {
    let mut out = Vec::new();
    let environment_root = std::fs::canonicalize(base_dir).map_err(|error| {
        Diagnostic::error(
            "E1331",
            format!("environment root `{}` cannot be resolved: {error}", base_dir.display()),
            "imports follow physical paths and cannot be evaluated from an unresolved root".to_string(),
            "run the command from an existing project directory".to_string(),
            None,
        )
    })?;
    let mut seen_files = std::collections::BTreeSet::new();
    for item in &root.items {
        let Item::Module(m) = item else { continue };
        if !m.is_auto_discovered() {
            continue;
        }
        let mut directives = Vec::new();
        for import in &m.imports {
            collect_import_directives(import, &mut directives);
        }
        for imp in directives {
            if let Some(name) = qualified_call_name(imp) {
                if IntegrationKind::from_call(&name).is_some() {
                    continue;
                }
            }
            let rel = find_dir_arg(imp)?;
            let relative = Path::new(&rel);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|component| component == std::path::Component::ParentDir)
            {
                return Err(Diagnostic::error(
                    "E1331",
                    format!("module import `{rel}` escapes the environment root"),
                    "one environment graph may compose files below its root, but an import cannot escape it".to_string(),
                    "use a project-relative directory without `..`".to_string(),
                    Some(imp.span()),
                ));
            }
            let dir = base_dir.join(&rel);
            let real_dir = std::fs::canonicalize(&dir).map_err(|_| find_dir_missing(&dir, imp.span()))?;
            if !real_dir.starts_with(&environment_root) {
                return Err(Diagnostic::error(
                    "E1331",
                    format!("module import `{rel}` resolves outside the environment root"),
                    "imports follow physical paths and cannot cross the project boundary".to_string(),
                    "remove the escaping symlink or move the imported directory below the project root".to_string(),
                    Some(imp.span()),
                ));
            }
            for file in list_jet_files(&real_dir, imp)? {
                let canonical_file = std::fs::canonicalize(&file).map_err(|_| {
                    find_dir_missing(&real_dir, imp.span())
                })?;
                if !canonical_file.starts_with(&environment_root) || !canonical_file.is_file() {
                    return Err(Diagnostic::error(
                        "E1331",
                        format!("discovered module `{}` resolves outside the environment root", file.display()),
                        "imports follow physical paths and cannot cross the project boundary".to_string(),
                        "remove the escaping symlink or move the module below the environment root".to_string(),
                        Some(imp.span()),
                    ));
                }
                if !seen_files.insert(canonical_file.clone()) {
                    continue;
                }
                let file_src = std::fs::read_to_string(&canonical_file)
                    .map_err(|_| find_dir_missing(&real_dir, imp.span()))?;
                let prog = parse_program(&file_src)?;
                // Liftability law (U4): a discovered module may not import.
                for nested in &prog.items {
                    if let Item::Module(nm) = nested {
                        if !nm.imports.is_empty() {
                            return Err(discovered_module_imports(&canonical_file));
                        }
                    }
                }
                let file_base = canonical_file
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| base_dir.to_path_buf());
                out.push(EvalUnit {
                    items: prog.items,
                    src: file_src,
                    base_dir: file_base,
                    source_path: canonical_file,
                });
            }
        }
    }
    Ok(out)
}

fn collect_import_directives<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    match expr {
        Expr::ListLit(items, _) => {
            for item in items {
                collect_import_directives(item, out);
            }
        }
        _ => out.push(expr),
    }
}

/// Extract the literal directory path from an `imports: find("dir")` directive.
/// Anything else — a non-`find` expression, the wrong arity, or a non-literal
/// (interpolated) argument — is E0969.
fn find_dir_arg(imp: &Expr) -> Result<String, Diagnostic> {
    let Expr::Call(call) = imp else {
        return Err(bad_import_directive(imp.span()));
    };
    if call.name != Syntax::BUILTIN_FIND || call.args.len() != 1 {
        return Err(bad_import_directive(imp.span()));
    }
    let Expr::Str(parts, _) = &call.args[0].expr else {
        return Err(bad_import_directive(imp.span()));
    };
    let mut path = String::new();
    for part in parts {
        match part {
            StrPart::Lit(s) => path.push_str(s),
            StrPart::Interp(..) => return Err(bad_import_directive(imp.span())),
        }
    }
    Ok(path)
}

/// List the `*.jet` files directly under `dir`, sorted for determinism. A
/// missing/unreadable directory is E0970.
fn list_jet_files(dir: &Path, imp: &Expr) -> Result<Vec<PathBuf>, Diagnostic> {
    let entries = std::fs::read_dir(dir).map_err(|_| find_dir_missing(dir, imp.span()))?;
    let mut files = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|_| find_dir_missing(dir, imp.span()))?
            .path();
        if path.extension().and_then(|e| e.to_str()) == Some(Syntax::FILE_EXT) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Merge every enabled module's `sources:` block — across the root and every
/// discovered unit — into one `(name → upstream)` table (U5: same name +
/// different ref conflicts, E0967). Each `target@provider` ref (D-JPK-REF1) is translated
/// to the colon/flake upstream the providers realize (`github:owner/repo/rev`,
/// `path:./local`, `nixpkgs:channel`).
fn build_source_table(units: &[EvalUnit]) -> Result<SourceTable, Diagnostic> {
    let mut maps: Vec<BTreeMap<String, String>> = Vec::new();
    // U9: the provider kind is *inferred*, never declared. We record each
    // source's kind here, keyed by name, as we resolve its target. The §6 merge
    // guarantees a given name resolves to one upstream (else E0967), so the
    // probe result is consistent across units.
    let mut kinds: BTreeMap<String, ProviderKind> = BTreeMap::new();
    for (idx, unit) in units.iter().enumerate() {
        // Spans index into each unit's own source, but the CLI renders against
        // the root `env.jet`. Only the root unit (index 0) can safely carry a
        // span; a discovered file's diagnostic is span-less so it never slices
        // the wrong source.
        let is_root = idx == 0;
        for item in &unit.items {
            let Item::Module(m) = item else { continue };
            if !m.is_auto_discovered() {
                continue;
            }
            let mut map: BTreeMap<String, String> = BTreeMap::new();
            for s in &m.sources {
                let ref_text = unit.src[s.ref_span.start..s.ref_span.end].trim();
                let span = if is_root { Some(s.ref_span) } else { None };
                let pref = RefSpec::classify_provider_ref(ref_text)
                    .map_err(|_| bad_source_ref(ref_text, span))?;
                let upstream = format!(
                    "{}{}{}",
                    pref.provider.label(),
                    Syntax::REF_SEPARATOR,
                    pref.target
                );
                if let Some(existing) = map.get(&s.name) {
                    if existing != &upstream {
                        return Err(merge_error_to_diagnostic(
                            &Merge::MergeError::SourceConflict {
                                name: s.name.clone(),
                                a: existing.clone(),
                                b: upstream,
                            },
                        ));
                    }
                    continue;
                }
                // Probe the resolved target against the *declaring file's* dir,
                // so a bare `./local` path resolves where it was written.
                let kind = infer_provider_kind(&pref, &unit.base_dir);
                if let Some(existing) = kinds.get(&s.name) {
                    if existing != &kind {
                        return Err(Diagnostic::error(
                            "E0967",
                            format!("source `{}` resolves to conflicting provider kinds", s.name),
                            "one named source must have one deterministic realization path across the environment graph".to_string(),
                            "make every declaration use the same source location and provider kind".to_string(),
                            span,
                        ));
                    }
                } else {
                    kinds.insert(s.name.clone(), kind);
                }
                map.insert(s.name.clone(), upstream);
            }
            maps.push(map);
        }
    }
    let merged = Merge::merge_sources(&maps).map_err(|e| merge_error_to_diagnostic(&e))?;
    Ok(SourceTable::from_decls(merged.into_iter().map(
        |(name, upstream)| {
            let via = kinds.get(&name).copied().unwrap_or_default();
            (name, upstream, via)
        },
    )))
}

/// U9: infer whether a source is realized by the first-party `core` provider or
/// the `nix` compatibility provider from its *resolved target* — no marker is
/// declared. The rule (see syntax-decisions.md U9, unified-ecosystem.md §6): a
/// target carrying a `pkg.jet` is a Jet package repo (→ `core`); otherwise it
/// is a nix flake (→ `nix`).
///
/// The probe must never clone a nixpkgs-sized repo just to classify it:
/// - a bare path stats the directory locally (offline, free) — resolved here to a
///   concrete `Core`/`Nix`;
/// - `…@nixpkgs` is unconditionally `nix` — never probed;
/// - `…@github` is left **`Infer`**: its kind depends on whether the remote
///   repo carries a `pkg.jet`, which only a realize-time probe can answer
///   (this pure pass has no offline flag or source cache). `Provider::
///   resolve_kind` does the lightweight git peek when realization runs.
fn infer_provider_kind(pref: &RefSpec::ProviderRef, base_dir: &Path) -> ProviderKind {
    use super::super::RefSpec::Source;
    match pref.provider {
        Source::Path => {
            let (target, _) = RefSpec::split_channel_ref(&pref.target);
            let target = Path::new(target);
            let dir = if target.is_absolute() {
                target.to_path_buf()
            } else {
                base_dir.join(target)
            };
            if dir.join(Syntax::PACKAGE_FILE).is_file()
                || dir.join(Syntax::PAYLOAD_FILE).is_file()
            {
                ProviderKind::Core
            } else {
                ProviderKind::Nix
            }
        }
        // `…@github` can't be classified offline-and-free; defer to a realize-time
        // `pkg.jet` peek (U9).
        Source::Github => ProviderKind::Infer,
        // `…@nixpkgs` is always the nix collection; never probed. (`Named` can't
        // appear in a `target@provider` ref.)
        _ => ProviderKind::Nix,
    }
}
