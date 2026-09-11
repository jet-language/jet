//! Shared identity and provenance for generated foreign bridge artifacts.
//!
//! A bridge key is a digest of a length-delimited input record.  The record is
//! deliberately built by the caller: the binder or bridge builder owns the
//! descriptor inputs, while this module owns the stable encoding and artifact
//! provenance format.

use crate::AST::{BinderCapability, BinderDescriptor, ForeignAbiContract, ForeignScalar};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const IDENTITY_SCHEMA: &str = "jet-ffi-bridge-identity-v1";
pub const PROVENANCE_SCHEMA: &str = "jet-ffi-bridge-provenance-v1";
pub const FOREIGN_BOUNDARY_SCHEMA: &str = "jet-ffi-boundary-v1";
const SCALAR_BRIDGE_SCHEMA: &str = "jet-ffi-scalar-sidecar-v1";

/// One scalar function in the common checked sidecar ABI. Language adapters
/// parse their own declaration format into this shape, then share the C
/// boundary, Jet wrapper, and provenance rules below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarBridgeFunction {
    pub name: String,
    pub params: Vec<ForeignScalar>,
    pub result: ForeignScalar,
}

/// One exact file input in a bridge identity. The role is part of the record,
/// so a declaration, runtime, header, or generated worker cannot be mistaken
/// for another file merely because both happen to have the same bytes.
pub type BridgeSource<'a> = (&'a str, &'a Path);

/// A local native archive candidate used by a binding or link record.
/// `bytes == None` is deliberate evidence that the candidate was absent or
/// unreadable when the identity was made; it is not a permission to fall back
/// to a different host tool or guessed library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveInput {
    pub library: String,
    pub path: PathBuf,
    pub bytes: Option<Vec<u8>>,
}

/// A deterministic content-addressed identity record.
#[derive(Debug, Default)]
pub struct IdentityBuilder {
    bytes: Vec<u8>,
}

impl IdentityBuilder {
    /// Start an identity record with its schema as the first field.
    pub fn new(schema: &str) -> Self {
        let mut identity = Self::default();
        identity.field("schema", schema.as_bytes());
        identity
    }

    /// Add one ordered, length-delimited input field.
    pub fn field(&mut self, name: &str, value: &[u8]) {
        self.bytes
            .extend_from_slice(&(name.len() as u64).to_le_bytes());
        self.bytes.extend_from_slice(name.as_bytes());
        self.bytes
            .extend_from_slice(&(value.len() as u64).to_le_bytes());
        self.bytes.extend_from_slice(value);
    }

    /// Finish the record as a lowercase SHA-256 digest.
    pub fn finish(self) -> String {
        crate::SHA256::sha256_hex(&self.bytes)
    }
}
/// One source/build/run/target identity for a foreign boundary.
///
/// The identity deliberately names the declaration source, checked overlay,
/// generator, foreign implementation, toolchain, and target separately. A
/// path or a version by itself is not enough to keep a cached replacement
/// honest when the bytes behind it change.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForeignBoundaryIdentity {
    pub source: String,
    pub overlay: String,
    pub generator: String,
    pub implementation: String,
    pub toolchain: String,
    pub target: String,
}

impl ForeignBoundaryIdentity {
    pub fn new(
        source: impl Into<String>,
        overlay: impl Into<String>,
        generator: impl Into<String>,
        implementation: impl Into<String>,
        toolchain: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            overlay: overlay.into(),
            generator: generator.into(),
            implementation: implementation.into(),
            toolchain: toolchain.into(),
            target: target.into(),
        }
    }

    /// Map the boundary identity onto the one canonical derivation identity.
    /// Overlay and generator changes are source/build changes respectively;
    /// the foreign implementation remains the recorded run input.
    pub fn derivation_identity(&self) -> jet_foundation::Facts::DerivationIdentity {
        jet_foundation::Facts::DerivationIdentity::new(
            format!("source={};overlay={}", self.source, self.overlay),
            format!("generator={};toolchain={}", self.generator, self.toolchain),
            self.implementation.clone(),
            self.target.clone(),
        )
    }

    pub fn differs_from(&self, other: &Self) -> bool {
        self != other
    }

    /// Stable digest used by receipts and generated source comments.
    pub fn digest(&self) -> String {
        let mut identity = IdentityBuilder::new(FOREIGN_BOUNDARY_SCHEMA);
        for (name, value) in [
            ("source", self.source.as_str()),
            ("overlay", self.overlay.as_str()),
            ("generator", self.generator.as_str()),
            ("implementation", self.implementation.as_str()),
            ("toolchain", self.toolchain.as_str()),
            ("target", self.target.as_str()),
        ] {
            identity.field(name, value.as_bytes());
        }
        identity.finish()
    }

    fn is_complete(&self) -> bool {
        [
            &self.source,
            &self.overlay,
            &self.generator,
            &self.implementation,
            &self.toolchain,
            &self.target,
        ]
        .iter()
        .all(|value| !value.trim().is_empty())
    }
}

/// Evidence basis for one foreign-call obligation. The enum is defined in
/// Foundation so sema, TIR, and execution tiers consume the same vocabulary.
pub use jet_foundation::AST::FfiEvidenceBasis as ForeignEvidenceBasis;

/// Exact loaded-artifact coverage shared by every obligation in a boundary.
///
/// `None` means that the producer did not establish the corresponding list.
/// `Some(empty)` is an established empty list, so missing callback/dependency
/// analysis cannot be mistaken for "there were no callbacks/dependencies".
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForeignArtifactCoverage {
    pub loaded_artifact: String,
    pub transitive_dependencies: Option<Vec<String>>,
    pub reachable_callbacks: Option<Vec<String>>,
    pub compiler_flags: Option<Vec<String>>,
    pub target: String,
    pub generator: String,
}

impl ForeignArtifactCoverage {
    pub fn new(
        loaded_artifact: impl Into<String>,
        target: impl Into<String>,
        generator: impl Into<String>,
    ) -> Self {
        Self {
            loaded_artifact: loaded_artifact.into(),
            transitive_dependencies: None,
            reachable_callbacks: None,
            compiler_flags: None,
            target: target.into(),
            generator: generator.into(),
        }
    }

    pub fn with_transitive_dependencies<I, S>(mut self, dependencies: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.transitive_dependencies = Some(sorted_strings(dependencies));
        self
    }

    pub fn with_reachable_callbacks<I, S>(mut self, callbacks: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.reachable_callbacks = Some(sorted_strings(callbacks));
        self
    }

    pub fn with_compiler_flags<I, S>(mut self, flags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.compiler_flags = Some(sorted_strings(flags));
        self
    }

    pub fn digest(&self) -> String {
        let mut identity = IdentityBuilder::new(FOREIGN_BOUNDARY_SCHEMA);
        identity.field("loaded-artifact", self.loaded_artifact.as_bytes());
        identity.field(
            "transitive-dependencies",
            stable_optional_strings(self.transitive_dependencies.as_deref()).as_bytes(),
        );
        identity.field(
            "reachable-callbacks",
            stable_optional_strings(self.reachable_callbacks.as_deref()).as_bytes(),
        );
        identity.field(
            "compiler-flags",
            stable_optional_strings(self.compiler_flags.as_deref()).as_bytes(),
        );
        identity.field("target", self.target.as_bytes());
        identity.field("generator", self.generator.as_bytes());
        identity.finish()
    }

    fn is_complete(&self) -> bool {
        !self.loaded_artifact.trim().is_empty()
            && self.transitive_dependencies.is_some()
            && self.reachable_callbacks.is_some()
            && self.compiler_flags.is_some()
            && !self.target.trim().is_empty()
            && !self.generator.trim().is_empty()
    }
}

/// One obligation row in the canonical foreign boundary contract.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForeignObligationEvidence {
    pub obligation: String,
    pub basis: ForeignEvidenceBasis,
    pub checker: String,
    pub assumptions: Vec<String>,
    pub coverage_digest: String,
}

impl ForeignObligationEvidence {
    pub fn new(
        obligation: impl Into<String>,
        basis: ForeignEvidenceBasis,
        checker: impl Into<String>,
        assumptions: impl IntoIterator<Item = String>,
        coverage: &ForeignArtifactCoverage,
    ) -> Self {
        Self {
            obligation: obligation.into(),
            basis,
            checker: checker.into(),
            assumptions: sorted_strings(assumptions),
            coverage_digest: coverage.digest(),
        }
    }

    fn validate(&self, coverage: &ForeignArtifactCoverage) -> Result<(), String> {
        if self.obligation.trim().is_empty() {
            return Err("foreign obligation evidence needs an obligation name".into());
        }
        if self.checker.trim().is_empty() {
            return Err(format!(
                "foreign obligation `{}` needs an evidence checker",
                self.obligation
            ));
        }
        if self.assumptions.is_empty() {
            return Err(format!(
                "foreign obligation `{}` needs explicit assumptions",
                self.obligation
            ));
        }
        if self.coverage_digest != coverage.digest() {
            return Err(format!(
                "foreign obligation `{}` is attached to stale artifact coverage",
                self.obligation
            ));
        }
        if self.basis != ForeignEvidenceBasis::Unknown && !coverage.is_complete() {
            return Err(format!(
                "foreign obligation `{}` cannot claim {} without complete artifact coverage",
                self.obligation,
                self.basis.as_str()
            ));
        }
        Ok(())
    }
}

/// The obligation names every foreign descriptor must expose.
pub const FOREIGN_BOUNDARY_OBLIGATIONS: &[&str] = &[
    "abi",
    "layout",
    "width",
    "alignment",
    "ownership",
    "lifetime",
    "cleanup",
    "encoding",
    "nullability",
    "errors",
    "exceptions",
    "callbacks",
    "task-thread",
    "target",
    "copy-cost",
    "effects",
];

fn sorted_strings<I, S>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut values = values
        .into_iter()
        .map(Into::into)
        .filter(|value: &String| !value.trim().is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn stable_optional_strings(values: Option<&[String]>) -> String {
    match values {
        None => "unknown".into(),
        Some(values) if values.is_empty() => "none".into(),
        Some(values) => values.join("\n"),
    }
}

fn provenance_optional_strings(values: Option<&[String]>) -> String {
    match values {
        None => "unknown".into(),
        Some(values) if values.is_empty() => "none".into(),
        Some(values) => values.join("|"),
    }
}

/// Whether a foreign family has a checked replacement path or only a binder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ForeignBoundaryDisposition {
    Supported,
    BinderOnly,
    Unsupported,
}

impl ForeignBoundaryDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::BinderOnly => "binder-only",
            Self::Unsupported => "unsupported",
        }
    }
}

/// The source remains authoritative until a recorded comparison accepts the
/// replacement. This is a state marker, not a second evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ForeignSourceAuthority {
    ForeignUntilAccepted,
    ReplacementAccepted,
}

impl ForeignSourceAuthority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ForeignUntilAccepted => "foreign-until-accepted",
            Self::ReplacementAccepted => "replacement-accepted",
        }
    }
}

/// One raw case from the existing comparison/evidence machinery.
///
/// The boundary keeps the input and both raw observations, but does not run a
/// relation or create an evidence graph of its own. `outcome` uses the
/// comparison API's vocabulary (`matched`, `mismatch`, `empty`, `unsupported`,
/// or `unknown`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForeignBoundaryObservation {
    pub case_id: String,
    pub input: String,
    pub foreign: String,
    pub replacement: String,
    pub outcome: String,
    pub assumptions: Vec<String>,
    pub evidence: Option<jet_foundation::Evidence::EvidenceIdentity>,
}

impl ForeignBoundaryObservation {
    pub fn new(
        case_id: impl Into<String>,
        input: impl Into<String>,
        foreign: impl Into<String>,
        replacement: impl Into<String>,
        outcome: impl Into<String>,
    ) -> Self {
        Self {
            case_id: case_id.into(),
            input: input.into(),
            foreign: foreign.into(),
            replacement: replacement.into(),
            outcome: outcome.into(),
            assumptions: Vec::new(),
            evidence: None,
        }
    }

    pub fn with_assumptions(mut self, assumptions: impl IntoIterator<Item = String>) -> Self {
        self.assumptions = assumptions.into_iter().collect();
        self.assumptions.sort();
        self.assumptions.dedup();
        self
    }

    pub fn with_evidence(
        mut self,
        evidence: jet_foundation::Evidence::EvidenceIdentity,
    ) -> Self {
        self.evidence = Some(evidence);
        self
    }

    fn validate(&self) -> Result<(), String> {
        if self.case_id.trim().is_empty() || self.input.trim().is_empty() {
            return Err("foreign boundary comparison cases need an id and input".into());
        }
        if self.foreign.trim().is_empty() || self.replacement.trim().is_empty() {
            return Err(
                "foreign boundary comparison cases retain both raw foreign and replacement observations"
                    .into(),
            );
        }
        if !matches!(
            self.outcome.as_str(),
            "matched"
                | "mismatch"
                | "empty"
                | "unsupported"
                | "unavailable"
                | "timeout"
                | "cancelled"
                | "invalid_oracle"
                | "contaminated"
                | "unknown"
        ) {
            return Err(format!(
                "unknown foreign boundary comparison outcome `{}`",
                self.outcome
            ));
        }
        Ok(())
    }
}

/// Family-level capability projection over the canonical binder descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForeignBoundaryCapability {
    pub language: crate::AST::ForeignLanguage,
    pub disposition: ForeignBoundaryDisposition,
    pub source_import: bool,
    pub mixed_debug: bool,
    pub native_export: bool,
    pub build_host: bool,
    pub reason: &'static str,
}

/// Project the consumed family matrix without looking at command presence.
///
/// Active descriptors provide an executable binder. Only the named narrow
/// conversion families claim source import; C/C++ source import and
/// COM/Ada/Pascal semantic translation remain explicitly unavailable.
pub fn foreign_boundary_capability(
    language: crate::AST::ForeignLanguage,
) -> ForeignBoundaryCapability {
    let descriptor = crate::AST::binder_descriptor(language);
    let active = descriptor.is_some_and(|descriptor| {
        descriptor.status == crate::AST::BinderStatus::Active
    });
    let source_import = matches!(
        language,
        crate::AST::ForeignLanguage::Py
            | crate::AST::ForeignLanguage::Java
            | crate::AST::ForeignLanguage::DotNet
            | crate::AST::ForeignLanguage::JS
            | crate::AST::ForeignLanguage::Go
    );
    let binder_only = matches!(
        language,
        crate::AST::ForeignLanguage::Com
            | crate::AST::ForeignLanguage::Ada
            | crate::AST::ForeignLanguage::Pascal
    );
    let disposition = if !active {
        ForeignBoundaryDisposition::Unsupported
    } else if binder_only {
        ForeignBoundaryDisposition::BinderOnly
    } else {
        ForeignBoundaryDisposition::Supported
    };
    let reason = match disposition {
        ForeignBoundaryDisposition::Supported if source_import => {
            "active adapter with a narrow checked conversion subset"
        }
        ForeignBoundaryDisposition::Supported => {
            "active checked binder; semantic source import is unavailable"
        }
        ForeignBoundaryDisposition::BinderOnly => {
            "binder/adapter path is executable; semantic source translation is unavailable"
        }
        ForeignBoundaryDisposition::Unsupported => {
            "no active binder descriptor is registered for this family"
        }
    };
    ForeignBoundaryCapability {
        language,
        disposition,
        source_import,
        mixed_debug: matches!(
            language,
            crate::AST::ForeignLanguage::C
                | crate::AST::ForeignLanguage::Cpp
                | crate::AST::ForeignLanguage::Py
                | crate::AST::ForeignLanguage::Java
                | crate::AST::ForeignLanguage::DotNet
                | crate::AST::ForeignLanguage::JS
                | crate::AST::ForeignLanguage::Go
        ),
        native_export: matches!(
            language,
            crate::AST::ForeignLanguage::C | crate::AST::ForeignLanguage::Cpp
        ),
        build_host: matches!(
            language,
            crate::AST::ForeignLanguage::C | crate::AST::ForeignLanguage::Cpp
        ),
        reason,
    }
}

/// Return the complete family matrix used by foreign inspection and
/// comparison views. Every language gets one explicit executable,
/// binder-only, or unsupported disposition.
pub fn foreign_boundary_capabilities() -> Vec<ForeignBoundaryCapability> {
    crate::AST::ForeignLanguage::ALL
        .iter()
        .copied()
        .map(foreign_boundary_capability)
        .collect()
}


/// Build the canonical boundary row for an import operation.
///
/// Source conversion and binder-only preparation share the descriptor,
/// identity, coverage, and provenance path. The capability matrix remains the
/// authority for whether a family has semantic conversion; this constructor
/// deliberately does not upgrade binder-only families.
pub fn source_import_boundary(
    language: crate::AST::ForeignLanguage,
    library: impl Into<String>,
    identity: ForeignBoundaryIdentity,
    coverage: ForeignArtifactCoverage,
) -> Result<ForeignBoundaryContract, String> {
    let descriptor = crate::AST::binder_descriptor(language)
        .ok_or_else(|| format!("missing binder descriptor for {language:?}"))?;
    let boundary = ForeignBoundaryContract::new(*descriptor, library, identity)
        .with_artifact_coverage(coverage);
    boundary.validate()?;
    Ok(boundary)
}

/// Stable receipt emitted when a checked comparison accepts a replacement
/// projection for a foreign source. The receipt references the existing
/// derivation/evidence/comparison records; it does not copy or re-evaluate
/// their graphs.
pub const FOREIGN_SOURCE_RECEIPT_SCHEMA: &str = "jet-ffi-source-receipt-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignSourceReceipt {
    pub schema: &'static str,
    pub boundary_digest: String,
    pub boundary_identity: ForeignBoundaryIdentity,
    pub artifact_coverage_digest: String,
    pub source_identity: String,
    pub replacement_identity: String,
    pub source_authority: ForeignSourceAuthority,
    pub derivation: jet_foundation::Facts::DerivationRef,
    pub evidence: Vec<jet_foundation::Evidence::EvidenceIdentity>,
    pub comparison_artifact_id: String,
    pub comparison_schema_version: u16,
    pub comparison_status: String,
    pub comparison_relation: String,
    pub compared_samples: usize,
    pub discarded_cases: usize,
    pub first_difference: Option<usize>,
    pub comparison_reason: Option<String>,
}

impl ForeignSourceReceipt {
    fn from_contract(
        contract: &ForeignBoundaryContract,
        comparison: &jet_foundation::TestingComparison::ComparisonRecord,
        replacement_identity: String,
    ) -> Result<Self, String> {
        let derivation = contract
            .derivation
            .clone()
            .ok_or_else(|| "accepted foreign replacement is missing its derivation reference".to_string())?;
        let comparison_artifact_id = comparison.artifact_id()?;
        let receipt = Self {
            schema: FOREIGN_SOURCE_RECEIPT_SCHEMA,
            boundary_digest: contract.digest(),
            boundary_identity: contract.identity.clone(),
            artifact_coverage_digest: contract.artifact_coverage.digest(),
            source_identity: contract.identity.source.clone(),
            replacement_identity,
            source_authority: contract.source_authority,
            derivation,
            evidence: contract.evidence.clone(),
            comparison_artifact_id,
            comparison_schema_version: comparison.schema_version,
            comparison_status: comparison.status.as_str().to_string(),
            comparison_relation: comparison.relation.as_str().to_string(),
            compared_samples: comparison.samples.len(),
            discarded_cases: comparison.discarded_cases,
            first_difference: comparison.first_difference,
            comparison_reason: comparison.reason.clone(),
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != FOREIGN_SOURCE_RECEIPT_SCHEMA {
            return Err(format!("unsupported foreign source receipt schema `{}`", self.schema));
        }
        if self.boundary_digest.trim().is_empty()
            || !self.boundary_identity.is_complete()
            || self.artifact_coverage_digest.trim().is_empty()
            || self.source_identity.trim().is_empty()
            || self.replacement_identity.trim().is_empty()
            || self.comparison_artifact_id.trim().is_empty()
            || self.comparison_artifact_id.len() != 64
            || !self
                .comparison_artifact_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("foreign source receipt has incomplete identity or coverage".into());
        }
        if self.source_authority != ForeignSourceAuthority::ReplacementAccepted {
            return Err(
                "foreign source receipt requires replacement-accepted source authority".into(),
            );
        }
        if self.derivation.id.trim().is_empty() || self.evidence.is_empty() {
            return Err(
                "foreign source receipt requires checked derivation and evidence references".into(),
            );
        }
        if self.comparison_schema_version
            != jet_foundation::TestingComparison::TEST_COMPARISON_SCHEMA_VERSION
            || self.comparison_status != "matched"
            || self.comparison_relation.trim().is_empty()
            || self.compared_samples == 0
            || self.discarded_cases != 0
            || self.first_difference.is_some()
        {
            return Err(
                "foreign source receipt requires a non-empty matched comparison without discarded cases"
                    .into(),
            );
        }
        Ok(())
    }

    /// Queryable fields for the existing provenance/receipt channels.
    pub fn provenance_fields(&self) -> Vec<(String, String)> {
        let mut fields = vec![
            ("source-receipt-schema".into(), self.schema.into()),
            ("source-receipt-boundary".into(), self.boundary_digest.clone()),
            (
                "source-receipt-coverage".into(),
                self.artifact_coverage_digest.clone(),
            ),
            (
                "source-receipt-authority".into(),
                self.source_authority.as_str().into(),
            ),
            (
                "source-receipt-source".into(),
                self.source_identity.clone(),
            ),
            (
                "source-receipt-replacement".into(),
                self.replacement_identity.clone(),
            ),
            (
                "source-receipt-derivation".into(),
                self.derivation.id.clone(),
            ),
            (
                "source-receipt-comparison-artifact".into(),
                self.comparison_artifact_id.clone(),
            ),
            (
                "source-receipt-comparison-schema".into(),
                self.comparison_schema_version.to_string(),
            ),
            (
                "source-receipt-comparison-status".into(),
                self.comparison_status.clone(),
            ),
            (
                "source-receipt-comparison-relation".into(),
                self.comparison_relation.clone(),
            ),
            (
                "source-receipt-compared-samples".into(),
                self.compared_samples.to_string(),
            ),
            (
                "source-receipt-discarded-cases".into(),
                self.discarded_cases.to_string(),
            ),
        ];
        for evidence in &self.evidence {
            fields.push((
                "source-receipt-evidence".into(),
                format!("{}/{}/{}", evidence.report_id, evidence.claim_id, evidence.evidence_id),
            ));
        }
        if let Some(reason) = &self.comparison_reason {
            fields.push(("source-receipt-comparison-reason".into(), reason.clone()));
        }
        fields
    }

    /// Deterministic text form suitable for the existing receipt store.
    pub fn render(&self) -> Result<String, String> {
        self.validate()?;
        let mut output = String::new();
        for (name, value) in self.provenance_fields() {
            append_line(&mut output, &name, &value)?;
        }
        output.push_str(&format!(
            "source-receipt-boundary-source={}\nsource-receipt-boundary-overlay={}\nsource-receipt-boundary-generator={}\nsource-receipt-boundary-implementation={}\nsource-receipt-boundary-toolchain={}\nsource-receipt-boundary-target={}\n",
            self.boundary_identity.source,
            self.boundary_identity.overlay,
            self.boundary_identity.generator,
            self.boundary_identity.implementation,
            self.boundary_identity.toolchain,
            self.boundary_identity.target,
        ));
        Ok(output)
    }
}

/// The one consumed boundary row for a binder invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignBoundaryContract {
    pub schema: &'static str,
    pub language: crate::AST::ForeignLanguage,
    pub library: String,
    pub descriptor: String,
    pub abi: ForeignAbiContract,
    pub scalar_widths: BTreeMap<String, String>,
    pub alignment: String,
    pub ownership: String,
    pub cleanup: String,
    pub encoding: String,
    pub nullability: String,
    pub errors: String,
    pub exceptions: String,
    pub callbacks: String,
    pub task_thread: String,
    pub target_availability: String,
    pub copy_cost: String,
    /// The descriptor-owned effect root. It is never narrowed from a foreign
    /// effect to purity by an evidence row.
    pub effects: String,
    pub effect_root: String,
    /// Exact loaded artifact and reachable native coverage shared by all rows.
    pub artifact_coverage: ForeignArtifactCoverage,
    pub identity: ForeignBoundaryIdentity,
    pub disposition: ForeignBoundaryDisposition,
    pub source_import: bool,
    pub source_authority: ForeignSourceAuthority,
    /// Receipt of the checked comparison that made a replacement selectable.
    /// It is a projection over the canonical reasoning records, not a second
    /// facts/evidence graph.
    pub source_receipt: Option<ForeignSourceReceipt>,
    pub derivation: Option<jet_foundation::Facts::DerivationRef>,
    pub evidence: Vec<jet_foundation::Evidence::EvidenceIdentity>,
    pub assumptions: Vec<String>,
    pub observations: Vec<ForeignBoundaryObservation>,
    /// One evidence row for every canonical ABI/safety/effect obligation.
    pub obligations: Vec<ForeignObligationEvidence>,
}

impl ForeignBoundaryContract {
    pub fn new(
        descriptor: BinderDescriptor,
        library: impl Into<String>,
        identity: ForeignBoundaryIdentity,
    ) -> Self {
        let capability = foreign_boundary_capability(descriptor.language);
        let mut scalar_widths = BTreeMap::new();
        for (name, scalar) in [
            ("integer", descriptor.contract.integer),
            ("floating", descriptor.contract.floating),
            ("boolean", descriptor.contract.boolean),
            ("character", descriptor.contract.character),
            ("string", descriptor.contract.string),
        ] {
            scalar_widths.insert(name.to_string(), scalar_width(scalar).to_string());
        }
        let is_cpp = descriptor.language == crate::AST::ForeignLanguage::Cpp;
        let is_c = descriptor.language == crate::AST::ForeignLanguage::C;
        let artifact_coverage = ForeignArtifactCoverage::new(
            "not-recorded",
            identity.target.clone(),
            identity.generator.clone(),
        );
        let coverage_digest = artifact_coverage.digest();
        let obligations = FOREIGN_BOUNDARY_OBLIGATIONS
            .iter()
            .map(|obligation| ForeignObligationEvidence {
                obligation: (*obligation).into(),
                basis: ForeignEvidenceBasis::Unknown,
                checker: "not-established".into(),
                assumptions: vec![
                    "descriptor declarations do not prove the foreign implementation".into(),
                ],
                coverage_digest: coverage_digest.clone(),
            })
            .collect();
        Self {
            schema: FOREIGN_BOUNDARY_SCHEMA,
            language: descriptor.language,
            library: library.into(),
            descriptor: descriptor.stamp(),
            abi: descriptor.contract,
            scalar_widths,
            alignment: if is_cpp {
                "opaque-handle; native class layout intentionally unclaimed".into()
            } else if is_c {
                "target-declared C ABI; aggregate alignment requires #Layout(c)".into()
            } else {
                "adapter-declared; no native aggregate layout is claimed".into()
            },
            ownership: if is_cpp {
                "opaque-owned-handle".into()
            } else {
                format!("{:?}", descriptor.contract.ownership)
            },
            cleanup: if is_cpp {
                "consuming close removes the handle slot before delete".into()
            } else {
                "descriptor/overlay close contract".into()
            },
            encoding: if is_cpp {
                "scalar subset; no implicit text conversion".into()
            } else {
                "descriptor-declared encoding".into()
            },
            nullability: if is_cpp {
                "null handle is invalid; no nullable scalar claim".into()
            } else {
                "descriptor-declared; unknown remains unsupported".into()
            },
            errors: if is_cpp {
                "Jet checked result plus take_error status".into()
            } else {
                format!("{:?}", descriptor.contract.errors)
            },
            exceptions: if is_cpp {
                "caught at the C++ shim; mapped to Exception".into()
            } else {
                "foreign exceptions are not translated implicitly".into()
            },
            callbacks: if is_cpp {
                "C ABI callback pointer; captured state is rejected".into()
            } else {
                format!("{:?}", descriptor.contract.callbacks)
            },
            task_thread: format!(
                "async={:?};task-boundary={:?};thread-crossing=descriptor-declared",
                descriptor.contract.async_completion, descriptor.contract.task_boundary
            ),
            target_availability: format!(
                "target={}; source/tool/overlay identities are exact",
                identity.target
            ),
            copy_cost: if is_cpp {
                "scalar by-value; handles are slot lookups, never native object copies".into()
            } else {
                "adapter/binder-declared conversion cost".into()
            },
            effects: format!("{};foreign-unverified", descriptor.effect_root),
            effect_root: descriptor.effect_root.into(),
            artifact_coverage,
            identity,
            disposition: capability.disposition,
            source_import: capability.source_import,
            source_authority: ForeignSourceAuthority::ForeignUntilAccepted,
            source_receipt: None,
            derivation: None,
            evidence: Vec::new(),
            assumptions: Vec::new(),
            observations: Vec::new(),
            obligations,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != FOREIGN_BOUNDARY_SCHEMA {
            return Err(format!("unsupported foreign boundary schema `{}`", self.schema));
        }
        if self.library.trim().is_empty() || self.descriptor.trim().is_empty() {
            return Err("foreign boundary requires a library and descriptor".into());
        }
        if !self.identity.is_complete() {
            return Err("foreign boundary identity is incomplete".into());
        }
        if self.effect_root.trim().is_empty()
            || !self
                .effects
                .split(';')
                .any(|effect| effect == self.effect_root)
        {
            return Err("foreign effects cannot be narrowed below the descriptor effect root".into());
        }
        if self.obligations.len() < FOREIGN_BOUNDARY_OBLIGATIONS.len() {
            return Err("foreign boundary is missing canonical obligation rows".into());
        }
        let mut obligation_names = BTreeSet::new();
        for obligation in &self.obligations {
            obligation.validate(&self.artifact_coverage)?;
            if !obligation_names.insert(obligation.obligation.as_str()) {
                return Err(format!(
                    "foreign boundary repeats obligation `{}`",
                    obligation.obligation
                ));
            }
        }
        if FOREIGN_BOUNDARY_OBLIGATIONS
            .iter()
            .any(|name| !obligation_names.contains(name))
        {
            return Err("foreign boundary omitted a canonical obligation row".into());
        }

        if !self.evidence.is_empty() && self.derivation.is_none() {
            return Err("foreign boundary evidence requires a checked derivation reference".into());
        }
        if !self.source_import
            && self.source_authority == ForeignSourceAuthority::ReplacementAccepted
        {
            return Err(format!(
                "foreign source replacement is unavailable for {}",
                self.language.root()
            ));
        }
        match (&self.source_authority, &self.source_receipt) {
            (ForeignSourceAuthority::ReplacementAccepted, Some(receipt)) => {
                receipt.validate()?;
                if receipt.boundary_digest != self.digest()
                    || receipt.boundary_identity != self.identity
                    || receipt.artifact_coverage_digest != self.artifact_coverage.digest()
                    || receipt.source_authority != self.source_authority
                {
                    return Err(
                        "foreign source receipt does not match its accepted boundary".into(),
                    );
                }
            }
            (ForeignSourceAuthority::ReplacementAccepted, None) => {
                return Err("accepted foreign replacement is missing its source receipt".into());
            }
            (ForeignSourceAuthority::ForeignUntilAccepted, Some(_)) => {
                return Err("foreign source receipt requires replacement acceptance".into());
            }
            (ForeignSourceAuthority::ForeignUntilAccepted, None) => {}
        }
        for observation in &self.observations {
            observation.validate()?;
        }
        if self.source_authority == ForeignSourceAuthority::ReplacementAccepted
            && (self.observations.is_empty()
                || self
                    .observations
                    .iter()
                    .any(|observation| observation.outcome != "matched"))
        {
            return Err(
                "a foreign replacement requires non-empty matched comparison observations".into(),
            );
        }
        Ok(())
    }

    pub fn with_assumptions(mut self, assumptions: impl IntoIterator<Item = String>) -> Self {
        self.assumptions = assumptions.into_iter().collect();
        self.assumptions.sort();
        self.assumptions.dedup();
        self
    }

    /// Attach exact artifact coverage and retarget every existing obligation
    /// row to the same coverage digest.
    pub fn with_artifact_coverage(mut self, coverage: ForeignArtifactCoverage) -> Self {
        let digest = coverage.digest();
        self.artifact_coverage = coverage;
        for obligation in &mut self.obligations {
            obligation.coverage_digest = digest.clone();
        }
        self
    }

    /// Replace one obligation row without creating a parallel evidence store.
    pub fn record_obligation(
        &mut self,
        obligation: ForeignObligationEvidence,
    ) -> Result<(), String> {
        obligation.validate(&self.artifact_coverage)?;
        if let Some(existing) = self
            .obligations
            .iter_mut()
            .find(|existing| existing.obligation == obligation.obligation)
        {
            *existing = obligation;
        } else {
            self.obligations.push(obligation);
        }
        self.obligations
            .sort_by(|left, right| left.obligation.cmp(&right.obligation));
        Ok(())
    }

    /// Record one basis/checker/assumption row against this contract's exact
    /// loaded artifact coverage.
    pub fn set_obligation(
        &mut self,
        name: impl Into<String>,
        basis: ForeignEvidenceBasis,
        checker: impl Into<String>,
        assumptions: impl IntoIterator<Item = String>,
    ) -> Result<(), String> {
        self.record_obligation(ForeignObligationEvidence::new(
            name,
            basis,
            checker,
            assumptions,
            &self.artifact_coverage,
        ))
    }

    pub fn obligation(&self, name: &str) -> Option<&ForeignObligationEvidence> {
        self.obligations
            .iter()
            .find(|obligation| obligation.obligation == name)
    }

    /// Safe/user-visible projections require a non-unknown basis and complete
    /// loaded-artifact coverage. TRUSTED remains callable only outside that
    /// guarantee path.
    pub fn permits_guarantee(&self, name: &str) -> bool {
        self.artifact_coverage.is_complete()
            && self
                .obligation(name)
                .is_some_and(|obligation| obligation.basis.permits_guarantee())
    }

    pub fn has_unknown_obligations(&self) -> bool {
        self.obligations
            .iter()
            .any(|obligation| obligation.basis == ForeignEvidenceBasis::Unknown)
    }

    pub fn has_complete_artifact_coverage(&self) -> bool {
        self.artifact_coverage.is_complete()
    }

    /// Project the canonical row into the lower-layer carrier used by sema,
    /// TIR, and execution tiers. The carrier has no independent semantics.
    pub fn carrier_facts(&self) -> jet_foundation::AST::FfiBoundaryFacts {
        jet_foundation::AST::FfiBoundaryFacts {
            schema: self.schema.to_string(),
            digest: self.digest(),
            language: self.language.root().to_string(),
            library: self.library.clone(),
            effect_root: self.effect_root.clone(),
            effects: self.effects.clone(),
            source_authority: self.source_authority.as_str().to_string(),
            artifact_coverage_digest: self.artifact_coverage.digest(),
            loaded_artifact: self.artifact_coverage.loaded_artifact.clone(),
            transitive_dependencies: self.artifact_coverage.transitive_dependencies.clone(),
            reachable_callbacks: self.artifact_coverage.reachable_callbacks.clone(),
            compiler_flags: self.artifact_coverage.compiler_flags.clone(),
            target: self.artifact_coverage.target.clone(),
            generator: self.artifact_coverage.generator.clone(),
            obligations: self
                .obligations
                .iter()
                .map(|obligation| jet_foundation::AST::FfiBoundaryObligation {
                    name: obligation.obligation.clone(),
                    basis: obligation.basis,
                    checker: obligation.checker.clone(),
                    assumptions: obligation.assumptions.clone(),
                    coverage_digest: obligation.coverage_digest.clone(),
                })
                .collect(),
        }
    }

    pub fn attach_derivation(
        mut self,
        record: &jet_foundation::Facts::DerivationRecord,
    ) -> Result<Self, String> {
        record.validate()?;
        if record.identity != self.identity.derivation_identity() {
            return Err("foreign boundary derivation identity does not match its source/build/run/target inputs".into());
        }
        self.derivation = Some(record.reference());
        Ok(self)
    }

    pub fn attach_evidence(
        mut self,
        record: &jet_foundation::Evidence::EvidenceRecord,
    ) -> Result<Self, String> {
        record
            .validate()
            .map_err(|error| format!("foreign boundary evidence is invalid: {error}"))?;
        let Some(derivation) = &record.derivation else {
            return Err("foreign boundary evidence must reference a checked derivation".into());
        };
        if self.derivation.as_ref() != Some(derivation) {
            return Err("foreign boundary evidence and derivation references disagree".into());
        }
        if !self.evidence.contains(&record.identity) {
            self.evidence.push(record.identity.clone());
            self.evidence.sort();
        }
        Ok(self)
    }

    /// Consume the existing checked derivation, evidence, and comparison
    /// records before allowing a replacement projection to become selectable.
    /// The returned receipt is the only new record; it references the existing
    /// canonical records instead of building a second reasoning graph.
    pub fn accept_reasoned_replacement(
        &mut self,
        current_identity: &ForeignBoundaryIdentity,
        current_coverage: &ForeignArtifactCoverage,
        derivation: &jet_foundation::Facts::DerivationRecord,
        evidence: &jet_foundation::Evidence::EvidenceRecord,
        comparison: &jet_foundation::TestingComparison::ComparisonRecord,
        replacement_identity: impl Into<String>,
    ) -> Result<ForeignSourceReceipt, String> {
        self.validate()?;
        if self.replacement_accepted() {
            return Err("foreign replacement is already accepted for this boundary".into());
        }
        if self.stale_against(current_identity) {
            return Err(
                "foreign replacement reasoning is stale against the current boundary identity"
                    .into(),
            );
        }
        if self.coverage_stale_against(current_coverage) {
            return Err(
                "foreign replacement reasoning is stale against current artifact coverage".into(),
            );
        }
        let replacement_identity = replacement_identity.into();
        if replacement_identity.trim().is_empty() {
            return Err("foreign replacement reasoning requires a replacement identity".into());
        }
        if evidence.outcome.is_failure()
            || evidence.outcome.is_incomplete()
            || matches!(
                evidence.outcome,
                jet_foundation::Evidence::EvidenceOutcome::Unchecked
            )
        {
            return Err(format!(
                "foreign replacement evidence is not a complete successful observation: {}",
                evidence.outcome.as_str()
            ));
        }
        comparison
            .assert_equal()
            .map_err(|error| format!("foreign replacement comparison was not accepted: {error}"))?;
        let mut candidate = self.clone();
        candidate = candidate.attach_derivation(derivation)?;
        candidate = candidate.attach_evidence(evidence)?;
        candidate.record_comparison(comparison, Some(evidence.identity.clone()))?;
        candidate.mark_replacement_accepted()?;
        let receipt =
            ForeignSourceReceipt::from_contract(&candidate, comparison, replacement_identity)?;
        candidate.source_receipt = Some(receipt.clone());
        candidate.validate()?;
        *self = candidate;
        Ok(receipt)
    }

    pub fn record_observation(
        &mut self,
        observation: ForeignBoundaryObservation,
    ) -> Result<(), String> {
        observation.validate()?;
        self.observations.push(observation);
        Ok(())
    }

    /// Consume one canonical comparison record without creating another
    /// evaluator or evidence graph. Every sampled case keeps both raw sides;
    /// terminal outcomes retain their exact status/reason as assumptions
    /// because no raw observation exists to copy.
    pub fn record_comparison(
        &mut self,
        record: &jet_foundation::TestingComparison::ComparisonRecord,
        evidence: Option<jet_foundation::Evidence::EvidenceIdentity>,
    ) -> Result<(), String> {
        if record.schema_version
            != jet_foundation::TestingComparison::TEST_COMPARISON_SCHEMA_VERSION
        {
            return Err(format!(
                "unsupported comparison schema version {}",
                record.schema_version
            ));
        }
        if record.universal_proof {
            return Err("foreign boundary comparison cannot claim universal proof".into());
        }
        let outcome = record.status.as_str().to_string();
        let relation = record.relation.as_str().to_string();
        if record.samples.is_empty() {
            let reason = record
                .reason
                .as_deref()
                .unwrap_or("comparison produced no samples");
            self.assumptions
                .push(format!("comparison-terminal:{outcome}:{reason}"));
            self.assumptions
                .push(format!("comparison-relation:{relation}"));
            self.assumptions.sort();
            self.assumptions.dedup();
            return Ok(());
        }
        for sample in &record.samples {
            let mut assumptions = vec![
                format!("relation={relation}"),
                format!("source={}", sample.identity.source),
                format!("tool={}", sample.identity.tool),
                format!("target={}", sample.identity.target),
            ];
            if let Some(replay) = &sample.reference_replay {
                assumptions.push(format!("reference-replay={}", replay.raw));
            }
            if let Some(replay) = &sample.candidate_replay {
                assumptions.push(format!("replacement-replay={}", replay.raw));
            }
            let observation = ForeignBoundaryObservation::new(
                sample.identity.case_id.clone(),
                sample.identity.input_id.clone(),
                sample.reference.raw.clone(),
                sample.candidate.raw.clone(),
                outcome.clone(),
            )
            .with_assumptions(assumptions);
            let observation = match evidence.clone() {
                Some(evidence) => observation.with_evidence(evidence),
                None => observation,
            };
            self.record_observation(observation)?;
        }
        Ok(())
    }
    /// Accept a replacement only after the checked workflow has persisted its
    /// explicit source receipt. This prevents a bare matched row from creating
    /// an accepted-but-unreceipted boundary.
    pub fn accept_replacement(&mut self) -> Result<(), String> {
        if self.source_receipt.is_none() {
            return Err(
                "use accept_reasoned_replacement to attach the checked source receipt".into(),
            );
        }
        self.mark_replacement_accepted()
    }

    fn mark_replacement_accepted(&mut self) -> Result<(), String> {
        if !self.source_import {
            return Err(format!(
                "foreign source replacement is unavailable for {}",
                self.language.root()
            ));
        }
        if self.derivation.is_none() || self.evidence.is_empty() {
            return Err(
                "cannot accept a foreign replacement without checked derivation and evidence references"
                    .into(),
            );
        }
        if self.observations.is_empty()
            || self
                .observations
                .iter()
                .any(|observation| {
                    observation.outcome != "matched" || observation.evidence.is_none()
                })
        {
            return Err(
                "cannot accept a foreign replacement without all matched cases carrying evidence"
                    .into(),
            );
        }
        self.source_authority = ForeignSourceAuthority::ReplacementAccepted;
        Ok(())
    }

    pub fn stale_against(&self, current: &ForeignBoundaryIdentity) -> bool {
        self.identity.differs_from(current)
    }

    /// Stable digest for a complete boundary row. The raw observations remain
    /// available through `observations`; this digest only keys the row.
    pub fn digest(&self) -> String {
        let mut identity = IdentityBuilder::new(FOREIGN_BOUNDARY_SCHEMA);
        for (name, value) in self.stable_fields() {
            identity.field(name, value.as_bytes());
        }
        identity.finish()
    }

    pub fn coverage_stale_against(&self, current: &ForeignArtifactCoverage) -> bool {
        self.artifact_coverage.digest() != current.digest()
    }

    /// Whether a consumer may select the replacement projection. Unsupported,
    /// mismatched, stale, or merely sampled rows keep foreign source text
    /// authoritative.
    pub fn replacement_accepted(&self) -> bool {
        self.source_import && self.source_authority == ForeignSourceAuthority::ReplacementAccepted
    }

    /// Compact source-safe marker carried by generated bindings. The digest
    /// points back to the complete structured row in provenance.
    pub fn stamp(&self) -> String {
        format!(
            "{};language={};library={};disposition={};source-authority={};digest={}",
            self.schema,
            self.language.root(),
            self.library,
            self.disposition.as_str(),
            self.source_authority.as_str(),
            self.digest()
        )
    }

    /// Queryable fields merged into the existing bridge provenance record.
    pub fn provenance_fields(&self) -> Vec<(String, String)> {
        let mut fields = vec![
            ("boundary-schema".into(), self.schema.into()),
            ("boundary-digest".into(), self.digest()),
            ("boundary-language".into(), self.language.root().into()),
            ("boundary-library".into(), self.library.clone()),
            ("boundary-descriptor".into(), self.descriptor.clone()),
            (
                "boundary-disposition".into(),
                self.disposition.as_str().into(),
            ),
            (
                "boundary-source-import".into(),
                self.source_import.to_string(),
            ),
            (
                "boundary-source-authority".into(),
                self.source_authority.as_str().into(),
            ),
            ("boundary-alignment".into(), self.alignment.clone()),
            ("boundary-ownership".into(), self.ownership.clone()),
            ("boundary-cleanup".into(), self.cleanup.clone()),
            ("boundary-encoding".into(), self.encoding.clone()),
            ("boundary-nullability".into(), self.nullability.clone()),
            ("boundary-errors".into(), self.errors.clone()),
            ("boundary-exceptions".into(), self.exceptions.clone()),
            ("boundary-callbacks".into(), self.callbacks.clone()),
            ("boundary-task-thread".into(), self.task_thread.clone()),
            (
                "boundary-target-availability".into(),
                self.target_availability.clone(),
            ),
            ("boundary-copy-cost".into(), self.copy_cost.clone()),
            ("boundary-effects".into(), self.effects.clone()),
            ("boundary-effect-root".into(), self.effect_root.clone()),
            (
                "boundary-artifact-coverage".into(),
                self.artifact_coverage.digest(),
            ),
            (
                "boundary-loaded-artifact".into(),
                self.artifact_coverage.loaded_artifact.clone(),
            ),
            (
                "boundary-transitive-dependencies".into(),
                provenance_optional_strings(self.artifact_coverage.transitive_dependencies.as_deref()),
            ),
            (
                "boundary-reachable-callbacks".into(),
                provenance_optional_strings(self.artifact_coverage.reachable_callbacks.as_deref()),
            ),
            (
                "boundary-compiler-flags".into(),
                provenance_optional_strings(self.artifact_coverage.compiler_flags.as_deref()),
            ),
            (
                "boundary-coverage-target".into(),
                self.artifact_coverage.target.clone(),
            ),
            (
                "boundary-coverage-generator".into(),
                self.artifact_coverage.generator.clone(),
            ),
            ("boundary-overlay-identity".into(), self.identity.overlay.clone()),
            (
                "boundary-generator-identity".into(),
                self.identity.generator.clone(),
            ),
            (
                "boundary-implementation-identity".into(),
                self.identity.implementation.clone(),
            ),
            (
                "boundary-toolchain-identity".into(),
                self.identity.toolchain.clone(),
            ),
            ("boundary-target-identity".into(), self.identity.target.clone()),
        ];
        for (name, width) in &self.scalar_widths {
            fields.push((format!("boundary-width-{name}"), width.clone()));
        }
        if let Some(derivation) = &self.derivation {
            fields.push(("boundary-derivation".into(), derivation.id.clone()));
        }
        for evidence in &self.evidence {
            fields.push((
                "boundary-evidence".into(),
                format!(
                    "{}/{}/{}",
                    evidence.report_id, evidence.claim_id, evidence.evidence_id
                ),
            ));
        }
        for assumption in &self.assumptions {
            fields.push(("boundary-assumption".into(), assumption.clone()));
        }
        for obligation in &self.obligations {
            fields.push((
                "boundary-obligation".into(),
                format!(
                    "{}\t{}\t{}\t{}\t{}",
                    obligation.obligation,
                    obligation.basis.as_str(),
                    obligation.checker,
                    obligation.coverage_digest,
                    obligation.assumptions.join("|"),
                ),
            ));
        }
        for observation in &self.observations {
            fields.push((
                "boundary-comparison".into(),
                format!(
                    "{}\t{}\t{}\t{}\t{}",
                    observation.case_id,
                    observation.input,
                    observation.outcome,
                    observation.foreign,
                    observation.replacement
                ),
            ));
        }
        if let Some(receipt) = &self.source_receipt {
            fields.extend(receipt.provenance_fields());
        }
        fields
    }

    fn stable_fields(&self) -> Vec<(&str, String)> {
        let mut fields = vec![
            ("schema", self.schema.to_string()),
            ("language", self.language.root().to_string()),
            ("library", self.library.clone()),
            ("descriptor", self.descriptor.clone()),
            ("abi", self.abi.stamp()),
            ("alignment", self.alignment.clone()),
            ("ownership", self.ownership.clone()),
            ("cleanup", self.cleanup.clone()),
            ("encoding", self.encoding.clone()),
            ("nullability", self.nullability.clone()),
            ("errors", self.errors.clone()),
            ("exceptions", self.exceptions.clone()),
            ("callbacks", self.callbacks.clone()),
            ("task_thread", self.task_thread.clone()),
            ("target_availability", self.target_availability.clone()),
            ("copy_cost", self.copy_cost.clone()),
            ("effects", self.effects.clone()),
            ("effect_root", self.effect_root.clone()),
            ("artifact_coverage", self.artifact_coverage.digest()),
            (
                "loaded_artifact",
                self.artifact_coverage.loaded_artifact.clone(),
            ),
            (
                "transitive_dependencies",
                stable_optional_strings(
                    self.artifact_coverage.transitive_dependencies.as_deref(),
                ),
            ),
            (
                "reachable_callbacks",
                stable_optional_strings(self.artifact_coverage.reachable_callbacks.as_deref()),
            ),
            (
                "compiler_flags",
                stable_optional_strings(self.artifact_coverage.compiler_flags.as_deref()),
            ),
            ("coverage_target", self.artifact_coverage.target.clone()),
            ("coverage_generator", self.artifact_coverage.generator.clone()),
            (
                "obligations",
                self.obligations
                    .iter()
                    .map(|obligation| {
                        format!(
                            "{}:{}:{}:{}:{}",
                            obligation.obligation,
                            obligation.basis.as_str(),
                            obligation.checker,
                            obligation.coverage_digest,
                            obligation.assumptions.join("|"),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            ("source_import", self.source_import.to_string()),
            (
                "source_authority",
                self.source_authority.as_str().to_string(),
            ),
            ("source_identity", self.identity.source.clone()),
            ("overlay_identity", self.identity.overlay.clone()),
            ("generator_identity", self.identity.generator.clone()),
            (
                "implementation_identity",
                self.identity.implementation.clone(),
            ),
            ("toolchain_identity", self.identity.toolchain.clone()),
            ("target_identity", self.identity.target.clone()),
        ];
        for (name, width) in &self.scalar_widths {
            fields.push((name.as_str(), width.clone()));
        }
        fields
    }
}

fn scalar_width(scalar: ForeignScalar) -> &'static str {
    match scalar {
        ForeignScalar::Int => "target-declared-integer-width",
        ForeignScalar::Float => "target-declared-floating-width",
        ForeignScalar::Bool => "target-declared-boolean-width",
        ForeignScalar::Char => "target-declared-character-width",
        ForeignScalar::String => "pointer-plus-encoding",
        ForeignScalar::Unsupported => "unsupported",
    }
}

/// Return the target identity used by adapters that do not receive a target
/// argument directly. Environment-provided target wins over host guessing.
pub fn foreign_host_target() -> String {
    std::env::var("JET_TARGET")
        .or_else(|_| std::env::var("TARGET"))
        .unwrap_or_else(|_| format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS))
}


/// Parsed artifact provenance. Repeated fields are retained in insertion order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub schema: String,
    pub identity: String,
    pub fields: BTreeMap<String, Vec<String>>,
}

impl Provenance {
    /// Return the first value for a field.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.fields
            .get(name)
            .and_then(|values| values.first())
            .map(String::as_str)
    }
}

/// Build the identity shared by a scalar binder's bridge build and its
/// provenance sidecar. Input bytes matter: a source path alone cannot notice a
/// changed foreign implementation behind the same filename.
pub fn scalar_bridge_identity(
    descriptor: BinderDescriptor,
    lib: &str,
    abi: &str,
    runtime: &str,
    source: &Path,
    worker: &Path,
    functions: &[ScalarBridgeFunction],
) -> Result<String, String> {
    scalar_bridge_identity_with_sources(
        descriptor,
        lib,
        abi,
        runtime,
        &[("source", source)],
        worker,
        functions,
    )
}

/// Build a scalar bridge identity from all exact foreign source files involved
/// in the bridge. `sources` must include the primary `source` passed to the C
/// bridge renderer; adapters may add declaration files or other local inputs.
pub fn scalar_bridge_identity_with_sources(
    descriptor: BinderDescriptor,
    lib: &str,
    abi: &str,
    runtime: &str,
    sources: &[BridgeSource<'_>],
    worker: &Path,
    functions: &[ScalarBridgeFunction],
) -> Result<String, String> {
    validate_scalar_bridge(abi, descriptor, functions)?;
    let expected_abi = format!("{}{lib}", descriptor.language.bridge_prefix());
    if abi != expected_abi {
        return Err("foreign bridge ABI name does not match its descriptor language".into());
    }
    scalar_bridge_identity_with_descriptor(
        descriptor.language.root(),
        lib,
        abi,
        runtime,
        &descriptor.stamp(),
        sources,
        worker,
        functions,
    )
}

fn scalar_bridge_identity_with_descriptor(
    language: &str,
    lib: &str,
    abi: &str,
    runtime: &str,
    descriptor: &str,
    sources: &[BridgeSource<'_>],
    worker: &Path,
    functions: &[ScalarBridgeFunction],
) -> Result<String, String> {
    let worker_bytes = fs::read(worker).map_err(|error| {
        format!(
            "could not read {} for bridge identity: {error}",
            worker.display()
        )
    })?;
    let mut identity = IdentityBuilder::new(IDENTITY_SCHEMA);
    identity.field("bridge_schema", SCALAR_BRIDGE_SCHEMA.as_bytes());
    identity.field("language", language.as_bytes());
    identity.field("library", lib.as_bytes());
    identity.field("abi", abi.as_bytes());
    identity.field("runtime", runtime.as_bytes());
    identity.field("runtime_toolchain", tool_identity(runtime).as_bytes());
    identity.field("cc", tool_identity("cc").as_bytes());
    identity.field("ar", tool_identity("ar").as_bytes());
    identity.field("descriptor", descriptor.as_bytes());
    for (role, source) in sources {
        if role.is_empty() {
            return Err("foreign bridge source roles cannot be empty".into());
        }
        let path_field = format!("{role}_path");
        identity.field(&path_field, source.as_os_str().as_encoded_bytes());
        let source_bytes = fs::read(source).map_err(|error| {
            format!(
                "could not read {} for bridge identity: {error}",
                source.display()
            )
        })?;
        let bytes_field = format!("{role}_bytes");
        identity.field(&bytes_field, &source_bytes);
    }
    identity.field("worker_path", worker.as_os_str().as_encoded_bytes());
    identity.field("worker_bytes", &worker_bytes);
    for function in functions {
        identity.field("function", function.name.as_bytes());
        for parameter in &function.params {
            identity.field("parameter", format!("{parameter:?}").as_bytes());
        }
        identity.field("result", format!("{:?}", function.result).as_bytes());
    }
    Ok(identity.finish())
}

/// Write the queryable provenance sidecar for one published bridge.
pub fn write_provenance(
    path: &Path,
    identity: &str,
    fields: &[(&str, &str)],
    artifacts: &[(String, String)],
) -> Result<(), String> {
    let text = render_provenance(identity, fields, artifacts)?;
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, text.as_bytes())
        .map_err(|error| format!("could not stage {}: {error}", path.display()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("could not publish {}: {error}", path.display()));
    }
    Ok(())
}

/// Render the common queryable provenance format without publishing a file.
/// Binders use this when their result object carries the record to a CLI
/// writer; `write_provenance` uses the same formatter for on-disk records.
pub fn render_provenance(
    identity: &str,
    fields: &[(&str, &str)],
    artifacts: &[(String, String)],
) -> Result<String, String> {
    let mut text = format!("schema={PROVENANCE_SCHEMA}\nidentity={identity}\n");
    let has_boundary_fields = fields
        .iter()
        .any(|(name, _)| name.starts_with("boundary-"));
    let language = fields
        .iter()
        .find_map(|(name, value)| (*name == "language").then_some(*value))
        .and_then(crate::AST::ForeignLanguage::from_root);
    for (name, value) in fields {
        append_line(&mut text, name, value)?;
    }
    if let Some(language) = language.filter(|_| !has_boundary_fields) {
        let value = |name: &str| {
            fields
                .iter()
                .find_map(|(field, value)| (*field == name).then_some(*value))
        };
        let abi = value("abi");
        let library = value("library")
            .or_else(|| value("lib"))
            .or_else(|| {
                abi.and_then(|abi| {
                    abi.strip_prefix("jet_")
                        .and_then(|rest| rest.split_once('_'))
                        .map(|(_, library)| library)
                })
            })
            .unwrap_or("not-recorded");
        let loaded_artifact = artifacts
            .iter()
            .map(|(path, digest)| format!("{path}:sha256-{digest}"))
            .collect::<Vec<_>>()
            .join("|");
        let source = value("source")
            .or_else(|| value("source_path"))
            .or_else(|| value("type-library"))
            .unwrap_or("not-recorded");
        let overlay = value("overlay")
            .or_else(|| value("overlay-identity"))
            .unwrap_or("none");
        let generator = value("binder-schema")
            .or_else(|| value("descriptor"))
            .unwrap_or("not-recorded");
        let implementation = value("generated")
            .or_else(|| value("worker"))
            .or_else(|| value("archive"))
            .or_else(|| (!loaded_artifact.is_empty()).then_some(loaded_artifact.as_str()))
            .unwrap_or("not-recorded");
        let toolchain = value("toolchain")
            .or_else(|| value("runtime"))
            .or_else(|| value("cc"))
            .or_else(|| value("compiler"))
            .unwrap_or("not-recorded");
        let target = value("target")
            .or_else(|| value("foreign-host-target"))
            .map(str::to_string)
            .unwrap_or_else(foreign_host_target);
        let descriptor = crate::AST::binder_descriptor(language)
            .ok_or_else(|| format!("missing binder descriptor for {language:?}"))?;
        let boundary = ForeignBoundaryContract::new(
            *descriptor,
            library,
            ForeignBoundaryIdentity::new(
                source,
                overlay,
                generator,
                implementation,
                toolchain,
                target.clone(),
            ),
        )
        .with_artifact_coverage(ForeignArtifactCoverage::new(
            if loaded_artifact.is_empty() {
                "not-recorded"
            } else {
                loaded_artifact.as_str()
            },
            target,
            generator,
        ));
        for (name, value) in boundary.provenance_fields() {
            append_line(&mut text, &name, &value)?;
        }
        let capability = foreign_boundary_capability(language);
        append_line(
            &mut text,
            "boundary-mixed-debug",
            if capability.mixed_debug { "true" } else { "false" },
        )?;
        append_line(
            &mut text,
            "boundary-native-export",
            if capability.native_export { "true" } else { "false" },
        )?;
        append_line(
            &mut text,
            "boundary-build-host",
            if capability.build_host { "true" } else { "false" },
        )?;
        append_line(&mut text, "boundary-reason", capability.reason)?;
    }
    for (relative, digest) in artifacts {
        append_line(&mut text, &format!("artifact.{relative}"), digest)?;
    }
    Ok(text)
}

/// Append the canonical boundary row to a binder-owned provenance record.
///
/// Adapters that use a legacy text writer call this at their producer boundary;
/// the row still names the same descriptor, artifact coverage, and evidence
/// vocabulary as adapters using [`render_provenance`].
pub fn append_boundary_provenance(
    text: &mut String,
    descriptor: BinderDescriptor,
    library: impl Into<String>,
    identity: ForeignBoundaryIdentity,
    coverage: ForeignArtifactCoverage,
) -> Result<(), String> {
    if text
        .lines()
        .any(|line| line.starts_with("boundary-schema="))
    {
        return Ok(());
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let boundary = ForeignBoundaryContract::new(descriptor, library, identity)
        .with_artifact_coverage(coverage);
    for (name, value) in boundary.provenance_fields() {
        append_line(text, &name, &value)?;
    }
    let capability = foreign_boundary_capability(descriptor.language);
    append_line(
        text,
        "boundary-mixed-debug",
        if capability.mixed_debug { "true" } else { "false" },
    )?;
    append_line(
        text,
        "boundary-native-export",
        if capability.native_export { "true" } else { "false" },
    )?;
    append_line(
        text,
        "boundary-build-host",
        if capability.build_host { "true" } else { "false" },
    )?;
    append_line(text, "boundary-reason", capability.reason)?;
    Ok(())
}

/// Append a boundary for an adapter that publishes a legacy text record.
pub fn append_boundary_for_artifact(
    text: &mut String,
    descriptor: BinderDescriptor,
    library: impl Into<String>,
    source: &Path,
    archive: &Path,
    toolchain: impl Into<String>,
) -> Result<(), String> {
    let source_digest = sha_file(source)?;
    let archive_digest = sha_file(archive)?;
    let archive_name = archive
        .file_name()
        .ok_or_else(|| "foreign artifact path has no file name".to_string())?
        .to_string_lossy()
        .into_owned();
    let target = foreign_host_target();
    append_boundary_provenance(
        text,
        descriptor,
        library,
        ForeignBoundaryIdentity::new(
            format!("source:{}:sha256-{source_digest}", source.display()),
            "none",
            descriptor.stamp(),
            format!("archive:{archive_name}:sha256-{archive_digest}"),
            toolchain,
            target.clone(),
        ),
        ForeignArtifactCoverage::new(
            format!("{archive_name}:sha256-{archive_digest}"),
            target,
            descriptor.stamp(),
        ),
    )
}
fn parse_provenance(
    path: &Path,
) -> Result<(String, Option<String>, BTreeMap<String, Vec<String>>), String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mut schema = None;
    let mut identity = None;
    let mut fields = BTreeMap::<String, Vec<String>>::new();
    for line in text.lines() {
        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| format!("malformed bridge provenance line in {}", path.display()))?;
        if name.is_empty() || value.is_empty() {
            return Err(format!(
                "empty bridge provenance field in {}",
                path.display()
            ));
        }
        match name {
            "schema" => {
                if schema.replace(value.to_string()).is_some() {
                    return Err(format!(
                        "duplicate bridge provenance schema in {}",
                        path.display()
                    ));
                }
            }
            "identity" => {
                if identity.replace(value.to_string()).is_some() {
                    return Err(format!(
                        "duplicate bridge provenance identity in {}",
                        path.display()
                    ));
                }
            }
            _ => fields
                .entry(name.to_string())
                .or_default()
                .push(value.to_string()),
        }
    }
    let schema =
        schema.ok_or_else(|| format!("bridge provenance has no schema: {}", path.display()))?;
    Ok((schema, identity, fields))
}

/// Read and validate a bridge provenance sidecar.
pub fn read_provenance(path: &Path) -> Result<Provenance, String> {
    let (schema, identity, fields) = parse_provenance(path)?;
    let identity =
        identity.ok_or_else(|| format!("bridge provenance has no identity: {}", path.display()))?;
    if schema != PROVENANCE_SCHEMA {
        return Err(format!("unsupported bridge provenance schema `{schema}`"));
    }
    Ok(Provenance {
        schema,
        identity,
        fields,
    })
}

/// Read the canonical boundary component embedded in a binder provenance sidecar.
pub fn read_boundary_provenance(path: &Path) -> Result<Provenance, String> {
    let (_, _, fields) = parse_provenance(path)?;
    if !fields
        .get("boundary-schema")
        .and_then(|values| values.first())
        .is_some_and(|value| value == FOREIGN_BOUNDARY_SCHEMA)
    {
        return Err(format!(
            "foreign boundary provenance has no canonical boundary row: {}",
            path.display()
        ));
    }
    let identity = fields
        .get("boundary-digest")
        .and_then(|values| values.first())
        .cloned()
        .ok_or_else(|| format!("foreign boundary provenance has no digest: {}", path.display()))?;
    Ok(Provenance {
        schema: FOREIGN_BOUNDARY_SCHEMA.to_string(),
        identity,
        fields,
    })
}

fn append_line(text: &mut String, name: &str, value: &str) -> Result<(), String> {
    if name.is_empty()
        || value.is_empty()
        || name
            .bytes()
            .any(|byte| matches!(byte, b'\n' | b'\r' | b'='))
        || value.bytes().any(|byte| matches!(byte, b'\n' | b'\r'))
    {
        return Err("bridge provenance fields cannot contain line breaks or `=`".into());
    }
    text.push_str(name);
    text.push('=');
    text.push_str(value);
    text.push('\n');
    Ok(())
}

/// Compile the one C ABI used by supervised scalar adapters. The worker owns
/// transport and language exceptions; this bridge only marshals scalars and
/// turns every non-OK response into a checked error code.
pub fn compile_scalar_sidecar(
    cache: &Path,
    abi: &str,
    runtime: &str,
    worker: &Path,
    source: &Path,
    descriptor: BinderDescriptor,
    functions: &[ScalarBridgeFunction],
) -> Result<PathBuf, String> {
    validate_scalar_bridge(abi, descriptor, functions)?;
    let lib = scalar_bridge_library(abi, descriptor)?;
    let identity =
        scalar_bridge_identity(descriptor, lib, abi, runtime, source, worker, functions)?;
    compile_scalar_sidecar_with_identity(
        cache, &identity, abi, runtime, worker, source, descriptor, functions,
    )
}

/// Compile or reuse one content-addressed scalar sidecar. The returned path is
/// the stable projection consumed by the existing generated binding cache; the
/// actual build artifact lives below `.bridges/<identity>`.
pub fn compile_scalar_sidecar_with_identity(
    cache: &Path,
    identity: &str,
    abi: &str,
    runtime: &str,
    worker: &Path,
    source: &Path,
    descriptor: BinderDescriptor,
    functions: &[ScalarBridgeFunction],
) -> Result<PathBuf, String> {
    compile_scalar_sidecar_with_identity_and_sources(
        cache,
        identity,
        abi,
        runtime,
        worker,
        source,
        descriptor,
        &[("source", source)],
        functions,
    )
}

/// Compile or reuse a scalar sidecar while checking an identity containing
/// additional exact source files (for example a JS declaration plus runtime).
pub fn compile_scalar_sidecar_with_identity_and_sources(
    cache: &Path,
    identity: &str,
    abi: &str,
    runtime: &str,
    worker: &Path,
    source: &Path,
    descriptor: BinderDescriptor,
    sources: &[BridgeSource<'_>],
    functions: &[ScalarBridgeFunction],
) -> Result<PathBuf, String> {
    validate_scalar_bridge(abi, descriptor, functions)?;
    let lib = scalar_bridge_library(abi, descriptor)?;
    let expected_identity = scalar_bridge_identity_with_sources(
        descriptor,
        lib,
        abi,
        runtime,
        sources,
        worker,
        functions,
    )?;
    if expected_identity != identity {
        return Err("foreign bridge identity does not match its descriptor inputs".into());
    }
    fs::create_dir_all(cache)
        .map_err(|error| format!("could not create foreign binding cache: {error}"))?;
    let store = cache.join(".bridges").join(identity);
    let cached_archive = store.join(format!("lib{abi}.a"));
    let archive = cache.join(format!("lib{abi}.a"));
    if scalar_archive_valid(&cached_archive) {
        fs::copy(&cached_archive, &archive).map_err(|error| {
            format!(
                "could not project cached foreign bridge {}: {error}",
                archive.display()
            )
        })?;
        return Ok(archive);
    }
    fs::create_dir_all(&store)
        .map_err(|error| format!("could not create scalar bridge cache: {error}"))?;
    let c_path = store.join(format!("{abi}.c"));
    let object = store.join(format!("{abi}.o"));
    let staged_archive = store.join(format!(".{abi}.a.tmp.{}", std::process::id()));
    fs::write(
        &c_path,
        render_scalar_c(abi, runtime, worker, source, descriptor, functions)?,
    )
    .map_err(|error| format!("could not write foreign bridge: {error}"))?;
    let compile = Command::new("cc")
        .args(["-std=c11", "-D_POSIX_C_SOURCE=200809L", "-fPIC", "-c"])
        .arg(&c_path)
        .arg("-o")
        .arg(&object)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "the provisioned cc tool was not found".to_string()
            } else {
                format!("could not start cc: {error}")
            }
        })?;
    if !compile.status.success() {
        let detail = String::from_utf8_lossy(&compile.stderr).trim().to_string();
        let _ = fs::remove_file(&c_path);
        let _ = fs::remove_file(&object);
        return Err(format!("cc rejected the foreign bridge: {detail}"));
    }
    let archive_result = Command::new("ar")
        .arg("rcs")
        .arg(&staged_archive)
        .arg(&object)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "the provisioned ar tool was not found".to_string()
            } else {
                format!("could not start ar: {error}")
            }
        })?;
    let _ = fs::remove_file(&c_path);
    let _ = fs::remove_file(&object);
    if !archive_result.status.success() {
        let detail = String::from_utf8_lossy(&archive_result.stderr)
            .trim()
            .to_string();
        return Err(format!("ar rejected the foreign bridge: {detail}"));
    }
    if let Err(error) = fs::rename(&staged_archive, &cached_archive) {
        let _ = fs::remove_file(&staged_archive);
        return Err(format!("could not publish cached foreign bridge: {error}"));
    }
    fs::write(
        store.join(format!("lib{abi}.sha256")),
        crate::SHA256::sha256_hex(
            &fs::read(&cached_archive)
                .map_err(|error| format!("could not read cached foreign bridge: {error}"))?,
        ),
    )
    .map_err(|error| format!("could not publish scalar bridge cache identity: {error}"))?;
    fs::copy(&cached_archive, &archive).map_err(|error| {
        format!(
            "could not project foreign bridge {}: {error}",
            archive.display()
        )
    })?;
    Ok(archive)
}

/// Publish a queryable descriptor/provenance record for a scalar adapter.
pub fn write_scalar_provenance(
    cache: &Path,
    lib: &str,
    descriptor: BinderDescriptor,
    runtime: &str,
    source: &Path,
    worker: &Path,
    archive: &Path,
) -> Result<PathBuf, String> {
    write_scalar_provenance_with_sources(
        cache,
        lib,
        descriptor,
        runtime,
        source,
        &[("source", source)],
        worker,
        archive,
        &[],
    )
}

/// Publish scalar provenance using the exact function list used to build the
/// bridge. Binders call this variant so the sidecar identity cannot drift from
/// the cache key.
pub fn write_scalar_provenance_with_functions(
    cache: &Path,
    lib: &str,
    descriptor_row: BinderDescriptor,
    runtime: &str,
    source: &Path,
    worker: &Path,
    archive: &Path,
    functions: &[ScalarBridgeFunction],
) -> Result<PathBuf, String> {
    write_scalar_provenance_with_sources(
        cache,
        lib,
        descriptor_row,
        runtime,
        source,
        &[("source", source)],
        worker,
        archive,
        functions,
    )
}

/// Publish scalar provenance from the exact source roles used by the bridge.
/// The common record carries a digest and path for every role, while the
/// identity itself carries the complete bytes.
pub fn write_scalar_provenance_with_sources(
    cache: &Path,
    lib: &str,
    descriptor_row: BinderDescriptor,
    runtime: &str,
    source: &Path,
    sources: &[BridgeSource<'_>],
    worker: &Path,
    archive: &Path,
    functions: &[ScalarBridgeFunction],
) -> Result<PathBuf, String> {
    if !sources.iter().any(|(_, path)| *path == source) {
        return Err("foreign bridge provenance sources omit the primary source".into());
    }
    let contract = descriptor_row.contract;
    let descriptor = descriptor_row.stamp();
    let calling = format!("{:?}", contract.calling_convention);
    let layout = format!("{:?}", contract.layout);
    let ownership = format!("{:?}", contract.ownership);
    let errors = format!("{:?}", contract.errors);
    let callbacks = format!("{:?}", contract.callbacks);
    let async_completion = format!("{:?}", contract.async_completion);
    let task_boundary = format!("{:?}", contract.task_boundary);
    let safety = format!("{:?}", contract.safety);
    let provider = format!("{:?}", descriptor_row.provider);
    let language = descriptor_row.language;
    let abi = format!("jet_{}_{}", language.root(), lib);
    let identity = scalar_bridge_identity_with_sources(
        descriptor_row,
        lib,
        &abi,
        runtime,
        sources,
        worker,
        functions,
    )?;
    let runtime_toolchain = tool_identity(runtime);
    let cc = tool_identity("cc");
    let ar = tool_identity("ar");
    let worker_digest = sha_file(worker)?;
    let archive_digest = sha_file(archive)?;
    let source_identity = sources
        .iter()
        .map(|(role, path)| {
            Ok(format!(
                "{role}:{}:{}",
                path.display(),
                sha_file(path)?
            ))
        })
        .collect::<Result<Vec<_>, String>>()?
        .join("|");
    let boundary_identity = ForeignBoundaryIdentity::new(
        source_identity,
        "none",
        format!("descriptor={descriptor}"),
        format!("worker:{}:{worker_digest}", worker.display()),
        format!("runtime={runtime_toolchain};cc={cc};ar={ar}"),
        foreign_host_target(),
    );
    let mut boundary = ForeignBoundaryContract::new(descriptor_row, lib, boundary_identity);
    let coverage = ForeignArtifactCoverage::new(
        format!("{}:sha256-{archive_digest}", archive.display()),
        foreign_host_target(),
        boundary.identity.generator.clone(),
    )
    .with_transitive_dependencies(Vec::<String>::new())
    .with_reachable_callbacks(Vec::<String>::new())
    .with_compiler_flags(Vec::<String>::new());
    boundary = boundary.with_artifact_coverage(coverage);
    for obligation in FOREIGN_BOUNDARY_OBLIGATIONS {
        boundary.set_obligation(
            (*obligation).to_string(),
            ForeignEvidenceBasis::Trusted,
            "scalar-sidecar-generator",
            [
                "adapter implementation remains TRUSTED outside hardening".into(),
                "loaded archive and generated worker identities are recorded".into(),
            ],
        )?;
    }
    let provenance = cache.join(format!("{lib}.provenance"));
    let mut fields = vec![
        ("language".to_string(), language.root().to_string()),
        ("abi".to_string(), abi),
        ("runtime".to_string(), runtime.to_string()),
        (
            "transport".to_string(),
            "supervised-scalar-sidecar".to_string(),
        ),
        ("descriptor".to_string(), descriptor),
        ("calling".to_string(), calling),
        ("layout".to_string(), layout),
        ("ownership".to_string(), ownership),
        ("errors".to_string(), errors),
        ("callbacks".to_string(), callbacks),
        ("async".to_string(), async_completion),
        ("tasks".to_string(), task_boundary),
        ("safety".to_string(), safety),
        ("provider".to_string(), provider),
        ("runtime-toolchain".to_string(), runtime_toolchain),
        ("cc".to_string(), cc),
        ("ar".to_string(), ar),
    ];
    fields.extend(boundary.provenance_fields());
    for (role, path) in sources {
        fields.push((role.to_string(), path.to_string_lossy().into_owned()));
        fields.push((
            format!("{role}-sha256"),
            sha_file(path)?,
        ));
    }
    fields.push(("worker".to_string(), worker.to_string_lossy().into_owned()));
    fields.push(("worker-sha256".to_string(), worker_digest.clone()));
    for function in functions {
        fields.push(("function".to_string(), function.name.clone()));
        fields.push((
            "function-params".to_string(),
            format!("{:?}", function.params),
        ));
        fields.push((
            "function-result".to_string(),
            format!("{:?}", function.result),
        ));
    }
    let field_refs: Vec<(&str, &str)> = fields
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let artifacts = [
        (
            worker
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            worker_digest,
        ),
        (
            archive
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            archive_digest,
        ),
    ];
    let artifact_refs: Vec<(String, String)> = artifacts.into_iter().collect();
    write_provenance(&provenance, &identity, &field_refs, &artifact_refs)?;
    Ok(provenance)
}

/// Render the shared safe wrapper. Raw C symbols stay private to the generated
/// module and every public call checks the adapter's contained error slot.
pub fn render_scalar_jet(
    abi: &str,
    descriptor: BinderDescriptor,
    functions: &[ScalarBridgeFunction],
) -> Result<String, String> {
    validate_scalar_bridge(abi, descriptor, functions)?;
    let effect = descriptor.effect_root;
    let contract = descriptor.stamp();
    let mut out = format!("// jet-ffi-descriptor={contract}\n#Import module c.{abi} {{\n");
    for function in functions {
        let _ = write!(out, "    fn {}(", function.name);
        for (index, scalar) in function.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            let jet_type = scalar.jet_name().ok_or_else(|| {
                format!("foreign scalar `{scalar:?}` is outside the checked sidecar ABI")
            })?;
            let _ = write!(out, "arg{index}: {jet_type}");
        }
        let result = function.result.jet_name().ok_or_else(|| {
            format!(
                "foreign scalar `{:?}` is outside the checked sidecar ABI",
                function.result
            )
        })?;
        let _ = writeln!(out, ") {result} = \"{abi}_{}\"", function.name);
    }
    let _ = writeln!(out, "    fn take_error() Int = \"{abi}_take_error\"\n}}");
    let _ = writeln!(out, "use c.{abi} as abi\n");
    for function in functions {
        let _ = write!(out, "pub fn {}(", function.name);
        for (index, scalar) in function.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            let jet_type = scalar.jet_name().ok_or_else(|| {
                format!("foreign scalar `{scalar:?}` is outside the checked sidecar ABI")
            })?;
            let _ = write!(out, "arg{index}: {jet_type}");
        }
        let result = function.result.jet_name().ok_or_else(|| {
            format!(
                "foreign scalar `{:?}` is outside the checked sidecar ABI",
                function.result
            )
        })?;
        let _ = writeln!(out, ") {result} !String -[{effect}]> {{",);
        let _ = write!(out, "    value :: abi.{}(", function.name);
        for index in 0..function.params.len() {
            if index > 0 {
                out.push_str(", ");
            }
            let _ = write!(out, "arg{index}");
        }
        out.push_str(")\n    if abi.take_error() != 0 {\n        return Err(\"foreign call failed\")\n    }\n    return Ok(value)\n}\n\n");
    }
    Ok(out)
}

fn render_scalar_c(
    abi: &str,
    runtime: &str,
    worker: &Path,
    source: &Path,
    descriptor: BinderDescriptor,
    functions: &[ScalarBridgeFunction],
) -> Result<String, String> {
    let descriptor = descriptor.stamp();
    let worker = c_escape(&shell_quote(&worker.to_string_lossy()));
    let source = c_escape(&shell_quote(&source.to_string_lossy()));
    let runtime = c_escape(&shell_quote(runtime));
    let mut out = format!(
        "/* jet-ffi-descriptor={descriptor} */\n#include <ctype.h>\n#include <errno.h>\n#include <stdbool.h>\n#include <stdint.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\nstatic _Thread_local int64_t jet_failed;\nstatic int jet_invoke(const char *command, char *output, size_t capacity) {{\n    jet_failed = 0;\n    FILE *pipe = popen(command, \"r\");\n    if (!pipe) {{ jet_failed = 1; return 0; }}\n    if (!fgets(output, (int)capacity, pipe)) {{ jet_failed = 2; pclose(pipe); return 0; }}\n    int status = pclose(pipe);\n    if (status != 0 || strncmp(output, \"OK \", 3) != 0) {{ jet_failed = 3; return 0; }}\n    return 1;\n}}\n"
    );
    let _ = writeln!(
        out,
        "int64_t {abi}_take_error(void) {{ int64_t value = jet_failed; jet_failed = 0; return value; }}"
    );
    for function in functions {
        let result_type =
            c_type(function.result).ok_or_else(|| unsupported_scalar(function.result))?;
        let _ = write!(out, "{result_type} {abi}_{}(", function.name);
        for (index, scalar) in function.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            let parameter_type = c_type(*scalar).ok_or_else(|| unsupported_scalar(*scalar))?;
            let _ = write!(out, "{parameter_type} arg{index}");
        }
        out.push_str(") {\n    char output[256];\n    char command[8192];\n    int written = snprintf(command, sizeof(command), \"");
        out.push_str(&runtime);
        out.push(' ');
        out.push_str(&worker);
        out.push(' ');
        out.push_str(&source);
        out.push(' ');
        out.push_str(&c_escape(&shell_quote(&function.name)));
        for scalar in &function.params {
            out.push_str(format_spec(*scalar).ok_or_else(|| unsupported_scalar(*scalar))?);
        }
        if function.params.is_empty() {
            out.push_str(" 2>/dev/null\");\n");
        } else {
            out.push_str(" 2>/dev/null\", ");
            for (index, scalar) in function.params.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str(
                    &format_arg(*scalar, index).ok_or_else(|| unsupported_scalar(*scalar))?,
                );
            }
            out.push_str(");\n");
        }
        out.push_str("    if (written < 0 || (size_t)written >= sizeof(command) || !jet_invoke(command, output, sizeof(output))) return (");
        out.push_str(
            zero_value(function.result).ok_or_else(|| unsupported_scalar(function.result))?,
        );
        out.push_str(");\n    char *end = NULL;\n    errno = 0;\n    ");
        match function.result {
            ForeignScalar::Int => out.push_str(
                "long long value = strtoll(output + 3, &end, 10); if (errno || end == output + 3) { jet_failed = 4; return 0; }\n",
            ),
            ForeignScalar::Float => out.push_str(
                "double value = strtod(output + 3, &end); if (errno || end == output + 3) { jet_failed = 4; return 0; }\n",
            ),
            ForeignScalar::Bool => out.push_str(
                "bool value; if (strncmp(output + 3, \"true\", 4) == 0) { value = true; end = output + 7; } else if (strncmp(output + 3, \"false\", 5) == 0) { value = false; end = output + 8; } else { jet_failed = 4; return false; }\n",
            ),
            scalar => return Err(unsupported_scalar(scalar)),
        }
        out.push_str("    while (*end && isspace((unsigned char)*end)) end++;\n    if (*end) { jet_failed = 4; return ");
        out.push_str(
            zero_value(function.result).ok_or_else(|| unsupported_scalar(function.result))?,
        );
        out.push_str("; }\n    return value;\n}\n");
    }
    Ok(out)
}

fn c_type(scalar: ForeignScalar) -> Option<&'static str> {
    match scalar {
        ForeignScalar::Int => Some("int64_t"),
        ForeignScalar::Float => Some("double"),
        ForeignScalar::Bool => Some("bool"),
        ForeignScalar::Char | ForeignScalar::String | ForeignScalar::Unsupported => None,
    }
}

fn format_spec(scalar: ForeignScalar) -> Option<&'static str> {
    match scalar {
        ForeignScalar::Int => Some(" %lld"),
        ForeignScalar::Float => Some(" %.17g"),
        ForeignScalar::Bool => Some(" %s"),
        ForeignScalar::Char | ForeignScalar::String | ForeignScalar::Unsupported => None,
    }
}

fn format_arg(scalar: ForeignScalar, index: usize) -> Option<String> {
    match scalar {
        ForeignScalar::Int => Some(format!("(long long)arg{index}")),
        ForeignScalar::Float => Some(format!("arg{index}")),
        ForeignScalar::Bool => Some(format!("arg{index} ? \"true\" : \"false\"")),
        ForeignScalar::Char | ForeignScalar::String | ForeignScalar::Unsupported => None,
    }
}

fn zero_value(scalar: ForeignScalar) -> Option<&'static str> {
    match scalar {
        ForeignScalar::Int => Some("0"),
        ForeignScalar::Float => Some("0.0"),
        ForeignScalar::Bool => Some("false"),
        ForeignScalar::Char | ForeignScalar::String | ForeignScalar::Unsupported => None,
    }
}

fn unsupported_scalar(scalar: ForeignScalar) -> String {
    format!("foreign scalar `{scalar:?}` is outside the checked sidecar ABI")
}

fn validate_scalar_bridge(
    abi: &str,
    descriptor: BinderDescriptor,
    functions: &[ScalarBridgeFunction],
) -> Result<(), String> {
    scalar_bridge_library(abi, descriptor)?;
    if descriptor.contract != ForeignAbiContract::MESSAGE {
        return Err(format!(
            "foreign binder `{}` does not expose the checked adapter contract",
            descriptor.language.root()
        ));
    }
    for capability in [
        BinderCapability::TypedStub,
        BinderCapability::SafeWrapper,
        BinderCapability::OwnershipConversion,
        BinderCapability::LayoutValidation,
        BinderCapability::ErrorConversion,
        BinderCapability::CacheProvenance,
    ] {
        if !descriptor.capabilities.contains(&capability) {
            return Err(format!(
                "foreign binder `{}` is missing capability `{capability:?}`",
                descriptor.language.root()
            ));
        }
    }
    let mut names = BTreeMap::new();
    for function in functions {
        if !is_identifier(&function.name) {
            return Err(format!("invalid foreign function name `{}`", function.name));
        }
        if names.insert(&function.name, ()).is_some() {
            return Err(format!("duplicate foreign function `{}`", function.name));
        }
        for scalar in function
            .params
            .iter()
            .copied()
            .chain(std::iter::once(function.result))
        {
            if !matches!(
                scalar,
                ForeignScalar::Int | ForeignScalar::Float | ForeignScalar::Bool
            ) {
                return Err(format!(
                    "foreign scalar `{scalar:?}` is outside the checked sidecar ABI"
                ));
            }
        }
    }
    Ok(())
}

fn scalar_bridge_library<'a>(
    abi: &'a str,
    descriptor: BinderDescriptor,
) -> Result<&'a str, String> {
    let prefix = descriptor.language.bridge_prefix();
    let Some(lib) = abi.strip_prefix(prefix).filter(|lib| is_identifier(lib)) else {
        return Err(format!(
            "foreign bridge ABI name `{abi}` does not match `{prefix}<library>`"
        ));
    };
    Ok(lib)
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn scalar_archive_valid(archive: &Path) -> bool {
    let Some(store) = archive.parent() else {
        return false;
    };
    let Some(file_name) = archive.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Ok(expected) = fs::read_to_string(store.join(format!("{file_name}.sha256"))) else {
        return false;
    };
    let expected = expected.trim();
    expected.len() == 64
        && expected
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        && fs::read(archive)
            .ok()
            .is_some_and(|bytes| crate::SHA256::sha256_hex(&bytes) == expected)
}

fn shell_quote(value: &str) -> String {
    let mut out = String::from("'");
    for (index, part) in value.split('\'').enumerate() {
        if index > 0 {
            out.push_str("'\\''");
        }
        out.push_str(part);
    }
    out.push('\'');
    out
}

fn c_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(character),
        }
    }
    out
}

const LOCAL_ARCHIVE_SUFFIXES: &[&str] = &[".a", ".so", ".dylib", ".dll", ".lib"];

/// Resolve each named native library to the first exact local archive found in
/// the declared search paths. Missing candidates remain records, so adding or
/// replacing a local archive changes the identity instead of silently reusing
/// a stale bridge.
pub fn local_archive_inputs(
    library_dirs: &[PathBuf],
    libraries: &[String],
) -> Vec<ArchiveInput> {
    if library_dirs.is_empty() {
        return Vec::new();
    }
    libraries
        .iter()
        .map(|library| {
            let mut path = None;
            'directories: for directory in library_dirs {
                for suffix in LOCAL_ARCHIVE_SUFFIXES {
                    let candidate = directory.join(format!("lib{library}{suffix}"));
                    if candidate.is_file() {
                        path = Some(candidate);
                        break 'directories;
                    }
                }
            }
            let path = path.unwrap_or_else(|| {
                library_dirs
                    .first()
                    .map(|directory| directory.join(format!("lib{library}.a")))
                    .unwrap_or_else(|| PathBuf::from(format!("lib{library}.a")))
            });
            let path = path.canonicalize().unwrap_or(path);
            let bytes = fs::read(&path).ok();
            ArchiveInput {
                library: library.clone(),
                path,
                bytes,
            }
        })
        .collect()
}

/// Add local archive path/byte records to a binding identity.
pub fn add_local_archive_inputs(
    identity: &mut IdentityBuilder,
    library_dirs: &[PathBuf],
    libraries: &[String],
) {
    let inputs = local_archive_inputs(library_dirs, libraries);
    add_archive_inputs(identity, &inputs);
}

/// Add already-resolved archive records to an identity. Callers that have a
/// provider-specific notion of which libraries are local can use this without
/// manufacturing missing candidates for system or provisioned dependencies.
pub fn add_archive_inputs(identity: &mut IdentityBuilder, inputs: &[ArchiveInput]) {
    for input in inputs {
        identity.field("linked_library", input.library.as_bytes());
        identity.field("linked_archive_path", input.path.as_os_str().as_encoded_bytes());
        match &input.bytes {
            Some(bytes) => identity.field("linked_archive_bytes", bytes),
            None => identity.field("linked_archive_missing", b"true"),
        }
    }
}

/// Resolve one pinned host tool without relying on the shell's executable
/// suffix rules. Windows toolchains commonly expose `cc.exe`/`ar.exe`, while
/// POSIX toolchains expose the unsuffixed names.
pub fn tool_path(tool: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).find_map(|directory| {
        let candidate = directory.join(tool);
        if candidate.is_file() {
            return Some(candidate);
        }
        cfg!(windows)
            .then(|| directory.join(format!("{tool}.exe")))
            .filter(|candidate| candidate.is_file())
    })
}

pub fn tool_identity(tool: &str) -> String {
    let Some(path) = tool_path(tool) else {
        return "missing".to_string();
    };
    let path = path.canonicalize().unwrap_or(path);
    let version = Command::new(&path)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" | ")
        })
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "unavailable".to_string());
    format!("{};version={version}", path.display())
}

pub fn sha_file(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    Ok(crate::SHA256::sha256_hex(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AST::ForeignLanguage;

    #[test]
    fn identity_is_stable_and_input_sensitive() {
        let mut first = IdentityBuilder::new(IDENTITY_SCHEMA);
        first.field("descriptor", b"one");
        let mut same = IdentityBuilder::new(IDENTITY_SCHEMA);
        same.field("descriptor", b"one");
        let mut changed = IdentityBuilder::new(IDENTITY_SCHEMA);
        changed.field("descriptor", b"two");
        let first = first.finish();
        assert_eq!(first, same.finish());
        assert_ne!(first, changed.finish());
    }

    #[test]
    fn descriptor_drives_stub_and_binder_source() {
        let descriptor = *crate::AST::binder_descriptor(ForeignLanguage::Py).unwrap();
        let functions = [ScalarBridgeFunction {
            name: "probe".into(),
            params: vec![ForeignScalar::Int],
            result: ForeignScalar::Int,
        }];
        let stub = render_scalar_jet("jet_py_probe", descriptor, &functions).unwrap();
        let binder = render_scalar_c(
            "jet_py_probe",
            "python3",
            Path::new("worker.py"),
            Path::new("probe.py"),
            descriptor,
            &functions,
        )
        .unwrap();

        let mut changed = descriptor;
        changed.effect_root = "FFI.changed";
        let changed_stub = render_scalar_jet("jet_py_probe", changed, &functions).unwrap();
        let changed_binder = render_scalar_c(
            "jet_py_probe",
            "python3",
            Path::new("worker.py"),
            Path::new("probe.py"),
            changed,
            &functions,
        )
        .unwrap();

        assert!(stub.contains("-[FFI.Py]>"));
        assert!(changed_stub.contains("-[FFI.changed]>"));
        assert_ne!(stub, changed_stub);
        assert_ne!(binder, changed_binder);
        assert!(binder.contains(&descriptor.stamp()));
        assert!(changed_binder.contains(&changed.stamp()));
    }

    #[test]
    fn unsupported_scalar_fails_closed_before_rendering() {
        let descriptor = *crate::AST::binder_descriptor(ForeignLanguage::Py).unwrap();
        let functions = [ScalarBridgeFunction {
            name: "bad".into(),
            params: vec![ForeignScalar::Unsupported],
            result: ForeignScalar::Int,
        }];
        let error = render_scalar_jet("jet_py_bad", descriptor, &functions).unwrap_err();
        assert!(error.contains("outside the checked sidecar ABI"));
    }

    #[test]
    fn descriptor_language_owns_scalar_bridge_abi_prefix() {
        let descriptor = *crate::AST::binder_descriptor(ForeignLanguage::Py).unwrap();
        let functions = [ScalarBridgeFunction {
            name: "probe".into(),
            params: vec![ForeignScalar::Int],
            result: ForeignScalar::Int,
        }];
        let error = render_scalar_jet("jet_js_probe", descriptor, &functions).unwrap_err();
        assert!(error.contains("does not match `jet_py_<library>`"));
        let error = scalar_bridge_identity(
            descriptor,
            "other",
            "jet_py_probe",
            "python3",
            Path::new("missing.py"),
            Path::new("missing_worker.py"),
            &functions,
        )
        .unwrap_err();
        assert!(error.contains("does not match its descriptor language"));
    }

    #[test]
    fn scalar_bridge_cache_reuses_identical_inputs_and_misses_source_changes() {
        if !Command::new("cc")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
            || !Command::new("ar")
                .arg("--version")
                .output()
                .is_ok_and(|output| output.status.success())
        {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "jet_scalar_bridge_cache_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("ops.py");
        let worker = root.join("ops_worker.py");
        fs::write(&source, "def probe(value): return value\n").unwrap();
        fs::write(&worker, "print('worker')\n").unwrap();
        let functions = [ScalarBridgeFunction {
            name: "probe".into(),
            params: vec![ForeignScalar::Int],
            result: ForeignScalar::Int,
        }];
        let descriptor = *crate::AST::binder_descriptor(ForeignLanguage::Py).unwrap();
        let identity = scalar_bridge_identity(
            descriptor,
            "ops",
            "jet_py_ops",
            "python3",
            &source,
            &worker,
            &functions,
        )
        .unwrap();
        let mut changed_descriptor = descriptor;
        changed_descriptor.effect_root = "FFI.changed";
        let descriptor_identity = scalar_bridge_identity(
            changed_descriptor,
            "ops",
            "jet_py_ops",
            "python3",
            &source,
            &worker,
            &functions,
        )
        .unwrap();
        assert_ne!(identity, descriptor_identity);
        let mut changed_capabilities = descriptor;
        changed_capabilities.capabilities = &[];
        let capability_error = scalar_bridge_identity(
            changed_capabilities,
            "ops",
            "jet_py_ops",
            "python3",
            &source,
            &worker,
            &functions,
        )
        .unwrap_err();
        assert!(capability_error.contains("missing capability"));
        let runtime_identity = scalar_bridge_identity(
            descriptor,
            "ops",
            "jet_py_ops",
            "python3-alt",
            &source,
            &worker,
            &functions,
        )
        .unwrap();
        assert_ne!(identity, runtime_identity);
        let worker_bytes = fs::read(&worker).unwrap();
        fs::write(&worker, b"print('changed worker')\n").unwrap();
        let worker_identity = scalar_bridge_identity(
            descriptor,
            "ops",
            "jet_py_ops",
            "python3",
            &source,
            &worker,
            &functions,
        )
        .unwrap();
        assert_ne!(identity, worker_identity);
        fs::write(&worker, worker_bytes).unwrap();
        let first = compile_scalar_sidecar_with_identity(
            &root,
            &identity,
            "jet_py_ops",
            "python3",
            &worker,
            &source,
            descriptor,
            &functions,
        )
        .unwrap();
        let store = root.join(".bridges").join(&identity);
        assert!(store.join("libjet_py_ops.a").is_file());
        fs::remove_file(&first).unwrap();
        let reused = compile_scalar_sidecar_with_identity(
            &root,
            &identity,
            "jet_py_ops",
            "python3",
            &worker,
            &source,
            descriptor,
            &functions,
        )
        .unwrap();
        assert_eq!(first, reused);
        assert!(reused.is_file());

        fs::write(&source, "def probe(value): return value + 1\n").unwrap();
        let stale = compile_scalar_sidecar_with_identity(
            &root,
            &identity,
            "jet_py_ops",
            "python3",
            &worker,
            &source,
            descriptor,
            &functions,
        )
        .unwrap_err();
        assert!(stale.contains("does not match its descriptor inputs"));
        let changed = scalar_bridge_identity(
            descriptor,
            "ops",
            "jet_py_ops",
            "python3",
            &source,
            &worker,
            &functions,
        )
        .unwrap();
        assert_ne!(identity, changed);
        compile_scalar_sidecar_with_identity(
            &root,
            &changed,
            "jet_py_ops",
            "python3",
            &worker,
            &source,
            descriptor,
            &functions,
        )
        .unwrap();
        assert!(root
            .join(".bridges")
            .join(changed)
            .join("libjet_py_ops.a")
            .is_file());
        let _ = fs::remove_dir_all(root);
    }
}
