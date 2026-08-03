//! Provider translation layer (D-JPK5).
//!
//! Jetpack owns the package lifecycle. Nix is a *compatibility provider*: we
//! translate a Jetpack ref into a flake ref, ask Nix to realize it, parse the
//! store path it prints, and turn that into a `bin` directory for PATH. The
//! native Jetpack builder can later sit beside this same `Realized` boundary.
//!
//! Determinism for tests: when a fixtures dir is supplied (the `--offline`
//! path, or `JETPACK_FIXTURES`), we read a canned `nix build --json` file
//! instead of shelling out — exactly the Forge fixture pattern.

use jet_env_model::ModuleEval::{AdapterPlan, AdapterRecipe};
use super::PackageManifest;
use super::Recipe::{self, BuildContext, BuildRecipe, BuildStep};
use super::RefSpec::{ProviderKind, RefSpec, Source, SourceTable};
use super::JSON;
use crate::SHA256;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

mod remote;
mod package;
mod cran;
mod fetch;
mod luarocks;
mod script_registry;
use cran::CranProvider;
use luarocks::LuaRocksProvider;
use script_registry::{Kind as ScriptRegistryKind, ScriptRegistryProvider};

use remote::{
    copy_tree, fetch_remote_repo, infer_package_kind, parse_remote_source, source_cache_dir,
    source_repo, tree_fingerprint, RemoteSource,
};
use package::{
    canonical_package_kind, canonical_source_dir, core_recipe_identity, core_tree_fingerprint,
    find_canonical_package, toolchain_facts, validate_core_source_tree,
};
#[cfg(test)]
use remote::file_has_top_level_run;

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
) -> String {
    if matches!(&plan.recipe, AdapterRecipe::Build(_)) {
        recipe.build_identity_for_source(&plan.name, &plan.source, source_digest, platform)
    } else {
        recipe.build_identity(&plan.name, source_digest, platform)
    }
}

pub(crate) fn adapter_cache_identity(
    source_digest: &str,
    action_identity: &str,
    ctx: &Ctx,
) -> super::Store::CacheIdentity {
    cache_identity(
        source_digest,
        &format!("adapter-v1:{action_identity}"),
        ctx,
    )
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

pub fn validate_cache_authority(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
) -> Result<(), ProviderError> {
    let Some(project) = ctx.project_dir else { return Ok(()); };
    match resolve_kind(spec, table, ctx.offline, ctx.store_dir) {
        ProviderKind::Cran => {
            let Some((_, _, repository, locked, _)) = super::Lock::cran_realization(project, &spec.raw) else { return Ok(()); };
            let current = cran::cache_authority(ctx)?;
            ensure_locked_authority("CRAN", &repository, &locked, &current)
        }
        ProviderKind::LuaRocks => {
            let Some((_, _, repository, locked, _)) = super::Lock::luarocks_realization(project, &spec.raw) else { return Ok(()); };
            let current = luarocks::cache_authority(ctx)?;
            ensure_locked_authority("LuaRocks", &repository, &locked, &current)
        }
        ProviderKind::RubyGems | ProviderKind::Cpan | ProviderKind::Packagist => {
            let kind = script_registry_kind(resolve_kind(spec, table, ctx.offline, ctx.store_dir))
                .ok_or_else(|| ProviderError::Registry("script registry", "unknown provider".into()))?;
            let Some((_, _, repository, locked, _)) = super::Lock::registry_realization(project, kind.label(), &spec.raw) else { return Ok(()); };
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
                "locked provider authority does not match current policy.providers (locked repository `{repository}`, current `{}`)",
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
                .or_else(|| PackageManifest::discover_module_in(&repo, &spec.package).ok())?;
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
                let manifest = match PackageManifest::PackManifest::load(&repo) {
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
                owned_output: Some(
                    ctx.store_dir
                        .join(format!("{}-{}", spec.package, &source_fingerprint[..12])),
                ),
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
                identity: super::Store::CacheIdentity {
                    source_fingerprint: env.output_hash.clone(),
                    recipe_fingerprint: SHA256::sha256_hex(NIX_RECIPE_ID.as_bytes()),
                    policy_fingerprint: super::RuntimePolicy::cache_policy_fingerprint(
                        ctx.offline,
                    ),
                    platform,
                },
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
                    platform: if env.platform.is_empty() { super::Envelope::host_platform() } else { env.platform.clone() },
                    ..provider_cache_identity(&source_hash, cran::RECIPE_ID, ctx, &authority.provenance())
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
                    platform: if env.platform.is_empty() { super::Envelope::host_platform() } else { env.platform.clone() },
                    ..provider_cache_identity(&source_hash, luarocks::RECIPE_ID, ctx, &authority.provenance())
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
                    platform: if env.platform.is_empty() { super::Envelope::host_platform() } else { env.platform.clone() },
                    ..provider_cache_identity(&source_hash, kind.recipe(), ctx, &authority.provenance())
                },
                owned_output: Some(PathBuf::from(output)),
                allow_unsigned_local: true,
            })
        }
        // An inferred source realized offline defaults to nix with no lock-backed
        // identity to match; no early cache path.
        ProviderKind::Infer => None,
    }
}

/// Derive the adapter cache identity without trusting an existing output.
/// Staging reads the declared source; the output path follows only from those
/// bytes plus the normalized recipe.
pub fn adapter_cache_expectation(
    plan: &AdapterPlan,
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
        Source::Named(name) => table.upstream(name).unwrap_or(name),
        _ => spec.source.label(),
    };
    (
        normalize_nix_identity(source),
        normalize_nix_identity(&flake_ref(spec, table)),
        normalize_nix_identity(spec.source.label()),
        normalize_nix_identity(&spec.package),
    )
}

pub(crate) fn project_lock_digest(project: Option<&Path>) -> Result<String, ProviderError> {
    let Some(project) = project.filter(|path| path.is_dir()) else {
        return Ok(String::new());
    };
    let path = super::Store::lock_path(project);
    match std::fs::read(&path) {
        Ok(raw) => Ok(SHA256::sha256_hex(&raw)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(ProviderError::BadOutput(format!(
            "could not read project lock `{}`: {error}",
            path.display()
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

fn primary_nix_output_digest(
    named_output_digests: &BTreeMap<String, String>,
) -> Option<String> {
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
            ProviderError::BadOutput(format!(
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
                return Err(ProviderError::BadOutput(format!(
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
    let cache_identity = super::Store::CacheIdentity {
        source_fingerprint: envelope.output_hash.clone(),
        recipe_fingerprint: SHA256::sha256_hex(NIX_RECIPE_ID.as_bytes()),
        policy_fingerprint: super::RuntimePolicy::cache_policy_fingerprint(ctx.offline),
        platform: envelope.platform.clone(),
    };
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
        ("nix.identity.source".into(), identity.normalized_source.clone()),
        ("nix.identity.node".into(), identity.normalized_node.clone()),
        ("nix.identity.alias".into(), identity.normalized_alias.clone()),
        ("nix.identity.query".into(), identity.normalized_query.clone()),
        ("nix.lock.digest".into(), identity.lock_digest.clone()),
        ("nix.envelope.digest".into(), identity.envelope_digest.clone()),
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
    for (name, digest) in &identity.named_output_digests {
        facts.insert(format!("nix.output.{name}.digest"), digest.clone());
    }
    facts
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
    let current_lock_digest = project_lock_digest(Some(project))?;
    if &current_lock_digest != expected_lock_digest {
        return Err(ProviderError::BadOutput(format!(
            "Nix project lock changed after Store registration: prepared `{expected_lock_digest}`, current `{current_lock_digest}`"
        )));
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
        },
    )
    .map_err(ProviderError::BadOutput)?;
    let lock_digest = project_lock_digest(Some(project))?;
    super::Store::refresh_nix_lock_digest(roots, entry, &lock_digest)
        .map_err(|error| ProviderError::BadOutput(format!(
            "could not refresh the Nix Store producer after lock publication: {error}"
        )))
}

/// How a dependency was realized, for the `jet build` per-package report
/// (`built | substituted | cached`, mirroring the D-JPK-CACHE1 example output).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceState {
    /// Compiled from source by the first-party core provider this run.
    Built,
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
            SourceState::Cached => "cached",
            SourceState::Substituted => "substituted",
        }
    }
}

/// What can go wrong realizing a ref through a provider. Each maps to a
/// friendly diagnostic (see `report`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// The `nix` binary isn't installed / on PATH and this source needs it.
    NixMissing,
    /// `nix build` ran but failed; carries a trimmed reason.
    BuildFailed(String),
    /// The provider's JSON didn't have the shape we expected.
    BadOutput(String),
    /// Offline/fixture mode but no fixture file for this ref.
    FixtureMissing(PathBuf),
    /// The selected provider can't realize this ref yet.
    Unsupported(String),
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
    /// E1271: a source channel cannot be resolved or is unlocked in a context
    /// that may not resolve it.
    Channel(String),
    /// E1276: `--offline` forbids a network fetch or metadata refresh.
    Offline(String),
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
            ProviderError::Channel(_) => Some("E1271"),
            ProviderError::BuildDebug(_) => Some("E1273"),
            ProviderError::Offline(_) => Some("E1276"),
            ProviderError::Cran(_) => None,
            ProviderError::LuaRocks(_) => None,
            ProviderError::Registry(_, _) => None,
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
}

/// D-JPK-OFFLINE2=B: the stable recipe id for a Nix-provider realization. Hashed
/// into the cache identity's `recipe_fingerprint`, recomputed offline from this
/// constant so a lock-backed reuse reproduces it without any Nix/network call.
pub(crate) const NIX_RECIPE_ID: &str = "nix-compat-v1";

/// Translate a Jetpack ref into the provider's flake ref. Users never type
/// `#`; this is the single place `:` becomes the Nix selector. A named source
/// (D-JPK17) resolves through `table` to its upstream/pin, then selects the
/// package as a flake attr: `<upstream>#<package>`.
pub fn flake_ref(spec: &RefSpec, table: &SourceTable) -> String {
    match &spec.source {
        Source::Nixpkgs => format!("nixpkgs#{}", nix_package_name(&spec.package)),
        Source::Github => format!("github:{}", spec.package),
        Source::Path => format!("path:{}", spec.package),
        Source::Cran => format!("cran:{}", spec.package),
        Source::LuaRocks => format!("luarocks:{}", spec.package),
        Source::RubyGems => format!("ruby:{}", spec.package),
        Source::Cpan => format!("perl:{}", spec.package),
        Source::Packagist => format!("php:{}", spec.package),
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
    package.split_once("#version=").map_or(package, |(name, _)| name)
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

/// Whether the `nix` binary is reachable on PATH (U16). Used by the two call
/// sites that shell out to `nix` for something other than a package ref —
/// `jet env`'s foreign-flake/devenv fallback and `jet bridge flake` — so both
/// fail with a clean E1256 up front instead of a raw spawn error partway
/// through.
pub fn nix_on_path() -> bool {
    Command::new("nix")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
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

/// The Nix compatibility provider: translates a ref to a flake ref and shells
/// out to `nix build --no-link --json` (R3 will remove the installed-`nix`
/// requirement; the boundary here does not change).
pub(crate) struct NixProvider;

impl Provider for NixProvider {
    fn realize(
        &self,
        spec: &RefSpec,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Realized, ProviderError> {
        let stdout = match ctx.fixtures {
            Some(dir) => {
                let path = dir.join(fixture_name(spec));
                std::fs::read_to_string(&path).map_err(|_| ProviderError::FixtureMissing(path))?
            }
            None if ctx.offline => {
                return Err(ProviderError::Offline(format!(
                    "`{}` is not in the hangar and --offline forbids fetching provider output",
                    spec.raw
                )))
            }
            None => run_nix(spec, table)?,
        };
        let mut realized = parse_realization(spec, &stdout)?;
        let identity = prepare_nix_identity(spec, table, ctx, &realized)?;
        realized.cache_identity = identity.cache_identity.clone();
        let previous = realized.producer;
        let mut facts = previous.facts;
        facts.extend(prepared_nix_facts(&identity));
        realized.producer = super::Store::ProducerRecord::new(
            previous.provider,
            previous.immutable_source,
            previous.source_digest,
            previous.plan,
            previous.toolchain_facts,
            format!(
                "policy={}\nplatform={}",
                realized.cache_identity.policy_fingerprint,
                realized.cache_identity.platform
            ),
            facts,
        )
        .map_err(ProviderError::BadOutput)?;
        Ok(realized)
    }
}

/// The first-party Jet package provider (R2/U10). Realizes a Jet package with
/// no Nix at all: it discovers the package's `module <name>` in the source repo
/// (Chunk 3), reads the repo's `pkg.jet` `packages:` index for the package's
/// kind (Chunk 4), and materializes that source tree into the Jetpack store —
/// staging a `bin/` for an `executable`, source-only for a `library`. R2
/// supports local and git-backed remote source repos.
pub(crate) struct CoreProvider;

impl Provider for CoreProvider {
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
        let canonical_package = find_canonical_package(&repo, &spec.package)
            .map_err(ProviderError::CoreBuild)?;
        let (src_dir, canonical, canonical_kind, canonical_version) =
            if let Some((package_root, facts)) = canonical_package {
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
                let source_dir = PackageManifest::discover_module_in(&repo, &spec.package)
                    .map_err(|e| match e {
                        PackageManifest::DiscoveryError::NotFound { name } => {
                            ProviderError::CoreBuild(format!(
                                "source repo at {} has no `module {name}` — add a .{} file declaring it",
                                repo.display(),
                                crate::Syntax::FILE_EXT,
                            ))
                        }
                        PackageManifest::DiscoveryError::Ambiguous { name, paths } => {
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
        let toolchain = super::Toolchain::Toolchain::resolve_for_core(ctx.offline);
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
            PackageManifest::PackManifest::load(&repo)
        } {
            None => None,
            Some(Ok(manifest)) => Some(manifest),
            Some(Err(error)) => {
                return Err(ProviderError::CoreBuild(format!(
                    "package manifest {} is invalid: {error:?}",
                    PackageManifest::PackManifest::path_in(&repo).display()
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
            .map(|pm| pm.package.version.clone())
            .unwrap_or_default()
        };
        let (bin, rlib, recipe_id) = match kind {
            PackageManifest::PackageKind::Executable => (
                out_dir.join("bin").to_string_lossy().into_owned(),
                String::new(),
                "core-source",
            ),
            PackageManifest::PackageKind::Library => {
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
                    let rlib = build_rlib_from_cargo_mode(
                        &out_dir,
                        ctx.store_dir,
                        toolchain,
                        ctx.offline,
                    )
                        .map_err(ProviderError::CoreBuild)?;
                    (String::new(), rlib, "core-cargo-rlib")
                } else {
                    (String::new(), String::new(), "core-source")
                }
            }
        };
        super::Store::seal_local_output(&out_dir).map_err(|error| {
            ProviderError::CoreBuild(format!("could not seal package output: {error}"))
        })?;
        let out = out_dir.to_string_lossy().into_owned();
        let envelope = super::Envelope::Envelope::for_output(&out, &spec.raw, recipe_id);
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
        let producer = producer_record(
            "core",
            &format!("cas:{source_fingerprint}"),
            &source_fingerprint,
            BTreeMap::from([
                ("action.kind".into(), "core-build".into()),
                ("action.recipe".into(), recipe_identity.clone()),
            ]),
            &toolchain_facts(toolchain.as_ref()),
            &identity,
            BTreeMap::from([
                ("source.kind".into(), "core-package-tree".into()),
                ("source.tree_schema".into(), "jet-core-source-tree-v2".into()),
                ("source.tree_fingerprint".into(), fp.clone()),
                ("artifact.kind".into(), recipe_id.to_string()),
                (
                    "execution.platform".into(),
                    super::Envelope::host_platform(),
                ),
            ]),
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
    let Some(pid) = pid else { return false; };
    if pid == std::process::id() {
        return true;
    }
    #[cfg(unix)]
    if Path::new("/proc").join(pid.to_string()).exists() {
        return true;
    }
    // Platforms without a process table still get a conservative grace
    // period. A malformed or very old marker is safe to reclaim.
    let Some(started) = started else { return false; };
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
        if key.is_empty()
            || key.contains(std::path::MAIN_SEPARATOR)
            || key == "."
            || key == ".."
        {
            return Err("cargo scratch key is not a safe single path component".to_string());
        }
        let root = hangar_dir.join(BUILD_SCRATCH_DIR);
        match std::fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(format!("build scratch root is not a directory: {}", root.display()));
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
                return Err(format!("build scratch path is not a directory: {}", path.display()));
            }
            if active_tmp_marker_is_live(&path) {
                return Err(format!("build scratch path is already active: {}", path.display()));
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
/// (cache hit). Returns the absolute path to the rlib inside the object, or an
/// error. Every failure is returned to the caller. A missing rlib is not a valid
/// library realization: silently returning an empty artifact would make the
/// package appear built while leaving the eventual failure to an unrelated
/// linker or importer.
///
/// `toolchain` is the resolved pinned/realized build toolchain
/// (D-JPK-BUILDTOOL1=A): the build execs *its* `cargo`, so a bridge's output
/// hash does not depend on whatever host `cargo` happens to be on PATH when the
/// toolchain is a pinned object.
#[cfg(test)]
pub(crate) fn build_rlib_from_cargo(
    pkg_dir: &Path,
    hangar_dir: &Path,
    toolchain: &super::Toolchain::Toolchain,
) -> Result<String, String> {
    build_rlib_from_cargo_mode(pkg_dir, hangar_dir, toolchain, false)
}

fn build_rlib_from_cargo_mode(
    pkg_dir: &Path,
    hangar_dir: &Path,
    toolchain: &super::Toolchain::Toolchain,
    offline: bool,
) -> Result<String, String> {
    if offline && !toolchain.pinned {
        return Err("offline Core builds require a pinned realized toolchain".to_string());
    }
    if offline
        && !std::fs::symlink_metadata(pkg_dir.join("Cargo.lock"))
            .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            .unwrap_or(false)
    {
        return Err("offline Core builds require a regular Cargo.lock".to_string());
    }
    // Cache hit: a previously realized object already carries its rlib.
    if let Some(existing) = find_rlib_in(pkg_dir) {
        return Ok(existing);
    }
    let cache_key = pkg_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "pkg".to_string());
    let scratch = BuildScratch::new(hangar_dir, &cache_key)?;
    let mut command = Command::new(&toolchain.cargo);
    command
        .arg("build")
        .arg("--lib")
        .arg("--release")
        .arg("--manifest-path")
        .arg(pkg_dir.join("Cargo.toml"));
    if offline {
        command.arg("--offline").arg("--locked");
    }
    let out = command
        .env("CARGO_TARGET_DIR", &scratch.path)
        .output()
        .map_err(|error| format!("could not execute pinned cargo: {error}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(format!(
            "pinned cargo failed with {}{}{}",
            out.status,
            if stderr.is_empty() { "" } else { ": " },
            if stderr.is_empty() { stdout.trim() } else { stderr.trim() }
        ));
    }
    // Find the rlib in the scratch `release/` dir and copy it into the object.
    let release = scratch.path.join("release");
    let built = std::fs::read_dir(&release)
        .map_err(|error| format!("pinned cargo produced no release directory: {error}"))?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("rlib")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("lib"))
                    .unwrap_or(false)
        })
        .ok_or_else(|| "pinned cargo produced no lib*.rlib artifact".to_string())?;
    let file_name = built
        .file_name()
        .ok_or_else(|| "pinned cargo rlib has no file name".to_string())?;
    let dest = pkg_dir.join(file_name);
    std::fs::copy(&built, &dest)
        .map_err(|error| format!("could not copy rlib into package object: {error}"))?;
    Ok(dest.to_string_lossy().into_owned())
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

/// Pick the provider for an already-resolved kind. `Core` → the first-party
/// builder; everything else → the Nix compatibility provider.
pub(crate) fn provider_for(kind: ProviderKind) -> Box<dyn Provider> {
    match kind {
        ProviderKind::Core => Box::new(CoreProvider),
        ProviderKind::Cran => Box::new(CranProvider),
        ProviderKind::LuaRocks => Box::new(LuaRocksProvider),
        ProviderKind::RubyGems => Box::new(ScriptRegistryProvider(ScriptRegistryKind::RubyGems)),
        ProviderKind::Cpan => Box::new(ScriptRegistryProvider(ScriptRegistryKind::Cpan)),
        ProviderKind::Packagist => Box::new(ScriptRegistryProvider(ScriptRegistryKind::Packagist)),
        _ => Box::new(NixProvider),
    }
}

/// Resolve a ref's concrete provider kind (`Nix`/`Core`), running the U9
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
    let Source::Named(name) = &spec.source else { return ProviderKind::Nix; };
    match table.provider(name) {
        ProviderKind::Core => ProviderKind::Core,
        ProviderKind::Cran => ProviderKind::Cran,
        ProviderKind::LuaRocks => ProviderKind::LuaRocks,
        ProviderKind::RubyGems => ProviderKind::RubyGems,
        ProviderKind::Cpan => ProviderKind::Cpan,
        ProviderKind::Packagist => ProviderKind::Packagist,
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
    matches!(resolve_kind(spec, table, offline, cache_dir), ProviderKind::Nix | ProviderKind::Infer)
}

/// U23 / D-JPK-NONIX1=A: package refs that resolve through the Nix
/// compatibility provider need the Nix bridge unless a fixture is standing in
/// for that provider. This fact is computed before spawning `nix`, so no-Nix
/// machines get one package-focused diagnostic instead of a raw spawn error.
pub fn needs_nix_bridge(
    spec: &RefSpec,
    table: &SourceTable,
    offline: bool,
    cache_dir: &Path,
) -> Option<NixBridgeNeed> {
    if uses_nix_provider(spec, table, offline, cache_dir) {
        Some(NixBridgeNeed {
            reference: spec.raw.clone(),
            package: spec.short_name().to_string(),
        })
    } else {
        None
    }
}

/// Realize a ref through its provider. The resolver entry point: it never knows
/// or cares which backend runs — that is the whole point of the boundary.
pub(crate) fn realize(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
) -> Result<Realized, ProviderError> {
    let kind = resolve_kind(spec, table, ctx.offline, ctx.store_dir);
    provider_for(kind).realize(spec, table, ctx)
}

/// U20: realize an inline `Pkg.adapt(...)` plan into the same `Realized`
/// boundary as provider-backed packages.
pub(crate) fn realize_adapter(
    plan: &AdapterPlan,
    ctx: &Ctx,
    expected: &super::Store::CacheExpectation,
) -> Result<Realized, ProviderError> {
    let source_ref = super::RefSpec::classify_provider_ref(&plan.source).map_err(|_| {
        ProviderError::Adapter(format!(
            "adapter source `{}` is not a provider ref",
            plan.source
        ))
    })?;
    let staged = stage_adapter_source(&source_ref, ctx)?;
    let recipe = adapter_recipe_to_build(&plan.recipe);
    let recipe_hash = recipe.recipe_hash();
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
        tools: std::collections::HashMap::new(),
        fetch_cache: &fetch_cache,
        offline: ctx.offline,
    };
    let mut attempt = super::BuildDebug::Attempt::new(
        &plan.name,
        &format!("adapt:{}:{}", plan.name, plan.source),
        "adapter",
        &recipe_hash,
        &source_hash,
    );
    if let Err(d) = Recipe::run_logged(&recipe, &build_ctx, None, &mut attempt) {
        attempt.preserve_scratch(ctx.store_dir, &staged, &out_dir);
        let _ = attempt.persist(ctx.store_dir);
        return Err(ProviderError::BuildDebug(format!(
                "adapter `{}` failed at step {} of {}: {} — full log: `jet logs {}`; rerun with `--shell-on-fail` to debug inside {}",
                plan.name,
                attempt.failed_step,
                attempt.steps.len(),
                d.what,
                plan.name,
                attempt.scratch_dir
            )));
    }
    let _ = attempt.persist(ctx.store_dir);
    super::Store::seal_local_output(&out_dir)
        .map_err(|error| ProviderError::Adapter(format!("could not seal adapter output: {error}")))?;
    let out = out_dir.to_string_lossy().into_owned();
    let bin_dir = out_dir.join("bin");
    let bin = if bin_dir.is_dir() {
        bin_dir.to_string_lossy().into_owned()
    } else {
        String::new()
    };
    let envelope = super::Envelope::Envelope::for_output(
        &out,
        &format!("adapt:{}:{}", plan.name, plan.source),
        &format!("adapter:{build_identity}"),
    );
    let replay = Recipe::lower_to_plan(&recipe, &plan.name, &build_ctx.tools)
        .map_err(|d| ProviderError::Adapter(d.what))?
        .replay_record()
        .map_err(ProviderError::Adapter)?;
    let producer = super::Store::ProducerRecord::new(
        "adapter",
        format!("cas:{source_fingerprint}"),
        &source_fingerprint,
        replay,
        format!(
            "declared-tools:{:?}\nbuild-identity={build_identity}\ncapabilities={}",
            build_ctx.tools,
            recipe.declared_capabilities().join(",")
        ),
        format!("policy={}\nplatform={}", identity.policy_fingerprint, identity.platform),
        BTreeMap::from([
            ("adapter.source".into(), plan.source.clone()),
            ("build.identity".into(), build_identity),
            ("build.capabilities".into(), recipe.declared_capabilities().join(",")),
        ]),
    )
    .map_err(ProviderError::Adapter)?;
    Ok(Realized {
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
    })
}

fn adapter_recipe_to_build(recipe: &AdapterRecipe) -> BuildRecipe {
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

fn stage_adapter_source(
    source: &super::RefSpec::ProviderRef,
    ctx: &Ctx,
) -> Result<PathBuf, ProviderError> {
    match source.provider {
        Source::Path => {
            let (target, _) = super::RefSpec::split_channel_ref(&source.target);
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
        Source::Nixpkgs => Err(ProviderError::Adapter(
            "`...@nixpkgs` is an index source, not source bytes; use `jetpack add <ref> --adapt` to draft a concrete adapter.".to_string(),
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
        Source::Named(_) => Err(ProviderError::Adapter(
            "adapter source must be a source ref such as `owner/repo@github` or a bare path such as `./vendor/tool`.".to_string(),
        )),
    }
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

fn run_nix(spec: &RefSpec, table: &SourceTable) -> Result<String, ProviderError> {
    ensure_network_allowed("run nix provider")?;
    let output = Command::new("nix")
        .args(["build", "--no-link", "--json"])
        .arg(flake_ref(spec, table))
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ProviderError::NixMissing)
        }
        Err(e) => return Err(ProviderError::BuildFailed(e.to_string())),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr
            .trim()
            .lines()
            .last()
            .unwrap_or("nix build failed")
            .to_string();
        return Err(ProviderError::BuildFailed(reason));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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

/// Parse `nix build --json` output: an array of build results, each with an
/// `outputs` object. `out` is canonical primary; `bin` remains a named output.
fn parse_realization(spec: &RefSpec, stdout: &str) -> Result<Realized, ProviderError> {
    let parsed = JSON::parse_lenient(stdout).map_err(ProviderError::BadOutput)?;
    let bad_output = |reason: String| ProviderError::BadOutput(parsed.diagnostic(reason));
    let arr = parsed.value.as_array().map_err(&bad_output)?;
    let first = arr
        .first()
        .ok_or_else(|| bad_output("provider produced no build results".into()))?;
    let outputs = first.get("outputs").map_err(&bad_output)?;
    let outputs = outputs.as_object().map_err(&bad_output)?;
    let drv_path = first
        .get("drvPath")
        .and_then(|value| value.as_str())
        .map_err(&bad_output)?;
    if drv_path.trim().is_empty() {
        return Err(bad_output(
            "provider output had no exact `drvPath`".into(),
        ));
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
    ]);
    let mut facts = BTreeMap::from([("nix.drv_path".into(), drv_path.to_string())]);
    for (name, path) in &named_outputs {
        replay_facts.insert(format!("nix.output.{name}"), path.clone());
        facts.insert(format!("nix.output.{name}"), path.clone());
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

    fn empty() -> SourceTable {
        SourceTable::empty()
    }

    #[test]
    fn provider_module_stays_split_by_source_ownership() {
        const MAX_MODULE_LINES: usize = 2500;
        let root = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/Provider.rs"),
        )
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
            flake_ref(&classify("fastfetch@nixpkgs").unwrap(), &empty()),
            "nixpkgs#fastfetch"
        );
        assert_eq!(
            flake_ref(&classify("o/r@github").unwrap(), &empty()),
            "github:o/r"
        );
    }

    #[test]
    fn flake_ref_strips_jet_version_selector_only_for_nix() {
        let nix = classify("rustc#version=1.80.0@nixpkgs").unwrap();
        assert_eq!(flake_ref(&nix, &empty()), "nixpkgs#rustc");
        let cran = classify("jsonlite#version=1.9.0@cran").unwrap();
        assert_eq!(flake_ref(&cran, &empty()), "cran:jsonlite#version=1.9.0");
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
    fn fixture_name_sanitizes_slashes() {
        let s = classify("halcyonomega/cfg@github").unwrap();
        assert_eq!(fixture_name(&s), "github-halcyonomega_cfg.json");
    }

    #[test]
    fn parses_good_output() {
        let spec = classify("fastfetch@nixpkgs").unwrap();
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
            r.producer.plan.facts().get("nix.output.out").map(String::as_str),
            Some("/nix/store/abc-fastfetch-2.0")
        );
    }

    /// Card #641: `nix build --json` output wrapped in the host's own
    /// store-optimise noise (hard-link ceiling hit) must still realize —
    /// this used to die with `ProviderError::BadOutput`, "likely a Jetpack
    /// bug", for any user on a large optimised store.
    #[test]
    fn tolerates_nix_store_noise_around_output() {
        let spec = classify("fastfetch@nixpkgs").unwrap();
        let stdout = "\"/nix/store/.links/1gs2lc42h68lmq8fkcwp96lhnrqcyr3zwmi75k0896nbvc3p4fpc\" has maximum number of links\n\
             [{\"drvPath\":\"/nix/store/abc-fastfetch.drv\",\"outputs\":{\"out\":\"/nix/store/abc-fastfetch-2.0\"}}]\n\
             \"/nix/store/.links/1gs2lc42h68lmq8fkcwp96lhnrqcyr3zwmi75k0896nbvc3p4fpc\" has maximum number of links\n";
        let r = parse_realization(&spec, stdout).unwrap();
        assert_eq!(r.out, "/nix/store/abc-fastfetch-2.0");
    }

    #[test]
    fn tolerates_nix_hard_link_noise_between_multiline_realization_lines() {
        let spec = classify("fastfetch@nixpkgs").unwrap();
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
        let spec = classify("fastfetch@nixpkgs").unwrap();
        let payload = r#"[{"drvPath":"/nix/store/abc-fastfetch.drv","outputs":{"out":"/nix/store/abc-fastfetch-2.0"}}]"#;
        assert!(matches!(
            parse_realization(&spec, &format!("{payload}\n{payload}\n")),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn realization_schema_error_retains_filtered_provider_noise() {
        let spec = classify("fastfetch@nixpkgs").unwrap();
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
        let spec = classify("git@nixpkgs").unwrap();
        let stdout = r#"[{"drvPath":"/nix/store/x.drv","outputs":{"out":"/nix/store/x","bin":"/nix/store/x-bin"}}]"#;
        let r = parse_realization(&spec, stdout).unwrap();
        assert_eq!(r.out, "/nix/store/x");
        assert_eq!(r.bin, "/nix/store/x-bin/bin");
        assert_eq!(r.named_outputs.len(), 2);
    }

    #[test]
    fn empty_output_is_bad() {
        let spec = classify("x@nixpkgs").unwrap();
        assert!(matches!(
            parse_realization(&spec, "[]"),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn garbage_output_is_bad() {
        let spec = classify("x@nixpkgs").unwrap();
        assert!(matches!(
            parse_realization(&spec, "not json"),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn missing_outputs_key_is_bad() {
        let spec = classify("x@nixpkgs").unwrap();
        assert!(matches!(
            parse_realization(&spec, r#"[{"drvPath":"/x.drv"}]"#),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn missing_exact_derivation_is_bad() {
        let spec = classify("x@nixpkgs").unwrap();
        assert!(matches!(
            parse_realization(
                &spec,
                r#"[{"outputs":{"out":"/nix/store/x"}}]"#
            ),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn fixture_missing_errors() {
        let spec = classify("nope@nixpkgs").unwrap();
        let dir = std::env::temp_dir();
        let ctx = Ctx {
            fixtures: Some(&dir.join("definitely-not-here-xyz")),
            store_dir: &dir,
            offline: false,
            project_dir: None,
        };
        match realize(&spec, &empty(), &ctx) {
            Err(ProviderError::FixtureMissing(_)) => {}
            other => panic!("expected FixtureMissing, got {other:?}"),
        }
    }

    #[test]
    fn core_provider_builds_local_package() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        // Repo with pkg.jet + a `module hello` declaration + bin/. No env.jet
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
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn core_provider_kind_decides_path_entry() {
        // U10 Chunk 4: the repo's `pkg.jet` `packages:` index decides what a
        // realized `core` package puts on PATH. `executable` → a `bin/` dir;
        // `library` → no bin (staged source only). Both stage the tree.
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-core-kind");
        let repo = base.join("jet-pkgs");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            repo.join("pkg.jet"),
            "payload: { name: \"p\", version: \"0.1.0\" }\npackages: { hello: executable, mathlib: library }\n",
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
            ("user.name", "Jetpack Test"),
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
        run(&["config", "user.name", "Jetpack Test"]);
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

        // A repo carrying `pkg.jet` is a Jet package source → core.
        let with = base.join("with-pack");
        if !init_git_repo(
            &with,
            &[("pkg.jet", "payload: { name: \"p\", version: \"0.1.0\" }\n")],
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
            "a remote carrying pkg.jet must infer core"
        );

        // A repo with no `pkg.jet` is a plain (nix) flake/source → nix.
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
            "a remote with no pkg.jet must infer nix"
        );

        // Offline with no cached checkout can't probe → defaults to nix even for
        // the pkg.jet-bearing repo.
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
            &[("pkg.jet", "payload: { name: \"p\", version: \"0.1.0\" }\n")],
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
            "a commit-SHA-pinned remote with pkg.jet must infer core"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn realize_resolves_inferred_remote_to_core() {
        // U9 end-to-end at the realize boundary: an `Infer` source — the kind a
        // typed `…@github` source carries — whose remote has a `pkg.jet`
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
                ("pkg.jet", "payload: { name: \"p\", version: \"0.1.0\" }\n"),
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
                    "packages/hello/pkg.jet",
                    "payload: { name: \"hello\", version: \"0.1.0\" }\n",
                ),
                ("packages/hello/hello.jet", "module hello { }\n"),
                ("packages/hello/bin/hello", "#!/bin/sh\necho hi\n"),
                (
                    "packages/world/pkg.jet",
                    "payload: { name: \"world\", version: \"0.1.0\" }\n",
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
        };
        let r = realize(&spec, &table, &ctx).unwrap();
        assert_eq!(r.name, "hello");

        // The source-cache checkout has ONLY the addressed member's subtree.
        let remote = parse_remote_source(&upstream).unwrap();
        let cache = source_cache_dir(&store, &remote);
        assert!(
            cache.join("packages/hello/pkg.jet").is_file(),
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
                    "packages/app/pkg.jet",
                    "payload: { name: \"app\", version: \"0.1.0\" }\ndeps: { log: ../logging }\n",
                ),
                ("packages/app/app.jet", "module app { }\n"),
                (
                    "packages/logging/pkg.jet",
                    "payload: { name: \"logging\", version: \"0.1.0\" }\n",
                ),
                ("packages/logging/logging.jet", "module logging { }\n"),
                (
                    "packages/unrelated/pkg.jet",
                    "payload: { name: \"unrelated\", version: \"0.1.0\" }\n",
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
        };
        realize(&spec, &table, &ctx).unwrap();

        let remote = parse_remote_source(&upstream).unwrap();
        let cache = source_cache_dir(&store, &remote);
        assert!(cache.join("packages/app/pkg.jet").is_file(), "app subtree");
        assert!(
            cache.join("packages/logging/pkg.jet").is_file(),
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
                // is NOT a workspace member (no pkg.jet of its own).
                (
                    "packages/app/pkg.jet",
                    "payload: { name: \"app\", version: \"0.1.0\" }\ndeps: { ghost: ../ghost }\n",
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
        };
        let realized = realize(&spec, &table, &ctx).unwrap();
        let output = Path::new(&realized.out);
        assert!(output.join("main.jet").is_file());
        assert!(!output.join("package.jet").is_file(), "source root is the member source dir");
        assert_eq!(realized.version, "0.1.0");

        let before = cache_expectation(&spec, &table, &ctx).expect("canonical cache identity");
        assert_eq!(before.identity, realized.cache_identity);
        std::fs::write(source.join("extra.jet"), "fn extra() {}\n").unwrap();
        let after = cache_expectation(&spec, &table, &ctx).expect("changed cache identity");
        assert_ne!(before.identity.source_fingerprint, after.identity.source_fingerprint);
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
            repo.join("pkg.jet"),
            "payload: { name: \"p\", version: \"0.1.0\" }\npackages: { mathlib: library }\n",
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
    #[cfg(unix)]
    fn bridge_build_uses_pinned_toolchain() {
        // T2 (D-JPK-BUILDTOOL1=A): a bridge build execs the *pinned* toolchain's
        // cargo, not host cargo. A fixture toolchain stands in for #179's hangar
        // object; its cargo shim emits a deterministic rlib, so the output hash
        // is stable across builds regardless of host cargo. Two fresh builds
        // (no cache hit) produce byte-identical output — proof the pinned tool
        // ran, not whatever host cargo is on PATH.
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

        let build_once = |tag: &str| -> Vec<u8> {
            let pkg = base.join(tag);
            std::fs::create_dir_all(&pkg).unwrap();
            std::fs::write(pkg.join("Cargo.toml"), "[package]\nname=\"math\"\n").unwrap();
            let rlib =
                build_rlib_from_cargo(&pkg, &hangar, &tc).expect("pinned build produced rlib");
            let bytes = std::fs::read(&rlib).unwrap();
            // The rlib lands inside the object, and the scratch is swept.
            assert!(rlib.starts_with(pkg.to_string_lossy().as_ref()));
            assert!(
                !hangar.join(BUILD_SCRATCH_DIR).join(tag).exists(),
                "build scratch must be swept after the build"
            );
            bytes
        };

        let a = build_once("pkg-a");
        let b = build_once("pkg-b");
        assert_eq!(a, b"PINNED-RLIB-BYTES", "the pinned toolchain's cargo ran");
        assert_eq!(
            a, b,
            "output is stable across builds with the pinned toolchain"
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
