use super::handles::{ProbeId, SigningIdentityId, ToolchainHandle, ToolchainId};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockRecord {
    pub key: String,
    pub digest: String,
}

impl LockRecord {
    pub fn new(key: impl Into<String>, digest: impl Into<String>) -> Self {
        LockRecord {
            key: key.into(),
            digest: digest.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvenanceSource {
    InferredHost,
    JetpackDependency(String),
    AmbientRecord(String),
    UserDeclared(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProvenance {
    pub source: ProvenanceSource,
    pub lock: Option<LockRecord>,
}

impl BuildProvenance {
    pub fn inferred_host() -> Self {
        BuildProvenance {
            source: ProvenanceSource::InferredHost,
            lock: None,
        }
    }

    pub fn jetpack_dependency(dep: impl Into<String>, lock: LockRecord) -> Self {
        BuildProvenance {
            source: ProvenanceSource::JetpackDependency(dep.into()),
            lock: Some(lock),
        }
    }

    pub fn ambient_record(record: impl Into<String>) -> Self {
        BuildProvenance {
            source: ProvenanceSource::AmbientRecord(record.into()),
            lock: None,
        }
    }

    pub fn user_declared(source: impl Into<String>, lock: Option<LockRecord>) -> Self {
        BuildProvenance {
            source: ProvenanceSource::UserDeclared(source.into()),
            lock,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkIdentity {
    pub name: String,
    pub version: String,
    pub provenance: BuildProvenance,
}

impl SdkIdentity {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        provenance: BuildProvenance,
    ) -> Self {
        SdkIdentity {
            name: name.into(),
            version: version.into(),
            provenance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkerIdentity {
    pub name: String,
    pub provenance: BuildProvenance,
}

impl LinkerIdentity {
    pub fn new(name: impl Into<String>, provenance: BuildProvenance) -> Self {
        LinkerIdentity {
            name: name.into(),
            provenance,
        }
    }
}

/// Sysroot identity for cross compilation (E4-JP15) — enters action keys with
/// toolchain / SDK / linker / signing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysrootIdentity {
    pub name: String,
    pub path_digest: String,
    pub provenance: BuildProvenance,
}

impl SysrootIdentity {
    pub fn new(
        name: impl Into<String>,
        path_digest: impl Into<String>,
        provenance: BuildProvenance,
    ) -> Self {
        SysrootIdentity {
            name: name.into(),
            path_digest: path_digest.into(),
            provenance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningIdentitySpec {
    pub label: String,
    pub provenance: BuildProvenance,
}

impl SigningIdentitySpec {
    pub fn new(label: impl Into<String>, provenance: BuildProvenance) -> Self {
        SigningIdentitySpec {
            label: label.into(),
            provenance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSigningIdentity {
    pub id: SigningIdentityId,
    pub name: String,
    pub label: String,
    pub provenance: BuildProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolchainRole {
    Host,
    Target,
}

/// How an action resolves its executable. Ambient resolution is retained for
/// existing host actions. Declared-only resolution is the hermetic path used
/// by provisioned toolchains: no executable may come from PATH or an
/// undeclared filesystem spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolchainResolution {
    #[default]
    Ambient,
    DeclaredOnly,
}

/// A read-only host directory made visible to a sandboxed build action. The
/// source and destination are both part of the action's toolchain identity;
/// the execution adapters only marshal this declaration to their native
/// sandbox backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildMount {
    pub source: String,
    pub destination: String,
    /// Stable content identity for action keys. Empty means the source path
    /// itself is the identity, which preserves the behavior of generic
    /// callers; provisioned toolchains supply their Hangar object digest.
    pub identity: String,
}

impl BuildMount {
    pub fn read_only(
        source: impl Into<String>,
        destination: impl Into<String>,
    ) -> Self {
        BuildMount {
            source: source.into(),
            destination: destination.into(),
            identity: String::new(),
        }
    }

    pub fn read_only_with_identity(
        source: impl Into<String>,
        destination: impl Into<String>,
        identity: impl Into<String>,
    ) -> Self {
        BuildMount {
            source: source.into(),
            destination: destination.into(),
            identity: identity.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainSpec {
    pub role: ToolchainRole,
    pub host_triple: String,
    pub target_triple: String,
    pub sdk: Option<SdkIdentity>,
    pub linker: Option<LinkerIdentity>,
    pub sysroot: Option<SysrootIdentity>,
    /// Executables are part of the declared toolchain. Target actions and
    /// probes resolve through this map instead of borrowing the host PATH.
    pub tools: BTreeMap<String, String>,
    pub resolution: ToolchainResolution,
    pub mounts: Vec<BuildMount>,
    pub provenance: BuildProvenance,
}

impl ToolchainSpec {
    pub fn target(target_triple: impl Into<String>, provenance: BuildProvenance) -> Self {
        ToolchainSpec {
            role: ToolchainRole::Target,
            host_triple: "host".to_string(),
            target_triple: target_triple.into(),
            sdk: None,
            linker: None,
            sysroot: None,
            tools: BTreeMap::new(),
            resolution: ToolchainResolution::Ambient,
            mounts: Vec::new(),
            provenance,
        }
    }

    pub fn host(host_triple: impl Into<String>, provenance: BuildProvenance) -> Self {
        let host_triple = host_triple.into();
        ToolchainSpec {
            role: ToolchainRole::Host,
            target_triple: host_triple.clone(),
            host_triple,
            sdk: None,
            linker: None,
            sysroot: None,
            tools: BTreeMap::new(),
            resolution: ToolchainResolution::Ambient,
            mounts: Vec::new(),
            provenance,
        }
    }

    pub fn with_host_triple(mut self, host_triple: impl Into<String>) -> Self {
        self.host_triple = host_triple.into();
        self
    }

    pub fn with_sdk(mut self, sdk: SdkIdentity) -> Self {
        self.sdk = Some(sdk);
        self
    }

    pub fn with_linker(mut self, linker: LinkerIdentity) -> Self {
        self.linker = Some(linker);
        self
    }

    pub fn with_sysroot(mut self, sysroot: SysrootIdentity) -> Self {
        self.sysroot = Some(sysroot);
        self
    }

    pub fn with_tool(mut self, name: impl Into<String>, path: impl Into<String>) -> Self {
        self.tools.insert(name.into(), path.into());
        self
    }

    /// Require every action executable to be named by this toolchain. This is
    /// the only mode allowed for a hermetic provisioned compiler.
    pub fn declared_only(mut self) -> Self {
        self.resolution = ToolchainResolution::DeclaredOnly;
        self
    }

    /// Make one immutable directory available to actions using this
    /// toolchain. The sandbox validates and mounts it read-only at launch.
    pub fn with_read_only_mount(
        mut self,
        source: impl Into<String>,
        destination: impl Into<String>,
    ) -> Self {
        self.mounts.push(BuildMount::read_only(source, destination));
        self
    }

    pub fn with_read_only_mount_identity(
        mut self,
        source: impl Into<String>,
        destination: impl Into<String>,
        identity: impl Into<String>,
    ) -> Self {
        self.mounts.push(BuildMount::read_only_with_identity(
            source,
            destination,
            identity,
        ));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildToolchain {
    pub id: ToolchainId,
    pub name: String,
    pub role: ToolchainRole,
    pub host_triple: String,
    pub target_triple: String,
    pub sdk: Option<SdkIdentity>,
    pub linker: Option<LinkerIdentity>,
    pub sysroot: Option<SysrootIdentity>,
    pub tools: BTreeMap<String, String>,
    pub resolution: ToolchainResolution,
    pub mounts: Vec<BuildMount>,
    pub provenance: BuildProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReproducibilityClass {
    Reproducible,
    Ambient,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeKind {
    FindProgram {
        program: String,
    },
    PkgConfig {
        package: String,
        min_version: Option<String>,
    },
    HeaderCheck {
        header: String,
    },
    CompileCheck {
        name: String,
        includes: Vec<String>,
        code: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSpec {
    pub kind: ProbeKind,
    pub reproducibility: ReproducibilityClass,
    pub provenance: BuildProvenance,
    pub toolchain: Option<ToolchainHandle>,
}

impl ProbeSpec {
    pub fn find_program(program: impl Into<String>) -> Self {
        ProbeSpec {
            kind: ProbeKind::FindProgram {
                program: program.into(),
            },
            reproducibility: ReproducibilityClass::Ambient,
            provenance: BuildProvenance::ambient_record("find_program"),
            toolchain: None,
        }
    }

    pub fn pkg_config(package: impl Into<String>) -> Self {
        ProbeSpec {
            kind: ProbeKind::PkgConfig {
                package: package.into(),
                min_version: None,
            },
            reproducibility: ReproducibilityClass::Ambient,
            provenance: BuildProvenance::ambient_record("pkg_config"),
            toolchain: None,
        }
    }

    pub fn header_check(header: impl Into<String>) -> Self {
        ProbeSpec {
            kind: ProbeKind::HeaderCheck {
                header: header.into(),
            },
            reproducibility: ReproducibilityClass::Ambient,
            provenance: BuildProvenance::ambient_record("header_check"),
            toolchain: None,
        }
    }

    pub fn compile_check(
        name: impl Into<String>,
        includes: impl IntoIterator<Item = impl Into<String>>,
        code: impl Into<String>,
    ) -> Self {
        ProbeSpec {
            kind: ProbeKind::CompileCheck {
                name: name.into(),
                includes: includes.into_iter().map(Into::into).collect(),
                code: code.into(),
            },
            reproducibility: ReproducibilityClass::Ambient,
            provenance: BuildProvenance::ambient_record("compile_check"),
            toolchain: None,
        }
    }

    pub fn with_min_version(mut self, version: impl Into<String>) -> Self {
        if let ProbeKind::PkgConfig { min_version, .. } = &mut self.kind {
            *min_version = Some(version.into());
        }
        self
    }

    pub fn with_toolchain(mut self, toolchain: ToolchainHandle) -> Self {
        self.toolchain = Some(toolchain);
        self
    }

    pub fn with_reproducibility(mut self, reproducibility: ReproducibilityClass) -> Self {
        self.reproducibility = reproducibility;
        self
    }

    pub fn with_provenance(mut self, provenance: BuildProvenance) -> Self {
        self.provenance = provenance;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProbe {
    pub id: ProbeId,
    pub name: String,
    pub kind: ProbeKind,
    pub reproducibility: ReproducibilityClass,
    pub provenance: BuildProvenance,
    pub toolchain: ToolchainHandle,
}
