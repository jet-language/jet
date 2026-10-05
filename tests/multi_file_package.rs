//! Package member declarations and generic instances have one meaning on every tier.
mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

use std::path::PathBuf;
use std::process::Command;

const GOLDEN: &str = include_str!("../Examples/features/expected/modules/package_members.out");
const FILES: &[(&str, &str)] = &[
    ("package.jet", include_str!("../Examples/features/modules/package_members/package.jet")),
    ("run.jet", include_str!("../Examples/features/modules/package_members/run.jet")),
    ("shapes.jet", include_str!("../Examples/features/modules/package_members/shapes.jet")),
    ("slots.jet", include_str!("../Examples/features/modules/package_members/slots.jet")),
    ("inventory.jet", include_str!("../Examples/features/modules/package_members/inventory.jet")),
];

#[test]
fn package_members_have_all_tier_parity() {
    tir_support::assert_example_cli_tiers_agree("modules/package_members", GOLDEN);

    // The shared example helper covers O0, O2 and the interpreter. Also exercise
    // the public O1 build command, not a direct rustc compilation of its output.
    let scratch = common::Scratch::new("package_members_o1");
    for (name, source) in FILES {
        std::fs::write(scratch.join(name), source).unwrap();
    }
    let build = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["build", "run.jet"])
        .current_dir(&scratch.path)
        .env("JET_STORE_DIR", scratch.join("cache"))
        .env("JETPACK_ROOT", scratch.join("jetpack"))
        .env("JETPACK_ENV", "1")
        .env("NO_COLOR", "1")
        .output().unwrap();
    assert!(build.status.success(), "O1 build failed:\n{}", String::from_utf8_lossy(&build.stderr));
    let output = Command::new(scratch.join(".jet/build/run"))
        .current_dir(&scratch.path).output().unwrap();
    assert!(output.status.success(), "O1 execution failed:\n{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), GOLDEN);
}

#[test]
fn sibling_calls_keep_private_generic_functions_live() {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("Examples/features/modules/package_members/run.jet");
    let mut bundle = jet::Loader::load_entry_with_overlay(entry.to_str().unwrap(), None, false)
        .expect("load package members");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check);
    assert!(!diagnostics.iter().any(|d| d.severity == jet::Diagnostics::Severity::Error),
        "package member checking failed: {diagnostics:#?}");
    assert!(!diagnostics.iter().any(|d| d.code == "L0104"),
        "sibling calls must reach their private declaring functions: {diagnostics:#?}");
}
