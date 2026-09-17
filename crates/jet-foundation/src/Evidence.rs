// Typed evidence reports shared by compile-time and runtime producers.
//
// The report is deliberately independent of a backend. Producers only append
// typed records to the bounded wire stream; the host merges and projects those
// records without parsing terminal text or carrying a second JSON shape.

pub use crate::Facts::{
    DerivationDisposition, DerivationIdentity, DerivationMethod, DerivationObservation,
    DerivationPayload, DerivationRecord, DerivationRef,
};

pub const EVIDENCE_REPORT_SCHEMA: &str = "jet.evidence";
pub const EVIDENCE_REPORT_VERSION: u16 = 3;
pub const EVIDENCE_REPORT_MAGIC: &[u8; 8] = b"JETEVID3";
pub const EVIDENCE_REPORT_ENV: &str = "JET_EVIDENCE_REPORT";
pub const EVIDENCE_REPORT_ID_ENV: &str = "JET_EVIDENCE_REPORT_ID";
pub const EVIDENCE_TOOLCHAIN_ENV: &str = "JET_EVIDENCE_TOOLCHAIN";
pub const EVIDENCE_TARGET_ENV: &str = "JET_EVIDENCE_TARGET";
pub const EVIDENCE_PROFILE_ENV: &str = "JET_EVIDENCE_PROFILE";
pub const EVIDENCE_SOURCE_REVISION_ENV: &str = "JET_EVIDENCE_SOURCE_REVISION";
pub const EVIDENCE_BUILD_REVISION_ENV: &str = "JET_EVIDENCE_BUILD_REVISION";
pub const EVIDENCE_REVISION_ENV: &str = "JET_EVIDENCE_REVISION";

const MAX_REPORT_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECORDS: usize = 10_000;
const MAX_FIELD_BYTES: usize = 1024 * 1024;
const MAX_COLLECTION_ITEMS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum EvidenceProducerKind {
    Test = 0,
    Prove = 1,
    Compile = 2,
    Runtime = 3,
}

impl EvidenceProducerKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Test => "jet-test",
            Self::Prove => "jet-prove",
            Self::Compile => "jet-compile",
            Self::Runtime => "jet-runtime",
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Test),
            1 => Some(Self::Prove),
            2 => Some(Self::Compile),
            3 => Some(Self::Runtime),
            _ => None,
        }
    }

    pub const fn code(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum EvidenceKind {
    Unit = 0,
    Contract = 1,
    Runtime = 2,
    Property = 3,
    Doctest = 4,
}

impl EvidenceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Contract => "contract",
            Self::Runtime => "runtime",
            Self::Property => "property",
            Self::Doctest => "doctest",
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Unit),
            1 => Some(Self::Contract),
            2 => Some(Self::Runtime),
            3 => Some(Self::Property),
            4 => Some(Self::Doctest),
            _ => None,
        }
    }

    pub const fn code(self) -> u8 {
        self as u8
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum EvidenceFacet {
    #[default]
    All = 0,
    Refinements = 1,
    Effects = 2,
    Taint = 3,
    Contracts = 4,
    Tests = 5,
    Budgets = 6,
    Replay = 7,
    Solver = 8,
}

impl EvidenceFacet {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Refinements => "refinements",
            Self::Effects => "effects",
            Self::Taint => "taint",
            Self::Contracts => "contracts",
            Self::Tests => "tests",
            Self::Budgets => "budgets",
            Self::Replay => "replay",
            Self::Solver => "solver",
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::All),
            1 => Some(Self::Refinements),
            2 => Some(Self::Effects),
            3 => Some(Self::Taint),
            4 => Some(Self::Contracts),
            5 => Some(Self::Tests),
            6 => Some(Self::Budgets),
            7 => Some(Self::Replay),
            8 => Some(Self::Solver),
            _ => None,
        }
    }

    pub const fn code(self) -> u8 {
        self as u8
    }
    pub const fn for_kind(kind: EvidenceKind) -> Self {
        match kind {
            EvidenceKind::Contract => Self::Contracts,
            EvidenceKind::Unit | EvidenceKind::Runtime | EvidenceKind::Property | EvidenceKind::Doctest => {
                Self::Tests
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum EvidenceOutcome {
    Proved = 0,
    Generated = 1,
    Passed = 2,
    Failed = 3,
    Checked = 4,
    Unchecked = 5,
    Unavailable = 6,
    Incomplete = 7,
    NotObserved = 8,
    Skipped = 9,
    /// Test execution reached a terminal infrastructure/runtime error.
    ///
    /// This is distinct from `Unavailable`: an unavailable producer did not
    /// observe the test, while an error is an observed terminal outcome.
    Error = 10,
}

impl EvidenceOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proved => "proved",
            Self::Generated => "generated",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Checked => "checked",
            Self::Unchecked => "unchecked",
            Self::Unavailable => "unavailable",
            Self::Incomplete => "incomplete",
            Self::NotObserved => "not_observed",
            Self::Skipped => "skipped",
            Self::Error => "error",
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Proved),
            1 => Some(Self::Generated),
            2 => Some(Self::Passed),
            3 => Some(Self::Failed),
            4 => Some(Self::Checked),
            5 => Some(Self::Unchecked),
            6 => Some(Self::Unavailable),
            7 => Some(Self::Incomplete),
            8 => Some(Self::NotObserved),
            9 => Some(Self::Skipped),
            10 => Some(Self::Error),
            _ => None,
        }
    }

    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn is_failure(self) -> bool {
        matches!(self, Self::Failed | Self::Error)
    }

    pub const fn is_incomplete(self) -> bool {
        matches!(
            self,
            Self::Unavailable | Self::Incomplete | Self::NotObserved | Self::Skipped
        )
    }

    /// State values used by the test producer wire protocol.
    pub const fn from_test_state(state: u8) -> Option<Self> {
        match state {
            0 => Some(Self::Passed),
            1 => Some(Self::Failed),
            2 => Some(Self::Skipped),
            3 => Some(Self::Unavailable),
            4 => Some(Self::Error),
            _ => None,
        }
    }

    pub const fn test_state(self) -> Option<u8> {
        match self {
            Self::Passed => Some(0),
            Self::Failed => Some(1),
            Self::Skipped => Some(2),
            Self::Unavailable => Some(3),
            Self::Error => Some(4),
            _ => None,
        }
    }
}


#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum EvidenceExpectation {
    #[default]
    Ordinary = 0,
    ExpectedFailure = 1,
}

impl EvidenceExpectation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ordinary => "ordinary",
            Self::ExpectedFailure => "expected_failure",
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Ordinary),
            1 => Some(Self::ExpectedFailure),
            _ => None,
        }
    }

    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn validate(self, outcome: EvidenceOutcome) -> Result<(), EvidenceExpectationError> {
        if matches!(self, Self::ExpectedFailure)
            && !matches!(outcome, EvidenceOutcome::Failed | EvidenceOutcome::Passed)
        {
            return Err(EvidenceExpectationError::InvalidOutcome {
                expectation: self,
                outcome,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceExpectationError {
    InvalidOutcome {
        expectation: EvidenceExpectation,
        outcome: EvidenceOutcome,
    },
}

impl std::fmt::Display for EvidenceExpectationError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOutcome {
                expectation,
                outcome,
            } => write!(
                output,
                "evidence expectation {} cannot describe outcome {}",
                expectation.as_str(),
                outcome.as_str()
            ),
        }
    }
}

impl std::error::Error for EvidenceExpectationError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceCompleteness {
    Complete,
    Incomplete { reason: String },
    Unavailable { reason: String },
}

impl EvidenceCompleteness {
    pub const fn state(&self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Incomplete { .. } => "incomplete",
            Self::Unavailable { .. } => "unavailable",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Complete => None,
            Self::Incomplete { reason } | Self::Unavailable { reason } => Some(reason),
        }
    }

    pub fn merge(&mut self, other: &Self) {
        let rank = |value: &Self| match value {
            Self::Complete => 0u8,
            Self::Incomplete { .. } => 1,
            Self::Unavailable { .. } => 2,
        };
        if rank(other) > rank(self) {
            *self = other.clone();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceIdentity {
    pub report_id: String,
    pub claim_id: String,
    pub evidence_id: String,
}

impl EvidenceIdentity {
    pub fn new(
        report_id: impl Into<String>,
        claim_id: impl Into<String>,
        evidence_id: impl Into<String>,
    ) -> Self {
        Self {
            report_id: report_id.into(),
            claim_id: claim_id.into(),
            evidence_id: evidence_id.into(),
        }
    }

    pub fn for_record(
        report_id: &str,
        claim_id: &str,
        producer: EvidenceProducerKind,
        kind: EvidenceKind,
        source: &EvidenceSource,
        count: u64,
        detail: &str,
    ) -> Self {
        let evidence_id = stable_token(&[
            report_id,
            claim_id,
            producer.as_str(),
            kind.as_str(),
            &source.path,
            &source.line.to_string(),
            &source.column.to_string(),
            &count.to_string(),
            detail,
        ]);
        Self::new(report_id, claim_id, evidence_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceSource {
    pub path: String,
    pub line: u32,
    pub column: u32,
}

impl EvidenceSource {
    pub fn new(path: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            path: path.into(),
            line,
            column,
        }
    }
}

impl Default for EvidenceSource {
    fn default() -> Self {
        Self::new(String::new(), 0, 0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceBuild {
    pub toolchain: String,
    pub target: String,
    pub profile: String,
}

impl EvidenceBuild {
    pub fn new(
        toolchain: impl Into<String>,
        target: impl Into<String>,
        profile: impl Into<String>,
    ) -> Self {
        Self {
            toolchain: toolchain.into(),
            target: target.into(),
            profile: profile.into(),
        }
    }

    fn from_environment() -> Self {
        Self::new(
            std::env::var(EVIDENCE_TOOLCHAIN_ENV).unwrap_or_default(),
            std::env::var(EVIDENCE_TARGET_ENV).unwrap_or_default(),
            std::env::var(EVIDENCE_PROFILE_ENV).unwrap_or_default(),
        )
    }
}

impl Default for EvidenceBuild {
    fn default() -> Self {
        Self::new(String::new(), String::new(), String::new())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceRevision {
    pub source: String,
    pub build: String,
    pub revision: String,
}

impl EvidenceRevision {
    pub fn new(
        source: impl Into<String>,
        build: impl Into<String>,
        revision: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            build: build.into(),
            revision: revision.into(),
        }
    }

    fn from_environment() -> Self {
        Self::new(
            std::env::var(EVIDENCE_SOURCE_REVISION_ENV).unwrap_or_default(),
            std::env::var(EVIDENCE_BUILD_REVISION_ENV).unwrap_or_default(),
            std::env::var(EVIDENCE_REVISION_ENV).unwrap_or_default(),
        )
    }
}

impl Default for EvidenceRevision {
    fn default() -> Self {
        Self::new(String::new(), String::new(), String::new())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceAttachment {
    pub name: String,
    pub value: String,
}

impl EvidenceAttachment {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceDiagnostic {
    pub code: String,
    pub message: String,
    pub source: Option<EvidenceSource>,
}

impl EvidenceDiagnostic {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        source: Option<EvidenceSource>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            source,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceChainLink {
    pub evidence_id: String,
    pub relation: String,
}

impl EvidenceChainLink {
    pub fn new(evidence_id: impl Into<String>, relation: impl Into<String>) -> Self {
        Self {
            evidence_id: evidence_id.into(),
            relation: relation.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceRecord {
    pub identity: EvidenceIdentity,
    pub kind: EvidenceKind,
    pub facet: EvidenceFacet,
    pub producer: EvidenceProducerKind,
    pub outcome: EvidenceOutcome,
    pub expectation: EvidenceExpectation,
    pub count: u64,
    pub source: EvidenceSource,
    pub build: EvidenceBuild,
    pub revision: EvidenceRevision,
    pub detail: String,
    pub attachments: Vec<EvidenceAttachment>,
    pub diagnostics: Vec<EvidenceDiagnostic>,
    pub completeness: EvidenceCompleteness,
    pub chain: Vec<EvidenceChainLink>,
    /// Interned reason link. The canonical payload is held once by the
    /// containing `EvidenceReport`.
    pub derivation: Option<DerivationRef>,
}

impl EvidenceRecord {
    pub fn new(
        identity: EvidenceIdentity,
        kind: EvidenceKind,
        producer: EvidenceProducerKind,
        outcome: EvidenceOutcome,
        count: u64,
        source: EvidenceSource,
        build: EvidenceBuild,
        revision: EvidenceRevision,
    ) -> Self {
        let completeness = match outcome {
            EvidenceOutcome::Unavailable => EvidenceCompleteness::Unavailable {
                reason: "producer unavailable".into(),
            },
            EvidenceOutcome::Incomplete
            | EvidenceOutcome::NotObserved
            | EvidenceOutcome::Skipped => EvidenceCompleteness::Incomplete {
                reason: outcome.as_str().into(),
            },
            _ => EvidenceCompleteness::Complete,
        };
        Self {
            identity,
            kind,
            facet: EvidenceFacet::for_kind(kind),
            producer,
            outcome,
            expectation: EvidenceExpectation::Ordinary,
            count,
            source,
            build,
            revision,
            detail: String::new(),
            attachments: Vec::new(),
            diagnostics: Vec::new(),
            completeness,
            chain: Vec::new(),
            derivation: None,
        }
    }
    pub fn validate(&self) -> Result<(), EvidenceExpectationError> {
        self.expectation.validate(self.outcome)
    }
    pub const fn is_expected_failure(&self) -> bool {
        matches!(
            (self.outcome, self.expectation),
            (EvidenceOutcome::Failed, EvidenceExpectation::ExpectedFailure)
        )
    }

    pub const fn is_unexpected_pass(&self) -> bool {
        matches!(
            (self.outcome, self.expectation),
            (EvidenceOutcome::Passed, EvidenceExpectation::ExpectedFailure)
        )
    }


    pub fn with_expectation(
        mut self,
        expectation: EvidenceExpectation,
    ) -> Result<Self, EvidenceExpectationError> {
        self.set_expectation(expectation)?;
        Ok(self)
    }

    pub fn set_expectation(
        &mut self,
        expectation: EvidenceExpectation,
    ) -> Result<(), EvidenceExpectationError> {
        expectation.validate(self.outcome)?;
        self.expectation = expectation;
        Ok(())
    }

    pub fn from_test_codes(
        kind_code: u8,
        state: u8,
        name: &str,
        message: &str,
        file: &str,
        line: u32,
    ) -> Result<Self, String> {
        let kind = EvidenceKind::from_code(kind_code)
            .ok_or_else(|| format!("unknown evidence kind {kind_code}"))?;
        let outcome = match (kind, state) {
            // A property pass is a generated-input claim, not merely a
            // hand-authored example. Preserve that grade in the typed
            // evidence record consumed by `jet inspect claims`.
            (EvidenceKind::Property, 0) => EvidenceOutcome::Generated,
            (_, state) => EvidenceOutcome::from_test_state(state)
                .ok_or_else(|| format!("unknown evidence state {state}"))?,
        };
        let source_line = if matches!(kind, EvidenceKind::Property) {
            0
        } else {
            line
        };
        let count = if matches!(kind, EvidenceKind::Property) {
            u64::from(line)
        } else {
            1
        };
        let source = EvidenceSource::new(file, source_line, 1);
        let report_id = std::env::var(EVIDENCE_REPORT_ID_ENV).unwrap_or_else(|_| "test-run".into());
        let identity = EvidenceIdentity::for_record(
            &report_id,
            if name.is_empty() { "<anonymous>" } else { name },
            EvidenceProducerKind::Test,
            kind,
            &source,
            count,
            message,
        );
        let mut record = Self::new(
            identity,
            kind,
            EvidenceProducerKind::Test,
            outcome,
            count,
            source,
            EvidenceBuild::from_environment(),
            EvidenceRevision::from_environment(),
        );
        record.detail = message.to_owned();
        if !message.is_empty() {
            record
                .attachments
                .push(EvidenceAttachment::new("detail", message));
        }
        if matches!(outcome, EvidenceOutcome::Failed | EvidenceOutcome::Error) {
            let code = if matches!(kind, EvidenceKind::Runtime) && name.starts_with('E') {
                name
            } else {
                "E3001"
            };
            record.diagnostics.push(EvidenceDiagnostic::new(
                code,
                message,
                Some(record.source.clone()),
            ));
        }
        Ok(record)
    }
    pub fn with_facet(mut self, facet: EvidenceFacet) -> Self {
        self.facet = facet;
        self
    }

    pub fn set_facet(&mut self, facet: EvidenceFacet) {
        self.facet = facet;
    }
    pub fn with_derivation(mut self, derivation: &DerivationRecord) -> Self {
        self.derivation = Some(derivation.reference());
        self
    }

    pub fn set_derivation(&mut self, derivation: &DerivationRecord) {
        self.derivation = Some(derivation.reference());
    }

    pub fn clear_derivation(&mut self) {
        self.derivation = None;
    }
    /// Build the canonical derivation for a record emitted by a test/runtime
    /// producer. This keeps producer callsites on the shared relation rather
    /// than allowing a second claim/evidence shape.
    pub fn checked_derivation(&self) -> DerivationRecord {
        let method = if matches!(self.kind, EvidenceKind::Property) {
            DerivationMethod::SampledAgreement
        } else {
            DerivationMethod::RecordedExecution
        };
        let disposition = if self.outcome.is_incomplete()
            || matches!(self.outcome, EvidenceOutcome::Unchecked)
        {
            DerivationDisposition::Unavailable
        } else {
            DerivationDisposition::Current
        };
        DerivationRecord::new(
            self.identity.evidence_id.clone(),
            self.identity.claim_id.clone(),
            self.producer.as_str(),
            method,
            "evidence-record",
            [
                self.identity.claim_id.clone(),
                self.identity.evidence_id.clone(),
            ],
            DerivationIdentity::new(
                self.revision.source.clone(),
                self.revision.build.clone(),
                self.revision.revision.clone(),
                self.build.target.clone(),
            ),
        )
        .with_disposition(disposition)
    }

    pub fn with_report_id(mut self, report_id: &str) -> Self {
        if self.identity.report_id == report_id {
            return self;
        }
        self.identity = EvidenceIdentity::for_record(
            report_id,
            &self.identity.claim_id,
            self.producer,
            self.kind,
            &self.source,
            self.count,
            &self.detail,
        );
        self
    }

    pub fn state_code(&self) -> u8 {
        self.outcome.test_state().unwrap_or_else(|| {
            if self.outcome.is_failure() {
                1
            } else if self.outcome.is_incomplete() {
                3
            } else {
                0
            }
        })
    }

    pub fn claim_name(&self) -> &str {
        &self.identity.claim_id
    }

    pub fn seed_or_detail(&self) -> &str {
        &self.source.path
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceReport {
    pub schema_version: u16,
    pub identity: EvidenceIdentity,
    pub producer: EvidenceProducerKind,
    pub source: EvidenceSource,
    pub build: EvidenceBuild,
    pub revision: EvidenceRevision,
    pub records: Vec<EvidenceRecord>,
    /// Intern table for checked derivation payloads. Records retain only the
    /// stable `DerivationRef` id.
    pub derivations: Vec<DerivationRecord>,
    pub attachments: Vec<EvidenceAttachment>,
    pub diagnostics: Vec<EvidenceDiagnostic>,
    pub completeness: EvidenceCompleteness,
}

impl EvidenceReport {
    pub fn new(
        report_id: impl Into<String>,
        producer: EvidenceProducerKind,
        source: EvidenceSource,
        build: EvidenceBuild,
        revision: EvidenceRevision,
    ) -> Self {
        let report_id = report_id.into();
        Self {
            schema_version: EVIDENCE_REPORT_VERSION,
            identity: EvidenceIdentity::new(report_id, "", ""),
            producer,
            source,
            build,
            revision,
            records: Vec::new(),
            derivations: Vec::new(),
            attachments: Vec::new(),
            diagnostics: Vec::new(),
            completeness: EvidenceCompleteness::Complete,
        }
    }

    pub fn from_records(report_id: impl Into<String>, records: Vec<EvidenceRecord>) -> Self {
        Self::try_from_records(report_id, records)
            .expect("evidence records in one report must share report identity")
    }

    pub fn try_from_records(
        report_id: impl Into<String>,
        records: Vec<EvidenceRecord>,
    ) -> Result<Self, EvidenceMergeError> {
        Self::try_from_records_with_derivations(report_id, records, Vec::new())
    }

    pub fn try_from_records_with_derivations(
        report_id: impl Into<String>,
        records: Vec<EvidenceRecord>,
        derivations: Vec<DerivationRecord>,
    ) -> Result<Self, EvidenceMergeError> {
        let report_id = report_id.into();
        let producer = records
            .first()
            .map(|record| record.producer)
            .unwrap_or(EvidenceProducerKind::Test);
        let source = records
            .first()
            .map(|record| record.source.clone())
            .unwrap_or_default();
        let build = records
            .first()
            .map(|record| record.build.clone())
            .unwrap_or_default();
        let revision = records
            .first()
            .map(|record| record.revision.clone())
            .unwrap_or_default();
        let mut report = Self::new(report_id, producer, source, build, revision);
        for derivation in derivations {
            derivation
                .validate()
                .map_err(|message| EvidenceMergeError::InvalidDerivation { message })?;
            report.intern_derivation(derivation);
        }
        for record in records {
            let result = if record.derivation.is_some() {
                report.add_record(record)
            } else {
                let derivation = record.checked_derivation();
                report.add_record_with_derivation(record, derivation)
            };
            result?;
        }
        Ok(report)
    }

    /// Intern one canonical derivation payload and return the stable link used
    /// by evidence rows and all downstream projections.
    pub fn intern_derivation(&mut self, derivation: DerivationRecord) -> DerivationRef {
        if let Some(existing) = self
            .derivations
            .iter()
            .find(|existing| existing.id == derivation.id)
        {
            return existing.reference();
        }
        let reference = derivation.reference();
        self.derivations.push(derivation);
        self.derivations.sort_by(|left, right| left.id.cmp(&right.id));
        reference
    }

    pub fn derivation(&self, reference: &DerivationRef) -> Option<&DerivationRecord> {
        self.derivations
            .iter()
            .find(|derivation| derivation.id == reference.id)
    }
    /// Mark every current derivation depending on changed source/build/run or
    /// target inputs stale. Unknown and terminal dispositions remain explicit.
    pub fn invalidate_derivations(&mut self, current: &DerivationIdentity) -> usize {
        self.derivations
            .iter_mut()
            .map(|derivation| usize::from(derivation.invalidate_if_identity_changed(current)))
            .sum()
    }


    pub fn add_record_with_derivation(
        &mut self,
        mut record: EvidenceRecord,
        derivation: DerivationRecord,
    ) -> Result<(), EvidenceMergeError> {
        derivation
            .validate()
            .map_err(|message| EvidenceMergeError::InvalidDerivation { message })?;
        let reference = self.intern_derivation(derivation);
        record.derivation = Some(reference);
        self.add_record(record)
    }

    pub fn add_record(&mut self, record: EvidenceRecord) -> Result<(), EvidenceMergeError> {
        record.validate().map_err(EvidenceMergeError::from)?;
        if record.identity.report_id != self.identity.report_id {
            return Err(EvidenceMergeError::IdentityConflict {
                field: "report_id",
                expected: self.identity.report_id.clone(),
                found: record.identity.report_id,
            });
        }
        if let Some(reference) = &record.derivation {
            let Some(derivation) = self.derivation(reference) else {
                return Err(EvidenceMergeError::MissingDerivation {
                    evidence_id: record.identity.evidence_id.clone(),
                    derivation_id: reference.id.clone(),
                });
            };
            if derivation.identity.source != record.revision.source
                || derivation.identity.build != record.revision.build
                || derivation.identity.run != record.revision.revision
                || derivation.identity.target != record.build.target
            {
                return Err(EvidenceMergeError::InvalidDerivation {
                    message: format!(
                        "derivation {} provenance does not match evidence {}",
                        derivation.id, record.identity.evidence_id
                    ),
                });
            }
        }
        if let Some(existing) = self
            .records
            .iter()
            .find(|existing| existing.identity.evidence_id == record.identity.evidence_id)
        {
            if existing == &record {
                return Ok(());
            }
            return Err(EvidenceMergeError::RecordConflict {
                evidence_id: record.identity.evidence_id,
            });
        }
        self.completeness.merge(&record.completeness);
        self.records.push(record);
        self.records.sort_by(record_order);
        Ok(())
    }

    pub fn merge(reports: &[EvidenceReport]) -> Result<Self, EvidenceMergeError> {
        let Some(first) = reports.first() else {
            return Err(EvidenceMergeError::Empty);
        };
        let mut merged = first.clone();
        let initial_records = merged.records.clone();
        for record in initial_records {
            merged.add_record(record)?;
        }
        for report in &reports[1..] {
            if report.schema_version != merged.schema_version {
                return Err(EvidenceMergeError::SchemaConflict {
                    expected: merged.schema_version,
                    found: report.schema_version,
                });
            }
            check_identity(
                "report_id",
                &merged.identity.report_id,
                &report.identity.report_id,
            )?;
            check_identity(
                "claim_id",
                &merged.identity.claim_id,
                &report.identity.claim_id,
            )?;
            check_identity(
                "evidence_id",
                &merged.identity.evidence_id,
                &report.identity.evidence_id,
            )?;
            // Records carry producer/source/build/revision provenance. Those
            // fields may differ when `jet prove` combines its own record with
            // a child `jet test` record.
            merged.completeness.merge(&report.completeness);
            for derivation in &report.derivations {
                merged.intern_derivation(derivation.clone());
            }
            for record in &report.records {
                merged.add_record(record.clone())?;
            }
            for attachment in &report.attachments {
                if !merged.attachments.contains(attachment) {
                    merged.attachments.push(attachment.clone());
                }
            }
            for diagnostic in &report.diagnostics {
                if !merged.diagnostics.contains(diagnostic) {
                    merged.diagnostics.push(diagnostic.clone());
                }
            }
        }
        merged.attachments.sort();
        merged.diagnostics.sort();
        Ok(merged)
    }

    pub fn project(&self) -> Vec<EvidenceProjection> {
        self.query(None)
    }

    pub fn query(&self, claim_id: Option<&str>) -> Vec<EvidenceProjection> {
        let mut rows: Vec<_> = self
            .records
            .iter()
            .filter(|record| claim_id.map_or(true, |claim| record.identity.claim_id == claim))
            .map(EvidenceProjection::from_record)
            .collect();
        rows.sort_by(projection_order);
        rows
    }

    pub fn is_complete(&self) -> bool {
        matches!(self.completeness, EvidenceCompleteness::Complete)
            && self
                .records
                .iter()
                .all(|record| matches!(record.completeness, EvidenceCompleteness::Complete))
    }

    pub fn json(&self) -> String {
        let mut out = String::new();
        out.push('{');
        push_json_key(&mut out, "schema");
        push_json_string(&mut out, &format!("{EVIDENCE_REPORT_SCHEMA}/v{}", self.schema_version));
        out.push(',');
        push_json_key(&mut out, "identity");
        out.push('{');
        push_json_key(&mut out, "claim");
        push_json_string(&mut out, &self.identity.claim_id);
        out.push(',');
        push_json_key(&mut out, "evidence");
        push_json_string(&mut out, &self.identity.evidence_id);
        out.push(',');
        push_json_key(&mut out, "report");
        push_json_string(&mut out, &self.identity.report_id);
        out.push('}');
        out.push(',');
        push_json_key(&mut out, "producer");
        push_json_string(&mut out, self.producer.as_str());
        out.push(',');
        push_json_key(&mut out, "source");
        source_json(&mut out, &self.source);
        out.push(',');
        push_json_key(&mut out, "build");
        build_json(&mut out, &self.build);
        out.push(',');
        push_json_key(&mut out, "revision");
        revision_json(&mut out, &self.revision);
        out.push(',');
        push_json_key(&mut out, "attachments");
        attachments_json(&mut out, &self.attachments);
        out.push(',');
        push_json_key(&mut out, "diagnostics");
        diagnostics_json(&mut out, &self.diagnostics);
        out.push(',');
        push_json_key(&mut out, "completeness");
        completeness_json(&mut out, &self.completeness);
        out.push(',');
        push_json_key(&mut out, "derivations");
        out.push('[');
        for (index, derivation) in self.derivations.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&derivation.to_json());
        }
        out.push(']');
        out.push(',');
        push_json_key(&mut out, "evidence");
        out.push('[');
        for (index, record) in self.sorted_records().iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            record_json(&mut out, record);
        }
        out.push(']');
        out.push('}');
        out
    }

    pub fn read(path: &std::path::Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let (records, derivations) = decode_report_bytes(&bytes)?;
        let report_id = records
            .first()
            .map(|record| record.identity.report_id.clone())
            .unwrap_or_else(|| "test-run".into());
        let report = Self::try_from_records_with_derivations(report_id, records, derivations)
            .map_err(|error| error.to_string())?;
        Ok(report)
    }
    /// Encode this report through the same framed stream used by append-only
    /// producers. Every referenced derivation is emitted with its full typed
    /// definition; a ref without a definition is rejected before encoding.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let mut output = EVIDENCE_REPORT_MAGIC.to_vec();
        output.extend_from_slice(&EVIDENCE_REPORT_VERSION.to_be_bytes());
        for record in &self.records {
            let mut frame = Vec::new();
            match &record.derivation {
                Some(reference) => {
                    let derivation = self.derivation(reference).ok_or_else(|| {
                        format!(
                            "evidence record {} references missing derivation {}",
                            record.identity.evidence_id, reference.id
                        )
                    })?;
                    encode_record_with_derivation(record, derivation, &mut frame)?;
                }
                None => encode_record(record, &mut frame)?,
            }
            output.extend_from_slice(&frame);
            if output.len() > MAX_REPORT_BYTES {
                return Err("evidence report exceeds the 16 MiB limit".into());
            }
        }
        Ok(output)
    }

    /// Persist this report through the same framed stream used by append-only
    /// producers.
    pub fn write(&self, path: &std::path::Path) -> Result<(), String> {
        std::fs::write(path, self.encode()?).map_err(|error| error.to_string())
    }


    fn sorted_records(&self) -> Vec<&EvidenceRecord> {
        let mut records: Vec<_> = self.records.iter().collect();
        records.sort_by(|left, right| record_order(left, right));
        records
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceProjection {
    pub evidence_id: String,
    pub claim_id: String,
    pub kind: EvidenceKind,
    pub facet: EvidenceFacet,
    pub producer: EvidenceProducerKind,
    pub expectation: EvidenceExpectation,
    pub outcome: EvidenceOutcome,
    pub source: EvidenceSource,
    pub completeness: EvidenceCompleteness,
    pub chain: Vec<EvidenceChainLink>,
    pub derivation: Option<DerivationRef>,
}

impl EvidenceProjection {
    fn from_record(record: &EvidenceRecord) -> Self {
        let mut chain = record.chain.clone();
        chain.sort();
        Self {
            evidence_id: record.identity.evidence_id.clone(),
            claim_id: record.identity.claim_id.clone(),
            kind: record.kind,
            facet: record.facet,
            producer: record.producer,
            expectation: record.expectation,
            outcome: record.outcome,
            source: record.source.clone(),
            completeness: record.completeness.clone(),
            chain,
            derivation: record.derivation.clone(),
        }
    }

    pub fn name(&self) -> String {
        format!(
            "{}:{} via {}",
            self.source.path,
            self.source.line,
            self.producer.as_str()
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceMergeError {
    Empty,
    SchemaConflict { expected: u16, found: u16 },
    IdentityConflict {
        field: &'static str,
        expected: String,
        found: String,
    },
    RecordConflict { evidence_id: String },
    MissingDerivation {
        evidence_id: String,
        derivation_id: String,
    },
    InvalidDerivation { message: String },
    InvalidExpectation {
        expectation: EvidenceExpectation,
        outcome: EvidenceOutcome,
    },
}

impl std::fmt::Display for EvidenceMergeError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => output.write_str("cannot merge an empty evidence report set"),
            Self::SchemaConflict { expected, found } => {
                write!(output, "evidence report schema conflict: {expected} versus {found}")
            }
            Self::IdentityConflict {
                field,
                expected,
                found,
            } => write!(
                output,
                "evidence report identity conflict for {field}: {expected} versus {found}"
            ),
            Self::RecordConflict { evidence_id } => {
                write!(output, "evidence record conflict for {evidence_id}")
            }
            Self::MissingDerivation {
                evidence_id,
                derivation_id,
            } => write!(
                output,
                "evidence record {evidence_id} references missing derivation {derivation_id}"
            ),
            Self::InvalidDerivation { message } => {
                write!(output, "invalid derivation: {message}")
            }
            Self::InvalidExpectation {
                expectation,
                outcome,
            } => write!(
                output,
                "evidence expectation {} cannot describe outcome {}",
                expectation.as_str(),
                outcome.as_str()
            ),
        }
    }
}

impl std::error::Error for EvidenceMergeError {}
impl From<EvidenceExpectationError> for EvidenceMergeError {
    fn from(error: EvidenceExpectationError) -> Self {
        match error {
            EvidenceExpectationError::InvalidOutcome {
                expectation,
                outcome,
            } => Self::InvalidExpectation {
                expectation,
                outcome,
            },
        }
    }
}

std::thread_local! {
    static TEST_EXPECTATION: std::cell::Cell<EvidenceExpectation> =
        const { std::cell::Cell::new(EvidenceExpectation::Ordinary) };
}

/// Keep evidence from one test attached to its declared expectation, including
/// runtime stops caught by the harness. Nested scopes restore even on unwind.
pub fn jet_evidence_with_expectation<R>(expected_failure: bool, run: impl FnOnce() -> R) -> R {
    struct Restore(EvidenceExpectation);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_EXPECTATION.with(|expectation| expectation.set(self.0));
        }
    }
    let expectation = if expected_failure {
        EvidenceExpectation::ExpectedFailure
    } else {
        EvidenceExpectation::Ordinary
    };
    let _restore = Restore(TEST_EXPECTATION.with(|current| current.replace(expectation)));
    run()
}


pub fn write_record_from_codes(
    path: &std::path::Path,
    kind: u8,
    state: u8,
    name: &str,
    message: &str,
    file: &str,
    line: u32,
) -> Result<(), String> {
    let mut record = EvidenceRecord::from_test_codes(kind, state, name, message, file, line)?;
    if matches!(record.outcome, EvidenceOutcome::Passed | EvidenceOutcome::Failed) {
        record
            .set_expectation(TEST_EXPECTATION.with(|expectation| expectation.get()))
            .map_err(|error| error.to_string())?;
    }
    let derivation = record.checked_derivation();
    record.set_derivation(&derivation);
    write_record_with_derivation(path, &record, &derivation)
}
pub fn write_record_with_derivation(
    path: &std::path::Path,
    record: &EvidenceRecord,
    derivation: &DerivationRecord,
) -> Result<(), String> {
    let mut frame = Vec::new();
    encode_record_with_derivation(record, derivation, &mut frame)?;
    append_frame(path, &frame)
}


/// Generated AOT test/contract code calls this function.  The function is
/// intentionally part of this source file so the same typed producer exists in
/// Foundation and in the emitted executable, with no textual copy of the wire
/// format.
pub fn jet_evidence_record_write(
    kind: u8,
    state: u8,
    name: &str,
    message: &str,
    file: &str,
    line: u32,
) {
    let Ok(path) = std::env::var(EVIDENCE_REPORT_ENV) else {
        return;
    };
    let _ = write_record_from_codes(
        std::path::Path::new(&path),
        kind,
        state,
        name,
        message,
        file,
        line,
    );
}

pub fn decode_report_bytes(
    bytes: &[u8],
) -> Result<(Vec<EvidenceRecord>, Vec<DerivationRecord>), String> {
    if bytes.len() > MAX_REPORT_BYTES {
        return Err("evidence report exceeds the 16 MiB limit".into());
    }
    if bytes.get(..8) != Some(EVIDENCE_REPORT_MAGIC) {
        return Err("invalid evidence report magic".into());
    }
    let mut at = 8usize;
    let version = read_u16(bytes, &mut at)?;
    if version != EVIDENCE_REPORT_VERSION {
        return Err(format!(
            "unsupported evidence report schema version {version}"
        ));
    }
    let mut records = Vec::new();
    let mut derivations = Vec::new();
    while at < bytes.len() {
        if records.len() >= MAX_RECORDS {
            return Err("too many evidence report records".into());
        }
        let (record, derivation) = decode_record(bytes, &mut at)?;
        if let Some(derivation) = derivation {
            if record.derivation.as_ref() != Some(&derivation.reference()) {
                return Err(format!(
                    "evidence record {} has a derivation reference mismatch",
                    record.identity.evidence_id
                ));
            }
            if let Some(existing) = derivations
                .iter()
                .find(|existing: &&DerivationRecord| existing.id == derivation.id)
            {
                if existing != &derivation {
                    return Err(format!(
                        "conflicting derivation definition {}",
                        derivation.id
                    ));
                }
            } else {
                derivations.push(derivation);
            }
        } else if record.derivation.is_some() {
            return Err(format!(
                "evidence record {} has an unresolved derivation reference",
                record.identity.evidence_id
            ));
        }
        records.push(record);
    }
    Ok((records, derivations))
}


static EVIDENCE_REPORT_APPEND_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

fn append_frame(path: &std::path::Path, frame: &[u8]) -> Result<(), String> {
    if frame.len() > MAX_REPORT_BYTES {
        return Err("evidence record exceeds the 16 MiB limit".into());
    }
    let _guard = EVIDENCE_REPORT_APPEND_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut options = std::fs::OpenOptions::new();
    options.write(true).append(true).create(true);
    let mut report = options.open(path).map_err(|error| error.to_string())?;
    let length = report
        .metadata()
        .map_err(|error| error.to_string())?
        .len() as usize;
    if length == 0 {
        use std::io::Write as _;
        report
            .write_all(EVIDENCE_REPORT_MAGIC)
            .map_err(|error| error.to_string())?;
        report
            .write_all(&EVIDENCE_REPORT_VERSION.to_be_bytes())
            .map_err(|error| error.to_string())?;
    } else if length < 10 {
        return Err("truncated evidence report header".into());
    } else {
        let mut header = [0u8; 10];
        use std::io::Read as _;
        let mut reader = std::fs::File::open(path).map_err(|error| error.to_string())?;
        reader
            .read_exact(&mut header)
            .map_err(|error| error.to_string())?;
        if header[..8] != EVIDENCE_REPORT_MAGIC[..]
            || u16::from_be_bytes([header[8], header[9]]) != EVIDENCE_REPORT_VERSION
        {
            return Err("evidence report schema conflict".into());
        }
    }
    use std::io::Write as _;
    report.write_all(frame).map_err(|error| error.to_string())?;
    report.flush().map_err(|error| error.to_string())
}

fn encode_record(record: &EvidenceRecord, output: &mut Vec<u8>) -> Result<(), String> {
    if record.derivation.is_some() {
        return Err(
            "evidence record has a derivation; encode with its canonical definition".into(),
        );
    }
    encode_record_body(record, output)?;
    output.push(0);
    Ok(())
}

fn encode_record_with_derivation(
    record: &EvidenceRecord,
    derivation: &DerivationRecord,
    output: &mut Vec<u8>,
) -> Result<(), String> {
    record.validate().map_err(|error| error.to_string())?;
    let reference = derivation.reference();
    if record.derivation.as_ref() != Some(&reference) {
        return Err(format!(
            "evidence record {} does not reference derivation {}",
            record.identity.evidence_id, derivation.id
        ));
    }
    encode_record_body(record, output)?;
    output.push(1);
    encode_derivation(derivation, output)
}

fn encode_record_body(record: &EvidenceRecord, output: &mut Vec<u8>) -> Result<(), String> {
    record.validate().map_err(|error| error.to_string())?;
    output.push(record.kind.code());
    output.push(record.producer.code());
    output.push(record.facet.code());
    // Expected failure uses the otherwise-unused high bit of the bounded
    // outcome code. Version 3 appends a derivation definition marker.
    let outcome_code = record.outcome.code()
        | if matches!(record.expectation, EvidenceExpectation::ExpectedFailure) {
            0x80
        } else {
            0
        };
    output.push(outcome_code);
    output.push(completeness_code(&record.completeness));
    output.extend_from_slice(&record.count.to_be_bytes());
    output.extend_from_slice(&record.source.line.to_be_bytes());
    output.extend_from_slice(&record.source.column.to_be_bytes());
    for value in [
        &record.identity.report_id,
        &record.identity.claim_id,
        &record.identity.evidence_id,
        &record.source.path,
        &record.build.toolchain,
        &record.build.target,
        &record.build.profile,
        &record.revision.source,
        &record.revision.build,
        &record.revision.revision,
        &record.detail,
    ] {
        write_string(output, value)?;
    }
    write_collection_len(output, record.attachments.len())?;
    for attachment in sorted_attachments(&record.attachments) {
        write_string(output, &attachment.name)?;
        write_string(output, &attachment.value)?;
    }
    write_collection_len(output, record.diagnostics.len())?;
    for diagnostic in sorted_diagnostics(&record.diagnostics) {
        write_string(output, &diagnostic.code)?;
        write_string(output, &diagnostic.message)?;
        match &diagnostic.source {
            Some(source) => {
                output.push(1);
                output.extend_from_slice(&source.line.to_be_bytes());
                output.extend_from_slice(&source.column.to_be_bytes());
                write_string(output, &source.path)?;
            }
            None => output.push(0),
        }
    }
    write_collection_len(output, record.chain.len())?;
    for link in sorted_chain(&record.chain) {
        write_string(output, &link.evidence_id)?;
        write_string(output, &link.relation)?;
    }
    write_string(
        output,
        record.completeness.reason().unwrap_or_default(),
    )?;
    write_string(
        output,
        record
            .derivation
            .as_ref()
            .map(|reference| reference.id.as_str())
            .unwrap_or_default(),
    )?;
    Ok(())
}

fn encode_derivation(
    derivation: &DerivationRecord,
    output: &mut Vec<u8>,
) -> Result<(), String> {
    derivation.validate()?;
    write_string(output, &derivation.id)?;
    write_string(output, &derivation.subject)?;
    write_string(output, &derivation.claim)?;
    write_string(output, &derivation.producer)?;
    output.push(derivation_method_code(derivation.method));
    write_string(output, &derivation.rule)?;
    write_collection_len(output, derivation.premises.len())?;
    for premise in &derivation.premises {
        write_string(output, premise)?;
    }
    for value in [
        &derivation.identity.source,
        &derivation.identity.build,
        &derivation.identity.run,
        &derivation.identity.target,
    ] {
        write_string(output, value)?;
    }
    write_collection_len(output, derivation.assumptions.len())?;
    for assumption in &derivation.assumptions {
        write_string(output, assumption)?;
    }
    output.push(derivation_disposition_code(derivation.disposition));
    match &derivation.observation {
        Some(observation) => {
            output.push(1);
            write_optional_string(output, observation.event_id.as_deref())?;
            write_optional_string(output, observation.counterexample_id.as_deref())?;
        }
        None => output.push(0),
    }
    match &derivation.payload {
        Some(DerivationPayload::FrameSchedule(schedule)) => {
            output.push(1);
            write_string(output, &schedule.canonical_json())?;
        }
        None => output.push(0),
    }
    Ok(())
}

fn decode_derivation(bytes: &[u8], at: &mut usize) -> Result<DerivationRecord, String> {
    let id = read_string(bytes, at)?;
    let subject = read_string(bytes, at)?;
    let claim = read_string(bytes, at)?;
    let producer = read_string(bytes, at)?;
    let method = derivation_method_from_code(read_byte(bytes, at)?)?;
    let rule = read_string(bytes, at)?;
    let premise_count = read_collection_len(bytes, at)?;
    let mut premises = Vec::with_capacity(premise_count);
    for _ in 0..premise_count {
        premises.push(read_string(bytes, at)?);
    }
    let identity = DerivationIdentity::new(
        read_string(bytes, at)?,
        read_string(bytes, at)?,
        read_string(bytes, at)?,
        read_string(bytes, at)?,
    );
    let assumption_count = read_collection_len(bytes, at)?;
    let mut assumptions = Vec::with_capacity(assumption_count);
    for _ in 0..assumption_count {
        assumptions.push(read_string(bytes, at)?);
    }
    let disposition = derivation_disposition_from_code(read_byte(bytes, at)?)?;
    let observation = match read_byte(bytes, at)? {
        0 => None,
        1 => Some(DerivationObservation {
            event_id: read_optional_string(bytes, at)?,
            counterexample_id: read_optional_string(bytes, at)?,
        }),
        value => return Err(format!("unknown derivation observation marker {value}")),
    };
    let payload = match read_byte(bytes, at)? {
        0 => None,
        1 => Some(DerivationPayload::FrameSchedule(
            crate::ResourceSchedule::JetFrameSchedule::from_canonical_json(&read_string(
                bytes, at,
            )?)?,
        )),
        value => return Err(format!("unknown derivation payload marker {value}")),
    };
    let mut derivation = DerivationRecord::new(
        subject,
        claim,
        producer,
        method,
        rule,
        premises,
        identity,
    )
    .with_assumptions(assumptions)
    .with_disposition(disposition);
    if let Some(observation) = observation {
        derivation = derivation.with_observation(observation);
    }
    if let Some(payload) = payload {
        derivation = derivation.with_payload(payload);
    }
    if derivation.id != id {
        return Err(format!("derivation {id} has a non-canonical identity"));
    }
    Ok(derivation)
}

fn derivation_method_code(method: DerivationMethod) -> u8 {
    match method {
        DerivationMethod::StaticDerivation => 0,
        DerivationMethod::FormalProof => 1,
        DerivationMethod::RecordedExecution => 2,
        DerivationMethod::SampledAgreement => 3,
        DerivationMethod::ExternalAssumption => 4,
    }
}

fn derivation_method_from_code(code: u8) -> Result<DerivationMethod, String> {
    match code {
        0 => Ok(DerivationMethod::StaticDerivation),
        1 => Ok(DerivationMethod::FormalProof),
        2 => Ok(DerivationMethod::RecordedExecution),
        3 => Ok(DerivationMethod::SampledAgreement),
        4 => Ok(DerivationMethod::ExternalAssumption),
        _ => Err(format!("unknown derivation method {code}")),
    }
}

fn derivation_disposition_code(disposition: DerivationDisposition) -> u8 {
    match disposition {
        DerivationDisposition::Current => 0,
        DerivationDisposition::Stale => 1,
        DerivationDisposition::Expired => 2,
        DerivationDisposition::Redacted => 3,
        DerivationDisposition::Unavailable => 4,
        DerivationDisposition::Unsupported => 5,
        DerivationDisposition::BudgetExhausted => 6,
        DerivationDisposition::Unknown => 7,
    }
}

fn derivation_disposition_from_code(code: u8) -> Result<DerivationDisposition, String> {
    match code {
        0 => Ok(DerivationDisposition::Current),
        1 => Ok(DerivationDisposition::Stale),
        2 => Ok(DerivationDisposition::Expired),
        3 => Ok(DerivationDisposition::Redacted),
        4 => Ok(DerivationDisposition::Unavailable),
        5 => Ok(DerivationDisposition::Unsupported),
        6 => Ok(DerivationDisposition::BudgetExhausted),
        7 => Ok(DerivationDisposition::Unknown),
        _ => Err(format!("unknown derivation disposition {code}")),
    }
}

fn write_optional_string(output: &mut Vec<u8>, value: Option<&str>) -> Result<(), String> {
    match value {
        Some(value) => {
            output.push(1);
            write_string(output, value)?;
        }
        None => output.push(0),
    }
    Ok(())
}

fn read_optional_string(bytes: &[u8], at: &mut usize) -> Result<Option<String>, String> {
    match read_byte(bytes, at)? {
        0 => Ok(None),
        1 => Ok(Some(read_string(bytes, at)?)),
        value => Err(format!("unknown optional string marker {value}")),
    }
}

fn decode_record(
    bytes: &[u8],
    at: &mut usize,
) -> Result<(EvidenceRecord, Option<DerivationRecord>), String> {
    let kind_code = read_byte(bytes, at)?;
    let producer_code = read_byte(bytes, at)?;
    let facet_code = read_byte(bytes, at)?;
    let encoded_outcome = read_byte(bytes, at)?;
    let expectation = if encoded_outcome & 0x80 != 0 {
        EvidenceExpectation::ExpectedFailure
    } else {
        EvidenceExpectation::Ordinary
    };
    let outcome_code = encoded_outcome & 0x7f;
    let completeness = completeness_from_code(read_byte(bytes, at)?, bytes, at)?;
    let kind = EvidenceKind::from_code(kind_code)
        .ok_or_else(|| format!("unknown evidence kind {kind_code}"))?;
    let producer = EvidenceProducerKind::from_code(producer_code)
        .ok_or_else(|| format!("unknown evidence producer {producer_code}"))?;
    let facet = EvidenceFacet::from_code(facet_code)
        .ok_or_else(|| format!("unknown evidence facet {facet_code}"))?;
    let outcome = EvidenceOutcome::from_code(outcome_code)
        .ok_or_else(|| format!("unknown evidence outcome {outcome_code}"))?;
    expectation
        .validate(outcome)
        .map_err(|error| error.to_string())?;
    let count = read_u64(bytes, at)?;
    let line = read_u32(bytes, at)?;
    let column = read_u32(bytes, at)?;
    let report_id = read_string(bytes, at)?;
    let claim_id = read_string(bytes, at)?;
    let evidence_id = read_string(bytes, at)?;
    let source = EvidenceSource::new(read_string(bytes, at)?, line, column);
    let build = EvidenceBuild::new(
        read_string(bytes, at)?,
        read_string(bytes, at)?,
        read_string(bytes, at)?,
    );
    let revision = EvidenceRevision::new(
        read_string(bytes, at)?,
        read_string(bytes, at)?,
        read_string(bytes, at)?,
    );
    let detail = read_string(bytes, at)?;
    let attachments = decode_attachments(bytes, at)?;
    let diagnostics = decode_diagnostics(bytes, at)?;
    let chain = decode_chain(bytes, at)?;
    let reason = read_string(bytes, at)?;
    let derivation_id = read_string(bytes, at)?;
    let derivation = (!derivation_id.is_empty()).then(|| DerivationRef::new(derivation_id));
    let derivation_definition = match read_byte(bytes, at)? {
        0 => None,
        1 => Some(decode_derivation(bytes, at)?),
        value => return Err(format!("unknown derivation definition marker {value}")),
    };
    let completeness = match completeness {
        DecodedCompleteness::Complete => EvidenceCompleteness::Complete,
        DecodedCompleteness::Incomplete => EvidenceCompleteness::Incomplete { reason },
        DecodedCompleteness::Unavailable => EvidenceCompleteness::Unavailable { reason },
    };
    let record = EvidenceRecord {
        identity: EvidenceIdentity::new(report_id, claim_id, evidence_id),
        kind,
        facet,
        producer,
        outcome,
        expectation,
        count,
        source,
        build,
        revision,
        detail,
        attachments,
        diagnostics,
        completeness,
        chain,
        derivation,
    };
    Ok((record, derivation_definition))
}

fn decode_attachments(
    bytes: &[u8],
    at: &mut usize,
) -> Result<Vec<EvidenceAttachment>, String> {
    let count = read_collection_len(bytes, at)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(EvidenceAttachment::new(
            read_string(bytes, at)?,
            read_string(bytes, at)?,
        ));
    }
    Ok(values)
}

fn decode_diagnostics(
    bytes: &[u8],
    at: &mut usize,
) -> Result<Vec<EvidenceDiagnostic>, String> {
    let count = read_collection_len(bytes, at)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let code = read_string(bytes, at)?;
        let message = read_string(bytes, at)?;
        let source = if read_byte(bytes, at)? == 1 {
            let line = read_u32(bytes, at)?;
            let column = read_u32(bytes, at)?;
            let path = read_string(bytes, at)?;
            Some(EvidenceSource::new(path, line, column))
        } else {
            None
        };
        values.push(EvidenceDiagnostic::new(code, message, source));
    }
    Ok(values)
}

fn decode_chain(bytes: &[u8], at: &mut usize) -> Result<Vec<EvidenceChainLink>, String> {
    let count = read_collection_len(bytes, at)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(EvidenceChainLink::new(
            read_string(bytes, at)?,
            read_string(bytes, at)?,
        ));
    }
    Ok(values)
}

fn write_string(output: &mut Vec<u8>, value: &str) -> Result<(), String> {
    if value.len() > MAX_FIELD_BYTES {
        return Err("evidence report field exceeds the 1 MiB limit".into());
    }
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn read_string(bytes: &[u8], at: &mut usize) -> Result<String, String> {
    let length = usize::try_from(read_u64(bytes, at)?)
        .map_err(|_| "evidence report field length is too large")?;
    if length > MAX_FIELD_BYTES {
        return Err("evidence report field exceeds the 1 MiB limit".into());
    }
    let end = at
        .checked_add(length)
        .ok_or("evidence report field offset overflow")?;
    let value = bytes
        .get(*at..end)
        .ok_or("truncated evidence report field")?;
    *at = end;
    String::from_utf8(value.to_vec()).map_err(|_| "non-UTF-8 evidence report field".into())
}

fn write_collection_len(output: &mut Vec<u8>, length: usize) -> Result<(), String> {
    if length > MAX_COLLECTION_ITEMS {
        return Err("evidence report collection is too large".into());
    }
    output.extend_from_slice(&(length as u32).to_be_bytes());
    Ok(())
}

fn read_collection_len(bytes: &[u8], at: &mut usize) -> Result<usize, String> {
    let length = usize::try_from(read_u32(bytes, at)?)
        .map_err(|_| "evidence report collection length is too large")?;
    if length > MAX_COLLECTION_ITEMS {
        return Err("evidence report collection is too large".into());
    }
    Ok(length)
}

fn read_byte(bytes: &[u8], at: &mut usize) -> Result<u8, String> {
    let value = *bytes.get(*at).ok_or("truncated evidence report")?;
    *at += 1;
    Ok(value)
}

fn read_u16(bytes: &[u8], at: &mut usize) -> Result<u16, String> {
    let end = at.checked_add(2).ok_or("evidence report offset overflow")?;
    let raw = bytes.get(*at..end).ok_or("truncated evidence report")?;
    *at = end;
    Ok(u16::from_be_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], at: &mut usize) -> Result<u32, String> {
    let end = at.checked_add(4).ok_or("evidence report offset overflow")?;
    let raw = bytes.get(*at..end).ok_or("truncated evidence report")?;
    *at = end;
    Ok(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_u64(bytes: &[u8], at: &mut usize) -> Result<u64, String> {
    let end = at.checked_add(8).ok_or("evidence report offset overflow")?;
    let raw = bytes.get(*at..end).ok_or("truncated evidence report")?;
    *at = end;
    Ok(u64::from_be_bytes([
        raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
    ]))
}

#[derive(Clone, Copy)]
enum DecodedCompleteness {
    Complete,
    Incomplete,
    Unavailable,
}

fn completeness_code(completeness: &EvidenceCompleteness) -> u8 {
    match completeness {
        EvidenceCompleteness::Complete => 0,
        EvidenceCompleteness::Incomplete { .. } => 1,
        EvidenceCompleteness::Unavailable { .. } => 2,
    }
}

fn completeness_from_code(
    code: u8,
    bytes: &[u8],
    at: &mut usize,
) -> Result<DecodedCompleteness, String> {
    let _ = (bytes, at);
    match code {
        0 => Ok(DecodedCompleteness::Complete),
        1 => Ok(DecodedCompleteness::Incomplete),
        2 => Ok(DecodedCompleteness::Unavailable),
        _ => Err(format!("unknown evidence completeness {code}")),
    }
}

fn check_identity<T: PartialEq + std::fmt::Debug>(
    field: &'static str,
    expected: &T,
    found: &T,
) -> Result<(), EvidenceMergeError> {
    if expected == found {
        return Ok(());
    }
    Err(EvidenceMergeError::IdentityConflict {
        field,
        expected: format!("{expected:?}"),
        found: format!("{found:?}"),
    })
}

fn record_order(left: &EvidenceRecord, right: &EvidenceRecord) -> std::cmp::Ordering {
    left.source
        .path
        .cmp(&right.source.path)
        .then_with(|| left.source.line.cmp(&right.source.line))
        .then_with(|| left.source.column.cmp(&right.source.column))
        .then_with(|| left.producer.cmp(&right.producer))
        .then_with(|| left.kind.cmp(&right.kind))
        .then_with(|| left.facet.cmp(&right.facet))
        .then_with(|| left.identity.evidence_id.cmp(&right.identity.evidence_id))
}

fn projection_order(
    left: &EvidenceProjection,
    right: &EvidenceProjection,
) -> std::cmp::Ordering {
    left.source
        .path
        .cmp(&right.source.path)
        .then_with(|| left.source.line.cmp(&right.source.line))
        .then_with(|| left.source.column.cmp(&right.source.column))
        .then_with(|| left.producer.cmp(&right.producer))
        .then_with(|| left.facet.cmp(&right.facet))
        .then_with(|| left.evidence_id.cmp(&right.evidence_id))
}

fn sorted_attachments(values: &[EvidenceAttachment]) -> Vec<EvidenceAttachment> {
    let mut values = values.to_vec();
    values.sort();
    values
}

fn sorted_diagnostics(values: &[EvidenceDiagnostic]) -> Vec<EvidenceDiagnostic> {
    let mut values = values.to_vec();
    values.sort();
    values
}

fn sorted_chain(values: &[EvidenceChainLink]) -> Vec<EvidenceChainLink> {
    let mut values = values.to_vec();
    values.sort();
    values
}

fn stable_token(parts: &[&str]) -> String {
    let mut first = 0xcbf29ce484222325u64;
    let mut second = 0x9e3779b185ebca87u64;
    for part in parts {
        let length = (part.len() as u64).to_be_bytes();
        for byte in length.iter().chain(part.as_bytes()) {
            first ^= u64::from(*byte);
            first = first.wrapping_mul(0x100000001b3);
            second ^= first.rotate_left(17) ^ u64::from(*byte);
            second = second.wrapping_mul(0x9e3779b185ebca87);
        }
    }
    format!("{first:016x}{second:016x}")
}

fn push_json_key(output: &mut String, key: &str) {
    push_json_string(output, key);
    output.push(':');
}

fn push_json_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

fn source_json(output: &mut String, source: &EvidenceSource) {
    output.push('{');
    push_json_key(output, "column");
    output.push_str(&source.column.to_string());
    output.push(',');
    push_json_key(output, "line");
    output.push_str(&source.line.to_string());
    output.push(',');
    push_json_key(output, "path");
    push_json_string(output, &source.path);
    output.push('}');
}

fn build_json(output: &mut String, build: &EvidenceBuild) {
    output.push('{');
    push_json_key(output, "profile");
    push_json_string(output, &build.profile);
    output.push(',');
    push_json_key(output, "target");
    push_json_string(output, &build.target);
    output.push(',');
    push_json_key(output, "toolchain");
    push_json_string(output, &build.toolchain);
    output.push('}');
}

fn revision_json(output: &mut String, revision: &EvidenceRevision) {
    output.push('{');
    push_json_key(output, "build");
    push_json_string(output, &revision.build);
    output.push(',');
    push_json_key(output, "revision");
    push_json_string(output, &revision.revision);
    output.push(',');
    push_json_key(output, "source");
    push_json_string(output, &revision.source);
    output.push('}');
}

fn attachments_json(output: &mut String, values: &[EvidenceAttachment]) {
    output.push('[');
    for (index, value) in sorted_attachments(values).iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push('{');
        push_json_key(output, "name");
        push_json_string(output, &value.name);
        output.push(',');
        push_json_key(output, "value");
        push_json_string(output, &value.value);
        output.push('}');
    }
    output.push(']');
}

fn diagnostics_json(output: &mut String, values: &[EvidenceDiagnostic]) {
    output.push('[');
    for (index, value) in sorted_diagnostics(values).iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push('{');
        push_json_key(output, "code");
        push_json_string(output, &value.code);
        output.push(',');
        push_json_key(output, "message");
        push_json_string(output, &value.message);
        output.push(',');
        push_json_key(output, "source");
        match &value.source {
            Some(source) => source_json(output, source),
            None => output.push_str("null"),
        }
        output.push('}');
    }
    output.push(']');
}

fn completeness_json(output: &mut String, completeness: &EvidenceCompleteness) {
    output.push('{');
    push_json_key(output, "reason");
    match completeness.reason() {
        Some(reason) => push_json_string(output, reason),
        None => output.push_str("null"),
    }
    output.push(',');
    push_json_key(output, "state");
    push_json_string(output, completeness.state());
    output.push('}');
}

fn record_json(output: &mut String, record: &EvidenceRecord) {
    output.push('{');
    push_json_key(output, "attachments");
    attachments_json(output, &record.attachments);
    output.push(',');
    push_json_key(output, "chain");
    output.push('[');
    for (index, link) in sorted_chain(&record.chain).iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push('{');
        push_json_key(output, "evidence");
        push_json_string(output, &link.evidence_id);
        output.push(',');
        push_json_key(output, "relation");
        push_json_string(output, &link.relation);
        output.push('}');
    }
    output.push(']');
    output.push(',');
    push_json_key(output, "derivation");
    match &record.derivation {
        Some(reference) => {
            output.push('{');
            push_json_key(output, "id");
            push_json_string(output, &reference.id);
            output.push('}');
        }
        None => output.push_str("null"),
    }
    output.push(',');
    push_json_key(output, "claim");
    push_json_string(output, &record.identity.claim_id);
    output.push(',');
    push_json_key(output, "completeness");
    completeness_json(output, &record.completeness);
    output.push(',');
    push_json_key(output, "count");
    output.push_str(&record.count.to_string());
    output.push(',');
    push_json_key(output, "diagnostics");
    diagnostics_json(output, &record.diagnostics);
    output.push(',');
    push_json_key(output, "evidence");
    push_json_string(output, &record.identity.evidence_id);
    output.push(',');
    push_json_key(output, "expectation");
    push_json_string(output, record.expectation.as_str());
    output.push(',');
    push_json_key(output, "facet");
    push_json_string(output, record.facet.as_str());
    output.push(',');
    push_json_key(output, "kind");
    push_json_string(output, record.kind.as_str());
    output.push(',');
    push_json_key(output, "outcome");
    push_json_string(output, record.outcome.as_str());
    output.push(',');
    push_json_key(output, "producer");
    push_json_string(output, record.producer.as_str());
    output.push(',');
    push_json_key(output, "source");
    source_json(output, &record.source);
    output.push('}');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: u8, state: u8, name: &str, file: &str, line: u32) -> EvidenceRecord {
        EvidenceRecord::from_test_codes(kind, state, name, "", file, line).unwrap()
    }

    #[test]
    fn typed_wire_round_trip_preserves_unavailable_and_chain() {
        let mut value = record(0, 3, "missing", "a.jet", 7);
        value.chain.push(EvidenceChainLink::new("upstream", "derived-from"));
        let mut bytes = Vec::new();
        encode_record(&value, &mut bytes).unwrap();
        let mut wire = EVIDENCE_REPORT_MAGIC.to_vec();
        wire.extend_from_slice(&EVIDENCE_REPORT_VERSION.to_be_bytes());
        wire.extend_from_slice(&bytes);
        let (decoded, derivations) = decode_report_bytes(&wire).unwrap();
        assert!(derivations.is_empty());
        assert_eq!(decoded, vec![value]);
        assert!(!decoded[0].completeness.state().is_empty());
    }

    #[test]
    fn derivation_wire_round_trip_preserves_payload_and_reference() {
        let schedule = crate::ResourceSchedule::JetFrameSchedule {
            operations: Vec::new(),
            dependencies: Vec::new(),
            transfers: Vec::new(),
            reuse: Vec::new(),
            retentions: Vec::new(),
            conservative: vec!["unknown schedule".to_string()],
        };
        let derivation = DerivationRecord::new(
            "subject",
            "claim",
            "jet-sema",
            DerivationMethod::StaticDerivation,
            "checked",
            vec!["premise".to_string()],
            DerivationIdentity::new("source", "build", "", "target"),
        )
        .with_payload(DerivationPayload::FrameSchedule(schedule));
        let value = record(0, 0, "claim", "a.jet", 1).with_derivation(&derivation);
        let mut bytes = Vec::new();
        encode_record_with_derivation(&value, &derivation, &mut bytes).unwrap();
        let mut wire = EVIDENCE_REPORT_MAGIC.to_vec();
        wire.extend_from_slice(&EVIDENCE_REPORT_VERSION.to_be_bytes());
        wire.extend_from_slice(&bytes);
        let (decoded, derivations) = decode_report_bytes(&wire).unwrap();
        assert_eq!(decoded, vec![value]);
        assert_eq!(derivations, vec![derivation]);
    }

    #[test]
    fn merge_rejects_identity_and_schema_conflicts() {
        let one = EvidenceReport::from_records(
            "report",
            vec![record(0, 0, "a", "a.jet", 1).with_report_id("report")],
        );
        let two = EvidenceReport::from_records(
            "other",
            vec![record(0, 0, "b", "a.jet", 2).with_report_id("other")],
        );
        assert!(matches!(EvidenceReport::merge(&[one.clone(), two.clone()]), Err(EvidenceMergeError::IdentityConflict { field: "report_id", .. })));
        let mut bad = EvidenceReport::from_records(
            "report",
            vec![record(0, 0, "b", "a.jet", 2).with_report_id("report")],
        );
        bad.schema_version = EVIDENCE_REPORT_VERSION + 1;
        assert!(matches!(EvidenceReport::merge(&[one, bad]), Err(EvidenceMergeError::SchemaConflict { .. })));
    }

    #[test]
    fn projection_is_source_then_producer_then_identity() {
        let records = vec![
            record(0, 0, "z", "b.jet", 1).with_report_id("report"),
            record(0, 0, "a", "a.jet", 2).with_report_id("report"),
            record(0, 0, "b", "a.jet", 1).with_report_id("report"),
        ];
        let report = EvidenceReport::from_records("report", records);
        let rows = report.project();
        assert_eq!(rows[0].source.path, "a.jet");
        assert_eq!(rows[0].source.line, 1);
        assert_eq!(rows[1].source.line, 2);
        assert!(rows[0].name().contains("jet-test"));
    }
}
