//! D-BUILDENTRY1/D-BUILDACTION1: real Jet `fn build` vertical.

use jet::Comptime::Build::{ActionOutcome, BuildCapability};
use jet::Driver::{BuildRunOptions, compile_bundle_path_build};
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
    }
}

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
}

#[test]
fn root_fn_build_executes_graph_materializes_and_frontend_checks_generated_source() {
    let root = project("vertical");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) #(Exec, Fs) -> BuildPlan ? {
    b.generate("generated_message", "fn generated_message() -> String {{ return \"built\" }}")?
    stamp :: b.action(
        "stamp",
        [],
        [".jet/generated/app/stamp.txt"],
        ["sh", "-c", "printf stamped > .jet/generated/app/stamp.txt"],
        ["Exec", "Fs"]
    )?
    app :: b.add_executable("app", ["main.jet"], [stamp])?
    return b.plan(app)
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

    let second = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap();
    let build = second.build.unwrap();
    assert!(build.execution.events.iter().any(|event| matches!(
        event,
        jet::Comptime::Build::BuildExecutionEvent::Finished {
            outcome: ActionOutcome::RestoredFromCache,
            ..
        }
    )));
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
    app :: b.add_executable("app", ["main.jet"], [])?
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
fn build(b: BuildContext) #(Exec, Fs) -> BuildPlan ? {
    action :: b.action("escape", [], ["out"], ["sh", "-c", "printf bad > out"], ["Exec"])?
    app :: b.add_executable("app", ["main.jet"], [action])?
    return b.plan(app)
}
fn run() {}
"#,
    );
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), BuildRunOptions::default())
        .unwrap_err();
    assert!(errors.iter().any(|d| d.code == "E3504"));
    assert!(!root.join("out").exists());
}

#[test]
fn action_generated_jet_reenters_frontend_before_runtime_codegen() {
    let root = project("action-generated");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) #(Exec, Fs) -> BuildPlan ? {
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
fn run() {}
"#,
    );
    let errors = compile_bundle_path_build(entry.to_str().unwrap(), opts()).unwrap_err();
    assert!(errors.iter().any(|diag| diag.what.contains("generated action `bad-gen`")));
}

#[test]
fn jet_build_command_runs_selected_build_entry() {
    let root = project("cli");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) #(Exec, Fs) -> BuildPlan ? {
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
        .arg("graph")
        .arg(&entry)
        .arg("--json")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let json = String::from_utf8_lossy(&output.stdout);
    assert!(json.contains("\"name\":\"app\""));
    assert!(json.contains("\"name\":\"never-run\""));
    assert!(!root.join("out").exists(), "graph query must never execute actions");

    write(&entry, "fn build() -> Int { return 1 }\nfn run() {}\n");
    let (diags, _) = jet::Driver::check_file(entry.to_str().unwrap(), None, true);
    assert!(diags.iter().any(|diag| diag.code == "E3501"));
}

#[test]
fn typed_toolchain_and_probe_flow_into_executed_action() {
    let root = project("toolchain-probe");
    let entry = root.join("main.jet");
    write(
        &entry,
        r#"
fn build(b: BuildContext) #(Exec, Fs) -> BuildPlan ? {
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
fn build(b: BuildContext) #(Exec, Fs) -> BuildPlan ? {
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
