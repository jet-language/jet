//! #1180: authority-bound agent execution closeout.
//!
//! These sources go through the public Core process surface. The parity helper
//! runs each one through AOT, default `jet run`, and the forced interpreter.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn native_backend_available() -> bool {
    jetpack::RuntimePolicy::detect_sandbox().level == jetpack::RuntimePolicy::SandboxLevel::Strong
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn agent_executor_plan_and_receipt_preserve_one_policy_across_tiers() {
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["printf", "agent-safe"])
        .stdout(.Capture)
        .stderr(.Capture)
        .under(policy)
    if spec.plan() == {
        .Ok(plan) -> {
            receipt :: spec.run_checked() ?? panic("authority-bound run failed")
            print(plan.executable_identity == receipt.executable_identity)
            print(plan.argv == receipt.argv)
            print(plan.input_digest == receipt.input_digest)
            print(plan.policy_digest == receipt.policy_digest)
            print(plan.backend == receipt.backend)
            print(plan.authority == receipt.authority)
            print(plan.descendants == receipt.descendants)
            print(plan.limits == receipt.limits)
            print(plan.outputs[0] == receipt.outputs[0])
            print(plan.outputs[1] == receipt.outputs[1])
            print(receipt.outputs.len() == plan.outputs.len() + 1)
            print(receipt.output)
            print(receipt.redacted)
        }
        .Err(_) -> print("unsupported")
    }
}
"#;
    let expected = if native_backend_available() {
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\nagent-safe\ntrue\n"
    } else {
        "unsupported\n"
    };
    tir_support::assert_tiers_agree("agent_executor_plan_receipt", source, expected);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn agent_executor_hostile_filesystem_examples_fail_closed_on_all_tiers() {
    let marker = std::env::temp_dir().join(format!(
        "jet-agent-executor-host-write-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    read_spec :: process.cmd(["sh", "-c", "if test -r /etc/passwd; then exit 41; else exit 0; fi"]).under(policy)
    if read_spec.plan() == {
        .Ok(_) -> {
            read :: read_spec.run_checked()
            if read == {
                .Ok(_) -> print("host-read-blocked")
                .Err(_) -> print("host-read-escaped")
            }
        }
        .Err(_) -> print("unsupported")
    }

    write_spec :: process.cmd(["sh", "-c", "if printf escaped > '__MARKER__'; then exit 41; else exit 0; fi"]).under(policy)
    if write_spec.plan() == {
        .Ok(_) -> {
            write :: write_spec.run_checked()
            if write == {
                .Ok(_) -> print("host-write-blocked")
                .Err(_) -> print("host-write-escaped")
            }
        }
        .Err(_) -> print("unsupported")
    }
}
"#
    .replace("__MARKER__", &marker.to_string_lossy());
    let expected = if native_backend_available() {
        "host-read-blocked\nhost-write-blocked\n"
    } else {
        "unsupported\nunsupported\n"
    };
    tir_support::assert_tiers_agree("agent_executor_hostile_filesystem", &source, expected);
    assert!(
        !marker.exists(),
        "authority-bound process wrote outside its output grant"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn agent_executor_explicit_cancellation_reaps_descendants_on_all_tiers() {
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["sh", "-c", "while true; do sleep 1; done"])
        .stdout(.Capture)
        .stderr(.Capture)
        .under(policy)
    if spec.spawn() == {
        .Ok(child) -> {
            child.kill() ?? panic("authority-bound cancellation failed")
            receipt :: child.wait() ?? panic("cancelled child did not reap")
            print(receipt.success)
            print(receipt.timed_out)
            print(receipt.descendants)
        }
        .Err(_) -> print("unsupported")
    }
}
"#;
    let expected = if native_backend_available() {
        "false\nfalse\ncontained\n"
    } else {
        "unsupported\n"
    };
    tir_support::assert_tiers_agree("agent_executor_cancellation", source, expected);
}

#[cfg(target_os = "windows")]
#[test]
fn agent_executor_windows_appcontainer_plan_and_receipt_preserve_policy() {
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["cmd.exe", "/C", "exit", "0"]).under(policy)
    if spec.plan() == {
        .Ok(plan) -> {
            receipt :: spec.run_checked() ?? panic("Windows authority-bound run failed")
            print(plan.backend)
            print(plan.executable_identity == receipt.executable_identity)
            print(plan.argv == receipt.argv)
            print(plan.input_digest == receipt.input_digest)
            print(plan.policy_digest == receipt.policy_digest)
            print(plan.authority == receipt.authority)
            print(plan.descendants == receipt.descendants)
            print(plan.limits == receipt.limits)
            print(plan.outputs[0] == receipt.outputs[0])
            print(plan.outputs[1] == receipt.outputs[1])
            print(receipt.outputs.len() == plan.outputs.len() + 1)
            print(receipt.redacted)
        }
        .Err(_) -> print("refused")
    }
}
"#;
    tir_support::assert_tiers_agree(
        "agent_executor_windows_appcontainer_plan_receipt",
        source,
        "windows-appcontainer\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
}

#[cfg(target_os = "windows")]
#[test]
fn agent_executor_windows_appcontainer_denies_host_read() {
    let marker = std::env::temp_dir().join(format!(
        "jet-agent-executor-windows-host-secret-{}.txt",
        std::process::id()
    ));
    std::fs::write(&marker, "host-secret").expect("write Windows host-read marker");
    let marker = marker.to_string_lossy().replace('\\', "/");
    let source = format!(
        r#"
use core.process as process

fn run() {{
    policy :: process.workspace()
    spec :: process.cmd(["cmd.exe", "/C", "type \"{marker}\""]).under(policy)
    if spec.plan() == {{
        .Ok(plan) -> {{
            print(plan.backend)
            result :: spec.run_checked()
            if result == {{
                .Ok(_) -> print("escaped")
                .Err(_) -> print("blocked")
            }}
        }}
        .Err(_) -> print("refused")
    }}
}}
"#
    );
    tir_support::assert_tiers_agree(
        "agent_executor_windows_appcontainer_host_read",
        &source,
        "windows-appcontainer\nblocked\n",
    );
    let _ = std::fs::remove_file(marker);
}
