//! The single Hangar-backed C/C++ toolchain descriptor.
//!
//! A C driver never discovers a compiler from the host.  It consumes one
//! verified Hangar object whose producer record names the compiler, C++
//! compiler, linker, sysroot, host, target, ABI, and every content digest.
//! The optional fixture manifest is only an acquisition input for tests and
//! offline bundles; the resulting object is still verified and recorded by
//! the ordinary Jetpack Store boundary.

use super::{
    cache_identity, host_nix_system, Ctx, DownloadPlan, PlanItem, PlanState, ProviderError,
    Realized, SourceState,
};
use crate::Comptime::Build::{
    BuildProvenance, LinkerIdentity, LockRecord, SdkIdentity, SysrootIdentity, ToolchainSpec,
};
use crate::NixIndex::{IndexKey, IndexTrustTier, NixIndexClient, VerifiedIndexRecord};
use crate::RefSpec::{RefSpec as PackageRef, Source};
use crate::Store::{
    admit_nix_closure_with_progress, plan_nix_downloads, AdmittedNixClosure, CacheExpectation,
    NixOutputRequest, ProducerRecord, Roots, StoreEntry,
};
use crate::{Envelope, JSON, SHA256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub(crate) const PACKAGE: &str = "cc-toolchain";
pub(crate) const SOURCE: &str = "jetpack";
pub(crate) const RECIPE_ID: &str = "jet-cc-toolchain-v1";
const PRODUCER: &str = "jet-cc";
const NIX_PRODUCER: &str = "nix";
const SCHEMA: &str = "1";
const FIXTURE_MANIFEST: &str = "jet-cc-toolchain";
const FIXTURE_LAYOUT: &str = "fixture-v1";
const NIX_LAYOUT: &str = "nix-index-hangar-v1";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_BUNDLE_NODES: usize = 250_000;
const MAX_BUNDLE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_NIX_OBJECTS: usize = 250_000;
const MAX_MARKER_BYTES: u64 = 16 * 1024;

/// D-ADOPT-CC1=B acquisition record.
///
/// This record is intentionally a small, reviewable description. The compiler
/// and linker bytes are selected from the exact signed Nix index records below;
/// the Nix cache then admits their complete signed closure into Hangar. No
/// compiler payload is vendored in the repository and no host executable is a
/// fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NixAcquisitionRecord {
    id: &'static str,
    channel: &'static str,
    revision: &'static str,
    system: &'static str,
    target: &'static str,
    compiler_attr: &'static [&'static str],
    linker_attr: &'static [&'static str],
    compiler_path: &'static str,
    cxx_path: &'static str,
    linker_path: &'static str,
    sysroot_marker: &'static str,
    abi: &'static str,
}

/// The first production record is deliberately scoped to the platform for
/// which the signed feed currently publishes both `gcc` and `lld` outputs.
/// Adding another target requires another signed index/cache record, not a
/// host-PATH guess or a copy of this target's sysroot.
const NIX_ACQUISITION_RECORDS: &[NixAcquisitionRecord] = &[NixAcquisitionRecord {
    id: "jet-cc-nixos-unstable-e5bdc4a4-x86_64-linux-v1",
    channel: "nixos-unstable",
    revision: "e5bdc4a41d4c072fe1e3787eaa0320a384741d44",
    system: "x86_64-linux",
    target: "x86_64-unknown-linux-gnu",
    compiler_attr: &["gcc"],
    linker_attr: &["lld"],
    compiler_path: "bin/gcc",
    cxx_path: "bin/g++",
    linker_path: "bin/ld.lld",
    sysroot_marker: "nix-support/orig-libc-dev",
    abi: "gnu",
}];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CcToolchainDescriptor {
    pub(crate) entry_id: String,
    pub(crate) root: PathBuf,
    pub(crate) host: String,
    pub(crate) target: String,
    pub(crate) version: String,
    pub(crate) abi: String,
    pub(crate) compiler: String,
    pub(crate) compiler_sha256: String,
    pub(crate) cxx: String,
    pub(crate) cxx_sha256: String,
    pub(crate) linker: String,
    pub(crate) linker_sha256: String,
    pub(crate) sysroot: String,
    pub(crate) sysroot_sha256: String,
    pub(crate) bundle_sha256: String,
    pub(crate) layout: String,
    pub(crate) compiler_root: PathBuf,
    pub(crate) cxx_root: PathBuf,
    pub(crate) linker_root: PathBuf,
    pub(crate) sysroot_root: PathBuf,
    pub(crate) mounts: Vec<CcMount>,
    pub(crate) references: Vec<String>,
    pub(crate) envelope: Envelope::Envelope,
    pub(crate) producer: ProducerRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CcMount {
    pub(crate) store_path: String,
    pub(crate) digest: String,
    pub(crate) source: PathBuf,
    pub(crate) destination: PathBuf,
}

impl CcToolchainDescriptor {
    pub(crate) fn virtual_root(&self) -> PathBuf {
        PathBuf::from("/jet/toolchains").join(&self.bundle_sha256)
    }

    pub(crate) fn virtual_bin(&self) -> PathBuf {
        if self.is_nix_layout() {
            self.role_virtual_root("compiler").join("bin")
        } else {
            self.virtual_root().join("bin")
        }
    }

    pub(crate) fn sysroot_path(&self) -> PathBuf {
        if self.is_nix_layout() {
            self.role_virtual_root("sysroot")
        } else {
            self.virtual_root().join(&self.sysroot)
        }
    }

    pub(crate) fn virtual_path_env(&self) -> String {
        let mut paths = vec![self.virtual_bin()];
        if self.is_nix_layout() {
            paths.push(self.role_virtual_root("linker").join("bin"));
            paths.extend(
                self.mounts
                    .iter()
                    .map(|mount| mount.destination.join("bin")),
            );
        }
        std::env::join_paths(paths)
            .map(|paths| paths.to_string_lossy().into_owned())
            .unwrap_or_else(|_| self.virtual_bin().to_string_lossy().into_owned())
    }

    pub(crate) fn identity(&self) -> String {
        format!("{RECIPE_ID}\n{}", source_fingerprint(self))
    }

    pub(crate) fn cache_identity(&self, ctx: &Ctx<'_>) -> crate::Store::CacheIdentity {
        cache_identity(&source_fingerprint(self), RECIPE_ID, ctx)
    }

    pub(crate) fn toolchain_spec(&self) -> ToolchainSpec {
        let provenance = BuildProvenance::jetpack_dependency(
            format!("{PACKAGE}@{SOURCE}"),
            LockRecord::new("hangar", self.bundle_sha256.clone()),
        );
        let mut spec = ToolchainSpec::target(self.target.clone(), provenance.clone())
            .with_host_triple(self.host.clone())
            .with_sdk(SdkIdentity::new(
                "jet-cc",
                self.version.clone(),
                provenance.clone(),
            ))
            .with_linker(LinkerIdentity::new(
                format!("{}@{}", self.linker, self.linker_sha256),
                provenance.clone(),
            ))
            .with_sysroot(SysrootIdentity::new(
                self.sysroot.clone(),
                self.sysroot_sha256.clone(),
                provenance.clone(),
            ))
            .with_tool("cc", self.compiler_virtual_path())
            .with_tool("c++", self.cxx_virtual_path())
            .with_tool("ld", self.linker_virtual_path())
            .declared_only();
        if self.is_nix_layout() {
            for mount in &self.mounts {
                spec = spec.with_read_only_mount_identity(
                    mount.source.to_string_lossy(),
                    mount.destination.to_string_lossy(),
                    mount.digest.clone(),
                );
            }
            let linker_digest = self.linker_root_digest();
            for (role, root, digest) in [
                ("compiler", &self.compiler_root, self.bundle_sha256.as_str()),
                ("linker", &self.linker_root, linker_digest.as_str()),
                ("sysroot", &self.sysroot_root, self.sysroot_sha256.as_str()),
            ] {
                spec = spec.with_read_only_mount_identity(
                    root.to_string_lossy(),
                    self.role_virtual_root(role).to_string_lossy(),
                    digest,
                );
            }
        } else {
            spec = spec.with_read_only_mount_identity(
                self.root.to_string_lossy(),
                self.virtual_root().to_string_lossy(),
                self.bundle_sha256.clone(),
            );
        }
        spec
    }

    fn is_nix_layout(&self) -> bool {
        self.layout == NIX_LAYOUT
    }

    fn role_virtual_root(&self, role: &str) -> PathBuf {
        self.virtual_root().join(role)
    }

    fn compiler_virtual_path(&self) -> String {
        if self.is_nix_layout() {
            self.role_virtual_path("compiler", &self.compiler)
        } else {
            self.virtual_path(&self.compiler)
        }
    }

    fn cxx_virtual_path(&self) -> String {
        if self.is_nix_layout() {
            self.role_virtual_path("compiler", &self.cxx)
        } else {
            self.virtual_path(&self.cxx)
        }
    }

    fn linker_virtual_path(&self) -> String {
        if self.is_nix_layout() {
            self.role_virtual_path("linker", &self.linker)
        } else {
            self.virtual_path(&self.linker)
        }
    }

    fn role_virtual_path(&self, role: &str, relative: &str) -> String {
        self.role_virtual_root(role)
            .join(relative)
            .to_string_lossy()
            .into_owned()
    }

    fn virtual_path(&self, relative: &str) -> String {
        self.virtual_root()
            .join(relative)
            .to_string_lossy()
            .into_owned()
    }

    fn linker_root_digest(&self) -> String {
        self.mounts
            .iter()
            .find(|mount| mount.source == self.linker_root)
            .map(|mount| mount.digest.clone())
            .unwrap_or_else(|| self.bundle_sha256.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BundleManifest {
    host: String,
    target: String,
    version: String,
    abi: String,
    bundle: String,
    compiler: String,
    compiler_sha256: String,
    cxx: String,
    cxx_sha256: String,
    linker: String,
    linker_sha256: String,
    sysroot: String,
    sysroot_sha256: String,
    bundle_sha256: String,
    source: String,
}

impl BundleManifest {
    fn descriptor(&self, root: PathBuf, entry_id: String) -> CcToolchainDescriptor {
        let producer = ProducerRecord::new(
            PRODUCER,
            self.source.clone(),
            self.bundle_sha256.clone(),
            crate::Comptime::Build::BuildPlanReplay::from_facts(BTreeMap::from([
                ("action.kind".into(), "cc-toolchain-provision".into()),
                ("action.recipe".into(), RECIPE_ID.into()),
            ]))
            .expect("static C toolchain plan facts are valid"),
            RECIPE_ID,
            "policy=content-addressed\nplatform=host",
            manifest_facts(self),
        )
        .expect("validated C toolchain manifest facts are valid");
        CcToolchainDescriptor {
            entry_id,
            root: root.clone(),
            host: self.host.clone(),
            target: self.target.clone(),
            version: self.version.clone(),
            abi: self.abi.clone(),
            compiler: self.compiler.clone(),
            compiler_sha256: self.compiler_sha256.clone(),
            cxx: self.cxx.clone(),
            cxx_sha256: self.cxx_sha256.clone(),
            linker: self.linker.clone(),
            linker_sha256: self.linker_sha256.clone(),
            sysroot: self.sysroot.clone(),
            sysroot_sha256: self.sysroot_sha256.clone(),
            bundle_sha256: self.bundle_sha256.clone(),
            layout: FIXTURE_LAYOUT.into(),
            compiler_root: root.clone(),
            cxx_root: root.clone(),
            linker_root: root.clone(),
            sysroot_root: root.clone(),
            mounts: vec![CcMount {
                store_path: String::new(),
                digest: self.bundle_sha256.clone(),
                source: root.clone(),
                destination: PathBuf::from("/jet/toolchains").join(&self.bundle_sha256),
            }],
            references: Vec::new(),
            envelope: Envelope::Envelope::default(),
            producer,
        }
    }
}

/// Whether a package ref names the one C/C++ toolchain package. It is kept in
/// the existing native provider rather than adding a second ProviderKind.
pub(crate) fn is_reference(spec: &PackageRef) -> bool {
    let package = spec.package.split_once('#').map_or(spec.package.as_str(), |(name, _)| name);
    package == PACKAGE
        && (matches!(&spec.source, Source::Jetpack)
            || matches!(&spec.source, Source::Named(name) if name == SOURCE))
}

pub(crate) fn requested_target(spec: &PackageRef) -> Result<String, String> {
    let (_, selector) = spec
        .package
        .split_once('#')
        .map_or((spec.package.as_str(), None), |(name, selector)| {
            (name, Some(selector))
        });
    let target = match selector {
        None | Some("") => host_target_triple(),
        Some(value) => value
            .strip_prefix("target=")
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                "C toolchain selectors must use `#target=<target-triple>`".to_string()
            })?,
    };
    normalize_target(&target)
}

pub(crate) fn host_target_triple() -> String {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "linux" => format!("{arch}-unknown-linux-gnu"),
        "macos" => format!("{arch}-apple-darwin"),
        "windows" => format!("{arch}-pc-windows-msvc"),
        os => format!("{arch}-{os}"),
    }
}

fn normalize_target(target: &str) -> Result<String, String> {
    let target = if matches!(target, "native" | "host") {
        host_target_triple()
    } else {
        target.to_string()
    };
    if target.is_empty()
        || target.len() > 128
        || !target.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(format!("invalid C toolchain target `{target}`"));
    }
    Ok(target)
}

pub(crate) fn cache_expectation(
    spec: &PackageRef,
    ctx: &Ctx<'_>,
) -> Option<CacheExpectation> {
    let target = requested_target(spec).ok()?;
    let descriptor = resolve_for_target(ctx.store_dir, &target, ctx.fixtures.is_some()).ok()?;
    Some(CacheExpectation {
        identity: descriptor.cache_identity(ctx),
        owned_output: Some(descriptor.root),
        allow_unsigned_local: true,
    })
}

pub(crate) fn plan_downloads(
    specs: &[PackageRef],
    ctx: &Ctx<'_>,
) -> Result<DownloadPlan, ProviderError> {
    let mut plan = DownloadPlan::default();
    for spec in specs {
        let target = requested_target(spec).map_err(ProviderError::Unsupported)?;
        match resolve_for_target(ctx.store_dir, &target, ctx.fixtures.is_some()) {
            Ok(_) => {
                plan.add_item(PlanItem {
                    package: spec.raw.clone(),
                    state: PlanState::Cached,
                    download_bytes: Some(0),
                    disk_bytes: None,
                });
                continue;
            }
            Err(error) if !is_missing_descriptor(&error, &target) => {
                return Err(ProviderError::BadOutput(error));
            }
            Err(_) => {}
        }
        if let Some(fixtures) = ctx.fixtures {
            if fixture_manifest(fixtures, &target)
                .map_err(ProviderError::Unsupported)?
                .is_some()
            {
                plan.add_item(PlanItem {
                    package: spec.raw.clone(),
                    state: PlanState::New,
                    download_bytes: None,
                    disk_bytes: None,
                });
                continue;
            }
        }
        let record = acquisition_record(&target).map_err(ProviderError::Unsupported)?;
        let index = ctx.nix_index.ok_or_else(|| {
            if ctx.offline {
                ProviderError::Offline(format!(
                    "the signed C/C++ toolchain index for `{}` is not available offline",
                    record.id
                ))
            } else {
                ProviderError::Unsupported(
                    "the signed C/C++ toolchain index is not configured; Jet will not use a host or PATH compiler"
                        .into(),
                )
            }
        })?;
        let (compiler, linker) = resolve_nix_records(index, record)?;
        let roots = ctx.nix_roots.ok_or_else(|| {
            ProviderError::BadOutput(
                "C toolchain download planning has no Hangar roots for closure admission".into(),
            )
        })?;
        let requests = vec![
            NixOutputRequest {
                name: "compiler".into(),
                store_path: signed_output_path(&compiler, "compiler")
                    .map_err(ProviderError::BadOutput)?,
            },
            NixOutputRequest {
                name: "linker".into(),
                store_path: signed_output_path(&linker, "linker")
                    .map_err(ProviderError::BadOutput)?,
            },
        ];
        let nix = plan_nix_downloads(
            roots,
            &requests
                .iter()
                .map(|request| request.store_path.clone())
                .collect::<Vec<_>>(),
            ctx.offline,
            crate::Store::current_progress(),
        )
        .map_err(|error| ProviderError::NixCache(error.to_string()))?;
        plan.add_nix(nix);
        plan.add_item(PlanItem {
            package: spec.raw.clone(),
            state: PlanState::New,
            download_bytes: None,
            disk_bytes: None,
        });
    }
    Ok(plan)
}

pub(crate) fn realize(
    spec: &PackageRef,
    ctx: &Ctx<'_>,
) -> Result<Realized, ProviderError> {
    let target = requested_target(spec).map_err(ProviderError::Unsupported)?;
    match resolve_for_target(ctx.store_dir, &target, ctx.fixtures.is_some()) {
        Ok(descriptor) => return Ok(realized_from_descriptor(spec, ctx, descriptor)),
        Err(error) if is_missing_descriptor(&error, &target) => {}
        Err(error) => return Err(ProviderError::BadOutput(error)),
    }
    if let Some(fixtures) = ctx.fixtures {
        if let Some(manifest) = fixture_manifest(fixtures, &target)
            .map_err(ProviderError::Unsupported)?
        {
            return provision_fixture(spec, ctx, &manifest);
        }
    }
    realize_nix(spec, ctx, &target)
}

fn acquisition_record(target: &str) -> Result<&'static NixAcquisitionRecord, String> {
    let target = normalize_target(target)?;
    NIX_ACQUISITION_RECORDS
        .iter()
        .find(|record| record.target == target)
        .ok_or_else(|| {
            format!(
                "no signed C/C++ toolchain acquisition record supports target `{target}`; supported targets: {}",
                NIX_ACQUISITION_RECORDS
                    .iter()
                    .map(|record| record.target)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn index_key(record: &NixAcquisitionRecord, attrpath: &[&str]) -> IndexKey {
    IndexKey {
        channel: record.channel.into(),
        revision: record.revision.into(),
        system: record.system.into(),
        attrpath: attrpath.iter().map(|part| (*part).into()).collect(),
    }
}

fn resolve_nix_records(
    index: &NixIndexClient<'_>,
    record: &NixAcquisitionRecord,
) -> Result<(VerifiedIndexRecord, VerifiedIndexRecord), ProviderError> {
    if host_nix_system() != Some(record.system) {
        return Err(ProviderError::Unsupported(format!(
            "signed C/C++ toolchain record `{}` is for `{}`, but this machine is `{}`",
            record.id,
            record.system,
            host_nix_system().unwrap_or("an unsupported host")
        )));
    }
    let compiler = resolve_nix_record(index, record, record.compiler_attr, "compiler")?;
    let linker = resolve_nix_record(index, record, record.linker_attr, "linker")?;
    Ok((compiler, linker))
}

fn resolve_nix_record(
    index: &NixIndexClient<'_>,
    record: &NixAcquisitionRecord,
    attrpath: &[&str],
    role: &str,
) -> Result<VerifiedIndexRecord, ProviderError> {
    let key = index_key(record, attrpath);
    let verified = index.resolve(&key).map_err(ProviderError::NixIndex)?;
    if verified.trust != IndexTrustTier::OfficialSigned {
        return Err(ProviderError::Unsupported(format!(
            "C/C++ {role} record `{}` was not resolved from an official signed Nix index",
            record.id
        )));
    }
    if verified.record.attrpath != key.attrpath {
        return Err(ProviderError::BadOutput(format!(
            "signed C/C++ {role} record attrpath disagrees with `{}`",
            key.attrpath.join(".")
        )));
    }
    Ok(verified)
}

fn signed_output_path(
    record: &VerifiedIndexRecord,
    role: &str,
) -> Result<String, String> {
    let path = record
        .record
        .outputs
        .get("out")
        .cloned()
        .ok_or_else(|| format!("signed C/C++ {role} record has no `out` output"))?;
    validate_nix_store_path(&path)?;
    Ok(path)
}

fn realize_nix(
    spec: &PackageRef,
    ctx: &Ctx<'_>,
    target: &str,
) -> Result<Realized, ProviderError> {
    let record = acquisition_record(target).map_err(ProviderError::Unsupported)?;
    let index = ctx.nix_index.ok_or_else(|| {
        if ctx.offline {
            ProviderError::Offline(format!(
                "signed C/C++ toolchain index `{}` is not cached for offline use",
                record.id
            ))
        } else {
            ProviderError::Unsupported(
                "the signed C/C++ toolchain index is not configured; Jet will not use a host or PATH compiler"
                    .into(),
            )
        }
    })?;
    let (compiler_record, linker_record) = resolve_nix_records(index, record)?;
    let roots = ctx.nix_roots.ok_or_else(|| {
        ProviderError::BadOutput(
            "signed C/C++ toolchain realization has no Hangar roots for closure admission".into(),
        )
    })?;
    let compiler_path = signed_output_path(&compiler_record, "compiler")
        .map_err(ProviderError::BadOutput)?;
    let linker_path = signed_output_path(&linker_record, "linker")
        .map_err(ProviderError::BadOutput)?;
    let admitted = admit_nix_closure_with_progress(
        roots,
        &[
            NixOutputRequest {
                name: "compiler".into(),
                store_path: compiler_path,
            },
            NixOutputRequest {
                name: "linker".into(),
                store_path: linker_path,
            },
        ],
        ctx.offline,
        crate::Store::current_progress(),
    )
    .map_err(|error| ProviderError::NixCache(error.to_string()))?;
    let descriptor = nix_descriptor(
        record,
        &compiler_record,
        &linker_record,
        admitted,
        roots,
    )?;
    Ok(realized_from_descriptor_state(
        spec,
        ctx,
        descriptor,
        SourceState::Substituted,
    ))
}

fn nix_descriptor(
    record: &NixAcquisitionRecord,
    compiler_record: &VerifiedIndexRecord,
    linker_record: &VerifiedIndexRecord,
    admitted: AdmittedNixClosure,
    roots: &Roots,
) -> Result<CcToolchainDescriptor, ProviderError> {
    if admitted.objects.is_empty() {
        return Err(ProviderError::BadOutput(
            "signed C/C++ toolchain admission returned an empty closure".into(),
        ));
    }
    if admitted.objects.len() > MAX_NIX_OBJECTS {
        return Err(ProviderError::BadOutput(format!(
            "signed C/C++ toolchain closure exceeds {MAX_NIX_OBJECTS} objects"
        )));
    }
    let compiler_path = signed_output_path(compiler_record, "compiler")
        .map_err(ProviderError::BadOutput)?;
    let linker_path = signed_output_path(linker_record, "linker")
        .map_err(ProviderError::BadOutput)?;
    let compiler = admitted.outputs.get("compiler").ok_or_else(|| {
        ProviderError::BadOutput("C/C++ admission returned no compiler output".into())
    })?;
    let linker = admitted.outputs.get("linker").ok_or_else(|| {
        ProviderError::BadOutput("C/C++ admission returned no linker output".into())
    })?;
    if compiler.store_path != compiler_path || linker.store_path != linker_path {
        return Err(ProviderError::BadOutput(
            "C/C++ admission changed a signed toolchain output path".into(),
        ));
    }
    let compiler_root = checked_admitted_object(roots, compiler, "compiler")?;
    let linker_root = checked_admitted_object(roots, linker, "linker")?;
    let marker = checked_relative_path(
        &compiler_root,
        record.sysroot_marker,
        "sysroot marker",
    )
    .map_err(ProviderError::BadOutput)?;
    let sysroot_store_path = read_store_path_marker(&marker)?;
    let sysroot = admitted.objects.get(&sysroot_store_path).ok_or_else(|| {
        ProviderError::BadOutput(format!(
            "C/C++ compiler sysroot marker names `{sysroot_store_path}`, which is not in the admitted closure"
        ))
    })?;
    let sysroot_root = checked_admitted_object(roots, sysroot, "sysroot")?;
    let mounts = admitted_mounts(roots, &admitted)?;
    let mut references = admitted
        .objects
        .values()
        .filter(|object| object.hangar_digest != compiler.hangar_digest)
        .map(|object| object.hangar_digest.clone())
        .collect::<BTreeSet<_>>();
    references.remove(&compiler.hangar_digest);
    let references = references.into_iter().collect::<Vec<_>>();
    let compiler_sha256 = SHA256::sha256_file_hex(&compiler_root.join(record.compiler_path))
        .map_err(|error| ProviderError::BadOutput(format!("hash C compiler: {error}")))?;
    let cxx_sha256 = SHA256::sha256_file_hex(&compiler_root.join(record.cxx_path))
        .map_err(|error| ProviderError::BadOutput(format!("hash C++ compiler: {error}")))?;
    let linker_sha256 = SHA256::sha256_file_hex(&linker_root.join(record.linker_path))
        .map_err(|error| ProviderError::BadOutput(format!("hash C linker: {error}")))?;
    let sysroot_sha256 = sysroot.hangar_digest.clone();
    let version = format!(
        "gcc-{}+lld-{}",
        compiler_record.record.version, linker_record.record.version
    );
    let source = format!(
        "nixpkgs:{}#{}:{}",
        record.channel, record.revision, record.system
    );
    let facts = nix_facts(
        record,
        compiler_record,
        linker_record,
        compiler,
        linker,
        sysroot,
        &sysroot_store_path,
        &compiler_sha256,
        &cxx_sha256,
        &linker_sha256,
        &mounts,
        &references,
        &admitted.closure_receipt_sha256,
    );
    let plan = crate::Comptime::Build::BuildPlanReplay::from_facts(facts.clone())
        .map_err(ProviderError::BadOutput)?;
    let producer = ProducerRecord::new(
        NIX_PRODUCER,
        source,
        compiler.hangar_digest.clone(),
        plan,
        "jet-cc:nix-index-hangar-v1",
        "policy=official-signed-index+signed-nix-cache\nplatform=host",
        facts,
    )
    .map_err(ProviderError::BadOutput)?;
    let descriptor = CcToolchainDescriptor {
        entry_id: format!("{PACKAGE}-{}-{}", record.id, compiler.hangar_digest),
        root: compiler_root.clone(),
        host: host_target_triple(),
        target: record.target.into(),
        version,
        abi: record.abi.into(),
        compiler: record.compiler_path.into(),
        compiler_sha256,
        cxx: record.cxx_path.into(),
        cxx_sha256,
        linker: record.linker_path.into(),
        linker_sha256,
        sysroot: "sysroot".into(),
        sysroot_sha256,
        bundle_sha256: compiler.hangar_digest.clone(),
        layout: NIX_LAYOUT.into(),
        compiler_root,
        cxx_root: checked_admitted_object(roots, compiler, "C++ compiler")?,
        linker_root,
        sysroot_root,
        mounts,
        references,
        envelope: Envelope::Envelope {
            output_hash: compiler.hangar_digest.clone(),
            platform: Envelope::host_platform(),
            signature: String::new(),
            provenance: format!("{} via signed Nix index and cache", record.id),
        },
        producer,
    };
    Ok(descriptor)
}

fn admitted_mounts(
    roots: &Roots,
    admitted: &AdmittedNixClosure,
) -> Result<Vec<CcMount>, ProviderError> {
    let mut mounts = Vec::with_capacity(admitted.objects.len());
    for (store_path, object) in &admitted.objects {
        if store_path != &object.store_path {
            return Err(ProviderError::BadOutput(format!(
                "C/C++ closure key `{store_path}` disagrees with `{}`",
                object.store_path
            )));
        }
        validate_nix_store_path(store_path).map_err(ProviderError::BadOutput)?;
        validate_tree_digest(&object.hangar_digest, "C/C++ closure object digest")
            .map_err(ProviderError::BadOutput)?;
        let source = checked_admitted_object(roots, object, "closure object")?;
        mounts.push(CcMount {
            store_path: store_path.clone(),
            digest: object.hangar_digest.clone(),
            source,
            destination: PathBuf::from(store_path),
        });
    }
    Ok(mounts)
}

fn checked_admitted_object(
    roots: &Roots,
    object: &crate::Store::NixCache::AdmittedNixObject,
    label: &str,
) -> Result<PathBuf, ProviderError> {
    validate_tree_digest(&object.hangar_digest, &format!("{label} digest"))
        .map_err(ProviderError::BadOutput)?;
    let hangar = fs::canonicalize(roots.hangar_dir())
        .map_err(|error| ProviderError::BadOutput(format!("canonicalize Hangar: {error}")))?;
    let objects = hangar.join("objects");
    let source = fs::canonicalize(&object.hangar_path).map_err(|error| {
        ProviderError::BadOutput(format!("C/C++ {label} Hangar object is unavailable: {error}"))
    })?;
    if !source.starts_with(&objects)
        || source.file_name().and_then(|name| name.to_str())
            != Some(object.hangar_digest.as_str())
    {
        return Err(ProviderError::BadOutput(format!(
            "C/C++ {label} Hangar object is outside the content-addressed object pool"
        )));
    }
    Ok(source)
}

fn read_store_path_marker(path: &Path) -> Result<String, ProviderError> {
    let bytes = read_bounded_file_named(path, MAX_MARKER_BYTES, "C/C++ sysroot marker")
        .map_err(ProviderError::BadOutput)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| ProviderError::BadOutput("C/C++ sysroot marker is not UTF-8".into()))?
        .trim();
    validate_nix_store_path(text).map_err(ProviderError::BadOutput)?;
    Ok(text.into())
}

fn nix_facts(
    record: &NixAcquisitionRecord,
    compiler_record: &VerifiedIndexRecord,
    linker_record: &VerifiedIndexRecord,
    compiler: &crate::Store::NixCache::AdmittedNixObject,
    linker: &crate::Store::NixCache::AdmittedNixObject,
    sysroot: &crate::Store::NixCache::AdmittedNixObject,
    sysroot_store_path: &str,
    compiler_sha256: &str,
    cxx_sha256: &str,
    linker_sha256: &str,
    mounts: &[CcMount],
    references: &[String],
    closure_receipt: &str,
) -> BTreeMap<String, String> {
    let mut facts = BTreeMap::from([
        ("cc.schema".into(), SCHEMA.into()),
        ("cc.producer".into(), PRODUCER.into()),
        ("cc.layout".into(), NIX_LAYOUT.into()),
        ("cc.host".into(), host_target_triple()),
        ("cc.target".into(), record.target.into()),
        (
            "cc.version".into(),
            format!("gcc-{}+lld-{}", compiler_record.record.version, linker_record.record.version),
        ),
        ("cc.abi".into(), record.abi.into()),
        ("cc.compiler".into(), record.compiler_path.into()),
        ("cc.compiler.sha256".into(), compiler_sha256.into()),
        ("cc.cxx".into(), record.cxx_path.into()),
        ("cc.cxx.sha256".into(), cxx_sha256.into()),
        ("cc.linker".into(), record.linker_path.into()),
        ("cc.linker.sha256".into(), linker_sha256.into()),
        ("cc.sysroot".into(), "sysroot".into()),
        ("cc.sysroot.sha256".into(), sysroot.hangar_digest.clone()),
        ("cc.bundle.sha256".into(), compiler.hangar_digest.clone()),
        ("cc.compiler.root.sha256".into(), compiler.hangar_digest.clone()),
        ("cc.cxx.root.sha256".into(), compiler.hangar_digest.clone()),
        ("cc.linker.root.sha256".into(), linker.hangar_digest.clone()),
        ("cc.sysroot.root.sha256".into(), sysroot.hangar_digest.clone()),
        ("cc.mount.count".into(), mounts.len().to_string()),
        ("cc.nix.record.id".into(), record.id.into()),
        ("cc.nix.channel".into(), record.channel.into()),
        ("cc.nix.revision".into(), record.revision.into()),
        ("cc.nix.system".into(), record.system.into()),
        ("cc.nix.compiler.attrpath".into(), record.compiler_attr.join(".")),
        ("cc.nix.linker.attrpath".into(), record.linker_attr.join(".")),
        ("cc.nix.sysroot.marker".into(), record.sysroot_marker.into()),
        (
            "cc.nix.compiler.record".into(),
            compiler_record.record.canonical_json(),
        ),
        (
            "cc.nix.linker.record".into(),
            linker_record.record.canonical_json(),
        ),
        (
            "cc.nix.compiler.index.proof".into(),
            compiler_record.proof.canonical_json(),
        ),
        (
            "cc.nix.linker.index.proof".into(),
            linker_record.proof.canonical_json(),
        ),
        (
            "cc.nix.compiler.store_path".into(),
            compiler.store_path.clone(),
        ),
        ("cc.nix.linker.store_path".into(), linker.store_path.clone()),
        ("cc.nix.sysroot.store_path".into(), sysroot_store_path.into()),
        (
            "cc.nix.compiler.cache.proof".into(),
            compiler.upstream_proof_sha256.clone(),
        ),
        (
            "cc.nix.linker.cache.proof".into(),
            linker.upstream_proof_sha256.clone(),
        ),
        ("cc.nix.closure.receipt".into(), closure_receipt.into()),
        ("cc.manifest".into(), record.id.into()),
        ("cc.references".into(), references.join(",")),
    ]);
    for (index, mount) in mounts.iter().enumerate() {
        facts.insert(format!("cc.mount.{index}.store_path"), mount.store_path.clone());
        facts.insert(format!("cc.mount.{index}.digest"), mount.digest.clone());
    }
    facts.extend(super::nix_build_facts_record());
    facts
}

fn realized_from_descriptor(
    spec: &PackageRef,
    ctx: &Ctx<'_>,
    descriptor: CcToolchainDescriptor,
) -> Realized {
    realized_from_descriptor_state(spec, ctx, descriptor, SourceState::Cached)
}

fn realized_from_descriptor_state(
    spec: &PackageRef,
    ctx: &Ctx<'_>,
    descriptor: CcToolchainDescriptor,
    source_state: SourceState,
) -> Realized {
    let identity = descriptor.cache_identity(ctx);
    let out = descriptor.root.to_string_lossy().into_owned();
    Realized {
        name: PACKAGE.into(),
        version: descriptor.version.clone(),
        reference: spec.raw.clone(),
        out: out.clone(),
        bin: descriptor.root.join("bin").to_string_lossy().into_owned(),
        rlib: String::new(),
        envelope: descriptor.envelope.clone(),
        cache_identity: identity,
        source_state,
        named_outputs: BTreeMap::from([("out".into(), out)]),
        references: descriptor.references.clone(),
        producer: descriptor.producer,
    }
}

fn provision_fixture(
    spec: &PackageRef,
    ctx: &Ctx<'_>,
    manifest: &BundleManifest,
) -> Result<Realized, ProviderError> {
    if manifest.host != host_target_triple() {
        return Err(ProviderError::Unsupported(format!(
            "C toolchain bundle host `{}` does not match this machine `{}`",
            manifest.host,
            host_target_triple()
        )));
    }
    let bundle_root =
        manifest_bundle_root(ctx.fixtures, manifest).map_err(ProviderError::BadOutput)?;
    ensure_real_directory(ctx.store_dir).map_err(ProviderError::BadOutput)?;
    let suffix = manifest
        .bundle_sha256
        .strip_prefix("sha256-")
        .unwrap_or(&manifest.bundle_sha256);
    let suffix = &suffix[..suffix.len().min(12)];
    let staging = ctx
        .store_dir
        .join(format!(".{PACKAGE}-{}-{suffix}.partial", manifest.version));
    let output = ctx
        .store_dir
        .join(format!("{PACKAGE}-{}-{suffix}", manifest.version));
    if fs::symlink_metadata(&staging).is_ok() || fs::symlink_metadata(&output).is_ok() {
        return Err(ProviderError::BadOutput(format!(
            "C toolchain Hangar publication path already exists: {}",
            output.display()
        )));
    }
    let result = (|| {
        copy_tree(&bundle_root, &staging)?;
        crate::Store::seal_local_output(&staging)
            .map_err(|error| format!("could not seal C toolchain bundle: {error}"))?;
        verify_manifest_payload(&staging, manifest)?;
        let actual_bundle = Envelope::try_output_hash_of(&staging.to_string_lossy())
            .map_err(|error| format!("could not hash C toolchain bundle: {error}"))?;
        if actual_bundle != manifest.bundle_sha256 {
            return Err(format!(
                "C toolchain bundle digest mismatch: expected {}, got {actual_bundle}",
                manifest.bundle_sha256
            ));
        }
        fs::rename(&staging, &output)
            .map_err(|error| format!("could not publish C toolchain bundle: {error}"))?;
        let mut descriptor = manifest.descriptor(output.clone(), output.to_string_lossy().into_owned());
        descriptor.root = output.clone();
        descriptor.envelope = Envelope::Envelope::for_output(
            &output.to_string_lossy(),
            &spec.raw,
            RECIPE_ID,
        );
        let identity = descriptor.cache_identity(ctx);
        let out = output.to_string_lossy().into_owned();
        let mut producer = descriptor.producer;
        producer
            .facts
            .insert("cache.output".into(), descriptor.bundle_sha256.clone());
        Ok(Realized {
            name: PACKAGE.into(),
            version: descriptor.version,
            reference: spec.raw.clone(),
            out: out.clone(),
            bin: output.join("bin").to_string_lossy().into_owned(),
            rlib: String::new(),
            envelope: descriptor.envelope,
            cache_identity: identity,
            source_state: SourceState::Downloaded,
            named_outputs: BTreeMap::from([("out".into(), out)]),
            references: Vec::new(),
            producer,
        })
    })();
    if result.is_err() {
        remove_tree(&staging);
    }
    result.map_err(ProviderError::BadOutput)
}

/// Resolve and verify a descriptor from a committed, receipt-checked Hangar
/// projection. Store registration and closure admission remain the single
/// write/recording boundary.
pub(crate) fn resolve_for_target(
    hangar_root: &Path,
    target: &str,
    allow_fixture: bool,
) -> Result<CcToolchainDescriptor, String> {
    let target = normalize_target(target)?;
    let roots = roots_for_hangar(hangar_root)?;
    let entries = crate::Store::list_checked(&roots)
        .map_err(|error| format!("could not inspect Hangar C toolchain records: {error}"))?;
    let mut corrupt = None;
    for entry in entries {
        if entry.name != PACKAGE {
            continue;
        }
        let producer = match ProducerRecord::decode(&entry.producer_record) {
            Ok(producer) => producer,
            Err(error) => {
                corrupt = Some(format!("Hangar C toolchain `{}` has an invalid producer record: {error}", entry.id));
                continue;
            }
        };
        let layout = producer.facts.get("cc.layout").map(String::as_str);
        let is_cc_record = match (producer.provider.as_str(), layout) {
            (PRODUCER, Some(FIXTURE_LAYOUT)) => true,
            (NIX_PRODUCER, Some(NIX_LAYOUT))
                if producer.facts.get("cc.producer").map(String::as_str)
                    == Some(PRODUCER) => true,
            _ => false,
        };
        if !is_cc_record {
            corrupt = Some(format!(
                "Hangar entry `{}` is named `{PACKAGE}` but was not produced by the C/C++ provider",
                entry.id
            ));
            continue;
        }
        if !allow_fixture && layout == Some(FIXTURE_LAYOUT) {
            continue;
        }
        if producer.facts.get("cc.target") != Some(&target) {
            continue;
        }
        if layout == Some(NIX_LAYOUT) {
            let record = match acquisition_record(&target) {
                Ok(record) => record,
                Err(error) => {
                    corrupt = Some(error);
                    continue;
                }
            };
            if let Err(error) = validate_pinned_nix_descriptor(&producer, record) {
                corrupt = Some(format!(
                    "Hangar C toolchain `{}` is not the pinned acquisition: {error}",
                    entry.id
                ));
                continue;
            }
        }
        match descriptor_from_entry(&roots, &entry, producer) {
            Ok(descriptor) => return Ok(descriptor),
            Err(error) => corrupt = Some(error),
        }
    }
    if let Some(error) = corrupt {
        return Err(error);
    }
    Err(format!(
        "Hangar has no verified C/C++ toolchain descriptor for target `{target}`"
    ))
}

fn is_missing_descriptor(error: &str, target: &str) -> bool {
    error == format!("Hangar has no verified C/C++ toolchain descriptor for target `{target}`")
}

fn validate_pinned_nix_descriptor(
    producer: &ProducerRecord,
    record: &NixAcquisitionRecord,
) -> Result<(), String> {
    let expected_source = format!(
        "nixpkgs:{}#{}:{}",
        record.channel, record.revision, record.system
    );
    if producer.immutable_source != expected_source {
        return Err(format!(
            "immutable source `{}` does not match pinned C/C++ record `{}`",
            producer.immutable_source, record.id
        ));
    }
    let expected = [
        ("cc.nix.record.id", record.id.to_string()),
        ("cc.nix.channel", record.channel.to_string()),
        ("cc.nix.revision", record.revision.to_string()),
        ("cc.nix.system", record.system.to_string()),
        ("cc.nix.compiler.attrpath", record.compiler_attr.join(".")),
        ("cc.nix.linker.attrpath", record.linker_attr.join(".")),
        ("cc.nix.sysroot.marker", record.sysroot_marker.to_string()),
        ("cc.manifest", record.id.to_string()),
        ("cc.compiler", record.compiler_path.to_string()),
        ("cc.cxx", record.cxx_path.to_string()),
        ("cc.linker", record.linker_path.to_string()),
        ("cc.abi", record.abi.to_string()),
    ];
    for (key, expected) in expected {
        if producer.facts.get(key).map(String::as_str) != Some(expected.as_str()) {
            return Err(format!(
                "fact `{key}` does not match pinned C/C++ record `{}`",
                record.id
            ));
        }
    }
    Ok(())
}

fn descriptor_from_entry(
    roots: &Roots,
    entry: &StoreEntry,
    producer: ProducerRecord,
) -> Result<CcToolchainDescriptor, String> {
    let fact = |key: &str| {
        producer
            .facts
            .get(key)
            .cloned()
            .ok_or_else(|| format!("Hangar C toolchain `{}` is missing fact `{key}`", entry.id))
    };
    if fact("cc.schema")? != SCHEMA {
        return Err(format!(
            "Hangar C toolchain `{}` has an unsupported descriptor schema",
            entry.id
        ));
    }
    let root = checked_entry_root(&roots.hangar_dir(), entry)?;
    let layout = fact("cc.layout")?;
    let bundle_sha256 = fact("cc.bundle.sha256")?;
    let (compiler_root, cxx_root, linker_root, sysroot_root, mounts) = match layout.as_str() {
        FIXTURE_LAYOUT => {
            let mount = CcMount {
                store_path: String::new(),
                digest: bundle_sha256.clone(),
                source: root.clone(),
                destination: PathBuf::from("/jet/toolchains").join(&bundle_sha256),
            };
            (
                root.clone(),
                root.clone(),
                root.clone(),
                root.clone(),
                vec![mount],
            )
        }
        NIX_LAYOUT => {
            let compiler_root = checked_object_digest_path(
                &roots.hangar_dir(),
                &fact("cc.compiler.root.sha256")?,
                "compiler",
            )?;
            let cxx_root = checked_object_digest_path(
                &roots.hangar_dir(),
                &fact("cc.cxx.root.sha256")?,
                "C++ compiler",
            )?;
            let linker_root = checked_object_digest_path(
                &roots.hangar_dir(),
                &fact("cc.linker.root.sha256")?,
                "linker",
            )?;
            let sysroot_root = checked_object_digest_path(
                &roots.hangar_dir(),
                &fact("cc.sysroot.root.sha256")?,
                "sysroot",
            )?;
            let mounts = entry_mounts(&roots.hangar_dir(), &fact)?;
            (compiler_root, cxx_root, linker_root, sysroot_root, mounts)
        }
        _ => {
            return Err(format!(
                "Hangar C toolchain `{}` has unsupported layout `{layout}`",
                entry.id
            ))
        }
    };
    let mut descriptor = CcToolchainDescriptor {
        entry_id: entry.id.clone(),
        root: root.clone(),
        host: fact("cc.host")?,
        target: fact("cc.target")?,
        version: fact("cc.version")?,
        abi: fact("cc.abi")?,
        compiler: fact("cc.compiler")?,
        compiler_sha256: fact("cc.compiler.sha256")?,
        cxx: fact("cc.cxx")?,
        cxx_sha256: fact("cc.cxx.sha256")?,
        linker: fact("cc.linker")?,
        linker_sha256: fact("cc.linker.sha256")?,
        sysroot: fact("cc.sysroot")?,
        sysroot_sha256: fact("cc.sysroot.sha256")?,
        bundle_sha256,
        layout,
        compiler_root,
        cxx_root,
        linker_root,
        sysroot_root,
        mounts,
        references: entry.references.clone(),
        envelope: entry.envelope.clone(),
        producer,
    };
    validate_descriptor(&descriptor, entry, &roots.hangar_dir())?;
    let expected_source = source_fingerprint(&descriptor);
    if entry.cache_identity.source_fingerprint != expected_source {
        return Err(format!(
            "Hangar C toolchain `{}` has a cache source identity mismatch",
            entry.id
        ));
    }
    if entry.cache_identity.recipe_fingerprint != SHA256::sha256_hex(RECIPE_ID.as_bytes()) {
        return Err(format!(
            "Hangar C toolchain `{}` has a cache recipe identity mismatch",
            entry.id
        ));
    }
    if descriptor.producer.source_digest != descriptor.bundle_sha256 {
        return Err(format!(
            "Hangar C toolchain `{}` has a source digest mismatch",
            entry.id
        ));
    }
    descriptor.producer = ProducerRecord::decode(&entry.producer_record)
        .map_err(|error| format!("could not re-read C toolchain producer record: {error}"))?;
    Ok(descriptor)
}

fn entry_mounts<F>(
    hangar_root: &Path,
    fact: &F,
) -> Result<Vec<CcMount>, String>
where
    F: Fn(&str) -> Result<String, String>,
{
    let count = fact("cc.mount.count")?
        .parse::<usize>()
        .map_err(|_| "Hangar C toolchain has an invalid mount count".to_string())?;
    if count == 0 || count > MAX_NIX_OBJECTS {
        return Err(format!(
            "Hangar C toolchain has an invalid mount count `{count}`"
        ));
    }
    let mut stores = BTreeSet::new();
    let mut digests = BTreeSet::new();
    let mut mounts = Vec::with_capacity(count);
    for index in 0..count {
        let store_path = fact(&format!("cc.mount.{index}.store_path"))?;
        let digest = fact(&format!("cc.mount.{index}.digest"))?;
        validate_nix_store_path(&store_path)?;
        validate_tree_digest(&digest, "C/C++ closure object digest")?;
        if !stores.insert(store_path.clone()) || !digests.insert(digest.clone()) {
            return Err("Hangar C toolchain has duplicate closure mounts".to_string());
        }
        mounts.push(CcMount {
            source: checked_object_digest_path(hangar_root, &digest, "closure object")?,
            destination: PathBuf::from(&store_path),
            store_path,
            digest,
        });
    }
    Ok(mounts)
}

fn checked_object_digest_path(
    hangar_root: &Path,
    digest: &str,
    label: &str,
) -> Result<PathBuf, String> {
    validate_tree_digest(digest, &format!("{label} digest"))?;
    let hangar = fs::canonicalize(hangar_root)
        .map_err(|error| format!("canonicalize Hangar root: {error}"))?;
    let objects = hangar.join("objects");
    let path = objects.join(digest);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("Hangar C toolchain {label} is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
        return Err(format!(
            "Hangar C toolchain {label} is not a regular object"
        ));
    }
    let canonical = fs::canonicalize(&path)
        .map_err(|error| format!("canonicalize Hangar C toolchain {label}: {error}"))?;
    if !canonical.starts_with(&objects)
        || canonical.file_name().and_then(|name| name.to_str()) != Some(digest)
    {
        return Err(format!(
            "Hangar C toolchain {label} escapes the object pool"
        ));
    }
    Ok(canonical)
}

fn validate_descriptor(
    descriptor: &CcToolchainDescriptor,
    entry: &StoreEntry,
    hangar_root: &Path,
) -> Result<(), String> {
    if !matches!(descriptor.layout.as_str(), FIXTURE_LAYOUT | NIX_LAYOUT) {
        return Err(format!(
            "Hangar C toolchain `{}` has unsupported layout `{}`",
            entry.id, descriptor.layout
        ));
    }
    if descriptor.host != host_target_triple() {
        return Err(format!(
            "Hangar C toolchain `{}` targets host `{}`, but this machine is `{}`",
            entry.id,
            descriptor.host,
            host_target_triple()
        ));
    }
    normalize_target(&descriptor.target)?;
    for (name, value) in [
        ("version", descriptor.version.as_str()),
        ("ABI", descriptor.abi.as_str()),
    ] {
        validate_text_field(value, name).map_err(|error| {
            format!("Hangar C toolchain `{}` has an invalid {error}", entry.id)
        })?;
    }
    for (name, path) in [
        ("compiler", descriptor.compiler.as_str()),
        ("c++ compiler", descriptor.cxx.as_str()),
        ("linker", descriptor.linker.as_str()),
        ("sysroot", descriptor.sysroot.as_str()),
    ] {
        validate_relative_path(path)
            .map_err(|error| format!("Hangar C toolchain `{}` {name}: {error}", entry.id))?;
    }
    validate_sha256(&descriptor.compiler_sha256, "compiler digest")?;
    validate_sha256(&descriptor.cxx_sha256, "C++ compiler digest")?;
    validate_sha256(&descriptor.linker_sha256, "linker digest")?;
    validate_tree_digest(&descriptor.sysroot_sha256, "sysroot digest")?;
    validate_tree_digest(&descriptor.bundle_sha256, "bundle digest")?;
    if descriptor.producer.provider == NIX_PRODUCER {
        if descriptor.producer.facts.get("cc.producer").map(String::as_str)
            != Some(PRODUCER)
        {
            return Err(format!(
                "Hangar C toolchain `{}` has an invalid Nix producer marker",
                entry.id
            ));
        }
        super::validate_nix_build_facts(&descriptor.producer)
            .map_err(|error| format!("Hangar C toolchain `{}`: {error}", entry.id))?;
    }
    if entry.envelope.output_hash != descriptor.bundle_sha256
        || entry.envelope.platform != Envelope::host_platform()
        || entry.cache_identity.platform != Envelope::host_platform()
    {
        return Err(format!(
            "Hangar C toolchain `{}` has an envelope/platform identity mismatch",
            entry.id
        ));
    }
    let actual_bundle = Envelope::try_output_hash_of_in_hangar(
        &entry.out,
        hangar_root,
        false,
    )?;
    if actual_bundle != descriptor.bundle_sha256 {
        return Err(format!(
            "Hangar C toolchain `{}` bundle digest changed: expected {}, got {actual_bundle}",
            entry.id, descriptor.bundle_sha256
        ));
    }
    let root = Path::new(&descriptor.root);
    if descriptor.layout == FIXTURE_LAYOUT {
        if descriptor.compiler_root != root
            || descriptor.cxx_root != root
            || descriptor.linker_root != root
            || descriptor.sysroot_root != root
        {
            return Err(format!(
                "Hangar C toolchain `{}` has inconsistent fixture roots",
                entry.id
            ));
        }
        if descriptor.mounts.len() != 1
            || descriptor.mounts[0].source != root
            || descriptor.mounts[0].digest != descriptor.bundle_sha256
        {
            return Err(format!(
                "Hangar C toolchain `{}` has an invalid fixture mount",
                entry.id
            ));
        }
        verify_file(root, &descriptor.compiler, &descriptor.compiler_sha256, "compiler")?;
        verify_file(root, &descriptor.cxx, &descriptor.cxx_sha256, "C++ compiler")?;
        verify_file(root, &descriptor.linker, &descriptor.linker_sha256, "linker")?;
        verify_tree(root, &descriptor.sysroot, &descriptor.sysroot_sha256, "sysroot")?;
    } else {
        if descriptor.root != descriptor.compiler_root
            || descriptor.cxx_root != descriptor.compiler_root
            || descriptor.sysroot != "sysroot"
        {
            return Err(format!(
                "Hangar C toolchain `{}` has inconsistent Nix role roots",
                entry.id
            ));
        }
        let mut stores = BTreeSet::new();
        let mut digests = BTreeSet::new();
        for mount in &descriptor.mounts {
            validate_nix_store_path(&mount.store_path)?;
            validate_tree_digest(&mount.digest, "C/C++ closure object digest")?;
            if !stores.insert(mount.store_path.clone()) || !digests.insert(mount.digest.clone()) {
                return Err(format!(
                    "Hangar C toolchain `{}` has duplicate Nix mounts",
                    entry.id
                ));
            }
            let expected = checked_object_digest_path(hangar_root, &mount.digest, "closure object")?;
            if expected != mount.source || mount.destination != PathBuf::from(&mount.store_path) {
                return Err(format!(
                    "Hangar C toolchain `{}` has an invalid Nix mount",
                    entry.id
                ));
            }
        }
        let role_mount = |root: &Path| {
            descriptor
                .mounts
                .iter()
                .find(|mount| mount.source == root)
        };
        if role_mount(&descriptor.compiler_root)
            .is_none_or(|mount| mount.digest != descriptor.bundle_sha256)
            || role_mount(&descriptor.linker_root).is_none()
            || role_mount(&descriptor.sysroot_root).is_none()
        {
            return Err(format!(
                "Hangar C toolchain `{}` is missing a role object mount",
                entry.id
            ));
        }
        verify_file(
            &descriptor.compiler_root,
            &descriptor.compiler,
            &descriptor.compiler_sha256,
            "compiler",
        )?;
        verify_file(
            &descriptor.cxx_root,
            &descriptor.cxx,
            &descriptor.cxx_sha256,
            "C++ compiler",
        )?;
        verify_file(
            &descriptor.linker_root,
            &descriptor.linker,
            &descriptor.linker_sha256,
            "linker",
        )?;
        let sysroot_metadata = fs::symlink_metadata(&descriptor.sysroot_root)
            .map_err(|error| format!("C toolchain sysroot is unavailable: {error}"))?;
        if sysroot_metadata.file_type().is_symlink() || !sysroot_metadata.is_dir() {
            return Err("C toolchain sysroot is not a real directory".to_string());
        }
        let actual_sysroot = Envelope::try_output_hash_of_in_hangar(
            &descriptor.sysroot_root.to_string_lossy(),
            hangar_root,
            false,
        )?;
        if actual_sysroot != descriptor.sysroot_sha256 {
            return Err(format!(
                "C toolchain sysroot digest mismatch: expected {}, got {actual_sysroot}",
                descriptor.sysroot_sha256
            ));
        }
    }
    Ok(())
}

fn checked_entry_root(hangar_root: &Path, entry: &StoreEntry) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(&entry.out)
        .map_err(|error| format!("Hangar C toolchain `{}` output is unavailable: {error}", entry.id))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "Hangar C toolchain `{}` output is not a real directory",
            entry.id
        ));
    }
    let hangar = fs::canonicalize(hangar_root)
        .map_err(|error| format!("could not canonicalize Hangar root: {error}"))?;
    let root = fs::canonicalize(&entry.out)
        .map_err(|error| format!("could not canonicalize C toolchain output: {error}"))?;
    if !root.starts_with(&hangar) || root == hangar {
        return Err(format!(
            "Hangar C toolchain `{}` output escapes the Hangar root",
            entry.id
        ));
    }
    Ok(root)
}

fn verify_file(root: &Path, relative: &str, expected: &str, label: &str) -> Result<(), String> {
    let path = checked_relative_path(root, relative, label)?;
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("C toolchain {label} is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return Err(format!("C toolchain {label} is not a non-empty regular file"));
    }
    let actual = SHA256::sha256_file_hex(&path)
        .map_err(|error| format!("could not hash C toolchain {label}: {error}"))?;
    if actual != expected {
        return Err(format!(
            "C toolchain {label} digest mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}

fn verify_tree(root: &Path, relative: &str, expected: &str, label: &str) -> Result<(), String> {
    let path = checked_relative_path(root, relative, label)?;
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("C toolchain {label} is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("C toolchain {label} is not a real directory"));
    }
    let actual = Envelope::try_output_hash_of(&path.to_string_lossy())
        .map_err(|error| format!("could not hash C toolchain {label}: {error}"))?;
    if actual != expected {
        return Err(format!(
            "C toolchain {label} digest mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}

fn checked_relative_path(root: &Path, relative: &str, label: &str) -> Result<PathBuf, String> {
    validate_relative_path(relative)
        .map_err(|error| format!("C toolchain {label} path: {error}"))?;
    let mut current = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(part) = component else {
            return Err(format!("C toolchain {label} path is not normalized"));
        };
        current.push(part);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| format!("C toolchain {label} path is unavailable: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("C toolchain {label} path traverses a symlink"));
        }
    }
    Ok(current)
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    let value = Path::new(path);
    if path.is_empty()
        || value.is_absolute()
        || value.components().any(|component| {
            matches!(component, Component::CurDir | Component::ParentDir | Component::Prefix(_))
        })
        || path.contains('\0')
    {
        return Err(format!("`{path}` is not a normalized relative path"));
    }
    Ok(())
}

fn validate_nix_store_path(path: &str) -> Result<(), String> {
    let Some(name) = path.strip_prefix("/nix/store/") else {
        return Err(format!("`{path}` is not an absolute Nix store path"));
    };
    if name.is_empty()
        || name.len() > 256
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name == "."
        || name == ".."
    {
        return Err(format!("`{path}` is not a normalized Nix store path"));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
    {
        return Err(format!("`{path}` contains an invalid Nix store name"));
    }
    Ok(())
}

fn validate_text_field(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label}: value is empty"));
    }
    if value.bytes().any(|byte| {
        byte.is_ascii_control() || matches!(byte, b'/' | b'\\')
    }) {
        return Err(format!("{label}: value contains a control character or path separator"));
    }
    Ok(())
}

fn validate_sha256(value: &str, label: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!("{label} is not a lowercase SHA-256 digest"));
    }
    Ok(())
}

fn validate_tree_digest(value: &str, label: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256-") else {
        return Err(format!("{label} is not a Hangar SHA-256 digest"));
    };
    validate_sha256(hex, label)
}

fn roots_for_hangar(hangar: &Path) -> Result<Roots, String> {
    let root = hangar
        .parent()
        .ok_or_else(|| "Hangar root has no parent".to_string())?
        .to_path_buf();
    let dev_mode = hangar
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "Hangar");
    let roots = Roots { root, dev_mode };
    if roots.hangar_dir() != hangar {
        return Err(format!(
            "unsupported Hangar layout `{}`",
            hangar.display()
        ));
    }
    Ok(roots)
}

fn source_fingerprint(descriptor: &CcToolchainDescriptor) -> String {
    let mounts = descriptor
        .mounts
        .iter()
        .map(|mount| format!("{}={}", mount.store_path, mount.digest))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "jet-cc-toolchain-v1\nlayout={}\nsource={}\nhost={}\ntarget={}\nversion={}\nabi={}\ncompiler={}\ncompiler.sha256={}\ncxx={}\ncxx.sha256={}\nlinker={}\nlinker.sha256={}\nsysroot={}\nsysroot.sha256={}\nbundle.sha256={}\nmounts={}\nreferences={}\n",
        descriptor.layout,
        descriptor.producer.immutable_source,
        descriptor.host,
        descriptor.target,
        descriptor.version,
        descriptor.abi,
        descriptor.compiler,
        descriptor.compiler_sha256,
        descriptor.cxx,
        descriptor.cxx_sha256,
        descriptor.linker,
        descriptor.linker_sha256,
        descriptor.sysroot,
        descriptor.sysroot_sha256,
        descriptor.bundle_sha256,
        mounts,
        descriptor.references.join(","),
    )
}

fn manifest_facts(manifest: &BundleManifest) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("cc.schema".into(), SCHEMA.into()),
        ("cc.producer".into(), PRODUCER.into()),
        ("cc.layout".into(), FIXTURE_LAYOUT.into()),
        ("cc.host".into(), manifest.host.clone()),
        ("cc.target".into(), manifest.target.clone()),
        ("cc.version".into(), manifest.version.clone()),
        ("cc.abi".into(), manifest.abi.clone()),
        ("cc.compiler".into(), manifest.compiler.clone()),
        ("cc.compiler.sha256".into(), manifest.compiler_sha256.clone()),
        ("cc.cxx".into(), manifest.cxx.clone()),
        ("cc.cxx.sha256".into(), manifest.cxx_sha256.clone()),
        ("cc.linker".into(), manifest.linker.clone()),
        ("cc.linker.sha256".into(), manifest.linker_sha256.clone()),
        ("cc.sysroot".into(), manifest.sysroot.clone()),
        ("cc.sysroot.sha256".into(), manifest.sysroot_sha256.clone()),
        ("cc.bundle.sha256".into(), manifest.bundle_sha256.clone()),
        (
            "cc.compiler.root.sha256".into(),
            manifest.bundle_sha256.clone(),
        ),
        ("cc.cxx.root.sha256".into(), manifest.bundle_sha256.clone()),
        ("cc.linker.root.sha256".into(), manifest.bundle_sha256.clone()),
        ("cc.sysroot.root.sha256".into(), manifest.bundle_sha256.clone()),
        ("cc.mount.count".into(), "1".into()),
        ("cc.mount.0.digest".into(), manifest.bundle_sha256.clone()),
        ("cc.manifest".into(), manifest.source.clone()),
    ])
}

fn fixture_manifest(
    fixtures: &Path,
    target: &str,
) -> Result<Option<BundleManifest>, String> {
    let target = normalize_target(target)?;
    let exact = fixtures.join(format!("{FIXTURE_MANIFEST}-{target}.json"));
    let generic = fixtures.join(format!("{FIXTURE_MANIFEST}.json"));
    let path = if fs::symlink_metadata(&exact)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        exact
    } else if target == host_target_triple()
        && fs::symlink_metadata(&generic)
            .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            .unwrap_or(false)
    {
        generic
    } else {
        return Ok(None);
    };
    let bytes = read_bounded_file(&path, MAX_MANIFEST_BYTES)?;
    let value = JSON::parse(
        std::str::from_utf8(&bytes).map_err(|_| "C toolchain manifest is not UTF-8".to_string())?,
    )
    .map_err(|error| format!("parse C toolchain manifest: {error}"))?;
    let object = value
        .as_object()
        .map_err(|error| format!("C toolchain manifest: {error}"))?;
    let schema = json_number(object, "schema")?;
    if schema != 1 {
        return Err("unsupported C toolchain manifest schema".to_string());
    }
    let manifest = BundleManifest {
        host: json_string(object, "host")?,
        target: normalize_target(&json_string(object, "target")?)?,
        version: json_string(object, "version")?,
        abi: json_string(object, "abi")?,
        bundle: json_string(object, "bundle")?,
        compiler: json_string(object, "compiler")?,
        compiler_sha256: json_string(object, "compiler_sha256")?,
        cxx: json_string(object, "cxx")?,
        cxx_sha256: json_string(object, "cxx_sha256")?,
        linker: json_string(object, "linker")?,
        linker_sha256: json_string(object, "linker_sha256")?,
        sysroot: json_string(object, "sysroot")?,
        sysroot_sha256: json_string(object, "sysroot_sha256")?,
        bundle_sha256: json_string(object, "bundle_sha256")?,
        source: format!(
            "fixture://{}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(FIXTURE_MANIFEST)
        ),
    };
    if manifest.target != target {
        return Err(format!(
            "C toolchain manifest target `{}` does not match requested `{target}`",
            manifest.target
        ));
    }
    if manifest.host != host_target_triple() {
        return Err(format!(
            "C toolchain manifest host `{}` does not match this machine `{}`",
            manifest.host,
            host_target_triple()
        ));
    }
    validate_manifest_shape(&manifest)?;
    Ok(Some(manifest))
}

fn validate_manifest_shape(manifest: &BundleManifest) -> Result<(), String> {
    validate_relative_path(&manifest.bundle)?;
    for path in [
        manifest.compiler.as_str(),
        manifest.cxx.as_str(),
        manifest.linker.as_str(),
        manifest.sysroot.as_str(),
    ] {
        validate_relative_path(path)?;
    }
    for (value, label) in [
        (&manifest.compiler_sha256, "compiler digest"),
        (&manifest.cxx_sha256, "C++ compiler digest"),
        (&manifest.linker_sha256, "linker digest"),
    ] {
        validate_sha256(value, label)?;
    }
    validate_tree_digest(&manifest.sysroot_sha256, "sysroot digest")?;
    validate_tree_digest(&manifest.bundle_sha256, "bundle digest")?;
    validate_text_field(&manifest.version, "version")?;
    validate_text_field(&manifest.abi, "ABI")?;
    Ok(())
}

fn manifest_bundle_root(
    fixtures: Option<&Path>,
    manifest: &BundleManifest,
) -> Result<PathBuf, String> {
    let fixtures = fixtures.ok_or_else(|| "C toolchain fixture root is missing".to_string())?;
    let root = checked_relative_path(fixtures, &manifest.bundle, "bundle")?;
    let metadata = fs::symlink_metadata(&root)
        .map_err(|error| format!("C toolchain bundle is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("C toolchain bundle is not a real directory".to_string());
    }
    Ok(root)
}

fn verify_manifest_payload(root: &Path, manifest: &BundleManifest) -> Result<(), String> {
    verify_file(root, &manifest.compiler, &manifest.compiler_sha256, "compiler")?;
    verify_file(root, &manifest.cxx, &manifest.cxx_sha256, "C++ compiler")?;
    verify_file(root, &manifest.linker, &manifest.linker_sha256, "linker")?;
    verify_tree(root, &manifest.sysroot, &manifest.sysroot_sha256, "sysroot")?;
    Ok(())
}

fn read_bounded_file(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    read_bounded_file_named(path, limit, "C toolchain manifest")
}

fn read_bounded_file_named(path: &Path, limit: u64, label: &str) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("read {label}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{label} is not a regular file"));
    }
    if metadata.len() > limit {
        return Err(format!("{label} exceeds its size limit"));
    }
    let mut file = File::open(path).map_err(|error| format!("read {label}: {error}"))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read {label}: {error}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{label} exceeds its size limit"));
    }
    Ok(bytes)
}

fn json_string(
    object: &BTreeMap<String, JSON::JSONValue>,
    field: &str,
) -> Result<String, String> {
    object
        .get(field)
        .ok_or_else(|| format!("C toolchain manifest misses `{field}`"))?
        .as_str()
        .map(str::to_string)
        .map_err(|error| format!("C toolchain manifest field `{field}`: {error}"))
}

fn json_number(
    object: &BTreeMap<String, JSON::JSONValue>,
    field: &str,
) -> Result<i64, String> {
    match object.get(field) {
        Some(JSON::JSONValue::Number(value)) => Ok(*value),
        _ => Err(format!("C toolchain manifest field `{field}` is not a number")),
    }
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let mut nodes = 0usize;
    let mut bytes = 0u64;
    copy_tree_inner(source, destination, &mut nodes, &mut bytes)
}

fn copy_tree_inner(
    source: &Path,
    destination: &Path,
    nodes: &mut usize,
    bytes: &mut u64,
) -> Result<(), String> {
    *nodes = nodes.saturating_add(1);
    if *nodes > MAX_BUNDLE_NODES {
        return Err("C toolchain bundle has too many filesystem nodes".to_string());
    }
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("read C toolchain bundle: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "C toolchain bundle contains symlink `{}`",
            source.display()
        ));
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination)
            .map_err(|error| format!("create C toolchain bundle directory: {error}"))?;
        let mut children = fs::read_dir(source)
            .map_err(|error| format!("read C toolchain bundle directory: {error}"))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read C toolchain bundle directory: {error}"))?;
        children.sort();
        for child in children {
            let name = child
                .file_name()
                .ok_or_else(|| "C toolchain bundle has an unnamed node".to_string())?;
            copy_tree_inner(&child, &destination.join(name), nodes, bytes)?;
        }
        fs::set_permissions(destination, metadata.permissions())
            .map_err(|error| format!("preserve C toolchain bundle permissions: {error}"))?;
    } else if metadata.is_file() {
        *bytes = bytes
            .checked_add(metadata.len())
            .ok_or_else(|| "C toolchain bundle size overflowed".to_string())?;
        if *bytes > MAX_BUNDLE_BYTES {
            return Err("C toolchain bundle exceeds its size limit".to_string());
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create C toolchain bundle parent: {error}"))?;
        }
        fs::copy(source, destination)
            .map_err(|error| format!("copy C toolchain bundle file: {error}"))?;
        fs::set_permissions(destination, metadata.permissions())
            .map_err(|error| format!("preserve C toolchain file permissions: {error}"))?;
    } else {
        return Err(format!(
            "C toolchain bundle contains unsupported node `{}`",
            source.display()
        ));
    }
    Ok(())
}

fn ensure_real_directory(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| format!("create Hangar directory: {error}"))?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("inspect Hangar directory: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("Hangar path `{}` is not a real directory", path.display()));
    }
    Ok(())
}

fn remove_tree(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        let _ = fs::remove_file(path);
    } else if metadata.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cc_refs_are_one_native_provider_surface() {
        let spec = crate::RefSpec::classify("cc-toolchain@jetpack").unwrap();
        assert!(is_reference(&spec));
        assert_eq!(requested_target(&spec).unwrap(), host_target_triple());
    }

    #[test]
    fn target_selector_is_explicit_and_safe() {
        let spec = crate::RefSpec::classify("cc-toolchain@jetpack#target=aarch64-unknown-linux-gnu")
            .unwrap();
        assert_eq!(
            requested_target(&spec).unwrap(),
            "aarch64-unknown-linux-gnu"
        );
        assert!(normalize_target("../host").is_err());
    }

    #[test]
    fn manifest_text_cannot_escape_publication_path() {
        assert!(validate_text_field("../outside", "version").is_err());
        assert!(validate_text_field("1\n2", "version").is_err());
        assert!(validate_text_field("clang-18", "version").is_ok());
    }
}
