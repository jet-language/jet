//! Card #1747: the `#Context(deadline: …)` clock, budget, and
//! `JetDeadlineGuard` now live in one file, `Prelude/Deadline.rs`, included
//! by both the AOT emission list (`Codegen/mod.rs`) and the in-crate
//! `jet_codegen::scheduler` module that the Cranelift JIT and `net_http_rt`
//! compile. `MathRandomTime.rs`, `SchedulerHost.rs`, and `net_http_rt.rs`
//! used to hand-type their own copy of the clock function and the
//! thread-local deadline cell; this guard fails if a second copy reappears.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

const DEADLINE_HOME: &str = "crates/jet-codegen/src/Prelude/Deadline.rs";

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

fn homes_for(needle: &str) -> Vec<String> {
    let root = root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);
    let mut homes = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).unwrap_or_default();
        if text.contains(needle) {
            homes.push(file.strip_prefix(&root).unwrap_or(file).display().to_string());
        }
    }
    homes
}

#[test]
fn deadline_clock_fn_lives_only_in_the_one_renderer() {
    let homes = homes_for("fn jet_std_time_now");
    assert_eq!(
        homes,
        vec![DEADLINE_HOME.to_string()],
        "jet_std_time_now must be defined only in {DEADLINE_HOME}, found in: {homes:?}"
    );
}

#[test]
fn deadline_thread_local_lives_only_in_the_one_renderer() {
    let homes = homes_for("static JET_CTX_DEADLINE_MS");
    assert_eq!(
        homes,
        vec![DEADLINE_HOME.to_string()],
        "JET_CTX_DEADLINE_MS must be declared only in {DEADLINE_HOME}, found in: {homes:?}"
    );
}
