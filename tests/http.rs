use std::fs;
use std::path::{Path, PathBuf};

use jet::Interpreter::dev_iteration;
use jet_foundation::JitBackend::RunOutcome;

mod common;

struct Output {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

fn on_large_stack(work: impl FnOnce() + Send) {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn_scoped(scope, work)
            .unwrap()
            .join()
            .unwrap();
    });
}

fn run_dev(path: &Path, use_interpreter: bool) -> Output {
    let shown = path.to_string_lossy().into_owned();
    match dev_iteration(&shown, false, use_interpreter) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Output {
            stdout,
            stderr,
            exit_code,
        },
        RunOutcome::Problems(diags) => {
            panic!("{} failed: {diags:?}", path.display())
        }
    }
}

fn example_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/net/http_readiness.jet")
}

#[test]
fn jit_http_readiness_uses_shared_scheduler_poller() {
    let runtime = include_str!("../crates/jet-jit/src/net_http_rt.rs");
    let scheduler = include_str!("../crates/jet-codegen/src/Prelude/Scheduler.rs");
    for symbol in [
        "jet_scheduler_io_wait",
        "jet_scheduler_tcp_listener_io_wait",
        "jet_scheduler_tcp_stream_ready_wait",
        "jet_scheduler_udp_io_wait",
    ] {
        assert!(runtime.contains(symbol), "JIT runtime does not import {symbol}");
        assert!(
            scheduler.contains(&format!("pub fn {symbol}")),
            "Prelude scheduler does not define {symbol}"
        );
        assert!(
            !runtime.contains(&format!("fn {symbol}")),
            "JIT runtime still defines local {symbol}"
        );
    }
    assert!(!runtime.contains("fn jet_scheduler_tcp_stream_io_wait"));
    assert!(!runtime.contains("thread::sleep"));
}

#[test]
fn http_readiness_matches_interpreter_default_jit_and_aot() {
    if !common::have_rustc() {
        return;
    }
    if !jet_jit::cranelift_host_supported() {
        if std::env::var("JET_REQUIRE_CRANELIFT_HOST").as_deref() == Ok("1") {
            panic!("Cranelift host required but unavailable");
        }
        return;
    }

    on_large_stack(|| {
        let path = example_path();
        let source = fs::read_to_string(&path).unwrap();
        let expected = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("examples/features/expected/net/http_readiness.out"),
        )
        .unwrap();

        let interpreted = run_dev(&path, true);
        assert_eq!(interpreted.stdout, expected, "interpreter stdout");
        assert_eq!(interpreted.stderr, "", "interpreter stderr");
        assert_eq!(interpreted.exit_code, 0, "interpreter exit code");

        jet_jit::reset_jit_trace_for_test();
        let default = run_dev(&path, false);
        assert_eq!(default.stdout, expected, "default stdout");
        assert_eq!(default.stderr, "", "default stderr");
        assert_eq!(default.exit_code, 0, "default exit code");
        assert!(jet_jit::jit_executed_for_test(), "default run did not execute JIT");
        assert!(!jet_jit::deopt_invoked_for_test(), "default run deopted");
        assert!(!jet_jit::fallback_invoked_for_test(), "default run fell back");

        let (code, aot_stdout, aot_stderr) =
            common::build_and_run("jet_http_readiness", "http_readiness", &source);
        assert_eq!(code, 0, "AOT exit code: {aot_stderr}");
        assert_eq!(aot_stdout, expected, "AOT stdout");
        assert_eq!(aot_stderr, "", "AOT stderr");
    });
}
