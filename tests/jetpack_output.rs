mod common;

use std::fs;
use std::path::{Path, PathBuf};

fn rust_sources(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn uncoded_errors_use_registered_report_snapshot() {
    let theme = jetpack::Output::Theme { color: false };
    let actual = theme.render_error_coded(
        "E1340",
        "package download failed",
        "the remote server closed the connection",
        "check the network connection and run the command again",
    );
    assert_eq!(
        actual,
        include_str!("fixtures/jetpack-diagnostics/E1340.stderr")
    );
}

#[test]
fn unsupported_nix_projection_uses_registered_report_snapshot() {
    let theme = jetpack::Output::Theme { color: false };
    let actual = theme.render_error_coded(
        "E1351",
        "Nix closure execution is unavailable on this host.",
        "Jetpack could not create the isolated /nix/store projection: rootless `/nix/store` projection needs `unshare`.",
        "Use a Linux host with unprivileged user and mount namespaces, or choose a native provider for this package.",
    );
    assert_eq!(
        actual,
        include_str!("fixtures/jetpack-diagnostics/E1351.stderr")
    );
}

#[test]
fn pinned_channel_update_uses_registered_report_snapshot() {
    let theme = jetpack::Output::Theme { color: false };
    let actual = theme.render_error_coded(
        "E1352",
        "Source `stable` is pinned and cannot be updated.",
        "a pinned source has no moving channel policy, so Jetpack must never change its lock or manifest",
        "declare `stable` with `#latest` for manual movement or `#auto` for automatic movement",
    );
    assert_eq!(
        actual,
        include_str!("fixtures/jetpack-diagnostics/E1352.stderr")
    );
}

#[test]
fn retired_off_compiler_report_renderers_do_not_return() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    rust_sources(&root.join("crates/jetpack/src"), &mut sources);
    sources.push(root.join("Source/CmdGc.rs"));

    let retired = [
        "eprintln!(\"error:",
        "eprintln!(\"Error [",
        "self.red(\"error:\")",
        "format!(\"error[{code}]:\")",
        "format!(\"warning[{code}]:\")",
        "matches the compiler's house style",
    ];
    for path in sources {
        let source = fs::read_to_string(&path).unwrap();
        for text in retired {
            assert!(
                !source.contains(text),
                "{} restored retired report text `{text}`",
                path.display()
            );
        }
    }
}
