//! D-BUILDENTRY1/D-BUILDACTION1: real Jet `fn build` vertical.

mod common;

use jet::Comptime::Build::{ActionOutcome, BuildCapability, BuildPolicy, CacheHitReason};
use jet::Driver::{BuildQueryExpression, BuildRunOptions, compile_bundle_path_build};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jet-build-entry-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn opts() -> BuildRunOptions {
    BuildRunOptions {
        grants: BTreeSet::from([BuildCapability::Exec, BuildCapability::FS]),
        policy: BuildPolicy::allow_all(),
        execute: true,
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

fn ci_opts() -> BuildRunOptions {
    BuildRunOptions {
        policy: BuildPolicy::ci_default(),
        grants: BTreeSet::from([BuildCapability::Exec, BuildCapability::FS]),
        gates: jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
        ..opts()
    }
}

fn inspect_opts() -> BuildRunOptions {
    BuildRunOptions {
        execute: false,
        ..opts()
    }
}

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
}

fn first_file_under(path: &Path) -> PathBuf {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            return first_file_under(&path);
        }
        return path;
    }
    panic!("no cache blob under {}", path.display());
}

fn multi_dependency_fixture(name: &str) -> (PathBuf, PathBuf) {
    let root = project(name);
    let deps = root.join("deps");
    let dep_a = deps.join("dep_a");
    let dep_b = deps.join("dep_b");
    fs::create_dir_all(&dep_a).unwrap();
    fs::create_dir_all(&dep_b).unwrap();
    write(
        &root.join("package.jet"),
        "name: \"app\"\nversion: \"0.1.0\"\ndeps: { dep_a: ./deps/dep_a, dep_b: ./deps/dep_b }\n",
    );
    write(
        &dep_a.join("package.jet"),
        "name: \"dep_a\"\nversion: \"0.1.0\"\n",
    );
    write(
        &dep_b.join("package.jet"),
        "name: \"dep_b\"\nversion: \"0.1.0\"\n",
    );
    write(
        &dep_a.join("dep_a.jet"),
        "pub fn value() => Int { return 1 }\n",
    );
    let dep_b_source = dep_b.join("dep_b.jet");
    write(&dep_b_source, "pub fn value() => Int { return 2 }\n");
    write(
        &root.join("main.jet"),
        r#"
use dep_a
use dep_b

fn build(b: BuildContext) => BuildPlan ? {
    app :: b.add_executable("app", ["main.jet"], [])?
    return b.plan(app)
}

fn run() { print(dep_a.value()); print(dep_b.value()) }
"#,
    );
    (root, dep_b_source)
}

fn median_micros(samples: &mut [u128]) -> u128 {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[test]
fn root_fn_build_executes_graph_materializes_and_frontend_checks_generated_source() {
    let root = project("vertical");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    b.generate("generated_message", "fn generated_message() => String {{ return \"built\" }}")?
    #Impure("write declared build output") {
    stamp :: b.action(
        "stamp",
        [],
        [".jet/generated/app/stamp.txt"],
        ["sh", "-c", "printf stamped > .jet/generated/app/stamp.txt"],
        ["Exec", "FS"]
    )?
    app :: b.add_executable("app", ["main.jet", ".jet/generated/main/generated_message.jet"], [stamp])?
    return b.plan(app)
    }
    return b.plan()
}

fn run() { print("ok") }
"#,
    );

    let first = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
    let build = first.build.expect("root fn build should run");
    assert_eq!(build.plan.targets()[0].name, "app");
    assert_eq!(build.plan.actions()[0].name, "stamp");
    assert_eq!(build.execution.metrics.actions_total, 1);
    assert!(matches!(
        build.execution.events.last(),
        Some(jet::Comptime::Build::BuildExecutionEvent::Finished {
            outcome: ActionOutcome::Succeeded { exit_code: 0 },
            ..
        })
    ));
    assert_eq!(
        fs::read_to_string(root.join(".jet/generated/app/stamp.txt")).unwrap(),
        "stamped"
    );
    let generated = &build.generated[0];
    assert_eq!(generated.name, "generated_message");
    assert!(generated.path.exists());
    assert!(generated.digest.as_str().starts_with("sha256:"));
    let lock = fs::read_to_string(root.join(".jet/lock")).unwrap();
    assert!(lock.contains(".jet/generated/main/generated_message.jet"));
    assert!(lock.contains(generated.digest.as_str()));
    assert!(first.compile.rust.contains("fn main"));
    assert!(first.compile.rust.contains("generated_message"));

    let second = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
    let build = second.build.unwrap();
    assert!(build.execution.events.iter().any(|event| matches!(
        event,
        jet::Comptime::Build::BuildExecutionEvent::Finished {
            outcome: ActionOutcome::RestoredFromCache,
            ..
        }
    )));
    let rebuilt = build
        .plan
        .last_rebuild_explanation(&root, "stamp")
        .unwrap()
        .expect("real execution must persist rebuild provenance");
    assert_eq!(
        rebuilt.status,
        jet::Comptime::Build::ActionCacheStatus::Hit(
            CacheHitReason::LocalActionRecordMatched
        )
    );
    assert_eq!(rebuilt.reason, "local action record matched");
    let explain = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "inspect",
            "explain-build",
            "stamp",
            entry.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("jet inspect explain-build");
    assert!(
        explain.status.success(),
        "explain-build failed: {}",
        String::from_utf8_lossy(&explain.stderr)
    );
    assert!(
        String::from_utf8_lossy(&explain.stdout)
            .contains("rebuild=local action record matched"),
        "explain-build must expose real cache provenance: {}",
        String::from_utf8_lossy(&explain.stdout)
    );
}

#[test]
fn package_manifest_build_entry_uses_the_same_pipeline_as_a_file_entry() {
    let root = project("package-entry");
    write(
        &root.join("package.jet"),
        r#"
name: "package-entry"
version: "0.1.0"
fn build(b: BuildContext) => BuildPlan ? {
    b.generate("package_message", "fn package_message() => String {{ return \"package\" }}")?
    app :: b.add_executable("app", ["main.jet", ".jet/generated/package-entry/package_message.jet"], [])?
    return b.plan(app)
}

"#,
    );
    let entry = root.join("main.jet");
    write(
        &entry,
        "fn run() { print(package_message()) }\n",
    );

    let output = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
    let build = output.build.expect("package.jet fn build should be selected");
    assert_eq!(build.plan.targets()[0].name, "app");
    assert_eq!(build.generated.len(), 1);
    assert!(output.compile.rust.contains("package_message"));
}

#[test]
fn multi_dependency_build_restores_every_unchanged_compiler_artifact() {
    let (root, _) = multi_dependency_fixture("package-artifact-restore");
    let entry = root.join("main.jet");

    let first = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
    let first_build = first.build.expect("first dependency build should run");
    let compiler_actions = first_build
        .plan
        .actions()
        .iter()
        .filter(|action| action.is_compiler_owned())
        .collect::<Vec<_>>();
    assert_eq!(compiler_actions.len(), 3, "root plus two dependencies");
    assert_eq!(first_build.execution.metrics.cache_restored_actions, 0);
    assert_eq!(
        first_build
            .execution
            .events
            .iter()
            .filter(|event| matches!(
                event,
                jet::Comptime::Build::BuildExecutionEvent::Finished {
                    action,
                    outcome: ActionOutcome::Succeeded { .. },
                } if first_build.plan.action_handle(*action).and_then(|handle| first_build.plan.action(handle)).is_some_and(|action| action.is_compiler_owned())
            ))
            .count(),
        3
    );

    let second = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
    let second_build = second.build.expect("warm dependency build should run");
    assert_eq!(second_build.execution.metrics.cache_restored_actions, 3);
    assert_eq!(
        second_build
            .execution
            .events
            .iter()
            .filter(|event| matches!(
                event,
                jet::Comptime::Build::BuildExecutionEvent::Finished {
                    action,
                    outcome: ActionOutcome::RestoredFromCache,
                } if second_build.plan.action_handle(*action).and_then(|handle| second_build.plan.action(handle)).is_some_and(|action| action.is_compiler_owned())
            ))
            .count(),
        3
    );
    assert!(!second_build.execution.events.iter().any(|event| matches!(
        event,
        jet::Comptime::Build::BuildExecutionEvent::Finished {
            action,
            outcome: ActionOutcome::Succeeded { .. },
        } if second_build.plan.action_handle(*action).and_then(|handle| second_build.plan.action(handle)).is_some_and(|action| action.is_compiler_owned())
    )));
    for action in second_build
        .plan
        .actions()
        .iter()
        .filter(|action| action.is_compiler_owned())
    {
        assert!(root.join(action.outputs[0].as_str()).is_file());
    }
}

#[test]
fn warm_dependency_cache_still_runs_frontend_diagnostics() {
    let (root, dep_b_source) = multi_dependency_fixture("malformed-dependent-warm-cache");
    let entry = root.join("main.jet");
    let first = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
    let artifact = root.join(".jet/build-cache/package-artifacts/dep_b.sealed");
    assert!(artifact.is_file(), "first build must seal dep_b");
    let before = fs::read(&artifact).unwrap();

    write(&dep_b_source, "pub fn value() => Int { return 2\n");
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap_err();
    assert!(!errors.is_empty());
    assert!(errors.iter().all(|diagnostic| diagnostic.code != "ICE"));
    assert!(errors.iter().any(|diagnostic| diagnostic.code.starts_with('E')));
    assert_eq!(fs::read(&artifact).unwrap(), before);
    drop(first);
}

#[test]
fn compiler_self_speed_reports_clean_and_incremental_medians() {
    let (root, _) = multi_dependency_fixture("compiler-self-speed");
    let entry = root.join("main.jet");
    let mut clean = Vec::new();
    for _ in 0..3 {
        let _ = fs::remove_dir_all(root.join(".jet/build-cache"));
        let start = Instant::now();
        compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
        clean.push(start.elapsed().as_micros());
    }
    let mut incremental = Vec::new();
    for _ in 0..3 {
        let start = Instant::now();
        let output = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
        let build = output.build.expect("incremental build should expose execution");
        assert_eq!(build.execution.metrics.cache_restored_actions, 3);
        incremental.push(start.elapsed().as_micros());
    }
    let clean_median = median_micros(&mut clean);
    let incremental_median = median_micros(&mut incremental);
    eprintln!(
        "compiler self-speed: clean_median_us={} incremental_median_us={} samples=3",
        clean_median, incremental_median
    );
    assert!(clean_median > 0);
    assert!(incremental_median > 0);
}

#[test]
fn package_and_file_build_entries_are_rejected_as_one_unit() {
    let root = project("package-entry-conflict");
    write(
        &root.join("package.jet"),
        "name: \"package-entry-conflict\"\nversion: \"0.1.0\"\nfn build(b: BuildContext) => BuildPlan ? { return b.plan() }\n",
    );
    let entry = root.join("main.jet");
    write(
        &entry,
        "fn build(b: BuildContext) => BuildPlan ? { return b.plan() }\nfn run() {}\n",
    );

    let errors = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap_err();
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E3520"));
}

#[test]
fn package_build_entry_is_discovered_from_one_unimported_source_file() {
    let root = project("package-source-entry");
    write(
        &root.join("package.jet"),
        "name: \"package-source-entry\"\nversion: \"0.1.0\"\n",
    );
    write(&root.join("run.jet"), "fn run() {}\n");
    fs::create_dir_all(root.join("tools")).unwrap();
    write(
        &root.join("tools/build.jet"),
        "fn build(b: BuildContext) => BuildPlan ? { target :: b.add_library(\"discovered\", [\"run.jet\"], [])?; return b.plan(target) }\n",
    );

    let output = compile_bundle_path_build(root.join("run.jet").to_str().unwrap(), opts())
        .expect("an unimported package source may own fn build");
    assert_eq!(output.build.unwrap().plan.targets()[0].name, "discovered");
}

#[test]
fn imported_build_function_is_not_a_package_entry() {
    let root = project("imported-build-entry");
    write(
        &root.join("package.jet"),
        "name: \"imported-build-entry\"\nversion: \"0.1.0\"\n",
    );
    write(
        &root.join("run.jet"),
        "use \"./tools/build\" as build_tool\nfn run() {}\n",
    );
    fs::create_dir_all(root.join("tools")).unwrap();
    write(
        &root.join("tools/build.jet"),
        "fn build(b: BuildContext) => BuildPlan ? { return b.plan() }\n",
    );

    let output = compile_bundle_path_build(root.join("run.jet").to_str().unwrap(), opts())
        .expect("an imported fn build is not a package build authority");
    assert!(output.build.is_none());
}

#[test]
fn package_build_entry_duplicates_name_both_source_locations() {
    let root = project("package-source-conflict");
    write(
        &root.join("package.jet"),
        "name: \"package-source-conflict\"\nversion: \"0.1.0\"\n",
    );
    write(&root.join("run.jet"), "fn run() {}\n");
    write(
        &root.join("a.jet"),
        "fn build(b: BuildContext) => BuildPlan ? { return b.plan() }\n",
    );
    write(
        &root.join("b.jet"),
        "fn build(b: BuildContext) => BuildPlan ? { return b.plan() }\n",
    );

    let errors = compile_bundle_path_build(root.join("run.jet").to_str().unwrap(), opts())
        .expect_err("two package build entries must be rejected");
    let diagnostic = errors
        .iter()
        .find(|diagnostic| diagnostic.code == "E3520")
        .expect("duplicate build entries use the registered conflict diagnostic");
    assert!(diagnostic.what.contains("a.jet:1"), "{}", diagnostic.what);
    assert!(diagnostic.what.contains("b.jet:1"), "{}", diagnostic.what);
}

#[test]
fn package_build_discovery_stops_at_nested_package_boundary() {
    let root = project("package-boundary");
    write(
        &root.join("package.jet"),
        "name: \"package-boundary\"\nversion: \"0.1.0\"\n",
    );
    write(&root.join("run.jet"), "fn run() {}\n");
    fs::create_dir_all(root.join("tools")).unwrap();
    write(
        &root.join("tools/build.jet"),
        "fn build(b: BuildContext) => BuildPlan ? { target :: b.add_library(\"root\", [\"run.jet\"], [])?; return b.plan(target) }\n",
    );
    fs::create_dir_all(root.join("packages/nested/tools")).unwrap();
    write(
        &root.join("packages/nested/package.jet"),
        "name: \"nested\"\nversion: \"0.1.0\"\n",
    );
    write(&root.join("packages/nested/run.jet"), "fn run() {}\n");
    write(
        &root.join("packages/nested/tools/build.jet"),
        "fn build(b: BuildContext) => BuildPlan ? { target :: b.add_library(\"nested\", [\"run.jet\"], [])?; return b.plan(target) }\n",
    );

    let output = compile_bundle_path_build(root.join("run.jet").to_str().unwrap(), opts())
        .expect("nested package build entries are not part of the root package");
    assert_eq!(output.build.unwrap().plan.targets()[0].name, "root");
}

#[test]
fn file_local_duplicate_build_entries_name_both_sites() {
    let root = project("file-entry-conflict");
    let entry = root.join("main.jet");
    write(
        &entry,
        "fn build(b: BuildContext) => BuildPlan ? { return b.plan() }\n\nfn build(b: BuildContext) => BuildPlan ? { return b.plan() }\nfn run() {}\n",
    );

    let errors = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap_err();
    let diagnostic = errors
        .iter()
        .find(|diagnostic| diagnostic.code == "E3520")
        .expect("duplicate file-local build entries use the conflict diagnostic");
    assert!(diagnostic.what.contains("main.jet:1"), "{}", diagnostic.what);
    assert!(diagnostic.what.contains("main.jet:3"), "{}", diagnostic.what);
}

#[test]
fn workspace_build_uses_batteries_for_missing_member_and_root_entries() {
    let root = project("workspace-entry-fallback");
    let packages = root.join("packages");
    fs::create_dir_all(packages.join("a/tools")).unwrap();
    fs::create_dir_all(packages.join("b")).unwrap();
    write(
        &root.join("workspace.jet"),
        "module workspace { members: [\"./packages/a\", \"./packages/b\"] }\nfn run() { print(\"workspace\") }\n",
    );
    write(
        &packages.join("a/package.jet"),
        "name: \"a\"\nversion: \"0.1.0\"\n",
    );
    write(&packages.join("a/run.jet"), "fn run() {}\n");
    write(
        &packages.join("a/tools/build.jet"),
        "fn build(b: BuildContext) => BuildPlan ? { b.generate(\"a_generated\", \"fn a_generated() => String {{ return \\\"a\\\" }}\")?; target :: b.add_library(\"a\", [\"run.jet\", \".jet/generated/a/a_generated.jet\"], [])?; return b.plan(target) }\n",
    );
    write(
        &packages.join("b/package.jet"),
        "name: \"b\"\nversion: \"0.1.0\"\ndeps: { a: ../a }\n",
    );
    write(&packages.join("b/run.jet"), "fn run() {}\n");

    let output = jet::compile_programmable_build_opts(
        root.join("workspace.jet").to_str().unwrap(),
        &[],
        false,
        jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
        false,
        false,
        false,
        None,
    )
    .expect("workspace build should fall back to batteries for missing fn build");
    assert!(packages.join("a/.jet/generated/a/a_generated.jet").is_file());
    assert!(output.rust.contains("workspace"));
}

#[test]
fn production_build_bridge_imports_only_the_canonical_legacy_project_file() {
    let root = project("legacy-import");
    let entry = root.join("main.jet");
    write(&root.join("Cargo.toml"), "[package]\nname = \"legacy-import\"\n");
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("invoke one explicitly imported legacy project file") {
        tc :: b.toolchain("native", "x86_64-linux")?
        identity :: b.signing("builder", "ci")?
        imported :: b.legacy(
            "cargo",
            "cargo",
            ["Cargo.toml"],
            ["target/debug/legacy-import"],
            ["cargo", "build", "--bin", "legacy-import"],
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
"#,
    );

    let errors = compile_bundle_path_build(entry.to_str().unwrap(), ci_opts()).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|diagnostic| diagnostic.what.contains("legacy build wrappers are disabled in CI")),
        "{errors:#?}"
    );
}

#[test]
fn production_legacy_import_uses_project_contents_for_the_typed_action() {
    let root = project("legacy-content");
    let entry = root.join("main.jet");
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"legacy-content\"\nversion = \"1.2.3\"\n\n[[bin]]\nname = \"cli\"\n",
    );
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("inspect the canonical Cargo import") {
        tc :: b.toolchain("native", "x86_64-linux")?
        identity :: b.signing("builder", "ci")?
        imported :: b.legacy(
            "cargo",
            "cargo-import",
            ["Cargo.toml"],
            ["target/debug/cli"],
            ["cargo", "build", "--bin", "cli"],
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
"#,
    );

    let output = compile_bundle_path_build(entry.to_str().unwrap(), inspect_opts()).unwrap();
    let build = output.build.expect("legacy import should produce a build plan");
    let action = build
        .plan
        .actions()
        .iter()
        .find(|action| action.name == "cargo-import")
        .expect("imported action");
    assert_eq!(
        action.argv,
        vec!["cargo", "build", "--bin", "cli"]
    );
    assert_eq!(
        action
            .inputs
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        vec!["Cargo.toml"]
    );
    assert_eq!(
        action
            .outputs
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        vec!["target/debug/cli"]
    );
    assert_eq!(
        action.labels.get("legacy.version").map(String::as_str),
        Some("1.2.3")
    );
    assert_eq!(
        action.labels.get("legacy.target").map(String::as_str),
        None
    );
}

#[test]
fn production_legacy_import_rejects_unsupported_project_constructs() {
    let root = project("legacy-unsupported");
    let entry = root.join("main.jet");
    write(
        &root.join("CMakeLists.txt"),
        "add_custom_command(OUTPUT generated COMMAND sh -c \"touch generated\")\n",
    );
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("reject unsupported CMake import") {
        tc :: b.toolchain("native", "x86_64-linux")?
        identity :: b.signing("builder", "ci")?
        imported :: b.legacy(
            "cmake",
            "cmake-import",
            ["CMakeLists.txt"],
            ["build/app"],
            ["cmake", "--build", "build"],
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
            "CMakeLists.txt"
        )?
        app :: b.add_executable("app", ["main.jet"], [imported])?
        return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );

    let errors = compile_bundle_path_build(entry.to_str().unwrap(), inspect_opts()).unwrap_err();
    assert!(errors.iter().any(|diagnostic| {
        diagnostic.what.contains("unsupported construct")
            && diagnostic.what.contains("add_custom_command")
    }), "{errors:#?}");
}

#[test]
fn workspace_build_runs_members_in_dependency_order_then_its_own_plan() {
    let root = project("workspace-entry");
    let packages = root.join("packages");
    fs::create_dir_all(packages.join("a")).unwrap();
    fs::create_dir_all(packages.join("b")).unwrap();
    write(
        &root.join("workspace.jet"),
        r#"
module workspace {
    members: ["./packages/a", "./packages/b"]
}

fn build(b: BuildContext) => BuildPlan ? {
    app :: b.add_executable("workspace", ["workspace.jet"], [])?
    return b.plan(app)
}
"#,
    );
    write(
        &packages.join("a").join("package.jet"),
        r#"
name: "a"
version: "0.1.0"
    fn build(b: BuildContext) => BuildPlan ? {
    b.generate("a_generated", "fn a_generated() => String {{ return \"a\" }}")?
    target :: b.add_library("a", [".jet/generated/a/a_generated.jet"], [])?
    return b.plan(target)
}
"#,
    );
    write(
        &packages.join("b").join("package.jet"),
        "name: \"b\"\nversion: \"0.1.0\"\ndeps: { a: ../a }\n",
    );
    write(
        &packages.join("b").join("run.jet"),
        "fn build(b: BuildContext) => BuildPlan ? {\n    b.generate(\"b_generated\", \"fn b_generated() => String {{ return \\\"b\\\" }}\")?\n    target :: b.add_library(\"b\", [\"run.jet\", \".jet/generated/b/b_generated.jet\"], [])?\n    return b.plan(target)\n}\nfn run() {}\n",
    );

    let output = jet::compile_programmable_build_opts(
        root.join("workspace.jet").to_str().unwrap(),
        &[],
        false,
        jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
        false,
        false,
        false,
        None,
    )
    .expect("workspace and member build entries should share the Driver pipeline");
    assert!(root
        .join("packages/a/.jet/generated/a/a_generated.jet")
        .is_file());
    assert!(root
        .join("packages/b/.jet/generated/b/b_generated.jet")
        .is_file());
    assert!(output.rust.contains("workspace"));
}

#[test]
fn workspace_cli_grant_does_not_authorize_member_builds() {
    let root = project("workspace-grant-boundary");
    let member = root.join("packages/member");
    fs::create_dir_all(&member).unwrap();
    write(
        &root.join("workspace.jet"),
        "module workspace { members: [\"./packages/member\"] }\nfn build(b: BuildContext) => BuildPlan ? { return b.plan() }\n",
    );
    write(
        &member.join("package.jet"),
        "name: \"member\"\nversion: \"0.1.0\"\n",
    );
    write(
        &member.join("run.jet"),
        r#"
fn build(b: BuildContext) =[Exec]=> BuildPlan ? {
    #Impure("member must use its own grant") {
        action :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf bad > stamp"], ["Exec"])?
        app :: b.add_executable("app", ["run.jet"], [action])?
        return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );

    let errors = jet::compile_programmable_build_opts(
        root.join("workspace.jet").to_str().unwrap(),
        &["exec".to_string()],
        false,
        jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
        false,
        false,
        false,
        None,
    )
    .unwrap_err();
    assert!(
        errors.iter().any(|diagnostic| diagnostic.code == "E3504"),
        "member must not inherit the workspace CLI grant: {errors:#?}"
    );
    assert!(!member.join("stamp").exists());
}

#[test]
fn failed_action_replaces_stale_rebuild_provenance() {
    let root = project("failed-provenance");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec]=> BuildPlan ? {
    #Impure("run declared failing action") {
        fail :: b.action("fail", [], ["never"], ["sh", "-c", "exit 23"], ["Exec"])?
        app :: b.add_executable("app", ["main.jet"], [fail])?
        return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );
    assert!(compile_bundle_path_build(entry.to_str().unwrap(), opts()).is_err());
    let plan = jet::Driver::query_build_plan(entry.to_str().unwrap())
        .unwrap()
        .unwrap();
    let explanation = plan
        .last_rebuild_explanation(&root, "fail")
        .unwrap()
        .expect("failed execution must persist provenance");
    assert_eq!(
        explanation.reason,
        "action failed with exit code 23 after no local action record"
    );
}

#[test]
fn cache_restore_provenance_distinguishes_missing_and_invalid_blobs() {
    for (case, damage, expected) in [
        (
            "missing",
            "remove",
            jet::Comptime::Build::CacheMissReason::DeclaredOutputMissing,
        ),
        (
            "invalid",
            "corrupt",
            jet::Comptime::Build::CacheMissReason::CacheRecordInvalid,
        ),
        (
            "invalid-record",
            "corrupt-record",
            jet::Comptime::Build::CacheMissReason::CacheRecordInvalid,
        ),
    ] {
        let root = project(&format!("restore-{case}"));
        let entry = root.join("main.jet");
        write(
            &entry,
            r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("write declared cached output") {
        emit :: b.action("emit", [], ["artifact"], ["sh", "-c", "printf fresh > artifact"], ["Exec", "FS"])?
        app :: b.add_executable("app", ["main.jet"], [emit])?
        return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
        );
        compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
        if damage == "corrupt-record" {
            let record = first_file_under(&root.join(".jet/build-cache/actions"));
            fs::write(record, "not an action record").unwrap();
        } else {
            let blob = first_file_under(&root.join(".jet/build-cache/cas/blobs"));
            if damage == "remove" {
                fs::remove_file(blob).unwrap();
            } else {
                fs::write(blob, "corrupt").unwrap();
            }
        }
        let rebuilt = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
        let explanation = rebuilt
            .build
            .unwrap()
            .plan
            .last_rebuild_explanation(&root, "emit")
            .unwrap()
            .unwrap();
        assert_eq!(
            explanation.status,
            jet::Comptime::Build::ActionCacheStatus::Miss(expected),
            "{case} restore failure was misclassified"
        );
    }
}

#[test]
fn malformed_generated_source_is_a_jet_diagnostic_before_codegen() {
    let root = project("bad-generated");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) => BuildPlan ? {
    b.generate("broken", "fn nope(")?
    app :: b.add_executable("app", ["main.jet", ".jet/generated/main/broken.jet"], [])?
    return b.plan(app)
}
fn run() {}
"#,
    );
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default())
        .unwrap_err();
    assert!(!errors.is_empty());
    assert!(errors.iter().all(|d| d.code != "ICE"));
    assert!(errors.iter().any(|d| d.what.contains("generated")));
    assert!(!root.join(".jet/generated/main/broken.jet").exists());
    assert!(!root.join(".jet/lock").exists());
}

#[test]
fn imported_fn_build_never_runs_and_bad_root_signature_is_e3501() {
    let root = project("selection");
    let dep = root.join("dep.jet");
    write(
        &dep,
        r#"
fn build(b: BuildContext) => BuildPlan ? {
    b.generate("should_not_exist", "fn hidden() {{}}")?
    return b.plan()
}
pub fn helper() {}
"#,
    );
    let entry = root.join("main.jet");
    write(&entry, "use \"./dep\" as dep\nfn run() { dep.helper() }\n");
    let out = compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default()).unwrap();
    assert!(out.build.is_none());
    assert!(!root.join(".jet/generated").exists());

    write(&entry, "fn build() => Int { return 1 }\nfn run() {}\n");
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default())
        .unwrap_err();
    assert!(errors.iter().any(|d| d.code == "E3501"));
}

#[test]
fn ungranted_action_fails_before_process_spawn() {
    let root = project("grant");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("exercise denied authority") {
    action :: b.action("escape", [], ["out"], ["sh", "-c", "printf bad > out"], ["Exec"])?
    app :: b.add_executable("app", ["main.jet"], [action])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default())
        .unwrap_err();
    assert!(errors.iter().any(|d| d.code == "E3503"));
    assert!(!root.join("out").exists());
}

#[test]
fn action_generated_jet_reenters_frontend_before_runtime_codegen() {
    let root = project("action-generated");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("generate declared source") {
    action :: b.action(
        "bad-gen",
        [],
        [".jet/generated/main/bad.jet"],
        ["sh", "-c", "printf 'fn nope(' > .jet/generated/main/bad.jet"],
        ["Exec", "FS"]
    )?
    app :: b.add_executable("app", ["main.jet", ".jet/generated/main/bad.jet"], [action])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap_err();
    assert!(errors.iter().any(|diag| diag.what.contains("generated action `bad-gen`")));
    assert!(!root.join(".jet/generated/main/bad.jet").exists());
}

#[test]
fn jet_build_command_runs_selected_build_entry() {
    let root = project("cli");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("write CLI test output") {
    action :: b.action(
        "stamp",
        [],
        ["stamp.txt"],
        ["sh", "-c", "printf cli-built > stamp.txt"],
        ["Exec", "FS"]
    )?
    app :: b.add_executable("app", ["main.jet"], [action])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() { print("ok") }
"#,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .current_dir(&root)
        .arg("build")
        .arg(&entry)
        .arg("--allow-exec")
        .arg("--allow-fs")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(root.join("stamp.txt")).unwrap(), "cli-built");
}

#[test]
fn jet_build_positional_name_resolves_one_workspace_member() {
    let root = project("workspace-member-cli");
    let member = root.join("packages/one");
    fs::create_dir_all(&member).unwrap();
    write(
        &root.join("workspace.jet"),
        "module workspace { members: [\"./packages/one\"] }\n",
    );
    write(
        &member.join("package.jet"),
        "name: \"one\"\nversion: \"0.1.0\"\n",
    );
    write(&member.join("run.jet"), "fn run() { print(\"one\") }\n");
    fs::create_dir_all(member.join("tools")).unwrap();
    write(
        &member.join("tools/build.jet"),
        "fn build(b: BuildContext) => BuildPlan ? { b.generate(\"member_generated\", \"fn member_generated() => String {{ return \\\"one\\\" }}\")?; app :: b.add_executable(\"one\", [\"run.jet\", \".jet/generated/one/member_generated.jet\"], [])?; return b.plan(app) }\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .current_dir(&root)
        .args(["build", "one"])
        .output()
        .expect("jet build <member>");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(member.join(".jet/generated/one/member_generated.jet").is_file());
}

#[test]
fn graph_query_is_static_json_and_lsp_check_sees_bad_signature() {
    let root = project("query");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) => BuildPlan ? {
    action :: b.action("never-run", [], ["out"], ["sh", "-c", "exit 99"], [])?
    app :: b.add_executable("app", ["main.jet"], [action])?
    return b.plan(app)
}
fn run() {}
"#,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "graph"])
        .arg(&entry)
        .arg("--json")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let json = String::from_utf8_lossy(&output.stdout);
    assert!(json.contains("\"name\":\"app\""));
    assert!(json.contains("\"name\":\"never-run\""));
    assert!(!root.join("out").exists(), "graph query must never execute actions");
    let lsp_plan = jet::Driver::query_build_plan(entry.to_str().unwrap()).unwrap().unwrap();
    assert_eq!(lsp_plan.graph().targets[0].name, "app");
    assert_eq!(lsp_plan.graph().actions[0].name, "never-run");

    write(&entry, "fn build() => Int { return 1 }\nfn run() {}\n");
    let (diags, _) = jet::Driver::check_file(entry.to_str().unwrap(), None, true);
    assert!(diags.iter().any(|diag| diag.code == "E3501"));
}

#[test]
fn graph_query_inspects_declared_effects_without_execution_grants() {
    let root = project("query-effects");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("declare an inspectable action") {
        action :: b.action("never-run", [], ["out"], ["sh", "-c", "exit 91"], ["Exec", "FS"])?
        app :: b.add_executable("app", ["main.jet"], [action])?
        return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );
    let plan = jet::Driver::evaluate_build_query(
        entry.to_str().unwrap(),
        BuildQueryExpression::Build,
    )
    .unwrap()
    .expect("declared effects remain inspectable without execution grants");
    assert_eq!(plan.actions()[0].name, "never-run");
    assert!(!root.join("out").exists(), "query must never execute action");
}

#[test]
fn graph_query_denies_ambient_impure_effects_before_host_side_effects() {
    let root = project("query-denies-ambient");
    let entry = root.join("main.jet");
    let marker = root.join("must-not-exist");
    write(
        &entry,
        &format!(
            r#"
use core.files as files
use core.env as env
use core.process as process

fn build(b: BuildContext) => BuildPlan ? {{
    #Impure("hostile inspection probe") {{
        write_result :: files.write("{}", "owned")
        env.set("JET_QUERY_MUST_NOT_SET", "owned")
        process_result :: process.run(["sh", "-c", "exit 97"])
    }}
    return b.plan()
}}
fn run() {{}}
"#,
            marker.display()
        ),
    );

    std::env::remove_var("JET_QUERY_MUST_NOT_SET");
    let diagnostics = jet::Driver::evaluate_build_query(
        entry.to_str().unwrap(),
        BuildQueryExpression::Build,
    )
    .expect_err("inspection must reject ambient comptime authority");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3411"),
        "unexpected diagnostics: {diagnostics:?}"
    );
    assert!(!marker.exists(), "inspection wrote to the host filesystem");
    assert_eq!(std::env::var_os("JET_QUERY_MUST_NOT_SET"), None);
}

#[test]
fn graph_query_denies_each_ambient_authority_class() {
    let cases = [
        (
            "filesystem",
            "core.files",
            "attempt :: api.write(\"blocked\", \"owned\")",
        ),
        (
            "environment",
            "core.env",
            "api.set(\"JET_QUERY_BLOCKED\", \"owned\")",
        ),
        (
            "exec-process",
            "core.process",
            "attempt :: api.run([\"sh\", \"-c\", \"exit 97\"])",
        ),
        (
            "network",
            "core.net",
            "attempt :: api.tcp_listen(\"127.0.0.1:0\")",
        ),
    ];
    for (name, module, call) in cases {
        let root = project(&format!("query-denies-{name}"));
        let entry = root.join("main.jet");
        write(
            &entry,
            &format!(
                "use {module} as api\nfn build(b: BuildContext) => BuildPlan ? {{\n    #Impure(\"hostile {name} probe\") {{ {call} }}\n    return b.plan()\n}}\nfn run() {{}}\n"
            ),
        );
        let diagnostics = jet::Driver::evaluate_build_query(
            entry.to_str().unwrap(),
            BuildQueryExpression::Build,
        )
        .expect_err("inspection must reject ambient comptime authority");
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == "E3411"),
            "{name} escaped inspection authority: {diagnostics:?}"
        );
    }
}

#[test]
fn graph_overlay_uses_unsaved_text_and_canonical_cli_facts() {
    let root = project("query-overlay");
    let entry = root.join("main.jet");
    write(&entry, "fn build(b: BuildContext) => BuildPlan ? { app :: b.add_executable(\"disk\", [\"main.jet\"], [])?\n return b.plan(app) }\nfn run() {}\n");
    let unsaved = "fn build(b: BuildContext) => BuildPlan ? { app :: b.add_executable(\"unsaved\", [\"main.jet\"], [])?\n return b.plan(app) }\nfn run() {}\n";
    let disk = jet::Driver::query_build_plan(entry.to_str().unwrap()).unwrap().unwrap();
    let overlay = jet::Driver::query_build_plan_with_overlay(entry.to_str().unwrap(), unsaved).unwrap().unwrap();
    assert_eq!(disk.targets()[0].name, "disk");
    assert_eq!(overlay.targets()[0].name, "unsaved");
    let json = jet::Driver::build_plan_json(&overlay);
    assert!(json.contains("\"files\"") && json.contains("\"toolchains\"") && json.contains("\"generated\""));
    let editor_json = jet::LSP::build_graph_json(entry.to_str().unwrap(), unsaved)
        .unwrap()
        .expect("overlay has build graph");
    assert_eq!(editor_json, json, "editor and CLI must serialize one BuildPlan graph");
    let queried = jet::Driver::evaluate_build_query(
        entry.to_str().unwrap(),
        BuildQueryExpression::Build,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        jet::Driver::build_plan_json(&queried),
        jet::Driver::build_plan_json(&disk),
        "fixed `build` expression must evaluate typed graph facts"
    );
}

#[test]
fn unselected_action_output_never_runs_checks_or_leaks() {
    let root = project("selected-closure");
    let entry = root.join("main.jet");
    write(&entry, r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("declare selected closure") {
    bad :: b.action("unselected", [], ["missing-generated.jet"], ["sh", "-c", "exit 77"], ["Exec", "FS"])?
    ignored :: b.add_executable("ignored", ["missing-generated.jet"], [bad])?
    app :: b.add_executable("app", ["main.jet"], [])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#);
    let output = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
    assert_eq!(output.build.unwrap().execution.metrics.actions_total, 0);
    assert!(!root.join("missing-generated.jet").exists());
}

#[test]
fn unselected_malformed_generate_is_never_materialized_or_checked() {
    let root = project("unselected-generate");
    let entry = root.join("main.jet");
    write(&entry, r#"
fn build(b: BuildContext) => BuildPlan ? {
    b.generate("ignored", "fn broken(")?
    app :: b.add_executable("app", ["main.jet"], [])?
    return b.plan(app)
}
fn run() {}
"#);
    compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default()).unwrap();
    assert!(!root.join(".jet/generated/main/ignored.jet").exists());
}

#[test]
fn runtime_reload_error_rolls_back_action_outputs_and_lock() {
    let root = project("reload-rollback");
    let entry = root.join("main.jet");
    write(&entry, r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("rollback after runtime reload") {
    stamp :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf changed > stamp"], ["Exec", "FS"])?
    app :: b.add_executable("app", ["main.jet", "missing.jet"], [stamp])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#);
    assert!(compile_bundle_path_build(entry.to_str().unwrap(), opts()).is_err());
    assert!(!root.join("stamp").exists());
    assert!(!root.join(".jet/lock").exists());
}

#[test]
fn program_info_uses_qualified_collision_free_type_function_and_method_identities() {
    // Programmable-build / effect-facts frames exceed the default test thread
    // stack under full-suite parallelism. Same pattern as distinct-conversion
    // and LSP incremental diag parity (ddd6dca7f).
    let worker = std::thread::Builder::new()
        .name("build-entry-program-identities".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(run_program_info_uses_qualified_collision_free_type_function_and_method_identities)
        .expect("start build_entry program-identities worker");
    if let Err(payload) = worker.join() {
        std::panic::resume_unwind(payload);
    }
}

fn run_program_info_uses_qualified_collision_free_type_function_and_method_identities() {
    let root = project("program-identities");
    write(&root.join("left.jet"), "use core.net as net\npub enum Choice { A }\nfn helper() { net.tcp_connect(\"127.0.0.1:1\") ?? panic(\"net\") }\npub fn same() { helper(); panic(\"left\") }\npub fn answer() => Int { return 7 }\n");
    write(&root.join("right.jet"), "pub struct Choice { value: Int }\nimpl Choice { pub fn inspect(self) {} }\nfn helper() {}\npub fn same() { helper() }\n");
    let entry = root.join("main.jet");
    write(&entry, r#"
use "./left" as left
use "./right" as right
fn build(b: BuildContext) => BuildPlan ? {
    answer :: left.answer()
    if answer == 7 { b.error(b.program.functions()[0].span, "CALL", "qualified", "evaluator", "ok") }
    loop ty, b.program.types() {
        if ty.identity == "left::Choice" { b.error(ty.span, "ENUM", "enum", "identity", "ok") }
        loop method, ty.methods {
            if method.identity == "right::Choice.inspect" { b.error(ty.span, "METHOD", "method", "identity", "ok") }
        }
    }
    loop f, b.program.functions() {
        if f.name == "build" { b.error(f.span, "BUILDREFLECT", "build leaked into ProgramInfo", "build is an authoring hook, not runtime program surface", "keep build excluded from the read-only reflection snapshot") }
        if f.identity == "left::same" && f.effects.has("Net") && f.reaches_panic() { b.error(f.span, "LEFT", "left", "effect", "ok") }
        if f.identity == "right::same" && (f.effects.has("Net") || f.reaches_panic()) { b.error(f.span, "BAD", "collision", "effect", "fix") }
    }
    return b.plan()
}
fn run() { left.same(); right.same() }
"#);
    let (check_diags, _, facts) = jet::Driver::check_file_with_effect_facts(entry.to_str().unwrap(), None, false);
    assert!(!check_diags.iter().any(|diag| diag.severity == jet::Diagnostics::Severity::Error), "{check_diags:#?}");
    assert!(facts.solved.contains_key("left::same") && facts.solved.contains_key("right::same"));
    assert!(!facts.solved.contains_key("same"), "duplicate short aliases must not exist");
    assert!(facts.solved["left::same"].contains("Net"));
    assert!(!facts.solved["right::same"].contains("Net"));
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default()).unwrap_err();
    let codes = errors.iter().map(|diag| diag.code.as_str()).collect::<BTreeSet<_>>();
    assert!(codes.contains("CALL") && codes.contains("ENUM") && codes.contains("METHOD") && codes.contains("LEFT"), "{errors:#?}");
    assert!(!codes.contains("BAD"), "{errors:#?}");
}

#[test]
fn programmable_build_executes_destination_owned_distinct_conversion() {
    // Programmable-build / distinct-conversion frames exceed the default test
    // thread stack under full-suite parallelism. Match LSP incremental diag
    // parity (ddd6dca7f): isolate one worker with a larger stack.
    let worker = std::thread::Builder::new()
        .name("build-entry-distinct-conversion".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(run_programmable_build_executes_destination_owned_distinct_conversion)
        .expect("start build_entry distinct-conversion worker");
    if let Err(payload) = worker.join() {
        std::panic::resume_unwind(payload);
    }
}

fn run_programmable_build_executes_destination_owned_distinct_conversion() {
    let root = project("distinct-conversion");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
#Numeric BuildCode :: distinct U8

fn build(b: BuildContext) => BuildPlan ? {
    code :: BuildCode.from_int(7)?
    expected :: U8.from_int(7)?
    if code.raw() == expected {
        b.error(b.program.functions()[0].span, "DISTINCT", "converted", "executed", "ok")
    }
    return b.plan()
}

fn run() {}
"#,
    );

    let errors = compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default())
        .expect_err("build-time distinct conversion should reach the marker diagnostic");
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "DISTINCT"), "{errors:#?}");
}

#[test]
fn typed_toolchain_and_probe_flow_into_executed_action() {
    let root = project("toolchain-probe");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("probe selected toolchain") {
    tc :: b.toolchain("native", "x86_64-linux")?
    shell :: b.probe("shell", "find_program", "sh")?
    action :: b.action(
        "stamp",
        [],
        ["probe.txt"],
        ["sh", "-c", "printf probe > probe.txt"],
        ["Exec", "FS"],
        tc,
        [shell]
    )?
    app :: b.add_executable("app", ["main.jet"], [action])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );
    let output = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
    let build = output.build.unwrap();
    assert_eq!(build.probes.len(), 1);
    assert!(build.probes[0].success);
    assert_eq!(build.plan.toolchains().len(), 2);
    assert_eq!(build.plan.actions()[0].probes.len(), 1);
    assert_eq!(fs::read_to_string(root.join("probe.txt")).unwrap(), "probe");
}

#[cfg(unix)]
#[test]
fn sandbox_refuses_output_parent_symlink_escape() {
    use std::os::unix::fs::symlink;

    let root = project("symlink-output");
    let outside = project("symlink-outside");
    symlink(&outside, root.join("redirect")).unwrap();
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("exercise hostile output path") {
    action :: b.action(
        "escape",
        [],
        ["redirect/pwn"],
        ["sh", "-c", "mkdir -p redirect; printf escaped > redirect/pwn"],
        ["Exec", "FS"]
    )?
    app :: b.add_executable("app", ["main.jet"], [action])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap_err();
    assert!(errors.iter().any(|diag| diag.code == "E3505"));
    assert!(!outside.join("pwn").exists());
}

#[test]
fn root_build_can_emit_structured_program_diagnostics() {
    let root = project("program-diagnostic");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
struct Entity { id: Int }

fn build(b: BuildContext) => BuildPlan ? {
    types :: b.program.types()
    entity :: types[0]
    b.error(
        entity.span,
        "ORG01",
        "entity must define archive",
        "company policy requires archival",
        "add an archive method"
    )
    return b.plan()
}

fn run() {}
"#,
    );
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default())
        .unwrap_err();
    let diagnostic = errors.iter().find(|diag| diag.code == "ORG01").unwrap();
    assert_eq!(diagnostic.what, "entity must define archive");
    assert_eq!(diagnostic.why, "company policy requires archival");
    assert_eq!(diagnostic.fix, "add an archive method");
    assert!(diagnostic.span.is_some());
}

#[test]
fn locked_generated_drift_fails_before_materialization() {
    let root = project("locked-drift");
    let entry = root.join("main.jet");
    let source = |value: &str| format!(r#"
fn build(b: BuildContext) => BuildPlan ? {{
    b.generate("value", "fn generated_value() => String {{{{ return \"{value}\" }}}}")?
    app :: b.add_executable("app", ["main.jet", ".jet/generated/main/value.jet"], [])?
    return b.plan(app)
}}
fn run() {{ print(generated_value()) }}
"#);
    write(&entry, &source("one"));
    compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default()).unwrap();
    let generated = root.join(".jet/generated/main/value.jet");
    let before = fs::read_to_string(&generated).unwrap();
    write(&entry, &source("two"));
    let mut locked = BuildRunOptions::default();
    locked.locked = true;
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), locked).unwrap_err();
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E3512"));
    assert_eq!(fs::read_to_string(generated).unwrap(), before);
}

#[test]
fn generated_sources_stage_in_dependency_rounds_and_compile_as_one_program() {
    let root = project("generated-rounds");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) => BuildPlan ? {
    b.generate("consumer", "use \"provider\"\npub fn generated_value() => String {{ return provider.message() }}")?
    b.generate("provider", "pub fn message() => String {{ return \"round two\" }}")?
    app :: b.add_executable("app", ["main.jet", ".jet/generated/main/consumer.jet", ".jet/generated/main/provider.jet"], [])?
    return b.plan(app)
}
fn run() { print(generated_value()) }
"#,
    );
    let output = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
    let build = output.build.unwrap();
    assert_eq!(build.generated.len(), 2);
    assert!(root.join(".jet/generated/main/provider.jet").is_file());
    assert!(root.join(".jet/generated/main/consumer.jet").is_file());
    assert!(output.compile.rust.contains("generated_value"));
}

#[test]
fn generated_source_dependency_cycles_fail_before_any_file_is_written() {
    let root = project("generated-cycle");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) => BuildPlan ? {
    b.generate("alpha", "use \"beta\"\npub fn alpha() {{}}")?
    b.generate("beta", "use \"alpha\"\npub fn beta() {{}}")?
    app :: b.add_executable("app", ["main.jet", ".jet/generated/main/alpha.jet", ".jet/generated/main/beta.jet"], [])?
    return b.plan(app)
}
fn run() {}
"#,
    );
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap_err();
    assert!(errors.iter().any(|diagnostic| {
        diagnostic.code == "E3511"
            && diagnostic.fix.contains("alpha")
            && diagnostic.fix.contains("beta")
    }));
    assert!(!root.join(".jet/generated/main/alpha.jet").exists());
    assert!(!root.join(".jet/generated/main/beta.jet").exists());
}

#[test]
fn emit_generated_exports_the_exact_materialized_source() {
    let root = project("emit-generated");
    let entry = root.join("main.jet");
    let generated = "fn exported_generated() => String { return \"exported\" }";
    let generated_literal = generated
        .replace('"', "\\\"")
        .replace('{', "{{")
        .replace('}', "}}");
    write(
        &entry,
        &format!(
            r#"
fn build(b: BuildContext) => BuildPlan ? {{
    b.generate("exported", "{generated_literal}")?
    app :: b.add_executable("app", ["main.jet"], [])?
    return b.plan(app)
}}
fn run() {{ print(exported_generated()) }}
"#
        ),
    );
    jet::compile_programmable_build_emit_generated_opts(
        entry.to_str().unwrap(),
        &[],
        false,
        jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
        false,
        false,
        false,
        None,
    )
    .unwrap();
    // Keep the package segment from `.jet/generated/<package>/<name>.jet`
    // in the visible export tree.
    assert_eq!(
        fs::read_to_string(root.join("build/generated/main/exported.jet")).unwrap(),
        generated
    );
}

#[test]
fn package_grant_and_workspace_ceiling_resolve_before_execution() {
    let root = project("policy-chain");
    let entry = root.join("main.jet");
    write(
        &root.join("package.jet"),
        "name: \"policy-chain\"\nversion: \"0.1.0\"\nbuild: { allow: #(Exec) }\n",
    );
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec]=> BuildPlan ? {
    #Impure("policy chain test") {
    action :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf ok > stamp"], ["Exec"])?
    app :: b.add_executable("app", ["main.jet"], [action])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );
    jet::compile_programmable_build_opts(entry.to_str().unwrap(), &[], false, jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure), false, false, false, None).unwrap();
    assert_eq!(fs::read_to_string(root.join("stamp")).unwrap(), "ok");

    fs::remove_file(root.join("stamp")).unwrap();
    write(&root.join("workspace.jet"), "module workspace { policy: .{ deny: #(Exec) } }\n");
    let errors = jet::compile_programmable_build_opts(
        entry.to_str().unwrap(),
        &["exec".to_string()],
        false,
        jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
        false,
        false,
        false,
        None,
    ).unwrap_err();
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E3503"));
    assert!(!root.join("stamp").exists());
}

#[test]
fn workspace_subject_grant_authorizes_a_package_without_cli_flags() {
    let root = project("workspace-grant");
    let entry = root.join("run.jet");
    write(
        &root.join("package.jet"),
        "name: \"workspace-app\"\nversion: \"0.1.0\"\n",
    );
    write(
        &root.join("authority.jet"),
        "module workspace { policy: .{ grants: .{ \"workspace-app\": #(Exec) } } }\n",
    );
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec]=> BuildPlan ? {
    #Impure("workspace grant test") {
        action :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf workspace > stamp"], ["Exec"])?
        app :: b.add_executable("app", ["run.jet"], [action])?
        return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );
    jet::compile_programmable_build_opts(
        entry.to_str().unwrap(),
        &[],
        false,
        jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
        false,
        false,
        false,
        None,
    )
    .unwrap();
    assert_eq!(fs::read_to_string(root.join("stamp")).unwrap(), "workspace");
}

#[test]
fn malformed_package_and_workspace_build_policy_fail_closed() {
    let root = project("malformed-policy");
    let entry = root.join("run.jet");
    write(&entry, r#"
fn build(b: BuildContext) =[Exec]=> BuildPlan ? {
    #Impure("must never run under malformed policy") {
    action :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf bad > stamp"], ["Exec"])?
    app :: b.add_executable("app", ["run.jet"], [action])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#);
    write(&root.join("package.jet"), "name: \"bad\"\nversion: \"0.1.0\"\nbuild: { allow: Exec }\n");
    let errors = jet::compile_programmable_build_opts(entry.to_str().unwrap(), &[], false, jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure), false, false, false, None).unwrap_err();
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E1221"), "{errors:#?}");
    assert!(!root.join("stamp").exists());

    write(&root.join("package.jet"), "name: \"bad\"\nversion: \"0.1.0\"\nbuild: { allow: #(Exec) }\n");
    write(&root.join("workspace.jet"), "module workspace { policy: .{ deny: Exec } }\n");
    let errors = jet::compile_programmable_build_opts(entry.to_str().unwrap(), &[], false, jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure), false, false, false, None).unwrap_err();
    assert!(errors.iter().any(|diagnostic| {
        diagnostic.code == "E3503"
            && diagnostic.what == "This root build asks for authority missing from its declaration, `#Impure` gate, or effective policy."
            && diagnostic.why == "Build authority must pass all three independent checks before any probe or action executes."
            && diagnostic.fix == "Declare the effect, gate the ambient operation with `#Impure(\"reason\")`, and grant the effect through CLI/package/workspace policy."
    }), "{errors:#?}");
    assert!(!root.join("stamp").exists());

    fs::remove_file(root.join("workspace.jet")).unwrap();
    fs::create_dir(root.join("workspace.jet")).unwrap();
    let errors = jet::compile_programmable_build_opts(entry.to_str().unwrap(), &[], false, jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure), false, false, false, None).unwrap_err();
    assert!(errors.iter().any(|diagnostic| {
        diagnostic.code == "E3503" && diagnostic.why.contains("present but unavailable")
    }), "{errors:#?}");
}

#[test]
fn unsupported_workspace_policy_allow_is_e3503() {
    let root = project("unsupported-workspace-policy");
    let entry = root.join("run.jet");
    write(
        &root.join("authority.jet"),
        "module workspace { policy: .{ allow: #(Exec) } }\n",
    );
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec]=> BuildPlan ? {
    #Impure("unsupported workspace policy must block build") {
        action :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf bad > stamp"], ["Exec"])?
        app :: b.add_executable("app", ["run.jet"], [action])?
        return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );

    let errors = jet::compile_programmable_build_opts(
        entry.to_str().unwrap(),
        &[],
        false,
        jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
        false,
        false,
        false,
        None,
    )
    .unwrap_err();
    assert!(errors.iter().any(|diagnostic| {
        diagnostic.code == "E3503" && diagnostic.why.contains("policy.allow")
    }), "{errors:#?}");
    assert!(!root.join("stamp").exists());
}

#[test]
fn nested_workspace_is_the_module_import_root() {
    let root = project("nested-workspace-import-root");
    let child = root.join("child");
    fs::create_dir_all(&child).unwrap();
    write(
        &root.join("package.jet"),
        "name: \"outer\"\nversion: \"0.1.0\"\n",
    );
    write(
        &root.join("outside.jet"),
        "module _outside { pub fn fixture() {} }\n",
    );
    write(
        &child.join("boundary.jet"),
        "module workspace { policy: .{ deny: #(FS) } }\n",
    );
    let entry = child.join("run.jet");
    write(
        &entry,
        "use project._outside as outside\nfn run() { outside.fixture() }\n",
    );

    let errors = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap_err();
    let diagnostic = errors
        .iter()
        .find(|diagnostic| diagnostic.code == "E0603")
        .expect("project-local import must fail at the nested workspace root");
    assert_eq!(diagnostic.what, "can't find a project module named `_outside`");
    assert_eq!(
        diagnostic.why,
        "project-local imports resolve declared module names, not filenames"
    );
    assert_eq!(
        diagnostic.fix,
        "declare `module _outside { ... }` under this project"
    );
}

#[test]
fn module_directory_entry_requires_run_jet_not_main_jet() {
    let root = project("module-run-entry");
    let module = root.join("tool");
    fs::create_dir_all(&module).unwrap();
    write(&module.join("main.jet"), "pub fn run() {}\n");
    let entry = root.join("run.jet");
    write(&entry, "use tool\nfn run() {}\n");

    let errors = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap_err();
    let diagnostic = errors
        .iter()
        .find(|diagnostic| diagnostic.code == "E0603")
        .expect("retired main.jet must not resolve a module directory");
    assert_eq!(diagnostic.what, "can't find a module named `tool`");
    assert_eq!(
        diagnostic.why,
        "search from the project root for `tool.jet`, or `tool/tool/tool.jet` / `run.jet`"
    );
    assert_eq!(diagnostic.fix, "add `tool.jet` under this project, or fix the `use` name");
    assert!(diagnostic.why.contains("run.jet"));
    assert!(!diagnostic.why.contains("main.jet"));
}

#[test]
fn outer_package_grant_cannot_override_inner_workspace_deny() {
    let root = project("policy-precedence");
    let child = root.join("child");
    fs::create_dir_all(&child).unwrap();
    write(&root.join("package.jet"), "name: \"parent\"\nversion: \"0.1.0\"\nbuild: { allow: #(Exec) }\n");
    write(&child.join("boundary.jet"), "module workspace { policy_note: .{ deny: #(FS) }, policy: .{ trust: .{ nested: .{ deny: #(FS) } }, deny: #(Exec) } }\n");
    let entry = child.join("run.jet");
    write(&entry, r#"
fn build(b: BuildContext) =[Exec]=> BuildPlan ? {
    #Impure("workspace ceiling wins last") {
    action :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf bad > stamp"], ["Exec"])?
    app :: b.add_executable("app", ["run.jet"], [action])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#);
    let errors = jet::compile_programmable_build_opts(entry.to_str().unwrap(), &[], false, jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure), false, false, false, None).unwrap_err();
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E3503"), "{errors:#?}");
    assert!(!child.join("stamp").exists());
}

#[test]
fn outer_workspace_grant_does_not_cross_inner_workspace_boundary() {
    let root = project("workspace-grant-boundary");
    let child = root.join("child");
    fs::create_dir_all(&child).unwrap();
    write(
        &root.join("outer-authority.jet"),
        "module workspace { policy: .{ grants: .{ \"run\": #(Exec) } } }\n",
    );
    write(
        &child.join("inner-authority.jet"),
        "module workspace { policy: .{ deny: #(FS) } }\n",
    );
    let entry = child.join("run.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec]=> BuildPlan ? {
    #Impure("outer workspace grant must not apply") {
    action :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf bad > stamp"], ["Exec"])?
    app :: b.add_executable("app", ["run.jet"], [action])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );

    let errors = jet::compile_programmable_build_opts(
        entry.to_str().unwrap(),
        &[],
        false,
        jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
        false,
        false,
        false,
        None,
    )
    .unwrap_err();
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E3503"), "{errors:#?}");
    assert!(!child.join("stamp").exists());
}

#[test]
fn inner_workspace_grant_overrides_outer_workspace_deny_from_canonical_run() {
    let root = project("workspace-grant-precedence");
    let child = root.join("child");
    fs::create_dir_all(&child).unwrap();
    write(
        &root.join("outer-authority.jet"),
        "module workspace { policy: .{ deny: #(Exec) } }\n",
    );
    write(
        &child.join("package.jet"),
        "name: \"run\"\nversion: \"0.1.0\"\n",
    );
    write(
        &child.join("authority.jet"),
        "module workspace { policy: .{ grants: .{ \"run\": #(Exec) } } }\n",
    );
    let entry = child.join("run.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[Exec]=> BuildPlan ? {
    #Impure("inner workspace grant must apply") {
        action :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf inner > stamp"], ["Exec"])?
        app :: b.add_executable("app", ["run.jet"], [action])?
        return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );

    jet::compile_programmable_build_opts(
        entry.to_str().unwrap(),
        &[],
        false,
        jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
        false,
        false,
        false,
        None,
    )
    .expect("inner workspace grant should be the only active workspace policy");
    assert_eq!(fs::read_to_string(child.join("stamp")).unwrap(), "inner");
}

#[test]
fn programmable_staging_preserves_web_cross_freestanding_and_plugin_modes() {
    let root = project("target-modes");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) => BuildPlan ? {
    app :: b.add_executable("app", ["main.jet"], [])?
    return b.plan(app)
}
fn run() {}
"#,
    );
    let mut web = BuildRunOptions::default();
    web.web_target = true;
    assert!(compile_bundle_path_build(entry.to_str().unwrap(), web).unwrap().compile.web.is_some());

    let mut cross = BuildRunOptions::default();
    cross.cross_target = Some("x86_64-unknown-linux-gnu".to_string());
    assert!(compile_bundle_path_build(entry.to_str().unwrap(), cross).unwrap().compile.rust.contains("fn main"));

    let mut freestanding = BuildRunOptions::default();
    freestanding.freestanding = true;
    let freestanding_result = compile_bundle_path_build(entry.to_str().unwrap(), freestanding);
    assert!(freestanding_result.is_ok(), "{:#?}", freestanding_result.err());

    write(
        &entry,
        r#"
fn build(b: BuildContext) => BuildPlan ? {
    plugin :: b.add_library("plugin", ["main.jet"], [])?
    return b.plan(plugin)
}
pub fn transform(value: Int) => Int { return value + 1 }
"#,
    );
    let mut plugin = BuildRunOptions::default();
    plugin.plugin_target = true;
    assert!(compile_bundle_path_build(entry.to_str().unwrap(), plugin).unwrap().compile.plugin.is_some());
}

#[test]
fn locked_action_output_drift_rolls_back_filesystem() {
    let root = project("locked-action-output");
    let entry = root.join("main.jet");
    let source = |value: &str| format!(r#"
fn build(b: BuildContext) =[Exec]=> BuildPlan ? {{
    #Impure("locked action output") {{
    action :: b.action("emit", [], ["artifact"], ["sh", "-c", "printf {value} > artifact"], ["Exec"])?
    app :: b.add_executable("app", ["main.jet"], [action])?
    return b.plan(app)
    }}
    return b.plan()
}}
fn run() {{}}
"#);
    write(&entry, &source("one"));
    compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
    assert_eq!(fs::read_to_string(root.join("artifact")).unwrap(), "one");
    write(&entry, &source("two"));
    let mut locked = opts();
    locked.locked = true;
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), locked).unwrap_err();
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E3512"));
    assert_eq!(fs::read_to_string(root.join("artifact")).unwrap(), "one");
}

#[test]
fn build_context_find_and_embed_are_locked_tier_one_inputs() {
    let root = project("find-embed");
    fs::create_dir_all(root.join("assets")).unwrap();
    write(&root.join("assets/message.txt"), "hello");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) =[FS]=> BuildPlan ? {
    files :: b.find("assets/*.txt")
    message :: b.embed(files[0])
    b.generate("asset", "fn generated_asset() => String {{ return \"hello\" }}")?
    app :: b.add_executable("app", ["main.jet", ".jet/generated/main/asset.jet"], [])?
    return b.plan(app)
}
fn run() { print(generated_asset()) }
"#,
    );
    let output = compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default()).unwrap();
    assert!(output.compile.comptime_inputs.iter().any(|input| input.path == "assets/message.txt"));
    let lock = fs::read_to_string(root.join(".jet/lock")).unwrap();
    assert!(lock.contains("assets/message.txt"));
}

#[test]
fn build_context_fetch_uses_the_locked_tier_one_host_surface() {
    let root = project("build-context-fetch");
    let input = root.join("input.txt");
    write(&input, "hello");
    let entry = root.join("main.jet");
    write(
        &entry,
        &format!(
            r#"
fn build(b: BuildContext) => BuildPlan ? {{
    content :: b.fetch("file://{}", "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")?
    if content != "hello" {{ panic("unexpected fetch content") }}
    app :: b.add_executable("app", ["main.jet"], [])?
    return b.plan(app)
}}
fn run() {{}}
"#,
            input.display()
        ),
    );

    let output =
        compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default()).unwrap();
    let input_key = format!("url:file://{}", input.display());
    assert!(
        output
            .compile
            .comptime_inputs
            .iter()
            .any(|locked| locked.path == input_key
                && locked.hash
                    == "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"),
        "{:#?}",
        output.compile.comptime_inputs
    );
    let lock = fs::read_to_string(root.join(".jet/lock")).unwrap();
    assert!(lock.contains(&input_key), "{lock}");
}

#[test]
fn pure_core_call_inside_impure_does_not_require_gates() {
    // Programmable-build frames exceed the default test thread stack under
    // full-suite parallelism. Same pattern as distinct-conversion /
    // program-identities (ddd6dca7f).
    let worker = std::thread::Builder::new()
        .name("build-entry-pure-inside-impure".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(run_pure_core_call_inside_impure_does_not_require_gates)
        .expect("start build_entry pure-inside-impure worker");
    if let Err(payload) = worker.join() {
        std::panic::resume_unwind(payload);
    }
}

fn run_pure_core_call_inside_impure_does_not_require_gates() {
    let root = project("pure-inside-impure");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
use core.math as math
fn build(b: BuildContext) => BuildPlan ? {
    #Impure("scope contains no ambient effect") {
        value :: math.abs((-5))
        if value == 5 {
            b.error(b.program.functions()[0].span, "PURE", "pure", "executed", "ok")
        }
    }
    return b.plan()
}
fn run() {}
"#,
    );

    let errors = compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default())
        .expect_err("pure Core call should execute without --gate impure=allow");
    assert!(
        errors.iter().any(|diagnostic| diagnostic.code == "PURE"),
        "{errors:#?}"
    );
    assert!(!errors.iter().any(|diagnostic| diagnostic.code == "E3411"));
}

#[test]
fn vault_is_denied_unconditionally_inside_impure_build_context() {
    let root = project("vault-inside-impure");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
use core.vault as vault
fn build(b: BuildContext) =[Secret]=> BuildPlan ? {
    #Impure("must not grant secret access") {
        secret :: vault.get("db_password")
    }
    return b.plan()
}
fn run() {}
"#,
    );

    let errors = compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default())
        .expect_err("vault access must remain denied without an impurity escape hatch");
    assert!(
        errors.iter().any(|diagnostic| diagnostic.code == "E1265"),
        "{errors:#?}"
    );
    assert!(!errors.iter().any(|diagnostic| diagnostic.code == "E3411"));
}
