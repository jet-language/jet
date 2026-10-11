//! S48 trait slots declared in another module.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

/// `slot = Val(concrete)` into an optional field typed by a trait that
/// another module declares boxes the payload. The trait is recognized by its
/// checked identity, not only when the current module declares it.
#[test]
fn imported_trait_slot_assignment_boxes_the_payload() {
    let scratch = common::Scratch::new("imported_trait_slot_box");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    fs::write(
        scratch.path.join("adapter.jet"),
        "trait Adapter {\n    fn id(self) -> Int\n}\nstruct Config {\n    adapter: Adapter?{None}\n}\n",
    )
    .unwrap();
    let path = scratch.path.join("main.jet");
    fs::write(
        &path,
        "module adapter\nuse adapter.[Adapter, Config]\nstruct Concrete {\n    n: Int\n}\nimpl Concrete.Adapter {\n    fn id(self) -> Int { self.n }\n}\nfn run() {\n    config := Config{}\n    config.adapter = Val(Concrete{n: 1})\n    print(config.adapter == None)\n}\n",
    )
    .unwrap();
    let (diagnostics, bundle, _, nominals) =
        jet::Driver::check_file_with_effect_facts(path.to_str().unwrap(), None, false);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
        "{diagnostics:#?}"
    );
    let bundle = bundle.expect("checked imported trait fixture");
    let request = jet_foundation::MIR::MirArtifactRequest::new(
        jet_foundation::MIR::MirArtifactTarget::RustAot,
        jet_foundation::MIR::MirArtifactKind::NativeExecutable,
        jet_foundation::MIR::MirArtifactBuildMode::Dev,
    );
    let (program, _) = jet::Codegen::TIR::lower_checked_mir_program_for(&bundle, &nominals, request)
        .expect("imported trait slot assignment lowers");
    let boxes = program.functions.iter()
        .filter(|function| function.name.ends_with("run"))
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
        .any(|instruction| matches!(instruction.operation, jet_foundation::MIR::MirOperation::TraitBox { .. }));
    assert!(boxes, "the imported trait slot assignment did not box its payload");
}
