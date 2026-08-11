//! Card #1751: the 80x24 terminal default is one fact,
//! `crates/jet-codegen/src/Prelude/TerminalDefault.rs`'s
//! `JET_TERMINAL_DEFAULT_COLS`/`JET_TERMINAL_DEFAULT_ROWS`. `TerminalPolicy::default`
//! (Prelude/CoreLib/JetStd/CommonTypes.rs) and `PtyConfig::default`
//! (Prelude/CoreLib/ProcessPty.rs, dual-compiled for the resident JIT host)
//! both read it instead of hand-typing `80`/`24`. This guard fails if a
//! second hand-typed `cols: 80` / `rows: 24` pair reappears anywhere under
//! `crates/` outside the one home.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

const TERMINAL_DEFAULT_HOME: &str = "crates/jet-codegen/src/Prelude/TerminalDefault.rs";

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
fn no_hand_typed_80x24_terminal_default_outside_the_one_home() {
    let root = root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);

    let mut violations = Vec::new();
    for file in files {
        let rel = file.strip_prefix(&root).unwrap_or(&file).display().to_string();
        if rel == TERMINAL_DEFAULT_HOME {
            continue;
        }
        let text = fs::read_to_string(&file).unwrap_or_default();
        for (n, line) in text.lines().enumerate() {
            let has_cols_80 = line.contains("cols: 80") || line.contains("cols:80");
            let has_rows_24 = line.contains("rows: 24") || line.contains("rows:24");
            if has_cols_80 || has_rows_24 {
                violations.push(format!("{rel}:{}: {}", n + 1, line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "hand-typed 80x24 terminal default outside the one home ({TERMINAL_DEFAULT_HOME}):\n{}",
        violations.join("\n")
    );
}

#[test]
fn common_types_and_process_pty_read_the_one_home() {
    let root = root();
    let common_types = fs::read_to_string(
        root.join("crates/jet-codegen/src/Prelude/CoreLib/JetStd/CommonTypes.rs"),
    )
    .unwrap();
    assert!(
        common_types.contains("super::terminal_default::JET_TERMINAL_DEFAULT_COLS")
            && common_types.contains("super::terminal_default::JET_TERMINAL_DEFAULT_ROWS"),
        "TerminalPolicy::default must read Prelude/TerminalDefault.rs, not a literal"
    );
    let process_pty =
        fs::read_to_string(root.join("crates/jet-codegen/src/Prelude/CoreLib/ProcessPty.rs"))
            .unwrap();
    assert!(
        process_pty.contains("super::terminal_default::JET_TERMINAL_DEFAULT_COLS")
            && process_pty.contains("super::terminal_default::JET_TERMINAL_DEFAULT_ROWS"),
        "PtyConfig::default must read Prelude/TerminalDefault.rs, not a literal"
    );
    let jit_process =
        fs::read_to_string(root.join("crates/jet-jit/src/Process.rs")).unwrap();
    assert!(
        jit_process.contains("PtyConfig::default()"),
        "jet_jit_process_spec_terminal must build PtyConfig::default(), not a hand-typed literal"
    );
}
