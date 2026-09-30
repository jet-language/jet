//! Typed diagnostics as record values (#2517).
//!
//! The Receipt and the per-package `jet.diags` record carry diagnostics in
//! this one encoding. Every field keeps its type (code, severity, spans,
//! origins, fixes, causes) so each invocation renders them for its own
//! terminal mode instead of replaying bytes from an earlier terminal.
//!
//! Origins are stored once in an origin table. An origin whose revision is
//! one of the caller's closure sources is stored by revision only; the reader
//! supplies the bytes from its own closure. Any other origin source is
//! embedded.

use jet_foundation::Diagnostics::{
    CryptoMisuseReason, Diagnostic, DiagnosticCause, DiagnosticLabel, DiagnosticOrigin,
    FixApplicability, FixSafety, NoFixReason, NoFixReasonKind, ReportMoment, Severity, Span,
    StructuredDiagnostic, TextEdit,
};
use jet_foundation::RecordCodec::{record_map, RecordValue};
use std::collections::BTreeSet;
use std::sync::Arc;

/// The two record sections one diagnostic list encodes to.
pub struct DiagnosticSections {
    pub origins: Vec<RecordValue>,
    pub diagnostics: Vec<RecordValue>,
}

/// Encode diagnostics. `closure_revisions` holds the source revisions the
/// reader can resolve itself. Returns an error for a diagnostic payload the
/// codec does not carry; the caller then leaves the result unrecorded.
pub fn encode_diagnostics(
    diagnostics: &[Diagnostic],
    closure_revisions: &BTreeSet<&str>,
) -> Result<DiagnosticSections, String> {
    let mut origins = OriginTable::default();
    let diagnostics = diagnostics
        .iter()
        .map(|diagnostic| encode_diagnostic(diagnostic, &mut origins))
        .collect::<Result<Vec<_>, _>>()?;
    let origins = origins
        .entries
        .iter()
        .map(|origin| {
            let source = if closure_revisions.contains(origin.revision.as_str()) {
                RecordValue::Null
            } else {
                RecordValue::str(origin.source.as_str())
            };
            record_map([
                ("display", RecordValue::str(origin.display.as_str())),
                ("path", RecordValue::str(origin.path.as_str())),
                ("revision", RecordValue::str(origin.revision.as_str())),
                ("source", source),
            ])
        })
        .collect();
    Ok(DiagnosticSections {
        origins,
        diagnostics,
    })
}

/// Decode diagnostics. `source_for_revision` returns the exact source text
/// for a revision in the caller's current closure; an origin it cannot
/// resolve makes the whole list undecodable (a miss, never a replay against
/// different bytes).
pub fn decode_diagnostics(
    origins: &[RecordValue],
    diagnostics: &[RecordValue],
    source_for_revision: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<Diagnostic>, String> {
    let mut table = Vec::with_capacity(origins.len());
    for origin in origins {
        let display = text(origin, "display")?;
        let path = text(origin, "path")?;
        let revision = text(origin, "revision")?;
        let source = match optional_text(origin, "source")? {
            Some(source) => source,
            None => source_for_revision(&revision)
                .ok_or_else(|| format!("diagnostic origin `{path}` is not in the closure"))?,
        };
        let decoded = DiagnosticOrigin::new(display, path, source);
        if decoded.revision != revision {
            return Err(format!("diagnostic origin `{}` revision mismatch", decoded.path));
        }
        table.push(Arc::new(decoded));
    }
    diagnostics
        .iter()
        .map(|diagnostic| decode_diagnostic(diagnostic, &table))
        .collect()
}

#[derive(Default)]
struct OriginTable {
    entries: Vec<Arc<DiagnosticOrigin>>,
}

impl OriginTable {
    fn index(&mut self, origin: &Arc<DiagnosticOrigin>) -> usize {
        if let Some(index) = self
            .entries
            .iter()
            .position(|existing| Arc::ptr_eq(existing, origin) || **existing == **origin)
        {
            return index;
        }
        self.entries.push(origin.clone());
        self.entries.len() - 1
    }

    fn optional(&mut self, origin: Option<&Arc<DiagnosticOrigin>>) -> RecordValue {
        origin.map_or(RecordValue::Null, |origin| {
            RecordValue::Int(self.index(origin) as i64)
        })
    }
}

fn span_value(span: Span) -> RecordValue {
    RecordValue::List(vec![
        RecordValue::Int(span.start as i64),
        RecordValue::Int(span.end as i64),
    ])
}

fn optional_span(span: Option<Span>) -> RecordValue {
    span.map_or(RecordValue::Null, span_value)
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Lint => "lint",
    }
}

fn parse_severity(value: &str) -> Result<Severity, String> {
    match value {
        "error" => Ok(Severity::Error),
        "lint" => Ok(Severity::Lint),
        _ => Err(format!("unknown severity `{value}`")),
    }
}

fn parse_moment(value: &str) -> Result<ReportMoment, String> {
    match value {
        "compile" => Ok(ReportMoment::Compile),
        "run" => Ok(ReportMoment::Run),
        "test" => Ok(ReportMoment::Test),
        "tool" => Ok(ReportMoment::Tool),
        _ => Err(format!("unknown report moment `{value}`")),
    }
}

fn parse_applicability(value: &str) -> Result<FixApplicability, String> {
    match value {
        "safe" => Ok(FixApplicability::Safe),
        "suggested" => Ok(FixApplicability::Suggested),
        _ => Err(format!("unknown fix applicability `{value}`")),
    }
}

fn parse_safety(value: &str) -> Result<FixSafety, String> {
    [
        FixSafety::Formatting,
        FixSafety::BehaviorPreserving,
        FixSafety::ApiChanging,
        FixSafety::TargetChanging,
        FixSafety::NeedsReview,
    ]
    .into_iter()
    .find(|safety| safety.as_str() == value)
    .ok_or_else(|| format!("unknown fix safety `{value}`"))
}

fn crypto_reason_name(reason: CryptoMisuseReason) -> &'static str {
    match reason {
        CryptoMisuseReason::InvalidLength => "invalid_length",
        CryptoMisuseReason::NonceLength => "nonce_length",
        CryptoMisuseReason::OutputLength => "output_length",
        CryptoMisuseReason::SaltLength => "salt_length",
        CryptoMisuseReason::MemoryCost => "memory_cost",
        CryptoMisuseReason::IterationCount => "iteration_count",
        CryptoMisuseReason::LaneCount => "lane_count",
        CryptoMisuseReason::MemoryTimeCost => "memory_time_cost",
        CryptoMisuseReason::RawNonce => "raw_nonce",
        CryptoMisuseReason::RawAlgorithm => "raw_algorithm",
        CryptoMisuseReason::DeterministicEntropy => "deterministic_entropy",
    }
}

fn parse_crypto_reason(value: &str) -> Result<CryptoMisuseReason, String> {
    [
        CryptoMisuseReason::InvalidLength,
        CryptoMisuseReason::NonceLength,
        CryptoMisuseReason::OutputLength,
        CryptoMisuseReason::SaltLength,
        CryptoMisuseReason::MemoryCost,
        CryptoMisuseReason::IterationCount,
        CryptoMisuseReason::LaneCount,
        CryptoMisuseReason::MemoryTimeCost,
        CryptoMisuseReason::RawNonce,
        CryptoMisuseReason::RawAlgorithm,
        CryptoMisuseReason::DeterministicEntropy,
    ]
    .into_iter()
    .find(|reason| crypto_reason_name(*reason) == value)
    .ok_or_else(|| format!("unknown crypto misuse reason `{value}`"))
}

fn encode_edit(edit: &TextEdit) -> RecordValue {
    record_map([
        ("span", span_value(edit.span)),
        ("text", RecordValue::str(edit.new_text.as_str())),
    ])
}

fn encode_structured(structured: &StructuredDiagnostic) -> RecordValue {
    match structured {
        StructuredDiagnostic::SuggestedEdits { edits } => record_map([
            ("kind", RecordValue::str("suggested_edits")),
            ("edits", RecordValue::List(edits.iter().map(encode_edit).collect())),
        ]),
        StructuredDiagnostic::CryptoMisuse {
            reason,
            operation,
            expected,
            actual,
        } => record_map([
            ("kind", RecordValue::str("crypto_misuse")),
            ("reason", RecordValue::str(crypto_reason_name(*reason))),
            ("operation", RecordValue::str(operation.as_str())),
            ("expected", RecordValue::optional_str(expected.as_deref())),
            // `i128` does not fit the codec's integer; it is kept as text.
            ("actual", actual.map_or(RecordValue::Null, |actual| RecordValue::Str(actual.to_string()))),
        ]),
        StructuredDiagnostic::BuildError { report } => record_map([
            ("kind", RecordValue::str("build_error")),
            ("report", RecordValue::Str(report.to_json())),
        ]),
        StructuredDiagnostic::RuntimeHostFault { stdout } => record_map([
            ("kind", RecordValue::str("runtime_host_fault")),
            ("stdout", RecordValue::str(stdout.as_str())),
        ]),
    }
}

/// Every `Diagnostic` field is destructured so a new field fails to compile
/// here until the record carries it.
fn encode_diagnostic(diagnostic: &Diagnostic, origins: &mut OriginTable) -> Result<RecordValue, String> {
    let Diagnostic {
        moment,
        severity,
        code,
        what,
        why,
        fix,
        span,
        origin,
        labels,
        cause,
        edit,
        applicability,
        safety,
        no_fix_reason,
        detail,
        structured,
        decision_row,
        denial_kind,
        call_chain,
        scope_chain,
        nearest_granting_scope,
    } = diagnostic;
    if decision_row.is_some() {
        return Err(format!("diagnostic `{code}` carries a decision row"));
    }
    let labels = labels
        .iter()
        .map(|label| {
            RecordValue::List(vec![
                RecordValue::Int(label.span.start as i64),
                RecordValue::Int(label.span.end as i64),
                RecordValue::str(label.message.as_str()),
            ])
        })
        .collect();
    let cause = cause
        .iter()
        .map(|cause| {
            record_map([
                ("code", RecordValue::str(cause.code.as_str())),
                ("span", optional_span(cause.span)),
                ("origin", origins.optional(cause.origin.as_ref())),
            ])
        })
        .collect();
    let no_fix = no_fix_reason.as_ref().map_or(RecordValue::Null, |reason| {
        record_map([
            ("kind", RecordValue::str(reason.kind.as_str())),
            ("next", RecordValue::str(reason.next.as_str())),
        ])
    });
    Ok(record_map([
        ("moment", RecordValue::str(moment.as_str())),
        ("severity", RecordValue::str(severity_name(*severity))),
        ("code", RecordValue::str(code.as_str())),
        ("what", RecordValue::str(what.as_str())),
        ("why", RecordValue::str(why.as_str())),
        ("fix", RecordValue::str(fix.as_str())),
        ("span", optional_span(*span)),
        ("origin", origins.optional(origin.as_ref())),
        ("labels", RecordValue::List(labels)),
        ("cause", RecordValue::List(cause)),
        ("edit", edit.as_ref().map_or(RecordValue::Null, encode_edit)),
        ("applicability", RecordValue::optional_str(applicability.map(FixApplicability::as_str))),
        ("safety", RecordValue::optional_str(safety.map(FixSafety::as_str))),
        ("no_fix", no_fix),
        ("detail", RecordValue::optional_str(detail.as_deref())),
        ("structured", structured.as_ref().map_or(RecordValue::Null, encode_structured)),
        ("denial_kind", RecordValue::optional_str(denial_kind.as_deref())),
        ("call_chain", RecordValue::list_of_str(call_chain)),
        ("scope_chain", RecordValue::list_of_str(scope_chain)),
        (
            "nearest_granting_scope",
            RecordValue::optional_str(nearest_granting_scope.as_deref()),
        ),
    ]))
}

fn field<'a>(value: &'a RecordValue, name: &str) -> Result<&'a RecordValue, String> {
    value.field(name).map_err(|error| error.to_string())
}

fn text(value: &RecordValue, name: &str) -> Result<String, String> {
    field(value, name)?
        .as_str()
        .map(str::to_string)
        .map_err(|error| error.to_string())
}

fn optional_text(value: &RecordValue, name: &str) -> Result<Option<String>, String> {
    Ok(field(value, name)?
        .as_optional_str()
        .map_err(|error| error.to_string())?
        .map(str::to_string))
}

fn list<'a>(value: &'a RecordValue, name: &str) -> Result<&'a [RecordValue], String> {
    field(value, name)?.as_list().map_err(|error| error.to_string())
}

fn offset(value: &RecordValue) -> Result<usize, String> {
    value.as_usize().map_err(|error| error.to_string())
}

fn decode_span(value: &RecordValue) -> Result<Span, String> {
    let pair = value.as_list().map_err(|error| error.to_string())?;
    if pair.len() != 2 {
        return Err("diagnostic span must be a [start, end] pair".to_string());
    }
    Ok(Span::new(offset(&pair[0])?, offset(&pair[1])?))
}

fn decode_optional_span(value: &RecordValue) -> Result<Option<Span>, String> {
    if value.is_null() {
        return Ok(None);
    }
    decode_span(value).map(Some)
}

fn decode_origin(
    value: &RecordValue,
    origins: &[Arc<DiagnosticOrigin>],
) -> Result<Option<Arc<DiagnosticOrigin>>, String> {
    if value.is_null() {
        return Ok(None);
    }
    origins
        .get(offset(value)?)
        .cloned()
        .map(Some)
        .ok_or_else(|| "diagnostic origin index is out of range".to_string())
}

fn decode_edit(value: &RecordValue) -> Result<TextEdit, String> {
    Ok(TextEdit {
        span: decode_span(field(value, "span")?)?,
        new_text: text(value, "text")?,
    })
}

fn decode_structured(value: &RecordValue) -> Result<StructuredDiagnostic, String> {
    match text(value, "kind")?.as_str() {
        "suggested_edits" => Ok(StructuredDiagnostic::SuggestedEdits {
            edits: list(value, "edits")?
                .iter()
                .map(decode_edit)
                .collect::<Result<_, _>>()?,
        }),
        "crypto_misuse" => Ok(StructuredDiagnostic::CryptoMisuse {
            reason: parse_crypto_reason(&text(value, "reason")?)?,
            operation: text(value, "operation")?,
            expected: optional_text(value, "expected")?,
            actual: optional_text(value, "actual")?
                .map(|actual| {
                    actual
                        .parse::<i128>()
                        .map_err(|_| format!("invalid crypto misuse value `{actual}`"))
                })
                .transpose()?,
        }),
        "build_error" => Ok(StructuredDiagnostic::BuildError {
            report: jet_foundation::Outcome::JetErrorReport::from_json(&text(value, "report")?)?,
        }),
        "runtime_host_fault" => Ok(StructuredDiagnostic::RuntimeHostFault {
            stdout: text(value, "stdout")?,
        }),
        other => Err(format!("unknown structured diagnostic `{other}`")),
    }
}

fn decode_diagnostic(
    value: &RecordValue,
    origins: &[Arc<DiagnosticOrigin>],
) -> Result<Diagnostic, String> {
    let labels = list(value, "labels")?
        .iter()
        .map(|label| {
            let parts = label.as_list().map_err(|error| error.to_string())?;
            if parts.len() != 3 {
                return Err("diagnostic label must be [start, end, message]".to_string());
            }
            Ok(DiagnosticLabel {
                span: Span::new(offset(&parts[0])?, offset(&parts[1])?),
                message: parts[2].as_str().map_err(|error| error.to_string())?.to_string(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let cause = list(value, "cause")?
        .iter()
        .map(|cause| {
            Ok(DiagnosticCause {
                code: text(cause, "code")?,
                span: decode_optional_span(field(cause, "span")?)?,
                origin: decode_origin(field(cause, "origin")?, origins)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let edit = match field(value, "edit")? {
        RecordValue::Null => None,
        edit => Some(decode_edit(edit)?),
    };
    let no_fix_reason = match field(value, "no_fix")? {
        RecordValue::Null => None,
        reason => Some(NoFixReason::try_new(
            NoFixReasonKind::parse(&text(reason, "kind")?)?,
            text(reason, "next")?,
        )?),
    };
    let structured = match field(value, "structured")? {
        RecordValue::Null => None,
        structured => Some(decode_structured(structured)?),
    };
    Ok(Diagnostic {
        moment: parse_moment(&text(value, "moment")?)?,
        severity: parse_severity(&text(value, "severity")?)?,
        code: text(value, "code")?,
        what: text(value, "what")?,
        why: text(value, "why")?,
        fix: text(value, "fix")?,
        span: decode_optional_span(field(value, "span")?)?,
        origin: decode_origin(field(value, "origin")?, origins)?,
        labels,
        cause,
        edit,
        applicability: optional_text(value, "applicability")?
            .map(|value| parse_applicability(&value))
            .transpose()?,
        safety: optional_text(value, "safety")?
            .map(|value| parse_safety(&value))
            .transpose()?,
        no_fix_reason,
        detail: optional_text(value, "detail")?,
        structured,
        decision_row: None,
        denial_kind: optional_text(value, "denial_kind")?,
        call_chain: field(value, "call_chain")?
            .str_list()
            .map_err(|error| error.to_string())?,
        scope_chain: field(value, "scope_chain")?
            .str_list()
            .map_err(|error| error.to_string())?,
        nearest_granting_scope: optional_text(value, "nearest_granting_scope")?,
    })
}
