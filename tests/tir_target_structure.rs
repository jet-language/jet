//! Structural guard for the independently compiled TIR feature-family targets.

use std::fs;
use std::path::Path;

#[test]
fn tir_integration_target_stays_split_by_feature_family() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        !root.join("tests/tir.rs").exists(),
        "tests/tir.rs must not recreate one giant TIR integration target"
    );

    for target in [
        "tir_collections_and_methods",
        "tir_control_and_data",
        "tir_core_and_closures",
        "tir_data_math_reactive",
        "tir_io_and_ownership",
        "tir_language_features",
        "tir_modules_and_enums",
        "tir_patterns_and_fields",
        "tir_unsafe_and_runtime",
    ] {
        let path = root.join(format!("tests/{target}.rs"));
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        assert!(
            source.lines().count() <= 800,
            "{target}.rs grew past its feature-family boundary"
        );
        assert!(
            !source.contains("include!(") && !source.contains("#[path = \"tir/"),
            "{target}.rs must remain an independent Cargo target, not a module shell"
        );
    }
}
