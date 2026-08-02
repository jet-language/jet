//! Focused production legacy-bridge contract coverage for Epoch 4.

use jet::Comptime::Build::{BuildCapability, BuildPolicy};
use jet::Driver::{compile_bundle_path_build, BuildRunOptions};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

const LEGACY_TEMPLATE: &str = r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("exercise the typed legacy bridge") {
        tc :: b.toolchain("native", "x86_64-linux")?
        identity :: b.signing("builder", "ci")?
        imported :: b.legacy(
            "cargo",
            "legacy",
            __INPUTS__,
            __OUTPUTS__,
            __ARGV__,
            ["Exec", "FS"],
            tc,
            [],
            identity,
            "generic",
            [],
            [],
            [],
            [],
            [],
            "cached",
            "Cargo.toml"
        )?
        app :: b.add_executable("app", ["main.jet"], [imported])?
        return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#;

fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "jet-build-entry-epoch4-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn source(inputs: &str, outputs: &str, argv: &str) -> String {
    LEGACY_TEMPLATE
        .replace("__INPUTS__", inputs)
        .replace("__OUTPUTS__", outputs)
        .replace("__ARGV__", argv)
}

fn options(policy: BuildPolicy) -> BuildRunOptions {
    BuildRunOptions {
        grants: BTreeSet::from([BuildCapability::Exec, BuildCapability::FS]),
        policy,
        execute: false,
        allow_impure: true,
        inspect_only: false,
        emit_generated: false,
        locked: false,
        freestanding: false,
        web_target: false,
        plugin_target: false,
        cross_target: None,
        remote: None,
    }
}

fn write_entry(root: &Path, text: &str) -> PathBuf {
    let entry = root.join("main.jet");
    fs::write(&entry, text).unwrap();
    entry
}

#[test]
fn ci_denies_legacy_before_reading_a_missing_manifest() {
    let root = project("ci-policy-order");
    let entry = write_entry(
        &root,
        &source(
            "[\"Cargo.toml\"]",
            "[\"target/debug/cli\"]",
            "[\"cargo\", \"build\", \"--bin\", \"cli\"]",
        ),
    );

    let errors = compile_bundle_path_build(
        entry.to_str().unwrap(),
        options(BuildPolicy::ci_default()),
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|diagnostic| diagnostic.what.contains("legacy build wrappers are disabled in CI")),
        "CI policy must win over missing canonical manifest: {errors:#?}"
    );
    assert!(
        !errors
            .iter()
            .any(|diagnostic| diagnostic.what.contains("requires `Cargo.toml`")),
        "denied wrappers must not attempt canonical import: {errors:#?}"
    );
}

#[test]
fn allowed_legacy_bridge_rejects_stale_canonical_facts() {
    for (name, inputs, outputs, argv, expected) in [
        (
            "argv",
            "[\"Cargo.toml\"]",
            "[\"target/debug/cli\"]",
            "[\"cargo\", \"check\"]",
            "argv does not match",
        ),
        (
            "inputs",
            "[\"Cargo.toml\", \"extra.txt\"]",
            "[\"target/debug/cli\"]",
            "[\"cargo\", \"build\", \"--bin\", \"cli\"]",
            "inputs must exactly match",
        ),
        (
            "outputs",
            "[\"Cargo.toml\"]",
            "[\"target/debug/other\"]",
            "[\"cargo\", \"build\", \"--bin\", \"cli\"]",
            "outputs must exactly match",
        ),
    ] {
        let root = project(&format!("stale-{name}"));
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"legacy-contract\"\n\n[[bin]]\nname = \"cli\"\n",
        )
        .unwrap();
        let entry = write_entry(&root, &source(inputs, outputs, argv));
        let errors = compile_bundle_path_build(
            entry.to_str().unwrap(),
            options(BuildPolicy::allow_all()),
        )
        .unwrap_err();
        assert!(
            errors.iter().any(|diagnostic| diagnostic.what.contains(expected)),
            "stale {name} declaration must be rejected: {errors:#?}"
        );
    }
}
