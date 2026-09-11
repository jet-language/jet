//! Canonical, tier-neutral comparison evidence for `core.testing.compare`.
//!
//! This module owns the observation relation and verdict state.  Runners may
//! collect observations in different ways, but they must construct this
//! record before rendering or persisting a result.  In particular, an equal
//! finite sample is never represented as a universal proof.

use std::fmt;
use crate::PerformanceBudget::CanonicalJson;

pub const TEST_COMPARISON_SCHEMA_VERSION: u16 = 1;

/// A comparison verdict.  The non-success states are deliberately distinct:
/// a caller must not turn an empty, unavailable, or cancelled run into pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComparisonStatus {
    Matched,
    Mismatch,
    Empty,
    Unsupported,
    Unavailable,
    Timeout,
    Cancelled,
    InvalidOracle,
    Contaminated,
}

impl ComparisonStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Mismatch => "mismatch",
            Self::Empty => "empty",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::InvalidOracle => "invalid_oracle",
            Self::Contaminated => "contaminated",
        }
    }

    pub const fn is_success(self) -> bool {
        matches!(self, Self::Matched)
    }
}

impl fmt::Display for ComparisonStatus {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

/// The relation used to decide whether two observations agree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservationRelation {
    TypedEquality,
    OrderedEffects,
    TypedFailure,
    Custom(String),
}

impl ObservationRelation {
    pub fn as_str(&self) -> &str {
        match self {
            Self::TypedEquality => "typed_equality",
            Self::OrderedEffects => "ordered_effects",
            Self::TypedFailure => "typed_failure",
            Self::Custom(name) => name.as_str(),
        }
    }

    /// The direct Core callable carrier only supplies a typed return value.
    /// Relations that require effects or typed failures must arrive through a
    /// recorded observation adapter; treating missing fields as equal would
    /// turn an unobserved relation into a false pass.
    pub const fn supports_plain_callable(&self) -> bool {
        matches!(self, Self::TypedEquality)
    }

    fn compares(
        &self,
        reference: &ComparisonObservation,
        candidate: &ComparisonObservation,
        declared_equal: Option<bool>,
    ) -> Result<bool, ()> {
        match self {
            Self::TypedEquality => Ok(reference.raw == candidate.raw),
            Self::OrderedEffects => Ok(
                reference.raw == candidate.raw
                    && reference.ordered_effects == candidate.ordered_effects
                    && reference.cleanup == candidate.cleanup
                    && reference.mutation == candidate.mutation,
            ),
            Self::TypedFailure => Ok(
                reference.typed_failure == candidate.typed_failure
                    && reference.raw == candidate.raw,
            ),
            Self::Custom(_) => declared_equal.ok_or(()),
        }
    }
}

/// One side's observable result.  `raw` is the typed, canonical rendering;
/// the remaining fields retain observations that are otherwise easy to lose
/// when a runner only compares return values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComparisonObservation {
    pub raw: String,
    pub typed_failure: Option<String>,
    pub mutation: Option<String>,
    pub ordered_effects: Vec<String>,
    pub cleanup: Vec<String>,
}

impl ComparisonObservation {
    pub fn value(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            typed_failure: None,
            mutation: None,
            ordered_effects: Vec::new(),
            cleanup: Vec::new(),
        }
    }

    pub fn failure(raw: impl Into<String>, failure: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            typed_failure: Some(failure.into()),
            mutation: None,
            ordered_effects: Vec::new(),
            cleanup: Vec::new(),
        }
    }

    pub fn with_effects(mut self, effects: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.ordered_effects = effects.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_cleanup(mut self, cleanup: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.cleanup = cleanup.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_mutation(mut self, mutation: impl Into<String>) -> Self {
        self.mutation = Some(mutation.into());
        self
    }
}

/// Stable identity for one replayable case.  Empty identity fields are
/// rejected by [`compare_samples`] rather than silently producing anonymous
/// evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComparisonIdentity {
    pub case_id: String,
    pub input_id: String,
    pub seed: Option<u64>,
    pub source: String,
    pub tool: String,
    pub target: String,
}

impl ComparisonIdentity {
    pub fn new(
        case_id: impl Into<String>,
        input_id: impl Into<String>,
        source: impl Into<String>,
        tool: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        Self {
            case_id: case_id.into(),
            input_id: input_id.into(),
            seed: None,
            source: source.into(),
            tool: tool.into(),
            target: target.into(),
        }
    }

    pub fn is_complete(&self) -> bool {
        !self.case_id.is_empty()
            && !self.input_id.is_empty()
            && !self.source.is_empty()
            && !self.tool.is_empty()
            && !self.target.is_empty()
    }
}

/// A reference/candidate observation pair, including optional independent
/// replays used to detect cross-case state contamination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComparisonSample {
    pub identity: ComparisonIdentity,
    pub reference: ComparisonObservation,
    pub candidate: ComparisonObservation,
    pub reference_replay: Option<ComparisonObservation>,
    pub candidate_replay: Option<ComparisonObservation>,
    /// Required only for [`ObservationRelation::Custom`].
    pub declared_equal: Option<bool>,
}

impl ComparisonSample {
    pub fn new(
        identity: ComparisonIdentity,
        reference: ComparisonObservation,
        candidate: ComparisonObservation,
    ) -> Self {
        Self {
            identity,
            reference,
            candidate,
            reference_replay: None,
            candidate_replay: None,
            declared_equal: None,
        }
    }

    pub fn with_replays(
        mut self,
        reference: ComparisonObservation,
        candidate: ComparisonObservation,
    ) -> Self {
        self.reference_replay = Some(reference);
        self.candidate_replay = Some(candidate);
        self
    }

    pub fn with_declared_equal(mut self, equal: bool) -> Self {
        self.declared_equal = Some(equal);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComparisonRecord {
    pub schema_version: u16,
    pub status: ComparisonStatus,
    pub relation: ObservationRelation,
    pub samples: Vec<ComparisonSample>,
    pub discarded_cases: usize,
    pub first_difference: Option<usize>,
    pub reduced_counterexample: Option<ComparisonSample>,
    pub contamination: Option<String>,
    pub reason: Option<String>,
    /// Always false.  This field makes the non-proof boundary explicit to
    /// consumers that would otherwise infer universality from `Matched`.
    pub universal_proof: bool,
}
impl ComparisonRecord {
    /// Build a terminal outcome when one side could not produce an
    /// observation.  Terminal outcomes are intentionally never inferred from
    /// an empty sample list: the caller must name the concrete status and
    /// reason, and `assert_equal` will reject every non-matched status.
    pub fn terminal(
        relation: ObservationRelation,
        status: ComparisonStatus,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: TEST_COMPARISON_SCHEMA_VERSION,
            status,
            relation,
            samples: Vec::new(),
            discarded_cases: 0,
            first_difference: None,
            reduced_counterexample: None,
            contamination: None,
            reason: Some(reason.into()),
            universal_proof: false,
        }
    }

    /// Render the complete comparison record as the canonical payload carried
    /// by an existing receipt section. The payload retains both observations,
    /// identities, replay checks, and the reduced mismatch; the CLI summary is
    /// intentionally not used as a persistence format.
    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        self.validate()?;
        let samples = self
            .samples
            .iter()
            .map(comparison_sample_json)
            .collect::<Result<Vec<_>, _>>()?;
        let reduced_counterexample = self
            .reduced_counterexample
            .as_ref()
            .map(comparison_sample_json)
            .transpose()?
            .unwrap_or(CanonicalJson::Null);
        CanonicalJson::object([
            (
                "contamination".into(),
                optional_string_json(self.contamination.as_ref()),
            ),
            (
                "discarded_cases".into(),
                CanonicalJson::Integer(self.discarded_cases.to_string()),
            ),
            (
                "first_difference".into(),
                self.first_difference
                    .map(|value| CanonicalJson::Integer(value.to_string()))
                    .unwrap_or(CanonicalJson::Null),
            ),
            (
                "reason".into(),
                optional_string_json(self.reason.as_ref()),
            ),
            ("reduced_counterexample".into(), reduced_counterexample),
            (
                "relation".into(),
                CanonicalJson::String(self.relation.as_str().to_string()),
            ),
            ("samples".into(), CanonicalJson::Array(samples)),
            (
                "schema_version".into(),
                CanonicalJson::Integer(self.schema_version.to_string()),
            ),
            (
                "status".into(),
                CanonicalJson::String(self.status.as_str().to_string()),
            ),
            (
                "universal_proof".into(),
                CanonicalJson::Bool(self.universal_proof),
            ),
        ])
    }

    /// Decode one authenticated canonical comparison payload.
    pub fn from_json(value: &CanonicalJson) -> Result<Self, String> {
        let schema_version = json_integer(value, "schema_version")?
            .parse::<u16>()
            .map_err(|_| "comparison schema_version is not a valid u16".to_string())?;
        if schema_version != TEST_COMPARISON_SCHEMA_VERSION {
            return Err(format!(
                "unsupported comparison schema version {schema_version}"
            ));
        }
        let status = parse_comparison_status(json_text(value, "status")?)?;
        let relation = parse_observation_relation(json_text(value, "relation")?)?;
        let samples = json_array(value, "samples")?
            .iter()
            .enumerate()
            .map(|(index, sample)| comparison_sample_from_json(sample, index))
            .collect::<Result<Vec<_>, _>>()?;
        let discarded_cases = json_integer(value, "discarded_cases")?
            .parse::<usize>()
            .map_err(|_| "comparison discarded_cases is not a valid usize".to_string())?;
        let first_difference = json_optional_integer(value, "first_difference")?
            .map(|value| {
                value
                    .parse::<usize>()
                    .map_err(|_| "comparison first_difference is not a valid usize".to_string())
            })
            .transpose()?;
        let reduced_counterexample = json_optional_value(value, "reduced_counterexample")?
            .map(|value| comparison_sample_from_json(value, 0))
            .transpose()?;
        let contamination = json_optional_string(value, "contamination")?;
        let reason = json_optional_string(value, "reason")?;
        let universal_proof = json_bool(value, "universal_proof")?;
        let record = Self {
            schema_version,
            status,
            relation,
            samples,
            discarded_cases,
            first_difference,
            reduced_counterexample,
            contamination,
            reason,
            universal_proof,
        };
        record.validate()?;
        Ok(record)
    }

    /// Validate structural invariants before a record is indexed or attached
    /// to a receipt. A matched record must be a finite non-discarded sample;
    /// no status is allowed to carry the universal-proof bit.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != TEST_COMPARISON_SCHEMA_VERSION {
            return Err(format!(
                "unsupported comparison schema version {}",
                self.schema_version
            ));
        }
        if self.universal_proof {
            return Err("comparison records cannot claim universal proof".into());
        }
        if self.status == ComparisonStatus::Matched
            && (self.samples.is_empty() || self.discarded_cases != 0)
        {
            return Err(
                "matched comparison records require non-empty samples and no discarded cases"
                    .into(),
            );
        }
        if let Some(index) = self.first_difference {
            if index >= self.samples.len() {
                return Err("comparison first_difference is outside samples".into());
            }
        }
        if self.reduced_counterexample.is_some() && self.status != ComparisonStatus::Mismatch {
            return Err(
                "comparison reduced_counterexample is only valid for a mismatch".into(),
            );
        }
        for sample in &self.samples {
            validate_comparison_sample(sample)?;
        }
        if let Some(sample) = &self.reduced_counterexample {
            validate_comparison_sample(sample)?;
        }
        Ok(())
    }

    /// Stable identity for the complete canonical record payload.
    pub fn artifact_id(&self) -> Result<String, String> {
        Ok(self.to_json()?.sha256())
    }

    pub fn assert_equal(&self) -> Result<(), ComparisonAssertionError> {
        if self.status.is_success() && !self.samples.is_empty() && self.discarded_cases == 0 {
            return Ok(());
        }
        Err(ComparisonAssertionError {
            status: self.status,
            reason: self
                .reason
                .clone()
                .unwrap_or_else(|| "comparison did not establish equality".to_string()),
        })
    }

    pub fn first_mismatch(&self) -> Option<&ComparisonSample> {
        self.reduced_counterexample
            .as_ref()
            .or_else(|| self.first_difference.and_then(|index| self.samples.get(index)))
    }
}

fn optional_string_json(value: Option<&String>) -> CanonicalJson {
    value
        .cloned()
        .map(CanonicalJson::String)
        .unwrap_or(CanonicalJson::Null)
}

fn comparison_observation_json(
    observation: &ComparisonObservation,
) -> Result<CanonicalJson, String> {
    CanonicalJson::object([
        (
            "cleanup".into(),
            CanonicalJson::Array(
                observation
                    .cleanup
                    .iter()
                    .cloned()
                    .map(CanonicalJson::String)
                    .collect(),
            ),
        ),
        (
            "mutation".into(),
            optional_string_json(observation.mutation.as_ref()),
        ),
        (
            "ordered_effects".into(),
            CanonicalJson::Array(
                observation
                    .ordered_effects
                    .iter()
                    .cloned()
                    .map(CanonicalJson::String)
                    .collect(),
            ),
        ),
        ("raw".into(), CanonicalJson::String(observation.raw.clone())),
        (
            "typed_failure".into(),
            optional_string_json(observation.typed_failure.as_ref()),
        ),
    ])
}

fn comparison_identity_json(identity: &ComparisonIdentity) -> Result<CanonicalJson, String> {
    CanonicalJson::object([
        ("case_id".into(), CanonicalJson::String(identity.case_id.clone())),
        (
            "input_id".into(),
            CanonicalJson::String(identity.input_id.clone()),
        ),
        (
            "seed".into(),
            identity
                .seed
                .map(|value| CanonicalJson::Integer(value.to_string()))
                .unwrap_or(CanonicalJson::Null),
        ),
        ("source".into(), CanonicalJson::String(identity.source.clone())),
        ("target".into(), CanonicalJson::String(identity.target.clone())),
        ("tool".into(), CanonicalJson::String(identity.tool.clone())),
    ])
}

fn comparison_sample_json(sample: &ComparisonSample) -> Result<CanonicalJson, String> {
    CanonicalJson::object([
        (
            "candidate".into(),
            comparison_observation_json(&sample.candidate)?,
        ),
        (
            "candidate_replay".into(),
            sample
                .candidate_replay
                .as_ref()
                .map(comparison_observation_json)
                .transpose()?
                .unwrap_or(CanonicalJson::Null),
        ),
        (
            "declared_equal".into(),
            sample
                .declared_equal
                .map(CanonicalJson::Bool)
                .unwrap_or(CanonicalJson::Null),
        ),
        (
            "identity".into(),
            comparison_identity_json(&sample.identity)?,
        ),
        (
            "reference".into(),
            comparison_observation_json(&sample.reference)?,
        ),
        (
            "reference_replay".into(),
            sample
                .reference_replay
                .as_ref()
                .map(comparison_observation_json)
                .transpose()?
                .unwrap_or(CanonicalJson::Null),
        ),
    ])
}

fn json_object<'a>(
    value: &'a CanonicalJson,
    label: &str,
) -> Result<&'a std::collections::BTreeMap<String, CanonicalJson>, String> {
    match value {
        CanonicalJson::Object(fields) => Ok(fields),
        _ => Err(format!("{label} must be an object")),
    }
}

fn json_field<'a>(value: &'a CanonicalJson, key: &str) -> Result<&'a CanonicalJson, String> {
    json_object(value, "comparison value")?
        .get(key)
        .ok_or_else(|| format!("comparison value is missing `{key}`"))
}

fn json_text<'a>(value: &'a CanonicalJson, key: &str) -> Result<&'a str, String> {
    match json_field(value, key)? {
        CanonicalJson::String(value) => Ok(value),
        _ => Err(format!("comparison field `{key}` must be a string")),
    }
}

fn json_integer<'a>(value: &'a CanonicalJson, key: &str) -> Result<&'a str, String> {
    match json_field(value, key)? {
        CanonicalJson::Integer(value) => Ok(value),
        _ => Err(format!("comparison field `{key}` must be an integer")),
    }
}

fn json_bool(value: &CanonicalJson, key: &str) -> Result<bool, String> {
    match json_field(value, key)? {
        CanonicalJson::Bool(value) => Ok(*value),
        _ => Err(format!("comparison field `{key}` must be a boolean")),
    }
}

fn json_array<'a>(value: &'a CanonicalJson, key: &str) -> Result<&'a [CanonicalJson], String> {
    match json_field(value, key)? {
        CanonicalJson::Array(values) => Ok(values),
        _ => Err(format!("comparison field `{key}` must be an array")),
    }
}

fn json_optional_value<'a>(
    value: &'a CanonicalJson,
    key: &str,
) -> Result<Option<&'a CanonicalJson>, String> {
    match json_field(value, key)? {
        CanonicalJson::Null => Ok(None),
        value => Ok(Some(value)),
    }
}

fn json_optional_string(
    value: &CanonicalJson,
    key: &str,
) -> Result<Option<String>, String> {
    let Some(value) = json_optional_value(value, key)? else {
        return Ok(None);
    };
    match value {
        CanonicalJson::String(value) => Ok(Some(value.clone())),
        _ => Err(format!("comparison field `{key}` must be a string or null")),
    }
}

fn json_optional_integer(
    value: &CanonicalJson,
    key: &str,
) -> Result<Option<String>, String> {
    let Some(value) = json_optional_value(value, key)? else {
        return Ok(None);
    };
    match value {
        CanonicalJson::Integer(value) => Ok(Some(value.clone())),
        _ => Err(format!("comparison field `{key}` must be an integer or null")),
    }
}

fn parse_comparison_status(value: &str) -> Result<ComparisonStatus, String> {
    match value {
        "matched" => Ok(ComparisonStatus::Matched),
        "mismatch" => Ok(ComparisonStatus::Mismatch),
        "empty" => Ok(ComparisonStatus::Empty),
        "unsupported" => Ok(ComparisonStatus::Unsupported),
        "unavailable" => Ok(ComparisonStatus::Unavailable),
        "timeout" => Ok(ComparisonStatus::Timeout),
        "cancelled" => Ok(ComparisonStatus::Cancelled),
        "invalid_oracle" => Ok(ComparisonStatus::InvalidOracle),
        "contaminated" => Ok(ComparisonStatus::Contaminated),
        _ => Err(format!("unknown comparison status `{value}`")),
    }
}

fn parse_observation_relation(value: &str) -> Result<ObservationRelation, String> {
    if value.is_empty() {
        return Err("comparison relation cannot be empty".into());
    }
    Ok(match value {
        "typed_equality" => ObservationRelation::TypedEquality,
        "ordered_effects" => ObservationRelation::OrderedEffects,
        "typed_failure" => ObservationRelation::TypedFailure,
        other => ObservationRelation::Custom(other.to_string()),
    })
}

fn json_strings(
    value: &CanonicalJson,
    key: &str,
) -> Result<Vec<String>, String> {
    json_array(value, key)?
        .iter()
        .map(|value| match value {
            CanonicalJson::String(value) => Ok(value.clone()),
            _ => Err(format!("comparison field `{key}` entries must be strings")),
        })
        .collect()
}

fn comparison_observation_from_json(
    value: &CanonicalJson,
    label: &str,
) -> Result<ComparisonObservation, String> {
    let raw = json_text(value, "raw")?.to_string();
    let typed_failure = json_optional_string(value, "typed_failure")?;
    let mutation = json_optional_string(value, "mutation")?;
    let ordered_effects = json_strings(value, "ordered_effects")?;
    let cleanup = json_strings(value, "cleanup")?;
    let mut observation = typed_failure
        .map(|failure| ComparisonObservation::failure(raw.clone(), failure))
        .unwrap_or_else(|| ComparisonObservation::value(raw));
    observation.ordered_effects = ordered_effects;
    observation.cleanup = cleanup;
    observation.mutation = mutation;
    if observation.raw.is_empty() && label.is_empty() {
        return Err("comparison observation raw value cannot be empty".into());
    }
    Ok(observation)
}

fn comparison_identity_from_json(value: &CanonicalJson) -> Result<ComparisonIdentity, String> {
    let case_id = json_text(value, "case_id")?.to_string();
    let input_id = json_text(value, "input_id")?.to_string();
    let source = json_text(value, "source")?.to_string();
    let target = json_text(value, "target")?.to_string();
    let tool = json_text(value, "tool")?.to_string();
    let seed = json_optional_integer(value, "seed")?
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "comparison identity seed is not a valid u64".to_string())
        })
        .transpose()?;
    let mut identity = ComparisonIdentity::new(case_id, input_id, source, tool, target);
    identity.seed = seed;
    Ok(identity)
}

fn comparison_sample_from_json(
    value: &CanonicalJson,
    index: usize,
) -> Result<ComparisonSample, String> {
    let identity = comparison_identity_from_json(json_field(value, "identity")?)?;
    let reference = comparison_observation_from_json(
        json_field(value, "reference")?,
        &format!("sample {index} reference"),
    )?;
    let candidate = comparison_observation_from_json(
        json_field(value, "candidate")?,
        &format!("sample {index} candidate"),
    )?;
    let mut sample = ComparisonSample::new(identity, reference, candidate);
    let reference_replay = json_optional_value(value, "reference_replay")?
        .map(|value| comparison_observation_from_json(value, "reference replay"))
        .transpose()?;
    let candidate_replay = json_optional_value(value, "candidate_replay")?
        .map(|value| comparison_observation_from_json(value, "candidate replay"))
        .transpose()?;
    match (reference_replay, candidate_replay) {
        (Some(reference), Some(candidate)) => {
            sample = sample.with_replays(reference, candidate);
        }
        (None, None) => {}
        _ => return Err(format!("comparison sample {index} must provide both replays")),
    }
    if let Some(value) = json_optional_value(value, "declared_equal")? {
        sample.declared_equal = match value {
            CanonicalJson::Bool(value) => Some(*value),
            _ => return Err(format!("comparison sample {index} declared_equal must be bool or null")),
        };
    }
    Ok(sample)
}

fn validate_comparison_sample(sample: &ComparisonSample) -> Result<(), String> {
    if !sample.identity.is_complete() {
        return Err("comparison sample identity is incomplete".into());
    }
    if sample.reference_replay.is_some() != sample.candidate_replay.is_some() {
        return Err("comparison sample replay fields must be paired".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComparisonAssertionError {
    pub status: ComparisonStatus,
    pub reason: String,
}

impl fmt::Display for ComparisonAssertionError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "comparison {}: {}", self.status, self.reason)
    }
}

impl std::error::Error for ComparisonAssertionError {}

/// Compare one finite corpus.  This function is the one canonical verdict
/// engine used by the CLI and the generated Prelude adapter.
pub fn compare_samples(
    relation: ObservationRelation,
    samples: impl IntoIterator<Item = ComparisonSample>,
) -> ComparisonRecord {
    compare_samples_with_discarded(relation, samples, 0)
}

pub fn compare_samples_with_discarded(
    relation: ObservationRelation,
    samples: impl IntoIterator<Item = ComparisonSample>,
    discarded_cases: usize,
) -> ComparisonRecord {
    let samples = samples.into_iter().collect::<Vec<_>>();
    let mut record = ComparisonRecord {
        schema_version: TEST_COMPARISON_SCHEMA_VERSION,
        status: ComparisonStatus::Empty,
        relation,
        samples,
        discarded_cases,
        first_difference: None,
        reduced_counterexample: None,
        contamination: None,
        reason: None,
        universal_proof: false,
    };

    if record.samples.is_empty() {
        record.reason = Some(if discarded_cases == 0 {
            "comparison corpus is empty".to_string()
        } else {
            "all comparison cases were discarded".to_string()
        });
        return record;
    }
    if record.samples.iter().any(|sample| !sample.identity.is_complete()) {
        record.status = ComparisonStatus::InvalidOracle;
        record.reason = Some("comparison sample identity is incomplete".to_string());
        return record;
    }

    for (index, sample) in record.samples.iter().enumerate() {
        if let Some(replay) = &sample.reference_replay {
            if replay != &sample.reference {
                record.status = ComparisonStatus::Contaminated;
                record.contamination = Some(format!(
                    "reference observation changed on replay for case `{}`",
                    sample.identity.case_id
                ));
                record.reason = record.contamination.clone();
                return record;
            }
        }
        if let Some(replay) = &sample.candidate_replay {
            if replay != &sample.candidate {
                record.status = ComparisonStatus::Contaminated;
                record.contamination = Some(format!(
                    "candidate observation changed on replay for case `{}`",
                    sample.identity.case_id
                ));
                record.reason = record.contamination.clone();
                return record;
            }
        }
        let equal = match record.relation.compares(
            &sample.reference,
            &sample.candidate,
            sample.declared_equal,
        ) {
            Ok(equal) => equal,
            Err(()) => {
                record.status = ComparisonStatus::InvalidOracle;
                record.reason = Some(
                    "custom comparison relation did not declare an observation result".to_string(),
                );
                return record;
            }
        };
        if !equal {
            record.status = ComparisonStatus::Mismatch;
            record.first_difference = Some(index);
            record.reduced_counterexample = Some(sample.clone());
            record.reason = Some(format!(
                "first differing observation is case `{}`",
                sample.identity.case_id
            ));
            return record;
        }
    }

    if discarded_cases > 0 {
        record.status = ComparisonStatus::InvalidOracle;
        record.reason = Some("comparison discarded cases and cannot claim equality".to_string());
    } else {
        record.status = ComparisonStatus::Matched;
        record.reason = Some(format!(
            "{} observed case(s) matched under {}",
            record.samples.len(),
            record.relation.as_str()
        ));
    }
    record
}

/// Preserve the first mismatch while reducing a corpus supplied by a caller.
/// The reducer is deliberately relation-aware: it only accepts a candidate
/// reduction when the same relation still observes a mismatch.
pub fn reduce_mismatch(
    record: &ComparisonRecord,
    candidates: impl IntoIterator<Item = Vec<ComparisonSample>>,
) -> Option<ComparisonSample> {
    if record.status != ComparisonStatus::Mismatch {
        return None;
    }
    for candidate in candidates {
        let reduced = compare_samples(record.relation.clone(), candidate);
        if reduced.status == ComparisonStatus::Mismatch {
            return reduced.reduced_counterexample;
        }
    }
    record.reduced_counterexample.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(case_id: &str, reference: &str, candidate: &str) -> ComparisonSample {
        ComparisonSample::new(
            ComparisonIdentity::new(case_id, format!("input-{case_id}"), "fixture", "test", "host"),
            ComparisonObservation::value(reference),
            ComparisonObservation::value(candidate),
        )
    }

    #[test]
    fn finite_equal_corpus_is_not_universal_proof() {
        let record = compare_samples(ObservationRelation::TypedEquality, [sample("one", "a", "a")]);
        assert_eq!(record.status, ComparisonStatus::Matched);
        assert!(!record.universal_proof);
        assert!(record.assert_equal().is_ok());
    }

    #[test]
    fn empty_discarded_and_custom_without_declaration_cannot_pass() {
        let empty = compare_samples(ObservationRelation::TypedEquality, []);
        assert_eq!(empty.status, ComparisonStatus::Empty);
        assert!(empty.assert_equal().is_err());

        let discarded = compare_samples_with_discarded(
            ObservationRelation::TypedEquality,
            [sample("one", "a", "a")],
            1,
        );
        assert_eq!(discarded.status, ComparisonStatus::InvalidOracle);
        assert!(discarded.assert_equal().is_err());

        let custom = compare_samples(
            ObservationRelation::Custom("domain".to_string()),
            [sample("one", "a", "a")],
        );
        assert_eq!(custom.status, ComparisonStatus::InvalidOracle);
        assert!(custom.assert_equal().is_err());
    }

    #[test]
    fn replay_contamination_and_relation_aware_reduction_are_preserved() {
        let contaminated = compare_samples(
            ObservationRelation::TypedEquality,
            [sample("one", "a", "a").with_replays(
                ComparisonObservation::value("changed"),
                ComparisonObservation::value("a"),
            )],
        );
        assert_eq!(contaminated.status, ComparisonStatus::Contaminated);
        assert!(contaminated.contamination.is_some());

        let record = compare_samples(
            ObservationRelation::OrderedEffects,
            [ComparisonSample::new(
                ComparisonIdentity::new("one", "input-one", "fixture", "test", "host"),
                ComparisonObservation::value("same").with_effects(["write-a", "write-b"]),
                ComparisonObservation::value("same").with_effects(["write-b", "write-a"]),
            )],
        );
        assert_eq!(record.status, ComparisonStatus::Mismatch);
        let reduced = reduce_mismatch(&record, [vec![sample("wrong", "a", "a")]]);
        assert_eq!(
            reduced.as_ref().map(|sample| sample.identity.case_id.as_str()),
            Some("one")
        );
    }
}
