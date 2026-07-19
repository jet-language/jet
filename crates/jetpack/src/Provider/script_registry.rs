//! Native RubyGems, CPAN, and Packagist providers (D-FFI-RUBY1/PERL1/PHP1).

use super::{cache_identity, producer_record, Ctx, Provider, ProviderError, Realized, SourceState};
use crate::RefSpec::{RefSpec, SourceTable};
use crate::JSON::Json;
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    RubyGems,
    Cpan,
    Packagist,
}

impl Kind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::RubyGems => "ruby",
            Self::Cpan => "perl",
            Self::Packagist => "php",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::RubyGems => "RubyGems",
            Self::Cpan => "CPAN",
            Self::Packagist => "Packagist",
        }
    }

    pub(super) fn recipe(self) -> &'static str {
        match self {
            Self::RubyGems => "rubygems-provider-v1",
            Self::Cpan => "cpan-provider-v1",
            Self::Packagist => "packagist-provider-v1",
        }
    }

    fn repository(self) -> String {
        #[cfg(test)]
        if let Some(value) = TEST_REPOSITORIES.read().unwrap().get(self.label()).cloned() {
            return value;
        }
        match self {
            Self::RubyGems => "https://index.rubygems.org".into(),
            Self::Cpan => "https://fastapi.metacpan.org".into(),
            Self::Packagist => "https://repo.packagist.org".into(),
        }
    }

    fn fetch_authorities(self) -> &'static [&'static str] {
        match self {
            Self::RubyGems => &["index.rubygems.org"],
            Self::Cpan => &["fastapi.metacpan.org", "cpan.metacpan.org"],
            Self::Packagist => &[
                "repo.packagist.org",
                "api.github.com",
                "codeload.github.com",
                "github.com",
            ],
        }
    }

    fn valid_name(self, name: &str) -> bool {
        match self {
            Self::RubyGems => safe_piece(name, "_-.") && !name.contains('/'),
            Self::Cpan => {
                !name.is_empty()
                    && name.split("::").all(|piece| safe_piece(piece, "_-."))
                    && !name.contains('/')
            }
            Self::Packagist => {
                let mut pieces = name.split('/');
                matches!((pieces.next(), pieces.next(), pieces.next()), (Some(a), Some(b), None) if safe_piece(a, "_-.") && safe_piece(b, "_-."))
            }
        }
    }
}

#[cfg(test)]
static TEST_REPOSITORIES: std::sync::RwLock<BTreeMap<&'static str, String>> =
    std::sync::RwLock::new(BTreeMap::new());

#[derive(Debug, Clone)]
struct Dependency {
    name: String,
    requirement: String,
}

#[derive(Debug, Clone)]
enum Integrity {
    Sha256(String),
    Sha1(String),
    ImmutableGit {
        repository: String,
        reference: String,
    },
}

#[derive(Debug, Clone)]
struct Package {
    name: String,
    version: String,
    url: String,
    integrity: Integrity,
    dependencies: Vec<Dependency>,
    psr4: BTreeMap<String, Vec<String>>,
}

#[derive(Debug)]
struct Artifact {
    package: Package,
    path: PathBuf,
    sha256: String,
}

pub(super) struct ScriptRegistryProvider(pub(super) Kind);

impl Provider for ScriptRegistryProvider {
    fn realize(
        &self,
        spec: &RefSpec,
        _table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Realized, ProviderError> {
        let kind = self.0;
        if ctx.offline {
            return Err(ProviderError::Offline(format!(
                "`{}` is not in verified Hangar and --offline forbids {} metadata or source fetches",
                spec.raw,
                kind.title()
            )));
        }
        let (root_name, root_version) = parse_ref(kind, &spec.package)?;
        let authority = super::fetch::Authority::load(
            ctx,
            kind.label(),
            &kind.repository(),
            kind.fetch_authorities(),
        )
        .map_err(|error| fail(kind, error))?;
        let repository = authority.registry().to_string();
        let fetch_authority = authority.provenance();
        let scratch = Scratch::new(ctx.store_dir, kind)?;
        let packages = resolve_closure(
            kind,
            &authority,
            &scratch,
            root_name,
            root_version,
        )?;
        let mut artifacts = Vec::new();
        for package in packages {
            let path = scratch.path.join(format!(
                "{}-{}{}",
                safe_filename(&package.name),
                safe_filename(&package.version),
                archive_suffix(kind)
            ));
            authority
                .to_path(&package.url, &path, &scratch.path)
                .map_err(|error| fail(kind, error))?;
            let bytes = std::fs::read(&path)
                .map_err(|error| fail(kind, format!("could not read fetched source: {error}")))?;
            verify_integrity(kind, &package, &bytes)?;
            artifacts.push(Artifact {
                package,
                path,
                sha256: SHA256::sha256_hex(&bytes),
            });
        }
        let source_hash = closure_hash(kind, &artifacts);
        if let Some(project) = ctx.project_dir {
            if let Some((_output, locked_hash, locked_repository, _)) =
                crate::Lock::registry_realization(project, kind.label(), &spec.raw)
            {
                if locked_hash != source_hash || locked_repository != repository {
                    return Err(fail(
                        kind,
                        format!(
                            "locked {} source integrity changed for `{}` (expected {} from {}, got {} from {})",
                            kind.title(), spec.raw, locked_hash, locked_repository, source_hash, repository
                        ),
                    ));
                }
            }
        }
        let root = artifacts
            .last()
            .ok_or_else(|| fail(kind, "resolved closure omitted root package"))?;
        let out_dir = ctx.store_dir.join(format!(
            "{}-{}-{}",
            safe_filename(&root.package.name),
            safe_filename(&root.package.version),
            &source_hash[..12]
        ));
        if out_dir.exists() {
            return Err(fail(
                kind,
                format!(
                    "unverified existing output {}; run `jet clean` before rebuilding",
                    out_dir.display()
                ),
            ));
        }
        let sources = out_dir.join("sources");
        std::fs::create_dir_all(&sources)
            .map_err(|error| fail(kind, format!("could not stage provider output: {error}")))?;
        for artifact in &artifacts {
            std::fs::copy(
                &artifact.path,
                sources.join(artifact.path.file_name().unwrap_or_default()),
            )
            .map_err(|error| fail(kind, format!("could not preserve source archive: {error}")))?;
            install_artifact(kind, artifact, &out_dir, &scratch)?;
        }
        write_runtime_projection(kind, &artifacts, &out_dir)?;
        std::fs::write(
            out_dir.join(format!("{}.provenance", kind.label())),
            render_provenance(
                kind,
                &repository,
                &fetch_authority,
                &source_hash,
                &artifacts,
            ),
        )
        .map_err(|error| fail(kind, format!("could not write provenance: {error}")))?;
        crate::Store::seal_local_output(&out_dir)
            .map_err(|error| fail(kind, format!("could not seal provider output: {error}")))?;

        let out = out_dir.to_string_lossy().into_owned();
        let envelope = crate::Envelope::Envelope::for_output(&out, &spec.raw, kind.recipe());
        let dependencies = artifacts
            .iter()
            .filter(|artifact| !std::ptr::eq(*artifact, root))
            .map(|artifact| {
                format!(
                    "{}#version={}",
                    artifact.package.name, artifact.package.version
                )
            })
            .collect::<Vec<_>>();
        if let Some(project) = ctx.project_dir {
            crate::Lock::record_registry_realization(
                project,
                kind.label(),
                &root.package.name,
                &root.package.version,
                &spec.raw,
                &out,
                &source_hash,
                &repository,
                dependencies,
                crate::Lock::LockEnvelope {
                    output_hash: envelope.output_hash.clone(),
                    platform: envelope.platform.clone(),
                    signature: envelope.signature.clone(),
                    provenance: envelope.provenance.clone(),
                },
            );
        }
        let identity = cache_identity(&source_hash, kind.recipe(), ctx);
        let (references, mut facts) = dependency_objects(&root.package.name, &artifacts);
        facts.insert("repository".into(), repository.clone());
        facts.insert("fetch.authority".into(), fetch_authority.clone());
        let producer = producer_record(
            kind.label(),
            &format!("cas:{source_hash}"),
            &source_hash,
            BTreeMap::from([
                ("action.kind".into(), format!("{}-install", kind.label())),
                ("repository".into(), repository),
                ("fetch.authority".into(), fetch_authority),
                ("package.version".into(), root.package.version.clone()),
                ("scripts".into(), "disabled".into()),
                ("plugins".into(), "disabled".into()),
            ]),
            kind.recipe(),
            &identity,
            facts,
        )
        .map_err(|error| fail(kind, format!("invalid producer record: {error:?}")))?;
        Ok(Realized {
            name: safe_filename(&root.package.name),
            version: root.package.version.clone(),
            reference: spec.raw.clone(),
            out: out.clone(),
            bin: String::new(),
            rlib: String::new(),
            envelope,
            cache_identity: identity,
            source_state: SourceState::Built,
            named_outputs: BTreeMap::from([("out".into(), out)]),
            references,
            producer,
        })
    }
}

fn fail(kind: Kind, reason: impl Into<String>) -> ProviderError {
    ProviderError::Registry(kind.title(), reason.into())
}

fn parse_ref<'a>(kind: Kind, raw: &'a str) -> Result<(&'a str, &'a str), ProviderError> {
    let (name, selector) = raw.split_once('#').unwrap_or((raw, ""));
    if !kind.valid_name(name) {
        return Err(fail(
            kind,
            format!("invalid {} package name `{name}`", kind.title()),
        ));
    }
    let version = selector.strip_prefix("version=").unwrap_or("");
    if version.is_empty() || !safe_piece(version, "+_-.") {
        return Err(fail(
            kind,
            format!(
                "{} ref `{raw}` is mutable; use `{name}#version=<exact>`",
                kind.title()
            ),
        ));
    }
    Ok((name, version))
}

fn resolve_closure(
    kind: Kind,
    authority: &super::fetch::Authority,
    scratch: &Scratch,
    root: &str,
    version: &str,
) -> Result<Vec<Package>, ProviderError> {
    fn solve(
        kind: Kind,
        authority: &super::fetch::Authority,
        scratch: &Scratch,
        constraints: BTreeMap<String, Vec<String>>,
        selected: BTreeMap<String, Package>,
        candidates: &mut BTreeMap<String, Vec<Package>>,
    ) -> Result<Option<BTreeMap<String, Package>>, ProviderError> {
        if selected.iter().any(|(name, package)| {
            constraints
                .get(name)
                .is_some_and(|requirements| !requirements.iter().all(|requirement| {
                    version_satisfies(&package.version, requirement)
                }))
        }) {
            return Ok(None);
        }
        let Some(name) = constraints
            .keys()
            .find(|name| !selected.contains_key(*name))
            .cloned()
        else {
            return Ok(Some(selected));
        };
        if !candidates.contains_key(&name) {
            let mut fetched = fetch_candidates(kind, authority, scratch, &name)?;
            fetched.sort_by(|left, right| {
                compare_versions(&right.version, &left.version)
                    .then_with(|| left.url.cmp(&right.url))
            });
            candidates.insert(name.clone(), fetched);
        }
        let requirements = constraints.get(&name).cloned().unwrap_or_default();
        for package in candidates.get(&name).cloned().unwrap_or_default() {
            if !requirements
                .iter()
                .all(|requirement| version_satisfies(&package.version, requirement))
            {
                continue;
            }
            let mut next_constraints = constraints.clone();
            for dependency in &package.dependencies {
                next_constraints
                    .entry(dependency.name.clone())
                    .or_default()
                    .push(dependency.requirement.clone());
            }
            let mut next_selected = selected.clone();
            next_selected.insert(name.clone(), package);
            if let Some(solution) = solve(
                kind,
                authority,
                scratch,
                next_constraints,
                next_selected,
                candidates,
            )? {
                return Ok(Some(solution));
            }
        }
        Ok(None)
    }
    fn order(
        kind: Kind,
        name: &str,
        selected: &BTreeMap<String, Package>,
        active: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
        out: &mut Vec<Package>,
    ) -> Result<(), ProviderError> {
        if done.contains(name) {
            return Ok(());
        }
        if !active.insert(name.to_string()) {
            return Err(fail(kind, format!("dependency cycle includes `{name}`")));
        }
        let package = selected
            .get(name)
            .ok_or_else(|| fail(kind, format!("resolved package `{name}` disappeared")))?;
        for dependency in &package.dependencies {
            order(kind, &dependency.name, selected, active, done, out)?;
        }
        active.remove(name);
        done.insert(name.to_string());
        out.push(package.clone());
        Ok(())
    }
    let constraints = BTreeMap::from([(root.to_string(), vec![format!("={version}")])]);
    let selected = solve(kind, authority, scratch, constraints, BTreeMap::new(), &mut BTreeMap::new())?
        .ok_or_else(|| fail(kind, format!("no complete dependency solution exists for `{root}` {version}")))?;
    let mut out = Vec::new();
    order(
        kind,
        root,
        &selected,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
        &mut out,
    )?;
    Ok(out)
}

fn fetch_candidates(
    kind: Kind,
    authority: &super::fetch::Authority,
    scratch: &Scratch,
    name: &str,
) -> Result<Vec<Package>, ProviderError> {
    match kind {
        Kind::RubyGems => fetch_rubygems(authority, scratch, name),
        Kind::Cpan => fetch_cpan(authority, scratch, name),
        Kind::Packagist => fetch_packagist(authority, scratch, name),
    }
}

fn fetch_rubygems(
    authority: &super::fetch::Authority,
    scratch: &Scratch,
    name: &str,
) -> Result<Vec<Package>, ProviderError> {
    let repository = authority.registry();
    let raw = authority
        .text(&format!("{repository}/info/{name}"), &scratch.path)
        .map_err(|error| fail(Kind::RubyGems, error))?;
    let mut out = Vec::new();
    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let (left, attributes) = line
            .split_once('|')
            .ok_or_else(|| fail(Kind::RubyGems, "compact-index info line has no attributes"))?;
        let mut fields = left.splitn(2, ' ');
        let version = fields.next().unwrap_or("").trim();
        if !safe_piece(version, "+_-.") {
            return Err(fail(
                Kind::RubyGems,
                format!("unsafe gem version `{version}`"),
            ));
        }
        let checksum = attributes
            .split(',')
            .find_map(|field| field.trim().strip_prefix("checksum:"))
            .filter(|value| valid_hex(value, 64))
            .ok_or_else(|| {
                fail(
                    Kind::RubyGems,
                    format!("gem `{name}` {version} has no SHA-256 compact-index checksum"),
                )
            })?;
        let mut dependencies = Vec::new();
        if let Some(raw_dependencies) = fields.next() {
            for raw_dependency in raw_dependencies
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let (dep_name, requirement) = raw_dependency
                    .split_once(':')
                    .unwrap_or((raw_dependency, ">= 0"));
                if matches!(dep_name, "ruby" | "rubygems") {
                    continue;
                }
                if !Kind::RubyGems.valid_name(dep_name) {
                    return Err(fail(
                        Kind::RubyGems,
                        format!("gem `{name}` has unsafe dependency `{dep_name}`"),
                    ));
                }
                dependencies.push(Dependency {
                    name: dep_name.to_string(),
                    requirement: requirement.replace('&', ","),
                });
            }
        }
        out.push(Package {
            name: name.to_string(),
            version: version.to_string(),
            url: format!("{repository}/gems/{name}-{version}.gem"),
            integrity: Integrity::Sha256(checksum.to_ascii_lowercase()),
            dependencies,
            psr4: BTreeMap::new(),
        });
    }
    if out.is_empty() {
        Err(fail(
            Kind::RubyGems,
            format!("compact index has no versions for gem `{name}`"),
        ))
    } else {
        Ok(out)
    }
}

fn fetch_cpan(
    authority: &super::fetch::Authority,
    scratch: &Scratch,
    name: &str,
) -> Result<Vec<Package>, ProviderError> {
    let repository = authority.registry();
    let distribution = authority
        .text(
            &format!("{repository}/v1/module/{}", url_piece(name)),
            &scratch.path,
        )
        .ok()
        .and_then(|raw| crate::JSON::parse(&raw).ok())
        .and_then(|value| value.get("distribution").ok()?.as_str().ok().map(str::to_string))
        .unwrap_or_else(|| name.to_string());
    let query = format!(
        "{repository}/v1/release/_search?q={}&size=500",
        url_piece(&format!("distribution:{distribution}"))
    );
    let value = authority
        .text(&query, &scratch.path)
        .map_err(|error| fail(Kind::Cpan, error))
        .and_then(|raw| {
            crate::JSON::parse(&raw)
                .map_err(|error| fail(Kind::Cpan, format!("invalid MetaCPAN JSON: {error}")))
        })?;
    let releases = value
        .get("hits")
        .and_then(|hits| hits.get("hits"))
        .and_then(Json::as_array)
        .map_err(|error| fail(Kind::Cpan, format!("invalid MetaCPAN release search: {error}")))?;
    let mut out = Vec::new();
    for hit in releases {
        let release = hit
            .get("_source")
            .map_err(|error| fail(Kind::Cpan, format!("invalid MetaCPAN release hit: {error}")))?;
        out.push(parse_cpan_release(repository, release)?);
    }
    if out.is_empty() {
        return Err(fail(
            Kind::Cpan,
            format!("MetaCPAN has no releases for distribution `{distribution}`"),
        ));
    }
    Ok(out)
}

fn parse_cpan_release(repository: &str, value: &Json) -> Result<Package, ProviderError> {
    let package_name = json_string(&value, "distribution", Kind::Cpan)?;
    let version = json_string(&value, "version", Kind::Cpan)?;
    let url = json_string(&value, "download_url", Kind::Cpan)?;
    let checksum = json_string(&value, "checksum_sha256", Kind::Cpan)?;
    if !Kind::Cpan.valid_name(package_name)
        || !safe_piece(version, "+_-.")
        || !valid_hex(checksum, 64)
    {
        return Err(fail(
            Kind::Cpan,
            "MetaCPAN release identity is unsafe or unhashed",
        ));
    }
    let mut dependencies = Vec::new();
    if let Some(Json::Array(items)) = value.as_object().ok().and_then(|obj| obj.get("dependency")) {
        for item in items {
            let relationship = json_string(item, "relationship", Kind::Cpan)?;
            let phase = json_string(item, "phase", Kind::Cpan)?;
            if relationship != "requires" || phase != "runtime" {
                continue;
            }
            let dep_name = json_string(item, "module", Kind::Cpan)?;
            if dep_name == "perl" {
                continue;
            }
            if !Kind::Cpan.valid_name(dep_name) {
                return Err(fail(
                    Kind::Cpan,
                    format!("unsafe CPAN dependency `{dep_name}`"),
                ));
            }
            dependencies.push(Dependency {
                name: dep_name.to_string(),
                requirement: cpan_requirement(
                    item.as_object()
                        .ok()
                        .and_then(|obj| obj.get("version"))
                        .and_then(|value| value.as_str().ok())
                        .filter(|value| !value.is_empty())
                        .unwrap_or("0"),
                ),
            });
        }
    }
    Ok(Package {
        name: package_name.to_string(),
        version: version.to_string(),
        url: absolutize(repository, url),
        integrity: Integrity::Sha256(checksum.to_ascii_lowercase()),
        dependencies,
        psr4: BTreeMap::new(),
    })
}

fn fetch_packagist(
    authority: &super::fetch::Authority,
    scratch: &Scratch,
    name: &str,
) -> Result<Vec<Package>, ProviderError> {
    let repository = authority.registry();
    let value = fetch_json(
        Kind::Packagist,
        authority,
        scratch,
        &format!("{repository}/p2/{name}.json"),
    )?;
    let packages = value
        .get("packages")
        .and_then(Json::as_object)
        .map_err(|error| {
            fail(
                Kind::Packagist,
                format!("invalid Packagist packages: {error}"),
            )
        })?;
    let releases = packages
        .get(name)
        .ok_or_else(|| {
            fail(
                Kind::Packagist,
                format!("Packagist has no package `{name}`"),
            )
        })?
        .as_array()
        .map_err(|error| {
            fail(
                Kind::Packagist,
                format!("invalid Packagist releases: {error}"),
            )
        })?;
    let releases = expand_packagist_releases(&value, releases)?;
    let mut out = Vec::new();
    for release in &releases {
        let version = json_string(release, "version", Kind::Packagist)?.trim_start_matches('v');
        if !safe_piece(version, "+_-.") {
            continue;
        }
        if release
            .as_object()
            .ok()
            .and_then(|obj| obj.get("type"))
            .and_then(|value| value.as_str().ok())
            == Some("composer-plugin")
        {
            return Err(fail(
                Kind::Packagist,
                format!(
                    "Composer plugin package `{name}` is not allowed to execute during realization"
                ),
            ));
        }
        let dist = release
            .get("dist")
            .map_err(|error| fail(Kind::Packagist, error))?;
        let dist_type = json_string(dist, "type", Kind::Packagist)?;
        if dist_type != "zip" {
            return Err(fail(
                Kind::Packagist,
                format!("package `{name}` {version} uses unsupported dist type `{dist_type}`"),
            ));
        }
        let url = json_string(dist, "url", Kind::Packagist)?;
        let shasum = json_string(dist, "shasum", Kind::Packagist)?;
        let integrity = match shasum.len() {
            40 if valid_hex(shasum, 40) => Integrity::Sha1(shasum.to_ascii_lowercase()),
            64 if valid_hex(shasum, 64) => Integrity::Sha256(shasum.to_ascii_lowercase()),
            0 => {
                let reference = json_string(dist, "reference", Kind::Packagist)?;
                let repository = verified_packagist_git_source(release, url, reference).ok_or_else(|| {
                    fail(
                        Kind::Packagist,
                        format!("package `{name}` {version} has neither a digest nor a verified immutable GitHub source authority"),
                    )
                })?;
                Integrity::ImmutableGit {
                    repository,
                    reference: reference.to_ascii_lowercase(),
                }
            }
            _ => {
                return Err(fail(
                    Kind::Packagist,
                    format!("package `{name}` {version} has no supported dist checksum"),
                ))
            }
        };
        let mut dependencies = Vec::new();
        if let Some(Json::Object(require)) =
            release.as_object().ok().and_then(|obj| obj.get("require"))
        {
            for (dep_name, requirement) in require {
                if dep_name == "php" || dep_name.starts_with("ext-") || dep_name.starts_with("lib-")
                {
                    continue;
                }
                if !Kind::Packagist.valid_name(dep_name) {
                    return Err(fail(
                        Kind::Packagist,
                        format!("package `{name}` has unsafe dependency `{dep_name}`"),
                    ));
                }
                dependencies.push(Dependency {
                    name: dep_name.clone(),
                    requirement: requirement
                        .as_str()
                        .map_err(|error| fail(Kind::Packagist, error))?
                        .to_string(),
                });
            }
        }
        let mut psr4 = BTreeMap::new();
        if let Some(Json::Object(autoload)) =
            release.as_object().ok().and_then(|obj| obj.get("autoload"))
        {
            if autoload.keys().any(|key| key != "psr-4") {
                return Err(fail(
                    Kind::Packagist,
                    format!("package `{name}` {version} uses unsupported executable or non-PSR-4 autoload metadata"),
                ));
            }
            if let Some(Json::Object(mappings)) = autoload.get("psr-4") {
                for (prefix, value) in mappings {
                    let paths = match value {
                        Json::Str(path) => vec![path.clone()],
                        Json::Array(paths) => paths
                            .iter()
                            .map(|path| {
                                path.as_str()
                                    .map(str::to_string)
                                    .map_err(|error| fail(Kind::Packagist, error))
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                        _ => return Err(fail(Kind::Packagist, "PSR-4 path must be a string or ordered string array")),
                    };
                    if prefix.contains('\0') || paths.is_empty() || paths.iter().any(|path| {
                        path.contains('\0')
                            || (!path.trim_matches('/').is_empty()
                                && !safe_relative(Path::new(path.trim_matches('/'))))
                    }) {
                        return Err(fail(Kind::Packagist, "unsafe PSR-4 autoload path"));
                    }
                    psr4.insert(prefix.clone(), paths);
                }
            }
        }
        out.push(Package {
            name: name.to_string(),
            version: version.to_string(),
            url: absolutize(repository, url),
            integrity,
            dependencies,
            psr4,
        });
    }
    if out.is_empty() {
        Err(fail(
            Kind::Packagist,
            format!("Packagist has no usable release for `{name}`"),
        ))
    } else {
        Ok(out)
    }
}

fn expand_packagist_releases(
    metadata: &Json,
    releases: &[Json],
) -> Result<Vec<Json>, ProviderError> {
    let minified = metadata
        .as_object()
        .ok()
        .and_then(|object| object.get("minified"))
        .and_then(|value| value.as_str().ok());
    if minified.is_none() {
        return Ok(releases.to_vec());
    }
    if minified != Some("composer/2.0") {
        return Err(fail(
            Kind::Packagist,
            "unsupported Packagist metadata minifier",
        ));
    }
    let mut expanded = Vec::with_capacity(releases.len());
    let mut previous = BTreeMap::new();
    for release in releases {
        let changes = release.as_object().map_err(|error| {
            fail(
                Kind::Packagist,
                format!("invalid minified release: {error}"),
            )
        })?;
        for (key, value) in changes {
            if matches!(value, Json::Str(value) if value == "__unset") {
                previous.remove(key);
            } else {
                previous.insert(key.clone(), value.clone());
            }
        }
        expanded.push(Json::Object(previous.clone()));
    }
    Ok(expanded)
}

fn fetch_json(
    kind: Kind,
    authority: &super::fetch::Authority,
    scratch: &Scratch,
    url: &str,
) -> Result<Json, ProviderError> {
    let raw = authority
        .text(url, &scratch.path)
        .map_err(|error| fail(kind, error))?;
    crate::JSON::parse(&raw).map_err(|error| {
        fail(
            kind,
            format!("invalid {} metadata JSON: {error}", kind.title()),
        )
    })
}

fn verified_packagist_git_source(release: &Json, dist_url: &str, reference: &str) -> Option<String> {
    if !matches!(reference.len(), 40 | 64)
        || !reference.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    let source = release.get("source").ok()?;
    if json_string(source, "type", Kind::Packagist).ok()? != "git"
        || json_string(source, "reference", Kind::Packagist).ok()? != reference
    {
        return None;
    }
    let repository = json_string(source, "url", Kind::Packagist).ok()?;
    let path = repository.strip_prefix("https://github.com/")?.strip_suffix(".git")?;
    let mut pieces = path.split('/');
    let (owner, repo) = (pieces.next()?, pieces.next()?);
    if pieces.next().is_some() || !safe_piece(owner, "-_.") || !safe_piece(repo, "-_.") {
        return None;
    }
    let expected = format!("https://api.github.com/repos/{owner}/{repo}/zipball/{reference}");
    (dist_url.split('?').next()? == expected).then(|| repository.to_string())
}

fn json_string<'a>(value: &'a Json, field: &str, kind: Kind) -> Result<&'a str, ProviderError> {
    value
        .get(field)
        .and_then(Json::as_str)
        .map_err(|error| fail(kind, format!("invalid metadata field `{field}`: {error}")))
}

fn install_artifact(
    kind: Kind,
    artifact: &Artifact,
    out: &Path,
    scratch: &Scratch,
) -> Result<(), ProviderError> {
    match kind {
        Kind::RubyGems => install_gem(artifact, out, scratch),
        Kind::Cpan => install_cpan(artifact, out, scratch),
        Kind::Packagist => install_composer(artifact, out, scratch),
    }
}

fn install_gem(artifact: &Artifact, out: &Path, scratch: &Scratch) -> Result<(), ProviderError> {
    validate_tar(Kind::RubyGems, &artifact.path, false)?;
    let unpack = scratch
        .path
        .join(format!("gem-{}", safe_filename(&artifact.package.name)));
    std::fs::create_dir_all(&unpack).map_err(|error| {
        fail(
            Kind::RubyGems,
            format!("could not create gem scratch: {error}"),
        )
    })?;
    run_archive_command(
        Kind::RubyGems,
        Command::new("tar")
            .args(["-xf"])
            .arg(&artifact.path)
            .arg("-C")
            .arg(&unpack),
        "unpack gem container",
    )?;
    let data = unpack.join("data.tar.gz");
    if !data.is_file() {
        return Err(fail(Kind::RubyGems, "gem archive has no data.tar.gz"));
    }
    validate_tar(Kind::RubyGems, &data, true)?;
    let gem_root = out.join("gems").join(format!(
        "{}-{}",
        artifact.package.name, artifact.package.version
    ));
    std::fs::create_dir_all(&gem_root).map_err(|error| {
        fail(
            Kind::RubyGems,
            format!("could not create gem home: {error}"),
        )
    })?;
    run_archive_command(
        Kind::RubyGems,
        Command::new("tar")
            .args(["-xzf"])
            .arg(&data)
            .arg("-C")
            .arg(&gem_root),
        "unpack gem payload",
    )?;
    reject_tree_features(
        Kind::RubyGems,
        &gem_root,
        &["ext", "extensions", "plugins", "rubygems_plugin.rb"],
    )
}

fn install_cpan(artifact: &Artifact, out: &Path, scratch: &Scratch) -> Result<(), ProviderError> {
    validate_tar(Kind::Cpan, &artifact.path, true)?;
    let unpack = scratch
        .path
        .join(format!("cpan-{}", safe_filename(&artifact.package.name)));
    std::fs::create_dir_all(&unpack).map_err(|error| {
        fail(
            Kind::Cpan,
            format!("could not create CPAN scratch: {error}"),
        )
    })?;
    run_archive_command(
        Kind::Cpan,
        Command::new("tar")
            .args(["-xzf"])
            .arg(&artifact.path)
            .arg("-C")
            .arg(&unpack),
        "unpack CPAN distribution",
    )?;
    let root = archive_root(Kind::Cpan, &unpack)?;
    reject_tree_features(Kind::Cpan, &root, &[])?;
    let library = root.join("lib");
    if !library.is_dir() {
        return Err(fail(
            Kind::Cpan,
            format!(
                "CPAN distribution `{}` has no pure-Perl lib directory",
                artifact.package.name
            ),
        ));
    }
    copy_tree(Kind::Cpan, &library, &out.join("lib"))
}

fn install_composer(
    artifact: &Artifact,
    out: &Path,
    scratch: &Scratch,
) -> Result<(), ProviderError> {
    const ZIP_LIST: &str = "$z=new ZipArchive(); if($z->open($argv[1])!==true) exit(2); for($i=0;$i<$z->numFiles;$i++){ $ops=0;$attr=0;$z->getExternalAttributesIndex($i,$ops,$attr); echo dechex(($attr>>16)&0xf000),\"\\t\",$z->getNameIndex($i),\"\\n\"; }";
    let listing = Command::new("php")
        .args([
            "-d",
            "auto_prepend_file=",
            "-d",
            "auto_append_file=",
            "-r",
            ZIP_LIST,
            "--",
        ])
        .arg(&artifact.path)
        .output()
        .map_err(|error| {
            fail(
                Kind::Packagist,
                format!("could not inspect zip archive: {error}"),
            )
        })?;
    if !listing.status.success() {
        return Err(fail(
            Kind::Packagist,
            "Packagist dist is not a readable zip archive",
        ));
    }
    let listing = String::from_utf8(listing.stdout)
        .map_err(|_| fail(Kind::Packagist, "Packagist zip paths are not UTF-8"))?;
    for line in listing.lines() {
        let (mode, entry) = line
            .split_once('\t')
            .ok_or_else(|| fail(Kind::Packagist, "Packagist zip type listing is malformed"))?;
        if entry.is_empty() || !safe_relative(Path::new(entry.trim_end_matches('/'))) {
            return Err(fail(
                Kind::Packagist,
                format!("Packagist zip contains unsafe path `{entry}`"),
            ));
        }
        if !matches!(mode, "0" | "4000" | "8000") {
            return Err(fail(
                Kind::Packagist,
                format!("Packagist zip contains link or special entry `{entry}`"),
            ));
        }
    }
    let unpack = scratch
        .path
        .join(format!("php-{}", safe_filename(&artifact.package.name)));
    std::fs::create_dir_all(&unpack).map_err(|error| {
        fail(
            Kind::Packagist,
            format!("could not create zip scratch: {error}"),
        )
    })?;
    const ZIP_EXTRACT: &str = "$z=new ZipArchive(); if($z->open($argv[1])!==true) exit(2); if(!$z->extractTo($argv[2])) exit(3);";
    run_archive_command(
        Kind::Packagist,
        Command::new("php")
            .args([
                "-d",
                "auto_prepend_file=",
                "-d",
                "auto_append_file=",
                "-r",
                ZIP_EXTRACT,
                "--",
            ])
            .arg(&artifact.path)
            .arg(&unpack),
        "unpack Packagist dist with provisioned PHP ZipArchive",
    )?;
    let root = archive_root(Kind::Packagist, &unpack)?;
    reject_tree_features(Kind::Packagist, &root, &["composer-plugin"])?;
    copy_tree(
        Kind::Packagist,
        &root,
        &out.join("vendor").join(&artifact.package.name),
    )
}

fn write_runtime_projection(
    kind: Kind,
    artifacts: &[Artifact],
    out: &Path,
) -> Result<(), ProviderError> {
    match kind {
        Kind::RubyGems => {
            let mut ruby_lib = Vec::new();
            for artifact in artifacts {
                let lib = PathBuf::from("gems")
                    .join(format!(
                        "{}-{}",
                        artifact.package.name, artifact.package.version
                    ))
                    .join("lib");
                if out.join(&lib).is_dir() {
                    ruby_lib.push(lib.to_string_lossy().into_owned());
                }
            }
            std::fs::write(out.join("gem-home"), ".\n")
                .and_then(|_| std::fs::write(out.join("gem-path"), ".\n"))
                .and_then(|_| {
                    std::fs::write(
                        out.join("ruby-lib"),
                        format!(
                            "{}\n",
                            ruby_lib.join(&crate::Platform::path_separator().to_string())
                        ),
                    )
                })
                .map_err(|error| {
                    fail(
                        kind,
                        format!("could not write Ruby runtime projection: {error}"),
                    )
                })
        }
        Kind::Cpan => std::fs::write(out.join("perl5lib"), "lib\n").map_err(|error| {
            fail(
                kind,
                format!("could not write Perl runtime projection: {error}"),
            )
        }),
        Kind::Packagist => {
            let autoload = out.join("vendor/autoload.php");
            let mut mappings = Vec::new();
            for artifact in artifacts {
                for (prefix, paths) in &artifact.package.psr4 {
                    mappings.push((
                        prefix.clone(),
                        paths
                            .iter()
                            .map(|relative| {
                                PathBuf::from(&artifact.package.name)
                                    .join(relative.trim_matches('/'))
                            })
                            .collect::<Vec<_>>(),
                    ));
                }
            }
            mappings.sort_by(|left, right| {
                right
                    .0
                    .len()
                    .cmp(&left.0.len())
                    .then_with(|| left.0.cmp(&right.0))
            });
            let mut php = String::from(
                "<?php\nspl_autoload_register(static function (string $class): void {\n",
            );
            for (prefix, directories) in mappings {
                php.push_str(&format!(
                    "    if (str_starts_with($class, '{}')) {{\n",
                    php_quote(&prefix),
                ));
                for directory in directories {
                    php.push_str(&format!(
                        "        $file = __DIR__.'/{}'.str_replace('\\\\', '/', substr($class, {})).'.php'; if (is_file($file)) {{ require $file; return; }}\n",
                        php_quote(&format!("{}/", directory.display())),
                        prefix.len()
                    ));
                }
                php.push_str("    }\n");
            }
            php.push_str("});\nreturn true;\n");
            if let Some(parent) = autoload.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    fail(
                        kind,
                        format!("could not create Composer vendor dir: {error}"),
                    )
                })?;
            }
            std::fs::write(&autoload, php)
                .and_then(|_| {
                    std::fs::write(out.join("composer-autoload"), "vendor/autoload.php\n")
                })
                .map_err(|error| {
                    fail(
                        kind,
                        format!("could not write Composer autoloader: {error}"),
                    )
                })
        }
    }
}

fn validate_tar(kind: Kind, path: &Path, gzip: bool) -> Result<(), ProviderError> {
    let list_flag = if gzip { "-tzf" } else { "-tf" };
    let verbose_flag = if gzip { "-tvzf" } else { "-tvf" };
    let listing = Command::new("tar")
        .arg(list_flag)
        .arg(path)
        .output()
        .map_err(|error| fail(kind, format!("could not inspect source archive: {error}")))?;
    if !listing.status.success() {
        return Err(fail(kind, "source is not a readable tar archive"));
    }
    let listing = String::from_utf8(listing.stdout)
        .map_err(|_| fail(kind, "source archive paths are not UTF-8"))?;
    for entry in listing.lines() {
        if entry.is_empty() || !safe_relative(Path::new(entry.trim_end_matches('/'))) {
            return Err(fail(
                kind,
                format!("source archive contains unsafe path `{entry}`"),
            ));
        }
    }
    let verbose = Command::new("tar")
        .arg(verbose_flag)
        .arg(path)
        .output()
        .map_err(|error| {
            fail(
                kind,
                format!("could not inspect source archive types: {error}"),
            )
        })?;
    if !verbose.status.success() {
        return Err(fail(kind, "source archive types could not be inspected"));
    }
    for line in String::from_utf8(verbose.stdout)
        .map_err(|_| fail(kind, "source archive listing is not UTF-8"))?
        .lines()
    {
        if !matches!(line.as_bytes().first(), Some(b'-' | b'd')) {
            return Err(fail(
                kind,
                "source archives may contain only regular files and directories",
            ));
        }
    }
    Ok(())
}

fn run_archive_command(
    kind: Kind,
    command: &mut Command,
    action: &str,
) -> Result<(), ProviderError> {
    let status = command
        .status()
        .map_err(|error| fail(kind, format!("could not {action}: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(fail(kind, format!("could not {action}")))
    }
}

fn reject_tree_features(kind: Kind, root: &Path, forbidden: &[&str]) -> Result<(), ProviderError> {
    fn walk(kind: Kind, root: &Path, forbidden: &[&str]) -> Result<(), ProviderError> {
        for entry in std::fs::read_dir(root)
            .map_err(|error| fail(kind, format!("could not inspect installed source: {error}")))?
        {
            let entry = entry.map_err(|error| {
                fail(kind, format!("could not inspect installed source: {error}"))
            })?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
                fail(kind, format!("could not inspect installed source: {error}"))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(fail(kind, "installed source contains a symbolic link"));
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if forbidden.iter().chain(native_suffixes(kind)).any(|item| {
                let item = item.to_ascii_lowercase();
                name == item || (item.starts_with('.') && name.ends_with(&item))
            }) {
                return Err(fail(
                    kind,
                    format!("package requires unsupported native/plugin feature `{name}`"),
                ));
            }
            if metadata.is_dir() {
                walk(kind, &path, forbidden)?;
            } else if !metadata.is_file() {
                return Err(fail(kind, "installed source contains a special file"));
            }
        }
        Ok(())
    }
    walk(kind, root, forbidden)
}

fn native_suffixes(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::RubyGems => &[".so", ".bundle", ".dll", ".dylib", ".o", ".a"],
        Kind::Cpan => &[
            ".xs", ".c", ".cc", ".cpp", ".h", ".so", ".bundle", ".dll", ".dylib",
            ".bs", ".o", ".a",
        ],
        Kind::Packagist => &[".so", ".bundle", ".dll", ".dylib", ".phar"],
    }
}

fn copy_tree(kind: Kind, source: &Path, destination: &Path) -> Result<(), ProviderError> {
    std::fs::create_dir_all(destination).map_err(|error| {
        fail(
            kind,
            format!("could not create installed package tree: {error}"),
        )
    })?;
    for entry in std::fs::read_dir(source)
        .map_err(|error| fail(kind, format!("could not read package tree: {error}")))?
    {
        let entry =
            entry.map_err(|error| fail(kind, format!("could not read package tree: {error}")))?;
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|error| fail(kind, format!("could not inspect package tree: {error}")))?;
        let target = destination.join(entry.file_name());
        if metadata.file_type().is_symlink() {
            return Err(fail(kind, "package tree contains a symbolic link"));
        }
        if metadata.is_dir() {
            copy_tree(kind, &entry.path(), &target)?;
        } else if metadata.is_file() {
            std::fs::copy(entry.path(), target)
                .map_err(|error| fail(kind, format!("could not install package file: {error}")))?;
        } else {
            return Err(fail(kind, "package tree contains a special file"));
        }
    }
    Ok(())
}

fn archive_root(kind: Kind, unpack: &Path) -> Result<PathBuf, ProviderError> {
    let entries = std::fs::read_dir(unpack)
        .map_err(|error| fail(kind, format!("could not inspect unpacked source: {error}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| fail(kind, format!("could not inspect unpacked source: {error}")))?;
    if entries.len() == 1 && entries[0].path().is_dir() {
        Ok(entries[0].path())
    } else {
        Ok(unpack.to_path_buf())
    }
}

fn verify_integrity(kind: Kind, package: &Package, bytes: &[u8]) -> Result<(), ProviderError> {
    let (algorithm, expected, actual) = match &package.integrity {
        Integrity::Sha256(expected) => ("SHA-256", expected.clone(), SHA256::sha256_hex(bytes)),
        Integrity::Sha1(expected) => ("SHA-1", expected.clone(), sha1_hex(bytes)),
        Integrity::ImmutableGit { repository, reference } => {
            if bytes.is_empty() {
                return Err(fail(
                    kind,
                    format!("empty source fetched for immutable `{repository}` reference `{reference}`"),
                ));
            }
            return Ok(());
        }
    };
    if expected == actual {
        Ok(())
    } else {
        Err(fail(
            kind,
            format!(
                "{} source hash mismatch for `{}` {} (metadata {}, fetched {})",
                algorithm, package.name, package.version, expected, actual
            ),
        ))
    }
}

fn dependency_objects(
    root: &str,
    artifacts: &[Artifact],
) -> (Vec<String>, BTreeMap<String, String>) {
    let mut references = Vec::new();
    let mut facts = BTreeMap::new();
    for artifact in artifacts
        .iter()
        .filter(|artifact| artifact.package.name != root)
    {
        let digest = format!("sha256-{}", artifact.sha256);
        facts.insert(
            format!("dependency.object.{digest}"),
            format!(
                "sources/{}",
                artifact
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ),
        );
        references.push(digest);
    }
    (references, facts)
}

fn closure_hash(kind: Kind, artifacts: &[Artifact]) -> String {
    let mut identity = format!("jet-{}-source-v1\0", kind.label()).into_bytes();
    for artifact in artifacts {
        for value in [
            artifact.package.name.as_str(),
            artifact.package.version.as_str(),
            artifact.sha256.as_str(),
        ] {
            identity.extend_from_slice(value.as_bytes());
            identity.push(0);
        }
    }
    SHA256::sha256_hex(&identity)
}

fn render_provenance(
    kind: Kind,
    repository: &str,
    fetch_authority: &str,
    source_hash: &str,
    artifacts: &[Artifact],
) -> String {
    let mut out = format!(
        "schema=jet-{}-provider-v1\nrepository={repository}\nfetch_authority={fetch_authority}\nsource_hash={source_hash}\nscripts=disabled\nplugins=disabled\n",
        kind.label()
    );
    for artifact in artifacts {
        out.push_str(&format!(
            "package={}:{}:{}\n",
            artifact.package.name, artifact.package.version, artifact.sha256
        ));
    }
    out
}

fn archive_suffix(kind: Kind) -> &'static str {
    match kind {
        Kind::RubyGems => ".gem",
        Kind::Cpan => ".tar.gz",
        Kind::Packagist => ".zip",
    }
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn safe_piece(value: &str, punctuation: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || punctuation.contains(character))
}

fn safe_filename(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn absolutize(repository: &str, url: &str) -> String {
    if url.contains("://") {
        url.to_string()
    } else {
        format!(
            "{}/{}",
            repository.trim_end_matches('/'),
            url.trim_start_matches('/')
        )
    }
}

fn url_piece(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn php_quote(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(store: &Path, kind: Kind) -> Result<Self, ProviderError> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = store.join(super::BUILD_SCRATCH_DIR).join(format!(
            "{}-{}-{nonce}",
            kind.label(),
            std::process::id()
        ));
        std::fs::create_dir_all(&path)
            .map_err(|error| fail(kind, format!("could not create provider scratch: {error}")))?;
        Ok(Self { path })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn version_satisfies(actual: &str, requirement: &str) -> bool {
    let requirement = requirement.trim();
    if requirement.is_empty() || matches!(requirement, "*" | ">= 0" | ">=0") {
        return true;
    }
    requirement.split("||").any(|alternative| {
        let normalized = alternative.replace(',', " ");
        let tokens = normalized.split_whitespace().collect::<Vec<_>>();
        let mut index = 0;
        while index < tokens.len() {
            let token = tokens[index];
            let (operator, wanted, consumed) = if is_operator(token) {
                let Some(wanted) = tokens.get(index + 1) else {
                    return false;
                };
                (token, *wanted, 2)
            } else {
                let (operator, wanted) = [">=", "<=", "!=", "==", "~>", "=", ">", "<", "~", "^"]
                    .into_iter()
                    .find_map(|operator| {
                        token
                            .strip_prefix(operator)
                            .map(|wanted| (operator, wanted))
                    })
                    .unwrap_or(("=", token));
                (operator, wanted, 1)
            };
            if wanted.is_empty() || !satisfies_one(actual, operator, wanted.trim_start_matches('v'))
            {
                return false;
            }
            index += consumed;
        }
        true
    })
}

fn cpan_requirement(requirement: &str) -> String {
    let requirement = requirement.trim();
    if requirement.is_empty() || requirement == "0" {
        ">= 0".into()
    } else if is_operator(requirement)
        || [">=", "<=", "!=", "==", "~>", "=", ">", "<", "~", "^"]
            .into_iter()
            .any(|operator| requirement.starts_with(operator))
    {
        requirement.into()
    } else {
        format!(">= {requirement}")
    }
}

fn is_operator(value: &str) -> bool {
    matches!(
        value,
        ">=" | "<=" | "!=" | "==" | "=" | ">" | "<" | "~>" | "~" | "^"
    )
}

fn satisfies_one(actual: &str, operator: &str, wanted: &str) -> bool {
    use std::cmp::Ordering;
    let actual = actual.trim_start_matches('v');
    let ordering = compare_versions(actual, wanted);
    match operator {
        ">=" => ordering != Ordering::Less,
        ">" => ordering == Ordering::Greater,
        "<=" => ordering != Ordering::Greater,
        "<" => ordering == Ordering::Less,
        "!=" => ordering != Ordering::Equal,
        "=" | "==" => ordering == Ordering::Equal,
        "~" | "~>" => {
            ordering != Ordering::Less
                && upper_bound(wanted, operator == "~>")
                    .is_some_and(|upper| compare_versions(actual, &upper) == Ordering::Less)
        }
        "^" => {
            ordering != Ordering::Less
                && caret_upper_bound(wanted)
                    .is_some_and(|upper| compare_versions(actual, &upper) == Ordering::Less)
        }
        _ => false,
    }
}

fn numeric_parts(version: &str) -> Option<Vec<u64>> {
    version
        .split(['.', '-', '+'])
        .map(|part| part.parse::<u64>().ok())
        .collect()
}

fn upper_bound(version: &str, pessimistic: bool) -> Option<String> {
    let mut parts = numeric_parts(version)?;
    let index = if pessimistic && parts.len() > 1 {
        parts.len() - 2
    } else {
        0
    };
    parts[index] += 1;
    for part in parts.iter_mut().skip(index + 1) {
        *part = 0;
    }
    Some(
        parts
            .into_iter()
            .map(|part| part.to_string())
            .collect::<Vec<_>>()
            .join("."),
    )
}

fn caret_upper_bound(version: &str) -> Option<String> {
    let mut parts = numeric_parts(version)?;
    let index = parts
        .iter()
        .position(|part| *part != 0)
        .unwrap_or(parts.len().saturating_sub(1));
    parts[index] += 1;
    for part in parts.iter_mut().skip(index + 1) {
        *part = 0;
    }
    Some(
        parts
            .into_iter()
            .map(|part| part.to_string())
            .collect::<Vec<_>>()
            .join("."),
    )
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let left = left
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .collect::<Vec<_>>();
    let right = right
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .collect::<Vec<_>>();
    for index in 0..left.len().max(right.len()) {
        let a = left.get(index).copied().unwrap_or("0");
        let b = right.get(index).copied().unwrap_or("0");
        let ordering = match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            (Ok(_), Err(_)) => Ordering::Greater,
            (Err(_), Ok(_)) => Ordering::Less,
            (Err(_), Err(_)) => a.cmp(b),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn sha1_hex(input: &[u8]) -> String {
    let mut h = [
        0x67452301u32,
        0xEFCDAB89,
        0x98BADCFE,
        0x10325476,
        0xC3D2E1F0,
    ];
    let bit_len = (input.len() as u64) * 8;
    let mut bytes = input.to_vec();
    bytes.push(0x80);
    while bytes.len() % 64 != 56 {
        bytes.push(0);
    }
    bytes.extend_from_slice(&bit_len.to_be_bytes());
    for block in bytes.chunks_exact(64) {
        let mut words = [0u32; 80];
        for (index, chunk) in block.chunks_exact(4).enumerate() {
            words[index] = u32::from_be_bytes(chunk.try_into().unwrap_or([0; 4]));
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (index, word) in words.iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e]) {
            *slot = slot.wrapping_add(value);
        }
    }
    h.into_iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use std::fs;
    use std::process::Command;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn parses_native_registry_metadata_and_constraints() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert!(version_satisfies("2.4.1", "^2.0"));
        assert!(!version_satisfies("3.0.0", "^2.0"));
        assert!(version_satisfies("1.8.9", "~> 1.8"));
        assert!(!version_satisfies("2.0", "~> 1.8"));
        assert!(version_satisfies("1.5", ">=1.0 <2.0"));
        assert!(!version_satisfies("2.5", ">=1.0 <2.0"));
        assert_eq!(cpan_requirement("2.27300"), ">= 2.27300");

        let ruby = "2.0.0 jetdep:>= 1.0&< 2.0|checksum:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n";
        assert_eq!(
            parse_ruby_fixture("jetapp", ruby).unwrap()[0].dependencies[0].requirement,
            ">= 1.0,< 2.0"
        );
        assert!(parse_cpan_fixture(r#"{"distribution":"Jet-App","version":"2.0","download_url":"/Jet-App.tar.gz","checksum_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","dependency":[{"module":"Jet-Dep","phase":"runtime","relationship":"requires","version":">= 1.0"}]}"#).is_ok());
        assert!(parse_packagist_fixture("jet/app", r#"{"packages":{"jet/app":[{"version":"2.0.0","type":"library","dist":{"type":"zip","url":"/jet-app.zip","shasum":"a9993e364706816aba3e25717850c26c9cd0d89d"},"require":{"jet/dep":"^1.0"},"autoload":{"psr-4":{"Jet\\App\\":"src/"}}}]}}"#).is_ok());
        let minified = parse_packagist_fixture("jet/app", r#"{"minified":"composer/2.0","packages":{"jet/app":[{"name":"jet/app","version":"2.0.0","type":"library","dist":{"type":"zip","url":"/jet-app.zip","shasum":"a9993e364706816aba3e25717850c26c9cd0d89d"},"require":{"jet/dep":"^1.0"},"autoload":{"psr-4":{"Jet\\App\\":["missing/","src/"],"":["fallback/"]}}},{"version":"1.9.0","dist":{"type":"zip","url":"/jet-app-old.zip","shasum":"a9993e364706816aba3e25717850c26c9cd0d89d"},"require":"__unset"} ]}}"#).unwrap();
        assert_eq!(minified.len(), 2);
        assert!(minified[1].dependencies.is_empty());
        assert_eq!(
            minified[1].psr4.get("Jet\\App\\"),
            Some(&vec!["missing/".to_string(), "src/".to_string()])
        );
        let reference = "0123456789abcdef0123456789abcdef01234567";
        let immutable = parse_packagist_fixture("jet/app", &format!(r#"{{"packages":{{"jet/app":[{{"version":"2.0.0","type":"library","source":{{"type":"git","url":"https://github.com/jet/app.git","reference":"{reference}"}},"dist":{{"type":"zip","url":"https://api.github.com/repos/jet/app/zipball/{reference}","reference":"{reference}","shasum":""}},"autoload":{{"psr-4":{{"Jet\\App\\":"src/"}}}}}}]}}}}"#)).unwrap();
        assert!(matches!(&immutable[0].integrity, Integrity::ImmutableGit { repository, reference: locked } if repository == "https://github.com/jet/app.git" && locked == reference));
        assert!(parse_packagist_fixture("jet/app", &format!(r#"{{"packages":{{"jet/app":[{{"version":"2.0.0","type":"library","source":{{"type":"git","url":"https://github.com/attacker/jet-app.git","reference":"{reference}"}},"dist":{{"type":"zip","url":"https://api.github.com/repos/jet/app/zipball/{reference}?source=https://github.com/attacker/jet-app.git","reference":"{reference}","shasum":""}}}}]}}}}"#)).is_err());

        let tampered = Package {
            name: "jetapp".into(),
            version: "2.0.0".into(),
            url: "unused".into(),
            integrity: Integrity::Sha256("00".repeat(32)),
            dependencies: Vec::new(),
            psr4: BTreeMap::new(),
        };
        assert!(verify_integrity(Kind::RubyGems, &tampered, b"changed source").is_err());
    }

    #[test]
    fn solver_backtracks_across_diamond_and_cpan_enumerates_older_releases() {
        let dir = std::env::temp_dir().join(format!("jet-registry-solver-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("info")).unwrap();
        let hash = "a".repeat(64);
        fs::write(dir.join("info/root"), format!("1.0 a:>= 1&< 3,b:= 1|checksum:{hash}\n")).unwrap();
        fs::write(dir.join("info/a"), format!("2.0 c:>= 2|checksum:{hash}\n1.0 c:< 2|checksum:{hash}\n")).unwrap();
        fs::write(dir.join("info/b"), format!("1.0 c:< 2|checksum:{hash}\n")).unwrap();
        fs::write(dir.join("info/c"), format!("2.0 |checksum:{hash}\n1.0 |checksum:{hash}\n")).unwrap();
        let scratch = Scratch::new(&dir, Kind::RubyGems).unwrap();
        let ctx = Ctx { fixtures: None, store_dir: &dir, offline: false, project_dir: None };
        let authority = super::super::fetch::Authority::load(
            &ctx,
            "ruby",
            &format!("file://{}", dir.display()),
            Kind::RubyGems.fetch_authorities(),
        ).unwrap();
        let closure = resolve_closure(Kind::RubyGems, &authority, &scratch, "root", "1.0").unwrap();
        let selected = closure.iter().map(|package| (package.name.as_str(), package.version.as_str())).collect::<BTreeMap<_, _>>();
        assert_eq!(selected.get("a"), Some(&"1.0"));
        assert_eq!(selected.get("c"), Some(&"1.0"));

        let cpan = parse_cpan_search_fixture(r#"{"hits":{"hits":[{"_source":{"distribution":"Jet-App","version":"2.0","download_url":"/Jet-App-2.tar.gz","checksum_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","dependency":[]}},{"_source":{"distribution":"Jet-App","version":"1.0","download_url":"/Jet-App-1.tar.gz","checksum_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","dependency":[]}}]}}"#).unwrap();
        assert_eq!(cpan.iter().map(|package| package.version.as_str()).collect::<Vec<_>>(), vec!["2.0", "1.0"]);
        assert!(cpan.iter().any(|package| version_satisfies(&package.version, "=1.0")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn real_registry_closures_execute_lock_replay_offline_and_reject_tamper() {
        for tool in ["curl", "tar", "gzip", "ruby", "perl", "php"] {
            assert!(which(tool).is_some(), "Nix dev shell must provision {tool}");
        }
        assert!(Command::new("php")
            .args(["-r", "exit(class_exists('ZipArchive') ? 0 : 1);"])
            .status()
            .is_ok_and(|status| status.success()), "Nix dev shell PHP must provide ZipArchive");
        let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let base = std::env::temp_dir().join(format!("jet-script-registry-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();

        let ruby_repo = base.join("rubygems");
        let ruby_dep = write_gem(
            &base,
            &ruby_repo,
            "jetdep",
            "1.0.0",
            "module JetDep; def self.value = 41; end\n",
        );
        let ruby_app = write_gem(
            &base,
            &ruby_repo,
            "jetapp",
            "2.0.0",
            "require 'jetdep'\nmodule JetApp; def self.value = JetDep.value + 1; end\n",
        );
        fs::create_dir_all(ruby_repo.join("info")).unwrap();
        fs::write(
            ruby_repo.join("info/jetdep"),
            format!("1.0.0 |checksum:{ruby_dep}\n"),
        )
        .unwrap();
        fs::write(
            ruby_repo.join("info/jetapp"),
            format!("2.0.0 jetdep:>= 1.0&< 2.0|checksum:{ruby_app}\n"),
        )
        .unwrap();

        let cpan_repo = base.join("cpan");
        let cpan_dep = write_cpan(
            &base,
            &cpan_repo,
            "Jet-Dep",
            "1.0",
            "JetDep.pm",
            "package JetDep; sub value { 41 } 1;\n",
        );
        let cpan_app = write_cpan(
            &base,
            &cpan_repo,
            "Jet-App",
            "2.0",
            "JetApp.pm",
            "package JetApp; use JetDep; sub value { JetDep::value() + 1 } 1;\n",
        );
        write_cpan_metadata(&cpan_repo, "Jet-Dep", "1.0", &cpan_dep, &[]);
        write_cpan_metadata(
            &cpan_repo,
            "Jet-App",
            "2.0",
            &cpan_app,
            &[("Jet-Dep", ">= 1.0")],
        );

        let php_repo = base.join("packagist");
        let php_dep = write_zip(&base, &php_repo, "jet-dep", "src/Value.php", "<?php namespace Jet\\Dep; final class Value { public static function get(): int { return 41; } }\n");
        let php_app = write_zip(&base, &php_repo, "jet-app", "src/Value.php", "<?php namespace Jet\\App; final class Value { public static function get(): int { return \\Jet\\Dep\\Value::get() + 1; } }\n");
        write_packagist_metadata(&php_repo, "jet/dep", "1.0.0", &php_dep, None, "Jet\\Dep\\");
        write_packagist_metadata(
            &php_repo,
            "jet/app",
            "2.0.0",
            &php_app,
            Some(("jet/dep", "^1.0")),
            "Jet\\App\\",
        );

        *TEST_REPOSITORIES.write().unwrap() = BTreeMap::from([
            ("ruby", format!("file://{}", ruby_repo.display())),
            ("perl", format!("file://{}", cpan_repo.display())),
            ("php", format!("file://{}", php_repo.display())),
        ]);

        exercise_provider(
            &base,
            "ruby",
            "ruby:jetapp#version=2.0.0",
            "ruby-lib",
            "ruby",
            &["-e", "require 'jetapp'; print JetApp.value"],
        );
        exercise_provider(
            &base,
            "perl",
            "perl:Jet-App#version=2.0",
            "perl5lib",
            "perl",
            &["-MJetApp", "-e", "print JetApp::value()"],
        );
        exercise_provider(
            &base,
            "php",
            "php:jet/app#version=2.0.0",
            "composer-autoload",
            "php",
            &[
                "-r",
                "require getenv('COMPOSER_AUTOLOAD'); echo Jet\\App\\Value::get();",
            ],
        );

        let original_dir = std::env::current_dir().unwrap();
        let compose_project = base.join("compose-project");
        fs::create_dir_all(&compose_project).unwrap();
        std::env::set_current_dir(&compose_project).unwrap();
        let compose_roots = Store::Roots {
            root: base.join("compose-hangar"),
            dev_mode: true,
        };
        let table = SourceTable::empty();
        let refs = [
            "ruby:jetapp#version=2.0.0",
            "perl:Jet-App#version=2.0",
            "php:jet/app#version=2.0.0",
        ]
        .into_iter()
        .map(|reference| crate::RefSpec::classify_in(reference, &table).unwrap())
        .collect();
        let env = crate::CLI::compose_refs_for_test(&compose_roots, refs).unwrap();
        for variable in ["GEM_HOME", "GEM_PATH", "RUBYLIB", "PERL5LIB", "COMPOSER_AUTOLOAD"] {
            let value = env.vars.get(variable).unwrap_or_else(|| panic!("compose_env omitted {variable}"));
            assert!(value.split(crate::Platform::path_separator()).all(|path| Path::new(path).is_absolute()), "{variable} was not canonical: {value}");
        }
        for (runtime, variable, args) in [
            ("ruby", "RUBYLIB", &["-e", "require 'jetapp'; print JetApp.value"][..]),
            ("perl", "PERL5LIB", &["-MJetApp", "-e", "print JetApp::value()"][..]),
            ("php", "COMPOSER_AUTOLOAD", &["-r", "require getenv('COMPOSER_AUTOLOAD'); echo Jet\\App\\Value::get();"][..]),
        ] {
            let output = Command::new(runtime).args(args).env(variable, &env.vars[variable]).output().unwrap();
            assert!(output.status.success(), "{runtime}: {}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout), "42");
        }
        std::env::set_current_dir(original_dir).unwrap();

        *TEST_REPOSITORIES.write().unwrap() = BTreeMap::new();
        let _ = make_writable(&base);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    #[cfg(unix)]
    fn hostile_archives_reject_runtime_native_extensions() {
        for tool in ["tar", "gzip", "php"] {
            assert!(which(tool).is_some(), "Nix dev shell must provision {tool}");
        }
        assert!(Command::new("php")
            .args(["-r", "exit(class_exists('ZipArchive') ? 0 : 1);"])
            .status()
            .is_ok_and(|status| status.success()), "Nix dev shell PHP must provide ZipArchive");

        let base = std::env::temp_dir().join(format!("jet-script-native-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();

        let ruby_repo = base.join("rubygems");
        write_gem_file(&base, &ruby_repo, "badgem", "1.0", "lib/bad.so", b"native");
        let ruby_archive = ruby_repo.join("gems/badgem-1.0.gem");
        let ruby = test_artifact(Kind::RubyGems, "badgem", "1.0", ruby_archive);
        let ruby_scratch = Scratch::new(&base, Kind::RubyGems).unwrap();
        assert!(install_gem(&ruby, &base.join("ruby-out"), &ruby_scratch).is_err());

        let cpan_repo = base.join("cpan");
        write_cpan(&base, &cpan_repo, "Bad-Cpan", "1.0", "Bad.so", "native");
        let cpan_archive = cpan_repo.join("authors/Bad-Cpan-1.0.tar.gz");
        let cpan = test_artifact(Kind::Cpan, "Bad-Cpan", "1.0", cpan_archive);
        let cpan_scratch = Scratch::new(&base, Kind::Cpan).unwrap();
        assert!(install_cpan(&cpan, &base.join("cpan-out"), &cpan_scratch).is_err());

        let php_repo = base.join("packagist");
        write_zip(&base, &php_repo, "bad-php", "ext/bad.so", "native");
        let php_archive = php_repo.join("dist/bad-php.zip");
        let php = test_artifact(Kind::Packagist, "bad/php", "1.0", php_archive);
        let php_scratch = Scratch::new(&base, Kind::Packagist).unwrap();
        assert!(install_composer(&php, &base.join("php-out"), &php_scratch).is_err());

        let _ = fs::remove_dir_all(base);
    }

    fn test_artifact(_kind: Kind, name: &str, version: &str, path: PathBuf) -> Artifact {
        Artifact {
            sha256: SHA256::sha256_hex(&fs::read(&path).unwrap()),
            path,
            package: Package {
                name: name.into(),
                version: version.into(),
                url: String::new(),
                integrity: Integrity::Sha256("0".repeat(64)),
                dependencies: Vec::new(),
                psr4: BTreeMap::new(),
            },
        }
    }

    fn exercise_provider(
        base: &Path,
        label: &str,
        reference: &str,
        projection: &str,
        runtime: &str,
        args: &[&str],
    ) {
        let project = base.join(format!("{label}-project"));
        let roots = Store::Roots {
            root: base.join(format!("{label}-hangar")),
            dev_mode: true,
        };
        let store = roots.hangar_dir().join("provider-output");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&store).unwrap();
        let table = SourceTable::empty();
        let spec = crate::RefSpec::classify_in(reference, &table).unwrap();
        let online = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
            project_dir: Some(&project),
        };
        let realized = Store::realize_verified(
            &roots,
            &online,
            Store::RealizeRequest::Package {
                spec: &spec,
                table: &table,
            },
        )
        .unwrap();
        let root = realized.original_output();
        let value = fs::read_to_string(root.join(projection)).unwrap();
        let value = value
            .trim()
            .split(crate::Platform::path_separator())
            .map(|path| {
                let path = Path::new(path);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    root.join(path)
                }
                .to_string_lossy()
                .into_owned()
            })
            .collect::<Vec<_>>()
            .join(&crate::Platform::path_separator().to_string());
        let variable = match label {
            "ruby" => "RUBYLIB",
            "perl" => "PERL5LIB",
            "php" => "COMPOSER_AUTOLOAD",
            _ => unreachable!(),
        };
        let output = Command::new(runtime)
            .args(args)
            .env(variable, value)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42");
        let lock = fs::read_to_string(project.join(crate::Syntax::UNIFIED_LOCK_FILE)).unwrap();
        assert!(lock.contains(&format!("registry = \"{label}\"")), "{lock}");
        assert!(lock.contains("source-hash = \""));
        assert!(lock.contains("dependencies = ["));
        let provenance = fs::read_to_string(
            realized
                .original_output()
                .join(format!("{label}.provenance")),
        )
        .unwrap();
        assert!(provenance.contains("scripts=disabled\nplugins=disabled\n"));
        assert!(
            Store::find_by_reference(&roots, reference).is_some(),
            "{label}: Hangar entry missing"
        );
        assert!(
            crate::Lock::registry_realization(&project, label, reference).is_some(),
            "{label}: registry lock did not round-trip\n{lock}"
        );

        let offline = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: true,
            project_dir: Some(&project),
        };
        let replay = Store::realize_verified(
            &roots,
            &offline,
            Store::RealizeRequest::Package {
                spec: &spec,
                table: &table,
            },
        )
        .unwrap();
        assert_eq!(replay.source_state(), SourceState::Cached);

        let target = replay.original_output().join(format!("{label}.provenance"));
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o644);
        fs::set_permissions(&target, permissions).unwrap();
        fs::write(&target, "tampered\n").unwrap();
        assert!(matches!(
            Store::realize_verified(
                &roots,
                &offline,
                Store::RealizeRequest::Package {
                    spec: &spec,
                    table: &table
                },
            ),
            Err(Store::RealizeError::Integrity(_))
        ));
    }

    fn parse_ruby_fixture(name: &str, raw: &str) -> Result<Vec<Package>, ProviderError> {
        let dir = std::env::temp_dir().join(format!("jet-ruby-info-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("info")).unwrap();
        std::fs::write(dir.join("info").join(name), raw).unwrap();
        let scratch = Scratch::new(&dir, Kind::RubyGems).unwrap();
        let ctx = Ctx { fixtures: None, store_dir: &dir, offline: false, project_dir: None };
        let authority = super::super::fetch::Authority::load(
            &ctx,
            Kind::RubyGems.label(),
            &format!("file://{}", dir.display()),
            Kind::RubyGems.fetch_authorities(),
        ).unwrap();
        let result = fetch_rubygems(&authority, &scratch, name);
        let _ = std::fs::remove_dir_all(dir);
        result
    }

    fn parse_cpan_fixture(raw: &str) -> Result<Vec<Package>, ProviderError> {
        parse_cpan_search_fixture(&format!(r#"{{"hits":{{"hits":[{{"_source":{raw}}}]}}}}"#))
    }

    fn parse_cpan_search_fixture(raw: &str) -> Result<Vec<Package>, ProviderError> {
        let dir = std::env::temp_dir().join(format!("jet-cpan-info-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("v1/release")).unwrap();
        std::fs::write(
            dir.join("v1/release/_search-Jet-App"),
            raw,
        ).unwrap();
        let scratch = Scratch::new(&dir, Kind::Cpan).unwrap();
        let ctx = Ctx { fixtures: None, store_dir: &dir, offline: false, project_dir: None };
        let authority = super::super::fetch::Authority::load(
            &ctx,
            Kind::Cpan.label(),
            &format!("file://{}", dir.display()),
            Kind::Cpan.fetch_authorities(),
        ).unwrap();
        let result = fetch_cpan(&authority, &scratch, "Jet-App");
        let _ = std::fs::remove_dir_all(dir);
        result
    }

    fn parse_packagist_fixture(name: &str, raw: &str) -> Result<Vec<Package>, ProviderError> {
        let dir = std::env::temp_dir().join(format!("jet-php-info-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("p2").join(format!("{name}.json"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, raw).unwrap();
        let scratch = Scratch::new(&dir, Kind::Packagist).unwrap();
        let ctx = Ctx { fixtures: None, store_dir: &dir, offline: false, project_dir: None };
        let authority = super::super::fetch::Authority::load(
            &ctx,
            Kind::Packagist.label(),
            &format!("file://{}", dir.display()),
            Kind::Packagist.fetch_authorities(),
        ).unwrap();
        let result = fetch_packagist(&authority, &scratch, name);
        let _ = std::fs::remove_dir_all(dir);
        result
    }

    fn which(tool: &str) -> Option<PathBuf> {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|directory| directory.join(tool))
            .find(|path| path.is_file())
    }

    fn write_gem(base: &Path, repo: &Path, name: &str, version: &str, code: &str) -> String {
        write_gem_file(
            base,
            repo,
            name,
            version,
            &format!("lib/{name}.rb"),
            code.as_bytes(),
        )
    }

    fn write_gem_file(
        base: &Path,
        repo: &Path,
        name: &str,
        version: &str,
        relative: &str,
        contents: &[u8],
    ) -> String {
        let source = base.join(format!("gem-source-{name}"));
        let payload = base.join(format!("gem-payload-{name}.tar.gz"));
        let archive = repo.join("gems").join(format!("{name}-{version}.gem"));
        let file = source.join(relative);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        fs::write(file, contents).unwrap();
        assert!(Command::new("tar")
            .args(["-czf"])
            .arg(&payload)
            .arg("-C")
            .arg(&source)
            .arg("lib")
            .status()
            .unwrap()
            .success());
        let container = base.join(format!("gem-container-{name}"));
        fs::create_dir_all(&container).unwrap();
        fs::copy(&payload, container.join("data.tar.gz")).unwrap();
        assert!(Command::new("tar")
            .args(["-cf"])
            .arg(&archive)
            .arg("-C")
            .arg(&container)
            .arg("data.tar.gz")
            .status()
            .unwrap()
            .success());
        SHA256::sha256_hex(&fs::read(archive).unwrap())
    }

    fn write_cpan(
        base: &Path,
        repo: &Path,
        name: &str,
        version: &str,
        module: &str,
        code: &str,
    ) -> String {
        let source_root = base.join(format!("cpan-source-{name}"));
        let distribution = source_root.join(format!("{name}-{version}"));
        fs::create_dir_all(distribution.join("lib")).unwrap();
        fs::write(distribution.join("lib").join(module), code).unwrap();
        let archive = repo
            .join("authors")
            .join(format!("{name}-{version}.tar.gz"));
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        assert!(Command::new("tar")
            .args(["-czf"])
            .arg(&archive)
            .arg("-C")
            .arg(&source_root)
            .arg(format!("{name}-{version}"))
            .status()
            .unwrap()
            .success());
        SHA256::sha256_hex(&fs::read(archive).unwrap())
    }

    fn write_cpan_metadata(
        repo: &Path,
        name: &str,
        version: &str,
        sha256: &str,
        dependencies: &[(&str, &str)],
    ) {
        fs::create_dir_all(repo.join("v1/release")).unwrap();
        let deps = dependencies
            .iter()
            .map(|(dependency, requirement)| {
                format!(
                    "{{\"module\":\"{dependency}\",\"phase\":\"runtime\",\"relationship\":\"requires\",\"version\":\"{requirement}\"}}"
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        fs::write(
            repo.join("v1/release").join(format!("_search-{name}")),
            format!(
                "{{\"hits\":{{\"hits\":[{{\"_source\":{{\"distribution\":\"{name}\",\"version\":\"{version}\",\"download_url\":\"/authors/{name}-{version}.tar.gz\",\"checksum_sha256\":\"{sha256}\",\"dependency\":[{deps}]}}}}]}}}}"
            ),
        )
        .unwrap();
    }

    fn write_zip(base: &Path, repo: &Path, name: &str, relative: &str, code: &str) -> String {
        let source = base.join(format!("zip-source-{name}"));
        let package_root = source.join(name);
        let file = package_root.join(relative);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, code).unwrap();
        let archive = repo.join("dist").join(format!("{name}.zip"));
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        const CREATE_ZIP: &str = "$z=new ZipArchive(); if($z->open($argv[2],ZipArchive::CREATE|ZipArchive::OVERWRITE)!==true) exit(2); $root=rtrim($argv[1],DIRECTORY_SEPARATOR).DIRECTORY_SEPARATOR; $it=new RecursiveIteratorIterator(new RecursiveDirectoryIterator($argv[1],FilesystemIterator::SKIP_DOTS)); foreach($it as $f){$z->addFile($f->getPathname(),substr($f->getPathname(),strlen($root)));} if(!$z->close()) exit(3);";
        assert!(Command::new("php")
            .args(["-r", CREATE_ZIP, "--"])
            .arg(&source)
            .arg(&archive)
            .status()
            .unwrap()
            .success());
        sha1_hex(&fs::read(archive).unwrap())
    }

    fn write_packagist_metadata(
        repo: &Path,
        name: &str,
        version: &str,
        sha1: &str,
        dependency: Option<(&str, &str)>,
        prefix: &str,
    ) {
        let path = repo.join("p2").join(format!("{name}.json"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let require = dependency
            .map(|(dep, requirement)| format!(",\"require\":{{\"{dep}\":\"{requirement}\"}}"))
            .unwrap_or_default();
        let archive = name.replace('/', "-");
        fs::write(
            path,
            format!(
                "{{\"packages\":{{\"{name}\":[{{\"version\":\"{version}\",\"type\":\"library\",\"dist\":{{\"type\":\"zip\",\"url\":\"/dist/{archive}.zip\",\"shasum\":\"{sha1}\"}}{require},\"autoload\":{{\"psr-4\":{{\"{escaped_prefix}\":[\"missing/\",\"src/\"]}}}}}}]}}}}",
                escaped_prefix = prefix.replace('\\', "\\\\")
            ),
        )
        .unwrap();
    }

    fn make_writable(path: &Path) -> std::io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                make_writable(&entry?.path())?;
            }
        }
        let mut permissions = metadata.permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() | 0o700);
        fs::set_permissions(path, permissions)
    }
}
