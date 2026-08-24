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

fn example_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("examples/features/net/{name}.jet"))
}

#[test]
fn jit_http_readiness_uses_shared_scheduler_poller() {
    let runtime = include_str!("../crates/jet-jit/src/net_http_rt.rs");
    let scheduler = include_str!("../crates/jet-codegen/src/Prelude/Scheduler.rs");
    let net = include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs");
    for symbol in [
        "jet_scheduler_io_wait",
        "jet_scheduler_tcp_listener_io_wait",
        "jet_scheduler_tcp_stream_ready_wait",
        "jet_scheduler_udp_io_wait",
    ] {
        assert!(
            runtime.contains(symbol),
            "JIT runtime does not import {symbol}"
        );
        assert!(
            scheduler.contains(&format!("pub fn {symbol}")),
            "Prelude scheduler does not define {symbol}"
        );
        assert!(
            !runtime.contains(&format!("fn {symbol}")),
            "JIT runtime still defines local {symbol}"
        );
    }
    #[cfg(unix)]
    assert!(
        net.contains("jet_scheduler_tcp_stream_ready_wait(stream, read, write, operation)"),
        "TCP stream waits must use the shared raw-fd poller"
    );
    assert!(!runtime.contains("fn jet_scheduler_tcp_stream_io_wait"));
    assert!(!runtime.contains("thread::sleep"));
}

#[test]
fn resident_http_stop_uses_the_shared_runtime_boundary() {
    let runtime = include_str!("../crates/jet-jit/src/net_http_rt.rs");
    assert!(
        runtime.contains("runtime_host::runtime_stop_unwind(\"E3001\""),
        "resident HTTP failures must enter the shared runtime report boundary"
    );
    assert!(
        !runtime.contains("std::process::exit"),
        "resident HTTP helpers must not terminate the host process"
    );
}

fn assert_http_example_matches_all_tiers(name: &str) {
    let path = example_path(name);
    let source = fs::read_to_string(&path).unwrap();
    let expected = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("examples/features/expected/net/{name}.out")),
    )
    .unwrap();

    let interpreted = run_dev(&path, true);
    assert_eq!(interpreted.stdout, expected, "{name} interpreter stdout");
    assert_eq!(interpreted.stderr, "", "{name} interpreter stderr");
    assert_eq!(interpreted.exit_code, 0, "{name} interpreter exit code");

    jet_jit::reset_jit_trace_for_test();
    let default = run_dev(&path, false);
    assert_eq!(default.stdout, expected, "{name} default stdout");
    assert_eq!(default.stderr, "", "{name} default stderr");
    assert_eq!(default.exit_code, 0, "{name} default exit code");
    assert!(
        jet_jit::jit_executed_for_test(),
        "{name} default run did not execute JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test(),
        "{name} default run deopted"
    );
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "{name} default run fell back"
    );

    let (code, aot_stdout, aot_stderr) =
        common::build_and_run(&format!("jet_{name}"), name, &source);
    assert_eq!(code, 0, "{name} AOT exit code: {aot_stderr}");
    assert_eq!(aot_stdout, expected, "{name} AOT stdout");
    assert_eq!(aot_stderr, "", "{name} AOT stderr");
}

#[test]
fn http_examples_match_interpreter_default_jit_and_aot() {
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
        assert_http_example_matches_all_tiers("http_readiness");
        assert_http_example_matches_all_tiers("http_client");
    });
}
