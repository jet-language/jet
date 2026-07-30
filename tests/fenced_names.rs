#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

use tir_support::{build_and_run, have_rustc};

const SOURCE: &str = r#"
fn run() {
    <: score1..score3 :> :: 7
    print(<: score1..score3 :>)
}
"#;

#[test]
fn fenced_names_match_on_aot_jit_and_interpreter() {
    let expected = "7\n7\n7\n";
    if have_rustc() {
        let (code, stdout) = build_and_run("fenced_names", SOURCE);
        assert_eq!(code, 0);
        assert_eq!(stdout, expected);
    }

    let dir = std::env::temp_dir().join(format!("jet_fenced_names_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, SOURCE).unwrap();
    let shown = path.to_string_lossy().into_owned();

    let mut bundle = jet::Loader::load_entry(&shown).expect("fenced-name bundle should load");
    let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.severity,
                jet::Diagnostics::Severity::Error
            )
        })
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "fenced names must type-check: {errors:?}");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "expanded statements must stay resident-JIT safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle).expect("expanded statements must compile in resident JIT");

    for (tier, force_interpreter) in [("resident JIT", false), ("interpreter", true)] {
        jet_jit::reset_jit_trace_for_test();
        match jet::Interpreter::dev_iteration(&shown, false, force_interpreter) {
            jet::Interpreter::RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(exit_code, 0, "{tier} exit drift");
                assert_eq!(stderr, "", "{tier} stderr drift");
                assert_eq!(stdout, expected, "{tier} expansion order drift");
                if !force_interpreter {
                    assert!(
                        !jet_jit::deopt_invoked_for_test(),
                        "fenced names resident JIT must not fall back"
                    );
                }
            }
            jet::Interpreter::RunOutcome::Problems(diagnostics) => {
                panic!("{tier} rejected fenced names: {diagnostics:?}")
            }
        }
    }
}
