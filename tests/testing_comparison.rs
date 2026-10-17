//! #4906: execute the actual Jet Foundation classifier fixture through the
//! existing compiler-source parity assembly and default/interpreter runners.
mod common;

use common::Scratch;
use std::fs;

#[test]
fn typed_equality_uses_checked_value_facts_on_both_jet_tiers() {
    let mut source = common::compiler_parity_source("comparison equality", |path| {
        matches!(
            path,
            "Compiler/JetFoundation/Source/Hash/SHA256.jet"
                | "Compiler/JetFoundation/Source/Encoding/CanonicalJson.jet"
                | "Compiler/JetFoundation/Source/Evidence/TestingComparison.jet"
                | "Compiler/JetFoundation/Source/Evidence/ComparisonVerdict.jet"
        )
    });
    source.push_str(include_str!(
        "../Compiler/JetFoundation/Tests/ComparisonEquality.jet"
    ));
    let scratch = Scratch::new("comparison_equality");
    fs::write(
        scratch.join("package.jet"),
        "name: \"comparison_equality\"\nedition: \"2028\"\n",
    )
    .expect("write isolated comparison fixture manifest");
    let entry = scratch.join("main.jet");
    fs::write(&entry, source).expect("write actual comparison classifier fixture");
    for (tier, force_interpreter) in [("default", false), ("interpreter", true)] {
        let outcome = common::dev_run(entry.to_str().unwrap(), false, force_interpreter);
        let jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } = outcome
        else {
            panic!("{tier} comparison equality fixture did not run: {outcome:?}");
        };
        assert_eq!(exit_code, 0, "{tier} comparison equality failed: {stderr}");
        assert_eq!(
            stdout, "",
            "{tier} comparison fixture printed unexpected output"
        );
        assert_eq!(stderr, "", "{tier} comparison fixture emitted diagnostics");
    }
}
