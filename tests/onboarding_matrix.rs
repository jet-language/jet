//! Acceptance proof for the beginner terminal/editor state matrix.
//!
//! The CLI checks below use the built `jet` binary. The source assertions bind
//! the documented editor actions and the real PTY matrix to their production
//! seams, so this lane cannot pass with a docs-only or mock-only scaffold.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_else(|error| panic!("read {rel}: {error}"))
}

fn jet(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("jet {args:?} should start: {error}"))
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn terminal_editor_state_matrix() {
    // FEATURE_CLAIM: claim.tooling-cli / cli-terminal-matrix
    let guide = read("docs/first-hour.md");
    for state in [
        "## Terminal and editor state matrix",
        "| Install ready |",
        "| Scaffolded |",
        "| Valid edit |",
        "| Invalid edit |",
        "| Missing entry |",
        "| Ambiguous project |",
        "| Legacy layout |",
        "| Offline or unsupported host |",
        "| Learn next |",
    ] {
        assert!(
            guide.contains(state),
            "first-hour matrix lost state {state}"
        );
    }
    assert!(guide.contains("jet ?") && guide.contains("jet ? run"));
    assert!(guide.contains("jet check run.jet") && guide.contains("jet test run.jet"));
    let diagnostics = read("docs/spec/diagnostics.md");
    assert!(diagnostics.contains("canonical `run.jet` is missing"));
    let example = read("examples/features/basics/first_hour.jet");
    assert!(example.contains("First-hour tour"));
    assert!(root()
        .join("examples/features/expected/basics/first_hour.out")
        .is_file());
    assert!(read("tests/golden.rs").contains("examples/features"));

    let vscode = read("editors/vscode/extension.js");
    assert!(
        vscode.contains("[\"run\", file]") && vscode.contains("[\"test\", file]"),
        "VS Code code lenses must invoke the production jet binary"
    );
    for docs in ["editors/vscode/README.md", "editors/zed/README.md"] {
        let editor_docs = read(docs);
        assert!(
            editor_docs.contains("run/test code lenses")
                || (editor_docs.contains("Run File") && editor_docs.contains("Test File")),
            "editor docs lost the run/test entry path: {docs}"
        );
    }
    let lsp = read("tests/lsp.rs");
    assert!(
        lsp.contains("fn lsp_document_links_and_code_lenses()")
            && lsp.contains("jet.runFile")
            && lsp.contains("jet.testFile"),
        "LSP editor acceptance must keep both code lenses"
    );
    let terminal_matrix = read("tests/help_pty.rs");
    for proof in [
        "fn enter_expands_then_guides_without_hooks_instead_of_pasting()",
        "fn shell_prefill_mode_keeps_palette_on_tty_while_stdout_is_captured()",
        "fn explicit_color_law_reaches_interactive_renderer()",
        "fn live_narrow_resize_uses_actual_terminal_size()",
    ] {
        assert!(terminal_matrix.contains(proof), "PTY matrix lost {proof}");
    }

    let root = common::Scratch::new("onboarding-state-matrix");
    let created = jet(&["new", "hello"], &root.path);
    assert!(
        created.status.success(),
        "jet new failed:\n{}",
        stderr(&created)
    );
    let project = root.join("hello");
    for file in ["package.jet", "run.jet", ".gitignore"] {
        assert!(project.join(file).is_file(), "scaffold is missing {file}");
    }

    let palette = jet(&["?", "--color=never"], &project);
    assert!(
        palette.status.success(),
        "non-TTY help failed:\n{}",
        stderr(&palette)
    );
    assert!(stdout(&palette).contains("Build & Run"));
    assert!(
        !stdout(&palette).contains('\u{1b}'),
        "NO_COLOR leaked into help"
    );
    let search = jet(&["?", "run", "--color=never"], &project);
    assert!(
        search.status.success(),
        "non-TTY help search failed:\n{}",
        stderr(&search)
    );
    assert!(stdout(&search).contains("jet run"));
    assert!(
        !stdout(&search).contains('\u{1b}'),
        "NO_COLOR leaked into search"
    );

    let first_run = jet(&["run"], &project);
    assert!(
        first_run.status.success(),
        "bare jet run failed:\n{}",
        stderr(&first_run)
    );
    assert_eq!(stdout(&first_run), "hello, world\n");
    let explicit_run = jet(&["run", "run.jet"], &project);
    assert!(
        explicit_run.status.success(),
        "explicit jet run failed:\n{}",
        stderr(&explicit_run)
    );

    fs::rename(project.join("run.jet"), project.join("run.jet.retired"))
        .expect("hide the default entry for recovery");
    let missing = jet(&["run"], &project);
    assert!(!missing.status.success(), "missing entry unexpectedly ran");
    assert!(stderr(&missing).contains("E2105"));
    assert!(
        stderr(&missing).contains("create `run.jet`")
            && stderr(&missing).contains("jet run <file.jet>"),
        "missing entry lost its recovery path:\n{}",
        stderr(&missing)
    );
    fs::rename(project.join("run.jet.retired"), project.join("run.jet"))
        .expect("restore the default entry after recovery proof");

    let source_path = project.join("run.jet");
    let source = fs::read_to_string(&source_path).expect("read scaffold entry");
    let edited = source.replace("hello, world", "hello from Jet");
    fs::write(&source_path, &edited).expect("write edited entry");
    for args in [["check", "run.jet"], ["test", "run.jet"]] {
        let result = jet(&args, &project);
        assert!(
            result.status.success(),
            "jet {args:?} failed:\n{}",
            stderr(&result)
        );
    }
    let edited_run = jet(&["run"], &project);
    assert!(
        edited_run.status.success(),
        "edited jet run failed:\n{}",
        stderr(&edited_run)
    );
    assert_eq!(stdout(&edited_run), "hello from Jet\n");

    fs::write(&source_path, edited.replace("print", "pirnt")).expect("write invalid entry");
    let invalid = jet(&["check", "run.jet"], &project);
    assert!(
        !invalid.status.success(),
        "invalid source unexpectedly passed check"
    );
    assert!(
        stderr(&invalid).contains("E0102"),
        "invalid edit lost E0102:\n{}",
        stderr(&invalid)
    );
    assert!(
        stderr(&invalid).contains("Fix:"),
        "invalid edit lost Fix:\n{}",
        stderr(&invalid)
    );

    fs::write(&source_path, edited).expect("restore edited entry");
    let recovered = jet(&["run"], &project);
    assert!(
        recovered.status.success(),
        "recovery run failed:\n{}",
        stderr(&recovered)
    );
    assert_eq!(stdout(&recovered), "hello from Jet\n");
}
