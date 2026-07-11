//! Tower #126: emitted AOT programs must SHIP and SELECT the native readiness
//! backend (epoll on Linux / kqueue on BSD-Apple / IOCP on Windows), not the portable
//! poll fallback — while staying I1-clean once the vetted native region is
//! excluded from the `unsafe` scan (the same contract `tests/golden.rs` enforces).

const TASK_PROGRAM: &str = r#"
use core.tasks as tasks

fn run() {
    (sender, ch) :: tasks.channel<Int>(1)
    sender.send(1)
    print(ch.receive() ?? panic("channel closed"))
}
"#;

/// Drop the inclusive span between two markers — mirrors golden.rs's I1 scan.
fn strip_region(src: &str, begin: &str, end: &str) -> String {
    match (src.find(begin), src.find(end)) {
        (Some(b), Some(e)) if e >= b => {
            let mut s = src[..b].to_string();
            s.push_str(&src[e + end.len()..]);
            s
        }
        _ => src.to_string(),
    }
}

/// Brace-match a `mod NAME { … }` block out (the pre-existing vetted `jet_mem`
/// arena helper carries its own D-LL1 `unsafe`; golden.rs strips it the same way).
fn strip_mod(src: &str, name: &str) -> String {
    let Some(start) = src.find(&format!("mod {}", name)) else {
        return src.to_string();
    };
    let bytes = src.as_bytes();
    let (mut depth, mut i, mut end, mut seen) = (0usize, start, src.len(), false);
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                seen = true;
            }
            b'}' => {
                depth -= 1;
                if seen && depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let mut s = src[..start].to_string();
    s.push_str(&src[end..]);
    s
}

#[test]
fn emitted_scheduler_ships_native_readiness_backend() {
    let out = jet::compile(TASK_PROGRAM).expect("task program should compile");
    let rust = &out.rust;

    // The native epoll/kqueue syscall paths are present in the emitted program,
    // not stripped out.
    assert!(
        rust.contains("epoll_wait") && rust.contains("epoll_create1"),
        "emitted program must ship the native epoll backend (Tower #126)"
    );
    assert!(
        rust.contains("fn kqueue()"),
        "emitted program must ship the kqueue backend behind cfg"
    );
    assert!(
        rust.contains("CreateIoCompletionPort")
            && rust.contains("GetQueuedCompletionStatus")
            && rust.contains("WSARecv")
            && rust.contains("CancelIoEx")
            && rust.contains("PostQueuedCompletionStatus"),
        "emitted program must ship real IOCP registration, completion, wake, and cancellation"
    );
    assert!(
        rust.contains("enum IoBackendState")
            && rust.contains("IoBackendState::Failed")
            && rust.contains("scheduler IOCP completion port failed")
            && rust.contains("drain_deadline")
            && rust.contains("METRIC_IO_PORT_CLOSED")
            && rust.contains("iocp_shutdown_done"),
        "emitted IOCP backend must publish terminal failure and reject later waits"
    );
    assert!(
        !rust.contains("IOCP path: fall back to portable poll"),
        "Windows native path must never route through portable readiness polling"
    );

    // The JIT-only Cargo feature gate must be rewritten at emit time so a bare
    // `rustc` build (no Cargo features) still selects native purely on target_os.
    assert!(
        !rust.contains("feature = \"jet_native_io\""),
        "emitted program must not gate native IO behind a Cargo feature it never sets"
    );

    // Backend is actually SELECTED, not merely present: the io_backend reporter
    // returns the native name on the native targets.
    assert!(
        rust.contains("return \"epoll\";")
            && rust.contains("return \"kqueue\";")
            && rust.contains("return \"iocp\";"),
        "emitted io_backend must select native backends per target_os"
    );
}

#[test]
fn emitted_scheduler_native_region_is_the_only_unsafe() {
    let out = jet::compile(TASK_PROGRAM).expect("task program should compile");
    let scanned = strip_region(
        &out.rust,
        "// jet:scheduler-native-begin",
        "// jet:scheduler-native-end",
    );
    // Pre-existing vetted internals carry their own audited `unsafe`; golden.rs
    // strips the same set before its I1 scan.
    let mut scanned = scanned;
    for name in [
        "jet_mem",
        "jet_txn",
        "jet_term_unix",
        "jet_term_windows",
        "jet_os_unix",
        "jet_atomic_windows",
        "jet_gtk",
    ] {
        scanned = strip_mod(&scanned, name);
    }
    assert!(
        scanned.contains("epoll_wait") == false,
        "the native region should have been excised for the scan"
    );
    assert!(
        !scanned.contains("unsafe"),
        "I1: all emitted `unsafe` must live inside the vetted jet:scheduler-native region"
    );
}
