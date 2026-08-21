#[path = "common/mod.rs"]
mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

#[test]
fn authority_is_a_named_prelude_rights_carrier() {
    let source = r#"
struct Holder {
    abilities: Abilities
}

fn run() {
    abilities :: Abilities.workspace()
    print("abilities")
}
"#;
    let output = jet::compile(source).expect("Abilities type should compile");
    assert!(output.rust.contains("pub struct JetAuthority"), "{}", output.rust);
    assert!(
        output
            .rust
            .contains("rights: std::collections::BTreeSet<String>"),
        "{}",
        output.rust
    );
    assert!(output.rust.contains("JetAuthority"), "{}", output.rust);
    assert!(
        output.rust.contains("JetAuthority::workspace()"),
        "{}",
        output.rust
    );
}

const AUTHORITY_VALUE_SOURCE: &str = r#"
fn run() {
    #Abilities(scope: IO) {
        abilities :: Abilities.workspace()
        narrowed :: abilities.with("FS.Read")
        released :: narrowed.without("FS.Read")
        print("abilities")
    }
}
"#;

#[test]
fn authority_with_and_without_are_the_only_narrowing_family() {
    let output = jet::compile(AUTHORITY_VALUE_SOURCE).expect("Abilities operations should compile");
    assert!(output.rust.contains("jet_authority_with"), "{}", output.rust);
    assert!(output.rust.contains("jet_authority_without"), "{}", output.rust);
    tir_support::assert_tiers_agree("authority_narrowing", AUTHORITY_VALUE_SOURCE, "abilities\n");
}

#[test]
fn authority_with_outside_held_rights_is_e0712() {
    let source = r#"
fn run() {
    abilities :: Abilities.workspace()
    abilities.with("FS.Write")
}
"#;
    let (_, _, stderr) = tir_support::jit_run("authority_outside", source);
    assert!(stderr.contains("E0712"), "JIT must report E0712: {stderr}");
}

#[test]
fn authority_boundary_consumers_take_the_named_value() {
    let source = r#"
use core.process as process
use core.plugin as plugin

fn run() {
    #Abilities(abilities: Exec, IO) {
        result :: process.run(["echo", "abilities"], abilities)
        plugin :: plugin.load("missing.wasm", abilities)
        print("boundary")
    }
}
"#;
    let output = jet::compile(source).expect("boundary APIs should accept Abilities");
    assert!(
        output.rust.contains("jet_std_process_run_with_authority"),
        "{}",
        output.rust
    );
    assert!(output.rust.contains("jet_plugin_load"), "{}", output.rust);
    assert!(output.rust.contains("JetAuthority"), "{}", output.rust);
}

#[test]
fn authority_is_not_a_type_selection_or_dispatch_input() {
    let source = r#"
fn run() {
        #Abilities(Abilities) {
        print("Abilities must remain ordinary data")
    }
}
"#;
    let diagnostics = jet::compile(source).expect_err("Abilities must not act as an effect/type fact");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0930"), "{diagnostics:#?}");
}

#[test]
fn authority_parser_accepts_the_named_value() {
    let (tokens, diagnostics) = jet::Lexer::lex("fn run() { #Abilities(scope: IO) { value :: Abilities.workspace() } }");
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    jet::Parser::parse(&tokens).expect("parser must accept Abilities");
}

#[test]
fn authority_sema_keeps_the_named_type() {
    let output = jet::compile(AUTHORITY_VALUE_SOURCE).expect("sema should accept Abilities");
    assert!(output.rust.contains("JetAuthority"), "{}", output.rust);
}

#[test]
fn authority_tir_runs_the_same_value() {
    tir_support::assert_tiers_agree("authority_tir", AUTHORITY_VALUE_SOURCE, "abilities\n");
}

#[test]
fn authority_aot_runs_the_same_value() {
    let (code, stdout, stderr) = tir_support::build_and_run_full("jet_authority_aot", "authority_aot", AUTHORITY_VALUE_SOURCE);
    assert_eq!(code, 0, "AOT failed: {stderr}");
    assert_eq!(stdout, "abilities\n");
}

#[test]
fn authority_jit_runs_the_same_value() {
    let (code, stdout, stderr) = tir_support::jit_run("authority_jit", AUTHORITY_VALUE_SOURCE);
    assert_eq!(code, 0, "JIT failed: {stderr}");
    assert_eq!(stdout, "abilities\n");
}

#[test]
fn authority_dev_runs_the_same_value() {
    let (code, stdout, stderr) = tir_support::interpreter_run("authority_dev", AUTHORITY_VALUE_SOURCE);
    assert_eq!(code, 0, "dev/interpreter failed: {stderr}");
    assert_eq!(stdout, "abilities\n");
}

#[test]
fn authority_comptime_uses_the_same_value() {
    let source = "@abilities :: Abilities.workspace().with(\"FS.Read\")\n\nfn run() { print(\"abilities\") }\n";
    let output = jet::compile(source).expect("comptime should construct Abilities");
    assert!(output.rust.contains("JetAuthority"), "{}", output.rust);
}

#[test]
fn authority_repl_accepts_the_same_value() {
    let transcript = jet::REPL::run_transcript(
        &["abilities :: Abilities.workspace()", "narrowed :: abilities.with(\"FS.Read\")", "print(\"abilities\")"],
        None,
    );
    assert!(transcript.contains("abilities"), "REPL changed Abilities meaning: {transcript}");
}

#[test]
fn authority_web_accepts_the_same_value() {
    let root = std::env::temp_dir().join(format!("jet_authority_web_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("web abilities scratch");
    let entry = root.join("run.jet");
    std::fs::write(&entry, AUTHORITY_VALUE_SOURCE).expect("web Abilities source");
    let output = jet::compile_web(entry.to_str().unwrap()).expect("web should accept Abilities");
    let web = output.web.expect("web tier dropped Abilities");
    assert!(web.js_app.contains("jet_authority_with"), "web lost Abilities.with");
    assert!(web.js_app.contains("jet_authority_without"), "web lost Abilities.without");
    let _ = std::fs::remove_dir_all(root);
}
