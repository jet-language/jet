// D-CLAIM1 / D-TEST-EVIDENCE1: one typed test-evidence boundary for every
// execution tier. Foundation owns the jet.evidence/v1 record and report shape;
// this file owns only test meaning and validation. Hosts provide execution and
// I/O, then hand the typed values here.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestEvidenceError {
    SchemaConflict { expected: u16, found: u16 },
    ProducerConflict {
        expected: EvidenceProducerKind,
        found: EvidenceProducerKind,
    },
    EmptyReportId,
    EmptyClaimId,
    EmptyEvidenceId,
    IdentityConflict {
        field: &'static str,
        expected: String,
        found: String,
    },
    InvalidOutcome { outcome: EvidenceOutcome },
    InvalidExpectation {
        expectation: EvidenceExpectation,
        outcome: EvidenceOutcome,
    },
    Io {
        operation: String,
        path: String,
        message: String,
    },
    ContradictoryOutcome {
        claim_id: String,
        existing: EvidenceOutcome,
        found: EvidenceOutcome,
    },
    Foundation(EvidenceMergeError),
}

impl TestEvidenceError {
    pub fn io(
        operation: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::Io {
            operation: operation.into(),
            path: path.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for TestEvidenceError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SchemaConflict { expected, found } => write!(
                output,
                "test evidence requires jet.evidence/v{expected}, found v{found}"
            ),
            Self::ProducerConflict { expected, found } => write!(
                output,
                "test evidence producer must be {}, found {}",
                expected.as_str(),
                found.as_str()
            ),
            Self::EmptyReportId => output.write_str("test evidence report id must not be empty"),
            Self::EmptyClaimId => output.write_str("test evidence claim id must not be empty"),
            Self::EmptyEvidenceId => output.write_str("test evidence evidence id must not be empty"),
            Self::IdentityConflict {
                field,
                expected,
                found,
            } => write!(
                output,
                "test evidence identity conflict for {field}: {expected} versus {found}"
            ),
            Self::InvalidOutcome { outcome } => write!(
                output,
                "test evidence outcome {} is not a terminal test outcome",
                outcome.as_str()
            ),
            Self::InvalidExpectation {
                expectation,
                outcome,
            } => write!(
                output,
                "test evidence expectation {} cannot describe outcome {}",
                expectation.as_str(),
                outcome.as_str()
            ),
            Self::Io {
                operation,
                path,
                message,
            } => write!(
                output,
                "test evidence {operation} failed for {path}: {message}"
            ),
            Self::ContradictoryOutcome {
                claim_id,
                existing,
                found,
            } => write!(
                output,
                "test claim {claim_id} has contradictory terminal outcomes: {} versus {}",
                existing.as_str(),
                found.as_str()
            ),
            Self::Foundation(error) => error.fmt(output),
        }
    }
}

impl std::error::Error for TestEvidenceError {}

impl From<EvidenceMergeError> for TestEvidenceError {
    fn from(error: EvidenceMergeError) -> Self {
        Self::Foundation(error)
    }
}
impl From<EvidenceExpectationError> for TestEvidenceError {
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


fn validate_test_report(report: &EvidenceReport) -> Result<(), TestEvidenceError> {
    if report.schema_version != EVIDENCE_REPORT_VERSION {
        return Err(TestEvidenceError::SchemaConflict {
            expected: EVIDENCE_REPORT_VERSION,
            found: report.schema_version,
        });
    }
    if report.producer != EvidenceProducerKind::Test {
        return Err(TestEvidenceError::ProducerConflict {
            expected: EvidenceProducerKind::Test,
            found: report.producer,
        });
    }
    if report.identity.report_id.is_empty() {
        return Err(TestEvidenceError::EmptyReportId);
    }
    Ok(())
}

// Property passes and producer-unavailable outcomes are terminal wire records
// for a test run even though they do not count as ordinary example passes.
fn is_terminal_test_outcome(outcome: EvidenceOutcome) -> bool {
    matches!(
        outcome,
        EvidenceOutcome::Generated
            | EvidenceOutcome::Passed
            | EvidenceOutcome::Failed
            | EvidenceOutcome::Skipped
            | EvidenceOutcome::Unavailable
            | EvidenceOutcome::Error
    )
}

fn validate_test_record(
    report: &EvidenceReport,
    record: &EvidenceRecord,
) -> Result<(), TestEvidenceError> {
    if record.producer != EvidenceProducerKind::Test {
        return Err(TestEvidenceError::ProducerConflict {
            expected: EvidenceProducerKind::Test,
            found: record.producer,
        });
    }
    if record.identity.report_id != report.identity.report_id {
        return Err(TestEvidenceError::IdentityConflict {
            field: "report_id",
            expected: report.identity.report_id.clone(),
            found: record.identity.report_id.clone(),
        });
    }
    if record.identity.claim_id.is_empty() {
        return Err(TestEvidenceError::EmptyClaimId);
    }
    if record.identity.evidence_id.is_empty() {
        return Err(TestEvidenceError::EmptyEvidenceId);
    }
    if !is_terminal_test_outcome(record.outcome) {
        return Err(TestEvidenceError::InvalidOutcome {
            outcome: record.outcome,
        });
    }
    record.validate().map_err(TestEvidenceError::from)?;
    if !report.identity.claim_id.is_empty()
        && report.identity.claim_id != record.identity.claim_id
    {
        return Err(TestEvidenceError::IdentityConflict {
            field: "claim_id",
            expected: report.identity.claim_id.clone(),
            found: record.identity.claim_id.clone(),
        });
    }
    if !report.identity.evidence_id.is_empty()
        && report.identity.evidence_id != record.identity.evidence_id
    {
        return Err(TestEvidenceError::IdentityConflict {
            field: "evidence_id",
            expected: report.identity.evidence_id.clone(),
            found: record.identity.evidence_id.clone(),
        });
    }
    Ok(())
}

fn validate_test_records(report: &EvidenceReport) -> Result<(), TestEvidenceError> {
    let mut outcomes = std::collections::BTreeMap::<&str, EvidenceOutcome>::new();
    let mut evidence = std::collections::BTreeMap::<&str, &EvidenceRecord>::new();
    for record in &report.records {
        validate_test_record(report, record)?;
        if let Some(existing) = outcomes.get(record.identity.claim_id.as_str()) {
            if *existing != record.outcome {
                return Err(TestEvidenceError::ContradictoryOutcome {
                    claim_id: record.identity.claim_id.clone(),
                    existing: *existing,
                    found: record.outcome,
                });
            }
        } else {
            outcomes.insert(record.identity.claim_id.as_str(), record.outcome);
        }
        if let Some(existing) = evidence.get(record.identity.evidence_id.as_str()) {
            if *existing != record {
                return Err(TestEvidenceError::Foundation(
                    EvidenceMergeError::RecordConflict {
                        evidence_id: record.identity.evidence_id.clone(),
                    },
                ));
            }
        } else {
            evidence.insert(record.identity.evidence_id.as_str(), record);
        }
    }
    Ok(())
}

/// Add one test case without re-encoding or rewriting its identity, source,
/// build/revision provenance, attachments, completeness, diagnostics, or chain.
/// Equal duplicate records are idempotent; a claim cannot finish twice with
/// different terminal outcomes.
pub fn record_test_case(
    report: &mut EvidenceReport,
    record: EvidenceRecord,
) -> Result<(), TestEvidenceError> {
    validate_test_report(report)?;
    validate_test_record(report, &record)?;
    if let Some(existing) = report
        .records
        .iter()
        .find(|existing| existing.identity.claim_id == record.identity.claim_id)
    {
        if existing.outcome != record.outcome {
            return Err(TestEvidenceError::ContradictoryOutcome {
                claim_id: record.identity.claim_id.clone(),
                existing: existing.outcome,
                found: record.outcome,
            });
        }
    }
    let result = if record.derivation.is_some() {
        report.add_record(record)
    } else {
        let derivation = record.checked_derivation();
        report.add_record_with_derivation(record, derivation)
    };
    result.map_err(TestEvidenceError::Foundation)
}

/// Validate and canonicalize a test report. Re-adding through Foundation's
/// typed insertion path deduplicates equal records, rejects conflicting IDs,
/// merges completeness, and applies its deterministic record order. Report
/// attachments, diagnostics, root provenance, and every record link remain
/// untouched.
pub fn finalize_test_report(
    mut report: EvidenceReport,
) -> Result<EvidenceReport, TestEvidenceError> {
    validate_test_report(&report)?;
    validate_test_records(&report)?;
    let records = std::mem::take(&mut report.records);
    for record in records {
        record_test_case(&mut report, record)?;
    }
    Ok(report)
}

/// Validate the typed test report and return Foundation's deterministic
/// projection. No terminal text or JSON is parsed and no host policy is applied.
pub fn query_test_evidence(
    report: &EvidenceReport,
    claim_id: Option<&str>,
) -> Result<Vec<EvidenceProjection>, TestEvidenceError> {
    validate_test_report(report)?;
    validate_test_records(report)?;
    Ok(report.query(claim_id))
}
