//! Native LuaRocks provider (D-FFI-LUA1, D-JPK-PROVIDERS2).

use super::{cache_identity, Ctx, Provider, ProviderError, Realized, SourceState};
use crate::RefSpec::{RefSpec, SourceTable};
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub(super) const RECIPE_ID: &str = "luarocks-provider-v1";
const DEFAULT_REPOSITORY: &str = "https://luarocks.org";

#[derive(Debug, Clone)]
struct Rockspec {
    name: String,
    version: String,
    source_url: String,
    source_hash: String,
    dependencies: Vec<Dependency>,
    modules: BTreeMap<String, String>,
    platforms: Vec<String>,
}

#[derive(Debug, Clone)]
struct Dependency {
    name: String,
    requirements: Vec<(String, String)>,
}

#[derive(Debug)]
struct Artifact {
    spec: Rockspec,
    rockspec_path: PathBuf,
    source_path: PathBuf,
    rockspec_hash: String,
    source_hash: String,
}

pub(super) struct LuaRocksProvider;

impl Provider for LuaRocksProvider {
    fn realize(
        &self,
        spec: &RefSpec,
        _table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Realized, ProviderError> {
        if ctx.offline {
            return Err(ProviderError::Offline(format!(
                "`{}` is not in the verified hangar and --offline forbids LuaRocks metadata or source fetches",
                spec.raw
            )));
        }
        let repository = repository();
        let (root_name, root_version) = parse_ref(&spec.package)?;
        let scratch = Scratch::new(ctx.store_dir)?;
        let manifest_path = scratch.path.join("manifest-5.4");
        download(&format!("{repository}/manifest-5.4"), &manifest_path)?;
        let manifest = std::fs::read_to_string(&manifest_path)
            .map_err(|e| error(format!("could not read LuaRocks manifest: {e}")))?;
        require_manifest_version(&manifest, root_name, root_version)?;

        let mut specs = BTreeMap::new();
        resolve_rockspec(
            root_name,
            root_version,
            &repository,
            &manifest,
            &scratch,
            &mut specs,
            &mut BTreeSet::new(),
        )?;
        let order = dependency_order(root_name, &specs)?;
        let mut artifacts = Vec::new();
        for name in &order {
            let rockspec = specs
                .get(name)
                .ok_or_else(|| {
                    error(format!(
                        "resolved LuaRocks dependency `{name}` has no rockspec"
                    ))
                })?
                .clone();
            let rockspec_path = scratch
                .path
                .join(format!("{}-{}.rockspec", rockspec.name, rockspec.version));
            let rockspec_bytes = std::fs::read(&rockspec_path)
                .map_err(|e| error(format!("could not reread rockspec: {e}")))?;
            let source_name = safe_url_basename(&rockspec.source_url)?;
            let source_path = scratch.path.join(format!("{}-{source_name}", rockspec.name));
            download(&rockspec.source_url, &source_path)?;
            let source_bytes = std::fs::read(&source_path)
                .map_err(|e| error(format!("could not read LuaRocks source: {e}")))?;
            let source_hash = SHA256::sha256_hex(&source_bytes);
            if source_hash != rockspec.source_hash {
                return Err(error(format!(
                    "LuaRocks source hash mismatch for `{}` (rockspec {}, fetched {})",
                    rockspec.name, rockspec.source_hash, source_hash
                )));
            }
            artifacts.push(Artifact {
                spec: rockspec,
                rockspec_path,
                source_path,
                rockspec_hash: SHA256::sha256_hex(&rockspec_bytes),
                source_hash,
            });
        }
        let source_hash = closure_hash(&artifacts);
        if let Some(project) = ctx.project_dir {
            if let Some((_output, locked_hash, locked_repo, _)) =
                crate::Lock::luarocks_realization(project, &spec.raw)
            {
                if locked_hash != source_hash || locked_repo != repository {
                    return Err(error(format!(
                        "locked LuaRocks source integrity changed for `{}` (expected {} from {}, got {} from {})",
                        spec.raw, locked_hash, locked_repo, source_hash, repository
                    )));
                }
            }
        }

        let root = specs
            .get(root_name)
            .ok_or_else(|| {
                error(format!(
                    "resolved LuaRocks root `{root_name}` has no rockspec"
                ))
            })?;
        let out_dir = ctx.store_dir.join(format!(
            "{}-{}-{}",
            root.name,
            root.version,
            &source_hash[..12]
        ));
        if out_dir.exists() {
            return Err(error(format!(
                "unverified existing output {}; run `jet clean` before rebuilding",
                out_dir.display()
            )));
        }
        let lua_dir = out_dir.join("share/lua/5.4");
        let c_dir = out_dir.join("lib/lua/5.4");
        let sources = out_dir.join("sources");
        std::fs::create_dir_all(&lua_dir)
            .and_then(|_| std::fs::create_dir_all(&c_dir))
            .and_then(|_| std::fs::create_dir_all(&sources))
            .map_err(|e| error(format!("could not stage LuaRocks output: {e}")))?;
        for artifact in &artifacts {
            preserve_and_install(artifact, &sources, &lua_dir, &scratch)?;
        }
        write_runtime_files(&out_dir, &lua_dir, &c_dir)?;
        std::fs::write(
            out_dir.join("luarocks.provenance"),
            render_provenance(&repository, &source_hash, &artifacts),
        )
        .map_err(|e| error(format!("could not write LuaRocks provenance: {e}")))?;
        crate::Store::seal_local_output(&out_dir)
            .map_err(|e| error(format!("could not seal LuaRocks output: {e}")))?;
        let out = out_dir.to_string_lossy().into_owned();
        let envelope = crate::Envelope::Envelope::for_output(&out, &spec.raw, RECIPE_ID);
        let mut deps = Vec::new();
        for name in order.iter().filter(|name| name.as_str() != root_name) {
            let dep = specs.get(name).ok_or_else(|| {
                error(format!(
                    "ordered LuaRocks dependency `{name}` has no rockspec"
                ))
            })?;
            deps.push(format!("{}#version={}", dep.name, dep.version));
        }
        if let Some(project) = ctx.project_dir {
            crate::Lock::record_luarocks_realization(
                project,
                &root.name,
                &root.version,
                &spec.raw,
                &out,
                &source_hash,
                &repository,
                deps,
                crate::Lock::LockEnvelope {
                    output_hash: envelope.output_hash.clone(),
                    platform: envelope.platform.clone(),
                    signature: envelope.signature.clone(),
                    provenance: envelope.provenance.clone(),
                },
            );
        }
        Ok(Realized {
            name: root.name.clone(),
            version: root.version.clone(),
            reference: spec.raw.clone(),
            out: out.clone(),
            bin: out_dir.join("bin").to_string_lossy().into_owned(),
            rlib: String::new(),
            envelope,
            cache_identity: cache_identity(&source_hash, RECIPE_ID, ctx),
            source_state: SourceState::Built,
        })
    }
}

fn error(message: impl Into<String>) -> ProviderError {
    ProviderError::LuaRocks(message.into())
}

fn repository() -> String {
    #[cfg(test)]
    if let Some(repository) = TEST_REPOSITORY.read().unwrap().clone() {
        return repository;
    }
    DEFAULT_REPOSITORY.to_string()
}

#[cfg(test)]
static TEST_REPOSITORY: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

fn parse_ref(raw: &str) -> Result<(&str, &str), ProviderError> {
    let (name, selector) = raw.split_once('#').unwrap_or((raw, ""));
    if !valid_component(name) {
        return Err(error(format!("invalid LuaRocks package name `{name}`")));
    }
    if selector.is_empty() {
        return Err(error(format!(
            "LuaRocks ref `{raw}` is mutable; use `{name}#version=<exact>`"
        )));
    }
    let version = selector.strip_prefix("version=").unwrap_or(selector);
    if !valid_component(version) {
        return Err(error(format!("invalid LuaRocks version `{version}`")));
    }
    Ok((name, version))
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        && value != "."
        && value != ".."
}

fn resolve_rockspec(
    name: &str,
    version: &str,
    repository: &str,
    manifest: &str,
    scratch: &Scratch,
    specs: &mut BTreeMap<String, Rockspec>,
    active: &mut BTreeSet<String>,
) -> Result<(), ProviderError> {
    if let Some(existing) = specs.get(name) {
        if existing.version != version {
            return Err(error(format!(
                "LuaRocks closure selects `{name}` as both {} and {version}",
                existing.version
            )));
        }
        return Ok(());
    }
    if !active.insert(name.to_string()) {
        return Err(error(format!("LuaRocks dependency cycle includes `{name}`")));
    }
    require_manifest_version(manifest, name, version)?;
    let path = scratch.path.join(format!("{name}-{version}.rockspec"));
    download(&format!("{repository}/{name}-{version}.rockspec"), &path)?;
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| error(format!("could not read `{name}` rockspec: {e}")))?;
    let spec = parse_rockspec(&raw)?;
    if spec.name != name || spec.version != version {
        return Err(error(format!(
            "rockspec identity drift: requested `{name}` `{version}`, got `{}` `{}`",
            spec.name, spec.version
        )));
    }
    ensure_platform_supported(&spec.platforms)?;
    let deps = spec.dependencies.clone();
    specs.insert(name.to_string(), spec);
    for dep in deps {
        if dep.name == "lua" {
            continue;
        }
        let selected = manifest_versions(manifest, &dep.name)?
            .into_iter()
            .filter(|candidate| requirements_satisfied(candidate, &dep.requirements))
            .max_by(|a, b| compare_versions(a, b))
            .ok_or_else(|| {
                error(format!(
                    "LuaRocks dependency `{}` has no manifest version satisfying {}",
                    dep.name,
                    render_requirements(&dep.requirements)
                ))
            })?;
        resolve_rockspec(
            &dep.name,
            &selected,
            repository,
            manifest,
            scratch,
            specs,
            active,
        )?;
    }
    active.remove(name);
    Ok(())
}

fn parse_rockspec(raw: &str) -> Result<Rockspec, ProviderError> {
    let name = assignment_string(raw, "package")?;
    let version = assignment_string(raw, "version")?;
    if !valid_component(&name) || !valid_component(&version) {
        return Err(error("rockspec contains unsafe package identity"));
    }
    let source = assignment_table(raw, "source")?;
    let source_url = table_string(source, "url")?;
    let source_hash = table_string(source, "sha256")?;
    if source_hash.len() != 64 || !source_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(error(format!("rockspec `{name}` needs a SHA-256 source hash")));
    }
    let dependencies = assignment_table_optional(raw, "dependencies")
        .map(parse_dependencies)
        .transpose()?
        .unwrap_or_default();
    let platforms = assignment_table_optional(raw, "supported_platforms")
        .map(string_values)
        .transpose()?
        .unwrap_or_default();
    let build = assignment_table(raw, "build")?;
    if table_string(build, "type")? != "builtin" {
        return Err(error(format!(
            "LuaRocks package `{name}` uses a native or command build; this provider currently accepts only auditable builtin module maps"
        )));
    }
    let modules = keyed_strings(table_table(build, "modules")?)?;
    if modules.is_empty() {
        return Err(error(format!("rockspec `{name}` has no installable modules")));
    }
    for (module, path) in &modules {
        if !valid_module_name(module) || !safe_relative(Path::new(path)) || !path.ends_with(".lua") {
            return Err(error(format!("rockspec `{name}` contains unsafe or native module mapping `{module}` = `{path}`")));
        }
    }
    Ok(Rockspec {
        name,
        version,
        source_url,
        source_hash: source_hash.to_ascii_lowercase(),
        dependencies,
        modules,
        platforms,
    })
}

fn parse_dependencies(raw: &str) -> Result<Vec<Dependency>, ProviderError> {
    let mut out = Vec::new();
    for item in string_values(raw)? {
        let words = item.split_whitespace().collect::<Vec<_>>();
        let Some(name) = words.first().copied() else { continue };
        if !valid_component(name) {
            return Err(error(format!("unsafe LuaRocks dependency `{name}`")));
        }
        let mut requirements = Vec::new();
        let rest = item[name.len()..].trim();
        for requirement in rest.split(',').map(str::trim).filter(|v| !v.is_empty()) {
            let mut parts = requirement.split_whitespace();
            let operator = parts.next().unwrap_or("");
            let wanted = parts.next().unwrap_or("");
            if parts.next().is_some()
                || !matches!(operator, ">=" | ">" | "<=" | "<" | "=" | "==" | "~>")
                || !valid_component(wanted)
            {
                return Err(error(format!("unsupported LuaRocks constraint `{requirement}`")));
            }
            requirements.push((operator.to_string(), wanted.to_string()));
        }
        out.push(Dependency { name: name.to_string(), requirements });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn dependency_order(root: &str, specs: &BTreeMap<String, Rockspec>) -> Result<Vec<String>, ProviderError> {
    fn visit(
        name: &str,
        specs: &BTreeMap<String, Rockspec>,
        active: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
        out: &mut Vec<String>,
    ) -> Result<(), ProviderError> {
        if done.contains(name) { return Ok(()); }
        if !active.insert(name.to_string()) {
            return Err(error(format!("LuaRocks dependency cycle includes `{name}`")));
        }
        let spec = specs.get(name).ok_or_else(|| error(format!("missing resolved LuaRocks dependency `{name}`")))?;
        for dep in &spec.dependencies {
            if dep.name != "lua" { visit(&dep.name, specs, active, done, out)?; }
        }
        active.remove(name);
        done.insert(name.to_string());
        out.push(name.to_string());
        Ok(())
    }
    let mut out = Vec::new();
    visit(root, specs, &mut BTreeSet::new(), &mut BTreeSet::new(), &mut out)?;
    Ok(out)
}

fn preserve_and_install(
    artifact: &Artifact,
    sources: &Path,
    lua_dir: &Path,
    scratch: &Scratch,
) -> Result<(), ProviderError> {
    std::fs::copy(
        &artifact.rockspec_path,
        sources.join(artifact.rockspec_path.file_name().unwrap_or_default()),
    )
    .and_then(|_| {
        std::fs::copy(
            &artifact.source_path,
            sources.join(artifact.source_path.file_name().unwrap_or_default()),
        )
    })
    .map_err(|e| error(format!("could not preserve LuaRocks source: {e}")))?;
    let unpack = scratch.path.join(format!("unpack-{}", artifact.spec.name));
    std::fs::create_dir_all(&unpack)
        .map_err(|e| error(format!("could not create LuaRocks unpack directory: {e}")))?;
    validate_archive(&artifact.source_path)?;
    let status = Command::new("tar")
        .args(["--extract", "--gzip", "--no-same-owner", "--no-same-permissions", "--file"])
        .arg(&artifact.source_path)
        .arg("-C")
        .arg(&unpack)
        .status()
        .map_err(|e| error(format!("could not start provisioned tar: {e}")))?;
    if !status.success() {
        return Err(error(format!("could not unpack source for `{}`", artifact.spec.name)));
    }
    let root = archive_root(&unpack)?;
    for (module, relative) in &artifact.spec.modules {
        let source = root.join(relative);
        if !source.is_file() {
            return Err(error(format!("module `{module}` source `{relative}` is missing")));
        }
        let target = lua_dir.join(format!("{}.lua", module.replace('.', "/")));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| error(format!("could not create module directory: {e}")))?;
        }
        std::fs::copy(&source, &target)
            .map_err(|e| error(format!("could not install module `{module}`: {e}")))?;
    }
    Ok(())
}

fn validate_archive(path: &Path) -> Result<(), ProviderError> {
    let output = Command::new("tar")
        .arg("-tzf")
        .arg(path)
        .output()
        .map_err(|e| error(format!("could not inspect LuaRocks archive: {e}")))?;
    if !output.status.success() {
        return Err(error("LuaRocks source is not a readable tar.gz archive"));
    }
    let listing = String::from_utf8(output.stdout)
        .map_err(|_| error("LuaRocks archive contains non-UTF-8 paths"))?;
    for entry in listing.lines() {
        if entry.is_empty() || !safe_relative(Path::new(entry)) {
            return Err(error(format!("LuaRocks archive contains unsafe path `{entry}`")));
        }
    }
    let verbose = Command::new("tar")
        .arg("-tvzf")
        .arg(path)
        .output()
        .map_err(|e| error(format!("could not inspect LuaRocks archive types: {e}")))?;
    if !verbose.status.success() {
        return Err(error("LuaRocks source archive types could not be inspected"));
    }
    let verbose = String::from_utf8(verbose.stdout)
        .map_err(|_| error("LuaRocks archive listing is not UTF-8"))?;
    for entry in verbose.lines() {
        if matches!(entry.as_bytes().first(), Some(b'l' | b'h')) {
            return Err(error("LuaRocks source archives may not contain symbolic or hard links"));
        }
    }
    Ok(())
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|component| matches!(component, Component::Normal(_)))
}

fn archive_root(unpack: &Path) -> Result<PathBuf, ProviderError> {
    let entries = std::fs::read_dir(unpack)
        .map_err(|e| error(format!("could not inspect unpacked source: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| error(format!("could not inspect unpacked source: {e}")))?;
    if entries.len() == 1 && entries[0].path().is_dir() {
        Ok(entries[0].path())
    } else {
        Ok(unpack.to_path_buf())
    }
}

fn write_runtime_files(out: &Path, lua_dir: &Path, c_dir: &Path) -> Result<(), ProviderError> {
    let lua_path = format!("{}/?.lua;{}/?/init.lua", lua_dir.display(), lua_dir.display());
    let lua_cpath = format!("{}/?.so", c_dir.display());
    std::fs::write(out.join("lua-path"), format!("{lua_path}\n"))
        .and_then(|_| std::fs::write(out.join("lua-cpath"), format!("{lua_cpath}\n")))
        .map_err(|e| error(format!("could not write Lua module search paths: {e}")))?;
    let lua = which("lua").ok_or_else(|| error("provisioned Lua 5.4 was not found"))?;
    let bin = out.join("bin");
    std::fs::create_dir_all(&bin)
        .map_err(|e| error(format!("could not create LuaRocks wrapper directory: {e}")))?;
    let wrapper = bin.join("lua-with-rocks");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nLUA_PATH='{};;' LUA_CPATH='{};;' exec '{}' \"$@\"\n",
            shell_quote(&lua_path), shell_quote(&lua_cpath), shell_quote(&lua.to_string_lossy())
        ),
    )
    .map_err(|e| error(format!("could not write Lua runtime wrapper: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| error(format!("could not make Lua runtime wrapper executable: {e}")))?;
    }
    Ok(())
}

fn shell_quote(value: &str) -> String { value.replace('\'', "'\\''") }

fn which(tool: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(tool))
        .find(|path| path.is_file())
}

fn download(url: &str, path: &Path) -> Result<(), ProviderError> {
    let status = Command::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error", "--max-time", "60", "--output"])
        .arg(path)
        .arg(url)
        .status()
        .map_err(|e| error(format!("could not start provisioned curl: {e}")))?;
    if status.success() { Ok(()) } else { Err(error(format!("could not fetch `{url}`"))) }
}

fn safe_url_basename(url: &str) -> Result<String, ProviderError> {
    let value = url.split('?').next().unwrap_or(url).rsplit('/').next().unwrap_or("");
    if !valid_component(value) && !value.ends_with(".tar.gz") {
        return Err(error(format!("LuaRocks source URL has unsafe archive name `{value}`")));
    }
    if value.is_empty() || value == "." || value == ".." || value.contains(['/', '\\']) {
        return Err(error(format!("LuaRocks source URL has unsafe archive name `{value}`")));
    }
    Ok(value.to_string())
}

fn closure_hash(artifacts: &[Artifact]) -> String {
    let mut identity = b"jet-luarocks-source-v1\0".to_vec();
    for artifact in artifacts {
        for value in [
            artifact.spec.name.as_str(),
            artifact.spec.version.as_str(),
            artifact.rockspec_hash.as_str(),
            artifact.source_hash.as_str(),
        ] {
            identity.extend_from_slice(value.as_bytes());
            identity.push(0);
        }
    }
    SHA256::sha256_hex(&identity)
}

fn render_provenance(repository: &str, source_hash: &str, artifacts: &[Artifact]) -> String {
    let mut out = format!("schema=jet-luarocks-provider-v1\nrepository={repository}\nsource_hash={source_hash}\nplatform={}\n", crate::Envelope::host_platform());
    for artifact in artifacts {
        out.push_str(&format!(
            "package={}:{}:rockspec={}:source={}\n",
            artifact.spec.name, artifact.spec.version, artifact.rockspec_hash, artifact.source_hash
        ));
    }
    out
}

fn ensure_platform_supported(platforms: &[String]) -> Result<(), ProviderError> {
    if platforms.is_empty() { return Ok(()); }
    let host = if cfg!(target_os = "windows") { "windows" } else if cfg!(target_os = "macos") { "macosx" } else { "linux" };
    let positive = platforms.iter().filter(|v| !v.starts_with('!')).collect::<Vec<_>>();
    let denied = platforms.iter().any(|v| v == &format!("!{host}") || (!cfg!(target_os = "windows") && v == "!unix"));
    let allowed = positive.is_empty() || positive.iter().any(|v| v.as_str() == host || (!cfg!(target_os = "windows") && v.as_str() == "unix"));
    if denied || !allowed {
        Err(error(format!("LuaRocks package does not support host platform `{host}`")))
    } else { Ok(()) }
}

fn requirements_satisfied(version: &str, requirements: &[(String, String)]) -> bool {
    requirements.iter().all(|(operator, wanted)| {
        let cmp = compare_versions(version, wanted);
        match operator.as_str() {
            ">=" => cmp.is_ge(), ">" => cmp.is_gt(), "<=" => cmp.is_le(), "<" => cmp.is_lt(),
            "=" | "==" => cmp.is_eq(),
            "~>" => cmp.is_ge() && version.split('.').next() == wanted.split('.').next(),
            _ => false,
        }
    })
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let parts = |value: &str| value.split(['.', '-', '_']).map(|v| v.to_string()).collect::<Vec<_>>();
    let left = parts(left); let right = parts(right);
    for i in 0..left.len().max(right.len()) {
        let a = left.get(i).map(String::as_str).unwrap_or("0");
        let b = right.get(i).map(String::as_str).unwrap_or("0");
        let cmp = match (a.parse::<u64>(), b.parse::<u64>()) { (Ok(a), Ok(b)) => a.cmp(&b), _ => a.cmp(b) };
        if !cmp.is_eq() { return cmp; }
    }
    std::cmp::Ordering::Equal
}

fn render_requirements(requirements: &[(String, String)]) -> String {
    if requirements.is_empty() { "any version".into() } else { requirements.iter().map(|(op, v)| format!("{op} {v}")).collect::<Vec<_>>().join(", ") }
}

// Rockspecs and manifests are Lua data files. These readers accept only quoted
// scalar/table fields needed by the provider; no Lua code is ever executed.
fn assignment_string(raw: &str, key: &str) -> Result<String, ProviderError> {
    let value = assignment_value(raw, key).ok_or_else(|| error(format!("rockspec has no `{key}` field")))?;
    parse_quoted(value).map(|v| v.0)
}

fn assignment_table<'a>(raw: &'a str, key: &str) -> Result<&'a str, ProviderError> {
    assignment_table_optional(raw, key).ok_or_else(|| error(format!("rockspec has no `{key}` table")))
}

fn assignment_table_optional<'a>(raw: &'a str, key: &str) -> Option<&'a str> {
    let value = assignment_value(raw, key)?;
    balanced_table(value).ok().map(|v| v.0)
}

fn assignment_value<'a>(raw: &'a str, key: &str) -> Option<&'a str> {
    for (offset, _) in raw.match_indices(key) {
        let before = raw[..offset].chars().next_back();
        let after = raw[offset + key.len()..].chars().next();
        let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
        if before.is_some_and(is_ident) || after.is_some_and(is_ident) { continue; }
        let tail = raw[offset + key.len()..].trim_start();
        if let Some(value) = tail.strip_prefix('=') {
            return Some(value.trim_start());
        }
    }
    None
}

fn table_string(raw: &str, key: &str) -> Result<String, ProviderError> {
    let value = assignment_value(raw, key).ok_or_else(|| error(format!("table has no `{key}` field")))?;
    parse_quoted(value).map(|v| v.0)
}

fn table_table<'a>(raw: &'a str, key: &str) -> Result<&'a str, ProviderError> {
    let value = assignment_value(raw, key).ok_or_else(|| error(format!("table has no `{key}` table")))?;
    balanced_table(value).map(|v| v.0)
}

fn parse_quoted(raw: &str) -> Result<(String, usize), ProviderError> {
    let raw = raw.trim_start();
    let quote = raw.as_bytes().first().copied().ok_or_else(|| error("missing quoted value"))?;
    if quote != b'\'' && quote != b'"' { return Err(error("provider metadata requires quoted string values")); }
    let mut out = String::new(); let mut escaped = false;
    for (i, ch) in raw[1..].char_indices() {
        if escaped { out.push(ch); escaped = false; continue; }
        if ch == '\\' { escaped = true; continue; }
        if ch as u32 == quote as u32 { return Ok((out, i + 2)); }
        out.push(ch);
    }
    Err(error("unterminated provider metadata string"))
}

fn balanced_table(raw: &str) -> Result<(&str, usize), ProviderError> {
    let offset = raw.len() - raw.trim_start().len();
    let raw = raw.trim_start();
    if !raw.starts_with('{') { return Err(error("provider metadata requires a table")); }
    let bytes = raw.as_bytes(); let mut depth = 0usize; let mut quote = None; let mut escaped = false; let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if escaped { escaped = false; } else if b == b'\\' { escaped = true; } else if b == q { quote = None; }
        } else if b == b'\'' || b == b'"' { quote = Some(b); }
        else if b == b'-' && bytes.get(i + 1) == Some(&b'-') { while i < bytes.len() && bytes[i] != b'\n' { i += 1; } continue; }
        else if b == b'{' { depth += 1; }
        else if b == b'}' { depth -= 1; if depth == 0 { return Ok((&raw[1..i], offset + i + 1)); } }
        i += 1;
    }
    Err(error("unterminated provider metadata table"))
}

fn string_values(raw: &str) -> Result<Vec<String>, ProviderError> {
    let mut out = Vec::new(); let bytes = raw.as_bytes(); let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let (value, used) = parse_quoted(&raw[i..])?;
            out.push(value); i += used;
        } else { i += 1; }
    }
    Ok(out)
}

fn keyed_strings(raw: &str) -> Result<BTreeMap<String, String>, ProviderError> {
    let mut out = BTreeMap::new();
    for line in raw.lines() {
        let line = line.trim().trim_end_matches(',').trim();
        if line.is_empty() || line.starts_with("--") { continue; }
        let Some((key, value)) = line.split_once('=') else { return Err(error(format!("unsupported module entry `{line}`"))) };
        let key = key.trim();
        let key = if key.starts_with('[') {
            parse_quoted(key.trim_start_matches('['))?.0
        } else { key.to_string() };
        let value = parse_quoted(value)?.0;
        if out.insert(key.clone(), value).is_some() { return Err(error(format!("duplicate module `{key}`"))); }
    }
    Ok(out)
}

fn valid_module_name(value: &str) -> bool {
    !value.is_empty() && value.split('.').all(valid_component)
}

fn manifest_versions(raw: &str, name: &str) -> Result<Vec<String>, ProviderError> {
    let repository = assignment_table(raw, "repository")?;
    let package = keyed_table(repository, name).ok_or_else(|| error(format!("LuaRocks manifest has no package `{name}`")))?;
    let mut versions = Vec::new(); let mut cursor = package;
    while let Some(open) = cursor.find("[\"").or_else(|| cursor.find("['")) {
        cursor = &cursor[open + 1..];
        let (value, used) = parse_quoted(cursor)?;
        cursor = &cursor[used..];
        if valid_component(&value) { versions.push(value); }
    }
    versions.sort_by(|a, b| compare_versions(a, b)); versions.dedup();
    if versions.is_empty() { Err(error(format!("LuaRocks manifest has no versions for `{name}`"))) } else { Ok(versions) }
}

fn require_manifest_version(raw: &str, name: &str, version: &str) -> Result<(), ProviderError> {
    if manifest_versions(raw, name)?.iter().any(|v| v == version) { Ok(()) }
    else { Err(error(format!("LuaRocks manifest has no `{name}` version `{version}`"))) }
}

fn keyed_table<'a>(raw: &'a str, wanted: &str) -> Option<&'a str> {
    let patterns = [format!("[\"{wanted}\"]"), format!("['{wanted}']"), wanted.to_string()];
    for pattern in patterns {
        let mut cursor = 0usize;
        while let Some(found) = raw[cursor..].find(&pattern) {
            let start = cursor + found + pattern.len();
            let rest = raw[start..].trim_start();
            if let Some(value) = rest.strip_prefix('=').map(str::trim_start) {
                if let Ok((table, _)) = balanced_table(value) { return Some(table); }
            }
            cursor = start;
        }
    }
    None
}

struct Scratch { path: PathBuf }
impl Scratch {
    fn new(store: &Path) -> Result<Self, ProviderError> {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let path = store.join(super::BUILD_SCRATCH_DIR).join(format!("luarocks-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|e| error(format!("could not create LuaRocks scratch: {e}")))?;
        Ok(Self { path })
    }
}
impl Drop for Scratch { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.path); } }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use std::fs;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn parses_rockspec_and_rejects_mutable_or_unsafe_refs() {
        let raw = "package = \"jetapp\"\nversion = \"2.0-1\"\nsource = { url = \"file:///tmp/jetapp.tar.gz\", sha256 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" }\ndependencies = { \"lua >= 5.4\", \"jetdep >= 1.0, < 2.0\" }\nsupported_platforms = { \"unix\", \"!windows\" }\nbuild = { type = \"builtin\", modules = { [\"jetapp\"] = \"jetapp.lua\" } }\n";
        let spec = parse_rockspec(raw).unwrap();
        assert_eq!(spec.name, "jetapp");
        assert_eq!(spec.dependencies.iter().find(|dep| dep.name == "jetdep").unwrap().requirements.len(), 2);
        assert!(parse_ref("jetapp").is_err());
        assert!(parse_ref("../../escape#version=1").is_err());
    }

    #[test]
    fn manifest_resolution_is_exact_and_semver_aware() {
        let raw = "repository = { [\"jetdep\"] = { [\"1.0-1\"] = { { arch = \"rockspec\" } }, [\"1.5-1\"] = { { arch = \"rockspec\" } } }, [\"jetapp\"] = { [\"2.0-1\"] = { { arch = \"rockspec\" } } } }";
        assert_eq!(manifest_versions(raw, "jetdep").unwrap(), vec!["1.0-1", "1.5-1"]);
        require_manifest_version(raw, "jetapp", "2.0-1").unwrap();
        assert!(require_manifest_version(raw, "jetapp", "9.0-1").is_err());
        assert!(requirements_satisfied("1.5-1", &[(">=".into(), "1.0".into()), ("<".into(), "2.0".into())]));
    }

    #[test]
    #[cfg(unix)]
    fn real_two_rock_install_executes_locks_replays_and_rejects_drift_and_tamper() {
        if which("lua").is_none() || which("curl").is_none() || which("tar").is_none() {
            eprintln!("note: skipping LuaRocks provider vertical (need lua, curl, tar)");
            return;
        }
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = std::env::temp_dir().join(format!("jet-luarocks-provider-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let repo = base.join("repo");
        let source = base.join("source");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&source).unwrap();
        let dep_archive = archive_source(&source, &repo, "jetdep", "return { value = function() return 41 end }\n");
        let app_archive = archive_source(&source, &repo, "jetapp", "local dep = require(\"jetdep\")\nreturn { value = function() return dep.value() + 1 end }\n");
        write_rockspec(&repo, "jetdep", "1.0-1", &dep_archive, "", "jetdep.lua");
        write_rockspec(&repo, "jetapp", "2.0-1", &app_archive, "\"jetdep >= 1.0, < 2.0\",", "jetapp.lua");
        fs::write(repo.join("manifest-5.4"), "repository = {\n  [\"jetdep\"] = { [\"1.0-1\"] = { { arch = \"rockspec\" } } },\n  [\"jetapp\"] = { [\"2.0-1\"] = { { arch = \"rockspec\" } } },\n}\n").unwrap();
        *TEST_REPOSITORY.write().unwrap() = Some(format!("file://{}", repo.display()));

        let project = base.join("project");
        let store = base.join("store");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&store).unwrap();
        let roots = Store::Roots { root: base.join("hangar"), dev_mode: true };
        let table = SourceTable::empty();
        let spec = crate::RefSpec::classify_in("luarocks:jetapp#version=2.0-1", &table).unwrap();
        let online = Ctx { fixtures: None, store_dir: &store, offline: false, project_dir: Some(&project) };
        let realized = Store::realize_verified(&roots, &online, Store::RealizeRequest::Package { spec: &spec, table: &table }).unwrap();
        let wrapper = Path::new(&realized.metadata().bin).join("lua-with-rocks");
        let output = Command::new(&wrapper).args(["-e", "io.write(require(\"jetapp\").value())"]).output().unwrap();
        assert!(output.status.success(), "installed Lua closure failed: {}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42");
        let output_root = realized.original_output().to_path_buf();
        let provenance = fs::read_to_string(output_root.join("luarocks.provenance")).unwrap();
        assert!(provenance.contains("package=jetdep:1.0-1:"));
        assert!(provenance.contains("package=jetapp:2.0-1:"));
        let lock = fs::read_to_string(project.join(crate::Syntax::UNIFIED_LOCK_FILE)).unwrap();
        assert!(lock.contains("luarocks = \"luarocks:jetapp#version=2.0-1\""));
        assert!(lock.contains("dependencies = [\"jetdep#version=1.0-1\"]"));

        let hidden = base.join("repo-offline");
        fs::rename(&repo, &hidden).unwrap();
        let offline = Ctx { fixtures: None, store_dir: &store, offline: true, project_dir: Some(&project) };
        let replay = Store::realize_verified(&roots, &offline, Store::RealizeRequest::Package { spec: &spec, table: &table }).unwrap();
        assert_eq!(replay.source_state(), SourceState::Cached);
        fs::rename(&hidden, &repo).unwrap();

        let changed_archive = archive_source(&source, &repo, "jetapp", "local dep = require(\"jetdep\")\nreturn { value = function() return dep.value() + 2 end }\n");
        write_rockspec(&repo, "jetapp", "2.0-1", &changed_archive, "\"jetdep >= 1.0, < 2.0\",", "jetapp.lua");
        let hostile_store = base.join("hostile-store");
        fs::create_dir_all(&hostile_store).unwrap();
        let hostile = Ctx { fixtures: None, store_dir: &hostile_store, offline: false, project_dir: Some(&project) };
        assert!(matches!(LuaRocksProvider.realize(&spec, &table, &hostile), Err(ProviderError::LuaRocks(reason)) if reason.contains("integrity changed")));

        let module = output_root.join("share/lua/5.4/jetapp.lua");
        let mut permissions = fs::metadata(&module).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o644);
        fs::set_permissions(&module, permissions).unwrap();
        fs::write(&module, "return { value = function() return 99 end }\n").unwrap();
        assert!(Store::realize_verified(&roots, &offline, Store::RealizeRequest::Package { spec: &spec, table: &table }).is_err());

        *TEST_REPOSITORY.write().unwrap() = None;
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    fn archive_source(source_root: &Path, repo: &Path, name: &str, code: &str) -> String {
        let dir = source_root.join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.lua")), code).unwrap();
        let archive = repo.join(format!("{name}.tar.gz"));
        let _ = fs::remove_file(&archive);
        let status = Command::new("tar").args(["-czf"]).arg(&archive).arg("-C").arg(source_root).arg(name).status().unwrap();
        assert!(status.success());
        SHA256::sha256_hex(&fs::read(archive).unwrap())
    }

    #[cfg(unix)]
    fn write_rockspec(repo: &Path, name: &str, version: &str, hash: &str, dependency: &str, module: &str) {
        fs::write(repo.join(format!("{name}-{version}.rockspec")), format!(
            "package = \"{name}\"\nversion = \"{version}\"\nsource = {{ url = \"file://{repo}/{name}.tar.gz\", sha256 = \"{hash}\" }}\ndependencies = {{ \"lua >= 5.4\", {dependency} }}\nsupported_platforms = {{ \"unix\", \"!windows\" }}\nbuild = {{ type = \"builtin\", modules = {{ [\"{name}\"] = \"{module}\" }} }}\n",
            repo = repo.display()
        )).unwrap();
    }
}
