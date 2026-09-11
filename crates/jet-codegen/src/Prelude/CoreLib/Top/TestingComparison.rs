// D-TEST-COMPARE1=A: the Foundation comparison engine is embedded in the
// generated Prelude by Codegen/mod.rs.  This carrier only invokes the two
// checked callables, records their observations, and marshals the canonical
// Foundation verdict into the Core plain type.

fn jet_testing_relation(
    relation: &String,
) -> crate::jet_testing_comparison_foundation::ObservationRelation {
    match relation.as_str() {
        "" | "typed_equality" => {
            crate::jet_testing_comparison_foundation::ObservationRelation::TypedEquality
        }
        "ordered_effects" => {
            crate::jet_testing_comparison_foundation::ObservationRelation::OrderedEffects
        }
        "typed_failure" => {
            crate::jet_testing_comparison_foundation::ObservationRelation::TypedFailure
        }
        other => crate::jet_testing_comparison_foundation::ObservationRelation::Custom(
            other.to_string(),
        ),
    }
}

fn jet_testing_observation(
    value: &jet_std::DataTree,
) -> crate::jet_testing_comparison_foundation::ComparisonObservation {
    crate::jet_testing_comparison_foundation::ComparisonObservation::value(
        jet_std::render_datatree_json(value, false, 0),
    )
}

fn jet_testing_record(
    record: crate::jet_testing_comparison_foundation::ComparisonRecord,
    cases: &Vec<jet_std::DataTree>,
    reference_values: Vec<jet_std::DataTree>,
    candidate_values: Vec<jet_std::DataTree>,
    seed: Option<i64>,
) -> jet_std::JetTestComparison {
    let case_ids = record
        .samples
        .iter()
        .map(|sample| sample.identity.case_id.clone())
        .collect::<Vec<_>>();
    let first_difference = record
        .first_difference
        .map(|index| index as i64)
        .unwrap_or(-1);
    jet_std::JetTestComparison {
        status: record.status.as_str().to_string(),
        relation: record.relation.as_str().to_string(),
        source: "core.testing".to_string(),
        tool: "jet".to_string(),
        target: "embedded-prelude".to_string(),
        seed,
        case_ids,
        inputs: cases.clone(),
        reference: reference_values,
        candidate: candidate_values,
        first_difference,
        reason: record
            .reason
            .unwrap_or_else(|| "comparison has no reason".to_string()),
        universal_proof: record.universal_proof,
    }
}

fn jet_testing_compare(
    cases: &Vec<jet_std::DataTree>,
    reference: Box<dyn Fn(jet_std::DataTree) -> jet_std::DataTree>,
    candidate: Box<dyn Fn(jet_std::DataTree) -> jet_std::DataTree>,
    relation: &String,
) -> jet_std::JetTestComparison {
    let relation = jet_testing_relation(relation);
    if !relation.supports_plain_callable()
        && matches!(relation.as_str(), "ordered_effects" | "typed_failure")
    {
        let record = crate::jet_testing_comparison_foundation::ComparisonRecord::terminal(
            relation,
            crate::jet_testing_comparison_foundation::ComparisonStatus::Unsupported,
            "relation requires recorded observations",
        );
        return jet_testing_record(record, cases, Vec::new(), Vec::new(), None);
    }
    let seed = std::env::var("JET_PROP_SEED")
        .ok()
        .and_then(|value| value.parse::<i64>().ok());
    let mut samples = Vec::with_capacity(cases.len());
    let mut reference_values = Vec::with_capacity(cases.len());
    let mut candidate_values = Vec::with_capacity(cases.len());

    for (index, input) in cases.iter().enumerate() {
        // Each implementation receives an independent input clone.  A panic
        // or runtime stop is an unavailable comparison, never a matched one.
        let reference_value = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            reference(input.clone())
        })) {
            Ok(value) => value,
            Err(_) => {
                let record = crate::jet_testing_comparison_foundation::ComparisonRecord::terminal(
                    relation.clone(),
                    crate::jet_testing_comparison_foundation::ComparisonStatus::Unavailable,
                    format!("reference implementation crashed on case-{index}"),
                );
                return jet_testing_record(record, cases, reference_values, candidate_values, seed);
            }
        };
        let candidate_value = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            candidate(input.clone())
        })) {
            Ok(value) => value,
            Err(_) => {
                reference_values.push(reference_value);
                let record = crate::jet_testing_comparison_foundation::ComparisonRecord::terminal(
                    relation.clone(),
                    crate::jet_testing_comparison_foundation::ComparisonStatus::Unavailable,
                    format!("candidate implementation crashed on case-{index}"),
                );
                return jet_testing_record(record, cases, reference_values, candidate_values, seed);
            }
        };
        let identity = crate::jet_testing_comparison_foundation::ComparisonIdentity::new(
            format!("case-{index}"),
            format!("input-{index}"),
            "core.testing",
            "jet",
            "embedded-prelude",
        );
        samples.push(
            crate::jet_testing_comparison_foundation::ComparisonSample::new(
                identity,
                jet_testing_observation(&reference_value),
                jet_testing_observation(&candidate_value),
            ),
        );
        reference_values.push(reference_value);
        candidate_values.push(candidate_value);
    }

    let record = crate::jet_testing_comparison_foundation::compare_samples(relation, samples);
    jet_testing_record(record, cases, reference_values, candidate_values, seed)
}

fn jet_testing_assert_equal(comparison: &jet_std::JetTestComparison) -> bool {
    comparison.status == "matched"
        && !comparison.universal_proof
        && comparison.first_difference < 0
        && !comparison.inputs.is_empty()
}

fn jet_testing_status(comparison: &jet_std::JetTestComparison) -> String {
    comparison.status.clone()
}

impl JetShow for jet_std::JetTestComparison {
    fn jet_show(&self) -> String {
        format!(
            "TestComparison {{ status: {}, relation: {}, first_difference: {}, reason: {} }}",
            self.status, self.relation, self.first_difference, self.reason
        )
    }
}
