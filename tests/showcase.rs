//! Golden tests for E2-M17 showcase programs.
//! Fixed inputs and pinned outputs — permanent regression armor (I5).
//!
//! Showcases:
//!   1. jetgrep    — CLI tool (M7 streaming I/O, regex, tests)
//!   2. jsonfmt    — CLI tool (M9 data-format libs)
//!   3. wordfreq   — CLI tool (M7 streaming I/O)
//!   4. library    — library authoring (M6: traits, delegation, labels)
//!   5. lowlevel   — expert low-level tier (M13: @unsafe, Ptr<T>)
//!   6. freestanding — freestanding / cross-compile smoke (M15)
//!   7. http_service — HTTP service with tasks + channels (M10)

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct ShowcaseCase {
    name: &'static str,
    tool: &'static str,
    args: &'static [&'static str],
    /// When set, expect this exit code instead of success.
    exit_code: Option<i32>,
    /// When set, stderr must contain this substring (in addition to `.err.out` check).
    stderr_contains: Option<&'static str>,
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn run_showcase(root: &Path, jet: &Path, tool: &str, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(jet);
    cmd.arg("run").arg(root.join(tool)).current_dir(root);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.output().unwrap()
}

#[test]
fn showcase_tools_golden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    assert!(jet.exists(), "build the jet binary first (cargo build)");
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping showcase golden run");
        return;
    }

    let expected_dir = root.join("examples/showcase/expected");
    let cases = [
        // Showcase 0 — syntax canon (the compiling "what works today" surface)
        ShowcaseCase {
            name: "canon",
            tool: "examples/showcase/canon.jet",
            args: &[],
            exit_code: None,
            stderr_contains: None,
        },
        // Showcase 1 — CLI tool (jetgrep)
        ShowcaseCase {
            name: "jetgrep",
            tool: "examples/showcase/jetgrep.jet",
            args: &["-n", "the", "examples/showcase/fixtures/sample.txt"],
            exit_code: None,
            stderr_contains: None,
        },
        ShowcaseCase {
            name: "jetgrep_r",
            tool: "examples/showcase/jetgrep.jet",
            args: &["-r", "grep", "examples/showcase/fixtures"],
            exit_code: None,
            stderr_contains: None,
        },
        // Showcase 1 continued — CLI tool (jsonfmt)
        ShowcaseCase {
            name: "jsonfmt",
            tool: "examples/showcase/jsonfmt.jet",
            args: &["examples/showcase/fixtures/sample.json"],
            exit_code: None,
            stderr_contains: None,
        },
        ShowcaseCase {
            name: "jsonfmt_err",
            tool: "examples/showcase/jsonfmt.jet",
            args: &["examples/showcase/fixtures/bad.json"],
            exit_code: Some(1),
            stderr_contains: Some("line"),
        },
        // Showcase 1 continued — CLI tool (wordfreq)
        ShowcaseCase {
            name: "wordfreq",
            tool: "examples/showcase/wordfreq.jet",
            args: &["examples/showcase/fixtures"],
            exit_code: None,
            stderr_contains: None,
        },
        // Showcase 2 — HTTP service (M10: tasks, channels, net, HTTP)
        ShowcaseCase {
            name: "http_service",
            tool: "examples/showcase/http_service.jet",
            args: &[],
            exit_code: None,
            stderr_contains: None,
        },
        // Showcase 3 — library authoring (M6: traits, delegation, labels)
        ShowcaseCase {
            name: "library",
            tool: "examples/showcase/library.jet",
            args: &[],
            exit_code: None,
            stderr_contains: None,
        },
        // Showcase 5 — expert low-level tier (M13: @unsafe, Ptr<T>, @audit)
        ShowcaseCase {
            name: "lowlevel",
            tool: "examples/showcase/lowlevel.jet",
            args: &[],
            exit_code: None,
            stderr_contains: None,
        },
        // Showcase 6 — freestanding / cross-compile smoke (M15)
        ShowcaseCase {
            name: "freestanding",
            tool: "examples/showcase/freestanding.jet",
            args: &[],
            exit_code: None,
            stderr_contains: None,
        },
    ];

    for case in cases {
        let out = run_showcase(&root, &jet, case.tool, case.args);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);

        if let Some(code) = case.exit_code {
            assert_eq!(
                out.status.code(),
                Some(code),
                "exit code mismatch for showcase case {}",
                case.name
            );
            if let Some(needle) = case.stderr_contains {
                assert!(
                    stderr.contains(needle),
                    "stderr for {} should contain {:?}, got:\n{}",
                    case.name,
                    needle,
                    stderr
                );
            }
            let err_path = expected_dir.join(format!("{}.err.out", case.name));
            let expected_err = fs::read_to_string(&err_path).unwrap_or_else(|_| {
                panic!("missing examples/showcase/expected/{}.err.out", case.name)
            });
            assert_eq!(stderr, expected_err, "stderr mismatch for {}", case.name);
        } else {
            assert!(
                out.status.success(),
                "showcase case {} failed:\nstdout: {}\nstderr: {}",
                case.name,
                stdout,
                stderr
            );
            let out_path = expected_dir.join(format!("{}.out", case.name));
            let expected = fs::read_to_string(&out_path)
                .unwrap_or_else(|_| panic!("missing examples/showcase/expected/{}.out", case.name));
            assert_eq!(stdout, expected, "output mismatch for {}", case.name);
        }
    }
}
