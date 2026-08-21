mod common;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use jet_foundation::JetTrace::{jettrace_artifact, trace_id, verify_jettrace};
use jet_foundation::PerformanceBudget::CanonicalJson;

static SELF_ATTACH_LOCK: Mutex<()> = Mutex::new(());

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn run_jet(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(jet())
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("jet {:?} failed to launch: {e}", args))
}

fn temp_workspace() -> PathBuf {
    let root = common::unique_tmp("jet-perf");
    fs::create_dir_all(&root).unwrap();
    root
}
include!("jet_perf_trace_parts/views.rs");
include!("jet_perf_trace_parts/base.rs");
