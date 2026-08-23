//! #1179: authority-bound receipts must not expose an unredacted output path.

#[path = "common/mod.rs"]
mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

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
