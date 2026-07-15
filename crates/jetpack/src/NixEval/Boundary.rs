//! Authority boundary shared by every native evaluator stage.
//!
//! Stage implementations live below `NixEval` and require a permit minted here.
//! Only unit-test harnesses can mint partial-stage permits. Oracle execution is
//! separately gated by exact committed build identity. JP11 must add a distinct
//! verified product entry point; provider code cannot reach this private module.

#![allow(dead_code)] // B-F consume this seam as they land; product use stays forbidden.

use crate::JSON::{self, Json};
use std::collections::BTreeMap;
use std::fmt;

const ORACLE_JSON: &str = include_str!("../../../../tests/fixtures/nix-compat/oracle.json");
const NIX_VERSION: &str = "2.34.8";
const NIX_TAG_OBJECT: &str = "b6769c588f60b3e762f73d3a8cf60294df078ccd";
const NIX_SOURCE_COMMIT: &str = "f3f1c3c5b8ad91850e0f7c590cf177f7ab022024";
const NIXPKGS_REVISION: &str = "b5aa0fbd538984f6e3d201be0005b4463d8b09f8";
const NIXPKGS_LAST_MODIFIED: u64 = 1_782_723_713;
const NIXPKGS_NAR_HASH: &str = "sha256-oPXCU/SSUokcGaJREHibG1CBX3+s/W7orDWQOZDsEeQ=";
const REQUIRED_SYSTEMS: [&str; 4] = [
    "aarch64-darwin",
    "aarch64-linux",
    "x86_64-darwin",
    "x86_64-linux",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::NixEval) enum BoundaryError {
    Manifest(String),
    UnsupportedOracleSystem(String),
    MissingOracleIdentity { system: String, field: &'static str },
    OracleIdentityMismatch {
        system: String,
        field: &'static str,
        expected: String,
        actual: String,
    },
    OracleBuildBlocked { system: String, status: String },
}

impl fmt::Display for BoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(reason) => write!(f, "invalid pinned Nix oracle manifest: {reason}"),
            Self::UnsupportedOracleSystem(system) => {
                write!(f, "unsupported pinned Nix oracle system `{system}`")
            }
            Self::MissingOracleIdentity { system, field } => {
                write!(f, "pinned Nix oracle `{system}` is missing `{field}`")
            }
            Self::OracleIdentityMismatch {
                system,
                field,
                expected,
                actual,
            } => write!(
                f,
                "pinned Nix oracle `{system}` `{field}` mismatch: expected `{expected}`, got `{actual}`"
            ),
            Self::OracleBuildBlocked { system, status } => {
                write!(f, "pinned Nix oracle `{system}` is `{status}`")
            }
        }
    }
}

impl std::error::Error for BoundaryError {}

type Result<T> = std::result::Result<T, BoundaryError>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct OracleBuild {
    build_nar_hash: Option<String>,
    executable_nar_hash: Option<String>,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::NixEval) struct OracleManifest {
    nix_version: String,
    nix_tag_object: String,
    nix_source_commit: String,
    builds: BTreeMap<String, OracleBuild>,
    nixpkgs_revision: String,
    nixpkgs_last_modified: u64,
    nixpkgs_nar_hash: String,
    corpus_status: String,
}

impl OracleManifest {
    pub(in crate::NixEval) fn embedded() -> Result<Self> {
        let manifest = Self::parse(ORACLE_JSON)?;
        manifest.validate_pin()?;
        Ok(manifest)
    }

    pub(in crate::NixEval) fn parse(text: &str) -> Result<Self> {
        let root = JSON::parse(text).map_err(BoundaryError::Manifest)?;
        let root = object(&root, "root")?;
        exact_keys(root, &["schema", "nix", "nixpkgs", "corpus_status"], "root")?;
        let schema = root
            .get("schema")
            .ok_or_else(|| missing("root.schema"))?;
        if !matches!(schema, Json::Num(value) if *value == 1.0) {
            return Err(BoundaryError::Manifest("`schema` must be 1".into()));
        }

        let nix = child_object(root, "nix")?;
        exact_keys(nix, &["version", "tag_object", "source_commit", "builds"], "nix")?;
        let builds_obj = child_object(nix, "builds")?;
        let mut builds = BTreeMap::new();
        for (system, value) in builds_obj {
            let build = object(value, &format!("nix.builds.{system}"))?;
            exact_keys(
                build,
                &["build_nar_hash", "executable_nar_hash", "status"],
                &format!("nix.builds.{system}"),
            )?;
            builds.insert(
                system.clone(),
                OracleBuild {
                    build_nar_hash: optional_string(build, "build_nar_hash")?,
                    executable_nar_hash: optional_string(build, "executable_nar_hash")?,
                    status: required_string(build, "status")?,
                },
            );
        }

        let nixpkgs = child_object(root, "nixpkgs")?;
        exact_keys(nixpkgs, &["rev", "last_modified", "nar_hash"], "nixpkgs")?;
        Ok(Self {
            nix_version: required_string(nix, "version")?,
            nix_tag_object: required_string(nix, "tag_object")?,
            nix_source_commit: required_string(nix, "source_commit")?,
            builds,
            nixpkgs_revision: required_string(nixpkgs, "rev")?,
            nixpkgs_last_modified: required_u64(nixpkgs, "last_modified")?,
            nixpkgs_nar_hash: required_string(nixpkgs, "nar_hash")?,
            corpus_status: required_string(root, "corpus_status")?,
        })
    }

    fn validate_pin(&self) -> Result<()> {
        exact("nix.version", &self.nix_version, NIX_VERSION)?;
        exact("nix.tag_object", &self.nix_tag_object, NIX_TAG_OBJECT)?;
        exact("nix.source_commit", &self.nix_source_commit, NIX_SOURCE_COMMIT)?;
        exact("nixpkgs.rev", &self.nixpkgs_revision, NIXPKGS_REVISION)?;
        if self.nixpkgs_last_modified != NIXPKGS_LAST_MODIFIED {
            return Err(BoundaryError::Manifest(format!(
                "`nixpkgs.last_modified` must be `{NIXPKGS_LAST_MODIFIED}`, got `{}`",
                self.nixpkgs_last_modified
            )));
        }
        exact("nixpkgs.nar_hash", &self.nixpkgs_nar_hash, NIXPKGS_NAR_HASH)?;
        let systems: Vec<&str> = self.builds.keys().map(String::as_str).collect();
        if systems != REQUIRED_SYSTEMS {
            return Err(BoundaryError::Manifest(format!(
                "oracle systems must be exactly {REQUIRED_SYSTEMS:?}, got {systems:?}"
            )));
        }
        for (system, build) in &self.builds {
            for (field, value) in [
                ("build_nar_hash", build.build_nar_hash.as_deref()),
                ("executable_nar_hash", build.executable_nar_hash.as_deref()),
            ] {
                if let Some(value) = value {
                    if !value.starts_with("sha256-") || value.len() <= "sha256-".len() {
                        return Err(BoundaryError::Manifest(format!(
                            "nix.builds.{system}.{field} is not a NAR hash"
                        )));
                    }
                }
            }
            if build.status != "blocked" && build.status != "ready" {
                return Err(BoundaryError::Manifest(format!(
                    "nix.builds.{system}.status must be `blocked` or `ready`"
                )));
            }
        }
        Ok(())
    }

    pub(in crate::NixEval) fn verify_oracle(
        &self,
        observed: &OracleBuildIdentity,
    ) -> Result<VerifiedOracle> {
        let expected = self
            .builds
            .get(&observed.system)
            .ok_or_else(|| BoundaryError::UnsupportedOracleSystem(observed.system.clone()))?;
        let build_hash = expected.build_nar_hash.as_deref().ok_or_else(|| {
            BoundaryError::MissingOracleIdentity {
                system: observed.system.clone(),
                field: "build_nar_hash",
            }
        })?;
        let executable_hash = expected.executable_nar_hash.as_deref().ok_or_else(|| {
            BoundaryError::MissingOracleIdentity {
                system: observed.system.clone(),
                field: "executable_nar_hash",
            }
        })?;
        compare_identity(
            &observed.system,
            "build_nar_hash",
            build_hash,
            &observed.build_nar_hash,
        )?;
        compare_identity(
            &observed.system,
            "executable_nar_hash",
            executable_hash,
            &observed.executable_nar_hash,
        )?;
        if expected.status != "ready" {
            return Err(BoundaryError::OracleBuildBlocked {
                system: observed.system.clone(),
                status: expected.status.clone(),
            });
        }
        Ok(VerifiedOracle {
            system: observed.system.clone(),
        })
    }

    pub(in crate::NixEval) fn nix_version(&self) -> &str {
        &self.nix_version
    }

    pub(in crate::NixEval) fn nix_tag_object(&self) -> &str {
        &self.nix_tag_object
    }

    pub(in crate::NixEval) fn nix_source_commit(&self) -> &str {
        &self.nix_source_commit
    }

    pub(in crate::NixEval) fn nixpkgs_revision(&self) -> &str {
        &self.nixpkgs_revision
    }

    pub(in crate::NixEval) fn nixpkgs_last_modified(&self) -> u64 {
        self.nixpkgs_last_modified
    }

    pub(in crate::NixEval) fn nixpkgs_nar_hash(&self) -> &str {
        &self.nixpkgs_nar_hash
    }

    pub(in crate::NixEval) fn systems(&self) -> Vec<&str> {
        self.builds.keys().map(String::as_str).collect()
    }

    fn product_ready(&self) -> bool {
        self.corpus_status == "bit_exact"
            && self.builds.values().all(|build| {
                build.status == "ready"
                    && build.build_nar_hash.is_some()
                    && build.executable_nar_hash.is_some()
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::NixEval) struct OracleBuildIdentity {
    system: String,
    build_nar_hash: String,
    executable_nar_hash: String,
}

impl OracleBuildIdentity {
    pub(in crate::NixEval) fn new(
        system: &str,
        build_nar_hash: &str,
        executable_nar_hash: &str,
    ) -> Self {
        Self {
            system: system.to_string(),
            build_nar_hash: build_nar_hash.to_string(),
            executable_nar_hash: executable_nar_hash.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::NixEval) struct VerifiedOracle {
    system: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::NixEval) enum InternalStage {
    Syntax,
    Values,
    Evaluation,
    Authority,
    Derivation,
    Flakes,
}

#[derive(Debug)]
pub(in crate::NixEval) struct InternalTestHarness {
    private: (),
}

impl InternalTestHarness {
    pub(in crate::NixEval) fn engine(&self) -> &'static str {
        "native-jetpack"
    }
}

#[derive(Debug)]
pub(in crate::NixEval) struct InternalStagePermit {
    stage: InternalStage,
}

impl InternalStagePermit {
    pub(in crate::NixEval) fn stage(&self) -> InternalStage {
        self.stage
    }
}

#[derive(Debug, Clone)]
pub(in crate::NixEval) struct NativeBoundary {
    manifest: OracleManifest,
}

impl NativeBoundary {
    pub(in crate::NixEval) fn embedded() -> Result<Self> {
        Ok(Self {
            manifest: OracleManifest::embedded()?,
        })
    }

    #[cfg(test)]
    pub(in crate::NixEval) fn internal_test_harness(&self) -> InternalTestHarness {
        InternalTestHarness { private: () }
    }

    pub(in crate::NixEval) fn authorize_internal(
        &self,
        _harness: &InternalTestHarness,
        stage: InternalStage,
    ) -> InternalStagePermit {
        InternalStagePermit { stage }
    }

    pub(in crate::NixEval) fn product_ready(&self) -> bool {
        self.manifest.product_ready()
    }
}

fn object<'a>(value: &'a Json, path: &str) -> Result<&'a BTreeMap<String, Json>> {
    value
        .as_object()
        .map_err(|_| BoundaryError::Manifest(format!("`{path}` must be an object")))
}

fn child_object<'a>(
    parent: &'a BTreeMap<String, Json>,
    key: &str,
) -> Result<&'a BTreeMap<String, Json>> {
    let value = parent.get(key).ok_or_else(|| missing(key))?;
    object(value, key)
}

fn required_string(parent: &BTreeMap<String, Json>, key: &str) -> Result<String> {
    parent
        .get(key)
        .ok_or_else(|| missing(key))?
        .as_str()
        .map(str::to_string)
        .map_err(|_| BoundaryError::Manifest(format!("`{key}` must be a string")))
}

fn required_u64(parent: &BTreeMap<String, Json>, key: &str) -> Result<u64> {
    match parent.get(key).ok_or_else(|| missing(key))? {
        Json::Num(value)
            if value.is_finite()
                && *value >= 0.0
                && value.fract() == 0.0
                && *value <= u64::MAX as f64 =>
        {
            Ok(*value as u64)
        }
        _ => Err(BoundaryError::Manifest(format!(
            "`{key}` must be an unsigned integer"
        ))),
    }
}

fn optional_string(parent: &BTreeMap<String, Json>, key: &str) -> Result<Option<String>> {
    match parent.get(key).ok_or_else(|| missing(key))? {
        Json::Null => Ok(None),
        Json::Str(value) => Ok(Some(value.clone())),
        _ => Err(BoundaryError::Manifest(format!(
            "`{key}` must be a string or null"
        ))),
    }
}

fn exact_keys(parent: &BTreeMap<String, Json>, expected: &[&str], path: &str) -> Result<()> {
    let actual: Vec<&str> = parent.keys().map(String::as_str).collect();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    if actual != expected {
        return Err(BoundaryError::Manifest(format!(
            "`{path}` keys must be exactly {expected:?}, got {actual:?}"
        )));
    }
    Ok(())
}

fn missing(path: &str) -> BoundaryError {
    BoundaryError::Manifest(format!("missing `{path}`"))
}

fn exact(field: &str, actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        return Err(BoundaryError::Manifest(format!(
            "`{field}` must be `{expected}`, got `{actual}`"
        )));
    }
    Ok(())
}

fn compare_identity(system: &str, field: &'static str, expected: &str, actual: &str) -> Result<()> {
    if expected != actual {
        return Err(BoundaryError::OracleIdentityMismatch {
            system: system.to_string(),
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}
