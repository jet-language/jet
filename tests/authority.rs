#[path = "common/mod.rs"]
mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

#[test]
fn authority_is_a_named_prelude_rights_carrier() {
    let source = r#"
struct Holder {
    authority: Authority
}

fn run() {
    authority :: Authority.workspace()
    print("authority")
}
"#;
    let output = jet::compile(source).expect("Authority type should compile");
    assert!(
        output.rust.contains("pub struct JetAuthority"),
        "{}",
        output.rust
    );
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
    assert!(output.rust.contains("FS.Read:repo"), "{}", output.rust);
    assert!(
        output.rust.contains("FS.Write:.jet/build"),
        "{}",
        output.rust
    );
}

fn host_executable(name: &str) -> String {
    std::env::split_paths(&std::env::var_os("PATH").expect("PATH should be set"))
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| std::fs::canonicalize(candidate).ok())
        .unwrap_or_else(|| std::path::PathBuf::from(format!("/bin/{name}")))
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

const AUTHORITY_VALUE_SOURCE: &str = r#"
fn run() {
    #FX(authority: FS.Read, IO) {
        narrowed :: authority.with("FS.Read")
        _released :: narrowed.without("FS.Read")
        print("authority")
    }
}
"#;

#[test]
fn authority_with_and_without_are_the_only_narrowing_family() {
    let output = jet::compile(AUTHORITY_VALUE_SOURCE).expect("Authority operations should compile");
    assert!(
        output.rust.contains("jet_authority_with"),
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("jet_authority_without"),
        "{}",
        output.rust
    );
    tir_support::assert_tiers_agree("authority_narrowing", AUTHORITY_VALUE_SOURCE, "authority\n");
}

#[test]
fn authority_with_outside_held_rights_is_e0712() {
    let source = r#"
fn run() {
    authority :: Authority.workspace()
    authority.with("FS.Write")
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

struct SessionHolder {
    authority: Authority
}

fn run() {
    session :: SessionHolder{authority: Authority.workspace()}
    #FX(authority: Exec, IO) {
        result :: process.run(["echo", "authority"], authority)
        plugin :: plugin.load("missing.wasm", session.authority)
        print("boundary")
    }
}
"#;
    let output = jet::compile(source).expect("boundary APIs should accept Authority");
    assert!(
        output.rust.contains("jet_std_process_run_with_authority"),
        "{}",
        output.rust
    );
    assert!(output.rust.contains("jet_plugin_load"), "{}", output.rust);
    assert!(output.rust.contains("JetAuthority"), "{}", output.rust);
    assert!(
        output.rust.contains("jet_authority_to_wire"),
        "{}",
        output.rust
    );
    assert!(!output.rust.contains("let _authority"), "{}", output.rust);
}

#[test]
fn authority_process_boundary_runs_on_all_hosted_tiers() {
    let source = r#"
use core.process as process

fn run() {
    #FX(authority: Exec, IO) {
        result :: process.run(["echo", "boundary"], authority)
        print("boundary")
    }
}
"#;
    tir_support::assert_tiers_agree("authority_process_boundary", source, "boundary\n");
}

#[test]
fn authority_plan_refuses_before_spawn_on_all_hosted_tiers() {
    let marker =
        std::env::temp_dir().join(format!("jet-authority-plan-marker-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["sh", "-c", "printf spawned > '__MARKER__'"]).under(policy)
    if spec.plan() == {
        .Ok(_) -> print("spawned")
        .Err(_) -> print("refused")
    }
}
"#
    .replace("__MARKER__", &marker.to_string_lossy());
    let output = jet::compile(&source).expect("authority plan API should compile");
    assert!(
        output.rust.contains("jet_std_process_spec_under"),
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("jet_process_spec_plan"),
        "{}",
        output.rust
    );
    let native_backend = jetpack::RuntimePolicy::detect_sandbox().level
        == jetpack::RuntimePolicy::SandboxLevel::Strong;
    let expected = if cfg!(any(target_os = "linux", target_os = "macos")) && native_backend {
        "spawned\n"
    } else {
        "refused\n"
    };
    tir_support::assert_tiers_agree("authority_plan_no_spawn", &source, expected);
    assert!(
        !marker.exists(),
        "plan() spawned the authority-bound command"
    );
}

#[test]
fn authority_process_binds_exact_resource_grants() {
    let cargo = host_executable("cargo");
    let source = r#"
use core.process as process

fn run() {
    policy :: Authority.from_rights([
        "FS.Read:repo",
        "FS.Write:.jet/build",
        "Exec:__CARGO__",
    ])
    spec :: process.cmd(["__CARGO__", "test"]).under(policy)
    if spec.plan() == {
        .Ok(_) -> print("planned")
        .Err(_) -> print("refused")
    }
}
"#
    .replace("__CARGO__", &cargo);
    let output = jet::compile(&source).expect("exact process grants should compile");
    assert!(
        output.rust.contains("JetAuthority::__jet_from_rights"),
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("jet_std_process_spec_under"),
        "{}",
        output.rust
    );
    let native_backend = jetpack::RuntimePolicy::detect_sandbox().level
        == jetpack::RuntimePolicy::SandboxLevel::Strong;
    let expected = if cfg!(any(target_os = "linux", target_os = "macos")) && native_backend {
        "planned\n"
    } else {
        "refused\n"
    };
    tir_support::assert_tiers_agree("authority_process_exact_grants", &source, expected);
}

#[test]
fn authority_process_refuses_unenforced_scoped_network() {
    let source = r#"
use core.process as process

fn run() {
    policy :: Authority.from_rights(["Net:example.com"])
    spec :: process.cmd(["/usr/bin/true"]).under(policy)
    if spec.plan() == {
        .Ok(_) -> print("accepted")
        .Err(_) -> print("refused")
    }
}
"#;
    tir_support::assert_tiers_agree("authority_process_scoped_network", source, "refused\n");
}

#[cfg(unix)]
#[test]
fn authority_process_receipt_redacts_secret_output_on_all_hosted_tiers() {
    let source = r#"
use core.process as process

fn run() {
    spec :: process.cmd(["sh", "-c", "printf '%s' \"$SECRET_TOKEN\""])
        .env_clear()
        .env("SECRET_TOKEN", "receipt-secret")
        .stdout(.Capture)
        .stderr(.Capture)
    receipt :: spec.run() ?? panic("process receipt failed")
    print(receipt.output)
    print(receipt.redacted)
    print(receipt.policy_digest != "")
    print(receipt.descendants)
}
"#;
    let output = jet::compile(source).expect("ProcessReceipt fields should compile");
    assert!(output.rust.contains("ProcessReceipt"), "{}", output.rust);
    tir_support::assert_tiers_agree(
        "authority_process_receipt_redaction",
        source,
        "<redacted>\ntrue\ntrue\ncontained\n",
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn authority_process_plan_and_receipt_share_the_policy_digest() {
    let printf = host_executable("printf");
    let source = r#"
use core.process as process

fn run() {
    policy :: Authority.from_rights([
        "FS.Read:repo",
        "FS.Write:.jet/build",
        "Exec:__PRINTF__",
    ])
    spec :: process.cmd(["__PRINTF__", "receipt"]).under(policy)
    if spec.plan() == {
        .Ok(plan) -> {
            receipt :: spec.run() ?? panic("run failed")
            print(plan.policy_digest == receipt.policy_digest)
            print(receipt.redacted)
        }
        .Err(_) -> print("refused")
    }
}
"#
    .replace("__PRINTF__", &printf);
    let native_backend = jetpack::RuntimePolicy::detect_sandbox().level
        == jetpack::RuntimePolicy::SandboxLevel::Strong;
    tir_support::assert_tiers_agree(
        "authority_process_receipt_digest",
        &source,
        if native_backend {
            "true\ntrue\n"
        } else {
            "refused\n"
        },
    );
}

#[cfg(target_os = "macos")]
#[test]
fn authority_process_macos_seatbelt_runs_and_denies_host_read() {
    let success = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["/usr/bin/printf", "sandboxed"]).under(policy)
    if spec.plan() == {
        .Ok(plan) -> {
            print(plan.backend)
            result :: spec.run_checked()
            if result == {
                .Ok(value) -> print(value.output)
                .Err(_) -> print("denied")
            }
        }
        .Err(_) -> print("refused")
    }
}
"#;
    tir_support::assert_tiers_agree(
        "authority_macos_seatbelt_success",
        success,
        "macos-seatbelt\nsandboxed\n",
    );

    let hostile = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    result :: process.cmd(["/bin/sh", "-c", "if test -r /etc/passwd; then exit 41; else exit 0; fi"]).under(policy).run_checked()
    if result == {
        .Ok(_) -> print("blocked")
        .Err(_) -> print("escaped")
    }
}
"#;
    tir_support::assert_tiers_agree("authority_macos_seatbelt_host_read", hostile, "blocked\n");
}

#[cfg(target_os = "linux")]
#[test]
fn authority_process_linux_bwrap_runs_and_denies_host_read() {
    let success = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["printf", "sandboxed"]).under(policy)
    if spec.plan() == {
        .Ok(plan) -> {
            if {
                plan.backend == "linux-bwrap" -> {
                    result :: spec.run_checked()
                    if result == {
                        .Ok(value) -> print(value.output)
                        .Err(_) -> print("denied")
                    }
                }
                else -> print("wrong-backend")
            }
        }
        .Err(_) -> print("refused")
    }
}
"#;
    let native_backend = jetpack::RuntimePolicy::detect_sandbox().level
        == jetpack::RuntimePolicy::SandboxLevel::Strong;
    tir_support::assert_tiers_agree(
        "authority_linux_bwrap_success",
        success,
        if native_backend {
            "sandboxed\n"
        } else {
            "refused\n"
        },
    );

    let hostile = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    result :: process.cmd(["sh", "-c", "if test -r /etc/passwd; then exit 41; else exit 0; fi"]).under(policy).run_checked()
    if result == {
        .Ok(_) -> print("blocked")
        .Err(_) -> print("escaped")
    }
}
"#;
    tir_support::assert_tiers_agree(
        "authority_linux_bwrap_host_read",
        hostile,
        if native_backend {
            "blocked\n"
        } else {
            "refused\n"
        },
    );
}

#[cfg(target_os = "windows")]
#[test]
fn authority_process_windows_appcontainer_runs_and_denies_host_read() {
    let success = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    result :: process.cmd(["cmd.exe", "/C", "exit", "0"]).under(policy).run_checked()
    if result == {
        .Ok(_) -> print("sandboxed")
        .Err(_) -> print("refused")
    }
}
"#;
    tir_support::assert_tiers_agree(
        "authority_windows_appcontainer_success",
        success,
        "sandboxed\n",
    );

    let marker = std::env::temp_dir().join(format!(
        "jet-authority-windows-host-secret-{}.txt",
        std::process::id()
    ));
    std::fs::write(&marker, "host-secret").expect("write Windows host-read marker");
    let marker = marker.to_string_lossy().replace('\\', "/");
    let hostile = format!(
        r#"
use core.process as process

fn run() {{
    policy :: process.workspace()
    result :: process.cmd(["cmd.exe", "/C", "type \"{marker}\""]).under(policy).run_checked()
    if result == {{
        .Ok(_) -> print("escaped")
        .Err(_) -> print("blocked")
    }}
}}
"#
    );
    tir_support::assert_tiers_agree(
        "authority_windows_appcontainer_host_read",
        &hostile,
        "blocked\n",
    );
    let _ = std::fs::remove_file(marker);
}

#[cfg(target_os = "windows")]
#[test]
fn authority_process_windows_plan_and_receipt_share_policy_digest() {
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["cmd.exe", "/C", "exit", "0"]).under(policy)
    plan :: spec.plan() ?? panic("plan failed")
    receipt :: spec.run() ?? panic("run failed")
    print(plan.policy_digest == receipt.policy_digest)
    print(receipt.backend)
}
"#;
    tir_support::assert_tiers_agree(
        "authority_process_windows_receipt_digest",
        source,
        "true\nwindows-appcontainer\n",
    );
}

#[test]
fn authority_example_runs_on_all_hosted_tiers() {
    tir_support::assert_example_cli_tiers_agree("types/authority", "authority\n");
}

#[test]
fn authority_is_not_a_type_selection_or_dispatch_input() {
    let source = r#"
fn run() {
        #FX(Authority) {
        print("Authority must remain ordinary data")
    }
}
"#;
    let diagnostics =
        jet::compile(source).expect_err("Authority must not act as an effect/type fact");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0930"),
        "{diagnostics:#?}"
    );
}

#[test]
fn authority_parser_accepts_the_named_value() {
    let (tokens, diagnostics) = jet::Lexer::lex(
        "fn run() { #FX(authority: FS.Read) { value :: authority.with(\"FS.Read\") } }",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    jet::Parser::parse(&tokens).expect("parser must accept Authority");
}

#[test]
fn authority_sema_keeps_the_named_type() {
    let output = jet::compile(AUTHORITY_VALUE_SOURCE).expect("sema should accept Authority");
    assert!(output.rust.contains("JetAuthority"), "{}", output.rust);
}

#[test]
fn authority_tir_runs_the_same_value() {
    tir_support::assert_tiers_agree("authority_tir", AUTHORITY_VALUE_SOURCE, "authority\n");
}

#[test]
fn authority_aot_runs_the_same_value() {
    let (code, stdout, stderr) = tir_support::build_and_run_full(
        "jet_authority_aot",
        "authority_aot",
        AUTHORITY_VALUE_SOURCE,
    );
    assert_eq!(code, 0, "AOT failed: {stderr}");
    assert_eq!(stdout, "authority\n");
}

#[test]
fn authority_jit_runs_the_same_value() {
    let (code, stdout, stderr) = tir_support::jit_run("authority_jit", AUTHORITY_VALUE_SOURCE);
    assert_eq!(code, 0, "JIT failed: {stderr}");
    assert_eq!(stdout, "authority\n");
}

#[test]
fn authority_dev_runs_the_same_value() {
    let (code, stdout, stderr) =
        tir_support::interpreter_run("authority_dev", AUTHORITY_VALUE_SOURCE);
    assert_eq!(code, 0, "dev/interpreter failed: {stderr}");
    assert_eq!(stdout, "authority\n");
}

#[test]
fn authority_comptime_uses_the_same_value() {
    let source = "@authority :: Authority.from_rights([\"FS.Read\", \"IO\"])\n@narrowed :: authority.with(\"FS.Read\")\n@released :: narrowed.without(\"FS.Read\")\n\nfn run() { print(\"authority\") }\n";
    let output = jet::compile(source).expect("comptime should construct Authority");
    assert!(output.rust.contains("JetAuthority"), "{}", output.rust);
}

#[test]
fn authority_repl_accepts_the_same_value() {
    let transcript = jet::REPL::run_transcript(
        &[
            "authority :: Authority.workspace()",
            "narrowed :: authority.with(\"FS.Read\")",
            "released :: narrowed.without(\"FS.Read\")",
            "#FX(scoped: FS.Read) { inside :: scoped.with(\"FS.Read\") }",
            "print(\"authority\")",
        ],
        None,
    );
    assert!(
        transcript.contains("authority"),
        "REPL changed Authority meaning: {transcript}"
    );
}

#[test]
fn authority_web_accepts_the_same_value() {
    let source = "#Target(Web)\nfn run() {\n    #FX(authority: IO) {\n        narrowed :: authority.with(\"IO\")\n        released :: narrowed.without(\"IO\")\n        value :: released\n    }\n}\n";
    let web = jet::compile_web_with_path(source, "tests/fixtures/authority_web.jet")
        .expect("web should accept Authority")
        .web
        .expect("web tier dropped Authority");
    assert!(
        web.wasm_rust
            .contains("pub extern \"C\" fn jet_export_run() -> i32"),
        "web run export missing"
    );
    assert!(
        web.wasm_rust.contains("jet_authority_with"),
        "web lost Authority.with"
    );
    assert!(
        web.wasm_rust.contains("jet_authority_without"),
        "web lost Authority.without"
    );
    assert!(
        !web.wasm_rust.contains("struct Authority"),
        "web handle leaked into emission"
    );
}
