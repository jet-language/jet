/// Emit the complete typed diagnostic bridge used by the private bootstrap
/// artifact.  The generated code deliberately projects every Source field;
/// registry rows are consulted only to validate an edit channel and never to
/// fill an omitted Source value. Its `@…@` markers are resolved against the
/// binding metadata when the host glue is rendered.
pub(crate) fn append_diagnostic_codec(out: &mut String) {
    out.push_str(
        r#"
fn __jet_bootstrap_int_i64(value: &jet_foundation::Numeric::JetInt, label: &str) -> Result<i64, String> {
    value.to_i64().ok_or_else(|| format!("{label} is outside the signed 64-bit bootstrap range"))
}
fn __jet_bootstrap_int_u64(value: &jet_foundation::Numeric::JetInt, label: &str) -> Result<u64, String> {
    u64::try_from(__jet_bootstrap_int_i64(value, label)?).map_err(|_| format!("{label} is negative"))
}
fn __jet_bootstrap_u64_int(value: u64, label: &str) -> Result<jet_foundation::Numeric::JetInt, String> {
    let value = i64::try_from(value).map_err(|_| format!("{label} exceeds Source Int"))?;
    Ok(jet_foundation::Numeric::JetInt::from_i64(value))
}
fn __jet_bootstrap_usize_int(value: usize, label: &str) -> Result<jet_foundation::Numeric::JetInt, String> {
    __jet_bootstrap_u64_int(u64::try_from(value).map_err(|_| format!("{label} exceeds u64"))?, label)
}
fn __jet_bootstrap_span_to_host(value: &@t.Span@) -> Result<::jet_foundation::Diagnostics::Span, String> {
    let start = usize::try_from(__jet_bootstrap_int_u64(&value.@f.Span.start@, "diagnostic span start")?)
        .map_err(|_| "diagnostic span start exceeds usize".to_string())?;
    let end = usize::try_from(__jet_bootstrap_int_u64(&value.@f.Span.end@, "diagnostic span end")?)
        .map_err(|_| "diagnostic span end exceeds usize".to_string())?;
    if end < start { return Err("diagnostic span ends before it starts".to_string()); }
    Ok(::jet_foundation::Diagnostics::Span { start, end })
}
fn __jet_bootstrap_span_from_host(value: &::jet_foundation::Diagnostics::Span) -> Result<@t.Span@, String> {
    if value.end < value.start { return Err("diagnostic span ends before it starts".to_string()); }
    Ok(@t.Span@ {
        @f.Span.start@: __jet_bootstrap_usize_int(value.start, "diagnostic span start")?,
        @f.Span.end@: __jet_bootstrap_usize_int(value.end, "diagnostic span end")?,
    })
}
fn __jet_bootstrap_text_edit_to_host(value: &@t.TextEdit@) -> Result<::jet_foundation::Diagnostics::TextEdit, String> {
    Ok(::jet_foundation::Diagnostics::TextEdit {
        span: __jet_bootstrap_span_to_host(&value.@f.TextEdit.span@)?,
        new_text: value.@f.TextEdit.new_text@.clone(),
    })
}
fn __jet_bootstrap_text_edit_from_host(value: &::jet_foundation::Diagnostics::TextEdit) -> Result<@t.TextEdit@, String> {
    Ok(@t.TextEdit@ {
        @f.TextEdit.span@: __jet_bootstrap_span_from_host(&value.span)?,
        @f.TextEdit.new_text@: value.new_text.clone(),
    })
}
fn __jet_bootstrap_origin_to_host(value: &@t.DiagnosticOrigin@) -> ::jet_foundation::Diagnostics::DiagnosticOrigin {
    ::jet_foundation::Diagnostics::DiagnosticOrigin {
        display: value.@f.DiagnosticOrigin.display@.clone(),
        path: value.@f.DiagnosticOrigin.path@.clone(),
        source: value.@f.DiagnosticOrigin.source@.clone(),
        revision: value.@f.DiagnosticOrigin.revision@.clone(),
    }
}
fn __jet_bootstrap_origin_from_host(value: &::jet_foundation::Diagnostics::DiagnosticOrigin) -> @t.DiagnosticOrigin@ {
    @t.DiagnosticOrigin@ {
        @f.DiagnosticOrigin.display@: value.display.clone(),
        @f.DiagnosticOrigin.path@: value.path.clone(),
        @f.DiagnosticOrigin.source@: value.source.clone(),
        @f.DiagnosticOrigin.revision@: value.revision.clone(),
    }
}
/// The canonical path a Jet diagnostic's source path names. The Jet driver
/// keys sources by display path (Identity.jet `jet_driver_display_path`):
/// relative to the entry root, or canonical for a file outside it.
fn __jet_bootstrap_source_physical_path(
    snapshot: &crate::compiler_bootstrap_host::AuthorizedSourceSnapshot,
    display: &str,
) -> String {
    let Some(root) = snapshot.roots.iter().find(|root| root.identity == snapshot.entry_root_identity) else {
        return display.to_string();
    };
    if root.canonical_path.is_empty() || display.starts_with('/') {
        display.to_string()
    } else if display.is_empty() {
        root.canonical_path.clone()
    } else if root.canonical_path.ends_with('/') {
        format!("{}{display}", root.canonical_path)
    } else {
        format!("{}/{display}", root.canonical_path)
    }
}
fn __jet_bootstrap_validate_origin(
    value: &::jet_foundation::Diagnostics::DiagnosticOrigin,
    source_path: Option<&String>,
    generated_source_files: &[@t.MIRSourceFile@],
    snapshot: &crate::compiler_bootstrap_host::AuthorizedSourceSnapshot,
) -> Result<bool, String> {
    let expected_revision = ::jet_foundation::SHA256::sha256_hex(value.source.as_bytes());
    if value.revision != expected_revision {
        return Err(format!("diagnostic origin `{}` has a revision that does not match its exact source bytes", value.path));
    }
    if let Some(source_path) = source_path {
        if value.path != *source_path && value.path != __jet_bootstrap_source_physical_path(snapshot, source_path) {
            return Err(format!("diagnostic origin path `{}` disagrees with source path `{source_path}`", value.path));
        }
    }
    let mut generated = None;
    for file in generated_source_files {
        if file.@f.MIRSourceFile.path@ == value.path {
            if generated.is_some() {
                return Err(format!("Jet generated diagnostic origin path `{}` is ambiguous", value.path));
            }
            generated = Some(file);
        }
    }
    let mut selected = None;
    for root in &snapshot.roots {
        for file in root.files.iter().chain(&root.foreign_cache_files) {
            if file.path == value.path {
                if selected.is_some() {
                    return Err(format!("diagnostic origin path `{}` is ambiguous in the authorized snapshot", value.path));
                }
                selected = Some(file);
            }
        }
    }
    if generated.is_some() && selected.is_some() {
        return Err(format!("diagnostic origin path `{}` collides between generated and authorized disk sources", value.path));
    }
    let generated_origin = generated.is_some();
    let expected_source = if let Some(file) = generated {
        file.@f.MIRSourceFile.source@.as_str()
    } else {
        selected
            .ok_or_else(|| format!("diagnostic origin path `{}` is absent from the authorized snapshot and generated sidecar", value.path))?
            .source
            .as_str()
    };
    if value.source != expected_source {
        return Err(format!("diagnostic origin `{}` does not carry the authorized source snapshot", value.path));
    }
    Ok(generated_origin)
}
fn __jet_bootstrap_cause_to_host(
    value: &@t.DiagnosticCause@,
    generated_source_files: &[@t.MIRSourceFile@],
    snapshot: &crate::compiler_bootstrap_host::AuthorizedSourceSnapshot,
) -> Result<::jet_foundation::Diagnostics::DiagnosticCause, String> {
    Ok(::jet_foundation::Diagnostics::DiagnosticCause {
        code: value.@f.DiagnosticCause.code@.clone(),
        span: match value.@f.DiagnosticCause.span@.as_ref().ok() {
            Some(span) => Some(__jet_bootstrap_span_to_host(span)?),
            None => None,
        },
        origin: match value.@f.DiagnosticCause.origin@.as_ref().ok() {
            Some(origin) => {
                let origin = __jet_bootstrap_origin_to_host(origin);
                __jet_bootstrap_validate_origin(&origin, None, generated_source_files, snapshot)?;
                Some(::std::sync::Arc::new(origin))
            }
            None => None,
        },
    })
}
fn __jet_bootstrap_cause_from_host(value: &::jet_foundation::Diagnostics::DiagnosticCause) -> Result<@t.DiagnosticCause@, String> {
    Ok(@t.DiagnosticCause@ {
        @f.DiagnosticCause.code@: value.code.clone(),
        @f.DiagnosticCause.span@: match value.span.as_ref() {
            Some(span) => Ok(__jet_bootstrap_span_from_host(span)?),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
        @f.DiagnosticCause.origin@: match value.origin.as_ref() {
            Some(origin) => Ok(__jet_bootstrap_origin_from_host(origin)),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
    })
}
fn __jet_bootstrap_no_fix_to_host(value: &@t.DiagnosticNoFixReason@) -> Result<::jet_foundation::Report::NoFixReason, String> {
    let kind = match &value.@f.DiagnosticNoFixReason.kind@ {
        @v.DiagnosticNoFixReasonKind.Behavior@ => ::jet_foundation::Report::NoFixReasonKind::Behavior,
        @v.DiagnosticNoFixReasonKind.Design@ => ::jet_foundation::Report::NoFixReasonKind::Design,
        @v.DiagnosticNoFixReasonKind.Ambiguous@ => ::jet_foundation::Report::NoFixReasonKind::Ambiguous,
    };
    let reason = ::jet_foundation::Report::NoFixReason { kind, next: value.@f.DiagnosticNoFixReason.next@.clone() };
    reason.validate()?;
    Ok(reason)
}
fn __jet_bootstrap_no_fix_from_host(value: &::jet_foundation::Report::NoFixReason) -> @t.DiagnosticNoFixReason@ {
    @t.DiagnosticNoFixReason@ {
        @f.DiagnosticNoFixReason.kind@: match value.kind {
            ::jet_foundation::Report::NoFixReasonKind::Behavior => @v.DiagnosticNoFixReasonKind.Behavior@,
            ::jet_foundation::Report::NoFixReasonKind::Design => @v.DiagnosticNoFixReasonKind.Design@,
            ::jet_foundation::Report::NoFixReasonKind::Ambiguous => @v.DiagnosticNoFixReasonKind.Ambiguous@,
        },
        @f.DiagnosticNoFixReason.next@: value.next.clone(),
    }
}
"#,
    );
    out.push_str(
        r#"
fn __jet_bootstrap_error_context_to_host(value: &@t.DiagnosticErrorContextFrame@) -> Result<::jet_foundation::Outcome::JetErrorContextFrame, String> {
    Ok(::jet_foundation::Outcome::JetErrorContextFrame {
        text: value.@f.DiagnosticErrorContextFrame.text@.clone(),
        file: value.@f.DiagnosticErrorContextFrame.file@.clone(),
        line: u32::try_from(__jet_bootstrap_int_u64(&value.@f.DiagnosticErrorContextFrame.line@, "build-error context line")?)
            .map_err(|_| "build-error context line exceeds u32".to_string())?,
    })
}
fn __jet_bootstrap_error_context_from_host(value: &::jet_foundation::Outcome::JetErrorContextFrame) -> @t.DiagnosticErrorContextFrame@ {
    @t.DiagnosticErrorContextFrame@ {
        @f.DiagnosticErrorContextFrame.text@: value.text.clone(),
        @f.DiagnosticErrorContextFrame.file@: value.file.clone(),
        @f.DiagnosticErrorContextFrame.line@: jet_foundation::Numeric::JetInt::from_i64(i64::from(value.line)),
    }
}
fn __jet_bootstrap_error_conversion_to_host(value: &@t.DiagnosticErrorConversion@) -> ::jet_foundation::Outcome::JetErrorConversion {
    ::jet_foundation::Outcome::JetErrorConversion { source: value.@f.DiagnosticErrorConversion.source@.clone(), target: value.@f.DiagnosticErrorConversion.target@.clone() }
}
fn __jet_bootstrap_error_conversion_from_host(value: &::jet_foundation::Outcome::JetErrorConversion) -> @t.DiagnosticErrorConversion@ {
    @t.DiagnosticErrorConversion@ { @f.DiagnosticErrorConversion.source@: value.source.clone(), @f.DiagnosticErrorConversion.target@: value.target.clone() }
}
fn __jet_bootstrap_error_field_to_host(value: &@t.DiagnosticErrorField@) -> ::jet_foundation::Outcome::JetErrorField {
    ::jet_foundation::Outcome::JetErrorField { name: value.@f.DiagnosticErrorField.name@.clone(), value: value.@f.DiagnosticErrorField.value@.clone() }
}
fn __jet_bootstrap_error_field_from_host(value: &::jet_foundation::Outcome::JetErrorField) -> @t.DiagnosticErrorField@ {
    @t.DiagnosticErrorField@ { @f.DiagnosticErrorField.name@: value.name.clone(), @f.DiagnosticErrorField.value@: value.value.clone() }
}
fn __jet_bootstrap_error_span_to_host(value: &@t.DiagnosticErrorSpan@) -> Result<::jet_foundation::Outcome::JetErrorSpan, String> {
    let start = usize::try_from(__jet_bootstrap_int_u64(&value.@f.DiagnosticErrorSpan.start@, "build-error span start")?).map_err(|_| "build-error span start exceeds usize".to_string())?;
    let end = usize::try_from(__jet_bootstrap_int_u64(&value.@f.DiagnosticErrorSpan.end@, "build-error span end")?).map_err(|_| "build-error span end exceeds usize".to_string())?;
    if end < start { return Err("build-error span ends before it starts".to_string()); }
    Ok(::jet_foundation::Outcome::JetErrorSpan { start, end })
}
/// The Source journey frame records no column; the host frame's 1-based
/// column is 0 ("not recorded") until the Jet frame carries one.
fn __jet_bootstrap_error_journey_to_host(value: &@t.DiagnosticErrorJourneyFrame@) -> Result<::jet_foundation::Outcome::JetErrorJourneyFrame, String> {
    Ok(::jet_foundation::Outcome::JetErrorJourneyFrame {
        fn_name: value.@f.DiagnosticErrorJourneyFrame.fn_name@.clone(),
        file: value.@f.DiagnosticErrorJourneyFrame.file@.clone(),
        line: u32::try_from(__jet_bootstrap_int_u64(&value.@f.DiagnosticErrorJourneyFrame.line@, "build-error journey line")?).map_err(|_| "build-error journey line exceeds u32".to_string())?,
        column: 0,
        note: value.@f.DiagnosticErrorJourneyFrame.note@.clone(),
        hops: u32::try_from(__jet_bootstrap_int_u64(&value.@f.DiagnosticErrorJourneyFrame.hops@, "build-error journey hops")?).map_err(|_| "build-error journey hops exceeds u32".to_string())?,
    })
}
fn __jet_bootstrap_error_journey_from_host(value: &::jet_foundation::Outcome::JetErrorJourneyFrame) -> @t.DiagnosticErrorJourneyFrame@ {
    @t.DiagnosticErrorJourneyFrame@ {
        @f.DiagnosticErrorJourneyFrame.fn_name@: value.fn_name.clone(),
        @f.DiagnosticErrorJourneyFrame.file@: value.file.clone(),
        @f.DiagnosticErrorJourneyFrame.line@: jet_foundation::Numeric::JetInt::from_i64(i64::from(value.line)),
        @f.DiagnosticErrorJourneyFrame.note@: value.note.clone(),
        @f.DiagnosticErrorJourneyFrame.hops@: jet_foundation::Numeric::JetInt::from_i64(i64::from(value.hops)),
    }
}
fn __jet_bootstrap_error_details_to_host(value: &@t.DiagnosticErrorDetails@) -> Result<::jet_foundation::Outcome::JetErrorDetails, String> {
    Ok(::jet_foundation::Outcome::JetErrorDetails {
        variant: value.@f.DiagnosticErrorDetails.variant@.clone(),
        fields: value.@f.DiagnosticErrorDetails.fields@.iter().map(|field| Ok(__jet_bootstrap_error_field_to_host(field))).collect::<Result<Vec<_>, String>>()?,
        source_span: match value.@f.DiagnosticErrorDetails.source_span@.as_ref().ok() {
            Some(span) => Some(__jet_bootstrap_error_span_to_host(span)?),
            None => None,
        },
    })
}
/// The Source report keeps its whole journey in `source_journey`; it records
/// no separate origin frame, so the host report's `origin` stays empty.
fn __jet_bootstrap_build_error_to_host(value: &@t.DiagnosticBuildError@) -> Result<::jet_foundation::Outcome::JetErrorReport, String> {
    Ok(::jet_foundation::Outcome::JetErrorReport {
        code: value.@f.DiagnosticBuildError.code@.as_ref().ok().cloned(),
        message: value.@f.DiagnosticBuildError.message@.clone(),
        typed_identity: value.@f.DiagnosticBuildError.typed_identity@.as_ref().ok().cloned(),
        causes: value.@f.DiagnosticBuildError.causes@.iter().map(__jet_bootstrap_build_error_to_host).collect::<Result<Vec<_>, String>>()?,
        context_frames: value.@f.DiagnosticBuildError.context_frames@.iter().map(__jet_bootstrap_error_context_to_host).collect::<Result<Vec<_>, String>>()?,
        origin: None,
        source_journey: value.@f.DiagnosticBuildError.source_journey@.iter().map(__jet_bootstrap_error_journey_to_host).collect::<Result<Vec<_>, String>>()?,
        conversion_history: value.@f.DiagnosticBuildError.conversion_history@.iter().map(|item| Ok(__jet_bootstrap_error_conversion_to_host(item))).collect::<Result<Vec<_>, String>>()?,
        details: match value.@f.DiagnosticBuildError.details@.as_ref().ok() {
            Some(details) => Some(__jet_bootstrap_error_details_to_host(details)?),
            None => None,
        },
    })
}
fn __jet_bootstrap_build_error_from_host(value: &::jet_foundation::Outcome::JetErrorReport) -> @t.DiagnosticBuildError@ {
    @s.DiagnosticBuildError@ {
        code: match value.code.as_ref() {
            Some(code) => Ok(code.clone()),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
        message: value.message.clone(),
        typed_identity: match value.typed_identity.as_ref() {
            Some(identity) => Ok(identity.clone()),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
        causes: value.causes.iter().map(__jet_bootstrap_build_error_from_host).collect::<Vec<_>>(),
        context_frames: value.context_frames.iter().map(__jet_bootstrap_error_context_from_host).collect::<Vec<_>>(),
        source_journey: value.source_journey.iter().map(__jet_bootstrap_error_journey_from_host).collect::<Vec<_>>(),
        conversion_history: value.conversion_history.iter().map(__jet_bootstrap_error_conversion_from_host).collect::<Vec<_>>(),
        details: match value.details.as_ref() {
            Some(details) => Ok(@s.DiagnosticErrorDetails@ {
                variant: details.variant.clone(),
                fields: details.fields.iter().map(__jet_bootstrap_error_field_from_host).collect::<Vec<_>>(),
                source_span: match details.source_span.as_ref() {
                    Some(span) => Ok(@s.DiagnosticErrorSpan@ {
                        start: jet_foundation::Numeric::JetInt::from_i64(span.start as i64),
                        end: jet_foundation::Numeric::JetInt::from_i64(span.end as i64),
                    }),
                    None => Err(jet_foundation::Outcome::JetAbsent),
                },
            }),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
    }
}
"#,
    );
    out.push_str(
        r#"
fn __jet_bootstrap_decision_to_host(value: &@t.MIRDecisionRow@) -> Result<::jet_foundation::MIR::MirDecisionRow, String> {
    let kind = match &value.@f.MIRDecisionRow.kind@ {
        @v.MIRDecisionKind.Tier@ => ::jet_foundation::MIR::MirDecisionKind::Tier,
        @v.MIRDecisionKind.Inline@ => ::jet_foundation::MIR::MirDecisionKind::Inline,
        @v.MIRDecisionKind.Vectorize@ => ::jet_foundation::MIR::MirDecisionKind::Vectorize,
        @v.MIRDecisionKind.Parallel@ => ::jet_foundation::MIR::MirDecisionKind::Parallel,
        @v.MIRDecisionKind.Copy@ => ::jet_foundation::MIR::MirDecisionKind::Copy,
        @v.MIRDecisionKind.Bounds@ => ::jet_foundation::MIR::MirDecisionKind::Bounds,
        @v.MIRDecisionKind.Deopt@ => ::jet_foundation::MIR::MirDecisionKind::Deopt,
        @v.MIRDecisionKind.Unreachable@ => ::jet_foundation::MIR::MirDecisionKind::Unreachable,
        @v.MIRDecisionKind.LoopInvariant@ => ::jet_foundation::MIR::MirDecisionKind::LoopInvariant,
    };
    let disposition = match &value.@f.MIRDecisionRow.disposition@ {
        @v.MIRDecisionDisposition.Accepted@ => ::jet_foundation::MIR::MirDecisionDisposition::Accepted,
        @v.MIRDecisionDisposition.Rejected@ => ::jet_foundation::MIR::MirDecisionDisposition::Rejected,
        @v.MIRDecisionDisposition.Selected@ => ::jet_foundation::MIR::MirDecisionDisposition::Selected,
        @v.MIRDecisionDisposition.NotAttempted@ => ::jet_foundation::MIR::MirDecisionDisposition::NotAttempted,
        @v.MIRDecisionDisposition.Unavailable@ => ::jet_foundation::MIR::MirDecisionDisposition::Unavailable,
    };
    let function = match value.@f.MIRDecisionRow.function@.as_ref().ok() {
        Some(function) => Some(::jet_foundation::MIR::MirFunctionId(function.@f.MIRFunctionID.value@)),
        None => None,
    };
    let edit = match value.@f.MIRDecisionRow.edit@.as_ref().ok() {
        Some(edit) => Some(::jet_foundation::MIR::MirDecisionEdit {
            span: __jet_bootstrap_span_to_host(&edit.@f.MIRDecisionEdit.span@)?,
            replacement: edit.@f.MIRDecisionEdit.replacement@.clone(),
        }),
        None => None,
    };
    let derivation = match value.@f.MIRDecisionRow.derivation@.as_ref().ok() {
        Some(derivation) => Some(::jet_foundation::FactsDerivation::DerivationRef { id: derivation.@f.MIRDerivationRef.id@.clone() }),
        None => None,
    };
    let identity = match value.@f.MIRDecisionRow.identity@.as_ref().ok() {
        Some(identity) => Some(::jet_foundation::MIR::MirDecisionIdentity {
            source: identity.@f.MIRDecisionIdentity.source@.clone(),
            configuration: identity.@f.MIRDecisionIdentity.configuration@.clone(),
            profile: identity.@f.MIRDecisionIdentity.profile@.clone(),
            target: identity.@f.MIRDecisionIdentity.target@.clone(),
            implementation: identity.@f.MIRDecisionIdentity.implementation@.clone(),
            artifact: identity.@f.MIRDecisionIdentity.artifact@.clone(),
            run: identity.@f.MIRDecisionIdentity.run@.clone(),
        }),
        None => None,
    };
    let evidence_method = match value.@f.MIRDecisionRow.evidence_method@.as_ref().ok() {
        Some(method) => Some(match method {
            @v.MIRDerivationMethod.StaticDerivation@ => ::jet_foundation::FactsDerivation::DerivationMethod::StaticDerivation,
            @v.MIRDerivationMethod.FormalProof@ => ::jet_foundation::FactsDerivation::DerivationMethod::FormalProof,
            @v.MIRDerivationMethod.RecordedExecution@ => ::jet_foundation::FactsDerivation::DerivationMethod::RecordedExecution,
            @v.MIRDerivationMethod.SampledAgreement@ => ::jet_foundation::FactsDerivation::DerivationMethod::SampledAgreement,
            @v.MIRDerivationMethod.ExternalAssumption@ => ::jet_foundation::FactsDerivation::DerivationMethod::ExternalAssumption,
        }),
        None => None,
    };
    let derivation_disposition = match value.@f.MIRDecisionRow.derivation_disposition@.as_ref().ok() {
        Some(disposition) => Some(match disposition {
            @v.MIRDerivationDisposition.Current@ => ::jet_foundation::FactsDerivation::DerivationDisposition::Current,
            @v.MIRDerivationDisposition.Stale@ => ::jet_foundation::FactsDerivation::DerivationDisposition::Stale,
            @v.MIRDerivationDisposition.Expired@ => ::jet_foundation::FactsDerivation::DerivationDisposition::Expired,
            @v.MIRDerivationDisposition.Redacted@ => ::jet_foundation::FactsDerivation::DerivationDisposition::Redacted,
            @v.MIRDerivationDisposition.Unavailable@ => ::jet_foundation::FactsDerivation::DerivationDisposition::Unavailable,
            @v.MIRDerivationDisposition.Unsupported@ => ::jet_foundation::FactsDerivation::DerivationDisposition::Unsupported,
            @v.MIRDerivationDisposition.BudgetExhausted@ => ::jet_foundation::FactsDerivation::DerivationDisposition::BudgetExhausted,
            @v.MIRDerivationDisposition.Unknown@ => ::jet_foundation::FactsDerivation::DerivationDisposition::Unknown,
        }),
        None => None,
    };
    Ok(::jet_foundation::MIR::MirDecisionRow {
        id: value.@f.MIRDecisionRow.id@,
        kind,
        disposition,
        function,
        function_name: value.@f.MIRDecisionRow.function_name@.clone(),
        span: __jet_bootstrap_span_to_host(&value.@f.MIRDecisionRow.span@)?,
        rule: value.@f.MIRDecisionRow.rule@.clone(),
        reason: value.@f.MIRDecisionRow.reason@.clone(),
        producer: value.@f.MIRDecisionRow.producer@.clone(),
        evidence: value.@f.MIRDecisionRow.evidence@.clone(),
        edit,
        derivation,
        identity,
        evidence_method,
        derivation_disposition,
    })
}
fn __jet_bootstrap_decision_from_host(value: &::jet_foundation::MIR::MirDecisionRow) -> Result<@t.MIRDecisionRow@, String> {
    Ok(@t.MIRDecisionRow@ {
        @f.MIRDecisionRow.id@: value.id,
        @f.MIRDecisionRow.kind@: match value.kind {
            ::jet_foundation::MIR::MirDecisionKind::Tier => @v.MIRDecisionKind.Tier@,
            ::jet_foundation::MIR::MirDecisionKind::Inline => @v.MIRDecisionKind.Inline@,
            ::jet_foundation::MIR::MirDecisionKind::Vectorize => @v.MIRDecisionKind.Vectorize@,
            ::jet_foundation::MIR::MirDecisionKind::Parallel => @v.MIRDecisionKind.Parallel@,
            ::jet_foundation::MIR::MirDecisionKind::Copy => @v.MIRDecisionKind.Copy@,
            ::jet_foundation::MIR::MirDecisionKind::Bounds => @v.MIRDecisionKind.Bounds@,
            ::jet_foundation::MIR::MirDecisionKind::Deopt => @v.MIRDecisionKind.Deopt@,
            ::jet_foundation::MIR::MirDecisionKind::Unreachable => @v.MIRDecisionKind.Unreachable@,
            ::jet_foundation::MIR::MirDecisionKind::LoopInvariant => @v.MIRDecisionKind.LoopInvariant@,
        },
        @f.MIRDecisionRow.disposition@: match value.disposition {
            ::jet_foundation::MIR::MirDecisionDisposition::Accepted => @v.MIRDecisionDisposition.Accepted@,
            ::jet_foundation::MIR::MirDecisionDisposition::Rejected => @v.MIRDecisionDisposition.Rejected@,
            ::jet_foundation::MIR::MirDecisionDisposition::Selected => @v.MIRDecisionDisposition.Selected@,
            ::jet_foundation::MIR::MirDecisionDisposition::NotAttempted => @v.MIRDecisionDisposition.NotAttempted@,
            ::jet_foundation::MIR::MirDecisionDisposition::Unavailable => @v.MIRDecisionDisposition.Unavailable@,
        },
        @f.MIRDecisionRow.function@: match value.function {
            Some(function) => Ok(@t.MIRFunctionID@ { @f.MIRFunctionID.value@: function.0 }),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
        @f.MIRDecisionRow.function_name@: value.function_name.clone(),
        @f.MIRDecisionRow.span@: __jet_bootstrap_span_from_host(&value.span)?,
        @f.MIRDecisionRow.rule@: value.rule.clone(),
        @f.MIRDecisionRow.reason@: value.reason.clone(),
        @f.MIRDecisionRow.producer@: value.producer.clone(),
        @f.MIRDecisionRow.evidence@: value.evidence.clone(),
        @f.MIRDecisionRow.edit@: match value.edit.as_ref() {
            Some(edit) => Ok(@t.MIRDecisionEdit@ { @f.MIRDecisionEdit.span@: __jet_bootstrap_span_from_host(&edit.span)?, @f.MIRDecisionEdit.replacement@: edit.replacement.clone() }),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
        @f.MIRDecisionRow.derivation@: match value.derivation.as_ref() {
            Some(derivation) => Ok(@t.MIRDerivationRef@ { @f.MIRDerivationRef.id@: derivation.id.clone() }),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
        @f.MIRDecisionRow.identity@: match value.identity.as_ref() {
            Some(identity) => Ok(@t.MIRDecisionIdentity@ {
                @f.MIRDecisionIdentity.source@: identity.source.clone(), @f.MIRDecisionIdentity.configuration@: identity.configuration.clone(),
                @f.MIRDecisionIdentity.profile@: identity.profile.clone(), @f.MIRDecisionIdentity.target@: identity.target.clone(),
                @f.MIRDecisionIdentity.implementation@: identity.implementation.clone(), @f.MIRDecisionIdentity.artifact@: identity.artifact.clone(),
                @f.MIRDecisionIdentity.run@: identity.run.clone(),
            }),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
        @f.MIRDecisionRow.evidence_method@: match value.evidence_method {
            Some(method) => Ok(match method {
                ::jet_foundation::FactsDerivation::DerivationMethod::StaticDerivation => @v.MIRDerivationMethod.StaticDerivation@,
                ::jet_foundation::FactsDerivation::DerivationMethod::FormalProof => @v.MIRDerivationMethod.FormalProof@,
                ::jet_foundation::FactsDerivation::DerivationMethod::RecordedExecution => @v.MIRDerivationMethod.RecordedExecution@,
                ::jet_foundation::FactsDerivation::DerivationMethod::SampledAgreement => @v.MIRDerivationMethod.SampledAgreement@,
                ::jet_foundation::FactsDerivation::DerivationMethod::ExternalAssumption => @v.MIRDerivationMethod.ExternalAssumption@,
            }),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
        @f.MIRDecisionRow.derivation_disposition@: match value.derivation_disposition {
            Some(disposition) => Ok(match disposition {
                ::jet_foundation::FactsDerivation::DerivationDisposition::Current => @v.MIRDerivationDisposition.Current@,
                ::jet_foundation::FactsDerivation::DerivationDisposition::Stale => @v.MIRDerivationDisposition.Stale@,
                ::jet_foundation::FactsDerivation::DerivationDisposition::Expired => @v.MIRDerivationDisposition.Expired@,
                ::jet_foundation::FactsDerivation::DerivationDisposition::Redacted => @v.MIRDerivationDisposition.Redacted@,
                ::jet_foundation::FactsDerivation::DerivationDisposition::Unavailable => @v.MIRDerivationDisposition.Unavailable@,
                ::jet_foundation::FactsDerivation::DerivationDisposition::Unsupported => @v.MIRDerivationDisposition.Unsupported@,
                ::jet_foundation::FactsDerivation::DerivationDisposition::BudgetExhausted => @v.MIRDerivationDisposition.BudgetExhausted@,
                ::jet_foundation::FactsDerivation::DerivationDisposition::Unknown => @v.MIRDerivationDisposition.Unknown@,
            }),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
    })
}
"#,
    );
    out.push_str(
        r#"
fn __jet_bootstrap_crypto_reason_to_host(value: &@t.DiagnosticCryptoMisuseReason@) -> ::jet_foundation::Diagnostics::CryptoMisuseReason {
    match value {
        @v.DiagnosticCryptoMisuseReason.InvalidLength@ => ::jet_foundation::Diagnostics::CryptoMisuseReason::InvalidLength,
        @v.DiagnosticCryptoMisuseReason.NonceLength@ => ::jet_foundation::Diagnostics::CryptoMisuseReason::NonceLength,
        @v.DiagnosticCryptoMisuseReason.OutputLength@ => ::jet_foundation::Diagnostics::CryptoMisuseReason::OutputLength,
        @v.DiagnosticCryptoMisuseReason.SaltLength@ => ::jet_foundation::Diagnostics::CryptoMisuseReason::SaltLength,
        @v.DiagnosticCryptoMisuseReason.MemoryCost@ => ::jet_foundation::Diagnostics::CryptoMisuseReason::MemoryCost,
        @v.DiagnosticCryptoMisuseReason.IterationCount@ => ::jet_foundation::Diagnostics::CryptoMisuseReason::IterationCount,
        @v.DiagnosticCryptoMisuseReason.LaneCount@ => ::jet_foundation::Diagnostics::CryptoMisuseReason::LaneCount,
        @v.DiagnosticCryptoMisuseReason.MemoryTimeCost@ => ::jet_foundation::Diagnostics::CryptoMisuseReason::MemoryTimeCost,
        @v.DiagnosticCryptoMisuseReason.RawNonce@ => ::jet_foundation::Diagnostics::CryptoMisuseReason::RawNonce,
        @v.DiagnosticCryptoMisuseReason.RawAlgorithm@ => ::jet_foundation::Diagnostics::CryptoMisuseReason::RawAlgorithm,
        @v.DiagnosticCryptoMisuseReason.DeterministicEntropy@ => ::jet_foundation::Diagnostics::CryptoMisuseReason::DeterministicEntropy,
    }
}
fn __jet_bootstrap_crypto_reason_from_host(value: ::jet_foundation::Diagnostics::CryptoMisuseReason) -> @t.DiagnosticCryptoMisuseReason@ {
    match value {
        ::jet_foundation::Diagnostics::CryptoMisuseReason::InvalidLength => @v.DiagnosticCryptoMisuseReason.InvalidLength@,
        ::jet_foundation::Diagnostics::CryptoMisuseReason::NonceLength => @v.DiagnosticCryptoMisuseReason.NonceLength@,
        ::jet_foundation::Diagnostics::CryptoMisuseReason::OutputLength => @v.DiagnosticCryptoMisuseReason.OutputLength@,
        ::jet_foundation::Diagnostics::CryptoMisuseReason::SaltLength => @v.DiagnosticCryptoMisuseReason.SaltLength@,
        ::jet_foundation::Diagnostics::CryptoMisuseReason::MemoryCost => @v.DiagnosticCryptoMisuseReason.MemoryCost@,
        ::jet_foundation::Diagnostics::CryptoMisuseReason::IterationCount => @v.DiagnosticCryptoMisuseReason.IterationCount@,
        ::jet_foundation::Diagnostics::CryptoMisuseReason::LaneCount => @v.DiagnosticCryptoMisuseReason.LaneCount@,
        ::jet_foundation::Diagnostics::CryptoMisuseReason::MemoryTimeCost => @v.DiagnosticCryptoMisuseReason.MemoryTimeCost@,
        ::jet_foundation::Diagnostics::CryptoMisuseReason::RawNonce => @v.DiagnosticCryptoMisuseReason.RawNonce@,
        ::jet_foundation::Diagnostics::CryptoMisuseReason::RawAlgorithm => @v.DiagnosticCryptoMisuseReason.RawAlgorithm@,
        ::jet_foundation::Diagnostics::CryptoMisuseReason::DeterministicEntropy => @v.DiagnosticCryptoMisuseReason.DeterministicEntropy@,
    }
}
fn __jet_bootstrap_structured_to_host(value: &@t.DiagnosticStructured@) -> Result<::jet_foundation::Diagnostics::StructuredDiagnostic, String> {
    match value {
        @p.DiagnosticStructured.SuggestedEdits@{ edits } => Ok(::jet_foundation::Diagnostics::StructuredDiagnostic::SuggestedEdits {
            edits: edits.iter().map(|edit| __jet_bootstrap_text_edit_to_host(edit)).collect::<Result<Vec<_>, String>>()?,
        }),
        @p.DiagnosticStructured.CryptoMisuse@{ reason, operation, expected, actual } => Ok(::jet_foundation::Diagnostics::StructuredDiagnostic::CryptoMisuse {
            reason: __jet_bootstrap_crypto_reason_to_host(reason),
            operation: operation.clone(),
            expected: expected.as_ref().ok().cloned(),
            actual: match actual.as_ref().ok() {
                Some(value) => Some(i128::from(__jet_bootstrap_int_i64(value, "crypto structured actual")?)),
                None => None,
            },
        }),
        @p.DiagnosticStructured.BuildError@{ report } => Ok(::jet_foundation::Diagnostics::StructuredDiagnostic::BuildError {
            report: __jet_bootstrap_build_error_to_host(report)?,
        }),
        @p.DiagnosticStructured.RuntimeHostFault@{ stdout } => Ok(::jet_foundation::Diagnostics::StructuredDiagnostic::RuntimeHostFault { stdout: stdout.clone() }),
    }
}
fn __jet_bootstrap_structured_from_host(value: &::jet_foundation::Diagnostics::StructuredDiagnostic) -> Result<@t.DiagnosticStructured@, String> {
    match value {
        ::jet_foundation::Diagnostics::StructuredDiagnostic::SuggestedEdits { edits } => Ok(@new.DiagnosticStructured.SuggestedEdits@{
            edits: edits.iter().map(__jet_bootstrap_text_edit_from_host).collect::<Result<Vec<_>, String>>()?,
        }),
        ::jet_foundation::Diagnostics::StructuredDiagnostic::CryptoMisuse { reason, operation, expected, actual } => Ok(@new.DiagnosticStructured.CryptoMisuse@{
            reason: __jet_bootstrap_crypto_reason_from_host(*reason),
            operation: operation.to_string(),
            expected: match expected { Some(value) => Ok(value.to_string()), None => Err(jet_foundation::Outcome::JetAbsent) },
            actual: match actual { Some(value) => Ok(jet_foundation::Numeric::JetInt::from_i64(i64::try_from(*value).map_err(|_| "crypto structured actual exceeds Source Int".to_string())?)), None => Err(jet_foundation::Outcome::JetAbsent) },
        }),
        ::jet_foundation::Diagnostics::StructuredDiagnostic::BuildError { report } => Ok(@new.DiagnosticStructured.BuildError@{ report: __jet_bootstrap_build_error_from_host(report) }),
        ::jet_foundation::Diagnostics::StructuredDiagnostic::RuntimeHostFault { stdout } => Ok(@new.DiagnosticStructured.RuntimeHostFault@{ stdout: stdout.clone() }),
    }
}
"#,
    );
    out.push_str(
        r#"
fn __jet_bootstrap_moment_to_host(value: &@t.DiagnosticMoment@) -> ::jet_foundation::Diagnostics::ReportMoment {
    match value {
        @v.DiagnosticMoment.Compile@ => ::jet_foundation::Diagnostics::ReportMoment::Compile,
        @v.DiagnosticMoment.Run@ => ::jet_foundation::Diagnostics::ReportMoment::Run,
        @v.DiagnosticMoment.Test@ => ::jet_foundation::Diagnostics::ReportMoment::Test,
        @v.DiagnosticMoment.Tool@ => ::jet_foundation::Diagnostics::ReportMoment::Tool,
    }
}
fn __jet_bootstrap_moment_from_host(value: ::jet_foundation::Diagnostics::ReportMoment) -> @t.DiagnosticMoment@ {
    match value {
        ::jet_foundation::Diagnostics::ReportMoment::Compile => @v.DiagnosticMoment.Compile@,
        ::jet_foundation::Diagnostics::ReportMoment::Run => @v.DiagnosticMoment.Run@,
        ::jet_foundation::Diagnostics::ReportMoment::Test => @v.DiagnosticMoment.Test@,
        ::jet_foundation::Diagnostics::ReportMoment::Tool => @v.DiagnosticMoment.Tool@,
    }
}
fn __jet_bootstrap_applicability_to_host(value: &@t.DiagnosticFixApplicability@) -> ::jet_foundation::Report::FixApplicability {
    match value {
        @v.DiagnosticFixApplicability.Safe@ => ::jet_foundation::Report::FixApplicability::Safe,
        @v.DiagnosticFixApplicability.Suggested@ => ::jet_foundation::Report::FixApplicability::Suggested,
    }
}
fn __jet_bootstrap_applicability_from_host(value: ::jet_foundation::Report::FixApplicability) -> @t.DiagnosticFixApplicability@ {
    match value {
        ::jet_foundation::Report::FixApplicability::Safe => @v.DiagnosticFixApplicability.Safe@,
        ::jet_foundation::Report::FixApplicability::Suggested => @v.DiagnosticFixApplicability.Suggested@,
    }
}
fn __jet_bootstrap_safety_to_host(value: &@t.DiagnosticFixSafety@) -> ::jet_foundation::Report::FixSafety {
    match value {
        @v.DiagnosticFixSafety.Formatting@ => ::jet_foundation::Report::FixSafety::Formatting,
        @v.DiagnosticFixSafety.BehaviorPreserving@ => ::jet_foundation::Report::FixSafety::BehaviorPreserving,
        @v.DiagnosticFixSafety.APIChanging@ => ::jet_foundation::Report::FixSafety::ApiChanging,
        @v.DiagnosticFixSafety.TargetChanging@ => ::jet_foundation::Report::FixSafety::TargetChanging,
        @v.DiagnosticFixSafety.NeedsReview@ => ::jet_foundation::Report::FixSafety::NeedsReview,
    }
}
fn __jet_bootstrap_safety_from_host(value: ::jet_foundation::Report::FixSafety) -> @t.DiagnosticFixSafety@ {
    match value {
        ::jet_foundation::Report::FixSafety::Formatting => @v.DiagnosticFixSafety.Formatting@,
        ::jet_foundation::Report::FixSafety::BehaviorPreserving => @v.DiagnosticFixSafety.BehaviorPreserving@,
        ::jet_foundation::Report::FixSafety::ApiChanging => @v.DiagnosticFixSafety.APIChanging@,
        ::jet_foundation::Report::FixSafety::TargetChanging => @v.DiagnosticFixSafety.TargetChanging@,
        ::jet_foundation::Report::FixSafety::NeedsReview => @v.DiagnosticFixSafety.NeedsReview@,
    }
}
fn __jet_bootstrap_diagnostic_from_host(value: &::jet_foundation::Diagnostics::Diagnostic) -> Result<@t.Diagnostic@, String> {
    Ok(@t.Diagnostic@ {
        @f.Diagnostic.moment@: __jet_bootstrap_moment_from_host(value.moment),
        @f.Diagnostic.severity@: jet_foundation::Numeric::JetInt::from_i64(match value.severity { ::jet_foundation::Diagnostics::Severity::Error => 1, ::jet_foundation::Diagnostics::Severity::Lint => 2 }),
        @f.Diagnostic.code@: value.code.clone(),
        @f.Diagnostic.span@: match value.span.as_ref() { Some(span) => Ok(__jet_bootstrap_span_from_host(span)?), None => Err(jet_foundation::Outcome::JetAbsent) },
        @f.Diagnostic.source_path@: Err(jet_foundation::Outcome::JetAbsent),
        @f.Diagnostic.source_offset@: Err(jet_foundation::Outcome::JetAbsent),
        @f.Diagnostic.source_start@: Err(jet_foundation::Outcome::JetAbsent),
        @f.Diagnostic.origin@: match value.origin.as_ref() { Some(origin) => Ok(__jet_bootstrap_origin_from_host(origin)), None => Err(jet_foundation::Outcome::JetAbsent) },
        @f.Diagnostic.cause@: value.cause.iter().map(__jet_bootstrap_cause_from_host).collect::<Result<Vec<_>, String>>()?,
        @f.Diagnostic.labels@: value.labels.iter().map(|label| Ok(@t.DiagnosticLabel@ {
            @f.DiagnosticLabel.span@: __jet_bootstrap_span_from_host(&label.span)?,
            @f.DiagnosticLabel.message@: label.message.clone(),
        })).collect::<Result<Vec<_>, String>>()?,
        @f.Diagnostic.what@: value.what.clone(),
        @f.Diagnostic.why@: value.why.clone(),
        @f.Diagnostic.fix@: value.fix.clone(),
        @f.Diagnostic.edit@: match value.edit.as_ref() { Some(edit) => Ok(__jet_bootstrap_text_edit_from_host(edit)?), None => Err(jet_foundation::Outcome::JetAbsent) },
        @f.Diagnostic.applicability@: match value.applicability { Some(value) => Ok(__jet_bootstrap_applicability_from_host(value)), None => Err(jet_foundation::Outcome::JetAbsent) },
        @f.Diagnostic.safety@: match value.safety { Some(value) => Ok(__jet_bootstrap_safety_from_host(value)), None => Err(jet_foundation::Outcome::JetAbsent) },
        @f.Diagnostic.no_fix_reason@: match value.no_fix_reason.as_ref() { Some(reason) => Ok(__jet_bootstrap_no_fix_from_host(reason)), None => Err(jet_foundation::Outcome::JetAbsent) },
        @f.Diagnostic.detail@: match value.detail.as_ref() { Some(detail) => Ok(detail.clone()), None => Err(jet_foundation::Outcome::JetAbsent) },
        @f.Diagnostic.structured@: match value.structured.as_ref() { Some(value) => Ok(__jet_bootstrap_structured_from_host(value)?), None => Err(jet_foundation::Outcome::JetAbsent) },
        @f.Diagnostic.decision_row@: match value.decision_row.as_ref() { Some(value) => Ok(__jet_bootstrap_decision_from_host(value)?), None => Err(jet_foundation::Outcome::JetAbsent) },
        @f.Diagnostic.denial_kind@: match value.denial_kind.as_ref() { Some(value) => Ok(value.clone()), None => Err(jet_foundation::Outcome::JetAbsent) },
        @f.Diagnostic.call_chain@: value.call_chain.clone(),
        @f.Diagnostic.scope_chain@: value.scope_chain.clone(),
        @f.Diagnostic.nearest_granting_scope@: match value.nearest_granting_scope.as_ref() { Some(value) => Ok(value.clone()), None => Err(jet_foundation::Outcome::JetAbsent) },
    })
}
"#,
    );
    out.push_str(
        r#"
fn __jet_bootstrap_project_span(
    value: &::jet_foundation::Diagnostics::Span,
    aggregate_offset: usize,
    source_start: usize,
    source: &str,
) -> Result<::jet_foundation::Diagnostics::Span, String> {
    let local_start = value.start.checked_sub(aggregate_offset).ok_or_else(|| "diagnostic span starts before its source segment".to_string())?;
    let local_end = value.end.checked_sub(aggregate_offset).ok_or_else(|| "diagnostic span ends before its source segment".to_string())?;
    if local_end < local_start || local_end > source.len() { return Err("diagnostic span is outside its exact source segment".to_string()); }
    let start = source_start.checked_add(local_start).ok_or_else(|| "diagnostic raw source start overflows usize".to_string())?;
    let end = source_start.checked_add(local_end).ok_or_else(|| "diagnostic raw source end overflows usize".to_string())?;
    if end < start || end > source.len() || !source.is_char_boundary(start) || !source.is_char_boundary(end) { return Err("diagnostic span is outside its exact source bytes".to_string()); }
    Ok(::jet_foundation::Diagnostics::Span { start, end })
}
fn __jet_bootstrap_validate_diagnostic_channel(
    value: &::jet_foundation::Diagnostics::Diagnostic,
    row: &::jet_foundation::Registry::DiagnosticRow,
) -> Result<(), String> {
    if value.moment != row.moment { return Err(format!("Jet diagnostic `{}` moment disagrees with its native registry row", value.code)); }
    if value.severity != row.severity { return Err(format!("Jet diagnostic `{}` severity disagrees with its native registry row", value.code)); }
    match (&value.edit, value.applicability, value.safety) {
        (Some(_), Some(::jet_foundation::Report::FixApplicability::Safe), Some(safety)) if !safety.auto_apply() => return Err(format!("safe diagnostic edit `{}` has a non-auto-applicable safety grade", value.code)),
        (Some(_), Some(_), Some(_)) => {}
        (Some(_), _, _) => return Err(format!("Jet diagnostic `{}` has an edit without both applicability and safety", value.code)),
        (None, None, None) => {}
        (None, _, _) => return Err(format!("Jet diagnostic `{}` has fix grades without an edit", value.code)),
    }
    if value.edit.is_some() && value.no_fix_reason.is_some() { return Err(format!("Jet diagnostic `{}` has both an edit and a no-fix reason", value.code)); }
    if let Some(reason) = value.no_fix_reason.as_ref() {
        reason.validate()?;
        if row.no_fix_reason.is_none() {
            return Err(format!("Jet diagnostic `{}` carries a no-fix reason on a row without a no-fix channel", value.code));
        }
    }
    if let Some(edit) = value.edit.as_ref() {
        let channel_ok = match row.structured_fix {
            Some(::jet_foundation::Registry::StructuredFix::SourceEdit
                | ::jet_foundation::Registry::StructuredFix::SuggestedSourceEdit
                | ::jet_foundation::Registry::StructuredFix::GeneratedMarkerGroup
                | ::jet_foundation::Registry::StructuredFix::GeneratedMissingArms
                | ::jet_foundation::Registry::StructuredFix::GeneratedCallValue
                | ::jet_foundation::Registry::StructuredFix::GeneratedScriptRun
                | ::jet_foundation::Registry::StructuredFix::GeneratedRedundantTailReturn
                | ::jet_foundation::Registry::StructuredFix::Replace { .. }
                | ::jet_foundation::Registry::StructuredFix::Remove { .. }) => true,
            None => row.no_fix_reason.is_some(),
            Some(::jet_foundation::Registry::StructuredFix::CryptoMisuse) => false,
        };
        if !channel_ok { return Err(format!("Jet diagnostic `{}` edit is not authorized by its native registry channel", value.code)); }
        if edit.span.end < edit.span.start { return Err(format!("Jet diagnostic `{}` edit span ends before it starts", value.code)); }
    }
    if let Some(::jet_foundation::Diagnostics::StructuredDiagnostic::CryptoMisuse { .. }) = value.structured.as_ref() {
        if row.structured_fix != Some(::jet_foundation::Registry::StructuredFix::CryptoMisuse) {
            return Err(format!("Jet diagnostic `{}` crypto structured data is not authorized by its native registry channel", value.code));
        }
    }
    if let Some(::jet_foundation::Diagnostics::StructuredDiagnostic::SuggestedEdits { .. }) = value.structured.as_ref() {
        if value.edit.is_none() || value.applicability.is_none() || value.safety.is_none() { return Err(format!("Jet diagnostic `{}` alternative edits need a graded primary edit", value.code)); }
    }
    Ok(())
}
fn __jet_bootstrap_diagnostic_to_host_with_sources(
    diagnostic: &@t.Diagnostic@,
    generated_source_files: &[@t.MIRSourceFile@],
    snapshot: &crate::compiler_bootstrap_host::AuthorizedSourceSnapshot,
) -> Result<::jet_foundation::Diagnostics::Diagnostic, String> {
    let code = diagnostic.@f.Diagnostic.code@.clone();
    let row = ::jet_foundation::Registry::diagnostic(&code)
        .ok_or_else(|| format!("Jet diagnostic code `{code}` has no native typed registry row"))?;
    let source_origin = diagnostic.@f.Diagnostic.origin@.as_ref().ok().map(__jet_bootstrap_origin_to_host);
    let source_path = diagnostic.@f.Diagnostic.source_path@.as_ref().ok();
    let source_offset = diagnostic.@f.Diagnostic.source_offset@.as_ref().ok();
    let source_start_value = diagnostic.@f.Diagnostic.source_start@.as_ref().ok();
    let mut report_source: &str = "";
    let mut origin = source_origin.map(::std::sync::Arc::new);
    let mut aggregate_offset = 0usize;
    let mut raw_source_start = 0usize;
    let mut generated_origin = false;
    if let Some(explicit) = origin.as_ref() {
        generated_origin = __jet_bootstrap_validate_origin(explicit, source_path, generated_source_files, snapshot)?;
        report_source = explicit.source.as_str();
        if source_path.is_some() {
            let source_offset = source_offset.ok_or_else(|| "Jet diagnostic source path has no aggregate source offset".to_string())?;
            aggregate_offset = usize::try_from(__jet_bootstrap_int_u64(source_offset, "diagnostic source offset")?)
                .map_err(|_| "diagnostic source offset exceeds usize".to_string())?;
            let source_start = source_start_value.ok_or_else(|| "Jet diagnostic source path has no raw source start".to_string())?;
            raw_source_start = usize::try_from(__jet_bootstrap_int_u64(source_start, "diagnostic source start")?)
                .map_err(|_| "diagnostic source start exceeds usize".to_string())?;
        } else if source_offset.is_some() || source_start_value.is_some() {
            return Err("Jet diagnostic has source mapping anchors without a source path".to_string());
        }
    } else if let Some(source_path) = source_path {
        let source_offset = source_offset.ok_or_else(|| "Jet diagnostic source path has no aggregate source offset".to_string())?;
        aggregate_offset = usize::try_from(__jet_bootstrap_int_u64(source_offset, "diagnostic source offset")?)
            .map_err(|_| "diagnostic source offset exceeds usize".to_string())?;
        let source_start = source_start_value.ok_or_else(|| "Jet diagnostic source path has no raw source start".to_string())?;
        raw_source_start = usize::try_from(__jet_bootstrap_int_u64(source_start, "diagnostic source start")?)
            .map_err(|_| "diagnostic source start exceeds usize".to_string())?;
        let mut generated = None;
        for file in generated_source_files {
            if file.@f.MIRSourceFile.path@ == *source_path {
                if generated.is_some() {
                    return Err(format!("Jet generated diagnostic source path `{source_path}` is ambiguous"));
                }
                generated = Some(file);
            }
        }
        let physical = __jet_bootstrap_source_physical_path(snapshot, source_path);
        let mut selected = None;
        for root in &snapshot.roots {
            for file in root.files.iter().chain(&root.foreign_cache_files) {
                if file.path == physical {
                    if selected.is_some() {
                        return Err(format!("Jet diagnostic source path `{source_path}` is ambiguous in the authorized snapshot"));
                    }
                    selected = Some(file);
                }
            }
        }
        if generated.is_some() && selected.is_some() {
            return Err(format!("Jet generated diagnostic source path `{source_path}` collides with an authorized disk source"));
        }
        if let Some(file) = generated {
            report_source = file.@f.MIRSourceFile.source@.as_str();
            origin = Some(::std::sync::Arc::new(::jet_foundation::Diagnostics::DiagnosticOrigin::new(
                file.@f.MIRSourceFile.path@.clone(),
                file.@f.MIRSourceFile.path@.clone(),
                file.@f.MIRSourceFile.source@.clone(),
            )));
            generated_origin = true;
        } else {
            let file = selected.ok_or_else(|| format!("Jet diagnostic source path `{source_path}` is absent from the authorized snapshot and generated sidecar"))?;
            report_source = file.source.as_str();
            origin = Some(::std::sync::Arc::new(::jet_foundation::Diagnostics::DiagnosticOrigin::new(
                file.relative_path.clone(),
                file.path.clone(),
                file.source.clone(),
            )));
        }
    } else if source_offset.is_some() || source_start_value.is_some() {
        return Err("Jet diagnostic has source mapping anchors without a source path".to_string());
    }
    let span = match diagnostic.@f.Diagnostic.span@.as_ref().ok() {
        Some(span) => {
            let aggregate = __jet_bootstrap_span_to_host(span)?;
            Some(if source_path.is_some() || origin.is_some() {
                __jet_bootstrap_project_span(&aggregate, aggregate_offset, raw_source_start, report_source)?
            } else {
                aggregate
            })
        }
        None => None,
    };
    let edit = match diagnostic.@f.Diagnostic.edit@.as_ref().ok() {
        Some(edit) => {
            if source_path.is_none() && origin.is_none() {
                return Err("Jet diagnostic has an edit without an exact source origin".to_string());
            }
            if generated_origin {
                return Err("compiler-generated diagnostics cannot propose edits to source files".to_string());
            }
            let aggregate = __jet_bootstrap_span_to_host(&edit.@f.TextEdit.span@)?;
            Some(::jet_foundation::Diagnostics::TextEdit {
                span: __jet_bootstrap_project_span(&aggregate, aggregate_offset, raw_source_start, report_source)?,
                new_text: edit.@f.TextEdit.new_text@.clone(),
            })
        }
        None => None,
    };
    let mut labels = Vec::with_capacity(diagnostic.@f.Diagnostic.labels@.len());
    for label in &diagnostic.@f.Diagnostic.labels@ {
        let aggregate = __jet_bootstrap_span_to_host(&label.@f.DiagnosticLabel.span@)?;
        labels.push(::jet_foundation::Diagnostics::DiagnosticLabel {
            span: if source_path.is_some() || origin.is_some() {
                __jet_bootstrap_project_span(&aggregate, aggregate_offset, raw_source_start, report_source)?
            } else {
                aggregate
            },
            message: label.@f.DiagnosticLabel.message@.clone(),
        });
    }
    let native = ::jet_foundation::Diagnostics::Diagnostic {
        moment: __jet_bootstrap_moment_to_host(&diagnostic.@f.Diagnostic.moment@),
        severity: match __jet_bootstrap_int_i64(&diagnostic.@f.Diagnostic.severity@, "diagnostic severity")? {
            1 => ::jet_foundation::Diagnostics::Severity::Error,
            2 => ::jet_foundation::Diagnostics::Severity::Lint,
            _ => return Err(format!("Jet diagnostic `{code}` has an unknown severity")),
        },
        code,
        what: diagnostic.@f.Diagnostic.what@.clone(),
        why: diagnostic.@f.Diagnostic.why@.clone(),
        fix: diagnostic.@f.Diagnostic.fix@.clone(),
        span,
        origin,
        labels,
        cause: diagnostic.@f.Diagnostic.cause@.iter().map(|cause| __jet_bootstrap_cause_to_host(cause, generated_source_files, snapshot)).collect::<Result<Vec<_>, String>>()?,
        edit,
        applicability: diagnostic.@f.Diagnostic.applicability@.as_ref().ok().map(__jet_bootstrap_applicability_to_host),
        safety: diagnostic.@f.Diagnostic.safety@.as_ref().ok().map(__jet_bootstrap_safety_to_host),
        no_fix_reason: match diagnostic.@f.Diagnostic.no_fix_reason@.as_ref().ok() {
            Some(reason) => Some(__jet_bootstrap_no_fix_to_host(reason)?),
            None => None,
        },
        detail: diagnostic.@f.Diagnostic.detail@.as_ref().ok().cloned(),
        structured: match diagnostic.@f.Diagnostic.structured@.as_ref().ok() {
            Some(value) => Some(__jet_bootstrap_structured_to_host(value)?),
            None => None,
        },
        decision_row: match diagnostic.@f.Diagnostic.decision_row@.as_ref().ok() {
            Some(value) => Some(__jet_bootstrap_decision_to_host(value)?),
            None => None,
        },
        denial_kind: diagnostic.@f.Diagnostic.denial_kind@.as_ref().ok().cloned(),
        call_chain: diagnostic.@f.Diagnostic.call_chain@.clone(),
        scope_chain: diagnostic.@f.Diagnostic.scope_chain@.clone(),
        nearest_granting_scope: diagnostic.@f.Diagnostic.nearest_granting_scope@.as_ref().ok().cloned(),
    };
    __jet_bootstrap_validate_diagnostic_channel(&native, row)?;
    Ok(native)
}
pub(crate) fn __jet_bootstrap_diagnostic_to_host(
    value: &@t.Diagnostic@,
    snapshot: &crate::compiler_bootstrap_host::AuthorizedSourceSnapshot,
) -> Result<::jet_foundation::Diagnostics::Diagnostic, String> {
    __jet_bootstrap_diagnostic_to_host_with_sources(value, &[], snapshot)
}
pub(crate) fn __jet_bootstrap_reports_from_result(
    value: &@t.JetDriverCompileResult@,
    snapshot: &crate::compiler_bootstrap_host::AuthorizedSourceSnapshot,
) -> Result<Vec<::jet_foundation::Report::ReportEnvelope>, String> {
    let report_file = ::jet_foundation::Report::ReportPath::from_process(&snapshot.entry_path);
    let mut reports = Vec::with_capacity(value.@f.JetDriverCompileResult.diagnostics@.len());
    for diagnostic in &value.@f.JetDriverCompileResult.diagnostics@ {
        let native = __jet_bootstrap_diagnostic_to_host_with_sources(
            diagnostic,
            &value.@f.JetDriverCompileResult.generated_source_files@,
            snapshot,
        )?;
        let report_source = native.origin.as_deref().map(|origin| origin.source.as_str()).unwrap_or("");
        reports.push(native.to_report(&report_file, report_source));
    }
    Ok(reports)
}
"#,
    );
}
