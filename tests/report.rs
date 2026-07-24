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
    assert!(jet::CLI::completions_bash().contains("report"));
    assert!(jet::CLI::completions_zsh().contains("report"));
    assert!(jet::CLI::completions_fish().contains("report"));
    assert!(jet::CLI::completions_powershell().contains("report"));
    assert!(jet::CLI::man_page(env!("CARGO_PKG_VERSION")).contains("report"));
}

#[cfg(target_os = "linux")]
#[test]
fn ordinary_build_opens_no_internet_connection() {
    Command::new("strace")
        .arg("-V")
        .output()
        .expect("strace is required for the Linux no-network proof");
    let root = scratch("build-network");
    fs::write(root.join("main.jet"), "fn run() { print(\"offline\") }\n").unwrap();
    let trace = root.join("network.trace");
    let output = Command::new("strace")
        .args(["-f", "-qq", "-e", "trace=network", "-o"])
        .arg(&trace)
        .arg(jet())
        .args(["build", "main.jet"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let calls = fs::read_to_string(&trace).unwrap();
    let internet = calls
        .lines()
        .filter(|line| {
            line.contains("socket(AF_INET")
                || line.contains("socket(AF_INET6")
                || (line.contains("connect(")
                    && (line.contains("AF_INET") || line.contains("AF_INET6")))
        })
        .collect::<Vec<_>>();
    assert!(
        internet.is_empty(),
        "ordinary build opened an Internet socket:\n{}",
        internet.join("\n")
    );
    let _ = fs::remove_dir_all(root);
}
