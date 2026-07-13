use jet::BudgetProviders::{evaluation_diagnostic, unavailable_if_too_few, FailureClass, ProviderCancellation, ProviderDiagnostic, ProviderEvidence, ProviderEvent, ProviderFailure, ProviderRegistry, ProviderRequest, ProviderSpec};
use jet_foundation::PerformanceBudget::{CanonicalJson, Direction, Evaluation, Evidence, PolicyOutcome, Rational};
use std::time::Duration;

fn snapshot(name:&str,diagnostic:&ProviderDiagnostic){let expected=std::fs::read_to_string(format!("tests/ui/perf_budget_{name}.stderr")).unwrap();assert_eq!(diagnostic.render(),expected);}

#[test]
fn public_provider_diagnostics_match_pinned_tool_snapshots(){
    let unavailable=ProviderEvidence{events:vec![ProviderEvent::Unavailable{spec:0,reason:"probe could not observe the declared ready event".into(),details:vec![]},ProviderEvent::Complete{request_id:"1".repeat(64),samples:0}]};snapshot("e2906_unavailable",&unavailable_if_too_few("api-p99",&unavailable,20).unwrap_err());
    let regression=Evaluation{point:Rational::integer(125),lower95:Some(Rational::integer(110)),upper95:Some(Rational::integer(140)),evidence:Evidence::Regression,outcome:PolicyOutcome::Fail,bootstrap:Vec::new()};snapshot("e2907_regression",&evaluation_diagnostic("api-p99",&regression,Direction::LowerIsBetter,&["a".repeat(64)]).unwrap());
    let operation=ProviderFailure{class:FailureClass::Malformed,reason:"provider stream has bad magic".into()};snapshot("e2908_operation",&operation.diagnostic("api-p99"));
}

fn panicking_provider(_: &ProviderRequest,_: &ProviderCancellation)->Result<Vec<ProviderEvent>,ProviderFailure>{panic!("raw provider panic from {}",file!())}

#[test]
fn subprocess_panicking_registered_provider_child(){
    if std::env::var_os("JET_TEST_PANICKING_PROVIDER_CHILD").is_none(){return}
    let request=ProviderRequest{schema:"jet.provider-request".into(),version:1,request_id:"1".repeat(64),provider_hash:"2".repeat(64),context_hash:"3".repeat(64),specs:vec![ProviderSpec{budget_hash:"4".repeat(64),metric:"BenchTime".into()}],workload:CanonicalJson::Null,policy:CanonicalJson::Null};
    let mut registry=ProviderRegistry::default();registry.register_in_process("panic",panicking_provider).unwrap();let failure=registry.collect("panic",&request,Duration::from_secs(1)).unwrap_err();eprint!("{}",failure.diagnostic("api-p99").render());
}

#[test]
fn panicking_registered_provider_never_leaks_rust_stderr(){
    let output=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","subprocess_panicking_registered_provider_child","--nocapture"]).env("JET_TEST_PANICKING_PROVIDER_CHILD","1").env("RUST_BACKTRACE","1").output().unwrap();assert!(output.status.success());let stderr=String::from_utf8(output.stderr).unwrap();let expected=std::fs::read_to_string("tests/ui/perf_budget_e2908_provider_panic.stderr").unwrap();assert_eq!(stderr,expected);for leak in ["panicked","BudgetProviders.rs","performance_budget_providers.rs","subprocess_panicking_registered_provider_child","stack backtrace"]{assert!(!stderr.contains(leak),"leaked `{leak}`: {stderr}");}
}
