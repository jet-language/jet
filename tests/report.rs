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

fn bundle_path(stdout: &[u8]) -> PathBuf {
    let line = String::from_utf8_lossy(stdout);
    PathBuf::from(
        line.strip_prefix("wrote local report bundle to ")
            .and_then(|value| value.strip_suffix('\n'))
            .unwrap_or_else(|| panic!("unexpected report output: {line}")),
    )
}

#[test]
fn report_is_explicit_local_private_and_repeatable() {
    let root = scratch("bundle");
    let source_secret = "JET_PRIVATE_SOURCE_MARKER_755";
    let environment_secret = "JET_PRIVATE_ENVIRONMENT_MARKER_755";
    fs::write(root.join("main.jet"), format!("// {source_secret}\n")).unwrap();
    let run = || {
        Command::new(jet())
            .arg("report")
            .env("JET_REPORT_PRIVATE_TEST", environment_secret)
            .current_dir(&root)
            .output()
            .unwrap()
    };

    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let relative = bundle_path(&first.stdout);
    assert!(relative.starts_with(Path::new(".jet/reports")));
    let bundle = root.join(&relative);
    let readme = fs::read(bundle.join("README.txt")).unwrap();
    let report = fs::read(bundle.join("report.txt")).unwrap();
    let names = fs::read_dir(&bundle)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"README.txt".into()));
    assert!(names.contains(&"report.txt".into()));

    let text = String::from_utf8(report.clone()).unwrap();
    assert!(text.contains("policy: zero telemetry; no network transmission"));
    let all = format!("{}{}", String::from_utf8(readme.clone()).unwrap(), text);
    let mut private_values = vec![
        root.display().to_string(),
        jet().display().to_string(),
        source_secret.to_string(),
        environment_secret.to_string(),
    ];
    if let Ok(hostname) = fs::read_to_string("/etc/hostname") {
        private_values.push(hostname.trim().to_string());
    }
    if let Ok(user) = std::env::var("USER") {
        private_values.push(user);
    }
    for forbidden in private_values.iter().filter(|value| !value.is_empty()) {
        assert!(
            !all.contains(forbidden),
            "private value leaked: {forbidden}"
        );
    }
    for forbidden in [
        "source code",
        "current directory",
        "arguments:",
        "environment:",
        "hostname:",
        "username:",
    ] {
        assert!(!text.contains(forbidden), "private field leaked: {forbidden}");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bundle, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(
            bundle.join("README.txt"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        fs::set_permissions(
            bundle.join("report.txt"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
    }

    let second = run();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(second.stdout, first.stdout);
    assert_eq!(fs::read(bundle.join("README.txt")).unwrap(), readme);
    assert_eq!(fs::read(bundle.join("report.txt")).unwrap(), report);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&bundle).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(bundle.join("README.txt"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(bundle.join("report.txt"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    fs::write(bundle.join("README.txt"), "redacted by user\n").unwrap();
    let changed = run();
    assert!(!changed.status.success());
    assert_eq!(
        fs::read_to_string(bundle.join("README.txt")).unwrap(),
        "redacted by user\n"
    );

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn report_rejects_hostile_local_links() {
    use std::os::unix::fs::symlink;

    let root = scratch("hostile-links");
    let outside = scratch("hostile-outside");
    let run = || {
        Command::new(jet())
            .arg("report")
            .current_dir(&root)
            .output()
            .unwrap()
    };

    symlink(&outside, root.join(".jet")).unwrap();
    assert!(!run().status.success());
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);

    fs::remove_file(root.join(".jet")).unwrap();
    fs::create_dir(root.join(".jet")).unwrap();
    symlink(&outside, root.join(".jet/reports")).unwrap();
    assert!(!run().status.success());
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);

    fs::remove_file(root.join(".jet/reports")).unwrap();
    let created = run();
    assert!(created.status.success());
    let bundle = root.join(bundle_path(&created.stdout));
    let victim = outside.join("victim.txt");
    fs::write(&victim, "keep me\n").unwrap();
    fs::remove_file(bundle.join("README.txt")).unwrap();
    symlink(&victim, bundle.join("README.txt")).unwrap();
    assert!(!run().status.success());
    assert_eq!(fs::read_to_string(&victim).unwrap(), "keep me\n");

    fs::remove_dir_all(&bundle).unwrap();
    symlink(&outside, &bundle).unwrap();
    assert!(!run().status.success());
    assert_eq!(fs::read_to_string(&victim).unwrap(), "keep me\n");

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn report_is_registered_in_cli_surfaces() {
    assert!(jet::CLI::is_builtin("report"));
    assert!(!jet::CLI::is_builtin("telemetry"));
    assert!(jet::CLI::completions_bash().contains("report"));
    assert!(jet::CLI::completions_zsh().contains("report"));
    assert!(jet::CLI::completions_fish().contains("report"));
    assert!(jet::CLI::completions_powershell().contains("report"));
    assert!(jet::CLI::man_page(env!("CARGO_PKG_VERSION")).contains("report"));

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cli");
    for (name, exact) in [
        ("completions_bash.txt", "perf report bench"),
        (
            "completions_zsh.txt",
            "'report:Write a private local report bundle'",
        ),
        (
            "completions_fish.txt",
            "-a report -d 'Write a private local report bundle'",
        ),
        (
            "completions_powershell.txt",
            "'budget','perf','report','bench'",
        ),
        (
            "man.txt",
            ".B report\nWrite a private local report bundle",
        ),
    ] {
        let golden = fs::read_to_string(root.join(name)).unwrap();
        assert!(golden.contains(exact), "{name} is missing `{exact}`");
        assert!(
            !golden.contains("report --send") && !golden.contains("telemetry"),
            "{name} must not advertise report --send or telemetry"
        );
    }
}

#[test]
fn report_rejects_send_flag_without_writing_bundle() {
    let root = scratch("reject-send");
    for args in [
        vec!["report", "--send"],
        vec!["report", "--send=somewhere"],
        vec!["report", "--send", ".jet/reports/x"],
    ] {
        let output = Command::new(jet())
            .args(&args)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "expected failure for {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("`jet report --send` is not available"),
            "missing D-REPORT-SEND1 refusal for {args:?}: {stderr}"
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("wrote local report bundle"));
        assert!(!root.join(".jet").exists(), "send attempt must write nothing");
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn zero_telemetry_policy_docs_and_source_audit() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let policy = fs::read_to_string(manifest.join("docs/reference/network-policy.md")).unwrap();
    assert!(policy.contains("D-TELEMETRY1=A"));
    assert!(policy.contains("D-REPORT-SEND1=A"));
    assert!(policy.contains("Jet sends no telemetry"));
    assert!(policy.contains("There is no `jet report --send` command"));
    assert!(policy.contains("Inventory of toolchain network paths"));
    assert!(!policy.contains("future send operation"));

    let report_src = fs::read_to_string(manifest.join("Source/CmdReport.rs")).unwrap();
    for forbidden in [
        "std::net",
        "TcpStream",
        "UdpSocket",
        "reqwest",
        "ureq",
        "curl",
        "reports.jet-lang.org",
    ] {
        assert!(
            !report_src.contains(forbidden),
            "CmdReport.rs must stay offline; found `{forbidden}`"
        );
    }

    let forbidden_endpoints = [
        "reports.jet-lang.org",
        "telemetry.jet-lang.org",
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
fn ordinary_build_and_report_open_no_network_connection() {
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
                && !(line.contains("recvfrom(")
                    && line.ends_with("\", 8, 0, NULL, NULL) = 0"))
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

    let report_root = scratch("report-network");
    let (report, calls) = traced_network_calls(&report_root, "report", &["report"]);
    assert!(
        report.status.success(),
        "report failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&report.stdout),
        String::from_utf8_lossy(&report.stderr),
    );
    assert!(
        calls.is_empty(),
        "jet report made a network syscall:\n{}",
        calls.join("\n")
    );

    let _ = fs::remove_dir_all(build_root);
    let _ = fs::remove_dir_all(report_root);
}
