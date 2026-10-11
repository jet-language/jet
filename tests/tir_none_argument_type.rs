//! A `None` argument to a source-owned Core call types from its parameter.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

/// `files.walk_files(directory, None) ?? return ...` (the JetCli foreign-link
/// shape): the absent `ignore: String?` argument is a `String?`, not the
/// contextless `Int?` placeholder of a bare `None`.
#[test]
fn none_argument_to_core_source_call_takes_its_parameter_type() {
    let scratch = common::Scratch::new("none_argument_type");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    let path = scratch.path.join("main.jet");
    fs::write(
        &path,
        "use core.files as files\nfn count(directory: String) -> Int Err! {\n    entries :: files.walk_files(directory, None) ?? return Err(\"walk\", code: \"walk\")\n    Ok(entries.len())\n}\nfn run() {\n    print(count(\".\") ?? 0)\n}\n",
    )
    .unwrap();
    let (diagnostics, bundle, _, nominals) =
        jet::Driver::check_file_with_effect_facts(path.to_str().unwrap(), None, false);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
        "{diagnostics:#?}"
    );
    let bundle = bundle.expect("checked walk_files fixture");
    let request = jet_foundation::MIR::MirArtifactRequest::new(
        jet_foundation::MIR::MirArtifactTarget::RustAot,
        jet_foundation::MIR::MirArtifactKind::NativeExecutable,
        jet_foundation::MIR::MirArtifactBuildMode::Dev,
    );
    let (program, _) = jet::Codegen::TIR::lower_checked_mir_program_for(&bundle, &nominals, request)
        .expect("walk_files fixture lowers");
    let absent = program.functions.iter()
        .filter(|function| function.name.ends_with("count"))
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
        .filter(|instruction| matches!(instruction.operation, jet_foundation::MIR::MirOperation::Absent))
        .map(|instruction| format!("{:?}", instruction.ty))
        .collect::<Vec<_>>();
    assert!(absent.iter().any(|ty| ty.contains("kind: String")), "no String? None argument: {absent:#?}");
    for ty in &absent {
        assert!(!ty.contains("kind: Option(MirType { kind: Int"), "None argument kept the Int? placeholder: {ty}");
    }
}
