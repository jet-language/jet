#[test]
fn perf_static_api_lowers_to_core_helpers() {
    let out = compile_temp(
        "perf_static.jet",
        r#"
fn run() => () ? {
    print(Perf.default_fidelity())
    Perf.override_fidelity(0.25)?
    print(Perf.fidelity())
    Perf.reset_fidelity()
}
"#,
    );
    assert!(out.rust.contains("jet_perf_default_fidelity()"));
    assert!(out.rust.contains("jet_perf_override_fidelity(0.25"));
    assert!(out.rust.contains("jet_perf_fidelity()"));
    assert!(out.rust.contains("jet_perf_reset_fidelity()"));
}

#[test]
fn perf_set_fidelity_alias_is_not_exported() {
    let src = r#"
use core.perf as perf

fn run() => () ? {
    perf.set_fidelity(0.25)?
}
"#;
    let dir = std::env::temp_dir().join(format!("jet_corelib_perf_alias_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("perf_alias.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let diags = jet::compile_with_path(src, &shown).expect_err("set_fidelity alias must not exist");
    let rendered = jet::render_diagnostics(&shown, src, &diags);
    assert!(
        rendered.contains("set_fidelity"),
        "diagnostic should name retired alias, got:\n{rendered}"
    );
    assert!(
        rendered.contains("has no item"),
        "diagnostic should reject retired alias, got:\n{rendered}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn perf_override_is_range_checked_and_resettable() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping perf runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_perf_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "perf_runtime",
        r#"
use core.perf as perf

fn run() => () ? {
    print(perf.default_fidelity())
    perf.override_fidelity(0.25)?
    print(perf.fidelity())
    perf.reset_fidelity()
    print(perf.fidelity())
    perf.override_fidelity(1.25)?
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 1, "out-of-range override should fail");
    assert_eq!(stdout, "1.0\n0.25\n1.0\n");
    assert!(
        stderr.contains("core.perf.Perf.override_fidelity needs 0.0 through 1.0"),
        "range error should be in Jet runtime terms, got {stderr:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn option_zip_and_lift2_combinators() {
    // D-HOLE1: `.zip`/`Option.lift2` — both present -> a present result; either
    // absent -> `None`. No general "hole" type; these are plain library combinators
    // on `T?`.
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping option combinator test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_option_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
fn missing_float() => Float? = None
fn multiply_options(x: Float, y: Float) => Float {
    return x * y
}
fn choose_multiplier() => fn(Float, Float) => Float {
    print("choose")
    return multiply_options
}
fn run() {
    both_a :: Val(2.0)
    both_b :: Val(5.0)
    print(both_a.zip(both_b).map((pair) => pair.a * pair.b))
    print(Option.lift2((x, y) => x * y, both_a, both_b))
    print(Option.lift2(multiply_options, both_a, both_b))
    multiplier :: multiply_options
    print(Option.lift2(multiplier, both_a, both_b))
    print(Option.lift2(choose_multiplier(), both_a, both_b))

    a_only :: Val(2.0)
    b_missing :: missing_float()
    print(a_only.zip(b_missing).map((pair) => pair.a * pair.b))
    print(Option.lift2((x, y) => x * y, a_only, b_missing))
    print(Option.lift2(choose_multiplier(), a_only, b_missing))

    both_missing_a :: missing_float()
    both_missing_b :: missing_float()
    print(both_missing_a.zip(both_missing_b).map((pair) => pair.a * pair.b))
    print(Option.lift2((x, y) => x * y, both_missing_a, both_missing_b))
}
"#;
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "option_combinators",
        source,
        &[],
        None,
    );
    assert_eq!(code, 0, "option combinator fixture failed: {stderr}");
    assert_eq!(
        stdout,
        "10.0\n10.0\n10.0\n10.0\nchoose\n10.0\nnull\nnull\nnull\nnull\nnull\n",
        "unexpected option combinator output: {stdout}"
    );
    let dev_path = dir.join("option_combinators.jet");
    fs::write(&dev_path, source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, true) {
        jet::Interpreter::RunOutcome::Ran {
            stdout: interpreted_stdout,
            stderr: interpreted_stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "forced interpreter failed: {interpreted_stderr}");
            assert_eq!(interpreted_stderr, "");
            assert_eq!(interpreted_stdout, stdout, "forced interpreter output drifted");
        }
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("forced interpreter rejected option combinator fixture: {diagnostics:?}");
        }
    }
    jet_jit::reset_jit_trace_for_test();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout: resident_stdout,
            stderr: resident_stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "resident JIT failed: {resident_stderr}");
            assert_eq!(resident_stderr, "");
            assert_eq!(resident_stdout, stdout, "resident JIT output drifted");
            assert!(
                jet_jit::jit_executed_for_test(),
                "option combinator fixture must execute resident JIT"
            );
            assert!(
                !jet_jit::deopt_invoked_for_test(),
                "option combinator fixture must not deopt"
            );
            assert!(
                !jet_jit::fallback_invoked_for_test(),
                "option combinator fixture must not fall back"
            );
        }
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("resident JIT rejected option combinator fixture: {diagnostics:?}");
        }
    }

    let web_source = r#"
#Target(Web)
#Target(JS)
fn add(x: Int, y: Int) => Int { return x + y }

#Target(JS)
fn choose() => fn(Int, Int) => Int {
    print("choose")
    return add
}

#Target(JS)
fn run() {
    if Option.lift2(add, Val(2), Val(3)) == .Val(value) { print(value) }
    local :: add
    if Option.lift2(local, Val(2), Val(3)) == .Val(value) { print(value) }
    if Option.lift2(choose(), Val(2), Val(3)) == .Val(value) { print(value) }
    if Option.lift2(choose(), Val(2), None) == .None { print("none") }
}
"#;
    if let Some(web_stdout) = run_web_js_source(&dir, "option_combinators_web", web_source) {
        assert_eq!(web_stdout, "5\n5\nchoose\n5\nnone\n");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn event_scope_subscribe_once_priority_and_hook_run() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping event runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_event_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "event_runtime",
        r#"
use core.event as event

fn run() {
    scope :: event.scope()
    ev :: event.with_policy<Int>(event.policy_sync())
    sub :: ev.on(scope, (n) => { print("low {n}") })
    ev.on_priority(scope, 10, (n) => { print("high {n}") })
    ev.once(scope, (n) => { print("once {n}") })
    print(ev.emit(1).summary())
    sub.unsubscribe()
    print(ev.emit(2).summary())
    print(scope.active_count())

    hook :: event.hook<Int, String>("base")
    hook.on(scope, (n) => "seen {n}")
    print(hook.run(7, "fallback"))
    scope.cancel()
    print(hook.run(8, "fallback"))
}

"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "event runtime failed: {stderr}");
    assert_eq!(
        stdout,
        "high 1\nlow 1\nonce 1\nevent delivered=3 queued=0 dropped=0\nhigh 2\nevent delivered=1 queued=0 dropped=0\n1\nseen 7\nfallback\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn async_event_scheduler_dispatch_and_invalid_capacity() {
    let have_rustc = common::have_rustc();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_corelib_async_event_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "async_event_scheduler",
        r#"
use core.event as event
use core.tasks as tasks

enum LocalState { Closed }

fn run() {
    local :: LocalState.Closed
    print("local={local == .Closed}")
    bad :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 0, overflow: .Block }, .Collect)
    if bad == {
        .Ok(_) -> print("bad accepted")
        .Err(_) -> print("invalid capacity")
    }
    scope :: event.scope()
    ev :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    (started_tx, started_rx) :: tasks.channel<Int>()
    (release_tx, release_rx) :: tasks.channel<Int>()
    ev.on(scope, (n: Int) => {
        started_tx.send(~n)
        released :: release_rx.receive() ?? panic("release")
    })
    first :: ev.emit_async(1)
    started_first :: started_rx.receive() ?? panic("started")
    second :: ev.emit_async(2)
    third :: ev.emit_async(3)
    print("queued={ev.queued_count()} running={ev.running_count()} blocked={ev.blocked_count()}")
    ev.close()
    release_tx.send(1)
    started_second :: started_rx.receive() ?? panic("second started")
    release_tx.send(2)
    first_report :: first.join()
    second_report :: second.join()
    third_report :: third.join()
    print("delivered={first_report.delivered_handlers() + second_report.delivered_handlers()}")
    print("delivered state={first_report.state() == .Delivered}")
    print("closed={!third_report.accepted() && third_report.state() == .Closed}")
    print(third_report.trace().summary())
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "async event runtime failed: {stderr}");
    assert_eq!(stdout, "local=true\ninvalid capacity\nqueued=1 running=1 blocked=1\ndelivered=2\ndelivered state=true\nclosed=true\npending -> terminal:Closed\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn async_event_overflow_and_failure_policies() {
    let have_rustc = common::have_rustc();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_corelib_async_event_policies_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "async_event_policies",
        r#"
use core.event as event
use core.tasks as tasks

fn panic_log_handler(n: Int) => () ? String {
    panic("log boom")
    return .Err("unreachable")
}

fn panic_ignore_handler(n: Int) => () ? String {
    panic("ignore boom")
    return .Err("unreachable")
}

fn run() {
    newest_scope :: event.scope()
    newest :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .DropNewest }, .Collect) ?? panic("policy")
    (newest_started_tx, newest_started_rx) :: tasks.channel<Int>()
    (newest_release_tx, newest_release_rx) :: tasks.channel<Int>()
    newest.on(newest_scope, (n: Int) => {
        newest_started_tx.send(~n)
        released_newest :: newest_release_rx.receive() ?? panic("release")
    })
    newest_first :: newest.emit_async(1)
    newest_started_first :: newest_started_rx.receive() ?? panic("started")
    newest_second :: newest.emit_async(2)
    newest_third :: newest.emit_async(3)
    newest_report :: newest_third.join()
    print("newest={!newest_report.accepted() && newest_report.state() == .DroppedNewest}")
    newest_release_tx.send(1)
    newest_started_second :: newest_started_rx.receive() ?? panic("second")
    newest_release_tx.send(2)
    newest_first.join()
    newest_second.join()

    oldest_scope :: event.scope()
    oldest :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .DropOldest }, .Collect) ?? panic("policy")
    (oldest_started_tx, oldest_started_rx) :: tasks.channel<Int>()
    (oldest_release_tx, oldest_release_rx) :: tasks.channel<Int>()
    oldest.on(oldest_scope, (n: Int) => {
        oldest_started_tx.send(~n)
        released_oldest :: oldest_release_rx.receive() ?? panic("release")
    })
    oldest_first :: oldest.emit_async(1)
    oldest_started_first :: oldest_started_rx.receive() ?? panic("started")
    oldest_evicted :: oldest.emit_async(2)
    oldest_third :: oldest.emit_async(3)
    oldest_report :: oldest_evicted.join()
    print("oldest={oldest_report.accepted() && oldest_report.state() == .DroppedOldest}")
    oldest_release_tx.send(1)
    oldest_started_third :: oldest_started_rx.receive() ?? panic("third")
    oldest_release_tx.send(3)
    oldest_first.join()
    oldest_third.join()

    once_scope :: event.scope()
    once_event :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 2, overflow: .Block }, .Collect) ?? panic("policy")
    (once_started_tx, once_started_rx) :: tasks.channel<Int>()
    (once_release_tx, once_release_rx) :: tasks.channel<Int>()
    once_event.on_priority(once_scope, 10, (n: Int) => {
        if n == 1 {
            once_started_tx.send(~n)
            released_once :: once_release_rx.receive() ?? panic("release")
        }
    })
    once_event.once(once_scope, (n: Int) => {})
    once_first :: once_event.emit_async(1)
    once_started :: once_started_rx.receive() ?? panic("started")
    once_second :: once_event.emit_async(2)
    once_release_tx.send(1)
    once_first_report :: once_first.join()
    once_second_report :: once_second.join()
    print("once first={once_first_report.delivered_handlers()} second={once_second_report.delivered_handlers()}")

    failure_scope :: event.scope()
    collect :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    collect.on_priority(failure_scope, 10, (n: Int) => .Err("high"))
    collect.on_priority(failure_scope, 0, (n: Int) => .Err("low"))
    collected :: collect.emit_async(1).join()
    print("collect={collected.state() == .HandlerFailed} handlers={collected.delivered_handlers()} failures={collected.failures().len()}")
    print(collected.trace().summary())

    stop :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .StopFirst) ?? panic("policy")
    stop.on_priority(failure_scope, 10, (n: Int) => .Err("first"))
    stop.on_priority(failure_scope, 0, (n: Int) => {})
    stopped :: stop.emit_async(1).join()
    print("stop={stopped.state() == .HandlerFailed} handlers={stopped.delivered_handlers()} failures={stopped.failures().len()}")

    log_errors :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Log) ?? panic("policy")
    log_errors.on_priority(failure_scope, 10, (n: Int) => .Err("logged secret"))
    log_errors.on_priority(failure_scope, 0, (n: Int) => {})
    logged_error :: log_errors.emit_async(1).join()
    print("log error={logged_error.state() == .Delivered} handlers={logged_error.delivered_handlers()} failures={logged_error.failures().len()} traced={logged_error.trace().summary().contains("failed")}")

    ignore_errors :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Ignore) ?? panic("policy")
    ignore_errors.on_priority(failure_scope, 10, (n: Int) => .Err("ignored secret"))
    ignore_errors.on_priority(failure_scope, 0, (n: Int) => {})
    ignored_error :: ignore_errors.emit_async(1).join()
    print("ignore error={ignored_error.state() == .Delivered} handlers={ignored_error.delivered_handlers()} failures={ignored_error.failures().len()} traced={ignored_error.trace().summary().contains("failed")}")

    panic_log :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Log) ?? panic("policy")
    panic_log.on_priority(failure_scope, 10, (n: Int) => panic_log_handler(n))
    panic_log.on_priority(failure_scope, 0, (n: Int) => {})
    logged_panic :: panic_log.emit_async(1).join()
    print("panic log={logged_panic.state() == .HandlerFailed} handlers={logged_panic.delivered_handlers()} failures={logged_panic.failures().len()} traced={logged_panic.trace().summary().contains("panic:log boom")}")

    panic_ignore :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Ignore) ?? panic("policy")
    panic_ignore.on_priority(failure_scope, 10, (n: Int) => panic_ignore_handler(n))
    panic_ignore.on_priority(failure_scope, 0, (n: Int) => {})
    ignored_panic :: panic_ignore.emit_async(1).join()
    print("panic ignore={ignored_panic.state() == .HandlerFailed} handlers={ignored_panic.delivered_handlers()} failures={ignored_panic.failures().len()} traced={ignored_panic.trace().summary().contains("panic:ignore boom")}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "async event policies failed: {stderr}");
    assert_eq!(
        stdout,
        "newest=true\noldest=true\nonce first=2 second=1\ncollect=true handlers=2 failures=2\nqueued -> running -> handler:0:failed -> handler:1:failed -> terminal:HandlerFailed\nstop=true handlers=1 failures=1\nlog error=true handlers=2 failures=0 traced=true\nignore error=true handlers=2 failures=0 traced=false\npanic log=true handlers=1 failures=1 traced=true\npanic ignore=true handlers=1 failures=1 traced=true\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn async_event_scope_cancel_and_inherited_deadline_are_single_terminal() {
    let have_rustc = common::have_rustc();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_corelib_async_event_lifecycle_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "async_event_lifecycle",
        r#"
use core.event as event
use core.tasks as tasks
use core.time as time

fn owner_teardown_task() => Task<DispatchReport<String>> {
    owner_scope :: event.scope()
    ev :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    (started_tx, started_rx) :: tasks.channel<Int>()
    (release_tx, release_rx) :: tasks.channel<Int>()
    ev.on(owner_scope, (n: Int) => {
        started_tx.send(~n)
        held_sender :: ~release_tx
        released :: release_rx.receive() ?? panic("release")
    })
    running :: ev.emit_async(98)
    started :: started_rx.receive() ?? panic("started")
    queued :: ev.emit_async(99)
    return queued
}

fn run() {
    cancel_scope :: event.scope()
    cancelled :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    (cancel_started_tx, cancel_started_rx) :: tasks.channel<Int>()
    (cancel_release_tx, cancel_release_rx) :: tasks.channel<Int>()
    cancelled.on(cancel_scope, (n: Int) => {
        cancel_started_tx.send(~n)
        released :: cancel_release_rx.receive() ?? panic("release")
    })
    cancel_running :: cancelled.emit_async(1)
    started :: cancel_started_rx.receive() ?? panic("started")
    cancel_queued :: cancelled.emit_async(2)
    cancel_pending :: cancelled.emit_async(3)
    print("before-cancel q={cancelled.queued_count()} r={cancelled.running_count()} p={cancelled.blocked_count()}")
    cancel_scope.cancel()
    pending_report :: cancel_pending.join()
    queued_report :: cancel_queued.join()
    running_report :: cancel_running.join()
    print("cancel pending={!pending_report.accepted() && pending_report.state() == .Cancelled} trace={pending_report.trace().summary()}")
    print("cancel queued={queued_report.accepted() && queued_report.state() == .Cancelled} trace={queued_report.trace().summary()}")
    print("cancel running={running_report.accepted() && running_report.state() == .Cancelled} trace={running_report.trace().summary()}")
    print("after-cancel q={cancelled.queued_count()} r={cancelled.running_count()} p={cancelled.blocked_count()}")

    queued_scope :: event.scope()
    queued_deadline :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    (queued_started_tx, queued_started_rx) :: tasks.channel<Int>()
    (queued_release_tx, queued_release_rx) :: tasks.channel<Int>()
    queued_deadline.on(queued_scope, (n: Int) => {
        queued_started_tx.send(~n)
        released :: queued_release_rx.receive() ?? panic("release")
    })
    queued_running :: queued_deadline.emit_async(10)
    queued_started :: queued_started_rx.receive() ?? panic("started")
    #Context(deadline: time.now() + 20) {
        expires_queued :: queued_deadline.emit_async(11)
        queued_expired :: expires_queued.join()
        print("deadline queued={queued_expired.accepted() && queued_expired.state() == .DeadlineExceeded} trace={queued_expired.trace().summary()}")
    }
    queued_scope.cancel()
    queued_running.join()

    pending_scope :: event.scope()
    pending_deadline :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    (pending_started_tx, pending_started_rx) :: tasks.channel<Int>()
    (pending_release_tx, pending_release_rx) :: tasks.channel<Int>()
    pending_deadline.on(pending_scope, (n: Int) => {
        pending_started_tx.send(~n)
        released :: pending_release_rx.receive() ?? panic("release")
    })
    pending_running :: pending_deadline.emit_async(20)
    pending_started :: pending_started_rx.receive() ?? panic("started")
    pending_queued :: pending_deadline.emit_async(21)
    #Context(deadline: time.now() + 20) {
        expires_pending :: pending_deadline.emit_async(22)
        pending_expired :: expires_pending.join()
        print("deadline pending={!pending_expired.accepted() && pending_expired.state() == .DeadlineExceeded} trace={pending_expired.trace().summary()}")
    }
    pending_scope.cancel()
    pending_queued.join()
    pending_running.join()

    owner_task :: owner_teardown_task()
    owner_report :: owner_task.join()
    print("owner teardown={owner_report.accepted() && owner_report.state() == .Cancelled} trace={owner_report.trace().summary()}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "async event lifecycle failed: {stderr}");
    assert_eq!(
        stdout,
        "before-cancel q=1 r=1 p=1\ncancel pending=true trace=pending -> terminal:Cancelled\ncancel queued=true trace=queued -> terminal:Cancelled\ncancel running=true trace=queued -> running -> terminal:Cancelled\nafter-cancel q=0 r=0 p=0\ndeadline queued=true trace=queued -> terminal:DeadlineExceeded\ndeadline pending=true trace=pending -> terminal:DeadlineExceeded\nowner teardown=true trace=queued -> terminal:Cancelled\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn async_event_terminal_transition_rejects_terminal_expected_phase() {
    let source = include_str!(
        "../../crates/jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs"
    );
    let complete_entry = source
        .split_once("fn complete_entry(")
        .expect("async event terminal transition")
        .1
        .split_once("fn complete_report(")
        .expect("async event report transition")
        .0;
    let terminal_guard = complete_entry
        .find("if expected == JET_EVENT_TERMINAL")
        .expect("terminal phase must be absorbing");
    let phase_cas = complete_entry
        .find("entry.phase.compare_exchange(")
        .expect("terminal transition CAS");
    assert!(
        terminal_guard < phase_cas,
        "TERMINAL -> TERMINAL must be rejected before the phase CAS"
    );
}

#[test]
fn async_event_cancel_and_close_winners_remain_immutable_after_task_drain() {
    let have_rustc = common::have_rustc();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_corelib_async_event_absorbing_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "async_event_absorbing",
        r#"
use core.event as event
use core.tasks as tasks
use core.time as time

fn run() {
    (cancel_gate_started_tx, cancel_gate_started_rx) :: tasks.channel<Int>()
    (cancel_gate_release_tx, cancel_gate_release_rx) :: tasks.channel<Int>()
    cancel_gate :: task {
        cancel_gate_started_tx.send(1)
        released :: cancel_gate_release_rx.receive() ?? panic("cancel gate")
    }
    cancel_gate_started :: cancel_gate_started_rx.receive() ?? panic("cancel gate start")

    cancel_scope :: event.scope()
    cancel_event :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    #Context(deadline: time.now() + 100000) {
        cancel_queued :: cancel_event.emit_async(1)
        cancel_pending :: cancel_event.emit_async(2)
        cancel_event.on(cancel_scope, (n: Int) => {})
        cancel_scope.cancel()
        cancel_gate_release_tx.send(1)
        queued_report :: cancel_queued.join()
        pending_report :: cancel_pending.join()
        print("cancel queued={queued_report.state() == .Cancelled} trace={queued_report.trace().summary()}")
        print("cancel pending={pending_report.state() == .Cancelled} trace={pending_report.trace().summary()}")
    }
    print("cancel counts={cancel_event.queued_count()},{cancel_event.running_count()},{cancel_event.blocked_count()}")

    (close_gate_started_tx, close_gate_started_rx) :: tasks.channel<Int>()
    (close_gate_release_tx, close_gate_release_rx) :: tasks.channel<Int>()
    close_gate :: task {
        close_gate_started_tx.send(1)
        released :: close_gate_release_rx.receive() ?? panic("close gate")
    }
    close_gate_started :: close_gate_started_rx.receive() ?? panic("close gate start")

    close_scope :: event.scope()
    close_event :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    close_event.on(close_scope, (n: Int) => {})
    #Context(deadline: time.now() + 100000) {
        close_queued :: close_event.emit_async(3)
        close_pending :: close_event.emit_async(4)
        close_event.close()
        close_scope.cancel()
        close_gate_release_tx.send(1)
        queued_report :: close_queued.join()
        pending_report :: close_pending.join()
        print("close queued={queued_report.state() == .Cancelled} trace={queued_report.trace().summary()}")
        print("close pending={pending_report.state() == .Closed} trace={pending_report.trace().summary()}")
    }
    print("close counts={close_event.queued_count()},{close_event.running_count()},{close_event.blocked_count()}")
}
"#,
        &[("JET_SCHEDULER_THREADS", "1")],
        None,
    );
    assert_eq!(code, 0, "async event absorbing terminal failed: {stderr}");
    assert_eq!(
        stdout,
        "cancel queued=true trace=queued -> terminal:Cancelled\ncancel pending=true trace=pending -> terminal:Cancelled\ncancel counts=0,0,0\nclose queued=true trace=queued -> terminal:Cancelled\nclose pending=true trace=pending -> terminal:Closed\nclose counts=0,0,0\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn event_sync_dispatch_handles_mutation_reentrancy_and_owner_drop() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_event_hostile_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "event_sync_hostile",
        r#"
use core.event as event

fn run() {
    scope :: event.scope()
    ev :: event.new<Int>()
    late :: ev.on(scope, (n) => { print("late {n}") })
    ev.on_priority(scope, 10, (n) => { print("killer {n}"); late.unsubscribe() })
    print(ev.emit(1).summary())
    print("listeners={ev.listener_count()}")

    additions :: event.scope()
    growing :: event.new<Int>()
    growing.on(additions, (n) => {
        print("root {n}")
        _ :: growing.on(additions, (m: Int) => { print("added {m}") })
    })
    print(growing.emit(1).summary())
    print(growing.emit(2).summary())

    nested_scope :: event.scope()
    nested :: event.new<Int>()
    nested.once(nested_scope, (n) => {
        print("once {n}")
        if n == 1 { nested.emit(2) }
    })
    print(nested.emit(1).summary())
    print("nested-listeners={nested.listener_count()}")

    owned :: event.new<Int>()
    if true {
        owner :: event.scope()
        owned.on(owner, (n) => { print("leaked {n}") })
    }
    print(owned.emit(9).summary())

    cancelled :: event.scope()
    stopped :: event.new<Int>()
    cancelled.cancel()
    stopped_sub :: stopped.on(cancelled, (n) => { print("cancelled event {n}") })
    print("cancelled-active={stopped_sub.is_active()}")
    print(stopped.emit(10).summary())
    stopped_hook :: event.hook<Int, String>("base")
    stopped_hook.on(cancelled, (n) => "cancelled hook {n}")
    print(stopped_hook.run(10, "fallback"))

    order_scope :: event.scope()
    ordered :: event.new<Int>()
    ordered.on_priority(order_scope, 5, (n) => { print("first {n}") })
    ordered.on_priority(order_scope, 5, (n) => { print("second {n}") })
    ordered.on(order_scope, (n) => { print("low {n}") })
    print(ordered.emit(3).summary())

    depth_scope :: event.scope()
    depth :: event.new<Int>()
    depth.on_priority(depth_scope, 5, (n) => {
        print("enter {n}")
        if n == 1 { print(depth.emit(2).summary()) }
    })
    depth.on(depth_scope, (n) => { print("leave {n}") })
    print(depth.emit(1).summary())
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "hostile event runtime failed: {stderr}");
    assert_eq!(
        stdout,
        "killer 1\nevent delivered=1 queued=0 dropped=0\nlisteners=1\nroot 1\nevent delivered=1 queued=0 dropped=0\nroot 2\nadded 2\nevent delivered=2 queued=0 dropped=0\nonce 1\nevent delivered=1 queued=0 dropped=0\nnested-listeners=0\nevent delivered=0 queued=0 dropped=0\ncancelled-active=false\nevent delivered=0 queued=0 dropped=0\nfallback\nfirst 3\nsecond 3\nlow 3\nevent delivered=3 queued=0 dropped=0\nenter 1\nenter 2\nleave 2\nevent delivered=2 queued=0 dropped=0\nleave 1\nevent delivered=2 queued=0 dropped=0\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn solve_solver_records_bool_constraints_in_order() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping solver runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_solve_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "solve_runtime",
        r#"
use core.solve as solve

fn run() {
    solver := solve.Solver.new(42)
    solver.require(1 + 1 == 2)
    solver.require(2 * 3 == 5)
    solver.require(true)
    print(solver.status())
    print(solver.failure_count())
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "failed\n1\n");
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn solve_require_needs_mutable_solver() {
    let src = r#"
use core.solve as solve

fn run() {
    solver :: solve.Solver.new(1)
    solver.require(true)
}
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "solver.require on immutable solver must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn solve_solver_type_name_is_reserved() {
    let src = r#"
struct Solver { value: Int }

fn run() {}
"#;
    let diags = jet::compile(src).expect_err("Solver is a reserved Core handle name");
    assert!(
        diags.iter().any(|d| d.code == "E0106"),
        "expected E0106, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn solve_constructor_is_static_only() {
    let src = r#"
use core.solve as solve

fn run() {
    solver := solve.Solver.new(1)
    solver.new(2)
}
"#;
    let diags = jet::compile(src).expect_err("solver.new must not be an instance method");
    assert!(
        !diags.is_empty(),
        "expected a diagnostic for instance constructor"
    );
}

#[test]
fn game_scene_asset_registration_needs_mutable_scene() {
    let src = r#"
use core.game as game

fn run() {
    scene :: game.Scene.new("arcade")
    scene.assets.image("assets/player.png") ?? panic("image")
}
"#;
    let diags = jet::compile(src).expect_err("asset registration must need edit access");
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn game_run_needs_mutable_scene_lvalue() {
    let src = r#"
use core.game as game

fn run() {
    print(game.run(game.Scene.new("arcade")))
}
"#;
    let diags = jet::compile(src).expect_err("game.run must reject temporary scene");
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn game_run_rejects_transposed_labels() {
    let src = r#"
use core.game as game

fn run() {
    scene := game.Scene.new("arcade")
    replay :: game.Replay.record("runs/demo.jetreplay")
    backend :: game.Backend.headless()
    print(game.run(scene, backend: backend, replay))
}
"#;
    let diags = jet::compile(src).expect_err("game.run labels must match positional shape");
    assert!(
        diags.iter().any(|d| matches!(d.code.as_str(), "E0764" | "E0769")),
        "expected E0764/E0769, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn game_headless_scene_replay_transcript_is_deterministic() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_game_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "game_headless",
        r#"
use core.game as game

module perf.game {
    budgets: [
        Budget.{ name: "frame", scope: .Scene("arcade"), metric: .FrameTime(.P99), provider: .SceneProbe("arcade"), comparison: .AbsoluteFrom("local/arcade"), limit: .AtMost(16ms) },
        Budget.{ name: "memory", scope: .Scene("arcade"), metric: .MemoryHighWater, provider: .SceneProbe("arcade"), comparison: .AbsoluteFrom("local/arcade"), limit: .AtMost(96MiB) },
        Budget.{ name: "assets", scope: .Scene("arcade"), metric: .SceneAssetBytes, provider: .SceneProbe("arcade"), comparison: .AbsoluteFrom("local/arcade"), limit: .AtMost(256KiB) },
        Budget.{ name: "draws", scope: .Scene("arcade"), metric: .DrawCalls(.P99), provider: .SceneProbe("arcade"), comparison: .AbsoluteFrom("local/arcade"), limit: .AtMost(4) },
    ]
}

struct Position { x: Int }
struct Velocity { dx: Int }

fn run() {
    scene := game.Scene.new("arcade")
    scene.assets.image("assets/player.png") ?? panic("image")
    scene.assets.sound("assets/jump.wav") ?? panic("sound")
    scene.input.bind("jump", "Space")
    scene.component<Position>()
    scene.component<Velocity>()
    hits :: scene.query<Position, Velocity>()
    print("query {hits.len()}")
    print("row {hits[0]}")
    backend := game.Backend.headless()
    n := 0
    loop {
        if !backend.should_continue() { break }
        backend.present()
        n += 1
    }
    print("budget {n}")
    scene.on_frame((frame) => {
        if frame.input.pressed("jump") {
            print("hook jump {frame.index}")
        }
    })
    replay :: game.Replay.record("runs/demo.jetreplay")
    print(game.run(scene, replay: replay))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "query 1\nrow Position{x:0},Velocity{dx:0}\nbudget 3\nhook jump 1\nscene:arcade\nbackend:headless/none/none\nreplay:runs/demo.jetreplay\nassets:image:assets/player.png,sound:assets/jump.wav\ninput:jump=Space\ncomponents:Position,Velocity\nframe:0 input:none\nframe:1 input:jump\nframe:2 input:none\n"
    );
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

// D-AUTH2=A / D-AUTH-TOKENPOLICY1=A: exercise the public Jet surface so the
// existing JSON parser, HMAC implementation, Ed25519 bridge, nominal claims,
// codegen, and linker all participate in the proof.
#[test]
fn core_auth_strict_jwt_and_paseto_hostile_matrix() {
    let dir = std::env::temp_dir().join(format!("jet_core_auth_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.auth as auth

fn run() {
    jwt_key :: [U8].{ 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102 }
    no_skew :: Duration.milliseconds(0) ?? panic("duration")
    valid_jwt := "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB9.3gbnbn_u-GjiQuGusiLrnMUzlo5c9rPeqAO0iWZxhrY"
    wrong_aud := "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImJpbGxpbmciLCJleHAiOjQxMDI0NDQ4MDB9.4HckXFIKTMLaJr8Zjz8hYC0NQ9gO1xbLzZwoNxU1ew4"
    missing_exp := "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkifQ.w3V9KixrW5iIdce6fH3-kTGBF1BoIAVN9jlaASUZyo8"
    missing_aud := "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImV4cCI6NDEwMjQ0NDgwMH0.DvdDttFvdgTOXtC2L5P1zfs2bIMtiEwN3al4EAHYyf8"
    wrong_alg := "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJleHAiOjQxMDI0NDQ4MDB9.Nq0tUwRS8BvslH3fvzVydHKrce-EcFBuLy7OpgQ2ICk"
    duplicate_header := "eyJhbGciOiJIUzI1NiIsImFsZyI6IlJTMjU2In0.eyJhdWQiOiJnYXRld2F5IiwiZXhwIjo0MTAyNDQ0ODAwfQ.MVJzUJG0exT9xheHOk7OpVqtfue7C_625krxtNm99qw"
    escaped_duplicate_header := "eyJhbGciOiJIUzI1NiIsIlx1MDA2MWxnIjoiUlMyNTYifQ.eyJhdWQiOiJnYXRld2F5IiwiZXhwIjo0MTAyNDQ0ODAwfQ.z6ZtYWs143-PSZdfZSqrtX1lZOOb5KiXh_J-H6nr5gs"
    duplicate_audience := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJnYXRld2F5IiwiYXVkIjoiYmlsbGluZyIsImV4cCI6NDEwMjQ0NDgwMH0.OVeIFJjjIN6Py2ZsvNiOFERv0Syt2nDTF2ZUZwWQkS0"
    escaped_duplicate_audience := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJnYXRld2F5IiwiXHUwMDYxdWQiOiJiaWxsaW5nIiwiZXhwIjo0MTAyNDQ0ODAwfQ.-RJABGbCML2FgyJx4iWT4NsklKovltcY_lyVzDNTec4"
    object_audience := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOnsieCI6ImdhdGV3YXkifSwiZXhwIjo0MTAyNDQ0ODAwfQ.BNUK56f_MGWL-7vRscOjDZGWtXZA18muouezh3BFg-Q"
    object_expiry := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJnYXRld2F5IiwiZXhwIjp7Im4iOjQxMDI0NDQ4MDB9fQ.X1BTPgGav4pUqxQVq2uMYt4_VYEHfMRGP1aI5V50k2g"
    wrong_issuer := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJnYXRld2F5IiwiaXNzIjoib3RoZXIiLCJleHAiOjQxMDI0NDQ4MDB9.ZVsh0LK7bvsylhpzu4i8TrgthCbSaelpKaoxWqF5-G4"
    expired := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJnYXRld2F5IiwiZXhwIjo5NDY2ODQ4MDB9.P-GYVR6Tc1zwSdZCEX6kbv4eSryvnxlevXfHU0MJMEg"
    overflow_expiry := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJnYXRld2F5IiwiZXhwIjo5MjIzMzcyMDM2ODU0Nzc1MDAwfQ.jHiJ1xzrrSVPwIEX-EujI-xiDDdgc7AvP6HsMWrb_L8"
    noncanonical_base64 := "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB9.3gbnbn_u-GjiQuGusiLrnMUzlo5c9rPeqAO0iWZxhrZ"
    if auth.verify_jwt(valid_jwt, key: jwt_key, audience: "gateway", issuer: "partner", clock_skew: no_skew) == {
        .Ok(claims) -> { print("ok:{claims.audience}") }
        .Err(_) -> { print("rejected") }
    }
    if auth.verify_jwt(wrong_aud, key: jwt_key, audience: "gateway") == {
        .Ok(_) -> { print("accepted") }
        .Err(error) -> {
            if error == {
                .WrongAudience(expected, actual) -> { print("aud:{expected}:{actual}") }
                else -> { print("wrong-error") }
            }
        }
    }
    if auth.verify_jwt(missing_exp, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("accepted") } .Err(_) -> { print("rejected") } }
    if auth.verify_jwt(missing_aud, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("accepted") } .Err(_) -> { print("rejected") } }
    if auth.verify_jwt(wrong_alg, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("accepted") } .Err(_) -> { print("rejected") } }
    if auth.verify_jwt("{valid_jwt}x", key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("accepted") } .Err(_) -> { print("rejected") } }
    if auth.verify_jwt(duplicate_header, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("duplicate-header-accepted") } .Err(_) -> { print("duplicate-header-rejected") } }
    if auth.verify_jwt(escaped_duplicate_header, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("escaped-duplicate-header-accepted") } .Err(_) -> { print("escaped-duplicate-header-rejected") } }
    if auth.verify_jwt(duplicate_audience, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("duplicate-audience-accepted") } .Err(_) -> { print("duplicate-audience-rejected") } }
    if auth.verify_jwt(escaped_duplicate_audience, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("escaped-duplicate-audience-accepted") } .Err(_) -> { print("escaped-duplicate-audience-rejected") } }
    if auth.verify_jwt(object_audience, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("object-audience-accepted") } .Err(_) -> { print("object-audience-rejected") } }
    if auth.verify_jwt(object_expiry, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("object-expiry-accepted") } .Err(_) -> { print("object-expiry-rejected") } }
    if auth.verify_jwt(wrong_issuer, key: jwt_key, audience: "gateway", issuer: "partner") == { .Ok(_) -> { print("issuer-accepted") } .Err(_) -> { print("issuer-rejected") } }
    if auth.verify_jwt(expired, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("expired-accepted") } .Err(_) -> { print("expired-rejected") } }
    if auth.verify_jwt(overflow_expiry, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("overflow-accepted") } .Err(_) -> { print("overflow-rejected") } }
    if auth.verify_jwt(noncanonical_base64, key: jwt_key, audience: "gateway", issuer: "partner") == { .Ok(_) -> { print("noncanonical-accepted") } .Err(_) -> { print("noncanonical-rejected") } }
    weak_key :: [U8].{ 115, 104, 111, 114, 116 }
    if auth.verify_jwt(valid_jwt, key: weak_key, audience: "gateway") == { .Ok(_) -> { print("weak-key-accepted") } .Err(error) -> { if error == { .WeakKey -> { print("weak-key-rejected") } else -> { print("weak-key-wrong-error") } } } }

    public_key :: [U8].{ 198, 185, 67, 192, 34, 178, 159, 209, 168, 14, 60, 124, 14, 126, 172, 99, 191, 6, 53, 9, 101, 220, 114, 205, 7, 138, 24, 227, 74, 150, 126, 45 }
    footer :: [U8].{ 107, 105, 100, 45, 49 }
    implicit :: [U8].{ 116, 101, 110, 97, 110, 116, 45, 97 }
    paseto := "v4.public.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB99cRKnMLYsWG_FHDSPR15TvgcHSv6gYcTBIy9ToyrtIMVWk4i5vp1sgI5rehiGKdAoyKHQ1zKXDe0It-WADRzAw.a2lkLTE"
    bad_signature := "v4.public.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB99cRKnMLYsWG_FHDSPR15TvgcHSv6gYcTBIy9ToyrtIMVWk4i5vp1sgI5rehiGKdAoyKHQ1zKXDe0It-WADRzAg.a2lkLTE"
    wrong_purpose := "v4.local.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB99cRKnMLYsWG_FHDSPR15TvgcHSv6gYcTBIy9ToyrtIMVWk4i5vp1sgI5rehiGKdAoyKHQ1zKXDe0It-WADRzAw.a2lkLTE"
    wrong_version := "v3.public.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB99cRKnMLYsWG_FHDSPR15TvgcHSv6gYcTBIy9ToyrtIMVWk4i5vp1sgI5rehiGKdAoyKHQ1zKXDe0It-WADRzAw.a2lkLTE"
    bad :: [U8].{ 98, 97, 100 }
    zero_key :: [U8].{ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 }
    if auth.verify_paseto(paseto, key: public_key, audience: "gateway", issuer: "partner", clock_skew: no_skew, footer: footer, implicit: implicit) == {
        .Ok(claims) -> { print("ok:{claims.audience}") }
        .Err(_) -> { print("rejected") }
    }
    if auth.verify_paseto(paseto, key: public_key, audience: "gateway", issuer: "partner", clock_skew: no_skew, footer: bad, implicit: implicit) == { .Ok(_) -> { print("accepted") } .Err(_) -> { print("rejected") } }
    if auth.verify_paseto(paseto, key: public_key, audience: "gateway", issuer: "partner", clock_skew: no_skew, footer: footer, implicit: bad) == { .Ok(_) -> { print("accepted") } .Err(_) -> { print("rejected") } }
    if auth.verify_paseto(wrong_version, key: public_key, audience: "gateway") == { .Ok(_) -> { print("wrong-version-accepted") } .Err(_) -> { print("wrong-version-rejected") } }
    if auth.verify_paseto(wrong_purpose, key: public_key, audience: "gateway") == { .Ok(_) -> { print("wrong-purpose-accepted") } .Err(_) -> { print("wrong-purpose-rejected") } }
    if auth.verify_paseto(paseto, key: bad, audience: "gateway") == { .Ok(_) -> { print("short-paseto-key-accepted") } .Err(_) -> { print("short-paseto-key-rejected") } }
    if auth.verify_paseto(paseto, key: zero_key, audience: "gateway") == { .Ok(_) -> { print("zero-paseto-key-accepted") } .Err(_) -> { print("zero-paseto-key-rejected") } }
    if auth.verify_paseto(bad_signature, key: public_key, audience: "gateway", issuer: "partner", clock_skew: no_skew, footer: footer, implicit: implicit) == { .Ok(_) -> { print("bad-signature-accepted") } .Err(_) -> { print("bad-signature-rejected") } }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "strict_tokens", source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "ok:gateway\naud:gateway:billing\nrejected\nrejected\nrejected\nrejected\nduplicate-header-rejected\nescaped-duplicate-header-rejected\nduplicate-audience-rejected\nescaped-duplicate-audience-rejected\nobject-audience-rejected\nobject-expiry-rejected\nissuer-rejected\nexpired-rejected\noverflow-rejected\nnoncanonical-rejected\nweak-key-rejected\nok:gateway\nrejected\nrejected\nwrong-version-rejected\nwrong-purpose-rejected\nshort-paseto-key-rejected\nzero-paseto-key-rejected\nbad-signature-rejected\n"
    );
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

fn auth_test_b64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let bits = (a << 16) | (b << 8) | c;
        out.push(TABLE[((bits >> 18) & 63) as usize] as char);
        out.push(TABLE[((bits >> 12) & 63) as usize] as char);
        if chunk.len() > 1 { out.push(TABLE[((bits >> 6) & 63) as usize] as char); }
        if chunk.len() > 2 { out.push(TABLE[(bits & 63) as usize] as char); }
    }
    out
}

fn auth_test_jwt(payload: &str) -> String {
    let key = b"0123456789abcdef0123456789abcdef";
    let header = auth_test_b64url(br#"{"alg":"HS256"}"#);
    let payload = auth_test_b64url(payload.as_bytes());
    let signed = format!("{header}.{payload}");
    let mut block = [0u8; 64];
    block[..key.len()].copy_from_slice(key);
    let mut inner = Vec::with_capacity(64 + signed.len());
    inner.extend(block.iter().map(|byte| byte ^ 0x36));
    inner.extend_from_slice(signed.as_bytes());
    let inner = jet::SHA256::sha256(&inner);
    let mut outer = Vec::with_capacity(96);
    outer.extend(block.iter().map(|byte| byte ^ 0x5c));
    outer.extend_from_slice(&inner);
    format!("{signed}.{}", auth_test_b64url(&jet::SHA256::sha256(&outer)))
}

#[test]
fn core_auth_expiry_equality_and_subsecond_skew() {
    let dir = std::env::temp_dir().join(format!("jet_core_auth_clock_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.auth as auth
use core.env as env

fn run() {
    key :: [U8].{ 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102 }
    token := env.get("JET_AUTH_CLOCK_TOKEN") ?? panic("token")
    zero :: Duration.milliseconds(0) ?? panic("zero")
    skew :: Duration.milliseconds(1500) ?? panic("skew")
    if auth.verify_jwt(token, key: key, audience: "gateway", issuer: "clock", clock_skew: zero) == {
        .Ok(_) -> { print("equality-accepted") }
        .Err(error) -> { if error == { .TokenExpired -> { print("equality-expired") } else -> { print("wrong-error") } } }
    }
    if auth.verify_jwt(token, key: key, audience: "gateway", issuer: "clock", clock_skew: skew) == {
        .Ok(_) -> { print("subsecond-skew-accepted") }
        .Err(_) -> { print("subsecond-skew-rejected") }
    }
}
"#;
    let shown = dir.join("clock.jet");
    fs::write(&shown, src).unwrap();
    let compiled = jet::compile_with_path(src, shown.to_str().unwrap()).unwrap();
    let rs = dir.join("clock.rs");
    let bin = dir.join("clock");
    let mut rustc = Command::new("rustc");
    common::add_generated_rust(
        &mut rustc,
        &rs,
        &compiled.rust,
        compiled.ffi.is_some(),
        &[],
    );
    rustc.arg("-o").arg(&bin);
    if let Some(link) = compiled.ffi {
        rustc.arg("--extern").arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) { rustc.arg("-L").arg(format!("dependency={}", deps_dir.display())); }
    }
    let built = rustc.output().unwrap();
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    let expires_at = now.as_secs() + 2;
    let token = auth_test_jwt(&format!(r#"{{"aud":"gateway","iss":"clock","exp":{expires_at}}}"#));
    let boundary_ms = u128::from(expires_at) * 1_000;
    while std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() < boundary_ms {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let run = Command::new(&bin).env("JET_AUTH_CLOCK_TOKEN", token).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "equality-expired\nsubsecond-skew-accepted\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_auth_requires_named_key_and_audience() {
    let src = r#"
use core.auth as auth

fn run() {
    token := "a.b.c"
    key := [0, 1, 2]
    auth.verify_jwt(token, key, "gateway")
}
"#;
    let diags = jet::compile(src).expect_err("auth trust inputs must be named");
    assert!(
        diags.iter().filter(|diagnostic| matches!(diagnostic.code.as_str(), "E0764" | "E0769")).count() >= 2,
        "expected key:/audience: label diagnostics, got {diags:?}"
    );
}

#[test]
fn tracked_float_origin_reports_binding_site_and_plain_float_is_untracked() {
    let dir = std::env::temp_dir().join(format!(
        "jet_float_binding_origin_aot_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let name = "float_binding_origin";
    let src = "fn run() {\n    #Track speed :: 3.5\n    plain :: 3.5\n    copied :: speed\n    print(speed.origin())\n    print((speed).origin())\n    print(plain.origin())\n    print(copied.origin())\n    print(next().origin())\n}\nfn next() => Float {\n    print(\"evaluated\")\n    return 3.5\n}\n";
    let (code, stdout, stderr) = build_and_run(&dir, name, src, &[], None);
    let source_path = dir.join(name);

    assert_eq!(code, 0, "tracked Float runtime failed: {stderr}");
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        format!(
            "tracked `speed` at {}:2:12: #Track speed :: 3.5\ntracked `speed` at {}:2:12: #Track speed :: 3.5\nuntracked\nuntracked\nevaluated\nuntracked\n",
            source_path.display(),
            source_path.display()
        )
    );

    fs::write(&source_path, src).unwrap();
    match jet::Interpreter::dev_iteration(source_path.to_str().unwrap(), false, true) {
        jet::Interpreter::RunOutcome::Ran {
            stdout: interpreted_stdout,
            stderr: interpreted_stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "forced interpreter failed: {interpreted_stderr}");
            assert_eq!(interpreted_stderr, "");
            assert_eq!(interpreted_stdout, stdout, "forced interpreter output drifted");
        }
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("forced interpreter rejected tracked-float fixture: {diagnostics:?}");
        }
    }
    jet_jit::reset_jit_trace_for_test();
    match jet::Interpreter::dev_iteration(source_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout: resident_stdout,
            stderr: resident_stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "resident JIT failed: {resident_stderr}");
            assert_eq!(resident_stderr, "");
            assert_eq!(resident_stdout, stdout, "resident JIT output drifted");
            assert!(
                jet_jit::jit_executed_for_test(),
                "tracked-float fixture must execute resident JIT"
            );
            assert!(
                !jet_jit::deopt_invoked_for_test(),
                "tracked-float fixture must not deopt"
            );
            assert!(
                !jet_jit::fallback_invoked_for_test(),
                "tracked-float fixture must not fall back"
            );
        }
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("resident JIT rejected tracked-float fixture: {diagnostics:?}");
        }
    }

    let web_source = r#"
#Target(Web)
#Target(JS)
fn run() {
    #Track speed :: 3.5
    plain :: 3.5
    copied :: speed
    print(speed.origin())
    print(plain.origin())
    print(copied.origin())
    print(next().origin())
}
#Target(JS)
fn next() => Float {
    print("evaluated")
    return 3.5
}
"#;
    if let Some(web_stdout) = run_web_js_source(&dir, "float_binding_origin_web", web_source) {
        let mut lines = web_stdout.lines();
        assert!(
            lines
                .next()
                .is_some_and(|line| line.starts_with("tracked `speed` at ")),
            "Web lost tracked origin: {web_stdout:?}"
        );
        assert_eq!(lines.next(), Some("untracked"), "Web plain origin drifted");
        assert_eq!(lines.next(), Some("untracked"), "Web copied origin drifted");
        assert_eq!(
            lines.next(),
            Some("evaluated"),
            "Web origin receiver evaluated wrong"
        );
        assert_eq!(lines.next(), Some("untracked"), "Web call origin drifted");
        assert!(
            lines.next().is_none(),
            "Web origin emitted extra output: {web_stdout:?}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}
