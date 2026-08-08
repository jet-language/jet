//! Card #1661 (D-ONCE-*): the jet binary panic hook renders one branded ICE
//! report on every uncaught panic, and the five hand-typed ICE banner
//! phrasings that used to live in `Source/CmdCompile.rs` collapse into the
//! one home: `jet_foundation::Diagnostics::render_ice_report`.
//!
//! Two guards:
//!   - `no_hand_typed_ice_banner_outside_the_one_home`: fails if a second
//!     hand-typed "internal compiler error:" *banner line* (the report's
//!     first line, as opposed to a plain panic message) reappears anywhere
//!     under `Source/` outside `Diagnostics.rs` itself.
//!   - `panic_hook_prints_the_branded_report_and_exits_101`: spawns the real
//!     `jet` binary with a deliberate panic (`JET_ICE_SELF_TEST=1`) and
//!     proves the hook's actual output: no raw Rust panic text / no
//!     `RUST_BACKTRACE` hint, the branded report instead, exit code 101.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_hand_typed_ice_banner_outside_the_one_home() {
    let root = root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("Source"), &mut files);

    let mut violations = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).unwrap_or_default();
        for (n, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            // A hand-typed banner literal: the exact ICE first-line prefix
            // inside a string, NOT a call into render_ice_report (which
            // legitimately passes a `what` string that may start with other
            // words) and NOT a reference to the shared function/macro.
            if (line.contains("\"internal compiler error")
                || line.contains("(internal compiler error)"))
                && !line.contains("render_ice_report")
            {
                violations.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "hand-typed ICE banner outside the one home (jet_foundation::Diagnostics::render_ice_report):\n{}",
        violations.join("\n")
    );
}

#[test]
fn panic_hook_prints_the_branded_report_and_exits_101() {
    let bin = env!("CARGO_BIN_EXE_jet");
    let out = Command::new(bin)
        .env("JET_ICE_SELF_TEST", "1")
        .output()
        .expect("failed to run jet");

    assert_eq!(
        out.status.code(),
        Some(101),
        "an uncaught panic must still exit ICE (101)"
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("internal compiler error: ICE self-test triggered by JET_ICE_SELF_TEST"),
        "missing branded ICE first line:\n{stderr}"
    );
    assert!(
        stderr.contains("This is a bug in jet, NOT in your program. Please report it,"),
        "missing branded report body:\n{stderr}"
    );
    assert!(
        !stderr.contains("RUST_BACKTRACE"),
        "raw Rust panic hint leaked past the hook:\n{stderr}"
    );
    assert!(
        !stderr.contains("thread 'main' panicked at"),
        "raw Rust panic text leaked past the hook:\n{stderr}"
    );
}
