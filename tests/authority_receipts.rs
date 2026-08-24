//! #1179: authority-bound receipts must not expose an unredacted output path.

#[path = "common/mod.rs"]
mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn native_backend_available() -> bool {
    jetpack::RuntimePolicy::detect_sandbox().level == jetpack::RuntimePolicy::SandboxLevel::Strong
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn authority_process_receipt_redacts_success_argv_and_output_on_all_hosted_tiers() {
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["/bin/sh", "-c", "printf '%s' \"$SECRET_TOKEN\"; printf '%s' authority-success-secret >&2"])
        .env_clear()
        .env("SECRET_TOKEN", "authority-success-secret")
        .stdout(.Capture)
        .stderr(.Capture)
        .under(policy)
    if spec.plan() == {
        .Ok(plan) -> {
            receipt :: spec.run() ?? panic("authority-bound receipt failed")
            print(plan.policy_digest == receipt.policy_digest)
            print(plan.argv[2].contains("authority-success-secret") == false)
            print(receipt.argv[2].contains("authority-success-secret") == false)
            print(receipt.output)
            print(receipt.errors)
            print(receipt.success)
            print(receipt.redacted)
        }
        .Err(_) -> print("unsupported")
    }
}
"#;
    let expected = if native_backend_available() {
        "true\ntrue\ntrue\n<redacted>\n<redacted>\ntrue\ntrue\n"
    } else {
        "unsupported\n"
    };
    tir_support::assert_tiers_agree(
        "authority_process_receipt_success_redaction",
        source,
        expected,
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn authority_process_receipt_redacts_failed_output_on_all_hosted_tiers() {
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["/bin/sh", "-c", "printf '%s' \"$SECRET_TOKEN\" >&2; exit 17"])
        .env_clear()
        .env("SECRET_TOKEN", "authority-failure-secret")
        .stdout(.Capture)
        .stderr(.Capture)
        .under(policy)
    if spec.plan() == {
        .Ok(_) -> {
            receipt :: spec.run() ?? panic("authority-bound failed receipt missing")
            print(receipt.success)
            print(receipt.code == 17)
            print(receipt.errors)
            print(receipt.redacted)
        }
        .Err(_) -> print("unsupported")
    }
}
"#;
    let expected = if native_backend_available() {
        "false\ntrue\n<redacted>\ntrue\n"
    } else {
        "unsupported\n"
    };
    tir_support::assert_tiers_agree(
        "authority_process_receipt_failure_redaction",
        source,
        expected,
    );
}

#[cfg(unix)]
#[test]
fn authority_process_refuses_live_output_before_spawn_on_all_hosted_tiers() {
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["/bin/sh", "-c", "printf '%s' \"$SECRET_TOKEN\""])
        .env_clear()
        .env("SECRET_TOKEN", "stream-secret")
        .stdout(.Stream)
        .stderr(.Capture)
        .under(policy)
    if spec.run() == {
        .Ok(_) -> print("accepted")
        .Err(_) -> print("refused")
    }
}
"#;
    tir_support::assert_tiers_agree(
        "authority_process_receipt_stream_redaction",
        source,
        "refused\n",
    );
}

#[cfg(unix)]
#[test]
fn authority_process_refuses_terminal_audit_bypass_on_all_hosted_tiers() {
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["/bin/sh", "-c", "printf terminal-secret"])
        .terminal()
        .under(policy)
    if spec.plan() == {
        .Ok(_) -> print("accepted")
        .Err(_) -> print("refused")
    }
}
"#;
    tir_support::assert_tiers_agree(
        "authority_process_terminal_audit_boundary",
        source,
        "refused\n",
    );
}
