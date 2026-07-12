use super::handles::{ProbeId, SigningIdentityId, ToolchainHandle, ToolchainId};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainSpec {
    pub role: ToolchainRole,
    pub host_triple: String,
    pub target_triple: String,
    pub sdk: Option<SdkIdentity>,
    pub linker: Option<LinkerIdentity>,
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
