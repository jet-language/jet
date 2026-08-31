//! Foreign package export and publication.
//!
//! The .jetlib file is the authority for this surface. It carries the
//! checked export table, ABI identity, target, and native payload digest.
//! This module projects that table into host package formats. It does not
//! parse Jet source or infer types from generated code.

use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as FmtWrite;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{JetLibAccess, JetLibArtifact, JetLibExport, JetLibScalar, JetLibStamp};

const FOREIGN_SCHEMA: u32 = 1;
const MAX_ARCHIVE_ENTRY: usize = 64 * 1024 * 1024;

/// A supported foreign package registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub enum ForeignRegistry {
    PyPi,
    Npm,
    Maven,
    NuGet,
}

impl ForeignRegistry {
    /// Parse the value accepted by jet publish --to.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pypi" | "python" | "python-wheel" => Ok(Self::PyPi),
            "npm" | "node" => Ok(Self::Npm),
            "maven" | "maven-central" | "jvm" => Ok(Self::Maven),
            "nuget" | "dotnet" | ".net" => Ok(Self::NuGet),
            _ => Err(format!(
                "unsupported foreign registry '{value}'; choose pypi, npm, maven, or nuget"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PyPi => "pypi",
            Self::Npm => "npm",
            Self::Maven => "maven",
            Self::NuGet => "nuget",
        }
    }
}

/// Options for a foreign package build and upload.
#[derive(Debug, Clone, Copy)]
pub struct ForeignPublishOptions {
    pub registry: ForeignRegistry,
    pub no_sign: bool,
}

/// Evidence returned after a package is built and, when configured, uploaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignPublishReport {
    pub registry: ForeignRegistry,
    pub artifact: PathBuf,
    pub artifact_digest: String,
    pub release_identity: String,
    pub uploaded: bool,
}

struct LibraryInput {
    root: PathBuf,
    package_name: String,
    version: String,
    license: String,
    license_text: String,
    library_name: String,
    runtime_name: String,
    runtime: Vec<u8>,
    header: Vec<u8>,
    stamp: JetLibStamp,
    abi_json: String,
    abi_digest: String,
    source_identity: String,
    release_identity: String,
    common_json: String,
    provenance_json: String,
    sbom: String,
    signature_json: String,
}

struct ArchiveEntry {
    name: String,
    bytes: Vec<u8>,
}

/// Build a foreign archive in target/foreign without uploading it.
pub fn build_foreign_package(
    root: &Path,
    registry: ForeignRegistry,
    no_sign: bool,
) -> Result<ForeignPublishReport, String> {
    let input = prepare_library(root, registry, no_sign)?;
    let archive = render_archive(registry, &input)?;
    let digest = format!("sha256-{}", crate::SHA256::sha256_hex(&archive));
    let output_dir = foreign_output_dir(root)?;
    let artifact = output_dir.join(artifact_file_name(registry, &input));
    write_atomic(&artifact, &archive)?;
    Ok(ForeignPublishReport {
        registry,
        artifact,
        artifact_digest: digest,
        release_identity: input.release_identity,
        uploaded: false,
    })
}

/// Build and publish one foreign archive. A local file:// endpoint is the
/// supported offline and air-gapped registry adapter. Network registries use
/// their native publisher and receive no archive before local validation ends.
pub fn publish_foreign_package(
    root: &Path,
    options: ForeignPublishOptions,
) -> Result<ForeignPublishReport, String> {
    let report = build_foreign_package(root, options.registry, options.no_sign)?;
    let endpoint = registry_endpoint(options.registry)?;
    publish_archive(options.registry, &endpoint, &report)?;
    Ok(ForeignPublishReport {
        uploaded: true,
        ..report
    })
}

fn prepare_library(
    root: &Path,
    registry: ForeignRegistry,
    no_sign: bool,
) -> Result<LibraryInput, String> {
    let facts = crate::Package::PackageFacts::load(root)
        .ok_or_else(|| "no package.jet found in the publish root".to_string())?
        .map_err(|error| format!("package facts are invalid: {error}"))?;
    let package_name = facts.name.clone();
    let version = facts
        .version
        .clone()
        .ok_or_else(|| "package.jet has no package version".to_string())?;
    let license = facts
        .license
        .clone()
        .ok_or_else(|| "package.jet has no SPDX license".to_string())?;
    crate::Publish::validate_published_license(&package_name, &version, Some(&license))
        .map_err(|error| error.detail)?;

    let libraries = facts
        .outputs
        .iter()
        .filter(|(_, output)| output.kind == crate::Package::PackageOutputKind::Library)
        .collect::<Vec<_>>();
    let (output_key, output) = match libraries.as_slice() {
        [(name, output)] => (name.as_str(), *output),
        [] => return Err("foreign publication needs one Library output".to_string()),
        many => {
            let names = many
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "foreign publication needs one Library output; found {names}"
            ));
        }
    };
    if !output.is_native() || !output.is_loadable() {
        return Err(format!(
            "Library output '{output_key}' must set native: true and loadable: true for foreign publication"
        ));
    }

    let library_name = output.name.clone();
    let entry = output.entry.clone().ok_or_else(|| {
        format!(
            "Library output '{output_key}' has no source entry; foreign publication needs a checked entry"
        )
    })?;
    run_library_build(root, output_key, &entry)?;

    let stem = library_stem(&library_name);
    let target_dir = root.join("target");
    let runtime_name = format!("lib{stem}.{}", shared_extension());
    let runtime_path = target_dir.join(&runtime_name);
    let header_path = target_dir.join(format!("{stem}.h"));
    let jetlib_path = target_dir.join(format!("{stem}.jetlib"));
    let runtime = read_regular_file(&runtime_path, "native host runtime")?;
    if runtime.is_empty() {
        return Err(format!("native host runtime '{}' is empty", runtime_path.display()));
    }
    let header = read_regular_file(&header_path, "C ABI header")?;
    validate_header(&header, &library_name, &runtime_name)?;
    let jetlib = read_regular_file(&jetlib_path, ".jetlib ABI artifact")?;
    let artifact = JetLibArtifact::decode(&jetlib)
        .map_err(|error| format!("could not decode '{}': {error}", jetlib_path.display()))?;
    crate::JetLib::validate_load_metadata(&artifact.stamp)?;
    crate::JetLib::validate_payload_digest(&artifact.stamp, &artifact.payload)?;
    if artifact.payload != runtime {
        return Err(format!(
            "native host runtime '{runtime_name}' does not match the sealed .jetlib payload"
        ));
    }
    if artifact.stamp.library_name != library_name {
        return Err(format!(
            "ABI artifact names Library '{}', but the output is '{library_name}'",
            artifact.stamp.library_name
        ));
    }

    let source_identity = source_identity(root)?;
    let license_text = load_license_text(root, &license)?;
    let abi_json = render_abi_json(&artifact.stamp);
    let abi_digest = format!("sha256-{}", crate::SHA256::sha256_hex(abi_json.as_bytes()));
    let release_identity = release_identity(
        &package_name,
        &version,
        &library_name,
        &artifact.stamp,
        &abi_digest,
        &source_identity,
    );
    let signature_json = signature_metadata(registry, &release_identity, no_sign)?;
    let common_json = render_common_json(
        &package_name,
        &version,
        &license,
        &library_name,
        &artifact.stamp,
        &runtime_name,
        &runtime,
        &abi_digest,
        &source_identity,
        &release_identity,
        registry,
    );
    let provenance_json = render_provenance_json(
        &package_name,
        &version,
        &library_name,
        &artifact.stamp,
        &runtime_name,
        &runtime,
        &abi_digest,
        &source_identity,
        &release_identity,
        registry,
    );
    let sbom = render_sbom(
        &package_name,
        &version,
        &license,
        &runtime_name,
        &runtime,
        &artifact.stamp,
        &abi_digest,
        &source_identity,
    );

    Ok(LibraryInput {
        root: root.to_path_buf(),
        package_name,
        version,
        license,
        license_text,
        library_name,
        runtime_name,
        runtime,
        header,
        stamp: artifact.stamp,
        abi_json,
        abi_digest,
        source_identity,
        release_identity,
        common_json,
        provenance_json,
        sbom,
        signature_json,
    })
}

fn run_library_build(root: &Path, output: &str, entry: &str) -> Result<(), String> {
    let jet = env::current_exe()
        .map_err(|error| format!("could not locate the Jet compiler: {error}"))?;
    let result = Command::new(jet)
        .current_dir(root)
        .args(["build", "--lib", "--locked", "--output", output, entry])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("could not start the Jet Library build: {error}"))?;
    if result.status.success() {
        return Ok(());
    }
    Err(format!(
        "Jet Library build failed with status {}",
        result
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".to_string())
    ))
}

fn shared_extension() -> &'static str {
    if cfg!(target_os = "macos") {
        "dylib"
    } else if cfg!(target_os = "windows") {
        "dll"
    } else {
        "so"
    }
}

fn shared_platform() -> String {
    format!("{}-{}", env::consts::OS, env::consts::ARCH)
}

fn node_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "win32"
    } else {
        env::consts::OS
    }
}

fn library_stem(name: &str) -> String {
    let mut stem = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if stem.is_empty() {
        stem.push_str("library");
    }
    if stem.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        stem.insert(0, '_');
    }
    stem
}

fn read_regular_file(path: &Path, what: &str) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("missing {what} '{}': {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{what} '{}' is not a regular file", path.display()));
    }
    fs::read(path).map_err(|error| format!("could not read {what} '{}': {error}", path.display()))
}

fn validate_header(header: &[u8], library_name: &str, runtime_name: &str) -> Result<(), String> {
    let text = std::str::from_utf8(header).map_err(|_| "C ABI header is not UTF-8".to_string())?;
    if !text.contains("jet-library-set-v1") {
        return Err("C ABI header has no Jet Library set marker".to_string());
    }
    if !text.contains("jet_text_free") && text.contains("JetText") {
        return Err("C ABI header omits the JetText release function".to_string());
    }
    if !text.contains(library_name) || !text.contains(runtime_name.trim_start_matches("lib")) {
        return Err("C ABI header does not identify the selected Library output".to_string());
    }
    Ok(())
}

fn source_identity(root: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    collect_source_files(root, Path::new("."), &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut bytes = Vec::new();
    for (relative, contents) in files {
        push_len_bytes(&mut bytes, relative.as_bytes());
        push_len_bytes(&mut bytes, &contents);
    }
    Ok(format!("sha256-{}", crate::SHA256::sha256_hex(&bytes)))
}

fn collect_source_files(
    root: &Path,
    relative: &Path,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), String> {
    let directory = root.join(relative);
    let mut entries = fs::read_dir(&directory)
        .map_err(|error| format!("could not read source tree '{}': {error}", directory.display()))?
        .collect::<Result<Vec<_>, io::Error>>()
        .map_err(|error| format!("could not enumerate source tree '{}': {error}", directory.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let child_relative = if relative == Path::new(".") {
            PathBuf::from(name.as_ref())
        } else {
            relative.join(name.as_ref())
        };
        if child_relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::Normal(value)
                    if value == "target" || value == "vendor" || value == "build"
            )
        }) {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            format!("could not inspect source path '{}': {error}", entry.path().display())
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "source identity refuses symlink '{}'",
                child_relative.display()
            ));
        }
        if metadata.is_dir() {
            collect_source_files(root, &child_relative, files)?;
        } else if metadata.is_file()
            && (child_relative == Path::new("package.jet")
                || child_relative == Path::new(".jet/lock")
                || child_relative
                    .extension()
                    .is_some_and(|extension| extension == "jet"))
        {
            let slash_path = child_relative.to_string_lossy().replace('\\', "/");
            files.push((
                slash_path,
                fs::read(entry.path()).map_err(|error| {
                    format!("could not read source path '{}': {error}", entry.path().display())
                })?,
            ));
        }
    }
    Ok(())
}

fn release_identity(
    package_name: &str,
    version: &str,
    library_name: &str,
    stamp: &JetLibStamp,
    abi_digest: &str,
    source_identity: &str,
) -> String {
    let mut bytes = Vec::new();
    let abi_version = stamp.abi_version.to_string();
    for value in [
        package_name,
        version,
        library_name,
        &stamp.compiler_version,
        &stamp.compiler_build,
        &stamp.target,
        &stamp.target_triple,
        &stamp.linker_identity,
        &stamp.abi_identity,
        &abi_version,
        abi_digest,
        source_identity,
        &stamp.payload_digest,
    ] {
        push_len_bytes(&mut bytes, value.as_bytes());
    }
    format!("jet-release-v1:sha256-{}", crate::SHA256::sha256_hex(&bytes))
}

fn render_abi_json(stamp: &JetLibStamp) -> String {
    let exports = stamp
        .exports
        .iter()
        .map(render_export_json)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":{FOREIGN_SCHEMA},\"abiVersion\":{},\"abiIdentity\":{},\"library\":{},\"exports\":[{}]}}",
        stamp.abi_version,
        json_string(&stamp.abi_identity),
        json_string(&stamp.library_name),
        exports
    )
}

fn render_export_json(export: &JetLibExport) -> String {
    let access = export
        .conventions
        .iter()
        .map(|value| json_string(access_name(*value)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"name\":{},\"symbol\":{},\"scalar\":{},\"params\":{},\"access\":[{}]}}",
        json_string(&export.name),
        json_string(&export.symbol),
        json_string(scalar_name(export.scalar)),
        export.params,
        access
    )
}

fn render_common_json(
    package_name: &str,
    version: &str,
    license: &str,
    library_name: &str,
    stamp: &JetLibStamp,
    runtime_name: &str,
    runtime: &[u8],
    abi_digest: &str,
    source_identity: &str,
    release_identity: &str,
    registry: ForeignRegistry,
) -> String {
    format!(
        "{{\"schema\":{FOREIGN_SCHEMA},\"package\":{},\"version\":{},\"license\":{},\"library\":{},\"registry\":{},\"target\":{},\"targetTriple\":{},\"runtime\":{{\"name\":{},\"digest\":{}}},\"abi\":{{\"version\":{},\"identity\":{},\"digest\":{}}},\"sourceIdentity\":{},\"compiler\":{{\"version\":{},\"build\":{},\"linker\":{}}},\"dependencies\":[],\"releaseIdentity\":{}}}",
        json_string(package_name),
        json_string(version),
        json_string(license),
        json_string(library_name),
        json_string(registry.as_str()),
        json_string(&stamp.target),
        json_string(&stamp.target_triple),
        json_string(runtime_name),
        json_string(&format!("sha256-{}", crate::SHA256::sha256_hex(runtime))),
        stamp.abi_version,
        json_string(&stamp.abi_identity),
        json_string(abi_digest),
        json_string(source_identity),
        json_string(&stamp.compiler_version),
        json_string(&stamp.compiler_build),
        json_string(&stamp.linker_identity),
        json_string(release_identity),
    )
}

fn render_provenance_json(
    package_name: &str,
    version: &str,
    library_name: &str,
    stamp: &JetLibStamp,
    runtime_name: &str,
    runtime: &[u8],
    abi_digest: &str,
    source_identity: &str,
    release_identity: &str,
    registry: ForeignRegistry,
) -> String {
    format!(
        "{{\"schema\":\"jet.provenance/v1\",\"subject\":{{\"package\":{},\"version\":{},\"library\":{},\"releaseIdentity\":{}}},\"source\":{{\"identity\":{},\"packageFile\":\"package.jet\"}},\"build\":{{\"compilerVersion\":{},\"compilerBuild\":{},\"target\":{},\"targetTriple\":{},\"linker\":{}}},\"abi\":{{\"identity\":{},\"digest\":{}}},\"runtime\":{{\"name\":{},\"digest\":{}}},\"registry\":{},\"parameters\":{{\"dependencies\":[],\"deterministic\":true}}}}",
        json_string(package_name),
        json_string(version),
        json_string(library_name),
        json_string(release_identity),
        json_string(source_identity),
        json_string(&stamp.compiler_version),
        json_string(&stamp.compiler_build),
        json_string(&stamp.target),
        json_string(&stamp.target_triple),
        json_string(&stamp.linker_identity),
        json_string(&stamp.abi_identity),
        json_string(abi_digest),
        json_string(runtime_name),
        json_string(&format!("sha256-{}", crate::SHA256::sha256_hex(runtime))),
        json_string(registry.as_str()),
    )
}

fn render_sbom(
    package_name: &str,
    version: &str,
    license: &str,
    runtime_name: &str,
    runtime: &[u8],
    stamp: &JetLibStamp,
    abi_digest: &str,
    source_identity: &str,
) -> String {
    let runtime_digest = crate::SHA256::sha256_hex(runtime);
    format!(
        "SPDXVersion: SPDX-2.3\nDataLicense: CC0-1.0\nSPDXID: SPDXRef-DOCUMENT\nDocumentName: jet-foreign-{package_name}-{version}\nDocumentNamespace: https://jet-lang.dev/sbom/{source_identity}\nCreator: Tool: jet\nPackageName: {package_name}\nSPDXID: SPDXRef-Package\nPackageVersion: {version}\nPackageLicenseDeclared: {license}\nPackageSupplier: Organization: Jet package publisher\nPackageDownloadLocation: NOASSERTION\nExternalRef: PACKAGE-MANAGER jet-release {source_identity}\nFileName: {runtime_name}\nFileChecksum: SHA256: {runtime_digest}\nPackageComment: ABI {abi_digest}\nCompilerTarget: {}\n",
        stamp.target
    )
}

fn signature_metadata(
    _registry: ForeignRegistry,
    release_identity: &str,
    no_sign: bool,
) -> Result<String, String> {
    if no_sign {
        return Ok(format!(
            "{{\"schema\":\"jet.signature/v1\",\"identity\":{},\"algorithm\":\"none\",\"signature\":null,\"publicKey\":null}}",
            json_string(release_identity)
        ));
    }
    // One release identity has one signing identity. The registry name is
    // transport metadata, not a second trust domain.
    let key_name = "foreign-release";
    let (seed, public_key) = if crate::Publish::Sign::key_exists(&key_name) {
        let (seed, _) = crate::Publish::Sign::key_paths(&key_name);
        let public_key = crate::Publish::Sign::read_public_key(&key_name)
            .ok_or_else(|| "foreign registry signing key has no public key".to_string())?;
        (seed, public_key)
    } else {
        let (seed, _, public_key) = crate::Publish::Sign::keygen(&key_name, false)
            .map_err(|error| format!("could not create the foreign registry signing key: {error:?}"))?;
        (seed, public_key)
    };
    let signature = crate::Publish::Sign::sign(&seed, release_identity)
        .map_err(|error| format!("could not sign the foreign release: {error:?}"))?;
    Ok(format!(
        "{{\"schema\":\"jet.signature/v1\",\"identity\":{},\"algorithm\":\"ed25519\",\"signature\":{},\"publicKey\":{}}}",
        json_string(release_identity),
        json_string(&signature),
        json_string(&public_key)
    ))
}

fn render_archive(registry: ForeignRegistry, input: &LibraryInput) -> Result<Vec<u8>, String> {
    let entries = match registry {
        ForeignRegistry::PyPi => python_entries(input)?,
        ForeignRegistry::Npm => npm_entries(input)?,
        ForeignRegistry::Maven => maven_entries(input)?,
        ForeignRegistry::NuGet => nuget_entries(input)?,
    };
    match registry {
        ForeignRegistry::Npm => tar_gz(&entries),
        ForeignRegistry::PyPi | ForeignRegistry::Maven | ForeignRegistry::NuGet => zip(&entries),
    }
}

fn python_entries(input: &LibraryInput) -> Result<Vec<ArchiveEntry>, String> {
    let distribution = python_distribution(&input.package_name);
    let module = python_module(&input.package_name);
    let dist_info = format!("{distribution}-{}.dist-info", input.version.replace('-', "_"));
    let mut entries = vec![
        ArchiveEntry {
            name: format!("{module}/__init__.py"),
            bytes: render_python(input).into_bytes(),
        },
        ArchiveEntry {
            name: format!("{module}/py.typed"),
            bytes: Vec::new(),
        },
        ArchiveEntry {
            name: format!("{module}/lib/{}", input.runtime_name),
            bytes: input.runtime.clone(),
        },
        ArchiveEntry {
            name: format!("{module}/jet-abi.json"),
            bytes: input.abi_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: format!("{module}/jet-release.json"),
            bytes: input.common_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: format!("{module}/jet-provenance.json"),
            bytes: input.provenance_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: format!("{module}/jet-sbom.spdx"),
            bytes: input.sbom.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: format!("{module}/jet-signature.json"),
            bytes: input.signature_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: format!("{module}/{}", license_file_name(&input.license)),
            bytes: input.license_text.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: format!("{dist_info}/METADATA"),
            bytes: format!(
                "Metadata-Version: 2.1\nName: {distribution}\nVersion: {}\nSummary: Jet foreign library\nLicense: {}\nRequires-Python: >=3.9\n\n{}",
                input.version,
                input.license,
                first_minute_python(&module)
            )
            .into_bytes(),
        },
        ArchiveEntry {
            name: format!("{dist_info}/WHEEL"),
            bytes: format!(
                "Wheel-Version: 1.0\nGenerator: jet\nRoot-Is-Purelib: false\nTag: py3-none-{}\n",
                python_platform()
            )
            .into_bytes(),
        },
    ];
    let record = render_python_record(&entries, &dist_info);
    entries.push(ArchiveEntry {
        name: format!("{dist_info}/RECORD"),
        bytes: record.into_bytes(),
    });
    Ok(entries)
}

fn npm_entries(input: &LibraryInput) -> Result<Vec<ArchiveEntry>, String> {
    let package_name = npm_package_name(&input.package_name);
    let addon = format!("{}.node", library_stem(&input.library_name));
    let platform = format!("{}-{}", node_platform(), env::consts::ARCH);
    let addon_bytes = build_node_addon(input, &addon)?;
    let package_json = format!(
        "{{\n  \"name\": {},\n  \"version\": {},\n  \"description\": \"Jet native library\",\n  \"main\": \"index.js\",\n  \"license\": {},\n  \"os\": [{}],\n  \"cpu\": [{}],\n  \"engines\": {{\"node\": \">=18\"}},\n  \"jet\": {{\"releaseIdentity\": {}, \"abiDigest\": {}, \"sourceIdentity\": {}}}\n}}\n",
        json_string(&package_name),
        json_string(&input.version),
        json_string(&input.license),
        json_string(node_platform()),
        json_string(env::consts::ARCH),
        json_string(&input.release_identity),
        json_string(&input.abi_digest),
        json_string(&input.source_identity),
    );
    Ok(vec![
        ArchiveEntry {
            name: "package/package.json".to_string(),
            bytes: package_json.into_bytes(),
        },
        ArchiveEntry {
            name: "package/index.js".to_string(),
            bytes: render_node_index(&addon).into_bytes(),
        },
        ArchiveEntry {
            name: format!("package/native/{platform}/{addon}"),
            bytes: addon_bytes,
        },
        ArchiveEntry {
            name: format!("package/native/{platform}/{}", input.runtime_name),
            bytes: input.runtime.clone(),
        },
        ArchiveEntry {
            name: "package/jet-abi.json".to_string(),
            bytes: input.abi_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "package/jet-release.json".to_string(),
            bytes: input.common_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "package/jet-provenance.json".to_string(),
            bytes: input.provenance_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "package/jet-sbom.spdx".to_string(),
            bytes: input.sbom.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "package/jet-signature.json".to_string(),
            bytes: input.signature_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: format!("package/{}", license_file_name(&input.license)),
            bytes: input.license_text.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "package/README.md".to_string(),
            bytes: first_minute_node(&package_name).into_bytes(),
        },
    ])
}

fn maven_entries(input: &LibraryInput) -> Result<Vec<ArchiveEntry>, String> {
    let package = java_package(&input.package_name);
    let class_name = java_class_name(&input.library_name);
    let class_dir = format!("{}/{}", package.replace('.', "/"), class_name);
    let jni_name = format!(
        "libjet_{}_jni.{}",
        library_stem(&input.library_name),
        shared_extension()
    );
    let class_bytes = build_java_classes(input, &package, &class_name, &jni_name)?;
    let jni_bytes = build_jni_bridge(input, &package, &class_name, &jni_name)?;
    let artifact = format!("{}-{}", java_artifact(&input.package_name), input.version);
    let pom = render_pom(input, &artifact);
    Ok(vec![
        ArchiveEntry {
            name: "META-INF/MANIFEST.MF".to_string(),
            bytes: b"Manifest-Version: 1.0\nCreated-By: jet\n\n".to_vec(),
        },
        ArchiveEntry {
            name: format!("{class_dir}.class"),
            bytes: class_bytes,
        },
        ArchiveEntry {
            name: format!("META-INF/native/{}/{}", shared_platform(), jni_name),
            bytes: jni_bytes,
        },
        ArchiveEntry {
            name: format!("META-INF/native/{}/{}", shared_platform(), input.runtime_name),
            bytes: input.runtime.clone(),
        },
        ArchiveEntry {
            name: format!("META-INF/maven/dev.jet/{artifact}/pom.xml"),
            bytes: pom.clone().into_bytes(),
        },
        ArchiveEntry {
            name: format!("META-INF/maven/dev.jet/{artifact}/pom.properties"),
            bytes: format!(
                "groupId=dev.jet\nartifactId={artifact}\nversion={}\n",
                input.version
            )
            .into_bytes(),
        },
        ArchiveEntry {
            name: "META-INF/jet-abi.json".to_string(),
            bytes: input.abi_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "META-INF/jet-release.json".to_string(),
            bytes: input.common_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "META-INF/jet-provenance.json".to_string(),
            bytes: input.provenance_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "META-INF/jet-sbom.spdx".to_string(),
            bytes: input.sbom.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "META-INF/jet-signature.json".to_string(),
            bytes: input.signature_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: format!("META-INF/{}", license_file_name(&input.license)),
            bytes: input.license_text.as_bytes().to_vec(),
        },
    ])
}

fn nuget_entries(input: &LibraryInput) -> Result<Vec<ArchiveEntry>, String> {
    let package = nuget_package_name(&input.package_name);
    let assembly = format!("{package}.dll");
    let assembly_bytes = build_dotnet_assembly(input, &package)?;
    let nuspec = render_nuspec(input, &package);
    Ok(vec![
        ArchiveEntry {
            name: format!("{package}.nuspec"),
            bytes: nuspec.into_bytes(),
        },
        ArchiveEntry {
            name: format!("lib/net8.0/{assembly}"),
            bytes: assembly_bytes,
        },
        ArchiveEntry {
            name: format!("runtimes/{}/native/{}", nuget_rid(), input.runtime_name),
            bytes: input.runtime.clone(),
        },
        ArchiveEntry {
            name: "jet-abi.json".to_string(),
            bytes: input.abi_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "jet-release.json".to_string(),
            bytes: input.common_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "jet-provenance.json".to_string(),
            bytes: input.provenance_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "jet-sbom.spdx".to_string(),
            bytes: input.sbom.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "jet-signature.json".to_string(),
            bytes: input.signature_json.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: license_file_name(&input.license).to_string(),
            bytes: input.license_text.as_bytes().to_vec(),
        },
        ArchiveEntry {
            name: "README.md".to_string(),
            bytes: first_minute_dotnet(&package).into_bytes(),
        },
    ])
}

fn render_python(input: &LibraryInput) -> String {
    let mut out = String::from(
        "\"\"\"Generated by Jet from the sealed .jetlib export table.\"\"\"\n\
         import ctypes\n\
         import pathlib\n\
         import sys\n\
         \n\
         class JetText(ctypes.Structure):\n\
             _fields_ = [(\"ptr\", ctypes.POINTER(ctypes.c_uint8)), (\"len\", ctypes.c_size_t)]\n\
         \n\
         class JetCallError(RuntimeError):\n\
             pass\n\
         \n\
         def _load():\n\
             try:\n\
                 library = ctypes.CDLL(str(pathlib.Path(__file__).with_name(\"lib\").joinpath(\"RUNTIME\")))\n\
             except OSError as error:\n\
                 raise JetCallError(\"Jet host runtime could not be loaded\") from error\n\
             if hasattr(library, \"jet_text_free\"):\n\
                 library.jet_text_free.argtypes = [JetText]\n\
                 library.jet_text_free.restype = None\n\
             return library\n\
         \n\
         _LIBRARY = _load()\n\
         \n\
         def _text_result(value):\n\
             if value.len == 0:\n\
                 _LIBRARY.jet_text_free(value)\n\
                 return \"\"\n\
             address = ctypes.cast(value.ptr, ctypes.c_void_p).value\n\
             if address is None or value.len > sys.maxsize or value.len > sys.maxsize - address:\n\
                 raise JetCallError(\"invalid JetText pointer-length pair\")\n\
             try:\n\
                 return ctypes.string_at(value.ptr, value.len).decode(\"utf-8\")\n\
             except UnicodeDecodeError as error:\n\
                 raise JetCallError(\"JetText contains invalid UTF-8\") from error\n\
             finally:\n\
                 _LIBRARY.jet_text_free(value)\n\
         \n"
            .replace("RUNTIME", &input.runtime_name),
    );
    for export in &input.stamp.exports {
        let ty = python_type(export.scalar);
        let args = (0..export.params)
            .map(|index| format!("p{index}"))
            .collect::<Vec<_>>();
        let signature = args
            .iter()
            .map(|arg| format!("{arg}: {ty}"))
            .collect::<Vec<_>>()
            .join(", ");
        let ctypes_args = (0..export.params)
            .map(|_| python_ctypes_type(export.scalar))
            .collect::<Vec<_>>()
            .join(", ");
        let call = if export.scalar == JetLibScalar::Text {
            format!(
                "    encoded = [value.encode(\"utf-8\") for value in [{args}]]\n    buffers = [ctypes.create_string_buffer(value) for value in encoded]\n    values = [JetText(ctypes.cast(value, ctypes.POINTER(ctypes.c_uint8)), len(raw)) for value, raw in zip(buffers, encoded)]\n    result = _LIBRARY.{symbol}(*values)",
                args = args.join(", "),
                symbol = export.symbol
            )
        } else {
            format!(
                "    result = _LIBRARY.{}({})",
                export.symbol,
                args.join(", ")
            )
        };
        let result = if export.scalar == JetLibScalar::Text {
            "    return _text_result(result)"
        } else {
            "    return result"
        };
        out.push_str(&format!(
            "def {name}({signature}) -> {ty}:\n    _LIBRARY.{symbol}.argtypes = [{ctypes_args}]\n    _LIBRARY.{symbol}.restype = {ctype}\n{call}\n{result}\n\n",
            name = export.name,
            signature = signature,
            ty = ty,
            symbol = export.symbol,
            ctypes_args = ctypes_args,
            ctype = python_ctypes_type(export.scalar),
            call = call,
            result = result,
        ));
    }
    out
}

fn render_node_index(addon: &str) -> String {
    format!(
        "const path = require('node:path');\nconst platform = process.platform + '-' + process.arch;\nmodule.exports = require(path.join(__dirname, 'native', platform, '{}'));\n",
        addon
    )
}

fn access_name(value: JetLibAccess) -> &'static str {
    match value {
        JetLibAccess::Read => "read",
        JetLibAccess::Write => "write",
        JetLibAccess::Move => "move",
    }
}

fn scalar_name(value: JetLibScalar) -> &'static str {
    match value {
        JetLibScalar::Int => "int",
        JetLibScalar::Float => "float",
        JetLibScalar::Bool => "bool",
        JetLibScalar::Text => "text",
    }
}

fn python_type(value: JetLibScalar) -> &'static str {
    match value {
        JetLibScalar::Int => "int",
        JetLibScalar::Float => "float",
        JetLibScalar::Bool => "bool",
        JetLibScalar::Text => "str",
    }
}

fn python_ctypes_type(value: JetLibScalar) -> &'static str {
    match value {
        JetLibScalar::Int => "ctypes.c_int64",
        JetLibScalar::Float => "ctypes.c_double",
        JetLibScalar::Bool => "ctypes.c_bool",
        JetLibScalar::Text => "JetText",
    }
}

fn normalized_component(value: &str, lower: bool) -> String {
    let mut result = String::new();
    for ch in value.chars() {
        let ch = if lower {
            ch.to_ascii_lowercase()
        } else {
            ch
        };
        if ch.is_ascii_alphanumeric() || ch == '_' {
            result.push(ch);
        } else {
            result.push('_');
        }
    }
    if result.is_empty() {
        result.push_str("library");
    }
    if result.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        result.insert(0, '_');
    }
    result
}

fn python_distribution(value: &str) -> String {
    normalized_component(value, false)
}

fn python_module(value: &str) -> String {
    normalized_component(value, false)
}

fn python_platform() -> String {
    if cfg!(target_os = "macos") {
        format!("macosx_{}_{}", env::consts::ARCH, env::consts::ARCH)
    } else if cfg!(target_os = "windows") {
        if env::consts::ARCH == "x86_64" {
            "win_amd64".to_string()
        } else {
            format!("win_{}", env::consts::ARCH)
        }
    } else {
        format!("linux_{}", env::consts::ARCH)
    }
}

fn npm_package_name(value: &str) -> String {
    format!("@jet/{}", normalized_component(value, true).replace('_', "-"))
}

fn java_package(value: &str) -> String {
    format!("dev.jet.{}", normalized_component(value, true))
}

fn java_class_name(value: &str) -> String {
    let component = normalized_component(value, false);
    let mut result = String::new();
    let mut upper = true;
    for ch in component.chars() {
        if upper {
            result.push(ch.to_ascii_uppercase());
            upper = false;
        } else {
            result.push(ch);
        }
        if ch == '_' {
            upper = true;
        }
    }
    if result == "Library" {
        result.push_str("Api");
    }
    result
}

fn java_artifact(value: &str) -> String {
    format!("jet-{}", normalized_component(value, true).replace('_', "-"))
}

fn nuget_package_name(value: &str) -> String {
    format!("Jet.{}", normalized_component(value, false))
}

fn nuget_rid() -> String {
    let os = if cfg!(target_os = "macos") {
        "osx"
    } else if cfg!(target_os = "windows") {
        "win"
    } else {
        "linux"
    };
    let arch = match env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        other => other,
    };
    format!("{os}-{arch}")
}

fn artifact_file_name(registry: ForeignRegistry, input: &LibraryInput) -> String {
    match registry {
        ForeignRegistry::PyPi => format!(
            "{}-{}-py3-none-{}.whl",
            python_distribution(&input.package_name),
            input.version,
            python_platform()
        ),
        ForeignRegistry::Npm => format!(
            "{}-{}.tgz",
            npm_package_name(&input.package_name)
                .trim_start_matches('@')
                .replace('/', "-"),
            input.version
        ),
        ForeignRegistry::Maven => format!(
            "{}-{}.jar",
            java_artifact(&input.package_name),
            input.version
        ),
        ForeignRegistry::NuGet => format!(
            "{}.{}.nupkg",
            nuget_package_name(&input.package_name),
            input.version
        ),
    }
}

fn license_file_name(_license: &str) -> &'static str {
    "LICENSE"
}

fn load_license_text(root: &Path, license: &str) -> Result<String, String> {
    for name in ["LICENSE", "LICENSE.txt", "COPYING", "COPYING.txt"] {
        let path = root.join(name);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "license file '{}' must not be a symlink",
                    path.display()
                ));
            }
            if metadata.is_file() {
                let bytes = read_regular_file(&path, "license file")?;
                return String::from_utf8(bytes)
                    .map_err(|_| format!("license file '{}' is not UTF-8", path.display()));
            }
        }
    }
    Ok(format!(
        "SPDX-License-Identifier: {license}\n\nThis package carries the source package's declared SPDX license.\n"
    ))
}

fn first_minute_python(module: &str) -> String {
    format!(
        "Install and call this package without Jet:\n\n  python -m pip install {module}\n  python -c \"from {module} import *; print(next(iter(locals())))\"\n"
    )
}

fn first_minute_node(package: &str) -> String {
    format!(
        "# Jet native package\n\nInstall with npm install {package}. Then:\n\nconst jet = require('{package}');\nconsole.log(Object.keys(jet));\n\nThe package contains its native runtime; Jet is not required.\n"
    )
}

fn first_minute_dotnet(package: &str) -> String {
    format!(
        "# Jet native package\n\nRun: dotnet add package {package}\n\nUse the typed {package}.Library methods from C#. The package contains its native runtime; Jet is not required.\n"
    )
}

fn render_python_record(entries: &[ArchiveEntry], dist_info: &str) -> String {
    let mut out = String::new();
    for entry in entries {
        let digest = crate::SHA256::sha256(&entry.bytes);
        let encoded = base64url(&digest);
        let _ = writeln!(
            out,
            "{},sha256={},{}",
            entry.name,
            encoded,
            entry.bytes.len()
        );
    }
    let _ = writeln!(out, "{dist_info}/RECORD,,");
    out
}

fn base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (a << 16) | (b << 8) | c;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        }
    }
    out
}

fn push_len_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(
        &u64::try_from(bytes.len())
            .expect("foreign identity field cannot exceed u64")
            .to_be_bytes(),
    );
    out.extend_from_slice(bytes);
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn xml_string(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("artifact '{}' has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create artifact directory: {error}"))?;
    let temp = parent.join(format!(
        ".{}.partial-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| format!("could not stage artifact '{}': {error}", temp.display()))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("could not finish artifact '{}': {error}", temp.display()))?;
        fs::rename(&temp, path)
            .map_err(|error| format!("could not install artifact '{}': {error}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn foreign_output_dir(root: &Path) -> Result<PathBuf, String> {
    let target = root.join("target");
    if let Ok(metadata) = fs::symlink_metadata(&target) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "foreign artifact directory '{}' is not a real target directory",
                target.display()
            ));
        }
    }
    let directory = target.join("foreign");
    if let Ok(metadata) = fs::symlink_metadata(&directory) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "foreign artifact directory '{}' is not a real directory",
                directory.display()
            ));
        }
    } else {
        fs::create_dir_all(&directory)
            .map_err(|error| format!("could not create '{}': {error}", directory.display()))?;
    }
    Ok(directory)
}

fn zip(entries: &[ArchiveEntry]) -> Result<Vec<u8>, String> {
    let mut files = BTreeMap::new();
    for entry in entries {
        validate_archive_entry(&entry.name, entry.bytes.len())?;
        if files.insert(entry.name.clone(), entry.bytes.clone()).is_some() {
            return Err(format!("foreign archive repeats entry '{}'", entry.name));
        }
    }
    let mut output = Vec::new();
    let mut central = Vec::new();
    let mut entry_count = 0usize;
    for (name, bytes) in files {
        let name_bytes = name.as_bytes();
        let name_len = u16::try_from(name_bytes.len())
            .map_err(|_| format!("foreign archive entry name '{name}' is too long"))?;
        let size = u32::try_from(bytes.len())
            .map_err(|_| format!("foreign archive entry '{name}' is too large for ZIP"))?;
        let offset = u32::try_from(output.len())
            .map_err(|_| "foreign ZIP is too large".to_string())?;
        let crc = crc32(&bytes);
        output.extend_from_slice(&0x04034b50u32.to_le_bytes());
        output.extend_from_slice(&20u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&crc.to_le_bytes());
        output.extend_from_slice(&size.to_le_bytes());
        output.extend_from_slice(&size.to_le_bytes());
        output.extend_from_slice(&name_len.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(name_bytes);
        output.extend_from_slice(&bytes);

        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
        entry_count += 1;
    }
    let central_offset =
        u32::try_from(output.len()).map_err(|_| "foreign ZIP is too large".to_string())?;
    let central_size =
        u32::try_from(central.len()).map_err(|_| "foreign ZIP is too large".to_string())?;
    output.extend_from_slice(&central);
    let entry_count = u16::try_from(entry_count)
        .map_err(|_| "foreign ZIP has too many entries".to_string())?;
    output.extend_from_slice(&0x06054b50u32.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&entry_count.to_le_bytes());
    output.extend_from_slice(&entry_count.to_le_bytes());
    output.extend_from_slice(&central_size.to_le_bytes());
    output.extend_from_slice(&central_offset.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    Ok(output)
}

fn validate_archive_entry(name: &str, size: usize) -> Result<(), String> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("foreign archive has unsafe entry name '{name}'"));
    }
    if name.as_bytes().len() > u16::MAX as usize {
        return Err(format!("foreign archive entry name '{name}' is too long"));
    }
    if size > MAX_ARCHIVE_ENTRY {
        return Err(format!(
            "foreign archive entry '{name}' exceeds the {} MiB limit",
            MAX_ARCHIVE_ENTRY / (1024 * 1024)
        ));
    }
    Ok(())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn tar_gz(entries: &[ArchiveEntry]) -> Result<Vec<u8>, String> {
    let mut files = BTreeMap::new();
    for entry in entries {
        validate_archive_entry(&entry.name, entry.bytes.len())?;
        if files.insert(entry.name.clone(), entry.bytes.clone()).is_some() {
            return Err(format!("foreign archive repeats entry '{}'", entry.name));
        }
    }
    let mut tar = Vec::new();
    for (name, bytes) in files {
        if name.as_bytes().len() > 100 {
            return Err(format!("NPM archive entry '{name}' is too long"));
        }
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        write_octal(&mut header[100..108], if is_native_name(&name) { 0o755 } else { 0o644 });
        write_octal(&mut header[108..116], 0);
        write_octal(&mut header[116..124], 0);
        write_octal(&mut header[124..136], bytes.len() as u64);
        write_octal(&mut header[136..148], 0);
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let checksum = header.iter().map(|byte| *byte as u32).sum::<u32>();
        let checksum_text = format!("{checksum:06o}");
        header[148..154].copy_from_slice(checksum_text.as_bytes());
        header[154] = 0;
        header[155] = b' ';
        tar.extend_from_slice(&header);
        tar.extend_from_slice(&bytes);
        let padding = (512 - (bytes.len() % 512)) % 512;
        tar.resize(tar.len() + padding, 0);
    }
    tar.resize(tar.len() + 1024, 0);
    gzip_store(&tar)
}

fn is_native_name(name: &str) -> bool {
    name.ends_with(".so")
        || name.ends_with(".dylib")
        || name.ends_with(".dll")
        || name.ends_with(".node")
}

fn write_octal(field: &mut [u8], value: u64) {
    let width = field.len();
    let text = format!("{value:0width$o}", width = width.saturating_sub(1));
    let start = width.saturating_sub(1).saturating_sub(text.len());
    field.fill(0);
    field[start..start + text.len()].copy_from_slice(text.as_bytes());
}

fn gzip_store(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3];
    if bytes.is_empty() {
        output.extend_from_slice(&[1, 0, 0, 255, 255]);
    } else {
        let chunks = bytes.chunks(65_535).collect::<Vec<_>>();
        for (index, chunk) in chunks.iter().enumerate() {
            let final_block = index + 1 == chunks.len();
            output.push(if final_block { 1 } else { 0 });
            let length = u16::try_from(chunk.len())
                .map_err(|_| "internal gzip block is too large".to_string())?;
            output.extend_from_slice(&length.to_le_bytes());
            output.extend_from_slice(&(!length).to_le_bytes());
            output.extend_from_slice(chunk);
        }
    }
    output.extend_from_slice(&crc32(bytes).to_le_bytes());
    output.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    Ok(output)
}

fn run_host_tool(command: &mut Command, tool: &str) -> Result<(), String> {
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let status = command.status().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!("required host tool '{tool}' was not found")
        } else {
            format!("could not start host tool '{tool}': {error}")
        }
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "host tool '{tool}' failed with status {}",
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string())
        ))
    }
}

fn find_tool_path(tool: &str) -> Result<PathBuf, String> {
    let path = env::var_os("PATH").ok_or_else(|| {
        format!("required host tool '{tool}' was not found because PATH is empty")
    })?;
    for directory in env::split_paths(&path) {
        let candidate = directory.join(tool);
        if candidate.is_file() {
            return fs::canonicalize(&candidate)
                .map_err(|error| format!("could not resolve host tool '{tool}': {error}"));
        }
    }
    Err(format!("required host tool '{tool}' was not found"))
}

fn java_home() -> Result<PathBuf, String> {
    if let Some(value) = env::var_os("JAVA_HOME").filter(|value| !value.is_empty()) {
        let home = PathBuf::from(value);
        if home.join("include").join("jni.h").is_file() {
            return Ok(home);
        }
        return Err(format!(
            "JAVA_HOME '{}' has no include/jni.h",
            home.display()
        ));
    }
    let javac = find_tool_path("javac")?;
    let home = javac
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "could not determine JAVA_HOME from javac".to_string())?
        .to_path_buf();
    if !home.join("include").join("jni.h").is_file() {
        return Err(format!(
            "the javac installation '{}' has no include/jni.h",
            home.display()
        ));
    }
    Ok(home)
}

fn work_dir(input: &LibraryInput, name: &str) -> Result<PathBuf, String> {
    let directory = foreign_output_dir(&input.root)?.join(format!(
        ".{}-{}",
        name,
        std::process::id()
    ));
    fs::create_dir(&directory)
        .map_err(|error| format!("could not create foreign build directory: {error}"))?;
    Ok(directory)
}

fn cleanup_work(directory: &Path) {
    let _ = fs::remove_dir_all(directory);
}

fn link_runtime_args(command: &mut Command, work: &Path, runtime_name: &str) {
    command
        .arg("-L")
        .arg(work)
        .arg(format!("-Wl,-rpath,{}", runtime_rpath()))
        .arg(format!("-l:{runtime_name}"));
}

fn runtime_rpath() -> &'static str {
    if cfg!(target_os = "macos") {
        "@loader_path"
    } else {
        "$ORIGIN"
    }
}

fn build_node_addon(input: &LibraryInput, addon: &str) -> Result<Vec<u8>, String> {
    if cfg!(target_os = "windows") {
        return Err("N-API packaging is not supported for the Windows target yet".to_string());
    }
    let work = work_dir(input, "node")?;
    let result = (|| {
        fs::write(work.join(&input.runtime_name), &input.runtime)
            .map_err(|error| format!("could not stage the Node runtime: {error}"))?;
        fs::write(work.join("api.h"), &input.header)
            .map_err(|error| format!("could not stage the C ABI header: {error}"))?;
        let source = render_node_addon(input);
        let source_path = work.join("addon.c");
        fs::write(&source_path, source.as_bytes())
            .map_err(|error| format!("could not write the N-API bridge: {error}"))?;
        let output = work.join(addon);
        let mut command = Command::new("cc");
        command
            .args(["-std=c11", "-shared", "-fPIC"])
            .arg(&source_path)
            .arg("-I")
            .arg(&work);
        if cfg!(target_os = "macos") {
            command.arg("-Wl,-undefined,dynamic_lookup");
        }
        link_runtime_args(&mut command, &work, &input.runtime_name);
        command.arg("-o").arg(&output);
        run_host_tool(&mut command, "cc")?;
        read_regular_file(&output, "N-API addon")
    })();
    cleanup_work(&work);
    result
}

fn render_node_addon(input: &LibraryInput) -> String {
    let mut output = String::from(
        "#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n#include <stdlib.h>\n#include \"api.h\"\n\
         typedef struct napi_env__ *napi_env;\n\
         typedef struct napi_value__ *napi_value;\n\
         typedef struct napi_callback_info__ *napi_callback_info;\n\
         typedef int32_t napi_status;\n\
         typedef napi_value (*napi_callback)(napi_env, napi_callback_info);\n\
         enum { napi_ok = 0 };\n\
         extern napi_status napi_get_cb_info(napi_env, napi_callback_info, size_t *, napi_value *, napi_value *, void **);\n\
         extern napi_status napi_get_value_int64(napi_env, napi_value, int64_t *);\n\
         extern napi_status napi_get_value_double(napi_env, napi_value, double *);\n\
         extern napi_status napi_get_value_bool(napi_env, napi_value, bool *);\n\
         extern napi_status napi_get_value_string_utf8(napi_env, napi_value, char *, size_t, size_t *);\n\
         extern napi_status napi_create_int64(napi_env, int64_t, napi_value *);\n\
         extern napi_status napi_create_double(napi_env, double, napi_value *);\n\
         extern napi_status napi_get_boolean(napi_env, bool, napi_value *);\n\
         extern napi_status napi_create_string_utf8(napi_env, const char *, size_t, napi_value *);\n\
         extern napi_status napi_create_function(napi_env, const char *, size_t, napi_callback, void *, napi_value *);\n\
         extern napi_status napi_set_named_property(napi_env, napi_value, const char *, napi_value);\n\
         extern napi_status napi_throw_error(napi_env, const char *, const char *);\n\
         static napi_value jet_napi_error(napi_env env, const char *message) {\n\
             napi_throw_error(env, NULL, message);\n\
             return NULL;\n\
         }\n\n",
    );
    for (index, export) in input.stamp.exports.iter().enumerate() {
        let params = usize::try_from(export.params).unwrap_or(0);
        let _ = writeln!(
            output,
            "static napi_value jet_napi_{index}(napi_env env, napi_callback_info info) {{"
        );
        let _ = writeln!(
            output,
            "    napi_value argv[{}]; size_t argc = {params};",
            params.max(1)
        );
        output.push_str(
            "    if (napi_get_cb_info(env, info, &argc, argv, NULL, NULL) != napi_ok || argc != ",
        );
        let _ = writeln!(output, "{params}) return jet_napi_error(env, \"invalid argument count\");");
        for param in 0..params {
            match export.scalar {
                JetLibScalar::Int => {
                    let _ = writeln!(
                        output,
                        "    int64_t arg{param}; if (napi_get_value_int64(env, argv[{param}], &arg{param}) != napi_ok) return jet_napi_error(env, \"expected an integer\");"
                    );
                }
                JetLibScalar::Float => {
                    let _ = writeln!(
                        output,
                        "    double arg{param}; if (napi_get_value_double(env, argv[{param}], &arg{param}) != napi_ok) return jet_napi_error(env, \"expected a number\");"
                    );
                }
                JetLibScalar::Bool => {
                    let _ = writeln!(
                        output,
                        "    bool arg{param}; if (napi_get_value_bool(env, argv[{param}], &arg{param}) != napi_ok) return jet_napi_error(env, \"expected a boolean\");"
                    );
                }
                JetLibScalar::Text => {
                    let _ = writeln!(
                        output,
                        "    size_t len{param} = 0; char *raw{param} = NULL; if (napi_get_value_string_utf8(env, argv[{param}], NULL, 0, &len{param}) != napi_ok || len{param} > (size_t)-1 - 1) return jet_napi_error(env, \"expected UTF-8 text\");"
                    );
                    let _ = writeln!(
                        output,
                        "    raw{param} = (char *)malloc(len{param} + 1); if (raw{param} == NULL) return jet_napi_error(env, \"could not allocate text argument\");"
                    );
                    let _ = writeln!(
                        output,
                        "    if (napi_get_value_string_utf8(env, argv[{param}], raw{param}, len{param} + 1, &len{param}) != napi_ok) {{ free(raw{param}); return jet_napi_error(env, \"expected UTF-8 text\"); }}"
                    );
                    let _ = writeln!(
                        output,
                        "    JetText arg{param} = {{ (const uint8_t *)raw{param}, len{param} }};"
                    );
                }
            }
        }
        let args = (0..params)
            .map(|param| format!("arg{param}"))
            .collect::<Vec<_>>()
            .join(", ");
        match export.scalar {
            JetLibScalar::Int => {
                let _ = writeln!(
                    output,
                    "    int64_t result = {}({args});",
                    export.symbol
                );
                for param in 0..params {
                    if export.scalar == JetLibScalar::Text {
                        let _ = writeln!(output, "    free(raw{param});");
                    }
                }
                output.push_str("    napi_value value; if (napi_create_int64(env, result, &value) != napi_ok) return jet_napi_error(env, \"could not create result\"); return value;\n");
            }
            JetLibScalar::Float => {
                let _ = writeln!(
                    output,
                    "    double result = {}({args});",
                    export.symbol
                );
                output.push_str("    napi_value value; if (napi_create_double(env, result, &value) != napi_ok) return jet_napi_error(env, \"could not create result\"); return value;\n");
            }
            JetLibScalar::Bool => {
                let _ = writeln!(
                    output,
                    "    bool result = {}({args});",
                    export.symbol
                );
                output.push_str("    napi_value value; if (napi_get_boolean(env, result, &value) != napi_ok) return jet_napi_error(env, \"could not create result\"); return value;\n");
            }
            JetLibScalar::Text => {
                let _ = writeln!(
                    output,
                    "    JetText result = {}({args});",
                    export.symbol
                );
                for param in 0..params {
                    let _ = writeln!(output, "    free(raw{param});");
                }
                output.push_str("    if (result.len != 0 && result.ptr == NULL) { jet_text_free(result); return jet_napi_error(env, \"JetText result has an invalid pointer\"); }\n");
                output.push_str("    napi_value value; if (napi_create_string_utf8(env, (const char *)result.ptr, result.len, &value) != napi_ok) { jet_text_free(result); return jet_napi_error(env, \"could not create text result\"); } jet_text_free(result); return value;\n");
            }
        }
        output.push_str("}\n\n");
    }
    output.push_str(
        "__attribute__((visibility(\"default\"))) napi_value napi_register_module_v1(napi_env env, napi_value exports) {\n",
    );
    for (index, export) in input.stamp.exports.iter().enumerate() {
        let _ = writeln!(
            output,
            "    napi_value fn{index}; if (napi_create_function(env, {}, {}, jet_napi_{index}, NULL, &fn{index}) != napi_ok || napi_set_named_property(env, exports, {}, fn{index}) != napi_ok) return NULL;",
            json_string(&export.name),
            export.name.len(),
            json_string(&export.name)
        );
    }
    output.push_str("    return exports;\n}\n");
    output
}

fn build_java_classes(
    input: &LibraryInput,
    package: &str,
    class_name: &str,
    jni_name: &str,
) -> Result<Vec<u8>, String> {
    let work = work_dir(input, "java-classes")?;
    let result = (|| {
        let source = render_java(input, package, class_name, jni_name);
        let source_path = work.join(format!("{class_name}.java"));
        fs::write(&source_path, source.as_bytes())
            .map_err(|error| format!("could not write the Java wrapper: {error}"))?;
        let classes = work.join("classes");
        fs::create_dir(&classes)
            .map_err(|error| format!("could not create Java class directory: {error}"))?;
        let mut command = Command::new("javac");
        command
            .args(["-encoding", "UTF-8", "-d"])
            .arg(&classes)
            .arg(&source_path);
        run_host_tool(&mut command, "javac")?;
        let class_path = classes
            .join(package.replace('.', "/"))
            .join(format!("{class_name}.class"));
        read_regular_file(&class_path, "JVM wrapper class")
    })();
    cleanup_work(&work);
    result
}

fn render_java(
    input: &LibraryInput,
    package: &str,
    class_name: &str,
    jni_name: &str,
) -> String {
    let platform = shared_platform();
    let mut output = format!(
        "package {package};\n\n\
         import java.io.IOException;\n\
         import java.io.InputStream;\n\
         import java.nio.file.Files;\n\
         import java.nio.file.Path;\n\
         import java.nio.file.StandardCopyOption;\n\n\
         public final class {class_name} {{\n\
             private static final String PLATFORM = \"{platform}\";\n\
             private static final String JNI_NAME = \"{jni_name}\";\n\
             private static final String RUNTIME_NAME = \"{runtime}\";\n\
             static {{ loadNative(); }}\n\
             private {class_name}() {{}}\n\
             private static void loadNative() {{\n\
                 try {{\n\
                     Path directory = Files.createTempDirectory(\"jet-native-\");\n\
                     copyResource(\"/META-INF/native/\" + PLATFORM + \"/\" + JNI_NAME, directory.resolve(JNI_NAME));\n\
                     copyResource(\"/META-INF/native/\" + PLATFORM + \"/\" + RUNTIME_NAME, directory.resolve(RUNTIME_NAME));\n\
                     System.load(directory.resolve(JNI_NAME).toString());\n\
                 }} catch (IOException error) {{\n\
                     throw new ExceptionInInitializerError(error);\n\
                 }}\n\
             }}\n\
             private static void copyResource(String resource, Path destination) throws IOException {{\n\
                 try (InputStream input = {class_name}.class.getResourceAsStream(resource)) {{\n\
                     if (input == null) throw new IOException(\"missing bundled Jet native runtime\");\n\
                     Files.copy(input, destination, StandardCopyOption.REPLACE_EXISTING);\n\
                 }}\n\
             }}\n",
        runtime = input.runtime_name
    );
    for (index, export) in input.stamp.exports.iter().enumerate() {
        let ty = java_type(export.scalar);
        let args = (0..export.params)
            .map(|param| format!("{ty} p{param}"))
            .collect::<Vec<_>>()
            .join(", ");
        let call_args = (0..export.params)
            .map(|param| format!("p{param}"))
            .collect::<Vec<_>>()
            .join(", ");
        let public_name = java_public_name(&export.name);
        let _ = writeln!(
            output,
            "             private static native {ty} jetCall{index}({args});\n\
             public static {ty} {public_name}({args}) {{ return jetCall{index}({call_args}); }}",
        );
    }
    output.push_str("}\n");
    output
}

fn java_type(value: JetLibScalar) -> &'static str {
    match value {
        JetLibScalar::Int => "long",
        JetLibScalar::Float => "double",
        JetLibScalar::Bool => "boolean",
        JetLibScalar::Text => "String",
    }
}

fn java_public_name(value: &str) -> String {
    let name = normalized_component(value, false);
    if [
        "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char",
        "class", "const", "continue", "default", "do", "double", "else", "enum",
        "extends", "final", "finally", "float", "for", "goto", "if", "implements",
        "import", "instanceof", "int", "interface", "long", "native", "new",
        "package", "private", "protected", "public", "return", "short", "static",
        "strictfp", "super", "switch", "synchronized", "this", "throw", "throws",
        "transient", "try", "void", "volatile", "while",
    ]
    .contains(&name.as_str())
    {
        format!("jet_{name}")
    } else {
        name
    }
}

fn build_jni_bridge(
    input: &LibraryInput,
    package: &str,
    class_name: &str,
    jni_name: &str,
) -> Result<Vec<u8>, String> {
    if cfg!(target_os = "windows") {
        return Err("JVM packaging is not supported for the Windows target yet".to_string());
    }
    let home = java_home()?;
    let work = work_dir(input, "jni")?;
    let result = (|| {
        fs::write(work.join(&input.runtime_name), &input.runtime)
            .map_err(|error| format!("could not stage the JVM runtime: {error}"))?;
        fs::write(work.join("api.h"), &input.header)
            .map_err(|error| format!("could not stage the C ABI header: {error}"))?;
        let source = render_jni_bridge(input, package, class_name);
        let source_path = work.join("jni.c");
        fs::write(&source_path, source.as_bytes())
            .map_err(|error| format!("could not write the JNI bridge: {error}"))?;
        let output = work.join(jni_name);
        let mut command = Command::new("cc");
        command
            .args(["-std=c11", "-shared", "-fPIC", "-I"])
            .arg(home.join("include"))
            .arg("-I")
            .arg(home.join("include").join(jni_include_dir()))
            .arg(&source_path)
            .arg("-I")
            .arg(&work);
        link_runtime_args(&mut command, &work, &input.runtime_name);
        command.arg("-o").arg(&output);
        run_host_tool(&mut command, "cc")?;
        read_regular_file(&output, "JNI bridge")
    })();
    cleanup_work(&work);
    result
}

fn jni_include_dir() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

fn jni_mangle(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => output.push(ch),
            '_' => output.push_str("_1"),
            ';' => output.push_str("_2"),
            '[' => output.push_str("_3"),
            ch => {
                let _ = write!(output, "_0{:04x}", ch as u32);
            }
        }
    }
    output
}

fn render_jni_bridge(input: &LibraryInput, package: &str, class_name: &str) -> String {
    let prefix = format!(
        "Java_{}_{}_",
        jni_mangle(package),
        jni_mangle(class_name)
    );
    let has_text = input
        .stamp
        .exports
        .iter()
        .any(|export| export.scalar == JetLibScalar::Text);
    let mut output = String::from(
        "#include <jni.h>\n#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n#include <stdlib.h>\n#include <string.h>\n#include \"api.h\"\n\
         static void jet_jni_throw(JNIEnv *env, const char *message) {\n\
             jclass type = (*env)->FindClass(env, \"java/lang/IllegalStateException\");\n\
             if (type != NULL) { (*env)->ThrowNew(env, type, message); (*env)->DeleteLocalRef(env, type); }\n\
         }\n",
    );
    if !has_text {
        output.push('\n');
    }
    for (index, export) in input.stamp.exports.iter().enumerate() {
        let result_type = jni_type(export.scalar);
        let _ = write!(
            output,
            "\nJNIEXPORT {result_type} JNICALL {prefix}jetCall{index}(JNIEnv *env, jclass ignored",
        );
        for param in 0..export.params {
            let _ = write!(
                output,
                ", {} arg{param}",
                jni_type(export.scalar)
            );
        }
        output.push_str(") {\n    (void)ignored;\n");
        let params = usize::try_from(export.params).unwrap_or(0);
        if export.scalar == JetLibScalar::Text {
            for param in 0..params {
                let _ = writeln!(
                    output,
                    "    const char *raw{param} = (*env)->GetStringUTFChars(env, arg{param}, NULL); if (raw{param} == NULL) {{ jet_jni_throw(env, \"could not read UTF-8 argument\");"
                );
                for prior in 0..param {
                    let _ = writeln!(
                        output,
                        "        (*env)->ReleaseStringUTFChars(env, arg{prior}, raw{prior});"
                    );
                }
                output.push_str("        return NULL; }\n");
                let _ = writeln!(
                    output,
                    "    JetText value{param} = {{ (const uint8_t *)raw{param}, (size_t)(*env)->GetStringUTFLength(env, arg{param}) }};"
                );
            }
            let args = (0..params)
                .map(|param| format!("value{param}"))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                output,
                "    JetText result = {}({args});",
                export.symbol
            );
            for param in 0..params {
                let _ = writeln!(
                    output,
                    "    (*env)->ReleaseStringUTFChars(env, arg{param}, raw{param});"
                );
            }
            output.push_str("    if (result.len != 0 && result.ptr == NULL) { jet_jni_throw(env, \"JetText result has an invalid pointer\"); return NULL; }\n");
            output.push_str("    if (result.len > (size_t)-1 - 1) { jet_jni_throw(env, \"JetText result is too large\"); return NULL; }\n");
            output.push_str("    char *copy = (char *)malloc(result.len + 1); if (copy == NULL) { jet_jni_throw(env, \"could not allocate text result\"); return NULL; } if (result.len != 0) memcpy(copy, result.ptr, result.len); copy[result.len] = 0; jstring value = (*env)->NewStringUTF(env, copy); free(copy); jet_text_free(result); if (value == NULL) { jet_jni_throw(env, \"could not create text result\"); return NULL; } return value;\n");
        } else {
            let args = (0..params)
                .map(|param| format!("arg{param}"))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                output,
                "    return ({}){}({args});",
                result_type,
                export.symbol
            );
        }
        output.push_str("}\n");
    }
    output
}

fn jni_type(value: JetLibScalar) -> &'static str {
    match value {
        JetLibScalar::Int => "jlong",
        JetLibScalar::Float => "jdouble",
        JetLibScalar::Bool => "jboolean",
        JetLibScalar::Text => "jstring",
    }
}

fn build_dotnet_assembly(input: &LibraryInput, package: &str) -> Result<Vec<u8>, String> {
    let work = work_dir(input, "dotnet")?;
    let result = (|| {
        let project = format!(
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Library</OutputType><AssemblyName>{}</AssemblyName><RootNamespace>Jet</RootNamespace><ImplicitUsings>disable</ImplicitUsings><Nullable>enable</Nullable><RestoreIgnoreFailedSources>true</RestoreIgnoreFailedSources></PropertyGroup></Project>",
            xml_string(package)
        );
        fs::write(work.join("Library.csproj"), project.as_bytes())
            .map_err(|error| format!("could not write the .NET project: {error}"))?;
        fs::write(work.join("Library.cs"), render_csharp(input, package).as_bytes())
            .map_err(|error| format!("could not write the C# wrapper: {error}"))?;
        let mut command = Command::new("dotnet");
        command
            .arg("build")
            .arg(work.join("Library.csproj"))
            .args([
                "-c",
                "Release",
                "--nologo",
                "-v:q",
                "--disable-build-servers",
                "-p:RestoreIgnoreFailedSources=true",
            ]);
        run_host_tool(&mut command, "dotnet")?;
        read_regular_file(
            &work
                .join("bin")
                .join("Release")
                .join("net8.0")
                .join(format!("{package}.dll")),
            ".NET wrapper assembly",
        )
    })();
    cleanup_work(&work);
    result
}

fn render_csharp(input: &LibraryInput, _package: &str) -> String {
    let namespace = format!("Jet.{}", normalized_component(&input.package_name, false));
    let mut output = format!(
        "using System;\nusing System.Runtime.InteropServices;\nusing System.Text;\n\nnamespace {namespace} {{\n\
         public sealed class JetCallException : Exception {{ public JetCallException(string message, Exception? inner = null) : base(message, inner) {{ }} }}\n\
         public static class Library {{\n\
             [StructLayout(LayoutKind.Sequential)] private struct JetText {{ public IntPtr Ptr; public UIntPtr Len; }}\n\
             private static class Native {{\n",
    );
    for (index, export) in input.stamp.exports.iter().enumerate() {
        let ty = csharp_type(export.scalar);
        let args = (0..export.params)
            .map(|param| format!("{} arg{param}", ty))
            .collect::<Vec<_>>()
            .join(", ");
        if export.scalar == JetLibScalar::Bool {
            let _ = writeln!(
                output,
                "                 [return: MarshalAs(UnmanagedType.I1)] [DllImport(\"{}\", EntryPoint = \"{}\", CallingConvention = CallingConvention.Cdecl)] internal static extern bool Call{index}({});",
                input.runtime_name,
                export.symbol,
                args.replace("bool ", "[MarshalAs(UnmanagedType.I1)] bool ")
            );
        } else {
            let _ = writeln!(
                output,
                "                 [DllImport(\"{}\", EntryPoint = \"{}\", CallingConvention = CallingConvention.Cdecl)] internal static extern {} Call{index}({});",
                input.runtime_name,
                export.symbol,
                ty,
                args
            );
        }
    }
    if input
        .stamp
        .exports
        .iter()
        .any(|export| export.scalar == JetLibScalar::Text)
    {
        let _ = writeln!(
            output,
            "                 [DllImport(\"{}\", EntryPoint = \"jet_text_free\", CallingConvention = CallingConvention.Cdecl)] internal static extern void Free(JetText value);",
            input.runtime_name
        );
    }
    output.push_str("             }\n");
    for (index, export) in input.stamp.exports.iter().enumerate() {
        let ty = csharp_type(export.scalar);
        let args = (0..export.params)
            .map(|param| format!("{ty} arg{param}"))
            .collect::<Vec<_>>()
            .join(", ");
        let names = (0..export.params)
            .map(|param| format!("arg{param}"))
            .collect::<Vec<_>>()
            .join(", ");
        let public_name = csharp_public_name(&export.name);
        if export.scalar == JetLibScalar::Text {
            output.push_str(&format!(
                "             public static string {public_name}({args}) {{\n"
            ));
            for param in 0..export.params {
                let _ = writeln!(
                    output,
                    "                 using var text{param} = TextArgument.Create(arg{param});"
                );
            }
            let call_args = (0..export.params)
                .map(|param| format!("text{param}.Value"))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                output,
                "                 return ReadText(Native.Call{index}({call_args}));\n             }}"
            );
        } else {
            let _ = writeln!(
                output,
                "             public static {ty} {public_name}({args}) => Native.Call{index}({names});"
            );
        }
    }
    if input
        .stamp
        .exports
        .iter()
        .any(|export| export.scalar == JetLibScalar::Text)
    {
        output.push_str(
            "             private static string ReadText(JetText value) {\n\
                 try {\n\
                     ulong length = value.Len.ToUInt64();\n\
                     if (length > int.MaxValue) throw new JetCallException(\"JetText result is too large\");\n\
                     if (length != 0 && value.Ptr == IntPtr.Zero) throw new JetCallException(\"JetText result has an invalid pointer\");\n\
                     if (length == 0) return string.Empty;\n\
                     byte[] bytes = new byte[(int)length]; Marshal.Copy(value.Ptr, bytes, 0, bytes.Length);\n\
                     return new UTF8Encoding(false, true).GetString(bytes);\n\
                 } catch (DecoderFallbackException error) {\n\
                     throw new JetCallException(\"JetText contains invalid UTF-8\", error);\n\
                 } finally { Native.Free(value); }\n\
             }\n\
             private sealed class TextArgument : IDisposable {\n\
                 private readonly IntPtr pointer; public JetText Value { get; }\n\
                 private TextArgument(IntPtr pointer, UIntPtr length) { this.pointer = pointer; Value = new JetText { Ptr = pointer, Len = length }; }\n\
                 public static TextArgument Create(string value) {\n\
                     if (value is null) throw new JetCallException(\"text argument cannot be null\");\n\
                     byte[] bytes = Encoding.UTF8.GetBytes(value); IntPtr pointer = Marshal.AllocHGlobal(Math.Max(1, bytes.Length));\n\
                     if (bytes.Length != 0) Marshal.Copy(bytes, 0, pointer, bytes.Length);\n\
                     return new TextArgument(pointer, new UIntPtr((ulong)bytes.Length));\n\
                 }\n\
                 public void Dispose() { if (pointer != IntPtr.Zero) Marshal.FreeHGlobal(pointer); }\n\
             }\n",
        );
    }
    output.push_str("         }\n}\n");
    output
}

fn csharp_type(value: JetLibScalar) -> &'static str {
    match value {
        JetLibScalar::Int => "long",
        JetLibScalar::Float => "double",
        JetLibScalar::Bool => "bool",
        JetLibScalar::Text => "JetText",
    }
}

fn csharp_public_name(value: &str) -> String {
    let name = normalized_component(value, false);
    if [
        "abstract", "as", "base", "bool", "break", "byte", "case", "catch", "char",
        "checked", "class", "const", "continue", "decimal", "default", "delegate",
        "do", "double", "else", "enum", "event", "explicit", "extern", "false",
        "finally", "fixed", "float", "for", "foreach", "goto", "if", "implicit",
        "in", "int", "interface", "internal", "is", "lock", "long", "namespace",
        "new", "null", "object", "operator", "out", "override", "params", "private",
        "protected", "public", "readonly", "ref", "return", "sbyte", "sealed",
        "short", "sizeof", "stackalloc", "static", "string", "struct", "switch",
        "this", "throw", "true", "try", "typeof", "uint", "ulong", "unchecked",
        "unsafe", "ushort", "using", "virtual", "void", "volatile", "while",
        "yield",
    ]
    .contains(&name.as_str())
    {
        format!("Jet_{name}")
    } else {
        name
    }
}

fn render_pom(input: &LibraryInput, artifact: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project xmlns=\"http://maven.apache.org/POM/4.0.0\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd\">\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>dev.jet</groupId>\n  <artifactId>{}</artifactId>\n  <version>{}</version>\n  <packaging>jar</packaging>\n  <name>Jet native library {}</name>\n  <licenses><license><name>{}</name><distribution>repo</distribution></license></licenses>\n  <properties><jet.releaseIdentity>{}</jet.releaseIdentity><jet.sourceIdentity>{}</jet.sourceIdentity><jet.abiDigest>{}</jet.abiDigest></properties>\n</project>\n",
        xml_string(artifact),
        xml_string(&input.version),
        xml_string(&input.library_name),
        xml_string(&input.license),
        xml_string(&input.release_identity),
        xml_string(&input.source_identity),
        xml_string(&input.abi_digest),
    )
}

fn render_nuspec(input: &LibraryInput, package: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<package xmlns=\"http://schemas.microsoft.com/packaging/2013/05/nuspec.xsd\">\n  <metadata>\n    <id>{}</id><version>{}</version><authors>Jet package publisher</authors><owners>Jet</owners>\n    <description>Typed native bindings for Jet library {}</description>\n    <license type=\"expression\">{}</license>\n    <repository type=\"git\" url=\"https://jet-lang.dev/source/{}\" />\n    <tags>jet native ffi abi</tags>\n    <dependencies />\n  </metadata>\n</package>\n",
        xml_string(package),
        xml_string(&input.version),
        xml_string(&input.library_name),
        xml_string(&input.license),
        xml_string(&input.source_identity),
    )
}

fn registry_endpoint(registry: ForeignRegistry) -> Result<String, String> {
    let specific = match registry {
        ForeignRegistry::PyPi => "JET_PYPI_URL",
        ForeignRegistry::Npm => "JET_NPM_REGISTRY",
        ForeignRegistry::Maven => "JET_MAVEN_URL",
        ForeignRegistry::NuGet => "JET_NUGET_SOURCE",
    };
    let endpoint = env::var(specific)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("JET_FOREIGN_REGISTRY_URL")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .ok_or_else(|| {
            format!(
                "foreign {} publication needs {specific} or JET_FOREIGN_REGISTRY_URL; use file:///absolute/path for offline publication",
                registry.as_str()
            )
        })?;
    if endpoint.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("foreign registry endpoint contains whitespace or control characters".to_string());
    }
    if endpoint.contains('@') {
        return Err(
            "foreign registry endpoint must not contain embedded credentials; use the registry credential helper"
                .to_string(),
        );
    }
    if let Some(path) = endpoint.strip_prefix("file://") {
        if path.is_empty() || !Path::new(path).is_absolute() {
            return Err("file registry endpoint must name an absolute path".to_string());
        }
        return Ok(endpoint);
    }
    if endpoint.starts_with("https://") || endpoint.starts_with("http://") {
        return Ok(endpoint);
    }
    Err("foreign registry endpoint must use http(s):// or file://".to_string())
}

fn publish_archive(
    registry: ForeignRegistry,
    endpoint: &str,
    report: &ForeignPublishReport,
) -> Result<(), String> {
    let bytes = read_regular_file(&report.artifact, "foreign package")?;
    let digest = format!("sha256-{}", crate::SHA256::sha256_hex(&bytes));
    if digest != report.artifact_digest {
        return Err("foreign package changed after validation; refusing publication".to_string());
    }
    if let Some(directory) = endpoint.strip_prefix("file://") {
        return publish_to_file_registry(directory, &report.artifact, &bytes);
    }
    let mut command = match registry {
        ForeignRegistry::PyPi => {
            let mut command = Command::new("twine");
            command.args(["upload", "--repository-url", endpoint]);
            command.arg(&report.artifact);
            command
        }
        ForeignRegistry::Npm => {
            let mut command = Command::new("npm");
            command.args(["publish", "--registry", endpoint, "--ignore-scripts"]);
            command.arg(&report.artifact);
            command
        }
        ForeignRegistry::Maven => {
            let mut command = Command::new("mvn");
            let artifact_id = report
                .artifact
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("jet-library")
                .strip_suffix(".jar")
                .unwrap_or("jet-library");
            command.args([
                "deploy:deploy-file",
                "-B",
                "-DgeneratePom=false",
                "-DrepositoryId=jet",
            ]);
            command.arg(format!("-Durl={endpoint}"));
            command.arg(format!("-Dfile={}", report.artifact.display()));
            command.arg(format!("-DartifactId={artifact_id}"));
            command.arg("-DgroupId=dev.jet");
            command
        }
        ForeignRegistry::NuGet => {
            let mut command = Command::new("dotnet");
            command.args(["nuget", "push", "--source", endpoint]);
            command.arg(&report.artifact);
            command
        }
    };
    run_host_tool(&mut command, registry.as_str())
}

fn publish_to_file_registry(
    directory: &str,
    artifact: &Path,
    bytes: &[u8],
) -> Result<(), String> {
    let root = Path::new(directory);
    if let Ok(metadata) = fs::symlink_metadata(root) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "file registry '{}' is not a real directory",
                root.display()
            ));
        }
    } else {
        fs::create_dir_all(root)
            .map_err(|error| format!("could not create file registry '{}': {error}", root.display()))?;
    }
    let name = artifact
        .file_name()
        .ok_or_else(|| "foreign artifact has no file name".to_string())?;
    let destination = root.join(name);
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(format!(
            "foreign registry already contains version artifact '{}'",
            name.to_string_lossy()
        ));
    }
    write_atomic(&destination, bytes)
}
