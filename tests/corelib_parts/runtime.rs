#[test]
fn perf_static_api_lowers_to_core_helpers() {
    let out = compile_temp(
        "perf_static.jet",
        r#"
fn run() ? {
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

fn run() ? {
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

fn run() ? {
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
fn missing_float() => Float? :: None
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

fn panic_log_handler(n: Int) ? String {
    panic("log boom")
    return .Err("unreachable")
}

fn panic_ignore_handler(n: Int) ? String {
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
    let high_range = auth_test_jwt(r#"{"aud":"gateway","exp":9007199254740993}"#);
    let lower_bound_expiry = auth_test_jwt(r#"{"aud":"gateway","exp":-9223372036854775}"#);
    let below_lower_bound_expiry = auth_test_jwt(r#"{"aud":"gateway","exp":-9223372036854776}"#);
    let overflow_expiry = auth_test_jwt(r#"{"aud":"gateway","exp":9223372036854775808}"#);
    let negative_zero_expiry = auth_test_jwt(r#"{"aud":"gateway","exp":-0}"#);
    let negative_zero_issued_at =
        auth_test_jwt(r#"{"aud":"gateway","exp":4102444800,"iat":-0}"#);
    let unicode_whitespace = auth_test_jwt(
        "{\"aud\":\"gateway\",\"exp\":\u{00a0}4102444800}",
    );
    let noncanonical_payload =
        auth_test_noncanonical_b64url(br#"{"aud":"gateway","iss":"partner","exp":4102444800}"#);
    let noncanonical_base64 = auth_test_jwt_signed(
        &auth_test_b64url(br#"{"alg":"HS256","typ":"JWT"}"#),
        &noncanonical_payload,
    );
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
    overflow_expiry := "@OVERFLOW_EXPIRY"
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
    if auth.verify_jwt("@LOWER_BOUND_EXPIRY", key: jwt_key, audience: "gateway") == {
        .Ok(_) -> { print("lower-bound-accepted") }
        .Err(error) -> { if error == { .TokenExpired -> { print("lower-bound-expired") } else -> { print("lower-bound-wrong-error") } } }
    }
    if auth.verify_jwt("@BELOW_LOWER_BOUND_EXPIRY", key: jwt_key, audience: "gateway") == {
        .Ok(_) -> { print("below-lower-bound-accepted") }
        .Err(error) -> {
            if error == {
                .MalformedToken(_) -> { print("below-lower-bound-malformed") }
                .TokenExpired -> { print("below-lower-bound-expired") }
                else -> { print("below-lower-bound-wrong-error") }
            }
        }
    }
    if auth.verify_jwt("@HIGH_RANGE", key: jwt_key, audience: "gateway") == { .Ok(claims) -> { print("high-range:{claims.expires_at}") } .Err(_) -> { print("high-range-rejected") } }
    if auth.verify_jwt("@NEGATIVE_ZERO_EXPIRY", key: jwt_key, audience: "gateway") == {
        .Ok(_) -> { print("negative-zero-expiry-accepted") }
        .Err(error) -> {
            if error == {
                .MalformedToken(reason) -> { print("negative-zero-expiry:{reason}") }
                else -> { print("negative-zero-expiry-wrong-error") }
            }
        }
    }
    if auth.verify_jwt("@NEGATIVE_ZERO_ISSUED_AT", key: jwt_key, audience: "gateway") == {
        .Ok(_) -> { print("negative-zero-iat-accepted") }
        .Err(error) -> {
            if error == {
                .MalformedToken(reason) -> { print("negative-zero-iat:{reason}") }
                else -> { print("negative-zero-iat-wrong-error") }
            }
        }
    }
    if auth.verify_jwt("@UNICODE_WHITESPACE", key: jwt_key, audience: "gateway") == {
        .Ok(_) -> { print("unicode-whitespace-accepted") }
        .Err(error) -> {
            if error == {
                .DecodeError(reason) -> { print("unicode-whitespace:{reason}") }
                else -> { print("unicode-whitespace-wrong-error") }
            }
        }
    }
    if auth.verify_jwt("@NONCANONICAL_BASE64", key: jwt_key, audience: "gateway", issuer: "partner") == {
        .Ok(_) -> { print("noncanonical-accepted") }
        .Err(error) -> {
            if error == {
                .DecodeError(reason) -> { print("noncanonical:{reason}") }
                .InvalidSignature -> { print("noncanonical-invalid-signature") }
                else -> { print("noncanonical-wrong-error") }
            }
        }
    }
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
"#
    .replace("@HIGH_RANGE", &high_range)
    .replace("@LOWER_BOUND_EXPIRY", &lower_bound_expiry)
    .replace("@BELOW_LOWER_BOUND_EXPIRY", &below_lower_bound_expiry)
    .replace("@OVERFLOW_EXPIRY", &overflow_expiry)
    .replace("@NEGATIVE_ZERO_EXPIRY", &negative_zero_expiry)
    .replace("@NEGATIVE_ZERO_ISSUED_AT", &negative_zero_issued_at)
    .replace("@UNICODE_WHITESPACE", &unicode_whitespace)
    .replace("@NONCANONICAL_BASE64", &noncanonical_base64);
    let (code, stdout, stderr) = build_and_run(&dir, "strict_tokens", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "ok:gateway\naud:gateway:billing\nrejected\nrejected\nrejected\nrejected\nduplicate-header-rejected\nescaped-duplicate-header-rejected\nduplicate-audience-rejected\nescaped-duplicate-audience-rejected\nobject-audience-rejected\nobject-expiry-rejected\nissuer-rejected\nexpired-rejected\noverflow-rejected\nlower-bound-expired\nbelow-lower-bound-expired\nhigh-range:9007199254740993\nnegative-zero-expiry:claim `exp` must be an exact integer\nnegative-zero-iat:claim `iat` must be an exact integer\nunicode-whitespace:expected a JSON value\nnoncanonical:non-canonical base64 trailing bits\nweak-key-rejected\nok:gateway\nrejected\nrejected\nwrong-version-rejected\nwrong-purpose-rejected\nshort-paseto-key-rejected\nzero-paseto-key-rejected\nbad-signature-rejected\n"
    );
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_auth_jwt_audience_shapes_match_aot_jit_and_interpreter() {
    let dir = std::env::temp_dir().join(format!("jet_core_auth_audience_shapes_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let scalar = auth_test_jwt(r#"{"aud":"gateway","exp":4102444800}"#);
    let list = auth_test_jwt(r#"{"aud":["gateway","billing"],"exp":4102444800}"#);
    let wrong_list = auth_test_jwt(r#"{"aud":["billing"],"exp":4102444800}"#);
    let empty = auth_test_jwt(r#"{"aud":[],"exp":4102444800}"#);
    let mixed = auth_test_jwt(r#"{"aud":["gateway",7],"exp":4102444800}"#);
    let nested = auth_test_jwt(r#"{"aud":[["gateway"]],"exp":4102444800}"#);
    let future_not_before =
        auth_test_jwt(r#"{"aud":"gateway","exp":4102444800,"nbf":4102444800}"#);
    let past_not_before =
        auth_test_jwt(r#"{"aud":"gateway","exp":4102444800,"nbf":1700000000}"#);
    let src = r#"
use core.auth as auth

fn run() {
    key :: [U8].{ 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102 }
    if auth.verify_jwt("@SCALAR", key: key, audience: "gateway") == {
        .Ok(claims) -> { print("scalar:{claims.audience}") }
        .Err(_) -> { print("scalar:rejected") }
    }
    if auth.verify_jwt("@LIST", key: key, audience: "gateway") == {
        .Ok(claims) -> { print("list:{claims.audience}") }
        .Err(_) -> { print("list:rejected") }
    }
    if auth.verify_jwt("@WRONG_LIST", key: key, audience: "gateway") == {
        .Ok(_) -> { print("wrong:accepted") }
        .Err(error) -> {
            if error == {
                .WrongAudience(expected, actual) -> { print("wrong:{expected}:{actual}") }
                else -> { print("wrong:wrong-error") }
            }
        }
    }
    if auth.verify_jwt("@EMPTY", key: key, audience: "gateway") == {
        .Ok(_) -> { print("empty:accepted") }
        .Err(error) -> {
            if error == {
                .MalformedToken(_) -> { print("empty:malformed") }
                else -> { print("empty:wrong-error") }
            }
        }
    }
    if auth.verify_jwt("@MIXED", key: key, audience: "gateway") == {
        .Ok(_) -> { print("mixed:accepted") }
        .Err(error) -> {
            if error == {
                .MalformedToken(_) -> { print("mixed:malformed") }
                else -> { print("mixed:wrong-error") }
            }
        }
    }
    if auth.verify_jwt("@NESTED", key: key, audience: "gateway") == {
        .Ok(_) -> { print("nested:accepted") }
        .Err(error) -> {
            if error == {
                .MalformedToken(_) -> { print("nested:malformed") }
                else -> { print("nested:wrong-error") }
            }
        }
    }
    if auth.verify_jwt("@FUTURE_NBF", key: key, audience: "gateway") == {
        .Ok(_) -> { print("future-nbf:accepted") }
        .Err(error) -> {
            if error == {
                .TokenNotYetValid -> { print("future-nbf:not-yet-valid") }
                else -> { print("future-nbf:wrong-error") }
            }
        }
    }
    if auth.verify_jwt("@PAST_NBF", key: key, audience: "gateway") == {
        .Ok(claims) -> {
            if claims.not_before == {
                .Val(value) -> { print("past-nbf:{value}") }
                else -> { print("past-nbf:none") }
            }
        }
        .Err(_) -> { print("past-nbf:rejected") }
    }
    public_key :: [U8].{ 198, 185, 67, 192, 34, 178, 159, 209, 168, 14, 60, 124, 14, 126, 172, 99, 191, 6, 53, 9, 101, 220, 114, 205, 7, 138, 24, 227, 74, 150, 126, 45 }
    footer :: [U8].{ 107, 105, 100, 45, 49 }
    implicit :: [U8].{ 116, 101, 110, 97, 110, 116, 45, 97 }
    no_skew :: Duration.milliseconds(0) ?? panic("duration")
    paseto := "v4.public.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB99cRKnMLYsWG_FHDSPR15TvgcHSv6gYcTBIy9ToyrtIMVWk4i5vp1sgI5rehiGKdAoyKHQ1zKXDe0It-WADRzAw.a2lkLTE"
    if auth.verify_paseto(paseto, key: public_key, audience: "gateway", issuer: "partner", clock_skew: no_skew, footer: footer, implicit: implicit) == {
        .Ok(claims) -> { print("paseto:{claims.audience}") }
        .Err(_) -> { print("paseto:rejected") }
    }
    if auth.verify_paseto(paseto, key: public_key, audience: "billing", issuer: "partner", clock_skew: no_skew, footer: footer, implicit: implicit) == {
        .Ok(_) -> { print("paseto-wrong:accepted") }
        .Err(error) -> {
            if error == {
                .WrongAudience(expected, actual) -> { print("paseto-wrong:{expected}:{actual}") }
                else -> { print("paseto-wrong:wrong-error") }
            }
        }
    }
}
"#
    .replace("@SCALAR", &scalar)
    .replace("@LIST", &list)
    .replace("@WRONG_LIST", &wrong_list)
    .replace("@EMPTY", &empty)
    .replace("@MIXED", &mixed)
    .replace("@NESTED", &nested)
    .replace("@FUTURE_NBF", &future_not_before)
    .replace("@PAST_NBF", &past_not_before);
    let (code, aot_stdout, stderr) = build_and_run(&dir, "audience_shapes", &src, &[], None);
    assert_eq!(code, 0, "audience-shape AOT failed: {stderr}");
    let expected = "scalar:gateway\nlist:gateway\nwrong:gateway:billing\nempty:malformed\nmixed:malformed\nnested:malformed\nfuture-nbf:not-yet-valid\npast-nbf:1700000000\npaseto:gateway\npaseto-wrong:billing:gateway\n";
    assert_eq!(aot_stdout, expected);

    let path = dir.join("audience_shapes");
    fs::write(&path, &src).unwrap();
    let shown = path.to_string_lossy().into_owned();
    jet_jit::reset_jit_trace_for_test();
    let interpreted = match jet::Interpreter::dev_iteration(&shown, false, true) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!(stderr, "");
            assert_eq!(exit_code, 0);
            stdout
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("audience-shape forced interpreter failed: {diags:?}")
        }
    };
    assert!(!jet_jit::jit_executed_for_test());
    assert!(!jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test());
    assert_eq!(interpreted, expected);

    jet_jit::reset_jit_trace_for_test();
    let resident = match jet::Interpreter::dev_iteration(&shown, false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!(stderr, "");
            assert_eq!(exit_code, 0);
            stdout
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("audience-shape resident JIT failed: {diags:?}")
        }
    };
    assert!(jet_jit::jit_executed_for_test(), "auth parity must execute resident JIT");
    assert!(!jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(), "auth parity must not deopt or fall back");
    assert_eq!(resident, expected);
    assert_eq!(resident, interpreted);
    assert_eq!(resident, aot_stdout);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_auth_jwt_iat_option_int_boundaries_match_aot_jit_and_interpreter() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_auth_iat_boundaries_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let negative = auth_test_jwt(r#"{"aud":"gateway","exp":4102444800,"iat":-1}"#);
    let maximum = auth_test_jwt(
        r#"{"aud":"gateway","exp":4102444800,"iat":9223372036854775807}"#,
    );
    let src = r#"
use core.auth as auth

fn run() {
    key :: [U8].{ 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102 }
    if auth.verify_jwt("@NEGATIVE", key: key, audience: "gateway") == {
        .Ok(claims) -> {
            negative_iat :: claims.issued_at
            negative_copy :: negative_iat
            if negative_copy == .Val(value) {
                print("iat-neg")
                print(value)
            } else {
                print("iat-neg:none")
            }
        }
        .Err(_) -> { print("iat-neg:error") }
    }
    if auth.verify_jwt("@MAXIMUM", key: key, audience: "gateway") == {
        .Ok(claims) -> {
            maximum_iat :: claims.issued_at
            maximum_copy :: maximum_iat
            if maximum_copy == .Val(value) {
                print("iat-max")
                print(value)
            } else {
                print("iat-max:none")
            }
        }
        .Err(_) -> { print("iat-max:error") }
    }
}
"#
    .replace("@NEGATIVE", &negative)
    .replace("@MAXIMUM", &maximum);
    let (code, aot_stdout, stderr) = build_and_run(&dir, "iat_boundaries", &src, &[], None);
    assert_eq!(code, 0, "iat-boundary AOT failed: {stderr}");
    let expected = "iat-neg\n-1\niat-max\n9223372036854775807\n";
    assert_eq!(aot_stdout, expected);

    let path = dir.join("iat_boundaries");
    fs::write(&path, &src).unwrap();
    let shown = path.to_string_lossy().into_owned();
    jet_jit::reset_jit_trace_for_test();
    let interpreted = match jet::Interpreter::dev_iteration(&shown, false, true) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(stderr, "");
            assert_eq!(exit_code, 0);
            stdout
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("iat-boundary forced interpreter failed: {diags:?}")
        }
    };
    assert!(!jet_jit::jit_executed_for_test());
    assert!(!jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test());
    assert_eq!(interpreted, expected);

    jet_jit::reset_jit_trace_for_test();
    let resident = match jet::Interpreter::dev_iteration(&shown, false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(stderr, "");
            assert_eq!(exit_code, 0);
            stdout
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("iat-boundary resident JIT failed: {diags:?}")
        }
    };
    assert!(
        jet_jit::jit_executed_for_test(),
        "iat boundary parity must execute resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "iat boundary parity must not deopt or fall back"
    );
    assert_eq!(resident, expected);
    assert_eq!(resident, interpreted);
    assert_eq!(resident, aot_stdout);
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

fn auth_test_noncanonical_b64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let remainder = bytes.len() % 3;
    assert!(remainder != 0);
    let mut encoded = auth_test_b64url(bytes);
    let last = encoded.pop().unwrap();
    let index = TABLE
        .iter()
        .position(|byte| *byte as char == last)
        .unwrap();
    let unused_bits: usize = if remainder == 1 { 4 } else { 2 };
    assert_eq!(index & ((1usize << unused_bits) - 1), 0);
    encoded.push(TABLE[index | 1] as char);
    encoded
}

fn auth_test_jwt_signed(header: &str, payload: &str) -> String {
    let key = b"0123456789abcdef0123456789abcdef";
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

fn auth_test_jwt(payload: &str) -> String {
    let header = auth_test_b64url(br#"{"alg":"HS256"}"#);
    let payload = auth_test_b64url(payload.as_bytes());
    auth_test_jwt_signed(&header, &payload)
}

#[test]
fn core_auth_expiry_equality_and_nanosecond_skew() {
    let dir = std::env::temp_dir().join(format!("jet_core_auth_clock_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    struct AuthClockFixture {
        token: String,
        skew_token: String,
        now_ns: i64,
        skew_expires_at: i64,
        equality_second: i64,
        equality: String,
        upper: String,
        above_upper: String,
        lower: String,
        below_lower: String,
        i64_max: String,
        i64_min: String,
        invalid: String,
    }

    let fresh_fixture = || {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let now_ns = i64::try_from(now.as_nanos()).unwrap();
        let now_secs = now_ns / 1_000_000_000;
        let expires_at = now_secs.checked_add(86_400).unwrap();
        let skew_expires_at = now_secs.checked_sub(60).unwrap();
        let token = auth_test_jwt(&format!(
            r#"{{"aud":"gateway","iss":"clock","exp":{expires_at}}}"#
        ));
        let skew_token = auth_test_jwt(&format!(
            r#"{{"aud":"gateway","iss":"clock","exp":{skew_expires_at}}}"#
        ));
        let mut invalid = token.clone();
        let last = invalid.pop().unwrap();
        invalid.push(if last == 'A' { 'B' } else { 'A' });
        AuthClockFixture {
            token,
            skew_token,
            now_ns,
            skew_expires_at,
            equality_second: now_secs,
            equality: auth_test_jwt(&format!(
                r#"{{"aud":"gateway","iss":"clock","exp":{now_secs}}}"#
            )),
            upper: auth_test_jwt(
                r#"{"aud":"gateway","iss":"clock","exp":9223372036854775}"#,
            ),
            above_upper: auth_test_jwt(
                r#"{"aud":"gateway","iss":"clock","exp":9223372036854775808}"#,
            ),
            lower: auth_test_jwt(
                r#"{"aud":"gateway","iss":"clock","exp":-9223372036854775}"#,
            ),
            below_lower: auth_test_jwt(
                r#"{"aud":"gateway","iss":"clock","exp":-9223372036854776}"#,
            ),
            i64_max: auth_test_jwt(
                r#"{"aud":"gateway","iss":"clock","exp":9223372036854775807}"#,
            ),
            i64_min: auth_test_jwt(
                r#"{"aud":"gateway","iss":"clock","exp":-9223372036854775808}"#,
            ),
            invalid,
        }
    };
    let source_for = |fixture: &AuthClockFixture| {
        r#"
use core.auth as auth

fn check(token: String, key: [U8], label: String, skew: Duration) {
    if auth.verify_jwt(token, key: key, audience: "gateway", issuer: "clock", clock_skew: skew) == {
        .Ok(_) -> { print("{label}-accepted") }
        .Err(error) -> {
            if error == {
                .TokenExpired -> { print("{label}-expired") }
                .MalformedToken(_) -> { print("{label}-malformed") }
                .InvalidSignature -> { print("{label}-invalid") }
                else -> { print("{label}-wrong-error") }
            }
        }
    }
}

fn run() {
    key :: [U8].{ 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102 }
    skew_token := "@SKEW_TOKEN"
    skew_expiry :: @SKEW_EXPIRY
    base_ns :: @NOW_NS - skew_expiry * 1000000000
    boundary_ns :: base_ns
    next_boundary_ns :: base_ns + 1
    boundary :: Duration.nanoseconds(boundary_ns) ?? panic("boundary")
    next_boundary :: Duration.nanoseconds(next_boundary_ns) ?? panic("next boundary")
    boundary_read :: boundary.in(.Nanoseconds) ?? panic("boundary read")
    next_boundary_read :: next_boundary.in(.Nanoseconds) ?? panic("next boundary read")
    if boundary_read == boundary_ns { print("skew-boundary-exact") } else { print("skew-boundary-truncated") }
    if next_boundary_read == next_boundary_ns { print("skew-boundary-plus-one-exact") } else { print("skew-boundary-plus-one-truncated") }
    margin_ns :: 2000000000000
    expired_skew :: Duration.nanoseconds(base_ns - margin_ns) ?? panic("expired skew")
    accepted_skew :: Duration.nanoseconds(base_ns + margin_ns) ?? panic("accepted skew")
    check(skew_token, key, "skew-margin-before", expired_skew)
    check(skew_token, key, "skew-margin-after", accepted_skew)
    token := "@TOKEN"
    zero :: Duration.milliseconds(0) ?? panic("zero")
    check(token, key, "future", zero)
    check("@EQUALITY", key, "equality", zero)
    check("@UPPER", key, "upper", zero)
    check("@ABOVE_UPPER", key, "above-upper", zero)
    check("@LOWER", key, "lower", zero)
    check("@BELOW_LOWER", key, "below-lower", zero)
    check("@I64_MAX", key, "i64-max", zero)
    check("@I64_MIN", key, "i64-min", zero)
    check("@INVALID", key, "invalid", zero)
    max_skew :: Duration.nanoseconds(9223372036854775807) ?? panic("max skew")
    min_skew :: Duration.nanoseconds(-9223372036854775807 - 1) ?? panic("min skew")
    check(token, key, "max-skew", max_skew)
    check(token, key, "min-skew", min_skew)
}
"#
        .replace("@TOKEN", &fixture.token)
        .replace("@SKEW_TOKEN", &fixture.skew_token)
        .replace("@NOW_NS", &fixture.now_ns.to_string())
        .replace("@SKEW_EXPIRY", &fixture.skew_expires_at.to_string())
        .replace("@EQUALITY", &fixture.equality)
        .replace("@UPPER", &fixture.upper)
        .replace("@ABOVE_UPPER", &fixture.above_upper)
        .replace("@LOWER", &fixture.lower)
        .replace("@BELOW_LOWER", &fixture.below_lower)
        .replace("@I64_MAX", &fixture.i64_max)
        .replace("@I64_MIN", &fixture.i64_min)
        .replace("@INVALID", &fixture.invalid)
    };

    let aot_fixture = fresh_fixture();
    let src = source_for(&aot_fixture);
    let shown = dir.join("clock.jet");
    fs::write(&shown, &src).unwrap();
    let compiled = jet::compile_with_path(&src, shown.to_str().unwrap()).unwrap();
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

    let wait_until_unix_second_after = |second: i64| {
        loop {
            let now_secs = i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )
            .unwrap();
            if now_secs > second {
                break;
            }
            std::thread::yield_now();
        }
    };
    wait_until_unix_second_after(aot_fixture.equality_second);
    let run = Command::new(&bin).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let aot_stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    let expected = "skew-boundary-exact\nskew-boundary-plus-one-exact\nskew-margin-before-expired\nskew-margin-after-accepted\nfuture-accepted\nequality-expired\nupper-accepted\nabove-upper-malformed\nlower-expired\nbelow-lower-expired\ni64-max-accepted\ni64-min-expired\ninvalid-invalid\nmax-skew-accepted\nmin-skew-expired\n";
    assert_eq!(aot_stdout, expected);

    let interpreter_fixture = fresh_fixture();
    let interpreter_src = source_for(&interpreter_fixture);
    fs::write(&shown, &interpreter_src).unwrap();
    let shown_text = shown.to_string_lossy().into_owned();
    wait_until_unix_second_after(interpreter_fixture.equality_second);
    jet_jit::reset_jit_trace_for_test();
    let interpreted = match jet::Interpreter::dev_iteration(&shown_text, false, true) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!(stderr, "");
            assert_eq!(exit_code, 0);
            stdout
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("clock forced interpreter failed: {diags:?}")
        }
    };
    assert!(!jet_jit::jit_executed_for_test());
    assert!(!jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test());
    assert_eq!(interpreted, expected);

    let resident_fixture = fresh_fixture();
    let resident_src = source_for(&resident_fixture);
    fs::write(&shown, &resident_src).unwrap();
    let shown_text = shown.to_string_lossy().into_owned();
    wait_until_unix_second_after(resident_fixture.equality_second);
    jet_jit::reset_jit_trace_for_test();
    let resident = match jet::Interpreter::dev_iteration(&shown_text, false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!(stderr, "");
            assert_eq!(exit_code, 0);
            stdout
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("clock resident JIT failed: {diags:?}")
        }
    };
    assert!(jet_jit::jit_executed_for_test(), "clock auth must execute resident JIT");
    assert!(!jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(), "clock auth must not deopt or fall back");
    assert_eq!(resident, expected);
    assert_eq!(resident, interpreted);
    assert_eq!(resident, aot_stdout);
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
