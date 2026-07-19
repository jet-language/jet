//! D-BUILDENTRY1/D-BUILDACTION1: real Jet `fn build` vertical.

use jet::Comptime::Build::{ActionOutcome, BuildCapability, CacheHitReason};
use jet::Driver::{BuildQueryExpression, BuildRunOptions, compile_bundle_path_build};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

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
        grants: BTreeSet::from([BuildCapability::Exec, BuildCapability::Fs]),
        execute: true,
        allow_impure: true,
        inspect_only: false,
        locked: false,
        freestanding: false,
        web_target: false,
        plugin_target: false,
        cross_target: None,
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

#[test]
fn root_fn_build_executes_graph_materializes_and_frontend_checks_generated_source() {
    let root = project("vertical");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) --[Exec, Fs]-> BuildPlan ? {
    b.generate("generated_message", "fn generated_message() -> String {{ return \"built\" }}")?
    @Impure("write declared build output") {
    stamp :: b.action(
        "stamp",
        [],
        [".jet/generated/app/stamp.txt"],
        ["sh", "-c", "printf stamped > .jet/generated/app/stamp.txt"],
        ["Exec", "Fs"]
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
fn failed_action_replaces_stale_rebuild_provenance() {
    let root = project("failed-provenance");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) --[Exec]-> BuildPlan ? {
    @Impure("run declared failing action") {
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
fn build(b: BuildContext) --[Exec, Fs]-> BuildPlan ? {
    @Impure("write declared cached output") {
        emit :: b.action("emit", [], ["artifact"], ["sh", "-c", "printf fresh > artifact"], ["Exec", "Fs"])?
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
fn build(b: BuildContext) -> BuildPlan ? {
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
fn build(b: BuildContext) -> BuildPlan ? {
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

    write(&entry, "fn build() -> Int { return 1 }\nfn run() {}\n");
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
fn build(b: BuildContext) --[Exec, Fs]-> BuildPlan ? {
    @Impure("exercise denied authority") {
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
fn build(b: BuildContext) --[Exec, Fs]-> BuildPlan ? {
    @Impure("generate declared source") {
    action :: b.action(
        "bad-gen",
        [],
        [".jet/generated/main/bad.jet"],
        ["sh", "-c", "printf 'fn nope(' > .jet/generated/main/bad.jet"],
        ["Exec", "Fs"]
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
fn build(b: BuildContext) --[Exec, Fs]-> BuildPlan ? {
    @Impure("write CLI test output") {
    action :: b.action(
        "stamp",
        [],
        ["stamp.txt"],
        ["sh", "-c", "printf cli-built > stamp.txt"],
        ["Exec", "Fs"]
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
fn graph_query_is_static_json_and_lsp_check_sees_bad_signature() {
    let root = project("query");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) -> BuildPlan ? {
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

    write(&entry, "fn build() -> Int { return 1 }\nfn run() {}\n");
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
fn build(b: BuildContext) --[Exec, Fs]-> BuildPlan ? {
    @Impure("declare an inspectable action") {
        action :: b.action("never-run", [], ["out"], ["sh", "-c", "exit 91"], ["Exec", "Fs"])?
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

fn build(b: BuildContext) -> BuildPlan ? {{
    @Impure("hostile inspection probe") {{
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
            "effect :: api.write(\"blocked\", \"owned\")",
        ),
        (
            "environment",
            "core.env",
            "api.set(\"JET_QUERY_BLOCKED\", \"owned\")",
        ),
        (
            "exec-process",
            "core.process",
            "effect :: api.run([\"sh\", \"-c\", \"exit 97\"])",
        ),
        (
            "network",
            "core.net",
            "effect :: api.tcp_listen(\"127.0.0.1:0\")",
        ),
    ];
    for (name, module, call) in cases {
        let root = project(&format!("query-denies-{name}"));
        let entry = root.join("main.jet");
        write(
            &entry,
            &format!(
                "use {module} as api\nfn build(b: BuildContext) -> BuildPlan ? {{\n    @Impure(\"hostile {name} probe\") {{ {call} }}\n    return b.plan()\n}}\nfn run() {{}}\n"
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
    write(&entry, "fn build(b: BuildContext) -> BuildPlan ? { app :: b.add_executable(\"disk\", [\"main.jet\"], [])?\n return b.plan(app) }\nfn run() {}\n");
    let unsaved = "fn build(b: BuildContext) -> BuildPlan ? { app :: b.add_executable(\"unsaved\", [\"main.jet\"], [])?\n return b.plan(app) }\nfn run() {}\n";
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
fn build(b: BuildContext) --[Exec, Fs]-> BuildPlan ? {
    @Impure("declare selected closure") {
    bad :: b.action("unselected", [], ["missing-generated.jet"], ["sh", "-c", "exit 77"], ["Exec", "Fs"])?
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
fn build(b: BuildContext) -> BuildPlan ? {
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
fn build(b: BuildContext) --[Exec, Fs]-> BuildPlan ? {
    @Impure("rollback after runtime reload") {
    stamp :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf changed > stamp"], ["Exec", "Fs"])?
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
    let root = project("program-identities");
    write(&root.join("left.jet"), "pub enum Choice { A }\nfn helper() --[Net]-> {}\npub fn same() { helper(); panic(\"left\") }\npub fn answer() -> Int { return 7 }\n");
    write(&root.join("right.jet"), "pub struct Choice { value: Int }\nimpl Choice { pub fn inspect(self) {} }\nfn helper() {}\npub fn same() { helper() }\n");
    let entry = root.join("main.jet");
    write(&entry, r#"
use "./left" as left
use "./right" as right
fn build(b: BuildContext) -> BuildPlan ? {
    answer :: left.answer()
    if answer == 7 { b.error(b.program.functions()[0].span, "CALL", "qualified", "evaluator", "ok") }
    loop ty; b.program.types() {
        if ty.identity == "left::Choice" { b.error(ty.span, "ENUM", "enum", "identity", "ok") }
        loop method; ty.methods {
            if method.identity == "right::Choice.inspect" { b.error(ty.span, "METHOD", "method", "identity", "ok") }
        }
    }
    loop f; b.program.functions() {
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
    let root = project("distinct-conversion");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
@Numeric BuildCode :: distinct U8

fn build(b: BuildContext) -> BuildPlan ? {
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
fn build(b: BuildContext) --[Exec, Fs]-> BuildPlan ? {
    @Impure("probe selected toolchain") {
    tc :: b.toolchain("native", "x86_64-linux")?
    shell :: b.probe("shell", "find_program", "sh")?
    action :: b.action(
        "stamp",
        [],
        ["probe.txt"],
        ["sh", "-c", "printf probe > probe.txt"],
        ["Exec", "Fs"],
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
fn build(b: BuildContext) --[Exec, Fs]-> BuildPlan ? {
    @Impure("exercise hostile output path") {
    action :: b.action(
        "escape",
        [],
        ["redirect/pwn"],
        ["sh", "-c", "mkdir -p redirect; printf escaped > redirect/pwn"],
        ["Exec", "Fs"]
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

fn build(b: BuildContext) -> BuildPlan ? {
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
fn build(b: BuildContext) -> BuildPlan ? {{
    b.generate("value", "fn generated_value() -> String {{{{ return \"{value}\" }}}}")?
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
fn package_grant_and_workspace_ceiling_resolve_before_execution() {
    let root = project("policy-chain");
    let entry = root.join("main.jet");
    write(
        &root.join("pkg.jet"),
        "payload: { name: \"policy-chain\", version: \"0.1.0\" }\nbuild: { allow: #(Exec) }\n",
    );
    write(
        &entry,
        r#"
fn build(b: BuildContext) --[Exec]-> BuildPlan ? {
    @Impure("policy chain test") {
    action :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf ok > stamp"], ["Exec"])?
    app :: b.add_executable("app", ["main.jet"], [action])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#,
    );
    jet::compile_programmable_build_opts(entry.to_str().unwrap(), &[], false, true, false, false, false, None).unwrap();
    assert_eq!(fs::read_to_string(root.join("stamp")).unwrap(), "ok");

    fs::remove_file(root.join("stamp")).unwrap();
    write(&root.join("workspace.jet"), "module workspace { policy: .{ deny: #(Exec) } }\n");
    let errors = jet::compile_programmable_build_opts(
        entry.to_str().unwrap(),
        &["exec".to_string()],
        false,
        true,
        false,
        false,
        false,
        None,
    ).unwrap_err();
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E3503"));
    assert!(!root.join("stamp").exists());
}

#[test]
fn malformed_package_and_workspace_build_policy_fail_closed() {
    let root = project("malformed-policy");
    let entry = root.join("main.jet");
    write(&entry, r#"
fn build(b: BuildContext) --[Exec]-> BuildPlan ? {
    @Impure("must never run under malformed policy") {
    action :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf bad > stamp"], ["Exec"])?
    app :: b.add_executable("app", ["main.jet"], [action])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#);
    write(&root.join("pkg.jet"), "payload: { name: \"bad\", version: \"0.1.0\" }\nbuild: { allow: Exec }\n");
    let errors = jet::compile_programmable_build_opts(entry.to_str().unwrap(), &[], false, true, false, false, false, None).unwrap_err();
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E1221"), "{errors:#?}");
    assert!(!root.join("stamp").exists());

    write(&root.join("pkg.jet"), "payload: { name: \"bad\", version: \"0.1.0\" }\nbuild: { allow: #(Exec) }\n");
    write(&root.join("workspace.jet"), "module workspace { policy: .{ trust: .{ note: \"nested } text\" }, deny: #(Exec) }\n");
    let errors = jet::compile_programmable_build_opts(entry.to_str().unwrap(), &[], false, true, false, false, false, None).unwrap_err();
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E3503" && diagnostic.what.contains("malformed")), "{errors:#?}");
    assert!(!root.join("stamp").exists());
}

#[test]
fn outer_package_grant_cannot_override_inner_workspace_deny() {
    let root = project("policy-precedence");
    let child = root.join("child");
    fs::create_dir_all(&child).unwrap();
    write(&root.join("pkg.jet"), "payload: { name: \"parent\", version: \"0.1.0\" }\nbuild: { allow: #(Exec) }\n");
    write(&child.join("workspace.jet"), "module workspace { policy_note: .{ deny: #(Fs) }, policy: .{ trust: .{ nested: .{ deny: #(Fs) } }, deny: #(Exec) } }\n");
    let entry = child.join("main.jet");
    write(&entry, r#"
fn build(b: BuildContext) --[Exec]-> BuildPlan ? {
    @Impure("workspace ceiling wins last") {
    action :: b.action("stamp", [], ["stamp"], ["sh", "-c", "printf bad > stamp"], ["Exec"])?
    app :: b.add_executable("app", ["main.jet"], [action])?
    return b.plan(app)
    }
    return b.plan()
}
fn run() {}
"#);
    let errors = jet::compile_programmable_build_opts(entry.to_str().unwrap(), &[], false, true, false, false, false, None).unwrap_err();
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E3503"), "{errors:#?}");
    assert!(!child.join("stamp").exists());
}

#[test]
fn programmable_staging_preserves_web_cross_freestanding_and_plugin_modes() {
    let root = project("target-modes");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) -> BuildPlan ? {
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
fn build(b: BuildContext) -> BuildPlan ? {
    plugin :: b.add_library("plugin", ["main.jet"], [])?
    return b.plan(plugin)
}
pub fn transform(value: Int) -> Int { return value + 1 }
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
fn build(b: BuildContext) --[Exec]-> BuildPlan ? {{
    @Impure("locked action output") {{
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
fn build(b: BuildContext) -> BuildPlan ? {
    files :: b.find("assets/*.txt")
    message :: b.embed(files[0])
    b.generate("asset", "fn generated_asset() -> String {{ return \"hello\" }}")?
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
