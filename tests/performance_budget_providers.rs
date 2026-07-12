use jet::BudgetProviders::{evaluation_diagnostic, unavailable_if_too_few, FailureClass, ProviderDiagnostic, ProviderEvidence, ProviderEvent, ProviderFailure};
use jet_foundation::PerformanceBudget::{Direction, Evaluation, Evidence, PolicyOutcome, Rational};

fn snapshot(name:&str,diagnostic:&ProviderDiagnostic){let expected=std::fs::read_to_string(format!("tests/ui/perf_budget_{name}.stderr")).unwrap();assert_eq!(diagnostic.render(),expected);}

#[test]
fn public_provider_diagnostics_match_pinned_tool_snapshots(){
    let unavailable=ProviderEvidence{events:vec![ProviderEvent::Unavailable{spec:0,reason:"probe could not observe the declared ready event".into(),details:vec![]},ProviderEvent::Complete{request_id:"1".repeat(64),samples:0}]};snapshot("e2906_unavailable",&unavailable_if_too_few("api-p99",&unavailable,20).unwrap_err());
    let regression=Evaluation{point:Rational::integer(125),lower95:Some(Rational::integer(110)),upper95:Some(Rational::integer(140)),evidence:Evidence::Regression,outcome:PolicyOutcome::Fail,bootstrap:Vec::new()};snapshot("e2907_regression",&evaluation_diagnostic("api-p99",&regression,Direction::LowerIsBetter,&["a".repeat(64)]).unwrap());
    let operation=ProviderFailure{class:FailureClass::Malformed,reason:"provider stream has bad magic".into()};snapshot("e2908_operation",&operation.diagnostic("api-p99"));
}
