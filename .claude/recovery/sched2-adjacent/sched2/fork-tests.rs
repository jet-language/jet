mod stream;
pub use stream::{
    jet_stream, JetStream, JetStreamCompletion, JetStreamIter, JetStreamSender,
};

#[cfg(test)]
mod interrupt_boundary_tests {
    use super::*;

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
        // D-VERDICT-1637-1: `bounded(0)` is a real memory/backpressure bound,
        // never a rendezvous handshake (no ratified zero-capacity semantics
        // exist) — it clamps to 1, matching Prelude/Scheduler.rs.
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
                },
                JetTaskControl::new(),
            ),
            (
                JetSchedulerJoin {
                    rx: fast_rx,
                    completion_order: fast_order,
                },
                JetTaskControl::new(),
            ),
        ]
    }

    #[test]
    fn race_uses_completion_order_when_results_are_already_ready() {
        assert_eq!(
            jet_scheduler_race(ready_entries_in_reverse_completion_order()),
            42
        );
    }

    #[test]
    fn any_uses_completion_order_when_results_are_already_ready() {
        assert_eq!(
            jet_scheduler_any(ready_entries_in_reverse_completion_order()),
            42
        );
    }

    #[test]
    fn scheduler_module_stays_under_the_size_boundary_with_no_io_fork() {
        // #1637: the `scheduler/io.rs` fork of Prelude/Scheduler.rs is deleted
        // (D-VERDICT-1637-1) — no live JIT-host caller ever registered real IO
        // through it (net_http_rt.rs runs its own local poll loop instead).
        // One scheduler substrate remains: this file.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        const MAX_MODULE_LINES: usize = 2500;
        let root_source = std::fs::read_to_string(root.join("src/scheduler.rs"))
            .unwrap_or_else(|error| panic!("failed to read src/scheduler.rs: {error}"));
        assert!(
            root_source.lines().count() < MAX_MODULE_LINES,
            "src/scheduler.rs must stay below the card #510 module boundary"
        );
        assert!(
            !root.join("src/scheduler/io.rs").exists(),
            "the scheduler/io.rs fork must stay deleted"
        );
        let production_root = root_source
            .split("#[cfg(test)]\nmod interrupt_boundary_tests")
            .next()
            .expect("scheduler test boundary");
        assert!(
            !production_root.contains("mod io;"),
            "scheduler root must not re-declare the deleted io fork"
        );
        assert!(
            !production_root.contains("include!("),
            "scheduler split must use normal Rust modules, never include! shells"
        );
    }

    fn select_with_timeout(
        channels: Vec<JetSchedulerChannel<i64>>,
        timers: Vec<u64>,
    ) -> JetSelectOutcome<i64> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let inners = channels.into_iter().map(|ch| ch.select_inner()).collect();
            let _ = tx.send(jet_scheduler_select(inners, timers));
        });
        rx.recv_timeout(Duration::from_millis(250))
            .expect("select did not wake after every channel closed")
    }

    #[test]
    fn closed_select_failure_unwinds_inside_runtime_boundary() {
        jet_scheduler_task_panic_enter();
        let result = std::panic::catch_unwind(|| jet_scheduler_fatal("select closed"));
        jet_scheduler_task_panic_leave();
        assert!(result.is_err());
    }

    #[test]
    fn select_returns_closed_when_one_channel_is_closed_and_empty() {
        let channel = JetSchedulerChannel::<i64>::new();
        channel.close();
        assert!(matches!(
            select_with_timeout(vec![channel], Vec::new()),
            JetSelectOutcome::Closed
        ));
    }

    #[test]
    fn select_returns_closed_only_when_all_channels_are_closed_and_empty() {
        let first = JetSchedulerChannel::<i64>::new();
        let second = JetSchedulerChannel::<i64>::new();
        first.close();
        second.close();
        assert!(matches!(
            select_with_timeout(vec![first, second], Vec::new()),
            JetSelectOutcome::Closed
        ));
    }

    #[test]
    fn select_wakes_when_last_open_channel_closes_after_park() {
        let channel = JetSchedulerChannel::<i64>::new();
        let closer = channel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            closer.close();
        });
        assert!(matches!(
            select_with_timeout(vec![channel], Vec::new()),
            JetSelectOutcome::Closed
        ));
    }

    #[test]
    fn select_ready_value_and_timer_keep_precedence_over_closed() {
        let valued = JetSchedulerChannel::<i64>::new();
        let sender = valued.sender();
        assert!(sender.send(7));
        drop(sender);
        assert!(matches!(
            select_with_timeout(vec![valued], Vec::new()),
            JetSelectOutcome::Recv { value: 7, .. }
        ));

        let closed = JetSchedulerChannel::<i64>::new();
        closed.close();
        assert!(matches!(
            select_with_timeout(vec![closed], vec![0]),
            JetSelectOutcome::After { arm: 0 }
        ));

        let closed = JetSchedulerChannel::<i64>::new();
        closed.close();
        assert!(matches!(
            select_with_timeout(vec![closed], vec![10]),
            JetSelectOutcome::After { arm: 0 }
        ));
    }

    #[test]
    fn select_cancellation_keeps_precedence_over_waiting() {
        let control = JetTaskControl::new();
        control.cancel();
        jet_scheduler_set_task_control(Some(control));
        let channel = JetSchedulerChannel::<i64>::new();
        let outcome = jet_scheduler_select(vec![channel.select_inner()], Vec::new());
        jet_scheduler_set_task_control(None);
        assert!(matches!(outcome, JetSelectOutcome::Closed));
    }

    #[test]
    fn select_cancellation_after_park_wakes_and_cleans_waiters() {
        let control = JetTaskControl::new();
        let channel = JetSchedulerChannel::<i64>::new();
        let inner = channel.select_inner();
        let selected_inner = inner.clone();
        let selected_control = control.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            jet_scheduler_set_task_control(Some(selected_control));
            let outcome = jet_scheduler_select(vec![selected_inner], Vec::new());
            jet_scheduler_set_task_control(None);
            let _ = tx.send(outcome);
        });

        let deadline = Instant::now() + Duration::from_millis(250);
        loop {
            let channel_registered = !inner.state.lock().unwrap().recv_waiters.is_empty();
            let cancel_registered = !control.cancel_waiters.lock().unwrap().is_empty();
            if channel_registered && cancel_registered {
                break;
            }
            assert!(Instant::now() < deadline, "select did not park");
            std::thread::yield_now();
        }

        control.cancel();
        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(250))
                .expect("cancelled select did not wake"),
            JetSelectOutcome::Closed
        ));
        assert!(inner.state.lock().unwrap().recv_waiters.is_empty());
        assert!(control.cancel_waiters.lock().unwrap().is_empty());
    }

    #[test]
    fn select_deadline_unwind_cleans_all_waiters() {
        let control = JetTaskControl::new();
        let channel = JetSchedulerChannel::<i64>::new();
        let inner = channel.select_inner();
        jet_scheduler_set_task_control(Some(control.clone()));
        jet_scheduler_task_panic_enter();
        let result = {
            TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.set(true));
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                jet_scheduler_select(vec![inner.clone()], Vec::new())
            }))
        };
        TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.set(false));
        jet_scheduler_task_panic_leave();
        jet_scheduler_set_task_control(None);

        assert!(result.is_err());
        assert!(inner.state.lock().unwrap().recv_waiters.is_empty());
        assert!(control.cancel_waiters.lock().unwrap().is_empty());
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
                total += channel.receive().expect("channel closed before all sends drained");
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
        assert_eq!(send_leak, 0, "send waiters leaked after drain (unbounded growth)");
        assert_eq!(recv_leak, 0, "recv waiters leaked after drain (unbounded growth)");
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
                let _ = self.0.compare_exchange(
                    0,
                    2,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
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
                        JetSchedulerResult::Panicked => "Panicked",
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
                assert_eq!(
                    rendered,
                    "Error [E3003]: deadline exceeded while waiting in task yield\n\
Why: this wait point observed the task context deadline from `#Context(deadline: …)`\n\
Fix: raise the deadline budget or shorten the work before this wait point"
                );
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
        assert_eq!(
            text,
            "Error [E3003]: deadline exceeded while waiting in shield exit\n\
Why: this wait point observed the task context deadline from `#Context(deadline: …)`\n\
Fix: raise the deadline budget or shorten the work before this wait point"
        );
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
