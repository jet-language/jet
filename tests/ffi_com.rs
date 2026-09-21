//! Focused COM/VBA fixture coverage for card #507.
//!
//! The checked-in example and IDL are portable source artifacts. The production
//! type-library bind and COM/Office execution remain Windows-host proof, so the
//! portable test only checks the honest non-Windows refusal.

use std::fs;

mod common;

#[cfg(not(target_os = "windows"))]
#[test]
fn com_example_is_rejected_before_binding_on_non_windows() {
    let root = common::unique_tmp("jet_com_example_gate");
    let entry = root.join("run.jet");
    let source = include_str!("../examples/features/lowlevel/polyglot_com/run.jet");
    fs::write(&entry, source).unwrap();

    let diagnostics = jet::compile_with_path(source, entry.to_str().unwrap())
        .expect_err("the COM example must be gated on a non-Windows host");
    assert_eq!(diagnostics.first().map(|diagnostic| diagnostic.code.as_str()), Some("E3260"));
    let rendered = jet::render_diagnostics(entry.to_str().unwrap(), source, &diagnostics);
    assert!(rendered.contains("`com.*` needs a Windows host"), "{rendered}");
}
