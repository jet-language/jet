//! Native CRAN provider (D-FFI-R1, D-JPK-PROVIDERS2).

use super::{cache_identity, producer_record, Ctx, Provider, ProviderError, Realized, SourceState};
use crate::RefSpec::{RefSpec, SourceTable};
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) const RECIPE_ID: &str = "cran-provider-v1";
const DEFAULT_REPOSITORY: &str = "https://cloud.r-project.org";

#[derive(Debug, Clone)]
struct Record {
    name: String,
    version: String,
    dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone)]
struct Dependency {
    name: String,
    requirement: Option<(String, String)>,
}

#[derive(Debug)]
struct SourceArtifact {
    record: Record,
    path: PathBuf,
    hash: String,
}

fn dependency_objects(root: &str, artifacts: &[SourceArtifact]) -> (Vec<String>, BTreeMap<String, String>) {
    let mut references = Vec::new();
    let mut facts = BTreeMap::new();
    for artifact in artifacts
        .iter()
        .filter(|artifact| artifact.record.name != root)
    {
        let digest = format!("sha256-{}", artifact.hash);
        let relative = format!(
            "sources/{}",
            artifact.path.file_name().unwrap_or_default().to_string_lossy()
        );
        facts.insert(format!("dependency.object.{digest}"), relative);
        references.push(digest);
    }
    (references, facts)
}

pub(super) struct CranProvider;

impl Provider for CranProvider {
    fn realize(
        &self,
        spec: &RefSpec,
        _table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Realized, ProviderError> {
        if ctx.offline {
            return Err(ProviderError::Offline(format!(
                "`{}` is not in the verified hangar and --offline forbids CRAN metadata or source fetches",
                spec.raw
            )));
        }
        let repository = repository();
        let (root_name, wanted_version) = parse_ref(&spec.package)?;
        let scratch = Scratch::new(ctx.store_dir)?;
        let metadata_path = scratch.path.join("PACKAGES");
        download(
            &format!("{repository}/src/contrib/PACKAGES"),
            &metadata_path,
        )?;
        let metadata = std::fs::read_to_string(&metadata_path)
            .map_err(|e| ProviderError::Cran(format!("could not read CRAN metadata: {e}")))?;
        let records = parse_packages(&metadata)?;
        let root = records.get(root_name).ok_or_else(|| {
            ProviderError::Cran(format!("CRAN metadata has no package `{root_name}`"))
        })?;
        if let Some(wanted) = wanted_version {
            if wanted != root.version {
                return Err(ProviderError::Cran(format!(
                    "CRAN metadata resolves `{root_name}` to {}, not locked version {wanted}",
                    root.version
                )));
            }
        }
        let order = dependency_order(root_name, &records)?;
        let mut artifacts = Vec::new();
        for name in &order {
            let record = records
                .get(name)
                .ok_or_else(|| {
                    ProviderError::Cran(format!(
                        "resolved CRAN dependency `{name}` has no metadata record"
                    ))
                })?
                .clone();
            let filename = format!("{}_{}.tar.gz", record.name, record.version);
            let path = scratch.path.join(&filename);
            download(&format!("{repository}/src/contrib/{filename}"), &path)?;
            let bytes = std::fs::read(&path)
                .map_err(|e| ProviderError::Cran(format!("could not read `{filename}`: {e}")))?;
            artifacts.push(SourceArtifact {
                record,
                path,
                hash: SHA256::sha256_hex(&bytes),
            });
        }
        let source_hash = closure_hash(&artifacts);
        if let Some(project) = ctx.project_dir {
            if let Some((_output, locked_hash, locked_repo, _)) =
                crate::Lock::cran_realization(project, &spec.raw)
            {
                if locked_hash != source_hash || locked_repo != repository {
                    return Err(ProviderError::Cran(format!(
                        "locked CRAN source integrity changed for `{}` (expected {} from {}, got {} from {})",
                        spec.raw, locked_hash, locked_repo, source_hash, repository
                    )));
                }
            }
        }

        let out_dir = ctx.store_dir.join(format!(
            "{}-{}-{}",
            root.name,
            root.version,
            &source_hash[..12]
        ));
        if out_dir.exists() {
            return Err(ProviderError::Cran(format!(
                "unverified existing output {}; run `jet clean` before rebuilding",
                out_dir.display()
            )));
        }
        let library = out_dir.join("library");
        let sources = out_dir.join("sources");
        std::fs::create_dir_all(&library)
            .and_then(|_| std::fs::create_dir_all(&sources))
            .map_err(|e| ProviderError::Cran(format!("could not stage CRAN output: {e}")))?;
        for artifact in &artifacts {
            let target = sources.join(artifact.path.file_name().unwrap_or_default());
            std::fs::copy(&artifact.path, &target)
                .map_err(|e| ProviderError::Cran(format!("could not preserve CRAN source: {e}")))?;
            install(&target, &library)?;
        }
        write_runtime_wrapper(&out_dir, &library)?;
        let provenance = render_provenance(&repository, &source_hash, &artifacts);
        std::fs::write(out_dir.join("cran.provenance"), &provenance)
            .map_err(|e| ProviderError::Cran(format!("could not write CRAN provenance: {e}")))?;
        crate::Store::seal_local_output(&out_dir)
            .map_err(|e| ProviderError::Cran(format!("could not seal CRAN output: {e}")))?;
        let out = out_dir.to_string_lossy().into_owned();
        let envelope = crate::Envelope::Envelope::for_output(&out, &spec.raw, RECIPE_ID);
        let deps = root
            .dependencies
            .iter()
            .map(|dep| dep.name.clone())
            .collect();
        if let Some(project) = ctx.project_dir {
            crate::Lock::record_cran_realization(
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
        let identity = cache_identity(&source_hash, RECIPE_ID, ctx);
        let (references, mut dependency_facts) = dependency_objects(&root.name, &artifacts);
        dependency_facts.insert("repository".into(), repository.clone());
        let producer = producer_record(
            "cran",
            &format!("cas:{source_hash}"),
            &source_hash,
            BTreeMap::from([
                ("action.kind".into(), "cran-install".into()),
                ("repository".into(), repository.clone()),
                ("package.version".into(), root.version.clone()),
            ]),
            "cran-provider-v1",
            &identity,
            dependency_facts,
        )
        .map_err(|error| ProviderError::Cran(format!("invalid producer record: {error:?}")))?;
        Ok(Realized {
            name: root.name.clone(),
            version: root.version.clone(),
            reference: spec.raw.clone(),
            out: out.clone(),
            bin: out_dir.join("bin").to_string_lossy().into_owned(),
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

fn repository() -> String {
    #[cfg(test)]
    if let Some(repository) = TEST_REPOSITORY.read().unwrap().clone() {
        return repository;
    }
    DEFAULT_REPOSITORY.to_string()
}

#[cfg(test)]
static TEST_REPOSITORY: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

fn parse_ref(raw: &str) -> Result<(&str, Option<&str>), ProviderError> {
    let (name, selector) = raw.split_once('#').unwrap_or((raw, ""));
    if !valid_component(name) {
        return Err(ProviderError::Cran(format!(
            "invalid CRAN package name `{name}`"
        )));
    }
    if selector.is_empty() {
        return Err(ProviderError::Cran(format!(
            "CRAN ref `{raw}` is mutable; use `{name}#version=<exact>` so the first fetch is reviewable"
        )));
    }
    let version = Some(selector.strip_prefix("version=").unwrap_or(selector));
    Ok((name, version))
}

fn parse_packages(raw: &str) -> Result<BTreeMap<String, Record>, ProviderError> {
    let mut records = BTreeMap::new();
    for paragraph in raw.replace("\r\n", "\n").split("\n\n") {
        let mut fields: BTreeMap<String, String> = BTreeMap::new();
        let mut current = String::new();
        for line in paragraph.lines() {
            if line.starts_with(' ') || line.starts_with('\t') {
                if let Some(value) = fields.get_mut(&current) {
                    value.push(' ');
                    value.push_str(line.trim());
                }
            } else if let Some((key, value)) = line.split_once(':') {
                current = key.to_string();
                fields.insert(current.clone(), value.trim().to_string());
            }
        }
        let (Some(name), Some(version)) = (fields.get("Package"), fields.get("Version")) else {
            continue;
        };
        if !valid_component(name) || !valid_component(version) {
            return Err(ProviderError::Cran(format!(
                "CRAN metadata contains unsafe package identity `{name}` version `{version}`"
            )));
        }
        let mut dependencies = Vec::new();
        for key in ["Depends", "Imports", "LinkingTo"] {
            if let Some(value) = fields.get(key) {
                for dep in value.split(',').filter_map(parse_dependency) {
                    if !is_base_package(&dep.name)
                        && !dependencies
                            .iter()
                            .any(|item: &Dependency| item.name == dep.name)
                    {
                        dependencies.push(dep);
                    }
                }
            }
        }
        dependencies.sort_by(|a, b| a.name.cmp(&b.name));
        records.insert(
            name.clone(),
            Record {
                name: name.clone(),
                version: version.clone(),
                dependencies,
            },
        );
    }
    if records.is_empty() {
        return Err(ProviderError::Cran(
            "CRAN PACKAGES metadata contained no package records".into(),
        ));
    }
    Ok(records)
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && value != "."
        && value != ".."
}

fn parse_dependency(raw: &str) -> Option<Dependency> {
    let raw = raw.trim();
    let name = raw.split([' ', '(']).next()?.trim();
    if name.is_empty() || name == "R" {
        return None;
    }
    let requirement = raw.split_once('(').and_then(|(_, rest)| {
        let words = rest
            .trim_end_matches(')')
            .split_whitespace()
            .collect::<Vec<_>>();
        (words.len() == 2).then(|| (words[0].to_string(), words[1].to_string()))
    });
    Some(Dependency {
        name: name.to_string(),
        requirement,
    })
}

fn is_base_package(name: &str) -> bool {
    matches!(
        name,
        "base"
            | "compiler"
            | "datasets"
            | "graphics"
            | "grDevices"
            | "grid"
            | "methods"
            | "parallel"
            | "splines"
            | "stats"
            | "stats4"
            | "tcltk"
            | "tools"
            | "utils"
    )
}

fn dependency_order(
    root: &str,
    records: &BTreeMap<String, Record>,
) -> Result<Vec<String>, ProviderError> {
    fn visit(
        name: &str,
        records: &BTreeMap<String, Record>,
        active: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
        out: &mut Vec<String>,
    ) -> Result<(), ProviderError> {
        if done.contains(name) {
            return Ok(());
        }
        if !active.insert(name.to_string()) {
            return Err(ProviderError::Cran(format!(
                "CRAN dependency cycle includes `{name}`"
            )));
        }
        let record = records.get(name).ok_or_else(|| {
            ProviderError::Cran(format!(
                "CRAN dependency `{name}` is absent from repository metadata"
            ))
        })?;
        for dep in &record.dependencies {
            let selected = records.get(&dep.name).ok_or_else(|| {
                ProviderError::Cran(format!(
                    "CRAN dependency `{}` is absent from repository metadata",
                    dep.name
                ))
            })?;
            if let Some((operator, wanted)) = &dep.requirement {
                if !version_satisfies(&selected.version, operator, wanted) {
                    return Err(ProviderError::Cran(format!(
                        "CRAN dependency `{}` requires {operator} {wanted}, but metadata selects {}",
                        dep.name, selected.version
                    )));
                }
            }
            visit(&dep.name, records, active, done, out)?;
        }
        active.remove(name);
        done.insert(name.to_string());
        out.push(name.to_string());
        Ok(())
    }
    let mut out = Vec::new();
    visit(
        root,
        records,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
        &mut out,
    )?;
    Ok(out)
}

fn version_satisfies(actual: &str, operator: &str, wanted: &str) -> bool {
    use std::cmp::Ordering;
    let cmp = compare_versions(actual, wanted);
    match operator {
        ">=" => cmp != Ordering::Less,
        ">" => cmp == Ordering::Greater,
        "<=" => cmp != Ordering::Greater,
        "<" => cmp == Ordering::Less,
        "=" | "==" => cmp == Ordering::Equal,
        _ => false,
    }
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let left = left.split(['.', '-']).collect::<Vec<_>>();
    let right = right.split(['.', '-']).collect::<Vec<_>>();
    for i in 0..left.len().max(right.len()) {
        let a = left.get(i).copied().unwrap_or("0");
        let b = right.get(i).copied().unwrap_or("0");
        let cmp = match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            _ => a.cmp(b),
        };
        if cmp != Ordering::Equal {
            return cmp;
        }
    }
    Ordering::Equal
}

fn closure_hash(artifacts: &[SourceArtifact]) -> String {
    let mut identity = b"jet-cran-source-v1\0".to_vec();
    for artifact in artifacts {
        identity.extend_from_slice(artifact.record.name.as_bytes());
        identity.push(0);
        identity.extend_from_slice(artifact.record.version.as_bytes());
        identity.push(0);
        identity.extend_from_slice(artifact.hash.as_bytes());
        identity.push(0);
    }
    SHA256::sha256_hex(&identity)
}

fn download(url: &str, path: &Path) -> Result<(), ProviderError> {
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--max-time",
            "60",
            "--output",
        ])
        .arg(path)
        .arg(url)
        .status()
        .map_err(|e| ProviderError::Cran(format!("could not start provisioned curl: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(ProviderError::Cran(format!("could not fetch `{url}`")))
    }
}

fn install(source: &Path, library: &Path) -> Result<(), ProviderError> {
    let status = Command::new("R")
        .args(["CMD", "INSTALL", "--no-multiarch", "--no-test-load"])
        .arg(format!("--library={}", library.display()))
        .arg(source)
        .env("R_LIBS_USER", library)
        .status()
        .map_err(|e| {
            ProviderError::Cran(format!("could not start provisioned R installer: {e}"))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(ProviderError::Cran(format!(
            "R rejected CRAN source `{}`",
            source.display()
        )))
    }
}

fn write_runtime_wrapper(out: &Path, library: &Path) -> Result<(), ProviderError> {
    let rscript = which("Rscript")
        .ok_or_else(|| ProviderError::Cran("provisioned Rscript was not found".into()))?;
    let bin = out.join("bin");
    std::fs::create_dir_all(&bin)
        .map_err(|e| ProviderError::Cran(format!("could not create R runtime wrapper: {e}")))?;
    let wrapper = bin.join("Rscript");
    let quote = |value: &Path| value.to_string_lossy().replace('\'', "'\\''");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nR_LIBS_USER='{}' exec '{}' \"$@\"\n",
            quote(library),
            quote(&rscript)
        ),
    )
    .map_err(|e| ProviderError::Cran(format!("could not write R runtime wrapper: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).map_err(
            |e| ProviderError::Cran(format!("could not make R runtime wrapper executable: {e}")),
        )?;
    }
    Ok(())
}

fn which(tool: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?
        .as_os_str()
        .to_str()
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join(tool))
        .find(|path| path.is_file())
}

fn render_provenance(repository: &str, source_hash: &str, artifacts: &[SourceArtifact]) -> String {
    let mut out = format!(
        "schema=jet-cran-provider-v1\nrepository={repository}\nsource_hash={source_hash}\n"
    );
    for artifact in artifacts {
        out.push_str(&format!(
            "package={}:{}:{}\n",
            artifact.record.name, artifact.record.version, artifact.hash
        ));
    }
    out
}

struct Scratch {
    path: PathBuf,
}
impl Scratch {
    fn new(store: &Path) -> Result<Self, ProviderError> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = store
            .join(super::BUILD_SCRATCH_DIR)
            .join(format!("cran-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&path)
            .map_err(|e| ProviderError::Cran(format!("could not create CRAN scratch: {e}")))?;
        Ok(Self { path })
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use std::fs;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn dependency_edges_use_exact_preserved_source_object_digests() {
        let artifact = |name: &str, hash: &str| SourceArtifact {
            record: Record {
                name: name.into(),
                version: "1".into(),
                dependencies: Vec::new(),
            },
            path: PathBuf::from(format!("/scratch/{name}.tar.gz")),
            hash: hash.into(),
        };
        let artifacts = [artifact("dep", "abcd"), artifact("app", "root")];
        let (references, facts) = dependency_objects("app", &artifacts);
        assert_eq!(references, vec!["sha256-abcd"]);
        assert_eq!(
            facts.get("dependency.object.sha256-abcd").map(String::as_str),
            Some("sources/dep.tar.gz")
        );
    }

    #[test]
    fn parses_real_cran_dcf_and_orders_dependencies() {
        let records = parse_packages("Package: dep\nVersion: 1.0\nImports: methods\n\nPackage: app\nVersion: 2.0\nDepends: R (>= 4.0), dep (>= 1.0)\n").unwrap();
        assert_eq!(
            dependency_order("app", &records).unwrap(),
            vec!["dep", "app"]
        );
        assert_eq!(parse_ref("app#version=2.0").unwrap(), ("app", Some("2.0")));
    }

    #[test]
    fn rejects_missing_and_cyclic_dependencies() {
        let missing = parse_packages("Package: app\nVersion: 1\nImports: absent\n").unwrap();
        assert!(
            matches!(dependency_order("app", &missing), Err(ProviderError::Cran(reason)) if reason.contains("absent"))
        );
        let cycle = parse_packages(
            "Package: a\nVersion: 1\nImports: b\n\nPackage: b\nVersion: 1\nImports: a\n",
        )
        .unwrap();
        assert!(dependency_order("a", &cycle).is_err());
        let incompatible = parse_packages(
            "Package: dep\nVersion: 1.0\n\nPackage: app\nVersion: 1\nImports: dep (>= 9.0)\n",
        )
        .unwrap();
        assert!(
            matches!(dependency_order("app", &incompatible), Err(ProviderError::Cran(reason)) if reason.contains("requires >= 9.0"))
        );
        assert!(parse_ref("app").is_err());
        assert!(parse_packages("Package: ../../escape\nVersion: 1\n").is_err());
    }

    #[test]
    #[cfg(unix)]
    fn real_cran_pipeline_installs_locks_replays_offline_and_rejects_changed_source() {
        if which("R").is_none()
            || which("Rscript").is_none()
            || which("curl").is_none()
            || which("tar").is_none()
        {
            eprintln!("note: skipping CRAN provider vertical (need R, Rscript, curl, tar)");
            return;
        }
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = std::env::temp_dir().join(format!("jet-cran-provider-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let repo = base.join("cran");
        let contrib = repo.join("src/contrib");
        let package_src = base.join("package-src");
        fs::create_dir_all(&contrib).unwrap();
        write_package(
            &package_src,
            "jetdep",
            "1.0",
            "",
            "export(dep_value)\n",
            "dep_value <- function() 41\n",
        );
        write_package(
            &package_src,
            "jetapp",
            "2.0",
            "Imports: jetdep\n",
            "import(jetdep)\nexport(app_value)\n",
            "app_value <- function() dep_value() + 1\n",
        );
        archive_package(&package_src, &contrib, "jetdep", "1.0");
        archive_package(&package_src, &contrib, "jetapp", "2.0");
        fs::write(contrib.join("PACKAGES"), "Package: jetdep\nVersion: 1.0\n\nPackage: jetapp\nVersion: 2.0\nImports: jetdep (>= 1.0)\n").unwrap();
        let repository = format!("file://{}", repo.display());
        *TEST_REPOSITORY.write().unwrap() = Some(repository);

        let project = base.join("project");
        let store = base.join("store");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&store).unwrap();
        let roots = Store::Roots {
            root: base.join("hangar"),
            dev_mode: true,
        };
        let table = SourceTable::empty();
        let spec = crate::RefSpec::classify_in("cran:jetapp#version=2.0", &table).unwrap();
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
        let bin = Path::new(&realized.metadata().bin).join("Rscript");
        let output = Command::new(&bin)
            .args(["--vanilla", "-e", "library(jetapp); cat(app_value())"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "installed CRAN closure not visible through provider Rscript: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42");
        let provenance =
            fs::read_to_string(realized.original_output().join("cran.provenance")).unwrap();
        assert!(provenance.contains("package=jetdep:1.0:"));
        assert!(provenance.contains("package=jetapp:2.0:"));
        let lock = fs::read_to_string(project.join(crate::Syntax::UNIFIED_LOCK_FILE)).unwrap();
        assert!(lock.contains("cran = \"cran:jetapp#version=2.0\""));
        assert!(lock.contains("dependencies = [\"jetdep\"]"));

        let hidden = base.join("cran-offline");
        fs::rename(&repo, &hidden).unwrap();
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
        fs::rename(&hidden, &repo).unwrap();

        fs::write(
            package_src.join("jetapp/R/value.R"),
            "app_value <- function() dep_value() + 2\n",
        )
        .unwrap();
        archive_package(&package_src, &contrib, "jetapp", "2.0");
        let hostile_store = base.join("hostile-store");
        fs::create_dir_all(&hostile_store).unwrap();
        let hostile = Ctx {
            fixtures: None,
            store_dir: &hostile_store,
            offline: false,
            project_dir: Some(&project),
        };
        assert!(matches!(
            super::CranProvider.realize(&spec, &table, &hostile),
            Err(ProviderError::Cran(reason)) if reason.contains("integrity changed")
        ));
        *TEST_REPOSITORY.write().unwrap() = None;
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    fn write_package(
        root: &Path,
        name: &str,
        version: &str,
        deps: &str,
        namespace: &str,
        code: &str,
    ) {
        let dir = root.join(name);
        fs::create_dir_all(dir.join("R")).unwrap();
        fs::write(dir.join("DESCRIPTION"), format!("Package: {name}\nVersion: {version}\nTitle: Jet CRAN provider test\nDescription: Deterministic provider integration package.\nAuthors@R: person(\"Jet\", \"Test\", email = \"jet@example.invalid\", role = c(\"aut\", \"cre\"))\nLicense: MIT\nEncoding: UTF-8\n{deps}" )).unwrap();
        fs::write(dir.join("NAMESPACE"), namespace).unwrap();
        fs::write(dir.join("R/value.R"), code).unwrap();
    }

    #[cfg(unix)]
    fn archive_package(root: &Path, contrib: &Path, name: &str, version: &str) {
        let archive = contrib.join(format!("{name}_{version}.tar.gz"));
        let _ = fs::remove_file(&archive);
        let status = Command::new("tar")
            .args(["-czf"])
            .arg(&archive)
            .arg("-C")
            .arg(root)
            .arg(name)
            .status()
            .unwrap();
        assert!(status.success());
    }
}
