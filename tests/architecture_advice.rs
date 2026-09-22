//! Card #2998: keep local complexity separate from bounded architecture advice.
//!
//! This fixture deliberately gives one checked consumer two call paths while
//! the ordering helpers disagree. The CLI must expose source-linked advice and
//! uncertainty, never turn equal complexity scores into an equivalence claim.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const ORDERING_POLICY_SOURCE: &str = "fn optimistic_rank(after: Int) -> Int { return after + 1 }\n\
fn persisted_rank(after: Int) -> Int { return after + 2 }\n\
fn run() { print(optimistic_rank(10) == persisted_rank(10)) }\n";

fn run_complexity_lint(file: &Path, json: bool) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command
        .args(["lint", "--complexity"])
        .arg(file)
        .current_dir(file.parent().expect("ordering-policy fixture parent"))
        .env("NO_COLOR", "1");
    if json {
        command.arg("--json");
    }
    let output = command
        .output()
        .expect("run architecture-advice complexity lint");
    assert!(
        output.status.success(),
        "complexity lint failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("complexity lint stdout is UTF-8")
}

fn assert_architecture_contract(json: &str, file: &Path) {
    assert!(
        json.contains("\"contract\":\"jet.architecture-advice/v1\""),
        "JSON must carry the versioned architecture-advice contract:\n{json}"
    );
    assert!(
        json.contains("\"architecture_advice\""),
        "JSON must expose the architecture-advice projection:\n{json}"
    );
    let source_name = file
        .file_name()
        .and_then(|name| name.to_str())
        .expect("ordering-policy fixture filename");
    let evidence_start = json
        .find("\"source_evidence\"")
        .expect("source evidence field");
    let evidence_end = json[evidence_start..]
        .find("\"affected_boundaries\"")
        .map(|offset| evidence_start + offset)
        .expect("affected boundaries after source evidence");
    let evidence = &json[evidence_start..evidence_end];
    assert!(
        evidence.contains(source_name),
        "source evidence must identify the concrete fixture source:\n{evidence}"
    );
    for evidence in [
        "\"role\":\"caller-definition\"",
        "\"role\":\"callee-definition\"",
        "\"role\":\"checked-call-site\"",
        "\"span\":{\"start\":",
    ] {
        assert!(
            json.contains(evidence),
            "JSON must carry concrete source evidence {evidence}:\n{json}"
        );
    }
    assert!(
        json.contains("\"affected_boundaries\"") && json.contains(" -> "),
        "JSON must carry a concrete module boundary:\n{json}"
    );
    assert!(
        json.contains("\"name\":\"checked_reference_resolution\",\"status\":\"passed\""),
        "checked-reference behavior must pass:\n{json}"
    );
    assert!(
        json.contains("\"name\":\"consumer_behavior\",\"status\":\"not_run\"")
            && json.contains("\"name\":\"cross_mode_agreement\",\"status\":\"unknown\""),
        "runtime uncertainty must remain explicit:\n{json}"
    );

    let local_start = json
        .find("\"local_complexity\"")
        .expect("local complexity field");
    let advice_start = json
        .find("\"architecture_advice\"")
        .expect("architecture advice field");
    assert!(
        local_start < advice_start,
        "local complexity and architecture advice must be separate fields:\n{json}"
    );
    let advice_end = json[advice_start..]
        .find("\"measurement\"")
        .map(|offset| advice_start + offset)
        .expect("measurement after architecture advice");
    let advice = &json[advice_start..advice_end];
    assert!(
        !advice.contains("\"score\"") && !advice.contains("equivalence"),
        "architecture advice must not present complexity as an equivalence score:\n{advice}"
    );
    assert!(
        json.contains("\"kind\":\"consumer-agreement\"")
            && json.contains("\"operator_action\":\"review_or_reject\""),
        "the candidate must remain advisory rather than an equivalence policy:\n{json}"
    );
}

#[test]
fn complexity_lint_reports_ordering_policy_advice_without_equivalence_claim() {
    let scratch = common::Scratch::new("architecture-advice-ordering-policy");
    let file: PathBuf = scratch.join("ordering_policy.jet");
    fs::write(&file, ORDERING_POLICY_SOURCE).expect("write ordering-policy fixture");

    let json = run_complexity_lint(&file, true);
    assert_architecture_contract(&json, &file);
    for function in ["optimistic_rank", "persisted_rank"] {
        assert!(
            json.contains(&format!("\"fn\":\"{function}\"")),
            "local complexity must retain {function}'s score row:\n{json}"
        );
    }
    assert!(
        json.contains("\"score\":0"),
        "the simple ordering helpers must retain their local complexity score:\n{json}"
    );

    let text = run_complexity_lint(&file, false);
    assert!(
        text.contains("architecture advice (D-ARCH-ADVICE1; advisory, operator may reject):"),
        "text output must identify the advisory architecture surface:\n{text}"
    );
    for evidence in [
        "evidence: caller-definition",
        "evidence: callee-definition",
        "evidence: checked-call-site",
        "boundary: ",
        " -> ",
        "check: checked_reference_resolution=passed",
        "check: consumer_behavior=not_run",
        "check: cross_mode_agreement=unknown",
    ] {
        assert!(
            text.contains(evidence),
            "text output must carry architecture evidence {evidence}:\n{text}"
        );
    }
    let advice_start = text
        .find("architecture advice (D-ARCH-ADVICE1; advisory, operator may reject):")
        .expect("text architecture advice heading");
    let advice = &text[advice_start..];
    assert!(
        !advice.contains("score") && !advice.contains("equivalence"),
        "text architecture advice must not present complexity as an equivalence score:\n{advice}"
    );
}
