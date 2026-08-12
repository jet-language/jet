// Host bindings for the compiled copy of `Prelude/Scheduler.rs`.
//
// I9: there is one scheduler. `Prelude/Scheduler.rs` is the scheduler; AOT
// embeds that source into the generated program and `jet_codegen::scheduler`
// compiles the same source for the Cranelift JIT and the interpreter's ambient
// host. Nothing in this file re-encodes scheduling policy, defaults, or error
// meaning. It only supplies the sibling-prelude symbols that the emitted
// program gets from its own flat module, plus the marshalling the JIT needs.

// ---------------------------------------------------------------------------
// Panic boundary. AOT source: Prelude/Core.rs.
// The scheduler asks "may I unwind out of this frame?" before it turns a
// cancel or a deadline into a panic. In an emitted program the answer also
// counts `#Interrupt` handler frames; the JIT host has no interrupt-handler
// prelude, so a scheduler task frame is the whole answer here.
// ---------------------------------------------------------------------------

thread_local! {
    static JET_IN_SCHEDULER_TASK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Enter a frame that a scheduler interrupt may unwind out of.
pub fn jet_scheduler_task_panic_enter() {
    JET_IN_SCHEDULER_TASK.with(|c| c.set(true));
}

/// Leave that frame.
pub fn jet_scheduler_task_panic_leave() {
    JET_IN_SCHEDULER_TASK.with(|c| c.set(false));
}

/// Whether the current JIT frame belongs to a scheduler task. The JIT uses
/// this only to keep a task's runtime trap local until its join boundary has
/// converted it into a `TaskFailure`; a sibling must not observe that trap at
/// its next loop header.
pub fn jet_scheduler_in_task() -> bool {
    JET_IN_SCHEDULER_TASK.with(|c| c.get())
}

pub fn jet_scheduler_panic_should_unwind() -> bool {
    JET_IN_SCHEDULER_TASK.with(|c| c.get())
}

fn jet_runtime_diagnostic(rendered: String) -> ! {
    eprintln!("{rendered}");
    std::process::exit(70);
}

// ---------------------------------------------------------------------------
// `#Para` deferred failure. AOT source: Prelude/Core.rs.
// Inside a `#Para` region a failure is carried to the collection point instead
// of raised in the worker. The JIT host has no `#Para` runtime, so the flag is
// never set here — but the one scheduler source keeps its real carrier path.
// ---------------------------------------------------------------------------

thread_local! {
    static JET_PARA_DEFER_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

enum JetParaRuntimeFailure {
    SchedulerFatal { msg: String },
}

// ---------------------------------------------------------------------------
// Task deadline. Clock, budget, and JetDeadlineGuard: one home in
// Prelude/Deadline.rs (card #1747), included above by `lib.rs`'s `scheduler`
// module and by the AOT emission list in `Codegen/mod.rs`, so the two cannot
// drift. The E3003 text comes from the one renderer in Prelude/TaskGroup.rs.
// ---------------------------------------------------------------------------

#[cfg(test)]
thread_local! {
    static TEST_DEADLINE_EXCEEDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn jet_deadline_remaining_ms() -> Option<i64> {
    if TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.get()) {
        return Some(0);
    }
    jet_ctx_deadline_ms().map(|d| d.saturating_sub(jet_std_time_now()))
}

#[cfg(not(test))]
fn jet_deadline_remaining_ms() -> Option<i64> {
    jet_ctx_deadline_ms().map(|d| d.saturating_sub(jet_std_time_now()))
}

fn jet_deadline_exceeded(wait_kind: &str) -> ! {
    let rendered = jet_std::jet_task_deadline(wait_kind).render();
    jet_std::jet_task_deadline_mark_pending();
    if jet_scheduler_panic_should_unwind()
        || jet_scheduler_wait_boundary_should_unwind()
        || jet_typed_deadline_boundary_should_unwind()
    {
        std::panic::panic_any(JetDeadlineUnwind { rendered });
    }
    jet_runtime_diagnostic(rendered);
}

// ---------------------------------------------------------------------------
// JIT marshalling. No counterpart in the emitted prelude, because AOT reaches
// the same scheduler entry points through generated code instead.
// ---------------------------------------------------------------------------

/// JIT marshalling for the one generic Prelude select door. Cranelift values
/// happen to use `i64` slots, including opaque String handles; that ABI choice
/// does not narrow the `Receiver<T>` language contract.
pub fn jet_scheduler_select_int_channels_timed<T: Send>(
    channels: &[JetSchedulerChannel<T>],
    timers: Vec<(u64, T)>,
) -> T {
    let recvs: Vec<_> = channels.iter().map(|c| c.select_inner()).collect();
    jet_scheduler_select_values(
        recvs,
        timers
            .into_iter()
            .map(|(ms, value)| (ms, Some(value)))
            .collect(),
    )
}

#[cfg(test)]
mod scheduler_host_tests {
    use super::*;

    #[test]
    fn the_jit_scheduler_is_the_prelude_scheduler() {
        // Anti-refork guard. The JIT host must compile Prelude/Scheduler.rs
        // itself, never a second copy under src/scheduler*.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(
            !root.join("src/scheduler.rs").exists(),
            "src/scheduler.rs is a second scheduler; the JIT compiles Prelude/Scheduler.rs"
        );
        assert!(
            !root.join("src/scheduler").exists(),
            "src/scheduler/ is a second scheduler; the JIT compiles Prelude/Scheduler.rs"
        );
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
        assert!(
            lib.contains("include!(\"Prelude/Scheduler.rs\")"),
            "the scheduler module must include the one Prelude scheduler source"
        );
    }

    #[test]
    fn stream_pull_releases_exactly_one_yield_at_a_time() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let (sender, mut consumer) = jet_stream::<i64>();
        let producer_events = Arc::clone(&events);
        let producer = std::thread::spawn(move || {
            producer_events.lock().unwrap().push("before-1");
            assert!(sender.send_stream(1));
            producer_events.lock().unwrap().push("after-1");
            assert!(!sender.send_stream(2));
            producer_events.lock().unwrap().push("after-2");
        });

        assert_eq!(consumer.pull(), Some(1));
        assert_eq!(*events.lock().unwrap(), vec!["before-1"]);
        assert_eq!(consumer.pull(), Some(2));
        assert_eq!(*events.lock().unwrap(), vec!["before-1", "after-1"]);

        drop(consumer);
        producer.join().unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            vec!["before-1", "after-1", "after-2"]
        );
    }

    #[test]
    fn stream_sender_observes_explicit_consumer_close() {
        let (sender, mut consumer) = jet_stream::<i64>();
        let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
        let producer = std::thread::spawn(move || {
            assert!(!sender.send_stream(1));
            done_tx.send(()).unwrap();
        });

        assert_eq!(consumer.pull(), Some(1));
        drop(consumer);
        done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("closed Stream did not release its producer");
        producer.join().unwrap();
    }

    #[test]
    fn stream_producer_failure_still_completes_consumer() {
        let (sender, mut consumer) = jet_stream::<i64>();
        let producer = std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                assert!(sender.send_stream(1));
                panic!("producer failure");
            }));
            if result.is_err() {
                sender.fail();
            }
        });

        assert_eq!(consumer.pull(), Some(1));
        assert_eq!(consumer.pull(), None);
        assert!(consumer.failed());
        drop(consumer);
        producer.join().unwrap();
    }

    #[test]
    fn spawned_deadline_keeps_its_rendered_diagnostic() {
        let join = jet_scheduler_spawn(|| -> i64 {
            std::panic::panic_any(JetDeadlineUnwind {
                rendered: "deadline detail".to_string(),
            })
        });
        match join.rx.recv_timeout(Duration::from_secs(1)).unwrap() {
            JetSchedulerResult::Deadline(rendered) => {
                assert_eq!(rendered, "deadline detail");
            }
            _ => unreachable!("spawned deadline must stay distinct from a task panic"),
        }
    }

    #[test]
    fn zero_capacity_channel_clamps_to_a_buffer_of_one() {
        // I9: `tasks.channel<T>(0)` must mean the same thing under the JIT as
        // under the shipped AOT prelude, whose `bounded()` clamps with
        // `capacity.max(1)`. Capacity is a memory/backpressure bound, not a
        // rendezvous handshake; no zero-capacity semantics are ratified.
        let channel = JetSchedulerChannel::bounded(0);
        let sender = channel.sender();
        let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            assert!(sender.send(7));
            done_tx.send(()).unwrap();
        });
        // A capacity-1 buffer accepts the first send without a receiver.
        done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(channel.receive(), Some(7));
        worker.join().unwrap();
    }

    fn ready_entries_in_reverse_completion_order(
    ) -> Vec<(JetSchedulerJoin<i64>, Arc<JetTaskControl>)> {
        let (slow_tx, slow_rx) = std::sync::mpsc::sync_channel(1);
        let (fast_tx, fast_rx) = std::sync::mpsc::sync_channel(1);
        fast_tx.send(JetSchedulerResult::Value(42)).unwrap();
        slow_tx.send(JetSchedulerResult::Value(7)).unwrap();

        let fast_order = Arc::new(OnceLock::new());
        fast_order.set(0).unwrap();
        let slow_order = Arc::new(OnceLock::new());
        slow_order.set(1).unwrap();

        vec![
            (
                JetSchedulerJoin {
                    rx: slow_rx,
                    completion_order: slow_order,
                    completion_wait: ParkSlot::new(),
                },
                JetTaskControl::new(),
            ),
            (
                JetSchedulerJoin {
                    rx: fast_rx,
                    completion_order: fast_order,
                    completion_wait: ParkSlot::new(),
                },
                JetTaskControl::new(),
            ),
        ]
    }

    #[test]
    fn race_uses_completion_order_when_results_are_already_ready() {
        assert_eq!(
            jet_scheduler_race(ready_entries_in_reverse_completion_order()).unwrap(),
            42
        );
    }

    #[test]
    fn any_uses_completion_order_when_results_are_already_ready() {
        assert_eq!(
            jet_scheduler_any(ready_entries_in_reverse_completion_order()).unwrap(),
            42
        );
    }

    #[test]
    fn timed_select_returns_the_timer_arm_value() {
        let channel = JetSchedulerChannel::<i64>::new();
        assert_eq!(
            jet_scheduler_select_int_channels_timed(&[channel], vec![(0, 99)]),
            99
        );
    }

    #[test]
    fn timed_select_preserves_a_non_int_payload() {
        let channel = JetSchedulerChannel::<String>::new();
        assert_eq!(
            jet_scheduler_select_int_channels_timed(&[channel], vec![(0, "generic".into())]),
            "generic"
        );
    }

    // Tower #126 scale guard. Drives `n` tasks against a capacity-1 channel so
    // every send hits backpressure and must PARK until the receiver drains it.
    // Deterministic (no rustc, no wall-clock thresholds) and it fails on the three
    // audited failure modes:
    //   * lost wake / deadlock  → the watchdog trips instead of hanging forever,
    //   * busy-wait             → zero real condvar blocks recorded,
    //   * waiter leak           → send/recv waiter vectors are not drained.
    fn run_backpressure_scale(n: i64) {
        let handle = std::thread::spawn(move || {
            let before = jet_scheduler_metric_park_blocks();
            let channel = JetSchedulerChannel::<i64>::bounded(1);
            for _ in 0..n {
                let sender = channel.sender();
                let _ = jet_scheduler_spawn(move || {
                    sender.send(1);
                });
            }
            let mut total = 0i64;
            for _ in 0..n {
                total += channel
                    .receive()
                    .expect("channel closed before all sends drained");
            }
            jet_scheduler_drain();
            let blocks = jet_scheduler_metric_park_blocks().saturating_sub(before);
            let inner = channel.select_inner();
            let st = inner.state.lock().unwrap();
            (total, blocks, st.send_waiters.len(), st.recv_waiters.len())
        });

        let start = Instant::now();
        let budget = Duration::from_secs(120);
        while !handle.is_finished() {
            assert!(
                start.elapsed() < budget,
                "scale workload hung: a park never woke (lost-wake / deadlock)"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let (total, blocks, send_leak, recv_leak) = handle.join().expect("scale worker panicked");

        assert_eq!(total, n, "every task's message must be delivered exactly once");
        assert!(
            blocks > 0,
            "no task ever blocked on a park condvar under capacity-1 backpressure — \
             the scheduler is busy-waiting, not parking"
        );
        assert_eq!(
            send_leak, 0,
            "send waiters leaked after drain (unbounded growth)"
        );
        assert_eq!(
            recv_leak, 0,
            "recv waiters leaked after drain (unbounded growth)"
        );
    }

    #[test]
    fn scale_10k_tasks_park_under_backpressure() {
        run_backpressure_scale(10_000);
    }

    #[test]
    #[ignore = "local 100k parked-task scale proof; run with --ignored"]
    fn scale_100k_tasks_park_under_backpressure() {
        run_backpressure_scale(100_000);
    }

    // Tower #126: prove pause/cancel are real control over a *running* task —
    // they actually park/unblock it at its wait point, not merely flip a flag a
    // `trace()` can read.

    #[test]
    fn pause_holds_a_running_task_at_its_wait_point_until_resume() {
        use std::sync::atomic::AtomicUsize;
        let control = JetTaskControl::new();
        let ready = JetSchedulerChannel::<i64>::new();
        let ready_tx = ready.sender();
        let work = JetSchedulerChannel::<i64>::new();
        let work_tx = work.sender();
        let progressed = Arc::new(AtomicUsize::new(0));

        let task_ready = ready_tx;
        let task_work = work;
        let task_progressed = progressed.clone();
        let _join = jet_scheduler_spawn_with_control(
            move || {
                task_ready.send(1);
                // Parks here until a value arrives AND the task is not paused.
                let _ = task_work.receive();
                task_progressed.fetch_add(1, Ordering::SeqCst);
            },
            control.clone(),
        );

        // Task has reached the wait point.
        assert_eq!(ready.receive(), Some(1));
        control.pause();
        // Make the value available: a flag-only "pause" would let the task run.
        work_tx.send(42);
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(
            progressed.load(Ordering::SeqCst),
            0,
            "paused task consumed the value and ran past its wait point — pause is not real"
        );

        control.resume();
        let start = Instant::now();
        while progressed.load(Ordering::SeqCst) == 0 {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "resumed task never progressed"
            );
            std::thread::yield_now();
        }
        jet_scheduler_drain();
    }

    // D-CANCELMODEL1=C: cancel is PREEMPTIVE — a cancelled parked task unwinds at
    // its wait point, runs Drop-backed cleanup, never runs the code after the wait,
    // and its result becomes Cancelled (not a delivered value).
    #[test]
    fn cancel_unwinds_a_parked_task_runs_drop_and_reports_cancelled() {
        use std::sync::atomic::AtomicUsize;

        // 0 = untouched, 1 = ran past the wait (BUG), 2 = Drop ran during unwind.
        struct DropMark(Arc<AtomicUsize>);
        impl Drop for DropMark {
            fn drop(&mut self) {
                // Only record the unwind cleanup; a normal return sets 1 first.
                let _ = self
                    .0
                    .compare_exchange(0, 2, Ordering::SeqCst, Ordering::SeqCst);
            }
        }

        let control = JetTaskControl::new();
        let ready = JetSchedulerChannel::<i64>::new();
        let ready_tx = ready.sender();
        // Nothing is ever sent on `work`: only cancellation can free the task.
        let work = JetSchedulerChannel::<i64>::new();
        let outcome = Arc::new(AtomicUsize::new(0));

        let task_ready = ready_tx;
        let task_work = work;
        let task_outcome = outcome.clone();
        let join = jet_scheduler_spawn_with_control(
            move || {
                let _mark = DropMark(task_outcome.clone());
                task_ready.send(1);
                // Cancel unwinds HERE; the store below must never run.
                let _got = task_work.receive();
                task_outcome.store(1, Ordering::SeqCst);
                0i64
            },
            control.clone(),
        );

        assert_eq!(ready.receive(), Some(1));
        // Task is now parked forever unless cancel actually unwinds it.
        control.cancel();
        let start = Instant::now();
        loop {
            match join.try_recv() {
                Some(JetSchedulerResult::Cancelled) => break,
                Some(other) => panic!(
                    "cancelled task must report Cancelled, got {}",
                    match other {
                        JetSchedulerResult::Value(_) => "Value",
                        JetSchedulerResult::Panicked(_) => "Panicked",
                        JetSchedulerResult::Cancelled => unreachable!(),
                        JetSchedulerResult::Deadline(_) => "Deadline",
                    }
                ),
                None => {
                    assert!(
                        start.elapsed() < Duration::from_secs(5),
                        "cancel did not unwind the parked task"
                    );
                    std::thread::yield_now();
                }
            }
        }
        assert_eq!(
            outcome.load(Ordering::SeqCst),
            2,
            "unwind must run Drop cleanup and skip the code after the wait point"
        );
        jet_scheduler_drain();
    }

    // D-CANCELMODEL1=C shield: a cancel that arrives while a shielded region runs
    // is DEFERRED — wait points inside complete normally, and the unwind lands only
    // when the region exits. Runtime machinery is syntax-free until D-SHIELDNAME1.
    #[test]
    fn shielded_region_defers_cancel_until_it_exits() {
        use std::sync::atomic::AtomicUsize;
        let control = JetTaskControl::new();
        let ready = JetSchedulerChannel::<i64>::new();
        let ready_tx = ready.sender();
        // A value IS delivered so the shielded recv can complete despite the cancel.
        let work = JetSchedulerChannel::<i64>::new();
        let work_tx = work.sender();
        // 0 none, bit1 = shielded recv completed, then unwind => Cancelled result.
        let stage = Arc::new(AtomicUsize::new(0));

        let task_ready = ready_tx;
        let task_work = work;
        let task_stage = stage.clone();
        let join = jet_scheduler_spawn_with_control(
            move || {
                task_ready.send(1);
                jet_scheduler_shield_enter();
                // Wait point INSIDE the shield: must NOT unwind on the pending cancel.
                let got = task_work.receive();
                if got == Some(42) {
                    task_stage.store(1, Ordering::SeqCst);
                }
                jet_scheduler_shield_leave(); // pending cancel unwinds HERE
                task_stage.store(9, Ordering::SeqCst); // must never run
                0i64
            },
            control.clone(),
        );

        assert_eq!(ready.receive(), Some(1));
        // Cancel while the task is parked inside the shield.
        control.cancel();
        // Give cancel a moment; the shielded recv must still be waiting for its value.
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(
            stage.load(Ordering::SeqCst),
            0,
            "shielded recv completed or unwound before its value arrived"
        );
        work_tx.send(42); // completes the shielded recv
        let start = Instant::now();
        loop {
            match join.try_recv() {
                Some(JetSchedulerResult::Cancelled) => break,
                Some(_) => panic!("shielded task must end Cancelled after the region"),
                None => {
                    assert!(
                        start.elapsed() < Duration::from_secs(5),
                        "deferred cancel never landed at shield exit"
                    );
                    std::thread::yield_now();
                }
            }
        }
        assert_eq!(
            stage.load(Ordering::SeqCst),
            1,
            "shielded recv must complete (stage 1) and the post-shield code must not run"
        );
        jet_scheduler_drain();
    }

    // D-CANCELMODEL1=C shield/deadline interaction: a deadline that closes while
    // shielded is likewise deferred to region exit (E3003 unwind), staying
    // consistent with the cancel case.
    #[test]
    fn shield_defers_deadline_until_it_exits() {
        jet_scheduler_set_task_control(Some(JetTaskControl::new()));
        jet_scheduler_task_panic_enter();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            jet_scheduler_shield_enter();
            // Deadline is exceeded, but we are shielded: no unwind here.
            TEST_DEADLINE_EXCEEDED.with(|d| d.set(true));
            let slot = ParkSlot::new();
            slot.wake();
            jet_scheduler_yield("shielded wait", &slot, Some(Duration::from_millis(1)));
            // Reaching here proves the shielded wait did not unwind on the deadline.
            jet_scheduler_shield_leave(); // deadline unwinds HERE
            "no-unwind"
        }));
        TEST_DEADLINE_EXCEEDED.with(|d| d.set(false));
        jet_scheduler_set_task_control(None);
        jet_scheduler_task_panic_leave();
        assert!(
            result.is_err(),
            "deadline deferred by the shield must unwind when the region exits"
        );
    }

    #[test]
    fn non_unwind_wait_boundary_returns_typed_deadline_for_yield() {
        jet_scheduler_set_task_control(Some(JetTaskControl::new()));
        jet_scheduler_task_panic_enter();
        TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.set(true));
        let result = jet_scheduler_wait_without_unwind(jet_scheduler_yield_now);
        TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.set(false));
        jet_scheduler_set_task_control(None);
        jet_scheduler_task_panic_leave();
        match result {
            JetSchedulerWait::Deadline(rendered) => {
                assert_eq!(rendered, jet_std::jet_task_deadline("task yield").render());
            }
            _ => panic!("yield deadline must cross native boundary as typed status"),
        }
    }

    // Exercise the exact RAII shape emitted by Codegen/TIR/emit/statements.rs.
    // These helpers deliberately do not call `_leave` from test bodies: Drop is
    // what must cover every control-flow and unwind edge.
    struct EmittedShieldGuard<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for EmittedShieldGuard<F> {
        fn drop(&mut self) {
            if let Some(f) = self.0.take() {
                f();
            }
        }
    }

    macro_rules! emitted_shield {
        ($body:block) => {{
            jet_scheduler_shield_enter();
            let _shield_guard = EmittedShieldGuard(Some(|| jet_scheduler_shield_leave()));
            $body
        }};
    }

    fn emitted_early_return() -> i64 {
        emitted_shield!({ return 17 });
    }

    fn emitted_try_exit() -> Result<i64, &'static str> {
        emitted_shield!({ Err("stop")? });
        Ok(1)
    }

    #[test]
    fn emitted_shield_guard_covers_control_flow_unwind_and_reset_matrix() {
        // Outside a task/catch frame, even an expired ambient deadline is inert.
        TEST_DEADLINE_EXCEEDED.with(|d| d.set(true));
        emitted_shield!({ assert!(!jet_scheduler_shielded()) });
        TEST_DEADLINE_EXCEEDED.with(|d| d.set(false));

        jet_scheduler_task_panic_enter();
        jet_scheduler_set_task_control(Some(JetTaskControl::new()));

        emitted_shield!({
            assert!(jet_scheduler_shielded());
            emitted_shield!({ assert!(jet_scheduler_shielded()) });
            assert!(jet_scheduler_shielded());
        });
        assert!(!jet_scheduler_shielded(), "nested guards must balance depth");
        assert_eq!(emitted_early_return(), 17);
        assert!(!jet_scheduler_shielded(), "return must drop the guard");
        assert_eq!(emitted_try_exit(), Err("stop"));
        assert!(!jet_scheduler_shielded(), "? must drop the guard");

        // A body panic wins over pending cancel/deadline: guard decrements depth
        // but must not begin a second panic while unwinding.
        for pending_deadline in [false, true] {
            let control = JetTaskControl::new();
            control.cancel();
            jet_scheduler_set_task_control(Some(control));
            TEST_DEADLINE_EXCEEDED.with(|d| d.set(pending_deadline));
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                emitted_shield!({ panic!("body panic") });
            }));
            let text = panic
                .expect_err("body must panic")
                .downcast::<&'static str>()
                .map(|s| *s)
                .unwrap_or("");
            assert_eq!(text, "body panic");
            assert!(!jet_scheduler_shielded(), "panic must reset shield depth");
            TEST_DEADLINE_EXCEEDED.with(|d| d.set(false));
        }

        // When both become pending during a normal body, deadline has priority.
        let control = JetTaskControl::new();
        control.cancel();
        jet_scheduler_set_task_control(Some(control));
        let both = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            emitted_shield!({ TEST_DEADLINE_EXCEEDED.with(|d| d.set(true)) });
        }));
        let payload = both.expect_err("pending deadline must land at guard drop");
        let text = payload
            .downcast_ref::<JetDeadlineUnwind>()
            .map(|deadline| deadline.rendered.as_str())
            .unwrap_or("");
        assert_eq!(text, jet_std::jet_task_deadline("shield exit").render());
        assert!(!jet_scheduler_shielded());
        TEST_DEADLINE_EXCEEDED.with(|d| d.set(false));

        // Same worker/thread can run a later task with clean depth and control.
        jet_scheduler_set_task_control(Some(JetTaskControl::new()));
        emitted_shield!({ assert!(jet_scheduler_shielded()) });
        assert!(!jet_scheduler_shielded(), "subsequent task must start clean");
        jet_scheduler_set_task_control(None);
        jet_scheduler_task_panic_leave();
    }
}
