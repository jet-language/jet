// D-REPORT-TEST1=A: one test-result report for generated harnesses and
// `jet prove`. Keep this source dependency-free so AOT can embed it and the
// compiler can call the same renderer for its host-side report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetTestReport {
    pub evidence: EvidenceReport,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub expected_failures: usize,
    pub unexpected_passes: usize,
}

impl JetTestReport {
    /// Build the human and machine test view from the one canonical evidence
    /// report. The caller owns insertion/finalization; this boundary validates
    /// the report without re-recording any evidence.
    pub fn from_evidence(report: &EvidenceReport) -> Result<Self, TestEvidenceError> {
        query_test_evidence(report, None)?;
        let mut view = Self {
            evidence: report.clone(),
            passed: 0,
            failed: 0,
            skipped: 0,
            expected_failures: 0,
            unexpected_passes: 0,
        };
        for record in &view.evidence.records {
            // Runtime records are diagnostic observations emitted while a
            // test body fails.  The harness' Unit record is the terminal
            // test outcome; counting both would turn one expected failure
            // into two while dropping the raw diagnostic would lose evidence.
            if !matches!(
                record.kind,
                EvidenceKind::Unit | EvidenceKind::Property | EvidenceKind::Doctest
            ) {
                continue;
            }
            if record.is_unexpected_pass() {
                view.unexpected_passes += 1;
                continue;
            }
            if record.is_expected_failure() {
                view.expected_failures += 1;
                continue;
            }
            match record.outcome {
                EvidenceOutcome::Passed => view.passed += 1,
                EvidenceOutcome::Failed | EvidenceOutcome::Error => view.failed += 1,
                EvidenceOutcome::Skipped => view.skipped += 1,
                _ => {}
            }
        }
        Ok(view)
    }

    /// D-TEST-XFAIL1=A counts expected failures and unexpected passes apart
    /// from ordinary pass/fail. Almost no run has either, so naming them
    /// unconditionally would put two zeroes on the end of every summary a
    /// person reads. They appear when they happened; the JSON always carries
    /// them, because a machine reader wants a fixed shape.
    pub fn summary(&self) -> String {
        let mut out = format!(
            "{} passed, {} failed, {} skipped",
            self.passed, self.failed, self.skipped
        );
        if self.expected_failures > 0 {
            out.push_str(&format!(", {} expected-fail", self.expected_failures));
        }
        if self.unexpected_passes > 0 {
            out.push_str(&format!(", {} unexpected-pass", self.unexpected_passes));
        }
        out
    }

    pub fn status_envelope(&self) -> Result<StatusEnvelope, TestEvidenceError> {
        let test = StatusValue::object(
            StatusFields::new()
                .with("failed", self.failed)
                .with("passed", self.passed)
                .with("skipped", self.skipped)
                .with("selected", self.selected())
                .with("expectedFailures", self.expected_failures)
                .with("unexpectedPasses", self.unexpected_passes),
        );
        Ok(StatusEnvelope::new(
            "test",
            self.failed == 0 && self.unexpected_passes == 0,
        )
        .with_field("test", test)
        .with_field("evidence", evidence_status_value(&self.evidence)))
    }

    pub fn json(&self) -> String {
        self.status_envelope()
            .expect("test report status construction cannot fail")
            .json()
    }

    fn selected(&self) -> usize {
        self.passed + self.failed + self.skipped + self.expected_failures + self.unexpected_passes
    }


    /// Render one failed/error evidence record using the established test
    /// report frame. All structured evidence stays owned by `self.evidence`;
    /// this is only the terminal view.
    pub fn render_record(record: &EvidenceRecord) -> String {
        let diagnostic = record.diagnostics.first();
        let code = diagnostic.map_or("E3001", |value| value.code.as_str());
        let message = if record.detail.is_empty() {
            diagnostic.map_or_else(|| record.outcome.as_str(), |value| value.message.as_str())
        } else {
            record.detail.as_str()
        };
        let heading = match record.outcome {
            EvidenceOutcome::Error => "Error",
            EvidenceOutcome::Failed => "Stop",
            _ => "Test",
        };
        let mut out = format!("  {heading} [{code}]: {message}\n");
        if !record.source.path.is_empty() && record.source.line != 0 {
            out.push_str(&format!(
                "    --> {}:{}\n",
                record.source.path, record.source.line
            ));
            if record.source.column != 0 {
                out.push_str(&format!(
                    "      {}^\n",
                    " ".repeat(record.source.column.saturating_sub(1) as usize)
                ));
            }
        }
        if let Some(diagnostic) = diagnostic {
            if diagnostic.message != message {
                for line in diagnostic.message.lines() {
                    out.push_str(&format!("    {line}\n"));
                }
            }
        }
        for attachment in &record.attachments {
            if attachment.name == "detail" || attachment.value.is_empty() {
                continue;
            }
            for line in attachment.value.lines() {
                out.push_str(&format!("    {}: {line}\n", attachment.name));
            }
        }
        out.push_str(&format!("  More: jet-lang.dev/e/{code}\n"));
        out
    }
}

fn evidence_status_value(report: &EvidenceReport) -> StatusValue {
    let mut records: Vec<_> = report.records.iter().collect();
    records.sort_by(|left, right| {
        left.source
            .path
            .cmp(&right.source.path)
            .then_with(|| left.source.line.cmp(&right.source.line))
            .then_with(|| left.source.column.cmp(&right.source.column))
            .then_with(|| left.producer.cmp(&right.producer))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.identity.evidence_id.cmp(&right.identity.evidence_id))
    });
    StatusValue::object(
        StatusFields::new()
            .with("schema", format!("{EVIDENCE_REPORT_SCHEMA}/v{}", report.schema_version))
            .with(
                "identity",
                StatusValue::object(
                    StatusFields::new()
                        .with("claim", report.identity.claim_id.clone())
                        .with("evidence", report.identity.evidence_id.clone())
                        .with("report", report.identity.report_id.clone()),
                ),
            )
            .with("producer", report.producer.as_str())
            .with("source", evidence_source_status_value(&report.source))
            .with("build", evidence_build_status_value(&report.build))
            .with("revision", evidence_revision_status_value(&report.revision))
            .with(
                "attachments",
                StatusValue::array(sorted_report_values(&report.attachments).into_iter().map(
                    |attachment| {
                        StatusValue::object(
                            StatusFields::new()
                                .with("name", attachment.name.clone())
                                .with("value", attachment.value.clone()),
                        )
                    },
                )),
            )
            .with(
                "diagnostics",
                StatusValue::array(sorted_report_values(&report.diagnostics).into_iter().map(
                    |diagnostic| {
                        StatusValue::object(
                            StatusFields::new()
                                .with("code", diagnostic.code.clone())
                                .with("message", diagnostic.message.clone())
                                .with(
                                    "source",
                                    diagnostic
                                        .source
                                        .as_ref()
                                        .map_or(StatusValue::Null, evidence_source_status_value),
                                ),
                        )
                    },
                )),
            )
            .with(
                "completeness",
                evidence_completeness_status_value(&report.completeness),
            )
            .with(
                "evidence",
                StatusValue::array(
                    records
                        .into_iter()
                        .map(evidence_record_status_value),
                ),
            ),
    )
}

fn evidence_record_status_value(record: &EvidenceRecord) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with(
                "attachments",
                StatusValue::array(sorted_report_values(&record.attachments).into_iter().map(
                    |attachment| {
                        StatusValue::object(
                            StatusFields::new()
                                .with("name", attachment.name.clone())
                                .with("value", attachment.value.clone()),
                        )
                    },
                )),
            )
            .with(
                "chain",
                StatusValue::array(sorted_report_values(&record.chain).into_iter().map(|link| {
                    StatusValue::object(
                        StatusFields::new()
                            .with("evidence", link.evidence_id.clone())
                            .with("relation", link.relation.clone()),
                    )
                })),
            )
            .with("claim", record.identity.claim_id.clone())
            .with(
                "completeness",
                evidence_completeness_status_value(&record.completeness),
            )
            .with("count", record.count)
            .with("detail", record.detail.clone())
            .with(
                "diagnostics",
                StatusValue::array(sorted_report_values(&record.diagnostics).into_iter().map(
                    |diagnostic| {
                        StatusValue::object(
                            StatusFields::new()
                                .with("code", diagnostic.code.clone())
                                .with("message", diagnostic.message.clone())
                                .with(
                                    "source",
                                    diagnostic
                                        .source
                                        .as_ref()
                                        .map_or(StatusValue::Null, evidence_source_status_value),
                                ),
                        )
                    },
                )),
            )
            .with("evidence", record.identity.evidence_id.clone())
            .with("expectation", record.expectation.as_str())
            .with("kind", record.kind.as_str())
            .with("facet", record.facet.as_str())
            .with("outcome", record.outcome.as_str())
            .with("producer", record.producer.as_str())
            .with("source", evidence_source_status_value(&record.source)),
    )
}

fn evidence_source_status_value(source: &EvidenceSource) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("column", source.column)
            .with("line", source.line)
            .with("path", source.path.clone()),
    )
}

fn evidence_build_status_value(build: &EvidenceBuild) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("profile", build.profile.clone())
            .with("target", build.target.clone())
            .with("toolchain", build.toolchain.clone()),
    )
}

fn evidence_revision_status_value(revision: &EvidenceRevision) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("build", revision.build.clone())
            .with("revision", revision.revision.clone())
            .with("source", revision.source.clone()),
    )
}

fn evidence_completeness_status_value(completeness: &EvidenceCompleteness) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with(
                "reason",
                completeness
                    .reason()
                    .map_or(StatusValue::Null, |reason| StatusValue::String(reason.to_owned())),
            )
            .with("state", completeness.state()),
    )
}

fn sorted_report_values<T: Ord>(values: &[T]) -> Vec<&T> {
    let mut values: Vec<_> = values.iter().collect();
    values.sort();
    values
}

#[cfg(test)]
mod test_report_tests {
    use super::*;

    fn evidence_report() -> EvidenceReport {
        let mut report = EvidenceReport::new(
            "test-report",
            EvidenceProducerKind::Test,
            EvidenceSource::default(),
            EvidenceBuild::default(),
            EvidenceRevision::default(),
        );
        let cases = [
            (0, 0, "pass", 1, EvidenceExpectation::Ordinary),
            (0, 1, "xfail", 2, EvidenceExpectation::ExpectedFailure),
            (0, 0, "xpass", 3, EvidenceExpectation::ExpectedFailure),
            (0, 2, "skip", 4, EvidenceExpectation::Ordinary),
            (0, 4, "error", 5, EvidenceExpectation::Ordinary),
            // Keep raw runtime observations in the report without counting
            // them as additional terminal test outcomes.
            (2, 1, "raw-xfail", 6, EvidenceExpectation::ExpectedFailure),
            (2, 1, "raw-fail", 7, EvidenceExpectation::Ordinary),
        ];
        for (kind, state, name, line, expectation) in cases {
            let record = EvidenceRecord::from_test_codes(
                kind,
                state,
                name,
                "",
                "tests.jet",
                line,
            )
            .unwrap()
            .with_report_id("test-report")
            .with_expectation(expectation)
            .unwrap();
            record_test_case(&mut report, record).unwrap();
        }
        finalize_test_report(report).unwrap()
    }

    #[test]
    fn canonical_records_drive_counts_and_status() {
        let view = JetTestReport::from_evidence(&evidence_report()).unwrap();
        assert_eq!(view.passed, 1);
        assert_eq!(view.failed, 1);
        assert_eq!(view.skipped, 1);
        assert_eq!(view.expected_failures, 1);
        assert_eq!(view.unexpected_passes, 1);
        assert_eq!(view.evidence.records.len(), 7);
        assert!(view
            .evidence
            .records
            .iter()
            .any(|record| record.identity.claim_id == "raw-xfail"));
        assert!(view
            .evidence
            .records
            .iter()
            .any(|record| record.identity.claim_id == "raw-fail"));

        let status = view.status_envelope().unwrap().json();
        assert!(status.contains("\"schema\":\"jet.status/v1\""));
        assert!(status.contains("\"claim\":\"xfail\""));
        assert!(status.contains("\"expectation\":\"expected_failure\""));
        assert!(status.contains("\"outcome\":\"error\""));
    }
}
