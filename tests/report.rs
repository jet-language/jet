mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn scratch(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "jet-report-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn compile_cascade_json_snapshot_keeps_roots_and_pruned_sites() {
    let source = include_str!("ui/e2392_root_cascade.jet");
    let diagnostics = jet::check_with_path("tests/ui/e2392_root_cascade.jet");
    assert!(!diagnostics.is_empty(), "cascade fixture must be rejected");
    let actual = jet::render_all_json(
        &jet::Diagnostics::ReportPath::from_process("<e2392_root_cascade.jet>"),
        source,
        &diagnostics,
    );
    assert_eq!(
        actual,
        include_str!("ui/e2392_root_cascade.json"),
        "JSON must retain one report per failure domain after pruning repeated sites"
    );
}

#[test]
fn compile_linked_cascade_json_snapshot_keeps_cause_chain() {
    let source = include_str!("ui/e2392_linked_cascade.jet");
    let diagnostics = jet::check_with_path("tests/ui/e2392_linked_cascade.jet");
    assert!(!diagnostics.is_empty(), "linked cascade fixture must be rejected");
    let actual = jet::render_all_json(
        &jet::Diagnostics::ReportPath::from_process("<e2392_linked_cascade.jet>"),
        source,
        &diagnostics,
    );
    assert_eq!(
        actual,
        include_str!("ui/e2392_linked_cascade.json"),
        "JSON must preserve the root-first cause chain"
    );
}

#[test]
fn zero_telemetry_policy_docs_and_source_audit() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let policy = fs::read_to_string(manifest.join("Docs/spec/reference/network-policy.md")).unwrap();
    assert!(policy.contains("D-TELEMETRY1=A"));
    assert!(policy.contains("Jet sends no telemetry"));
    assert!(policy.contains("Inventory of toolchain network paths"));
    assert!(!policy.contains("future send operation"));

    let forbidden_endpoints = [
        "reports.jet-lang.dev",
        "telemetry.jet-lang.dev",
        "/v1/telemetry",
        "sentry.io",
        "crashlytics",
        "segment.io",
        "gotelemetry",
    ];
    let roots = [
        manifest.join("Source"),
        manifest.join("crates/jet-cli"),
        manifest.join("crates/jetpack/src"),
    ];
    for root in roots {
        for entry in walkdir(&root) {
            let path = entry.as_path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let text = fs::read_to_string(path).unwrap();
            for needle in forbidden_endpoints {
                assert!(
                    !text.contains(needle),
                    "{} must not name telemetry endpoint `{needle}`",
                    path.display()
                );
            }
        }
    }
}

fn walkdir(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn traced_network_calls(
    root: &Path,
    tag: &str,
    args: &[&str],
) -> (std::process::Output, Vec<String>) {
    Command::new("strace")
        .arg("-V")
        .output()
        .expect("strace is required for the Linux no-network proof");
    let trace = root.join(format!("{tag}.network.trace"));
    let output = Command::new("strace")
        .args(["-f", "-qq", "-e", "trace=network", "-o"])
        .arg(&trace)
        .arg(jet())
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    let calls = fs::read_to_string(&trace)
        .unwrap()
        .lines()
        .filter(|line| !line.contains("--- SIG"))
        .map(str::to_string)
        .collect();
    (output, calls)
}

#[cfg(target_os = "linux")]
#[test]
fn ordinary_build_opens_no_network_connection() {
    let build_root = scratch("build-network");
    fs::write(
        build_root.join("main.jet"),
        "fn run() { print(\"offline\") }\n",
    )
    .unwrap();
    let (build, calls) = traced_network_calls(&build_root, "build", &["build", "main.jet"]);
    assert!(
        build.status.success(),
        "build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );
    // rustc uses one local AF_UNIX socket pair to learn whether spawning its
    // linker succeeded. strace classifies that process IPC as "network", even
    // though it cannot address a host. Permit only that exact request/reply.
    let spawn_sockets = calls
        .iter()
        .filter(|line| {
            line.contains("socketpair(AF_UNIX, SOCK_SEQPACKET|SOCK_CLOEXEC, 0,")
                && line.ends_with("= 0")
        })
        .count();
    let spawn_replies = calls
        .iter()
        .filter(|line| line.contains("recvfrom(") && line.ends_with("\", 8, 0, NULL, NULL) = 0"))
        .count();
    // sendfile between local fds is process I/O; strace still labels it network.
    let unexpected = calls
        .iter()
        .filter(|line| {
            !(line.contains("socketpair(AF_UNIX, SOCK_SEQPACKET|SOCK_CLOEXEC, 0,")
                && line.ends_with("= 0"))
                && !(line.contains("recvfrom(") && line.ends_with("\", 8, 0, NULL, NULL) = 0"))
                && !line.contains("sendfile(")
        })
        .collect::<Vec<_>>();
    assert!(
        unexpected.is_empty(),
        "ordinary build made an unexpected network syscall:\n{}",
        unexpected
            .iter()
            .map(|line| line.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(
        spawn_sockets, spawn_replies,
        "rustc's local exec-status socket/reply must stay paired"
    );

    let _ = fs::remove_dir_all(build_root);
}
