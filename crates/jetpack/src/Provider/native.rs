//! Native release-artifact package recipes.
//!
//! Card #2166 starts this provider with oh-my-pi. The recipe is deliberately
//! small: resolve one GitHub release, require its published SHA-256, stream the
//! asset into a Hangar-owned staging tree, and hand the normal `Realized`
//! boundary to Store. No Nix, curl, or wget participates in this path.

use super::{
    cache_identity, ensure_network_allowed, producer_record, Ctx, DownloadPlan, PlanItem,
    PlanState, Provider, ProviderError, Realized, SourceState,
};
use crate::NixIndex::NativeRecipe;
use crate::RefSpec::{RefSpec as PackageRef, Source, SourceTable};
use crate::{Envelope, JSON, SHA256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

pub(crate) const SOURCE_NAME: &str = "releases";
pub(crate) const UPSTREAM: &str = "github:can1357/oh-my-pi";
pub(crate) const PACKAGE: &str = "omp";
pub(crate) const ARTIFACT: &str = "omp-linux-x64";
pub(crate) const RECIPE_ID: &str = "jetpackage-omp-github-release-v1";
pub(crate) const API_ROOT: &str = "https://api.github.com/repos/can1357/oh-my-pi/releases";
pub(crate) const ARTIFACT_ROOT: &str = "https://github.com/can1357/oh-my-pi/releases/download";
/// Source hash carried by the current NixOS override. It is also the fallback
/// for an exact v18.0.0 pin when offline; moving releases must carry a fresh
/// GitHub asset digest instead of silently becoming TOFU.
pub(crate) const V18_SHA256: &str =
    "69065aefe916fe28a09a4a1396446f16a776b5b56af0867cb4db0f452d842851";
pub(crate) const MAX_METADATA_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleaseFacts {
    pub(crate) version: String,
    pub(crate) tag: String,
    pub(crate) url: String,
    pub(crate) sha256: String,
    pub(crate) fixture_artifact: Option<PathBuf>,
}

pub(crate) fn cache_expectation(
    spec: &PackageRef,
    table: &SourceTable,
    ctx: &Ctx,
) -> Option<crate::Store::CacheExpectation> {
    let facts = release_facts(spec, table, ctx).ok()?;
    let source = source_fingerprint(&facts);
    let output = output_path(ctx.store_dir, &facts);
    Some(crate::Store::CacheExpectation {
        identity: cache_identity(&source, RECIPE_ID, ctx),
        owned_output: Some(output),
        allow_unsigned_local: true,
    })
}

/// Return the artifact size already available in a fixture. Remote release
/// metadata does not currently carry a trusted size in the native recipe, so
/// callers keep that total unknown rather than printing a fabricated number.
pub(crate) fn download_size(
    spec: &PackageRef,
    table: &SourceTable,
    ctx: &Ctx,
) -> Result<Option<u64>, ProviderError> {
    let facts = release_facts(spec, table, ctx)?;
    facts
        .fixture_artifact
        .as_deref()
        .map(|path| {
            fs::symlink_metadata(path)
                .map(|metadata| metadata.len())
                .map_err(|error| native_error(format!("could not stat fixture artifact: {error}")))
        })
        .transpose()
}

pub(crate) fn catalog_cache_expectation(
    _spec: &PackageRef,
    recipe: &NativeRecipe,
    ctx: &Ctx,
) -> crate::Store::CacheExpectation {
    let source = recipe.canonical_json();
    crate::Store::CacheExpectation {
        identity: cache_identity(&source, CATALOG_RECIPE_ID, ctx),
        owned_output: Some(catalog_output_path(ctx.store_dir, recipe)),
        allow_unsigned_local: true,
    }
}

pub(crate) fn catalog_download_size(recipe: &NativeRecipe) -> Result<Option<u64>, ProviderError> {
    let Some(path) = recipe.url.strip_prefix("file://") else {
        return Ok(None);
    };
    fs::symlink_metadata(path)
        .map(|metadata| Some(metadata.len()))
        .map_err(|error| {
            native_catalog_error(format!(
                "could not stat native recipe artifact `{path}`: {error}"
            ))
        })
}

pub(crate) fn realize_catalog_recipe(
    spec: &PackageRef,
    recipe: &NativeRecipe,
    ctx: &Ctx,
) -> Result<Realized, ProviderError> {
    let identity = catalog_cache_expectation(spec, recipe, ctx).identity;
    let out_dir = catalog_output_path(ctx.store_dir, recipe);
    if fs::symlink_metadata(&out_dir).is_ok() {
        return Err(native_catalog_error(format!(
            "unverified existing output {}; run `jet clean` before rebuilding",
            out_dir.display()
        )));
    }
    let staging = ctx.store_dir.join(format!(
        ".jetpack-native-{}-{}-{}.partial",
        recipe.name,
        recipe.version,
        &recipe.sha256[..12]
    ));
    if fs::symlink_metadata(&staging).is_ok() {
        return Err(native_catalog_error(format!(
            "native package staging path already exists: {}",
            staging.display()
        )));
    }
    fs::create_dir_all(staging.join("bin")).map_err(|error| {
        native_catalog_error(format!("could not create Hangar staging tree: {error}"))
    })?;
    let artifact = staging.join("bin").join(&recipe.bin);
    let result = (|| {
        fetch_declared_artifact(&recipe.url, &artifact)?;
        let actual = SHA256::sha256_file_hex(&artifact).map_err(|error| {
            native_catalog_error(format!("could not hash native recipe artifact: {error}"))
        })?;
        if actual != recipe.sha256 {
            return Err(native_catalog_error(format!(
                "native recipe artifact digest mismatch: expected {}, got {actual}",
                recipe.sha256
            )));
        }
        make_executable(&artifact)?;
        crate::Store::seal_local_output(&staging).map_err(|error| {
            native_catalog_error(format!("could not seal native package output: {error}"))
        })?;
        fs::rename(&staging, &out_dir).map_err(|error| {
            native_catalog_error(format!("could not publish native package output: {error}"))
        })?;

        let out = out_dir.to_string_lossy().into_owned();
        let bin = out_dir.join("bin").to_string_lossy().into_owned();
        let envelope = Envelope::Envelope::for_output(&out, &spec.raw, CATALOG_RECIPE_ID);
        let lock_digest = super::project_lock_digest(ctx.project_dir)?;
        let plan_facts = BTreeMap::from([
            ("action.kind".into(), "native-catalog-prebuilt".into()),
            ("action.recipe".into(), CATALOG_RECIPE_ID.into()),
            ("build.sandbox".into(), "non-executing".into()),
            (
                "build.sandbox_policy".into(),
                "declared URL with pinned SHA-256".into(),
            ),
        ]);
        let facts = BTreeMap::from([
            ("source.kind".into(), "local-unofficial-catalog".into()),
            ("source.url".into(), recipe.url.clone()),
            ("source.sha256".into(), recipe.sha256.clone()),
            ("artifact.binary".into(), recipe.bin.clone()),
            ("artifact.verification".into(), "sha256".into()),
            ("nix.index.tier".into(), "local-unofficial".into()),
            ("nix.index.trust".into(), "unverified".into()),
            ("nix.index.signature-chain".into(), "none".into()),
            (
                super::NIX_NATIVE_FORMAT.into(),
                "jetpack-native-recipe-v1".into(),
            ),
            (super::NIX_NATIVE_DOCUMENT.into(), recipe.canonical_json()),
            ("nix.lock.digest".into(), lock_digest),
        ]);
        let producer = producer_record(
            "jetpackage",
            &recipe.url,
            &recipe.sha256,
            plan_facts,
            "jetpack-native-catalog-v1",
            &identity,
            facts,
        )?;
        let mut envelope = envelope;
        envelope.provenance = format!("local-unofficial-catalog:{}", recipe.url);
        Ok(Realized {
            name: recipe.name.clone(),
            version: recipe.version.clone(),
            reference: spec.raw.clone(),
            out: out.clone(),
            bin,
            rlib: String::new(),
            envelope,
            cache_identity: identity,
            source_state: SourceState::Downloaded,
            named_outputs: BTreeMap::from([("out".into(), out)]),
            references: Vec::new(),
            producer,
        })
    })();
    if result.is_err() {
        remove_staging(&staging);
    }
    result
}

const CATALOG_RECIPE_ID: &str = "jetpackage-native-catalog-prebuilt-v1";

fn catalog_output_path(store_dir: &Path, recipe: &NativeRecipe) -> PathBuf {
    store_dir.join(format!(
        "{}-{}-{}",
        recipe.name,
        recipe.version,
        &recipe.sha256[..12]
    ))
}

pub(crate) struct NativeProvider;

impl Provider for NativeProvider {
    fn cache_expectation(
        &self,
        spec: &PackageRef,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Option<crate::Store::CacheExpectation> {
        if super::cc_toolchain::is_reference(spec) {
            return super::cc_toolchain::cache_expectation(spec, ctx);
        }
        cache_expectation(spec, table, ctx)
    }

    fn plan_downloads(
        &self,
        specs: &[PackageRef],
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<DownloadPlan, ProviderError> {
        let mut plan = DownloadPlan::default();
        for spec in specs {
            if super::cc_toolchain::is_reference(spec) {
                plan.extend(super::cc_toolchain::plan_downloads(
                    std::slice::from_ref(spec),
                    ctx,
                )?);
                continue;
            }
            plan.add_item(PlanItem {
                package: spec.raw.clone(),
                state: PlanState::New,
                download_bytes: download_size(spec, table, ctx)?,
                disk_bytes: None,
            });
        }
        Ok(plan)
    }

    fn realize(
        &self,
        spec: &PackageRef,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Realized, ProviderError> {
        if super::cc_toolchain::is_reference(spec) {
            return super::cc_toolchain::realize(spec, ctx);
        }
        let facts = release_facts(spec, table, ctx)?;
        let source = source_fingerprint(&facts);
        let identity = cache_identity(&source, RECIPE_ID, ctx);
        let out_dir = output_path(ctx.store_dir, &facts);
        if fs::symlink_metadata(&out_dir).is_ok() {
            return Err(native_error(format!(
                "unverified existing output {}; run `jet clean` before rebuilding",
                out_dir.display()
            )));
        }
        let staging = ctx.store_dir.join(format!(
            ".omp-{}-{}-{}.partial",
            facts.version,
            &facts.sha256[..12],
            std::process::id()
        ));
        if fs::symlink_metadata(&staging).is_ok() {
            return Err(native_error(format!(
                "native package staging path already exists: {}",
                staging.display()
            )));
        }
        fs::create_dir_all(staging.join("bin")).map_err(|error| {
            native_error(format!("could not create Hangar staging tree: {error}"))
        })?;
        let artifact = staging.join("bin").join("omp");
        let result = (|| {
            fetch_artifact(&facts, &artifact)?;
            let actual = SHA256::sha256_file_hex(&artifact).map_err(|error| {
                native_error(format!("could not hash release artifact: {error}"))
            })?;
            if actual != facts.sha256 {
                return Err(native_error(format!(
                    "release artifact digest mismatch: expected {}, got {actual}",
                    facts.sha256
                )));
            }
            make_executable(&artifact)?;
            crate::Store::seal_local_output(&staging).map_err(|error| {
                native_error(format!("could not seal native package output: {error}"))
            })?;
            fs::rename(&staging, &out_dir).map_err(|error| {
                native_error(format!("could not publish native package output: {error}"))
            })?;

            let out = out_dir.to_string_lossy().into_owned();
            let bin = out_dir.join("bin").to_string_lossy().into_owned();
            let envelope = Envelope::Envelope::for_output(&out, &spec.raw, RECIPE_ID);
            let plan_facts = BTreeMap::from([
                ("action.kind".into(), "native-release-fetch".into()),
                ("action.recipe".into(), RECIPE_ID.into()),
                ("build.sandbox".into(), "non-executing".into()),
                (
                    "build.sandbox_policy".into(),
                    "verified upstream release digest".into(),
                ),
            ]);
            let facts_map = BTreeMap::from([
                ("source.kind".into(), "github-release".into()),
                ("source.repository".into(), UPSTREAM.into()),
                ("source.tag".into(), facts.tag.clone()),
                ("source.url".into(), facts.url.clone()),
                ("source.sha256".into(), facts.sha256.clone()),
                ("artifact.name".into(), ARTIFACT.into()),
                ("artifact.platform".into(), "x86_64-linux".into()),
                (
                    "artifact.verification".into(),
                    "github-release-sha256".into(),
                ),
            ]);
            let producer = producer_record(
                "jetpackage",
                &facts.url,
                &facts.sha256,
                plan_facts,
                "jetpackage-native-fetch-v1",
                &identity,
                facts_map,
            )?;
            Ok(Realized {
                name: PACKAGE.into(),
                version: facts.version,
                reference: spec.raw.clone(),
                out: out.clone(),
                bin,
                rlib: String::new(),
                envelope,
                cache_identity: identity,
                source_state: SourceState::Downloaded,
                named_outputs: BTreeMap::from([("out".into(), out.clone())]),
                references: Vec::new(),
                producer,
            })
        })();
        if result.is_err() {
            remove_staging(&staging);
        }
        result
    }
}

fn release_facts(
    spec: &PackageRef,
    table: &SourceTable,
    ctx: &Ctx,
) -> Result<ReleaseFacts, ProviderError> {
    let (package, selector) = spec
        .package
        .split_once('#')
        .map_or((spec.package.as_str(), None), |(name, selector)| {
            (name, Some(selector))
        });
    if package != PACKAGE {
        return Err(native_error(format!(
            "native release recipe `{SOURCE_NAME}` only knows `{PACKAGE}`, not `{package}`"
        )));
    }
    if !matches!(spec.source, Source::Releases)
        && !(matches!(&spec.source, Source::Named(name) if name == SOURCE_NAME))
    {
        return Err(native_error(format!(
            "native release recipe received unsupported source `{}`",
            spec.source.label()
        )));
    }
    let upstream = table.upstream(SOURCE_NAME).unwrap_or(UPSTREAM);
    let (base, upstream_selector) = split_upstream(upstream);
    if base != UPSTREAM {
        return Err(native_error(format!(
            "source `{SOURCE_NAME}` is pinned to `{base}`, expected `{UPSTREAM}`"
        )));
    }
    let requested_tag = selector
        .and_then(exact_tag)
        .or_else(|| upstream_selector.and_then(exact_tag));

    if let Some(fixtures) = ctx.fixtures {
        if let Some(facts) = fixture_facts(fixtures, requested_tag.clone())? {
            return Ok(facts);
        }
    }
    if let Some(tag) = requested_tag {
        if let Some(facts) = static_facts(&tag) {
            return Ok(facts);
        }
        if ctx.offline {
            return Err(ProviderError::Offline(format!(
                "release metadata for `{tag}` is not cached and --offline forbids fetching it"
            )));
        }
        return fetch_release_metadata(Some(&tag));
    }
    if ctx.offline {
        return Err(ProviderError::Offline(
            "the native release channel has no exact pin and --offline forbids release discovery"
                .into(),
        ));
    }
    fetch_release_metadata(None)
}

fn split_upstream(upstream: &str) -> (&str, Option<&str>) {
    match upstream.rsplit_once('#') {
        Some((base, selector)) => (base, Some(selector)),
        None => (upstream, None),
    }
}

fn exact_tag(selector: &str) -> Option<String> {
    if selector.is_empty()
        || matches!(selector, "latest" | "main")
        || (selector.starts_with('v') && selector.ends_with(".x"))
    {
        return None;
    }
    let tag = if selector.starts_with('v') {
        selector.to_string()
    } else {
        format!("v{selector}")
    };
    valid_tag(&tag).then_some(tag)
}

fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 128
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 128
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn static_facts(tag: &str) -> Option<ReleaseFacts> {
    (tag == "v18.0.0").then(|| ReleaseFacts {
        version: "18.0.0".into(),
        tag: tag.into(),
        url: format!("{ARTIFACT_ROOT}/{tag}/{ARTIFACT}"),
        sha256: V18_SHA256.into(),
        fixture_artifact: None,
    })
}

fn fixture_facts(
    fixtures: &Path,
    requested_tag: Option<String>,
) -> Result<Option<ReleaseFacts>, ProviderError> {
    let metadata = fixtures.join("jetpackage-omp.json");
    if !metadata.is_file() {
        return Ok(None);
    }
    let bytes = read_bounded_file(&metadata, MAX_METADATA_BYTES)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| native_error("native release fixture metadata is not UTF-8"))?;
    let value = JSON::parse(&text).map_err(native_error)?;
    let object = value
        .as_object()
        .map_err(|error| native_error(format!("native release fixture: {error}")))?;
    let tag = json_string(object, "tag")?;
    if !valid_tag(&tag) {
        return Err(native_error("native release fixture has an invalid tag"));
    }
    if requested_tag
        .as_deref()
        .is_some_and(|requested| requested != tag)
    {
        return Ok(None);
    }
    let version = json_string(object, "version")?;
    if !valid_version(&version) {
        return Err(native_error(
            "native release fixture has an invalid version",
        ));
    }
    let sha256 = json_string(object, "sha256")?;
    validate_digest(&sha256)?;
    let artifact_name = json_string(object, "artifact")?;
    let artifact = fixtures.join(&artifact_name);
    if !safe_fixture_path(fixtures, &artifact) {
        return Err(native_error(
            "native release fixture artifact escapes its fixture root",
        ));
    }
    if !artifact.is_file() {
        return Err(native_error(format!(
            "native release fixture is missing `{}`",
            artifact.display()
        )));
    }
    Ok(Some(ReleaseFacts {
        version,
        tag,
        url: format!("fixture://{artifact_name}"),
        sha256,
        fixture_artifact: Some(artifact),
    }))
}

fn fetch_release_metadata(tag: Option<&str>) -> Result<ReleaseFacts, ProviderError> {
    ensure_network_allowed("native release metadata")?;
    let url = match tag {
        Some(tag) => format!("{API_ROOT}/tags/{tag}"),
        None => format!("{API_ROOT}/latest"),
    };
    let bytes = fetch_bounded_bytes(&url, MAX_METADATA_BYTES, "native release metadata")
        .map_err(native_error)?;
    let value = JSON::parse(
        std::str::from_utf8(&bytes)
            .map_err(|_| native_error("GitHub release metadata is not UTF-8"))?,
    )
    .map_err(native_error)?;
    let object = value
        .as_object()
        .map_err(|error| native_error(format!("GitHub release metadata: {error}")))?;
    let tag = json_string(object, "tag_name")?;
    if !valid_tag(&tag) {
        return Err(native_error(
            "GitHub release metadata has an invalid tag_name",
        ));
    }
    let version = tag.strip_prefix('v').unwrap_or(&tag).to_string();
    if !valid_version(&version) {
        return Err(native_error(
            "GitHub release metadata has an invalid version",
        ));
    }
    let assets = object
        .get("assets")
        .ok_or_else(|| native_error("GitHub release metadata has no assets"))?
        .as_array()
        .map_err(|error| native_error(format!("GitHub release assets: {error}")))?;
    let asset = assets.iter().find_map(|asset| {
        let object = asset.as_object().ok()?;
        (json_string(object, "name").ok()? == ARTIFACT).then_some(object)
    });
    let asset = asset
        .ok_or_else(|| native_error(format!("GitHub release `{tag}` has no `{ARTIFACT}` asset")))?;
    let url = json_string(asset, "browser_download_url")?;
    if !url.starts_with("https://") {
        return Err(native_error("GitHub release asset URL is not HTTPS"));
    }
    let digest = match asset.get("digest") {
        Some(JSON::JSONValue::String(value)) => {
            value.strip_prefix("sha256:").unwrap_or(value).to_string()
        }
        _ => static_facts(&tag)
            .map(|facts| facts.sha256)
            .ok_or_else(|| native_error("GitHub release asset has no published SHA-256 digest"))?,
    };
    validate_digest(&digest)?;
    Ok(ReleaseFacts {
        version,
        tag,
        url,
        sha256: digest,
        fixture_artifact: None,
    })
}

fn json_string(
    object: &BTreeMap<String, JSON::JSONValue>,
    field: &str,
) -> Result<String, ProviderError> {
    match object.get(field) {
        Some(JSON::JSONValue::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(native_error(format!(
            "release metadata field `{field}` is not a string"
        ))),
    }
}

fn validate_digest(digest: &str) -> Result<(), ProviderError> {
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(native_error("native release digest is not a SHA-256 value"));
    }
    Ok(())
}

fn source_fingerprint(facts: &ReleaseFacts) -> String {
    format!("{}\ntag={}\nsha256={}", facts.url, facts.tag, facts.sha256)
}

fn output_path(store_dir: &Path, facts: &ReleaseFacts) -> PathBuf {
    store_dir.join(format!(
        "{PACKAGE}-{}-{}",
        facts.version,
        &facts.sha256[..12]
    ))
}

fn fetch_artifact(facts: &ReleaseFacts, destination: &Path) -> Result<(), ProviderError> {
    if let Some(source) = facts.fixture_artifact.as_deref() {
        return copy_bounded_file(source, destination);
    } else {
        fetch_remote_artifact(&facts.url, destination, "release artifact", |detail| {
            native_error(detail)
        })?;
    }
    validate_artifact_file(
        destination,
        |detail| native_error(detail),
        "release artifact",
    )
}

fn fetch_declared_artifact(url: &str, destination: &Path) -> Result<(), ProviderError> {
    if let Some(path) = url.strip_prefix("file://") {
        copy_bounded_file(Path::new(path), destination)
    } else {
        fetch_remote_artifact(url, destination, "native recipe artifact", |detail| {
            native_catalog_error(detail)
        })
    }
}

fn fetch_remote_artifact<F>(
    url: &str,
    destination: &Path,
    label: &str,
    error: F,
) -> Result<(), ProviderError>
where
    F: Fn(String) -> ProviderError,
{
    ensure_network_allowed(label)?;
    let response = jet_net::get_stream_follow_redirects(url, Duration::from_secs(120), 5)
        .map_err(|fetch_error| error(format!("could not fetch {label}: {fetch_error}")))?;
    if !(200..300).contains(&response.status()) {
        return Err(error(format!(
            "{label} URL returned HTTP {}",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ARTIFACT_BYTES)
    {
        return Err(error(format!("{label} exceeds its size bound")));
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|create_error| {
            error(format!(
                "could not create {label} staging file: {create_error}"
            ))
        })?;
    let mut limited = response.take(MAX_ARTIFACT_BYTES.saturating_add(1));
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = limited
            .read(&mut buffer)
            .map_err(|read_error| error(format!("could not read {label}: {read_error}")))?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > MAX_ARTIFACT_BYTES {
            return Err(error(format!("{label} exceeds its size bound")));
        }
        output
            .write_all(&buffer[..count])
            .map_err(|write_error| error(format!("could not write {label}: {write_error}")))?;
    }
    output
        .sync_all()
        .map_err(|sync_error| error(format!("could not sync {label}: {sync_error}")))?;
    validate_artifact_file(destination, error, label)
}

fn validate_artifact_file<F>(destination: &Path, error: F, label: &str) -> Result<(), ProviderError>
where
    F: Fn(String) -> ProviderError,
{
    let metadata = fs::symlink_metadata(destination)
        .map_err(|stat_error| error(format!("could not stat {label}: {stat_error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return Err(error(format!("{label} is not a non-empty regular file")));
    }
    if metadata.len() > MAX_ARTIFACT_BYTES {
        return Err(error(format!("{label} exceeds its size bound")));
    }
    Ok(())
}

fn copy_bounded_file(source: &Path, destination: &Path) -> Result<(), ProviderError> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| native_error(format!("could not stat fixture artifact: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return Err(native_error(
            "fixture artifact is not a non-empty regular file",
        ));
    }
    if metadata.len() > MAX_ARTIFACT_BYTES {
        return Err(native_error("fixture artifact exceeds its size bound"));
    }
    let mut input = File::open(source)
        .map_err(|error| native_error(format!("could not open fixture artifact: {error}")))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| native_error(format!("could not create fixture staging file: {error}")))?;
    io::copy(&mut input, &mut output)
        .map_err(|error| native_error(format!("could not copy fixture artifact: {error}")))?;
    output
        .sync_all()
        .map_err(|error| native_error(format!("could not sync fixture artifact: {error}")))?;
    Ok(())
}

/// Fetch one bounded remote payload through the native provider's network
/// policy. Toolchain channel metadata and artifacts use this same seam so the
/// updater does not grow a second HTTP client with different redirect,
/// timeout, size, or offline behavior.
pub(crate) fn fetch_bounded_bytes(
    url: &str,
    limit: u64,
    label: &str,
) -> Result<Vec<u8>, String> {
    ensure_network_allowed(label).map_err(|error| match error {
        ProviderError::Offline(detail) => detail,
        other => format!("{other:?}"),
    })?;
    let response = jet_net::get_stream_follow_redirects(url, Duration::from_secs(120), 5)
        .map_err(|error| format!("could not fetch {label}: {error}"))?;
    if !(200..300).contains(&response.status()) {
        return Err(format!(
            "{label} URL returned HTTP {}",
            response.status()
        ));
    }
    let content_length = response.content_length();
    if content_length.is_some_and(|length| length > limit) {
        return Err(format!("{label} exceeds its size bound"));
    }
    let mut body = Vec::new();
    response
        .take(limit.saturating_add(1))
        .read_to_end(&mut body)
        .map_err(|error| format!("could not read {label}: {error}"))?;
    if body.len() as u64 > limit {
        return Err(format!("{label} exceeds its size bound"));
    }
    if content_length.is_some_and(|length| length != body.len() as u64) {
        return Err(format!("{label} Content-Length disagrees"));
    }
    Ok(body)
}

fn safe_fixture_path(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        && path != root
}

fn read_bounded_file(path: &Path, limit: u64) -> Result<Vec<u8>, ProviderError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| native_error(format!("could not stat `{}`: {error}", path.display())))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > limit {
        return Err(native_error(format!(
            "bounded input `{}` is not a regular file within its limit",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| native_error(format!("could not open `{}`: {error}", path.display())))?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| native_error(format!("could not read `{}`: {error}", path.display())))?;
    if bytes.len() as u64 > limit {
        return Err(native_error("bounded input exceeds its size limit"));
    }
    Ok(bytes)
}

fn make_executable(path: &Path) -> Result<(), ProviderError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)
            .map_err(|error| native_error(format!("could not stat executable: {error}")))?
            .permissions()
            .mode();
        fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o755))
            .map_err(|error| native_error(format!("could not mark executable: {error}")))?;
    }
    Ok(())
}

fn remove_staging(path: &Path) {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            let _ = fs::remove_dir_all(path);
        } else {
            let _ = fs::remove_file(path);
        }
    }
}

fn native_error(reason: impl Into<String>) -> ProviderError {
    ProviderError::BuildFailed(format!("native jetpackage `{PACKAGE}`: {}", reason.into()))
}

fn native_catalog_error(reason: impl Into<String>) -> ProviderError {
    ProviderError::BuildFailed(format!(
        "local unofficial native catalog: {}",
        reason.into()
    ))
}
