//! Card #1660 (D-ONCE-*): the E3003 deadline-exceeded text was hand-typed in
//! four places (`Prelude/TaskGroup.rs`, `Prelude/CoreLib/Top/MathRandomTime.rs`,
//! `SchedulerHost.rs` test, `jet-jit/Concurrency.rs`) with two incompatible
//! `Why:`/`Fix:` spacings, and the E3001 unhandled-crypto text (emitted by
//! `Codegen/Items.rs`) used a third. All three tiers now materialize the
//! E3003 row from the one renderer, `TaskGroup::JetTaskDeadline::render`
//! (`jet_task_deadline(wait_kind).render()`), and both codes share the same
//! ` Why: … / Fix: …` spacing.
//!
//! Two guards:
//!   - `e3003_why_text_lives_only_in_the_one_renderer`: fails if the E3003
//!     `why` sentence reappears hand-typed anywhere under `crates/` outside
//!     `Prelude/TaskGroup.rs`.
//!   - `e3003_and_e3001_spell_why_and_fix_identically`: both codes' rendered
//!     `Why:`/`Fix:` lines use the same one-space-then-label convention.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

const E3003_HOME: &str = "crates/jet-codegen/src/Prelude/TaskGroup.rs";
const E3003_WHY: &str = "this wait point observed the task context deadline";

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
fn e3003_why_text_lives_only_in_the_one_renderer() {
    let root = root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);

    let mut homes = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).unwrap_or_default();
        if text.contains(E3003_WHY) {
            homes.push(file.strip_prefix(&root).unwrap_or(file).display().to_string());
        }
    }

    assert_eq!(
        homes,
        vec![E3003_HOME.to_string()],
        "E3003 'why' text must live only in {E3003_HOME}, found in: {homes:?}"
    );
}

#[test]
fn e3003_and_e3001_spell_why_and_fix_identically() {
    let root = root();

    let task_group = fs::read_to_string(root.join(E3003_HOME)).unwrap();
    assert!(
        task_group.contains("\\n Why: {}\\n Fix: {}"),
        "E3003 renderer must use ' Why: … / Fix: …' spacing (one leading space): {task_group}"
    );

    let items_rs = fs::read_to_string(root.join("crates/jet-codegen/src/Codegen/Items.rs")).unwrap();
    assert!(
        items_rs.contains("\\\" Why: {{}}\\\"") && items_rs.contains("\\\" Fix: handle the CryptoError"),
        "E3001 unhandled-crypto emission must keep the same ' Why: … / Fix: …' spacing as E3003"
    );
}
