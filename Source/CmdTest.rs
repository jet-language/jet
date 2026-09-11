//! `jet test-compare` command: replay a recorded reference/candidate corpus.
//!
//! The command deliberately consumes observations rather than inventing a second
//! execution engine.  A compiler/editor/plugin can write the same bounded JSON
//! record and this command applies the canonical Foundation verdict to it.

use crate::OutputAdapter::OutputMode;
use std::process::exit;

use jet::ExitCodes;
use jet::RecordIndex::{RecordKind, RecordLink};
use jet::ReceiptStore::{
    input_paths_for, receipt_root_for, ReceiptSection, ReceiptStore,
};
use jet_foundation::DataTree::DataTree;
use jet_foundation::Evidence::{
    EvidenceBuild, EvidenceKind, EvidenceOutcome, EvidenceProducerKind, EvidenceRecord,
    EvidenceReport, EvidenceRevision, EvidenceSource, EvidenceAttachment, EvidenceIdentity,
};
use jet_foundation::JSON::{json_escape, parse_json};
use jet_foundation::TestingComparison::{
    compare_samples_with_discarded, ComparisonIdentity, ComparisonObservation, ComparisonRecord,
    ComparisonSample, ComparisonStatus, ObservationRelation,
};
const DEFAULT_SOURCE: &str = "jet.test-compare";
const DEFAULT_TOOL: &str = "jet";
const DEFAULT_TARGET: &str = "recorded-corpus";
#[derive(Clone, Debug, Default)]
struct ComparisonBinding {
    boundary_digest: Option<String>,
    source_identity: Option<String>,
    replacement_identity: Option<String>,
    tool_identity: Option<String>,
    target_identity: Option<String>,
    coverage_digest: Option<String>,
    assumptions: Vec<String>,
}

fn validate_binding_shape(root: &DataTree) -> Result<(), String> {
    for key in [
        "foreign",
        "foreign-binding",
        "boundary_digest",
        "boundary-digest",
        "boundary",
        "source_identity",
        "source-identity",
        "replacement_identity",
        "replacement-identity",
        "tool_identity",
        "tool-identity",
        "target_identity",
        "target-identity",
        "coverage_digest",
        "coverage-digest",
        "coverage",
        "assumptions",
    ] {
        if object_value(root, key).is_some() {
            return Err(format!(
                "foreign binding field `{key}` must be nested under `foreign_binding`"
            ));
        }
    }
    let Some(binding) = object_value(root, "foreign_binding") else {
        return Ok(());
    };
    binding
        .as_object()
        .map_err(|_| "`foreign_binding` must be an object".to_string())?;
    for key in [
        "foreign",
        "foreign-binding",
        "foreign_binding",
        "boundary-digest",
        "boundary",
        "source-identity",
        "source",
        "replacement-identity",
        "replacement",
        "tool-identity",
        "tool",
        "target-identity",
        "target",
        "coverage-digest",
        "coverage",
    ] {
        if object_value(binding, key).is_some() {
            return Err(format!(
                "foreign binding field `{key}` is not a canonical snake_case field"
            ));
        }
    }
    Ok(())
}

impl ComparisonBinding {
    fn from_root(root: &DataTree) -> Result<Self, String> {
        validate_binding_shape(root)?;
        let binding = Self {
            boundary_digest: binding_text(root, "boundary_digest")?,
            source_identity: binding_text(root, "source_identity")?,
            replacement_identity: binding_text(root, "replacement_identity")?,
            tool_identity: binding_text(root, "tool_identity")?,
            target_identity: binding_text(root, "target_identity")?,
            coverage_digest: binding_text(root, "coverage_digest")?,
            assumptions: binding_strings(root, "assumptions")?,
        };
        if binding.boundary_digest.is_none()
            || binding.source_identity.is_none()
            || binding.replacement_identity.is_none()
            || binding.tool_identity.is_none()
            || binding.target_identity.is_none()
            || binding.coverage_digest.is_none()
        {
            return Err(
                "`foreign_binding` requires boundary_digest, source_identity, replacement_identity, tool_identity, target_identity, and coverage_digest"
                    .into(),
            );
        }
        Ok(binding)
    }

    fn attachments(&self) -> Vec<EvidenceAttachment> {
        let mut attachments = Vec::new();
        for (name, value) in [
            ("boundary", self.boundary_digest.as_ref()),
            ("source-identity", self.source_identity.as_ref()),
            ("replacement-identity", self.replacement_identity.as_ref()),
            ("tool-identity", self.tool_identity.as_ref()),
            ("target-identity", self.target_identity.as_ref()),
            ("coverage", self.coverage_digest.as_ref()),
        ] {
            if let Some(value) = value {
                attachments.push(EvidenceAttachment::new(name, value.clone()));
            }
        }
        attachments.extend(
            self.assumptions
                .iter()
                .cloned()
                .map(|value| EvidenceAttachment::new("assumption", value)),
        );
        attachments
    }
}

fn binding_value<'a>(root: &'a DataTree, key: &str) -> Option<&'a DataTree> {
    object_value(root, "foreign_binding")
        .and_then(|binding| object_value(binding, key))
}

fn binding_text(root: &DataTree, key: &str) -> Result<Option<String>, String> {
    let Some(value) = binding_value(root, key) else {
        return Ok(None);
    };
    match value {
        DataTree::Text(value) | DataTree::TypedText(value) if !value.trim().is_empty() => {
            Ok(Some(value.clone()))
        }
        DataTree::Text(_) | DataTree::TypedText(_) => {
            Err(format!("foreign binding `{key}` must not be empty"))
        }
        _ => Err(format!("foreign binding `{key}` must be a string")),
    }
}

fn binding_strings(root: &DataTree, key: &str) -> Result<Vec<String>, String> {
    let Some(value) = binding_value(root, key) else {
        return Ok(Vec::new());
    };
    let mut values = value
        .as_array()
        .map_err(|_| format!("foreign binding `{key}` must be an array"))?
        .iter()
        .map(|value| match value {
            DataTree::Text(value) | DataTree::TypedText(value) if !value.trim().is_empty() => {
                Ok(value.clone())
            }
            DataTree::Text(_) | DataTree::TypedText(_) => {
                Err(format!("foreign binding `{key}` entries must not be empty"))
            }
            _ => Err(format!("foreign binding `{key}` entries must be strings")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    values.sort();
    values.dedup();
    Ok(values)
}




pub(crate) fn run_test_compare(target: &str, args: &[String], mode: OutputMode) -> ! {
    let json_output = mode.json || args.iter().any(|arg| arg == "--json");
    let relation_override = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--relation="));
    let input = match std::fs::read_to_string(target) {
        Ok(input) => input,
        Err(error) => {
            emit_error(json_output, &format!("cannot read comparison corpus `{target}`: {error}"));
            exit(ExitCodes::USER_ERROR);
        }
    };
    let root = match parse_json(&input) {
        Ok(root) => root,
        Err(_) => {
            emit_error(json_output, &format!("invalid comparison corpus `{target}`: record is not valid JSON"));
            exit(ExitCodes::USER_ERROR);
        }
    };
    let binding = match ComparisonBinding::from_root(&root) {
        Ok(binding) => binding,
        Err(error) => {
            emit_error(
                json_output,
                &format!("invalid comparison corpus `{target}`: {error}"),
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    let mut record = match parse_record(&input, relation_override) {
        Ok(record) => record,
        Err(error) => {
            emit_error(json_output, &format!("invalid comparison corpus `{target}`: {error}"));
            exit(ExitCodes::USER_ERROR);
        }
    };
    // The source record may explicitly report a terminal execution outcome.
    // Such outcomes are never promoted to a pass, even if stale samples happen
    // to compare equal.
    if let Some(status) = object_text(&root, "status")
        .and_then(|value| comparison_status(value))
        .filter(|status| !matches!(status, ComparisonStatus::Matched | ComparisonStatus::Mismatch))
    {
        record.status = status;
        record.reason = Some(format!("recorded runner outcome is {}", status.as_str()));
        record.universal_proof = false;
    }
    if let Err(error) = persist_comparison_artifacts(target, args, &record, &binding) {
        emit_error(
            json_output,
            &format!("could not persist comparison record: {error}"),
        );
        exit(ExitCodes::USER_ERROR);
    }
    let rendered = render_record(&record, json_output);
    println!("{rendered}");
    exit(if record.status.is_success() {
        ExitCodes::OK
    } else {
        ExitCodes::USER_ERROR
    });
}

fn parse_record(input: &str, relation_override: Option<&str>) -> Result<ComparisonRecord, String> {
    let root = parse_json(input).map_err(|_| "record is not valid JSON".to_string())?;
    let object = root
        .as_object()
        .map_err(|_| "record root must be an object".to_string())?;
    let relation_name = relation_override
        .map(str::to_string)
        .or_else(|| object_text(&root, "relation").map(str::to_string))
        .unwrap_or_else(|| "typed_equality".to_string());
    let relation = match relation_name.as_str() {
        "typed_equality" => ObservationRelation::TypedEquality,
        "ordered_effects" => ObservationRelation::OrderedEffects,
        "typed_failure" => ObservationRelation::TypedFailure,
        other => ObservationRelation::Custom(other.to_string()),
    };
    let source = object_text(&root, "source").unwrap_or(DEFAULT_SOURCE).to_string();
    let tool = object_text(&root, "tool").unwrap_or(DEFAULT_TOOL).to_string();
    let target = object_text(&root, "target").unwrap_or(DEFAULT_TARGET).to_string();
    let seed = object_int(&root, "seed").and_then(|value| u64::try_from(value).ok());
    let discarded = object_int(&root, "discarded_cases")
        .or_else(|| object_int(&root, "discarded"))
        .map(|value| usize::try_from(value).map_err(|_| "discarded_cases must be nonnegative"))
        .transpose()?
        .unwrap_or(0);
    let cases = object_value(&root, "cases")
        .ok_or_else(|| "record is missing `cases`".to_string())?
        .as_array()
        .map_err(|_| "record `cases` must be an array".to_string())?;
    let mut samples = Vec::with_capacity(cases.len());
    for (index, case) in cases.iter().enumerate() {
        samples.push(parse_sample(
            case,
            index,
            seed,
            &source,
            &tool,
            &target,
            &relation,
        )?);
    }
    Ok(compare_samples_with_discarded(relation, samples, discarded))
}

fn persist_comparison_artifacts(
    target: &str,
    args: &[String],
    record: &ComparisonRecord,
    binding: &ComparisonBinding,
) -> Result<(), String> {
    let payload = record.to_json()?;
    let artifact_id = payload.sha256();
    let sample_count = u64::try_from(record.samples.len())
        .map_err(|_| "comparison sample count is too large".to_string())?;
    let first_identity = record.samples.first().map(|sample| &sample.identity);
    let source = first_identity
        .map(|identity| identity.source.clone())
        .unwrap_or_else(|| DEFAULT_SOURCE.to_string());
    let tool = first_identity
        .map(|identity| identity.tool.clone())
        .unwrap_or_else(|| DEFAULT_TOOL.to_string());
    let comparison_target = first_identity
        .map(|identity| identity.target.clone())
        .unwrap_or_else(|| target.to_string());
    let detail = record
        .reason
        .clone()
        .unwrap_or_else(|| format!("comparison record is {}", record.status.as_str()));
    let source_ref = EvidenceSource::new(source, 0, 1);
    let build = EvidenceBuild::new(tool, comparison_target, "comparison");
    let revision = EvidenceRevision::new(
        artifact_id.clone(),
        "jet-comparison-v1",
        artifact_id.clone(),
    );
    let report_id = format!("test-compare-{artifact_id}");
    let identity = EvidenceIdentity::for_record(
        &report_id,
        "comparison",
        EvidenceProducerKind::Test,
        EvidenceKind::Unit,
        &source_ref,
        sample_count,
        &detail,
    );
    let mut evidence = EvidenceRecord::new(
        identity,
        EvidenceKind::Unit,
        EvidenceProducerKind::Test,
        comparison_evidence_outcome(record.status),
        sample_count,
        source_ref.clone(),
        build.clone(),
        revision.clone(),
    );
    evidence.attachments.extend(binding.attachments());
    evidence.attachments.push(EvidenceAttachment::new(
        "comparison-artifact",
        artifact_id.clone(),
    ));
    let derivation = evidence.checked_derivation();
    let mut report = EvidenceReport::new(
        report_id.clone(),
        EvidenceProducerKind::Test,
        source_ref,
        build,
        revision,
    );
    report
        .add_record_with_derivation(evidence, derivation)
        .map_err(|error| error.to_string())?;
    crate::CmdCompile::persist_evidence_report(&report)?;

    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let argv = args.to_vec();
    let input_paths = input_paths_for("test-compare", &argv, &cwd);
    if input_paths.is_empty() {
        return Err("comparison corpus has no current input path".to_string());
    }
    let root = receipt_root_for("test-compare", &argv, &cwd);
    let store = ReceiptStore::new(root);
    let section = ReceiptSection::from_json("comparison", "ComparisonRecord", payload)?;
    let evidence_link = RecordLink::new(RecordKind::Evidence, report_id)?;
    let comparison_link = RecordLink::new(RecordKind::Comparison, artifact_id)?;
    let status = if record.status.is_success() { 0 } else { 1 };
    store.record_with_sections_and_links(
        "test-compare",
        &argv,
        &input_paths,
        status,
        &[],
        &[],
        &[section],
        &[evidence_link],
        &[comparison_link],
    )?;
    Ok(())
}

fn comparison_evidence_outcome(status: ComparisonStatus) -> EvidenceOutcome {
    match status {
        ComparisonStatus::Matched => EvidenceOutcome::Passed,
        ComparisonStatus::Mismatch => EvidenceOutcome::Failed,
        ComparisonStatus::InvalidOracle | ComparisonStatus::Contaminated => EvidenceOutcome::Error,
        ComparisonStatus::Empty
        | ComparisonStatus::Unsupported
        | ComparisonStatus::Unavailable
        | ComparisonStatus::Timeout
        | ComparisonStatus::Cancelled => EvidenceOutcome::Unavailable,
    }
}

fn parse_sample(
    value: &DataTree,
    index: usize,
    seed: Option<u64>,
    source: &str,
    tool: &str,
    target: &str,
    relation: &ObservationRelation,
) -> Result<ComparisonSample, String> {
    let case_id = object_text(value, "case_id")
        .or_else(|| object_text(value, "caseId"))
        .ok_or_else(|| format!("case {index} is missing `case_id`"))?;
    let input_id = object_text(value, "input_id")
        .or_else(|| object_text(value, "inputId"))
        .ok_or_else(|| format!("case `{case_id}` is missing `input_id`"))?;
    let mut identity = ComparisonIdentity::new(case_id, input_id, source, tool, target);
    identity.seed = object_int(value, "seed")
        .and_then(|value| u64::try_from(value).ok())
        .or(seed);
    let reference = parse_observation(value, "reference", relation)?;
    let candidate = parse_observation(value, "candidate", relation)?;
    let mut sample = ComparisonSample::new(identity, reference, candidate);
    if let (Some(reference), Some(candidate)) = (
        optional_observation(value, "reference_replay", relation)?,
        optional_observation(value, "candidate_replay", relation)?,
    ) {
        sample = sample.with_replays(reference, candidate);
    } else if object_value(value, "reference_replay").is_some()
        || object_value(value, "candidate_replay").is_some()
    {
        return Err(format!("case `{case_id}` must provide both replay observations"));
    }
    if let Some(declared_equal) = object_bool(value, "declared_equal") {
        sample = sample.with_declared_equal(declared_equal);
    }
    Ok(sample)
}

fn parse_observation(
    object: &DataTree,
    prefix: &str,
    relation: &ObservationRelation,
) -> Result<ComparisonObservation, String> {
    let raw_value = object_value(object, prefix)
        .ok_or_else(|| format!("case observation `{prefix}` is missing"))?;
    let raw = match raw_value {
        DataTree::Text(value) | DataTree::TypedText(value) => value.clone(),
        value => canonical_data_tree(value),
    };
    let failure = object_text(object, &format!("{prefix}_failure"));
    let mut observation = failure
        .map(|failure| ComparisonObservation::failure(raw.clone(), failure))
        .unwrap_or_else(|| ComparisonObservation::value(raw));
    if matches!(relation, ObservationRelation::OrderedEffects) {
        let effects = object_strings(object, &format!("{prefix}_effects"))?;
        let cleanup = object_strings(object, &format!("{prefix}_cleanup"))?;
        observation = observation.with_effects(effects).with_cleanup(cleanup);
    }
    if let Some(mutation) = object_text(object, &format!("{prefix}_mutation")) {
        observation = observation.with_mutation(mutation);
    }
    Ok(observation)
}

fn optional_observation(
    object: &DataTree,
    prefix: &str,
    relation: &ObservationRelation,
) -> Result<Option<ComparisonObservation>, String> {
    if object_value(object, prefix).is_none() {
        return Ok(None);
    }
    parse_observation(object, prefix, relation).map(Some)
}

fn render_record(record: &ComparisonRecord, json_output: bool) -> String {
    let status = record.status.as_str();
    let relation = record.relation.as_str();
    let reason = record.reason.as_deref().unwrap_or("comparison has no reason");
    if json_output {
        let first_difference = record
            .first_difference
            .map(|index| index.to_string())
            .unwrap_or_else(|| "null".to_string());
        return format!(
            "{{\"schema_version\":{},\"status\":\"{}\",\"relation\":\"{}\",\"samples\":{},\"discarded_cases\":{},\"first_difference\":{},\"universal_proof\":false,\"reason\":\"{}\"}}",
            record.schema_version,
            json_escape(status),
            json_escape(relation),
            record.samples.len(),
            record.discarded_cases,
            first_difference,
            json_escape(reason),
        );
    }
    let mut output = format!("status={status} relation={relation} samples={} reason={reason}", record.samples.len());
    if let Some(sample) = record.first_mismatch() {
        output.push_str(&format!(
            "\nreplay case={} input={} source={} tool={} target={}",
            sample.identity.case_id,
            sample.identity.input_id,
            sample.identity.source,
            sample.identity.tool,
            sample.identity.target,
        ));
        output.push_str(&format!(
            "\nreference={}\ncandidate={}",
            sample.reference.raw, sample.candidate.raw
        ));
    }
    output
}

fn emit_error(json_output: bool, message: &str) {
    if json_output {
        println!(
            "{{\"status\":\"invalid_oracle\",\"universal_proof\":false,\"error\":\"{}\"}}",
            json_escape(message)
        );
    } else {
        eprintln!("{message}");
    }
}

fn comparison_status(value: &str) -> Option<ComparisonStatus> {
    Some(match value {
        "matched" => ComparisonStatus::Matched,
        "mismatch" => ComparisonStatus::Mismatch,
        "empty" => ComparisonStatus::Empty,
        "unsupported" => ComparisonStatus::Unsupported,
        "unavailable" => ComparisonStatus::Unavailable,
        "timeout" => ComparisonStatus::Timeout,
        "cancelled" => ComparisonStatus::Cancelled,
        "invalid_oracle" => ComparisonStatus::InvalidOracle,
        "contaminated" => ComparisonStatus::Contaminated,
        _ => return None,
    })
}

fn object_value<'a>(value: &'a DataTree, key: &str) -> Option<&'a DataTree> {
    value
        .as_object()
        .ok()?
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
}

fn object_text<'a>(value: &'a DataTree, key: &str) -> Option<&'a str> {
    match object_value(value, key)? {
        DataTree::Text(value) | DataTree::TypedText(value) => Some(value),
        _ => None,
    }
}

fn object_int(value: &DataTree, key: &str) -> Option<i64> {
    match object_value(value, key)? {
        DataTree::Int(value) => Some(*value),
        DataTree::Number(value) => value.parse().ok(),
        _ => None,
    }
}

fn object_bool(value: &DataTree, key: &str) -> Option<bool> {
    match object_value(value, key)? {
        DataTree::Bool(value) => Some(*value),
        _ => None,
    }
}

fn object_strings(value: &DataTree, key: &str) -> Result<Vec<String>, String> {
    let Some(value) = object_value(value, key) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .map_err(|_| format!("`{key}` must be an array"))?
        .iter()
        .map(|value| match value {
            DataTree::Text(value) | DataTree::TypedText(value) => Ok(value.clone()),
            _ => Err(format!("`{key}` entries must be strings")),
        })
        .collect()
}

fn canonical_data_tree(value: &DataTree) -> String {
    match value {
        DataTree::Null => "null".to_string(),
        DataTree::Bool(value) => value.to_string(),
        DataTree::Int(value) => value.to_string(),
        DataTree::Float(value) => value.to_string(),
        DataTree::Number(value) => value.clone(),
        DataTree::TypedText(value) | DataTree::Text(value) => {
            format!("\"{}\"", json_escape(value))
        }
        DataTree::Bytes(value) => format!("bytes:{}", value.len()),
        DataTree::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_data_tree)
                .collect::<Vec<_>>()
                .join(",")
        ),
        DataTree::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| {
                    format!("\"{}\":{}", json_escape(key), canonical_data_tree(value))
                })
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}
