//! Focused production legacy-bridge contract coverage for Epoch 4.

mod common;

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
        gates: jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
        inspect_only: false,
        emit_generated: false,
        locked: false,
        freestanding: false,
        web_target: false,
        plugin_target: false,
        cross_target: None,
        profile: "dev".to_string(),
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

#[test]
fn workspace_build_observes_dependency_order_and_execution_chain() {
    let root = project("workspace-dependency-order");
    let packages = root.join("packages");
    fs::create_dir_all(packages.join("a")).unwrap();
    fs::create_dir_all(packages.join("b")).unwrap();
    let workspace_source = r#"
use b

module workspace {
    members: ["./packages/a", "./packages/b"]
}

fn build(b: BuildContext) =[Exec]=> BuildPlan ? {
    #Impure("workspace must observe member b") {
        action :: b.action(
            "workspace-order",
            [],
            ["workspace-stamp"],
            ["sh", "-c", "printf workspace > workspace-stamp"],
            ["Exec"]
        )?
        app :: b.add_executable("workspace", ["workspace.jet"], [action])?
        return b.plan(app)
    }
    return b.plan()
}
"#
    .to_string();
    fs::write(root.join("workspace.jet"), workspace_source).unwrap();
    fs::write(
        root.join("package.jet"),
        "name: \"workspace\"\nversion: \"0.1.0\"\ndeps: { b: ./packages/b }\n",
    )
    .unwrap();
    fs::write(
        packages.join("a").join("package.jet"),
        "name: \"a\"\nversion: \"0.1.0\"\nbuild: { allow: #(Exec) }\n",
    )
    .unwrap();
    fs::write(
        packages.join("b").join("package.jet"),
        "name: \"b\"\nversion: \"0.1.0\"\ndeps: { a: ../a }\nbuild: { allow: #(Exec) }\n",
    )
    .unwrap();
    fs::write(
        packages.join("a").join("run.jet"),
        r#"
fn build(b: BuildContext) =[Exec]=> BuildPlan ? {
    #Impure("member a records execution") {
        action :: b.action(
            "a-order",
            [],
            ["a/a.jet"],
            ["sh", "-c", "printf 'fn a_ready() => Int { return 1 }' > a/a.jet"],
            ["Exec"]
        )?
        target :: b.add_library("a", ["run.jet"], [action])?
        return b.plan(target)
    }
    return b.plan()
}
fn run() {}
"#
        .to_string(),
    )
    .unwrap();
    fs::write(
        packages.join("b").join("run.jet"),
        r#"
use a

fn build(b: BuildContext) =[Exec]=> BuildPlan ? {
    #Impure("member b records its realized package") {
        action :: b.action(
            "b-order",
            [],
            ["b/b.jet"],
            ["sh", "-c", "printf 'fn b_ready() => Int { return 1 }' > b/b.jet"],
            ["Exec"]
        )?
        target :: b.add_library("b", ["run.jet"], [action])?
        return b.plan(target)
    }
    return b.plan()
}
fn run() {}
"#
        .to_string(),
    )
    .unwrap();

    let plan = jetpack::WorkspaceFile::load(&root)
        .expect("workspace manifest should be found")
        .expect("workspace manifest should parse");
    let ordered = jetpack::MemberSelect::dependency_order(&root, &plan.members).unwrap();
    let names: Vec<_> = ordered.iter().map(|member| member.name.as_str()).collect();
    assert_eq!(names, ["a", "b"]);

    jet::compile_programmable_build_opts(
        root.join("workspace.jet").to_str().unwrap(),
        &["exec".to_string()],
        false,
        jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
        false,
        false,
        false,
        None,
    )
    .expect("workspace and member entries should run through the production driver");
    assert!(packages.join("a/a/a.jet").is_file());
    assert!(packages.join("b/b/b.jet").is_file());
    assert_eq!(
        fs::read_to_string(root.join("workspace-stamp")).unwrap(),
        "workspace"
    );
}
