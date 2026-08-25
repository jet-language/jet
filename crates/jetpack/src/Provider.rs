//! Provider translation layer (D-JPK5).
//!
//! Jetpack owns the package lifecycle. Nix is a *compatibility provider*: we
//! translate a Jetpack ref into a flake ref, consume a pinned compatibility
//! result, and turn that into a `bin` directory for PATH. The native Jetpack
//! builder can sit beside this same `Realized` boundary.
//!
//! Determinism for tests: when a fixtures dir is supplied (the `--offline`
//! path, or `JETPACK_FIXTURES`), we read a pinned compatibility result.
//! Production locked nixpkgs refs resolve through the signed index and native
//! cache admission. A true local catalog miss may use the one-shot interactive
//! Nix compatibility fallback; the resulting realization remains Jetpack-owned.

use super::Package;
use super::Recipe::{self, BuildContext, BuildRecipe, BuildStep};
use super::RefSpec::{ProviderKind, RefSpec, Source, SourceTable};
use super::JSON;
use crate::NixIndex::{IndexKey, IndexTrustTier, NixIndexClient, NixIndexError};
use crate::Store::{
    admit_nix_closure_with_progress, current_progress, plan_nix_downloads, AdmittedNixClosure,
    NixOutputRequest, Roots, StoreError,
};
use crate::SHA256;
use crate::Syntax;
use crate::{ProviderFactValue, ProviderFacts};
use jet_env_model::ModuleEval::{AdapterPlan, AdapterRecipe};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

mod cran;
mod fetch;
mod luarocks;
pub(crate) mod native;
mod package;
mod remote;
mod script_registry;
mod adapter;
pub(crate) use adapter::{adapter_recipe_to_build, realize_adapter, stage_adapter_source};
mod core;
pub(crate) use core::{active_tmp_marker_is_live, CoreProvider};
pub use core::{sweep_build_scratch, ACTIVE_TMP_MARKER, BUILD_SCRATCH_DIR};
#[cfg(test)]
pub(crate) use core::build_rlib_from_cargo;
use cran::CranProvider;
use luarocks::LuaRocksProvider;
use native::NativeProvider;
use script_registry::{Kind as ScriptRegistryKind, ScriptRegistryProvider};

use package::{
    canonical_package_kind, canonical_source_dir, core_recipe_identity, core_tree_fingerprint,
    find_canonical_package, toolchain_facts, validate_core_source_tree,
};
#[cfg(test)]
use remote::file_has_top_level_run;
use remote::{
    copy_tree, fetch_remote_repo, infer_package_kind, parse_remote_source, source_cache_dir,
    source_repo, tree_fingerprint, RemoteSource,
};

/// A realized package: where its bytes are and what to put on PATH. `bin` is
/// the directory to prepend to PATH, or **empty** for a `library` package (U10),
/// which is staged for import and contributes nothing to PATH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Realized {
    pub name: String,
    /// Package version for the hangar id (`<name>-<version>-<fp>`, D-PM1), or
    /// empty when the provider can't determine one (Phase-1 nix refs often).
    pub version: String,
    pub reference: String,
    pub out: String,
    pub bin: String,
    /// Path to the built Rust rlib artifact (D-BFS1). Set when the core provider
    /// compiles a library package that carries a `Cargo.toml`. Empty otherwise.
    pub rlib: String,
    /// D-JPK-CACHE1=A: the A4 envelope for this realized output — output hash,
    /// platform, signature slot, provenance. Makes the object cache-substitutable.
    pub envelope: super::Envelope::Envelope,
    pub cache_identity: super::Store::CacheIdentity,
    /// D-JPK-CACHE1 reporting (T4): how this realization was satisfied.
    pub source_state: SourceState,
    /// Provider output names mapped to exact immutable paths.
    pub named_outputs: BTreeMap<String, String>,
    /// CAS closure edges discovered by provider.
    pub references: Vec<String>,
    /// Canonical replay/source/toolchain/policy facts.
    pub producer: super::Store::ProducerRecord,
}

/// Immutable Nix identity prepared from the resolved request and output bytes.
/// The Store receives the same facts through the realization record; no ref
/// spelling can stand in for an output digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedNixIdentity {
    pub normalized_source: String,
    pub normalized_node: String,
    pub normalized_alias: String,
    pub normalized_query: String,
    pub lock_digest: String,
    pub named_output_digests: BTreeMap<String, String>,
    pub envelope_digest: String,
    pub cache_identity: super::Store::CacheIdentity,
}

pub(super) fn producer_record(
    provider: &str,
    immutable_source: &str,
    source_digest: &str,
    plan_facts: BTreeMap<String, String>,
    toolchain_facts: &str,
    identity: &super::Store::CacheIdentity,
    facts: BTreeMap<String, String>,
) -> Result<super::Store::ProducerRecord, ProviderError> {
    let plan = crate::Comptime::Build::BuildPlanReplay::from_facts(plan_facts)
        .map_err(ProviderError::CoreBuild)?;
    super::Store::ProducerRecord::new(
        provider,
        immutable_source,
        source_digest,
        plan,
        toolchain_facts,
        format!(
            "policy={}\nplatform={}",
            identity.policy_fingerprint, identity.platform
        ),
        facts,
    )
    .map_err(ProviderError::CoreBuild)
}

const SHARED_PROVIDER_FACTS: &str = "provider-facts";
const SHARED_PROVIDER_FACTS_DIGEST: &str = "provider-facts-digest";
const NIX_NATIVE_FORMAT: &str = "nix.native.format";
const NIX_NATIVE_DOCUMENT: &str = "nix.native.document";

/// Refresh the one provider carrier after a producer record changes. The
/// native producer record is retained as an opaque document, while its scalar
/// facts are additive typed facts with one provenance source. The two carrier
/// fields are omitted from that native document to avoid recursive growth on
/// cache and lock refreshes.
pub(crate) fn refresh_provider_facts(
    producer: &mut super::Store::ProducerRecord,
    reference: &str,
) -> Result<(), ProviderError> {
    let mut native = producer.clone();
    native.facts.remove(SHARED_PROVIDER_FACTS);
    native.facts.remove(SHARED_PROVIDER_FACTS_DIGEST);
    let mut native_plan_facts = native.plan.facts().clone();
    native_plan_facts.remove(SHARED_PROVIDER_FACTS);
    native_plan_facts.remove(SHARED_PROVIDER_FACTS_DIGEST);
    native.plan = crate::Comptime::Build::BuildPlanReplay::from_facts(native_plan_facts)
        .map_err(ProviderError::BadOutput)?;

    let mut shared = ProviderFacts::for_reference(&producer.provider, reference);
    shared.set_resolved_source(&producer.immutable_source);
    let native_document = producer
        .facts
        .get(NIX_NATIVE_DOCUMENT)
        .filter(|document| !document.trim().is_empty());
    let native_format = producer
        .facts
        .get(NIX_NATIVE_FORMAT)
        .filter(|format| !format.trim().is_empty());
    match (producer.provider.as_str(), native_format, native_document) {
        ("nix", Some(format), Some(document)) => shared.set_native_document(format, document),
        _ => shared.set_native_document("jet-producer-record-v1", &native.encode()),
    }
    for (key, value) in &producer.facts {
        if matches!(
            key.as_str(),
            SHARED_PROVIDER_FACTS | SHARED_PROVIDER_FACTS_DIGEST
        ) {
            continue;
        }
        shared.add_fact(
            key,
            ProviderFactValue::Text(value.clone()),
            "jet-producer-record-v1",
        );
    }
    for (key, value) in [
        ("provider.source_digest", producer.source_digest.clone()),
        ("provider.toolchain_facts", producer.toolchain_facts.clone()),
        ("provider.policy_facts", producer.policy_facts.clone()),
    ] {
        shared.add_fact(
            key,
            ProviderFactValue::Text(value),
            "jet-producer-record-v1",
        );
    }
    shared.validate().map_err(ProviderError::BadOutput)?;
    let encoded = shared.to_json();
    let digest = shared.digest();
    producer
        .facts
        .insert(SHARED_PROVIDER_FACTS.to_string(), encoded);
    producer
        .facts
        .insert(SHARED_PROVIDER_FACTS_DIGEST.to_string(), digest);
    Ok(())
}

fn cache_identity(source: &str, recipe: &str, ctx: &Ctx) -> super::Store::CacheIdentity {
    super::Store::CacheIdentity {
        source_fingerprint: source.to_string(),
        recipe_fingerprint: SHA256::sha256_hex(recipe.as_bytes()),
        policy_fingerprint: super::RuntimePolicy::cache_policy_fingerprint(ctx.offline),
        platform: super::Envelope::host_platform(),
    }
}

/// One adapter action identity for both cache expectation and realization.
/// Executable hooks use the exact approval subject; copy/prebuilt adapters keep
/// their non-executable identity and never enter the hook trust path.
pub(crate) fn adapter_action_identity(
    plan: &AdapterPlan,
    recipe: &BuildRecipe,
    source_digest: &str,
    platform: &str,
    table: &SourceTable,
) -> String {
    if matches!(&plan.recipe, AdapterRecipe::Build(_)) {
        let identity = recipe.build_identity_for_source_with_dependencies(
            &plan.name,
            &plan.source,
            source_digest,
            platform,
            &adapter_identity_inputs(plan, table),
        );
        // The cache identity records the required policy, not this machine's
        // current capability. A missing local backend must still be able to
        // select a trusted substitute produced under a native backend; the
        // actual backend is recorded separately in the producer receipt.
        format!("{identity}\nsandbox=native-required")
    } else {
        recipe.build_identity(&plan.name, source_digest, platform)
    }
}

/// Canonicalize the exact package refs used to provide build tools. The
/// dependency list is an authority input, not merely a realization hint:
/// changing a provider/source must change the hook subject before any tool is
/// realized.
pub(crate) fn adapter_dependency_refs(plan: &AdapterPlan) -> Vec<String> {
    let mut dependencies = plan
        .deps
        .iter()
        .map(jet_env_model::ModuleEval::pkg_ref)
        .collect::<Vec<_>>();
    dependencies.sort();
    dependencies
}

/// Add the exact declared source authorities to the hook subject. A named
/// source can keep the same spelling while its pinned upstream or provider
/// changes; that is a trust change even when the recipe and dependency refs do
/// not move.
pub(crate) fn adapter_identity_inputs(plan: &AdapterPlan, table: &SourceTable) -> Vec<String> {
    let mut inputs = adapter_dependency_refs(plan);
    inputs.extend(
        table
            .trust_lines()
            .into_iter()
            .map(|line| format!("authority:{line}")),
    );
    inputs.sort();
    inputs
}

pub(crate) fn adapter_cache_identity(
    source_digest: &str,
    action_identity: &str,
    ctx: &Ctx,
) -> super::Store::CacheIdentity {
    cache_identity(source_digest, &format!("adapter-v1:{action_identity}"), ctx)
}

fn provider_cache_identity(
    source: &str,
    recipe: &str,
    ctx: &Ctx,
    authority: &str,
) -> super::Store::CacheIdentity {
    let policy = format!(
        "{}\n{authority}",
        super::RuntimePolicy::cache_policy_fingerprint(ctx.offline)
    );
    super::Store::CacheIdentity {
        source_fingerprint: source.to_string(),
        recipe_fingerprint: SHA256::sha256_hex(recipe.as_bytes()),
        policy_fingerprint: SHA256::sha256_hex(policy.as_bytes()),
        platform: super::Envelope::host_platform(),
    }
}

fn nix_cache_identity(
    source: &str,
    platform: &str,
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
) -> super::Store::CacheIdentity {
    let (normalized_source, normalized_node, normalized_alias, normalized_query) =
        nix_identity_parts(spec, table);
    let authority = format!(
        "source={normalized_source}\nnode={normalized_node}\nalias={normalized_alias}\nquery={normalized_query}\nbuild-facts={}"
        , nix_build_facts_digest()
    );
    let mut identity = provider_cache_identity(source, NIX_RECIPE_ID, ctx, &authority);
    identity.platform = platform.to_string();
    identity
}

pub fn validate_cache_authority(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
) -> Result<(), ProviderError> {
    let Some(project) = ctx.project_dir else {
        return Ok(());
    };
    match resolve_kind(spec, table, ctx.offline, ctx.store_dir) {
        ProviderKind::Cran => {
            let Some((_, _, repository, locked, _)) =
                super::Lock::cran_realization(project, &spec.raw)
            else {
                return Ok(());
            };
            let current = cran::cache_authority(ctx)?;
            ensure_locked_authority("CRAN", &repository, &locked, &current)
        }
        ProviderKind::LuaRocks => {
            let Some((_, _, repository, locked, _)) =
                super::Lock::luarocks_realization(project, &spec.raw)
            else {
                return Ok(());
            };
            let current = luarocks::cache_authority(ctx)?;
            ensure_locked_authority("LuaRocks", &repository, &locked, &current)
        }
        ProviderKind::RubyGems | ProviderKind::Cpan | ProviderKind::Packagist => {
            let kind = script_registry_kind(resolve_kind(spec, table, ctx.offline, ctx.store_dir))
                .ok_or_else(|| {
                    ProviderError::Registry("script registry", "unknown provider".into())
                })?;
            let Some((_, _, repository, locked, _)) =
                super::Lock::registry_realization(project, kind.label(), &spec.raw)
            else {
                return Ok(());
            };
            let current = script_registry::cache_authority(kind, ctx)?;
            ensure_locked_authority(kind.title(), &repository, &locked, &current)
        }
        _ => Ok(()),
    }
}

fn ensure_locked_authority(
    provider: &'static str,
    repository: &str,
    locked: &str,
    current: &fetch::Authority,
) -> Result<(), ProviderError> {
    if repository != current.registry() || locked != current.provenance() {
        Err(ProviderError::Registry(
            provider,
            format!(
                "locked provider authority does not match current authority.providers (locked repository `{repository}`, current `{}`)",
                current.registry()
            ),
        ))
    } else {
        Ok(())
    }
}

/// Independently derive every fact required to trust an existing cache record.
/// A provider that cannot derive exact current source/recipe identity gets no
/// early cache path.
pub fn cache_expectation(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
) -> Option<super::Store::CacheExpectation> {
    match resolve_kind(spec, table, ctx.offline, ctx.store_dir) {
        ProviderKind::Core => {
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
            let toolchain = super::Toolchain::Toolchain::resolve_for_core(ctx.offline);
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
            Some(super::Store::CacheExpectation {
                identity: cache_identity(&source_fingerprint, &recipe, ctx),
                owned_output: Some(ctx.store_dir.join(format!(
                    "{}-{}",
                    spec.package,
                    &source_fingerprint[..12]
                ))),
                allow_unsigned_local: true,
            })
        }
        // D-JPK-OFFLINE2=B: a Nix ref may reuse a hangar copy offline, but only
        // against a locked identity recorded in the project `.jet/lock` — a plain
        // file read, zero live Nix/network. The identity reproduces exactly what
        // the realize path recorded (content-hash source anchor + constant
        // recipe/policy + host platform); `verify_cache_entry` then re-hashes the
        // on-disk closure through the same proof gate core refs use. No lock / no
        // entry → None, so offline keeps failing loudly (E1276), never serving a
        // spelling-trusted stale copy (card #418).
        ProviderKind::Nix => {
            let project = ctx.project_dir?;
            let (output, env) = super::Lock::nix_realization(project, &spec.raw)?;
            if env.output_hash.is_empty() {
                return None;
            }
            let platform = if env.platform.is_empty() {
                super::Envelope::host_platform()
            } else {
                env.platform.clone()
            };
            Some(super::Store::CacheExpectation {
                identity: nix_cache_identity(&env.output_hash, &platform, spec, table, ctx),
                owned_output: Some(PathBuf::from(output)),
                allow_unsigned_local: true,
            })
        }
        ProviderKind::Cran => {
            let project = ctx.project_dir?;
            let (output, source_hash, _repository, _locked_authority, env) =
                super::Lock::cran_realization(project, &spec.raw)?;
            let authority = cran::cache_authority(ctx).ok()?;
            Some(super::Store::CacheExpectation {
                identity: super::Store::CacheIdentity {
                    platform: if env.platform.is_empty() {
                        super::Envelope::host_platform()
                    } else {
                        env.platform.clone()
                    },
                    ..provider_cache_identity(
                        &source_hash,
                        cran::RECIPE_ID,
                        ctx,
                        &authority.provenance(),
                    )
                },
                owned_output: Some(PathBuf::from(output)),
                allow_unsigned_local: true,
            })
        }
        ProviderKind::LuaRocks => {
            let project = ctx.project_dir?;
            let (output, source_hash, _repository, _locked_authority, env) =
                super::Lock::luarocks_realization(project, &spec.raw)?;
            let authority = luarocks::cache_authority(ctx).ok()?;
            Some(super::Store::CacheExpectation {
                identity: super::Store::CacheIdentity {
                    platform: if env.platform.is_empty() {
                        super::Envelope::host_platform()
                    } else {
                        env.platform.clone()
                    },
                    ..provider_cache_identity(
                        &source_hash,
                        luarocks::RECIPE_ID,
                        ctx,
                        &authority.provenance(),
                    )
                },
                owned_output: Some(PathBuf::from(output)),
                allow_unsigned_local: true,
            })
        }
        ProviderKind::RubyGems | ProviderKind::Cpan | ProviderKind::Packagist => {
            let project = ctx.project_dir?;
            let kind = script_registry_kind(resolve_kind(spec, table, ctx.offline, ctx.store_dir))?;
            let (output, source_hash, _repository, _locked_authority, env) =
                super::Lock::registry_realization(project, kind.label(), &spec.raw)?;
            let authority = script_registry::cache_authority(kind, ctx).ok()?;
            Some(super::Store::CacheExpectation {
                identity: super::Store::CacheIdentity {
                    platform: if env.platform.is_empty() {
                        super::Envelope::host_platform()
                    } else {
                        env.platform.clone()
                    },
                    ..provider_cache_identity(
                        &source_hash,
                        kind.recipe(),
                        ctx,
                        &authority.provenance(),
                    )
                },
                owned_output: Some(PathBuf::from(output)),
                allow_unsigned_local: true,
            })
        }
        ProviderKind::JetPackage => native::cache_expectation(spec, table, ctx),
        ProviderKind::JetRegistry
        | ProviderKind::Npm
        | ProviderKind::Cargo
        | ProviderKind::PyPI
        | ProviderKind::SwiftPM => None,
        // An inferred source realized offline defaults to nix with no lock-backed
        // identity to match; no early cache path.
        ProviderKind::Infer => None,
    }
}

/// Return the exact external approval subject for a Core Cargo action without
/// executing package code. The CLI gates this subject before the Store reaches
/// the provider; project metadata can therefore describe a build but cannot
/// authorize its Cargo/build-script execution.
pub(crate) fn core_build_identity(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
) -> Result<Option<String>, String> {
    if resolve_kind(spec, table, ctx.offline, ctx.store_dir) != ProviderKind::Core {
        return Ok(None);
    }
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
    let toolchain = super::Toolchain::Toolchain::resolve_for_core(ctx.offline);
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
    let platform = super::Envelope::host_platform();
    let authority = format!(
        "jet-core-build-hook.v1\npackage={}\nprovider={}\nsource={}\nsource_digest={}\nplatform={}\nrecipe={}\ncapabilities=exec:cargo\n",
        spec.package, upstream, spec.raw, source_digest, platform, recipe
    );
    Ok(Some(format!(
        "build-sha256:{}",
        SHA256::sha256_hex(authority.as_bytes())
    )))
}

/// Derive the adapter cache identity without trusting an existing output.
/// Staging reads the declared source; the output path follows only from those
/// bytes plus the normalized recipe.
pub fn adapter_cache_expectation(
    plan: &AdapterPlan,
    table: &SourceTable,
    ctx: &Ctx,
) -> Result<super::Store::CacheExpectation, ProviderError> {
    let source_ref = super::RefSpec::classify_provider_ref(&plan.source).map_err(|_| {
        ProviderError::Adapter(format!(
            "adapter source `{}` is not a provider ref",
            plan.source
        ))
    })?;
    let staged = stage_adapter_source(&source_ref, ctx)?;
    let recipe = adapter_recipe_to_build(&plan.recipe);
    let source_hash = tree_fingerprint(&staged);
    let source_fingerprint = super::Envelope::try_output_hash_of(&staged.to_string_lossy())
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
        &super::Envelope::host_platform(),
        table,
    );
    let id_input = format!(
        "u20-adapter-v1\nname={}\nsource={}\nsource_hash={}\nidentity={}\n",
        plan.name, plan.source, source_hash, build_identity
    );
    let fp = SHA256::sha256_hex(id_input.as_bytes());
    Ok(super::Store::CacheExpectation {
        identity: adapter_cache_identity(&source_fingerprint, &build_identity, ctx),
        owned_output: Some(
            ctx.store_dir
                .join(format!("{}-adapter-{}", plan.name, &fp[..12])),
        ),
        allow_unsigned_local: true,
    })
}

fn normalize_nix_identity(value: &str) -> String {
    value.trim().replace('\\', "/")
}

pub(crate) fn nix_identity_parts(
    spec: &RefSpec,
    table: &SourceTable,
) -> (String, String, String, String) {
    let source = match &spec.source {
        Source::Jetpack | Source::Nixpkgs => Syntax::REF_SOURCE_JETPACK,
        Source::Named(name) => table.upstream(name).unwrap_or(name),
        _ => spec.source.label(),
    };
    let alias = match &spec.source {
        Source::Jetpack | Source::Nixpkgs => Syntax::REF_SOURCE_JETPACK,
        _ => spec.source.label(),
    };
    (
        normalize_nix_identity(source),
        normalize_nix_identity(&flake_ref(spec, table)),
        normalize_nix_identity(alias),
        normalize_nix_identity(&spec.package),
    )
}

pub(crate) fn project_lock_digest(project: Option<&Path>) -> Result<String, ProviderError> {
    let Some(project) = project.filter(|path| path.is_dir()) else {
        return Ok(String::new());
    };
    let path = super::Store::lock_path(project);
    match std::fs::read(&path) {
        Ok(raw) => {
            // Hangar receipts are a post-realization projection of this lock,
            // not a Nix input. Exclude them from the producer digest so adding
            // the receipt cannot invalidate the same cached realization on its
            // next use.
            let raw = String::from_utf8(raw).map_err(|error| {
                ProviderError::BadOutput(format!(
                    "project lock `{}` is not valid UTF-8: {error}",
                    path.display()
                ))
            })?;
            let mut lock = super::Lock::parse(&raw).map_err(|error| {
                ProviderError::BadOutput(format!(
                    "could not parse project lock `{}`: {error}",
                    path.display()
                ))
            })?;
            for package in &mut lock.packages {
                package.receipt = None;
            }
            Ok(SHA256::sha256_hex(super::Lock::write(&lock).as_bytes()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(ProviderError::BadOutput(format!(
            "could not read project lock `{}`: {error}",
            path.display()
        ))),
    }
}

fn validate_project_lock_layout(project: &Path) -> Result<(), ProviderError> {
    let managed = super::Store::managed_dir(project);
    match std::fs::symlink_metadata(&managed) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(ProviderError::BadOutput(format!(
                "project managed directory `{}` is not a real directory",
                managed.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(ProviderError::BadOutput(format!(
                "could not inspect project managed directory `{}`: {error}",
                managed.display()
            )));
        }
    }
    let lock = super::Store::lock_path(project);
    match std::fs::symlink_metadata(&lock) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(ProviderError::BadOutput(format!(
                "project lock `{}` is not a real file",
                lock.display()
            )))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ProviderError::BadOutput(format!(
            "could not inspect project lock `{}`: {error}",
            lock.display()
        ))),
    }
}

pub(crate) fn envelope_digest(envelope: &super::Envelope::Envelope) -> String {
    SHA256::sha256_hex(
        format!(
            "jet-nix-envelope-v1\noutput-hash={}\nplatform={}\nsignature={}\nprovenance={}\n",
            envelope.output_hash, envelope.platform, envelope.signature, envelope.provenance
        )
        .as_bytes(),
    )
}

fn primary_nix_output_digest(named_output_digests: &BTreeMap<String, String>) -> Option<String> {
    named_output_digests
        .get("out")
        .or_else(|| named_output_digests.get("bin"))
        .cloned()
}

/// Hash every Nix output from its current bytes and prepare the identity that
/// must survive Store registration. Existing project locks pin the primary
/// output digest; a missing lock is left for the first successful realization.
pub(crate) fn prepare_nix_identity(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
    realized: &Realized,
) -> Result<PreparedNixIdentity, ProviderError> {
    let mut named_output_digests = BTreeMap::new();
    for (name, path) in &realized.named_outputs {
        if name.trim().is_empty() || path.trim().is_empty() {
            return Err(ProviderError::BadOutput(format!(
                "Nix provider returned an empty named output `{name}`"
            )));
        }
        let digest = super::Envelope::try_output_hash_of(path).map_err(|reason| {
            ProviderError::Ingest(format!(
                "Nix output `{name}` at `{path}` could not be hashed from its bytes: {reason}"
            ))
        })?;
        if digest.trim().is_empty() {
            return Err(ProviderError::BadOutput(format!(
                "Nix output `{name}` at `{path}` has an empty byte digest"
            )));
        }
        named_output_digests.insert(name.clone(), digest);
    }
    let output_hash = primary_nix_output_digest(&named_output_digests).ok_or_else(|| {
        ProviderError::BadOutput("Nix provider returned no non-empty `out` or `bin` output".into())
    })?;
    if let Some(project) = ctx.project_dir.filter(|path| path.is_dir()) {
        if let Some((_, locked)) = super::Lock::nix_realization(project, &spec.raw) {
            if locked.output_hash != output_hash {
                return Err(ProviderError::Ingest(format!(
                    "Nix output digest mismatch for `{}`: lock has `{}`, realized bytes have `{output_hash}`",
                    spec.raw, locked.output_hash
                )));
            }
        }
    }
    let (normalized_source, normalized_node, normalized_alias, normalized_query) =
        nix_identity_parts(spec, table);
    let envelope = super::Envelope::Envelope {
        output_hash,
        platform: super::Envelope::host_platform(),
        signature: String::new(),
        provenance: format!("{} via nix", spec.raw),
    };
    let cache_identity =
        nix_cache_identity(&envelope.output_hash, &envelope.platform, spec, table, ctx);
    Ok(PreparedNixIdentity {
        normalized_source,
        normalized_node,
        normalized_alias,
        normalized_query,
        lock_digest: project_lock_digest(ctx.project_dir)?,
        named_output_digests,
        envelope_digest: envelope_digest(&envelope),
        cache_identity,
    })
}

fn prepared_nix_facts(identity: &PreparedNixIdentity) -> BTreeMap<String, String> {
    let mut facts = BTreeMap::from([
        (
            "nix.identity.source".into(),
            identity.normalized_source.clone(),
        ),
        ("nix.identity.node".into(), identity.normalized_node.clone()),
        (
            "nix.identity.alias".into(),
            identity.normalized_alias.clone(),
        ),
        (
            "nix.identity.query".into(),
            identity.normalized_query.clone(),
        ),
        ("nix.lock.digest".into(), identity.lock_digest.clone()),
        (
            "nix.envelope.digest".into(),
            identity.envelope_digest.clone(),
        ),
        (
            "nix.cache.source_fingerprint".into(),
            identity.cache_identity.source_fingerprint.clone(),
        ),
        (
            "nix.cache.recipe_fingerprint".into(),
            identity.cache_identity.recipe_fingerprint.clone(),
        ),
        (
            "nix.cache.policy_fingerprint".into(),
            identity.cache_identity.policy_fingerprint.clone(),
        ),
        (
            "nix.cache.platform".into(),
            identity.cache_identity.platform.clone(),
        ),
    ]);
    for (key, value) in nix_build_facts_record() {
        facts.insert(key, value);
    }
    for (name, digest) in &identity.named_output_digests {
        facts.insert(format!("nix.output.{name}.digest"), digest.clone());
    }
    facts
}

/// Canonical facts for the Nix 2.34 build boundary. These are data, not a
/// second evaluator: the provider records them so cache identity and runtime
/// projection can prove which compatibility contract was used.
pub(crate) fn nix_build_facts() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("nix.build.root".into(), "/build".into()),
        ("nix.build.home".into(), "/homeless-shelter".into()),
        ("nix.build.store".into(), "/nix/store".into()),
        ("nix.build.uid".into(), "unprivileged".into()),
        ("nix.build.time".into(), "deterministic".into()),
        ("nix.build.locale".into(), "C".into()),
    ])
}

fn nix_build_environment_facts() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".into(), "/homeless-shelter".into()),
        ("NIX_BUILD_TOP".into(), "/build".into()),
        ("TMPDIR".into(), "/build".into()),
        ("TEMPDIR".into(), "/build".into()),
        ("TMP".into(), "/build".into()),
        ("TEMP".into(), "/build".into()),
        ("NIX_STORE".into(), "/nix/store".into()),
        ("LANG".into(), "C".into()),
        ("LC_ALL".into(), "C".into()),
        ("TZ".into(), "UTC".into()),
    ])
}

fn nix_build_facts_digest() -> String {
    let mut canonical = String::new();
    for (key, value) in nix_build_facts().into_iter().chain(
        nix_build_environment_facts()
            .into_iter()
            .map(|(key, value)| (format!("nix.build.env.{key}"), value)),
    ) {
        canonical.push_str(&key);
        canonical.push('=');
        canonical.push_str(&value);
        canonical.push('\n');
    }
    SHA256::sha256_hex(canonical.as_bytes())
}

pub(crate) fn nix_build_facts_record() -> BTreeMap<String, String> {
    let mut facts = BTreeMap::from([("nix.build.facts.digest".into(), nix_build_facts_digest())]);
    for (key, value) in nix_build_facts() {
        facts.insert(key, value);
    }
    for (key, value) in nix_build_environment_facts() {
        facts.insert(format!("nix.build.env.{key}"), value);
    }
    facts
}

pub(crate) fn validate_nix_build_facts(
    producer: &super::Store::ProducerRecord,
) -> std::io::Result<()> {
    if producer.provider != "nix" {
        return Ok(());
    }
    for (key, expected) in nix_build_facts_record() {
        if producer.facts.get(&key) != Some(&expected) {
            return Err(std::io::Error::other(format!(
                "Nix producer fact `{key}` is missing or changed"
            )));
        }
    }
    Ok(())
}

/// Return only the fixed, audited environment projection recorded by a Nix
/// producer. PATH stays under Jetpack's package composition law.
pub(crate) fn nix_runtime_environment(
    producer: &super::Store::ProducerRecord,
) -> BTreeMap<String, String> {
    if validate_nix_build_facts(producer).is_err() {
        return BTreeMap::new();
    }
    nix_build_environment_facts()
}

pub(crate) fn validate_nix_lock_before_store(
    ctx: &Ctx,
    realized: &Realized,
) -> Result<(), ProviderError> {
    if realized.producer.provider != "nix" {
        return Ok(());
    }
    let Some(expected) = realized.producer.facts.get("nix.lock.digest") else {
        return Err(ProviderError::BadOutput(
            "Nix realization is missing its prepared lock digest".into(),
        ));
    };
    if let Some(project) = ctx.project_dir.filter(|path| path.is_dir()) {
        validate_project_lock_layout(project)?;
    }
    let current = project_lock_digest(ctx.project_dir)?;
    if &current != expected {
        return Err(ProviderError::BadOutput(format!(
            "Nix project lock changed before Store registration: prepared `{expected}`, current `{current}`"
        )));
    }
    Ok(())
}

/// Publish Nix lock state only after Store returned a registered entry.
/// Missing or non-directory project roots intentionally do nothing.
pub(crate) fn record_nix_lock_after_store(
    ctx: &Ctx,
    roots: &super::Store::Roots,
    entry: &super::Store::StoreEntry,
) -> Result<super::Store::StoreEntry, ProviderError> {
    let Some(project) = ctx.project_dir.filter(|path| path.is_dir()) else {
        return Ok(entry.clone());
    };
    if entry.reference.is_empty()
        || entry.envelope.output_hash.is_empty()
        || entry.cache_identity.source_fingerprint != entry.envelope.output_hash
        || entry.cache_identity.recipe_fingerprint != SHA256::sha256_hex(NIX_RECIPE_ID.as_bytes())
    {
        return Ok(entry.clone());
    }
    let Ok(producer) = super::Store::ProducerRecord::decode(&entry.producer_record) else {
        return Ok(entry.clone());
    };
    if producer.provider != "nix" {
        return Ok(entry.clone());
    }
    let Some(expected_lock_digest) = producer.facts.get("nix.lock.digest") else {
        return Err(ProviderError::BadOutput(
            "Nix Store entry is missing its prepared lock digest".into(),
        ));
    };
    validate_project_lock_layout(project)?;
    let current_lock_digest = project_lock_digest(Some(project))?;
    if &current_lock_digest != expected_lock_digest {
        return Err(ProviderError::BadOutput(format!(
            "Nix project lock changed after Store registration: prepared `{expected_lock_digest}`, current `{current_lock_digest}`"
        )));
    }
    let expected_lock_digest = expected_lock_digest.to_string();
    let catalog_tier = producer
        .facts
        .get("nix.index.tier")
        .cloned()
        .unwrap_or_default();
    let catalog_trust = producer
        .facts
        .get("nix.index.trust")
        .cloned()
        .unwrap_or_default();
    let refreshed = super::RuntimePolicy::with_project_lock(
        project,
        "nix-lock-publication",
        || {
            let current_lock_digest = project_lock_digest(Some(project))
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
            if current_lock_digest != expected_lock_digest {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Nix project lock changed during Store publication: prepared `{expected_lock_digest}`, current `{current_lock_digest}`"
                    ),
                ));
            }
            super::Lock::record_nix_realization(
                project,
                &entry.name,
                &entry.version,
                &entry.reference,
                &entry.out,
                super::Lock::LockEnvelope {
                    output_hash: entry.envelope.output_hash.clone(),
                    platform: entry.envelope.platform.clone(),
                    signature: entry.envelope.signature.clone(),
                    provenance: entry.envelope.provenance.clone(),
                    catalog_tier: catalog_tier.clone(),
                    catalog_trust: catalog_trust.clone(),
                },
            )
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
            let lock_digest = project_lock_digest(Some(project))
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
            super::Store::refresh_nix_lock_digest(roots, entry, &lock_digest)
                .map_err(|error| {
                    std::io::Error::other(format!(
                        "could not refresh the Nix Store producer after lock publication: {error}"
                    ))
                })
        },
    )
    .map_err(|error| ProviderError::BadOutput(error.to_string()))?;
    Ok(refreshed)
}

/// How a dependency was realized, for the `jet build` per-package report
/// (`built | substituted | cached`, mirroring the D-JPK-CACHE1 example output).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceState {
    /// Compiled from source by the first-party core provider this run.
    Built,
    /// Downloaded from a verified native release artifact.
    Downloaded,
    /// Reused an already-realized, content-addressed object (no rebuild).
    Cached,
    /// Realized through the Nix compatibility provider (substituted, not built
    /// from source by Jetpack).
    Substituted,
}

impl SourceState {
    pub fn label(self) -> &'static str {
        match self {
            SourceState::Built => "built",
            SourceState::Downloaded => "downloaded",
            SourceState::Cached => "cached",
            SourceState::Substituted => "substituted",
        }
    }
}

/// What can go wrong realizing a ref through a provider. Each maps to a
/// friendly diagnostic (see `report`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// A compatibility provider failed; carries a trimmed reason.
    BuildFailed(String),
    /// The provider's JSON didn't have the shape we expected.
    BadOutput(String),
    /// Provider output could not pass the byte-level ingest boundary.
    Ingest(String),
    /// Offline/fixture mode but no fixture file for this ref.
    FixtureMissing(PathBuf),
    /// The selected provider can't realize this ref yet.
    Unsupported(String),
    /// E1256: a bounded foreign projection could not translate the source
    /// into Jet facts. This is distinct from a provider that simply has no
    /// realization implementation.
    ForeignProjection(String),
    /// The first-party `core` builder could not realize the package.
    CoreBuild(String),
    /// Native CRAN metadata, integrity, dependency, or R installation failure.
    Cran(String),
    /// Native LuaRocks metadata, integrity, dependency, or installation failure.
    LuaRocks(String),
    /// Native RubyGems/CPAN/Packagist metadata, integrity, or install failure.
    Registry(&'static str, String),
    /// E1232 (D-MONOREF1): a monorepo source could not be fetched — the sparse
    /// subtree checkout and the full-clone fallback both failed.
    MonorepoFetch(String),
    /// E1233 (D-MONOREF1): an in-repo transitive dependency names a package that
    /// is not a member of the source repo's workspace index.
    MemberOutsideWorkspace(String),
    /// E1270: an adapter source/recipe cannot be realized by the native adapter
    /// path.
    Adapter(String),
    /// E1273: a recipe-backed package failed while running a logged build step.
    BuildDebug(String),
    /// E1275: a local executable action has no enforceable native sandbox.
    SandboxUnavailable(String),
    /// E1271: a source channel cannot be resolved or is unlocked in a context
    /// that may not resolve it.
    Channel(String),
    /// E1276: `--offline` forbids a network fetch or metadata refresh.
    Offline(String),
    /// E1348/E1349/E1276: signed nixpkgs index resolution failed.
    NixIndex(NixIndexError),
    /// E1350: standard Nix binary-cache closure admission failed.
    NixCache(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NixBridgeNeed {
    pub reference: String,
    pub package: String,
}

impl ProviderError {
    /// The registered diagnostic code, for the errors that carry one.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            ProviderError::MonorepoFetch(_) => Some("E1232"),
            ProviderError::MemberOutsideWorkspace(_) => Some("E1233"),
            ProviderError::Adapter(_) => Some("E1270"),
            ProviderError::ForeignProjection(_) => Some("E1256"),
            ProviderError::Channel(_) => Some("E1271"),
            ProviderError::BuildDebug(_) => Some("E1273"),
            ProviderError::SandboxUnavailable(_) => Some("E1275"),
            ProviderError::Offline(_) => Some("E1276"),
            ProviderError::Cran(_) => None,
            ProviderError::LuaRocks(_) => None,
            ProviderError::Registry(_, _) => None,
            ProviderError::Ingest(_) => Some("E1315"),
            ProviderError::NixIndex(error) => match error {
                NixIndexError::NotIndexed { .. } => Some("E1349"),
                NixIndexError::Offline(_) => Some("E1276"),
                NixIndexError::Invalid(_) | NixIndexError::Transport(_) => Some("E1348"),
            },
            ProviderError::NixCache(_) => Some("E1350"),
            _ => None,
        }
    }
}

/// What a provider needs to realize a ref, beyond the ref and source table:
/// the offline fixtures dir (nix) and the Jetpack store dir to materialize into
/// (core). Bundled so the `Provider` trait stays stable as providers grow.
pub struct Ctx<'a> {
    pub fixtures: Option<&'a Path>,
    pub store_dir: &'a Path,
    pub offline: bool,
    /// D-JPK-OFFLINE2=B: the project root whose `.jet/lock` records (on realize)
    /// and matches (on offline reuse) a Nix-provider package's locked identity.
    /// `None` for callers with no project context (JetOS realize, tests) — a Nix
    /// realize then records no lock and gets no offline reuse.
    pub project_dir: Option<&'a Path>,
    /// Signed nixpkgs index client. `None` keeps non-Nix and fixture callers
    /// independent from the production index transport.
    pub nix_index: Option<&'a NixIndexClient<'a>>,
    /// Roots used by the native cache admission seam. This is separate from
    /// `store_dir` because reproducibility probes use private Hangars.
    pub nix_roots: Option<&'a Roots>,
}

/// D-JPK-OFFLINE2=B: the stable recipe id for a Nix-provider realization. Hashed
/// into the cache identity's `recipe_fingerprint`, recomputed offline from this
/// constant so a lock-backed reuse reproduces it without any Nix/network call.
pub(crate) const NIX_RECIPE_ID: &str = "nix-compat-v1";

/// Translate a Jetpack ref into the backend reference. Nix sources use `#` as
/// the flake selector; direct ecosystem roots retain their package selector
/// facts in the provider reference. A named source (D-JPK17) resolves through
/// `table` to its upstream/pin, then selects the package as a flake attr:
/// `<upstream>#<package>`.
pub fn flake_ref(spec: &RefSpec, table: &SourceTable) -> String {
    match &spec.source {
        Source::Jetpack | Source::Nixpkgs => {
            format!("nixpkgs#{}", nix_package_name(&spec.package))
        }
        Source::Github => format!("github:{}", spec.package),
        Source::Path => format!("path:{}", spec.package),
        Source::Cran => format!("cran:{}", spec.package),
        Source::LuaRocks => format!("luarocks:{}", spec.package),
        Source::RubyGems => format!("ruby:{}", spec.package),
        Source::Cpan => format!("perl:{}", spec.package),
        Source::Packagist => format!("php:{}", spec.package),
        Source::JetRegistry => format!("jet-registry:{}", spec.package),
        Source::Npm => format!("npm:{}", spec.package),
        Source::Cargo => format!("cargo:{}", spec.package),
        Source::PyPI => format!("pypi:{}", spec.package),
        Source::SwiftPM => format!("swiftpm:{}", spec.package),
        Source::Releases => format!("releases:{}", spec.package),
        Source::Named(name) => {
            let upstream = table.upstream(name).unwrap_or(name);
            let package = if table.provider(name) == ProviderKind::Nix {
                nix_package_name(&spec.package)
            } else {
                &spec.package
            };
            format!("{upstream}#{package}")
        }
    }
}

fn nix_package_name(package: &str) -> &str {
    package
        .split_once("#version=")
        .map_or(package, |(name, _)| name)
}

pub(crate) fn host_nix_system() -> Option<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => Some("x86_64-linux"),
        ("aarch64", "linux") => Some("aarch64-linux"),
        ("x86_64", "macos") => Some("x86_64-darwin"),
        ("aarch64", "macos") => Some("aarch64-darwin"),
        _ => None,
    }
}

/// The lock's `channel` field holds the *tier* a source tracks (`latest`,
/// `auto`), which `realize.rs` compares against the manifest. The signed index
/// is keyed by the nixpkgs channel name instead, and that lives in the declared
/// upstream (`NixOS/nixpkgs/nixos-unstable@github`). Read each from its own
/// source; they are different facts that happen to share a word.
fn locked_nix_index_key_for_project(
    spec: &RefSpec,
    table: &SourceTable,
    project_dir: Option<&Path>,
    host_system: &str,
) -> Result<IndexKey, ProviderError> {
    let source_name = match &spec.source {
        Source::Named(name) => name.as_str(),
        Source::Jetpack | Source::Nixpkgs => Syntax::REF_SOURCE_JETPACK,
        _ => {
            return Err(ProviderError::Unsupported(format!(
                "Nix index lookup does not support `{}` as a Nix source",
                spec.source.label()
            )))
        }
    };
    let project = project_dir.ok_or_else(|| {
        ProviderError::Channel(format!(
            "Nix source `{source_name}` has no project lock containing an exact channel pin"
        ))
    })?;
    let locked = [source_name, Syntax::REF_SOURCE_NIXPKGS]
        .into_iter()
        .find_map(|name| super::Lock::locked_source_channel(project, name))
        .ok_or_else(|| {
        ProviderError::Channel(format!(
            "Nix source `{source_name}` has no exact lock entry; run `jetpack update` first"
        ))
    })?;
    let prefix = "github:NixOS/nixpkgs#";
    let revision = locked.exact.strip_prefix(prefix).ok_or_else(|| {
        ProviderError::Unsupported(format!(
            "Nix source `{source_name}` uses unsupported exact input `{}`; the signed index accepts only github:NixOS/nixpkgs#<revision>",
            locked.exact
        ))
    })?;
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(ProviderError::Channel(format!(
            "Nix source `{source_name}` has malformed exact revision `{revision}`"
        )));
    }
    // `NixOS/nixpkgs/nixos-unstable@github#auto` -> `nixos-unstable`. Read the
    // original spelling, not the live upstream: applying the lock rewrites the
    // upstream to `github:NixOS/nixpkgs#<revision>`, which no longer names the
    // channel the index is keyed by.
    //
    // The implicit `@nixpkgs` source has no declaration to read, so it keeps
    // naming its channel in the lock entry.
    let channel = table
        .source_ref(source_name)
        .map(|declared| {
            declared
                .split('@')
                .next()
                .unwrap_or_default()
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .filter(|channel| !channel.is_empty())
        .unwrap_or_else(|| locked.channel.clone());
    if !matches!(channel.as_str(), "nixpkgs-unstable" | "nixos-unstable") {
        return Err(ProviderError::Unsupported(format!(
            "Nix channel `{channel}` is not covered by the signed nixpkgs index"
        )));
    }
    if host_system.is_empty() {
        return Err(ProviderError::Unsupported(
            "the host system is not supported by the signed nixpkgs index".into(),
        ));
    }
    Ok(IndexKey {
        channel,
        revision: revision.to_string(),
        system: host_system.to_string(),
        attrpath: vec![nix_package_name(&spec.package).to_string()],
    })
}

pub(crate) fn locked_nix_index_key(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
    host_system: &str,
) -> Result<IndexKey, ProviderError> {
    locked_nix_index_key_for_project(spec, table, ctx.project_dir, host_system)
}

fn has_locked_nix_index_key(
    spec: &RefSpec,
    table: &SourceTable,
    project_dir: Option<&Path>,
    host_system: Option<&str>,
) -> bool {
    host_system
        .and_then(|system| locked_nix_index_key_for_project(spec, table, project_dir, system).ok())
        .is_some()
}

/// The fixture filename for a ref, e.g. `nixpkgs-fastfetch.json`.
pub fn fixture_name(spec: &RefSpec) -> String {
    let pkg = spec.package.replace('/', "_");
    format!("{}-{}.json", spec.source.label(), pkg)
}

/// Resolve the fixtures dir from an explicit flag or `JETPACK_FIXTURES`.
pub fn fixtures_from_env(explicit: Option<PathBuf>) -> Option<PathBuf> {
    explicit.or_else(|| std::env::var_os("JETPACK_FIXTURES").map(PathBuf::from))
}

// ──────────────────────────────────────────────
// Provider boundary (R0; see docs/plans/epoch-5/unified-ecosystem.md).
//
// The first-party core resolver owns realization; providers are extensions
// behind one trait. `core` realizes first-party Jet packages (no Nix); `nix`
// leverages nixpkgs. Source classification is performed once at the provider
// boundary, so every caller shares the same dispatch and evidence.
// ──────────────────────────────────────────────

/// A backend that realizes a ref into bytes + a `bin` dir. Both the first-party
/// `core` provider and the `nix` compatibility provider implement this.
pub(crate) trait Provider {
    /// Realize `spec`. `table` resolves named sources; `ctx` carries the
    /// offline fixtures dir and the store dir to materialize into.
    fn realize(
        &self,
        spec: &RefSpec,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Realized, ProviderError>;
}

/// The Nix compatibility provider for package references that are not yet
/// representable by the native provider. The normal path admits an explicit
/// pinned result or signed-index/cache substitution; a true signed-catalog miss
/// may use one interactive local Nix evaluation before Jetpack takes ownership.
pub(crate) struct NixProvider;

struct UnsupportedProvider(&'static str);

impl Provider for UnsupportedProvider {
    fn realize(
        &self,
        spec: &RefSpec,
        _table: &SourceTable,
        _ctx: &Ctx,
    ) -> Result<Realized, ProviderError> {
        Err(ProviderError::Unsupported(format!(
            "provider `{}` has no realization path for `{}`; import and lock its native facts first",
            self.0, spec.raw
        )))
    }
}

impl Provider for NixProvider {
    fn realize(
        &self,
        spec: &RefSpec,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Realized, ProviderError> {
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
                    "native Nix package realization needs a locked signed-index record for `{}`; Jetpack does not invoke an installed Nix executable",
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

fn realize_from_local_nix(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
    key: &IndexKey,
) -> Result<Realized, ProviderError> {
    let project = ctx.project_dir.ok_or_else(|| {
        ProviderError::Unsupported(
            "local Nix fallback requires a project with an exact source lock".into(),
        )
    })?;
    let source_name = match &spec.source {
        Source::Named(name) => name.as_str(),
        Source::Jetpack | Source::Nixpkgs => crate::Syntax::REF_SOURCE_JETPACK,
        _ => {
            return Err(ProviderError::Unsupported(
                "local Nix fallback requires a Jetpack-backed Nix source".into(),
            ))
        }
    };
    let invocation = crate::NixFallbackPolicy::run(
        project,
        source_name,
        &key.revision,
        &key.system,
        &key.attrpath,
        ctx.offline,
    )
    .map_err(|error| ProviderError::BuildFailed(format!("local Nix fallback failed: {error}")))?;
    let native_document = invocation.stdout.clone();
    let mut realized = parse_realization(spec, &invocation.stdout)?;
    realized
        .producer
        .facts
        .insert(NIX_NATIVE_FORMAT.to_string(), "jet-local-nix-v1".to_string());
    realized
        .producer
        .facts
        .insert(NIX_NATIVE_DOCUMENT.to_string(), native_document);
    realized.producer.facts.extend(invocation.facts);
    finalize_nix_realization(spec, table, ctx, realized)
}

fn nix_cache_error(roots: &Roots, error: StoreError) -> ProviderError {
    let mut detail = format!(
        "{}: {}; why: {}; fix: {}",
        error.code(),
        error.what(),
        error.why(),
        error.fix()
    );
    if let Some(store_path) = missing_nix_store_path(roots) {
        detail.push_str(&format!("; missing Nix reference `{store_path}`"));
    }
    ProviderError::NixCache(detail)
}

fn missing_nix_store_path(roots: &Roots) -> Option<String> {
    crate::Store::list_checked(roots)
        .ok()?
        .into_iter()
        .filter_map(|entry| {
            let producer = crate::Store::ProducerRecord::decode(&entry.producer_record).ok()?;
            let store_path = producer.facts.get("nix.store-path")?.clone();
            let digest = crate::Envelope::try_output_hash_of_in_hangar(
                &entry.out,
                &roots.hangar_dir(),
                false,
            )
            .ok();
            (digest.as_deref() != Some(entry.envelope.output_hash.as_str())).then_some(store_path)
        })
        .next()
}

fn finalize_nix_realization(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
    mut realized: Realized,
) -> Result<Realized, ProviderError> {
    let identity = prepare_nix_identity(spec, table, ctx, &realized)?;
    realized.cache_identity = identity.cache_identity.clone();
    let previous = realized.producer;
    let prepared_facts = prepared_nix_facts(&identity);
    let mut facts = previous.facts;
    facts.extend(prepared_facts.clone());
    let mut plan_facts = previous.plan.facts().clone();
    plan_facts.extend(prepared_facts);
    let plan = crate::Comptime::Build::BuildPlanReplay::from_facts(plan_facts)
        .map_err(ProviderError::BadOutput)?;
    realized.producer = super::Store::ProducerRecord::new(
        previous.provider,
        previous.immutable_source,
        previous.source_digest,
        plan,
        previous.toolchain_facts,
        format!(
            "policy={}\nplatform={}",
            realized.cache_identity.policy_fingerprint, realized.cache_identity.platform
        ),
        facts,
    )
    .map_err(ProviderError::BadOutput)?;
    Ok(realized)
}

fn realization_from_index(
    spec: &RefSpec,
    key: &IndexKey,
    verified: crate::NixIndex::VerifiedIndexRecord,
    admitted: AdmittedNixClosure,
) -> Result<Realized, ProviderError> {
    if verified.record.attrpath != key.attrpath {
        return Err(ProviderError::BadOutput(
            "signed nixpkgs record attrpath disagrees with the requested key".into(),
        ));
    }
    let expected_names = verified.record.outputs.keys().collect::<BTreeSet<_>>();
    let catalog_policy = match verified.trust {
        IndexTrustTier::OfficialSigned => "trusted substitution (signed index + Nix cache)",
        IndexTrustTier::LocalUnofficial => {
            "local unofficial catalog (unverified name-to-store-path mapping) + signature-verified Nix cache"
        }
    };
    let admitted_names = admitted.outputs.keys().collect::<BTreeSet<_>>();
    if expected_names != admitted_names {
        return Err(ProviderError::BadOutput(
            "Nix cache admission returned a different named-output set than the signed index"
                .into(),
        ));
    }
    if admitted.objects.is_empty() {
        return Err(ProviderError::BadOutput(
            "Nix cache admission returned an empty closure".into(),
        ));
    }
    for (store_path, object) in &admitted.objects {
        if *store_path != object.store_path {
            return Err(ProviderError::BadOutput(format!(
                "Nix cache closure key `{store_path}` disagrees with its store path `{}`",
                object.store_path
            )));
        }
        if object.hangar_path.as_os_str().is_empty() {
            return Err(ProviderError::BadOutput(format!(
                "Nix cache closure object `{store_path}` has no Hangar path"
            )));
        }
        if object.hangar_digest.trim().is_empty() {
            return Err(ProviderError::BadOutput(format!(
                "Nix cache closure object `{store_path}` has no Hangar digest"
            )));
        }
        if object.upstream_proof_sha256.trim().is_empty() {
            return Err(ProviderError::BadOutput(format!(
                "Nix cache closure object `{store_path}` has no upstream proof"
            )));
        }
        if object
            .direct_reference_digests
            .iter()
            .any(|digest| digest.is_empty())
        {
            return Err(ProviderError::BadOutput(format!(
                "Nix cache closure object `{store_path}` has an empty reference digest"
            )));
        }
    }

    let mut named_outputs = BTreeMap::new();
    let mut facts = BTreeMap::from([
        ("nix.drv_path".into(), verified.record.drv_path.clone()),
        ("nix.reference".into(), spec.raw.clone()),
        ("build.sandbox".into(), "non-executing".into()),
        (
            "build.sandbox_policy".into(),
            catalog_policy.into(),
        ),
        (NIX_NATIVE_FORMAT.into(), "jet-nixpkgs-index-v1".into()),
        (NIX_NATIVE_DOCUMENT.into(), verified.record.canonical_json()),
        ("nix.index.tier".into(), verified.trust.label().into()),
        ("nix.index.trust".into(), verified.trust.trust().into()),
        (
            "nix.index.signature-chain".into(),
            verified.trust.signature_chain().into(),
        ),
        ("nix.index.proof.v1".into(), verified.proof.canonical_json()),
        (
            "nix.index.record.sha256".into(),
            verified.proof.record_sha256.clone(),
        ),
        (
            "nix.index.target.sha256".into(),
            verified.proof.index_sha256.clone(),
        ),
        (
            "nix.index.manifest.sha256".into(),
            verified.proof.manifest_sha256.clone(),
        ),
        (
            "nix.cache.closure.receipt.sha256".into(),
            admitted.closure_receipt_sha256.clone(),
        ),
    ]);
    let mut replay_facts = BTreeMap::from([
        ("nix.drv_path".into(), verified.record.drv_path.clone()),
        ("nix.reference".into(), spec.raw.clone()),
        ("build.sandbox".into(), "non-executing".into()),
        (
            "build.sandbox_policy".into(),
            catalog_policy.into(),
        ),
    ]);
    for (name, store_path) in &verified.record.outputs {
        let object = admitted.outputs.get(name).ok_or_else(|| {
            ProviderError::BadOutput(format!(
                "Nix cache admission omitted indexed output `{name}`"
            ))
        })?;
        let closure_object = admitted.objects.get(&object.store_path).ok_or_else(|| {
            ProviderError::BadOutput(format!(
                "Nix cache closure omitted indexed output `{name}` at `{}`",
                object.store_path
            ))
        })?;
        if closure_object.hangar_path != object.hangar_path
            || closure_object.hangar_digest != object.hangar_digest
            || closure_object.direct_reference_digests != object.direct_reference_digests
            || closure_object.upstream_proof_sha256 != object.upstream_proof_sha256
        {
            return Err(ProviderError::BadOutput(format!(
                "Nix cache output `{name}` disagrees with its closure object"
            )));
        }
        if object.store_path != *store_path {
            return Err(ProviderError::BadOutput(format!(
                "Nix cache output `{name}` disagrees with the signed index: `{}` vs `{store_path}`",
                object.store_path
            )));
        }
        let hangar_path = object.hangar_path.to_string_lossy().into_owned();
        if hangar_path.trim().is_empty() {
            return Err(ProviderError::BadOutput(format!(
                "Nix cache output `{name}` has no Hangar path"
            )));
        }
        named_outputs.insert(name.clone(), hangar_path);
        facts.insert(format!("nix.output.{name}"), store_path.clone());
        facts.insert(
            format!("nix.cache.output.{name}.proof.sha256"),
            object.upstream_proof_sha256.clone(),
        );
        replay_facts.insert(format!("nix.output.{name}"), store_path.clone());
    }
    let primary_name = if named_outputs.contains_key("out") {
        "out"
    } else {
        "bin"
    };
    let primary = named_outputs.get(primary_name).cloned().ok_or_else(|| {
        ProviderError::BadOutput("indexed Nix record has no primary output".into())
    })?;
    let bin_root = named_outputs.get("bin").unwrap_or(&primary);
    let bin = Path::new(bin_root)
        .join("bin")
        .to_string_lossy()
        .into_owned();
    let name = nix_package_name(&spec.package).to_string();
    if let Some((_, expected)) = spec.package.split_once("#version=") {
        if verified.record.version != expected {
            return Err(ProviderError::BuildFailed(format!(
                "Nix package `{name}` realized as version `{}`, expected `{expected}`",
                verified.record.version
            )));
        }
    }
    let primary_object = admitted.outputs.get(primary_name).ok_or_else(|| {
        ProviderError::BadOutput("indexed Nix record has no admitted primary output".into())
    })?;
    let mut envelope = super::Envelope::Envelope::for_output(&primary, &spec.raw, "nix");
    envelope.output_hash = primary_object.hangar_digest.clone();
    let provisional_identity = super::Store::CacheIdentity {
        source_fingerprint: envelope.output_hash.clone(),
        recipe_fingerprint: SHA256::sha256_hex(NIX_RECIPE_ID.as_bytes()),
        policy_fingerprint: super::RuntimePolicy::cache_policy_fingerprint(false),
        platform: super::Envelope::host_platform(),
    };
    let derivation_digest = SHA256::sha256_hex(verified.record.drv_path.as_bytes());
    let producer = producer_record(
        "nix",
        &verified.record.drv_path,
        &derivation_digest,
        replay_facts,
        &format!("nix-derivation:{}", verified.record.drv_path),
        &provisional_identity,
        facts,
    )?;
    let mut references = admitted
        .outputs
        .values()
        .flat_map(|object| object.direct_reference_digests.iter().cloned())
        .collect::<Vec<_>>();
    references.sort();
    references.dedup();
    if references.iter().any(|digest| digest.is_empty()) {
        return Err(ProviderError::BadOutput(
            "Nix cache admission returned an empty closure reference digest".into(),
        ));
    }
    Ok(Realized {
        version: verified.record.version,
        name,
        reference: spec.raw.clone(),
        out: primary,
        bin,
        rlib: String::new(),
        envelope,
        cache_identity: super::Store::CacheIdentity::default(),
        source_state: SourceState::Substituted,
        named_outputs,
        references,
        producer,
    })
}


/// Pick the provider for an already-resolved kind. Direct external roots use
/// an explicit fail-closed boundary until their native realization path exists;
/// they never fall through to the Nix compatibility provider.
pub(crate) fn provider_for(kind: ProviderKind) -> Box<dyn Provider> {
    match kind {
        ProviderKind::Core => Box::new(CoreProvider),
        ProviderKind::Cran => Box::new(CranProvider),
        ProviderKind::LuaRocks => Box::new(LuaRocksProvider),
        ProviderKind::RubyGems => Box::new(ScriptRegistryProvider(ScriptRegistryKind::RubyGems)),
        ProviderKind::Cpan => Box::new(ScriptRegistryProvider(ScriptRegistryKind::Cpan)),
        ProviderKind::Packagist => Box::new(ScriptRegistryProvider(ScriptRegistryKind::Packagist)),
        ProviderKind::JetPackage => Box::new(NativeProvider),
        ProviderKind::JetRegistry => Box::new(UnsupportedProvider("jet-registry")),
        ProviderKind::Npm => Box::new(UnsupportedProvider("npm")),
        ProviderKind::Cargo => Box::new(UnsupportedProvider("cargo")),
        ProviderKind::PyPI => Box::new(UnsupportedProvider("pypi")),
        ProviderKind::SwiftPM => Box::new(UnsupportedProvider("swiftpm")),
        _ => Box::new(NixProvider),
    }
}

/// Resolve a ref's concrete provider kind, running the U9
/// realize-time probe when the source table left the kind to **inference**
/// (a typed `…@github` source). `offline`/`cache_dir` come from the realize
/// context: offline never hits the network — it reuses a cached checkout if
/// present, else falls back to `nix`.
///
/// Built-in sources, bare paths, and `…@nixpkgs` named sources are already concrete
/// in the table, so no probe runs for them.
pub fn resolve_kind(
    spec: &RefSpec,
    table: &SourceTable,
    offline: bool,
    cache_dir: &Path,
) -> ProviderKind {
    if matches!(spec.source, Source::Cran) {
        return ProviderKind::Cran;
    }
    if matches!(spec.source, Source::LuaRocks) {
        return ProviderKind::LuaRocks;
    }
    if matches!(spec.source, Source::RubyGems) {
        return ProviderKind::RubyGems;
    }
    if matches!(spec.source, Source::Cpan) {
        return ProviderKind::Cpan;
    }
    if matches!(spec.source, Source::Packagist) {
        return ProviderKind::Packagist;
    }
    if matches!(spec.source, Source::JetRegistry) {
        return ProviderKind::JetRegistry;
    }
    if matches!(spec.source, Source::Npm) {
        return ProviderKind::Npm;
    }
    if matches!(spec.source, Source::Cargo) {
        return ProviderKind::Cargo;
    }
    if matches!(spec.source, Source::PyPI) {
        return ProviderKind::PyPI;
    }
    if matches!(spec.source, Source::SwiftPM) {
        return ProviderKind::SwiftPM;
    }
    if matches!(spec.source, Source::Releases) {
        return ProviderKind::JetPackage;
    }
    let Source::Named(name) = &spec.source else {
        return ProviderKind::Nix;
    };
    match table.provider(name) {
        ProviderKind::Core => ProviderKind::Core,
        ProviderKind::Cran => ProviderKind::Cran,
        ProviderKind::LuaRocks => ProviderKind::LuaRocks,
        ProviderKind::RubyGems => ProviderKind::RubyGems,
        ProviderKind::Cpan => ProviderKind::Cpan,
        ProviderKind::Packagist => ProviderKind::Packagist,
        ProviderKind::JetRegistry => ProviderKind::JetRegistry,
        ProviderKind::Npm => ProviderKind::Npm,
        ProviderKind::Cargo => ProviderKind::Cargo,
        ProviderKind::PyPI => ProviderKind::PyPI,
        ProviderKind::SwiftPM => ProviderKind::SwiftPM,
        ProviderKind::JetPackage => ProviderKind::JetPackage,
        ProviderKind::Nix => ProviderKind::Nix,
        // U9: peek the remote's `pkg.jet` to choose core vs nix.
        ProviderKind::Infer => match table.upstream(name) {
            Some(upstream) => infer_remote_kind(upstream, offline, cache_dir),
            None => ProviderKind::Nix,
        },
    }
}

fn script_registry_kind(kind: ProviderKind) -> Option<ScriptRegistryKind> {
    match kind {
        ProviderKind::RubyGems => Some(ScriptRegistryKind::RubyGems),
        ProviderKind::Cpan => Some(ScriptRegistryKind::Cpan),
        ProviderKind::Packagist => Some(ScriptRegistryKind::Packagist),
        _ => None,
    }
}

/// True when realizing this ref goes through the Nix compatibility provider.
/// Resolves the kind first (so an inferred `…@github` source is probed).
pub fn uses_nix_provider(
    spec: &RefSpec,
    table: &SourceTable,
    offline: bool,
    cache_dir: &Path,
) -> bool {
    matches!(
        resolve_kind(spec, table, offline, cache_dir),
        ProviderKind::Nix | ProviderKind::Infer
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DownloadPlan {
    pub packages: usize,
    pub bytes: Option<u64>,
}

impl DownloadPlan {
    fn add(&mut self, packages: usize, bytes: Option<u64>) {
        self.packages = self.packages.saturating_add(packages);
        self.bytes = match (self.bytes, bytes) {
            (Some(left), Some(right)) => left.checked_add(right),
            _ => None,
        };
    }
}

/// Resolve acquisition metadata without realizing a provider. Nix uses the
/// signed index plus narinfo closure; native release fixtures contribute their
/// exact artifact length. Providers without a byte-bearing metadata seam keep
/// the total unknown instead of inventing one.
pub(crate) fn plan_downloads(
    specs: &[RefSpec],
    table: &SourceTable,
    ctx: &Ctx,
) -> Result<DownloadPlan, ProviderError> {
    let mut plan = DownloadPlan::default();
    let mut nix_paths = Vec::new();

    for spec in specs {
        match resolve_kind(spec, table, ctx.offline, ctx.store_dir) {
            ProviderKind::Nix => {
                let index = ctx.nix_index.ok_or_else(|| {
                    ProviderError::Unsupported(format!(
                        "download planning needs a signed index for `{}`",
                        spec.raw
                    ))
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
                        plan.add(1, None);
                        continue;
                    }
                    Err(error) => return Err(ProviderError::NixIndex(error)),
                };
                nix_paths.extend(verified.record.outputs.values().cloned());
            }
            ProviderKind::JetPackage => {
                plan.add(1, native::download_size(spec, table, ctx)?);
            }
            ProviderKind::Core => {
                if table
                    .upstream(spec.source.label())
                    .is_none_or(|upstream| !upstream.starts_with("path:"))
                {
                    plan.add(1, None);
                }
            }
            ProviderKind::Cran
            | ProviderKind::LuaRocks
            | ProviderKind::RubyGems
            | ProviderKind::Cpan
            | ProviderKind::Packagist => plan.add(1, None),
            ProviderKind::Infer
            | ProviderKind::JetRegistry
            | ProviderKind::Npm
            | ProviderKind::Cargo
            | ProviderKind::PyPI
            | ProviderKind::SwiftPM => {}
        }
    }

    if !nix_paths.is_empty() {
        let roots = ctx.nix_roots.ok_or_else(|| {
            ProviderError::BadOutput(
                "download planning has no Hangar roots for closure admission".into(),
            )
        })?;
        let nix = plan_nix_downloads(roots, &nix_paths, ctx.offline)
            .map_err(|error| nix_cache_error(roots, error))?;
        plan.add(nix.packages, Some(nix.bytes));
    }
    Ok(plan)
}

/// U23 / D-JPK-NIXSTORE1=D: package refs that resolve through the Nix
/// compatibility provider need an explicit compatibility output unless a
/// locked nixpkgs ref is representable by the signed index. This fact is
/// computed before provider dispatch, so genuine v1 holes get one package-
/// focused diagnostic without probing Nix.
pub fn needs_nix_bridge(
    spec: &RefSpec,
    table: &SourceTable,
    offline: bool,
    cache_dir: &Path,
    project_dir: Option<&Path>,
) -> Option<NixBridgeNeed> {
    if uses_nix_provider(spec, table, offline, cache_dir)
        && !has_locked_nix_index_key(spec, table, project_dir, host_nix_system())
    {
        Some(NixBridgeNeed {
            reference: spec.raw.clone(),
            package: spec.short_name().to_string(),
        })
    } else {
        None
    }
}

/// An invalid indexed-Nix cache entry may be repaired by the same signed
/// index/cache path that created it. Other providers keep the Store's
/// fail-closed integrity behavior when no generic cache binding exists.
pub(crate) fn can_repair_indexed_nix(spec: &RefSpec, table: &SourceTable, ctx: &Ctx) -> bool {
    ctx.fixtures.is_none()
        && uses_nix_provider(spec, table, ctx.offline, ctx.store_dir)
        && locked_nix_index_key(spec, table, ctx, host_nix_system().unwrap_or_default()).is_ok()
}

/// Realize a ref through its provider. The resolver entry point: it never knows
/// or cares which backend runs — that is the whole point of the boundary.
pub(crate) fn realize(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
) -> Result<Realized, ProviderError> {
    let kind = resolve_kind(spec, table, ctx.offline, ctx.store_dir);
    let mut realized = provider_for(kind).realize(spec, table, ctx)?;
    refresh_provider_facts(&mut realized.producer, &realized.reference)?;
    Ok(realized)
}




/// U9 remote probe: classify an `…@github`/git upstream as `Core` (it carries a
/// `package.jet` or migration-era `pkg.jet`) or `Nix` (it does not), peeking
/// **only** the root package marker — never
/// cloning a nixpkgs-sized repo just to classify it.
///
/// Resolution order:
/// 1. If a source-cache checkout already exists (a prior realize fetched it),
///    classify from the local tree — offline-safe, no network.
/// 2. Offline with no cache: we can't probe, so default to `nix`.
/// 3. Online: a lightweight `git` peek — a partial, no-checkout, depth-1 clone
///    (`--filter=tree:0`, so blobs/subtrees are never downloaded) into a temp
///    dir, then `git ls-tree <rev> package.jet pkg.jet`. Present → `Core`;
///    absent or any peek failure → `Nix` (the safe default; a github flake
///    still realizes through nix).
fn infer_remote_kind(upstream: &str, offline: bool, cache_dir: &Path) -> ProviderKind {
    let Ok(remote) = parse_remote_source(upstream) else {
        return ProviderKind::Nix;
    };
    // (1) Reuse a prior fetch.
    let cache = source_cache_dir(cache_dir, &remote);
    if cache.is_dir() {
        return pack_kind(
            cache.join(crate::Syntax::PACKAGE_FILE).is_file()
                || cache.join(crate::Syntax::PAYLOAD_FILE).is_file(),
        );
    }
    // (2) Offline can't reach the network; a remote we haven't cached stays nix.
    if offline {
        return ProviderKind::Nix;
    }
    // (3) Lightweight online peek.
    pack_kind(remote_has_pack_jet(&remote))
}

fn pack_kind(has_pack: bool) -> ProviderKind {
    if has_pack {
        ProviderKind::Core
    } else {
        ProviderKind::Nix
    }
}

/// Peek whether `remote` has a package marker at its root, without a full clone.
///
/// Fetches **only the named rev** into a throwaway repo, shallow (`--depth 1`)
/// and partial (`--filter=tree:0`, so trees/blobs are deferred), then reads the
/// root tree with `git ls-tree FETCH_HEAD`. Even a nixpkgs-sized repo transfers
/// just the one commit object plus the lazily-fetched root tree. `git fetch`
/// resolves a branch, tag, **or** commit SHA uniformly, so the rev's exact
/// The canonical `package.jet` marker is preferred, with `pkg.jet` retained for
/// migration-era sources. Any failure (no `git`, network error, unfetchable
/// rev) is treated as "no package marker" by the caller (→ nix), the safe
/// default.
fn remote_has_pack_jet(remote: &RemoteSource) -> bool {
    if network_denied() {
        return false;
    }
    if Command::new("git").arg("--version").output().is_err() {
        return false;
    }
    let tmp = std::env::temp_dir().join(format!(
        "jetpack-peek-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    if std::fs::create_dir_all(&tmp).is_err() {
        return false;
    }

    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&tmp)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    // A configured `origin` makes the partial fetch register a promisor remote,
    // so the deferred root tree can be lazily fetched on `ls-tree`.
    let rev = remote.rev.as_deref().unwrap_or("HEAD");
    let set_up = git(&["init", "--quiet"]) && git(&["remote", "add", "origin", &remote.url]);
    let fetched = set_up
        && git(&[
            "fetch",
            "--quiet",
            "--depth",
            "1",
            "--filter=tree:0",
            "origin",
            rev,
        ]);
    let has_pack = fetched
        && Command::new("git")
            .arg("-C")
            .arg(&tmp)
            .args([
                "ls-tree",
                "FETCH_HEAD",
                crate::Syntax::PACKAGE_FILE,
                crate::Syntax::PAYLOAD_FILE,
            ])
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false);

    let _ = std::fs::remove_dir_all(&tmp);
    has_pack
}

fn network_denied() -> bool {
    std::env::var_os("JETPACK_DENY_NETWORK").is_some_and(|v| !v.is_empty())
}

fn ensure_network_allowed(need: &str) -> Result<(), ProviderError> {
    if network_denied() {
        Err(ProviderError::Offline(format!(
            "network disabled by JETPACK_DENY_NETWORK while trying to {need}"
        )))
    } else {
        Ok(())
    }
}

/// Parse a pinned compatibility result: an array of build results, each with
/// an `outputs` object. `out` is canonical primary; `bin` remains a named output.
fn parse_realization(spec: &RefSpec, stdout: &str) -> Result<Realized, ProviderError> {
    let parsed = JSON::parse_lenient(stdout).map_err(ProviderError::BadOutput)?;
    let bad_output = |reason: String| ProviderError::BadOutput(parsed.diagnostic(reason));
    let arr = parsed.value.as_array().map_err(&bad_output)?;
    if arr.len() != 1 {
        return Err(bad_output(format!(
            "provider produced {} build results; expected exactly one",
            arr.len()
        )));
    }
    let first = arr
        .first()
        .ok_or_else(|| bad_output("provider produced no build results".into()))?;
    let fallback = crate::NixFallback::import_record(first)
        .map_err(|error| bad_output(format!("could not import fallback state: {error}")))?;
    let outputs = first.get("outputs").map_err(&bad_output)?;
    let outputs = outputs.as_object().map_err(&bad_output)?;
    let drv_path = first
        .get("drvPath")
        .and_then(|value| value.as_str())
        .map_err(&bad_output)?;
    if drv_path.trim().is_empty() {
        return Err(bad_output("provider output had no exact `drvPath`".into()));
    }

    let named_outputs = outputs
        .iter()
        .map(|(name, value)| {
            value
                .as_str()
                .map(|path| (name.clone(), path.to_string()))
                .map_err(&bad_output)
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    let out = outputs
        .get("out")
        .or_else(|| outputs.get("bin"))
        .and_then(|j| j.as_str().ok())
        .ok_or_else(|| bad_output("provider output had no `out`/`bin` store path".into()))?;

    let bin_root = named_outputs.get("bin").map(String::as_str).unwrap_or(out);
    let bin = format!("{}/bin", bin_root.trim_end_matches('/'));
    let name = nix_package_name(spec.short_name()).to_string();
    let envelope = super::Envelope::Envelope::for_output(out, &spec.raw, "nix");
    let provisional_identity = super::Store::CacheIdentity {
        source_fingerprint: envelope.output_hash.clone(),
        recipe_fingerprint: SHA256::sha256_hex(NIX_RECIPE_ID.as_bytes()),
        policy_fingerprint: super::RuntimePolicy::cache_policy_fingerprint(false),
        platform: super::Envelope::host_platform(),
    };
    let mut replay_facts = BTreeMap::from([
        ("nix.drv_path".into(), drv_path.to_string()),
        ("nix.reference".into(), spec.raw.clone()),
        ("build.sandbox".into(), "non-executing".into()),
        (
            "build.sandbox_policy".into(),
            "trusted substitution (no local executable launched)".into(),
        ),
    ]);
    let mut facts = BTreeMap::from([
        ("nix.drv_path".into(), drv_path.to_string()),
        ("build.sandbox".into(), "non-executing".into()),
        (
            "build.sandbox_policy".into(),
            "trusted substitution (no local executable launched)".into(),
        ),
    ]);
    for (name, path) in &named_outputs {
        replay_facts.insert(format!("nix.output.{name}"), path.clone());
        facts.insert(format!("nix.output.{name}"), path.clone());
    }
    if let Some(fallback) = fallback {
        for (key, value) in fallback.facts() {
            replay_facts.insert(key.clone(), value.clone());
            facts.insert(key.clone(), value.clone());
        }
    }
    // The `.drv` path is Nix's canonical input/action identity. Realized
    // outputs are consequences and must never enter the derivation digest.
    let derivation_digest = SHA256::sha256_hex(drv_path.as_bytes());
    let producer = producer_record(
        "nix",
        drv_path,
        &derivation_digest,
        replay_facts,
        &format!("nix-derivation:{drv_path}"),
        &provisional_identity,
        facts,
    )?;
    let version = nix_store_version(out, &name);
    if let Some((_, expected)) = spec.package.split_once("#version=") {
        if version != expected {
            return Err(ProviderError::BuildFailed(format!(
                "Nix package `{name}` realized as version `{version}`, expected `{expected}`"
            )));
        }
    }
    Ok(Realized {
        version,
        name,
        reference: spec.raw.clone(),
        out: out.to_string(),
        bin,
        rlib: String::new(),
        envelope,
        cache_identity: super::Store::CacheIdentity::default(),
        source_state: SourceState::Substituted,
        named_outputs,
        references: Vec::new(),
        producer,
    })
}

/// Recover a package version from a Nix store path basename, which by
/// convention is `<32-char-hash>-<pname>-<version>[-<output>]`. We strip the
/// fixed-width hash, the known `<name>-` prefix, and any trailing output
/// segment, then accept the remainder only if it looks like a version (leads
/// with a digit). Anything we can't confidently parse yields an empty version,
/// so the hangar id falls back to `<name>-<fp>` rather than guessing wrong.
fn nix_store_version(out: &str, name: &str) -> String {
    const HASH_LEN: usize = 32;
    const OUTPUT_SUFFIXES: &[&str] = &["-bin", "-dev", "-lib", "-doc", "-man", "-info", "-out"];

    let base = out.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let Some(rest) = base.get(HASH_LEN..) else {
        return String::new();
    };
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    let Some(mut version) = rest.strip_prefix(name).and_then(|s| s.strip_prefix('-')) else {
        return String::new();
    };
    for suffix in OUTPUT_SUFFIXES {
        if let Some(stripped) = version.strip_suffix(suffix) {
            version = stripped;
            break;
        }
    }
    if version.starts_with(|c: char| c.is_ascii_digit()) {
        version.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::RefSpec::{classify, classify_in};
    use super::*;
    use crate::Store::NixCache::AdmittedNixObject;

    fn empty() -> SourceTable {
        SourceTable::empty()
    }

    #[test]
    fn provider_module_stays_split_by_source_ownership() {
        const MAX_MODULE_LINES: usize = 2500;
        let root =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/Provider.rs"))
                .unwrap();
        let remote = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/Provider/remote.rs"),
        )
        .unwrap();
        let production_root = root.split("#[cfg(test)]\nmod tests").next().unwrap();

        // Keep the production module bounded; provider tests are intentionally
        // colocated because they exercise private dispatch helpers.
        assert!(production_root.lines().count() < MAX_MODULE_LINES);
        assert!(remote.lines().count() < MAX_MODULE_LINES);
        assert!(production_root.contains("\nmod remote;\n"));
        assert!(!production_root.contains("include!("));
        assert!(!remote.contains("include!("));
    }

    #[test]
    fn nix_lock_digest_ignores_hangar_receipt_projection() {
        let root = unique_dir("nix-lock-receipt-digest");
        let managed = root.join(crate::Syntax::SOURCE_ROOT_DIR);
        std::fs::create_dir_all(&managed).unwrap();
        let lock_path = managed.join("lock");
        let base = "version = 1\n\n[[package]]\nname = \"greet\"\nversion = \"1\"\nsource = { path = \"greet\" }\nfingerprint = \"\"\ndependencies = []\n";
        std::fs::write(&lock_path, base).unwrap();
        let without_receipt = project_lock_digest(Some(&root)).unwrap();

        std::fs::write(
            &lock_path,
            format!("{base}receipt = \"sha256-{}\"\n", "a".repeat(64)),
        )
        .unwrap();
        let with_receipt = project_lock_digest(Some(&root)).unwrap();
        assert_eq!(without_receipt, with_receipt);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn locked_nix_index_key_uses_exact_channel_and_literal_dot_attr() {
        let project = unique_dir("locked-nix-index-key");
        let managed = project.join(crate::Syntax::SOURCE_ROOT_DIR);
        std::fs::create_dir_all(&managed).unwrap();
        let revision = "c8f90650c15282fa8656a041bfbbd2403997a9a7";
        std::fs::write(
            managed.join("lock"),
            format!(
                "version = 1\n\n[[source_channel]]\nname = \"stable\"\nchannel = \"nixpkgs-unstable\"\nexact = \"github:NixOS/nixpkgs#{revision}\"\n\n[root]\ndependencies = []\n"
            ),
        )
        .unwrap();
        let table = SourceTable::from_decls([(
            "stable".to_string(),
            "NixOS/nixpkgs/nixpkgs-unstable@github".to_string(),
            ProviderKind::Nix,
        )]);
        let spec = classify_in("ripgrep.foo@stable", &table).unwrap();
        let store = project.join("hangar");
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
            project_dir: Some(&project),
            nix_index: None,
            nix_roots: None,
        };
        let key = locked_nix_index_key(&spec, &table, &ctx, "x86_64-linux").unwrap();
        assert_eq!(key.channel, "nixpkgs-unstable");
        assert_eq!(key.revision, revision);
        assert_eq!(key.system, "x86_64-linux");
        assert_eq!(key.attrpath, vec!["ripgrep.foo"]);
        std::fs::remove_dir_all(project).ok();
    }

    #[test]
    fn indexed_realization_records_union_and_both_provenance_families() {
        use crate::NixIndex::{IndexProof, IndexRecord, VerifiedIndexRecord};

        let root = unique_dir("indexed-realization-provenance");
        let out_path = root.join("objects/out");
        let bin_path = root.join("objects/bin");
        std::fs::create_dir_all(&out_path).unwrap();
        std::fs::create_dir_all(&bin_path).unwrap();
        let out_store = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-ripgrep-15.2.0";
        let bin_store = "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-ripgrep-15.2.0-bin";
        let leaf = "sha256-cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let bin_leaf = "sha256-dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        let record = IndexRecord {
            attrpath: vec!["ripgrep".into()],
            version: "15.2.0".into(),
            drv_path: "/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-ripgrep.drv".into(),
            outputs: BTreeMap::from([
                ("out".into(), out_store.into()),
                ("bin".into(), bin_store.into()),
            ]),
        };
        let proof = IndexProof {
            schema: 1,
            channel: "nixpkgs-unstable".into(),
            revision: "c8f90650c15282fa8656a041bfbbd2403997a9a7".into(),
            system: "x86_64-linux".into(),
            attrpath: vec!["ripgrep".into()],
            manifest_generation: 7,
            manifest_sha256: "1".repeat(64),
            index_sha256: "2".repeat(64),
            record_sha256: "3".repeat(64),
            jet_key_id: "test-key".into(),
            jet_signature: "signature".into(),
        };
        let admitted = AdmittedNixClosure {
            outputs: BTreeMap::from([
                (
                    "out".into(),
                    AdmittedNixObject {
                        store_path: out_store.into(),
                        hangar_path: out_path,
                        hangar_digest: "sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                        direct_reference_digests: vec![leaf.into()],
                        upstream_proof_sha256: "4".repeat(64),
                    },
                ),
                (
                    "bin".into(),
                    AdmittedNixObject {
                        store_path: bin_store.into(),
                        hangar_path: bin_path,
                        hangar_digest: "sha256-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                        direct_reference_digests: vec![bin_leaf.into(), leaf.into()],
                        upstream_proof_sha256: "5".repeat(64),
                    },
                ),
            ]),
            objects: BTreeMap::from([
                (
                    out_store.into(),
                    AdmittedNixObject {
                        store_path: out_store.into(),
                        hangar_path: root.join("objects/out"),
                        hangar_digest: "sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                        direct_reference_digests: vec![leaf.into()],
                        upstream_proof_sha256: "4".repeat(64),
                    },
                ),
                (
                    bin_store.into(),
                    AdmittedNixObject {
                        store_path: bin_store.into(),
                        hangar_path: root.join("objects/bin"),
                        hangar_digest: "sha256-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                        direct_reference_digests: vec![bin_leaf.into(), leaf.into()],
                        upstream_proof_sha256: "5".repeat(64),
                    },
                ),
            ]),
            closure_receipt_sha256: "6".repeat(64),
        };
        let spec = classify("ripgrep@jetpack").unwrap();
        let realized = realization_from_index(
            &spec,
            &IndexKey {
                channel: "nixpkgs-unstable".into(),
                revision: "c8f90650c15282fa8656a041bfbbd2403997a9a7".into(),
                system: "x86_64-linux".into(),
                attrpath: vec!["ripgrep".into()],
            },
            VerifiedIndexRecord {
                record,
                proof,
                trust: IndexTrustTier::OfficialSigned,
            },
            admitted,
        )
        .unwrap();

        assert_eq!(realized.source_state, SourceState::Substituted);
        assert_eq!(
            realized.references,
            vec![leaf.to_string(), bin_leaf.to_string()]
        );
        assert_eq!(realized.named_outputs.len(), 2);
        let facts = realized.producer.facts;
        assert!(facts.contains_key("nix.index.proof.v1"));
        assert_eq!(facts["nix.index.manifest.sha256"], "1".repeat(64));
        assert_eq!(facts["nix.cache.output.out.proof.sha256"], "4".repeat(64));
        assert_eq!(facts["nix.cache.output.bin.proof.sha256"], "5".repeat(64));
        assert_eq!(facts["nix.cache.closure.receipt.sha256"], "6".repeat(64));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn top_level_run_drives_kind_inference() {
        // D-ILE1: a top-level `fn run` means executable.
        assert!(file_has_top_level_run("fn run() {}\n"));
        assert!(file_has_top_level_run(
            "fn helper() => Int { return 1; }\nfn run() { print(\"hi\"); }\n"
        ));
        // A `run` nested in a module/impl block is not the entry point.
        assert!(!file_has_top_level_run("module m { fn run() {} }\n"));
        // A library: no top-level `fn run`.
        assert!(!file_has_top_level_run(
            "fn add(a: Int, b: Int) => Int { return a + b; }\n"
        ));
        // `fn run` inside a comment or string never counts.
        assert!(!file_has_top_level_run("// fn run()\nfn lib() {}\n"));
        assert!(!file_has_top_level_run(
            "fn lib() { let s = \"fn run()\"; }\n"
        ));
    }

    #[test]
    fn nix_store_version_parses_path_suffix() {
        let h = "0000000000000000000000000000000a"; // 32-char stand-in hash
                                                    // Plain `out` path: version is the trailing segment.
        assert_eq!(
            nix_store_version(&format!("/nix/store/{h}-fastfetch-2.1.0"), "fastfetch"),
            "2.1.0"
        );
        // Split `bin` output: the `-bin` suffix is stripped.
        assert_eq!(
            nix_store_version(&format!("/nix/store/{h}-ripgrep-14.1.0-bin"), "ripgrep"),
            "14.1.0"
        );
        // Hyphenated package names are honored by matching the known name.
        assert_eq!(
            nix_store_version(&format!("/nix/store/{h}-jq-lib-1.7.1"), "jq-lib"),
            "1.7.1"
        );
        // No recognizable version → empty, so the id falls back to `<name>-<fp>`.
        assert_eq!(
            nix_store_version(&format!("/nix/store/{h}-hello-unstable"), "hello"),
            ""
        );
        assert_eq!(nix_store_version("/some/local/path", "hello"), "");
    }

    #[test]
    fn translates_ref_to_flake() {
        assert_eq!(
            flake_ref(&classify("fastfetch@jetpack").unwrap(), &empty()),
            "nixpkgs#fastfetch"
        );
        assert_eq!(
            flake_ref(&classify("o/r@github").unwrap(), &empty()),
            "github:o/r"
        );
    }

    #[test]
    fn flake_ref_strips_jet_version_selector_only_for_nix() {
        let nix = classify("rustc@jetpack#version=1.80.0").unwrap();
        assert_eq!(flake_ref(&nix, &empty()), "nixpkgs#rustc");
        let cran = classify("jsonlite@cran#version=1.9.0").unwrap();
        assert_eq!(flake_ref(&cran, &empty()), "cran:jsonlite#version=1.9.0");
    }

    #[test]
    fn direct_ecosystem_roots_do_not_default_to_nix() {
        let ctx = Ctx {
            fixtures: None,
            store_dir: Path::new("."),
            offline: true,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };
        for (raw, kind, expected_ref) in [
            (
                "hello@jet-registry#version=1.0.0",
                ProviderKind::JetRegistry,
                "jet-registry:hello#version=1.0.0",
            ),
            (
                "left-pad@npm#version=1.3.0",
                ProviderKind::Npm,
                "npm:left-pad#version=1.3.0",
            ),
            (
                "serde@cargo#version=1.0.200",
                ProviderKind::Cargo,
                "cargo:serde#version=1.0.200",
            ),
        ] {
            let spec = classify(raw).unwrap();
            assert_eq!(resolve_kind(&spec, &empty(), true, Path::new(".")), kind);
            assert_eq!(flake_ref(&spec, &empty()), expected_ref);
            match provider_for(kind).realize(&spec, &empty(), &ctx) {
                Err(ProviderError::Unsupported(message)) => {
                    assert!(message.contains(kind.label()));
                    assert!(message.contains("import and lock"));
                }
                Err(error) => panic!("unexpected provider error: {error:?}"),
                Ok(_) => panic!("direct ecosystem root unexpectedly used a provider"),
            }
        }
    }

    #[test]
    fn named_source_flake_ref_uses_pin() {
        let table = SourceTable::from_decls([(
            "stable".to_string(),
            "github:NixOS/nixpkgs/nixos-24.05".to_string(),
            super::super::RefSpec::ProviderKind::Nix,
        )]);
        let spec = classify_in("ripgrep@stable", &table).unwrap();
        assert_eq!(
            flake_ref(&spec, &table),
            "github:NixOS/nixpkgs/nixos-24.05#ripgrep"
        );
        // The fixture name keys off the source name, so `stable-ripgrep.json`.
        assert_eq!(fixture_name(&spec), "stable-ripgrep.json");
    }

    #[test]
    fn named_source_changes_nix_cache_authority() {
        let old_table = SourceTable::from_decls([(
            "stable".to_string(),
            "github:NixOS/nixpkgs/nixos-24.05".to_string(),
            super::super::RefSpec::ProviderKind::Nix,
        )]);
        let new_table = SourceTable::from_decls([(
            "stable".to_string(),
            "github:NixOS/nixpkgs/nixos-24.11".to_string(),
            super::super::RefSpec::ProviderKind::Nix,
        )]);
        let spec = classify_in("ripgrep@stable", &old_table).unwrap();
        let store = PathBuf::from(".");
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: true,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };
        let old = nix_cache_identity("sha256:output", "linux-x86_64", &spec, &old_table, &ctx);
        let new = nix_cache_identity("sha256:output", "linux-x86_64", &spec, &new_table, &ctx);
        assert_ne!(
            old.policy_fingerprint, new.policy_fingerprint,
            "a named-source repoint must not reuse the old Nix cache authority"
        );
    }

    #[test]
    fn jetpack_refs_share_nix_cache_identity() {
        let table = SourceTable::empty();
        let bare = classify(&super::super::RefSpec::with_default_source("ripgrep")).unwrap();
        let explicit = classify("ripgrep@jetpack").unwrap();
        let legacy = RefSpec {
            source: Source::Nixpkgs,
            package: "ripgrep".into(),
            raw: "ripgrep@nixpkgs".into(),
        };
        let store = PathBuf::from(".");
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: true,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };

        assert_eq!(nix_identity_parts(&bare, &table), nix_identity_parts(&explicit, &table));
        assert_eq!(nix_identity_parts(&explicit, &table), nix_identity_parts(&legacy, &table));
        assert_eq!(
            nix_cache_identity("sha256:output", "linux-x86_64", &bare, &table, &ctx),
            nix_cache_identity("sha256:output", "linux-x86_64", &explicit, &table, &ctx)
        );
        assert_eq!(
            nix_cache_identity("sha256:output", "linux-x86_64", &explicit, &table, &ctx),
            nix_cache_identity("sha256:output", "linux-x86_64", &legacy, &table, &ctx)
        );
    }

    #[test]
    fn fixture_name_sanitizes_slashes() {
        let s = classify("halcyonomega/cfg@github").unwrap();
        assert_eq!(fixture_name(&s), "github-halcyonomega_cfg.json");
    }

    #[test]
    fn parses_good_output() {
        let spec = classify("fastfetch@jetpack").unwrap();
        let stdout = r#"[{"drvPath":"/nix/store/abc-fastfetch.drv","outputs":{"out":"/nix/store/abc-fastfetch-2.0"}}]"#;
        let r = parse_realization(&spec, stdout).unwrap();
        assert_eq!(r.out, "/nix/store/abc-fastfetch-2.0");
        assert_eq!(r.bin, "/nix/store/abc-fastfetch-2.0/bin");
        assert_eq!(r.name, "fastfetch");
        assert_eq!(r.producer.immutable_source, "/nix/store/abc-fastfetch.drv");
        assert_eq!(
            r.producer.source_digest,
            SHA256::sha256_hex(b"/nix/store/abc-fastfetch.drv")
        );
        assert!(!r.producer.facts.contains_key("closure.authority"));
        assert_eq!(
            r.producer
                .plan
                .facts()
                .get("nix.output.out")
                .map(String::as_str),
            Some("/nix/store/abc-fastfetch-2.0")
        );
    }

    #[test]
    fn fallback_imports_closed_graph_and_all_projections() {
        let spec = classify("fastfetch@jetpack").unwrap();
        let stdout = r#"[{"drvPath":"/nix/store/abc-fastfetch.drv","outputs":{"out":"/nix/store/abc-fastfetch-2.0","dev":"/nix/store/abc-fastfetch-2.0-dev"},"closedGraph":{"root":{"dependencies":["dep"]},"nodes":["dep"]},"dependencies":{"build":["dep"]},"sources":[{"path":"/nix/store/source","sha256":"source-hash"}],"hashes":{"out":"out-hash","dev":"dev-hash"},"losses":["shellHook"],"proof":{"evaluator":"nix-2.34","signature":"proof-signature"},"recipe":{"steps":[]},"lock":{"system":"x86_64-linux"}}]"#;
        let realized = parse_realization(&spec, stdout).unwrap();
        let facts = realized.producer.facts;
        for key in [
            "nix.fallback.graph",
            "nix.fallback.selected_outputs",
            "nix.fallback.dependencies",
            "nix.fallback.sources",
            "nix.fallback.hashes",
            "nix.fallback.losses",
            "nix.fallback.proof",
            "nix.fallback.recipe",
            "nix.fallback.lock",
            "nix.fallback.document",
            "nix.fallback.document.sha256",
            "nix.fallback.proof.sha256",
        ] {
            assert!(facts.get(key).is_some_and(|value| !value.is_empty()), "{key}");
        }
        assert!(facts["nix.fallback.graph"].contains("\"nodes\":[\"dep\"]"));
        assert!(facts["nix.fallback.losses"].contains("shellHook"));
        assert!(facts["nix.fallback.proof"].contains("proof-signature"));
        assert_eq!(facts["nix.fallback.selected_outputs"], facts["nix.fallback.outputs"]);
    }

    #[test]
    fn nix_build_facts_are_fixed_and_runtime_path_stays_composed() {
        let facts = nix_build_facts();
        assert_eq!(
            facts.get("nix.build.root").map(String::as_str),
            Some("/build")
        );
        assert_eq!(
            facts.get("nix.build.home").map(String::as_str),
            Some("/homeless-shelter")
        );
        assert_eq!(
            facts.get("nix.build.uid").map(String::as_str),
            Some("unprivileged")
        );
        assert_eq!(
            facts.get("nix.build.time").map(String::as_str),
            Some("deterministic")
        );
        let producer_facts = nix_build_facts_record();
        let plan = crate::Comptime::Build::BuildPlanReplay::from_facts(BTreeMap::new()).unwrap();
        let producer = super::super::Store::ProducerRecord::new(
            "nix",
            "/nix/store/fake.drv",
            "sha256-fake",
            plan,
            "nix-compat",
            "policy=test\nplatform=test",
            producer_facts,
        )
        .unwrap();
        let runtime = nix_runtime_environment(&producer);
        assert_eq!(
            runtime.get("HOME").map(String::as_str),
            Some("/homeless-shelter")
        );
        assert_eq!(
            runtime.get("NIX_BUILD_TOP").map(String::as_str),
            Some("/build")
        );
        assert_eq!(runtime.get("LC_ALL").map(String::as_str), Some("C"));
        assert!(!runtime.contains_key("PATH"));

        let mut tampered = producer.clone();
        tampered
            .facts
            .insert("nix.build.env.HOME".into(), "/tmp".into());
        assert!(validate_nix_build_facts(&tampered).is_err());
        assert!(nix_runtime_environment(&tampered).is_empty());
    }

    #[test]
    fn shared_carrier_rejects_unpinned_external_reference() {
        let plan = crate::Comptime::Build::BuildPlanReplay::from_facts(BTreeMap::from([(
            "action.kind".to_string(),
            "test".to_string(),
        )]))
        .unwrap();
        let mut producer = super::super::Store::ProducerRecord::new(
            "npm",
            "cas:source",
            "source",
            plan,
            "test-toolchain",
            "policy=test\nplatform=any",
            BTreeMap::new(),
        )
        .unwrap();
        let error = refresh_provider_facts(&mut producer, "left-pad@npm").unwrap_err();
        assert!(matches!(error, ProviderError::BadOutput(reason) if reason.contains("exact")));
        assert!(!producer.facts.contains_key("provider-facts"));
    }

    /// Card #641: `nix build --json` output wrapped in the host's own
    /// store-optimise noise (hard-link ceiling hit) must still realize —
    /// this used to die with `ProviderError::BadOutput`, "likely a Jetpack
    /// bug", for any user on a large optimised store.
    #[test]
    fn tolerates_nix_store_noise_around_output() {
        let spec = classify("fastfetch@jetpack").unwrap();
        let stdout = "\"/nix/store/.links/1gs2lc42h68lmq8fkcwp96lhnrqcyr3zwmi75k0896nbvc3p4fpc\" has maximum number of links\n\
             [{\"drvPath\":\"/nix/store/abc-fastfetch.drv\",\"outputs\":{\"out\":\"/nix/store/abc-fastfetch-2.0\"}}]\n\
             \"/nix/store/.links/1gs2lc42h68lmq8fkcwp96lhnrqcyr3zwmi75k0896nbvc3p4fpc\" has maximum number of links\n";
        let r = parse_realization(&spec, stdout).unwrap();
        assert_eq!(r.out, "/nix/store/abc-fastfetch-2.0");
    }

    #[test]
    fn tolerates_nix_hard_link_noise_between_multiline_realization_lines() {
        let spec = classify("fastfetch@jetpack").unwrap();
        let stdout = "[\n\
             {\"drvPath\":\"/nix/store/abc-fastfetch.drv\",\n\
             \"/nix/store/.links/1gs2lc42h68lmq8fkcwp96lhnrqcyr3zwmi75k0896nbvc3p4fpc\" has maximum number of links\n\
             \"outputs\":{\"out\":\"/nix/store/abc-fastfetch-2.0\"}}\n\
             ]\n";
        let r = parse_realization(&spec, stdout).unwrap();
        assert_eq!(r.out, "/nix/store/abc-fastfetch-2.0");
    }

    #[test]
    fn rejects_duplicate_realization_payloads() {
        let spec = classify("fastfetch@jetpack").unwrap();
        let payload = r#"[{"drvPath":"/nix/store/abc-fastfetch.drv","outputs":{"out":"/nix/store/abc-fastfetch-2.0"}}]"#;
        assert!(matches!(
            parse_realization(&spec, &format!("{payload}\n{payload}\n")),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn rejects_multiple_nix_realization_results_in_one_payload() {
        let spec = classify("fastfetch@jetpack").unwrap();
        let stdout = r#"[
            {"drvPath":"/nix/store/abc-fastfetch.drv","outputs":{"out":"/nix/store/abc-fastfetch-2.0"}},
            {"drvPath":"/nix/store/def-fastfetch.drv","outputs":{"out":"/nix/store/def-fastfetch-2.0"}}
        ]"#;
        let Err(ProviderError::BadOutput(reason)) = parse_realization(&spec, stdout) else {
            panic!("multiple Nix results must be rejected");
        };
        assert!(reason.contains("expected exactly one"), "{reason}");
    }

    #[test]
    fn realization_schema_error_retains_filtered_provider_noise() {
        let spec = classify("fastfetch@jetpack").unwrap();
        let noise = "warning: ignoring untrusted substituter";
        let error = parse_realization(&spec, &format!("{noise}\n[{{}}]\n")).unwrap_err();
        let ProviderError::BadOutput(reason) = error else {
            panic!("expected BadOutput, got {error:?}");
        };
        assert!(reason.contains("missing key `outputs`"));
        assert!(reason.contains(noise));
    }

    #[test]
    fn prefers_bin_output() {
        let spec = classify("git@jetpack").unwrap();
        let stdout = r#"[{"drvPath":"/nix/store/x.drv","outputs":{"out":"/nix/store/x","bin":"/nix/store/x-bin"}}]"#;
        let r = parse_realization(&spec, stdout).unwrap();
        assert_eq!(r.out, "/nix/store/x");
        assert_eq!(r.bin, "/nix/store/x-bin/bin");
        assert_eq!(r.named_outputs.len(), 2);
    }

    #[test]
    fn empty_output_is_bad() {
        let spec = classify("x@jetpack").unwrap();
        assert!(matches!(
            parse_realization(&spec, "[]"),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn garbage_output_is_bad() {
        let spec = classify("x@jetpack").unwrap();
        assert!(matches!(
            parse_realization(&spec, "not json"),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn missing_outputs_key_is_bad() {
        let spec = classify("x@jetpack").unwrap();
        assert!(matches!(
            parse_realization(&spec, r#"[{"drvPath":"/x.drv"}]"#),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn missing_exact_derivation_is_bad() {
        let spec = classify("x@jetpack").unwrap();
        assert!(matches!(
            parse_realization(&spec, r#"[{"outputs":{"out":"/nix/store/x"}}]"#),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn fixture_missing_errors() {
        let spec = classify("nope@jetpack").unwrap();
        let dir = std::env::temp_dir();
        let ctx = Ctx {
            fixtures: Some(&dir.join("definitely-not-here-xyz")),
            store_dir: &dir,
            offline: false,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };
        match realize(&spec, &empty(), &ctx) {
            Err(ProviderError::FixtureMissing(_)) => {}
            other => panic!("expected FixtureMissing, got {other:?}"),
        }
    }

    #[test]
    fn core_provider_builds_local_package() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        // Repo with package.jet + a `module hello` declaration + bin/. No env.jet
        // (U10 Chunk 3: CoreProvider discovers the package by module name).
        let base = unique_dir("jpk-core");
        let repo = base.join("jet-pkgs");
        let store = base.join("store");
        let hello_pkg = repo.join("pkgs/hello");
        let hello_bin = hello_pkg.join("bin");
        std::fs::create_dir_all(&hello_bin).unwrap();
        std::fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
        std::fs::write(hello_bin.join("hello"), "#!/bin/sh\necho hi\n").unwrap();
        std::fs::create_dir_all(&store).unwrap();

        let upstream = format!("path:{}", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Core)]);
        let spec = classify_in("hello@mine", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };
        // Dispatch must select the core provider, and it must materialize the
        // tree into the store with a real bin dir — no nix involved.
        assert_eq!(
            resolve_kind(&spec, &table, false, &store),
            ProviderKind::Core
        );
        let r = realize(&spec, &table, &ctx).unwrap();
        assert_eq!(r.name, "hello");
        assert!(std::path::Path::new(&r.bin).join("hello").is_file());
        let shared = ProviderFacts::from_json(
            r.producer
                .facts
                .get("provider-facts")
                .expect("production provider embeds shared facts"),
        )
        .expect("production provider emits provider-facts JSON");
        shared
            .validate()
            .expect("production provider facts are lossless");
        assert_eq!(shared.reference, r.reference);
        assert_eq!(shared.resolved_source, r.producer.immutable_source);
        assert!(!shared.native_document.is_empty());
        assert_eq!(
            r.producer.facts.get("provider-facts-digest"),
            Some(&shared.digest())
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn core_provider_kind_decides_path_entry() {
        // U10 Chunk 4: the repo's `package.jet` `packages:` index decides what a
        // realized `core` package puts on PATH. `executable` → a `bin/` dir;
        // `library` → no bin (staged source only). Both stage the tree.
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-core-kind");
        let repo = base.join("jet-pkgs");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            repo.join("package.jet"),
            "name: \"p\"\nversion: \"0.1.0\"\npackages: { hello: executable, mathlib: library }\n",
        )
        .unwrap();
        // executable: has a prebuilt bin/.
        let hello_bin = repo.join("pkgs/hello/bin");
        std::fs::create_dir_all(&hello_bin).unwrap();
        std::fs::write(repo.join("pkgs/hello/hello.jet"), "module hello { }\n").unwrap();
        std::fs::write(hello_bin.join("hello"), "#!/bin/sh\necho hi\n").unwrap();
        // library: module source, no bin/.
        let lib = repo.join("lib/mathlib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("mathlib.jet"), "module mathlib { }\n").unwrap();

        let upstream = format!("path:{}", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Core)]);
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };

        let exe = realize(&classify_in("hello@mine", &table).unwrap(), &table, &ctx).unwrap();
        assert!(
            !exe.bin.is_empty() && std::path::Path::new(&exe.bin).join("hello").is_file(),
            "executable must stage a bin/ on PATH: {exe:?}"
        );

        let lib = realize(&classify_in("mathlib@mine", &table).unwrap(), &table, &ctx).unwrap();
        assert!(
            lib.bin.is_empty(),
            "library must contribute no PATH entry: {lib:?}"
        );
        assert!(
            std::path::Path::new(&lib.out).join("mathlib.jet").is_file(),
            "library must stage its module source: {lib:?}"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn core_provider_fetches_remote_git_package() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("note: skipping remote core provider test (git not found)");
            return;
        }

        let base = unique_dir("jpk-core-remote");
        let repo = base.join("remote");
        let store = base.join("store");
        let hello_pkg = repo.join("pkgs/hello");
        let hello_bin = hello_pkg.join("bin");
        std::fs::create_dir_all(&hello_bin).unwrap();
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
        std::fs::write(hello_bin.join("hello"), "#!/bin/sh\necho remote\n").unwrap();

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo)
            .output()
            .unwrap();
        for (k, v) in [
            ("user.email", "jetpack@example.invalid"),
            ("user.name", "Jet Test"),
        ] {
            std::process::Command::new("git")
                .args(["config", k, v])
                .current_dir(&repo)
                .output()
                .unwrap();
        }
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo)
            .output()
            .unwrap();
        let commit = std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            commit.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        );

        let upstream = format!("file://{}#HEAD", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Core)]);
        let spec = classify_in("hello@mine", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };

        let r = realize(&spec, &table, &ctx).unwrap();
        assert_eq!(r.name, "hello");
        assert!(std::path::Path::new(&r.bin).join("hello").is_file());
        std::fs::remove_dir_all(&base).ok();
    }

    /// Init a git repo at `dir` with the given files and one commit. Returns
    /// false (skip) if `git` isn't available.
    fn init_git_repo(dir: &Path, files: &[(&str, &str)]) -> bool {
        if Command::new("git").arg("--version").output().is_err() {
            return false;
        }
        for (rel, body) in files {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap()
        };
        run(&["init", "--quiet"]);
        run(&["config", "user.email", "jetpack@example.invalid"]);
        run(&["config", "user.name", "Jet Test"]);
        run(&["add", "."]);
        let commit = run(&["commit", "--quiet", "-m", "init"]);
        assert!(
            commit.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        );
        true
    }

    #[test]
    fn resolve_kind_probes_remote_pack_jet() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-probe");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();

        // A repo carrying `package.jet` is a Jet package source → core.
        let with = base.join("with-pack");
        if !init_git_repo(
            &with,
            &[("package.jet", "name: \"p\"\nversion: \"0.1.0\"\n")],
        ) {
            eprintln!("note: skipping remote probe test (git not found)");
            return;
        }
        let with_table = SourceTable::from_decls([(
            "mine".to_string(),
            format!("file://{}", with.to_string_lossy()),
            ProviderKind::Infer,
        )]);
        let with_spec = classify_in("hello@mine", &with_table).unwrap();
        assert_eq!(
            resolve_kind(&with_spec, &with_table, false, &store),
            ProviderKind::Core,
            "a remote carrying package.jet must infer core"
        );

        // A repo with no `package.jet` is a plain (nix) flake/source → nix.
        let without = base.join("no-pack");
        init_git_repo(&without, &[("flake.nix", "{}\n")]);
        let without_table = SourceTable::from_decls([(
            "plain".to_string(),
            format!("file://{}", without.to_string_lossy()),
            ProviderKind::Infer,
        )]);
        let without_spec = classify_in("fd@plain", &without_table).unwrap();
        assert_eq!(
            resolve_kind(&without_spec, &without_table, false, &store),
            ProviderKind::Nix,
            "a remote with no package.jet must infer nix"
        );

        // Offline with no cached checkout can't probe → defaults to nix even for
        // the package.jet-bearing repo.
        let cold = base.join("cold-store");
        std::fs::create_dir_all(&cold).unwrap();
        assert_eq!(
            resolve_kind(&with_spec, &with_table, true, &cold),
            ProviderKind::Nix,
            "offline with no cache must not hit the network"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn remote_probe_resolves_a_commit_sha_rev() {
        // The uniform `git fetch <rev>` peek must resolve a source pinned to an
        // exact commit SHA the same as a branch/tag name (the case the earlier
        // `--branch`-only peek could not handle).
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-probe-sha");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        let repo = base.join("repo");
        if !init_git_repo(
            &repo,
            &[("package.jet", "name: \"p\"\nversion: \"0.1.0\"\n")],
        ) {
            eprintln!("note: skipping commit-sha probe test (git not found)");
            return;
        }
        let sha = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        let upstream = format!("file://{}#{}", repo.to_string_lossy(), sha);
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Infer)]);
        let spec = classify_in("hello@mine", &table).unwrap();
        assert_eq!(
            resolve_kind(&spec, &table, false, &store),
            ProviderKind::Core,
            "a commit-SHA-pinned remote with package.jet must infer core"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn realize_resolves_inferred_remote_to_core() {
        // U9 end-to-end at the realize boundary: an `Infer` source — the kind a
        // typed `…@github` source carries — whose remote has a `package.jet`
        // resolves to the `core` provider and builds the first-party package,
        // with no nix and no declared marker.
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-infer-build");
        let repo = base.join("remote");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        if !init_git_repo(
            &repo,
            &[
                ("package.jet", "name: \"p\"\nversion: \"0.1.0\"\n"),
                ("pkgs/hello/hello.jet", "module hello { }\n"),
                ("pkgs/hello/bin/hello", "#!/bin/sh\necho hi-infer\n"),
            ],
        ) {
            eprintln!("note: skipping inferred remote build test (git not found)");
            return;
        }
        let upstream = format!("file://{}", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Infer)]);
        let spec = classify_in("hello@mine", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };
        let r = realize(&spec, &table, &ctx).unwrap();
        assert_eq!(r.name, "hello");
        assert!(std::path::Path::new(&r.bin).join("hello").is_file());
        std::fs::remove_dir_all(&base).ok();
    }

    // ── Slice C: index-first sparse monorepo fetch (D-MONOREF1) ──

    #[test]
    fn sparse_fetch_materializes_only_addressed_member() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-sparse-one");
        let repo = base.join("mono");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        if !init_git_repo(
            &repo,
            &[
                (
                    "workspace.jet",
                    "module workspace { members: find(\"./packages\") }\n",
                ),
                (
                    "packages/hello/package.jet",
                    "name: \"hello\"\nversion: \"0.1.0\"\n",
                ),
                ("packages/hello/hello.jet", "module hello { }\n"),
                ("packages/hello/bin/hello", "#!/bin/sh\necho hi\n"),
                (
                    "packages/world/package.jet",
                    "name: \"world\"\nversion: \"0.1.0\"\n",
                ),
                ("packages/world/world.jet", "module world { }\n"),
            ],
        ) {
            eprintln!("note: skipping sparse fetch test (git not found)");
            return;
        }
        let upstream = format!("file://{}", repo.to_string_lossy());
        let table =
            SourceTable::from_decls([("mine".to_string(), upstream.clone(), ProviderKind::Core)]);
        let spec = classify_in("hello@mine", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };
        let r = realize(&spec, &table, &ctx).unwrap();
        assert_eq!(r.name, "hello");

        // The source-cache checkout has ONLY the addressed member's subtree.
        let remote = parse_remote_source(&upstream).unwrap();
        let cache = source_cache_dir(&store, &remote);
        assert!(
            cache.join("packages/hello/package.jet").is_file(),
            "addressed member must be checked out: {}",
            cache.display()
        );
        assert!(
            !cache.join("packages/world").exists(),
            "unaddressed member `world` must NOT be materialized (sparse): {}",
            cache.display()
        );
        // Root files are always present in cone mode.
        assert!(cache.join("workspace.jet").is_file());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn sparse_fetch_includes_in_repo_dependency() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-sparse-dep");
        let repo = base.join("mono");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        if !init_git_repo(
            &repo,
            &[
                (
                    "workspace.jet",
                    "module workspace { members: find(\"./packages\") }\n",
                ),
                // `app` depends on the in-repo `logging` member via a path ref
                // whose alias (`log`) differs from the member name — exercises
                // path-target resolution, not just name matching.
                (
                    "packages/app/package.jet",
                    "name: \"app\"\nversion: \"0.1.0\"\ndeps: { log: ../logging }\n",
                ),
                ("packages/app/app.jet", "module app { }\n"),
                (
                    "packages/logging/package.jet",
                    "name: \"logging\"\nversion: \"0.1.0\"\n",
                ),
                ("packages/logging/logging.jet", "module logging { }\n"),
                (
                    "packages/unrelated/package.jet",
                    "name: \"unrelated\"\nversion: \"0.1.0\"\n",
                ),
                ("packages/unrelated/unrelated.jet", "module unrelated { }\n"),
            ],
        ) {
            eprintln!("note: skipping sparse dep test (git not found)");
            return;
        }
        let upstream = format!("file://{}", repo.to_string_lossy());
        let table =
            SourceTable::from_decls([("mine".to_string(), upstream.clone(), ProviderKind::Core)]);
        let spec = classify_in("app@mine", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };
        realize(&spec, &table, &ctx).unwrap();

        let remote = parse_remote_source(&upstream).unwrap();
        let cache = source_cache_dir(&store, &remote);
        assert!(
            cache.join("packages/app/package.jet").is_file(),
            "app subtree"
        );
        assert!(
            cache.join("packages/logging/package.jet").is_file(),
            "in-repo dependency `logging` must be pulled into the sparse checkout: {}",
            cache.display()
        );
        assert!(
            !cache.join("packages/unrelated").exists(),
            "an unrelated member must stay out of the sparse checkout: {}",
            cache.display()
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn in_repo_dep_outside_workspace_is_e1233() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-e1233");
        let repo = base.join("mono");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        if !init_git_repo(
            &repo,
            &[
                (
                    "workspace.jet",
                    "module workspace { members: find(\"./packages\") }\n",
                ),
                // `app` depends on `packages/ghost`, a real repo directory that
                // is NOT a workspace member (no package.jet of its own).
                (
                    "packages/app/package.jet",
                    "name: \"app\"\nversion: \"0.1.0\"\ndeps: { ghost: ../ghost }\n",
                ),
                ("packages/app/app.jet", "module app { }\n"),
                ("packages/ghost/notes.txt", "not a package\n"),
            ],
        ) {
            eprintln!("note: skipping E1233 test (git not found)");
            return;
        }
        let upstream = format!("file://{}", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Core)]);
        let spec = classify_in("app@mine", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };
        match realize(&spec, &table, &ctx) {
            Err(e) => assert_eq!(e.code(), Some("E1233"), "expected E1233, got {e:?}"),
            Ok(r) => panic!("expected E1233, but realize succeeded: {r:?}"),
        }
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn canonical_monorepo_realization_uses_the_requested_member_root() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-canonical-mono");
        let repo = base.join("mono");
        let store = base.join("store");
        let source = repo.join("packages/hello/src");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(
            repo.join("package.jet"),
            "name: \"workspace\"\nmembers: find(\"./packages\")\n",
        )
        .unwrap();
        std::fs::write(
            repo.join("packages/hello/package.jet"),
            "name: \"hello\"\nversion: \"0.1.0\"\nsource: \"src\"\noutputs: .{ hello: .Executable.{ entry: run } }\ndefaults: .{ run: hello }\n",
        )
        .unwrap();
        std::fs::write(source.join("main.jet"), "fn run() { print(\"hello\") }\n").unwrap();
        std::fs::create_dir_all(repo.join("stray")).unwrap();
        std::fs::write(
            repo.join("stray/package.jet"),
            "name: \"stray\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();

        let (root, facts) = find_canonical_package(&repo, "hello")
            .unwrap()
            .expect("member Package must be discoverable");
        assert_eq!(root, repo.join("packages/hello"));
        assert_eq!(facts.name, "hello");
        assert!(find_canonical_package(&repo, "stray").is_err());

        let upstream = format!("path:{}", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Core)]);
        let spec = classify_in("hello@mine", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: true,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };
        let realized = realize(&spec, &table, &ctx).unwrap();
        let output = Path::new(&realized.out);
        assert!(output.join("main.jet").is_file());
        assert!(
            !output.join("package.jet").is_file(),
            "source root is the member source dir"
        );
        assert_eq!(realized.version, "0.1.0");

        let before = cache_expectation(&spec, &table, &ctx).expect("canonical cache identity");
        assert_eq!(before.identity, realized.cache_identity);
        std::fs::write(source.join("extra.jet"), "fn extra() {}\n").unwrap();
        let after = cache_expectation(&spec, &table, &ctx).expect("changed cache identity");
        assert_ne!(
            before.identity.source_fingerprint,
            after.identity.source_fingerprint
        );
        assert_ne!(before.owned_output, after.owned_output);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn core_build_writes_envelope() {
        // T0 (D-JPK-CACHE1=A): realizing a first-party library package produces
        // a hangar object whose record carries the full A4 envelope
        // (output_hash, platform, signature slot, provenance) — not just a
        // fingerprint. The envelope round-trips through the store record.
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        use super::super::Store;
        let base = unique_dir("jpk-envelope");
        let repo = base.join("jet-pkgs");
        let store = base.join("hangar");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            repo.join("package.jet"),
            "name: \"p\"\nversion: \"0.1.0\"\npackages: { mathlib: library }\n",
        )
        .unwrap();
        let lib = repo.join("lib/mathlib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("mathlib.jet"), "module mathlib { }\n").unwrap();

        let upstream = format!("path:{}", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Core)]);
        let spec = classify_in("mathlib@mine", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
            project_dir: None,
            nix_index: None,
            nix_roots: None,
        };
        let r = realize(&spec, &table, &ctx).unwrap();
        // The realized output carries a complete envelope.
        assert!(!r.envelope.is_empty(), "envelope must be populated: {r:?}");
        assert!(
            r.envelope.output_hash.starts_with("sha256-"),
            "output_hash must be a content hash: {:?}",
            r.envelope
        );
        assert!(!r.envelope.platform.is_empty(), "platform must be set");
        assert!(
            r.envelope.provenance.contains("mathlib@mine"),
            "provenance names the source ref: {:?}",
            r.envelope
        );
        assert!(
            r.envelope.signature.is_empty(),
            "signature slot stays empty until package signing (#13)"
        );

        // Persisting and re-reading the record keeps the envelope intact.
        let roots = Store::Roots {
            root: base.clone(),
            dev_mode: true,
        };
        let entry = Store::record_realized_mode(&roots, &r).unwrap();
        assert_eq!(entry.envelope, r.envelope);
        let listed = Store::list(&roots);
        let found = listed.iter().find(|e| e.id == entry.id).unwrap();
        assert_eq!(found.envelope.output_hash, r.envelope.output_hash);
        assert_eq!(found.envelope.platform, r.envelope.platform);
        assert_eq!(found.envelope.provenance, r.envelope.provenance);
        assert_eq!(
            Store::ProducerRecord::decode(&found.producer_record)
                .unwrap()
                .provider,
            "core"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn bridge_build_rejects_untrusted_toolchain_outside_native_closure() {
        // D-JPK-SANDBOX2=D: a pinned-looking cargo path is not enough to permit
        // an executable build. The Linux substrate accepts only tools from the
        // immutable closure, so a fixture outside `/nix/store` fails before its
        // shim can create an rlib.
        use super::super::Toolchain::Toolchain;
        use std::collections::HashMap;
        use std::os::unix::fs::PermissionsExt;
        let base = unique_dir("jpk-bridge");
        let tc_dir = base.join("toolchain");
        std::fs::create_dir_all(&tc_dir).unwrap();
        let cargo = tc_dir.join("cargo");
        std::fs::write(
            &cargo,
            "#!/bin/sh\nmkdir -p \"$CARGO_TARGET_DIR/release\"\n\
             printf 'PINNED-RLIB-BYTES' > \"$CARGO_TARGET_DIR/release/libmath.rlib\"\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
        let tc = Toolchain {
            cargo: cargo.clone(),
            id: "toolchain-test".to_string(),
            version: "9.9.9".to_string(),
            pinned: true,
            ring_artifacts: HashMap::new(),
        };

        let hangar = base.join("hangar");
        std::fs::create_dir_all(&hangar).unwrap();

        let pkg = base.join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("Cargo.toml"), "[package]\nname=\"math\"\n").unwrap();
        let error = build_rlib_from_cargo(&pkg, &hangar, &tc)
            .expect_err("a tool outside the immutable closure must not run");
        assert!(
            error.contains("outside the immutable tool closure")
                || error.contains("native sandbox"),
            "unexpected refusal: {error}"
        );
        assert!(!pkg.join("libmath.rlib").exists());
        assert!(
            std::fs::read_dir(hangar.join(BUILD_SCRATCH_DIR))
                .map(|entries| entries.flatten().next().is_none())
                .unwrap_or(true),
            "build scratch must be swept after refusal"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn tree_fingerprint_reflects_contents() {
        // Distinct package trees must hash differently (no store collisions);
        // identical trees must hash the same.
        let base = unique_dir("jpk-fp");
        let a = base.join("a");
        let b = base.join("b");
        let c = base.join("c");
        for (d, body) in [(&a, "one"), (&b, "two"), (&c, "one")] {
            std::fs::create_dir_all(d.join("bin")).unwrap();
            std::fs::write(d.join("bin/x"), body).unwrap();
        }
        assert_ne!(tree_fingerprint(&a), tree_fingerprint(&b));
        assert_eq!(tree_fingerprint(&a), tree_fingerprint(&c));
        std::fs::remove_dir_all(&base).ok();
    }

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p =
            std::env::temp_dir().join(format!("{tag}-{nanos}-{:?}", std::thread::current().id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
