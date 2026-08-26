//! #1180: hostile executor closeout proof.
//!
//! Keep this proof separate from the platform-specific rows in
//! `agent_executor.rs`: it owns the hostile matrix and does not compete with
//! the Windows/child-tree edits in that file.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

fn native_backend_available() -> bool {
    jetpack::RuntimePolicy::detect_sandbox().level == jetpack::RuntimePolicy::SandboxLevel::Strong
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[test]
fn agent_executor_rejects_unenforced_grants_before_spawn_on_all_tiers() {
    let marker = std::env::temp_dir().join(format!(
        "jet-agent-executor-unenforced-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    let source = format!(
        r#"
use core.process as process

fn run() {{
    policy :: Authority.from_rights(["FS.Read:home"])
    spec :: process.cmd(["sh", "-c", "printf escaped > '{marker}'"]).under(policy)
    if spec.plan() == {{
        .Ok(_) -> print("escaped")
        .Err(_) -> print("refused")
    }}
}}
"#,
        marker = marker.display()
    );
    tir_support::assert_tiers_agree("agent_executor_unenforced_grant", &source, "refused\n");
    assert!(!marker.exists(), "unenforced authority grant reached spawn");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn agent_executor_limits_and_secret_receipts_match_all_tiers() {
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()

    limited :: process.cmd(["sh", "-c", "printf 123456"])
        .stdout(.Capture)
        .stderr(.Capture)
        .output_limit(3)
        .under(policy)
    if limited.plan() == {
        .Ok(_) -> {
            if limited.run() == {
                .Ok(_) -> print("output-escaped")
                .Err(_) -> print("output-limit")
            }
        }
        .Err(_) -> print("unsupported")
    }

    timeout :: Duration.milliseconds(50) ?? panic("timeout duration")
    slow :: process.cmd(["sh", "-c", "while true; do :; done"])
        .timeout(timeout)
        .under(policy)
    if slow.plan() == {
        .Ok(_) -> {
            result :: slow.run()
            if result == {
                .Ok(receipt) -> print(receipt.timed_out)
                .Err(_) -> print("timeout-error")
            }
        }
        .Err(_) -> print("unsupported")
    }

    secret :: process.cmd(["sh", "-c", "printf '%s' \"$SECRET_TOKEN\""])
        .env_clear()
        .env("SECRET_TOKEN", "agent-receipt-secret")
        .under(policy)
    if secret.plan() == {
        .Ok(plan) -> {
            receipt :: secret.run() ?? panic("secret receipt")
            print(receipt.output)
            print(receipt.redacted)
            print(plan.policy_digest == receipt.policy_digest)
            print(plan.authority == receipt.authority)
        }
        .Err(_) -> print("unsupported")
    }
}
"#;
    let expected = if native_backend_available() {
        "output-limit\ntrue\n<redacted>\ntrue\ntrue\ntrue\n"
    } else {
        "unsupported\nunsupported\nunsupported\n"
    };
    tir_support::assert_tiers_agree("agent_executor_limits_receipts", source, expected);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn agent_executor_network_and_descendant_hostiles_fail_closed_on_all_tiers() {
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind hostile network oracle");
    listener
        .set_nonblocking(true)
        .expect("set hostile network oracle nonblocking");
    let port = listener
        .local_addr()
        .expect("network oracle address")
        .port();
    let stop = Arc::new(AtomicBool::new(false));
    let connected = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread_connected = Arc::clone(&connected);
    let network_thread = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !thread_stop.load(Ordering::Acquire) && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    thread_connected.store(true, Ordering::Release);
                    let mut bytes = Vec::new();
                    let _ = stream.read_to_end(&mut bytes);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });

    let sleep = host_executable("sleep");
    let source = format!(
        r#"
use core.process as process

fn run() {{
    policy :: process.workspace()

    network :: process.cmd(["bash", "-c", "if printf escaped > /dev/tcp/127.0.0.1/{port} 2>/dev/null; then exit 41; else exit 0; fi"]).under(policy)
    if network.plan() == {{
        .Ok(_) -> {{
            result :: network.run_checked()
            if result == {{
                .Ok(_) -> print("network-blocked")
                .Err(_) -> print("network-escaped")
            }}
        }}
        .Err(_) -> print("unsupported")
    }}

    descendants :: process.cmd(["sh", "-c", "({sleep} 1; printf child-leaked) & exit 0"])
        .stdout(.Capture)
        .stderr(.Capture)
        .under(policy)
    if descendants.plan() == {{
        .Ok(_) -> {{
            receipt :: descendants.run() ?? panic("descendant receipt")
            print(receipt.success)
            print(receipt.descendants)
            print(receipt.output == "")
        }}
        .Err(_) -> print("unsupported")
    }}
}}
"#,
        port = port,
        sleep = sleep
    );
    let expected = if native_backend_available() {
        "network-blocked\ntrue\ncontained\ntrue\n"
    } else {
        "unsupported\nunsupported\n"
    };
    tir_support::assert_tiers_agree("agent_executor_network_descendants", &source, expected);
    stop.store(true, Ordering::Release);
    network_thread.join().expect("network oracle thread");
    assert!(
        !connected.load(Ordering::Acquire),
        "authority-bound child reached host network"
    );
}

#[test]
fn agent_executor_example_matches_all_hosted_tiers() {
    let expected =
        if cfg!(any(target_os = "linux", target_os = "macos")) && native_backend_available() {
            "agent-safe\ntrue\ntrue\n"
        } else {
            "unsupported\n"
        };
    tir_support::assert_example_cli_tiers_agree("io/agent_executor", expected);
}
