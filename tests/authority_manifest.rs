use std::path::{Path, PathBuf};
use std::process::Command;

const AUTHORITY_PACKAGE: &str = r#"
name: "authority_tiers"
version: "0.1.0"
authority: .{
    holds: { allow: [IO], deny: [Exec] },
    grants: { "image-codec": [FS.Read] },
    trust: { default: prompt, ci: { prompt: deny }, services: { stripe: allow } },
    providers: { nix: { registry: "nixpkgs", deny: ["openssl-1.0"] } },
}
"#;

const AUTHORITY_PROGRAM: &str = r#"
@answer :: "authority"

fn run() {
    print(@answer)
}
"#;

fn authority_project(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "jet_authority_manifest_{label}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("authority tier project");
    std::fs::write(root.join("package.jet"), AUTHORITY_PACKAGE).expect("authority package");
    std::fs::write(root.join("run.jet"), AUTHORITY_PROGRAM).expect("authority program");
    root
}

fn run_authority_cli(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .expect("authority tier command")
}

fn assert_authority_output(output: &std::process::Output, tier: &str) {
    assert!(
        output.status.success(),
        "{tier} rejected the authority project:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "authority\n", "{tier}");
}

#[test]
fn malformed_authority_fields_share_e1221() {
    let cases = [
        (
            "holds",
            "authority: .{ holds: { allow: [NotAnAuthority] } }",
        ),
        (
            "grants",
            "authority: .{ grants: { \"dep\": [NotAnAuthority] } }",
        ),
        (
            "trust",
            "authority: .{ trust: { default: maybe } }",
        ),
        (
            "providers",
            "authority: .{ providers: { nix: { deny: [\"openssl-1.0\"] } } }",
        ),
    ];

    for (field, block) in cases {
        let source = format!("name: \"demo\"\nversion: \"0.1.0\"\n{block}\n");
        let diagnostic = jet::Manifest::parse(Path::new("package.jet"), &source)
            .unwrap_err();
        assert_eq!(diagnostic.code, "E1221", "{field}: {diagnostic:?}");
        assert!(diagnostic.what.contains("authority"), "{field}: {diagnostic:?}");
        assert!(diagnostic.fix.contains("authority:"), "{field}: {diagnostic:?}");
    }
}

#[test]
fn retired_authority_fields_name_the_new_block() {
    let cases = [
        "grants: .{ \"dep\": [Net] }",
        "policy: .{ trust: { default: prompt } }",
        "policy: .{ providers: { nix: { registry: \"nixpkgs\" } } }",
    ];

    for block in cases {
        let source = format!("name: \"demo\"\nversion: \"0.1.0\"\n{block}\n");
        let diagnostic = jet::Manifest::parse(Path::new("package.jet"), &source)
            .expect_err("retired authority field must fail");
        assert_eq!(diagnostic.code, "E1206", "{block}: {diagnostic:?}");
        assert!(diagnostic.why.contains("authority:"), "{block}: {diagnostic:?}");
        assert!(diagnostic.fix.contains("authority:"), "{block}: {diagnostic:?}");
    }
}

#[test]
fn policy_keeps_unsafe_mode_and_package_floors() {
    let source = r#"
name: "demo"
version: "0.1.0"
policy: .{ unsafe: .Forbid, explicit_units: true, copies: .Explicit, sentries: .On }
authority: .{ holds: { deny: [Mem.Alloc] } }
"#;
    let facts = jet::Package::PackageFacts::parse(source, "package.jet")
        .expect("policy floors and unsafe mode stay in policy");
    assert_eq!(facts.policy.declarations.len(), 4);
    assert_eq!(facts.authority.holds.deny, Some(vec!["Mem.Alloc".to_string()]));
    assert!(facts.authority.trust.is_none());
    assert!(facts.authority.providers.is_empty());
}

#[test]
fn retired_effect_budget_names_authority_holds() {
    let source = "name: \"demo\"\nversion: \"0.1.0\"\neffects: .{ deny: [Net] }\n";
    let diagnostic = jet::Manifest::parse(std::path::Path::new("package.jet"), source)
        .expect_err("retired effects key must fail closed");
    assert_eq!(diagnostic.code, "E1206");
    assert!(diagnostic.why.contains("authority.holds"), "{diagnostic:?}");
    assert!(diagnostic.fix.contains("authority.holds"), "{diagnostic:?}");
}

#[test]
fn i9_parser_reads_one_authority_block() {
    let facts = jet::Package::PackageFacts::parse(AUTHORITY_PACKAGE, "package.jet")
        .expect("parser accepts the one authority block");
    assert_eq!(facts.authority.holds.allow, Some(vec!["IO".to_string()]));
    assert_eq!(facts.authority.grants.len(), 1);
    assert!(facts.authority.trust.is_some());
    assert_eq!(facts.authority.providers.len(), 1);
}

#[test]
fn i9_sema_consumes_authority_holds() {
    let root = authority_project("sema");
    let entry = root.join("run.jet");
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).expect("load authority project");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "sema changed authority meaning: {diagnostics:#?}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn i9_tir_receives_the_authority_project() {
    let root = authority_project("tir");
    let entry = root.join("run.jet");
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap())
        .expect("TIR front end accepts authority project");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "TIR sema changed authority meaning: {diagnostics:#?}");
    let program = jet::Codegen::TIR::lower_jit_program(&bundle)
        .expect("authority project lowers through TIR");
    let mut sink = jet::Comptime::DevSink::default();
    jet::Codegen::TIR::run_named_func(&program, "run", Vec::new(), &mut sink)
        .expect("authority project runs through TIR");
    assert_eq!(sink.stdout, "authority\n");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn i9_interpreter_runs_the_authority_project() {
    let root = authority_project("interpreter");
    let entry = root.join("run.jet");
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap())
        .expect("interpreter front end accepts authority project");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "interpreter sema changed authority meaning: {diagnostics:#?}");

    match jet::Interpreter::run_checked(&bundle, true) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "interpreter stderr: {stderr}");
            assert_eq!(stdout, "authority\n");
        }
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("interpreter rejected the authority project: {diagnostics:?}")
        }
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn i9_aot_runs_the_authority_project() {
    let root = authority_project("aot");
    let output = run_authority_cli(&root, &["run", "--release", "run.jet"]);
    assert_authority_output(&output, "AOT");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn i9_jit_runs_the_authority_project() {
    let root = authority_project("jit");
    assert_authority_output(&run_authority_cli(&root, &["run", "run.jet"]), "JIT");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn i9_dev_runs_the_authority_project() {
    let root = authority_project("dev");
    assert_authority_output(
        &run_authority_cli(&root, &["dev", "run.jet", "--watch=off"]),
        "dev",
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn i9_comptime_keeps_the_authority_project_meaning() {
    let root = authority_project("comptime");
    let entry = root.join("run.jet");
    let output = jet::compile_with_path(AUTHORITY_PROGRAM, entry.to_str().unwrap())
        .expect("comptime front end accepts authority project");
    assert!(output.rust.contains("authority"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn i9_repl_keeps_the_authority_project_meaning() {
    let root = authority_project("repl");
    let project = root.to_string_lossy().to_string();
    let transcript = jet::REPL::run_transcript(&["run()"], Some(&project));
    assert!(transcript.contains("authority"), "REPL changed authority meaning: {transcript}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn i9_web_consumes_the_authority_project() {
    let root = authority_project("web");
    let entry = root.join("run.jet");
    let output = jet::compile_web(entry.to_str().unwrap()).expect("web accepts authority project");
    assert!(output.web.is_some(), "web tier dropped the authority project");
    let _ = std::fs::remove_dir_all(root);
}
