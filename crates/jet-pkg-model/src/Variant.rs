//! Typed package variants (E4-JP15 / D-JPK-VARIANT1=D).
//!
//! Closed native axes only: build/host/target role, OS, arch, runtime/libc,
//! linkage, ABI, artifact kind, and feature set. Every axis has a context-
//! derived default so beginners never write variants. Matching is
//! exact-then-compatible under one total order; an ambiguous tie is E1316.
//! Selected variants enter action keys and the semantic lock. Provider facts
//! influence selection only through explicit `variant_map` entries.

use crate::Diagnostics::Diagnostic;
use crate::Platform::{self, PlatformKey};
use std::collections::{BTreeMap, BTreeSet};

/// Dependency / build graph role (Nix-style build/host/target).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VariantRole {
    Build,
    Host,
    Target,
}

impl VariantRole {
    pub fn as_str(self) -> &'static str {
        match self {
            VariantRole::Build => "build",
            VariantRole::Host => "host",
            VariantRole::Target => "target",
        }
    }

    pub fn parse(raw: &str) -> Option<VariantRole> {
        match raw {
            "build" => Some(VariantRole::Build),
            "host" => Some(VariantRole::Host),
            "target" => Some(VariantRole::Target),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VariantOs {
    Linux,
    Macos,
    Windows,
}

impl VariantOs {
    pub fn as_str(self) -> &'static str {
        match self {
            VariantOs::Linux => Platform::OS_LINUX,
            VariantOs::Macos => Platform::OS_MACOS,
            VariantOs::Windows => Platform::OS_WINDOWS,
        }
    }

    pub fn parse(raw: &str) -> Option<VariantOs> {
        match raw {
            Platform::OS_LINUX | "Linux" => Some(VariantOs::Linux),
            Platform::OS_MACOS | "Macos" | "Darwin" | "darwin" => Some(VariantOs::Macos),
            Platform::OS_WINDOWS | "Windows" => Some(VariantOs::Windows),
            _ => None,
        }
    }

    pub fn from_platform_os(os: &str) -> VariantOs {
        VariantOs::parse(os).unwrap_or(VariantOs::Linux)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VariantArch {
    X86_64,
    Aarch64,
}

impl VariantArch {
    pub fn as_str(self) -> &'static str {
        match self {
            VariantArch::X86_64 => Platform::ARCH_X64,
            VariantArch::Aarch64 => Platform::ARCH_ARM64,
        }
    }

    pub fn parse(raw: &str) -> Option<VariantArch> {
        match raw {
            Platform::ARCH_X64 | "x64" | "amd64" => Some(VariantArch::X86_64),
            Platform::ARCH_ARM64 | "arm64" | "Aarch64" => Some(VariantArch::Aarch64),
            _ => None,
        }
    }

    pub fn from_platform_arch(arch: &str) -> VariantArch {
        VariantArch::parse(arch).unwrap_or(VariantArch::X86_64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VariantLibc {
    Gnu,
    Musl,
    Msvc,
    None,
}

impl VariantLibc {
    pub fn as_str(self) -> &'static str {
        match self {
            VariantLibc::Gnu => "gnu",
            VariantLibc::Musl => "musl",
            VariantLibc::Msvc => "msvc",
            VariantLibc::None => "none",
        }
    }

    pub fn parse(raw: &str) -> Option<VariantLibc> {
        match raw {
            "gnu" | "glibc" => Some(VariantLibc::Gnu),
            "musl" => Some(VariantLibc::Musl),
            "msvc" => Some(VariantLibc::Msvc),
            "none" | "null" => Some(VariantLibc::None),
            _ => None,
        }
    }

    pub fn default_for(os: VariantOs) -> VariantLibc {
        match os {
            VariantOs::Linux => VariantLibc::Gnu,
            VariantOs::Macos => VariantLibc::None,
            VariantOs::Windows => VariantLibc::Msvc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VariantLinkage {
    Shared,
    Static,
}

impl VariantLinkage {
    pub fn as_str(self) -> &'static str {
        match self {
            VariantLinkage::Shared => "shared",
            VariantLinkage::Static => "static",
        }
    }

    pub fn parse(raw: &str) -> Option<VariantLinkage> {
        match raw {
            "shared" | "dynamic" => Some(VariantLinkage::Shared),
            "static" => Some(VariantLinkage::Static),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VariantAbi {
    Sysv,
    Ms,
    Unknown,
}

impl VariantAbi {
    pub fn as_str(self) -> &'static str {
        match self {
            VariantAbi::Sysv => "sysv",
            VariantAbi::Ms => "ms",
            VariantAbi::Unknown => "unknown",
        }
    }

    pub fn parse(raw: &str) -> Option<VariantAbi> {
        match raw {
            "sysv" => Some(VariantAbi::Sysv),
            "ms" | "msvc" => Some(VariantAbi::Ms),
            "unknown" => Some(VariantAbi::Unknown),
            _ => None,
        }
    }

    pub fn default_for(os: VariantOs) -> VariantAbi {
        match os {
            VariantOs::Windows => VariantAbi::Ms,
            VariantOs::Linux | VariantOs::Macos => VariantAbi::Sysv,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArtifactKind {
    Library,
    Executable,
    Object,
    Archive,
    Headers,
    DevTool,
    Sysroot,
    Any,
}

impl ArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::Library => "library",
            ArtifactKind::Executable => "executable",
            ArtifactKind::Object => "object",
            ArtifactKind::Archive => "archive",
            ArtifactKind::Headers => "headers",
            ArtifactKind::DevTool => "dev-tool",
            ArtifactKind::Sysroot => "sysroot",
            ArtifactKind::Any => "any",
        }
    }

    pub fn parse(raw: &str) -> Option<ArtifactKind> {
        match raw {
            "library" | "lib" => Some(ArtifactKind::Library),
            "executable" | "bin" => Some(ArtifactKind::Executable),
            "object" | "obj" => Some(ArtifactKind::Object),
            "archive" => Some(ArtifactKind::Archive),
            "headers" | "include" => Some(ArtifactKind::Headers),
            "dev-tool" | "tool" => Some(ArtifactKind::DevTool),
            "sysroot" => Some(ArtifactKind::Sysroot),
            "any" => Some(ArtifactKind::Any),
            _ => None,
        }
    }
}

/// Closed typed variant (D-JPK-VARIANT1=D).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageVariant {
    pub role: VariantRole,
    pub os: VariantOs,
    pub arch: VariantArch,
    pub libc: VariantLibc,
    pub linkage: VariantLinkage,
    pub abi: VariantAbi,
    pub artifact: ArtifactKind,
    pub features: BTreeSet<String>,
}

impl PackageVariant {
    /// Total defaults from requesting context: host platform, Host role,
    /// shared linkage, library artifact, empty features.
    pub fn defaults_for_context(platform: &PlatformKey) -> PackageVariant {
        let os = VariantOs::from_platform_os(&platform.os);
        let arch = VariantArch::from_platform_arch(&platform.arch);
        PackageVariant {
            role: VariantRole::Host,
            os,
            arch,
            libc: VariantLibc::default_for(os),
            linkage: VariantLinkage::Shared,
            abi: VariantAbi::default_for(os),
            artifact: ArtifactKind::Library,
            features: BTreeSet::new(),
        }
    }

    pub fn host_defaults() -> PackageVariant {
        Self::defaults_for_context(&PlatformKey::host())
    }

    pub fn with_role(mut self, role: VariantRole) -> Self {
        self.role = role;
        self
    }

    pub fn with_os(mut self, os: VariantOs) -> Self {
        self.os = os;
        self.libc = VariantLibc::default_for(os);
        self.abi = VariantAbi::default_for(os);
        self
    }

    pub fn with_arch(mut self, arch: VariantArch) -> Self {
        self.arch = arch;
        self
    }

    pub fn with_libc(mut self, libc: VariantLibc) -> Self {
        self.libc = libc;
        self
    }

    pub fn with_linkage(mut self, linkage: VariantLinkage) -> Self {
        self.linkage = linkage;
        self
    }

    pub fn with_abi(mut self, abi: VariantAbi) -> Self {
        self.abi = abi;
        self
    }

    pub fn with_artifact(mut self, artifact: ArtifactKind) -> Self {
        self.artifact = artifact;
        self
    }

    pub fn with_feature(mut self, feature: impl Into<String>) -> Self {
        self.features.insert(feature.into());
        self
    }

    /// Canonical identity string for action keys and semantic lock `platform`.
    pub fn identity_key(&self) -> String {
        let mut feats: Vec<&str> = self.features.iter().map(String::as_str).collect();
        feats.sort_unstable();
        format!(
            "role={};os={};arch={};libc={};linkage={};abi={};artifact={};features={}",
            self.role.as_str(),
            self.os.as_str(),
            self.arch.as_str(),
            self.libc.as_str(),
            self.linkage.as_str(),
            self.abi.as_str(),
            self.artifact.as_str(),
            feats.join(",")
        )
    }

    /// Cross-compile need: Target role on a foreign triple.
    pub fn cross_target(os: VariantOs, arch: VariantArch, libc: VariantLibc) -> PackageVariant {
        PackageVariant {
            role: VariantRole::Target,
            os,
            arch,
            libc,
            linkage: VariantLinkage::Shared,
            abi: VariantAbi::default_for(os),
            artifact: ArtifactKind::Library,
            features: BTreeSet::new(),
        }
    }

    /// Exact match: every closed axis equal; features must be equal sets.
    pub fn matches_exact(&self, need: &PackageVariant) -> bool {
        self == need
    }

    /// Compatible under the total order: producer may be more specific on
    /// features (superset) and may use `ArtifactKind::Any` for any need.
    /// OS/arch/libc/linkage/abi/role must match exactly.
    pub fn matches_compatible(&self, need: &PackageVariant) -> bool {
        self.role == need.role
            && self.os == need.os
            && self.arch == need.arch
            && self.libc == need.libc
            && self.linkage == need.linkage
            && self.abi == need.abi
            && (self.artifact == need.artifact || self.artifact == ArtifactKind::Any)
            && need.features.is_subset(&self.features)
    }

    /// Axis-name order used for tie-break / ambiguity diagnostics.
    pub const AXIS_ORDER: &'static [&'static str] = &[
        "role",
        "os",
        "arch",
        "libc",
        "linkage",
        "abi",
        "artifact",
        "features",
    ];

    /// First axis where `a` and `b` differ under AXIS_ORDER.
    pub fn first_distinguishing_axis(a: &PackageVariant, b: &PackageVariant) -> &'static str {
        if a.role != b.role {
            return "role";
        }
        if a.os != b.os {
            return "os";
        }
        if a.arch != b.arch {
            return "arch";
        }
        if a.libc != b.libc {
            return "libc";
        }
        if a.linkage != b.linkage {
            return "linkage";
        }
        if a.abi != b.abi {
            return "abi";
        }
        if a.artifact != b.artifact {
            return "artifact";
        }
        "features"
    }
}

/// Candidate offered by a package producer, plus optional namespaced provider
/// facts that never influence native selection unless mapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantCandidate {
    pub label: String,
    pub variant: PackageVariant,
    pub provider_facts: BTreeMap<String, String>,
}

impl VariantCandidate {
    pub fn new(label: impl Into<String>, variant: PackageVariant) -> Self {
        VariantCandidate {
            label: label.into(),
            variant,
            provider_facts: BTreeMap::new(),
        }
    }

    pub fn with_provider_fact(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.provider_facts.insert(key.into(), value.into());
        self
    }
}

/// Explicit mapping from a provider fact key into a closed axis value.
/// Without a map, provider facts are ignored during selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantMapEntry {
    pub provider_key: String,
    pub axis: String,
    pub value: String,
}

/// Apply `variant_map` entries onto a candidate's typed axes before matching.
pub fn apply_variant_map(
    candidate: &VariantCandidate,
    maps: &[VariantMapEntry],
) -> PackageVariant {
    let mut v = candidate.variant.clone();
    for entry in maps {
        let Some(raw) = candidate.provider_facts.get(&entry.provider_key) else {
            continue;
        };
        // Mapped value may come from the fact or the declared map value.
        let use_val = if entry.value.is_empty() {
            raw.as_str()
        } else if raw == &entry.value || entry.value == "*" {
            if entry.value == "*" {
                raw.as_str()
            } else {
                entry.value.as_str()
            }
        } else {
            continue;
        };
        match entry.axis.as_str() {
            "role" => {
                if let Some(role) = VariantRole::parse(use_val) {
                    v.role = role;
                }
            }
            "os" => {
                if let Some(os) = VariantOs::parse(use_val) {
                    v = v.with_os(os);
                }
            }
            "arch" => {
                if let Some(arch) = VariantArch::parse(use_val) {
                    v.arch = arch;
                }
            }
            "libc" | "runtime" => {
                if let Some(libc) = VariantLibc::parse(use_val) {
                    v.libc = libc;
                }
            }
            "linkage" => {
                if let Some(linkage) = VariantLinkage::parse(use_val) {
                    v.linkage = linkage;
                }
            }
            "abi" => {
                if let Some(abi) = VariantAbi::parse(use_val) {
                    v.abi = abi;
                }
            }
            "artifact" => {
                if let Some(kind) = ArtifactKind::parse(use_val) {
                    v.artifact = kind;
                }
            }
            "features" => {
                v.features.insert(use_val.to_string());
            }
            _ => {}
        }
    }
    v
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariantSelectError {
    NoMatch {
        need: PackageVariant,
        candidate_count: usize,
    },
    Ambiguous {
        need: PackageVariant,
        axis: &'static str,
        labels: Vec<String>,
    },
}

impl VariantSelectError {
    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            VariantSelectError::NoMatch {
                need,
                candidate_count,
            } => Diagnostic::error(
                "E1316",
                format!(
                    "no package variant matches need `{}` among {candidate_count} candidate(s)",
                    need.identity_key()
                ),
                "Native selection uses only the closed typed axes (D-JPK-VARIANT1); none of the offered producers are exact or compatible.".to_string(),
                "Add a producer for this domain, loosen the need, or map provider facts with `variant_map`.".to_string(),
                None,
            ),
            VariantSelectError::Ambiguous {
                need,
                axis,
                labels,
            } => e1316_ambiguous(need, axis, labels),
        }
    }
}

/// E1316 — ambiguous variant candidates after exact-then-compatible matching.
pub fn e1316_ambiguous(
    need: &PackageVariant,
    axis: &str,
    labels: &[String],
) -> Diagnostic {
    let listed = labels.join(", ");
    Diagnostic::error(
        "E1316",
        format!(
            "ambiguous package variants for need `{}`: {listed} (first distinguishing axis: {axis})",
            need.identity_key()
        ),
        "Matching is exact-then-compatible under one total order; an ambiguous tie is never a silent pick (D-JPK-VARIANT1).".to_string(),
        "Add a `variant_map`, pin one candidate, or make the need more specific on the named axis.".to_string(),
        None,
    )
}

/// Select a producer variant: exact winners first, else compatible; one winner
/// only. Tie → Ambiguous naming the first distinguishing axis among survivors.
pub fn select_variant(
    need: &PackageVariant,
    candidates: &[VariantCandidate],
    maps: &[VariantMapEntry],
) -> Result<VariantCandidate, VariantSelectError> {
    let mapped: Vec<(usize, PackageVariant)> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (i, apply_variant_map(c, maps)))
        .collect();

    let exact: Vec<usize> = mapped
        .iter()
        .filter(|(_, v)| v.matches_exact(need))
        .map(|(i, _)| *i)
        .collect();
    let pool = if !exact.is_empty() {
        exact
    } else {
        mapped
            .iter()
            .filter(|(_, v)| v.matches_compatible(need))
            .map(|(i, _)| *i)
            .collect()
    };

    match pool.len() {
        0 => Err(VariantSelectError::NoMatch {
            need: need.clone(),
            candidate_count: candidates.len(),
        }),
        1 => Ok(candidates[pool[0]].clone()),
        _ => {
            let mut labels: Vec<String> = pool
                .iter()
                .map(|&i| candidates[i].label.clone())
                .collect();
            labels.sort();
            let a = &mapped
                .iter()
                .find(|(i, _)| *i == pool[0])
                .unwrap()
                .1;
            let b = &mapped
                .iter()
                .find(|(i, _)| *i == pool[1])
                .unwrap()
                .1;
            // If mapped variants are identical, distinguish via unmapped
            // provider facts that were not folded into axes — still ambiguous.
            let axis = if a == b {
                "provider-facts"
            } else {
                PackageVariant::first_distinguishing_axis(a, b)
            };
            Err(VariantSelectError::Ambiguous {
                need: need.clone(),
                axis,
                labels,
            })
        }
    }
}

/// Universal lock coverage: every declared supported domain must appear as a
/// selected variant identity. Returns missing domain identity keys.
pub fn missing_lock_domains(
    declared: &[PackageVariant],
    locked_identities: &BTreeSet<String>,
) -> Vec<String> {
    declared
        .iter()
        .map(|v| v.identity_key())
        .filter(|k| !locked_identities.contains(k))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_every_axis() {
        let v = PackageVariant::defaults_for_context(&PlatformKey {
            arch: Platform::ARCH_ARM64.to_string(),
            os: Platform::OS_LINUX.to_string(),
        });
        assert_eq!(v.role, VariantRole::Host);
        assert_eq!(v.os, VariantOs::Linux);
        assert_eq!(v.arch, VariantArch::Aarch64);
        assert_eq!(v.libc, VariantLibc::Gnu);
        assert_eq!(v.linkage, VariantLinkage::Shared);
        assert_eq!(v.abi, VariantAbi::Sysv);
        assert_eq!(v.artifact, ArtifactKind::Library);
        assert!(v.features.is_empty());
        assert!(v.identity_key().contains("arch=aarch64"));
    }

    #[test]
    fn exact_beats_compatible_and_ambiguity_names_axis() {
        let need = PackageVariant::cross_target(
            VariantOs::Linux,
            VariantArch::Aarch64,
            VariantLibc::Musl,
        )
        .with_linkage(VariantLinkage::Static);

        let a = VariantCandidate::new(
            "openssl-static-musl",
            need.clone(),
        );
        let _b = VariantCandidate::new(
            "openssl-static-musl-alt",
            need.clone().with_feature("legacy"),
        )
        .with_provider_fact("conan.compiler.runtime", "libstdc++11");

        // Exact unique.
        let got = select_variant(&need, &[a.clone()], &[]).unwrap();
        assert_eq!(got.label, "openssl-static-musl");

        // Two exact → ambiguous via unmapped provider facts.
        let b_exact = VariantCandidate::new("openssl-b", need.clone())
            .with_provider_fact("conan.compiler.runtime", "libstdc++11");
        let err = select_variant(&need, &[a, b_exact], &[]).unwrap_err();
        match err {
            VariantSelectError::Ambiguous { axis, labels, .. } => {
                assert_eq!(axis, "provider-facts");
                assert_eq!(labels.len(), 2);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn variant_map_folds_provider_fact_into_libc() {
        let need = PackageVariant::cross_target(
            VariantOs::Linux,
            VariantArch::Aarch64,
            VariantLibc::Musl,
        );
        let cand = VariantCandidate::new(
            "openssl",
            PackageVariant::cross_target(
                VariantOs::Linux,
                VariantArch::Aarch64,
                VariantLibc::Gnu,
            ),
        )
        .with_provider_fact("conan.compiler.libc", "musl");
        let maps = [VariantMapEntry {
            provider_key: "conan.compiler.libc".to_string(),
            axis: "libc".to_string(),
            value: "*".to_string(),
        }];
        let got = select_variant(&need, &[cand], &maps).unwrap();
        assert_eq!(got.label, "openssl");
    }
}
